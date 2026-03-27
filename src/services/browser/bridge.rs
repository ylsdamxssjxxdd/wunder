use super::config::EffectiveBrowserConfig;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

const BRIDGE_SCRIPT: &str = include_str!("browser_bridge.py");

#[derive(Debug, Deserialize)]
pub struct BridgeResponse {
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
}

struct BridgeSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    last_active: Instant,
}

impl BridgeSession {
    fn send(&mut self, command: &Value) -> Result<BridgeResponse, String> {
        let payload =
            serde_json::to_string(command).map_err(|err| format!("Serialize error: {err}"))?;
        self.stdin
            .write_all(payload.as_bytes())
            .map_err(|err| format!("Failed to write to browser stdin: {err}"))?;
        self.stdin
            .write_all(b"\n")
            .map_err(|err| format!("Failed to write command newline: {err}"))?;
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

impl Drop for BridgeSession {
    fn drop(&mut self) {
        self.kill();
    }
}

pub struct BrowserBridge {
    sessions: DashMap<String, Mutex<BridgeSession>>,
    config: EffectiveBrowserConfig,
    bridge_path: OnceLock<PathBuf>,
}

impl BrowserBridge {
    pub fn new(config: EffectiveBrowserConfig) -> Self {
        Self {
            sessions: DashMap::new(),
            config,
            bridge_path: OnceLock::new(),
        }
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn session_keys(&self) -> Vec<String> {
        self.sessions
            .iter()
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>()
    }

    pub async fn send_command(
        &self,
        session_key: &str,
        command: Value,
    ) -> Result<BridgeResponse, String> {
        tokio::task::block_in_place(|| self.ensure_session(session_key))?;
        if self.config.idle_timeout_secs > 0 {
            if let Some(entry) = self.sessions.get(session_key) {
                let mut session = entry.value().lock().await;
                if session.last_active.elapsed().as_secs() > self.config.idle_timeout_secs {
                    session.kill();
                    drop(session);
                    drop(entry);
                    self.sessions.remove(session_key);
                    tokio::task::block_in_place(|| self.ensure_session(session_key))?;
                }
            }
        }
        let entry = self
            .sessions
            .get(session_key)
            .ok_or_else(|| "Browser bridge session disappeared".to_string())?;
        let mut session = entry.value().lock().await;
        let response = match tokio::task::block_in_place(|| session.send(&command)) {
            Ok(response) => response,
            Err(err) => {
                drop(session);
                drop(entry);
                self.sessions.remove(session_key);
                return Err(err);
            }
        };
        if !response.success {
            let error = response
                .error
                .clone()
                .unwrap_or_else(|| "Unknown browser bridge error".to_string());
            warn!(session_key, error = %error, "Browser bridge command failed");
        }
        Ok(response)
    }

    pub async fn close_session(&self, session_key: &str) {
        if let Some((_, mutex)) = self.sessions.remove(session_key) {
            let mut session = mutex.lock().await;
            let _ = session.send(&serde_json::json!({ "action": "close" }));
            session.kill();
            info!(session_key, "Browser bridge session closed");
        }
    }

    fn ensure_session(&self, session_key: &str) -> Result<(), String> {
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
        let mut command = self.spawn_command(bridge_path);
        let mut child = command.spawn().map_err(|err| {
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
        let ready: BridgeResponse = serde_json::from_str(ready_line.trim())
            .map_err(|err| format!("Bridge startup failed: {err}. Output: {ready_line}"))?;
        if !ready.success {
            let error = ready
                .error
                .unwrap_or_else(|| "Unknown startup error".to_string());
            let _ = child.kill();
            return Err(format!("Browser bridge failed to start: {error}"));
        }
        self.sessions.insert(
            session_key.to_string(),
            Mutex::new(BridgeSession {
                child,
                stdin,
                stdout: reader,
                last_active: Instant::now(),
            }),
        );
        info!(session_key, "Browser bridge session created");
        Ok(())
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

    fn spawn_command(&self, bridge_path: &Path) -> std::process::Command {
        let mut command = std::process::Command::new(self.python_program());
        command.arg(bridge_path.to_string_lossy().as_ref());
        if self.config.headless {
            command.arg("--headless");
        } else {
            command.arg("--no-headless");
        }
        command
            .arg("--width")
            .arg(self.config.viewport_width.to_string())
            .arg("--height")
            .arg(self.config.viewport_height.to_string())
            .arg("--max-tabs")
            .arg(self.config.max_tabs_per_session.to_string())
            .arg("--timeout")
            .arg(self.config.timeout_secs.to_string());
        for arg in self.launch_args() {
            command.arg("--launch-arg").arg(arg);
        }
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::null());
        apply_browser_env(&mut command, &self.config);
        command
    }

    fn python_program(&self) -> String {
        self.config
            .python_path
            .clone()
            .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.to_string())
    }

    fn launch_args(&self) -> Vec<String> {
        let mut launch_args = self.config.launch_args.clone();
        if self.config.docker_enabled {
            if self.config.docker_use_no_sandbox
                && !launch_args.iter().any(|item| item == "--no-sandbox")
            {
                launch_args.push("--no-sandbox".to_string());
            }
            if self.config.docker_disable_dev_shm_usage
                && !launch_args
                    .iter()
                    .any(|item| item == "--disable-dev-shm-usage")
            {
                launch_args.push("--disable-dev-shm-usage".to_string());
            }
        }
        launch_args
    }
}

fn apply_browser_env(command: &mut std::process::Command, config: &EffectiveBrowserConfig) {
    command.env_clear();
    for key in [
        "SYSTEMROOT",
        "PATH",
        "TEMP",
        "TMP",
        "TMPDIR",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CACHE_HOME",
        "SSL_CERT_FILE",
        "PYTHONHOME",
        "PYTHONPATH",
        "LD_LIBRARY_PATH",
    ] {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
    command.env("PYTHONIOENCODING", "utf-8");
    if let Some(path) = &config.browsers_path {
        command.env("PLAYWRIGHT_BROWSERS_PATH", path);
    } else if let Ok(value) = std::env::var("PLAYWRIGHT_BROWSERS_PATH") {
        command.env("PLAYWRIGHT_BROWSERS_PATH", value);
    }
    if let Some(path) = &config.docker_downloads_root {
        command.env("WUNDER_BROWSER_DOWNLOAD_ROOT", path);
    }
    if let Ok(value) = std::env::var("PLAYWRIGHT_DOWNLOAD_HOST") {
        command.env("PLAYWRIGHT_DOWNLOAD_HOST", value);
    }
    if let Ok(value) = std::env::var("PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD") {
        command.env("PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD", value);
    }
}
