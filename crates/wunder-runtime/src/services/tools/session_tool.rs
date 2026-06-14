use super::{
    build_effective_tool_names, build_model_tool_success, build_parent_follow_up_announce,
    build_session_tool_result, clamp_limit, insert_run_metadata_field, load_agent_record,
    load_session_messages, normalize_optional_string, now_ts, parse_cleanup_mode,
    prepare_child_session, resolve_session_key, session_cleanup_label,
    should_auto_wake_parent_after_child_run, should_auto_wake_parent_follow_up, spawn_session_run,
    sync_announce_auto_wake, ChildSessionToolMode, PreparedChildSession, SessionCleanup,
    SessionHistoryArgs, SessionListArgs, SessionRunMeta, SessionSendArgs, SessionSpawnArgs,
    ToolContext, MAX_SESSION_HISTORY_ITEMS, MAX_SESSION_LIST_ITEMS, MAX_SESSION_MESSAGE_ITEMS,
};
use crate::i18n;
use crate::schemas::WunderRequest;
use crate::services::subagents;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::time::timeout;
use uuid::Uuid;

pub(crate) async fn sessions_list(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
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
            "updated_at": super::format_ts(record.updated_at),
            "last_message_at": super::format_ts(record.last_message_at),
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

pub(crate) async fn sessions_history(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
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

pub(crate) async fn sessions_send(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
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
        workspace_container_id: None,
        model_name: model_name.clone(),
        language: Some(i18n::get_language()),
        config_overrides: context.request_config_overrides.cloned(),
        agent_prompt,
        preview_skill: agent_record
            .as_ref()
            .map(|record| record.preview_skill)
            .unwrap_or(false),
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

pub(crate) async fn sessions_spawn(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
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
        child_agent_id.clone(),
        model_name.clone(),
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
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(
            "subagent_dispatch_item_update",
            json!({
                "status": "accepted",
                "terminal": false,
                "failed": false,
                "run_id": run_id.clone(),
                "session_id": child_session_id.clone(),
                "parent_session_id": context.session_id,
                "label": payload.label.clone(),
                "agent_id": child_agent_id.clone(),
                "model_name": model_name.clone(),
                "parent_user_round": context.user_round,
                "parent_model_round": context.model_round,
                "can_terminate": true,
            }),
        );
    }
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
