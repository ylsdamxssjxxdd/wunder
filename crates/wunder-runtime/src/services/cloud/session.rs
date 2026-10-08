//! Local cloud session persistence (`cloud.session.json`).
//!
//! The file only exists while logged in; a missing or unparsable file means
//! logged out. Schema follows the cloud connection plan §3.1. The file holds
//! the session token, so it is written with 0600 on unix (Windows keeps the
//! default user-profile ACL; the file lives under the user home which is not
//! readable by other accounts by default).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SESSION_FILE_DESKTOP: &str = "config/cloud.session.json";
pub const SESSION_FILE_CLI: &str = "cloud.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloudQuotaSnapshot {
    #[serde(default)]
    pub balance: i64,
    #[serde(default)]
    pub granted_total: i64,
    #[serde(default)]
    pub used_total: i64,
    #[serde(default)]
    pub daily_grant: i64,
    #[serde(default)]
    pub last_grant_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudAccountSnapshot {
    #[serde(default)]
    pub quota: CloudQuotaSnapshot,
    #[serde(default)]
    pub max_concurrent_calls: u64,
    #[serde(default)]
    pub concurrency_active: Option<u64>,
    #[serde(default)]
    pub concurrency_queued: Option<u64>,
    #[serde(default)]
    pub preferences_synced_at: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudLogReportState {
    #[serde(default = "default_log_report_enabled")]
    pub enabled: bool,
    #[serde(default = "default_log_report_level")]
    pub level: String,
    #[serde(default)]
    pub last_synced_seq: i64,
}

impl Default for CloudLogReportState {
    fn default() -> Self {
        Self {
            enabled: default_log_report_enabled(),
            level: default_log_report_level(),
            last_synced_seq: 0,
        }
    }
}

fn default_log_report_enabled() -> bool {
    true
}

fn default_log_report_level() -> String {
    "warn".to_string()
}

/// The on-disk session record. Distinct from the server-side
/// `CloudDeviceRecord` storage row; this one only lives locally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSessionFile {
    pub server: String,
    pub user_id: String,
    pub username: String,
    pub scope: String,
    pub token: String,
    /// Rotating refresh token for local sessions. `None` on legacy session
    /// files written before token rotation existed; those can only recover
    /// through a re-login.
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Access-token expiry (unix seconds) as reported by the server at
    /// login/refresh time; drives the proactive renewal check.
    #[serde(default)]
    pub token_expires_at: Option<f64>,
    pub device_id: String,
    pub device_name: String,
    pub client: String,
    pub logged_in_at: f64,
    #[serde(default)]
    pub account: Option<CloudAccountSnapshot>,
    #[serde(default)]
    pub log_report: CloudLogReportState,
    #[serde(default = "default_preferences_sync_enabled")]
    pub preferences_sync_enabled: bool,
}

fn default_preferences_sync_enabled() -> bool {
    true
}

impl CloudSessionFile {
    /// Session file path for a client form. Desktop and CLI share the same
    /// wunder home but use different file names/locations per §3.1.
    pub fn path_for(base_dir: &Path, client: &str) -> PathBuf {
        if client.eq_ignore_ascii_case("cli") {
            base_dir.join(SESSION_FILE_CLI)
        } else {
            base_dir.join(SESSION_FILE_DESKTOP)
        }
    }

    /// Load whichever session file exists (desktop or cli layout). Returns
    /// `Ok(None)` when neither exists; a corrupt file is treated as logged
    /// out and removed.
    pub fn load_any(base_dir: &Path) -> Option<Self> {
        for client in ["desktop", "cli"] {
            let path = Self::path_for(base_dir, client);
            match std::fs::read_to_string(&path) {
                Ok(text) => match serde_json::from_str::<Self>(&text) {
                    Ok(session) => return Some(session),
                    Err(err) => {
                        tracing::warn!("cloud session file is corrupt, ignoring: {err}");
                        let _ = std::fs::remove_file(&path);
                        return None;
                    }
                },
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => continue,
            }
        }
        None
    }

    pub fn save(&self, base_dir: &Path) -> Result<PathBuf> {
        let path = Self::path_for(base_dir, &self.client);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("create cloud session dir failed: {}", parent.display())
            })?;
        }
        let text = serde_json::to_string_pretty(self).context("serialize cloud session failed")?;
        wunder_core::atomic_write::atomic_write_text(&path, &text)
            .with_context(|| format!("write cloud session failed: {}", path.display()))?;
        restrict_permissions(&path);
        Ok(path)
    }

    pub fn remove(base_dir: &Path, client: &str) {
        let _ = std::fs::remove_file(Self::path_for(base_dir, client));
    }
}

fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        // Windows: keep the default user-profile ACL. The file only lives in
        // the per-user home directory; explicit ACL hardening would need the
        // windows ACL API and is intentionally not introduced here.
        let _ = path;
    }
}

/// Resolve the wunder home directory the same way desktop/cli launchers do:
/// `WUNDER_HOME` when set, otherwise `<user home>/.wunder`.
pub fn wunder_home_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("WUNDER_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return path;
    }
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| {
            let drive = std::env::var_os("HOMEDRIVE")?;
            let path = std::env::var_os("HOMEPATH")?;
            Some(PathBuf::from(drive).join(path))
        });
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(PathBuf::from);
    home.unwrap_or_else(|| std::env::temp_dir()).join(".wunder")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_path_uses_client_specific_layout() {
        let base = Path::new("/tmp/wunder-home");
        assert_eq!(
            CloudSessionFile::path_for(base, "desktop"),
            base.join("config/cloud.session.json")
        );
        assert_eq!(
            CloudSessionFile::path_for(base, "cli"),
            base.join("cloud.json")
        );
    }

    #[test]
    fn session_roundtrip_keeps_defaults() {
        let session = CloudSessionFile {
            server: "http://127.0.0.1:8000".to_string(),
            user_id: "alice".to_string(),
            username: "alice".to_string(),
            scope: "local_desktop".to_string(),
            token: "wund_test".to_string(),
            refresh_token: Some("wund_refresh_test".to_string()),
            token_expires_at: Some(3600.0),
            device_id: "device".to_string(),
            device_name: "pc".to_string(),
            client: "desktop".to_string(),
            logged_in_at: 1.0,
            account: None,
            log_report: CloudLogReportState::default(),
            preferences_sync_enabled: true,
        };
        let text = serde_json::to_string(&session).expect("serialize");
        let parsed: CloudSessionFile = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(parsed.log_report.level, "warn");
        assert!(parsed.log_report.enabled);
        assert!(parsed.preferences_sync_enabled);
        assert_eq!(parsed.refresh_token.as_deref(), Some("wund_refresh_test"));
        assert_eq!(parsed.token_expires_at, Some(3600.0));
    }

    /// Session files written before token rotation must still load; the new
    /// fields fall back to `None`.
    #[test]
    fn legacy_session_without_refresh_fields_loads() {
        let legacy = serde_json::json!({
            "server": "http://127.0.0.1:8000",
            "user_id": "alice",
            "username": "alice",
            "scope": "local_desktop",
            "token": "wund_test",
            "device_id": "device",
            "device_name": "pc",
            "client": "desktop",
            "logged_in_at": 1.0,
        });
        let parsed: CloudSessionFile =
            serde_json::from_value(legacy).expect("legacy session must parse");
        assert!(parsed.refresh_token.is_none());
        assert!(parsed.token_expires_at.is_none());
    }
}
