use super::PostgresStorage;
use crate::storage::{SessionLockRecord, SessionLockStatus, StorageBackend};
use anyhow::Result;

pub(super) trait PostgresSessionLockStorage {
    fn try_acquire_session_lock_impl(
        &self,
        session_id: &str,
        user_id: &str,
        agent_id: &str,
        ttl_s: f64,
        max_sessions: i64,
    ) -> Result<SessionLockStatus>;
    fn touch_session_lock_impl(&self, session_id: &str, ttl_s: f64) -> Result<()>;
    fn release_session_lock_impl(&self, session_id: &str) -> Result<()>;
    fn delete_session_locks_by_user_impl(&self, user_id: &str) -> Result<i64>;
    fn count_session_locks_impl(&self) -> Result<i64>;
    fn list_session_locks_by_user_impl(&self, user_id: &str) -> Result<Vec<SessionLockRecord>>;
}

impl PostgresSessionLockStorage for PostgresStorage {
    fn try_acquire_session_lock_impl(
        &self,
        _session_id: &str,
        _user_id: &str,
        _agent_id: &str,
        _ttl_s: f64,
        _max_sessions: i64,
    ) -> Result<SessionLockStatus> {
        self.ensure_initialized()?;
        let cleaned_session = _session_id.trim();
        let cleaned_user = _user_id.trim();
        let cleaned_agent = _agent_id.trim();
        if cleaned_session.is_empty() || cleaned_user.is_empty() {
            return Ok(SessionLockStatus::SystemBusy);
        }
        let max_sessions = _max_sessions.max(1);
        let ttl_s = _ttl_s.max(1.0);
        let now = Self::now_ts();
        let expires_at = now + ttl_s;

        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("DELETE FROM session_locks WHERE expires_at <= $1", &[&now])?;
        let inserted = tx.execute(
            "INSERT INTO session_locks (session_id, user_id, agent_id, created_time, updated_time, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT DO NOTHING",
            &[
                &cleaned_session,
                &cleaned_user,
                &cleaned_agent,
                &now,
                &now,
                &expires_at,
            ],
        )?;
        if inserted == 0 {
            let session_lock = tx.query_opt(
                "SELECT session_id FROM session_locks WHERE session_id = $1 LIMIT 1",
                &[&cleaned_session],
            )?;
            tx.commit()?;
            return Ok(if session_lock.is_some() {
                SessionLockStatus::UserBusy
            } else {
                SessionLockStatus::SystemBusy
            });
        }
        let total: i64 = tx
            .query_one("SELECT COUNT(*) FROM session_locks", &[])?
            .get(0);
        if total > max_sessions {
            tx.execute(
                "DELETE FROM session_locks WHERE session_id = $1",
                &[&cleaned_session],
            )?;
            tx.commit()?;
            return Ok(SessionLockStatus::SystemBusy);
        }
        tx.commit()?;
        Ok(SessionLockStatus::Acquired)
    }

    fn touch_session_lock_impl(&self, _session_id: &str, _ttl_s: f64) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = _session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(());
        }
        let ttl_s = _ttl_s.max(1.0);
        let now = Self::now_ts();
        let expires_at = now + ttl_s;
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE session_locks SET updated_time = $1, expires_at = $2 WHERE session_id = $3",
            &[&now, &expires_at, &cleaned_session],
        )?;
        Ok(())
    }

    fn release_session_lock_impl(&self, _session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = _session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "DELETE FROM session_locks WHERE session_id = $1",
            &[&cleaned_session],
        )?;
        Ok(())
    }

    fn delete_session_locks_by_user_impl(&self, _user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = _user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM session_locks WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn count_session_locks_impl(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let total = conn
            .query_one(
                "SELECT COUNT(*) FROM session_locks WHERE expires_at > $1",
                &[&now],
            )?
            .get(0);
        Ok(total)
    }

    fn list_session_locks_by_user_impl(&self, user_id: &str) -> Result<Vec<SessionLockRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT session_id, user_id, agent_id, updated_time, expires_at \
             FROM session_locks WHERE user_id = $1 AND expires_at > $2",
            &[&cleaned, &now],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(SessionLockRecord {
                session_id: row.get(0),
                user_id: row.get(1),
                agent_id: row.get(2),
                updated_time: row.get(3),
                expires_at: row.get(4),
            });
        }
        Ok(output)
    }
}
