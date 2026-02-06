use crate::gateway::{
    now_ts, GatewayClientMeta, GatewayConnectParams, GatewayHub, GatewayRole,
    GATEWAY_MAX_MESSAGE_BYTES,
};
use crate::i18n;
use crate::state::AppState;
use crate::storage::GatewayNodeTokenRecord;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Router};
use futures::{SinkExt, StreamExt as WsStreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;
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
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let config = state.config_store.get().await;
    if !config.gateway.enabled {
        return Err((StatusCode::NOT_FOUND, "gateway disabled").into_response());
    }
    let connection_id = format!("gw_{}", Uuid::new_v4().simple());
    Ok(ws
        .protocols(["wunder-gateway"])
        .max_message_size(GATEWAY_MAX_MESSAGE_BYTES)
        .max_frame_size(GATEWAY_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_gateway_ws(socket, state, connection_id, query)))
}

async fn handle_gateway_ws(
    socket: WebSocket,
    state: Arc<AppState>,
    connection_id: String,
    query: GatewayWsQuery,
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
    while let Some(Ok(message)) = WsStreamExt::next(&mut ws_receiver).await {
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
                        let method = envelope
                            .method
                            .as_deref()
                            .unwrap_or("")
                            .trim()
                            .to_ascii_lowercase();
                        if method == "connect" {
                            if connected.is_some() {
                                let _ = send_gateway_error(
                                    &out_tx,
                                    envelope.id.as_deref(),
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
                                envelope.id.as_deref(),
                                envelope.params,
                                &query,
                            )
                            .await
                            {
                                Ok(meta) => {
                                    connected = Some(meta);
                                }
                                Err(err) => {
                                    let _ = send_gateway_error(
                                        &out_tx,
                                        envelope.id.as_deref(),
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
                            let _ = send_gateway_ok(&out_tx, envelope.id.as_deref(), payload).await;
                            continue;
                        }

                        let _ = send_gateway_error(
                            &out_tx,
                            envelope.id.as_deref(),
                            "UNSUPPORTED_METHOD",
                            format!("unsupported method: {method}"),
                        )
                        .await;
                    }
                    "res" => {
                        if let Some(req_id) = envelope.id.as_deref() {
                            hub.handle_response(
                                req_id,
                                envelope.ok.unwrap_or(false),
                                envelope.payload,
                                envelope.error,
                            )
                            .await;
                        }
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

    let _ = writer.await;
}

async fn handle_connect(
    state: &Arc<AppState>,
    hub: &GatewayHub,
    connection_id: &str,
    out_tx: &mpsc::Sender<Message>,
    request_id: Option<&str>,
    params: Option<Value>,
    query: &GatewayWsQuery,
) -> Result<GatewayClientMeta, GatewayHandshakeError> {
    let payload: GatewayConnectParams = match params {
        Some(value) => serde_json::from_value(value)
            .map_err(|err| GatewayHandshakeError::new("INVALID_PAYLOAD", err.to_string()))?,
        None => GatewayConnectParams::default(),
    };
    let config = state.config_store.get().await;
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
            .or_else(|| query.token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if provided != Some(expected) {
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
        "server_time": now
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
    let message = json!({
        "type": "res",
        "id": request_id,
        "ok": false,
        "error": {
            "code": code,
            "message": message
        }
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
