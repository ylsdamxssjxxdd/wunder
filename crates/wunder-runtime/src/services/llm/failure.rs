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
        // 550/502 之类网关错误常常不给响应体；保留状态码并显式说明，
        // 避免出现 "LLM request failed: 550 " 这种尾随空格的诊断。
        let detail = self.detail.trim();
        if detail.is_empty() {
            write!(
                formatter,
                "LLM request failed: HTTP {} (empty response body)",
                self.status
            )
        } else {
            write!(
                formatter,
                "LLM request failed: HTTP {} {}",
                self.status, detail
            )
        }
    }
}

impl std::error::Error for LlmHttpError {}

/// Flattens a reqwest transport failure into one diagnostic sentence.
///
/// `reqwest::Error`'s own Display often stops at "error sending request for
/// url", while the actionable cause (connection reset, TLS, body decode) lives
/// in its source chain. The orchestrator only renders the top-level anyhow
/// message, so the chain must be folded in here.
pub(crate) fn summarize_llm_transport_error(error: &reqwest::Error) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(source) = current {
        let text = source.to_string();
        let text = text.trim();
        if !text.is_empty() && !parts.iter().any(|part| part == text) {
            parts.push(text.to_string());
        }
        current = source.source();
    }
    if parts.is_empty() {
        "unknown transport error".to_string()
    } else {
        parts.join(": ")
    }
}

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
