//! Lightweight stable core types.
//!
//! This crate is intentionally small in the first workspace split. Runtime
//! modules still re-export the legacy in-crate core surface while the remaining
//! reverse dependencies are migrated out of `wunder-runtime`.

pub mod approval;
pub mod atomic_write;
pub mod auth;
pub mod config;
pub mod drawio_config;
pub mod exec_policy;
pub mod i18n;
pub mod json_schema;
pub mod llm_speed;
pub mod onlyoffice_config;
pub mod path_utils;
pub mod repo_assets;
pub mod request_limits;
pub mod schemas;
pub mod storage_backend;
pub mod storage_bridge;
pub mod storage_constants;
pub mod storage_records;
pub mod token_utils;
pub mod tool_args;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeShape {
    Server,
    Cli,
    Desktop,
}

impl RuntimeShape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Cli => "cli",
            Self::Desktop => "desktop",
        }
    }
}
