use super::agent_runtime_store::PostgresAgentRuntimeStorage;
use super::log_stats_store::{decrement_usage_summary, read_meta_f64, write_meta_f64};
use super::{PgTx, PostgresStorage};
use crate::storage::{StorageLifecycle, USAGE_CHAT_WATERMARK_KEY, USAGE_TOOL_WATERMARK_KEY};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tracing::warn;

pub(super) trait PostgresRetentionStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>>;
    fn cleanup_orphan_session_logs_impl(&self, cutoff_epoch_s: f64)
        -> Result<HashMap<String, i64>>;
}

/// Width of one incremental orphan-sweep slice (seconds). Slices are swept in
/// order and the reached position is persisted, so a large backfilled history
/// converges over periodic runs instead of one unbounded transaction.
const ORPHAN_SLICE_SECONDS: f64 = 3600.0;
/// Orphan (session_id, user_id) pairs deleted per short transaction.
const ORPHAN_SLICE_SESSIONS: usize = 128;
/// Wall-clock budget per sweep invocation; the next periodic run continues
/// from the persisted watermark.
const ORPHAN_SWEEP_BUDGET: Duration = Duration::from_secs(15);
/// meta key of the persisted sweep watermark (epoch seconds, exclusive upper
/// bound of the fully swept range).
const ORPHAN_SWEEP_WM_KEY: &str = "orphan_sweep_time_wm";

/// Rows whose session vanished from the durable catalog. Evaluated inside a
/// bounded `created_time` window so the anti-join scan always has an early
/// exit through the time indexes.
const ORPHAN_PREDICATE: &str = "NOT EXISTS (SELECT 1 FROM chat_sessions c \
     WHERE c.session_id = t.session_id) \
   AND NOT EXISTS (SELECT 1 FROM deleted_session_log_grace g \
     WHERE g.session_id = t.session_id)";

impl PostgresRetentionStorage for PostgresStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 {
            return Ok(deleted);
        }
        // Only stream_events are ephemeral replay buffers. Conversation,
        // context, runtime and audit records stay durable and are removed only
        // through the administrator cleanup endpoint. PostgreSQL reclaims the
        // freed space itself via autovacuum.
        let removed = self.delete_stream_events_before_impl(cutoff_epoch_s)?;
        if removed > 0 {
            deleted.insert("stream_events".to_string(), removed);
        }
        Ok(deleted)
    }

    fn cleanup_orphan_session_logs_impl(
        &self,
        cutoff_epoch_s: f64,
    ) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 {
            return Ok(deleted);
        }
        // Orphan sweep: log rows whose session vanished from the durable
        // catalog (interrupted deletions, older purge paths). The sweep is
        // incremental: a persisted watermark splits time into one-hour slices,
        // each slice handled in one short transaction that collects a bounded
        // session sample through the time indexes and deletes by session. A
        // single anti-join DELETE across every partition is a minutes-long
        // transaction on stress-sized tables and starves the write path
        // (thread_log committers time out, live tool/assistant items stop
        // persisting, the frontend timeline goes blind until reload) — the
        // exact failure this rewrite removes.
        let deadline = Instant::now() + ORPHAN_SWEEP_BUDGET;
        loop {
            match self.sweep_orphan_slice(cutoff_epoch_s, &mut deleted) {
                Ok(outcome) => {
                    if outcome.reached_cutoff {
                        break;
                    }
                }
                Err(error) => {
                    // A failed or timed-out slice is skipped by advancing the
                    // watermark in a fresh transaction, so one hot slice can
                    // never wedge the sweep permanently.
                    warn!(error = %error, "orphan sweep slice skipped");
                    self.force_advance_orphan_watermark(cutoff_epoch_s)?;
                }
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        Ok(deleted)
    }
}

struct SliceOutcome {
    reached_cutoff: bool,
}

impl PostgresStorage {
    /// Sweep one time slice in a single short transaction.
    fn sweep_orphan_slice(
        &self,
        cutoff_epoch_s: f64,
        deleted: &mut HashMap<String, i64>,
    ) -> Result<SliceOutcome> {
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("SET LOCAL statement_timeout = '8s'", &[])?;
        tx.execute("SET LOCAL lock_timeout = '2s'", &[])?;
        let watermark = read_meta_f64(&mut tx, ORPHAN_SWEEP_WM_KEY)?
            .unwrap_or_else(|| earliest_orphan_source_time(&mut tx, cutoff_epoch_s));
        if watermark >= cutoff_epoch_s {
            return Ok(SliceOutcome {
                reached_cutoff: true,
            });
        }
        let slice_hi = (watermark + ORPHAN_SLICE_SECONDS).min(cutoff_epoch_s);
        let pairs = collect_orphan_session_pairs(&mut tx, watermark, slice_hi)?;
        if !pairs.is_empty() {
            let chat_wm = read_meta_f64(&mut tx, USAGE_CHAT_WATERMARK_KEY)?.unwrap_or(0.0);
            let tool_wm = read_meta_f64(&mut tx, USAGE_TOOL_WATERMARK_KEY)?.unwrap_or(0.0) as i64;
            let mut chat_deltas: Vec<(String, i64)> = Vec::new();
            let mut tool_deltas: Vec<(String, i64)> = Vec::new();
            for (session_id, user_id) in &pairs {
                delete_orphan_session_slice(
                    &mut tx,
                    session_id,
                    user_id,
                    watermark,
                    slice_hi,
                    chat_wm,
                    tool_wm,
                    &mut chat_deltas,
                    &mut tool_deltas,
                    deleted,
                )?;
            }
            // Rows inside the usage watermarks were counted in the per-user
            // summary; decrement it exactly like the previous sweep did.
            if !chat_deltas.is_empty() || !tool_deltas.is_empty() {
                decrement_usage_summary(&mut tx, &chat_deltas, &tool_deltas, Self::now_ts())?;
            }
        }
        write_meta_f64(&mut tx, ORPHAN_SWEEP_WM_KEY, slice_hi, Self::now_ts())?;
        tx.commit()?;
        Ok(SliceOutcome {
            reached_cutoff: slice_hi >= cutoff_epoch_s,
        })
    }

    /// Advance the sweep watermark past the current slice after a failure so
    /// the next slice gets its chance. Best effort: a failure here just means
    /// the next periodic run retries the same slice.
    fn force_advance_orphan_watermark(&self, cutoff_epoch_s: f64) -> Result<()> {
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("SET LOCAL statement_timeout = '2s'", &[])?;
        tx.execute("SET LOCAL lock_timeout = '2s'", &[])?;
        let watermark = read_meta_f64(&mut tx, ORPHAN_SWEEP_WM_KEY)?.unwrap_or(0.0);
        if watermark >= cutoff_epoch_s {
            return Ok(());
        }
        let slice_hi = (watermark + ORPHAN_SLICE_SECONDS).min(cutoff_epoch_s);
        write_meta_f64(&mut tx, ORPHAN_SWEEP_WM_KEY, slice_hi, Self::now_ts())?;
        tx.commit()?;
        Ok(())
    }

    /// Dedicated stress-writer entry point: the bulk writer runs against the
    /// already-initialized database of a live server, so the schema bootstrap
    /// (a giant implicit DDL transaction) must never run from this instance —
    /// at runtime it joins whatever lock traffic exists and amplifies it.
    pub fn assume_schema_initialized(&self) {
        self.initialized
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Initial sweep position: the oldest row across the source tables. Starting
/// at the epoch would spin through decades of empty slices; every MIN here is
/// an index-edge lookup (the turns table is small enough to scan directly).
fn earliest_orphan_source_time(tx: &mut PgTx<'_>, fallback: f64) -> f64 {
    let mut earliest = fallback;
    for table in [
        "thread_items",
        "tool_logs",
        "artifact_logs",
        "thread_log_changes",
        "thread_turns",
    ] {
        if let Ok(rows) = tx.query(&format!("SELECT min(created_time) FROM {table}"), &[]) {
            if let Some(value) = rows[0].try_get::<_, Option<f64>>(0).ok().flatten() {
                if value < earliest {
                    earliest = value;
                }
            }
        }
    }
    earliest
}

/// Collect a bounded sample of orphan sessions inside the slice. Each source
/// table has an early exit: `thread_items`/`tool_logs`/`artifact_logs` are
/// covered by created_time btree indexes, the turn/change tables stay small.
fn collect_orphan_session_pairs(
    tx: &mut PgTx<'_>,
    slice_lo: f64,
    slice_hi: f64,
) -> Result<Vec<(String, String)>> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for table in [
        "thread_items",
        "tool_logs",
        "artifact_logs",
        "thread_turns",
        "thread_log_changes",
    ] {
        let remaining = ORPHAN_SLICE_SESSIONS - pairs.len();
        if remaining == 0 {
            break;
        }
        let sql = format!(
            "SELECT t.session_id, t.user_id FROM {table} t \
             WHERE t.created_time >= $1 AND t.created_time < $2 AND {ORPHAN_PREDICATE} \
             LIMIT {remaining}"
        );
        for row in tx.query(&sql, &[&slice_lo, &slice_hi])? {
            let session_id: String = row.get(0);
            let user_id: String = row.get(1);
            // Dedupe on the (session, user) pair: the deletes below are
            // user-scoped, so a second user on the same vanished session must
            // not be collapsed away.
            if seen.insert(format!("{session_id}\u{0}{user_id}")) {
                pairs.push((session_id, user_id));
            }
        }
    }
    Ok(pairs)
}

/// Delete one orphan session's rows inside the slice, table by table, every
/// statement an index-backed point delete. Any statement error aborts the
/// whole slice transaction on purpose (the caller advances the watermark).
#[allow(clippy::too_many_arguments)]
fn delete_orphan_session_slice(
    tx: &mut PgTx<'_>,
    session_id: &str,
    user_id: &str,
    slice_lo: f64,
    slice_hi: f64,
    chat_wm: f64,
    tool_wm: i64,
    chat_deltas: &mut Vec<(String, i64)>,
    tool_deltas: &mut Vec<(String, i64)>,
    deleted: &mut HashMap<String, i64>,
) -> Result<()> {
    let mut add = |table: &str, removed: i64| {
        if removed > 0 {
            *deleted.entry(table.to_string()).or_insert(0) += removed;
        }
    };
    // thread_items: rows inside the chat watermark were counted in the usage
    // summary and must decrement it; the rest was never admitted.
    for row in tx.query(
        "DELETE FROM thread_items t WHERE t.session_id = $1 AND t.user_id = $2 \
           AND t.created_time >= $3 AND t.created_time < $4 \
           AND t.visibility = 'user' AND t.kind IN ('user_message','assistant_message') \
           AND t.created_time <= $5 RETURNING user_id",
        &[&session_id, &user_id, &slice_lo, &slice_hi, &chat_wm],
    )? {
        push_delta(chat_deltas, row.get(0));
    }
    let removed = tx.execute(
        "DELETE FROM thread_items t WHERE t.session_id = $1 AND t.user_id = $2 \
           AND t.created_time >= $3 AND t.created_time < $4",
        &[&session_id, &user_id, &slice_lo, &slice_hi],
    )?;
    add("thread_items", removed as i64);
    let removed = tx.execute(
        "DELETE FROM thread_item_blocks t WHERE t.session_id = $1 \
           AND t.created_time >= $2 AND t.created_time < $3",
        &[&session_id, &slice_lo, &slice_hi],
    )?;
    add("thread_item_blocks", removed as i64);
    // tool_logs: same split on the tool watermark with the excluded flag.
    for row in tx.query(
        "DELETE FROM tool_logs t WHERE t.user_id = $2 AND t.session_id = $1 \
           AND t.created_time >= $3 AND t.created_time < $4 \
           AND t.excluded = FALSE AND t.id <= $5 RETURNING user_id",
        &[&session_id, &user_id, &slice_lo, &slice_hi, &tool_wm],
    )? {
        push_delta(tool_deltas, row.get(0));
    }
    let removed = tx.execute(
        "DELETE FROM tool_logs t WHERE t.user_id = $2 AND t.session_id = $1 \
           AND t.created_time >= $3 AND t.created_time < $4",
        &[&session_id, &user_id, &slice_lo, &slice_hi],
    )?;
    add("tool_logs", removed as i64);
    let removed = tx.execute(
        "DELETE FROM artifact_logs t WHERE t.user_id = $2 AND t.session_id = $1 \
           AND t.created_time >= $3 AND t.created_time < $4",
        &[&session_id, &user_id, &slice_lo, &slice_hi],
    )?;
    add("artifact_logs", removed as i64);
    let removed = tx.execute(
        "DELETE FROM thread_turns t WHERE t.session_id = $1 \
           AND t.created_time >= $2 AND t.created_time < $3",
        &[&session_id, &slice_lo, &slice_hi],
    )?;
    add("thread_turns", removed as i64);
    let removed = tx.execute(
        "DELETE FROM thread_log_changes t WHERE t.session_id = $1 \
           AND t.created_time >= $2 AND t.created_time < $3",
        &[&session_id, &slice_lo, &slice_hi],
    )?;
    add("thread_log_changes", removed as i64);
    Ok(())
}

fn push_delta(deltas: &mut Vec<(String, i64)>, user_id: String) {
    if let Some(entry) = deltas.iter_mut().find(|(user, _)| user == &user_id) {
        entry.1 += 1;
    } else {
        deltas.push((user_id, 1));
    }
}
