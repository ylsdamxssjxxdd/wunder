//! Cloud account façade: login/logout/status projections for native UIs.
//!
//! Thin translations only — session persistence, token handling and cloud
//! model synthesis all live in the shared engine `CloudService`. A password
//! crosses this boundary once as a call argument and is never logged,
//! persisted or projected back.

use super::NativeDesktop;
use anyhow::{anyhow, bail, Result};
use wunder_server::cloud::{shared, CloudStatus};

/// Secret-free, UI-ready projection of the engine `CloudStatus`. Numbers stay
/// raw so the frontend owns formatting and the low-balance threshold. The
/// connection machine state passes through verbatim; timestamps arrive as
/// short Chinese relative strings, matching the façade's error copy style.
#[derive(Clone, Debug, Default)]
pub struct CloudStatusView {
    pub logged_in: bool,
    pub expired: bool,
    pub server: String,
    pub username: String,
    pub balance: i64,
    pub daily_grant: i64,
    pub max_concurrent_calls: u64,
    pub concurrency_active: u64,
    pub preferences_sync_enabled: bool,
    /// `online | reconnecting | expired | logged_out`, from the engine state machine.
    pub connection: String,
    /// Sanitized last failure summary; empty when the engine recorded none.
    pub last_error: String,
    /// "刚刚 / x 分钟前 / x 小时前"; empty when never successful.
    pub last_success_at: String,
    /// "x 秒后 / x 分钟后"; empty when no retry is scheduled.
    pub next_retry_at: String,
}

/// Short Chinese relative time for a past moment, clamped against clock skew.
fn relative_past(at: f64) -> String {
    let delta = (super::now_ts() - at).max(0.0);
    if delta < 60.0 {
        "刚刚".to_string()
    } else if delta < 3600.0 {
        format!("{} 分钟前", (delta / 60.0) as i64)
    } else {
        format!("{} 小时前", (delta / 3600.0) as i64)
    }
}

/// Short Chinese relative time for a scheduled moment in the future.
fn relative_future(at: f64) -> String {
    let delta = (at - super::now_ts()).max(0.0);
    if delta < 60.0 {
        format!("{} 秒后", delta as i64)
    } else if delta < 3600.0 {
        format!("{} 分钟后", (delta / 60.0) as i64)
    } else {
        format!("{} 小时后", (delta / 3600.0) as i64)
    }
}

impl CloudStatusView {
    fn project(status: CloudStatus) -> Self {
        Self {
            logged_in: status.logged_in,
            expired: status.expired,
            server: status.server.unwrap_or_default(),
            username: status.username.unwrap_or_default(),
            balance: status.quota.as_ref().map(|q| q.balance).unwrap_or(0),
            daily_grant: status.quota.as_ref().map(|q| q.daily_grant).unwrap_or(0),
            max_concurrent_calls: status.max_concurrent_calls.unwrap_or(0),
            concurrency_active: status.concurrency_active.unwrap_or(0),
            preferences_sync_enabled: status.preferences_sync_enabled,
            connection: status.connection.to_string(),
            last_error: status.last_error.unwrap_or_default(),
            last_success_at: status
                .last_success_at
                .map(relative_past)
                .unwrap_or_default(),
            next_retry_at: status
                .next_retry_at
                .map(relative_future)
                .unwrap_or_default(),
        }
    }

    /// The quota badge turns red at or below 10% of the daily grant.
    /// `balance * 10 <= daily_grant` avoids the integer division.
    pub fn low_balance(&self) -> bool {
        self.logged_in && self.balance * 10 <= self.daily_grant
    }
}

/// Project transport-shaped service errors into one-line user-facing causes.
/// `login` distinguishes the wrong-credential 401 from an expired session.
fn friendly_error(error: anyhow::Error, login: bool) -> anyhow::Error {
    let text = error.to_string();
    if text.contains("unreachable") {
        anyhow!("云端不可达，请检查服务地址与网络")
    } else if text.contains("401") {
        if login {
            anyhow!("用户名或密码错误")
        } else {
            anyhow!("登录已过期，请重新登录")
        }
    } else if text.contains("429") {
        anyhow!("请求过于频繁，请稍后再试")
    } else {
        error
    }
}

impl NativeDesktop {
    /// Login against `server` with a local-scope session. The engine persists
    /// the session, registers the device, pulls the account and synthesizes
    /// `cloud/<id>` models into the local config before returning.
    pub fn cloud_login(
        &self,
        server: &str,
        username: &str,
        password: &str,
    ) -> Result<CloudStatusView> {
        let server = server.trim();
        if server.is_empty() {
            bail!("请填写服务地址");
        }
        let lowered = server.to_ascii_lowercase();
        if !(lowered.starts_with("http://") || lowered.starts_with("https://")) {
            bail!("服务地址必须以 http:// 或 https:// 开头");
        }
        if username.trim().is_empty() || password.is_empty() {
            bail!("请填写用户名和密码");
        }
        let state = self.state().clone();
        let status = self
            .runtime
            .block_on(shared().login(
                &state.config_store,
                server,
                username.trim(),
                password,
                "desktop",
            ))
            .map_err(|error| friendly_error(error, true))?;
        Ok(CloudStatusView::project(status))
    }

    /// Logout: the engine invalidates the local-scope token, drops the session
    /// file and removes every synthesized cloud model entry.
    pub fn cloud_logout(&self) -> Result<CloudStatusView> {
        let state = self.state().clone();
        self.runtime
            .block_on(shared().logout(&state.config_store))
            .map_err(|error| friendly_error(error, false))?;
        Ok(CloudStatusView::project(shared().status()))
    }

    /// Current login/account snapshot for UI binding.
    pub fn cloud_status(&self) -> Result<CloudStatusView> {
        Ok(CloudStatusView::project(shared().status()))
    }

    /// Pull the account overview from the server. A 401 marks the session
    /// expired so the UI asks for a re-login.
    pub fn cloud_refresh_account(&self) -> Result<CloudStatusView> {
        self.runtime
            .block_on(shared().refresh_account())
            .map_err(|error| {
                if error.to_string().contains("401") {
                    shared().mark_expired();
                }
                friendly_error(error, false)
            })?;
        Ok(CloudStatusView::project(shared().status()))
    }

    /// Toggle basic-preference sharing (theme / avatar / send key).
    pub fn cloud_set_sync_preferences(&self, enabled: bool) -> Result<CloudStatusView> {
        self.runtime
            .block_on(shared().set_preferences_sync(enabled))
            .map_err(|error| friendly_error(error, false))?;
        Ok(CloudStatusView::project(shared().status()))
    }
}

#[cfg(test)]
mod tests {
    use super::CloudStatusView;

    /// The badge warns at or below 10% of the daily grant, evaluated without
    /// integer division so a 1-digit balance never slips through.
    #[test]
    fn low_balance_tracks_the_daily_grant_ratio() {
        let mut view = CloudStatusView {
            logged_in: true,
            balance: 101,
            daily_grant: 1000,
            ..Default::default()
        };
        assert!(!view.low_balance());
        view.balance = 100;
        assert!(view.low_balance());
        view.balance = 99;
        assert!(view.low_balance());
        // Logged out never warns, whatever the numbers say.
        view.logged_in = false;
        assert!(!view.low_balance());
    }
}
