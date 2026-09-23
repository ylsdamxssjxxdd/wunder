//! Notifications are hints; the durable run ledger remains authoritative.
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tokio::sync::watch;

#[derive(Default)]
pub(crate) struct RunSignals(Mutex<HashMap<String, Weak<watch::Sender<u64>>>>);

pub(crate) struct RunSubscription {
    _sender: Arc<watch::Sender<u64>>,
    pub receiver: watch::Receiver<u64>,
}

impl RunSignals {
    pub(crate) fn subscribe(&self, session: &str) -> Option<RunSubscription> {
        let mut entries = self.0.lock();
        let sender = if let Some(sender) = entries.get(session).and_then(Weak::upgrade) {
            sender
        } else {
            if entries.len() >= 8192 {
                entries.retain(|_, entry| entry.strong_count() > 0);
                if entries.len() >= 8192 {
                    return None;
                }
            }
            let (sender, _) = watch::channel(0_u64);
            let sender = Arc::new(sender);
            entries.insert(session.to_string(), Arc::downgrade(&sender));
            sender
        };
        Some(RunSubscription {
            receiver: sender.subscribe(),
            _sender: sender,
        })
    }

    pub(crate) fn notify(&self, session: &str) {
        let sender = self.0.lock().get(session).and_then(Weak::upgrade);
        if let Some(sender) = sender {
            sender.send_modify(|version| *version = version.wrapping_add(1));
        }
    }
}
