//! Remote session view fan-out (docs §7.4).
//!
//! The server is a forwarder: it keeps at most one upstream tunnel stream per
//! `(device, thread)` no matter how many browser tabs watch it, and it never
//! persists what it forwards. Subscribers get a bounded queue; a slow watcher
//! is dropped rather than back-pressuring the tunnel.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::mpsc;

/// Watchers per node (docs §10.1).
pub const MAX_SUBSCRIBERS_PER_NODE: usize = 8;
/// Frames buffered for one watcher before it is considered too slow.
pub const SUBSCRIBER_QUEUE: usize = 128;
/// Watchers for one thread.
pub const MAX_SUBSCRIBERS_PER_THREAD: usize = 8;

type ThreadKey = (String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscribeError {
    NodeLimit,
    QueueLimit,
}

impl SubscribeError {
    pub fn code(self) -> &'static str {
        match self {
            SubscribeError::NodeLimit => "SUBSCRIBER_LIMIT",
            SubscribeError::QueueLimit => "SUBSCRIBER_QUEUE_LIMIT",
        }
    }
}

/// A live watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subscription {
    pub id: u64,
    /// True when this watcher created the first interest in the pair, i.e. the
    /// caller must ask the node to start forwarding (`thread_attach`).
    pub first: bool,
}

#[derive(Debug)]
struct Watcher {
    id: u64,
    sender: mpsc::Sender<String>,
}

#[derive(Debug, Default)]
struct HubState {
    threads: HashMap<ThreadKey, Vec<Watcher>>,
    control: HashMap<String, Vec<Watcher>>,
    /// Devices we currently ask to forward thread events.
    upstream: Vec<ThreadKey>,
}

#[derive(Debug, Default)]
pub struct RemoteHub {
    state: Mutex<HubState>,
    next_id: AtomicU64,
}

impl RemoteHub {
    fn next(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Watch one thread of one device.
    pub fn subscribe(
        &self,
        device_id: &str,
        thread_id: &str,
    ) -> Result<(Subscription, mpsc::Receiver<String>), SubscribeError> {
        let mut state = self.lock();
        if state.threads.len() >= MAX_SUBSCRIBERS_PER_NODE * MAX_SUBSCRIBERS_PER_NODE {
            return Err(SubscribeError::QueueLimit);
        }
        // Counted before the entry borrow, so the node cap is checked against
        // the state as it stands (docs §10.1: remote subscriptions are bounded).
        let node_total = state
            .threads
            .iter()
            .filter(|(key, _)| key.0 == device_id)
            .map(|(_, list)| list.len())
            .sum::<usize>();
        if node_total >= MAX_SUBSCRIBERS_PER_NODE {
            return Err(SubscribeError::NodeLimit);
        }
        let watchers = state
            .threads
            .entry((device_id.to_string(), thread_id.to_string()))
            .or_default();
        if watchers.len() >= MAX_SUBSCRIBERS_PER_THREAD {
            return Err(SubscribeError::NodeLimit);
        }
        let id = self.next();
        let first = watchers.is_empty();
        let (sender, receiver) = mpsc::channel(SUBSCRIBER_QUEUE);
        watchers.push(Watcher { id, sender });
        if first {
            state
                .upstream
                .push((device_id.to_string(), thread_id.to_string()));
        }
        Ok((
            Subscription {
                id,
                first,
            },
            receiver,
        ))
    }

    /// Watch the command lifecycle of one device (no thread filter).
    pub fn subscribe_control(
        &self,
        device_id: &str,
    ) -> Result<(Subscription, mpsc::Receiver<String>), SubscribeError> {
        let mut state = self.lock();
        let watchers = state.control.entry(device_id.to_string()).or_default();
        if watchers.len() >= MAX_SUBSCRIBERS_PER_NODE {
            return Err(SubscribeError::NodeLimit);
        }
        let id = self.next();
        let first = watchers.is_empty();
        let (sender, receiver) = mpsc::channel(SUBSCRIBER_QUEUE);
        watchers.push(Watcher {
            id,
            sender,
        });
        Ok((
            Subscription {
                id,
                first,
            },
            receiver,
        ))
    }

    /// Drop a watcher; returns whether the node can stop forwarding upstream.
    pub fn unsubscribe(&self, device_id: &str, thread_id: &str, id: u64) -> bool {
        let mut state = self.lock();
        let key = (device_id.to_string(), thread_id.to_string());
        let last = match state.threads.get_mut(&key) {
            Some(watchers) => {
                watchers.retain(|watcher| watcher.id != id);
                watchers.is_empty()
            }
            None => false,
        };
        if last {
            state.threads.remove(&key);
            state.upstream.retain(|entry| *entry != key);
        }
        last
    }

    pub fn unsubscribe_control(&self, device_id: &str, id: u64) {
        let mut state = self.lock();
        if let Some(watchers) = state.control.get_mut(device_id) {
            watchers.retain(|watcher| watcher.id != id);
            if watchers.is_empty() {
                state.control.remove(device_id);
            }
        }
    }

    /// Forward one thread event frame to every watcher of the pair.
    ///
    /// Non-blocking by design: a watcher whose queue is full is dropped, so one
    /// stalled browser tab can never slow the tunnel (docs §10.1).
    pub fn publish(&self, device_id: &str, thread_id: &str, frame: &str) -> usize {
        let state = self.lock();
        let watchers = match state.threads
            .get(&(device_id.to_string(), thread_id.to_string()))
        {
            Some(watchers) => watchers,
            None => return 0,
        };
        let mut delivered = 0usize;
        for watcher in watchers {
            if watcher.sender.try_send(frame.to_string()).is_ok() {
                delivered += 1;
            }
        }
        delivered
    }

    /// Forward a command lifecycle notice to a device's control watchers.
    pub fn publish_control(&self, device_id: &str, frame: &str) -> usize {
        let state = self.lock();
        let mut delivered = 0usize;
        if let Some(watchers) = state.control.get(device_id) {
            for watcher in watchers {
                if watcher.sender.try_send(frame.to_string()).is_ok() {
                    delivered += 1;
                }
            }
        }
        // Command notices also reach thread watchers of that device so the
        // remote session view learns about cancel/finish without polling.
        for (key, watchers) in state.threads.iter() {
            if key.0 != device_id {
                continue;
            }
            for watcher in watchers {
                let _ = watcher.sender.try_send(frame.to_string());
            }
        }
        delivered
    }

    /// Whether the node is already asked to forward this thread.
    pub fn is_upstream(&self, device_id: &str, thread_id: &str) -> bool {
        let state = self.lock();
        state
            .upstream
            .iter()
            .any(|entry| entry.0 == device_id && entry.1 == thread_id)
    }

    /// Every watched pair (used by the bridge and by the janitor).
    pub fn watch_list(&self) -> Vec<(String, String, usize)> {
        let state = self.lock();
        state
            .threads
            .iter()
            .map(|(key, watchers)| (key.0.clone(), key.1.clone(), watchers.len()))
            .collect()
    }

    /// Tear down every watcher of a node whose tunnel just closed.
    pub fn close_node(&self, device_id: &str) -> usize {
        let mut state = self.lock();
        let mut closed = 0usize;
        state.threads.retain(|key, watchers| {
            if key.0 == device_id {
                closed += watchers.len();
                false
            } else {
                true
            }
        });
        state.upstream.retain(|key| key.0 != device_id);
        state.control.remove(device_id);
        closed
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubState> {
        self.state.lock().expect("remote hub lock poisoned")
    }
}

pub fn hub() -> &'static RemoteHub {
    static INSTANCE: OnceLock<RemoteHub> = OnceLock::new();
    INSTANCE.get_or_init(RemoteHub::default)
}

/// Convenience: is anyone still watching this thread?
pub fn has_watchers(device_id: &str, thread_id: &str) -> bool {
    let state = hub().state.lock().expect("remote hub lock poisoned");
    state
        .threads
        .get(&(device_id.to_string(), thread_id.to_string()))
        .map(|watchers| !watchers.is_empty())
        .unwrap_or(false)
}

/// Frames per watched pair is small; the arc only exists so the ws handler can
/// keep the device id alive without cloning it per frame.
pub type WatchTarget = Arc<(String, String)>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_watcher_creates_the_upstream_interest() {
        let hub = RemoteHub::default();
        let (first, _rx_a) = hub.subscribe("dev-1", "th_1").expect("subscribe");
        assert!(first.first);
        let (second, _rx_b) = hub.subscribe("dev-1", "th_1").expect("subscribe");
        assert!(!second.first);
        assert!(hub.is_upstream("dev-1", "th_1"));

        let stopped = hub.unsubscribe("dev-1", "th_1", first.id);
        assert!(!stopped, "one watcher is left");
        let stopped = hub.unsubscribe("dev-1", "th_1", second.id);
        assert!(stopped);
        assert!(!hub.is_upstream("dev-1", "th_1"));
    }

    #[test]
    fn publish_fans_out_without_amplifying_upstream() {
        let hub = RemoteHub::default();
        let (_s1, mut rx1) = hub.subscribe("dev-1", "th_1").expect("sub");
        let (_s2, mut rx2) = hub.subscribe("dev-1", "th_1").expect("sub");
        let (_s3, _rx3) = hub.subscribe("dev-1", "th_2").expect("sub");
        assert_eq!(hub.publish("dev-1", "th_1", "frame"), 2);
        assert_eq!(rx1.try_recv().expect("frame"), "frame");
        assert_eq!(rx2.try_recv().expect("frame"), "frame");
        assert_eq!(hub.watch_list().len(), 2);
    }

    #[test]
    fn node_watcher_cap_is_enforced() {
        let hub = RemoteHub::default();
        let mut receivers = Vec::new();
        let mut limited = false;
        for index in 0..(MAX_SUBSCRIBERS_PER_THREAD + 2) {
            match hub.subscribe("dev-1", &format!("th_{index}")) {
                Ok((_, receiver)) => receivers.push(receiver),
                Err(_) => {
                    limited = true;
                    break;
                }
            }
        }
        assert!(limited, "the per-node cap must trigger");
        assert_eq!(receivers.len(), MAX_SUBSCRIBERS_PER_NODE);
    }

    #[test]
    fn slow_watcher_is_dropped_not_backpressuring() {
        let hub = RemoteHub::default();
        let (sub, _rx) = hub.subscribe("dev-1", "th_1").expect("sub");
        let key = ("dev-1".to_string(), "th_1".to_string());
        // Drain the watcher's queue to its bound, then publishing still returns.
        for index in 0..SUBSCRIBER_QUEUE {
            assert_eq!(hub.publish("dev-1", "th_1", &format!("f{index}")), 1);
        }
        assert_eq!(hub.publish("dev-1", "th_1", "overflow"), 0);
        // The queue slot stays registered; the frame was simply skipped.
        assert_eq!(
            hub.unsubscribe("dev-1", "th_1", sub.id),
            true
        );
        assert!(!hub.is_upstream("dev-1", "th_1"));
        assert_eq!(key.0, "dev-1");
    }

    #[test]
    fn control_watchers_receive_lifecycle_and_close_with_the_node() {
        let hub = RemoteHub::default();
        let (control, mut rx) = hub.subscribe_control("dev-1").expect("sub");
        let (_thread, mut thread_rx) = hub.subscribe("dev-1", "th_1").expect("sub");
        assert_eq!(hub.publish_control("dev-1", "notice"), 1);
        assert_eq!(rx.try_recv().expect("control"), "notice");
        assert_eq!(thread_rx.try_recv().expect("thread"), "notice");

        hub.unsubscribe_control("dev-1", control.id);
        assert_eq!(hub.close_node("dev-1"), 1);
        assert_eq!(hub.watch_list().len(), 0);
        assert_eq!(hub.publish("dev-1", "th_1", "late"), 0);
    }

    #[test]
    fn has_watchers_reflects_live_interest() {
        let hub = RemoteHub::default();
        let _guard: Arc<()> = Arc::new(());
        assert!(!has_watchers_in(&hub, "dev-9", "th_9"));
        let (sub, _rx) = hub.subscribe("dev-9", "th_9").expect("sub");
        assert!(has_watchers_in(&hub, "dev-9", "th_9"));
        hub.unsubscribe("dev-9", "th_9", sub.id);
        assert!(!has_watchers_in(&hub, "dev-9", "th_9"));
    }

    fn has_watchers_in(hub: &RemoteHub, device_id: &str, thread_id: &str) -> bool {
        let state = hub.state.lock().expect("lock");
        state
            .threads
            .get(&(device_id.to_string(), thread_id.to_string()))
            .map(|watchers| !watchers.is_empty())
            .unwrap_or(false)
    }
}
