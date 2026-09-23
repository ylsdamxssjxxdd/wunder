//! Active child executions, including runs which have not registered with the monitor yet.
use anyhow::{bail, Result};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Default)]
pub(crate) struct ChildRuns {
    entries: Mutex<HashMap<String, Entry>>,
}

#[derive(Debug)]
struct Entry {
    ancestors: Vec<String>,
    token: CancellationToken,
    identity: Arc<()>,
    executing: bool,
}

#[derive(Debug)]
pub(crate) struct ChildRunGuard {
    registry: Arc<ChildRuns>,
    session_id: String,
    pub(crate) token: CancellationToken,
    identity: Arc<()>,
}

impl ChildRuns {
    pub(crate) fn register(
        self: &Arc<Self>,
        session_id: &str,
        parent: &str,
    ) -> Result<ChildRunGuard> {
        let mut entries = self.entries.lock();
        // A child has one active execution. Reuse is allowed only after it settles.
        if entries.get(session_id).is_some_and(|entry| entry.executing) {
            bail!("child session is still running; wait for it to settle before sending again");
        }
        if entries.len() >= 8192 {
            bail!("too many active child runs");
        }
        if entries
            .get(parent)
            .is_some_and(|entry| entry.token.is_cancelled())
        {
            bail!("parent run was interrupted");
        }
        let token = CancellationToken::new();
        let identity = Arc::new(());
        let mut ancestors = entries
            .get(parent)
            .map(|entry| entry.ancestors.clone())
            .unwrap_or_default();
        if ancestors.len() >= 32 {
            bail!("child run nesting exceeds 32 levels");
        }
        ancestors.push(parent.to_string());
        if let Some(previous) = entries.insert(
            session_id.to_string(),
            Entry {
                ancestors,
                token: token.clone(),
                identity: identity.clone(),
                executing: true,
            },
        ) {
            // Reassignment consumes any delayed wake-up from the previous task.
            previous.token.cancel();
        }
        Ok(ChildRunGuard {
            registry: self.clone(),
            session_id: session_id.to_string(),
            token,
            identity,
        })
    }

    pub(crate) fn cancel_tree(&self, root: &str) -> usize {
        let entries = self.entries.lock();
        let mut count = 0;
        for (id, entry) in entries.iter() {
            // Completed intermediate runs must not detach their background descendants.
            if id == root || entry.ancestors.iter().any(|parent| parent == root) {
                count += 1;
                entry.token.cancel();
            }
        }
        count
    }

    pub(crate) fn is_cancelled(&self, session_id: &str) -> bool {
        self.entries
            .lock()
            .get(session_id)
            .is_some_and(|entry| entry.token.is_cancelled())
    }

    pub(crate) fn token(&self, session_id: &str) -> Option<CancellationToken> {
        self.entries
            .lock()
            .get(session_id)
            .map(|entry| entry.token.clone())
    }
}

impl ChildRunGuard {
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }
    pub(crate) fn settle(&self) {
        if let Some(entry) = self.registry.entries.lock().get_mut(&self.session_id) {
            if Arc::ptr_eq(&entry.identity, &self.identity) {
                entry.executing = false;
            }
        }
    }
}

impl Drop for ChildRunGuard {
    fn drop(&mut self) {
        let mut entries = self.registry.entries.lock();
        if entries
            .get(&self.session_id)
            .is_some_and(|entry| Arc::ptr_eq(&entry.identity, &self.identity))
        {
            entries.remove(&self.session_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_reassignment_invalidates_old_wake_without_releasing_new_run() {
        let registry = Arc::new(ChildRuns::default());
        let previous = registry.register("child", "parent").unwrap();
        previous.settle();
        let next = registry.register("child", "parent").unwrap();
        assert!(previous.token.is_cancelled());
        drop(previous);
        assert_eq!(registry.cancel_tree("parent"), 1);
        assert!(next.token.is_cancelled());
    }

    #[test]
    fn child_cancellation_keeps_ancestry_after_intermediate_run_finishes() {
        let registry = Arc::new(ChildRuns::default());
        let child = registry.register("child", "parent").unwrap();
        let nested = registry.register("nested", "child").unwrap();
        drop(child);
        assert_eq!(registry.cancel_tree("parent"), 1);
        assert!(nested.token.is_cancelled());
    }

    #[test]
    fn cancellation_covers_pending_nested_runs_and_allows_later_reuse() {
        let registry = Arc::new(ChildRuns::default());
        let child = registry.register("child", "parent").unwrap();
        let nested = registry.register("nested", "child").unwrap();
        let unrelated = registry.register("other", "other_parent").unwrap();
        assert_eq!(registry.cancel_tree("parent"), 2);
        assert!(child.token.is_cancelled());
        assert!(nested.token.is_cancelled());
        assert!(!unrelated.token.is_cancelled());
        assert!(registry.register("late", "child").is_err());
        assert!(registry.register("child", "parent").is_err());
        drop(nested);
        drop(child);
        let resumed = registry.register("child", "parent").unwrap();
        assert!(!resumed.token.is_cancelled());
        assert_eq!(registry.cancel_tree("parent"), 1);
    }
}
