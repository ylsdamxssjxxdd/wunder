use super::{PgConn, PostgresStorage};
use crate::storage::StorageLifecycle;
use anyhow::Result;
use serde_json::Value;

impl PostgresStorage {
    pub(super) fn park_queued_admission(
        &self,
        record: &crate::storage::AgentTaskRecord,
    ) -> Result<()> {
        if record.request_payload.get("__queue_wait").is_none() {
            return Ok(());
        }
        let Some(turn) = record
            .request_payload
            .pointer("/config_overrides/__thread_log_turn_id")
            .and_then(Value::as_str)
        else {
            return Ok(());
        };
        // Queue transitions are part of the service control plane.  Calling
        // `LocalThreadStore::admission_store` here used to lazily create
        // queue_1.sqlite even when the server was configured for PostgreSQL.
        // Keep the transition idempotent and entirely in PostgreSQL instead.
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO service_thread_owners(thread_id,user_id,latest_user_round)
             VALUES($1,$2,0)
             ON CONFLICT(thread_id) DO UPDATE SET user_id=EXCLUDED.user_id
             WHERE service_thread_owners.user_id=EXCLUDED.user_id",
            &[&record.session_id, &record.user_id],
        )?;
        let owner = tx
            .query_opt(
                "SELECT user_id FROM service_thread_owners WHERE thread_id=$1",
                &[&record.session_id],
            )?
            .map(|row| row.get::<_, String>(0));
        if owner.as_deref() != Some(record.user_id.as_str()) {
            anyhow::bail!("thread owner mismatch");
        }
        tx.execute(
            "UPDATE service_thread_admissions SET status='queued',updated_at_ms=$3
             WHERE thread_id=$1 AND turn_id=$2 AND status='accepted'",
            &[
                &record.session_id,
                &turn,
                &chrono::Utc::now().timestamp_millis(),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(super) fn ensure_queue_control_schema(&self, conn: &mut PgConn<'_>) -> Result<()> {
        conn.batch_execute("CREATE TABLE IF NOT EXISTS service_thread_owners (
                thread_id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                latest_user_round BIGINT NOT NULL DEFAULT 0,
                mutation_pending TEXT
            );
            CREATE INDEX IF NOT EXISTS service_thread_owners_user
              ON service_thread_owners(user_id, thread_id);
            CREATE TABLE IF NOT EXISTS service_thread_admissions (
                thread_id TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                root_turn_id TEXT NOT NULL,
                user_round BIGINT NOT NULL,
                client_message_id TEXT,
                status TEXT NOT NULL,
                input_json TEXT,
                control_json TEXT,
                created_at_ms BIGINT NOT NULL,
                updated_at_ms BIGINT NOT NULL,
                PRIMARY KEY(thread_id, turn_id),
                UNIQUE(thread_id, client_message_id)
            );
            CREATE INDEX IF NOT EXISTS service_thread_admission_status
              ON service_thread_admissions(thread_id, status, user_round);
            CREATE INDEX IF NOT EXISTS service_thread_admission_round
              ON service_thread_admissions(thread_id, user_round);
            ALTER TABLE agent_tasks ADD COLUMN IF NOT EXISTS priority BIGINT NOT NULL DEFAULT 0;
            ALTER TABLE session_locks ADD COLUMN IF NOT EXISTS suspended BIGINT NOT NULL DEFAULT 0;
            CREATE INDEX IF NOT EXISTS idx_agent_tasks_session_dispatch ON agent_tasks(session_id, status, priority DESC, retry_at);
            CREATE INDEX IF NOT EXISTS idx_agent_tasks_dispatch ON agent_tasks(status, priority DESC, retry_at, created_at, task_id)")?;
        Ok(())
    }

    pub(super) fn claim_agent_task_impl(&self, task_id: &str, now: f64) -> Result<bool> {
        self.ensure_initialized()?;
        Ok(self.conn()?.execute(
            "UPDATE agent_tasks SET status='running', started_at=$1, updated_at=$1, finished_at=NULL, last_error=NULL WHERE task_id=$2 AND status IN ('pending','retry') AND retry_at<=$1 AND NOT EXISTS (SELECT 1 FROM agent_tasks ahead WHERE ahead.session_id=agent_tasks.session_id AND ahead.status IN ('pending','retry') AND ahead.retry_at<=$1 AND (ahead.priority>agent_tasks.priority OR (ahead.priority=agent_tasks.priority AND ahead.retry_at<agent_tasks.retry_at)))",
            &[&now, &task_id],
        )? == 1)
    }

    pub(super) fn promote_agent_task_impl(&self, task_id: &str, now: f64) -> Result<bool> {
        self.ensure_initialized()?;
        Ok(self.conn()?.execute(
            "UPDATE agent_tasks SET priority=1, retry_at=LEAST(retry_at, $1, COALESCE((SELECT MIN(a.retry_at)-1 FROM agent_tasks a WHERE a.session_id=agent_tasks.session_id AND a.status IN ('pending','retry') AND a.priority>0), retry_at)), updated_at=$1, request_payload=(request_payload::jsonb || '{\"queue_priority\":1}'::jsonb)::text WHERE task_id=$2 AND status IN ('pending','retry')",
            &[&now, &task_id],
        )? == 1)
    }

    pub(super) fn reorder_agent_tasks_impl(&self, task_ids: &[String], now: f64) -> Result<usize> {
        self.ensure_initialized()?;
        if task_ids.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        // retry_at 是派发序里排在 created_at 之前的位次键：越靠前越小，且必须不晚于 now 才可领取。
        let total = task_ids.len();
        let mut changed = 0usize;
        for (index, task_id) in task_ids.iter().enumerate() {
            let rank_at = now - (total - index) as f64;
            let patch = serde_json::json!({ "queue_priority": 0, "queue_rank": index });
            changed += tx.execute(
                "UPDATE agent_tasks SET priority=0, retry_at=$1, updated_at=$1, request_payload=(request_payload::jsonb || $2::text::jsonb)::text WHERE task_id=$3 AND status IN ('pending','retry')",
                &[&rank_at, &serde_json::to_string(&patch)?, task_id],
            )? as usize;
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
