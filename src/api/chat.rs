use crate::api::attachment_convert::{build_conversion_payload, convert_multipart_list};
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::monitor::MonitorState;
use crate::orchestrator::OrchestratorError;
use crate::orchestrator_constants::{
    OBSERVATION_PREFIX, STREAM_EVENT_FETCH_LIMIT, STREAM_EVENT_QUEUE_SIZE,
    STREAM_EVENT_RESUME_POLL_INTERVAL_S,
};
use crate::schemas::{AttachmentPayload, StreamEvent, WunderRequest};
use crate::services::agent_runtime::AgentSubmitOutcome;
use crate::state::AppState;
use crate::user_access::{build_user_tool_context, compute_allowed_tool_names, is_agent_allowed};
use crate::user_store::UserStore;
use anyhow::Error;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tracing::warn;
use uuid::Uuid;

const DEFAULT_SESSION_TITLE: &str = "新会话";
const DEFAULT_MESSAGE_LIMIT: i64 = 500;
const TOOL_OVERRIDE_NONE: &str = "__no_tools__";
const MAX_ATTACHMENT_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/chat/sessions",
            post(create_session).get(list_sessions),
        )
        .route(
            "/wunder/chat/sessions/{session_id}",
            get(get_session).delete(delete_session),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/events",
            get(get_session_events),
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
            "/wunder/chat/sessions/{session_id}/resume",
            get(resume_session),
        )
        .route(
            "/wunder/chat/sessions/{session_id}/cancel",
            post(cancel_session),
        )
        .route("/wunder/chat/system-prompt", post(system_prompt))
        .route(
            "/wunder/chat/sessions/{session_id}/system-prompt",
            post(session_system_prompt),
        )
        .route(
            "/wunder/chat/attachments/convert",
            post(chat_attachment_convert).layer(DefaultBodyLimit::max(MAX_ATTACHMENT_UPLOAD_BYTES)),
        )
}

#[derive(Debug, Deserialize)]
struct SessionListQuery {
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    page_size: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(
        default,
        alias = "parent_id",
        alias = "parentId",
        alias = "parentSessionId"
    )]
    parent_session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SendMessageRequest {
    content: String,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    attachments: Option<Vec<ChatAttachment>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatAttachment {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SystemPromptRequest {
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    tool_overrides: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SessionDetailQuery {
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ResumeQuery {
    #[serde(default)]
    after_event_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SessionToolsUpdateRequest {
    #[serde(default)]
    tool_overrides: Vec<String>,
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateSessionRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let now = now_ts();
    let session_id = format!("sess_{}", Uuid::new_v4().simple());
    let title = payload
        .title
        .unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string())
        .trim()
        .to_string();
    let agent_id = payload
        .agent_id
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let agent_record =
        fetch_agent_record(&state, &resolved.user, agent_id.as_deref(), false).await?;
    let tool_overrides = agent_record
        .as_ref()
        .map(|record| record.tool_names.clone())
        .unwrap_or_default();
    let record = crate::storage::ChatSessionRecord {
        session_id: session_id.clone(),
        user_id: resolved.user.user_id.clone(),
        title: if title.is_empty() {
            DEFAULT_SESSION_TITLE.to_string()
        } else {
            title
        },
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id,
        tool_overrides,
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    };
    state
        .user_store
        .upsert_chat_session(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let is_main = state
        .agent_runtime
        .set_main_session(
            &resolved.user.user_id,
            record.agent_id.as_deref().unwrap_or(""),
            &session_id,
            "create",
        )
        .await
        .is_ok();
    Ok(Json(
        json!({ "data": session_payload_with_main(&record, is_main) }),
    ))
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<SessionListQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let (offset, limit) = resolve_pagination(&query);
    let agent_id = query.agent_id.as_deref().map(|value| value.trim());
    let parent_session_id = query.parent_session_id.as_deref().map(|value| value.trim());
    let (sessions, total) = state
        .user_store
        .list_chat_sessions(
            &resolved.user.user_id,
            agent_id,
            parent_session_id,
            offset,
            limit,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut main_map: HashMap<String, Option<String>> = HashMap::new();
    for record in &sessions {
        let agent_key = record.agent_id.clone().unwrap_or_default();
        if main_map.contains_key(&agent_key) {
            continue;
        }
        let main = state
            .user_store
            .get_agent_thread(&resolved.user.user_id, &agent_key)
            .ok()
            .flatten()
            .map(|item| item.session_id);
        main_map.insert(agent_key, main);
    }
    let items = sessions
        .iter()
        .map(|record| {
            let agent_key = record.agent_id.clone().unwrap_or_default();
            let is_main = main_map
                .get(&agent_key)
                .and_then(|value| value.as_ref())
                .map(|session_id| session_id == &record.session_id)
                .unwrap_or(false);
            session_payload_with_main(record, is_main)
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "total": total, "items": items } })))
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<SessionDetailQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let is_admin = UserStore::is_admin(&resolved.user);
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
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;

    let limit = query
        .limit
        .unwrap_or_else(|| if is_admin { 0 } else { DEFAULT_MESSAGE_LIMIT });
    let mut history_incomplete = false;
    let history = match state
        .workspace
        .load_history(&resolved.user.user_id, &session_id, limit)
    {
        Ok(items) => items,
        Err(err) => {
            warn!(
                "load history failed: user_id={}, session_id={}, error={err}",
                resolved.user.user_id, session_id
            );
            history_incomplete = true;
            Vec::new()
        }
    };
    let session_status = state.monitor.get_record(&session_id).and_then(|record| {
        record
            .get("status")
            .and_then(Value::as_str)
            .map(|value| value.to_string())
    });
    let session_running = session_status
        .as_deref()
        .map(|status| {
            status == crate::monitor::MonitorState::STATUS_RUNNING
                || status == crate::monitor::MonitorState::STATUS_CANCELLING
        })
        .unwrap_or(false);
    let filtered_history = filter_history_messages(history, session_running);
    let mut messages = filtered_history
        .into_iter()
        .filter_map(map_history_message)
        .collect::<Vec<_>>();

    if session_running {
        let last_role = messages
            .last()
            .and_then(|item| item.get("role").and_then(Value::as_str))
            .unwrap_or("");
        if last_role == "user" || messages.is_empty() {
            messages.push(json!({
                "role": "assistant",
                "content": "",
                "created_at": format_ts(now_ts()),
                "stream_incomplete": true,
            }));
        } else if let Some(target) = messages.iter_mut().rev().find(|item| {
            item.get("role")
                .and_then(Value::as_str)
                .map(|value| value == "assistant")
                .unwrap_or(false)
        }) {
            if let Value::Object(ref mut map) = target {
                map.insert("stream_incomplete".to_string(), json!(true));
            }
        }
    }

    Ok(Json(json!({
        "data": {
            "id": record.session_id,
            "title": record.title,
            "created_at": format_ts(record.created_at),
            "updated_at": format_ts(record.updated_at),
            "last_message_at": format_ts(record.last_message_at),
            "agent_id": record.agent_id,
            "tool_overrides": record.tool_overrides,
            "history_incomplete": history_incomplete,
            "messages": messages
        }
    })))
}

async fn get_session_events(
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
    let rounds = state
        .monitor
        .get_record(&session_id)
        .map(|record| collect_session_event_rounds(&record))
        .unwrap_or_default();
    let running = state
        .monitor
        .get_record(&session_id)
        .map(|record| {
            record
                .get("status")
                .and_then(Value::as_str)
                .map(|status| {
                    status == MonitorState::STATUS_RUNNING
                        || status == MonitorState::STATUS_CANCELLING
                })
                .unwrap_or(false)
        })
        .unwrap_or(false);
    let last_event_id = {
        let storage = state.storage.clone();
        let session_id = session_id.clone();
        tokio::task::spawn_blocking(move || storage.get_max_stream_event_id(&session_id))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or(0)
    };
    Ok(Json(json!({
        "data": {
            "id": session_id,
            "rounds": rounds,
            "running": running,
            "last_event_id": last_event_id
        }
    })))
}

async fn delete_session(
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
    state
        .workspace
        .purge_session_data(&resolved.user.user_id, &session_id);
    let _ = state
        .storage
        .delete_cron_jobs_by_session(&resolved.user.user_id, &session_id);
    let _ = state
        .memory
        .delete_record(&resolved.user.user_id, &session_id);
    let _ = state.monitor.purge_session(&session_id);
    let _ = state
        .user_store
        .delete_chat_session(&resolved.user.user_id, &session_id);
    Ok(Json(json!({ "data": { "id": session_id } })))
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
    if payload.content.trim().is_empty() {
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
        payload.stream.unwrap_or(true),
        payload.attachments,
    )
    .await?;
    let wants_stream = request.stream;
    let outcome = state
        .agent_runtime
        .submit_user_request(request)
        .await
        .map_err(|err| {
            orchestrator_error_response(
                StatusCode::BAD_REQUEST,
                json!({"code": "INVALID_REQUEST", "message": err.to_string()}),
            )
        })?;

    match outcome {
        AgentSubmitOutcome::Queued(info) => {
            let payload = json!({
                "queued": true,
                "queue_id": info.task_id,
                "thread_id": info.thread_id,
                "session_id": info.session_id,
            });
            if wants_stream {
                let mapped = tokio_stream::iter(vec![Ok::<Event, std::convert::Infallible>(
                    Event::default().event("queued").data(payload.to_string()),
                )]);
                let sse = Sse::new(mapped)
                    .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)));
                Ok(sse.into_response())
            } else {
                Ok((StatusCode::ACCEPTED, Json(json!({ "data": payload }))).into_response())
            }
        }
        AgentSubmitOutcome::Run(request, lease) => {
            if request.stream {
                let stream = state
                    .orchestrator
                    .stream(request)
                    .await
                    .map_err(map_orchestrator_error)?;
                let lease_guard = lease;
                let mapped = stream.map(move |event| {
                    let _keep = &lease_guard;
                    match event {
                        Ok(event) => {
                            let mut builder = Event::default()
                                .event(event.event)
                                .data(event.data.to_string());
                            if let Some(id) = event.id {
                                builder = builder.id(id);
                            }
                            Ok::<Event, std::convert::Infallible>(builder)
                        }
                        Err(err) => {
                            let payload = json!({ "event": "error", "message": err.to_string() });
                            Ok::<Event, std::convert::Infallible>(
                                Event::default().event("error").data(payload.to_string()),
                            )
                        }
                    }
                });
                let sse = Sse::new(mapped)
                    .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)));
                Ok(sse.into_response())
            } else {
                let response = state
                    .orchestrator
                    .run(request)
                    .await
                    .map_err(map_orchestrator_error)?;
                Ok(Json(json!({ "data": response })).into_response())
            }
        }
    }
}

pub(crate) async fn build_chat_request(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
    content: String,
    stream: bool,
    attachments: Option<Vec<ChatAttachment>>,
) -> Result<WunderRequest, Response> {
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let content = content.trim().to_string();
    if content.is_empty() {
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
    let mut allowed = compute_allowed_tool_names(user, &user_context);
    let overrides = resolve_session_tool_overrides(&record, agent_record.as_ref());
    allowed = apply_tool_overrides(allowed, &overrides);
    let tool_names = finalize_tool_names(allowed);
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());

    if should_auto_title(&record.title) {
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
        })
        .map(|item| AttachmentPayload {
            name: item.name,
            content: item.content,
            content_type: item.mime_type,
        })
        .collect::<Vec<_>>();
    let attachments = if attachments.is_empty() {
        None
    } else {
        Some(attachments)
    };

    Ok(WunderRequest {
        user_id: user.user_id.clone(),
        question: content,
        tool_names,
        skip_tool_calls: false,
        stream,
        debug_payload: false,
        session_id: Some(session_id),
        agent_id: record.agent_id.clone(),
        model_name: None,
        language: Some(i18n::get_language()),
        config_overrides: None,
        agent_prompt,
        attachments,
        is_admin: UserStore::is_admin(user),
    })
}

async fn resume_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ResumeQuery>,
) -> Result<Response, Response> {
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

    if let Some(after_event_id) = query.after_event_id {
        let workspace = state.workspace.clone();
        let monitor = state.monitor.clone();
        let session_id = session_id.clone();
        let (event_tx, event_rx) = mpsc::channel::<Event>(STREAM_EVENT_QUEUE_SIZE);
        tokio::spawn(async move {
            let poll_interval =
                std::time::Duration::from_secs_f64(STREAM_EVENT_RESUME_POLL_INTERVAL_S);
            let mut last_event_id = after_event_id;
            loop {
                let session_id_snapshot = session_id.clone();
                let workspace_snapshot = workspace.clone();
                let records = tokio::task::spawn_blocking(move || {
                    workspace_snapshot.load_stream_events(
                        &session_id_snapshot,
                        last_event_id,
                        STREAM_EVENT_FETCH_LIMIT,
                    )
                })
                .await
                .unwrap_or_default();
                let mut progressed = false;
                for record in records {
                    let Some(event) = map_stream_event(record) else {
                        continue;
                    };
                    let parsed_id = event
                        .id
                        .as_ref()
                        .and_then(|value| value.parse::<i64>().ok())
                        .unwrap_or(0);
                    if parsed_id > last_event_id {
                        last_event_id = parsed_id;
                    }
                    let mut builder = Event::default()
                        .event(event.event)
                        .data(event.data.to_string());
                    if let Some(id) = event.id {
                        builder = builder.id(id);
                    }
                    if event_tx.send(builder).await.is_err() {
                        return;
                    }
                    progressed = true;
                }
                if !progressed {
                    let running = monitor
                        .get_record(&session_id)
                        .map(|record| {
                            record
                                .get("status")
                                .and_then(Value::as_str)
                                .map(|status| {
                                    status == MonitorState::STATUS_RUNNING
                                        || status == MonitorState::STATUS_CANCELLING
                                })
                                .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    if !running {
                        break;
                    }
                    tokio::time::sleep(poll_interval).await;
                }
            }
        });
        let stream =
            ReceiverStream::new(event_rx).map(|event| Ok::<Event, std::convert::Infallible>(event));
        let sse = Sse::new(stream)
            .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)));
        return Ok(sse.into_response());
    }

    let monitor = state.monitor.clone();
    let session_id = session_id.clone();
    let (event_tx, event_rx) = mpsc::channel::<Event>(STREAM_EVENT_QUEUE_SIZE);
    tokio::spawn(async move {
        let mut last_len: usize = 0;
        let mut initialized = false;
        let poll_interval = std::time::Duration::from_secs_f64(STREAM_EVENT_RESUME_POLL_INTERVAL_S);
        loop {
            let Some(record) = monitor.get_record(&session_id) else {
                break;
            };
            let status = record.get("status").and_then(Value::as_str).unwrap_or("");
            let running =
                status == MonitorState::STATUS_RUNNING || status == MonitorState::STATUS_CANCELLING;
            let events = record
                .get("events")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !initialized {
                last_len = events.len();
                initialized = true;
            } else if events.len() < last_len {
                last_len = 0;
            }
            let mut has_new_events = false;
            if events.len() > last_len {
                has_new_events = true;
                for event in events.iter().skip(last_len) {
                    let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
                    if event_type.is_empty() || !is_workflow_event(event_type) {
                        continue;
                    }
                    let data = event.get("data").cloned().unwrap_or(Value::Null);
                    let builder = Event::default()
                        .event(event_type.to_string())
                        .data(data.to_string());
                    if event_tx.send(builder).await.is_err() {
                        return;
                    }
                }
                last_len = events.len();
            }
            if !running && !has_new_events {
                break;
            }
            tokio::time::sleep(poll_interval).await;
        }
    });
    let stream =
        ReceiverStream::new(event_rx).map(|event| Ok::<Event, std::convert::Infallible>(event));
    let sse =
        Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)));
    Ok(sse.into_response())
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
    let cancelled = state.monitor.cancel(&session_id);
    Ok(Json(json!({ "data": { "cancelled": cancelled } })))
}

async fn system_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SystemPromptRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_context = build_user_tool_context(&state, &resolved.user.user_id).await;
    let agent_record =
        fetch_agent_record(&state, &resolved.user, payload.agent_id.as_deref(), false).await?;
    let mut allowed = compute_allowed_tool_names(&resolved.user, &user_context);
    let overrides = payload
        .tool_overrides
        .map(normalize_tool_overrides)
        .unwrap_or_else(|| resolve_agent_tool_defaults(agent_record.as_ref()));
    allowed = apply_tool_overrides(allowed, &overrides);
    let tool_names = finalize_tool_names(allowed);
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let workspace_id = state
        .workspace
        .scoped_user_id(&resolved.user.user_id, payload.agent_id.as_deref());
    let prompt = state
        .orchestrator
        .build_system_prompt(
            &user_context.config,
            &tool_names,
            &user_context.skills,
            Some(&user_context.bindings),
            &resolved.user.user_id,
            UserStore::is_admin(&resolved.user),
            &workspace_id,
            None,
            agent_prompt.as_deref(),
        )
        .await;
    Ok(Json(json!({ "data": { "prompt": prompt } })))
}

async fn session_system_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<SystemPromptRequest>,
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
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let stored_prompt = state
        .workspace
        .load_session_system_prompt_async(&resolved.user.user_id, &session_id, None)
        .await
        .unwrap_or(None);
    if let Some(prompt) = stored_prompt {
        return Ok(Json(json!({ "data": { "prompt": prompt } })));
    }
    let user_context = build_user_tool_context(&state, &resolved.user.user_id).await;
    let agent_record = fetch_agent_record(
        &state,
        &resolved.user,
        record.agent_id.as_deref().or(payload.agent_id.as_deref()),
        true,
    )
    .await?;
    let mut allowed = compute_allowed_tool_names(&resolved.user, &user_context);
    let overrides = if !record.tool_overrides.is_empty() {
        normalize_tool_overrides(record.tool_overrides.clone())
    } else if let Some(overrides) = payload.tool_overrides.as_ref() {
        normalize_tool_overrides(overrides.clone())
    } else {
        resolve_agent_tool_defaults(agent_record.as_ref())
    };
    allowed = apply_tool_overrides(allowed, &overrides);
    let tool_names = finalize_tool_names(allowed);
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let workspace_id = state.workspace.scoped_user_id(
        &resolved.user.user_id,
        record.agent_id.as_deref().or(payload.agent_id.as_deref()),
    );
    let prompt = state
        .orchestrator
        .build_system_prompt(
            &user_context.config,
            &tool_names,
            &user_context.skills,
            Some(&user_context.bindings),
            &resolved.user.user_id,
            UserStore::is_admin(&resolved.user),
            &workspace_id,
            None,
            agent_prompt.as_deref(),
        )
        .await;
    Ok(Json(json!({ "data": { "prompt": prompt } })))
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

    let agent_record =
        fetch_agent_record(&state, &resolved.user, record.agent_id.as_deref(), true).await?;
    let mut effective_allowed = compute_allowed_tool_names(&resolved.user, &user_context);
    let effective_overrides = resolve_session_tool_overrides(&record, agent_record.as_ref());
    effective_allowed = apply_tool_overrides(effective_allowed, &effective_overrides);
    let tool_names = finalize_tool_names(effective_allowed);
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let workspace_id = state
        .workspace
        .scoped_user_id(&resolved.user.user_id, record.agent_id.as_deref());
    let prompt = state
        .orchestrator
        .build_system_prompt(
            &user_context.config,
            &tool_names,
            &user_context.skills,
            Some(&user_context.bindings),
            &resolved.user.user_id,
            UserStore::is_admin(&resolved.user),
            &workspace_id,
            None,
            agent_prompt.as_deref(),
        )
        .await;
    let _ = state.workspace.save_session_system_prompt(
        &resolved.user.user_id,
        &session_id,
        &prompt,
        None,
    );

    Ok(Json(json!({
        "data": {
            "id": session_id,
            "tool_overrides": record.tool_overrides,
        }
    })))
}

async fn chat_attachment_convert(multipart: Multipart) -> Result<Json<Value>, Response> {
    let conversions = convert_multipart_list(multipart).await?;
    Ok(Json(json!({
        "data": build_conversion_payload(conversions),
    })))
}

fn map_history_message(item: Value) -> Option<Value> {
    let role = item.get("role").and_then(Value::as_str)?;
    if role == "system" || role == "tool" {
        return None;
    }
    let raw_content = item.get("content").cloned().unwrap_or(Value::Null);
    let content = normalize_message_content(&raw_content);
    let reasoning = if role == "assistant" {
        item.get("reasoning_content")
            .or_else(|| item.get("reasoning"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
    } else {
        ""
    };
    if role == "assistant" {
        let content_trimmed = content.trim();
        let keep_tool_message = item
            .get("_keep_tool_message")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !keep_tool_message
            && (is_tool_call_meta(&item)
                || is_tool_payload_value(&raw_content)
                || is_tool_payload_text(content_trimmed)
                || (content_trimmed.is_empty() && is_tool_payload_text(reasoning)))
        {
            return None;
        }
    }
    let created_at_raw = item.get("timestamp").and_then(Value::as_str).unwrap_or("");
    let created_at = format_ts_text(created_at_raw);
    let mut message = json!({
        "role": role,
        "content": content,
        "created_at": created_at,
    });
    if let Some(panel) = extract_question_panel(&item) {
        if let Value::Object(ref mut map) = message {
            map.insert("questionPanel".to_string(), panel);
        }
    }
    if role == "assistant" && !reasoning.is_empty() {
        if let Value::Object(ref mut map) = message {
            map.insert("reasoning".to_string(), json!(reasoning));
        }
    }
    Some(message)
}

fn filter_history_messages(mut history: Vec<Value>, preserve_last_assistant: bool) -> Vec<Value> {
    let mut drop_assistant = vec![false; history.len()];
    let mut last_assistant_idx: Option<usize> = None;
    for (index, item) in history.iter().enumerate() {
        let role = item.get("role").and_then(Value::as_str).unwrap_or("");
        match role {
            "assistant" => {
                last_assistant_idx = Some(index);
            }
            "tool" => {
                if let Some(target) = last_assistant_idx {
                    drop_assistant[target] = true;
                }
            }
            "user" => {
                last_assistant_idx = None;
            }
            _ => {}
        }
    }
    let preserved_idx = if preserve_last_assistant {
        history
            .iter()
            .rposition(|item| item.get("role").and_then(Value::as_str) == Some("assistant"))
    } else {
        None
    };
    if let Some(index) = preserved_idx {
        if let Some(Value::Object(ref mut map)) = history.get_mut(index) {
            map.insert("_keep_tool_message".to_string(), Value::Bool(true));
        }
    }
    history
        .into_iter()
        .enumerate()
        .filter(|(index, _)| {
            if drop_assistant[*index] {
                Some(*index) == preserved_idx
            } else {
                true
            }
        })
        .map(|(_, item)| item)
        .collect()
}

fn is_tool_call_meta(item: &Value) -> bool {
    item.get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("type"))
        .and_then(Value::as_str)
        .map(|value| value == "tool_call")
        .unwrap_or(false)
}

fn extract_question_panel(item: &Value) -> Option<Value> {
    let meta = item.get("meta").and_then(Value::as_object)?;
    let meta_type = meta.get("type").and_then(Value::as_str).unwrap_or("");
    if meta_type == "question_panel" {
        if let Some(panel) = meta.get("panel") {
            return Some(panel.clone());
        }
    }
    meta.get("question_panel")
        .or_else(|| meta.get("questionPanel"))
        .cloned()
}

fn is_tool_payload_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with(OBSERVATION_PREFIX) {
        return true;
    }
    if trimmed.contains("<tool_call")
        || trimmed.contains("</tool_call")
        || trimmed.contains("<tool>")
        || trimmed.contains("</tool>")
    {
        return true;
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return is_tool_payload(&value) && !has_visible_answer_fields(&value);
        }
    }
    trimmed.contains("\"tool_calls\"")
        || trimmed.contains("\"tool_call\"")
        || trimmed.contains("\"function_call\"")
        || trimmed.contains("\"tool_result\"")
}

fn has_visible_answer_fields(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    if has_non_empty_field(map.get("answer"))
        || has_non_empty_field(map.get("content"))
        || has_non_empty_field(map.get("message"))
    {
        return true;
    }
    let Some(data) = map.get("data").and_then(Value::as_object) else {
        return false;
    };
    has_non_empty_field(data.get("answer"))
        || has_non_empty_field(data.get("content"))
        || has_non_empty_field(data.get("message"))
}

fn has_non_empty_field(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

fn is_tool_payload_value(value: &Value) -> bool {
    match value {
        Value::String(text) => is_tool_payload_text(text),
        Value::Array(items) => items.iter().any(is_tool_payload_value),
        Value::Object(_) => is_tool_payload(value) && !has_visible_answer_fields(value),
        _ => false,
    }
}

fn is_tool_payload(value: &Value) -> bool {
    is_tool_call_payload(value) || is_tool_result_payload(value)
}

fn is_tool_call_payload(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.contains_key("tool_calls")
                || map.contains_key("tool_call")
                || map.contains_key("function_call")
            {
                return true;
            }
            if let Some(Value::String(kind)) = map.get("type") {
                let lowered = kind.to_lowercase();
                if lowered.contains("tool") || lowered.contains("function") {
                    return true;
                }
            }
            let has_tool = map.contains_key("tool") || map.contains_key("tool_name");
            let has_name = map.contains_key("name");
            let has_args = map.contains_key("arguments")
                || map.contains_key("args")
                || map.contains_key("parameters");
            if (has_tool || has_name) && has_args {
                return true;
            }
            let Some(Value::Object(function)) = map.get("function") else {
                return false;
            };
            let function_has_name = function.contains_key("name") || function.contains_key("tool");
            let function_has_args =
                function.contains_key("arguments") || function.contains_key("args");
            function_has_name && function_has_args
        }
        Value::Array(items) => items.iter().any(is_tool_call_payload),
        _ => false,
    }
}

fn is_tool_result_payload(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            let tool = map.get("tool").and_then(Value::as_str).unwrap_or("").trim();
            if tool.is_empty() {
                return false;
            }
            let has_ok = map.get("ok").and_then(Value::as_bool).is_some();
            let has_data = map.contains_key("data") || map.contains_key("result");
            let has_error = map.contains_key("error");
            let has_timestamp = map
                .get("timestamp")
                .and_then(Value::as_str)
                .map(|text| !text.trim().is_empty())
                .unwrap_or(false);
            (has_ok && (has_data || has_error)) || (has_timestamp && has_data)
        }
        Value::Array(items) => items.iter().any(is_tool_result_payload),
        _ => false,
    }
}

fn normalize_message_content(value: &Value) -> String {
    match value {
        Value::String(text) => text.to_string(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        parts.push(text.trim().to_string());
                    }
                }
            }
            if parts.is_empty() {
                String::new()
            } else {
                parts.join("\n")
            }
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn map_stream_event(record: Value) -> Option<StreamEvent> {
    let event_id = record.get("event_id").and_then(Value::as_i64);
    let event_type = record.get("event").and_then(Value::as_str)?;
    let data = record.get("data").cloned().unwrap_or(Value::Null);
    let timestamp = record
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|dt| dt.with_timezone(&Utc));
    Some(StreamEvent {
        event: event_type.to_string(),
        data,
        id: event_id.map(|value| value.to_string()),
        timestamp,
    })
}

fn session_payload(record: &crate::storage::ChatSessionRecord) -> Value {
    json!({
        "id": record.session_id,
        "title": record.title,
        "created_at": format_ts(record.created_at),
        "updated_at": format_ts(record.updated_at),
        "last_message_at": format_ts(record.last_message_at),
        "agent_id": record.agent_id,
        "tool_overrides": record.tool_overrides,
        "parent_session_id": record.parent_session_id,
        "parent_message_id": record.parent_message_id,
        "spawn_label": record.spawn_label,
        "spawned_by": record.spawned_by,
    })
}

fn session_payload_with_main(record: &crate::storage::ChatSessionRecord, is_main: bool) -> Value {
    let mut payload = session_payload(record);
    if let Value::Object(ref mut map) = payload {
        map.insert("is_main".to_string(), json!(is_main));
    }
    payload
}

fn resolve_pagination(query: &SessionListQuery) -> (i64, i64) {
    if let (Some(page), Some(size)) = (query.page, query.page_size) {
        let safe_page = page.max(1);
        let safe_size = size.max(1).min(200);
        return ((safe_page - 1) * safe_size, safe_size);
    }
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(50).max(1).min(200);
    (offset, limit)
}

fn finalize_tool_names(mut allowed: HashSet<String>) -> Vec<String> {
    if allowed.is_empty() {
        return vec!["__no_tools__".to_string()];
    }
    let mut list = allowed.drain().collect::<Vec<_>>();
    list.sort();
    list
}

async fn fetch_agent_record(
    state: &Arc<AppState>,
    user: &crate::storage::UserAccountRecord,
    agent_id: Option<&str>,
    allow_missing: bool,
) -> Result<Option<crate::storage::UserAgentRecord>, Response> {
    let Some(agent_id) = agent_id.map(str::trim).filter(|value| !value.is_empty()) else {
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

fn resolve_session_tool_overrides(
    record: &crate::storage::ChatSessionRecord,
    agent: Option<&crate::storage::UserAgentRecord>,
) -> Vec<String> {
    if !record.tool_overrides.is_empty() {
        normalize_tool_overrides(record.tool_overrides.clone())
    } else {
        resolve_agent_tool_defaults(agent)
    }
}

fn resolve_agent_tool_defaults(agent: Option<&crate::storage::UserAgentRecord>) -> Vec<String> {
    agent
        .map(|record| record.tool_names.clone())
        .unwrap_or_default()
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

fn filter_tool_overrides(values: Vec<String>, allowed: &HashSet<String>) -> Vec<String> {
    if values.iter().any(|name| name == TOOL_OVERRIDE_NONE) {
        return vec![TOOL_OVERRIDE_NONE.to_string()];
    }
    values
        .into_iter()
        .filter(|name| allowed.contains(name))
        .collect()
}

fn apply_tool_overrides(allowed: HashSet<String>, overrides: &[String]) -> HashSet<String> {
    if overrides.is_empty() {
        return allowed;
    }
    if overrides.iter().any(|name| name == TOOL_OVERRIDE_NONE) {
        return HashSet::new();
    }
    let override_set: HashSet<String> = overrides
        .iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    allowed
        .intersection(&override_set)
        .cloned()
        .collect::<HashSet<_>>()
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

fn format_ts_text(value: &str) -> String {
    let text = value.trim();
    if text.is_empty() {
        return String::new();
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return parsed.with_timezone(&Local).to_rfc3339();
    }
    text.to_string()
}

fn collect_session_event_rounds(record: &Value) -> Vec<Value> {
    let Some(events) = record.get("events").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut order = Vec::new();
    let mut grouped: HashMap<i64, Vec<Value>> = HashMap::new();
    let mut current_round: Option<i64> = None;
    let mut has_round_start = false;
    let register_round = |round: i64,
                          order: &mut Vec<i64>,
                          grouped: &mut HashMap<i64, Vec<Value>>,
                          current_round: &mut Option<i64>| {
        if round <= 0 {
            return;
        }
        if !grouped.contains_key(&round) {
            order.push(round);
            grouped.insert(round, Vec::new());
        }
        *current_round = Some(round);
    };
    for event in events {
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        let data = event.get("data").cloned().unwrap_or(Value::Null);
        let data_round = data.get("user_round").and_then(Value::as_i64);
        if event_type == "round_start" {
            let round = data_round
                .or_else(|| current_round.map(|value| value + 1))
                .unwrap_or(1);
            register_round(round, &mut order, &mut grouped, &mut current_round);
            has_round_start = true;
            continue;
        }
        if current_round.is_none() {
            if let Some(round) = data_round {
                register_round(round, &mut order, &mut grouped, &mut current_round);
            } else if is_workflow_event(event_type) {
                register_round(1, &mut order, &mut grouped, &mut current_round);
            }
        } else if !has_round_start {
            if let Some(round) = data_round {
                register_round(round, &mut order, &mut grouped, &mut current_round);
            }
        }
        let Some(round) = current_round else {
            continue;
        };
        if !is_workflow_event(event_type) {
            continue;
        }
        let timestamp = event
            .get("timestamp")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let timestamp = if timestamp > 0.0 {
            format_ts(timestamp)
        } else {
            String::new()
        };
        let entry = json!({
            "event": event_type,
            "data": data,
            "timestamp": timestamp,
        });
        grouped.entry(round).or_default().push(entry);
    }
    order
        .into_iter()
        .filter_map(|round| {
            let events = grouped.remove(&round).unwrap_or_default();
            if events.is_empty() {
                None
            } else {
                Some(json!({ "user_round": round, "events": events }))
            }
        })
        .collect()
}

fn is_workflow_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "progress"
            | "llm_request"
            | "llm_response"
            | "knowledge_request"
            | "compaction"
            | "tool_call"
            | "tool_result"
            | "plan_update"
            | "question_panel"
            | "llm_output_delta"
            | "llm_output"
            | "context_usage"
            | "quota_usage"
            | "round_usage"
            | "final"
            | "error"
    )
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn map_orchestrator_error(err: Error) -> Response {
    if let Some(orchestrator_err) = err.downcast_ref::<OrchestratorError>() {
        let status = match orchestrator_err.code() {
            "USER_BUSY" | "USER_QUOTA_EXCEEDED" => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::BAD_REQUEST,
        };
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
    crate::api::errors::error_response_with_detail(
        status,
        code.as_deref(),
        message,
        None,
        Some(payload),
    )
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
