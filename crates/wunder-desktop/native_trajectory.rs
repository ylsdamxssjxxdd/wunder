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
}
