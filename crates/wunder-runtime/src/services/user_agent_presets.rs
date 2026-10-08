use crate::config::{PresetCustomizable, UserAgentPresetConfig};
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
    normalize_sandbox_container_id, UserAccountRecord, UserAgentPresetBinding,
    UserAgentPresetSnapshot, UserAgentRecord,
};
use crate::user_access::{build_user_tool_context, compute_allowed_tool_names};
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const DEFAULT_AGENT_ACCESS_LEVEL: &str = "A";
const AGENT_STATUS_ACTIVE: &str = "active";
const AGENT_STATUS_ARCHIVED: &str = "archived";
/// Users per batched agent lookup while syncing a preset across the fleet.
const AGENT_BATCH_SIZE: usize = 400;

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
    /// Admin-declared customizable surface for users bound to this preset.
    pub customizable: PresetCustomizable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetSyncMode {
    Safe,
    Force,
}

#[derive(Debug, Clone, Default)]
pub struct PresetSyncSummary {
    /// Users whose instance was created, updated, or kept customized.
    pub affected_users: usize,
    /// Instances newly created for users without a bound instance.
    pub created_agents: usize,
    /// Instances whose preset-owned fields were written.
    pub updated_agents: usize,
    /// Instances where at least one user edit was preserved (`safe` mode only).
    pub skipped_customized: usize,
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
    let allowed_canonical: HashSet<String> = allowed
        .iter()
        .map(|name| crate::tools::resolve_tool_name(name.trim()))
        .filter(|name| !name.is_empty())
        .collect();
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw in values {
        let cleaned = raw.trim();
        if cleaned.is_empty() {
            continue;
        }
        let canonical = crate::tools::resolve_tool_name(cleaned);
        let name = if allowed_canonical.contains(&canonical) {
            canonical
        } else if allowed.contains(cleaned) {
            cleaned.to_string()
        } else {
            continue;
        };
        if seen.insert(name.clone()) {
            output.push(name);
        }
    }
    output
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
        customizable: normalized.customizable,
    })
}

pub fn configured_preset_agents_for_config(config: &crate::config::Config) -> Vec<PresetAgent> {
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

pub async fn configured_preset_agents(state: &AppState) -> Vec<PresetAgent> {
    let config = state.config_store.get().await;
    configured_preset_agents_for_config(&config)
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
        .filter(|record| record.name.trim() == cleaned)
        .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))
}

/// The `__default__` template row is not a user instance in the single-agent model.
pub fn is_default_agent_record(record: &UserAgentRecord) -> bool {
    record
        .agent_id
        .trim()
        .eq_ignore_ascii_case(crate::services::default_agent_protocol::DEFAULT_AGENT_ID_ALIAS)
}

pub fn agent_is_archived(record: &UserAgentRecord) -> bool {
    record
        .status
        .trim()
        .eq_ignore_ascii_case(AGENT_STATUS_ARCHIVED)
}

/// The user's single instance: the newest record carrying a `preset_binding`.
///
/// Exactly one record owns the binding (archiving clears it), so this is the
/// agent the user-side API and the admin preset panel both operate on.
pub fn bound_agent_record(agents: &[UserAgentRecord]) -> Option<&UserAgentRecord> {
    agents
        .iter()
        .filter(|record| !is_default_agent_record(record))
        .filter(|record| record.preset_binding.is_some())
        .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))
}

fn newest_active_agent(agents: &[UserAgentRecord]) -> Option<&UserAgentRecord> {
    agents
        .iter()
        .filter(|record| !is_default_agent_record(record))
        .filter(|record| !agent_is_archived(record))
        .max_by(|left, right| left.updated_at.total_cmp(&right.updated_at))
}

fn preset_by_name<'a>(preset_agents: &'a [PresetAgent], name: &str) -> Option<&'a PresetAgent> {
    let cleaned = name.trim();
    if cleaned.is_empty() {
        return None;
    }
    preset_agents.iter().find(|preset| preset.name == cleaned)
}

/// The user's unique agent instance plus the preset that governs it.
#[derive(Debug, Clone)]
pub struct UserAgentInstance {
    pub record: UserAgentRecord,
    /// Bound preset id (`None` for the legacy default agent).
    pub preset_id: Option<String>,
    pub preset_name: Option<String>,
    pub customizable: PresetCustomizable,
    /// True when a configured preset currently governs this instance.
    pub preset_governs: bool,
}

impl UserAgentInstance {
    /// Contract `preset_binding` payload for `/wunder/user/agent` (§12.2.1 3)).
    pub fn preset_binding_payload(&self) -> Value {
        match self.preset_id.as_deref() {
            Some(preset_id) => serde_json::json!({
                "preset_id": preset_id,
                "name": self.preset_name.clone().unwrap_or_default(),
            }),
            None => Value::Null,
        }
    }
}

/// Resolves the single instance without mutating storage.
///
/// Order: the preset-bound record, then the newest live agent (legacy or
/// pre-binding records), and finally the default agent template. Outside a
/// governing preset the user owns every field.
pub async fn resolve_user_agent_instance(
    state: &AppState,
    user: &UserAccountRecord,
) -> Result<UserAgentInstance> {
    let agents = state.user_store.list_user_agents(&user.user_id)?;
    if let Some(record) = user_instance_record(&agents) {
        let preset_id = record
            .preset_binding
            .as_ref()
            .map(|binding| binding.preset_id.clone());
        let preset = match preset_id.as_deref() {
            Some(preset_id) => configured_preset_agents(state)
                .await
                .into_iter()
                .find(|item| item.preset_id == preset_id),
            None => None,
        };
        return Ok(match preset {
            Some(preset) => UserAgentInstance {
                record: record.clone(),
                preset_id: Some(preset.preset_id),
                preset_name: Some(preset.name),
                customizable: preset.customizable,
                preset_governs: true,
            },
            // No live preset governs this record: the user owns its fields
            // instead of being locked out by a missing preset.
            None => UserAgentInstance {
                record: record.clone(),
                preset_id,
                preset_name: Some(record.name.clone()),
                customizable: PresetCustomizable::ALL,
                preset_governs: false,
            },
        });
    }
    let record = crate::services::default_agent_sync::load_effective_default_agent_record(
        state,
        &user.user_id,
    )
    .await?;
    Ok(UserAgentInstance {
        record,
        preset_id: None,
        preset_name: None,
        customizable: PresetCustomizable::ALL,
        preset_governs: false,
    })
}

fn sorted_tool_set(values: &[String]) -> Vec<String> {
    let mut items = normalize_tool_list(values.to_vec());
    items.sort();
    items
}

fn tool_selection_diverged(record: &UserAgentRecord, baseline: &UserAgentPresetSnapshot) -> bool {
    let stored = sorted_tool_set(&record.tool_names);
    let stored_declared = sorted_tool_set(&record.declared_tool_names);
    let baseline_selected = sorted_tool_set(&baseline.tool_names);
    let baseline_declared = sorted_tool_set(&baseline.declared_tool_names);
    stored != baseline_selected
        && stored != baseline_declared
        && stored_declared != baseline_declared
}

/// Customizable fields where the record diverged from the applied preset snapshot.
///
/// Keys follow the frozen contract `customizable` surface. `reasoning_effort`
/// has no persisted agent field (it is a per-session chat setting), so it is
/// never reported as customized.
pub fn customized_field_names(record: &UserAgentRecord) -> Vec<&'static str> {
    let Some(binding) = record.preset_binding.as_ref() else {
        return Vec::new();
    };
    let baseline = &binding.last_applied;
    let mut fields = Vec::new();
    if record.system_prompt.trim() != baseline.system_prompt.trim() {
        fields.push("system_prompt");
    }
    // `welcome` covers the agent's welcome content: greeting plus preset questions.
    if record.description.trim() != baseline.description.trim()
        || normalize_preset_questions(record.preset_questions.clone())
            != normalize_preset_questions(baseline.preset_questions.clone())
    {
        fields.push("welcome");
    }
    if normalize_optional_model_name(record.model_name.as_deref())
        != normalize_optional_model_name(baseline.model_name.as_deref())
    {
        fields.push("model_name");
    }
    if tool_selection_diverged(record, baseline) {
        fields.push("tool_names");
    }
    if normalize_agent_approval_mode(Some(&record.approval_mode))
        != normalize_agent_approval_mode(Some(&baseline.approval_mode))
    {
        fields.push("approval_mode");
    }
    fields
}

/// The record the admin lists report as the user's single instance: the bound
/// one when present, otherwise the newest live agent.
pub fn user_instance_record(agents: &[UserAgentRecord]) -> Option<&UserAgentRecord> {
    bound_agent_record(agents).or_else(|| newest_active_agent(agents))
}

/// Writes the admin user-list binding columns (`preset_id` / `agent_id` /
/// `customized_fields`) into an already serialized user item.
pub fn insert_binding_fields(
    map: &mut serde_json::Map<String, Value>,
    record: Option<&UserAgentRecord>,
) {
    let preset_id = record
        .and_then(|record| record.preset_binding.as_ref())
        .map(|binding| binding.preset_id.clone());
    map.insert(
        "preset_id".to_string(),
        preset_id.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert(
        "agent_id".to_string(),
        record
            .map(|record| Value::String(record.agent_id.clone()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "customized_fields".to_string(),
        Value::Array(
            record
                .map(customized_field_names)
                .unwrap_or_default()
                .into_iter()
                .map(|field| Value::String(field.to_string()))
                .collect(),
        ),
    );
}

/// `user_id → 保留实例 / 归档实例` manifest emitted whenever convergence archives.
fn log_convergence(user_id: &str, kept_agent_id: &str, archived_agent_ids: &[String]) {
    if archived_agent_ids.is_empty() {
        return;
    }
    tracing::info!(
        user_id = %user_id,
        kept_agent_id = %kept_agent_id,
        archived_agent_ids = ?archived_agent_ids,
        "single-agent convergence archived legacy instances (sessions retained)"
    );
}

/// Ensures the user owns exactly one agent instance, bound to one preset.
///
/// Binding source: the user's existing `preset_binding.preset_id`, falling back
/// to the first configured preset. Every other historical instance is archived
/// (read-only, sessions retained) instead of being deleted; the resulting
/// `user_id → kept / archived` manifest is logged.
pub async fn ensure_user_agent(state: &AppState, user: &UserAccountRecord) -> Result<bool> {
    let user_id = user.user_id.trim();
    if user_id.is_empty() {
        return Ok(false);
    }
    let presets = configured_preset_agents(state).await;
    if presets.is_empty() {
        return Ok(false);
    }
    let existing = state.user_store.list_user_agents(user_id)?;
    let bound = bound_agent_record(&existing);
    let bound_preset_id = bound
        .and_then(|record| record.preset_binding.as_ref())
        .map(|binding| binding.preset_id.clone());
    let Some(preset) = bound_preset_id
        .as_deref()
        .and_then(|preset_id| presets.iter().find(|item| item.preset_id == preset_id))
        .or_else(|| presets.first())
        .cloned()
    else {
        return Ok(false);
    };

    let keep_agent_id = bound
        .map(|record| record.agent_id.clone())
        .or_else(|| same_name_agent(&existing, &preset.name).map(|record| record.agent_id.clone()))
        .or_else(|| newest_active_agent(&existing).map(|record| record.agent_id.clone()));

    let now = now_ts();
    let mut changed = false;
    let kept_agent_id = match keep_agent_id {
        Some(agent_id) => {
            let Some(record) = existing.iter().find(|item| item.agent_id == agent_id) else {
                return Ok(false);
            };
            let mut updated = record.clone();
            let mut record_changed = false;
            let binding_matches = updated
                .preset_binding
                .as_ref()
                .map(|binding| binding.preset_id == preset.preset_id)
                .unwrap_or(false);
            if !binding_matches {
                // Rebind without touching preset-owned fields: the record keeps
                // its current values as the sync baseline so a later `safe` sync
                // can still converge it onto the preset.
                let baseline = record_snapshot(state, user, &updated).await;
                updated.preset_binding = Some(build_binding(&preset, &baseline));
                record_changed = true;
            }
            if agent_is_archived(&updated) {
                updated.status = AGENT_STATUS_ACTIVE.to_string();
                record_changed = true;
            }
            if record_changed {
                updated.updated_at = now;
                state.user_store.upsert_user_agent(&updated)?;
                changed = true;
            }
            updated.agent_id
        }
        None => {
            let record = create_preset_agent_record(state, user, &preset, now).await;
            state.user_store.upsert_user_agent(&record)?;
            changed = true;
            record.agent_id
        }
    };

    let mut archived_ids = Vec::new();
    for record in &existing {
        if record.agent_id == kept_agent_id || is_default_agent_record(record) {
            continue;
        }
        if agent_is_archived(record) && record.preset_binding.is_none() {
            continue;
        }
        let mut archived = record.clone();
        archived.status = AGENT_STATUS_ARCHIVED.to_string();
        archived.preset_binding = None;
        archived.updated_at = now;
        state.user_store.upsert_user_agent(&archived)?;
        archived_ids.push(archived.agent_id);
        changed = true;
    }
    log_convergence(user_id, &kept_agent_id, &archived_ids);

    if changed {
        if let Err(err) = state.inner_visible.sync_user_state(user_id).await {
            tracing::warn!(
                "failed to sync inner-visible preset state for {}: {err}",
                user.user_id
            );
        }
    }
    Ok(changed)
}

async fn record_snapshot(
    state: &AppState,
    user: &UserAccountRecord,
    record: &UserAgentRecord,
) -> UserAgentPresetSnapshot {
    let context = build_user_tool_context(state, &user.user_id).await;
    let skill_name_keys = collect_context_skill_names(&context);
    snapshot_from_record(record, &skill_name_keys)
}

/// Result of a bind / rebind / unbind batch (`POST /admin/preset_agents/bindings`).
#[derive(Debug, Clone, Default)]
pub struct PresetBindOutcome {
    pub affected_users: usize,
    pub created_agents: usize,
    pub rebound_agents: usize,
}

/// Ensures each listed user has exactly one instance bound to `preset`.
///
/// `require_current_preset_id` implements `action=unbind`（解绑即迁移）: only
/// users currently bound to that preset migrate, everyone else is skipped.
pub async fn apply_preset_binding(
    state: &AppState,
    preset: &PresetAgent,
    user_ids: &[String],
    require_current_preset_id: Option<&str>,
) -> Result<PresetBindOutcome> {
    let mut outcome = PresetBindOutcome::default();
    let now = now_ts();
    for raw_user_id in user_ids {
        let user_id = raw_user_id.trim();
        if user_id.is_empty() {
            continue;
        }
        let Some(user) = state.user_store.get_user_by_id(user_id)? else {
            continue;
        };
        let existing = state.user_store.list_user_agents(user_id)?;
        let bound = bound_agent_record(&existing).cloned();
        if let Some(required) = require_current_preset_id {
            let matches = bound
                .as_ref()
                .and_then(|record| record.preset_binding.as_ref())
                .map(|binding| binding.preset_id == required)
                .unwrap_or(false);
            if !matches {
                continue;
            }
        }
        let same_preset = bound
            .as_ref()
            .and_then(|record| record.preset_binding.as_ref())
            .map(|binding| binding.preset_id == preset.preset_id)
            .unwrap_or(false);

        let kept_agent_id = match bound.as_ref() {
            Some(record) if same_preset => {
                // Idempotent: refresh the applied revision, leave user edits alone.
                let mut updated = record.clone();
                let mut record_changed = false;
                let binding_stale = updated
                    .preset_binding
                    .as_ref()
                    .map(|binding| binding.preset_revision != preset.revision)
                    .unwrap_or(true);
                if binding_stale {
                    let snapshot = build_target_snapshot(state, &user, preset).await;
                    updated.preset_binding = Some(build_binding(preset, &snapshot));
                    record_changed = true;
                }
                if agent_is_archived(&updated) {
                    updated.status = AGENT_STATUS_ACTIVE.to_string();
                    record_changed = true;
                }
                if record_changed {
                    updated.updated_at = now;
                    state.user_store.upsert_user_agent(&updated)?;
                }
                updated.agent_id
            }
            Some(record) => {
                // Rebind: the new preset now defines the instance.
                let mut updated = record.clone();
                let target = build_target_snapshot(state, &user, preset).await;
                let baseline = record
                    .preset_binding
                    .as_ref()
                    .map(|binding| binding.last_applied.clone())
                    .unwrap_or_else(|| target.clone());
                apply_sync_mode(&mut updated, &baseline, &target, PresetSyncMode::Force);
                updated.preset_binding = Some(build_binding(preset, &target));
                updated.status = AGENT_STATUS_ACTIVE.to_string();
                updated.updated_at = now;
                state.user_store.upsert_user_agent(&updated)?;
                outcome.rebound_agents += 1;
                updated.agent_id
            }
            None => {
                let record = create_preset_agent_record(state, &user, preset, now).await;
                state.user_store.upsert_user_agent(&record)?;
                outcome.created_agents += 1;
                record.agent_id
            }
        };

        let mut archived_ids = Vec::new();
        for record in &existing {
            if record.agent_id == kept_agent_id || is_default_agent_record(record) {
                continue;
            }
            if agent_is_archived(record) && record.preset_binding.is_none() {
                continue;
            }
            let mut archived = record.clone();
            archived.status = AGENT_STATUS_ARCHIVED.to_string();
            archived.preset_binding = None;
            archived.updated_at = now;
            state.user_store.upsert_user_agent(&archived)?;
            archived_ids.push(archived.agent_id);
        }
        log_convergence(user_id, &kept_agent_id, &archived_ids);

        if let Err(err) = state.inner_visible.sync_user_state(user_id).await {
            tracing::warn!(
                "failed to sync inner-visible preset state for {}: {err}",
                user.user_id
            );
        }
        outcome.affected_users += 1;
    }
    Ok(outcome)
}

pub async fn ensure_user_agent_bootstrap(
    state: &AppState,
    user: &UserAccountRecord,
) -> Result<bool> {
    let default_changed =
        crate::services::default_agent_sync::ensure_user_default_agent_from_template(state, user)
            .await?;
    let preset_changed = ensure_user_agent(state, user).await?;
    Ok(default_changed || preset_changed)
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

/// Syncs one preset onto every user whose single instance is bound to it.
///
/// `safe` only writes fields the user has not diverged from the applied
/// snapshot; `force` overwrites every preset-owned field but still only touches
/// the agent record — already-initialized threads keep their frozen system
/// prompt. `dry_run` counts the same numbers without writing anything.
pub async fn sync_preset_across_users(
    state: &AppState,
    preset: &PresetAgent,
    mode: PresetSyncMode,
    unit_scope: Option<&[String]>,
    dry_run: bool,
) -> Result<PresetSyncSummary> {
    let (users, _) = state.user_store.list_users(None, unit_scope, 0, 0)?;
    let user_ids = users
        .iter()
        .map(|user| user.user_id.clone())
        .collect::<Vec<_>>();
    let mut agents_by_user: HashMap<String, Vec<UserAgentRecord>> = HashMap::new();
    for chunk in user_ids.chunks(AGENT_BATCH_SIZE) {
        for record in state.user_store.list_user_agents_for_users(chunk)? {
            agents_by_user
                .entry(record.user_id.clone())
                .or_default()
                .push(record);
        }
    }

    let mut summary = PresetSyncSummary::default();
    for user in users {
        let agents = agents_by_user.remove(&user.user_id).unwrap_or_default();
        let context = build_user_tool_context(state, &user.user_id).await;
        let skill_name_keys = collect_context_skill_names(&context);
        let target = build_target_snapshot_from_context(&user, preset, &context);
        let Some(record) = bound_agent_record(&agents).cloned() else {
            // No instance at all: this user gains one.
            summary.affected_users += 1;
            summary.created_agents += 1;
            if !dry_run {
                let created = create_preset_agent_record(state, &user, preset, now_ts()).await;
                state.user_store.upsert_user_agent(&created)?;
                if let Err(err) = state.inner_visible.sync_user_state(&user.user_id).await {
                    tracing::warn!(
                        "failed to sync inner-visible preset state for {}: {err}",
                        user.user_id
                    );
                }
            }
            continue;
        };
        let binding = record.preset_binding.as_ref().cloned();
        let binding_matches = binding
            .as_ref()
            .map(|item| item.preset_id == preset.preset_id)
            .unwrap_or(false);
        if !binding_matches {
            // Bound to another preset: not governed by this sync.
            continue;
        }

        let current = snapshot_from_record(&record, &skill_name_keys);
        let baseline = binding
            .as_ref()
            .map(|item| item.last_applied.clone())
            .unwrap_or_else(|| target.clone());
        let decision = plan_snapshot_sync(&current, &baseline, &target);
        let revision_matches = binding
            .as_ref()
            .map(|item| item.preset_revision == preset.revision)
            .unwrap_or(false);
        if !decision.visible_diff && revision_matches {
            continue;
        }

        let mut touched = false;
        // A field only gets written when it is safe (`safe`) or forced.
        let would_write = matches!(mode, PresetSyncMode::Force) && decision.visible_diff
            || matches!(mode, PresetSyncMode::Safe) && decision.safe_updates > 0;
        if would_write {
            summary.updated_agents += 1;
            touched = true;
        }
        if matches!(mode, PresetSyncMode::Safe) && decision.override_count > 0 {
            summary.skipped_customized += 1;
            touched = true;
        }
        if touched {
            summary.affected_users += 1;
        }
        if dry_run {
            continue;
        }

        let mut updated = record.clone();
        let applied = apply_sync_mode(&mut updated, &baseline, &target, mode);
        updated.preset_binding = Some(build_binding(preset, &target));
        if applied || !revision_matches {
            updated.updated_at = now_ts();
            state.user_store.upsert_user_agent(&updated)?;
            if let Err(err) = state.inner_visible.sync_user_state(&user.user_id).await {
                tracing::warn!(
                    "failed to sync inner-visible preset state for {}: {err}",
                    user.user_id
                );
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
