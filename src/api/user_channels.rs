use crate::api::user_context::resolve_user;
use crate::channels::catalog;
use crate::channels::types::{ChannelAccountConfig, FeishuConfig, WechatConfig, WechatMpConfig};
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
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use uuid::Uuid;

const USER_CHANNEL_FEISHU: &str = "feishu";
const USER_CHANNEL_QQBOT: &str = "qqbot";
const USER_CHANNEL_WHATSAPP: &str = "whatsapp";
const USER_CHANNEL_WECHAT: &str = "wechat";
const USER_CHANNEL_WECHAT_MP: &str = "wechat_mp";
const DEFAULT_GROUP_PEER_KIND: &str = "group";
const WILDCARD_PEER_ID: &str = "*";

#[derive(Debug, Deserialize)]
struct ChannelAccountsQuery {
    #[serde(default)]
    channel: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelAccountUpsertRequest {
    channel: String,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    create_new: Option<bool>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    account_name: Option<String>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    app_secret: Option<String>,
    #[serde(default)]
    receive_group_chat: Option<bool>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    peer_kind: Option<String>,
    #[serde(default)]
    config: Option<Value>,
    #[serde(default)]
    feishu: Option<FeishuAccountPayload>,
    #[serde(default)]
    wechat: Option<WechatAccountPayload>,
    #[serde(default)]
    wechat_mp: Option<WechatMpAccountPayload>,
}

#[derive(Debug, Deserialize)]
struct FeishuAccountPayload {
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    app_secret: Option<String>,
    #[serde(default)]
    domain: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WechatAccountPayload {
    #[serde(default)]
    corp_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    encoding_aes_key: Option<String>,
    #[serde(default)]
    domain: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WechatMpAccountPayload {
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    app_secret: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    encoding_aes_key: Option<String>,
    #[serde(default)]
    original_id: Option<String>,
    #[serde(default)]
    domain: Option<String>,
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
        .route(
            "/wunder/channels/accounts",
            get(list_channel_accounts).post(upsert_channel_account),
        )
        .route(
            "/wunder/channels/accounts/{channel}/{account_id}",
            delete(delete_channel_account_by_id),
        )
        .route(
            "/wunder/channels/accounts/{channel}",
            delete(delete_channel_account_legacy),
        )
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
    let resolved = resolve_user(&state, &headers, None).await?;
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

    let account_keys = list_owned_account_keys(&state, &user_id, channel_filter.as_deref())?;
    let mut items = Vec::new();
    for (channel, account_id) in account_keys {
        let record = state
            .storage
            .get_channel_account(&channel, &account_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let Some(record) = record else {
            continue;
        };

        let binding_pref = load_user_binding_pref(&state, &user_id, &channel, &account_id)?;
        items.push(build_user_account_item(
            &channel,
            &account_id,
            &record.status,
            Some(record.created_at),
            Some(record.updated_at),
            &record.config,
            binding_pref.as_deref(),
        ));
    }

    Ok(Json(json!({ "data": {
        "items": items,
        "supported_channels": supported_user_channel_items(),
    } })))
}

async fn upsert_channel_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ChannelAccountUpsertRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let config = state.config_store.get().await;
    if !config.channels.enabled && !config.gateway.enabled {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "channels disabled".to_string(),
        ));
    }

    let channel = normalize_user_channel(Some(payload.channel.as_str()))?;
    let requested_agent_id = payload
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(agent_id) = requested_agent_id.as_ref() {
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
    let existing_account_ids = list_owned_account_ids_for_channel(&state, &user_id, &channel)?;
    let requested_account_id = payload
        .account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let has_requested_account = requested_account_id.is_some();
    let create_new = payload.create_new.unwrap_or(false);

    let account_id = if let Some(account_id) = requested_account_id {
        if !existing_account_ids.iter().any(|item| item == &account_id) {
            return Err(error_response(
                StatusCode::FORBIDDEN,
                i18n::t("error.permission_denied"),
            ));
        }
        account_id
    } else if create_new || existing_account_ids.is_empty() {
        make_user_account_id()
    } else if existing_account_ids.len() == 1 {
        existing_account_ids[0].clone()
    } else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "account_id is required when multiple channel accounts exist".to_string(),
        ));
    };

    let existing = state
        .storage
        .get_channel_account(&channel, &account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if has_requested_account && existing.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "channel account not found".to_string(),
        ));
    }

    let mut config_value = existing
        .as_ref()
        .map(|record| record.config.clone())
        .unwrap_or_else(|| json!({}));
    if !config_value.is_object() {
        config_value = json!({});
    }

    if let Some(extra_config) = payload.config.as_ref() {
        let map = config_value.as_object_mut().ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "invalid channel config".to_string(),
            )
        })?;
        merge_json_object(map, extra_config)?;
    }

    if let Some(display_name) = payload
        .account_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let map = config_value.as_object_mut().ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "invalid channel config".to_string(),
            )
        })?;
        map.insert(
            "display_name".to_string(),
            Value::String(display_name.to_string()),
        );
    }

    let existing_agent_id = config_value
        .get("agent_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let existing_peer_kind = load_user_binding_pref(&state, &user_id, &channel, &account_id)?;
    let mut selected_peer_kind = payload
        .peer_kind
        .as_deref()
        .map(|value| normalize_user_peer_kind(&channel, value))
        .filter(|value| !value.is_empty())
        .or(existing_peer_kind.clone())
        .unwrap_or_else(|| default_peer_kind_for_channel(&channel, payload.receive_group_chat));

    if channel.eq_ignore_ascii_case(USER_CHANNEL_FEISHU) {
        let existing_feishu = ChannelAccountConfig::from_value(&config_value)
            .feishu
            .unwrap_or_default();

        let requested_app_id = payload
            .app_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                payload
                    .feishu
                    .as_ref()
                    .and_then(|value| value.app_id.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let app_id = requested_app_id
            .or_else(|| {
                existing_feishu
                    .app_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "feishu app_id is required".to_string(),
                )
            })?;

        let requested_app_secret = payload
            .app_secret
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                payload
                    .feishu
                    .as_ref()
                    .and_then(|value| value.app_secret.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let app_secret = requested_app_secret
            .or_else(|| {
                existing_feishu
                    .app_secret
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "feishu app_secret is required".to_string(),
                )
            })?;

        let domain = payload
            .domain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                payload
                    .feishu
                    .as_ref()
                    .and_then(|value| value.domain.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                existing_feishu
                    .domain
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "open.feishu.cn".to_string());

        let receive_group_chat = payload
            .receive_group_chat
            .unwrap_or_else(|| existing_peer_kind.as_deref() != Some("user"));
        selected_peer_kind = if receive_group_chat {
            "group".to_string()
        } else {
            "user".to_string()
        };

        let map = config_value.as_object_mut().ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "invalid channel config".to_string(),
            )
        })?;
        map.insert(
            "feishu".to_string(),
            json!(FeishuConfig {
                app_id: Some(app_id),
                app_secret: Some(app_secret),
                verification_token: None,
                encrypt_key: None,
                domain: Some(domain),
                receive_id_type: Some("chat_id".to_string()),
                long_connection_enabled: Some(true),
            }),
        );
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT) {
        let existing_wechat = ChannelAccountConfig::from_value(&config_value)
            .wechat
            .unwrap_or_default();

        let corp_id = payload
            .wechat
            .as_ref()
            .and_then(|value| value.corp_id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_wechat
                    .corp_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "wechat corp_id is required".to_string(),
                )
            })?;
        let agent_id = payload
            .wechat
            .as_ref()
            .and_then(|value| value.agent_id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_wechat
                    .agent_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "wechat agent_id is required".to_string(),
                )
            })?;
        let secret = payload
            .wechat
            .as_ref()
            .and_then(|value| value.secret.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_wechat
                    .secret
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "wechat secret is required".to_string(),
                )
            })?;
        let token = payload
            .wechat
            .as_ref()
            .and_then(|value| value.token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_wechat
                    .token
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let encoding_aes_key = payload
            .wechat
            .as_ref()
            .and_then(|value| value.encoding_aes_key.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_wechat
                    .encoding_aes_key
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let domain = payload
            .wechat
            .as_ref()
            .and_then(|value| value.domain.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_wechat
                    .domain
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "qyapi.weixin.qq.com".to_string());
        selected_peer_kind = "user".to_string();

        let map = config_value.as_object_mut().ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "invalid channel config".to_string(),
            )
        })?;
        map.insert(
            "wechat".to_string(),
            json!(WechatConfig {
                corp_id: Some(corp_id),
                agent_id: Some(agent_id),
                secret: Some(secret),
                token,
                encoding_aes_key,
                domain: Some(domain),
            }),
        );
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT_MP) {
        let existing_wechat_mp = ChannelAccountConfig::from_value(&config_value)
            .wechat_mp
            .unwrap_or_default();

        let app_id = payload
            .app_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                payload
                    .wechat_mp
                    .as_ref()
                    .and_then(|value| value.app_id.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                existing_wechat_mp
                    .app_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "wechat_mp app_id is required".to_string(),
                )
            })?;
        let app_secret = payload
            .app_secret
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                payload
                    .wechat_mp
                    .as_ref()
                    .and_then(|value| value.app_secret.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                existing_wechat_mp
                    .app_secret
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "wechat_mp app_secret is required".to_string(),
                )
            })?;
        let token = payload
            .wechat_mp
            .as_ref()
            .and_then(|value| value.token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_wechat_mp
                    .token
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let encoding_aes_key = payload
            .wechat_mp
            .as_ref()
            .and_then(|value| value.encoding_aes_key.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_wechat_mp
                    .encoding_aes_key
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let original_id = payload
            .wechat_mp
            .as_ref()
            .and_then(|value| value.original_id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_wechat_mp
                    .original_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let domain = payload
            .domain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                payload
                    .wechat_mp
                    .as_ref()
                    .and_then(|value| value.domain.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .or_else(|| {
                existing_wechat_mp
                    .domain
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "api.weixin.qq.com".to_string());
        selected_peer_kind = "user".to_string();

        let map = config_value.as_object_mut().ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "invalid channel config".to_string(),
            )
        })?;
        map.insert(
            "wechat_mp".to_string(),
            json!(WechatMpConfig {
                app_id: Some(app_id),
                app_secret: Some(app_secret),
                token,
                encoding_aes_key,
                original_id,
                domain: Some(domain),
            }),
        );
    } else if existing.is_none() && payload.config.is_none() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "config is required for this channel".to_string(),
        ));
    }

    if channel.eq_ignore_ascii_case(USER_CHANNEL_FEISHU)
        && !matches!(selected_peer_kind.as_str(), "user" | "group")
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "feishu peer_kind must be user or group".to_string(),
        ));
    }
    if channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT)
        && !matches!(selected_peer_kind.as_str(), "user")
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "wechat peer_kind must be user".to_string(),
        ));
    }
    if channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT_MP)
        && !matches!(selected_peer_kind.as_str(), "user")
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "wechat_mp peer_kind must be user".to_string(),
        ));
    }
    if selected_peer_kind.trim().is_empty() {
        selected_peer_kind = DEFAULT_GROUP_PEER_KIND.to_string();
    }

    {
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
                Value::String(make_user_inbound_token(&user_id, &channel, &account_id)),
            );
        }
        if let Some(agent_id) = requested_agent_id.clone().or(existing_agent_id.clone()) {
            map.insert("agent_id".to_string(), Value::String(agent_id));
        } else {
            map.insert("agent_id".to_string(), Value::Null);
        }
        map.insert("owner_user_id".to_string(), Value::String(user_id.clone()));
    }

    let agent_id_for_binding = requested_agent_id.clone().or(existing_agent_id);
    let enabled = payload.enabled.unwrap_or(true);
    let now = now_ts();
    let status = if enabled {
        "active".to_string()
    } else {
        "disabled".to_string()
    };
    let created_at = existing
        .as_ref()
        .map(|record| record.created_at)
        .unwrap_or(now);

    let account_record = crate::storage::ChannelAccountRecord {
        channel: channel.clone(),
        account_id: account_id.clone(),
        config: config_value.clone(),
        status: status.clone(),
        created_at,
        updated_at: now,
    };
    state
        .storage
        .upsert_channel_account(&account_record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    sync_user_default_binding(
        &state,
        &user_id,
        &channel,
        &account_id,
        &selected_peer_kind,
        agent_id_for_binding.as_deref(),
        enabled,
        now,
    )?;

    let item = build_user_account_item(
        &channel,
        &account_id,
        &status,
        Some(created_at),
        Some(now),
        &config_value,
        Some(&selected_peer_kind),
    );

    Ok(Json(json!({ "data": item })))
}

async fn delete_channel_account_by_id(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((channel, account_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let channel = normalize_user_channel(Some(channel.as_str()))?;
    let account_id = account_id.trim().to_string();
    if account_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    if !user_owns_channel_account(&state, &user_id, &channel, &account_id)? {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.permission_denied"),
        ));
    }

    let (deleted_account, deleted_bindings, deleted_user_bindings) =
        delete_channel_account_records(&state, &user_id, &channel, &account_id)?;

    Ok(Json(json!({ "data": {
        "channel": channel,
        "account_id": account_id,
        "deleted_accounts": deleted_account,
        "deleted_bindings": deleted_bindings,
        "deleted_user_bindings": deleted_user_bindings,
    }})))
}

async fn delete_channel_account_legacy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(channel): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let channel = normalize_user_channel(Some(channel.as_str()))?;

    let account_ids = list_owned_account_ids_for_channel(&state, &user_id, &channel)?;
    if account_ids.is_empty() {
        return Ok(Json(json!({ "data": {
            "channel": channel,
            "account_id": null,
            "deleted_accounts": 0,
            "deleted_bindings": 0,
            "deleted_user_bindings": 0,
        }})));
    }
    if account_ids.len() > 1 {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "multiple channel accounts exist, please specify account_id".to_string(),
        ));
    }

    let account_id = account_ids[0].clone();
    let (deleted_account, deleted_bindings, deleted_user_bindings) =
        delete_channel_account_records(&state, &user_id, &channel, &account_id)?;

    Ok(Json(json!({ "data": {
        "channel": channel,
        "account_id": account_id,
        "deleted_accounts": deleted_account,
        "deleted_bindings": deleted_bindings,
        "deleted_user_bindings": deleted_user_bindings,
    }})))
}

async fn list_channel_bindings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ChannelBindingsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let query_channel = query
        .channel
        .as_deref()
        .map(|value| normalize_user_channel(Some(value)))
        .transpose()?;

    let (bindings, total) = state
        .storage
        .list_channel_user_bindings(crate::storage::ListChannelUserBindingsQuery {
            channel: query_channel.as_deref(),
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
        .list_channel_bindings(query_channel.as_deref())
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
            let binding = binding_by_id.get(&binding_id).cloned().or_else(|| {
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
    let channel = normalize_user_channel(Some(payload.channel.as_str()))?;
    let account_id = payload.account_id.trim().to_string();
    let peer_kind = normalize_user_peer_kind(&channel, &payload.peer_kind);
    let peer_id = payload.peer_id.trim().to_string();
    if channel.is_empty() || account_id.is_empty() || peer_kind.is_empty() || peer_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    if channel.eq_ignore_ascii_case(USER_CHANNEL_FEISHU)
        && !matches!(peer_kind.as_str(), "user" | "group")
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "feishu peer_kind must be user or group".to_string(),
        ));
    }
    if channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT) && !matches!(peer_kind.as_str(), "user") {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "wechat peer_kind must be user".to_string(),
        ));
    }
    if channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT_MP) && !matches!(peer_kind.as_str(), "user")
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "wechat_mp peer_kind must be user".to_string(),
        ));
    }
    if !user_owns_channel_account(&state, &user_id, &channel, &account_id)? {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.permission_denied"),
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
    let channel = normalize_user_channel(Some(channel.as_str()))?;
    let account_id = account_id.trim().to_string();
    let peer_kind = peer_kind.trim().to_string();
    let peer_id = peer_id.trim().to_string();
    if channel.is_empty() || account_id.is_empty() || peer_kind.is_empty() || peer_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    if !user_owns_channel_account(&state, &user_id, &channel, &account_id)? {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.permission_denied"),
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

fn supported_user_channel_items() -> Vec<Value> {
    catalog::user_supported_channels()
        .into_iter()
        .map(|item| {
            json!({
                "channel": item.channel,
                "display_name": item.display_name,
                "description": item.description,
                "webhook_mode": item.webhook_mode,
                "docs_hint": item.docs_hint,
            })
        })
        .collect()
}

fn is_supported_user_channel(channel: &str) -> bool {
    catalog::find_channel(channel)
        .map(|item| item.user_supported)
        .unwrap_or(false)
}

fn list_owned_account_keys(
    state: &Arc<AppState>,
    user_id: &str,
    channel_filter: Option<&str>,
) -> Result<Vec<(String, String)>, Response> {
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
        if let Some(filter) = channel_filter {
            if !channel.eq_ignore_ascii_case(filter) {
                continue;
            }
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

    Ok(account_keys.into_iter().collect())
}

fn list_owned_account_ids_for_channel(
    state: &Arc<AppState>,
    user_id: &str,
    channel: &str,
) -> Result<Vec<String>, Response> {
    let normalized_channel = normalize_user_channel(Some(channel))?;
    let keys = list_owned_account_keys(state, user_id, Some(&normalized_channel))?;
    Ok(keys.into_iter().map(|(_, account_id)| account_id).collect())
}

fn user_owns_channel_account(
    state: &Arc<AppState>,
    user_id: &str,
    channel: &str,
    account_id: &str,
) -> Result<bool, Response> {
    let channel = normalize_user_channel(Some(channel))?;
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Ok(false);
    }

    let legacy_account_id = make_legacy_user_account_id(user_id, &channel);
    if account_id.eq_ignore_ascii_case(&legacy_account_id) {
        return Ok(true);
    }

    let (bindings, _) = state
        .storage
        .list_channel_user_bindings(crate::storage::ListChannelUserBindingsQuery {
            channel: Some(&channel),
            account_id: Some(account_id),
            peer_kind: None,
            peer_id: None,
            user_id: Some(user_id),
            offset: 0,
            limit: 1,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    Ok(!bindings.is_empty())
}

fn load_user_binding_pref(
    state: &Arc<AppState>,
    user_id: &str,
    channel: &str,
    account_id: &str,
) -> Result<Option<String>, Response> {
    let (items, _) = state
        .storage
        .list_channel_user_bindings(crate::storage::ListChannelUserBindingsQuery {
            channel: Some(channel),
            account_id: Some(account_id),
            peer_kind: None,
            peer_id: None,
            user_id: Some(user_id),
            offset: 0,
            limit: 200,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    for item in items {
        if is_wildcard_peer_id(&item.peer_id) {
            let peer_kind = item.peer_kind.trim();
            if !peer_kind.is_empty() {
                return Ok(Some(peer_kind.to_string()));
            }
        }
    }
    Ok(None)
}

fn build_user_account_item(
    channel: &str,
    account_id: &str,
    status: &str,
    created_at: Option<f64>,
    updated_at: Option<f64>,
    config: &Value,
    peer_kind_hint: Option<&str>,
) -> Value {
    let account_cfg = ChannelAccountConfig::from_value(config);
    let mut peer_kind = peer_kind_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_GROUP_PEER_KIND)
        .to_ascii_lowercase();
    if channel.eq_ignore_ascii_case(USER_CHANNEL_FEISHU)
        && !matches!(peer_kind.as_str(), "group" | "user")
    {
        peer_kind = "group".to_string();
    }
    if (channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT)
        || channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT_MP))
        && peer_kind != "user"
    {
        peer_kind = "user".to_string();
    }

    let active = status.trim().eq_ignore_ascii_case("active");
    let receive_group_chat = peer_kind == "group";

    let configured: bool;
    let config_preview: Value;
    let mut receive_id_type = "chat_id".to_string();
    let mut long_connection_enabled = true;

    if channel.eq_ignore_ascii_case(USER_CHANNEL_FEISHU) {
        let feishu = account_cfg.feishu.unwrap_or_default();
        let app_id = feishu
            .app_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let app_secret_set = feishu
            .app_secret
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let domain = feishu
            .domain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("open.feishu.cn")
            .to_string();
        receive_id_type = feishu
            .receive_id_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("chat_id")
            .to_string();
        long_connection_enabled = feishu.long_connection_enabled.unwrap_or(true);
        configured = !app_id.is_empty() && app_secret_set;
        config_preview = json!({
            "feishu": {
                "app_id": app_id,
                "app_secret_set": app_secret_set,
                "domain": domain,
            }
        });
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_QQBOT) {
        let qqbot = account_cfg.qqbot.unwrap_or_default();
        let app_id = qqbot
            .app_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let client_secret_set = qqbot
            .client_secret
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        configured = !app_id.is_empty() && client_secret_set;
        config_preview = json!({
            "qqbot": {
                "app_id": app_id,
                "client_secret_set": client_secret_set,
                "markdown_support": qqbot.markdown_support,
            }
        });
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_WHATSAPP) {
        let whatsapp = account_cfg.whatsapp_cloud.unwrap_or_default();
        let phone_number_id = whatsapp
            .phone_number_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let access_token_set = whatsapp
            .access_token
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let verify_token_set = whatsapp
            .verify_token
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        configured = !phone_number_id.is_empty() && access_token_set;
        config_preview = json!({
            "whatsapp_cloud": {
                "phone_number_id": phone_number_id,
                "access_token_set": access_token_set,
                "verify_token_set": verify_token_set,
                "api_version": whatsapp.api_version,
            }
        });
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT) {
        let wechat = account_cfg.wechat.unwrap_or_default();
        let corp_id = wechat
            .corp_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let agent_id = wechat
            .agent_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let secret_set = wechat
            .secret
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let token_set = wechat
            .token
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let encoding_aes_key_set = wechat
            .encoding_aes_key
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let domain = wechat
            .domain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("qyapi.weixin.qq.com")
            .to_string();
        configured = !corp_id.is_empty() && !agent_id.is_empty() && secret_set;
        config_preview = json!({
            "wechat": {
                "corp_id": corp_id,
                "agent_id": agent_id,
                "secret_set": secret_set,
                "token_set": token_set,
                "encoding_aes_key_set": encoding_aes_key_set,
                "domain": domain,
            }
        });
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT_MP) {
        let wechat_mp = account_cfg.wechat_mp.unwrap_or_default();
        let app_id = wechat_mp
            .app_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let app_secret_set = wechat_mp
            .app_secret
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let token_set = wechat_mp
            .token
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let encoding_aes_key_set = wechat_mp
            .encoding_aes_key
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let original_id = wechat_mp
            .original_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let domain = wechat_mp
            .domain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("api.weixin.qq.com")
            .to_string();
        configured = !app_id.is_empty() && app_secret_set;
        config_preview = json!({
            "wechat_mp": {
                "app_id": app_id,
                "app_secret_set": app_secret_set,
                "token_set": token_set,
                "encoding_aes_key_set": encoding_aes_key_set,
                "original_id": original_id,
                "domain": domain,
            }
        });
    } else {
        configured = config
            .as_object()
            .map(|map| !map.is_empty())
            .unwrap_or(false);
        config_preview = config.clone();
    }

    let display_name = config
        .get("display_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let mut meta = json!({
        "configured": configured,
        "peer_kind": peer_kind,
        "receive_group_chat": receive_group_chat,
    });
    if channel.eq_ignore_ascii_case(USER_CHANNEL_FEISHU) {
        if let Some(meta_map) = meta.as_object_mut() {
            meta_map.insert(
                "receive_id_type".to_string(),
                Value::String(receive_id_type),
            );
            meta_map.insert(
                "long_connection_enabled".to_string(),
                Value::Bool(long_connection_enabled),
            );
        }
    }

    json!({
        "channel": channel,
        "account_id": account_id,
        "name": display_name,
        "status": status,
        "active": active,
        "created_at": created_at,
        "updated_at": updated_at,
        "meta": meta,
        "config": config_preview,
        "raw_config": config,
    })
}

fn merge_json_object(target: &mut Map<String, Value>, patch: &Value) -> Result<(), Response> {
    let patch_obj = patch.as_object().ok_or_else(|| {
        error_response(
            StatusCode::BAD_REQUEST,
            "channel config must be a JSON object".to_string(),
        )
    })?;
    for (key, value) in patch_obj {
        target.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn delete_channel_account_records(
    state: &Arc<AppState>,
    user_id: &str,
    channel: &str,
    account_id: &str,
) -> Result<(i64, i64, i64), Response> {
    let deleted_account = state
        .storage
        .delete_channel_account(channel, account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let (bindings, _) = state
        .storage
        .list_channel_user_bindings(crate::storage::ListChannelUserBindingsQuery {
            channel: Some(channel),
            account_id: Some(account_id),
            peer_kind: None,
            peer_id: None,
            user_id: Some(user_id),
            offset: 0,
            limit: 200,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let mut deleted_bindings = 0_i64;
    let mut deleted_user_bindings = 0_i64;
    for record in bindings {
        deleted_user_bindings += state
            .storage
            .delete_channel_user_binding(
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let binding_id = make_user_binding_id(
            &record.user_id,
            &record.channel,
            &record.account_id,
            &record.peer_kind,
            &record.peer_id,
        );
        deleted_bindings += state
            .storage
            .delete_channel_binding(&binding_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }

    Ok((deleted_account, deleted_bindings, deleted_user_bindings))
}

fn default_peer_kind_for_channel(channel: &str, receive_group_chat: Option<bool>) -> String {
    if channel.eq_ignore_ascii_case(USER_CHANNEL_FEISHU) {
        return if receive_group_chat.unwrap_or(true) {
            "group".to_string()
        } else {
            "user".to_string()
        };
    }
    if channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT) {
        return "user".to_string();
    }
    if channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT_MP) {
        return "user".to_string();
    }
    if receive_group_chat == Some(false) {
        return "user".to_string();
    }
    DEFAULT_GROUP_PEER_KIND.to_string()
}

#[allow(clippy::too_many_arguments)]
fn sync_user_default_binding(
    state: &Arc<AppState>,
    user_id: &str,
    channel: &str,
    account_id: &str,
    selected_peer_kind: &str,
    agent_id: Option<&str>,
    enabled: bool,
    now: f64,
) -> Result<(), Response> {
    let selected_kind = normalize_user_peer_kind(channel, selected_peer_kind);
    if selected_kind.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "peer_kind is required".to_string(),
        ));
    }

    let (existing_bindings, _) = state
        .storage
        .list_channel_user_bindings(crate::storage::ListChannelUserBindingsQuery {
            channel: Some(channel),
            account_id: Some(account_id),
            peer_kind: None,
            peer_id: None,
            user_id: Some(user_id),
            offset: 0,
            limit: 200,
        })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    for record in existing_bindings {
        let keep = record.peer_kind == selected_kind && is_wildcard_peer_id(&record.peer_id);
        if keep {
            continue;
        }
        state
            .storage
            .delete_channel_user_binding(
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let binding_id = make_user_binding_id(
            user_id,
            &record.channel,
            &record.account_id,
            &record.peer_kind,
            &record.peer_id,
        );
        state
            .storage
            .delete_channel_binding(&binding_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }

    let selected_binding_id = make_user_binding_id(
        user_id,
        channel,
        account_id,
        &selected_kind,
        WILDCARD_PEER_ID,
    );
    let agent_id = agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());
    let binding_record = crate::storage::ChannelBindingRecord {
        binding_id: selected_binding_id,
        channel: channel.to_string(),
        account_id: account_id.to_string(),
        peer_kind: Some(selected_kind.clone()),
        peer_id: Some(WILDCARD_PEER_ID.to_string()),
        agent_id,
        tool_overrides: Vec::new(),
        priority: 100,
        enabled,
        created_at: now,
        updated_at: now,
    };
    state
        .storage
        .upsert_channel_binding(&binding_record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let user_binding_record = crate::storage::ChannelUserBindingRecord {
        channel: channel.to_string(),
        account_id: account_id.to_string(),
        peer_kind: selected_kind,
        peer_id: WILDCARD_PEER_ID.to_string(),
        user_id: user_id.to_string(),
        created_at: now,
        updated_at: now,
    };
    state
        .storage
        .upsert_channel_user_binding(&user_binding_record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    Ok(())
}

fn normalize_user_peer_kind(channel: &str, peer_kind: &str) -> String {
    let normalized = peer_kind.trim().to_ascii_lowercase();
    if (channel.trim().eq_ignore_ascii_case(USER_CHANNEL_FEISHU)
        || channel.trim().eq_ignore_ascii_case(USER_CHANNEL_WECHAT)
        || channel.trim().eq_ignore_ascii_case(USER_CHANNEL_WECHAT_MP))
        && matches!(normalized.as_str(), "dm" | "direct" | "single")
    {
        return "user".to_string();
    }
    normalized
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

fn make_user_account_id() -> String {
    format!("uacc_{}", Uuid::new_v4().simple())
}

fn make_user_inbound_token(user_id: &str, channel: &str, account_id: &str) -> String {
    let key = format!(
        "uacc-token:{user_id}|{channel}|{account_id}",
        user_id = user_id.trim().to_ascii_lowercase(),
        channel = channel.trim().to_ascii_lowercase(),
        account_id = account_id.trim().to_ascii_lowercase(),
    );
    format!(
        "utok_{}",
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

fn is_wildcard_peer_id(value: &str) -> bool {
    value.trim() == WILDCARD_PEER_ID
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
