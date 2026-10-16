use super::agent_runtime_store::PostgresAgentRuntimeStorage;
use super::log_maintenance::{self, PurgeBudget, UsageRows};
use super::log_stats_store::read_meta_f64;
use super::{PgTx, PostgresStorage};
use crate::storage::{OrphanSweepResult, StorageLifecycle};
use anyhow::Result;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub(super) trait PostgresRetentionStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>>;
    fn cleanup_orphan_session_logs_impl(&self, cutoff_epoch_s: f64) -> Result<OrphanSweepResult>;
}

/// Width of one incremental orphan-sweep slice (seconds). Slices are swept in
/// order and the reached position is persisted, so a large backfilled history
/// converges over periodic runs instead of one unbounded transaction.
const ORPHAN_SLICE_SECONDS: f64 = 3600.0;
/// Wall-clock budget per sweep invocation; the next periodic run continues
/// from the persisted watermark.
const ORPHAN_SWEEP_BUDGET: Duration = Duration::from_secs(15);
/// meta key of the persisted sweep watermark (epoch seconds, exclusive upper
/// bound of the fully swept range).
// Restart discovery once: the old scanner advanced after sampling only some
// sessions, or even after a rollback, and could leave older rows behind.
const ORPHAN_SWEEP_WM_KEY: &str = "orphan_sweep_time_wm_v2";

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

    fn cleanup_orphan_session_logs_impl(&self, cutoff_epoch_s: f64) -> Result<OrphanSweepResult> {
        self.ensure_initialized()?;
        let deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 {
            return Ok(OrphanSweepResult {
                deleted,
                completed: true,
                swept_until: cutoff_epoch_s,
            });
        }
        // Every source and projection uses the same committed row batches as
        // session purge. Advance the time watermark only after all tables in
        // the interval are exhausted; a busy or oversized interval resumes.
        let deadline = Instant::now() + ORPHAN_SWEEP_BUDGET;
        let mut deleted = HashMap::new();
        let mut swept_until = 0.0;
        loop {
            // Any failure leaves the interval watermark in place for retry.
            let slice =
                self.sweep_orphan_slice(cutoff_epoch_s, &mut deleted, &mut swept_until, deadline)?;
            if slice.reached_cutoff {
                return Ok(OrphanSweepResult {
                    deleted,
                    completed: true,
                    swept_until: cutoff_epoch_s,
                });
            }
            // Spending the budget mid-interval is the normal way a large
            // backlog converges: rows are already gone and the watermark
            // points at the resume position. Report it as progress instead of
            // failing, otherwise every pass logs an error for doing its job.
            if !slice.completed || Instant::now() >= deadline {
                return Ok(OrphanSweepResult {
                    deleted,
                    completed: false,
                    swept_until,
                });
            }
        }
    }
}

struct SliceOutcome {
    /// Every table in the interval was drained, so the watermark advanced.
    completed: bool,
    /// The swept range now covers the cutoff.
    reached_cutoff: bool,
}

impl PostgresStorage {
    /// Sweep one time interval with independently committed short batches.
    fn sweep_orphan_slice(
        &self,
        cutoff_epoch_s: f64,
        deleted: &mut HashMap<String, i64>,
        swept_until: &mut f64,
        deadline: Instant,
    ) -> Result<SliceOutcome> {
        let mut conn = self.conn()?;
        let mut tx = log_maintenance::begin(&mut conn)?;
        let watermark = match read_meta_f64(&mut tx, ORPHAN_SWEEP_WM_KEY)? {
            Some(value) => value,
            None => earliest_orphan_source_time(&mut tx, cutoff_epoch_s)?,
        };
        tx.commit()?;
        *swept_until = watermark;
        if watermark >= cutoff_epoch_s {
            return Ok(SliceOutcome {
                completed: true,
                reached_cutoff: true,
            });
        }
        let slice_hi = (watermark + ORPHAN_SLICE_SECONDS).min(cutoff_epoch_s);
        let predicate =
            format!("t.created_time >= $1 AND t.created_time < $2 AND {ORPHAN_PREDICATE}");
        let params: &[&(dyn tokio_postgres::types::ToSql + Sync)] = &[&watermark, &slice_hi];
        // One budget for the whole interval: whichever table still has rows
        // when the clock runs out resumes from the same watermark.
        let mut budget = PurgeBudget::until(deadline);
        let mut drained = true;
        for (source, table) in [
            (UsageRows::Chat, "thread_items"),
            (UsageRows::Tool, "tool_logs"),
        ] {
            let outcome = log_maintenance::delete_usage_rows(
                &mut conn,
                source,
                &predicate,
                params,
                &mut budget,
            )?;
            drained &= outcome.drained;
            if outcome.removed > 0 {
                *deleted.entry(table.to_string()).or_insert(0) += outcome.removed;
            }
        }
        for table in [
            "thread_item_blocks",
            "artifact_logs",
            "thread_turns",
            "thread_log_changes",
        ] {
            let outcome =
                log_maintenance::delete_rows(&mut conn, table, &predicate, params, &mut budget)?;
            drained &= outcome.drained;
            if outcome.removed > 0 {
                *deleted.entry(table.to_string()).or_insert(0) += outcome.removed;
            }
        }
        if !drained {
            // Rows remain in this interval. Leave the watermark untouched so
            // the next pass continues here instead of skipping them.
            return Ok(SliceOutcome {
                completed: false,
                reached_cutoff: false,
            });
        }
        // Concurrent sweepers can finish out of order, so never regress.
        let mut tx = log_maintenance::begin(&mut conn)?;
        tx.execute(
            "INSERT INTO meta(key,value,updated_time) VALUES($1,$2,$3)
             ON CONFLICT(key) DO UPDATE SET
             value=GREATEST(meta.value::double precision,EXCLUDED.value::double precision)::text,
             updated_time=EXCLUDED.updated_time",
            &[&ORPHAN_SWEEP_WM_KEY, &slice_hi.to_string(), &Self::now_ts()],
        )?;
        tx.commit()?;
        Ok(SliceOutcome {
            completed: true,
            reached_cutoff: slice_hi >= cutoff_epoch_s,
        })
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
fn earliest_orphan_source_time(tx: &mut PgTx<'_>, fallback: f64) -> Result<f64> {
    let mut earliest = fallback;
    for table in [
        "thread_items",
        "tool_logs",
        "artifact_logs",
        "thread_item_blocks",
        "thread_log_changes",
        "thread_turns",
    ] {
        let value: Option<f64> = tx
            .query_one(&format!("SELECT min(created_time) FROM {table}"), &[])?
            .get(0);
        if let Some(value) = value {
            earliest = earliest.min(value);
        }
    }
    Ok(earliest)
}
