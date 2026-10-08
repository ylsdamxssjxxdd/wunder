use crate::config::Config;
use crate::services::agent_abilities::resolve_agent_runtime_tool_names;
use crate::services::default_agent_sync::{DEFAULT_AGENT_ID_ALIAS, PRESET_TEMPLATE_USER_ID};
use crate::services::user_agent_presets::{configured_preset_agents_for_config, PresetAgent};
use crate::storage::{StorageBackend, UserAgentRecord};
use crate::user_store::build_default_agent_record_from_storage;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

#[derive(Clone)]
pub(crate) struct BenchmarkAgentProfile {
    pub(crate) preset_id: String,
    pub(crate) revision: u64,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) system_prompt: String,
    pub(crate) model_name: Option<String>,
    pub(crate) preview_skill: bool,
    pub(crate) sandbox_container_id: i32,
    pub(crate) tool_names: Vec<String>,
    pub(crate) declared_tool_names: Vec<String>,
    pub(crate) declared_skill_names: Vec<String>,
    pub(crate) status: String,
    pub(crate) is_default_agent: bool,
}

impl BenchmarkAgentProfile {
    pub(crate) fn requested_tool_names(&self) -> Vec<String> {
        resolve_agent_runtime_tool_names(
            &self.tool_names,
            &self.declared_tool_names,
            &self.declared_skill_names,
        )
    }

    pub(crate) fn snapshot(&self) -> Value {
        json!({
            "preset_id": self.preset_id,
            "revision": self.revision,
            "name": self.name,
            "description": self.description,
            "system_prompt": self.system_prompt,
            "model_name": self.model_name,
            "preview_skill": self.preview_skill,
            "sandbox_container_id": self.sandbox_container_id,
            "tool_names": self.tool_names,
            "declared_tool_names": self.declared_tool_names,
            "declared_skill_names": self.declared_skill_names,
            "status": self.status,
            "is_default_agent": self.is_default_agent,
        })
    }

    pub(crate) fn list_item(&self) -> Value {
        json!({
            "preset_id": self.preset_id,
            "revision": self.revision,
            "name": self.name,
            "description": self.description,
            "model_name": self.model_name,
            "preview_skill": self.preview_skill,
            "sandbox_container_id": self.sandbox_container_id,
            "tool_count": self.requested_tool_names().len(),
            "status": self.status,
            "is_default_agent": self.is_default_agent,
        })
    }
}

pub(crate) fn list_preset_agents(
    config: &Config,
    storage: &dyn StorageBackend,
) -> Result<Vec<Value>> {
    let mut profiles = Vec::new();
    profiles.push(default_agent_profile(storage)?);
    profiles.extend(
        configured_preset_agents_for_config(config)
            .iter()
            .map(profile_from_preset),
    );
    Ok(profiles
        .iter()
        .map(BenchmarkAgentProfile::list_item)
        .collect())
}

pub(crate) fn resolve_preset_agent(
    config: &Config,
    storage: &dyn StorageBackend,
    requested_preset_id: Option<&str>,
) -> Result<Option<BenchmarkAgentProfile>> {
    let Some(preset_id) = requested_preset_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if preset_id.eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS)
        || preset_id.eq_ignore_ascii_case("default")
    {
        return default_agent_profile(storage).map(Some);
    }
    configured_preset_agents_for_config(config)
        .iter()
        .find(|preset| preset.preset_id == preset_id)
        .map(profile_from_preset)
        .map(Some)
        .ok_or_else(|| anyhow!("preset agent not found"))
}

fn default_agent_profile(storage: &dyn StorageBackend) -> Result<BenchmarkAgentProfile> {
    let record = build_default_agent_record_from_storage(storage, PRESET_TEMPLATE_USER_ID)?;
    Ok(profile_from_record(record, DEFAULT_AGENT_ID_ALIAS, 1, true))
}

fn profile_from_preset(preset: &PresetAgent) -> BenchmarkAgentProfile {
    BenchmarkAgentProfile {
        preset_id: preset.preset_id.clone(),
        revision: preset.revision,
        name: preset.name.clone(),
        description: preset.description.clone(),
        system_prompt: preset.system_prompt.clone(),
        model_name: preset.model_name.clone(),
        preview_skill: preset.preview_skill,
        sandbox_container_id: preset.sandbox_container_id,
        tool_names: preset.tool_names.clone(),
        declared_tool_names: preset.declared_tool_names.clone(),
        declared_skill_names: preset.declared_skill_names.clone(),
        status: preset.status.clone(),
        is_default_agent: false,
    }
}

fn profile_from_record(
    record: UserAgentRecord,
    preset_id: &str,
    revision: u64,
    is_default_agent: bool,
) -> BenchmarkAgentProfile {
    BenchmarkAgentProfile {
        preset_id: preset_id.to_string(),
        revision,
        name: record.name,
        description: record.description,
        system_prompt: record.system_prompt,
        model_name: record.model_name,
        preview_skill: record.preview_skill,
        sandbox_container_id: record.sandbox_container_id,
        tool_names: record.tool_names,
        declared_tool_names: record.declared_tool_names,
        declared_skill_names: record.declared_skill_names,
        status: record.status,
        is_default_agent,
    }
}

#[cfg(test)]
mod tests {
    use super::BenchmarkAgentProfile;

    #[test]
    fn agent_profile_prefers_declared_tools_for_benchmark_execution() {
        let profile = BenchmarkAgentProfile {
            preset_id: "preset_example".to_string(),
            revision: 1,
            name: "Example".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            model_name: None,
            preview_skill: false,
            sandbox_container_id: 1,
            tool_names: vec!["write_file".to_string()],
            declared_tool_names: vec!["read_file".to_string()],
            declared_skill_names: vec!["knowledge_lookup".to_string()],
            status: "active".to_string(),
            is_default_agent: false,
        };

        assert_eq!(
            profile.requested_tool_names(),
            vec!["read_file".to_string(), "knowledge_lookup".to_string()]
        );
    }
}
