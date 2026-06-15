//! Lightweight stable core types.
//!
//! This crate is intentionally small in the first workspace split. Runtime
//! modules still re-export the legacy in-crate core surface while the remaining
//! reverse dependencies are migrated out of `wunder-runtime`.

pub mod json_schema;
pub mod path_utils;
pub mod repo_assets;
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
