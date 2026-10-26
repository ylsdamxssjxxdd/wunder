use super::{runtime_state::DatabaseRole, SqliteStorage};
use anyhow::Result;
use std::sync::atomic::Ordering;

pub(super) trait SqliteSchemaStorage {
    fn ensure_initialized_impl(&self) -> Result<()>;
}

impl SqliteSchemaStorage for SqliteStorage {
    fn ensure_initialized_impl(&self) -> Result<()> {
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        let _guard = self.init_guard.lock();
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut conn = self.open()?;
        let schema = match self.database_role {
            DatabaseRole::State => include_str!("schema_state.sql"),
            DatabaseRole::Queue => include_str!("schema_queue.sql"),
            DatabaseRole::Goals => include_str!("schema_goals.sql"),
            DatabaseRole::Memories => include_str!("schema_memories.sql"),
            DatabaseRole::Logs => include_str!("schema_logs.sql"),
        };
        let tx = conn.transaction()?;
        tx.execute_batch(schema)?;
        if matches!(self.database_role, DatabaseRole::Queue) {
            // 存量 queue 库的列补建：CREATE TABLE IF NOT EXISTS 不会为旧表加列。
            ensure_session_locks_column(&tx, "lock_token", "TEXT NOT NULL DEFAULT ''")?;
            ensure_session_locks_column(&tx, "suspended", "INTEGER NOT NULL DEFAULT 0")?;
        }
        tx.commit()?;
        *self.wal_anchor.lock() = Some(conn);
        self.initialized.store(true, Ordering::Release);
        Ok(())
    }
}

/// 为存量库补齐列；PRAGMA table_info 判定后幂等 ALTER，重复执行安全。
fn ensure_session_locks_column(
    tx: &rusqlite::Transaction<'_>,
    column: &str,
    ddl: &str,
) -> Result<()> {
    let mut statement = tx.prepare("PRAGMA table_info(session_locks)")?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|name| name == column);
    if !exists {
        tx.execute_batch(&format!(
            "ALTER TABLE session_locks ADD COLUMN {column} {ddl}"
        ))?;
    }
    Ok(())
}
