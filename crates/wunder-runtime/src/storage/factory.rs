use super::StorageBackend;
use crate::config::StorageConfig;
use anyhow::{anyhow, Result};
use std::path::PathBuf;
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

/// Dedicated SQLite state database for the legacy control-plane services.
/// Thread history is stored separately below the same root and must never be
/// folded back into this file.
pub fn sqlite_state_database_path(config: &StorageConfig) -> PathBuf {
    let root = config.data_root.trim();
    let root = if root.is_empty() {
        "./config/data"
    } else {
        root
    };
    PathBuf::from(root).join("state_1.sqlite")
}

#[cfg(any(feature = "sqlite-storage", test))]
fn build_sqlite_storage(config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    let path = sqlite_state_database_path(config);
    Ok(Arc::new(super::SqliteStorage::new(
        path.to_string_lossy().into_owned(),
    )))
}

#[cfg(not(any(feature = "sqlite-storage", test)))]
fn build_sqlite_storage(_config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    Err(storage_feature_disabled("sqlite", "sqlite-storage"))
}

#[cfg(feature = "postgres-storage")]
fn build_postgres_storage(config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    Ok(Arc::new(super::PostgresStorage::new_with_data_root(
        config.postgres.dsn.clone(),
        config.postgres.connect_timeout_s,
        config.postgres.pool_size,
        PathBuf::from(&config.data_root),
    )?))
}

#[cfg(not(feature = "postgres-storage"))]
fn build_postgres_storage(_config: &StorageConfig) -> Result<Arc<dyn StorageBackend>> {
    Err(storage_feature_disabled("postgres", "postgres-storage"))
}

fn storage_feature_disabled(backend: &str, feature: &str) -> anyhow::Error {
    anyhow!("storage backend '{backend}' is disabled; enable the '{feature}' feature")
}
