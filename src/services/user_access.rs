use crate::config::Config;
use crate::skills::SkillRegistry;
use crate::state::AppState;
use crate::storage::{
    UserAccountRecord, UserAgentAccessRecord, UserAgentRecord, UserToolAccessRecord,
};
use crate::tools::collect_available_tool_names;
use crate::user_tools::UserToolBindings;
use std::collections::HashSet;

pub struct UserToolContext {
    pub config: Config,
    pub skills: SkillRegistry,
    pub bindings: UserToolBindings,
    pub tool_access: Option<UserToolAccessRecord>,
}

pub async fn build_user_tool_context(state: &AppState, user_id: &str) -> UserToolContext {
    let config = state.config_store.get().await;
    let skills = state.skills.read().await.clone();
    let bindings = state
        .user_tool_manager
        .build_bindings(&config, &skills, user_id);
    let tool_access = state
        .user_store
        .get_user_tool_access(user_id)
        .unwrap_or(None);
    UserToolContext {
        config,
        skills,
        bindings,
        tool_access,
    }
}

pub async fn build_user_tool_context_for_catalog(
    state: &AppState,
    user_id: &str,
) -> UserToolContext {
    let config = state.config_store.get().await;
    let skills = state.skills.read().await.clone();
    let bindings = state
        .user_tool_manager
        .build_bindings_for_catalog(&config, &skills, user_id);
    let tool_access = state
        .user_store
        .get_user_tool_access(user_id)
        .unwrap_or(None);
    UserToolContext {
        config,
        skills,
        bindings,
        tool_access,
    }
}

pub fn compute_allowed_tool_names(
    _user: &UserAccountRecord,
    context: &UserToolContext,
) -> HashSet<String> {
    let mut allowed =
        collect_available_tool_names(&context.config, &context.skills, Some(&context.bindings));

    if let Some(access) = context.tool_access.as_ref() {
        if let Some(allowed_tools) = access.allowed_tools.as_ref() {
            let allowed_set: HashSet<String> = allowed_tools
                .iter()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect();
            allowed = allowed
                .intersection(&allowed_set)
                .cloned()
                .collect::<HashSet<_>>();
        }
        if !access.blocked_tools.is_empty() {
            let blocked_set: HashSet<String> = access
                .blocked_tools
                .iter()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect();
            for name in blocked_set {
                allowed.remove(&name);
            }
        }
    }

    allowed
}

pub fn filter_user_agents_by_access(
    user: &UserAccountRecord,
    access: Option<&UserAgentAccessRecord>,
    agents: Vec<UserAgentRecord>,
) -> Vec<UserAgentRecord> {
    agents
        .into_iter()
        .filter(|agent| is_agent_allowed(user, access, agent))
        .collect()
}

pub fn is_agent_allowed(
    user: &UserAccountRecord,
    access: Option<&UserAgentAccessRecord>,
    agent: &UserAgentRecord,
) -> bool {
    if agent.user_id != user.user_id {
        if !agent.is_shared {
            return false;
        }
    }
    if let Some(access) = access {
        if !access.blocked_agent_ids.is_empty()
            && access
                .blocked_agent_ids
                .iter()
                .any(|id| id == &agent.agent_id)
        {
            return false;
        }
        if let Some(allowed) = access.allowed_agent_ids.as_ref() {
            return allowed.iter().any(|id| id == &agent.agent_id);
        }
    }
    true
}
