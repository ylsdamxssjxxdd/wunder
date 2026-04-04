use crate::i18n;
use crate::services::swarm::beeroom::agent_in_hive;
use crate::storage::{StorageBackend, UserAgentAccessRecord, UserAgentRecord};
use crate::user_store::build_default_agent_record_from_storage;
use anyhow::{anyhow, Result};
use std::collections::HashSet;

const SWARM_NOT_FOUND_NAME_LIMIT: usize = 8;

pub(crate) fn resolve_swarm_agent_record(
    storage: &dyn StorageBackend,
    user_id: &str,
    current_agent_id: Option<&str>,
    include_current: bool,
    hive_id: &str,
    requested_agent_id: Option<&str>,
    requested_agent_name: Option<&str>,
) -> Result<Option<UserAgentRecord>> {
    let requested_agent_id = requested_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let requested_agent_name = requested_agent_name
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(agent_id) = requested_agent_id {
        let agent = load_accessible_agent_record(storage, user_id, agent_id)?
            .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
        ensure_agent_usable(&agent, current_agent_id, include_current, hive_id)?;
        return Ok(Some(agent));
    }

    let Some(agent_name) = requested_agent_name else {
        return Ok(None);
    };
    let lookup_key = normalize_swarm_agent_name_lookup_key(agent_name);
    let candidates = collect_swarm_agents_for_hint(
        storage,
        user_id,
        current_agent_id,
        include_current,
        hive_id,
    )?;
    let mut matched = candidates
        .into_iter()
        .filter(|agent| normalize_swarm_agent_name_lookup_key(&agent.name) == lookup_key)
        .collect::<Vec<_>>();
    matched.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    matched.dedup_by(|a, b| a.agent_id == b.agent_id);

    match matched.len() {
        0 => Err(anyhow!(build_agent_name_not_found_error(
            storage,
            user_id,
            current_agent_id,
            hive_id,
            agent_name,
        )?)),
        1 => Ok(matched.into_iter().next()),
        _ => {
            let candidates = matched
                .into_iter()
                .map(|agent| format!("{} ({})", agent.name.trim(), agent.agent_id))
                .collect::<Vec<_>>()
                .join(", ");
            Err(anyhow!(format!(
                "智能体名称“{agent_name}”存在歧义，请改用 agentId。候选：{candidates}"
            )))
        }
    }
}

pub(crate) fn normalize_swarm_agent_name_lookup_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn build_agent_name_not_found_error(
    storage: &dyn StorageBackend,
    user_id: &str,
    current_agent_id: Option<&str>,
    hive_id: &str,
    requested_name: &str,
) -> Result<String> {
    let names = collect_swarm_agents_for_hint(storage, user_id, current_agent_id, false, hive_id)?
        .into_iter()
        .map(|agent| agent.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .take(SWARM_NOT_FOUND_NAME_LIMIT)
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Ok(format!("未找到名为“{requested_name}”的智能体。"));
    }
    Ok(format!(
        "未找到名为“{requested_name}”的智能体。当前可用工蜂名称：{}",
        names.join("、")
    ))
}

fn collect_swarm_agents_for_hint(
    storage: &dyn StorageBackend,
    user_id: &str,
    current_agent_id: Option<&str>,
    include_current: bool,
    hive_id: &str,
) -> Result<Vec<UserAgentRecord>> {
    let access = storage.get_user_agent_access(user_id)?;
    let current_agent_id = current_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut agents = storage.list_user_agents(user_id)?;
    agents.extend(storage.list_shared_user_agents(user_id)?);
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for agent in agents {
        if agent.agent_id.trim().is_empty() {
            continue;
        }
        if !seen.insert(agent.agent_id.clone()) {
            continue;
        }
        if !is_agent_allowed_by_access(user_id, access.as_ref(), &agent) {
            continue;
        }
        if !agent_in_hive(&agent, hive_id) {
            continue;
        }
        if !include_current
            && current_agent_id.is_some_and(|value| value == agent.agent_id.as_str())
        {
            continue;
        }
        output.push(agent);
    }
    output.sort_by(|a, b| {
        b.updated_at
            .partial_cmp(&a.updated_at)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
    Ok(output)
}

fn ensure_agent_usable(
    agent: &UserAgentRecord,
    current_agent_id: Option<&str>,
    include_current: bool,
    hive_id: &str,
) -> Result<()> {
    if !include_current
        && current_agent_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some_and(|value| value == agent.agent_id.as_str())
    {
        return Err(anyhow!(
            "agent_swarm only manages agents other than the current agent"
        ));
    }
    if !agent_in_hive(agent, hive_id) {
        return Err(anyhow!("target is outside current hive"));
    }
    Ok(())
}

fn load_accessible_agent_record(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent_id: &str,
) -> Result<Option<UserAgentRecord>> {
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Ok(None);
    }
    let record = if is_default_agent_alias_value(cleaned) {
        Some(build_default_agent_record_from_storage(storage, user_id)?)
    } else {
        storage.get_user_agent_by_id(cleaned)?
    };
    let Some(record) = record else {
        return Ok(None);
    };
    let access = storage.get_user_agent_access(user_id)?;
    if !is_agent_allowed_by_access(user_id, access.as_ref(), &record) {
        return Ok(None);
    }
    Ok(Some(record))
}

fn is_default_agent_alias_value(raw: &str) -> bool {
    let cleaned = raw.trim();
    cleaned.eq_ignore_ascii_case("__default__") || cleaned.eq_ignore_ascii_case("default")
}

fn is_agent_allowed_by_access(
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

#[cfg(test)]
mod tests {
    use super::normalize_swarm_agent_name_lookup_key;

    #[test]
    fn normalize_swarm_agent_name_lookup_key_collapses_whitespace() {
        assert_eq!(
            normalize_swarm_agent_name_lookup_key("  政策专家\t（副） "),
            "政策专家 （副）"
        );
    }
}
