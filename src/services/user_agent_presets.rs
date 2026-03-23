use crate::config::UserAgentPresetConfig;
use crate::services::agent_abilities::resolve_selected_declared_names;
use crate::services::default_tool_profile::{
    curated_default_skill_names, curated_default_tool_names,
};
use crate::services::inner_visible::WorkerCardRecordUpdate;
use crate::services::preset_worker_cards;
use crate::services::worker_card_settings::{
    self, canonicalize_preset_config, collect_context_skill_names, collect_registry_skill_names,
    normalize_agent_approval_mode as shared_normalize_agent_approval_mode,
    normalize_agent_status as shared_normalize_agent_status,
    normalize_optional_model_name as shared_normalize_optional_model_name,
    normalize_preset_questions as shared_normalize_preset_questions,
    normalize_tool_list as shared_normalize_tool_list, preset_snapshot_from_record,
    preset_snapshot_from_update, preset_update_from_config,
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
fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    shared_normalize_tool_list(values)
}

pub fn normalize_preset_questions(values: Vec<String>) -> Vec<String> {
    shared_normalize_preset_questions(values)
}

pub fn normalize_optional_model_name(raw: Option<&str>) -> Option<String> {
    shared_normalize_optional_model_name(raw)
}

pub fn normalize_agent_approval_mode(raw: Option<&str>) -> String {
    shared_normalize_agent_approval_mode(raw)
}

pub fn normalize_agent_status(raw: Option<&str>) -> String {
    shared_normalize_agent_status(raw)
}

pub fn build_icon_payload(name: &str, color: &str) -> String {
    worker_card_settings::build_icon_payload(name, color)
}

pub fn filter_allowed_tools(values: &[String], allowed: &HashSet<String>) -> Vec<String> {
    values
        .iter()
        .filter(|name| allowed.contains(*name))
        .cloned()
        .collect()
}

fn preset_from_config_with_skill_names(
    config: &UserAgentPresetConfig,
    skill_name_keys: &HashSet<String>,
) -> Option<PresetAgent> {
    let preset_id = resolve_preset_id(&config.preset_id, &config.name);
    let normalized = canonicalize_preset_config(config, &preset_id, skill_name_keys)?;
    let update = preset_update_from_config(&normalized, skill_name_keys)?;
    let (icon_name, icon_color) =
        worker_card_settings::normalize_preset_icon_parts(update.icon.as_deref());
    Some(PresetAgent {
        preset_id,
        revision: normalized.revision.max(1),
        name: update.name,
        description: update.description,
        system_prompt: update.system_prompt,
        model_name: normalize_optional_model_name(update.model_name.as_deref()),
        icon_name,
        icon_color,
        sandbox_container_id: normalize_sandbox_container_id(update.sandbox_container_id),
        tool_names: normalize_tool_list(update.tool_names),
        declared_tool_names: normalize_tool_list(update.declared_tool_names),
        declared_skill_names: normalize_tool_list(update.declared_skill_names),
        preset_questions: normalize_preset_questions(update.preset_questions),
        approval_mode: normalize_agent_approval_mode(Some(&update.approval_mode)),
        status: normalize_agent_status(Some(&normalized.status)),
    })
}

pub async fn configured_preset_agents(state: &AppState) -> Vec<PresetAgent> {
    let config = state.config_store.get().await;
    let skill_name_keys = {
        let registry = state.skills.read().await;
        collect_registry_skill_names(&registry)
    };
    let configured =
        match preset_worker_cards::load_effective_preset_configs(&config, &skill_name_keys) {
            Ok(items) => items,
            Err(err) => {
                tracing::warn!("failed to load preset worker cards, falling back to config: {err}");
                config.user_agents.presets.clone()
            }
        };
    let mut seen_ids = HashSet::new();
    let mut presets = Vec::new();
    for item in &configured {
        let preset_id = resolve_preset_id(&item.preset_id, &item.name);
        let Some(normalized) = canonicalize_preset_config(item, &preset_id, &skill_name_keys)
        else {
            continue;
        };
        let Some(preset) = preset_from_config_with_skill_names(&normalized, &skill_name_keys)
        else {
            continue;
        };
        if seen_ids.insert(preset.preset_id.clone()) {
            presets.push(preset);
        }
    }
    presets
}

pub fn snapshot_from_record(record: &UserAgentRecord) -> UserAgentPresetSnapshot {
    preset_snapshot_from_record(record, &std::collections::HashSet::new())
}

pub async fn build_target_snapshot(
    state: &AppState,
    user: &UserAccountRecord,
    preset: &PresetAgent,
) -> UserAgentPresetSnapshot {
    let context = build_user_tool_context(state, &user.user_id).await;
    let allowed_tool_names = compute_allowed_tool_names(user, &context);
    let skill_name_keys = collect_context_skill_names(&context);
    let required_skill_names = curated_default_skill_names(&allowed_tool_names);
    let requested_tool_names = if preset.tool_names.is_empty() {
        curated_default_tool_names(&allowed_tool_names)
    } else {
        let mut merged = preset.tool_names.clone();
        merged.extend(required_skill_names);
        filter_allowed_tools(&normalize_tool_list(merged), &allowed_tool_names)
    };
    let (declared_tool_names, declared_skill_names) = resolve_selected_declared_names(
        &requested_tool_names,
        &preset.declared_tool_names,
        &preset.declared_skill_names,
        &skill_name_keys,
    );
    preset_snapshot_from_update(
        &worker_card_settings::canonicalize_worker_card_update(
            WorkerCardRecordUpdate {
                name: preset.name.clone(),
                description: preset.description.clone(),
                system_prompt: preset.system_prompt.clone(),
                model_name: normalize_optional_model_name(preset.model_name.as_deref()),
                ability_items: Vec::new(),
                tool_names: requested_tool_names,
                declared_tool_names,
                declared_skill_names,
                preset_questions: preset.preset_questions.clone(),
                approval_mode: preset.approval_mode.clone(),
                is_shared: false,
                icon: Some(build_icon_payload(&preset.icon_name, &preset.icon_color)),
                hive_id: DEFAULT_HIVE_ID.to_string(),
                sandbox_container_id: preset.sandbox_container_id,
            },
            &skill_name_keys,
        ),
        normalize_optional_model_name(preset.model_name.as_deref()),
        &preset.status,
    )
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
    compare_field!(ability_items);
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
    sync_field!(ability_items);
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
        ability_items: target.ability_items.clone(),
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
