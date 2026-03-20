use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ActiveTurnSnapshot {
    pub(super) session_id: String,
    pub(super) turn_id: String,
    pub(super) pending_approval_ids: Vec<String>,
    pub(super) waiting_for_user_input: bool,
}

#[derive(Default)]
pub(super) struct ActiveTurnRegistry {
    inner: Mutex<HashMap<String, ActiveTurnEntry>>,
}

#[derive(Clone, Debug)]
struct ActiveTurnEntry {
    turn_id: String,
    pending_approval_ids: HashSet<String>,
    waiting_for_user_input: bool,
}

impl ActiveTurnRegistry {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn begin_turn(&self, session_id: &str) -> ActiveTurnSnapshot {
        let cleaned_session = session_id.trim();
        let turn_id = format!("turn_{}", Uuid::new_v4().simple());
        let entry = ActiveTurnEntry {
            turn_id: turn_id.clone(),
            pending_approval_ids: HashSet::new(),
            waiting_for_user_input: false,
        };
        let snapshot = ActiveTurnSnapshot {
            session_id: cleaned_session.to_string(),
            turn_id,
            pending_approval_ids: Vec::new(),
            waiting_for_user_input: false,
        };
        self.inner
            .lock()
            .insert(cleaned_session.to_string(), entry);
        snapshot
    }

    pub(super) fn add_pending_approval(
        &self,
        session_id: &str,
        turn_id: &str,
        approval_id: &str,
    ) -> Option<ActiveTurnSnapshot> {
        self.update_turn(session_id, turn_id, |entry| {
            entry
                .pending_approval_ids
                .insert(approval_id.trim().to_string());
        })
    }

    pub(super) fn resolve_pending_approval(
        &self,
        session_id: &str,
        turn_id: &str,
        approval_id: &str,
    ) -> Option<ActiveTurnSnapshot> {
        self.update_turn(session_id, turn_id, |entry| {
            entry.pending_approval_ids.remove(approval_id.trim());
        })
    }

    pub(super) fn mark_waiting_user_input(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Option<ActiveTurnSnapshot> {
        self.update_turn(session_id, turn_id, |entry| {
            entry.waiting_for_user_input = true;
        })
    }

    pub(super) fn finish_turn(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Option<ActiveTurnSnapshot> {
        let cleaned_session = session_id.trim();
        let cleaned_turn = turn_id.trim();
        if cleaned_session.is_empty() || cleaned_turn.is_empty() {
            return None;
        }
        let mut guard = self.inner.lock();
        let entry = guard.get(cleaned_session)?;
        if entry.turn_id != cleaned_turn {
            return None;
        }
        let entry = guard.remove(cleaned_session)?;
        Some(snapshot_from_entry(cleaned_session, entry))
    }

    fn update_turn<F>(
        &self,
        session_id: &str,
        turn_id: &str,
        update: F,
    ) -> Option<ActiveTurnSnapshot>
    where
        F: FnOnce(&mut ActiveTurnEntry),
    {
        let cleaned_session = session_id.trim();
        let cleaned_turn = turn_id.trim();
        if cleaned_session.is_empty() || cleaned_turn.is_empty() {
            return None;
        }
        let mut guard = self.inner.lock();
        let entry = guard.get_mut(cleaned_session)?;
        if entry.turn_id != cleaned_turn {
            return None;
        }
        update(entry);
        Some(snapshot_from_entry(cleaned_session, entry.clone()))
    }
}

fn snapshot_from_entry(session_id: &str, entry: ActiveTurnEntry) -> ActiveTurnSnapshot {
    let mut pending_approval_ids = entry.pending_approval_ids.into_iter().collect::<Vec<_>>();
    pending_approval_ids.sort();
    ActiveTurnSnapshot {
        session_id: session_id.to_string(),
        turn_id: entry.turn_id,
        pending_approval_ids,
        waiting_for_user_input: entry.waiting_for_user_input,
    }
}

#[cfg(test)]
mod tests {
    use super::ActiveTurnRegistry;

    #[test]
    fn tracks_pending_approvals_within_turn() {
        let registry = ActiveTurnRegistry::new();
        let turn = registry.begin_turn("sess_1");

        let snapshot = registry
            .add_pending_approval("sess_1", &turn.turn_id, "approval_1")
            .expect("turn snapshot");
        assert_eq!(snapshot.pending_approval_ids, vec!["approval_1".to_string()]);

        let snapshot = registry
            .resolve_pending_approval("sess_1", &turn.turn_id, "approval_1")
            .expect("turn snapshot");
        assert!(snapshot.pending_approval_ids.is_empty());
    }

    #[test]
    fn marks_waiting_user_input_before_finish() {
        let registry = ActiveTurnRegistry::new();
        let turn = registry.begin_turn("sess_1");

        let snapshot = registry
            .mark_waiting_user_input("sess_1", &turn.turn_id)
            .expect("waiting snapshot");
        assert!(snapshot.waiting_for_user_input);

        let finished = registry
            .finish_turn("sess_1", &turn.turn_id)
            .expect("finished snapshot");
        assert!(finished.waiting_for_user_input);
    }

    #[test]
    fn ignores_updates_for_stale_turn_ids() {
        let registry = ActiveTurnRegistry::new();
        let turn = registry.begin_turn("sess_1");
        let next_turn = registry.begin_turn("sess_1");

        assert!(registry
            .add_pending_approval("sess_1", &turn.turn_id, "approval_1")
            .is_none());
        assert!(registry
            .add_pending_approval("sess_1", &next_turn.turn_id, "approval_2")
            .is_some());
    }
}
