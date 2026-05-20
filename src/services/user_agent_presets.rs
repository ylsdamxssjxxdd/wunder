use crate::config::UserAgentPresetConfig;
use crate::services::agent_abilities::resolve_selected_declared_names;
use crate::services::default_tool_profile::curated_default_tool_names;
use crate::services::inner_visible::WorkerCardRecordUpdate;
use crate::services::preset_worker_cards;
use crate::services::worker_card_settings::{
    self, canonicalize_preset_config, collect_configured_skill_names, collect_context_skill_names,
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
const PRESET_META_PREFIX: &str = "user_agent_presets_v1:";
const PRESET_CONTAINER_META_PREFIX: &str = "user_agent_presets_container_v1:";

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
    pub preview_skill: bool,
    pub model_name: Option<String>,
    pub icon: String,
    pub icon_name: String,
    pub icon_color: String,
    pub sandbox_container_id: i32,
    pub tool_names: Vec<String>,
    pub declared_tool_names: Vec<String>,
    pub declared_skill_names: Vec<String>,
    pub visible_unit_ids: Vec<String>,
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
    if let Some(explicit) = normalize_explicit_preset_id(raw_preset_id) {
        return explicit;
    }
    let stable_name = name.trim().to_lowercase();
    format!(
        "preset_{}",
        Uuid::new_v5(&Uuid::NAMESPACE_URL, stable_name.as_bytes()).simple()
    )
}

fn normalize_explicit_preset_id(raw_preset_id: &str) -> Option<String> {
    let cleaned = raw_preset_id.trim();
    if cleaned.is_empty() {
        return None;
    }
    if cleaned == "preset" {
        return None;
    }
    if cleaned.starts_with("preset_") {
        return Some(cleaned.to_string());
    }
    let suffix = cleaned.strip_prefix("agent_").unwrap_or(cleaned).trim();
    if suffix.is_empty() {
        None
    } else {
        Some(format!("preset_{suffix}"))
    }
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

pub fn filter_allowed_tools(values: &[String], allowed: &HashSet<String>) -> Vec<String> {
    values
        .iter()
        .filter(|name| allowed.contains(*name))
        .cloned()
        .collect()
}

pub fn build_requested_tool_names_for_sync(
    selected_tool_names: &[String],
    explicit_declared_tool_names: &[String],
    explicit_declared_skill_names: &[String],
    allowed_tool_names: &HashSet<String>,
) -> Vec<String> {
    let mut requested_tool_names = normalize_tool_list(selected_tool_names.to_vec());
    if requested_tool_names.is_empty() {
        requested_tool_names.extend(explicit_declared_tool_names.iter().cloned());
    }
    requested_tool_names.extend(explicit_declared_skill_names.iter().cloned());
    requested_tool_names = normalize_tool_list(requested_tool_names);
    if requested_tool_names.is_empty() {
        return curated_default_tool_names(allowed_tool_names);
    }
    filter_allowed_tools(
        &normalize_tool_list(requested_tool_names),
        allowed_tool_names,
    )
}

fn preset_from_config_with_skill_names(
    config: &UserAgentPresetConfig,
    skill_name_keys: &HashSet<String>,
) -> Option<PresetAgent> {
    let preset_id = resolve_preset_id(&config.preset_id, &config.name);
    let normalized = canonicalize_preset_config(config, &preset_id, skill_name_keys)?;
    let update = preset_update_from_config(&normalized, skill_name_keys)?;
    let icon = worker_card_settings::normalize_icon_payload(update.icon.as_deref());
    let (icon_name, icon_color) = worker_card_settings::normalize_preset_icon_parts(Some(&icon));
    Some(PresetAgent {
        preset_id,
        revision: normalized.revision.max(1),
        name: update.name,
        description: update.description,
        system_prompt: update.system_prompt,
        preview_skill: normalized.preview_skill,
        model_name: normalize_optional_model_name(update.model_name.as_deref()),
        icon,
        icon_name,
        icon_color,
        sandbox_container_id: normalize_sandbox_container_id(update.sandbox_container_id),
        tool_names: normalize_tool_list(update.tool_names),
        declared_tool_names: normalize_tool_list(update.declared_tool_names),
        declared_skill_names: normalize_tool_list(update.declared_skill_names),
        visible_unit_ids: normalized.visible_unit_ids.clone(),
        preset_questions: normalize_preset_questions(update.preset_questions),
        approval_mode: normalize_agent_approval_mode(Some(&update.approval_mode)),
        status: normalize_agent_status(Some(&normalized.status)),
    })
}

pub async fn configured_preset_agents(state: &AppState) -> Vec<PresetAgent> {
    let config = state.config_store.get().await;
    let skill_name_keys = collect_configured_skill_names(&config);
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

fn snapshot_from_record(
    record: &UserAgentRecord,
    skill_name_keys: &HashSet<String>,
) -> UserAgentPresetSnapshot {
    preset_snapshot_from_record(record, skill_name_keys)
}

fn build_target_snapshot_from_context(
    user: &UserAccountRecord,
    preset: &PresetAgent,
    context: &crate::user_access::UserToolContext,
) -> UserAgentPresetSnapshot {
    let allowed_tool_names = compute_allowed_tool_names(user, context);
    let skill_name_keys = collect_context_skill_names(context);
    let requested_tool_names = build_requested_tool_names_for_sync(
        &preset.tool_names,
        &preset.declared_tool_names,
        &preset.declared_skill_names,
        &allowed_tool_names,
    );
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
                preview_skill: preset.preview_skill,
                model_name: normalize_optional_model_name(preset.model_name.as_deref()),
                ability_items: Vec::new(),
                tool_names: requested_tool_names,
                declared_tool_names,
                declared_skill_names,
                visible_unit_ids: preset.visible_unit_ids.clone(),
                preset_questions: preset.preset_questions.clone(),
                approval_mode: preset.approval_mode.clone(),
                is_shared: false,
                icon: Some(preset.icon.clone()),
                hive_id: DEFAULT_HIVE_ID.to_string(),
                silent: false,
                prefer_mother: false,
                sandbox_container_id: preset.sandbox_container_id,
            },
            &skill_name_keys,
        ),
        normalize_optional_model_name(preset.model_name.as_deref()),
        &preset.status,
    )
}

pub async fn build_target_snapshot(
    state: &AppState,
    user: &UserAccountRecord,
    preset: &PresetAgent,
) -> UserAgentPresetSnapshot {
    let context = build_user_tool_context(state, &user.user_id).await;
    build_target_snapshot_from_context(user, preset, &context)
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

fn preset_meta_key(user_id: &str) -> String {
    format!("{PRESET_META_PREFIX}{user_id}")
}

fn preset_container_meta_key(user_id: &str) -> String {
    format!("{PRESET_CONTAINER_META_PREFIX}{user_id}")
}

fn preset_by_name(preset_agents: &[PresetAgent], name: &str) -> Option<PresetAgent> {
    let cleaned = name.trim();
    if cleaned.is_empty() {
        return None;
    }
    preset_agents
        .iter()
        .find(|preset| preset.name == cleaned)
        .cloned()
}

fn duplicate_preset_bound_agent_ids(
    records: &[UserAgentRecord],
    configured_preset_ids: &HashSet<String>,
) -> HashSet<String> {
    let mut duplicates_by_preset_id: HashMap<String, Vec<&UserAgentRecord>> = HashMap::new();
    for record in records {
        if normalize_hive_id(&record.hive_id) != DEFAULT_HIVE_ID {
            continue;
        }
        let Some(binding) = record.preset_binding.as_ref() else {
            continue;
        };
        if !configured_preset_ids.contains(&binding.preset_id) {
            continue;
        }
        duplicates_by_preset_id
            .entry(binding.preset_id.clone())
            .or_default()
            .push(record);
    }
    duplicates_by_preset_id
        .into_values()
        .filter(|items| items.len() > 1)
        .flat_map(|mut items| {
            items.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));
            items
                .into_iter()
                .skip(1)
                .map(|record| record.agent_id.clone())
        })
        .collect()
}

pub async fn ensure_user_preset_agents(state: &AppState, user: &UserAccountRecord) -> Result<bool> {
    state.user_store.ensure_default_hive(&user.user_id)?;
    let meta_key = preset_meta_key(&user.user_id);
    let container_meta_key = preset_container_meta_key(&user.user_id);
    let preset_agents = configured_preset_agents(state).await;
    let bootstrap_completed = state.user_store.get_meta(&meta_key)?.is_some();
    let configured_preset_ids = preset_agents
        .iter()
        .map(|preset| preset.preset_id.clone())
        .collect::<HashSet<_>>();
    let mut existing = state.user_store.list_user_agents(&user.user_id)?;

    let duplicate_ids = duplicate_preset_bound_agent_ids(&existing, &configured_preset_ids);
    if !duplicate_ids.is_empty() {
        for duplicate_id in &duplicate_ids {
            let _ = state
                .user_store
                .delete_user_agent(&user.user_id, duplicate_id);
        }
        existing.retain(|record| !duplicate_ids.contains(&record.agent_id));
    }

    let now = now_ts();
    let container_layout_seeded = state.user_store.get_meta(&container_meta_key)?.is_some();
    let mut target_by_preset_id = HashMap::new();
    let mut matched_preset_by_agent_id = HashMap::new();
    for preset in &preset_agents {
        if let Some(record) = find_preset_agent(&existing, preset) {
            matched_preset_by_agent_id.insert(record.agent_id.clone(), preset.clone());
            target_by_preset_id.insert(
                preset.preset_id.clone(),
                build_target_snapshot(state, user, preset).await,
            );
        }
    }

    let mut existing_mutated = false;
    for record in &existing {
        let mut updated = record.clone();
        let mut changed = false;
        let mut matched_preset = matched_preset_by_agent_id.get(&record.agent_id).cloned();
        if matched_preset.is_none() {
            matched_preset = preset_by_name(&preset_agents, updated.name.trim());
        }

        if !container_layout_seeded {
            if let Some(container_id) = matched_preset
                .as_ref()
                .map(|preset| preset.sandbox_container_id)
            {
                if updated.sandbox_container_id == 1 && updated.sandbox_container_id != container_id
                {
                    updated.sandbox_container_id = container_id;
                    changed = true;
                }
            }
        }

        if let Some(preset) = matched_preset.as_ref() {
            if updated.preset_binding.is_none() {
                let target = match target_by_preset_id.get(&preset.preset_id) {
                    Some(snapshot) => snapshot.clone(),
                    None => {
                        let snapshot = build_target_snapshot(state, user, preset).await;
                        target_by_preset_id.insert(preset.preset_id.clone(), snapshot.clone());
                        snapshot
                    }
                };
                updated.preset_binding = Some(build_binding(preset, &target));
                changed = true;
            }
        }

        if changed {
            updated.updated_at = now;
            state.user_store.upsert_user_agent(&updated)?;
            existing_mutated = true;
        }
    }

    if !container_layout_seeded {
        state.user_store.set_meta(&container_meta_key, "1")?;
    }
    if existing_mutated {
        existing = state.user_store.list_user_agents(&user.user_id)?;
    }

    let mut preset_agents_restored = false;
    for preset in &preset_agents {
        if find_preset_agent(&existing, preset).is_some() {
            continue;
        }
        let record = create_preset_agent_record(state, user, preset, now).await;
        state.user_store.upsert_user_agent(&record)?;
        preset_agents_restored = true;
    }

    let mut bootstrap_meta_written = false;
    if !bootstrap_completed && !preset_agents.is_empty() {
        state.user_store.set_meta(&meta_key, "1")?;
        bootstrap_meta_written = true;
    }

    let changed = existing_mutated
        || preset_agents_restored
        || bootstrap_meta_written
        || !container_layout_seeded
        || !duplicate_ids.is_empty();
    if changed {
        if let Err(err) = state.inner_visible.sync_user_state(&user.user_id).await {
            tracing::warn!(
                "failed to sync inner-visible preset state for {}: {err}",
                user.user_id
            );
        }
    }
    Ok(changed)
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
    compare_field!(preview_skill);
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
    sync_field!(preview_skill);
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
        preview_skill: target.preview_skill,
        model_name: target.model_name.clone(),
        ability_items: target.ability_items.clone(),
        tool_names: target.tool_names.clone(),
        declared_tool_names: target.declared_tool_names.clone(),
        declared_skill_names: target.declared_skill_names.clone(),
        visible_unit_ids: target.visible_unit_ids.clone(),
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
        silent: false,
        prefer_mother: false,
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
        let context = build_user_tool_context(state, &user.user_id).await;
        let skill_name_keys = collect_context_skill_names(&context);
        let target = build_target_snapshot_from_context(&user, preset, &context);
        let maybe_record = find_preset_agent(&agents, preset).cloned();
        let Some(mut record) = maybe_record else {
            summary.missing_users += 1;
            if !dry_run {
                let created = create_preset_agent_record(state, &user, preset, now_ts()).await;
                state.user_store.upsert_user_agent(&created)?;
                if let Err(err) = state.inner_visible.sync_user_state(&user.user_id).await {
                    tracing::warn!(
                        "failed to sync inner-visible preset state for {}: {err}",
                        user.user_id
                    );
                }
                summary.created_agents += 1;
            }
            continue;
        };

        summary.linked_users += 1;
        let current = snapshot_from_record(&record, &skill_name_keys);
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
            if let Err(err) = state.inner_visible.sync_user_state(&user.user_id).await {
                tracing::warn!(
                    "failed to sync inner-visible preset state for {}: {err}",
                    user.user_id
                );
            }
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

#[cfg(test)]
mod tests {
    use super::{resolve_preset_id, snapshot_from_record};
    use crate::schemas::AbilityKind;
    use crate::storage::UserAgentRecord;
    use std::collections::HashSet;

    #[test]
    fn snapshot_from_record_preserves_declared_skill_names_with_context_keys() {
        let record = UserAgentRecord {
            agent_id: "agent_snapshot_skill".to_string(),
            user_id: "user_snapshot_skill".to_string(),
            hive_id: "default".to_string(),
            name: "Snapshot Skill".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            preview_skill: false,
            model_name: None,
            tool_names: vec!["planner".to_string()],
            declared_tool_names: Vec::new(),
            declared_skill_names: vec!["planner".to_string()],
            visible_unit_ids: Vec::new(),
            ability_items: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
            created_at: 0.0,
            updated_at: 0.0,
            preset_binding: None,
            silent: false,
            prefer_mother: false,
        };
        let mut skill_name_keys = HashSet::new();
        skill_name_keys.insert("planner".to_string());

        let snapshot = snapshot_from_record(&record, &skill_name_keys);

        assert_eq!(snapshot.name, "Snapshot Skill");
        assert_eq!(snapshot.description, "");
        assert_eq!(snapshot.system_prompt, "");
        assert_eq!(snapshot.model_name, None);
        assert_eq!(snapshot.ability_items.len(), 1);
        assert_eq!(snapshot.ability_items[0].runtime_name, "planner");
        assert_eq!(snapshot.ability_items[0].kind, AbilityKind::Skill);
        assert_eq!(snapshot.tool_names, vec!["planner".to_string()]);
        assert!(snapshot.declared_tool_names.is_empty());
        assert_eq!(snapshot.declared_skill_names, vec!["planner".to_string()]);
        assert!(snapshot.preset_questions.is_empty());
        assert_eq!(snapshot.approval_mode, "full_auto");
        assert_eq!(snapshot.status, "active");
        assert_eq!(snapshot.icon, None);
        assert_eq!(snapshot.sandbox_container_id, 1);
    }

    #[test]
    fn resolve_preset_id_generates_stable_prefixed_id() {
        assert_eq!(
            resolve_preset_id("", "公文写作"),
            "preset_ba13fa8e3c9450ffa41a822f9cbe717a"
        );
        assert_eq!(
            resolve_preset_id("", "Policy Analysis / Draft"),
            "preset_b906e0f59742575587df537983651419"
        );
    }

    #[test]
    fn resolve_preset_id_normalizes_explicit_prefixes_to_preset_style() {
        assert_eq!(
            resolve_preset_id("agent_existing", "任意名称"),
            "preset_existing"
        );
        assert_eq!(
            resolve_preset_id("preset_existing", "任意名称"),
            "preset_existing"
        );
    }
}
