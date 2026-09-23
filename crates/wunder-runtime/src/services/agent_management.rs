//! Transport-independent edits for a user's agent directory.
use crate::services::{default_agent_protocol, default_agent_sync, user_agent_presets, worker_card_settings};
use crate::{state::AppState, storage::UserAgentRecord, user_access};
use anyhow::{anyhow, bail, Result};

pub async fn list(state: &AppState, user_id: &str) -> Result<Vec<UserAgentRecord>> {
    let user = state
        .user_store
        .get_user_by_id(user_id)?
        .ok_or_else(|| anyhow!("用户不存在"))?;
    state.user_store.ensure_default_hive(user_id)?;
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

pub async fn create(state: &AppState, user_id: &str, name: &str) -> Result<UserAgentRecord> {
    validate_name(name)?;
    state.user_store.ensure_default_hive(user_id)?;
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
