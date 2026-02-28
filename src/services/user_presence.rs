use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_ONLINE_TTL_SECS: f64 = 90.0;
const DEFAULT_RETAIN_SECS: f64 = 24.0 * 60.0 * 60.0;
const GC_INTERVAL_SECS: f64 = 60.0;

#[derive(Debug, Clone, Serialize)]
pub struct UserPresenceView {
    pub online: bool,
    pub last_seen_at: f64,
    pub connection_count: i64,
}

#[derive(Debug, Clone)]
struct UserPresenceEntry {
    connection_count: i64,
    last_seen_at: f64,
}

#[derive(Debug)]
struct UserPresenceState {
    entries: HashMap<String, UserPresenceEntry>,
    last_gc_at: f64,
}

pub struct UserPresenceService {
    online_ttl_secs: f64,
    retain_secs: f64,
    state: Mutex<UserPresenceState>,
}

impl UserPresenceService {
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_ONLINE_TTL_SECS, DEFAULT_RETAIN_SECS)
    }

    pub fn with_ttl(online_ttl_secs: f64, retain_secs: f64) -> Self {
        let now = now_ts();
        Self {
            online_ttl_secs: online_ttl_secs.max(1.0),
            retain_secs: retain_secs.max(online_ttl_secs.max(1.0) * 2.0),
            state: Mutex::new(UserPresenceState {
                entries: HashMap::new(),
                last_gc_at: now,
            }),
        }
    }

    pub fn touch(&self, user_id: &str, now: f64) {
        self.update(user_id, now, |entry| {
            entry.last_seen_at = now;
        });
    }

    pub fn connect(&self, user_id: &str, now: f64) {
        self.update(user_id, now, |entry| {
            entry.connection_count = entry.connection_count.saturating_add(1);
            entry.last_seen_at = now;
        });
    }

    pub fn disconnect(&self, user_id: &str, now: f64) {
        self.update(user_id, now, |entry| {
            if entry.connection_count > 0 {
                entry.connection_count -= 1;
            }
            entry.last_seen_at = now;
        });
    }

    pub fn snapshot(&self, user_id: &str, now: f64) -> Option<UserPresenceView> {
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return None;
        }
        let now = normalized_now(now);
        let mut guard = self.state.lock().ok()?;
        self.gc_if_needed(&mut guard, now);
        let entry = guard.entries.get(cleaned)?;
        Some(self.to_view(entry, now))
    }

    pub fn snapshot_many<I, S>(&self, user_ids: I, now: f64) -> HashMap<String, UserPresenceView>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let now = normalized_now(now);
        let Some(mut guard) = self.state.lock().ok() else {
            return HashMap::new();
        };
        self.gc_if_needed(&mut guard, now);
        let mut snapshots = HashMap::new();
        for user_id in user_ids {
            let cleaned = user_id.as_ref().trim();
            if cleaned.is_empty() {
                continue;
            }
            if let Some(entry) = guard.entries.get(cleaned) {
                snapshots.insert(cleaned.to_string(), self.to_view(entry, now));
            }
        }
        snapshots
    }

    fn update<F>(&self, user_id: &str, now: f64, mut update_fn: F)
    where
        F: FnMut(&mut UserPresenceEntry),
    {
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return;
        }
        let now = normalized_now(now);
        let Some(mut guard) = self.state.lock().ok() else {
            return;
        };
        self.gc_if_needed(&mut guard, now);
        let entry = guard
            .entries
            .entry(cleaned.to_string())
            .or_insert(UserPresenceEntry {
                connection_count: 0,
                last_seen_at: now,
            });
        update_fn(entry);
    }

    fn to_view(&self, entry: &UserPresenceEntry, now: f64) -> UserPresenceView {
        let elapsed = (now - entry.last_seen_at).max(0.0);
        UserPresenceView {
            online: entry.connection_count > 0 || elapsed <= self.online_ttl_secs,
            last_seen_at: entry.last_seen_at,
            connection_count: entry.connection_count,
        }
    }

    fn gc_if_needed(&self, state: &mut UserPresenceState, now: f64) {
        if now - state.last_gc_at < GC_INTERVAL_SECS {
            return;
        }
        state.last_gc_at = now;
        state.entries.retain(|_, entry| {
            entry.connection_count > 0 || now - entry.last_seen_at <= self.retain_secs
        });
    }
}

impl Default for UserPresenceService {
    fn default() -> Self {
        Self::new()
    }
}

fn normalized_now(now: f64) -> f64 {
    if now.is_finite() && now > 0.0 {
        now
    } else {
        now_ts()
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::UserPresenceService;

    #[test]
    fn touch_marks_user_online_within_ttl() {
        let service = UserPresenceService::with_ttl(5.0, 60.0);
        service.touch("alice", 10.0);
        let snapshot = service
            .snapshot("alice", 14.0)
            .expect("presence should exist");
        assert!(snapshot.online);
        assert_eq!(snapshot.connection_count, 0);
        assert_eq!(snapshot.last_seen_at, 10.0);
    }

    #[test]
    fn touch_expires_after_ttl() {
        let service = UserPresenceService::with_ttl(5.0, 60.0);
        service.touch("alice", 10.0);
        let snapshot = service
            .snapshot("alice", 20.0)
            .expect("presence should exist");
        assert!(!snapshot.online);
    }

    #[test]
    fn connect_disconnect_updates_connection_count() {
        let service = UserPresenceService::with_ttl(5.0, 60.0);
        service.connect("alice", 10.0);
        service.connect("alice", 12.0);
        let online = service
            .snapshot("alice", 100.0)
            .expect("presence should exist");
        assert!(online.online);
        assert_eq!(online.connection_count, 2);
        service.disconnect("alice", 101.0);
        let still_online = service
            .snapshot("alice", 102.0)
            .expect("presence should exist");
        assert!(still_online.online);
        assert_eq!(still_online.connection_count, 1);
        service.disconnect("alice", 103.0);
        let from_touch = service
            .snapshot("alice", 104.0)
            .expect("presence should exist");
        assert!(from_touch.online);
        assert_eq!(from_touch.connection_count, 0);
        let offline = service
            .snapshot("alice", 110.0)
            .expect("presence should exist");
        assert!(!offline.online);
    }
}
