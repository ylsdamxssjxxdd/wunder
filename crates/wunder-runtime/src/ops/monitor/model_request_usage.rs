use super::{now_ts, MonitorState};

impl MonitorState {
    /// Count dispatched provider requests and return the absolute thread total.
    pub fn record_model_request(&self, session_id: &str, count: i64) -> Option<i64> {
        if count <= 0 {
            return None;
        }
        let now = now_ts();
        let (total, to_persist) = {
            let mut sessions = self.sessions.lock();
            let record = sessions.get_mut(session_id)?;
            record.model_request_count = record
                .model_request_count
                .map(|value| value.saturating_add(count));
            record.updated_time = now;
            record.dirty = true;
            (
                record.model_request_count,
                self.maybe_persist_record(record, now, false),
            )
        };
        if let Some(record) = to_persist {
            self.save_record(&record);
        }
        total
    }
}

#[cfg(all(test, feature = "sqlite-storage"))]
mod tests {
    use super::*;
    use crate::config::ObservabilityConfig;
    use crate::storage::{SqliteStorage, StorageBackend};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn thread_quota_survives_event_eviction_storage_reload_and_new_turns() {
        let root = tempfile::tempdir().unwrap();
        let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
            root.path().join("state.db").to_string_lossy().into_owned(),
        ));
        storage.ensure_initialized().unwrap();
        let monitor = MonitorState::new(
            storage.clone(),
            ObservabilityConfig::default(),
            root.path().to_string_lossy().into_owned(),
        );
        monitor.register("session_1", "user_1", "", "", false, false);
        assert_eq!(monitor.record_model_request("session_1", 1), Some(1));
        assert_eq!(monitor.record_model_request("session_1", 1), Some(2));
        monitor.register("session_1", "user_1", "", "", false, false);
        assert_eq!(monitor.record_model_request("session_1", 1), Some(3));
        monitor.record_event(
            "session_1",
            "model_request_usage",
            &json!({"session_request_count":3,"request_count":1}),
        );
        let mut payload = monitor.sessions.lock()["session_1"].to_storage();
        payload["events"] = json!([]);
        let hydrated = super::super::SessionRecord::from_storage(&payload).unwrap();
        assert_eq!(
            (
                hydrated.model_request_count,
                hydrated.consumed_tokens,
                hydrated.tool_calls
            ),
            (Some(3), 0, 0)
        );
        // A cold catalog must read the same serialized summary, without scanning messages.
        payload["session_id"] = json!("session_2");
        storage.upsert_monitor_record(&payload).unwrap();
        assert_eq!(
            monitor.session_usage_summaries(&["session_2".into()])["session_2"],
            (0, 0, Some(3))
        );
        payload
            .as_object_mut()
            .unwrap()
            .remove("model_request_count");
        payload.as_object_mut().unwrap().remove("quota_used");
        assert_eq!(
            super::super::SessionRecord::from_storage(&payload)
                .unwrap()
                .model_request_count,
            None
        );
    }
}
