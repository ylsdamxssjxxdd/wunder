use crate::i18n;
use crate::monitor::MonitorState;
use crate::orchestrator_constants::{
    STREAM_EVENT_FETCH_LIMIT, STREAM_EVENT_RESUME_POLL_INTERVAL_S,
};
use crate::schemas::StreamEvent;
use crate::state::AppState;
use axum::extract::ws::Message;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderValue};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;

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

pub(crate) fn apply_ws_auth_headers(headers: &HeaderMap, query: &WsQuery) -> HeaderMap {
    let mut auth_headers = headers.clone();
    if auth_headers.get(AUTHORIZATION).is_none() {
        let token = query
            .access_token
            .as_deref()
            .or_else(|| query.token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty());
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

pub(crate) async fn send_ws_ready(
    tx: &mpsc::Sender<Message>,
    connection_id: &str,
    now_ts: f64,
) -> Result<(), ()> {
    let payload = json!({
        "connection_id": connection_id,
        "server_time": now_ts,
    });
    send_ws_message(tx, "ready", None, Some(payload)).await
}

pub(crate) async fn send_ws_pong(tx: &mpsc::Sender<Message>) -> Result<(), ()> {
    send_ws_message(
        tx,
        "pong",
        None,
        Some(json!({ "ts": Utc::now().timestamp() })),
    )
    .await
}

pub(crate) async fn send_ws_event(
    tx: &mpsc::Sender<Message>,
    request_id: Option<&str>,
    event: StreamEvent,
) -> Result<(), ()> {
    let payload = json!({
        "event": event.event,
        "id": event.id,
        "data": event.data,
    });
    send_ws_message(tx, "event", request_id, Some(payload)).await
}

pub(crate) async fn send_ws_error(
    tx: &mpsc::Sender<Message>,
    request_id: Option<&str>,
    code: &str,
    message: String,
) -> Result<(), ()> {
    let payload = json!({
        "code": code,
        "message": message,
    });
    send_ws_message(tx, "error", request_id, Some(payload)).await
}

pub(crate) async fn send_ws_message(
    tx: &mpsc::Sender<Message>,
    kind: &str,
    request_id: Option<&str>,
    payload: Option<Value>,
) -> Result<(), ()> {
    let mut map = serde_json::Map::new();
    map.insert("type".to_string(), json!(kind));
    if let Some(request_id) = request_id {
        map.insert("request_id".to_string(), json!(request_id));
    }
    if let Some(payload) = payload {
        map.insert("payload".to_string(), payload);
    }
    let text = Value::Object(map).to_string();
    tx.send(Message::Text(text.into())).await.map_err(|_| ())
}

pub(crate) async fn resume_stream_events(
    state: Arc<AppState>,
    session_id: String,
    after_event_id: i64,
    request_id: Option<&str>,
    tx: mpsc::Sender<Message>,
) {
    let workspace = state.workspace.clone();
    let monitor = state.monitor.clone();
    let poll_interval = std::time::Duration::from_secs_f64(STREAM_EVENT_RESUME_POLL_INTERVAL_S);
    let mut last_event_id = after_event_id;
    loop {
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
            if !running {
                break;
            }
            tokio::time::sleep(poll_interval).await;
        }
    }
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
