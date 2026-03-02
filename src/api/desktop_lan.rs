use crate::services::desktop_lan::{self, DesktopLanEnvelope, DesktopLanPeerSnapshot};
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{
    routing::{get, post},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;

type HmacSha256 = Hmac<Sha256>;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/desktop/lan/peers", get(list_peers))
        .route("/wunder/desktop/lan/envelope", post(receive_envelope))
        .route("/wunder/desktop/lan/ws", get(lan_ws))
}

#[derive(Debug, Deserialize)]
struct DesktopLanInboundPayload {
    #[serde(default)]
    peer: Option<DesktopLanPeerSnapshot>,
    envelope: DesktopLanEnvelope,
}

#[derive(Debug, Deserialize)]
struct LanDirectMessagePayload {
    content: String,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    client_msg_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LanGroupMessagePayload {
    global_group_id: String,
    group_name: String,
    content: String,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    client_msg_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LanGroupUpsertPayload {
    global_group_id: String,
    group_name: String,
    owner_user_id: String,
    #[serde(default)]
    member_user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct LanGroupAnnouncementPayload {
    global_group_id: String,
    #[serde(default)]
    announcement: Option<String>,
}

async fn list_peers(State(_state): State<Arc<AppState>>) -> Json<Value> {
    let items = desktop_lan::manager().list_peers().await;
    Json(json!({
        "data": {
            "items": items,
            "total": items.len()
        }
    }))
}

async fn receive_envelope(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DesktopLanInboundPayload>,
) -> Result<Json<Value>, Response> {
    let remote_ip = extract_request_ip(&headers);
    let data = process_inbound_payload(state.as_ref(), payload, &headers, remote_ip).await?;
    Ok(Json(json!({ "data": data })))
}

async fn lan_ws(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let remote_ip = extract_request_ip(&headers);
    Ok(ws
        .protocols(["wunder-lan"])
        .max_message_size(512 * 1024)
        .on_upgrade(move |socket| handle_lan_ws(socket, state, headers, remote_ip)))
}

async fn handle_lan_ws(
    socket: WebSocket,
    state: Arc<AppState>,
    headers: HeaderMap,
    remote_ip: Option<IpAddr>,
) {
    let (mut sender, mut receiver) = socket.split();
    while let Some(message) = receiver.next().await {
        match message {
            Ok(Message::Text(text)) => {
                let response = match serde_json::from_str::<DesktopLanInboundPayload>(&text) {
                    Ok(payload) => {
                        match process_inbound_payload(state.as_ref(), payload, &headers, remote_ip)
                            .await
                        {
                            Ok(data) => json!({ "type": "ack", "ok": true, "data": data }),
                            Err(err) => {
                                let status = err.status();
                                json!({
                                    "type": "ack",
                                    "ok": false,
                                    "error": status.as_u16(),
                                    "message": status.canonical_reason().unwrap_or("request failed")
                                })
                            }
                        }
                    }
                    Err(err) => json!({
                        "type": "ack",
                        "ok": false,
                        "error": StatusCode::BAD_REQUEST.as_u16(),
                        "message": format!("invalid payload: {err}")
                    }),
                };
                if sender
                    .send(Message::Text(response.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Ok(Message::Ping(payload)) => {
                if sender.send(Message::Pong(payload)).await.is_err() {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

async fn process_inbound_payload(
    state: &AppState,
    payload: DesktopLanInboundPayload,
    headers: &HeaderMap,
    remote_ip: Option<IpAddr>,
) -> Result<Value, Response> {
    let manager = desktop_lan::manager();
    let settings = manager.settings().await;
    if !settings.enabled {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "desktop lan mesh is disabled".to_string(),
        ));
    }

    if let Some(ip) = remote_ip {
        if !settings.allows_ip(ip) {
            return Err(error_response(
                StatusCode::FORBIDDEN,
                format!("lan source denied: {ip}"),
            ));
        }
    }

    if !verify_envelope_signature(&payload.envelope, &settings.shared_secret, headers) {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid lan envelope signature".to_string(),
        ));
    }
    let source_peer = payload.peer.clone();

    if let Some(peer) = payload.peer {
        if settings.is_peer_blocked(&peer.peer_id) {
            return Err(error_response(
                StatusCode::FORBIDDEN,
                format!("peer blocked: {}", peer.peer_id),
            ));
        }
        let _ = manager.upsert_peer(peer).await;
    }

    let envelope_id = payload.envelope.envelope_id.trim();
    if envelope_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "envelope_id is required".to_string(),
        ));
    }
    let accepted = manager.accepts_envelope(envelope_id).await;
    if accepted {
        apply_user_world_envelope(state, &payload.envelope, source_peer.as_ref()).await?;
    }
    Ok(json!({
        "accepted": accepted,
        "envelope_id": envelope_id,
        "duplicate": !accepted
    }))
}

async fn apply_user_world_envelope(
    state: &AppState,
    envelope: &DesktopLanEnvelope,
    source_snapshot: Option<&DesktopLanPeerSnapshot>,
) -> Result<(), Response> {
    let envelope_type = envelope.envelope_type.trim().to_ascii_lowercase();
    if envelope_type.is_empty() {
        return Ok(());
    }
    let local_user_id = resolve_desktop_user_id();
    let source_peer_id = envelope.source_peer_id.trim();
    if source_peer_id.is_empty() {
        return Ok(());
    }
    let remote_user_id = lan_peer_user_id(
        source_peer_id,
        source_snapshot.and_then(|peer| normalize_lan_ip(peer.lan_ip.as_str())),
    );
    match envelope_type.as_str() {
        "uw_direct_message" => {
            let payload: LanDirectMessagePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|err| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        format!("invalid direct message payload: {err}"),
                    )
                })?;
            let content = payload.content.trim();
            if content.is_empty() {
                return Ok(());
            }
            let conversation = state
                .user_world
                .resolve_or_create_direct_conversation(&local_user_id, &remote_user_id, now_ts())
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            let client_msg_id = payload
                .client_msg_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("lan:{}", envelope.envelope_id));
            state
                .user_world
                .send_message(
                    &remote_user_id,
                    &conversation.conversation_id,
                    content,
                    payload.content_type.as_deref().unwrap_or("text"),
                    Some(client_msg_id.as_str()),
                    now_ts(),
                )
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        "uw_group_upsert" => {
            let payload: LanGroupUpsertPayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|err| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    format!("invalid group upsert payload: {err}"),
                )
            })?;
            let _ = ensure_group_conversation(
                state,
                &payload.global_group_id,
                &payload.group_name,
                Some(payload.owner_user_id.as_str()),
                &payload.member_user_ids,
            )
            .await?;
        }
        "uw_group_message" => {
            let payload: LanGroupMessagePayload = serde_json::from_value(envelope.payload.clone())
                .map_err(|err| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        format!("invalid group message payload: {err}"),
                    )
                })?;
            let content = payload.content.trim();
            if content.is_empty() {
                return Ok(());
            }
            let conversation_id = ensure_group_conversation(
                state,
                &payload.global_group_id,
                &payload.group_name,
                Some(remote_user_id.as_str()),
                &[],
            )
            .await?;
            let client_msg_id = payload
                .client_msg_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("lan:{}", envelope.envelope_id));
            state
                .user_world
                .send_message(
                    &remote_user_id,
                    &conversation_id,
                    content,
                    payload.content_type.as_deref().unwrap_or("text"),
                    Some(client_msg_id.as_str()),
                    now_ts(),
                )
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        "uw_group_announcement" => {
            let payload: LanGroupAnnouncementPayload =
                serde_json::from_value(envelope.payload.clone()).map_err(|err| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        format!("invalid group announcement payload: {err}"),
                    )
                })?;
            let conversation_id = ensure_group_conversation(
                state,
                &payload.global_group_id,
                &payload.global_group_id,
                Some(remote_user_id.as_str()),
                &[],
            )
            .await?;
            let Some(conversation) = state
                .storage
                .get_user_world_conversation(&conversation_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            else {
                return Ok(());
            };
            let Some(group_id) = conversation.group_id.as_deref() else {
                return Ok(());
            };
            let _ = state
                .user_world
                .update_group_announcement(
                    &local_user_id,
                    group_id,
                    payload.announcement.as_deref(),
                    now_ts(),
                )
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        _ => {}
    }
    Ok(())
}

async fn ensure_group_conversation(
    state: &AppState,
    global_group_id: &str,
    group_name: &str,
    owner_user_id: Option<&str>,
    member_user_ids: &[String],
) -> Result<String, Response> {
    let group_key = global_group_id.trim();
    if group_key.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "global_group_id is required".to_string(),
        ));
    }

    if let Some(existing) = desktop_lan::manager()
        .conversation_id_by_group(group_key)
        .await
    {
        return Ok(existing);
    }

    let local_user_id = resolve_desktop_user_id();
    let owner = owner_user_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(local_user_id.as_str())
        .to_string();
    let mut members = member_user_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<HashSet<_>>();
    members.insert(local_user_id.clone());
    members.insert(owner.clone());

    let mut normalized_members = members.into_iter().collect::<Vec<_>>();
    normalized_members.sort();

    let name = group_name.trim();
    let effective_name = if name.is_empty() { group_key } else { name };
    let created = state
        .user_world
        .create_group(&owner, effective_name, &normalized_members, now_ts())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    desktop_lan::manager()
        .register_group_link(group_key, &created.conversation_id)
        .await;
    Ok(created.conversation_id)
}

fn resolve_desktop_user_id() -> String {
    std::env::var("WUNDER_DESKTOP_USER_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "desktop_user".to_string())
}

fn lan_peer_user_id(peer_id: &str, lan_ip: Option<String>) -> String {
    let peer_id = peer_id.trim();
    if let Some(lan_ip) = lan_ip {
        return format!("lan:{peer_id}@{lan_ip}");
    }
    format!("lan:{peer_id}")
}

fn normalize_lan_ip(value: &str) -> Option<String> {
    let cleaned = value.trim().trim_matches(|ch| ch == '[' || ch == ']');
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

fn verify_envelope_signature(
    envelope: &DesktopLanEnvelope,
    shared_secret: &str,
    headers: &HeaderMap,
) -> bool {
    let secret = shared_secret.trim();
    if secret.is_empty() {
        return true;
    }
    let provided = envelope
        .signature
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| extract_signature_from_headers(headers));
    let Some(provided) = provided else {
        return false;
    };
    let expected = sign_envelope(secret, envelope);
    provided.eq_ignore_ascii_case(&expected)
}

fn sign_envelope(shared_secret: &str, envelope: &DesktopLanEnvelope) -> String {
    let content = build_signature_payload(envelope);
    let mut mac = match HmacSha256::new_from_slice(shared_secret.as_bytes()) {
        Ok(value) => value,
        Err(_) => return String::new(),
    };
    mac.update(content.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn build_signature_payload(envelope: &DesktopLanEnvelope) -> String {
    let payload = serde_json::to_string(&envelope.payload).unwrap_or_else(|_| "null".to_string());
    format!(
        "{}|{}|{}|{}|{}|{}",
        envelope.envelope_id.trim(),
        envelope.envelope_type.trim(),
        envelope.source_peer_id.trim(),
        envelope.source_user_id.trim(),
        envelope.sent_at,
        payload
    )
}

fn extract_signature_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-wunder-lan-signature")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn extract_request_ip(headers: &HeaderMap) -> Option<IpAddr> {
    let candidates = [
        "x-wunder-lan-ip",
        "x-real-ip",
        "x-forwarded-for",
        "forwarded",
    ];
    for header in candidates {
        let Some(raw) = headers.get(header).and_then(|value| value.to_str().ok()) else {
            continue;
        };
        for token in raw.split(',') {
            let candidate = token.trim();
            if candidate.is_empty() {
                continue;
            }
            if let Ok(ip) = candidate.parse::<IpAddr>() {
                return Some(ip);
            }
            if let Some((ip, _port)) = candidate.rsplit_once(':') {
                if let Ok(parsed) = ip.trim().parse::<IpAddr>() {
                    return Some(parsed);
                }
            }
        }
    }
    None
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or(0.0)
}

fn error_response(status: StatusCode, message: String) -> Response {
    (
        status,
        Json(json!({
            "error": status.as_u16(),
            "message": message,
        })),
    )
        .into_response()
}
