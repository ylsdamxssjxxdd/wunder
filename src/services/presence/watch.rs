use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

const GC_INTERVAL_SECS: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionTargetKind {
    Session,
    BeeroomGroup,
    UserWorldConversation,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectionWatchView {
    pub target_kind: ProjectionTargetKind,
    pub target_id: String,
    pub watch_count: i64,
    pub user_count: i64,
    pub last_seen_at: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ProjectionWatchMetrics {
    pub total_watch_count: i64,
    pub total_target_count: i64,
    pub session_watch_count: i64,
    pub beeroom_group_watch_count: i64,
    pub user_world_conversation_watch_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProjectionTarget {
    kind: ProjectionTargetKind,
    target_id: String,
}

#[derive(Debug, Clone)]
struct ProjectionWatchEntry {
    connection_id: String,
    user_id: String,
    target: ProjectionTarget,
}

#[derive(Debug, Default)]
struct ProjectionTargetAggregate {
    watch_keys: HashSet<String>,
    user_counts: HashMap<String, usize>,
    last_seen_at: f64,
}

#[derive(Debug)]
struct ProjectionWatchState {
    entries: HashMap<String, ProjectionWatchEntry>,
    by_connection: HashMap<String, HashSet<String>>,
    by_target: HashMap<ProjectionTarget, ProjectionTargetAggregate>,
    last_gc_at: f64,
}

pub struct ProjectionWatchService {
    state: RwLock<ProjectionWatchState>,
}

impl ProjectionWatchService {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(ProjectionWatchState {
                entries: HashMap::new(),
                by_connection: HashMap::new(),
                by_target: HashMap::new(),
                last_gc_at: now_ts(),
            }),
        }
    }

    pub fn watch(
        &self,
        connection_id: &str,
        request_id: &str,
        user_id: &str,
        target_kind: ProjectionTargetKind,
        target_id: &str,
        now: f64,
    ) {
        let cleaned_connection_id = normalize_key(connection_id);
        let cleaned_request_id = normalize_key(request_id);
        let cleaned_user_id = normalize_key(user_id);
        let cleaned_target_id = normalize_key(target_id);
        if cleaned_connection_id.is_empty()
            || cleaned_request_id.is_empty()
            || cleaned_user_id.is_empty()
            || cleaned_target_id.is_empty()
        {
            return;
        }
        let now = normalized_now(now);
        let watch_key = make_watch_key(&cleaned_connection_id, &cleaned_request_id);
        let target = ProjectionTarget {
            kind: target_kind,
            target_id: cleaned_target_id,
        };
        let Some(mut guard) = self.state.write().ok() else {
            return;
        };
        self.gc_if_needed(&mut guard, now);
        if let Some(previous) = guard.entries.remove(&watch_key) {
            detach_watch(&mut guard, &watch_key, &previous);
        }
        let entry = ProjectionWatchEntry {
            connection_id: cleaned_connection_id.clone(),
            user_id: cleaned_user_id.clone(),
            target: target.clone(),
        };
        guard.entries.insert(watch_key.clone(), entry);
        guard
            .by_connection
            .entry(cleaned_connection_id)
            .or_default()
            .insert(watch_key.clone());
        let aggregate = guard.by_target.entry(target).or_default();
        aggregate.watch_keys.insert(watch_key);
        *aggregate.user_counts.entry(cleaned_user_id).or_insert(0) += 1;
        aggregate.last_seen_at = now;
    }

    pub fn unwatch(&self, connection_id: &str, request_id: &str) {
        let cleaned_connection_id = normalize_key(connection_id);
        let cleaned_request_id = normalize_key(request_id);
        if cleaned_connection_id.is_empty() || cleaned_request_id.is_empty() {
            return;
        }
        let watch_key = make_watch_key(&cleaned_connection_id, &cleaned_request_id);
        let Some(mut guard) = self.state.write().ok() else {
            return;
        };
        if let Some(entry) = guard.entries.remove(&watch_key) {
            detach_watch(&mut guard, &watch_key, &entry);
        }
    }

    pub fn disconnect_connection(&self, connection_id: &str) {
        let cleaned_connection_id = normalize_key(connection_id);
        if cleaned_connection_id.is_empty() {
            return;
        }
        let Some(mut guard) = self.state.write().ok() else {
            return;
        };
        let watch_keys = guard
            .by_connection
            .remove(&cleaned_connection_id)
            .unwrap_or_default();
        for watch_key in watch_keys {
            if let Some(entry) = guard.entries.remove(&watch_key) {
                detach_watch(&mut guard, &watch_key, &entry);
            }
        }
    }

    pub fn snapshot(
        &self,
        target_kind: ProjectionTargetKind,
        target_id: &str,
        now: f64,
    ) -> Option<ProjectionWatchView> {
        let cleaned_target_id = normalize_key(target_id);
        if cleaned_target_id.is_empty() {
            return None;
        }
        let now = normalized_now(now);
        let Some(mut guard) = self.state.write().ok() else {
            return None;
        };
        self.gc_if_needed(&mut guard, now);
        let target = ProjectionTarget {
            kind: target_kind,
            target_id: cleaned_target_id.clone(),
        };
        let aggregate = guard.by_target.get(&target)?;
        Some(ProjectionWatchView {
            target_kind,
            target_id: cleaned_target_id,
            watch_count: aggregate.watch_keys.len() as i64,
            user_count: aggregate.user_counts.len() as i64,
            last_seen_at: aggregate.last_seen_at,
        })
    }

    pub fn metrics(&self, now: f64) -> ProjectionWatchMetrics {
        let now = normalized_now(now);
        let Some(mut guard) = self.state.write().ok() else {
            return ProjectionWatchMetrics::default();
        };
        self.gc_if_needed(&mut guard, now);
        let mut metrics = ProjectionWatchMetrics {
            total_watch_count: guard.entries.len() as i64,
            total_target_count: guard.by_target.len() as i64,
            ..ProjectionWatchMetrics::default()
        };
        for entry in guard.entries.values() {
            match entry.target.kind {
                ProjectionTargetKind::Session => metrics.session_watch_count += 1,
                ProjectionTargetKind::BeeroomGroup => metrics.beeroom_group_watch_count += 1,
                ProjectionTargetKind::UserWorldConversation => {
                    metrics.user_world_conversation_watch_count += 1;
                }
            }
        }
        metrics
    }

    fn gc_if_needed(&self, state: &mut ProjectionWatchState, now: f64) {
        if now - state.last_gc_at < GC_INTERVAL_SECS {
            return;
        }
        state.last_gc_at = now;
        state.by_connection.retain(|_, watch_keys| {
            watch_keys.retain(|watch_key| state.entries.contains_key(watch_key));
            !watch_keys.is_empty()
        });
        state
            .by_target
            .retain(|_, aggregate| !aggregate.watch_keys.is_empty());
    }
}

impl Default for ProjectionWatchService {
    fn default() -> Self {
        Self::new()
    }
}

fn detach_watch(state: &mut ProjectionWatchState, watch_key: &str, entry: &ProjectionWatchEntry) {
    if let Some(connection_watch_keys) = state.by_connection.get_mut(&entry.connection_id) {
        connection_watch_keys.remove(watch_key);
        if connection_watch_keys.is_empty() {
            state.by_connection.remove(&entry.connection_id);
        }
    }
    if let Some(aggregate) = state.by_target.get_mut(&entry.target) {
        aggregate.watch_keys.remove(watch_key);
        if let Some(count) = aggregate.user_counts.get_mut(&entry.user_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                aggregate.user_counts.remove(&entry.user_id);
            }
        }
        if aggregate.watch_keys.is_empty() {
            state.by_target.remove(&entry.target);
        }
    }
}

fn make_watch_key(connection_id: &str, request_id: &str) -> String {
    format!("{connection_id}:{request_id}")
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
