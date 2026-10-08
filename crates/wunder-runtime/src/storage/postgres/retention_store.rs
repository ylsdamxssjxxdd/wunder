use super::agent_runtime_store::PostgresAgentRuntimeStorage;
use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use std::collections::HashMap;

pub(super) trait PostgresRetentionStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>>;
}

impl PostgresRetentionStorage for PostgresStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>> {
        self.ensure_initialized()?;
        let mut deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 {
            return Ok(deleted);
        }
        // Only stream_events are ephemeral replay buffers. Conversation,
        // context, runtime and audit records stay durable and are removed only
        // through the administrator cleanup endpoint. PostgreSQL reclaims the
        // freed space itself via autovacuum.
        let removed = self.delete_stream_events_before_impl(cutoff_epoch_s)?;
        if removed > 0 {
            deleted.insert("stream_events".to_string(), removed);
        }
        Ok(deleted)
    }
}
