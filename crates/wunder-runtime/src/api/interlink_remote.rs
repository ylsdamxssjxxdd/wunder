//! Remote session view websocket (docs §7.4): a browser watches one thread of
//! one local node.
//!
//! The server is a pure forwarder. It keeps at most one upstream tunnel stream
//! per `(device, thread)` however many tabs watch it, buffers nothing
//! persistently, and answers a refresh with a fresh snapshot from the node.
//! Nothing here writes to the database: remote message bodies flow through and
//! are never stored (docs §9.3 2).

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures::SinkExt;
use futures::StreamExt;
use futures::stream::SplitSink;
use serde::Deserialize;
use serde_json::json;

use wunder_core::interlink::{
    FRAME_EVENT, REMOTE_FRAME_CLOSE, REMOTE_FRAME_ERROR, REMOTE_WS_PROTOCOL, TUNNEL_WS_PROTOCOL,
};

use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::core::blocking;
use crate::services::interlink::{registry, remote};
use crate::state::AppState;

const AUTHORIZATION: &str = "authorization";
const WS_MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// Keep-alive cadence for a watching tab (docs §4.4 transport layer).
const PING_INTERVAL_S: u64 = 25;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/interlink/remote_ws", get(remote_ws))
}

#[derive(Debug, Deserialize, Default)]
struct RemoteQuery {
    /// `device:<id>` - the node whose thread is watched.
    #[serde(default)]
    target: Option<String>,
    /// Local thread id inside that node.
    #[serde(default)]
    thread: Option<String>,
    #[serde(default)]
    token: Option<String>,
}

async fn remote_ws(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<RemoteQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let config = state.config_store.get().await;
    if !config.interlink.enabled {
        drop(config);
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "interlink is disabled on this server".to_string(),
        ));
    }
    drop(config);

    let auth_headers = match query.token.as_deref().map(str::trim) {
        Some(token) if !token.is_empty() && headers.get(AUTHORIZATION).is_none() => {
            let mut cloned = headers.clone();
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
                cloned.insert(AUTHORIZATION, value);
            }
            cloned
        }
        _ => headers.clone(),
    };
    let resolved = match resolve_user(&state, &auth_headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return Err(response),
    };
    let user_id = resolved.user.user_id.clone();

    let Some(device_id) = query
        .target
        .as_deref()
        .and_then(|value| value.strip_prefix("device:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "target must be device:<id>".to_string(),
        ));
    };
    let Some(thread_id) = query
        .thread
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "thread is required".to_string(),
        ));
    };

    // Ownership check: watching a remote thread is an owner-only operation.
    let storage = state.storage.clone();
    let lookup = device_id.to_string();
    let device = match blocking::run_db("api.interlink_remote.device", move || {
        storage.get_cloud_device(&lookup)
    })
    .await
    {
        Ok(device) => device,
        Err(err) => return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())),
    };
    match device {
        Some(device) if !device.revoked && device.user_id == user_id => {}
        _ => {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "device is revoked, unknown or not owned by this account".to_string(),
            ))
        }
    }

    let device_id = device_id.to_string();
    let thread_id = thread_id.to_string();
    Ok(ws
        .protocols([REMOTE_WS_PROTOCOL, TUNNEL_WS_PROTOCOL])
        .max_message_size(WS_MAX_MESSAGE_BYTES)
        .max_frame_size(WS_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_remote(socket, state, device_id, thread_id)))
}

/// Bridge one watched `(device, thread)` pair onto the browser socket.
async fn handle_remote(
    socket: WebSocket,
    _state: Arc<AppState>,
    device_id: String,
    thread_id: String,
) {
    let (mut sender, mut receiver) = socket.split();

    if registry().by_device(&device_id).is_none() {
        let _ = send_json(
            &mut sender,
            &json!({"type": REMOTE_FRAME_ERROR, "code": "NODE_OFFLINE", "thread_id": thread_id}),
        )
        .await;
        let _ = send_json(&mut sender, &json!({"type": REMOTE_FRAME_CLOSE, "reason": "node_offline"})).await;
        let _ = sender.send(Message::Close(None)).await;
        return;
    }

    // Thread watcher (bounded by the hub: 8 per node, 128 frames queued).
    let (thread_sub, mut thread_rx) = match remote::hub().subscribe(&device_id, &thread_id) {
        Ok(pair) => pair,
        Err(error) => {
            let _ = send_json(
                &mut sender,
                &json!({"type": REMOTE_FRAME_ERROR, "code": error.code()}),
            )
            .await;
            let _ = sender.send(Message::Close(None)).await;
            return;
        }
    };
    if thread_sub.first {
        // First watcher asks the node to start forwarding this thread; the node
        // answers with a snapshot frame and then deltas (docs §7.4).
        if let Some(live) = registry().by_device(&device_id) {
            let _ = registry().dispatch(
                &device_id,
                &live.channel_id,
                crate::services::interlink::OutboundFrame::Text(InterlinkEvent::attach(
                    &device_id, &thread_id,
                )),
            );
        }
    }

    loop {
        tokio::select! {
            frame = thread_rx.recv() => {
                let Some(frame) = frame else { break };
                if send_text(&mut sender, &frame).await.is_err() {
                    break;
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(PING_INTERVAL_S)) => {
                if sender.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    // The view is read-only; the browser drives the node with
                    // `POST /wunder/interlink/commands` instead (docs §7.4).
                    Some(Ok(_)) => {}
                }
            }
        }
    }

    let last = remote::hub().unsubscribe(&device_id, &thread_id, thread_sub.id);
    if last {
        // Nobody watches it any more: stop the upstream stream.
        let frame = InterlinkEvent::detach(&device_id, &thread_id);
        if let Some(live) = registry().by_device(&device_id) {
            let _ = registry().dispatch(
                &device_id,
                &live.channel_id,
                crate::services::interlink::OutboundFrame::Text(frame),
            );
        }
    }
    let _ = send_json(&mut sender, &json!({"type": REMOTE_FRAME_CLOSE, "reason": "unsubscribed"})).await;
    let _ = sender.send(Message::Close(None)).await;
}

/// Small builder for the two upstream notices the node understands.
struct InterlinkEvent;

impl InterlinkEvent {
    fn attach(device_id: &str, thread_id: &str) -> String {
        event_text(device_id, json!({"kind": "thread_attach", "thread_id": thread_id}))
    }

    fn detach(device_id: &str, thread_id: &str) -> String {
        event_text(device_id, json!({"kind": "thread_detach", "thread_id": thread_id}))
    }
}

fn event_text(_device_id: &str, payload: serde_json::Value) -> String {
    let frame = wunder_core::interlink::InterlinkFrame::new(
        FRAME_EVENT,
        format!("frm_{}", uuid::Uuid::new_v4().simple()),
        now_unix_seconds(),
        None,
        payload,
    );
    serde_json::to_string(&frame).unwrap_or_else(|_| "{}".to_string())
}

async fn send_json(sender: &mut SplitSink<WebSocket, Message>, value: &serde_json::Value) -> Result<(), ()> {
    send_text(sender, &value.to_string()).await
}

async fn send_text(sender: &mut SplitSink<WebSocket, Message>, text: &str) -> Result<(), ()> {
    sender.send(Message::Text(text.to_string().into())).await.map_err(|_| ())
}

fn now_unix_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_and_detach_notices_are_well_formed_frames() {
        let text = InterlinkEvent::attach("dev-1", "th_local_1");
        let value: serde_json::Value = serde_json::from_str(&text).expect("frame json");
        assert_eq!(value["type"], FRAME_EVENT);
        assert_eq!(value["payload"]["kind"], "thread_attach");
        assert_eq!(value["payload"]["thread_id"], "th_local_1");
        assert_eq!(value["v"], 1);

        let detach: serde_json::Value =
            serde_json::from_str(&InterlinkEvent::detach("dev-1", "th_local_1"))
                .expect("frame json");
        assert_eq!(detach["payload"]["kind"], "thread_detach");
    }

    #[test]
    fn remote_query_defaults_when_params_are_absent() {
        let query: RemoteQuery = serde_json::from_value(json!({})).expect("deserialize");
        assert!(query.target.is_none());
        assert!(query.thread.is_none());
    }
}
