use super::*;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;
use tokio::time::sleep;
use uuid::Uuid;

const SUBAGENT_WAIT_DEFAULT_POLL_S: f64 = 1.0;
const SUBAGENT_WAIT_MIN_POLL_S: f64 = 0.2;
const SUBAGENT_WAIT_MAX_POLL_S: f64 = 5.0;
const SUBAGENT_SUMMARY_MAX_CHARS: usize = 160;

#[derive(Debug, Deserialize)]
struct SubagentControlArgs {
    action: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct SubagentTargetArgs {
    #[serde(default, rename = "runIds", alias = "run_ids")]
    run_ids: Option<Vec<String>>,
    #[serde(default, alias = "runId", alias = "run_id")]
    run_id: Option<String>,
    #[serde(default, rename = "sessionIds", alias = "session_ids")]
    session_ids: Option<Vec<String>>,
    #[serde(
        default,
        alias = "sessionId",
        alias = "session_id",
        alias = "childSessionId",
        alias = "child_session_id",
        alias = "sessionKey",
        alias = "session_key"
    )]
    session_id: Option<String>,
    #[serde(default, rename = "dispatchId", alias = "dispatch_id")]
    dispatch_id: Option<String>,
    #[serde(
        default,
        alias = "parentId",
        alias = "parent_id",
        alias = "parentSessionId"
    )]
    parent_id: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SubagentStatusArgs {
    #[serde(flatten)]
    target: SubagentTargetArgs,
}

#[derive(Debug, Deserialize)]
struct SubagentHistoryArgs {
    #[serde(flatten)]
    target: SubagentTargetArgs,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "includeTools", alias = "include_tools")]
    include_tools: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SubagentSendArgs {
    #[serde(flatten)]
    target: SubagentTargetArgs,
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
}

#[derive(Debug, Deserialize)]
struct SubagentWaitArgs {
    #[serde(flatten)]
    target: SubagentTargetArgs,
    #[serde(default, rename = "waitSeconds", alias = "wait_seconds")]
    wait_seconds: Option<f64>,
    #[serde(
        default,
        rename = "pollIntervalSeconds",
        alias = "poll_interval_seconds"
    )]
    poll_interval_seconds: Option<f64>,
    #[serde(
        default,
        rename = "waitMode",
        alias = "wait_mode",
        alias = "completionMode",
        alias = "completion_mode"
    )]
    wait_mode: Option<String>,
    #[serde(
        default,
        rename = "remainingAction",
        alias = "remaining_action",
        alias = "loserAction",
        alias = "loser_action"
    )]
    remaining_action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubagentInterruptArgs {
    #[serde(flatten)]
    target: SubagentTargetArgs,
}

#[derive(Debug, Deserialize)]
struct SubagentSessionControlArgs {
    #[serde(flatten)]
    target: SubagentTargetArgs,
    #[serde(default)]
    cascade: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
struct SubagentBatchTaskArgs {
    #[serde(alias = "message", alias = "prompt")]
    task: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "runTimeoutSeconds", alias = "run_timeout_seconds")]
    run_timeout_seconds: Option<f64>,
    #[serde(default)]
    cleanup: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubagentBatchSpawnArgs {
    #[serde(default)]
    tasks: Vec<SubagentBatchTaskArgs>,
    #[serde(default, alias = "message", alias = "prompt")]
    task: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "runTimeoutSeconds", alias = "run_timeout_seconds")]
    run_timeout_seconds: Option<f64>,
    #[serde(default)]
    cleanup: Option<String>,
    #[serde(default, rename = "dispatchLabel", alias = "dispatch_label")]
    dispatch_label: Option<String>,
    #[serde(default, rename = "waitSeconds", alias = "wait_seconds")]
    wait_seconds: Option<f64>,
    #[serde(
        default,
        rename = "pollIntervalSeconds",
        alias = "poll_interval_seconds"
    )]
    poll_interval_seconds: Option<f64>,
    #[serde(default)]
    strategy: Option<String>,
    #[serde(
        default,
        rename = "remainingAction",
        alias = "remaining_action",
        alias = "loserAction",
        alias = "loser_action"
    )]
    remaining_action: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedTargetSet {
    run_ids: Vec<String>,
    session_ids: Vec<String>,
    dispatch_id: Option<String>,
    parent_id: Option<String>,
    limit: i64,
}

#[derive(Debug, Clone)]
struct SubagentRunSnapshot {
    key: String,
    status: String,
    terminal: bool,
    failed: bool,
    updated_time: f64,
    payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitCompletionMode {
    All,
    Any,
    FirstSuccess,
}

impl WaitCompletionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Any => "any",
            Self::FirstSuccess => "first_success",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatchDispatchStrategy {
    ParallelAll,
    FirstSuccess,
    ReviewThenMerge,
}

impl BatchDispatchStrategy {
    fn as_str(self) -> &'static str {
        match self {
            Self::ParallelAll => "parallel_all",
            Self::FirstSuccess => "first_success",
            Self::ReviewThenMerge => "review_then_merge",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemainingBranchAction {
    Keep,
    Interrupt,
    Close,
}

impl RemainingBranchAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Interrupt => "interrupt",
            Self::Close => "close",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WaitProgressState {
    completion_reached: bool,
    all_finished: bool,
    matched_total: i64,
    matched_success_total: i64,
    matched_failed_total: i64,
    completed_reason: &'static str,
}

pub(super) async fn execute(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentControlArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = payload.action.trim();
    if action.is_empty() {
        return Err(anyhow!("subagent_control action is required"));
    }
    match action.to_ascii_lowercase().as_str() {
        "list" | "sessions_list" | "session_list" | "会话列表" | "列表" => {
            list(context, args).await
        }
        "history" | "sessions_history" | "session_history" | "会话历史" | "历史" => {
            history(context, args).await
        }
        "send" | "sessions_send" | "session_send" | "会话发送" | "发送" => {
            send(context, args).await
        }
        "spawn" | "sessions_spawn" | "session_spawn" | "会话派生" | "派生" => {
            super::sessions_spawn(context, args).await
        }
        "batch_spawn" | "batchspawn" | "dispatch" | "批量派生" | "调度" => {
            batch_spawn(context, args).await
        }
        "status" | "inspect" | "状态" => status(context, args).await,
        "wait" | "join" | "collect" | "等待" => wait(context, args).await,
        "interrupt" | "cancel" | "stop" | "中断" | "取消" => interrupt(context, args).await,
        "close" | "关闭" => close(context, args).await,
        "resume" | "reopen" | "恢复" => resume(context, args).await,
        _ => Err(anyhow!("unknown subagent_control action: {action}")),
    }
}

async fn list(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: super::SessionListArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let parent_session_id = resolve_subagent_parent_scope(payload.parent_id, context.session_id)?;
    let mut scoped_args = args.clone();
    if let Value::Object(ref mut map) = scoped_args {
        map.insert("parentId".to_string(), json!(parent_session_id));
    }
    super::sessions_list(context, &scoped_args).await
}

async fn history(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentHistoryArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let session_id = resolve_single_child_session_target(context, &payload.target, "history")?;
    let scoped_args = json!({
        "session_id": session_id,
        "limit": payload.limit,
        "includeTools": payload.include_tools.unwrap_or(false),
    });
    super::sessions_history(context, &scoped_args).await
}

async fn send(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentSendArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let session_id = resolve_single_child_session_target(context, &payload.target, "send")?;
    let scoped_args = json!({
        "session_id": session_id,
        "message": payload.message,
        "timeoutSeconds": payload.timeout_seconds,
        "announceParentSessionId": payload.announce_parent_session_id,
        "label": payload.label,
        "announcePersistHistory": payload.announce_persist_history,
        "announceEmitParentEvents": payload.announce_emit_parent_events,
    });
    super::sessions_send(context, &scoped_args).await
}

fn resolve_subagent_parent_scope(
    explicit_parent_id: Option<String>,
    current_session_id: &str,
) -> Result<String> {
    normalize_optional_string(explicit_parent_id)
        .or_else(|| normalize_optional_string(Some(current_session_id.to_string())))
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))
}

fn resolve_single_child_session_target(
    context: &ToolContext<'_>,
    target: &SubagentTargetArgs,
    action: &str,
) -> Result<String> {
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let current_session_id = context.session_id.trim();
    if current_session_id.is_empty() {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    }

    let mut resolved_session_ids = Vec::new();
    let mut requested_session_ids = target.session_ids.clone().unwrap_or_default();
    if let Some(session_id) = target.session_id.clone() {
        requested_session_ids.push(session_id);
    }
    let requested_session_ids = dedupe_non_empty_strings(requested_session_ids);
    let first_requested_session_id = requested_session_ids.first().cloned();
    for requested_session_id in requested_session_ids {
        match resolve_direct_child_session_id(
            context,
            user_id,
            current_session_id,
            &requested_session_id,
            action,
        )? {
            Some(session_id) => resolved_session_ids.push(session_id),
            None => {
                return Err(build_child_session_target_error(
                    action,
                    Some(&requested_session_id),
                ));
            }
        }
    }

    let should_resolve_selector = !target.run_ids.as_ref().is_none_or(Vec::is_empty)
        || target.run_id.as_ref().is_some()
        || normalize_optional_string(target.dispatch_id.clone()).is_some()
        || normalize_optional_string(target.parent_id.clone()).is_some();
    if should_resolve_selector {
        let selector = resolve_targets(target, None)?;
        for snapshot in collect_snapshots(context, &selector)? {
            if let Some(session_id) = snapshot
                .payload
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                let session_id = ensure_direct_child_session_id(
                    context,
                    user_id,
                    current_session_id,
                    &session_id,
                    action,
                )?;
                resolved_session_ids.push(session_id);
            }
        }
    }

    let resolved_session_ids = dedupe_non_empty_strings(resolved_session_ids);
    match resolved_session_ids.as_slice() {
        [session_id] => Ok(session_id.clone()),
        [] => {
            if let Some(session_id) =
                find_single_direct_child_session_id(context, user_id, current_session_id)?
            {
                return Ok(session_id);
            }
            Err(build_child_session_target_error(
                action,
                first_requested_session_id.as_deref(),
            ))
        }
        _ => Err(anyhow!(
            "subagent_control {action} requires exactly one child session target; use the exact session_id returned by spawn or a single runId"
        )),
    }
}

fn resolve_direct_child_session_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
    requested_session_id: &str,
    action: &str,
) -> Result<Option<String>> {
    let requested_session_id = requested_session_id.trim();
    if requested_session_id.is_empty() {
        return Ok(None);
    }
    if let Some(record) = context
        .storage
        .get_chat_session(user_id, requested_session_id)?
    {
        if !is_direct_child_session(record.parent_session_id.as_deref(), current_session_id) {
            return Err(anyhow!(
                "subagent_control {action} requires a direct child session of the current session"
            ));
        }
        return Ok(Some(record.session_id));
    }
    let similar =
        find_similar_child_session_id(context, user_id, current_session_id, requested_session_id)?;
    if similar.is_some() {
        return Ok(similar);
    }
    find_single_direct_child_session_id(context, user_id, current_session_id)
}

fn ensure_direct_child_session_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
    session_id: &str,
    action: &str,
) -> Result<String> {
    let Some(record) = context.storage.get_chat_session(user_id, session_id)? else {
        return Err(build_child_session_target_error(action, Some(session_id)));
    };
    if !is_direct_child_session(record.parent_session_id.as_deref(), current_session_id) {
        return Err(anyhow!(
            "subagent_control {action} requires a direct child session of the current session"
        ));
    }
    Ok(record.session_id)
}

fn build_child_session_target_error(
    action: &str,
    requested_session_id: Option<&str>,
) -> anyhow::Error {
    let mut message = format!(
        "subagent_control {action} target not found under the current session; use the exact session_id returned by spawn or a runId returned by spawn/list"
    );
    if let Some(requested_session_id) = requested_session_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        message.push_str(&format!(" (requested: {requested_session_id})"));
    }
    anyhow!(message)
}

fn find_single_direct_child_session_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
) -> Result<Option<String>> {
    let (sessions, _) = context.storage.list_chat_sessions(
        user_id,
        None,
        Some(current_session_id),
        0,
        MAX_SESSION_LIST_ITEMS,
    )?;
    Ok(select_single_direct_child_session_id(
        sessions.into_iter().map(|session| session.session_id).collect(),
    ))
}

fn select_single_direct_child_session_id(session_ids: Vec<String>) -> Option<String> {
    let session_ids = dedupe_non_empty_strings(session_ids);
    match session_ids.as_slice() {
        [session_id] => Some(session_id.clone()),
        _ => None,
    }
}

fn find_similar_child_session_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
    requested_session_id: &str,
) -> Result<Option<String>> {
    let requested_session_id = requested_session_id.trim();
    if requested_session_id.is_empty() {
        return Ok(None);
    }
    let (sessions, _) = context.storage.list_chat_sessions(
        user_id,
        None,
        Some(current_session_id),
        0,
        MAX_SESSION_LIST_ITEMS,
    )?;
    let mut best_distance: Option<usize> = None;
    let mut matches = Vec::new();
    for session in sessions {
        let candidate = session.session_id.trim();
        let Some(distance) = bounded_edit_distance(requested_session_id, candidate, 2) else {
            continue;
        };
        match best_distance {
            None => {
                best_distance = Some(distance);
                matches.clear();
                matches.push(session.session_id);
            }
            Some(current_best) if distance < current_best => {
                best_distance = Some(distance);
                matches.clear();
                matches.push(session.session_id);
            }
            Some(current_best) if distance == current_best => {
                matches.push(session.session_id);
            }
            Some(_) => {}
        }
    }
    Ok(if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    })
}

fn bounded_edit_distance(left: &str, right: &str, max_distance: usize) -> Option<usize> {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    let left_len = left_chars.len();
    let right_len = right_chars.len();
    if left_len.abs_diff(right_len) > max_distance {
        return None;
    }
    let mut previous = (0..=right_len).collect::<Vec<_>>();
    for (left_index, left_char) in left_chars.iter().enumerate() {
        let mut current = vec![left_index + 1; right_len + 1];
        let mut row_min = current[0];
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_char != right_char);
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            let substitution = previous[right_index] + substitution_cost;
            let value = insertion.min(deletion).min(substitution);
            current[right_index + 1] = value;
            row_min = row_min.min(value);
        }
        if row_min > max_distance {
            return None;
        }
        previous = current;
    }
    let distance = previous[right_len];
    (distance <= max_distance).then_some(distance)
}

fn is_direct_child_session(
    target_parent_session_id: Option<&str>,
    current_session_id: &str,
) -> bool {
    let current_session_id = current_session_id.trim();
    if current_session_id.is_empty() {
        return false;
    }
    target_parent_session_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value == current_session_id)
}

async fn batch_spawn(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentBatchSpawnArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let strategy = parse_batch_dispatch_strategy(payload.strategy.as_deref());
    let remaining_action = parse_remaining_branch_action(payload.remaining_action.as_deref())
        .unwrap_or_else(|| default_remaining_branch_action_for_strategy(strategy));
    let mut tasks = payload.tasks.clone();
    if tasks.is_empty() {
        if let Some(task) = normalize_optional_string(payload.task.clone()) {
            tasks.push(SubagentBatchTaskArgs {
                task,
                label: payload.label.clone(),
                agent_id: payload.agent_id.clone(),
                model: payload.model.clone(),
                run_timeout_seconds: payload.run_timeout_seconds,
                cleanup: payload.cleanup.clone(),
            });
        }
    }
    if tasks.is_empty() {
        return Err(anyhow!("batch_spawn requires at least one task"));
    }
    let parent_session_id = context.session_id.trim().to_string();
    if parent_session_id.is_empty() {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    }

    let dispatch_id = format!("dispatch_{}", Uuid::new_v4().simple());
    let dispatch_label = normalize_optional_string(payload.dispatch_label.clone())
        .or_else(|| normalize_optional_string(payload.label.clone()));
    let task_total = tasks.len() as i64;
    emit_dispatch_start(
        context,
        &dispatch_id,
        task_total,
        dispatch_label.as_deref(),
        strategy,
        remaining_action,
    );

    let mut startup_items = Vec::with_capacity(tasks.len());
    let mut startup_failed_items = Vec::new();
    let mut run_ids = Vec::new();
    for (index, task) in tasks.into_iter().enumerate() {
        let label = normalize_optional_string(task.label.clone())
            .or_else(|| normalize_optional_string(payload.label.clone()));
        let agent_id = normalize_optional_string(task.agent_id.clone())
            .or_else(|| normalize_optional_string(payload.agent_id.clone()));
        let model_name = normalize_optional_string(task.model.clone())
            .or_else(|| normalize_optional_string(payload.model.clone()));
        let cleanup_mode =
            super::parse_cleanup_mode(task.cleanup.as_deref().or(payload.cleanup.as_deref()));
        let run_timeout_s = task
            .run_timeout_seconds
            .or(payload.run_timeout_seconds)
            .filter(|value| *value > 0.0);

        let item = match super::prepare_child_session(
            context,
            &parent_session_id,
            &task.task,
            label.clone(),
            agent_id,
            model_name,
            super::ChildSessionToolMode::InheritParentSession,
        ) {
            Ok(prepared) => {
                let PreparedChildSession {
                    child_session_id,
                    child_agent_id,
                    model_name,
                    request,
                    mut announce,
                    mut run_metadata,
                } = prepared;
                let run_id = format!("run_{}", Uuid::new_v4().simple());
                announce.dispatch_id = Some(dispatch_id.clone());
                announce.strategy = Some(strategy.as_str().to_string());
                announce.completion_mode =
                    Some(completion_mode_from_strategy(strategy).as_str().to_string());
                announce.remaining_action = Some(remaining_action.as_str().to_string());
                announce.parent_user_round = context.user_round;
                announce.parent_model_round = context.model_round;
                announce.parent_turn_ref = crate::services::subagents::encode_parent_turn_ref(
                    context.user_round,
                    context.model_round,
                );
                announce.emit_parent_events = true;
                announce.auto_wake = true;
                announce.persist_history_message = false;
                super::insert_run_metadata_field(&mut run_metadata, "spawn_mode", json!("batch"));
                super::insert_run_metadata_field(
                    &mut run_metadata,
                    "dispatch_id",
                    json!(dispatch_id),
                );
                super::insert_run_metadata_field(&mut run_metadata, "dispatch_index", json!(index));
                super::insert_run_metadata_field(
                    &mut run_metadata,
                    "dispatch_size",
                    json!(task_total),
                );
                super::insert_run_metadata_field(
                    &mut run_metadata,
                    "dispatch_label",
                    dispatch_label
                        .clone()
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                );
                super::insert_run_metadata_field(
                    &mut run_metadata,
                    "strategy",
                    json!(strategy.as_str()),
                );
                super::insert_run_metadata_field(
                    &mut run_metadata,
                    "completion_mode",
                    json!(completion_mode_from_strategy(strategy).as_str()),
                );
                super::insert_run_metadata_field(
                    &mut run_metadata,
                    "remaining_action",
                    json!(remaining_action.as_str()),
                );
                super::insert_run_metadata_field(
                    &mut run_metadata,
                    "cleanup",
                    json!(super::session_cleanup_label(cleanup_mode)),
                );
                super::insert_run_metadata_field(
                    &mut run_metadata,
                    "run_timeout_seconds",
                    json!(run_timeout_s.unwrap_or(0.0)),
                );
                match super::spawn_session_run(
                    context,
                    request,
                    run_id.clone(),
                    Some(parent_session_id.clone()),
                    child_agent_id.clone(),
                    model_name.clone(),
                    SessionRunMeta {
                        dispatch_id: Some(dispatch_id.clone()),
                        run_kind: Some("subagent".to_string()),
                        requested_by: Some("subagent_control".to_string()),
                        metadata: Some(run_metadata),
                    },
                    Some(announce),
                    cleanup_mode,
                    run_timeout_s,
                )
                .await
                {
                    Ok(_) => {
                        run_ids.push(run_id.clone());
                        json!({
                            "dispatch_id": dispatch_id.clone(),
                            "index": index,
                            "status": "accepted",
                            "terminal": false,
                            "failed": false,
                            "run_id": run_id,
                            "session_id": child_session_id,
                            "parent_session_id": parent_session_id.clone(),
                            "label": label,
                            "task": task.task,
                            "agent_id": child_agent_id,
                            "model_name": model_name,
                        })
                    }
                    Err(err) => {
                        super::cleanup_session(
                            &context.storage,
                            &context.workspace,
                            context.monitor.as_ref(),
                            context.user_id,
                            &child_session_id,
                        );
                        json!({
                            "dispatch_id": dispatch_id.clone(),
                            "index": index,
                            "status": "error",
                            "terminal": true,
                            "failed": true,
                            "session_id": child_session_id,
                            "parent_session_id": parent_session_id.clone(),
                            "label": label,
                            "task": task.task,
                            "error": err.to_string(),
                        })
                    }
                }
            }
            Err(err) => json!({
                "dispatch_id": dispatch_id.clone(),
                "index": index,
                "status": "error",
                "terminal": true,
                "failed": true,
                "parent_session_id": parent_session_id.clone(),
                "label": label,
                "task": task.task,
                "error": err.to_string(),
            }),
        };
        if item.get("failed").and_then(Value::as_bool).unwrap_or(false) {
            startup_failed_items.push(item.clone());
        }
        emit_control_event(context, "subagent_dispatch_item_update", &item);
        startup_items.push(item);
    }

    if run_ids.is_empty() {
        let result = decorate_dispatch_result(
            json!({
            "status": "error",
            "dispatch_id": dispatch_id.clone(),
            "requested_total": startup_items.len(),
            "accepted_total": 0,
            "startup_failed_total": startup_failed_items.len(),
            "items": startup_items,
            }),
            strategy,
            dispatch_label.as_deref(),
            remaining_action,
        );
        emit_control_event(context, "subagent_dispatch_finish", &result);
        return Ok(result);
    }

    let wait_seconds = payload.wait_seconds.unwrap_or(0.0).max(0.0);
    if wait_seconds <= 0.0 {
        let result = decorate_dispatch_result(
            json!({
            "status": if startup_failed_items.is_empty() { "accepted" } else { "partial" },
            "dispatch_id": dispatch_id.clone(),
            "requested_total": startup_items.len(),
            "accepted_total": run_ids.len(),
            "startup_failed_total": startup_failed_items.len(),
            "run_ids": run_ids,
            "items": startup_items,
            }),
            strategy,
            dispatch_label.as_deref(),
            remaining_action,
        );
        emit_control_event(context, "subagent_dispatch_finish", &result);
        return Ok(result);
    }

    let wait_result = wait_for_targets(
        context,
        ResolvedTargetSet {
            run_ids: run_ids.clone(),
            session_ids: Vec::new(),
            dispatch_id: Some(dispatch_id.clone()),
            parent_id: None,
            limit: clamp_limit(
                Some(startup_items.len() as i64),
                startup_items.len() as i64,
                MAX_SESSION_LIST_ITEMS,
            ),
        },
        wait_seconds,
        payload
            .poll_interval_seconds
            .unwrap_or(SUBAGENT_WAIT_DEFAULT_POLL_S),
        completion_mode_from_strategy(strategy),
        true,
    )
    .await?;
    let mut merged = merge_wait_result(
        wait_result,
        &dispatch_id,
        startup_items.len() as i64,
        run_ids.len() as i64,
        startup_failed_items,
    );
    apply_remaining_settlement(context, &mut merged, remaining_action);
    let merged = decorate_dispatch_result(
        merged,
        strategy,
        dispatch_label.as_deref(),
        remaining_action,
    );
    emit_control_event(context, "subagent_dispatch_finish", &merged);
    Ok(merged)
}

async fn status(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentStatusArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let selector = resolve_targets(&payload.target, Some(context.session_id))?;
    let snapshots = collect_snapshots(context, &selector)?;
    let summary = summarize_snapshots(
        &selector,
        snapshots.clone(),
        0.0,
        0.0,
        WaitCompletionMode::All,
        evaluate_wait_progress(WaitCompletionMode::All, &snapshots),
        false,
    );
    emit_control_event(context, "subagent_status", &summary);
    Ok(wrap_missing_target_summary(summary, "status"))
}

async fn wait(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentWaitArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let selector = resolve_targets(&payload.target, None)?;
    let wait_mode = parse_wait_completion_mode(payload.wait_mode.as_deref());
    let remaining_action = parse_remaining_branch_action(payload.remaining_action.as_deref())
        .unwrap_or(RemainingBranchAction::Keep);
    let mut result = wait_for_targets(
        context,
        selector,
        payload.wait_seconds.unwrap_or(0.0).max(0.0),
        payload
            .poll_interval_seconds
            .unwrap_or(SUBAGENT_WAIT_DEFAULT_POLL_S),
        wait_mode,
        true,
    )
    .await?;
    apply_remaining_settlement(context, &mut result, remaining_action);
    if result.get("dispatch_id").and_then(Value::as_str).is_some() {
        emit_control_event(context, "subagent_dispatch_finish", &result);
    } else {
        emit_control_event(context, "subagent_status", &result);
    }
    Ok(wrap_missing_target_summary(result, "wait"))
}

fn wrap_missing_target_summary(summary: Value, action: &str) -> Value {
    if !selected_items_all_not_found(&summary) {
        return summary;
    }
    super::build_failed_tool_result(
        format!(
            "subagent_control {action} target not found under the current session"
        ),
        summary,
        super::ToolErrorMeta::new(
            "SUBAGENT_TARGET_NOT_FOUND",
            Some(
                "Use the exact `session_id`/`child_session_id` returned by `spawn`, or pass a `runId` returned by `spawn`/`list`.".to_string(),
            ),
            false,
            None,
        ),
        false,
    )
}

fn selected_items_all_not_found(summary: &Value) -> bool {
    let Some(items) = summary.get("selected_items").and_then(Value::as_array) else {
        return false;
    };
    !items.is_empty()
        && items.iter().all(|item| {
            item.get("status").and_then(Value::as_str) == Some("not_found")
                || item.get("error").and_then(Value::as_str) == Some("session not found")
                || item.get("error").and_then(Value::as_str) == Some("run not found")
        })
}

async fn interrupt(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentInterruptArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let selector = resolve_targets(&payload.target, None)?;
    let session_ids = collect_target_session_ids(context, &selector, false)?;
    let monitor = context
        .monitor
        .as_ref()
        .ok_or_else(|| anyhow!("monitor unavailable"))?;
    let mut updated_total = 0_i64;
    let mut items = Vec::with_capacity(session_ids.len());
    for session_id in session_ids {
        let updated = monitor.cancel(&session_id);
        if updated {
            updated_total += 1;
        }
        let item = json!({
            "session_id": session_id,
            "status": if updated { "cancelling" } else { "unchanged" },
            "updated": updated,
        });
        emit_control_event(context, "subagent_interrupt", &item);
        items.push(item);
    }
    Ok(
        json!({ "status": if updated_total > 0 { "ok" } else { "noop" }, "updated_total": updated_total, "items": items }),
    )
}

async fn close(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    session_control(context, args, "closed", true, "subagent_close").await
}

async fn resume(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    session_control(context, args, "active", false, "subagent_resume").await
}

async fn session_control(
    context: &ToolContext<'_>,
    args: &Value,
    next_status: &str,
    cancel_running: bool,
    event_type: &str,
) -> Result<Value> {
    let payload: SubagentSessionControlArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let selector = resolve_targets(&payload.target, None)?;
    let session_ids =
        collect_target_session_ids(context, &selector, payload.cascade.unwrap_or(false))?;
    let mut updated_total = 0_i64;
    let mut items = Vec::with_capacity(session_ids.len());
    for session_id in session_ids {
        let item = update_session_status(context, &session_id, next_status, cancel_running)?;
        if item
            .get("updated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            updated_total += 1;
        }
        emit_control_event(context, event_type, &item);
        items.push(item);
    }
    Ok(
        json!({ "status": if updated_total > 0 { "ok" } else { "noop" }, "updated_total": updated_total, "items": items }),
    )
}

fn resolve_targets(
    payload: &SubagentTargetArgs,
    default_parent_session_id: Option<&str>,
) -> Result<ResolvedTargetSet> {
    let mut run_ids = payload.run_ids.clone().unwrap_or_default();
    if let Some(run_id) = payload.run_id.clone() {
        run_ids.push(run_id);
    }
    let mut session_ids = payload.session_ids.clone().unwrap_or_default();
    if let Some(session_id) = payload.session_id.clone() {
        session_ids.push(session_id);
    }
    let dispatch_id = normalize_optional_string(payload.dispatch_id.clone());
    let run_ids = dedupe_non_empty_strings(run_ids);
    let session_ids = dedupe_non_empty_strings(session_ids);
    let mut parent_id = normalize_optional_string(payload.parent_id.clone());
    if run_ids.is_empty() && session_ids.is_empty() && dispatch_id.is_none() && parent_id.is_none()
    {
        parent_id = default_parent_session_id
            .and_then(|value| normalize_optional_string(Some(value.to_string())));
    }
    if run_ids.is_empty() && session_ids.is_empty() && dispatch_id.is_none() && parent_id.is_none()
    {
        return Err(anyhow!("subagent target is required"));
    }
    Ok(ResolvedTargetSet {
        run_ids,
        session_ids,
        dispatch_id,
        parent_id,
        limit: clamp_limit(payload.limit, 50, MAX_SESSION_LIST_ITEMS),
    })
}

fn collect_snapshots(
    context: &ToolContext<'_>,
    selector: &ResolvedTargetSet,
) -> Result<Vec<SubagentRunSnapshot>> {
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let mut run_ids = selector.run_ids.clone();
    if let Some(dispatch_id) = selector.dispatch_id.as_deref() {
        let records =
            context
                .storage
                .list_session_runs_by_dispatch(user_id, dispatch_id, selector.limit)?;
        run_ids.extend(records.into_iter().map(|record| record.run_id));
    }
    if let Some(parent_id) = selector.parent_id.as_deref() {
        let records =
            context
                .storage
                .list_session_runs_by_parent(user_id, parent_id, selector.limit)?;
        let mut seen_sessions = HashSet::new();
        for record in records {
            if seen_sessions.insert(record.session_id.clone()) {
                run_ids.push(record.run_id);
            }
        }
    }
    let run_ids = dedupe_non_empty_strings(run_ids);
    let mut snapshots = Vec::new();
    let mut seen_keys = HashSet::new();
    let mut seen_session_ids = HashSet::new();
    for run_id in run_ids {
        let snapshot = build_run_snapshot(context, &run_id)?;
        if let Some(session_id) = snapshot
            .payload
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            seen_session_ids.insert(session_id);
        }
        if seen_keys.insert(snapshot.key.clone()) {
            snapshots.push(snapshot);
        }
    }
    for session_id in &selector.session_ids {
        if seen_session_ids.contains(session_id) {
            continue;
        }
        let snapshot = build_session_snapshot(context, session_id)?;
        if seen_keys.insert(snapshot.key.clone()) {
            snapshots.push(snapshot);
        }
    }
    snapshots.sort_by(|left, right| {
        right
            .updated_time
            .partial_cmp(&left.updated_time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(snapshots)
}

fn build_run_snapshot(context: &ToolContext<'_>, run_id: &str) -> Result<SubagentRunSnapshot> {
    let user_id = context.user_id.trim();
    if let Some(record) = context.storage.get_session_run(run_id)? {
        let session = context
            .storage
            .get_chat_session(user_id, &record.session_id)
            .ok()
            .flatten();
        let runtime_status = runtime_status(context, &record.session_id);
        let session_status = session
            .as_ref()
            .map(|entry| normalize_session_status(&entry.status))
            .unwrap_or_else(|| "missing".to_string());
        let status =
            resolve_effective_status(&record.status, runtime_status.as_deref(), &session_status);
        let terminal = is_terminal_status(&status);
        let failed = is_failed_status(&status);
        let message =
            run_message_for_status(&status, record.result.as_deref(), record.error.as_deref());
        let metadata = record.metadata.clone();
        let (parent_turn_ref, parent_user_round, parent_model_round) =
            crate::services::subagents::parent_turn_payload(
                metadata.as_ref(),
                session
                    .as_ref()
                    .and_then(|entry| entry.parent_message_id.as_deref()),
            );
        let mut payload = json!({
            "run_id": record.run_id,
            "dispatch_id": record.dispatch_id,
            "run_kind": record.run_kind,
            "requested_by": record.requested_by,
            "status": status,
            "run_status": record.status,
            "runtime_status": runtime_status,
            "session_status": session_status,
            "terminal": terminal,
            "failed": failed,
            "session_id": record.session_id,
            "parent_session_id": record.parent_session_id,
            "agent_id": record.agent_id,
            "model_name": record.model_name,
            "queued_time": record.queued_time,
            "started_time": record.started_time,
            "finished_time": record.finished_time,
            "elapsed_s": record.elapsed_s,
            "result": record.result,
            "error": record.error,
            "agent_state": {
                "status": collab_agent_status(&status),
                "message": message,
            },
            "updated_time": record.updated_time,
            "title": session.as_ref().map(|entry| entry.title.clone()),
            "spawn_label": session.as_ref().and_then(|entry| entry.spawn_label.clone()),
            "spawned_by": session.as_ref().and_then(|entry| entry.spawned_by.clone()),
        });
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "metadata".to_string(),
                metadata.clone().unwrap_or(Value::Null),
            );
            object.insert(
                "controller_session_id".to_string(),
                crate::services::subagents::run_metadata_field(
                    metadata.as_ref(),
                    "controller_session_id",
                ),
            );
            object.insert(
                "depth".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "depth"),
            );
            object.insert(
                "role".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "role"),
            );
            object.insert(
                "control_scope".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "control_scope"),
            );
            object.insert(
                "spawn_mode".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "spawn_mode"),
            );
            object.insert(
                "strategy".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "strategy"),
            );
            object.insert(
                "completion_mode".to_string(),
                crate::services::subagents::run_metadata_field(
                    metadata.as_ref(),
                    "completion_mode",
                ),
            );
            object.insert(
                "remaining_action".to_string(),
                crate::services::subagents::run_metadata_field(
                    metadata.as_ref(),
                    "remaining_action",
                ),
            );
            object.insert(
                "dispatch_label".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "dispatch_label"),
            );
            object.insert(
                "dispatch_index".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "dispatch_index"),
            );
            object.insert(
                "dispatch_size".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "dispatch_size"),
            );
            object.insert(
                "cleanup".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "cleanup"),
            );
            object.insert(
                "run_timeout_seconds".to_string(),
                crate::services::subagents::run_metadata_field(
                    metadata.as_ref(),
                    "run_timeout_seconds",
                ),
            );
            object.insert("parent_turn_ref".to_string(), parent_turn_ref);
            object.insert("parent_user_round".to_string(), parent_user_round);
            object.insert("parent_model_round".to_string(), parent_model_round);
        }
        Ok(SubagentRunSnapshot {
            key: record.run_id.clone(),
            status: status.clone(),
            terminal,
            failed,
            updated_time: record.updated_time,
            payload,
        })
    } else {
        Ok(SubagentRunSnapshot {
            key: run_id.trim().to_string(),
            status: "not_found".to_string(),
            terminal: true,
            failed: true,
            updated_time: 0.0,
            payload: json!({
                "run_id": run_id,
                "status": "not_found",
                "terminal": true,
                "failed": true,
                "agent_state": {
                    "status": "not_found",
                    "message": "run not found",
                },
                "error": "run not found",
            }),
        })
    }
}

fn build_session_snapshot(
    context: &ToolContext<'_>,
    session_id: &str,
) -> Result<SubagentRunSnapshot> {
    let user_id = context.user_id.trim();
    let Some(session) = context.storage.get_chat_session(user_id, session_id)? else {
        return Ok(SubagentRunSnapshot {
            key: session_id.trim().to_string(),
            status: "not_found".to_string(),
            terminal: true,
            failed: true,
            updated_time: 0.0,
            payload: json!({
                "session_id": session_id,
                "status": "not_found",
                "terminal": true,
                "failed": true,
                "error": "session not found",
            }),
        });
    };
    if let Some(record) = context
        .storage
        .list_session_runs_by_session(user_id, &session.session_id, 1)?
        .into_iter()
        .next()
    {
        return build_run_snapshot(context, &record.run_id);
    }
    let session_status = normalize_session_status(&session.status);
    let runtime_status = runtime_status(context, &session.session_id);
    let status = resolve_effective_status("", runtime_status.as_deref(), &session_status);
    let terminal = is_terminal_status(&status);
    let failed = is_failed_status(&status);
    let (parent_turn_ref, parent_user_round, parent_model_round) =
        crate::services::subagents::parent_turn_payload(None, session.parent_message_id.as_deref());
    let mut payload = json!({
        "status": status,
        "runtime_status": runtime_status,
        "session_status": session_status,
        "terminal": terminal,
        "failed": failed,
        "agent_state": {
            "status": collab_agent_status(&status),
            "message": serde_json::Value::Null,
        },
        "session_id": session.session_id,
        "parent_session_id": session.parent_session_id,
        "agent_id": session.agent_id,
        "title": session.title,
        "spawn_label": session.spawn_label,
        "spawned_by": session.spawned_by,
        "updated_time": session.updated_at,
    });
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "controller_session_id".to_string(),
            session
                .parent_session_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        object.insert("metadata".to_string(), Value::Null);
        object.insert("depth".to_string(), Value::Null);
        object.insert("role".to_string(), Value::Null);
        object.insert("control_scope".to_string(), Value::Null);
        object.insert("spawn_mode".to_string(), Value::Null);
        object.insert("strategy".to_string(), Value::Null);
        object.insert("completion_mode".to_string(), Value::Null);
        object.insert("remaining_action".to_string(), Value::Null);
        object.insert("dispatch_label".to_string(), Value::Null);
        object.insert("dispatch_index".to_string(), Value::Null);
        object.insert("dispatch_size".to_string(), Value::Null);
        object.insert("cleanup".to_string(), Value::Null);
        object.insert("run_timeout_seconds".to_string(), Value::Null);
        object.insert("parent_turn_ref".to_string(), parent_turn_ref);
        object.insert("parent_user_round".to_string(), parent_user_round);
        object.insert("parent_model_round".to_string(), parent_model_round);
    }
    Ok(SubagentRunSnapshot {
        key: session.session_id.clone(),
        status: status.clone(),
        terminal,
        failed,
        updated_time: session.updated_at,
        payload,
    })
}

async fn wait_for_targets(
    context: &ToolContext<'_>,
    selector: ResolvedTargetSet,
    wait_seconds: f64,
    poll_interval_seconds: f64,
    completion_mode: WaitCompletionMode,
    emit_progress: bool,
) -> Result<Value> {
    let poll_interval = normalize_poll_interval(poll_interval_seconds);
    let started_at = Instant::now();
    let mut status_index = HashMap::new();
    loop {
        let snapshots = collect_snapshots(context, &selector)?;
        if snapshots.is_empty() {
            return Ok(summarize_snapshots(
                &selector,
                snapshots,
                wait_seconds,
                0.0,
                completion_mode,
                WaitProgressState {
                    completion_reached: true,
                    all_finished: true,
                    matched_total: 0,
                    matched_success_total: 0,
                    matched_failed_total: 0,
                    completed_reason: "empty",
                },
                false,
            ));
        }
        let elapsed_s = started_at.elapsed().as_secs_f64();
        let progress_state = evaluate_wait_progress(completion_mode, &snapshots);
        let timed_out =
            wait_seconds > 0.0 && elapsed_s >= wait_seconds && !progress_state.completion_reached;
        emit_wait_updates(context, &selector, &snapshots, &mut status_index);
        if emit_progress {
            emit_wait_progress(context, &selector, &snapshots, elapsed_s);
        }
        if progress_state.completion_reached || timed_out || wait_seconds <= 0.0 {
            return Ok(summarize_snapshots(
                &selector,
                snapshots,
                wait_seconds,
                elapsed_s,
                completion_mode,
                progress_state,
                timed_out,
            ));
        }
        sleep(tokio::time::Duration::from_secs_f64(poll_interval)).await;
    }
}

fn summarize_snapshots(
    selector: &ResolvedTargetSet,
    snapshots: Vec<SubagentRunSnapshot>,
    wait_seconds: f64,
    elapsed_s: f64,
    completion_mode: WaitCompletionMode,
    progress_state: WaitProgressState,
    timed_out: bool,
) -> Value {
    let total = snapshots.len() as i64;
    let done_total = snapshots.iter().filter(|item| item.terminal).count() as i64;
    let success_total = snapshots
        .iter()
        .filter(|item| item.status == "success")
        .count() as i64;
    let failed_total = snapshots.iter().filter(|item| item.failed).count() as i64;
    let queued_total = snapshots
        .iter()
        .filter(|item| item.status == "queued")
        .count() as i64;
    let running_total = snapshots
        .iter()
        .filter(|item| matches!(item.status.as_str(), "running" | "waiting" | "cancelling"))
        .count() as i64;
    let selected_items = collect_selected_items(completion_mode, &snapshots);
    let status = summarize_wait_status(
        total,
        failed_total,
        timed_out,
        completion_mode,
        progress_state,
    );
    json!({
        "status": status,
        "dispatch_id": selector.dispatch_id.clone(),
        "parent_id": selector.parent_id.clone(),
        "completion_mode": completion_mode.as_str(),
        "completion_reached": progress_state.completion_reached,
        "completed_reason": progress_state.completed_reason,
        "wait_seconds": wait_seconds,
        "elapsed_s": elapsed_s,
        "all_finished": progress_state.all_finished,
        "total": total,
        "done_total": done_total,
        "success_total": success_total,
        "failed_total": failed_total,
        "queued_total": queued_total,
        "running_total": running_total,
        "selected_total": progress_state.matched_total,
        "selected_success_total": progress_state.matched_success_total,
        "selected_failed_total": progress_state.matched_failed_total,
        "run_ids": selector.run_ids.clone(),
        "session_ids": selector.session_ids.clone(),
        "selected_items": selected_items,
        "items": snapshots.into_iter().map(|item| item.payload).collect::<Vec<_>>(),
    })
}

fn merge_wait_result(
    wait_result: Value,
    dispatch_id: &str,
    requested_total: i64,
    accepted_total: i64,
    startup_failed_items: Vec<Value>,
) -> Value {
    let mut result = wait_result;
    let Some(object) = result.as_object_mut() else {
        return result;
    };
    let total = object.get("total").and_then(Value::as_i64).unwrap_or(0);
    let current_status = object
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("running")
        .to_string();
    let failed_total = object
        .get("failed_total")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if let Some(items) = object.get_mut("items").and_then(Value::as_array_mut) {
        items.extend(startup_failed_items.clone());
    }
    let merged_failed_total = failed_total + startup_failed_items.len() as i64;
    object.insert("dispatch_id".to_string(), json!(dispatch_id));
    object.insert("requested_total".to_string(), json!(requested_total));
    object.insert("accepted_total".to_string(), json!(accepted_total));
    object.insert(
        "startup_failed_total".to_string(),
        json!(startup_failed_items.len() as i64),
    );
    object.insert(
        "total".to_string(),
        json!(total + startup_failed_items.len() as i64),
    );
    object.insert("failed_total".to_string(), json!(merged_failed_total));
    if current_status == "ok" && !startup_failed_items.is_empty() {
        object.insert("status".to_string(), json!("partial"));
    }
    result
}

fn collect_target_session_ids(
    context: &ToolContext<'_>,
    selector: &ResolvedTargetSet,
    cascade: bool,
) -> Result<Vec<String>> {
    let mut session_ids = selector.session_ids.clone();
    for snapshot in collect_snapshots(context, selector)? {
        if let Some(session_id) = snapshot
            .payload
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            session_ids.push(session_id);
        }
    }
    let mut session_ids = dedupe_non_empty_strings(session_ids);
    if cascade {
        let descendants = collect_descendant_session_ids(context, &session_ids, selector.limit)?;
        session_ids.extend(descendants);
        session_ids = dedupe_non_empty_strings(session_ids);
    }
    Ok(session_ids)
}

fn collect_descendant_session_ids(
    context: &ToolContext<'_>,
    root_session_ids: &[String],
    limit: i64,
) -> Result<Vec<String>> {
    let user_id = context.user_id.trim();
    let mut queue: VecDeque<String> = root_session_ids.iter().cloned().collect();
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    while let Some(parent_session_id) = queue.pop_front() {
        let (children, _) = context.storage.list_chat_sessions_by_status(
            user_id,
            None,
            Some(&parent_session_id),
            Some("all"),
            0,
            limit,
        )?;
        for child in children {
            if seen.insert(child.session_id.clone()) {
                queue.push_back(child.session_id.clone());
                output.push(child.session_id);
                if output.len() >= limit as usize {
                    return Ok(output);
                }
            }
        }
    }
    Ok(output)
}

fn update_session_status(
    context: &ToolContext<'_>,
    session_id: &str,
    next_status: &str,
    cancel_running: bool,
) -> Result<Value> {
    let user_id = context.user_id.trim();
    let Some(mut record) = context.storage.get_chat_session(user_id, session_id)? else {
        return Ok(json!({ "session_id": session_id, "status": "not_found", "updated": false }));
    };
    let updated = record.status.trim() != next_status;
    if cancel_running {
        if let Some(monitor) = context.monitor.as_ref() {
            let _ = monitor.cancel(session_id);
        }
    }
    if updated {
        record.status = next_status.to_string();
        record.updated_at = now_ts();
        context.storage.upsert_chat_session(&record)?;
    }
    Ok(json!({
        "session_id": session_id,
        "status": next_status,
        "updated": updated,
        "title": record.title,
        "parent_session_id": record.parent_session_id,
    }))
}

fn emit_dispatch_start(
    context: &ToolContext<'_>,
    dispatch_id: &str,
    total: i64,
    label: Option<&str>,
    strategy: BatchDispatchStrategy,
    remaining_action: RemainingBranchAction,
) {
    emit_control_event(
        context,
        "subagent_dispatch_start",
        &json!({
            "dispatch_id": dispatch_id,
            "parent_session_id": context.session_id,
            "total": total,
            "label": label,
            "strategy": strategy.as_str(),
            "completion_mode": completion_mode_from_strategy(strategy).as_str(),
            "remaining_action": remaining_action.as_str(),
        }),
    );
}

fn emit_control_event(context: &ToolContext<'_>, event_type: &str, payload: &Value) {
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(event_type, payload.clone());
    }
}

fn emit_wait_progress(
    context: &ToolContext<'_>,
    selector: &ResolvedTargetSet,
    snapshots: &[SubagentRunSnapshot],
    elapsed_s: f64,
) {
    emit_control_event(
        context,
        "progress",
        &json!({
            "stage": "subagent_wait",
            "summary": i18n::t("monitor.summary.subagent_wait"),
            "dispatch_id": selector.dispatch_id.clone(),
            "parent_id": selector.parent_id.clone(),
            "total": snapshots.len(),
            "done_total": snapshots.iter().filter(|item| item.terminal).count(),
            "failed_total": snapshots.iter().filter(|item| item.failed).count(),
            "elapsed_s": elapsed_s,
        }),
    );
}

fn emit_wait_updates(
    context: &ToolContext<'_>,
    selector: &ResolvedTargetSet,
    snapshots: &[SubagentRunSnapshot],
    status_index: &mut HashMap<String, String>,
) {
    for snapshot in snapshots {
        if status_index.get(&snapshot.key) == Some(&snapshot.status) {
            continue;
        }
        status_index.insert(snapshot.key.clone(), snapshot.status.clone());
        let mut payload = snapshot.payload.clone();
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "dispatch_id".to_string(),
                json!(selector.dispatch_id.clone()),
            );
        }
        emit_control_event(context, "subagent_dispatch_item_update", &payload);
    }
}

fn runtime_status(context: &ToolContext<'_>, session_id: &str) -> Option<String> {
    context
        .monitor
        .as_ref()
        .and_then(|monitor| monitor.get_record(session_id))
        .and_then(|entry| {
            entry
                .get("status")
                .and_then(Value::as_str)
                .map(|value| value.trim().to_ascii_lowercase())
        })
        .filter(|value| !value.is_empty())
}

fn normalize_session_status(status: &str) -> String {
    let normalized = status.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        "active".to_string()
    } else {
        normalized
    }
}

fn resolve_effective_status(
    run_status: &str,
    runtime_status: Option<&str>,
    session_status: &str,
) -> String {
    let run_status = run_status.trim().to_ascii_lowercase();
    if is_terminal_status(&run_status) {
        return run_status;
    }
    if session_status == "closed" {
        return "closed".to_string();
    }
    if let Some(runtime_status) = runtime_status {
        let runtime_status = runtime_status.trim().to_ascii_lowercase();
        if !runtime_status.is_empty() {
            return runtime_status;
        }
    }
    if run_status.is_empty() {
        if session_status == "active" {
            "idle".to_string()
        } else {
            session_status.to_string()
        }
    } else {
        run_status
    }
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "success" | "error" | "timeout" | "cancelled" | "failed" | "closed" | "idle" | "not_found"
    )
}

fn is_failed_status(status: &str) -> bool {
    matches!(
        status,
        "error" | "timeout" | "cancelled" | "failed" | "closed" | "not_found"
    )
}

fn normalize_poll_interval(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return SUBAGENT_WAIT_DEFAULT_POLL_S;
    }
    value.clamp(SUBAGENT_WAIT_MIN_POLL_S, SUBAGENT_WAIT_MAX_POLL_S)
}

fn parse_wait_completion_mode(value: Option<&str>) -> WaitCompletionMode {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "any" | "one" | "first" | "first_terminal" => WaitCompletionMode::Any,
        "first_success" | "success" => WaitCompletionMode::FirstSuccess,
        _ => WaitCompletionMode::All,
    }
}

fn parse_batch_dispatch_strategy(value: Option<&str>) -> BatchDispatchStrategy {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "first_success" | "success" => BatchDispatchStrategy::FirstSuccess,
        "review_then_merge" | "merge" | "collect" => BatchDispatchStrategy::ReviewThenMerge,
        _ => BatchDispatchStrategy::ParallelAll,
    }
}

fn parse_remaining_branch_action(value: Option<&str>) -> Option<RemainingBranchAction> {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "" => None,
        "keep" | "none" => Some(RemainingBranchAction::Keep),
        "interrupt" | "cancel" | "stop" => Some(RemainingBranchAction::Interrupt),
        "close" | "shutdown" => Some(RemainingBranchAction::Close),
        _ => None,
    }
}

fn default_remaining_branch_action_for_strategy(
    strategy: BatchDispatchStrategy,
) -> RemainingBranchAction {
    match strategy {
        BatchDispatchStrategy::ParallelAll | BatchDispatchStrategy::ReviewThenMerge => {
            RemainingBranchAction::Keep
        }
        BatchDispatchStrategy::FirstSuccess => RemainingBranchAction::Interrupt,
    }
}

fn completion_mode_from_strategy(strategy: BatchDispatchStrategy) -> WaitCompletionMode {
    match strategy {
        BatchDispatchStrategy::ParallelAll | BatchDispatchStrategy::ReviewThenMerge => {
            WaitCompletionMode::All
        }
        BatchDispatchStrategy::FirstSuccess => WaitCompletionMode::FirstSuccess,
    }
}

fn evaluate_wait_progress(
    completion_mode: WaitCompletionMode,
    snapshots: &[SubagentRunSnapshot],
) -> WaitProgressState {
    let all_finished = snapshots.iter().all(|item| item.terminal);
    let terminal_total = snapshots.iter().filter(|item| item.terminal).count() as i64;
    let success_total = snapshots
        .iter()
        .filter(|item| item.status == "success")
        .count() as i64;
    let failed_total = snapshots
        .iter()
        .filter(|item| item.terminal && item.failed)
        .count() as i64;
    match completion_mode {
        WaitCompletionMode::All => WaitProgressState {
            completion_reached: all_finished,
            all_finished,
            matched_total: terminal_total,
            matched_success_total: success_total,
            matched_failed_total: failed_total,
            completed_reason: if all_finished {
                "all_finished"
            } else {
                "pending"
            },
        },
        WaitCompletionMode::Any => WaitProgressState {
            completion_reached: terminal_total > 0,
            all_finished,
            matched_total: terminal_total,
            matched_success_total: success_total,
            matched_failed_total: failed_total,
            completed_reason: if terminal_total > 0 {
                "first_terminal"
            } else {
                "pending"
            },
        },
        WaitCompletionMode::FirstSuccess => WaitProgressState {
            completion_reached: success_total > 0 || all_finished,
            all_finished,
            matched_total: if success_total > 0 {
                success_total
            } else {
                terminal_total
            },
            matched_success_total: success_total,
            matched_failed_total: if success_total > 0 { 0 } else { failed_total },
            completed_reason: if success_total > 0 {
                "first_success"
            } else if all_finished {
                "all_finished_without_success"
            } else {
                "pending"
            },
        },
    }
}

fn collect_selected_items(
    completion_mode: WaitCompletionMode,
    snapshots: &[SubagentRunSnapshot],
) -> Vec<Value> {
    match completion_mode {
        WaitCompletionMode::All | WaitCompletionMode::Any => snapshots
            .iter()
            .filter(|item| item.terminal)
            .map(|item| item.payload.clone())
            .collect(),
        WaitCompletionMode::FirstSuccess => {
            let selected = snapshots
                .iter()
                .filter(|item| item.status == "success")
                .map(|item| item.payload.clone())
                .collect::<Vec<_>>();
            if selected.is_empty() {
                snapshots
                    .iter()
                    .filter(|item| item.terminal)
                    .map(|item| item.payload.clone())
                    .collect()
            } else {
                selected
            }
        }
    }
}

fn summarize_wait_status(
    total: i64,
    failed_total: i64,
    timed_out: bool,
    completion_mode: WaitCompletionMode,
    progress_state: WaitProgressState,
) -> &'static str {
    if total == 0 {
        return "empty";
    }
    if timed_out {
        return "timeout";
    }
    match completion_mode {
        WaitCompletionMode::All => {
            if progress_state.all_finished {
                if failed_total == 0 {
                    "ok"
                } else {
                    "partial"
                }
            } else {
                "running"
            }
        }
        WaitCompletionMode::Any => {
            if !progress_state.completion_reached {
                "running"
            } else if progress_state.matched_success_total > 0
                && progress_state.matched_failed_total == 0
            {
                "ok"
            } else {
                "partial"
            }
        }
        WaitCompletionMode::FirstSuccess => {
            if progress_state.matched_success_total > 0 {
                "ok"
            } else if progress_state.all_finished {
                "partial"
            } else {
                "running"
            }
        }
    }
}

fn collab_agent_status(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "queued" | "accepted" | "active" => "pending_init",
        "running" | "waiting" => "running",
        "cancelling" | "cancelled" => "interrupted",
        "success" | "idle" => "completed",
        "error" | "timeout" | "failed" => "errored",
        "closed" => "shutdown",
        "not_found" => "not_found",
        _ => "running",
    }
}

fn run_message_for_status(
    status: &str,
    result: Option<&str>,
    error: Option<&str>,
) -> Option<String> {
    let source = if status == "success" {
        result
    } else if is_failed_status(status) {
        error
    } else {
        None
    }?;
    let text = source.trim();
    if text.is_empty() {
        None
    } else {
        Some(truncate_text(text, SUBAGENT_SUMMARY_MAX_CHARS))
    }
}

fn apply_remaining_settlement(
    context: &ToolContext<'_>,
    result: &mut Value,
    action: RemainingBranchAction,
) {
    let Some(object) = result.as_object_mut() else {
        return;
    };
    object.insert("remaining_action".to_string(), json!(action.as_str()));
    let completed_reason = object
        .get("completed_reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let items = object
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pending_items = collect_pending_settlement_items(completed_reason, &items);
    object.insert(
        "remaining_active_total".to_string(),
        json!(pending_items.len() as i64),
    );
    if pending_items.is_empty() || action == RemainingBranchAction::Keep {
        object.insert("remaining_action_applied".to_string(), json!(false));
        object.insert("settled_total".to_string(), json!(0));
        object.insert("settled_items".to_string(), json!(Vec::<Value>::new()));
        return;
    }

    let settled_items = pending_items
        .iter()
        .map(|item| match action {
            RemainingBranchAction::Keep => {
                json!({ "status": "noop", "updated": false, "action": action.as_str() })
            }
            RemainingBranchAction::Interrupt => interrupt_remaining_session(context, item),
            RemainingBranchAction::Close => close_remaining_session(context, item),
        })
        .collect::<Vec<_>>();
    object.insert("remaining_action_applied".to_string(), json!(true));
    object.insert(
        "settled_total".to_string(),
        json!(settled_items.len() as i64),
    );
    object.insert("settled_items".to_string(), json!(settled_items));
}

fn collect_pending_settlement_items(completed_reason: &str, items: &[Value]) -> Vec<Value> {
    if !matches!(completed_reason, "first_success" | "first_terminal") {
        return Vec::new();
    }
    items
        .iter()
        .filter(|item| {
            !item
                .get("terminal")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter(|item| {
            item.get("session_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
        })
        .cloned()
        .collect()
}

fn interrupt_remaining_session(context: &ToolContext<'_>, item: &Value) -> Value {
    let session_id = item
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if session_id.is_empty() {
        return json!({
            "action": RemainingBranchAction::Interrupt.as_str(),
            "status": "error",
            "updated": false,
            "error": "session_id is required",
        });
    }
    let previous_status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let Some(monitor) = context.monitor.as_ref() else {
        return json!({
            "session_id": session_id,
            "action": RemainingBranchAction::Interrupt.as_str(),
            "status": "error",
            "updated": false,
            "previous_status": previous_status,
            "error": "monitor unavailable",
        });
    };
    let updated = monitor.cancel(session_id);
    let payload = json!({
        "session_id": session_id,
        "run_id": item.get("run_id").cloned().unwrap_or(Value::Null),
        "dispatch_id": item.get("dispatch_id").cloned().unwrap_or(Value::Null),
        "action": RemainingBranchAction::Interrupt.as_str(),
        "status": if updated { "cancelling" } else { "unchanged" },
        "updated": updated,
        "previous_status": previous_status,
    });
    emit_control_event(context, "subagent_interrupt", &payload);
    payload
}

fn close_remaining_session(context: &ToolContext<'_>, item: &Value) -> Value {
    let session_id = item
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if session_id.is_empty() {
        return json!({
            "action": RemainingBranchAction::Close.as_str(),
            "status": "error",
            "updated": false,
            "error": "session_id is required",
        });
    }
    let previous_status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match update_session_status(context, session_id, "closed", true) {
        Ok(mut payload) => {
            if let Some(object) = payload.as_object_mut() {
                object.insert(
                    "action".to_string(),
                    json!(RemainingBranchAction::Close.as_str()),
                );
                object.insert("previous_status".to_string(), json!(previous_status));
                object.insert(
                    "run_id".to_string(),
                    item.get("run_id").cloned().unwrap_or(Value::Null),
                );
                object.insert(
                    "dispatch_id".to_string(),
                    item.get("dispatch_id").cloned().unwrap_or(Value::Null),
                );
            }
            emit_control_event(context, "subagent_close", &payload);
            payload
        }
        Err(err) => json!({
            "session_id": session_id,
            "action": RemainingBranchAction::Close.as_str(),
            "status": "error",
            "updated": false,
            "previous_status": previous_status,
            "error": err.to_string(),
        }),
    }
}

fn decorate_dispatch_result(
    mut result: Value,
    strategy: BatchDispatchStrategy,
    label: Option<&str>,
    remaining_action: RemainingBranchAction,
) -> Value {
    let Some(object) = result.as_object_mut() else {
        return result;
    };
    object.insert("strategy".to_string(), json!(strategy.as_str()));
    object.insert(
        "completion_mode".to_string(),
        json!(completion_mode_from_strategy(strategy).as_str()),
    );
    object.insert("label".to_string(), json!(label));
    object.insert(
        "remaining_action".to_string(),
        json!(remaining_action.as_str()),
    );
    let selected_items = object
        .get("selected_items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let items = object
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(summary) = build_dispatch_summary(strategy, &items, &selected_items) {
        object.insert("summary".to_string(), json!(summary));
    }
    if strategy == BatchDispatchStrategy::FirstSuccess {
        if let Some(selected_item) = select_preferred_dispatch_item(&selected_items, true)
            .or_else(|| select_preferred_dispatch_item(&items, true))
        {
            object.insert("winner_item".to_string(), selected_item.clone());
            object.insert("selected_item".to_string(), selected_item);
        }
    }
    result
}

fn build_dispatch_summary(
    strategy: BatchDispatchStrategy,
    items: &[Value],
    selected_items: &[Value],
) -> Option<String> {
    match strategy {
        BatchDispatchStrategy::ParallelAll => None,
        BatchDispatchStrategy::FirstSuccess => {
            let preferred = select_preferred_dispatch_item(selected_items, true)
                .or_else(|| select_preferred_dispatch_item(items, true))?;
            build_dispatch_item_summary_line(&preferred)
        }
        BatchDispatchStrategy::ReviewThenMerge => {
            let lines = items
                .iter()
                .filter_map(build_dispatch_item_summary_line)
                .collect::<Vec<_>>();
            if lines.is_empty() {
                None
            } else {
                Some(lines.join("\n"))
            }
        }
    }
}

fn select_preferred_dispatch_item(items: &[Value], prefer_success: bool) -> Option<Value> {
    let preferred = items
        .iter()
        .filter(|item| {
            !prefer_success || item.get("status").and_then(Value::as_str) == Some("success")
        })
        .min_by(|left, right| dispatch_item_sort_key(left).cmp(&dispatch_item_sort_key(right)))
        .cloned();
    if preferred.is_some() || prefer_success {
        preferred
    } else {
        items
            .iter()
            .min_by(|left, right| dispatch_item_sort_key(left).cmp(&dispatch_item_sort_key(right)))
            .cloned()
    }
}

fn dispatch_item_sort_key(item: &Value) -> (i64, String) {
    let index = item
        .get("index")
        .and_then(Value::as_i64)
        .unwrap_or(i64::MAX);
    let key = item
        .get("run_id")
        .or_else(|| item.get("session_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    (index, key)
}

fn build_dispatch_item_summary_line(item: &Value) -> Option<String> {
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let label = item
        .get("label")
        .or_else(|| item.get("spawn_label"))
        .or_else(|| item.get("title"))
        .or_else(|| item.get("session_id"))
        .or_else(|| item.get("run_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("subagent");
    let detail = item
        .get("agent_state")
        .and_then(Value::as_object)
        .and_then(|state| state.get("message"))
        .and_then(Value::as_str)
        .or_else(|| item.get("result").and_then(Value::as_str))
        .or_else(|| item.get("error").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_text(value, SUBAGENT_SUMMARY_MAX_CHARS))
        .unwrap_or_else(|| status.to_string());
    Some(format!("[{label}][{status}] {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_status_prefers_terminal_run_status() {
        assert_eq!(
            resolve_effective_status("success", Some("running"), "active"),
            "success"
        );
        assert_eq!(
            resolve_effective_status("timeout", Some("running"), "active"),
            "timeout"
        );
    }

    #[test]
    fn effective_status_falls_back_to_runtime_and_session() {
        assert_eq!(
            resolve_effective_status("queued", Some("running"), "active"),
            "running"
        );
        assert_eq!(resolve_effective_status("", None, "closed"), "closed");
        assert_eq!(resolve_effective_status("", None, "active"), "idle");
    }

    #[test]
    fn resolve_targets_uses_default_parent_only_when_needed() {
        let fallback = resolve_targets(&SubagentTargetArgs::default(), Some(" parent_session "))
            .expect("default parent should be accepted");
        assert!(fallback.run_ids.is_empty());
        assert!(fallback.session_ids.is_empty());
        assert_eq!(fallback.dispatch_id.as_deref(), None);
        assert_eq!(fallback.parent_id.as_deref(), Some("parent_session"));
        assert_eq!(fallback.limit, 50);

        let explicit = resolve_targets(
            &SubagentTargetArgs {
                run_ids: Some(vec![" run_1 ".to_string(), "run_1".to_string()]),
                dispatch_id: Some(" dispatch_1 ".to_string()),
                limit: Some(MAX_SESSION_LIST_ITEMS + 100),
                ..SubagentTargetArgs::default()
            },
            Some("parent_session"),
        )
        .expect("explicit selectors should not inject parent");
        assert_eq!(explicit.run_ids, vec!["run_1".to_string()]);
        assert_eq!(explicit.dispatch_id.as_deref(), Some("dispatch_1"));
        assert_eq!(explicit.parent_id, None);
        assert_eq!(explicit.limit, MAX_SESSION_LIST_ITEMS);
    }

    #[test]
    fn resolve_targets_requires_any_selector_or_default_parent() {
        let error = resolve_targets(&SubagentTargetArgs::default(), None)
            .expect_err("empty selector should fail");
        assert!(error.to_string().contains("target is required"));
    }

    #[test]
    fn resolve_subagent_parent_scope_defaults_to_current_session() {
        let resolved = resolve_subagent_parent_scope(None, " sess_parent ")
            .expect("current session should become default parent");
        assert_eq!(resolved, "sess_parent");
    }

    #[test]
    fn resolve_subagent_parent_scope_prefers_explicit_parent() {
        let resolved =
            resolve_subagent_parent_scope(Some(" sess_explicit ".to_string()), "sess_current")
                .expect("explicit parent should win");
        assert_eq!(resolved, "sess_explicit");
    }

    #[test]
    fn direct_child_session_requires_parent_match() {
        assert!(is_direct_child_session(
            Some(" sess_parent "),
            "sess_parent"
        ));
        assert!(!is_direct_child_session(Some("sess_other"), "sess_parent"));
        assert!(!is_direct_child_session(None, "sess_parent"));
        assert!(!is_direct_child_session(Some("sess_parent"), ""));
    }

    #[test]
    fn select_single_direct_child_session_id_accepts_only_unique_session() {
        assert_eq!(
            select_single_direct_child_session_id(vec![" sess_child ".to_string()]),
            Some("sess_child".to_string())
        );
        assert_eq!(
            select_single_direct_child_session_id(vec![
                "sess_child".to_string(),
                " sess_child ".to_string()
            ]),
            Some("sess_child".to_string())
        );
        assert_eq!(
            select_single_direct_child_session_id(vec![
                "sess_child_1".to_string(),
                "sess_child_2".to_string()
            ]),
            None
        );
    }

    #[test]
    fn normalize_poll_interval_clamps_to_supported_range() {
        assert_eq!(
            normalize_poll_interval(f64::NAN),
            SUBAGENT_WAIT_DEFAULT_POLL_S
        );
        assert_eq!(normalize_poll_interval(-1.0), SUBAGENT_WAIT_DEFAULT_POLL_S);
        assert_eq!(normalize_poll_interval(0.1), SUBAGENT_WAIT_MIN_POLL_S);
        assert_eq!(normalize_poll_interval(10.0), SUBAGENT_WAIT_MAX_POLL_S);
        assert_eq!(normalize_poll_interval(1.5), 1.5);
    }

    #[test]
    fn merge_wait_result_preserves_dispatch_totals_and_partial_status() {
        let merged = merge_wait_result(
            json!({
                "status": "ok",
                "total": 2,
                "failed_total": 0,
                "items": [
                    { "run_id": "run_1", "status": "success" },
                    { "run_id": "run_2", "status": "success" }
                ]
            }),
            "dispatch_1",
            3,
            2,
            vec![json!({ "status": "error", "task": "failed at startup" })],
        );
        assert_eq!(
            merged.get("dispatch_id").and_then(Value::as_str),
            Some("dispatch_1")
        );
        assert_eq!(
            merged.get("requested_total").and_then(Value::as_i64),
            Some(3)
        );
        assert_eq!(
            merged.get("accepted_total").and_then(Value::as_i64),
            Some(2)
        );
        assert_eq!(
            merged.get("startup_failed_total").and_then(Value::as_i64),
            Some(1)
        );
        assert_eq!(merged.get("failed_total").and_then(Value::as_i64), Some(1));
        assert_eq!(merged.get("total").and_then(Value::as_i64), Some(3));
        assert_eq!(
            merged.get("status").and_then(Value::as_str),
            Some("partial")
        );
        assert_eq!(
            merged.get("items").and_then(Value::as_array).map(Vec::len),
            Some(3)
        );
    }

    #[test]
    fn wait_progress_any_mode_completes_on_first_terminal() {
        let progress = evaluate_wait_progress(
            WaitCompletionMode::Any,
            &[
                snapshot("run-1", "running", false),
                snapshot("run-2", "error", true),
            ],
        );
        assert!(progress.completion_reached);
        assert!(!progress.all_finished);
        assert_eq!(progress.matched_total, 1);
        assert_eq!(progress.completed_reason, "first_terminal");
    }

    #[test]
    fn wait_progress_first_success_prefers_success_before_all_done() {
        let progress = evaluate_wait_progress(
            WaitCompletionMode::FirstSuccess,
            &[
                snapshot("run-1", "success", true),
                snapshot("run-2", "running", false),
                snapshot("run-3", "error", true),
            ],
        );
        assert!(progress.completion_reached);
        assert_eq!(progress.matched_total, 1);
        assert_eq!(progress.matched_success_total, 1);
        assert_eq!(progress.completed_reason, "first_success");
    }

    #[test]
    fn dispatch_summary_first_success_picks_earliest_success() {
        let summary = build_dispatch_summary(
            BatchDispatchStrategy::FirstSuccess,
            &[
                json!({"index": 3, "label": "late", "status": "success", "result": "second"}),
                json!({"index": 1, "label": "early", "status": "success", "result": "first"}),
            ],
            &[],
        )
        .expect("summary should exist");
        assert_eq!(summary, "[early][success] first");
    }

    #[test]
    fn dispatch_summary_review_then_merge_collects_lines() {
        let summary = build_dispatch_summary(
            BatchDispatchStrategy::ReviewThenMerge,
            &[
                json!({"label": "alpha", "status": "success", "result": "done"}),
                json!({"label": "beta", "status": "error", "error": "failed"}),
            ],
            &[],
        )
        .expect("summary should exist");
        assert_eq!(summary, "[alpha][success] done\n[beta][error] failed");
    }

    #[test]
    fn collab_agent_status_maps_wunder_states() {
        assert_eq!(collab_agent_status("queued"), "pending_init");
        assert_eq!(collab_agent_status("cancelled"), "interrupted");
        assert_eq!(collab_agent_status("success"), "completed");
        assert_eq!(collab_agent_status("closed"), "shutdown");
        assert_eq!(collab_agent_status("error"), "errored");
    }

    #[test]
    fn first_success_strategy_defaults_to_interrupt_remaining() {
        assert_eq!(
            default_remaining_branch_action_for_strategy(BatchDispatchStrategy::FirstSuccess),
            RemainingBranchAction::Interrupt
        );
        assert_eq!(
            default_remaining_branch_action_for_strategy(BatchDispatchStrategy::ReviewThenMerge),
            RemainingBranchAction::Keep
        );
    }

    #[test]
    fn collect_pending_settlement_items_only_returns_active_items_for_early_completion() {
        let items = collect_pending_settlement_items(
            "first_success",
            &[
                json!({"session_id": "sess_winner", "status": "success", "terminal": true}),
                json!({"session_id": "sess_running", "status": "running", "terminal": false}),
                json!({"session_id": "sess_waiting", "status": "waiting", "terminal": false}),
            ],
        );
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].get("session_id").and_then(Value::as_str),
            Some("sess_running")
        );
        assert_eq!(
            items[1].get("session_id").and_then(Value::as_str),
            Some("sess_waiting")
        );
        assert!(collect_pending_settlement_items("all_finished", &items).is_empty());
    }

    #[test]
    fn decorate_dispatch_result_adds_winner_item_for_first_success() {
        let result = decorate_dispatch_result(
            json!({
                "items": [
                    {"index": 2, "label": "beta", "status": "success", "result": "second"},
                    {"index": 1, "label": "alpha", "status": "success", "result": "first"}
                ],
                "selected_items": [],
            }),
            BatchDispatchStrategy::FirstSuccess,
            Some("race"),
            RemainingBranchAction::Interrupt,
        );
        assert_eq!(result.get("winner_item"), result.get("selected_item"));
        assert_eq!(
            result
                .get("winner_item")
                .and_then(|value| value.get("label"))
                .and_then(Value::as_str),
            Some("alpha")
        );
        assert_eq!(
            result.get("remaining_action").and_then(Value::as_str),
            Some("interrupt")
        );
    }

    #[test]
    fn bounded_edit_distance_handles_single_missing_character() {
        let requested = "sess_3d1c426e1e524e59d73b05045a68326";
        let actual = "sess_3d1c426e1e5244e59d73b05045a68326";
        assert_eq!(bounded_edit_distance(requested, actual, 2), Some(1));
    }

    #[test]
    fn bounded_edit_distance_rejects_far_match() {
        assert_eq!(bounded_edit_distance("sess_alpha", "sess_omega", 2), None);
    }

    #[test]
    fn selected_items_all_not_found_only_matches_full_not_found_sets() {
        assert!(selected_items_all_not_found(&json!({
            "selected_items": [
                {"status": "not_found", "error": "session not found"},
                {"status": "not_found", "error": "run not found"}
            ]
        })));
        assert!(!selected_items_all_not_found(&json!({
            "selected_items": [
                {"status": "not_found", "error": "session not found"},
                {"status": "running"}
            ]
        })));
    }

    fn snapshot(key: &str, status: &str, terminal: bool) -> SubagentRunSnapshot {
        SubagentRunSnapshot {
            key: key.to_string(),
            status: status.to_string(),
            terminal,
            failed: is_failed_status(status),
            updated_time: 0.0,
            payload: json!({
                "run_id": key,
                "status": status,
            }),
        }
    }
}
