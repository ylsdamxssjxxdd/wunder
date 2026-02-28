use crate::api::user_context::resolve_user;
use crate::api::ws_helpers::{
    apply_ws_auth_headers, parse_payload, send_ws_error, send_ws_event, send_ws_pong,
    send_ws_ready, ws_protocol_info, WsEnvelope, WsFeatures, WsPolicy, WsQuery, WsReadyPayload,
    WsSender, WS_MAX_MESSAGE_BYTES,
};
use crate::orchestrator_constants::STREAM_EVENT_QUEUE_SIZE;
use crate::schemas::StreamEvent;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::{routing::get, Router};
use chrono::Utc;
use futures::{SinkExt, StreamExt as WsStreamExt};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/user_world/ws", get(user_world_ws))
}

#[derive(Debug, Deserialize)]
struct WsWatchPayload {
    conversation_id: String,
    #[serde(default)]
    after_event_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct WsSendPayload {
    conversation_id: String,
    content: String,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    client_msg_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsReadPayload {
    conversation_id: String,
    #[serde(default)]
    last_read_message_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct WsCancelPayload {
    #[serde(default)]
    target_request_id: Option<String>,
}

#[derive(Clone)]
struct WatchTask {
    cancel: CancellationToken,
}

async fn user_world_ws(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let auth_headers = apply_ws_auth_headers(&headers, &query);
    let resolved = resolve_user(&state, &auth_headers, None).await?;
    let connection_id = format!("uwws_{}", Uuid::new_v4().simple());
    Ok(ws
        .protocols(["wunder"])
        .max_message_size(WS_MAX_MESSAGE_BYTES)
        .max_frame_size(WS_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_ws(socket, state, resolved.user.user_id, connection_id)))
}

async fn handle_ws(
    socket: WebSocket,
    state: Arc<AppState>,
    user_id: String,
    connection_id: String,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(STREAM_EVENT_QUEUE_SIZE);
    let ws_tx = WsSender::new(out_tx.clone());
    let watch_tasks: Arc<Mutex<HashMap<String, WatchTask>>> = Arc::new(Mutex::new(HashMap::new()));

    let writer = tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            if ws_sender.send(message).await.is_err() {
                break;
            }
        }
    });

    let connected_at = Utc::now().timestamp_millis() as f64 / 1000.0;
    state.user_presence.connect(&user_id, connected_at);
    let now_ts = Utc::now().timestamp_millis() as f64 / 1000.0;
    let ready_payload = WsReadyPayload {
        connection_id,
        server_time: now_ts,
        protocol: ws_protocol_info(),
        policy: WsPolicy::default_policy(),
        features: WsFeatures {
            multiplex: true,
            resume: true,
            watch: true,
            ping_pong: true,
        },
    };
    let _ = send_ws_ready(&ws_tx, None, ready_payload.clone()).await;

    while let Some(Ok(message)) = WsStreamExt::next(&mut ws_receiver).await {
        match message {
            Message::Text(text) => {
                let envelope: WsEnvelope = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(err) => {
                        let _ = send_ws_error(&ws_tx, None, "INVALID_JSON", err.to_string()).await;
                        continue;
                    }
                };
                let kind = envelope.kind.trim().to_ascii_lowercase();
                let request_id = resolve_request_id(envelope.request_id.as_deref());
                match kind.as_str() {
                    "connect" => {
                        let _ =
                            send_ws_ready(&ws_tx, Some(&request_id), ready_payload.clone()).await;
                    }
                    "ping" => {
                        let _ = send_ws_pong(&ws_tx).await;
                    }
                    "watch" => {
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
                        let conversation_id = payload.conversation_id.trim().to_string();
                        if conversation_id.is_empty() {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "BAD_REQUEST",
                                "conversation_id is required".to_string(),
                            )
                            .await;
                            continue;
                        }
                        let cancel = CancellationToken::new();
                        {
                            let mut guard = watch_tasks.lock().await;
                            if let Some(existing) = guard.remove(&request_id) {
                                existing.cancel.cancel();
                            }
                            guard.insert(
                                request_id.clone(),
                                WatchTask {
                                    cancel: cancel.clone(),
                                },
                            );
                        }
                        let state_snapshot = state.clone();
                        let user_snapshot = user_id.clone();
                        let req_snapshot = request_id.clone();
                        let ws_tx_snapshot = ws_tx.clone();
                        let after_event_id = payload.after_event_id.unwrap_or(0).max(0);
                        tokio::spawn(async move {
                            run_watch_loop(
                                state_snapshot,
                                user_snapshot,
                                conversation_id,
                                req_snapshot,
                                after_event_id,
                                cancel,
                                ws_tx_snapshot,
                            )
                            .await;
                        });
                    }
                    "send" => {
                        let payload = match parse_payload::<WsSendPayload>(envelope.payload) {
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
                        let conversation_id = payload.conversation_id.trim();
                        let content = payload.content.trim();
                        if conversation_id.is_empty() || content.is_empty() {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "BAD_REQUEST",
                                "conversation_id/content is required".to_string(),
                            )
                            .await;
                            continue;
                        }
                        match state
                            .user_world
                            .send_message(
                                &user_id,
                                conversation_id,
                                content,
                                payload.content_type.as_deref().unwrap_or("text"),
                                payload.client_msg_id.as_deref(),
                                Utc::now().timestamp_millis() as f64 / 1000.0,
                            )
                            .await
                        {
                            Ok(result) => {
                                let ack = StreamEvent {
                                    event: "ack".to_string(),
                                    data: json!(result),
                                    id: None,
                                    timestamp: Some(Utc::now()),
                                };
                                let final_event = StreamEvent {
                                    event: "final".to_string(),
                                    data: json!({ "ok": true }),
                                    id: None,
                                    timestamp: Some(Utc::now()),
                                };
                                let _ = send_ws_event(&ws_tx, Some(&request_id), ack).await;
                                let _ = send_ws_event(&ws_tx, Some(&request_id), final_event).await;
                            }
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    "BAD_REQUEST",
                                    err.to_string(),
                                )
                                .await;
                            }
                        }
                    }
                    "read" => {
                        let payload = match parse_payload::<WsReadPayload>(envelope.payload) {
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
                        let conversation_id = payload.conversation_id.trim();
                        if conversation_id.is_empty() {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "BAD_REQUEST",
                                "conversation_id is required".to_string(),
                            )
                            .await;
                            continue;
                        }
                        match state
                            .user_world
                            .mark_read(
                                &user_id,
                                conversation_id,
                                payload.last_read_message_id,
                                Utc::now().timestamp_millis() as f64 / 1000.0,
                            )
                            .await
                        {
                            Ok(result) => {
                                let ack = StreamEvent {
                                    event: "ack".to_string(),
                                    data: json!(result),
                                    id: None,
                                    timestamp: Some(Utc::now()),
                                };
                                let final_event = StreamEvent {
                                    event: "final".to_string(),
                                    data: json!({ "ok": true }),
                                    id: None,
                                    timestamp: Some(Utc::now()),
                                };
                                let _ = send_ws_event(&ws_tx, Some(&request_id), ack).await;
                                let _ = send_ws_event(&ws_tx, Some(&request_id), final_event).await;
                            }
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    "BAD_REQUEST",
                                    err.to_string(),
                                )
                                .await;
                            }
                        }
                    }
                    "cancel" => {
                        let payload = parse_payload::<WsCancelPayload>(envelope.payload).ok();
                        let target_id = payload
                            .and_then(|item| item.target_request_id)
                            .map(|value| value.trim().to_string())
                            .filter(|value| !value.is_empty())
                            .unwrap_or_else(|| request_id.clone());
                        if let Some(task) = watch_tasks.lock().await.remove(&target_id) {
                            task.cancel.cancel();
                        }
                        let final_event = StreamEvent {
                            event: "final".to_string(),
                            data: json!({ "ok": true, "cancelled": target_id }),
                            id: None,
                            timestamp: Some(Utc::now()),
                        };
                        let _ = send_ws_event(&ws_tx, Some(&request_id), final_event).await;
                    }
                    _ => {
                        let _ = send_ws_error(
                            &ws_tx,
                            Some(&request_id),
                            "UNSUPPORTED_TYPE",
                            format!("unsupported ws type: {kind}"),
                        )
                        .await;
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    {
        let mut guard = watch_tasks.lock().await;
        for (_, task) in guard.drain() {
            task.cancel.cancel();
        }
    }
    state
        .user_presence
        .disconnect(&user_id, Utc::now().timestamp_millis() as f64 / 1000.0);
    drop(out_tx);
    let _ = writer.await;
}

async fn run_watch_loop(
    state: Arc<AppState>,
    user_id: String,
    conversation_id: String,
    request_id: String,
    after_event_id: i64,
    cancel: CancellationToken,
    ws_tx: WsSender,
) {
    let mut current_event_id = after_event_id;
    let initial = state
        .user_world
        .list_events(&user_id, &conversation_id, current_event_id, 200);
    let Ok(initial_events) = initial else {
        let _ = send_ws_error(
            &ws_tx,
            Some(&request_id),
            "BAD_REQUEST",
            "watch conversation failed".to_string(),
        )
        .await;
        return;
    };
    for event in initial_events {
        current_event_id = current_event_id.max(event.event_id);
        let stream_event = StreamEvent {
            event: event.event_type,
            data: event.payload,
            id: Some(event.event_id.to_string()),
            timestamp: Some(Utc::now()),
        };
        if send_ws_event(&ws_tx, Some(&request_id), stream_event)
            .await
            .is_err()
        {
            return;
        }
    }
    let mut receiver = match state.user_world.subscribe_user(&user_id).await {
        Ok(receiver) => receiver,
        Err(err) => {
            let _ = send_ws_error(&ws_tx, Some(&request_id), "BAD_REQUEST", err.to_string()).await;
            return;
        }
    };
    let watching_event = StreamEvent {
        event: "watching".to_string(),
        data: json!({
            "conversation_id": conversation_id,
            "after_event_id": current_event_id,
        }),
        id: None,
        timestamp: Some(Utc::now()),
    };
    let _ = send_ws_event(&ws_tx, Some(&request_id), watching_event).await;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                return;
            }
            recv = receiver.recv() => {
                match recv {
                    Ok(event) => {
                        if event.conversation_id != conversation_id || event.event_id <= current_event_id {
                            continue;
                        }
                        current_event_id = event.event_id;
                        let stream_event = StreamEvent {
                            event: event.event_type,
                            data: event.payload,
                            id: Some(event.event_id.to_string()),
                            timestamp: Some(Utc::now()),
                        };
                        if send_ws_event(&ws_tx, Some(&request_id), stream_event).await.is_err() {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let Ok(events) = state
                            .user_world
                            .list_events(&user_id, &conversation_id, current_event_id, 200) else {
                                continue;
                            };
                        for event in events {
                            current_event_id = current_event_id.max(event.event_id);
                            let stream_event = StreamEvent {
                                event: event.event_type,
                                data: event.payload,
                                id: Some(event.event_id.to_string()),
                                timestamp: Some(Utc::now()),
                            };
                            if send_ws_event(&ws_tx, Some(&request_id), stream_event).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return;
                    }
                }
            }
        }
    }
}

fn resolve_request_id(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("req_{}", Uuid::new_v4().simple()))
}
