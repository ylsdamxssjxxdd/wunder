//! Bounded log deletion and coordination with usage-watermark reconciliation.
use super::{PgConn, PgTx};
use crate::storage::{USAGE_CHAT_WATERMARK_KEY, USAGE_TOOL_WATERMARK_KEY};
use anyhow::{ensure, Result};
use std::time::{Duration, Instant};
use tokio_postgres::types::ToSql;

pub(super) const ROW_BATCH: i64 = 1024;

/// Ceiling on committed batches, independent of the wall clock.  Time is the
/// real bound; this only stops a loop that never observes it.
const MAX_BATCHES: usize = 2048;
/// Consecutive batches that select rows but remove none before the purge
/// gives up and reports an unfinished slice.  A row that survives the DELETE
/// (concurrent writer, trigger) never resolves by retrying in a tight loop.
const MAX_STALLED_BATCHES: usize = 3;

/// Limits work across all tables in one invocation, not just one DELETE.
pub(super) struct PurgeBudget {
    remaining: usize,
    deadline: Instant,
}

impl PurgeBudget {
    /// Budget for one-shot purges that must finish inside a single caller
    /// request.
    pub(super) fn new() -> Self {
        Self::until(Instant::now() + Duration::from_secs(5))
    }

    /// Budget shared with a resumable sweep: the caller's own deadline decides
    /// how much one pass performs.  A fixed batch count cannot express that —
    /// an interval holding more rows than the cap turns every pass into a
    /// failure that reports no progress, while a fast database leaves most of
    /// the wall-clock budget unused.
    pub(super) fn until(deadline: Instant) -> Self {
        Self {
            remaining: MAX_BATCHES,
            deadline,
        }
    }

    pub(super) fn exhausted(&self) -> bool {
        self.remaining == 0 || Instant::now() >= self.deadline
    }

    pub(super) fn committed(&mut self, rows: i64) {
        if rows > 0 {
            self.remaining = self.remaining.saturating_sub(1);
        }
    }
}

/// One bounded purge.  `drained` is false only when the budget ran out with
/// rows still matching, which is a normal step of a resumable sweep and not a
/// failure of the database.
#[derive(Clone, Copy, Default)]
pub(super) struct PurgeOutcome {
    pub removed: i64,
    pub drained: bool,
}

/// Callers that report success as "everything is gone" — session purge clears
/// the durable deletion marker afterwards — must fail on a partial purge,
/// otherwise the remaining rows never get a second pass.
pub(super) fn require_drained(outcome: PurgeOutcome) -> Result<i64> {
    ensure!(
        outcome.drained,
        "log purge slice incomplete; retry from the durable deletion marker"
    );
    Ok(outcome.removed)
}

pub(super) fn begin<'a>(conn: &'a mut PgConn<'_>) -> Result<PgTx<'a>> {
    let mut tx = conn.transaction()?;
    // PostgreSQL may otherwise hash every matching session row on each batch.
    // Keep the DELETE on nested tuple lookups and the anti-joins on their
    // catalog indexes; these settings end with this transaction.
    tx.batch_execute(
        "SET LOCAL lock_timeout = '2s'; SET LOCAL statement_timeout = '8s';
         SET LOCAL enable_hashjoin = off; SET LOCAL enable_mergejoin = off",
    )?;
    Ok(tx)
}

/// All usage-affecting deletes acquire the shared lock BEFORE reading their
/// watermarks or touching rows. Reconciliation takes the exclusive lock.
/// Try-locks avoid accumulating background jobs behind another replica.
pub(super) fn lock_usage(tx: &mut PgTx<'_>, reconcile: bool) -> Result<()> {
    let function = if reconcile {
        "pg_try_advisory_xact_lock"
    } else {
        "pg_try_advisory_xact_lock_shared"
    };
    let acquired: bool = tx
        .query_one(
            &format!("SELECT {function}(1937202, hashtext(current_schema()))"),
            &[],
        )?
        .get(0);
    ensure!(acquired, "usage maintenance busy; retry on the next pass");
    Ok(())
}

/// Table and predicate come exclusively from static SQL at the call sites.
/// tableoid is required: two monthly partitions may contain the same ctid.
pub(super) fn delete_rows(
    conn: &mut PgConn<'_>,
    table: &str,
    predicate: &str,
    params: &[&(dyn ToSql + Sync)],
    budget: &mut PurgeBudget,
) -> Result<PurgeOutcome> {
    let sql = format!(
        "WITH doomed AS MATERIALIZED (
            SELECT tableoid, ctid FROM {table} t WHERE {predicate} LIMIT {ROW_BATCH}
         ), del AS (
            DELETE FROM {table} t USING doomed d
            WHERE t.tableoid=d.tableoid AND t.ctid=d.ctid AND ({predicate}) RETURNING 1
         ) SELECT (SELECT COUNT(*) FROM del), (SELECT COUNT(*) FROM doomed)"
    );
    let mut total = 0;
    let mut stalled = 0usize;
    loop {
        // Always run one batch, even on an already-spent budget: a resumable
        // sweep that deletes nothing reports no progress and never converges.
        let mut tx = begin(conn)?;
        let row = tx.query_one(&sql, params)?;
        let removed: i64 = row.get(0);
        let selected: i64 = row.get(1);
        tx.commit()?;
        total += removed;
        budget.committed(selected);
        stalled = if removed == 0 && selected > 0 {
            stalled + 1
        } else {
            0
        };
        // A concurrent purge/update can remove the selected tuple before our
        // DELETE reaches it. An empty DELETE alone does not prove completion.
        if selected < ROW_BATCH && removed == selected {
            return Ok(PurgeOutcome {
                removed: total,
                drained: true,
            });
        }
        if budget.exhausted() || stalled >= MAX_STALLED_BATCHES {
            return Ok(PurgeOutcome {
                removed: total,
                drained: false,
            });
        }
    }
}

/// Single-call purge for callers that cannot resume: a partial purge becomes
/// an error so the durable marker survives.
pub(super) fn delete_rows_all(
    conn: &mut PgConn<'_>,
    table: &str,
    predicate: &str,
    params: &[&(dyn ToSql + Sync)],
    budget: &mut PurgeBudget,
) -> Result<i64> {
    require_drained(delete_rows(conn, table, predicate, params, budget)?)
}

#[derive(Clone, Copy)]
pub(super) enum UsageRows {
    Chat,
    Tool,
}

/// Delete source rows and adjust only counters admitted by the frozen usage
/// watermark. Returning grouped deltas keeps memory bounded by ROW_BATCH.
pub(super) fn delete_usage_rows(
    conn: &mut PgConn<'_>,
    source: UsageRows,
    predicate: &str,
    params: &[&(dyn ToSql + Sync)],
    budget: &mut PurgeBudget,
) -> Result<PurgeOutcome> {
    use super::log_stats_store::{decrement_usage_summary, read_meta_f64};
    let watermark_param = params.len() + 1;
    let (table, key, columns, counted) = match source {
        UsageRows::Chat => (
            "thread_items",
            USAGE_CHAT_WATERMARK_KEY,
            "t.user_id,t.visibility,t.kind,t.created_time",
            format!("visibility='user' AND kind IN ('user_message','assistant_message') AND created_time<=${watermark_param}"),
        ),
        UsageRows::Tool => (
            "tool_logs",
            USAGE_TOOL_WATERMARK_KEY,
            "t.user_id,t.excluded,t.id",
            format!("excluded=FALSE AND id<=${watermark_param}"),
        ),
    };
    let sql = format!(
        "WITH doomed AS MATERIALIZED (
            SELECT tableoid,ctid FROM {table} t WHERE {predicate} LIMIT {ROW_BATCH}
         ), del AS (
            DELETE FROM {table} t USING doomed d
            WHERE t.tableoid=d.tableoid AND t.ctid=d.ctid AND ({predicate}) RETURNING {columns}
         ) SELECT user_id,COUNT(*) AS removed,COUNT(*) FILTER (WHERE {counted}) AS counted
           FROM del GROUP BY user_id
           UNION ALL SELECT NULL::TEXT,0,(SELECT COUNT(*) FROM doomed)
           ORDER BY user_id NULLS LAST"
    );
    let mut total = 0;
    let mut stalled = 0usize;
    loop {
        let mut tx = begin(conn)?;
        let watermark = read_meta_f64(&mut tx, key)?.unwrap_or(0.0);
        let tool_watermark = watermark as i64;
        let mut args = params.to_vec();
        args.push(match source {
            UsageRows::Chat => &watermark,
            UsageRows::Tool => &tool_watermark,
        });
        let mut removed = 0;
        let mut selected = 0;
        let mut deltas = Vec::new();
        for row in tx.query(&sql, &args)? {
            if let Some(user_id) = row.get::<_, Option<String>>(0) {
                removed += row.get::<_, i64>(1);
                deltas.push((user_id, row.get::<_, i64>(2)));
            } else {
                selected = row.get::<_, i64>(2);
            }
        }
        let now = super::PostgresStorage::now_ts();
        match source {
            UsageRows::Chat => decrement_usage_summary(&mut tx, &deltas, &[], now)?,
            UsageRows::Tool => decrement_usage_summary(&mut tx, &[], &deltas, now)?,
        }
        tx.commit()?;
        total += removed;
        budget.committed(selected);
        stalled = if removed == 0 && selected > 0 {
            stalled + 1
        } else {
            0
        };
        if selected < ROW_BATCH && removed == selected {
            return Ok(PurgeOutcome {
                removed: total,
                drained: true,
            });
        }
        if budget.exhausted() || stalled >= MAX_STALLED_BATCHES {
            return Ok(PurgeOutcome {
                removed: total,
                drained: false,
            });
        }
    }
}

/// Single-call purge for callers that cannot resume: a partial purge becomes
/// an error so the durable marker survives.
pub(super) fn delete_usage_rows_all(
    conn: &mut PgConn<'_>,
    source: UsageRows,
    predicate: &str,
    params: &[&(dyn ToSql + Sync)],
    budget: &mut PurgeBudget,
) -> Result<i64> {
    require_drained(delete_usage_rows(conn, source, predicate, params, budget)?)
}

#[cfg(test)]
#[path = "log_maintenance_tests.rs"]
mod tests;
