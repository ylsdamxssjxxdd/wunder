//! Reusable readers own no write lock and never retain a read transaction.
use super::SqliteStorage;
use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use std::ops::{Deref, DerefMut};
use std::time::Duration;

// Two idle readers, each configured with a 1 MiB page cache on 32-bit too.
const IDLE_READERS: usize = 2;

pub(super) struct ReadConnection<'a> {
    storage: &'a SqliteStorage,
    connection: Option<Connection>,
}

impl Deref for ReadConnection<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        self.connection.as_ref().expect("checked-out reader")
    }
}

impl DerefMut for ReadConnection<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        self.connection.as_mut().expect("checked-out reader")
    }
}

impl Drop for ReadConnection<'_> {
    fn drop(&mut self) {
        let Some(conn) = self.connection.take() else {
            return;
        };
        // A presentation deadline belongs to one request, not the connection.
        conn.progress_handler(0, None::<fn() -> bool>);
        if !conn.is_autocommit() && conn.execute_batch("ROLLBACK").is_err() {
            return;
        }
        let mut idle = self.storage.read_connections.lock();
        if idle.len() < IDLE_READERS {
            idle.push(conn);
        }
    }
}

impl SqliteStorage {
    pub(super) fn read_connection(&self) -> Result<ReadConnection<'_>> {
        let cached = self.read_connections.lock().pop();
        let conn = match cached {
            Some(conn) => conn,
            None => {
                let conn = Connection::open_with_flags(
                    &self.db_path,
                    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )?;
                conn.pragma_update(None, "cache_size", -1024)?;
                conn.set_prepared_statement_cache_capacity(32);
                conn
            }
        };
        conn.busy_timeout(Duration::from_millis(250))?;
        Ok(ReadConnection {
            storage: self,
            connection: Some(conn),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MetaStore, StorageLifecycle};

    #[test]
    fn readers_reuse_pages_without_pinning_snapshots_or_waiting_for_writer() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        db.ensure_initialized().unwrap();
        db.set_meta("fixture-key", "before").unwrap();
        let mut writer = db.open().unwrap();
        let tx = writer
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        tx.execute("UPDATE meta SET value='after' WHERE key='fixture-key'", [])
            .unwrap();
        assert_eq!(
            db.get_meta("fixture-key").unwrap().as_deref(),
            Some("before")
        );
        tx.commit().unwrap();
        assert_eq!(
            db.get_meta("fixture-key").unwrap().as_deref(),
            Some("after")
        );
        let reader = db.read_connection().unwrap();
        assert!(reader.execute("DELETE FROM meta", []).is_err());
        let handle = unsafe { reader.handle() };
        // Returning an interrupted view must reset its request deadline.
        reader.progress_handler(1, Some(|| true));
        assert!(reader
            .query_row("SELECT count(*) FROM meta", [], |r| r.get::<_, i64>(0))
            .is_err());
        reader.progress_handler(0, None::<fn() -> bool>);
        reader.execute_batch("BEGIN; SELECT * FROM meta").unwrap();
        drop(reader);
        let reused = db.read_connection().unwrap();
        assert_eq!(unsafe { reused.handle() }, handle);
        assert!(reused.is_autocommit());
        assert!(reused
            .query_row("SELECT count(*) FROM meta", [], |r| r.get::<_, i64>(0))
            .is_ok());
        drop(reused);
        let readers = (0..6)
            .map(|_| db.read_connection().unwrap())
            .collect::<Vec<_>>();
        drop(readers);
        assert_eq!(db.read_connections.lock().len(), IDLE_READERS);
    }
}
