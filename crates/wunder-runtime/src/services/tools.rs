// Builtin tool definitions and execution entrypoint.
// NOTE FOR CONTRIBUTORS:
// This file is in maintenance mode due to its size and complexity.
// Do not add new tool business logic directly in `tools.rs`.
// Implement new capabilities in dedicated modules/files and only wire them here.
mod a2a_tool;
mod agent_swarm_tool;
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
#[cfg(feature = "desktop-control")]
mod desktop_control_impl;
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
#[cfg(any(feature = "web-fetch", test))]
mod web_fetch_provider;
#[cfg(feature = "web-fetch")]
mod web_fetch_provider_impl;
mod web_fetch_tool;
#[cfg(feature = "web-fetch")]
mod web_fetch_tool_impl;
mod web_search_tool;

pub(crate) use agent_swarm_tool::{agent_swarm, current_agent_id};
#[cfg(test)]
pub(crate) use agent_swarm_tool::{
    agent_swarm_batch_send, agent_swarm_send, enrich_agent_swarm_spawn_response,
    infer_swarm_agent_name_from_task_message,
};
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
        "send" | "sessions_send" | "session_send" => sessions_send(context, args).await,
        "spawn" | "sessions_spawn" | "session_spawn" | "会话派生" | "派生" => {
            sessions_spawn(context, args).await
        }
        _ => Err(anyhow!("未知子智能体控制 action: {action}")),
    }
}

#[cfg(test)]
mod tests;
