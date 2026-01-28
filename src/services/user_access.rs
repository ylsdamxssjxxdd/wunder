use crate::config::Config;
use crate::skills::SkillRegistry;
use crate::state::AppState;
use crate::storage::{
    UserAccountRecord, UserAgentAccessRecord, UserAgentRecord, UserToolAccessRecord,
};
use crate::tools::{builtin_aliases, collect_available_tool_names, resolve_tool_name};
use crate::user_tools::{UserToolBindings, UserToolKind};
use std::collections::{HashMap, HashSet};

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
    user: &UserAccountRecord,
    context: &UserToolContext,
) -> HashSet<String> {
    let mut allowed =
        collect_available_tool_names(&context.config, &context.skills, Some(&context.bindings));

    let user_level = normalize_access_level(&user.access_level);
    let access_index = build_tool_access_index(context);
    allowed = allowed
        .into_iter()
        .filter(|name| {
            let tool_level = access_index.resolve_level(name);
            is_level_allowed(&user_level, &tool_level)
        })
        .collect();

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
    let user_level = normalize_access_level(&user.access_level);
    let agent_level = normalize_access_level(&agent.access_level);
    if agent.user_id != user.user_id {
        if !agent.is_shared {
            return false;
        }
        if user_level != agent_level {
            return false;
        }
    }
    if !is_level_allowed(&user_level, &agent_level) {
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

fn normalize_access_level(raw: &str) -> String {
    let level = raw.trim().to_uppercase();
    if level == "B" || level == "C" {
        level
    } else {
        "A".to_string()
    }
}

fn access_level_rank(level: &str) -> i32 {
    match level.trim().to_uppercase().as_str() {
        "C" => 1,
        "B" => 2,
        _ => 3,
    }
}

fn is_level_allowed(user_level: &str, tool_level: &str) -> bool {
    access_level_rank(user_level) >= access_level_rank(tool_level)
}

struct ToolAccessIndex {
    builtin_enabled: HashSet<String>,
    builtin_aliases: HashSet<String>,
    mcp_names: HashSet<String>,
    skill_names: HashSet<String>,
    knowledge_names: HashSet<String>,
    user_skill_names: HashSet<String>,
    user_alias_kinds: HashMap<String, UserToolKind>,
}

impl ToolAccessIndex {
    fn resolve_level(&self, name: &str) -> String {
        if let Some(kind) = self.user_alias_kinds.get(name) {
            return match kind {
                UserToolKind::Skill => "A".to_string(),
                UserToolKind::Knowledge => "B".to_string(),
                UserToolKind::Mcp => "C".to_string(),
            };
        }

        if self.user_skill_names.contains(name) || self.skill_names.contains(name) {
            return "A".to_string();
        }

        if self.knowledge_names.contains(name) {
            return "B".to_string();
        }

        if name.starts_with("a2a@") {
            return "C".to_string();
        }

        if self.mcp_names.contains(name) {
            return "C".to_string();
        }

        let canonical = resolve_tool_name(name);
        if self.builtin_enabled.contains(&canonical) || self.builtin_aliases.contains(name) {
            return "C".to_string();
        }

        "C".to_string()
    }
}

fn build_tool_access_index(context: &UserToolContext) -> ToolAccessIndex {
    let builtin_enabled: HashSet<String> = context
        .config
        .tools
        .builtin
        .enabled
        .iter()
        .map(|item| resolve_tool_name(item))
        .filter(|item| !item.trim().is_empty())
        .collect();
    let builtin_aliases: HashSet<String> = builtin_aliases()
        .into_iter()
        .filter_map(|(alias, canonical)| {
            if builtin_enabled.contains(&canonical) {
                Some(alias)
            } else {
                None
            }
        })
        .collect();
    let mcp_names: HashSet<String> = context
        .config
        .mcp
        .servers
        .iter()
        .filter(|server| server.enabled)
        .flat_map(|server| {
            let allow: HashSet<String> = server.allow_tools.iter().cloned().collect();
            server
                .tool_specs
                .iter()
                .filter(|tool| !tool.name.trim().is_empty())
                .filter(move |tool| allow.is_empty() || allow.contains(&tool.name))
                .map(|tool| format!("{}@{}", server.name, tool.name))
        })
        .collect();
    let skill_names: HashSet<String> = context
        .skills
        .list_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    let knowledge_names: HashSet<String> = context
        .config
        .knowledge
        .bases
        .iter()
        .filter(|base| base.enabled && !base.name.trim().is_empty())
        .map(|base| base.name.trim().to_string())
        .collect();
    let user_skill_names: HashSet<String> = context
        .bindings
        .skill_specs
        .iter()
        .map(|spec| spec.name.clone())
        .collect();
    let user_alias_kinds: HashMap<String, UserToolKind> = context
        .bindings
        .alias_map
        .iter()
        .map(|(alias, info)| (alias.clone(), info.kind.clone()))
        .collect();
    ToolAccessIndex {
        builtin_enabled,
        builtin_aliases,
        mcp_names,
        skill_names,
        knowledge_names,
        user_skill_names,
        user_alias_kinds,
    }
}
