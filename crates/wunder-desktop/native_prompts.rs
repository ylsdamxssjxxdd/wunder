//! System prompt projection for the desktop shell. The chat greeting reads the
//! prompt a thread would be started with; nothing here mutates packs.

use super::NativeDesktop;
use anyhow::Result;

impl NativeDesktop {
    /// Read the active thread's frozen prompt, or preview its initial prompt.
    pub fn preview_thread_system_prompt(
        &self,
        session_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> Result<String> {
        self.runtime
            .block_on(wunder_server::api::chat::preview_system_prompt_for_user(
                self.state().clone(),
                self.user_id(),
                session_id,
                agent_id,
            ))
    }
}
