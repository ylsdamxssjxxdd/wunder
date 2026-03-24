use crate::services::memory_agent_settings::AgentMemorySettingsService;
use crate::state::AppState;
use anyhow::Result;

pub fn disable_auto_extract_for_agents(
    state: &AppState,
    user_id: &str,
    mother_agent_id: &str,
    worker_agent_ids: &[String],
) -> Result<()> {
    let settings = AgentMemorySettingsService::new(state.storage.clone());

    // Keep simulation deterministic: disable asynchronous memory-auto-extract jobs.
    let _ = settings.set_auto_extract_enabled(user_id, None, false)?;
    let _ = settings.set_auto_extract_enabled(user_id, Some(mother_agent_id), false)?;
    for worker_agent_id in worker_agent_ids {
        let cleaned = worker_agent_id.trim();
        if cleaned.is_empty() {
            continue;
        }
        let _ = settings.set_auto_extract_enabled(user_id, Some(cleaned), false)?;
    }
    Ok(())
}
