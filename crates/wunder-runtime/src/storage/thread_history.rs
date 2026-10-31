//! Rebuildable SQLite projection for canonical rollout JSONL.
//!
//! A rollout file remains the source of truth.  This module only stores
//! completed turn metadata, bounded previews and verified body locators so
//! history can be paged without duplicating canonical bodies. Callers invoke projection
//! only after the recorder's flush barrier; the projection may lag a rollout,
//! but must never get ahead of it.

use super::thread_history_body::{item_digest, ItemBodyReader, ItemBodyRef, ITEM_BODY_COLUMNS};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Maximum item count accepted for one history read.  Keeping the cap in the
/// store prevents a caller from accidentally materialising a whole rollout.
pub(crate) const THREAD_HISTORY_PAGE_MAX: usize = 200;
const PROJECTION_BATCH_RECORDS: usize = 512;
const PROJECTION_BATCH_BYTES: usize = 4 * 1024 * 1024;
const PROJECTION_MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// Bounded active window per rollout. Only the newest items keep a full row
/// (locator, digest, preview); older items leave the window and are re-read
/// from the canonical rollout through sparse anchors. This keeps the index
/// size independent of total thread length.
pub(crate) const ACTIVE_WINDOW_ITEMS: i64 = 2000;
/// A sparse anchor is stored every `ANCHOR_INTERVAL` completed items, giving a
/// re-read start point so at most one interval needs sequential scanning.
pub(crate) const ANCHOR_INTERVAL: i64 = 1024;
/// Free pages tolerated before the projection hands them back to the
/// filesystem. `auto_vacuum=INCREMENTAL` only moves freed pages onto the
/// freelist at commit time; without an explicit `incremental_vacuum` the file
/// keeps its high-water mark forever, so a trimmed window would still look
/// like it grows with the log.
const RECLAIM_FREE_PAGES: i64 = 512;
pub(crate) const HISTORY_SCHEMA_VERSION: u32 = 5;

/// A generic record in the canonical JSONL stream.
///
/// The recorder owns the policy that decides which records reach JSONL.  The
/// projector only recognizes completed turn/item events and advances through
/// every complete record, leaving unknown record kinds for other readers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RolloutEnvelope {
    #[serde(default)]
    pub timestamp: Value,
    #[serde(default)]
    pub timestamp_ms: Option<i64>,
    pub ordinal: u64,
    #[serde(rename = "type")]
    pub record_type: String,
    /// An optional flat importer discriminant. Canonical events instead use
    /// `payload: { event, payload }`, which the projector unwraps explicitly.
    #[serde(default, rename = "event")]
    pub event_type: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

/// The durable position consumed by the SQLite projection.  Offsets are in
/// the uncompressed JSONL byte space and always point after a complete line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ProjectionState {
    pub next_rollout_byte_offset: u64,
    pub next_rollout_ordinal: u64,
    pub history_revision: u64,
}

/// Result of replaying the currently available complete JSONL prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProjectionProgress {
    pub state: ProjectionState,
    pub records_projected: u64,
    pub completed_items_projected: u64,
    /// An unfinished final line was deliberately not projected.  It becomes
    /// eligible only after the recorder writes its terminating newline.
    pub has_unfinished_tail: bool,
}

/// Revision-bound exclusive keyset cursor.  A caller must reset pagination if
/// a newer projection revision has appeared or a rollout lineage changed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ThreadHistoryCursor {
    pub history_revision: u64,
    pub rollout_ordinal: u64,
    pub item_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ThreadHistoryPageRequest {
    pub limit: usize,
    pub before: Option<ThreadHistoryCursor>,
}

/// A completed item hydrated from its canonical record, not a persisted copy.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThreadHistoryItem {
    pub rollout_id: String,
    pub turn_id: Option<String>,
    pub root_turn_id: Option<String>,
    pub item_id: String,
    pub item_type: String,
    pub created_at_ms: i64,
    pub rollout_ordinal: u64,
    pub item_json: Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ThreadHistoryPage {
    pub history_revision: u64,
    /// Newest-first, so a UI that renders oldest-first can prepend a page
    /// after reversing just this bounded result.
    pub items: Vec<ThreadHistoryItem>,
    pub next_before: Option<ThreadHistoryCursor>,
    pub has_more: bool,
}

/// Completed/running turn metadata.  The full display body stays in
/// the canonical rollout; this structure is for bounded trajectory reads.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThreadHistoryTurn {
    pub rollout_id: String,
    pub turn_id: String,
    pub root_turn_id: Option<String>,
    pub status: String,
    pub error_json: Option<Value>,
    pub started_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub first_user_item_id: Option<String>,
    pub final_agent_item_id: Option<String>,
    pub start_rollout_byte_offset: Option<u64>,
    pub end_rollout_byte_offset: Option<u64>,
    pub start_ordinal: Option<u64>,
    pub end_ordinal: Option<u64>,
    pub updated_at_ordinal: u64,
    /// Count derived from the indexed item projection for this turn.
    pub item_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ThreadHistoryTurnPage {
    pub history_revision: u64,
    pub turns: Vec<ThreadHistoryTurn>,
    pub next_before: Option<ThreadHistoryCursor>,
    pub has_more: bool,
}

/// A bounded forward view of canonical display events. Ordinals refer to the
/// rollout and may have gaps because model context records are not display
/// events. No second append-only change log is materialised for this view.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThreadHistoryEvent {
    pub ordinal: u64,
    pub kind: ThreadHistoryEventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ThreadHistoryEventKind {
    TurnStarted(ThreadHistoryTurn),
    ItemCompleted(ThreadHistoryItem),
    TurnFinished(ThreadHistoryTurn),
}

/// Local SQLite implementation of the rebuildable projection.
#[derive(Debug, Clone)]
pub(crate) struct ThreadHistoryStore {
    db_path: PathBuf,
    pool: super::sqlite_pool::SqlitePool,
    scope: Option<(String, Vec<HistorySegment>)>,
}

#[derive(Debug, Clone)]
pub(crate) struct HistorySegment {
    pub rollout_id: String,
    pub start_ordinal: u64,
    pub end_ordinal: Option<u64>,
}

/// Bounded re-read direction over a rollout lineage. `cursor` is always the
/// exclusive pagination edge: older pages continue strictly below it, newer
/// pages strictly above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScanDirection {
    Older,
    Newer,
}

#[derive(Debug, Default)]
pub(super) struct ScannedItems {
    /// Descending ordinals for `Older`, ascending for `Newer`.
    pub items: Vec<ThreadHistoryItem>,
    /// The scan stopped at its record/byte budget while matching history
    /// remained beyond the returned edge.
    pub more: bool,
}

impl ThreadHistoryStore {
    #[cfg(test)]
    fn persist_test_source(&self, rollout_id: &str, jsonl: &[u8]) -> Result<()> {
        let root = self.db_path.parent().unwrap();
        fs::create_dir_all(root.join("sessions"))?;
        let relative = format!("sessions/{rollout_id}.jsonl");
        fs::write(root.join(&relative), jsonl)?;
        let conn = Connection::open(root.join("state_1.sqlite"))?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS canonical_rollouts(rollout_id TEXT PRIMARY KEY,rollout_path TEXT NOT NULL);")?;
        conn.execute("INSERT INTO canonical_rollouts(rollout_id,rollout_path) VALUES(?1,?2) ON CONFLICT(rollout_id) DO UPDATE SET rollout_path=excluded.rollout_path",params![rollout_id,relative])?;
        Ok(())
    }

    pub(crate) fn delete_rollout(&self, rollout_id: &str) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM thread_items WHERE rollout_id=?1", [rollout_id])?;
        tx.execute("DELETE FROM thread_turns WHERE rollout_id=?1", [rollout_id])?;
        tx.execute(
            "DELETE FROM thread_root_counts WHERE rollout_id=?1",
            [rollout_id],
        )?;
        for table in [
            "thread_chat_heads_v2",
            "thread_chat_checkpoints_v2",
            "thread_chat_projection_state_v2",
            "thread_chat_checkpoint_state_v2",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE rollout_id=?1"),
                [rollout_id],
            )?;
        }
        tx.execute(
            "DELETE FROM thread_history_projection_state WHERE rollout_id=?1",
            [rollout_id],
        )?;
        tx.commit()?;
        conn.execute_batch("PRAGMA incremental_vacuum(256);")?;
        Ok(())
    }
    /// Opens or creates an independent projection database.  It must not point
    /// at the legacy monolithic application database.
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create thread history directory {}", parent.display()))?;
        }
        let store = Self {
            pool: super::sqlite_pool::SqlitePool::new(db_path.clone()),
            db_path,
            scope: None,
        };
        let mut conn = store.connect()?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        super::thread_history_migration::migrate(&mut conn, &store.db_path)?;
        initialize_schema(&conn)?;
        Ok(store)
    }

    pub(crate) fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub(crate) fn scoped(&self, current: &str, segments: Vec<HistorySegment>) -> Self {
        Self {
            db_path: self.db_path.clone(),
            pool: self.pool.clone(),
            scope: Some((current.to_owned(), segments)),
        }
    }

    /// Temporary views only join indexed segments. No inherited body is copied
    /// into a child rollout or its projection, even when history is forked.
    pub(crate) fn apply_scope(
        conn: &Connection,
        current: &str,
        segments: &[HistorySegment],
    ) -> Result<()> {
        if segments.is_empty() || segments.len() > 64 {
            bail!("invalid history lineage segment count");
        }
        validate_id("rollout_id", current)?;
        // Ordinary threads need no temporary views or schema reparsing.
        if segments.len() == 1
            && segments[0].rollout_id == current
            && segments[0].start_ordinal == 0
            && segments[0].end_ordinal.is_none()
        {
            return Ok(());
        }
        let quote = |v: &str| format!("'{}'", v.replace('\'', "''"));
        let current = quote(current);
        let condition = |column: &str, segment: &HistorySegment| -> Result<String> {
            validate_id("rollout_id", &segment.rollout_id)?;
            Ok(format!(
                "rollout_id={} AND {column}>={} {}",
                quote(&segment.rollout_id),
                sqlite_i64(segment.start_ordinal)?,
                segment
                    .end_ordinal
                    .map(|end| sqlite_i64(end).map(|end| format!("AND {column}<{end}")))
                    .transpose()?
                    .unwrap_or_default()
            ))
        };
        conn.execute_batch("DROP VIEW IF EXISTS temp.thread_items;")?;
        let items=segments.iter().map(|s|condition("rollout_ordinal",s).map(|c|format!("SELECT {current} AS rollout_id,turn_id,root_turn_id,item_id,item_type,created_at_ms,rollout_ordinal,rollout_id AS source_rollout_id,record_byte_offset,record_byte_length,item_bytes,item_digest,preview_json,visible,message_context FROM main.thread_items WHERE {c}"))).collect::<Result<Vec<_>>>()?.join(" UNION ALL ");
        let turns=segments.iter().map(|s|condition("COALESCE(start_ordinal,end_ordinal,0)",s).map(|c|{
            let end=s.end_ordinal.map(|v|v.to_string()).unwrap_or(i64::MAX.to_string());
            format!("SELECT {current} AS rollout_id,turn_id,root_turn_id,CASE WHEN end_ordinal>={end} THEN 'running' ELSE status END AS status,error_json,started_at_ms,CASE WHEN end_ordinal>={end} THEN NULL ELSE completed_at_ms END AS completed_at_ms,duration_ms,first_user_item_id,final_agent_item_id,start_rollout_byte_offset,end_rollout_byte_offset,start_ordinal,CASE WHEN end_ordinal>={end} THEN NULL ELSE end_ordinal END AS end_ordinal,updated_at_ordinal,item_count FROM main.thread_turns WHERE {c}")
        })).collect::<Result<Vec<_>>>()?.join(" UNION ALL ");
        let counts=segments.iter().map(|s|->Result<String>{
            let physical=quote(&s.rollout_id);
            if s.end_ordinal.is_none() {
                return Ok(format!("SELECT {current} AS rollout_id,root_turn_id,visible_items,workflow_items FROM main.thread_root_counts WHERE rollout_id={physical}"));
            }
            let range=condition("i.rollout_ordinal",s)?.replacen("rollout_id=","i.rollout_id=",1);
            // Keep root predicates pushable through UNION ALL. Each count uses
            // the root/ordinal index and never groups the entire inherited log.
            Ok(format!("SELECT {current} AS rollout_id,c.root_turn_id,
                (SELECT COUNT(*) FROM main.thread_items i WHERE {range} AND COALESCE(i.root_turn_id,i.turn_id)=c.root_turn_id AND i.visible=1) AS visible_items,
                (SELECT COUNT(*) FROM main.thread_items i WHERE {range} AND COALESCE(i.root_turn_id,i.turn_id)=c.root_turn_id AND i.visible=1 AND i.item_type!='user_message') AS workflow_items
                FROM main.thread_root_counts c WHERE c.rollout_id={physical}"))
        }).collect::<Result<Vec<_>>>()?.join(" UNION ALL ");
        let chat = segments.iter().map(|s| -> Result<String> {
            let physical = quote(&s.rollout_id);
            if let Some(end) = s.end_ordinal {
                Ok(format!("SELECT {current} AS rollout_id,h.root_turn_id,h.rollout_ordinal,h.summary_json
                    FROM main.thread_chat_heads_v2 h WHERE h.rollout_id={physical}
                    AND h.rollout_ordinal<{} UNION ALL
                    SELECT {current} AS rollout_id,h.root_turn_id,c.rollout_ordinal,c.summary_json
                    FROM main.thread_chat_heads_v2 h JOIN main.thread_chat_checkpoints_v2 c
                    ON c.rollout_id=h.rollout_id AND c.root_turn_id=h.root_turn_id
                    AND c.rollout_ordinal=(SELECT MAX(p.rollout_ordinal) FROM main.thread_chat_checkpoints_v2 p
                        WHERE p.rollout_id=h.rollout_id AND p.root_turn_id=h.root_turn_id AND p.rollout_ordinal<{})
                    WHERE h.rollout_id={physical} AND h.rollout_ordinal>={}",
                    sqlite_i64(end)?,sqlite_i64(end)?,sqlite_i64(end)?))
            } else {
                Ok(format!("SELECT {current} AS rollout_id,root_turn_id,rollout_ordinal,summary_json
                    FROM main.thread_chat_heads_v2 WHERE rollout_id={physical}"))
            }
        }).collect::<Result<Vec<_>>>()?.join(" UNION ALL ");
        conn.execute_batch(&format!("CREATE TEMP VIEW thread_items AS {items}; CREATE TEMP VIEW thread_turns AS {turns}; CREATE TEMP VIEW thread_root_counts AS {counts}; CREATE TEMP VIEW thread_chat_heads_v2 AS {chat};"))?;
        Ok(())
    }

    pub(crate) fn ensure_chat_summary(&self, rollout_id: &str) -> Result<()> {
        super::thread_chat_summary::ensure_ready(&mut *self.connect()?, rollout_id)
    }

    /// Materialize cutoff checkpoints for one lineage segment.  Scoped reads
    /// of a branched segment recover per-root summaries as of `through`
    /// (exclusive); heads alone cannot answer that, and writing one checkpoint
    /// per item on the hot path would duplicate the head for every entry.
    pub(crate) fn ensure_chat_checkpoints(&self, rollout_id: &str, through_ordinal: u64) -> Result<()> {
        validate_id("rollout_id", rollout_id)?;
        let through = sqlite_i64(through_ordinal)?;
        super::thread_chat_summary::ensure_checkpoints(&mut *self.connect()?, rollout_id, through)
    }

    /// Returns the existing projection waterline without reading a rollout.
    pub(crate) fn projection_state(&self, rollout_id: &str) -> Result<ProjectionState> {
        validate_id("rollout_id", rollout_id)?;
        let conn = self.connect()?;
        load_state(&conn, rollout_id)
    }

    /// Incrementally projects the complete records from a full, uncompressed
    /// rollout JSONL byte buffer.  A non-newline-terminated tail is ignored so
    /// a concurrent reader cannot persist a half-written JSON object.
    ///
    /// This operation performs all item/turn writes and the new waterline in
    /// one SQLite transaction.  On a projection error nothing from this call
    /// is committed; a later retry resumes from the previous byte boundary.
    #[cfg(test)]
    pub(crate) fn project_jsonl_bytes(
        &self,
        rollout_id: &str,
        jsonl: &[u8],
    ) -> Result<ProjectionProgress> {
        validate_id("rollout_id", rollout_id)?;
        self.persist_test_source(rollout_id, jsonl)?;
        let conn = self.connect()?;
        let current = load_state(&conn, rollout_id)?;
        let start = usize::try_from(current.next_rollout_byte_offset)
            .map_err(|_| anyhow!("projection byte offset exceeds this platform's address space"))?;
        if start > jsonl.len() {
            bail!(
                "rollout {rollout_id} is shorter than its projected prefix; rebuild this derived projection"
            );
        }
        self.project_chunk(rollout_id, current, &jsonl[start..])
    }

    /// Replays at most 512 records / 4 MiB from the saved byte position.
    /// A single legal line may use up to 8 MiB; larger lines fail explicitly.
    /// `flushed_end` must be obtained from the recorder flush receipt.
    pub(crate) fn project_file_batch(
        &self,
        path: impl AsRef<Path>,
        rollout_id: &str,
        flushed_end: u64,
    ) -> Result<ProjectionProgress> {
        validate_id("rollout_id", rollout_id)?;
        let current = self.projection_state(rollout_id)?;
        if current.next_rollout_byte_offset > flushed_end {
            bail!("rollout flushed boundary is before its projected prefix");
        }
        if current.next_rollout_byte_offset == flushed_end {
            return Ok(ProjectionProgress {
                state: current,
                ..Default::default()
            });
        }
        let mut file = super::rollout_compression::open_rollout_reader(path.as_ref())
            .context("open rollout projection source")?;
        if file.len() < flushed_end {
            bail!("rollout file is shorter than its flushed boundary");
        }
        self.project_reader_batch(&mut file, rollout_id, flushed_end)
    }

    pub(crate) fn project_reader_batch(
        &self,
        file: &mut (impl Read + Seek),
        rollout_id: &str,
        flushed_end: u64,
    ) -> Result<ProjectionProgress> {
        validate_id("rollout_id", rollout_id)?;
        let current = self.projection_state(rollout_id)?;
        if current.next_rollout_byte_offset > flushed_end {
            bail!("rollout flushed boundary is before its projected prefix");
        }
        file.seek(SeekFrom::Start(current.next_rollout_byte_offset))?;
        let remaining = flushed_end - current.next_rollout_byte_offset;
        let mut reader = BufReader::new(file.take(remaining));
        let mut batch = Vec::with_capacity(PROJECTION_BATCH_BYTES);
        let mut lines = 0usize;
        while lines < PROJECTION_BATCH_RECORDS && batch.len() < PROJECTION_BATCH_BYTES {
            let before = batch.len();
            let complete = read_bounded_line(&mut reader, &mut batch)?;
            if batch.len() == before {
                break;
            }
            if before > 0 && batch.len() > PROJECTION_BATCH_BYTES {
                // Leave this complete record for the next seek/batch. A
                // single large record may use the explicit 8 MiB allowance,
                // but it cannot push an ordinary multi-record batch over its
                // byte budget.
                batch.truncate(before);
                break;
            }
            if !complete {
                break;
            }
            lines += 1;
        }
        self.project_chunk(rollout_id, current, &batch)
    }

    fn project_chunk(
        &self,
        rollout_id: &str,
        current: ProjectionState,
        jsonl: &[u8],
    ) -> Result<ProjectionProgress> {
        let mut conn = self.connect()?;
        super::thread_chat_summary::ensure_ready(&mut conn, rollout_id)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if load_state(&tx, rollout_id)? != current {
            bail!("thread history projection advanced concurrently; retry the batch");
        }
        let mut offset = 0usize;
        let mut next_ordinal = current.next_rollout_ordinal;
        let mut records_projected = 0u64;
        let mut items_projected = 0u64;

        while let Some(relative_end) = jsonl[offset..].iter().position(|byte| *byte == b'\n') {
            let line_end = offset + relative_end;
            let next_offset = line_end + 1;
            let raw_line = trim_line_ending(&jsonl[offset..line_end]);
            if !raw_line.is_empty() {
                let text = std::str::from_utf8(raw_line).with_context(|| {
                    format!("rollout {rollout_id} has non-UTF-8 JSONL at byte {offset}")
                })?;
                let envelope: RolloutEnvelope = serde_json::from_str(text).with_context(|| {
                    format!("rollout {rollout_id} has invalid JSONL record at byte {offset}")
                })?;
                if envelope.ordinal < next_ordinal {
                    bail!(
                        "rollout {rollout_id} ordinal {} is before projected ordinal {next_ordinal}",
                        envelope.ordinal
                    );
                }
                let record_offset = current
                    .next_rollout_byte_offset
                    .checked_add(
                        u64::try_from(offset)
                            .map_err(|_| anyhow!("rollout byte offset cannot fit in u64"))?,
                    )
                    .ok_or_else(|| anyhow!("rollout byte offset overflow"))?;
                project_envelope(
                    &tx,
                    rollout_id,
                    &envelope,
                    record_offset,
                    (next_offset - offset) as u64,
                )?;
                next_ordinal = envelope
                    .ordinal
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("rollout ordinal overflow"))?;
                records_projected += 1;
                if is_completed_item(&envelope) {
                    items_projected += 1;
                }
            }
            offset = next_offset;
        }

        let has_unfinished_tail = offset < jsonl.len();
        let final_offset = current
            .next_rollout_byte_offset
            .checked_add(
                u64::try_from(offset)
                    .map_err(|_| anyhow!("rollout byte offset cannot fit in u64"))?,
            )
            .ok_or_else(|| anyhow!("rollout byte offset overflow"))?;
        let mut state = current;
        if records_projected > 0 || final_offset != current.next_rollout_byte_offset {
            // Advancing over blank complete lines is harmless and prevents the
            // scanner from reparsing them forever.  A revision changes only
            // when a canonical record was actually handled.
            state.next_rollout_byte_offset = final_offset;
            state.next_rollout_ordinal = next_ordinal;
            if records_projected > 0 {
                state.history_revision = state
                    .history_revision
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("thread history revision overflow"))?;
            }
            save_state(&tx, rollout_id, state)?;
            super::thread_chat_summary::save_position(
                &tx,
                rollout_id,
                sqlite_i64(state.next_rollout_ordinal)?,
            )?;
        }
        let trimmed = trim_active_window(&tx, rollout_id)?;
        tx.commit()?;
        if trimmed > 0 {
            reclaim_free_pages(&conn)?;
        }

        Ok(ProjectionProgress {
            state,
            records_projected,
            completed_items_projected: items_projected,
            has_unfinished_tail,
        })
    }

    /// Reads completed items with a revision-bound, descending keyset cursor.
    /// It never uses OFFSET, so older pages do not become increasingly costly.
    pub(crate) fn page_items(
        &self,
        rollout_id: &str,
        request: ThreadHistoryPageRequest,
    ) -> Result<ThreadHistoryPage> {
        self.page_items_for_turn(rollout_id, None, request)
    }

    pub(crate) fn page_items_for_turn(
        &self,
        rollout_id: &str,
        turn_id: Option<&str>,
        request: ThreadHistoryPageRequest,
    ) -> Result<ThreadHistoryPage> {
        validate_id("rollout_id", rollout_id)?;
        if let Some(turn_id) = turn_id {
            validate_id("turn_id", turn_id)?;
        }
        let mut connection = self.connect_read()?;
        let conn = connection.transaction()?;
        let state = load_state(&conn, rollout_id)?;
        if let Some(cursor) = request.before.as_ref() {
            if cursor.history_revision != state.history_revision {
                bail!(
                    "thread history cursor revision {} does not match current revision {}",
                    cursor.history_revision,
                    state.history_revision
                );
            }
        }

        let limit = request.limit.clamp(1, THREAD_HISTORY_PAGE_MAX);
        let mut bodies = ItemBodyReader::for_connection(&conn)?;
        let fetch = i64::try_from(limit + 1).expect("thread history page maximum fits i64");
        let (mut items, budget_limited) = if let Some(cursor) = request.before.as_ref() {
            let ordinal = i64::try_from(cursor.rollout_ordinal)
                .map_err(|_| anyhow!("history cursor ordinal exceeds SQLite integer range"))?;
            let mut statement = conn.prepare(&format!(
                "SELECT turn_id,root_turn_id,item_id,item_type,created_at_ms,rollout_ordinal,{ITEM_BODY_COLUMNS}
                 FROM thread_items
                 WHERE rollout_id=?1
                   AND (rollout_ordinal < ?2 OR (rollout_ordinal = ?2 AND item_id < ?3))
                   AND (?5 IS NULL OR turn_id=?5)
                 ORDER BY rollout_ordinal DESC,item_id DESC
                 LIMIT ?4"
            ))?;
            let rows =
                statement.query(params![rollout_id, ordinal, cursor.item_id, fetch, turn_id])?;
            collect_item_page(rows, rollout_id, &mut bodies)?
        } else {
            let mut statement = conn.prepare(&format!(
                "SELECT turn_id,root_turn_id,item_id,item_type,created_at_ms,rollout_ordinal,{ITEM_BODY_COLUMNS}
                 FROM thread_items
                 WHERE rollout_id=?1 AND (?3 IS NULL OR turn_id=?3)
                 ORDER BY rollout_ordinal DESC,item_id DESC
                 LIMIT ?2"
            ))?;
            let rows = statement.query(params![rollout_id, fetch, turn_id])?;
            collect_item_page(rows, rollout_id, &mut bodies)?
        };

        let window_extra = items.len() > limit;
        if window_extra {
            items.pop();
        }
        let mut more = window_extra || budget_limited;
        let root = self.db_path.parent().expect("history database has a parent");
        let segments: &[HistorySegment] = match &self.scope {
            Some((current, segments)) if current == rollout_id => segments.as_slice(),
            _ => &[],
        };

        // The active window could not fill the page: re-read the remainder from
        // the canonical rollout through sparse anchors. A rollout ordinal
        // identifies one record, so the keyset edge needs no id tiebreak here.
        if !more && items.len() < limit {
            let edge = items
                .last()
                .map(|item| i64::try_from(item.rollout_ordinal).unwrap_or(i64::MAX))
                .or_else(|| {
                    request.before.as_ref().map(|cursor| {
                        i64::try_from(cursor.rollout_ordinal).unwrap_or(i64::MAX)
                    })
                });
            let accept = |item: &ThreadHistoryItem| {
                turn_id.map_or(true, |turn| item.turn_id.as_deref() == Some(turn))
            };
            let missing = limit - items.len();
            let hint = match turn_id {
                Some(turn) => turn_scan_hint(&conn, rollout_id, turn)?,
                None => None,
            };
            let scanned = scan_lineage(
                root,
                &conn,
                segments,
                rollout_id,
                ScanDirection::Older,
                edge,
                None,
                None,
                missing,
                hint,
                &accept,
            )?;
            more = scanned.more;
            // The re-read slice is newest-first and already bounded by this
            // page's missing count, so it appends without reordering.
            items.extend(scanned.items);
        }
        // Report older history only when a cursor can address the next page;
        // an empty page never loops the caller.
        let has_more = more && !items.is_empty();
        let next_before = if has_more {
            items.last().map(|last| ThreadHistoryCursor {
                history_revision: state.history_revision,
                rollout_ordinal: last.rollout_ordinal,
                item_id: last.item_id.clone(),
            })
        } else {
            None
        };
        Ok(ThreadHistoryPage {
            history_revision: state.history_revision,
            items,
            next_before,
            has_more,
        })
    }

    pub(crate) fn read_item(&self, rollout_id: &str, item_id: &str) -> Result<ThreadHistoryItem> {
        self.find_item(rollout_id, item_id)?
            .ok_or_else(|| anyhow!("thread history item not found"))
    }

    pub(crate) fn find_item(
        &self,
        rollout_id: &str,
        item_id: &str,
    ) -> Result<Option<ThreadHistoryItem>> {
        validate_id("rollout_id", rollout_id)?;
        validate_id("item_id", item_id)?;
        let conn = self.connect_read()?;
        let mut statement = conn.prepare(&format!(
            "SELECT turn_id,root_turn_id,item_id,item_type,created_at_ms,rollout_ordinal,{ITEM_BODY_COLUMNS}
             FROM thread_items WHERE rollout_id=?1 AND item_id=?2 LIMIT 1"
        ))?;
        let rows = statement.query(params![rollout_id, item_id])?;
        let mut projected = collect_items(
            rows,
            rollout_id,
            &mut ItemBodyReader::for_connection(&conn)?,
        )?;
        if let Some(item) = projected.pop() {
            return Ok(Some(item));
        }
        // Outside the active window: locate the owning turn and re-read the
        // canonical rollout so the read stays bounded.
        let root = self.db_path.parent().expect("history database has a parent");
        let segments: &[HistorySegment] = match &self.scope {
            Some((current, segments)) if current == rollout_id => segments.as_slice(),
            _ => &[],
        };
        find_item_via_anchor(&conn, root, segments, rollout_id, item_id)
    }

    pub(crate) fn read_canonical_item(
        &self,
        rollout_id: &str,
        item_id: &str,
    ) -> Result<wunder_core::thread_store::CanonicalItem> {
        serde_json::from_value(self.read_item(rollout_id, item_id)?.item_json)
            .context("history item is not a canonical completed item")
    }

    pub(crate) fn page_turns(
        &self,
        rollout_id: &str,
        request: ThreadHistoryPageRequest,
    ) -> Result<ThreadHistoryTurnPage> {
        validate_id("rollout_id", rollout_id)?;
        let mut connection = self.connect()?;
        let conn = connection.transaction()?;
        let state = load_state(&conn, rollout_id)?;
        if request
            .before
            .as_ref()
            .is_some_and(|cursor| cursor.history_revision != state.history_revision)
        {
            bail!("thread history cursor revision does not match current revision");
        }
        let limit = request.limit.clamp(1, THREAD_HISTORY_PAGE_MAX);
        let mut statement = conn.prepare(
            "SELECT t.turn_id,t.root_turn_id,t.status,t.error_json,t.started_at_ms,t.completed_at_ms,t.duration_ms,
                    t.first_user_item_id,t.final_agent_item_id,t.start_rollout_byte_offset,
                    t.end_rollout_byte_offset,t.start_ordinal,t.end_ordinal,t.updated_at_ordinal,
                    t.item_count
             FROM thread_turns t WHERE t.rollout_id=?1
               AND (?2 IS NULL OR COALESCE(t.start_ordinal,0) < ?2
                 OR (COALESCE(t.start_ordinal,0)=?2 AND t.turn_id < ?3))
             ORDER BY COALESCE(t.start_ordinal,0) DESC,t.turn_id DESC LIMIT ?4"
        )?;
        let ordinal = request
            .before
            .as_ref()
            .map(|cursor| sqlite_i64(cursor.rollout_ordinal))
            .transpose()?;
        let id = request
            .before
            .as_ref()
            .map(|cursor| cursor.item_id.as_str());
        let mut rows = statement.query(params![rollout_id, ordinal, id, (limit + 1) as i64])?;
        let mut turns = Vec::new();
        while let Some(row) = rows.next()? {
            let mut turn = read_turn(row, rollout_id)?;
            turn.item_count =
                u32::try_from(row.get::<_, i64>(14)?).context("turn item count exceeds u32")?;
            turns.push(turn);
        }
        let has_more = turns.len() > limit;
        if has_more {
            turns.pop();
        }
        let next_before = if has_more {
            turns.last().map(|last| ThreadHistoryCursor {
                history_revision: state.history_revision,
                rollout_ordinal: last.start_ordinal.unwrap_or_default(),
                item_id: last.turn_id.clone(),
            })
        } else {
            None
        };
        Ok(ThreadHistoryTurnPage {
            history_revision: state.history_revision,
            turns,
            next_before,
            has_more,
        })
    }

    /// Lists turn metadata newest-first.  This is intentionally bounded too;
    /// detailed bodies are obtained from `page_items`.
    pub(crate) fn recent_turns(
        &self,
        rollout_id: &str,
        limit: usize,
    ) -> Result<Vec<ThreadHistoryTurn>> {
        validate_id("rollout_id", rollout_id)?;
        let conn = self.connect()?;
        let limit = i64::try_from(limit.clamp(1, THREAD_HISTORY_PAGE_MAX))
            .expect("thread history page maximum fits i64");
        let mut statement = conn.prepare(
            "SELECT turn_id,root_turn_id,status,error_json,started_at_ms,completed_at_ms,duration_ms,
                    first_user_item_id,final_agent_item_id,start_rollout_byte_offset,
                    end_rollout_byte_offset,start_ordinal,end_ordinal,updated_at_ordinal
             FROM thread_turns WHERE rollout_id=?1
             ORDER BY COALESCE(end_ordinal,start_ordinal,0) DESC,turn_id DESC LIMIT ?2",
        )?;
        let mut rows = statement.query(params![rollout_id, limit])?;
        let mut turns = Vec::new();
        while let Some(row) = rows.next()? {
            let error: Option<String> = row.get(3)?;
            turns.push(ThreadHistoryTurn {
                rollout_id: rollout_id.to_owned(),
                turn_id: row.get(0)?,
                root_turn_id: row.get(1)?,
                status: row.get(2)?,
                error_json: error
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()
                    .context("thread turn contains invalid error JSON")?,
                started_at_ms: row.get(4)?,
                completed_at_ms: row.get(5)?,
                duration_ms: row.get(6)?,
                first_user_item_id: row.get(7)?,
                final_agent_item_id: row.get(8)?,
                start_rollout_byte_offset: sqlite_u64(row.get::<_, Option<i64>>(9)?)?,
                end_rollout_byte_offset: sqlite_u64(row.get::<_, Option<i64>>(10)?)?,
                start_ordinal: sqlite_u64(row.get::<_, Option<i64>>(11)?)?,
                end_ordinal: sqlite_u64(row.get::<_, Option<i64>>(12)?)?,
                updated_at_ordinal: sqlite_u64_required(row.get(13)?)?,
                item_count: 0,
            });
        }
        Ok(turns)
    }

    /// Reads at most 200 display events after an exclusive rollout ordinal.
    /// Passing `None` includes ordinal zero. The caller owns rollout identity
    /// validation; an append does not invalidate this forward cursor.
    pub(crate) fn forward_events(
        &self,
        rollout_id: &str,
        after_ordinal: Option<u64>,
        limit: usize,
    ) -> Result<Vec<ThreadHistoryEvent>> {
        self.forward_display_events(rollout_id, after_ordinal, limit, false)
    }

    pub(crate) fn forward_visible_events(
        &self,
        rollout_id: &str,
        after_ordinal: Option<u64>,
        limit: usize,
    ) -> Result<Vec<ThreadHistoryEvent>> {
        self.forward_display_events(rollout_id, after_ordinal, limit, true)
    }

    fn forward_display_events(
        &self,
        rollout_id: &str,
        after_ordinal: Option<u64>,
        limit: usize,
        visible: bool,
    ) -> Result<Vec<ThreadHistoryEvent>> {
        validate_id("rollout_id", rollout_id)?;
        let after = after_ordinal.map(sqlite_i64).transpose()?.unwrap_or(-1);
        let limit = limit.clamp(1, THREAD_HISTORY_PAGE_MAX) as i64;
        let mut conn = self.connect_read()?;
        let snapshot = conn.transaction()?;
        let filter = if visible {
            format!("AND {}", super::canonical_thread_log::VISIBLE)
        } else {
            String::new()
        };
        let mut statement = snapshot.prepare(&format!(
            "SELECT event_kind,event_ordinal,entity_id FROM (
                SELECT 0 AS event_kind,start_ordinal AS event_ordinal,turn_id AS entity_id
                  FROM thread_turns WHERE rollout_id=?1 AND start_ordinal>?2
                UNION ALL
                SELECT 1,rollout_ordinal,item_id
                  FROM thread_items WHERE rollout_id=?1 AND rollout_ordinal>?2 {filter}
                UNION ALL
                SELECT 2,end_ordinal,turn_id
                  FROM thread_turns WHERE rollout_id=?1 AND end_ordinal>?2
             ) ORDER BY event_ordinal ASC,event_kind ASC,entity_id ASC LIMIT ?3"
        ))?;
        let references = statement
            .query_map(params![rollout_id, after, limit], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut turns = snapshot.prepare(
            "SELECT turn_id,root_turn_id,status,error_json,started_at_ms,completed_at_ms,duration_ms,
                    first_user_item_id,final_agent_item_id,start_rollout_byte_offset,
                    end_rollout_byte_offset,start_ordinal,end_ordinal,updated_at_ordinal
             FROM thread_turns WHERE rollout_id=?1 AND turn_id=?2 LIMIT 1",
        )?;
        let mut items = snapshot.prepare(&format!(
            "SELECT turn_id,root_turn_id,item_id,item_type,created_at_ms,rollout_ordinal,{ITEM_BODY_COLUMNS}
             FROM thread_items WHERE rollout_id=?1 AND item_id=?2 LIMIT 1"
        ))?;
        let mut bodies = ItemBodyReader::for_connection(&snapshot)?;
        let mut events = Vec::with_capacity(references.len());
        let mut bytes = 0;
        for (kind, ordinal, id) in references {
            let kind = if kind == 1 {
                let size: i64 = snapshot.query_row(
                    "SELECT item_bytes FROM thread_items WHERE rollout_id=?1 AND item_id=?2",
                    params![rollout_id, id],
                    |r| r.get(0),
                )?;
                if bytes + size > 16 * 1024 * 1024 && !events.is_empty() {
                    break;
                }
                bytes += size;
                let rows = items.query(params![rollout_id, id])?;
                let item = collect_items(rows, rollout_id, &mut bodies)?
                    .pop()
                    .ok_or_else(|| anyhow!("projected history event item disappeared"))?;
                ThreadHistoryEventKind::ItemCompleted(item)
            } else {
                let mut rows = turns.query(params![rollout_id, id])?;
                let row = rows
                    .next()?
                    .ok_or_else(|| anyhow!("projected history event turn disappeared"))?;
                let turn = read_turn(row, rollout_id)?;
                if kind == 0 {
                    ThreadHistoryEventKind::TurnStarted(turn)
                } else {
                    ThreadHistoryEventKind::TurnFinished(turn)
                }
            };
            events.push(ThreadHistoryEvent {
                ordinal: sqlite_u64_required(ordinal)?,
                kind,
            });
        }
        Ok(events)
    }

    fn connect(&self) -> Result<super::sqlite_pool::ConnectionLease> {
        if self.scope.is_some() {
            return self.connect_read();
        }
        self.pool.get()
    }

    fn connect_read(&self) -> Result<super::sqlite_pool::ConnectionLease> {
        let conn = Connection::open_with_flags(
            &self.db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::from_secs(2))?;
        prepare_item_view(&conn)?;
        if let Some((current, segments)) = &self.scope {
            Self::apply_scope(&conn, current, segments)?;
        }
        Ok(super::sqlite_pool::ConnectionLease::uncached(conn))
    }
}

pub(super) fn prepare_item_view(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TEMP VIEW thread_items AS SELECT *,rollout_id AS source_rollout_id FROM main.thread_items;")?;
    Ok(())
}

pub(super) fn message_context(item: &Value, kind: &str) -> bool {
    let payload = item.get("payload").unwrap_or(item);
    let body = if payload.get("item_id").and_then(Value::as_str).is_some()
        && payload.get("payload").is_some_and(Value::is_object)
    {
        &payload["payload"]
    } else {
        payload
    };
    matches!(kind, "user_message" | "assistant_message" | "agent_message")
        || (kind == "tool_result" && body["role"] == "tool")
        || (kind == "configuration"
            && body["role"] == "system"
            && body
                .pointer("/meta/replacement_history")
                .is_some_and(Value::is_array))
}

fn initialize_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS thread_history_projection_state (
            rollout_id TEXT PRIMARY KEY NOT NULL,
            next_rollout_byte_offset INTEGER NOT NULL,
            next_rollout_ordinal INTEGER NOT NULL,
            history_revision INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS thread_turns (
            rollout_id TEXT NOT NULL,
            turn_id TEXT NOT NULL,
            root_turn_id TEXT,
            status TEXT NOT NULL,
            error_json TEXT,
            started_at_ms INTEGER NOT NULL,
            completed_at_ms INTEGER,
            duration_ms INTEGER,
            first_user_item_id TEXT,
            final_agent_item_id TEXT,
            start_rollout_byte_offset INTEGER,
            end_rollout_byte_offset INTEGER,
            start_ordinal INTEGER,
            end_ordinal INTEGER,
            item_count INTEGER NOT NULL DEFAULT 0,
            updated_at_ordinal INTEGER NOT NULL,
            PRIMARY KEY (rollout_id, turn_id)
        );
        CREATE TABLE IF NOT EXISTS thread_item_anchors (
            rollout_id TEXT NOT NULL,
            rollout_ordinal INTEGER NOT NULL,
            item_id TEXT NOT NULL,
            record_byte_offset INTEGER NOT NULL,
            record_byte_length INTEGER NOT NULL,
            PRIMARY KEY (rollout_id, rollout_ordinal)
        );
        CREATE TABLE IF NOT EXISTS thread_items (
            rollout_id TEXT NOT NULL,
            turn_id TEXT,
            root_turn_id TEXT,
            item_id TEXT NOT NULL,
            item_type TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            rollout_ordinal INTEGER NOT NULL,
            record_byte_offset INTEGER NOT NULL,
            record_byte_length INTEGER NOT NULL,
            item_bytes INTEGER NOT NULL,
            item_digest BLOB NOT NULL,
            message_context INTEGER NOT NULL DEFAULT 0,
            preview_json TEXT NOT NULL DEFAULT '{}',
            visible INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (rollout_id, item_id)
        );
        CREATE TABLE IF NOT EXISTS thread_root_counts (
            rollout_id TEXT NOT NULL, root_turn_id TEXT NOT NULL,
            visible_items INTEGER NOT NULL DEFAULT 0, workflow_items INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(rollout_id, root_turn_id)
        );
        CREATE INDEX IF NOT EXISTS idx_thread_history_items_page
            ON thread_items(rollout_id, rollout_ordinal DESC, item_id DESC);
        CREATE INDEX IF NOT EXISTS idx_thread_history_items_turn
            ON thread_items(rollout_id, turn_id, rollout_ordinal);
        CREATE INDEX IF NOT EXISTS idx_thread_history_items_type
            ON thread_items(rollout_id, item_type, rollout_ordinal DESC);
        CREATE INDEX IF NOT EXISTS idx_thread_history_items_root
            ON thread_items(rollout_id, COALESCE(root_turn_id,turn_id), rollout_ordinal);
        CREATE INDEX IF NOT EXISTS idx_thread_history_items_root_type
            ON thread_items(rollout_id, COALESCE(root_turn_id,turn_id), item_type, rollout_ordinal DESC);
        CREATE INDEX IF NOT EXISTS idx_thread_history_turns_page
            ON thread_turns(rollout_id, end_ordinal DESC, start_ordinal DESC, turn_id DESC);
        CREATE INDEX IF NOT EXISTS idx_thread_history_turns_start_page
            ON thread_turns(rollout_id, COALESCE(start_ordinal,0) DESC, turn_id DESC);
        CREATE INDEX IF NOT EXISTS idx_thread_history_turns_forward_start
            ON thread_turns(rollout_id, start_ordinal);
        CREATE INDEX IF NOT EXISTS idx_thread_history_anchors_ordinal
            ON thread_item_anchors(rollout_id, rollout_ordinal);",
    )?;
    // Upgrade an already-created table without relying solely on the migration
    // branch; ADD COLUMN is a no-op once the column exists.
    ensure_column(
        conn,
        "thread_turns",
        "item_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::thread_chat_summary::initialize(conn)
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    let exists: i64 = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
        params![table, column],
        |row| row.get(0),
    )?;
    if exists == 0 {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition};"))?;
    }
    Ok(())
}

/// Drop rows outside the bounded active window. The kept rows are the newest
/// `ACTIVE_WINDOW_ITEMS` ordinals; everything older stays reachable through
/// sparse anchors and the canonical rollout.
fn trim_active_window(tx: &Transaction<'_>, rollout_id: &str) -> Result<usize> {
    let deleted = tx.execute(
        "DELETE FROM thread_items
         WHERE rollout_id=?1
           AND rollout_ordinal <= (
               SELECT rollout_ordinal FROM thread_items
                WHERE rollout_id=?1
                ORDER BY rollout_ordinal DESC
                LIMIT 1 OFFSET ?2)",
        params![rollout_id, ACTIVE_WINDOW_ITEMS],
    )?;
    Ok(deleted)
}

/// Hand trimmed pages back to the filesystem. Only an explicit
/// `incremental_vacuum` shrinks an INCREMENTAL database, and it must run
/// outside the write transaction.
fn reclaim_free_pages(conn: &Connection) -> Result<()> {
    let auto_vacuum: i64 = conn.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
    if auto_vacuum != 2 {
        return Ok(());
    }
    let free: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    if free < RECLAIM_FREE_PAGES {
        return Ok(());
    }
    conn.execute_batch(&format!("PRAGMA incremental_vacuum({free});"))?;
    Ok(())
}

/// Re-read completed items outside the active window. Only the newest
/// `ACTIVE_WINDOW_ITEMS` rows of each physical rollout keep an index row, so
/// every older view hydrates from the canonical JSONL through the sparse
/// anchors. Each call is bounded by an anchor interval, a record budget and a
/// byte budget, so its cost never depends on the total rollout length.
///
/// `cursor` is the exclusive pagination edge, `floor` an inclusive lower
/// ordinal and `ceiling` an exclusive upper ordinal. `start_hint` is the
/// owning turn's exact `(ordinal, byte)` record boundary: unlike sparse
/// anchors it always sits at the top of the requested range, so a trimmed
/// turn is re-read in one pass. The accept callback sees the canonical item
/// exactly as the projection would have stored it.
pub(super) fn scan_lineage(
    root: &Path,
    history: &Connection,
    segments: &[HistorySegment],
    current: &str,
    direction: ScanDirection,
    cursor: Option<i64>,
    floor: Option<i64>,
    ceiling: Option<i64>,
    limit: usize,
    start_hint: Option<(i64, u64)>,
    accept: &dyn Fn(&ThreadHistoryItem) -> bool,
) -> Result<ScannedItems> {
    let mut output = ScannedItems::default();
    if limit == 0 {
        return Ok(output);
    }
    let synthesized;
    let segments = if segments.is_empty() {
        synthesized = vec![HistorySegment {
            rollout_id: current.to_owned(),
            start_ordinal: 0,
            end_ordinal: None,
        }];
        synthesized.as_slice()
    } else {
        segments
    };
    // The re-read is bounded by records and bytes, never by the rollout
    // length. Exact turn boundaries keep ordinary reads far below both caps.
    let mut budget = MAX_SCAN_RECORDS;
    let mut bytes = 0usize;
    let mut catalog = None;
    match direction {
        ScanDirection::Older => {
            let edge = cursor.unwrap_or(i64::MAX);
            'lineage: for segment in segments.iter().rev() {
                let (low, high) = segment_bounds(segment);
                let low_bound = floor.map_or(low, |floor| floor.max(low));
                let mut stop = edge.min(high);
                while stop > low_bound {
                    // Prefer whatever boundary sits closest to the page edge:
                    // the owning turn's exact start record when it is newer
                    // than the nearest sparse anchor. Ranges before the first
                    // anchor still live at the start of the segment, so they
                    // fall back to byte zero instead of reporting the oldest
                    // items unreachable.
                    let anchor = segment_anchor(history, &segment.rollout_id, low, stop - 1)?;
                    let hint = segment_hint(start_hint, segment, stop - 1);
                    let (start_byte, start_ordinal) = match (anchor, hint) {
                        (Some((anchor_byte, anchor_ordinal)), Some((hint_ordinal, hint_byte))) => {
                            if hint_ordinal >= anchor_ordinal {
                                (hint_byte, hint_ordinal)
                            } else {
                                (anchor_byte, anchor_ordinal)
                            }
                        }
                        (Some((byte, ordinal)), None) => (byte, ordinal),
                        (None, Some((ordinal, byte))) => (byte, ordinal),
                        (None, None) => (0, low),
                    };
                    let mut chunk_items = Vec::new();
                    let chunk = scan_segment_records(
                        root,
                        &mut catalog,
                        segment,
                        current,
                        start_byte,
                        low_bound,
                        stop,
                        &mut budget,
                        &mut bytes,
                        None,
                        &mut chunk_items,
                        accept,
                    )?;
                    if chunk.truncated {
                        // A partial chunk ends below the page edge; including
                        // it would leave a hole above the returned items.
                        output.more = true;
                        break 'lineage;
                    }
                    // Chunks walk from the page edge downwards, so each one is
                    // flipped into page order before it joins the newest-first
                    // buffer. A later (older) chunk can never displace a newer
                    // item once the page limit is reached.
                    chunk_items.reverse();
                    output.items.extend(chunk_items);
                    if output.items.len() > limit {
                        output.items.truncate(limit);
                        output.more = true;
                        break 'lineage;
                    }
                    // The chunk covered everything below `stop` within this
                    // segment once the scan reached the segment floor.
                    if start_ordinal <= low_bound {
                        break;
                    }
                    stop = start_ordinal;
                }
            }
        }
        ScanDirection::Newer => {
            let from = cursor.map_or(i64::MIN, |cursor| cursor.saturating_add(1));
            'lineage: for segment in segments.iter() {
                let (low, high) = segment_bounds(segment);
                let low_bound = floor.map_or(low, |floor| floor.max(low)).max(from);
                let stop = ceiling.map_or(high, |ceiling| ceiling.min(high));
                if stop <= low_bound {
                    continue;
                }
                let anchor = segment_anchor(history, &segment.rollout_id, low, low_bound)?;
                let hint = segment_hint(start_hint, segment, low_bound);
                let start_byte = match (anchor, hint) {
                    (Some((anchor_byte, anchor_ordinal)), Some((hint_ordinal, hint_byte))) => {
                        if hint_ordinal > anchor_ordinal {
                            hint_byte
                        } else {
                            anchor_byte
                        }
                    }
                    (Some((anchor_byte, _)), None) => anchor_byte,
                    (None, Some((_, hint_byte))) => hint_byte,
                    (None, None) => 0,
                };
                let chunk = scan_segment_records(
                    root,
                    &mut catalog,
                    segment,
                    current,
                    start_byte,
                    low_bound,
                    stop,
                    &mut budget,
                    &mut bytes,
                    Some(limit),
                    &mut output.items,
                    accept,
                )?;
                if chunk.truncated || chunk.filled {
                    output.more = true;
                    break 'lineage;
                }
            }
        }
    }
    Ok(output)
}

fn segment_bounds(segment: &HistorySegment) -> (i64, i64) {
    let low = i64::try_from(segment.start_ordinal).unwrap_or(i64::MAX);
    let high = segment
        .end_ordinal
        .map(|end| i64::try_from(end).unwrap_or(i64::MAX))
        .unwrap_or(i64::MAX);
    (low, high)
}

/// Bind a turn's exact start boundary to the physical segment that owns it.
/// The hint is only a valid scan start at or below the caller's edge; a hint
/// from another lineage segment must not be used against this one.
fn segment_hint(
    hint: Option<(i64, u64)>,
    segment: &HistorySegment,
    edge: i64,
) -> Option<(i64, u64)> {
    let (ordinal, byte) = hint?;
    let (low, high) = segment_bounds(segment);
    (ordinal >= low && ordinal < high && ordinal <= edge).then_some((ordinal, byte))
}

/// Exact canonical re-read start of one root turn. The projector records the
/// byte offset of a turn's earliest record, so a turn that left the active
/// window is re-read from its own boundary instead of waiting for the nearest
/// sparse anchor, which can sit thousands of records away.
pub(super) fn turn_scan_hint(
    history: &Connection,
    rollout_id: &str,
    root: &str,
) -> Result<Option<(i64, u64)>> {
    Ok(history
        .query_row(
            "SELECT start_ordinal,start_rollout_byte_offset FROM thread_turns
             WHERE rollout_id=?1 AND COALESCE(root_turn_id,turn_id)=?2
               AND start_ordinal IS NOT NULL AND start_rollout_byte_offset IS NOT NULL
             ORDER BY start_ordinal ASC LIMIT 1",
            params![rollout_id, root],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .map(|(ordinal, offset)| -> Result<(i64, u64)> {
            Ok((ordinal, sqlite_u64_required(offset)?))
        })
        .transpose()?)
}

/// Nearest sparse anchor at or below `through`; anchors are stored per
/// physical rollout, never per logical lineage.
fn segment_anchor(
    history: &Connection,
    rollout_id: &str,
    floor: i64,
    through: i64,
) -> Result<Option<(u64, i64)>> {
    Ok(history
        .query_row(
            "SELECT record_byte_offset,rollout_ordinal FROM thread_item_anchors
             WHERE rollout_id=?1 AND rollout_ordinal>=?2 AND rollout_ordinal<=?3
             ORDER BY rollout_ordinal DESC LIMIT 1",
            params![rollout_id, floor, through],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .map(|(offset, ordinal)| -> Result<(u64, i64)> {
            Ok((sqlite_u64_required(offset)?, ordinal))
        })
        .transpose()?)
}

struct ScanChunk {
    /// The accepted-item limit was reached before the ordinal range ended.
    filled: bool,
    /// The record or byte budget ran out before the ordinal range ended.
    truncated: bool,
}

/// Sequential forward read of one physical segment inside `[floor,stop)`.
/// Accepted items append in ascending ordinal order. Callers own the ring and
/// page truncation, so this function only reports whether it stopped early.
#[allow(clippy::too_many_arguments)]
fn scan_segment_records(
    root: &Path,
    catalog: &mut Option<Connection>,
    segment: &HistorySegment,
    current: &str,
    start_byte: u64,
    floor: i64,
    stop: i64,
    budget: &mut i64,
    bytes: &mut usize,
    accept_limit: Option<usize>,
    items: &mut Vec<ThreadHistoryItem>,
    accept: &dyn Fn(&ThreadHistoryItem) -> bool,
) -> Result<ScanChunk> {
    let mut chunk = ScanChunk {
        filled: accept_limit.is_some_and(|limit| items.len() >= limit),
        truncated: false,
    };
    if chunk.filled {
        return Ok(chunk);
    }
    if *budget <= 0 {
        chunk.truncated = true;
        return Ok(chunk);
    }
    if catalog.is_none() {
        *catalog = Some(super::thread_history_body::open_catalog(root)?);
    }
    let source = super::thread_history_body::source_path(
        catalog.as_ref().expect("catalog opened"),
        root,
        &segment.rollout_id,
    )?;
    let mut file = super::rollout_compression::open_rollout_reader(&source)?;
    file.seek(SeekFrom::Start(start_byte))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        if !read_bounded_line(&mut reader, &mut line)? {
            break;
        }
        if line.is_empty() || line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        *budget -= 1;
        if *budget <= 0 {
            chunk.truncated = true;
            break;
        }
        // Count every byte the scan had to read. The budget must bound real
        // IO, not only the accepted display items, so a range far from its
        // start cannot turn into an unbounded read.
        *bytes = (*bytes).saturating_add(line.len());
        if *bytes > SCAN_BYTES_BUDGET {
            chunk.truncated = true;
            break;
        }
        // An unfinished tail or a foreign line must not poison a bounded read.
        let envelope: RolloutEnvelope = match serde_json::from_slice(&line) {
            Ok(envelope) => envelope,
            Err(_) => continue,
        };
        let ordinal = match i64::try_from(envelope.ordinal) {
            Ok(ordinal) => ordinal,
            Err(_) => continue,
        };
        if ordinal >= stop {
            break;
        }
        if ordinal < floor || !is_completed_item(&envelope) {
            continue;
        }
        let item = match completed_item(event_body(&envelope)) {
            Ok(item) => item,
            Err(_) => continue,
        };
        let Some(item) = envelope_to_history_item(current, &envelope, item, ordinal)? else {
            continue;
        };
        if !accept(&item) {
            continue;
        }
        items.push(item);
        if accept_limit.is_some_and(|limit| items.len() >= limit) {
            chunk.filled = true;
            break;
        }
    }
    Ok(chunk)
}

const SCAN_BYTES_BUDGET: usize = 16 * 1024 * 1024;
/// Hard record ceiling for one bounded re-read. The byte budget normally
/// stops a scan first; this protects against pathologically small records.
const MAX_SCAN_RECORDS: i64 = 65_536;

fn item_identity(item: &Value) -> Result<String> {
    item.get("id")
        .or_else(|| item.get("item_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("canonical item has no identity"))
}

/// Project one canonical completed envelope into the same shape the active
/// window rows produce.
fn envelope_to_history_item(
    rollout_id: &str,
    envelope: &RolloutEnvelope,
    item: &Value,
    ordinal: i64,
) -> Result<Option<ThreadHistoryItem>> {
    let body = event_body(envelope);
    let item_type = item
        .get("item_type")
        .or_else(|| item.get("type"))
        .or_else(|| item.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let turn_id = body.get("turn_id").and_then(Value::as_str).map(str::to_string);
    let root_turn_id = body
        .get("root_turn_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let created_at_ms = body
        .get("created_at_ms")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    Ok(Some(ThreadHistoryItem {
        rollout_id: rollout_id.to_owned(),
        turn_id,
        root_turn_id,
        item_id: item_identity(item)?,
        item_type,
        created_at_ms,
        rollout_ordinal: u64::try_from(ordinal)?,
        item_json: item.clone(),
    }))
}

/// Locate one item outside the active window. The owning turn narrows the
/// ordinal range so at most one anchor interval is re-read; a missing turn row
/// falls back to the lineage bounds, still bounded by the scan budget.
pub(super) fn find_item_via_anchor(
    history: &Connection,
    root: &Path,
    segments: &[HistorySegment],
    rollout_id: &str,
    item_id: &str,
) -> Result<Option<ThreadHistoryItem>> {
    let (turn_id, turn_start, turn_end, start_hint) = match item_id.rsplit_once(':') {
        Some((turn, _)) => {
            let range: Option<(i64, i64, Option<i64>)> = history
                .query_row(
                    "SELECT COALESCE(start_ordinal,end_ordinal,0),
                            COALESCE(end_ordinal,updated_at_ordinal,0),
                            start_rollout_byte_offset
                     FROM thread_turns WHERE rollout_id=?1 AND turn_id=?2",
                    params![rollout_id, turn],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            match range {
                Some((start, end, offset)) => {
                    let hint = match offset {
                        Some(offset) => Some((start, sqlite_u64_required(offset)?)),
                        None => None,
                    };
                    (Some(turn), start, end, hint)
                }
                None => (Some(turn), 0, i64::MAX, None),
            }
        }
        None => (None, 0, i64::MAX, None),
    };
    let accept = |candidate: &ThreadHistoryItem| {
        candidate.item_id == item_id
            && turn_id.map_or(true, |turn| {
                candidate
                    .turn_id
                    .as_deref()
                    .map_or(true, |item_turn| item_turn == turn)
            })
    };
    let older = scan_lineage(
        root,
        history,
        segments,
        rollout_id,
        ScanDirection::Older,
        Some(turn_end.saturating_add(1)),
        Some(turn_start),
        None,
        1,
        start_hint,
        &accept,
    )?;
    if let Some(item) = older.items.into_iter().next() {
        return Ok(Some(item));
    }
    let newer = scan_lineage(
        root,
        history,
        segments,
        rollout_id,
        ScanDirection::Newer,
        Some(turn_start.saturating_sub(1)),
        None,
        Some(turn_end.saturating_add(1)),
        1,
        start_hint,
        &accept,
    )?;
    Ok(newer.items.into_iter().next())
}

fn load_state(conn: &Connection, rollout_id: &str) -> Result<ProjectionState> {
    conn.query_row(
        "SELECT next_rollout_byte_offset,next_rollout_ordinal,history_revision
         FROM thread_history_projection_state WHERE rollout_id=?1",
        [rollout_id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )
    .optional()?
    .map(|(offset, ordinal, revision)| {
        Ok(ProjectionState {
            next_rollout_byte_offset: sqlite_u64_required(offset)?,
            next_rollout_ordinal: sqlite_u64_required(ordinal)?,
            history_revision: sqlite_u64_required(revision)?,
        })
    })
    .unwrap_or_else(|| Ok(ProjectionState::default()))
}

fn save_state(tx: &Transaction<'_>, rollout_id: &str, state: ProjectionState) -> Result<()> {
    tx.execute(
        "INSERT INTO thread_history_projection_state
             (rollout_id,next_rollout_byte_offset,next_rollout_ordinal,history_revision)
         VALUES (?1,?2,?3,?4)
         ON CONFLICT(rollout_id) DO UPDATE SET
           next_rollout_byte_offset=excluded.next_rollout_byte_offset,
           next_rollout_ordinal=excluded.next_rollout_ordinal,
           history_revision=excluded.history_revision",
        params![
            rollout_id,
            sqlite_i64(state.next_rollout_byte_offset)?,
            sqlite_i64(state.next_rollout_ordinal)?,
            sqlite_i64(state.history_revision)?,
        ],
    )?;
    Ok(())
}

fn project_envelope(
    tx: &Transaction<'_>,
    rollout_id: &str,
    envelope: &RolloutEnvelope,
    byte_offset: u64,
    byte_length: u64,
) -> Result<()> {
    let event_kind = canonical_kind(envelope);
    match event_kind.as_deref() {
        Some("turn_started") => project_turn_started(tx, rollout_id, envelope, byte_offset),
        Some("turn_completed") | Some("turn_complete") => {
            project_turn_finished(tx, rollout_id, envelope, byte_offset, "completed")
        }
        Some("turn_aborted") | Some("turn_cancelled") => {
            project_turn_finished(tx, rollout_id, envelope, byte_offset, "aborted")
        }
        Some("turn_failed") => {
            project_turn_finished(tx, rollout_id, envelope, byte_offset, "failed")
        }
        Some("item_completed") => {
            project_completed_item(tx, rollout_id, envelope, byte_offset, byte_length)
        }
        _ => Ok(()),
    }
}

pub(super) fn is_completed_item(envelope: &RolloutEnvelope) -> bool {
    canonical_kind(envelope).as_deref() == Some("item_completed")
}

fn project_turn_started(
    tx: &Transaction<'_>,
    rollout_id: &str,
    envelope: &RolloutEnvelope,
    byte_offset: u64,
) -> Result<()> {
    let body = event_body(envelope);
    let turn_id = required_string(body, "turn_id", "turn_started")?;
    let root_turn_id = optional_string(body, "root_turn_id");
    let started_at_ms = event_time_ms(body, envelope).unwrap_or_default();
    let ordinal = sqlite_i64(envelope.ordinal)?;
    tx.execute(
        "INSERT INTO thread_turns
             (rollout_id,turn_id,root_turn_id,status,started_at_ms,start_rollout_byte_offset,start_ordinal,updated_at_ordinal)
         VALUES (?1,?2,?3,'running',?4,?5,?6,?6)
         ON CONFLICT(rollout_id,turn_id) DO UPDATE SET
           root_turn_id=COALESCE(excluded.root_turn_id,thread_turns.root_turn_id),
           started_at_ms=CASE WHEN thread_turns.started_at_ms=0 THEN excluded.started_at_ms ELSE thread_turns.started_at_ms END,
           start_rollout_byte_offset=COALESCE(thread_turns.start_rollout_byte_offset,excluded.start_rollout_byte_offset),
           start_ordinal=COALESCE(thread_turns.start_ordinal,excluded.start_ordinal),
           updated_at_ordinal=MAX(thread_turns.updated_at_ordinal,excluded.updated_at_ordinal)",
        params![
            rollout_id,
            turn_id,
            root_turn_id,
            started_at_ms,
            sqlite_i64(byte_offset)?,
            ordinal,
        ],
    )?;
    Ok(())
}

fn project_turn_finished(
    tx: &Transaction<'_>,
    rollout_id: &str,
    envelope: &RolloutEnvelope,
    byte_offset: u64,
    status: &str,
) -> Result<()> {
    let body = event_body(envelope);
    let turn_id = required_string(body, "turn_id", "turn completion")?;
    let root_turn_id = optional_string(body, "root_turn_id");
    let completed_at_ms = optional_i64(body, "completed_at_ms")
        .or_else(|| optional_i64(body, "aborted_at_ms"))
        .or_else(|| event_time_ms(body, envelope))
        .unwrap_or_default();
    let started_at_ms = optional_i64(body, "started_at_ms").unwrap_or(completed_at_ms);
    let error_json = body
        .get("error")
        .filter(|value| !value.is_null())
        .map(serde_json::to_string)
        .transpose()?;
    let ordinal = sqlite_i64(envelope.ordinal)?;
    tx.execute(
        "INSERT INTO thread_turns
             (rollout_id,turn_id,root_turn_id,status,error_json,started_at_ms,completed_at_ms,duration_ms,
              end_rollout_byte_offset,start_ordinal,end_ordinal,updated_at_ordinal)
         VALUES (?1,?2,?3,?4,?5,?6,?7,0,?8,?9,?9,?9)
         ON CONFLICT(rollout_id,turn_id) DO UPDATE SET
           root_turn_id=COALESCE(excluded.root_turn_id,thread_turns.root_turn_id),
           status=excluded.status,
           error_json=COALESCE(excluded.error_json,thread_turns.error_json),
           completed_at_ms=excluded.completed_at_ms,
           duration_ms=CASE WHEN thread_turns.started_at_ms > 0
             THEN MAX(0,excluded.completed_at_ms-thread_turns.started_at_ms) ELSE NULL END,
           end_rollout_byte_offset=excluded.end_rollout_byte_offset,
           end_ordinal=excluded.end_ordinal,
           updated_at_ordinal=MAX(thread_turns.updated_at_ordinal,excluded.updated_at_ordinal)",
        params![
            rollout_id,
            turn_id,
            root_turn_id,
            status,
            error_json,
            started_at_ms,
            completed_at_ms,
            sqlite_i64(byte_offset)?,
            ordinal,
        ],
    )?;
    Ok(())
}

fn project_completed_item(
    tx: &Transaction<'_>,
    rollout_id: &str,
    envelope: &RolloutEnvelope,
    byte_offset: u64,
    byte_length: u64,
) -> Result<()> {
    let body = event_body(envelope);
    let item = completed_item(body)?;
    let item_id = item
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| item.get("item_id").and_then(Value::as_str))
        .or_else(|| body.get("item_id").and_then(Value::as_str))
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| anyhow!("item_completed event has no item id"))?;
    let turn_id = optional_string(body, "turn_id").or_else(|| optional_string(item, "turn_id"));
    let root_turn_id =
        optional_string(body, "root_turn_id").or_else(|| optional_string(item, "root_turn_id"));
    let item_type = item
        .get("item_type")
        .or_else(|| item.get("type"))
        .or_else(|| item.get("kind"))
        .and_then(Value::as_str)
        .filter(|kind| !kind.trim().is_empty())
        .unwrap_or("unknown");
    let created_at_ms = event_time_ms(item, envelope)
        .or_else(|| event_time_ms(body, envelope))
        .unwrap_or_default();
    let ordinal = sqlite_i64(envelope.ordinal)?;
    let (item_bytes, digest) = item_digest(item)?;
    let preview = item_preview(item);
    let preview_json = serde_json::to_string(&preview)?;
    let visible = item_is_visible(item);

    if let Some(turn_id) = turn_id.as_deref() {
        ensure_turn_for_item(
            tx,
            rollout_id,
            turn_id,
            root_turn_id.as_deref(),
            created_at_ms,
            byte_offset,
            envelope.ordinal,
        )?;
    }
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO thread_items
             (rollout_id,turn_id,root_turn_id,item_id,item_type,created_at_ms,rollout_ordinal,record_byte_offset,record_byte_length,item_bytes,item_digest,preview_json,visible,message_context)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![
            rollout_id,
            turn_id,
            root_turn_id,
            item_id,
            item_type,
            created_at_ms,
            ordinal,
            sqlite_i64(byte_offset)?,
            sqlite_i64(byte_length)?,
            i64::try_from(item_bytes)?,
            digest,
            preview_json,
            visible,
            message_context(item, item_type),
        ],
    )?;
    if inserted == 0 {
        let existing: Vec<u8> = tx.query_row(
            "SELECT item_digest FROM thread_items WHERE rollout_id=?1 AND item_id=?2",
            params![rollout_id, item_id],
            |row| row.get(0),
        )?;
        if existing != digest {
            bail!(
                "rollout {rollout_id} completed item {item_id} conflicts with its immutable projection"
            );
        }
        return Ok(());
    }

    // Sparse anchor: keep a re-read locator at fixed ordinal intervals before
    // the row can leave the active window.
    if ordinal % ANCHOR_INTERVAL == 0 {
        tx.execute(
            "INSERT OR IGNORE INTO thread_item_anchors
                 (rollout_id,rollout_ordinal,item_id,record_byte_offset,record_byte_length)
             VALUES (?1,?2,?3,?4,?5)",
            params![rollout_id, ordinal, item_id, sqlite_i64(byte_offset)?, sqlite_i64(byte_length)?],
        )?;
    }

    if let Some(turn_id) = turn_id.as_deref() {
        super::thread_chat_summary::project_item(
            tx,
            rollout_id,
            root_turn_id.as_deref().unwrap_or(turn_id),
            turn_id,
            item_id,
            item_type,
            ordinal,
            visible,
            &preview,
        )?;
        tx.execute("INSERT INTO thread_root_counts(rollout_id,root_turn_id,visible_items,workflow_items)
            VALUES(?1,?2,?3,?4) ON CONFLICT(rollout_id,root_turn_id) DO UPDATE SET
            visible_items=visible_items+excluded.visible_items, workflow_items=workflow_items+excluded.workflow_items",
            params![rollout_id,root_turn_id.as_deref().unwrap_or(turn_id), i64::from(visible),
                i64::from(visible && item_type != "user_message")])?;
        // Durable per-turn total survives even after the item row leaves the
        // active window, so turn pages do not depend on a thread_items COUNT.
        tx.execute(
            "UPDATE thread_turns SET item_count=item_count+1,
                updated_at_ordinal=MAX(updated_at_ordinal,?2)
             WHERE rollout_id=?1 AND turn_id=?3",
            params![rollout_id, ordinal, turn_id],
        )?;
        if is_user_item(item, item_type) {
            tx.execute(
                "UPDATE thread_turns SET first_user_item_id=COALESCE(first_user_item_id,?3),
                    updated_at_ordinal=MAX(updated_at_ordinal,?4)
                 WHERE rollout_id=?1 AND turn_id=?2",
                params![rollout_id, turn_id, item_id, ordinal],
            )?;
        }
        if is_agent_item(item, item_type) {
            tx.execute(
                "UPDATE thread_turns SET final_agent_item_id=?3,
                    updated_at_ordinal=MAX(updated_at_ordinal,?4)
                 WHERE rollout_id=?1 AND turn_id=?2",
                params![rollout_id, turn_id, item_id, ordinal],
            )?;
        }
    }
    Ok(())
}

/// Display visibility is decided once by the projector and again on every
/// canonical re-read, so a trimmed window row and a rollout item agree.
pub(super) fn item_is_visible(item: &Value) -> bool {
    let payload = item.get("payload").unwrap_or(item);
    payload
        .get("visibility")
        .and_then(Value::as_str)
        .unwrap_or("user")
        == "user"
        && payload.pointer("/meta/hidden").and_then(Value::as_bool) != Some(true)
        && payload
            .pointer("/payload/meta/hidden")
            .and_then(Value::as_bool)
            != Some(true)
        && payload.get("kind").and_then(Value::as_str) != Some("system_message")
}

/// A bounded card-grade projection lives alongside the completed item, never
/// another body. The whitelist below carries exactly what the chat and
/// trajectory pages render: identity, lifecycle, short text previews, bounded
/// numeric stats and precomputed tool previews. Arguments, results, display
/// copies and full content stay in the canonical log and are read through
/// verified body locators when a view actually needs them.
pub(crate) fn item_preview(item: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for key in [
        "item_id",
        "turn_id",
        "root_turn_id",
        "created_at_ms",
        "kind",
    ] {
        if let Some(value) = item.get(key) {
            out.insert(key.into(), value.clone());
        }
    }
    if item
        .get("display_output_truncated")
        .and_then(Value::as_bool)
        == Some(true)
    {
        out.insert("display_output_truncated".into(), Value::Bool(true));
    }
    let payload = item.get("payload").unwrap_or(item);
    out.insert("payload".into(), preview_payload(payload));
    Value::Object(out)
}

/// Bounded copy of a diagnostics object (`error`, `error_meta`,
/// `trajectory_retry`, `trajectory_failure`): exactly the scalar fields
/// services::thread_log::diagnostics reads, never provider bodies or detail.
fn bounded_diagnostics(map: &serde_json::Map<String, Value>) -> Value {
    use serde_json::json;
    use wunder_core::thread_view::preview;
    let mut out = serde_json::Map::new();
    for key in [
        "code",
        "source_stage",
        "recovery_action",
        "retry_reason",
        "reason",
    ] {
        if let Some(text) = map
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            out.insert(key.into(), json!(preview(text, 128)));
        }
    }
    if let Some(text) = map
        .get("status")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        out.insert("status".into(), json!(preview(text, 48)));
    }
    if let Some(text) = map
        .get("message")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        out.insert("message".into(), json!(preview(text, 512)));
    }
    for key in ["attempt", "max_attempts", "delay_s", "retry_after_ms"] {
        if let Some(value) = map.get(key).filter(|value| value.is_number()) {
            out.insert(key.into(), value.clone());
        }
    }
    for key in ["retryable", "will_retry"] {
        if let Some(value) = map.get(key).and_then(Value::as_bool) {
            out.insert(key.into(), json!(value));
        }
    }
    // One bounded nesting level: error objects carry their own error_meta.
    if let Some(meta) = map.get("error_meta").and_then(Value::as_object) {
        let mut kept = serde_json::Map::new();
        for key in ["source_stage", "recovery_action"] {
            if let Some(text) = meta
                .get(key)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                kept.insert(key.into(), json!(preview(text, 128)));
            }
        }
        if let Some(value) = meta.get("retryable").and_then(Value::as_bool) {
            kept.insert("retryable".into(), json!(value));
        }
        if let Some(value) = meta.get("retry_after_ms").filter(|value| value.is_number()) {
            kept.insert("retry_after_ms".into(), value.clone());
        }
        if !kept.is_empty() {
            out.insert("error_meta".into(), Value::Object(kept));
        }
    }
    Value::Object(out)
}

fn preview_payload(payload: &Value) -> Value {
    use serde_json::json;
    if !payload.is_object() {
        return match payload.as_str() {
            Some(text) => json!({"content": wunder_core::thread_view::preview(text, 256)}),
            None => json!({}),
        };
    }
    let wrapped = payload.get("item_id").and_then(Value::as_str).is_some()
        && payload.get("payload").is_some_and(Value::is_object);
    if !wrapped {
        return preview_body(payload);
    }
    let mut out = serde_json::Map::new();
    for key in [
        "item_id",
        "turn_id",
        "root_turn_id",
        "kind",
        "status",
        "visibility",
    ] {
        if let Some(value) = payload.get(key) {
            out.insert(key.into(), value.clone());
        }
    }
    out.insert("payload".into(), preview_body(&payload["payload"]));
    Value::Object(out)
}

fn preview_body(body: &Value) -> Value {
    use serde_json::json;
    use wunder_core::thread_view::preview;
    let Some(map) = body.as_object() else {
        return json!({});
    };
    let mut out = serde_json::Map::new();
    // Identity and lifecycle scalars are small by contract and must survive
    // verbatim for grouping, dedup and status rendering.
    for key in [
        "kind",
        "status",
        "visibility",
        "role",
        "event_type",
        "tool_call_id",
        "model_round",
        "process_seq",
        "ok",
        "success",
        "is_error",
    ] {
        if let Some(value) = map.get(key) {
            if value.is_string() || value.is_number() || value.is_boolean() {
                out.insert(key.into(), value.clone());
            }
        }
    }
    for key in ["tool", "tool_name", "name"] {
        if let Some(text) = map.get(key).and_then(Value::as_str) {
            if !text.is_empty() {
                out.insert(key.into(), json!(preview(text, 96)));
            }
        }
    }
    if let Some(reference) = map.get("schema_ref").filter(|value| value.is_object()) {
        out.insert("schema_ref".into(), reference.clone());
    }
    // Timing scalars drive trajectory span/duration math.
    for key in [
        "started_at",
        "completed_at",
        "start_time",
        "end_time",
        "started_time",
        "finished_time",
        "finished_at",
        "elapsed_s",
        "duration_s",
        "duration_ms",
        "durationMs",
        "prefill_duration_s",
        "decode_duration_s",
        "ttft_ms",
    ] {
        if let Some(value) = map.get(key) {
            if value.is_number() || value.is_string() {
                out.insert(key.into(), value.clone());
            }
        }
    }
    // Failure diagnostics read a small bounded field set (see
    // services::thread_log::diagnostics): `error` may be a string or an
    // object, and the retry/failure snapshots are structured objects.
    for key in ["code", "stop_reason"] {
        if let Some(text) = map.get(key).and_then(Value::as_str) {
            if !text.is_empty() {
                out.insert(key.into(), json!(preview(text, 128)));
            }
        }
    }
    match map.get("error") {
        Some(Value::String(error)) if !error.is_empty() => {
            out.insert("error".into(), json!(preview(error, 512)));
        }
        Some(Value::Object(error)) => {
            out.insert("error".into(), bounded_diagnostics(error));
        }
        _ => {}
    }
    if let Some(message) = map
        .get("message")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        out.insert("message".into(), json!(preview(message, 512)));
    }
    if let Some(value) = map.get("retryable").and_then(Value::as_bool) {
        out.insert("retryable".into(), json!(value));
    }
    if let Some(value) = map.get("retry_after_ms").filter(|value| value.is_number()) {
        out.insert("retry_after_ms".into(), value.clone());
    }
    for key in ["error_meta", "trajectory_retry", "trajectory_failure"] {
        if let Some(value) = map.get(key).and_then(Value::as_object) {
            out.insert(key.into(), bounded_diagnostics(value));
        }
    }
    // Card-grade text previews; full content and reasoning stay in the log.
    let content = wunder_core::thread_view::message_content_text(&body["content"])
        .unwrap_or_default();
    if !content.is_empty() {
        out.insert("content".into(), json!(preview(&content, 256)));
        out.insert("_content_bytes".into(), Value::from(content.len()));
        out.insert(
            "_content_utf16".into(),
            Value::from(content.encode_utf16().count()),
        );
    }
    for key in ["reasoning", "reasoning_content"] {
        if let Some(reasoning) = map.get(key).and_then(Value::as_str) {
            if !reasoning.is_empty() {
                out.insert(key.into(), json!(preview(reasoning, 256)));
                out.insert(
                    "_reasoning_utf16".into(),
                    Value::from(reasoning.encode_utf16().count()),
                );
                break;
            }
        }
    }
    if map
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty())
    {
        out.insert("tool_calls".into(), json!([{}]));
    }
    if let Some(attachments) = map.get("attachments").and_then(Value::as_array) {
        let bounded = super::thread_view_projection::user_attachments(attachments);
        if !bounded.is_empty() {
            out.insert("attachments".into(), Value::Array(bounded));
        }
    }
    // Bounded numeric stats feed trajectory chips and the chat bubble footer.
    if let Some(stats) = super::thread_view_projection::finite_stats(body) {
        out.insert("message_stats".into(), stats);
    }
    if let Some(breakdown) = super::thread_view_projection::request_token_breakdown(body) {
        out.insert("request_token_breakdown".into(), breakdown);
    }
    if let Some(timing) = map.get("stream_timing").and_then(Value::as_object) {
        let bounded: serde_json::Map<_, _> = timing
            .iter()
            .filter(|(_, value)| value.is_number())
            .take(8)
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        if !bounded.is_empty() {
            out.insert("stream_timing".into(), Value::Object(bounded));
        }
    }
    // Meta whitelist: compaction identity, model-context markers and tool
    // duration metadata. Prompt and session payloads never enter a preview.
    if let Some(meta) = map.get("meta").and_then(Value::as_object) {
        let mut kept = serde_json::Map::new();
        for key in ["type", "compaction_id", "language"] {
            if let Some(text) = meta.get(key).and_then(Value::as_str) {
                if !text.is_empty() {
                    kept.insert(key.into(), json!(preview(text, 64)));
                }
            }
        }
        for key in ["duration_ms", "durationMs", "retry_after_ms"] {
            if let Some(value) = meta.get(key).filter(|value| value.is_number()) {
                kept.insert(key.into(), value.clone());
            }
        }
        if let Some(value) = meta.get("retryable").and_then(Value::as_bool) {
            kept.insert("retryable".into(), json!(value));
        }
        if meta
            .get("workspace_changed")
            .is_some_and(|value| value == &Value::Bool(true) || value.is_number())
        {
            kept.insert("workspace_changed".into(), meta["workspace_changed"].clone());
        }
        if !kept.is_empty() {
            out.insert("meta".into(), Value::Object(kept));
        }
    }
    // A question panel is a control surface; store the bounded extracted panel
    // so the chat projection can recover it without the carrier's full result.
    if let Some(panel) = super::thread_view_projection::extract_question_panel(body) {
        if let Ok(panel) = serde_json::to_value(panel) {
            out.insert("question_panel".into(), panel);
        }
    }
    // Plans and compaction summaries render from small structured fields.
    for key in ["plan", "steps"] {
        if let Some(entries) = map.get(key).and_then(Value::as_array) {
            let bounded: Vec<Value> = entries
                .iter()
                .take(32)
                .map(|entry| {
                    let mut step = serde_json::Map::new();
                    if let Some(text) = entry.as_str() {
                        step.insert("step".into(), json!(preview(text, 256)));
                    }
                    for field in ["step", "title"] {
                        if let Some(text) = entry.get(field).and_then(Value::as_str) {
                            step.insert(field.into(), json!(preview(text, 256)));
                        }
                    }
                    if let Some(status) = entry.get("status").and_then(Value::as_str) {
                        step.insert("status".into(), json!(preview(status, 32)));
                    }
                    Value::Object(step)
                })
                .collect();
            if !bounded.is_empty() {
                out.insert(key.into(), Value::Array(bounded));
            }
            break;
        }
    }
    if let Some(explanation) = map.get("explanation").and_then(Value::as_str) {
        if !explanation.is_empty() {
            out.insert("explanation".into(), json!(preview(explanation, 512)));
        }
    }
    for key in [
        "summary_text",
        "summaryText",
        "summary_model_output",
        "summaryModelOutput",
        "summary",
    ] {
        if let Some(text) = map.get(key).and_then(Value::as_str) {
            if !text.trim().is_empty() {
                out.insert(key.into(), json!(preview(text, 512)));
                break;
            }
        }
    }
    // Subagent runtime cards and trajectory projections use a fixed field set.
    if let Some(runtime) = map.get("runtime").and_then(Value::as_object) {
        let mut kept = serde_json::Map::new();
        for key in [
            "label",
            "spawn_label",
            "title",
            "status",
            "run_id",
            "session_id",
            "dispatch_id",
            "run_kind",
        ] {
            if let Some(text) = runtime.get(key).and_then(Value::as_str) {
                if !text.is_empty() {
                    kept.insert(key.into(), json!(preview(text, 128)));
                }
            }
        }
        for key in ["user_message", "task", "summary", "latest_message"] {
            if let Some(text) = runtime.get(key).and_then(Value::as_str) {
                if !text.is_empty() {
                    kept.insert(key.into(), json!(preview(text, 512)));
                }
            }
        }
        for key in ["terminal", "failed", "can_terminate"] {
            if let Some(value) = runtime.get(key).and_then(Value::as_bool) {
                kept.insert(key.into(), json!(value));
            }
        }
        for key in [
            "started_time",
            "finished_time",
            "updated_time",
            "queued_time",
            "elapsed_s",
        ] {
            if let Some(value) = runtime.get(key).filter(|value| value.is_number()) {
                kept.insert(key.into(), value.clone());
            }
        }
        if let Some(metrics) = runtime.get("metrics").and_then(Value::as_object) {
            let bounded: serde_json::Map<_, _> = metrics
                .iter()
                .filter(|(_, value)| {
                    value.is_number() || value.is_boolean() || value.is_string()
                })
                .take(16)
                .map(|(key, value)| (preview(key, 64), value.clone()))
                .collect();
            if !bounded.is_empty() {
                kept.insert("metrics".into(), Value::Object(bounded));
            }
        }
        if !kept.is_empty() {
            out.insert("runtime".into(), Value::Object(kept));
        }
    }
    // Tool ledger rows carry precomputed bounded previews; the raw args and
    // result objects remain available through the canonical body locators.
    let kind = map.get("kind").and_then(Value::as_str).unwrap_or("");
    if matches!(kind, "tool_call" | "tool_result" | "tool") || map.get("tool").is_some() {
        if let Some(args) = super::thread_view_projection::tool_args_preview(body) {
            out.insert("args_preview".into(), json!(args));
        }
        if let Some(result) = super::thread_view_projection::tool_result_preview(body) {
            out.insert("result_preview".into(), json!(result));
        }
    }
    Value::Object(out)
}

fn ensure_turn_for_item(
    tx: &Transaction<'_>,
    rollout_id: &str,
    turn_id: &str,
    root_turn_id: Option<&str>,
    created_at_ms: i64,
    byte_offset: u64,
    ordinal: u64,
) -> Result<()> {
    let ordinal = sqlite_i64(ordinal)?;
    tx.execute(
        "INSERT INTO thread_turns
             (rollout_id,turn_id,root_turn_id,status,started_at_ms,start_rollout_byte_offset,start_ordinal,updated_at_ordinal)
         VALUES (?1,?2,?3,'unknown',?4,?5,?6,?6)
         ON CONFLICT(rollout_id,turn_id) DO UPDATE SET
           root_turn_id=COALESCE(thread_turns.root_turn_id,excluded.root_turn_id),
           updated_at_ordinal=MAX(thread_turns.updated_at_ordinal,excluded.updated_at_ordinal)",
        params![
            rollout_id,
            turn_id,
            root_turn_id,
            created_at_ms,
            sqlite_i64(byte_offset)?,
            ordinal,
        ],
    )?;
    Ok(())
}

pub(super) fn completed_item(body: &Value) -> Result<&Value> {
    match body.get("item") {
        Some(Value::String(raw)) => {
            // A string item is not a valid canonical display item.  Keeping
            // this as an explicit error prevents silently projecting only an
            // opaque second copy that callers cannot inspect.
            let _ = raw;
            bail!("item_completed contains a string item instead of JSON")
        }
        Some(item) if item.is_object() => Ok(item),
        Some(_) => bail!("item_completed contains a non-object item"),
        None if body.is_object() => Ok(body),
        None => bail!("item_completed payload is not an object"),
    }
}

fn canonical_kind(envelope: &RolloutEnvelope) -> Option<String> {
    let outer = normalize_kind(&envelope.record_type)?;
    if outer == "event" {
        envelope
            .event_type
            .as_deref()
            .and_then(normalize_kind)
            .or_else(|| {
                envelope
                    .payload
                    .get("event")
                    .or_else(|| envelope.payload.get("event_type"))
                    .or_else(|| envelope.payload.get("type"))
                    .and_then(Value::as_str)
                    .and_then(normalize_kind)
            })
    } else if matches!(outer.as_str(), "event_msg" | "event_message") {
        let wrapper = envelope.payload.as_object()?;
        wrapper
            .get("event_type")
            .or_else(|| wrapper.get("type"))
            .and_then(Value::as_str)
            .and_then(normalize_kind)
    } else {
        Some(outer)
    }
}

pub(super) fn event_body(envelope: &RolloutEnvelope) -> &Value {
    let outer = normalize_kind(&envelope.record_type);
    if matches!(outer.as_deref(), Some("event_msg" | "event_message"))
        || (outer.as_deref() == Some("event") && envelope.payload.get("event").is_some())
    {
        envelope.payload.get("payload").unwrap_or(&envelope.payload)
    } else {
        &envelope.payload
    }
}

fn normalize_kind(kind: &str) -> Option<String> {
    let trimmed = kind.trim();
    let mut normalized = String::with_capacity(trimmed.len() + 4);
    let mut previous_was_lower_or_digit = false;
    for character in trimmed.chars() {
        if matches!(character, '-' | '.' | ' ') {
            if !normalized.is_empty() && !normalized.ends_with('_') {
                normalized.push('_');
            }
            previous_was_lower_or_digit = false;
            continue;
        }
        if character.is_ascii_uppercase()
            && previous_was_lower_or_digit
            && !normalized.ends_with('_')
        {
            normalized.push('_');
        }
        normalized.push(character.to_ascii_lowercase());
        previous_was_lower_or_digit = character.is_ascii_lowercase() || character.is_ascii_digit();
    }
    let normalized = normalized.trim_matches('_').to_owned();
    (!normalized.is_empty()).then_some(normalized)
}

fn required_string<'a>(value: &'a Value, key: &str, context: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|raw| !raw.trim().is_empty())
        .ok_or_else(|| anyhow!("{context} is missing {key}"))
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(ToOwned::to_owned)
}

fn optional_i64(value: &Value, key: &str) -> Option<i64> {
    let value = value.get(key)?;
    if key.ends_with("_ms") {
        value.as_i64().or_else(|| value.as_str()?.parse().ok())
    } else {
        value_to_ms(value)
    }
}

fn event_time_ms(value: &Value, envelope: &RolloutEnvelope) -> Option<i64> {
    optional_i64(value, "created_at_ms")
        .or_else(|| optional_i64(value, "started_at_ms"))
        .or_else(|| optional_i64(value, "completed_at_ms"))
        .or_else(|| optional_i64(value, "aborted_at_ms"))
        .or_else(|| optional_i64(value, "timestamp_ms"))
        .or(envelope.timestamp_ms)
        .or_else(|| value_to_ms(&envelope.timestamp))
}

pub(super) fn read_bounded_line(reader: &mut impl BufRead, output: &mut Vec<u8>) -> Result<bool> {
    let start = output.len();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(false);
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        if output.len() - start + count > PROJECTION_MAX_LINE_BYTES {
            bail!("rollout JSONL line exceeds the 8 MiB projection limit");
        }
        output.extend_from_slice(&available[..count]);
        reader.consume(count);
        if newline.is_some() {
            return Ok(true);
        }
    }
}

fn read_turn(row: &rusqlite::Row<'_>, rollout_id: &str) -> Result<ThreadHistoryTurn> {
    let error: Option<String> = row.get(3)?;
    Ok(ThreadHistoryTurn {
        rollout_id: rollout_id.to_owned(),
        turn_id: row.get(0)?,
        root_turn_id: row.get(1)?,
        status: row.get(2)?,
        error_json: error.as_deref().map(serde_json::from_str).transpose()?,
        started_at_ms: row.get(4)?,
        completed_at_ms: row.get(5)?,
        duration_ms: row.get(6)?,
        first_user_item_id: row.get(7)?,
        final_agent_item_id: row.get(8)?,
        start_rollout_byte_offset: sqlite_u64(row.get(9)?)?,
        end_rollout_byte_offset: sqlite_u64(row.get(10)?)?,
        start_ordinal: sqlite_u64(row.get(11)?)?,
        end_ordinal: sqlite_u64(row.get(12)?)?,
        updated_at_ordinal: sqlite_u64_required(row.get(13)?)?,
        item_count: 0,
    })
}

fn value_to_ms(value: &Value) -> Option<i64> {
    if let Some(value) = value.as_i64() {
        return Some(if value.unsigned_abs() < 100_000_000_000 {
            value.saturating_mul(1000)
        } else {
            value
        });
    }
    let raw = value.as_str()?;
    if let Ok(value) = raw.parse::<i64>() {
        return Some(if value.unsigned_abs() < 100_000_000_000 {
            value.saturating_mul(1000)
        } else {
            value
        });
    }
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc).timestamp_millis())
}

fn is_user_item(item: &Value, item_type: &str) -> bool {
    matches!(item_type, "user" | "user_message")
        || item.get("role").and_then(Value::as_str) == Some("user")
}

fn is_agent_item(item: &Value, item_type: &str) -> bool {
    matches!(
        item_type,
        "assistant" | "agent" | "assistant_message" | "agent_message"
    ) || matches!(
        item.get("role").and_then(Value::as_str),
        Some("assistant" | "agent")
    )
}

fn trim_line_ending(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn collect_items(
    rows: rusqlite::Rows<'_>,
    rollout_id: &str,
    bodies: &mut ItemBodyReader,
) -> Result<Vec<ThreadHistoryItem>> {
    collect_item_page(rows, rollout_id, bodies).map(|(items, _)| items)
}

fn collect_item_page(
    mut rows: rusqlite::Rows<'_>,
    rollout_id: &str,
    bodies: &mut ItemBodyReader,
) -> Result<(Vec<ThreadHistoryItem>, bool)> {
    let mut items = Vec::new();
    let mut bytes = 0usize;
    while let Some(row) = rows.next()? {
        let reference = ItemBodyRef::from_row(row, 6)?;
        if bytes.saturating_add(reference.item_bytes) > 16 * 1024 * 1024 && !items.is_empty() {
            return Ok((items, true));
        }
        bytes += reference.item_bytes;
        let item_id: String = row.get(2)?;
        let ordinal = sqlite_u64_required(row.get(5)?)?;
        let item_json = bodies.read(&reference, ordinal, &item_id)?;
        items.push(ThreadHistoryItem {
            rollout_id: rollout_id.to_owned(),
            turn_id: row.get(0)?,
            root_turn_id: row.get(1)?,
            item_id,
            item_type: row.get(3)?,
            created_at_ms: row.get(4)?,
            rollout_ordinal: ordinal,
            item_json,
        });
    }
    Ok((items, false))
}

fn validate_id(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 512 || value.contains('\0') {
        bail!("invalid {name}");
    }
    Ok(())
}

fn sqlite_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| anyhow!("value exceeds SQLite INTEGER range"))
}

fn sqlite_u64(value: Option<i64>) -> Result<Option<u64>> {
    value.map(sqlite_u64_required).transpose()
}

fn sqlite_u64_required(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| anyhow!("SQLite contains a negative unsigned history value"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn store() -> (tempfile::TempDir, ThreadHistoryStore) {
        let directory = tempdir().expect("temporary directory");
        let store = ThreadHistoryStore::open(directory.path().join("thread_history_1.sqlite"))
            .expect("open history projection");
        (directory, store)
    }

    fn line(ordinal: u64, record_type: &str, payload: Value) -> String {
        serde_json::to_string(&json!({
            "timestamp": "2026-01-01T00:00:00Z",
            "ordinal": ordinal,
            "type": record_type,
            "payload": payload,
        }))
        .expect("serialize record")
            + "\n"
    }

    #[test]
    fn projects_completed_records_incrementally_and_skips_unfinished_tail() {
        let (_directory, store) = store();
        let mut rollout = line(
            0,
            "turn_started",
            json!({"turn_id":"turn-a","root_turn_id":"root-a","started_at_ms":100}),
        );
        rollout.push_str(&line(
            1,
            "item_completed",
            json!({"turn_id":"turn-a","item":{"id":"item-user","type":"user_message","created_at_ms":101,"content":"first"}}),
        ));
        let unfinished = serde_json::to_string(&json!({
            "timestamp":"2026-01-01T00:00:02Z","ordinal":2,"type":"item_completed",
            "payload":{"turn_id":"turn-a","item":{"id":"item-assistant","type":"assistant_message","created_at_ms":102,"content":"second"}}
        }))
        .expect("serialize unfinished");
        rollout.push_str(&unfinished);

        let first = store
            .project_jsonl_bytes("rollout-a", rollout.as_bytes())
            .expect("project completed prefix");
        assert_eq!(first.records_projected, 2);
        assert_eq!(first.completed_items_projected, 1);
        assert!(first.has_unfinished_tail);
        assert_eq!(first.state.next_rollout_ordinal, 2);

        rollout.push('\n');
        rollout.push_str(&line(
            3,
            "turn_completed",
            json!({"turn_id":"turn-a","completed_at_ms":160}),
        ));
        let second = store
            .project_jsonl_bytes("rollout-a", rollout.as_bytes())
            .expect("project appended records");
        assert_eq!(second.records_projected, 2);
        assert_eq!(second.completed_items_projected, 1);
        assert!(!second.has_unfinished_tail);
        assert_eq!(second.state.next_rollout_ordinal, 4);
        assert_eq!(second.state.history_revision, 2);

        let page = store
            .page_items(
                "rollout-a",
                ThreadHistoryPageRequest {
                    limit: 10,
                    before: None,
                },
            )
            .expect("read projected items");
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].item_id, "item-assistant");
        assert_eq!(page.items[1].item_id, "item-user");
        let turns = store.recent_turns("rollout-a", 10).expect("turn page");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].status, "completed");
        assert_eq!(turns[0].first_user_item_id.as_deref(), Some("item-user"));
        assert_eq!(
            turns[0].final_agent_item_id.as_deref(),
            Some("item-assistant")
        );
        assert_eq!(turns[0].duration_ms, Some(60));
    }

    #[test]
    fn item_pagination_is_keyset_and_cursor_is_revision_bound() {
        let (_directory, store) = store();
        let mut rollout = String::new();
        for ordinal in 0..5 {
            rollout.push_str(&line(
                ordinal,
                "item_completed",
                json!({
                    "turn_id":"turn-page",
                    "item":{"id":format!("item-{ordinal}"),"type":"assistant_message","created_at_ms":ordinal}
                }),
            ));
        }
        store
            .project_jsonl_bytes("rollout-page", rollout.as_bytes())
            .expect("project page fixture");

        let first = store
            .page_items(
                "rollout-page",
                ThreadHistoryPageRequest {
                    limit: 2,
                    before: None,
                },
            )
            .expect("first page");
        assert_eq!(
            first
                .items
                .iter()
                .map(|item| item.item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["item-4", "item-3"]
        );
        assert!(first.has_more);
        let second = store
            .page_items(
                "rollout-page",
                ThreadHistoryPageRequest {
                    limit: 2,
                    before: first.next_before.clone(),
                },
            )
            .expect("second page");
        assert_eq!(
            second
                .items
                .iter()
                .map(|item| item.item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["item-2", "item-1"]
        );

        rollout.push_str(&line(
            5,
            "item_completed",
            json!({"turn_id":"turn-page","item":{"id":"item-5","type":"assistant_message","created_at_ms":5}}),
        ));
        store
            .project_jsonl_bytes("rollout-page", rollout.as_bytes())
            .expect("append projection");
        let err = store
            .page_items(
                "rollout-page",
                ThreadHistoryPageRequest {
                    limit: 2,
                    before: first.next_before,
                },
            )
            .expect_err("old cursor must reset after a newer projection");
        assert!(err.to_string().contains("cursor revision"));
    }

    #[test]
    fn wrapper_event_msg_projects_the_inner_event_payload() {
        let (_directory, store) = store();
        let rollout = line(
            0,
            "event_msg",
            json!({
                "type":"ItemCompleted",
                "payload":{"turn_id":"turn-wrapper","item":{"id":"item-wrapper","type":"user_message","created_at_ms":1}}
            }),
        );
        store
            .project_jsonl_bytes("rollout-wrapper", rollout.as_bytes())
            .expect("project wrapped event");
        let page = store
            .page_items("rollout-wrapper", ThreadHistoryPageRequest::default())
            .expect("read wrapper projection");
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].item_id, "item-wrapper");
    }

    #[test]
    fn large_canonical_bodies_do_not_expand_the_index_and_keep_full_unicode_text() {
        let (directory, store) = store();
        let content = format!("{}needle-tail", "fixture 中🙂 ".repeat(160_000));
        let rollout = line(
            0,
            "item_completed",
            json!({"turn_id":"fixture-turn","item":{
            "item_id":"fixture-body","turn_id":"fixture-turn","kind":"assistant_message",
            "created_at_ms":1,"payload":{"role":"assistant","content":content}}}),
        );
        store
            .project_jsonl_bytes("fixture-rollout", rollout.as_bytes())
            .unwrap();
        let item = store.read_item("fixture-rollout", "fixture-body").unwrap();
        assert_eq!(item.item_json["payload"]["content"], content);
        let conn = Connection::open(store.db_path()).unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        let index_bytes = fs::metadata(store.db_path()).unwrap().len();
        assert!(index_bytes * 10 < rollout.len() as u64);
        assert_eq!(
            fs::read_to_string(directory.path().join("sessions/fixture-rollout.jsonl")).unwrap(),
            rollout
        );
        let path = directory.path().join("sessions/fixture-rollout.jsonl");
        fs::write(&path, rollout.replace("needle-tail", "edited-tail")).unwrap();
        assert!(store.read_item("fixture-rollout", "fixture-body").is_err());
    }

    #[test]
    fn forward_event_pages_preserve_sparse_ordinals_across_appends() {
        let (_directory, store) = store();
        let mut rollout = line(
            0,
            "turn_started",
            json!({"turn_id":"turn-forward","started_at_ms":1}),
        );
        rollout.push_str(&line(1, "response_item", json!({"content":"context only"})));
        rollout.push_str(&line(2, "item_completed", json!({"turn_id":"turn-forward","item":{"id":"item-forward","type":"assistant_message","created_at_ms":2}})));
        store
            .project_jsonl_bytes("rollout-forward", rollout.as_bytes())
            .unwrap();
        let first = store.forward_events("rollout-forward", None, 1).unwrap();
        assert_eq!(first[0].ordinal, 0);
        assert!(matches!(
            first[0].kind,
            ThreadHistoryEventKind::TurnStarted(_)
        ));
        rollout.push_str(&line(
            3,
            "turn_completed",
            json!({"turn_id":"turn-forward","completed_at_ms":4}),
        ));
        store
            .project_jsonl_bytes("rollout-forward", rollout.as_bytes())
            .unwrap();
        let rest = store
            .forward_events("rollout-forward", Some(first[0].ordinal), 10)
            .unwrap();
        assert_eq!(
            rest.iter().map(|event| event.ordinal).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(matches!(
            rest[0].kind,
            ThreadHistoryEventKind::ItemCompleted(_)
        ));
        assert!(matches!(
            rest[1].kind,
            ThreadHistoryEventKind::TurnFinished(_)
        ));
        assert!(store
            .forward_events("rollout-forward", Some(3), 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn canonical_nested_events_keep_exact_millisecond_fields() {
        use wunder_core::thread_store::{
            CanonicalEvent, CanonicalItem, CanonicalItemKind, CanonicalRecord, ItemId,
            RolloutRecord, TurnId,
        };
        let (directory, store) = store();
        let turn_id = TurnId::new("turn-canonical").expect("turn id");
        let events = vec![
            CanonicalEvent::TurnStarted {
                turn_id: turn_id.clone(),
                root_turn_id: None,
                started_at_ms: 100,
            },
            CanonicalEvent::ItemCompleted(CanonicalItem {
                item_id: ItemId::new("item-canonical").expect("item id"),
                turn_id: turn_id.clone(),
                root_turn_id: None,
                created_at_ms: 110,
                kind: CanonicalItemKind::UserMessage,
                payload: json!({"text":"sample"}),
                display_output: None,
                display_output_truncated: false,
            }),
            CanonicalEvent::TurnAborted {
                turn_id,
                aborted_at_ms: 160,
                error: None,
            },
        ];
        let mut bytes = Vec::new();
        for (ordinal, event) in events.into_iter().enumerate() {
            serde_json::to_writer(
                &mut bytes,
                &RolloutRecord {
                    timestamp_ms: 0,
                    ordinal: ordinal as u64,
                    record: CanonicalRecord::Event(event),
                },
            )
            .expect("encode canonical record");
            bytes.push(b'\n');
        }
        let path = directory.path().join("rollout.jsonl");
        fs::write(&path, &bytes).expect("write canonical rollout");
        store
            .persist_test_source("rollout-canonical", &bytes)
            .unwrap();
        let progress = store
            .project_file_batch(&path, "rollout-canonical", bytes.len() as u64)
            .expect("project canonical file");
        assert_eq!(progress.completed_items_projected, 1);
        let turns = store
            .page_turns(
                "rollout-canonical",
                ThreadHistoryPageRequest {
                    limit: 10,
                    before: None,
                },
            )
            .expect("turn page");
        assert_eq!(turns.turns[0].started_at_ms, 100);
        assert_eq!(turns.turns[0].completed_at_ms, Some(160));
        assert_eq!(turns.turns[0].duration_ms, Some(60));
        assert_eq!(turns.turns[0].item_count, 1);
        assert_eq!(
            store
                .read_canonical_item("rollout-canonical", "item-canonical")
                .expect("read canonical item")
                .created_at_ms,
            110
        );
    }

    #[test]
    fn file_projection_batches_resume_from_saved_byte_offset() {
        let (directory, store) = store();
        let mut bytes = String::new();
        for ordinal in 0..520 {
            bytes.push_str(&line(ordinal, "item_completed", json!({"turn_id":"turn-batch","item":{"id":format!("item-{ordinal}"),"type":"user_message","created_at_ms":ordinal}})));
        }
        let path = directory.path().join("rollout.jsonl");
        fs::write(&path, bytes.as_bytes()).expect("write rollout file");
        let first = store
            .project_file_batch(&path, "rollout-batch", bytes.len() as u64)
            .expect("first batch");
        assert_eq!(first.records_projected, 512);
        assert!(first.state.next_rollout_byte_offset < bytes.len() as u64);
        let second = store
            .project_file_batch(&path, "rollout-batch", bytes.len() as u64)
            .expect("second batch");
        assert_eq!(second.records_projected, 8);
        assert_eq!(second.state.next_rollout_byte_offset, bytes.len() as u64);
    }

    #[test]
    fn byte_limited_history_pages_keep_an_exclusive_cursor() {
        let (_directory, store) = store();
        let content = "x".repeat(6 * 1024 * 1024);
        let mut source = String::new();
        for ordinal in 0..3 {
            let bytes = line(
                ordinal,
                "item_completed",
                json!({"turn_id":"turn-large","item":{"id":format!("item-{ordinal}"),"type":"assistant_message","content":content}}),
            );
            // Each projection call starts at the previously consumed prefix.
            source.push_str(&bytes);
            store
                .persist_test_source("rollout-large", source.as_bytes())
                .unwrap();
            let state = store.projection_state("rollout-large").unwrap();
            store
                .project_chunk("rollout-large", state, bytes.as_bytes())
                .unwrap();
        }
        let first = store
            .page_items(
                "rollout-large",
                ThreadHistoryPageRequest {
                    limit: 10,
                    before: None,
                },
            )
            .unwrap();
        assert_eq!(first.items.len(), 2);
        assert!(first.has_more);
        let second = store
            .page_items(
                "rollout-large",
                ThreadHistoryPageRequest {
                    limit: 10,
                    before: first.next_before,
                },
            )
            .unwrap();
        assert_eq!(second.items.len(), 1);
        assert!(!second.has_more);
        assert_eq!(second.items[0].item_id, "item-0");
    }

    fn settled_index_bytes(store: &ThreadHistoryStore) -> u64 {
        let conn = Connection::open(store.db_path()).expect("open projection for checkpoint");
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .expect("truncate write-ahead log");
        drop(conn);
        fs::metadata(store.db_path()).unwrap().len()
    }

    /// Regression: the index used to keep one row per completed item, so its
    /// size tracked the canonical log one-to-one. Only the newest
    /// `ACTIVE_WINDOW_ITEMS` rows may stay materialized, the file must stop
    /// growing once the window is full, and older items must still be readable
    /// through the sparse anchors.
    #[test]
    fn active_window_bounds_the_index_and_keeps_older_items_reachable() {
        let (_directory, store) = store();
        let total: u64 = 6_000;
        let batch: u64 = 400;
        let mut source = String::new();
        let mut settled_at_full_window = 0u64;
        let mut start = 0u64;
        while start < total {
            let end = (start + batch).min(total);
            let mut chunk = String::new();
            for ordinal in start..end {
                chunk.push_str(&line(
                    ordinal,
                    "item_completed",
                    json!({"turn_id":"turn-window","item":{
                        "id":format!("item-{ordinal}"),"type":"assistant_message",
                        "created_at_ms":ordinal,"content":"x".repeat(64)}}),
                ));
            }
            source.push_str(&chunk);
            store
                .persist_test_source("rollout-window", source.as_bytes())
                .unwrap();
            let state = store.projection_state("rollout-window").unwrap();
            store
                .project_chunk("rollout-window", state, chunk.as_bytes())
                .unwrap();
            if settled_at_full_window == 0 && end >= ACTIVE_WINDOW_ITEMS as u64 + 800 {
                settled_at_full_window = settled_index_bytes(&store);
            }
            start = end;
        }

        let conn = Connection::open(store.db_path()).unwrap();
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM thread_items WHERE rollout_id='rollout-window'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rows, ACTIVE_WINDOW_ITEMS);
        let anchors: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM thread_item_anchors WHERE rollout_id='rollout-window'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(anchors, (total / ANCHOR_INTERVAL as u64 + 1) as i64);
        drop(conn);

        let settled = settled_index_bytes(&store);
        assert!(
            settled <= settled_at_full_window * 3 / 2,
            "index grew with the log after the window filled: {settled_at_full_window} -> {settled}"
        );
        assert!(
            settled < 8 * 1024 * 1024,
            "bounded window index is unexpectedly large: {settled} bytes"
        );

        // The oldest items left the window; they are still served from the
        // canonical rollout through the nearest sparse anchor.
        let old = store
            .read_item("rollout-window", "item-100")
            .expect("item outside the active window is still readable");
        assert_eq!(old.item_json["id"], "item-100");
    }
}

