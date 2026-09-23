use super::PostgresStorage;
use crate::storage::{AgentTaskRecord, StorageLifecycle};
use anyhow::{bail, Result};
use serde_json::Value;

impl PostgresStorage {
    pub(super) fn insert_agent_message_task_impl(
        &self,
        record: &AgentTaskRecord,
        limit: i64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let mut connection = self.conn()?;
        let mut tx = connection.transaction()?;
        // Serialize only this thread across server processes; no global queue lock.
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
            &[&record.thread_id],
        )?;
        if let Some(row) = tx.query_opt("SELECT request_payload FROM agent_tasks WHERE task_id=$1 AND user_id=$2 AND session_id=$3",
            &[&record.task_id,&record.user_id,&record.session_id])? {
            let existing: Value = serde_json::from_str(row.get::<_, &str>(0))?;
            if existing.get("question") != record.request_payload.get("question") { bail!("message_id conflict"); }
            return Ok(());
        }
        let count:i64 = tx.query_one("SELECT COUNT(*) FROM agent_tasks WHERE thread_id=$1 AND status IN ('pending','retry','running')",
            &[&record.thread_id])?.get(0);
        if count >= limit {
            bail!("parent message queue is full");
        }
        tx.execute("INSERT INTO agent_tasks(task_id,thread_id,user_id,agent_id,session_id,status,request_payload,retry_count,retry_at,created_at,updated_at) VALUES ($1,$2,$3,$4,$5,'pending',$6,0,$7,$8,$9)",
            &[&record.task_id,&record.thread_id,&record.user_id,&record.agent_id,&record.session_id,
                &serde_json::to_string(&record.request_payload)?,&record.retry_at,&record.created_at,&record.updated_at])?;
        tx.commit()?;
        Ok(())
    }
}
