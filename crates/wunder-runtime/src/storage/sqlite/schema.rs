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
        tx.commit()?;
        *self.wal_anchor.lock() = Some(conn);
        self.initialized.store(true, Ordering::Release);
        Ok(())
    }
}
