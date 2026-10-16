use super::{PgTx, PostgresStorage};
use crate::storage::{
    StorageLifecycle, USAGE_CHAT_WATERMARK_KEY, USAGE_FULL_RECONCILE_INTERVAL_S,
    USAGE_FULL_RECONCILE_KEY, USAGE_TOOL_WATERMARK_KEY,
};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio_postgres::types::ToSql;

pub(super) trait PostgresLogStatsStorage {
    fn get_user_chat_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_user_tool_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>>;
    fn get_user_usage_summary_impl(&self) -> Result<HashMap<String, (i64, i64)>>;
    fn reconcile_user_usage_impl(&self, full: bool) -> Result<()>;
    fn get_tool_usage_stats_impl(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>>;
    fn get_tool_session_usage_impl(
        &self,
        tool: &str,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<Vec<HashMap<String, Value>>>;
    fn get_log_usage_impl(&self) -> Result<u64>;
    fn delete_logs_by_time_range_impl(
        &self,
        start_time: f64,
        end_time: f64,
    ) -> Result<HashMap<String, i64>>;
    fn delete_thread_logs_by_user_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_tool_logs_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_tool_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn delete_artifact_logs_impl(&self, user_id: &str) -> Result<i64>;
    fn delete_artifact_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn mark_deleted_session_log_grace_impl(
        &self,
        user_id: &str,
        session_id: &str,
        deleted_at: f64,
    ) -> Result<()>;
    fn list_expired_deleted_session_log_grace_impl(
        &self,
        cutoff: f64,
        limit: i64,
    ) -> Result<Vec<(String, String)>>;
    fn delete_deleted_session_log_grace_impl(&self, user_id: &str, session_id: &str) -> Result<()>;
}

/// Exclusion is now a materialized column: the write path flags rows once,
/// stats queries never rescan the payload text.
fn append_tool_log_exclusions(filters: &mut Vec<String>) {
    filters.push("excluded = FALSE".to_string());
}

pub(super) fn read_meta_f64(tx: &mut PgTx<'_>, key: &str) -> Result<Option<f64>> {
    Ok(tx
        .query_opt("SELECT value FROM meta WHERE key = $1", &[&key])?
        .and_then(|row| row.get::<_, String>(0).trim().parse::<f64>().ok()))
}

pub(super) fn write_meta_f64(tx: &mut PgTx<'_>, key: &str, value: f64, now: f64) -> Result<()> {
    tx.execute(
        "INSERT INTO meta (key, value, updated_time) VALUES ($1, $2, $3) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_time = EXCLUDED.updated_time",
        &[&key, &value.to_string(), &now],
    )?;
    Ok(())
}

/// Decrement summary counters for rows that were inside the reconcile
/// watermarks when deleted. Summary values clamp at zero so a drifted
/// summary cannot go negative; the periodic full reconcile corrects drift.
pub(super) fn decrement_usage_summary(
    tx: &mut PgTx<'_>,
    chat: &[(String, i64)],
    tool: &[(String, i64)],
    now: f64,
) -> Result<()> {
    for (user_id, delta) in chat {
        if *delta > 0 {
            tx.execute(
                "UPDATE user_usage_summary SET chat_records = GREATEST(chat_records - $2, 0), \
                 updated_time = $3 WHERE user_id = $1",
                &[user_id, delta, &now],
            )?;
        }
    }
    for (user_id, delta) in tool {
        if *delta > 0 {
            tx.execute(
                "UPDATE user_usage_summary SET tool_records = GREATEST(tool_records - $2, 0), \
                 updated_time = $3 WHERE user_id = $1",
                &[user_id, delta, &now],
            )?;
        }
    }
    Ok(())
}

impl PostgresLogStatsStorage for PostgresStorage {
    fn get_user_chat_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT l.user_id, COUNT(i.item_id) as chat_records, COALESCE(MAX(i.updated_time), MAX(l.updated_time)) as last_time FROM thread_logs l LEFT JOIN thread_items i ON i.session_id=l.session_id AND i.user_id=l.user_id AND i.visibility='user' AND i.kind IN ('user_message','assistant_message') GROUP BY l.user_id",
            &[],
        )?;
        let mut stats = HashMap::new();
        for row in rows {
            let user_id: String = row.get(0);
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let count: i64 = row.get(1);
            let last_time: f64 = row.try_get(2).unwrap_or(0.0);
            let mut entry = HashMap::new();
            entry.insert("chat_records".to_string(), count);
            entry.insert("last_time".to_string(), last_time.floor() as i64);
            stats.insert(cleaned.to_string(), entry);
        }
        Ok(stats)
    }

    fn get_user_tool_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut query = String::from(
            "SELECT user_id, COUNT(*) as tool_records, MAX(created_time) as last_time FROM tool_logs",
        );
        let params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        let mut filters = Vec::new();
        append_tool_log_exclusions(&mut filters);
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY user_id");
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&query, &params_ref)?;
        let mut stats = HashMap::new();
        for row in rows {
            let user_id: String = row.get(0);
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let count: i64 = row.get(1);
            let last_time: f64 = row.try_get(2).unwrap_or(0.0);
            let mut entry = HashMap::new();
            entry.insert("tool_records".to_string(), count);
            entry.insert("last_time".to_string(), last_time.floor() as i64);
            stats.insert(cleaned.to_string(), entry);
        }
        Ok(stats)
    }

    fn get_user_usage_summary_impl(&self) -> Result<HashMap<String, (i64, i64)>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT user_id, chat_records, tool_records FROM user_usage_summary",
            &[],
        )?;
        let mut summary = HashMap::new();
        for row in rows {
            let user_id: String = row.get(0);
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            summary.insert(
                cleaned.to_string(),
                (row.get::<_, i64>(1), row.get::<_, i64>(2)),
            );
        }
        Ok(summary)
    }

    fn reconcile_user_usage_impl(&self, full: bool) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        // Reconciliation is maintenance work and is safe to retry from its
        // persisted watermarks.  Never hold summary/index locks for minutes
        // while a live session purge is waiting on the same user row.
        tx.execute("SET LOCAL lock_timeout = '2s'", &[])?;
        tx.execute("SET LOCAL statement_timeout = '20s'", &[])?;
        let now = Self::now_ts();
        let last_full = read_meta_f64(&mut tx, USAGE_FULL_RECONCILE_KEY)?;
        let needs_full = full
            || last_full.is_none()
            || now - last_full.unwrap_or(0.0) >= USAGE_FULL_RECONCILE_INTERVAL_S;
        if needs_full {
            // Full recompute: replace the summary in one transaction. Chat
            // counts follow the thread_logs join shape so both paths agree.
            tx.execute("DELETE FROM user_usage_summary", &[])?;
            tx.execute(
                "INSERT INTO user_usage_summary (user_id, chat_records, tool_records, updated_time) \
                 SELECT u.user_id, COALESCE(c.chat_records, 0), COALESCE(t.tool_records, 0), $1 \
                 FROM (SELECT user_id FROM thread_logs GROUP BY user_id \
                       UNION SELECT user_id FROM tool_logs WHERE excluded = FALSE GROUP BY user_id) u \
                 LEFT JOIN (SELECT l.user_id, COUNT(i.item_id) AS chat_records \
                            FROM thread_logs l \
                            LEFT JOIN thread_items i ON i.session_id = l.session_id AND i.user_id = l.user_id \
                              AND i.visibility = 'user' AND i.kind IN ('user_message','assistant_message') \
                            GROUP BY l.user_id) c ON c.user_id = u.user_id \
                 LEFT JOIN (SELECT user_id, COUNT(*) AS tool_records FROM tool_logs \
                            WHERE excluded = FALSE GROUP BY user_id) t ON t.user_id = u.user_id",
                &[&now],
            )?;
            let tool_max: i64 = tx
                .query_one("SELECT COALESCE(MAX(id), 0) FROM tool_logs", &[])?
                .get(0);
            let chat_max: f64 = tx
                .query_one(
                    "SELECT COALESCE(MAX(created_time), 0) FROM thread_items",
                    &[],
                )?
                .get(0);
            write_meta_f64(&mut tx, USAGE_TOOL_WATERMARK_KEY, tool_max as f64, now)?;
            write_meta_f64(&mut tx, USAGE_CHAT_WATERMARK_KEY, chat_max, now)?;
            write_meta_f64(&mut tx, USAGE_FULL_RECONCILE_KEY, now, now)?;
        } else {
            // Incremental: aggregate only rows past the watermarks, then
            // advance them inside the same transaction. Watermarks always
            // move forward; a weekly full pass corrects any drift.
            let tool_wm = read_meta_f64(&mut tx, USAGE_TOOL_WATERMARK_KEY)?.unwrap_or(0.0) as i64;
            let chat_wm = read_meta_f64(&mut tx, USAGE_CHAT_WATERMARK_KEY)?.unwrap_or(0.0);
            tx.execute(
                "INSERT INTO user_usage_summary (user_id, chat_records, tool_records, updated_time) \
                 SELECT user_id, 0, COUNT(*), $2 FROM tool_logs \
                 WHERE id > $1 AND excluded = FALSE GROUP BY user_id \
                 ON CONFLICT (user_id) DO UPDATE SET \
                   tool_records = user_usage_summary.tool_records + EXCLUDED.tool_records, \
                   updated_time = EXCLUDED.updated_time",
                &[&tool_wm, &now],
            )?;
            tx.execute(
                "INSERT INTO user_usage_summary (user_id, chat_records, tool_records, updated_time) \
                 SELECT user_id, COUNT(*), 0, $2 FROM thread_items \
                 WHERE created_time > $1 AND visibility = 'user' \
                   AND kind IN ('user_message','assistant_message') GROUP BY user_id \
                 ON CONFLICT (user_id) DO UPDATE SET \
                   chat_records = user_usage_summary.chat_records + EXCLUDED.chat_records, \
                   updated_time = EXCLUDED.updated_time",
                &[&chat_wm, &now],
            )?;
            let tool_max: i64 = tx
                .query_one("SELECT COALESCE(MAX(id), 0) FROM tool_logs", &[])?
                .get(0);
            let chat_max: f64 = tx
                .query_one(
                    "SELECT COALESCE(MAX(created_time), 0) FROM thread_items",
                    &[],
                )?
                .get(0);
            if tool_max as f64 > tool_wm as f64 {
                write_meta_f64(&mut tx, USAGE_TOOL_WATERMARK_KEY, tool_max as f64, now)?;
            }
            if chat_max > chat_wm {
                write_meta_f64(&mut tx, USAGE_CHAT_WATERMARK_KEY, chat_max, now)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn get_tool_usage_stats_impl(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut query = String::from("SELECT tool, COUNT(*) as tool_records FROM tool_logs");
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        let mut filters = Vec::new();
        if let Some(since) = since_time.filter(|value| *value > 0.0) {
            params.push(Box::new(since));
            filters.push(format!("created_time >= ${}", params.len()));
        }
        if let Some(until) = until_time.filter(|value| *value > 0.0) {
            params.push(Box::new(until));
            filters.push(format!("created_time <= ${}", params.len()));
        }
        append_tool_log_exclusions(&mut filters);
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY tool ORDER BY tool_records DESC");

        let mut conn = self.conn()?;
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&query, &params_ref)?;
        let mut stats = HashMap::new();
        for row in rows {
            let tool: Option<String> = row.try_get(0).ok();
            let Some(tool) = tool else {
                continue;
            };
            let cleaned = tool.trim();
            if cleaned.is_empty() {
                continue;
            }
            let count: i64 = row.get(1);
            stats.insert(cleaned.to_string(), count);
        }
        Ok(stats)
    }

    fn get_tool_session_usage_impl(
        &self,
        tool: &str,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.ensure_initialized()?;
        let cleaned = tool.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let mut query = String::from(
            "SELECT session_id, user_id, COUNT(*) as tool_calls, MAX(created_time) as last_time FROM tool_logs WHERE tool = $1",
        );
        let mut params: Vec<Box<dyn ToSql + Sync>> = vec![Box::new(cleaned.to_string())];
        let mut filters = Vec::new();
        if let Some(since) = since_time.filter(|value| *value > 0.0) {
            params.push(Box::new(since));
            filters.push(format!("created_time >= ${}", params.len()));
        }
        if let Some(until) = until_time.filter(|value| *value > 0.0) {
            params.push(Box::new(until));
            filters.push(format!("created_time <= ${}", params.len()));
        }
        append_tool_log_exclusions(&mut filters);
        if !filters.is_empty() {
            query.push_str(" AND ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY session_id, user_id ORDER BY last_time DESC");

        let mut conn = self.conn()?;
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&query, &params_ref)?;
        let mut sessions = Vec::new();
        for row in rows {
            let session_id: String = row.get(0);
            let cleaned_session = session_id.trim();
            if cleaned_session.is_empty() {
                continue;
            }
            let user_id: String = row.get(1);
            let tool_calls: i64 = row.get(2);
            let last_time: f64 = row.try_get(3).unwrap_or(0.0);
            let mut entry = HashMap::new();
            entry.insert("session_id".to_string(), json!(cleaned_session));
            entry.insert("user_id".to_string(), json!(user_id.trim()));
            entry.insert("tool_calls".to_string(), json!(tool_calls));
            entry.insert("last_time".to_string(), json!(last_time));
            sessions.push(entry);
        }
        Ok(sessions)
    }

    fn get_log_usage_impl(&self) -> Result<u64> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let row = conn.query_one(
            "SELECT \
            COALESCE(pg_total_relation_size(to_regclass('tool_logs')), 0) + \
            COALESCE(pg_total_relation_size(to_regclass('artifact_logs')), 0) + \
            COALESCE(pg_total_relation_size(to_regclass('monitor_sessions')), 0) + \
            COALESCE(pg_total_relation_size(to_regclass('stream_events')), 0) + \
            COALESCE(pg_total_relation_size(to_regclass('memory_task_logs')), 0)",
            &[],
        )?;
        let total: i64 = row.get(0);
        Ok(total.max(0) as u64)
    }

    fn delete_logs_by_time_range_impl(
        &self,
        start_time: f64,
        end_time: f64,
    ) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let start = start_time.min(end_time);
        let end = start_time.max(end_time);
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
            return Ok(HashMap::new());
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("SET LOCAL lock_timeout = '5s'", &[])?;
        tx.execute("SET LOCAL statement_timeout = '30s'", &[])?;
        // Serialize this rare maintenance command with new thread starts; a
        // transaction alone would still allow a start between guard and deletion.
        tx.execute("LOCK TABLE chat_sessions, session_locks, agent_tasks, session_runs, monitor_sessions, \
            thread_logs, thread_turns, thread_items, thread_item_blocks, thread_log_changes, thread_log_metrics, stream_events, tool_logs, artifact_logs, cron_jobs, session_goals \
            IN SHARE ROW EXCLUSIVE MODE", &[])?;
        let now = Self::now_ts();
        let live = crate::storage::session_cleanup::LIVE_SESSION_PREDICATE.replace(":now", "$1");
        // Freeze the protected set before deleting monitor evidence. Never erase
        // the replay/history of a running, queued or approval-suspended thread.
        tx.execute(
            &format!(
                "CREATE TEMP TABLE protected_chat_sessions ON COMMIT DROP AS \
            SELECT c.session_id FROM chat_sessions c WHERE {live}"
            ),
            &[&now],
        )?;
        let mut results = HashMap::new();
        // tool_logs: rows inside the usage watermark were counted in the
        // summary and must decrement it; rows past the watermark (or excluded)
        // were never counted and are removed without adjustment.
        let tool_wm = read_meta_f64(&mut tx, USAGE_TOOL_WATERMARK_KEY)?.unwrap_or(0.0) as i64;
        let counted_rows = tx.query(
            "WITH del AS (DELETE FROM tool_logs WHERE created_time >= $1 AND created_time <= $2 \
               AND excluded = FALSE AND id <= $3 \
               AND NOT EXISTS (SELECT 1 FROM protected_chat_sessions p WHERE p.session_id = tool_logs.session_id) \
               RETURNING user_id) SELECT user_id, COUNT(*) FROM del GROUP BY user_id",
            &[&start, &end, &tool_wm],
        )?;
        let mut tool_counted = 0i64;
        let mut tool_deltas: Vec<(String, i64)> = Vec::new();
        for row in counted_rows {
            let user_id: String = row.get(0);
            let count: i64 = row.get(1);
            tool_counted += count;
            tool_deltas.push((user_id, count));
        }
        let tool_rest = tx.execute(
            "DELETE FROM tool_logs WHERE created_time >= $1 AND created_time <= $2 \
             AND NOT EXISTS (SELECT 1 FROM protected_chat_sessions p WHERE p.session_id = tool_logs.session_id)",
            &[&start, &end],
        )? as i64;
        results.insert("tool_logs".to_string(), tool_counted + tool_rest);
        decrement_usage_summary(&mut tx, &[], &tool_deltas, Self::now_ts())?;
        let mut delete_range = |table: &str, time_field: &str| -> Result<i64> {
            let sql =
                format!("DELETE FROM {table} WHERE {time_field} >= $1 AND {time_field} <= $2 \
                    AND NOT EXISTS (SELECT 1 FROM protected_chat_sessions p WHERE p.session_id = {table}.session_id)");
            Ok(tx.execute(&sql, &[&start, &end])? as i64)
        };
        results.insert(
            "artifact_logs".to_string(),
            delete_range("artifact_logs", "created_time")?,
        );
        results.insert(
            "monitor_sessions".to_string(),
            delete_range("monitor_sessions", "updated_time")?,
        );
        results.insert(
            "stream_events".to_string(),
            delete_range("stream_events", "created_time")?,
        );
        results.insert(
            "memory_task_logs".to_string(),
            delete_range("memory_task_logs", "updated_time")?,
        );
        results.insert(
            "chat_sessions".to_string(),
            super::session_cleanup::remove_empty_catalog(&mut tx, start, end, now)?,
        );
        tx.commit()?;
        Ok(results)
    }

    fn delete_thread_logs_by_user_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        if user_id.trim().is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        // User-wide log deletion is an explicit administrative operation, but
        // it must still yield to live traffic.  The transaction rolls back on
        // timeout and can be resumed by the caller instead of retaining locks
        // for minutes.
        tx.execute("SET LOCAL lock_timeout = '2s'", &[])?;
        tx.execute("SET LOCAL statement_timeout = '20s'", &[])?;
        let now = Self::now_ts();
        let chat_wm = read_meta_f64(&mut tx, USAGE_CHAT_WATERMARK_KEY)?.unwrap_or(0.0);
        let mut removed = 0i64;
        tx.execute("DELETE FROM thread_trajectory_tiles WHERE session_id IN (SELECT session_id FROM thread_view_heads WHERE user_id=$1)",&[&user_id])?;
        for table in [
            "thread_body_search",
            "thread_body_manifests",
            "thread_body_chunks",
            "thread_chat_rows",
            "thread_trajectory_rows",
            "thread_view_heads",
            "thread_item_blocks",
            "thread_log_changes",
            "thread_turns",
            "thread_log_metrics",
            "thread_logs",
        ] {
            removed += tx.execute(
                &format!("DELETE FROM {table} WHERE user_id=$1"),
                &[&user_id],
            )? as i64;
        }
        // thread_items: only the visible-message rows inside the chat
        // watermark were counted; the remainder was never in the summary.
        let counted: i64 = tx
            .query_one(
                "WITH del AS (DELETE FROM thread_items WHERE user_id=$1 AND visibility='user' \
                   AND kind IN ('user_message','assistant_message') AND created_time <= $2 RETURNING 1) \
                 SELECT COUNT(*) FROM del",
                &[&user_id, &chat_wm],
            )?
            .get(0);
        let rest = tx.execute("DELETE FROM thread_items WHERE user_id=$1", &[&user_id])? as i64;
        removed += counted + rest;
        decrement_usage_summary(&mut tx, &[(user_id.to_string(), counted)], &[], now)?;
        tx.commit()?;
        Ok(removed)
    }

    fn delete_tool_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("SET LOCAL lock_timeout = '2s'", &[])?;
        tx.execute("SET LOCAL statement_timeout = '20s'", &[])?;
        let now = Self::now_ts();
        let tool_wm = read_meta_f64(&mut tx, USAGE_TOOL_WATERMARK_KEY)?.unwrap_or(0.0) as i64;
        let counted: i64 = tx
            .query_one(
                "WITH del AS (DELETE FROM tool_logs WHERE user_id=$1 AND excluded=FALSE \
                   AND id <= $2 RETURNING 1) SELECT COUNT(*) FROM del",
                &[&cleaned, &tool_wm],
            )?
            .get(0);
        let rest = tx.execute("DELETE FROM tool_logs WHERE user_id=$1", &[&cleaned])? as i64;
        decrement_usage_summary(&mut tx, &[], &[(cleaned.to_string(), counted)], now)?;
        tx.commit()?;
        Ok(counted + rest)
    }

    fn delete_tool_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("SET LOCAL lock_timeout = '2s'", &[])?;
        // A session tombstone retries this work.  Bound the whole
        // delete-and-counter transaction so it cannot monopolize a user's
        // summary row while the reconciler is trying to advance its watermark.
        tx.execute("SET LOCAL statement_timeout = '20s'", &[])?;
        let now = Self::now_ts();
        let tool_wm = read_meta_f64(&mut tx, USAGE_TOOL_WATERMARK_KEY)?.unwrap_or(0.0) as i64;
        let counted: i64 = tx
            .query_one(
                "WITH del AS (DELETE FROM tool_logs WHERE user_id=$1 AND session_id=$2 \
                   AND excluded=FALSE AND id <= $3 RETURNING 1) SELECT COUNT(*) FROM del",
                &[&cleaned_user, &cleaned_session, &tool_wm],
            )?
            .get(0);
        let rest = tx.execute(
            "DELETE FROM tool_logs WHERE user_id=$1 AND session_id=$2",
            &[&cleaned_user, &cleaned_session],
        )? as i64;
        decrement_usage_summary(&mut tx, &[], &[(cleaned_user.to_string(), counted)], now)?;
        tx.commit()?;
        Ok(counted + rest)
    }

    fn delete_artifact_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM artifact_logs WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn delete_artifact_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("SET LOCAL lock_timeout = '2s'", &[])?;
        tx.execute("SET LOCAL statement_timeout = '20s'", &[])?;
        let affected = tx.execute(
            "DELETE FROM artifact_logs WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        tx.commit()?;
        Ok(affected as i64)
    }

    fn mark_deleted_session_log_grace_impl(
        &self,
        user_id: &str,
        session_id: &str,
        deleted_at: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || !deleted_at.is_finite() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO deleted_session_log_grace (user_id, session_id, deleted_at) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (user_id, session_id) DO UPDATE SET deleted_at = excluded.deleted_at",
            &[&cleaned_user, &cleaned_session, &deleted_at],
        )?;
        Ok(())
    }

    fn list_expired_deleted_session_log_grace_impl(
        &self,
        cutoff: f64,
        limit: i64,
    ) -> Result<Vec<(String, String)>> {
        self.ensure_initialized()?;
        if !cutoff.is_finite() || limit <= 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT user_id, session_id FROM deleted_session_log_grace \
             WHERE deleted_at <= $1 ORDER BY deleted_at ASC LIMIT $2",
            &[&cutoff, &limit],
        )?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect())
    }

    fn delete_deleted_session_log_grace_impl(&self, user_id: &str, session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "DELETE FROM deleted_session_log_grace WHERE user_id = $1 AND session_id = $2",
            &[&user_id, &session_id],
        )?;
        Ok(())
    }
}
