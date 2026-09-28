pub(crate) const TOOL_LOG_SKILL_READ_MARKER: &str = "\"source\":\"skill_read\"";

// tool_logs column budget: keep head+tail of serialized args/data with a
// marker so rows stay bounded regardless of tool output size.
pub(crate) const TOOL_LOG_HEAD_CHARS: usize = 2000;
pub(crate) const TOOL_LOG_TAIL_CHARS: usize = 2000;
pub(crate) const TOOL_LOG_TRUNCATION_MARKER: &str = "...(truncated)...";

/// Bound a persisted tool log column to head+tail chars, inserting a marker
/// between the kept segments. Char-based truncation, mirroring the tool
/// result truncation style in the orchestrator.
pub(crate) fn truncate_tool_log_column(text: &str) -> String {
    let total_chars = text.chars().count();
    let budget = TOOL_LOG_HEAD_CHARS + TOOL_LOG_TAIL_CHARS;
    if total_chars <= budget {
        return text.to_string();
    }
    let head: String = text.chars().take(TOOL_LOG_HEAD_CHARS).collect();
    let tail: String = text
        .chars()
        .skip(total_chars - TOOL_LOG_TAIL_CHARS)
        .collect();
    format!("{head}{TOOL_LOG_TRUNCATION_MARKER}{tail}")
}

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
