use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::watch;

/// Per-session durable change notifications.
///
/// Emit publishes a durable-history wake after a projection commit. Session
/// feeders subscribe so they never poll while idle. The stored cursor remains
/// monotonic, but each publish wakes receivers because a rollout replacement
/// can change its contents without increasing the old waterline.
#[derive(Default)]
pub struct ThreadChangeHub {
    inner: Mutex<HashMap<String, watch::Sender<i64>>>,
    context: Mutex<HashMap<String, watch::Sender<Value>>>,
    goal: Mutex<HashMap<String, watch::Sender<Value>>>,
}

impl ThreadChangeHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the latest durable cursor for a session and wake its feeders.
    pub fn publish(&self, session_id: &str, cursor: i64) {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return;
        }
        let mut guard = self.inner.lock();
        if let Some(sender) = guard.get(session_id) {
            sender.send_if_modified(|current| {
                if cursor > *current {
                    *current = cursor;
                }
                // A replacement can have the same (or a lower) position. A
                // watch version is a commit wake, not an ordering authority.
                true
            });
            if sender.receiver_count() > 0 {
                return;
            }
        }
        // Entries without subscribers are dead weight; drop them. A later
        // subscribe() re-creates the channel; the feeder's initial bounded
        // replay covers anything published in between.
        let subscribed = guard
            .get(session_id)
            .map(|sender| sender.receiver_count() > 0)
            .unwrap_or(false);
        if !subscribed {
            guard.remove(session_id);
        }
    }

    /// Subscribe to projection commit wakes for a session. The receiver starts
    /// at zero for a fresh session; feeders treat it as a wake, never as truth.
    pub fn subscribe(&self, session_id: &str) -> watch::Receiver<i64> {
        let session_id = session_id.trim();
        let mut guard = self.inner.lock();
        guard.retain(|_, sender| sender.receiver_count() > 0);
        if let Some(sender) = guard.get(session_id) {
            return sender.subscribe();
        }
        let (sender, receiver) = watch::channel(0i64);
        guard.insert(session_id.to_string(), sender);
        receiver
    }

    /// Publish the latest context usage snapshot for a session. This is an
    /// in-process acceleration path only; the persisted context token value
    /// remains the recovery source when a subscriber reconnects.
    pub fn publish_context_usage(&self, session_id: &str, payload: Value) {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return;
        }
        let mut guard = self.context.lock();
        if let Some(sender) = guard.get(session_id) {
            let _ = sender.send(payload.clone());
            if sender.receiver_count() > 0 {
                return;
            }
        }
        let subscribed = guard
            .get(session_id)
            .map(|sender| sender.receiver_count() > 0)
            .unwrap_or(false);
        if !subscribed {
            guard.remove(session_id);
        }
    }

    /// Subscribe to context usage snapshots for one session. The initial
    /// `Null` value is intentionally not a snapshot; consumers should retain
    /// their durable initial read until the first change arrives.
    pub fn subscribe_context_usage(&self, session_id: &str) -> watch::Receiver<Value> {
        let session_id = session_id.trim();
        let mut guard = self.context.lock();
        guard.retain(|_, sender| sender.receiver_count() > 0);
        if let Some(sender) = guard.get(session_id) {
            return sender.subscribe();
        }
        let (sender, receiver) = watch::channel(Value::Null);
        guard.insert(session_id.to_string(), sender);
        receiver
    }

    /// Publish a complete current goal projection for one session. Unlike
    /// thread-log cursors, a null payload is meaningful: it represents a
    /// successfully cleared goal.
    pub fn publish_goal_update(&self, session_id: &str, payload: Value) {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return;
        }
        let mut guard = self.goal.lock();
        if let Some(sender) = guard.get(session_id) {
            let _ = sender.send(payload.clone());
            if sender.receiver_count() > 0 {
                return;
            }
        }
        let subscribed = guard
            .get(session_id)
            .map(|sender| sender.receiver_count() > 0)
            .unwrap_or(false);
        if !subscribed {
            guard.remove(session_id);
        }
    }

    /// Subscribe to complete current goal projections. The initial null is a
    /// sentinel, not a snapshot; callers retain their durable initial read
    /// until a later published update.
    pub fn subscribe_goal_updates(&self, session_id: &str) -> watch::Receiver<Value> {
        let session_id = session_id.trim();
        let mut guard = self.goal.lock();
        guard.retain(|_, sender| sender.receiver_count() > 0);
        if let Some(sender) = guard.get(session_id) {
            return sender.subscribe();
        }
        let (sender, receiver) = watch::channel(Value::Null);
        guard.insert(session_id.to_string(), sender);
        receiver
    }
}

impl crate::services::goal::service::GoalUpdatePublisher for ThreadChangeHub {
    fn publish_goal_update(&self, session_id: &str, payload: Value) {
        Self::publish_goal_update(self, session_id, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_wakes_subscribers_with_monotonic_cursor() {
        let hub = ThreadChangeHub::new();
        let mut rx = hub.subscribe("session_a");
        hub.publish("session_a", 5);
        assert_eq!(*rx.borrow(), 5);
        rx.borrow_and_update();
        // Lower cursors do not move the stored watermark backwards, but still
        // publish a wake for a replacement rollout.
        hub.publish("session_a", 4);
        assert_eq!(*rx.borrow(), 5);
        assert!(rx.has_changed().expect("replacement wake"));
        rx.borrow_and_update();
        hub.publish("session_a", 5);
        assert!(rx.has_changed().expect("same-position wake"));
        rx.borrow_and_update();
        hub.publish("session_a", 7);
        assert_eq!(*rx.borrow(), 7);
    }

    #[test]
    fn publish_without_subscribers_is_dropped() {
        let hub = ThreadChangeHub::new();
        hub.publish("session_b", 3);
        hub.publish("session_c", 0);
        let rx = hub.subscribe("session_b");
        assert_eq!(*rx.borrow(), 0);
        hub.publish("session_b", 4);
        assert_eq!(*rx.borrow(), 4);
    }

    #[test]
    fn empty_session_ids_are_ignored() {
        let hub = ThreadChangeHub::new();
        let rx = hub.subscribe("  ");
        hub.publish(" ", 9);
        assert_eq!(*rx.borrow(), 0);
        drop(rx);
    }

    #[test]
    fn context_usage_wakes_subscribers_with_latest_snapshot() {
        let hub = ThreadChangeHub::new();
        let mut rx = hub.subscribe_context_usage("session_a");
        hub.publish_context_usage("session_a", serde_json::json!({"context_tokens": 12}));
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update()["context_tokens"], 12);
    }

    #[test]
    fn goal_updates_wake_subscribers_and_keep_cleared_state() {
        let hub = ThreadChangeHub::new();
        let mut rx = hub.subscribe_goal_updates("session_a");
        hub.publish_goal_update("session_a", serde_json::json!({"phase": "active"}));
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update()["phase"], "active");
        hub.publish_goal_update("session_a", Value::Null);
        assert!(rx.has_changed().unwrap());
        assert!(rx.borrow_and_update().is_null());
    }
}
