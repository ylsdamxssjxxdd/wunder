//! Synthetic thread generation through the production canonical storage path.
//!
//! Synthetic service threads are persisted in PostgreSQL, matching live users.

use super::PostgresStorage;
use crate::storage::stress_model::{StressThreadSpec, StressThreadStats};
use anyhow::Result;

impl PostgresStorage {
    /// Synthetic runs exercise the production PostgreSQL admission and
    /// canonical writer. No local SQLite or JSONL history is opened.
    pub fn generate_stress_thread(
        &self,
        user_id: &str,
        spec: &StressThreadSpec,
        mut progress: impl FnMut(i64, i64) + Send,
    ) -> Result<StressThreadStats> {
        let store = self.service_threads_ref()?.clone();
        self.block_on(
            store.generate_stress_thread(user_id, spec, move |done, items| progress(done, items)),
        )?
    }
}
