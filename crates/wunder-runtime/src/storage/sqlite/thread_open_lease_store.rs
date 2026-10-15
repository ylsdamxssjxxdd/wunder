use super::SqliteStorage;
use crate::storage::{StorageLifecycle, ThreadOpenLeaseRecord, ThreadOpenLeaseStatus};
use anyhow::Result;
use rusqlite::{params, ErrorCode, OptionalExtension, TransactionBehavior};

pub(super) trait SqliteThreadOpenLeaseStorage {
    fn try_acquire_thread_open_lease_impl(
        &self,
        session_id: &str,
        user_id: &str,
        owner_instance_id: &str,
        client_kind: &str,
        ttl_s: f64,
    ) -> Result<ThreadOpenLeaseStatus>;
    fn touch_thread_open_lease_impl(
        &self,
        session_id: &str,
        owner_instance_id: &str,
        ttl_s: f64,
    ) -> Result<bool>;
    fn release_thread_open_lease_impl(
        &self,
        session_id: &str,
        owner_instance_id: &str,
    ) -> Result<bool>;
    fn get_thread_open_lease_impl(
        &self,
        session_id: &str,
        user_id: &str,
    ) -> Result<Option<ThreadOpenLeaseRecord>>;
}

impl SqliteThreadOpenLeaseStorage for SqliteStorage {
    fn try_acquire_thread_open_lease_impl(
        &self,
        session_id: &str,
        user_id: &str,
        owner_instance_id: &str,
        client_kind: &str,
        ttl_s: f64,
    ) -> Result<ThreadOpenLeaseStatus> {
        self.ensure_initialized()?;
        let session_id = session_id.trim();
        let user_id = user_id.trim();
        let owner_instance_id = owner_instance_id.trim();
        let client_kind = client_kind.trim();
        if session_id.is_empty() || user_id.is_empty() || owner_instance_id.is_empty() {
            return Ok(ThreadOpenLeaseStatus::Busy);
        }
        let now = Self::now_ts();
        let expires_at = now + ttl_s.max(1.0);
        let mut conn = self.open()?;
        let tx = match conn.transaction_with_behavior(TransactionBehavior::Immediate) {
            Ok(tx) => tx,
            Err(rusqlite::Error::SqliteFailure(err, _))
                if matches!(
                    err.code,
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                ) =>
            {
                return Ok(ThreadOpenLeaseStatus::Busy)
            }
            Err(err) => return Err(err.into()),
        };
        tx.execute(
            "DELETE FROM thread_open_leases WHERE expires_at <= ?",
            params![now],
        )?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT owner_instance_id FROM thread_open_leases WHERE session_id = ?",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(owner) = existing {
            if owner != owner_instance_id {
                tx.commit()?;
                return Ok(ThreadOpenLeaseStatus::Busy);
            }
            tx.execute(
                "UPDATE thread_open_leases SET updated_time = ?, expires_at = ?, client_kind = ? WHERE session_id = ? AND owner_instance_id = ?",
                params![now, expires_at, client_kind, session_id, owner_instance_id],
            )?;
            tx.commit()?;
            return Ok(ThreadOpenLeaseStatus::Acquired);
        }
        // A running or queued turn owns the execution lock or a durable queue
        // row. Keep the thread unavailable to a new UI even after the previous
        // window disappears; both are durable sources of truth for work that
        // must not be opened concurrently.
        let running: i64 = tx.query_row(
            "SELECT CASE WHEN EXISTS (SELECT 1 FROM session_locks WHERE session_id = ? AND expires_at > ?)
             OR EXISTS (SELECT 1 FROM agent_tasks WHERE session_id = ? AND status IN ('pending','retry','running'))
             OR EXISTS (SELECT 1 FROM thread_turns WHERE session_id = ? AND status IN ('accepted','queued','running','waiting_input','waiting_approval'))
             THEN 1 ELSE 0 END",
            params![session_id, now, session_id, session_id],
            |row| row.get(0),
        )?;
        if running > 0 {
            tx.commit()?;
            return Ok(ThreadOpenLeaseStatus::Busy);
        }
        tx.execute(
            "INSERT INTO thread_open_leases (session_id, user_id, owner_instance_id, client_kind, updated_time, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![session_id, user_id, owner_instance_id, client_kind, now, expires_at],
        )?;
        tx.commit()?;
        Ok(ThreadOpenLeaseStatus::Acquired)
    }

    fn touch_thread_open_lease_impl(
        &self,
        session_id: &str,
        owner_instance_id: &str,
        ttl_s: f64,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let changed = self.open()?.execute(
            "UPDATE thread_open_leases SET updated_time = ?, expires_at = ? WHERE session_id = ? AND owner_instance_id = ? AND expires_at > ?",
            params![now, now + ttl_s.max(1.0), session_id.trim(), owner_instance_id.trim(), now],
        )?;
        Ok(changed > 0)
    }

    fn release_thread_open_lease_impl(
        &self,
        session_id: &str,
        owner_instance_id: &str,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let changed = self.open()?.execute(
            "DELETE FROM thread_open_leases WHERE session_id = ? AND owner_instance_id = ?",
            params![session_id.trim(), owner_instance_id.trim()],
        )?;
        Ok(changed > 0)
    }

    fn get_thread_open_lease_impl(
        &self,
        session_id: &str,
        user_id: &str,
    ) -> Result<Option<ThreadOpenLeaseRecord>> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let conn = self.open()?;
        conn.query_row(
            "SELECT session_id, user_id, owner_instance_id, client_kind, updated_time, expires_at FROM thread_open_leases WHERE session_id = ? AND user_id = ? AND expires_at > ?",
            params![session_id.trim(), user_id.trim(), now],
            |row| Ok(ThreadOpenLeaseRecord {
                session_id: row.get(0)?,
                user_id: row.get(1)?,
                owner_instance_id: row.get(2)?,
                client_kind: row.get(3)?,
                updated_time: row.get(4)?,
                expires_at: row.get(5)?,
            }),
        ).optional().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{SessionLockStore, ThreadOpenLeaseStore};
    use tempfile::tempdir;

    #[test]
    fn lease_is_exclusive_across_storage_connections() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("shared.db");
        let first = SqliteStorage::new(path.to_string_lossy().to_string());
        let second = SqliteStorage::new(path.to_string_lossy().to_string());
        assert_eq!(
            first
                .try_acquire_thread_open_lease("thread", "user", "instance-a", "desktop", 30.0)
                .expect("first lease"),
            ThreadOpenLeaseStatus::Acquired
        );
        assert_eq!(
            second
                .try_acquire_thread_open_lease("thread", "user", "instance-b", "cli", 30.0)
                .expect("second lease"),
            ThreadOpenLeaseStatus::Busy
        );
        assert!(!second
            .release_thread_open_lease("thread", "instance-b")
            .expect("wrong release"));
    }

    #[test]
    fn execution_lock_blocks_new_open_lease() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("running.db");
        let storage = SqliteStorage::new(path.to_string_lossy().to_string());
        assert_eq!(
            storage
                .try_acquire_session_lock("thread", "user", "agent", 30.0, 8)
                .expect("execution lock"),
            crate::storage::SessionLockStatus::Acquired
        );
        assert_eq!(
            storage
                .try_acquire_thread_open_lease("thread", "user", "instance", "desktop", 30.0)
                .expect("open lease"),
            ThreadOpenLeaseStatus::Busy
        );
    }
}
