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
use url::{Host, Url};
use uuid::Uuid;

const TEMP_DIR_ROOT_ENV: &str = "WUNDER_TEMP_DIR_ROOT";

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
            "sessions": self.bridge.session_keys(),
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
            "status" => Ok(self.status()),
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
            "click" => self.shortcut_act(scope, "click", args).await,
            "type" => self.shortcut_act(scope, "type", args).await,
            "press" => self.shortcut_act(scope, "press", args).await,
            "hover" => self.shortcut_act(scope, "hover", args).await,
            "wait" => self.shortcut_act(scope, "wait", args).await,
            _ => Err(anyhow!("Unknown browser action: {action}")),
        }
    }

    async fn start(&self, scope: &BrowserSessionScope) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let response = self
            .bridge
            .send_command(
                &session_key,
                json!({
                    "action": "start",
                    "profile": scope.profile_name().unwrap_or_else(|| self.config.default_profile.clone()),
                }),
            )
            .await
            .map_err(|err| anyhow!(err))?;
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
            .bridge
            .send_command(
                &session_key,
                json!({
                    "action": "tabs",
                    "profile": scope.profile_name().unwrap_or_else(|| self.config.default_profile.clone()),
                }),
            )
            .await
            .map_err(|err| anyhow!(err))?;
        self.extract_data(response, "Browser tabs query failed")
    }

    async fn open(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let url = optional_string(args, "url")
            .map(|value| self.normalize_url(value))
            .transpose()?;
        let response = self
            .bridge
            .send_command(
                &session_key,
                json!({
                    "action": "open",
                    "profile": scope.profile_name().unwrap_or_else(|| self.config.default_profile.clone()),
                    "url": url,
                }),
            )
            .await
            .map_err(|err| anyhow!(err))?;
        self.extract_data(response, "Open browser tab failed")
    }

    async fn focus(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let target_id = required_string(args, "target_id")?;
        let response = self
            .bridge
            .send_command(
                &session_key,
                json!({
                    "action": "focus",
                    "target_id": target_id,
                }),
            )
            .await
            .map_err(|err| anyhow!(err))?;
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
            .bridge
            .send_command(
                &session_key,
                json!({
                    "action": "close",
                    "target_id": target_id,
                }),
            )
            .await
            .map_err(|err| anyhow!(err))?;
        self.extract_data(response, "Close browser tab failed")
    }

    async fn navigate(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let url = self.normalize_url(required_string(args, "url")?)?;
        let response = self
            .bridge
            .send_command(
                &session_key,
                json!({
                    "action": "navigate",
                    "target_id": optional_string(args, "target_id"),
                    "url": url,
                }),
            )
            .await
            .map_err(|err| anyhow!(err))?;
        self.extract_data(response, "Navigate browser failed")
    }

    async fn snapshot(&self, scope: &BrowserSessionScope, args: &Value) -> Result<Value> {
        self.ensure_available()?;
        let session_key = scope.session_key()?;
        let response = self
            .bridge
            .send_command(
                &session_key,
                json!({
                    "action": "snapshot",
                    "target_id": optional_string(args, "target_id"),
                    "format": optional_string(args, "format").unwrap_or("role"),
                    "interactive": optional_bool(args, "interactive").unwrap_or(true),
                    "compact": optional_bool(args, "compact").unwrap_or(true),
                    "max_chars": optional_u64(args, "max_chars")
                        .map(|value| value.min(self.config.max_snapshot_chars as u64))
                        .unwrap_or(self.config.max_snapshot_chars as u64),
                }),
            )
            .await
            .map_err(|err| anyhow!(err))?;
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
            .bridge
            .send_command(
                &session_key,
                json!({
                    "action": "act",
                    "request": request,
                    "target_id": optional_string(args, "target_id"),
                }),
            )
            .await
            .map_err(|err| anyhow!(err))?;
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
            .bridge
            .send_command(
                &session_key,
                json!({
                    "action": "screenshot",
                    "target_id": optional_string(args, "target_id"),
                    "full_page": optional_bool(args, "full_page").unwrap_or(false),
                }),
            )
            .await
            .map_err(|err| anyhow!(err))?;
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
            .bridge
            .send_command(
                &session_key,
                json!({
                    "action": "read_page",
                    "target_id": optional_string(args, "target_id"),
                    "max_chars": optional_u64(args, "max_chars")
                        .map(|value| value.min(self.config.max_snapshot_chars as u64))
                        .unwrap_or(self.config.max_snapshot_chars as u64),
                }),
            )
            .await
            .map_err(|err| anyhow!(err))?;
        self.extract_data(response, "Browser page read failed")
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
            Err(anyhow!(
                "{}: {}",
                fallback_error,
                response
                    .error
                    .unwrap_or_else(|| "Unknown browser bridge error".to_string())
            ))
        }
    }
}

pub fn browser_service(config: &Config) -> &'static BrowserControlService {
    static INSTANCE: OnceLock<BrowserControlService> = OnceLock::new();
    INSTANCE.get_or_init(|| BrowserControlService::new(effective_browser_config(config)))
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
    Ok(current.join("temp_dir"))
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
            }
            IpAddr::V6(addr) => {
                addr.is_loopback() || addr.is_unspecified() || addr.is_unique_local()
            }
        };
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{browser_service, is_private_host, BrowserControlService};
    use crate::config::Config;

    #[test]
    fn detects_private_hosts() {
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("localhost"));
        assert!(!is_private_host("example.com"));
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
