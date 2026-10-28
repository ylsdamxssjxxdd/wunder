use super::runtime_log_store::PostgresRuntimeLogStorage;
use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
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
        self.ensure_initialized()?;
        let _store = self.service_threads_ref()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT owner.user_id,COUNT(*)::BIGINT,
                MAX((item.preview_json::jsonb->>'created_at_ms')::bigint)::DOUBLE PRECISION/1000
             FROM service_thread_items_v2 item
             JOIN service_thread_owners owner ON owner.thread_id=item.thread_id
             WHERE item.visible AND item.preview_json::jsonb->>'kind' IN ('user_message','assistant_message')
             GROUP BY owner.user_id",
            &[],
        )?;
        let mut output = HashMap::new();
        for row in rows {
            let user: String = row.get(0);
            let mut stats = HashMap::new();
            stats.insert("chat_records".into(), row.get::<_, i64>(1));
            stats.insert(
                "last_time".into(),
                row.get::<_, Option<f64>>(2).unwrap_or(0.0) as i64,
            );
            output.insert(user, stats);
        }
        Ok(output)
    }

    fn get_user_tool_stats_impl(&self) -> Result<HashMap<String, HashMap<String, i64>>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query("SELECT user_id, COUNT(*)::BIGINT, MAX(created_time) FROM runtime_tool_metrics WHERE excluded=FALSE GROUP BY user_id", &[])?
            .into_iter().map(|row| (row.get::<_, String>(0), row.get::<_, i64>(1), row.get::<_, Option<f64>>(2).unwrap_or(0.0))).collect::<Vec<_>>();
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
        let mut conn = self.conn()?;
        let mut clauses = vec!["excluded=FALSE".to_owned()];
        let mut values: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = Vec::new();
        if let Some(since) = since_time.filter(|v| *v > 0.0) {
            values.push(Box::new(since));
            clauses.push(format!("created_time >= ${}", values.len()));
        }
        if let Some(until) = until_time.filter(|v| *v > 0.0) {
            values.push(Box::new(until));
            clauses.push(format!("created_time <= ${}", values.len()));
        }
        let query = format!("SELECT tool, COUNT(*)::BIGINT FROM runtime_tool_metrics WHERE {} GROUP BY tool ORDER BY COUNT(*) DESC", clauses.join(" AND "));
        let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            values.iter().map(|v| &**v as _).collect();
        let rows = conn
            .query(&query, &params)?
            .into_iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, i64>(1)))
            .collect::<Vec<_>>();
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
        let mut conn = self.conn()?;
        let mut clauses = vec!["tool=$1".to_owned(), "excluded=FALSE".to_owned()];
        let mut values: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> =
            vec![Box::new(cleaned.to_owned())];
        if let Some(since) = since_time.filter(|v| *v > 0.0) {
            values.push(Box::new(since));
            clauses.push(format!("created_time >= ${}", values.len()));
        }
        if let Some(until) = until_time.filter(|v| *v > 0.0) {
            values.push(Box::new(until));
            clauses.push(format!("created_time <= ${}", values.len()));
        }
        let query = format!("SELECT session_id,user_id,COUNT(*)::BIGINT,MAX(created_time) FROM runtime_tool_metrics WHERE {} GROUP BY session_id,user_id ORDER BY MAX(created_time) DESC", clauses.join(" AND "));
        let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            values.iter().map(|v| &**v as _).collect();
        let rows = conn
            .query(&query, &params)?
            .into_iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    row.get::<_, String>(1),
                    row.get::<_, i64>(2),
                    row.get::<_, Option<f64>>(3).unwrap_or(0.0),
                )
            })
            .collect::<Vec<_>>();
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
        let mut control = self.conn()?;
        let control_bytes: i64 = control.query_one(
            "SELECT COALESCE(pg_total_relation_size(to_regclass('monitor_sessions')),0) + COALESCE(pg_total_relation_size(to_regclass('stream_events')),0) + COALESCE(pg_total_relation_size(to_regclass('runtime_tool_metrics')),0) + COALESCE(pg_total_relation_size(to_regclass('runtime_artifact_resources')),0)", &[])? .get(0);
        Ok(control_bytes.max(0) as u64)
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
        // Keep protection and deletion in one database transaction. A single
        // anti-join replaces fetching every diagnostic session into memory.
        let removed = tx.execute(
            "DELETE FROM runtime_tool_metrics metric
             WHERE created_time>=$1 AND created_time<=$2 AND NOT EXISTS(
                SELECT 1 FROM protected_chat_sessions protected
                WHERE protected.session_id=metric.session_id)",
            &[&start, &end],
        )?;
        tx.commit()?;
        output.insert("runtime_tool_metrics".into(), removed as i64);
        Ok(output)
    }

    fn delete_thread_logs_by_user_impl(&self, user_id: &str) -> Result<i64> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Ok(0);
        }
        self.ensure_initialized()?;
        let _store = self.service_threads_ref()?;
        let mut conn = self.conn()?;
        let mut removed = 0_i64;
        let mut after: Option<String> = None;
        // A bounded keyset batch locks owners in a stable order. Admissions
        // use the same owner lock, so accepted bodies cannot survive deletion.
        loop {
            let mut tx = conn.transaction()?;
            tx.batch_execute("SET LOCAL lock_timeout='2s'; SET LOCAL statement_timeout='8s'")?;
            let ids = tx
                .query(
                    "SELECT thread_id FROM service_thread_owners WHERE user_id=$1
                 AND ($2::text IS NULL OR thread_id>$2) ORDER BY thread_id LIMIT 100",
                    &[&user_id, &after],
                )?
                .into_iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>();
            if ids.is_empty() {
                tx.commit()?;
                break;
            }
            let next_after = ids.last().cloned();
            for id in &ids {
                super::PostgresAdmissionStore::lock_thread(&mut tx, id)?;
            }
            let ids = tx.query(
                "SELECT thread_id FROM service_thread_owners WHERE user_id=$1 AND thread_id=ANY($2) ORDER BY thread_id FOR UPDATE",
                &[&user_id, &ids],
            )?.into_iter().map(|row| row.get::<_,String>(0)).collect::<Vec<_>>();
            if ids.is_empty() {
                tx.commit()?;
                after = next_after;
                continue;
            }
            tx.execute(
                "DELETE FROM service_thread_admissions WHERE thread_id=ANY($1)",
                &[&ids],
            )?;
            // Canonical records, session metadata and all display indexes have
            // ON DELETE CASCADE, so the thread root is the sole history delete.
            tx.execute(
                "DELETE FROM service_threads WHERE thread_id=ANY($1)",
                &[&ids],
            )?;
            tx.execute(
                "DELETE FROM service_thread_owners WHERE user_id=$1 AND thread_id=ANY($2)",
                &[&user_id, &ids],
            )?;
            tx.commit()?;
            removed = removed.saturating_add(ids.len() as i64);
            after = next_after;
        }
        Ok(removed)
    }

    fn delete_tool_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.delete_tool_metrics_impl(user_id, None)
    }

    fn delete_tool_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_tool_metrics_impl(user_id, Some(session_id))
    }

    fn delete_artifact_logs_impl(&self, user_id: &str) -> Result<i64> {
        self.delete_artifact_resources_impl(user_id, None)
    }

    fn delete_artifact_logs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.delete_artifact_resources_impl(user_id, Some(session_id))
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
