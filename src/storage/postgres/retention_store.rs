use super::PostgresStorage;
use crate::storage::StorageBackend;
use anyhow::Result;
use std::collections::HashMap;

pub(super) trait PostgresRetentionStorage {
    fn cleanup_retention_impl(&self, retention_days: i64) -> Result<HashMap<String, i64>>;
}

impl PostgresRetentionStorage for PostgresStorage {
    fn cleanup_retention_impl(&self, retention_days: i64) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        if retention_days <= 0 {
            return Ok(HashMap::new());
        }
        let cutoff = Self::now_ts() - (retention_days as f64 * 86400.0);
        if cutoff <= 0.0 {
            return Ok(HashMap::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query("SELECT user_id, roles FROM user_accounts", &[])?;
        let mut admin_ids = Vec::new();
        for row in rows {
            let user_id: String = row.get(0);
            let roles_raw: Option<String> = row.get(1);
            let roles = Self::parse_string_list(roles_raw);
            if roles
                .iter()
                .any(|role| role == "admin" || role == "super_admin")
            {
                admin_ids.push(user_id);
            }
        }
        let mut results = HashMap::new();
        let mut delete_with_filter = |base_sql: &str, allow_null_user: bool| -> Result<i64> {
            if admin_ids.is_empty() {
                return Ok(conn.execute(base_sql, &[&cutoff])? as i64);
            }
            let sql = if allow_null_user {
                format!("{base_sql} AND (user_id IS NULL OR user_id <> ALL($2))")
            } else {
                format!("{base_sql} AND user_id <> ALL($2)")
            };
            Ok(conn.execute(&sql, &[&cutoff, &admin_ids])? as i64)
        };
        let chat = delete_with_filter("DELETE FROM chat_history WHERE created_time < $1", false)?;
        results.insert("chat_history".to_string(), chat);
        let model_context = delete_with_filter(
            "DELETE FROM model_context_entries WHERE created_time < $1",
            false,
        )?;
        results.insert("model_context_entries".to_string(), model_context);
        let tool = delete_with_filter("DELETE FROM tool_logs WHERE created_time < $1", false)?;
        results.insert("tool_logs".to_string(), tool);
        let artifact =
            delete_with_filter("DELETE FROM artifact_logs WHERE created_time < $1", false)?;
        results.insert("artifact_logs".to_string(), artifact);
        let monitor =
            delete_with_filter("DELETE FROM monitor_sessions WHERE updated_time < $1", true)?;
        results.insert("monitor_sessions".to_string(), monitor);
        let stream =
            delete_with_filter("DELETE FROM stream_events WHERE created_time < $1", false)?;
        results.insert("stream_events".to_string(), stream);
        let session_runs =
            delete_with_filter("DELETE FROM session_runs WHERE updated_time < $1", false)?;
        results.insert("session_runs".to_string(), session_runs);
        Ok(results)
    }
}
