use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::orchestrator::OrchestratorError;
use crate::schemas::{AttachmentPayload, WunderRequest};
use crate::services::agent_abilities::resolve_agent_runtime_tool_names;
use crate::services::chat_cancel_marker::persist_user_cancelled_turn_marker;
use crate::services::llm::is_llm_model;
use crate::services::orchestration_context::{
    active_orchestration_for_agent, build_locked_thread_message,
    repair_orchestration_session_main_thread, session_orchestration_lock_info,
    ORCHESTRATION_THREAD_LOCKED_CODE,
};
use crate::services::runtime::thread::ThreadSubmitOutcome;
use crate::services::subagents;
use crate::state::AppState;
use crate::user_access::{build_user_tool_context, compute_allowed_tool_names, is_agent_allowed};
use crate::user_store::UserStore;
use anyhow::Error;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

const DEFAULT_SESSION_TITLE: &str = "新会话";
const TOOL_OVERRIDE_NONE: &str = "__no_tools__";
const CHAT_SESSION_STATUS_ACTIVE: &str = "active";
const CHAT_SESSION_STATUS_ARCHIVED: &str = "archived";
const ORCHESTRATION_SOURCE_HEADER: &str = "x-wunder-orchestration-source";
pub(crate) const ORCHESTRATION_SOURCE_ALLOW: &str = "beeroom_orchestration";

mod events;
mod media;
mod prompt;
mod sessions;

use sessions::{is_session_runtime_active, is_session_stream_active_or_queued};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(events::router())
        .merge(media::router())
        .merge(prompt::router())
        .merge(sessions::router())
        .route(
            "/wunder/chat/sessions/{session_id}/subagents",
            get(list_session_subagents),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/subagents/control",
            post(control_session_subagents),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/tools",
            post(update_session_tools),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/messages",
            post(send_message),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/cancel",
            post(cancel_session),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/compaction",
            post(compact_session),
        )
}

#[derive(Debug, Deserialize)]
struct SendMessageRequest {
    content: String,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default, alias = "debugPayload", alias = "debug_payload")]
    debug_payload: bool,
    #[serde(default)]
    attachments: Option<Vec<ChatAttachment>>,
    #[serde(default)]
    tool_call_mode: Option<String>,
    #[serde(
        default,
        alias = "approvalMode",
        alias = "approval_mode",
        alias = "permissionLevel",
        alias = "permission_level"
    )]
    approval_mode: Option<String>,
}

pub(crate) struct ChatRequestOverrides {
    pub(crate) tool_call_mode: Option<String>,
    pub(crate) approval_mode: Option<String>,
    pub(crate) debug_payload: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatAttachment {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default, alias = "publicPath")]
    public_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionSubagentQuery {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "dispatchId", alias = "dispatch_id")]
    dispatch_id: Option<String>,
    #[serde(
        default,
        rename = "parentTurnRef",
        alias = "parent_turn_ref",
        alias = "turn_ref",
        alias = "turnRef"
    )]
    parent_turn_ref: Option<String>,
    #[serde(
        default,
        rename = "parentUserRound",
        alias = "parent_user_round",
        alias = "user_round",
        alias = "userRound"
    )]
    parent_user_round: Option<i64>,
    #[serde(
        default,
        rename = "latestTurnOnly",
        alias = "latest_turn_only",
        alias = "latest_turn",
        alias = "latestTurn"
    )]
    latest_turn_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SessionSubagentControlRequest {
    action: String,
    #[serde(default, rename = "sessionIds", alias = "session_ids")]
    session_ids: Vec<String>,
    #[serde(default, rename = "dispatchId", alias = "dispatch_id")]
    dispatch_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionToolsUpdateRequest {
    #[serde(default)]
    tool_overrides: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SessionCompactionRequest {
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default, alias = "debugPayload", alias = "debug_payload")]
    debug_payload: bool,
}

async fn list_session_subagents(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<SessionSubagentQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let items = subagents::list_parent_subagents_with_options(
        state.storage.as_ref(),
        Some(state.monitor.as_ref()),
        &resolved.user.user_id,
        &session_id,
        subagents::ParentSubagentListOptions {
            limit: query.limit,
            dispatch_id: query.dispatch_id,
            parent_turn_ref: query.parent_turn_ref,
            parent_user_round: query.parent_user_round,
            latest_turn_only: query.latest_turn_only.unwrap_or(false),
        },
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "session_id": session_id,
            "items": items,
        }
    })))
}

async fn control_session_subagents(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<SessionSubagentControlRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let result = subagents::control_parent_subagents(
        state.storage.as_ref(),
        Some(state.monitor.as_ref()),
        &resolved.user.user_id,
        &session_id,
        &payload.action,
        &payload.session_ids,
        payload.dispatch_id.as_deref(),
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": result })))
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Response, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    if payload.content.trim().is_empty()
        && !has_non_empty_chat_attachments(payload.attachments.as_deref())
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let orchestration_source = headers
        .get(ORCHESTRATION_SOURCE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .unwrap_or_default();
    let allow_orchestration_send = orchestration_source == ORCHESTRATION_SOURCE_ALLOW;
    let session_record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Some(record) = session_record.as_ref() {
        reject_or_repair_orchestration_dispatch(
            state.as_ref(),
            &resolved.user.user_id,
            record,
            allow_orchestration_send,
        )?;
    } else if !allow_orchestration_send {
        reject_locked_orchestration_session(state.as_ref(), &resolved.user.user_id, &session_id)?;
    }
    let request = build_chat_request(
        &state,
        &resolved.user,
        &session_id,
        payload.content,
        payload.stream.unwrap_or(true),
        payload.attachments,
        ChatRequestOverrides {
            tool_call_mode: payload.tool_call_mode,
            approval_mode: payload.approval_mode,
            debug_payload: payload.debug_payload,
        },
    )
    .await?;
    let wants_stream = request.stream;
    if wants_stream {
        return Err(orchestrator_error_response(
            StatusCode::BAD_REQUEST,
            json!({
                "code": "CHAT_WS_REQUIRED",
                "message": "chat streaming is available only through /wunder/chat/ws",
            }),
        ));
    }
    let outcome = state
        .kernel
        .thread_runtime
        .submit_user_request(request)
        .await
        .map_err(|err| {
            orchestrator_error_response(
                StatusCode::BAD_REQUEST,
                json!({"code": "INVALID_REQUEST", "message": err.to_string()}),
            )
        })?;

    match outcome {
        ThreadSubmitOutcome::Queued(info) => {
            let payload = json!({
                "queued": true,
                "queue_id": info.task_id,
                "thread_id": info.thread_id,
                "session_id": info.session_id,
                "queue_ahead": info.queue_ahead,
                "queue_total": info.queue_total,
            });
            Ok((StatusCode::ACCEPTED, Json(json!({ "data": payload }))).into_response())
        }
        ThreadSubmitOutcome::Run(request, lease) => {
            let request = *request;
            let _lease = lease;
            let user_id_for_goal = request.user_id.clone();
            let session_id_for_goal = request.session_id.clone();
            let response = state
                .kernel
                .orchestrator
                .run(request)
                .await
                .map_err(map_orchestrator_error)?;
            if response.stop_reason.as_deref() != Some("question_panel") {
                if let Some(session_id) = session_id_for_goal.as_deref() {
                    state
                        .kernel
                        .thread_runtime
                        .spawn_goal_continuation_after_cooldown(
                            user_id_for_goal,
                            session_id.to_string(),
                        );
                }
            }
            Ok(Json(json!({ "data": response })).into_response())
        }
    }
}

fn has_non_empty_chat_attachments(attachments: Option<&[ChatAttachment]>) -> bool {
    attachments
        .map(|items| {
            items.iter().any(|item| {
                item.content
                    .as_ref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
                    || item
                        .public_path
                        .as_ref()
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

pub(crate) async fn build_chat_request(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
    content: String,
    stream: bool,
    attachments: Option<Vec<ChatAttachment>>,
    request_overrides: ChatRequestOverrides,
) -> Result<WunderRequest, Response> {
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let content = content.trim().to_string();
    let has_attachments = has_non_empty_chat_attachments(attachments.as_deref());
    if content.is_empty() && !has_attachments {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }

    let now = now_ts();
    let record = state
        .user_store
        .get_chat_session(&user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let record = record.unwrap_or_else(|| crate::storage::ChatSessionRecord {
        session_id: session_id.clone(),
        user_id: user.user_id.clone(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        status: CHAT_SESSION_STATUS_ACTIVE.to_string(),
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id: None,
        tool_overrides: Vec::new(),
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    });
    state
        .user_store
        .upsert_chat_session(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let user_context = build_user_tool_context(state, &user.user_id).await;
    let agent_record = fetch_agent_record(state, user, record.agent_id.as_deref(), true).await?;
    let frozen_tool_overrides = state
        .workspace
        .load_session_frozen_tool_overrides_async(&user.user_id, &session_id)
        .await;
    let mut allowed = compute_allowed_tool_names(user, &user_context);
    let overrides = resolve_session_tool_overrides(
        &record,
        frozen_tool_overrides.as_deref(),
        agent_record.as_ref(),
    );
    if frozen_tool_overrides.is_none() {
        // Freeze the agent-default tool baseline on the first accepted user
        // message so later agent edits cannot silently drift an existing thread.
        state
            .workspace
            .save_session_frozen_tool_overrides(&user.user_id, &session_id, &overrides);
    }
    let agent_defaults = resolve_agent_tool_defaults(agent_record.as_ref());
    allowed = apply_tool_overrides(allowed, &overrides, &agent_defaults);
    let tool_names = finalize_tool_names(allowed);
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let preview_skill = agent_record
        .as_ref()
        .map(|record| record.preview_skill)
        .unwrap_or(false);

    let is_first_user_message = state
        .workspace
        .load_history_page(&user.user_id, &session_id, None, 2)
        .map(|items| {
            !items.iter().any(|item| {
                item.get("role")
                    .and_then(Value::as_str)
                    .map(|role| role == "user")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    if is_first_user_message && should_auto_title(&record.title) {
        if let Some(title) = build_session_title(&content) {
            let _ =
                state
                    .user_store
                    .update_chat_session_title(&user.user_id, &session_id, &title, now);
        }
    }
    let _ = state
        .user_store
        .touch_chat_session(&user.user_id, &session_id, now, now);

    let attachments = attachments
        .unwrap_or_default()
        .into_iter()
        .filter(|item| {
            item.content
                .as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
                || item
                    .public_path
                    .as_ref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
        })
        .map(|item| AttachmentPayload {
            name: item.name,
            content: item.content,
            content_type: item.mime_type,
            public_path: item.public_path,
        })
        .collect::<Vec<_>>();
    let attachments = if attachments.is_empty() {
        None
    } else {
        Some(attachments)
    };

    let tool_call_mode = normalize_tool_call_mode(request_overrides.tool_call_mode.as_deref())?;
    let request_approval_mode =
        normalize_optional_approval_mode(request_overrides.approval_mode.as_deref());
    let agent_approval_mode = agent_record
        .as_ref()
        .map(|record| normalize_agent_approval_mode(Some(record.approval_mode.as_str())));
    let resolved_approval_mode = request_approval_mode.or(agent_approval_mode);
    let config = state.config_store.get().await;
    let selected_model_name = resolve_chat_model_name(&config, agent_record.as_ref());
    let mut config_override_map = serde_json::Map::new();
    if let Some(mode) = tool_call_mode {
        if let Some(selected_model) = selected_model_name.as_deref() {
            config_override_map.insert(
                "llm".to_string(),
                json!({
                    "models": {
                        selected_model: {
                            "tool_call_mode": mode
                        }
                    }
                }),
            );
        }
    }
    if let Some(mode) = resolved_approval_mode {
        config_override_map.insert(
            "security".to_string(),
            json!({
                "approval_mode": mode
            }),
        );
    }
    let config_overrides = if config_override_map.is_empty() {
        None
    } else {
        Some(Value::Object(config_override_map))
    };

    Ok(WunderRequest {
        user_id: user.user_id.clone(),
        question: content,
        tool_names,
        skip_tool_calls: false,
        stream,
        debug_payload: request_overrides.debug_payload,
        session_id: Some(session_id),
        agent_id: record.agent_id.clone(),
        workspace_container_id: None,
        model_name: selected_model_name,
        language: Some(i18n::get_language()),
        config_overrides,
        agent_prompt,
        preview_skill,
        attachments,
        allow_queue: true,
        is_admin: UserStore::is_admin(user),
        approval_tx: None,
    })
}

fn normalize_tool_call_mode(raw: Option<&str>) -> Result<Option<String>, Response> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let normalized = raw.to_ascii_lowercase();
    if normalized == "tool_call" || normalized == "function_call" || normalized == "freeform_call" {
        return Ok(Some(normalized));
    }
    Err(error_response(
        StatusCode::BAD_REQUEST,
        "invalid tool_call_mode, expected tool_call/function_call/freeform_call".to_string(),
    ))
}

fn normalize_agent_approval_mode(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or("").trim().to_ascii_lowercase();
    match cleaned.as_str() {
        "suggest" => "suggest".to_string(),
        "auto_edit" | "auto-edit" => "auto_edit".to_string(),
        "full_auto" | "full-auto" => "full_auto".to_string(),
        _ => "full_auto".to_string(),
    }
}

fn normalize_optional_approval_mode(raw: Option<&str>) -> Option<String> {
    let cleaned = raw.map(str::trim).filter(|value| !value.is_empty())?;
    Some(normalize_agent_approval_mode(Some(cleaned)))
}

async fn cancel_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let _record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let goal_cleared = crate::services::goal::clear_goal(
        state.storage.clone(),
        &resolved.user.user_id,
        &session_id,
    )
    .await
    .unwrap_or(false);
    let cancelled = state.monitor.cancel_with_source(&session_id, "rest_cancel");
    let marker_persisted = persist_user_cancelled_turn_marker(
        state.workspace.clone(),
        state.user_store.clone(),
        &resolved.user.user_id,
        &session_id,
        "rest_cancel",
    )
    .await
    .unwrap_or(false);
    Ok(Json(
        json!({ "data": { "cancelled": cancelled, "goal_cleared": goal_cleared, "marker_persisted": marker_persisted } }),
    ))
}

async fn compact_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<SessionCompactionRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let session_record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;

    let monitor_status = state.monitor.get_record(&session_id).and_then(|record| {
        record
            .get("status")
            .and_then(Value::as_str)
            .map(ToString::to_string)
    });
    if is_session_stream_active_or_queued(&state.user_store, monitor_status.as_deref(), &session_id)
    {
        return Err(error_response(
            StatusCode::CONFLICT,
            i18n::t("error.session_not_found_or_running"),
        ));
    }

    let agent_id = session_record.agent_id.clone();
    let agent_record =
        fetch_agent_record(&state, &resolved.user, agent_id.as_deref(), true).await?;
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let preview_skill = agent_record
        .as_ref()
        .map(|record| record.preview_skill)
        .unwrap_or(false);
    let user_id = resolved.user.user_id.clone();
    let is_admin = UserStore::is_admin(&resolved.user);
    let manual_user_round = state.monitor.register(
        &session_id,
        &user_id,
        agent_id.as_deref().unwrap_or(""),
        "",
        is_admin,
        payload.debug_payload,
    );
    let orchestrator = state.kernel.orchestrator.clone();
    let session_id_for_task = session_id.clone();
    let user_id_for_task = user_id.clone();
    let model_name = payload.model_name.clone();
    let agent_id_for_task = agent_id.clone();
    let agent_prompt_for_task = agent_prompt.clone();
    let debug_payload = payload.debug_payload;
    tokio::spawn(async move {
        let result = orchestrator
            .force_compact_session(
                &user_id_for_task,
                &session_id_for_task,
                is_admin,
                model_name.as_deref(),
                agent_id_for_task.as_deref(),
                agent_prompt_for_task.as_deref(),
                Some(preview_skill),
                Some(manual_user_round),
                debug_payload,
                true,
            )
            .await;
        if let Err(err) = result {
            warn!("manual compaction turn failed for session {session_id_for_task}: {err}");
        }
    });
    Ok(Json(json!({
        "data": {
            "accepted": true,
            "running": true,
            "user_round": manual_user_round,
            "session_id": session_id,
        }
    })))
}

async fn update_session_tools(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<SessionToolsUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let record = state
        .user_store
        .get_chat_session(&resolved.user.user_id, &session_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut record = record
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let user_context = build_user_tool_context(&state, &resolved.user.user_id).await;
    let allowed = compute_allowed_tool_names(&resolved.user, &user_context);
    let overrides =
        filter_tool_overrides(normalize_tool_overrides(payload.tool_overrides), &allowed);
    record.tool_overrides = overrides;
    record.updated_at = now_ts();
    state
        .user_store
        .upsert_chat_session(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    Ok(Json(json!({
        "data": {
            "id": session_id,
            "tool_overrides": record.tool_overrides,
        }
    })))
}

pub(crate) fn reject_locked_orchestration_session(
    state: &AppState,
    user_id: &str,
    session_id: &str,
) -> Result<(), Response> {
    if let Some((lock_state, lock_binding)) =
        session_orchestration_lock_info(state.storage.as_ref(), user_id, session_id)
    {
        return Err(crate::api::errors::error_response_with_detail(
            StatusCode::CONFLICT,
            Some(ORCHESTRATION_THREAD_LOCKED_CODE),
            build_locked_thread_message(&lock_state, &lock_binding),
            Some("Use the orchestration page to continue this orchestration thread."),
            Some(json!({
                "group_id": lock_state.group_id,
                "orchestration_id": lock_state.orchestration_id,
                "run_id": lock_state.run_id,
                "session_id": lock_binding.session_id,
                "agent_id": lock_binding.agent_id,
                "role": lock_binding.role,
            })),
        ));
    }
    Ok(())
}

pub(crate) fn reject_or_repair_orchestration_dispatch(
    state: &AppState,
    user_id: &str,
    session: &crate::storage::ChatSessionRecord,
    allow_orchestration_send: bool,
) -> Result<(), Response> {
    let session_id = session.session_id.trim();
    let agent_id = session.agent_id.as_deref().unwrap_or("").trim();
    if agent_id.is_empty() {
        if !allow_orchestration_send {
            reject_locked_orchestration_session(state, user_id, session_id)?;
        }
        return Ok(());
    }
    if let Some((lock_state, lock_binding)) =
        active_orchestration_for_agent(state.storage.as_ref(), user_id, agent_id)
    {
        if lock_binding.session_id.trim() != session_id {
            return Err(crate::api::errors::error_response_with_detail(
                StatusCode::CONFLICT,
                Some(ORCHESTRATION_THREAD_LOCKED_CODE),
                build_locked_thread_message(&lock_state, &lock_binding),
                Some("Use the orchestration page to continue this orchestration thread."),
                Some(json!({
                    "group_id": lock_state.group_id,
                    "orchestration_id": lock_state.orchestration_id,
                    "run_id": lock_state.run_id,
                    "session_id": lock_binding.session_id,
                    "agent_id": lock_binding.agent_id,
                    "role": lock_binding.role,
                })),
            ));
        }
        if allow_orchestration_send {
            let round_index = crate::services::orchestration_context::load_session_context(
                state.storage.as_ref(),
                user_id,
                session_id,
            )
            .map(|context| context.round_index)
            .unwrap_or(1);
            let _ = repair_orchestration_session_main_thread(
                state.storage.as_ref(),
                user_id,
                session_id,
                round_index,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            return Ok(());
        }
    }
    if !allow_orchestration_send {
        reject_locked_orchestration_session(state, user_id, session_id)?;
    }
    Ok(())
}

pub(super) fn resolve_chat_model_name(
    config: &crate::config::Config,
    agent_record: Option<&crate::storage::UserAgentRecord>,
) -> Option<String> {
    if let Some(name) =
        agent_record.and_then(|record| normalize_optional_model_name(record.model_name.as_deref()))
    {
        if config
            .llm
            .models
            .get(&name)
            .is_some_and(crate::services::llm::is_llm_model)
        {
            return Some(name);
        }
    }
    resolve_default_model_key(config)
}

fn resolve_default_model_key(config: &crate::config::Config) -> Option<String> {
    let default_key = config.llm.default.trim();
    if !default_key.is_empty() && config.llm.models.get(default_key).is_some_and(is_llm_model) {
        return Some(default_key.to_string());
    }
    for (key, cfg) in config.llm.models.iter() {
        if !is_llm_model(cfg) {
            continue;
        }
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn normalize_optional_model_name(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn finalize_tool_names(mut allowed: HashSet<String>) -> Vec<String> {
    if allowed.is_empty() {
        return vec!["__no_tools__".to_string()];
    }
    let mut list = allowed.drain().collect::<Vec<_>>();
    list.sort();
    list
}

pub(super) async fn fetch_agent_record(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    agent_id: Option<&str>,
    allow_missing: bool,
) -> Result<Option<crate::storage::UserAgentRecord>, Response> {
    let normalized_agent_id = agent_id.map(str::trim).filter(|value| !value.is_empty());
    if normalized_agent_id.is_none() || is_default_agent_alias(normalized_agent_id) {
        let record = crate::user_store::build_default_agent_record_from_storage(
            state.storage.as_ref(),
            &user.user_id,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        return Ok(Some(record));
    }
    let Some(agent_id) = normalized_agent_id else {
        return Ok(None);
    };
    let record = state
        .user_store
        .get_user_agent_by_id(agent_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(record) = record else {
        if allow_missing {
            return Ok(None);
        }
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.agent_not_found"),
        ));
    };
    let access = state
        .user_store
        .get_user_agent_access(&user.user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !is_agent_allowed(user, access.as_ref(), &record) {
        if allow_missing {
            return Ok(None);
        }
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.agent_not_found"),
        ));
    }
    Ok(Some(record))
}

pub(super) fn resolve_session_tool_overrides(
    record: &crate::storage::ChatSessionRecord,
    frozen_tool_overrides: Option<&[String]>,
    agent: Option<&crate::storage::UserAgentRecord>,
) -> Vec<String> {
    if !record.tool_overrides.is_empty() {
        normalize_tool_overrides(record.tool_overrides.clone())
    } else if let Some(snapshot) = frozen_tool_overrides {
        normalize_tool_overrides(snapshot.to_vec())
    } else {
        resolve_agent_tool_defaults(agent)
    }
}

pub(super) fn resolve_agent_tool_defaults(
    agent: Option<&crate::storage::UserAgentRecord>,
) -> Vec<String> {
    let Some(record) = agent else {
        return Vec::new();
    };
    resolve_agent_runtime_tool_names(
        &record.tool_names,
        &record.declared_tool_names,
        &record.declared_skill_names,
    )
}

pub(super) fn resolve_agent_workspace_id(
    state: &AppState,
    user_id: &str,
    agent_id: Option<&str>,
    agent_record: Option<&crate::storage::UserAgentRecord>,
) -> String {
    if let Some(record) = agent_record {
        return state
            .workspace
            .scoped_user_id_by_container(user_id, record.sandbox_container_id);
    }
    if is_default_agent_alias(agent_id) || agent_id.is_none() {
        if let Ok(record) = crate::user_store::build_default_agent_record_from_storage(
            state.storage.as_ref(),
            user_id,
        ) {
            return state
                .workspace
                .scoped_user_id_by_container(user_id, record.sandbox_container_id);
        }
        return state
            .workspace
            .scoped_user_id_by_container(user_id, state.user_store.default_sandbox_container_id());
    }
    if let Some(container_id) = state
        .user_store
        .resolve_agent_sandbox_container_id(agent_id)
    {
        return state
            .workspace
            .scoped_user_id_by_container(user_id, container_id);
    }
    state.workspace.scoped_user_id(user_id, agent_id)
}

fn is_default_agent_alias(agent_id: Option<&str>) -> bool {
    let Some(cleaned) = agent_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    cleaned.eq_ignore_ascii_case("__default__") || cleaned.eq_ignore_ascii_case("default")
}

pub(super) fn normalize_tool_overrides(values: Vec<String>) -> Vec<String> {
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

fn filter_tool_overrides(values: Vec<String>, allowed: &HashSet<String>) -> Vec<String> {
    if values.iter().any(|name| name == TOOL_OVERRIDE_NONE) {
        return vec![TOOL_OVERRIDE_NONE.to_string()];
    }
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw in values {
        if let Some(mapped) = resolve_override_name_with_allowed(&raw, allowed) {
            if seen.insert(mapped.clone()) {
                output.push(mapped);
            }
        }
    }
    output
}

pub(super) fn apply_tool_overrides(
    allowed: HashSet<String>,
    overrides: &[String],
    agent_defaults: &[String],
) -> HashSet<String> {
    if overrides.is_empty() {
        return allowed;
    }
    if overrides.iter().any(|name| name == TOOL_OVERRIDE_NONE) {
        return HashSet::new();
    }
    let scoped_defaults: HashSet<String> = agent_defaults
        .iter()
        .map(String::as_str)
        .filter_map(|name| resolve_override_name_with_allowed(name, &allowed))
        .collect();
    let mut filtered = HashSet::new();
    for raw in overrides {
        if let Some(mapped) = resolve_override_name_with_allowed(raw, &allowed) {
            if !scoped_defaults.is_empty() && !scoped_defaults.contains(&mapped) {
                continue;
            }
            filtered.insert(mapped);
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

fn should_auto_title(title: &str) -> bool {
    let cleaned = title.trim();
    cleaned.is_empty() || cleaned == "新会话" || cleaned == "未命名会话"
}

fn build_session_title(content: &str) -> Option<String> {
    let cleaned = content.trim().replace('\n', " ");
    if cleaned.is_empty() {
        return None;
    }
    let mut output = cleaned;
    if output.chars().count() > 20 {
        output = output.chars().take(20).collect::<String>();
        output.push_str("...");
    }
    Some(output)
}

fn format_ts(ts: f64) -> String {
    let millis = (ts * 1000.0) as i64;
    DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|dt| dt.with_timezone(&Local).to_rfc3339())
        .unwrap_or_default()
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn map_orchestrator_error(err: Error) -> Response {
    if let Some(orchestrator_err) = err.downcast_ref::<OrchestratorError>() {
        let status = crate::api::errors::status_for_error_code(orchestrator_err.code());
        return orchestrator_error_response(status, orchestrator_err.to_payload());
    }
    orchestrator_error_response(
        StatusCode::BAD_REQUEST,
        json!({
            "code": "INTERNAL_ERROR",
            "message": err.to_string(),
        }),
    )
}

fn orchestrator_error_response(status: StatusCode, payload: Value) -> Response {
    let code = payload
        .get("code")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("request failed")
        .to_string();
    let hint = code
        .as_deref()
        .and_then(crate::api::errors::hint_for_error_code);
    crate::api::errors::error_response_with_detail(
        status,
        code.as_deref(),
        message,
        hint,
        Some(payload),
    )
}

pub(super) fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

#[cfg(test)]
mod tests {
    use super::{has_non_empty_chat_attachments, ChatAttachment};

    #[test]
    fn has_non_empty_chat_attachments_accepts_public_path_only_attachment() {
        let attachments = vec![ChatAttachment {
            name: Some("heart.png".to_string()),
            content: Some("   ".to_string()),
            mime_type: Some("image/png".to_string()),
            public_path: Some("users/u1/heart.png".to_string()),
        }];
        assert!(has_non_empty_chat_attachments(Some(&attachments)));
    }

    #[test]
    fn has_non_empty_chat_attachments_rejects_blank_attachment_entries() {
        let attachments = vec![ChatAttachment {
            name: Some("blank.txt".to_string()),
            content: Some("   ".to_string()),
            mime_type: Some("text/plain".to_string()),
            public_path: Some("   ".to_string()),
        }];
        assert!(!has_non_empty_chat_attachments(Some(&attachments)));
    }
}
