//! Cloud channel support for local engines (desktop/cli): session persistence,
//! login/logout lifecycle, cloud model synthesis, device log reporting and the
//! call-time queue handling shared with the LLM client. See the local cloud
//! connection plan §3.1/§4.x.

pub mod reporter;
pub mod service;
pub mod session;

pub(crate) use service::network_error_summary;
pub use service::{
    desired_cloud_models, remove_cloud_models, shared, CloudService, CloudStatus,
    CLOUD_MODEL_PREFIX, CLOUD_PROVIDER, CLOUD_QUEUE_JITTER_MS, CLOUD_QUEUE_MAX_WAIT_SECS,
    CONNECTION_EXPIRED, CONNECTION_LOGGED_OUT, CONNECTION_ONLINE, CONNECTION_RECONNECTING,
    TOKEN_RENEW_AHEAD_SECS, TOKEN_RENEW_CHECK_INTERVAL_SECS,
};
pub use session::{CloudAccountSnapshot, CloudQuotaSnapshot, CloudSessionFile};

use std::fmt;

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
