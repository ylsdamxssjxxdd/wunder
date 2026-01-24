use super::*;

#[derive(Debug)]
pub(crate) struct OrchestratorError {
    code: &'static str,
    message: String,
    detail: Option<Value>,
}

impl OrchestratorError {
    pub(super) fn new(code: &'static str, message: String, detail: Option<Value>) -> Self {
        Self {
            code,
            message,
            detail,
        }
    }

    pub(super) fn invalid_request(message: String) -> Self {
        Self::new("INVALID_REQUEST", message, None)
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

    pub(super) fn internal(message: String) -> Self {
        Self::new("INTERNAL_ERROR", message, None)
    }

    pub(super) fn user_quota_exceeded(status: UserQuotaStatus) -> Self {
        let message = i18n::t("error.user_quota_exceeded");
        Self::new(
            "USER_QUOTA_EXCEEDED",
            message,
            Some(json!({
                "daily_quota": status.daily_quota,
                "used": status.used,
                "remaining": status.remaining,
                "date": status.date,
            })),
        )
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn to_payload(&self) -> Value {
        let mut payload = json!({
            "code": self.code,
            "message": self.message,
        });
        if let Some(detail) = &self.detail {
            if let Value::Object(ref mut map) = payload {
                map.insert("detail".to_string(), detail.clone());
            }
        }
        payload
    }
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for OrchestratorError {}
