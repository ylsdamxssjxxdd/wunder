//! Interlink node registry (I2).
//!
//! Unified presence view over three kinds of nodes: persistent cloud devices
//! (`device:<id>`), the server node (`cloud`) and volatile web sessions
//! (`web:<connection_id>`). Persistent nodes are supplied by the caller from
//! storage; this registry owns only the ephemeral web nodes plus a live
//! activity overlay applied uniformly to any node id.
//!
//! See docs/云端本地互通方案.md §5.1 for the aggregate-status rule.

use std::collections::HashMap;
use std::sync::RwLock;

use serde_json::json;
use wunder_core::interlink::{
    InterlinkNodeView, NODE_STATUS_AWAY, NODE_STATUS_BUSY, NODE_STATUS_OFFLINE, NODE_STATUS_ONLINE,
    NODE_STATUS_RECONNECTING, NODE_TYPE_WEB,
};

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
    last_seen_at: f64,
}

/// Volatile node registry.
#[derive(Debug, Default)]
pub struct NodeRegistry {
    web_nodes: RwLock<HashMap<String, WebNodeEntry>>,
    activity: RwLock<HashMap<String, NodeActivity>>,
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
    pub fn register_web(&self, user_id: &str, connection_id: &str, label: &str, now: f64) -> String {
        let entry = WebNodeEntry {
            user_id: user_id.to_string(),
            connection_id: connection_id.to_string(),
            label: label.to_string(),
            last_seen_at: now,
        };
        self.web_nodes
            .write()
            .expect("node registry lock poisoned")
            .insert(connection_id.to_string(), entry);
        Self::web_node_id(connection_id)
    }

    /// Refresh the last-seen timestamp of an existing web node.
    pub fn touch_web(&self, connection_id: &str, now: f64) {
        if let Some(entry) = self
            .web_nodes
            .write()
            .expect("node registry lock poisoned")
            .get_mut(connection_id)
        {
            entry.last_seen_at = now;
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
    }

    /// Report live activity for any node id. Passing `busy = false, away = false`
    /// clears the overlay.
    pub fn report_activity(&self, node_id: &str, busy: bool, away: bool, now: f64) {
        let mut activity = self.activity.write().expect("node registry lock poisoned");
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

    /// Garbage collect stale web nodes and activity entries.
    ///
    /// Web nodes are dropped once they are older than `2 * ttl`; activity is
    /// dropped once it is older than `ttl`. Returns the number of removed
    /// entries (web nodes + activity).
    pub fn gc(&self, now: f64, ttl: f64) -> usize {
        let mut removed = 0usize;
        {
            let mut web = self.web_nodes.write().expect("node registry lock poisoned");
            let before = web.len();
            web.retain(|_, entry| now - entry.last_seen_at <= ttl * 2.0);
            removed += before - web.len();
        }
        {
            let mut activity = self.activity.write().expect("node registry lock poisoned");
            let before = activity.len();
            activity.retain(|_, entry| now - entry.updated_at <= ttl);
            removed += before - activity.len();
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
        let web = self.web_nodes.read().expect("node registry lock poisoned").clone();

        let mut views: Vec<InterlinkNodeView> = persistent;
        for node in views.iter_mut() {
            overlay_activity(node, &activity, now, ttl);
        }

        for entry in web.values() {
            if entry.user_id != user_id {
                continue;
            }
            views.push(web_node_view(entry, &activity, now, ttl));
        }

        views.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        views
    }
}

/// Apply the activity overlay to a node, respecting the freshness window.
fn overlay_activity(
    node: &mut InterlinkNodeView,
    activity: &HashMap<String, NodeActivity>,
    now: f64,
    ttl: f64,
) {
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
    now: f64,
    ttl: f64,
) -> InterlinkNodeView {
    let fresh = now - entry.last_seen_at <= ttl;
    let mut view = InterlinkNodeView {
        node_id: NodeRegistry::web_node_id(&entry.connection_id),
        node_type: NODE_TYPE_WEB.to_string(),
        user_id: entry.user_id.clone(),
        label: entry.label.clone(),
        status: if fresh {
            NODE_STATUS_ONLINE.to_string()
        } else {
            NODE_STATUS_OFFLINE.to_string()
        },
        last_seen_at: entry.last_seen_at,
        capabilities: Vec::new(),
        shadow_revision: 0,
        connected: fresh,
        meta: json!({}),
    };
    if fresh {
        overlay_activity(&mut view, activity, now, ttl);
    }
    view
}

/// Derive a persistent device node status from its last-seen timestamp.
///
/// P0 uses heartbeat freshness only (tunnel wiring lands in I4/I5):
/// - `age <= ttl`  -> online
/// - `age <= 2*ttl` -> reconnecting
/// - otherwise -> offline
pub fn derive_device_status(now: f64, last_seen_at: f64, ttl: f64) -> &'static str {
    let age = now - last_seen_at;
    if age <= ttl {
        NODE_STATUS_ONLINE
    } else if age <= ttl * 2.0 {
        NODE_STATUS_RECONNECTING
    } else {
        NODE_STATUS_OFFLINE
    }
}

/// Aggregate per plan §5.1: any node online/busy -> online; all away -> away;
/// otherwise offline. Empty lists are offline.
pub fn aggregate_status<'a, I>(nodes: I) -> &'static str
where
    I: IntoIterator<Item = &'a InterlinkNodeView>,
{
    let mut any = false;
    let mut any_away = false;
    for node in nodes {
        any = true;
        match node.status.as_str() {
            NODE_STATUS_ONLINE | NODE_STATUS_BUSY => return NODE_STATUS_ONLINE,
            NODE_STATUS_AWAY => any_away = true,
            _ => {}
        }
    }
    if any_away {
        return NODE_STATUS_AWAY;
    }
    let _ = any;
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

    #[test]
    fn aggregate_rule_matches_plan() {
        let online = vec![view("a", NODE_STATUS_ONLINE), view("b", NODE_STATUS_OFFLINE)];
        assert_eq!(aggregate_status(online.iter()), NODE_STATUS_ONLINE);

        let busy = vec![view("a", NODE_STATUS_BUSY), view("b", NODE_STATUS_AWAY)];
        assert_eq!(aggregate_status(busy.iter()), NODE_STATUS_BUSY);

        let away = vec![view("a", NODE_STATUS_AWAY), view("b", NODE_STATUS_OFFLINE)];
        assert_eq!(aggregate_status(away.iter()), NODE_STATUS_AWAY);

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
    fn web_nodes_are_scoped_by_user_and_follow_ttl() {
        let registry = NodeRegistry::new();
        registry.register_web("u1", "conn-1", "web", 100.0);
        registry.register_web("u2", "conn-2", "web", 100.0);

        let nodes = registry.compose_user_nodes("u1", 110.0, 90.0, vec![]);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "web:conn-1");
        assert_eq!(nodes[0].status, NODE_STATUS_ONLINE);

        // Stale web node stays listed but offline.
        let nodes = registry.compose_user_nodes("u1", 300.0, 90.0, vec![]);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].status, NODE_STATUS_OFFLINE);
        assert!(!nodes[0].connected);
    }

    #[test]
    fn activity_overlay_marks_busy_and_away() {
        let registry = NodeRegistry::new();
        let persistent = vec![view("device:pc-01", NODE_STATUS_ONLINE)];

        registry.report_activity("device:pc-01", true, false, 100.0);
        let nodes = registry.compose_user_nodes("u1", 110.0, 90.0, persistent.clone());
        assert_eq!(nodes[0].status, NODE_STATUS_BUSY);

        registry.report_activity("device:pc-01", false, true, 100.0);
        let nodes = registry.compose_user_nodes("u1", 110.0, 90.0, persistent.clone());
        assert_eq!(nodes[0].status, NODE_STATUS_AWAY);

        // Stale activity no longer applies.
        let nodes = registry.compose_user_nodes("u1", 500.0, 90.0, persistent);
        assert_eq!(nodes[0].status, NODE_STATUS_ONLINE);
    }

    #[test]
    fn unregister_and_gc_drop_stale_entries() {
        let registry = NodeRegistry::new();
        registry.register_web("u1", "conn-1", "web", 100.0);
        registry.unregister_web("conn-1");
        assert!(registry.compose_user_nodes("u1", 101.0, 90.0, vec![]).is_empty());

        registry.register_web("u1", "conn-2", "web", 100.0);
        registry.report_activity("device:x", true, false, 100.0);
        let removed = registry.gc(500.0, 90.0);
        assert!(removed >= 2);
    }
}