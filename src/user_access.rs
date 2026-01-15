use crate::config::Config;
use crate::skills::SkillRegistry;
use crate::state::AppState;
use crate::storage::UserAccountRecord;
use crate::tools::collect_available_tool_names;
use crate::user_tools::{UserToolBindings, UserToolKind};
use std::collections::HashSet;

pub struct UserToolContext {
    pub config: Config,
    pub skills: SkillRegistry,
    pub bindings: UserToolBindings,
    pub whitelist: Option<Vec<String>>,
}

pub async fn build_user_tool_context(state: &AppState, user_id: &str) -> UserToolContext {
    let config = state.config_store.get().await;
    let skills = state.skills.read().await.clone();
    let bindings = state
        .user_tool_manager
        .build_bindings(&config, &skills, user_id);
    let whitelist = state
        .user_store
        .get_user_tool_access(user_id)
        .unwrap_or(None);
    UserToolContext {
        config,
        skills,
        bindings,
        whitelist,
    }
}

pub fn compute_allowed_tool_names(
    user: &UserAccountRecord,
    context: &UserToolContext,
    selected_shared_tools: Option<&[String]>,
) -> HashSet<String> {
    let mut allowed =
        collect_available_tool_names(&context.config, &context.skills, Some(&context.bindings));

    if let Some(whitelist) = context.whitelist.as_ref() {
        let whitelist_set: HashSet<String> = whitelist
            .iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        allowed = allowed
            .intersection(&whitelist_set)
            .cloned()
            .collect::<HashSet<_>>();
    } else {
        let access_level = user.access_level.trim().to_uppercase();
        if access_level == "B" || access_level == "C" {
            let skill_names: HashSet<String> = context
                .skills
                .list_specs()
                .into_iter()
                .map(|spec| spec.name)
                .collect();
            for name in skill_names {
                allowed.remove(&name);
            }
            for (alias, item) in &context.bindings.alias_map {
                if matches!(item.kind, UserToolKind::Skill) {
                    allowed.remove(alias);
                }
            }
        }
        if access_level == "C" {
            for base in &context.config.knowledge.bases {
                if base.enabled {
                    let name = base.name.trim();
                    if !name.is_empty() {
                        allowed.remove(name);
                    }
                }
            }
            for (alias, item) in &context.bindings.alias_map {
                if matches!(item.kind, UserToolKind::Knowledge) {
                    allowed.remove(alias);
                }
            }
        }
    }

    if let Some(selected) = selected_shared_tools {
        let selected_set: HashSet<String> = selected
            .iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        let shared_tools: Vec<String> = context
            .bindings
            .alias_map
            .iter()
            .filter_map(|(alias, item)| {
                if item.owner_id != user.user_id {
                    Some(alias.clone())
                } else {
                    None
                }
            })
            .collect();
        for name in shared_tools {
            if !selected_set.contains(&name) {
                allowed.remove(&name);
            }
        }
    }

    allowed
}
