use super::agent_runtime_store::SqliteAgentRuntimeStorage;
use super::SqliteStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use std::collections::HashMap;

pub(super) trait SqliteRetentionStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>>;
}

impl SqliteRetentionStorage for SqliteStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 {
            return Ok(deleted);
        }
        // Only stream_events are ephemeral replay buffers. Conversation,
        // context, runtime and audit records stay durable and are removed only
        // through the administrator cleanup endpoint.
        let removed = self.delete_stream_events_before_impl(cutoff_epoch_s)?;
        if removed > 0 {
            deleted.insert("stream_events".to_string(), removed);
            // Reclaim pages freed by the deletion and truncate the WAL so the
            // database file actually shrinks after retention cleanup.
            let conn = self.open()?;
            conn.execute_batch("PRAGMA incremental_vacuum;")?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
    use rusqlite::params;
    use tempfile::tempdir;

    fn insert_stream_event(
        storage: &SqliteStorage,
        session_id: &str,
        event_id: i64,
        created_time: f64,
    ) {
        let conn = storage.open().expect("open sqlite");
        conn.execute(
            "INSERT INTO stream_events (session_id, event_id, user_id, payload, created_time)
             VALUES (?, ?, ?, ?, ?)",
            params![
                session_id,
                event_id,
                "user-a",
                r#"{"event":"final"}"#,
                created_time
            ],
        )
        .expect("insert stream event");
    }

    fn count_stream_events(storage: &SqliteStorage) -> i64 {
        let conn = storage.open().expect("open sqlite");
        conn.query_row("SELECT COUNT(*) FROM stream_events", [], |row| row.get(0))
            .expect("count stream events")
    }

    #[test]
    fn cleanup_retention_removes_only_expired_stream_events() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("stream-event-retention.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");

        let now = SqliteStorage::now_ts();
        insert_stream_event(&storage, "session-a", 1, now - 48.0 * 3600.0);
        insert_stream_event(&storage, "session-a", 2, now - 3600.0);
        // Durable records sharing the same age must survive cleanup.
        storage
            .append_chat(
                "user-a",
                &serde_json::json!({
                    "session_id": "session-a",
                    "role": "user",
                    "content": "keep me"
                }),
            )
            .expect("append chat");

        let cutoff = now - 24.0 * 3600.0;
        let deleted = storage
            .cleanup_retention(cutoff)
            .expect("cleanup retention");
        assert_eq!(deleted.get("stream_events").copied(), Some(1));
        assert_eq!(count_stream_events(&storage), 1);
        assert_eq!(
            storage
                .load_chat_history("user-a", "session-a", None)
                .expect("load chat history")
                .len(),
            1
        );
    }

    #[test]
    fn cleanup_retention_disabled_with_nonpositive_cutoff() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("stream-event-retention-disabled.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize storage");
        insert_stream_event(&storage, "session-a", 1, 1.0);

        let deleted = storage.cleanup_retention(0.0).expect("cleanup retention");
        assert!(deleted.is_empty());
        assert_eq!(count_stream_events(&storage), 1);
    }
}
