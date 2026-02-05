use crate::api::ws_helpers::WsClientInfo;
use axum::http::header::{AUTHORIZATION, HOST, ORIGIN, USER_AGENT};
use axum::http::HeaderMap;
use tracing::{info, warn};

#[derive(Clone, Debug, Default)]
pub(crate) struct WsConnMeta {
    pub user_agent: Option<String>,
    pub origin: Option<String>,
    pub host: Option<String>,
    pub forwarded_for: Option<String>,
    pub real_ip: Option<String>,
    pub has_protocol_token: bool,
    pub has_authorization: bool,
}

impl WsConnMeta {
    pub(crate) fn from_headers(headers: &HeaderMap, has_protocol_token: bool) -> Self {
        let pick = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string())
        };
        Self {
            user_agent: pick(USER_AGENT.as_str()),
            origin: pick(ORIGIN.as_str()),
            host: pick(HOST.as_str()),
            forwarded_for: pick("x-forwarded-for"),
            real_ip: pick("x-real-ip"),
            has_protocol_token,
            has_authorization: headers.get(AUTHORIZATION).is_some(),
        }
    }
}

pub(crate) fn log_ws_open(endpoint: &str, connection_id: &str, user_id: &str, meta: &WsConnMeta) {
    info!(
        target: "wunder.ws",
        ws_event = "open",
        endpoint,
        connection_id,
        user_id,
        auth_via_protocol = meta.has_protocol_token,
        auth_via_header = meta.has_authorization,
        user_agent = ?meta.user_agent,
        origin = ?meta.origin,
        host = ?meta.host,
        forwarded_for = ?meta.forwarded_for,
        real_ip = ?meta.real_ip,
        "ws connection opened",
    );
}

pub(crate) fn log_ws_ready(
    endpoint: &str,
    connection_id: &str,
    user_id: &str,
    protocol_version: i32,
    protocol_min: i32,
    protocol_max: i32,
) {
    info!(
        target: "wunder.ws",
        ws_event = "ready",
        endpoint,
        connection_id,
        user_id,
        protocol_version,
        protocol_min,
        protocol_max,
        "ws ready sent",
    );
}

pub(crate) fn log_ws_handshake(
    endpoint: &str,
    connection_id: &str,
    user_id: &str,
    client_min: i32,
    client_max: i32,
    implicit: bool,
    client: Option<&WsClientInfo>,
) {
    info!(
        target: "wunder.ws",
        ws_event = "handshake",
        endpoint,
        connection_id,
        user_id,
        client_min,
        client_max,
        implicit,
        client_name = ?client.and_then(|value| value.name.as_deref()),
        client_version = ?client.and_then(|value| value.version.as_deref()),
        client_platform = ?client.and_then(|value| value.platform.as_deref()),
        client_mode = ?client.and_then(|value| value.mode.as_deref()),
        "ws handshake accepted",
    );
}

pub(crate) fn log_ws_handshake_error(
    endpoint: &str,
    connection_id: &str,
    user_id: &str,
    code: &str,
    message: &str,
) {
    warn!(
        target: "wunder.ws",
        ws_event = "handshake_error",
        endpoint,
        connection_id,
        user_id,
        code,
        message,
        "ws handshake rejected",
    );
}

pub(crate) fn log_ws_message(
    endpoint: &str,
    connection_id: &str,
    user_id: &str,
    message_type: &str,
    request_id: Option<&str>,
    session_id: Option<&str>,
) {
    info!(
        target: "wunder.ws",
        ws_event = "message",
        endpoint,
        connection_id,
        user_id,
        message_type,
        request_id = ?request_id,
        session_id = ?session_id,
        "ws message received",
    );
}

pub(crate) fn log_ws_parse_error(endpoint: &str, connection_id: &str, user_id: &str, error: &str) {
    warn!(
        target: "wunder.ws",
        ws_event = "parse_error",
        endpoint,
        connection_id,
        user_id,
        error,
        "ws message parse error",
    );
}

pub(crate) fn log_ws_close(
    endpoint: &str,
    connection_id: &str,
    user_id: &str,
    code: Option<u16>,
    reason: Option<&str>,
    duration_ms: Option<u128>,
) {
    info!(
        target: "wunder.ws",
        ws_event = "close",
        endpoint,
        connection_id,
        user_id,
        code = ?code,
        reason = ?reason,
        duration_ms = ?duration_ms,
        "ws connection closed",
    );
}
