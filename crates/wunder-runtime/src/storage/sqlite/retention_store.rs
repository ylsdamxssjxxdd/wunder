use super::SqliteStorage;
use crate::storage::StorageBackend;
use anyhow::Result;
use rusqlite::params_from_iter;
use rusqlite::types::Value as SqlValue;
use std::collections::HashMap;

pub(super) trait SqliteRetentionStorage {
    fn cleanup_retention_impl(&self, retention_days: i64) -> Result<HashMap<String, i64>>;
}

impl SqliteRetentionStorage for SqliteStorage {
    fn cleanup_retention_impl(&self, retention_days: i64) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        if retention_days <= 0 {
            return Ok(HashMap::new());
        }
        let cutoff = Self::now_ts() - (retention_days as f64 * 86400.0);
        if cutoff <= 0.0 {
            return Ok(HashMap::new());
        }
        let conn = self.open()?;
        let mut results = HashMap::new();
        let mut admin_ids = Vec::new();
        let mut stmt = conn.prepare("SELECT user_id, roles FROM user_accounts")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<std::result::Result<Vec<(String, Option<String>)>, _>>()?;
        for (user_id, roles_raw) in rows {
            let roles = Self::parse_string_list(roles_raw);
            if roles
                .iter()
                .any(|role| role == "admin" || role == "super_admin")
            {
                admin_ids.push(user_id);
            }
        }
        let hookup_delete = |table: &str, time_field: &str, allow_null_user: bool| -> Result<i64> {
            let mut sql = format!("DELETE FROM {table} WHERE {time_field} < ?");
            let mut params: Vec<SqlValue> = vec![SqlValue::from(cutoff)];
            if !admin_ids.is_empty() {
                let placeholders = vec!["?"; admin_ids.len()].join(", ");
                if allow_null_user {
                    sql.push_str(&format!(
                        " AND (user_id IS NULL OR user_id NOT IN ({placeholders}))"
                    ));
                } else {
                    sql.push_str(&format!(" AND user_id NOT IN ({placeholders})"));
                }
                for user_id in &admin_ids {
                    params.push(SqlValue::from(user_id.clone()));
                }
            }
            Ok(conn.execute(&sql, params_from_iter(params))? as i64)
        };
        let chat = hookup_delete("chat_history", "created_time", false)?;
        results.insert("chat_history".to_string(), chat);
        let model_context = hookup_delete("model_context_entries", "created_time", false)?;
        results.insert("model_context_entries".to_string(), model_context);
        let tool = hookup_delete("tool_logs", "created_time", false)?;
        results.insert("tool_logs".to_string(), tool);
        let artifact = hookup_delete("artifact_logs", "created_time", false)?;
        results.insert("artifact_logs".to_string(), artifact);
        let monitor = hookup_delete("monitor_sessions", "COALESCE(updated_time, 0)", true)?;
        results.insert("monitor_sessions".to_string(), monitor);
        let stream = hookup_delete("stream_events", "created_time", false)?;
        results.insert("stream_events".to_string(), stream);
        let session_runs = hookup_delete("session_runs", "COALESCE(updated_time, 0)", false)?;
        results.insert("session_runs".to_string(), session_runs);
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::{StorageBackend, UserAccountRecord};
    use rusqlite::params;
    use tempfile::tempdir;

    fn sample_user(user_id: &str) -> UserAccountRecord {
        UserAccountRecord {
            user_id: user_id.to_string(),
            username: user_id.to_string(),
            email: None,
            password_hash: "hash".to_string(),
            roles: vec!["user".to_string()],
            status: "active".to_string(),
            access_level: "A".to_string(),
            unit_id: None,
            token_balance: 0,
            token_granted_total: 0,
            token_used_total: 0,
            last_token_grant_date: None,
            experience_total: 0,
            is_demo: false,
            created_at: 1.0,
            updated_at: 1.0,
            last_login_at: None,
        }
    }

    #[test]
    fn cleanup_retention_removes_expired_model_context_entries() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("model-context-retention.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        storage
            .upsert_user_account(&sample_user("regular"))
            .expect("insert regular user");
        let mut admin = sample_user("admin");
        admin.roles = vec!["admin".to_string()];
        storage
            .upsert_user_account(&admin)
            .expect("insert admin user");

        let conn = storage.open().expect("open sqlite");
        let expired = SqliteStorage::now_ts() - 3.0 * 86400.0;
        conn.execute(
            "INSERT INTO model_context_entries (user_id, session_id, role, payload, created_time)
             VALUES (?, ?, ?, ?, ?)",
            params![
                "regular",
                "session-a",
                "user",
                r#"{"role":"user","content":"expired"}"#,
                expired,
            ],
        )
        .expect("insert expired context");
        conn.execute(
            "INSERT INTO model_context_entries (user_id, session_id, role, payload, created_time)
             VALUES (?, ?, ?, ?, ?)",
            params![
                "admin",
                "session-a",
                "user",
                r#"{"role":"user","content":"admin-kept"}"#,
                expired,
            ],
        )
        .expect("insert admin context");
        drop(conn);

        let deleted = storage.cleanup_retention(1).expect("cleanup retention");
        assert_eq!(deleted.get("model_context_entries").copied(), Some(1));
        assert!(storage
            .load_model_context_entries("regular", "session-a", None)
            .expect("load regular entries")
            .is_empty());
        assert_eq!(
            storage
                .load_model_context_entries("admin", "session-a", None)
                .expect("load admin entries")
                .len(),
            1
        );
    }
}
