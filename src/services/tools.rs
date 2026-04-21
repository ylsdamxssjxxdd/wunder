// 内置工具定义与执行入口，保持工具名称与协议一致。
//
// NOTE FOR CONTRIBUTORS:
// This file is in maintenance mode due to its size and complexity.
// Do not add new tool business logic directly in `tools.rs`.
// Implement new capabilities in dedicated modules/files and only wire them here.
mod apply_patch_tool;
mod browser_tool;
mod catalog;
mod channel_tool;
pub(crate) mod command_options;
pub(crate) mod command_output_guard;
pub mod command_sessions;
mod context;
mod desktop_control;
mod dispatch;
mod freeform;
mod memory_manager_tool;
mod read_file_guard;
mod read_image_tool;
mod read_indentation;
mod search_content_tool;
mod self_status_tool;
mod session_run_stream;
pub(crate) mod sessions_yield_tool;
mod skill_call;
mod sleep_tool;
mod subagent_control;
mod swarm_realtime;
mod swarm_tool_error;
mod swarm_tool_hint;
mod thread_control_tool;
pub(crate) mod tool_error;
mod web_fetch_tool;

#[cfg(test)]
pub(crate) use catalog::builtin_tool_specs_with_language;
pub(crate) use catalog::yaml_to_json;
pub use catalog::{
    a2a_service_schema, a2a_service_schema_with_language, browser_tools_available,
    build_desktop_followup_user_message, build_read_image_followup_user_message, builtin_aliases,
    builtin_tool_specs, collect_available_tool_names, collect_prompt_tool_specs,
    collect_prompt_tool_specs_with_language, desktop_tools_available, extract_sleep_seconds,
    filter_tool_names_by_model_capability, is_browser_tool_name, is_desktop_control_tool_name,
    is_read_image_tool_name, is_sleep_tool_name, resolve_tool_name,
};
pub use context::{build_tool_roots, ToolContext, ToolEventEmitter, ToolRoots};
pub(crate) use context::{
    collect_allow_roots, collect_read_roots, resolve_path_in_roots, resolve_tool_path,
    roots_allow_any_path,
};
pub use dispatch::{execute_builtin_tool, execute_tool};
pub(crate) use freeform::{
    build_responses_freeform_tool, extract_freeform_tool_input, is_freeform_tool_name,
    render_prompt_tool_spec,
};
pub(crate) use memory_manager_tool::execute_memory_manager_tool;
pub(crate) use thread_control_tool::execute_thread_control_tool;

use crate::a2a_store::A2aTask;
use crate::command_utils;
use crate::config::{
    is_debug_log_level, normalize_knowledge_base_type, A2aServiceConfig, Config,
    KnowledgeBaseConfig, KnowledgeBaseType,
};
use crate::core::atomic_write::atomic_write_text;
use crate::core::python_runtime;
use crate::core::tool_args::recover_tool_args_value as recover_tool_args_value_lossy;
use crate::core::tool_fs_filter;
use crate::gateway::GatewayNodeInvokeRequest;
use crate::history::HistoryManager;
use crate::i18n;
use crate::knowledge;
use crate::llm::embed_texts;
use crate::lsp::LspDiagnostic;
use crate::mcp;
use crate::monitor::MonitorState;
use crate::orchestrator_constants::truncate_tool_result_text;
use crate::path_utils::{
    is_within_root, normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::sandbox;
use crate::schemas::WunderRequest;
use crate::services::agent_abilities::resolve_agent_runtime_tool_names;
use crate::services::orchestration_context::{
    active_orchestration_for_agent, build_worker_dispatch_message,
    ensure_orchestration_member_session, load_dispatch_context, session_has_visible_history,
    session_orchestration_run_root,
};
use crate::services::subagents;
use crate::services::swarm::beeroom::{
    agent_in_hive, build_swarm_dispatch_message, claim_mother_agent as claim_swarm_mother_agent,
    ensure_swarm_agent_in_hive as ensure_swarm_agent_in_beeroom,
    resolve_swarm_hive_id as resolve_swarm_hive_scope,
};
use crate::skills::{execute_skill, SkillRegistry, SkillSpec};
use crate::storage::{
    normalize_hive_id, AgentThreadRecord, ChatSessionRecord, SessionRunRecord, StorageBackend,
    TeamRunRecord, TeamTaskRecord, UserAgentAccessRecord, UserAgentRecord,
};
use crate::user_store::{build_default_agent_record_from_storage, UserStore};
use crate::user_tools::{UserToolAlias, UserToolKind};
use crate::vector_knowledge;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
#[cfg(windows)]
use encoding_rs::GBK;
use futures::stream::{self, StreamExt};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Map, Value};
use skill_call::render_skill_markdown_for_model;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use swarm_realtime::{
    apply_session_run_to_swarm_task, emit_swarm_run_started, emit_swarm_run_terminal,
    emit_swarm_task_dispatched, emit_swarm_task_updated, reconcile_swarm_task_from_session_run,
    sync_swarm_run_summary,
};
use swarm_tool_error::{
    agent_swarm_batch_send_example, agent_swarm_send_example, agent_swarm_spawn_example,
    agent_swarm_wait_example, build_agent_swarm_args_failure,
};
use swarm_tool_hint::resolve_swarm_agent_record;
use tokio::io::AsyncReadExt;
use tokio::sync::oneshot;
use tokio::time::{sleep, timeout};
use tracing::warn;
use url::Url;
use uuid::Uuid;
use walkdir::WalkDir;

use command_options::{apply_time_budget_secs, parse_command_budget, parse_dry_run};
use command_output_guard::{
    derive_capture_policies, render_command_output, CommandOutputCapture, CommandOutputCaptureMeta,
    CommandOutputCollector, CommandOutputPolicy, DEFAULT_CAPTURE_TOTAL_BYTES,
    STDERR_CAPTURE_POLICY, STDOUT_CAPTURE_POLICY,
};
use command_sessions::{CommandSessionLaunchMode, CommandSessionStream, CommandSessionTracker};
use tool_error::{
    build_execute_command_failure_data, build_execute_command_failure_message,
    build_failed_tool_result, ToolErrorMeta,
};

const MAX_READ_BYTES: usize = 1024 * 1024;
const MAX_READ_LINES: usize = 2000;
const DEFAULT_START_LINE_WINDOW: usize = 200;
const MAX_READ_BUDGET_FILES: usize = 20;
const MIN_READ_OUTPUT_BUDGET_BYTES: usize = 1024;
const MAX_READ_OUTPUT_BUDGET_BYTES: usize = 2 * 1024 * 1024;
const MAX_READ_TIME_BUDGET_MS: u64 = 10 * 60 * 1000;
const MAX_RANGE_SPAN: usize = 2000;
const DEFAULT_LIST_DEPTH: usize = 1;
const DEFAULT_LIST_PAGE_LIMIT: usize = 500;
const MAX_LIST_ITEMS: usize = 500;
const MAX_SEARCH_MATCHES: usize = 200;
const MAX_LSP_DIAGNOSTICS: usize = 20;
const MAX_SESSION_LIST_ITEMS: i64 = 200;
const MAX_SESSION_HISTORY_ITEMS: i64 = 500;
const MAX_SESSION_MESSAGE_ITEMS: i64 = 50;
const MAX_USER_WORLD_LIST_LIMIT: i64 = 500;
const USER_WORLD_FILE_STAGING_DIR: &str = "user_world_uploads";
const LOCAL_PTC_TIMEOUT_S: u64 = 60;
const LOCAL_PTC_DIR_NAME: &str = "ptc_temp";
const TOOL_OVERRIDE_NONE: &str = "__no_tools__";
const DEFAULT_SESSION_TITLE: &str = "新会话";
const ANNOUNCE_SKIP: &str = "ANNOUNCE_SKIP";
const SWARM_WAIT_DEFAULT_POLL_S: f64 = 1.0;
const SWARM_WAIT_MIN_POLL_S: f64 = 0.2;
const SWARM_WAIT_MAX_POLL_S: f64 = 5.0;
const SUBAGENT_MESSAGE_PREVIEW_MAX_CHARS: usize = 240;

fn compact_cron_tool_result(value: Value) -> Value {
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut output = json!({ "action": action });
    if let Some(removed) = value.get("removed") {
        output["removed"] = removed.clone();
    }
    if let Some(queued) = value.get("queued") {
        output["queued"] = queued.clone();
    }
    if let Some(reason) = value.get("reason") {
        output["reason"] = reason.clone();
    }
    if let Some(deduped) = value.get("deduped") {
        output["deduped"] = deduped.clone();
    }
    if let Some(job) = value.get("job") {
        output["job"] = compact_cron_job(job);
    }
    if let Some(jobs) = value.get("jobs") {
        output["jobs"] = compact_cron_jobs(jobs);
    }
    if let Some(scheduler) = value.get("scheduler") {
        output["scheduler"] = scheduler.clone();
    }
    if let Some(user_jobs) = value.get("user_jobs") {
        output["user_jobs"] = user_jobs.clone();
    }
    output
}

pub(crate) fn build_model_tool_success(
    action: &str,
    state: &str,
    summary: impl Into<String>,
    data: Value,
) -> Value {
    json!({
        "ok": true,
        "action": action,
        "state": state,
        "summary": summary.into(),
        "data": data,
    })
}

pub(crate) fn build_model_tool_success_with_hint(
    action: &str,
    state: &str,
    summary: impl Into<String>,
    data: Value,
    next_step_hint: Option<String>,
) -> Value {
    let mut result = build_model_tool_success(action, state, summary, data);
    if let Some(next_step_hint) = next_step_hint
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        result["next_step_hint"] = Value::String(next_step_hint);
    }
    result
}

fn collect_orchestration_run_roots(context: &ToolContext<'_>) -> Vec<PathBuf> {
    session_orchestration_run_root(
        context.storage.as_ref(),
        context.workspace.as_ref(),
        context.workspace_id,
        context.user_id,
        context.session_id,
    )
    .into_iter()
    .filter(|path| path.exists())
    .collect()
}

fn collect_orchestration_aware_allow_roots(context: &ToolContext<'_>) -> Vec<PathBuf> {
    let mut roots = collect_allow_roots(context);
    roots.extend(collect_orchestration_run_roots(context));
    roots
}

pub(crate) fn tool_result_data(value: &Value) -> &Value {
    value.get("data").unwrap_or(value)
}

pub(crate) fn tool_result_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    tool_result_data(value).get(key).or_else(|| value.get(key))
}

fn tool_result_field_or_null(value: &Value, key: &str) -> Value {
    tool_result_field(value, key)
        .cloned()
        .unwrap_or(Value::Null)
}

fn compact_cron_jobs(value: &Value) -> Value {
    let Some(items) = value.as_array() else {
        return Value::Array(Vec::new());
    };
    let jobs = items.iter().map(compact_cron_job).collect::<Vec<_>>();
    Value::Array(jobs)
}

fn compact_cron_job(job: &Value) -> Value {
    let schedule = job.get("schedule").and_then(Value::as_object);
    let schedule = json!({
        "kind": schedule.and_then(|map| map.get("kind")).cloned().unwrap_or(Value::Null),
        "at": schedule.and_then(|map| map.get("at")).cloned().unwrap_or(Value::Null),
        "every_ms": schedule.and_then(|map| map.get("every_ms")).cloned().unwrap_or(Value::Null),
        "cron": schedule.and_then(|map| map.get("cron")).cloned().unwrap_or(Value::Null),
        "tz": schedule.and_then(|map| map.get("tz")).cloned().unwrap_or(Value::Null)
    });
    let next_run = job
        .get("next_run_at_text")
        .cloned()
        .or_else(|| job.get("next_run_at").cloned())
        .unwrap_or(Value::Null);
    let last_run = job
        .get("last_run_at_text")
        .cloned()
        .or_else(|| job.get("last_run_at").cloned())
        .unwrap_or(Value::Null);
    json!({
        "job_id": job.get("job_id").cloned().unwrap_or(Value::Null),
        "name": job.get("name").cloned().unwrap_or(Value::Null),
        "enabled": job.get("enabled").cloned().unwrap_or(Value::Null),
        "schedule": schedule,
        "next_run_at": next_run,
        "last_run_at": last_run,
        "last_status": job.get("last_status").cloned().unwrap_or(Value::Null)
    })
}

#[derive(Debug, Deserialize)]
struct PlanUpdateArgs {
    #[serde(default)]
    explanation: Option<String>,
    plan: Vec<PlanItemArgs>,
}

#[derive(Debug, Deserialize)]
struct PlanItemArgs {
    step: String,
    #[serde(default)]
    status: Option<String>,
}

fn normalize_plan_status(value: Option<&str>) -> String {
    let raw = value.unwrap_or("").trim().to_lowercase();
    if raw.is_empty() {
        return "pending".to_string();
    }
    let normalized = raw.replace(['-', ' '], "_");
    match normalized.as_str() {
        "pending" => "pending".to_string(),
        "in_progress" | "inprogress" => "in_progress".to_string(),
        "completed" | "complete" | "done" => "completed".to_string(),
        _ => "pending".to_string(),
    }
}

async fn execute_plan_tool(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: PlanUpdateArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    if payload.plan.is_empty() {
        return Err(anyhow!(i18n::t("tool.plan.plan_required")));
    }
    let mut seen_in_progress = false;
    let mut normalized_plan = Vec::new();
    for item in payload.plan {
        let step = item.step.trim().to_string();
        if step.is_empty() {
            continue;
        }
        let mut status = normalize_plan_status(item.status.as_deref());
        if status == "in_progress" {
            if seen_in_progress {
                status = "pending".to_string();
            } else {
                seen_in_progress = true;
            }
        }
        normalized_plan.push(json!({
            "step": step,
            "status": status
        }));
    }
    if normalized_plan.is_empty() {
        return Err(anyhow!(i18n::t("tool.plan.plan_required")));
    }
    let explanation = payload.explanation.and_then(|text| {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(
            "plan_update",
            json!({
                "explanation": explanation,
                "plan": normalized_plan
            }),
        );
    }
    Ok(build_model_tool_success(
        "plan_update",
        "completed",
        "Updated the execution plan.",
        json!({ "status": "ok" }),
    ))
}

#[derive(Debug)]
struct QuestionPanelRoute {
    label: String,
    description: Option<String>,
    recommended: bool,
}

#[derive(Debug)]
struct QuestionPanelPayload {
    question: String,
    routes: Vec<QuestionPanelRoute>,
    multiple: bool,
}

fn normalize_question_panel_payload(args: &Value) -> Result<QuestionPanelPayload> {
    let Some(obj) = args.as_object() else {
        return Err(anyhow!(i18n::t("tool.question_panel.routes_required")));
    };
    let question = obj
        .get("question")
        .or_else(|| obj.get("prompt"))
        .or_else(|| obj.get("title"))
        .or_else(|| obj.get("header"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let question = if question.is_empty() {
        i18n::t("tool.question_panel.default_question")
    } else {
        question
    };
    let multiple = obj
        .get("multiple")
        .or_else(|| obj.get("allow_multiple"))
        .or_else(|| obj.get("multi"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let routes = obj
        .get("routes")
        .or_else(|| obj.get("options"))
        .or_else(|| obj.get("choices"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut normalized = Vec::new();
    for item in routes {
        let (label, description, recommended) = match item {
            Value::String(value) => (value, None, false),
            Value::Object(map) => {
                let label = map
                    .get("label")
                    .or_else(|| map.get("title"))
                    .or_else(|| map.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let description = map
                    .get("description")
                    .or_else(|| map.get("detail"))
                    .or_else(|| map.get("desc"))
                    .or_else(|| map.get("summary"))
                    .and_then(Value::as_str)
                    .map(|value| value.to_string());
                let recommended = map
                    .get("recommended")
                    .or_else(|| map.get("preferred"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                (label, description, recommended)
            }
            _ => (String::new(), None, false),
        };
        let label = label.trim().to_string();
        if label.is_empty() {
            continue;
        }
        let description = description.and_then(|value| {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let recommended = recommended || label.contains("推荐");
        normalized.push(QuestionPanelRoute {
            label,
            description,
            recommended,
        });
    }
    if normalized.is_empty() {
        return Err(anyhow!(i18n::t("tool.question_panel.routes_required")));
    }
    Ok(QuestionPanelPayload {
        question,
        routes: normalized,
        multiple,
    })
}

async fn execute_question_panel_tool(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload = normalize_question_panel_payload(args)?;
    let question = payload.question.clone();
    let routes = payload
        .routes
        .iter()
        .map(|route| {
            json!({
                "label": route.label,
                "description": route.description,
                "recommended": route.recommended
            })
        })
        .collect::<Vec<_>>();
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(
            "question_panel",
            json!({
                "question": question.clone(),
                "routes": routes.clone(),
                "multiple": payload.multiple,
                "keep_open": true
            }),
        );
    }
    Ok(build_model_tool_success(
        "question_panel",
        "awaiting_input",
        "Opened a question panel and is waiting for user input.",
        json!({
            "question": question,
            "routes": routes,
            "multiple": payload.multiple
        }),
    ))
}

#[derive(Debug, Deserialize)]
struct UserWorldToolArgs {
    action: String,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    user_ids: Option<Vec<String>>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    client_msg_id: Option<String>,
}

async fn user_world_tool(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: UserWorldToolArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = payload.action.trim().to_lowercase();
    match action.as_str() {
        "list_users" | "list" | "users" => user_world_list_users(context, &payload).await,
        "send_message" | "send" | "message" => user_world_send_message(context, &payload).await,
        _ => Err(anyhow!("未知用户世界工具 action: {action}")),
    }
}

async fn user_world_list_users(
    context: &ToolContext<'_>,
    payload: &UserWorldToolArgs,
) -> Result<Value> {
    let keyword = payload
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let offset = payload.offset.unwrap_or(0).max(0);
    let limit = payload.limit.unwrap_or(0);
    let limit = if limit <= 0 {
        0
    } else {
        limit.clamp(1, MAX_USER_WORLD_LIST_LIMIT)
    };
    let user_store = UserStore::new(context.storage.clone());
    let (users, total) = user_store.list_users(keyword, None, offset, limit)?;
    let items = users
        .into_iter()
        .map(|user| {
            json!({
                "user_id": user.user_id,
                "username": user.username,
                "status": user.status,
                "unit_id": user.unit_id
            })
        })
        .collect::<Vec<_>>();
    Ok(build_model_tool_success(
        "list_users",
        "completed",
        format!("Listed {total} users from user world."),
        json!({
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit
        }),
    ))
}

#[derive(Debug, Clone)]
struct UserWorldFileRefMatch {
    token_start: usize,
    token_end: usize,
    normalized_path: String,
    suffix: String,
}

#[derive(Debug, Clone)]
struct UserWorldCopiedFile {
    source_path: String,
    staged_path: String,
    entry_type: &'static str,
}

fn user_world_file_ref_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?m)(^|[\s\n])@("[^"]+"|'[^']+'|\S+)"#)
            .expect("user world file ref regex must be valid")
    })
}

fn is_user_world_file_ref_suffix(ch: char) -> bool {
    matches!(
        ch,
        ')' | ']'
            | '}'
            | '>'
            | ','
            | '.'
            | ';'
            | ':'
            | '!'
            | '?'
            | '，'
            | '。'
            | '；'
            | '：'
            | '！'
            | '？'
            | '）'
            | '】'
            | '》'
            | '、'
    )
}

fn split_user_world_file_ref_suffix(value: &str) -> (&str, &str) {
    let mut split_at = value.len();
    for (index, ch) in value.char_indices().rev() {
        if is_user_world_file_ref_suffix(ch) {
            split_at = index;
        } else {
            break;
        }
    }
    if split_at == value.len() {
        (value, "")
    } else {
        (&value[..split_at], &value[split_at..])
    }
}

fn looks_like_user_world_file_ref(raw: &str, normalized: &str) -> bool {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('@') {
        return false;
    }
    if raw.starts_with('/')
        || raw.starts_with('\\')
        || raw.starts_with("./")
        || raw.starts_with("../")
        || raw.starts_with("workspaces/")
        || raw.starts_with("/workspaces/")
        || raw.starts_with("workspace/")
        || raw.starts_with("/workspace/")
    {
        return true;
    }
    normalized.contains('/') || normalized.contains('.')
}

fn normalize_user_world_file_ref_path(raw: &str, source_workspace_id: &str) -> Option<String> {
    let mut value = raw.trim().replace('\\', "/");
    if value.is_empty() {
        return None;
    }
    if let Some(stripped) = value.strip_prefix("/workspaces/") {
        let stripped = stripped.trim_matches('/');
        let mut segments = stripped.splitn(2, '/');
        let owner = segments.next().unwrap_or("").trim();
        let rest = segments.next().unwrap_or("").trim();
        if owner.is_empty() || rest.is_empty() {
            return None;
        }
        if owner != source_workspace_id {
            return None;
        }
        value = rest.to_string();
    } else if let Some(stripped) = value.strip_prefix("workspaces/") {
        let stripped = stripped.trim_matches('/');
        let mut segments = stripped.splitn(2, '/');
        let owner = segments.next().unwrap_or("").trim();
        let rest = segments.next().unwrap_or("").trim();
        if owner.is_empty() || rest.is_empty() {
            return None;
        }
        if owner != source_workspace_id {
            return None;
        }
        value = rest.to_string();
    } else if let Some(stripped) = value.strip_prefix("/workspace/") {
        value = stripped.trim_matches('/').to_string();
    } else if let Some(stripped) = value.strip_prefix("workspace/") {
        value = stripped.trim_matches('/').to_string();
    }
    while let Some(stripped) = value.strip_prefix("./") {
        value = stripped.to_string();
    }
    value = value.trim_start_matches('/').trim().to_string();
    if value.is_empty() {
        return None;
    }
    if !looks_like_user_world_file_ref(raw, &value) {
        return None;
    }
    let candidate = Path::new(&value);
    for component in candidate.components() {
        match component {
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => return None,
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Some(value)
}

fn extract_user_world_file_refs(
    content: &str,
    source_workspace_id: &str,
) -> Vec<UserWorldFileRefMatch> {
    let mut items = Vec::new();
    for captures in user_world_file_ref_regex().captures_iter(content) {
        let Some(token_match) = captures.get(2) else {
            continue;
        };
        let token = token_match.as_str();
        if token.trim().is_empty() {
            continue;
        }
        let wrapped_in_quotes = ((token.starts_with('"') && token.ends_with('"'))
            || (token.starts_with('\'') && token.ends_with('\'')))
            && token.len() >= 2;
        let (raw_path, suffix) = if wrapped_in_quotes {
            (&token[1..token.len().saturating_sub(1)], "")
        } else {
            split_user_world_file_ref_suffix(token)
        };
        let Some(normalized_path) =
            normalize_user_world_file_ref_path(raw_path, source_workspace_id)
        else {
            continue;
        };
        items.push(UserWorldFileRefMatch {
            token_start: token_match.start(),
            token_end: token_match.end(),
            normalized_path,
            suffix: suffix.to_string(),
        });
    }
    items
}

fn copy_user_world_staged_path(source: &Path, destination: &Path) -> Result<()> {
    if source.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in WalkDir::new(source).min_depth(1) {
            let entry = entry?;
            if entry.file_type().is_symlink() {
                return Err(anyhow!(
                    "symbolic links are not supported in user_world file refs"
                ));
            }
            let relative = entry.path().strip_prefix(source).unwrap_or(entry.path());
            let target = destination.join(relative);
            if entry.file_type().is_dir() {
                fs::create_dir_all(&target)?;
            } else if entry.file_type().is_file() {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(entry.path(), &target)?;
            }
        }
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    Ok(())
}

fn stage_user_world_file_refs(
    context: &ToolContext<'_>,
    content: &str,
) -> Result<(String, Vec<UserWorldCopiedFile>)> {
    let matches = extract_user_world_file_refs(content, context.workspace_id);
    if matches.is_empty() {
        return Ok((content.to_string(), Vec::new()));
    }
    let source_workspace_id = context.workspace_id;
    let sender_workspace_id = context.workspace.scoped_user_id(context.user_id, None);
    let source_root = context.workspace.ensure_user_root(source_workspace_id)?;
    let _ = context.workspace.ensure_user_root(&sender_workspace_id)?;
    let transfer_id = format!(
        "{}_{}",
        Utc::now().format("%Y%m%d%H%M%S"),
        Uuid::new_v4().simple()
    );
    let mut staged_path_map = HashMap::<String, UserWorldCopiedFile>::new();
    for item in &matches {
        if staged_path_map.contains_key(&item.normalized_path) {
            continue;
        }
        let source_target = context
            .workspace
            .resolve_path(source_workspace_id, &item.normalized_path)?;
        if !source_target.exists() {
            return Err(anyhow!(
                "user_world file ref not found in workspace: {}",
                item.normalized_path
            ));
        }
        if !is_within_root(&source_root, &source_target) {
            return Err(anyhow!(
                "user_world file ref is outside workspace: {}",
                item.normalized_path
            ));
        }
        let source_meta = fs::symlink_metadata(&source_target)?;
        if source_meta.file_type().is_symlink() {
            return Err(anyhow!(
                "symbolic links are not supported in user_world file refs: {}",
                item.normalized_path
            ));
        }
        let staged_path = format!(
            "{}/{}/{}",
            USER_WORLD_FILE_STAGING_DIR, transfer_id, item.normalized_path
        );
        let destination = context
            .workspace
            .resolve_path(&sender_workspace_id, &staged_path)?;
        copy_user_world_staged_path(&source_target, &destination)?;
        staged_path_map.insert(
            item.normalized_path.clone(),
            UserWorldCopiedFile {
                source_path: item.normalized_path.clone(),
                staged_path,
                entry_type: if source_target.is_dir() {
                    "dir"
                } else {
                    "file"
                },
            },
        );
    }
    let mut rewritten = String::with_capacity(content.len() + matches.len() * 32);
    let mut cursor = 0usize;
    for item in &matches {
        let Some(staged) = staged_path_map.get(&item.normalized_path) else {
            continue;
        };
        rewritten.push_str(&content[cursor..item.token_start]);
        let quoted_path = staged.staged_path.replace('"', "%22");
        rewritten.push('"');
        rewritten.push_str(&quoted_path);
        rewritten.push('"');
        rewritten.push_str(&item.suffix);
        cursor = item.token_end;
    }
    rewritten.push_str(&content[cursor..]);
    let mut copied = staged_path_map.into_values().collect::<Vec<_>>();
    copied.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    Ok((rewritten, copied))
}

async fn user_world_send_message(
    context: &ToolContext<'_>,
    payload: &UserWorldToolArgs,
) -> Result<Value> {
    let user_world = context
        .user_world
        .as_ref()
        .ok_or_else(|| anyhow!(i18n::t("error.internal_error")))?;
    let sender = context.user_id.trim();
    if sender.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let content = payload.content.as_deref().unwrap_or("").trim();
    if content.is_empty() {
        return Err(anyhow!("content is required"));
    }
    let content_type = payload
        .content_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("text");
    let mut raw_targets = Vec::new();
    if let Some(user_id) = payload.user_id.as_deref() {
        raw_targets.push(user_id.to_string());
    }
    if let Some(user_ids) = payload.user_ids.as_ref() {
        raw_targets.extend(user_ids.iter().map(|value| value.to_string()));
    }
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for raw in raw_targets {
        let cleaned = raw.trim();
        if cleaned.is_empty() {
            continue;
        }
        if seen.insert(cleaned.to_string()) {
            targets.push(cleaned.to_string());
        }
    }
    if targets.is_empty() {
        return Err(anyhow!("user_id or user_ids required"));
    }
    let user_store = UserStore::new(context.storage.clone());
    let mut target_exists = HashMap::new();
    let mut has_valid_target = false;
    for target in &targets {
        if target == sender {
            continue;
        }
        let exists = user_store.get_user_by_id(target)?.is_some();
        if exists {
            has_valid_target = true;
        }
        target_exists.insert(target.to_string(), exists);
    }
    let (content, copied_files) = if has_valid_target {
        stage_user_world_file_refs(context, content)?
    } else {
        (content.to_string(), Vec::new())
    };
    let mut results = Vec::new();
    for target in targets {
        if target == sender {
            results.push(json!({
                "user_id": target,
                "ok": false,
                "error": "cannot send to self"
            }));
            continue;
        }
        if !target_exists.get(&target).copied().unwrap_or(false) {
            results.push(json!({
                "user_id": target,
                "ok": false,
                "error": "user not found"
            }));
            continue;
        }
        let now = now_ts();
        let conversation =
            match user_world.resolve_or_create_direct_conversation(sender, &target, now) {
                Ok(value) => value,
                Err(err) => {
                    results.push(json!({
                        "user_id": target,
                        "ok": false,
                        "error": err.to_string()
                    }));
                    continue;
                }
            };
        let send_result = match user_world
            .send_message(
                sender,
                &conversation.conversation_id,
                &content,
                content_type,
                payload.client_msg_id.as_deref(),
                now,
            )
            .await
        {
            Ok(value) => value,
            Err(err) => {
                results.push(json!({
                    "user_id": target,
                    "ok": false,
                    "error": err.to_string()
                }));
                continue;
            }
        };
        results.push(json!({
            "user_id": target,
            "ok": true,
            "conversation_id": conversation.conversation_id,
            "message_id": send_result.message.message_id,
            "inserted": send_result.inserted
        }));
    }
    Ok(build_model_tool_success(
        "send_message",
        "completed",
        format!("Processed {} user world message deliveries.", results.len()),
        json!({
            "results": results,
            "staged_files": copied_files.iter().map(|item| {
                json!({
                    "source_path": item.source_path,
                    "staged_path": item.staged_path,
                    "entry_type": item.entry_type
                })
            }).collect::<Vec<_>>()
        }),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeInvokeAction {
    List,
    Invoke,
}

#[derive(Debug, Deserialize)]
struct NodeInvokeArgs {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    timeout_s: Option<f64>,
    #[serde(default)]
    metadata: Option<Value>,
}

async fn execute_node_invoke(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: NodeInvokeArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    match resolve_node_invoke_action(&payload)? {
        NodeInvokeAction::List => execute_node_list(context).await,
        NodeInvokeAction::Invoke => execute_node_invoke_action(context, payload).await,
    }
}

fn resolve_node_invoke_action(payload: &NodeInvokeArgs) -> Result<NodeInvokeAction> {
    if let Some(action) = payload.action.as_deref() {
        let action = action.trim();
        if action.is_empty() {
            return Err(anyhow!("节点调用 action 不能为空"));
        }
        let normalized = action.to_ascii_lowercase();
        return match normalized.as_str() {
            "list" | "ls" | "列表" | "列出" => Ok(NodeInvokeAction::List),
            "invoke" | "call" | "调用" => Ok(NodeInvokeAction::Invoke),
            _ => Err(anyhow!("未知节点调用 action: {action}")),
        };
    }
    let has_node_id = payload
        .node_id
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_command = payload
        .command
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    if has_node_id && has_command {
        Ok(NodeInvokeAction::Invoke)
    } else {
        Err(anyhow!(
            "节点调用缺少 action，支持 list/invoke；兼容模式下需提供 node_id 与 command"
        ))
    }
}

async fn execute_node_list(context: &ToolContext<'_>) -> Result<Value> {
    let gateway = context
        .gateway
        .clone()
        .ok_or_else(|| anyhow!("gateway not available"))?;
    let snapshot = gateway.snapshot().await;
    let mut nodes = Vec::new();
    for item in snapshot.items {
        if !item.role.eq_ignore_ascii_case("node") {
            continue;
        }
        let Some(node_id) = normalize_optional_string(item.node_id) else {
            continue;
        };
        nodes.push(json!({
            "node_id": node_id,
            "connection_id": item.connection_id,
            "scopes": item.scopes,
            "caps": item.caps,
            "commands": item.commands,
            "connected_at": item.connected_at,
            "last_seen_at": item.last_seen_at,
            "client": item.client
        }));
    }
    nodes.sort_by(|left, right| {
        let left_node = left.get("node_id").and_then(Value::as_str).unwrap_or("");
        let right_node = right.get("node_id").and_then(Value::as_str).unwrap_or("");
        left_node.cmp(right_node).then_with(|| {
            let left_connection = left
                .get("connection_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let right_connection = right
                .get("connection_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            left_connection.cmp(right_connection)
        })
    });
    Ok(build_model_tool_success(
        "list",
        "completed",
        format!("Listed {} gateway nodes.", nodes.len()),
        json!({
            "state_version": snapshot.state_version,
            "count": nodes.len(),
            "nodes": nodes
        }),
    ))
}

async fn execute_node_invoke_action(
    context: &ToolContext<'_>,
    payload: NodeInvokeArgs,
) -> Result<Value> {
    let gateway = context
        .gateway
        .clone()
        .ok_or_else(|| anyhow!("gateway not available"))?;
    let node_id = normalize_optional_string(payload.node_id)
        .ok_or_else(|| anyhow!("节点调用 invoke 需要 node_id"))?;
    let command = normalize_optional_string(payload.command)
        .ok_or_else(|| anyhow!("节点调用 invoke 需要 command"))?;
    let timeout_s = payload.timeout_s.unwrap_or(30.0);
    let result = gateway
        .invoke_node(GatewayNodeInvokeRequest {
            node_id: node_id.clone(),
            command: command.clone(),
            args: payload.args,
            timeout_s,
            metadata: payload.metadata,
        })
        .await?;
    if result.ok {
        Ok(build_model_tool_success(
            "invoke",
            "completed",
            format!("Invoked command {command} on node {node_id}."),
            json!({
                "node_id": node_id,
                "command": command,
                "result": result.payload
            }),
        ))
    } else {
        let message = result
            .error
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("node invoke failed");
        Err(anyhow!(message.to_string()))
    }
}

#[derive(Debug, Deserialize)]
struct SessionListArgs {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "activeMinutes", alias = "active_minutes")]
    active_minutes: Option<f64>,
    #[serde(default, rename = "messageLimit", alias = "message_limit")]
    message_limit: Option<i64>,
    #[serde(
        default,
        alias = "parent_id",
        alias = "parentId",
        alias = "parentSessionId"
    )]
    parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionHistoryArgs {
    #[serde(
        default,
        alias = "session_id",
        alias = "sessionId",
        alias = "sessionKey",
        alias = "session_key"
    )]
    session_key: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "includeTools", alias = "include_tools")]
    include_tools: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SessionSendArgs {
    #[serde(
        default,
        alias = "session_id",
        alias = "sessionId",
        alias = "sessionKey",
        alias = "session_key"
    )]
    session_key: Option<String>,
    message: String,
    #[serde(default, rename = "timeoutSeconds", alias = "timeout_seconds")]
    timeout_seconds: Option<f64>,
    #[serde(
        default,
        rename = "announceParentSessionId",
        alias = "announce_parent_session_id"
    )]
    announce_parent_session_id: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(
        default,
        rename = "announcePersistHistory",
        alias = "announce_persist_history"
    )]
    announce_persist_history: Option<bool>,
    #[serde(
        default,
        rename = "announceEmitParentEvents",
        alias = "announce_emit_parent_events"
    )]
    announce_emit_parent_events: Option<bool>,
    #[serde(default, rename = "waitForever", alias = "wait_forever")]
    wait_forever: Option<bool>,
    #[serde(default, rename = "teamTaskId", alias = "team_task_id")]
    team_task_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawSessionSpawnArgs {
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default, rename = "agentName", alias = "agent_name", alias = "name")]
    agent_name: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "runTimeoutSeconds", alias = "run_timeout_seconds")]
    run_timeout_seconds: Option<f64>,
    #[serde(default)]
    cleanup: Option<String>,
    #[serde(default, rename = "threadStrategy", alias = "thread_strategy")]
    thread_strategy: Option<String>,
    #[serde(default, rename = "reuseMainThread", alias = "reuse_main_thread")]
    reuse_main_thread: Option<bool>,
}

#[derive(Debug)]
struct SessionSpawnArgs {
    task: String,
    label: Option<String>,
    agent_id: Option<String>,
    agent_name: Option<String>,
    model: Option<String>,
    run_timeout_seconds: Option<f64>,
    cleanup: Option<String>,
    thread_strategy: Option<String>,
    reuse_main_thread: Option<bool>,
}

impl<'de> Deserialize<'de> for SessionSpawnArgs {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawSessionSpawnArgs::deserialize(deserializer)?;
        let task = raw
            .task
            .or(raw.message)
            .or(raw.prompt)
            .ok_or_else(|| serde::de::Error::missing_field("task"))?;
        Ok(Self {
            task,
            label: raw.label,
            agent_id: raw.agent_id,
            agent_name: raw.agent_name,
            model: raw.model,
            run_timeout_seconds: raw.run_timeout_seconds,
            cleanup: raw.cleanup,
            thread_strategy: raw.thread_strategy,
            reuse_main_thread: raw.reuse_main_thread,
        })
    }
}

#[derive(Debug)]
struct SessionRunOutcome {
    status: String,
    answer: Option<String>,
    error: Option<String>,
    elapsed_s: f64,
}

#[derive(Debug, Clone, Default)]
struct SessionRunMeta {
    dispatch_id: Option<String>,
    run_kind: Option<String>,
    requested_by: Option<String>,
    team_task_id: Option<String>,
    metadata: Option<Value>,
}

#[derive(Debug, Clone, Copy)]
enum SwarmWaitMode {
    Immediate,
    Finite(f64),
    Infinite,
}

#[derive(Debug, Clone)]
struct PreparedChildSession {
    child_session_id: String,
    child_agent_id: Option<String>,
    model_name: Option<String>,
    request: WunderRequest,
    announce: AnnounceConfig,
    run_metadata: Value,
}

#[derive(Clone, Copy)]
enum SessionCleanup {
    Keep,
    Delete,
}

#[derive(Clone, Copy)]
enum ChildSessionToolMode {
    InheritParentSession,
    UseTargetAgentDefaults,
}

#[derive(Debug, Clone)]
struct AnnounceConfig {
    parent_session_id: String,
    label: Option<String>,
    dispatch_id: Option<String>,
    strategy: Option<String>,
    completion_mode: Option<String>,
    remaining_action: Option<String>,
    parent_turn_ref: Option<String>,
    parent_user_round: Option<i64>,
    parent_model_round: Option<i64>,
    emit_parent_events: bool,
    auto_wake: bool,
    persist_history_message: bool,
}

const MAX_SUBAGENT_SESSION_DEPTH: usize = 32;

fn session_cleanup_label(cleanup: SessionCleanup) -> &'static str {
    match cleanup {
        SessionCleanup::Keep => "keep",
        SessionCleanup::Delete => "delete",
    }
}

fn child_session_depth(
    storage: &dyn StorageBackend,
    user_id: &str,
    parent_session_id: &str,
) -> i64 {
    let cleaned_user = user_id.trim();
    let cleaned_parent = parent_session_id.trim();
    if cleaned_user.is_empty() || cleaned_parent.is_empty() {
        return 1;
    }
    let mut depth = 0_i64;
    let mut cursor = Some(cleaned_parent.to_string());
    let mut seen = HashSet::new();
    while let Some(session_id) = cursor {
        if !seen.insert(session_id.clone()) || depth >= MAX_SUBAGENT_SESSION_DEPTH as i64 {
            break;
        }
        let Some(session) = storage
            .get_chat_session(cleaned_user, &session_id)
            .ok()
            .flatten()
        else {
            break;
        };
        let Some(next_parent) = session
            .parent_session_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            break;
        };
        depth += 1;
        cursor = Some(next_parent.to_string());
    }
    depth + 1
}

fn should_auto_wake_parent_after_child_run(wait_forever: bool, timeout_seconds: f64) -> bool {
    !wait_forever && timeout_seconds <= 0.0
}

fn should_auto_wake_parent_follow_up(
    is_swarm_task: bool,
    wait_forever: bool,
    timeout_seconds: f64,
) -> bool {
    !is_swarm_task && should_auto_wake_parent_after_child_run(wait_forever, timeout_seconds)
}

fn sync_announce_auto_wake(
    announce: &mut AnnounceConfig,
    run_metadata: Option<&mut Value>,
    auto_wake: bool,
) {
    announce.auto_wake = auto_wake;
    if let Some(metadata) = run_metadata {
        insert_run_metadata_field(metadata, "auto_wake", json!(auto_wake));
    }
}

fn build_parent_follow_up_announce(
    parent_session_id: Option<String>,
    child_session_id: &str,
    label: Option<String>,
    emit_parent_events: bool,
    persist_history_message: bool,
    auto_wake: bool,
    parent_turn_ref: Option<String>,
    parent_user_round: Option<i64>,
    parent_model_round: Option<i64>,
) -> Option<AnnounceConfig> {
    let child_session_id = child_session_id.trim();
    let parent_session_id = normalize_optional_string(parent_session_id)
        .filter(|parent_session_id| parent_session_id != child_session_id)?;
    Some(AnnounceConfig {
        parent_session_id,
        label,
        dispatch_id: None,
        strategy: None,
        completion_mode: None,
        remaining_action: None,
        parent_turn_ref,
        parent_user_round,
        parent_model_round,
        emit_parent_events,
        auto_wake,
        persist_history_message,
    })
}

fn subagent_control_scope(tool_names: &[String]) -> &'static str {
    if tool_names
        .iter()
        .any(|name| resolve_tool_name(name) == "子智能体控制")
    {
        "children"
    } else {
        "none"
    }
}

fn subagent_role_for_scope(control_scope: &str) -> &'static str {
    if control_scope == "children" {
        "orchestrator"
    } else {
        "worker"
    }
}

fn build_prepared_child_run_metadata(
    context: &ToolContext<'_>,
    parent_session_id: &str,
    parent_tool_names: &[String],
    parent_turn_ref: Option<&str>,
) -> Value {
    let depth = child_session_depth(context.storage.as_ref(), context.user_id, parent_session_id);
    let control_scope = subagent_control_scope(parent_tool_names);
    json!({
        "controller_session_id": parent_session_id.trim(),
        "parent_turn_ref": parent_turn_ref.map(str::to_string),
        "parent_user_round": context.user_round,
        "parent_model_round": context.model_round,
        "depth": depth,
        "role": subagent_role_for_scope(control_scope),
        "control_scope": control_scope,
        "auto_wake": false,
        "emit_parent_events": true,
    })
}

fn insert_run_metadata_field(target: &mut Value, key: &str, value: Value) {
    let Some(object) = target.as_object_mut() else {
        return;
    };
    object.insert(key.to_string(), value);
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SubagentControlArgs {
    action: String,
}

async fn subagent_control(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    self::subagent_control::execute(context, args).await
}

#[allow(dead_code)]
async fn subagent_control_legacy(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentControlArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = payload.action.trim();
    if action.is_empty() {
        return Err(anyhow!("子智能体控制 action 不能为空"));
    }
    let action_lower = action.to_lowercase();
    match action_lower.as_str() {
        "list" | "sessions_list" | "session_list" | "会话列表" | "列表" => {
            sessions_list(context, args).await
        }
        "history" | "sessions_history" | "session_history" | "会话历史" | "历史" => {
            sessions_history(context, args).await
        }
        "send" | "sessions_send" | "session_send" | "会话发送" | "发送" => {
            sessions_send(context, args).await
        }
        "spawn" | "sessions_spawn" | "session_spawn" | "会话派生" | "派生" => {
            sessions_spawn(context, args).await
        }
        _ => Err(anyhow!("未知子智能体控制 action: {action}")),
    }
}

#[derive(Debug, Deserialize)]
struct AgentSwarmControlArgs {
    action: String,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmListArgs {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "activeMinutes", alias = "active_minutes")]
    active_minutes: Option<f64>,
    #[serde(default, rename = "includeCurrent", alias = "include_current")]
    include_current: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmStatusArgs {
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default, rename = "agentName", alias = "agent_name", alias = "name")]
    agent_name: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "includeCurrent", alias = "include_current")]
    include_current: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmSendArgs {
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default, rename = "agentName", alias = "agent_name", alias = "name")]
    agent_name: Option<String>,
    #[serde(
        default,
        alias = "session_id",
        alias = "sessionId",
        alias = "sessionKey",
        alias = "session_key"
    )]
    session_key: Option<String>,
    message: String,
    #[serde(default, rename = "threadStrategy", alias = "thread_strategy")]
    thread_strategy: Option<String>,
    #[serde(default, rename = "reuseMainThread", alias = "reuse_main_thread")]
    reuse_main_thread: Option<bool>,
    #[serde(default, rename = "timeoutSeconds", alias = "timeout_seconds")]
    timeout_seconds: Option<f64>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, rename = "includeCurrent", alias = "include_current")]
    include_current: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmBatchTaskArgs {
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default, rename = "agentName", alias = "agent_name", alias = "name")]
    agent_name: Option<String>,
    #[serde(
        default,
        alias = "session_id",
        alias = "sessionId",
        alias = "sessionKey",
        alias = "session_key"
    )]
    session_key: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default, rename = "threadStrategy", alias = "thread_strategy")]
    thread_strategy: Option<String>,
    #[serde(default, rename = "reuseMainThread", alias = "reuse_main_thread")]
    reuse_main_thread: Option<bool>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, rename = "includeCurrent", alias = "include_current")]
    include_current: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmBatchSendArgs {
    #[serde(default)]
    tasks: Vec<AgentSwarmBatchTaskArgs>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default, rename = "threadStrategy", alias = "thread_strategy")]
    thread_strategy: Option<String>,
    #[serde(default, rename = "reuseMainThread", alias = "reuse_main_thread")]
    reuse_main_thread: Option<bool>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, rename = "waitSeconds", alias = "wait_seconds")]
    wait_seconds: Option<f64>,
    #[serde(
        default,
        rename = "pollIntervalSeconds",
        alias = "poll_interval_seconds"
    )]
    poll_interval_seconds: Option<f64>,
    #[serde(default, rename = "includeCurrent", alias = "include_current")]
    include_current: Option<bool>,
    #[serde(default, rename = "teamRunId", alias = "team_run_id")]
    team_run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmWaitArgs {
    #[serde(default, rename = "runIds", alias = "run_ids")]
    run_ids: Option<Vec<String>>,
    #[serde(default, alias = "runId", alias = "run_id")]
    run_id: Option<String>,
    #[serde(default, rename = "waitSeconds", alias = "wait_seconds")]
    wait_seconds: Option<f64>,
    #[serde(
        default,
        rename = "pollIntervalSeconds",
        alias = "poll_interval_seconds"
    )]
    poll_interval_seconds: Option<f64>,
}

#[derive(Debug, Clone)]
struct SwarmRunSnapshot {
    status: String,
    terminal: bool,
    failed: bool,
    payload: Value,
}

#[derive(Debug, Default, Clone)]
struct AgentSwarmRuntime {
    lock_sessions: HashSet<String>,
    running_sessions: HashSet<String>,
}

#[derive(Debug, Clone)]
struct SwarmBatchDispatchTask {
    index: usize,
    team_task_id: String,
    message: String,
    label: Option<String>,
    agent_id: String,
    agent_name: String,
    session_id: String,
    created_session: bool,
    thread_strategy: &'static str,
    tool_names: Vec<String>,
    model_name: Option<String>,
    agent_prompt: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwarmWorkerThreadStrategy {
    FreshMainThread,
    MainThread,
}

impl SwarmWorkerThreadStrategy {
    fn as_tool_value(self) -> &'static str {
        match self {
            Self::FreshMainThread => "fresh_main_thread",
            Self::MainThread => "main_thread",
        }
    }
}

fn parse_swarm_worker_thread_strategy(
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

fn resolve_swarm_batch_tool_names(
    context: &ToolContext<'_>,
    config: &Config,
    skills: &SkillRegistry,
    allowed_tools: &HashSet<String>,
    user_id: &str,
    session: &ChatSessionRecord,
    agent: &UserAgentRecord,
) -> Vec<String> {
    let frozen_tool_overrides = context
        .workspace
        .load_session_frozen_tool_overrides(user_id, &session.session_id);
    let overrides =
        resolve_session_tool_overrides(session, frozen_tool_overrides.as_deref(), Some(agent));
    let filtered = apply_tool_overrides(allowed_tools.clone(), &overrides, config, skills);
    finalize_tool_names(filtered)
}

async fn dispatch_swarm_batch_task(
    context: &ToolContext<'_>,
    task: SwarmBatchDispatchTask,
) -> Result<Value> {
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }

    let model_name = task.model_name.clone();
    let request = WunderRequest {
        user_id: user_id.to_string(),
        question: task.message,
        tool_names: task.tool_names,
        skip_tool_calls: false,
        stream: false,
        debug_payload: false,
        session_id: Some(task.session_id.clone()),
        agent_id: Some(task.agent_id.clone()),
        model_name: model_name.clone(),
        language: Some(i18n::get_language()),
        config_overrides: context.request_config_overrides.cloned(),
        agent_prompt: task.agent_prompt,
        attachments: None,
        allow_queue: true,
        is_admin: context.is_admin,
        approval_tx: None,
    };

    // Swarm tasks are reported via team_* realtime/timeline events.
    // Also emit subagent_dispatch_item_update so the frontend sub-agent panel refreshes.
    // Emit subagent_dispatch_item_update so the frontend sub-agent panel refreshes
    // without polluting the queen bee's chat transcript.
    let _task_label = task.label.as_deref();
    let announce = Some(AnnounceConfig {
        parent_session_id: context.session_id.to_string(),
        label: task.label.clone(),
        dispatch_id: None,
        strategy: None,
        completion_mode: None,
        remaining_action: None,
        parent_turn_ref: None,
        parent_user_round: None,
        parent_model_round: None,
        emit_parent_events: true,
        auto_wake: false,
        persist_history_message: false,
    });

    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let _receiver = spawn_session_run(
        context,
        request,
        run_id.clone(),
        Some(context.session_id.to_string()),
        Some(task.agent_id.clone()),
        model_name,
        SessionRunMeta {
            run_kind: Some("swarm".to_string()),
            requested_by: Some("agent_swarm".to_string()),
            team_task_id: Some(task.team_task_id.clone()),
            ..SessionRunMeta::default()
        },
        announce,
        SessionCleanup::Keep,
        None,
    )
    .await?;

    Ok(build_model_tool_success(
        "spawn",
        "accepted",
        format!("Spawned swarm task {}.", task.team_task_id),
        json!({
            "task_id": task.team_task_id,
            "run_id": run_id,
            "session_id": task.session_id,
            "agent_id": task.agent_id,
            "agent_name": task.agent_name,
            "created_session": task.created_session,
            "thread_strategy": task.thread_strategy,
        }),
    ))
}

fn claim_swarm_mother_for_context(
    context: &ToolContext<'_>,
    user_id: &str,
    hive_id: &str,
) -> Result<Option<String>> {
    let Some(agent_id) = current_agent_id(context) else {
        return Ok(None);
    };
    let mother_agent_id =
        claim_swarm_mother_agent(context.storage.as_ref(), user_id, hive_id, &agent_id)?;
    Ok(Some(mother_agent_id))
}

fn create_swarm_team_run_record(
    context: &ToolContext<'_>,
    user_id: &str,
    hive_id: &str,
    mother_agent_id: Option<String>,
    team_run_id_override: Option<&str>,
    strategy: &str,
    task_total: usize,
) -> TeamRunRecord {
    let now = now_ts();
    let team_run_id = team_run_id_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("team_{}", Uuid::new_v4().simple()));
    TeamRunRecord {
        team_run_id,
        user_id: user_id.to_string(),
        hive_id: hive_id.to_string(),
        parent_session_id: context.session_id.to_string(),
        parent_agent_id: current_agent_id(context),
        mother_agent_id,
        strategy: strategy.to_string(),
        status: "queued".to_string(),
        task_total: task_total as i64,
        task_success: 0,
        task_failed: 0,
        context_tokens_total: 0,
        context_tokens_peak: 0,
        model_round_total: 0,
        started_time: Some(now),
        finished_time: None,
        elapsed_s: None,
        summary: None,
        error: None,
        updated_time: now,
    }
}

fn create_swarm_team_task_record(
    run: &TeamRunRecord,
    agent_id: &str,
    target_session_id: Option<String>,
    spawned_session_id: Option<String>,
    priority: i64,
) -> TeamTaskRecord {
    let now = now_ts();
    TeamTaskRecord {
        task_id: format!("task_{}", Uuid::new_v4().simple()),
        team_run_id: run.team_run_id.clone(),
        user_id: run.user_id.clone(),
        hive_id: run.hive_id.clone(),
        agent_id: agent_id.to_string(),
        target_session_id,
        spawned_session_id,
        session_run_id: None,
        status: "queued".to_string(),
        retry_count: 0,
        priority,
        started_time: None,
        finished_time: None,
        elapsed_s: None,
        result_summary: None,
        error: None,
        updated_time: now,
    }
}

#[allow(dead_code)]
fn build_swarm_timeout_monitoring_payload(
    run_id: Option<&str>,
    team_run_id: &str,
    target_session_id: &str,
) -> Value {
    let run_id = run_id.map(str::trim).filter(|value| !value.is_empty());
    let mut suggestions = Vec::new();
    if let Some(run_id) = run_id {
        suggestions.push(json!({
            "purpose": "继续等待 60 秒",
            "args": {
                "action": "wait",
                "run_ids": [run_id],
                "wait_seconds": 60
            }
        }));
        suggestions.push(json!({
            "purpose": "立即查看当前快照",
            "args": {
                "action": "wait",
                "run_ids": [run_id],
                "wait_seconds": 0
            }
        }));
    }
    let session_key = target_session_id.trim();
    if !session_key.is_empty() {
        suggestions.push(json!({
            "purpose": "查看工蜂会话历史",
            "args": {
                "action": "history",
                "session_id": session_key
            }
        }));
    }
    json!({
        "note": "本次仅等待超时，工蜂可能仍在执行。",
        "team_run_id": team_run_id,
        "run_id": run_id,
        "session_id": if session_key.is_empty() { Value::Null } else { json!(session_key) },
        "suggested_calls": suggestions
    })
}

#[allow(dead_code)]
fn build_swarm_wait_monitoring_payload(run_ids: &[String]) -> Value {
    let run_ids = dedupe_non_empty_strings(run_ids.to_vec());
    json!({
        "note": "任务尚未全部完成，可继续等待或先查看快照。",
        "run_ids": run_ids,
        "suggested_calls": [
            {
                "purpose": "继续等待 60 秒",
                "args": {
                    "action": "wait",
                    "run_ids": run_ids,
                    "wait_seconds": 60
                }
            },
            {
                "purpose": "立即查看当前快照",
                "args": {
                    "action": "wait",
                    "run_ids": run_ids,
                    "wait_seconds": 0
                }
            }
        ]
    })
}

fn resolve_swarm_wait_mode(
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

fn swarm_wait_seconds_value(wait_mode: SwarmWaitMode) -> Option<f64> {
    match wait_mode {
        SwarmWaitMode::Immediate => Some(0.0),
        SwarmWaitMode::Finite(timeout_s) => Some(timeout_s),
        SwarmWaitMode::Infinite => None,
    }
}

fn is_swarm_task_terminal_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "success" | "error" | "failed" | "timeout" | "cancelled"
    )
}

async fn agent_swarm(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmControlArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "unknown",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm arguments are invalid: {err}"),
                "请先提供 action，并只使用 list、status、send、history、spawn、batch_send、wait 之一。",
                &["action"],
                json!({ "action": "list" }),
                args,
                json!({
                    "allowed_actions": ["list", "status", "send", "history", "spawn", "batch_send", "wait"]
                }),
            ));
        }
    };
    let action = payload.action.trim();
    if action.is_empty() {
        return Ok(build_agent_swarm_args_failure(
            "unknown",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm action cannot be empty",
            "请先提供 action，并只使用 list、status、send、history、spawn、batch_send、wait 之一。",
            &["action"],
            json!({ "action": "list" }),
            args,
            json!({
                "allowed_actions": ["list", "status", "send", "history", "spawn", "batch_send", "wait"]
            }),
        ));
    }
    let action_lower = action.to_lowercase();
    match action_lower.as_str() {
        "list" | "agents_list" | "agent_list" | "swarm_list" => {
            agent_swarm_list(context, args).await
        }
        "status" | "agent_status" | "agents_status" | "swarm_status" => {
            agent_swarm_status(context, args).await
        }
        "send" | "agent_send" | "agents_send" | "swarm_send" => {
            agent_swarm_send(context, args).await
        }
        "batch_send" | "swarm_batch_send" | "agents_batch_send" | "batch" | "fanout"
        | "dispatch" => agent_swarm_batch_send(context, args).await,
        "wait" | "join" | "collect" | "swarm_wait" => agent_swarm_wait(context, args).await,
        "history" | "agent_history" | "agents_history" | "swarm_history" => {
            agent_swarm_history(context, args).await
        }
        "spawn" | "agent_spawn" | "agents_spawn" | "swarm_spawn" => {
            agent_swarm_spawn(context, args).await
        }
        _ => Ok(build_agent_swarm_args_failure(
            action,
            "TOOL_ARGS_INVALID",
            format!("unknown agent_swarm action: {action}"),
            "请改用 list、status、send、history、spawn、batch_send、wait 之一，并严格按对应字段传参。",
            &["action"],
            json!({ "action": "list" }),
            args,
            json!({
                "allowed_actions": ["list", "status", "send", "history", "spawn", "batch_send", "wait"]
            }),
        )),
    }
}

async fn agent_swarm_list(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmListArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let limit = clamp_limit(payload.limit, 50, MAX_SESSION_LIST_ITEMS);
    let include_current = payload.include_current.unwrap_or(false);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let cutoff = payload
        .active_minutes
        .filter(|value| *value > 0.0)
        .map(|value| now_ts() - value * 60.0);
    let runtime_map = collect_swarm_runtime(context, user_id)?;
    let mut items = Vec::new();
    for agent in collect_swarm_agents(context, user_id, include_current, &swarm_hive_id)? {
        let runtime = runtime_map.get(&agent.agent_id);
        let (sessions, session_total) =
            context
                .storage
                .list_chat_sessions(user_id, Some(&agent.agent_id), None, 0, 1)?;
        let latest = sessions.first();
        if let Some(cutoff) = cutoff {
            let latest_updated = latest.map(|record| record.updated_at).unwrap_or(0.0);
            let active_count = merge_swarm_active_sessions(runtime).len();
            if latest_updated < cutoff && active_count == 0 {
                continue;
            }
        }
        let active_session_ids = merge_swarm_active_sessions(runtime);
        let last_status =
            latest.and_then(|record| monitor_session_status(context, &record.session_id));
        items.push(json!({
            "agent_id": agent.agent_id,
            "hive_id": normalize_hive_id(&agent.hive_id),
            "name": agent.name,
            "description": agent.description,
            "status": agent.status,
            "is_shared": agent.is_shared,
            "access_level": agent.access_level,
            "updated_at": format_ts(agent.updated_at),
            "session_total": session_total,
            "active_session_total": active_session_ids.len(),
            "running_session_total": runtime.map(|entry| entry.running_sessions.len()).unwrap_or(0),
            "lock_session_total": runtime.map(|entry| entry.lock_sessions.len()).unwrap_or(0),
            "active_session_ids": active_session_ids,
            "last_session_id": latest.map(|record| record.session_id.clone()),
            "last_message_at": latest.map(|record| format_ts(record.last_message_at)),
            "last_session_status": last_status,
        }));
        if items.len() as i64 >= limit {
            break;
        }
    }
    Ok(build_model_tool_success(
        "list",
        "completed",
        format!("Listed {} swarm workers.", items.len()),
        json!({ "total": items.len(), "items": items }),
    ))
}

async fn agent_swarm_status(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmStatusArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let requested_agent_id = normalize_optional_string(payload.agent_id);
    let requested_agent_name = normalize_optional_string(payload.agent_name);
    if requested_agent_id.is_none() && requested_agent_name.is_none() {
        return agent_swarm_list(context, args).await;
    }
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let include_current = payload.include_current.unwrap_or(false);
    let current_agent_id = current_agent_id(context);
    let Some(agent) = resolve_swarm_agent_record(
        context.storage.as_ref(),
        user_id,
        current_agent_id.as_deref(),
        include_current,
        &swarm_hive_id,
        requested_agent_id.as_deref(),
        requested_agent_name.as_deref(),
    )?
    else {
        return agent_swarm_list(context, args).await;
    };
    let agent_id = agent.agent_id.clone();
    let limit = clamp_limit(payload.limit, 20, MAX_SESSION_LIST_ITEMS);
    let runtime_map = collect_swarm_runtime(context, user_id)?;
    let runtime = runtime_map.get(&agent_id);
    let active_session_ids = merge_swarm_active_sessions(runtime);
    let active_set: HashSet<String> = active_session_ids.iter().cloned().collect();
    let (sessions, session_total) =
        context
            .storage
            .list_chat_sessions(user_id, Some(&agent_id), None, 0, limit)?;
    let mut recent_sessions = Vec::with_capacity(sessions.len());
    for record in sessions {
        let status = monitor_session_status(context, &record.session_id);
        recent_sessions.push(json!({
            "session_id": record.session_id,
            "title": record.title,
            "updated_at": format_ts(record.updated_at),
            "last_message_at": format_ts(record.last_message_at),
            "parent_session_id": record.parent_session_id,
            "status": status,
            "active": active_set.contains(&record.session_id),
        }));
    }
    Ok(build_model_tool_success(
        "status",
        "completed",
        format!("Loaded swarm status for {}.", agent.name),
        json!({
            "agent": {
                "agent_id": agent.agent_id,
                "hive_id": normalize_hive_id(&agent.hive_id),
                "name": agent.name,
                "description": agent.description,
                "status": agent.status,
                "is_shared": agent.is_shared,
                "access_level": agent.access_level,
                "updated_at": format_ts(agent.updated_at),
            },
            "session_total": session_total,
            "active_session_total": active_session_ids.len(),
            "running_session_total": runtime.map(|entry| entry.running_sessions.len()).unwrap_or(0),
            "lock_session_total": runtime.map(|entry| entry.lock_sessions.len()).unwrap_or(0),
            "active_session_ids": active_session_ids,
            "recent_sessions": recent_sessions,
        }),
    ))
}

async fn agent_swarm_send(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmSendArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "send",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm send arguments are invalid: {err}"),
                "请提供 action=\"send\"，并同时给出非空 message 与 agent_name、agent_id、session_id 三者之一；优先填写 agent_name。",
                &["message", "agent_name|agent_id|session_id"],
                agent_swarm_send_example(),
                args,
                json!({}),
            ));
        }
    };
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let message = payload.message.trim().to_string();
    if message.is_empty() {
        return Ok(build_agent_swarm_args_failure(
            "send",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm send requires non-empty message",
            "请提供非空 message，并指定 agent_name、agent_id 或 session_id；优先使用 agent_name。",
            &["message"],
            agent_swarm_send_example(),
            args,
            json!({}),
        ));
    }
    let current_agent_id = current_agent_id(context);
    let include_current = payload.include_current.unwrap_or(false);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let requested_agent_id = normalize_optional_string(payload.agent_id);
    let requested_agent_name = normalize_optional_string(payload.agent_name);
    let thread_strategy = match parse_swarm_worker_thread_strategy(
        payload.thread_strategy.as_deref(),
        payload.reuse_main_thread,
    ) {
        Ok(strategy) => strategy,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "send",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm send thread strategy is invalid: {err}"),
                "threadStrategy 只支持 fresh_main_thread 或 main_thread；也可以改用 reuseMainThread=true。",
                &[],
                agent_swarm_send_example(),
                args,
                json!({
                    "allowed_thread_strategies": ["fresh_main_thread", "main_thread"]
                }),
            ));
        }
    };
    let has_session_key = payload
        .session_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    if requested_agent_id.is_none() && requested_agent_name.is_none() && !has_session_key {
        return Ok(build_agent_swarm_args_failure(
            "send",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm send requires agent_id/agent_name or session_id",
            "请至少提供一个目标字段：agent_name、agent_id 或 session_id，再发送 message；优先使用 agent_name。",
            &["agent_name|agent_id|session_id"],
            agent_swarm_send_example(),
            args,
            json!({}),
        ));
    }
    let (target_agent, target_session_id, created_session) = if let Some(session_key) =
        payload.session_key
    {
        let session_id = resolve_session_key(Some(session_key))?;
        let record = context
            .storage
            .get_chat_session(user_id, &session_id)?
            .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
        let resolved_agent_id = normalize_optional_string(record.agent_id.clone())
            .ok_or_else(|| anyhow!("agent_swarm send target session is missing agent_id"))?;
        if !include_current {
            ensure_swarm_target_not_current(&resolved_agent_id, current_agent_id.as_deref())?;
        }
        if let Some(requested) = requested_agent_id.as_ref() {
            if requested != &resolved_agent_id {
                return Err(anyhow!(
                    "agent_swarm send agent_id does not match target session"
                ));
            }
        }
        if let Some(requested) = requested_agent_name.as_deref() {
            let agent = load_agent_record(
                context.storage.as_ref(),
                user_id,
                Some(&resolved_agent_id),
                false,
            )?
            .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
            if swarm_tool_hint::normalize_swarm_agent_name_lookup_key(requested)
                != swarm_tool_hint::normalize_swarm_agent_name_lookup_key(&agent.name)
            {
                return Err(anyhow!(
                    "agent_swarm send agent_name does not match target session"
                ));
            }
        }
        let target_agent = load_agent_record(
            context.storage.as_ref(),
            user_id,
            Some(&resolved_agent_id),
            false,
        )?
        .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
        ensure_swarm_agent_in_hive(&target_agent, &swarm_hive_id)?;
        (target_agent, session_id, false)
    } else {
        let target_agent = resolve_swarm_agent_record(
            context.storage.as_ref(),
            user_id,
            current_agent_id.as_deref(),
            include_current,
            &swarm_hive_id,
            requested_agent_id.as_deref(),
            requested_agent_name.as_deref(),
        )?
        .ok_or_else(|| anyhow!("agent_swarm send requires agent_id/agent_name or session_id"))?;
        match thread_strategy {
            SwarmWorkerThreadStrategy::MainThread => {
                let (main_session, created_main_session) = if let Some((orchestration_state, _)) =
                    active_orchestration_for_agent(
                        context.storage.as_ref(),
                        user_id,
                        &target_agent.agent_id,
                    )
                {
                    let (binding, created) = ensure_orchestration_member_session(
                        context.storage.as_ref(),
                        user_id,
                        &orchestration_state,
                        &target_agent,
                    )?;
                    let session = context
                        .storage
                        .get_chat_session(user_id, &binding.session_id)?
                        .ok_or_else(|| anyhow!("orchestration worker session not found"))?;
                    (session, created)
                } else {
                    crate::services::swarm::beeroom::resolve_or_create_agent_main_session(
                        context.storage.as_ref(),
                        user_id,
                        &target_agent,
                    )?
                };
                (target_agent, main_session.session_id, created_main_session)
            }
            SwarmWorkerThreadStrategy::FreshMainThread => {
                // Callers can still force a clean worker thread, but this is no longer the
                // default path for swarm workers.
                let dispatch_preview = build_swarm_dispatch_message(
                    context.storage.as_ref(),
                    context.monitor.as_deref(),
                    user_id,
                    &swarm_hive_id,
                    current_agent_id.as_deref(),
                    context.session_id,
                    None,
                    None,
                    &message,
                )?;
                let prepared = prepare_swarm_child_session(
                    context,
                    &dispatch_preview,
                    payload.label.clone(),
                    &target_agent.agent_id,
                )?;
                (target_agent, prepared.child_session_id, true)
            }
        }
    };
    let target_agent_id = target_agent.agent_id.clone();
    let orchestration_context = load_dispatch_context(
        context.storage.as_ref(),
        context.workspace.as_ref(),
        context.workspace_id,
        user_id,
        context.session_id,
    );

    let mother_agent_id = claim_swarm_mother_for_context(context, user_id, &swarm_hive_id)?;
    let mut run_record = create_swarm_team_run_record(
        context,
        user_id,
        &swarm_hive_id,
        mother_agent_id,
        None,
        "direct_send",
        1,
    );
    context.storage.upsert_team_run(&run_record)?;
    emit_swarm_run_started(context, &run_record);
    let mut task_record = create_swarm_team_task_record(
        &run_record,
        &target_agent_id,
        Some(target_session_id.clone()),
        created_session.then_some(target_session_id.clone()),
        0,
    );
    let dispatch_message = build_swarm_dispatch_message(
        context.storage.as_ref(),
        context.monitor.as_deref(),
        user_id,
        &swarm_hive_id,
        current_agent_id.as_deref(),
        context.session_id,
        Some(&run_record.team_run_id),
        Some(&task_record.task_id),
        &message,
    )?;
    let dispatch_message = build_worker_dispatch_message(
        context.config,
        context.workspace.as_ref(),
        &context
            .workspace
            .scoped_user_id_by_container(user_id, target_agent.sandbox_container_id),
        &dispatch_message,
        orchestration_context.as_ref(),
        &target_agent_id,
        &target_agent.name,
        created_session
            || !session_has_visible_history(context.storage.as_ref(), user_id, &target_session_id),
    );
    context.storage.upsert_team_task(&task_record)?;
    emit_swarm_task_dispatched(context, &run_record, &task_record);

    let wait_mode = resolve_swarm_wait_mode(
        payload.timeout_seconds,
        context.config.tools.swarm.default_timeout_s,
    );
    let mut send_args = json!({
        "session_id": target_session_id,
        "message": dispatch_message,
    });
    // Notify the parent session (queen bee) so the frontend sub-agent panel refreshes
    send_args["announceParentSessionId"] = json!(context.session_id);
    send_args["announcePersistHistory"] = json!(false);
    send_args["announceEmitParentEvents"] = json!(true);
    send_args["teamTaskId"] = json!(task_record.task_id);
    match wait_mode {
        SwarmWaitMode::Immediate => {
            send_args["timeoutSeconds"] = json!(0.0);
        }
        SwarmWaitMode::Finite(timeout_s) => {
            send_args["timeoutSeconds"] = json!(timeout_s);
        }
        SwarmWaitMode::Infinite => {
            send_args["waitForever"] = json!(true);
        }
    }
    let result = match sessions_send(context, &send_args).await {
        Ok(value) => value,
        Err(err) => {
            task_record.status = "error".to_string();
            task_record.error = Some(truncate_tool_result_text(&err.to_string()));
            task_record.updated_time = now_ts();
            task_record.finished_time = Some(task_record.updated_time);
            context.storage.upsert_team_task(&task_record)?;
            emit_swarm_task_updated(context, &run_record, &task_record);
            let (terminal, failed) = sync_swarm_run_summary(
                context,
                &mut run_record,
                std::slice::from_ref(&task_record),
            )?;
            if terminal {
                emit_swarm_run_terminal(context, &run_record, failed);
            }
            return Err(err);
        }
    };
    let state = tool_result_field(&result, "state")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            normalize_tool_run_state(
                result
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("accepted"),
            )
        });
    let run_id = tool_result_field(&result, "run_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if let Some(run_id) = run_id.as_deref() {
        task_record.session_run_id = Some(run_id.to_string());
    }
    let reply = tool_result_field(&result, "reply")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let error_text = tool_result_field(&result, "error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let elapsed_s = tool_result_field(&result, "elapsed_s")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0);
    let should_sync_progress = matches!(state.as_str(), "accepted" | "running" | "timeout");
    if let Some(run_id) = run_id.as_deref() {
        if should_sync_progress {
            if let Some(session_run) = context.storage.get_session_run(run_id)? {
                if !is_swarm_task_terminal_status(&session_run.status) {
                    apply_session_run_to_swarm_task(&mut task_record, &session_run);
                }
            }
            task_record.updated_time = task_record.updated_time.max(now_ts());
            context.storage.upsert_team_task(&task_record)?;
            emit_swarm_task_updated(context, &run_record, &task_record);
            let (terminal, failed) = sync_swarm_run_summary(
                context,
                &mut run_record,
                std::slice::from_ref(&task_record),
            )?;
            if terminal {
                emit_swarm_run_terminal(context, &run_record, failed);
            }
        }
    } else if state == "error" {
        task_record.status = "error".to_string();
        task_record.error = error_text.as_deref().map(truncate_tool_result_text);
        task_record.updated_time = now_ts();
        task_record.finished_time = Some(task_record.updated_time);
        context.storage.upsert_team_task(&task_record)?;
        emit_swarm_task_updated(context, &run_record, &task_record);
        let (terminal, failed) =
            sync_swarm_run_summary(context, &mut run_record, std::slice::from_ref(&task_record))?;
        if terminal {
            emit_swarm_run_terminal(context, &run_record, failed);
        }
    }
    return Ok(build_agent_swarm_tool_result(
        "send",
        &state,
        run_record.team_run_id,
        Some(task_record.task_id),
        run_id,
        Some(target_session_id),
        Some(target_agent_id),
        Some(target_agent.name),
        Some(created_session),
        reply,
        error_text,
        elapsed_s,
        Some(json!({
            "thread_strategy": if has_session_key {
                "session_key"
            } else {
                thread_strategy.as_tool_value()
            }
        })),
    ));
    /*
    let mut response = json!({
        "action": "send",
        "status": status,
        "run_id": run_id,
        "team_run_id": run_record.team_run_id,
        "task_id": task_record.task_id,
        "elapsed_s": elapsed_s,
        "target_agent_id": target_agent_id,
        "target_agent_name": target_agent.name,
        "target_session_id": target_session_id,
        "created_session": created_session,
        "reply": reply,
        "error": error_text,
    });
    if let Value::Object(ref mut map) = response {
        let status_message = match status.as_str() {
            "ok" => "工蜂已完成并返回结果。".to_string(),
            "timeout" => format!(
                "等待工蜂结果超时（{} 秒），工蜂可能仍在执行。",
                swarm_wait_seconds_value(wait_mode).unwrap_or_default()
            ),
            "error" => "工蜂执行失败，请查看 error 字段。".to_string(),
            _ => "任务已派发，可稍后继续调用 wait 监视进度。".to_string(),
        };
        map.insert("message".to_string(), json!(status_message));
        if status == "timeout" {
            map.insert("timed_out".to_string(), json!(true));
            map.insert(
                "monitoring".to_string(),
                build_swarm_timeout_monitoring_payload(
                    run_id.as_deref(),
                    &run_record.team_run_id,
                    &target_session_id,
                ),
            );
        }
    }
    Ok(response)
    */
}

async fn agent_swarm_batch_send(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmBatchSendArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm batch_send arguments are invalid: {err}"),
                "请提供 action=\"batch_send\" 和非空 tasks；每个 task 都必须指定目标，message 建议在每个 task 内显式给出。",
                &["tasks"],
                agent_swarm_batch_send_example(),
                args,
                json!({}),
            ));
        }
    };
    if payload.tasks.is_empty() {
        return Ok(build_agent_swarm_args_failure(
            "batch_send",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm batch_send requires non-empty tasks",
            "请提供非空 tasks。每个 task 至少要指定一个目标字段和一个非空 message。",
            &["tasks"],
            agent_swarm_batch_send_example(),
            args,
            json!({}),
        ));
    }

    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }

    let max_tasks = context
        .config
        .tools
        .swarm
        .max_parallel_tasks_per_team
        .max(1);
    if payload.tasks.len() > max_tasks {
        return Ok(build_agent_swarm_args_failure(
            "batch_send",
            "TOOL_ARGS_INVALID",
            format!(
                "agent_swarm batch_send task count {} exceeds max_parallel_tasks_per_team {}",
                payload.tasks.len(),
                max_tasks
            ),
            format!("请减少 tasks 数量到 {max_tasks} 个以内，或拆成多次 batch_send。"),
            &[],
            agent_swarm_batch_send_example(),
            args,
            json!({
                "max_parallel_tasks_per_team": max_tasks
            }),
        ));
    }

    let shared_message = normalize_optional_string(payload.message.clone());
    let shared_label = normalize_optional_string(payload.label.clone());
    let shared_thread_strategy = match parse_swarm_worker_thread_strategy(
        payload.thread_strategy.as_deref(),
        payload.reuse_main_thread,
    ) {
        Ok(strategy) => strategy,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm batch_send thread strategy is invalid: {err}"),
                "threadStrategy 只支持 fresh_main_thread 或 main_thread；也可以改用 reuseMainThread=true。",
                &[],
                agent_swarm_batch_send_example(),
                args,
                json!({
                    "allowed_thread_strategies": ["fresh_main_thread", "main_thread"]
                }),
            ));
        }
    };
    for (index, task) in payload.tasks.iter().enumerate() {
        let has_target = normalize_optional_string(task.agent_id.clone()).is_some()
            || normalize_optional_string(task.agent_name.clone()).is_some()
            || task
                .session_key
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
        if !has_target {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_MISSING_FIELD",
                format!(
                    "agent_swarm batch_send task[{index}] requires agent_id/agent_name or session_id"
                ),
                "请在每个 task 内至少提供 agent_name、agent_id、session_id 之一，不要传空对象；优先使用 agent_name。",
                &["tasks[].agent_name|agent_id|session_id"],
                agent_swarm_batch_send_example(),
                args,
                json!({
                    "task_index": index,
                    "expected_task_shape": {
                        "agent_name": "worker_a",
                        "message": "请总结政府退休政策的核心要点。"
                    }
                }),
            ));
        }
        let has_message = normalize_optional_string(task.message.clone())
            .or_else(|| shared_message.clone())
            .is_some();
        if !has_message {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_MISSING_FIELD",
                format!("agent_swarm batch_send task[{index}] requires message"),
                "请为每个 task 提供非空 message，或在顶层传入共享 message。最稳妥的写法是每个 task 都显式填写 message。",
                &["tasks[].message"],
                agent_swarm_batch_send_example(),
                args,
                json!({
                    "task_index": index,
                    "expected_task_shape": {
                        "agent_name": "worker_a",
                        "message": "请总结政府退休政策的核心要点。"
                    }
                }),
            ));
        }
    }
    let default_include_current = payload.include_current.unwrap_or(false);
    let current_agent_id = current_agent_id(context);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let mother_agent_id = claim_swarm_mother_for_context(context, user_id, &swarm_hive_id)?;
    let orchestration_context = load_dispatch_context(
        context.storage.as_ref(),
        context.workspace.as_ref(),
        context.workspace_id,
        user_id,
        context.session_id,
    );
    let mut run_record = create_swarm_team_run_record(
        context,
        user_id,
        &swarm_hive_id,
        mother_agent_id,
        payload.team_run_id.as_deref(),
        "batch_send",
        payload.tasks.len(),
    );
    context.storage.upsert_team_run(&run_record)?;
    emit_swarm_run_started(context, &run_record);
    let allowed_tools = collect_user_allowed_tools(context, user_id)?;

    let agent_access = context.storage.get_user_agent_access(user_id)?;
    let mut agent_map = HashMap::new();
    let mut agents = context.storage.list_user_agents(user_id)?;
    agents.extend(context.storage.list_shared_user_agents(user_id)?);
    for agent in agents {
        if agent.agent_id.trim().is_empty() {
            continue;
        }
        if !is_agent_allowed_by_access(user_id, agent_access.as_ref(), &agent) {
            continue;
        }
        ensure_swarm_agent_in_hive(&agent, &swarm_hive_id)?;
        agent_map.entry(agent.agent_id.clone()).or_insert(agent);
    }

    let (sessions, _) = context
        .storage
        .list_chat_sessions(user_id, None, None, 0, 4096)?;
    let mut sessions_by_id = HashMap::with_capacity(sessions.len());
    for session in sessions {
        sessions_by_id.insert(session.session_id.clone(), session.clone());
    }

    let mut dispatch_plan = Vec::with_capacity(payload.tasks.len());
    let mut task_records_by_index = HashMap::new();
    for (index, task) in payload.tasks.into_iter().enumerate() {
        let message = normalize_optional_string(task.message)
            .or_else(|| shared_message.clone())
            .ok_or_else(|| anyhow!("agent_swarm batch_send task[{index}] requires message"))?;
        let label = normalize_optional_string(task.label).or_else(|| shared_label.clone());
        let include_current = task.include_current.unwrap_or(default_include_current);
        let requested_agent_id = normalize_optional_string(task.agent_id);
        let requested_agent_name = normalize_optional_string(task.agent_name);
        let task_thread_strategy = match if task.thread_strategy.is_some()
            || task.reuse_main_thread.is_some()
        {
            parse_swarm_worker_thread_strategy(
                task.thread_strategy.as_deref(),
                task.reuse_main_thread,
            )
        } else {
            Ok(shared_thread_strategy)
        } {
            Ok(strategy) => strategy,
            Err(err) => {
                return Ok(build_agent_swarm_args_failure(
                    "batch_send",
                    "TOOL_ARGS_INVALID",
                    format!(
                        "agent_swarm batch_send task[{index}] thread strategy is invalid: {err}"
                    ),
                    "每个 task 的 threadStrategy 只支持 fresh_main_thread 或 main_thread；也可以改用 reuseMainThread=true。",
                    &[],
                    agent_swarm_batch_send_example(),
                    args,
                    json!({
                        "task_index": index,
                        "allowed_thread_strategies": ["fresh_main_thread", "main_thread"]
                    }),
                ));
            }
        };
        let requested_session_id = task
            .session_key
            .map(|value| resolve_session_key(Some(value)))
            .transpose()?;

        let (agent_record, session_record) = if let Some(session_id) = requested_session_id {
            let session_record = if let Some(record) = sessions_by_id.get(&session_id).cloned() {
                record
            } else {
                context
                    .storage
                    .get_chat_session(user_id, &session_id)?
                    .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?
            };

            let resolved_agent_id = normalize_optional_string(session_record.agent_id.clone())
                .ok_or_else(|| anyhow!("agent_swarm send target session is missing agent_id"))?;
            if !include_current {
                ensure_swarm_target_not_current(&resolved_agent_id, current_agent_id.as_deref())?;
            }
            if let Some(requested) = requested_agent_id.as_ref() {
                if requested != &resolved_agent_id {
                    return Err(anyhow!(
                        "agent_swarm send agent_id does not match target session"
                    ));
                }
            }
            if let Some(requested) = requested_agent_name.as_deref() {
                let agent_record = if let Some(agent) = agent_map.get(&resolved_agent_id).cloned() {
                    agent
                } else {
                    load_agent_record(
                        context.storage.as_ref(),
                        user_id,
                        Some(&resolved_agent_id),
                        false,
                    )?
                    .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?
                };
                if swarm_tool_hint::normalize_swarm_agent_name_lookup_key(requested)
                    != swarm_tool_hint::normalize_swarm_agent_name_lookup_key(&agent_record.name)
                {
                    return Err(anyhow!(
                        "agent_swarm send agent_name does not match target session"
                    ));
                }
            }

            let agent_record = if let Some(agent) = agent_map.get(&resolved_agent_id).cloned() {
                agent
            } else {
                load_agent_record(
                    context.storage.as_ref(),
                    user_id,
                    Some(&resolved_agent_id),
                    false,
                )?
                .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?
            };
            ensure_swarm_agent_in_hive(&agent_record, &swarm_hive_id)?;

            sessions_by_id.insert(session_record.session_id.clone(), session_record.clone());
            (agent_record, Some(session_record))
        } else {
            let agent_record = resolve_swarm_agent_record(
                context.storage.as_ref(),
                user_id,
                current_agent_id.as_deref(),
                include_current,
                &swarm_hive_id,
                requested_agent_id.as_deref(),
                requested_agent_name.as_deref(),
            )?
            .ok_or_else(|| {
                anyhow!("agent_swarm send requires agent_id/agent_name or session_id")
            })?;
            // Swarm batch dispatch also forces a fresh worker thread when the caller
            // does not explicitly target an existing session_key.
            (agent_record, None)
        };
        let mut task_record =
            create_swarm_team_task_record(&run_record, &agent_record.agent_id, None, None, 0);
        let dispatch_message = build_swarm_dispatch_message(
            context.storage.as_ref(),
            context.monitor.as_deref(),
            user_id,
            &swarm_hive_id,
            current_agent_id.as_deref(),
            context.session_id,
            Some(&run_record.team_run_id),
            Some(&task_record.task_id),
            &message,
        )?;
        let (
            session_id,
            created_session,
            resolved_thread_strategy,
            tool_names,
            model_name,
            agent_prompt,
        ) = if let Some(session_record) = session_record {
            let tool_names = resolve_swarm_batch_tool_names(
                context,
                context.config,
                context.skills,
                &allowed_tools,
                user_id,
                &session_record,
                &agent_record,
            );
            let agent_prompt = {
                let prompt = agent_record.system_prompt.trim();
                if prompt.is_empty() {
                    None
                } else {
                    Some(prompt.to_string())
                }
            };
            let model_name = normalize_optional_string(agent_record.model_name.clone());
            (
                session_record.session_id,
                false,
                "session_key",
                tool_names,
                model_name,
                agent_prompt,
            )
        } else {
            match task_thread_strategy {
                SwarmWorkerThreadStrategy::MainThread => {
                    let (main_session, created_main_session) =
                        if let Some((orchestration_state, _)) = active_orchestration_for_agent(
                            context.storage.as_ref(),
                            user_id,
                            &agent_record.agent_id,
                        ) {
                            let (binding, created) = ensure_orchestration_member_session(
                                context.storage.as_ref(),
                                user_id,
                                &orchestration_state,
                                &agent_record,
                            )?;
                            let session = context
                                .storage
                                .get_chat_session(user_id, &binding.session_id)?
                                .ok_or_else(|| anyhow!("orchestration worker session not found"))?;
                            (session, created)
                        } else {
                            crate::services::swarm::beeroom::resolve_or_create_agent_main_session(
                                context.storage.as_ref(),
                                user_id,
                                &agent_record,
                            )?
                        };
                    let tool_names = resolve_swarm_batch_tool_names(
                        context,
                        context.config,
                        context.skills,
                        &allowed_tools,
                        user_id,
                        &main_session,
                        &agent_record,
                    );
                    let agent_prompt = {
                        let prompt = agent_record.system_prompt.trim();
                        if prompt.is_empty() {
                            None
                        } else {
                            Some(prompt.to_string())
                        }
                    };
                    let model_name = normalize_optional_string(agent_record.model_name.clone());
                    (
                        main_session.session_id,
                        created_main_session,
                        SwarmWorkerThreadStrategy::MainThread.as_tool_value(),
                        tool_names,
                        model_name,
                        agent_prompt,
                    )
                }
                SwarmWorkerThreadStrategy::FreshMainThread => {
                    let prepared = prepare_swarm_child_session(
                        context,
                        &dispatch_message,
                        label.clone(),
                        &agent_record.agent_id,
                    )?;
                    (
                        prepared.child_session_id,
                        true,
                        SwarmWorkerThreadStrategy::FreshMainThread.as_tool_value(),
                        prepared.request.tool_names,
                        prepared.model_name,
                        prepared.request.agent_prompt,
                    )
                }
            }
        };
        let dispatch_message = build_worker_dispatch_message(
            context.config,
            context.workspace.as_ref(),
            &context
                .workspace
                .scoped_user_id_by_container(user_id, agent_record.sandbox_container_id),
            &dispatch_message,
            orchestration_context.as_ref(),
            &agent_record.agent_id,
            &agent_record.name,
            created_session
                || !session_has_visible_history(context.storage.as_ref(), user_id, &session_id),
        );
        task_record.target_session_id = Some(session_id.clone());
        task_record.spawned_session_id = created_session.then_some(session_id.clone());
        context.storage.upsert_team_task(&task_record)?;
        emit_swarm_task_dispatched(context, &run_record, &task_record);
        task_records_by_index.insert(index, task_record.clone());

        dispatch_plan.push(SwarmBatchDispatchTask {
            index,
            team_task_id: task_record.task_id,
            message: dispatch_message,
            label,
            agent_id: agent_record.agent_id,
            agent_name: agent_record.name,
            session_id,
            created_session,
            thread_strategy: resolved_thread_strategy,
            tool_names,
            model_name,
            agent_prompt,
        });
    }

    let dispatch_parallelism = dispatch_plan.len().min(max_tasks).max(1);
    let mut dispatches = stream::iter(dispatch_plan.into_iter().map(|task| async move {
        let index = task.index;
        let result = dispatch_swarm_batch_task(context, task).await;
        (index, result)
    }))
    .buffer_unordered(dispatch_parallelism);

    let mut indexed_items = Vec::new();
    let mut run_ids = Vec::new();
    while let Some((index, result)) = dispatches.next().await {
        match result {
            Ok(result) => {
                let run_id = tool_result_field(&result, "run_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("")
                    .to_string();
                if !run_id.is_empty() {
                    run_ids.push(run_id.clone());
                }
                if let Some(task_record) = task_records_by_index.get_mut(&index) {
                    if !run_id.is_empty() {
                        task_record.session_run_id = Some(run_id.clone());
                        if let Some(session_run) = context.storage.get_session_run(&run_id)? {
                            if !is_swarm_task_terminal_status(&session_run.status) {
                                apply_session_run_to_swarm_task(task_record, &session_run);
                            }
                        }
                    }
                    let status = tool_result_field(&result, "status")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or("accepted")
                        .to_ascii_lowercase();
                    if status == "error" && run_id.is_empty() {
                        task_record.status = "error".to_string();
                        task_record.error = tool_result_field(&result, "error")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(truncate_tool_result_text);
                        task_record.elapsed_s = tool_result_field(&result, "elapsed_s")
                            .and_then(Value::as_f64)
                            .filter(|value| value.is_finite() && *value >= 0.0);
                    }
                    task_record.updated_time = now_ts();
                    if matches!(task_record.status.as_str(), "success" | "timeout" | "error") {
                        task_record.finished_time = Some(task_record.updated_time);
                    }
                    context.storage.upsert_team_task(task_record)?;
                    emit_swarm_task_updated(context, &run_record, task_record);
                }
                let task_id = tool_result_field(&result, "task_id")
                    .cloned()
                    .unwrap_or_else(|| {
                        task_records_by_index
                            .get(&index)
                            .map(|item| json!(item.task_id))
                            .unwrap_or(Value::Null)
                    });
                let mut item = json!({
                    "index": index,
                    "status": tool_result_field(&result, "status")
                        .cloned()
                        .unwrap_or_else(|| json!("accepted")),
                    "run_id": if run_id.is_empty() { Value::Null } else { json!(run_id) },
                    "target_agent_id": tool_result_field_or_null(&result, "agent_id"),
                    "target_agent_name": tool_result_field_or_null(&result, "agent_name"),
                    "target_session_id": tool_result_field_or_null(&result, "session_id"),
                    "task_id": task_id,
                    "created_session": tool_result_field(&result, "created_session")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    "thread_strategy": tool_result_field(&result, "thread_strategy")
                        .cloned()
                        .unwrap_or_else(|| json!(shared_thread_strategy.as_tool_value())),
                });
                if let Some(error) = tool_result_field(&result, "error") {
                    if let Value::Object(ref mut map) = item {
                        map.insert("error".to_string(), error.clone());
                    }
                }
                indexed_items.push((index, item));
            }
            Err(err) => {
                let error_text = truncate_tool_result_text(&err.to_string());
                if let Some(task_record) = task_records_by_index.get_mut(&index) {
                    task_record.status = "error".to_string();
                    task_record.error = Some(error_text.clone());
                    task_record.updated_time = now_ts();
                    task_record.finished_time = Some(task_record.updated_time);
                    context.storage.upsert_team_task(task_record)?;
                    emit_swarm_task_updated(context, &run_record, task_record);
                }
                indexed_items.push((
                    index,
                    json!({
                        "index": index,
                        "status": "error",
                        "task_id": task_records_by_index
                            .get(&index)
                            .map(|item| json!(item.task_id))
                            .unwrap_or(Value::Null),
                        "error": error_text,
                    }),
                ));
            }
        }
    }

    indexed_items.sort_by_key(|(index, _)| *index);
    let items = indexed_items
        .into_iter()
        .map(|(_, item)| item)
        .collect::<Vec<_>>();
    let run_ids = dedupe_non_empty_strings(run_ids);
    let accepted_total = items
        .iter()
        .filter(|item| {
            item.get("run_id")
                .and_then(Value::as_str)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
        .count();
    let failed_total = items.len().saturating_sub(accepted_total);
    let run_tasks = task_records_by_index.values().cloned().collect::<Vec<_>>();
    let (terminal, failed) = sync_swarm_run_summary(context, &mut run_record, &run_tasks)?;
    if terminal {
        emit_swarm_run_terminal(context, &run_record, failed);
    }

    let mut state = if accepted_total > 0 {
        if failed_total > 0 {
            "partial".to_string()
        } else {
            "accepted".to_string()
        }
    } else {
        "error".to_string()
    };
    let mut wait_result_value = Value::Null;

    let wait_mode = resolve_swarm_wait_mode(
        payload.wait_seconds,
        context.config.tools.swarm.default_timeout_s,
    );
    if !matches!(wait_mode, SwarmWaitMode::Immediate) {
        let poll_interval_seconds = payload
            .poll_interval_seconds
            .unwrap_or(SWARM_WAIT_DEFAULT_POLL_S);
        let wait_result = wait_for_swarm_runs(
            context,
            &run_ids,
            swarm_wait_seconds_value(wait_mode),
            poll_interval_seconds,
            true,
        )
        .await?;
        if let Some(wait_items) = tool_result_field(&wait_result, "items").and_then(Value::as_array)
        {
            let mut snapshots_by_run_id: HashMap<String, &Value> = HashMap::new();
            for item in wait_items {
                let Some(run_id) = item
                    .get("run_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                snapshots_by_run_id.insert(run_id.to_string(), item);
            }

            for task_record in task_records_by_index.values_mut() {
                let Some(run_id) = task_record
                    .session_run_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                let Some(snapshot) = snapshots_by_run_id.get(run_id) else {
                    continue;
                };
                let snapshot_status = snapshot
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("")
                    .to_ascii_lowercase();
                task_record.status = match snapshot_status.as_str() {
                    "success" | "ok" => "success".to_string(),
                    "timeout" => "timeout".to_string(),
                    "cancelled" => "cancelled".to_string(),
                    "queued" | "running" => "queued".to_string(),
                    _ => "error".to_string(),
                };
                task_record.started_time = snapshot
                    .get("started_time")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value > 0.0);
                task_record.finished_time = snapshot
                    .get("finished_time")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value > 0.0);
                task_record.elapsed_s = snapshot
                    .get("elapsed_s")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value >= 0.0);
                task_record.result_summary = snapshot
                    .get("result_preview")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(truncate_tool_result_text);
                task_record.error = snapshot
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(truncate_tool_result_text);
                task_record.updated_time = snapshot
                    .get("updated_time")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value > 0.0)
                    .unwrap_or_else(now_ts);
                if matches!(
                    task_record.status.as_str(),
                    "success" | "timeout" | "error" | "cancelled"
                ) && task_record.finished_time.is_none()
                {
                    task_record.finished_time = Some(task_record.updated_time);
                }
                context.storage.upsert_team_task(task_record)?;
                emit_swarm_task_updated(context, &run_record, task_record);
            }

            let run_tasks = task_records_by_index.values().cloned().collect::<Vec<_>>();
            let (terminal, failed) = sync_swarm_run_summary(context, &mut run_record, &run_tasks)?;
            if terminal {
                emit_swarm_run_terminal(context, &run_record, failed);
            }
        }
        wait_result_value = wait_result.clone();
        if let Some(wait_state) = wait_result.get("state").and_then(Value::as_str) {
            state = wait_state.to_string();
        }
    }
    Ok(build_agent_swarm_tool_result(
        "batch_send",
        &state,
        run_record.team_run_id,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(json!({
            "run_ids": run_ids,
            "counts": {
                "total": items.len(),
                "accepted": accepted_total,
                "failed": failed_total,
            },
            "items": items,
            "wait": wait_result_value,
        })),
    ))
}

async fn agent_swarm_wait(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmWaitArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "wait",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm wait arguments are invalid: {err}"),
                "请提供 action=\"wait\"，并传入 run_ids 数组或单个 run_id。",
                &["run_ids|run_id"],
                agent_swarm_wait_example(),
                args,
                json!({}),
            ));
        }
    };
    let mut run_ids = payload.run_ids.unwrap_or_default();
    if let Some(run_id) = payload.run_id {
        run_ids.push(run_id);
    }
    let run_ids = dedupe_non_empty_strings(run_ids);
    if run_ids.is_empty() {
        return Ok(build_agent_swarm_args_failure(
            "wait",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm wait requires runIds",
            "请传入 run_ids 数组或 run_id。通常应先从 send/batch_send 的返回结果中复制 run_id 再等待。",
            &["run_ids|run_id"],
            agent_swarm_wait_example(),
            args,
            json!({}),
        ));
    }
    let wait_mode = resolve_swarm_wait_mode(
        payload.wait_seconds,
        context.config.tools.swarm.default_timeout_s,
    );
    let poll_interval_seconds = payload
        .poll_interval_seconds
        .unwrap_or(SWARM_WAIT_DEFAULT_POLL_S);
    wait_for_swarm_runs(
        context,
        &run_ids,
        swarm_wait_seconds_value(wait_mode),
        poll_interval_seconds,
        true,
    )
    .await
}

fn compact_swarm_run_result_preview(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(truncate_tool_result_text)
}

async fn wait_for_swarm_runs(
    context: &ToolContext<'_>,
    run_ids: &[String],
    wait_seconds: Option<f64>,
    poll_interval_seconds: f64,
    emit_progress: bool,
) -> Result<Value> {
    let run_ids = dedupe_non_empty_strings(run_ids.to_vec());
    if run_ids.is_empty() {
        return Ok(build_model_tool_success(
            "wait",
            "error",
            "No worker runs were provided.",
            json!({
                "run_ids": [],
                "counts": {
                    "total": 0,
                    "done": 0,
                    "success": 0,
                    "failed": 0,
                    "queued": 0,
                    "running": 0,
                },
                "items": [],
            }),
        ));
    }

    let poll_interval = normalize_swarm_poll_interval(poll_interval_seconds);
    let started_at = Instant::now();

    loop {
        let snapshots = collect_swarm_run_snapshots(context, &run_ids)?;
        let total = snapshots.len();
        let done_total = snapshots.iter().filter(|item| item.terminal).count();
        let success_total = snapshots
            .iter()
            .filter(|item| item.status == "success")
            .count();
        let failed_total = snapshots.iter().filter(|item| item.failed).count();
        let queued_total = snapshots
            .iter()
            .filter(|item| item.status == "queued")
            .count();
        let running_total = snapshots
            .iter()
            .filter(|item| item.status == "running")
            .count();
        let elapsed_s = started_at.elapsed().as_secs_f64();
        let all_finished = done_total >= total;
        let timed_out = wait_seconds
            .filter(|value| *value > 0.0)
            .is_some_and(|value| elapsed_s >= value && !all_finished);
        let immediate_snapshot = wait_seconds.is_some_and(|value| value <= 0.0);

        if all_finished || timed_out || immediate_snapshot {
            let state = if all_finished {
                if failed_total == 0 {
                    "completed"
                } else {
                    "partial"
                }
            } else if timed_out {
                "timeout"
            } else {
                "running"
            };
            let items = snapshots
                .into_iter()
                .map(|item| item.payload)
                .collect::<Vec<_>>();
            let response = build_model_tool_success_with_hint(
                "wait",
                state,
                if state == "completed" {
                    "All worker runs finished.".to_string()
                } else if state == "partial" {
                    "Worker runs finished with partial success.".to_string()
                } else if state == "timeout" {
                    "Waiting for worker runs timed out.".to_string()
                } else {
                    "Worker runs are still executing.".to_string()
                },
                json!({
                    "run_ids": run_ids.clone(),
                    "wait_seconds": wait_seconds,
                    "wait_forever": wait_seconds.is_none(),
                    "elapsed_s": elapsed_s,
                    "all_finished": all_finished,
                    "counts": {
                        "total": total,
                        "done": done_total,
                        "success": success_total,
                        "failed": failed_total,
                        "queued": queued_total,
                        "running": running_total,
                    },
                    "items": items.clone(),
                }),
                if timed_out || !all_finished {
                    Some(
                        "Use agent_swarm.wait again or inspect status/history before treating unfinished worker runs as complete."
                            .to_string(),
                    )
                } else {
                    None
                },
            );
            crate::services::subagents::suppress_auto_wake_from_wait_result(&response);
            return Ok(response);
            /*
            let mut response = json!({
                "action": "wait",
                "status": status,
                "wait_seconds": wait_seconds,
                "wait_forever": wait_seconds.is_none(),
                "elapsed_s": elapsed_s,
                "all_finished": all_finished,
                "total": total,
                "done_total": done_total,
                "success_total": success_total,
                "failed_total": failed_total,
                "queued_total": queued_total,
                "running_total": running_total,
                "run_ids": run_ids.clone(),
                "items": items,
            });
            if let Value::Object(ref mut map) = response {
                let message = if timed_out {
                    format!(
                        "等待蜂群结果超时（{} 秒），工蜂可能仍在执行。",
                        wait_seconds.unwrap_or_default()
                    )
                } else if all_finished {
                    if failed_total == 0 {
                        "蜂群任务已全部完成。".to_string()
                    } else {
                        "蜂群任务已结束，但存在失败/超时条目。".to_string()
                    }
                } else {
                    "已返回当前快照，任务仍在进行中。".to_string()
                };
                map.insert("message".to_string(), json!(message));
                if timed_out || !all_finished {
                    map.insert(
                        "monitoring".to_string(),
                        build_swarm_wait_monitoring_payload(&run_ids),
                    );
                }
            }
            crate::services::subagents::suppress_auto_wake_from_wait_result(&response);
            return Ok(response);
            */
        }

        if emit_progress {
            if let Some(emitter) = context.event_emitter.as_ref() {
                emitter.emit(
                    "progress",
                    json!({
                        "stage": "swarm_wait",
                        "summary": i18n::t("monitor.summary.swarm_wait"),
                        "total": total,
                        "done_total": done_total,
                        "success_total": success_total,
                        "failed_total": failed_total,
                        "elapsed_s": elapsed_s,
                    }),
                );
            }
        }

        sleep(Duration::from_secs_f64(poll_interval)).await;
    }
}

fn collect_swarm_run_snapshots(
    context: &ToolContext<'_>,
    run_ids: &[String],
) -> Result<Vec<SwarmRunSnapshot>> {
    let mut output = Vec::with_capacity(run_ids.len());
    for run_id in run_ids {
        let record = context.storage.get_session_run(run_id)?;
        if let Some(record) = record {
            let status = record.status.trim().to_ascii_lowercase();
            let terminal = is_swarm_run_terminal(&status);
            let failed = is_swarm_run_failed(&status);
            output.push(SwarmRunSnapshot {
                status,
                terminal,
                failed,
                payload: json!({
                    "run_id": record.run_id,
                    "status": record.status,
                    "terminal": terminal,
                    "failed": failed,
                    "session_id": record.session_id,
                    "agent_id": record.agent_id,
                    "started_time": record.started_time,
                    "finished_time": record.finished_time,
                    "elapsed_s": record.elapsed_s,
                    "result_preview": compact_swarm_run_result_preview(record.result.as_deref()),
                    "error": record.error,
                    "updated_time": record.updated_time,
                }),
            });
        } else {
            output.push(SwarmRunSnapshot {
                status: "not_found".to_string(),
                terminal: true,
                failed: true,
                payload: json!({
                    "run_id": run_id,
                    "status": "not_found",
                    "terminal": true,
                    "failed": true,
                    "error": "run not found",
                }),
            });
        }
    }
    Ok(output)
}

fn is_swarm_run_terminal(status: &str) -> bool {
    matches!(
        status,
        "success" | "error" | "timeout" | "cancelled" | "failed" | "not_found"
    )
}

fn is_swarm_run_failed(status: &str) -> bool {
    matches!(
        status,
        "error" | "timeout" | "cancelled" | "failed" | "not_found"
    )
}

fn normalize_swarm_poll_interval(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return SWARM_WAIT_DEFAULT_POLL_S;
    }
    value.clamp(SWARM_WAIT_MIN_POLL_S, SWARM_WAIT_MAX_POLL_S)
}

fn dedupe_non_empty_strings(items: Vec<String>) -> Vec<String> {
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

async fn agent_swarm_history(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionHistoryArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let session_id = resolve_session_key(payload.session_key.clone())?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let include_current = args
        .get("includeCurrent")
        .or_else(|| args.get("include_current"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let requested_agent_id = args
        .get("agentId")
        .or_else(|| args.get("agent_id"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let record = context
        .storage
        .get_chat_session(user_id, &session_id)?
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let target_agent_id = normalize_optional_string(record.agent_id.clone())
        .ok_or_else(|| anyhow!("agent_swarm history target session is missing agent_id"))?;
    let target_agent = load_agent_record(
        context.storage.as_ref(),
        user_id,
        Some(&target_agent_id),
        false,
    )?
    .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
    ensure_swarm_agent_in_hive(&target_agent, &swarm_hive_id)?;
    if let Some(requested_agent_id) = requested_agent_id {
        if requested_agent_id != target_agent_id {
            return Err(anyhow!(
                "agent_swarm history agent_id does not match target session"
            ));
        }
    }
    if !include_current {
        ensure_swarm_target_not_current(&target_agent_id, current_agent_id(context).as_deref())?;
    }
    sessions_history(context, args).await
}

async fn agent_swarm_spawn(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionSpawnArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "spawn",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm spawn arguments are invalid: {err}"),
                "请提供 action=\"spawn\"、非空 task，以及 agent_name 或 agent_id；优先使用 agent_name。临时子会话请改用 subagent_control.spawn。",
                &["task", "agent_name|agent_id"],
                agent_swarm_spawn_example(),
                args,
                json!({}),
            ));
        }
    };
    let SessionSpawnArgs {
        task,
        label,
        agent_id,
        agent_name,
        model: _,
        run_timeout_seconds,
        cleanup: _,
        thread_strategy,
        reuse_main_thread,
    } = payload;
    if task.trim().is_empty() {
        return Ok(build_agent_swarm_args_failure(
            "spawn",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm spawn requires non-empty task",
            "请提供非空 task；如果是向已存在智能体派发任务，优先写清楚预期产出。",
            &["task"],
            agent_swarm_spawn_example(),
            args,
            json!({}),
        ));
    }
    let requested_agent_id = normalize_optional_string(agent_id);
    let requested_agent_name = normalize_optional_string(agent_name);
    if requested_agent_id.is_none() && requested_agent_name.is_none() {
        return Ok(build_agent_swarm_args_failure(
            "spawn",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm spawn requires agent_id/agent_name",
            "请提供 agent_name 或 agent_id；优先使用 agent_name。如果你想创建临时子会话而不是调用已存在智能体，请改用 subagent_control.spawn。",
            &["agent_name|agent_id"],
            agent_swarm_spawn_example(),
            args,
            json!({}),
        ));
    }
    let include_current = args
        .get("includeCurrent")
        .or_else(|| args.get("include_current"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let user_id = context.user_id.trim();
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let target_agent = resolve_swarm_agent_record(
        context.storage.as_ref(),
        user_id,
        current_agent_id(context).as_deref(),
        include_current,
        &swarm_hive_id,
        requested_agent_id.as_deref(),
        requested_agent_name.as_deref(),
    )?
    .ok_or_else(|| anyhow!("agent_swarm spawn target agent not found"))?;
    let mut send_args = json!({
        "agentId": target_agent.agent_id,
        "message": task,
    });
    if let Value::Object(ref mut map) = send_args {
        if let Some(label) = label {
            map.insert("label".to_string(), json!(label));
        }
        if include_current {
            map.insert("includeCurrent".to_string(), Value::Bool(true));
        }
        if let Some(timeout_seconds) = run_timeout_seconds {
            map.insert(
                "timeoutSeconds".to_string(),
                json!(timeout_seconds.max(0.0)),
            );
        }
        if let Some(thread_strategy) = thread_strategy {
            map.insert("threadStrategy".to_string(), json!(thread_strategy));
        }
        if let Some(reuse_main_thread) = reuse_main_thread {
            map.insert("reuseMainThread".to_string(), json!(reuse_main_thread));
        }
        if let Some(hive_id) = swarm_hive_arg(args) {
            map.insert("hiveId".to_string(), json!(hive_id));
        }
    }
    let result = agent_swarm_send(context, &send_args).await?;
    Ok(enrich_agent_swarm_spawn_response(result))
}

fn enrich_agent_swarm_spawn_response(mut response: Value) -> Value {
    let state = response
        .get("state")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("accepted")
        .to_ascii_lowercase();
    let child_session_id = tool_result_field(&response, "session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if let Some(object) = response.as_object_mut() {
        object.insert("action".to_string(), json!("spawn"));
        if state == "accepted" {
            object.insert(
                "summary".to_string(),
                json!("Worker session was created and the initial task was queued."),
            );
        } else if state == "completed" {
            object.insert(
                "summary".to_string(),
                json!("Worker session completed the initial task."),
            );
        }
    }
    if let Some(data) = response.get_mut("data").and_then(Value::as_object_mut) {
        data.insert("spawned".to_string(), Value::Bool(true));
        if let Some(session_id) = child_session_id {
            data.insert("child_session_id".to_string(), Value::String(session_id));
        }
    }
    response
}

fn collect_swarm_agents(
    context: &ToolContext<'_>,
    user_id: &str,
    include_current: bool,
    hive_id: &str,
) -> Result<Vec<UserAgentRecord>> {
    let access = context.storage.get_user_agent_access(user_id)?;
    let current_agent_id = current_agent_id(context);
    let mut agents = context.storage.list_user_agents(user_id)?;
    agents.extend(context.storage.list_shared_user_agents(user_id)?);
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for agent in agents {
        if agent.agent_id.trim().is_empty() {
            continue;
        }
        if !seen.insert(agent.agent_id.clone()) {
            continue;
        }
        if !is_agent_allowed_by_access(user_id, access.as_ref(), &agent) {
            continue;
        }
        if !agent_in_hive(&agent, hive_id) {
            continue;
        }
        if !include_current
            && current_agent_id
                .as_deref()
                .is_some_and(|value| value == agent.agent_id.as_str())
        {
            continue;
        }
        output.push(agent);
    }
    output.sort_by(|a, b| {
        b.updated_at
            .partial_cmp(&a.updated_at)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
    Ok(output)
}

fn collect_swarm_runtime(
    context: &ToolContext<'_>,
    user_id: &str,
) -> Result<HashMap<String, AgentSwarmRuntime>> {
    let mut output = HashMap::new();
    for lock in context.storage.list_session_locks_by_user(user_id)? {
        let agent_id = lock.agent_id.trim();
        let session_id = lock.session_id.trim();
        if agent_id.is_empty() || session_id.is_empty() {
            continue;
        }
        output
            .entry(agent_id.to_string())
            .or_insert_with(AgentSwarmRuntime::default)
            .lock_sessions
            .insert(session_id.to_string());
    }
    if let Some(monitor) = context.monitor.as_ref() {
        for session in monitor.list_sessions(true) {
            let session_user_id = session.get("user_id").and_then(Value::as_str).unwrap_or("");
            if session_user_id.trim() != user_id {
                continue;
            }
            let agent_id = session
                .get("agent_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            let session_id = session
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if agent_id.is_empty() || session_id.is_empty() {
                continue;
            }
            output
                .entry(agent_id.to_string())
                .or_insert_with(AgentSwarmRuntime::default)
                .running_sessions
                .insert(session_id.to_string());
        }
    }
    Ok(output)
}

fn merge_swarm_active_sessions(runtime: Option<&AgentSwarmRuntime>) -> Vec<String> {
    let Some(runtime) = runtime else {
        return Vec::new();
    };
    let mut sessions = runtime.lock_sessions.clone();
    sessions.extend(runtime.running_sessions.clone());
    let mut output = sessions.into_iter().collect::<Vec<_>>();
    output.sort();
    output
}

fn monitor_session_status(context: &ToolContext<'_>, session_id: &str) -> Option<String> {
    context
        .monitor
        .as_ref()
        .and_then(|monitor| monitor.get_record(session_id))
        .and_then(|entry| {
            entry
                .get("status")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
}

fn ensure_swarm_target_not_current(
    target_agent_id: &str,
    current_agent_id: Option<&str>,
) -> Result<()> {
    if current_agent_id.is_some_and(|value| value == target_agent_id) {
        return Err(anyhow!(
            "agent_swarm only manages agents other than the current agent"
        ));
    }
    Ok(())
}

fn swarm_hive_arg(args: &Value) -> Option<&str> {
    args.get("hiveId")
        .or_else(|| args.get("hive_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn resolve_swarm_hive_id(
    context: &ToolContext<'_>,
    user_id: &str,
    requested_hive_id: Option<&str>,
) -> Result<String> {
    resolve_swarm_hive_scope(
        context.storage.as_ref(),
        user_id,
        current_agent_id(context).as_deref(),
        requested_hive_id,
    )
}

fn prepare_swarm_child_session(
    context: &ToolContext<'_>,
    task: &str,
    label: Option<String>,
    agent_id: &str,
) -> Result<PreparedChildSession> {
    let parent_session_id = context.session_id.trim();
    if parent_session_id.is_empty() {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    }
    let prepared = prepare_child_session(
        context,
        parent_session_id,
        task,
        label,
        Some(agent_id.to_string()),
        None,
        ChildSessionToolMode::UseTargetAgentDefaults,
    )?;
    let user_id = context.user_id.trim();
    if !user_id.is_empty() {
        if let Some(ref child_agent_id) = prepared.child_agent_id {
            bind_child_session_as_agent_main_thread(
                context.storage.as_ref(),
                user_id,
                child_agent_id,
                &prepared.child_session_id,
            )?;
        }
        if let Some(mut session) = context
            .storage
            .get_chat_session(user_id, &prepared.child_session_id)?
        {
            session.spawned_by = Some("agent_swarm".to_string());
            context.storage.upsert_chat_session(&session)?;
        }
    }
    Ok(prepared)
}

fn ensure_swarm_agent_in_hive(agent: &UserAgentRecord, hive_id: &str) -> Result<()> {
    ensure_swarm_agent_in_beeroom(agent, hive_id)
}

async fn sessions_list(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionListArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let limit = clamp_limit(payload.limit, 50, MAX_SESSION_LIST_ITEMS);
    let message_limit = clamp_limit(payload.message_limit, 0, MAX_SESSION_MESSAGE_ITEMS);
    let parent_id = normalize_optional_string(payload.parent_id);
    let (mut sessions, _) =
        context
            .storage
            .list_chat_sessions(user_id, None, parent_id.as_deref(), 0, limit)?;

    if let Some(active_minutes) = payload.active_minutes.filter(|value| *value > 0.0) {
        let cutoff = now_ts() - active_minutes * 60.0;
        sessions.retain(|record| record.updated_at >= cutoff);
    }
    let total = sessions.len() as i64;
    let mut items = Vec::with_capacity(sessions.len());
    for record in sessions {
        let status = context
            .monitor
            .as_ref()
            .and_then(|monitor| monitor.get_record(&record.session_id))
            .and_then(|entry| {
                entry
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string())
            });
        let messages = if message_limit > 0 {
            Some(
                load_session_messages(
                    context.workspace.clone(),
                    user_id.to_string(),
                    record.session_id.clone(),
                    message_limit,
                    false,
                )
                .await,
            )
        } else {
            None
        };
        let mut item = json!({
            "session_id": record.session_id,
            "title": record.title,
            "agent_id": record.agent_id,
            "updated_at": format_ts(record.updated_at),
            "last_message_at": format_ts(record.last_message_at),
            "parent_session_id": record.parent_session_id,
            "parent_message_id": record.parent_message_id,
            "spawn_label": record.spawn_label,
            "spawned_by": record.spawned_by,
            "status": status,
        });
        if let Some(messages) = messages {
            if let Value::Object(ref mut map) = item {
                map.insert("messages".to_string(), json!(messages));
            }
        }
        items.push(item);
    }
    Ok(build_model_tool_success(
        "list",
        "completed",
        format!("Listed {total} sessions."),
        json!({ "total": total, "items": items }),
    ))
}

async fn sessions_history(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionHistoryArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let session_id = resolve_session_key(payload.session_key)?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let limit = clamp_limit(payload.limit, 200, MAX_SESSION_HISTORY_ITEMS);
    let include_tools = payload.include_tools.unwrap_or(false);
    let record = context
        .storage
        .get_chat_session(user_id, &session_id)?
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let messages = load_session_messages(
        context.workspace.clone(),
        user_id.to_string(),
        record.session_id,
        limit,
        include_tools,
    )
    .await;
    Ok(build_model_tool_success(
        "history",
        "completed",
        format!(
            "Loaded {} messages from session {}.",
            messages.len(),
            session_id
        ),
        json!({ "session_id": session_id, "messages": messages }),
    ))
}

fn normalize_tool_run_state(status: &str) -> String {
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

fn build_session_tool_result(
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

fn build_agent_swarm_tool_result(
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

async fn sessions_send(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionSendArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let session_id = resolve_session_key(payload.session_key)?;
    let message = payload.message.trim().to_string();
    if message.is_empty() {
        return Err(anyhow!(i18n::t("error.content_required")));
    }
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let record = context
        .storage
        .get_chat_session(user_id, &session_id)?
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let agent_record = load_agent_record(
        context.storage.as_ref(),
        user_id,
        record.agent_id.as_deref(),
        true,
    )?;
    let tool_names = build_effective_tool_names(context, user_id, &record, agent_record.as_ref())?;
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let model_name = agent_record
        .as_ref()
        .and_then(|record| normalize_optional_string(record.model_name.clone()));
    let now = now_ts();
    let _ = context
        .storage
        .touch_chat_session(user_id, &session_id, now, now);
    let wait_forever = payload.wait_forever.unwrap_or(false);
    let timeout_seconds = payload.timeout_seconds.unwrap_or(0.0).max(0.0);
    let swarm_team_task_id = normalize_optional_string(payload.team_task_id);
    let auto_wake = should_auto_wake_parent_follow_up(
        swarm_team_task_id.is_some(),
        wait_forever,
        timeout_seconds,
    );
    let request = WunderRequest {
        user_id: user_id.to_string(),
        question: message,
        tool_names,
        skip_tool_calls: false,
        stream: true,
        debug_payload: false,
        session_id: Some(session_id.clone()),
        agent_id: record.agent_id.clone(),
        model_name: model_name.clone(),
        language: Some(i18n::get_language()),
        config_overrides: context.request_config_overrides.cloned(),
        agent_prompt,
        attachments: None,
        allow_queue: true,
        is_admin: context.is_admin,
        approval_tx: None,
    };
    let announce_label = normalize_optional_string(payload.label);
    let announce = build_parent_follow_up_announce(
        payload.announce_parent_session_id,
        &session_id,
        announce_label,
        payload.announce_emit_parent_events.unwrap_or(false),
        payload.announce_persist_history.unwrap_or(true),
        auto_wake,
        subagents::encode_parent_turn_ref(context.user_round, context.model_round),
        context.user_round,
        context.model_round,
    );

    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let (run_kind, requested_by) = if swarm_team_task_id.is_some() {
        ("swarm".to_string(), "agent_swarm".to_string())
    } else {
        ("subagent".to_string(), "subagent_control".to_string())
    };
    let receiver = spawn_session_run(
        context,
        request,
        run_id.clone(),
        Some(context.session_id.to_string()),
        record.agent_id.clone(),
        model_name,
        SessionRunMeta {
            run_kind: Some(run_kind),
            requested_by: Some(requested_by),
            team_task_id: swarm_team_task_id,
            ..SessionRunMeta::default()
        },
        announce,
        SessionCleanup::Keep,
        None,
    )
    .await?;

    if wait_forever {
        return match receiver.await {
            Ok(outcome) => {
                if outcome.status == "success" {
                    Ok(build_session_tool_result(
                        "send",
                        "ok",
                        Some(session_id.clone()),
                        run_id,
                        outcome.answer,
                        None,
                        Some(outcome.elapsed_s),
                        None,
                    ))
                } else {
                    Ok(build_session_tool_result(
                        "send",
                        &outcome.status,
                        Some(session_id.clone()),
                        run_id,
                        None,
                        Some(outcome.error.unwrap_or_else(|| "unknown".to_string())),
                        Some(outcome.elapsed_s),
                        None,
                    ))
                }
            }
            Err(err) => Ok(build_session_tool_result(
                "send",
                "error",
                Some(session_id.clone()),
                run_id,
                None,
                Some(err.to_string()),
                None,
                None,
            )),
        };
    }
    if timeout_seconds <= 0.0 {
        return Ok(build_session_tool_result(
            "send",
            "accepted",
            Some(session_id),
            run_id,
            None,
            None,
            None,
            Some(
                "send only queued the child turn. Use wait/status/history before treating the child reply as finished."
                    .to_string(),
            ),
        ));
    }
    let outcome = timeout(Duration::from_secs_f64(timeout_seconds), receiver).await;
    match outcome {
        Ok(Ok(outcome)) => {
            if outcome.status == "success" {
                Ok(build_session_tool_result(
                    "send",
                    "ok",
                    Some(session_id.clone()),
                    run_id,
                    outcome.answer,
                    None,
                    Some(outcome.elapsed_s),
                    None,
                ))
            } else {
                Ok(build_session_tool_result(
                    "send",
                    &outcome.status,
                    Some(session_id.clone()),
                    run_id,
                    None,
                    Some(outcome.error.unwrap_or_else(|| "unknown".to_string())),
                    Some(outcome.elapsed_s),
                    None,
                ))
            }
        }
        Ok(Err(err)) => Ok(build_session_tool_result(
            "send",
            "error",
            Some(session_id.clone()),
            run_id,
            None,
            Some(err.to_string()),
            None,
            None,
        )),
        Err(_) => Ok(build_session_tool_result(
            "send",
            "timeout",
            Some(session_id),
            run_id,
            None,
            Some("timeout".to_string()),
            None,
            Some(
                "The child run may still be executing. Use wait/status/history before retrying or reporting completion."
                    .to_string(),
            ),
        )),
    }
}

fn prepare_child_session(
    context: &ToolContext<'_>,
    parent_session_id: &str,
    task: &str,
    label: Option<String>,
    agent_id: Option<String>,
    model_name: Option<String>,
    tool_mode: ChildSessionToolMode,
) -> Result<PreparedChildSession> {
    let cleaned_task = task.trim();
    if cleaned_task.is_empty() {
        return Err(anyhow!(i18n::t("error.content_required")));
    }
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let cleaned_parent_session_id = parent_session_id.trim();
    if cleaned_parent_session_id.is_empty() {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    }

    let label = normalize_optional_string(label);
    let agent_id = normalize_optional_string(agent_id);
    let model_name = normalize_optional_string(model_name);
    let parent_record = context
        .storage
        .get_chat_session(user_id, cleaned_parent_session_id)
        .unwrap_or(None);
    let parent_agent_id = parent_record
        .as_ref()
        .and_then(|record| record.agent_id.clone())
        .or_else(|| context.agent_id.map(|value| value.to_string()));
    let parent_agent_record = load_agent_record(
        context.storage.as_ref(),
        user_id,
        parent_agent_id.as_deref(),
        true,
    )?;
    let parent_tool_names = if let Some(record) = parent_record.as_ref() {
        build_effective_tool_names(context, user_id, record, parent_agent_record.as_ref())?
    } else {
        finalize_tool_names(collect_user_allowed_tools(context, user_id)?)
    };

    let (child_agent_id, child_agent_record) = resolve_child_agent(
        context.storage.as_ref(),
        user_id,
        agent_id.as_deref(),
        parent_agent_id.as_deref(),
    )?;
    let child_tool_names = resolve_child_session_tool_names(
        tool_mode,
        &parent_tool_names,
        child_agent_record.as_ref(),
    );
    // Keep the first spawned child turn aligned with the same effective model
    // resolution used by the normal chat entrypoint.
    let resolved_model_name = model_name.or_else(|| {
        resolve_effective_agent_model_name(context.config, child_agent_record.as_ref())
    });
    let agent_prompt = child_agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());

    let now = now_ts();
    let child_session_id = format!("sess_{}", Uuid::new_v4().simple());
    let parent_turn_ref =
        subagents::encode_parent_turn_ref(context.user_round, context.model_round);
    let child_record = ChatSessionRecord {
        session_id: child_session_id.clone(),
        user_id: user_id.to_string(),
        title: label
            .clone()
            .unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string()),
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id: child_agent_id.clone(),
        tool_overrides: child_tool_names.clone(),
        parent_session_id: Some(cleaned_parent_session_id.to_string()),
        parent_message_id: parent_turn_ref.clone(),
        spawn_label: label.clone(),
        spawned_by: Some("model".to_string()),
    };
    context.storage.upsert_chat_session(&child_record)?;
    let run_metadata = build_prepared_child_run_metadata(
        context,
        cleaned_parent_session_id,
        &parent_tool_names,
        parent_turn_ref.as_deref(),
    );

    Ok(PreparedChildSession {
        child_session_id: child_session_id.clone(),
        child_agent_id: child_agent_id.clone(),
        model_name: resolved_model_name.clone(),
        request: WunderRequest {
            user_id: user_id.to_string(),
            question: cleaned_task.to_string(),
            tool_names: child_tool_names,
            skip_tool_calls: false,
            stream: true,
            debug_payload: false,
            session_id: Some(child_session_id),
            agent_id: child_agent_id,
            model_name: resolved_model_name,
            language: Some(i18n::get_language()),
            config_overrides: context.request_config_overrides.cloned(),
            agent_prompt,
            attachments: None,
            allow_queue: true,
            is_admin: context.is_admin,
            approval_tx: None,
        },
        announce: AnnounceConfig {
            parent_session_id: cleaned_parent_session_id.to_string(),
            label,
            dispatch_id: None,
            strategy: None,
            completion_mode: None,
            remaining_action: None,
            parent_turn_ref,
            parent_user_round: context.user_round,
            parent_model_round: context.model_round,
            emit_parent_events: true,
            // Background child runs wake the parent once they settle.
            // Waiting parent turns collect the result inline and disable auto_wake later.
            auto_wake: false,
            persist_history_message: false,
        },
        run_metadata,
    })
}

fn bind_child_session_as_agent_main_thread(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent_id: &str,
    child_session_id: &str,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_agent_id = agent_id.trim();
    let cleaned_child_session_id = child_session_id.trim();
    if cleaned_user_id.is_empty()
        || cleaned_agent_id.is_empty()
        || cleaned_child_session_id.is_empty()
    {
        return Ok(());
    }
    let now = now_ts();
    let existing_thread = storage.get_agent_thread(cleaned_user_id, cleaned_agent_id)?;
    let thread_record = AgentThreadRecord {
        thread_id: format!("thread_{cleaned_child_session_id}"),
        user_id: cleaned_user_id.to_string(),
        agent_id: cleaned_agent_id.to_string(),
        session_id: cleaned_child_session_id.to_string(),
        status: existing_thread
            .as_ref()
            .map(|record| record.status.trim().to_string())
            .filter(|status| !status.is_empty())
            .unwrap_or_else(|| "idle".to_string()),
        created_at: existing_thread
            .as_ref()
            .map(|record| record.created_at)
            .unwrap_or(now),
        updated_at: now,
    };
    storage.upsert_agent_thread(&thread_record)?;
    Ok(())
}

fn resolve_effective_agent_model_name(
    config: &Config,
    agent_record: Option<&UserAgentRecord>,
) -> Option<String> {
    if let Some(model_name) = agent_record
        .and_then(|record| normalize_optional_string(record.model_name.clone()))
        .filter(|model_name| {
            config
                .llm
                .models
                .get(model_name)
                .is_some_and(crate::services::llm::is_llm_model)
        })
    {
        return Some(model_name);
    }

    let default_model_name = config.llm.default.trim();
    if config
        .llm
        .models
        .get(default_model_name)
        .is_some_and(crate::services::llm::is_llm_model)
    {
        return Some(default_model_name.to_string());
    }

    config.llm.models.iter().find_map(|(name, model)| {
        if !crate::services::llm::is_llm_model(model) {
            return None;
        }
        let trimmed = name.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

async fn sessions_spawn(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionSpawnArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let parent_session_id = context.session_id.trim().to_string();
    let prepared = prepare_child_session(
        context,
        &parent_session_id,
        &payload.task,
        payload.label.clone(),
        payload.agent_id.clone(),
        payload.model.clone(),
        ChildSessionToolMode::InheritParentSession,
    )?;
    let PreparedChildSession {
        child_session_id,
        child_agent_id,
        model_name,
        request,
        mut announce,
        mut run_metadata,
    } = prepared;
    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let cleanup = parse_cleanup_mode(payload.cleanup.as_deref());
    let wait_seconds = payload.run_timeout_seconds.unwrap_or(0.0).max(0.0);
    sync_announce_auto_wake(
        &mut announce,
        Some(&mut run_metadata),
        should_auto_wake_parent_after_child_run(false, wait_seconds),
    );
    insert_run_metadata_field(&mut run_metadata, "spawn_mode", json!("single"));
    insert_run_metadata_field(
        &mut run_metadata,
        "cleanup",
        json!(session_cleanup_label(cleanup)),
    );
    insert_run_metadata_field(
        &mut run_metadata,
        "run_timeout_seconds",
        json!(wait_seconds),
    );
    insert_run_metadata_field(&mut run_metadata, "background", json!(wait_seconds <= 0.0));
    let mut receiver = spawn_session_run(
        context,
        request,
        run_id.clone(),
        Some(parent_session_id),
        child_agent_id,
        model_name,
        SessionRunMeta {
            run_kind: Some("subagent".to_string()),
            requested_by: Some("subagent_control".to_string()),
            metadata: Some(run_metadata),
            ..SessionRunMeta::default()
        },
        Some(announce),
        cleanup,
        payload.run_timeout_seconds,
    )
    .await?;
    if wait_seconds <= 0.0 {
        return Ok(build_session_tool_result(
            "spawn",
            "accepted",
            Some(child_session_id),
            run_id,
            None,
            None,
            None,
            Some(
                "spawn already dispatched the initial child task. Use status/wait/history before sending another turn or reporting completion."
                    .to_string(),
            ),
        ));
    }
    let summary = i18n::t("monitor.summary.subagent_wait");
    let wait_payload = json!({
        "stage": "subagent_wait",
        "summary": summary,
        "run_id": run_id,
        "child_session_id": child_session_id.clone()
    });
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit("progress", wait_payload);
    }
    let start_wait = Instant::now();
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    let deadline = tokio::time::sleep(Duration::from_secs_f64(wait_seconds));
    tokio::pin!(deadline);
    let outcome = loop {
        tokio::select! {
            result = &mut receiver => {
                break Ok(result);
            }
            _ = heartbeat.tick() => {
                if let Some(emitter) = context.event_emitter.as_ref() {
                    emitter.emit("progress", json!({
                        "stage": "subagent_wait",
                        "summary": i18n::t("monitor.summary.subagent_wait"),
                        "run_id": run_id,
                        "child_session_id": child_session_id.clone(),
                        "elapsed_s": start_wait.elapsed().as_secs_f64()
                    }));
                }
            }
            _ = &mut deadline => {
                break Err("timeout");
            }
        }
    };
    match outcome {
        Ok(Ok(outcome)) => {
            if outcome.status == "success" {
                Ok(build_session_tool_result(
                    "spawn",
                    "ok",
                    Some(child_session_id.clone()),
                    run_id,
                    outcome.answer,
                    None,
                    Some(outcome.elapsed_s),
                    None,
                ))
            } else {
                Ok(build_session_tool_result(
                    "spawn",
                    &outcome.status,
                    Some(child_session_id.clone()),
                    run_id,
                    None,
                    Some(outcome.error.unwrap_or_else(|| "unknown".to_string())),
                    Some(outcome.elapsed_s),
                    None,
                ))
            }
        }
        Ok(Err(err)) => Ok(build_session_tool_result(
            "spawn",
            "error",
            Some(child_session_id.clone()),
            run_id,
            None,
            Some(err.to_string()),
            None,
            None,
        )),
        Err(_) => Ok(build_session_tool_result(
            "spawn",
            "timeout",
            Some(child_session_id),
            run_id,
            None,
            Some("timeout".to_string()),
            None,
            Some(
                "The child session may still be running. Use status/wait/history before retrying or sending another turn."
                    .to_string(),
            ),
        )),
    }
}

fn resolve_child_agent(
    storage: &dyn StorageBackend,
    user_id: &str,
    requested_agent_id: Option<&str>,
    parent_agent_id: Option<&str>,
) -> Result<(Option<String>, Option<UserAgentRecord>)> {
    let requested = requested_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(requested) = requested {
        if let Some(record) = load_agent_record(storage, user_id, Some(requested), true)? {
            return Ok((Some(record.agent_id.clone()), Some(record)));
        }
    }
    let parent = parent_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(parent) = parent {
        if let Some(record) = load_agent_record(storage, user_id, Some(parent), true)? {
            return Ok((Some(record.agent_id.clone()), Some(record)));
        }
    }
    Ok((None, None))
}

fn session_run_runtime() -> &'static tokio::runtime::Runtime {
    static SESSION_RUN_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    SESSION_RUN_RUNTIME.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|parallelism| parallelism.get().clamp(8, 128))
            .unwrap_or(16);
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(threads)
            .max_blocking_threads(1024)
            .thread_name("wunder-session-run")
            .enable_all()
            .build()
            .expect("build session run runtime")
    })
}

#[allow(clippy::too_many_arguments)]
async fn spawn_session_run(
    context: &ToolContext<'_>,
    request: WunderRequest,
    run_id: String,
    parent_session_id: Option<String>,
    agent_id: Option<String>,
    model_name: Option<String>,
    run_meta: SessionRunMeta,
    announce: Option<AnnounceConfig>,
    cleanup: SessionCleanup,
    run_timeout_s: Option<f64>,
) -> Result<oneshot::Receiver<SessionRunOutcome>> {
    let orchestrator = context
        .orchestrator
        .clone()
        .ok_or_else(|| anyhow!(i18n::t("error.internal_error")))?;
    let session_id = request
        .session_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let user_id = request.user_id.clone();
    let request_config_overrides = request.config_overrides.clone();
    let mut run_metadata = match run_meta.metadata.clone() {
        Some(Value::Object(map)) => Value::Object(map),
        _ => json!({}),
    };
    let user_message_preview =
        truncate_text(request.question.trim(), SUBAGENT_MESSAGE_PREVIEW_MAX_CHARS);
    if !user_message_preview.is_empty() {
        insert_run_metadata_field(
            &mut run_metadata,
            "user_message_preview",
            json!(user_message_preview),
        );
    }
    if let Some(team_task_id) = run_meta.team_task_id.as_deref() {
        insert_run_metadata_field(&mut run_metadata, "team_task_id", json!(team_task_id));
    }
    let now = now_ts();
    let record = SessionRunRecord {
        run_id: run_id.clone(),
        session_id: session_id.clone(),
        parent_session_id: parent_session_id.clone(),
        user_id: user_id.clone(),
        dispatch_id: run_meta.dispatch_id.clone(),
        run_kind: run_meta.run_kind.clone(),
        requested_by: run_meta.requested_by.clone(),
        agent_id: agent_id.clone(),
        model_name: model_name.clone(),
        status: "queued".to_string(),
        queued_time: now,
        started_time: 0.0,
        finished_time: 0.0,
        elapsed_s: 0.0,
        result: None,
        error: None,
        updated_time: now,
        metadata: Some(run_metadata),
    };
    {
        let queued_storage = context.storage.clone();
        let queued_record = record.clone();
        tokio::task::spawn_blocking(move || queued_storage.upsert_session_run(&queued_record))
            .await
            .map_err(|err| anyhow!(err.to_string()))??;
    }

    let storage = context.storage.clone();
    let workspace = context.workspace.clone();
    let monitor = context.monitor.clone();
    let beeroom_realtime = context.beeroom_realtime.clone();
    let swarm_team_task_id = run_meta.team_task_id.clone();
    let (tx, rx) = oneshot::channel::<SessionRunOutcome>();
    let announce_for_start = announce.clone();
    tokio::spawn(async move {
        let started = now_ts();
        let running = SessionRunRecord {
            status: "running".to_string(),
            started_time: started,
            updated_time: started,
            ..record.clone()
        };
        {
            let storage_for_start = storage.clone();
            let user_for_start = user_id.clone();
            let session_for_start = session_id.clone();
            let running_for_start = running.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = storage_for_start.touch_chat_session(
                    &user_for_start,
                    &session_for_start,
                    started,
                    started,
                );
                let _ = storage_for_start.upsert_session_run(&running_for_start);
            })
            .await;
        }
        if let Some(config) = announce_for_start
            .as_ref()
            .filter(|entry| entry.emit_parent_events)
        {
            let _ = subagents::emit_child_runtime_update(
                storage.clone(),
                monitor.as_ref().map(Arc::clone),
                &user_id,
                &config.parent_session_id,
                &session_id,
            )
            .await;
        }

        // Use a dedicated runtime so high fan-out runs do not contend with the main runtime worker pool.
        let mut run_request = request;
        run_request.stream = true;
        let child_orchestrator = orchestrator.clone();
        let mut run_handle = tokio::task::spawn_blocking(move || {
            session_run_runtime().block_on(session_run_stream::run_request(
                child_orchestrator,
                run_request,
            ))
        });
        let mut timeout_triggered = false;
        let run_result = if let Some(timeout_s) = run_timeout_s.filter(|value| *value > 0.0) {
            let timeout_duration = Duration::from_secs_f64(timeout_s);
            tokio::select! {
                res = &mut run_handle => match res {
                    Ok(value) => value,
                    Err(err) => Err(anyhow!(err.to_string())),
                },
                _ = sleep(timeout_duration) => {
                    timeout_triggered = true;
                    run_handle.abort();
                    if let Some(monitor) = monitor.as_ref() {
                        let _ = monitor.cancel(&session_id);
                    }
                    Err(anyhow!("timeout"))
                }
            }
        } else {
            match run_handle.await {
                Ok(value) => value,
                Err(err) => Err(anyhow!(err.to_string())),
            }
        };
        let finished = now_ts();
        let elapsed = (finished - started).max(0.0);
        let (status, answer, error) = match run_result {
            Ok(response) => {
                let answer =
                    truncate_tool_result_text(response.answer.as_deref().unwrap_or_default());
                ("success".to_string(), Some(answer), None)
            }
            Err(err) => {
                if timeout_triggered {
                    ("timeout".to_string(), None, Some("timeout".to_string()))
                } else {
                    (
                        "error".to_string(),
                        None,
                        Some(truncate_tool_result_text(&err.to_string())),
                    )
                }
            }
        };
        let finished_record = SessionRunRecord {
            status: status.clone(),
            finished_time: finished,
            elapsed_s: elapsed,
            result: answer.clone(),
            error: error.clone(),
            updated_time: finished,
            ..running
        };
        {
            let storage_for_finish = storage.clone();
            let finished_for_write = finished_record.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = storage_for_finish.upsert_session_run(&finished_for_write);
            })
            .await;
        }
        if let Some(team_task_id) = swarm_team_task_id.as_deref() {
            if let Err(err) = reconcile_swarm_task_from_session_run(
                storage.as_ref(),
                monitor.as_ref().map(Arc::clone),
                beeroom_realtime.as_ref().map(Arc::clone),
                team_task_id,
                &finished_record,
            ) {
                warn!(
                    task_id = team_task_id,
                    run_id = %finished_record.run_id,
                    "failed to reconcile swarm task from session run: {err}"
                );
            }
        }

        if let Some(announce) = announce {
            if announce.persist_history_message && !should_skip_announce(answer.as_deref()) {
                append_child_announce(
                    &workspace,
                    &storage,
                    &user_id,
                    &announce.parent_session_id,
                    &session_id,
                    &run_id,
                    &status,
                    answer.as_deref(),
                    error.as_deref(),
                    elapsed,
                    model_name.as_deref(),
                    announce.label.as_deref(),
                );
            }
            if announce.emit_parent_events || announce.auto_wake {
                let parent_dispatch = subagents::ParentDispatchConfig {
                    parent_session_id: announce.parent_session_id.clone(),
                    dispatch_id: announce.dispatch_id.clone(),
                    strategy: announce.strategy.clone(),
                    completion_mode: announce.completion_mode.clone(),
                    remaining_action: announce.remaining_action.clone(),
                    label: announce.label.clone(),
                    parent_turn_ref: announce.parent_turn_ref.clone(),
                    parent_user_round: announce.parent_user_round,
                    parent_model_round: announce.parent_model_round,
                    emit_parent_events: announce.emit_parent_events,
                    auto_wake: announce.auto_wake,
                };
                subagents::handle_child_completion(
                    storage.clone(),
                    monitor.as_ref().map(Arc::clone),
                    orchestrator.clone(),
                    user_id.clone(),
                    session_id.clone(),
                    run_id.clone(),
                    answer.clone(),
                    error.clone(),
                    request_config_overrides.clone(),
                    parent_dispatch,
                )
                .await;
            }
        }
        if matches!(cleanup, SessionCleanup::Delete) {
            cleanup_session(
                &storage,
                &workspace,
                monitor.as_ref(),
                &user_id,
                &session_id,
            );
        }

        let _ = tx.send(SessionRunOutcome {
            status,
            answer,
            error,
            elapsed_s: elapsed,
        });
    });
    Ok(rx)
}

fn parse_cleanup_mode(value: Option<&str>) -> SessionCleanup {
    match value.unwrap_or("").trim().to_lowercase().as_str() {
        "delete" | "remove" => SessionCleanup::Delete,
        _ => SessionCleanup::Keep,
    }
}

fn cleanup_session(
    storage: &Arc<dyn StorageBackend>,
    workspace: &WorkspaceManager,
    monitor: Option<&Arc<MonitorState>>,
    user_id: &str,
    session_id: &str,
) {
    workspace.purge_session_data(user_id, session_id);
    let _ = storage.delete_chat_session(user_id, session_id);
    if let Some(monitor) = monitor {
        let _ = monitor.purge_session(session_id);
    }
}

#[allow(clippy::too_many_arguments)]
fn append_child_announce(
    workspace: &WorkspaceManager,
    storage: &Arc<dyn StorageBackend>,
    user_id: &str,
    parent_session_id: &str,
    child_session_id: &str,
    run_id: &str,
    status: &str,
    answer: Option<&str>,
    error: Option<&str>,
    elapsed_s: f64,
    model_name: Option<&str>,
    label: Option<&str>,
) {
    let result_text = if status == "success" {
        answer.unwrap_or("ok").trim()
    } else {
        error.unwrap_or("error").trim()
    };
    let mut notes = vec![
        format!("run_id={run_id}"),
        format!("session_id={child_session_id}"),
        format!("elapsed_s={:.2}", elapsed_s),
    ];
    if let Some(model) = model_name {
        if !model.trim().is_empty() {
            notes.push(format!("model={}", model.trim()));
        }
    }
    if let Some(label) = label {
        if !label.trim().is_empty() {
            notes.push(format!("label={}", label.trim()));
        }
    }
    let content = format!(
        "Status: {status}\nResult: {result}\nNotes: {notes}",
        status = status,
        result = result_text,
        notes = notes.join(", ")
    );
    let timestamp = Local::now().to_rfc3339();
    let meta = json!({
        "type": "subagent_announce",
        "run_id": run_id,
        "child_session_id": child_session_id,
        "status": status,
        "elapsed_s": elapsed_s
    });
    let payload = json!({
        "role": "assistant",
        "content": content,
        "session_id": parent_session_id,
        "timestamp": timestamp,
        "meta": meta,
    });
    let _ = workspace.append_chat(user_id, &payload);
    let now = now_ts();
    let _ = storage.touch_chat_session(user_id, parent_session_id, now, now);
}

fn should_skip_announce(answer: Option<&str>) -> bool {
    answer
        .map(|value| value.trim() == ANNOUNCE_SKIP)
        .unwrap_or(false)
}

async fn load_session_messages(
    workspace: Arc<WorkspaceManager>,
    user_id: String,
    session_id: String,
    limit: i64,
    include_tools: bool,
) -> Vec<Value> {
    tokio::task::spawn_blocking(move || {
        if include_tools {
            let history = workspace
                .load_history(&user_id, &session_id, limit)
                .unwrap_or_default();
            history
                .into_iter()
                .filter(|item| item.get("role").and_then(Value::as_str) != Some("system"))
                .collect()
        } else {
            let manager = HistoryManager;
            manager.load_history_messages(&workspace, &user_id, &session_id, limit)
        }
    })
    .await
    .unwrap_or_default()
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn current_agent_id(context: &ToolContext<'_>) -> Option<String> {
    context
        .agent_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_session_key(value: Option<String>) -> Result<String> {
    let Some(key) = normalize_optional_string(value) else {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    };
    Ok(key)
}

fn clamp_limit(value: Option<i64>, default: i64, max: i64) -> i64 {
    value.unwrap_or(default).max(0).min(max)
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn format_ts(ts: f64) -> String {
    let millis = (ts * 1000.0) as i64;
    chrono::DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|dt| dt.with_timezone(&Local).to_rfc3339())
        .unwrap_or_default()
}

fn truncate_text(text: &str, max_chars: usize) -> String {
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

fn collect_user_allowed_tools(context: &ToolContext<'_>, user_id: &str) -> Result<HashSet<String>> {
    let mut allowed =
        collect_available_tool_names(context.config, context.skills, context.user_tool_bindings);
    let access = context.storage.get_user_tool_access(user_id)?;
    if let Some(access) = access {
        if let Some(allowed_tools) = access
            .allowed_tools
            .as_ref()
            .filter(|items| !items.is_empty())
        {
            let allowed_set: HashSet<String> = allowed_tools
                .iter()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect();
            allowed = allowed
                .intersection(&allowed_set)
                .cloned()
                .collect::<HashSet<_>>();
        }
    }
    Ok(allowed)
}

fn normalize_tool_overrides(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let mut has_none = false;
    for raw in values {
        let name = raw.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        if name == TOOL_OVERRIDE_NONE {
            has_none = true;
        }
        seen.insert(name.clone());
        output.push(name);
    }
    if has_none {
        vec![TOOL_OVERRIDE_NONE.to_string()]
    } else {
        output
    }
}

fn resolve_session_tool_overrides(
    record: &ChatSessionRecord,
    frozen_tool_overrides: Option<&[String]>,
    agent: Option<&UserAgentRecord>,
) -> Vec<String> {
    if !record.tool_overrides.is_empty() {
        normalize_tool_overrides(record.tool_overrides.clone())
    } else if let Some(snapshot) = frozen_tool_overrides {
        normalize_tool_overrides(snapshot.to_vec())
    } else {
        resolve_agent_tool_defaults(agent)
    }
}

fn resolve_agent_tool_defaults(agent: Option<&UserAgentRecord>) -> Vec<String> {
    let Some(record) = agent else {
        return Vec::new();
    };
    resolve_agent_runtime_tool_names(
        &record.tool_names,
        &record.declared_tool_names,
        &record.declared_skill_names,
    )
}

fn resolve_child_session_tool_names(
    mode: ChildSessionToolMode,
    parent_tool_names: &[String],
    child_agent: Option<&UserAgentRecord>,
) -> Vec<String> {
    match mode {
        ChildSessionToolMode::InheritParentSession => parent_tool_names.to_vec(),
        ChildSessionToolMode::UseTargetAgentDefaults => {
            let defaults = resolve_agent_tool_defaults(child_agent);
            if defaults.is_empty() {
                parent_tool_names.to_vec()
            } else {
                defaults
            }
        }
    }
}

fn apply_tool_overrides(
    allowed: HashSet<String>,
    overrides: &[String],
    config: &Config,
    skills: &SkillRegistry,
) -> HashSet<String> {
    if overrides.is_empty() {
        return allowed;
    }
    if overrides.iter().any(|name| name == TOOL_OVERRIDE_NONE) {
        return HashSet::new();
    }
    let mut filtered = HashSet::new();
    for raw in overrides {
        if let Some(mapped) = resolve_override_name_with_allowed(raw, &allowed) {
            filtered.insert(mapped);
        }
    }
    if config.server.mode.trim().eq_ignore_ascii_case("desktop") {
        let skill_names: HashSet<String> = skills
            .list_specs()
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        for name in &allowed {
            if skill_names.contains(name) {
                filtered.insert(name.clone());
            }
        }
    }
    filtered
}

fn resolve_override_name_with_allowed(raw: &str, allowed: &HashSet<String>) -> Option<String> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return None;
    }
    if allowed.contains(cleaned) {
        return Some(cleaned.to_string());
    }
    for (index, _) in cleaned.match_indices('@') {
        let suffix = cleaned[index + 1..].trim();
        if !suffix.is_empty() && allowed.contains(suffix) {
            return Some(suffix.to_string());
        }
    }
    None
}

fn finalize_tool_names(mut allowed: HashSet<String>) -> Vec<String> {
    if allowed.is_empty() {
        return vec![TOOL_OVERRIDE_NONE.to_string()];
    }
    let mut list = allowed.drain().collect::<Vec<_>>();
    list.sort();
    list
}

fn build_effective_tool_names(
    context: &ToolContext<'_>,
    user_id: &str,
    record: &ChatSessionRecord,
    agent: Option<&UserAgentRecord>,
) -> Result<Vec<String>> {
    let allowed = collect_user_allowed_tools(context, user_id)?;
    let frozen_tool_overrides = context
        .workspace
        .load_session_frozen_tool_overrides(user_id, &record.session_id);
    let overrides = resolve_session_tool_overrides(record, frozen_tool_overrides.as_deref(), agent);
    let allowed = apply_tool_overrides(allowed, &overrides, context.config, context.skills);
    Ok(finalize_tool_names(allowed))
}

fn is_agent_allowed_by_access(
    user_id: &str,
    access: Option<&UserAgentAccessRecord>,
    agent: &UserAgentRecord,
) -> bool {
    if agent.user_id != user_id && !agent.is_shared {
        return false;
    }
    if let Some(access) = access {
        if !access.blocked_agent_ids.is_empty()
            && access
                .blocked_agent_ids
                .iter()
                .any(|id| id == &agent.agent_id)
        {
            return false;
        }
        if let Some(allowed) = access.allowed_agent_ids.as_ref() {
            return allowed.iter().any(|id| id == &agent.agent_id);
        }
    }
    true
}

fn load_agent_record(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent_id: Option<&str>,
    allow_missing: bool,
) -> Result<Option<UserAgentRecord>> {
    let Some(agent_id) = agent_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let record = if is_default_agent_alias_value(agent_id) {
        Some(build_default_agent_record_from_storage(storage, user_id)?)
    } else {
        storage.get_user_agent_by_id(agent_id)?
    };
    let Some(record) = record else {
        if allow_missing {
            return Ok(None);
        }
        return Err(anyhow!(i18n::t("error.agent_not_found")));
    };
    let access = storage.get_user_agent_access(user_id)?;
    if !is_agent_allowed_by_access(user_id, access.as_ref(), &record) {
        if allow_missing {
            return Ok(None);
        }
        return Err(anyhow!(i18n::t("error.agent_not_found")));
    }
    Ok(Some(record))
}

fn is_default_agent_alias_value(raw: &str) -> bool {
    let cleaned = raw.trim();
    cleaned.eq_ignore_ascii_case("__default__") || cleaned.eq_ignore_ascii_case("default")
}

async fn execute_user_tool(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    match alias.kind {
        UserToolKind::Mcp => execute_user_mcp_tool(context, alias, args).await,
        UserToolKind::Skill => execute_user_skill(context, alias, args).await,
        UserToolKind::Knowledge => execute_user_knowledge(context, alias, args).await,
    }
}

async fn execute_user_skill(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    let manager = context
        .user_tool_manager
        .as_ref()
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_loaded")))?;
    let bindings = context
        .user_tool_bindings
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_loaded")))?;
    let registry = manager
        .get_user_skill_registry(context.config, bindings, &alias.owner_id)
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_loaded")))?;
    let spec = registry
        .get(&alias.target)
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_found")))?;
    let result = execute_skill(&spec, args, 60).await.map_err(|err| {
        anyhow!(i18n::t_with_params(
            "tool.invoke.user_skill_failed",
            &HashMap::from([("detail".to_string(), err.to_string())]),
        ))
    })?;
    context.workspace.mark_tree_dirty(context.workspace_id);
    Ok(result)
}

async fn execute_user_mcp_tool(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    let target = alias.target.trim();
    let Some((server_name, tool_name)) = split_mcp_target(target) else {
        return Err(anyhow!(i18n::t("tool.invoke.mcp_name_invalid")));
    };
    let bindings = context
        .user_tool_bindings
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.mcp_server_unavailable")))?;
    let server_map = bindings.mcp_servers.get(&alias.owner_id);
    let server_config = server_map.and_then(|map| map.get(server_name));
    let Some(server_config) = server_config else {
        return Err(anyhow!(i18n::t("tool.invoke.mcp_server_unavailable")));
    };
    let result = mcp::call_tool_with_server(context.config, server_config, tool_name, args)
        .await
        .map_err(|err| {
            anyhow!(i18n::t_with_params(
                "tool.invoke.mcp_call_failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            ))
        })?;
    if result
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(anyhow!(i18n::t("tool.invoke.mcp_result_error")));
    }
    Ok(build_model_tool_success(
        "mcp_call",
        "completed",
        format!("Called MCP tool {tool_name}@{server_name}."),
        json!({
            "server": server_name,
            "tool": tool_name,
            "result": result
        }),
    ))
}

async fn execute_user_knowledge(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    let Some(query) = resolve_query_text(args) else {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    };
    let store = context
        .user_tool_store
        .ok_or_else(|| anyhow!(i18n::t("error.knowledge_base_not_found")))?;
    let payload = store.load_user_tools(&alias.owner_id);
    let base_info = payload
        .knowledge_bases
        .iter()
        .find(|base| base.name == alias.target)
        .cloned()
        .ok_or_else(|| anyhow!(i18n::t("error.knowledge_base_not_found")))?;
    let base_type = normalize_knowledge_base_type(base_info.base_type.as_deref());
    let root = store
        .resolve_knowledge_base_root_with_type(&alias.owner_id, &base_info.name, base_type, false)
        .map_err(|err| anyhow!(err.to_string()))?;
    let base = KnowledgeBaseConfig {
        name: base_info.name.clone(),
        description: base_info.description.clone(),
        root: root.to_string_lossy().to_string(),
        enabled: base_info.enabled,
        shared: Some(base_info.shared),
        base_type: base_info.base_type.clone(),
        embedding_model: base_info.embedding_model.clone(),
        chunk_size: base_info.chunk_size,
        chunk_overlap: base_info.chunk_overlap,
        top_k: base_info.top_k,
        score_threshold: base_info.score_threshold,
    };
    if base_type == KnowledgeBaseType::Vector {
        return execute_vector_knowledge(context, &base, Some(&alias.owner_id), args).await;
    }
    let llm_config = knowledge::resolve_llm_config(context.config, None);
    let docs = if let Some(emitter) = context.event_emitter.as_ref() {
        let include_payload = is_debug_log_level(&context.config.observability.log_level);
        let log_request = |mut payload: Value| {
            if !include_payload {
                if let Value::Object(ref mut map) = payload {
                    map.remove("payload");
                }
            }
            emitter.emit("knowledge_request", payload);
        };
        knowledge::query_knowledge_documents(
            &query,
            &base,
            llm_config.as_ref(),
            extract_limit(args),
            Some(&log_request),
        )
        .await
    } else {
        knowledge::query_knowledge_documents(
            &query,
            &base,
            llm_config.as_ref(),
            extract_limit(args),
            None,
        )
        .await
    };
    let documents = docs
        .into_iter()
        .map(|doc| doc.to_value())
        .collect::<Vec<_>>();
    Ok(build_knowledge_tool_success(
        &base.name,
        Some(&query),
        &[],
        None,
        false,
        documents,
        None,
    ))
}

async fn execute_knowledge_tool(
    context: &ToolContext<'_>,
    base: &KnowledgeBaseConfig,
    args: &Value,
) -> Result<Value> {
    let Some(query) = resolve_query_text(args) else {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    };
    if base.is_vector() {
        return execute_vector_knowledge(context, base, None, args).await;
    }
    let _ =
        knowledge::resolve_knowledge_root(base, false).map_err(|err| anyhow!(err.to_string()))?;
    let llm_config = knowledge::resolve_llm_config(context.config, None);
    let docs = if let Some(emitter) = context.event_emitter.as_ref() {
        let include_payload = is_debug_log_level(&context.config.observability.log_level);
        let log_request = |mut payload: Value| {
            if !include_payload {
                if let Value::Object(ref mut map) = payload {
                    map.remove("payload");
                }
            }
            emitter.emit("knowledge_request", payload);
        };
        knowledge::query_knowledge_documents(
            &query,
            base,
            llm_config.as_ref(),
            extract_limit(args),
            Some(&log_request),
        )
        .await
    } else {
        knowledge::query_knowledge_documents(
            &query,
            base,
            llm_config.as_ref(),
            extract_limit(args),
            None,
        )
        .await
    };
    let documents = docs
        .into_iter()
        .map(|doc| doc.to_value())
        .collect::<Vec<_>>();
    Ok(build_knowledge_tool_success(
        &base.name,
        Some(&query),
        &[],
        None,
        false,
        documents,
        None,
    ))
}

async fn execute_vector_knowledge(
    context: &ToolContext<'_>,
    base: &KnowledgeBaseConfig,
    owner_id: Option<&str>,
    args: &Value,
) -> Result<Value> {
    let keywords = extract_keywords(args);
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let queries = if !keywords.is_empty() {
        keywords
    } else if !query.is_empty() {
        vec![query.clone()]
    } else {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    };
    vector_knowledge::ensure_vector_base_config(base)?;
    let embedding_name = base.embedding_model.as_deref().unwrap_or("").trim();
    let embed_config = vector_knowledge::resolve_embedding_model(context.config, embedding_name)?;
    let timeout_s = embed_config.timeout_s.unwrap_or(120);
    let vectors = embed_texts(&embed_config, &queries, timeout_s).await?;
    if vectors.len() != queries.len() {
        return Err(anyhow!("embedding response size mismatch"));
    }
    let client = vector_knowledge::resolve_weaviate_client(context.config)?;
    let owner_key = vector_knowledge::resolve_owner_key(owner_id);
    let top_k = extract_limit(args).unwrap_or_else(|| vector_knowledge::resolve_top_k(base));
    let base_name = base.name.clone();
    let embedding_name = embedding_name.to_string();
    let query_results =
        futures::future::join_all(vectors.into_iter().enumerate().map(|(index, vector)| {
            let client = client.clone();
            let owner_key = owner_key.clone();
            let base_name = base_name.clone();
            let embedding_name = embedding_name.clone();
            let keyword = queries.get(index).cloned().unwrap_or_default();
            async move {
                let mut hits = client
                    .query_chunks(&owner_key, &base_name, &embedding_name, &vector, top_k)
                    .await?;
                if let Some(threshold) = base.score_threshold {
                    hits.retain(|hit| hit.score.unwrap_or(0.0) >= f64::from(threshold));
                }
                if hits.len() > top_k {
                    hits.truncate(top_k);
                }
                Ok::<_, anyhow::Error>((index, keyword, hits))
            }
        }))
        .await;
    let mut aggregated = Vec::new();
    for result in query_results {
        aggregated.push(result?);
    }
    aggregated.sort_by_key(|(index, _, _)| *index);
    if let Some(emitter) = context.event_emitter.as_ref() {
        let mut payload = json!({
            "knowledge_base": base.name,
            "vector": true,
            "embedding_model": embedding_name.clone(),
            "owner_id": owner_key,
            "limit": top_k,
            "score_threshold": base.score_threshold
        });
        if queries.len() == 1 {
            payload["query"] = json!(queries[0].clone());
        } else {
            payload["keywords"] = json!(queries.clone());
        }
        emitter.emit("knowledge_request", payload);
    }
    let mut grouped_results = Vec::new();
    let mut flat_documents = Vec::new();
    let multi_query = queries.len() > 1;
    let mut seen_chunks = HashSet::new();
    for (_, keyword, hits) in aggregated {
        let documents = hits
            .into_iter()
            .filter_map(|hit| {
                if multi_query {
                    let key = format!("{}::{}", hit.doc_id, hit.chunk_index);
                    if !seen_chunks.insert(key) {
                        return None;
                    }
                }
                let mut doc = json!({
                    "doc_id": hit.doc_id,
                    "document": hit.doc_name,
                    "name": hit.doc_name,
                    "chunk_index": hit.chunk_index,
                    "start": hit.start,
                    "end": hit.end,
                    "content": hit.content,
                    "embedding_model": hit.embedding_model,
                    "score": hit.score
                });
                if multi_query {
                    doc["keyword"] = json!(keyword);
                }
                Some(doc)
            })
            .collect::<Vec<_>>();
        if multi_query {
            flat_documents.extend(documents.clone());
        }
        grouped_results.push(json!({
            "keyword": keyword,
            "documents": documents
        }));
    }
    let documents = if queries.len() == 1 {
        grouped_results
            .first()
            .and_then(|value| value.get("documents"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    } else {
        flat_documents
    };
    Ok(build_knowledge_tool_success(
        &base.name,
        if query.is_empty() {
            None
        } else {
            Some(query.as_str())
        },
        &queries,
        Some(embedding_name.as_str()),
        true,
        documents,
        (queries.len() > 1).then_some(grouped_results),
    ))
}

fn split_mcp_target(target: &str) -> Option<(&str, &str)> {
    let mut parts = target.splitn(2, '@');
    let server = parts.next()?.trim();
    let tool = parts.next()?.trim();
    if server.is_empty() || tool.is_empty() {
        None
    } else {
        Some((server, tool))
    }
}

fn find_knowledge_base(config: &Config, name: &str) -> Option<KnowledgeBaseConfig> {
    config
        .knowledge
        .bases
        .iter()
        .find(|base| base.enabled && base.name == name && !base.root.trim().is_empty())
        .cloned()
}

fn compact_knowledge_document_for_model(item: &Value) -> Value {
    json!({
        "document": item.get("document").cloned().unwrap_or(Value::Null),
        "name": item.get("name").cloned().unwrap_or(Value::Null),
        "section_path": item.get("section_path").cloned().unwrap_or(Value::Null),
        "content": item.get("content").cloned().unwrap_or(Value::Null),
        "score": item.get("score").cloned().unwrap_or(Value::Null),
        "reason": item.get("reason").cloned().unwrap_or(Value::Null),
    })
}

fn compact_vector_knowledge_document_for_model(item: &Value) -> Value {
    json!({
        "doc_id": item.get("doc_id").cloned().unwrap_or(Value::Null),
        "document": item.get("document").cloned().unwrap_or(Value::Null),
        "chunk_index": item.get("chunk_index").cloned().unwrap_or(Value::Null),
        "start": item.get("start").cloned().unwrap_or(Value::Null),
        "end": item.get("end").cloned().unwrap_or(Value::Null),
        "content": item.get("content").cloned().unwrap_or(Value::Null),
        "score": item.get("score").cloned().unwrap_or(Value::Null),
        "keyword": item.get("keyword").cloned().unwrap_or(Value::Null),
    })
}

fn build_knowledge_tool_success(
    base_name: &str,
    query: Option<&str>,
    queries: &[String],
    embedding_model: Option<&str>,
    vector: bool,
    documents: Vec<Value>,
    grouped_queries: Option<Vec<Value>>,
) -> Value {
    let compact_documents = documents
        .iter()
        .map(|item| {
            if vector {
                compact_vector_knowledge_document_for_model(item)
            } else {
                compact_knowledge_document_for_model(item)
            }
        })
        .collect::<Vec<_>>();
    let count = compact_documents.len();
    let mut data = json!({
        "knowledge_base": base_name,
        "vector": vector,
        "count": count,
        "documents": compact_documents,
    });
    if let Some(map) = data.as_object_mut() {
        if let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) {
            map.insert("query".to_string(), Value::String(query.to_string()));
        }
        if queries.len() > 1 {
            map.insert("keywords".to_string(), json!(queries));
        }
        if let Some(embedding_model) = embedding_model
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert(
                "embedding_model".to_string(),
                Value::String(embedding_model.to_string()),
            );
        }
        if let Some(grouped_queries) = grouped_queries {
            let compact_queries = grouped_queries
                .into_iter()
                .map(|entry| {
                    json!({
                        "keyword": entry.get("keyword").cloned().unwrap_or(Value::Null),
                        "documents": entry
                            .get("documents")
                            .and_then(Value::as_array)
                            .map(|items| {
                                items.iter()
                                    .map(compact_vector_knowledge_document_for_model)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>();
            map.insert("queries".to_string(), json!(compact_queries));
        }
    }
    build_model_tool_success_with_hint(
        "knowledge",
        "completed",
        format!("Retrieved {count} knowledge snippets from {base_name}."),
        data,
        (count == 0).then(|| {
            "No matching knowledge snippets were found. Refine the query or try narrower keywords."
                .to_string()
        }),
    )
}

fn extract_keywords(args: &Value) -> Vec<String> {
    let Some(Value::Array(items)) = args.get("keywords") else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        let Some(text) = item.as_str() else {
            continue;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            output.push(trimmed.to_string());
        }
    }
    output
}

fn resolve_query_text(args: &Value) -> Option<String> {
    if let Some(text) = args.get("query").and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let keywords = extract_keywords(args);
    if keywords.is_empty() {
        None
    } else {
        Some(keywords.join(" "))
    }
}

fn extract_limit(args: &Value) -> Option<usize> {
    let value = args.get("limit")?;
    if let Some(num) = value.as_u64() {
        return Some(num as usize);
    }
    if let Some(num) = value.as_i64() {
        if num > 0 {
            return Some(num as usize);
        }
    }
    if let Some(num) = value.as_f64() {
        if num > 0.0 {
            return Some(num as usize);
        }
    }
    if let Some(text) = value.as_str() {
        if let Ok(num) = text.trim().parse::<usize>() {
            if num > 0 {
                return Some(num);
            }
        }
    }
    None
}

fn parse_timeout_secs(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(num)) => num.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn extract_direct_patch_from_command(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.starts_with("*** Begin Patch") && trimmed.ends_with("*** End Patch") {
        return Some(trimmed.to_string());
    }
    None
}

fn resolve_stream_chunk_size(config: &Config) -> usize {
    let size = config.server.stream_chunk_size;
    if size == 0 {
        1024
    } else {
        size
    }
}

fn safe_chunk_boundary(text: &str, max_bytes: usize) -> usize {
    if text.len() <= max_bytes {
        return text.len();
    }
    let mut index = max_bytes.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    if index == 0 {
        index = max_bytes.min(text.len());
        while index < text.len() && !text.is_char_boundary(index) {
            index += 1;
        }
        if index == 0 {
            index = text.len();
        }
    }
    index
}

#[allow(clippy::too_many_arguments)]
fn emit_tool_output_chunks(
    emitter: &ToolEventEmitter,
    tool_name: &str,
    command: &str,
    stream_name: &str,
    pending: &mut String,
    chunk_size: usize,
    force: bool,
    command_session: Option<&CommandSessionTracker>,
) {
    if pending.is_empty() {
        return;
    }
    let limit = chunk_size.max(1);
    loop {
        if pending.is_empty() {
            break;
        }
        if !force && pending.len() < limit {
            break;
        }
        let take_len = if pending.len() <= limit {
            pending.len()
        } else {
            safe_chunk_boundary(pending, limit)
        };
        if take_len == 0 {
            break;
        }
        let chunk = pending[..take_len].to_string();
        pending.replace_range(..take_len, "");
        if chunk.is_empty() {
            break;
        }
        let mut payload = serde_json::Map::new();
        payload.insert("tool".to_string(), Value::String(tool_name.to_string()));
        payload.insert("command".to_string(), Value::String(command.to_string()));
        payload.insert("stream".to_string(), Value::String(stream_name.to_string()));
        payload.insert("delta".to_string(), Value::String(chunk));
        if let Some(command_session) = command_session {
            command_session.decorate_legacy_payload(&mut payload);
        }
        emitter.emit("tool_output_delta", Value::Object(payload));
    }
}

fn command_session_stream_from_name(stream_name: &str) -> CommandSessionStream {
    if stream_name.eq_ignore_ascii_case("pty") {
        CommandSessionStream::Pty
    } else if stream_name.to_ascii_lowercase().contains("err") {
        CommandSessionStream::Stderr
    } else {
        CommandSessionStream::Stdout
    }
}

#[allow(clippy::too_many_arguments)]
async fn read_stream_output<R>(
    mut reader: R,
    emitter: Option<ToolEventEmitter>,
    tool_name: String,
    command: String,
    stream_name: &'static str,
    chunk_size: usize,
    capture_policy: command_output_guard::CommandOutputPolicy,
    command_session: Option<CommandSessionTracker>,
) -> Result<CommandOutputCapture>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let read_size = chunk_size.max(256);
    let mut buffer = vec![0u8; read_size];
    let mut collector = CommandOutputCollector::new(capture_policy);
    let stream_emitter = emitter.as_ref().filter(|item| item.stream_enabled());
    let command_stream = command_session_stream_from_name(stream_name);

    let mut pending_bytes = Vec::new();
    let mut pending_text = String::new();
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        collector.push_chunk(chunk);
        if let Some(command_session) = command_session.as_ref() {
            command_session.emit_delta(command_stream, chunk);
        }
        if stream_emitter.is_some() {
            pending_bytes.extend_from_slice(chunk);
            loop {
                match std::str::from_utf8(&pending_bytes) {
                    Ok(valid) => {
                        if !valid.is_empty() {
                            pending_text.push_str(valid);
                        }
                        pending_bytes.clear();
                        break;
                    }
                    Err(err) => {
                        let valid_up_to = err.valid_up_to();
                        if valid_up_to == 0 {
                            break;
                        }
                        let valid = &pending_bytes[..valid_up_to];
                        let text = std::str::from_utf8(valid).unwrap_or_default();
                        if !text.is_empty() {
                            pending_text.push_str(text);
                        }
                        pending_bytes.drain(..valid_up_to);
                    }
                }
            }
            if let Some(stream_emitter) = stream_emitter {
                emit_tool_output_chunks(
                    stream_emitter,
                    &tool_name,
                    &command,
                    stream_name,
                    &mut pending_text,
                    chunk_size,
                    false,
                    command_session.as_ref(),
                );
            }
        }
    }

    if let Some(stream_emitter) = stream_emitter {
        if !pending_bytes.is_empty() {
            pending_text.push_str(decode_command_output(&pending_bytes).as_str());
            pending_bytes.clear();
        }
        emit_tool_output_chunks(
            stream_emitter,
            &tool_name,
            &command,
            stream_name,
            &mut pending_text,
            chunk_size,
            true,
            command_session.as_ref(),
        );
    }

    Ok(collector.finish())
}

struct CommandRunResult {
    returncode: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
    stdout_capture: CommandOutputCaptureMeta,
    stderr_capture: CommandOutputCaptureMeta,
    command_session_id: Option<String>,
}

fn compact_command_result_for_model(item: &Value) -> Value {
    let output_meta = item.get("output_meta").and_then(Value::as_object);
    json!({
        "command": item.get("command").cloned().unwrap_or(Value::Null),
        "command_index": item.get("command_index").cloned().unwrap_or(Value::Null),
        "command_session_id": item.get("command_session_id").cloned().unwrap_or(Value::Null),
        "returncode": item.get("returncode").cloned().unwrap_or(Value::Null),
        "stdout": item.get("stdout").cloned().unwrap_or(Value::Null),
        "stderr": item.get("stderr").cloned().unwrap_or(Value::Null),
        "truncated": output_meta
            .and_then(|meta| meta.get("truncated"))
            .cloned()
            .unwrap_or(Value::Null),
        "total_bytes": output_meta
            .and_then(|meta| meta.get("total_bytes"))
            .cloned()
            .unwrap_or(Value::Null),
        "omitted_bytes": output_meta
            .and_then(|meta| meta.get("omitted_bytes"))
            .cloned()
            .unwrap_or(Value::Null),
    })
}

fn compact_command_results_for_model(items: &[Value]) -> Vec<Value> {
    items.iter().map(compact_command_result_for_model).collect()
}

async fn join_output_task(
    handle: Option<tokio::task::JoinHandle<Result<CommandOutputCapture>>>,
) -> Result<CommandOutputCapture> {
    match handle {
        Some(handle) => match handle.await {
            Ok(result) => result,
            Err(err) => Err(anyhow!(err.to_string())),
        },
        None => Ok(CommandOutputCapture::empty()),
    }
}

fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    #[cfg(windows)]
    {
        if looks_like_utf16_output(bytes) {
            if let Some(text) = decode_utf16_output(bytes) {
                return text;
            }
        }
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    let utf8_lossy = String::from_utf8_lossy(bytes).to_string();

    #[cfg(windows)]
    {
        let (decoded, _, _) = GBK.decode(bytes);
        let gbk_text = decoded.into_owned();
        if should_prefer_decoded_text(&gbk_text, &utf8_lossy) {
            return gbk_text;
        }

        if let Some(text) = decode_utf16_output(bytes) {
            if should_prefer_decoded_text(&text, &utf8_lossy) {
                return text;
            }
        }
    }

    utf8_lossy
}

#[cfg(windows)]
fn looks_like_utf16_output(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return false;
    }

    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return true;
    }

    let odd_bytes = bytes.len() / 2;
    if odd_bytes == 0 {
        return false;
    }

    let zero_odd = bytes
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    zero_odd * 100 >= odd_bytes * 20
}

#[cfg(windows)]
fn should_prefer_decoded_text(candidate: &str, fallback: &str) -> bool {
    if candidate.trim().is_empty() {
        return false;
    }

    let candidate_replacement = candidate.chars().filter(|ch| *ch == '\u{FFFD}').count();
    let fallback_replacement = fallback.chars().filter(|ch| *ch == '\u{FFFD}').count();

    if candidate_replacement < fallback_replacement {
        return true;
    }
    if candidate_replacement > fallback_replacement {
        return false;
    }

    fallback_replacement > 0 && contains_cjk(candidate) && !contains_cjk(fallback)
}

#[cfg(windows)]
fn contains_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch,
            '\u{3400}'..='\u{4DBF}'
                | '\u{4E00}'..='\u{9FFF}'
                | '\u{F900}'..='\u{FAFF}'
                | '\u{20000}'..='\u{2A6DF}'
                | '\u{2A700}'..='\u{2B73F}'
                | '\u{2B740}'..='\u{2B81F}'
                | '\u{2B820}'..='\u{2CEAF}'
        )
    })
}

#[cfg(windows)]
fn decode_utf16_output(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 || !bytes.len().is_multiple_of(2) {
        return None;
    }

    let (is_big_endian, start) = if bytes.starts_with(&[0xFE, 0xFF]) {
        (true, 2)
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        (false, 2)
    } else {
        (false, 0)
    };

    let payload = &bytes[start..];
    if payload.is_empty() || !payload.len().is_multiple_of(2) {
        return None;
    }

    let units = payload
        .chunks_exact(2)
        .map(|chunk| {
            if is_big_endian {
                u16::from_be_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_le_bytes([chunk[0], chunk[1]])
            }
        })
        .collect::<Vec<_>>();
    let text = String::from_utf16(&units).ok()?;
    if text.is_empty() {
        None
    } else {
        Some(text.trim_matches('\u{FEFF}').to_string())
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_spawned_child_streaming(
    context: &ToolContext<'_>,
    mut child: tokio::process::Child,
    tool_name: &str,
    command_text: &str,
    timeout: Option<Duration>,
    stdout_policy: CommandOutputPolicy,
    stderr_policy: CommandOutputPolicy,
    command_session: Option<CommandSessionTracker>,
) -> Result<CommandRunResult> {
    let chunk_size = resolve_stream_chunk_size(context.config);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_task = stdout.map(|stdout| {
        let emitter = context.event_emitter.clone();
        let tool_name = tool_name.to_string();
        let command_text = command_text.to_string();
        let command_session = command_session.clone();
        tokio::spawn(async move {
            read_stream_output(
                stdout,
                emitter,
                tool_name,
                command_text,
                "stdout",
                chunk_size,
                stdout_policy,
                command_session,
            )
            .await
        })
    });
    let stderr_task = stderr.map(|stderr| {
        let emitter = context.event_emitter.clone();
        let tool_name = tool_name.to_string();
        let command_text = command_text.to_string();
        let command_session = command_session.clone();
        tokio::spawn(async move {
            read_stream_output(
                stderr,
                emitter,
                tool_name,
                command_text,
                "stderr",
                chunk_size,
                stderr_policy,
                command_session,
            )
            .await
        })
    });

    let result = async {
        let mut timed_out = false;
        let status = if let Some(timeout) = timeout {
            match tokio::time::timeout(timeout, child.wait()).await {
                Ok(result) => Some(result?),
                Err(_) => {
                    timed_out = true;
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    None
                }
            }
        } else {
            Some(child.wait().await?)
        };

        let stdout_capture = join_output_task(stdout_task).await?;
        let stderr_capture = join_output_task(stderr_task).await?;
        Ok::<_, anyhow::Error>((status, timed_out, stdout_capture, stderr_capture))
    }
    .await;

    let (status, timed_out, stdout_capture, stderr_capture) = match result {
        Ok(value) => value,
        Err(err) => {
            if let Some(command_session) = command_session.as_ref() {
                command_session.emit_exit(None, false, Some(err.to_string()));
            }
            return Err(err);
        }
    };
    let stdout = render_command_output(&stdout_capture, decode_command_output);
    let stderr = render_command_output(&stderr_capture, decode_command_output);
    let exit_code = status.and_then(|value| value.code());
    let returncode = exit_code.unwrap_or(-1);
    if let Some(command_session) = command_session.as_ref() {
        command_session.emit_exit(exit_code, timed_out, None);
    }

    Ok(CommandRunResult {
        returncode,
        stdout,
        stderr,
        timed_out,
        stdout_capture: stdout_capture.meta,
        stderr_capture: stderr_capture.meta,
        command_session_id: command_session.map(|item| item.command_session_id().to_string()),
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_command_streaming(
    context: &ToolContext<'_>,
    command: &str,
    cwd: &Path,
    timeout: Option<Duration>,
    tool_name: &str,
    stdout_policy: CommandOutputPolicy,
    stderr_policy: CommandOutputPolicy,
    command_index: usize,
) -> Result<CommandRunResult> {
    let command_text = command.to_string();
    let runtime = python_runtime::resolve_python_runtime();
    let (mut cmd, used_direct) = if let Some(runtime) = runtime.as_ref() {
        if let Some(cmd) =
            command_utils::build_direct_command_with_python_override(command, cwd, &runtime.bin)
        {
            (cmd, true)
        } else {
            (command_utils::build_shell_command(command, cwd), false)
        }
    } else if let Some(cmd) = command_utils::build_direct_command(command, cwd) {
        (cmd, true)
    } else {
        (command_utils::build_shell_command(command, cwd), false)
    };
    if let Some(runtime) = runtime.as_ref() {
        python_runtime::apply_python_env(&mut cmd, runtime);
    }
    let initial_launch_mode = if used_direct {
        CommandSessionLaunchMode::Direct
    } else {
        CommandSessionLaunchMode::Shell
    };
    let initial_shell_name =
        (!used_direct).then(|| command_utils::resolve_shell_name(command).to_string());
    cmd.kill_on_drop(true);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let (child, launch_mode, shell_name) = match cmd.spawn() {
        Ok(child) => (
            child,
            if used_direct {
                CommandSessionLaunchMode::Direct
            } else {
                CommandSessionLaunchMode::Shell
            },
            (!used_direct).then(|| command_utils::resolve_shell_name(command).to_string()),
        ),
        Err(err) if used_direct && command_utils::is_not_found_error(&err) => {
            let mut cmd = command_utils::build_shell_command(command, cwd);
            if let Some(runtime) = runtime.as_ref() {
                python_runtime::apply_python_env(&mut cmd, runtime);
            }
            cmd.kill_on_drop(true);
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            let fallback_shell_name = command_utils::resolve_shell_name(command).to_string();
            match cmd.spawn() {
                Ok(child) => (
                    child,
                    CommandSessionLaunchMode::Shell,
                    Some(fallback_shell_name),
                ),
                Err(fallback_err) => {
                    if let Some(command_session) = CommandSessionTracker::start(
                        context,
                        &command_text,
                        &cwd.to_string_lossy(),
                        command_index,
                        Some(fallback_shell_name),
                        CommandSessionLaunchMode::Shell,
                        false,
                        false,
                    ) {
                        command_session.emit_failed_to_start(fallback_err.to_string());
                    }
                    return Err(anyhow!(fallback_err));
                }
            }
        }
        Err(err) => {
            if let Some(command_session) = CommandSessionTracker::start(
                context,
                &command_text,
                &cwd.to_string_lossy(),
                command_index,
                initial_shell_name.clone(),
                initial_launch_mode,
                false,
                false,
            ) {
                command_session.emit_failed_to_start(err.to_string());
            }
            return Err(anyhow!(err));
        }
    };
    let command_session = CommandSessionTracker::start(
        context,
        &command_text,
        &cwd.to_string_lossy(),
        command_index,
        shell_name,
        launch_mode,
        false,
        false,
    );
    run_spawned_child_streaming(
        context,
        child,
        tool_name,
        &command_text,
        timeout,
        stdout_policy,
        stderr_policy,
        command_session,
    )
    .await
}

async fn run_ptc_python_script_streaming(
    context: &ToolContext<'_>,
    script_path: &Path,
    workdir: &Path,
    timeout: Option<Duration>,
) -> Result<CommandRunResult> {
    #[cfg(windows)]
    let candidates: &[(&str, &[&str])] = &[("py", &["-3"]), ("python", &[]), ("python3", &[])];
    #[cfg(not(windows))]
    let candidates: &[(&str, &[&str])] = &[("python3", &[]), ("python", &[])];

    let tool_name = resolve_tool_name("ptc");
    let script_text = script_path.to_string_lossy().to_string();
    let mut last_error: Option<anyhow::Error> = None;
    let mut tried = Vec::new();

    if let Some(runtime) = python_runtime::resolve_python_runtime() {
        let program = runtime.bin.to_string_lossy().to_string();
        tried.push(program.clone());
        let mut cmd = tokio::process::Command::new(&program);
        cmd.arg(script_path);
        cmd.current_dir(workdir);
        cmd.env("PYTHONIOENCODING", "utf-8");
        python_runtime::apply_python_env(&mut cmd, &runtime);
        command_utils::apply_platform_spawn_options(&mut cmd);
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let command_text = format!("{program} {script_text}");
        match cmd.spawn() {
            Ok(child) => {
                return run_spawned_child_streaming(
                    context,
                    child,
                    &tool_name,
                    &command_text,
                    timeout,
                    STDOUT_CAPTURE_POLICY,
                    STDERR_CAPTURE_POLICY,
                    None,
                )
                .await;
            }
            Err(err) if command_utils::is_not_found_error(&err) => {}
            Err(err) => {
                let detail = format!("{program}: {err}");
                last_error = Some(anyhow!(detail));
            }
        }
    }
    for (program, prefix_args) in candidates {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(*prefix_args);
        cmd.arg(script_path);
        cmd.current_dir(workdir);
        cmd.env("PYTHONIOENCODING", "utf-8");
        command_utils::apply_platform_spawn_options(&mut cmd);
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut parts = Vec::new();
        parts.push((*program).to_string());
        parts.extend(prefix_args.iter().map(|value| (*value).to_string()));
        parts.push(script_text.clone());
        let command_text = parts.join(" ");
        tried.push((*program).to_string());

        match cmd.spawn() {
            Ok(child) => {
                return run_spawned_child_streaming(
                    context,
                    child,
                    &tool_name,
                    &command_text,
                    timeout,
                    STDOUT_CAPTURE_POLICY,
                    STDERR_CAPTURE_POLICY,
                    None,
                )
                .await;
            }
            Err(err) if command_utils::is_not_found_error(&err) => continue,
            Err(err) => {
                let detail = format!("{program}: {err}");
                last_error = Some(anyhow!(detail));
                break;
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| anyhow!("python interpreter not found (tried: {})", tried.join(", "))))
}

async fn execute_command(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    let dry_run = parse_dry_run(&args);
    let command_budget = parse_command_budget(&args);
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if let Some(patch_input) = extract_direct_patch_from_command(&content) {
        // Route accidental inline patch payloads to apply_patch to keep edit semantics stable.
        let payload = json!({
            "input": patch_input,
            "dry_run": dry_run,
        });
        let mut result = apply_patch_tool::apply_patch(context, &payload).await?;
        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "intercepted_from".to_string(),
                Value::String("execute_command".to_string()),
            );
        }
        if !dry_run {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
    }
    if sandbox::sandbox_enabled(context.config) {
        let result = sandbox::execute_tool(
            context.config,
            context.workspace.as_ref(),
            context.user_id,
            context.workspace_id,
            context.session_id,
            "执行命令",
            &args,
            context.user_tool_bindings,
        )
        .await;
        if !dry_run {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
    }

    if content.is_empty() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.exec.command_required"),
            json!({}),
            ToolErrorMeta::new(
                "TOOL_EXEC_COMMAND_REQUIRED",
                Some("请在 content 中提供要执行的命令或脚本文本。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }
    let content = context
        .workspace
        .replace_public_root_in_text(context.workspace_id, &content);

    let allow_commands = &context.config.security.allow_commands;
    let allow_all = allow_commands.iter().any(|item| item == "*");
    let normalized_allow_commands = if allow_all {
        Vec::new()
    } else {
        allow_commands
            .iter()
            .map(|item| item.trim().to_lowercase())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    };
    let timeout_s = parse_timeout_secs(args.get("timeout_s"))
        .unwrap_or(0.0)
        .max(0.0);
    let timeout_s = apply_time_budget_secs(timeout_s, &command_budget);
    let timeout = if timeout_s > 0.0 {
        Some(Duration::from_secs_f64(timeout_s))
    } else {
        None
    };
    let workdir = args.get("workdir").and_then(Value::as_str).unwrap_or("");
    let cwd = if workdir.is_empty() {
        context.workspace.ensure_user_root(context.workspace_id)?
    } else {
        context
            .workspace
            .resolve_path(context.workspace_id, workdir)?
    };
    if !cwd.exists() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.exec.workdir_not_found"),
            json!({ "workdir": workdir }),
            ToolErrorMeta::new(
                "TOOL_EXEC_WORKDIR_NOT_FOUND",
                Some("请确认 workdir 路径存在且在允许范围内。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }
    if !cwd.is_dir() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.exec.workdir_not_dir"),
            json!({ "workdir": workdir }),
            ToolErrorMeta::new(
                "TOOL_EXEC_WORKDIR_NOT_DIR",
                Some("请将 workdir 指向目录而非文件。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }

    let mut results = Vec::new();
    let mut guarded_total_bytes: usize = 0;
    let mut guarded_omitted_bytes: usize = 0;
    let mut guarded_total_commands: usize = 0;
    let mut guarded_truncated_commands: usize = 0;
    let execute_tool_name = resolve_tool_name("execute_command");
    let (stdout_policy, stderr_policy) =
        derive_capture_policies(command_budget.output_budget_bytes);
    let effective_output_budget_bytes = stdout_policy
        .max_bytes()
        .saturating_add(stderr_policy.max_bytes());
    let commands = if allow_all {
        vec![content.clone()]
    } else {
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    if commands.is_empty() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.exec.command_required"),
            json!({}),
            ToolErrorMeta::new(
                "TOOL_EXEC_COMMAND_REQUIRED",
                Some("请在 content 中提供要执行的命令或脚本文本。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }
    if let Some(max_commands) = command_budget.max_commands {
        if commands.len() > max_commands {
            return Ok(build_failed_tool_result(
                format!(
                    "command count {} exceeds budget limit {}",
                    commands.len(),
                    max_commands
                ),
                json!({
                    "command_count": commands.len(),
                    "max_commands": max_commands,
                }),
                ToolErrorMeta::new(
                    "TOOL_EXEC_BUDGET_COMMAND_LIMIT",
                    Some("请减少单次执行命令数量，或提高 max_commands 预算。".to_string()),
                    true,
                    Some(200),
                ),
                false,
            ));
        }
    }
    if dry_run {
        return Ok(build_model_tool_success(
            "execute_command",
            "dry_run",
            "Validated command plan without execution.",
            json!({
                "dry_run": true,
                "workdir": cwd.to_string_lossy().to_string(),
                "command_count": commands.len(),
                "commands": commands,
                "timeout_s": timeout_s,
                "budget": command_budget.to_json(),
                "output_guard": {
                    "default_total_bytes": DEFAULT_CAPTURE_TOTAL_BYTES,
                    "effective_total_bytes": effective_output_budget_bytes,
                },
                "sandbox": false,
            }),
        ));
    }
    for (command_index, command) in commands.into_iter().enumerate() {
        if command.trim().is_empty() {
            continue;
        }
        if !allow_all {
            let lower = command.to_lowercase();
            if !normalized_allow_commands
                .iter()
                .any(|item| lower.starts_with(item))
            {
                return Ok(build_failed_tool_result(
                    i18n::t("tool.exec.not_allowed"),
                    json!({
                        "command": command,
                    }),
                    ToolErrorMeta::new(
                        "TOOL_EXEC_NOT_ALLOWED",
                        Some("命令不在 allow_commands 白名单内。".to_string()),
                        false,
                        None,
                    ),
                    false,
                ));
            }
        }
        let run = run_command_streaming(
            context,
            &command,
            &cwd,
            timeout,
            &execute_tool_name,
            stdout_policy,
            stderr_policy,
            command_index,
        )
        .await?;
        let command_total_bytes = run
            .stdout_capture
            .total_bytes
            .saturating_add(run.stderr_capture.total_bytes);
        let command_omitted_bytes = run
            .stdout_capture
            .omitted_bytes
            .saturating_add(run.stderr_capture.omitted_bytes);
        let command_truncated = run.stdout_capture.truncated || run.stderr_capture.truncated;
        guarded_total_bytes = guarded_total_bytes.saturating_add(command_total_bytes);
        guarded_omitted_bytes = guarded_omitted_bytes.saturating_add(command_omitted_bytes);
        guarded_total_commands = guarded_total_commands.saturating_add(1);
        if command_truncated {
            guarded_truncated_commands = guarded_truncated_commands.saturating_add(1);
        }
        results.push(json!({
            "command": command,
            "command_index": command_index,
            "command_session_id": run.command_session_id,
            "returncode": run.returncode,
            "stdout": run.stdout,
            "stderr": run.stderr,
            "output_meta": {
                "truncated": command_truncated,
                "total_bytes": command_total_bytes,
                "omitted_bytes": command_omitted_bytes,
                "stdout": run.stdout_capture.to_json(),
                "stderr": run.stderr_capture.to_json(),
            },
        }));
        if run.timed_out {
            let detail = if timeout_s > 0.0 {
                format!("timeout after {timeout_s}s")
            } else {
                "timeout".to_string()
            };
            if let Some(last) = results.last_mut().and_then(Value::as_object_mut) {
                let previous = last
                    .get("stderr")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let merged = if previous.trim().is_empty() {
                    detail.clone()
                } else {
                    format!("{previous}\n{detail}")
                };
                last.insert("stderr".to_string(), Value::String(merged));
            }
            context.workspace.mark_tree_dirty(context.workspace_id);
            return Ok(build_failed_tool_result(
                build_execute_command_failure_message(&results, true),
                build_execute_command_failure_data(
                    &results,
                    guarded_total_commands,
                    guarded_truncated_commands > 0,
                    guarded_omitted_bytes,
                    true,
                ),
                ToolErrorMeta::new(
                    "TOOL_EXEC_TIMEOUT",
                    Some(
                        "命令执行超时，可拆分脚本或提高 timeout/budget.time_budget_ms 后重试。"
                            .to_string(),
                    ),
                    true,
                    Some(500),
                ),
                false,
            ));
        }
        if run.returncode != 0 {
            context.workspace.mark_tree_dirty(context.workspace_id);
            return Ok(build_failed_tool_result(
                build_execute_command_failure_message(&results, false),
                build_execute_command_failure_data(
                    &results,
                    guarded_total_commands,
                    guarded_truncated_commands > 0,
                    guarded_omitted_bytes,
                    false,
                ),
                ToolErrorMeta::new(
                    "TOOL_EXEC_NON_ZERO_EXIT",
                    Some("命令返回非 0，请先根据 stderr 修正后再重试。".to_string()),
                    false,
                    None,
                ),
                false,
            ));
        }
    }
    context.workspace.mark_tree_dirty(context.workspace_id);
    Ok(build_model_tool_success_with_hint(
        "execute_command",
        "completed",
        format!("Executed {guarded_total_commands} commands."),
        json!({
            "results": compact_command_results_for_model(&results),
            "budget": command_budget.to_json(),
            "output_guard": {
                "truncated": guarded_truncated_commands > 0,
                "commands": guarded_total_commands,
                "truncated_commands": guarded_truncated_commands,
                "total_bytes": guarded_total_bytes,
                "omitted_bytes": guarded_omitted_bytes,
                "effective_total_bytes": effective_output_budget_bytes,
            },
            "sandbox": false,
        }),
        (guarded_truncated_commands > 0).then(|| {
            "Command output was truncated by the output guard. Narrow the command or raise output_budget_bytes only if needed."
                .to_string()
        }),
    ))
}

fn normalize_ptc_script_name(raw_filename: &str) -> std::result::Result<PathBuf, &'static str> {
    let filename = raw_filename.trim();
    if filename.is_empty() {
        return Err("tool.ptc.filename_required");
    }

    let mut script_name = PathBuf::from(filename);
    if script_name.file_name().and_then(|name| name.to_str()) != Some(filename) {
        return Err("tool.ptc.filename_invalid");
    }
    if script_name.extension().is_none() {
        script_name.set_extension("py");
    }
    if !script_name
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("py"))
        .unwrap_or(false)
    {
        return Err("tool.ptc.ext_invalid");
    }

    Ok(script_name)
}

fn build_ptc_exec_error(detail: impl Into<String>) -> Value {
    build_failed_tool_result(
        i18n::t_with_params(
            "tool.ptc.exec_error",
            &HashMap::from([("detail".to_string(), detail.into())]),
        ),
        json!({}),
        ToolErrorMeta::new(
            "TOOL_PTC_EXEC_ERROR",
            Some(
                "Inspect stderr/stdout and the saved script path, then fix the Python script or workdir."
                    .to_string(),
            ),
            false,
            None,
        ),
        false,
    )
}

fn recover_tool_args_value(args: &Value) -> Value {
    recover_tool_args_value_lossy(args)
}

async fn execute_ptc(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    if sandbox::sandbox_enabled(context.config) {
        let result = sandbox::execute_tool(
            context.config,
            context.workspace.as_ref(),
            context.user_id,
            context.workspace_id,
            context.session_id,
            "ptc",
            &args,
            context.user_tool_bindings,
        )
        .await;
        context.workspace.mark_tree_dirty(context.workspace_id);
        return Ok(result);
    }

    let filename = args
        .get("filename")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let script_name = match normalize_ptc_script_name(filename) {
        Ok(name) => name,
        Err(key) => {
            return Ok(build_failed_tool_result(
                i18n::t(key),
                json!({}),
                ToolErrorMeta::new(
                    "TOOL_PTC_INVALID_FILENAME",
                    Some(
                        "Use a simple Python filename like helper.py without path separators."
                            .to_string(),
                    ),
                    false,
                    None,
                ),
                false,
            ));
        }
    };

    let workdir = args.get("workdir").and_then(Value::as_str).unwrap_or("");
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    if content.trim().is_empty() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.ptc.content_required"),
            json!({}),
            ToolErrorMeta::new(
                "TOOL_PTC_CONTENT_REQUIRED",
                Some("Provide the full Python script content in content.".to_string()),
                false,
                None,
            ),
            false,
        ));
    }

    let content = context
        .workspace
        .replace_public_root_in_text(context.workspace_id, content);
    let workdir_path = if workdir.is_empty() {
        if context
            .config
            .server
            .mode
            .trim()
            .eq_ignore_ascii_case("cli")
        {
            let configured_root = context.config.workspace.root.trim();
            if configured_root.is_empty() {
                context.workspace.ensure_user_root(context.workspace_id)?
            } else {
                PathBuf::from(configured_root)
            }
        } else {
            context.workspace.ensure_user_root(context.workspace_id)?
        }
    } else {
        context
            .workspace
            .resolve_path(context.workspace_id, workdir)?
    };

    if let Err(err) = tokio::fs::create_dir_all(&workdir_path).await {
        return Ok(build_ptc_exec_error(err.to_string()));
    }

    let ptc_root = context
        .workspace
        .resolve_path(context.workspace_id, LOCAL_PTC_DIR_NAME)?;
    if let Err(err) = tokio::fs::create_dir_all(&ptc_root).await {
        return Ok(build_ptc_exec_error(err.to_string()));
    }

    let script_path = ptc_root.join(script_name);
    if let Err(err) = tokio::fs::write(&script_path, content).await {
        return Ok(build_ptc_exec_error(err.to_string()));
    }

    let output = match run_ptc_python_script_streaming(
        context,
        &script_path,
        &workdir_path,
        Some(Duration::from_secs(LOCAL_PTC_TIMEOUT_S)),
    )
    .await
    {
        Ok(output) => output,
        Err(err) => return Ok(build_ptc_exec_error(err.to_string())),
    };

    let data = json!({
        "path": context
            .workspace
            .display_path(context.workspace_id, &script_path),
        "workdir": context
            .workspace
            .display_path(context.workspace_id, &workdir_path),
        "returncode": output.returncode,
        "stdout": output.stdout,
        "stderr": output.stderr,
    });

    context.workspace.mark_tree_dirty(context.workspace_id);

    if output.timed_out {
        let detail = format!("timeout after {}s", LOCAL_PTC_TIMEOUT_S);
        return Ok(build_failed_tool_result(
            i18n::t_with_params(
                "tool.ptc.exec_error",
                &HashMap::from([("detail".to_string(), detail)]),
            ),
            data,
            ToolErrorMeta::new(
                "TOOL_PTC_TIMEOUT",
                Some(
                    "Shorten the script, reduce external waits, or switch to execute_command for simpler shell work."
                        .to_string(),
                ),
                false,
                None,
            ),
            false,
        ));
    }

    if output.returncode != 0 {
        return Ok(build_failed_tool_result(
            i18n::t("tool.ptc.exec_failed"),
            data,
            ToolErrorMeta::new(
                "TOOL_PTC_EXEC_FAILED",
                Some("Inspect stderr and fix the Python script before retrying.".to_string()),
                false,
                None,
            ),
            false,
        ));
    }

    Ok(build_model_tool_success(
        "ptc",
        "completed",
        format!("Executed Python script {}.", script_path.display()),
        data,
    ))
}

async fn list_files(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .to_string();
    let max_depth = args
        .get("max_depth")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_LIST_DEPTH as u64) as usize;
    let pagination = match parse_list_files_pagination(args) {
        Ok(value) => value,
        Err(err) => {
            return Ok(build_failed_tool_result(
                err.to_string(),
                json!({}),
                ToolErrorMeta::new(
                    "TOOL_LIST_INVALID_ARGS",
                    Some(
                        "Use cursor/offset as non-negative integers and limit within 1..500."
                            .to_string(),
                    ),
                    false,
                    None,
                ),
                false,
            ));
        }
    };
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let extra_roots = collect_read_roots(context);
    tokio::task::spawn_blocking(move || {
        list_files_inner(
            workspace.as_ref(),
            &user_id,
            &path,
            &extra_roots,
            max_depth,
            pagination.start,
            pagination.limit,
        )
    })
    .await
    .map_err(|err| anyhow!(err.to_string()))?
}

#[derive(Clone, Copy, Debug)]
struct ListFilesPagination {
    start: usize,
    limit: usize,
}

fn parse_list_files_pagination(args: &Value) -> Result<ListFilesPagination> {
    let start = if let Some(cursor) = args.get("cursor") {
        parse_list_cursor_value(cursor)?
    } else if let Some(offset) = args.get("offset") {
        parse_list_offset_value(offset)?
    } else {
        0
    };
    let limit = if let Some(limit) = args.get("limit") {
        parse_list_limit_value(limit)?
    } else {
        DEFAULT_LIST_PAGE_LIMIT
    };
    Ok(ListFilesPagination {
        start,
        limit: limit.clamp(1, MAX_LIST_ITEMS),
    })
}

fn parse_list_cursor_value(value: &Value) -> Result<usize> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(0);
            }
            trimmed
                .parse::<usize>()
                .map_err(|_| anyhow!("cursor must be a non-negative integer string"))
        }
        Value::Number(number) => number
            .as_u64()
            .map(|raw| raw as usize)
            .ok_or_else(|| anyhow!("cursor must be a non-negative integer string")),
        Value::Null => Ok(0),
        _ => Err(anyhow!("cursor must be a non-negative integer string")),
    }
}

fn parse_list_offset_value(value: &Value) -> Result<usize> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .map(|raw| raw as usize)
            .ok_or_else(|| anyhow!("offset must be a non-negative integer")),
        Value::String(text) => text
            .trim()
            .parse::<usize>()
            .map_err(|_| anyhow!("offset must be a non-negative integer")),
        Value::Null => Ok(0),
        _ => Err(anyhow!("offset must be a non-negative integer")),
    }
}

fn parse_list_limit_value(value: &Value) -> Result<usize> {
    let parsed = match value {
        Value::Number(number) => number
            .as_u64()
            .map(|raw| raw as usize)
            .ok_or_else(|| anyhow!("limit must be a positive integer"))?,
        Value::String(text) => text
            .trim()
            .parse::<usize>()
            .map_err(|_| anyhow!("limit must be a positive integer"))?,
        Value::Null => DEFAULT_LIST_PAGE_LIMIT,
        _ => return Err(anyhow!("limit must be a positive integer")),
    };
    Ok(parsed.max(1).min(MAX_LIST_ITEMS))
}

fn list_files_inner(
    workspace: &WorkspaceManager,
    user_id: &str,
    path: &str,
    extra_roots: &[PathBuf],
    max_depth: usize,
    page_start: usize,
    page_limit: usize,
) -> Result<Value> {
    let root = resolve_tool_path(workspace, user_id, path, extra_roots)?;
    if !root.exists() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.list.path_not_found"),
            json!({ "path": path }),
            ToolErrorMeta::new(
                "TOOL_LIST_PATH_NOT_FOUND",
                Some(
                    "Use a directory path that exists under the current workspace or allowed roots."
                        .to_string(),
                ),
                false,
                None,
            ),
            false,
        ));
    }
    let mut items = Vec::new();
    let mut seen_entries: usize = 0;
    let mut has_more = false;
    let unrestricted_paths = roots_allow_any_path(extra_roots);
    if unrestricted_paths {
        for entry in WalkDir::new(&root)
            .min_depth(1)
            .max_depth(max_depth.saturating_add(1))
            .into_iter()
            .filter_map(|item| item.ok())
        {
            let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            let mut display = rel.to_string_lossy().replace('\\', "/");
            if entry.file_type().is_dir() {
                display.push('/');
            }
            if seen_entries < page_start {
                seen_entries += 1;
                continue;
            }
            if items.len() >= page_limit {
                has_more = true;
                break;
            }
            items.push(display);
            seen_entries += 1;
        }
    } else {
        for entry in WalkDir::new(&root)
            .min_depth(1)
            .max_depth(max_depth.saturating_add(1))
            .into_iter()
            .filter_entry(|entry| !tool_fs_filter::should_skip_walk_entry(entry))
            .filter_map(|item| item.ok())
        {
            let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            let mut display = rel.to_string_lossy().replace('\\', "/");
            if entry.file_type().is_dir() {
                display.push('/');
            }
            if seen_entries < page_start {
                seen_entries += 1;
                continue;
            }
            if items.len() >= page_limit {
                has_more = true;
                break;
            }
            items.push(display);
            seen_entries += 1;
        }
    }
    let returned = items.len();
    let next_offset = page_start.saturating_add(returned);
    let next_cursor = has_more.then(|| next_offset.to_string());
    Ok(build_model_tool_success_with_hint(
        "list_files",
        "completed",
        format!("Listed {returned} entries from {path}."),
        json!({
            "path": path,
            "items": items,
            "offset": page_start,
            "limit": page_limit,
            "returned": returned,
            "has_more": has_more,
            "next_offset": has_more.then_some(next_offset),
            "next_cursor": next_cursor,
            "max_depth": max_depth,
        }),
        has_more.then(|| {
            "More entries are available. Reuse next_cursor to continue listing the same directory."
                .to_string()
        }),
    ))
}

#[derive(Clone, Debug)]
struct ReadFileSpec {
    path: String,
    requested_ranges: Vec<(usize, usize)>,
    ranges: Vec<(usize, usize)>,
    used_default_range: bool,
    mode: ReadFileMode,
    indentation: read_indentation::IndentationReadOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadFileMode {
    Slice,
    Indentation,
}

#[derive(Clone, Copy, Debug, Default)]
struct ReadBudget {
    time_budget_ms: Option<u64>,
    output_budget_bytes: Option<usize>,
    max_files: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadFailureKind {
    PathInvalid,
    NotFound,
    Binary,
}

#[derive(Clone, Debug)]
struct ReadFailure {
    kind: ReadFailureKind,
}

#[derive(Clone, Debug)]
struct ReadSpecParseError {
    code: &'static str,
    message: String,
    hint: Option<String>,
    data: Value,
}

impl ReadBudget {
    fn to_json(self) -> Value {
        json!({
            "time_budget_ms": self.time_budget_ms,
            "output_budget_bytes": self.output_budget_bytes,
            "max_files": self.max_files,
        })
    }
}

impl ReadSpecParseError {
    fn invalid_args(message: String) -> Self {
        Self {
            code: "TOOL_READ_INVALID_ARGS",
            message,
            hint: Some("请检查 files/path/line_ranges/mode/budget 参数格式。".to_string()),
            data: json!({}),
        }
    }

    fn reversed_range(start: usize, end: usize) -> Self {
        let params = HashMap::from([
            ("start".to_string(), start.to_string()),
            ("end".to_string(), end.to_string()),
        ]);
        Self {
            code: "TOOL_READ_INVALID_RANGE",
            message: i18n::t_with_params("tool.read.invalid_reversed_range", &params),
            hint: Some(i18n::t("tool.read.invalid_reversed_range_hint")),
            data: json!({
                "kind": "reversed_line_range",
                "start_line": start,
                "end_line": end,
            }),
        }
    }

    fn too_many_files(count: usize, max: usize) -> Self {
        let params = HashMap::from([
            ("count".to_string(), count.to_string()),
            ("max".to_string(), max.to_string()),
        ]);
        Self {
            code: "TOOL_READ_TOO_MANY_FILES",
            message: i18n::t_with_params("tool.read.too_many_files", &params),
            hint: Some(i18n::t("tool.read.too_many_files_hint")),
            data: json!({
                "kind": "too_many_files",
                "count": count,
                "max_files": max,
            }),
        }
    }
}

fn parse_read_file_specs(
    args: &Value,
) -> std::result::Result<Vec<ReadFileSpec>, ReadSpecParseError> {
    let mut specs = Vec::new();

    if let Some(files) = args.get("files").and_then(Value::as_array) {
        if files.len() > MAX_READ_BUDGET_FILES {
            return Err(ReadSpecParseError::too_many_files(
                files.len(),
                MAX_READ_BUDGET_FILES,
            ));
        }
        for file in files {
            let Some(obj) = file.as_object() else {
                continue;
            };
            if let Some(spec) = parse_read_file_spec_object(obj)? {
                specs.push(spec);
            }
        }
    }

    if specs.is_empty() {
        if let Some(obj) = args.as_object() {
            if let Some(spec) = parse_read_file_spec_object(obj)? {
                specs.push(spec);
            }
        } else if let Some(path) = args
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            specs.push(ReadFileSpec {
                path: path.to_string(),
                requested_ranges: vec![(1, MAX_READ_LINES)],
                ranges: vec![(1, MAX_READ_LINES)],
                used_default_range: true,
                mode: ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            });
        }
    }

    if specs.is_empty() {
        return Err(ReadSpecParseError::invalid_args(i18n::t(
            "tool.read.no_path",
        )));
    }
    Ok(coalesce_read_specs(specs))
}

fn parse_read_file_spec_object(
    obj: &serde_json::Map<String, Value>,
) -> std::result::Result<Option<ReadFileSpec>, ReadSpecParseError> {
    let path = normalize_read_path_hint(
        obj.get("path")
            .or_else(|| obj.get("file_path"))
            .or_else(|| obj.get("file"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
    );
    if path.is_empty() {
        return Ok(None);
    }

    let mut requested_ranges = Vec::new();
    let mut ranges = Vec::new();
    if let Some(Value::Array(items)) = obj.get("line_ranges") {
        for item in items {
            let Some(pair) = item.as_array() else {
                continue;
            };
            if pair.len() < 2 {
                continue;
            }
            let Some(start) = pair.first().and_then(parse_line_number) else {
                continue;
            };
            let Some(end) = pair.get(1).and_then(parse_line_number) else {
                continue;
            };
            validate_line_range_order(start, end)?;
            requested_ranges.push((start, end));
            ranges.push(normalize_range(start, end));
        }
    }

    if let Some(start) = obj.get("start_line").and_then(parse_line_number) {
        let end = obj
            .get("end_line")
            .and_then(parse_line_number)
            .unwrap_or_else(|| start.saturating_add(DEFAULT_START_LINE_WINDOW.saturating_sub(1)));
        validate_line_range_order(start, end)?;
        requested_ranges.push((start, end));
        ranges.push(normalize_range(start, end));
    }
    if let Some(offset) = obj.get("offset").and_then(parse_line_number) {
        let limit = obj
            .get("limit")
            .and_then(parse_line_number)
            .unwrap_or(MAX_READ_LINES)
            .max(1);
        let end = offset.saturating_add(limit.saturating_sub(1));
        requested_ranges.push((offset, end));
        ranges.push(normalize_range(offset, end));
    }

    let used_default_range = ranges.is_empty();
    if ranges.is_empty() {
        requested_ranges.push((1, MAX_READ_LINES));
        ranges.push((1, MAX_READ_LINES));
    }
    ranges = merge_read_ranges(ranges);
    let mode = parse_read_mode(obj);
    let mut indentation = parse_indentation_options(obj);
    if indentation.anchor_line.is_none() {
        indentation.anchor_line = ranges.first().map(|(start, _)| *start);
    }
    Ok(Some(ReadFileSpec {
        path,
        requested_ranges,
        ranges,
        used_default_range,
        mode,
        indentation,
    }))
}

fn parse_read_mode(obj: &serde_json::Map<String, Value>) -> ReadFileMode {
    let raw = obj
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("slice")
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "indentation" | "indent" | "block" => ReadFileMode::Indentation,
        _ => ReadFileMode::Slice,
    }
}

fn parse_indentation_options(
    obj: &serde_json::Map<String, Value>,
) -> read_indentation::IndentationReadOptions {
    let mut options = read_indentation::IndentationReadOptions::default();
    let Some(indentation) = obj.get("indentation").and_then(Value::as_object) else {
        return options;
    };
    options.anchor_line = indentation.get("anchor_line").and_then(parse_line_number);
    options.max_levels = indentation
        .get("max_levels")
        .and_then(parse_line_number)
        .unwrap_or(0);
    options.include_siblings = indentation
        .get("include_siblings")
        .and_then(Value::as_bool)
        .unwrap_or(options.include_siblings);
    options.include_header = indentation
        .get("include_header")
        .and_then(Value::as_bool)
        .unwrap_or(options.include_header);
    options.max_lines = indentation.get("max_lines").and_then(parse_line_number);
    options
}

fn parse_read_budget(args: &Value) -> ReadBudget {
    let Some(obj) = args.as_object() else {
        return ReadBudget::default();
    };
    let budget_obj = obj.get("budget").and_then(Value::as_object);
    let time_budget_ms = budget_obj
        .and_then(|value| value.get("time_budget_ms"))
        .or_else(|| obj.get("time_budget_ms"))
        .and_then(parse_optional_positive_u64)
        .map(|value| value.clamp(1, MAX_READ_TIME_BUDGET_MS));
    let output_budget_bytes = budget_obj
        .and_then(|value| value.get("output_budget_bytes"))
        .or_else(|| obj.get("output_budget_bytes"))
        .and_then(parse_optional_positive_usize)
        .map(|value| value.clamp(MIN_READ_OUTPUT_BUDGET_BYTES, MAX_READ_OUTPUT_BUDGET_BYTES));
    let max_files = budget_obj
        .and_then(|value| value.get("max_files"))
        .or_else(|| obj.get("max_files"))
        .and_then(parse_optional_positive_usize)
        .map(|value| value.clamp(1, MAX_READ_BUDGET_FILES));
    ReadBudget {
        time_budget_ms,
        output_budget_bytes,
        max_files,
    }
}

fn parse_optional_positive_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
    .filter(|value| *value > 0)
}

fn parse_optional_positive_usize(value: &Value) -> Option<usize> {
    parse_optional_positive_u64(value).map(|value| value as usize)
}

fn normalize_read_path_hint(path: String) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed.to_string()
}

fn normalize_read_path_for_workspace(raw_path: &str, workspace_id: &str) -> String {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let normalized = trimmed.replace('\\', "/");

    for prefix in ["/workspaces/", "workspaces/"] {
        if let Some(value) = normalized.strip_prefix(prefix) {
            let candidate = value.trim_matches('/').trim();
            if candidate.is_empty() {
                return String::new();
            }
            let mut segments = candidate.splitn(2, '/');
            let owner = segments.next().unwrap_or("").trim();
            let rest = segments.next().unwrap_or("").trim();
            if rest.is_empty() {
                return owner.to_string();
            }
            if owner == workspace_id {
                return rest.to_string();
            }
            return format!("/workspaces/{candidate}");
        }
    }

    for prefix in ["/workspace/", "workspace/"] {
        if let Some(value) = normalized.strip_prefix(prefix) {
            return value.trim_matches('/').trim().to_string();
        }
    }

    trimmed.to_string()
}

fn parse_line_number(value: &Value) -> Option<usize> {
    if let Some(num) = value.as_u64() {
        return Some(num as usize);
    }
    if let Some(num) = value.as_i64() {
        if num >= 0 {
            return Some(num as usize);
        }
    }
    if let Some(num) = value.as_f64() {
        if num >= 0.0 {
            return Some(num as usize);
        }
    }
    if let Some(text) = value.as_str() {
        if let Ok(num) = text.trim().parse::<usize>() {
            return Some(num);
        }
    }
    None
}

fn normalize_range(start: usize, end: usize) -> (usize, usize) {
    let start = start.max(1);
    let end = end.max(start);
    if end - start + 1 > MAX_RANGE_SPAN {
        return (start, start + MAX_RANGE_SPAN - 1);
    }
    (start, end)
}

fn validate_line_range_order(
    start: usize,
    end: usize,
) -> std::result::Result<(), ReadSpecParseError> {
    if end < start {
        return Err(ReadSpecParseError::reversed_range(start, end));
    }
    Ok(())
}

fn merge_read_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    if ranges.len() <= 1 {
        return ranges;
    }
    ranges.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        if let Some((_, last_end)) = merged.last_mut() {
            if start <= last_end.saturating_add(1) {
                *last_end = (*last_end).max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

fn can_merge_read_specs(left: &ReadFileSpec, right: &ReadFileSpec) -> bool {
    left.path == right.path
        && left.mode == right.mode
        && match left.mode {
            ReadFileMode::Slice => true,
            ReadFileMode::Indentation => left.indentation == right.indentation,
        }
}

fn coalesce_read_specs(specs: Vec<ReadFileSpec>) -> Vec<ReadFileSpec> {
    let mut merged = Vec::with_capacity(specs.len());
    for mut spec in specs {
        spec.ranges = merge_read_ranges(spec.ranges);
        if let Some(last) = merged.last_mut() {
            if can_merge_read_specs(last, &spec) {
                last.requested_ranges.extend(spec.requested_ranges);
                last.ranges.extend(spec.ranges);
                last.ranges = merge_read_ranges(std::mem::take(&mut last.ranges));
                last.used_default_range &= spec.used_default_range;
                continue;
            }
        }
        merged.push(spec);
    }
    merged
}

fn summarize_read_ranges(ranges: &[(usize, usize)], total_lines: usize) -> (usize, bool) {
    if total_lines == 0 {
        return (0, true);
    }
    if ranges.is_empty() {
        return (0, false);
    }
    let mut intervals = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        let start = (*start).max(1);
        if start > total_lines {
            continue;
        }
        let end = (*end).min(total_lines).max(start);
        intervals.push((start, end));
    }
    if intervals.is_empty() {
        return (0, false);
    }
    intervals.sort_by_key(|(start, _)| *start);
    let mut read_lines = 0usize;
    let mut current = intervals[0];
    for (start, end) in intervals.into_iter().skip(1) {
        if start <= current.1 + 1 {
            current.1 = current.1.max(end);
        } else {
            read_lines += current.1 - current.0 + 1;
            current = (start, end);
        }
    }
    read_lines += current.1 - current.0 + 1;
    let complete = read_lines == total_lines;
    (read_lines, complete)
}

fn summarize_slice_eof(ranges: &[(usize, usize)], total_lines: usize) -> (bool, bool) {
    if total_lines == 0 || ranges.is_empty() {
        return (false, false);
    }
    let mut hit_eof = false;
    let mut range_reaches_eof = false;
    for (start, end) in ranges {
        if *start > total_lines {
            hit_eof = true;
            continue;
        }
        if *end >= total_lines {
            hit_eof = true;
            range_reaches_eof = true;
        }
    }
    (hit_eof, range_reaches_eof)
}

fn slice_request_satisfied(ranges: &[(usize, usize)], total_lines: usize) -> bool {
    if ranges.is_empty() {
        return total_lines == 0;
    }
    ranges
        .iter()
        .all(|(start, end)| *start <= total_lines && *end <= total_lines)
}

fn summary_requires_read_continuation(summary: &Value) -> bool {
    let Some(obj) = summary.as_object() else {
        return false;
    };
    if obj
        .get("truncated_by_size")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    let used_default_range = obj
        .get("used_default_range")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let read_lines = obj.get("read_lines").and_then(Value::as_u64).unwrap_or(0);
    let total_lines = obj.get("total_lines").and_then(Value::as_u64).unwrap_or(0);
    used_default_range && read_lines > 0 && total_lines > read_lines
}

fn compact_read_file_summary_for_model(summary: &Value) -> Value {
    json!({
        "path": summary.get("path").cloned().unwrap_or(Value::Null),
        "mode": summary.get("mode").cloned().unwrap_or(Value::Null),
        "requested_ranges": summary.get("requested_ranges").cloned().unwrap_or(Value::Null),
        "effective_ranges": summary.get("effective_ranges").cloned().unwrap_or(Value::Null),
        "used_default_range": summary.get("used_default_range").cloned().unwrap_or(Value::Null),
        "exists": summary.get("exists").cloned().unwrap_or(Value::Null),
        "binary": summary.get("binary").cloned().unwrap_or(Value::Null),
        "mime_type": summary.get("mime_type").cloned().unwrap_or(Value::Null),
        "size_bytes": summary.get("size_bytes").cloned().unwrap_or(Value::Null),
        "read_lines": summary.get("read_lines").cloned().unwrap_or(Value::Null),
        "total_lines": summary.get("total_lines").cloned().unwrap_or(Value::Null),
        "request_satisfied": summary.get("request_satisfied").cloned().unwrap_or(Value::Null),
        "complete": summary.get("complete").cloned().unwrap_or(Value::Null),
        "truncated_by_size": summary.get("truncated_by_size").cloned().unwrap_or(Value::Null),
        "error": summary.get("error").cloned().unwrap_or(Value::Null),
    })
}

async fn read_files(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    let dry_run = parse_dry_run(&args);
    let read_budget = parse_read_budget(&args);
    let mut specs = match parse_read_file_specs(&args) {
        Ok(specs) => specs,
        Err(err) => {
            return Ok(build_failed_tool_result(
                err.message,
                err.data,
                ToolErrorMeta::new(err.code, err.hint, false, None),
                false,
            ));
        }
    };
    let user_id = context.workspace_id.to_string();
    for spec in &mut specs {
        spec.path = normalize_read_path_for_workspace(&spec.path, &user_id);
    }
    let requested_files = specs.len();
    specs = coalesce_read_specs(specs);
    let mut budget_file_limit_hit = false;
    if let Some(max_files) = read_budget.max_files {
        if specs.len() > max_files {
            specs.truncate(max_files);
            budget_file_limit_hit = true;
        }
    }

    let specs_for_lsp = specs.clone();
    let workspace = context.workspace.clone();
    let extra_roots = collect_read_roots(context);
    let budget_for_task = read_budget;
    let result = tokio::task::spawn_blocking(move || {
        read_files_inner(
            workspace.as_ref(),
            &user_id,
            &extra_roots,
            specs,
            budget_for_task,
            dry_run,
            requested_files,
            budget_file_limit_hit,
        )
    })
    .await
    .map_err(|err| anyhow!(err.to_string()))?;
    if result.is_ok() && context.config.lsp.enabled && !dry_run {
        for spec in specs_for_lsp {
            if let Ok(target) = context
                .workspace
                .resolve_path(context.workspace_id, &spec.path)
            {
                let _ = touch_lsp_file(context, &target, false).await;
            }
        }
    }
    result
}

fn read_files_inner(
    workspace: &WorkspaceManager,
    user_id: &str,
    extra_roots: &[PathBuf],
    specs: Vec<ReadFileSpec>,
    budget: ReadBudget,
    dry_run: bool,
    requested_files: usize,
    budget_file_limit_hit: bool,
) -> Result<Value> {
    let started_at = Instant::now();
    let mut outputs = Vec::new();
    let mut summaries = Vec::new();
    let mut failures = Vec::new();
    let mut successful_reads = 0usize;
    let mut timeout_hit = false;
    let mut output_budget_hit = false;
    let mut output_budget_omitted_bytes = 0usize;
    for spec in specs {
        if let Some(limit_ms) = budget.time_budget_ms {
            if started_at.elapsed() >= Duration::from_millis(limit_ms) {
                timeout_hit = true;
                break;
            }
        }
        let raw_path = spec.path.as_str();
        let requested_ranges = spec.requested_ranges.clone();
        let effective_ranges = spec.ranges.clone();
        let range_args_normalized = requested_ranges != effective_ranges;
        let mut summary = json!({
            "path": raw_path,
            "requested_ranges": requested_ranges,
            "effective_ranges": effective_ranges,
            "range_args_normalized": range_args_normalized,
            "used_default_range": spec.used_default_range,
            "read_lines": 0,
            "total_lines": 0,
            "complete": false,
            "dry_run": dry_run
        });
        let target = match workspace.resolve_path(user_id, raw_path) {
            Ok(path) => Some(path),
            Err(err) => {
                if let Some(resolved) = resolve_path_in_roots(raw_path, extra_roots) {
                    Some(resolved)
                } else {
                    let message = err.to_string();
                    outputs.push(format!(">>> {}\n{}", raw_path, message));
                    failures.push(ReadFailure {
                        kind: ReadFailureKind::PathInvalid,
                    });
                    if let Value::Object(ref mut map) = summary {
                        map.insert("error".to_string(), Value::String(message));
                    }
                    None
                }
            }
        };
        let Some(target) = target else {
            summaries.push(summary);
            continue;
        };
        if let Value::Object(ref mut map) = summary {
            map.insert(
                "resolved_path".to_string(),
                Value::String(target.to_string_lossy().to_string()),
            );
        }
        if !target.exists() {
            let message = i18n::t("tool.read.not_found");
            outputs.push(format!(">>> {}\n{}", raw_path, message));
            failures.push(ReadFailure {
                kind: ReadFailureKind::NotFound,
            });
            if let Value::Object(ref mut map) = summary {
                map.insert("exists".to_string(), Value::Bool(false));
                map.insert("error".to_string(), Value::String(message));
            }
            summaries.push(summary);
            continue;
        }
        let size = target.metadata().map(|meta| meta.len()).unwrap_or(0);
        if dry_run {
            if let Value::Object(ref mut map) = summary {
                map.insert("exists".to_string(), Value::Bool(true));
                map.insert("size_bytes".to_string(), Value::from(size));
                map.insert(
                    "mode".to_string(),
                    Value::String(match spec.mode {
                        ReadFileMode::Slice => "slice".to_string(),
                        ReadFileMode::Indentation => "indentation".to_string(),
                    }),
                );
            }
            outputs.push(format!(
                ">>> {}\n[dry_run] exists=true size={} bytes mode={}",
                raw_path,
                size,
                match spec.mode {
                    ReadFileMode::Slice => "slice",
                    ReadFileMode::Indentation => "indentation",
                }
            ));
            successful_reads += 1;
            summaries.push(summary);
            continue;
        }
        let source_truncated_by_size = size > MAX_READ_BYTES as u64;
        let guarded = read_file_guard::read_text_file_with_limit(&target, MAX_READ_BYTES)?;
        let content = match guarded {
            read_file_guard::ReadFileGuardResult::Text(content) => content,
            read_file_guard::ReadFileGuardResult::Omitted(notice) => {
                let read_file_guard::BinaryFileNotice {
                    message,
                    kind,
                    mime_type,
                } = notice;
                if let Value::Object(ref mut map) = summary {
                    map.insert("binary".to_string(), Value::Bool(true));
                    map.insert("kind".to_string(), Value::String(kind.to_string()));
                    if let Some(mime_type) = mime_type {
                        map.insert("mime_type".to_string(), Value::String(mime_type));
                    }
                    map.insert("size_bytes".to_string(), Value::from(size));
                }
                outputs.push(format!(">>> {}\n{}", raw_path, message));
                failures.push(ReadFailure {
                    kind: ReadFailureKind::Binary,
                });
                summaries.push(summary);
                continue;
            }
        };
        successful_reads += 1;
        let lines: Vec<&str> = content.lines().collect();
        let loaded_lines = lines.len();
        if let Value::Object(ref mut map) = summary {
            map.insert("size_bytes".to_string(), Value::from(size));
            map.insert(
                "truncated_by_size".to_string(),
                Value::Bool(source_truncated_by_size),
            );
            if source_truncated_by_size {
                map.insert("loaded_lines".to_string(), Value::from(loaded_lines as u64));
                map.insert(
                    "loaded_bytes".to_string(),
                    Value::from(content.len() as u64),
                );
            }
        }
        match spec.mode {
            ReadFileMode::Slice => {
                let (read_lines, mut complete) = summarize_read_ranges(&spec.ranges, loaded_lines);
                let request_satisfied = slice_request_satisfied(&spec.ranges, loaded_lines);
                let (hit_eof, range_reaches_eof) = if source_truncated_by_size {
                    complete = false;
                    (false, false)
                } else {
                    summarize_slice_eof(&spec.ranges, loaded_lines)
                };
                if let Value::Object(ref mut map) = summary {
                    map.insert("mode".to_string(), Value::String("slice".to_string()));
                    map.insert("read_lines".to_string(), Value::from(read_lines as u64));
                    map.insert("total_lines".to_string(), Value::from(loaded_lines as u64));
                    map.insert(
                        "request_satisfied".to_string(),
                        Value::Bool(request_satisfied),
                    );
                    map.insert("complete".to_string(), Value::Bool(complete));
                    map.insert("hit_eof".to_string(), Value::Bool(hit_eof));
                    map.insert(
                        "range_reaches_eof".to_string(),
                        Value::Bool(range_reaches_eof),
                    );
                }
                let mut file_output = Vec::new();
                if source_truncated_by_size {
                    file_output.push(i18n::t("tool.read.truncated_prefix"));
                }
                let show_range_headers = spec.ranges.len() > 1;
                for (start, end) in spec.ranges {
                    if lines.is_empty() {
                        file_output.push(i18n::t("tool.read.empty_file"));
                        continue;
                    }
                    if start > lines.len() {
                        if source_truncated_by_size {
                            let params = HashMap::from([
                                ("start".to_string(), start.to_string()),
                                ("end".to_string(), end.to_string()),
                                ("loaded".to_string(), lines.len().to_string()),
                            ]);
                            file_output.push(i18n::t_with_params(
                                "tool.read.range_out_of_truncated_excerpt",
                                &params,
                            ));
                        } else {
                            let params = HashMap::from([
                                ("start".to_string(), start.to_string()),
                                ("end".to_string(), end.to_string()),
                                ("total".to_string(), lines.len().to_string()),
                            ]);
                            file_output
                                .push(i18n::t_with_params("tool.read.range_out_of_file", &params));
                        }
                        continue;
                    }
                    let last = end.min(lines.len());
                    let mut slice_lines = Vec::new();
                    if show_range_headers {
                        slice_lines.push(format!("[lines {start}-{last}]"));
                    }
                    for (idx, line) in lines.iter().enumerate().take(last).skip(start - 1) {
                        slice_lines.push(format!("{}: {}", idx + 1, line));
                    }
                    file_output.push(slice_lines.join("\n"));
                    if source_truncated_by_size && end > lines.len() {
                        let params = HashMap::from([
                            ("start".to_string(), start.to_string()),
                            ("end".to_string(), end.to_string()),
                            ("loaded".to_string(), lines.len().to_string()),
                        ]);
                        file_output.push(i18n::t_with_params(
                            "tool.read.range_out_of_truncated_excerpt",
                            &params,
                        ));
                    }
                }
                let joined = file_output.join("\n---\n");
                outputs.push(format!(">>> {}\n{}", raw_path, joined));
            }
            ReadFileMode::Indentation => {
                let selected = read_indentation::read_block(&content, &spec.indentation);
                let read_lines = selected.len();
                let complete = !source_truncated_by_size && loaded_lines == read_lines;
                if let Value::Object(ref mut map) = summary {
                    map.insert("mode".to_string(), Value::String("indentation".to_string()));
                    map.insert("read_lines".to_string(), Value::from(read_lines as u64));
                    map.insert("total_lines".to_string(), Value::from(loaded_lines as u64));
                    map.insert("complete".to_string(), Value::Bool(complete));
                }
                let mut parts = Vec::new();
                if source_truncated_by_size {
                    parts.push(i18n::t("tool.read.truncated_prefix"));
                }
                if selected.is_empty() {
                    parts.push(i18n::t("tool.read.empty_file"));
                } else {
                    let formatted = selected
                        .into_iter()
                        .map(|(line, text)| format!("{line}: {text}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    parts.push(formatted);
                }
                outputs.push(format!(">>> {}\n{}", raw_path, parts.join("\n")));
            }
        }
        summaries.push(summary);
    }
    let mut result = if outputs.is_empty() {
        i18n::t("tool.read.empty_result")
    } else {
        outputs.join("\n\n")
    };
    let bytes_before_budget = result.len();
    if let Some(output_budget_bytes) = budget.output_budget_bytes {
        let (truncated, omitted) = truncate_utf8_output(result.as_str(), output_budget_bytes);
        if omitted > 0 {
            output_budget_hit = true;
            output_budget_omitted_bytes = omitted;
        }
        result = truncated;
    }
    let continuation_required =
        output_budget_hit || summaries.iter().any(summary_requires_read_continuation);
    let processed_files = summaries.len();
    let mut data = json!({
        "content": result,
        "files": summaries
            .iter()
            .map(compact_read_file_summary_for_model)
            .collect::<Vec<_>>(),
        "dry_run": dry_run,
        "requested_files": requested_files,
        "processed_files": processed_files,
        "budget_file_limit_hit": budget_file_limit_hit,
        "timeout_hit": timeout_hit,
        "output_budget_hit": output_budget_hit,
        "output_budget_omitted_bytes": output_budget_omitted_bytes,
        "content_bytes_before_budget": bytes_before_budget,
        "budget": budget.to_json(),
    });
    if continuation_required {
        data["continuation_required"] = Value::Bool(true);
        data["continuation_hint"] = Value::String(i18n::t("tool.read.continuation_hint"));
    }
    if successful_reads == 0 && !failures.is_empty() {
        let (code, hint) = classify_read_failure(&failures);
        let failure_files = summaries
            .iter()
            .zip(failures.iter())
            .map(|(summary, failure)| compact_read_failure_for_model(summary, failure))
            .collect::<Vec<_>>();
        let error = failure_files
            .first()
            .map(summarize_read_failure_for_model)
            .unwrap_or_else(|| i18n::t("tool.read.empty_result"));
        return Ok(build_failed_tool_result(
            error,
            build_read_failure_data(&failure_files),
            ToolErrorMeta::new(code, Some(hint), false, None),
            false,
        ));
    }
    Ok(build_model_tool_success_with_hint(
        "read_file",
        if dry_run { "dry_run" } else { "completed" },
        if dry_run {
            format!("Validated {processed_files} file read targets without reading content.")
        } else {
            format!("Read {processed_files} files.")
        },
        data,
        continuation_required.then(|| i18n::t("tool.read.continuation_hint")),
    ))
}

fn build_read_failure_data(failures: &[Value]) -> Value {
    match failures {
        [] => json!({}),
        [single] => single.clone(),
        many => json!({
            "failed_count": many.len(),
            "failures": many,
        }),
    }
}

fn compact_read_failure_for_model(summary: &Value, failure: &ReadFailure) -> Value {
    let mut map = Map::new();
    if let Some(path) = summary
        .get("path")
        .cloned()
        .filter(|value| !value.is_null())
    {
        map.insert("path".to_string(), path);
    }
    match failure.kind {
        ReadFailureKind::PathInvalid => {
            map.insert(
                "reason".to_string(),
                Value::String("path_invalid".to_string()),
            );
        }
        ReadFailureKind::NotFound => {
            map.insert("reason".to_string(), Value::String("not_found".to_string()));
        }
        ReadFailureKind::Binary => {
            map.insert("reason".to_string(), Value::String("binary".to_string()));
            if let Some(kind) = summary
                .get("kind")
                .cloned()
                .filter(|value| !value.is_null())
            {
                map.insert("kind".to_string(), kind);
            }
            if let Some(mime_type) = summary
                .get("mime_type")
                .cloned()
                .filter(|value| !value.is_null())
            {
                map.insert("mime_type".to_string(), mime_type);
            }
            if let Some(size_bytes) = summary
                .get("size_bytes")
                .cloned()
                .filter(|value| !value.is_null())
            {
                map.insert("size_bytes".to_string(), size_bytes);
            }
            if summary
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "image")
            {
                map.insert(
                    "suggested_tool".to_string(),
                    Value::String(read_image_tool::TOOL_READ_IMAGE.to_string()),
                );
            }
        }
    }
    Value::Object(map)
}

fn summarize_read_failure_for_model(failure: &Value) -> String {
    let path = failure
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("目标文件");
    match failure
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "path_invalid" => format!("{path} 路径无效或超出工作区。"),
        "not_found" => format!("{path} 不存在。"),
        "binary" => {
            if failure
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "image")
            {
                format!(
                    "{path} 是图片，请改用{}。",
                    read_image_tool::TOOL_READ_IMAGE
                )
            } else {
                format!("{path} 是二进制文件，读取文件仅支持纯文本。")
            }
        }
        _ => format!("{path} 无法读取。"),
    }
}

fn classify_read_failure(failures: &[ReadFailure]) -> (&'static str, String) {
    let all_are = |kind| failures.iter().all(|failure| failure.kind == kind);
    if all_are(ReadFailureKind::NotFound) {
        return (
            "TOOL_READ_NOT_FOUND",
            "请先调用列出文件确认真实路径；若目标是技能正文，优先使用技能调用，不要猜测 SKILL.md 路径。".to_string(),
        );
    }
    if all_are(ReadFailureKind::PathInvalid) {
        return (
            "TOOL_READ_PATH_INVALID",
            "请使用相对路径，或直接传入当前工作区的 /workspaces/{user_id}/... 公共路径；不要越界到其他工作区。".to_string(),
        );
    }
    if all_are(ReadFailureKind::Binary) {
        return (
            "TOOL_READ_BINARY_FILE",
            "该工具只适合纯文本文件；图片请改用读图工具，Office/PDF/压缩包请改用对应工具。"
                .to_string(),
        );
    }
    (
        "TOOL_READ_NO_USABLE_TEXT",
        "请先列出文件或搜索内容定位更精确的文本文件，再读取所需片段。".to_string(),
    )
}

fn truncate_utf8_output(text: &str, budget_bytes: usize) -> (String, usize) {
    if text.len() <= budget_bytes {
        return (text.to_string(), 0);
    }
    let mut cut = budget_bytes.min(text.len());
    while cut > 0 && !text.is_char_boundary(cut) {
        cut = cut.saturating_sub(1);
    }
    if cut == 0 {
        return ("".to_string(), text.len());
    }
    let omitted = text.len().saturating_sub(cut);
    (
        format!(
            "{}\n...(truncated read output, omitted {} bytes)...",
            &text[..cut],
            omitted
        ),
        omitted,
    )
}

#[derive(Debug, Deserialize)]
struct SkillCallArgs {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    skill_name: Option<String>,
}

async fn execute_skill_call(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SkillCallArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let raw_name = payload
        .name
        .or(payload.skill_name)
        .unwrap_or_default()
        .trim()
        .to_string();
    if raw_name.is_empty() {
        return Err(anyhow!(i18n::t("tool.skill_call.name_required")));
    }

    let mut selected: Option<SkillSpec> = None;
    if let Some(bindings) = context.user_tool_bindings {
        if let Some(spec) = bindings
            .skill_specs
            .iter()
            .find(|spec| spec.name == raw_name)
        {
            selected = Some(spec.clone());
        } else {
            let suffix = format!("@{raw_name}");
            let matches: Vec<SkillSpec> = bindings
                .skill_specs
                .iter()
                .filter(|spec| spec.name.ends_with(&suffix))
                .cloned()
                .collect();
            if matches.len() == 1 {
                selected = Some(matches[0].clone());
            } else if matches.len() > 1 {
                let candidates = matches
                    .iter()
                    .map(|spec| spec.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(anyhow!(i18n::t_with_params(
                    "tool.skill_call.ambiguous",
                    &HashMap::from([
                        ("name".to_string(), raw_name.clone()),
                        ("candidates".to_string(), candidates),
                    ]),
                )));
            }
        }
    }
    if selected.is_none() {
        selected = context.skills.get(&raw_name);
    }

    let Some(spec) = selected else {
        return Err(anyhow!(i18n::t_with_params(
            "tool.skill_call.not_found",
            &HashMap::from([("name".to_string(), raw_name)]),
        )));
    };

    let content = std::fs::read_to_string(&spec.path).map_err(|err| {
        anyhow!(i18n::t_with_params(
            "tool.skill_call.read_failed",
            &HashMap::from([("detail".to_string(), err.to_string())]),
        ))
    })?;
    let tree = build_skill_tree(&spec.root);
    let path = absolute_path_string_from_text(&spec.path);
    let root = absolute_path_string(&spec.root);
    let content = render_skill_markdown_for_model(&content, &root);
    Ok(build_model_tool_success(
        "skill_call",
        "completed",
        format!("Loaded skill {}.", spec.name),
        json!({
            "name": spec.name,
            "description": spec.description,
            "path": path,
            "root": root,
            "content": content,
            "tree": tree
        }),
    ))
}

fn build_skill_tree(root: &Path) -> Vec<String> {
    let mut items = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(|item| item.ok())
    {
        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
        let mut display = rel.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            display.push('/');
        }
        items.push(display);
    }
    items
}

fn absolute_path_string(path: &Path) -> String {
    let normalized = normalize_existing_path(path);
    let mut text = normalized.to_string_lossy().to_string();
    if cfg!(windows) {
        if let Some(stripped) = text.strip_prefix(r"\\?\") {
            text = stripped.to_string();
        }
    }
    text.replace('\\', "/")
}

fn absolute_path_string_from_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    absolute_path_string(&PathBuf::from(raw))
}

fn normalize_lsp_extension(value: &str) -> String {
    value.trim().trim_start_matches('.').to_lowercase()
}

fn lsp_file_extension(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_string();
    normalize_lsp_extension(&ext)
}

fn lsp_matches_file(config: &Config, path: &Path) -> bool {
    let extension = lsp_file_extension(path);
    config
        .lsp
        .servers
        .iter()
        .filter(|server| server.enabled)
        .any(|server| {
            if server.extensions.is_empty() {
                return true;
            }
            server
                .extensions
                .iter()
                .any(|ext| normalize_lsp_extension(ext) == extension)
        })
}

fn resolve_lsp_timeout_s(config: &Config) -> u64 {
    if config.lsp.timeout_s == 0 {
        30
    } else {
        config.lsp.timeout_s
    }
}

fn parse_lsp_position(args: &Value) -> Result<(u32, u32)> {
    let line = args
        .get("line")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("缺少 line"))?;
    let character = args
        .get("character")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("缺少 character"))?;
    if line == 0 || character == 0 {
        return Err(anyhow!("line/character 必须 >= 1"));
    }
    Ok(((line - 1) as u32, (character - 1) as u32))
}

fn lsp_path_to_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| anyhow!("LSP 文件路径无效"))
}

fn format_lsp_diagnostics(diagnostics: &[LspDiagnostic]) -> Option<Value> {
    if diagnostics.is_empty() {
        return None;
    }
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for diag in diagnostics {
        if diag.is_error() {
            errors.push(diag.pretty());
        } else {
            warnings.push(diag.pretty());
        }
    }
    let total = errors.len() + warnings.len();
    let mut items: Vec<String> = errors.iter().chain(warnings.iter()).cloned().collect();
    let truncated = items.len() > MAX_LSP_DIAGNOSTICS;
    if truncated {
        items.truncate(MAX_LSP_DIAGNOSTICS);
    }
    Some(json!({
        "total": total,
        "errors": errors.len(),
        "warnings": warnings.len(),
        "truncated": truncated,
        "items": items,
    }))
}

fn lsp_diagnostics_summary(context: &ToolContext<'_>, path: &Path) -> Option<Value> {
    let diagnostics_map = context
        .lsp_manager
        .diagnostics_for_user(context.workspace_id);
    if diagnostics_map.is_empty() {
        return None;
    }
    let target = normalize_target_path(path);
    let target_compare = normalize_path_for_compare(&target);
    for (candidate, diagnostics) in diagnostics_map {
        if normalize_path_for_compare(&candidate) == target_compare {
            return format_lsp_diagnostics(&diagnostics);
        }
    }
    None
}

async fn touch_lsp_file(
    context: &ToolContext<'_>,
    path: &Path,
    wait_for_diagnostics: bool,
) -> Value {
    if !context.config.lsp.enabled {
        return Value::Null;
    }
    let workspace_root = context.workspace.workspace_root(context.workspace_id);
    if !is_within_root(&workspace_root, path) {
        return json!({
            "enabled": true,
            "matched": false,
            "touched": false,
            "diagnostics": Option::<Value>::None,
            "error": "文件不在工作区范围内"
        });
    }
    let matched = lsp_matches_file(context.config, path);
    if !matched {
        return json!({
            "enabled": true,
            "matched": false,
            "touched": false,
            "diagnostics": Option::<Value>::None,
            "error": "未匹配到可用的 LSP 服务"
        });
    }
    let mut diagnostics = None;
    let mut error = None;
    let touched = match context
        .lsp_manager
        .touch_file(
            context.config,
            context.workspace_id,
            path,
            wait_for_diagnostics,
        )
        .await
    {
        Ok(()) => true,
        Err(err) => {
            warn!("LSP touch failed: {err}");
            error = Some(err.to_string());
            false
        }
    };
    if touched && wait_for_diagnostics {
        diagnostics = lsp_diagnostics_summary(context, path);
    }
    json!({
        "enabled": true,
        "matched": matched,
        "touched": touched,
        "diagnostics": diagnostics,
        "error": error
    })
}

struct WriteFileOutcome {
    target: PathBuf,
    existed: bool,
    previous_bytes: u64,
}

async fn write_file(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if path.is_empty() {
        return Ok(build_failed_tool_result(
            "缺少 path",
            json!({}),
            ToolErrorMeta::new(
                "TOOL_WRITE_PATH_REQUIRED",
                Some("请提供写入目标路径。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }
    let dry_run = parse_dry_run(&args);
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    let path = path.to_string();
    let content = content.to_string();
    let bytes = content.len();
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let path_for_write = path.clone();
    let allow_roots = collect_orchestration_aware_allow_roots(context);
    let write_outcome = tokio::task::spawn_blocking(move || {
        let target =
            resolve_tool_path(workspace.as_ref(), &user_id, &path_for_write, &allow_roots)?;
        if target.exists() && target.is_dir() {
            return Err(anyhow!("target path is a directory"));
        }
        let existed = target.exists();
        let previous_bytes = if existed {
            target.metadata().map(|meta| meta.len()).unwrap_or(0)
        } else {
            0
        };
        if dry_run {
            return Ok::<WriteFileOutcome, anyhow::Error>(WriteFileOutcome {
                target,
                existed,
                previous_bytes,
            });
        }
        let workspace_root = workspace.workspace_root(&user_id);
        let default_workspace_target = workspace.resolve_path(&user_id, &path_for_write)?;
        if is_within_root(&workspace_root, &target)
            && normalize_path_for_compare(&normalize_target_path(&target))
                == normalize_path_for_compare(&normalize_target_path(&default_workspace_target))
        {
            workspace.write_file(&user_id, &path_for_write, &content, true)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            atomic_write_text(&target, &content)?;
        }
        Ok::<WriteFileOutcome, anyhow::Error>(WriteFileOutcome {
            target,
            existed,
            previous_bytes,
        })
    })
    .await
    .map_err(|err| anyhow!(err.to_string()));
    let write_outcome = match write_outcome {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(err)) | Err(err) => {
            return Ok(build_failed_tool_result(
                format!("写入文件失败：{err}"),
                json!({
                    "path": path,
                    "dry_run": dry_run,
                }),
                ToolErrorMeta::new(
                    "TOOL_WRITE_FAILED",
                    Some("请确认路径权限与目录状态后重试。".to_string()),
                    true,
                    Some(200),
                ),
                false,
            ));
        }
    };
    let lsp_info = if dry_run {
        Value::Null
    } else {
        touch_lsp_file(context, &write_outcome.target, true).await
    };
    Ok(build_model_tool_success(
        "write_file",
        if dry_run { "dry_run" } else { "completed" },
        if dry_run {
            format!("Validated write target for {path} without writing content.")
        } else if write_outcome.existed {
            format!("Updated file {path}.")
        } else {
            format!("Created file {path}.")
        },
        json!({
            "path": path,
            "bytes": bytes,
            "dry_run": dry_run,
            "existed": write_outcome.existed,
            "previous_bytes": write_outcome.previous_bytes,
            "target": write_outcome.target.to_string_lossy().to_string(),
            "lsp": lsp_info
        }),
    ))
}

async fn lsp_query(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if !context.config.lsp.enabled {
        return Err(anyhow!("LSP 未启用"));
    }
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 operation"))?
        .trim()
        .to_string();
    if operation.is_empty() {
        return Err(anyhow!("operation 不能为空"));
    }
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 path"))?
        .trim()
        .to_string();
    if path.is_empty() {
        return Err(anyhow!("path 不能为空"));
    }
    let target = context
        .workspace
        .resolve_path(context.workspace_id, &path)?;
    if !target.exists() {
        return Err(anyhow!("LSP 文件不存在: {path}"));
    }
    context
        .lsp_manager
        .touch_file(context.config, context.workspace_id, &target, false)
        .await?;
    let uri = lsp_path_to_uri(&target)?;
    let timeout_s = resolve_lsp_timeout_s(context.config);
    let operation_key = normalize_lsp_operation_key(&operation);
    let needs_position = matches!(
        operation_key.as_str(),
        "definition" | "references" | "hover" | "implementation" | "callhierarchy"
    );
    let position_value = if needs_position {
        let (line, character) = parse_lsp_position(args)?;
        Some(json!({ "line": line, "character": character }))
    } else {
        None
    };
    let query = if operation_key == "workspacesymbol" {
        Some(
            args.get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
        )
    } else {
        None
    };
    if operation_key == "workspacesymbol" && query.as_deref().unwrap_or("").is_empty() {
        return Err(anyhow!("workspaceSymbol 缺少 query"));
    }
    let call_direction = args
        .get("call_hierarchy_direction")
        .and_then(Value::as_str)
        .unwrap_or("incoming")
        .trim()
        .to_lowercase();
    let text_document = json!({ "uri": uri });
    let position_value = position_value.clone();
    let query_value = query.clone();
    let operation_key = operation_key.clone();
    let call_direction = call_direction.clone();
    let results = context
        .lsp_manager
        .run_on_clients(
            context.config,
            context.workspace_id,
            &target,
            move |client| {
                let text_document = text_document.clone();
                let position = position_value.clone();
                let query = query_value.clone();
                let operation = operation_key.clone();
                let direction = call_direction.clone();
                async move {
                    let server_id = client.server_id().to_string();
                    let server_name = client.server_name().to_string();
                    let result = match operation.as_str() {
                        "definition" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/definition",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "references" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/references",
                                    json!({
                                        "textDocument": text_document,
                                        "position": position,
                                        "context": { "includeDeclaration": true }
                                    }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "hover" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/hover",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "documentsymbol" => {
                            client
                                .request(
                                    "textDocument/documentSymbol",
                                    json!({ "textDocument": text_document }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "workspacesymbol" => {
                            let query = query.unwrap_or_default();
                            client
                                .request("workspace/symbol", json!({ "query": query }), timeout_s)
                                .await?
                        }
                        "implementation" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/implementation",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "callhierarchy" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            let items = client
                                .request(
                                    "textDocument/prepareCallHierarchy",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?;
                            let calls = if let Some(item) =
                                items.as_array().and_then(|items| items.first()).cloned()
                            {
                                let method = if direction == "outgoing" {
                                    "callHierarchy/outgoingCalls"
                                } else {
                                    "callHierarchy/incomingCalls"
                                };
                                client
                                    .request(method, json!({ "item": item }), timeout_s)
                                    .await?
                            } else {
                                Value::Null
                            };
                            json!({
                                "items": items,
                                "direction": direction,
                                "calls": calls
                            })
                        }
                        _ => {
                            return Err(anyhow!("未知 LSP operation: {operation}"));
                        }
                    };
                    Ok(json!({
                        "server_id": server_id,
                        "server_name": server_name,
                        "result": result
                    }))
                }
            },
        )
        .await?;
    Ok(build_model_tool_success_with_hint(
        "lsp_query",
        "completed",
        format!("Ran LSP {operation} on {path} across {} servers.", results.len()),
        json!({
            "operation": operation,
            "path": path,
            "results": results,
            "server_count": results.len(),
        }),
        results.is_empty().then(|| {
            "No LSP servers returned a result for this file. Check server availability or file type support."
                .to_string()
        }),
    ))
}

fn normalize_lsp_operation_key(raw: &str) -> String {
    raw.trim().to_ascii_lowercase().replace(['_', '-'], "")
}

#[derive(Clone)]
struct A2aTaskSnapshot {
    task_id: String,
    context_id: Option<String>,
    status: Option<String>,
    endpoint: Option<String>,
    service_name: Option<String>,
    answer: Option<String>,
    updated_time: Option<String>,
    refresh_error: Option<String>,
}

impl A2aTaskSnapshot {
    fn to_value(&self) -> Value {
        json!({
            "task_id": self.task_id,
            "context_id": self.context_id,
            "status": self.status,
            "endpoint": self.endpoint,
            "service_name": self.service_name,
            "answer": self.answer,
            "updated_time": self.updated_time,
            "refresh_error": self.refresh_error,
        })
    }

    fn is_done(&self) -> bool {
        self.status
            .as_deref()
            .map(is_a2a_task_finished)
            .unwrap_or(false)
    }
}

struct A2aTaskInfo {
    id: String,
    context_id: Option<String>,
    status: Option<String>,
    answer: Option<String>,
}

struct A2aObserveSnapshot {
    tasks: Vec<A2aTaskSnapshot>,
    pending: Vec<A2aTaskSnapshot>,
}

fn build_a2a_snapshot_success(
    action: &str,
    snapshot: &A2aObserveSnapshot,
    elapsed_s: Option<f64>,
    timed_out: bool,
) -> Value {
    let done = snapshot.pending.is_empty();
    let total = snapshot.tasks.len();
    let pending = snapshot.pending.len();
    let mut data = json!({
        "tasks": snapshot.tasks.iter().map(A2aTaskSnapshot::to_value).collect::<Vec<_>>(),
        "pending": snapshot.pending.iter().map(A2aTaskSnapshot::to_value).collect::<Vec<_>>(),
        "done": done,
        "total": total,
        "pending_total": pending,
    });
    if let Some(map) = data.as_object_mut() {
        if let Some(elapsed_s) = elapsed_s {
            map.insert("elapsed_s".to_string(), json!(elapsed_s));
        }
        if action == "a2a_wait" {
            map.insert("timeout".to_string(), Value::Bool(timed_out));
        }
    }
    let summary = if action == "a2a_wait" {
        if done {
            format!("All {total} A2A tasks finished.")
        } else if timed_out {
            format!("{pending} of {total} A2A tasks are still pending after waiting.")
        } else {
            format!("{pending} of {total} A2A tasks are still pending.")
        }
    } else if done {
        format!("Observed {total} A2A tasks; all finished.")
    } else {
        format!("Observed {total} A2A tasks; {pending} still pending.")
    };
    build_model_tool_success_with_hint(
        action,
        if done { "completed" } else { "running" },
        summary,
        data,
        (!done).then(|| {
            "Call a2a_wait again or inspect the pending tasks before assuming the A2A workflow is complete."
                .to_string()
        }),
    )
}

fn is_a2a_service_tool(name: &str) -> bool {
    name.starts_with("a2a@") && name.len() > "a2a@".len()
}

fn is_mcp_tool_name(name: &str) -> bool {
    name.contains('@') && !is_a2a_service_tool(name)
}

fn split_mcp_tool_name(name: &str) -> Result<(String, String)> {
    let (server, tool) = name
        .split_once('@')
        .ok_or_else(|| anyhow!("MCP 工具名称格式不正确"))?;
    if server.trim().is_empty() || tool.trim().is_empty() {
        return Err(anyhow!("MCP 工具名称格式不正确"));
    }
    Ok((server.trim().to_string(), tool.trim().to_string()))
}

async fn execute_mcp_tool(context: &ToolContext<'_>, name: &str, args: &Value) -> Result<Value> {
    let (server_name, tool_name) = split_mcp_tool_name(name)?;
    mcp::call_tool(context.config, &server_name, &tool_name, args).await
}

/// 调用 A2A 服务执行任务，并将结果写入任务存储。
async fn execute_a2a_service(context: &ToolContext<'_>, name: &str, args: &Value) -> Result<Value> {
    let service_name = name.trim_start_matches("a2a@");
    let service = resolve_a2a_service(context.config, service_name, "")
        .ok_or_else(|| anyhow!("A2A 服务不存在: {service_name}"))?;
    if !service.enabled {
        return Err(anyhow!("A2A 服务已禁用: {service_name}"));
    }
    let content = extract_text_arg(args, &["content", "task", "message", "text"])
        .ok_or_else(|| anyhow!("A2A 任务内容不能为空"))?;
    let session_id = extract_text_arg(args, &["session_id", "context_id", "task_id"]);
    let user_id = service
        .user_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(context.user_id);
    let mut message = json!({
        "parts": [
            { "text": content }
        ]
    });
    if let Some(session_id) = session_id.as_ref() {
        message["taskId"] = Value::String(session_id.clone());
        message["contextId"] = Value::String(session_id.clone());
    }
    let mut params = json!({ "message": message });
    if !user_id.trim().is_empty() {
        params["userId"] = Value::String(user_id.to_string());
    }
    let payload = json!({
        "jsonrpc": "2.0",
        "id": Uuid::new_v4().to_string(),
        "method": "SendMessage",
        "params": params
    });
    let headers = build_a2a_headers(context.config, service)?;
    let timeout_s = args
        .get("timeout_s")
        .and_then(Value::as_u64)
        .unwrap_or(context.config.a2a.timeout_s);
    let response = send_a2a_request(
        context.http,
        &service.endpoint,
        headers,
        &payload,
        timeout_s,
    )
    .await?;
    let info = parse_a2a_task_info(&response).ok_or_else(|| anyhow!("A2A 返回缺少任务信息"))?;
    let now = Utc::now();
    context.a2a_store.insert(A2aTask {
        id: info.id.clone(),
        user_id: context.user_id.to_string(),
        status: info.status.clone().unwrap_or_default(),
        context_id: info.context_id.clone(),
        endpoint: Some(service.endpoint.clone()),
        service_name: Some(service.name.clone()),
        method: Some("SendMessage".to_string()),
        created_time: now,
        updated_time: now,
        answer: info.answer.clone().unwrap_or_default(),
    });
    Ok(build_model_tool_success(
        "a2a_send",
        "accepted",
        format!(
            "Submitted task {} to A2A service {}.",
            info.id, service.name
        ),
        json!({
            "endpoint": service.endpoint,
            "service_name": service.name,
            "task_id": info.id,
            "context_id": info.context_id,
            "status": info.status,
            "answer": info.answer,
        }),
    ))
}

/// 观察 A2A 任务状态并返回快照。
async fn a2a_observe(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let snapshot = a2a_observe_snapshot(context, args).await?;
    Ok(build_a2a_snapshot_success(
        "a2a_observe",
        &snapshot,
        None,
        false,
    ))
}

/// 等待 A2A 任务完成或达到超时时间。
async fn a2a_wait(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let timeout_s = args
        .get("wait_s")
        .and_then(Value::as_f64)
        .or_else(|| args.get("timeout_s").and_then(Value::as_f64))
        .unwrap_or(30.0)
        .max(0.0);
    let poll_interval_s = args
        .get("poll_interval_s")
        .and_then(Value::as_f64)
        .unwrap_or(1.5)
        .max(0.2);
    let start = Instant::now();
    let mut last_snapshot = a2a_observe_snapshot(context, args).await?;
    loop {
        if last_snapshot.pending.is_empty() {
            break;
        }
        if timeout_s > 0.0 && start.elapsed().as_secs_f64() >= timeout_s {
            break;
        }
        let remaining = if timeout_s > 0.0 {
            (timeout_s - start.elapsed().as_secs_f64()).max(0.0)
        } else {
            poll_interval_s
        };
        let delay = poll_interval_s.min(remaining.max(0.0));
        if delay <= 0.0 {
            break;
        }
        sleep(Duration::from_secs_f64(delay)).await;
        last_snapshot = a2a_observe_snapshot(context, args).await?;
    }
    let elapsed = start.elapsed().as_secs_f64();
    let elapsed_s = (elapsed * 1000.0).round() / 1000.0;
    let timed_out = !last_snapshot.pending.is_empty() && timeout_s > 0.0 && elapsed >= timeout_s;
    Ok(build_a2a_snapshot_success(
        "a2a_wait",
        &last_snapshot,
        Some(elapsed_s),
        timed_out,
    ))
}

async fn a2a_observe_snapshot(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<A2aObserveSnapshot> {
    let explicit_task_ids = parse_string_list(
        args.get("task_ids")
            .or_else(|| args.get("task_id"))
            .or_else(|| args.get("taskId")),
    );
    let explicit_endpoint = args
        .get("endpoint")
        .and_then(Value::as_str)
        .map(normalize_a2a_endpoint)
        .unwrap_or_default();
    let explicit_service = args
        .get("service_name")
        .or_else(|| args.get("service"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let refresh = args.get("refresh").and_then(Value::as_bool).unwrap_or(true);
    let timeout_s = args
        .get("timeout_s")
        .and_then(Value::as_u64)
        .unwrap_or(context.config.a2a.timeout_s);

    let mut tasks = Vec::new();
    let mut seen = HashSet::new();

    for task in context.a2a_store.list_by_user(context.user_id) {
        if !explicit_task_ids.is_empty() && !explicit_task_ids.contains(&task.id) {
            continue;
        }
        if !explicit_service.is_empty()
            && task
                .service_name
                .as_deref()
                .map(|name| name != explicit_service)
                .unwrap_or(true)
        {
            continue;
        }
        if !explicit_endpoint.is_empty()
            && task
                .endpoint
                .as_deref()
                .map(|value| normalize_a2a_endpoint(value) != explicit_endpoint)
                .unwrap_or(true)
        {
            continue;
        }
        let snapshot = build_snapshot_from_task(&task);
        seen.insert(task.id.clone());
        tasks.push(snapshot);
    }

    if let Some(entries) = args.get("tasks").and_then(Value::as_array) {
        for entry in entries {
            if let Some(snapshot) =
                build_snapshot_from_value(entry, &explicit_endpoint, &explicit_service)
            {
                if seen.insert(snapshot.task_id.clone()) {
                    tasks.push(snapshot);
                }
            }
        }
    }

    for task_id in explicit_task_ids {
        if seen.contains(&task_id) {
            continue;
        }
        tasks.push(A2aTaskSnapshot {
            task_id,
            context_id: None,
            status: None,
            endpoint: if explicit_endpoint.is_empty() {
                None
            } else {
                Some(explicit_endpoint.clone())
            },
            service_name: if explicit_service.is_empty() {
                None
            } else {
                Some(explicit_service.clone())
            },
            answer: None,
            updated_time: None,
            refresh_error: None,
        });
    }

    if refresh {
        for item in tasks.iter_mut() {
            if let Err(err) = refresh_a2a_task(context, item, timeout_s).await {
                item.refresh_error = Some(err.to_string());
            }
        }
    }

    let pending = tasks
        .iter()
        .filter(|&item| !item.is_done())
        .cloned()
        .collect::<Vec<_>>();
    Ok(A2aObserveSnapshot { tasks, pending })
}

fn build_snapshot_from_task(task: &A2aTask) -> A2aTaskSnapshot {
    A2aTaskSnapshot {
        task_id: task.id.clone(),
        context_id: task.context_id.clone(),
        status: Some(task.status.clone()),
        endpoint: task.endpoint.clone(),
        service_name: task.service_name.clone(),
        answer: if task.answer.is_empty() {
            None
        } else {
            Some(task.answer.clone())
        },
        updated_time: Some(task.updated_time.with_timezone(&Local).to_rfc3339()),
        refresh_error: None,
    }
}

fn build_snapshot_from_value(
    value: &Value,
    default_endpoint: &str,
    default_service: &str,
) -> Option<A2aTaskSnapshot> {
    let obj = value.as_object()?;
    let task_id = obj
        .get("task_id")
        .or_else(|| obj.get("taskId"))
        .or_else(|| obj.get("id"))
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if task_id.is_empty() {
        return None;
    }
    let endpoint = obj
        .get("endpoint")
        .and_then(Value::as_str)
        .map(normalize_a2a_endpoint)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            if default_endpoint.is_empty() {
                None
            } else {
                Some(default_endpoint.to_string())
            }
        });
    let service_name = obj
        .get("service_name")
        .or_else(|| obj.get("service"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            if default_service.is_empty() {
                None
            } else {
                Some(default_service.to_string())
            }
        });
    Some(A2aTaskSnapshot {
        task_id,
        context_id: obj
            .get("context_id")
            .or_else(|| obj.get("contextId"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        status: obj
            .get("status")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        endpoint,
        service_name,
        answer: obj
            .get("answer")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        updated_time: obj
            .get("updated_time")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        refresh_error: None,
    })
}

fn parse_string_list(value: Option<&Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(|text| text.trim().to_string()))
            .filter(|text| !text.is_empty())
            .collect(),
        Value::String(text) => text
            .split(',')
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_text_arg(args: &Value, keys: &[&str]) -> Option<String> {
    let obj = args.as_object()?;
    for key in keys {
        if let Some(Value::String(text)) = obj.get(*key) {
            let value = text.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

async fn refresh_a2a_task(
    context: &ToolContext<'_>,
    snapshot: &mut A2aTaskSnapshot,
    timeout_s: u64,
) -> Result<()> {
    if snapshot.task_id.trim().is_empty() {
        return Ok(());
    }
    let endpoint = match snapshot.endpoint.clone() {
        Some(endpoint) if !endpoint.is_empty() => endpoint,
        _ => {
            let service_name = snapshot.service_name.as_deref().unwrap_or("");
            if let Some(service) = resolve_a2a_service(context.config, service_name, "") {
                snapshot.endpoint = Some(service.endpoint.clone());
                snapshot.service_name = Some(service.name.clone());
                service.endpoint.clone()
            } else {
                return Ok(());
            }
        }
    };
    let service_name = snapshot.service_name.clone().unwrap_or_default();
    let service = resolve_a2a_service(context.config, &service_name, &endpoint);
    let headers = match service {
        Some(service) => build_a2a_headers(context.config, service)?,
        None => build_a2a_headers_for_endpoint(context.config, &endpoint)?,
    };
    let payload = json!({
        "jsonrpc": "2.0",
        "id": Uuid::new_v4().to_string(),
        "method": "GetTask",
        "params": { "name": format!("tasks/{}", snapshot.task_id) }
    });
    let response = send_a2a_request(context.http, &endpoint, headers, &payload, timeout_s).await?;
    if let Some(info) = parse_a2a_task_info(&response) {
        snapshot.context_id = info.context_id.clone();
        snapshot.status = info.status.clone();
        snapshot.answer = info.answer.clone();
        snapshot.updated_time = Some(Local::now().to_rfc3339());
        snapshot.refresh_error = None;
        context.a2a_store.update(&info.id, |task| {
            task.context_id = info.context_id.clone();
            task.status = info.status.clone().unwrap_or_default();
            task.answer = info.answer.clone().unwrap_or_default();
            task.updated_time = Utc::now();
        });
    }
    Ok(())
}

fn resolve_a2a_service<'a>(
    config: &'a Config,
    service_name: &str,
    endpoint: &str,
) -> Option<&'a A2aServiceConfig> {
    let normalized_endpoint = normalize_a2a_endpoint(endpoint);
    config.a2a.services.iter().find(|service| {
        if !service_name.is_empty() && service.name == service_name {
            return true;
        }
        if !normalized_endpoint.is_empty() {
            return normalize_a2a_endpoint(&service.endpoint) == normalized_endpoint;
        }
        false
    })
}

fn normalize_a2a_endpoint(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn build_a2a_headers(config: &Config, service: &A2aServiceConfig) -> Result<HeaderMap> {
    let mut header_map = HeaderMap::new();
    for (key, value) in &service.headers {
        let name = HeaderName::from_bytes(key.as_bytes())?;
        let value = HeaderValue::from_str(value)?;
        header_map.insert(name, value);
    }
    if let Some(auth) = &service.auth {
        let auth_json = yaml_to_json(auth);
        if let Value::Object(map) = auth_json {
            if let Some(Value::String(token)) = map.get("bearer_token") {
                let header = HeaderValue::from_str(&format!("Bearer {token}"))?;
                header_map.insert(HeaderName::from_static("authorization"), header);
            }
            if let Some(Value::String(token)) = map.get("token") {
                let header = HeaderValue::from_str(&format!("Bearer {token}"))?;
                header_map.insert(HeaderName::from_static("authorization"), header);
            }
            if let Some(Value::String(token)) = map.get("api_key") {
                let header = HeaderValue::from_str(token)?;
                header_map.insert(HeaderName::from_static("x-api-key"), header);
            }
        }
    }
    let has_auth = header_map
        .keys()
        .any(|key| key.as_str().eq_ignore_ascii_case("authorization"));
    let has_api_key = header_map
        .keys()
        .any(|key| key.as_str().eq_ignore_ascii_case("x-api-key"));
    if should_attach_a2a_api_key(config, service) && !has_auth && !has_api_key {
        if let Some(api_key) = config.api_key() {
            header_map.insert(
                HeaderName::from_static("x-api-key"),
                HeaderValue::from_str(&api_key)?,
            );
        }
    }
    Ok(header_map)
}

fn build_a2a_headers_for_endpoint(config: &Config, endpoint: &str) -> Result<HeaderMap> {
    let mut header_map = HeaderMap::new();
    if let Some(api_key) = config.api_key() {
        if let Ok(parsed) = url::Url::parse(endpoint) {
            let path = parsed.path().trim_end_matches('/');
            if path.ends_with("/a2a") {
                header_map.insert(
                    HeaderName::from_static("x-api-key"),
                    HeaderValue::from_str(&api_key)?,
                );
            }
        }
    }
    Ok(header_map)
}

fn should_attach_a2a_api_key(config: &Config, service: &A2aServiceConfig) -> bool {
    if config.api_key().is_none() {
        return false;
    }
    if service.name.eq_ignore_ascii_case("wunder") {
        return true;
    }
    if let Ok(parsed) = url::Url::parse(&service.endpoint) {
        let path = parsed.path().trim_end_matches('/');
        return path.ends_with("/a2a");
    }
    false
}

async fn send_a2a_request(
    client: &reqwest::Client,
    endpoint: &str,
    headers: HeaderMap,
    payload: &Value,
    timeout_s: u64,
) -> Result<Value> {
    let mut request = client.post(endpoint).headers(headers).json(payload);
    if timeout_s > 0 {
        request = request.timeout(Duration::from_secs(timeout_s));
    }
    let response = request.send().await?;
    let status = response.status();
    let text = response.text().await?;
    let body: Value =
        serde_json::from_str(&text).map_err(|_| anyhow!("A2A 响应非 JSON: {text}"))?;
    if !status.is_success() {
        return Err(anyhow!("A2A 请求失败: {status}"));
    }
    if body.get("error").is_some() {
        return Err(anyhow!("A2A 返回错误: {body}"));
    }
    Ok(body)
}

fn parse_a2a_task_info(value: &Value) -> Option<A2aTaskInfo> {
    let result = value.get("result").unwrap_or(value);
    let task = result.get("task").unwrap_or(result);
    let task_obj = task.as_object()?;
    let id = task_obj
        .get("id")
        .or_else(|| task_obj.get("task_id"))
        .or_else(|| task_obj.get("taskId"))
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if id.is_empty() {
        return None;
    }
    let context_id = task_obj
        .get("contextId")
        .or_else(|| task_obj.get("context_id"))
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let status = match task_obj.get("status") {
        Some(Value::Object(status_obj)) => status_obj
            .get("state")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        Some(Value::String(text)) => Some(text.to_string()),
        _ => None,
    };
    let answer = extract_a2a_answer(task);
    Some(A2aTaskInfo {
        id,
        context_id,
        status,
        answer: if answer.is_empty() {
            None
        } else {
            Some(answer)
        },
    })
}

fn extract_a2a_answer(task: &Value) -> String {
    if let Some(answer) = task.get("answer").and_then(Value::as_str) {
        return answer.to_string();
    }
    let mut parts = Vec::new();
    if let Some(artifacts) = task.get("artifacts").and_then(Value::as_array) {
        for artifact in artifacts {
            if let Some(items) = artifact.get("parts").and_then(Value::as_array) {
                for part in items {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        parts.push(text.to_string());
                    }
                }
            }
        }
    }
    parts.join("\n")
}

fn is_a2a_task_finished(status: &str) -> bool {
    matches!(
        status.to_lowercase().as_str(),
        "completed" | "failed" | "cancelled" | "rejected"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a_store::A2aStore;
    use crate::config::LlmModelConfig;
    use crate::lsp::LspManager;
    use crate::storage::{AgentThreadRecord, ChatSessionRecord, SqliteStorage, UserAgentRecord};
    use crate::workspace::WorkspaceManager;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn sample_chat_session_record(agent_id: &str) -> ChatSessionRecord {
        ChatSessionRecord {
            session_id: "sess_test".to_string(),
            user_id: "alice".to_string(),
            title: "test".to_string(),
            status: "active".to_string(),
            created_at: 1.0,
            updated_at: 1.0,
            last_message_at: 1.0,
            agent_id: Some(agent_id.to_string()),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        }
    }

    fn sample_agent_record() -> UserAgentRecord {
        UserAgentRecord {
            agent_id: "agent_policy_worker".to_string(),
            user_id: "alice".to_string(),
            hive_id: "hive_policy".to_string(),
            name: "政策副手".to_string(),
            description: String::new(),
            system_prompt: "use policy knowledge".to_string(),
            model_name: None,
            ability_items: Vec::new(),
            tool_names: vec!["技能创建器".to_string()],
            declared_tool_names: vec!["read_file".to_string()],
            declared_skill_names: vec!["政策知识库检索技能".to_string()],
            preset_questions: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: "auto_edit".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
            created_at: 1.0,
            updated_at: 1.0,
            preset_binding: None,
            silent: false,
            prefer_mother: false,
        }
    }

    fn sample_llm_model_config(model: &str) -> LlmModelConfig {
        LlmModelConfig {
            enable: Some(true),
            provider: Some("openai".to_string()),
            api_mode: None,
            base_url: Some("http://127.0.0.1:18080/v1".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some(model.to_string()),
            temperature: Some(0.0),
            timeout_s: Some(15),
            retry: Some(0),
            max_rounds: Some(4),
            max_context: Some(16_384),
            max_output: Some(256),
            support_vision: Some(false),
            support_hearing: Some(false),
            stream: Some(false),
            stream_include_usage: Some(false),
            history_compaction_ratio: None,
            tool_call_mode: Some("tool_call".to_string()),
            reasoning_effort: None,
            model_type: Some("llm".to_string()),
            stop: None,
            mock_if_unconfigured: None,
        }
    }

    fn sample_parent_agent_record() -> UserAgentRecord {
        UserAgentRecord {
            agent_id: "agent_parent".to_string(),
            user_id: "alice".to_string(),
            hive_id: "hive_policy".to_string(),
            name: "母蜂".to_string(),
            description: String::new(),
            system_prompt: "coordinate workers".to_string(),
            model_name: None,
            ability_items: Vec::new(),
            tool_names: vec!["智能体蜂群".to_string()],
            declared_tool_names: vec!["agent_swarm".to_string()],
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: "auto_edit".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
            created_at: 1.0,
            updated_at: 1.0,
            preset_binding: None,
            silent: false,
            prefer_mother: false,
        }
    }

    #[test]
    fn extract_user_world_file_refs_handles_quotes_suffix_and_email_mentions() {
        let content =
            "查看 @./docs/report.md, 以及 @\"assets/my file.txt\"，并抄送 @alice@example.com";
        let refs = extract_user_world_file_refs(content, "owner__c__2");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].normalized_path, "docs/report.md");
        assert_eq!(refs[0].suffix, ",");
        assert_eq!(refs[1].normalized_path, "assets/my file.txt");
        assert_eq!(refs[1].suffix, "");
    }

    #[test]
    fn extract_user_world_file_refs_accepts_workspace_prefixed_token() {
        let content = "@/workspaces/owner__c__2/projects/demo.md";
        let refs = extract_user_world_file_refs(content, "owner__c__2");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].normalized_path, "projects/demo.md");
    }

    #[test]
    fn extract_user_world_file_refs_ignores_mismatched_workspace_owner() {
        let content = "@/workspaces/another_owner/demo.md";
        let refs = extract_user_world_file_refs(content, "owner__c__2");
        assert!(refs.is_empty());
    }

    #[test]
    fn parse_list_files_pagination_defaults_to_500() {
        let pagination = parse_list_files_pagination(&json!({})).expect("default pagination");
        assert_eq!(pagination.start, 0);
        assert_eq!(pagination.limit, DEFAULT_LIST_PAGE_LIMIT);
    }

    #[test]
    fn parse_list_files_pagination_accepts_cursor_and_clamps_limit() {
        let pagination = parse_list_files_pagination(&json!({
            "cursor": "12",
            "limit": 9999
        }))
        .expect("pagination should parse");
        assert_eq!(pagination.start, 12);
        assert_eq!(pagination.limit, MAX_LIST_ITEMS);
    }

    #[test]
    fn parse_list_files_pagination_rejects_invalid_cursor() {
        let err = parse_list_files_pagination(&json!({
            "cursor": "not-a-number"
        }))
        .expect_err("cursor should be validated");
        assert!(err.to_string().contains("cursor"));
    }

    #[tokio::test]
    async fn write_file_uses_orchestration_run_root_for_round_short_paths() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.sqlite3");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = dir.path().join("workspace");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        ));
        let run_root = workspace_root.join("workspace-test").join("orchestration").join("orch_demo");
        std::fs::create_dir_all(&run_root).expect("create run root");
        crate::services::orchestration_context::persist_session_context(
            storage.as_ref(),
            "alice",
            "sess_mother",
            &crate::services::orchestration_context::OrchestrationSessionContext {
                mode: crate::services::orchestration_context::ORCHESTRATION_MODE.to_string(),
                run_id: "orch_demo".to_string(),
                group_id: "hive_demo".to_string(),
                role: "mother".to_string(),
                round_index: 2,
                mother_agent_id: "agent_mother".to_string(),
            },
        )
        .expect("persist orchestration context");

        let config = Config::default();
        let a2a_store = A2aStore::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let lsp_manager = LspManager::new(workspace.clone());
        let context = ToolContext {
            user_id: "alice",
            session_id: "sess_mother",
            workspace_id: "workspace-test",
            agent_id: Some("agent_mother"),
            user_round: Some(2),
            model_round: Some(1),
            is_admin: false,
            storage: storage.clone(),
            orchestrator: None,
            monitor: None,
            beeroom_realtime: None,
            workspace: workspace.clone(),
            lsp_manager,
            config: &config,
            a2a_store: &a2a_store,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };

        let result = write_file(
            &context,
            &json!({
                "path": "round_02/worker/report.txt",
                "content": "artifact"
            }),
        )
        .await
        .expect("write file");

        assert_eq!(result["ok"], true);
        assert!(run_root.join("round_02/worker/report.txt").is_file());
        assert!(!workspace_root
            .join("workspace-test")
            .join("round_02/worker/report.txt")
            .exists());
    }

    #[test]
    fn normalize_lsp_operation_key_accepts_snake_case_and_legacy_camel_case() {
        assert_eq!(
            normalize_lsp_operation_key("document_symbol"),
            "documentsymbol"
        );
        assert_eq!(
            normalize_lsp_operation_key("documentSymbol"),
            "documentsymbol"
        );
        assert_eq!(
            normalize_lsp_operation_key("workspace-symbol"),
            "workspacesymbol"
        );
        assert_eq!(
            normalize_lsp_operation_key("call_hierarchy"),
            "callhierarchy"
        );
    }

    #[test]
    fn session_spawn_args_accept_message_alias() {
        let payload: SessionSpawnArgs = serde_json::from_value(json!({
            "message": "hello child"
        }))
        .expect("message alias should deserialize");
        assert_eq!(payload.task, "hello child");
    }

    #[test]
    fn session_spawn_args_prefers_task_when_task_and_message_are_both_present() {
        let payload: SessionSpawnArgs = serde_json::from_value(json!({
            "task": "explicit task",
            "message": "legacy alias"
        }))
        .expect("task and message should deserialize together");
        assert_eq!(payload.task, "explicit task");
    }

    #[test]
    fn session_spawn_args_accept_thread_strategy_aliases() {
        let camel: SessionSpawnArgs = serde_json::from_value(json!({
            "task": "hello child",
            "threadStrategy": "main_thread",
        }))
        .expect("camel thread strategy should deserialize");
        assert_eq!(camel.thread_strategy.as_deref(), Some("main_thread"));

        let snake: SessionSpawnArgs = serde_json::from_value(json!({
            "task": "hello child",
            "thread_strategy": "fresh_main_thread",
            "reuse_main_thread": true,
        }))
        .expect("snake thread strategy should deserialize");
        assert_eq!(snake.thread_strategy.as_deref(), Some("fresh_main_thread"));
        assert_eq!(snake.reuse_main_thread, Some(true));
    }

    #[test]
    fn list_files_inner_supports_cursor_pagination() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("list-files-pagination.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = dir.path().join("workspaces");
        let workspace = WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &HashMap::new(),
        );

        let user_root = workspace_root.join("admin");
        std::fs::create_dir_all(&user_root).expect("create user root");
        for idx in 0..5usize {
            std::fs::write(user_root.join(format!("f{idx}.txt")), "demo").expect("write file");
        }

        let page1 = list_files_inner(&workspace, "admin", ".", &[], 1, 0, 2).expect("page1");
        assert_eq!(
            page1
                .pointer("/data/items")
                .and_then(Value::as_array)
                .map(|v| v.len()),
            Some(2)
        );
        assert_eq!(
            page1.pointer("/data/next_cursor").and_then(Value::as_str),
            Some("2")
        );
        assert_eq!(
            page1.pointer("/data/has_more").and_then(Value::as_bool),
            Some(true)
        );

        let page2 = list_files_inner(&workspace, "admin", ".", &[], 1, 2, 2).expect("page2");
        assert_eq!(
            page2
                .pointer("/data/items")
                .and_then(Value::as_array)
                .map(|v| v.len()),
            Some(2)
        );
        assert_eq!(
            page2.pointer("/data/next_cursor").and_then(Value::as_str),
            Some("4")
        );
        assert_eq!(
            page2.pointer("/data/has_more").and_then(Value::as_bool),
            Some(true)
        );

        let page3 = list_files_inner(&workspace, "admin", ".", &[], 1, 4, 2).expect("page3");
        assert_eq!(
            page3
                .pointer("/data/items")
                .and_then(Value::as_array)
                .map(|v| v.len()),
            Some(1)
        );
        assert_eq!(page3.pointer("/data/next_cursor"), Some(&Value::Null));
        assert_eq!(
            page3.pointer("/data/has_more").and_then(Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn parse_read_file_specs_accepts_shorthand_path_payload() {
        let specs = parse_read_file_specs(&json!({
            "path": "Cargo.toml",
            "start_line": 2,
            "end_line": 5,
        }))
        .expect("shorthand path should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "Cargo.toml");
        assert_eq!(specs[0].ranges, vec![(2, 5)]);
    }

    #[test]
    fn parse_read_file_specs_accepts_file_path_alias() {
        let specs = parse_read_file_specs(&json!({
            "file_path": "README.md",
        }))
        .expect("file_path alias should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "README.md");
        assert_eq!(specs[0].ranges, vec![(1, MAX_READ_LINES)]);
    }

    #[test]
    fn parse_read_file_specs_accepts_offset_and_limit_aliases() {
        let specs = parse_read_file_specs(&json!({
            "file_path": "README.md",
            "offset": 15,
            "limit": 20,
        }))
        .expect("offset/limit alias should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "README.md");
        assert_eq!(specs[0].ranges, vec![(15, 34)]);
    }

    #[test]
    fn parse_read_file_specs_treats_start_line_without_end_line_as_window() {
        let specs = parse_read_file_specs(&json!({
            "path": "README.md",
            "start_line": 18,
        }))
        .expect("start_line window payload should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "README.md");
        assert_eq!(
            specs[0].ranges,
            vec![(18, 18 + DEFAULT_START_LINE_WINDOW - 1)]
        );
    }

    #[test]
    fn parse_read_file_specs_clamps_explicit_range_to_max_span() {
        let specs = parse_read_file_specs(&json!({
            "path": "README.md",
            "start_line": 10,
            "end_line": 10000,
        }))
        .expect("explicit range should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].ranges, vec![(10, 10 + MAX_RANGE_SPAN - 1)]);
    }

    #[test]
    fn parse_read_file_specs_rejects_descending_ranges() {
        let err = parse_read_file_specs(&json!({
            "path": "README.md",
            "start_line": 80,
            "end_line": 12,
        }))
        .expect_err("descending ranges should fail");

        assert_eq!(err.code, "TOOL_READ_INVALID_RANGE");
        assert!(err.message.contains("80"));
        assert!(err.message.contains("12"));
    }

    #[test]
    fn parse_read_file_specs_rejects_more_than_max_budget_files() {
        let files = (0..=MAX_READ_BUDGET_FILES)
            .map(|idx| {
                json!({
                    "path": format!("docs/{idx}.md"),
                })
            })
            .collect::<Vec<_>>();
        let err = parse_read_file_specs(&json!({
            "files": files,
        }))
        .expect_err("oversized files payload should fail");

        assert_eq!(err.code, "TOOL_READ_TOO_MANY_FILES");
        assert_eq!(err.data.get("count").and_then(Value::as_u64), Some(21));
        assert_eq!(
            err.data.get("max_files").and_then(Value::as_u64),
            Some(MAX_READ_BUDGET_FILES as u64)
        );
    }

    #[test]
    fn parse_read_file_specs_normalizes_zero_start_line_to_first_line() {
        let specs = parse_read_file_specs(&json!({
            "path": "README.md",
            "start_line": 0,
            "end_line": 12,
        }))
        .expect("zero-based start should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].requested_ranges, vec![(0, 12)]);
        assert_eq!(specs[0].ranges, vec![(1, 12)]);
    }

    #[test]
    fn parse_read_file_specs_coalesces_adjacent_slice_specs_for_same_file() {
        let specs = parse_read_file_specs(&json!({
            "files": [
                {
                    "path": "README.md",
                    "start_line": 0,
                    "end_line": 100
                },
                {
                    "path": "README.md",
                    "start_line": 100,
                    "end_line": 200
                },
                {
                    "path": "README.md",
                    "start_line": 200,
                    "end_line": 300
                }
            ]
        }))
        .expect("adjacent slice specs should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].ranges, vec![(1, 300)]);
    }

    #[test]
    fn normalize_read_path_for_workspace_strips_matching_workspace_id() {
        let normalized = normalize_read_path_for_workspace(
            "/workspaces/admin/agents/demo.worker-card.json",
            "admin",
        );
        assert_eq!(normalized, "agents/demo.worker-card.json");
    }

    #[test]
    fn normalize_read_path_for_workspace_keeps_mismatched_workspace_id() {
        let normalized =
            normalize_read_path_for_workspace("/workspaces/another_owner/demo.txt", "admin");
        assert_eq!(normalized, "/workspaces/another_owner/demo.txt");
    }

    #[test]
    fn normalize_read_path_for_workspace_accepts_legacy_workspace_prefix() {
        let normalized = normalize_read_path_for_workspace("/workspaces/Cargo.toml", "admin");
        assert_eq!(normalized, "Cargo.toml");
    }

    #[test]
    fn parse_read_file_specs_parses_indentation_mode() {
        let specs = parse_read_file_specs(&json!({
            "path": "src/main.rs",
            "mode": "indentation",
            "indentation": {
                "anchor_line": 12,
                "max_levels": 2,
                "include_siblings": true,
                "include_header": false,
                "max_lines": 40
            }
        }))
        .expect("indentation mode should parse");

        assert_eq!(specs.len(), 1);
        assert!(matches!(specs[0].mode, ReadFileMode::Indentation));
        assert_eq!(specs[0].indentation.anchor_line, Some(12));
        assert_eq!(specs[0].indentation.max_levels, 2);
        assert!(specs[0].indentation.include_siblings);
        assert!(!specs[0].indentation.include_header);
        assert_eq!(specs[0].indentation.max_lines, Some(40));
    }

    #[test]
    fn parse_read_budget_reads_nested_and_top_level_fields() {
        let budget = parse_read_budget(&json!({
            "time_budget_ms": 9000,
            "budget": {
                "output_budget_bytes": 4096,
                "max_files": 3
            }
        }));
        assert_eq!(budget.time_budget_ms, Some(9000));
        assert_eq!(budget.output_budget_bytes, Some(4096));
        assert_eq!(budget.max_files, Some(3));
    }

    #[test]
    fn read_files_inner_returns_failed_result_when_all_files_missing() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("read-files-missing.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = dir.path().join("workspaces");
        let workspace = WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &HashMap::new(),
        );

        let value = read_files_inner(
            &workspace,
            "admin",
            &[],
            vec![ReadFileSpec {
                path: "missing.txt".to_string(),
                requested_ranges: vec![(1, 20)],
                ranges: vec![(1, 20)],
                used_default_range: false,
                mode: ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            }],
            ReadBudget::default(),
            false,
            1,
            false,
        )
        .expect("read files result");

        assert_eq!(value.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(
            value.pointer("/error_meta/code").and_then(Value::as_str),
            Some("TOOL_READ_NOT_FOUND")
        );
        assert_eq!(
            value.pointer("/data/path").and_then(Value::as_str),
            Some("missing.txt")
        );
        assert_eq!(
            value.pointer("/data/reason").and_then(Value::as_str),
            Some("not_found")
        );
        assert!(value.pointer("/data/content").is_none());
    }

    #[test]
    fn read_files_inner_returns_compact_binary_failure() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("read-files-binary.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = dir.path().join("workspaces");
        let workspace = WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &HashMap::new(),
        );

        let user_root = workspace_root.join("admin");
        std::fs::create_dir_all(&user_root).expect("create user root");
        let file_path = user_root.join("heart.png");
        std::fs::write(&file_path, b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR").expect("write png");

        let value = read_files_inner(
            &workspace,
            "admin",
            &[],
            vec![ReadFileSpec {
                path: "heart.png".to_string(),
                requested_ranges: vec![(1, 20)],
                ranges: vec![(1, 20)],
                used_default_range: false,
                mode: ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            }],
            ReadBudget::default(),
            false,
            1,
            false,
        )
        .expect("read files result");

        assert_eq!(value.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(
            value.pointer("/error_meta/code").and_then(Value::as_str),
            Some("TOOL_READ_BINARY_FILE")
        );
        assert_eq!(
            value.pointer("/data/path").and_then(Value::as_str),
            Some("heart.png")
        );
        assert_eq!(
            value.pointer("/data/kind").and_then(Value::as_str),
            Some("image")
        );
        assert_eq!(
            value.pointer("/data/mime_type").and_then(Value::as_str),
            Some("image/png")
        );
        assert_eq!(
            value
                .pointer("/data/suggested_tool")
                .and_then(Value::as_str),
            Some(read_image_tool::TOOL_READ_IMAGE)
        );
        assert!(value.pointer("/data/content").is_none());
    }

    #[test]
    fn read_files_inner_returns_truncated_excerpt_for_large_text_file() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("read-files-large.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = dir.path().join("workspaces");
        let workspace = WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &HashMap::new(),
        );

        let user_root = workspace_root.join("admin");
        std::fs::create_dir_all(&user_root).expect("create user root");
        let file_path = user_root.join("large.md");
        let mut content = String::new();
        for idx in 1..=60_000usize {
            content.push_str(&format!("line {idx:05} {}\n", "x".repeat(24)));
        }
        std::fs::write(&file_path, content).expect("write large file");

        let value = read_files_inner(
            &workspace,
            "admin",
            &[],
            vec![ReadFileSpec {
                path: "large.md".to_string(),
                requested_ranges: vec![(1, 5)],
                ranges: vec![(1, 5)],
                used_default_range: false,
                mode: ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            }],
            ReadBudget::default(),
            false,
            1,
            false,
        )
        .expect("read files result");

        assert_ne!(value.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(
            value
                .pointer("/data/files/0/truncated_by_size")
                .and_then(Value::as_bool),
            Some(true)
        );
        let body = value
            .pointer("/data/content")
            .and_then(Value::as_str)
            .expect("content should exist");
        assert!(body.contains("line 00001"));
        assert!(body.contains(">>> large.md"));
    }

    #[test]
    fn read_files_inner_marks_default_full_window_as_continuable() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("read-files-default-window.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = dir.path().join("workspaces");
        let workspace = WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &HashMap::new(),
        );

        let user_root = workspace_root.join("admin");
        std::fs::create_dir_all(&user_root).expect("create user root");
        let file_path = user_root.join("treaty.md");
        let mut content = String::new();
        for idx in 1..=2_500usize {
            content.push_str(&format!("line {idx:05}\n"));
        }
        std::fs::write(&file_path, content).expect("write treaty file");

        let value = read_files_inner(
            &workspace,
            "admin",
            &[],
            vec![ReadFileSpec {
                path: "treaty.md".to_string(),
                requested_ranges: vec![(1, MAX_READ_LINES)],
                ranges: vec![(1, MAX_READ_LINES)],
                used_default_range: true,
                mode: ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            }],
            ReadBudget::default(),
            false,
            1,
            false,
        )
        .expect("read files result");

        assert_eq!(
            value
                .pointer("/data/continuation_required")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            value
                .pointer("/data/files/0/request_satisfied")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            value
                .pointer("/data/files/0/used_default_range")
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn compact_command_result_for_model_flattens_output_guard_fields() {
        let value = compact_command_result_for_model(&json!({
            "command": "rg hello src",
            "command_index": 1,
            "command_session_id": "cmdsess_1",
            "returncode": 0,
            "stdout": "hello",
            "stderr": "",
            "output_meta": {
                "truncated": true,
                "total_bytes": 8192,
                "omitted_bytes": 2048
            },
            "raw_bytes": 12345
        }));

        assert_eq!(
            value,
            json!({
                "command": "rg hello src",
                "command_index": 1,
                "command_session_id": "cmdsess_1",
                "returncode": 0,
                "stdout": "hello",
                "stderr": "",
                "truncated": true,
                "total_bytes": 8192,
                "omitted_bytes": 2048
            })
        );
    }

    #[test]
    fn compact_knowledge_document_for_model_keeps_only_model_relevant_fields() {
        let value = compact_knowledge_document_for_model(&json!({
            "code": "sec-1",
            "document": "design.md",
            "name": "Overview",
            "section_path": ["Overview"],
            "content": "Important details",
            "score": 0.92,
            "reason": "semantic_match",
        }));

        assert_eq!(
            value,
            json!({
                "document": "design.md",
                "name": "Overview",
                "section_path": ["Overview"],
                "content": "Important details",
                "score": 0.92,
                "reason": "semantic_match",
            })
        );
    }

    #[test]
    fn compact_vector_knowledge_document_for_model_drops_embedding_noise() {
        let value = compact_vector_knowledge_document_for_model(&json!({
            "doc_id": "doc-1",
            "document": "guide.md",
            "name": "guide.md",
            "chunk_index": 3,
            "start": 120,
            "end": 240,
            "content": "Chunk content",
            "embedding_model": "bge-large",
            "score": 0.81,
            "keyword": "timeout",
        }));

        assert_eq!(
            value,
            json!({
                "doc_id": "doc-1",
                "document": "guide.md",
                "chunk_index": 3,
                "start": 120,
                "end": 240,
                "content": "Chunk content",
                "score": 0.81,
                "keyword": "timeout",
            })
        );
    }

    #[test]
    fn build_a2a_snapshot_success_marks_running_and_includes_hint() {
        let snapshot = A2aObserveSnapshot {
            tasks: vec![
                A2aTaskSnapshot {
                    task_id: "task-1".to_string(),
                    context_id: Some("ctx-1".to_string()),
                    status: Some("running".to_string()),
                    endpoint: Some("http://a2a.local".to_string()),
                    service_name: Some("helper".to_string()),
                    answer: None,
                    updated_time: Some("2026-01-01T00:00:00+08:00".to_string()),
                    refresh_error: None,
                },
                A2aTaskSnapshot {
                    task_id: "task-2".to_string(),
                    context_id: Some("ctx-2".to_string()),
                    status: Some("completed".to_string()),
                    endpoint: Some("http://a2a.local".to_string()),
                    service_name: Some("helper".to_string()),
                    answer: Some("done".to_string()),
                    updated_time: Some("2026-01-01T00:00:01+08:00".to_string()),
                    refresh_error: None,
                },
            ],
            pending: vec![A2aTaskSnapshot {
                task_id: "task-1".to_string(),
                context_id: Some("ctx-1".to_string()),
                status: Some("running".to_string()),
                endpoint: Some("http://a2a.local".to_string()),
                service_name: Some("helper".to_string()),
                answer: None,
                updated_time: Some("2026-01-01T00:00:00+08:00".to_string()),
                refresh_error: None,
            }],
        };

        let value = build_a2a_snapshot_success("a2a_wait", &snapshot, Some(1.25), true);

        assert_eq!(value.get("state").and_then(Value::as_str), Some("running"));
        assert_eq!(
            value.pointer("/data/pending_total").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            value.pointer("/data/timeout").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            value.get("next_step_hint").and_then(Value::as_str),
            Some(
                "Call a2a_wait again or inspect the pending tasks before assuming the A2A workflow is complete."
            )
        );
    }

    #[test]
    fn summarize_slice_eof_marks_eof_ranges() {
        let (hit_eof, range_reaches_eof) = summarize_slice_eof(&[(100, 200)], 178);
        assert!(hit_eof);
        assert!(range_reaches_eof);

        let (hit_eof, range_reaches_eof) = summarize_slice_eof(&[(200, 300)], 178);
        assert!(hit_eof);
        assert!(!range_reaches_eof);

        let (hit_eof, range_reaches_eof) = summarize_slice_eof(&[(1, 50)], 178);
        assert!(!hit_eof);
        assert!(!range_reaches_eof);
    }

    #[test]
    fn truncate_utf8_output_respects_char_boundary() {
        let text = "a中b";
        let (truncated, omitted) = truncate_utf8_output(text, 2);
        assert!(omitted > 0);
        assert!(truncated.contains("truncated read output"));
    }

    #[test]
    fn extract_direct_patch_from_command_accepts_raw_patch_payload() {
        let command = r#"
*** Begin Patch
*** Update File: src/main.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
"#;
        let extracted = extract_direct_patch_from_command(command);
        assert!(extracted.is_some());
        let patch = extracted.expect("patch should be extracted");
        assert!(patch.starts_with("*** Begin Patch"));
        assert!(patch.ends_with("*** End Patch"));
    }

    #[test]
    fn extract_direct_patch_from_command_rejects_wrapped_shell_text() {
        let command = r#"cat <<'PATCH'
*** Begin Patch
*** Update File: src/main.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
PATCH"#;
        assert!(extract_direct_patch_from_command(command).is_none());
    }

    #[test]
    fn builtin_tool_specs_excludes_replace_text() {
        let specs = builtin_tool_specs_with_language("zh-CN");
        assert!(specs.iter().all(|spec| spec.name != "替换文本"));
        assert!(specs.iter().any(|spec| spec.name == "应用补丁"));
    }

    #[test]
    fn builtin_aliases_excludes_replace_text() {
        let aliases = builtin_aliases();
        assert!(!aliases.contains_key("replace_text"));
        assert_eq!(
            aliases.get("apply_patch").map(String::as_str),
            Some("应用补丁")
        );
        assert_eq!(
            aliases
                .get(read_image_tool::TOOL_VIEW_IMAGE_ALIAS)
                .map(String::as_str),
            Some(read_image_tool::TOOL_READ_IMAGE)
        );
        assert_eq!(
            aliases
                .get(sleep_tool::TOOL_SLEEP_ALIAS)
                .map(String::as_str),
            Some(sleep_tool::TOOL_SLEEP_WAIT)
        );
    }

    #[test]
    fn load_agent_record_accepts_default_agent_alias() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("tools-default-agent.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());

        let record = load_agent_record(&storage, "alice", Some("__default__"), false)
            .expect("load default agent")
            .expect("default agent record");

        assert_eq!(record.agent_id, "__default__");
        assert_eq!(record.user_id, "alice");
    }

    #[test]
    fn resolve_session_tool_overrides_prefers_declared_agent_defaults() {
        let session = sample_chat_session_record("agent_policy_worker");
        let agent = sample_agent_record();

        let overrides = resolve_session_tool_overrides(&session, None, Some(&agent));

        assert_eq!(
            overrides,
            vec!["read_file".to_string(), "政策知识库检索技能".to_string()]
        );
    }

    #[test]
    fn resolve_child_session_tool_names_uses_target_agent_defaults_for_swarm_children() {
        let parent_tool_names = vec!["技能创建器".to_string()];
        let agent = sample_agent_record();

        let inherited = resolve_child_session_tool_names(
            ChildSessionToolMode::InheritParentSession,
            &parent_tool_names,
            Some(&agent),
        );
        let swarm_defaults = resolve_child_session_tool_names(
            ChildSessionToolMode::UseTargetAgentDefaults,
            &parent_tool_names,
            Some(&agent),
        );

        assert_eq!(inherited, vec!["技能创建器".to_string()]);
        assert_eq!(
            swarm_defaults,
            vec!["read_file".to_string(), "政策知识库检索技能".to_string()]
        );
    }

    #[tokio::test]
    async fn prepare_swarm_child_session_creates_fresh_main_thread_even_when_worker_has_existing_main_session(
    ) {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("swarm-fresh-main-thread.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let parent_agent = sample_parent_agent_record();
        let worker_agent = sample_agent_record();
        storage_backend
            .upsert_user_agent(&parent_agent)
            .expect("upsert parent agent");
        storage_backend
            .upsert_user_agent(&worker_agent)
            .expect("upsert worker agent");

        let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
        parent_session.session_id = "sess_parent".to_string();
        parent_session.tool_overrides = vec!["agent_swarm".to_string()];
        storage_backend
            .upsert_chat_session(&parent_session)
            .expect("upsert parent session");

        let mut old_worker_session = sample_chat_session_record(&worker_agent.agent_id);
        old_worker_session.session_id = "sess_worker_existing".to_string();
        storage_backend
            .upsert_chat_session(&old_worker_session)
            .expect("upsert existing worker session");
        storage_backend
            .upsert_agent_thread(&AgentThreadRecord {
                thread_id: "thread_existing_worker".to_string(),
                user_id: "alice".to_string(),
                agent_id: worker_agent.agent_id.clone(),
                session_id: old_worker_session.session_id.clone(),
                status: "idle".to_string(),
                created_at: 1.0,
                updated_at: 1.0,
            })
            .expect("bind existing worker main thread");

        let workspace_root = dir.path().join("workspace");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage_backend.clone(),
            0,
            &HashMap::new(),
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let config = Config::default();
        let a2a_store = A2aStore::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let context = ToolContext {
            user_id: "alice",
            session_id: "sess_parent",
            workspace_id: "workspace-test",
            agent_id: Some("agent_parent"),
            user_round: Some(1),
            model_round: Some(1),
            is_admin: false,
            storage: storage_backend.clone(),
            orchestrator: None,
            monitor: None,
            beeroom_realtime: None,
            workspace,
            lsp_manager,
            config: &config,
            a2a_store: &a2a_store,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };

        let prepared = prepare_swarm_child_session(
            &context,
            "clean worker context",
            Some("政策副手".to_string()),
            &worker_agent.agent_id,
        )
        .expect("prepare fresh swarm child session");

        assert_ne!(prepared.child_session_id, old_worker_session.session_id);
        assert_eq!(
            storage_backend
                .get_agent_thread("alice", &worker_agent.agent_id)
                .expect("get worker thread")
                .expect("worker thread")
                .session_id,
            prepared.child_session_id
        );
        assert_eq!(
            storage_backend
                .get_chat_session("alice", &prepared.child_session_id)
                .expect("load child session")
                .expect("child session")
                .parent_session_id
                .as_deref(),
            Some("sess_parent")
        );
        assert_eq!(
            storage_backend
                .get_chat_session("alice", &prepared.child_session_id)
                .expect("reload child session")
                .expect("child session")
                .spawned_by
                .as_deref(),
            Some("agent_swarm")
        );
    }

    #[tokio::test]
    async fn prepare_child_session_does_not_rebind_parent_agent_main_thread_for_subagent_children()
    {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("subagent-keep-parent-main-thread.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let parent_agent = sample_parent_agent_record();
        storage_backend
            .upsert_user_agent(&parent_agent)
            .expect("upsert parent agent");

        let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
        parent_session.session_id = "sess_parent".to_string();
        storage_backend
            .upsert_chat_session(&parent_session)
            .expect("upsert parent session");
        storage_backend
            .upsert_agent_thread(&AgentThreadRecord {
                thread_id: "thread_parent_main".to_string(),
                user_id: "alice".to_string(),
                agent_id: parent_agent.agent_id.clone(),
                session_id: parent_session.session_id.clone(),
                status: "idle".to_string(),
                created_at: 1.0,
                updated_at: 1.0,
            })
            .expect("bind parent main thread");

        let workspace_root = dir.path().join("workspace");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage_backend.clone(),
            0,
            &HashMap::new(),
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let config = Config::default();
        let a2a_store = A2aStore::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let context = ToolContext {
            user_id: "alice",
            session_id: "sess_parent",
            workspace_id: "workspace-test",
            agent_id: Some(parent_agent.agent_id.as_str()),
            user_round: Some(1),
            model_round: Some(1),
            is_admin: false,
            storage: storage_backend.clone(),
            orchestrator: None,
            monitor: None,
            beeroom_realtime: None,
            workspace,
            lsp_manager,
            config: &config,
            a2a_store: &a2a_store,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };

        let prepared = prepare_child_session(
            &context,
            "sess_parent",
            "delegate subagent task",
            Some("temporary child".to_string()),
            None,
            None,
            ChildSessionToolMode::InheritParentSession,
        )
        .expect("prepare child session");

        assert_ne!(prepared.child_session_id, "sess_parent");
        assert_eq!(
            storage_backend
                .get_agent_thread("alice", &parent_agent.agent_id)
                .expect("get parent thread")
                .expect("parent thread")
                .session_id,
            "sess_parent"
        );
        assert_eq!(
            storage_backend
                .get_chat_session("alice", &prepared.child_session_id)
                .expect("load child session")
                .expect("child session")
                .parent_session_id
                .as_deref(),
            Some("sess_parent")
        );
        assert_eq!(
            storage_backend
                .get_chat_session("alice", &prepared.child_session_id)
                .expect("reload child session")
                .expect("child session")
                .spawned_by
                .as_deref(),
            Some("model")
        );
    }

    #[tokio::test]
    async fn swarm_worker_subagent_child_does_not_steal_worker_main_thread() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir
            .path()
            .join("swarm-worker-subagent-keeps-worker-main-thread.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let parent_agent = sample_parent_agent_record();
        let worker_agent = sample_agent_record();
        storage_backend
            .upsert_user_agent(&parent_agent)
            .expect("upsert parent agent");
        storage_backend
            .upsert_user_agent(&worker_agent)
            .expect("upsert worker agent");

        let mut mother_session = sample_chat_session_record(&parent_agent.agent_id);
        mother_session.session_id = "sess_mother".to_string();
        mother_session.tool_overrides = vec!["agent_swarm".to_string()];
        storage_backend
            .upsert_chat_session(&mother_session)
            .expect("upsert mother session");

        let workspace_root = dir.path().join("workspace");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage_backend.clone(),
            0,
            &HashMap::new(),
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let config = Config::default();
        let a2a_store = A2aStore::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let mother_context = ToolContext {
            user_id: "alice",
            session_id: "sess_mother",
            workspace_id: "workspace-test",
            agent_id: Some(parent_agent.agent_id.as_str()),
            user_round: Some(1),
            model_round: Some(1),
            is_admin: false,
            storage: storage_backend.clone(),
            orchestrator: None,
            monitor: None,
            beeroom_realtime: None,
            workspace: workspace.clone(),
            lsp_manager: lsp_manager.clone(),
            config: &config,
            a2a_store: &a2a_store,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };

        let worker_prepared = prepare_swarm_child_session(
            &mother_context,
            "worker task",
            Some("worker".to_string()),
            &worker_agent.agent_id,
        )
        .expect("prepare worker session");

        assert_eq!(
            storage_backend
                .get_agent_thread("alice", &worker_agent.agent_id)
                .expect("get worker thread after swarm dispatch")
                .expect("worker thread after swarm dispatch")
                .session_id,
            worker_prepared.child_session_id
        );

        let worker_context = ToolContext {
            user_id: "alice",
            session_id: worker_prepared.child_session_id.as_str(),
            workspace_id: "workspace-test",
            agent_id: Some(worker_agent.agent_id.as_str()),
            user_round: Some(1),
            model_round: Some(1),
            is_admin: false,
            storage: storage_backend.clone(),
            orchestrator: None,
            monitor: None,
            beeroom_realtime: None,
            workspace,
            lsp_manager,
            config: &config,
            a2a_store: &a2a_store,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };

        let subagent_prepared = prepare_child_session(
            &worker_context,
            &worker_prepared.child_session_id,
            "subagent task",
            Some("temporary worker child".to_string()),
            None,
            None,
            ChildSessionToolMode::InheritParentSession,
        )
        .expect("prepare subagent child session");

        assert_ne!(
            subagent_prepared.child_session_id,
            worker_prepared.child_session_id
        );
        assert_eq!(
            storage_backend
                .get_agent_thread("alice", &worker_agent.agent_id)
                .expect("get worker thread after subagent spawn")
                .expect("worker thread after subagent spawn")
                .session_id,
            worker_prepared.child_session_id
        );
        assert_eq!(
            storage_backend
                .get_chat_session("alice", &subagent_prepared.child_session_id)
                .expect("load subagent child session")
                .expect("subagent child session")
                .parent_session_id
                .as_deref(),
            Some(worker_prepared.child_session_id.as_str())
        );
    }

    #[test]
    fn prepare_child_session_inherits_effective_model_from_parent_agent() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("subagent-inherit-model.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let mut parent_agent = sample_parent_agent_record();
        parent_agent.model_name = Some("model-parent".to_string());
        storage_backend
            .upsert_user_agent(&parent_agent)
            .expect("upsert parent agent");

        let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
        parent_session.session_id = "sess_parent".to_string();
        storage_backend
            .upsert_chat_session(&parent_session)
            .expect("upsert parent session");

        let workspace_root = dir.path().join("workspace");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage_backend.clone(),
            0,
            &HashMap::new(),
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let mut config = Config::default();
        config.llm.default = "model-default".to_string();
        config.llm.models.insert(
            "model-default".to_string(),
            sample_llm_model_config("provider-default"),
        );
        config.llm.models.insert(
            "model-parent".to_string(),
            sample_llm_model_config("provider-parent"),
        );
        let a2a_store = A2aStore::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let context = ToolContext {
            user_id: "alice",
            session_id: "sess_parent",
            workspace_id: "workspace-test",
            agent_id: Some(parent_agent.agent_id.as_str()),
            user_round: Some(1),
            model_round: Some(1),
            is_admin: false,
            storage: storage_backend.clone(),
            orchestrator: None,
            monitor: None,
            beeroom_realtime: None,
            workspace,
            lsp_manager,
            config: &config,
            a2a_store: &a2a_store,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };

        let prepared = prepare_child_session(
            &context,
            "sess_parent",
            "solve it",
            Some("worker".to_string()),
            None,
            None,
            ChildSessionToolMode::InheritParentSession,
        )
        .expect("prepare child session");

        assert_eq!(prepared.model_name.as_deref(), Some("model-parent"));
        assert_eq!(prepared.request.model_name.as_deref(), Some("model-parent"));
    }

    #[test]
    fn prepare_swarm_child_session_uses_target_agent_model_for_initial_run() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("swarm-target-model.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let parent_agent = sample_parent_agent_record();
        let mut worker_agent = sample_agent_record();
        worker_agent.model_name = Some("model-worker".to_string());
        storage_backend
            .upsert_user_agent(&parent_agent)
            .expect("upsert parent agent");
        storage_backend
            .upsert_user_agent(&worker_agent)
            .expect("upsert worker agent");

        let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
        parent_session.session_id = "sess_parent".to_string();
        parent_session.tool_overrides = vec!["agent_swarm".to_string()];
        storage_backend
            .upsert_chat_session(&parent_session)
            .expect("upsert parent session");

        let workspace_root = dir.path().join("workspace");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage_backend.clone(),
            0,
            &HashMap::new(),
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let mut config = Config::default();
        config.llm.default = "model-default".to_string();
        config.llm.models.insert(
            "model-default".to_string(),
            sample_llm_model_config("provider-default"),
        );
        config.llm.models.insert(
            "model-worker".to_string(),
            sample_llm_model_config("provider-worker"),
        );
        let a2a_store = A2aStore::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let context = ToolContext {
            user_id: "alice",
            session_id: "sess_parent",
            workspace_id: "workspace-test",
            agent_id: Some(parent_agent.agent_id.as_str()),
            user_round: Some(1),
            model_round: Some(1),
            is_admin: false,
            storage: storage_backend.clone(),
            orchestrator: None,
            monitor: None,
            beeroom_realtime: None,
            workspace,
            lsp_manager,
            config: &config,
            a2a_store: &a2a_store,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };

        let prepared = prepare_swarm_child_session(
            &context,
            "review this",
            Some("worker".to_string()),
            &worker_agent.agent_id,
        )
        .expect("prepare swarm child session");

        assert_eq!(prepared.model_name.as_deref(), Some("model-worker"));
        assert_eq!(prepared.request.model_name.as_deref(), Some("model-worker"));
    }

    #[test]
    fn agent_swarm_batch_send_args_accept_team_run_id_aliases() {
        let camel: AgentSwarmBatchSendArgs = serde_json::from_value(json!({
            "tasks": [{ "agent_id": "worker_a", "message": "hello" }],
            "teamRunId": "team_demo_camel",
        }))
        .expect("parse camel args");
        assert_eq!(camel.team_run_id.as_deref(), Some("team_demo_camel"));

        let snake: AgentSwarmBatchSendArgs = serde_json::from_value(json!({
            "tasks": [{ "agent_id": "worker_a", "message": "hello" }],
            "team_run_id": "team_demo_snake",
        }))
        .expect("parse snake args");
        assert_eq!(snake.team_run_id.as_deref(), Some("team_demo_snake"));
    }

    #[test]
    fn agent_swarm_send_args_accept_thread_strategy_aliases() {
        let camel: AgentSwarmSendArgs = serde_json::from_value(json!({
            "agent_name": "worker_a",
            "message": "hello",
            "threadStrategy": "main_thread",
        }))
        .expect("parse camel send args");
        assert_eq!(camel.thread_strategy.as_deref(), Some("main_thread"));

        let snake: AgentSwarmSendArgs = serde_json::from_value(json!({
            "agent_name": "worker_a",
            "message": "hello",
            "thread_strategy": "fresh_main_thread",
            "reuse_main_thread": true,
        }))
        .expect("parse snake send args");
        assert_eq!(snake.thread_strategy.as_deref(), Some("fresh_main_thread"));
        assert_eq!(snake.reuse_main_thread, Some(true));
    }

    #[test]
    fn agent_swarm_batch_send_args_accept_thread_strategy_aliases() {
        let camel: AgentSwarmBatchSendArgs = serde_json::from_value(json!({
            "tasks": [{ "agent_id": "worker_a", "message": "hello" }],
            "threadStrategy": "main_thread",
        }))
        .expect("parse camel batch args");
        assert_eq!(camel.thread_strategy.as_deref(), Some("main_thread"));

        let snake: AgentSwarmBatchSendArgs = serde_json::from_value(json!({
            "tasks": [{
                "agent_id": "worker_a",
                "message": "hello",
                "thread_strategy": "fresh_main_thread",
                "reuse_main_thread": true
            }],
            "reuse_main_thread": true,
        }))
        .expect("parse snake batch args");
        assert_eq!(snake.reuse_main_thread, Some(true));
        assert_eq!(
            snake.tasks[0].thread_strategy.as_deref(),
            Some("fresh_main_thread")
        );
        assert_eq!(snake.tasks[0].reuse_main_thread, Some(true));
    }

    #[test]
    fn agent_swarm_send_args_accept_canonical_session_id() {
        let payload: AgentSwarmSendArgs = serde_json::from_value(json!({
            "session_id": "sess_worker_demo",
            "message": "hello",
        }))
        .expect("parse canonical send args");
        assert_eq!(payload.session_key.as_deref(), Some("sess_worker_demo"));
        assert_eq!(payload.message, "hello");
    }

    #[test]
    fn agent_swarm_wait_args_accept_canonical_run_ids() {
        let payload: AgentSwarmWaitArgs = serde_json::from_value(json!({
            "run_ids": ["run_demo_1"],
            "wait_seconds": 3,
        }))
        .expect("parse canonical wait args");
        assert_eq!(payload.run_ids, Some(vec!["run_demo_1".to_string()]));
        assert_eq!(payload.wait_seconds, Some(3.0));
    }

    #[test]
    fn tool_result_field_reads_nested_data_before_top_level() {
        let result = json!({
            "status": "top-level-status",
            "data": {
                "status": "accepted",
                "run_id": "run_worker_a",
                "agent_id": "worker_a",
                "agent_name": "Worker A",
                "session_id": "sess_worker_a",
                "created_session": true,
                "thread_strategy": "main_thread",
                "error": "nested error"
            }
        });

        assert_eq!(
            tool_result_field(&result, "status").and_then(Value::as_str),
            Some("accepted")
        );
        assert_eq!(
            tool_result_field(&result, "run_id").and_then(Value::as_str),
            Some("run_worker_a")
        );
        assert_eq!(
            tool_result_field(&result, "agent_id").and_then(Value::as_str),
            Some("worker_a")
        );
        assert_eq!(
            tool_result_field(&result, "session_id").and_then(Value::as_str),
            Some("sess_worker_a")
        );
        assert_eq!(
            tool_result_field(&result, "created_session").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            tool_result_field_or_null(&result, "thread_strategy").as_str(),
            Some("main_thread")
        );
        assert_eq!(tool_result_field_or_null(&result, "missing"), Value::Null);
        assert_eq!(
            tool_result_field(&result, "error").and_then(Value::as_str),
            Some("nested error")
        );
    }

    #[test]
    fn parse_swarm_worker_thread_strategy_supports_main_thread_option() {
        assert_eq!(
            parse_swarm_worker_thread_strategy(None, None).expect("default strategy"),
            SwarmWorkerThreadStrategy::MainThread
        );
        assert_eq!(
            parse_swarm_worker_thread_strategy(Some("main_thread"), None)
                .expect("main_thread strategy"),
            SwarmWorkerThreadStrategy::MainThread
        );
        assert_eq!(
            parse_swarm_worker_thread_strategy(Some("fresh_main_thread"), None)
                .expect("fresh_main_thread strategy"),
            SwarmWorkerThreadStrategy::FreshMainThread
        );
        assert_eq!(
            parse_swarm_worker_thread_strategy(None, Some(true)).expect("reuseMainThread strategy"),
            SwarmWorkerThreadStrategy::MainThread
        );
    }

    #[test]
    fn parse_swarm_worker_thread_strategy_rejects_unknown_value() {
        let err = parse_swarm_worker_thread_strategy(Some("reuse_previous"), None)
            .expect_err("unknown strategy should fail");
        assert!(err.to_string().contains("fresh_main_thread"));
        assert!(err.to_string().contains("main_thread"));
    }

    #[tokio::test]
    async fn swarm_main_thread_strategy_reuses_existing_worker_main_thread() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("swarm-main-thread-reuse.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let worker_agent = sample_agent_record();
        storage_backend
            .upsert_user_agent(&worker_agent)
            .expect("upsert worker agent");

        let mut worker_session = sample_chat_session_record(&worker_agent.agent_id);
        worker_session.session_id = "sess_worker_main".to_string();
        storage_backend
            .upsert_chat_session(&worker_session)
            .expect("upsert worker session");
        storage_backend
            .upsert_agent_thread(&AgentThreadRecord {
                thread_id: "thread_worker_main".to_string(),
                user_id: "alice".to_string(),
                agent_id: worker_agent.agent_id.clone(),
                session_id: worker_session.session_id.clone(),
                status: "idle".to_string(),
                created_at: 1.0,
                updated_at: 1.0,
            })
            .expect("bind worker main thread");

        let (resolved, created) =
            crate::services::swarm::beeroom::resolve_or_create_agent_main_session(
                storage_backend.as_ref(),
                "alice",
                &worker_agent,
            )
            .expect("resolve main session");

        assert!(!created);
        assert_eq!(resolved.session_id, worker_session.session_id);
        assert_eq!(
            storage_backend
                .get_agent_thread("alice", &worker_agent.agent_id)
                .expect("get agent thread")
                .expect("agent thread")
                .session_id,
            worker_session.session_id
        );
    }

    #[tokio::test]
    async fn swarm_main_thread_strategy_creates_worker_main_thread_when_missing() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("swarm-main-thread-create.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let worker_agent = sample_agent_record();
        storage_backend
            .upsert_user_agent(&worker_agent)
            .expect("upsert worker agent");

        let (resolved, created) =
            crate::services::swarm::beeroom::resolve_or_create_agent_main_session(
                storage_backend.as_ref(),
                "alice",
                &worker_agent,
            )
            .expect("create main session");

        assert!(created);
        assert_eq!(
            resolved.agent_id.as_deref(),
            Some(worker_agent.agent_id.as_str())
        );
        assert!(resolved.parent_session_id.is_none());
        assert!(resolved.spawned_by.is_none());
        assert_eq!(
            storage_backend
                .get_agent_thread("alice", &worker_agent.agent_id)
                .expect("get agent thread")
                .expect("agent thread")
                .session_id,
            resolved.session_id
        );
        assert_eq!(
            storage_backend
                .get_chat_session("alice", &resolved.session_id)
                .expect("load created session")
                .expect("created session")
                .session_id,
            resolved.session_id
        );
    }

    #[tokio::test]
    async fn agent_swarm_batch_send_missing_message_returns_actionable_failure_and_skips_team_run()
    {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("swarm-batch-send-validation.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let parent_agent = sample_parent_agent_record();
        let worker_agent = sample_agent_record();
        storage_backend
            .upsert_user_agent(&parent_agent)
            .expect("upsert parent agent");
        storage_backend
            .upsert_user_agent(&worker_agent)
            .expect("upsert worker agent");

        let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
        parent_session.session_id = "sess_parent".to_string();
        parent_session.tool_overrides = vec!["agent_swarm".to_string()];
        storage_backend
            .upsert_chat_session(&parent_session)
            .expect("upsert parent session");

        let workspace_root = dir.path().join("workspace");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage_backend.clone(),
            0,
            &HashMap::new(),
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let config = Config::default();
        let a2a_store = A2aStore::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let context = ToolContext {
            user_id: "alice",
            session_id: "sess_parent",
            workspace_id: "workspace-test",
            agent_id: Some("agent_parent"),
            user_round: Some(1),
            model_round: Some(1),
            is_admin: false,
            storage: storage_backend.clone(),
            orchestrator: None,
            monitor: None,
            beeroom_realtime: None,
            workspace,
            lsp_manager,
            config: &config,
            a2a_store: &a2a_store,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };

        let result = agent_swarm_batch_send(
            &context,
            &json!({
                "action": "batch_send",
                "tasks": [
                    { "agent_name": "worker_a" }
                ]
            }),
        )
        .await
        .expect("batch send result");

        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(
            result.pointer("/error_meta/code").and_then(Value::as_str),
            Some("TOOL_ARGS_MISSING_FIELD")
        );
        assert_eq!(
            result.pointer("/data/task_index").and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            result
                .pointer("/data/example/action")
                .and_then(Value::as_str),
            Some("batch_send")
        );
        let (runs, total) = storage_backend
            .list_team_runs("alice", Some("hive_policy"), Some("sess_parent"), 0, 20)
            .expect("list team runs");
        assert_eq!(total, 0);
        assert!(runs.is_empty());
    }

    #[tokio::test]
    async fn agent_swarm_send_missing_target_returns_actionable_failure() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("swarm-send-validation.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let parent_agent = sample_parent_agent_record();
        storage_backend
            .upsert_user_agent(&parent_agent)
            .expect("upsert parent agent");

        let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
        parent_session.session_id = "sess_parent".to_string();
        parent_session.tool_overrides = vec!["agent_swarm".to_string()];
        storage_backend
            .upsert_chat_session(&parent_session)
            .expect("upsert parent session");

        let workspace_root = dir.path().join("workspace");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage_backend.clone(),
            0,
            &HashMap::new(),
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let config = Config::default();
        let a2a_store = A2aStore::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let context = ToolContext {
            user_id: "alice",
            session_id: "sess_parent",
            workspace_id: "workspace-test",
            agent_id: Some("agent_parent"),
            user_round: Some(1),
            model_round: Some(1),
            is_admin: false,
            storage: storage_backend,
            orchestrator: None,
            monitor: None,
            beeroom_realtime: None,
            workspace,
            lsp_manager,
            config: &config,
            a2a_store: &a2a_store,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };

        let result = agent_swarm_send(
            &context,
            &json!({
                "action": "send",
                "message": "请总结政府退休政策的核心要点。"
            }),
        )
        .await
        .expect("send result");

        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(
            result.pointer("/error_meta/code").and_then(Value::as_str),
            Some("TOOL_ARGS_MISSING_FIELD")
        );
        assert_eq!(
            result
                .pointer("/data/example/agent_name")
                .and_then(Value::as_str),
            Some("worker_a")
        );
    }

    #[test]
    fn resolve_swarm_wait_mode_defaults_to_infinite_when_config_is_zero() {
        assert!(matches!(
            resolve_swarm_wait_mode(None, 0),
            SwarmWaitMode::Infinite
        ));
        assert!(matches!(
            resolve_swarm_wait_mode(Some(0.0), 0),
            SwarmWaitMode::Immediate
        ));
        assert!(matches!(
            resolve_swarm_wait_mode(Some(12.0), 0),
            SwarmWaitMode::Finite(timeout) if (timeout - 12.0).abs() < f64::EPSILON
        ));
        assert!(matches!(
            resolve_swarm_wait_mode(None, 30),
            SwarmWaitMode::Finite(timeout) if (timeout - 30.0).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn background_child_runs_enable_parent_auto_wake() {
        assert!(should_auto_wake_parent_after_child_run(false, 0.0));
        assert!(!should_auto_wake_parent_after_child_run(false, 5.0));
        assert!(!should_auto_wake_parent_after_child_run(true, 0.0));
        assert!(should_auto_wake_parent_follow_up(false, false, 0.0));
        assert!(!should_auto_wake_parent_follow_up(true, false, 0.0));
    }

    #[test]
    fn enrich_agent_swarm_spawn_response_preserves_spawn_contract() {
        let response = enrich_agent_swarm_spawn_response(json!({
            "ok": true,
            "action": "send",
            "state": "accepted",
            "summary": "Worker task was queued and is still running.",
            "data": {
                "run_id": "run_swarm_demo",
                "session_id": "sess_worker_demo",
                "team_run_id": "team_demo",
                "task_id": "task_demo"
            }
        }));

        assert_eq!(
            response.get("action").and_then(Value::as_str),
            Some("spawn")
        );
        assert_eq!(
            response.pointer("/data/session_id").and_then(Value::as_str),
            Some("sess_worker_demo")
        );
        assert_eq!(
            response
                .pointer("/data/child_session_id")
                .and_then(Value::as_str),
            Some("sess_worker_demo")
        );
        assert_eq!(
            response.pointer("/data/run_id").and_then(Value::as_str),
            Some("run_swarm_demo")
        );
        assert_eq!(
            response.pointer("/data/spawned").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            response.get("state").and_then(Value::as_str),
            Some("accepted")
        );
    }

    #[test]
    fn sync_announce_auto_wake_updates_run_metadata() {
        let mut announce = AnnounceConfig {
            parent_session_id: "sess_parent".to_string(),
            label: None,
            dispatch_id: None,
            strategy: None,
            completion_mode: None,
            remaining_action: None,
            parent_turn_ref: None,
            parent_user_round: None,
            parent_model_round: None,
            emit_parent_events: true,
            auto_wake: false,
            persist_history_message: false,
        };
        let mut run_metadata = json!({});

        sync_announce_auto_wake(&mut announce, Some(&mut run_metadata), true);

        assert!(announce.auto_wake);
        assert_eq!(
            run_metadata.get("auto_wake").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn build_parent_follow_up_announce_keeps_turn_context_for_auto_wake() {
        let announce = build_parent_follow_up_announce(
            Some("sess_parent".to_string()),
            "sess_child",
            Some("worker".to_string()),
            true,
            false,
            true,
            Some("subagent_turn:3:2".to_string()),
            Some(3),
            Some(2),
        )
        .expect("announce");

        assert_eq!(announce.parent_session_id, "sess_parent");
        assert_eq!(
            announce.parent_turn_ref.as_deref(),
            Some("subagent_turn:3:2")
        );
        assert_eq!(announce.parent_user_round, Some(3));
        assert_eq!(announce.parent_model_round, Some(2));
        assert!(announce.auto_wake);
        assert!(!announce.persist_history_message);
    }

    #[test]
    fn build_parent_follow_up_announce_rejects_same_session() {
        assert!(build_parent_follow_up_announce(
            Some("sess_same".to_string()),
            "sess_same",
            None,
            true,
            false,
            true,
            None,
            None,
            None,
        )
        .is_none());
    }

    #[test]
    fn filter_tool_names_by_model_capability_blocks_read_image_when_vision_disabled() {
        let names = HashSet::from([
            read_image_tool::TOOL_READ_IMAGE.to_string(),
            read_image_tool::TOOL_READ_IMAGE_ALIAS.to_string(),
            "read_file".to_string(),
        ]);
        let filtered = filter_tool_names_by_model_capability(names, false);
        assert!(!filtered.contains(read_image_tool::TOOL_READ_IMAGE));
        assert!(!filtered.contains(read_image_tool::TOOL_READ_IMAGE_ALIAS));
        assert!(filtered.contains("read_file"));
    }

    #[test]
    fn normalize_ptc_script_name_accepts_simple_filename() {
        let script = normalize_ptc_script_name("demo").expect("filename should be normalized");
        assert_eq!(script, PathBuf::from("demo.py"));
    }

    #[test]
    fn normalize_ptc_script_name_rejects_path_segments() {
        let error = normalize_ptc_script_name("nested/demo.py").expect_err("path must be rejected");
        assert_eq!(error, "tool.ptc.filename_invalid");
    }

    #[test]
    fn normalize_ptc_script_name_rejects_non_python_extension() {
        let error = normalize_ptc_script_name("demo.txt").expect_err("non-python ext should fail");
        assert_eq!(error, "tool.ptc.ext_invalid");
    }

    #[cfg(windows)]
    #[test]
    fn decode_command_output_prefers_gbk_when_utf8_lossy_contains_replacements() {
        let expected = "\u{65e0}\u{6cd5}\u{5c06} pip \u{8bc6}\u{522b}\u{4e3a} cmdlet";
        let (encoded, _, _) = GBK.encode(expected);
        let decoded = decode_command_output(encoded.as_ref());
        assert!(decoded.contains("\u{65e0}\u{6cd5}\u{5c06}"));
        assert!(decoded.contains("cmdlet"));
    }

    #[cfg(windows)]
    #[test]
    fn decode_command_output_handles_utf16_le_streams() {
        let expected = "PowerShell output";
        let utf16_bytes = expected
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect::<Vec<_>>();
        let decoded = decode_command_output(&utf16_bytes);
        assert_eq!(decoded, expected);
    }
}
