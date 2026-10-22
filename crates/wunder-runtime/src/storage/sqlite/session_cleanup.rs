use super::SqliteStorage;
use crate::storage::StorageLifecycle;
use anyhow::Result;

impl SqliteStorage {
    pub(super) fn delete_chat_sessions_by_user_impl(&self, user_id: &str) -> Result<i64> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Ok(0);
        }
        self.ensure_initialized()?;
        // Goals are durable runtime state in their own database. It is safe
        // to issue this before state deletion because user deletion is
        // idempotent and never depends on an all-database transaction.
        let goals = self.open_goals()?;
        goals.execute("DELETE FROM session_goals WHERE user_id = ?", [user_id])?;
        let mut conn = self.open()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM cron_jobs WHERE user_id = ?", [user_id])?;
        let count = tx.execute("DELETE FROM chat_sessions WHERE user_id = ?", [user_id])?;
        tx.commit()?;
        Ok(count as i64)
    }
}
