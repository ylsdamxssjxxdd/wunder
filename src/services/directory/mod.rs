use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteTargetKind {
    Thread,
    Mission,
    Projection,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSubmitLeaseSnapshot {
    pub session_id: String,
    pub owner_id: String,
    pub epoch: u64,
    pub acquired_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteLeaseSnapshot {
    pub target_kind: RouteTargetKind,
    pub target_id: String,
    pub owner_id: String,
    pub session_id: Option<String>,
    pub user_id: Option<String>,
    pub epoch: u64,
    pub acquired_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RouteLeaseMetricsSnapshot {
    pub submit_lease_count: i64,
    pub active_route_count: i64,
    pub thread_route_count: i64,
    pub mission_route_count: i64,
    pub projection_route_count: i64,
}

#[derive(Debug, Clone)]
struct SessionSubmitLeaseEntry {
    session_id: String,
    owner_id: String,
    epoch: u64,
    acquired_at: f64,
    updated_at: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RouteTargetKey {
    kind: RouteTargetKind,
    target_id: String,
}

#[derive(Debug, Clone)]
struct RouteLeaseEntry {
    key: RouteTargetKey,
    owner_id: String,
    session_id: Option<String>,
    user_id: Option<String>,
    epoch: u64,
    acquired_at: f64,
    updated_at: f64,
}

#[derive(Debug)]
struct RouteLeaseState {
    next_epoch: u64,
    submit_leases: HashMap<String, SessionSubmitLeaseEntry>,
    route_leases: HashMap<RouteTargetKey, RouteLeaseEntry>,
}

#[derive(Debug)]
pub struct RouteLeaseService {
    state: RwLock<RouteLeaseState>,
}

#[derive(Debug)]
pub struct SessionSubmitLeaseGuard {
    service: Arc<RouteLeaseService>,
    session_id: String,
    epoch: u64,
}

impl Drop for SessionSubmitLeaseGuard {
    fn drop(&mut self) {
        self.service
            .release_submit_lease_if_matches(&self.session_id, self.epoch);
    }
}

#[derive(Debug)]
pub struct RouteLeaseGuard {
    service: Arc<RouteLeaseService>,
    target_kind: RouteTargetKind,
    target_id: String,
    epoch: u64,
}

impl RouteLeaseGuard {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

impl Drop for RouteLeaseGuard {
    fn drop(&mut self) {
        self.service
            .release_route_lease_if_matches(self.target_kind, &self.target_id, self.epoch);
    }
}

impl RouteLeaseService {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(RouteLeaseState {
                next_epoch: 1,
                submit_leases: HashMap::new(),
                route_leases: HashMap::new(),
            }),
        }
    }

    pub fn try_acquire_submit_lease(
        self: &Arc<Self>,
        session_id: &str,
        owner_id: &str,
    ) -> Option<SessionSubmitLeaseGuard> {
        let cleaned_session_id = normalize_key(session_id);
        let cleaned_owner_id = normalize_key(owner_id);
        if cleaned_session_id.is_empty() || cleaned_owner_id.is_empty() {
            return None;
        }
        let now = now_ts();
        let Some(mut guard) = self.state.write().ok() else {
            return None;
        };
        if guard.submit_leases.contains_key(&cleaned_session_id) {
            return None;
        }
        let epoch = next_epoch(&mut guard);
        guard.submit_leases.insert(
            cleaned_session_id.clone(),
            SessionSubmitLeaseEntry {
                session_id: cleaned_session_id.clone(),
                owner_id: cleaned_owner_id,
                epoch,
                acquired_at: now,
                updated_at: now,
            },
        );
        Some(SessionSubmitLeaseGuard {
            service: self.clone(),
            session_id: cleaned_session_id,
            epoch,
        })
    }

    pub fn try_acquire_route_lease(
        self: &Arc<Self>,
        target_kind: RouteTargetKind,
        target_id: &str,
        owner_id: &str,
        session_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Option<RouteLeaseGuard> {
        let cleaned_target_id = normalize_key(target_id);
        let cleaned_owner_id = normalize_key(owner_id);
        if cleaned_target_id.is_empty() || cleaned_owner_id.is_empty() {
            return None;
        }
        let key = RouteTargetKey {
            kind: target_kind,
            target_id: cleaned_target_id.clone(),
        };
        let now = now_ts();
        let Some(mut guard) = self.state.write().ok() else {
            return None;
        };
        if guard.route_leases.contains_key(&key) {
            return None;
        }
        let epoch = next_epoch(&mut guard);
        guard.route_leases.insert(
            key.clone(),
            RouteLeaseEntry {
                key,
                owner_id: cleaned_owner_id,
                session_id: normalize_optional_key(session_id),
                user_id: normalize_optional_key(user_id),
                epoch,
                acquired_at: now,
                updated_at: now,
            },
        );
        Some(RouteLeaseGuard {
            service: self.clone(),
            target_kind,
            target_id: cleaned_target_id,
            epoch,
        })
    }

    pub fn submit_snapshot(&self, session_id: &str) -> Option<SessionSubmitLeaseSnapshot> {
        let cleaned_session_id = normalize_key(session_id);
        if cleaned_session_id.is_empty() {
            return None;
        }
        let guard = self.state.read().ok()?;
        let entry = guard.submit_leases.get(&cleaned_session_id)?;
        Some(SessionSubmitLeaseSnapshot {
            session_id: entry.session_id.clone(),
            owner_id: entry.owner_id.clone(),
            epoch: entry.epoch,
            acquired_at: entry.acquired_at,
            updated_at: entry.updated_at,
        })
    }

    pub fn route_snapshot(
        &self,
        target_kind: RouteTargetKind,
        target_id: &str,
    ) -> Option<RouteLeaseSnapshot> {
        let cleaned_target_id = normalize_key(target_id);
        if cleaned_target_id.is_empty() {
            return None;
        }
        let guard = self.state.read().ok()?;
        let entry = guard.route_leases.get(&RouteTargetKey {
            kind: target_kind,
            target_id: cleaned_target_id,
        })?;
        Some(route_entry_to_snapshot(entry))
    }

    pub fn metrics_snapshot(&self) -> RouteLeaseMetricsSnapshot {
        let Some(guard) = self.state.read().ok() else {
            return RouteLeaseMetricsSnapshot::default();
        };
        let mut snapshot = RouteLeaseMetricsSnapshot {
            submit_lease_count: guard.submit_leases.len() as i64,
            active_route_count: guard.route_leases.len() as i64,
            ..RouteLeaseMetricsSnapshot::default()
        };
        for entry in guard.route_leases.values() {
            match entry.key.kind {
                RouteTargetKind::Thread => snapshot.thread_route_count += 1,
                RouteTargetKind::Mission => snapshot.mission_route_count += 1,
                RouteTargetKind::Projection => snapshot.projection_route_count += 1,
            }
        }
        snapshot
    }

    fn release_submit_lease_if_matches(&self, session_id: &str, epoch: u64) {
        let cleaned_session_id = normalize_key(session_id);
        if cleaned_session_id.is_empty() {
            return;
        }
        let Some(mut guard) = self.state.write().ok() else {
            return;
        };
        let should_remove = guard
            .submit_leases
            .get(&cleaned_session_id)
            .map(|entry| entry.epoch == epoch)
            .unwrap_or(false);
        if should_remove {
            guard.submit_leases.remove(&cleaned_session_id);
        }
    }

    fn release_route_lease_if_matches(
        &self,
        target_kind: RouteTargetKind,
        target_id: &str,
        epoch: u64,
    ) {
        let cleaned_target_id = normalize_key(target_id);
        if cleaned_target_id.is_empty() {
            return;
        }
        let key = RouteTargetKey {
            kind: target_kind,
            target_id: cleaned_target_id,
        };
        let Some(mut guard) = self.state.write().ok() else {
            return;
        };
        let should_remove = guard
            .route_leases
            .get(&key)
            .map(|entry| entry.epoch == epoch)
            .unwrap_or(false);
        if should_remove {
            guard.route_leases.remove(&key);
        }
    }
}

impl Default for RouteLeaseService {
    fn default() -> Self {
        Self::new()
    }
}

fn route_entry_to_snapshot(entry: &RouteLeaseEntry) -> RouteLeaseSnapshot {
    RouteLeaseSnapshot {
        target_kind: entry.key.kind,
        target_id: entry.key.target_id.clone(),
        owner_id: entry.owner_id.clone(),
        session_id: entry.session_id.clone(),
        user_id: entry.user_id.clone(),
        epoch: entry.epoch,
        acquired_at: entry.acquired_at,
        updated_at: entry.updated_at,
    }
}

fn next_epoch(state: &mut RouteLeaseState) -> u64 {
    let epoch = state.next_epoch;
    state.next_epoch = state.next_epoch.saturating_add(1);
    epoch
}

fn normalize_key(raw: &str) -> String {
    raw.trim().to_string()
}

fn normalize_optional_key(raw: Option<&str>) -> Option<String> {
    raw.map(normalize_key).filter(|value| !value.is_empty())
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::{RouteLeaseService, RouteTargetKind};
    use std::sync::Arc;

    #[test]
    fn submit_lease_is_single_owner_and_released_on_drop() {
        let service = Arc::new(RouteLeaseService::new());
        let guard = service
            .try_acquire_submit_lease("sess-1", "runtime-a")
            .expect("first submit lease should succeed");
        assert!(service
            .try_acquire_submit_lease("sess-1", "runtime-b")
            .is_none());
        let snapshot = service
            .submit_snapshot("sess-1")
            .expect("submit snapshot should exist");
        assert_eq!(snapshot.owner_id, "runtime-a");
        drop(guard);
        assert!(service.submit_snapshot("sess-1").is_none());
    }

    #[test]
    fn route_lease_is_single_owner_and_reports_metrics() {
        let service = Arc::new(RouteLeaseService::new());
        let thread_guard = service
            .try_acquire_route_lease(
                RouteTargetKind::Thread,
                "thread_sess-1",
                "runtime-thread",
                Some("sess-1"),
                Some("user-1"),
            )
            .expect("thread lease should succeed");
        let mission_guard = service
            .try_acquire_route_lease(
                RouteTargetKind::Mission,
                "run-1",
                "runtime-mission",
                None,
                Some("user-1"),
            )
            .expect("mission lease should succeed");
        assert!(service
            .try_acquire_route_lease(
                RouteTargetKind::Thread,
                "thread_sess-1",
                "runtime-other",
                Some("sess-1"),
                Some("user-1"),
            )
            .is_none());
        let snapshot = service
            .route_snapshot(RouteTargetKind::Thread, "thread_sess-1")
            .expect("thread route snapshot should exist");
        assert_eq!(snapshot.owner_id, "runtime-thread");
        assert_eq!(snapshot.session_id.as_deref(), Some("sess-1"));
        assert_eq!(snapshot.user_id.as_deref(), Some("user-1"));
        let metrics = service.metrics_snapshot();
        assert_eq!(metrics.active_route_count, 2);
        assert_eq!(metrics.thread_route_count, 1);
        assert_eq!(metrics.mission_route_count, 1);
        drop(thread_guard);
        drop(mission_guard);
        let metrics = service.metrics_snapshot();
        assert_eq!(metrics.active_route_count, 0);
    }
}
