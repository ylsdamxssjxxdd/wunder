use super::agent_runtime_store::SqliteAgentRuntimeStorage;
use super::SqliteStorage;
use crate::storage::{OrphanSweepResult, StorageLifecycle};
use anyhow::Result;
use std::collections::HashMap;

pub(super) trait SqliteRetentionStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>>;
    fn cleanup_orphan_session_logs_impl(&self, cutoff_epoch_s: f64) -> Result<OrphanSweepResult>;
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
            let conn = self.open_logs()?;
            conn.execute_batch("PRAGMA incremental_vacuum;")?;
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        Ok(deleted)
    }

    fn cleanup_orphan_session_logs_impl(&self, cutoff_epoch_s: f64) -> Result<OrphanSweepResult> {
        self.ensure_initialized()?;
        let mut deleted = HashMap::new();
        if cutoff_epoch_s > 0.0 && cutoff_epoch_s.is_finite() {
            let _ = self.runtime_logs()?;
            let mut conn = self.open_logs()?;
            self.attach_state_and_queue(&conn)?;
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            for table in ["runtime_tool_metrics", "runtime_artifact_resources"] {
                let count = tx.execute(&format!(
                    "DELETE FROM {table} WHERE created_time<?1
                     AND NOT EXISTS(SELECT 1 FROM control_state.chat_sessions c WHERE c.session_id={table}.session_id)
                     AND NOT EXISTS(SELECT 1 FROM deleted_session_log_grace g WHERE g.session_id={table}.session_id)"
                ), [cutoff_epoch_s])? as i64;
                if count > 0 {
                    deleted.insert(table.to_owned(), count);
                }
            }
            tx.commit()?;
        }
        Ok(OrphanSweepResult {
            deleted,
            completed: true,
            swept_until: cutoff_epoch_s,
            deferred: false,
        })
    }
}
