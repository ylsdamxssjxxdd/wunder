use super::{build_model_tool_success, build_model_tool_success_with_hint};
use crate::i18n;
use crate::orchestrator_constants::truncate_tool_result_text;
use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) const SWARM_WAIT_DEFAULT_POLL_S: f64 = 1.0;
pub(crate) const SWARM_WAIT_MIN_POLL_S: f64 = 0.2;
pub(crate) const SWARM_WAIT_MAX_POLL_S: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SwarmWaitMode {
    Immediate,
    Finite(f64),
    Infinite,
}

#[derive(Clone, Copy)]
pub(crate) enum SessionCleanup {
    Keep,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwarmWorkerThreadStrategy {
    FreshMainThread,
    MainThread,
}

impl SwarmWorkerThreadStrategy {
    pub(crate) fn as_tool_value(self) -> &'static str {
        match self {
            Self::FreshMainThread => "fresh_main_thread",
            Self::MainThread => "main_thread",
        }
    }
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

pub(crate) fn parse_swarm_worker_thread_strategy(
    thread_strategy: Option<&str>,
    reuse_main_thread: Option<bool>,
) -> Result<SwarmWorkerThreadStrategy> {
    if reuse_main_thread.unwrap_or(false) {
        return Ok(SwarmWorkerThreadStrategy::MainThread);
    }
    let normalized = thread_strategy
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase().replace('-', "_"))
        .unwrap_or_default();
    if normalized.is_empty() {
        return Ok(SwarmWorkerThreadStrategy::MainThread);
    }
    if matches!(
        normalized.as_str(),
        "fresh_main_thread" | "new_main_thread" | "fresh" | "new_thread"
    ) {
        return Ok(SwarmWorkerThreadStrategy::FreshMainThread);
    }
    if matches!(
        normalized.as_str(),
        "main_thread" | "current_main_thread" | "reuse_main_thread" | "main"
    ) {
        return Ok(SwarmWorkerThreadStrategy::MainThread);
    }
    Err(anyhow!(
        "invalid thread_strategy: {normalized}; expected fresh_main_thread or main_thread"
    ))
}

pub(crate) fn resolve_swarm_wait_mode(
    requested_timeout_s: Option<f64>,
    default_timeout_s: u64,
) -> SwarmWaitMode {
    match requested_timeout_s {
        Some(timeout_s) if timeout_s > 0.0 => SwarmWaitMode::Finite(timeout_s),
        Some(_) => SwarmWaitMode::Immediate,
        None if default_timeout_s > 0 => SwarmWaitMode::Finite(default_timeout_s as f64),
        None => SwarmWaitMode::Infinite,
    }
}

pub(crate) fn swarm_wait_seconds_value(wait_mode: SwarmWaitMode) -> Option<f64> {
    match wait_mode {
        SwarmWaitMode::Immediate => Some(0.0),
        SwarmWaitMode::Finite(timeout_s) => Some(timeout_s),
        SwarmWaitMode::Infinite => None,
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_agent_swarm_tool_result(
    action: &str,
    state: &str,
    team_run_id: String,
    task_id: Option<String>,
    run_id: Option<String>,
    session_id: Option<String>,
    target_agent_id: Option<String>,
    target_agent_name: Option<String>,
    created_session: Option<bool>,
    reply: Option<String>,
    error: Option<String>,
    elapsed_s: Option<f64>,
    extra: Option<Value>,
) -> Value {
    let summary = match state {
        "completed" => "Worker finished and returned a result.".to_string(),
        "accepted" => "Worker task was queued and is still running.".to_string(),
        "running" => "Worker task is still running.".to_string(),
        "timeout" => {
            "Waiting for the worker timed out; the run may still be executing.".to_string()
        }
        "partial" => "Worker batch finished with partial success.".to_string(),
        "cancelled" => "Worker task was cancelled.".to_string(),
        _ => "Worker task failed.".to_string(),
    };
    let next_step_hint = if matches!(state, "accepted" | "running" | "timeout") {
        Some(
            "Use agent_swarm.wait or status/history before treating the worker result as final."
                .to_string(),
        )
    } else {
        None
    };
    let mut data = json!({
        "team_run_id": team_run_id,
        "task_id": task_id,
        "run_id": run_id,
        "session_id": session_id,
        "target_agent_id": target_agent_id,
        "target_agent_name": target_agent_name,
        "created_session": created_session,
        "reply": reply,
        "error": error,
        "elapsed_s": elapsed_s,
    });
    if let Some(extra) = extra {
        if let (Some(data_map), Some(extra_map)) = (data.as_object_mut(), extra.as_object()) {
            for (key, value) in extra_map {
                data_map.insert(key.clone(), value.clone());
            }
        }
    }
    build_model_tool_success_with_hint(action, state, summary, data, next_step_hint)
}

pub(crate) fn is_swarm_task_terminal_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "success" | "error" | "failed" | "timeout" | "cancelled"
    )
}

pub(crate) fn skipped_swarm_task_result(
    action: &str,
    team_task_id: &str,
    session_id: &str,
    agent_id: &str,
    agent_name: &str,
    reason: &str,
) -> Value {
    build_model_tool_success(
        action,
        "skipped",
        format!("Skipped swarm task {team_task_id}: {reason}."),
        json!({
            "task_id": team_task_id,
            "run_id": Value::Null,
            "session_id": session_id,
            "agent_id": agent_id,
            "agent_name": agent_name,
            "skipped": true,
            "skip_reason": reason,
        }),
    )
}

pub(crate) fn compact_swarm_run_result_preview(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(truncate_tool_result_text)
}

pub(crate) fn is_swarm_run_terminal(status: &str) -> bool {
    matches!(
        status,
        "success" | "error" | "timeout" | "cancelled" | "failed" | "not_found"
    )
}

pub(crate) fn is_swarm_run_failed(status: &str) -> bool {
    matches!(
        status,
        "error" | "timeout" | "cancelled" | "failed" | "not_found"
    )
}

pub(crate) fn normalize_swarm_poll_interval(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return SWARM_WAIT_DEFAULT_POLL_S;
    }
    value.clamp(SWARM_WAIT_MIN_POLL_S, SWARM_WAIT_MAX_POLL_S)
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

pub(crate) fn resolve_session_key(value: Option<String>) -> Result<String> {
    let Some(key) = normalize_optional_string(value) else {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    };
    Ok(key)
}

fn is_swarm_artifact_path_misused_as_session_key(value: &str) -> bool {
    let normalized = value.trim().replace('\\', "/");
    let stripped = normalized.strip_prefix("./").unwrap_or(&normalized);
    stripped.starts_with("orchestration/")
        || stripped.starts_with("workspaces/")
        || stripped.starts_with("/workspaces/")
}

pub(crate) fn resolve_swarm_batch_session_key(value: Option<String>) -> Result<Option<String>> {
    let Some(key) = normalize_optional_string(value) else {
        return Ok(None);
    };
    if is_swarm_artifact_path_misused_as_session_key(&key) {
        return Ok(None);
    }
    Ok(Some(resolve_session_key(Some(key))?))
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
