use crate::storage::StorageBackend;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

#[derive(Clone)]
pub struct StreamEventService {
    storage: Arc<dyn StorageBackend>,
    stream_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    tail_cache: Arc<RwLock<HashMap<String, i64>>>,
}

impl StreamEventService {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            storage,
            stream_locks: Arc::new(RwLock::new(HashMap::new())),
            tail_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn append_event(
        &self,
        session_id: &str,
        user_id: &str,
        payload: Value,
    ) -> Result<i64> {
        let cleaned_session = normalize_session_id(session_id)?;
        let cleaned_user = normalize_user_id(user_id)?;
        let lock = self.ensure_stream_lock(&cleaned_session).await;
        let _guard = lock.lock().await;
        let cached_tail = self.cached_tail(&cleaned_session).await;
        // Read storage tail inside the per-stream lock to avoid stale cache overwrite.
        let storage_tail = self
            .load_tail_from_storage(cleaned_session.clone())
            .await
            .unwrap_or(0);
        let next_event_id = cached_tail.max(storage_tail).saturating_add(1);
        let storage = self.storage.clone();
        let session_snapshot = cleaned_session.clone();
        let user_snapshot = cleaned_user.clone();
        tokio::task::spawn_blocking(move || {
            storage.append_stream_event(&session_snapshot, &user_snapshot, next_event_id, &payload)
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))?;
        self.update_tail_cache(&cleaned_session, next_event_id)
            .await;
        Ok(next_event_id)
    }

    pub async fn list_events(
        &self,
        session_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        let cleaned_session = normalize_session_id(session_id)?;
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let storage = self.storage.clone();
        let session_snapshot = cleaned_session.clone();
        let events = tokio::task::spawn_blocking(move || {
            storage.load_stream_events(&session_snapshot, after_event_id.max(0), limit.max(1))
        })
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))?;
        if let Some(max_event_id) = events.iter().filter_map(extract_event_id).max() {
            self.update_tail_cache(&cleaned_session, max_event_id).await;
        }
        Ok(events)
    }

    pub async fn tail_event_id(&self, session_id: &str) -> Result<i64> {
        let cleaned_session = normalize_session_id(session_id)?;
        let cached = self.cached_tail(&cleaned_session).await;
        if cached > 0 {
            return Ok(cached);
        }
        let tail = self
            .load_tail_from_storage(cleaned_session.clone())
            .await
            .unwrap_or(0);
        if tail > 0 {
            self.update_tail_cache(&cleaned_session, tail).await;
        }
        Ok(tail)
    }

    async fn load_tail_from_storage(&self, session_id: String) -> Result<i64> {
        let storage = self.storage.clone();
        tokio::task::spawn_blocking(move || storage.get_max_stream_event_id(&session_id))
            .await
            .unwrap_or_else(|err| Err(anyhow!(err)))
    }

    async fn cached_tail(&self, session_id: &str) -> i64 {
        self.tail_cache
            .read()
            .await
            .get(session_id)
            .copied()
            .unwrap_or(0)
    }

    async fn update_tail_cache(&self, session_id: &str, event_id: i64) {
        if event_id <= 0 {
            return;
        }
        let mut guard = self.tail_cache.write().await;
        let entry = guard.entry(session_id.to_string()).or_insert(0);
        *entry = (*entry).max(event_id);
    }

    async fn ensure_stream_lock(&self, session_id: &str) -> Arc<Mutex<()>> {
        if let Some(lock) = self.stream_locks.read().await.get(session_id).cloned() {
            return lock;
        }
        let mut guard = self.stream_locks.write().await;
        guard
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

fn normalize_session_id(session_id: &str) -> Result<String> {
    let cleaned = session_id.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("session_id is required"));
    }
    Ok(cleaned.to_string())
}

fn normalize_user_id(user_id: &str) -> Result<String> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("user_id is required"));
    }
    Ok(cleaned.to_string())
}

fn extract_event_id(value: &Value) -> Option<i64> {
    value.get("event_id").and_then(Value::as_i64)
}

#[cfg(test)]
mod tests {
    use super::StreamEventService;
    use crate::storage::{SqliteStorage, StorageBackend};
    use serde_json::json;
    use std::sync::Arc;

    fn build_service() -> StreamEventService {
        let db_path = std::env::temp_dir().join(format!(
            "wunder_stream_events_{}.db",
            uuid::Uuid::new_v4().simple()
        ));
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        StreamEventService::new(storage)
    }

    #[tokio::test]
    async fn append_event_assigns_monotonic_ids() {
        let service = build_service();
        let first = service
            .append_event(
                "chat::user-a::sess-a",
                "user-a",
                json!({"event":"channel_message","data":{"seq":1}}),
            )
            .await
            .expect("append first event");
        let second = service
            .append_event(
                "chat::user-a::sess-a",
                "user-a",
                json!({"event":"channel_message","data":{"seq":2}}),
            )
            .await
            .expect("append second event");
        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_eq!(
            service
                .tail_event_id("chat::user-a::sess-a")
                .await
                .expect("tail event id"),
            2
        );
    }

    #[tokio::test]
    async fn append_event_is_serialized_per_stream() {
        let service = Arc::new(build_service());
        let mut tasks = Vec::new();
        for seq in 0..40 {
            let service_snapshot = service.clone();
            tasks.push(tokio::spawn(async move {
                service_snapshot
                    .append_event(
                        "chat::user-a::sess-concurrent",
                        "user-a",
                        json!({"event":"channel_message","data":{"seq":seq}}),
                    )
                    .await
                    .expect("append event")
            }));
        }
        let mut ids = Vec::new();
        for task in tasks {
            ids.push(task.await.expect("task should finish"));
        }
        ids.sort_unstable();
        assert_eq!(ids.first().copied(), Some(1));
        assert_eq!(ids.last().copied(), Some(40));
        let events = service
            .list_events("chat::user-a::sess-concurrent", 0, 200)
            .await
            .expect("load events");
        assert_eq!(events.len(), 40);
    }

    #[tokio::test]
    async fn list_events_updates_tail_cache() {
        let service = build_service();
        for seq in 0..3 {
            service
                .append_event(
                    "chat::user-a::sess-cache",
                    "user-a",
                    json!({"event":"channel_message","data":{"seq":seq}}),
                )
                .await
                .expect("append event");
        }
        let replay = service
            .list_events("chat::user-a::sess-cache", 1, 10)
            .await
            .expect("replay");
        assert_eq!(replay.len(), 2);
        assert_eq!(
            service
                .tail_event_id("chat::user-a::sess-cache")
                .await
                .expect("tail event id"),
            3
        );
    }
}
