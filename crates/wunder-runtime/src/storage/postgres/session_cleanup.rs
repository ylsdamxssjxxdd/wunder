use super::{PgTx, PostgresStorage};
use crate::storage::{session_cleanup::*, StorageLifecycle};
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
        for table in CATALOG_DEPENDENT_TABLES.iter().chain(["cron_jobs"].iter()) {
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

pub(super) fn remove_empty_catalog(
    tx: &mut PgTx<'_>,
    start: f64,
    end: f64,
    now: f64,
) -> Result<i64> {
    let live = LIVE_SESSION_PREDICATE.replace(":now", "$3");
    tx.execute(&format!(
        "CREATE TEMP TABLE cleared_chat_sessions ON COMMIT DROP AS SELECT c.session_id, c.user_id FROM chat_sessions c \
         WHERE c.last_message_at >= $1 AND c.updated_at <= $2 AND ({EMPTY_HISTORY_PREDICATE}) AND NOT ({live})"
    ), &[&start, &end, &now])?;
    for table in CATALOG_DEPENDENT_TABLES {
        tx.execute(&format!("DELETE FROM {table} WHERE session_id IN (SELECT session_id FROM cleared_chat_sessions)"), &[])?;
    }
    Ok(tx.execute("DELETE FROM chat_sessions WHERE session_id IN (SELECT session_id FROM cleared_chat_sessions)", &[])? as i64)
}
