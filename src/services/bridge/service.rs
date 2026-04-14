use crate::channels::types::ChannelMessage;
use crate::config_store::ConfigStore;
use crate::services::bridge::identity::{
    extract_bridge_identity, normalize_bridge_route_status, normalize_bridge_thread_strategy,
    normalize_bridge_username_policy, BRIDGE_CENTER_STATUS_ACTIVE,
    BRIDGE_FALLBACK_POLICY_FORBID_OWNER, BRIDGE_ROUTE_STATUS_ACTIVE,
    BRIDGE_THREAD_STRATEGY_MAIN_THREAD, BRIDGE_USERNAME_POLICY_NAMESPACED_GENERATED,
};
use crate::services::external::ensure_external_embed_agent_with_runtime;
use crate::services::user_store::UserStore;
use crate::skills::SkillRegistry;
use crate::storage::{
    BridgeCenterAccountRecord, BridgeCenterRecord, BridgeDeliveryLogRecord, BridgeUserRouteRecord,
    StorageBackend,
};
use crate::user_tools::UserToolManager;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BridgeRouteResolution {
    pub center: BridgeCenterRecord,
    pub center_account: BridgeCenterAccountRecord,
    pub route: BridgeUserRouteRecord,
    pub session_strategy: String,
}

#[derive(Clone)]
pub struct BridgeRuntime {
    pub config_store: ConfigStore,
    pub skills: Arc<RwLock<SkillRegistry>>,
    pub user_tool_manager: Arc<UserToolManager>,
    pub user_store: Arc<UserStore>,
    pub storage: Arc<dyn StorageBackend>,
}

#[allow(dead_code)]
pub async fn is_bridge_managed_account(
    runtime: &BridgeRuntime,
    channel: &str,
    account_id: &str,
) -> Result<bool> {
    let storage = runtime.storage.clone();
    let channel = channel.trim().to_string();
    let account_id = account_id.trim().to_string();
    let record = tokio::task::spawn_blocking(move || {
        storage.get_bridge_center_account_by_channel_account(&channel, &account_id)
    })
    .await
    .unwrap_or_else(|err| Err(anyhow!(err)))?;
    Ok(record.is_some())
}

pub async fn resolve_inbound_bridge_route(
    runtime: &BridgeRuntime,
    message: &ChannelMessage,
) -> Result<Option<BridgeRouteResolution>> {
    let storage = runtime.storage.clone();
    let channel = message.channel.trim().to_string();
    let account_id = message.account_id.trim().to_string();
    let center_account = tokio::task::spawn_blocking(move || {
        storage.get_bridge_center_account_by_channel_account(&channel, &account_id)
    })
    .await
    .unwrap_or_else(|err| Err(anyhow!(err)))?;
    let Some(center_account) = center_account else {
        return Ok(None);
    };

    let center = load_center(runtime, &center_account.center_id).await?;
    ensure_center_account_active(&center, &center_account)?;

    let identity = extract_bridge_identity(
        message,
        center_account
            .identity_strategy
            .as_deref()
            .or(Some(center.default_identity_strategy.as_str())),
    )?;
    let existing_route = load_route_by_identity(
        runtime,
        &center_account.center_account_id,
        &identity.external_identity_key,
    )
    .await?;

    let resolved_route = match existing_route {
        Some(route) => ensure_existing_route(runtime, route, &identity).await?,
        None => auto_provision_route(runtime, &center, &center_account, &identity).await?,
    };

    Ok(Some(BridgeRouteResolution {
        center,
        center_account: center_account.clone(),
        route: resolved_route,
        session_strategy: center_account
            .thread_strategy
            .as_deref()
            .map(normalize_bridge_thread_strategy)
            .unwrap_or_else(|| BRIDGE_THREAD_STRATEGY_MAIN_THREAD.to_string()),
    }))
}

pub fn append_bridge_meta(meta: &mut Value, resolution: &BridgeRouteResolution) {
    let Some(meta_obj) = meta.as_object_mut() else {
        return;
    };
    meta_obj.insert(
        "bridge_center_id".to_string(),
        Value::String(resolution.center.center_id.clone()),
    );
    meta_obj.insert(
        "bridge_center_account_id".to_string(),
        Value::String(resolution.center_account.center_account_id.clone()),
    );
    meta_obj.insert(
        "bridge_route_id".to_string(),
        Value::String(resolution.route.route_id.clone()),
    );
}

pub async fn log_bridge_delivery(
    runtime: &BridgeRuntime,
    resolution: &BridgeRouteResolution,
    direction: &str,
    stage: &str,
    status: &str,
    provider_message_id: Option<&str>,
    session_id: Option<&str>,
    summary: &str,
    payload: Option<Value>,
) -> Result<()> {
    let record = BridgeDeliveryLogRecord {
        delivery_id: format!("bdl_{}", Uuid::new_v4().simple()),
        center_id: resolution.center.center_id.clone(),
        center_account_id: resolution.center_account.center_account_id.clone(),
        route_id: Some(resolution.route.route_id.clone()),
        direction: direction.trim().to_string(),
        stage: stage.trim().to_string(),
        provider_message_id: provider_message_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        session_id: session_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        status: status.trim().to_string(),
        summary: summary.trim().to_string(),
        payload,
        created_at: now_ts(),
    };
    let storage = runtime.storage.clone();
    tokio::task::spawn_blocking(move || storage.insert_bridge_delivery_log(&record))
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))
}

pub async fn touch_bridge_route_after_outbound(
    runtime: &BridgeRuntime,
    route_id: &str,
    session_id: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    let Some(mut record) = load_route(runtime, route_id).await? else {
        return Ok(());
    };
    let now = now_ts();
    record.last_seen_at = now;
    record.last_outbound_at = Some(now);
    if let Some(session_id) = session_id.map(str::trim).filter(|value| !value.is_empty()) {
        record.last_session_id = Some(session_id.to_string());
    }
    record.last_error = error
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    record.updated_at = now;
    let storage = runtime.storage.clone();
    tokio::task::spawn_blocking(move || storage.upsert_bridge_user_route(&record))
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))
}

async fn auto_provision_route(
    runtime: &BridgeRuntime,
    center: &BridgeCenterRecord,
    center_account: &BridgeCenterAccountRecord,
    identity: &crate::services::bridge::BridgeIdentity,
) -> Result<BridgeUserRouteRecord> {
    let preset_name = resolve_target_preset_name(center, center_account)?;
    ensure_preset_exists(runtime, &preset_name).await?;

    let username = build_bridge_username(center, identity);
    let desktop_mode = is_desktop_mode(runtime).await;
    let (session, user_created, _) = crate::services::external::provision_external_launch_session(
        runtime.user_store.as_ref(),
        &username,
        None,
        center.target_unit_id.clone(),
        desktop_mode,
        crate::services::user_store::UserStore::default_session_scope(),
    )?;
    let (agent, agent_created) = ensure_external_embed_agent_with_runtime(
        &runtime.config_store,
        &runtime.skills,
        &runtime.user_tool_manager,
        runtime.user_store.as_ref(),
        &session.user,
        &preset_name,
    )
    .await?;
    let now = now_ts();
    let route = BridgeUserRouteRecord {
        route_id: format!("brt_{}", Uuid::new_v4().simple()),
        center_id: center.center_id.clone(),
        center_account_id: center_account.center_account_id.clone(),
        channel: center_account.channel.clone(),
        account_id: center_account.account_id.clone(),
        external_identity_key: identity.external_identity_key.clone(),
        external_user_key: identity.external_user_key.clone(),
        external_display_name: identity.external_display_name.clone(),
        external_peer_id: identity.external_peer_id.clone(),
        external_sender_id: identity.external_sender_id.clone(),
        external_thread_id: identity.external_thread_id.clone(),
        external_profile: Some(identity.external_profile.clone()),
        wunder_user_id: session.user.user_id.clone(),
        agent_id: agent.agent_id.clone(),
        agent_name: agent.name.clone(),
        user_created,
        agent_created,
        status: BRIDGE_ROUTE_STATUS_ACTIVE.to_string(),
        last_session_id: None,
        last_error: None,
        first_seen_at: now,
        last_seen_at: now,
        last_inbound_at: Some(now),
        last_outbound_at: None,
        created_at: now,
        updated_at: now,
    };
    let storage = runtime.storage.clone();
    let inserted = route.clone();
    tokio::task::spawn_blocking(move || storage.upsert_bridge_user_route(&inserted))
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))?;

    if user_created {
        insert_audit_log(
            runtime,
            &center.center_id,
            None,
            "system",
            "bridge",
            "auto_provision_user",
            json!({
                "wunder_user_id": session.user.user_id,
                "external_identity_key": identity.external_identity_key,
                "center_account_id": center_account.center_account_id,
            }),
        )
        .await?;
    }
    if agent_created {
        insert_audit_log(
            runtime,
            &center.center_id,
            None,
            "system",
            "bridge",
            "auto_bind_agent",
            json!({
                "agent_id": agent.agent_id,
                "agent_name": agent.name,
                "wunder_user_id": session.user.user_id,
            }),
        )
        .await?;
    }

    load_route_by_identity(
        runtime,
        &center_account.center_account_id,
        &identity.external_identity_key,
    )
    .await?
    .ok_or_else(|| anyhow!("bridge route create lost after upsert"))
}

async fn ensure_existing_route(
    runtime: &BridgeRuntime,
    mut route: BridgeUserRouteRecord,
    identity: &crate::services::bridge::BridgeIdentity,
) -> Result<BridgeUserRouteRecord> {
    let status = normalize_bridge_route_status(&route.status);
    if status != BRIDGE_ROUTE_STATUS_ACTIVE {
        return Err(anyhow!("bridge route is {status}"));
    }
    let now = now_ts();
    route.external_user_key = identity.external_user_key.clone();
    route.external_display_name = identity.external_display_name.clone();
    route.external_peer_id = identity.external_peer_id.clone();
    route.external_sender_id = identity.external_sender_id.clone();
    route.external_thread_id = identity.external_thread_id.clone();
    route.external_profile = Some(identity.external_profile.clone());
    route.last_seen_at = now;
    route.last_inbound_at = Some(now);
    route.updated_at = now;
    route.last_error = None;
    let storage = runtime.storage.clone();
    let updated_route = route.clone();
    tokio::task::spawn_blocking(move || storage.upsert_bridge_user_route(&updated_route))
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))?;
    load_route(runtime, &route.route_id)
        .await?
        .ok_or_else(|| anyhow!("bridge route missing after update"))
}

async fn load_center(runtime: &BridgeRuntime, center_id: &str) -> Result<BridgeCenterRecord> {
    let storage = runtime.storage.clone();
    let center_id = center_id.trim().to_string();
    tokio::task::spawn_blocking(move || storage.get_bridge_center(&center_id))
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))?
        .ok_or_else(|| anyhow!("bridge center not found"))
}

async fn load_route(
    runtime: &BridgeRuntime,
    route_id: &str,
) -> Result<Option<BridgeUserRouteRecord>> {
    let storage = runtime.storage.clone();
    let route_id = route_id.trim().to_string();
    tokio::task::spawn_blocking(move || storage.get_bridge_user_route(&route_id))
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))
}

async fn load_route_by_identity(
    runtime: &BridgeRuntime,
    center_account_id: &str,
    external_identity_key: &str,
) -> Result<Option<BridgeUserRouteRecord>> {
    let storage = runtime.storage.clone();
    let center_account_id = center_account_id.trim().to_string();
    let external_identity_key = external_identity_key.trim().to_string();
    tokio::task::spawn_blocking(move || {
        storage.get_bridge_user_route_by_identity(&center_account_id, &external_identity_key)
    })
    .await
    .unwrap_or_else(|err| Err(anyhow!(err)))
}

fn ensure_center_account_active(
    center: &BridgeCenterRecord,
    center_account: &BridgeCenterAccountRecord,
) -> Result<()> {
    if !center
        .status
        .trim()
        .eq_ignore_ascii_case(BRIDGE_CENTER_STATUS_ACTIVE)
    {
        return Err(anyhow!("bridge center is not active"));
    }
    if !center_account.enabled {
        return Err(anyhow!("bridge center account is disabled"));
    }
    if center_account
        .fallback_policy
        .trim()
        .eq_ignore_ascii_case(BRIDGE_FALLBACK_POLICY_FORBID_OWNER)
    {
        return Ok(());
    }
    Err(anyhow!("bridge center account fallback policy is invalid"))
}

fn resolve_target_preset_name(
    center: &BridgeCenterRecord,
    center_account: &BridgeCenterAccountRecord,
) -> Result<String> {
    let preset_name = center_account
        .default_preset_agent_name_override
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| center.default_preset_agent_name.trim());
    if preset_name.is_empty() {
        return Err(anyhow!("bridge center preset agent is empty"));
    }
    Ok(preset_name.to_string())
}

fn build_bridge_username(
    center: &BridgeCenterRecord,
    identity: &crate::services::bridge::BridgeIdentity,
) -> String {
    let policy = normalize_bridge_username_policy(&center.username_policy);
    if policy != BRIDGE_USERNAME_POLICY_NAMESPACED_GENERATED {
        if let Some(raw) = identity
            .external_user_key
            .as_deref()
            .or(identity.external_display_name.as_deref())
            .and_then(crate::services::user_store::UserStore::normalize_user_id)
        {
            if !crate::services::user_store::UserStore::is_default_admin(&raw) {
                return raw;
            }
        }
    }
    let center_code = normalize_center_code(&center.code);
    let mut hasher = Sha256::new();
    hasher.update(identity.external_identity_key.as_bytes());
    let digest = hasher.finalize();
    let short_hash = hex::encode(digest);
    format!("bridge_{}_{}", center_code, &short_hash[..12])
}

fn normalize_center_code(value: &str) -> String {
    let mut output = value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .map(|ch| ch.to_ascii_lowercase())
        .collect::<String>();
    if output.is_empty() {
        output = "hub".to_string();
    }
    output
}

async fn ensure_preset_exists(runtime: &BridgeRuntime, preset_name: &str) -> Result<()> {
    let preset_name = preset_name.trim();
    if preset_name.is_empty() {
        return Err(anyhow!("bridge preset agent is empty"));
    }
    let config = runtime.config_store.get().await;
    let exists = config
        .user_agents
        .presets
        .iter()
        .any(|item| item.name.trim() == preset_name);
    if exists {
        return Ok(());
    }
    Err(anyhow!("preset agent '{preset_name}' not found"))
}

async fn is_desktop_mode(runtime: &BridgeRuntime) -> bool {
    runtime
        .config_store
        .get()
        .await
        .server
        .mode
        .trim()
        .eq_ignore_ascii_case("desktop")
}

async fn insert_audit_log(
    runtime: &BridgeRuntime,
    center_id: &str,
    route_id: Option<&str>,
    actor_type: &str,
    actor_id: &str,
    action: &str,
    detail: Value,
) -> Result<()> {
    let record = crate::storage::BridgeRouteAuditLogRecord {
        audit_id: format!("bal_{}", Uuid::new_v4().simple()),
        center_id: center_id.to_string(),
        route_id: route_id.map(str::to_string),
        actor_type: actor_type.to_string(),
        actor_id: actor_id.to_string(),
        action: action.to_string(),
        detail: Some(detail),
        created_at: now_ts(),
    };
    let storage = runtime.storage.clone();
    tokio::task::spawn_blocking(move || storage.insert_bridge_route_audit_log(&record))
        .await
        .unwrap_or_else(|err| Err(anyhow!(err)))
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
