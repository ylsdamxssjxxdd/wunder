use crate::core::blocking;
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
use anyhow::Error;
use axum::extract::ws::Message;
use axum::http::header::AUTHORIZATION;
use axum::http::header::SEC_WEBSOCKET_PROTOCOL;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
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
const STREAM_EVENT_HEARTBEAT_INTERVAL_S: f64 = 15.0;

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
    pub goal: bool,
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
                .or(query.token.as_deref())
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

enum WsTrySendError {
    Full,
    Closed,
}

fn try_send_text_lossy(sender: &WsSender, text: String) -> Result<(), WsTrySendError> {
    match sender.tx.try_send(Message::Text(text.into())) {
        Ok(()) => Ok(()),
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Ok(()),
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Err(WsTrySendError::Closed),
    }
}

fn try_send_text_strict(sender: &WsSender, text: String) -> Result<(), WsTrySendError> {
    match sender.tx.try_send(Message::Text(text.into())) {
        Ok(()) => Ok(()),
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Err(WsTrySendError::Full),
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Err(WsTrySendError::Closed),
    }
}

async fn send_text_backpressured(sender: &WsSender, text: String) -> Result<(), WsTrySendError> {
    sender
        .tx
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| WsTrySendError::Closed)
}

fn send_slow_client_warning(
    sender: &WsSender,
    request_id: Option<&str>,
    reason: &str,
    queue_capacity: usize,
    strict: bool,
) -> Result<(), WsTrySendError> {
    if !should_warn_slow_client(sender) {
        return Ok(());
    }
    let warning_payload = json!({
        "event": "slow_client",
        "data": {
            "reason": reason,
            "queue_capacity": queue_capacity,
            "resume_recommended": true,
            "ts": Utc::now().to_rfc3339(),
        },
    });
    let warning_text = build_ws_text("event", request_id, Some(warning_payload));
    if strict {
        try_send_text_strict(sender, warning_text)
    } else {
        try_send_text_lossy(sender, warning_text)
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
    let event_name = event.event.clone();
    let is_delta = event_name == "llm_output_delta";
    let event_id = event.id.clone();
    let data = enrich_ws_event_data(event.data, event_id.as_deref());
    let payload = json!({
        "event": event_name,
        "id": event_id,
        "data": data,
    });
    if is_delta {
        let text = build_ws_text("event", request_id, Some(payload));
        return send_text_backpressured(tx, text).await.map_err(|_| ());
    }
    let text = build_ws_text("event", request_id, Some(payload.clone()));
    match try_send_text_strict(tx, text) {
        Ok(()) => Ok(()),
        Err(WsTrySendError::Full) => {
            let _ = send_slow_client_warning(
                tx,
                request_id,
                "queue_full_resume_required",
                tx.tx.capacity(),
                false,
            );
            let retry_text = build_ws_text("event", request_id, Some(payload));
            send_text_backpressured(tx, retry_text)
                .await
                .map_err(|_| ())
        }
        Err(WsTrySendError::Closed) => Err(()),
    }
}

fn enrich_ws_event_data(data: Value, event_id: Option<&str>) -> Value {
    let Some(parsed_id) = event_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<i64>().ok())
    else {
        return data;
    };
    let mut data = data;
    if let Value::Object(ref mut map) = data {
        map.entry("event_id".to_string())
            .or_insert_with(|| json!(parsed_id));
        map.entry("event_seq".to_string())
            .or_insert_with(|| json!(parsed_id));
        if let Some(Value::Object(inner)) = map.get_mut("data") {
            inner
                .entry("event_id".to_string())
                .or_insert_with(|| json!(parsed_id));
            inner
                .entry("event_seq".to_string())
                .or_insert_with(|| json!(parsed_id));
        }
    }
    data
}

pub(crate) async fn send_ws_error(
    tx: &WsSender,
    request_id: Option<&str>,
    code: &str,
    message: String,
) -> Result<(), ()> {
    let payload = json!({
        "code": code,
        "message": message,
    });
    send_ws_error_payload(tx, request_id, payload).await
}

pub(crate) async fn send_ws_error_payload(
    tx: &WsSender,
    request_id: Option<&str>,
    payload: Value,
) -> Result<(), ()> {
    let payload = finalize_ws_error_payload(payload);
    send_ws_message(tx, "error", request_id, Some(payload)).await
}

pub(crate) fn ws_error_payload_from_anyhow(err: &Error) -> Value {
    if let Some(orchestrator_err) = err.downcast_ref::<crate::orchestrator::OrchestratorError>() {
        return orchestrator_err.to_payload();
    }
    json!({
        "status": StatusCode::BAD_REQUEST.as_u16(),
        "code": "INTERNAL_ERROR",
        "message": err.to_string(),
    })
}

fn finalize_ws_error_payload(payload: Value) -> Value {
    let status = payload
        .get("status")
        .and_then(Value::as_u64)
        .and_then(|raw| u16::try_from(raw).ok())
        .and_then(|raw| StatusCode::from_u16(raw).ok())
        .or_else(|| {
            payload
                .get("code")
                .and_then(Value::as_str)
                .map(crate::api::errors::status_for_error_code)
        })
        .unwrap_or(StatusCode::BAD_REQUEST);
    let code = payload.get("code").and_then(Value::as_str);
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| status.to_string());
    let hint = payload
        .get("hint")
        .and_then(Value::as_str)
        .or_else(|| code.and_then(crate::api::errors::hint_for_error_code));
    let mut merged = crate::api::errors::build_error_meta(status, code, message, hint).to_value();
    if let (Value::Object(target), Value::Object(source)) = (&mut merged, &payload) {
        for key in ["detail", "error_meta", "trace_id", "timestamp"] {
            if let Some(value) = source.get(key) {
                target.insert(key.to_string(), value.clone());
            }
        }
    }
    merged
}

pub(crate) async fn send_ws_message(
    tx: &WsSender,
    kind: &str,
    request_id: Option<&str>,
    payload: Option<Value>,
) -> Result<(), ()> {
    let text = build_ws_text(kind, request_id, payload);
    try_send_text_strict(tx, text).map_err(|_| ())
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
    let user_store = state.user_store.clone();
    let base_interval = std::time::Duration::from_secs_f64(STREAM_EVENT_RESUME_POLL_INTERVAL_S);
    let heartbeat_interval = std::time::Duration::from_secs_f64(STREAM_EVENT_HEARTBEAT_INTERVAL_S);
    let mut idle_rounds: usize = 0;
    let mut poll_interval = base_interval;
    let mut last_event_id = after_event_id;
    let mut last_heartbeat = std::time::Instant::now();
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
                    .map(is_stream_active_status)
                    .unwrap_or(false)
            })
            .unwrap_or(false)
            || has_active_queue_task(user_store.as_ref(), &session_id);

        let session_id_snapshot = session_id.clone();
        let workspace_snapshot = workspace.clone();
        let records = blocking::run_fs("api.ws_helpers.resume_stream_events", move || {
            Ok(workspace_snapshot.load_stream_events(
                &session_id_snapshot,
                last_event_id,
                STREAM_EVENT_FETCH_LIMIT,
            ))
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
            if running && last_heartbeat.elapsed() >= heartbeat_interval {
                let heartbeat = StreamEvent {
                    event: "heartbeat".to_string(),
                    data: json!({
                        "ts": Utc::now().to_rfc3339(),
                        "running": running,
                    }),
                    id: None,
                    timestamp: Some(Utc::now()),
                };
                if send_ws_event(&tx, request_id, heartbeat).await.is_err() {
                    return;
                }
                last_heartbeat = std::time::Instant::now();
            }
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
            last_heartbeat = std::time::Instant::now();
            idle_rounds = 0;
            poll_interval = base_interval;
        }
    }
}

pub(crate) async fn resume_queued_stream_events(
    state: Arc<AppState>,
    session_id: String,
    queue_id: String,
    after_event_id: i64,
    request_id: Option<&str>,
    tx: WsSender,
    cancel: Option<CancellationToken>,
) {
    let workspace = state.workspace.clone();
    let user_store = state.user_store.clone();
    let base_interval = std::time::Duration::from_secs_f64(STREAM_EVENT_RESUME_POLL_INTERVAL_S);
    let heartbeat_interval = std::time::Duration::from_secs_f64(STREAM_EVENT_HEARTBEAT_INTERVAL_S);
    let mut idle_rounds: usize = 0;
    let mut poll_interval = base_interval;
    let mut last_event_id = after_event_id;
    let mut last_heartbeat = std::time::Instant::now();
    let mut saw_terminal_queue_event = false;
    let mut saw_queue_start = false;
    loop {
        if cancel
            .as_ref()
            .map(|token| token.is_cancelled())
            .unwrap_or(false)
        {
            return;
        }
        let session_id_snapshot = session_id.clone();
        let workspace_snapshot = workspace.clone();
        let records = blocking::run_fs("api.ws_helpers.resume_queued_stream_events", move || {
            Ok(workspace_snapshot.load_stream_events(
                &session_id_snapshot,
                last_event_id,
                STREAM_EVENT_FETCH_LIMIT,
            ))
        })
        .await
        .unwrap_or_default();
        let mut progressed = false;
        for record in records {
            let Some(event) = map_stream_event(record) else {
                continue;
            };
            progressed = true;
            let parsed_id = event
                .id
                .as_ref()
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0);
            if parsed_id > last_event_id {
                last_event_id = parsed_id;
            }
            let queue_event_kind = queue_event_kind(&event);
            let matches_queue = queue_event_kind
                .map(|_| is_queue_event_for_queue(&event, &queue_id))
                .unwrap_or(false);
            let is_terminal_queue_event = queue_event_kind
                .is_some_and(|kind| is_terminal_queue_event_kind(kind))
                && matches_queue;
            let should_forward = if let Some(kind) = queue_event_kind {
                if !matches_queue {
                    false
                } else {
                    if kind == "queue_start" {
                        saw_queue_start = true;
                    }
                    true
                }
            } else {
                saw_queue_start
            };
            if should_forward && send_ws_event(&tx, request_id, event).await.is_err() {
                return;
            }
            if is_terminal_queue_event {
                saw_terminal_queue_event = true;
                break;
            }
        }
        if saw_terminal_queue_event {
            break;
        }
        if !progressed {
            let queue_running = has_active_queue_task(user_store.as_ref(), &session_id);
            if queue_running && last_heartbeat.elapsed() >= heartbeat_interval {
                let heartbeat = StreamEvent {
                    event: "heartbeat".to_string(),
                    data: json!({
                        "ts": Utc::now().to_rfc3339(),
                        "running": true,
                    }),
                    id: None,
                    timestamp: Some(Utc::now()),
                };
                if send_ws_event(&tx, request_id, heartbeat).await.is_err() {
                    return;
                }
                last_heartbeat = std::time::Instant::now();
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
            last_heartbeat = std::time::Instant::now();
            idle_rounds = 0;
            poll_interval = base_interval;
        }
    }
}

fn is_stream_active_status(status: &str) -> bool {
    matches!(
        status,
        MonitorState::STATUS_RUNNING
            | MonitorState::STATUS_CANCELLING
            | MonitorState::STATUS_QUEUED
            | MonitorState::STATUS_WAITING
    )
}

fn has_active_queue_task(user_store: &crate::user_store::UserStore, session_id: &str) -> bool {
    let cleaned_session = session_id.trim();
    if cleaned_session.is_empty() {
        return false;
    }
    let thread_id = format!("thread_{cleaned_session}");
    user_store
        .list_agent_tasks_by_thread(&thread_id, None, 8)
        .map(|tasks| {
            tasks.iter().any(|task| {
                task.status == "pending" || task.status == "retry" || task.status == "running"
            })
        })
        .unwrap_or(false)
}

fn queue_event_kind(event: &StreamEvent) -> Option<&str> {
    let normalized_event = event.event.trim();
    match normalized_event {
        "queue_enter" | "queue_update" | "queue_start" | "queue_finish" | "queue_fail" => {
            Some(normalized_event)
        }
        _ => None,
    }
}

fn is_terminal_queue_event_kind(kind: &str) -> bool {
    kind == "queue_finish" || kind == "queue_fail"
}

fn is_queue_event_for_queue(event: &StreamEvent, queue_id: &str) -> bool {
    let expected_queue_id = queue_id.trim();
    if expected_queue_id.is_empty() {
        return true;
    }
    let candidate = event
        .data
        .get("queue_id")
        .or_else(|| event.data.get("queueId"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    candidate == expected_queue_id
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config_store::ConfigStore;
    use crate::state::{AppState, AppStateInitOptions};
    use serde_json::json;
    use std::time::Duration;

    async fn build_test_state(name: &str) -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_dir.path().to_path_buf();
        let mut config = Config::default();
        config.storage.backend = "sqlite".to_string();
        config.storage.db_path = root
            .join(format!("{name}.db"))
            .to_string_lossy()
            .to_string();
        config.workspace.root = root.join("workspaces").to_string_lossy().to_string();
        config.skills.enabled.clear();
        let config_store = ConfigStore::new(root.join("wunder.yaml"));
        let config_for_store = config.clone();
        config_store
            .update(|current| *current = config_for_store.clone())
            .await
            .expect("write config");
        let state = Arc::new(
            AppState::new_with_options(
                config_store,
                config,
                AppStateInitOptions::cli_default().with_start_thread_runtime(false),
            )
            .expect("create app state"),
        );
        (state, temp_dir)
    }

    fn parse_ws_event_type(raw: &str) -> String {
        let payload: Value = serde_json::from_str(raw).expect("parse ws payload");
        payload
            .get("payload")
            .and_then(|value| value.get("event"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }

    fn parse_ws_event_id(raw: &str) -> i64 {
        let payload: Value = serde_json::from_str(raw).expect("parse ws payload");
        payload
            .get("payload")
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0)
    }

    #[test]
    fn queue_event_matching_requires_queue_id_when_expected_is_known() {
        let event = StreamEvent {
            event: "queue_finish".to_string(),
            data: json!({}),
            id: Some("1".to_string()),
            timestamp: None,
        };
        assert!(!is_queue_event_for_queue(&event, "queue-known"));
        assert!(is_queue_event_for_queue(&event, ""));
    }

    #[tokio::test]
    async fn queued_resume_uses_anchor_and_stops_at_matching_terminal_event() {
        let (state, _temp_dir) = build_test_state("queued-resume-anchor").await;
        let session_id = "sess_queued_resume_anchor";
        let user_id = "user_queued_resume_anchor";

        state
            .storage
            .append_stream_event(
                session_id,
                user_id,
                1,
                &json!({
                    "event": "llm_output_delta",
                    "data": { "delta": "old" },
                    "timestamp": "2026-03-07T00:00:00+08:00"
                }),
            )
            .expect("append old event");
        state
            .storage
            .append_stream_event(
                session_id,
                user_id,
                2,
                &json!({
                    "event": "queue_enter",
                    "data": { "queue_id": "queue-a" },
                    "timestamp": "2026-03-07T00:00:01+08:00"
                }),
            )
            .expect("append queue enter");
        state
            .storage
            .append_stream_event(
                session_id,
                user_id,
                3,
                &json!({
                    "event": "queue_start",
                    "data": { "queue_id": "queue-a" },
                    "timestamp": "2026-03-07T00:00:02+08:00"
                }),
            )
            .expect("append queue start");
        state
            .storage
            .append_stream_event(
                session_id,
                user_id,
                4,
                &json!({
                    "event": "llm_output_delta",
                    "data": { "delta": "current" },
                    "timestamp": "2026-03-07T00:00:03+08:00"
                }),
            )
            .expect("append current delta");
        state
            .storage
            .append_stream_event(
                session_id,
                user_id,
                5,
                &json!({
                    "event": "queue_finish",
                    "data": { "queue_id": "queue-a" },
                    "timestamp": "2026-03-07T00:00:04+08:00"
                }),
            )
            .expect("append queue finish");
        state
            .storage
            .append_stream_event(
                session_id,
                user_id,
                6,
                &json!({
                    "event": "llm_output_delta",
                    "data": { "delta": "next-turn" },
                    "timestamp": "2026-03-07T00:00:05+08:00"
                }),
            )
            .expect("append later event");

        let (tx, mut rx) = mpsc::channel::<Message>(8);
        resume_queued_stream_events(
            state,
            session_id.to_string(),
            "queue-a".to_string(),
            1,
            Some("req-queue-a"),
            WsSender::new(tx),
            None,
        )
        .await;

        let mut events = Vec::new();
        while let Ok(Some(Message::Text(raw))) =
            tokio::time::timeout(Duration::from_millis(20), rx.recv()).await
        {
            events.push(raw.to_string());
        }
        assert_eq!(
            events
                .iter()
                .map(|raw| parse_ws_event_type(raw))
                .collect::<Vec<_>>(),
            vec![
                "queue_enter",
                "queue_start",
                "llm_output_delta",
                "queue_finish"
            ]
        );
        assert_eq!(
            events
                .iter()
                .map(|raw| parse_ws_event_id(raw))
                .collect::<Vec<_>>(),
            vec![2, 3, 4, 5]
        );
    }

    #[tokio::test]
    async fn queued_resume_does_not_forward_previous_task_tail_before_queue_start() {
        let (state, _temp_dir) = build_test_state("queued-resume-previous-tail").await;
        let session_id = "sess_queued_resume_previous_tail";
        let user_id = "user_queued_resume_previous_tail";
        for (event_id, payload) in [
            (
                1,
                json!({
                    "event": "queue_enter",
                    "data": { "queue_id": "queue-new" },
                    "timestamp": "2026-03-07T00:00:00+08:00"
                }),
            ),
            (
                2,
                json!({
                    "event": "llm_output_delta",
                    "data": { "delta": "old-task-tail" },
                    "timestamp": "2026-03-07T00:00:01+08:00"
                }),
            ),
            (
                3,
                json!({
                    "event": "queue_finish",
                    "data": { "queue_id": "queue-old" },
                    "timestamp": "2026-03-07T00:00:02+08:00"
                }),
            ),
            (
                4,
                json!({
                    "event": "queue_start",
                    "data": { "queue_id": "queue-new" },
                    "timestamp": "2026-03-07T00:00:03+08:00"
                }),
            ),
            (
                5,
                json!({
                    "event": "llm_output_delta",
                    "data": { "delta": "new-task" },
                    "timestamp": "2026-03-07T00:00:04+08:00"
                }),
            ),
            (
                6,
                json!({
                    "event": "queue_finish",
                    "data": { "queue_id": "queue-new" },
                    "timestamp": "2026-03-07T00:00:05+08:00"
                }),
            ),
        ] {
            state
                .storage
                .append_stream_event(session_id, user_id, event_id, &payload)
                .expect("append stream event");
        }

        let (tx, mut rx) = mpsc::channel::<Message>(8);
        resume_queued_stream_events(
            state,
            session_id.to_string(),
            "queue-new".to_string(),
            0,
            Some("req-queue-new"),
            WsSender::new(tx),
            None,
        )
        .await;

        let mut events = Vec::new();
        while let Ok(Some(Message::Text(raw))) =
            tokio::time::timeout(Duration::from_millis(20), rx.recv()).await
        {
            events.push(raw.to_string());
        }
        assert_eq!(
            events
                .iter()
                .map(|raw| parse_ws_event_type(raw))
                .collect::<Vec<_>>(),
            vec![
                "queue_enter",
                "queue_start",
                "llm_output_delta",
                "queue_finish"
            ]
        );
        assert_eq!(
            events
                .iter()
                .map(|raw| parse_ws_event_id(raw))
                .collect::<Vec<_>>(),
            vec![1, 4, 5, 6]
        );
    }

    #[tokio::test]
    async fn non_delta_event_waits_for_queue_capacity_and_succeeds() {
        let (tx, mut rx) = mpsc::channel::<Message>(1);
        tx.try_send(Message::Text("busy".into()))
            .expect("fill queue");
        let sender = WsSender::new(tx);
        let drain = tokio::spawn(async move {
            let first = rx.recv().await.expect("first queued message");
            assert!(matches!(first, Message::Text(_)));
            rx.recv().await.expect("second queued message")
        });
        let event = StreamEvent {
            event: "tool_call".to_string(),
            data: json!({"tool":"read_file"}),
            id: Some("1".to_string()),
            timestamp: None,
        };
        assert!(send_ws_event(&sender, Some("req-1"), event).await.is_ok());
        let second = drain.await.expect("drain task should finish");
        let Message::Text(raw) = second else {
            panic!("expected tool_call message");
        };
        assert!(raw.contains("\"event\":\"tool_call\""));
    }

    #[tokio::test]
    async fn ws_event_data_exposes_event_sequence_for_projection_replay() {
        let (tx, mut rx) = mpsc::channel::<Message>(4);
        let sender = WsSender::new(tx);
        let event = StreamEvent {
            event: "final".to_string(),
            data: json!({"data":{"answer":"ok"}}),
            id: Some("7".to_string()),
            timestamp: None,
        };
        assert!(send_ws_event(&sender, Some("req-seq"), event).await.is_ok());
        let message = rx.recv().await.expect("ws event");
        let Message::Text(raw) = message else {
            panic!("expected text ws event");
        };
        let payload: Value = serde_json::from_str(&raw).expect("ws json");
        let data = payload
            .get("payload")
            .and_then(|value| value.get("data"))
            .expect("event data");
        assert_eq!(data.get("event_id").and_then(Value::as_i64), Some(7));
        assert_eq!(data.get("event_seq").and_then(Value::as_i64), Some(7));
        assert_eq!(
            data.get("data")
                .and_then(|value| value.get("event_seq"))
                .and_then(Value::as_i64),
            Some(7)
        );
    }

    #[tokio::test]
    async fn delta_event_waits_for_queue_capacity_and_succeeds() {
        let (tx, mut rx) = mpsc::channel::<Message>(1);
        tx.try_send(Message::Text("busy".into()))
            .expect("fill queue");
        let sender = WsSender::new(tx);
        let event = StreamEvent {
            event: "llm_output_delta".to_string(),
            data: json!({"delta":"hello"}),
            id: Some("2".to_string()),
            timestamp: None,
        };
        let send = tokio::spawn(async move { send_ws_event(&sender, Some("req-2"), event).await });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            !send.is_finished(),
            "delta send should wait while the queue is full"
        );
        let first = rx.recv().await.expect("queued busy message");
        assert!(matches!(first, Message::Text(_)));
        assert!(send.await.expect("send task should finish").is_ok());
        let second = rx.recv().await.expect("delta message");
        let Message::Text(raw) = second else {
            panic!("expected delta message");
        };
        assert!(raw.contains("\"event\":\"llm_output_delta\""));
        assert!(raw.contains("\"delta\":\"hello\""));
    }

    #[tokio::test]
    async fn structured_error_payload_preserves_detail_fields() {
        let (tx, mut rx) = mpsc::channel::<Message>(4);
        let sender = WsSender::new(tx);
        send_ws_error_payload(
            &sender,
            Some("req-structured"),
            json!({
                "code": "INVALID_REQUEST",
                "message": "input too long",
                "detail": {
                    "field": "input_text",
                    "max_chars": 10,
                    "actual_chars": 12
                }
            }),
        )
        .await
        .expect("send error payload");

        let Some(Message::Text(raw)) = rx.recv().await else {
            panic!("expected ws text message");
        };
        let payload: Value = serde_json::from_str(raw.as_str()).expect("parse ws envelope");
        assert_eq!(payload["type"], json!("error"));
        assert_eq!(payload["request_id"], json!("req-structured"));
        assert_eq!(payload["payload"]["code"], json!("INVALID_REQUEST"));
        assert_eq!(payload["payload"]["status"], json!(400));
        assert_eq!(payload["payload"]["detail"]["field"], json!("input_text"));
        assert_eq!(payload["payload"]["detail"]["actual_chars"], json!(12));
    }

    #[tokio::test]
    async fn exception_structured_error_payload_preserves_error_meta() {
        let (tx, mut rx) = mpsc::channel::<Message>(4);
        let sender = WsSender::new(tx);
        send_ws_error_payload(
            &sender,
            Some("req-meta"),
            json!({
                "code": "LLM_UNAVAILABLE",
                "message": "provider unavailable",
                "error_meta": {
                    "category": "provider",
                    "severity": "error",
                    "retryable": true,
                    "retry_after_ms": 500,
                    "source_stage": "llm",
                    "recovery_action": "retry_later"
                }
            }),
        )
        .await
        .expect("send error payload");

        let Some(Message::Text(raw)) = rx.recv().await else {
            panic!("expected ws text message");
        };
        let payload: Value = serde_json::from_str(raw.as_str()).expect("parse ws envelope");
        assert_eq!(payload["payload"]["code"], json!("LLM_UNAVAILABLE"));
        assert_eq!(payload["payload"]["status"], json!(503));
        assert_eq!(
            payload["payload"]["error_meta"]["category"],
            json!("provider")
        );
        assert_eq!(payload["payload"]["error_meta"]["retryable"], json!(true));
        assert_eq!(
            payload["payload"]["error_meta"]["retry_after_ms"],
            json!(500)
        );
    }
}
