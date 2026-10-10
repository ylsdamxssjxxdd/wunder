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

// ---------------------------------------------------------------------------
// Interlink tunnel section (docs §3.3)
// ---------------------------------------------------------------------------

/// Default full-shadow period (seconds).
pub const INTERLINK_SHADOW_INTERVAL_DEFAULT_S: u64 = 300;
/// Shadow period floor: below this the node is polling itself, not reacting.
pub const INTERLINK_SHADOW_INTERVAL_MIN_S: u64 = 30;
pub const INTERLINK_SHADOW_INTERVAL_MAX_S: u64 = 3_600;
/// Default cap of one remote file pull, in MiB (docs §6.4).
pub const INTERLINK_MAX_FILE_PULL_MB_DEFAULT: u64 = 20;
pub const INTERLINK_MAX_FILE_PULL_MB_MAX: u64 = 1_024;
/// Approval policy values (docs §7.3 2). Only these three are honoured.
pub const APPROVAL_DEFAULT_PROMPT: &str = "prompt";
pub const APPROVAL_DEFAULT_ALLOW_READONLY: &str = "allow_readonly";
pub const APPROVAL_DEFAULT_DENY_ALL: &str = "deny_all";

/// Local interlink tunnel settings, persisted next to the session token in the
/// same 0600 file. `node_secret` is the only tunnel credential that lives on
/// disk; it is never logged and never leaves this file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterlinkSessionConfig {
    /// User kill switch: `false` means the tunnel is never opened.
    #[serde(default = "default_interlink_enabled")]
    pub enabled: bool,
    /// Protocol version the node offers (docs §4.1 negotiation).
    #[serde(default = "default_interlink_protocol")]
    pub protocol: i64,
    #[serde(default = "default_interlink_shadow_interval_s")]
    pub shadow_interval_s: u64,
    /// `prompt | allow_readonly | deny_all` (docs §7.3); never applies to L2/L3.
    #[serde(default = "default_interlink_approval_default")]
    pub approval_default: String,
    #[serde(default = "default_interlink_max_file_pull_mb")]
    pub max_file_pull_mb: u64,
    #[serde(default)]
    pub node_secret: Option<String>,
    #[serde(default)]
    pub secret_version: i64,
}

impl Default for InterlinkSessionConfig {
    fn default() -> Self {
        Self {
            enabled: default_interlink_enabled(),
            protocol: default_interlink_protocol(),
            shadow_interval_s: default_interlink_shadow_interval_s(),
            approval_default: default_interlink_approval_default(),
            max_file_pull_mb: default_interlink_max_file_pull_mb(),
            node_secret: None,
            secret_version: 0,
        }
    }
}

impl InterlinkSessionConfig {
    /// Shadow period clamped into a sane band; a hand-edited file cannot make
    /// the node poll every millisecond or never at all.
    pub fn shadow_interval_s(&self) -> u64 {
        self.shadow_interval_s
            .clamp(INTERLINK_SHADOW_INTERVAL_MIN_S, INTERLINK_SHADOW_INTERVAL_MAX_S)
    }

    /// Remote pull ceiling in bytes, clamped to the documented band.
    pub fn max_file_pull_bytes(&self) -> u64 {
        self.max_file_pull_mb
            .clamp(1, INTERLINK_MAX_FILE_PULL_MB_MAX)
            .saturating_mul(1024 * 1024)
    }

    /// Normalized approval policy; unknown values fall back to `prompt`, the
    /// only fail-closed default (docs §14: default must be safe).
    pub fn approval_policy(&self) -> &'static str {
        match self.approval_default.trim().to_ascii_lowercase().as_str() {
            APPROVAL_DEFAULT_ALLOW_READONLY => APPROVAL_DEFAULT_ALLOW_READONLY,
            APPROVAL_DEFAULT_DENY_ALL => APPROVAL_DEFAULT_DENY_ALL,
            _ => APPROVAL_DEFAULT_PROMPT,
        }
    }

    fn normalized(&self) -> Self {
        Self {
            enabled: self.enabled,
            protocol: self.protocol.max(1),
            shadow_interval_s: self.shadow_interval_s(),
            approval_default: self.approval_policy().to_string(),
            max_file_pull_mb: self.max_file_pull_mb.clamp(1, INTERLINK_MAX_FILE_PULL_MB_MAX),
            node_secret: self
                .node_secret
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            secret_version: self.secret_version.max(0),
        }
    }
}

fn default_interlink_enabled() -> bool {
    true
}

fn default_interlink_protocol() -> i64 {
    1
}

fn default_interlink_shadow_interval_s() -> u64 {
    INTERLINK_SHADOW_INTERVAL_DEFAULT_S
}

fn default_interlink_approval_default() -> String {
    APPROVAL_DEFAULT_PROMPT.to_string()
}

fn default_interlink_max_file_pull_mb() -> u64 {
    INTERLINK_MAX_FILE_PULL_MB_DEFAULT
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
    /// Interlink tunnel settings (docs §3.3). Absent in files written before
    /// the tunnel existed, where it falls back to the documented defaults.
    #[serde(default)]
    pub interlink: InterlinkSessionConfig,
}

fn default_preferences_sync_enabled() -> bool {
    true
}

impl CloudSessionFile {
    /// Copy with the interlink band clamped, so a hand-edited file cannot
    /// smuggle out-of-range periods or caps into the tunnel.
    fn normalized_copy(&self) -> Self {
        let mut copy = self.clone();
        copy.interlink = copy.interlink.normalized();
        copy
    }

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
        let text = serde_json::to_string_pretty(&self.normalized_copy())
            .context("serialize cloud session failed")?;
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
            interlink: InterlinkSessionConfig::default(),
        };
        let text = serde_json::to_string(&session).expect("serialize");
        let parsed: CloudSessionFile = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(parsed.log_report.level, "warn");
        assert!(parsed.log_report.enabled);
        assert!(parsed.preferences_sync_enabled);
        assert_eq!(parsed.refresh_token.as_deref(), Some("wund_refresh_test"));
        assert_eq!(parsed.token_expires_at, Some(3600.0));
        assert!(parsed.interlink.enabled);
        assert_eq!(parsed.interlink.protocol, 1);
        assert_eq!(parsed.interlink.shadow_interval_s, 300);
        assert_eq!(parsed.interlink.approval_default, "prompt");
        assert_eq!(parsed.interlink.max_file_pull_mb, 20);
        assert!(parsed.interlink.node_secret.is_none());
        assert_eq!(parsed.interlink.secret_version, 0);
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
        // A pre-tunnel file keeps the documented defaults, tunnel on.
        assert!(parsed.interlink.enabled);
        assert_eq!(parsed.interlink.shadow_interval_s, 300);
        assert_eq!(parsed.interlink.approval_default, "prompt");
    }

    fn session_with_interlink(interlink: InterlinkSessionConfig) -> CloudSessionFile {
        CloudSessionFile {
            server: "http://127.0.0.1:8000".to_string(),
            user_id: "u-test".to_string(),
            username: "u-test".to_string(),
            scope: "local_desktop".to_string(),
            token: "wund_test".to_string(),
            refresh_token: None,
            token_expires_at: None,
            device_id: "dev-test".to_string(),
            device_name: "pc-test".to_string(),
            client: "desktop".to_string(),
            logged_in_at: 1.0,
            account: None,
            log_report: CloudLogReportState::default(),
            preferences_sync_enabled: true,
            interlink,
        }
    }

    /// The interlink section survives `save`/`load_any`, including the node
    /// secret, and out-of-range values are clamped on write.
    #[test]
    fn interlink_section_round_trips_through_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let session = session_with_interlink(InterlinkSessionConfig {
            enabled: true,
            protocol: 1,
            shadow_interval_s: 5,
            approval_default: "allow_readonly".to_string(),
            max_file_pull_mb: 99_999,
            node_secret: Some("a".repeat(64)),
            secret_version: 3,
        });
        session.save(dir.path()).expect("save");
        let loaded = CloudSessionFile::load_any(dir.path()).expect("load");
        assert_eq!(loaded.interlink.shadow_interval_s, INTERLINK_SHADOW_INTERVAL_MIN_S);
        assert_eq!(
            loaded.interlink.max_file_pull_mb,
            INTERLINK_MAX_FILE_PULL_MB_MAX
        );
        assert_eq!(loaded.interlink.approval_policy(), "allow_readonly");
        assert_eq!(loaded.interlink.node_secret.as_deref(), Some(&"a".repeat(64)[..]));
        assert_eq!(loaded.interlink.secret_version, 3);
        assert_eq!(loaded.interlink.max_file_pull_bytes(), 1024 * 1024 * 1024);
    }

    #[test]
    fn kill_switch_and_unknown_approval_policy_are_fail_closed() {
        let off = session_with_interlink(InterlinkSessionConfig {
            enabled: false,
            ..Default::default()
        });
        assert!(!off.interlink.enabled);

        let mut config = InterlinkSessionConfig::default();
        config.approval_default = "allow_everything".to_string();
        assert_eq!(config.approval_policy(), "prompt");
        config.approval_default = " DENY_ALL ".to_string();
        assert_eq!(config.approval_policy(), "deny_all");
        // Blank secret material is dropped instead of being stored as "".
        config.node_secret = Some("   ".to_string());
        let dir = tempfile::tempdir().expect("temp dir");
        let session = session_with_interlink(config);
        session.save(dir.path()).expect("save");
        let loaded = CloudSessionFile::load_any(dir.path()).expect("load");
        assert!(loaded.interlink.node_secret.is_none());
    }
}
