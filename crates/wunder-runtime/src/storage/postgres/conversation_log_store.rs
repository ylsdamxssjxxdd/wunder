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
        super::super::canonical_thread_log::wait(
            self.canonical_threads()?
                .append_chat_payload(user_id, payload),
        )?
    }

    fn append_tool_log_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.runtime_logs()?.append_tool(user_id, payload)
    }

    fn append_artifact_log_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.runtime_logs()?.append_artifact(user_id, payload)
    }

    fn load_artifact_logs_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.runtime_logs()?
            .load_artifacts(user_id, session_id, limit)
    }
}
