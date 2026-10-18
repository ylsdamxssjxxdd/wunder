//! Cloud channel support for local engines (desktop/cli): session persistence,
//! login/logout lifecycle, cloud model synthesis, device log reporting and the
//! call-time queue handling shared with the LLM client. See the local cloud
//! connection plan §3.1/§4.x.

pub mod reporter;
pub mod service;
pub mod session;
mod thread_watch;
pub use thread_watch::{CloudThreadCancellation, CloudThreadEvent, CloudThreadWatch};

pub(crate) use service::network_error_summary;
pub use service::{
    desired_cloud_models, remove_cloud_models, shared, CloudService, CloudStatus,
    CLOUD_MODEL_PREFIX, CLOUD_PROVIDER, CLOUD_QUEUE_JITTER_MS, CLOUD_QUEUE_MAX_WAIT_SECS,
    CONNECTION_EXPIRED, CONNECTION_LOGGED_OUT, CONNECTION_ONLINE, CONNECTION_RECONNECTING,
    TOKEN_RENEW_AHEAD_SECS, TOKEN_RENEW_CHECK_INTERVAL_SECS,
};
pub use session::{CloudAccountSnapshot, CloudQuotaSnapshot, CloudSessionFile};

use std::fmt;

#[derive(Debug)]
pub struct CloudInterlinkError {
    pub status: u16,
    pub operation: &'static str,
    pub message: String,
}

impl fmt::Display for CloudInterlinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cloud interlink {} failed: {} {}", self.operation, self.status, self.message)
    }
}
impl std::error::Error for CloudInterlinkError {}

#[derive(Debug)]
pub struct CloudTransportError(pub String);
impl fmt::Display for CloudTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cloud request failed: {}", self.0)
    }
}
impl std::error::Error for CloudTransportError {}

pub fn retryable_cloud_read(error: &anyhow::Error) -> bool {
    if error.is::<CloudTransportError>() { return true; }
    if let Some(error) = error.downcast_ref::<CloudInterlinkError>() {
        return matches!(error.status, 408 | 429 | 500..=599);
    }
    error.downcast_ref::<reqwest::Error>().is_some_and(|error| {
        error.is_timeout() || error.is_connect() || error.is_body() || error.is_request()
    })
}

#[cfg(test)]
mod read_retry_tests {
    use super::*;

    #[test]
    fn retries_transient_reads_and_stops_for_permissions_or_missing_threads() {
        for status in [408, 429, 500, 502, 503, 504] {
            let error = anyhow::Error::new(CloudInterlinkError { status, operation: "fixture", message: String::new() });
            assert!(retryable_cloud_read(&error));
        }
        for status in [400, 401, 403, 404, 409, 422] {
            let error = anyhow::Error::new(CloudInterlinkError { status, operation: "fixture", message: String::new() });
            assert!(!retryable_cloud_read(&error));
        }
        assert!(!retryable_cloud_read(&anyhow::anyhow!("invalid fixture")));
        assert!(retryable_cloud_read(&CloudTransportError("connection unavailable".into()).into()));
    }
}

/// Structured error returned when the cloud channel answers 401: the local
/// session must be refreshed by a re-login. Downcastable from `anyhow::Error`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CloudSessionExpired;

impl fmt::Display for CloudSessionExpired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cloud session expired, please log in again")
    }
}

impl std::error::Error for CloudSessionExpired {}

/// Structured error for `429 USER_QUOTA_INSUFFICIENT`; carries the server's
/// account quota snapshot for UI display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudQuotaInsufficient {
    pub balance: i64,
    pub granted_total: i64,
    pub used_total: i64,
    pub daily_grant: i64,
}

impl fmt::Display for CloudQuotaInsufficient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cloud quota insufficient: balance {} (granted {}, used {}, daily grant {})",
            self.balance, self.granted_total, self.used_total, self.daily_grant
        )
    }
}

impl std::error::Error for CloudQuotaInsufficient {}
