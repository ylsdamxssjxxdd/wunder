use super::ToolContext;
use crate::config::{BrowserToolConfig, Config};
use crate::core::python_runtime;
use anyhow::{anyhow, Result};
use base64::Engine;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use url::Url;
use uuid::Uuid;

const BRIDGE_SCRIPT: &str = include_str!("browser_bridge.py");
const TEMP_DIR_ROOT_ENV: &str = "WUNDER_TEMP_DIR_ROOT";

pub const TOOL_BROWSER: &str = "浏览器";
pub const TOOL_BROWSER_NAVIGATE: &str = "浏览器导航";
pub const TOOL_BROWSER_CLICK: &str = "浏览器点击";
pub const TOOL_BROWSER_TYPE: &str = "浏览器输入";
pub const TOOL_BROWSER_SCREENSHOT: &str = "浏览器截图";
pub const TOOL_BROWSER_READ_PAGE: &str = "浏览器读页";
pub const TOOL_BROWSER_CLOSE: &str = "浏览器关闭";

#[derive(Debug, Copy, Clone)]
enum BrowserAction {
    Navigate,
    Click,
    Type,
    Screenshot,
    ReadPage,
    Close,
}

impl BrowserAction {
    fn from_str(raw: &str) -> Option<Self> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "navigate" | "nav" | "open" => Some(Self::Navigate),
            "click" => Some(Self::Click),
            "type" | "input" | "fill" => Some(Self::Type),
            "screenshot" | "shot" | "capture" => Some(Self::Screenshot),
            "read_page" | "readpage" | "read" | "page" => Some(Self::ReadPage),
            "close" | "quit" | "exit" => Some(Self::Close),
            _ => None,
        }
    }
}

fn action_from_tool_name(name: &str) -> Option<BrowserAction> {
    match name.trim() {
        TOOL_BROWSER_NAVIGATE | "browser_navigate" => Some(BrowserAction::Navigate),
        TOOL_BROWSER_CLICK | "browser_click" => Some(BrowserAction::Click),
        TOOL_BROWSER_TYPE | "browser_type" => Some(BrowserAction::Type),
        TOOL_BROWSER_SCREENSHOT | "browser_screenshot" => Some(BrowserAction::Screenshot),
        TOOL_BROWSER_READ_PAGE | "browser_read_page" => Some(BrowserAction::ReadPage),
        TOOL_BROWSER_CLOSE | "browser_close" => Some(BrowserAction::Close),
        _ => None,
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "action")]
pub enum BrowserCommand {
    Navigate { url: String },
    Click { selector: String },
    Type { selector: String, text: String },
    Screenshot,
    ReadPage,
    Close,
}

#[derive(Debug, Deserialize)]
pub struct BrowserResponse {
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
}

struct BrowserSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    last_active: Instant,
}

impl BrowserSession {
    fn send(&mut self, cmd: &BrowserCommand) -> Result<BrowserResponse, String> {
        let json = serde_json::to_string(cmd).map_err(|err| format!("Serialize error: {err}"))?;
        self.stdin
            .write_all(json.as_bytes())
            .map_err(|err| format!("Failed to write to browser stdin: {err}"))?;
        self.stdin
            .write_all(b"\n")
            .map_err(|err| format!("Failed to write newline: {err}"))?;
        self.stdin
            .flush()
            .map_err(|err| format!("Failed to flush browser stdin: {err}"))?;

        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .map_err(|err| format!("Failed to read browser stdout: {err}"))?;

        if line.trim().is_empty() {
            return Err("Browser bridge closed unexpectedly".to_string());
        }

        self.last_active = Instant::now();
        serde_json::from_str(line.trim())
            .map_err(|err| format!("Failed to parse browser response: {err}"))
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        self.kill();
    }
}

pub struct BrowserManager {
    sessions: DashMap<String, Mutex<BrowserSession>>,
    config: BrowserToolConfig,
    bridge_path: OnceLock<PathBuf>,
}

impl BrowserManager {
    pub fn new(mut config: BrowserToolConfig) -> Self {
        config.viewport_width = config.viewport_width.max(1);
        config.viewport_height = config.viewport_height.max(1);
        config.timeout_secs = config.timeout_secs.max(1);
        config.max_sessions = config.max_sessions.max(1);
        Self {
            sessions: DashMap::new(),
            config,
            bridge_path: OnceLock::new(),
        }
    }

    fn ensure_bridge_script(&self) -> Result<&PathBuf, String> {
        if let Some(path) = self.bridge_path.get() {
            return Ok(path);
        }
        let dir = std::env::temp_dir().join("wunder_browser");
        std::fs::create_dir_all(&dir).map_err(|err| format!("Failed to create temp dir: {err}"))?;
        let path = dir.join("browser_bridge.py");
        std::fs::write(&path, BRIDGE_SCRIPT)
            .map_err(|err| format!("Failed to write bridge script: {err}"))?;
        debug!(path = %path.display(), "Wrote browser bridge script");
        let _ = self.bridge_path.set(path);
        Ok(self.bridge_path.get().expect("bridge script path missing"))
    }

    fn resolve_python_invocation(&self) -> (String, Option<python_runtime::PythonRuntime>) {
        if let Some(path) = self
            .config
            .python_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "auto")
        {
            return (path.to_string(), None);
        }
        if let Some(runtime) = python_runtime::resolve_python_runtime() {
            return (runtime.bin.to_string_lossy().to_string(), Some(runtime));
        }
        let fallback = if cfg!(windows) { "python" } else { "python3" };
        (fallback.to_string(), None)
    }

    fn apply_python_env(cmd: &mut std::process::Command, runtime: &python_runtime::PythonRuntime) {
        if !runtime.embedded {
            return;
        }
        cmd.env(
            "WUNDER_PYTHON_BIN",
            runtime.bin.to_string_lossy().to_string(),
        );
        if let Some(home) = &runtime.home {
            cmd.env("PYTHONHOME", home.to_string_lossy().to_string());
        }
        if let Some(site_packages) = &runtime.site_packages {
            cmd.env("PYTHONPATH", site_packages.to_string_lossy().to_string());
        }
        if let Some(cert) = &runtime.ssl_cert {
            cmd.env("SSL_CERT_FILE", cert.to_string_lossy().to_string());
        }
        if let Some(home) = &runtime.home {
            let rc = home.join("etc/matplotlibrc");
            if rc.is_file() {
                cmd.env("MATPLOTLIBRC", rc.to_string_lossy().to_string());
            }
            let cartopy_dir = home.join("share/cartopy");
            if cartopy_dir.is_dir() {
                cmd.env(
                    "CARTOPY_DATA_DIR",
                    cartopy_dir.to_string_lossy().to_string(),
                );
            }
        }
        cmd.env("PYTHONNOUSERSITE", "1");
        cmd.env("PIP_NO_INDEX", "1");
        if let Some(bin_dir) = runtime.bin.parent() {
            prepend_path_env(cmd, "PATH", bin_dir);
        }
        if let Some(lib_dir) = &runtime.lib_dir {
            prepend_path_env(cmd, "LD_LIBRARY_PATH", lib_dir);
        }
    }

    fn apply_browser_env(
        cmd: &mut std::process::Command,
        runtime: Option<&python_runtime::PythonRuntime>,
    ) {
        cmd.env_clear();
        if let Ok(value) = std::env::var("SYSTEMROOT") {
            cmd.env("SYSTEMROOT", value);
        }
        if let Ok(value) = std::env::var("PATH") {
            cmd.env("PATH", value);
        }
        if let Ok(value) = std::env::var("TEMP") {
            cmd.env("TEMP", value);
        }
        if let Ok(value) = std::env::var("TMP") {
            cmd.env("TMP", value);
        }
        if let Ok(value) = std::env::var("USERPROFILE") {
            cmd.env("USERPROFILE", value);
        }
        if let Ok(value) = std::env::var("APPDATA") {
            cmd.env("APPDATA", value);
        }
        if let Ok(value) = std::env::var("LOCALAPPDATA") {
            cmd.env("LOCALAPPDATA", value);
        }
        if let Ok(value) = std::env::var("HOME") {
            cmd.env("HOME", value);
        }
        if let Ok(value) = std::env::var("TMPDIR") {
            cmd.env("TMPDIR", value);
        }
        if let Ok(value) = std::env::var("XDG_CACHE_HOME") {
            cmd.env("XDG_CACHE_HOME", value);
        }
        if let Ok(value) = std::env::var("PLAYWRIGHT_BROWSERS_PATH") {
            cmd.env("PLAYWRIGHT_BROWSERS_PATH", value);
        }
        if let Ok(value) = std::env::var("PLAYWRIGHT_DOWNLOAD_HOST") {
            cmd.env("PLAYWRIGHT_DOWNLOAD_HOST", value);
        }
        if let Ok(value) = std::env::var("PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD") {
            cmd.env("PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD", value);
        }
        cmd.env("PYTHONIOENCODING", "utf-8");
        if let Some(runtime) = runtime {
            Self::apply_python_env(cmd, runtime);
        }
    }

    fn get_or_create_sync(&self, session_key: &str) -> Result<(), String> {
        if self.sessions.contains_key(session_key) {
            return Ok(());
        }
        if self.sessions.len() >= self.config.max_sessions {
            return Err(format!(
                "Maximum browser sessions reached ({})",
                self.config.max_sessions
            ));
        }

        let bridge_path = self.ensure_bridge_script()?;
        let (program, runtime) = self.resolve_python_invocation();

        let mut cmd = std::process::Command::new(&program);
        cmd.arg(bridge_path.to_string_lossy().as_ref());
        if self.config.headless {
            cmd.arg("--headless");
        } else {
            cmd.arg("--no-headless");
        }
        cmd.arg("--width")
            .arg(self.config.viewport_width.to_string());
        cmd.arg("--height")
            .arg(self.config.viewport_height.to_string());
        cmd.arg("--timeout")
            .arg(self.config.timeout_secs.to_string());

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        Self::apply_browser_env(&mut cmd, runtime.as_ref());

        let mut child = cmd.spawn().map_err(|err| {
            format!(
                "Failed to spawn browser bridge: {err}. Ensure Python and Playwright are installed."
            )
        })?;

        let stdin = child.stdin.take().ok_or("Failed to capture bridge stdin")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Failed to capture bridge stdout")?;
        let mut reader = BufReader::new(stdout);

        let mut ready_line = String::new();
        reader
            .read_line(&mut ready_line)
            .map_err(|err| format!("Bridge failed to start: {err}"))?;

        if ready_line.trim().is_empty() {
            let _ = child.kill();
            return Err(
                "Browser bridge exited without ready signal. Check Python/Playwright installation."
                    .to_string(),
            );
        }

        let ready: BrowserResponse = serde_json::from_str(ready_line.trim())
            .map_err(|err| format!("Bridge startup failed: {err}. Output: {ready_line}"))?;

        if !ready.success {
            let err = ready.error.unwrap_or_else(|| "Unknown error".to_string());
            let _ = child.kill();
            return Err(format!("Browser bridge failed to start: {err}"));
        }

        info!(session_key, "Browser session created");
        let session = BrowserSession {
            child,
            stdin,
            stdout: reader,
            last_active: Instant::now(),
        };
        self.sessions
            .insert(session_key.to_string(), Mutex::new(session));
        Ok(())
    }

    pub async fn send_command(
        &self,
        session_key: &str,
        cmd: BrowserCommand,
    ) -> Result<BrowserResponse, String> {
        tokio::task::block_in_place(|| self.get_or_create_sync(session_key))?;

        if self.config.idle_timeout_secs > 0 {
            if let Some(entry) = self.sessions.get(session_key) {
                let mut session = entry.value().lock().await;
                if session.last_active.elapsed().as_secs() > self.config.idle_timeout_secs {
                    session.kill();
                    drop(session);
                    drop(entry);
                    self.sessions.remove(session_key);
                    tokio::task::block_in_place(|| self.get_or_create_sync(session_key))?;
                }
            }
        }

        let entry = self
            .sessions
            .get(session_key)
            .ok_or_else(|| "Browser session disappeared".to_string())?;
        let mut session = entry.value().lock().await;
        let response = tokio::task::block_in_place(|| session.send(&cmd))?;

        if !response.success {
            let err = response
                .error
                .clone()
                .unwrap_or_else(|| "Unknown error".to_string());
            warn!(session_key, error = %err, "Browser command failed");
        }

        Ok(response)
    }

    pub async fn close_session(&self, session_key: &str) {
        if let Some((_, session_mutex)) = self.sessions.remove(session_key) {
            let mut session = session_mutex.lock().await;
            let _ = session.send(&BrowserCommand::Close);
            session.kill();
            info!(session_key, "Browser session closed");
        }
    }
}

pub fn browser_tools_enabled(config: &Config) -> bool {
    config.server.mode.trim().eq_ignore_ascii_case("desktop") && config.tools.browser.enabled
}

pub fn is_browser_tool_name(name: &str) -> bool {
    matches!(
        name,
        TOOL_BROWSER
            | TOOL_BROWSER_NAVIGATE
            | TOOL_BROWSER_CLICK
            | TOOL_BROWSER_TYPE
            | TOOL_BROWSER_SCREENSHOT
            | TOOL_BROWSER_READ_PAGE
            | TOOL_BROWSER_CLOSE
    )
}

pub async fn tool_browser(
    context: &ToolContext<'_>,
    tool_name: &str,
    args: &Value,
) -> Result<Value> {
    let action = if let Some(raw) = args.get("action").and_then(Value::as_str) {
        BrowserAction::from_str(raw).ok_or_else(|| anyhow!("Unknown browser action: {raw}"))?
    } else {
        action_from_tool_name(tool_name).ok_or_else(|| anyhow!("Missing 'action' parameter"))?
    };
    match action {
        BrowserAction::Navigate => tool_browser_navigate(context, args).await,
        BrowserAction::Click => tool_browser_click(context, args).await,
        BrowserAction::Type => tool_browser_type(context, args).await,
        BrowserAction::Screenshot => tool_browser_screenshot(context, args).await,
        BrowserAction::ReadPage => tool_browser_read_page(context, args).await,
        BrowserAction::Close => tool_browser_close(context, args).await,
    }
}

pub async fn tool_browser_navigate(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    ensure_browser_available(context.config)?;
    let url = require_str(args, "url")?;
    let normalized = normalize_url(url)?;
    let session_key = browser_session_key(context);
    let mgr = browser_manager(context.config)?;
    let resp = mgr
        .send_command(
            &session_key,
            BrowserCommand::Navigate {
                url: normalized.clone(),
            },
        )
        .await
        .map_err(|err| anyhow!(err))?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "Navigate failed".to_string())));
    }
    let data = resp.data.unwrap_or_default();
    let title = data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let page_url = data
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or(&normalized)
        .to_string();
    let content = data
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(json!({
        "url": page_url,
        "title": title,
        "content": content,
    }))
}

pub async fn tool_browser_click(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    ensure_browser_available(context.config)?;
    let selector = require_str(args, "selector")?;
    let session_key = browser_session_key(context);
    let mgr = browser_manager(context.config)?;
    let resp = mgr
        .send_command(
            &session_key,
            BrowserCommand::Click {
                selector: selector.to_string(),
            },
        )
        .await
        .map_err(|err| anyhow!(err))?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "Click failed".to_string())));
    }
    let data = resp.data.unwrap_or_default();
    let title = data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let url = data
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(json!({
        "clicked": selector,
        "title": title,
        "url": url,
    }))
}

pub async fn tool_browser_type(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    ensure_browser_available(context.config)?;
    let selector = require_str(args, "selector")?;
    let text = require_str(args, "text")?;
    let session_key = browser_session_key(context);
    let mgr = browser_manager(context.config)?;
    let resp = mgr
        .send_command(
            &session_key,
            BrowserCommand::Type {
                selector: selector.to_string(),
                text: text.to_string(),
            },
        )
        .await
        .map_err(|err| anyhow!(err))?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "Type failed".to_string())));
    }
    Ok(json!({
        "selector": selector,
        "text": text,
    }))
}

pub async fn tool_browser_screenshot(context: &ToolContext<'_>, _args: &Value) -> Result<Value> {
    ensure_browser_available(context.config)?;
    let session_key = browser_session_key(context);
    let mgr = browser_manager(context.config)?;
    let resp = mgr
        .send_command(&session_key, BrowserCommand::Screenshot)
        .await
        .map_err(|err| anyhow!(err))?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "Screenshot failed".to_string())));
    }
    let data = resp.data.unwrap_or_default();
    let b64 = data
        .get("image_base64")
        .and_then(Value::as_str)
        .unwrap_or("");
    if b64.is_empty() {
        return Err(anyhow!("Screenshot returned empty payload"));
    }
    let url = data
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|err| anyhow!("Screenshot base64 decode failed: {err}"))?;
    let (filename, download_url) = save_screenshot(&bytes)?;

    Ok(json!({
        "url": url,
        "filename": filename,
        "download_url": download_url,
    }))
}

pub async fn tool_browser_read_page(context: &ToolContext<'_>, _args: &Value) -> Result<Value> {
    ensure_browser_available(context.config)?;
    let session_key = browser_session_key(context);
    let mgr = browser_manager(context.config)?;
    let resp = mgr
        .send_command(&session_key, BrowserCommand::ReadPage)
        .await
        .map_err(|err| anyhow!(err))?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "ReadPage failed".to_string())));
    }
    let data = resp.data.unwrap_or_default();
    let title = data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let url = data
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let content = data
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(json!({
        "url": url,
        "title": title,
        "content": content,
    }))
}

pub async fn tool_browser_close(context: &ToolContext<'_>, _args: &Value) -> Result<Value> {
    if !browser_tools_enabled(context.config) {
        return Ok(json!({ "closed": true }));
    }
    let session_key = browser_session_key(context);
    if let Ok(mgr) = browser_manager(context.config) {
        mgr.close_session(&session_key).await;
    }
    Ok(json!({ "closed": true }))
}

fn ensure_browser_available(config: &Config) -> Result<()> {
    if browser_tools_enabled(config) {
        return Ok(());
    }
    Err(anyhow!("浏览器工具仅在 desktop 模式启用"))
}

fn browser_session_key(context: &ToolContext<'_>) -> String {
    if let Some(agent_id) = context
        .agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!("{}:{agent_id}", context.user_id);
    }
    format!("{}:{}", context.user_id, context.session_id)
}

fn browser_manager(config: &Config) -> Result<&'static BrowserManager> {
    static INSTANCE: OnceLock<BrowserManager> = OnceLock::new();
    let cfg = config.tools.browser.clone();
    Ok(INSTANCE.get_or_init(|| BrowserManager::new(cfg)))
}

fn normalize_url(input: &str) -> Result<String> {
    let parsed = Url::parse(input).map_err(|err| anyhow!("Invalid URL: {err}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed.to_string()),
        _ => Err(anyhow!("Only http/https URLs are supported")),
    }
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("Missing '{key}' parameter"))
}

fn save_screenshot(bytes: &[u8]) -> Result<(String, String)> {
    let dir = resolve_temp_dir()?;
    std::fs::create_dir_all(&dir).map_err(|err| anyhow!("Create temp dir failed: {err}"))?;
    let filename = format!("browser_shot_{}.png", Uuid::new_v4().simple());
    let path = dir.join(&filename);
    std::fs::write(&path, bytes).map_err(|err| anyhow!("Write screenshot failed: {err}"))?;
    let download_url = format!("/wunder/temp_dir/download?filename={filename}");
    Ok((filename, download_url))
}

fn resolve_temp_dir() -> Result<PathBuf> {
    if let Ok(value) = std::env::var(TEMP_DIR_ROOT_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.is_absolute() {
                return Ok(candidate);
            }
            let root = std::env::current_dir().map_err(|err| anyhow!(err))?;
            return Ok(root.join(candidate));
        }
    }
    let root = std::env::current_dir().map_err(|err| anyhow!(err))?;
    Ok(root.join("temp_dir"))
}

fn prepend_path_env(cmd: &mut std::process::Command, key: &str, value: &Path) {
    let mut entries = vec![value.to_path_buf()];
    if let Some(existing) = std::env::var_os(key) {
        entries.extend(std::env::split_paths(&existing));
    }
    match std::env::join_paths(entries) {
        Ok(merged) => {
            cmd.env(key, merged);
        }
        Err(_) => {
            let prefix = value.to_string_lossy();
            let sep = if cfg!(windows) { ';' } else { ':' };
            let merged = match std::env::var(key) {
                Ok(existing) if !existing.trim().is_empty() => format!("{prefix}{sep}{existing}"),
                _ => prefix.to_string(),
            };
            cmd.env(key, merged);
        }
    };
}
