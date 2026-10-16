//! Bounded log deletion and coordination with usage-watermark reconciliation.
use super::{PgConn, PgTx};
use crate::storage::{USAGE_CHAT_WATERMARK_KEY, USAGE_TOOL_WATERMARK_KEY};
use anyhow::{ensure, Result};
use std::time::{Duration, Instant};
use tokio_postgres::types::ToSql;

pub(super) const ROW_BATCH: i64 = 256;

/// Limits work across all tables in one invocation, not just one DELETE.
pub(super) struct PurgeBudget {
    remaining: usize,
    deadline: Instant,
}

impl PurgeBudget {
    pub(super) fn new() -> Self {
        Self {
            remaining: 16,
            deadline: Instant::now() + Duration::from_secs(5),
        }
    }

    pub(super) fn check(&self) -> Result<()> {
        ensure!(
            self.remaining > 0 && Instant::now() < self.deadline,
            "log purge slice incomplete; retry from the durable deletion marker"
        );
        Ok(())
    }

    pub(super) fn committed(&mut self, rows: i64) {
        if rows > 0 {
            self.remaining = self.remaining.saturating_sub(1);
        }
    }
}

pub(super) fn begin<'a>(conn: &'a mut PgConn<'_>) -> Result<PgTx<'a>> {
    let mut tx = conn.transaction()?;
    tx.batch_execute("SET LOCAL lock_timeout = '2s'; SET LOCAL statement_timeout = '8s'")?;
    // PostgreSQL may otherwise hash every matching session row on each tiny
    // batch. Keep the DELETE on nested tuple lookups and the anti-joins on
    // their catalog indexes; these settings end with this transaction.
    tx.batch_execute("SET LOCAL enable_hashjoin = off; SET LOCAL enable_mergejoin = off")?;
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
) -> Result<i64> {
    let sql = format!(
        "WITH doomed AS MATERIALIZED (
            SELECT tableoid, ctid FROM {table} t WHERE {predicate} LIMIT {ROW_BATCH}
         ), del AS (
            DELETE FROM {table} t USING doomed d
            WHERE t.tableoid=d.tableoid AND t.ctid=d.ctid AND ({predicate}) RETURNING 1
         ) SELECT (SELECT COUNT(*) FROM del), (SELECT COUNT(*) FROM doomed)"
    );
    let mut total = 0;
    loop {
        budget.check()?;
        let mut tx = begin(conn)?;
        let row = tx.query_one(&sql, params)?;
        let removed: i64 = row.get(0);
        let selected: i64 = row.get(1);
        tx.commit()?;
        total += removed;
        budget.committed(selected);
        // A concurrent purge/update can remove the selected tuple before our
        // DELETE reaches it. An empty DELETE alone does not prove completion.
        if selected < ROW_BATCH && removed == selected {
            return Ok(total);
        }
    }
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
) -> Result<i64> {
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
    loop {
        budget.check()?;
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
        if selected < ROW_BATCH && removed == selected {
            return Ok(total);
        }
    }
}

#[cfg(test)]
#[path = "log_maintenance_tests.rs"]
mod tests;
