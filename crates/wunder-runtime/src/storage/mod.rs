// 存储模块：封装 SQLite/Postgres 持久化读写，提供统一的历史/监控/记忆接口。

mod backend;
mod bridge;
mod constants;
mod factory;
#[cfg(feature = "postgres-storage")]
mod postgres;
mod records;
#[cfg(any(feature = "sqlite-storage", test))]
mod sqlite;

pub use backend::*;
pub use bridge::*;
pub use constants::{
    normalize_hive_id, normalize_sandbox_container_id, normalize_workspace_container_id,
    DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID,
    MIN_SANDBOX_CONTAINER_ID, USER_PRIVATE_CONTAINER_ID,
};
#[cfg(any(feature = "postgres-storage", feature = "sqlite-storage", test))]
pub(crate) use constants::{TOOL_LOG_EXCLUDED_NAMES, TOOL_LOG_SKILL_READ_MARKER};
pub use factory::build_storage;
#[cfg(feature = "postgres-storage")]
pub use postgres::PostgresStorage;
pub use records::*;
#[cfg(any(feature = "sqlite-storage", test))]
pub use sqlite::SqliteStorage;
