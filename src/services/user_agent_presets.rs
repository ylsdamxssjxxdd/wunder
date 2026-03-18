use crate::config::UserAgentPresetConfig;
use crate::services::default_tool_profile::{
    curated_default_skill_names, curated_default_tool_names,
};
use crate::state::AppState;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, UserAccountRecord, UserAgentPresetBinding,
    UserAgentPresetSnapshot, UserAgentRecord, DEFAULT_HIVE_ID,
};
use crate::user_access::{build_user_tool_context, compute_allowed_tool_names};
use anyhow::{anyhow, Result};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const DEFAULT_AGENT_ACCESS_LEVEL: &str = "A";
const DEFAULT_AGENT_APPROVAL_MODE: &str = "full_auto";
const DEFAULT_AGENT_STATUS: &str = "active";
const PRESET_TEMPLATE_USER_ID: &str = "preset_template";

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[derive(Debug, Clone)]
pub struct PresetAgent {
    pub preset_id: String,
    pub revision: u64,
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub model_name: Option<String>,
    pub icon_name: String,
    pub icon_color: String,
    pub sandbox_container_id: i32,
    pub tool_names: Vec<String>,
    pub declared_tool_names: Vec<String>,
    pub declared_skill_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub approval_mode: String,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetSyncMode {
    Safe,
    Force,
}

#[derive(Debug, Clone, Default)]
pub struct PresetSyncSummary {
    pub total_users: usize,
    pub linked_users: usize,
    pub missing_users: usize,
    pub up_to_date_agents: usize,
    pub stale_agents: usize,
    pub safe_update_agents: usize,
    pub overridden_agents: usize,
    pub force_update_agents: usize,
    pub created_agents: usize,
    pub updated_agents: usize,
    pub rebound_agents: usize,
}

pub fn resolve_preset_id(raw_preset_id: &str, name: &str) -> String {
    let cleaned = raw_preset_id.trim();
    if !cleaned.is_empty() {
        return cleaned.to_string();
    }
    let stable_name = name.trim().to_lowercase();
    format!(
        "preset_{}",
        Uuid::new_v5(&Uuid::NAMESPACE_URL, stable_name.as_bytes()).simple()
    )
}

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

pub fn filter_allowed_tools(values: &[String], allowed: &HashSet<String>) -> Vec<String> {
    values
        .iter()
        .filter(|name| allowed.contains(*name))
        .cloned()
        .collect()
}

pub fn preset_from_config(config: &UserAgentPresetConfig) -> Option<PresetAgent> {
    let name = config.name.trim();
    if name.is_empty() {
        return None;
    }
    let icon_name = if config.icon_name.trim().is_empty() {
        "spark".to_string()
    } else {
        config.icon_name.trim().to_string()
    };
    let icon_color = if config.icon_color.trim().is_empty() {
        "#94a3b8".to_string()
    } else {
        config.icon_color.trim().to_string()
    };
    Some(PresetAgent {
        preset_id: resolve_preset_id(&config.preset_id, name),
        revision: config.revision.max(1),
        name: name.to_string(),
        description: config.description.trim().to_string(),
        system_prompt: config.system_prompt.trim().to_string(),
        model_name: normalize_optional_model_name(config.model_name.as_deref()),
        icon_name,
        icon_color,
        sandbox_container_id: normalize_sandbox_container_id(config.sandbox_container_id),
        tool_names: normalize_tool_list(config.tool_names.clone()),
        declared_tool_names: normalize_tool_list(config.declared_tool_names.clone()),
        declared_skill_names: normalize_tool_list(config.declared_skill_names.clone()),
        preset_questions: normalize_preset_questions(config.preset_questions.clone()),
        approval_mode: normalize_agent_approval_mode(Some(&config.approval_mode)),
        status: normalize_agent_status(Some(&config.status)),
    })
}

pub async fn configured_preset_agents(state: &AppState) -> Vec<PresetAgent> {
    let config = state.config_store.get().await;
    let template_agents = state
        .user_store
        .list_user_agents(PRESET_TEMPLATE_USER_ID)
        .unwrap_or_default();
    let mut seen_ids = HashSet::new();
    let mut presets = Vec::new();
    for item in &config.user_agents.presets {
        let Some(mut preset) = preset_from_config(item) else {
            continue;
        };
        if let Some(template_agent) = same_name_agent(&template_agents, &preset.name) {
            apply_template_agent_fields(&mut preset, template_agent);
        }
        if seen_ids.insert(preset.preset_id.clone()) {
            presets.push(preset);
        }
    }
    presets
}

pub fn snapshot_from_record(record: &UserAgentRecord) -> UserAgentPresetSnapshot {
    UserAgentPresetSnapshot {
        name: record.name.clone(),
        description: record.description.clone(),
        system_prompt: record.system_prompt.clone(),
        model_name: record.model_name.clone(),
        tool_names: normalize_tool_list(record.tool_names.clone()),
        declared_tool_names: normalize_tool_list(record.declared_tool_names.clone()),
        declared_skill_names: normalize_tool_list(record.declared_skill_names.clone()),
        preset_questions: normalize_preset_questions(record.preset_questions.clone()),
        approval_mode: normalize_agent_approval_mode(Some(&record.approval_mode)),
        status: normalize_agent_status(Some(&record.status)),
        icon: record.icon.clone(),
        sandbox_container_id: normalize_sandbox_container_id(record.sandbox_container_id),
    }
}

pub async fn build_target_snapshot(
    state: &AppState,
    user: &UserAccountRecord,
    preset: &PresetAgent,
) -> UserAgentPresetSnapshot {
    let context = build_user_tool_context(state, &user.user_id).await;
    let allowed_tool_names = compute_allowed_tool_names(user, &context);
    let required_skill_names = curated_default_skill_names(&allowed_tool_names);
    let requested_tool_names = if preset.tool_names.is_empty() {
        curated_default_tool_names(&allowed_tool_names)
    } else {
        let mut merged = preset.tool_names.clone();
        merged.extend(required_skill_names);
        filter_allowed_tools(&normalize_tool_list(merged), &allowed_tool_names)
    };
    UserAgentPresetSnapshot {
        name: preset.name.clone(),
        description: preset.description.clone(),
        system_prompt: preset.system_prompt.clone(),
        model_name: normalize_optional_model_name(preset.model_name.as_deref()),
        tool_names: requested_tool_names,
        declared_tool_names: preset.declared_tool_names.clone(),
        declared_skill_names: preset.declared_skill_names.clone(),
        preset_questions: preset.preset_questions.clone(),
        approval_mode: preset.approval_mode.clone(),
        status: preset.status.clone(),
        icon: Some(build_icon_payload(&preset.icon_name, &preset.icon_color)),
        sandbox_container_id: preset.sandbox_container_id,
    }
}

pub fn build_binding(
    preset: &PresetAgent,
    snapshot: &UserAgentPresetSnapshot,
) -> UserAgentPresetBinding {
    UserAgentPresetBinding {
        preset_id: preset.preset_id.clone(),
        preset_revision: preset.revision,
        last_applied: snapshot.clone(),
    }
}

fn same_name_agent<'a>(agents: &'a [UserAgentRecord], name: &str) -> Option<&'a UserAgentRecord> {
    let cleaned = name.trim();
    agents
        .iter()
        .filter(|record| normalize_hive_id(&record.hive_id) == DEFAULT_HIVE_ID)
        .filter(|record| record.name.trim() == cleaned)
        .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))
}

fn apply_template_agent_fields(preset: &mut PresetAgent, template: &UserAgentRecord) {
    preset.description = template.description.trim().to_string();
    preset.system_prompt = template.system_prompt.trim().to_string();
    preset.model_name = normalize_optional_model_name(template.model_name.as_deref());
    preset.sandbox_container_id = normalize_sandbox_container_id(template.sandbox_container_id);
    preset.tool_names = normalize_tool_list(template.tool_names.clone());
    preset.declared_tool_names = normalize_tool_list(template.declared_tool_names.clone());
    preset.declared_skill_names = normalize_tool_list(template.declared_skill_names.clone());
    preset.preset_questions = normalize_preset_questions(template.preset_questions.clone());
    preset.approval_mode = normalize_agent_approval_mode(Some(&template.approval_mode));
    preset.status = normalize_agent_status(Some(&template.status));
}

pub fn find_preset_agent<'a>(
    agents: &'a [UserAgentRecord],
    preset: &PresetAgent,
) -> Option<&'a UserAgentRecord> {
    agents
        .iter()
        .filter(|record| normalize_hive_id(&record.hive_id) == DEFAULT_HIVE_ID)
        .filter(|record| {
            record
                .preset_binding
                .as_ref()
                .map(|binding| binding.preset_id == preset.preset_id)
                .unwrap_or(false)
        })
        .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))
        .or_else(|| same_name_agent(agents, &preset.name))
}

fn baseline_snapshot(
    record: &UserAgentRecord,
    preset: &PresetAgent,
    target: &UserAgentPresetSnapshot,
) -> UserAgentPresetSnapshot {
    match record.preset_binding.as_ref() {
        Some(binding) if binding.preset_id == preset.preset_id => binding.last_applied.clone(),
        _ => target.clone(),
    }
}

#[derive(Debug, Default)]
struct SyncDecision {
    visible_diff: bool,
    safe_updates: usize,
    override_count: usize,
}

// Compare field-by-field so safe sync only touches values that still match the
// last applied template snapshot. Divergence means the user customized it.
fn plan_snapshot_sync(
    current: &UserAgentPresetSnapshot,
    baseline: &UserAgentPresetSnapshot,
    target: &UserAgentPresetSnapshot,
) -> SyncDecision {
    let mut decision = SyncDecision::default();
    macro_rules! compare_field {
        ($field:ident) => {
            if current.$field != target.$field {
                decision.visible_diff = true;
                if current.$field == baseline.$field {
                    decision.safe_updates += 1;
                } else {
                    decision.override_count += 1;
                }
            }
        };
    }
    compare_field!(name);
    compare_field!(description);
    compare_field!(system_prompt);
    compare_field!(model_name);
    compare_field!(tool_names);
    compare_field!(declared_tool_names);
    compare_field!(declared_skill_names);
    compare_field!(preset_questions);
    compare_field!(approval_mode);
    compare_field!(status);
    compare_field!(icon);
    compare_field!(sandbox_container_id);
    decision
}

fn apply_sync_mode(
    record: &mut UserAgentRecord,
    baseline: &UserAgentPresetSnapshot,
    target: &UserAgentPresetSnapshot,
    mode: PresetSyncMode,
) -> bool {
    let mut changed = false;
    macro_rules! sync_field {
        ($field:ident) => {
            if record.$field != target.$field {
                let should_apply =
                    matches!(mode, PresetSyncMode::Force) || record.$field == baseline.$field;
                if should_apply {
                    record.$field = target.$field.clone();
                    changed = true;
                }
            }
        };
    }
    sync_field!(name);
    sync_field!(description);
    sync_field!(system_prompt);
    sync_field!(model_name);
    sync_field!(tool_names);
    sync_field!(declared_tool_names);
    sync_field!(declared_skill_names);
    sync_field!(preset_questions);
    sync_field!(approval_mode);
    sync_field!(status);
    sync_field!(icon);
    if record.sandbox_container_id != target.sandbox_container_id {
        let should_apply = matches!(mode, PresetSyncMode::Force)
            || record.sandbox_container_id == baseline.sandbox_container_id;
        if should_apply {
            record.sandbox_container_id = target.sandbox_container_id;
            changed = true;
        }
    }
    changed
}

pub async fn create_preset_agent_record(
    state: &AppState,
    user: &UserAccountRecord,
    preset: &PresetAgent,
    now: f64,
) -> UserAgentRecord {
    let target = build_target_snapshot(state, user, preset).await;
    UserAgentRecord {
        agent_id: format!("agent_{}", Uuid::new_v4().simple()),
        user_id: user.user_id.clone(),
        hive_id: DEFAULT_HIVE_ID.to_string(),
        name: target.name.clone(),
        description: target.description.clone(),
        system_prompt: target.system_prompt.clone(),
        model_name: target.model_name.clone(),
        tool_names: target.tool_names.clone(),
        declared_tool_names: target.declared_tool_names.clone(),
        declared_skill_names: target.declared_skill_names.clone(),
        preset_questions: target.preset_questions.clone(),
        access_level: DEFAULT_AGENT_ACCESS_LEVEL.to_string(),
        approval_mode: target.approval_mode.clone(),
        is_shared: false,
        status: target.status.clone(),
        icon: target.icon.clone(),
        sandbox_container_id: target.sandbox_container_id,
        created_at: now,
        updated_at: now,
        preset_binding: Some(build_binding(preset, &target)),
    }
}

pub async fn sync_preset_across_users(
    state: &AppState,
    preset: &PresetAgent,
    mode: PresetSyncMode,
    unit_scope: Option<&[String]>,
    dry_run: bool,
) -> Result<PresetSyncSummary> {
    let (users, _) = state.user_store.list_users(None, unit_scope, 0, 0)?;
    let mut summary = PresetSyncSummary {
        total_users: users.len(),
        ..PresetSyncSummary::default()
    };
    for user in users {
        state.user_store.ensure_default_hive(&user.user_id)?;
        let agents = state.user_store.list_user_agents(&user.user_id)?;
        let target = build_target_snapshot(state, &user, preset).await;
        let maybe_record = find_preset_agent(&agents, preset).cloned();
        let Some(mut record) = maybe_record else {
            summary.missing_users += 1;
            if !dry_run {
                let created = create_preset_agent_record(state, &user, preset, now_ts()).await;
                state.user_store.upsert_user_agent(&created)?;
                summary.created_agents += 1;
            }
            continue;
        };

        summary.linked_users += 1;
        let current = snapshot_from_record(&record);
        let baseline = baseline_snapshot(&record, preset, &target);
        let decision = plan_snapshot_sync(&current, &baseline, &target);
        let binding_matches = record
            .preset_binding
            .as_ref()
            .map(|binding| {
                binding.preset_id == preset.preset_id && binding.preset_revision == preset.revision
            })
            .unwrap_or(false);

        if !decision.visible_diff && binding_matches {
            summary.up_to_date_agents += 1;
            continue;
        }

        summary.stale_agents += 1;
        if decision.safe_updates > 0 || !binding_matches || record.preset_binding.is_none() {
            summary.safe_update_agents += 1;
        }
        if decision.override_count > 0 {
            summary.overridden_agents += 1;
        }
        if decision.visible_diff || !binding_matches || record.preset_binding.is_none() {
            summary.force_update_agents += 1;
        }

        if dry_run {
            continue;
        }

        let applied = apply_sync_mode(&mut record, &baseline, &target, mode);
        let was_bound = record.preset_binding.is_some();
        record.preset_binding = Some(build_binding(preset, &target));
        if applied || !binding_matches || !was_bound {
            record.updated_at = now_ts();
            state.user_store.upsert_user_agent(&record)?;
            if applied {
                summary.updated_agents += 1;
            } else {
                summary.rebound_agents += 1;
            }
        }
    }
    Ok(summary)
}

pub async fn find_preset_by_id(state: &AppState, preset_id: &str) -> Result<PresetAgent> {
    let cleaned = preset_id.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("preset_id is required"));
    }
    configured_preset_agents(state)
        .await
        .into_iter()
        .find(|item| item.preset_id == cleaned)
        .ok_or_else(|| anyhow!("preset agent not found"))
}

pub fn configs_by_preset_id(
    items: &[UserAgentPresetConfig],
) -> HashMap<String, UserAgentPresetConfig> {
    let mut output = HashMap::new();
    for item in items {
        let preset_id = resolve_preset_id(&item.preset_id, &item.name);
        output.insert(preset_id, item.clone());
    }
    output
}
