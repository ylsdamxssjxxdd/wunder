use crate::api::user_context::resolve_user;
use crate::api::ws_helpers::{
    apply_ws_auth_headers, has_ws_protocol_token, negotiate_ws_protocol, parse_connect_payload,
    parse_payload, resolve_session_id, resume_stream_events, send_ws_error, send_ws_event,
    send_ws_pong, send_ws_ready, ws_protocol_info, WsEnvelope, WsFeatures, WsPolicy, WsQuery,
    WsReadyPayload, WsSender, WS_MAX_MESSAGE_BYTES, WS_PROTOCOL_VERSION,
};
use crate::api::ws_log::{
    log_ws_close, log_ws_handshake, log_ws_handshake_error, log_ws_message, log_ws_open,
    log_ws_parse_error, log_ws_ready, WsConnMeta,
};
use crate::i18n;
use crate::orchestrator_constants::STREAM_EVENT_QUEUE_SIZE;
use crate::schemas::{AttachmentPayload, StreamEvent, WunderRequest};
use crate::services::agent_runtime::AgentSubmitOutcome;
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
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const WS_ENDPOINT: &str = "/wunder/ws";

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

#[derive(Clone)]
struct WsStreamEntry {
    session_id: Option<String>,
    cancel: CancellationToken,
    task_id: String,
    cancel_session: bool,
}

async fn wunder_ws(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let auth_headers = apply_ws_auth_headers(&headers, &query);
    let resolved = resolve_user(&state, &auth_headers, query.user_id.as_deref()).await?;
    let has_protocol_token = has_ws_protocol_token(&headers);
    let conn_meta = WsConnMeta::from_headers(&headers, has_protocol_token);
    let connection_id = format!("ws_{}", Uuid::new_v4().simple());
    Ok(ws
        .protocols(["wunder"])
        .max_message_size(WS_MAX_MESSAGE_BYTES)
        .max_frame_size(WS_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| {
            handle_ws(socket, state, resolved.user, connection_id, conn_meta)
        }))
}

async fn handle_ws(
    socket: WebSocket,
    state: Arc<AppState>,
    user: crate::storage::UserAccountRecord,
    connection_id: String,
    conn_meta: WsConnMeta,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(STREAM_EVENT_QUEUE_SIZE);
    let ws_tx = WsSender::new(out_tx.clone());
    let started_at = std::time::Instant::now();
    let tasks: Arc<Mutex<HashMap<String, WsStreamEntry>>> = Arc::new(Mutex::new(HashMap::new()));

    let writer = tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            if ws_sender.send(message).await.is_err() {
                break;
            }
        }
    });

    log_ws_open(WS_ENDPOINT, &connection_id, &user.user_id, &conn_meta);
    let now_ts = Utc::now().timestamp_millis() as f64 / 1000.0;
    let protocol = ws_protocol_info();
    let policy = WsPolicy::default_policy();
    let features = WsFeatures {
        multiplex: true,
        resume: true,
        watch: false,
        ping_pong: true,
    };
    let ready_payload = WsReadyPayload {
        connection_id: connection_id.clone(),
        server_time: now_ts,
        protocol: protocol.clone(),
        policy: policy.clone(),
        features: features.clone(),
    };
    let _ = send_ws_ready(&ws_tx, None, ready_payload.clone()).await;
    log_ws_ready(
        WS_ENDPOINT,
        &connection_id,
        &user.user_id,
        protocol.version,
        protocol.min,
        protocol.max,
    );

    let mut handshake_done = false;
    let mut close_logged = false;

    while let Some(Ok(message)) = WsStreamExt::next(&mut ws_receiver).await {
        match message {
            Message::Text(text) => {
                let envelope: WsEnvelope = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(err) => {
                        log_ws_parse_error(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            &err.to_string(),
                        );
                        let _ = send_ws_error(
                            &ws_tx,
                            None,
                            "INVALID_JSON",
                            format!("invalid payload: {err}"),
                        )
                        .await;
                        continue;
                    }
                };

                let kind = envelope.kind.trim().to_ascii_lowercase();
                if !handshake_done && kind != "connect" && kind != "ping" {
                    handshake_done = true;
                    log_ws_handshake(
                        WS_ENDPOINT,
                        &connection_id,
                        &user.user_id,
                        WS_PROTOCOL_VERSION,
                        WS_PROTOCOL_VERSION,
                        true,
                        None,
                    );
                }

                match kind.as_str() {
                    "connect" => {
                        if handshake_done {
                            let _ = send_ws_error(
                                &ws_tx,
                                envelope.request_id.as_deref(),
                                "ALREADY_CONNECTED",
                                "connection already initialized".to_string(),
                            )
                            .await;
                            continue;
                        }
                        let request_id = resolve_request_id(envelope.request_id.as_deref());
                        let payload = match parse_connect_payload(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                continue;
                            }
                        };
                        match negotiate_ws_protocol(&payload) {
                            Ok(info) => {
                                handshake_done = true;
                                log_ws_handshake(
                                    WS_ENDPOINT,
                                    &connection_id,
                                    &user.user_id,
                                    info.client_min,
                                    info.client_max,
                                    false,
                                    info.client.as_ref(),
                                );
                                let _ =
                                    send_ws_ready(&ws_tx, Some(&request_id), ready_payload.clone())
                                        .await;
                                log_ws_ready(
                                    WS_ENDPOINT,
                                    &connection_id,
                                    &user.user_id,
                                    protocol.version,
                                    protocol.min,
                                    protocol.max,
                                );
                            }
                            Err(err) => {
                                log_ws_handshake_error(
                                    WS_ENDPOINT,
                                    &connection_id,
                                    &user.user_id,
                                    err.code(),
                                    &err.message(),
                                );
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                let _ = out_tx.send(Message::Close(None)).await;
                                close_logged = true;
                                log_ws_close(
                                    WS_ENDPOINT,
                                    &connection_id,
                                    &user.user_id,
                                    None,
                                    Some("handshake_failed"),
                                    Some(started_at.elapsed().as_millis()),
                                );
                                break;
                            }
                        }
                    }
                    "ping" => {
                        let _ = send_ws_pong(&ws_tx).await;
                    }
                    "start" => {
                        let request_id = resolve_request_id(envelope.request_id.as_deref());
                        let payload = match parse_payload::<WsStartPayload>(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
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
                                &ws_tx,
                                Some(&request_id),
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
                                        &ws_tx,
                                        Some(&request_id),
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
                                    &ws_tx,
                                    Some(&request_id),
                                    "PERMISSION_DENIED",
                                    i18n::t("error.permission_denied"),
                                )
                                .await;
                                continue;
                            }
                        }
                        log_ws_message(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            "start",
                            Some(&request_id),
                            session_id.as_deref(),
                        );
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
                        let outcome = match state.agent_runtime.submit_user_request(request).await {
                            Ok(outcome) => outcome,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    "BAD_REQUEST",
                                    err.to_string(),
                                )
                                .await;
                                continue;
                            }
                        };

                        let (request, lease) = match outcome {
                            AgentSubmitOutcome::Queued(info) => {
                                let payload = json!({
                                    "queued": true,
                                    "queue_id": info.task_id,
                                    "thread_id": info.thread_id,
                                    "session_id": info.session_id,
                                });
                                let queued_event = StreamEvent {
                                    event: "queued".to_string(),
                                    data: payload.clone(),
                                    id: None,
                                    timestamp: Some(Utc::now()),
                                };
                                let _ =
                                    send_ws_event(&ws_tx, Some(&request_id), queued_event).await;
                                let final_event = StreamEvent {
                                    event: "final".to_string(),
                                    data: payload,
                                    id: None,
                                    timestamp: Some(Utc::now()),
                                };
                                let _ = send_ws_event(&ws_tx, Some(&request_id), final_event).await;
                                continue;
                            }
                            AgentSubmitOutcome::Run(request, lease) => (request, lease),
                        };

                        let session_id_for_task = request.session_id.clone();
                        let ws_tx_snapshot = ws_tx.clone();
                        let state_snapshot = state.clone();
                        let (cancel, task_id) =
                            register_ws_task(&tasks, &request_id, session_id_for_task, true).await;
                        let tasks_cleanup = tasks.clone();
                        let request_id_cleanup = request_id.clone();
                        let task_id_cleanup = task_id.clone();
                        tokio::spawn(async move {
                            let _lease = lease;
                            match state_snapshot.orchestrator.stream(request).await {
                                Ok(stream) => {
                                    tokio::pin!(stream);
                                    loop {
                                        tokio::select! {
                                            _ = cancel.cancelled() => {
                                                break;
                                            }
                                            item = tokio_stream::StreamExt::next(&mut stream) => {
                                                let Some(item) = item else {
                                                    break;
                                                };
                                                let event = match item {
                                                    Ok(event) => event,
                                                    Err(_) => continue,
                                                };
                                                if send_ws_event(
                                                    &ws_tx_snapshot,
                                                    Some(&request_id_cleanup),
                                                    event,
                                                )
                                                .await
                                                .is_err()
                                                {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(err) => {
                                    if !cancel.is_cancelled() {
                                        let _ = send_ws_error(
                                            &ws_tx_snapshot,
                                            Some(&request_id_cleanup),
                                            "BAD_REQUEST",
                                            err.to_string(),
                                        )
                                        .await;
                                    }
                                }
                            }
                            cleanup_ws_task(&tasks_cleanup, &request_id_cleanup, &task_id_cleanup)
                                .await;
                        });
                    }
                    "resume" => {
                        let request_id = resolve_request_id(envelope.request_id.as_deref());
                        let payload = match parse_payload::<WsResumePayload>(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
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
                                &ws_tx,
                                Some(&request_id),
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
                                    &ws_tx,
                                    Some(&request_id),
                                    "SESSION_NOT_FOUND",
                                    i18n::t("error.session_not_found"),
                                )
                                .await;
                                continue;
                            }
                            Err(SessionAccessError::Forbidden) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    "PERMISSION_DENIED",
                                    i18n::t("error.permission_denied"),
                                )
                                .await;
                                continue;
                            }
                        }
                        log_ws_message(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            "resume",
                            Some(&request_id),
                            Some(&session_id),
                        );
                        let after_event_id = payload.after_event_id.unwrap_or(0);
                        if after_event_id <= 0 {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "AFTER_EVENT_ID_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        }
                        let ws_tx_snapshot = ws_tx.clone();
                        let state_snapshot = state.clone();
                        let (cancel, task_id) =
                            register_ws_task(&tasks, &request_id, Some(session_id.clone()), false)
                                .await;
                        let tasks_cleanup = tasks.clone();
                        let request_id_cleanup = request_id.clone();
                        let task_id_cleanup = task_id.clone();
                        tokio::spawn(async move {
                            resume_stream_events(
                                state_snapshot,
                                session_id,
                                after_event_id,
                                Some(&request_id_cleanup),
                                ws_tx_snapshot,
                                Some(cancel.clone()),
                                false,
                            )
                            .await;
                            cleanup_ws_task(&tasks_cleanup, &request_id_cleanup, &task_id_cleanup)
                                .await;
                        });
                    }
                    "cancel" => {
                        let payload = match envelope.payload {
                            Some(value) => match serde_json::from_value::<WsCancelPayload>(value) {
                                Ok(payload) => payload,
                                Err(err) => {
                                    let _ = send_ws_error(
                                        &ws_tx,
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
                        let request_id = normalize_request_id(envelope.request_id.as_deref());
                        let session_id =
                            resolve_session_id(envelope.session_id, payload.session_id);
                        let session_id = session_id.filter(|value| !value.trim().is_empty());
                        if request_id.is_none() && session_id.is_none() {
                            let _ = send_ws_error(
                                &ws_tx,
                                envelope.request_id.as_deref(),
                                "SESSION_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        }
                        log_ws_message(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            "cancel",
                            request_id.as_deref(),
                            session_id.as_deref(),
                        );
                        let mut cancel_session_id = None;
                        {
                            let mut guard = tasks.lock().await;
                            if let Some(request_id) = request_id.as_deref() {
                                if let Some(entry) = guard.remove(request_id) {
                                    entry.cancel.cancel();
                                    if entry.cancel_session {
                                        cancel_session_id = entry.session_id;
                                    }
                                }
                            } else if let Some(session_id) = session_id.as_deref() {
                                let targets = guard
                                    .iter()
                                    .filter_map(|(key, entry)| {
                                        if entry.session_id.as_deref() == Some(session_id) {
                                            Some(key.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect::<Vec<_>>();
                                for key in targets {
                                    if let Some(entry) = guard.remove(&key) {
                                        entry.cancel.cancel();
                                    }
                                }
                                cancel_session_id = Some(session_id.to_string());
                            }
                        }
                        if let Some(session_id) = cancel_session_id {
                            match validate_session_access(&state, &user, &session_id) {
                                Ok(()) => {
                                    let _ = state.monitor.cancel(&session_id);
                                }
                                Err(SessionAccessError::NotFound) => {
                                    let _ = send_ws_error(
                                        &ws_tx,
                                        envelope.request_id.as_deref(),
                                        "SESSION_NOT_FOUND",
                                        i18n::t("error.session_not_found"),
                                    )
                                    .await;
                                }
                                Err(SessionAccessError::Forbidden) => {
                                    let _ = send_ws_error(
                                        &ws_tx,
                                        envelope.request_id.as_deref(),
                                        "PERMISSION_DENIED",
                                        i18n::t("error.permission_denied"),
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                    _ => {
                        let _ = send_ws_error(
                            &ws_tx,
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
            Message::Close(frame) => {
                let (code, reason) = frame
                    .map(|value| (Some(value.code), Some(value.reason.to_string())))
                    .unwrap_or((None, None));
                close_logged = true;
                log_ws_close(
                    WS_ENDPOINT,
                    &connection_id,
                    &user.user_id,
                    code,
                    reason.as_deref(),
                    Some(started_at.elapsed().as_millis()),
                );
                break;
            }
            _ => {}
        }
    }

    drop(out_tx);
    let _ = writer.await;
    if !close_logged {
        log_ws_close(
            WS_ENDPOINT,
            &connection_id,
            &user.user_id,
            None,
            Some("eof"),
            Some(started_at.elapsed().as_millis()),
        );
    }
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

fn normalize_request_id(request_id: Option<&str>) -> Option<String> {
    request_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn resolve_request_id(request_id: Option<&str>) -> String {
    normalize_request_id(request_id).unwrap_or_else(|| format!("req_{}", Uuid::new_v4().simple()))
}

async fn register_ws_task(
    tasks: &Arc<Mutex<HashMap<String, WsStreamEntry>>>,
    request_id: &str,
    session_id: Option<String>,
    cancel_session: bool,
) -> (CancellationToken, String) {
    let cancel = CancellationToken::new();
    let task_id = Uuid::new_v4().simple().to_string();
    let mut guard = tasks.lock().await;
    if let Some(entry) = guard.insert(
        request_id.to_string(),
        WsStreamEntry {
            session_id,
            cancel: cancel.clone(),
            task_id: task_id.clone(),
            cancel_session,
        },
    ) {
        entry.cancel.cancel();
    }
    (cancel, task_id)
}

async fn cleanup_ws_task(
    tasks: &Arc<Mutex<HashMap<String, WsStreamEntry>>>,
    request_id: &str,
    task_id: &str,
) {
    let mut guard = tasks.lock().await;
    let should_remove = guard
        .get(request_id)
        .map(|entry| entry.task_id == task_id)
        .unwrap_or(false);
    if should_remove {
        guard.remove(request_id);
    }
}
