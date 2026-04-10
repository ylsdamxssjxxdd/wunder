use crate::services::agent_abilities::resolve_selected_declared_names;
use crate::services::default_agent_protocol::{
    default_agent_config_from_record, default_agent_meta_key, record_from_default_agent_config,
    DefaultAgentConfig,
};
use crate::services::default_tool_profile::curated_default_tool_names;
use crate::services::user_agent_presets::{
    build_requested_tool_names_for_sync, PresetSyncMode, PresetSyncSummary,
};
use crate::services::worker_card_settings::{
    canonicalize_default_agent_config, collect_context_skill_names,
    default_agent_update_from_config, preset_snapshot_from_record, preset_snapshot_from_update,
};
use crate::state::AppState;
use crate::storage::{
    normalize_sandbox_container_id, UserAccountRecord, UserAgentPresetBinding,
    UserAgentPresetSnapshot, UserAgentRecord, DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID,
};
use crate::user_access::{build_user_tool_context, compute_allowed_tool_names};
use anyhow::Result;
use chrono::Utc;
use std::collections::HashSet;

const DEFAULT_AGENT_ACCESS_LEVEL: &str = "A";
pub const DEFAULT_AGENT_ID_ALIAS: &str = "__default__";
const DEFAULT_AGENT_APPROVAL_MODE: &str = "full_auto";
pub const DEFAULT_AGENT_NAME: &str = "Default Agent";
const DEFAULT_AGENT_STATUS: &str = "active";
const DEFAULT_AGENT_SYNC_BINDING_PREFIX: &str = "default_agent_sync_binding_v1:";
pub const PRESET_TEMPLATE_USER_ID: &str = "preset_template";

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[derive(Debug, Default)]
struct SyncDecision {
    visible_diff: bool,
    safe_updates: usize,
    override_count: usize,
}

fn default_agent_sync_binding_key(user_id: &str) -> String {
    format!("{DEFAULT_AGENT_SYNC_BINDING_PREFIX}{}", user_id.trim())
}

fn synthetic_user(user_id: &str) -> UserAccountRecord {
    let now = now_ts();
    UserAccountRecord {
        user_id: user_id.trim().to_string(),
        username: user_id.trim().to_string(),
        email: None,
        password_hash: String::new(),
        roles: vec!["user".to_string()],
        status: "active".to_string(),
        access_level: DEFAULT_AGENT_ACCESS_LEVEL.to_string(),
        unit_id: None,
        token_balance: 0,
        token_granted_total: 0,
        token_used_total: 0,
        last_token_grant_date: None,
        experience_total: 0,
        is_demo: false,
        created_at: now,
        updated_at: now,
        last_login_at: None,
    }
}

async fn collect_user_skill_name_keys(state: &AppState, user_id: &str) -> HashSet<String> {
    let context = build_user_tool_context(state, user_id).await;
    collect_context_skill_names(&context)
}

fn normalize_default_agent_config(
    config: &mut DefaultAgentConfig,
    skill_name_keys: &HashSet<String>,
) {
    if config.name.trim().is_empty() {
        config.name = DEFAULT_AGENT_NAME.to_string();
    } else {
        config.name = config.name.trim().to_string();
    }
    let normalized = canonicalize_default_agent_config(config, skill_name_keys);
    config.description = normalized.description;
    config.system_prompt = normalized.system_prompt;
    config.ability_items = normalized.ability_items;
    config.tool_names = normalized.tool_names;
    config.declared_tool_names = normalized.declared_tool_names;
    config.declared_skill_names = normalized.declared_skill_names;
    config.preset_questions = normalized.preset_questions;
    config.approval_mode = normalized.approval_mode;
    config.status = normalized.status;
    config.icon = normalized.icon;
    config.sandbox_container_id = normalize_sandbox_container_id(normalized.sandbox_container_id);
    let now = now_ts();
    if config.created_at <= 0.0 {
        config.created_at = now;
    }
    if config.updated_at <= 0.0 {
        config.updated_at = config.created_at;
    }
}

fn config_from_record(
    record: &UserAgentRecord,
    skill_name_keys: &HashSet<String>,
) -> DefaultAgentConfig {
    let mut config = default_agent_config_from_record(record);
    normalize_default_agent_config(&mut config, skill_name_keys);
    config
}

fn record_from_config(user_id: &str, config: &DefaultAgentConfig) -> UserAgentRecord {
    record_from_default_agent_config(
        DEFAULT_AGENT_ID_ALIAS,
        user_id,
        DEFAULT_AGENT_ACCESS_LEVEL,
        config,
    )
}

async fn load_default_agent_config(
    state: &AppState,
    user_id: &str,
    skill_name_keys: &HashSet<String>,
) -> Result<Option<DefaultAgentConfig>> {
    let raw = state
        .user_store
        .get_meta(&default_agent_meta_key(user_id))?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return Ok(None);
    }
    let mut parsed = match serde_json::from_str::<DefaultAgentConfig>(cleaned) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    normalize_default_agent_config(&mut parsed, skill_name_keys);
    Ok(Some(parsed))
}

async fn build_default_agent_config(
    state: &AppState,
    user: &UserAccountRecord,
) -> DefaultAgentConfig {
    let context = build_user_tool_context(state, &user.user_id).await;
    let allowed = compute_allowed_tool_names(user, &context);
    let skill_name_keys = collect_context_skill_names(&context);
    let tool_names = curated_default_tool_names(&allowed);
    let mut config = DefaultAgentConfig {
        name: DEFAULT_AGENT_NAME.to_string(),
        description: String::new(),
        system_prompt: String::new(),
        ability_items: Vec::new(),
        tool_names,
        declared_tool_names: Vec::new(),
        declared_skill_names: Vec::new(),
        preset_questions: Vec::new(),
        approval_mode: DEFAULT_AGENT_APPROVAL_MODE.to_string(),
        status: DEFAULT_AGENT_STATUS.to_string(),
        icon: None,
        sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    normalize_default_agent_config(&mut config, &skill_name_keys);
    config
}

async fn resolve_default_agent_config(
    state: &AppState,
    user: &UserAccountRecord,
) -> Result<DefaultAgentConfig> {
    let skill_name_keys = collect_user_skill_name_keys(state, &user.user_id).await;
    if let Some(config) = load_default_agent_config(state, &user.user_id, &skill_name_keys).await? {
        return Ok(config);
    }
    if let Some(record) = state
        .user_store
        .storage_backend()
        .get_user_agent(&user.user_id, DEFAULT_AGENT_ID_ALIAS)?
    {
        return Ok(config_from_record(&record, &skill_name_keys));
    }
    Ok(build_default_agent_config(state, user).await)
}

pub async fn load_effective_default_agent_record(
    state: &AppState,
    user_id: &str,
) -> Result<UserAgentRecord> {
    let owner = state
        .user_store
        .get_user_by_id(user_id)?
        .unwrap_or_else(|| synthetic_user(user_id));
    let config = resolve_default_agent_config(state, &owner).await?;
    Ok(record_from_config(&owner.user_id, &config))
}

fn snapshot_from_default_record(
    record: &UserAgentRecord,
    skill_name_keys: &HashSet<String>,
) -> UserAgentPresetSnapshot {
    let mut snapshot = preset_snapshot_from_record(record, skill_name_keys);
    snapshot.model_name = None;
    snapshot
}

async fn build_target_snapshot(
    state: &AppState,
    user: &UserAccountRecord,
    template: &DefaultAgentConfig,
) -> UserAgentPresetSnapshot {
    let context = build_user_tool_context(state, &user.user_id).await;
    let allowed_tool_names = compute_allowed_tool_names(user, &context);
    let skill_name_keys = collect_context_skill_names(&context);
    let tool_names = build_requested_tool_names_for_sync(
        &template.tool_names,
        &template.declared_tool_names,
        &template.declared_skill_names,
        &allowed_tool_names,
    );
    let (declared_tool_names, declared_skill_names) = resolve_selected_declared_names(
        &tool_names,
        &template.declared_tool_names,
        &template.declared_skill_names,
        &skill_name_keys,
    );
    let mut snapshot = preset_snapshot_from_update(
        &default_agent_update_from_config(
            &DefaultAgentConfig {
                ability_items: Vec::new(),
                tool_names,
                declared_tool_names,
                declared_skill_names,
                ..template.clone()
            },
            &skill_name_keys,
        ),
        None,
        &template.status,
    );
    snapshot.model_name = None;
    snapshot
}

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

// Keep safe sync field-granular so user customizations on default-agent settings
// are preserved until the admin explicitly chooses force sync.
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

fn load_sync_binding(state: &AppState, user_id: &str) -> Result<Option<UserAgentPresetBinding>> {
    let raw = state
        .user_store
        .get_meta(&default_agent_sync_binding_key(user_id))?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return Ok(None);
    }
    let parsed = serde_json::from_str::<UserAgentPresetBinding>(cleaned).ok();
    Ok(parsed.filter(|binding| binding.preset_id == DEFAULT_AGENT_ID_ALIAS))
}

fn save_sync_binding(
    state: &AppState,
    user_id: &str,
    binding: &UserAgentPresetBinding,
) -> Result<()> {
    let payload = serde_json::to_string(binding)?;
    state
        .user_store
        .set_meta(&default_agent_sync_binding_key(user_id), &payload)?;
    Ok(())
}

fn has_explicit_default_agent_state(state: &AppState, user_id: &str) -> Result<bool> {
    if state
        .user_store
        .storage_backend()
        .get_user_agent(user_id, DEFAULT_AGENT_ID_ALIAS)?
        .is_some()
    {
        return Ok(true);
    }
    Ok(state
        .user_store
        .get_meta(&default_agent_meta_key(user_id))?
        .is_some_and(|raw| !raw.trim().is_empty()))
}

async fn persist_default_agent_state(
    state: &AppState,
    user_id: &str,
    record: &UserAgentRecord,
    sync_created: bool,
) -> Result<()> {
    let skill_name_keys = collect_user_skill_name_keys(state, user_id).await;
    let mut config = config_from_record(record, &skill_name_keys);
    if sync_created {
        let now = now_ts();
        config.created_at = now;
        config.updated_at = now;
    } else {
        config.updated_at = now_ts();
    }
    let payload = serde_json::to_string(&config)?;
    state
        .user_store
        .set_meta(&default_agent_meta_key(user_id), &payload)?;

    if state
        .user_store
        .storage_backend()
        .get_user_agent(user_id, DEFAULT_AGENT_ID_ALIAS)?
        .is_some()
    {
        let mut legacy = record.clone();
        legacy.user_id = user_id.trim().to_string();
        legacy.agent_id = DEFAULT_AGENT_ID_ALIAS.to_string();
        legacy.hive_id = DEFAULT_HIVE_ID.to_string();
        legacy.model_name = None;
        legacy.declared_tool_names = config.declared_tool_names.clone();
        legacy.declared_skill_names = config.declared_skill_names.clone();
        legacy.access_level = DEFAULT_AGENT_ACCESS_LEVEL.to_string();
        legacy.is_shared = false;
        legacy.preset_binding = None;
        legacy.created_at = config.created_at;
        legacy.updated_at = config.updated_at;
        state.user_store.upsert_user_agent(&legacy)?;
    }
    Ok(())
}

pub async fn sync_default_agent_across_users(
    state: &AppState,
    mode: PresetSyncMode,
    unit_scope: Option<&[String]>,
    dry_run: bool,
) -> Result<PresetSyncSummary> {
    let template_record =
        load_effective_default_agent_record(state, PRESET_TEMPLATE_USER_ID).await?;
    let template_skill_name_keys =
        collect_user_skill_name_keys(state, PRESET_TEMPLATE_USER_ID).await;
    let template_config = config_from_record(&template_record, &template_skill_name_keys);
    let (users, _) = state.user_store.list_users(None, unit_scope, 0, 0)?;
    let mut summary = PresetSyncSummary {
        total_users: users.len(),
        ..PresetSyncSummary::default()
    };

    for user in users {
        state.user_store.ensure_default_hive(&user.user_id)?;
        let current_record = load_effective_default_agent_record(state, &user.user_id).await?;
        let current_skill_name_keys = collect_user_skill_name_keys(state, &user.user_id).await;
        let current = snapshot_from_default_record(&current_record, &current_skill_name_keys);
        let target = build_target_snapshot(state, &user, &template_config).await;
        let binding = load_sync_binding(state, &user.user_id)?;
        let binding_matches = binding
            .as_ref()
            .map(|item| item.preset_id == DEFAULT_AGENT_ID_ALIAS)
            .unwrap_or(false);
        let has_explicit = has_explicit_default_agent_state(state, &user.user_id)?;
        let baseline = binding
            .as_ref()
            .map(|item| item.last_applied.clone())
            .unwrap_or_else(|| {
                if has_explicit {
                    target.clone()
                } else {
                    current.clone()
                }
            });
        let decision = plan_snapshot_sync(&current, &baseline, &target);

        if has_explicit {
            summary.linked_users += 1;
        } else {
            summary.missing_users += 1;
        }

        if !decision.visible_diff && binding_matches && has_explicit {
            summary.up_to_date_agents += 1;
            continue;
        }

        summary.stale_agents += 1;
        if decision.safe_updates > 0 || !binding_matches || !has_explicit {
            summary.safe_update_agents += 1;
        }
        if decision.override_count > 0 {
            summary.overridden_agents += 1;
        }
        if decision.visible_diff || !binding_matches || !has_explicit {
            summary.force_update_agents += 1;
        }

        if dry_run {
            continue;
        }

        let mut next_record = current_record.clone();
        let applied = apply_sync_mode(&mut next_record, &baseline, &target, mode);
        let write_config = !has_explicit || applied || !binding_matches;
        if write_config {
            persist_default_agent_state(state, &user.user_id, &next_record, !has_explicit).await?;
        }
        save_sync_binding(
            state,
            &user.user_id,
            &UserAgentPresetBinding {
                preset_id: DEFAULT_AGENT_ID_ALIAS.to_string(),
                preset_revision: 1,
                last_applied: target,
            },
        )?;
        if write_config {
            if let Err(err) = state.inner_visible.sync_user_state(&user.user_id).await {
                tracing::warn!(
                    "failed to sync inner-visible default-agent state for {}: {err}",
                    user.user_id
                );
            }
        }

        if !has_explicit {
            summary.created_agents += 1;
        } else if applied {
            summary.updated_agents += 1;
        } else if !binding_matches {
            summary.rebound_agents += 1;
        }
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use crate::services::default_tool_profile::curated_default_tool_names;
    use std::collections::HashSet;

    #[test]
    fn curated_default_agent_tools_follow_fixed_profile() {
        let allowed = HashSet::from([
            "最终回复".to_string(),
            "定时任务".to_string(),
            "休眠等待".to_string(),
            "读取文件".to_string(),
            "技能创建器".to_string(),
            "其他工具".to_string(),
        ]);
        assert_eq!(
            curated_default_tool_names(&allowed),
            vec![
                "最终回复".to_string(),
                "定时任务".to_string(),
                "休眠等待".to_string(),
                "读取文件".to_string(),
                "技能创建器".to_string(),
            ]
        );
    }
}
