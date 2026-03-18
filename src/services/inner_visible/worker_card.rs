use crate::storage::{UserAgentRecord, DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCardDocument {
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub metadata: WorkerCardMetadata,
    #[serde(default)]
    pub prompt: WorkerCardPrompt,
    #[serde(default)]
    pub abilities: WorkerCardAbilities,
    #[serde(default)]
    pub interaction: WorkerCardInteraction,
    #[serde(default)]
    pub runtime: WorkerCardRuntime,
    #[serde(default)]
    pub hive: WorkerCardHive,
    #[serde(default)]
    pub extensions: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCardMetadata {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub exported_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCardPrompt {
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub extra_prompt: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCardAbilities {
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCardInteraction {
    #[serde(default)]
    pub preset_questions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCardRuntime {
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub approval_mode: String,
    #[serde(default)]
    pub sandbox_container_id: i32,
    #[serde(default)]
    pub is_shared: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCardHive {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default)]
pub struct WorkerCardRecordUpdate {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub model_name: Option<String>,
    pub tool_names: Vec<String>,
    pub declared_tool_names: Vec<String>,
    pub declared_skill_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub approval_mode: String,
    pub is_shared: bool,
    pub icon: Option<String>,
    pub hive_id: String,
    pub sandbox_container_id: i32,
}

pub fn build_worker_card(
    record: &UserAgentRecord,
    hive_name: Option<&str>,
    hive_description: Option<&str>,
) -> WorkerCardDocument {
    WorkerCardDocument {
        schema_version: "wunder/worker-card@1".to_string(),
        kind: "WorkerCard".to_string(),
        metadata: WorkerCardMetadata {
            id: record.agent_id.clone(),
            name: record.name.clone(),
            description: record.description.clone(),
            icon: record.icon.clone().unwrap_or_default(),
            exported_at: Utc::now().to_rfc3339(),
        },
        prompt: WorkerCardPrompt {
            system_prompt: record.system_prompt.clone(),
            extra_prompt: String::new(),
        },
        abilities: WorkerCardAbilities {
            tool_names: normalize_names(if record.declared_tool_names.is_empty() {
                record.tool_names.clone()
            } else {
                record.declared_tool_names.clone()
            }),
            skills: normalize_names(record.declared_skill_names.clone()),
        },
        interaction: WorkerCardInteraction {
            preset_questions: normalize_names(record.preset_questions.clone()),
        },
        runtime: WorkerCardRuntime {
            model_name: record
                .model_name
                .clone()
                .filter(|value| !value.trim().is_empty()),
            approval_mode: record.approval_mode.clone(),
            sandbox_container_id: record.sandbox_container_id,
            is_shared: record.is_shared,
        },
        hive: WorkerCardHive {
            id: record.hive_id.clone(),
            name: hive_name.unwrap_or_default().to_string(),
            description: hive_description.unwrap_or_default().to_string(),
        },
        extensions: serde_json::Value::Object(Default::default()),
    }
}

pub fn parse_worker_card(
    document: WorkerCardDocument,
    system_prompt_override: Option<String>,
) -> WorkerCardRecordUpdate {
    let declared_tool_names = normalize_names(document.abilities.tool_names);
    let declared_skill_names = normalize_names(document.abilities.skills);
    let mut tool_names = declared_tool_names.clone();
    tool_names.extend(declared_skill_names.iter().cloned());
    WorkerCardRecordUpdate {
        name: document.metadata.name.trim().to_string(),
        description: document.metadata.description.trim().to_string(),
        system_prompt: system_prompt_override
            .unwrap_or(document.prompt.system_prompt)
            .trim()
            .to_string(),
        model_name: document
            .runtime
            .model_name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        tool_names: normalize_names(tool_names),
        declared_tool_names,
        declared_skill_names,
        preset_questions: normalize_names(document.interaction.preset_questions),
        approval_mode: normalize_approval_mode(&document.runtime.approval_mode),
        is_shared: document.runtime.is_shared,
        icon: Some(document.metadata.icon.trim().to_string()).filter(|value| !value.is_empty()),
        hive_id: normalize_hive_id(&document.hive.id),
        sandbox_container_id: normalize_container_id(document.runtime.sandbox_container_id),
    }
}

pub fn normalize_names(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw in values {
        let cleaned = raw.trim().to_string();
        if cleaned.is_empty() || !seen.insert(cleaned.clone()) {
            continue;
        }
        output.push(cleaned);
    }
    output
}

pub fn normalize_approval_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "suggest" => "suggest".to_string(),
        "auto_edit" | "auto-edit" => "auto_edit".to_string(),
        "full_auto" | "full-auto" => "full_auto".to_string(),
        _ => "full_auto".to_string(),
    }
}

pub fn normalize_container_id(raw: i32) -> i32 {
    if (1..=10).contains(&raw) {
        raw
    } else {
        DEFAULT_SANDBOX_CONTAINER_ID
    }
}

pub fn normalize_hive_id(raw: &str) -> String {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        DEFAULT_HIVE_ID.to_string()
    } else {
        cleaned.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{build_worker_card, parse_worker_card, UserAgentRecord};

    #[test]
    fn parse_worker_card_merges_declared_dependencies_into_runtime_tools() {
        let payload = parse_worker_card(
            serde_json::from_str(
                r#"{
                  "metadata": { "name": "demo" },
                  "abilities": {
                    "tool_names": ["read_file", "read_file"],
                    "skills": ["planner"]
                  },
                  "runtime": { "approval_mode": "auto-edit", "sandbox_container_id": 2 }
                }"#,
            )
            .expect("worker card"),
            None,
        );
        assert_eq!(payload.declared_tool_names, vec!["read_file".to_string()]);
        assert_eq!(payload.declared_skill_names, vec!["planner".to_string()]);
        assert_eq!(
            payload.tool_names,
            vec!["read_file".to_string(), "planner".to_string()]
        );
        assert_eq!(payload.approval_mode, "auto_edit".to_string());
        assert_eq!(payload.sandbox_container_id, 2);
    }

    #[test]
    fn build_worker_card_prefers_declared_tools_for_worker_card_output() {
        let record = UserAgentRecord {
            agent_id: "agent-1".to_string(),
            user_id: "u1".to_string(),
            hive_id: "default".to_string(),
            name: "Agent".to_string(),
            description: "desc".to_string(),
            system_prompt: "prompt".to_string(),
            model_name: Some("gpt".to_string()),
            tool_names: vec!["read_file".to_string(), "planner".to_string()],
            declared_tool_names: vec!["read_file".to_string()],
            declared_skill_names: vec!["planner".to_string()],
            preset_questions: vec!["Q".to_string()],
            access_level: "A".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
            created_at: 1.0,
            updated_at: 2.0,
            preset_binding: None,
        };
        let document = build_worker_card(&record, Some("Default"), Some("Hive"));
        assert_eq!(document.abilities.tool_names, vec!["read_file".to_string()]);
        assert_eq!(document.abilities.skills, vec!["planner".to_string()]);
        assert_eq!(document.runtime.model_name, Some("gpt".to_string()));
    }
}
