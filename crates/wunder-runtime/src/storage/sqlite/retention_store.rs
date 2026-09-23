use super::SqliteStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
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
        // Conversation, context, runtime and audit records are durable. The
        // retention scheduler remains a compatibility no-op; deletion is
        // exclusively performed through the administrator cleanup endpoint.
        let _ = retention_days;
        Ok(HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
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
            quota_balance: 0,
            quota_granted_total: 0,
            quota_used_total: 0,
            last_quota_grant_date: None,
            experience_total: 0,
            is_demo: false,
            created_at: 1.0,
            updated_at: 1.0,
            last_login_at: None,
        }
    }

    #[test]
    fn cleanup_retention_preserves_durable_history() {
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
        assert!(deleted.is_empty());
        assert_eq!(storage
            .load_model_context_entries("regular", "session-a", None)
            .expect("load regular entries")
            .len(), 1);
        assert_eq!(
            storage
                .load_model_context_entries("admin", "session-a", None)
                .expect("load admin entries")
                .len(),
            1
        );
    }
}
