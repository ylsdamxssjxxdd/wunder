use super::build_model_tool_success_with_hint;
use chrono::{Local, Utc};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(crate) enum SessionCleanup {
    Keep,
    Delete,
}

pub(crate) fn session_cleanup_label(cleanup: SessionCleanup) -> &'static str {
    match cleanup {
        SessionCleanup::Keep => "keep",
        SessionCleanup::Delete => "delete",
    }
}

pub(crate) fn parse_cleanup_mode(value: Option<&str>) -> SessionCleanup {
    match value.unwrap_or("").trim().to_lowercase().as_str() {
        "delete" | "remove" => SessionCleanup::Delete,
        _ => SessionCleanup::Keep,
    }
}

pub(crate) fn normalize_tool_run_state(status: &str) -> String {
    match status.trim().to_ascii_lowercase().as_str() {
        "" => "accepted".to_string(),
        "ok" | "success" => "completed".to_string(),
        "accepted" => "accepted".to_string(),
        "running" | "queued" | "waiting" => "running".to_string(),
        "timeout" => "timeout".to_string(),
        "cancelled" | "cancelling" => "cancelled".to_string(),
        "partial" => "partial".to_string(),
        "error" | "failed" => "error".to_string(),
        other => other.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_session_tool_result(
    action: &str,
    raw_status: &str,
    session_id: Option<String>,
    run_id: String,
    reply: Option<String>,
    error: Option<String>,
    elapsed_s: Option<f64>,
    next_step_hint: Option<String>,
) -> Value {
    let state = normalize_tool_run_state(raw_status);
    let summary = match state.as_str() {
        "completed" => match action {
            "spawn" => "Child session completed the initial task.".to_string(),
            _ => "Child session completed the requested turn.".to_string(),
        },
        "accepted" => match action {
            "spawn" => "Child session was created and the initial task was queued.".to_string(),
            _ => "Child session accepted the message and is still running.".to_string(),
        },
        "running" => "Child session is still running.".to_string(),
        "timeout" => {
            "Waiting for the child session timed out; the run may still be executing.".to_string()
        }
        "cancelled" => "Child session run was cancelled.".to_string(),
        "partial" => "Child session finished with partial results.".to_string(),
        _ => "Child session run failed.".to_string(),
    };
    build_model_tool_success_with_hint(
        action,
        &state,
        summary,
        json!({
            "run_id": run_id,
            "session_id": session_id,
            "reply": reply,
            "error": error,
            "elapsed_s": elapsed_s,
            "reply_pending": matches!(state.as_str(), "accepted" | "running" | "timeout"),
        }),
        next_step_hint,
    )
}

pub(crate) fn dedupe_non_empty_strings(items: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for item in items {
        let cleaned = item.trim();
        if cleaned.is_empty() {
            continue;
        }
        if seen.insert(cleaned.to_string()) {
            output.push(cleaned.to_string());
        }
    }
    output
}

pub(crate) fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

pub(crate) fn clamp_limit(value: Option<i64>, default: i64, max: i64) -> i64 {
    value.unwrap_or(default).max(0).min(max)
}

pub(crate) fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

pub(crate) fn format_ts(ts: f64) -> String {
    let millis = (ts * 1000.0) as i64;
    chrono::DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|dt| dt.with_timezone(&Local).to_rfc3339())
        .unwrap_or_default()
}

pub(crate) fn truncate_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut output = trimmed.chars().take(max_chars).collect::<String>();
    output.push_str("...");
    output
}
