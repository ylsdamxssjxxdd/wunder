//! Shared agent execution settings for interactive and scheduled requests.
use crate::services::agent_abilities::resolve_agent_runtime_tool_names;
use crate::services::llm::is_llm_model;
use crate::tools::expand_browser_group_selection;
use std::collections::HashSet;
const TOOL_OVERRIDE_NONE: &str = "__no_tools__";

pub(crate) fn resolve_chat_model_name(
    config: &crate::config::Config,
    agent_record: Option<&crate::storage::UserAgentRecord>,
) -> Option<String> {
    if let Some(name) =
        agent_record.and_then(|record| normalize_optional_model_name(record.model_name.as_deref()))
    {
        if config
            .llm
            .models
            .get(&name)
            .is_some_and(crate::services::llm::is_llm_model)
        {
            return Some(name);
        }
    }
    resolve_default_model_key(config)
}

fn resolve_default_model_key(config: &crate::config::Config) -> Option<String> {
    let default_key = config.llm.default.trim();
    if !default_key.is_empty() && config.llm.models.get(default_key).is_some_and(is_llm_model) {
        return Some(default_key.to_string());
    }
    for (key, cfg) in config.llm.models.iter() {
        if !is_llm_model(cfg) {
            continue;
        }
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub(crate) fn normalize_optional_model_name(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn finalize_tool_names(mut allowed: HashSet<String>) -> Vec<String> {
    if allowed.is_empty() {
        return vec!["__no_tools__".to_string()];
    }
    let mut list = allowed.drain().collect::<Vec<_>>();
    list.sort();
    list
}

pub(crate) fn resolve_session_tool_overrides(
    record: &crate::storage::ChatSessionRecord,
    frozen_tool_overrides: Option<&[String]>,
    agent: Option<&crate::storage::UserAgentRecord>,
) -> Vec<String> {
    if !record.tool_overrides.is_empty() {
        normalize_tool_overrides(record.tool_overrides.clone())
    } else if let Some(snapshot) = frozen_tool_overrides {
        normalize_tool_overrides(snapshot.to_vec())
    } else {
        resolve_agent_tool_defaults(agent)
    }
}

pub(crate) fn resolve_agent_tool_defaults(
    agent: Option<&crate::storage::UserAgentRecord>,
) -> Vec<String> {
    let Some(record) = agent else {
        return Vec::new();
    };
    resolve_agent_runtime_tool_names(
        &record.tool_names,
        &record.declared_tool_names,
        &record.declared_skill_names,
    )
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

pub(crate) fn apply_tool_overrides(
    allowed: HashSet<String>,
    overrides: &[String],
    agent_defaults: &[String],
) -> HashSet<String> {
    if overrides.is_empty() {
        return allowed;
    }
    if overrides.iter().any(|name| name == TOOL_OVERRIDE_NONE) {
        return HashSet::new();
    }
    let scoped_defaults: HashSet<String> = agent_defaults
        .iter()
        .map(String::as_str)
        .filter_map(|name| resolve_override_name_with_allowed(name, &allowed))
        .collect();
    let mut filtered = HashSet::new();
    for raw in overrides {
        if let Some(mapped) = resolve_override_name_with_allowed(raw, &allowed) {
            if !scoped_defaults.is_empty() && !scoped_defaults.contains(&mapped) {
                continue;
            }
            filtered.insert(mapped);
        }
    }
    expand_browser_group_selection(filtered, &allowed)
}

pub(crate) fn resolve_override_name_with_allowed(
    raw: &str,
    allowed: &HashSet<String>,
) -> Option<String> {
    let allowed_canonical: HashSet<String> = allowed
        .iter()
        .map(|name| crate::tools::resolve_tool_name(name.trim()))
        .filter(|name| !name.is_empty())
        .collect();
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return None;
    }
    if allowed.contains(cleaned) {
        return Some(cleaned.to_string());
    }
    let canonical = crate::tools::resolve_tool_name(cleaned);
    if canonical != cleaned && allowed_canonical.contains(&canonical) {
        return Some(canonical);
    }
    for (index, _) in cleaned.match_indices('@') {
        let suffix = cleaned[index + 1..].trim();
        if !suffix.is_empty() && allowed.contains(suffix) {
            return Some(suffix.to_string());
        }
        let canonical_suffix = crate::tools::resolve_tool_name(suffix);
        if !suffix.is_empty() && allowed_canonical.contains(&canonical_suffix) {
            return Some(canonical_suffix);
        }
    }
    None
}
