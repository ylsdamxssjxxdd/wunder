use super::StorageBackend;
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
        "sqlite" | "default" => build_sqlite_storage(config),
        "postgres" | "postgresql" | "pg" | "auto" => build_postgres_storage(config),
        other => Err(anyhow!("unknown storage backend: {other}")),
    }
}

#[cfg(any(feature = "sqlite-storage", test))]
fn build_sqlite_storage(config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    Ok(Arc::new(super::SqliteStorage::new(
        config.db_path.trim().to_string(),
    )))
}

#[cfg(not(any(feature = "sqlite-storage", test)))]
fn build_sqlite_storage(_config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    Err(storage_feature_disabled("sqlite", "sqlite-storage"))
}

#[cfg(feature = "postgres-storage")]
fn build_postgres_storage(config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    Ok(Arc::new(super::PostgresStorage::new(
        config.postgres.dsn.clone(),
        config.postgres.connect_timeout_s,
        config.postgres.pool_size,
    )?))
}

#[cfg(not(feature = "postgres-storage"))]
fn build_postgres_storage(_config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    Err(storage_feature_disabled("postgres", "postgres-storage"))
}

fn storage_feature_disabled(backend: &str, feature: &str) -> anyhow::Error {
    anyhow!("storage backend '{backend}' is disabled; enable the '{feature}' feature")
}
