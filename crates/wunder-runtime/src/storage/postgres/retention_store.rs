use super::agent_runtime_store::PostgresAgentRuntimeStorage;
use super::PostgresStorage;
use crate::storage::{OrphanSweepResult, StorageLifecycle};
use anyhow::Result;
use std::collections::HashMap;

pub(super) trait PostgresRetentionStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>>;
    fn cleanup_orphan_session_logs_impl(&self, cutoff_epoch_s: f64) -> Result<OrphanSweepResult>;
}

impl PostgresRetentionStorage for PostgresStorage {
    fn cleanup_retention_impl(&self, cutoff_epoch_s: f64) -> Result<HashMap<String, i64>> {
        let mut deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 || !cutoff_epoch_s.is_finite() {
            return Ok(deleted);
        }
        self.ensure_initialized()?;
        let count = self.delete_stream_events_before_impl(cutoff_epoch_s)?;
        if count > 0 {
            deleted.insert("stream_events".into(), count);
        }
        Ok(deleted)
    }

    fn cleanup_orphan_session_logs_impl(&self, cutoff_epoch_s: f64) -> Result<OrphanSweepResult> {
        let mut deleted = HashMap::new();
        if cutoff_epoch_s <= 0.0 || !cutoff_epoch_s.is_finite() {
            return Ok(OrphanSweepResult {
                deleted,
                completed: true,
                swept_until: cutoff_epoch_s,
                deferred: false,
            });
        }
        self.ensure_initialized()?;
        let logs = self.runtime_logs()?;
        let candidates = {
            let conn = logs.connection();
            let mut statement=conn.prepare("SELECT DISTINCT session_id FROM runtime_tool_metrics WHERE created_time<?1 UNION SELECT DISTINCT session_id FROM runtime_artifact_resources WHERE created_time<?1")?;
            let rows = statement.query_map([cutoff_epoch_s], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        // RuntimeLogs has a fixed global row bound; identity transfer stays
        // bounded and PostgreSQL never scans or removes canonical history.
        let mut conn = self.conn()?;
        let mut live = std::collections::HashSet::new();
        for batch in candidates.chunks(100) {
            let rows=conn.query("SELECT session_id FROM chat_sessions WHERE session_id=ANY($1) UNION SELECT session_id FROM deleted_session_log_grace WHERE session_id=ANY($1)", &[&batch])?;
            live.extend(rows.into_iter().map(|r| r.get::<_, String>(0)));
        }
        let mut conn = logs.connection();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for session in candidates.into_iter().filter(|s| !live.contains(s)) {
            for table in ["runtime_tool_metrics", "runtime_artifact_resources"] {
                let count = tx.execute(
                    &format!("DELETE FROM {table} WHERE session_id=?1 AND created_time<?2"),
                    rusqlite::params![session, cutoff_epoch_s],
                )? as i64;
                if count > 0 {
                    *deleted.entry(table.to_owned()).or_insert(0) += count;
                }
            }
        }
        tx.commit()?;
        Ok(OrphanSweepResult {
            deleted,
            completed: true,
            swept_until: cutoff_epoch_s,
            deferred: false,
        })
    }
}
