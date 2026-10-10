//! Interlink node registry (I2).
//!
//! Unified presence view over three kinds of nodes: persistent cloud devices
//! (`device:<id>`), the server node (`cloud`) and volatile web sessions
//! (`web:<connection_id>`). Persistent nodes are supplied by the caller from
//! storage; this registry owns only the ephemeral web nodes plus a live
//! activity overlay applied uniformly to any node id.
//!
//! See docs/云端本地互通方案.md §2.4 (identity) and §5.1 (status model).
//!
//! Liveness model for web nodes (§2.4 "既有 chat_ws/core_ws 连接存活"): a web
//! node is alive while the websocket handler that registered it still holds the
//! returned [`WebNodeLease`]. The lease is an `Arc` the handler owns, so a
//! dropped or aborted handler releases it implicitly and `gc()` reaps the node
//! even when the unregister step never ran. No heartbeat traffic is needed, so
//! presence costs nothing per message.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, Weak};

use serde_json::json;
use wunder_core::interlink::{
    InterlinkNodeView, NODE_STATUS_AWAY, NODE_STATUS_BUSY, NODE_STATUS_OFFLINE, NODE_STATUS_ONLINE,
    NODE_STATUS_RECONNECTING, NODE_TYPE_WEB,
};

/// Idle window after which a connected web session degrades to `away`
/// (docs §5.1: `页面隐藏>10min`). The hive client sends no visibility signal
/// over its websocket, so idleness is derived server-side from the last user
/// activity (message send / thread start) instead of inventing a protocol.
pub const WEB_AWAY_IDLE_SECS: f64 = 600.0;

/// Live activity overlay for a node. `busy` / `away` are only meaningful while
/// the reported activity is fresh (within the presence TTL).
#[derive(Debug, Clone, PartialEq)]
struct NodeActivity {
    busy: bool,
    away: bool,
    updated_at: f64,
}

/// A browser node that only exists while a websocket connection is alive.
#[derive(Debug, Clone)]
struct WebNodeEntry {
    user_id: String,
    connection_id: String,
    label: String,
    /// Last user activity on this session (connect, message send, turn start).
    /// Drives the `away` idle rule; liveness comes from [`WebNodeEntry::lease`].
    last_seen_at: f64,
    /// Weak view of the owning connection's lease. `None` is a lease-less
    /// registration (a caller that mirrors an external liveness signal) which
    /// falls back to TTL freshness.
    lease: Option<Weak<()>>,
}

impl WebNodeEntry {
    fn is_alive(&self, now: f64, ttl: f64) -> bool {
        match &self.lease {
            Some(weak) => weak.strong_count() > 0,
            None => (now - self.last_seen_at).max(0.0) <= ttl,
        }
    }

    /// Node ids are `web:<connection_id>`; keep the derivation next to the entry.
    fn node_id(&self) -> String {
        NodeRegistry::web_node_id(&self.connection_id)
    }
}

/// Kept alive by the websocket handler that registered the session. Dropping it
/// marks the node dead, so both the orderly close path and an aborted handler
/// converge on the same "gone" outcome.
#[derive(Debug)]
#[must_use = "a web node is only reported online while its lease is held"]
pub struct WebNodeLease {
    node_id: String,
    _keep_alive: Arc<()>,
}

impl WebNodeLease {
    /// Node id of the registered session (`web:<connection_id>`).
    pub fn node_id(&self) -> &str {
        &self.node_id
    }
}

/// Releases the busy marker of one running turn when dropped. Reference
/// counted, so a connection multiplexing several turns stays `busy` until the
/// last one finishes; every exit path (completion, error, cancel, abort) drops
/// the guard.
#[derive(Debug)]
#[must_use = "the node stays busy only while this guard is held"]
pub struct BusyGuard {
    depth: Arc<AtomicUsize>,
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.depth.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Volatile node registry.
#[derive(Debug, Default)]
pub struct NodeRegistry {
    web_nodes: RwLock<HashMap<String, WebNodeEntry>>,
    activity: RwLock<HashMap<String, NodeActivity>>,
    /// `node_id -> running turns`; kept separate from [`NodeActivity`] because a
    /// running turn has no TTL (it ends when its task ends).
    busy_depth: RwLock<HashMap<String, Arc<AtomicUsize>>>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Node id for a web session: `web:<connection_id>`.
    pub fn web_node_id(connection_id: &str) -> String {
        format!("{NODE_TYPE_WEB}:{connection_id}")
    }

    /// Register (or refresh) a web node owned by `user_id`.
    ///
    /// The returned lease must be held for the lifetime of the connection.
    pub fn register_web(&self, user_id: &str, connection_id: &str, label: &str, now: f64) -> WebNodeLease {
        let keep_alive = Arc::new(());
        let entry = WebNodeEntry {
            user_id: user_id.to_string(),
            connection_id: connection_id.to_string(),
            label: label.to_string(),
            last_seen_at: now,
            lease: Some(Arc::downgrade(&keep_alive)),
        };
        self.web_nodes
            .write()
            .expect("node registry lock poisoned")
            .insert(connection_id.to_string(), entry);
        WebNodeLease {
            node_id: Self::web_node_id(connection_id),
            _keep_alive: keep_alive,
        }
    }

    /// Refresh the last-activity timestamp of an existing web node.
    ///
    /// Called on the connect and user-action paths only; a no-op for unknown
    /// connections so a stale handler cannot resurrect a reaped node. An
    /// explicit `away` report is cleared here: user activity means not idle.
    pub fn touch_web(&self, connection_id: &str, now: f64) {
        let node_id = Self::web_node_id(connection_id);
        let known = {
            let mut web = self
                .web_nodes
                .write()
                .expect("node registry lock poisoned");
            match web.get_mut(connection_id) {
                Some(entry) => {
                    entry.last_seen_at = now;
                    true
                }
                None => false,
            }
        };
        if !known {
            return;
        }
        let mut activity = self
            .activity
            .write()
            .expect("node registry lock poisoned");
        if activity
            .get(&node_id)
            .is_some_and(|entry| entry.away && !entry.busy)
        {
            activity.remove(&node_id);
        }
    }

    /// Drop a web node (websocket closed).
    pub fn unregister_web(&self, connection_id: &str) {
        self.web_nodes
            .write()
            .expect("node registry lock poisoned")
            .remove(connection_id);
        self.activity
            .write()
            .expect("node registry lock poisoned")
            .remove(&Self::web_node_id(connection_id));
        // Busy depth is intentionally left alone: a running turn still holds its
        // guard, and `gc()` reaps the counter once the last guard is dropped.
    }

    /// Report live activity for any node id. Passing `busy = false, away = false`
    /// clears the overlay.
    pub fn report_activity(&self, node_id: &str, busy: bool, away: bool, now: f64) {
        let mut activity = self
            .activity
            .write()
            .expect("node registry lock poisoned");
        if !busy && !away {
            activity.remove(node_id);
            return;
        }
        activity.insert(
            node_id.to_string(),
            NodeActivity {
                busy,
                away,
                updated_at: now,
            },
        );
    }

    /// Explicitly clear the activity overlay for a node.
    pub fn clear_activity(&self, node_id: &str) {
        self.activity
            .write()
            .expect("node registry lock poisoned")
            .remove(node_id);
    }

    /// Mark a node busy for as long as the returned guard lives (one running
    /// turn). Use [`Self::report_activity`] for activity reported by a peer
    /// (the desktop beat in I5) instead.
    pub fn begin_busy(&self, node_id: &str) -> BusyGuard {
        let depth = {
            let mut busy = self
                .busy_depth
                .write()
                .expect("node registry lock poisoned");
            Arc::clone(
                busy.entry(node_id.to_string())
                    .or_insert_with(|| Arc::new(AtomicUsize::new(0))),
            )
        };
        depth.fetch_add(1, Ordering::AcqRel);
        BusyGuard { depth }
    }

    /// Number of live web nodes (observability + bounds checking).
    pub fn web_node_count(&self) -> usize {
        self.web_nodes
            .read()
            .expect("node registry lock poisoned")
            .len()
    }

    /// Garbage collect stale web nodes, activity entries and busy counters.
    ///
    /// Web nodes with a lease are dropped once the lease is gone (the handler
    /// exited or was aborted), lease-less nodes once they exceed `2 * ttl`;
    /// activity is dropped once it exceeds `ttl`; busy counters are dropped
    /// once no guard still holds them. Returns the number of removed entries.
    pub fn gc(&self, now: f64, ttl: f64) -> usize {
        let mut removed = 0usize;
        {
            let mut web = self
                .web_nodes
                .write()
                .expect("node registry lock poisoned");
            let before = web.len();
            web.retain(|_, entry| match &entry.lease {
                Some(weak) => weak.strong_count() > 0,
                None => now - entry.last_seen_at <= ttl * 2.0,
            });
            removed += before - web.len();
        }
        {
            let mut activity = self
                .activity
                .write()
                .expect("node registry lock poisoned");
            let before = activity.len();
            activity.retain(|_, entry| now - entry.updated_at <= ttl);
            removed += before - activity.len();
        }
        {
            let mut busy = self
                .busy_depth
                .write()
                .expect("node registry lock poisoned");
            // The registry itself holds one strong count; more than one means a
            // guard (a running turn) is still alive.
            let before = busy.len();
            busy.retain(|_, depth| Arc::strong_count(depth) > 1);
            removed += before - busy.len();
        }
        removed
    }

    /// Compose the full node list for a user: the caller-provided persistent
    /// nodes (devices + server) with the activity overlay applied, followed by
    /// the user's live web nodes.
    pub fn compose_user_nodes(
        &self,
        user_id: &str,
        now: f64,
        ttl: f64,
        persistent: Vec<InterlinkNodeView>,
    ) -> Vec<InterlinkNodeView> {
        let activity = self
            .activity
            .read()
            .expect("node registry lock poisoned")
            .clone();
        let busy = self
            .busy_depth
            .read()
            .expect("node registry lock poisoned")
            .clone();
        let web = self
            .web_nodes
            .read()
            .expect("node registry lock poisoned")
            .clone();

        let mut views: Vec<InterlinkNodeView> = persistent;
        for node in views.iter_mut() {
            overlay_activity(node, &activity, &busy, now, ttl);
        }

        for entry in web.values() {
            if entry.user_id != user_id {
                continue;
            }
            views.push(web_node_view(entry, &activity, &busy, now, ttl));
        }

        views.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        views
    }
}

/// Live busy signal for a node id (running turns reported by this process).
fn busy_depth(busy: &HashMap<String, Arc<AtomicUsize>>, node_id: &str) -> usize {
    busy.get(node_id)
        .map(|depth| depth.load(Ordering::Acquire))
        .unwrap_or(0)
}

/// Apply the activity overlay to a node, respecting the freshness window.
fn overlay_activity(
    node: &mut InterlinkNodeView,
    activity: &HashMap<String, NodeActivity>,
    busy: &HashMap<String, Arc<AtomicUsize>>,
    now: f64,
    ttl: f64,
) {
    if busy_depth(busy, &node.node_id) > 0 {
        node.status = NODE_STATUS_BUSY.to_string();
        return;
    }
    let Some(entry) = activity.get(&node.node_id) else {
        return;
    };
    if now - entry.updated_at > ttl {
        return;
    }
    if entry.busy {
        node.status = NODE_STATUS_BUSY.to_string();
    } else if entry.away && node.status == NODE_STATUS_ONLINE {
        node.status = NODE_STATUS_AWAY.to_string();
    }
}

/// Build the view for a volatile web node from its registry entry.
fn web_node_view(
    entry: &WebNodeEntry,
    activity: &HashMap<String, NodeActivity>,
    busy: &HashMap<String, Arc<AtomicUsize>>,
    now: f64,
    ttl: f64,
) -> InterlinkNodeView {
    let node_id = entry.node_id();
    let alive = entry.is_alive(now, ttl);
    let idle = (now - entry.last_seen_at).max(0.0);
    let mut status = NODE_STATUS_OFFLINE;
    if alive {
        status = if busy_depth(busy, &node_id) > 0 {
            NODE_STATUS_BUSY
        } else if activity_is_away(activity, &node_id, now, ttl) || idle > WEB_AWAY_IDLE_SECS {
            NODE_STATUS_AWAY
        } else {
            NODE_STATUS_ONLINE
        };
    }

    InterlinkNodeView {
        node_id,
        node_type: NODE_TYPE_WEB.to_string(),
        user_id: entry.user_id.clone(),
        label: entry.label.clone(),
        status: status.to_string(),
        last_seen_at: entry.last_seen_at,
        capabilities: Vec::new(),
        shadow_revision: 0,
        connected: alive,
        meta: json!({}),
    }
}

fn activity_is_away(
    activity: &HashMap<String, NodeActivity>,
    node_id: &str,
    now: f64,
    ttl: f64,
) -> bool {
    activity
        .get(node_id)
        .filter(|entry| now - entry.updated_at <= ttl)
        .is_some_and(|entry| entry.away && !entry.busy)
}

/// Derive a persistent device node status from heartbeat freshness only.
///
/// This is the §4.4 fallback for a device that never opened a tunnel
/// (未开隧道时退化为日志心跳):
/// - `age <= ttl` -> online
/// - `age <= 2*ttl` -> reconnecting
/// - otherwise -> offline
///
/// When a tunnel signal is available, use [`derive_device_status_with_tunnel`].
pub fn derive_device_status(now: f64, last_seen_at: f64, ttl: f64) -> &'static str {
    let age = (now - last_seen_at).max(0.0);
    if age <= ttl {
        NODE_STATUS_ONLINE
    } else if age <= ttl * 2.0 {
        NODE_STATUS_RECONNECTING
    } else {
        NODE_STATUS_OFFLINE
    }
}

/// Derive a persistent device node status from the live tunnel signal plus the
/// heartbeat freshness (§5.1: `online = 隧道活 + beat 正常`,
/// `reconnecting = TTL 未过期但通道断`).
///
/// `tunnel_live` is a tri-state so a device without any tunnel keeps the §4.4
/// heartbeat fallback instead of being pinned to `reconnecting`:
/// - `Some(true)`: the live channel registry currently holds a channel for the
///   device (`services::interlink::registry().by_device(&device_id).is_some()`).
/// - `Some(false)`: a channel was established for the device and is now gone
///   (`cloud_devices.interlink.tunnel_connected == false` after a close).
/// - `None`: no tunnel signal at all (interlink disabled, or the device never
///   opened a tunnel) -> freshness decides.
///
/// Callers in `api/interlink.rs` are expected to pass
/// `if registry().by_device(&device_id).is_some() { Some(true) } else { row_tunnel_connected }`
/// where `row_tunnel_connected` is the persisted `cloud_devices.interlink
/// .tunnel_connected` (`Option<bool>`). This function stays pure so the
/// presence rules are testable without the tunnel.
pub fn derive_device_status_with_tunnel(
    now: f64,
    last_seen_at: f64,
    ttl: f64,
    tunnel_live: Option<bool>,
) -> &'static str {
    match tunnel_live {
        Some(true) => NODE_STATUS_ONLINE,
        Some(false) => {
            if (now - last_seen_at).max(0.0) <= ttl {
                NODE_STATUS_RECONNECTING
            } else {
                NODE_STATUS_OFFLINE
            }
        }
        None => derive_device_status(now, last_seen_at, ttl),
    }
}

/// Aggregate per plan §5.1: any node online/busy -> online; all away -> away;
/// otherwise offline. Empty lists are offline.
pub fn aggregate_status<'a, I>(nodes: I) -> &'static str
where
    I: IntoIterator<Item = &'a InterlinkNodeView>,
{
    let mut any_away = false;
    for node in nodes {
        match node.status.as_str() {
            NODE_STATUS_ONLINE | NODE_STATUS_BUSY => return NODE_STATUS_ONLINE,
            NODE_STATUS_AWAY => any_away = true,
            _ => {}
        }
    }
    if any_away {
        return NODE_STATUS_AWAY;
    }
    NODE_STATUS_OFFLINE
}

/// Count nodes considered "online" for the user card (`online` or `busy`).
pub fn online_count<'a, I>(nodes: I) -> usize
where
    I: IntoIterator<Item = &'a InterlinkNodeView>,
{
    nodes
        .into_iter()
        .filter(|node| matches!(node.status.as_str(), NODE_STATUS_ONLINE | NODE_STATUS_BUSY))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(node_id: &str, status: &str) -> InterlinkNodeView {
        InterlinkNodeView {
            node_id: node_id.to_string(),
            node_type: "desktop".to_string(),
            user_id: "u1".to_string(),
            label: node_id.to_string(),
            status: status.to_string(),
            last_seen_at: 0.0,
            capabilities: Vec::new(),
            shadow_revision: 0,
            connected: false,
            meta: json!({}),
        }
    }

    const TTL: f64 = 90.0;

    #[test]
    fn aggregate_rule_matches_plan() {
        let online = vec![view("a", NODE_STATUS_ONLINE), view("b", NODE_STATUS_OFFLINE)];
        assert_eq!(aggregate_status(online.iter()), NODE_STATUS_ONLINE);

        // §5.1: any online/busy node makes the *user* online.
        let busy = vec![view("a", NODE_STATUS_BUSY), view("b", NODE_STATUS_AWAY)];
        assert_eq!(aggregate_status(busy.iter()), NODE_STATUS_ONLINE);

        let away = vec![view("a", NODE_STATUS_AWAY), view("b", NODE_STATUS_OFFLINE)];
        assert_eq!(aggregate_status(away.iter()), NODE_STATUS_AWAY);

        let all_away = vec![view("a", NODE_STATUS_AWAY), view("b", NODE_STATUS_AWAY)];
        assert_eq!(aggregate_status(all_away.iter()), NODE_STATUS_AWAY);

        let offline = vec![view("a", NODE_STATUS_OFFLINE), view("b", NODE_STATUS_RECONNECTING)];
        assert_eq!(aggregate_status(offline.iter()), NODE_STATUS_OFFLINE);

        let empty: Vec<InterlinkNodeView> = Vec::new();
        assert_eq!(aggregate_status(empty.iter()), NODE_STATUS_OFFLINE);
    }

    #[test]
    fn online_count_counts_online_and_busy() {
        let nodes = vec![
            view("a", NODE_STATUS_ONLINE),
            view("b", NODE_STATUS_BUSY),
            view("c", NODE_STATUS_AWAY),
            view("d", NODE_STATUS_OFFLINE),
        ];
        assert_eq!(online_count(nodes.iter()), 2);
    }

    #[test]
    fn device_status_ttl_transitions() {
        assert_eq!(derive_device_status(100.0, 90.0, 90.0), NODE_STATUS_ONLINE);
        assert_eq!(derive_device_status(100.0, 40.0, 90.0), NODE_STATUS_ONLINE); // age 60 <= 90
        assert_eq!(
            derive_device_status(100.0, -10.0, 90.0),
            NODE_STATUS_RECONNECTING
        ); // age 110 in (90, 180]
        assert_eq!(derive_device_status(1000.0, 0.0, 90.0), NODE_STATUS_OFFLINE);
    }

    #[test]
    fn device_status_with_tunnel_covers_reconnecting() {
        // Tunnel up: online regardless of a still-fresh heartbeat.
        assert_eq!(
            derive_device_status_with_tunnel(100.0, 90.0, TTL, Some(true)),
            NODE_STATUS_ONLINE
        );
        // Channel gone but the beat has not expired: reconnecting.
        assert_eq!(
            derive_device_status_with_tunnel(100.0, 60.0, TTL, Some(false)),
            NODE_STATUS_RECONNECTING
        );
        // Channel gone and the beat expired: offline.
        assert_eq!(
            derive_device_status_with_tunnel(1000.0, 0.0, TTL, Some(false)),
            NODE_STATUS_OFFLINE
        );
        // No tunnel signal at all: the heartbeat fallback (§4.4) decides.
        assert_eq!(
            derive_device_status_with_tunnel(100.0, 90.0, TTL, None),
            NODE_STATUS_ONLINE
        );
        assert_eq!(
            derive_device_status_with_tunnel(130.0, 0.0, TTL, None),
            NODE_STATUS_RECONNECTING
        );
        assert_eq!(
            derive_device_status_with_tunnel(1000.0, 0.0, TTL, None),
            NODE_STATUS_OFFLINE
        );
    }

    #[test]
    fn web_connect_reports_online_and_disconnect_removes_the_node() {
        let registry = NodeRegistry::new();
        let lease = registry.register_web("u1", "conn-1", "web·chat", 100.0);
        let nodes = registry.compose_user_nodes("u1", 101.0, TTL, vec![]);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "web:conn-1");
        assert_eq!(nodes[0].node_type, NODE_TYPE_WEB);
        assert_eq!(nodes[0].status, NODE_STATUS_ONLINE);
        assert!(nodes[0].connected);
        assert_eq!(aggregate_status(nodes.iter()), NODE_STATUS_ONLINE);

        registry.unregister_web("conn-1");
        assert!(registry
            .compose_user_nodes("u1", 102.0, TTL, vec![])
            .is_empty());
        assert_eq!(registry.web_node_count(), 0);
        // The dangling lease must not resurrect anything.
        assert_eq!(lease.node_id(), "web:conn-1");
    }

    #[test]
    fn web_nodes_are_scoped_by_user_and_stay_alive_while_leased() {
        let registry = NodeRegistry::new();
        let _lease_u1 = registry.register_web("u1", "conn-1", "web·chat", 100.0);
        let _lease_u2 = registry.register_web("u2", "conn-2", "web·core", 100.0);

        let nodes = registry.compose_user_nodes("u1", 110.0, TTL, vec![]);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "web:conn-1");

        // A long-lived connection is not expired by TTL: liveness is the lease.
        let nodes = registry.compose_user_nodes("u1", 100.0 + 3_600.0, TTL, vec![]);
        assert!(nodes[0].connected);
        assert_eq!(registry.web_node_count(), 2);
    }

    #[test]
    fn lease_less_web_node_follows_ttl_and_goes_offline() {
        let registry = NodeRegistry::new();
        registry
            .web_nodes
            .write()
            .expect("lock poisoned")
            .insert(
                "conn-9".to_string(),
                WebNodeEntry {
                    user_id: "u1".to_string(),
                    connection_id: "conn-9".to_string(),
                    label: "web".to_string(),
                    last_seen_at: 100.0,
                    lease: None,
                },
            );

        let nodes = registry.compose_user_nodes("u1", 300.0, TTL, vec![]);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].status, NODE_STATUS_OFFLINE);
        assert!(!nodes[0].connected);
        assert_eq!(registry.gc(300.0, TTL), 1);
        assert_eq!(registry.web_node_count(), 0);
    }

    #[test]
    fn activity_makes_a_web_node_busy_then_online_again() {
        let registry = NodeRegistry::new();
        let lease = registry.register_web("u1", "conn-1", "web·chat", 100.0);
        let guard = registry.begin_busy(lease.node_id());
        let guard_two = registry.begin_busy(lease.node_id());

        let nodes = registry.compose_user_nodes("u1", 101.0, TTL, vec![]);
        assert_eq!(nodes[0].status, NODE_STATUS_BUSY);
        assert_eq!(online_count(nodes.iter()), 1);

        // The second turn keeps the node busy until the last guard is dropped.
        drop(guard);
        let nodes = registry.compose_user_nodes("u1", 101.0, TTL, vec![]);
        assert_eq!(nodes[0].status, NODE_STATUS_BUSY);
        drop(guard_two);
        let nodes = registry.compose_user_nodes("u1", 101.0, TTL, vec![]);
        assert_eq!(nodes[0].status, NODE_STATUS_ONLINE);
    }

    #[test]
    fn idle_web_node_turns_away_after_the_idle_window() {
        let registry = NodeRegistry::new();
        let _lease = registry.register_web("u1", "conn-1", "web·chat", 100.0);

        // Still active: recent user activity.
        registry.touch_web("conn-1", 160.0);
        let nodes = registry.compose_user_nodes("u1", 220.0, TTL, vec![]);
        assert_eq!(nodes[0].status, NODE_STATUS_ONLINE);

        // Idle past the window: away, not offline (§5.1 web column).
        let idle_now = 100.0 + 60.0 + WEB_AWAY_IDLE_SECS + 1.0;
        let nodes = registry.compose_user_nodes("u1", idle_now, TTL, vec![]);
        assert_eq!(nodes[0].status, NODE_STATUS_AWAY);
        assert!(nodes[0].connected);
        assert_eq!(aggregate_status(nodes.iter()), NODE_STATUS_AWAY);
        assert_eq!(online_count(nodes.iter()), 0);

        // New activity clears the idle state again.
        registry.touch_web("conn-1", idle_now);
        let nodes = registry.compose_user_nodes("u1", idle_now, TTL, vec![]);
        assert_eq!(nodes[0].status, NODE_STATUS_ONLINE);
    }

    #[test]
    fn explicit_away_overlay_marks_and_activity_clears_it() {
        let registry = NodeRegistry::new();
        let lease = registry.register_web("u1", "conn-1", "web·chat", 100.0);
        registry.report_activity(lease.node_id(), false, true, 100.0);

        let nodes = registry.compose_user_nodes("u1", 110.0, TTL, vec![]);
        assert_eq!(nodes[0].status, NODE_STATUS_AWAY);

        registry.touch_web("conn-1", 110.0);
        let nodes = registry.compose_user_nodes("u1", 110.0, TTL, vec![]);
        assert_eq!(nodes[0].status, NODE_STATUS_ONLINE);
    }

    #[test]
    fn activity_overlay_marks_busy_and_away_on_persistent_nodes() {
        let registry = NodeRegistry::new();
        let persistent = vec![view("device:pc-01", NODE_STATUS_ONLINE)];

        registry.report_activity("device:pc-01", true, false, 100.0);
        let nodes = registry.compose_user_nodes("u1", 110.0, TTL, persistent.clone());
        assert_eq!(nodes[0].status, NODE_STATUS_BUSY);

        registry.report_activity("device:pc-01", false, true, 100.0);
        let nodes = registry.compose_user_nodes("u1", 110.0, TTL, persistent.clone());
        assert_eq!(nodes[0].status, NODE_STATUS_AWAY);

        // Stale activity no longer applies.
        let nodes = registry.compose_user_nodes("u1", 500.0, TTL, persistent);
        assert_eq!(nodes[0].status, NODE_STATUS_ONLINE);
    }

    #[test]
    fn gc_bounds_the_registry_after_leaked_handlers() {
        let registry = NodeRegistry::new();
        let lease = registry.register_web("u1", "conn-1", "web·chat", 100.0);
        let guard = registry.begin_busy(lease.node_id());
        registry.report_activity("device:pc-01", true, false, 100.0);
        assert_eq!(registry.busy_depth.read().expect("lock poisoned").len(), 1);

        // Nothing is collectable while the connection and its turn are alive.
        assert_eq!(registry.gc(150.0, TTL), 0);
        assert_eq!(registry.web_node_count(), 1);

        // A handler that never reached `unregister_web` (aborted task) is reaped
        // as soon as its lease is gone, together with its busy counter.
        drop(guard);
        drop(lease);
        assert_eq!(registry.gc(200.0, TTL), 3);
        assert_eq!(registry.web_node_count(), 0);
        assert!(registry
            .compose_user_nodes("u1", 200.0, TTL, vec![])
            .is_empty());
    }

    #[test]
    fn unregister_and_gc_drop_stale_entries() {
        let registry = NodeRegistry::new();
        let lease = registry.register_web("u1", "conn-1", "web·chat", 100.0);
        registry.report_activity(lease.node_id(), true, false, 100.0);
        registry.unregister_web("conn-1");
        assert!(registry
            .compose_user_nodes("u1", 101.0, TTL, vec![])
            .is_empty());
        assert!(
            !registry
                .activity
                .read()
                .expect("lock poisoned")
                .contains_key(lease.node_id())
        );

        let second = registry.register_web("u1", "conn-2", "web·chat", 100.0);
        registry.report_activity("device:x", true, false, 100.0);
        registry.clear_activity(second.node_id());
        drop(second);
        assert_eq!(registry.gc(500.0, TTL), 2);
        assert_eq!(registry.web_node_count(), 0);
    }
}
