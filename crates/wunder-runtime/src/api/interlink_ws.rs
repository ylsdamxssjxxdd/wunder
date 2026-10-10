//! Interlink tunnel websocket API (I4): ticket issuance + the local-node
//! tunnel handshake and heartbeat loop. See docs/云端本地互通方案.md §3.2, §4.1,
//! §4.2 and §5.1.
//!
//! Surface:
//! - `POST /wunder/interlink/channel_ticket` - one-time, device-bound, 60s
//!   ticket for the tunnel handshake.
//! - `WS   /wunder/interlink/ws?ticket=itk_<uuid>` - local node tunnel:
//!   `hello` -> `hello_ack` -> `ping`/`pong`.
//!
//! I4 owns `hello`/`hello_ack`/`ping`/`pong`; I5 adds the inbound
//! `shadow_full`/`shadow_delta` projection (`apply_shadow_frame`). Command/event
//! frames remain out of scope (I7+): the loop replies
//! `error{unsupported_frame}` for anything it does not own yet.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use wunder_core::interlink::{
    default_device_capabilities, InterlinkFrame, InterlinkHello, InterlinkHelloAck,
    CAP_AGENT_SPAWN, CAP_QUERY_BASIC, CAP_SHADOW_FULL, CAP_SHADOW_MINIMAL, CAP_THREAD_DRIVE,
    CAP_TOOL_EXEC, CAP_WORKSPACE_READ_BINARY, CAP_WORKSPACE_WRITE, FRAME_CLOSE, FRAME_ERROR,
    FRAME_HELLO, FRAME_HELLO_ACK, FRAME_PING, FRAME_PONG, FRAME_SHADOW_DELTA, FRAME_SHADOW_FULL,
    INTERLINK_PROTOCOL_VERSION,
};

use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::core::blocking;
use crate::services::interlink::{registry, shadow, LiveChannel};
use crate::state::AppState;
use crate::storage::{CloudDeviceInterlinkPatch, InterlinkChannelRecord};

/// Ticket lifetime (docs §3.2 / §4.1: 60s, one-time, bound to the device).
const TICKET_TTL_S: f64 = 60.0;
/// Device id header used by the cloud channel; the tunnel reuses it.
const DEVICE_HEADER: &str = "x-wunder-device-id";
/// Websocket subprotocol for the tunnel.
const WS_PROTOCOL: &str = "wunder-interlink";
const WS_MAX_MESSAGE_BYTES: usize = 512 * 1024;
/// Instance id stored on every channel row (single instance is `local`; the
/// column exists for future multi-replica routing, docs §3.1/§10.4).
const INSTANCE_ID: &str = "local";

/// User-facing interlink tunnel routes.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/interlink/channel_ticket", post(issue_ticket))
        .route("/wunder/interlink/ws", get(interlink_ws))
}

// ---------------------------------------------------------------------------
// One-time ticket store (process-local; lost on restart by design - the tunnel
// simply re-requests a ticket).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct TicketEntry {
    device_id: String,
    user_id: String,
    expires_at: f64,
}

#[derive(Debug, Default)]
struct TicketStore {
    tickets: RwLock<HashMap<String, TicketEntry>>,
}

impl TicketStore {
    fn insert(&self, ticket: String, entry: TicketEntry) {
        let mut guard = self.tickets.write().expect("ticket store lock poisoned");
        guard.insert(ticket, entry);
    }

    /// Remove and return a ticket, consuming it exactly once. Expired tickets
    /// (and any other expired leftovers) are dropped opportunistically.
    fn consume(&self, ticket: &str, now: f64) -> Option<TicketEntry> {
        let mut guard = self.tickets.write().expect("ticket store lock poisoned");
        guard.retain(|_, entry| entry.expires_at > now);
        guard.remove(ticket)
    }
}

fn ticket_store() -> &'static TicketStore {
    static INSTANCE: OnceLock<TicketStore> = OnceLock::new();
    INSTANCE.get_or_init(TicketStore::default)
}

// ---------------------------------------------------------------------------
// POST /wunder/interlink/channel_ticket
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct TicketRequest {
    #[serde(default)]
    device_id: Option<String>,
}

/// Issue a one-time tunnel ticket for a device owned by the caller.
async fn issue_ticket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Option<Json<TicketRequest>>,
) -> Response {
    let resolved = match resolve_user(&state, &headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return response,
    };
    let user_id = resolved.user.user_id.clone();

    let config = state.config_store.get().await;
    if !config.interlink.enabled {
        return error_response(
            StatusCode::NOT_FOUND,
            "interlink is disabled on this server".to_string(),
        );
    }
    drop(config);

    // The device may be named in the body or in the cloud device header.
    let device_id = body
        .and_then(|Json(request)| request.device_id)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            headers
                .get(DEVICE_HEADER)
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
    let Some(device_id) = device_id else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "device_id is required (json body or x-wunder-device-id header)".to_string(),
        );
    };

    let storage = state.storage.clone();
    let lookup = device_id.clone();
    let record = match blocking::run_db("api.interlink_ws.get_device", move || {
        storage.get_cloud_device(&lookup)
    })
    .await
    {
        Ok(record) => record,
        Err(err) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
        }
    };
    let Some(record) = record else {
        return device_error();
    };
    if record.revoked || record.user_id != user_id {
        return device_error();
    }
    // `interlink_enabled == None` means "not yet configured" -> treat as on.
    if record
        .interlink
        .as_deref()
        .and_then(|patch| patch.interlink_enabled)
        == Some(false)
    {
        return error_response(
            StatusCode::FORBIDDEN,
            "interlink is disabled for this device".to_string(),
        );
    }

    let now = now_unix_seconds();
    let ticket = format!("itk_{}", Uuid::new_v4().simple());
    ticket_store().insert(
        ticket.clone(),
        TicketEntry {
            device_id: device_id.clone(),
            user_id,
            expires_at: now + TICKET_TTL_S,
        },
    );

    Json(json!({
        "data": {
            "ticket": ticket,
            "expires_in": TICKET_TTL_S as i64,
            "device_id": device_id,
        }
    }))
    .into_response()
}

fn device_error() -> Response {
    error_response(
        StatusCode::UNAUTHORIZED,
        "device is revoked, unknown or not owned by this account".to_string(),
    )
}

// ---------------------------------------------------------------------------
// WS /wunder/interlink/ws
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct InterlinkWsQuery {
    #[serde(default)]
    ticket: Option<String>,
}

/// Upgrade a local node into a tunnel websocket.
async fn interlink_ws(
    State(state): State<Arc<AppState>>,
    Query(query): Query<InterlinkWsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let now = now_unix_seconds();
    let ticket = query
        .ticket
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "ticket is required".to_string()))?;
    let entry = ticket_store().consume(ticket, now).ok_or_else(|| {
        error_response(
            StatusCode::UNAUTHORIZED,
            "ticket is invalid or expired".to_string(),
        )
    })?;

    Ok(ws
        .protocols([WS_PROTOCOL])
        .max_message_size(WS_MAX_MESSAGE_BYTES)
        .max_frame_size(WS_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_ws(socket, state, entry)))
}

/// Drive the tunnel handshake and the steady-state heartbeat loop.
async fn handle_ws(socket: WebSocket, state: Arc<AppState>, ticket: TicketEntry) {
    let (mut sender, mut receiver) = socket.split();

    let config = state.config_store.get().await;
    if !config.interlink.enabled {
        let _ = send_close(&mut sender, "interlink_disabled", None).await;
        return;
    }
    let heartbeat_s = config.interlink.heartbeat_s.max(1);
    let max_channels = config.interlink.max_channels_per_device;
    drop(config);

    // 1) First frame must be `hello`.
    let hello = match read_hello(&mut receiver).await {
        Ok(hello) => hello,
        Err(reason) => {
            let _ = send_close(&mut sender, &reason, None).await;
            return;
        }
    };

    // 2) Handshake validation.
    if hello.device_id != ticket.device_id {
        let _ = send_close(&mut sender, "device_mismatch", None).await;
        return;
    }
    if hello.protocol != INTERLINK_PROTOCOL_VERSION {
        // Only protocol v1 exists today; a newer node asks us to upgrade and an
        // older one is unsupported (docs §4.1 version negotiation).
        let reason = if hello.protocol > INTERLINK_PROTOCOL_VERSION {
            "upgrade_required"
        } else {
            "protocol_unsupported"
        };
        let _ = send_close(&mut sender, reason, None).await;
        return;
    }
    // TODO(I4): 完整 HMAC 校验。P0 仅做基础校验(hmac 非空 + 设备归属)。
    // 完整方案见 docs §4.1: hmac = HMAC_SHA256(node_secret, ticket)，
    // 服务端另存 HMAC(server_pepper, node_secret) 以反推比对。
    if hello.hmac.trim().is_empty() {
        let _ = send_close(&mut sender, "hmac_required", None).await;
        return;
    }

    let now = now_unix_seconds();
    // Re-validate against storage: ownership, revocation and the node switch.
    let storage = state.storage.clone();
    let lookup = hello.device_id.clone();
    let record = match blocking::run_db("api.interlink_ws.get_device", move || {
        storage.get_cloud_device(&lookup)
    })
    .await
    {
        Ok(record) => record,
        Err(_) => {
            let _ = send_close(&mut sender, "storage_error", None).await;
            return;
        }
    };
    let Some(record) = record else {
        let _ = send_close(&mut sender, "device_unknown", None).await;
        return;
    };
    if record.revoked || record.user_id != ticket.user_id {
        let _ = send_close(&mut sender, "device_revoked", None).await;
        return;
    }
    if record
        .interlink
        .as_deref()
        .and_then(|patch| patch.interlink_enabled)
        == Some(false)
    {
        let _ = send_close(&mut sender, "interlink_disabled", None).await;
        return;
    }
    if max_channels == 0 {
        let _ = send_close(&mut sender, "channel_limit", None).await;
        return;
    }

    // 3) Grant capabilities: intersect the declared set with the server-known
    //    set (admin policy overrides land in a later stage, docs §9.2).
    let declared = if hello.capabilities.is_empty() {
        default_device_capabilities()
    } else {
        hello.capabilities.clone()
    };
    let granted = grant_capabilities(&declared);

    let channel_id = format!("ch_{}", Uuid::new_v4().simple());
    let connection_id = format!("itl_{}", Uuid::new_v4().simple());
    let live = LiveChannel {
        device_id: hello.device_id.clone(),
        user_id: ticket.user_id.clone(),
        client: record.client.clone(),
        channel_id: channel_id.clone(),
        connection_id,
        protocol_version: hello.protocol,
        capabilities: granted.clone(),
        connected_at: now,
        last_seen_at: now,
        rtt_ms: None,
    };

    // New chain supersedes the old one for the same device.
    let _superseded = registry().register(live.clone());

    // Persist the channel snapshot and flip the device tunnel flag; the node
    // list reads `tunnel_connected` for its `connected` field (docs §5.1).
    upsert_channel(&state, &live).await;
    set_tunnel_connected(&state, &live.device_id, true, now).await;

    // 4) hello_ack.
    let shadow_revision = shadow_revision(&state, &live.device_id).await;
    let ack = InterlinkHelloAck {
        channel_id: channel_id.clone(),
        protocol: INTERLINK_PROTOCOL_VERSION,
        capabilities_granted: granted,
        server_time: now,
        shadow_revision,
    };
    if send_frame(
        &mut sender,
        FRAME_HELLO_ACK,
        Some(&channel_id),
        None,
        serde_json::to_value(&ack).unwrap_or_else(|_| json!({})),
    )
    .await
    .is_err()
    {
        teardown(&state, &live, "send_failed").await;
        return;
    }

    // 5) Steady-state heartbeat loop.
    let mut ticker = tokio::time::interval(Duration::from_secs(heartbeat_s));
    // Consume the immediate first tick produced by `interval`.
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let beat_now = now_unix_seconds();
                if !registry().heartbeat(&live.device_id, &channel_id, beat_now, None) {
                    let _ = send_close(&mut sender, "superseded", Some(&channel_id)).await;
                    break;
                }
                persist_heartbeat(&state, &live.device_id, &channel_id, beat_now).await;
                if send_frame(&mut sender, FRAME_PING, Some(&channel_id), None, json!({})).await.is_err() {
                    break;
                }
            }
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let frame = match serde_json::from_str::<InterlinkFrame>(&text) {
                            Ok(frame) => frame,
                            Err(_) => {
                                let _ = send_frame(&mut sender, FRAME_ERROR, Some(&channel_id), None, json!({"message": "bad_frame"})).await;
                                continue;
                            }
                        };
                        match frame.kind.as_str() {
                            FRAME_PING => {
                                let beat_now = now_unix_seconds();
                                if !registry().heartbeat(&live.device_id, &channel_id, beat_now, None) {
                                    let _ = send_close(&mut sender, "superseded", Some(&channel_id)).await;
                                    break;
                                }
                                persist_heartbeat(&state, &live.device_id, &channel_id, beat_now).await;
                                if send_frame(&mut sender, FRAME_PONG, Some(&channel_id), frame.corr.as_deref(), json!({})).await.is_err() {
                                    break;
                                }
                            }
                            FRAME_PONG => {
                                let beat_now = now_unix_seconds();
                                // RTT is derived from the `ts` we stamped on our ping.
                                let rtt_ms = if frame.ts > 0.0 && beat_now >= frame.ts {
                                    Some((((beat_now - frame.ts) * 1000.0).round() as i64).max(0))
                                } else {
                                    None
                                };
                                if !registry().heartbeat(&live.device_id, &channel_id, beat_now, rtt_ms) {
                                    let _ = send_close(&mut sender, "superseded", Some(&channel_id)).await;
                                    break;
                                }
                                persist_heartbeat(&state, &live.device_id, &channel_id, beat_now).await;
                            }
                            FRAME_CLOSE => break,
                            FRAME_SHADOW_FULL | FRAME_SHADOW_DELTA => {
                                if let Some(code) = apply_shadow_frame(&state, &live, &frame).await {
                                    let _ = send_frame(
                                        &mut sender,
                                        FRAME_ERROR,
                                        Some(&channel_id),
                                        frame.corr.as_deref(),
                                        json!({"message": code, "type": frame.kind}),
                                    ).await;
                                }
                            }
                            _ => {
                                // I7+ owns command/event frames.
                                let _ = send_frame(
                                    &mut sender,
                                    FRAME_ERROR,
                                    Some(&channel_id),
                                    frame.corr.as_deref(),
                                    json!({"message": "unsupported_frame", "type": frame.kind}),
                                ).await;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        // Transport-level keepalive: mirror it with a pong.
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }

    teardown(&state, &live, "closed").await;
}

// ---------------------------------------------------------------------------
// Frame helpers
// ---------------------------------------------------------------------------

fn build_frame(
    kind: &str,
    channel_id: Option<&str>,
    corr: Option<&str>,
    payload: Value,
) -> InterlinkFrame {
    let mut frame = InterlinkFrame::new(
        kind,
        format!("frm_{}", Uuid::new_v4().simple()),
        now_unix_seconds(),
        channel_id.map(str::to_string),
        payload,
    );
    if let Some(corr) = corr {
        frame = frame.with_corr(corr);
    }
    frame
}

async fn send_frame(
    sender: &mut SplitSink<WebSocket, Message>,
    kind: &str,
    channel_id: Option<&str>,
    corr: Option<&str>,
    payload: Value,
) -> Result<(), ()> {
    let frame = build_frame(kind, channel_id, corr, payload);
    let text = serde_json::to_string(&frame).map_err(|_| ())?;
    sender
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

/// Emit a `close` frame carrying `reason`, then drop the websocket.
async fn send_close(
    sender: &mut SplitSink<WebSocket, Message>,
    reason: &str,
    channel_id: Option<&str>,
) -> Result<(), ()> {
    let _ = send_frame(
        sender,
        FRAME_CLOSE,
        channel_id,
        None,
        json!({ "reason": reason }),
    )
    .await;
    sender.send(Message::Close(None)).await.map_err(|_| ())
}

/// Read the mandatory first frame and decode it as `hello`.
async fn read_hello(receiver: &mut SplitStream<WebSocket>) -> Result<InterlinkHello, String> {
    loop {
        match receiver.next().await {
            Some(Ok(Message::Text(text))) => {
                let frame: InterlinkFrame =
                    serde_json::from_str(&text).map_err(|_| "bad_frame".to_string())?;
                if frame.kind != FRAME_HELLO {
                    return Err("expected_hello".to_string());
                }
                let hello: InterlinkHello =
                    serde_json::from_value(frame.payload).map_err(|_| "bad_hello".to_string())?;
                return Ok(hello);
            }
            Some(Ok(Message::Binary(_))) => return Err("unexpected_binary".to_string()),
            Some(Ok(Message::Close(_))) | None => return Err("closed_before_hello".to_string()),
            // Pre-hello control frames (ping/pong/other) are ignored.
            Some(Ok(_)) => continue,
            Some(Err(_)) => return Err("socket_error".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Capability grant + storage helpers
// ---------------------------------------------------------------------------

/// The capability names the server recognises (docs §9.2). Declared names
/// outside this set are dropped rather than echoed back.
fn known_capabilities() -> [&'static str; 8] {
    [
        CAP_QUERY_BASIC,
        CAP_WORKSPACE_READ_BINARY,
        CAP_SHADOW_FULL,
        CAP_SHADOW_MINIMAL,
        CAP_THREAD_DRIVE,
        CAP_WORKSPACE_WRITE,
        CAP_TOOL_EXEC,
        CAP_AGENT_SPAWN,
    ]
}

/// Intersect a declared capability list with the server-known set, preserving
/// order and dropping duplicates/unknown names.
fn grant_capabilities(declared: &[String]) -> Vec<String> {
    let known = known_capabilities();
    let mut granted: Vec<String> = Vec::new();
    for cap in declared {
        let cap = cap.trim();
        if cap.is_empty() || !known.contains(&cap) {
            continue;
        }
        if !granted.iter().any(|existing| existing == cap) {
            granted.push(cap.to_string());
        }
    }
    granted
}

async fn upsert_channel(state: &AppState, live: &LiveChannel) {
    let storage = state.storage.clone();
    let record = InterlinkChannelRecord {
        channel_id: live.channel_id.clone(),
        device_id: live.device_id.clone(),
        user_id: live.user_id.clone(),
        client: live.client.clone(),
        instance_id: INSTANCE_ID.to_string(),
        protocol_version: live.protocol_version,
        caps: serde_json::to_string(&live.capabilities).ok(),
        connected_at: live.connected_at,
        last_seen_at: live.last_seen_at,
        rtt_ms: live.rtt_ms,
        resumed_count: 0,
        closed_reason: None,
    };
    let _ = blocking::run_db("api.interlink_ws.upsert_channel", move || {
        storage.upsert_interlink_channel(&record)
    })
    .await;
}

/// Heartbeat persistence: touch the device `last_seen_at` and advance the
/// channel row's `last_seen_at` (read-modify-write keeps the other columns).
async fn persist_heartbeat(state: &AppState, device_id: &str, channel_id: &str, now: f64) {
    let storage = state.storage.clone();
    let lookup = device_id.to_string();
    let _ = blocking::run_db("api.interlink_ws.touch_device", move || {
        storage.touch_cloud_device(&lookup, now)
    })
    .await;

    let storage = state.storage.clone();
    let lookup = channel_id.to_string();
    let _ = blocking::run_db("api.interlink_ws.channel_beat", move || {
        if let Some(mut record) = storage.get_interlink_channel(&lookup)? {
            record.last_seen_at = now;
            storage.upsert_interlink_channel(&record)?;
        }
        Ok::<(), anyhow::Error>(())
    })
    .await;
}

/// Flip the persisted tunnel flag consumed by `GET /wunder/interlink/nodes`.
async fn set_tunnel_connected(state: &AppState, device_id: &str, connected: bool, now: f64) {
    let storage = state.storage.clone();
    let lookup = device_id.to_string();
    let patch = CloudDeviceInterlinkPatch {
        tunnel_connected: Some(connected),
        last_tunnel_at: if connected { Some(now) } else { None },
        ..Default::default()
    };
    let _ = blocking::run_db("api.interlink_ws.set_tunnel", move || {
        storage.update_cloud_device_interlink(&lookup, &patch)
    })
    .await;
}

/// Project one shadow frame into `interlink_node_shadows` (I5).
///
/// Reads the stored shadow as the merge base, delegates the revision/merge
/// rules to `services::interlink::shadow`, and upserts the result. Returns a
/// stable error code when the frame must be rejected (`invalid_revision` /
/// `payload_too_large`), else `None`. Stale deltas are silently ignored.
async fn apply_shadow_frame(
    state: &AppState,
    live: &LiveChannel,
    frame: &InterlinkFrame,
) -> Option<&'static str> {
    let storage = state.storage.clone();
    let lookup = live.device_id.clone();
    let previous = match blocking::run_db("api.interlink_ws.shadow_read", move || {
        storage.get_interlink_shadow(&lookup)
    })
    .await
    {
        Ok(previous) => previous,
        Err(_) => return Some("storage_error"),
    };

    let now = now_unix_seconds();
    match shadow::project_shadow_frame(
        &frame.kind,
        &frame.payload,
        &live.device_id,
        &live.user_id,
        previous.as_ref(),
        now,
    ) {
        shadow::ShadowOutcome::Apply(record) => {
            let storage = state.storage.clone();
            let _ = blocking::run_db("api.interlink_ws.shadow_upsert", move || {
                storage.upsert_interlink_shadow(&record)
            })
            .await;
            None
        }
        shadow::ShadowOutcome::Ignore => None,
        shadow::ShadowOutcome::Reject(code) => Some(code),
    }
}

async fn shadow_revision(state: &AppState, device_id: &str) -> i64 {
    let storage = state.storage.clone();
    let lookup = device_id.to_string();
    blocking::run_db("api.interlink_ws.shadow_revision", move || {
        storage.get_interlink_shadow_revision(&lookup)
    })
    .await
    .unwrap_or(0)
}

/// Close a channel: drop it from the registry, mark the channel row closed and
/// clear the device tunnel flag (only when this handler still owned the slot,
/// so a superseding channel is never torn down).
async fn teardown(state: &AppState, live: &LiveChannel, reason: &str) {
    let owned = registry().unregister(&live.device_id, &live.channel_id);

    let storage = state.storage.clone();
    let channel_id = live.channel_id.clone();
    let reason_owned = reason.to_string();
    let _ = blocking::run_db("api.interlink_ws.close_channel", move || {
        storage.close_interlink_channel(&channel_id, &reason_owned)
    })
    .await;

    if owned {
        set_tunnel_connected(state, &live.device_id, false, 0.0).await;
    }
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
    fn ticket_store_is_single_use_and_expires() {
        let store = TicketStore::default();
        store.insert(
            "itk_a".to_string(),
            TicketEntry {
                device_id: "d1".to_string(),
                user_id: "u1".to_string(),
                expires_at: 100.0,
            },
        );
        assert!(store.consume("itk_a", 50.0).is_some());
        // Consumed once: the second attempt finds nothing.
        assert!(store.consume("itk_a", 50.0).is_none());

        store.insert(
            "itk_b".to_string(),
            TicketEntry {
                device_id: "d1".to_string(),
                user_id: "u1".to_string(),
                expires_at: 100.0,
            },
        );
        // Expired tickets are pruned.
        assert!(store.consume("itk_b", 200.0).is_none());
        assert!(store.consume("itk_b", 201.0).is_none());
    }

    #[test]
    fn grant_capabilities_drops_unknown_and_duplicates() {
        let declared = vec![
            CAP_QUERY_BASIC.to_string(),
            CAP_QUERY_BASIC.to_string(),
            "totally.unknown".to_string(),
            CAP_THREAD_DRIVE.to_string(),
        ];
        let granted = grant_capabilities(&declared);
        assert_eq!(granted, vec![CAP_QUERY_BASIC, CAP_THREAD_DRIVE]);
    }

    #[test]
    fn build_frame_sets_type_and_channel() {
        let frame = build_frame(FRAME_PING, Some("ch_1"), Some("cmd_1"), json!({}));
        let value = serde_json::to_value(&frame).expect("serialize");
        assert_eq!(value["type"], FRAME_PING);
        assert_eq!(value["channel_id"], "ch_1");
        assert_eq!(value["corr"], "cmd_1");
    }
}