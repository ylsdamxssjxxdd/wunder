use super::config::EffectiveBrowserConfig;
use crate::core::command_utils;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
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
/// A sandbox-hosted bridge has to survive the same page stalls a local one
/// does; the ceiling only exists so a dead sandbox cannot pin the pump.
const REMOTE_COMMAND_GRACE: Duration = Duration::from_secs(60);
const REMOTE_CREATE_TIMEOUT: Duration = Duration::from_secs(180);
const REMOTE_CLOSE_TIMEOUT: Duration = Duration::from_secs(3);
/// A session this recently active is never stolen by the capacity eviction.
const ACTIVE_PROTECT: Duration = Duration::from_secs(5);
/// Session key prefix for browsers the warm pool owns. They stay out of the
/// session table until a real session adopts one, so they neither count against
/// `max_sessions` nor look reclaimable to the idle sweep.
const WARM_KEY_PREFIX: &str = "warm-pool";

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
    /// Mutable because a warm-pool browser is launched before its owner exists
    /// and is renamed when a session adopts it.
    key: RwLock<String>,
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

    fn key(&self) -> String {
        self.key
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    /// Hand a pooled browser to its owner. Callers rename under the session
    /// table lock, so the session is never reachable under two keys at once.
    fn set_key(&self, key: &str) {
        if let Ok(mut guard) = self.key.write() {
            *guard = key.to_string();
        }
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

/// Everything needed to launch a bridge process. Owned (not borrowed) so a
/// warm-pool launch can run on a plain thread while the caller keeps serving.
#[derive(Clone)]
struct LaunchSpec {
    config: EffectiveBrowserConfig,
    bridge_path: PathBuf,
}

/// Longest a failed warm launch stops the pool from retrying. A host without
/// Playwright must not retry every tick forever.
const WARM_RETRY_CEILING: Duration = Duration::from_secs(900);

/// A compose stack starts the sandbox only after the API reports healthy, so
/// the first warm launch can beat the sandbox listener by a second or two.
/// These attempts cover that window without turning it into a warning.
const WARM_PIPELINE_ATTEMPTS: u32 = 4;
const WARM_PIPELINE_BACKOFF: Duration = Duration::from_millis(1200);
/// Retry gap while the sandbox stays unreachable. Much shorter than
/// `WARM_RETRY_CEILING`: an unreachable sandbox usually comes up on its own,
/// while a broken local launch will not.
const WARM_PIPELINE_RETRY: Duration = Duration::from_secs(30);

/// Browsers kept hot so the first session of a turn does not wait out the
/// Chromium launch. Every field is owned so the background maintainer and the
/// launch threads can hold their own handle.
#[derive(Clone)]
struct WarmPool {
    spec: LaunchSpec,
    ready: Arc<Mutex<Vec<Arc<BridgeSession>>>>,
    launching: Arc<AtomicUsize>,
    /// Consecutive launch failures, which drive the retry backoff.
    failures: Arc<AtomicUsize>,
    /// Elapsed-ms stamp before which no launch is attempted.
    retry_after_ms: Arc<AtomicU64>,
    /// Set once an unreachable sandbox has been reported, so a stack that comes
    /// up in the wrong order logs one line instead of one per retry.
    pipeline_warned: Arc<AtomicBool>,
    target: usize,
}

/// Tell "the sandbox is not listening yet" apart from "the browser cannot be
/// launched here". Only the first one deserves patience and a quiet log.
fn is_pipeline_unready(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    [
        "did not accept the bridge",
        "error sending request",
        "connection refused",
        "actively refused",
        "os error 10061",
        "dns error",
        "failed to lookup address",
        "connection timed out",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

impl WarmPool {
    fn warm_slot() -> u32 {
        static SLOT: AtomicU32 = AtomicU32::new(0);
        SLOT.fetch_add(1, Ordering::Relaxed)
    }

    /// Launch browsers until the pool holds `target` ready ones plus whatever
    /// is already in flight. Launching is the slow part, so it happens on its
    /// own thread and only readiness is awaited there.
    fn top_up(&self) -> usize {
        if elapsed_millis() < self.retry_after_ms.load(Ordering::Acquire) {
            return 0;
        }
        let in_flight = self.launching.load(Ordering::Acquire);
        let ready = self
            .ready
            .lock()
            .map(|guard| guard.len())
            .unwrap_or_else(|poisoned| poisoned.into_inner().len());
        let missing = self.target.saturating_sub(ready + in_flight);
        let mut launched = 0;
        for _ in 0..missing {
            self.launching.fetch_add(1, Ordering::AcqRel);
            let pool = self.clone();
            let slot = Self::warm_slot();
            let spawned = std::thread::Builder::new()
                .name(format!("browser-warm-{slot}"))
                .spawn(move || pool.launch_and_park(slot));
            if let Err(err) = spawned {
                self.launching.fetch_sub(1, Ordering::AcqRel);
                warn!(error = %err, "Failed to start warm browser launch thread");
                break;
            }
            launched += 1;
        }
        launched
    }

    /// Launch one pooled browser and park it once it reports ready. Runs on a
    /// plain thread: `blocking_recv` panics inside an async context.
    fn launch_and_park(&self, slot: u32) {
        let key = format!("{WARM_KEY_PREFIX}:{slot}");
        let mut attempt = 0;
        let outcome = loop {
            let outcome = start_session(&key, &self.spec).and_then(|session| {
                let startup = session
                    .startup_rx
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take();
                let ready = match startup {
                    Some(receiver) => receiver.blocking_recv().unwrap_or_else(|_| {
                        Err("bridge pump ended before reporting readiness".to_string())
                    }),
                    None => Err("warm browser readiness was claimed by another caller".to_string()),
                };
                ready.map(|()| session)
            });
            match outcome {
                Ok(session) => break Ok(session),
                Err(err) if attempt + 1 < WARM_PIPELINE_ATTEMPTS && is_pipeline_unready(&err) => {
                    attempt += 1;
                    debug!(
                        slot,
                        attempt,
                        error = %err,
                        "Sandbox not listening for the warm browser yet"
                    );
                    std::thread::sleep(WARM_PIPELINE_BACKOFF * attempt);
                }
                Err(err) => break Err(err),
            }
        };
        match outcome {
            Ok(session) => {
                session.state.store(STATE_READY, Ordering::Release);
                session.touch();
                if let Ok(mut guard) = self.ready.lock() {
                    guard.push(session);
                }
                self.failures.store(0, Ordering::Release);
                self.retry_after_ms.store(0, Ordering::Release);
                self.pipeline_warned.store(false, Ordering::Release);
                info!(slot, "Warm browser ready and parked");
            }
            Err(err) if is_pipeline_unready(&err) => {
                self.retry_after_ms.store(
                    elapsed_millis() + WARM_PIPELINE_RETRY.as_millis() as u64,
                    Ordering::Release,
                );
                if self.pipeline_warned.swap(true, Ordering::AcqRel) {
                    debug!(slot, error = %err, "Sandbox still not accepting the warm pool");
                } else {
                    warn!(
                        slot,
                        error = %err,
                        "Sandbox has not accepted the warm pool yet; retrying every {}s",
                        WARM_PIPELINE_RETRY.as_secs()
                    );
                }
            }
            Err(err) => {
                warn!(slot, error = %err, "Warm browser did not become ready");
                let failures = self.failures.fetch_add(1, Ordering::AcqRel) + 1;
                let backoff = REAPER_PERIOD
                    .saturating_mul(1u32 << failures.min(5))
                    .min(WARM_RETRY_CEILING);
                self.retry_after_ms.store(
                    elapsed_millis() + backoff.as_millis() as u64,
                    Ordering::Release,
                );
            }
        }
        self.launching.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct BrowserBridge {
    sessions: Arc<DashMap<String, Arc<BridgeSession>>>,
    warm: Arc<Mutex<Vec<Arc<BridgeSession>>>>,
    warm_launching: Arc<AtomicUsize>,
    warm_failures: Arc<AtomicUsize>,
    warm_retry_after_ms: Arc<AtomicU64>,
    warm_pipeline_warned: Arc<AtomicBool>,
    config: EffectiveBrowserConfig,
    bridge_path: OnceLock<PathBuf>,
    reaper_started: AtomicU8,
}

impl BrowserBridge {
    pub fn new(config: EffectiveBrowserConfig) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            warm: Arc::new(Mutex::new(Vec::new())),
            warm_launching: Arc::new(AtomicUsize::new(0)),
            warm_failures: Arc::new(AtomicUsize::new(0)),
            warm_retry_after_ms: Arc::new(AtomicU64::new(0)),
            warm_pipeline_warned: Arc::new(AtomicBool::new(false)),
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

    /// Launch the pool without waiting for the first browser call. Server
    /// bootstrap calls this so the Chromium cost is paid while nobody is
    /// waiting; returns how many launches were started.
    pub fn prewarm(&self) -> usize {
        self.ensure_reaper();
        self.top_up()
    }

    /// `(ready, launching)` for the warm pool, so a slow first call can be told
    /// apart from a pool that never filled.
    pub fn warm_pool_state(&self) -> (usize, usize) {
        let count = |guard: &Vec<Arc<BridgeSession>>| {
            guard
                .iter()
                .filter(|session| session.is_ready() && !session.tx.is_closed())
                .count()
        };
        let ready = match self.warm.lock() {
            Ok(guard) => count(&guard),
            Err(poisoned) => count(&poisoned.into_inner()),
        };
        (ready, self.warm_launching.load(Ordering::Acquire))
    }

    fn warm_target(&self) -> usize {
        // Clamped again here: a pooled browser is useless without a free slot,
        // and `EffectiveBrowserConfig` can be assembled outside the normal path.
        self.config.prewarm_sessions.min(self.config.max_sessions)
    }

    fn warm_pool(&self) -> Option<WarmPool> {
        if self.warm_target() == 0 {
            return None;
        }
        let spec = match self.launch_spec() {
            Ok(spec) => spec,
            Err(err) => {
                warn!(error = %err, "Warm browser pool unavailable");
                return None;
            }
        };
        Some(WarmPool {
            spec,
            ready: Arc::clone(&self.warm),
            launching: Arc::clone(&self.warm_launching),
            failures: Arc::clone(&self.warm_failures),
            retry_after_ms: Arc::clone(&self.warm_retry_after_ms),
            pipeline_warned: Arc::clone(&self.warm_pipeline_warned),
            target: self.warm_target(),
        })
    }

    fn top_up(&self) -> usize {
        self.warm_pool().map(|pool| pool.top_up()).unwrap_or(0)
    }

    /// Take a ready browser out of the pool, dropping any that died or never
    /// finished launching. Dropping lets the pump reap the child.
    fn take_warm(&self) -> Option<Arc<BridgeSession>> {
        let mut guard = self
            .warm
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while let Some(session) = guard.pop() {
            // A closed command channel means the bridge process is gone, which
            // the readiness flag alone cannot tell.
            if session.is_ready() && !session.tx.is_closed() {
                return Some(session);
            }
        }
        None
    }

    fn launch_spec(&self) -> Result<LaunchSpec, String> {
        let bridge_path = self.ensure_bridge_script()?.to_path_buf();
        Ok(LaunchSpec {
            config: self.config.clone(),
            bridge_path,
        })
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

        // A pooled browser has already paid the Chromium launch, so adopt one
        // before falling back to a cold start.
        if let Some(warm) = self.take_warm() {
            warm.set_key(session_key);
            let displaced = match self.sessions.entry(session_key.to_string()) {
                dashmap::mapref::entry::Entry::Occupied(entry) => Some(Arc::clone(entry.get())),
                dashmap::mapref::entry::Entry::Vacant(vacant) => {
                    vacant.insert(Arc::clone(&warm));
                    None
                }
            };
            if let Some(existing) = displaced {
                // Another call won the race: this browser has no slot, so let
                // go of it and attach to theirs instead.
                drop(warm);
                return self.attach(&existing, deadline).await;
            }
            // Keep the pool filled so the next session does not wait either.
            self.top_up();
            warm.touch();
            return Ok(warm);
        }

        let session = match self.launch(session_key)? {
            LaunchOutcome::Owned(session) => session,
            LaunchOutcome::Adopted(existing) => return self.attach(&existing, deadline).await,
        };
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
                session.key()
            ));
        }
        self.remove_if_same(session);
        Err(format!(
            "Browser bridge session '{}' is no longer alive; retry the command.",
            session.key()
        ))
    }

    /// Start a browser for `session_key` and register it, or attach to the
    /// session a concurrent caller already registered under that key.
    fn launch(&self, session_key: &str) -> Result<LaunchOutcome, String> {
        let spec = self.launch_spec()?;
        let session = start_session(session_key, &spec)?;
        let displaced = match self.sessions.entry(session_key.to_string()) {
            dashmap::mapref::entry::Entry::Occupied(entry) => Some(Arc::clone(entry.get())),
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                vacant.insert(Arc::clone(&session));
                None
            }
        };
        match displaced {
            // Another call registered first, so this browser has no slot: the
            // winner is returned and ours is reaped when the last Arc drops.
            Some(existing) => Ok(LaunchOutcome::Adopted(existing)),
            None => Ok(LaunchOutcome::Owned(session)),
        }
    }
}

/// What a caller got from starting a browser: either the session it now owns,
/// or the one a racing caller registered first.
enum LaunchOutcome {
    Owned(Arc<BridgeSession>),
    Adopted(Arc<BridgeSession>),
}

/// Spawn a bridge process and its pump thread without registering the session.
/// Shared by the on-demand path and the warm pool, which registers later.
fn start_session(session_key: &str, spec: &LaunchSpec) -> Result<Arc<BridgeSession>, String> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(COMMAND_QUEUE_DEPTH);
    let (startup_tx, startup_rx) = oneshot::channel::<Result<(), String>>();
    let session = Arc::new(BridgeSession {
        key: RwLock::new(session_key.to_string()),
        tx: cmd_tx,
        child: Arc::new(Mutex::new(None)),
        state: AtomicU8::new(STATE_LAUNCHING),
        last_active_ms: AtomicU64::new(elapsed_millis()),
        in_flight: Arc::new(AtomicU64::new(0)),
        startup_rx: Mutex::new(Some(startup_rx)),
    });

    // Sandbox deployment: the bridge process belongs to the sandbox container,
    // so this process only keeps the session table and the HTTP transport.
    let (io, local_child): (Box<dyn BridgeExchange>, Option<Child>) =
        match spec.config.sandbox_endpoint.as_deref() {
            Some(endpoint) => {
                let spawned = spawn_remote_bridge(endpoint, session_key, &spec.config)?;
                let timeout = Duration::from_secs(spec.config.timeout_secs)
                    .saturating_add(REMOTE_COMMAND_GRACE);
                (
                    Box::new(RemoteExchange::new(
                        endpoint,
                        spawned.bridge_id,
                        spawned.ready_line,
                        timeout,
                    )),
                    None,
                )
            }
            None => {
                let spawned = spawn_bridge(&spec.config, &spec.bridge_path)?;
                (
                    Box::new(LocalExchange::new(spawned.stdin, spawned.stdout)),
                    Some(spawned.child),
                )
            }
        };
    {
        let mut guard = session
            .child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = local_child;
    }

    let pump_child = Arc::clone(&session.child);
    let pump_slot = short_slot(session_key);
    let spawned_pump = std::thread::Builder::new()
        .name(format!("browser-bridge-pump-{pump_slot}"))
        .spawn(move || pump_loop(io, cmd_rx, startup_tx, pump_child));
    if let Err(err) = spawned_pump {
        // No pump means nobody will ever reap the child, so kill it here.
        session.teardown();
        return Err(format!("Failed to start browser bridge pump: {err}"));
    }
    Ok(session)
}

fn spawn_bridge(
    config: &EffectiveBrowserConfig,
    bridge_path: &Path,
) -> Result<SpawnedBridge, String> {
    let mut child = build_spawn_command(config, bridge_path)
        .spawn()
        .map_err(|err| {
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

/// A bridge running inside the sandbox, plus the readiness line it opened with.
struct SpawnedRemoteBridge {
    bridge_id: String,
    ready_line: String,
}

/// Ask the sandbox to start one bridge and wait for its readiness line.
///
/// The sandbox blocks until Python + Chromium report ready, so this call owns
/// the whole cold start. A bridge id is unique per launch: a retired session
/// must not collide with the process it just gave up.
fn spawn_remote_bridge(
    endpoint: &str,
    session_key: &str,
    config: &EffectiveBrowserConfig,
) -> Result<SpawnedRemoteBridge, String> {
    static LAUNCH_SEQ: AtomicU64 = AtomicU64::new(0);
    let bridge_id = format!(
        "{}-{}-{}",
        short_slot(session_key),
        std::process::id(),
        LAUNCH_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    let payload = serde_json::json!({
        "bridge_id": bridge_id,
        "spec": bridge_spec(config),
    });
    let body = remote_post(
        &format!("{}/browser/bridges", endpoint.trim_end_matches('/')),
        &payload,
        REMOTE_CREATE_TIMEOUT,
    )
    .map_err(|err| {
        format!(
            "Browser runs in the sandbox but {endpoint} did not accept the bridge: {err}. \
             Check that the sandbox container is running and WUNDER_SANDBOX_ENDPOINT is reachable."
        )
    })?;
    if body.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("sandbox rejected the browser bridge");
        return Err(format!(
            "Browser bridge failed to start in the sandbox: {error}"
        ));
    }
    let ready_line = body
        .get("line")
        .and_then(Value::as_str)
        .ok_or_else(|| "Browser sandbox returned no readiness line".to_string())?
        .to_string();
    Ok(SpawnedRemoteBridge {
        bridge_id,
        ready_line,
    })
}

/// The launch parameters a sandbox-hosted bridge needs. Everything is resolved
/// here, so the sandbox never reads the server's browser config.
fn bridge_spec(config: &EffectiveBrowserConfig) -> Value {
    serde_json::json!({
        "python_path": config.python_path,
        "browsers_path": config.browsers_path,
        "headless": config.headless,
        "width": config.viewport_width,
        "height": config.viewport_height,
        "max_tabs": config.max_tabs_per_session,
        "timeout_secs": config.timeout_secs,
        "idle_timeout_secs": config.idle_timeout_secs,
        "mode": config.launch_mode,
        "channel": config.channel,
        "executable_path": config.executable_path,
        "attach_endpoint": config.attach_endpoint,
        "allow_private_network": config.allow_private_network,
        "deny_file_scheme": config.deny_file_scheme,
        "hostname_allowlist": config.hostname_allowlist,
        "launch_args": launch_args(config),
        // The sandbox has its own filesystem layout; the server's downloads
        // root would point at a directory that does not exist there.
        "download_root": Value::Null,
    })
}

fn remote_http() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
}

fn remote_post(url: &str, payload: &Value, timeout: Duration) -> Result<Value, String> {
    let response = remote_http()
        .post(url)
        .timeout(timeout)
        .json(payload)
        .send()
        .map_err(|err| err.to_string())?;
    let status = response.status();
    let text = response
        .text()
        .map_err(|err| format!("sandbox response read failed: {err}"))?;
    if !status.is_success() {
        return Err(format!("sandbox returned HTTP {status}: {}", text.trim()));
    }
    serde_json::from_str(text.trim()).map_err(|err| format!("sandbox response was not JSON: {err}"))
}

impl BrowserBridge {
    fn remove_if_same(&self, session: &Arc<BridgeSession>) {
        self.sessions
            .remove_if(&session.key(), |_, existing| Arc::ptr_eq(existing, session));
    }

    fn retire(&self, session: &Arc<BridgeSession>, reason: &str) {
        self.remove_if_same(session);
        session.teardown();
        debug!(session_key = %session.key(), reason, "Browser bridge session retired");
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
    /// when some later, unrelated browser call happens to sweep. The same tick
    /// refills the warm pool, which is what keeps it warm across a long idle.
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
        let janitor = BridgeJanitor {
            sessions: Arc::clone(&self.sessions),
            idle_timeout_secs: self.config.idle_timeout_secs,
            warm: self.warm_pool(),
        };
        tokio::task::spawn(async move { janitor.run().await });
    }

    fn ensure_bridge_script(&self) -> Result<&PathBuf, String> {
        if let Some(path) = self.bridge_path.get() {
            return Ok(path);
        }
        let path = write_bridge_script()?;
        debug!(path = %path.display(), "Wrote browser bridge script");
        let _ = self.bridge_path.set(path);
        Ok(self.bridge_path.get().expect("bridge script path missing"))
    }
}

/// Write the embedded bridge script to a per-process temp path.
///
/// Public within the crate because the sandbox host needs the same script: in
/// `deployment = sandbox` the bridge process lives there, not here.
pub(crate) fn write_bridge_script() -> Result<PathBuf, String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Content-addressed name: a second instance reuses the file instead of
    // truncating a script another process may be executing right now. Windows
    // in particular refuses to write a file in use, and the failure would look
    // like "browser is broken" rather than "two instances share a temp file".
    let dir = std::env::temp_dir().join("wunder_browser");
    std::fs::create_dir_all(&dir).map_err(|err| format!("Failed to create temp dir: {err}"))?;
    let mut hasher = DefaultHasher::new();
    BRIDGE_SCRIPT.hash(&mut hasher);
    let name = format!("browser_bridge-{:016x}.py", hasher.finish());
    let path = dir.join(&name);
    let expected = BRIDGE_SCRIPT.len() as u64;
    if std::fs::metadata(&path)
        .map(|meta| meta.len() == expected)
        .unwrap_or(false)
    {
        return Ok(path);
    }
    // Write aside and rename, so a reader never sees a half-written script.
    let staging = dir.join(format!("{name}.{}.tmp", std::process::id()));
    std::fs::write(&staging, BRIDGE_SCRIPT)
        .map_err(|err| format!("Failed to write bridge script: {err}"))?;
    match std::fs::rename(&staging, &path) {
        Ok(()) => Ok(path),
        // Another process published the same content first; its file is fine.
        Err(_) if path.exists() => {
            let _ = std::fs::remove_file(&staging);
            Ok(path)
        }
        Err(err) => {
            let _ = std::fs::remove_file(&staging);
            Err(format!("Failed to publish bridge script: {err}"))
        }
    }
}

/// Build the bridge child command. Free function so the warm pool can launch
/// from a plain thread without borrowing the bridge.
fn build_spawn_command(
    config: &EffectiveBrowserConfig,
    bridge_path: &Path,
) -> std::process::Command {
    let mut command = std::process::Command::new(python_program(config));
    command.arg(bridge_path.to_string_lossy().as_ref());
    if config.headless {
        command.arg("--headless");
    } else {
        command.arg("--no-headless");
    }
    command
        .arg("--width")
        .arg(config.viewport_width.to_string())
        .arg("--height")
        .arg(config.viewport_height.to_string())
        .arg("--max-tabs")
        .arg(config.max_tabs_per_session.to_string())
        .arg("--timeout")
        .arg(config.timeout_secs.to_string());
    let launch_mode = config.launch_mode.trim();
    if !launch_mode.is_empty() {
        command.arg("--mode").arg(launch_mode);
    }
    if let Some(channel) = &config.channel {
        command.arg("--channel").arg(channel);
    }
    if let Some(path) = &config.executable_path {
        command.arg("--executable-path").arg(path);
    }
    if let Some(endpoint) = &config.attach_endpoint {
        command.arg("--attach-endpoint").arg(endpoint);
    }
    // The URL policy is enforced inside the browser so that redirects,
    // subresources and script-driven requests cannot route around it.
    if config.allow_private_network {
        command.arg("--allow-private-network");
    }
    if !config.deny_file_scheme {
        command.arg("--allow-file-scheme");
    }
    for host in &config.hostname_allowlist {
        command.arg("--allow-host").arg(host);
    }
    for arg in launch_args(config) {
        command.arg(format!("--launch-arg={arg}"));
    }
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::null());
    apply_browser_env(&mut command, config);
    command_utils::apply_platform_spawn_options_std(&mut command);
    command
}

fn python_program(config: &EffectiveBrowserConfig) -> String {
    config
        .python_path
        .clone()
        .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.to_string())
}

fn launch_args(config: &EffectiveBrowserConfig) -> Vec<String> {
    let mut launch_args = config.launch_args.clone();
    if config.docker_enabled {
        if config.docker_use_no_sandbox && !launch_args.iter().any(|item| item == "--no-sandbox") {
            launch_args.push("--no-sandbox".to_string());
        }
        if config.docker_disable_dev_shm_usage
            && !launch_args
                .iter()
                .any(|item| item == "--disable-dev-shm-usage")
        {
            launch_args.push("--disable-dev-shm-usage".to_string());
        }
    }
    launch_args
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

/// Periodic maintenance: reclaim abandoned sessions and keep the warm pool
/// filled. One task per process, started on the first browser call.
struct BridgeJanitor {
    sessions: Arc<DashMap<String, Arc<BridgeSession>>>,
    idle_timeout_secs: u64,
    warm: Option<WarmPool>,
}

impl BridgeJanitor {
    async fn run(self) {
        let sweep = self.idle_timeout_secs > 0;
        let limit = Duration::from_secs(self.idle_timeout_secs);
        let mut ticker = tokio::time::interval(REAPER_PERIOD);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if sweep {
                sweep_at(&self.sessions, elapsed_millis(), limit);
            }
            if let Some(pool) = &self.warm {
                pool.top_up();
            }
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
    mut io: Box<dyn BridgeExchange>,
    mut cmd_rx: mpsc::Receiver<Command>,
    startup_tx: oneshot::Sender<Result<(), String>>,
    child: Arc<Mutex<Option<Child>>>,
) {
    let startup = io.ready();
    let ready = startup.is_ok();
    let _ = startup_tx.send(startup);
    // A failed handshake must still reach the awaiting caller as a launch
    // error, not as a generic command timeout.
    if ready {
        loop {
            let Some(command) = cmd_rx.blocking_recv() else {
                break;
            };
            match io.exchange(&command.line) {
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

    io.close();
    // Last word on the process: kill if something else did not, then reap.
    if let Ok(mut guard) = child.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Where the bridge process lives. Both sides speak the same line protocol;
/// only the pipe differs, so sessions, warm pooling and error contracts stay
/// identical whether Chromium runs locally or in the sandbox container.
trait BridgeExchange: Send {
    /// Consume the startup line.
    fn ready(&mut self) -> Result<(), String>;
    /// One command line in, one reply line out.
    fn exchange(&mut self, line: &str) -> Result<BridgeResponse, String>;
    /// Release the far end. Called once by the pump on every exit path.
    fn close(&mut self);
}

struct LocalExchange {
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl LocalExchange {
    fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            stdin,
            reader: BufReader::new(stdout),
        }
    }
}

impl BridgeExchange for LocalExchange {
    fn ready(&mut self) -> Result<(), String> {
        read_ready(&mut self.reader)
    }

    fn exchange(&mut self, line: &str) -> Result<BridgeResponse, String> {
        write_command(&mut self.stdin, line).and_then(|_| read_reply(&mut self.reader))
    }

    fn close(&mut self) {}
}

/// Bridge hosted by the sandbox. The ready line arrives with the create call,
/// so `ready()` only replays its verdict instead of opening a second read.
struct RemoteExchange {
    endpoint: String,
    bridge_id: String,
    ready_line: Option<String>,
    /// The page's own timeout plus room for the sandbox hop; a wedged page must
    /// fail the command instead of pinning the pump.
    command_timeout: Duration,
}

impl RemoteExchange {
    fn new(
        endpoint: &str,
        bridge_id: String,
        ready_line: String,
        command_timeout: Duration,
    ) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            bridge_id,
            ready_line: Some(ready_line),
            command_timeout,
        }
    }
}

impl BridgeExchange for RemoteExchange {
    fn ready(&mut self) -> Result<(), String> {
        let line = self
            .ready_line
            .take()
            .ok_or_else(|| "Browser sandbox returned no readiness line".to_string())?;
        parse_ready_line(&line)
    }

    fn exchange(&mut self, line: &str) -> Result<BridgeResponse, String> {
        let body = remote_post(
            &format!("{}/browser/bridges/command", self.endpoint),
            &serde_json::json!({ "bridge_id": self.bridge_id, "line": line }),
            self.command_timeout,
        )?;
        if body.get("ok").and_then(Value::as_bool) != Some(true) {
            let error = body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("browser sandbox command failed")
                .to_string();
            return Err(format!("Browser sandbox: {error}"));
        }
        let reply = body
            .get("line")
            .and_then(Value::as_str)
            .ok_or_else(|| "Browser sandbox returned an empty reply".to_string())?;
        serde_json::from_str(reply.trim())
            .map_err(|err| format!("Failed to parse browser response: {err}"))
    }

    fn close(&mut self) {
        let _ = remote_post(
            &format!("{}/browser/bridges/close", self.endpoint),
            &serde_json::json!({ "bridge_id": self.bridge_id }),
            REMOTE_CLOSE_TIMEOUT,
        );
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
    parse_ready_line(line.trim())
}

/// Interpret the bridge's first protocol line. Shared by the local pipe and the
/// sandbox transport so both report launch failures the same way.
fn parse_ready_line(line: &str) -> Result<(), String> {
    let ready: BridgeResponse = serde_json::from_str(line)
        .map_err(|err| format!("Bridge startup failed: {err}. Output: {line}"))?;
    if ready.success {
        log_startup_cost(ready.data.as_ref());
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

/// A cold start is paid once per browser session, so it is worth knowing which
/// phase owns it: Python import, the Playwright driver, or Chromium itself.
fn log_startup_cost(data: Option<&Value>) {
    let Some(data) = data else {
        return;
    };
    let total_ms = data
        .get("startup_ms")
        .and_then(|value| value.get("total_ms"))
        .and_then(Value::as_u64);
    let Some(total_ms) = total_ms else {
        return;
    };
    let phases = || {
        let startup = data.get("startup_ms").and_then(Value::as_object);
        ["import_ms", "driver_ms", "launch_ms"]
            .into_iter()
            .filter_map(|key| {
                startup
                    .and_then(|map| map.get(key))
                    .and_then(Value::as_u64)
                    .map(|value| format!("{key}={value}"))
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    if total_ms >= 10_000 {
        warn!(total_ms, breakdown = %phases(), "Browser bridge cold start was slow");
    } else {
        debug!(total_ms, breakdown = %phases(), "Browser bridge ready");
    }
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
    apply_bridge_environment(
        command,
        config.browsers_path.as_deref(),
        config.docker_downloads_root.as_deref(),
    );
}

/// Environment for a bridge process. Shared with the sandbox host so both
/// sides start Python the same way: a cleared environment plus the few keys
/// Playwright actually needs, never the server's own secrets.
pub(crate) fn apply_bridge_environment(
    command: &mut std::process::Command,
    browsers_path: Option<&str>,
    download_root: Option<&Path>,
) {
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
    if let Some(path) = browsers_path {
        command.env("PLAYWRIGHT_BROWSERS_PATH", path);
    } else if let Ok(value) = std::env::var("PLAYWRIGHT_BROWSERS_PATH") {
        command.env("PLAYWRIGHT_BROWSERS_PATH", value);
    }
    if let Some(path) = download_root {
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
        bridge_spec, elapsed_millis, is_pipeline_unready, launch_deadline, parse_ready_line,
        pick_evict_candidate, short_slot, sweep_at, BridgeResponse, BridgeSession, BrowserBridge,
        Command, ACTIVE_PROTECT, LAUNCH_GRACE, STATE_LAUNCHING, STATE_READY, WARM_PIPELINE_RETRY,
        WARM_RETRY_CEILING,
    };
    use crate::config::Config;
    use crate::services::browser::config::effective_browser_config;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
    use std::sync::{Arc, Mutex, RwLock};
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
        bridge_with_prewarm(idle_timeout_secs, max_sessions, 0)
    }

    fn bridge_with_prewarm(
        idle_timeout_secs: u64,
        max_sessions: usize,
        prewarm_sessions: usize,
    ) -> BrowserBridge {
        let mut config = Config::default();
        config.browser.enabled = true;
        config.tools.browser.enabled = true;
        let mut effective = effective_browser_config(&config);
        effective.idle_timeout_secs = idle_timeout_secs;
        effective.max_sessions = max_sessions;
        effective.prewarm_sessions = prewarm_sessions;
        BrowserBridge::new(effective)
    }

    /// A session that answers nothing: stands in for a bridge wedged inside a
    /// page, without depending on Python or a browser in the test environment.
    fn silent_session(key: &str, idle_ms: u64) -> (Arc<BridgeSession>, mpsc::Receiver<Command>) {
        let (tx, rx) = mpsc::channel::<Command>(1);
        let session = Arc::new(BridgeSession {
            key: RwLock::new(key.to_string()),
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
        bridge.sessions.insert(session.key(), session.clone());
        let error = bridge
            .send_command(
                &session.key(),
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

    #[test]
    fn warm_pool_is_disabled_by_zero_target() {
        let bridge = bridge_with_prewarm(300, 5, 0);
        assert_eq!(bridge.warm_target(), 0);
        assert_eq!(bridge.prewarm(), 0);
        bridge.warm_pool_state();
        assert_eq!(bridge.warm_pool_state(), (0, 0));
    }

    #[test]
    fn prewarm_target_never_exceeds_the_session_table() {
        let bridge = bridge_with_prewarm(300, 2, 9);
        assert_eq!(bridge.warm_target(), 2);
    }

    #[test]
    fn ready_warm_session_is_handed_out_and_renamed() {
        let bridge = bridge_with_prewarm(300, 5, 1);
        let (session, _rx) = silent_session("warm-pool:1", 0);
        bridge.warm.lock().unwrap().push(Arc::clone(&session));
        assert_eq!(bridge.warm_pool_state(), (1, 0));

        let adopted = bridge.take_warm().expect("a ready warm browser");
        adopted.set_key("u1:thread");
        assert_eq!(adopted.key(), "u1:thread");
        assert!(bridge.warm.lock().unwrap().is_empty());
        // Pooled browsers never count as sessions until they are adopted.
        assert_eq!(bridge.session_count(), 0);
    }

    #[test]
    fn dead_warm_session_is_discarded_instead_of_handed_out() {
        let bridge = bridge_with_prewarm(300, 5, 1);
        let (session, _rx) = silent_session("warm-pool:2", 0);
        session.mark_dead();
        bridge.warm.lock().unwrap().push(session);
        assert!(bridge.take_warm().is_none());
        assert_eq!(bridge.warm_pool_state(), (0, 0));
    }

    #[test]
    fn sandbox_connect_failures_count_as_unready_pipeline() {
        assert!(is_pipeline_unready(
            "Browser runs in the sandbox but http://wunder-sandbox:9001 did not accept the \
             bridge: error sending request for url (http://wunder-sandbox:9001/browser/bridges)"
        ));
        assert!(is_pipeline_unready("Connection refused (os error 111)"));
        assert!(is_pipeline_unready(
            "dns error: failed to lookup address information"
        ));
        assert!(!is_pipeline_unready(
            "Browser bridge failed to start: ModuleNotFoundError: No module named 'playwright'"
        ));
        // An unreachable sandbox is retried far sooner than a broken launch.
        assert!(WARM_PIPELINE_RETRY < WARM_RETRY_CEILING);
    }

    #[test]
    fn warm_pool_backs_off_after_a_failed_launch() {
        let bridge = bridge_with_prewarm(300, 5, 1);
        // Backoff gate: no launch is attempted while the retry stamp is ahead.
        bridge
            .warm_retry_after_ms
            .store(u64::MAX, Ordering::Release);
        assert_eq!(bridge.top_up(), 0);
        assert_eq!(bridge.warm_pool_state(), (0, 0));
    }

    #[test]
    fn sandbox_ready_line_uses_the_local_launch_verdict() {
        let ready = json!({"success": true, "data": {"status": "ready"}}).to_string();
        assert!(parse_ready_line(&ready).is_ok());
        let failed = json!({"success": false, "error": "playwright not installed"}).to_string();
        let err = parse_ready_line(&failed).expect_err("startup failure");
        assert!(err.contains("playwright not installed"), "got: {err}");
    }

    #[test]
    fn sandbox_spec_carries_the_launch_policy() {
        // The sandbox reads no browser config of its own, so every policy knob
        // that shapes the launch has to travel in the spec.
        let mut config = Config::default();
        config.browser.enabled = true;
        config.tools.browser.enabled = true;
        config.browser.deployment = "sandbox".to_string();
        config.browser.playwright.timeout_secs = 45;
        config.browser.limits.max_tabs_per_session = 3;
        config.browser.security.hostname_allowlist = vec!["Example.com".to_string()];
        config.browser.docker.enabled = true;
        let effective = effective_browser_config(&config);
        let spec = bridge_spec(&effective);
        assert_eq!(spec["timeout_secs"], 45);
        assert_eq!(spec["max_tabs"], 3);
        assert_eq!(spec["hostname_allowlist"][0], "example.com");
        assert_eq!(spec["mode"], "launch");
        // Container launch args must survive the hop, or Chromium cannot start
        // in the sandbox container at all.
        let args = spec["launch_args"].as_array().expect("launch args");
        assert!(args.iter().any(|arg| arg == "--no-sandbox"));
        // The server's downloads root points at a path the sandbox lacks.
        assert!(spec["download_root"].is_null());
    }
}
