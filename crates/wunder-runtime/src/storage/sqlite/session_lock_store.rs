use super::SqliteStorage;
use crate::storage::{SessionLockRecord, SessionLockStatus, StorageLifecycle};
use anyhow::Result;
use rusqlite::{params, ErrorCode, TransactionBehavior};

pub(super) trait SqliteSessionLockStorage {
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

impl SqliteSessionLockStorage for SqliteStorage {
    fn try_acquire_session_lock_impl(
        &self,
        session_id: &str,
        user_id: &str,
        agent_id: &str,
        ttl_s: f64,
        max_sessions: i64,
    ) -> Result<SessionLockStatus> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        if cleaned_session.is_empty() || cleaned_user.is_empty() {
            return Ok(SessionLockStatus::SystemBusy);
        }
        let max_sessions = max_sessions.max(1);
        let ttl_s = ttl_s.max(1.0);
        let now = Self::now_ts();
        let expires_at = now + ttl_s;
        let mut conn = self.open()?;
        let tx = match conn.transaction_with_behavior(TransactionBehavior::Immediate) {
            Ok(tx) => tx,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if matches!(
                    err.code,
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                ) =>
            {
                return Ok(SessionLockStatus::SystemBusy)
            }
            Err(err) => return Err(err.into()),
        };
        tx.execute(
            "DELETE FROM session_locks WHERE expires_at <= ?",
            params![now],
        )?;
        let insert = tx.execute(
            "INSERT INTO session_locks (session_id, user_id, agent_id, created_time, updated_time, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                cleaned_session,
                cleaned_user,
                cleaned_agent,
                now,
                now,
                expires_at
            ],
        );
        match insert {
            Ok(_) => {
                let total: i64 =
                    tx.query_row("SELECT COUNT(*) FROM session_locks", [], |row| row.get(0))?;
                if total > max_sessions {
                    tx.execute(
                        "DELETE FROM session_locks WHERE session_id = ?",
                        params![cleaned_session],
                    )?;
                    tx.commit()?;
                    return Ok(SessionLockStatus::SystemBusy);
                }
                tx.commit()?;
                Ok(SessionLockStatus::Acquired)
            }
            Err(rusqlite::Error::SqliteFailure(err, _))
                if matches!(err.code, ErrorCode::ConstraintViolation) =>
            {
                tx.commit()?;
                Ok(SessionLockStatus::UserBusy)
            }
            Err(rusqlite::Error::SqliteFailure(err, _))
                if matches!(
                    err.code,
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                ) =>
            {
                tx.commit()?;
                Ok(SessionLockStatus::SystemBusy)
            }
            Err(err) => Err(err.into()),
        }
    }

    fn touch_session_lock_impl(&self, session_id: &str, ttl_s: f64) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(());
        }
        let ttl_s = ttl_s.max(1.0);
        let now = Self::now_ts();
        let expires_at = now + ttl_s;
        let conn = self.open()?;
        conn.execute(
            "UPDATE session_locks SET updated_time = ?, expires_at = ? WHERE session_id = ?",
            params![now, expires_at, cleaned_session],
        )?;
        Ok(())
    }

    fn release_session_lock_impl(&self, session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM session_locks WHERE session_id = ?",
            params![cleaned_session],
        )?;
        Ok(())
    }

    fn delete_session_locks_by_user_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM session_locks WHERE user_id = ?",
            params![cleaned_user],
        )?;
        Ok(affected as i64)
    }

    fn count_session_locks_impl(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let conn = self.open()?;
        let total = conn.query_row(
            "SELECT COUNT(*) FROM session_locks WHERE expires_at > ?",
            params![now],
            |row| row.get(0),
        )?;
        Ok(total)
    }

    fn list_session_locks_by_user_impl(&self, user_id: &str) -> Result<Vec<SessionLockRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let now = Self::now_ts();
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT session_id, user_id, agent_id, updated_time, expires_at \
             FROM session_locks WHERE user_id = ? AND expires_at > ?",
        )?;
        let rows = stmt
            .query_map(params![cleaned_user, now], |row| {
                Ok(SessionLockRecord {
                    session_id: row.get(0)?,
                    user_id: row.get(1)?,
                    agent_id: row.get(2)?,
                    updated_time: row.get(3)?,
                    expires_at: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<SessionLockRecord>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
    use tempfile::tempdir;

    fn build_storage() -> (SqliteStorage, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("session-lock-store.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize sqlite");
        (storage, dir)
    }

    #[test]
    fn session_lock_roundtrip_enforces_duplicate_and_limit() {
        let (storage, _dir) = build_storage();

        assert_eq!(
            storage
                .try_acquire_session_lock("session-a", "user-a", "agent-a", 30.0, 2)
                .expect("acquire first lock"),
            SessionLockStatus::Acquired
        );
        assert_eq!(
            storage
                .try_acquire_session_lock("session-a", "user-a", "agent-a", 30.0, 2)
                .expect("acquire duplicate lock"),
            SessionLockStatus::UserBusy
        );
        storage
            .touch_session_lock("session-a", 60.0)
            .expect("touch lock");

        let locks = storage
            .list_session_locks_by_user("user-a")
            .expect("list user locks");
        assert_eq!(locks.len(), 1);
        assert_eq!(locks[0].session_id, "session-a");
        assert_eq!(storage.count_session_locks().expect("count locks"), 1);

        assert_eq!(
            storage
                .try_acquire_session_lock("session-b", "user-a", "agent-b", 30.0, 1)
                .expect("acquire over limit"),
            SessionLockStatus::SystemBusy
        );
        assert_eq!(storage.count_session_locks().expect("count after limit"), 1);

        storage
            .release_session_lock("session-a")
            .expect("release lock");
        assert_eq!(storage.count_session_locks().expect("count released"), 0);

        assert_eq!(
            storage
                .try_acquire_session_lock("session-c", "user-a", "agent-c", 30.0, 2)
                .expect("acquire after release"),
            SessionLockStatus::Acquired
        );
        assert_eq!(
            storage
                .delete_session_locks_by_user("user-a")
                .expect("delete user locks"),
            1
        );
    }
}
