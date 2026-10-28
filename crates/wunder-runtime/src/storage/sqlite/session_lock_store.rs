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
    fn touch_session_lock_impl(
        &self,
        session_id: &str,
        lock_token: &str,
        ttl_s: f64,
    ) -> Result<bool>;
    fn release_session_lock_impl(&self, session_id: &str, lock_token: &str) -> Result<()>;
    fn force_release_session_lock_impl(&self, session_id: &str) -> Result<()>;
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
        // 每次获得颁发一个不可预测的新代次，旧代次的心跳/释放随后全部失配。
        let lock_token = uuid::Uuid::new_v4().simple().to_string();
        let mut conn = self.open_queue()?;
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
            "INSERT INTO session_locks (session_id, user_id, agent_id, lock_token, created_time, updated_time, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                cleaned_session,
                cleaned_user,
                cleaned_agent,
                lock_token,
                now,
                now,
                expires_at
            ],
        );
        match insert {
            Ok(_) => {
                let total: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM session_locks WHERE suspended = 0",
                    [],
                    |row| row.get(0),
                )?;
                if total > max_sessions {
                    tx.execute(
                        "DELETE FROM session_locks WHERE session_id = ?",
                        params![cleaned_session],
                    )?;
                    tx.commit()?;
                    return Ok(SessionLockStatus::SystemBusy);
                }
                tx.commit()?;
                Ok(SessionLockStatus::Acquired { lock_token })
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

    fn touch_session_lock_impl(
        &self,
        session_id: &str,
        lock_token: &str,
        ttl_s: f64,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(false);
        }
        let ttl_s = ttl_s.max(1.0);
        let now = Self::now_ts();
        let expires_at = now + ttl_s;
        let conn = self.open_queue()?;
        // WHERE 携带代次：过期被接管后，旧持有者的迟到心跳不再续期新持有者的行。
        let changed = conn.execute(
            "UPDATE session_locks SET updated_time = ?, expires_at = ? WHERE session_id = ? AND lock_token = ?",
            params![now, expires_at, cleaned_session, lock_token],
        )?;
        Ok(changed > 0)
    }

    fn release_session_lock_impl(&self, session_id: &str, lock_token: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(());
        }
        let conn = self.open_queue()?;
        // WHERE 携带代次：旧持有者的迟到释放不会删除新持有者的行。
        conn.execute(
            "DELETE FROM session_locks WHERE session_id = ? AND lock_token = ?",
            params![cleaned_session, lock_token],
        )?;
        Ok(())
    }

    fn force_release_session_lock_impl(&self, session_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Ok(());
        }
        let conn = self.open_queue()?;
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
        let conn = self.open_queue()?;
        let affected = conn.execute(
            "DELETE FROM session_locks WHERE user_id = ?",
            params![cleaned_user],
        )?;
        Ok(affected as i64)
    }

    fn count_session_locks_impl(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let conn = self.open_queue()?;
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
        let conn = self.open_queue()?;
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
    fn task_threads_of_one_agent_are_independent() {
        let (storage, _dir) = build_storage();
        let mut tokens = std::collections::HashMap::new();
        for id in ["task-a", "task-b"] {
            match storage
                .try_acquire_session_lock(id, "user-a", "agent-a", 60.0, 8)
                .unwrap()
            {
                SessionLockStatus::Acquired { lock_token } => {
                    tokens.insert(id, lock_token);
                }
                other => panic!("expected acquired, got {other:?}"),
            }
        }
        assert_eq!(
            storage
                .try_acquire_session_lock("task-a", "user-a", "agent-a", 60.0, 8)
                .unwrap(),
            SessionLockStatus::UserBusy
        );
        storage
            .release_session_lock("task-a", &tokens["task-a"])
            .unwrap();
        let remaining = storage.list_session_locks_by_user("user-a").unwrap();
        assert_eq!(
            remaining
                .iter()
                .map(|lock| lock.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["task-b"]
        );
    }

    #[test]
    fn session_lock_roundtrip_enforces_duplicate_and_limit() {
        let (storage, _dir) = build_storage();

        let token_a = match storage
            .try_acquire_session_lock("session-a", "user-a", "agent-a", 30.0, 2)
            .expect("acquire first lock")
        {
            SessionLockStatus::Acquired { lock_token } => lock_token,
            other => panic!("expected acquired, got {other:?}"),
        };
        assert_eq!(
            storage
                .try_acquire_session_lock("session-a", "user-a", "agent-a", 30.0, 2)
                .expect("acquire duplicate lock"),
            SessionLockStatus::UserBusy
        );
        assert!(storage
            .touch_session_lock("session-a", &token_a, 60.0)
            .expect("touch lock"));

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
            .release_session_lock("session-a", &token_a)
            .expect("release lock");
        assert_eq!(storage.count_session_locks().expect("count released"), 0);

        assert!(matches!(
            storage
                .try_acquire_session_lock("session-c", "user-a", "agent-c", 30.0, 2)
                .expect("acquire after release"),
            SessionLockStatus::Acquired { .. }
        ));
        assert_eq!(
            storage
                .delete_session_locks_by_user("user-a")
                .expect("delete user locks"),
            1
        );
    }

    /// FV-01 核心交错：持有者心跳停顿到锁过期，接管者获得新代次；旧持有者恢复后的
    /// 迟到释放与迟到心跳不得删除或续期新持有者的锁行。两个 storage 实例共享同一
    /// 数据库文件，等价于两个本地进程。
    #[test]
    fn stale_holder_late_release_and_touch_cannot_reach_the_new_generation() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("shared.db");
        let path_str = path.to_string_lossy().to_string();
        let first = SqliteStorage::new(path_str.clone());
        let second = SqliteStorage::new(path_str.clone());
        let token_a = match first
            .try_acquire_session_lock("thread", "user", "agent", 30.0, 8)
            .expect("first acquire")
        {
            SessionLockStatus::Acquired { lock_token } => lock_token,
            other => panic!("expected acquired, got {other:?}"),
        };
        // 强制旧持有者的行过期，等价于心跳停顿超过 TTL。session_locks 在 Queue 角色
        // 库（与主库同目录的 queue_1.sqlite）中。
        let queue_path = dir.path().join("queue_1.sqlite");
        let conn = rusqlite::Connection::open(&queue_path).expect("open raw connection");
        conn.execute("UPDATE session_locks SET expires_at = 0", [])
            .expect("force expiry");
        drop(conn);

        // 接管者获得全新代次。
        let token_b = match second
            .try_acquire_session_lock("thread", "user", "agent", 30.0, 8)
            .expect("takeover acquire")
        {
            SessionLockStatus::Acquired { lock_token } => lock_token,
            other => panic!("expected acquired, got {other:?}"),
        };
        assert_ne!(token_a, token_b, "takeover must mint a fresh token");
        let expiry_b = second.list_session_locks_by_user("user").unwrap()[0].expires_at;

        // 旧持有者恢复后的迟到心跳：失配，且不得续期新持有者的行。
        assert!(!first
            .touch_session_lock("thread", &token_a, 60.0)
            .expect("stale touch"));
        let locks = second.list_session_locks_by_user("user").unwrap();
        assert_eq!(locks.len(), 1);
        assert_eq!(
            locks[0].expires_at, expiry_b,
            "stale touch must not extend the new generation"
        );

        // 迟到释放：不得删除新持有者的行。
        first
            .release_session_lock("thread", &token_a)
            .expect("stale release");
        assert_eq!(second.list_session_locks_by_user("user").unwrap().len(), 1);

        // 新持有者自己的心跳与释放仍然生效。
        assert!(second
            .touch_session_lock("thread", &token_b, 45.0)
            .expect("live touch"));
        second
            .release_session_lock("thread", &token_b)
            .expect("live release");
        assert_eq!(second.count_session_locks().unwrap(), 0);
    }
}
