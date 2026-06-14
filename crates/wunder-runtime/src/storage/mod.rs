// 存储模块：封装 SQLite/Postgres 持久化读写，提供统一的历史/监控/记忆接口。

mod backend;
mod bridge;
mod constants;
mod factory;
mod postgres;
mod records;
mod sqlite;

pub use backend::StorageBackend;
pub use bridge::*;
pub(crate) use constants::{TOOL_LOG_EXCLUDED_NAMES, TOOL_LOG_SKILL_READ_MARKER};
pub use constants::{
    normalize_hive_id, normalize_sandbox_container_id, normalize_workspace_container_id,
    DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID,
    MIN_SANDBOX_CONTAINER_ID, USER_PRIVATE_CONTAINER_ID,
};
pub use factory::build_storage;
pub use postgres::PostgresStorage;
pub use records::*;
pub use sqlite::SqliteStorage;