use crate::config::{Config, UserAgentPresetConfig};
use crate::services::agent_abilities::{normalize_ability_items, resolve_selected_declared_names};
use crate::services::default_agent_protocol::DefaultAgentConfig;
use crate::services::inner_visible::{
    build_worker_card, parse_worker_card, WorkerCardRecordUpdate,
};
use crate::services::skills::SkillRegistry;
use crate::services::user_access::UserToolContext;
use crate::services::user_tools::UserToolKind;
use crate::skills::load_skills;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, UserAgentPresetSnapshot, UserAgentRecord,
    DEFAULT_HIVE_ID,
};
use std::collections::HashSet;

const DEFAULT_AGENT_APPROVAL_MODE: &str = "full_auto";
const DEFAULT_AGENT_STATUS: &str = "active";
const DEFAULT_PRESET_ICON_NAME: &str = "spark";
const DEFAULT_PRESET_ICON_COLOR: &str = "#94a3b8";
const CANONICAL_AGENT_ID: &str = "__worker_card_settings__";
const CANONICAL_USER_ID: &str = "__worker_card_settings__";

pub fn normalize_tool_list(values: Vec<String>) -> Vec<String> {
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

pub fn normalize_preset_questions(values: Vec<String>) -> Vec<String> {
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

pub fn normalize_optional_model_name(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn normalize_agent_approval_mode(raw: Option<&str>) -> String {
    match raw.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "suggest" => "suggest".to_string(),
        "auto_edit" | "auto-edit" => "auto_edit".to_string(),
        "full_auto" | "full-auto" => "full_auto".to_string(),
        _ => DEFAULT_AGENT_APPROVAL_MODE.to_string(),
    }
}

pub fn normalize_agent_status(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or(DEFAULT_AGENT_STATUS).trim();
    if cleaned.is_empty() {
        DEFAULT_AGENT_STATUS.to_string()
    } else {
        cleaned.to_string()
    }
}

pub fn build_icon_payload(name: &str, color: &str) -> String {
    serde_json::json!({ "name": name, "color": color }).to_string()
}

pub fn normalize_preset_icon_name(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or_default().trim();
    if cleaned.is_empty() {
        return DEFAULT_PRESET_ICON_NAME.to_string();
    }
    if cleaned
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return cleaned.to_string();
    }
    DEFAULT_PRESET_ICON_NAME.to_string()
}

fn normalize_icon_color(raw: &str) -> Option<String> {
    let cleaned = raw.trim().trim_start_matches('#');
    let expanded = match cleaned.len() {
        3 if cleaned.chars().all(|ch| ch.is_ascii_hexdigit()) => {
            cleaned.chars().flat_map(|ch| [ch, ch]).collect::<String>()
        }
        6 if cleaned.chars().all(|ch| ch.is_ascii_hexdigit()) => cleaned.to_string(),
        _ => return None,
    };
    Some(format!("#{}", expanded.to_ascii_lowercase()))
}

pub fn normalize_preset_icon_color(raw: Option<&str>) -> String {
    raw.and_then(normalize_icon_color)
        .unwrap_or_else(|| DEFAULT_PRESET_ICON_COLOR.to_string())
}

pub fn normalize_preset_icon_parts(raw: Option<&str>) -> (String, String) {
    let cleaned = raw.unwrap_or_default().trim();
    if cleaned.is_empty() {
        return (
            normalize_preset_icon_name(None),
            normalize_preset_icon_color(None),
        );
    }
    if cleaned.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(cleaned) {
            let icon_name =
                normalize_preset_icon_name(value.get("name").and_then(serde_json::Value::as_str));
            let icon_color = value
                .get("color")
                .and_then(serde_json::Value::as_str)
                .map_or_else(
                    || normalize_preset_icon_color(None),
                    |color| normalize_preset_icon_color(Some(color)),
                );
            return (icon_name, icon_color);
        }
    }
    (
        normalize_preset_icon_name(Some(cleaned)),
        normalize_preset_icon_color(None),
    )
}

pub fn collect_context_skill_names(context: &UserToolContext) -> HashSet<String> {
    let mut output = HashSet::new();
    for spec in context.skills.list_specs() {
        let cleaned = spec.name.trim();
        if !cleaned.is_empty() {
            output.insert(cleaned.to_string());
        }
    }
    for spec in &context.bindings.skill_specs {
        let cleaned = spec.name.trim();
        if !cleaned.is_empty() {
            output.insert(cleaned.to_string());
        }
    }
    for (alias, info) in &context.bindings.alias_map {
        if !matches!(info.kind, UserToolKind::Skill) {
            continue;
        }
        let cleaned_alias = alias.trim();
        if !cleaned_alias.is_empty() {
            output.insert(cleaned_alias.to_string());
        }
        let cleaned_target = info.target.trim();
        if !cleaned_target.is_empty() {
            output.insert(cleaned_target.to_string());
        }
    }
    output
}

pub fn collect_registry_skill_names(registry: &SkillRegistry) -> HashSet<String> {
    registry
        .list_specs()
        .into_iter()
        .filter_map(|spec| {
            let cleaned = spec.name.trim();
            (!cleaned.is_empty()).then(|| cleaned.to_string())
        })
        .collect()
}

pub fn collect_configured_skill_names(config: &Config) -> HashSet<String> {
    let registry = load_skills(config, false, false, true);
    collect_registry_skill_names(&registry)
}

fn record_from_update(update: &WorkerCardRecordUpdate) -> UserAgentRecord {
    UserAgentRecord {
        agent_id: CANONICAL_AGENT_ID.to_string(),
        user_id: CANONICAL_USER_ID.to_string(),
        hive_id: normalize_hive_id(&update.hive_id),
        name: update.name.trim().to_string(),
        description: update.description.trim().to_string(),
        system_prompt: update.system_prompt.trim().to_string(),
        model_name: normalize_optional_model_name(update.model_name.as_deref()),
        ability_items: normalize_ability_items(update.ability_items.clone()),
        tool_names: normalize_tool_list(update.tool_names.clone()),
        declared_tool_names: normalize_tool_list(update.declared_tool_names.clone()),
        declared_skill_names: normalize_tool_list(update.declared_skill_names.clone()),
        preset_questions: normalize_preset_questions(update.preset_questions.clone()),
        access_level: "A".to_string(),
        approval_mode: normalize_agent_approval_mode(Some(&update.approval_mode)),
        is_shared: update.is_shared,
        status: DEFAULT_AGENT_STATUS.to_string(),
        icon: update.icon.as_deref().and_then(|value| {
            let cleaned = value.trim();
            (!cleaned.is_empty()).then(|| cleaned.to_string())
        }),
        sandbox_container_id: normalize_sandbox_container_id(update.sandbox_container_id),
        created_at: 0.0,
        updated_at: 0.0,
        preset_binding: None,
        silent: update.silent,
        prefer_mother: update.prefer_mother,
    }
}

pub fn canonicalize_worker_card_update(
    update: WorkerCardRecordUpdate,
    skill_name_keys: &HashSet<String>,
) -> WorkerCardRecordUpdate {
    let mut selected_tool_names = normalize_tool_list(update.tool_names.clone());
    let explicit_declared_tool_names = normalize_tool_list(update.declared_tool_names.clone());
    let explicit_declared_skill_names = normalize_tool_list(update.declared_skill_names.clone());
    if selected_tool_names.is_empty() {
        selected_tool_names.extend(explicit_declared_tool_names.iter().cloned());
    }
    selected_tool_names.extend(explicit_declared_skill_names.iter().cloned());
    selected_tool_names = normalize_tool_list(selected_tool_names);
    let (declared_tool_names, declared_skill_names) = if selected_tool_names.is_empty() {
        (explicit_declared_tool_names, explicit_declared_skill_names)
    } else {
        resolve_selected_declared_names(
            &selected_tool_names,
            &explicit_declared_tool_names,
            &explicit_declared_skill_names,
            skill_name_keys,
        )
    };
    let record = record_from_update(&WorkerCardRecordUpdate {
        tool_names: selected_tool_names,
        declared_tool_names,
        declared_skill_names,
        ..update
    });
    let mut parsed = parse_worker_card(
        build_worker_card(&record, None, None, skill_name_keys),
        None,
    );
    if parsed.name.is_empty() {
        parsed.name = record.name;
    }
    parsed.description = parsed.description.trim().to_string();
    parsed.system_prompt = parsed.system_prompt.trim().to_string();
    parsed.model_name = normalize_optional_model_name(parsed.model_name.as_deref());
    parsed.ability_items = normalize_ability_items(parsed.ability_items);
    parsed.tool_names = normalize_tool_list(parsed.tool_names);
    parsed.declared_tool_names = normalize_tool_list(parsed.declared_tool_names);
    parsed.declared_skill_names = normalize_tool_list(parsed.declared_skill_names);
    parsed.preset_questions = normalize_preset_questions(parsed.preset_questions);
    parsed.approval_mode = normalize_agent_approval_mode(Some(&parsed.approval_mode));
    parsed.icon = parsed.icon.as_deref().and_then(|value| {
        let cleaned = value.trim();
        (!cleaned.is_empty()).then(|| cleaned.to_string())
    });
    parsed.hive_id = normalize_hive_id(&parsed.hive_id);
    parsed.sandbox_container_id = normalize_sandbox_container_id(parsed.sandbox_container_id);
    parsed
}

pub fn worker_card_update_from_record(
    record: &UserAgentRecord,
    skill_name_keys: &HashSet<String>,
) -> WorkerCardRecordUpdate {
    canonicalize_worker_card_update(
        WorkerCardRecordUpdate {
            name: record.name.clone(),
            description: record.description.clone(),
            system_prompt: record.system_prompt.clone(),
            model_name: record.model_name.clone(),
            ability_items: record.ability_items.clone(),
            tool_names: record.tool_names.clone(),
            declared_tool_names: record.declared_tool_names.clone(),
            declared_skill_names: record.declared_skill_names.clone(),
            preset_questions: record.preset_questions.clone(),
            approval_mode: record.approval_mode.clone(),
            is_shared: record.is_shared,
            icon: record.icon.clone(),
            hive_id: record.hive_id.clone(),
            sandbox_container_id: record.sandbox_container_id,
            silent: record.silent,
            prefer_mother: record.prefer_mother,
        },
        skill_name_keys,
    )
}

pub fn preset_snapshot_from_update(
    update: &WorkerCardRecordUpdate,
    model_name: Option<String>,
    status: &str,
) -> UserAgentPresetSnapshot {
    UserAgentPresetSnapshot {
        name: update.name.clone(),
        description: update.description.clone(),
        system_prompt: update.system_prompt.clone(),
        model_name: model_name
            .or_else(|| normalize_optional_model_name(update.model_name.as_deref())),
        ability_items: normalize_ability_items(update.ability_items.clone()),
        tool_names: normalize_tool_list(update.tool_names.clone()),
        declared_tool_names: normalize_tool_list(update.declared_tool_names.clone()),
        declared_skill_names: normalize_tool_list(update.declared_skill_names.clone()),
        preset_questions: normalize_preset_questions(update.preset_questions.clone()),
        approval_mode: normalize_agent_approval_mode(Some(&update.approval_mode)),
        status: normalize_agent_status(Some(status)),
        icon: update.icon.clone(),
        sandbox_container_id: normalize_sandbox_container_id(update.sandbox_container_id),
    }
}

pub fn preset_snapshot_from_record(
    record: &UserAgentRecord,
    skill_name_keys: &HashSet<String>,
) -> UserAgentPresetSnapshot {
    let update = worker_card_update_from_record(record, skill_name_keys);
    preset_snapshot_from_update(
        &update,
        record.model_name.clone(),
        &normalize_agent_status(Some(&record.status)),
    )
}

pub fn preset_update_from_config(
    config: &UserAgentPresetConfig,
    skill_name_keys: &HashSet<String>,
) -> Option<WorkerCardRecordUpdate> {
    let name = config.name.trim();
    if name.is_empty() {
        return None;
    }
    Some(canonicalize_worker_card_update(
        WorkerCardRecordUpdate {
            name: name.to_string(),
            description: config.description.trim().to_string(),
            system_prompt: config.system_prompt.trim().to_string(),
            model_name: normalize_optional_model_name(config.model_name.as_deref()),
            ability_items: Vec::new(),
            tool_names: normalize_tool_list(config.tool_names.clone()),
            declared_tool_names: normalize_tool_list(config.declared_tool_names.clone()),
            declared_skill_names: normalize_tool_list(config.declared_skill_names.clone()),
            preset_questions: normalize_preset_questions(config.preset_questions.clone()),
            approval_mode: normalize_agent_approval_mode(Some(&config.approval_mode)),
            is_shared: false,
            icon: Some(build_icon_payload(&config.icon_name, &config.icon_color)),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            silent: false,
            prefer_mother: false,
            sandbox_container_id: normalize_sandbox_container_id(config.sandbox_container_id),
        },
        skill_name_keys,
    ))
}

pub fn preset_config_from_update(
    preset_id: &str,
    revision: u64,
    status: &str,
    update: &WorkerCardRecordUpdate,
) -> UserAgentPresetConfig {
    let (icon_name, icon_color) = normalize_preset_icon_parts(update.icon.as_deref());
    UserAgentPresetConfig {
        preset_id: preset_id.trim().to_string(),
        revision: revision.max(1),
        name: update.name.clone(),
        description: update.description.clone(),
        system_prompt: update.system_prompt.clone(),
        model_name: normalize_optional_model_name(update.model_name.as_deref()),
        icon_name,
        icon_color,
        sandbox_container_id: normalize_sandbox_container_id(update.sandbox_container_id),
        tool_names: normalize_tool_list(update.tool_names.clone()),
        declared_tool_names: normalize_tool_list(update.declared_tool_names.clone()),
        declared_skill_names: normalize_tool_list(update.declared_skill_names.clone()),
        preset_questions: normalize_preset_questions(update.preset_questions.clone()),
        approval_mode: normalize_agent_approval_mode(Some(&update.approval_mode)),
        status: normalize_agent_status(Some(status)),
    }
}

pub fn canonicalize_preset_config(
    config: &UserAgentPresetConfig,
    preset_id: &str,
    skill_name_keys: &HashSet<String>,
) -> Option<UserAgentPresetConfig> {
    let update = preset_update_from_config(config, skill_name_keys)?;
    Some(preset_config_from_update(
        preset_id,
        config.revision,
        &config.status,
        &update,
    ))
}

pub fn default_agent_update_from_config(
    config: &DefaultAgentConfig,
    skill_name_keys: &HashSet<String>,
) -> WorkerCardRecordUpdate {
    canonicalize_worker_card_update(
        WorkerCardRecordUpdate {
            name: config.name.trim().to_string(),
            description: config.description.trim().to_string(),
            system_prompt: config.system_prompt.trim().to_string(),
            model_name: None,
            ability_items: config.ability_items.clone(),
            tool_names: normalize_tool_list(config.tool_names.clone()),
            declared_tool_names: normalize_tool_list(config.declared_tool_names.clone()),
            declared_skill_names: normalize_tool_list(config.declared_skill_names.clone()),
            preset_questions: normalize_preset_questions(config.preset_questions.clone()),
            approval_mode: normalize_agent_approval_mode(Some(&config.approval_mode)),
            is_shared: false,
            icon: config
                .icon
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            silent: config.silent,
            prefer_mother: config.prefer_mother,
            sandbox_container_id: normalize_sandbox_container_id(config.sandbox_container_id),
        },
        skill_name_keys,
    )
}

pub fn default_agent_config_from_update(
    update: &WorkerCardRecordUpdate,
    status: &str,
    created_at: f64,
    updated_at: f64,
) -> DefaultAgentConfig {
    DefaultAgentConfig {
        name: update.name.clone(),
        description: update.description.clone(),
        system_prompt: update.system_prompt.clone(),
        ability_items: normalize_ability_items(update.ability_items.clone()),
        tool_names: normalize_tool_list(update.tool_names.clone()),
        declared_tool_names: normalize_tool_list(update.declared_tool_names.clone()),
        declared_skill_names: normalize_tool_list(update.declared_skill_names.clone()),
        preset_questions: normalize_preset_questions(update.preset_questions.clone()),
        approval_mode: normalize_agent_approval_mode(Some(&update.approval_mode)),
        status: normalize_agent_status(Some(status)),
        icon: update.icon.clone(),
        sandbox_container_id: normalize_sandbox_container_id(update.sandbox_container_id),
        silent: update.silent,
        prefer_mother: update.prefer_mother,
        created_at,
        updated_at,
    }
}

pub fn canonicalize_default_agent_config(
    config: &DefaultAgentConfig,
    skill_name_keys: &HashSet<String>,
) -> DefaultAgentConfig {
    default_agent_config_from_update(
        &default_agent_update_from_config(config, skill_name_keys),
        &config.status,
        config.created_at,
        config.updated_at,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        canonicalize_default_agent_config, canonicalize_preset_config,
        collect_configured_skill_names, normalize_preset_icon_parts, preset_update_from_config,
    };
    use crate::config::{Config, UserAgentPresetConfig};
    use crate::services::default_agent_protocol::DefaultAgentConfig;
    use std::collections::HashSet;

    fn sample_skill_keys() -> HashSet<String> {
        HashSet::from(["planner".to_string()])
    }

    #[test]
    fn canonicalize_preset_config_reclassifies_skills_via_worker_card_rules() {
        let preset = UserAgentPresetConfig {
            preset_id: "preset_demo".to_string(),
            revision: 2,
            name: "Demo Preset".to_string(),
            description: "desc".to_string(),
            system_prompt: "prompt".to_string(),
            model_name: Some("model-a".to_string()),
            icon_name: "spark".to_string(),
            icon_color: "#ABC".to_string(),
            sandbox_container_id: 99,
            tool_names: vec!["planner".to_string(), "read_file".to_string()],
            declared_tool_names: vec!["planner".to_string()],
            declared_skill_names: Vec::new(),
            preset_questions: vec![" q1 ".to_string(), "q1".to_string()],
            approval_mode: "full-auto".to_string(),
            status: "active".to_string(),
        };

        let normalized =
            canonicalize_preset_config(&preset, "preset_demo", &sample_skill_keys()).unwrap();
        assert_eq!(
            normalized.tool_names,
            vec!["read_file".to_string(), "planner".to_string()]
        );
        assert_eq!(
            normalized.declared_tool_names,
            vec!["read_file".to_string()]
        );
        assert_eq!(normalized.declared_skill_names, vec!["planner".to_string()]);
        assert_eq!(normalized.sandbox_container_id, 10);
        assert_eq!(normalized.preset_questions, vec!["q1".to_string()]);
        assert_eq!(normalized.approval_mode, "full_auto");
        assert_eq!(normalized.icon_color, "#aabbcc");
    }

    #[test]
    fn canonicalize_preset_config_keeps_explicit_declared_skills_selected() {
        let preset = UserAgentPresetConfig {
            preset_id: "preset_demo".to_string(),
            revision: 2,
            name: "Demo Preset".to_string(),
            description: "desc".to_string(),
            system_prompt: "prompt".to_string(),
            model_name: Some("model-a".to_string()),
            icon_name: "spark".to_string(),
            icon_color: "#ABC".to_string(),
            sandbox_container_id: 2,
            tool_names: vec!["read_file".to_string()],
            declared_tool_names: vec!["read_file".to_string()],
            declared_skill_names: vec!["planner".to_string()],
            preset_questions: Vec::new(),
            approval_mode: "full_auto".to_string(),
            status: "active".to_string(),
        };

        let normalized =
            canonicalize_preset_config(&preset, "preset_demo", &sample_skill_keys()).unwrap();
        assert_eq!(
            normalized.tool_names,
            vec!["read_file".to_string(), "planner".to_string()]
        );
        assert_eq!(
            normalized.declared_tool_names,
            vec!["read_file".to_string()]
        );
        assert_eq!(normalized.declared_skill_names, vec!["planner".to_string()]);
    }

    #[test]
    fn canonicalize_preset_config_preserves_declared_skills_when_selected_list_also_contains_them()
    {
        let preset = UserAgentPresetConfig {
            preset_id: "preset_demo".to_string(),
            revision: 2,
            name: "Demo Preset".to_string(),
            description: "desc".to_string(),
            system_prompt: "prompt".to_string(),
            model_name: Some("model-a".to_string()),
            icon_name: "spark".to_string(),
            icon_color: "#ABC".to_string(),
            sandbox_container_id: 2,
            tool_names: vec!["read_file".to_string(), "planner".to_string()],
            declared_tool_names: vec!["read_file".to_string()],
            declared_skill_names: vec!["planner".to_string()],
            preset_questions: Vec::new(),
            approval_mode: "full_auto".to_string(),
            status: "active".to_string(),
        };

        let normalized =
            canonicalize_preset_config(&preset, "preset_demo", &sample_skill_keys()).unwrap();
        assert_eq!(
            normalized.tool_names,
            vec!["read_file".to_string(), "planner".to_string()]
        );
        assert_eq!(
            normalized.declared_tool_names,
            vec!["read_file".to_string()]
        );
        assert_eq!(normalized.declared_skill_names, vec!["planner".to_string()]);
    }

    #[test]
    fn canonicalize_default_agent_config_round_trips_through_worker_card() {
        let config = DefaultAgentConfig {
            name: "Default Agent".to_string(),
            description: "desc".to_string(),
            system_prompt: "prompt".to_string(),
            tool_names: vec!["planner".to_string(), "read_file".to_string()],
            declared_tool_names: vec!["planner".to_string()],
            declared_skill_names: Vec::new(),
            approval_mode: "suggest".to_string(),
            status: "active".to_string(),
            sandbox_container_id: 0,
            created_at: 1.0,
            updated_at: 2.0,
            ..Default::default()
        };
        let normalized = canonicalize_default_agent_config(&config, &sample_skill_keys());
        assert_eq!(
            normalized.declared_tool_names,
            vec!["read_file".to_string()]
        );
        assert_eq!(normalized.declared_skill_names, vec!["planner".to_string()]);
        assert_eq!(normalized.sandbox_container_id, 1);
        assert_eq!(normalized.approval_mode, "suggest");
    }

    #[test]
    fn preset_update_from_config_preserves_structured_icon_payload() {
        let preset = UserAgentPresetConfig {
            preset_id: String::new(),
            revision: 1,
            name: "Demo".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            model_name: None,
            icon_name: "spark".to_string(),
            icon_color: "#123456".to_string(),
            sandbox_container_id: 1,
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            approval_mode: "full_auto".to_string(),
            status: "active".to_string(),
        };
        let update = preset_update_from_config(&preset, &HashSet::new()).unwrap();
        let (icon_name, icon_color) = normalize_preset_icon_parts(update.icon.as_deref());
        assert_eq!(icon_name, "spark");
        assert_eq!(icon_color, "#123456");
    }

    #[test]
    fn collect_configured_skill_names_scans_repo_skills() {
        let mut config = Config::default();
        config.skills.enabled.clear();
        config.skills.paths.clear();

        let skill_names = collect_configured_skill_names(&config);
        assert!(
            skill_names.contains("技能创建器"),
            "configured skill scan should include repo skill names"
        );
    }
}
