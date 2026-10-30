use super::thread_history::{
    completed_item, event_body, is_completed_item, item_preview, message_context,
    read_bounded_line, RolloutEnvelope,
};
use super::thread_history_body::{item_digest, open_catalog, source_path};
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;
use std::io::BufReader;
use std::path::Path;

const SCHEMA_VERSION: i64 = 4;
const COMPACTION_PENDING: i64 = 2;

pub(super) fn migrate(conn: &mut Connection, path: &Path) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        bail!("history index was created by a newer application version");
    }
    let legacy: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('thread_items') WHERE name='item_json')",
        [],
        |row| row.get(0),
    )?;
    if legacy {
        migrate_bodies(conn, path)?;
    }
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let mut compact = version == COMPACTION_PENDING;
    if (2..SCHEMA_VERSION).contains(&version) {
        // v4: previews became a strict whitelist projection instead of a deep
        // copy, and per-item chat checkpoints moved to lazy materialization.
        // Rebuild the stored previews from the canonical sources and drop the
        // accumulated checkpoint rows; heads keep every field they consume,
        // so they stay valid without a rebuild.
        rebuild_previews(conn, path)?;
        conn.execute_batch(
            "DROP TABLE IF EXISTS thread_chat_checkpoints_v2;
             DROP TABLE IF EXISTS thread_chat_checkpoint_state_v2;
             DROP TABLE IF EXISTS thread_chat_heads;
             DROP TABLE IF EXISTS thread_chat_checkpoints;
             DROP TABLE IF EXISTS thread_chat_projection_state;",
        )?;
        compact = true;
    }
    if compact {
        tracing::info!("compacting migrated canonical history index");
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA auto_vacuum=INCREMENTAL; VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
    } else if version == 0 {
        conn.execute_batch("PRAGMA auto_vacuum=INCREMENTAL;")?;
    }
    if version < SCHEMA_VERSION {
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

/// Re-derive `preview_json` from the canonical rollout records. A preview is
/// a rebuildable projection and the pre-v4 shape is a readable superset, so a
/// rollout whose source is unavailable keeps its old previews; the failure is
/// logged and never blocks the upgrade.
fn rebuild_previews(conn: &mut Connection, path: &Path) -> Result<()> {
    let root = path
        .parent()
        .ok_or_else(|| anyhow!("history database has no parent"))?;
    let catalog = match open_catalog(root) {
        Ok(catalog) => catalog,
        Err(error) => {
            tracing::warn!("history preview rebuild skipped: {error}");
            return Ok(());
        }
    };
    tracing::info!("rebuilding canonical history previews");
    let mut after = String::new();
    loop {
        let next: Option<(String, u64)> = conn
            .query_row(
                "SELECT rollout_id,next_rollout_byte_offset FROM thread_history_projection_state
             WHERE rollout_id>?1 ORDER BY rollout_id LIMIT 1",
                [&after],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((rollout, boundary)) = next else {
            break;
        };
        after = rollout.clone();
        if let Err(error) = rebuild_rollout_previews(conn, &catalog, root, &rollout, boundary) {
            tracing::warn!("history preview rebuild skipped for {rollout}: {error}");
        }
    }
    Ok(())
}

fn rebuild_rollout_previews(
    conn: &mut Connection,
    catalog: &Connection,
    root: &Path,
    rollout: &str,
    boundary: u64,
) -> Result<()> {
    let path = source_path(catalog, root, rollout)?;
    let mut reader = BufReader::new(super::rollout_compression::open_rollout_stream(path)?);
    let mut offset = 0u64;
    while offset < boundary {
        let mut updates: Vec<(String, String)> = Vec::new();
        let mut batch_bytes = 0usize;
        while offset < boundary && updates.len() < 512 && batch_bytes < 4 * 1024 * 1024 {
            let mut line = Vec::new();
            if !read_bounded_line(&mut reader, &mut line)? || line.is_empty() {
                bail!("cannot rebuild history previews: canonical source is incomplete");
            }
            let length = line.len() as u64;
            if offset.checked_add(length).is_none_or(|end| end > boundary) {
                bail!("cannot rebuild history previews: canonical boundary is invalid");
            }
            if !line.iter().all(u8::is_ascii_whitespace) {
                let envelope: RolloutEnvelope = serde_json::from_slice(&line)?;
                if is_completed_item(&envelope) {
                    let item = completed_item(event_body(&envelope))?;
                    let body = event_body(&envelope);
                    let id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("item_id").and_then(Value::as_str))
                        .or_else(|| body.get("item_id").and_then(Value::as_str));
                    if let Some(id) = id {
                        updates.push((id.to_owned(), item_preview(item).to_string()));
                    }
                }
            }
            offset += length;
            batch_bytes += line.len();
        }
        if updates.is_empty() {
            continue;
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (id, preview) in &updates {
            tx.execute(
                "UPDATE thread_items SET preview_json=?3 WHERE rollout_id=?1 AND item_id=?2",
                params![rollout, id, preview],
            )?;
        }
        tx.commit()?;
    }
    Ok(())
}

fn migrate_bodies(conn: &mut Connection, path: &Path) -> Result<()> {
    let root = path
        .parent()
        .ok_or_else(|| anyhow!("history database has no parent"))?;
    let catalog = open_catalog(root)?;
    tracing::info!("migrating canonical history bodies to verified log locators");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS thread_items_compact (
            rollout_id TEXT NOT NULL,turn_id TEXT,root_turn_id TEXT,item_id TEXT NOT NULL,
            item_type TEXT NOT NULL,created_at_ms INTEGER NOT NULL,rollout_ordinal INTEGER NOT NULL,
            record_byte_offset INTEGER NOT NULL,record_byte_length INTEGER NOT NULL,
            item_bytes INTEGER NOT NULL,item_digest BLOB NOT NULL,message_context INTEGER NOT NULL,
            preview_json TEXT NOT NULL DEFAULT '{}',visible INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY(rollout_id,item_id)
        );",
    )?;
    conn.execute("DELETE FROM thread_items_compact", [])?;
    let mut after = String::new();
    loop {
        let next: Option<(String, u64)> = conn
            .query_row(
                "SELECT rollout_id,next_rollout_byte_offset FROM thread_history_projection_state
             WHERE rollout_id>?1 ORDER BY rollout_id LIMIT 1",
                [&after],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((rollout, boundary)) = next else {
            break;
        };
        after = rollout.clone();
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM thread_items WHERE rollout_id=?1",
            [&rollout],
            |row| row.get(0),
        )?;
        if count == 0 {
            continue;
        }
        let path = source_path(&catalog, root, &rollout)?;
        let mut reader = BufReader::new(super::rollout_compression::open_rollout_stream(path)?);
        let mut offset = 0u64;
        while offset < boundary {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut batch_bytes = 0usize;
            let mut records = 0usize;
            while offset < boundary && records < 512 && batch_bytes < 4 * 1024 * 1024 {
                let mut line = Vec::new();
                if !read_bounded_line(&mut reader, &mut line)? || line.is_empty() {
                    bail!("cannot migrate history index: canonical source is incomplete");
                }
                let length = line.len() as u64;
                if offset.checked_add(length).is_none_or(|end| end > boundary) {
                    bail!("cannot migrate history index: canonical boundary is invalid");
                }
                if !line.iter().all(u8::is_ascii_whitespace) {
                    let envelope: RolloutEnvelope = serde_json::from_slice(&line)?;
                    if is_completed_item(&envelope) {
                        let item = completed_item(event_body(&envelope))?;
                        let body = event_body(&envelope);
                        let id = item
                            .get("id")
                            .and_then(Value::as_str)
                            .or_else(|| item.get("item_id").and_then(Value::as_str))
                            .or_else(|| body.get("item_id").and_then(Value::as_str));
                        if let Some(id) = id {
                            let old: Option<(String, String, u64)> = tx
                                .query_row(
                                    "SELECT item_json,item_type,rollout_ordinal FROM thread_items
                                 WHERE rollout_id=?1 AND item_id=?2",
                                    params![rollout, id],
                                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                                )
                                .optional()?;
                            if let Some((raw, kind, ordinal)) = old {
                                let existing: Value = serde_json::from_str(&raw)?;
                                if existing != *item {
                                    bail!("cannot migrate history index: source and projection disagree");
                                }
                                if ordinal == envelope.ordinal {
                                    let (bytes, digest) = item_digest(item)?;
                                    tx.execute(
                                        "INSERT OR REPLACE INTO thread_items_compact
                                         SELECT rollout_id,turn_id,root_turn_id,item_id,item_type,created_at_ms,
                                            rollout_ordinal,?3,?4,?5,?6,?7,preview_json,visible
                                         FROM thread_items WHERE rollout_id=?1 AND item_id=?2",
                                        params![rollout,id,i64::try_from(offset)?,i64::try_from(length)?,
                                            i64::try_from(bytes)?,digest,message_context(item,&kind)],
                                    )?;
                                }
                            }
                        }
                    }
                }
                offset += length;
                batch_bytes += line.len();
                records += 1;
            }
            tx.commit()?;
        }
    }
    let missing: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM thread_items old LEFT JOIN thread_items_compact new
            ON new.rollout_id=old.rollout_id AND new.item_id=old.item_id
            WHERE new.item_id IS NULL OR new.rollout_ordinal!=old.rollout_ordinal)",
        [],
        |row| row.get(0),
    )?;
    if missing {
        bail!(
            "cannot migrate history index: not all legacy items have a verified canonical source"
        );
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(
        "DROP TABLE thread_items;
         ALTER TABLE thread_items_compact RENAME TO thread_items;
         PRAGMA user_version=2;",
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::thread_history::{ThreadHistoryPageRequest, ThreadHistoryStore};
    use serde_json::json;
    use std::fs;

    fn legacy(root: &Path) -> (std::path::PathBuf, String, Value) {
        fs::create_dir_all(root.join("sessions")).unwrap();
        let item = json!({"item_id":"fixture-item","turn_id":"fixture-turn",
            "kind":"assistant_message","created_at_ms":1,
            "payload":{"role":"assistant","content":format!("{}needle-tail","fixture-text ".repeat(320_000))}});
        let record = json!({"ordinal":0,"type":"item_completed","payload":{
            "turn_id":"fixture-turn","item":item}})
        .to_string()
            + "\n";
        let source = root.join("sessions/fixture.jsonl");
        fs::write(&source, &record).unwrap();
        let catalog = Connection::open(root.join("state_1.sqlite")).unwrap();
        catalog
            .execute_batch(
                "CREATE TABLE canonical_rollouts(rollout_id TEXT PRIMARY KEY,rollout_path TEXT);
            INSERT INTO canonical_rollouts VALUES('fixture-rollout','sessions/fixture.jsonl');",
            )
            .unwrap();
        let path = root.join("thread_history_1.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE thread_items(
            rollout_id TEXT NOT NULL,turn_id TEXT,root_turn_id TEXT,item_id TEXT NOT NULL,
            item_type TEXT NOT NULL,created_at_ms INTEGER NOT NULL,rollout_ordinal INTEGER NOT NULL,
            item_json TEXT NOT NULL,preview_json TEXT NOT NULL,visible INTEGER NOT NULL,
            PRIMARY KEY(rollout_id,item_id));
            CREATE TABLE thread_history_projection_state(rollout_id TEXT PRIMARY KEY,
                next_rollout_byte_offset INTEGER NOT NULL,next_rollout_ordinal INTEGER NOT NULL,
                history_revision INTEGER NOT NULL);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO thread_items VALUES('fixture-rollout','fixture-turn',NULL,
            'fixture-item','assistant_message',1,0,?1,?2,1)",
            params![
                item.to_string(),
                super::super::thread_history::item_preview(&item).to_string()
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO thread_history_projection_state VALUES('fixture-rollout',?1,1,7)",
            [record.len() as i64],
        )
        .unwrap();
        drop(conn);
        (path, record, item)
    }

    #[test]
    fn migration_verifies_bodies_reclaims_disk_and_preserves_full_text() {
        let directory = tempfile::tempdir().unwrap();
        let (path, record, item) = legacy(directory.path());
        let old_bytes = fs::metadata(&path).unwrap().len();
        let store = ThreadHistoryStore::open(&path).unwrap();
        let page = store
            .page_items("fixture-rollout", ThreadHistoryPageRequest::default())
            .unwrap();
        assert_eq!(page.history_revision, 7);
        assert_eq!(page.items[0].item_json, item);
        let conn = Connection::open(&path).unwrap();
        let has_body: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('thread_items') WHERE name='item_json')",[],|row|row.get(0)).unwrap();
        assert!(!has_body);
        assert_eq!(
            conn.query_row("PRAGMA auto_vacuum", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        drop(conn);
        drop(store);
        let new_bytes = fs::metadata(&path).unwrap().len();
        eprintln!("fixture history migration: {old_bytes} -> {new_bytes} bytes");
        assert!(new_bytes * 10 < old_bytes);
        assert_eq!(
            fs::read_to_string(directory.path().join("sessions/fixture.jsonl")).unwrap(),
            record
        );
        let reopened = ThreadHistoryStore::open(path).unwrap();
        assert_eq!(
            reopened
                .read_item("fixture-rollout", "fixture-item")
                .unwrap()
                .item_json,
            item
        );
    }

    #[test]
    fn migration_failure_keeps_legacy_body_and_retries_after_source_restored() {
        let directory = tempfile::tempdir().unwrap();
        let (path, record, item) = legacy(directory.path());
        let source = directory.path().join("sessions/fixture.jsonl");
        fs::write(&source, record.replace("needle-tail", "changed-tail")).unwrap();
        assert!(ThreadHistoryStore::open(&path).is_err());
        let conn = Connection::open(&path).unwrap();
        let raw: String = conn
            .query_row("SELECT item_json FROM thread_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), item);
        drop(conn);
        fs::write(&source, &record).unwrap();
        let store = ThreadHistoryStore::open(&path).unwrap();
        assert_eq!(
            store
                .read_item("fixture-rollout", "fixture-item")
                .unwrap()
                .item_json,
            item
        );
    }

    #[test]
    fn missing_source_never_discards_the_legacy_copy() {
        let directory = tempfile::tempdir().unwrap();
        let (path, _, item) = legacy(directory.path());
        fs::remove_file(directory.path().join("sessions/fixture.jsonl")).unwrap();
        assert!(ThreadHistoryStore::open(&path).is_err());
        let conn = Connection::open(path).unwrap();
        let raw: String = conn
            .query_row("SELECT item_json FROM thread_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), item);
    }

    #[test]
    fn migration_and_body_reads_support_compressed_sources_without_publishing_plain_copy() {
        let directory = tempfile::tempdir().unwrap();
        let (path, record, item) = legacy(directory.path());
        let source = directory.path().join("sessions/fixture.jsonl");
        let compressed = super::super::rollout_compression::compressed_path(&source);
        let encoded = zstd::encode_all(record.as_bytes(), 3).unwrap();
        fs::write(&compressed, &encoded).unwrap();
        fs::remove_file(&source).unwrap();
        let store = ThreadHistoryStore::open(&path).unwrap();
        assert_eq!(
            store
                .read_item("fixture-rollout", "fixture-item")
                .unwrap()
                .item_json,
            item
        );
        assert!(!source.exists());
        assert_eq!(fs::read(compressed).unwrap(), encoded);
    }

    #[test]
    fn post_switch_compaction_is_retryable_and_future_versions_are_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let (path, _, item) = legacy(directory.path());
        let store = ThreadHistoryStore::open(&path).unwrap();
        drop(store);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA user_version=2;").unwrap();
        drop(conn);
        let store = ThreadHistoryStore::open(&path).unwrap();
        assert_eq!(
            store
                .read_item("fixture-rollout", "fixture-item")
                .unwrap()
                .item_json,
            item
        );
        drop(store);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA user_version=99;").unwrap();
        drop(conn);
        assert!(ThreadHistoryStore::open(&path).is_err());
        let conn = Connection::open(path).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            99
        );
    }
}
