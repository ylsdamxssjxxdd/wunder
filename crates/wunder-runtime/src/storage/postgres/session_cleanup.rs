use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;

impl PostgresStorage {
    pub(super) fn delete_chat_sessions_by_user_impl(&self, user_id: &str) -> Result<i64> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Ok(0);
        }
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        for table in ["session_goals", "cron_jobs", "channel_sessions"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE user_id = $1"),
                &[&user_id],
            )?;
        }
        let count = tx.execute("DELETE FROM chat_sessions WHERE user_id = $1", &[&user_id])?;
        tx.commit()?;
        Ok(count as i64)
    }
}
