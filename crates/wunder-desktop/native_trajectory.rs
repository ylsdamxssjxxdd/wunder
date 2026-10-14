//! Thread trajectory data access for the desktop shell. The trajectory page
//! reads the same storage snapshot the web shell's snapshot endpoint serves;
//! no separate pagination or export path is kept for it.
use super::NativeDesktop;
use anyhow::Result;
use serde_json::Value;

impl NativeDesktop {
    /// Full user-visible snapshot of one thread: turns, user-visible items and
    /// text blocks, read directly from storage in one bounded query set.
    pub fn thread_snapshot(&self, session: &str) -> Result<Value> {
        self.state()
            .storage
            .thread_snapshot(self.user_id(), session)
    }

    /// Bounded presentation trajectory used by the native shell.  The web
    /// shell and the desktop shell now read the same durable projection: the
    /// rows are keyset paged while `tiles` contains the full, bounded overview
    /// pyramid.  This keeps a long thread from being reconstructed by replaying
    /// its raw event log on the UI thread.
    pub fn thread_trajectory_page(
        &self,
        session: &str,
        before: Option<i64>,
        limit: usize,
    ) -> Result<Value> {
        let page = self.state().storage.thread_view_trajectory_page(
            self.user_id(),
            session,
            before,
            None,
            limit,
        )?;
        Ok(serde_json::to_value(page)?)
    }
}
