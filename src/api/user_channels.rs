use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::state::AppState;
use crate::user_access::is_agent_allowed;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{
    routing::{delete, get},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct ChannelAccountsQuery {
    #[serde(default)]
    channel: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelBindingsQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    peer_kind: Option<String>,
    #[serde(default)]
    peer_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelBindingUpsertRequest {
    channel: String,
    account_id: String,
    peer_kind: String,
    peer_id: String,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    tool_overrides: Option<Vec<String>>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    priority: Option<i64>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/channels/accounts", get(list_channel_accounts))
        .route(
            "/wunder/channels/bindings",
            get(list_channel_bindings).post(upsert_channel_binding),
        )
        .route(
            "/wunder/channels/bindings/{channel}/{account_id}/{peer_kind}/{peer_id}",
            delete(delete_channel_binding),
        )
}

async fn list_channel_accounts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ChannelAccountsQuery>,
) -> Result<Json<Value>, Response> {
    let _resolved = resolve_user(&state, &headers, None).await?;
    let config = state.config_store.get().await;
    if !config.channels.enabled && !config.gateway.enabled {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "channels disabled".to_string(),
        ));
    }
    let channel = query
        .channel
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let records = state
        .storage
        .list_channel_accounts(channel, Some("active"))
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .into_iter()
        .map(|record| {
            json!({
                "channel": record.channel,
                "account_id": record.account_id,
                "status": record.status,
                "created_at": record.created_at,
                "updated_at": record.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn list_channel_bindings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ChannelBindingsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let (bindings, total) = state
        .storage
        .list_channel_user_bindings(crate::storage::ListChannelUserBindingsQuery {
            channel: query.channel.as_deref(),
            account_id: query.account_id.as_deref(),
            peer_kind: query.peer_kind.as_deref(),
            peer_id: query.peer_id.as_deref(),
            user_id: Some(&user_id),
            offset: 0,
            limit: 200,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let channel_bindings = state
        .storage
        .list_channel_bindings(query.channel.as_deref())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut binding_by_id: HashMap<String, crate::storage::ChannelBindingRecord> = HashMap::new();
    let mut binding_by_peer: HashMap<String, crate::storage::ChannelBindingRecord> = HashMap::new();
    for record in channel_bindings {
        binding_by_id.insert(record.binding_id.clone(), record.clone());
        if let (Some(peer_kind), Some(peer_id)) =
            (record.peer_kind.as_ref(), record.peer_id.as_ref())
        {
            let key = peer_key(&record.channel, &record.account_id, peer_kind, peer_id);
            let replace = match binding_by_peer.get(&key) {
                Some(existing) => record.priority > existing.priority,
                None => true,
            };
            if replace {
                binding_by_peer.insert(key, record);
            }
        }
    }
    let items = bindings
        .into_iter()
        .map(|record| {
            let binding_id = make_user_binding_id(
                &user_id,
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
            );
            let binding = binding_by_id
                .get(&binding_id)
                .cloned()
                .or_else(|| {
                    binding_by_peer
                        .get(&peer_key(
                            &record.channel,
                            &record.account_id,
                            &record.peer_kind,
                            &record.peer_id,
                        ))
                        .cloned()
                });
            json!({
                "binding_id": binding_id,
                "channel": record.channel,
                "account_id": record.account_id,
                "peer_kind": record.peer_kind,
                "peer_id": record.peer_id,
                "user_id": record.user_id,
                "agent_id": binding.as_ref().and_then(|item| item.agent_id.clone()),
                "tool_overrides": binding.as_ref().map(|item| item.tool_overrides.clone()).unwrap_or_default(),
                "priority": binding.as_ref().map(|item| item.priority).unwrap_or(0),
                "enabled": binding.as_ref().map(|item| item.enabled).unwrap_or(false),
                "created_at": record.created_at,
                "updated_at": record.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items, "total": total } })))
}

async fn upsert_channel_binding(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ChannelBindingUpsertRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let channel = payload.channel.trim().to_string();
    let account_id = payload.account_id.trim().to_string();
    let peer_kind = payload.peer_kind.trim().to_string();
    let peer_id = payload.peer_id.trim().to_string();
    if channel.is_empty() || account_id.is_empty() || peer_kind.is_empty() || peer_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let config = state.config_store.get().await;
    if !config.channels.enabled && !config.gateway.enabled {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "channels disabled".to_string(),
        ));
    }
    let account = state
        .storage
        .get_channel_account(&channel, &account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                "channel account not found".to_string(),
            )
        })?;
    if account.status.trim().to_lowercase() != "active" {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "channel account disabled".to_string(),
        ));
    }
    let agent_id = payload
        .agent_id
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(agent_id) = agent_id.as_ref() {
        let record = state
            .user_store
            .get_user_agent_by_id(agent_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .ok_or_else(|| {
                error_response(StatusCode::NOT_FOUND, i18n::t("error.agent_not_found"))
            })?;
        let access = state
            .user_store
            .get_user_agent_access(&user_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if !is_agent_allowed(&resolved.user, access.as_ref(), &record) {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.agent_not_found"),
            ));
        }
    }
    let binding_id = make_user_binding_id(&user_id, &channel, &account_id, &peer_kind, &peer_id);
    let now = now_ts();
    let record = crate::storage::ChannelBindingRecord {
        binding_id: binding_id.clone(),
        channel: channel.clone(),
        account_id: account_id.clone(),
        peer_kind: Some(peer_kind.clone()),
        peer_id: Some(peer_id.clone()),
        agent_id: agent_id.clone(),
        tool_overrides: payload.tool_overrides.unwrap_or_default(),
        priority: payload.priority.unwrap_or(100),
        enabled: payload.enabled.unwrap_or(true),
        created_at: now,
        updated_at: now,
    };
    state
        .storage
        .upsert_channel_binding(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let user_binding = crate::storage::ChannelUserBindingRecord {
        channel: channel.clone(),
        account_id: account_id.clone(),
        peer_kind: peer_kind.clone(),
        peer_id: peer_id.clone(),
        user_id: user_id.clone(),
        created_at: now,
        updated_at: now,
    };
    state
        .storage
        .upsert_channel_user_binding(&user_binding)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": {
        "binding_id": record.binding_id,
        "channel": record.channel,
        "account_id": record.account_id,
        "peer_kind": record.peer_kind,
        "peer_id": record.peer_id,
        "agent_id": record.agent_id,
        "tool_overrides": record.tool_overrides,
        "priority": record.priority,
        "enabled": record.enabled,
        "user_id": user_binding.user_id,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    }})))
}

async fn delete_channel_binding(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((channel, account_id, peer_kind, peer_id)): AxumPath<(String, String, String, String)>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let channel = channel.trim().to_string();
    let account_id = account_id.trim().to_string();
    let peer_kind = peer_kind.trim().to_string();
    let peer_id = peer_id.trim().to_string();
    if channel.is_empty() || account_id.is_empty() || peer_kind.is_empty() || peer_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let existing = state
        .storage
        .get_channel_user_binding(&channel, &account_id, &peer_kind, &peer_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Some(record) = existing {
        if record.user_id != user_id {
            return Err(error_response(
                StatusCode::FORBIDDEN,
                i18n::t("error.permission_denied"),
            ));
        }
    } else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "binding not found".to_string(),
        ));
    }
    let binding_id = make_user_binding_id(&user_id, &channel, &account_id, &peer_kind, &peer_id);
    let affected_binding = state
        .storage
        .delete_channel_binding(&binding_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let affected_user_binding = state
        .storage
        .delete_channel_user_binding(&channel, &account_id, &peer_kind, &peer_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": {
        "binding_id": binding_id,
        "deleted_bindings": affected_binding,
        "deleted_user_bindings": affected_user_binding,
    }})))
}

fn make_user_binding_id(
    user_id: &str,
    channel: &str,
    account_id: &str,
    peer_kind: &str,
    peer_id: &str,
) -> String {
    let key = format!(
        "user:{user_id}|{channel}|{account_id}|{peer_kind}|{peer_id}",
        user_id = user_id.trim().to_ascii_lowercase(),
        channel = channel.trim().to_ascii_lowercase(),
        account_id = account_id.trim().to_ascii_lowercase(),
        peer_kind = peer_kind.trim().to_ascii_lowercase(),
        peer_id = peer_id.trim()
    );
    format!(
        "ubind_{}",
        Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes()).simple()
    )
}

fn peer_key(channel: &str, account_id: &str, peer_kind: &str, peer_id: &str) -> String {
    format!(
        "{}:{}:{}:{}",
        channel.trim().to_ascii_lowercase(),
        account_id.trim().to_ascii_lowercase(),
        peer_kind.trim().to_ascii_lowercase(),
        peer_id.trim()
    )
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
