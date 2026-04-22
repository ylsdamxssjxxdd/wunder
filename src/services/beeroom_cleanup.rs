use crate::core::state::AppState;
use crate::services::hive_pack::{get_latest_hive_pack_import_binding, HivePackImportBinding};
use crate::services::orchestration_context::{
    clear_history_record, clear_hive_state, clear_member_bindings, clear_round_state,
    clear_session_context, list_history_records, load_hive_state,
};
use crate::services::swarm::beeroom::mother_meta_key;
use crate::storage::{normalize_hive_id, UserAgentRecord, DEFAULT_HIVE_ID};
use anyhow::Result;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeeroomDeleteMode {
    Standard,
    Purge,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BeeroomDeleteReport {
    pub reset_agent_total: i64,
    pub deleted_agent_total: i64,
    pub deleted_mission_total: i64,
    pub deleted_chat_message_total: i64,
    pub deleted_skill_total: i64,
    pub skipped_skill_total: i64,
    pub deleted_binding_total: i64,
}

pub fn delete_group(
    state: &AppState,
    user_id: &str,
    group_id: &str,
    mode: BeeroomDeleteMode,
) -> Result<BeeroomDeleteReport> {
    let cleaned_user_id = user_id.trim();
    let cleaned_group_id = normalize_hive_id(group_id);
    let mut report = BeeroomDeleteReport::default();
    if cleaned_user_id.is_empty()
        || cleaned_group_id.is_empty()
        || cleaned_group_id == DEFAULT_HIVE_ID
    {
        return Ok(report);
    }

    let members = state
        .user_store
        .list_user_agents_by_hive(cleaned_user_id, &cleaned_group_id)?;
    let member_ids = members
        .iter()
        .map(|agent| agent.agent_id.clone())
        .collect::<Vec<_>>();

    let import_binding =
        get_latest_hive_pack_import_binding(state, cleaned_user_id, &cleaned_group_id)?;

    match mode {
        BeeroomDeleteMode::Standard => {
            if !member_ids.is_empty() {
                report.reset_agent_total = state.user_store.move_agents_to_hive(
                    cleaned_user_id,
                    DEFAULT_HIVE_ID,
                    &member_ids,
                )?;
            }
        }
        BeeroomDeleteMode::Purge => {
            report.deleted_agent_total =
                purge_group_agents(state, cleaned_user_id, &members, import_binding.as_ref())?;
            let skill_report = purge_imported_skills_if_safe(
                state,
                cleaned_user_id,
                import_binding.as_ref(),
                &members,
            )?;
            report.deleted_skill_total = skill_report.deleted;
            report.skipped_skill_total = skill_report.skipped;
            report.deleted_binding_total = usize::from(import_binding.is_some()) as i64;
        }
    }

    report.deleted_mission_total = state
        .user_store
        .delete_team_runs_by_hive(cleaned_user_id, &cleaned_group_id)?;
    report.deleted_chat_message_total = state
        .user_store
        .delete_beeroom_chat_messages(cleaned_user_id, &cleaned_group_id)?;

    clear_group_runtime_state(state, cleaned_user_id, &cleaned_group_id)?;
    clear_hive_pack_import_binding(state, cleaned_user_id, &cleaned_group_id)?;
    Ok(report)
}

fn purge_group_agents(
    state: &AppState,
    user_id: &str,
    members: &[UserAgentRecord],
    binding: Option<&HivePackImportBinding>,
) -> Result<i64> {
    let owned_agent_ids = binding
        .map(|item| {
            item.agent_ids
                .iter()
                .map(|agent_id| agent_id.trim().to_string())
                .filter(|agent_id| !agent_id.is_empty())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_else(|| {
            members
                .iter()
                .map(|agent| agent.agent_id.trim().to_string())
                .filter(|agent_id| !agent_id.is_empty())
                .collect::<HashSet<_>>()
        });
    let mut deleted_total = 0_i64;
    for member in members {
        let agent_id = member.agent_id.trim();
        if agent_id.is_empty() || !owned_agent_ids.contains(agent_id) {
            continue;
        }
        deleted_total += state.user_store.delete_user_agent(user_id, agent_id)?;
        if let Err(err) = state.inner_visible.remove_agent_files(user_id, agent_id) {
            tracing::warn!("failed to remove inner-visible files for {user_id}/{agent_id}: {err}");
        }
        let mut workspace_ids = state
            .workspace
            .scoped_user_id_variants(user_id, Some(agent_id));
        workspace_ids.sort();
        workspace_ids.dedup();
        for workspace_id in workspace_ids {
            let _ = state.workspace.purge_user_data(&workspace_id);
        }
    }
    Ok(deleted_total)
}

#[derive(Debug, Clone, Copy, Default)]
struct SkillDeleteReport {
    deleted: i64,
    skipped: i64,
}

fn purge_imported_skills_if_safe(
    state: &AppState,
    user_id: &str,
    binding: Option<&HivePackImportBinding>,
    group_members: &[UserAgentRecord],
) -> Result<SkillDeleteReport> {
    let Some(binding) = binding else {
        return Ok(SkillDeleteReport::default());
    };
    if binding.skill_names.is_empty() {
        return Ok(SkillDeleteReport::default());
    }

    let skill_root = state.user_tool_store.get_skill_root(user_id);
    let group_agent_ids = group_members
        .iter()
        .map(|agent| agent.agent_id.trim().to_string())
        .filter(|agent_id| !agent_id.is_empty())
        .collect::<HashSet<_>>();
    let remaining_agents = state.user_store.list_user_agents(user_id)?;
    let mut report = SkillDeleteReport::default();
    let mut enabled = state
        .user_tool_store
        .load_user_tools(user_id)
        .skills
        .enabled;
    let mut shared = state.user_tool_store.load_user_tools(user_id).skills.shared;

    for skill_name in &binding.skill_names {
        let cleaned_skill_name = skill_name.trim();
        if cleaned_skill_name.is_empty() {
            continue;
        }
        let in_use_elsewhere = remaining_agents.iter().any(|agent| {
            let agent_id = agent.agent_id.trim();
            !group_agent_ids.contains(agent_id)
                && agent_uses_skill(agent, cleaned_skill_name, state, user_id)
        });
        if in_use_elsewhere {
            report.skipped += 1;
            continue;
        }
        let skill_dir = skill_root.join(cleaned_skill_name);
        if !skill_dir.exists() || !skill_dir.is_dir() {
            continue;
        }
        std::fs::remove_dir_all(&skill_dir)?;
        enabled.retain(|value| value.trim() != cleaned_skill_name);
        shared.retain(|value| value.trim() != cleaned_skill_name);
        report.deleted += 1;
    }

    if report.deleted > 0 {
        state
            .user_tool_store
            .update_skills(user_id, enabled, shared)?;
        state.user_tool_manager.clear_skill_cache(Some(user_id));
    }
    Ok(report)
}

fn agent_uses_skill(
    agent: &UserAgentRecord,
    skill_name: &str,
    state: &AppState,
    user_id: &str,
) -> bool {
    let cleaned_skill_name = skill_name.trim();
    if cleaned_skill_name.is_empty() {
        return false;
    }
    let bare = cleaned_skill_name.to_string();
    let runtime = state
        .user_tool_store
        .build_user_skill_name(user_id, user_id, cleaned_skill_name);
    agent
        .declared_skill_names
        .iter()
        .any(|value| value.trim() == cleaned_skill_name)
        || agent.tool_names.iter().any(|value| {
            let cleaned = value.trim();
            cleaned == bare || cleaned == runtime
        })
}

fn clear_group_runtime_state(state: &AppState, user_id: &str, group_id: &str) -> Result<()> {
    let cleaned_user_id = user_id.trim();
    let cleaned_group_id = group_id.trim();
    let storage = state.storage.as_ref();
    if let Some(hive_state) = load_hive_state(storage, cleaned_user_id, cleaned_group_id) {
        clear_session_context(storage, cleaned_user_id, &hive_state.mother_session_id)?;
        clear_round_state(storage, cleaned_user_id, &hive_state.orchestration_id)?;
        clear_member_bindings(storage, &hive_state.orchestration_id)?;
    }
    for history in list_history_records(storage, cleaned_user_id, cleaned_group_id)? {
        clear_history_record(
            storage,
            cleaned_user_id,
            cleaned_group_id,
            &history.orchestration_id,
        )?;
        clear_round_state(storage, cleaned_user_id, &history.orchestration_id)?;
        clear_member_bindings(storage, &history.orchestration_id)?;
        clear_session_context(storage, cleaned_user_id, &history.mother_session_id)?;
    }
    clear_hive_state(storage, cleaned_user_id, cleaned_group_id)?;
    storage.delete_meta_prefix(&mother_meta_key(cleaned_user_id, cleaned_group_id))?;
    Ok(())
}

fn clear_hive_pack_import_binding(state: &AppState, user_id: &str, group_id: &str) -> Result<()> {
    let prefix = format!(
        "{}{}:{}",
        crate::services::hive_pack::HIVE_PACK_IMPORT_BINDING_PREFIX,
        user_id.trim(),
        normalize_hive_id(group_id)
    );
    state.storage.delete_meta_prefix(&prefix)?;
    Ok(())
}
