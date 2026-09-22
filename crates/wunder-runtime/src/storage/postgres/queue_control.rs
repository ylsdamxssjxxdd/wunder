use super::{PgConn, PostgresStorage};
use crate::storage::StorageLifecycle;
use anyhow::Result;
use serde_json::Value;

impl PostgresStorage {
    pub(super) fn ensure_queue_control_schema(&self, conn: &mut PgConn<'_>) -> Result<()> {
        conn.batch_execute("ALTER TABLE agent_tasks ADD COLUMN IF NOT EXISTS priority BIGINT NOT NULL DEFAULT 0;
            ALTER TABLE session_locks ADD COLUMN IF NOT EXISTS suspended BIGINT NOT NULL DEFAULT 0;
            CREATE INDEX IF NOT EXISTS idx_agent_tasks_dispatch ON agent_tasks(status, priority DESC, retry_at, created_at, task_id)")?;
        Ok(())
    }

    pub(super) fn claim_agent_task_impl(&self, task_id: &str, now: f64) -> Result<bool> {
        self.ensure_initialized()?;
        Ok(self.conn()?.execute(
            "UPDATE agent_tasks SET status='running', started_at=$1, updated_at=$1, finished_at=NULL, last_error=NULL WHERE task_id=$2 AND status IN ('pending','retry') AND retry_at<=$1",
            &[&now, &task_id],
        )? == 1)
    }

    pub(super) fn promote_agent_task_impl(&self, task_id: &str, now: f64) -> Result<bool> {
        self.ensure_initialized()?;
        Ok(self.conn()?.execute(
            "UPDATE agent_tasks SET priority=1, retry_at=LEAST(retry_at, $1), updated_at=$1, request_payload=(request_payload::jsonb || '{\"queue_priority\":1}'::jsonb)::text WHERE task_id=$2 AND status IN ('pending','retry')",
            &[&now, &task_id],
        )? == 1)
    }

    pub(super) fn update_agent_task_queue_payload_impl(
        &self,
        task_id: &str,
        payload: &Value,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        Ok(self.conn()?.execute(
            "UPDATE agent_tasks SET request_payload=(request_payload::jsonb || $1::text::jsonb)::text WHERE task_id=$2 AND status IN ('pending','retry')",
            &[&serde_json::to_string(payload)?, &task_id],
        )? == 1)
    }

    pub(super) fn set_session_lock_suspended_impl(
        &self,
        session_id: &str,
        suspended: bool,
        max_active: i64,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("LOCK TABLE session_locks IN SHARE ROW EXCLUSIVE MODE", &[])?;
        let changed = tx.execute(
            "UPDATE session_locks SET suspended=$1 WHERE session_id=$2 AND expires_at>$3 AND (suspended=$1 OR $1=1 OR (SELECT COUNT(*) FROM session_locks WHERE suspended=0 AND expires_at>$3)<$4)",
            &[&i64::from(suspended), &session_id, &Self::now_ts(), &max_active.max(1)],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }
}
