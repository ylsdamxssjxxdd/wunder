use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
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
struct UserConnectionEntry {
    connection_ids: HashSet<String>,
    last_seen_at: f64,
}

#[derive(Debug)]
struct ConnectionPresenceState {
    entries: HashMap<String, UserConnectionEntry>,
    connection_owners: HashMap<String, String>,
    last_gc_at: f64,
}

pub struct ConnectionPresenceService {
    online_ttl_secs: f64,
    retain_secs: f64,
    state: RwLock<ConnectionPresenceState>,
}

impl ConnectionPresenceService {
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_ONLINE_TTL_SECS, DEFAULT_RETAIN_SECS)
    }

    pub fn with_ttl(online_ttl_secs: f64, retain_secs: f64) -> Self {
        let now = now_ts();
        Self {
            online_ttl_secs: online_ttl_secs.max(1.0),
            retain_secs: retain_secs.max(online_ttl_secs.max(1.0) * 2.0),
            state: RwLock::new(ConnectionPresenceState {
                entries: HashMap::new(),
                connection_owners: HashMap::new(),
                last_gc_at: now,
            }),
        }
    }

    pub fn touch(&self, user_id: &str, now: f64) {
        let cleaned = normalize_key(user_id);
        if cleaned.is_empty() {
            return;
        }
        let now = normalized_now(now);
        let Some(mut guard) = self.state.write().ok() else {
            return;
        };
        self.gc_if_needed(&mut guard, now);
        let entry = guard.entries.entry(cleaned).or_insert(UserConnectionEntry {
            connection_ids: HashSet::new(),
            last_seen_at: now,
        });
        entry.last_seen_at = now;
    }

    pub fn connect(&self, user_id: &str, connection_id: &str, now: f64) {
        let cleaned_user_id = normalize_key(user_id);
        let cleaned_connection_id = normalize_key(connection_id);
        if cleaned_user_id.is_empty() || cleaned_connection_id.is_empty() {
            return;
        }
        let now = normalized_now(now);
        let Some(mut guard) = self.state.write().ok() else {
            return;
        };
        self.gc_if_needed(&mut guard, now);
        if let Some(previous_owner) = guard
            .connection_owners
            .insert(cleaned_connection_id.clone(), cleaned_user_id.clone())
        {
            remove_connection_from_user(
                &mut guard.entries,
                &previous_owner,
                &cleaned_connection_id,
                now,
            );
        }
        let entry = guard
            .entries
            .entry(cleaned_user_id)
            .or_insert(UserConnectionEntry {
                connection_ids: HashSet::new(),
                last_seen_at: now,
            });
        entry.connection_ids.insert(cleaned_connection_id);
        entry.last_seen_at = now;
    }

    pub fn disconnect(&self, user_id: &str, connection_id: &str, now: f64) {
        let cleaned_user_id = normalize_key(user_id);
        let cleaned_connection_id = normalize_key(connection_id);
        if cleaned_user_id.is_empty() || cleaned_connection_id.is_empty() {
            return;
        }
        let now = normalized_now(now);
        let Some(mut guard) = self.state.write().ok() else {
            return;
        };
        self.gc_if_needed(&mut guard, now);
        let owner = guard
            .connection_owners
            .remove(&cleaned_connection_id)
            .unwrap_or(cleaned_user_id);
        remove_connection_from_user(&mut guard.entries, &owner, &cleaned_connection_id, now);
    }

    pub fn snapshot(&self, user_id: &str, now: f64) -> Option<UserPresenceView> {
        let cleaned = normalize_key(user_id);
        if cleaned.is_empty() {
            return None;
        }
        let now = normalized_now(now);
        let Some(mut guard) = self.state.write().ok() else {
            return None;
        };
        self.gc_if_needed(&mut guard, now);
        let entry = guard.entries.get(&cleaned)?;
        Some(self.to_view(entry, now))
    }

    pub fn snapshot_many<I, S>(&self, user_ids: I, now: f64) -> HashMap<String, UserPresenceView>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let now = normalized_now(now);
        let Some(mut guard) = self.state.write().ok() else {
            return HashMap::new();
        };
        self.gc_if_needed(&mut guard, now);
        let mut snapshots = HashMap::new();
        for user_id in user_ids {
            let cleaned = normalize_key(user_id.as_ref());
            if cleaned.is_empty() {
                continue;
            }
            if let Some(entry) = guard.entries.get(&cleaned) {
                snapshots.insert(cleaned, self.to_view(entry, now));
            }
        }
        snapshots
    }

    fn to_view(&self, entry: &UserConnectionEntry, now: f64) -> UserPresenceView {
        let elapsed = (now - entry.last_seen_at).max(0.0);
        UserPresenceView {
            online: !entry.connection_ids.is_empty() || elapsed <= self.online_ttl_secs,
            last_seen_at: entry.last_seen_at,
            connection_count: entry.connection_ids.len() as i64,
        }
    }

    fn gc_if_needed(&self, state: &mut ConnectionPresenceState, now: f64) {
        if now - state.last_gc_at < GC_INTERVAL_SECS {
            return;
        }
        state.last_gc_at = now;
        state.entries.retain(|_, entry| {
            !entry.connection_ids.is_empty() || now - entry.last_seen_at <= self.retain_secs
        });
        state.connection_owners.retain(|connection_id, user_id| {
            state
                .entries
                .get(user_id)
                .map(|entry| entry.connection_ids.contains(connection_id))
                .unwrap_or(false)
        });
    }
}

impl Default for ConnectionPresenceService {
    fn default() -> Self {
        Self::new()
    }
}

fn remove_connection_from_user(
    entries: &mut HashMap<String, UserConnectionEntry>,
    user_id: &str,
    connection_id: &str,
    now: f64,
) {
    let Some(entry) = entries.get_mut(user_id) else {
        return;
    };
    entry.connection_ids.remove(connection_id);
    entry.last_seen_at = now;
}

fn normalize_key(raw: &str) -> String {
    raw.trim().to_string()
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
