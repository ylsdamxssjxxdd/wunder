use crate::api::chat::{build_chat_request, ChatAttachment};
use crate::api::user_context::resolve_user;
use crate::api::ws_helpers::{
    apply_ws_auth_headers, parse_payload, resolve_session_id, resume_stream_events, send_ws_error,
    send_ws_event, send_ws_pong, send_ws_ready, WsEnvelope, WsQuery, WsSender,
};
use crate::i18n;
use crate::orchestrator_constants::STREAM_EVENT_QUEUE_SIZE;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, Router};
use chrono::Utc;
use futures::{SinkExt, StreamExt as WsStreamExt};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/chat/ws", get(chat_ws))
}

#[derive(Debug, Deserialize)]
struct WsStartPayload {
    content: String,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    attachments: Option<Vec<ChatAttachment>>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsResumePayload {
    after_event_id: Option<i64>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsWatchPayload {
    #[serde(default)]
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
}

async fn chat_ws(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let auth_headers = apply_ws_auth_headers(&headers, &query);
    let resolved = resolve_user(&state, &auth_headers, None).await?;
    Ok(ws
        .protocols(["wunder"])
        .on_upgrade(move |socket| handle_ws(socket, state, resolved.user)))
}

async fn handle_ws(
    socket: WebSocket,
    state: Arc<AppState>,
    user: crate::storage::UserAccountRecord,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(STREAM_EVENT_QUEUE_SIZE);
    let ws_tx = WsSender::new(out_tx.clone());
    let connection_id = format!("ws_{}", Uuid::new_v4().simple());
    let now_ts = Utc::now().timestamp_millis() as f64 / 1000.0;
    let tasks: Arc<Mutex<HashMap<String, WsStreamEntry>>> = Arc::new(Mutex::new(HashMap::new()));

    let writer = tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            if ws_sender.send(message).await.is_err() {
                break;
            }
        }
    });

    let _ = send_ws_ready(&ws_tx, &connection_id, now_ts).await;

    while let Some(Ok(message)) = WsStreamExt::next(&mut ws_receiver).await {
        match message {
            Message::Text(text) => {
                let envelope: WsEnvelope = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(err) => {
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

                match envelope.kind.trim().to_ascii_lowercase().as_str() {
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
                        let stream = payload.stream.unwrap_or(true);
                        let request = match build_chat_request(
                            &state,
                            &user,
                            &session_id,
                            payload.content,
                            stream,
                            payload.attachments,
                        )
                        .await
                        {
                            Ok(request) => request,
                            Err(response) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    map_ws_error_code(response.status()),
                                    extract_error_message(response),
                                )
                                .await;
                                continue;
                            }
                        };

                        let ws_tx_snapshot = ws_tx.clone();
                        let state_snapshot = state.clone();
                        let (cancel, task_id) =
                            register_ws_task(&tasks, &request_id, Some(session_id.clone())).await;
                        let tasks_cleanup = tasks.clone();
                        let request_id_cleanup = request_id.clone();
                        let task_id_cleanup = task_id.clone();
                        tokio::spawn(async move {
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
                        if !session_exists(&state, &user.user_id, &session_id) {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "SESSION_NOT_FOUND",
                                i18n::t("error.session_not_found"),
                            )
                            .await;
                            continue;
                        }
                        let ws_tx_snapshot = ws_tx.clone();
                        let state_snapshot = state.clone();
                        let (cancel, task_id) =
                            register_ws_task(&tasks, &request_id, Some(session_id.clone())).await;
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
                    "watch" => {
                        let request_id = resolve_request_id(envelope.request_id.as_deref());
                        let payload = match parse_payload::<WsWatchPayload>(envelope.payload) {
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
                        let after_event_id = payload.after_event_id.unwrap_or(0).max(0);
                        let ws_tx_snapshot = ws_tx.clone();
                        let state_snapshot = state.clone();
                        let (cancel, task_id) =
                            register_ws_task(&tasks, &request_id, Some(session_id.clone())).await;
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
                                true,
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
                        let mut cancel_session_id = session_id.clone();
                        {
                            let mut guard = tasks.lock().await;
                            if let Some(request_id) = request_id.as_deref() {
                                if let Some(entry) = guard.remove(request_id) {
                                    entry.cancel.cancel();
                                    if cancel_session_id.is_none() {
                                        cancel_session_id = entry.session_id;
                                    }
                                }
                            }
                            if let Some(session_id) = session_id.as_deref() {
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
                            }
                        }
                        if let Some(session_id) = cancel_session_id {
                            if session_exists(&state, &user.user_id, &session_id) {
                                let _ = state.monitor.cancel(&session_id);
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
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    drop(out_tx);
    let _ = writer.await;
}

fn session_exists(state: &AppState, user_id: &str, session_id: &str) -> bool {
    state
        .user_store
        .get_chat_session(user_id, session_id)
        .ok()
        .flatten()
        .is_some()
}

fn extract_error_message(response: Response) -> String {
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        return i18n::t("error.auth_required");
    }
    if status == StatusCode::NOT_FOUND {
        return i18n::t("error.session_not_found");
    }
    i18n::t("error.content_required")
}

fn map_ws_error_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "AUTH_REQUIRED",
        StatusCode::NOT_FOUND => "SESSION_NOT_FOUND",
        StatusCode::BAD_REQUEST => "INVALID_REQUEST",
        _ => "BAD_REQUEST",
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
