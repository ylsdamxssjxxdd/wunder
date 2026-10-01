use parking_lot::Mutex;
use std::collections::HashMap;
use tokio::sync::watch;

/// Per-session durable change notifications.
///
/// Emit publishes the latest durable change cursor after a thread-log commit;
/// session feeders subscribe so they wake immediately instead of waiting for
/// the next poll tick. The hub is in-process acceleration only: the feeder
/// always keeps its poll fallback, so multi-instance deployments converge
/// through polling and never depend on the hub for correctness.
#[derive(Default)]
pub struct ThreadChangeHub {
    inner: Mutex<HashMap<String, watch::Sender<i64>>>,
}

impl ThreadChangeHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the latest durable cursor for a session and wake its feeders.
    pub fn publish(&self, session_id: &str, cursor: i64) {
        let session_id = session_id.trim();
        if session_id.is_empty() || cursor <= 0 {
            return;
        }
        let mut guard = self.inner.lock();
        if let Some(sender) = guard.get(session_id) {
            sender.send_if_modified(|current| {
                if cursor > *current {
                    *current = cursor;
                    true
                } else {
                    false
                }
            });
            if sender.receiver_count() > 0 {
                return;
            }
        }
        // Entries without subscribers are dead weight; drop them. A later
        // subscribe() re-creates the channel and the feeder's poll fallback
        // covers anything published in between.
        let subscribed = guard
            .get(session_id)
            .map(|sender| sender.receiver_count() > 0)
            .unwrap_or(false);
        if !subscribed {
            guard.remove(session_id);
        }
    }

    /// Subscribe to cursor changes for a session. The receiver starts at 0
    /// for a fresh session; feeders treat it as "wake early", never as truth.
    pub fn subscribe(&self, session_id: &str) -> watch::Receiver<i64> {
        let session_id = session_id.trim();
        let mut guard = self.inner.lock();
        if let Some(sender) = guard.get(session_id) {
            return sender.subscribe();
        }
        let (sender, receiver) = watch::channel(0i64);
        guard.insert(session_id.to_string(), sender);
        receiver
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_wakes_subscribers_with_monotonic_cursor() {
        let hub = ThreadChangeHub::new();
        let rx = hub.subscribe("session_a");
        hub.publish("session_a", 5);
        assert_eq!(*rx.borrow(), 5);
        // Lower or equal cursors never move the receiver backwards.
        hub.publish("session_a", 4);
        assert_eq!(*rx.borrow(), 5);
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
}
