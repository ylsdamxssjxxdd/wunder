//! Transport-independent edits for a user's agent directory.
use crate::services::{
    agent_abilities, default_agent_protocol, default_agent_sync, user_agent_presets,
    worker_card_settings,
};
use crate::{state::AppState, storage::UserAgentRecord, user_access};
use anyhow::{anyhow, bail, Result};

#[derive(Debug, Clone)]
pub struct AgentSettingsUpdate {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub model_name: String,
    pub icon_name: String,
    pub icon_color: String,
    /// Full canonical icon payload (`{"kind":..}` JSON). When present it wins
    /// over the flattened `icon_name`/`icon_color` pair so native clients can
    /// persist static and companion avatars exactly like the web client.
    pub icon: Option<String>,
    pub tool_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub sandbox_container_id: i32,
    pub approval_mode: String,
    pub preview_skill: bool,
    pub silent: bool,
    pub prefer_mother: bool,
}

pub async fn list(state: &AppState, user_id: &str) -> Result<Vec<UserAgentRecord>> {
    let user = state
        .user_store
        .get_user_by_id(user_id)?
        .ok_or_else(|| anyhow!("用户不存在"))?;
    user_agent_presets::ensure_user_agent_bootstrap(state, &user).await?;
    state.inner_visible.sync_user_state(user_id).await?;
    let access = state.user_store.get_user_agent_access(user_id)?;
    let mut records =
        vec![default_agent_sync::load_effective_default_agent_record(state, user_id).await?];
    records.extend(
        user_access::filter_user_agents_by_access(
            &user,
            access.as_ref(),
            state.user_store.list_user_agents(user_id)?,
        )
        .into_iter()
        .filter(|row| row.agent_id != "__default__"),
    );
    records.truncate(100);
    Ok(records)
}

pub async fn owned(state: &AppState, user_id: &str, agent_id: &str) -> Result<UserAgentRecord> {
    if matches!(agent_id.trim(), "" | "default" | "__default__") {
        return default_agent_sync::load_effective_default_agent_record(state, user_id).await;
    }
    let user = state
        .user_store
        .get_user_by_id(user_id)?
        .ok_or_else(|| anyhow!("用户不存在"))?;
    let access = state.user_store.get_user_agent_access(user_id)?;
    let record = state
        .user_store
        .get_user_agent(user_id, agent_id)?
        .ok_or_else(|| anyhow!("智能体不存在"))?;
    if !user_access::is_agent_allowed(&user, access.as_ref(), &record) {
        bail!("无权访问此智能体");
    }
    Ok(record)
}

/// Transport-independent delete shared by the server API and the native
/// desktop façade. Removes the record, its materialized worker-card files and
/// the scoped workspace data, so the next bidirectional sync cannot restore
/// the agent from disk.
pub async fn delete(state: &AppState, user_id: &str, agent_id: &str) -> Result<()> {
    let record = owned(state, user_id, agent_id).await?;
    if record.is_shared {
        bail!("共享智能体不能由当前用户删除");
    }
    let deleted = state
        .user_store
        .delete_user_agent(user_id, &record.agent_id)?;
    if deleted == 0 {
        bail!("智能体不存在或无权删除");
    }
    if let Err(err) = state
        .inner_visible
        .remove_agent_files(user_id, &record.agent_id)
    {
        tracing::warn!(
            "failed to remove inner-visible files for {}/{}: {err}",
            user_id,
            record.agent_id
        );
    }
    // Purge only agent-scoped workspace variants. In single-root deployments
    // every variant collapses to the base user, where a purge would wipe the
    // user's shared sessions and cron jobs, so scoping must be effective.
    let scoped = state
        .workspace
        .scoped_user_id_variants(user_id, Some(&record.agent_id));
    let unscoped = state.workspace.scoped_user_id_variants(user_id, None);
    if scoped != unscoped {
        let mut workspace_ids = scoped;
        workspace_ids.sort();
        workspace_ids.dedup();
        for workspace_id in workspace_ids {
            let _ = state.workspace.purge_user_data(&workspace_id);
        }
    }
    Ok(())
}

pub async fn create(state: &AppState, user_id: &str, name: &str) -> Result<UserAgentRecord> {
    validate_name(name)?;
    let mut record = owned(state, user_id, "__default__").await?;
    let user = state
        .user_store
        .get_user_by_id(user_id)?
        .ok_or_else(|| anyhow!("用户不存在"))?;
    let context = user_access::build_user_tool_context(state, user_id).await;
    let allowed = user_access::compute_allowed_tool_names(&user, &context);
    record.tool_names = user_agent_presets::filter_allowed_tools(&record.tool_names, &allowed);
    record.agent_id = format!("agent_{}", uuid::Uuid::new_v4().simple());
    record.name = name.trim().into();
    record.is_shared = false;
    record.status = "active".into();
    record.preset_binding = None;
    record.created_at = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    record.updated_at = record.created_at;
    state.user_store.upsert_user_agent(&record)?;
    materialize(state, user_id).await;
    Ok(record)
}

#[allow(clippy::too_many_arguments)]
pub async fn update(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    name: &str,
    description: &str,
    system_prompt: &str,
    model_name: &str,
    icon_name: &str,
    icon_color: &str,
) -> Result<UserAgentRecord> {
    validate_name(name)?;
    if description.len() > 16_384
        || system_prompt.len() > 65_536
        || model_name.len() > 512
        || description.contains('\0')
        || system_prompt.contains('\0')
        || model_name.chars().any(char::is_control)
    {
        bail!("智能体配置过长或包含无效字符");
    }
    if icon_name.len() > 96 || icon_color.len() > 32 {
        bail!("头像配置无效");
    }
    let mut record = owned(state, user_id, agent_id).await?;
    let config = state.config_store.get().await;
    let model = model_name.trim();
    if !model.is_empty()
        && !config
            .llm
            .models
            .get(model)
            .is_some_and(crate::llm::is_llm_model)
    {
        bail!("请选择已配置的对话模型");
    }
    // The default agent follows the global default model; never silently ignore an edit.
    if record.agent_id == "__default__" && !model.is_empty() && model != config.llm.default {
        bail!("默认智能体跟随系统默认模型，请在模型配置中修改");
    }
    record.name = name.trim().into();
    record.description = description.into();
    record.system_prompt = system_prompt.into();
    record.model_name = (!model.is_empty()).then(|| model.to_string());
    // Keep avatar data in the shared canonical JSON format used by server clients.
    record.icon = Some(worker_card_settings::build_icon_payload(
        &worker_card_settings::normalize_preset_icon_name(Some(icon_name)),
        &worker_card_settings::normalize_preset_icon_color(Some(icon_color)),
    ));
    record.updated_at = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    // Only the agent template changes. Existing thread prompts remain frozen.
    if record.agent_id == "__default__" {
        let value = default_agent_protocol::default_agent_config_from_record(&record);
        state.user_store.set_meta(
            &default_agent_protocol::default_agent_meta_key(user_id),
            &serde_json::to_string(&value)?,
        )?;
    } else {
        state.user_store.upsert_user_agent(&record)?;
    }
    materialize(state, user_id).await;
    Ok(record)
}

/// Applies the complete agent editor payload used by native clients.  The
/// basic update above remains the compatibility path for older callers; this
/// method owns validation, permission filtering and persistence of the
/// runtime fields that the web editor exposes.
pub async fn update_settings(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    input: AgentSettingsUpdate,
) -> Result<UserAgentRecord> {
    let mut record = update(
        state,
        user_id,
        agent_id,
        &input.name,
        &input.description,
        &input.system_prompt,
        &input.model_name,
        &input.icon_name,
        &input.icon_color,
    )
    .await?;
    let user = state
        .user_store
        .get_user_by_id(user_id)?
        .ok_or_else(|| anyhow!("用户不存在"))?;
    let context = user_access::build_user_tool_context(state, user_id).await;
    let allowed = user_access::compute_allowed_tool_names(&user, &context);
    let container = input.sandbox_container_id.clamp(1, 10);
    // Mirror the server API update route: the worker-card projection prefers
    // ability_items/declared names over tool_names, so all three must move
    // together or the next bidirectional sync resurrects the stale selection.
    let skill_name_keys = worker_card_settings::collect_context_skill_names(&context);
    let selection = agent_abilities::resolve_agent_ability_selection(
        &input.tool_names,
        None,
        None,
        None,
        &skill_name_keys,
    );
    record.tool_names = user_agent_presets::filter_allowed_tools(&selection.tool_names, &allowed);
    record.ability_items = selection.ability_items;
    record.declared_tool_names = selection.declared_tool_names;
    record.declared_skill_names = selection.declared_skill_names;
    record.preset_questions =
        user_agent_presets::normalize_preset_questions(input.preset_questions);
    record.approval_mode =
        user_agent_presets::normalize_agent_approval_mode(Some(&input.approval_mode));
    record.sandbox_container_id = container;
    record.preview_skill = input.preview_skill;
    record.silent = input.silent;
    record.prefer_mother = input.prefer_mother;
    if let Some(icon) = input
        .icon
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if icon.len() > 1024 {
            bail!("头像配置无效");
        }
        record.icon = Some(worker_card_settings::normalize_icon_payload(Some(icon)));
    }
    record.updated_at = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    if record.agent_id == "__default__" {
        let value = default_agent_protocol::default_agent_config_from_record(&record);
        state.user_store.set_meta(
            &default_agent_protocol::default_agent_meta_key(user_id),
            &serde_json::to_string(&value)?,
        )?;
    } else {
        state.user_store.upsert_user_agent(&record)?;
    }
    materialize(state, user_id).await;
    Ok(record)
}

fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        bail!("名称不能为空、超过 80 个字符或包含控制字符");
    }
    Ok(())
}

async fn materialize(state: &AppState, user_id: &str) {
    if let Err(error) = state.inner_visible.materialize_user_state(user_id).await {
        tracing::warn!(%error, "agent settings file projection failed");
    }
}
