use crate::i18n;
use crate::monitor::MonitorState;
use crate::orchestrator_constants::{
    STREAM_EVENT_FETCH_LIMIT, STREAM_EVENT_QUEUE_SIZE, STREAM_EVENT_RESUME_POLL_BACKOFF_AFTER,
    STREAM_EVENT_RESUME_POLL_BACKOFF_FACTOR, STREAM_EVENT_RESUME_POLL_INTERVAL_S,
    STREAM_EVENT_RESUME_POLL_MAX_INTERVAL_S, STREAM_EVENT_SLOW_CLIENT_QUEUE_WATERMARK,
    STREAM_EVENT_SLOW_CLIENT_WARN_INTERVAL_S,
};
use crate::schemas::StreamEvent;
use crate::state::AppState;
use axum::extract::ws::Message;
use axum::http::header::AUTHORIZATION;
use axum::http::header::SEC_WEBSOCKET_PROTOCOL;
use axum::http::{HeaderMap, HeaderValue};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub(crate) const WS_PROTOCOL_VERSION: i32 = 1;
pub(crate) const WS_PROTOCOL_MIN_VERSION: i32 = 1;
pub(crate) const WS_PROTOCOL_MAX_VERSION: i32 = 1;
pub(crate) const WS_MAX_MESSAGE_BYTES: usize = 512 * 1024;

#[derive(Debug, Deserialize)]
pub(crate) struct WsQuery {
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WsEnvelope {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub payload: Option<Value>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct WsClientInfo {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct WsConnectPayload {
    #[serde(default)]
    pub protocol_version: Option<i32>,
    #[serde(default)]
    pub min_protocol_version: Option<i32>,
    #[serde(default)]
    pub max_protocol_version: Option<i32>,
    #[serde(default)]
    pub client: Option<WsClientInfo>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct WsProtocolInfo {
    pub version: i32,
    pub min: i32,
    pub max: i32,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct WsPolicy {
    pub max_message_bytes: usize,
    pub stream_queue_size: usize,
    pub slow_client_queue_watermark: usize,
    pub resume_poll_interval_s: f64,
    pub resume_poll_max_interval_s: f64,
    pub resume_poll_backoff_factor: f64,
    pub resume_poll_backoff_after: usize,
    pub stream_event_fetch_limit: i64,
}

impl WsPolicy {
    pub(crate) fn default_policy() -> Self {
        Self {
            max_message_bytes: WS_MAX_MESSAGE_BYTES,
            stream_queue_size: STREAM_EVENT_QUEUE_SIZE,
            slow_client_queue_watermark: STREAM_EVENT_SLOW_CLIENT_QUEUE_WATERMARK,
            resume_poll_interval_s: STREAM_EVENT_RESUME_POLL_INTERVAL_S,
            resume_poll_max_interval_s: STREAM_EVENT_RESUME_POLL_MAX_INTERVAL_S,
            resume_poll_backoff_factor: STREAM_EVENT_RESUME_POLL_BACKOFF_FACTOR,
            resume_poll_backoff_after: STREAM_EVENT_RESUME_POLL_BACKOFF_AFTER,
            stream_event_fetch_limit: STREAM_EVENT_FETCH_LIMIT,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct WsFeatures {
    pub multiplex: bool,
    pub resume: bool,
    pub watch: bool,
    pub ping_pong: bool,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct WsReadyPayload {
    pub connection_id: String,
    pub server_time: f64,
    pub protocol: WsProtocolInfo,
    pub policy: WsPolicy,
    pub features: WsFeatures,
}

#[derive(Debug, Clone)]
pub(crate) struct WsHandshakeInfo {
    pub client: Option<WsClientInfo>,
    pub client_min: i32,
    pub client_max: i32,
}

#[derive(Debug)]
pub(crate) enum WsHandshakeError {
    InvalidRange { min: i32, max: i32 },
    ProtocolMismatch { min: i32, max: i32, server: i32 },
}

impl WsHandshakeError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::InvalidRange { .. } => "INVALID_PROTOCOL_RANGE",
            Self::ProtocolMismatch { .. } => "PROTOCOL_MISMATCH",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::InvalidRange { min, max } => {
                format!("invalid protocol range: min={min}, max={max}")
            }
            Self::ProtocolMismatch { min, max, server } => {
                format!("protocol mismatch: server={server}, client_range={min}..{max}")
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct WsSender {
    tx: mpsc::Sender<Message>,
    slow_warn_at: Arc<AtomicU64>,
}

impl WsSender {
    pub(crate) fn new(tx: mpsc::Sender<Message>) -> Self {
        Self {
            tx,
            slow_warn_at: Arc::new(AtomicU64::new(0)),
        }
    }
}

pub(crate) fn apply_ws_auth_headers(headers: &HeaderMap, query: &WsQuery) -> HeaderMap {
    let mut auth_headers = headers.clone();
    if auth_headers.get(AUTHORIZATION).is_none() {
        let token = extract_ws_protocol_token(headers).or_else(|| {
            query
                .access_token
                .as_deref()
                .or_else(|| query.token.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        });
        if let Some(token) = token {
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
                auth_headers.insert(AUTHORIZATION, value);
            }
        }
    }
    auth_headers
}

pub(crate) fn resolve_session_id(
    primary: Option<String>,
    secondary: Option<String>,
) -> Option<String> {
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

pub(crate) fn ws_protocol_info() -> WsProtocolInfo {
    WsProtocolInfo {
        version: WS_PROTOCOL_VERSION,
        min: WS_PROTOCOL_MIN_VERSION,
        max: WS_PROTOCOL_MAX_VERSION,
    }
}

pub(crate) fn parse_connect_payload(
    payload: Option<Value>,
) -> Result<WsConnectPayload, WsPayloadError> {
    match payload {
        Some(payload) => serde_json::from_value(payload)
            .map_err(|err| WsPayloadError::Invalid(format!("invalid payload: {err}"))),
        None => Ok(WsConnectPayload::default()),
    }
}

pub(crate) fn negotiate_ws_protocol(
    payload: &WsConnectPayload,
) -> Result<WsHandshakeInfo, WsHandshakeError> {
    let mut min = payload.min_protocol_version;
    let mut max = payload.max_protocol_version;
    if min.is_none() && max.is_none() {
        if let Some(version) = payload.protocol_version {
            min = Some(version);
            max = Some(version);
        }
    }
    let min = min.unwrap_or(WS_PROTOCOL_VERSION);
    let max = max.unwrap_or(WS_PROTOCOL_VERSION);
    if min <= 0 || max <= 0 || min > max {
        return Err(WsHandshakeError::InvalidRange { min, max });
    }
    if WS_PROTOCOL_VERSION < min || WS_PROTOCOL_VERSION > max {
        return Err(WsHandshakeError::ProtocolMismatch {
            min,
            max,
            server: WS_PROTOCOL_VERSION,
        });
    }
    Ok(WsHandshakeInfo {
        client: payload.client.clone(),
        client_min: min,
        client_max: max,
    })
}

#[derive(Debug)]
pub(crate) enum WsPayloadError {
    Missing,
    Invalid(String),
}

impl WsPayloadError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Missing => "PAYLOAD_REQUIRED",
            Self::Invalid(_) => "INVALID_PAYLOAD",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Missing => i18n::t("error.param_required"),
            Self::Invalid(message) => message.clone(),
        }
    }
}

pub(crate) fn parse_payload<T: for<'de> Deserialize<'de>>(
    payload: Option<Value>,
) -> Result<T, WsPayloadError> {
    let Some(payload) = payload else {
        return Err(WsPayloadError::Missing);
    };
    serde_json::from_value(payload)
        .map_err(|err| WsPayloadError::Invalid(format!("invalid payload: {err}")))
}

fn build_ws_text(kind: &str, request_id: Option<&str>, payload: Option<Value>) -> String {
    let mut map = serde_json::Map::new();
    map.insert("type".to_string(), json!(kind));
    if let Some(request_id) = request_id {
        map.insert("request_id".to_string(), json!(request_id));
    }
    if let Some(payload) = payload {
        map.insert("payload".to_string(), payload);
    }
    Value::Object(map).to_string()
}

fn should_warn_slow_client(sender: &WsSender) -> bool {
    let now = Utc::now().timestamp().max(0) as u64;
    let last = sender.slow_warn_at.load(Ordering::Relaxed);
    if now.saturating_sub(last) < STREAM_EVENT_SLOW_CLIENT_WARN_INTERVAL_S {
        return false;
    }
    sender.slow_warn_at.store(now, Ordering::Relaxed);
    true
}

fn try_send_text(sender: &WsSender, text: String) -> Result<(), ()> {
    match sender.tx.try_send(Message::Text(text.into())) {
        Ok(()) => Ok(()),
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Ok(()),
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Err(()),
    }
}

pub(crate) async fn send_ws_ready(
    tx: &WsSender,
    request_id: Option<&str>,
    payload: WsReadyPayload,
) -> Result<(), ()> {
    send_ws_message(tx, "ready", request_id, Some(json!(payload))).await
}

pub(crate) async fn send_ws_pong(tx: &WsSender) -> Result<(), ()> {
    send_ws_message(
        tx,
        "pong",
        None,
        Some(json!({ "ts": Utc::now().timestamp() })),
    )
    .await
}

pub(crate) async fn send_ws_event(
    tx: &WsSender,
    request_id: Option<&str>,
    event: StreamEvent,
) -> Result<(), ()> {
    let is_delta = event.event == "llm_output_delta";
    let queue_capacity = tx.tx.capacity();
    let payload = json!({
        "event": event.event,
        "id": event.id,
        "data": event.data,
    });
    if is_delta {
        if queue_capacity <= STREAM_EVENT_SLOW_CLIENT_QUEUE_WATERMARK {
            if should_warn_slow_client(tx) {
                let warning_payload = json!({
                    "event": "slow_client",
                    "data": {
                        "reason": "queue_backpressure",
                        "queue_capacity": queue_capacity,
                        "ts": Utc::now().to_rfc3339(),
                    },
                });
                let warning_text = build_ws_text("event", request_id, Some(warning_payload));
                if try_send_text(tx, warning_text).is_err() {
                    return Err(());
                }
            }
            return Ok(());
        }
        let text = build_ws_text("event", request_id, Some(payload));
        return try_send_text(tx, text);
    }
    send_ws_message(tx, "event", request_id, Some(payload)).await
}

pub(crate) async fn send_ws_error(
    tx: &WsSender,
    request_id: Option<&str>,
    code: &str,
    message: String,
) -> Result<(), ()> {
    let status = crate::api::errors::status_for_error_code(code);
    let payload = crate::api::errors::build_error_meta(
        status,
        Some(code),
        message,
        crate::api::errors::hint_for_error_code(code),
    )
    .to_value();
    send_ws_message(tx, "error", request_id, Some(payload)).await
}

pub(crate) async fn send_ws_message(
    tx: &WsSender,
    kind: &str,
    request_id: Option<&str>,
    payload: Option<Value>,
) -> Result<(), ()> {
    let text = build_ws_text(kind, request_id, payload);
    tx.tx.send(Message::Text(text.into())).await.map_err(|_| ())
}

pub(crate) async fn resume_stream_events(
    state: Arc<AppState>,
    session_id: String,
    after_event_id: i64,
    request_id: Option<&str>,
    tx: WsSender,
    cancel: Option<CancellationToken>,
    keep_alive: bool,
) {
    let workspace = state.workspace.clone();
    let monitor = state.monitor.clone();
    let base_interval = std::time::Duration::from_secs_f64(STREAM_EVENT_RESUME_POLL_INTERVAL_S);
    let mut idle_rounds: usize = 0;
    let mut poll_interval = base_interval;
    let mut last_event_id = after_event_id;
    loop {
        if cancel
            .as_ref()
            .map(|token| token.is_cancelled())
            .unwrap_or(false)
        {
            return;
        }
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

        if keep_alive && !running && last_event_id <= 0 {
            idle_rounds = idle_rounds.saturating_add(1);
            if idle_rounds > STREAM_EVENT_RESUME_POLL_BACKOFF_AFTER {
                let next = poll_interval.as_secs_f64() * STREAM_EVENT_RESUME_POLL_BACKOFF_FACTOR;
                poll_interval = std::time::Duration::from_secs_f64(
                    next.min(STREAM_EVENT_RESUME_POLL_MAX_INTERVAL_S),
                );
            }
            if let Some(token) = cancel.as_ref() {
                tokio::select! {
                    _ = token.cancelled() => return,
                    _ = tokio::time::sleep(poll_interval) => {}
                }
            } else {
                tokio::time::sleep(poll_interval).await;
            }
            continue;
        }

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
            if !running && !keep_alive {
                break;
            }
            idle_rounds = idle_rounds.saturating_add(1);
            if idle_rounds > STREAM_EVENT_RESUME_POLL_BACKOFF_AFTER {
                let next = poll_interval.as_secs_f64() * STREAM_EVENT_RESUME_POLL_BACKOFF_FACTOR;
                poll_interval = std::time::Duration::from_secs_f64(
                    next.min(STREAM_EVENT_RESUME_POLL_MAX_INTERVAL_S),
                );
            }
            if let Some(token) = cancel.as_ref() {
                tokio::select! {
                    _ = token.cancelled() => return,
                    _ = tokio::time::sleep(poll_interval) => {}
                }
            } else {
                tokio::time::sleep(poll_interval).await;
            }
        } else {
            idle_rounds = 0;
            poll_interval = base_interval;
        }
    }
}

pub(crate) fn has_ws_protocol_token(headers: &HeaderMap) -> bool {
    extract_ws_protocol_token(headers).is_some()
}

fn extract_ws_protocol_token(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(SEC_WEBSOCKET_PROTOCOL)?.to_str().ok()?;
    for raw in header.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        if let Some(rest) = item.strip_prefix("wunder-auth.") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Some(rest) = item.strip_prefix("access_token.") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Some(rest) = item.strip_prefix("token.") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
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
