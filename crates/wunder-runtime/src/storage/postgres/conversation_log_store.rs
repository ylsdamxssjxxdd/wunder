use super::runtime_log_store::PostgresRuntimeLogStorage;
use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use serde_json::Value;

pub(super) trait PostgresConversationLogStorage {
    fn append_chat_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_tool_log_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn append_artifact_log_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
    fn load_artifact_logs_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>>;
}

impl PostgresConversationLogStorage for PostgresStorage {
    fn append_chat_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let store = self.service_threads_ref()?.clone();
        super::super::canonical_thread_log::wait(store.append_chat_payload(user_id, payload))??;
        Ok(())
    }

    fn append_tool_log_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.append_tool_metric_impl(user_id, payload)
    }

    fn append_artifact_log_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.append_artifact_resource_impl(user_id, payload)
    }

    fn load_artifact_logs_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.load_artifact_resources_impl(user_id, session_id, limit)
    }
}
