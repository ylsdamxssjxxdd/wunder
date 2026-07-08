use crate::api::admin::{error_response, now_ts, resolve_monitor_session_agent_name};
use crate::channels::feishu;
use crate::channels::types::ChannelAccountConfig;
use crate::channels::weixin;
use crate::channels::xmpp;
use crate::core::runtime_metrics;
use crate::i18n;
use crate::services::default_agent_sync::DEFAULT_AGENT_ID_ALIAS;
use crate::state::AppState;
use crate::storage::{ChannelAccountRecord, ListChannelUserBindingsQuery, StorageBackend};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::delete, routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/admin/channels/accounts",
            get(admin_channel_accounts),
        )
        .route(
            "/wunder/admin/channels/accounts/batch",
            post(admin_channel_accounts_batch),
        )
        .route(
            "/wunder/admin/channels/accounts/{channel}/{account_id}",
            delete(admin_channel_account_delete),
        )
        .route(
            "/wunder/admin/channels/accounts/{channel}/{account_id}/impact",
            get(admin_channel_account_delete_impact),
        )
        .route(
            "/wunder/admin/channels/bindings",
            get(admin_channel_bindings),
        )
        .route(
            "/wunder/admin/channels/user_bindings",
            get(admin_channel_user_bindings),
        )
        .route(
            "/wunder/admin/channels/sessions",
            get(admin_channel_sessions),
        )
        .route(
            "/wunder/admin/channels/runtime_logs",
            get(admin_channel_runtime_logs),
        )
        .route(
            "/wunder/admin/channels/runtime_logs/probe",
            post(admin_channel_runtime_probe),
        )
}

#[derive(Debug, Deserialize)]
struct ChannelAccountQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    owner_user_id: Option<String>,
    #[serde(default)]
    issue_only: Option<bool>,
    #[serde(default)]
    last_active_after: Option<f64>,
    #[serde(default)]
    last_active_before: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ChannelAccountBatchItemRequest {
    channel: String,
    account_id: String,
}

#[derive(Debug, Deserialize)]
struct ChannelAccountBatchRequest {
    action: String,
    #[serde(default)]
    items: Vec<ChannelAccountBatchItemRequest>,
}

#[derive(Debug, Clone, Copy)]
enum ChannelAccountBatchAction {
    Enable,
    Disable,
    Delete,
}

impl ChannelAccountBatchAction {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "enable" => Some(Self::Enable),
            "disable" => Some(Self::Disable),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Delete => "delete",
        }
    }

    fn target_status(self) -> Option<&'static str> {
        match self {
            Self::Enable => Some("active"),
            Self::Disable => Some("disabled"),
            Self::Delete => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChannelBindingQuery {
    #[serde(default)]
    channel: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelUserBindingQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    peer_kind: Option<String>,
    #[serde(default)]
    peer_id: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ChannelSessionQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    peer_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ChannelRuntimeLogsQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ChannelRuntimeLogsProbeRequest {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum LongConnectionRuntimeStatus {
    Running,
    MissingCredentials,
    Disabled,
    AccountInactive,
    NotConfigured,
}

impl LongConnectionRuntimeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::MissingCredentials => "missing_credentials",
            Self::Disabled => "disabled",
            Self::AccountInactive => "account_inactive",
            Self::NotConfigured => "not_configured",
        }
    }
}

fn resolve_channel_binding_count(
    storage: &dyn StorageBackend,
    channel: &str,
    account_id: &str,
) -> Result<i64, anyhow::Error> {
    let (_, total) = storage.list_channel_user_bindings(ListChannelUserBindingsQuery {
        channel: Some(channel),
        account_id: Some(account_id),
        peer_kind: None,
        peer_id: None,
        user_id: None,
        offset: 0,
        limit: 1,
    })?;
    Ok(total)
}

const ADMIN_CHANNEL_OWNER_PREVIEW_LIMIT: i64 = 200;
const ADMIN_CHANNEL_BATCH_LIMIT: usize = 500;
const ADMIN_CHANNEL_WILDCARD_PEER_ID: &str = "*";

fn normalize_channel_event_ts(raw: Option<f64>) -> Option<f64> {
    raw.filter(|value| value.is_finite() && *value > 0.0)
}

fn contains_ignore_ascii_case(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    haystack.to_ascii_lowercase().contains(needle_lower)
}

fn make_channel_user_binding_id(
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

fn build_channel_peer_key(
    channel: &str,
    account_id: &str,
    peer_kind: &str,
    peer_id: &str,
) -> String {
    format!(
        "{}:{}:{}:{}",
        channel.trim().to_ascii_lowercase(),
        account_id.trim().to_ascii_lowercase(),
        peer_kind.trim().to_ascii_lowercase(),
        peer_id.trim()
    )
}

fn runtime_connection_status_not_running(runtime: &Value, key: &str) -> bool {
    runtime
        .get(key)
        .and_then(|item| item.get("status"))
        .and_then(Value::as_str)
        .map(|status| !status.eq_ignore_ascii_case("running"))
        .unwrap_or(false)
}

fn runtime_has_issue(runtime: &Value) -> bool {
    runtime_connection_status_not_running(runtime, "feishu_long_connection")
        || runtime_connection_status_not_running(runtime, "weixin_long_connection")
        || runtime_connection_status_not_running(runtime, "xmpp_long_connection")
}

fn build_channel_account_runtime(
    storage: &dyn StorageBackend,
    record: &ChannelAccountRecord,
    binding_count: Option<i64>,
) -> Value {
    let account_cfg = ChannelAccountConfig::from_value(&record.config);
    if record
        .channel
        .trim()
        .eq_ignore_ascii_case(feishu::FEISHU_CHANNEL)
    {
        let resolved_binding_count = binding_count.or_else(|| {
            resolve_channel_binding_count(storage, &record.channel, &record.account_id).ok()
        });
        let Some(feishu_cfg) = account_cfg.feishu else {
            return json!({
                "feishu_long_connection": {
                    "status": LongConnectionRuntimeStatus::NotConfigured.as_str(),
                    "binding_count": resolved_binding_count.unwrap_or(0),
                    "long_connection_enabled": false,
                    "has_credentials": false,
                }
            });
        };

        let long_connection_enabled = feishu::long_connection_enabled(&feishu_cfg);
        let has_credentials = feishu::has_long_connection_credentials(&feishu_cfg);
        let account_active = record.status.trim().eq_ignore_ascii_case("active");
        let binding_count = resolved_binding_count;

        let status = if !account_active {
            LongConnectionRuntimeStatus::AccountInactive
        } else if !long_connection_enabled {
            LongConnectionRuntimeStatus::Disabled
        } else if !has_credentials {
            LongConnectionRuntimeStatus::MissingCredentials
        } else {
            LongConnectionRuntimeStatus::Running
        };

        return json!({
            "feishu_long_connection": {
                "status": status.as_str(),
                "binding_count": binding_count,
                "long_connection_enabled": long_connection_enabled,
                "has_credentials": has_credentials,
            }
        });
    }

    if record
        .channel
        .trim()
        .eq_ignore_ascii_case(weixin::WEIXIN_CHANNEL)
    {
        let resolved_binding_count = binding_count.or_else(|| {
            resolve_channel_binding_count(storage, &record.channel, &record.account_id).ok()
        });
        let Some(weixin_cfg) = account_cfg.weixin else {
            return json!({
                "weixin_long_connection": {
                    "status": LongConnectionRuntimeStatus::NotConfigured.as_str(),
                    "binding_count": resolved_binding_count.unwrap_or(0),
                    "long_connection_enabled": false,
                    "has_credentials": false,
                }
            });
        };

        let long_connection_enabled = weixin::long_connection_enabled(&weixin_cfg);
        let has_credentials = weixin::has_long_connection_credentials(&weixin_cfg);
        let account_active = record.status.trim().eq_ignore_ascii_case("active");
        let binding_count = resolved_binding_count;

        let status = if !account_active {
            LongConnectionRuntimeStatus::AccountInactive
        } else if !long_connection_enabled {
            LongConnectionRuntimeStatus::Disabled
        } else if !has_credentials {
            LongConnectionRuntimeStatus::MissingCredentials
        } else {
            LongConnectionRuntimeStatus::Running
        };

        return json!({
            "weixin_long_connection": {
                "status": status.as_str(),
                "binding_count": binding_count,
                "long_connection_enabled": long_connection_enabled,
                "has_credentials": has_credentials,
            }
        });
    }

    if record
        .channel
        .trim()
        .eq_ignore_ascii_case(xmpp::XMPP_CHANNEL)
    {
        let resolved_binding_count = binding_count.or_else(|| {
            resolve_channel_binding_count(storage, &record.channel, &record.account_id).ok()
        });
        let Some(xmpp_cfg) = account_cfg.xmpp else {
            return json!({
                "xmpp_long_connection": {
                    "status": LongConnectionRuntimeStatus::NotConfigured.as_str(),
                    "binding_count": resolved_binding_count.unwrap_or(0),
                    "long_connection_enabled": false,
                    "has_credentials": false,
                }
            });
        };

        let long_connection_enabled = xmpp::long_connection_enabled(&xmpp_cfg);
        let has_credentials = xmpp::has_long_connection_credentials(&xmpp_cfg);
        let account_active = record.status.trim().eq_ignore_ascii_case("active");
        let binding_count = resolved_binding_count;

        let status = if !account_active {
            LongConnectionRuntimeStatus::AccountInactive
        } else if !long_connection_enabled {
            LongConnectionRuntimeStatus::Disabled
        } else if !has_credentials {
            LongConnectionRuntimeStatus::MissingCredentials
        } else {
            LongConnectionRuntimeStatus::Running
        };

        return json!({
            "xmpp_long_connection": {
                "status": status.as_str(),
                "binding_count": binding_count,
                "long_connection_enabled": long_connection_enabled,
                "has_credentials": has_credentials,
            }
        });
    }

    json!({})
}

fn resolve_channel_owner_preview(
    state: &Arc<AppState>,
    channel: &str,
    account_id: &str,
    username_cache: &mut HashMap<String, String>,
    agent_name_cache: &mut HashMap<String, Option<String>>,
) -> Result<(Vec<Value>, i64), Response> {
    let (bindings, binding_total) = state
        .storage
        .list_channel_user_bindings(ListChannelUserBindingsQuery {
            channel: Some(channel),
            account_id: Some(account_id),
            peer_kind: None,
            peer_id: None,
            user_id: None,
            offset: 0,
            limit: ADMIN_CHANNEL_OWNER_PREVIEW_LIMIT,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let channel_bindings = state
        .storage
        .list_channel_bindings(Some(channel))
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut binding_by_id: HashMap<String, crate::storage::ChannelBindingRecord> = HashMap::new();
    let mut binding_by_peer: HashMap<String, crate::storage::ChannelBindingRecord> = HashMap::new();
    let mut wildcard_binding_by_kind: HashMap<String, crate::storage::ChannelBindingRecord> =
        HashMap::new();
    for binding in channel_bindings
        .into_iter()
        .filter(|item| item.account_id.eq_ignore_ascii_case(account_id))
    {
        binding_by_id.insert(binding.binding_id.clone(), binding.clone());
        if let (Some(peer_kind), Some(peer_id)) =
            (binding.peer_kind.as_ref(), binding.peer_id.as_ref())
        {
            let exact_key = build_channel_peer_key(channel, account_id, peer_kind, peer_id);
            let should_replace = binding_by_peer
                .get(&exact_key)
                .map(|existing| binding.priority > existing.priority)
                .unwrap_or(true);
            if should_replace {
                binding_by_peer.insert(exact_key, binding.clone());
            }
            if peer_id.trim() == ADMIN_CHANNEL_WILDCARD_PEER_ID {
                let wildcard_key = build_channel_peer_key(
                    channel,
                    account_id,
                    peer_kind,
                    ADMIN_CHANNEL_WILDCARD_PEER_ID,
                );
                let should_replace = wildcard_binding_by_kind
                    .get(&wildcard_key)
                    .map(|existing| binding.priority > existing.priority)
                    .unwrap_or(true);
                if should_replace {
                    wildcard_binding_by_kind.insert(wildcard_key, binding.clone());
                }
            }
        }
    }
    let mut owners = Vec::new();
    let mut seen = HashSet::new();
    for record in bindings {
        let user_id = record.user_id.trim();
        if user_id.is_empty() {
            continue;
        }
        let binding_id = make_channel_user_binding_id(
            user_id,
            &record.channel,
            &record.account_id,
            &record.peer_kind,
            &record.peer_id,
        );
        let exact_key = build_channel_peer_key(
            &record.channel,
            &record.account_id,
            &record.peer_kind,
            &record.peer_id,
        );
        let wildcard_key = build_channel_peer_key(
            &record.channel,
            &record.account_id,
            &record.peer_kind,
            ADMIN_CHANNEL_WILDCARD_PEER_ID,
        );
        let matched_binding = binding_by_id
            .get(&binding_id)
            .or_else(|| binding_by_peer.get(&exact_key))
            .or_else(|| wildcard_binding_by_kind.get(&wildcard_key));
        let agent_id = matched_binding
            .and_then(|item| item.agent_id.as_ref())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let dedup_key = format!(
            "{}::{}",
            user_id.to_ascii_lowercase(),
            agent_id
                .as_deref()
                .unwrap_or(DEFAULT_AGENT_ID_ALIAS)
                .to_ascii_lowercase()
        );
        if !seen.insert(dedup_key) {
            continue;
        }
        let username = if let Some(cached) = username_cache.get(user_id) {
            cached.clone()
        } else {
            let resolved = state
                .user_store
                .get_user_by_id(user_id)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
                .map(|user| {
                    let cleaned = user.username.trim();
                    if cleaned.is_empty() {
                        user.user_id.trim().to_string()
                    } else {
                        cleaned.to_string()
                    }
                })
                .unwrap_or_else(|| user_id.to_string());
            username_cache.insert(user_id.to_string(), resolved.clone());
            resolved
        };
        let agent_cache_key = format!(
            "{}::{}",
            user_id.to_ascii_lowercase(),
            agent_id
                .as_deref()
                .unwrap_or(DEFAULT_AGENT_ID_ALIAS)
                .to_ascii_lowercase()
        );
        let agent_name = if let Some(cached) = agent_name_cache.get(&agent_cache_key) {
            cached.clone()
        } else {
            let resolved = resolve_monitor_session_agent_name(
                state.as_ref(),
                user_id,
                agent_id.as_deref().unwrap_or(""),
            )?;
            agent_name_cache.insert(agent_cache_key, resolved.clone());
            resolved
        };
        owners.push(json!({
            "user_id": user_id,
            "username": username,
            "agent_id": agent_id,
            "agent_name": agent_name,
        }));
    }
    Ok((owners, binding_total))
}

fn resolve_channel_account_delete_impact(
    state: &Arc<AppState>,
    channel: &str,
    account_id: &str,
) -> Result<Value, Response> {
    let account_exists = state
        .storage
        .get_channel_account(channel, account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .is_some();
    let bindings = state
        .storage
        .list_channel_bindings(Some(channel))
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .into_iter()
        .filter(|binding| binding.account_id.eq_ignore_ascii_case(account_id))
        .count() as i64;
    let (_, user_bindings) = state
        .storage
        .list_channel_user_bindings(ListChannelUserBindingsQuery {
            channel: Some(channel),
            account_id: Some(account_id),
            peer_kind: None,
            peer_id: None,
            user_id: None,
            offset: 0,
            limit: 1,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let (_, sessions) = state
        .storage
        .list_channel_sessions(Some(channel), Some(account_id), None, None, 0, 1)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let messages = state
        .storage
        .get_channel_message_stats(channel, account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .total;
    let outbox = state
        .storage
        .get_channel_outbox_stats(channel, account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(json!({
        "account_exists": account_exists,
        "bindings": bindings,
        "user_bindings": user_bindings,
        "sessions": sessions,
        "messages": messages,
        "outbox_total": outbox.total,
        "outbox_pending": outbox.pending,
        "outbox_retry": outbox.retry,
        "outbox_failed": outbox.failed,
    }))
}

fn delete_channel_account_records(
    storage: &dyn StorageBackend,
    channel: &str,
    account_id: &str,
) -> Result<(i64, i64, i64, i64, i64, i64), anyhow::Error> {
    let bindings = storage.list_channel_bindings(Some(channel))?;
    let mut deleted_bindings = 0_i64;
    for binding in bindings {
        if !binding.account_id.eq_ignore_ascii_case(account_id) {
            continue;
        }
        deleted_bindings += storage.delete_channel_binding(&binding.binding_id)?;
    }

    let mut deleted_user_bindings = 0_i64;
    loop {
        let (records, _) = storage.list_channel_user_bindings(ListChannelUserBindingsQuery {
            channel: Some(channel),
            account_id: Some(account_id),
            peer_kind: None,
            peer_id: None,
            user_id: None,
            offset: 0,
            limit: 500,
        })?;
        if records.is_empty() {
            break;
        }
        for record in records {
            deleted_user_bindings += storage.delete_channel_user_binding(
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
            )?;
        }
    }

    let deleted_sessions = storage.delete_channel_sessions(channel, account_id)?;
    let deleted_messages = storage.delete_channel_messages(channel, account_id)?;
    let deleted_outbox = storage.delete_channel_outbox(channel, account_id)?;

    let deleted_account = storage.delete_channel_account(channel, account_id)?;

    Ok((
        deleted_account,
        deleted_bindings,
        deleted_user_bindings,
        deleted_sessions,
        deleted_messages,
        deleted_outbox,
    ))
}

fn admin_delete_channel_account_records(
    state: &Arc<AppState>,
    channel: &str,
    account_id: &str,
) -> Result<(i64, i64, i64, i64, i64, i64), Response> {
    delete_channel_account_records(state.storage.as_ref(), channel, account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
}

async fn admin_channel_accounts(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ChannelAccountQuery>,
) -> Result<Json<Value>, Response> {
    let channel = query.channel.as_deref().map(|value| value.trim());
    let status = query.status.as_deref().map(|value| value.trim());
    let keyword = query
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let owner_user_filter = query
        .owner_user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let issue_only = query.issue_only.unwrap_or(false);
    let last_active_after = normalize_channel_event_ts(query.last_active_after);
    let last_active_before = normalize_channel_event_ts(query.last_active_before);
    let records = state
        .storage
        .list_channel_accounts(channel, status)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut username_cache: HashMap<String, String> = HashMap::new();
    let mut agent_name_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut items = Vec::with_capacity(records.len());
    for record in records {
        let (owners, binding_count) = resolve_channel_owner_preview(
            &state,
            &record.channel,
            &record.account_id,
            &mut username_cache,
            &mut agent_name_cache,
        )?;
        let (sessions_preview, session_count) = state
            .storage
            .list_channel_sessions(
                Some(&record.channel),
                Some(&record.account_id),
                None,
                None,
                0,
                1,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let message_stats = state
            .storage
            .get_channel_message_stats(&record.channel, &record.account_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let outbox_stats = state
            .storage
            .get_channel_outbox_stats(&record.channel, &record.account_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let runtime =
            build_channel_account_runtime(state.storage.as_ref(), &record, Some(binding_count));
        let owner_user_id = owners
            .first()
            .and_then(|item| item.get("user_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let owner_username = owners
            .first()
            .and_then(|item| item.get("username"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let owner_count = owners.len();
        let session_last_message_at = sessions_preview
            .first()
            .and_then(|item| normalize_channel_event_ts(Some(item.last_message_at)));
        let last_communication_at = normalize_channel_event_ts(message_stats.last_message_at)
            .or(normalize_channel_event_ts(outbox_stats.last_sent_at))
            .or(normalize_channel_event_ts(outbox_stats.last_failed_at))
            .or(session_last_message_at);
        let inbound_message_count = message_stats.total.max(0);
        let communication_count = inbound_message_count + outbox_stats.total.max(0);
        let sent_or_failed = outbox_stats.sent.max(0) + outbox_stats.failed.max(0);
        let outbound_success_rate = if sent_or_failed > 0 {
            (outbox_stats.sent.max(0) as f64) / (sent_or_failed as f64)
        } else {
            0.0
        };
        let account_active = record.status.trim().eq_ignore_ascii_case("active");
        let has_runtime_issue = runtime_has_issue(&runtime);
        let has_delivery_issue = outbox_stats.failed > 0 || outbox_stats.retry > 0;
        let has_issue = !account_active || has_runtime_issue || has_delivery_issue;
        if issue_only && !has_issue {
            continue;
        }
        if let Some(after) = last_active_after {
            if last_communication_at.unwrap_or(0.0) < after {
                continue;
            }
        }
        if let Some(before) = last_active_before {
            if last_communication_at.unwrap_or(f64::MAX) > before {
                continue;
            }
        }
        if let Some(owner_filter) = owner_user_filter.as_deref() {
            let matched = owners.iter().any(|item| {
                item.get("user_id")
                    .and_then(Value::as_str)
                    .map(|user_id| user_id.eq_ignore_ascii_case(owner_filter))
                    .unwrap_or(false)
            }) || owner_user_id.eq_ignore_ascii_case(owner_filter);
            if !matched {
                continue;
            }
        }
        if let Some(keyword) = keyword.as_deref() {
            let keyword_matched = contains_ignore_ascii_case(&record.channel, keyword)
                || contains_ignore_ascii_case(&record.account_id, keyword)
                || contains_ignore_ascii_case(&record.status, keyword)
                || contains_ignore_ascii_case(&owner_user_id, keyword)
                || contains_ignore_ascii_case(&owner_username, keyword)
                || owners.iter().any(|item| {
                    let user_id = item.get("user_id").and_then(Value::as_str).unwrap_or("");
                    let username = item.get("username").and_then(Value::as_str).unwrap_or("");
                    let agent_id = item.get("agent_id").and_then(Value::as_str).unwrap_or("");
                    let agent_name = item.get("agent_name").and_then(Value::as_str).unwrap_or("");
                    contains_ignore_ascii_case(user_id, keyword)
                        || contains_ignore_ascii_case(username, keyword)
                        || contains_ignore_ascii_case(agent_id, keyword)
                        || contains_ignore_ascii_case(agent_name, keyword)
                });
            if !keyword_matched {
                continue;
            }
        }
        items.push(json!({
            "channel": record.channel,
            "account_id": record.account_id,
            "config": record.config,
            "status": record.status,
            "created_at": record.created_at,
            "updated_at": record.updated_at,
            "runtime": runtime,
            "owner_user_id": owner_user_id,
            "owner_username": owner_username,
            "owners": owners,
            "owner_count": owner_count,
            "binding_count": binding_count,
            "session_count": session_count,
            "message_count": inbound_message_count,
            "inbound_message_count": inbound_message_count,
            "outbound_total_count": outbox_stats.total,
            "outbound_sent_count": outbox_stats.sent,
            "outbound_failed_count": outbox_stats.failed,
            "outbound_retry_count": outbox_stats.retry,
            "outbound_pending_count": outbox_stats.pending,
            "outbound_retry_attempts": outbox_stats.retry_attempts,
            "outbound_success_rate": outbound_success_rate,
            "communication_count": communication_count,
            "last_communication_at": last_communication_at,
            "has_issue": has_issue,
        }));
    }
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_channel_accounts_batch(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChannelAccountBatchRequest>,
) -> Result<Json<Value>, Response> {
    let action = ChannelAccountBatchAction::parse(&payload.action).ok_or_else(|| {
        error_response(
            StatusCode::BAD_REQUEST,
            "invalid batch action, expected enable/disable/delete".to_string(),
        )
    })?;

    if payload.items.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }

    let mut dedup = HashSet::new();
    let mut targets = Vec::new();
    for item in payload.items {
        let channel = item.channel.trim();
        let account_id = item.account_id.trim();
        if channel.is_empty() || account_id.is_empty() {
            continue;
        }
        let dedup_key = format!(
            "{}::{}",
            channel.to_ascii_lowercase(),
            account_id.to_ascii_lowercase()
        );
        if dedup.insert(dedup_key) {
            targets.push((channel.to_string(), account_id.to_string()));
        }
    }

    if targets.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    if targets.len() > ADMIN_CHANNEL_BATCH_LIMIT {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            format!("batch items exceed limit {ADMIN_CHANNEL_BATCH_LIMIT}"),
        ));
    }

    let now = now_ts();
    let target_status = action.target_status();
    let mut success = 0_i64;
    let mut failed = 0_i64;
    let mut skipped = 0_i64;
    let mut deleted_accounts = 0_i64;
    let mut deleted_bindings = 0_i64;
    let mut deleted_user_bindings = 0_i64;
    let mut deleted_sessions = 0_i64;
    let mut deleted_messages = 0_i64;
    let mut deleted_outbox = 0_i64;
    let mut items = Vec::with_capacity(targets.len());

    for (channel, account_id) in targets {
        match action {
            ChannelAccountBatchAction::Enable | ChannelAccountBatchAction::Disable => {
                match state.storage.get_channel_account(&channel, &account_id) {
                    Ok(Some(mut record)) => {
                        let Some(next_status) = target_status else {
                            failed += 1;
                            items.push(json!({
                                "channel": channel,
                                "account_id": account_id,
                                "ok": false,
                                "action": action.as_str(),
                                "result": "failed",
                                "error": "invalid target status",
                            }));
                            continue;
                        };
                        if record.status.eq_ignore_ascii_case(next_status) {
                            skipped += 1;
                            items.push(json!({
                                "channel": channel,
                                "account_id": account_id,
                                "ok": true,
                                "action": action.as_str(),
                                "result": "noop",
                                "status": record.status,
                            }));
                            continue;
                        }
                        record.status = next_status.to_string();
                        record.updated_at = now;
                        match state.storage.upsert_channel_account(&record) {
                            Ok(()) => {
                                success += 1;
                                items.push(json!({
                                    "channel": channel,
                                    "account_id": account_id,
                                    "ok": true,
                                    "action": action.as_str(),
                                    "result": "updated",
                                    "status": record.status,
                                    "updated_at": record.updated_at,
                                }));
                            }
                            Err(err) => {
                                failed += 1;
                                items.push(json!({
                                    "channel": channel,
                                    "account_id": account_id,
                                    "ok": false,
                                    "action": action.as_str(),
                                    "result": "failed",
                                    "error": err.to_string(),
                                }));
                            }
                        }
                    }
                    Ok(None) => {
                        skipped += 1;
                        items.push(json!({
                            "channel": channel,
                            "account_id": account_id,
                            "ok": true,
                            "action": action.as_str(),
                            "result": "not_found",
                        }));
                    }
                    Err(err) => {
                        failed += 1;
                        items.push(json!({
                            "channel": channel,
                            "account_id": account_id,
                            "ok": false,
                            "action": action.as_str(),
                            "result": "failed",
                            "error": err.to_string(),
                        }));
                    }
                }
            }
            ChannelAccountBatchAction::Delete => {
                match delete_channel_account_records(state.storage.as_ref(), &channel, &account_id)
                {
                    Ok((
                        removed_account,
                        removed_bindings,
                        removed_user_bindings,
                        removed_sessions,
                        removed_messages,
                        removed_outbox,
                    )) => {
                        deleted_accounts += removed_account;
                        deleted_bindings += removed_bindings;
                        deleted_user_bindings += removed_user_bindings;
                        deleted_sessions += removed_sessions;
                        deleted_messages += removed_messages;
                        deleted_outbox += removed_outbox;
                        if removed_account > 0 {
                            success += 1;
                        } else {
                            skipped += 1;
                        }
                        items.push(json!({
                            "channel": channel,
                            "account_id": account_id,
                            "ok": true,
                            "action": action.as_str(),
                            "result": if removed_account > 0 { "deleted" } else { "not_found" },
                            "deleted_accounts": removed_account,
                            "deleted_bindings": removed_bindings,
                            "deleted_user_bindings": removed_user_bindings,
                            "deleted_sessions": removed_sessions,
                            "deleted_messages": removed_messages,
                            "deleted_outbox": removed_outbox,
                        }));
                    }
                    Err(err) => {
                        failed += 1;
                        items.push(json!({
                            "channel": channel,
                            "account_id": account_id,
                            "ok": false,
                            "action": action.as_str(),
                            "result": "failed",
                            "error": err.to_string(),
                        }));
                    }
                }
            }
        }
    }

    Ok(Json(json!({ "data": {
        "action": action.as_str(),
        "total": items.len(),
        "success": success,
        "failed": failed,
        "skipped": skipped,
        "deleted_accounts": deleted_accounts,
        "deleted_bindings": deleted_bindings,
        "deleted_user_bindings": deleted_user_bindings,
        "deleted_sessions": deleted_sessions,
        "deleted_messages": deleted_messages,
        "deleted_outbox": deleted_outbox,
        "items": items,
    }})))
}

async fn admin_channel_account_delete_impact(
    State(state): State<Arc<AppState>>,
    AxumPath((channel, account_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let cleaned_channel = channel.trim().to_string();
    let cleaned_account = account_id.trim().to_string();
    if cleaned_channel.is_empty() || cleaned_account.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let impact = resolve_channel_account_delete_impact(&state, &cleaned_channel, &cleaned_account)?;
    Ok(Json(json!({ "data": impact })))
}

async fn admin_channel_account_delete(
    State(state): State<Arc<AppState>>,
    AxumPath((channel, account_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let cleaned_channel = channel.trim().to_string();
    let cleaned_account = account_id.trim().to_string();
    if cleaned_channel.is_empty() || cleaned_account.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }

    let (
        deleted_account,
        deleted_bindings,
        deleted_user_bindings,
        deleted_sessions,
        deleted_messages,
        deleted_outbox,
    ) = admin_delete_channel_account_records(&state, &cleaned_channel, &cleaned_account)?;

    Ok(Json(json!({ "data": {
        "channel": cleaned_channel,
        "account_id": cleaned_account,
        "deleted_accounts": deleted_account,
        "deleted_bindings": deleted_bindings,
        "deleted_user_bindings": deleted_user_bindings,
        "deleted_sessions": deleted_sessions,
        "deleted_messages": deleted_messages,
        "deleted_outbox": deleted_outbox,
    }})))
}

async fn admin_channel_bindings(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ChannelBindingQuery>,
) -> Result<Json<Value>, Response> {
    let channel = query.channel.as_deref().map(|value| value.trim());
    let records = state
        .storage
        .list_channel_bindings(channel)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .into_iter()
        .map(|record| {
            json!({
                "binding_id": record.binding_id,
                "channel": record.channel,
                "account_id": record.account_id,
                "peer_kind": record.peer_kind,
                "peer_id": record.peer_id,
                "agent_id": record.agent_id,
                "tool_overrides": record.tool_overrides,
                "priority": record.priority,
                "enabled": record.enabled,
                "created_at": record.created_at,
                "updated_at": record.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_channel_user_bindings(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ChannelUserBindingQuery>,
) -> Result<Json<Value>, Response> {
    let (items, total) = state
        .storage
        .list_channel_user_bindings(crate::storage::ListChannelUserBindingsQuery {
            channel: query.channel.as_deref(),
            account_id: query.account_id.as_deref(),
            peer_kind: query.peer_kind.as_deref(),
            peer_id: query.peer_id.as_deref(),
            user_id: query.user_id.as_deref(),
            offset: query.offset.unwrap_or(0),
            limit: query.limit.unwrap_or(50),
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = items
        .into_iter()
        .map(|record| {
            json!({
                "channel": record.channel,
                "account_id": record.account_id,
                "peer_kind": record.peer_kind,
                "peer_id": record.peer_id,
                "user_id": record.user_id,
                "created_at": record.created_at,
                "updated_at": record.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items, "total": total } })))
}

async fn admin_channel_sessions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ChannelSessionQuery>,
) -> Result<Json<Value>, Response> {
    let (items, total) = state
        .storage
        .list_channel_sessions(
            query.channel.as_deref(),
            query.account_id.as_deref(),
            query.peer_id.as_deref(),
            query.session_id.as_deref(),
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(50),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = items
        .into_iter()
        .map(|record| {
            json!({
                "channel": record.channel,
                "account_id": record.account_id,
                "peer_kind": record.peer_kind,
                "peer_id": record.peer_id,
                "thread_id": record.thread_id,
                "session_id": record.session_id,
                "agent_id": record.agent_id,
                "user_id": record.user_id,
                "tts_enabled": record.tts_enabled,
                "tts_voice": record.tts_voice,
                "metadata": record.metadata,
                "last_message_at": record.last_message_at,
                "created_at": record.created_at,
                "updated_at": record.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items, "total": total } })))
}

fn admin_channel_runtime_status_payload(owned_accounts: usize, scanned_total: usize) -> Value {
    json!({
        "collector_alive": true,
        "server_ts": chrono::Utc::now().timestamp_millis() as f64 / 1000.0,
        "owned_accounts": owned_accounts,
        "scanned_total": scanned_total,
    })
}

async fn admin_channel_runtime_logs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ChannelRuntimeLogsQuery>,
) -> Result<Json<Value>, Response> {
    runtime_metrics::record_loop_tick("api.admin.channels.runtime_logs", "request");
    let channel_filter = query
        .channel
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let account_filter = query
        .account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let limit = query.limit.unwrap_or(80).clamp(1, 200);
    let query_limit = (limit.saturating_mul(4)).clamp(limit, 400);

    let runtime_logs = state.control.channels.list_runtime_logs(
        channel_filter.as_deref(),
        account_filter.as_deref(),
        query_limit,
    );
    let scanned_total = runtime_logs.len();
    let mut items = Vec::new();
    for (index, item) in runtime_logs.into_iter().enumerate() {
        let channel = item.channel.trim().to_ascii_lowercase();
        let account_id = item.account_id.trim().to_string();
        if channel.is_empty() {
            continue;
        }
        if let Some(expected_channel) = channel_filter.as_deref() {
            if !channel.eq_ignore_ascii_case(expected_channel) {
                continue;
            }
        }
        if let Some(expected_account_id) = account_filter.as_deref() {
            if !account_id.eq_ignore_ascii_case(expected_account_id) {
                continue;
            }
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

    let owned_accounts = state
        .storage
        .list_channel_accounts(channel_filter.as_deref(), None)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .into_iter()
        .filter(|record| {
            account_filter
                .as_deref()
                .is_none_or(|account_id| record.account_id.eq_ignore_ascii_case(account_id))
        })
        .count();

    Ok(Json(json!({ "data": {
        "items": items,
        "total": items.len(),
        "status": admin_channel_runtime_status_payload(owned_accounts, scanned_total),
    } })))
}

async fn admin_channel_runtime_probe(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChannelRuntimeLogsProbeRequest>,
) -> Result<Json<Value>, Response> {
    let channel = payload
        .channel
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| {
            error_response(StatusCode::BAD_REQUEST, "channel is required".to_string())
        })?;
    let account_id = payload
        .account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(account_id_ref) = account_id.as_deref() {
        let exists = state
            .storage
            .get_channel_account(&channel, account_id_ref)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .is_some();
        if !exists {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                format!("channel account not found: {channel}/{account_id_ref}"),
            ));
        }
    }
    let message = payload
        .message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "runtime probe ok: admin".to_string());
    state.control.channels.record_runtime_info(
        &channel,
        account_id.as_deref(),
        "runtime_probe",
        message.clone(),
    );
    let owned_accounts = state
        .storage
        .list_channel_accounts(Some(&channel), None)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .into_iter()
        .filter(|record| {
            account_id
                .as_deref()
                .is_none_or(|value| record.account_id.eq_ignore_ascii_case(value))
        })
        .count();
    Ok(Json(json!({ "data": {
        "channel": channel,
        "account_id": account_id,
        "event": "runtime_probe",
        "message": message,
        "ts": chrono::Utc::now().timestamp_millis() as f64 / 1000.0,
        "status": admin_channel_runtime_status_payload(owned_accounts, 1),
    } })))
}
