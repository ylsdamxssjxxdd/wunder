use crate::config::GatewayConfig;
use crate::gateway::{
    now_ts, GatewayClientMeta, GatewayConnectParams, GatewayHub, GatewayRole,
    GATEWAY_HANDSHAKE_TIMEOUT_MS, GATEWAY_MAX_MESSAGE_BYTES,
};
use crate::i18n;
use crate::state::AppState;
use crate::storage::GatewayNodeTokenRecord;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Router};
use futures::{SinkExt, StreamExt as WsStreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{timeout_at, Duration, Instant};
use uuid::Uuid;

const GATEWAY_ENDPOINT: &str = "/wunder/gateway/ws";

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route(GATEWAY_ENDPOINT, get(gateway_ws))
}

#[derive(Debug, Deserialize)]
struct GatewayWsQuery {
    #[serde(default)]
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GatewayEnvelope {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    payload: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    #[allow(dead_code)]
    event: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct GatewayRequestMeta {
    host: Option<String>,
    origin: Option<String>,
    forwarded_for: Option<String>,
    real_ip: Option<String>,
    remote_addr: Option<String>,
}

#[derive(Debug)]
struct GatewayHandshakeError {
    code: &'static str,
    message: String,
}

impl GatewayHandshakeError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

async fn gateway_ws(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GatewayWsQuery>,
    headers: HeaderMap,
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let config = state.config_store.get().await;
    if !config.gateway.enabled {
        return Err((StatusCode::NOT_FOUND, "gateway disabled").into_response());
    }
    let connection_id = format!("gw_{}", Uuid::new_v4().simple());
    let request_meta = GatewayRequestMeta {
        host: header_value(&headers, "host"),
        origin: header_value(&headers, "origin"),
        forwarded_for: header_value(&headers, "x-forwarded-for"),
        real_ip: header_value(&headers, "x-real-ip"),
        remote_addr: Some(remote_addr.to_string()),
    };
    Ok(ws
        .protocols(["wunder-gateway"])
        .max_message_size(GATEWAY_MAX_MESSAGE_BYTES)
        .max_frame_size(GATEWAY_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| {
            handle_gateway_ws(socket, state, connection_id, query, request_meta)
        }))
}

async fn handle_gateway_ws(
    socket: WebSocket,
    state: Arc<AppState>,
    connection_id: String,
    query: GatewayWsQuery,
    request_meta: GatewayRequestMeta,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(64);
    let writer = tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            if ws_sender.send(message).await.is_err() {
                break;
            }
        }
    });

    let hub = state.gateway.clone();
    let challenge = json!({
        "type": "event",
        "event": "connect.challenge",
        "payload": {
            "nonce": Uuid::new_v4().simple().to_string(),
            "ts": (now_ts() * 1000.0).round()
        }
    });
    let _ = out_tx
        .send(Message::Text(challenge.to_string().into()))
        .await;

    let mut connected: Option<GatewayClientMeta> = None;
    let handshake_deadline = Instant::now() + Duration::from_millis(GATEWAY_HANDSHAKE_TIMEOUT_MS);
    loop {
        let next_message = if connected.is_some() {
            WsStreamExt::next(&mut ws_receiver).await
        } else {
            match timeout_at(handshake_deadline, WsStreamExt::next(&mut ws_receiver)).await {
                Ok(frame) => frame,
                Err(_) => {
                    let _ = send_gateway_error(
                        &out_tx,
                        None,
                        "HANDSHAKE_TIMEOUT",
                        "connect handshake timeout".to_string(),
                    )
                    .await;
                    break;
                }
            }
        };
        let Some(Ok(message)) = next_message else {
            break;
        };
        match message {
            Message::Text(text) => {
                let envelope: GatewayEnvelope = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(err) => {
                        let _ = send_gateway_error(
                            &out_tx,
                            None,
                            "INVALID_JSON",
                            format!("invalid payload: {err}"),
                        )
                        .await;
                        continue;
                    }
                };

                let kind = envelope.kind.trim().to_ascii_lowercase();
                let now = now_ts();
                hub.touch_client(&connection_id, now).await;

                match kind.as_str() {
                    "req" => {
                        let request_id = match normalize_request_id(envelope.id.as_deref()) {
                            Some(value) => value,
                            None => {
                                let _ = send_gateway_error(
                                    &out_tx,
                                    None,
                                    "INVALID_REQUEST",
                                    "request id required".to_string(),
                                )
                                .await;
                                continue;
                            }
                        };
                        let method = envelope
                            .method
                            .as_deref()
                            .unwrap_or("")
                            .trim()
                            .to_ascii_lowercase();
                        if method.is_empty() {
                            let _ = send_gateway_error(
                                &out_tx,
                                Some(&request_id),
                                "INVALID_REQUEST",
                                "method required".to_string(),
                            )
                            .await;
                            continue;
                        }
                        if connected.is_none() && method != "connect" {
                            let _ = send_gateway_error(
                                &out_tx,
                                Some(&request_id),
                                "INVALID_HANDSHAKE",
                                "first request must be connect".to_string(),
                            )
                            .await;
                            break;
                        }
                        if method == "connect" {
                            if connected.is_some() {
                                let _ = send_gateway_error(
                                    &out_tx,
                                    Some(&request_id),
                                    "ALREADY_CONNECTED",
                                    "connection already initialized".to_string(),
                                )
                                .await;
                                continue;
                            }
                            match handle_connect(
                                &state,
                                &hub,
                                &connection_id,
                                &out_tx,
                                Some(&request_id),
                                envelope.params,
                                &query,
                                &request_meta,
                            )
                            .await
                            {
                                Ok(meta) => {
                                    connected = Some(meta);
                                }
                                Err(err) => {
                                    let _ = send_gateway_error(
                                        &out_tx,
                                        Some(&request_id),
                                        err.code,
                                        err.message,
                                    )
                                    .await;
                                }
                            }
                            continue;
                        }

                        if method == "presence.get" || method == "gateway.presence" {
                            let snapshot = hub.snapshot().await;
                            let payload = json!({
                                "type": "presence",
                                "stateVersion": snapshot.state_version,
                                "items": snapshot.items
                            });
                            let _ = send_gateway_ok(&out_tx, Some(&request_id), payload).await;
                            continue;
                        }

                        let _ = send_gateway_error(
                            &out_tx,
                            Some(&request_id),
                            "UNSUPPORTED_METHOD",
                            format!("unsupported method: {method}"),
                        )
                        .await;
                    }
                    "res" => {
                        let request_id = match normalize_request_id(envelope.id.as_deref()) {
                            Some(value) => value,
                            None => {
                                let _ = send_gateway_error(
                                    &out_tx,
                                    None,
                                    "INVALID_RESPONSE",
                                    "response id required".to_string(),
                                )
                                .await;
                                continue;
                            }
                        };
                        let Some(ok) = envelope.ok else {
                            let _ = send_gateway_error(
                                &out_tx,
                                Some(&request_id),
                                "INVALID_RESPONSE",
                                "response ok field required".to_string(),
                            )
                            .await;
                            continue;
                        };
                        let source_node_id =
                            connected.as_ref().and_then(|meta| meta.node_id.as_deref());
                        hub.handle_response(
                            &request_id,
                            &connection_id,
                            source_node_id,
                            ok,
                            envelope.payload,
                            envelope.error,
                        )
                        .await;
                    }
                    "event" => {
                        // Ignore unrecognized events from clients for now.
                    }
                    "ping" => {
                        let _ = send_gateway_event(&out_tx, "pong", json!({ "ts": now })).await;
                    }
                    _ => {
                        let _ = send_gateway_error(
                            &out_tx,
                            envelope.id.as_deref(),
                            "UNSUPPORTED_TYPE",
                            format!("unsupported message type: {}", envelope.kind),
                        )
                        .await;
                    }
                }
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    if let Some(snapshot) = hub.unregister_client(&connection_id).await {
        hub.broadcast_event(
            "gateway.presence.update",
            json!({ "stateVersion": snapshot.state_version, "items": snapshot.items }),
            Some(snapshot.state_version),
        )
        .await;
    }

    drop(out_tx);
    let _ = writer.await;
}

#[allow(clippy::too_many_arguments)]
async fn handle_connect(
    state: &Arc<AppState>,
    hub: &GatewayHub,
    connection_id: &str,
    out_tx: &mpsc::Sender<Message>,
    request_id: Option<&str>,
    params: Option<Value>,
    query: &GatewayWsQuery,
    request_meta: &GatewayRequestMeta,
) -> Result<GatewayClientMeta, GatewayHandshakeError> {
    let payload: GatewayConnectParams = match params {
        Some(value) => serde_json::from_value(value)
            .map_err(|err| GatewayHandshakeError::new("INVALID_PAYLOAD", err.to_string()))?,
        None => GatewayConnectParams::default(),
    };
    let config = state.config_store.get().await;
    validate_request_origin(request_meta, &config.gateway)?;
    let client_ip = resolve_client_ip(request_meta, &config.gateway.trusted_proxies);
    let server_version = config.gateway.protocol_version;
    let (min_protocol, max_protocol) = resolve_protocol_range(&payload, server_version)?;
    let role = GatewayRole::from_str(payload.role.as_deref().unwrap_or(""));
    if role == GatewayRole::Unknown {
        return Err(GatewayHandshakeError::new("ROLE_REQUIRED", "role required"));
    }
    let auth = payload.auth.clone().unwrap_or_default();
    if let Some(expected) = config
        .gateway
        .auth_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let provided = auth
            .token
            .as_deref()
            .or(query.token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let authorized = provided
            .map(|provided_token| secure_compare(provided_token, expected))
            .unwrap_or(false);
        if !authorized {
            return Err(GatewayHandshakeError::new(
                "UNAUTHORIZED",
                i18n::t("error.permission_denied"),
            ));
        }
    }

    let mut node_token_record: Option<GatewayNodeTokenRecord> = None;
    if role == GatewayRole::Node {
        if let Some(token) = auth
            .node_token
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let record = state
                .storage
                .get_gateway_node_token(token)
                .map_err(|err| GatewayHandshakeError::new("BAD_REQUEST", err.to_string()))?;
            if let Some(record) = record {
                if record.status != "active" {
                    return Err(GatewayHandshakeError::new(
                        "UNAUTHORIZED",
                        "node token disabled",
                    ));
                }
                node_token_record = Some(record);
            } else {
                return Err(GatewayHandshakeError::new(
                    "UNAUTHORIZED",
                    "node token invalid",
                ));
            }
        }
        let require_token =
            config.gateway.node_token_required || !config.gateway.allow_unpaired_nodes;
        if require_token
            && node_token_record.is_none()
            && !config.gateway.allow_gateway_token_for_nodes
        {
            return Err(GatewayHandshakeError::new(
                "UNAUTHORIZED",
                "node token required",
            ));
        }
    }

    let node_id = node_token_record
        .as_ref()
        .map(|record| record.node_id.clone())
        .or_else(|| payload.device.as_ref().and_then(|device| device.id.clone()))
        .or_else(|| {
            if role == GatewayRole::Node {
                Some(format!("node_{}", Uuid::new_v4().simple()))
            } else {
                None
            }
        });

    let mut client_info = payload.client.clone().unwrap_or_default();
    if client_info.id.is_none() {
        client_info.id = payload
            .device
            .as_ref()
            .and_then(|device| device.name.clone());
    }
    if client_info
        .id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(GatewayHandshakeError::new(
            "CLIENT_ID_REQUIRED",
            "client id required",
        ));
    }

    let now = now_ts();
    let meta = GatewayClientMeta {
        connection_id: connection_id.to_string(),
        role,
        user_id: payload.user_id.clone(),
        node_id: node_id.clone(),
        scopes: payload.scopes.clone(),
        caps: payload.caps.clone(),
        commands: payload.commands.clone(),
        client: Some(client_info.clone()),
        connected_at: now,
        last_seen_at: now,
        device_fingerprint: payload
            .device
            .as_ref()
            .and_then(|device| device.device_fingerprint.clone())
            .or_else(|| payload.device.as_ref().and_then(|device| device.id.clone())),
    };

    let snapshot = hub.register_client(meta.clone(), out_tx.clone()).await;
    if let Some(mut record) = node_token_record {
        record.last_used_at = Some(now);
        record.updated_at = now;
        let _ = state.storage.upsert_gateway_node_token(&record);
    }
    let policy = GatewayHub::default_policy();
    let payload = json!({
        "type": "hello-ok",
        "connection_id": connection_id,
        "protocol": {
            "version": server_version,
            "min": min_protocol,
            "max": max_protocol,
            "server": server_version
        },
        "policy": policy,
        "presence": snapshot.items,
        "stateVersion": snapshot.state_version,
        "server_time": now,
        "client_ip": client_ip
    });
    send_gateway_ok(out_tx, request_id, payload).await?;
    hub.broadcast_event(
        "gateway.presence.update",
        json!({ "stateVersion": snapshot.state_version, "items": snapshot.items }),
        Some(snapshot.state_version),
    )
    .await;
    Ok(meta)
}

fn resolve_protocol_range(
    payload: &GatewayConnectParams,
    server_version: i32,
) -> Result<(i32, i32), GatewayHandshakeError> {
    let mut min = payload.min_protocol;
    let mut max = payload.max_protocol;
    if min.is_none() && max.is_none() {
        if let Some(version) = payload.protocol_version {
            min = Some(version);
            max = Some(version);
        }
    }
    let min = min.unwrap_or(server_version);
    let max = max.unwrap_or(server_version);
    if min <= 0 || max <= 0 || min > max {
        return Err(GatewayHandshakeError::new(
            "INVALID_PROTOCOL_RANGE",
            format!("invalid protocol range: min={min}, max={max}"),
        ));
    }
    if server_version < min || server_version > max {
        return Err(GatewayHandshakeError::new(
            "PROTOCOL_MISMATCH",
            format!(
                "protocol mismatch: server={}, client_range={}..{}",
                server_version, min, max
            ),
        ));
    }
    Ok((min, max))
}

async fn send_gateway_ok(
    tx: &mpsc::Sender<Message>,
    request_id: Option<&str>,
    payload: Value,
) -> Result<(), GatewayHandshakeError> {
    let id = request_id
        .map(|value| value.to_string())
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let message = json!({
        "type": "res",
        "id": id,
        "ok": true,
        "payload": payload
    });
    tx.send(Message::Text(message.to_string().into()))
        .await
        .map_err(|_| GatewayHandshakeError::new("CONNECTION_CLOSED", "connection closed"))
}

async fn send_gateway_error(
    tx: &mpsc::Sender<Message>,
    request_id: Option<&str>,
    code: &str,
    message: String,
) -> Result<(), GatewayHandshakeError> {
    let status = crate::api::errors::status_for_error_code(code);
    let error = crate::api::errors::build_error_meta(
        status,
        Some(code),
        message,
        crate::api::errors::hint_for_error_code(code),
    )
    .to_value();
    let message = json!({
        "type": "res",
        "id": request_id,
        "ok": false,
        "error": error
    });
    tx.send(Message::Text(message.to_string().into()))
        .await
        .map_err(|_| GatewayHandshakeError::new("CONNECTION_CLOSED", "connection closed"))
}

async fn send_gateway_event(
    tx: &mpsc::Sender<Message>,
    event: &str,
    payload: Value,
) -> Result<(), GatewayHandshakeError> {
    let message = json!({
        "type": "event",
        "event": event,
        "payload": payload
    });
    tx.send(Message::Text(message.to_string().into()))
        .await
        .map_err(|_| GatewayHandshakeError::new("CONNECTION_CLOSED", "connection closed"))
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_request_id(request_id: Option<&str>) -> Option<String> {
    request_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn secure_compare(left: &str, right: &str) -> bool {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let max_len = left_bytes.len().max(right_bytes.len());
    let mut diff = left_bytes.len() ^ right_bytes.len();
    for idx in 0..max_len {
        let left_byte = left_bytes.get(idx).copied().unwrap_or_default();
        let right_byte = right_bytes.get(idx).copied().unwrap_or_default();
        diff |= (left_byte ^ right_byte) as usize;
    }
    diff == 0
}

fn validate_request_origin(
    request_meta: &GatewayRequestMeta,
    config: &GatewayConfig,
) -> Result<(), GatewayHandshakeError> {
    let Some(origin_raw) = request_meta
        .origin
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let origin = origin_raw.to_ascii_lowercase();
    let origin_host = parse_origin_host(&origin)
        .ok_or_else(|| GatewayHandshakeError::new("INVALID_ORIGIN", "invalid origin"))?;
    let request_host = request_meta
        .host
        .as_deref()
        .and_then(extract_host_name)
        .unwrap_or_default();
    if !request_host.is_empty() && origin_host == request_host {
        return Ok(());
    }
    if !request_host.is_empty() && is_loopback_host(&origin_host) && is_loopback_host(&request_host)
    {
        return Ok(());
    }

    let origin_allowed = config.allowed_origins.iter().any(|allowed| {
        let normalized = allowed.trim().to_ascii_lowercase();
        !normalized.is_empty() && normalized == origin
    });
    if origin_allowed {
        return Ok(());
    }

    Err(GatewayHandshakeError::new(
        "ORIGIN_NOT_ALLOWED",
        "origin not allowed",
    ))
}

fn resolve_client_ip(
    request_meta: &GatewayRequestMeta,
    trusted_proxies: &[String],
) -> Option<String> {
    let remote_ip = request_meta
        .remote_addr
        .as_deref()
        .and_then(parse_socket_ip)
        .or_else(|| request_meta.real_ip.as_deref().and_then(parse_ip_literal));
    let remote_ip = remote_ip?;

    let trusted_proxy = is_trusted_proxy(remote_ip, trusted_proxies);
    if trusted_proxy {
        if let Some(forwarded) = request_meta
            .forwarded_for
            .as_deref()
            .and_then(parse_forwarded_for_ip)
        {
            return Some(forwarded.to_string());
        }
        if let Some(real_ip) = request_meta.real_ip.as_deref().and_then(parse_ip_literal) {
            return Some(real_ip.to_string());
        }
    }

    Some(remote_ip.to_string())
}

fn parse_origin_host(origin: &str) -> Option<String> {
    url::Url::parse(origin)
        .ok()?
        .host_str()
        .map(|host| host.to_ascii_lowercase())
}

fn extract_host_name(raw: &str) -> Option<String> {
    let lowered = raw.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return None;
    }
    if lowered.starts_with('[') {
        let end = lowered.find(']')?;
        return Some(lowered[1..end].to_string());
    }
    Some(lowered.split(':').next().unwrap_or_default().to_string())
}

fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

fn parse_socket_ip(raw: &str) -> Option<IpAddr> {
    raw.parse::<SocketAddr>()
        .map(|addr| addr.ip())
        .or_else(|_| raw.parse::<IpAddr>())
        .ok()
}

fn parse_ip_literal(raw: &str) -> Option<IpAddr> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let literal = if trimmed.starts_with('[') && trimmed.ends_with(']') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    literal.parse::<IpAddr>().ok()
}

fn parse_forwarded_for_ip(raw: &str) -> Option<IpAddr> {
    let first = raw.split(',').next()?.trim();
    let first = first.strip_prefix("for=").unwrap_or(first);
    parse_ip_literal(first)
}

fn is_trusted_proxy(remote_ip: IpAddr, trusted_proxies: &[String]) -> bool {
    trusted_proxies.iter().any(|entry| {
        let candidate = entry.trim();
        if candidate.eq_ignore_ascii_case("loopback") {
            return remote_ip.is_loopback();
        }
        candidate
            .parse::<IpAddr>()
            .map(|ip| ip == remote_ip)
            .unwrap_or(false)
    })
}
