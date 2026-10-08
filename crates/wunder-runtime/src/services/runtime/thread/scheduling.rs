use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
pub struct CooperativeScheduler {
    entries: Mutex<HashMap<String, Entry>>,
}

struct Entry {
    is_admin: bool,
    pause_requested: bool,
    suspended: bool,
    resume_granted: bool,
    order: std::time::Instant,
}

pub struct SchedulingGuard {
    scheduler: Arc<CooperativeScheduler>,
    session_id: String,
}

impl Drop for SchedulingGuard {
    fn drop(&mut self) {
        self.scheduler.entries.lock().remove(&self.session_id);
    }
}

impl CooperativeScheduler {
    pub fn register(self: &Arc<Self>, session_id: &str, is_admin: bool) -> SchedulingGuard {
        self.entries.lock().insert(
            session_id.to_string(),
            Entry {
                is_admin,
                pause_requested: false,
                suspended: false,
                resume_granted: false,
                order: std::time::Instant::now(),
            },
        );
        SchedulingGuard {
            scheduler: self.clone(),
            session_id: session_id.to_string(),
        }
    }

    pub fn request_slot(&self, target: &str) -> Option<String> {
        let mut entries = self.entries.lock();
        // Only one outstanding handoff is needed; repeated admin clicks are idempotent.
        if entries.values().any(|entry| entry.pause_requested)
            || entries.values().filter(|entry| entry.suspended).count() >= 32
        {
            return None;
        }
        let candidate = entries
            .iter()
            .filter(|(id, entry)| {
                id.as_str() != target
                    && !entry.is_admin
                    && !entry.suspended
                    && !entry.resume_granted
            })
            .min_by_key(|(_, entry)| entry.order)
            .map(|(id, _)| id.clone())?;
        entries.get_mut(&candidate)?.pause_requested = true;
        Some(candidate)
    }

    pub fn pause_requested(&self, session_id: &str) -> bool {
        self.entries
            .lock()
            .get(session_id)
            .is_some_and(|entry| entry.pause_requested)
    }

    pub fn state(&self, session_id: &str) -> Option<&'static str> {
        self.entries.lock().get(session_id).and_then(|entry| {
            if entry.suspended {
                Some("suspended")
            } else if entry.pause_requested {
                Some("pausing")
            } else {
                None
            }
        })
    }

    pub fn mark_suspended(&self, session_id: &str) {
        if let Some(entry) = self.entries.lock().get_mut(session_id) {
            entry.pause_requested = false;
            entry.suspended = true;
            entry.order = std::time::Instant::now();
        }
    }

    pub fn suspended_count(&self) -> usize {
        self.entries
            .lock()
            .values()
            .filter(|entry| entry.suspended && !entry.resume_granted)
            .count()
    }

    pub fn grant_resume(&self) -> Option<String> {
        let mut entries = self.entries.lock();
        let candidate = entries
            .iter()
            .filter(|(_, entry)| entry.suspended && !entry.resume_granted)
            .min_by_key(|(_, entry)| entry.order)
            .map(|(id, _)| id.clone())?;
        entries.get_mut(&candidate)?.resume_granted = true;
        Some(candidate)
    }

    pub fn resume_granted(&self, session_id: &str) -> bool {
        self.entries
            .lock()
            .get(session_id)
            .is_some_and(|entry| entry.resume_granted)
    }

    pub fn mark_resumed(&self, session_id: &str) {
        if let Some(entry) = self.entries.lock().get_mut(session_id) {
            entry.suspended = false;
            entry.resume_granted = false;
        }
    }

    /// Remove a handoff entry when the user explicitly cancels the session.
    /// This wakes any suspended future so cancellation remains terminal.
    pub fn cancel(&self, session_id: &str) {
        self.entries.lock().remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_preserves_owner_and_reserves_resume_capacity() {
        let scheduler = Arc::new(CooperativeScheduler::default());
        let owner = scheduler.register("session-a", false);
        let admin = scheduler.register("session-b", true);
        assert_eq!(
            scheduler.request_slot("session-b"),
            Some("session-a".into())
        );
        assert_eq!(scheduler.request_slot("session-b"), None);
        scheduler.mark_suspended("session-a");
        assert_eq!(scheduler.suspended_count(), 1);
        assert_eq!(scheduler.grant_resume(), Some("session-a".into()));
        assert_eq!(scheduler.suspended_count(), 0);
        assert!(scheduler.resume_granted("session-a"));
        scheduler.mark_resumed("session-a");
        drop(owner);
        assert_eq!(scheduler.request_slot("session-c"), None);
        drop(admin);
    }
}
