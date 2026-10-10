use super::SqliteStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::Value;

impl SqliteStorage {
    pub(super) fn ensure_queue_control_schema(&self, conn: &Connection) -> Result<()> {
        for (table, column, ddl) in [
            (
                "agent_tasks",
                "priority",
                "ALTER TABLE agent_tasks ADD COLUMN priority INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "session_locks",
                "suspended",
                "ALTER TABLE session_locks ADD COLUMN suspended INTEGER NOT NULL DEFAULT 0",
            ),
        ] {
            let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if !columns.iter().any(|name| name == column) {
                conn.execute(ddl, [])?;
            }
        }
        conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_agent_tasks_dispatch ON agent_tasks(status, priority DESC, retry_at, created_at, task_id)")?;
        Ok(())
    }

    pub(super) fn claim_agent_task_impl(&self, task_id: &str, now: f64) -> Result<bool> {
        self.ensure_initialized()?;
        Ok(self.open()?.execute(
            "UPDATE agent_tasks SET status='running', started_at=?, updated_at=?, finished_at=NULL, last_error=NULL WHERE task_id=? AND status IN ('pending','retry') AND retry_at<=?",
            params![now, now, task_id, now],
        )? == 1)
    }

    pub(super) fn promote_agent_task_impl(&self, task_id: &str, now: f64) -> Result<bool> {
        self.ensure_initialized()?;
        Ok(self.open()?.execute(
            "UPDATE agent_tasks SET priority=1, retry_at=MIN(retry_at, ?), updated_at=?, request_payload=json_set(request_payload, '$.queue_priority', 1) WHERE task_id=? AND status IN ('pending','retry')",
            params![now, now, task_id],
        )? == 1)
    }

    pub(super) fn reorder_agent_tasks_impl(
        &self,
        task_ids: &[String],
        now: f64,
    ) -> Result<usize> {
        self.ensure_initialized()?;
        if task_ids.is_empty() {
            return Ok(0);
        }
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // retry_at 是派发序里排在 created_at 之前的位次键：越靠前越小，且必须不晚于 now 才可领取。
        let total = task_ids.len();
        let mut changed = 0usize;
        for (index, task_id) in task_ids.iter().enumerate() {
            let rank_at = now - (total - index) as f64;
            changed += tx.execute(
                "UPDATE agent_tasks SET priority=0, retry_at=?, updated_at=?, request_payload=json_set(request_payload, '$.queue_priority', 0, '$.queue_rank', ?) WHERE task_id=? AND status IN ('pending','retry')",
                params![rank_at, now, index as i64, task_id],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    pub(super) fn update_agent_task_queue_payload_impl(
        &self,
        task_id: &str,
        payload: &Value,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        // Only queue metadata is patched; concurrent cancellation/promotion owns task state.
        Ok(self.open()?.execute(
            "UPDATE agent_tasks SET request_payload=json_patch(request_payload, ?) WHERE task_id=? AND status IN ('pending','retry')",
            params![serde_json::to_string(payload)?, task_id],
        )? == 1)
    }

    pub(super) fn set_session_lock_suspended_impl(
        &self,
        session_id: &str,
        suspended: bool,
        max_active: i64,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Self::now_ts();
        let changed = tx.execute(
            "UPDATE session_locks SET suspended=? WHERE session_id=? AND expires_at>? AND (suspended=? OR ?=1 OR (SELECT COUNT(*) FROM session_locks WHERE suspended=0 AND expires_at>?)<?)",
            params![i64::from(suspended), session_id, now, i64::from(suspended), i64::from(suspended), now, max_active.max(1)],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }
}
