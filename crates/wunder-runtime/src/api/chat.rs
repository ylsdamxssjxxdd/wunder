use crate::api::user_context::resolve_user;
use crate::core::long_task;
use crate::i18n;
use crate::orchestrator::OrchestratorError;
use crate::schemas::{AttachmentPayload, WunderRequest};
pub(crate) use crate::services::agent_execution::{
    apply_tool_overrides, finalize_tool_names, normalize_tool_overrides,
    resolve_agent_tool_defaults, resolve_chat_model_name, resolve_override_name_with_allowed,
    resolve_session_tool_overrides,
};
use crate::services::llm::normalize_reasoning_effort;
use crate::services::runtime::thread::ThreadSubmitOutcome;
use crate::services::subagents;
use crate::services::user_agent_presets::normalize_agent_approval_mode;
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

mod events;
mod media;
mod prompt;
mod queue;
mod sessions;
mod stress;

use sessions::{has_active_queue_task, is_session_stream_active_or_queued};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(events::router())
        .merge(media::router())
        .merge(prompt::router())
        .merge(queue::router())
        .merge(sessions::router())
        .merge(stress::router())
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
    #[serde(default, alias = "clientMessageId")]
    client_message_id: Option<String>,
    #[serde(default)]
    stream: Option<bool>,
    // Deprecated compatibility field: parsed for older clients but ignored;
    // request logging is unified to the compact profile.
    #[serde(default, alias = "debugPayload", alias = "debug_payload")]
    #[allow(dead_code)]
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
    #[serde(default, alias = "reasoningEffort", alias = "reasoning_effort")]
    reasoning_effort: Option<String>,
}

pub(crate) struct ChatRequestOverrides {
    pub(crate) tool_call_mode: Option<String>,
    pub(crate) approval_mode: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
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
    client_message_id: Option<String>,
    #[serde(default)]
    model_name: Option<String>,
    // Deprecated compatibility field: parsed for older clients but ignored;
    // compaction logging is unified to the compact profile.
    #[serde(default, alias = "debugPayload", alias = "debug_payload")]
    #[allow(dead_code)]
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
    let request = build_chat_request(
        &state,
        &resolved.user,
        &session_id,
        payload.content,
        payload.client_message_id,
        payload.stream.unwrap_or(true),
        payload.attachments,
        ChatRequestOverrides {
            tool_call_mode: payload.tool_call_mode,
            approval_mode: payload.approval_mode,
            reasoning_effort: payload.reasoning_effort,
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
                "active_ahead": info.active_ahead,
                "wait_ahead": info.wait_ahead,
                "queue_change_seq": info.queue_change_seq,
                "queue_after_change_seq": info.queue_after_change_seq,
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
                        .orchestrator
                        .goal_handle()
                        .driver()
                        .notify_turn_ended(
                            &user_id_for_goal,
                            session_id,
                            None,
                            crate::services::goal::TurnEndOutcome::Completed,
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
    client_message_id: Option<String>,
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
        workspace_id: None,
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

    // Desktop threads carry a workspace binding; its real host folder becomes
    // the tool root for this request (read/write/executed in place).
    let mut request_workspace_id = record.workspace_id.clone();
    if let Some(workspace_id) = request_workspace_id.as_deref().map(str::trim) {
        match state.storage.get_workspace(&user.user_id, workspace_id) {
            Ok(Some(workspace)) => {
                state
                    .workspace
                    .register_workspace_root(workspace_id, &workspace.root_path);
            }
            Ok(None) => {
                // The workspace was deleted while threads still point at it.
                request_workspace_id = None;
            }
            Err(err) => {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    format!("{}: {err}", i18n::t("workspace.root_missing")),
                ));
            }
        }
    }

    let user_context = build_user_tool_context(state, &user.user_id).await;
    let agent_record = fetch_agent_record(state, user, record.agent_id.as_deref(), true).await?;
    let mut allowed = compute_allowed_tool_names(user, &user_context);
    let overrides = state
        .kernel
        .orchestrator
        .resolve_frozen_session_tool_overrides(&record, agent_record.as_ref())
        .await;
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

    // ThreadLog owns the durable user-turn directory. The admission
    // transaction, rather than a message projection, defines first use.
    let is_first_user_message = state
        .storage
        .get_thread_log_counts(&user.user_id, &session_id, false)
        .map(|(user_turn_total, _item_total)| user_turn_total == 0)
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
    let request_reasoning_effort =
        normalize_reasoning_effort(request_overrides.reasoning_effort.as_deref()).or_else(|| {
            normalize_reasoning_effort(Some(
                &state
                    .workspace
                    .load_session_reasoning_effort(&user.user_id, &session_id),
            ))
        });
    let agent_approval_mode = agent_record
        .as_ref()
        .map(|record| normalize_agent_approval_mode(Some(record.approval_mode.as_str())));
    let resolved_approval_mode = request_approval_mode.or(agent_approval_mode);
    let config = state.config_store.get().await;
    let selected_model_name = resolve_chat_model_name(&config, agent_record.as_ref());
    let mut config_override_map = serde_json::Map::new();
    let mut selected_model_overrides = serde_json::Map::new();
    if let Some(mode) = tool_call_mode {
        selected_model_overrides.insert("tool_call_mode".to_string(), json!(mode));
    }
    if let Some(mode) = resolved_approval_mode {
        config_override_map.insert(
            "security".to_string(),
            json!({
                "approval_mode": mode
            }),
        );
    }
    if let Some(effort) = request_reasoning_effort {
        selected_model_overrides.insert("reasoning_effort".to_string(), json!(effort));
    }
    if let Some(selected_model) = selected_model_name.as_deref() {
        if !selected_model_overrides.is_empty() {
            let mut models = serde_json::Map::new();
            models.insert(
                selected_model.to_string(),
                Value::Object(selected_model_overrides),
            );
            config_override_map.insert(
                "llm".to_string(),
                Value::Object({
                    let mut llm = serde_json::Map::new();
                    llm.insert("models".to_string(), Value::Object(models));
                    llm
                }),
            );
        }
    }
    let mut config_overrides = if config_override_map.is_empty() {
        None
    } else {
        Some(Value::Object(config_override_map))
    };
    // Host-attested human origin: every build_chat_request caller is a direct
    // human chat surface (HTTP, WebSocket, embedded native). Scheduled and
    // model-initiated deliveries go through runtime.build_request instead, so
    // goal tool authority can trust this stamp.
    crate::services::goal::mark_human_source(&mut config_overrides);

    Ok(WunderRequest {
        user_id: user.user_id.clone(),
        question: content,
        client_message_id: normalize_optional_client_message_id(client_message_id.as_deref()),
        tool_names,
        skip_tool_calls: false,
        stream,
        session_id: Some(session_id),
        agent_id: record.agent_id.clone(),
        workspace_container_id: None,
        workspace_id: request_workspace_id,
        model_name: selected_model_name,
        language: Some(i18n::get_language()),
        config_overrides,
        agent_prompt,
        preview_skill,
        attachments,
        allow_queue: true,
        is_admin: UserStore::is_admin(user),
        enforce_runtime_queue: true,
        approval_tx: None,
    })
}

/// Resolve a native thread's capacity using the same agent/model policy as chat.
pub async fn native_chat_context_capacity(
    state: &Arc<AppState>,
    user_id: &str,
    session_id: &str,
) -> anyhow::Result<Option<u32>> {
    let db = state.clone();
    let owner = user_id.to_owned();
    let target = session_id.to_owned();
    let agent = crate::blocking::run_db("native.chat.context_model", move || {
        let session = db
            .user_store
            .get_chat_session(&owner, &target)?
            .ok_or_else(|| anyhow::anyhow!("chat session not found"))?;
        match session
            .agent_id
            .as_deref()
            .filter(|id| !id.is_empty() && *id != "__default__")
        {
            Some(id) => db.user_store.get_user_agent_by_id(id),
            None => Ok(Some(
                crate::user_store::build_default_agent_record_from_storage(
                    db.storage.as_ref(),
                    &owner,
                )?,
            )),
        }
    })
    .await?;
    let config = state.config_store.get().await;
    Ok(resolve_chat_model_name(&config, agent.as_ref())
        .and_then(|key| config.llm.models.get(&key))
        .and_then(|model| model.max_context)
        .filter(|value| *value > 0))
}

/// Build a local desktop request without going through the HTTP adapter.
///
/// The transport handlers still own authentication and response shaping; the
/// embedded desktop supplies its already-resolved local user directly.
pub async fn build_native_chat_request(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
    content: String,
    client_message_id: Option<String>,
    attachments: Vec<AttachmentPayload>,
    reasoning_effort: Option<String>,
) -> anyhow::Result<WunderRequest> {
    build_chat_request(
        state,
        user,
        session_id,
        content,
        client_message_id,
        true,
        Some(
            attachments
                .into_iter()
                .map(|attachment| ChatAttachment {
                    name: attachment.name,
                    content: attachment.content,
                    mime_type: attachment.content_type,
                    public_path: attachment.public_path,
                })
                .collect(),
        ),
        ChatRequestOverrides {
            tool_call_mode: None,
            approval_mode: None,
            reasoning_effort,
        },
    )
    .await
    .map_err(|_| anyhow::anyhow!("chat request was rejected"))
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
    let goal_cleared = state
        .kernel
        .orchestrator
        .goal_handle()
        .clear(state.storage.clone(), &resolved.user.user_id, &session_id)
        .await
        .is_ok();
    let cancel_settlement = state
        .kernel
        .thread_runtime
        .cancel_session_activity(&resolved.user.user_id, &session_id, "rest_cancel")
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "cancelled": cancel_settlement.monitor_cancelled,
            "child_sessions_cancelled": cancel_settlement.child_sessions_cancelled,
            "goal_cleared": goal_cleared,
            "queued_tasks_cancelled": cancel_settlement.queued_tasks_cancelled,
            "running_tasks_marked_cancelled": cancel_settlement.running_tasks_marked_cancelled,
            "thread_status_reset": cancel_settlement.thread_status_reset,
            "settlement_event_id": cancel_settlement.settlement_event_id,
        }
    })))
}

fn normalize_optional_client_message_id(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(128).collect::<String>())
}

async fn compact_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<SessionCompactionRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    submit_session_compaction(&state, &resolved.user, &session_id, payload).await
}

/// Native and HTTP clients share acceptance, scheduling and durable compaction records.
pub async fn compact_native_session(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
) -> anyhow::Result<Value> {
    submit_session_compaction(
        state,
        user,
        session_id,
        SessionCompactionRequest {
            client_message_id: None,
            model_name: None,
            debug_payload: false,
        },
    )
    .await
    .map(|Json(value)| value)
    .map_err_response()
    .await
}

pub(super) trait NativeResponseResult<T> {
    async fn map_err_response(self) -> anyhow::Result<T>;
}
impl<T> NativeResponseResult<T> for Result<T, Response> {
    async fn map_err_response(self) -> anyhow::Result<T> {
        match self {
            Ok(value) => Ok(value),
            Err(response) => {
                let status = response.status();
                let body = axum::body::to_bytes(response.into_body(), 16384).await?;
                let data: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let message = data["error"]["message"]
                    .as_str()
                    .or_else(|| data["message"].as_str())
                    .unwrap_or("命令执行失败");
                Err(anyhow::anyhow!("{message} ({status})"))
            }
        }
    }
}

async fn submit_session_compaction(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
    payload: SessionCompactionRequest,
) -> Result<Json<Value>, Response> {
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let session_record = state
        .user_store
        .get_chat_session(&user.user_id, &session_id)
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
    let agent_record = fetch_agent_record(state, user, agent_id.as_deref(), true).await?;
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let preview_skill = agent_record
        .as_ref()
        .map(|record| record.preview_skill)
        .unwrap_or(false);
    let user_id = user.user_id.clone();
    let is_admin = UserStore::is_admin(user);
    let accepted = state
        .kernel
        .orchestrator
        .committer
        .accept_turn(
            &user_id,
            &session_id,
            &json!({
                "role": "user", "content": "/compact",
                "client_message_id": payload.client_message_id,
                "meta": {"type": "manual_compaction_command", "manual_compaction": true}
            }),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let manual_user_round = accepted["user_turn_index"]
        .as_i64()
        .expect("accepted round");
    state.monitor.register_continuation(
        &session_id,
        &user_id,
        agent_id.as_deref().unwrap_or(""),
        "/compact",
        is_admin,
        manual_user_round,
    );
    let orchestrator = state.kernel.orchestrator.clone();
    let session_id_for_task = session_id.clone();
    let user_id_for_task = user_id.clone();
    let model_name = payload.model_name.clone();
    let agent_id_for_task = agent_id.clone();
    let agent_prompt_for_task = agent_prompt.clone();
    long_task::spawn("api.chat.force_compact_session", async move {
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
            "turn_id": accepted["turn_id"],
            "client_message_id": payload.client_message_id,
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

/// Resolve one agent record for a chat request, honoring the user's agent
/// access list. `allow_missing` keeps read paths tolerant (a deleted agent must
/// not break history) while write paths still reject.
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
