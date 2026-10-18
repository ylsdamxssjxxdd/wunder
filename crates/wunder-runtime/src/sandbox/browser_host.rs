//! Playwright bridge processes hosted by the sandbox.
//!
//! 容器部署里 Chromium 是该容器最大的内存与崩溃源。把桥接进程放到沙盒里，
//! 主进程只保留会话表、预热池和传输层：浏览器崩了、吃满内存了，都不再拖累
//! 服务本体。协议与本地管道完全一致（一行命令、一行应答），所以上层的
//! 会话语义、预热池和失败契约不需要分叉。

use crate::services::browser::bridge::{apply_bridge_environment, write_bridge_script};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Bounded like the server's own session table: an unbounded bridge map is just
/// a slow OOM with extra steps.
const MAX_BRIDGES: usize = 32;
const JANITOR_PERIOD: Duration = Duration::from_secs(60);
const STARTUP_FLOOR: Duration = Duration::from_secs(120);

/// Launch parameters resolved by the server. The sandbox deliberately reads no
/// browser config of its own: one source of truth, the process that owns policy.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BridgeSpec {
    pub python_path: Option<String>,
    pub browsers_path: Option<String>,
    pub download_root: Option<String>,
    pub headless: bool,
    pub width: u32,
    pub height: u32,
    pub max_tabs: usize,
    pub timeout_secs: u64,
    /// How long an idle bridge may keep Chromium alive here. This is the safety
    /// net for a server that died without closing its bridges.
    pub idle_timeout_secs: u64,
    pub mode: String,
    pub channel: Option<String>,
    pub executable_path: Option<String>,
    pub attach_endpoint: Option<String>,
    pub allow_private_network: bool,
    pub deny_file_scheme: bool,
    pub hostname_allowlist: Vec<String>,
    pub launch_args: Vec<String>,
}

impl Default for BridgeSpec {
    fn default() -> Self {
        Self {
            python_path: None,
            browsers_path: None,
            download_root: None,
            headless: true,
            width: 1280,
            height: 720,
            max_tabs: 8,
            timeout_secs: 60,
            idle_timeout_secs: 1800,
            mode: "launch".to_string(),
            channel: None,
            executable_path: None,
            attach_endpoint: None,
            allow_private_network: false,
            deny_file_scheme: true,
            hostname_allowlist: Vec::new(),
            launch_args: Vec::new(),
        }
    }
}

impl BridgeSpec {
    fn startup_timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.max(30)).max(STARTUP_FLOOR)
    }
}

struct BridgeIo {
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

struct HostedBridge {
    bridge_id: String,
    /// Kept apart from `io` on purpose: closing must be able to kill a bridge
    /// that is parked inside a read, without waiting for that read to return.
    child: Arc<Mutex<Option<Child>>>,
    io: Mutex<BridgeIo>,
    last_active_ms: AtomicU64,
    idle_timeout_secs: u64,
}

impl HostedBridge {
    fn touch(&self) {
        self.last_active_ms.store(now_ms(), Ordering::Relaxed);
    }

    fn idle_for(&self, now: u64) -> Duration {
        Duration::from_millis(now.saturating_sub(self.last_active_ms.load(Ordering::Relaxed)))
    }

    fn kill(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
            }
        }
    }

    fn reap(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

fn bridges() -> &'static Mutex<HashMap<String, Arc<HostedBridge>>> {
    static BRIDGES: OnceLock<Mutex<HashMap<String, Arc<HostedBridge>>>> = OnceLock::new();
    BRIDGES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn start_instant() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    start_instant().elapsed().as_millis() as u64
}

fn lock_map() -> std::sync::MutexGuard<'static, HashMap<String, Arc<HostedBridge>>> {
    bridges()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Start one bridge and return its raw readiness line.
///
/// Blocking: the caller runs it on a blocking thread. The line is handed back
/// verbatim so the server parses startup failures with its own rules.
pub fn create(bridge_id: &str, spec: &BridgeSpec) -> Result<String, String> {
    let bridge_id = bridge_id.trim();
    if bridge_id.is_empty() {
        return Err("bridge_id is required".to_string());
    }
    // Same id means the server is replacing a bridge it no longer tracks.
    close(bridge_id);
    ensure_capacity()?;

    let mut command = build_command(spec)?;
    let mut child = command.spawn().map_err(|err| {
        format!(
            "Failed to spawn browser bridge in the sandbox: {err}. \
             Ensure Python and Playwright are installed in this image."
        )
    })?;
    let stdin = child
        .stdin
        .take()
        .ok_or("Failed to capture browser bridge stdin")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture browser bridge stdout")?;

    let child_slot = Arc::new(Mutex::new(Some(child)));
    let watchdog = StartupWatchdog::arm(Arc::clone(&child_slot), spec.startup_timeout());
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let read = reader.read_line(&mut line);
    watchdog.disarm();
    if let Err(err) = read {
        reap_slot(&child_slot);
        return Err(format!(
            "Browser bridge failed to start in the sandbox: {err}"
        ));
    }
    if line.trim().is_empty() {
        reap_slot(&child_slot);
        return Err(
            "Browser bridge exited in the sandbox without a ready signal. Check that Chromium is \
             installed for Playwright in this image."
                .to_string(),
        );
    }

    let bridge = Arc::new(HostedBridge {
        bridge_id: bridge_id.to_string(),
        child: child_slot,
        io: Mutex::new(BridgeIo { stdin, reader }),
        last_active_ms: AtomicU64::new(now_ms()),
        idle_timeout_secs: spec.idle_timeout_secs,
    });
    lock_map().insert(bridge_id.to_string(), bridge);
    ensure_janitor();
    info!(bridge_id, "Browser bridge started in the sandbox");
    Ok(line.trim().to_string())
}

/// One command line in, one reply line out.
pub fn command(bridge_id: &str, line: &str) -> Result<String, String> {
    let bridge = lookup(bridge_id)?;
    bridge.touch();
    let mut io = bridge
        .io
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let outcome = (|| -> Result<String, String> {
        io.stdin
            .write_all(line.as_bytes())
            .and_then(|_| io.stdin.write_all(b"\n"))
            .and_then(|_| io.stdin.flush())
            .map_err(|err| format!("Failed to write to browser bridge: {err}"))?;
        let mut reply = String::new();
        io.reader
            .read_line(&mut reply)
            .map_err(|err| format!("Failed to read browser bridge reply: {err}"))?;
        if reply.trim().is_empty() {
            return Err("Browser bridge closed unexpectedly".to_string());
        }
        Ok(reply.trim().to_string())
    })();
    drop(io);
    if outcome.is_err() {
        // A bridge that cannot answer is dead weight; drop it before the slot
        // leaks into the next command.
        close(bridge_id);
    } else {
        bridge.touch();
    }
    outcome
}

/// Stop one bridge. Idempotent: closing a bridge that already died is normal.
pub fn close(bridge_id: &str) -> bool {
    let removed = lock_map().remove(bridge_id.trim());
    match removed {
        Some(bridge) => {
            bridge.reap();
            debug!(
                bridge_id = bridge.bridge_id.as_str(),
                "Browser bridge stopped"
            );
            true
        }
        None => false,
    }
}

pub fn list() -> Value {
    let now = now_ms();
    let guard = lock_map();
    let mut items = guard
        .values()
        .map(|bridge| {
            json!({
                "bridge_id": bridge.bridge_id,
                "idle_s": bridge.idle_for(now).as_secs(),
                "idle_timeout_s": bridge.idle_timeout_secs,
                "pid": bridge
                    .child
                    .lock()
                    .ok()
                    .and_then(|guard| guard.as_ref().map(Child::id)),
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left["bridge_id"].as_str().cmp(&right["bridge_id"].as_str()));
    json!({ "bridges": items, "count": items.len(), "limit": MAX_BRIDGES })
}

/// A zero timeout disables the safety net rather than expiring everything.
fn expired(idle_for: Duration, idle_timeout_secs: u64) -> bool {
    idle_timeout_secs > 0 && idle_for >= Duration::from_secs(idle_timeout_secs)
}

/// Kill bridges idle past their own timeout, plus any that outlived their
/// process. Returns how many were stopped.
pub fn sweep_idle() -> usize {
    let now = now_ms();
    let expired_ids = lock_map()
        .values()
        .filter(|bridge| expired(bridge.idle_for(now), bridge.idle_timeout_secs))
        .map(|bridge| bridge.bridge_id.clone())
        .collect::<Vec<_>>();
    for bridge_id in &expired_ids {
        if close(bridge_id) {
            warn!(bridge_id, "Idle browser bridge in the sandbox was reaped");
        }
    }
    expired_ids.len()
}

fn lookup(bridge_id: &str) -> Result<Arc<HostedBridge>, String> {
    lock_map()
        .get(bridge_id.trim())
        .cloned()
        .ok_or_else(|| "browser bridge is not running in the sandbox".to_string())
}

fn kill_slot(slot: &Arc<Mutex<Option<Child>>>) {
    if let Ok(mut guard) = slot.lock() {
        if let Some(child) = guard.as_mut() {
            let _ = child.kill();
        }
    }
}

/// Kill and wait. A half-started bridge that never reported ready still owns a
/// process; without the wait it lingers as a zombie for the life of the image.
fn reap_slot(slot: &Arc<Mutex<Option<Child>>>) {
    if let Ok(mut guard) = slot.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A launch that never reports ready must not pin the caller forever: kill the
/// child after the timeout, which unblocks the pending read.
struct StartupWatchdog {
    armed: Arc<std::sync::atomic::AtomicBool>,
}

impl StartupWatchdog {
    fn arm(slot: Arc<Mutex<Option<Child>>>, timeout: Duration) -> Self {
        let armed = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let flag = Arc::clone(&armed);
        let _ = std::thread::Builder::new()
            .name("browser-host-watchdog".to_string())
            .spawn(move || {
                let deadline = Instant::now() + timeout;
                while Instant::now() < deadline {
                    if !flag.load(Ordering::Acquire) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                if flag.load(Ordering::Acquire) {
                    warn!("Browser bridge did not report ready in the sandbox; killing it");
                    kill_slot(&slot);
                }
            });
        Self { armed }
    }

    fn disarm(&self) {
        self.armed.store(false, Ordering::Release);
    }
}

fn ensure_capacity() -> Result<(), String> {
    let mut guard = lock_map();
    if guard.len() < MAX_BRIDGES {
        return Ok(());
    }
    let now = now_ms();
    let victim = guard
        .values()
        .max_by_key(|bridge| bridge.idle_for(now))
        .map(|bridge| bridge.bridge_id.clone());
    let Some(victim) = victim else {
        return Err("sandbox browser bridge limit reached".to_string());
    };
    if let Some(bridge) = guard.remove(&victim) {
        bridge.reap();
        warn!(
            bridge_id = victim.as_str(),
            limit = MAX_BRIDGES,
            "Browser bridge evicted to free a slot in the sandbox"
        );
    }
    Ok(())
}

fn ensure_janitor() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            // No runtime (unit tests): the sweep still runs on traffic.
            return;
        };
        handle.spawn(async move {
            let mut ticker = tokio::time::interval(JANITOR_PERIOD);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                sweep_idle();
            }
        });
    });
}

fn build_command(spec: &BridgeSpec) -> Result<Command, String> {
    let script: PathBuf = write_bridge_script()?;
    let python = spec
        .python_path
        .clone()
        .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.to_string());
    let mut command = Command::new(python);
    command.arg(script);
    push_bridge_args(&mut command, spec);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let download_root = spec
        .download_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    apply_bridge_environment(
        &mut command,
        spec.browsers_path.as_deref(),
        download_root.as_deref(),
    );
    crate::core::command_utils::apply_platform_spawn_options_std(&mut command);
    Ok(command)
}

/// The bridge's own argument surface, mirroring `build_spawn_command` in the
/// local bridge so both hosts accept exactly the same launch policy.
fn push_bridge_args(command: &mut Command, spec: &BridgeSpec) {
    if spec.headless {
        command.arg("--headless");
    } else {
        command.arg("--no-headless");
    }
    command
        .arg("--width")
        .arg(spec.width.max(1).to_string())
        .arg("--height")
        .arg(spec.height.max(1).to_string())
        .arg("--max-tabs")
        .arg(spec.max_tabs.max(1).to_string())
        .arg("--timeout")
        .arg(spec.timeout_secs.max(1).to_string());
    let mode = spec.mode.trim();
    if !mode.is_empty() {
        command.arg("--mode").arg(mode);
    }
    if let Some(channel) = spec.channel.as_deref().map(str::trim) {
        if !channel.is_empty() {
            command.arg("--channel").arg(channel);
        }
    }
    if let Some(path) = spec.executable_path.as_deref().map(str::trim) {
        if !path.is_empty() {
            command.arg("--executable-path").arg(path);
        }
    }
    if let Some(endpoint) = spec.attach_endpoint.as_deref().map(str::trim) {
        if !endpoint.is_empty() {
            command.arg("--attach-endpoint").arg(endpoint);
        }
    }
    if spec.allow_private_network {
        command.arg("--allow-private-network");
    }
    if !spec.deny_file_scheme {
        command.arg("--allow-file-scheme");
    }
    for host in &spec.hostname_allowlist {
        let host = host.trim();
        if !host.is_empty() {
            command.arg("--allow-host").arg(host);
        }
    }
    for arg in &spec.launch_args {
        command.arg(format!("--launch-arg={arg}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_are_a_usable_headless_browser() {
        let spec = BridgeSpec::default();
        assert!(spec.headless);
        assert_eq!(spec.mode, "launch");
        assert!(spec.deny_file_scheme);
        assert!(!spec.allow_private_network);
        assert!(spec.idle_timeout_secs > 0);
    }

    #[test]
    fn startup_timeout_has_a_floor() {
        let mut spec = BridgeSpec::default();
        spec.timeout_secs = 1;
        assert!(spec.startup_timeout() >= STARTUP_FLOOR);
    }

    #[test]
    fn missing_bridge_is_reported_by_id() {
        assert!(lookup("no-such-bridge").is_err());
        assert!(!close("no-such-bridge"));
    }

    #[test]
    fn idle_expiry_honours_a_zero_timeout_as_disabled() {
        // 0 means "keep it": the server turns the safety net off, and reading
        // that as "expired at once" would kill every bridge on the first tick.
        assert!(!expired(Duration::from_secs(10_000), 0));
        assert!(!expired(Duration::from_secs(9), 10));
        assert!(expired(Duration::from_secs(10), 10));
    }

    #[test]
    fn launch_parameters_reach_the_command_line() {
        let mut spec = BridgeSpec::default();
        spec.width = 1024;
        spec.max_tabs = 3;
        spec.launch_args = vec!["--no-sandbox".to_string()];
        spec.hostname_allowlist = vec!["example.com".to_string()];
        let mut command = Command::new("python3");
        push_bridge_args(&mut command, &spec);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(args.contains(&"1024".to_string()));
        assert!(args.contains(&"3".to_string()));
        assert!(args.contains(&"--launch-arg=--no-sandbox".to_string()));
        assert!(args.contains(&"example.com".to_string()));
        assert!(args.contains(&"--headless".to_string()));
    }
}
