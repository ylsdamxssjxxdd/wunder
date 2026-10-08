use super::SqliteStorage;
use crate::storage::{AgentTaskRecord, StorageLifecycle};
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::Value;

impl SqliteStorage {
    pub(super) fn insert_agent_message_task_impl(
        &self,
        record: &AgentTaskRecord,
        limit: i64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let mut slot = self.agent_message_connection.lock();
        if slot.is_none() {
            *slot = Some(self.open()?);
        }
        let connection = slot.as_mut().expect("initialized message connection");
        // IMMEDIATE protects both the capacity check and insert from competing writers.
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx.query_row("SELECT request_payload FROM agent_tasks WHERE task_id=? AND user_id=? AND session_id=?",
            params![record.task_id,record.user_id,record.session_id], |row| row.get(0)).optional()?;
        if let Some(existing) = existing {
            let existing: Value = serde_json::from_str(&existing)?;
            if existing.get("question") != record.request_payload.get("question") {
                bail!("message_id conflict");
            }
            return Ok(());
        }
        let count:i64 = tx.query_row("SELECT COUNT(*) FROM agent_tasks WHERE thread_id=? AND status IN ('pending','retry','running')",
            [&record.thread_id], |row| row.get(0))?;
        if count >= limit {
            bail!("parent message queue is full");
        }
        tx.execute("INSERT INTO agent_tasks(task_id,thread_id,user_id,agent_id,session_id,status,request_payload,retry_count,retry_at,created_at,updated_at) VALUES (?,?,?,?,?,'pending',?,0,?,?,?)",
            params![record.task_id,record.thread_id,record.user_id,record.agent_id,record.session_id,
                serde_json::to_string(&record.request_payload)?,record.retry_at,record.created_at,record.updated_at])?;
        tx.commit()?;
        Ok(())
    }
}
