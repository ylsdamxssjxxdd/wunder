//! `CloudLogReporter`: bounded in-memory buffer of explicit engine events,
//! flushed in batches to `POST /wunder/cloud/logs`. A successful upload also
//! touches the device on the server, so the reporter doubles as the heartbeat
//! (§4.1.5). Disk spill is intentionally simplified away: when the buffer is
//! full the oldest entries are dropped — local state stays authoritative and
//! the cloud log namespace is diagnostic only.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tokio::sync::Notify;

pub const MAX_BUFFERED_LOGS: usize = 500;
pub const FLUSH_BATCH_ITEMS: usize = 100;
pub const FLUSH_TRIGGER_ITEMS: usize = 50;
pub const PERIODIC_FLUSH_SECS: u64 = 30;
pub const INITIAL_BACKOFF_SECS: u64 = 30;
pub const MAX_BACKOFF_SECS: u64 = 300;
pub const MAX_MESSAGE_CHARS: usize = 512;
/// Idle heartbeat cadence: an empty log batch keeps the device `last_seen_at`
/// fresh on the server while nothing is uploaded.
pub const HEARTBEAT_INTERVAL_SECS: u64 = 300;
/// Retry wait after a failed heartbeat.
pub const HEARTBEAT_RETRY_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudLogEntry {
    pub seq: i64,
    pub level: String,
    pub category: String,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_session_id: Option<String>,
    pub created_at: f64,
}

#[derive(Debug, Deserialize)]
pub struct LogUploadResponse {
    #[serde(default)]
    pub accepted: i64,
    #[serde(default)]
    pub last_seq: Option<i64>,
}

/// Sequence cursor plus bounded buffer shared between the sync `report()`
/// entry point and the async flush task. Sequence numbers continue from the
/// persisted `last_synced_seq` so `(device_id, seq)` stays a stable idempotency
/// key across restarts.
pub(super) struct LogReporter {
    next_seq: AtomicU64,
    buffer: Mutex<VecDeque<CloudLogEntry>>,
    pub(super) wake: Notify,
}

impl LogReporter {
    pub(super) fn new(last_synced_seq: i64) -> Self {
        Self {
            next_seq: AtomicU64::new(last_synced_seq.max(0) as u64 + 1),
            buffer: Mutex::new(VecDeque::with_capacity(64)),
            wake: Notify::new(),
        }
    }

    pub(super) fn push(
        &self,
        level: &str,
        category: &str,
        event: &str,
        message: Option<&str>,
        local_session_id: Option<&str>,
    ) {
        let entry = CloudLogEntry {
            seq: self.next_seq.fetch_add(1, Ordering::Relaxed) as i64,
            level: level.to_string(),
            category: category.to_string(),
            event: event.to_string(),
            message: message.map(truncate_chars),
            local_session_id: local_session_id.map(str::to_string),
            created_at: now_unix_seconds(),
        };
        let mut buffer = self.buffer.lock().unwrap_or_else(|err| err.into_inner());
        if buffer.len() >= MAX_BUFFERED_LOGS {
            buffer.pop_front();
            tracing::debug!("cloud log buffer full, dropped the oldest entry");
        }
        buffer.push_back(entry);
        let pending = buffer.len();
        drop(buffer);
        if pending >= FLUSH_TRIGGER_ITEMS {
            self.wake.notify_one();
        }
    }

    /// Take up to one batch; entries are removed so a slow upload cannot grow
    /// the buffer while concurrent `report()` calls keep appending.
    pub(super) fn take_batch(&self) -> Vec<CloudLogEntry> {
        let mut buffer = self.buffer.lock().unwrap_or_else(|err| err.into_inner());
        let take = buffer.len().min(FLUSH_BATCH_ITEMS);
        buffer.drain(..take).collect()
    }

    pub(super) fn requeue_front(&self, entries: Vec<CloudLogEntry>) {
        if entries.is_empty() {
            return;
        }
        let mut buffer = self.buffer.lock().unwrap_or_else(|err| err.into_inner());
        for entry in entries.into_iter().rev() {
            buffer.push_front(entry);
        }
        while buffer.len() > MAX_BUFFERED_LOGS {
            buffer.pop_front();
        }
    }

    pub(super) fn pending(&self) -> usize {
        self.buffer
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .len()
    }

    pub(super) fn clear(&self) {
        self.buffer
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
    }
}

pub(super) fn now_unix_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn truncate_chars(raw: &str) -> String {
    if raw.chars().count() <= MAX_MESSAGE_CHARS {
        return raw.to_string();
    }
    raw.chars().take(MAX_MESSAGE_CHARS).collect()
}
