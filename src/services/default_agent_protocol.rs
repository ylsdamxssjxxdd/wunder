use crate::schemas::AbilityDescriptor;
use crate::storage::{UserAgentRecord, DEFAULT_HIVE_ID};
use serde::{Deserialize, Serialize};

pub const DEFAULT_AGENT_META_PREFIX: &str = "default_agent:";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DefaultAgentConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub preview_skill: bool,
    #[serde(default)]
    pub ability_items: Vec<AbilityDescriptor>,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub declared_tool_names: Vec<String>,
    #[serde(default)]
    pub declared_skill_names: Vec<String>,
    #[serde(default)]
    pub preset_questions: Vec<String>,
    #[serde(default)]
    pub approval_mode: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub sandbox_container_id: i32,
    #[serde(default)]
    pub silent: bool,
    #[serde(default)]
    pub prefer_mother: bool,
    #[serde(default)]
    pub created_at: f64,
    #[serde(default)]
    pub updated_at: f64,
}

pub fn default_agent_meta_key(user_id: &str) -> String {
    format!("{DEFAULT_AGENT_META_PREFIX}{}", user_id.trim())
}

pub fn default_agent_config_from_record(record: &UserAgentRecord) -> DefaultAgentConfig {
    DefaultAgentConfig {
        name: record.name.clone(),
        description: record.description.clone(),
        system_prompt: record.system_prompt.clone(),
        preview_skill: record.preview_skill,
        ability_items: record.ability_items.clone(),
        tool_names: record.tool_names.clone(),
        declared_tool_names: record.declared_tool_names.clone(),
        declared_skill_names: record.declared_skill_names.clone(),
        preset_questions: record.preset_questions.clone(),
        approval_mode: record.approval_mode.clone(),
        status: record.status.clone(),
        icon: record.icon.clone(),
        sandbox_container_id: record.sandbox_container_id,
        silent: record.silent,
        prefer_mother: record.prefer_mother,
        created_at: record.created_at,
        updated_at: record.updated_at,
    }
}

pub fn record_from_default_agent_config(
    agent_id: &str,
    user_id: &str,
    access_level: &str,
    config: &DefaultAgentConfig,
) -> UserAgentRecord {
    UserAgentRecord {
        agent_id: agent_id.trim().to_string(),
        user_id: user_id.trim().to_string(),
        hive_id: DEFAULT_HIVE_ID.to_string(),
        name: config.name.clone(),
        description: config.description.clone(),
        system_prompt: config.system_prompt.clone(),
        preview_skill: config.preview_skill,
        model_name: None,
        ability_items: config.ability_items.clone(),
        tool_names: config.tool_names.clone(),
        declared_tool_names: config.declared_tool_names.clone(),
        declared_skill_names: config.declared_skill_names.clone(),
        preset_questions: config.preset_questions.clone(),
        access_level: access_level.trim().to_string(),
        approval_mode: config.approval_mode.clone(),
        is_shared: false,
        status: config.status.clone(),
        icon: config.icon.clone(),
        sandbox_container_id: config.sandbox_container_id,
        created_at: config.created_at,
        updated_at: config.updated_at,
        preset_binding: None,
        silent: config.silent,
        prefer_mother: config.prefer_mother,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        default_agent_config_from_record, default_agent_meta_key, record_from_default_agent_config,
        DefaultAgentConfig,
    };
    use crate::storage::DEFAULT_HIVE_ID;

    #[test]
    fn builds_trimmed_default_agent_meta_key() {
        assert_eq!(default_agent_meta_key(" user-a "), "default_agent:user-a");
    }

    #[test]
    fn config_and_record_conversion_roundtrip() {
        let config = DefaultAgentConfig {
            name: "Default Agent".to_string(),
            description: "demo".to_string(),
            system_prompt: "prompt".to_string(),
            tool_names: vec!["read_file".to_string()],
            declared_tool_names: vec!["read_file".to_string()],
            approval_mode: "full_auto".to_string(),
            status: "active".to_string(),
            sandbox_container_id: 2,
            created_at: 1.0,
            updated_at: 2.0,
            ..Default::default()
        };
        let record = record_from_default_agent_config("__default__", "user-a", "A", &config);
        let restored = default_agent_config_from_record(&record);
        assert_eq!(record.agent_id, "__default__");
        assert_eq!(record.user_id, "user-a");
        assert_eq!(record.hive_id, DEFAULT_HIVE_ID);
        assert_eq!(restored.name, config.name);
        assert_eq!(restored.system_prompt, config.system_prompt);
        assert_eq!(restored.tool_names, config.tool_names);
    }
}
