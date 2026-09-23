//! Bounded, turn-owned mailboxes. Durable history remains owned by the receiving thread.
use anyhow::{bail, Result};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) const MAX_MESSAGE_BYTES: usize = 20_000;
const MAX_PENDING_MESSAGES: usize = 64;
const MAX_PENDING_BYTES: usize = 256 * 1024;
const MAX_RECEIPTS: usize = 256;
const MAX_MAILBOXES: usize = 8192;

#[derive(Clone, Debug)]
pub(crate) struct AgentMessage {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub text: String,
    pub cancellation: Option<CancellationToken>,
}

impl AgentMessage {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.text.trim().is_empty() || self.text.len() > MAX_MESSAGE_BYTES {
            bail!("message must contain 1..20000 UTF-8 bytes");
        }
        if self.id.is_empty() || self.id.len() > 128 {
            bail!("message_id must contain 1..128 bytes");
        }
        if self.cancelled() {
            bail!("message source was interrupted");
        }
        Ok(())
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    }
}

#[derive(Default)]
pub(crate) struct Mailboxes {
    entries: Mutex<HashMap<(String, String), Arc<Mailbox>>>,
}

#[derive(Default)]
struct MailboxState {
    closed: bool,
    pending: VecDeque<AgentMessage>,
    bytes: usize,
    // Keep bounded receipts for retry idempotency within this turn.
    receipts: VecDeque<(String, String, [u8; 32])>,
}

#[derive(Default)]
struct Mailbox {
    state: Mutex<MailboxState>,
}

pub(crate) struct MailboxGuard {
    registry: Arc<Mailboxes>,
    key: (String, String),
    mailbox: Arc<Mailbox>,
}

impl Mailboxes {
    pub(crate) fn has_pending(&self, user: &str, session: &str) -> bool {
        self.entries
            .lock()
            .get(&(user.to_string(), session.to_string()))
            .is_some_and(|mailbox| {
                mailbox
                    .state
                    .lock()
                    .pending
                    .iter()
                    .any(|message| !message.cancelled())
            })
    }
    pub(crate) fn is_open(&self, user: &str, session: &str) -> bool {
        self.entries
            .lock()
            .get(&(user.to_string(), session.to_string()))
            .is_some_and(|mailbox| !mailbox.state.lock().closed)
    }
    pub(crate) fn open(self: &Arc<Self>, user: &str, session: &str) -> Result<MailboxGuard> {
        let key = (user.to_string(), session.to_string());
        let mut entries = self.entries.lock();
        if entries.contains_key(&key) || entries.len() >= MAX_MAILBOXES {
            bail!("thread mailbox unavailable");
        }
        let mailbox = Arc::new(Mailbox::default());
        entries.insert(key.clone(), mailbox.clone());
        Ok(MailboxGuard {
            registry: self.clone(),
            key,
            mailbox,
        })
    }

    /// False means there is no accepting turn; the caller must queue a new turn or retry.
    pub(crate) fn send(&self, user: &str, session: &str, message: AgentMessage) -> Result<bool> {
        message.validate()?;
        let mailbox = self
            .entries
            .lock()
            .get(&(user.to_string(), session.to_string()))
            .cloned();
        let Some(mailbox) = mailbox else {
            return Ok(false);
        };
        let mut state = mailbox.state.lock();
        if state.closed {
            return Ok(false);
        }
        let mut digest = Sha256::new();
        for field in [&message.source, &message.kind, &message.text] {
            digest.update((field.len() as u64).to_le_bytes());
            digest.update(field.as_bytes());
        }
        let fingerprint: [u8; 32] = digest.finalize().into();
        if let Some(receipt) = state
            .receipts
            .iter()
            .find(|receipt| receipt.0 == message.source && receipt.1 == message.id)
        {
            if receipt.2 != fingerprint {
                bail!("message_id already belongs to a different message");
            }
            return Ok(true);
        }
        if state.pending.len() >= MAX_PENDING_MESSAGES
            || state.bytes + message.text.len() > MAX_PENDING_BYTES
        {
            bail!("thread mailbox is full; wait for delivery before retrying");
        }
        if state.receipts.len() == MAX_RECEIPTS {
            state.receipts.pop_front();
        }
        state
            .receipts
            .push_back((message.source.clone(), message.id.clone(), fingerprint));
        state.bytes += message.text.len();
        state.pending.push_back(message);
        Ok(true)
    }
}

impl MailboxGuard {
    /// Checking for new input and closing admission share one lock: accepted messages
    /// cannot fall between a final empty check and completion of the receiving turn.
    pub(crate) fn take(&self, close_if_empty: bool) -> Vec<AgentMessage> {
        let mut state = self.mailbox.state.lock();
        if close_if_empty && state.pending.iter().all(AgentMessage::cancelled) {
            state.closed = true;
        }
        state.bytes = 0;
        state.pending.drain(..).collect()
    }

    pub(crate) fn close(&self) -> Vec<AgentMessage> {
        let mut state = self.mailbox.state.lock();
        state.closed = true;
        state.bytes = 0;
        state.pending.drain(..).collect()
    }
}

impl Drop for MailboxGuard {
    fn drop(&mut self) {
        self.close();
        let mut entries = self.registry.entries.lock();
        if entries
            .get(&self.key)
            .is_some_and(|entry| Arc::ptr_eq(entry, &self.mailbox))
        {
            entries.remove(&self.key);
        }
    }
}

#[cfg(test)]
#[path = "mailbox_tests.rs"]
mod tests;
