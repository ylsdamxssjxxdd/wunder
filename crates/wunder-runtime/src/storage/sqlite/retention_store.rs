use super::agent_runtime_store::SqliteAgentRuntimeStorage;
use super::SqliteStorage;
use crate::storage::{
    OrphanSweepResult, StorageLifecycle, USAGE_CHAT_WATERMARK_KEY, USAGE_TOOL_WATERMARK_KEY,
};
use anyhow::Result;
use rusqlite::params;
use std::collections::HashMap;

pub(super) trait SqliteRetentionStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>>;
    fn cleanup_orphan_session_logs_impl(&self, cutoff_epoch_s: f64) -> Result<OrphanSweepResult>;
}

impl SqliteRetentionStorage for SqliteStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 {
            return Ok(deleted);
        }
        // Only stream_events are ephemeral replay buffers. Conversation,
        // context, runtime and audit records stay durable and are removed only
        // through the administrator cleanup endpoint.
        let removed = self.delete_stream_events_before_impl(cutoff_epoch_s)?;
        if removed > 0 {
            deleted.insert("stream_events".to_string(), removed);
            // Reclaim pages freed by the deletion and truncate the WAL so the
            // database file actually shrinks after retention cleanup.
            let conn = self.open()?;
            conn.execute_batch("PRAGMA incremental_vacuum;")?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        Ok(deleted)
    }

    fn cleanup_orphan_session_logs_impl(&self, cutoff_epoch_s: f64) -> Result<OrphanSweepResult> {
        self.ensure_initialized()?;
        let mut deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 {
            // Nothing to sweep, so nothing is left behind.
            return Ok(OrphanSweepResult {
                deleted,
                completed: true,
                swept_until: cutoff_epoch_s,
            });
        }
        // Orphan sweep: log rows whose session vanished from the durable
        // catalog. Only rows older than the cutoff are swept so in-flight
        // sessions are never touched, and deletion-grace sessions keep their
        // evidence. Summary counters only decrement for rows inside the
        // reconcile watermarks; the rest was never counted.
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let now = Self::now_ts();
        let chat_wm =
            super::log_stats_store::read_meta_f64_tx(&tx, USAGE_CHAT_WATERMARK_KEY)?.unwrap_or(0.0);
        let tool_wm = super::log_stats_store::read_meta_f64_tx(&tx, USAGE_TOOL_WATERMARK_KEY)?
            .unwrap_or(0.0) as i64;
        // `{table}` is interpolated per iteration below, so the predicate is
        // built with replace(), not a raw string literal - `{table}` inside a
        // plain literal never goes through the formatter and stays literal in
        // the SQL, which SQLite rejects as "unrecognized token".
        let orphan_predicate = |table: &str| {
            format!(
                "created_time < ?1 \
                 AND NOT EXISTS (SELECT 1 FROM chat_sessions c WHERE c.session_id = {table}.session_id) \
                 AND NOT EXISTS (SELECT 1 FROM deleted_session_log_grace g WHERE g.session_id = {table}.session_id)"
            )
        };
        let mut removed_total = 0i64;
        for table in ["thread_turns", "thread_log_changes", "artifact_logs"] {
            let sql = format!("DELETE FROM {table} WHERE {}", orphan_predicate(table));
            let removed = tx.execute(&sql, params![cutoff_epoch_s])? as i64;
            if removed > 0 {
                removed_total += removed;
                deleted.insert(table.to_string(), removed);
            }
        }
        // thread_items: counted visible-message rows decrement chat_records;
        // everything else was never in the summary. Per-user deltas are
        // aggregated before deletion, in the same transaction.
        let chat_deltas: Vec<(String, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT user_id, COUNT(*) FROM thread_items WHERE created_time < ?1 \
                 AND NOT EXISTS (SELECT 1 FROM chat_sessions c WHERE c.session_id = thread_items.session_id) \
                 AND NOT EXISTS (SELECT 1 FROM deleted_session_log_grace g WHERE g.session_id = thread_items.session_id) \
                 AND visibility='user' AND kind IN ('user_message','assistant_message') \
                 AND created_time <= ?2 GROUP BY user_id",
            )?;
            let rows = stmt
                .query_map(params![cutoff_epoch_s, chat_wm], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<std::result::Result<Vec<(String, i64)>, _>>()?;
            rows
        };
        let counted_items_deleted = tx.execute(
            "DELETE FROM thread_items WHERE created_time < ?1 \
             AND NOT EXISTS (SELECT 1 FROM chat_sessions c WHERE c.session_id = thread_items.session_id) \
             AND NOT EXISTS (SELECT 1 FROM deleted_session_log_grace g WHERE g.session_id = thread_items.session_id) \
             AND visibility='user' AND kind IN ('user_message','assistant_message') \
             AND created_time <= ?2",
            params![cutoff_epoch_s, chat_wm],
        )? as i64;
        let rest_items = tx.execute(
            "DELETE FROM thread_items WHERE created_time < ?1 \
             AND NOT EXISTS (SELECT 1 FROM chat_sessions c WHERE c.session_id = thread_items.session_id) \
             AND NOT EXISTS (SELECT 1 FROM deleted_session_log_grace g WHERE g.session_id = thread_items.session_id)",
            params![cutoff_epoch_s],
        )? as i64;
        if counted_items_deleted + rest_items > 0 {
            removed_total += counted_items_deleted + rest_items;
            deleted.insert(
                "thread_items".to_string(),
                counted_items_deleted + rest_items,
            );
        }
        // tool_logs: counted rows decrement tool_records.
        let tool_deltas: Vec<(String, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT user_id, COUNT(*) FROM tool_logs WHERE created_time < ?1 \
                 AND NOT EXISTS (SELECT 1 FROM chat_sessions c WHERE c.session_id = tool_logs.session_id) \
                 AND NOT EXISTS (SELECT 1 FROM deleted_session_log_grace g WHERE g.session_id = tool_logs.session_id) \
                 AND excluded = 0 AND id <= ?2 GROUP BY user_id",
            )?;
            let rows = stmt
                .query_map(params![cutoff_epoch_s, tool_wm], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<std::result::Result<Vec<(String, i64)>, _>>()?;
            rows
        };
        let counted_tools_deleted = tx.execute(
            "DELETE FROM tool_logs WHERE created_time < ?1 \
             AND NOT EXISTS (SELECT 1 FROM chat_sessions c WHERE c.session_id = tool_logs.session_id) \
             AND NOT EXISTS (SELECT 1 FROM deleted_session_log_grace g WHERE g.session_id = tool_logs.session_id) \
             AND excluded = 0 AND id <= ?2",
            params![cutoff_epoch_s, tool_wm],
        )? as i64;
        let rest_tools = tx.execute(
            "DELETE FROM tool_logs WHERE created_time < ?1 \
             AND NOT EXISTS (SELECT 1 FROM chat_sessions c WHERE c.session_id = tool_logs.session_id) \
             AND NOT EXISTS (SELECT 1 FROM deleted_session_log_grace g WHERE g.session_id = tool_logs.session_id)",
            params![cutoff_epoch_s],
        )? as i64;
        if counted_tools_deleted + rest_tools > 0 {
            removed_total += counted_tools_deleted + rest_tools;
            deleted.insert("tool_logs".to_string(), counted_tools_deleted + rest_tools);
        }
        super::log_stats_store::decrement_usage_summary_tx(&tx, &chat_deltas, &tool_deltas, now)?;
        tx.commit()?;
        if removed_total > 0 {
            conn.execute_batch("PRAGMA incremental_vacuum;")?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        // One transaction covers every table, so the sweep always reaches the
        // cutoff.
        Ok(OrphanSweepResult {
            deleted,
            completed: true,
            swept_until: cutoff_epoch_s,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
    use rusqlite::params;
    use tempfile::tempdir;

    fn insert_stream_event(
        storage: &SqliteStorage,
        session_id: &str,
        event_id: i64,
        created_time: f64,
    ) {
        let conn = storage.open().expect("open sqlite");
        conn.execute(
            "INSERT INTO stream_events (session_id, event_id, user_id, payload, created_time)
             VALUES (?, ?, ?, ?, ?)",
            params![
                session_id,
                event_id,
                "user-a",
                r#"{"event":"final"}"#,
                created_time
            ],
        )
        .expect("insert stream event");
    }

    fn count_stream_events(storage: &SqliteStorage) -> i64 {
        let conn = storage.open().expect("open sqlite");
        conn.query_row("SELECT COUNT(*) FROM stream_events", [], |row| row.get(0))
            .expect("count stream events")
    }

    #[test]
    fn cleanup_retention_removes_only_expired_stream_events() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("stream-event-retention.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");

        let now = SqliteStorage::now_ts();
        insert_stream_event(&storage, "session-a", 1, now - 48.0 * 3600.0);
        insert_stream_event(&storage, "session-a", 2, now - 3600.0);
        // Durable records sharing the same age must survive cleanup.
        storage
            .append_chat(
                "user-a",
                &serde_json::json!({
                    "session_id": "session-a",
                    "role": "user",
                    "content": "keep me"
                }),
            )
            .expect("append chat");

        let cutoff = now - 24.0 * 3600.0;
        let deleted = storage
            .cleanup_retention(cutoff)
            .expect("cleanup retention");
        assert_eq!(deleted.get("stream_events").copied(), Some(1));
        assert_eq!(count_stream_events(&storage), 1);
        assert_eq!(
            storage
                .load_thread_context_items("user-a", "session-a", 500, true)
                .expect("load chat history")
                .len(),
            1
        );
    }

    #[test]
    fn cleanup_retention_disabled_with_nonpositive_cutoff() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("stream-event-retention-disabled.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        insert_stream_event(&storage, "session-a", 1, 1.0);

        let deleted = storage.cleanup_retention(0.0).expect("cleanup retention");
        assert!(deleted.is_empty());
        assert_eq!(count_stream_events(&storage), 1);
    }
}
