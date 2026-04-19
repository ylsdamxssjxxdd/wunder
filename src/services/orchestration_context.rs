use crate::config::Config;
use crate::services::attachment::sanitize_filename_stem;
use crate::prompting::read_prompt_template;
use crate::storage::{AgentThreadRecord, ChatSessionRecord, StorageBackend, UserAgentRecord};
use crate::workspace::WorkspaceManager;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

const SESSION_META_PREFIX: &str = "orchestration_session:";
const HIVE_STATE_META_PREFIX: &str = "orchestration_hive_state:";
const MEMBER_BINDING_META_PREFIX: &str = "orchestration_member_binding:";
const HISTORY_META_PREFIX: &str = "orchestration_history:";
const ROUND_STATE_META_PREFIX: &str = "orchestration_round_state:";
const SITUATION_FILE_NAME: &str = "situation.txt";

pub const ORCHESTRATION_MODE: &str = "orchestration";
pub const ORCHESTRATION_THREAD_LOCKED_CODE: &str = "ORCHESTRATION_THREAD_LOCKED";
pub const ORCHESTRATION_HISTORY_STATUS_ACTIVE: &str = "active";
pub const ORCHESTRATION_HISTORY_STATUS_CLOSED: &str = "closed";

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OrchestrationSessionContext {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub round_index: i64,
    #[serde(default)]
    pub mother_agent_id: String,
}

#[derive(Debug, Clone)]
pub struct OrchestrationDispatchContext {
    pub round_index: i64,
    pub situation: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OrchestrationHiveState {
    #[serde(default)]
    pub orchestration_id: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub mother_agent_id: String,
    #[serde(default)]
    pub mother_agent_name: String,
    #[serde(default)]
    pub mother_session_id: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub entered_at: f64,
    #[serde(default)]
    pub updated_at: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OrchestrationMemberBinding {
    #[serde(default)]
    pub orchestration_id: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub agent_name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub created_at: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OrchestrationHistoryRecord {
    #[serde(default)]
    pub orchestration_id: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub mother_agent_id: String,
    #[serde(default)]
    pub mother_agent_name: String,
    #[serde(default)]
    pub mother_session_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub latest_round_index: i64,
    #[serde(default)]
    pub entered_at: f64,
    #[serde(default)]
    pub updated_at: f64,
    #[serde(default)]
    pub exited_at: f64,
    #[serde(default)]
    pub restored_at: f64,
    #[serde(default)]
    pub parent_orchestration_id: String,
    #[serde(default)]
    pub branch_root_orchestration_id: String,
    #[serde(default)]
    pub branch_from_round_index: i64,
    #[serde(default)]
    pub branch_depth: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OrchestrationRoundRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub situation: String,
    #[serde(default)]
    pub user_message: String,
    #[serde(default)]
    pub created_at: f64,
    #[serde(default)]
    pub finalized_at: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OrchestrationSuppressedMessageRange {
    #[serde(default)]
    pub start_at: f64,
    #[serde(default)]
    pub end_at: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OrchestrationRoundState {
    #[serde(default)]
    pub orchestration_id: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub rounds: Vec<OrchestrationRoundRecord>,
    #[serde(default)]
    pub suppressed_message_ranges: Vec<OrchestrationSuppressedMessageRange>,
    #[serde(default)]
    pub updated_at: f64,
}

pub fn session_meta_key(user_id: &str, session_id: &str) -> String {
    format!(
        "{SESSION_META_PREFIX}{}:{}",
        user_id.trim(),
        session_id.trim()
    )
}

pub fn hive_state_meta_key(user_id: &str, group_id: &str) -> String {
    format!(
        "{HIVE_STATE_META_PREFIX}{}:{}",
        user_id.trim(),
        group_id.trim()
    )
}

pub fn member_binding_meta_key(orchestration_id: &str, agent_id: &str) -> String {
    format!(
        "{MEMBER_BINDING_META_PREFIX}{}:{}",
        orchestration_id.trim(),
        agent_id.trim()
    )
}

pub fn history_meta_key(user_id: &str, group_id: &str, orchestration_id: &str) -> String {
    format!(
        "{HISTORY_META_PREFIX}{}:{}:{}",
        user_id.trim(),
        group_id.trim(),
        orchestration_id.trim()
    )
}

pub fn round_state_meta_key(user_id: &str, orchestration_id: &str) -> String {
    format!(
        "{ROUND_STATE_META_PREFIX}{}:{}",
        user_id.trim(),
        orchestration_id.trim()
    )
}

fn history_meta_prefix(user_id: &str, group_id: &str) -> String {
    format!("{HISTORY_META_PREFIX}{}:{}:", user_id.trim(), group_id.trim())
}

pub fn normalize_round_index(round_index: i64) -> i64 {
    round_index.max(1)
}

pub fn round_dir_name(round_index: i64) -> String {
    format!("round_{:04}", normalize_round_index(round_index))
}

pub fn round_id(round_index: i64) -> String {
    round_dir_name(round_index)
}

pub fn situation_path(run_id: &str, round_index: i64) -> String {
    [
        "orchestration".to_string(),
        run_id.trim().to_string(),
        round_dir_name(round_index),
        SITUATION_FILE_NAME.to_string(),
    ]
    .into_iter()
    .filter(|item| !item.trim().is_empty())
    .collect::<Vec<_>>()
    .join("/")
}

pub fn orchestration_agent_artifact_dir_name(agent_name: &str, fallback_agent_id: &str) -> String {
    let cleaned_name = sanitize_filename_stem(agent_name.trim());
    if !cleaned_name.is_empty() {
        return cleaned_name;
    }
    let cleaned_fallback = sanitize_filename_stem(fallback_agent_id.trim());
    if !cleaned_fallback.is_empty() {
        return cleaned_fallback;
    }
    "worker".to_string()
}

pub fn prompt_agent_artifact_path(
    round_index: i64,
    agent_name: &str,
    fallback_agent_id: &str,
) -> String {
    [
        round_dir_name(round_index),
        orchestration_agent_artifact_dir_name(agent_name, fallback_agent_id),
    ]
        .into_iter()
        .filter(|item| !item.trim().is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn clear_session_context(
    storage: &dyn StorageBackend,
    user_id: &str,
    session_id: &str,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_session_id = session_id.trim();
    if cleaned_user_id.is_empty() || cleaned_session_id.is_empty() {
        return Ok(());
    }
    storage.delete_meta_prefix(&session_meta_key(cleaned_user_id, cleaned_session_id))?;
    Ok(())
}

pub fn persist_session_context(
    storage: &dyn StorageBackend,
    user_id: &str,
    session_id: &str,
    context: &OrchestrationSessionContext,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_session_id = session_id.trim();
    if cleaned_user_id.is_empty() || cleaned_session_id.is_empty() {
        return Ok(());
    }
    let mut normalized = context.clone();
    normalized.mode = normalized.mode.trim().to_string();
    normalized.run_id = normalized.run_id.trim().to_string();
    normalized.group_id = normalized.group_id.trim().to_string();
    normalized.role = normalized.role.trim().to_string();
    normalized.mother_agent_id = normalized.mother_agent_id.trim().to_string();
    normalized.round_index = normalize_round_index(normalized.round_index);
    storage.set_meta(
        &session_meta_key(cleaned_user_id, cleaned_session_id),
        &serde_json::to_string(&normalized)?,
    )?;
    Ok(())
}

pub fn load_session_context(
    storage: &dyn StorageBackend,
    user_id: &str,
    session_id: &str,
) -> Option<OrchestrationSessionContext> {
    let cleaned_user_id = user_id.trim();
    let cleaned_session_id = session_id.trim();
    if cleaned_user_id.is_empty() || cleaned_session_id.is_empty() {
        return None;
    }
    let raw = storage
        .get_meta(&session_meta_key(cleaned_user_id, cleaned_session_id))
        .ok()
        .flatten()?;
    let mut context = serde_json::from_str::<OrchestrationSessionContext>(raw.trim()).ok()?;
    context.mode = context.mode.trim().to_string();
    context.role = context.role.trim().to_string();
    context.run_id = context.run_id.trim().to_string();
    context.group_id = context.group_id.trim().to_string();
    context.mother_agent_id = context.mother_agent_id.trim().to_string();
    context.round_index = normalize_round_index(context.round_index);
    if context.mode != ORCHESTRATION_MODE || context.run_id.is_empty() || context.round_index <= 0 {
        return None;
    }
    Some(context)
}

pub fn persist_hive_state(
    storage: &dyn StorageBackend,
    user_id: &str,
    state: &OrchestrationHiveState,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_group_id = state.group_id.trim();
    if cleaned_user_id.is_empty() || cleaned_group_id.is_empty() {
        return Ok(());
    }
    let mut normalized = state.clone();
    normalized.orchestration_id = normalized.orchestration_id.trim().to_string();
    normalized.run_id = normalized.run_id.trim().to_string();
    normalized.group_id = cleaned_group_id.to_string();
    normalized.mother_agent_id = normalized.mother_agent_id.trim().to_string();
    normalized.mother_agent_name = normalized.mother_agent_name.trim().to_string();
    normalized.mother_session_id = normalized.mother_session_id.trim().to_string();
    storage.set_meta(
        &hive_state_meta_key(cleaned_user_id, cleaned_group_id),
        &serde_json::to_string(&normalized)?,
    )?;
    Ok(())
}

pub fn load_hive_state(
    storage: &dyn StorageBackend,
    user_id: &str,
    group_id: &str,
) -> Option<OrchestrationHiveState> {
    let cleaned_user_id = user_id.trim();
    let cleaned_group_id = group_id.trim();
    if cleaned_user_id.is_empty() || cleaned_group_id.is_empty() {
        return None;
    }
    let raw = storage
        .get_meta(&hive_state_meta_key(cleaned_user_id, cleaned_group_id))
        .ok()
        .flatten()?;
    let mut state = serde_json::from_str::<OrchestrationHiveState>(raw.trim()).ok()?;
    state.orchestration_id = state.orchestration_id.trim().to_string();
    state.run_id = state.run_id.trim().to_string();
    state.group_id = state.group_id.trim().to_string();
    state.mother_agent_id = state.mother_agent_id.trim().to_string();
    state.mother_agent_name = state.mother_agent_name.trim().to_string();
    state.mother_session_id = state.mother_session_id.trim().to_string();
    if !state.active
        || state.orchestration_id.is_empty()
        || state.run_id.is_empty()
        || state.group_id.is_empty()
        || state.mother_agent_id.is_empty()
        || state.mother_session_id.is_empty()
    {
        return None;
    }
    Some(state)
}

pub fn clear_hive_state(
    storage: &dyn StorageBackend,
    user_id: &str,
    group_id: &str,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_group_id = group_id.trim();
    if cleaned_user_id.is_empty() || cleaned_group_id.is_empty() {
        return Ok(());
    }
    storage.delete_meta_prefix(&hive_state_meta_key(cleaned_user_id, cleaned_group_id))?;
    Ok(())
}

pub fn persist_member_binding(
    storage: &dyn StorageBackend,
    binding: &OrchestrationMemberBinding,
) -> Result<()> {
    let orchestration_id = binding.orchestration_id.trim();
    let agent_id = binding.agent_id.trim();
    if orchestration_id.is_empty() || agent_id.is_empty() {
        return Ok(());
    }
    let mut normalized = binding.clone();
    normalized.orchestration_id = orchestration_id.to_string();
    normalized.run_id = normalized.run_id.trim().to_string();
    normalized.group_id = normalized.group_id.trim().to_string();
    normalized.agent_id = agent_id.to_string();
    normalized.agent_name = normalized.agent_name.trim().to_string();
    normalized.role = normalized.role.trim().to_string();
    normalized.session_id = normalized.session_id.trim().to_string();
    normalized.title = normalized.title.trim().to_string();
    storage.set_meta(
        &member_binding_meta_key(orchestration_id, agent_id),
        &serde_json::to_string(&normalized)?,
    )?;
    Ok(())
}

pub fn load_member_binding(
    storage: &dyn StorageBackend,
    orchestration_id: &str,
    agent_id: &str,
) -> Option<OrchestrationMemberBinding> {
    let cleaned_orchestration_id = orchestration_id.trim();
    let cleaned_agent_id = agent_id.trim();
    if cleaned_orchestration_id.is_empty() || cleaned_agent_id.is_empty() {
        return None;
    }
    let raw = storage
        .get_meta(&member_binding_meta_key(cleaned_orchestration_id, cleaned_agent_id))
        .ok()
        .flatten()?;
    let mut binding = serde_json::from_str::<OrchestrationMemberBinding>(raw.trim()).ok()?;
    binding.orchestration_id = binding.orchestration_id.trim().to_string();
    binding.run_id = binding.run_id.trim().to_string();
    binding.group_id = binding.group_id.trim().to_string();
    binding.agent_id = binding.agent_id.trim().to_string();
    binding.agent_name = binding.agent_name.trim().to_string();
    binding.role = binding.role.trim().to_string();
    binding.session_id = binding.session_id.trim().to_string();
    binding.title = binding.title.trim().to_string();
    if binding.orchestration_id.is_empty()
        || binding.agent_id.is_empty()
        || binding.session_id.is_empty()
    {
        return None;
    }
    Some(binding)
}

pub fn list_member_bindings(
    storage: &dyn StorageBackend,
    orchestration_id: &str,
) -> Result<Vec<OrchestrationMemberBinding>> {
    let cleaned_orchestration_id = orchestration_id.trim();
    if cleaned_orchestration_id.is_empty() {
        return Ok(Vec::new());
    }
    let prefix = format!("{MEMBER_BINDING_META_PREFIX}{cleaned_orchestration_id}:");
    let mut items = storage
        .list_meta_prefix(&prefix)?
        .into_iter()
        .filter_map(|(_, value)| serde_json::from_str::<OrchestrationMemberBinding>(value.trim()).ok())
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
    Ok(items)
}

pub fn clear_member_bindings(storage: &dyn StorageBackend, orchestration_id: &str) -> Result<()> {
    let cleaned_orchestration_id = orchestration_id.trim();
    if cleaned_orchestration_id.is_empty() {
        return Ok(());
    }
    let prefix = format!("{MEMBER_BINDING_META_PREFIX}{cleaned_orchestration_id}:");
    storage.delete_meta_prefix(&prefix)?;
    Ok(())
}

pub fn build_history_record_from_state(
    state: &OrchestrationHiveState,
    status: &str,
    latest_round_index: i64,
) -> OrchestrationHistoryRecord {
    OrchestrationHistoryRecord {
        orchestration_id: state.orchestration_id.trim().to_string(),
        run_id: state.run_id.trim().to_string(),
        group_id: state.group_id.trim().to_string(),
        mother_agent_id: state.mother_agent_id.trim().to_string(),
        mother_agent_name: state.mother_agent_name.trim().to_string(),
        mother_session_id: state.mother_session_id.trim().to_string(),
        status: status.trim().to_string(),
        latest_round_index: normalize_round_index(latest_round_index),
        entered_at: state.entered_at,
        updated_at: state.updated_at,
        exited_at: 0.0,
        restored_at: 0.0,
        parent_orchestration_id: String::new(),
        branch_root_orchestration_id: state.orchestration_id.trim().to_string(),
        branch_from_round_index: 0,
        branch_depth: 0,
    }
}

pub fn build_branch_history_record_from_state(
    state: &OrchestrationHiveState,
    status: &str,
    latest_round_index: i64,
    source: &OrchestrationHistoryRecord,
    branch_from_round_index: i64,
) -> OrchestrationHistoryRecord {
    let mut record = build_history_record_from_state(state, status, latest_round_index);
    record.parent_orchestration_id = source.orchestration_id.trim().to_string();
    record.branch_root_orchestration_id = if source.branch_root_orchestration_id.trim().is_empty() {
        source.orchestration_id.trim().to_string()
    } else {
        source.branch_root_orchestration_id.trim().to_string()
    };
    record.branch_from_round_index = normalize_round_index(branch_from_round_index);
    record.branch_depth = source.branch_depth.max(0) + 1;
    record
}

pub fn persist_history_record(
    storage: &dyn StorageBackend,
    user_id: &str,
    record: &OrchestrationHistoryRecord,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_group_id = record.group_id.trim();
    let cleaned_orchestration_id = record.orchestration_id.trim();
    if cleaned_user_id.is_empty() || cleaned_group_id.is_empty() || cleaned_orchestration_id.is_empty()
    {
        return Ok(());
    }
    let mut normalized = record.clone();
    normalized.orchestration_id = cleaned_orchestration_id.to_string();
    normalized.run_id = normalized.run_id.trim().to_string();
    normalized.group_id = cleaned_group_id.to_string();
    normalized.mother_agent_id = normalized.mother_agent_id.trim().to_string();
    normalized.mother_agent_name = normalized.mother_agent_name.trim().to_string();
    normalized.mother_session_id = normalized.mother_session_id.trim().to_string();
    normalized.status = normalized.status.trim().to_string();
    normalized.latest_round_index = normalize_round_index(normalized.latest_round_index);
    normalized.parent_orchestration_id = normalized.parent_orchestration_id.trim().to_string();
    normalized.branch_root_orchestration_id = if normalized.branch_root_orchestration_id.trim().is_empty() {
        cleaned_orchestration_id.to_string()
    } else {
        normalized.branch_root_orchestration_id.trim().to_string()
    };
    normalized.branch_from_round_index = normalized.branch_from_round_index.max(0);
    normalized.branch_depth = normalized.branch_depth.max(0);
    storage.set_meta(
        &history_meta_key(cleaned_user_id, cleaned_group_id, cleaned_orchestration_id),
        &serde_json::to_string(&normalized)?,
    )?;
    Ok(())
}

pub fn load_history_record(
    storage: &dyn StorageBackend,
    user_id: &str,
    group_id: &str,
    orchestration_id: &str,
) -> Option<OrchestrationHistoryRecord> {
    let cleaned_user_id = user_id.trim();
    let cleaned_group_id = group_id.trim();
    let cleaned_orchestration_id = orchestration_id.trim();
    if cleaned_user_id.is_empty() || cleaned_group_id.is_empty() || cleaned_orchestration_id.is_empty()
    {
        return None;
    }
    let raw = storage
        .get_meta(&history_meta_key(
            cleaned_user_id,
            cleaned_group_id,
            cleaned_orchestration_id,
        ))
        .ok()
        .flatten()?;
    let mut record = serde_json::from_str::<OrchestrationHistoryRecord>(raw.trim()).ok()?;
    record.orchestration_id = record.orchestration_id.trim().to_string();
    record.run_id = record.run_id.trim().to_string();
    record.group_id = record.group_id.trim().to_string();
    record.mother_agent_id = record.mother_agent_id.trim().to_string();
    record.mother_agent_name = record.mother_agent_name.trim().to_string();
    record.mother_session_id = record.mother_session_id.trim().to_string();
    record.status = record.status.trim().to_string();
    record.latest_round_index = normalize_round_index(record.latest_round_index);
    record.parent_orchestration_id = record.parent_orchestration_id.trim().to_string();
    record.branch_root_orchestration_id = if record.branch_root_orchestration_id.trim().is_empty() {
        cleaned_orchestration_id.to_string()
    } else {
        record.branch_root_orchestration_id.trim().to_string()
    };
    record.branch_from_round_index = record.branch_from_round_index.max(0);
    record.branch_depth = record.branch_depth.max(0);
    if record.orchestration_id.is_empty()
        || record.group_id.is_empty()
        || record.run_id.is_empty()
        || record.mother_agent_id.is_empty()
        || record.mother_session_id.is_empty()
    {
        return None;
    }
    Some(record)
}

pub fn clear_history_record(
    storage: &dyn StorageBackend,
    user_id: &str,
    group_id: &str,
    orchestration_id: &str,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_group_id = group_id.trim();
    let cleaned_orchestration_id = orchestration_id.trim();
    if cleaned_user_id.is_empty() || cleaned_group_id.is_empty() || cleaned_orchestration_id.is_empty()
    {
        return Ok(());
    }
    storage.delete_meta_prefix(&history_meta_key(
        cleaned_user_id,
        cleaned_group_id,
        cleaned_orchestration_id,
    ))?;
    Ok(())
}

pub fn list_history_records(
    storage: &dyn StorageBackend,
    user_id: &str,
    group_id: &str,
) -> Result<Vec<OrchestrationHistoryRecord>> {
    let cleaned_user_id = user_id.trim();
    let cleaned_group_id = group_id.trim();
    if cleaned_user_id.is_empty() || cleaned_group_id.is_empty() {
        return Ok(Vec::new());
    }
    let mut items = storage
        .list_meta_prefix(&history_meta_prefix(cleaned_user_id, cleaned_group_id))?
        .into_iter()
        .filter_map(|(_, value)| serde_json::from_str::<OrchestrationHistoryRecord>(value.trim()).ok())
        .filter_map(|record| {
            load_history_record(
                storage,
                cleaned_user_id,
                cleaned_group_id,
                &record.orchestration_id,
            )
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .updated_at
            .partial_cmp(&left.updated_at)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.orchestration_id.cmp(&right.orchestration_id))
    });
    Ok(items)
}

pub fn normalize_round_record(record: &OrchestrationRoundRecord) -> OrchestrationRoundRecord {
    let index = normalize_round_index(record.index);
    OrchestrationRoundRecord {
        id: if record.id.trim().is_empty() {
            round_id(index)
        } else {
            record.id.trim().to_string()
        },
        index,
        situation: record.situation.trim().to_string(),
        user_message: record.user_message.trim().to_string(),
        created_at: record.created_at.max(0.0),
        finalized_at: record.finalized_at.max(0.0),
    }
}

pub fn persist_round_state(
    storage: &dyn StorageBackend,
    user_id: &str,
    state: &OrchestrationRoundState,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_orchestration_id = state.orchestration_id.trim();
    if cleaned_user_id.is_empty() || cleaned_orchestration_id.is_empty() {
        return Ok(());
    }
    let mut rounds = state
        .rounds
        .iter()
        .map(normalize_round_record)
        .filter(|round| round.index > 0)
        .collect::<Vec<_>>();
    rounds.sort_by_key(|round| round.index);
    rounds.dedup_by_key(|round| round.index);
    let suppressed_message_ranges = state
        .suppressed_message_ranges
        .iter()
        .filter_map(|range| {
            let start_at = range.start_at.max(0.0);
            let end_at = range.end_at.max(0.0);
            if start_at <= 0.0 || end_at < start_at {
                return None;
            }
            Some(OrchestrationSuppressedMessageRange { start_at, end_at })
        })
        .collect::<Vec<_>>();
    let normalized = OrchestrationRoundState {
        orchestration_id: cleaned_orchestration_id.to_string(),
        run_id: state.run_id.trim().to_string(),
        group_id: state.group_id.trim().to_string(),
        rounds,
        suppressed_message_ranges,
        updated_at: state.updated_at.max(0.0),
    };
    storage.set_meta(
        &round_state_meta_key(cleaned_user_id, cleaned_orchestration_id),
        &serde_json::to_string(&normalized)?,
    )?;
    Ok(())
}

pub fn load_round_state(
    storage: &dyn StorageBackend,
    user_id: &str,
    orchestration_id: &str,
) -> Option<OrchestrationRoundState> {
    let cleaned_user_id = user_id.trim();
    let cleaned_orchestration_id = orchestration_id.trim();
    if cleaned_user_id.is_empty() || cleaned_orchestration_id.is_empty() {
        return None;
    }
    let raw = storage
        .get_meta(&round_state_meta_key(cleaned_user_id, cleaned_orchestration_id))
        .ok()
        .flatten()?;
    let mut state = serde_json::from_str::<OrchestrationRoundState>(raw.trim()).ok()?;
    state.orchestration_id = state.orchestration_id.trim().to_string();
    state.run_id = state.run_id.trim().to_string();
    state.group_id = state.group_id.trim().to_string();
    if state.orchestration_id.is_empty() {
        state.orchestration_id = cleaned_orchestration_id.to_string();
    }
    state.rounds = state
        .rounds
        .iter()
        .map(normalize_round_record)
        .collect::<Vec<_>>();
    state.rounds.sort_by_key(|round| round.index);
    state.rounds.dedup_by_key(|round| round.index);
    state.suppressed_message_ranges = state
        .suppressed_message_ranges
        .into_iter()
        .filter(|range| range.start_at > 0.0 && range.end_at >= range.start_at)
        .collect();
    if state.orchestration_id.is_empty() {
        return None;
    }
    Some(state)
}

pub fn clear_round_state(
    storage: &dyn StorageBackend,
    user_id: &str,
    orchestration_id: &str,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_orchestration_id = orchestration_id.trim();
    if cleaned_user_id.is_empty() || cleaned_orchestration_id.is_empty() {
        return Ok(());
    }
    storage.delete_meta_prefix(&round_state_meta_key(cleaned_user_id, cleaned_orchestration_id))?;
    Ok(())
}

pub fn build_initial_round_state(state: &OrchestrationHiveState) -> OrchestrationRoundState {
    let now = now_ts();
    OrchestrationRoundState {
        orchestration_id: state.orchestration_id.trim().to_string(),
        run_id: state.run_id.trim().to_string(),
        group_id: state.group_id.trim().to_string(),
        rounds: vec![OrchestrationRoundRecord {
            id: round_id(1),
            index: 1,
            situation: String::new(),
            user_message: String::new(),
            created_at: now,
            finalized_at: 0.0,
        }],
        suppressed_message_ranges: Vec::new(),
        updated_at: now,
    }
}

pub fn latest_formal_round_index(round_state: Option<&OrchestrationRoundState>) -> i64 {
    round_state
        .and_then(|state| {
            state
                .rounds
                .iter()
                .filter(|round| !round.user_message.trim().is_empty())
                .map(|round| round.index)
                .max()
        })
        .unwrap_or(1)
        .max(1)
}

pub fn rebuild_branch_round_state(
    source: &OrchestrationRoundState,
    target_orchestration_id: &str,
    target_run_id: &str,
    branch_from_round_index: i64,
    now: f64,
) -> OrchestrationRoundState {
    let retained_index = normalize_round_index(branch_from_round_index);
    let mut rounds = source
        .rounds
        .iter()
        .filter(|round| round.index <= retained_index)
        .map(normalize_round_record)
        .collect::<Vec<_>>();
    if rounds.is_empty() {
        rounds.push(OrchestrationRoundRecord {
            id: round_id(1),
            index: 1,
            situation: String::new(),
            user_message: String::new(),
            created_at: now,
            finalized_at: 0.0,
        });
    }
    rounds.sort_by_key(|round| round.index);
    OrchestrationRoundState {
        orchestration_id: target_orchestration_id.trim().to_string(),
        run_id: target_run_id.trim().to_string(),
        group_id: source.group_id.trim().to_string(),
        rounds,
        suppressed_message_ranges: Vec::new(),
        updated_at: now,
    }
}

pub fn copy_round_directory_tree(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    source_run_id: &str,
    target_run_id: &str,
    inclusive_round_index: i64,
) -> Result<()> {
    let max_round = normalize_round_index(inclusive_round_index);
    for round_index in 1..=max_round {
        let source = workspace.resolve_path(
            workspace_id,
            &[
                "orchestration",
                source_run_id.trim(),
                &round_dir_name(round_index),
            ]
            .join("/"),
        )?;
        if !source.exists() {
            continue;
        }
        let target = workspace.resolve_path(
            workspace_id,
            &[
                "orchestration",
                target_run_id.trim(),
                &round_dir_name(round_index),
            ]
            .join("/"),
        )?;
        copy_dir_recursive(&source, &target)?;
    }
    Ok(())
}

pub fn copy_round_situation_files(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    source_run_id: &str,
    target_run_id: &str,
    start_round_index: i64,
) -> Result<()> {
    let source_root = workspace.resolve_path(
        workspace_id,
        &["orchestration", source_run_id.trim()].join("/"),
    )?;
    if !source_root.exists() || !source_root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(source_root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let round_index = parse_round_index_from_dir_name(&name);
        if round_index < normalize_round_index(start_round_index) {
            continue;
        }
        let source = entry.path().join(SITUATION_FILE_NAME);
        if !source.is_file() {
            continue;
        }
        let target = workspace.resolve_path(
            workspace_id,
            &situation_path(target_run_id, round_index),
        )?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    }
    Ok(())
}

pub fn clear_orchestration_workspace_tree(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    run_id: &str,
) -> Result<()> {
    let cleaned_run_id = run_id.trim();
    if cleaned_run_id.is_empty() {
        return Ok(());
    }
    let target = workspace.resolve_path(
        workspace_id,
        &["orchestration", cleaned_run_id].join("/"),
    )?;
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    Ok(())
}

pub fn delete_round_directories_after(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    run_id: &str,
    retained_round_index: i64,
) -> Result<()> {
    let root = workspace.resolve_path(
        workspace_id,
        &["orchestration", run_id.trim()].join("/"),
    )?;
    if !root.exists() || !root.is_dir() {
        return Ok(());
    }
    let retained = normalize_round_index(retained_round_index);
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let round_index = parse_round_index_from_dir_name(&name);
        if round_index <= retained {
            continue;
        }
        fs::remove_dir_all(entry.path())?;
    }
    Ok(())
}

pub fn copy_chat_history_until_round(
    storage: &dyn StorageBackend,
    user_id: &str,
    source_session_id: &str,
    target_session_id: &str,
    inclusive_round_index: i64,
) -> Result<()> {
    let target_round = normalize_round_index(inclusive_round_index);
    let messages = storage.load_chat_history(user_id.trim(), source_session_id.trim(), None)?;
    let mut copied_user_round = 0_i64;
    for message in messages {
        let role = message
            .get("role")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if role == "user" {
            copied_user_round += 1;
            if copied_user_round > target_round {
                break;
            }
        }
        let mut cloned = message;
        if let serde_json::Value::Object(ref mut map) = cloned {
            map.insert(
                "session_id".to_string(),
                serde_json::Value::String(target_session_id.trim().to_string()),
            );
            map.remove("_history_id");
        }
        storage.append_chat(user_id.trim(), &cloned)?;
    }
    Ok(())
}

pub fn active_orchestration_for_agent(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent_id: &str,
) -> Option<(OrchestrationHiveState, OrchestrationMemberBinding)> {
    let cleaned_user_id = user_id.trim();
    let cleaned_agent_id = agent_id.trim();
    if cleaned_user_id.is_empty() || cleaned_agent_id.is_empty() {
        return None;
    }
    let items = storage
        .list_meta_prefix(&format!("{HIVE_STATE_META_PREFIX}{cleaned_user_id}:"))
        .ok()?;
    for (_, raw) in items {
        let state = serde_json::from_str::<OrchestrationHiveState>(raw.trim()).ok()?;
        if !state.active || state.orchestration_id.trim().is_empty() {
            continue;
        }
        let binding = load_member_binding(storage, &state.orchestration_id, cleaned_agent_id)?;
        if binding.session_id.trim().is_empty() {
            continue;
        }
        return Some((state, binding));
    }
    None
}

pub fn session_orchestration_lock_info(
    storage: &dyn StorageBackend,
    user_id: &str,
    session_id: &str,
) -> Option<(OrchestrationHiveState, OrchestrationMemberBinding)> {
    let cleaned_user_id = user_id.trim();
    let cleaned_session_id = session_id.trim();
    if cleaned_user_id.is_empty() || cleaned_session_id.is_empty() {
        return None;
    }
    let context = load_session_context(storage, cleaned_user_id, cleaned_session_id)?;
    if context.group_id.trim().is_empty() {
        return None;
    }
    let state = load_hive_state(storage, cleaned_user_id, &context.group_id)?;
    let bindings = list_member_bindings(storage, &state.orchestration_id).ok()?;
    bindings
        .into_iter()
        .find(|binding| binding.session_id.trim() == cleaned_session_id)
        .map(|binding| (state, binding))
}

pub fn build_locked_thread_message(
    state: &OrchestrationHiveState,
    binding: &OrchestrationMemberBinding,
) -> String {
    let role = if binding.role.trim() == "mother" {
        "母蜂"
    } else {
        "工蜂"
    };
    format!(
        "{role}当前处于编排态，请在编排页面继续操作以保持线程连续性。蜂群：{}",
        state.group_id.trim()
    )
}

pub fn build_orchestration_thread_title(agent_name: &str) -> String {
    let cleaned_name = agent_name.trim().trim_start_matches('@').trim();
    if cleaned_name.is_empty() {
        "编排".to_string()
    } else {
        format!("编排+{cleaned_name}")
    }
}

pub fn build_chat_session_with_title(
    user_id: &str,
    agent: &UserAgentRecord,
    title: &str,
) -> ChatSessionRecord {
    let now = now_ts();
    ChatSessionRecord {
        session_id: format!("sess_{}", Uuid::new_v4().simple()),
        user_id: user_id.trim().to_string(),
        title: title.trim().to_string(),
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id: Some(agent.agent_id.trim().to_string()),
        tool_overrides: Vec::new(),
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    }
}

pub fn ensure_orchestration_member_session(
    storage: &dyn StorageBackend,
    user_id: &str,
    state: &OrchestrationHiveState,
    agent: &UserAgentRecord,
) -> Result<(OrchestrationMemberBinding, bool)> {
    if let Some(existing) = load_member_binding(storage, &state.orchestration_id, &agent.agent_id) {
        if let Some(session) = storage.get_chat_session(user_id.trim(), existing.session_id.trim())? {
            let session_agent_id = session.agent_id.as_deref().unwrap_or("").trim();
            if session_agent_id == agent.agent_id.trim() {
                bind_member_session_as_main_thread(
                    storage,
                    user_id,
                    &agent.agent_id,
                    existing.session_id.trim(),
                )?;
                return Ok((existing, false));
            }
        }
    }

    let role = if agent.agent_id.trim() == state.mother_agent_id.trim() {
        "mother"
    } else {
        "worker"
    };
    let title = build_orchestration_thread_title(&agent.name);
    let session = build_chat_session_with_title(user_id, agent, &title);
    storage.upsert_chat_session(&session)?;
    let binding = OrchestrationMemberBinding {
        orchestration_id: state.orchestration_id.clone(),
        run_id: state.run_id.clone(),
        group_id: state.group_id.clone(),
        agent_id: agent.agent_id.trim().to_string(),
        agent_name: agent.name.trim().to_string(),
        role: role.to_string(),
        session_id: session.session_id.clone(),
        title,
        created_at: session.created_at,
    };
    persist_member_binding(storage, &binding)?;
    persist_session_context(
        storage,
        user_id,
        &session.session_id,
        &OrchestrationSessionContext {
            mode: ORCHESTRATION_MODE.to_string(),
            run_id: state.run_id.clone(),
            group_id: state.group_id.clone(),
            role: role.to_string(),
            round_index: 1,
            mother_agent_id: state.mother_agent_id.clone(),
        },
    )?;
    bind_member_session_as_main_thread(storage, user_id, &agent.agent_id, &session.session_id)?;
    Ok((binding, true))
}

fn bind_member_session_as_main_thread(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent_id: &str,
    session_id: &str,
) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_agent_id = agent_id.trim();
    let cleaned_session_id = session_id.trim();
    if cleaned_user_id.is_empty() || cleaned_agent_id.is_empty() || cleaned_session_id.is_empty() {
        return Ok(());
    }
    let now = now_ts();
    let existing = storage.get_agent_thread(cleaned_user_id, cleaned_agent_id)?;
    let (thread_id, created_at, status) = if let Some(record) = existing {
        let next_thread_id = if record.thread_id.trim().is_empty() {
            format!("thread_{cleaned_session_id}")
        } else {
            record.thread_id
        };
        let next_status = if record.status.trim().is_empty() {
            "idle".to_string()
        } else {
            record.status
        };
        (next_thread_id, record.created_at, next_status)
    } else {
        (
            format!("thread_{cleaned_session_id}"),
            now,
            "idle".to_string(),
        )
    };
    storage.upsert_agent_thread(&AgentThreadRecord {
        thread_id,
        user_id: cleaned_user_id.to_string(),
        agent_id: cleaned_agent_id.to_string(),
        session_id: cleaned_session_id.to_string(),
        status,
        created_at,
        updated_at: now,
    })?;
    Ok(())
}

pub fn load_dispatch_context(
    storage: &dyn StorageBackend,
    workspace: &WorkspaceManager,
    workspace_id: &str,
    user_id: &str,
    session_id: &str,
) -> Option<OrchestrationDispatchContext> {
    let context = load_session_context(storage, user_id, session_id)?;
    let situation = read_situation_file(workspace, workspace_id, &context.run_id, context.round_index)
        .unwrap_or_default();
    Some(OrchestrationDispatchContext {
        round_index: context.round_index,
        situation,
    })
}

pub fn build_worker_dispatch_message(
    config: &Config,
    base_message: &str,
    context: Option<&OrchestrationDispatchContext>,
    target_agent_id: &str,
    target_agent_name: &str,
    is_first_worker_turn: bool,
) -> String {
    let Some(context) = context else {
        return base_message.trim().to_string();
    };
    let worker_name = if target_agent_name.trim().is_empty() {
        target_agent_id.trim()
    } else {
        target_agent_name.trim()
    };
    let artifact_path = prompt_agent_artifact_path(context.round_index, worker_name, target_agent_id);
    let mut blocks = Vec::new();
    if is_first_worker_turn {
        blocks.push(render_prompt_template(
            config,
            "worker_first_dispatch",
            &[
                ("worker_name", worker_name),
                ("artifact_path", artifact_path.as_str()),
            ],
        ));
    }
    blocks.push(render_prompt_template(
        config,
        "worker_round_artifacts",
        &[("artifact_path", artifact_path.as_str())],
    ));
    if !context.situation.trim().is_empty() {
        blocks.push(render_prompt_template(
            config,
            "situation_context",
            &[("situation", context.situation.trim())],
        ));
    }
    blocks.push(base_message.trim().to_string());
    blocks
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn session_has_visible_history(
    storage: &dyn StorageBackend,
    user_id: &str,
    session_id: &str,
) -> bool {
    storage
        .load_chat_history(user_id.trim(), session_id.trim(), Some(1))
        .map(|items| !items.is_empty())
        .unwrap_or(false)
}

fn read_situation_file(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    run_id: &str,
    round_index: i64,
) -> Option<String> {
    let path = situation_path(run_id, round_index);
    let target = workspace.resolve_path(workspace_id, &path).ok()?;
    if !target.is_file() {
        return None;
    }
    std::fs::read_to_string(target)
        .ok()
        .map(|content| content.trim().to_string())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    if source.is_file() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
        return Ok(());
    }
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let child_source = entry.path();
        let child_target = target.join(entry.file_name());
        if child_source.is_dir() {
            copy_dir_recursive(&child_source, &child_target)?;
        } else {
            if let Some(parent) = child_target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(child_source, child_target)?;
        }
    }
    Ok(())
}

fn parse_round_index_from_dir_name(value: &str) -> i64 {
    let Some(rest) = value.trim().strip_prefix("round_") else {
        return 0;
    };
    rest.parse::<i64>().unwrap_or(0).max(0)
}

fn render_prompt_template(_config: &Config, name: &str, values: &[(&str, &str)]) -> String {
    let locale = if crate::i18n::get_language()
        .to_ascii_lowercase()
        .starts_with("en")
    {
        "en"
    } else {
        "zh"
    };
    let localized_path = Path::new("prompts")
        .join(locale)
        .join("orchestration")
        .join(format!("{name}.txt"));
    let fallback_path = Path::new("prompts")
        .join("orchestration")
        .join(format!("{name}.txt"));
    let localized = read_prompt_template(&localized_path);
    let template = if localized.trim().is_empty() {
        read_prompt_template(&fallback_path)
    } else {
        localized
    };
    render_template(&template, values)
}

fn render_template(template: &str, values: &[(&str, &str)]) -> String {
    let mut output = template.to_string();
    for (key, value) in values {
        output = output.replace(&format!("{{{{{key}}}}}"), value);
        output = output.replace(&format!("{{{{ {key} }}}}"), value);
    }
    output.trim().to_string()
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn full_agent_artifact_path(
        run_id: &str,
        round_index: i64,
        agent_name: &str,
        fallback_agent_id: &str,
    ) -> String {
        [
            "orchestration".to_string(),
            run_id.trim().to_string(),
            round_dir_name(round_index),
            orchestration_agent_artifact_dir_name(agent_name, fallback_agent_id),
        ]
        .into_iter()
        .filter(|item| !item.trim().is_empty())
        .collect::<Vec<_>>()
        .join("/")
    }

    #[test]
    fn builds_round_paths() {
        assert_eq!(
            situation_path("orch_demo", 2),
            "orchestration/orch_demo/round_0002/situation.txt"
        );
        assert_eq!(
            full_agent_artifact_path("orch_demo", 3, "情报官", "agent_a"),
            "orchestration/orch_demo/round_0003/情报官"
        );
        assert_eq!(
            prompt_agent_artifact_path(3, "情报官", "agent_a"),
            "round_0003/情报官"
        );
    }

    #[test]
    fn worker_dispatch_includes_orchestration_context() {
        let context = OrchestrationDispatchContext {
            round_index: 1,
            situation: "market pressure".to_string(),
        };
        let message = build_worker_dispatch_message(
            &Config::default(),
            "analyze risk",
            Some(&context),
            "agent_worker",
            "Risk Worker",
            false,
        );
        assert!(message.contains("round_0001/Risk Worker"));
        assert!(!message.contains("orchestration/orch_demo/round_0001/agent_worker"));
        assert!(message.contains("market pressure"));
        assert!(message.contains("analyze risk"));
    }

    #[test]
    fn artifact_dir_name_prefers_agent_name() {
        assert_eq!(
            orchestration_agent_artifact_dir_name("蓝军技术官", "agent_x"),
            "蓝军技术官"
        );
        assert_eq!(
            orchestration_agent_artifact_dir_name(" Demo:Agent? ", "agent_x"),
            "Demo_Agent_"
        );
        assert_eq!(
            orchestration_agent_artifact_dir_name("   ", "agent_x"),
            "agent_x"
        );
    }

    #[test]
    fn builds_stable_meta_keys() {
        assert_eq!(
            hive_state_meta_key("u1", "hive_a"),
            "orchestration_hive_state:u1:hive_a"
        );
        assert_eq!(
            member_binding_meta_key("orch_1", "agent_a"),
            "orchestration_member_binding:orch_1:agent_a"
        );
    }
}
