use crate::api::chat::{build_chat_request, ChatAttachment};
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::monitor::MonitorState;
use crate::orchestrator_constants::{
    STREAM_EVENT_FETCH_LIMIT, STREAM_EVENT_QUEUE_SIZE, STREAM_EVENT_RESUME_POLL_INTERVAL_S,
};
use crate::schemas::StreamEvent;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::{routing::get, Router};
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt as WsStreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/chat/ws", get(chat_ws))
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsEnvelope {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
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
struct WsCancelPayload {
    #[serde(default)]
    session_id: Option<String>,
}

async fn chat_ws(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let mut auth_headers = headers.clone();
    if auth_headers.get(AUTHORIZATION).is_none() {
        let token = query
            .access_token
            .as_deref()
            .or_else(|| query.token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(token) = token {
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
                auth_headers.insert(AUTHORIZATION, value);
            }
        }
    }
    let resolved = resolve_user(&state, &auth_headers, None).await?;
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
                            "BAD_REQUEST",
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
                            Err(message) => {
                                let _ = send_ws_error(
                                    &out_tx,
                                    envelope.request_id.as_deref(),
                                    "BAD_REQUEST",
                                    message,
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
                                "BAD_REQUEST",
                                i18n::t("error.content_required"),
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
                                    &out_tx,
                                    envelope.request_id.as_deref(),
                                    "BAD_REQUEST",
                                    extract_error_message(response),
                                )
                                .await;
                                continue;
                            }
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
                            Err(message) => {
                                let _ = send_ws_error(
                                    &out_tx,
                                    envelope.request_id.as_deref(),
                                    "BAD_REQUEST",
                                    message,
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
                                "BAD_REQUEST",
                                i18n::t("error.content_required"),
                            )
                            .await;
                            continue;
                        };
                        let after_event_id = payload.after_event_id.unwrap_or(0);
                        if after_event_id <= 0 {
                            let _ = send_ws_error(
                                &out_tx,
                                envelope.request_id.as_deref(),
                                "BAD_REQUEST",
                                i18n::t("error.content_required"),
                            )
                            .await;
                            continue;
                        }
                        if !session_exists(&state, &user.user_id, &session_id) {
                            let _ = send_ws_error(
                                &out_tx,
                                envelope.request_id.as_deref(),
                                "NOT_FOUND",
                                i18n::t("error.session_not_found"),
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
                        let payload = parse_payload::<WsCancelPayload>(envelope.payload)
                            .unwrap_or(WsCancelPayload { session_id: None });
                        let session_id =
                            resolve_session_id(envelope.session_id, payload.session_id);
                        let Some(session_id) = session_id.filter(|value| !value.trim().is_empty())
                        else {
                            let _ = send_ws_error(
                                &out_tx,
                                envelope.request_id.as_deref(),
                                "BAD_REQUEST",
                                i18n::t("error.content_required"),
                            )
                            .await;
                            continue;
                        };
                        if !session_exists(&state, &user.user_id, &session_id) {
                            let _ = send_ws_error(
                                &out_tx,
                                envelope.request_id.as_deref(),
                                "NOT_FOUND",
                                i18n::t("error.session_not_found"),
                            )
                            .await;
                            continue;
                        }
                        let _ = state.monitor.cancel(&session_id);
                    }
                    _ => {
                        let _ = send_ws_error(
                            &out_tx,
                            envelope.request_id.as_deref(),
                            "BAD_REQUEST",
                            i18n::t("error.content_required"),
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

fn is_stream_busy(task: &Option<tokio::task::JoinHandle<()>>) -> bool {
    match task {
        Some(handle) => !handle.is_finished(),
        None => false,
    }
}

fn resolve_session_id(primary: Option<String>, secondary: Option<String>) -> Option<String> {
    primary
        .and_then(|value| {
            if value.trim().is_empty() {
                None
            } else {
                Some(value)
            }
        })
        .or_else(|| {
            secondary.and_then(|value| {
                if value.trim().is_empty() {
                    None
                } else {
                    Some(value)
                }
            })
        })
}

fn session_exists(state: &AppState, user_id: &str, session_id: &str) -> bool {
    state
        .user_store
        .get_chat_session(user_id, session_id)
        .ok()
        .flatten()
        .is_some()
}

fn parse_payload<T: for<'de> Deserialize<'de>>(payload: Option<Value>) -> Result<T, String> {
    let Some(payload) = payload else {
        return Err(i18n::t("error.content_required"));
    };
    serde_json::from_value(payload).map_err(|err| format!("invalid payload: {err}"))
}

async fn send_ws_ready(
    tx: &mpsc::Sender<Message>,
    connection_id: &str,
    now_ts: f64,
) -> Result<(), ()> {
    let payload = json!({
        "connection_id": connection_id,
        "server_time": now_ts,
    });
    send_ws_message(tx, "ready", None, Some(payload)).await
}

async fn send_ws_pong(tx: &mpsc::Sender<Message>) -> Result<(), ()> {
    send_ws_message(
        tx,
        "pong",
        None,
        Some(json!({ "ts": Utc::now().timestamp() })),
    )
    .await
}

async fn send_ws_event(
    tx: &mpsc::Sender<Message>,
    request_id: Option<&str>,
    event: StreamEvent,
) -> Result<(), ()> {
    let payload = json!({
        "event": event.event,
        "id": event.id,
        "data": event.data,
    });
    send_ws_message(tx, "event", request_id, Some(payload)).await
}

async fn send_ws_error(
    tx: &mpsc::Sender<Message>,
    request_id: Option<&str>,
    code: &str,
    message: String,
) -> Result<(), ()> {
    let payload = json!({
        "code": code,
        "message": message,
    });
    send_ws_message(tx, "error", request_id, Some(payload)).await
}

async fn send_ws_message(
    tx: &mpsc::Sender<Message>,
    kind: &str,
    request_id: Option<&str>,
    payload: Option<Value>,
) -> Result<(), ()> {
    let mut map = serde_json::Map::new();
    map.insert("type".to_string(), json!(kind));
    if let Some(request_id) = request_id {
        map.insert("request_id".to_string(), json!(request_id));
    }
    if let Some(payload) = payload {
        map.insert("payload".to_string(), payload);
    }
    let text = Value::Object(map).to_string();
    tx.send(Message::Text(text.into())).await.map_err(|_| ())
}

async fn resume_stream_events(
    state: Arc<AppState>,
    session_id: String,
    after_event_id: i64,
    request_id: Option<&str>,
    tx: mpsc::Sender<Message>,
) {
    let workspace = state.workspace.clone();
    let monitor = state.monitor.clone();
    let poll_interval = std::time::Duration::from_secs_f64(STREAM_EVENT_RESUME_POLL_INTERVAL_S);
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
            if send_ws_event(&tx, request_id, event).await.is_err() {
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
