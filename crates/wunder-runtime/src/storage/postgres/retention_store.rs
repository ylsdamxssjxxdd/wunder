use super::PostgresStorage;
use crate::storage::StorageLifecycle;
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
        // Conversation, context, runtime and audit records are durable. The
        // retention scheduler remains a compatibility no-op; deletion is
        // exclusively performed through the administrator cleanup endpoint.
        let _ = retention_days;
        Ok(HashMap::new())
    }
}
