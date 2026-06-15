pub(crate) const TOOL_LOG_SKILL_READ_MARKER: &str = "\"source\":\"skill_read\"";

pub(crate) const TOOL_LOG_EXCLUDED_NAMES: &[&str] = &[
    "final_response",
    "最终回复",
    "update_plan",
    "sessions_yield",
    "yield",
    "会话让出",
    "计划面板",
    "question_panel",
    "ask_panel",
    "问询面板",
    "a2ui",
    "a2a_observe",
    "a2a_wait",
    "a2a观察",
    "a2a等待",
    "performance_log",
];

pub use wunder_core::storage_constants::{
    normalize_hive_id, normalize_sandbox_container_id, normalize_workspace_container_id,
    DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID, MAX_SANDBOX_CONTAINER_ID,
    MIN_SANDBOX_CONTAINER_ID, USER_PRIVATE_CONTAINER_ID,
};
