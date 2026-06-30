use super::{collect_available_tool_names, ToolContext, TOOL_OVERRIDE_NONE};
use crate::config::Config;
use crate::i18n;
use crate::services::agent_abilities::resolve_agent_runtime_tool_names;
use crate::skills::SkillRegistry;
use crate::storage::{ChatSessionRecord, StorageBackend, UserAgentAccessRecord, UserAgentRecord};
use crate::tools::resolve_tool_name;
use crate::user_store::build_default_agent_record_from_storage;
use anyhow::{anyhow, Result};
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(crate) enum ChildSessionToolMode {
    InheritParentSession,
    UseTargetAgentDefaults,
}

pub(crate) fn collect_user_allowed_tools(
    context: &ToolContext<'_>,
    user_id: &str,
) -> Result<HashSet<String>> {
    let mut allowed =
        collect_available_tool_names(context.config, context.skills, context.user_tool_bindings);
    let access = context.storage.get_user_tool_access(user_id)?;
    if let Some(access) = access {
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
    Ok(allowed)
}

pub(crate) fn normalize_tool_overrides(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let mut has_none = false;
    for raw in values {
        let name = raw.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        if name == TOOL_OVERRIDE_NONE {
            has_none = true;
        }
        seen.insert(name.clone());
        output.push(name);
    }
    if has_none {
        vec![TOOL_OVERRIDE_NONE.to_string()]
    } else {
        output
    }
}

pub(crate) fn resolve_session_tool_overrides(
    record: &ChatSessionRecord,
    frozen_tool_overrides: Option<&[String]>,
    agent: Option<&UserAgentRecord>,
) -> Vec<String> {
    if !record.tool_overrides.is_empty() {
        normalize_tool_overrides(record.tool_overrides.clone())
    } else if let Some(snapshot) = frozen_tool_overrides {
        normalize_tool_overrides(snapshot.to_vec())
    } else {
        resolve_agent_tool_defaults(agent)
    }
}

pub(crate) fn resolve_agent_tool_defaults(agent: Option<&UserAgentRecord>) -> Vec<String> {
    let Some(record) = agent else {
        return Vec::new();
    };
    resolve_agent_runtime_tool_names(
        &record.tool_names,
        &record.declared_tool_names,
        &record.declared_skill_names,
    )
}

pub(crate) fn resolve_child_session_tool_names(
    mode: ChildSessionToolMode,
    parent_tool_names: &[String],
    child_agent: Option<&UserAgentRecord>,
) -> Vec<String> {
    match mode {
        ChildSessionToolMode::InheritParentSession => parent_tool_names.to_vec(),
        ChildSessionToolMode::UseTargetAgentDefaults => {
            let defaults = resolve_agent_tool_defaults(child_agent);
            if defaults.is_empty() {
                parent_tool_names.to_vec()
            } else {
                defaults
            }
        }
    }
}

pub(crate) fn apply_tool_overrides(
    allowed: HashSet<String>,
    overrides: &[String],
    config: &Config,
    skills: &SkillRegistry,
) -> HashSet<String> {
    if overrides.is_empty() {
        return allowed;
    }
    if overrides.iter().any(|name| name == TOOL_OVERRIDE_NONE) {
        return HashSet::new();
    }
    let mut filtered = HashSet::new();
    for raw in overrides {
        if let Some(mapped) = resolve_override_name_with_allowed(raw, &allowed) {
            filtered.insert(mapped);
        }
    }
    if config.server.mode.trim().eq_ignore_ascii_case("desktop") {
        let skill_names: HashSet<String> = skills
            .list_specs()
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        for name in &allowed {
            if skill_names.contains(name) {
                filtered.insert(name.clone());
            }
        }
    }
    filtered
}

pub(crate) fn resolve_override_name_with_allowed(
    raw: &str,
    allowed: &HashSet<String>,
) -> Option<String> {
    let allowed_canonical: HashSet<String> = allowed
        .iter()
        .map(|name| resolve_tool_name(name.trim()))
        .filter(|name| !name.is_empty())
        .collect();
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return None;
    }
    if allowed.contains(cleaned) {
        return Some(cleaned.to_string());
    }
    let canonical = resolve_tool_name(cleaned);
    if canonical != cleaned && allowed_canonical.contains(&canonical) {
        return Some(canonical);
    }
    for (index, _) in cleaned.match_indices('@') {
        let suffix = cleaned[index + 1..].trim();
        if !suffix.is_empty() && allowed.contains(suffix) {
            return Some(suffix.to_string());
        }
        let canonical_suffix = resolve_tool_name(suffix);
        if !suffix.is_empty() && allowed_canonical.contains(&canonical_suffix) {
            return Some(canonical_suffix);
        }
    }
    None
}

pub(crate) fn finalize_tool_names(mut allowed: HashSet<String>) -> Vec<String> {
    if allowed.is_empty() {
        return vec![TOOL_OVERRIDE_NONE.to_string()];
    }
    let mut list = allowed.drain().collect::<Vec<_>>();
    list.sort();
    list
}

pub(crate) fn build_effective_tool_names(
    context: &ToolContext<'_>,
    user_id: &str,
    record: &ChatSessionRecord,
    agent: Option<&UserAgentRecord>,
) -> Result<Vec<String>> {
    let allowed = collect_user_allowed_tools(context, user_id)?;
    let frozen_tool_overrides = context
        .workspace
        .load_session_frozen_tool_overrides(user_id, &record.session_id);
    let overrides = resolve_session_tool_overrides(record, frozen_tool_overrides.as_deref(), agent);
    let allowed = apply_tool_overrides(allowed, &overrides, context.config, context.skills);
    Ok(finalize_tool_names(allowed))
}

pub(crate) fn is_agent_allowed_by_access(
    user_id: &str,
    access: Option<&UserAgentAccessRecord>,
    agent: &UserAgentRecord,
) -> bool {
    if agent.user_id != user_id && !agent.is_shared {
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

pub(crate) fn load_agent_record(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent_id: Option<&str>,
    allow_missing: bool,
) -> Result<Option<UserAgentRecord>> {
    let Some(agent_id) = agent_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let record = if is_default_agent_alias_value(agent_id) {
        Some(build_default_agent_record_from_storage(storage, user_id)?)
    } else {
        storage.get_user_agent_by_id(agent_id)?
    };
    let Some(record) = record else {
        if allow_missing {
            return Ok(None);
        }
        return Err(anyhow!(i18n::t("error.agent_not_found")));
    };
    let access = storage.get_user_agent_access(user_id)?;
    if !is_agent_allowed_by_access(user_id, access.as_ref(), &record) {
        if allow_missing {
            return Ok(None);
        }
        return Err(anyhow!(i18n::t("error.agent_not_found")));
    }
    Ok(Some(record))
}

pub(crate) fn is_default_agent_alias_value(raw: &str) -> bool {
    let cleaned = raw.trim();
    cleaned.eq_ignore_ascii_case("__default__") || cleaned.eq_ignore_ascii_case("default")
}
