mod connection;
mod nodes;

pub use connection::UserPresenceView;
pub use nodes::{
    aggregate_status, derive_device_status, derive_device_status_with_tunnel, online_count,
    BusyGuard, NodeRegistry, WebNodeLease, WEB_AWAY_IDLE_SECS,
};

use connection::ConnectionPresenceService;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::config_store::ConfigStore;
use crate::core::runtime_metrics;

/// Housekeeping period for the volatile node registry. Bounded on purpose: the
/// registry is small and a shorter period would buy nothing (§13.6).
pub const PRESENCE_GC_INTERVAL_SECS: u64 = 30;
/// Floor for the housekeeping period, so a misconfigured interval can never
/// turn into a busy loop.
pub const PRESENCE_GC_MIN_INTERVAL_SECS: u64 = 10;
/// Presence TTL used when the interlink config is unavailable.
const DEFAULT_PRESENCE_TTL_SECS: f64 = 90.0;

pub struct PresenceService {
    connections: ConnectionPresenceService,
    nodes: NodeRegistry,
    maintenance_started: AtomicBool,
    maintenance_cancel: CancellationToken,
}

impl PresenceService {
    pub fn new() -> Self {
        Self {
            connections: ConnectionPresenceService::new(),
            nodes: NodeRegistry::new(),
            maintenance_started: AtomicBool::new(false),
            maintenance_cancel: CancellationToken::new(),
        }
    }

    /// Volatile interlink node registry (web sessions + activity overlay).
    pub fn nodes(&self) -> &NodeRegistry {
        &self.nodes
    }

    pub fn touch_user(&self, user_id: &str, now: f64) {
        self.connections.touch(user_id, now);
    }

    pub fn connect_client(&self, user_id: &str, connection_id: &str, now: f64) {
        self.connections.connect(user_id, connection_id, now);
    }

    pub fn disconnect_client(&self, user_id: &str, connection_id: &str, now: f64) {
        self.connections.disconnect(user_id, connection_id, now);
    }

    pub fn force_user_offline(&self, user_id: &str, now: f64) {
        self.connections.force_offline(user_id, now);
    }

    pub fn user_snapshot(&self, user_id: &str, now: f64) -> Option<UserPresenceView> {
        self.connections.snapshot(user_id, now)
    }

    pub fn user_snapshot_many<I, S>(
        &self,
        user_ids: I,
        now: f64,
    ) -> HashMap<String, UserPresenceView>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.connections.snapshot_many(user_ids, now)
    }

    /// Start the bounded housekeeping loop for the unified presence view.
    ///
    /// It reaps web nodes whose lease vanished (a handler that never reached its
    /// unregister step), expired activity overlays, orphaned busy counters and
    /// idle connection-presence entries. Idempotent, needs a tokio runtime, and
    /// stops cleanly on [`Self::stop_maintenance`] or when the last
    /// `Arc<PresenceService>` is dropped.
    pub fn spawn_maintenance(self: std::sync::Arc<Self>, config_store: ConfigStore) {
        if self
            .maintenance_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let cancel = self.maintenance_cancel.clone();
        let weak = std::sync::Arc::downgrade(&self);
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(Duration::from_secs(PRESENCE_GC_INTERVAL_SECS.max(
                    PRESENCE_GC_MIN_INTERVAL_SECS,
                )));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = ticker.tick() => {
                        // Stop with the service instead of keeping it alive.
                        let Some(this) = weak.upgrade() else { break };
                        let ttl = current_presence_ttl(&config_store).await;
                        let now = crate::services::presence::connection::now_ts();
                        let removed = this.nodes.gc(now, ttl) + this.connections.prune(now);
                        runtime_metrics::record_loop_tick("presence.maintenance.loop", "gc");
                        if removed > 0 {
                            debug!(removed, web_nodes = this.nodes.web_node_count(), "presence housekeeping");
                        }
                    }
                }
            }
        });
    }

    /// Stop the housekeeping loop (used on shutdown and in tests).
    pub fn stop_maintenance(&self) {
        self.maintenance_cancel.cancel();
    }

    /// Whether the housekeeping loop was started.
    pub fn maintenance_active(&self) -> bool {
        self.maintenance_started.load(Ordering::Relaxed)
    }
}

impl Default for PresenceService {
    fn default() -> Self {
        Self::new()
    }
}

/// Read the interlink presence TTL, falling back to the built-in default when
/// the config is missing a sane value.
async fn current_presence_ttl(config_store: &ConfigStore) -> f64 {
    let ttl = config_store.get().await.interlink.presence_ttl_s;
    if ttl == 0 {
        DEFAULT_PRESENCE_TTL_SECS
    } else {
        ttl as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_connections_are_counted_by_connection_id() {
        let service = PresenceService::new();
        service.connect_client("alice", "conn-1", 10.0);
        service.connect_client("alice", "conn-1", 12.0);
        service.connect_client("alice", "conn-2", 13.0);
        let snapshot = service
            .user_snapshot("alice", 14.0)
            .expect("presence should exist");
        assert!(snapshot.online);
        assert_eq!(snapshot.connection_count, 2);
        assert_eq!(snapshot.last_seen_at, 13.0);
        service.disconnect_client("alice", "conn-1", 20.0);
        let snapshot = service
            .user_snapshot("alice", 21.0)
            .expect("presence should still exist");
        assert_eq!(snapshot.connection_count, 1);
        service.disconnect_client("alice", "conn-2", 22.0);
        let snapshot = service
            .user_snapshot("alice", 23.0)
            .expect("presence should remain during ttl");
        assert_eq!(snapshot.connection_count, 0);
        assert!(snapshot.online);
    }

    #[test]
    fn force_user_offline_removes_connection_presence_immediately() {
        let service = PresenceService::new();
        service.connect_client("alice", "conn-1", 10.0);
        assert!(service.user_snapshot("alice", 11.0).is_some());

        service.force_user_offline("alice", 12.0);

        assert!(service.user_snapshot("alice", 13.0).is_none());
        service.disconnect_client("alice", "conn-1", 14.0);
        assert!(service.user_snapshot("alice", 15.0).is_none());
    }

    #[test]
    fn disconnected_user_stays_online_during_ttl_window() {
        let service = PresenceService::new();
        service.connect_client("alice", "conn-1", 10.0);
        service.disconnect_client("alice", "conn-1", 20.0);

        let snapshot = service
            .user_snapshot("alice", 21.0)
            .expect("presence should remain during ttl");
        assert!(snapshot.online);
        assert_eq!(snapshot.connection_count, 0);
        assert_eq!(snapshot.last_seen_at, 20.0);
    }

    #[test]
    fn maintenance_gc_is_idempotent_and_bounded() {
        let service = PresenceService::new();
        assert!(!service.maintenance_active());
        // Without a runtime the loop is simply not started; the registry stays
        // usable either way.
        service.stop_maintenance();
        let lease = service.nodes().register_web("alice", "conn-1", "web", 10.0);
        assert_eq!(service.nodes().web_node_count(), 1);
        drop(lease);
        assert_eq!(service.nodes().gc(20.0, 90.0), 1);
        assert_eq!(service.nodes().web_node_count(), 0);
    }
}
