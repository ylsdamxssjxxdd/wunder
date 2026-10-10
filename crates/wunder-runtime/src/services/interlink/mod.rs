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

/// Workspace shadow pipeline (I5): frame -> `interlink_node_shadows`
/// projection with revision monotonicity and delta merge.
pub mod shadow;

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
}

#[derive(Debug, Default)]
struct RegistryState {
    /// `device_id -> live channel` (single-link model by default).
    channels: HashMap<String, LiveChannel>,
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
    /// `superseded` (docs §4.1 "new chain supersedes old").
    pub fn register(&self, channel: LiveChannel) -> Option<LiveChannel> {
        let key = channel.device_id.clone();
        let mut state = self
            .state
            .write()
            .expect("live channel registry lock poisoned");
        state.channels.insert(key, channel)
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
        }
        owns
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
        }
    }

    #[test]
    fn register_returns_superseded_channel() {
        let reg = LiveChannelRegistry::new();
        assert!(reg.register(channel("d1", "ch_a", 10.0)).is_none());
        let old = reg.register(channel("d1", "ch_b", 11.0)).expect("superseded");
        assert_eq!(old.channel_id, "ch_a");
        assert_eq!(reg.by_device("d1").expect("live").channel_id, "ch_b");
        assert_eq!(reg.count(), 1);
    }

    #[test]
    fn stale_handler_cannot_evict_newer_channel() {
        let reg = LiveChannelRegistry::new();
        reg.register(channel("d1", "ch_a", 10.0));
        reg.register(channel("d1", "ch_b", 11.0));

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
        reg.register(channel("d1", "ch_a", 10.0));
        reg.register(channel("d2", "ch_b", 200.0));
        assert_eq!(reg.count_for_device("d1"), 1);
        assert_eq!(reg.snapshot().len(), 2);

        let removed = reg.gc(300.0, 90.0);
        assert_eq!(removed, 1);
        assert!(reg.by_device("d1").is_none());
        assert!(reg.by_device("d2").is_some());
    }
}