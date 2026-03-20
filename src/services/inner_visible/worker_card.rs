use crate::schemas::AbilityKind;
use crate::storage::{UserAgentRecord, DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID};
use chrono::Utc;
use serde::{Deserialize, Deserializer, Serialize};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_prompt: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCardAbilityItem {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub runtime_name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub kind: AbilityKind,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkerCardAbilities {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<WorkerCardAbilityItem>,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(skip)]
    pub tool_names_present: bool,
    #[serde(skip)]
    pub skills_present: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct WorkerCardAbilitiesRaw {
    #[serde(default)]
    items: Vec<WorkerCardAbilityItem>,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    skills: Vec<String>,
}

impl<'de> Deserialize<'de> for WorkerCardAbilities {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let Some(object) = value.as_object() else {
            return Ok(Self::default());
        };
        let tool_names_present = object.contains_key("tool_names");
        let skills_present = object.contains_key("skills");
        let raw: WorkerCardAbilitiesRaw =
            serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(Self {
            items: raw.items,
            tool_names: raw.tool_names,
            skills: raw.skills,
            tool_names_present,
            skills_present,
        })
    }
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
    skill_name_keys: &HashSet<String>,
) -> WorkerCardDocument {
    let (declared_tool_names, declared_skill_names) =
        split_record_worker_card_abilities(record, skill_name_keys);
    WorkerCardDocument {
        schema_version: "wunder/worker-card@2".to_string(),
        kind: "WorkerCard".to_string(),
        metadata: WorkerCardMetadata {
            id: record.agent_id.clone(),
            name: record.name.clone(),
            description: record.description.clone(),
            icon: record.icon.clone().unwrap_or_default(),
            exported_at: Utc::now().to_rfc3339(),
        },
        prompt: WorkerCardPrompt {
            system_prompt: None,
            extra_prompt: Some(record.system_prompt.trim().to_string())
                .filter(|value| !value.is_empty()),
        },
        abilities: WorkerCardAbilities {
            items: build_worker_card_ability_items(&declared_tool_names, &declared_skill_names),
            tool_names: declared_tool_names,
            skills: declared_skill_names,
            tool_names_present: true,
            skills_present: true,
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
    let (declared_tool_names, declared_skill_names) =
        split_document_worker_card_abilities(&document.abilities);
    let mut tool_names = declared_tool_names.clone();
    tool_names.extend(declared_skill_names.iter().cloned());
    WorkerCardRecordUpdate {
        name: document.metadata.name.trim().to_string(),
        description: document.metadata.description.trim().to_string(),
        system_prompt: system_prompt_override
            .unwrap_or_else(|| worker_card_prompt_text(&document.prompt))
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

fn split_record_worker_card_abilities(
    record: &UserAgentRecord,
    skill_name_keys: &HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    if !record.declared_tool_names.is_empty() || !record.declared_skill_names.is_empty() {
        return (
            normalize_names(record.declared_tool_names.clone()),
            normalize_names(record.declared_skill_names.clone()),
        );
    }

    let mut tool_names = Vec::new();
    let mut skill_names = Vec::new();
    for name in normalize_names(record.tool_names.clone()) {
        if skill_name_keys.contains(&name) {
            skill_names.push(name);
        } else {
            tool_names.push(name);
        }
    }
    (tool_names, skill_names)
}

fn build_worker_card_ability_items(
    declared_tool_names: &[String],
    declared_skill_names: &[String],
) -> Vec<WorkerCardAbilityItem> {
    let mut items = Vec::new();
    for name in normalize_names(declared_tool_names.to_vec()) {
        items.push(WorkerCardAbilityItem {
            id: format!("tool:{name}"),
            name: name.clone(),
            runtime_name: name.clone(),
            display_name: name,
            description: String::new(),
            kind: AbilityKind::Tool,
        });
    }
    for name in normalize_names(declared_skill_names.to_vec()) {
        items.push(WorkerCardAbilityItem {
            id: format!("skill:{name}"),
            name: name.clone(),
            runtime_name: name.clone(),
            display_name: name,
            description: String::new(),
            kind: AbilityKind::Skill,
        });
    }
    items
}

fn split_document_worker_card_abilities(
    abilities: &WorkerCardAbilities,
) -> (Vec<String>, Vec<String>) {
    if abilities.tool_names_present || abilities.skills_present {
        return (
            normalize_names(abilities.tool_names.clone()),
            normalize_names(abilities.skills.clone()),
        );
    }

    let mut tool_names = Vec::new();
    let mut skill_names = Vec::new();
    for item in &abilities.items {
        let runtime_name = item.runtime_name.trim();
        let fallback_name = item.name.trim();
        let name = if runtime_name.is_empty() {
            fallback_name
        } else {
            runtime_name
        };
        if name.is_empty() {
            continue;
        }
        match item.kind {
            AbilityKind::Skill => skill_names.push(name.to_string()),
            AbilityKind::Tool => tool_names.push(name.to_string()),
        }
    }
    (normalize_names(tool_names), normalize_names(skill_names))
}

fn worker_card_prompt_text(prompt: &WorkerCardPrompt) -> String {
    [
        prompt.system_prompt.as_deref(),
        prompt.extra_prompt.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(ToOwned::to_owned)
    .collect::<Vec<_>>()
    .join("\n\n")
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
    use super::{build_worker_card, parse_worker_card, AbilityKind, UserAgentRecord};
    use std::collections::HashSet;

    #[test]
    fn parse_worker_card_merges_declared_dependencies_into_runtime_tools() {
        let payload = parse_worker_card(
            serde_json::from_str(
                r#"{
                  "metadata": { "name": "demo" },
                  "prompt": {
                    "system_prompt": "legacy",
                    "extra_prompt": "extra"
                  },
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
        assert_eq!(payload.system_prompt, "legacy\n\nextra".to_string());
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
        let document = build_worker_card(
            &record,
            Some("Default"),
            Some("Hive"),
            &HashSet::from(["planner".to_string()]),
        );
        assert_eq!(document.schema_version, "wunder/worker-card@2");
        assert_eq!(document.abilities.tool_names, vec!["read_file".to_string()]);
        assert_eq!(document.abilities.skills, vec!["planner".to_string()]);
        assert_eq!(document.abilities.items.len(), 2);
        assert!(document
            .abilities
            .items
            .iter()
            .any(|item| { item.runtime_name == "planner" && item.kind == AbilityKind::Skill }));
        assert_eq!(document.prompt.system_prompt, None);
        assert_eq!(document.prompt.extra_prompt, Some("prompt".to_string()));
        assert_eq!(document.runtime.model_name, Some("gpt".to_string()));
    }

    #[test]
    fn build_worker_card_infers_skills_from_runtime_tool_names_when_declared_empty() {
        let record = UserAgentRecord {
            agent_id: "agent-1".to_string(),
            user_id: "u1".to_string(),
            hive_id: "default".to_string(),
            name: "Agent".to_string(),
            description: "desc".to_string(),
            system_prompt: String::new(),
            model_name: None,
            tool_names: vec![
                "read_file".to_string(),
                "技能创建器".to_string(),
                "write_file".to_string(),
            ],
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
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
        let document = build_worker_card(
            &record,
            Some("Default"),
            Some("Hive"),
            &HashSet::from(["技能创建器".to_string()]),
        );
        assert_eq!(
            document.abilities.tool_names,
            vec!["read_file".to_string(), "write_file".to_string()]
        );
        assert_eq!(document.abilities.skills, vec!["技能创建器".to_string()]);
    }

    #[test]
    fn parse_worker_card_supports_structured_ability_items_without_legacy_arrays() {
        let payload = parse_worker_card(
            serde_json::from_str(
                r#"{
                  "schema_version": "wunder/worker-card@2",
                  "metadata": { "name": "demo" },
                  "abilities": {
                    "items": [
                      { "runtime_name": "read_file", "kind": "tool" },
                      { "runtime_name": "planner", "kind": "skill" }
                    ]
                  }
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
    }
}
