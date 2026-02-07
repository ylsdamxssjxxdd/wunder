use axum::http::header::{HeaderName, HeaderValue};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(crate) const TRACE_HEADER: &str = "x-trace-id";
pub(crate) const ERROR_CODE_HEADER: &str = "x-error-code";

#[derive(Debug, Clone)]
pub(crate) struct ErrorMeta {
    pub code: String,
    pub message: String,
    pub status: u16,
    pub hint: String,
    pub trace_id: String,
    pub timestamp: f64,
}

impl ErrorMeta {
    pub(crate) fn to_value(&self) -> Value {
        json!({
            "code": self.code,
            "message": self.message,
            "status": self.status,
            "hint": self.hint,
            "trace_id": self.trace_id,
            "timestamp": self.timestamp,
        })
    }
}

pub(crate) fn build_error_meta(
    status: StatusCode,
    code: Option<&str>,
    message: impl Into<String>,
    hint: Option<&str>,
) -> ErrorMeta {
    let message = message.into();
    let code = code
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_error_code(status))
        .to_string();
    let hint = hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_hint(status))
        .to_string();
    ErrorMeta {
        code,
        message,
        status: status.as_u16(),
        hint,
        trace_id: format!("err_{}", Uuid::new_v4().simple()),
        timestamp: now_unix_seconds(),
    }
}

pub(crate) fn status_for_error_code(code: &str) -> StatusCode {
    let normalized = code.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "AUTH_REQUIRED" | "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
        "FORBIDDEN" | "PERMISSION_DENIED" => StatusCode::FORBIDDEN,
        "NOT_FOUND" | "SESSION_NOT_FOUND" | "TASK_NOT_FOUND" => StatusCode::NOT_FOUND,
        "CONFLICT" | "TASK_NOT_CANCELABLE" => StatusCode::CONFLICT,
        "CONTENT_TYPE_NOT_SUPPORTED" => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "HANDSHAKE_TIMEOUT" | "REQUEST_TIMEOUT" => StatusCode::REQUEST_TIMEOUT,
        "PAYLOAD_TOO_LARGE" => StatusCode::PAYLOAD_TOO_LARGE,
        "RATE_LIMITED" | "USER_BUSY" | "USER_QUOTA_EXCEEDED" => StatusCode::TOO_MANY_REQUESTS,
        "PUSH_NOT_SUPPORTED" => StatusCode::NOT_IMPLEMENTED,
        "SERVICE_UNAVAILABLE" | "CONNECTION_CLOSED" => StatusCode::SERVICE_UNAVAILABLE,
        "UPSTREAM_TIMEOUT" => StatusCode::GATEWAY_TIMEOUT,
        "INTERNAL_ERROR" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    }
}

pub(crate) fn hint_for_error_code(code: &str) -> Option<&'static str> {
    let normalized = code.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "INVALID_JSON" => Some("Send valid JSON payload encoded in UTF-8."),
        "INVALID_REQUEST" | "INVALID_RESPONSE" | "INVALID_PAYLOAD" | "INVALID_PARAMS" => {
            Some("Check required fields and payload schema before retrying.")
        }
        "INVALID_HANDSHAKE" | "HANDSHAKE_TIMEOUT" => {
            Some("Send connect handshake first and complete it within timeout.")
        }
        "INVALID_PROTOCOL_RANGE" | "PROTOCOL_MISMATCH" | "VERSION_NOT_SUPPORTED" => {
            Some("Align client protocol version with the server supported range.")
        }
        "AUTH_REQUIRED" | "UNAUTHORIZED" | "PERMISSION_DENIED" | "FORBIDDEN" => {
            Some("Check authentication credentials and permission scope.")
        }
        "SESSION_REQUIRED" | "SESSION_NOT_FOUND" => {
            Some("Verify session identifier and ownership before retrying.")
        }
        "USER_BUSY" | "USER_QUOTA_EXCEEDED" | "RATE_LIMITED" => {
            Some("Retry later or reduce request frequency.")
        }
        "UNSUPPORTED_METHOD" | "METHOD_NOT_FOUND" | "UNSUPPORTED_TYPE" => {
            Some("Verify method/type against the protocol documentation.")
        }
        "CONTENT_TYPE_NOT_SUPPORTED" => Some("Use a supported content type and retry."),
        "TASK_NOT_FOUND" | "TASK_NOT_CANCELABLE" => {
            Some("Verify task id and current task state before retrying.")
        }
        "CONNECTION_CLOSED" => Some("Reconnect websocket and retry the request."),
        _ => None,
    }
}

pub fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    error_response_with_detail(status, None, message, None, None)
}

pub fn error_response_with_detail(
    status: StatusCode,
    code: Option<&str>,
    message: impl Into<String>,
    hint: Option<&str>,
    detail: Option<Value>,
) -> Response {
    let meta = build_error_meta(status, code, message, hint);
    let detail = build_detail_payload(&meta.message, detail);
    let payload = json!({
        "ok": false,
        "error": meta.to_value(),
        "detail": detail,
    });

    let mut response = (status, Json(payload)).into_response();
    if let Ok(value) = HeaderValue::from_str(&meta.trace_id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(TRACE_HEADER), value);
    }
    if let Ok(value) = HeaderValue::from_str(&meta.code) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(ERROR_CODE_HEADER), value);
    }
    response
}

fn build_detail_payload(message: &str, detail: Option<Value>) -> Value {
    match detail {
        Some(Value::Object(mut map)) => {
            map.entry("message".to_string())
                .or_insert_with(|| Value::String(message.to_string()));
            Value::Object(map)
        }
        Some(value) => json!({
            "message": message,
            "detail": value,
        }),
        None => json!({
            "message": message,
        }),
    }
}

fn default_error_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "BAD_REQUEST",
        StatusCode::UNAUTHORIZED => "UNAUTHORIZED",
        StatusCode::FORBIDDEN => "FORBIDDEN",
        StatusCode::NOT_FOUND => "NOT_FOUND",
        StatusCode::CONFLICT => "CONFLICT",
        StatusCode::REQUEST_TIMEOUT => "REQUEST_TIMEOUT",
        StatusCode::PAYLOAD_TOO_LARGE => "PAYLOAD_TOO_LARGE",
        StatusCode::TOO_MANY_REQUESTS => "RATE_LIMITED",
        StatusCode::UNPROCESSABLE_ENTITY => "UNPROCESSABLE_ENTITY",
        StatusCode::SERVICE_UNAVAILABLE => "SERVICE_UNAVAILABLE",
        StatusCode::GATEWAY_TIMEOUT => "UPSTREAM_TIMEOUT",
        _ if status.is_server_error() => "INTERNAL_ERROR",
        _ => "REQUEST_ERROR",
    }
}

fn default_hint(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "Verify request parameters and payload format.",
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            "Check authentication credentials and permission scope."
        }
        StatusCode::NOT_FOUND => "Verify requested resource path or identifier.",
        StatusCode::TOO_MANY_REQUESTS => "Retry later or reduce request frequency.",
        StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT => {
            "Service may be warming up or upstream dependency timed out."
        }
        _ if status.is_server_error() => "Retry later or contact support with trace_id.",
        _ => "Inspect request and try again.",
    }
}

fn now_unix_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn default_error_response_contains_unified_fields() {
        let response = error_response(StatusCode::BAD_REQUEST, "invalid payload");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let trace_id = response
            .headers()
            .get(TRACE_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(trace_id.starts_with("err_"));

        let error_code = response
            .headers()
            .get(ERROR_CODE_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert_eq!(error_code, "BAD_REQUEST");

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read response body");
        let payload: Value = serde_json::from_slice(&body).expect("parse response json");

        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["error"]["code"], json!("BAD_REQUEST"));
        assert_eq!(payload["error"]["message"], json!("invalid payload"));
        assert_eq!(payload["error"]["status"], json!(400));
        assert_eq!(payload["error"]["trace_id"], json!(trace_id));
        assert!(payload["error"]["timestamp"].as_f64().unwrap_or_default() > 0.0);
        assert_eq!(payload["detail"]["message"], json!("invalid payload"));
    }

    #[tokio::test]
    async fn custom_error_code_and_detail_are_preserved() {
        let response = error_response_with_detail(
            StatusCode::TOO_MANY_REQUESTS,
            Some("USER_BUSY"),
            "session is busy",
            Some("Retry with a different session or wait for completion."),
            Some(json!({ "code": "USER_BUSY" })),
        );
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        let error_code = response
            .headers()
            .get(ERROR_CODE_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert_eq!(error_code, "USER_BUSY");

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read response body");
        let payload: Value = serde_json::from_slice(&body).expect("parse response json");

        assert_eq!(payload["error"]["code"], json!("USER_BUSY"));
        assert_eq!(
            payload["error"]["hint"],
            json!("Retry with a different session or wait for completion.")
        );
        assert_eq!(payload["detail"]["code"], json!("USER_BUSY"));
        assert_eq!(payload["detail"]["message"], json!("session is busy"));
    }

    #[test]
    fn status_mapping_for_custom_error_code_is_stable() {
        assert_eq!(
            status_for_error_code("USER_QUOTA_EXCEEDED"),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            status_for_error_code("SESSION_NOT_FOUND"),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status_for_error_code("CONTENT_TYPE_NOT_SUPPORTED"),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
    }
}
