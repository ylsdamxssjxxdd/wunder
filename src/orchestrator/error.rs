use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorCategory {
    Input,
    Contention,
    Cancellation,
    Provider,
    Context,
    Quota,
    Internal,
}

impl ErrorCategory {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Contention => "contention",
            Self::Cancellation => "cancellation",
            Self::Provider => "provider",
            Self::Context => "context",
            Self::Quota => "quota",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorSeverity {
    Info,
    Warning,
    Error,
}

impl ErrorSeverity {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorSourceStage {
    Request,
    SessionLock,
    Runtime,
    Llm,
}

impl ErrorSourceStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::SessionLock => "session_lock",
            Self::Runtime => "runtime",
            Self::Llm => "llm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryAction {
    FixRequest,
    RetryLater,
    RetryNextTurn,
    CompactContext,
    AwaitQuota,
    RebuildRuntime,
}

impl RecoveryAction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::FixRequest => "fix_request",
            Self::RetryLater => "retry_later",
            Self::RetryNextTurn => "retry_next_turn",
            Self::CompactContext => "compact_context",
            Self::AwaitQuota => "await_quota",
            Self::RebuildRuntime => "rebuild_runtime",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OrchestratorErrorMeta {
    category: ErrorCategory,
    severity: ErrorSeverity,
    retryable: bool,
    retry_after_ms: Option<u64>,
    source_stage: ErrorSourceStage,
    recovery_action: RecoveryAction,
}

impl OrchestratorErrorMeta {
    const fn new(
        category: ErrorCategory,
        severity: ErrorSeverity,
        retryable: bool,
        retry_after_ms: Option<u64>,
        source_stage: ErrorSourceStage,
        recovery_action: RecoveryAction,
    ) -> Self {
        Self {
            category,
            severity,
            retryable,
            retry_after_ms,
            source_stage,
            recovery_action,
        }
    }

    fn to_value(self) -> Value {
        json!({
            "category": self.category.as_str(),
            "severity": self.severity.as_str(),
            "retryable": self.retryable,
            "retry_after_ms": self.retry_after_ms,
            "source_stage": self.source_stage.as_str(),
            "recovery_action": self.recovery_action.as_str(),
        })
    }
}

#[derive(Debug)]
pub(crate) struct OrchestratorError {
    code: &'static str,
    message: String,
    detail: Option<Value>,
    meta: OrchestratorErrorMeta,
}

impl OrchestratorError {
    pub(super) fn new(code: &'static str, message: String, detail: Option<Value>) -> Self {
        let meta = default_meta_for_code(code);
        Self {
            code,
            message,
            detail,
            meta,
        }
    }

    pub(super) fn invalid_request(message: String) -> Self {
        Self::new("INVALID_REQUEST", message, None)
    }

    pub(super) fn invalid_request_with_detail(message: String, detail: Value) -> Self {
        Self::new("INVALID_REQUEST", message, Some(detail))
    }

    pub(super) fn user_busy(message: String) -> Self {
        Self::new("USER_BUSY", message, None)
    }

    pub(super) fn cancelled(message: String) -> Self {
        Self::new("CANCELLED", message, None)
    }

    pub(super) fn llm_unavailable(message: String) -> Self {
        Self::new("LLM_UNAVAILABLE", message, None)
    }

    pub(super) fn context_window_exceeded(message: String) -> Self {
        Self::new("CONTEXT_WINDOW_EXCEEDED", message, None)
    }

    pub(super) fn internal(message: String) -> Self {
        Self::new("INTERNAL_ERROR", message, None)
    }

    pub(super) fn user_token_insufficient(status: UserTokenBalanceStatus) -> Self {
        let message = i18n::t("error.user_token_insufficient");
        Self::new(
            "USER_TOKEN_INSUFFICIENT",
            message,
            Some(json!({
                "token_balance": status.balance,
                "token_granted_total": status.granted_total,
                "token_used_total": status.used_total,
                "daily_token_grant": status.daily_grant,
                "last_token_grant_date": status.last_grant_date,
            })),
        )
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn retryable(&self) -> bool {
        self.meta.retryable
    }

    pub(crate) fn retry_after_ms(&self) -> Option<u64> {
        self.meta.retry_after_ms
    }

    pub(crate) fn recovery_action(&self) -> &'static str {
        self.meta.recovery_action.as_str()
    }

    pub(crate) fn to_payload(&self) -> Value {
        let mut payload = json!({
            "code": self.code,
            "message": self.message,
            "error_meta": self.meta.to_value(),
        });
        if let Some(detail) = &self.detail {
            if let Value::Object(ref mut map) = payload {
                map.insert("detail".to_string(), detail.clone());
            }
        }
        payload
    }
}

fn default_meta_for_code(code: &str) -> OrchestratorErrorMeta {
    match code.trim().to_ascii_uppercase().as_str() {
        "INVALID_REQUEST" => OrchestratorErrorMeta::new(
            ErrorCategory::Input,
            ErrorSeverity::Warning,
            false,
            None,
            ErrorSourceStage::Request,
            RecoveryAction::FixRequest,
        ),
        "USER_BUSY" => OrchestratorErrorMeta::new(
            ErrorCategory::Contention,
            ErrorSeverity::Warning,
            true,
            Some((SESSION_LOCK_BUSY_RETRY_S * 1000.0) as u64),
            ErrorSourceStage::SessionLock,
            RecoveryAction::RetryLater,
        ),
        "CANCELLED" => OrchestratorErrorMeta::new(
            ErrorCategory::Cancellation,
            ErrorSeverity::Info,
            true,
            None,
            ErrorSourceStage::Runtime,
            RecoveryAction::RetryNextTurn,
        ),
        "LLM_UNAVAILABLE" => OrchestratorErrorMeta::new(
            ErrorCategory::Provider,
            ErrorSeverity::Error,
            true,
            None,
            ErrorSourceStage::Llm,
            RecoveryAction::RetryLater,
        ),
        "CONTEXT_WINDOW_EXCEEDED" => OrchestratorErrorMeta::new(
            ErrorCategory::Context,
            ErrorSeverity::Warning,
            true,
            None,
            ErrorSourceStage::Llm,
            RecoveryAction::CompactContext,
        ),
        "USER_QUOTA_EXCEEDED" | "USER_TOKEN_INSUFFICIENT" => OrchestratorErrorMeta::new(
            ErrorCategory::Quota,
            ErrorSeverity::Warning,
            true,
            None,
            ErrorSourceStage::Runtime,
            RecoveryAction::AwaitQuota,
        ),
        _ => OrchestratorErrorMeta::new(
            ErrorCategory::Internal,
            ErrorSeverity::Error,
            false,
            None,
            ErrorSourceStage::Runtime,
            RecoveryAction::RebuildRuntime,
        ),
    }
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OrchestratorError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_payload_contains_runtime_error_meta() {
        let err = OrchestratorError::llm_unavailable("provider unavailable".to_string());
        let payload = err.to_payload();
        assert_eq!(payload["code"], json!("LLM_UNAVAILABLE"));
        assert_eq!(payload["error_meta"]["category"], json!("provider"));
        assert_eq!(payload["error_meta"]["severity"], json!("error"));
        assert_eq!(payload["error_meta"]["retryable"], json!(true));
        assert_eq!(
            payload["error_meta"]["recovery_action"],
            json!("retry_later")
        );
    }

    #[test]
    fn exception_context_window_exceeded_requests_compaction() {
        let err = OrchestratorError::context_window_exceeded("context exceeded".to_string());
        assert!(err.retryable());
        assert_eq!(err.recovery_action(), "compact_context");
        assert_eq!(err.to_payload()["error_meta"]["source_stage"], json!("llm"));
    }
}
