use crate::config_store::ConfigStore;
use crate::core::state::AppState;
use crate::services::user_access::{compute_allowed_tool_names, UserToolContext};
use crate::services::user_store::UserStore;
use crate::skills::SkillRegistry;
use crate::storage::{
    normalize_sandbox_container_id, UserAccountRecord, UserAgentRecord, DEFAULT_HIVE_ID,
};
use crate::user_tools::UserToolManager;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub const DEFAULT_EXTERNAL_LAUNCH_PASSWORD: &str = "123456";

pub async fn resolve_or_create_external_embed_agent(
    state: &Arc<AppState>,
    user: &UserAccountRecord,
    target_agent_name: &str,
) -> Result<UserAgentRecord> {
    ensure_external_embed_agent(state, user, target_agent_name)
        .await
        .map(|(agent, _)| agent)
}

pub async fn ensure_external_embed_agent(
    state: &Arc<AppState>,
    user: &UserAccountRecord,
    target_agent_name: &str,
) -> Result<(UserAgentRecord, bool)> {
    ensure_external_embed_agent_with_runtime(
        &state.config_store,
        &state.skills,
        &state.user_tool_manager,
        state.user_store.as_ref(),
        user,
        target_agent_name,
    )
    .await
}

pub async fn ensure_external_embed_agent_with_runtime(
    config_store: &ConfigStore,
    skills: &Arc<RwLock<SkillRegistry>>,
    user_tool_manager: &Arc<UserToolManager>,
    user_store: &UserStore,
    user: &UserAccountRecord,
    target_agent_name: &str,
) -> Result<(UserAgentRecord, bool)> {
    let cleaned_name = target_agent_name.trim();
    if cleaned_name.is_empty() {
        return Err(anyhow!("external embed agent is empty"));
    }
    let all_agents = user_store.list_user_agents(&user.user_id)?;
    let mut candidates = all_agents
        .into_iter()
        .filter(|item| item.name.trim() == cleaned_name)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));

    let config = config_store.get().await;
    let preset = config
        .user_agents
        .presets
        .into_iter()
        .find(|item| item.name.trim() == cleaned_name);

    // Prefer the user's same-name custom agent before falling back to preset templates.
    if let Some(preset) = preset.as_ref() {
        if let Some(custom) = candidates
            .iter()
            .find(|item| !is_external_embed_preset_template(item, preset))
            .cloned()
        {
            return Ok((custom, false));
        }
    }
    if let Some(existing) = candidates.first().cloned() {
        return Ok((existing, false));
    }

    let Some(preset) = preset else {
        return Err(anyhow!("agent '{cleaned_name}' not found"));
    };

    user_store.ensure_default_hive(&user.user_id)?;
    let config = config_store.get().await;
    let skills = skills.read().await.clone();
    let bindings = user_tool_manager.build_bindings(&config, &skills, &user.user_id);
    let tool_access = user_store.get_user_tool_access(&user.user_id)?;
    let context = UserToolContext {
        config,
        skills,
        bindings,
        tool_access,
    };
    let mut tool_names = compute_allowed_tool_names(user, &context)
        .into_iter()
        .collect::<Vec<_>>();
    tool_names.sort();

    let icon_name = if preset.icon_name.trim().is_empty() {
        "spark".to_string()
    } else {
        preset.icon_name.trim().to_string()
    };
    let icon_color = if preset.icon_color.trim().is_empty() {
        "#94a3b8".to_string()
    } else {
        preset.icon_color.trim().to_string()
    };
    let icon = json!({
        "name": icon_name,
        "color": icon_color
    })
    .to_string();
    let now = now_ts();
    let created = UserAgentRecord {
        agent_id: format!("agent_{}", Uuid::new_v4().simple()),
        user_id: user.user_id.clone(),
        hive_id: DEFAULT_HIVE_ID.to_string(),
        name: cleaned_name.to_string(),
        description: preset.description.trim().to_string(),
        system_prompt: preset.system_prompt.trim().to_string(),
        model_name: None,
        ability_items: Vec::new(),
        declared_tool_names: Vec::new(),
        declared_skill_names: Vec::new(),
        tool_names,
        preset_questions: Vec::new(),
        access_level: "A".to_string(),
        approval_mode: "full_auto".to_string(),
        is_shared: false,
        status: "active".to_string(),
        icon: Some(icon),
        sandbox_container_id: normalize_sandbox_container_id(preset.sandbox_container_id),
        created_at: now,
        updated_at: now,
        preset_binding: None,
        silent: false,
        prefer_mother: false,
    };
    user_store.upsert_user_agent(&created)?;
    Ok((created, true))
}

pub fn resolve_external_embed_target_agent_name(
    requested_agent_name: Option<&str>,
    default_agent_name: Option<String>,
) -> Result<String> {
    if let Some(agent_name) = requested_agent_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(agent_name.to_string());
    }

    default_agent_name.ok_or_else(|| anyhow!("external embed preset agent is not configured"))
}

pub fn provision_external_user(
    user_store: &UserStore,
    username: &str,
    password: &str,
    unit_id: Option<String>,
    desktop_mode: bool,
    session_scope: &str,
) -> Result<(crate::services::user_store::UserSession, bool, bool)> {
    let normalized =
        UserStore::normalize_user_id(username).ok_or_else(|| anyhow!("invalid username"))?;
    if UserStore::is_default_admin(&normalized) {
        return Err(anyhow!("admin account is protected"));
    }
    let password = password.trim();
    if password.is_empty() {
        return Err(anyhow!("password is empty"));
    }

    let mut created = false;
    let mut updated = false;

    let existing = user_store.get_user_by_username(&normalized)?;
    if let Some(mut user) = existing {
        if UserStore::is_admin(&user) {
            return Err(anyhow!("admin account is protected"));
        }
        if user.status.trim().to_lowercase() != "active" {
            return Err(anyhow!("user disabled"));
        }

        // Keep wunder password in sync with external system password.
        if !UserStore::verify_password(&user.password_hash, password) {
            user.password_hash = UserStore::hash_password(password)?;
            user.updated_at = now_ts();
            user_store.update_user(&user)?;
            updated = true;
        }

        if sync_external_unit_binding(user_store, &mut user, unit_id.as_deref(), desktop_mode)? {
            user.updated_at = now_ts();
            user_store.update_user(&user)?;
            updated = true;
        }
    } else {
        let create_unit_id = if desktop_mode { None } else { unit_id.clone() };
        let mut created_user = user_store.create_user(
            &normalized,
            None,
            password,
            Some("A"),
            create_unit_id,
            vec!["user".to_string()],
            "active",
            false,
        )?;
        if sync_external_unit_binding(
            user_store,
            &mut created_user,
            unit_id.as_deref(),
            desktop_mode,
        )? {
            created_user.updated_at = now_ts();
            user_store.update_user(&created_user)?;
            updated = true;
        }
        created = true;
    }

    let session = user_store.login_with_scope(&normalized, password, session_scope)?;
    Ok((session, created, updated))
}

pub fn provision_external_launch_session(
    user_store: &UserStore,
    username: &str,
    password: Option<&str>,
    unit_id: Option<String>,
    desktop_mode: bool,
    session_scope: &str,
) -> Result<(crate::services::user_store::UserSession, bool, bool)> {
    if let Some(password) = password {
        let cleaned = password.trim();
        if !cleaned.is_empty() {
            return provision_external_user(
                user_store,
                username,
                cleaned,
                unit_id,
                desktop_mode,
                session_scope,
            );
        }
    }

    let normalized =
        UserStore::normalize_user_id(username).ok_or_else(|| anyhow!("invalid username"))?;
    if UserStore::is_default_admin(&normalized) {
        return Err(anyhow!("admin account is protected"));
    }

    let mut created = false;
    let mut updated = false;
    let existing = user_store.get_user_by_username(&normalized)?;
    let user = if let Some(mut user) = existing {
        if UserStore::is_admin(&user) {
            return Err(anyhow!("admin account is protected"));
        }
        if user.status.trim().to_lowercase() != "active" {
            return Err(anyhow!("user disabled"));
        }
        if sync_external_unit_binding(user_store, &mut user, unit_id.as_deref(), desktop_mode)? {
            user.updated_at = now_ts();
            user_store.update_user(&user)?;
            updated = true;
        }
        user
    } else {
        let create_unit_id = if desktop_mode { None } else { unit_id.clone() };
        let mut created_user = user_store.create_user(
            &normalized,
            None,
            DEFAULT_EXTERNAL_LAUNCH_PASSWORD,
            Some("A"),
            create_unit_id,
            vec!["user".to_string()],
            "active",
            false,
        )?;
        if sync_external_unit_binding(
            user_store,
            &mut created_user,
            unit_id.as_deref(),
            desktop_mode,
        )? {
            created_user.updated_at = now_ts();
            user_store.update_user(&created_user)?;
            updated = true;
        }
        created = true;
        created_user
    };
    let session = user_store.issue_session_for_user_with_scope(user, session_scope)?;
    Ok((session, created, updated))
}

fn sync_external_unit_binding(
    user_store: &UserStore,
    user: &mut UserAccountRecord,
    unit_id: Option<&str>,
    desktop_mode: bool,
) -> Result<bool> {
    let Some(next_unit_id) = unit_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(false);
    };
    if user.unit_id.as_deref() == Some(next_unit_id) {
        return Ok(false);
    }
    if desktop_mode {
        user.unit_id = Some(next_unit_id.to_string());
        return Ok(true);
    }

    let next_unit = user_store
        .get_org_unit(next_unit_id)?
        .ok_or_else(|| anyhow!("unit not found"))?;
    user.unit_id = Some(next_unit.unit_id.clone());
    Ok(true)
}

fn is_external_embed_preset_template(
    candidate: &UserAgentRecord,
    preset: &crate::config::UserAgentPresetConfig,
) -> bool {
    let preset_description = preset.description.trim();
    let preset_prompt = preset.system_prompt.trim();
    candidate.description.trim() == preset_description
        && candidate.system_prompt.trim() == preset_prompt
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
