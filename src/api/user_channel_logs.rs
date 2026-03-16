use crate::api::user_context::resolve_user;
use crate::channels::catalog;
use crate::channels::types::ChannelAccountConfig;
use crate::i18n;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use uuid::Uuid;

const USER_CHANNEL_FEISHU: &str = "feishu";
const USER_CHANNEL_QQBOT: &str = "qqbot";
const USER_CHANNEL_WHATSAPP: &str = "whatsapp";
const USER_CHANNEL_WECHAT: &str = "wechat";
const USER_CHANNEL_WECHAT_MP: &str = "wechat_mp";
const USER_CHANNEL_XMPP: &str = "xmpp";

#[derive(Debug, Deserialize)]
struct ChannelRuntimeLogsQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/wunder/channels/runtime_logs",
        get(list_channel_runtime_logs),
    )
}

async fn list_channel_runtime_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ChannelRuntimeLogsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();

    let config = state.config_store.get().await;
    if !config.channels.enabled && !config.gateway.enabled {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "channels disabled".to_string(),
        ));
    }

    let channel_filter = query
        .channel
        .as_deref()
        .map(|value| normalize_user_channel(Some(value)))
        .transpose()?;
    let account_filter = query
        .account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let agent_filter = query
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let limit = query.limit.unwrap_or(80).clamp(1, 200);

    let account_keys = list_owned_account_keys_for_agent(
        &state,
        &user_id,
        channel_filter.as_deref(),
        agent_filter.as_deref(),
    )?;
    if account_keys.is_empty() {
        return Ok(Json(json!({ "data": { "items": [], "total": 0 } })));
    }
    if let Some(account_id) = account_filter.as_deref() {
        let channel = channel_filter.clone().unwrap_or_default();
        if !channel.is_empty() && !account_keys.contains(&(channel, account_id.to_string())) {
            return Ok(Json(json!({ "data": { "items": [], "total": 0 } })));
        }
    }

    let query_limit = (limit.saturating_mul(4)).clamp(limit, 400);
    let runtime_logs = state.channels.list_runtime_logs(
        channel_filter.as_deref(),
        account_filter.as_deref(),
        query_limit,
    );
    let mut items = Vec::new();
    for (index, item) in runtime_logs.into_iter().enumerate() {
        let channel = item.channel.trim().to_ascii_lowercase();
        let account_id = item.account_id.trim().to_string();
        if channel.is_empty() {
            continue;
        }
        let allowed = if account_id.is_empty() {
            account_keys
                .iter()
                .any(|(c, _)| c.eq_ignore_ascii_case(&channel))
        } else {
            account_keys.contains(&(channel.clone(), account_id.clone()))
        };
        if !allowed {
            continue;
        }
        items.push(json!({
            "id": format!("{channel}:{account_id}:{:.3}:{index}", item.ts),
            "ts": item.ts,
            "level": item.level,
            "channel": channel,
            "account_id": account_id,
            "event": item.event,
            "message": item.message,
            "repeat_count": item.repeat_count,
        }));
        if items.len() >= limit {
            break;
        }
    }

    Ok(Json(json!({ "data": {
        "items": items,
        "total": items.len(),
    } })))
}

fn list_owned_account_keys_for_agent(
    state: &Arc<AppState>,
    user_id: &str,
    channel_filter: Option<&str>,
    agent_filter: Option<&str>,
) -> Result<BTreeSet<(String, String)>, Response> {
    let all_owned = list_owned_account_keys(state, user_id, channel_filter)?;
    let Some(agent_id) = agent_filter else {
        return Ok(all_owned);
    };

    let (bindings, _) = state
        .storage
        .list_channel_user_bindings(crate::storage::ListChannelUserBindingsQuery {
            channel: channel_filter,
            account_id: None,
            peer_kind: None,
            peer_id: None,
            user_id: Some(user_id),
            offset: 0,
            limit: 1000,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let channel_bindings = state
        .storage
        .list_channel_bindings(channel_filter)
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

    let mut matched = BTreeSet::new();
    for record in bindings {
        let binding_id = make_user_binding_id(
            user_id,
            &record.channel,
            &record.account_id,
            &record.peer_kind,
            &record.peer_id,
        );
        let binding = binding_by_id.get(&binding_id).or_else(|| {
            binding_by_peer.get(&peer_key(
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
            ))
        });
        let Some(binding) = binding else {
            continue;
        };
        let binding_agent = binding
            .agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if binding_agent != Some(agent_id) || !binding.enabled {
            continue;
        }
        let channel = record.channel.trim().to_ascii_lowercase();
        let account_id = record.account_id.trim().to_string();
        if channel.is_empty() || account_id.is_empty() {
            continue;
        }
        matched.insert((channel, account_id));
    }

    if matched.is_empty() {
        let channel_accounts = state
            .storage
            .list_channel_accounts(channel_filter, Some("active"))
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        for record in channel_accounts {
            let channel = record.channel.trim().to_ascii_lowercase();
            let account_id = record.account_id.trim().to_string();
            if channel.is_empty() || account_id.is_empty() {
                continue;
            }
            let key = (channel, account_id);
            if !all_owned.contains(&key) {
                continue;
            }
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let account_agent = account_cfg
                .agent_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if account_agent == Some(agent_id) {
                matched.insert(key);
            }
        }
    }

    matched.retain(|item| all_owned.contains(item));
    Ok(matched)
}

fn list_owned_account_keys(
    state: &Arc<AppState>,
    user_id: &str,
    channel_filter: Option<&str>,
) -> Result<BTreeSet<(String, String)>, Response> {
    let mut account_keys: BTreeSet<(String, String)> = BTreeSet::new();
    let (bindings, _) = state
        .storage
        .list_channel_user_bindings(crate::storage::ListChannelUserBindingsQuery {
            channel: channel_filter,
            account_id: None,
            peer_kind: None,
            peer_id: None,
            user_id: Some(user_id),
            offset: 0,
            limit: 1000,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    for binding in bindings {
        let channel = binding.channel.trim().to_ascii_lowercase();
        if !is_supported_user_channel(&channel) {
            continue;
        }
        let account_id = binding.account_id.trim().to_string();
        if account_id.is_empty() {
            continue;
        }
        account_keys.insert((channel, account_id));
    }

    for channel in resolve_user_channels(channel_filter)? {
        let legacy_account_id = make_legacy_user_account_id(user_id, &channel);
        let legacy_record = state
            .storage
            .get_channel_account(&channel, &legacy_account_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if legacy_record.is_some() {
            account_keys.insert((channel, legacy_account_id));
        }
    }
    Ok(account_keys)
}

fn normalize_user_channel(channel: Option<&str>) -> Result<String, Response> {
    let channel = channel
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error_response(StatusCode::BAD_REQUEST, i18n::t("error.content_required"))
        })?;
    let normalized = channel.to_ascii_lowercase();
    if is_supported_user_channel(&normalized) {
        return Ok(normalized);
    }
    Err(error_response(
        StatusCode::BAD_REQUEST,
        "unsupported channel".to_string(),
    ))
}

fn resolve_user_channels(channel: Option<&str>) -> Result<Vec<String>, Response> {
    if let Some(channel) = channel {
        return Ok(vec![normalize_user_channel(Some(channel))?]);
    }
    Ok(catalog::user_supported_channel_names()
        .into_iter()
        .map(str::to_string)
        .collect())
}

fn is_supported_user_channel(channel: &str) -> bool {
    matches!(
        channel,
        USER_CHANNEL_FEISHU
            | USER_CHANNEL_QQBOT
            | USER_CHANNEL_WHATSAPP
            | USER_CHANNEL_WECHAT
            | USER_CHANNEL_WECHAT_MP
            | USER_CHANNEL_XMPP
    )
}

fn make_legacy_user_account_id(user_id: &str, channel: &str) -> String {
    let key = format!(
        "uacc:{user_id}|{channel}",
        user_id = user_id.trim().to_ascii_lowercase(),
        channel = channel.trim().to_ascii_lowercase(),
    );
    format!(
        "uacc_{}",
        Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes()).simple()
    )
}

fn make_user_binding_id(
    user_id: &str,
    channel: &str,
    account_id: &str,
    peer_kind: &str,
    peer_id: &str,
) -> String {
    let key = format!(
        "ubind:{user_id}|{channel}|{account_id}|{peer_kind}|{peer_id}",
        user_id = user_id.trim().to_ascii_lowercase(),
        channel = channel.trim().to_ascii_lowercase(),
        account_id = account_id.trim().to_ascii_lowercase(),
        peer_kind = peer_kind.trim().to_ascii_lowercase(),
        peer_id = peer_id.trim().to_ascii_lowercase(),
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
        peer_id.trim().to_ascii_lowercase(),
    )
}

fn error_response(status: StatusCode, message: String) -> Response {
    let mut response = Json(json!({ "error": message })).into_response();
    *response.status_mut() = status;
    response
}
