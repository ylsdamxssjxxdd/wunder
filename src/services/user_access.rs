use crate::config::Config;
use crate::skills::SkillRegistry;
use crate::state::AppState;
use crate::storage::{
    UserAccountRecord, UserAgentAccessRecord, UserAgentRecord, UserToolAccessRecord,
};
use crate::tools::{collect_available_tool_names, resolve_tool_name};
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
        if let Some(allowed_tools) = access
            .allowed_tools
            .as_ref()
            .filter(|items| !items.is_empty())
        {
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
    }

    if context
        .config
        .server
        .mode
        .trim()
        .eq_ignore_ascii_case("desktop")
    {
        let has_plan_tool = allowed
            .iter()
            .any(|name| resolve_tool_name(name) == "计划面板");
        if !has_plan_tool {
            allowed.insert("计划面板".to_string());
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
    if agent.user_id != user.user_id && !agent.is_shared {
        return false;
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

#[cfg(test)]
mod tests {
    use super::{compute_allowed_tool_names, UserToolContext};
    use crate::config::Config;
    use crate::skills::SkillRegistry;
    use crate::storage::{UserAccountRecord, UserToolAccessRecord};
    use crate::user_tools::UserToolBindings;

    fn sample_user() -> UserAccountRecord {
        UserAccountRecord {
            user_id: "admin".to_string(),
            username: "admin".to_string(),
            email: None,
            password_hash: String::new(),
            roles: vec!["admin".to_string()],
            status: "active".to_string(),
            access_level: "A".to_string(),
            unit_id: None,
            token_balance: 0,
            token_granted_total: 0,
            token_used_total: 0,
            last_token_grant_date: None,
            experience_total: 0,
            is_demo: false,
            created_at: 0.0,
            updated_at: 0.0,
            last_login_at: None,
        }
    }

    #[test]
    fn empty_tool_access_whitelist_does_not_block_everything() {
        let mut config = Config::default();
        config.server.mode = "server".to_string();
        config.tools.builtin.enabled = vec!["read_file".to_string()];
        let context = UserToolContext {
            config,
            skills: SkillRegistry::default(),
            bindings: UserToolBindings::default(),
            tool_access: Some(UserToolAccessRecord {
                user_id: "admin".to_string(),
                allowed_tools: Some(Vec::new()),
                updated_at: 0.0,
            }),
        };

        let allowed = compute_allowed_tool_names(&sample_user(), &context);
        assert!(allowed.contains("读取文件") || allowed.contains("read_file"));
    }
}
