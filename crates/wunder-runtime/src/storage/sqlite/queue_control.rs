use super::SqliteStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;
use rusqlite::{params, TransactionBehavior};
use serde_json::Value;

impl SqliteStorage {
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
        self.canonical_threads()?.admission_store()?.park_queued(
            &record.user_id,
            &record.session_id,
            turn,
        )?;
        Ok(())
    }

    pub(super) fn claim_agent_task_impl(&self, task_id: &str, now: f64) -> Result<bool> {
        self.ensure_initialized()?;
        Ok(self.open_queue()?.execute(
            "UPDATE agent_tasks SET status='running', started_at=?, updated_at=?, finished_at=NULL, last_error=NULL WHERE task_id=? AND status IN ('pending','retry') AND retry_at<=? AND NOT EXISTS (SELECT 1 FROM agent_tasks ahead WHERE ahead.session_id=agent_tasks.session_id AND ahead.status IN ('pending','retry') AND ahead.retry_at<=? AND (ahead.priority>agent_tasks.priority OR (ahead.priority=agent_tasks.priority AND ahead.retry_at<agent_tasks.retry_at)))",
            params![now, now, task_id, now, now],
        )? == 1)
    }

    pub(super) fn promote_agent_task_impl(&self, task_id: &str, now: f64) -> Result<bool> {
        self.ensure_initialized()?;
        Ok(self.open_queue()?.execute(
            "UPDATE agent_tasks SET priority=1, retry_at=MIN(retry_at, ?, COALESCE((SELECT MIN(a.retry_at)-1 FROM agent_tasks a WHERE a.session_id=agent_tasks.session_id AND a.status IN ('pending','retry') AND a.priority>0), retry_at)), updated_at=?, request_payload=json_set(request_payload, '$.queue_priority', 1) WHERE task_id=? AND status IN ('pending','retry')",
            params![now, now, task_id],
        )? == 1)
    }

    pub(super) fn reorder_agent_tasks_impl(&self, task_ids: &[String], now: f64) -> Result<usize> {
        self.ensure_initialized()?;
        if task_ids.is_empty() {
            return Ok(0);
        }
        let mut conn = self.open_queue()?;
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
        Ok(self.open_queue()?.execute(
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
        let mut conn = self.open_queue()?;
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
