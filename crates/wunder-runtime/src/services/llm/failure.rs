use super::truncate_text;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderFailureKind {
    Permanent,
    InvalidRequest,
    ContextWindow,
    Transient,
}

fn provider_failure_kind(error: &Value) -> Option<ProviderFailureKind> {
    [error.get("code"), error.get("type")]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|code| match code.trim().to_ascii_lowercase().as_str() {
            "insufficient_quota"
            | "billing_hard_limit_reached"
            | "invalid_api_key"
            | "authentication_error"
            | "permission_error"
            | "permission_denied"
            | "not_found_error"
            | "model_not_found" => Some(ProviderFailureKind::Permanent),
            "invalid_request_error" | "invalid_request" => {
                Some(ProviderFailureKind::InvalidRequest)
            }
            "context_length_exceeded" | "context_window_exceeded" | "prompt_too_long" => {
                Some(ProviderFailureKind::ContextWindow)
            }
            "rate_limit_exceeded"
            | "rate_limit_error"
            | "server_error"
            | "internal_error"
            | "internal_server_error"
            | "overloaded_error"
            | "timeout_error"
            | "request_timeout"
            | "service_unavailable" => Some(ProviderFailureKind::Transient),
            _ => None,
        })
}

#[derive(Debug)]
pub(crate) struct LlmHttpError {
    pub(crate) status: u16,
    pub(crate) kind: Option<ProviderFailureKind>,
    detail: String,
}

impl LlmHttpError {
    pub(crate) fn new(status: u16, body: &str) -> Self {
        let payload = serde_json::from_str::<Value>(body).unwrap_or(Value::Null);
        let kind = provider_failure_kind(payload.get("error").unwrap_or(&payload));
        Self {
            status,
            kind,
            detail: truncate_text(body, 2048),
        }
    }

    pub(crate) fn retryable(&self) -> bool {
        if matches!(
            self.kind,
            Some(
                ProviderFailureKind::Permanent
                    | ProviderFailureKind::InvalidRequest
                    | ProviderFailureKind::ContextWindow
            )
        ) {
            return false;
        }
        matches!(self.status, 408 | 409 | 425 | 429 | 500..=599)
    }
}

impl std::fmt::Display for LlmHttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "LLM request failed: {} {}",
            self.status, self.detail
        )
    }
}

impl std::error::Error for LlmHttpError {}

#[derive(Debug)]
pub(crate) struct LlmPayloadError {
    pub(crate) kind: Option<ProviderFailureKind>,
    detail: String,
}

impl LlmPayloadError {
    pub(crate) fn from_payload(payload: &Value) -> Option<Self> {
        let response = payload.get("response").unwrap_or(payload);
        let payload_type = payload.get("type").and_then(Value::as_str).unwrap_or("");
        let error = response
            .get("error")
            .filter(|error| !error.is_null())
            .or_else(|| payload.get("error").filter(|error| !error.is_null()));
        if error.is_none()
            && !matches!(payload_type, "error" | "response.failed")
            && response.get("status").and_then(Value::as_str) != Some("failed")
        {
            return None;
        }
        let error = error.unwrap_or(payload);
        Some(Self {
            kind: provider_failure_kind(error),
            detail: truncate_text(&error.to_string(), 2048),
        })
    }
}

impl std::fmt::Display for LlmPayloadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "LLM provider payload failed: {}", self.detail)
    }
}

impl std::error::Error for LlmPayloadError {}
