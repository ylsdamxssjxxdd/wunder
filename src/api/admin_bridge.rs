use crate::auth as guard_auth;
use crate::channels::catalog::user_supported_channels;
use crate::channels::types::{ChannelAccountConfig, WeixinConfig};
use crate::channels::weixin;
use crate::services::bridge::{
    build_bridge_provider_caps, normalize_bridge_center_status, normalize_bridge_fallback_policy,
    normalize_bridge_identity_strategy, normalize_bridge_reply_strategy,
    normalize_bridge_route_status, normalize_bridge_thread_strategy,
    normalize_bridge_username_policy, BRIDGE_FALLBACK_POLICY_FORBID_OWNER,
    BRIDGE_ROUTE_STATUS_ACTIVE,
};
use crate::services::default_agent_sync::{
    load_effective_default_agent_record, DEFAULT_AGENT_ID_ALIAS, PRESET_TEMPLATE_USER_ID,
};
use crate::services::user_agent_presets::configured_preset_agents;
use crate::state::AppState;
use crate::storage::{
    BridgeCenterAccountRecord, BridgeCenterRecord, BridgeDeliveryLogRecord,
    BridgeRouteAuditLogRecord, BridgeUserRouteRecord, ChannelAccountRecord,
    ListBridgeCenterAccountsQuery, ListBridgeCentersQuery, ListBridgeDeliveryLogsQuery,
    ListBridgeRouteAuditLogsQuery, ListBridgeUserRoutesQuery,
};
use crate::user_store::UserStore;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

const BRIDGE_LIST_LIMIT_MAX: i64 = 500;
const BRIDGE_OVERVIEW_LIMIT: i64 = 10_000;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/bridge/metadata", get(admin_bridge_metadata))
        .route(
            "/wunder/admin/bridge/supported_channels",
            get(admin_bridge_supported_channels),
        )
        .route(
            "/wunder/admin/bridge/centers",
            get(admin_bridge_centers).post(admin_bridge_center_upsert),
        )
        .route(
            "/wunder/admin/bridge/centers/{center_id}",
            get(admin_bridge_center_detail).delete(admin_bridge_center_delete),
        )
        .route(
            "/wunder/admin/bridge/centers/{center_id}/accounts",
            get(admin_bridge_center_accounts).post(admin_bridge_center_account_create),
        )
        .route(
            "/wunder/admin/bridge/centers/{center_id}/weixin_bind",
            post(admin_bridge_center_weixin_bind),
        )
        .route(
            "/wunder/admin/bridge/accounts/{center_account_id}",
            patch(admin_bridge_center_account_update).delete(admin_bridge_center_account_delete),
        )
        .route("/wunder/admin/bridge/routes", get(admin_bridge_routes))
        .route(
            "/wunder/admin/bridge/routes/{route_id}",
            get(admin_bridge_route_detail).patch(admin_bridge_route_patch),
        )
        .route(
            "/wunder/admin/bridge/delivery_logs",
            get(admin_bridge_delivery_logs),
        )
}

async fn admin_bridge_metadata(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    let preset_agents = bridge_preset_agents_payload(&state).await?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let channel_accounts = state
        .storage
        .list_channel_accounts(None, Some("active"))
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "default_password": crate::services::external::DEFAULT_EXTERNAL_LAUNCH_PASSWORD,
            "supported_channels": supported_channel_payload(&state),
            "preset_agents": preset_agents,
            "channel_accounts": channel_accounts
                .into_iter()
                .map(|record| json!({
                    "channel": record.channel,
                    "account_id": record.account_id,
                    "status": record.status,
                }))
                .collect::<Vec<_>>(),
            "org_units": units
                .into_iter()
                .map(|unit| json!({
                    "unit_id": unit.unit_id,
                    "name": unit.name,
                    "path_name": unit.path_name,
                    "level": unit.level,
                }))
                .collect::<Vec<_>>(),
        }
    })))
}

async fn bridge_preset_agents_payload(state: &Arc<AppState>) -> Result<Vec<Value>, Response> {
    let mut items = Vec::new();
    let mut seen_names = HashSet::new();

    let default_record = load_effective_default_agent_record(state.as_ref(), PRESET_TEMPLATE_USER_ID)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let default_name = default_record.name.trim();
    if !default_name.is_empty() {
        seen_names.insert(default_name.to_string());
        items.push(json!({
            "preset_id": DEFAULT_AGENT_ID_ALIAS,
            "name": default_name,
            "description": default_record.description.trim(),
            "is_default_agent": true,
        }));
    }

    for preset in configured_preset_agents(state.as_ref()).await {
        let name = preset.name.trim();
        if name.is_empty() || seen_names.contains(name) {
            continue;
        }
        seen_names.insert(name.to_string());
        items.push(json!({
            "preset_id": preset.preset_id,
            "name": name,
            "description": preset.description.trim(),
            "is_default_agent": false,
        }));
    }

    Ok(items)
}

async fn admin_bridge_supported_channels(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    Ok(Json(json!({
        "data": {
            "items": supported_channel_payload(&state),
        }
    })))
}

async fn admin_bridge_centers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<BridgeCentersQuery>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    let limit = normalize_limit(query.limit);
    let offset = query.offset.unwrap_or(0).max(0);
    let (items, total) = state
        .storage
        .list_bridge_centers(ListBridgeCentersQuery {
            status: query.status.as_deref(),
            keyword: query.keyword.as_deref(),
            offset,
            limit,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let overview = load_bridge_overview_maps(&state)?;
    let items = items
        .iter()
        .map(|record| bridge_center_payload(&state, record, &overview))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({
        "data": {
            "total": total,
            "items": items,
        }
    })))
}

async fn admin_bridge_center_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(center_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    let center = state
        .storage
        .get_bridge_center(center_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(StatusCode::NOT_FOUND, "bridge center not found".to_string())
        })?;
    let overview = load_bridge_overview_maps(&state)?;
    let accounts = state
        .storage
        .list_bridge_center_accounts(ListBridgeCenterAccountsQuery {
            center_id: Some(center.center_id.as_str()),
            channel: None,
            account_id: None,
            enabled: None,
            offset: 0,
            limit: BRIDGE_OVERVIEW_LIMIT,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .0;
    Ok(Json(json!({
        "data": {
            "center": bridge_center_payload(&state, &center, &overview)?,
            "shared_channels": accounts
                .iter()
                .map(|record| bridge_center_account_payload(record, &overview))
                .collect::<Vec<_>>(),
            "accounts": accounts
                .iter()
                .map(|record| bridge_center_account_payload(record, &overview))
                .collect::<Vec<_>>(),
        }
    })))
}

#[derive(Debug, Default)]
struct BridgeOverviewMaps {
    center_name: HashMap<String, String>,
    center_account_count: HashMap<String, i64>,
    center_route_count: HashMap<String, i64>,
    center_active_route_count: HashMap<String, i64>,
    account_route_count: HashMap<String, i64>,
    account_active_route_count: HashMap<String, i64>,
}

#[derive(Debug, Deserialize)]
struct BridgeCentersQuery {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct BridgeRoutesQuery {
    #[serde(default)]
    center_id: Option<String>,
    #[serde(default)]
    center_account_id: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    wunder_user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct BridgeDeliveryLogsQuery {
    #[serde(default)]
    center_id: Option<String>,
    #[serde(default)]
    center_account_id: Option<String>,
    #[serde(default)]
    route_id: Option<String>,
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct BridgeCenterUpsertPayload {
    #[serde(default)]
    center_id: Option<String>,
    name: String,
    code: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<String>,
    default_preset_agent_name: String,
    #[serde(default)]
    target_unit_id: Option<String>,
    #[serde(default)]
    default_identity_strategy: Option<String>,
    #[serde(default)]
    username_policy: Option<String>,
    #[serde(default)]
    settings: Option<Value>,
    #[serde(default)]
    shared_channels: Option<Vec<BridgeCenterAccountUpsertPayload>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct BridgeCenterAccountUpsertPayload {
    #[serde(default)]
    center_account_id: Option<String>,
    #[serde(default)]
    center_id: Option<String>,
    channel: String,
    account_id: String,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    default_preset_agent_name_override: Option<String>,
    #[serde(default)]
    identity_strategy: Option<String>,
    #[serde(default)]
    thread_strategy: Option<String>,
    #[serde(default)]
    reply_strategy: Option<String>,
    #[serde(default)]
    fallback_policy: Option<String>,
    #[serde(default)]
    status_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BridgeCenterWeixinBindPayload {
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default, alias = "apiBase")]
    api_base: Option<String>,
    #[serde(default, alias = "botType")]
    bot_type: Option<String>,
    bot_token: String,
    ilink_bot_id: String,
    #[serde(default)]
    ilink_user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BridgeRoutePatchPayload {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    clear_last_error: Option<bool>,
}

async fn admin_bridge_center_upsert(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<BridgeCenterUpsertPayload>,
) -> Result<Json<Value>, Response> {
    let admin = ensure_admin_user(&state, &headers)?;
    let shared_channels = payload.shared_channels.clone();
    let name = required_text(&payload.name, "name")?;
    let code = required_text(&payload.code, "code")?.to_ascii_lowercase();
    let preset_name = required_text(
        &payload.default_preset_agent_name,
        "default_preset_agent_name",
    )?;
    ensure_preset_exists(&state, &preset_name).await?;
    if let Some(unit_id) = payload.target_unit_id.as_deref() {
        ensure_org_unit_exists(&state, unit_id)?;
    }
    if let Some(existing) = state
        .storage
        .get_bridge_center_by_code(&code)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        if payload.center_id.as_deref() != Some(existing.center_id.as_str()) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("bridge center code already exists: {code}"),
            ));
        }
    }
    let now = now_ts();
    let center_id = payload
        .center_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("bc_{}", Uuid::new_v4().simple()));
    let created_at = state
        .storage
        .get_bridge_center(&center_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .map(|record| record.created_at)
        .unwrap_or(now);
    let record = BridgeCenterRecord {
        center_id: center_id.clone(),
        name,
        code,
        description: clean_optional_text(payload.description.as_deref()),
        owner_user_id: admin.user_id.clone(),
        status: normalize_bridge_center_status(payload.status.as_deref().unwrap_or("active")),
        default_preset_agent_name: preset_name,
        target_unit_id: clean_optional_text(payload.target_unit_id.as_deref()),
        default_identity_strategy: normalize_bridge_identity_strategy(
            payload
                .default_identity_strategy
                .as_deref()
                .unwrap_or("sender_in_peer"),
        ),
        username_policy: normalize_bridge_username_policy(
            payload
                .username_policy
                .as_deref()
                .unwrap_or("namespaced_generated"),
        ),
        password_policy: "fixed_default_123456".to_string(),
        settings: payload.settings.unwrap_or_else(|| json!({})),
        created_at,
        updated_at: now,
    };
    state
        .storage
        .upsert_bridge_center(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Some(shared_channels) = shared_channels {
        sync_bridge_center_accounts(&state, &record, shared_channels).await?;
    }
    let overview = load_bridge_overview_maps(&state)?;
    let center_accounts = state
        .storage
        .list_bridge_center_accounts(ListBridgeCenterAccountsQuery {
            center_id: Some(center_id.as_str()),
            channel: None,
            account_id: None,
            enabled: None,
            offset: 0,
            limit: BRIDGE_OVERVIEW_LIMIT,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .0;
    Ok(Json(json!({
        "data": {
            "center": bridge_center_payload(&state, &record, &overview)?,
            "shared_channels": center_accounts
                .iter()
                .map(|item| bridge_center_account_payload(item, &overview))
                .collect::<Vec<_>>(),
        }
    })))
}

async fn admin_bridge_center_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(center_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    let center_id = center_id.trim();
    if center_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "center_id is required".to_string(),
        ));
    }
    state
        .storage
        .delete_bridge_delivery_logs_by_center(center_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state
        .storage
        .delete_bridge_route_audit_logs_by_center(center_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state
        .storage
        .delete_bridge_user_routes_by_center(center_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state
        .storage
        .delete_bridge_center_accounts_by_center(center_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let deleted = state
        .storage
        .delete_bridge_center(center_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "deleted": deleted,
        }
    })))
}

async fn admin_bridge_center_accounts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(center_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    let center_id = required_text(&center_id, "center_id")?;
    let overview = load_bridge_overview_maps(&state)?;
    let accounts = state
        .storage
        .list_bridge_center_accounts(ListBridgeCenterAccountsQuery {
            center_id: Some(center_id.as_str()),
            channel: None,
            account_id: None,
            enabled: None,
            offset: 0,
            limit: BRIDGE_OVERVIEW_LIMIT,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .0;
    Ok(Json(json!({
        "data": {
            "items": accounts
                .iter()
                .map(|record| bridge_center_account_payload(record, &overview))
                .collect::<Vec<_>>(),
        }
    })))
}

async fn admin_bridge_center_account_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(center_id): AxumPath<String>,
    Json(payload): Json<BridgeCenterAccountUpsertPayload>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    upsert_bridge_center_account(&state, payload, Some(center_id)).await
}

async fn admin_bridge_center_account_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(center_account_id): AxumPath<String>,
    Json(mut payload): Json<BridgeCenterAccountUpsertPayload>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    payload.center_account_id = Some(center_account_id);
    upsert_bridge_center_account(&state, payload, None).await
}

async fn admin_bridge_center_account_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(center_account_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    let center_account_id = required_text(&center_account_id, "center_account_id")?;
    delete_bridge_center_account_cascade(&state, &center_account_id)?;
    Ok(Json(json!({
        "data": {
            "deleted": true,
        }
    })))
}

async fn admin_bridge_center_weixin_bind(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(center_id): AxumPath<String>,
    Json(payload): Json<BridgeCenterWeixinBindPayload>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    let center_id = required_text(&center_id, "center_id")?;
    let center = state
        .storage
        .get_bridge_center(&center_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(StatusCode::NOT_FOUND, "bridge center not found".to_string())
        })?;
    let account_id = clean_optional_text(payload.account_id.as_deref())
        .unwrap_or_else(|| build_bridge_weixin_account_id(&center));
    let api_base = weixin::normalize_api_base(payload.api_base.as_deref())
        .unwrap_or_else(|| weixin::DEFAULT_API_BASE.to_string());
    let bot_type = weixin::normalize_bot_type(payload.bot_type.as_deref());
    let bot_token = required_text(&payload.bot_token, "bot_token")?;
    let ilink_bot_id = required_text(&payload.ilink_bot_id, "ilink_bot_id")?;
    let ilink_user_id = clean_optional_text(payload.ilink_user_id.as_deref());
    let now = now_ts();

    let existing_account = state
        .storage
        .get_channel_account(weixin::WEIXIN_CHANNEL, &account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let created_at = existing_account
        .as_ref()
        .map(|record| record.created_at)
        .unwrap_or(now);
    let mut config_value = existing_account
        .as_ref()
        .map(|record| record.config.clone())
        .unwrap_or_else(|| json!({}));
    if !config_value.is_object() {
        config_value = Value::Object(Map::new());
    }
    let existing_weixin = ChannelAccountConfig::from_value(&config_value)
        .weixin
        .unwrap_or_default();
    let long_connection_enabled = weixin::long_connection_enabled(&existing_weixin);
    let map = config_value.as_object_mut().ok_or_else(|| {
        error_response(
            StatusCode::BAD_REQUEST,
            "invalid channel config".to_string(),
        )
    })?;
    let inbound_token_missing = map
        .get("inbound_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none();
    if inbound_token_missing {
        map.insert(
            "inbound_token".to_string(),
            Value::String(format!("bridgech_{}", Uuid::new_v4().simple())),
        );
    }
    map.insert(
        "owner_user_id".to_string(),
        Value::String(center.owner_user_id.clone()),
    );
    map.insert("agent_id".to_string(), Value::Null);
    map.insert(
        "weixin".to_string(),
        json!(WeixinConfig {
            api_base: Some(api_base.clone()),
            cdn_base: existing_weixin.cdn_base.clone(),
            bot_token: Some(bot_token),
            ilink_bot_id: Some(ilink_bot_id),
            ilink_user_id,
            bot_type: Some(bot_type.clone()),
            long_connection_enabled: Some(long_connection_enabled),
            allow_from: existing_weixin.allow_from.clone(),
            poll_timeout_ms: existing_weixin.poll_timeout_ms,
            api_timeout_ms: existing_weixin.api_timeout_ms,
            max_consecutive_failures: existing_weixin.max_consecutive_failures,
            backoff_ms: existing_weixin.backoff_ms,
            typing_enabled: existing_weixin.typing_enabled,
            media_enabled: existing_weixin.media_enabled,
            route_tag: existing_weixin.route_tag.clone(),
        }),
    );

    let channel_account = ChannelAccountRecord {
        channel: weixin::WEIXIN_CHANNEL.to_string(),
        account_id: account_id.clone(),
        config: config_value.clone(),
        status: "active".to_string(),
        created_at,
        updated_at: now,
    };
    state
        .storage
        .upsert_channel_account(&channel_account)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let existing_bindings = state
        .storage
        .list_bridge_center_accounts(ListBridgeCenterAccountsQuery {
            center_id: Some(center.center_id.as_str()),
            channel: None,
            account_id: None,
            enabled: None,
            offset: 0,
            limit: BRIDGE_OVERVIEW_LIMIT,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .0;
    let reused_center_account_id = existing_bindings
        .iter()
        .find(|record| {
            record.channel.eq_ignore_ascii_case(weixin::WEIXIN_CHANNEL)
                && record.account_id.eq_ignore_ascii_case(&account_id)
        })
        .map(|record| record.center_account_id.clone());
    for record in existing_bindings {
        if record.channel.eq_ignore_ascii_case(weixin::WEIXIN_CHANNEL)
            && record.account_id.eq_ignore_ascii_case(&account_id)
        {
            continue;
        }
        delete_bridge_center_account_cascade(&state, &record.center_account_id)?;
    }

    let bridge_record = build_bridge_center_account_record(
        &state,
        &center,
        BridgeCenterAccountUpsertPayload {
            center_account_id: reused_center_account_id,
            center_id: Some(center.center_id.clone()),
            channel: weixin::WEIXIN_CHANNEL.to_string(),
            account_id,
            enabled: Some(true),
            default_preset_agent_name_override: None,
            identity_strategy: None,
            thread_strategy: None,
            reply_strategy: None,
            fallback_policy: None,
            status_reason: None,
        },
    )
    .await?;
    state
        .storage
        .upsert_bridge_center_account(&bridge_record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let weixin_cfg = ChannelAccountConfig::from_value(&channel_account.config)
        .weixin
        .unwrap_or_default();
    let long_connection_enabled = weixin::long_connection_enabled(&weixin_cfg);
    let configured = weixin::has_long_connection_credentials(&weixin_cfg);
    if configured {
        state.channels.record_runtime_info(
            weixin::WEIXIN_CHANNEL,
            Some(&channel_account.account_id),
            "weixin_config_ready",
            format!(
                "bridge weixin config ready; runtime_mode=long_poll, long_connection_enabled={long_connection_enabled}"
            ),
        );
    } else {
        state.channels.record_runtime_warn(
            weixin::WEIXIN_CHANNEL,
            Some(&channel_account.account_id),
            "weixin_config_incomplete",
            "bridge weixin config incomplete: api_base/bot_token/ilink_bot_id missing".to_string(),
        );
    }

    let overview = load_bridge_overview_maps(&state)?;
    Ok(Json(json!({
        "data": {
            "center": bridge_center_payload(&state, &center, &overview)?,
            "account": bridge_center_account_payload(&bridge_record, &overview),
            "channel_account": {
                "channel": channel_account.channel,
                "account_id": channel_account.account_id,
                "status": channel_account.status,
                "updated_at": channel_account.updated_at,
                "config": channel_account.config,
            },
        }
    })))
}

async fn admin_bridge_routes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<BridgeRoutesQuery>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    let limit = normalize_limit(query.limit);
    let offset = query.offset.unwrap_or(0).max(0);
    let overview = load_bridge_overview_maps(&state)?;
    let (items, total) = state
        .storage
        .list_bridge_user_routes(ListBridgeUserRoutesQuery {
            center_id: query.center_id.as_deref(),
            center_account_id: query.center_account_id.as_deref(),
            channel: query.channel.as_deref(),
            account_id: query.account_id.as_deref(),
            status: query.status.as_deref(),
            keyword: query.keyword.as_deref(),
            wunder_user_id: query.wunder_user_id.as_deref(),
            agent_id: query.agent_id.as_deref(),
            external_identity_key: None,
            offset,
            limit,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = items
        .iter()
        .map(|record| bridge_route_payload(&state, record, &overview))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({
        "data": {
            "total": total,
            "items": items,
        }
    })))
}

async fn admin_bridge_route_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(route_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    let route = state
        .storage
        .get_bridge_user_route(route_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(StatusCode::NOT_FOUND, "bridge route not found".to_string())
        })?;
    let overview = load_bridge_overview_maps(&state)?;
    let logs = state
        .storage
        .list_bridge_delivery_logs(ListBridgeDeliveryLogsQuery {
            center_id: Some(route.center_id.as_str()),
            center_account_id: Some(route.center_account_id.as_str()),
            route_id: Some(route.route_id.as_str()),
            direction: None,
            status: None,
            limit: 50,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let audit_logs = state
        .storage
        .list_bridge_route_audit_logs(ListBridgeRouteAuditLogsQuery {
            center_id: Some(route.center_id.as_str()),
            route_id: Some(route.route_id.as_str()),
            limit: 50,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "route": bridge_route_payload(&state, &route, &overview)?,
            "delivery_logs": logs
                .iter()
                .map(bridge_delivery_log_payload)
                .collect::<Vec<_>>(),
            "audit_logs": audit_logs
                .iter()
                .map(bridge_route_audit_payload)
                .collect::<Vec<_>>(),
        }
    })))
}

async fn admin_bridge_route_patch(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(route_id): AxumPath<String>,
    Json(payload): Json<BridgeRoutePatchPayload>,
) -> Result<Json<Value>, Response> {
    let admin = ensure_admin_user(&state, &headers)?;
    let route_id = required_text(&route_id, "route_id")?;
    let mut route = state
        .storage
        .get_bridge_user_route(&route_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(StatusCode::NOT_FOUND, "bridge route not found".to_string())
        })?;
    if let Some(status) = payload.status.as_deref() {
        route.status = normalize_bridge_route_status(status);
    }
    if payload.clear_last_error.unwrap_or(false) {
        route.last_error = None;
    }
    route.updated_at = now_ts();
    state
        .storage
        .upsert_bridge_user_route(&route)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let audit = BridgeRouteAuditLogRecord {
        audit_id: format!("bal_{}", Uuid::new_v4().simple()),
        center_id: route.center_id.clone(),
        route_id: Some(route.route_id.clone()),
        actor_type: "admin".to_string(),
        actor_id: admin.user_id,
        action: "route_status_update".to_string(),
        detail: Some(json!({
            "status": route.status,
            "clear_last_error": payload.clear_last_error.unwrap_or(false),
        })),
        created_at: now_ts(),
    };
    state
        .storage
        .insert_bridge_route_audit_log(&audit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let overview = load_bridge_overview_maps(&state)?;
    Ok(Json(json!({
        "data": {
            "route": bridge_route_payload(&state, &route, &overview)?,
        }
    })))
}

async fn admin_bridge_delivery_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<BridgeDeliveryLogsQuery>,
) -> Result<Json<Value>, Response> {
    ensure_admin_user(&state, &headers)?;
    let logs = state
        .storage
        .list_bridge_delivery_logs(ListBridgeDeliveryLogsQuery {
            center_id: query.center_id.as_deref(),
            center_account_id: query.center_account_id.as_deref(),
            route_id: query.route_id.as_deref(),
            direction: query.direction.as_deref(),
            status: query.status.as_deref(),
            limit: normalize_limit(query.limit),
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": logs
                .iter()
                .map(bridge_delivery_log_payload)
                .collect::<Vec<_>>(),
        }
    })))
}

async fn upsert_bridge_center_account(
    state: &Arc<AppState>,
    payload: BridgeCenterAccountUpsertPayload,
    path_center_id: Option<String>,
) -> Result<Json<Value>, Response> {
    let center_id = path_center_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| clean_optional_text(payload.center_id.as_deref()))
        .ok_or_else(|| {
            error_response(StatusCode::BAD_REQUEST, "center_id is required".to_string())
        })?;
    let center = state
        .storage
        .get_bridge_center(&center_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "bridge center not found".to_string(),
            )
        })?;
    let record = build_bridge_center_account_record(state, &center, payload).await?;
    let existing_accounts = state
        .storage
        .list_bridge_center_accounts(ListBridgeCenterAccountsQuery {
            center_id: Some(center.center_id.as_str()),
            channel: None,
            account_id: None,
            enabled: None,
            offset: 0,
            limit: BRIDGE_OVERVIEW_LIMIT,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .0;
    if existing_accounts
        .iter()
        .any(|item| item.center_account_id != record.center_account_id)
    {
        return Err(error_response(
            StatusCode::CONFLICT,
            format!(
                "each bridge node supports only one channel: {}",
                center.center_id
            ),
        ));
    }
    state
        .storage
        .upsert_bridge_center_account(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let overview = load_bridge_overview_maps(state)?;
    Ok(Json(json!({
        "data": {
            "center": bridge_center_payload(state, &center, &overview)?,
            "account": bridge_center_account_payload(&record, &overview),
        }
    })))
}

async fn sync_bridge_center_accounts(
    state: &Arc<AppState>,
    center: &BridgeCenterRecord,
    shared_channels: Vec<BridgeCenterAccountUpsertPayload>,
) -> Result<(), Response> {
    if shared_channels.len() > 1 {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            format!(
                "each bridge node supports only one channel: {}",
                center.center_id
            ),
        ));
    }
    let existing_accounts = state
        .storage
        .list_bridge_center_accounts(ListBridgeCenterAccountsQuery {
            center_id: Some(center.center_id.as_str()),
            channel: None,
            account_id: None,
            enabled: None,
            offset: 0,
            limit: BRIDGE_OVERVIEW_LIMIT,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .0;
    let mut seen_keys = HashSet::new();
    let mut next_records = Vec::with_capacity(shared_channels.len());
    for payload in shared_channels {
        let account_key = bridge_center_account_key(
            required_text(&payload.channel, "channel")?
                .to_ascii_lowercase()
                .as_str(),
            required_text(&payload.account_id, "account_id")?.as_str(),
        );
        if !seen_keys.insert(account_key.clone()) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("duplicate shared channel in node payload: {account_key}"),
            ));
        }
        let record = build_bridge_center_account_record(state, center, payload).await?;
        next_records.push(record);
    }
    let keep_ids = next_records
        .iter()
        .map(|item| item.center_account_id.clone())
        .collect::<HashSet<_>>();

    for record in next_records {
        state
            .storage
            .upsert_bridge_center_account(&record)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }

    for existing in existing_accounts {
        if !keep_ids.contains(&existing.center_account_id) {
            delete_bridge_center_account_cascade(state, &existing.center_account_id)?;
        }
    }
    Ok(())
}

async fn build_bridge_center_account_record(
    state: &Arc<AppState>,
    center: &BridgeCenterRecord,
    payload: BridgeCenterAccountUpsertPayload,
) -> Result<BridgeCenterAccountRecord, Response> {
    let channel = required_text(&payload.channel, "channel")?.to_ascii_lowercase();
    let account_id = required_text(&payload.account_id, "account_id")?;
    ensure_channel_account_exists(state, &channel, &account_id)?;
    if let Some(override_name) = payload.default_preset_agent_name_override.as_deref() {
        ensure_preset_exists(state, override_name).await?;
    }
    let requested_center_account_id = clean_optional_text(payload.center_account_id.as_deref());
    let existing_by_binding = state
        .storage
        .get_bridge_center_account_by_channel_account(&channel, &account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Some(existing) = existing_by_binding.as_ref() {
        if existing.center_id != center.center_id {
            return Err(error_response(
                StatusCode::CONFLICT,
                format!("shared channel already attached to another node: {channel}/{account_id}"),
            ));
        }
    }
    let existing_by_id = if let Some(center_account_id) = requested_center_account_id.as_deref() {
        let record = state
            .storage
            .get_bridge_center_account(center_account_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if let Some(existing) = record.as_ref() {
            if existing.center_id != center.center_id {
                return Err(error_response(
                    StatusCode::CONFLICT,
                    format!(
                        "center_account_id already belongs to another node: {center_account_id}"
                    ),
                ));
            }
        }
        record
    } else {
        None
    };
    let reused_record = resolve_bridge_center_account_record(
        existing_by_id,
        existing_by_binding,
        &channel,
        &account_id,
    )?;
    let now = now_ts();
    let center_account_id = reused_record
        .as_ref()
        .map(|record| record.center_account_id.clone())
        .or(requested_center_account_id)
        .unwrap_or_else(|| format!("bca_{}", Uuid::new_v4().simple()));
    let created_at = reused_record
        .as_ref()
        .map(|record| record.created_at)
        .unwrap_or(now);
    let adapter_registered = state.channels.adapter_registry().get(&channel).is_some();
    Ok(BridgeCenterAccountRecord {
        center_account_id,
        center_id: center.center_id.clone(),
        channel: channel.clone(),
        account_id,
        enabled: payload.enabled.unwrap_or(true),
        default_preset_agent_name_override: clean_optional_text(
            payload.default_preset_agent_name_override.as_deref(),
        ),
        identity_strategy: clean_optional_text(payload.identity_strategy.as_deref())
            .map(|value| normalize_bridge_identity_strategy(&value)),
        thread_strategy: clean_optional_text(payload.thread_strategy.as_deref())
            .map(|value| normalize_bridge_thread_strategy(&value)),
        reply_strategy: Some(normalize_bridge_reply_strategy(
            payload.reply_strategy.as_deref().unwrap_or("reply_only"),
        )),
        fallback_policy: normalize_bridge_fallback_policy(
            payload
                .fallback_policy
                .as_deref()
                .unwrap_or(BRIDGE_FALLBACK_POLICY_FORBID_OWNER),
        ),
        provider_caps: Some(build_bridge_provider_caps(&channel, adapter_registered)),
        status_reason: clean_optional_text(payload.status_reason.as_deref()),
        created_at,
        updated_at: now,
    })
}

fn resolve_bridge_center_account_record(
    existing_by_id: Option<BridgeCenterAccountRecord>,
    existing_by_binding: Option<BridgeCenterAccountRecord>,
    channel: &str,
    account_id: &str,
) -> Result<Option<BridgeCenterAccountRecord>, Response> {
    match (existing_by_id, existing_by_binding) {
        (Some(by_id), Some(by_binding)) => {
            if by_id.center_account_id != by_binding.center_account_id {
                return Err(error_response(
                    StatusCode::CONFLICT,
                    format!("conflicting shared channel records found for {channel}/{account_id}"),
                ));
            }
            Ok(Some(by_id))
        }
        (Some(by_id), None) => Ok(Some(by_id)),
        (None, Some(by_binding)) => Ok(Some(by_binding)),
        (None, None) => Ok(None),
    }
}

fn bridge_center_account_key(channel: &str, account_id: &str) -> String {
    format!(
        "{}:{}",
        channel.trim().to_ascii_lowercase(),
        account_id.trim()
    )
}

fn build_bridge_weixin_account_id(center: &BridgeCenterRecord) -> String {
    let normalized = center
        .center_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if normalized.is_empty() {
        return format!("bridge_weixin_{}", Uuid::new_v4().simple());
    }
    format!("bridge_weixin_{normalized}")
}

fn delete_bridge_center_account_cascade(
    state: &Arc<AppState>,
    center_account_id: &str,
) -> Result<(), Response> {
    state
        .storage
        .delete_bridge_delivery_logs_by_center_account(center_account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state
        .storage
        .delete_bridge_route_audit_logs_by_center_account(center_account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state
        .storage
        .delete_bridge_user_routes_by_center_account(center_account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state
        .storage
        .delete_bridge_center_account(center_account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(())
}

fn bridge_center_payload(
    state: &Arc<AppState>,
    record: &BridgeCenterRecord,
    overview: &BridgeOverviewMaps,
) -> Result<Value, Response> {
    let owner = state
        .user_store
        .get_user_by_id(&record.owner_user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(json!({
        "center_id": record.center_id,
        "name": record.name,
        "code": record.code,
        "description": record.description,
        "owner_user_id": record.owner_user_id,
        "owner_username": owner.as_ref().map(|user| user.username.clone()),
        "status": record.status,
        "default_preset_agent_name": record.default_preset_agent_name,
        "target_unit_id": record.target_unit_id,
        "default_identity_strategy": record.default_identity_strategy,
        "username_policy": record.username_policy,
        "password_policy": record.password_policy,
        "settings": record.settings,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "account_count": overview
            .center_account_count
            .get(&record.center_id)
            .copied()
            .unwrap_or(0),
        "shared_channel_count": overview
            .center_account_count
            .get(&record.center_id)
            .copied()
            .unwrap_or(0),
        "route_count": overview
            .center_route_count
            .get(&record.center_id)
            .copied()
            .unwrap_or(0),
        "active_route_count": overview
            .center_active_route_count
            .get(&record.center_id)
            .copied()
            .unwrap_or(0),
    }))
}

fn bridge_center_account_payload(
    record: &BridgeCenterAccountRecord,
    overview: &BridgeOverviewMaps,
) -> Value {
    json!({
        "center_account_id": record.center_account_id,
        "center_id": record.center_id,
        "center_name": overview.center_name.get(&record.center_id),
        "channel": record.channel,
        "account_id": record.account_id,
        "enabled": record.enabled,
        "default_preset_agent_name_override": record.default_preset_agent_name_override,
        "identity_strategy": record.identity_strategy,
        "thread_strategy": record.thread_strategy,
        "reply_strategy": record.reply_strategy,
        "fallback_policy": record.fallback_policy,
        "provider_caps": record.provider_caps,
        "status_reason": record.status_reason,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "route_count": overview
            .account_route_count
            .get(&record.center_account_id)
            .copied()
            .unwrap_or(0),
        "active_route_count": overview
            .account_active_route_count
            .get(&record.center_account_id)
            .copied()
            .unwrap_or(0),
    })
}

fn bridge_route_payload(
    state: &Arc<AppState>,
    record: &BridgeUserRouteRecord,
    overview: &BridgeOverviewMaps,
) -> Result<Value, Response> {
    let user = state
        .user_store
        .get_user_by_id(&record.wunder_user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(json!({
        "route_id": record.route_id,
        "center_id": record.center_id,
        "center_name": overview.center_name.get(&record.center_id),
        "center_account_id": record.center_account_id,
        "channel": record.channel,
        "account_id": record.account_id,
        "external_identity_key": record.external_identity_key,
        "external_user_key": record.external_user_key,
        "external_display_name": record.external_display_name,
        "external_peer_id": record.external_peer_id,
        "external_sender_id": record.external_sender_id,
        "external_thread_id": record.external_thread_id,
        "external_profile": record.external_profile,
        "wunder_user_id": record.wunder_user_id,
        "wunder_username": user.as_ref().map(|item| item.username.clone()),
        "agent_id": record.agent_id,
        "agent_name": record.agent_name,
        "user_created": record.user_created,
        "agent_created": record.agent_created,
        "status": record.status,
        "last_session_id": record.last_session_id,
        "last_error": record.last_error,
        "first_seen_at": record.first_seen_at,
        "last_seen_at": record.last_seen_at,
        "last_inbound_at": record.last_inbound_at,
        "last_outbound_at": record.last_outbound_at,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    }))
}

fn bridge_delivery_log_payload(record: &BridgeDeliveryLogRecord) -> Value {
    json!({
        "delivery_id": record.delivery_id,
        "center_id": record.center_id,
        "center_account_id": record.center_account_id,
        "route_id": record.route_id,
        "direction": record.direction,
        "stage": record.stage,
        "provider_message_id": record.provider_message_id,
        "session_id": record.session_id,
        "status": record.status,
        "summary": record.summary,
        "payload": record.payload,
        "created_at": record.created_at,
    })
}

fn bridge_route_audit_payload(record: &BridgeRouteAuditLogRecord) -> Value {
    json!({
        "audit_id": record.audit_id,
        "center_id": record.center_id,
        "route_id": record.route_id,
        "actor_type": record.actor_type,
        "actor_id": record.actor_id,
        "action": record.action,
        "detail": record.detail,
        "created_at": record.created_at,
    })
}

fn supported_channel_payload(state: &Arc<AppState>) -> Vec<Value> {
    let registry = state.channels.adapter_registry();
    user_supported_channels()
        .into_iter()
        .map(|item| {
            let adapter_registered = registry.get(item.channel).is_some();
            json!({
                "channel": item.channel,
                "display_name": item.display_name,
                "description": item.description,
                "webhook_mode": item.webhook_mode,
                "docs_hint": item.docs_hint,
                "adapter_registered": adapter_registered,
                "provider_caps": build_bridge_provider_caps(item.channel, adapter_registered),
            })
        })
        .collect()
}

fn load_bridge_overview_maps(state: &Arc<AppState>) -> Result<BridgeOverviewMaps, Response> {
    let centers = state
        .storage
        .list_bridge_centers(ListBridgeCentersQuery {
            status: None,
            keyword: None,
            offset: 0,
            limit: BRIDGE_OVERVIEW_LIMIT,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .0;
    let accounts = state
        .storage
        .list_bridge_center_accounts(ListBridgeCenterAccountsQuery {
            center_id: None,
            channel: None,
            account_id: None,
            enabled: None,
            offset: 0,
            limit: BRIDGE_OVERVIEW_LIMIT,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .0;
    let routes = state
        .storage
        .list_bridge_user_routes(ListBridgeUserRoutesQuery {
            center_id: None,
            center_account_id: None,
            channel: None,
            account_id: None,
            status: None,
            keyword: None,
            wunder_user_id: None,
            agent_id: None,
            external_identity_key: None,
            offset: 0,
            limit: BRIDGE_OVERVIEW_LIMIT,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .0;
    let mut overview = BridgeOverviewMaps::default();
    for center in centers {
        overview
            .center_name
            .insert(center.center_id.clone(), center.name.clone());
    }
    for account in accounts {
        *overview
            .center_account_count
            .entry(account.center_id.clone())
            .or_insert(0) += 1;
    }
    for route in routes {
        *overview
            .center_route_count
            .entry(route.center_id.clone())
            .or_insert(0) += 1;
        *overview
            .account_route_count
            .entry(route.center_account_id.clone())
            .or_insert(0) += 1;
        if normalize_bridge_route_status(&route.status) == BRIDGE_ROUTE_STATUS_ACTIVE {
            *overview
                .center_active_route_count
                .entry(route.center_id.clone())
                .or_insert(0) += 1;
            *overview
                .account_active_route_count
                .entry(route.center_account_id.clone())
                .or_insert(0) += 1;
        }
    }
    Ok(overview)
}

fn ensure_channel_account_exists(
    state: &Arc<AppState>,
    channel: &str,
    account_id: &str,
) -> Result<(), Response> {
    state
        .storage
        .get_channel_account(channel, account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                format!("channel account not found: {channel}/{account_id}"),
            )
        })?;
    Ok(())
}

fn ensure_org_unit_exists(state: &Arc<AppState>, unit_id: &str) -> Result<(), Response> {
    state
        .user_store
        .get_org_unit(unit_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(StatusCode::BAD_REQUEST, "target unit not found".to_string())
        })?;
    Ok(())
}

async fn ensure_preset_exists(state: &Arc<AppState>, preset_name: &str) -> Result<(), Response> {
    let cleaned = required_text(preset_name, "preset_name")?;
    let default_record = load_effective_default_agent_record(state.as_ref(), PRESET_TEMPLATE_USER_ID)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let default_name = default_record.name.trim();
    if (!default_name.is_empty() && default_name == cleaned)
        || cleaned.eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS)
    {
        return Ok(());
    }
    let exists = configured_preset_agents(state.as_ref())
        .await
        .iter()
        .any(|item| {
            item.name.trim() == cleaned || item.preset_id.trim().eq_ignore_ascii_case(&cleaned)
        });
    if exists {
        return Ok(());
    }
    Err(error_response(
        StatusCode::BAD_REQUEST,
        format!("preset agent not found: {cleaned}"),
    ))
}

fn ensure_admin_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<crate::storage::UserAccountRecord, Response> {
    let Some(token) = guard_auth::extract_bearer_token(headers) else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "auth required".to_string(),
        ));
    };
    let user = state
        .user_store
        .authenticate_token(&token)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::UNAUTHORIZED, "auth required".to_string()))?;
    if !UserStore::is_admin(&user) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "admin required".to_string(),
        ));
    }
    Ok(user)
}

fn required_text(raw: &str, field: &str) -> Result<String, Response> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            format!("{field} is required"),
        ));
    }
    Ok(cleaned.to_string())
}

fn clean_optional_text(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(100).clamp(1, BRIDGE_LIST_LIMIT_MAX)
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

#[cfg(test)]
mod tests {
    use super::{
        bridge_center_account_key, build_bridge_weixin_account_id,
        resolve_bridge_center_account_record, BridgeCenterAccountUpsertPayload,
        BridgeCenterUpsertPayload,
    };
    use crate::storage::{BridgeCenterAccountRecord, BridgeCenterRecord};
    use futures::executor::block_on;
    use serde_json::json;

    fn sample_center_account_record(
        center_account_id: &str,
        center_id: &str,
        channel: &str,
        account_id: &str,
    ) -> BridgeCenterAccountRecord {
        BridgeCenterAccountRecord {
            center_account_id: center_account_id.to_string(),
            center_id: center_id.to_string(),
            channel: channel.to_string(),
            account_id: account_id.to_string(),
            enabled: true,
            default_preset_agent_name_override: None,
            identity_strategy: None,
            thread_strategy: None,
            reply_strategy: None,
            fallback_policy: "forbid_owner".to_string(),
            provider_caps: None,
            status_reason: None,
            created_at: 1.0,
            updated_at: 1.0,
        }
    }

    #[test]
    fn bridge_center_upsert_payload_supports_embedded_shared_channels() {
        let payload: BridgeCenterUpsertPayload = serde_json::from_value(json!({
            "name": "舰桥节点",
            "code": "service_center",
            "default_preset_agent_name": "客服助手",
            "shared_channels": [
                {
                    "channel": "xmpp",
                    "account_id": "support@example.com",
                    "enabled": true,
                    "thread_strategy": "main_thread"
                }
            ]
        }))
        .expect("payload should deserialize");

        assert_eq!(payload.name, "舰桥节点");
        assert_eq!(payload.code, "service_center");
        assert_eq!(payload.default_preset_agent_name, "客服助手");
        assert_eq!(
            payload.shared_channels,
            Some(vec![BridgeCenterAccountUpsertPayload {
                center_account_id: None,
                center_id: None,
                channel: "xmpp".to_string(),
                account_id: "support@example.com".to_string(),
                enabled: Some(true),
                default_preset_agent_name_override: None,
                identity_strategy: None,
                thread_strategy: Some("main_thread".to_string()),
                reply_strategy: None,
                fallback_policy: None,
                status_reason: None,
            }])
        );
    }

    #[test]
    fn bridge_center_upsert_payload_allows_missing_shared_channels() {
        let payload: BridgeCenterUpsertPayload = serde_json::from_value(json!({
            "name": "舰桥节点",
            "code": "service_center",
            "default_preset_agent_name": "客服助手"
        }))
        .expect("payload should deserialize");

        assert_eq!(payload.shared_channels, None);
    }

    #[test]
    fn bridge_center_account_key_normalizes_channel_case() {
        assert_eq!(
            bridge_center_account_key("XMPP", "support@example.com"),
            "xmpp:support@example.com"
        );
    }

    #[test]
    fn build_bridge_weixin_account_id_uses_stable_center_id() {
        let center = BridgeCenterRecord {
            center_id: "bc_AbC-123".to_string(),
            name: "舰桥".to_string(),
            code: "bridge".to_string(),
            description: None,
            owner_user_id: "admin".to_string(),
            status: "active".to_string(),
            default_preset_agent_name: "客服".to_string(),
            target_unit_id: None,
            default_identity_strategy: "sender_in_peer".to_string(),
            username_policy: "namespaced_generated".to_string(),
            password_policy: "fixed_default_123456".to_string(),
            settings: json!({}),
            created_at: 1.0,
            updated_at: 1.0,
        };

        assert_eq!(
            build_bridge_weixin_account_id(&center),
            "bridge_weixin_bc_abc_123"
        );
    }

    #[test]
    fn resolve_bridge_center_account_record_reuses_binding_match() {
        let record =
            sample_center_account_record("bca_existing", "bc_node", "xmpp", "support@example.com");
        let resolved = resolve_bridge_center_account_record(
            None,
            Some(record.clone()),
            "xmpp",
            "support@example.com",
        )
        .expect("record should resolve");
        assert_eq!(
            resolved
                .as_ref()
                .map(|item| item.center_account_id.as_str()),
            Some(record.center_account_id.as_str())
        );
    }

    #[test]
    fn resolve_bridge_center_account_record_rejects_conflicting_records() {
        let by_id =
            sample_center_account_record("bca_by_id", "bc_node", "xmpp", "support@example.com");
        let by_binding = sample_center_account_record(
            "bca_by_binding",
            "bc_node",
            "xmpp",
            "support@example.com",
        );
        let error = resolve_bridge_center_account_record(
            Some(by_id),
            Some(by_binding),
            "xmpp",
            "support@example.com",
        )
        .expect_err("conflict should fail");
        let body = block_on(axum::body::to_bytes(error.into_body(), usize::MAX))
            .expect("body read should succeed");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("body should be json");
        assert_eq!(
            payload["error"]["message"],
            "conflicting shared channel records found for xmpp/support@example.com"
        );
    }
}
