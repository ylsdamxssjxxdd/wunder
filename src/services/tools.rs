// Builtin tool definitions and execution entrypoint.
// NOTE FOR CONTRIBUTORS:
// This file is in maintenance mode due to its size and complexity.
// Do not add new tool business logic directly in `tools.rs`.
// Implement new capabilities in dedicated modules/files and only wire them here.
mod a2a_tool;
mod apply_patch_tool;
mod apply_patch_update;
mod browser_tool;
mod catalog;
mod channel_tool;
pub(crate) mod command_options;
pub(crate) mod command_output_guard;
pub mod command_sessions;
mod command_tool;
mod context;
mod desktop_control;
mod dispatch;
mod edit_file2_tool;
mod file_tool;
mod freeform;
mod knowledge_tool;
mod lsp_tool;
mod mcp_pack;
mod memory_manager_tool;
mod multimodal_generation_tool;
mod node_invoke_tool;
mod panel_tools;
mod read_file_guard;
mod read_image_tool;
mod read_indentation;
mod schedule_task_tool;
mod search_content_tool;
mod self_status_tool;
mod session_announce_support;
mod session_run_lifecycle;
mod session_run_stream;
mod session_tool;
mod session_tool_access;
mod session_tool_args;
mod session_tool_support;
pub(crate) mod sessions_yield_tool;
pub(crate) mod skill_call;
mod sleep_tool;
mod subagent_control;
mod swarm_realtime;
mod swarm_run_support;
mod swarm_tool_error;
mod swarm_tool_hint;
mod thread_control_tool;
pub(crate) mod tool_error;
mod user_tool_dispatch;
mod user_world_tool;
mod web_fetch_provider;
mod web_fetch_tool;
mod web_search_tool;

#[cfg(test)]
pub(crate) use catalog::builtin_tool_specs_with_language;
pub use catalog::{
    a2a_service_schema, a2a_service_schema_with_language, browser_tools_available,
    build_desktop_followup_user_message, build_mcp_tool_alias_entries,
    build_mcp_tool_alias_entries_for_names, build_read_image_followup_user_message,
    build_runtime_tool_display_map, builtin_aliases, builtin_tool_specs,
    collect_available_tool_names, collect_enabled_tool_names_for_catalog,
    collect_prompt_tool_specs, collect_prompt_tool_specs_with_language, desktop_tools_available,
    extract_sleep_seconds, filter_tool_names_by_model_capability, is_browser_tool_name,
    is_desktop_control_tool_name, is_read_image_tool_name, is_sleep_tool_name,
    resolve_runtime_tool_display_name, resolve_tool_name,
};
pub use context::{build_tool_roots, ToolContext, ToolEventEmitter, ToolRoots};
pub(crate) use context::{
    collect_allow_roots, collect_read_roots, resolve_tool_path, roots_allow_any_path,
};
pub use dispatch::{execute_builtin_tool, execute_tool};
pub(crate) use freeform::{
    build_responses_freeform_tool, extract_freeform_tool_input, is_freeform_tool_name,
    render_prompt_tool_spec,
};
pub(crate) use lsp_tool::touch_lsp_file;
pub(crate) use mcp_pack::{
    runtime_name as mcp_pack_runtime_name, schema as mcp_pack_schema,
    spec_for_server as mcp_pack_spec_for_server, MCP_PACK_TOOL_NAME,
};
pub(crate) use memory_manager_tool::execute_memory_manager_tool;
pub(crate) use session_announce_support::{
    append_child_announce, build_parent_follow_up_announce, insert_run_metadata_field,
    should_auto_wake_parent_after_child_run, should_auto_wake_parent_follow_up,
    should_skip_announce, sync_announce_auto_wake, AnnounceConfig,
};
pub(crate) use session_run_lifecycle::{
    cleanup_session, load_session_messages, prepare_child_session, prepare_swarm_child_session,
    spawn_session_run, PreparedChildSession, SessionRunMeta,
};
pub(crate) use session_tool::{sessions_history, sessions_list, sessions_send, sessions_spawn};
pub(crate) use session_tool_access::{
    apply_tool_overrides, build_effective_tool_names, collect_user_allowed_tools,
    finalize_tool_names, is_agent_allowed_by_access, load_agent_record,
    resolve_child_session_tool_names, resolve_session_tool_overrides, ChildSessionToolMode,
};
pub(crate) use session_tool_args::{
    AgentSwarmBatchSendArgs, AgentSwarmControlArgs, AgentSwarmListArgs, AgentSwarmSendArgs,
    AgentSwarmStatusArgs, AgentSwarmWaitArgs, SessionHistoryArgs, SessionListArgs, SessionSendArgs,
    SessionSpawnArgs, SubagentControlArgs,
};
pub(crate) use session_tool_support::{
    build_agent_swarm_tool_result, build_session_tool_result, clamp_limit,
    compact_swarm_run_result_preview, dedupe_non_empty_strings, format_ts, is_swarm_run_failed,
    is_swarm_run_terminal, is_swarm_task_terminal_status, normalize_optional_string,
    normalize_swarm_poll_interval, normalize_tool_run_state, now_ts, parse_cleanup_mode,
    parse_swarm_worker_thread_strategy, resolve_session_key, resolve_swarm_batch_session_key,
    resolve_swarm_wait_mode, session_cleanup_label, skipped_swarm_task_result,
    swarm_wait_seconds_value, truncate_text, SessionCleanup, SwarmWaitMode,
    SwarmWorkerThreadStrategy, SWARM_WAIT_DEFAULT_POLL_S,
};
pub(crate) use swarm_run_support::{
    claim_swarm_mother_for_context, create_swarm_team_run_record, create_swarm_team_task_record,
    wait_for_swarm_runs,
};
pub(crate) use thread_control_tool::execute_thread_control_tool;
pub(crate) use user_tool_dispatch::{execute_mcp_tool, execute_user_tool, is_mcp_tool_name};

use crate::config::Config;
use crate::core::tool_args::recover_tool_args_value as recover_tool_args_value_lossy;
use crate::i18n;
use crate::orchestrator_constants::truncate_tool_result_text;
use crate::sandbox;
use crate::schemas::WunderRequest;
use crate::services::orchestration_context::{
    active_orchestration_for_agent, build_worker_dispatch_message,
    ensure_orchestration_member_session, load_dispatch_context, persist_session_context,
    session_has_visible_history, session_orchestration_run_root, OrchestrationSessionContext,
    ORCHESTRATION_MODE,
};
use crate::services::orchestration_run_control::worker_already_dispatched_in_round;
use crate::services::swarm::beeroom::{
    agent_in_hive, build_swarm_dispatch_message,
    ensure_swarm_agent_in_hive as ensure_swarm_agent_in_beeroom,
    resolve_swarm_hive_id as resolve_swarm_hive_scope,
};
use crate::skills::SkillRegistry;
use crate::storage::{normalize_hive_id, ChatSessionRecord, UserAgentRecord};
use anyhow::{anyhow, Result};
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use swarm_realtime::{
    apply_session_run_to_swarm_task, emit_swarm_run_started, emit_swarm_run_terminal,
    emit_swarm_task_dispatched, emit_swarm_task_updated, sync_swarm_run_summary,
};
use swarm_tool_error::{
    agent_swarm_batch_send_example, agent_swarm_send_example, agent_swarm_spawn_example,
    agent_swarm_wait_example, build_agent_swarm_args_failure,
};
use swarm_tool_hint::resolve_swarm_agent_record;
use uuid::Uuid;

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
const MAX_SESSION_LIST_ITEMS: i64 = 200;
const MAX_SESSION_HISTORY_ITEMS: i64 = 500;
const MAX_SESSION_MESSAGE_ITEMS: i64 = 50;
const LOCAL_PTC_TIMEOUT_S: u64 = 60;
const LOCAL_PTC_DIR_NAME: &str = "ptc_temp";
const TOOL_OVERRIDE_NONE: &str = "__no_tools__";

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

pub(crate) fn collect_orchestration_aware_allow_roots(context: &ToolContext<'_>) -> Vec<PathBuf> {
    let mut roots = collect_allow_roots(context);
    roots.extend(collect_orchestration_run_roots(context));
    roots
}

pub(crate) use edit_file2_tool::edit_file2;

pub(crate) async fn execute_in_sandbox(
    context: &ToolContext<'_>,
    tool: &str,
    args: &Value,
) -> Option<Value> {
    if !sandbox::sandbox_enabled(context.config) {
        return None;
    }
    Some(
        sandbox::execute_tool(
            context.config,
            context.workspace.as_ref(),
            context.user_id,
            context.workspace_id,
            context.session_id,
            tool,
            args,
            context.user_tool_bindings,
        )
        .await,
    )
}

pub(crate) fn recover_tool_args_value(args: &Value) -> Value {
    recover_tool_args_value_lossy(args)
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
        "send" | "sessions_send" | "session_send" => {
            sessions_send(context, args).await
        }
        "spawn" | "sessions_spawn" | "session_spawn" | "会话派生" | "派生" => {
            sessions_spawn(context, args).await
        }
        _ => Err(anyhow!("未知子智能体控制 action: {action}")),
    }
}

#[derive(Debug, Default, Clone)]
struct AgentSwarmRuntime {
    lock_sessions: HashSet<String>,
    running_sessions: HashSet<String>,
}

#[derive(Debug, Clone)]
struct SwarmBatchDispatchTask {
    index: usize,
    agent_id: String,
    agent_name: String,
    session_id: String,
    created_session: bool,
    thread_strategy: &'static str,
    team_task_id: String,
    message: String,
    label: Option<String>,
    tool_names: Vec<String>,
    model_name: Option<String>,
    agent_prompt: Option<String>,
    preview_skill: bool,
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
        workspace_container_id: None,
        model_name: model_name.clone(),
        language: Some(i18n::get_language()),
        config_overrides: context.request_config_overrides.cloned(),
        agent_prompt: task.agent_prompt,
        preview_skill: task.preview_skill,
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
            "purpose": "continue waiting for 60 seconds",
            "args": {
                "action": "wait",
                "run_ids": [run_id],
                "wait_seconds": 60
            }
        }));
        suggestions.push(json!({
            "purpose": "inspect current snapshot",
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
            "purpose": "inspect worker session history",
            "args": {
                "action": "history",
                "session_id": session_key
            }
        }));
    }
    json!({
        "note": "This wait timed out; the worker may still be running.",
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
        "note": "Some tasks are not finished; continue waiting or inspect a snapshot.",
        "run_ids": run_ids,
        "suggested_calls": [
            {
                "purpose": "continue waiting for 60 seconds",
                "args": {
                    "action": "wait",
                    "run_ids": run_ids,
                    "wait_seconds": 60
                }
            },
            {
                "purpose": "inspect current snapshot",
                "args": {
                    "action": "wait",
                    "run_ids": run_ids,
                    "wait_seconds": 0
                }
            }
        ]
    })
}

async fn agent_swarm(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmControlArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "unknown",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm arguments are invalid: {err}"),
                "Provide action and use one of list/status/send/history/spawn/batch_send/wait.",
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
            "Provide action and use one of list/status/send/history/spawn/batch_send/wait.",
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
            "Use one of list/status/send/history/spawn/batch_send/wait with the required fields.",
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
                "Provide action=\"send\", a non-empty message, and one of agent_name/agent_id/session_id.",
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
            "Provide a non-empty message and identify the target with agent_name, agent_id, or session_id.",
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
                "threadStrategy must be fresh_main_thread or main_thread; reuseMainThread=true is also supported.",
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
            "Provide at least one target field: agent_name, agent_id, or session_id, then send message.",
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
                    ) {
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
    if let Some(orchestration_context) = orchestration_context.as_ref() {
        persist_session_context(
            context.storage.as_ref(),
            user_id,
            &target_session_id,
            &OrchestrationSessionContext {
                mode: ORCHESTRATION_MODE.to_string(),
                run_id: orchestration_context.run_id.clone(),
                group_id: orchestration_context.group_id.clone(),
                role: "worker".to_string(),
                round_index: orchestration_context.round_index,
                mother_agent_id: orchestration_context.mother_agent_id.clone(),
            },
        )?;
    }
    context.storage.upsert_team_task(&task_record)?;
    emit_swarm_task_dispatched(context, &run_record, &task_record);

    if worker_already_dispatched_in_round(
        context.storage.as_ref(),
        user_id,
        context.session_id,
        &target_session_id,
    )? {
        task_record.status = "success".to_string();
        task_record.result_summary = Some("already_dispatched_this_round".to_string());
        task_record.updated_time = now_ts();
        task_record.finished_time = Some(task_record.updated_time);
        task_record.elapsed_s = Some(0.0);
        context.storage.upsert_team_task(&task_record)?;
        emit_swarm_task_updated(context, &run_record, &task_record);
        let (terminal, failed) =
            sync_swarm_run_summary(context, &mut run_record, std::slice::from_ref(&task_record))?;
        if terminal {
            emit_swarm_run_terminal(context, &run_record, failed);
        }
        return Ok(skipped_swarm_task_result(
            "send",
            &task_record.task_id,
            &target_session_id,
            &target_agent_id,
            &target_agent.name,
            "already_dispatched_this_round",
        ));
    }

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
}

async fn agent_swarm_batch_send(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmBatchSendArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm batch_send arguments are invalid: {err}"),
                "Provide action=\"batch_send\" and non-empty tasks. Each task needs a target and message.",
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
            "Provide non-empty tasks. Each task needs a target field and a non-empty message.",
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
            format!("Reduce tasks to {max_tasks} or split the batch_send call."),
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
                "threadStrategy must be fresh_main_thread or main_thread; reuseMainThread=true is also supported.",
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
        let task_message =
            normalize_optional_string(task.message.clone()).or_else(|| shared_message.clone());
        let inferred_agent_name = task_message
            .as_deref()
            .and_then(infer_swarm_agent_name_from_task_message);
        let has_target = normalize_optional_string(task.agent_id.clone()).is_some()
            || normalize_optional_string(task.agent_name.clone()).is_some()
            || inferred_agent_name.is_some()
            || resolve_swarm_batch_session_key(task.session_key.clone())
                .ok()
                .flatten()
                .is_some();
        if !has_target {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_MISSING_FIELD",
                format!(
                    "agent_swarm batch_send task[{index}] requires agent_id/agent_name or session_id"
                ),
                "Each task needs one of agent_name, agent_id, or session_id.",
                &["tasks[].agent_name|agent_id|session_id"],
                agent_swarm_batch_send_example(),
                args,
                json!({
                    "task_index": index,
                    "expected_task_shape": {
                        "agent_name": "worker_a",
                        "message": "Summarize the requested material."
                    }
                }),
            ));
        }
        let has_message = task_message.is_some();
        if !has_message {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_MISSING_FIELD",
                format!("agent_swarm batch_send task[{index}] requires message"),
                "Provide a non-empty message for each task or a shared top-level message.",
                &["tasks[].message"],
                agent_swarm_batch_send_example(),
                args,
                json!({
                    "task_index": index,
                    "expected_task_shape": {
                        "agent_name": "worker_a",
                        "message": "Summarize the requested material."
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
        // Batch send should only validate the concrete requested targets.
        // Unrelated agents from other hives must not poison the local lookup cache.
        if !agent_in_hive(&agent, &swarm_hive_id) {
            continue;
        }
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
    let mut indexed_items = Vec::new();
    let mut run_ids = Vec::new();
    for (index, task) in payload.tasks.into_iter().enumerate() {
        let message = normalize_optional_string(task.message)
            .or_else(|| shared_message.clone())
            .ok_or_else(|| anyhow!("agent_swarm batch_send task[{index}] requires message"))?;
        let label = normalize_optional_string(task.label).or_else(|| shared_label.clone());
        let include_current = task.include_current.unwrap_or(default_include_current);
        let requested_agent_id = normalize_optional_string(task.agent_id);
        let requested_agent_name = normalize_optional_string(task.agent_name)
            .or_else(|| infer_swarm_agent_name_from_task_message(&message));
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
                    "Each task threadStrategy must be fresh_main_thread or main_thread; reuseMainThread=true is also supported.",
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
        let requested_session_id = resolve_swarm_batch_session_key(task.session_key)?;

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
        let dispatch_message = build_swarm_dispatch_message(
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
        let (
            session_id,
            created_session,
            resolved_thread_strategy,
            tool_names,
            model_name,
            agent_prompt,
            preview_skill,
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
                agent_record.preview_skill,
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
                        agent_record.preview_skill,
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
                        prepared.request.preview_skill,
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
        if let Some(orchestration_context) = orchestration_context.as_ref() {
            persist_session_context(
                context.storage.as_ref(),
                user_id,
                &session_id,
                &OrchestrationSessionContext {
                    mode: ORCHESTRATION_MODE.to_string(),
                    run_id: orchestration_context.run_id.clone(),
                    group_id: orchestration_context.group_id.clone(),
                    role: "worker".to_string(),
                    round_index: orchestration_context.round_index,
                    mother_agent_id: orchestration_context.mother_agent_id.clone(),
                },
            )?;
        }
        if worker_already_dispatched_in_round(
            context.storage.as_ref(),
            user_id,
            context.session_id,
            &session_id,
        )? {
            let skipped_task_id = format!("task_{}", Uuid::new_v4().simple());
            indexed_items.push((
                index,
                skipped_swarm_task_result(
                    "batch_send",
                    &skipped_task_id,
                    &session_id,
                    &agent_record.agent_id,
                    &agent_record.name,
                    "already_dispatched_this_round",
                ),
            ));
            continue;
        }

        dispatch_plan.push(SwarmBatchDispatchTask {
            index,
            agent_id: agent_record.agent_id,
            agent_name: agent_record.name,
            session_id,
            created_session,
            thread_strategy: resolved_thread_strategy,
            team_task_id: String::new(),
            message: dispatch_message,
            label,
            tool_names,
            model_name,
            agent_prompt,
            preview_skill,
        });
    }

    if dispatch_plan.is_empty() {
        indexed_items.sort_by_key(|(index, _)| *index);
        let items = indexed_items
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>();
        return Ok(build_model_tool_success(
            "batch_send",
            "skipped",
            "All swarm tasks were already dispatched in this round.",
            json!({
                "items": items,
                "task_total": 0,
                "task_success": 0,
                "task_failed": 0,
                "skip_reason": "already_dispatched_this_round",
                "team_run_id": Value::Null,
            }),
        ));
    }

    let mut run_record = create_swarm_team_run_record(
        context,
        user_id,
        &swarm_hive_id,
        mother_agent_id,
        payload.team_run_id.as_deref(),
        "batch_send",
        dispatch_plan.len(),
    );
    context.storage.upsert_team_run(&run_record)?;
    emit_swarm_run_started(context, &run_record);

    let mut task_records_by_index = HashMap::new();
    for task in &mut dispatch_plan {
        let task_record = create_swarm_team_task_record(
            &run_record,
            &task.agent_id,
            Some(task.session_id.clone()),
            task.created_session.then_some(task.session_id.clone()),
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
            &task.message,
        )?;
        task.message = dispatch_message;
        task.team_task_id = task_record.task_id.clone();
        context.storage.upsert_team_task(&task_record)?;
        emit_swarm_task_dispatched(context, &run_record, &task_record);
        task_records_by_index.insert(task.index, task_record.clone());
    }

    let dispatch_targets_by_index = dispatch_plan
        .iter()
        .map(|task| {
            (
                task.index,
                json!({
                    "index": task.index,
                    "agent_id": task.agent_id,
                    "target_agent_id": task.agent_id,
                    "agent_name": task.agent_name,
                    "target_agent_name": task.agent_name,
                    "session_id": task.session_id,
                    "target_session_id": task.session_id,
                    "created_session": task.created_session,
                    "thread_strategy": task.thread_strategy,
                }),
            )
        })
        .collect::<HashMap<_, _>>();

    let dispatch_parallelism = dispatch_plan.len().min(max_tasks).max(1);
    let mut dispatches = stream::iter(dispatch_plan.into_iter().map(|task| async move {
        let index = task.index;
        let result = dispatch_swarm_batch_task(context, task).await;
        (index, result)
    }))
    .buffer_unordered(dispatch_parallelism);

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
                    "agent_name": tool_result_field_or_null(&result, "agent_name"),
                    "target_agent_name": tool_result_field_or_null(&result, "agent_name"),
                    "session_id": tool_result_field_or_null(&result, "session_id"),
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
                let mut item = dispatch_targets_by_index
                    .get(&index)
                    .cloned()
                    .unwrap_or_else(|| json!({ "index": index }));
                if let Value::Object(ref mut map) = item {
                    map.insert("status".to_string(), json!("error"));
                    map.insert(
                        "task_id".to_string(),
                        task_records_by_index
                            .get(&index)
                            .map(|item| json!(item.task_id))
                            .unwrap_or(Value::Null),
                    );
                    map.insert("error".to_string(), json!(error_text));
                }
                indexed_items.push((index, item));
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
    let skipped_total = items
        .iter()
        .filter(|item| {
            item.get("skipped")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let failed_total = items
        .len()
        .saturating_sub(accepted_total)
        .saturating_sub(skipped_total);
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
    } else if skipped_total > 0 && failed_total == 0 {
        "skipped".to_string()
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
                "Provide action=\"wait\" and run_ids or run_id.",
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
            "Provide run_ids or run_id. Usually copy run_id from a send/batch_send result first.",
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
                "Provide action=\"spawn\", a non-empty task, and agent_name or agent_id.",
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
            "Provide a non-empty task with clear expected output.",
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
            "Provide agent_name or agent_id. Use subagent_control.spawn for a temporary child session.",
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

fn ensure_swarm_agent_in_hive(agent: &UserAgentRecord, hive_id: &str) -> Result<()> {
    ensure_swarm_agent_in_beeroom(agent, hive_id)
}

fn current_agent_id(context: &ToolContext<'_>) -> Option<String> {
    context
        .agent_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn infer_swarm_agent_name_from_task_message(message: &str) -> Option<String> {
    let normalized = message.replace('\r', "\n");
    for marker in ["你的角色：", "你的角色:", "角色：", "角色:", "role:"] {
        if let Some((_, tail)) = normalized.split_once(marker) {
            let candidate = tail
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .trim_matches(|ch| matches!(ch, '"' | '\'' | '`' | '“' | '”' | '。' | '，' | ','));
            if !candidate.is_empty() && candidate.chars().count() <= 64 {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a_store::A2aStore;
    use crate::config::LlmModelConfig;
    use crate::lsp::LspManager;
    use crate::storage::{
        AgentThreadRecord, ChatSessionRecord, SqliteStorage, StorageBackend, UserAgentRecord,
    };
    use crate::workspace::WorkspaceManager;
    #[cfg(windows)]
    use encoding_rs::GBK;
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
            name: "worker_a".to_string(),
            description: String::new(),
            system_prompt: "use policy knowledge".to_string(),
            preview_skill: false,
            model_name: None,
            ability_items: Vec::new(),
            tool_names: vec!["skill_creator".to_string()],
            declared_tool_names: vec!["read_file".to_string()],
            declared_skill_names: vec!["sample_skill".to_string()],
            visible_unit_ids: Vec::new(),
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
            max_rounds: Some(4),
            max_context: Some(16_384),
            max_output: Some(256),
            thinking_token_budget: None,
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
            ..Default::default()
        }
    }

    fn sample_parent_agent_record() -> UserAgentRecord {
        UserAgentRecord {
            agent_id: "agent_parent".to_string(),
            user_id: "alice".to_string(),
            hive_id: "hive_policy".to_string(),
            name: "parent_agent".to_string(),
            description: String::new(),
            system_prompt: "coordinate workers".to_string(),
            preview_skill: false,
            model_name: None,
            ability_items: Vec::new(),
            tool_names: vec!["agent_swarm".to_string()],
            declared_tool_names: vec!["agent_swarm".to_string()],
            declared_skill_names: Vec::new(),
            visible_unit_ids: Vec::new(),
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
    fn parse_list_files_pagination_defaults_to_500() {
        let pagination =
            file_tool::parse_list_files_pagination(&json!({})).expect("default pagination");
        assert_eq!(pagination.start, 0);
        assert_eq!(pagination.limit, DEFAULT_LIST_PAGE_LIMIT);
    }

    #[test]
    fn parse_list_files_pagination_accepts_cursor_and_clamps_limit() {
        let pagination = file_tool::parse_list_files_pagination(&json!({
            "cursor": "12",
            "limit": 9999
        }))
        .expect("pagination should parse");
        assert_eq!(pagination.start, 12);
        assert_eq!(pagination.limit, MAX_LIST_ITEMS);
    }

    #[test]
    fn parse_list_files_pagination_rejects_invalid_cursor() {
        let err = file_tool::parse_list_files_pagination(&json!({
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
        let run_root = workspace_root
            .join("workspace-test")
            .join("orchestration")
            .join("orch_demo");
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

        let result = file_tool::write_file(
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

        let page1 =
            file_tool::list_files_inner(&workspace, "admin", ".", &[], 1, 0, 2).expect("page1");
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

        let page2 =
            file_tool::list_files_inner(&workspace, "admin", ".", &[], 1, 2, 2).expect("page2");
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

        let page3 =
            file_tool::list_files_inner(&workspace, "admin", ".", &[], 1, 4, 2).expect("page3");
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
    fn list_files_inner_reads_public_workspace_directory_outside_current_workspace_root() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("list-files-public.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = dir.path().join("workspaces");
        let workspace = WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &HashMap::new(),
        );

        let target_dir = workspace_root
            .join("admin")
            .join("skills")
            .join("my-test-skill");
        std::fs::create_dir_all(target_dir.join("assets")).expect("mkdir");
        std::fs::write(target_dir.join("SKILL.md"), "# demo").expect("write skill");
        std::fs::write(target_dir.join("assets").join("example.txt"), "hello")
            .expect("write asset");

        let value = file_tool::list_files_inner(
            &workspace,
            "admin__c__1",
            "/workspaces/admin/skills/my-test-skill",
            &[PathBuf::from("/")],
            2,
            0,
            20,
        )
        .expect("list files result");

        assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
        let items = value
            .pointer("/data/items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(items.iter().any(|item| item.as_str() == Some("SKILL.md")));
        assert!(items.iter().any(|item| item.as_str() == Some("assets/")));
    }

    #[test]
    fn parse_read_file_specs_accepts_shorthand_path_payload() {
        let specs = file_tool::parse_read_file_specs(&json!({
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
        let specs = file_tool::parse_read_file_specs(&json!({
            "file_path": "README.md",
        }))
        .expect("file_path alias should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "README.md");
        assert_eq!(specs[0].ranges, vec![(1, MAX_READ_LINES)]);
    }

    #[test]
    fn parse_read_file_specs_accepts_offset_and_limit_aliases() {
        let specs = file_tool::parse_read_file_specs(&json!({
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
        let specs = file_tool::parse_read_file_specs(&json!({
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
        let specs = file_tool::parse_read_file_specs(&json!({
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
        let err = file_tool::parse_read_file_specs(&json!({
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
        let err = file_tool::parse_read_file_specs(&json!({
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
        let specs = file_tool::parse_read_file_specs(&json!({
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
        let specs = file_tool::parse_read_file_specs(&json!({
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
        let normalized = file_tool::normalize_read_path_for_workspace(
            "/workspaces/admin/agents/demo.worker-card.json",
            "admin",
        );
        assert_eq!(normalized, "agents/demo.worker-card.json");
    }

    #[test]
    fn normalize_read_path_for_workspace_keeps_mismatched_workspace_id() {
        let normalized = file_tool::normalize_read_path_for_workspace(
            "/workspaces/another_owner/demo.txt",
            "admin",
        );
        assert_eq!(normalized, "/workspaces/another_owner/demo.txt");
    }

    #[test]
    fn normalize_read_path_for_workspace_accepts_legacy_workspace_prefix() {
        let normalized =
            file_tool::normalize_read_path_for_workspace("/workspaces/Cargo.toml", "admin");
        assert_eq!(normalized, "Cargo.toml");
    }

    #[test]
    fn parse_read_file_specs_parses_indentation_mode() {
        let specs = file_tool::parse_read_file_specs(&json!({
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
        assert!(matches!(
            specs[0].mode,
            file_tool::ReadFileMode::Indentation
        ));
        assert_eq!(specs[0].indentation.anchor_line, Some(12));
        assert_eq!(specs[0].indentation.max_levels, 2);
        assert!(specs[0].indentation.include_siblings);
        assert!(!specs[0].indentation.include_header);
        assert_eq!(specs[0].indentation.max_lines, Some(40));
    }

    #[test]
    fn parse_read_budget_reads_nested_and_top_level_fields() {
        let budget = file_tool::parse_read_budget(&json!({
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

        let value = file_tool::read_files_inner(
            &workspace,
            "admin",
            &[],
            vec![file_tool::ReadFileSpec {
                path: "missing.txt".to_string(),
                requested_ranges: vec![(1, 20)],
                ranges: vec![(1, 20)],
                used_default_range: false,
                mode: file_tool::ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            }],
            file_tool::ReadBudget::default(),
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

        let value = file_tool::read_files_inner(
            &workspace,
            "admin",
            &[],
            vec![file_tool::ReadFileSpec {
                path: "heart.png".to_string(),
                requested_ranges: vec![(1, 20)],
                ranges: vec![(1, 20)],
                used_default_range: false,
                mode: file_tool::ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            }],
            file_tool::ReadBudget::default(),
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

        let value = file_tool::read_files_inner(
            &workspace,
            "admin",
            &[],
            vec![file_tool::ReadFileSpec {
                path: "large.md".to_string(),
                requested_ranges: vec![(1, 5)],
                ranges: vec![(1, 5)],
                used_default_range: false,
                mode: file_tool::ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            }],
            file_tool::ReadBudget::default(),
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
        assert_eq!(
            value
                .pointer("/data/patch_usage_hint")
                .and_then(Value::as_str),
            Some(i18n::t("tool.read.patch_usage_hint").as_str())
        );
    }

    #[test]
    fn read_files_inner_prefers_workspace_file_for_relative_path_with_extra_root() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("read-files-relative.db");
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
        let workspace_file = user_root.join("note.txt");
        std::fs::write(&workspace_file, "workspace only\n").expect("write workspace file");
        let extra_root = dir.path().join("extra");
        std::fs::create_dir_all(&extra_root).expect("create extra root");
        std::fs::write(extra_root.join("note.txt"), "extra root\n").expect("write extra file");

        let value = file_tool::read_files_inner(
            &workspace,
            "admin",
            &[extra_root.clone()],
            vec![file_tool::ReadFileSpec {
                path: "note.txt".to_string(),
                requested_ranges: vec![(1, 20)],
                ranges: vec![(1, 20)],
                used_default_range: false,
                mode: file_tool::ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            }],
            file_tool::ReadBudget::default(),
            false,
            1,
            false,
        )
        .expect("read files result");

        assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
        let body = value
            .pointer("/data/content")
            .and_then(Value::as_str)
            .expect("content should exist");
        assert!(body.contains("workspace only"));
        assert!(!body.contains("extra root"));
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

        let value = file_tool::read_files_inner(
            &workspace,
            "admin",
            &[],
            vec![file_tool::ReadFileSpec {
                path: "treaty.md".to_string(),
                requested_ranges: vec![(1, MAX_READ_LINES)],
                ranges: vec![(1, MAX_READ_LINES)],
                used_default_range: true,
                mode: file_tool::ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            }],
            file_tool::ReadBudget::default(),
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
        assert_eq!(
            value
                .pointer("/data/patch_usage_hint")
                .and_then(Value::as_str),
            Some(i18n::t("tool.read.patch_usage_hint").as_str())
        );
    }

    #[test]
    fn compact_command_result_for_model_flattens_output_guard_fields() {
        let value = command_tool::compact_command_result_for_model(&json!({
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
    fn summarize_slice_eof_marks_eof_ranges() {
        let (hit_eof, range_reaches_eof) = file_tool::summarize_slice_eof(&[(100, 200)], 178);
        assert!(hit_eof);
        assert!(range_reaches_eof);

        let (hit_eof, range_reaches_eof) = file_tool::summarize_slice_eof(&[(200, 300)], 178);
        assert!(hit_eof);
        assert!(!range_reaches_eof);

        let (hit_eof, range_reaches_eof) = file_tool::summarize_slice_eof(&[(1, 50)], 178);
        assert!(!hit_eof);
        assert!(!range_reaches_eof);
    }

    #[test]
    fn truncate_utf8_output_respects_char_boundary() {
        let text = "a中b";
        let (truncated, omitted) = file_tool::truncate_utf8_output(text, 2);
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
        let extracted = command_tool::extract_direct_patch_from_command(command);
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
        assert!(command_tool::extract_direct_patch_from_command(command).is_none());
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
            vec!["read_file".to_string(), "sample_skill".to_string()]
        );
    }

    #[test]
    fn resolve_child_session_tool_names_uses_target_agent_defaults_for_swarm_children() {
        let parent_tool_names = vec!["skill_creator".to_string()];
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

        assert_eq!(inherited, vec!["skill_creator".to_string()]);
        assert_eq!(
            swarm_defaults,
            vec!["read_file".to_string(), "sample_skill".to_string()]
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
            Some("worker_a".to_string()),
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

    #[tokio::test]
    async fn prepare_swarm_child_session_uses_target_agent_model_for_initial_run() {
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
    fn swarm_batch_helpers_ignore_artifact_session_id_and_infer_role_name() {
        assert_eq!(
            resolve_swarm_batch_session_key(Some("orchestration/artifact/round_01/".to_string()))
                .expect("resolve artifact path session key"),
            None
        );
        assert_eq!(
            resolve_swarm_batch_session_key(Some(
                "/workspaces/admin__c__1/orchestration/artifact".to_string()
            ))
            .expect("resolve public path session key"),
            None
        );
        assert_eq!(
            resolve_swarm_batch_session_key(Some("sess_worker".to_string()))
                .expect("resolve normal session key")
                .as_deref(),
            Some("sess_worker")
        );
        assert_eq!(
            infer_swarm_agent_name_from_task_message(
                "Task 1\nrole: worker_a\nComplete the assigned task."
            )
            .as_deref(),
            Some("worker_a")
        );
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
    fn skipped_swarm_task_result_exposes_skipped_reason_in_data() {
        let result = skipped_swarm_task_result(
            "batch_send",
            "task_a",
            "sess_a",
            "agent_a",
            "Worker A",
            "already_dispatched_this_round",
        );
        assert_eq!(
            tool_result_field(&result, "state").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            tool_result_field(&result, "task_id").and_then(Value::as_str),
            Some("task_a")
        );
        assert_eq!(
            tool_result_field(&result, "skip_reason").and_then(Value::as_str),
            Some("already_dispatched_this_round")
        );
        assert_eq!(tool_result_field(&result, "run_id"), Some(&Value::Null));
    }

    #[test]
    fn batch_send_all_skipped_response_keeps_team_run_null() {
        let result = build_model_tool_success(
            "batch_send",
            "skipped",
            "All swarm tasks were already dispatched in this round.",
            json!({
                "items": [
                    tool_result_data(&skipped_swarm_task_result(
                        "batch_send",
                        "task_a",
                        "sess_a",
                        "agent_a",
                        "Worker A",
                        "already_dispatched_this_round"
                    )).clone()
                ],
                "task_total": 0,
                "task_success": 0,
                "task_failed": 0,
                "skip_reason": "already_dispatched_this_round",
                "team_run_id": Value::Null,
            }),
        );
        assert_eq!(
            tool_result_field(&result, "state").and_then(Value::as_str),
            Some("skipped")
        );
        assert_eq!(
            tool_result_field(&result, "team_run_id"),
            Some(&Value::Null)
        );
        assert_eq!(
            tool_result_field(&result, "skip_reason").and_then(Value::as_str),
            Some("already_dispatched_this_round")
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
    async fn agent_swarm_batch_send_ignores_artifact_path_session_id_and_infers_agent_name() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("swarm-batch-send-artifact-session.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let parent_agent = sample_parent_agent_record();
        let mut worker_agent = sample_agent_record();
        worker_agent.name = "worker_a".to_string();
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
                    {
                        "session_id": "orchestration/artifact/round_01/",
                        "message": "Task 1\nrole: worker_a\nComplete the assigned task."
                    }
                ]
            }),
        )
        .await
        .expect("batch send result");

        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            result.pointer("/data/counts/total").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            result
                .pointer("/data/items/0/agent_name")
                .and_then(Value::as_str),
            Some("worker_a")
        );
        assert_ne!(
            result
                .pointer("/data/items/0/session_id")
                .and_then(Value::as_str),
            Some("orchestration/artifact/round_01/")
        );
    }

    #[tokio::test]
    async fn agent_swarm_batch_send_ignores_other_hive_agents_during_prefetch() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("swarm-batch-send-cross-hive-prefetch.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let storage_backend: Arc<dyn StorageBackend> = storage.clone();

        let parent_agent = sample_parent_agent_record();
        let worker_agent = sample_agent_record();
        let mut other_hive_agent = sample_agent_record();
        other_hive_agent.agent_id = "agent_other_hive".to_string();
        other_hive_agent.hive_id = "hive_other".to_string();
        other_hive_agent.name = "other hive worker".to_string();
        storage_backend
            .upsert_user_agent(&parent_agent)
            .expect("upsert parent agent");
        storage_backend
            .upsert_user_agent(&worker_agent)
            .expect("upsert worker agent");
        storage_backend
            .upsert_user_agent(&other_hive_agent)
            .expect("upsert other hive agent");

        let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
        parent_session.session_id = "sess_parent".to_string();
        parent_session.tool_overrides = vec!["agent_swarm".to_string()];
        storage_backend
            .upsert_chat_session(&parent_session)
            .expect("upsert parent session");

        let mut worker_session = sample_chat_session_record(&worker_agent.agent_id);
        worker_session.session_id = "sess_worker".to_string();
        storage_backend
            .upsert_chat_session(&worker_session)
            .expect("upsert worker session");

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

        let err = agent_swarm_batch_send(
            &context,
            &json!({
                "action": "batch_send",
                "tasks": [{
                    "session_id": "sess_worker",
                    "agent_name": "wrong worker name",
                    "message": "review this",
                    "wait_seconds": 0
                }]
            }),
        )
        .await
        .expect_err("cross-hive prefetch should not fail before target validation");

        assert!(
            err.to_string()
                .contains("agent_swarm send agent_name does not match target session"),
            "unexpected error: {err}"
        );
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
                "message": "Summarize the requested material."
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
        let script =
            command_tool::normalize_ptc_script_name("demo").expect("filename should be normalized");
        assert_eq!(script, PathBuf::from("demo.py"));
    }

    #[test]
    fn normalize_ptc_script_name_rejects_path_segments() {
        let error = command_tool::normalize_ptc_script_name("nested/demo.py")
            .expect_err("path must be rejected");
        assert_eq!(error, "tool.ptc.filename_invalid");
    }

    #[test]
    fn normalize_ptc_script_name_rejects_non_python_extension() {
        let error = command_tool::normalize_ptc_script_name("demo.txt")
            .expect_err("non-python ext should fail");
        assert_eq!(error, "tool.ptc.ext_invalid");
    }

    #[cfg(windows)]
    #[test]
    fn decode_command_output_prefers_gbk_when_utf8_lossy_contains_replacements() {
        let expected = "\u{65e0}\u{6cd5}\u{5c06} pip \u{8bc6}\u{522b}\u{4e3a} cmdlet";
        let (encoded, _, _) = GBK.encode(expected);
        let decoded = command_tool::decode_command_output(encoded.as_ref());
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
        let decoded = command_tool::decode_command_output(&utf16_bytes);
        assert_eq!(decoded, expected);
    }
}
