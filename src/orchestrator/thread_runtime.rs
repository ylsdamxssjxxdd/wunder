use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThreadRuntimeStatus {
    NotLoaded,
    Idle,
    Running,
    WaitingApproval,
    WaitingUserInput,
    #[allow(dead_code)]
    SystemError,
}

impl ThreadRuntimeStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::NotLoaded => "not_loaded",
            Self::Idle => "idle",
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::WaitingUserInput => "waiting_user_input",
            Self::SystemError => "system_error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ThreadRuntimeSnapshot {
    pub(super) session_id: String,
    pub(super) status: ThreadRuntimeStatus,
    pub(super) subscriber_count: usize,
    pub(super) active_turn_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ThreadRuntimeCloseEvent {
    pub(super) session_id: String,
    pub(super) last_status: ThreadRuntimeStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ThreadRuntimeUpdate {
    pub(super) status: Option<ThreadRuntimeSnapshot>,
    pub(super) closed: Option<ThreadRuntimeCloseEvent>,
}

#[derive(Default)]
pub(super) struct ThreadRuntimeRegistry {
    inner: Mutex<HashMap<String, ThreadRuntimeEntry>>,
}

#[derive(Clone, Debug)]
struct ThreadRuntimeEntry {
    status: ThreadRuntimeStatus,
    subscriber_count: usize,
    active_turn_id: Option<String>,
}

impl ThreadRuntimeRegistry {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn attach_subscriber(&self, session_id: &str) -> Option<ThreadRuntimeSnapshot> {
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return None;
        }
        let mut guard = self.inner.lock();
        let entry =
            guard
                .entry(cleaned_session.to_string())
                .or_insert_with(|| ThreadRuntimeEntry {
                    status: ThreadRuntimeStatus::Idle,
                    subscriber_count: 0,
                    active_turn_id: None,
                });
        entry.subscriber_count = entry.subscriber_count.saturating_add(1);
        Some(snapshot_from_entry(cleaned_session, entry))
    }

    pub(super) fn detach_subscriber(&self, session_id: &str) -> ThreadRuntimeUpdate {
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return ThreadRuntimeUpdate {
                status: None,
                closed: None,
            };
        }
        let mut guard = self.inner.lock();
        let Some(entry) = guard.get_mut(cleaned_session) else {
            return ThreadRuntimeUpdate {
                status: None,
                closed: None,
            };
        };
        if entry.subscriber_count > 0 {
            entry.subscriber_count -= 1;
        }
        if entry.subscriber_count == 0 && entry.active_turn_id.is_none() {
            let last_status = entry.status;
            guard.remove(cleaned_session);
            return ThreadRuntimeUpdate {
                status: None,
                closed: Some(ThreadRuntimeCloseEvent {
                    session_id: cleaned_session.to_string(),
                    last_status,
                }),
            };
        }
        ThreadRuntimeUpdate {
            status: None,
            closed: None,
        }
    }

    pub(super) fn begin_turn(&self, session_id: &str, turn_id: &str) -> ThreadRuntimeUpdate {
        self.transition(
            session_id,
            Some(turn_id),
            ThreadRuntimeStatus::Running,
            true,
        )
    }

    pub(super) fn snapshot(&self, session_id: &str) -> Option<ThreadRuntimeSnapshot> {
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return None;
        }
        self.inner
            .lock()
            .get(cleaned_session)
            .map(|entry| snapshot_from_entry(cleaned_session, entry))
    }

    pub(super) fn set_status(
        &self,
        session_id: &str,
        turn_id: &str,
        status: ThreadRuntimeStatus,
    ) -> ThreadRuntimeUpdate {
        self.transition(session_id, Some(turn_id), status, false)
    }

    pub(super) fn finish_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        status: ThreadRuntimeStatus,
    ) -> ThreadRuntimeUpdate {
        let cleaned_session = session_id.trim();
        let cleaned_turn = turn_id.trim();
        if cleaned_session.is_empty() || cleaned_turn.is_empty() {
            return ThreadRuntimeUpdate {
                status: None,
                closed: None,
            };
        }
        let mut guard = self.inner.lock();
        let Some(entry) = guard.get_mut(cleaned_session) else {
            return ThreadRuntimeUpdate {
                status: None,
                closed: None,
            };
        };
        if entry.active_turn_id.as_deref() != Some(cleaned_turn) {
            return ThreadRuntimeUpdate {
                status: None,
                closed: None,
            };
        }
        let previous = snapshot_from_entry(cleaned_session, entry);
        entry.status = status;
        entry.active_turn_id = None;
        let next_snapshot = snapshot_from_entry(cleaned_session, entry);
        let status_snapshot = if previous != next_snapshot {
            Some(next_snapshot)
        } else {
            None
        };
        if entry.subscriber_count == 0 {
            let last_status = entry.status;
            guard.remove(cleaned_session);
            return ThreadRuntimeUpdate {
                status: status_snapshot,
                closed: Some(ThreadRuntimeCloseEvent {
                    session_id: cleaned_session.to_string(),
                    last_status,
                }),
            };
        }
        ThreadRuntimeUpdate {
            status: status_snapshot,
            closed: None,
        }
    }

    fn transition(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        status: ThreadRuntimeStatus,
        force_turn_id: bool,
    ) -> ThreadRuntimeUpdate {
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return ThreadRuntimeUpdate {
                status: None,
                closed: None,
            };
        }
        let mut guard = self.inner.lock();
        let entry =
            guard
                .entry(cleaned_session.to_string())
                .or_insert_with(|| ThreadRuntimeEntry {
                    status: ThreadRuntimeStatus::Idle,
                    subscriber_count: 0,
                    active_turn_id: None,
                });
        let previous = snapshot_from_entry(cleaned_session, entry);
        let Some(turn_id) = turn_id.map(str::trim).filter(|value| !value.is_empty()) else {
            return ThreadRuntimeUpdate {
                status: None,
                closed: None,
            };
        };
        if !force_turn_id && entry.active_turn_id.as_deref() != Some(turn_id) {
            return ThreadRuntimeUpdate {
                status: None,
                closed: None,
            };
        }
        entry.status = status;
        entry.active_turn_id = Some(turn_id.to_string());
        let next_snapshot = snapshot_from_entry(cleaned_session, entry);
        let status = if previous != next_snapshot {
            Some(next_snapshot)
        } else {
            None
        };
        ThreadRuntimeUpdate {
            status,
            closed: None,
        }
    }
}

pub(super) fn thread_status_payload(snapshot: &ThreadRuntimeSnapshot) -> Value {
    json!({
        "session_id": snapshot.session_id,
        "thread_id": format!("thread_{}", snapshot.session_id),
        "status": snapshot.status.as_str(),
        "loaded": snapshot.status != ThreadRuntimeStatus::NotLoaded,
        "subscriber_count": snapshot.subscriber_count,
        "active_turn_id": snapshot.active_turn_id,
    })
}

pub(super) fn thread_closed_payload(event: &ThreadRuntimeCloseEvent) -> Value {
    json!({
        "session_id": event.session_id,
        "thread_id": format!("thread_{}", event.session_id),
        "status": ThreadRuntimeStatus::NotLoaded.as_str(),
        "loaded": false,
        "subscriber_count": 0,
        "last_status": event.last_status.as_str(),
        "reason": "runtime_unloaded",
    })
}

pub(super) fn thread_not_loaded_payload(event: &ThreadRuntimeCloseEvent) -> Value {
    json!({
        "session_id": event.session_id,
        "thread_id": format!("thread_{}", event.session_id),
        "status": ThreadRuntimeStatus::NotLoaded.as_str(),
        "loaded": false,
        "subscriber_count": 0,
        "active_turn_id": Value::Null,
    })
}

fn snapshot_from_entry(session_id: &str, entry: &ThreadRuntimeEntry) -> ThreadRuntimeSnapshot {
    ThreadRuntimeSnapshot {
        session_id: session_id.to_string(),
        status: entry.status,
        subscriber_count: entry.subscriber_count,
        active_turn_id: entry.active_turn_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        thread_closed_payload, thread_not_loaded_payload, thread_status_payload,
        ThreadRuntimeRegistry, ThreadRuntimeStatus,
    };

    #[test]
    fn keeps_running_runtime_loaded_without_subscribers() {
        let registry = ThreadRuntimeRegistry::new();
        let _ = registry.attach_subscriber("sess_1");
        let _ = registry.begin_turn("sess_1", "turn_1");

        let update = registry.detach_subscriber("sess_1");
        assert!(update.closed.is_none());

        let finish = registry.finish_turn("sess_1", "turn_1", ThreadRuntimeStatus::Idle);
        assert_eq!(
            finish.closed.expect("closed event").last_status,
            ThreadRuntimeStatus::Idle
        );
    }

    #[test]
    fn unloads_idle_runtime_when_last_subscriber_leaves() {
        let registry = ThreadRuntimeRegistry::new();
        let _ = registry.attach_subscriber("sess_1");

        let update = registry.detach_subscriber("sess_1");
        assert_eq!(
            update.closed.expect("closed event").last_status,
            ThreadRuntimeStatus::Idle
        );
    }

    #[test]
    fn status_payload_exposes_loaded_runtime_fields() {
        let registry = ThreadRuntimeRegistry::new();
        let _ = registry.attach_subscriber("sess_1");
        let update = registry.begin_turn("sess_1", "turn_1");
        let payload = thread_status_payload(&update.status.expect("status snapshot"));
        assert_eq!(payload["session_id"], "sess_1");
        assert_eq!(payload["status"], "running");
        assert_eq!(payload["loaded"], true);
        assert_eq!(payload["subscriber_count"], 1);
    }

    #[test]
    fn snapshot_returns_current_runtime_state() {
        let registry = ThreadRuntimeRegistry::new();
        let _ = registry.attach_subscriber("sess_1");
        let _ = registry.begin_turn("sess_1", "turn_1");

        let snapshot = registry.snapshot("sess_1").expect("runtime snapshot");
        assert_eq!(snapshot.status, ThreadRuntimeStatus::Running);
        assert_eq!(snapshot.subscriber_count, 1);
        assert_eq!(snapshot.active_turn_id.as_deref(), Some("turn_1"));
    }

    #[test]
    fn closed_payload_reports_not_loaded_status() {
        let payload = thread_closed_payload(&super::ThreadRuntimeCloseEvent {
            session_id: "sess_1".to_string(),
            last_status: ThreadRuntimeStatus::WaitingUserInput,
        });
        assert_eq!(payload["session_id"], "sess_1");
        assert_eq!(payload["status"], "not_loaded");
        assert_eq!(payload["last_status"], "waiting_user_input");
        assert_eq!(payload["loaded"], false);
    }

    #[test]
    fn not_loaded_payload_keeps_thread_status_shape() {
        let payload = thread_not_loaded_payload(&super::ThreadRuntimeCloseEvent {
            session_id: "sess_1".to_string(),
            last_status: ThreadRuntimeStatus::Idle,
        });
        assert_eq!(payload["session_id"], "sess_1");
        assert_eq!(payload["status"], "not_loaded");
        assert_eq!(payload["subscriber_count"], 0);
        assert_eq!(payload["loaded"], false);
    }
}
