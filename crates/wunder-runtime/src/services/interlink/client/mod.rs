//! Outbound interlink tunnel client for a local node (蜂窝 / 舵机).
//!
//! The node never listens: it opens one long-lived WSS to the server and
//! carries every plane of the interlink protocol over that single socket
//! (docs §2.2, §2.3). This module owns the lifecycle, the outbound queues, the
//! presence beat and the remote session forwarding; the pieces are split into
//! [`handshake`] (bootstrap + reconnect policy), [`shadow`] (collector),
//! [`execute`] (command executor), [`approval`] (human-in-the-loop gate) and
//! [`stream`] (data plane).
//!
//! Guarantees this file is responsible for:
//! * idempotent, process-wide `start`, gated by a cloud session **and** the
//!   session's `interlink.enabled` kill switch;
//! * every queue bounded (outbound control/data, inbound commands, prompt
//! * approvals, in-flight commands, thread forwarders);
//! * the tunnel never blocks the engine: heavy work runs on the blocking pool
//!   or in detached tasks, the socket task only frames.

pub mod approval;
pub mod execute;
pub mod handshake;
pub mod shadow;
pub mod stream;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Notify};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;

use wunder_core::interlink::{
    APPROVAL_APPROVED, APPROVAL_EXPIRED, APPROVAL_NONE, APPROVAL_REJECTED, CAP_AGENT_SPAWN,
    CAP_QUERY_BASIC, CAP_SHADOW_FULL, CAP_SHADOW_MINIMAL, CAP_THREAD_DRIVE, CAP_TOOL_EXEC,
    CAP_WORKSPACE_READ_BINARY, CAP_WORKSPACE_WRITE, CMD_COMMAND_CANCEL, EVENT_NODE_JOINED,
    EVENT_PRESENCE, EVENT_THREAD, FRAME_COMMAND, FRAME_COMMAND_ACK, FRAME_COMMAND_RESULT,
    FRAME_ERROR, FRAME_EVENT, FRAME_HELLO_ACK, FRAME_PONG, NODE_STATUS_AWAY, NODE_STATUS_BUSY,
    NODE_STATUS_ONLINE, ERR_NODE_BUSY, InterlinkFrame, InterlinkHelloAck, TUNNEL_WS_PROTOCOL,
};

use crate::core::long_task;
use crate::services::cloud::{self, CloudSessionFile};
pub use crate::services::interlink::client::approval::{Decision, PendingApproval};
use crate::services::interlink::client::approval::{ApprovalGate, GateError};
use crate::services::interlink::client::execute::{
    CommandLedger, CommandReport, CommandSpec, ExecContext, InFlightTable, Preflight,
};
use crate::services::interlink::client::handshake::{
    CloseReason, HandshakeError, NodeSecret, RetryPolicy, SecretFlow,
};
use crate::services::interlink::client::shadow::{ShadowCollector, ShadowSources, TreeLimits};
use crate::services::interlink::OutboundFrame;
use crate::state::AppState;

/// Local engine user of a desktop node when the form does not name one.
pub const DEFAULT_DESKTOP_USER_ID: &str = "desktop_user";
/// Local engine user of a cli node when the form does not name one.
pub const DEFAULT_CLI_USER_ID: &str = "cli_user";
/// Idle wait while no session exists or the kill switch is on: the client is
/// then a cheap poller, and the state surfaced is `disabled`.
pub const IDLE_WAIT: Duration = Duration::from_secs(30);
/// Application-level presence beat (docs §4.4).
pub const PRESENCE_INTERVAL: Duration = Duration::from_secs(60);
/// Idle time after which a beat reports `away` (docs §5.1).
pub const AWAY_IDLE_S: f64 = 600.0;
/// Bounded inbound command queue (docs §10.1 inflight plus a small buffer).
pub const COMMAND_QUEUE_CAPACITY: usize = 16;
/// Bounded control queue of one connection.
pub const CONTROL_QUEUE_CAPACITY: usize = 64;
/// Data queue capacity == chunk window (docs §10.1: 16 chunks in flight).
pub const DATA_QUEUE_CAPACITY: usize = stream::MAX_INFLIGHT_CHUNKS;
/// Thread event forwarders per node (docs §10.1: remote subscriptions ≤8).
pub const MAX_THREAD_FORWARDERS: usize = 8;
/// Dead-link watchdog: the server pings every `heartbeat_s`; three missed
/// beats with nothing at all from the server means the socket is a zombie.
pub const HEARTBEAT_MISSED_BEATS: u32 = 3;
/// Default server heartbeat period, used until a config says otherwise.
pub const DEFAULT_HEARTBEAT_S: u64 = 25;

/// Tunnel sub-state surfaced next to the cloud channel badge (docs §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelState {
    /// First connection attempt of this cycle.
    Connecting,
    /// Handshake completed; the steady-state loop is running.
    Connected,
    /// Reconnect backoff is running.
    Reconnecting,
    /// Never started: no session, kill switch on, or the server refuses this
    /// node outright (revoked / disabled / protocol too old).
    Disabled,
}

/// UI-facing tunnel status.
#[derive(Debug, Clone, Serialize)]
pub struct TunnelStatus {
    pub state: TunnelState,
    pub device_id: Option<String>,
    pub channel_id: Option<String>,
    pub capabilities: Vec<String>,
    pub shadow_revision: i64,
    pub pending_approvals: usize,
    pub inflight_commands: usize,
    pub watched_threads: usize,
    /// Frames the bounded outbound queues refused since this process started
    /// (docs §10.2: saturation degrades, it never blocks the engine).
    pub dropped_frames: u64,
    /// Sanitized summary; never a token, secret or url.
    pub last_error: Option<String>,
    pub next_retry_at: Option<f64>,
}

/// What a local form must tell the client about itself. The ids live in the
/// forms (`wunder-desktop`/`wunder-cli`), so they are injected rather than
/// duplicated: `start` falls back to the profile default.
#[derive(Debug, Clone)]
pub struct InterlinkLocalOptions {
    /// Local engine user whose threads and workspace are projected.
    pub local_user_id: String,
    /// Desktop workspace binding; `None` keeps the default scope.
    pub workspace_id: Option<String>,
    /// Where `cloud.session.json` lives; defaults to the wunder home.
    pub session_base_dir: PathBuf,
}

impl InterlinkLocalOptions {
    /// Profile-derived defaults.
    pub fn for_state(state: &AppState) -> Self {
        let local_user_id = match state.runtime_profile {
            crate::state::AppRuntimeProfile::DesktopEmbedded => DEFAULT_DESKTOP_USER_ID,
            crate::state::AppRuntimeProfile::CliEmbedded => DEFAULT_CLI_USER_ID,
            _ => "local",
        }
        .to_string();
        Self {
            local_user_id,
            workspace_id: None,
            session_base_dir: crate::services::cloud::session::wunder_home_dir(),
        }
    }
}

/// Build/platform facts that bound what this node may announce (docs §9.2: a
/// capability is declared only when this build can actually honour it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodePlatform {
    /// The Windows 7 build target has no sandbox/job-object primitives, so a
    /// remotely started command could not be bounded or killed here.
    pub legacy_windows: bool,
    /// Spawning a work unit needs a live thread runtime.
    pub thread_runtime_active: bool,
}

impl NodePlatform {
    pub fn detect(state: &AppState) -> Self {
        Self {
            legacy_windows: cfg!(target_vendor = "win7"),
            thread_runtime_active: state.runtime_capabilities.thread_runtime_active,
        }
    }
}

/// The L3 capabilities this build/platform can honour, empty on a node that
/// cannot run them. Pure so the rule is testable without a tunnel.
pub fn platform_capabilities(platform: &NodePlatform) -> Vec<&'static str> {
    let mut caps = Vec::with_capacity(2);
    if !platform.legacy_windows {
        caps.push(CAP_TOOL_EXEC);
    }
    if platform.thread_runtime_active {
        caps.push(CAP_AGENT_SPAWN);
    }
    caps
}

/// Capabilities declared in `hello` and intersected by the server (docs §9.2).
/// Anything outside this set is refused by [`execute::preflight`] even when a
/// server granted it, so a wrong `hello_ack` cannot make the node run something
/// it does not support.
pub fn declared_capabilities(platform: &NodePlatform) -> Vec<String> {
    let mut caps = vec![
        CAP_SHADOW_FULL.to_string(),
        CAP_SHADOW_MINIMAL.to_string(),
        CAP_QUERY_BASIC.to_string(),
        CAP_WORKSPACE_READ_BINARY.to_string(),
        CAP_THREAD_DRIVE.to_string(),
        CAP_WORKSPACE_WRITE.to_string(),
    ];
    caps.extend(
        platform_capabilities(platform)
            .into_iter()
            .map(str::to_string),
    );
    caps
}

/// One connection's outbound router: two bounded queues, one socket.
pub struct TunnelWriter {
    control: mpsc::Sender<OutboundFrame>,
    data: mpsc::Sender<OutboundFrame>,
    channel_id: RwLock<Option<String>>,
}

/// Frames a full bounded queue refused, cumulative for the process so a
/// degradation stays visible across reconnects (docs §10.2).
static DROPPED_FRAMES: AtomicU64 = AtomicU64::new(0);

/// Frames the tunnel had to drop since this process started.
pub fn dropped_frames() -> u64 {
    DROPPED_FRAMES.load(Ordering::Relaxed)
}

impl TunnelWriter {
    pub(crate) fn new() -> (
        Arc<Self>,
        mpsc::Receiver<OutboundFrame>,
        mpsc::Receiver<OutboundFrame>,
    ) {
        let (control_tx, control_rx) = mpsc::channel(CONTROL_QUEUE_CAPACITY);
        let (data_tx, data_rx) = mpsc::channel(DATA_QUEUE_CAPACITY);
        (
            Arc::new(Self {
                control: control_tx,
                data: data_tx,
                channel_id: RwLock::new(None),
            }),
            control_rx,
            data_rx,
        )
    }

    pub fn channel_id(&self) -> Option<String> {
        self.channel_id
            .read()
            .expect("tunnel writer lock poisoned")
            .clone()
    }

    fn set_channel_id(&self, channel_id: &str) {
        *self
            .channel_id
            .write()
            .expect("tunnel writer lock poisoned") = Some(channel_id.to_string());
    }

    /// Data queue, handed to the chunk pump so it can await a free slot.
    pub fn data_sender(&self) -> &mpsc::Sender<OutboundFrame> {
        &self.data
    }

    fn frame(&self, kind: &str, corr: Option<&str>, payload: Value) -> InterlinkFrame {
        let mut frame = InterlinkFrame::new(
            kind,
            handshake::frame_id(),
            now_ts(),
            self.channel_id(),
            payload,
        );
        if let Some(corr) = corr {
            frame = frame.with_corr(corr);
        }
        frame
    }

    /// Queue one frame; awaits a free slot (never drops silently on backpressure).
    pub async fn send_frame(
        &self,
        kind: &str,
        corr: Option<&str>,
        payload: Value,
    ) -> Result<(), ()> {
        let frame = self.frame(kind, corr, payload);
        let text = serde_json::to_string(&frame).map_err(|_| ())?;
        self.control
            .send(OutboundFrame::Text(text))
            .await
            .map_err(|_| ())
    }

    /// Fire-and-forget variant for frames that may be dropped when the tunnel
    /// is saturated (presence, forwarded thread events - docs §10.2).
    pub fn try_send_frame(&self, kind: &str, corr: Option<&str>, payload: Value) -> bool {
        let frame = self.frame(kind, corr, payload);
        match serde_json::to_string(&frame) {
            Ok(text) => {
                if self
                    .control
                    .try_send(OutboundFrame::Text(text))
                    .is_err()
                {
                    DROPPED_FRAMES.fetch_add(1, Ordering::Relaxed);
                    return false;
                }
                true
            }
            Err(_) => false,
        }
    }
}

/// Everything the client keeps about the node itself.
struct ClientInner {
    state: Arc<AppState>,
    options: InterlinkLocalOptions,
    writer: RwLock<Option<Arc<TunnelWriter>>>,
    tunnel: RwLock<TunnelState>,
    capabilities: RwLock<Vec<String>>,
    shadow_revision: std::sync::atomic::AtomicI64,
    resume_channel: RwLock<Option<String>>,
    approvals: ApprovalGate,
    ledger: CommandLedger,
    in_flight: InFlightTable,
    collector: ShadowCollector,
    shadow_wake: Arc<Notify>,
    /// Command sender of the current generation; `start()` swaps it together
    /// with the dispatcher's matching receiver.
    commands: RwLock<mpsc::Sender<CommandSpec>>,
    forwarders: RwLock<HashMap<String, Forwarder>>,
    secret: RwLock<Option<NodeSecret>>,
    last_error: RwLock<Option<String>>,
    next_retry_at: RwLock<Option<f64>>,
    last_busy_at: AtomicU64,
    /// Cancellation token of the **current start generation**: `stop()` cancels
    /// it and `start()` installs a fresh one, so a stopped tunnel can be
    /// re-opened in-process (kill switch lifted, config flipped back) instead of
    /// staying dead until the form restarts.
    stop: RwLock<CancellationToken>,
    running: AtomicBool,
}

struct Forwarder {
    cancel: CancellationToken,
}

impl ClientInner {
    fn new(state: Arc<AppState>, options: InterlinkLocalOptions) -> Arc<Self> {
        // The receiver belongs to a start generation; an unstarted client keeps
        // its sender pointed at a closed channel, so `try_send` fails loudly
        // instead of parking commands nobody will ever run.
        let (commands, receiver) = mpsc::channel(COMMAND_QUEUE_CAPACITY);
        drop(receiver);
        Arc::new(Self {
            state,
            options,
            writer: RwLock::new(None),
            tunnel: RwLock::new(TunnelState::Disabled),
            capabilities: RwLock::new(Vec::new()),
            shadow_revision: std::sync::atomic::AtomicI64::new(0),
            resume_channel: RwLock::new(None),
            approvals: ApprovalGate::default(),
            ledger: CommandLedger::default(),
            in_flight: InFlightTable::default(),
            collector: ShadowCollector::new(),
            shadow_wake: Arc::new(Notify::new()),
            commands: RwLock::new(commands),
            forwarders: RwLock::new(HashMap::new()),
            secret: RwLock::new(None),
            last_error: RwLock::new(None),
            next_retry_at: RwLock::new(None),
            last_busy_at: AtomicU64::new(0),
            stop: RwLock::new(CancellationToken::new()),
            running: AtomicBool::new(false),
        })
    }

    // -- state helpers ------------------------------------------------------

    fn set_tunnel(&self, state: TunnelState) {
        *self.tunnel.write().expect("tunnel state lock poisoned") = state;
    }

    fn tunnel_state(&self) -> TunnelState {
        *self.tunnel.read().expect("tunnel state lock poisoned")
    }

    fn set_error(&self, summary: Option<String>) {
        *self.last_error.write().expect("tunnel state lock poisoned") = summary;
    }

    fn set_next_retry(&self, at: Option<f64>) {
        *self.next_retry_at.write().expect("tunnel state lock poisoned") = at;
    }

    fn session(&self) -> Option<CloudSessionFile> {
        cloud::shared().session()
    }

    /// Persist the node secret into the session file (0600 through `save`).
    /// The in-memory cloud session keeps its own copy of everything else; this
    /// client reads the secret back from the file after a restart.
    fn store_secret(&self, secret: &NodeSecret) {
        let Some(mut session) = self.session() else {
            return;
        };
        session.interlink.node_secret = Some(secret.secret.clone());
        session.interlink.secret_version = secret.version;
        let base = self.options.session_base_dir.clone();
        long_task::spawn("interlink.client.store_secret", async move {
            let _ = tokio::task::spawn_blocking(move || session.save(&base)).await;
        });
    }

    fn cached_secret(&self) -> Option<NodeSecret> {
        if let Some(secret) = self
            .secret
            .read()
            .expect("tunnel state lock poisoned")
            .clone()
        {
            return Some(secret);
        }
        let session = self.session()?;
        let secret = session.interlink.node_secret.clone()?;
        if secret.trim().is_empty() {
            return None;
        }
        let version = session.interlink.secret_version.max(1);
        let secret = NodeSecret { secret, version };
        *self.secret.write().expect("tunnel state lock poisoned") = Some(secret.clone());
        Some(secret)
    }

    fn writer(&self) -> Option<Arc<TunnelWriter>> {
        self.writer.read().expect("tunnel state lock poisoned").clone()
    }

    fn clear_writer(&self, writer: &Arc<TunnelWriter>) {
        let mut guard = self.writer.write().expect("tunnel state lock poisoned");
        if guard
            .as_ref()
            .map(|current| Arc::ptr_eq(current, writer))
            .unwrap_or(false)
        {
            *guard = None;
        }
    }

    fn mark_dirty_local(&self, sections: shadow::Sections) {
        self.collector.mark_dirty(sections);
    }

    /// Local activity: how many threads this node is running right now.
    fn active_threads(&self) -> usize {
        self.state.monitor.count_active_sessions()
    }

    fn presence_payload(&self, now: f64) -> Value {
        let active = self.active_threads();
        let last_busy = self.last_busy_at.load(Ordering::Acquire) as f64;
        if active > 0 {
            self.last_busy_at.store(now as u64, Ordering::Release);
        }
        let idle_s = if last_busy <= 0.0 {
            0.0
        } else {
            (now - last_busy).max(0.0)
        };
        let status = if active > 0 {
            NODE_STATUS_BUSY
        } else if idle_s > AWAY_IDLE_S {
            NODE_STATUS_AWAY
        } else {
            NODE_STATUS_ONLINE
        };
        json!({
            "kind": EVENT_PRESENCE,
            "status": status,
            "active_threads": active,
            "idle_s": idle_s,
        })
    }

    /// The shadow budget from the local config section (docs §3.3 server side
    /// values are mirrored in the same schema for the node).
    async fn tree_limits(&self) -> TreeLimits {
        let config = self.state.config_store.get().await;
        let shadow_config = &config.interlink.shadow;
        TreeLimits {
            max_entries: shadow_config.tree_max_entries.clamp(1, shadow::TREE_MAX_ENTRIES),
            depth: shadow_config.tree_depth.clamp(1, shadow::TREE_DEPTH),
            threads_max: shadow_config.threads_max.clamp(1, shadow::THREADS_MAX),
            tasks_max: shadow::TASKS_MAX,
        }
    }

    async fn chunk_bytes(&self) -> usize {
        stream::chunk_bytes_for(self.state.config_store.get().await.interlink.file_chunk_kb)
    }

    fn source_tag(&self, writer: &Option<Arc<TunnelWriter>>) -> String {
        let channel = writer
            .as_ref()
            .and_then(|writer| writer.channel_id())
            .unwrap_or_else(|| "offline".to_string());
        format!("remote:cloud:{channel}")
    }

    async fn exec_context(&self, writer: &Arc<TunnelWriter>, session: &CloudSessionFile) -> ExecContext {
        ExecContext {
            state: self.state.clone(),
            local_user_id: self.options.local_user_id.clone(),
            workspace_id: self.options.workspace_id.clone(),
            max_file_pull_bytes: session.interlink.max_file_pull_bytes(),
            chunk_bytes: self.chunk_bytes().await,
            rate_bps: stream::MAX_BYTES_PER_S,
            source_tag: self.source_tag(&Some(writer.clone())),
        }
    }

    async fn shadow_sources(&self, session: &CloudSessionFile) -> ShadowSources {
        let identity = shadow::NodeIdentity::from_session(&session.client, &session.device_name);
        ShadowSources::resolved(
            identity,
            self.options.local_user_id.clone(),
            self.options.workspace_id.clone(),
            self.tree_limits().await,
            &self.capabilities(),
        )
    }

    fn capabilities(&self) -> Vec<String> {
        self.capabilities
            .read()
            .expect("tunnel state lock poisoned")
            .clone()
    }

    /// What this node is able to honour right now: the same set `hello`
    /// declared, recomputed so a capability the server handed out by mistake is
    /// still refused (docs §9.2 three-way intersection).
    fn declared_capabilities(&self) -> Vec<String> {
        declared_capabilities(&NodePlatform::detect(&self.state))
    }

    fn set_capabilities(&self, caps: Vec<String>) {
        *self.capabilities.write().expect("tunnel state lock poisoned") = caps;
    }

    fn status(&self) -> TunnelStatus {
        let writer = self.writer();
        TunnelStatus {
            state: self.tunnel_state(),
            device_id: self.session().map(|session| session.device_id),
            channel_id: writer.as_ref().and_then(|writer| writer.channel_id()),
            capabilities: self.capabilities(),
            shadow_revision: self.shadow_revision(),
            pending_approvals: self.approvals.pending().len(),
            inflight_commands: self.in_flight.len(),
            watched_threads: self
                .forwarders
                .read()
                .expect("forwarder table lock poisoned")
                .len(),
            dropped_frames: dropped_frames(),
            last_error: self
                .last_error
                .read()
                .expect("tunnel state lock poisoned")
                .clone(),
            next_retry_at: *self.next_retry_at.read().expect("tunnel state lock poisoned"),
        }
    }

    fn shadow_revision(&self) -> i64 {
        self.shadow_revision.load(Ordering::Acquire)
    }
}

/// The client handle a UI or a test holds. One instance exists per process.
#[derive(Clone)]
pub struct InterlinkClient {
    inner: Arc<ClientInner>,
}

impl InterlinkClient {
    /// Build a client without starting it (tests and embedded facades).
    pub fn new(state: Arc<AppState>, options: InterlinkLocalOptions) -> Self {
        Self {
            inner: ClientInner::new(state, options),
        }
    }

    pub fn status(&self) -> TunnelStatus {
        self.inner.status()
    }

    /// Prompt queue for the local modal / TUI line (docs §7.3).
    pub fn pending_approvals(&self) -> Vec<PendingApproval> {
        self.inner.approvals.pending()
    }

    /// Apply the user's decision. `remember` extends the L1 scope memory by
    /// [`approval::MEMORY_TTL_S`]; L2/L3 can never be remembered.
    pub fn decide_approval(&self, approval_id: &str, decision: Decision, remember: bool) -> bool {
        self.inner
            .approvals
            .decide(approval_id, decision, remember, now_ts())
    }

    /// Revoke every remembered grant (UI "stop trusting this source").
    pub fn revoke_remembered(&self) {
        self.inner.approvals.forget_memory();
    }

    /// Nudge the collector after a local change the engine knows about but the
    /// cheap signals cannot see (rename, scheduled-task edit).
    pub fn invalidate_shadow(&self, sections: shadow::Sections) {
        self.inner.mark_dirty_local(sections);
    }

    /// Start the tunnel loop. Idempotent: a second call is a no-op, and after a
    /// [`Self::stop`] it opens a fresh generation, so a stopped tunnel can be
    /// re-opened in the same process.
    pub fn start(&self) {
        let inner = self.inner.clone();
        if inner.running.swap(true, Ordering::AcqRel) {
            return;
        }
        // Replace the token `stop()` cancelled before any task is spawned.
        inner.begin_generation();
        inner.set_tunnel(TunnelState::Connecting);
        // One command channel per generation: the previous receiver was moved
        // into the dispatcher that `stop()` just cancelled, so taking it from a
        // slot would leave a reconnected node with nobody to run commands.
        let (sender, receiver) = mpsc::channel(COMMAND_QUEUE_CAPACITY);
        *inner.commands.write().expect("command sender lock poisoned") = sender;
        long_task::spawn("interlink.client.commands", command_dispatcher(
            inner.clone(),
            receiver,
        ));
        long_task::spawn("interlink.client.run_loop", run_loop(inner.clone()));
    }

    /// Stop the tunnel and forget everything transient. The kill switch uses
    /// the same path, so a restart picks up the new session config.
    pub fn stop(&self) {
        if !self.inner.running.load(Ordering::Acquire) {
            return;
        }
        self.inner.running.store(false, Ordering::Release);
        self.inner.cancel_stop();
        self.inner.set_tunnel(TunnelState::Disabled);
        if let Some(writer) = self.inner.writer() {
            self.inner.clear_writer(&writer);
        }
        self.inner.approvals.clear();
        self.inner.set_capabilities(Vec::new());
        self.drop_all_forwarders();
        let inner = self.inner.clone();
        long_task::spawn("interlink.client.abort", async move {
            inner.ledger.abort_all().await;
        });
    }

    fn drop_all_forwarders(&self) {
        let forwarders: Vec<Forwarder> = {
            let mut table = self
                .inner
                .forwarders
                .write()
                .expect("forwarder table lock poisoned");
            table.drain().map(|(_, entry)| entry).collect()
        };
        for entry in forwarders {
            entry.cancel.cancel();
        }
    }
}

// ---------------------------------------------------------------------------
// Public process-wide surface: what the Slint UI and the TUI call
// ---------------------------------------------------------------------------

static SHARED: OnceLock<Arc<InterlinkClient>> = OnceLock::new();

/// Start the tunnel for this process, if a cloud session exists and its
/// `interlink.enabled` switch is on. Idempotent; safe to call on every start
/// of a local form, and a no-op on the server form.
pub fn start(state: Arc<AppState>) {
    start_with_options(state.clone(), InterlinkLocalOptions::for_state(state.as_ref()));
}

/// Same as [`start`] with the form's own local identity.
pub fn start_with_options(state: Arc<AppState>, options: InterlinkLocalOptions) {
    if state.runtime_profile == crate::state::AppRuntimeProfile::ServerDistributed {
        // The server hosts tunnels; it never opens one.
        return;
    }
    let client = SHARED
        .get_or_init(|| Arc::new(InterlinkClient::new(state, options)))
        .clone();
    client.start();
}

/// The process-wide client, when this form started one.
pub fn shared() -> Option<Arc<InterlinkClient>> {
    SHARED.get().cloned()
}

/// Tunnel sub-state for the UI badge.
pub fn status() -> TunnelStatus {
    shared()
        .map(|client| client.status())
        .unwrap_or(TunnelStatus {
            state: TunnelState::Disabled,
            device_id: None,
            channel_id: None,
            capabilities: Vec::new(),
            shadow_revision: 0,
            pending_approvals: 0,
            inflight_commands: 0,
            watched_threads: 0,
            dropped_frames: 0,
            last_error: None,
            next_retry_at: None,
        })
}

pub fn pending_approvals() -> Vec<PendingApproval> {
    shared().map(|client| client.pending_approvals()).unwrap_or_default()
}

pub fn decide_approval(approval_id: &str, decision: Decision, remember: bool) -> bool {
    shared()
        .map(|client| client.decide_approval(approval_id, decision, remember))
        .unwrap_or(false)
}

/// Stop the tunnel (also the kill-switch path once the session file is saved
/// with `enabled: false`).
pub fn stop() {
    if let Some(client) = shared() {
        client.stop();
    }
}

/// Unix seconds; the tunnel envelope timestamps use it.
pub(crate) fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs_f64())
        .unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// Run loop: bootstrap, connect, backoff
// ---------------------------------------------------------------------------

async fn run_loop(inner: Arc<ClientInner>) {
    let mut attempt: u32 = 0;
    let mut flow = SecretFlow::default();
    loop {
        if inner.stop_token().is_cancelled() {
            return;
        }
        let Some(session) = inner.session() else {
            // Logged out: stay quiet and cheap, surface `disabled`.
            inner.set_tunnel(TunnelState::Disabled);
            inner.set_capabilities(Vec::new());
            sleep_or_stop(&inner, IDLE_WAIT).await;
            continue;
        };
        if !session.interlink.enabled {
            inner.set_tunnel(TunnelState::Disabled);
            inner.set_error(Some("interlink is disabled locally".to_string()));
            sleep_or_stop(&inner, IDLE_WAIT).await;
            continue;
        }
        inner.set_error(None);
        // Bootstrap the node secret before the first ticket (docs §4.1).
        if inner.cached_secret().is_none() {
            match handshake::fetch_node_secret(&session).await {
                Ok(secret) => {
                    *inner.secret.write().expect("tunnel state lock poisoned") = Some(secret);
                    inner.store_secret(inner.secret_value().as_ref().unwrap());
                }
                Err(err) => {
                    // No secret surface (older server) or a transient failure:
                    // keep the cloud channel working and back off.
                    inner.set_error(Some(err.summary()));
                    inner.set_tunnel(TunnelState::Disabled);
                    let delay = jittered(handshake::backoff_delay(attempt.max(1)));
                    inner.set_next_retry(Some(now_ts() + delay.as_secs_f64()));
                    sleep_or_stop(&inner, delay).await;
                    attempt = attempt.saturating_add(1);
                    continue;
                }
            }
        }
        let secret = match inner.secret_value() {
            Some(secret) => secret,
            None => {
                inner.set_tunnel(TunnelState::Disabled);
                sleep_or_stop(&inner, IDLE_WAIT).await;
                continue;
            }
        };

        inner.set_tunnel(if attempt == 0 {
            TunnelState::Connecting
        } else {
            TunnelState::Reconnecting
        });
        let outcome = connect_once(inner.clone(), &session, secret).await;
        if inner.stop_token().is_cancelled() {
            return;
        }
        let reason = match outcome {
            Ok(()) => {
                // Clean end of a steady-state session (close, socket error).
                flow.reset();
                CloseReason::ClosedByPeer
            }
            Err(reason) => reason,
        };
        let (policy, refresh_secret) = handshake::secret_retry_plan(&mut flow, &reason);
        if refresh_secret {
            match handshake::fetch_node_secret(&session).await {
                Ok(secret) => {
                    inner.store_secret(&secret);
                    *inner.secret.write().expect("tunnel state lock poisoned") = Some(secret);
                }
                Err(err) => inner.set_error(Some(err.summary())),
            }
        }
        match policy {
            RetryPolicy::Now => attempt = attempt.saturating_add(1),
            RetryPolicy::Backoff => attempt = attempt.saturating_add(1),
            RetryPolicy::Cooldown => attempt = COOLDOWN_ATTEMPT,
        }
        let delay = jittered(handshake::backoff_delay(attempt.max(1)));
        inner.set_next_retry(Some(now_ts() + delay.as_secs_f64()));
        inner.set_tunnel(TunnelState::Reconnecting);
        sleep_or_stop(&inner, delay).await;
    }
}

/// Backoff step that lands on the documented 300 s ceiling: used for reasons
/// a fast retry cannot fix (docs §4.4).
const COOLDOWN_ATTEMPT: u32 = 9;

impl ClientInner {
    fn secret_value(&self) -> Option<NodeSecret> {
        self.secret.read().expect("tunnel state lock poisoned").clone()
    }

    /// Cancellation token of the current generation.
    fn stop_token(&self) -> CancellationToken {
        self.stop
            .read()
            .expect("tunnel stop lock poisoned")
            .clone()
    }

    /// Cancel the current generation. Both this and [`Self::begin_generation`]
    /// take the lock, so a concurrent `start` can neither observe a half-applied
    /// switch nor slip a fresh token in that nobody cancels.
    fn cancel_stop(&self) {
        self.stop
            .read()
            .expect("tunnel stop lock poisoned")
            .cancel();
    }

    /// Start a new generation with an un-cancelled token.
    fn begin_generation(&self) {
        *self.stop.write().expect("tunnel stop lock poisoned") = CancellationToken::new();
    }
}

async fn sleep_or_stop(inner: &Arc<ClientInner>, delay: Duration) {
    let stop = inner.stop_token();
    tokio::select! {
        _ = tokio::time::sleep(delay) => {}
        _ = stop.cancelled() => {}
    }
}

fn jittered(delay: Duration) -> Duration {
    handshake::jittered(delay, handshake::jitter_sample())
}

// ---------------------------------------------------------------------------
// One connection attempt
// ---------------------------------------------------------------------------

async fn connect_once(
    inner: Arc<ClientInner>,
    session: &CloudSessionFile,
    secret: NodeSecret,
) -> Result<(), CloseReason> {
    let ticket = handshake::fetch_channel_ticket(session)
        .await
        .map_err(|err| reason_for_error(&err))?;
    let url = handshake::tunnel_url(&session.server, &ticket)
        .ok_or(CloseReason::Unknown("bad_tunnel_url".to_string()))?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| CloseReason::Unknown("bad_request".to_string()))?;
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        TUNNEL_WS_PROTOCOL
            .parse()
            .map_err(|_| CloseReason::Unknown("bad_subprotocol".to_string()))?,
    );
    let socket = tokio::time::timeout(handshake::CONNECT_TIMEOUT, connect_async(request))
        .await
        .map_err(|_| CloseReason::Timeout)?
        .map_err(|_| CloseReason::Transport)?
        .0;

    let (writer, control_rx, data_rx) = TunnelWriter::new();
    let (mut sink, mut stream) = socket.split();
    let hello = handshake::build_hello(
        session,
        &secret,
        &ticket,
        &inner.declared_capabilities(),
        inner.resume_channel().as_deref(),
        &env!("CARGO_PKG_VERSION").to_string(),
    );
    sink.send(WsMessage::Text(
        serde_json::to_string(&hello)
            .map_err(|_| CloseReason::Unknown("hello_encode".to_string()))?,
    ))
    .await
    .map_err(|_| CloseReason::Transport)?;

    let ack = await_hello_ack(&mut stream, &inner).await?;
    writer.set_channel_id(&ack.channel_id);
    inner.set_capabilities(ack.capabilities_granted.clone());
    inner.shadow_revision.store(ack.shadow_revision.max(0), Ordering::Release);
    inner.set_resume_channel(&ack.channel_id);
    // A working handshake is a real cloud exchange: refresh the badge.
    cloud::shared().mark_online();
    let minimal = !ack
        .capabilities_granted
        .iter()
        .any(|cap| cap == CAP_SHADOW_FULL);
    let revision = inner.collector.reset(minimal);
    inner.shadow_revision.store(revision, Ordering::Release);
    *inner.writer.write().expect("tunnel state lock poisoned") = Some(writer.clone());
    inner.set_tunnel(TunnelState::Connected);
    inner.set_error(None);

    // Writer task: control frames always go out before data chunks (docs §10
    // frame priority), both queues are bounded. Protocol-level pongs travel a
    // small raw lane so the read loop can answer pings after the sink moved.
    let (pong_tx, pong_rx) = mpsc::channel::<WsMessage>(8);
    let writer_task = tokio::spawn(write_pump(
        sink,
        writer.clone(),
        control_rx,
        data_rx,
        pong_rx,
        inner.stop_token(),
    ));

    // First projection right after the ack (docs §6.2 1).
    send_full_shadow(&inner, &writer, session, revision).await;

    let heartbeat_s = inner
        .state
        .config_store
        .get()
        .await
        .interlink
        .heartbeat_s
        .max(1);
    let full_interval_s = session.interlink.shadow_interval_s().max(shadow::DELTA_MERGE_WINDOW.as_secs() * 2);

    let mut watchdog = tokio::time::interval(Duration::from_secs(
        heartbeat_s * u64::from(HEARTBEAT_MISSED_BEATS.max(1)),
    ));
    watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    watchdog.tick().await;
    let mut presence = tokio::time::interval(PRESENCE_INTERVAL);
    presence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    presence.tick().await;
    let mut delta_window = tokio::time::interval(shadow::DELTA_MERGE_WINDOW);
    delta_window.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    delta_window.tick().await;
    let mut full_timer = tokio::time::interval(Duration::from_secs(full_interval_s));
    full_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    full_timer.tick().await;
    let stop = inner.stop_token();

    let reason = loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(WsMessage::Text(text))) => {
                        if let Some(reason) = handle_inbound(&inner, &writer, &text) {
                            break reason;
                        }
                    }
                    Some(Ok(WsMessage::Ping(payload))) => {
                        if pong_tx.send(WsMessage::Pong(payload)).await.is_err() {
                            break CloseReason::Transport;
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break CloseReason::ClosedByPeer,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break CloseReason::Transport,
                }
            }
            _ = presence.tick() => {
                let payload = inner.presence_payload(now_ts());
                writer.try_send_frame(FRAME_EVENT, None, payload);
            }
            _ = delta_window.tick() => {
                pump_shadow_signals(&inner, &writer, session).await;
            }
            _ = full_timer.tick() => {
                let revision = inner.collector.next_revision();
                send_full_shadow(&inner, &writer, session, revision).await;
            }
            _ = inner.shadow_wake.notified() => {
                let revision = inner.collector.next_revision();
                send_full_shadow(&inner, &writer, session, revision).await;
            }
            _ = watchdog.tick() => {
                // Nothing at all from the server for three heartbeat periods.
                break CloseReason::Timeout;
            }
            _ = stop.cancelled() => break CloseReason::ClosedByPeer,
        }
    };

    writer_task.abort();
    inner.clear_writer(&writer);
    inner.set_capabilities(Vec::new());
    inner.approvals.clear();
    if let Some(client) = shared() {
        client.drop_all_forwarders();
    }
    let aborted = inner.ledger.abort_all().await;
    if aborted > 0 {
        tracing::debug!("[interlink] aborted {aborted} in-flight command(s) on tunnel close");
    }
    Err(reason)
}

impl ClientInner {
    fn resume_channel(&self) -> Option<String> {
        self.resume_channel
            .read()
            .expect("tunnel state lock poisoned")
            .clone()
    }

    fn set_resume_channel(&self, channel_id: &str) {
        *self
            .resume_channel
            .write()
            .expect("tunnel state lock poisoned") = Some(channel_id.to_string());
    }
}

async fn await_hello_ack(
    stream: &mut futures::stream::SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>,
    inner: &Arc<ClientInner>,
) -> Result<InterlinkHelloAck, CloseReason> {
    let read = tokio::time::timeout(handshake::CONNECT_TIMEOUT, async {
        while let Some(message) = stream.next().await {
            match message {
                Ok(WsMessage::Text(text)) => {
                    let Ok(frame) = serde_json::from_str::<InterlinkFrame>(&text) else {
                        continue;
                    };
                    if frame.kind == FRAME_HELLO_ACK {
                        return serde_json::from_value::<InterlinkHelloAck>(frame.payload)
                            .map_err(|_| CloseReason::Unknown("bad_hello_ack".to_string()));
                    }
                    if frame.kind == wunder_core::interlink::FRAME_CLOSE {
                        let reason = frame
                            .payload
                            .get("reason")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        return Err(CloseReason::parse(reason));
                    }
                    // Anything else before the ack is ignored: the protocol
                    // has nothing to say until the channel exists.
                }
                Ok(_) => continue,
                Err(_) => return Err(CloseReason::Transport),
            }
        }
        Err(CloseReason::ClosedByPeer)
    })
    .await
    .map_err(|_| CloseReason::Timeout)?;
    let ack = read?;
    let _ = inner;
    Ok(ack)
}

fn reason_for_error(err: &HandshakeError) -> CloseReason {
    match err {
        HandshakeError::Closed(reason) => reason.clone(),
        HandshakeError::Expired => CloseReason::Unknown("session_expired".to_string()),
        HandshakeError::Network => CloseReason::Transport,
        HandshakeError::Transport => CloseReason::Transport,
        HandshakeError::Timeout => CloseReason::Timeout,
        HandshakeError::Unavailable(_) => CloseReason::Unknown("endpoint_unavailable".to_string()),
        HandshakeError::Rejected(_) => CloseReason::Unknown("endpoint_rejected".to_string()),
        HandshakeError::Protocol(code) => CloseReason::Unknown((*code).to_string()),
    }
}

/// Route one inbound JSON frame. `Some(reason)` ends the connection.
fn handle_inbound(
    inner: &Arc<ClientInner>,
    writer: &Arc<TunnelWriter>,
    text: &str,
) -> Option<CloseReason> {
    let frame: InterlinkFrame = match serde_json::from_str(text) {
        Ok(frame) => frame,
        Err(_) => {
            writer.try_send_frame(FRAME_ERROR, None, json!({"message": "bad_frame"}));
            return None;
        }
    };
    match frame.kind.as_str() {
        // The server pings; the answer is a bare `pong` on the same socket.
        wunder_core::interlink::FRAME_PING | wunder_core::interlink::FRAME_PONG => {
            if frame.kind == wunder_core::interlink::FRAME_PING {
                writer.try_send_frame(FRAME_PONG, frame.corr.as_deref(), json!({}));
            }
            None
        }
        FRAME_COMMAND => {
            let Some(spec) = CommandSpec::parse(frame.corr.as_deref(), &frame.payload) else {
                writer.try_send_frame(
                    FRAME_ERROR,
                    frame.corr.as_deref(),
                    json!({"message": "bad_command"}),
                );
                return None;
            };
            if spec.control || spec.kind == CMD_COMMAND_CANCEL {
                let target = spec
                    .target_command_id
                    .clone()
                    .unwrap_or_else(|| spec.command_id.clone());
                let inner = inner.clone();
                long_task::spawn("interlink.client.command_cancel", async move {
                    inner.ledger.cancel(&target).await;
                });
                return None;
            }
            let job = spec;
            let sender = inner
                .commands
                .read()
                .expect("command sender lock poisoned")
                .clone();
            match sender.try_send(job) {
                Ok(()) => None,
                Err(_) => {
                    // Queue full, or no live generation to run it: the server
                    // owns the retry (docs §4.3).
                    let payload = json!({
                        "accepted": false,
                        "approval_state": APPROVAL_NONE,
                        "error": ERR_NODE_BUSY,
                        "retry_after_ms": 2_000,
                    });
                    writer.try_send_frame(FRAME_COMMAND_ACK, Some(frame.corr.unwrap_or_default().as_str()), payload);
                    None
                }
            }
        }
        FRAME_EVENT | EVENT_NODE_JOINED_FRAME => {
            if frame.kind == FRAME_EVENT {
                handle_event_notice(inner, writer, &frame.payload);
            }
            None
        }
        wunder_core::interlink::FRAME_CLOSE => {
            let reason = frame
                .payload
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Some(CloseReason::parse(reason))
        }
        wunder_core::interlink::FRAME_ERROR => {
            let message = frame
                .payload
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("server_error");
            inner.set_error(Some(execute::sanitize(message)));
            None
        }
        other => {
            writer.try_send_frame(
                FRAME_ERROR,
                frame.corr.as_deref(),
                json!({"message": "unsupported_frame", "type": other}),
            );
            None
        }
    }
}

/// `FRAME_EVENT` + the node.joined notice kind, kept as a local const so the
/// match above stays exhaustive-looking without a wildcard arm.
const EVENT_NODE_JOINED_FRAME: &str = EVENT_NODE_JOINED;

/// Notices the server pushes down the tunnel (docs §7.4, §5.3).
fn handle_event_notice(inner: &Arc<ClientInner>, writer: &Arc<TunnelWriter>, payload: &Value) {
    match payload.get("kind").and_then(Value::as_str) {
        Some("thread_attach") => {
            if let Some(thread_id) = payload.get("thread_id").and_then(Value::as_str) {
                inner.attach_thread(writer, thread_id);
            }
        }
        Some("thread_detach") => {
            if let Some(thread_id) = payload.get("thread_id").and_then(Value::as_str) {
                inner.detach_thread(thread_id);
            }
        }
        _ => {}
    }
}

impl ClientInner {
    /// Start forwarding one thread's durable event stream upstream. Bounded by
    /// [`MAX_THREAD_FORWARDERS`]; the feeder is the same projection the local
    /// UI consumes, so no second event source exists (docs §7.4).
    fn attach_thread(self: &Arc<Self>, writer: &Arc<TunnelWriter>, thread_id: &str) {
        let thread_id = thread_id.trim().to_string();
        if thread_id.is_empty() {
            return;
        }
        {
            let table = self.forwarders.read().expect("forwarder table lock poisoned");
            if table.contains_key(&thread_id) || table.len() >= MAX_THREAD_FORWARDERS {
                return;
            }
        }
        let cancel = CancellationToken::new();
        let already = {
            let mut table = self.forwarders.write().expect("forwarder table lock poisoned");
            if table.len() >= MAX_THREAD_FORWARDERS || table.contains_key(&thread_id) {
                false
            } else {
                table.insert(thread_id.clone(), Forwarder { cancel: cancel.clone() });
                true
            }
        };
        if !already {
            return;
        }
        let inner = self.clone();
        let writer = writer.clone();
        let owned_thread = thread_id.clone();
        long_task::spawn("interlink.client.thread_forward", async move {
            let Some(user_id) = inner.options.local_user_id.clone().into() else {
                return;
            };
            // Snapshot first, then deltas from the durable cursor.
            let (snapshot, cursor) = thread_snapshot(&inner, &user_id, &owned_thread).await;
            if !writer.try_send_frame(
                FRAME_EVENT,
                None,
                json!({
                    "kind": EVENT_THREAD,
                    "thread_id": owned_thread,
                    "seq": cursor,
                    "payload": { "snapshot": snapshot },
                }),
            ) {
                inner.forget_forwarder(&owned_thread);
                return;
            }
            let receiver = match crate::services::thread_change_feeder::watch_thread_changes(
                inner.state.clone(),
                owned_thread.clone(),
                cursor,
                Some(cancel.clone()),
            )
            .await
            {
                Ok(receiver) => receiver,
                Err(_) => {
                    inner.forget_forwarder(&owned_thread);
                    return;
                }
            };
            forward_changes(inner.clone(), writer.clone(), owned_thread.clone(), receiver, cancel)
                .await;
            inner.forget_forwarder(&owned_thread);
        });
    }

    fn detach_thread(self: &Arc<Self>, thread_id: &str) {
        let entry = {
            let mut table = self.forwarders.write().expect("forwarder table lock poisoned");
            table.remove(thread_id.trim())
        };
        if let Some(entry) = entry {
            entry.cancel.cancel();
        }
    }

    fn forget_forwarder(&self, thread_id: &str) {
        let mut table = self.forwarders.write().expect("forwarder table lock poisoned");
        table.remove(thread_id);
    }
}

async fn thread_snapshot(inner: &Arc<ClientInner>, user_id: &str, thread_id: &str) -> (Value, i64) {
    let workspace = inner.state.workspace.clone();
    let owner = user_id.to_string();
    let thread = thread_id.to_string();
    let seq_thread = thread.clone();
    let cursor = crate::core::blocking::run_db("interlink.client.snapshot_seq", move || {
        workspace.latest_thread_change_seq(&seq_thread)
    })
    .await
    .unwrap_or(0);
    let workspace = inner.state.workspace.clone();
    let snapshot = crate::core::blocking::run_db("interlink.client.snapshot", move || {
        workspace.try_load_thread_snapshot(&owner, &thread)
    })
    .await
    .ok()
    .unwrap_or(Value::Null);
    (snapshot, cursor)
}

async fn forward_changes(
    inner: Arc<ClientInner>,
    writer: Arc<TunnelWriter>,
    thread_id: String,
    mut receiver: mpsc::Receiver<crate::ThreadChangeFrame>,
    cancel: CancellationToken,
) {
    use crate::ThreadChangeFrame;
    let stop = inner.stop_token();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = stop.cancelled() => return,
            frame = receiver.recv() => {
                let Some(frame) = frame else { return };
                let payload = match &frame {
                    ThreadChangeFrame::Change { event, data, .. } => {
                        json!({ "event": event, "data": data })
                    }
                    ThreadChangeFrame::SnapshotRequired { data } => {
                        json!({ "snapshot_required": true, "data": data })
                    }
                    ThreadChangeFrame::Overflow { cursor, .. } => {
                        json!({ "overflow": true, "cursor": cursor })
                    }
                };
                // A full queue means nobody is watching: drop the frame rather
                // than back-pressure the tunnel (docs §7.4).
                writer.try_send_frame(
                    FRAME_EVENT,
                    None,
                    json!({
                        "kind": EVENT_THREAD,
                        "thread_id": thread_id,
                        "seq": frame.seq(),
                        "payload": payload,
                    }),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Outbound writer pump
// ---------------------------------------------------------------------------

async fn write_pump<S>(
    mut sink: futures::stream::SplitSink<S, WsMessage>,
    writer: Arc<TunnelWriter>,
    mut control_rx: mpsc::Receiver<OutboundFrame>,
    mut data_rx: mpsc::Receiver<OutboundFrame>,
    mut pong_rx: mpsc::Receiver<WsMessage>,
    stop: CancellationToken,
) where
    S: futures::Sink<WsMessage> + futures::Stream + Unpin + Send + 'static,
{
    loop {
        let message = tokio::select! {
            biased;
            Some(message) = pong_rx.recv() => message,
            Some(frame) = control_rx.recv() => {
                match frame {
                    OutboundFrame::Text(text) => WsMessage::Text(text.into()),
                    OutboundFrame::Binary(bytes) => WsMessage::Binary(bytes.into()),
                }
            }
            Some(frame) = data_rx.recv() => {
                match frame {
                    OutboundFrame::Text(text) => WsMessage::Text(text.into()),
                    OutboundFrame::Binary(bytes) => WsMessage::Binary(bytes.into()),
                }
            }
            _ = stop.cancelled() => return,
            else => return,
        };
        // The channel id may have been set after the frame was queued; stamp
        // it here so a frame produced before the ack still carries nothing.
        let message = match message {
            WsMessage::Text(text) => WsMessage::Text(with_channel(&writer, text).into()),
            other => other,
        };
        if sink.send(message).await.is_err() {
            return;
        }
    }
}

/// Fill `channel_id` on a frame that was built before the ack landed.
fn with_channel(writer: &Arc<TunnelWriter>, text: String) -> String {
    let Some(channel_id) = writer.channel_id() else {
        return text;
    };
    let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
        return text;
    };
    if value.get("channel_id").map(|v| v.is_null()).unwrap_or(false) {
        if let Some(map) = value.as_object_mut() {
            map.insert("channel_id".to_string(), json!(channel_id));
            return serde_json::to_string(&value).unwrap_or(text);
        }
    }
    text
}

// ---------------------------------------------------------------------------
// Shadow upload
// ---------------------------------------------------------------------------

async fn send_full_shadow(
    inner: &Arc<ClientInner>,
    writer: &Arc<TunnelWriter>,
    session: &CloudSessionFile,
    revision: i64,
) {
    let sources = inner.shadow_sources(session).await;
    let counters = shadow::RuntimeCounters {
        active_threads: inner.active_threads(),
        queued: inner.in_flight.len(),
    };
    let last_error = inner
        .last_error
        .read()
        .expect("tunnel state lock poisoned")
        .clone();
    let payload =
        shadow::build_full(&inner.state, &sources, &inner.collector, revision, counters, last_error.as_deref()).await;
    inner.shadow_revision.store(revision, Ordering::Release);
    if writer
        .send_frame(wunder_core::interlink::FRAME_SHADOW_FULL, None, payload)
        .await
        .is_err()
    {
        return;
    }
    // A queued task may have marked the collector while the projection was
    // being computed; the next window picks it up.
    inner.collector.mark_dirty(shadow::Sections::empty());
}

/// Every merge-window tick: fold the cheap signals in, then upload whatever is
/// dirty (docs §6.2 2).
async fn pump_shadow_signals(inner: &Arc<ClientInner>, writer: &Arc<TunnelWriter>, session: &CloudSessionFile) {
    // Cheap signals only until something is actually dirty: this runs every
    // merge window on an idle node, so it must not clone the config or read the
    // workspace tree just to learn there is nothing to send (docs §10.2).
    let scope = shadow::workspace_scope(
        &inner.state,
        &inner.options.local_user_id,
        inner.options.workspace_id.as_deref(),
    );
    let tree_version = inner.state.workspace.get_tree_version(&scope);
    let active = inner.active_threads() as u64;
    inner.collector.refresh_signals(tree_version, active);
    let sections = inner.collector.take_dirty();
    if sections.is_empty() {
        return;
    }
    let revision = inner.collector.next_revision();
    let sources = inner.shadow_sources(session).await;
    let counters = shadow::RuntimeCounters {
        active_threads: active as usize,
        queued: inner.in_flight.len(),
    };
    let payload = match shadow::build_delta(
        &inner.state,
        &sources,
        &inner.collector,
        sections,
        revision,
        counters,
        None,
    )
    .await
    {
        Some(payload) => payload,
        None => return,
    };
    inner.shadow_revision.store(revision, Ordering::Release);
    if writer
        .send_frame(wunder_core::interlink::FRAME_SHADOW_DELTA, None, payload)
        .await
        .is_ok()
    {
        // Nothing to do: the revision counter is already monotonic.
    }
}

// ---------------------------------------------------------------------------
// Command dispatcher
// ---------------------------------------------------------------------------

async fn command_dispatcher(inner: Arc<ClientInner>, mut receiver: mpsc::Receiver<CommandSpec>) {
    let stop = inner.stop_token();
    loop {
        let spec = tokio::select! {
            Some(spec) = receiver.recv() => spec,
            _ = stop.cancelled() => return,
            else => return,
        };
        let inner = inner.clone();
        long_task::spawn("interlink.client.command_run", async move {
            run_command(inner, spec).await;
        });
    }
}

async fn run_command(inner: Arc<ClientInner>, spec: CommandSpec) {
    let Some(writer) = inner.writer() else {
        return; // the tunnel is gone; the server already knows
    };
    let Some(session) = inner.session() else {
        return;
    };
    let now = now_ts();

    // 1) idempotency first: a replay never reaches the gate or the engine.
    match inner.ledger.classify(&spec.command_id, now).await {
        execute::Begin::Replay(result) => {
            writer
                .send_frame(FRAME_COMMAND_RESULT, Some(&spec.command_id), result)
                .await
                .ok();
            return;
        }
        execute::Begin::ReplayEmpty => return,
        execute::Begin::InFlight => return,
        execute::Begin::Fresh => {}
    }
    if !inner
        .ledger
        .claim(&spec.command_id, &spec.kind, now_ts())
        .await
    {
        return;
    }
    if !inner.in_flight.try_add(&spec.command_id, execute::INFLIGHT_MAX) {
        inner.ledger.finish(&spec.command_id, Value::Null, now_ts()).await;
        writer
            .send_frame(
                FRAME_COMMAND_ACK,
                Some(&spec.command_id),
                json!({
                    "accepted": false,
                    "approval_state": APPROVAL_NONE,
                    "error": ERR_NODE_BUSY,
                    "retry_after_ms": 2_000,
                }),
            )
            .await
            .ok();
        return;
    }

    // 2) capability + approval precedence.
    let policy = session.interlink.approval_policy();
    let preflight = execute::preflight(
        &spec,
        &inner.capabilities(),
        &inner.declared_capabilities(),
        &inner.approvals,
        policy,
        now,
    );
    let approval_state = match preflight {
        Preflight::Allow => APPROVAL_NONE.to_string(),
        Preflight::Reject(code, state) => {
            let refused = CommandReport::ack_denied(code, state);
            writer
                .send_frame(FRAME_COMMAND_ACK, Some(&spec.command_id), refused.clone())
                .await
                .ok();
            inner.in_flight.remove(&spec.command_id);
            inner.ledger.finish(&spec.command_id, refused, now_ts()).await;
            return;
        }
        Preflight::NeedPrompt => match request_decision(&inner, &writer, &spec).await {
            Some(state) => state,
            None => {
                inner.in_flight.remove(&spec.command_id);
                return;
            }
        },
    };

    // 3) accepted: ack, then execute.
    writer
        .send_frame(
            FRAME_COMMAND_ACK,
            Some(&spec.command_id),
            CommandReport::ack(true, &approval_state),
        )
        .await
        .ok();

    let ctx = inner.exec_context(&writer, &session).await;
    let cancel = inner
        .ledger
        .cancel_token_for(&spec.command_id)
        .await
        .unwrap_or_default();
    let report = execute::execute(&spec, &ctx, &writer, &inner.collector, &inner.shadow_wake, cancel).await;
    let payload = report.payload();
    writer
        .send_frame(FRAME_COMMAND_RESULT, Some(&spec.command_id), payload.clone())
        .await
        .ok();
    inner.in_flight.remove(&spec.command_id);
    inner.ledger.finish(&spec.command_id, payload, now_ts()).await;
    // Any command may have moved local state; the delta window picks it up.
    inner.collector.mark_dirty(shadow::command_sections(&spec.kind));
}

/// Ask a human; returns the `approval_state` to ack with, or `None` when the
/// prompt could not be raised or vanished.
async fn request_decision(
    inner: &Arc<ClientInner>,
    writer: &Arc<TunnelWriter>,
    spec: &CommandSpec,
) -> Option<String> {
    let now = now_ts();
    let window = approval::window_seconds(spec.approval_expires_at, spec.timeout_s, now);
    let expires_at = spec
        .approval_expires_at
        .unwrap_or_else(|| now + window);
    let approval_id = spec
        .approval_id
        .clone()
        .unwrap_or_else(|| format!("apr_local_{}", spec.command_id));
    let pending = PendingApproval {
        approval_id: approval_id.clone(),
        command_id: spec.command_id.clone(),
        kind: spec.kind.clone(),
        level: spec.level().to_string(),
        risk: spec.risk().to_string(),
        from_node: spec.from_node.clone(),
        prompt: approval::prompt_text(&spec.kind, &spec.from_node, &spec.args),
        expires_at,
    };
    let receiver = match inner.approvals.open(pending, now) {
        Ok(receiver) => receiver,
        Err(GateError::QueueFull) => {
            writer
                .send_frame(
                    FRAME_COMMAND_ACK,
                    Some(&spec.command_id),
                    CommandReport::ack(false, APPROVAL_REJECTED),
                )
                .await
                .ok();
            inner.in_flight.remove(&spec.command_id);
            return None;
        }
    };
    let stop = inner.stop_token();
    let decision = tokio::select! {
        answered = receiver => answered.ok(),
        _ = tokio::time::sleep(Duration::from_secs_f64(window.max(0.001))) => None,
        _ = stop.cancelled() => None,
    };
    let state = match decision {
        Some(Decision::Approve) => APPROVAL_APPROVED.to_string(),
        Some(Decision::Deny) | None => {
            let state = if decision.is_none() && now_ts() >= expires_at {
                APPROVAL_EXPIRED
            } else {
                APPROVAL_REJECTED
            };
            writer
                .send_frame(
                    FRAME_COMMAND_ACK,
                    Some(&spec.command_id),
                    CommandReport::ack(false, state),
                )
                .await
                .ok();
            inner.approvals.dismiss_command(&spec.command_id);
            inner.in_flight.remove(&spec.command_id);
            return None;
        }
    };
    Some(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform(legacy_windows: bool, thread_runtime_active: bool) -> NodePlatform {
        NodePlatform {
            legacy_windows,
            thread_runtime_active,
        }
    }

    #[test]
    fn l3_declaration_follows_the_build_and_platform() {
        let full = platform(false, true);
        assert_eq!(
            platform_capabilities(&full),
            vec![CAP_TOOL_EXEC, CAP_AGENT_SPAWN]
        );
        // A legacy Windows 7 build cannot bound a remote command: no tool.exec,
        // while the engine's own spawn path stays available (docs §9.2).
        let legacy = platform(true, true);
        assert_eq!(platform_capabilities(&legacy), vec![CAP_AGENT_SPAWN]);
        // No thread runtime: nothing may be spawned on this node.
        let idle = platform(false, false);
        assert_eq!(platform_capabilities(&idle), vec![CAP_TOOL_EXEC]);
        assert!(platform_capabilities(&platform(true, false)).is_empty());
    }

    #[test]
    fn declared_set_always_covers_the_lower_tiers() {
        for platform in [platform(false, true), platform(true, false)] {
            let declared = declared_capabilities(&platform);
            for cap in [
                CAP_SHADOW_FULL,
                CAP_SHADOW_MINIMAL,
                CAP_QUERY_BASIC,
                CAP_WORKSPACE_READ_BINARY,
                CAP_THREAD_DRIVE,
                CAP_WORKSPACE_WRITE,
            ] {
                assert!(declared.iter().any(|item| item == cap), "{cap} missing");
            }
        }
        let full = declared_capabilities(&platform(false, true));
        assert!(full.iter().any(|cap| cap == CAP_TOOL_EXEC));
        assert!(full.iter().any(|cap| cap == CAP_AGENT_SPAWN));
        let legacy = declared_capabilities(&platform(true, true));
        assert!(!legacy.iter().any(|cap| cap == CAP_TOOL_EXEC));
        assert!(legacy.iter().any(|cap| cap == CAP_AGENT_SPAWN));
    }
}
