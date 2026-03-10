use crate::api::beeroom::load_group;
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
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Response;
use axum::{routing::get, Router};
use chrono::Utc;
use futures::{SinkExt, Stream, StreamExt as WsStreamExt};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/beeroom/ws", get(beeroom_ws))
        .route(
            "/wunder/beeroom/groups/{group_id}/chat/stream",
            get(beeroom_chat_stream),
        )
}

#[derive(Debug, Deserialize)]
struct WsWatchPayload {
    group_id: String,
    #[serde(default)]
    after_event_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct WsCancelPayload {
    #[serde(default)]
    target_request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BeeroomStreamQuery {
    #[serde(default)]
    after_event_id: Option<i64>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
}

#[derive(Clone)]
struct WatchTask {
    cancel: CancellationToken,
}

async fn beeroom_ws(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let auth_headers = apply_ws_auth_headers(&headers, &query);
    let resolved = resolve_user(&state, &auth_headers, None).await?;
    let connection_id = format!("brws_{}", Uuid::new_v4().simple());
    Ok(ws
        .protocols(["wunder"])
        .max_message_size(WS_MAX_MESSAGE_BYTES)
        .max_frame_size(WS_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_ws(socket, state, resolved.user.user_id, connection_id)))
}

async fn beeroom_chat_stream(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Query(query): Query<BeeroomStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Response> {
    let BeeroomStreamQuery {
        after_event_id,
        access_token,
        token,
        user_id,
    } = query;
    let auth_headers = apply_ws_auth_headers(
        &headers,
        &WsQuery {
            access_token,
            token,
            user_id,
        },
    );
    let resolved = resolve_user(&state, &auth_headers, None).await?;
    let user_id = resolved.user.user_id;
    let group = load_group(state.as_ref(), &user_id, &group_id)?;
    let mut receiver = state
        .beeroom_realtime
        .subscribe_group(&user_id, &group.hive_id)
        .await
        .map_err(|err| {
            crate::api::errors::error_response(StatusCode::BAD_REQUEST, err.to_string())
        })?;
    let current_event_id = resolve_after_event_id(&headers, after_event_id);
    let normalized_group_id = group.hive_id.clone();
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(STREAM_EVENT_QUEUE_SIZE);

    tokio::spawn(async move {
        let mut cursor_event_id = current_event_id;
        let watching_event = Event::default().event("watching").data(
            json!({
                "group_id": &normalized_group_id,
                "after_event_id": cursor_event_id,
            })
            .to_string(),
        );
        if tx.send(Ok(watching_event)).await.is_err() {
            return;
        }
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if event.group_id != normalized_group_id || event.event_id <= cursor_event_id {
                        continue;
                    }
                    cursor_event_id = event.event_id;
                    let stream_event = Event::default()
                        .event(event.event_type)
                        .id(event.event_id.to_string())
                        .data(event.payload.to_string());
                    if tx.send(Ok(stream_event)).await.is_err() {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    let gap_event = Event::default().event("sync_required").data(
                        json!({
                            "group_id": &normalized_group_id,
                            "reason": "lagged",
                            "skipped": skipped,
                            "after_event_id": cursor_event_id,
                        })
                        .to_string(),
                    );
                    if tx.send(Ok(gap_event)).await.is_err() {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return;
                }
            }
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
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
                        let group_id = payload.group_id.trim().to_string();
                        if group_id.is_empty() {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "BAD_REQUEST",
                                "group_id is required".to_string(),
                            )
                            .await;
                            continue;
                        }
                        let Ok(group) = load_group(state.as_ref(), &user_id, &group_id) else {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "FORBIDDEN",
                                "beeroom group not found".to_string(),
                            )
                            .await;
                            continue;
                        };
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
                        let normalized_group_id = group.hive_id.clone();
                        let after_event_id = payload.after_event_id.unwrap_or(0).max(0);
                        tokio::spawn(async move {
                            run_watch_loop(
                                state_snapshot,
                                user_snapshot,
                                normalized_group_id,
                                req_snapshot,
                                after_event_id,
                                cancel,
                                ws_tx_snapshot,
                            )
                            .await;
                        });
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
    drop(out_tx);
    let _ = writer.await;
}

async fn run_watch_loop(
    state: Arc<AppState>,
    user_id: String,
    group_id: String,
    request_id: String,
    after_event_id: i64,
    cancel: CancellationToken,
    ws_tx: WsSender,
) {
    let mut current_event_id = after_event_id;
    let mut receiver = match state
        .beeroom_realtime
        .subscribe_group(&user_id, &group_id)
        .await
    {
        Ok(receiver) => receiver,
        Err(err) => {
            let _ = send_ws_error(&ws_tx, Some(&request_id), "BAD_REQUEST", err.to_string()).await;
            return;
        }
    };
    let watching_event = StreamEvent {
        event: "watching".to_string(),
        data: json!({
            "group_id": group_id,
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
                        if event.group_id != group_id || event.event_id <= current_event_id {
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
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        let gap_event = StreamEvent {
                            event: "sync_required".to_string(),
                            data: json!({
                                "group_id": group_id,
                                "reason": "lagged",
                                "skipped": skipped,
                                "after_event_id": current_event_id
                            }),
                            id: None,
                            timestamp: Some(Utc::now()),
                        };
                        if send_ws_event(&ws_tx, Some(&request_id), gap_event).await.is_err() {
                            return;
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

fn resolve_after_event_id(headers: &HeaderMap, query_after_event_id: Option<i64>) -> i64 {
    query_after_event_id
        .and_then(|value| (value >= 0).then_some(value))
        .or_else(|| {
            headers
                .get("last-event-id")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<i64>().ok())
                .and_then(|value| (value >= 0).then_some(value))
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::resolve_after_event_id;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn resolve_after_event_id_prefers_query_value() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", HeaderValue::from_static("42"));
        assert_eq!(resolve_after_event_id(&headers, Some(5)), 5);
    }

    #[test]
    fn resolve_after_event_id_uses_header_when_query_absent() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", HeaderValue::from_static("9"));
        assert_eq!(resolve_after_event_id(&headers, None), 9);
    }

    #[test]
    fn resolve_after_event_id_rejects_invalid_or_negative_values() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", HeaderValue::from_static("-3"));
        assert_eq!(resolve_after_event_id(&headers, None), 0);
        headers.insert("last-event-id", HeaderValue::from_static("bad"));
        assert_eq!(resolve_after_event_id(&headers, None), 0);
    }
}
