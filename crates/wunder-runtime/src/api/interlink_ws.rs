//! Interlink tunnel websocket API (I4/I7): ticket issuance, the authenticated
//! handshake, and the steady-state loop that carries heartbeats, workspace
//! shadows, remote command frames and the file data plane.
//!
//! Surface:
//! - `POST /wunder/interlink/channel_ticket` - one-time, device-bound, 60s
//!   ticket for the tunnel handshake.
//! - `WS   /wunder/interlink/ws?ticket=itk_<uuid>` - the local node tunnel.
//!
//! Frame contract (docs §4.2): `hello`/`hello_ack`/`ping`/`pong`/`close`,
//! `shadow_full`/`shadow_delta`, `command`/`command_ack`/`command_event`/
//! `command_result`, `event` (presence beats and forwarded thread events) and
//! binary data frames for large file reads. Anything else is refused with
//! `error{unsupported_frame}` - the protocol has no forgiving path.
//!
//! The server never blocks on the socket: outbound frames go through a bounded
//! per-channel queue (docs §10.1/§10.2).

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
    AUDIT_CHANNEL_CLOSE, AUDIT_CHANNEL_OPEN, AUDIT_CHANNEL_REJECTED, AUDIT_FILE_READ,
    AUDIT_SHADOW_SYNC, CAP_AGENT_SPAWN, CAP_QUERY_BASIC, CAP_SHADOW_FULL, CAP_SHADOW_MINIMAL,
    CAP_THREAD_DRIVE, CAP_TOOL_EXEC, CAP_WORKSPACE_READ_BINARY, CAP_WORKSPACE_WRITE,
    EVENT_PRESENCE, EVENT_THREAD, FRAME_CLOSE, FRAME_COMMAND_ACK, FRAME_COMMAND_EVENT,
    FRAME_COMMAND_RESULT, FRAME_ERROR, FRAME_EVENT, FRAME_HELLO, FRAME_HELLO_ACK, FRAME_PING,
    FRAME_PONG, FRAME_SHADOW_DELTA, FRAME_SHADOW_FULL, INTERLINK_PROTOCOL_VERSION, InterlinkFrame,
    InterlinkHello, InterlinkHelloAck,
    NODE_STATUS_AWAY, NODE_STATUS_BUSY, NODE_STATUS_ONLINE, REMOTE_FRAME_DELTA, TUNNEL_WS_PROTOCOL,
    default_device_capabilities,
};

use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::core::blocking;
use crate::services::interlink::{
    LiveChannel, LiveChannelRegistry, OutboundFrame, alerts, audit, blob, commands, digest,
    registry, remote, secret, shadow,
};
use crate::state::AppState;
use crate::storage::{CloudDeviceInterlinkPatch, InterlinkChannelRecord};

/// Ticket lifetime (docs §3.2 / §4.1: 60s, one-time, bound to the device).
const TICKET_TTL_S: f64 = 60.0;
/// Device id header used by the cloud channel; the tunnel reuses it.
const DEVICE_HEADER: &str = "x-wunder-device-id";
/// Message size cap for one tunnel frame.
pub const WS_MAX_MESSAGE_BYTES: usize = 512 * 1024;
/// Instance id stored on every channel row (single instance is `local`; the
/// column exists for future multi-replica routing, docs §3.1/§10.4).
pub const INSTANCE_ID: &str = "local";
/// Proximity header a reverse proxy sets; used to bind a ticket to its source.
const FORWARDED_HEADERS: [&str; 2] = ["x-forwarded-for", "x-real-ip"];
/// Beats older than this many seconds mark the node `away` (docs §5.1).
const AWAY_IDLE_S: f64 = 600.0;

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
    /// The literal ticket string; the node signs it, so the server must keep it
    /// until the handshake consumes it (docs §4.1).
    ticket: String,
    device_id: String,
    user_id: String,
    expires_at: f64,
    /// Source address of the issuer, when observable (docs §9.1).
    issued_from: Option<String>,
}

#[derive(Debug, Default)]
struct TicketStore {
    tickets: RwLock<HashMap<String, TicketEntry>>,
}

impl TicketStore {
    fn insert(&self, ticket: String, entry: TicketEntry) {
        let mut guard = self.tickets.write().expect("ticket store lock poisoned");
        // Bounded store: expired tickets are pruned on every write so a client
        // that never connects cannot grow this map.
        let now = now_unix_seconds();
        guard.retain(|_, entry| entry.expires_at > now);
        guard.insert(ticket, entry);
    }

    /// Remove and return a ticket, consuming it exactly once.
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
        .or_else(|| header_value(&headers, DEVICE_HEADER));
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
    if !interlink_allowed(&record.interlink) {
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
            ticket: ticket.clone(),
            device_id: device_id.clone(),
            user_id,
            expires_at: now + TICKET_TTL_S,
            issued_from: source_address(&headers),
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

/// `interlink_enabled == None` means "not yet configured" -> treat as on.
fn interlink_allowed(patch: &Option<Box<CloudDeviceInterlinkPatch>>) -> bool {
    patch
        .as_deref()
        .and_then(|patch| patch.interlink_enabled)
        != Some(false)
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
    headers: HeaderMap,
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
    // The ticket is bound to the address it was minted for (docs §9.1). A
    // server without a visible source address cannot enforce this and proceeds.
    if let (Some(issued_from), Some(presented)) = (
        entry.issued_from.as_deref(),
        source_address(&headers).as_deref(),
    ) {
        if issued_from != presented {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "ticket was issued for a different source address".to_string(),
            ));
        }
    }

    Ok(ws
        .protocols([TUNNEL_WS_PROTOCOL])
        .max_message_size(WS_MAX_MESSAGE_BYTES)
        .max_frame_size(WS_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_ws(socket, state, entry)))
}

/// Drive the tunnel handshake and the steady-state loop.
async fn handle_ws(socket: WebSocket, state: Arc<AppState>, ticket: TicketEntry) {
    let (mut sender, mut receiver) = socket.split();

    let config = state.config_store.get().await;
    if !config.interlink.enabled {
        drop(config);
        let _ = send_close(&mut sender, "interlink_disabled", None).await;
        return;
    }
    let heartbeat_s = config.interlink.heartbeat_s.max(1);
    let max_channels = config.interlink.max_channels_per_device;
    let _presence_ttl_s = config.interlink.presence_ttl_s.max(1) as f64;
    let limits = commands::Limits::from_config(&config.interlink);
    let shadow_limits = shadow_limit_config(&config.interlink.shadow);
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
    if !interlink_allowed(&record.interlink) {
        let _ = send_close(&mut sender, "interlink_disabled", None).await;
        return;
    }
    if max_channels == 0 {
        let _ = send_close(&mut sender, "channel_limit", None).await;
        return;
    }

    // 3) Node secret handshake (docs §4.1/§9.1): the server holds only a
    //    fingerprint, but the secret is derivable from the pepper, so the
    //    presented MAC can be re-computed without ever storing the secret.
    let patch = record.interlink.clone().unwrap_or_default();
    let stored_hash_input = patch.node_secret_hash.clone();
    let stored_version_input = patch.secret_version;
    let rotated_at_input = patch.secret_rotated_at.unwrap_or(0.0);
    let (pepper, stored_hash, stored_version, rotated_at) = {
        let storage = state.storage.clone();
        match blocking::run_db("api.interlink_ws.pepper", move || {
            let pepper = secret::ensure_pepper(&storage)?;
            Ok::<(String, Option<String>, i64, f64), anyhow::Error>((
                pepper,
                stored_hash_input,
                stored_version_input,
                rotated_at_input,
            ))
        })
        .await
        {
            Ok(values) => values,
            Err(_) => {
                let _ = send_close(&mut sender, "storage_error", None).await;
                return;
            }
        }
    };
    let Some(stored_hash) = stored_hash else {
        // No key was ever issued for this node: it must fetch one over the
        // authenticated REST surface before it can open a tunnel.
        let _ = send_close(&mut sender, "secret_required", None).await;
        return;
    };
    let outcome = secret::verify_handshake(
        &pepper,
        &hello.device_id,
        &stored_hash,
        stored_version,
        rotated_at,
        hello.secret_version,
        &ticket.ticket,
        hello.hmac.trim(),
        now,
    );
    if outcome != secret::HandshakeOutcome::Ok {
        let reason = match outcome {
            secret::HandshakeOutcome::UnknownVersion => "secret_version_unknown",
            // A retired key past the dual-key window: refuse and alert
            // (docs §13.5 20). The audit reason is what the hook matches on.
            secret::HandshakeOutcome::StaleVersion => alerts::REASON_SECRET_STALE_VERSION,
            _ => "hmac_mismatch",
        };
        let _ = send_close(&mut sender, reason, None).await;
        // A mismatch is exactly the "stolen token tries to impersonate a node"
        // signal the threat model calls out (docs §9.5) - leave an audit trail.
        write_audit(
            &state,
            audit::record(
                AUDIT_CHANNEL_REJECTED,
                &ticket.user_id,
                None,
                Some(&format!("device:{}", hello.device_id)),
                None,
                None,
                vec![("reason", json!(reason))],
            ),
        )
        .await;
        return;
    }

    // 4) Grant capabilities: declared ∩ known ∩ authorized ∩ admin policy
    //    (docs §9.2 - three-way intersection, never a superset).
    let declared = if hello.capabilities.is_empty() {
        default_device_capabilities()
    } else {
        hello.capabilities.clone()
    };
    let policy = digest::DevicePolicy::parse(patch.policy_overrides.as_deref());
    let authorized = patch
        .capabilities
        .as_deref()
        .map(|raw| parse_string_array(raw))
        .unwrap_or_else(default_device_capabilities);
    let granted = grant_capabilities(&declared, &authorized, &policy);

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
        presence_status: Some(NODE_STATUS_ONLINE.to_string()),
        active_threads: 0,
        resumed: false,
    };

    // New chain supersedes the old one for the same device.
    let (outbound_tx, mut outbound_rx) = LiveChannelRegistry::outbound_channel();
    let _superseded = registry().register(live.clone(), outbound_tx);

    // Persist the channel snapshot and flip the device tunnel flag; the node
    // list reads `tunnel_connected` for its `connected` field (docs §5.1).
    upsert_channel(&state, &live).await;
    set_tunnel_connected(&state, &live.device_id, true, now).await;
    write_audit(
        &state,
        audit::record(
            AUDIT_CHANNEL_OPEN,
            &live.user_id,
            None,
            Some(&format!("device:{}", live.device_id)),
            None,
            None,
            vec![
                ("channel_id", json!(channel_id)),
                ("caps", json!(granted)),
                ("protocol", json!(hello.protocol)),
            ],
        ),
    )
    .await;

    // Session recovery (docs §4.4): count the resume on the previous row.
    if let Some(previous_channel) = hello
        .resume_channel_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if resume_channel(&state, previous_channel).await {
            registry().mark_resumed(&live.device_id, &channel_id);
        }
    }

    // 5) hello_ack.
    let shadow_revision = shadow_revision(&state, &live.device_id).await;
    let ack = InterlinkHelloAck {
        channel_id: channel_id.clone(),
        protocol: INTERLINK_PROTOCOL_VERSION,
        capabilities_granted: granted.clone(),
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

    // 6) Tell the user's other live nodes a device joined (docs §5.3) and push
    //    anything that was parked for this node (docs §4.3 offline queue).
    broadcast_node_joined(&live).await;
    let _ = commands::drain(state.storage.clone(), &live.user_id, &live.device_id, &limits).await;

    // 7) Steady-state loop.
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
            queued = outbound_rx.recv() => {
                let Some(frame) = queued else {
                    // The registry dropped our queue: the slot is gone.
                    break;
                };
                let sent = match frame {
                    OutboundFrame::Text(text) => sender.send(Message::Text(text.into())).await,
                    OutboundFrame::Binary(bytes) => sender.send(Message::Binary(bytes.into())).await,
                };
                if sent.is_err() {
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
                        if !handle_inbound(&state, &mut sender, &live, &limits, &shadow_limits, frame).await {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if !handle_data_frame(&state, &live, &bytes).await {
                            break;
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

/// Handle one inbound JSON frame. Returns `false` when the loop must stop.
async fn handle_inbound(
    state: &Arc<AppState>,
    sender: &mut SplitSink<WebSocket, Message>,
    live: &LiveChannel,
    limits: &commands::Limits,
    shadow_limits: &shadow::ShadowLimits,
    frame: InterlinkFrame,
) -> bool {
    let channel_id = live.channel_id.as_str();
    let corr = frame.corr.as_deref().unwrap_or_default();
    match frame.kind.as_str() {
        FRAME_PING | FRAME_PONG => {
            let beat_now = now_unix_seconds();
            // RTT is derived from the `ts` we stamped on our ping.
            let rtt_ms = if frame.kind == FRAME_PONG && frame.ts > 0.0 && beat_now >= frame.ts {
                Some((((beat_now - frame.ts) * 1000.0).round() as i64).max(0))
            } else {
                None
            };
            if rtt_ms.is_none() {
                if !registry().heartbeat(&live.device_id, channel_id, beat_now, None) {
                    let _ = send_close(sender, "superseded", Some(channel_id)).await;
                    return false;
                }
                persist_heartbeat(state, &live.device_id, channel_id, beat_now).await;
            } else if !registry().heartbeat(&live.device_id, channel_id, beat_now, rtt_ms) {
                let _ = send_close(sender, "superseded", Some(channel_id)).await;
                return false;
            }
            if frame.kind == FRAME_PING
                && send_frame(sender, FRAME_PONG, Some(channel_id), frame.corr.as_deref(), json!({}))
                    .await
                    .is_err()
            {
                return false;
            }
            true
        }
        FRAME_CLOSE => false,
        FRAME_SHADOW_FULL | FRAME_SHADOW_DELTA => {
            if let Some(code) = apply_shadow_frame(state, live, &frame, shadow_limits).await {
                let _ = send_frame(
                    sender,
                    FRAME_ERROR,
                    Some(channel_id),
                    frame.corr.as_deref(),
                    json!({"message": code, "type": frame.kind}),
                )
                .await;
            }
            true
        }
        FRAME_COMMAND_ACK => {
            match commands::on_ack(state.storage.clone(), corr, &frame.payload, limits, now_unix_seconds()).await {
                Ok(commands::AckOutcome::Late(status)) => {
                    let _ = send_frame(
                        sender,
                        FRAME_ERROR,
                        Some(channel_id),
                        frame.corr.as_deref(),
                        json!({"message": "command_already_finished", "status": status}),
                    )
                    .await;
                }
                Ok(_) => {}
                Err(_) => {
                    let _ = send_frame(
                        sender,
                        FRAME_ERROR,
                        Some(channel_id),
                        frame.corr.as_deref(),
                        json!({"message": "storage_error"}),
                    )
                    .await;
                }
            }
            true
        }
        FRAME_COMMAND_EVENT => {
            // A `stream_open` event reserves the data-plane buffer first, so
            // binary chunks that follow it have somewhere to land (docs §6.4).
            if frame.payload.get("stream_open").is_some() {
                if let Err(code) = open_stream(corr, &frame.payload).await {
                    let _ = send_frame(
                        sender,
                        FRAME_ERROR,
                        Some(channel_id),
                        frame.corr.as_deref(),
                        json!({"message": code}),
                    )
                    .await;
                    return true;
                }
            }
            let _ = commands::on_event(state.storage.clone(), corr, &frame.payload, now_unix_seconds()).await;
            true
        }
        FRAME_COMMAND_RESULT => {
            if let Some(inline) = frame.payload.get("inline").and_then(Value::as_str) {
                store_inline(state, live, corr, inline, &frame.payload).await;
            }
            let _ = commands::on_result(
                state.storage.clone(),
                corr,
                &frame.payload,
                limits,
                now_unix_seconds(),
            )
            .await;
            true
        }
        FRAME_EVENT => handle_event_frame(state, live, &frame).await,
        _ => {
            // Strict protocol: nothing is silently tolerated (docs §14).
            let _ = send_frame(
                sender,
                FRAME_ERROR,
                Some(channel_id),
                frame.corr.as_deref(),
                json!({"message": "unsupported_frame", "type": frame.kind}),
            )
            .await;
            true
        }
    }
}

/// `event` frames carry presence beats and forwarded thread events (docs §4.4,
/// §7.4). Both are read-only observations: they never change command state.
async fn handle_event_frame(
    state: &Arc<AppState>,
    live: &LiveChannel,
    frame: &InterlinkFrame,
) -> bool {
    let now = now_unix_seconds();
    match frame.payload.get("kind").and_then(Value::as_str) {
        Some(EVENT_PRESENCE) => {
            let active_threads = frame
                .payload
                .get("active_threads")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let idle_s = frame
                .payload
                .get("idle_s")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let reported = frame.payload.get("status").and_then(Value::as_str);
            let status = match reported {
                Some(NODE_STATUS_BUSY) | Some(NODE_STATUS_AWAY) => reported.unwrap(),
                _ if active_threads > 0 => NODE_STATUS_BUSY,
                _ if idle_s > AWAY_IDLE_S => NODE_STATUS_AWAY,
                _ => NODE_STATUS_ONLINE,
            };
            if !registry().set_presence(&live.device_id, &live.channel_id, status, active_threads, now) {
                return false;
            }
            // The beat doubles as a liveness touch so presence stays correct
            // between pings (docs §5.1).
            persist_heartbeat(state, &live.device_id, &live.channel_id, now).await;
            true
        }
        Some(EVENT_THREAD) => {
            let Some(thread_id) = frame.payload.get("thread_id").and_then(Value::as_str) else {
                return true;
            };
            // Fan-out only; a node that sends events nobody watches is ignored
            // rather than buffered (docs §7.4 "no upstream amplification").
            // The first frame after an attach is the node's thread snapshot;
            // everything after it is a delta (docs §7.4 Snapshot -> Delta).
            let frame_type = if frame
                .payload
                .get("snapshot")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                wunder_core::interlink::REMOTE_FRAME_SNAPSHOT
            } else {
                REMOTE_FRAME_DELTA
            };
            let text = serde_json::to_string(&json!({
                "type": frame_type,
                "thread_id": thread_id,
                "seq": frame.payload.get("seq").cloned().unwrap_or(Value::Null),
                "payload": frame.payload.get("payload").cloned().unwrap_or(Value::Null),
            }))
            .unwrap_or_default();
            remote::hub().publish(&live.device_id, thread_id, &text);
            true
        }
        _ => true
    }
}

/// Register a data-plane stream announced by `command_event`.
async fn open_stream(command_id: &str, payload: &Value) -> Result<(), &'static str> {
    let spec = payload
        .get("stream_open")
        .and_then(Value::as_object)
        .ok_or("bad_stream_open")?;
    let stream_id = spec
        .get("stream_id")
        .and_then(Value::as_u64)
        .ok_or("stream_id_required")?;
    let declared_size = spec.get("size").and_then(Value::as_u64);
    let mime = spec.get("mime").and_then(Value::as_str);
    blob::store()
        .open(stream_id, command_id, declared_size, mime)
        .map_err(|error| error.code())
}

/// Persist an inline (<= 1 MiB) file result so the same blob endpoint serves
/// both transfer shapes (docs §6.4).
async fn store_inline(state: &Arc<AppState>, live: &LiveChannel, command_id: &str, inline: &str, payload: &Value) {
    let decoded = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(inline.as_bytes())
    };
    let Ok(bytes) = decoded else {
        return;
    };
    let mime = payload.get("mime").and_then(Value::as_str);
    let max_bytes = payload
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(blob::MAX_STREAM_BYTES as u64);
    if bytes.len() as u64 > max_bytes {
        let _ = live;
        return;
    }
    if blob::store()
        .store_inline(command_id, bytes.clone(), mime)
        .is_err()
    {
        return;
    }
    write_audit(
        state,
        audit::record(
            AUDIT_FILE_READ,
            &live.user_id,
            Some(&format!("device:{}", live.device_id)),
            Some("cloud"),
            Some(command_id),
            None,
            vec![
                ("size", json!(bytes.len())),
                ("sha256", json!(secret::sha256_hex(&bytes)[..16])),
                ("transport", json!("inline")),
            ],
        ),
    )
    .await;
}

/// Handle one inbound binary data frame (docs §4.2 header layout).
async fn handle_data_frame(state: &Arc<AppState>, live: &LiveChannel, bytes: &[u8]) -> bool {
    let Some((header, payload)) = blob::parse_header(bytes) else {
        return true;
    };
    match blob::store().push(&header, payload) {
        Ok(blob::ChunkOutcome::Complete { size, .. }) => {
            let digest = blob::store()
                .get_by_stream(header.stream_id)
                .map(|buffer| secret::sha256_hex(&buffer)[..16].to_string())
                .unwrap_or_default();
            write_audit(
                state,
                audit::record(
                    AUDIT_FILE_READ,
                    &live.user_id,
                    Some(&format!("device:{}", live.device_id)),
                    Some("cloud"),
                    None,
                    None,
                    vec![
                        ("stream_id", json!(header.stream_id)),
                        ("size", json!(size)),
                        ("sha256", json!(digest)),
                        ("transport", json!("chunks")),
                    ],
                ),
            )
            .await;
            true
        }
        Ok(blob::ChunkOutcome::Partial { .. }) => true,
        Err(_) => true,
    }
}

/// Close a device's tunnel from the server side (kill switch, revocation or an
/// admin policy change). Returns whether a tunnel was actually closed.
pub async fn close_tunnel(device_id: &str, reason: &str) -> bool {
    let Some(live) = registry().by_device(device_id) else {
        return false;
    };
    let frame = InterlinkFrame::new(
        FRAME_CLOSE,
        format!("frm_{}", Uuid::new_v4().simple()),
        now_unix_seconds(),
        Some(live.channel_id.clone()),
        json!({ "reason": reason }),
    );
    let _ = registry().dispatch(
        device_id,
        &live.channel_id,
        OutboundFrame::Text(serde_json::to_string(&frame).unwrap_or_default()),
    );
    // The node's own loop sees the close it accepted and tears down; drop the
    // slot now so no new command can be routed to a tunnel being retired.
    registry().unregister(device_id, &live.channel_id)
}

// ---------------------------------------------------------------------------
// Frame helpers
// ---------------------------------------------------------------------------

fn build_frame(kind: &str, channel_id: Option<&str>, corr: Option<&str>, payload: Value) -> InterlinkFrame {
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
    let _ = send_frame(sender, FRAME_CLOSE, channel_id, None, json!({ "reason": reason })).await;
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

/// The capability names the server recognises (docs §9.2).
pub fn known_capabilities() -> [&'static str; 8] {
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

/// Three-way intersection: declared ∩ authorized ∩ known, minus anything the
/// admin converged, with the shadow level forced down when policy says so.
pub fn grant_capabilities(
    declared: &[String],
    authorized: &[String],
    policy: &digest::DevicePolicy,
) -> Vec<String> {
    let known = known_capabilities();
    let mut granted: Vec<String> = Vec::new();
    for cap in declared {
        let cap = cap.trim();
        if cap.is_empty() || !known.contains(&cap) {
            continue;
        }
        if !authorized.iter().any(|allowed| allowed == cap) {
            continue;
        }
        if policy.disables_cap(cap) {
            continue;
        }
        if policy.forces_minimal_shadow() && cap == CAP_SHADOW_FULL {
            continue;
        }
        if !granted.iter().any(|existing| existing == cap) {
            granted.push(cap.to_string());
        }
    }
    if policy.forces_minimal_shadow() && !granted.iter().any(|cap| cap == CAP_SHADOW_MINIMAL) {
        granted.push(CAP_SHADOW_MINIMAL.to_string());
    }
    granted
}

/// Keep at most one tunnel per device when the config allows exactly one, so a
/// reconnect cannot leave two live channels racing for the same slot.
/// Grant + channel helpers are on `services::interlink::registry()`.
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
async fn apply_shadow_frame(
    state: &AppState,
    live: &LiveChannel,
    frame: &InterlinkFrame,
    limits: &shadow::ShadowLimits,
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
        shadow::ShadowOutcome::Apply(mut record) => {
            // The server is the authority on the projection budget (docs §6.1).
            shadow::enforce_limits(&mut record, limits);
            let storage = state.storage.clone();
            let revision = record.revision;
            let _ = blocking::run_db("api.interlink_ws.shadow_upsert", move || {
                storage.upsert_interlink_shadow(&record)
            })
            .await;
            write_audit(
                state,
                audit::record(
                    AUDIT_SHADOW_SYNC,
                    &live.user_id,
                    Some(&format!("device:{}", live.device_id)),
                    Some("cloud"),
                    None,
                    None,
                    vec![
                        ("revision", json!(revision)),
                        ("frame", json!(frame.kind)),
                    ],
                ),
            )
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

/// Count one resumed channel (docs §4.4); false when the row is unknown.
async fn resume_channel(state: &AppState, channel_id: &str) -> bool {
    let storage = state.storage.clone();
    let lookup = channel_id.to_string();
    blocking::run_db("api.interlink_ws.resume", move || {
        let Some(mut record) = storage.get_interlink_channel(&lookup)? else {
            return Ok::<bool, anyhow::Error>(false);
        };
        record.resumed_count += 1;
        storage.upsert_interlink_channel(&record)?;
        Ok(true)
    })
    .await
    .unwrap_or(false)
}

/// Broadcast `node.joined` to the user's other live tunnels (docs §5.3).
async fn broadcast_node_joined(live: &LiveChannel) {
    let frame = build_frame(
        FRAME_EVENT,
        None,
        None,
        json!({
            "kind": "node.joined",
            "device_id": live.device_id,
            "client": live.client,
            "at": live.connected_at,
        }),
    );
    let text = serde_json::to_string(&frame).unwrap_or_default();
    for other in registry().snapshot() {
        if other.user_id != live.user_id || other.device_id == live.device_id {
            continue;
        }
        let _ = registry().dispatch(
            &other.device_id,
            &other.channel_id,
            OutboundFrame::Text(text.clone()),
        );
    }
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

    write_audit(
        state,
        audit::record(
            AUDIT_CHANNEL_CLOSE,
            &live.user_id,
            Some(&format!("device:{}", live.device_id)),
            Some("cloud"),
            None,
            None,
            vec![
                ("channel_id", json!(live.channel_id)),
                ("reason", json!(reason)),
            ],
        ),
    )
    .await;

    if owned {
        set_tunnel_connected(state, &live.device_id, false, 0.0).await;
        // In-flight commands against a node that vanished answer immediately
        // (docs §13.2 7) and remote watchers are released.
        let _ = commands::fail_device_commands(state.storage.clone(), &live.device_id, now_unix_seconds()).await;
        remote::hub().close_node(&live.device_id);
    }
}

async fn write_audit(state: &AppState, record: crate::storage::InterlinkAuditRecord) {
    let storage = state.storage.clone();
    let _ = blocking::run_db("api.interlink_ws.audit", move || {
        storage.insert_interlink_audit(&record)
    })
    .await;
}

fn shadow_limit_config(config: &wunder_core::config::InterlinkShadowConfig) -> shadow::ShadowLimits {
    shadow::ShadowLimits {
        threads_max: config.threads_max.max(1),
        tree_max_entries: config.tree_max_entries.max(1),
        tree_depth: config.tree_depth.max(1),
    }
}

fn parse_string_array(raw: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(raw) {
        Ok(list) if !list.is_empty() => list,
        _ => default_device_capabilities(),
    }
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Best-effort source address for ticket binding: the first entry of
/// `x-forwarded-for`, else `x-real-ip`. Absent behind plain sockets.
fn source_address(headers: &HeaderMap) -> Option<String> {
    for name in FORWARDED_HEADERS {
        if let Some(value) = header_value(headers, name) {
            let candidate = value
                .split(',')
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("unknown"));
            if let Some(candidate) = candidate {
                return Some(candidate.to_string());
            }
        }
    }
    None
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

    fn ticket(entry: &str, expires_at: f64) -> TicketEntry {
        TicketEntry {
            ticket: format!("itk_{entry}"),
            device_id: entry.to_string(),
            user_id: "u1".to_string(),
            expires_at,
            issued_from: None,
        }
    }

    #[test]
    fn ticket_store_is_single_use_and_expires() {
        let store = TicketStore::default();
        store.insert("itk_a".to_string(), ticket("d1", 100.0));
        assert!(store.consume("itk_a", 50.0).is_some());
        // Consumed once: the second attempt finds nothing.
        assert!(store.consume("itk_a", 50.0).is_none());

        store.insert("itk_b".to_string(), ticket("d1", 100.0));
        // Expired tickets are pruned.
        assert!(store.consume("itk_b", 200.0).is_none());
    }

    #[test]
    fn ticket_store_prunes_expired_entries() {
        let store = TicketStore::default();
        for index in 0..64 {
            store.insert(format!("itk_expired_{index}"), ticket("d1", 10.0));
        }
        store.insert("itk_live".to_string(), ticket("d1", 10_000.0));
        assert!(store.consume("itk_live", 100.0).is_some());
        // Only the live entry survived the last write.
        assert_eq!(
            store
                .tickets
                .read()
                .expect("lock")
                .len(),
            0
        );
    }

    #[test]
    fn grant_drops_unknown_unauthorized_and_admin_disabled() {
        let authorized = vec![
            CAP_QUERY_BASIC.to_string(),
            CAP_THREAD_DRIVE.to_string(),
            CAP_WORKSPACE_WRITE.to_string(),
        ];
        let declared = vec![
            CAP_QUERY_BASIC.to_string(),
            CAP_QUERY_BASIC.to_string(),
            "totally.unknown".to_string(),
            CAP_THREAD_DRIVE.to_string(),
            CAP_WORKSPACE_WRITE.to_string(),
            CAP_TOOL_EXEC.to_string(),
        ];
        let policy = digest::DevicePolicy {
            disabled_caps: vec![CAP_WORKSPACE_WRITE.to_string()],
            ..Default::default()
        };
        let granted = grant_capabilities(&declared, &authorized, &policy);
        assert_eq!(granted, vec![CAP_QUERY_BASIC, CAP_THREAD_DRIVE]);
    }

    #[test]
    fn grant_forces_minimal_shadow() {
        let authorized = vec![CAP_SHADOW_FULL.to_string(), CAP_SHADOW_MINIMAL.to_string()];
        let declared = vec![CAP_SHADOW_FULL.to_string()];
        let granted = grant_capabilities(
            &declared,
            &authorized,
            &digest::DevicePolicy {
                shadow_mode: Some("minimal".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(granted, vec![CAP_SHADOW_MINIMAL]);
    }

    #[test]
    fn interlink_switch_is_fail_open_on_unset_and_closed_on_false() {
        assert!(interlink_allowed(&None));
        let mut patch = CloudDeviceInterlinkPatch::default();
        assert!(interlink_allowed(&Some(Box::new(patch.clone()))));
        patch.interlink_enabled = Some(true);
        assert!(interlink_allowed(&Some(Box::new(patch.clone()))));
        patch.interlink_enabled = Some(false);
        assert!(!interlink_allowed(&Some(Box::new(patch))));
    }

    #[test]
    fn build_frame_sets_type_channel_and_corr() {
        let frame = build_frame(FRAME_PING, Some("ch_1"), Some("cmd_1"), json!({}));
        let value = serde_json::to_value(&frame).expect("serialize");
        assert_eq!(value["type"], FRAME_PING);
        assert_eq!(value["channel_id"], "ch_1");
        assert_eq!(value["corr"], "cmd_1");
    }

    #[test]
    fn source_address_prefers_the_forwarded_chain() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.7, 10.0.0.1".parse().unwrap());
        assert_eq!(source_address(&headers).as_deref(), Some("203.0.113.7"));

        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "unknown".parse().unwrap());
        headers.insert("x-real-ip", "198.51.100.9".parse().unwrap());
        assert_eq!(source_address(&headers).as_deref(), Some("198.51.100.9"));

        assert_eq!(source_address(&HeaderMap::new()), None);
    }

    #[test]
    fn shadow_limits_come_from_the_configuration() {
        let config = wunder_core::config::InterlinkConfig::default();
        let limits = shadow_limit_config(&config.shadow);
        assert_eq!(limits.threads_max, 200);
        assert_eq!(limits.tree_max_entries, 500);
        assert_eq!(limits.tree_depth, 3);
    }
}
