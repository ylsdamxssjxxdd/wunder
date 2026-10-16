use super::bridge::{BridgeResponse, BrowserBridge};
use super::config::{effective_browser_config, EffectiveBrowserConfig};
use super::model::BrowserSessionScope;
use crate::config::Config;
use anyhow::{anyhow, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use url::{Host, Url};
use uuid::Uuid;

const TEMP_DIR_ROOT_ENV: &str = "WUNDER_TEMP_DIR_ROOT";
/// Grace on top of the per-call budget so the Rust deadline always fires after
/// Playwright's own timeout and can report the bridge's error instead.
const CALL_GRACE: Duration = Duration::from_secs(15);
/// Hard ceiling for a single bridge command, however the budget is derived.
const MAX_CALL_DEADLINE: Duration = Duration::from_secs(900);
/// A batch runs its steps serially inside one bridge command.
const MAX_BATCH_STEPS: u64 = 10;

/// Preserve partial progress when an interaction batch fails. Callers must not
/// blindly retry successful steps (for example, an already submitted form).
#[derive(Debug)]
pub struct BrowserCommandFailure {
    message: String,
    pub data: Value,
}

impl std::fmt::Display for BrowserCommandFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BrowserCommandFailure {}

pub struct BrowserControlService {
    config: EffectiveBrowserConfig,
    bridge: BrowserBridge,
}

impl BrowserControlService {
    pub fn new(config: EffectiveBrowserConfig) -> Self {
        Self {
            bridge: BrowserBridge::new(config.clone()),
            config,
        }
    }

    pub fn health(&self) -> Value {
        json!({
            "ok": true,
            "enabled": self.config.enabled,
            "deployment": self.config.deployment.clone(),
            "sessions": self.bridge.session_count(),
        })
    }

    pub fn status(&self) -> Value {
        self.status_for(None)
    }

    /// Model-facing status: one shared bridge process holds every tenant's
    /// sessions, so only the caller's own keys are listed.
    pub fn status_for(&self, owner: Option<&str>) -> Value {
        let sessions = match owner {
            Some(owner) if !owner.trim().is_empty() => {
                self.bridge.session_keys_for_owner(owner.trim())
            }
            _ => self.bridge.session_keys(),
        };
        json!({
            "ok": true,
            "enabled": self.config.enabled,
            "tool_visible": self.config.tool_visible,
            "deployment": self.config.deployment.clone(),
            "default_profile": self.config.default_profile.clone(),
            "profiles": self.config.profiles(),
            "limits": {
                "max_sessions": self.config.max_sessions,
                "max_tabs_per_session": self.config.max_tabs_per_session,
                "max_snapshot_chars": self.config.max_snapshot_chars,
                "max_download_bytes": self.config.max_download_bytes,
                "idle_timeout_secs": self.config.idle_timeout_secs,
            },
            "playwright": {
                "headless": self.config.headless,
                "viewport_width": self.config.viewport_width,
                "viewport_height": self.config.viewport_height,
                "timeout_secs": self.config.timeout_secs,
                "browsers_path": self.config.browsers_path.clone(),
                "launch_mode": self.config.launch_mode.clone(),
                "channel": self.config.channel.clone(),
                "executable_path": self.config.executable_path.clone(),
                "attach_endpoint_configured": self.config.attach_endpoint.is_some(),
            },
            "docker": {
                "enabled": self.config.docker_enabled,
                "use_no_sandbox": self.config.docker_use_no_sandbox,
                "disable_dev_shm_usage": self.config.docker_disable_dev_shm_usage,
                "downloads_root": self
                    .config
                    .docker_downloads_root
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
            },
            "control": {
                "host": self.config.control_host.clone(),
                "port": self.config.control_port,
                "public_base_url": self.config.control_public_base_url.clone(),
                "auth_token_configured": self.config.control_auth_token.is_some(),
            },
            "sessions": sessions,
        })
    }

    pub async fn profiles(&self) -> Result<Value> {
        self.ensure_available()?;
        Ok(json!({
            "ok": true,
            "default_profile": self.config.default_profile.clone(),
            "profiles": self.config.profiles(),
        }))
    }

    pub async fn execute(
        &self,
        scope: &BrowserSessionScope,
        action: &str,
        args: &Value,
    ) -> Result<Value> {
        let normalized = normalize_action(action);
        match normalized.as_str() {
            "status" => Ok(self.status_for(Some(&scope.user_id))),
            "profiles" => self.profiles().await,
            "start" => self.start(scope).await,
            "stop" => self.stop(scope).await,
            "tabs" => self.tabs(scope).await,
            "open" => self.open(scope, args).await,
            "focus" => self.focus(scope, args).await,
            "close" => self.close(scope, args).await,
            "navigate" => self.navigate(scope, args).await,
            "snapshot" => self.snapshot(scope, args).await,
            "act" => self.act(scope, args).await,
            "screenshot" => self.screenshot(scope, args).await,
            "read_page" => self.read_page(scope, args).await,
            "click" | "type" | "press" | "hover" | "wait" => {
                self.shortcut_act(scope, &normalized, args).await
            }
            _ => Err(anyhow!("Unknown browser action: {action}")),
        }
    }

    /// Every bridge command carries a wall-clock budget. Playwright's own
    /// timeouts do not bound a wedged page (a synchronous JS loop ignores them),
    /// and an unbounded read would hold a session slot and a worker forever.
    async fn command(
        &self,
        session_key: &str,
        action: &str,
        args: &Value,
        payload: Value,
    ) -> Result<BridgeResponse> {
        let deadline = call_deadline(self.config.timeout_secs, action, args);
        self.bridge
            .send_command(session_key, payload, deadline)
            .await
            .map_err(anyhow::Error::msg)
    }

    async fn start(&self, scope: &BrowserSessionScope) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let response = self
            .command(
                &session_key,
                "start",
                &json!({}),
                json!({
                    "action": "start",
                    "profile": scope.profile_name().unwrap_or_else(|| self.config.default_profile.clone()),
                }),
            )
            .await?;
        self.extract_data(response, "Browser session start failed")
    }

    async fn stop(&self, scope: &BrowserSessionScope) -> Result<Value> {
        if !self.config.enabled {
            return Ok(json!({ "ok": true, "closed": true }));
        }
        let session_key = scope.session_key()?;
        self.bridge.close_session(&session_key).await;
        Ok(json!({
            "ok": true,
            "closed": true,
            "browser_session_id": session_key,
        }))
    }

    async fn tabs(&self, scope: &BrowserSessionScope) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let response = self
            .command(
                &session_key,
                "tabs",
                &json!({}),
                json!({
                    "action": "tabs",
                    "profile": scope.profile_name().unwrap_or_else(|| self.config.default_profile.clone()),
                }),
            )
            .await?;
        self.extract_data(response, "Browser tabs query failed")
    }

    async fn open(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let url = optional_string(args, "url")
            .map(|value| self.normalize_url(value))
            .transpose()?;
        let timeout_ms = optional_timeout_ms(args, self.config.timeout_secs);
        let response = self
            .command(
                &session_key,
                "open",
                args,
                json!({
                    "action": "open",
                    "profile": scope.profile_name().unwrap_or_else(|| self.config.default_profile.clone()),
                    "url": url,
                    "timeout_ms": timeout_ms,
                }),
            )
            .await?;
        self.extract_data(response, "Open browser tab failed")
    }

    async fn focus(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let target_id = required_string(args, "target_id")?;
        let response = self
            .command(
                &session_key,
                "focus",
                args,
                json!({
                    "action": "focus",
                    "target_id": target_id,
                }),
            )
            .await?;
        self.extract_data(response, "Focus browser tab failed")
    }

    async fn close(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        if args.get("target_id").is_none() && args.get("request").is_none() {
            return self.stop(scope).await;
        }
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let target_id = optional_string(args, "target_id").map(ToString::to_string);
        let response = self
            .command(
                &session_key,
                "close",
                args,
                json!({
                    "action": "close",
                    "target_id": target_id,
                }),
            )
            .await?;
        self.extract_data(response, "Close browser tab failed")
    }

    async fn navigate(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let url = self.normalize_url(required_string(args, "url")?)?;
        let timeout_ms = optional_timeout_ms(args, self.config.timeout_secs);
        let response = self
            .command(
                &session_key,
                "navigate",
                args,
                json!({
                    "action": "navigate",
                    "target_id": optional_string(args, "target_id"),
                    "url": url,
                    "timeout_ms": timeout_ms,
                    "include_snapshot": optional_bool(args, "include_snapshot").unwrap_or(false),
                    "max_chars": self.snapshot_budget(args),
                }),
            )
            .await?;
        self.extract_data(response, "Navigate browser failed")
    }

    async fn snapshot(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let response = self
            .command(
                &session_key,
                "snapshot",
                args,
                json!({
                    "action": "snapshot",
                    "target_id": optional_string(args, "target_id"),
                    "format": optional_string(args, "format").unwrap_or("role"),
                    "interactive": optional_bool(args, "interactive").unwrap_or(true),
                    "compact": optional_bool(args, "compact").unwrap_or(true),
                    "max_chars": self.snapshot_budget(args),
                }),
            )
            .await?;
        self.extract_data(response, "Browser snapshot failed")
    }

    async fn act(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let request = args
            .get("request")
            .cloned()
            .unwrap_or_else(|| build_direct_request(args));
        if request.is_null() {
            return Err(anyhow!("Missing browser act request"));
        }
        let response = self
            .command(
                &session_key,
                "act",
                args,
                json!({
                    "action": "act",
                    "request": request,
                    "target_id": optional_string(args, "target_id"),
                    "include_snapshot": optional_bool(args, "include_snapshot").unwrap_or(false),
                    "max_chars": self.snapshot_budget(args),
                }),
            )
            .await?;
        self.extract_data(response, "Browser act failed")
    }

    async fn shortcut_act(
        &self,
        scope: &BrowserSessionScope,
        kind: &str,
        args: &Value,
    ) -> Result<Value> {
        let mut request = build_direct_request(args);
        if let Value::Object(ref mut map) = request {
            map.insert("kind".to_string(), json!(kind));
            if kind == "type" && !map.contains_key("text") {
                map.insert("text".to_string(), json!(required_string(args, "text")?));
            }
            if kind == "click" && !map.contains_key("selector") && !map.contains_key("ref") {
                map.insert(
                    "selector".to_string(),
                    json!(required_string(args, "selector")?),
                );
            }
        }
        self.act(
            scope,
            &json!({
                "request": request,
                "target_id": optional_string(args, "target_id"),
            }),
        )
        .await
    }

    async fn screenshot(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let response = self
            .command(
                &session_key,
                "screenshot",
                args,
                json!({
                    "action": "screenshot",
                    "target_id": optional_string(args, "target_id"),
                    "full_page": optional_bool(args, "full_page").unwrap_or(false),
                }),
            )
            .await?;
        let mut data = self.extract_data(response, "Browser screenshot failed")?;
        let base64_payload = data
            .get("image_base64")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Browser screenshot returned empty payload"))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(base64_payload)
            .map_err(|err| anyhow!("Browser screenshot base64 decode failed: {err}"))?;
        if bytes.len() > self.config.max_download_bytes {
            return Err(anyhow!(
                "Browser screenshot exceeds download limit ({} bytes)",
                self.config.max_download_bytes
            ));
        }
        if optional_bool(args, "save_to_workspace").unwrap_or(false) {
            return Ok(data);
        }
        let (filename, download_url) = save_screenshot(&bytes)?;
        if let Value::Object(ref mut map) = data {
            map.remove("image_base64");
            map.insert("filename".to_string(), json!(filename));
            map.insert("download_url".to_string(), json!(download_url));
        }
        Ok(data)
    }

    async fn read_page(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let response = self
            .command(
                &session_key,
                "read_page",
                args,
                json!({
                    "action": "read_page",
                    "target_id": optional_string(args, "target_id"),
                    "max_chars": self.snapshot_budget(args),
                }),
            )
            .await?;
        self.extract_data(response, "Browser page read failed")
    }

    /// Text payloads never exceed the configured character budget, whatever the
    /// caller asked for.
    fn snapshot_budget(&self, args: &Value) -> u64 {
        optional_u64(args, "max_chars")
            .unwrap_or(self.config.max_snapshot_chars as u64)
            .max(1)
            .min(self.config.max_snapshot_chars as u64)
    }

    fn ensure_available(&self) -> Result<()> {
        if self.config.enabled {
            return Ok(());
        }
        Err(anyhow!(
            "Browser runtime is disabled. Enable 'browser.enabled' or use desktop legacy mode."
        ))
    }

    fn normalize_url(&self, input: &str) -> Result<String> {
        let parsed = Url::parse(input).map_err(|err| anyhow!("Invalid URL: {err}"))?;
        if parsed.scheme().eq_ignore_ascii_case("file") && self.config.deny_file_scheme {
            return Err(anyhow!("file:// URLs are disabled for browser navigation"));
        }
        match parsed.scheme() {
            "http" | "https" => {}
            "file" => {}
            _ => {
                return Err(anyhow!(
                    "Only http/https URLs are supported for browser navigation"
                ));
            }
        }
        if let Some(host) = parsed.host() {
            let host_text = host_to_string(host);
            let host_lower = host_text.to_ascii_lowercase();
            let allowed = self
                .config
                .hostname_allowlist
                .iter()
                .any(|item| item == &host_lower);
            if !allowed && !self.config.allow_private_network && is_private_host(&host_lower) {
                return Err(anyhow!(
                    "Private-network browser navigation is disabled for host '{host_text}'"
                ));
            }
        }
        Ok(parsed.to_string())
    }

    fn extract_data(&self, response: BridgeResponse, fallback_error: &str) -> Result<Value> {
        if response.success {
            Ok(response.data.unwrap_or_else(|| json!({ "ok": true })))
        } else {
            let message = format!(
                "{}: {}",
                fallback_error,
                response
                    .error
                    .unwrap_or_else(|| "Unknown browser bridge error".to_string())
            );
            match response.data {
                Some(data) => Err(BrowserCommandFailure { message, data }.into()),
                None => Err(anyhow!(message)),
            }
        }
    }
}

pub fn browser_service(config: &Config) -> &'static BrowserControlService {
    static INSTANCE: OnceLock<BrowserControlService> = OnceLock::new();
    INSTANCE.get_or_init(|| BrowserControlService::new(effective_browser_config(config)))
}

/// Wall-clock budget for one bridge command.
///
/// The bridge's own timeout only bounds Playwright operations that accept one;
/// `evaluate` and a page with a wedged synchronous loop do not, so this is the
/// only ceiling on how long a single call can hold a session slot.
fn call_deadline(timeout_secs: u64, action: &str, args: &Value) -> Duration {
    let per_step = Duration::from_secs(timeout_secs.max(1).saturating_mul(4));
    let steps = if action == "act" && act_kind(args).as_deref() == Some("batch") {
        args.get("request")
            .and_then(|request| request.get("steps"))
            .and_then(Value::as_array)
            .map(|steps| (steps.len() as u64).clamp(1, MAX_BATCH_STEPS))
            .unwrap_or(1)
    } else {
        1
    };
    per_step
        .saturating_mul(steps as u32)
        .saturating_add(CALL_GRACE)
        .min(MAX_CALL_DEADLINE)
}

fn act_kind(args: &Value) -> Option<String> {
    args.get("request")
        .and_then(|request| request.get("kind"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn normalize_action(action: &str) -> String {
    let normalized = action.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "list_tabs" => "tabs".to_string(),
        "open_tab" => "open".to_string(),
        "focus_tab" => "focus".to_string(),
        "close_tab" => "close".to_string(),
        "read" | "page" | "readpage" => "read_page".to_string(),
        other => other.to_string(),
    }
}

fn build_direct_request(args: &Value) -> Value {
    let mut map = serde_json::Map::new();
    for key in [
        "kind",
        "target_id",
        "selector",
        "text",
        "key",
        "url",
        "load_state",
        "wait_ms",
        "timeout_ms",
        "timeout_secs",
        "full_page",
        "expression",
        "script",
        "to_ref",
        "to_selector",
    ] {
        if let Some(value) = args.get(key) {
            map.insert(key.to_string(), value.clone());
        }
    }
    if let Some(value) = args.get("ref") {
        map.insert("ref".to_string(), value.clone());
    }
    Value::Object(map)
}

fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("Missing browser parameter '{key}'"))
}

fn optional_string<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn optional_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

fn optional_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(|value| {
        if let Some(number) = value.as_u64() {
            Some(number)
        } else if let Some(number) = value.as_i64() {
            u64::try_from(number).ok()
        } else {
            None
        }
    })
}

fn optional_timeout_ms(args: &Value, default_timeout_secs: u64) -> Option<u64> {
    let value = optional_u64(args, "timeout_ms").or_else(|| {
        optional_u64(args, "timeout_secs").map(|seconds| seconds.saturating_mul(1000))
    })?;
    let default_ms = default_timeout_secs.max(1).saturating_mul(1000);
    Some(value.clamp(1, default_ms.saturating_mul(4)))
}

fn save_screenshot(bytes: &[u8]) -> Result<(String, String)> {
    let dir = resolve_temp_dir()?;
    std::fs::create_dir_all(&dir).map_err(|err| anyhow!("Create temp dir failed: {err}"))?;
    let filename = format!("browser_shot_{}.png", Uuid::new_v4().simple());
    let path = dir.join(&filename);
    std::fs::write(&path, bytes)
        .map_err(|err| anyhow!("Write browser screenshot failed: {err}"))?;
    Ok((
        filename.clone(),
        format!("/wunder/temp_dir/download?filename={filename}"),
    ))
}

fn resolve_temp_dir() -> Result<PathBuf> {
    if let Ok(value) = std::env::var(TEMP_DIR_ROOT_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.is_absolute() {
                return Ok(candidate);
            }
            let current = std::env::current_dir().map_err(|err| anyhow!(err))?;
            return Ok(current.join(candidate));
        }
    }
    let current = std::env::current_dir().map_err(|err| anyhow!(err))?;
    Ok(current.join("config").join("data").join("temp_dir"))
}

fn host_to_string(host: Host<&str>) -> String {
    match host {
        Host::Domain(value) => value.to_string(),
        Host::Ipv4(value) => value.to_string(),
        Host::Ipv6(value) => value.to_string(),
    }
}

fn is_private_host(host: &str) -> bool {
    if matches!(
        host,
        "localhost" | "host.docker.internal" | "gateway.docker.internal"
    ) || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(addr) => {
                addr.is_private()
                    || addr.is_loopback()
                    || addr.is_link_local()
                    || addr.is_broadcast()
                    || addr.is_unspecified()
                    || addr.is_multicast()
            }
            IpAddr::V6(addr) => {
                // An IPv4-mapped literal (::ffff:127.0.0.1) must be judged by the
                // IPv4 rules it aliases, otherwise it routes around this check.
                if let Some(mapped) = addr.to_ipv4_mapped() {
                    return is_private_host(&mapped.to_string());
                }
                addr.is_loopback()
                    || addr.is_unspecified()
                    || addr.is_unique_local()
                    || addr.is_unicast_link_local()
                    || addr.is_multicast()
            }
        };
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{
        browser_service, call_deadline, is_private_host, optional_timeout_ms,
        BrowserControlService, CALL_GRACE, MAX_CALL_DEADLINE,
    };
    use crate::config::Config;
    use crate::services::browser::config::effective_browser_config;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn detects_private_hosts() {
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("localhost"));
        assert!(is_private_host("169.254.169.254"));
        assert!(is_private_host("10.0.0.8"));
        assert!(is_private_host("::1"));
        assert!(is_private_host("fe80::1"));
        assert!(is_private_host("fc00::1"));
        // IPv4-mapped literals alias an IPv4 private address.
        assert!(is_private_host("::ffff:127.0.0.1"));
        assert!(is_private_host("::ffff:10.0.0.8"));
        assert!(!is_private_host("example.com"));
        assert!(!is_private_host("::ffff:8.8.8.8"));
    }

    #[test]
    fn command_deadline_is_bounded_and_batch_aware() {
        // A plain command gets 4x the configured timeout plus reporting grace.
        assert_eq!(
            call_deadline(30, "navigate", &json!({})),
            Duration::from_secs(120).saturating_add(CALL_GRACE)
        );
        // A wedged evaluate can never hold a slot for more than the ceiling.
        assert_eq!(
            call_deadline(u64::MAX, "act", &json!({"request": {"kind": "evaluate"}})),
            MAX_CALL_DEADLINE
        );
        let batch = json!({"request": {"kind": "batch", "steps": [1, 2, 3]}});
        assert_eq!(
            call_deadline(10, "act", &batch),
            Duration::from_secs(120).saturating_add(CALL_GRACE)
        );
        // Step count is clamped, so nesting cannot stretch the ceiling.
        let many = json!({"request": {"kind": "batch", "steps": vec![json!(1); 500]}});
        assert_eq!(
            call_deadline(10, "act", &many),
            Duration::from_secs(400).saturating_add(CALL_GRACE)
        );
        // Garbage timeouts still produce a usable window.
        assert!(call_deadline(0, "snapshot", &json!({})) >= CALL_GRACE);
    }

    #[test]
    fn browser_timeout_accepts_ms_and_secs_with_bound() {
        assert_eq!(
            optional_timeout_ms(&json!({ "timeout_ms": 60_000 }), 30),
            Some(60_000)
        );
        assert_eq!(
            optional_timeout_ms(&json!({ "timeout_secs": 45 }), 30),
            Some(45_000)
        );
        assert_eq!(
            optional_timeout_ms(&json!({ "timeout_ms": 999_000 }), 30),
            Some(120_000)
        );
        assert_eq!(optional_timeout_ms(&json!({}), 30), None);
    }

    #[test]
    fn status_and_budget_stay_scoped_and_bounded() {
        // Built directly: `browser_service` is a process-wide singleton, so a
        // shared instance would freeze whichever config another test loaded first.
        let mut config = Config::default();
        config.browser.enabled = true;
        config.browser.limits.max_snapshot_chars = 2_000;
        let service = BrowserControlService::new(effective_browser_config(&config));
        let scoped = service.status_for(Some("unit-a"));
        assert_eq!(
            scoped.get("ok").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        // No tenant keys leak into a scoped status payload.
        let sessions = scoped["sessions"].as_array().expect("sessions array");
        assert!(sessions
            .iter()
            .all(|key| key.as_str().unwrap_or_default().starts_with("unit-a:")));
        // A requested budget never exceeds the configured ceiling.
        assert_eq!(
            service.snapshot_budget(&json!({ "max_chars": 900_000 })),
            2_000
        );
        assert_eq!(service.snapshot_budget(&json!({})), 2_000);
        assert_eq!(service.snapshot_budget(&json!({"max_chars":300})), 300);
        assert_eq!(service.snapshot_budget(&json!({"max_chars":1})), 1);
        assert_eq!(service.snapshot_budget(&json!({"max_chars":0})), 1);
    }

    #[test]
    fn batch_failure_preserves_completed_progress() {
        let service = BrowserControlService::new(effective_browser_config(&Config::default()));
        let error = service
            .extract_data(
                super::BridgeResponse {
                    success: false,
                    error: Some("Step failed".to_string()),
                    traceback: None,
                    data: Some(json!({"completed_steps":2,"failed_step":3})),
                },
                "Browser batch failed",
            )
            .unwrap_err();
        let failure = error
            .downcast_ref::<super::BrowserCommandFailure>()
            .expect("structured failure");
        assert_eq!(failure.data["completed_steps"], 2);
        assert_eq!(failure.data["failed_step"], 3);
    }

    #[test]
    fn browser_service_singleton_initializes() {
        let mut config = Config::default();
        config.browser.enabled = true;
        let service: &BrowserControlService = browser_service(&config);
        assert_eq!(
            service
                .status()
                .get("ok")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }
}
