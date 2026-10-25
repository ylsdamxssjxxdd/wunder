//! Small per-store connection cache. Leases never hold the cache mutex while
//! executing SQL; transactions and thread execution fences keep their semantics.
use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

const MAX_IDLE: usize = 2;

#[derive(Clone, Debug)]
pub(super) struct SqlitePool(Arc<Pool>);

#[derive(Debug)]
struct Pool {
    path: PathBuf,
    idle: Mutex<Vec<Connection>>,
}

pub(super) struct ConnectionLease {
    connection: Option<Connection>,
    pool: Option<Arc<Pool>>,
}

impl SqlitePool {
    pub(super) fn new(path: impl Into<PathBuf>) -> Self {
        Self(Arc::new(Pool {
            path: path.into(),
            idle: Mutex::new(Vec::new()),
        }))
    }

    pub(super) fn get(&self) -> Result<ConnectionLease> {
        let cached = self.0.idle.lock().pop();
        let connection = match cached {
            Some(connection) => connection,
            None => {
                let connection = Connection::open(&self.0.path)?;
                connection.busy_timeout(Duration::from_secs(2))?;
                connection.execute_batch(
                    "PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL; PRAGMA cache_size=-1024;",
                )?;
                connection
            }
        };
        Ok(ConnectionLease {
            connection: Some(connection),
            pool: Some(self.0.clone()),
        })
    }
}

impl ConnectionLease {
    pub(super) fn uncached(connection: Connection) -> Self {
        Self {
            connection: Some(connection),
            pool: None,
        }
    }
}

impl Deref for ConnectionLease {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.connection.as_ref().unwrap()
    }
}
impl DerefMut for ConnectionLease {
    fn deref_mut(&mut self) -> &mut Connection {
        self.connection.as_mut().unwrap()
    }
}
impl Drop for ConnectionLease {
    fn drop(&mut self) {
        let Some(pool) = &self.pool else { return };
        let Some(connection) = self.connection.take() else {
            return;
        };
        // Never lend an unfinished transaction (or a failed rollback) to the
        // next caller, including when a blocking operation unwinds.
        if !connection.is_autocommit() && connection.execute_batch("ROLLBACK").is_err() {
            return;
        }
        let mut idle = pool.idle.lock();
        if idle.len() < MAX_IDLE {
            idle.push(connection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reused_connections_rollback_and_observe_other_commits() {
        let root = tempfile::tempdir().unwrap();
        let pool = SqlitePool::new(root.path().join("fixture.sqlite"));
        let first = pool.get().unwrap();
        first.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE fixture(value INTEGER); BEGIN; INSERT INTO fixture VALUES(1);").unwrap();
        drop(first);
        let reused = pool.get().unwrap();
        assert!(reused.is_autocommit());
        assert_eq!(
            reused
                .query_row("SELECT count(*) FROM fixture", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        let writer = pool.get().unwrap();
        writer.execute("INSERT INTO fixture VALUES(2)", []).unwrap();
        assert_eq!(
            reused
                .query_row("SELECT value FROM fixture", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        drop(writer);
        drop(reused);
        let leases = (0..8).map(|_| pool.get().unwrap()).collect::<Vec<_>>();
        drop(leases);
        assert_eq!(pool.0.idle.lock().len(), MAX_IDLE);
    }
}
