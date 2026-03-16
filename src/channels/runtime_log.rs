use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelRuntimeLogLevel {
    Info,
    Warn,
    Error,
}

impl ChannelRuntimeLogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelRuntimeLogEntry {
    pub ts: f64,
    pub level: String,
    pub channel: String,
    pub account_id: String,
    pub event: String,
    pub message: String,
    pub repeat_count: u32,
}

#[derive(Debug, Clone)]
struct StoredRuntimeLog {
    id: u64,
    key: String,
    updated_at: f64,
    entry: ChannelRuntimeLogEntry,
}

#[derive(Debug, Clone)]
pub struct ChannelRuntimeLogBuffer {
    capacity: usize,
    flood_window_s: f64,
    next_id: u64,
    latest_by_key: HashMap<String, u64>,
    entries: VecDeque<StoredRuntimeLog>,
}

impl ChannelRuntimeLogBuffer {
    pub fn new(capacity: usize, flood_window_s: f64) -> Self {
        Self {
            capacity: capacity.max(1),
            flood_window_s: flood_window_s.max(0.0),
            next_id: 1,
            latest_by_key: HashMap::new(),
            entries: VecDeque::new(),
        }
    }

    pub fn push(
        &mut self,
        level: ChannelRuntimeLogLevel,
        channel: &str,
        account_id: &str,
        event: &str,
        message: &str,
        ts: f64,
    ) {
        let normalized_channel = channel.trim().to_ascii_lowercase();
        let normalized_account = account_id.trim().to_string();
        let normalized_event = event.trim().to_ascii_lowercase();
        let clean_message = message.trim().to_string();
        let key = build_event_key(
            level,
            &normalized_channel,
            &normalized_account,
            &normalized_event,
            &clean_message,
        );

        if let Some(last_id) = self.latest_by_key.get(&key).copied() {
            if let Some(existing) = self.entries.iter_mut().find(|entry| entry.id == last_id) {
                if ts <= existing.updated_at + self.flood_window_s {
                    existing.updated_at = ts;
                    existing.entry.ts = ts;
                    existing.entry.message = clean_message;
                    existing.entry.repeat_count = existing.entry.repeat_count.saturating_add(1);
                    return;
                }
            } else {
                self.latest_by_key.remove(&key);
            }
        }

        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.latest_by_key.insert(key.clone(), id);
        self.entries.push_back(StoredRuntimeLog {
            id,
            key,
            updated_at: ts,
            entry: ChannelRuntimeLogEntry {
                ts,
                level: level.as_str().to_string(),
                channel: normalized_channel,
                account_id: normalized_account,
                event: normalized_event,
                message: clean_message,
                repeat_count: 1,
            },
        });
        self.trim_to_capacity();
    }

    pub fn list(
        &self,
        channel: Option<&str>,
        account_id: Option<&str>,
        limit: usize,
    ) -> Vec<ChannelRuntimeLogEntry> {
        let normalized_channel = channel
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        let normalized_account = account_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let max_items = limit.max(1);

        let mut items = Vec::new();
        for item in self.entries.iter().rev() {
            if let Some(channel_filter) = normalized_channel.as_deref() {
                if !item.entry.channel.eq_ignore_ascii_case(channel_filter) {
                    continue;
                }
            }
            if let Some(account_filter) = normalized_account.as_deref() {
                if item.entry.account_id != account_filter {
                    continue;
                }
            }
            items.push(item.entry.clone());
            if items.len() >= max_items {
                break;
            }
        }
        items
    }

    fn trim_to_capacity(&mut self) {
        while self.entries.len() > self.capacity {
            if let Some(removed) = self.entries.pop_front() {
                let should_remove = self
                    .latest_by_key
                    .get(&removed.key)
                    .is_some_and(|id| *id == removed.id);
                if should_remove {
                    self.latest_by_key.remove(&removed.key);
                }
            }
        }
    }
}

fn build_event_key(
    level: ChannelRuntimeLogLevel,
    channel: &str,
    account_id: &str,
    event: &str,
    message: &str,
) -> String {
    let normalized_message = normalize_message_for_key(message);
    format!(
        "{}|{}|{}|{}|{}",
        level.as_str(),
        channel,
        account_id,
        event,
        normalized_message
    )
}

fn normalize_message_for_key(message: &str) -> String {
    let mut output = String::new();
    let mut last_space = false;
    let mut last_digit = false;
    for ch in message.chars() {
        let mapped = if ch.is_ascii_digit() {
            if last_digit {
                continue;
            }
            last_digit = true;
            '#'
        } else if ch.is_ascii() {
            last_digit = false;
            ch.to_ascii_lowercase()
        } else {
            last_digit = false;
            ch
        };
        if mapped.is_whitespace() {
            if last_space {
                continue;
            }
            output.push(' ');
            last_space = true;
            continue;
        }
        if output.len() >= 180 {
            break;
        }
        output.push(mapped);
        last_space = false;
    }
    output.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{ChannelRuntimeLogBuffer, ChannelRuntimeLogEntry, ChannelRuntimeLogLevel};

    #[test]
    fn collapses_repeated_events_inside_flood_window() {
        let mut buffer = ChannelRuntimeLogBuffer::new(8, 20.0);
        buffer.push(
            ChannelRuntimeLogLevel::Warn,
            "xmpp",
            "acc_1",
            "long_connection_failed",
            "retry_in=3s connection refused",
            100.0,
        );
        buffer.push(
            ChannelRuntimeLogLevel::Warn,
            "xmpp",
            "acc_1",
            "long_connection_failed",
            "retry_in=30s connection refused",
            108.0,
        );
        assert_eq!(
            buffer.list(None, None, 10),
            vec![ChannelRuntimeLogEntry {
                ts: 108.0,
                level: "warn".to_string(),
                channel: "xmpp".to_string(),
                account_id: "acc_1".to_string(),
                event: "long_connection_failed".to_string(),
                message: "retry_in=30s connection refused".to_string(),
                repeat_count: 2,
            }]
        );
    }

    #[test]
    fn keeps_new_entry_after_flood_window_expired() {
        let mut buffer = ChannelRuntimeLogBuffer::new(8, 10.0);
        buffer.push(
            ChannelRuntimeLogLevel::Warn,
            "feishu",
            "acc_1",
            "long_connection_closed",
            "closed",
            100.0,
        );
        buffer.push(
            ChannelRuntimeLogLevel::Warn,
            "feishu",
            "acc_1",
            "long_connection_closed",
            "closed",
            111.0,
        );
        assert_eq!(buffer.list(None, None, 10).len(), 2);
        assert_eq!(buffer.list(None, None, 10)[0].repeat_count, 1);
        assert_eq!(buffer.list(None, None, 10)[1].repeat_count, 1);
    }

    #[test]
    fn evicts_oldest_items_when_capacity_reached() {
        let mut buffer = ChannelRuntimeLogBuffer::new(2, 5.0);
        buffer.push(
            ChannelRuntimeLogLevel::Info,
            "feishu",
            "acc_1",
            "event_a",
            "a",
            1.0,
        );
        buffer.push(
            ChannelRuntimeLogLevel::Info,
            "feishu",
            "acc_1",
            "event_b",
            "b",
            2.0,
        );
        buffer.push(
            ChannelRuntimeLogLevel::Info,
            "feishu",
            "acc_1",
            "event_c",
            "c",
            3.0,
        );

        let listed = buffer.list(None, None, 10);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].event, "event_c".to_string());
        assert_eq!(listed[1].event, "event_b".to_string());
    }
}
