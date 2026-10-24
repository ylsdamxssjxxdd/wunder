// 存储模块：封装 SQLite/Postgres 持久化读写，提供统一的历史/监控/记忆接口。

#[cfg(test)]
mod agent_message_tests;
mod backend;
mod bridge;
pub(crate) mod canonical_thread_log;
#[cfg(test)]
mod canonical_thread_log_tests;
#[cfg(test)]
mod cloud_tests;
mod constants;
mod factory;
#[cfg(feature = "postgres-storage")]
mod postgres;
#[cfg(test)]
mod queue_control_tests;
#[cfg(test)]
mod quota_tests;
mod records;
#[cfg(test)]
mod refresh_token_tests;
/// Canonical, append-only thread-history files.  This is deliberately kept
/// separate from the legacy SQL thread-log implementation while callers move
/// to the new ThreadStore contract.
pub mod rollout;
pub mod rollout_compression;
mod runtime;
pub(crate) mod runtime_logs;
#[cfg(any(feature = "sqlite-storage", feature = "postgres-storage", test))]
mod session_cleanup;
#[cfg(any(feature = "sqlite-storage", test))]
mod sqlite;
pub(crate) mod stress_model;
// The canonical history projection is deliberately isolated from the legacy
// thread log tables. It is a rebuildable SQLite view of rollout JSONL.
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod thread_admission;
mod thread_chat_summary;
pub(crate) mod thread_history;
#[cfg(test)]
mod thread_lifecycle_tests;
pub(crate) mod thread_mutation;
pub mod thread_store;
mod thread_view_projection;

pub use backend::*;
pub use bridge::*;
pub use constants::{
    normalize_sandbox_container_id, normalize_workspace_container_id, DEFAULT_SANDBOX_CONTAINER_ID,
    MAX_SANDBOX_CONTAINER_ID, MIN_SANDBOX_CONTAINER_ID, USER_PRIVATE_CONTAINER_ID,
};
pub use factory::{build_storage, sqlite_state_database_path};
#[cfg(all(feature = "postgres-storage", any(feature = "sqlite-storage", test)))]
pub(crate) use postgres::is_canonical_root_binding_failure;
#[cfg(all(feature = "postgres-storage", test))]
pub(crate) use postgres::CanonicalRootBindingError;
#[cfg(feature = "postgres-storage")]
pub use postgres::PostgresStorage;
pub use records::*;
#[cfg(any(feature = "sqlite-storage", test))]
pub use sqlite::SqliteStorage;
pub use stress_model::{
    StressThreadScenario, StressThreadSpec, StressThreadStats, StressThreadStorageBytes,
};
