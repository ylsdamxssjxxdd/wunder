use super::{PostgresStorage, SqliteStorage, StorageBackend};
use crate::config::StorageConfig;
use anyhow::{anyhow, Result};
use std::sync::Arc;

/// Build storage backend from config, selecting SQLite/Postgres.
pub fn build_storage(config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    let backend = config.backend.trim().to_lowercase();
    let backend = if backend.is_empty() {
        "sqlite".to_string()
    } else {
        backend
    };
    match backend.as_str() {
        "sqlite" | "default" => Ok(Arc::new(SqliteStorage::new(
            config.db_path.trim().to_string(),
        ))),
        "postgres" | "postgresql" | "pg" | "auto" => Ok(Arc::new(PostgresStorage::new(
            config.postgres.dsn.clone(),
            config.postgres.connect_timeout_s,
            config.postgres.pool_size,
        )?)),
        other => Err(anyhow!("未知存储后端: {other}")),
    }
}
