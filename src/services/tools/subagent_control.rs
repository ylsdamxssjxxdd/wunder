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
    #[serde(default)]
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

pub(super) async fn execute(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentControlArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = payload.action.trim();
    if action.is_empty() {
        return Err(anyhow!("subagent_control action is required"));
    }
    match action.to_ascii_lowercase().as_str() {
        "list" | "sessions_list" | "session_list" | "会话列表" | "列表" => {
            super::sessions_list(context, args).await
        }
        "history" | "sessions_history" | "session_history" | "会话历史" | "历史" => {
            super::sessions_history(context, args).await
        }
        "send" | "sessions_send" | "session_send" | "会话发送" | "发送" => {
            super::sessions_send(context, args).await
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

async fn batch_spawn(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentBatchSpawnArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
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
    emit_dispatch_start(
        context,
        &dispatch_id,
        tasks.len() as i64,
        dispatch_label.as_deref(),
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
        ) {
            Ok(prepared) => {
                let PreparedChildSession {
                    child_session_id,
                    child_agent_id,
                    model_name,
                    request,
                    announce,
                } = prepared;
                let run_id = format!("run_{}", Uuid::new_v4().simple());
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
        let result = json!({
            "status": "error",
            "dispatch_id": dispatch_id.clone(),
            "requested_total": startup_items.len(),
            "accepted_total": 0,
            "startup_failed_total": startup_failed_items.len(),
            "items": startup_items,
        });
        emit_control_event(context, "subagent_dispatch_finish", &result);
        return Ok(result);
    }

    let wait_seconds = payload.wait_seconds.unwrap_or(0.0).max(0.0);
    if wait_seconds <= 0.0 {
        let result = json!({
            "status": if startup_failed_items.is_empty() { "accepted" } else { "partial" },
            "dispatch_id": dispatch_id.clone(),
            "requested_total": startup_items.len(),
            "accepted_total": run_ids.len(),
            "startup_failed_total": startup_failed_items.len(),
            "run_ids": run_ids,
            "items": startup_items,
        });
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
        true,
    )
    .await?;
    let merged = merge_wait_result(
        wait_result,
        &dispatch_id,
        startup_items.len() as i64,
        run_ids.len() as i64,
        startup_failed_items,
    );
    emit_control_event(context, "subagent_dispatch_finish", &merged);
    Ok(merged)
}

async fn status(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentStatusArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let selector = resolve_targets(&payload.target, Some(context.session_id))?;
    let summary = summarize_snapshots(
        &selector,
        collect_snapshots(context, &selector)?,
        0.0,
        0.0,
        true,
        false,
    );
    emit_control_event(context, "subagent_status", &summary);
    Ok(summary)
}

async fn wait(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentWaitArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let selector = resolve_targets(&payload.target, None)?;
    let result = wait_for_targets(
        context,
        selector,
        payload.wait_seconds.unwrap_or(0.0).max(0.0),
        payload
            .poll_interval_seconds
            .unwrap_or(SUBAGENT_WAIT_DEFAULT_POLL_S),
        true,
    )
    .await?;
    if result.get("dispatch_id").and_then(Value::as_str).is_some() {
        emit_control_event(context, "subagent_dispatch_finish", &result);
    } else {
        emit_control_event(context, "subagent_status", &result);
    }
    Ok(result)
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
        Ok(SubagentRunSnapshot {
            key: record.run_id.clone(),
            status: status.clone(),
            terminal,
            failed,
            updated_time: record.updated_time,
            payload: json!({
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
                "updated_time": record.updated_time,
                "title": session.as_ref().map(|entry| entry.title.clone()),
                "spawn_label": session.as_ref().and_then(|entry| entry.spawn_label.clone()),
                "spawned_by": session.as_ref().and_then(|entry| entry.spawned_by.clone()),
            }),
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
    Ok(SubagentRunSnapshot {
        key: session.session_id.clone(),
        status: status.clone(),
        terminal,
        failed,
        updated_time: session.updated_at,
        payload: json!({
            "status": status,
            "runtime_status": runtime_status,
            "session_status": session_status,
            "terminal": terminal,
            "failed": failed,
            "session_id": session.session_id,
            "parent_session_id": session.parent_session_id,
            "agent_id": session.agent_id,
            "title": session.title,
            "spawn_label": session.spawn_label,
            "spawned_by": session.spawned_by,
            "updated_time": session.updated_at,
        }),
    })
}

async fn wait_for_targets(
    context: &ToolContext<'_>,
    selector: ResolvedTargetSet,
    wait_seconds: f64,
    poll_interval_seconds: f64,
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
                true,
                false,
            ));
        }
        let elapsed_s = started_at.elapsed().as_secs_f64();
        let all_finished = snapshots.iter().all(|item| item.terminal);
        let timed_out = wait_seconds > 0.0 && elapsed_s >= wait_seconds && !all_finished;
        emit_wait_updates(context, &selector, &snapshots, &mut status_index);
        if emit_progress {
            emit_wait_progress(context, &selector, &snapshots, elapsed_s);
        }
        if all_finished || timed_out || wait_seconds <= 0.0 {
            return Ok(summarize_snapshots(
                &selector,
                snapshots,
                wait_seconds,
                elapsed_s,
                all_finished,
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
    all_finished: bool,
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
    let status = if total == 0 {
        "empty"
    } else if timed_out {
        "timeout"
    } else if all_finished {
        if failed_total == 0 {
            "ok"
        } else {
            "partial"
        }
    } else {
        "running"
    };
    json!({
        "status": status,
        "dispatch_id": selector.dispatch_id.clone(),
        "parent_id": selector.parent_id.clone(),
        "wait_seconds": wait_seconds,
        "elapsed_s": elapsed_s,
        "all_finished": all_finished,
        "total": total,
        "done_total": done_total,
        "success_total": success_total,
        "failed_total": failed_total,
        "queued_total": queued_total,
        "running_total": running_total,
        "run_ids": selector.run_ids.clone(),
        "session_ids": selector.session_ids.clone(),
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
) {
    emit_control_event(
        context,
        "subagent_dispatch_start",
        &json!({
            "dispatch_id": dispatch_id,
            "parent_session_id": context.session_id,
            "total": total,
            "label": label,
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
}
