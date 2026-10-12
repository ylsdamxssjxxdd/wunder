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
    "update_plan",
    "sessions_yield",
    "yield",
    "会话让出",
    "计划面板",
    "question_panel",
    "ask_panel",
    "问询面板",
    "performance_log",
    "final_response",
    "最终回复",
];

/// Usage summary watermarks: reconcile only aggregates rows past these
/// markers, and delete paths only decrement rows counted inside them.
pub(crate) const USAGE_TOOL_WATERMARK_KEY: &str = "usage_tool_watermark";
pub(crate) const USAGE_CHAT_WATERMARK_KEY: &str = "usage_chat_watermark_ts";
pub(crate) const USAGE_FULL_RECONCILE_KEY: &str = "usage_full_reconcile_ts";
pub(crate) const USAGE_FULL_RECONCILE_INTERVAL_S: f64 = 7.0 * 24.0 * 3600.0;

/// Whether a tool log row is excluded from usage statistics at write time:
/// excluded tool names or skill_read diagnostics never reach stats queries.
/// Must be evaluated on the full (pre-truncation) data column so the marker
/// cannot be cut away by head+tail truncation.
pub(crate) fn tool_log_row_excluded(tool: &str, data: Option<&str>) -> bool {
    let cleaned = tool.trim();
    if TOOL_LOG_EXCLUDED_NAMES.iter().any(|name| *name == cleaned) {
        return true;
    }
    data.is_some_and(|data| data.contains(TOOL_LOG_SKILL_READ_MARKER))
}

pub use wunder_core::storage_constants::{
    normalize_sandbox_container_id, normalize_workspace_container_id, DEFAULT_SANDBOX_CONTAINER_ID,
    MAX_SANDBOX_CONTAINER_ID, MIN_SANDBOX_CONTAINER_ID, USER_PRIVATE_CONTAINER_ID,
};
