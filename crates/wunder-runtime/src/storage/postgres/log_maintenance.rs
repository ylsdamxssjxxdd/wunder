//! Bounded deletion of ephemeral PostgreSQL transport records.
use super::{PgConn, PgTx};
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
