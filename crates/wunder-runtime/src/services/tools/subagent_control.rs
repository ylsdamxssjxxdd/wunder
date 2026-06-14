use super::tool_error::{build_failed_tool_result, ToolErrorMeta};
use super::*;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

mod flow;
mod targeting;
use flow::*;
use targeting::*;

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
    #[serde(
        default,
        rename = "waitSeconds",
        alias = "wait_seconds",
        alias = "timeoutSeconds",
        alias = "timeout_seconds"
    )]
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

fn compact_subagent_item_for_model(item: &Value) -> Value {
    let label = item
        .get("label")
        .or_else(|| item.get("spawn_label"))
        .or_else(|| item.get("title"))
        .cloned()
        .unwrap_or(Value::Null);
    let result_preview = item
        .get("result_preview")
        .cloned()
        .or_else(|| item.get("result").cloned())
        .or_else(|| item.pointer("/agent_state/message").cloned())
        .unwrap_or(Value::Null);
    json!({
        "index": item.get("index").cloned().unwrap_or(Value::Null),
        "dispatch_id": item.get("dispatch_id").cloned().unwrap_or(Value::Null),
        "run_id": item.get("run_id").cloned().unwrap_or(Value::Null),
        "session_id": item.get("session_id").cloned().unwrap_or(Value::Null),
        "status": item.get("status").cloned().unwrap_or(Value::Null),
        "terminal": item.get("terminal").cloned().unwrap_or(Value::Null),
        "failed": item.get("failed").cloned().unwrap_or(Value::Null),
        "updated": item.get("updated").cloned().unwrap_or(Value::Null),
        "agent_id": item.get("agent_id").cloned().unwrap_or(Value::Null),
        "label": label,
        "elapsed_s": item.get("elapsed_s").cloned().unwrap_or(Value::Null),
        "result_preview": result_preview,
        "error": item.get("error").cloned().unwrap_or(Value::Null),
    })
}

fn compact_subagent_items(items: &[Value]) -> Vec<Value> {
    items.iter().map(compact_subagent_item_for_model).collect()
}

fn build_subagent_list_result(value: Value) -> Value {
    let total = value.get("total").and_then(Value::as_i64).unwrap_or(0);
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    super::build_model_tool_success(
        "list",
        "completed",
        format!("Found {total} child sessions."),
        json!({
            "total": total,
            "items": compact_subagent_items(&items),
        }),
    )
}

fn build_subagent_history_result(value: Value) -> Value {
    let payload = value.get("data").unwrap_or(&value);
    let session_id = payload
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let messages = payload
        .get("messages")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let total = messages.as_array().map(Vec::len).unwrap_or(0);
    super::build_model_tool_success(
        "history",
        "completed",
        format!("Loaded {total} messages from child session history."),
        json!({
            "session_id": session_id,
            "messages": messages,
        }),
    )
}

fn build_subagent_update_result(action: &str, value: Value) -> Value {
    let updated_total = value
        .get("updated_total")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let state = if updated_total > 0 {
        "completed"
    } else {
        "noop"
    };
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    super::build_model_tool_success(
        action,
        state,
        format!("{action} updated {updated_total} child sessions."),
        json!({
            "updated_total": updated_total,
            "items": compact_subagent_items(&items),
        }),
    )
}

fn compact_subagent_wait_result(action: &str, value: Value) -> Value {
    let state = value
        .get("status")
        .and_then(Value::as_str)
        .map(|status| match status {
            "ok" => "completed",
            other => other,
        })
        .unwrap_or("running");
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let selected_items = value
        .get("selected_items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut data = json!({
        "dispatch_id": value.get("dispatch_id").cloned().unwrap_or(Value::Null),
        "parent_id": value.get("parent_id").cloned().unwrap_or(Value::Null),
        "completion_mode": value.get("completion_mode").cloned().unwrap_or(Value::Null),
        "completed_reason": value.get("completed_reason").cloned().unwrap_or(Value::Null),
        "wait_seconds": value.get("wait_seconds").cloned().unwrap_or(Value::Null),
        "elapsed_s": value.get("elapsed_s").cloned().unwrap_or(Value::Null),
        "all_finished": value.get("all_finished").cloned().unwrap_or(Value::Null),
        "run_ids": value.get("run_ids").cloned().unwrap_or_else(|| json!([])),
        "session_ids": value.get("session_ids").cloned().unwrap_or_else(|| json!([])),
        "counts": {
            "total": value.get("total").cloned().unwrap_or(Value::Null),
            "done": value.get("done_total").cloned().unwrap_or(Value::Null),
            "success": value.get("success_total").cloned().unwrap_or(Value::Null),
            "failed": value.get("failed_total").cloned().unwrap_or(Value::Null),
            "queued": value.get("queued_total").cloned().unwrap_or(Value::Null),
            "running": value.get("running_total").cloned().unwrap_or(Value::Null),
            "selected": value.get("selected_total").cloned().unwrap_or(Value::Null),
            "selected_success": value.get("selected_success_total").cloned().unwrap_or(Value::Null),
            "selected_failed": value.get("selected_failed_total").cloned().unwrap_or(Value::Null),
        },
        "selected_items": compact_subagent_items(&selected_items),
        "items": compact_subagent_items(&items),
    });
    if let Some(extra_keys) = data.as_object_mut() {
        for key in [
            "remaining_action",
            "remaining_active_total",
            "remaining_action_applied",
            "settled_total",
            "settled_items",
            "requested_total",
            "accepted_total",
            "startup_failed_total",
            "summary",
            "winner_item",
            "selected_item",
            "strategy",
            "label",
        ] {
            if let Some(value) = value.get(key) {
                extra_keys.insert(
                    key.to_string(),
                    if matches!(key, "settled_items") {
                        Value::Array(
                            value
                                .as_array()
                                .cloned()
                                .unwrap_or_default()
                                .iter()
                                .map(compact_subagent_item_for_model)
                                .collect(),
                        )
                    } else if matches!(key, "winner_item" | "selected_item") {
                        compact_subagent_item_for_model(value)
                    } else {
                        value.clone()
                    },
                );
            }
        }
    }
    super::build_model_tool_success_with_hint(
        action,
        state,
        value
            .get("summary")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("subagent_control {action} returned {state}.")),
        data,
        if matches!(state, "running" | "timeout") {
            Some(
                "Use subagent_control.wait/status/history before treating unfinished child runs as complete."
                    .to_string(),
            )
        } else {
            None
        },
    )
}

async fn list(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: super::SessionListArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let parent_session_id = resolve_subagent_parent_scope(payload.parent_id, context.session_id)?;
    let mut scoped_args = args.clone();
    if let Value::Object(ref mut map) = scoped_args {
        map.insert("parentId".to_string(), json!(parent_session_id));
    }
    Ok(build_subagent_list_result(
        super::sessions_list(context, &scoped_args).await?,
    ))
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
    Ok(build_subagent_history_result(
        super::sessions_history(context, &scoped_args).await?,
    ))
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
    let wait_seconds = payload.wait_seconds.unwrap_or(0.0).max(0.0);
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
                super::sync_announce_auto_wake(
                    &mut announce,
                    Some(&mut run_metadata),
                    super::should_auto_wake_parent_after_child_run(false, wait_seconds),
                );
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
                        team_task_id: None,
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
        let result = compact_subagent_wait_result("batch_spawn", result);
        emit_control_event(context, "subagent_dispatch_finish", &result);
        return Ok(result);
    }

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
        let result = compact_subagent_wait_result("batch_spawn", result);
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
    let merged = compact_subagent_wait_result("batch_spawn", merged);
    emit_control_event(context, "subagent_dispatch_finish", &merged);
    Ok(merged)
}

async fn status(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentStatusArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let target = normalize_status_wait_target(context, &payload.target, "status")?;
    let selector = resolve_targets(&target, Some(context.session_id))?;
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
    crate::services::subagents::suppress_auto_wake_from_wait_result_with_parent(
        &summary,
        Some(context.session_id),
    );
    let summary = compact_subagent_wait_result("status", summary);
    emit_control_event(context, "subagent_status", &summary);
    Ok(wrap_missing_target_summary(summary, "status"))
}

async fn wait(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentWaitArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let target = normalize_status_wait_target(context, &payload.target, "wait")?;
    let selector = resolve_targets(&target, None)?;
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
    // Suppress parent auto-wake before compaction because compacted items drop
    // parent_session_id and can no longer be matched against the wake registry.
    crate::services::subagents::suppress_auto_wake_from_wait_result_with_parent(
        &result,
        Some(context.session_id),
    );
    let result = compact_subagent_wait_result("wait", result);
    if result
        .pointer("/data/dispatch_id")
        .and_then(Value::as_str)
        .is_some()
    {
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
    build_failed_tool_result(
        format!(
            "subagent_control {action} target not found under the current session"
        ),
        summary,
        ToolErrorMeta::new(
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
    let Some(items) = summary
        .get("selected_items")
        .or_else(|| summary.pointer("/data/selected_items"))
        .and_then(Value::as_array)
    else {
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
    Ok(build_subagent_update_result(
        "interrupt",
        json!({ "updated_total": updated_total, "items": items }),
    ))
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
    Ok(build_subagent_update_result(
        if next_status == "closed" {
            "close"
        } else {
            "resume"
        },
        json!({ "updated_total": updated_total, "items": items }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a_store::A2aStore;
    use crate::config::Config;
    use crate::lsp::LspManager;
    use crate::skills::SkillRegistry;
    use crate::storage::{ChatSessionRecord, SessionRunRecord, SqliteStorage, StorageBackend};
    use crate::workspace::WorkspaceManager;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::{tempdir, TempDir};

    struct SubagentTestHarness {
        _dir: TempDir,
        storage: Arc<dyn StorageBackend>,
        workspace: Arc<WorkspaceManager>,
        lsp_manager: Arc<LspManager>,
        config: Config,
        a2a_store: A2aStore,
        skills: SkillRegistry,
        http: reqwest::Client,
    }

    impl SubagentTestHarness {
        fn new() -> Self {
            let dir = tempdir().expect("tempdir");
            let db_path = dir.path().join("subagent-control-tests.db");
            let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
            storage.ensure_initialized().expect("init storage");
            let storage: Arc<dyn StorageBackend> = storage;
            let workspace_root = dir.path().join("workspace");
            let workspace = Arc::new(WorkspaceManager::new(
                workspace_root.to_string_lossy().as_ref(),
                storage.clone(),
                0,
                &HashMap::new(),
            ));
            Self {
                _dir: dir,
                storage,
                lsp_manager: LspManager::new(workspace.clone()),
                workspace,
                config: Config::default(),
                a2a_store: A2aStore::default(),
                skills: SkillRegistry::default(),
                http: reqwest::Client::new(),
            }
        }

        fn upsert_session(&self, session_id: &str, parent_session_id: Option<&str>) {
            self.storage
                .upsert_chat_session(&ChatSessionRecord {
                    session_id: session_id.to_string(),
                    user_id: "alice".to_string(),
                    title: session_id.to_string(),
                    status: "active".to_string(),
                    created_at: 1.0,
                    updated_at: 2.0,
                    last_message_at: 2.0,
                    agent_id: Some("agent_parent".to_string()),
                    tool_overrides: Vec::new(),
                    parent_session_id: parent_session_id.map(str::to_string),
                    parent_message_id: None,
                    spawn_label: None,
                    spawned_by: None,
                })
                .expect("upsert chat session");
        }

        fn upsert_run(&self, run_id: &str, session_id: &str, parent_session_id: Option<&str>) {
            self.storage
                .upsert_session_run(&SessionRunRecord {
                    run_id: run_id.to_string(),
                    session_id: session_id.to_string(),
                    parent_session_id: parent_session_id.map(str::to_string),
                    user_id: "alice".to_string(),
                    dispatch_id: None,
                    run_kind: Some("subagent".to_string()),
                    requested_by: Some("subagent_control".to_string()),
                    agent_id: Some("agent_parent".to_string()),
                    model_name: Some("test-model".to_string()),
                    status: "success".to_string(),
                    queued_time: 1.0,
                    started_time: 2.0,
                    finished_time: 3.0,
                    elapsed_s: 1.0,
                    result: Some("done".to_string()),
                    error: None,
                    updated_time: 3.0,
                    metadata: None,
                })
                .expect("upsert session run");
        }

        fn context<'a>(&'a self, session_id: &'a str) -> ToolContext<'a> {
            ToolContext {
                user_id: "alice",
                session_id,
                workspace_id: "workspace-test",
                agent_id: Some("agent_parent"),
                user_round: Some(1),
                model_round: Some(1),
                is_admin: false,
                storage: self.storage.clone(),
                orchestrator: None,
                monitor: None,
                beeroom_realtime: None,
                workspace: self.workspace.clone(),
                lsp_manager: self.lsp_manager.clone(),
                config: &self.config,
                a2a_store: &self.a2a_store,
                skills: &self.skills,
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
                http: &self.http,
            }
        }
    }

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
    fn single_child_autocorrect_predicates_require_single_unscoped_target() {
        assert!(should_autocorrect_single_child_session_target(
            &SubagentTargetArgs {
                session_id: Some("sess_child".to_string()),
                ..SubagentTargetArgs::default()
            }
        ));
        assert!(!should_autocorrect_single_child_session_target(
            &SubagentTargetArgs {
                session_id: Some("sess_child".to_string()),
                run_id: Some("run_child".to_string()),
                ..SubagentTargetArgs::default()
            }
        ));
        assert!(!should_autocorrect_single_child_session_target(
            &SubagentTargetArgs {
                session_ids: Some(vec!["sess_child_1".to_string(), "sess_child_2".to_string()]),
                ..SubagentTargetArgs::default()
            }
        ));
        assert!(should_autocorrect_single_child_run_target(
            &SubagentTargetArgs {
                run_id: Some("run_child".to_string()),
                ..SubagentTargetArgs::default()
            }
        ));
        assert!(!should_autocorrect_single_child_run_target(
            &SubagentTargetArgs {
                run_id: Some("run_child".to_string()),
                session_id: Some("sess_child".to_string()),
                ..SubagentTargetArgs::default()
            }
        ));
        assert!(!should_autocorrect_single_child_run_target(
            &SubagentTargetArgs {
                run_ids: Some(vec!["run_child_1".to_string(), "run_child_2".to_string()]),
                ..SubagentTargetArgs::default()
            }
        ));
    }

    #[tokio::test]
    async fn status_autocorrects_mistyped_single_child_session_id() {
        let harness = SubagentTestHarness::new();
        harness.upsert_session("sess_parent", None);
        harness.upsert_session("sess_9bd234172af749a69f760bd77cee4997", Some("sess_parent"));
        let context = harness.context("sess_parent");

        let result = status(
            &context,
            &json!({
                "session_id": "sess_9bd234172af749a69f760bd7cee497"
            }),
        )
        .await
        .expect("status should succeed");

        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            result
                .pointer("/data/items/0/session_id")
                .and_then(Value::as_str),
            Some("sess_9bd234172af749a69f760bd77cee4997")
        );
    }

    #[tokio::test]
    async fn wait_autocorrects_mistyped_single_child_session_id() {
        let harness = SubagentTestHarness::new();
        harness.upsert_session("sess_parent", None);
        harness.upsert_session("sess_c3387af607f44d47bf90e9b5893ce116", Some("sess_parent"));
        let context = harness.context("sess_parent");

        let result = wait(
            &context,
            &json!({
                "session_id": "sess_c387af607f4d47bf90e9b5893ce16",
                "wait_seconds": 0
            }),
        )
        .await
        .expect("wait should succeed");

        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            result
                .pointer("/data/session_ids/0")
                .and_then(Value::as_str),
            Some("sess_c3387af607f44d47bf90e9b5893ce116")
        );
        assert_eq!(
            result
                .pointer("/data/items/0/session_id")
                .and_then(Value::as_str),
            Some("sess_c3387af607f44d47bf90e9b5893ce116")
        );
    }

    #[tokio::test]
    async fn wait_autocorrects_mistyped_single_child_run_id() {
        let harness = SubagentTestHarness::new();
        harness.upsert_session("sess_parent", None);
        harness.upsert_session("sess_child", Some("sess_parent"));
        harness.upsert_run(
            "run_5a57ebf92c9e4bd781b33b77c0448568",
            "sess_child",
            Some("sess_parent"),
        );
        let context = harness.context("sess_parent");

        let result = wait(
            &context,
            &json!({
                "run_id": "run_5a57ebf92c9e4bd781b33b77c048568",
                "wait_seconds": 0
            }),
        )
        .await
        .expect("wait should succeed");

        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            result
                .pointer("/data/items/0/run_id")
                .and_then(Value::as_str),
            Some("run_5a57ebf92c9e4bd781b33b77c0448568")
        );
        assert_eq!(
            result
                .pointer("/data/items/0/session_id")
                .and_then(Value::as_str),
            Some("sess_child")
        );
    }

    #[tokio::test]
    async fn wait_accepts_timeout_seconds_alias_for_wait_seconds() {
        let harness = SubagentTestHarness::new();
        harness.upsert_session("sess_parent", None);
        harness.upsert_session("sess_child_timeout_alias", Some("sess_parent"));
        harness.upsert_run(
            "run_timeout_alias",
            "sess_child_timeout_alias",
            Some("sess_parent"),
        );
        let context = harness.context("sess_parent");

        let result = wait(
            &context,
            &json!({
                "run_id": "run_timeout_alias",
                "timeout_seconds": 60
            }),
        )
        .await
        .expect("wait should accept timeout_seconds alias");

        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            result.pointer("/data/wait_seconds").and_then(Value::as_f64),
            Some(60.0)
        );
    }

    #[tokio::test]
    async fn status_completed_result_suppresses_follow_up_auto_wake() {
        let harness = SubagentTestHarness::new();
        harness.upsert_session("sess_parent_status_consume", None);
        harness.upsert_session(
            "sess_child_status_consume",
            Some("sess_parent_status_consume"),
        );
        harness.upsert_run(
            "run_status_consume",
            "sess_child_status_consume",
            Some("sess_parent_status_consume"),
        );
        let context = harness.context("sess_parent_status_consume");

        let result = status(
            &context,
            &json!({
                "run_id": "run_status_consume"
            }),
        )
        .await
        .expect("status should succeed");

        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
        assert!(crate::services::subagents::is_auto_wake_consumed(
            "sess_parent_status_consume",
            None,
            Some("run_status_consume")
        ));
    }

    #[tokio::test]
    async fn status_completed_result_suppresses_follow_up_auto_wake_when_run_parent_is_missing() {
        let harness = SubagentTestHarness::new();
        harness.upsert_session("sess_parent_status_consume_fallback", None);
        harness.upsert_session(
            "sess_child_status_consume_fallback",
            Some("sess_parent_status_consume_fallback"),
        );
        harness.upsert_run(
            "run_status_consume_fallback",
            "sess_child_status_consume_fallback",
            None,
        );
        let context = harness.context("sess_parent_status_consume_fallback");

        let result = status(
            &context,
            &json!({
                "session_id": "sess_child_status_consume_fallback"
            }),
        )
        .await
        .expect("status should succeed");

        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
        assert!(crate::services::subagents::is_auto_wake_consumed(
            "sess_parent_status_consume_fallback",
            None,
            Some("run_status_consume_fallback")
        ));
    }

    #[tokio::test]
    async fn history_autocorrects_mistyped_single_child_session_id_and_preserves_wrapped_payload() {
        let harness = SubagentTestHarness::new();
        harness.upsert_session("sess_parent", None);
        harness.upsert_session("sess_414776c8546544509b592d06a7fb2c0b", Some("sess_parent"));
        let context = harness.context("sess_parent");

        let result = history(
            &context,
            &json!({
                "session_id": "sess_41476c8546544509b592d06a7fb2c0b"
            }),
        )
        .await
        .expect("history should succeed");

        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            result.pointer("/data/session_id").and_then(Value::as_str),
            Some("sess_414776c8546544509b592d06a7fb2c0b")
        );
        assert!(result
            .pointer("/data/messages")
            .and_then(Value::as_array)
            .is_some());
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
