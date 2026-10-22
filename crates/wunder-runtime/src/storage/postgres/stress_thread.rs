//! Synthetic thread generation through the production canonical storage path.
//!
//! PostgreSQL remains the control plane. Complete thread history is always
//! recorded in the configured local rollout store, so stress data exercises the
//! same bounded JSONL and projection path as a real thread.

use super::PostgresStorage;
use crate::storage::stress_model::{StressThreadSpec, StressThreadStats};
use anyhow::Result;

impl PostgresStorage {
    /// Synthetic runs exercise the production canonical writer and projector.
    /// PostgreSQL intentionally receives no `thread_*`, chat, or diagnostic
    /// history rows from this path.
    pub fn generate_stress_thread(
        &self,
        user_id: &str,
        spec: &StressThreadSpec,
        mut progress: impl FnMut(i64, i64) + Send,
    ) -> Result<StressThreadStats> {
        let store = self.canonical_threads()?.clone();
        self.block_on(
            store.generate_stress_thread(user_id, spec, move |done, items| progress(done, items)),
        )?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_uses_local_store_without_connecting_to_postgres() {
        let root = tempfile::tempdir().unwrap();
        // No server is needed: production stress history is entirely local.
        let storage = PostgresStorage::new_with_data_root(
            "host=invalid.invalid user=sample dbname=sample".into(),
            1,
            1,
            root.path().to_path_buf(),
        )
        .unwrap();
        assert!(!root.path().join("state_1.sqlite").exists());
        assert!(!root.path().join("logs_1.sqlite").exists());
        let stats = storage
            .generate_stress_thread(
                "sample-user",
                &StressThreadSpec {
                    session_id: "sample-thread".into(),
                    title: "Sample thread".into(),
                    workspace_id: None,
                    user_rounds: 2,
                    model_rounds_per_turn: 2,
                },
                |_, _| {},
            )
            .unwrap();
        assert_eq!(stats.user_turns, 2);
        assert!(stats.items > 2);
        assert!(root.path().join("state_1.sqlite").exists());
        assert!(root.path().join("thread_history_1.sqlite").exists());
        assert!(!root.path().join("logs_1.sqlite").exists());
        let rollout_count = walkdir::WalkDir::new(root.path().join("sessions"))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "jsonl")
            })
            .count();
        assert_eq!(rollout_count, 1);
    }
}
