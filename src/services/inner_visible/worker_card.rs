use crate::schemas::{AbilityDescriptor, AbilityGroupKey, AbilityKind, AbilitySourceKey};
use crate::services::agent_abilities::{
    build_ability_items_from_names, normalize_ability_items, resolve_record_ability_items,
    resolve_record_declared_names,
};
use crate::services::worker_card_protocol::{
    build_worker_card_prompt_envelope, resolve_worker_card_prompt_text, WorkerCardPrompt,
};
use crate::storage::{UserAgentRecord, DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID};
use chrono::Utc;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCardDocument {
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub metadata: WorkerCardMetadata,
    #[serde(default, skip_serializing_if = "WorkerCardPrompt::is_empty")]
    pub prompt: WorkerCardPrompt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_prompt: Option<String>,
    #[serde(default)]
    pub abilities: WorkerCardAbilities,
    #[serde(default)]
    pub interaction: WorkerCardInteraction,
    #[serde(default)]
    pub runtime: WorkerCardRuntime,
    #[serde(default)]
    pub hive: WorkerCardHive,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<WorkerCardPreset>,
    #[serde(default, skip_serializing_if = "worker_card_extensions_is_empty")]
    pub extensions: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerCardMetadata {
    #[serde(default)]
    pub agent_id: String,
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
    #[serde(default)]
    pub silent: bool,
    #[serde(default)]
    pub prefer_mother: bool,
    #[serde(default)]
    pub preview_skill: bool,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkerCardPreset {
    #[serde(default = "default_worker_card_preset_revision")]
    pub revision: u64,
    #[serde(default = "default_worker_card_preset_status")]
    pub status: String,
}

#[derive(Debug, Clone, Default)]
pub struct WorkerCardRecordUpdate {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub preview_skill: bool,
    pub model_name: Option<String>,
    pub ability_items: Vec<AbilityDescriptor>,
    pub tool_names: Vec<String>,
    pub declared_tool_names: Vec<String>,
    pub declared_skill_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub approval_mode: String,
    pub is_shared: bool,
    pub icon: Option<String>,
    pub hive_id: String,
    pub sandbox_container_id: i32,
    pub silent: bool,
    pub prefer_mother: bool,
}

pub fn build_worker_card(
    record: &UserAgentRecord,
    hive_name: Option<&str>,
    hive_description: Option<&str>,
    skill_name_keys: &HashSet<String>,
) -> WorkerCardDocument {
    let prompt_fields = build_worker_card_prompt_envelope(&record.system_prompt);
    let ability_items = resolve_record_ability_items(
        &record.ability_items,
        &record.tool_names,
        &record.declared_tool_names,
        &record.declared_skill_names,
        skill_name_keys,
    );
    let (declared_tool_names, declared_skill_names) = resolve_record_declared_names(
        &record.ability_items,
        &record.tool_names,
        &record.declared_tool_names,
        &record.declared_skill_names,
        skill_name_keys,
    );
    let worker_card_items = compact_worker_card_ability_items(
        build_worker_card_ability_items_from_descriptors(&ability_items),
        &declared_tool_names,
        &declared_skill_names,
    );
    WorkerCardDocument {
        schema_version: "wunder/worker-card@2".to_string(),
        kind: "WorkerCard".to_string(),
        metadata: WorkerCardMetadata {
            agent_id: record.agent_id.clone(),
            name: record.name.clone(),
            description: record.description.clone(),
            icon: record.icon.clone().unwrap_or_default(),
            exported_at: Utc::now().to_rfc3339(),
        },
        prompt: prompt_fields.prompt,
        system_prompt: prompt_fields.system_prompt,
        extra_prompt: prompt_fields.extra_prompt,
        abilities: WorkerCardAbilities {
            items: worker_card_items,
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
            silent: record.silent,
            prefer_mother: record.prefer_mother,
            preview_skill: record.preview_skill,
        },
        hive: WorkerCardHive {
            id: record.hive_id.clone(),
            name: hive_name.unwrap_or_default().to_string(),
            description: hive_description.unwrap_or_default().to_string(),
        },
        preset: None,
        extensions: serde_json::Value::Object(Default::default()),
    }
}

fn default_worker_card_preset_revision() -> u64 {
    1
}

fn default_worker_card_preset_status() -> String {
    "active".to_string()
}

fn worker_card_extensions_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

pub fn parse_worker_card(
    document: WorkerCardDocument,
    system_prompt_override: Option<String>,
) -> WorkerCardRecordUpdate {
    let ability_items = parse_document_ability_items(&document.abilities);
    let (declared_tool_names, declared_skill_names) =
        split_document_worker_card_abilities(&document.abilities);
    let mut tool_names = declared_tool_names.clone();
    tool_names.extend(declared_skill_names.iter().cloned());
    WorkerCardRecordUpdate {
        name: document.metadata.name.trim().to_string(),
        description: document.metadata.description.trim().to_string(),
        system_prompt: system_prompt_override
            .unwrap_or_else(|| {
                resolve_worker_card_prompt_text(
                    document.system_prompt.as_deref(),
                    document.extra_prompt.as_deref(),
                    &document.prompt,
                )
            })
            .trim()
            .to_string(),
        preview_skill: document.runtime.preview_skill,
        model_name: document
            .runtime
            .model_name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        ability_items,
        tool_names: normalize_names(tool_names),
        declared_tool_names,
        declared_skill_names,
        preset_questions: normalize_names(document.interaction.preset_questions),
        approval_mode: normalize_approval_mode(&document.runtime.approval_mode),
        is_shared: document.runtime.is_shared,
        icon: Some(document.metadata.icon.trim().to_string()).filter(|value| !value.is_empty()),
        hive_id: normalize_hive_id(&document.hive.id),
        sandbox_container_id: normalize_container_id(document.runtime.sandbox_container_id),
        silent: document.runtime.silent,
        prefer_mother: document.runtime.prefer_mother,
    }
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

fn build_worker_card_ability_items_from_descriptors(
    ability_items: &[AbilityDescriptor],
) -> Vec<WorkerCardAbilityItem> {
    let mut items = Vec::new();
    for item in ability_items {
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
        items.push(WorkerCardAbilityItem {
            id: item.id.trim().to_string(),
            name: if fallback_name.is_empty() {
                name.to_string()
            } else {
                fallback_name.to_string()
            },
            runtime_name: name.to_string(),
            display_name: item.display_name.trim().to_string(),
            description: item.description.trim().to_string(),
            kind: item.kind,
        });
    }
    if items.is_empty() {
        return build_worker_card_ability_items(&[], &[]);
    }
    items
}

fn worker_card_item_matches_legacy_shape(
    item: &WorkerCardAbilityItem,
    expected: &WorkerCardAbilityItem,
) -> bool {
    let runtime_name = item.runtime_name.trim();
    let fallback_name = item.name.trim();
    let resolved_name = if runtime_name.is_empty() {
        fallback_name
    } else {
        runtime_name
    };
    let normalized_name = if fallback_name.is_empty() {
        resolved_name
    } else {
        fallback_name
    };
    let display_name = item.display_name.trim();
    let normalized_display_name = if display_name.is_empty() {
        resolved_name
    } else {
        display_name
    };
    resolved_name == expected.runtime_name
        && item.kind == expected.kind
        && normalized_name == expected.name
        && normalized_display_name == expected.display_name
        && item.description.trim().is_empty()
}

fn compact_worker_card_ability_items(
    items: Vec<WorkerCardAbilityItem>,
    declared_tool_names: &[String],
    declared_skill_names: &[String],
) -> Vec<WorkerCardAbilityItem> {
    if items.is_empty() {
        return items;
    }
    let expected = build_worker_card_ability_items(declared_tool_names, declared_skill_names);
    if items.len() != expected.len() {
        return items;
    }
    if items
        .iter()
        .zip(expected.iter())
        .all(|(item, expected)| worker_card_item_matches_legacy_shape(item, expected))
    {
        Vec::new()
    } else {
        items
    }
}

fn parse_document_ability_items(abilities: &WorkerCardAbilities) -> Vec<AbilityDescriptor> {
    let (tool_names, skill_names) = split_document_worker_card_abilities(abilities);
    if abilities.tool_names_present || abilities.skills_present {
        let mut overrides = HashMap::new();
        for item in abilities
            .items
            .iter()
            .map(worker_card_item_to_ability_descriptor)
            .collect::<Vec<_>>()
        {
            let normalized_name = item.runtime_name.trim();
            if normalized_name.is_empty() {
                continue;
            }
            overrides.insert(
                format!("{:?}:{normalized_name}", item.kind),
                AbilityDescriptor {
                    runtime_name: normalized_name.to_string(),
                    ..item
                },
            );
        }
        return build_ability_items_from_names(&tool_names, &skill_names)
            .into_iter()
            .map(|item| {
                let key = format!("{:?}:{}", item.kind, item.runtime_name);
                overrides.remove(&key).unwrap_or(item)
            })
            .collect();
    }

    let items = abilities
        .items
        .iter()
        .map(worker_card_item_to_ability_descriptor)
        .collect::<Vec<_>>();
    let normalized = normalize_ability_items(items);
    if !normalized.is_empty() {
        return normalized;
    }
    build_ability_items_from_names(&tool_names, &skill_names)
}

fn worker_card_item_to_ability_descriptor(item: &WorkerCardAbilityItem) -> AbilityDescriptor {
    let (group, source) = match item.kind {
        AbilityKind::Tool => (AbilityGroupKey::Builtin, AbilitySourceKey::Builtin),
        AbilityKind::Skill => (AbilityGroupKey::Skills, AbilitySourceKey::Skill),
    };
    let runtime_name = item.runtime_name.trim();
    let fallback_name = item.name.trim();
    let name = if runtime_name.is_empty() {
        fallback_name
    } else {
        runtime_name
    };
    AbilityDescriptor {
        id: item.id.trim().to_string(),
        name: if fallback_name.is_empty() {
            name.to_string()
        } else {
            fallback_name.to_string()
        },
        runtime_name: name.to_string(),
        display_name: item.display_name.trim().to_string(),
        description: item.description.trim().to_string(),
        input_schema: Value::Null,
        group,
        source,
        kind: item.kind,
        owner_id: None,
        available: true,
        selected: true,
    }
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
            preview_skill: false,
            model_name: Some("gpt".to_string()),
            ability_items: Vec::new(),
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
            silent: false,
            prefer_mother: false,
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
        assert!(document.abilities.items.is_empty());
        assert_eq!(document.prompt.system_prompt, None);
        assert!(document.prompt.is_empty());
        assert_eq!(document.extra_prompt, Some("prompt".to_string()));
        assert_eq!(document.runtime.model_name, Some("gpt".to_string()));
    }

    #[test]
    fn build_worker_card_keeps_items_when_metadata_is_non_default() {
        let record = UserAgentRecord {
            agent_id: "agent-2".to_string(),
            user_id: "u1".to_string(),
            hive_id: "default".to_string(),
            name: "Agent".to_string(),
            description: "desc".to_string(),
            system_prompt: String::new(),
            preview_skill: false,
            model_name: None,
            ability_items: vec![crate::schemas::AbilityDescriptor {
                id: "builtin:read_file".to_string(),
                name: "read_file".to_string(),
                runtime_name: "read_file".to_string(),
                display_name: "Read File".to_string(),
                description: "Reads text files".to_string(),
                input_schema: serde_json::Value::Null,
                group: crate::schemas::AbilityGroupKey::Builtin,
                source: crate::schemas::AbilitySourceKey::Builtin,
                kind: AbilityKind::Tool,
                owner_id: None,
                available: true,
                selected: true,
            }],
            tool_names: vec!["read_file".to_string()],
            declared_tool_names: vec!["read_file".to_string()],
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
            silent: false,
            prefer_mother: false,
        };
        let document = build_worker_card(&record, Some("Default"), Some("Hive"), &HashSet::new());
        assert_eq!(document.abilities.items.len(), 1);
        assert_eq!(document.abilities.items[0].display_name, "Read File");
        assert_eq!(document.abilities.items[0].description, "Reads text files");
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
            preview_skill: false,
            model_name: None,
            ability_items: Vec::new(),
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
            silent: false,
            prefer_mother: false,
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

    #[test]
    fn parse_worker_card_prefers_explicit_arrays_over_stale_items() {
        let payload = parse_worker_card(
            serde_json::from_str(
                r#"{
                  "schema_version": "wunder/worker-card@2",
                  "metadata": { "name": "demo" },
                  "abilities": {
                    "items": [
                      { "runtime_name": "read_file", "kind": "tool" },
                      { "runtime_name": "planner", "kind": "skill" }
                    ],
                    "tool_names": ["read_file"],
                    "skills": []
                  }
                }"#,
            )
            .expect("worker card"),
            None,
        );
        assert_eq!(payload.declared_tool_names, vec!["read_file".to_string()]);
        assert!(payload.declared_skill_names.is_empty());
        assert_eq!(payload.tool_names, vec!["read_file".to_string()]);
        assert!(payload
            .ability_items
            .iter()
            .all(|item| item.runtime_name != "planner"));
    }

    #[test]
    fn parse_worker_card_supports_top_level_extra_prompt() {
        let payload = parse_worker_card(
            serde_json::from_str(
                r#"{
                  "schema_version": "wunder/worker-card@2",
                  "metadata": { "name": "demo" },
                  "extra_prompt": "top level prompt"
                }"#,
            )
            .expect("worker card"),
            None,
        );
        assert_eq!(payload.system_prompt, "top level prompt".to_string());
    }
}
