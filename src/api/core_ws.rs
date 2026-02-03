use crate::api::user_context::resolve_user;
use crate::api::ws_helpers::{
    apply_ws_auth_headers, parse_payload, resolve_session_id, resume_stream_events, send_ws_error,
    send_ws_event, send_ws_pong, send_ws_ready, WsEnvelope, WsQuery,
};
use crate::i18n;
use crate::orchestrator_constants::STREAM_EVENT_QUEUE_SIZE;
use crate::schemas::{AttachmentPayload, WunderRequest};
use crate::state::AppState;
use crate::user_store::UserStore;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::{routing::get, Router};
use chrono::Utc;
use futures::{SinkExt, StreamExt as WsStreamExt};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/ws", get(wunder_ws))
}

#[derive(Debug, Deserialize)]
struct WsStartPayload {
    #[serde(default)]
    user_id: Option<String>,
    question: String,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    skip_tool_calls: bool,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    debug_payload: bool,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    config_overrides: Option<Value>,
    #[serde(default)]
    agent_prompt: Option<String>,
    #[serde(default)]
    attachments: Option<Vec<AttachmentPayload>>,
}

#[derive(Debug, Deserialize)]
struct WsResumePayload {
    after_event_id: Option<i64>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsCancelPayload {
    #[serde(default)]
    session_id: Option<String>,
}

async fn wunder_ws(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let auth_headers = apply_ws_auth_headers(&headers, &query);
    let resolved = resolve_user(&state, &auth_headers, query.user_id.as_deref()).await?;
    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state, resolved.user)))
}

async fn handle_ws(
    socket: WebSocket,
    state: Arc<AppState>,
    user: crate::storage::UserAccountRecord,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(STREAM_EVENT_QUEUE_SIZE);
    let connection_id = format!("ws_{}", Uuid::new_v4().simple());
    let now_ts = Utc::now().timestamp_millis() as f64 / 1000.0;

    let writer = tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            if ws_sender.send(message).await.is_err() {
                break;
            }
        }
    });

    let _ = send_ws_ready(&out_tx, &connection_id, now_ts).await;

    let mut active_task: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(Ok(message)) = WsStreamExt::next(&mut ws_receiver).await {
        match message {
            Message::Text(text) => {
                let envelope: WsEnvelope = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(err) => {
                        let _ = send_ws_error(
                            &out_tx,
                            None,
                            "INVALID_JSON",
                            format!("invalid payload: {err}"),
                        )
                        .await;
                        continue;
                    }
                };

                match envelope.kind.trim().to_ascii_lowercase().as_str() {
                    "ping" => {
                        let _ = send_ws_pong(&out_tx).await;
                    }
                    "start" => {
                        if is_stream_busy(&active_task) {
                            let _ = send_ws_error(
                                &out_tx,
                                envelope.request_id.as_deref(),
                                "STREAM_BUSY",
                                i18n::t("error.user_session_busy"),
                            )
                            .await;
                            continue;
                        }
                        let payload = match parse_payload::<WsStartPayload>(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &out_tx,
                                    envelope.request_id.as_deref(),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                continue;
                            }
                        };
                        let _ = payload.stream;
                        let question = payload.question.trim().to_string();
                        if question.is_empty() {
                            let _ = send_ws_error(
                                &out_tx,
                                envelope.request_id.as_deref(),
                                "QUESTION_REQUIRED",
                                i18n::t("error.question_required"),
                            )
                            .await;
                            continue;
                        }
                        let payload_user_id = payload
                            .user_id
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(|value| value.to_string());
                        let user_id = match payload_user_id {
                            Some(value) => {
                                if value != user.user_id && !UserStore::is_admin(&user) {
                                    let _ = send_ws_error(
                                        &out_tx,
                                        envelope.request_id.as_deref(),
                                        "UNAUTHORIZED",
                                        i18n::t("error.auth_required"),
                                    )
                                    .await;
                                    continue;
                                }
                                value
                            }
                            None => user.user_id.clone(),
                        };
                        let session_id =
                            resolve_session_id(envelope.session_id, payload.session_id).and_then(
                                |value| {
                                    let trimmed = value.trim().to_string();
                                    if trimmed.is_empty() {
                                        None
                                    } else {
                                        Some(trimmed)
                                    }
                                },
                            );
                        if let Some(session_id) = session_id.as_deref() {
                            if let Err(SessionAccessError::Forbidden) =
                                validate_session_access(&state, &user, session_id)
                            {
                                let _ = send_ws_error(
                                    &out_tx,
                                    envelope.request_id.as_deref(),
                                    "PERMISSION_DENIED",
                                    i18n::t("error.permission_denied"),
                                )
                                .await;
                                continue;
                            }
                        }
                        let stream = true;
                        let request = WunderRequest {
                            user_id,
                            question,
                            tool_names: payload.tool_names,
                            skip_tool_calls: payload.skip_tool_calls,
                            stream,
                            debug_payload: payload.debug_payload,
                            session_id,
                            agent_id: payload.agent_id,
                            model_name: payload.model_name,
                            language: payload.language,
                            config_overrides: payload.config_overrides,
                            agent_prompt: payload.agent_prompt,
                            attachments: payload.attachments,
                            is_admin: UserStore::is_admin(&user),
                        };

                        let request_id = envelope.request_id.clone();
                        let out_tx_snapshot = out_tx.clone();
                        let state_snapshot = state.clone();
                        active_task = Some(tokio::spawn(async move {
                            match state_snapshot.orchestrator.stream(request).await {
                                Ok(stream) => {
                                    tokio::pin!(stream);
                                    while let Some(item) =
                                        tokio_stream::StreamExt::next(&mut stream).await
                                    {
                                        let event = match item {
                                            Ok(event) => event,
                                            Err(_) => continue,
                                        };
                                        if send_ws_event(
                                            &out_tx_snapshot,
                                            request_id.as_deref(),
                                            event,
                                        )
                                        .await
                                        .is_err()
                                        {
                                            break;
                                        }
                                    }
                                }
                                Err(err) => {
                                    let _ = send_ws_error(
                                        &out_tx_snapshot,
                                        request_id.as_deref(),
                                        "BAD_REQUEST",
                                        err.to_string(),
                                    )
                                    .await;
                                }
                            }
                        }));
                    }
                    "resume" => {
                        if is_stream_busy(&active_task) {
                            let _ = send_ws_error(
                                &out_tx,
                                envelope.request_id.as_deref(),
                                "STREAM_BUSY",
                                i18n::t("error.user_session_busy"),
                            )
                            .await;
                            continue;
                        }
                        let payload = match parse_payload::<WsResumePayload>(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &out_tx,
                                    envelope.request_id.as_deref(),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                continue;
                            }
                        };
                        let session_id =
                            resolve_session_id(envelope.session_id, payload.session_id);
                        let Some(session_id) = session_id.filter(|value| !value.trim().is_empty())
                        else {
                            let _ = send_ws_error(
                                &out_tx,
                                envelope.request_id.as_deref(),
                                "SESSION_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        };
                        match validate_session_access(&state, &user, &session_id) {
                            Ok(()) => {}
                            Err(SessionAccessError::NotFound) => {
                                let _ = send_ws_error(
                                    &out_tx,
                                    envelope.request_id.as_deref(),
                                    "SESSION_NOT_FOUND",
                                    i18n::t("error.session_not_found"),
                                )
                                .await;
                                continue;
                            }
                            Err(SessionAccessError::Forbidden) => {
                                let _ = send_ws_error(
                                    &out_tx,
                                    envelope.request_id.as_deref(),
                                    "PERMISSION_DENIED",
                                    i18n::t("error.permission_denied"),
                                )
                                .await;
                                continue;
                            }
                        }
                        let after_event_id = payload.after_event_id.unwrap_or(0);
                        if after_event_id <= 0 {
                            let _ = send_ws_error(
                                &out_tx,
                                envelope.request_id.as_deref(),
                                "AFTER_EVENT_ID_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        }
                        let request_id = envelope.request_id.clone();
                        let out_tx_snapshot = out_tx.clone();
                        let state_snapshot = state.clone();
                        active_task = Some(tokio::spawn(async move {
                            resume_stream_events(
                                state_snapshot,
                                session_id,
                                after_event_id,
                                request_id.as_deref(),
                                out_tx_snapshot,
                            )
                            .await;
                        }));
                    }
                    "cancel" => {
                        let payload = match envelope.payload {
                            Some(value) => match serde_json::from_value::<WsCancelPayload>(value) {
                                Ok(payload) => payload,
                                Err(err) => {
                                    let _ = send_ws_error(
                                        &out_tx,
                                        envelope.request_id.as_deref(),
                                        "INVALID_PAYLOAD",
                                        format!("invalid payload: {err}"),
                                    )
                                    .await;
                                    continue;
                                }
                            },
                            None => WsCancelPayload { session_id: None },
                        };
                        let session_id =
                            resolve_session_id(envelope.session_id, payload.session_id);
                        let Some(session_id) = session_id.filter(|value| !value.trim().is_empty())
                        else {
                            let _ = send_ws_error(
                                &out_tx,
                                envelope.request_id.as_deref(),
                                "SESSION_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        };
                        match validate_session_access(&state, &user, &session_id) {
                            Ok(()) => {}
                            Err(SessionAccessError::NotFound) => {
                                let _ = send_ws_error(
                                    &out_tx,
                                    envelope.request_id.as_deref(),
                                    "SESSION_NOT_FOUND",
                                    i18n::t("error.session_not_found"),
                                )
                                .await;
                                continue;
                            }
                            Err(SessionAccessError::Forbidden) => {
                                let _ = send_ws_error(
                                    &out_tx,
                                    envelope.request_id.as_deref(),
                                    "PERMISSION_DENIED",
                                    i18n::t("error.permission_denied"),
                                )
                                .await;
                                continue;
                            }
                        }
                        let _ = state.monitor.cancel(&session_id);
                    }
                    _ => {
                        let _ = send_ws_error(
                            &out_tx,
                            envelope.request_id.as_deref(),
                            "UNSUPPORTED_TYPE",
                            i18n::t("error.param_required"),
                        )
                        .await;
                    }
                }
            }
            Message::Ping(payload) => {
                let _ = out_tx.send(Message::Pong(payload)).await;
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    drop(out_tx);
    let _ = writer.await;
}

#[derive(Debug)]
enum SessionAccessError {
    NotFound,
    Forbidden,
}

fn validate_session_access(
    state: &AppState,
    user: &crate::storage::UserAccountRecord,
    session_id: &str,
) -> Result<(), SessionAccessError> {
    let record = state.monitor.get_record(session_id);
    let Some(record) = record else {
        return Err(SessionAccessError::NotFound);
    };
    if UserStore::is_admin(user) {
        return Ok(());
    }
    let record_user_id = record.get("user_id").and_then(Value::as_str).unwrap_or("");
    if record_user_id.is_empty() {
        return Err(SessionAccessError::NotFound);
    }
    if record_user_id == user.user_id {
        Ok(())
    } else {
        Err(SessionAccessError::Forbidden)
    }
}

fn is_stream_busy(task: &Option<tokio::task::JoinHandle<()>>) -> bool {
    match task {
        Some(handle) => !handle.is_finished(),
        None => false,
    }
}
