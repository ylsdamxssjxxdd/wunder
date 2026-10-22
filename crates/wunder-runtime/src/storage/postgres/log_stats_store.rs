use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter};
use serde_json::{json, Value};
use std::collections::HashMap;

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

impl PostgresLogStatsStorage for PostgresStorage {
    fn get_user_chat_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        crate::storage::runtime_logs::projected_chat_stats(self.canonical_threads()?.root())
    }

    fn get_user_tool_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let logs = self.runtime_logs()?;
        let conn = logs.connection();
        let mut query = String::from(
            "SELECT user_id, COUNT(*) as tool_records, MAX(created_time) as last_time FROM runtime_tool_metrics",
        );
        let mut filters = Vec::new();
        filters.push("excluded = 0".to_owned());
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY user_id");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, i64, f64)>, _>>()?;
        let mut stats = HashMap::new();
        for (user_id, count, last_time) in rows {
            let cleaned = user_id.trim();
            if cleaned.is_empty() {
                continue;
            }
            let mut entry = HashMap::new();
            entry.insert("tool_records".to_string(), count);
            entry.insert("last_time".to_string(), last_time.floor() as i64);
            stats.insert(cleaned.to_string(), entry);
        }
        Ok(stats)
    }

    fn get_user_usage_summary_impl(&self) -> Result<HashMap<String, (i64, i64)>> {
        let mut output = HashMap::new();
        for (user, stats) in self.get_user_chat_stats_impl()? {
            output.insert(user, (*stats.get("chat_records").unwrap_or(&0), 0));
        }
        for (user, stats) in self.get_user_tool_stats_impl()? {
            output.entry(user).or_insert((0, 0)).1 = *stats.get("tool_records").unwrap_or(&0);
        }
        Ok(output)
    }

    fn reconcile_user_usage_impl(&self, _full: bool) -> Result<()> {
        // Canonical history and bounded metrics are queried directly. There
        // is no secondary counter table to reconcile or rewrite while idle.
        Ok(())
    }

    fn get_tool_usage_stats_impl(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut query =
            String::from("SELECT tool, COUNT(*) as tool_records FROM runtime_tool_metrics");
        let mut params_list: Vec<SqlValue> = Vec::new();
        let mut filters = Vec::new();
        if let Some(since) = since_time.filter(|value| *value > 0.0) {
            filters.push("created_time >= ?".to_string());
            params_list.push(SqlValue::from(since));
        }
        if let Some(until) = until_time.filter(|value| *value > 0.0) {
            filters.push("created_time <= ?".to_string());
            params_list.push(SqlValue::from(until));
        }
        filters.push("excluded = 0".to_owned());
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY tool ORDER BY tool_records DESC");
        let logs = self.runtime_logs()?;
        let conn = logs.connection();
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<(String, i64)>, _>>()?;
        let mut stats = HashMap::new();
        for (tool, count) in rows {
            let cleaned = tool.trim();
            if cleaned.is_empty() {
                continue;
            }
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
            "SELECT session_id, user_id, COUNT(*) as tool_calls, MAX(created_time) as last_time FROM runtime_tool_metrics WHERE tool = ?",
        );
        let mut params_list: Vec<SqlValue> = vec![SqlValue::Text(cleaned.to_string())];
        let mut filters = Vec::new();
        if let Some(since) = since_time.filter(|value| *value > 0.0) {
            filters.push("created_time >= ?".to_string());
            params_list.push(SqlValue::from(since));
        }
        if let Some(until) = until_time.filter(|value| *value > 0.0) {
            filters.push("created_time <= ?".to_string());
            params_list.push(SqlValue::from(until));
        }
        filters.push("excluded = 0".to_owned());
        if !filters.is_empty() {
            query.push_str(" AND ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" GROUP BY session_id, user_id ORDER BY last_time DESC");
        let logs = self.runtime_logs()?;
        let conn = logs.connection();
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3).unwrap_or(0.0),
                ))
            })?
            .collect::<std::result::Result<Vec<(String, String, i64, f64)>, _>>()?;
        let mut sessions = Vec::new();
        for (session_id, user_id, tool_calls, last_time) in rows {
            let cleaned_session = session_id.trim();
            if cleaned_session.is_empty() {
                continue;
            }
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
        let logs = self.runtime_logs()?;
        let conn = logs.connection();
        let pages: i64 = conn.pragma_query_value(None, "page_count", |row| row.get(0))?;
        let free: i64 = conn.pragma_query_value(None, "freelist_count", |row| row.get(0))?;
        let size: i64 = conn.pragma_query_value(None, "page_size", |row| row.get(0))?;
        let local_bytes = pages
            .saturating_sub(free)
            .max(0)
            .saturating_mul(size.max(0)) as u64;
        drop(conn);
        let mut control = self.conn()?;
        let control_bytes: i64 = control.query_one(
            "SELECT COALESCE(pg_total_relation_size(to_regclass('monitor_sessions')),0) + COALESCE(pg_total_relation_size(to_regclass('stream_events')),0)", &[])? .get(0);
        Ok(local_bytes.saturating_add(control_bytes.max(0) as u64))
    }

    fn delete_logs_by_time_range_impl(
        &self,
        start_time: f64,
        end_time: f64,
    ) -> Result<HashMap<String, i64>> {
        let start = start_time.min(end_time);
        let end = start_time.max(end_time);
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
            return Ok(HashMap::new());
        }
        self.ensure_initialized()?;
        let mut control = self.conn()?;
        let mut tx = control.transaction()?;
        tx.batch_execute("SET LOCAL lock_timeout='2s'; SET LOCAL statement_timeout='8s'")?;
        let now = Self::now_ts();
        tx.execute("CREATE TEMP TABLE protected_chat_sessions ON COMMIT DROP AS
            SELECT session_id FROM session_locks WHERE expires_at>$1 OR suspended<>0
            UNION SELECT session_id FROM agent_tasks WHERE status IN ('pending','running','retry','queued','waiting')
            UNION SELECT session_id FROM session_runs WHERE status IN ('pending','running','queued','waiting','cancelling')
            UNION SELECT session_id FROM monitor_sessions WHERE status IN ('running','queued','waiting','cancelling')", &[&now])?;
        let mut output = HashMap::new();
        for (table, time) in [
            ("monitor_sessions", "COALESCE(updated_time,0)"),
            ("stream_events", "created_time"),
        ] {
            let count=tx.execute(&format!("DELETE FROM {table} WHERE {time}>=$1 AND {time}<=$2 AND NOT EXISTS(SELECT 1 FROM protected_chat_sessions p WHERE p.session_id={table}.session_id)"), &[&start,&end])?;
            output.insert(table.to_owned(), count as i64);
        }
        // The metric store is globally bounded. Fetch only protected session
        // identities which actually have diagnostic rows in this time range.
        let logs = self.runtime_logs()?;
        let candidates = {
            let conn = logs.connection();
            let mut statement=conn.prepare("SELECT DISTINCT session_id FROM runtime_tool_metrics WHERE created_time>=?1 AND created_time<=?2")?;
            let rows = statement.query_map(params![start, end], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut protected = Vec::new();
        for batch in candidates.chunks(100) {
            protected.extend(
                tx.query(
                    "SELECT session_id FROM protected_chat_sessions WHERE session_id=ANY($1)",
                    &[&batch],
                )?
                .into_iter()
                .map(|r| r.get::<_, String>(0)),
            );
        }
        tx.commit()?;
        let mut conn = logs.connection();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch("CREATE TEMP TABLE IF NOT EXISTS protected_metric_sessions(session_id TEXT PRIMARY KEY); DELETE FROM protected_metric_sessions;")?;
        for session in protected {
            tx.execute(
                "INSERT OR IGNORE INTO protected_metric_sessions(session_id) VALUES(?1)",
                [session],
            )?;
        }
        let removed=tx.execute("DELETE FROM runtime_tool_metrics WHERE created_time>=?1 AND created_time<=?2 AND NOT EXISTS(SELECT 1 FROM protected_metric_sessions p WHERE p.session_id=runtime_tool_metrics.session_id)", params![start,end])?;
        tx.commit()?;
        output.insert("runtime_tool_metrics".into(), removed as i64);
        Ok(output)
    }

    fn delete_thread_logs_by_user_impl(&self, user_id: &str) -> Result<i64> {
        super::super::canonical_thread_log::wait(
            self.canonical_threads()?.delete_user_threads(user_id),
        )?
    }

    fn delete_tool_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.runtime_logs()?.delete_tools(user_id, None)
    }

    fn delete_tool_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.runtime_logs()?.delete_tools(user_id, Some(session_id))
    }

    fn delete_artifact_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.runtime_logs()?.delete_artifacts(user_id, None)
    }

    fn delete_artifact_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.runtime_logs()?
            .delete_artifacts(user_id, Some(session_id))
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
