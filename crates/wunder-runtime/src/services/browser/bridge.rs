use super::config::EffectiveBrowserConfig;
use crate::core::command_utils;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

const BRIDGE_SCRIPT: &str = include_str!("browser_bridge.py");

/// Launches get their own grace: the interpreter has to import Playwright and
/// spawn a browser before the first line of protocol traffic.
pub const LAUNCH_GRACE: Duration = Duration::from_secs(30);
/// The bridge protocol is strictly serialized, so the queue stays depth 1:
/// extra depth would only buffer work that can no longer be delivered on time.
const COMMAND_QUEUE_DEPTH: usize = 1;
const REAPER_PERIOD: Duration = Duration::from_secs(30);
const ATTACH_POLL: Duration = Duration::from_millis(50);
/// A session this recently active is never stolen by the capacity eviction.
const ACTIVE_PROTECT: Duration = Duration::from_secs(5);

const STATE_LAUNCHING: u8 = 0;
const STATE_READY: u8 = 1;
const STATE_DEAD: u8 = 2;

#[derive(Debug, Deserialize)]
pub struct BridgeResponse {
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
    #[serde(default)]
    pub traceback: Option<String>,
}

impl BridgeResponse {
    fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
            traceback: None,
        }
    }
}

struct Command {
    line: String,
    reply: oneshot::Sender<BridgeResponse>,
}

/// Process handles handed over to the pump thread at launch.
struct SpawnedBridge {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

/// Handle onto one live bridge process.
///
/// Teardown needs to reach a bridge that is wedged inside the page, where the
/// pump thread is parked on a stdout read that will never return. So the child
/// is shared: any teardown path can `kill()` it (non-blocking), which closes the
/// pipes and wakes the pump, and the pump is the only place that `wait()`s.
struct BridgeSession {
    key: String,
    tx: mpsc::Sender<Command>,
    /// `None` only in tests that exercise lifecycle bookkeeping without a
    /// process; the pump takes it to reap, every other path only kills.
    child: Arc<Mutex<Option<Child>>>,
    state: AtomicU8,
    last_active_ms: AtomicU64,
    /// Commands currently awaiting a reply. A long in-flight command must not
    /// look idle to the reaper, and a cancelled caller releases this via Drop.
    in_flight: Arc<AtomicU64>,
    /// Owned by the caller that started the launch; consuming it publishes
    /// `STATE_READY`, so a second concurrent caller cannot launch a second
    /// browser for the same key.
    startup_rx: Mutex<Option<oneshot::Receiver<Result<(), String>>>>,
}

impl BridgeSession {
    fn is_ready(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_READY
    }

    fn is_launching(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_LAUNCHING
    }

    fn mark_dead(&self) {
        self.state.store(STATE_DEAD, Ordering::Release);
    }

    fn idle_for(&self, now_ms: u64) -> Duration {
        let last = self.last_active_ms.load(Ordering::Relaxed);
        Duration::from_millis(now_ms.saturating_sub(last))
    }

    fn touch(&self) {
        self.last_active_ms
            .store(elapsed_millis(), Ordering::Relaxed);
    }

    /// Mark dead and kill the process. `Child::kill` does not block, and the
    /// pump reaps it once the closed pipes end its read.
    fn teardown(&self) {
        self.mark_dead();
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

impl Drop for BridgeSession {
    fn drop(&mut self) {
        // Dropping the request channel ends the pump loop, which then kills and
        // reaps the child; mark dead so nobody reattaches to a gone session.
        self.mark_dead();
    }
}

/// Counts commands awaiting a reply so neither the idle sweep nor a capacity
/// eviction can cut a long-running command off mid-flight. The guard is also
/// dropped when the caller's future is cancelled, so the count cannot stick.
struct InFlight<'a> {
    session: &'a BridgeSession,
}

impl<'a> InFlight<'a> {
    fn new(session: &'a Arc<BridgeSession>) -> Self {
        session.in_flight.fetch_add(1, Ordering::AcqRel);
        Self { session }
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.session.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

fn epoch_start() -> &'static Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now)
}

fn elapsed_millis() -> u64 {
    epoch_start().elapsed().as_millis() as u64
}

pub struct BrowserBridge {
    sessions: Arc<DashMap<String, Arc<BridgeSession>>>,
    config: EffectiveBrowserConfig,
    bridge_path: OnceLock<PathBuf>,
    reaper_started: AtomicU8,
}

impl BrowserBridge {
    pub fn new(config: EffectiveBrowserConfig) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            config,
            bridge_path: OnceLock::new(),
            reaper_started: AtomicU8::new(0),
        }
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn session_keys(&self) -> Vec<String> {
        let mut keys = self
            .sessions
            .iter()
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }

    /// Session keys owned by `owner`; a session key is always `{user}:{tail}`,
    /// so status can stay scoped to the caller instead of naming every tenant's
    /// browser in one shared process.
    pub fn session_keys_for_owner(&self, owner: &str) -> Vec<String> {
        let prefix = format!("{owner}:");
        let mut keys = self
            .sessions
            .iter()
            .filter(|entry| entry.key().starts_with(&prefix))
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }

    pub async fn send_command(
        &self,
        session_key: &str,
        command: Value,
        call_deadline: Duration,
    ) -> Result<BridgeResponse, String> {
        let session = self.acquire_session(session_key).await?;
        let _in_flight = InFlight::new(&session);
        session.touch();

        let (reply_tx, reply_rx) = oneshot::channel();
        let line =
            serde_json::to_string(&command).map_err(|err| format!("Serialize error: {err}"))?;
        let deadline = tokio::time::Instant::now() + call_deadline;
        let permit = match tokio::time::timeout_at(deadline, session.tx.reserve()).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => {
                self.retire(&session, "bridge pump gone");
                return Err("Browser bridge closed before the command was queued.".to_string());
            }
            Err(_) => {
                // This command has not run: do not kill another caller's active
                // page operation just because the bounded queue is occupied.
                return Err(
                    "Browser command queue timed out; this command was not executed.".to_string(),
                );
            }
        };
        permit.send(Command {
            line,
            reply: reply_tx,
        });

        match tokio::time::timeout_at(deadline, reply_rx).await {
            Ok(Ok(response)) => {
                // Idle is counted from the last finished command, not from the
                // moment this one started.
                session.touch();
                if !response.success {
                    let error = response
                        .error
                        .clone()
                        .unwrap_or_else(|| "Unknown browser bridge error".to_string());
                    warn!(session_key, error = %error, "Browser bridge command failed");
                }
                Ok(response)
            }
            Ok(Err(_)) => {
                self.retire(&session, "bridge reply dropped");
                Err(format!(
                    "Browser bridge session '{session_key}' closed unexpectedly; retry the command."
                ))
            }
            Err(_elapsed) => {
                // The bridge is stuck inside the page: a wedged JS thread does
                // not honour Playwright's own timeouts, so this session can
                // never answer and must not keep holding its slot.
                self.retire(&session, "command deadline exceeded");
                warn!(
                    session_key,
                    deadline_ms = call_deadline.as_millis(),
                    "Browser bridge command exceeded its wall-clock deadline"
                );
                Err(format!(
                    "Browser bridge command timed out after {}s. The browser session was closed and will restart on the next call.",
                    call_deadline.as_secs()
                ))
            }
        }
    }

    pub async fn close_session(&self, session_key: &str) {
        if let Some((_, session)) = self.sessions.remove(session_key) {
            session.teardown();
            info!(session_key, "Browser bridge session closed");
        }
    }

    async fn acquire_session(&self, session_key: &str) -> Result<Arc<BridgeSession>, String> {
        self.ensure_reaper();
        let deadline = launch_deadline(self.config.timeout_secs);
        if let Some(entry) = self.sessions.get(session_key) {
            let session = Arc::clone(entry.value());
            drop(entry);
            return self.attach(&session, deadline).await;
        }

        if self.sessions.len() >= self.config.max_sessions {
            if self.sweep_expired() == 0 {
                self.evict_for_capacity()?;
            }
        }

        let session = self.launch(session_key)?;
        let startup = session
            .startup_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let outcome = match startup {
            None => Err("Browser bridge readiness was consumed by another call".to_string()),
            Some(rx) => match tokio::time::timeout(deadline, rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(
                    "Browser bridge pump ended before reporting readiness".to_string(),
                ),
                Err(_elapsed) => Err(format!(
                    "Browser bridge did not become ready within {}s. Check that Python and Playwright are installed.",
                    deadline.as_secs()
                )),
            },
        };
        match outcome {
            Ok(()) => {
                session.state.store(STATE_READY, Ordering::Release);
                session.touch();
                Ok(session)
            }
            Err(err) => {
                self.retire(&session, "launch failed");
                Err(err)
            }
        }
    }

    /// Reuse a ready session, wait out a launch started by a concurrent caller,
    /// or drop a dead one so the next attempt starts clean.
    async fn attach(
        &self,
        session: &Arc<BridgeSession>,
        deadline: Duration,
    ) -> Result<Arc<BridgeSession>, String> {
        if session.is_ready() {
            return Ok(Arc::clone(session));
        }
        if session.is_launching() {
            let waited = tokio::time::timeout(deadline, async {
                while session.is_launching() {
                    tokio::time::sleep(ATTACH_POLL).await;
                }
            })
            .await;
            if waited.is_ok() && session.is_ready() {
                return Ok(Arc::clone(session));
            }
            self.remove_if_same(session);
            return Err(format!(
                "Browser bridge session '{}' is being prepared by another call; retry this command.",
                session.key
            ));
        }
        self.remove_if_same(session);
        Err(format!(
            "Browser bridge session '{}' is no longer alive; retry the command.",
            session.key
        ))
    }

    fn launch(&self, session_key: &str) -> Result<Arc<BridgeSession>, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(COMMAND_QUEUE_DEPTH);
        let (startup_tx, startup_rx) = oneshot::channel::<Result<(), String>>();
        let session = Arc::new(BridgeSession {
            key: session_key.to_string(),
            tx: cmd_tx,
            child: Arc::new(Mutex::new(None)),
            state: AtomicU8::new(STATE_LAUNCHING),
            last_active_ms: AtomicU64::new(elapsed_millis()),
            in_flight: Arc::new(AtomicU64::new(0)),
            startup_rx: Mutex::new(Some(startup_rx)),
        });
        // Reserve the slot before touching the process: a concurrent caller
        // attaches to this launch instead of starting a second browser.
        match self.sessions.entry(session_key.to_string()) {
            dashmap::mapref::entry::Entry::Occupied(_) => {
                return Err(format!(
                    "Browser bridge session '{session_key}' is being prepared by another call."
                ));
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                vacant.insert(Arc::clone(&session));
            }
        }

        let spawn_result = self.spawn_bridge();
        let spawned = match spawn_result {
            Ok(spawned) => spawned,
            Err(err) => {
                self.remove_if_same(&session);
                return Err(err);
            }
        };
        let mut guard = session
            .child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(spawned.child);
        drop(guard);

        let pump_child = Arc::clone(&session.child);
        let pump_slot = short_slot(session_key);
        let spawn_pump = std::thread::Builder::new()
            .name(format!("browser-bridge-pump-{pump_slot}"))
            .spawn(move || {
                pump_loop(
                    spawned.stdin,
                    spawned.stdout,
                    cmd_rx,
                    startup_tx,
                    pump_child,
                )
            });
        if let Err(err) = spawn_pump {
            self.remove_if_same(&session);
            session.teardown();
            return Err(format!("Failed to start browser bridge pump: {err}"));
        }
        Ok(session)
    }

    fn spawn_bridge(&self) -> Result<SpawnedBridge, String> {
        let bridge_path = self.ensure_bridge_script()?.to_path_buf();
        let mut child = self.spawn_command(&bridge_path).spawn().map_err(|err| {
            format!(
                "Failed to spawn browser bridge: {err}. Ensure Python and Playwright are installed."
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or("Failed to capture bridge stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Failed to capture bridge stdout".to_string())?;
        Ok(SpawnedBridge {
            child,
            stdin,
            stdout,
        })
    }

    fn remove_if_same(&self, session: &Arc<BridgeSession>) {
        self.sessions
            .remove_if(&session.key, |_, existing| Arc::ptr_eq(existing, session));
    }

    fn retire(&self, session: &Arc<BridgeSession>, reason: &str) {
        self.remove_if_same(session);
        session.teardown();
        debug!(session_key = %session.key, reason, "Browser bridge session retired");
    }

    /// Free a slot by killing the least recently used session that is not
    /// currently in flight: better than refusing the whole feature.
    fn evict_for_capacity(&self) -> Result<(), String> {
        let candidates = self
            .sessions
            .iter()
            .filter(|entry| entry.value().in_flight.load(Ordering::Acquire) == 0)
            .map(|entry| {
                (
                    entry.last_active_ms.load(Ordering::Relaxed),
                    entry.key().clone(),
                )
            })
            .collect::<Vec<_>>();
        let victim = pick_evict_candidate(candidates, elapsed_millis(), ACTIVE_PROTECT);
        let Some(key) = victim else {
            return Err(format!(
                "Maximum browser sessions reached ({}) and all are active",
                self.config.max_sessions
            ));
        };
        if let Some((_, session)) = self.sessions.remove(&key) {
            session.teardown();
            warn!(
                session_key = %key,
                limit = self.config.max_sessions,
                "Browser session evicted to free a slot"
            );
        }
        Ok(())
    }

    /// Kill sessions idle past `idle_timeout_secs`. Runs on traffic and on the
    /// reaper timer, so a thread that never issues another command cannot keep
    /// a browser and its slot alive for the life of the process.
    pub fn sweep_expired(&self) -> usize {
        if self.config.idle_timeout_secs == 0 {
            return 0;
        }
        sweep_at(
            &self.sessions,
            elapsed_millis(),
            Duration::from_secs(self.config.idle_timeout_secs),
        )
    }

    /// One timer task per process: without it an abandoned session only dies
    /// when some later, unrelated browser call happens to sweep.
    fn ensure_reaper(&self) {
        if self.reaper_started.swap(1, Ordering::AcqRel) == 1 {
            return;
        }
        if tokio::runtime::Handle::try_current().is_err() {
            // No runtime (plain unit test, sync caller): the on-traffic sweep
            // still covers reclamation, so do not claim a task that never ran.
            self.reaper_started.store(0, Ordering::Release);
            return;
        }
        let sweeper = BridgeSweeper {
            sessions: Arc::clone(&self.sessions),
            idle_timeout_secs: self.config.idle_timeout_secs,
        };
        tokio::task::spawn(async move { sweeper.run().await });
    }

    fn ensure_bridge_script(&self) -> Result<&PathBuf, String> {
        if let Some(path) = self.bridge_path.get() {
            return Ok(path);
        }
        // Per-process slot: several instances on one machine must not overwrite
        // the embedded script while another one is mid-spawn.
        let dir = std::env::temp_dir()
            .join("wunder_browser")
            .join(format!("pid-{}", std::process::id()));
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
        let launch_mode = self.config.launch_mode.trim();
        if !launch_mode.is_empty() {
            command.arg("--mode").arg(launch_mode);
        }
        if let Some(channel) = &self.config.channel {
            command.arg("--channel").arg(channel);
        }
        if let Some(path) = &self.config.executable_path {
            command.arg("--executable-path").arg(path);
        }
        if let Some(endpoint) = &self.config.attach_endpoint {
            command.arg("--attach-endpoint").arg(endpoint);
        }
        // The URL policy is enforced inside the browser so that redirects,
        // subresources and script-driven requests cannot route around it.
        if self.config.allow_private_network {
            command.arg("--allow-private-network");
        }
        if !self.config.deny_file_scheme {
            command.arg("--allow-file-scheme");
        }
        for host in &self.config.hostname_allowlist {
            command.arg("--allow-host").arg(host);
        }
        for arg in self.launch_args() {
            command.arg(format!("--launch-arg={arg}"));
        }
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::null());
        apply_browser_env(&mut command, &self.config);
        command_utils::apply_platform_spawn_options_std(&mut command);
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

/// Wall-clock budget for a launch: the configured command timeout plus grace for
/// interpreter startup and browser spawn.
pub fn launch_deadline(timeout_secs: u64) -> Duration {
    Duration::from_secs(timeout_secs.max(1)).saturating_add(LAUNCH_GRACE)
}

/// Oldest session that has been quiet long enough to be safe to steal. Kept as
/// a pure function of `(age, now)` so the choice is testable without a clock.
fn pick_evict_candidate(
    mut candidates: Vec<(u64, String)>,
    now_ms: u64,
    protect: Duration,
) -> Option<String> {
    candidates.sort();
    candidates
        .into_iter()
        .find_map(move |(last_active_ms, key)| {
            (Duration::from_millis(now_ms.saturating_sub(last_active_ms)) >= protect).then_some(key)
        })
}

/// Reap sessions quieter than `limit`, measured against `now_ms` so the choice
/// is testable without a wall clock.
fn sweep_at(sessions: &DashMap<String, Arc<BridgeSession>>, now_ms: u64, limit: Duration) -> usize {
    // Collect first: locking a shard while iterating the same shard deadlocks.
    let expired = sessions
        .iter()
        .filter(|entry| {
            let session = entry.value();
            session.idle_for(now_ms) > limit && session.in_flight.load(Ordering::Acquire) == 0
        })
        .map(|entry| entry.key().clone())
        .collect::<Vec<_>>();
    let mut removed = 0usize;
    for key in expired {
        // Re-check under the write lock so a just-reused session wins.
        if let Some((_, session)) =
            sessions.remove_if(&key, |_, existing| existing.idle_for(now_ms) > limit)
        {
            session.teardown();
            removed += 1;
            info!(session_key = %key, "Browser bridge session reclaimed after idle timeout");
        }
    }
    removed
}

struct BridgeSweeper {
    sessions: Arc<DashMap<String, Arc<BridgeSession>>>,
    idle_timeout_secs: u64,
}

impl BridgeSweeper {
    async fn run(self) {
        if self.idle_timeout_secs == 0 {
            return;
        }
        let limit = Duration::from_secs(self.idle_timeout_secs);
        let mut ticker = tokio::time::interval(REAPER_PERIOD);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            sweep_at(&self.sessions, elapsed_millis(), limit);
        }
    }
}

fn short_slot(session_key: &str) -> String {
    let tail = session_key.rsplit(':').next().unwrap_or(session_key);
    let slot = tail.chars().take(12).collect::<String>();
    if slot.is_empty() {
        "session".to_string()
    } else {
        slot
    }
}

/// Blocking bridge I/O for one session. Exits by killing and reaping the child,
/// which is also what unblocks this thread when the page has wedged.
fn pump_loop(
    mut stdin: ChildStdin,
    stdout: ChildStdout,
    mut cmd_rx: mpsc::Receiver<Command>,
    startup_tx: oneshot::Sender<Result<(), String>>,
    child: Arc<Mutex<Option<Child>>>,
) {
    let mut reader = BufReader::new(stdout);
    let startup = read_ready(&mut reader);
    let ready = startup.is_ok();
    let _ = startup_tx.send(startup);
    // A failed handshake must still reach the awaiting caller as a launch
    // error, not as a generic command timeout.
    if ready {
        loop {
            let Some(command) = cmd_rx.blocking_recv() else {
                break;
            };
            let outcome =
                write_command(&mut stdin, &command.line).and_then(|_| read_reply(&mut reader));
            match outcome {
                Ok(response) => {
                    if command.reply.send(response).is_err() {
                        // Caller gave up (deadline or cancellation). Keep
                        // pumping: stopping here would desync the serialized
                        // protocol and poison the next command.
                        continue;
                    }
                }
                Err(err) => {
                    let _ = command.reply.send(BridgeResponse::failure(err));
                    break;
                }
            }
        }
    }

    // Last word on the process: kill if something else did not, then reap.
    if let Ok(mut guard) = child.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn read_ready(reader: &mut BufReader<ChildStdout>) -> Result<(), String> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|err| format!("Bridge failed to start: {err}"))?;
    if line.trim().is_empty() {
        return Err(
            "Browser bridge exited without ready signal. Check Python/Playwright installation."
                .to_string(),
        );
    }
    let ready: BridgeResponse = serde_json::from_str(line.trim())
        .map_err(|err| format!("Bridge startup failed: {err}. Output: {}", line.trim()))?;
    if ready.success {
        return Ok(());
    }
    let error = ready
        .error
        .unwrap_or_else(|| "Unknown startup error".to_string());
    let traceback = ready
        .traceback
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("\n{value}"))
        .unwrap_or_default();
    Err(format!(
        "Browser bridge failed to start: {error}{traceback}"
    ))
}

fn write_command(stdin: &mut ChildStdin, line: &str) -> Result<(), String> {
    stdin
        .write_all(line.as_bytes())
        .map_err(|err| format!("Failed to write to browser stdin: {err}"))?;
    stdin
        .write_all(b"\n")
        .map_err(|err| format!("Failed to write command newline: {err}"))?;
    stdin
        .flush()
        .map_err(|err| format!("Failed to flush browser stdin: {err}"))
}

fn read_reply(reader: &mut BufReader<ChildStdout>) -> Result<BridgeResponse, String> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|err| format!("Failed to read browser stdout: {err}"))?;
    if line.trim().is_empty() {
        return Err("Browser bridge closed unexpectedly".to_string());
    }
    serde_json::from_str(line.trim())
        .map_err(|err| format!("Failed to parse browser response: {err}"))
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

#[cfg(test)]
mod tests {
    use super::{
        elapsed_millis, launch_deadline, pick_evict_candidate, short_slot, sweep_at,
        BridgeResponse, BridgeSession, BrowserBridge, Command, ACTIVE_PROTECT, LAUNCH_GRACE,
        STATE_LAUNCHING, STATE_READY,
    };
    use crate::config::Config;
    use crate::services::browser::config::effective_browser_config;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[test]
    fn launch_deadline_covers_grace_and_stays_usable() {
        assert_eq!(
            launch_deadline(60),
            Duration::from_secs(60).saturating_add(LAUNCH_GRACE)
        );
        // A zero or garbage timeout still leaves a bounded, usable window.
        assert!(launch_deadline(0) >= LAUNCH_GRACE);
        assert!(launch_deadline(u128::from(u32::MAX) as u64).as_secs() > LAUNCH_GRACE.as_secs());
    }

    #[test]
    fn elapsed_clock_is_monotonic() {
        let first = elapsed_millis();
        let second = elapsed_millis();
        assert!(second >= first);
    }

    #[test]
    fn thread_names_stay_short_and_non_empty() {
        assert_eq!(short_slot("user:session:abc"), "abc");
        assert_eq!(
            short_slot("u:web-fetch:123:deadbeefcafe1234"),
            "deadbeefcafe"
        );
        assert_eq!(short_slot(":"), "session");
    }

    #[test]
    fn capacity_eviction_keeps_a_protection_window() {
        // Evicting a session that just answered would steal work mid-flight.
        assert!(ACTIVE_PROTECT >= Duration::from_secs(1));
    }

    #[test]
    fn bridge_failure_keeps_its_message() {
        let response = BridgeResponse::failure("boom");
        assert!(!response.success);
        assert_eq!(response.error.as_deref(), Some("boom"));
    }

    fn bridge_with(idle_timeout_secs: u64, max_sessions: usize) -> BrowserBridge {
        let mut config = Config::default();
        config.browser.enabled = true;
        config.tools.browser.enabled = true;
        let mut effective = effective_browser_config(&config);
        effective.idle_timeout_secs = idle_timeout_secs;
        effective.max_sessions = max_sessions;
        BrowserBridge::new(effective)
    }

    /// A session that answers nothing: stands in for a bridge wedged inside a
    /// page, without depending on Python or a browser in the test environment.
    fn silent_session(key: &str, idle_ms: u64) -> (Arc<BridgeSession>, mpsc::Receiver<Command>) {
        let (tx, rx) = mpsc::channel::<Command>(1);
        let session = Arc::new(BridgeSession {
            key: key.to_string(),
            tx,
            child: Arc::new(Mutex::new(None)),
            state: AtomicU8::new(STATE_READY),
            last_active_ms: AtomicU64::new(elapsed_millis().saturating_sub(idle_ms)),
            in_flight: Arc::new(AtomicU64::new(0)),
            startup_rx: Mutex::new(None),
        });
        (session, rx)
    }

    #[tokio::test]
    async fn wedged_bridge_is_given_up_on_and_loses_its_slot() {
        let bridge = bridge_with(300, 5);
        let (session, _rx) = silent_session("u1:s1", 0);
        bridge
            .sessions
            .insert("u1:s1".to_string(), Arc::clone(&session));
        let outcome = bridge
            .send_command(
                "u1:s1",
                json!({ "action": "tabs" }),
                Duration::from_millis(120),
            )
            .await;
        let err = outcome.expect_err("a bridge that never replies must fail the call");
        assert!(
            err.contains("timed out"),
            "deadline must be reported, got: {err}"
        );
        // The poisoned session must not keep holding a slot.
        assert_eq!(bridge.session_count(), 0);
        assert!(!session.is_ready());
    }

    #[tokio::test]
    async fn full_command_queue_is_bounded_without_killing_active_session() {
        let bridge = bridge_with(300, 5);
        let (session, _rx) = silent_session("sample:thread", 0);
        let (reply, _) = tokio::sync::oneshot::channel();
        session
            .tx
            .try_send(Command {
                line: "{}".to_string(),
                reply,
            })
            .unwrap();
        bridge.sessions.insert(session.key.clone(), session.clone());
        let error = bridge
            .send_command(
                &session.key,
                json!({"action":"tabs"}),
                Duration::from_millis(30),
            )
            .await
            .unwrap_err();
        assert!(error.contains("not executed"));
        assert!(session.is_ready());
        assert_eq!(bridge.session_count(), 1);
        assert_eq!(session.in_flight.load(Ordering::Acquire), 0);
    }

    #[test]
    fn idle_sessions_are_reaped_and_recent_ones_survive() {
        let bridge = bridge_with(300, 5);
        for (key, last_active_ms) in [("u1:old", 1_000u64), ("u2:edge", 5_500), ("u1:new", 9_000)] {
            let (session, _rx) = silent_session(key, 0);
            session
                .last_active_ms
                .store(last_active_ms, Ordering::Relaxed);
            bridge.sessions.insert(key.to_string(), session);
        }
        // now = 10s, window = 5s: only the 9s-quiet session goes, the 4.5s and
        // 1s-quiet ones stay.
        assert_eq!(
            sweep_at(&bridge.sessions, 10_000, Duration::from_secs(5)),
            1
        );
        assert_eq!(
            bridge.session_keys(),
            vec!["u1:new".to_string(), "u2:edge".to_string()]
        );
    }

    #[test]
    fn a_long_running_command_is_never_cut_off_by_reclamation() {
        let bridge = bridge_with(300, 2);
        let (busy, _rx) = silent_session("u1:busy", 0);
        busy.last_active_ms.store(0, Ordering::Relaxed);
        busy.in_flight.store(1, Ordering::Release);
        bridge
            .sessions
            .insert("u1:busy".to_string(), Arc::clone(&busy));
        // Silent since the beginning of the process, yet mid-command: kept.
        assert_eq!(
            sweep_at(&bridge.sessions, 10_000_000, Duration::from_secs(5)),
            0
        );
        assert!(bridge.evict_for_capacity().is_err());
        // Once the caller lets go (guard drop, including cancellation) it is
        // reclaimable again.
        busy.in_flight.store(0, Ordering::Release);
        assert_eq!(
            sweep_at(&bridge.sessions, 10_000_000, Duration::from_secs(5)),
            1
        );
        assert_eq!(bridge.session_count(), 0);
    }

    #[test]
    fn zero_idle_window_disables_reclamation() {
        let bridge = bridge_with(0, 5);
        let (session, _rx) = silent_session("u1:ancient", 0);
        session.last_active_ms.store(0, Ordering::Relaxed);
        bridge.sessions.insert("u1:ancient".to_string(), session);
        assert_eq!(bridge.sweep_expired(), 0);
        assert_eq!(bridge.session_count(), 1);
    }

    #[test]
    fn eviction_chooses_the_oldest_quiet_session() {
        let now = 100_000;
        let protect = Duration::from_secs(5);
        assert_eq!(
            pick_evict_candidate(
                vec![
                    (now, "u1:fresh".to_string()),
                    (70_000, "u2:middle".to_string()),
                    (10_000, "u3:oldest".to_string()),
                ],
                now,
                protect
            ),
            Some("u3:oldest".to_string())
        );
        // Nothing has been quiet long enough: refuse rather than steal live work.
        assert_eq!(
            pick_evict_candidate(
                vec![
                    (now - 1_000, "u1:a".to_string()),
                    (now - 2_000, "u2:b".to_string())
                ],
                now,
                protect
            ),
            None
        );
        assert_eq!(pick_evict_candidate(Vec::new(), now, protect), None);
    }

    #[tokio::test]
    async fn all_active_sessions_are_protected_from_eviction() {
        let bridge = bridge_with(300, 1);
        let (session, _rx) = silent_session("u1:fresh", 0);
        bridge.sessions.insert("u1:fresh".to_string(), session);
        assert_eq!(bridge.sweep_expired(), 0);
        assert!(bridge.evict_for_capacity().is_err());
        assert_eq!(bridge.session_count(), 1);
    }

    #[tokio::test]
    async fn status_keys_are_scoped_to_the_caller() {
        let bridge = bridge_with(300, 5);
        for key in ["u1:s1", "u1:s2", "u2:s1"] {
            let (session, _rx) = silent_session(key, 0);
            bridge.sessions.insert(key.to_string(), session);
        }
        assert_eq!(
            bridge.session_keys_for_owner("u1"),
            vec!["u1:s1".to_string(), "u1:s2".to_string()]
        );
        assert_eq!(bridge.session_keys().len(), 3);
    }

    #[tokio::test]
    async fn launching_session_is_not_reused_as_ready() {
        let bridge = bridge_with(300, 5);
        let (session, _rx) = silent_session("u1:launching", 0);
        session.state.store(STATE_LAUNCHING, Ordering::Release);
        bridge
            .sessions
            .insert("u1:launching".to_string(), Arc::clone(&session));
        // A launch that never publishes readiness must not be handed to the
        // caller as a usable session, and must not stay in the table.
        let attach = bridge.attach(&session, Duration::from_millis(60)).await;
        match attach {
            Ok(_) => panic!("a launch that never published readiness is not usable"),
            Err(err) => assert!(err.contains("being prepared"), "got: {err}"),
        }
        assert_eq!(bridge.session_count(), 0);
        assert!(!session.is_ready());
    }
}
