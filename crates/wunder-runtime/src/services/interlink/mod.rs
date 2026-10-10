//! Interlink tunnel live-channel registry (I4).
//!
//! Process-local authority for the active cloud<->local tunnel channels: maps a
//! persistent device (`device:<id>`) to the single channel it currently holds.
//! The `interlink_channels` table is only a durable snapshot; this registry is
//! the live source of truth for the running instance (docs §3.1, §4.1).
//!
//! Presence wiring (kept minimal for I4): a tunnel up/down also flips the
//! persisted `cloud_devices.tunnel_connected` flag through
//! `update_cloud_device_interlink`, which is exactly what
//! `GET /wunder/interlink/nodes` reads to report the node `connected` field.
//! We deliberately avoid a second in-memory presence path so the node list
//! cannot drift from the persisted device row.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use serde::Serialize;
use tokio::sync::mpsc;

/// Human-in-the-loop approval tickets (docs §7.3).
pub mod approvals;
/// Append-only audit trail + argument digests (docs §9.4).
pub mod audit;
/// Bounded tunnel data-plane buffer for large remote file reads (docs §6.4).
pub mod blob;
/// Outbound tunnel client for a local node (docs §2.2, §2.3): lifecycle,
/// presence beat, remote forwarding; used by 蜂窝 and 舵机.
pub mod client;
/// Remote command ledger: idempotency, state machine, inflight bound,
/// offline queue, timeout sweep (docs §4.3).
pub mod commands;
/// Argument digests, per-device policy overrides and approval prompts
/// (docs §9.2, §9.3).
pub mod digest;
/// Periodic maintenance: stale channels, command timeouts, approval expiry,
/// ledger/audit retention (docs §4.4, §9.4, §10).
pub mod janitor;
/// Remote session view fan-out: local thread events -> N web subscribers with
/// a single upstream per (device, thread) (docs §7.4).
pub mod remote;
/// Workspace shadow pipeline (I5): frame -> `interlink_node_shadows`
/// projection with revision monotonicity and delta merge.
pub mod shadow;
/// Node secret issuance, hashing and handshake signing (docs §4.1, §9.1).
pub mod secret;

/// A frame the server pushes down the tunnel towards one local node.
#[derive(Debug, Clone)]
pub enum OutboundFrame {
    Text(String),
    Binary(Vec<u8>),
}

/// Hard bound of the per-channel outbound queue (docs §10.1: every queue is
/// bounded; a full queue means the node is busy, never an unbounded backlog).
pub const OUTBOUND_QUEUE_CAPACITY: usize = 256;

/// One live tunnel channel held by a device.
#[derive(Debug, Clone, Serialize)]
pub struct LiveChannel {
    pub device_id: String,
    pub user_id: String,
    /// Client flavor reported by the device row (`desktop` / `cli` / ...).
    pub client: String,
    /// Per-connection channel id (`ch_<uuid>`), echoed in the frame envelope.
    pub channel_id: String,
    /// Websocket connection id (`itl_<uuid>`); unique per socket.
    pub connection_id: String,
    pub protocol_version: i64,
    /// Effective capability set granted for this session.
    pub capabilities: Vec<String>,
    pub connected_at: f64,
    pub last_seen_at: f64,
    pub rtt_ms: Option<i64>,
    /// Application-level status reported by the node's presence beat
    /// (`online`/`busy`/`away`, docs §5.1). `None` = the node never said.
    pub presence_status: Option<String>,
    /// Threads the node is running right now, from the same beat.
    pub active_threads: i64,
    /// True once the node asked to resume a dropped channel (docs §4.4).
    pub resumed: bool,
}

#[derive(Debug, Default)]
struct RegistryState {
    /// `device_id -> live channel` (single-link model by default).
    channels: HashMap<String, LiveChannel>,
    /// `device_id -> bounded outbound queue` owned by the live socket task.
    outbound: HashMap<String, mpsc::Sender<OutboundFrame>>,
}

/// Why a frame could not be handed to a node's tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchError {
    /// The device holds no live tunnel.
    NoChannel,
    /// The tunnel exists but its bounded queue is full (node is saturated).
    QueueFull,
    /// The slot is now owned by a different (superseding) channel.
    Superseded,
}

impl DispatchError {
    pub fn code(self) -> &'static str {
        match self {
            DispatchError::NoChannel => "NODE_OFFLINE",
            DispatchError::QueueFull => "NODE_BUSY",
            DispatchError::Superseded => "CHANNEL_SUPERSEDED",
        }
    }
}

/// Live channel registry, `RwLock`-guarded following the presence registry
/// style (`services/presence/connection.rs`).
#[derive(Debug, Default)]
pub struct LiveChannelRegistry {
    state: RwLock<RegistryState>,
}

impl LiveChannelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a freshly opened channel for a device, returning the channel it
    /// superseded (if any) so the caller can retire it with reason
    /// `superseded` (docs §4.1 "new chain supersedes old"). The outbound queue
    /// is swapped together with the channel, so a superseded socket can never
    /// be handed a new command.
    pub fn register(
        &self,
        channel: LiveChannel,
        outbound: mpsc::Sender<OutboundFrame>,
    ) -> Option<LiveChannel> {
        let key = channel.device_id.clone();
        let mut state = self
            .state
            .write()
            .expect("live channel registry lock poisoned");
        state.outbound.insert(key.clone(), outbound);
        state.channels.insert(key, channel)
    }

    /// Allocate the bounded queue a new socket task should register with.
    pub fn outbound_channel() -> (mpsc::Sender<OutboundFrame>, mpsc::Receiver<OutboundFrame>) {
        mpsc::channel(OUTBOUND_QUEUE_CAPACITY)
    }

    /// Push one frame into the live tunnel of `device_id` without blocking.
    ///
    /// Command dispatch never awaits the socket: a stalled node must not hold
    /// the issuing request open (docs §10.2 - the tunnel never blocks the
    /// engine's main path).
    pub fn dispatch(
        &self,
        device_id: &str,
        channel_id: &str,
        frame: OutboundFrame,
    ) -> Result<(), DispatchError> {
        let state = self
            .state
            .read()
            .expect("live channel registry lock poisoned");
        let live = match state.channels.get(device_id) {
            Some(live) if live.channel_id == channel_id => live,
            Some(_) => return Err(DispatchError::Superseded),
            None => return Err(DispatchError::NoChannel),
        };
        let _ = live;
        match state.outbound.get(device_id) {
            Some(sender) => match sender.try_send(frame) {
                Ok(()) => Ok(()),
                Err(mpsc::error::TrySendError::Full(_)) => Err(DispatchError::QueueFull),
                Err(mpsc::error::TrySendError::Closed(_)) => Err(DispatchError::Superseded),
            },
            None => Err(DispatchError::NoChannel),
        }
    }

    /// Refresh liveness for the channel that currently owns `device_id`.
    ///
    /// Returns `false` when a different channel now owns the device (the caller
    /// was superseded or already unregistered) so it can stop its loop.
    pub fn heartbeat(
        &self,
        device_id: &str,
        channel_id: &str,
        now: f64,
        rtt_ms: Option<i64>,
    ) -> bool {
        let mut state = self
            .state
            .write()
            .expect("live channel registry lock poisoned");
        match state.channels.get_mut(device_id) {
            Some(entry) if entry.channel_id == channel_id => {
                entry.last_seen_at = now;
                if rtt_ms.is_some() {
                    entry.rtt_ms = rtt_ms;
                }
                true
            }
            _ => false,
        }
    }

    /// Remove the channel only while it still owns the device, so a superseding
    /// channel is never evicted by a stale handler. Returns whether the entry
    /// was actually removed.
    pub fn unregister(&self, device_id: &str, channel_id: &str) -> bool {
        let mut state = self
            .state
            .write()
            .expect("live channel registry lock poisoned");
        let owns = state
            .channels
            .get(device_id)
            .map(|entry| entry.channel_id == channel_id)
            .unwrap_or(false);
        if owns {
            state.channels.remove(device_id);
            state.outbound.remove(device_id);
        }
        owns
    }

    /// Apply one presence beat (docs §4.4/§5.1) to the owning channel.
    /// Returns `false` when the caller no longer owns the device slot.
    pub fn set_presence(
        &self,
        device_id: &str,
        channel_id: &str,
        status: &str,
        active_threads: i64,
        now: f64,
    ) -> bool {
        let mut state = self
            .state
            .write()
            .expect("live channel registry lock poisoned");
        match state.channels.get_mut(device_id) {
            Some(entry) if entry.channel_id == channel_id => {
                entry.presence_status = Some(status.to_string());
                entry.active_threads = active_threads;
                entry.last_seen_at = now;
                true
            }
            _ => false,
        }
    }

    /// Mark the channel as a resumed one (docs §4.4 session recovery).
    pub fn mark_resumed(&self, device_id: &str, channel_id: &str) -> bool {
        let mut state = self
            .state
            .write()
            .expect("live channel registry lock poisoned");
        match state.channels.get_mut(device_id) {
            Some(entry) if entry.channel_id == channel_id => {
                entry.resumed = true;
                true
            }
            _ => false,
        }
    }

    /// Snapshot of every live channel.
    pub fn snapshot(&self) -> Vec<LiveChannel> {
        let state = self
            .state
            .read()
            .expect("live channel registry lock poisoned");
        state.channels.values().cloned().collect()
    }

    /// The live channel for one device, if any.
    pub fn by_device(&self, device_id: &str) -> Option<LiveChannel> {
        let state = self
            .state
            .read()
            .expect("live channel registry lock poisoned");
        state.channels.get(device_id).cloned()
    }

    /// Number of live channels.
    pub fn count(&self) -> usize {
        let state = self
            .state
            .read()
            .expect("live channel registry lock poisoned");
        state.channels.len()
    }

    /// Number of live channels for one device (0 or 1 in the default model;
    /// kept explicit so `max_channels_per_device` checks stay readable).
    pub fn count_for_device(&self, device_id: &str) -> usize {
        let state = self
            .state
            .read()
            .expect("live channel registry lock poisoned");
        usize::from(state.channels.contains_key(device_id))
    }

    /// Drop channels whose last heartbeat is older than `retain_s`. GC style
    /// mirrors the presence service; returns the number of removed entries.
    pub fn gc(&self, now: f64, retain_s: f64) -> usize {
        let mut state = self
            .state
            .write()
            .expect("live channel registry lock poisoned");
        let before = state.channels.len();
        state
            .channels
            .retain(|_, entry| now - entry.last_seen_at <= retain_s);
        let alive: Vec<String> = state.channels.keys().cloned().collect();
        state
            .outbound
            .retain(|device_id, _| alive.iter().any(|kept| kept == device_id));
        before - state.channels.len()
    }
}

/// Process-global registry shared by the tunnel ws handlers. Mirrors
/// `desktop_lan::manager()`; `AppState` is intentionally left untouched in I4.
pub fn registry() -> &'static LiveChannelRegistry {
    static INSTANCE: OnceLock<LiveChannelRegistry> = OnceLock::new();
    INSTANCE.get_or_init(LiveChannelRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(device_id: &str, channel_id: &str, now: f64) -> LiveChannel {
        LiveChannel {
            device_id: device_id.to_string(),
            user_id: "u1".to_string(),
            client: "desktop".to_string(),
            channel_id: channel_id.to_string(),
            connection_id: format!("itl_{channel_id}"),
            protocol_version: 1,
            capabilities: Vec::new(),
            connected_at: now,
            last_seen_at: now,
            rtt_ms: None,
            presence_status: None,
            active_threads: 0,
            resumed: false,
        }
    }

    fn queue() -> (mpsc::Sender<OutboundFrame>, mpsc::Receiver<OutboundFrame>) {
        LiveChannelRegistry::outbound_channel()
    }

    #[test]
    fn register_returns_superseded_channel() {
        let reg = LiveChannelRegistry::new();
        let (tx_a, _rx_a) = queue();
        let (tx_b, _rx_b) = queue();
        assert!(reg.register(channel("d1", "ch_a", 10.0), tx_a).is_none());
        let old = reg
            .register(channel("d1", "ch_b", 11.0), tx_b)
            .expect("superseded");
        assert_eq!(old.channel_id, "ch_a");
        assert_eq!(reg.by_device("d1").expect("live").channel_id, "ch_b");
        assert_eq!(reg.count(), 1);
    }

    #[test]
    fn stale_handler_cannot_evict_newer_channel() {
        let reg = LiveChannelRegistry::new();
        let (tx_a, _rx_a) = queue();
        let (tx_b, _rx_b) = queue();
        reg.register(channel("d1", "ch_a", 10.0), tx_a);
        reg.register(channel("d1", "ch_b", 11.0), tx_b);

        // The superseded channel stops on heartbeat and cannot unregister.
        assert!(!reg.heartbeat("d1", "ch_a", 12.0, None));
        assert!(!reg.unregister("d1", "ch_a"));
        assert_eq!(reg.by_device("d1").expect("live").channel_id, "ch_b");

        assert!(reg.heartbeat("d1", "ch_b", 12.5, Some(7)));
        assert_eq!(reg.by_device("d1").expect("live").rtt_ms, Some(7));
        assert!(reg.unregister("d1", "ch_b"));
        assert_eq!(reg.count(), 0);
    }

    #[test]
    fn snapshot_and_gc_drop_stale_channels() {
        let reg = LiveChannelRegistry::new();
        let (tx_a, _rx_a) = queue();
        let (tx_b, _rx_b) = queue();
        reg.register(channel("d1", "ch_a", 10.0), tx_a);
        reg.register(channel("d2", "ch_b", 200.0), tx_b);
        assert_eq!(reg.count_for_device("d1"), 1);
        assert_eq!(reg.snapshot().len(), 2);

        let removed = reg.gc(285.0, 90.0);
        assert_eq!(removed, 1);
        assert!(reg.by_device("d1").is_none());
        assert!(reg.by_device("d2").is_some());
    }

    #[test]
    fn dispatch_targets_only_the_owning_channel() {
        let reg = LiveChannelRegistry::new();
        let (tx_a, mut rx_a) = queue();
        let (tx_b, mut rx_b) = queue();
        reg.register(channel("d1", "ch_a", 10.0), tx_a);

        assert!(reg
            .dispatch("d1", "ch_a", OutboundFrame::Text("a".to_string()))
            .is_ok());
        // A stale channel id never reaches the live queue.
        assert_eq!(
            reg.dispatch("d1", "ch_stale", OutboundFrame::Text("b".to_string())),
            Err(DispatchError::Superseded)
        );
        assert_eq!(
            reg.dispatch("d2", "ch_x", OutboundFrame::Text("c".to_string())),
            Err(DispatchError::NoChannel)
        );
        assert_eq!(rx_a.try_recv().expect("frame").code(), 'a');

        // Superseding swaps the queue atomically: the old receiver drains empty.
        reg.register(channel("d1", "ch_b", 11.0), tx_b);
        assert!(reg
            .dispatch("d1", "ch_a", OutboundFrame::Text("z".to_string()))
            .is_err());
        assert!(reg
            .dispatch("d1", "ch_b", OutboundFrame::Text("y".to_string()))
            .is_ok());
        assert_eq!(rx_b.try_recv().expect("frame").code(), 'y');
    }

    #[test]
    fn dispatch_reports_queue_full_when_saturated() {
        let reg = LiveChannelRegistry::new();
        let (tx, _rx) = mpsc::channel(1);
        reg.register(channel("d1", "ch_a", 10.0), tx);

        assert!(reg
            .dispatch("d1", "ch_a", OutboundFrame::Text("1".to_string()))
            .is_ok());
        assert_eq!(
            reg.dispatch("d1", "ch_a", OutboundFrame::Text("2".to_string())),
            Err(DispatchError::QueueFull)
        );
    }

    #[test]
    fn gc_removes_outbound_queues_of_dropped_channels() {
        let reg = LiveChannelRegistry::new();
        let (tx_a, rx_a) = queue();
        reg.register(channel("d1", "ch_a", 10.0), tx_a);
        assert_eq!(reg.gc(300.0, 90.0), 1);
        // The queue receiver is closed once the registry drops its sender.
        assert!(rx_a.is_closed());
    }

    #[test]
    fn presence_beat_updates_only_the_owning_channel() {
        let reg = LiveChannelRegistry::new();
        let (tx, _rx) = queue();
        reg.register(channel("d1", "ch_a", 10.0), tx);

        assert!(reg.set_presence("d1", "ch_a", "busy", 2, 11.0));
        let live = reg.by_device("d1").expect("live");
        assert_eq!(live.presence_status.as_deref(), Some("busy"));
        assert_eq!(live.active_threads, 2);
        assert_eq!(live.last_seen_at, 11.0);

        // A superseded channel can never rewrite the node status.
        let (tx_b, _rx_b) = queue();
        reg.register(channel("d1", "ch_b", 12.0), tx_b);
        assert!(!reg.set_presence("d1", "ch_a", "away", 0, 13.0));
        assert!(reg.set_presence("d1", "ch_b", "away", 0, 13.0));
        assert!(!reg.mark_resumed("d1", "ch_a"));
        assert!(!reg.by_device("d1").expect("live").resumed);
        assert!(reg.mark_resumed("d1", "ch_b"));
        assert!(reg.by_device("d1").expect("live").resumed);
    }

    impl OutboundFrame {
        fn code(&self) -> char {
            match self {
                OutboundFrame::Text(text) => text.chars().next().unwrap_or(' '),
                OutboundFrame::Binary(bytes) => bytes.first().copied().unwrap_or(b' ') as char,
            }
        }
    }
}
