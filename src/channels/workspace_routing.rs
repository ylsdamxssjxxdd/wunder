use crate::storage::DEFAULT_SANDBOX_CONTAINER_ID;
use crate::user_store::UserStore;
use crate::workspace::WorkspaceManager;

pub fn resolve_channel_workspace_id(
    workspace: &WorkspaceManager,
    user_store: &UserStore,
    user_id: &str,
    agent_id: Option<&str>,
) -> String {
    let cleaned_agent = agent_id.map(str::trim).filter(|value| !value.is_empty());
    if cleaned_agent.is_none() || is_default_agent_alias(cleaned_agent) {
        return workspace.scoped_user_id_by_container(user_id, DEFAULT_SANDBOX_CONTAINER_ID);
    }
    if let Some(container_id) = user_store.resolve_agent_sandbox_container_id(cleaned_agent) {
        return workspace.scoped_user_id_by_container(user_id, container_id);
    }
    workspace.scoped_user_id(user_id, cleaned_agent)
}

fn is_default_agent_alias(agent_id: Option<&str>) -> bool {
    let Some(cleaned) = agent_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    cleaned.eq_ignore_ascii_case("__default__") || cleaned.eq_ignore_ascii_case("default")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_alias_detection_matches_orchestrator_rules() {
        assert!(is_default_agent_alias(Some("default")));
        assert!(is_default_agent_alias(Some("__default__")));
        assert!(is_default_agent_alias(Some(" DEFAULT ")));
        assert!(!is_default_agent_alias(None));
        assert!(!is_default_agent_alias(Some("")));
        assert!(!is_default_agent_alias(Some("agent_1")));
    }
}
