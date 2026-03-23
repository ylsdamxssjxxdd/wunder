use crate::api::user_context::resolve_user;
use crate::channels::catalog;
use crate::channels::qqbot;
use crate::channels::types::{
    ChannelAccountConfig, FeishuConfig, WechatConfig, WechatMpConfig, WeixinConfig,
};
use crate::channels::weixin;
use crate::i18n;
use crate::state::AppState;
use crate::user_access::is_agent_allowed;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{
    routing::{delete, get, post},
    Json, Router,
};
use base64::Engine;
use image::{DynamicImage, ImageBuffer, ImageFormat, Luma};
use qrcode::types::Color as QrColor;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::io::Cursor;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

const USER_CHANNEL_FEISHU: &str = "feishu";
const USER_CHANNEL_QQBOT: &str = "qqbot";
const USER_CHANNEL_WHATSAPP: &str = "whatsapp";
const USER_CHANNEL_WECHAT: &str = "wechat";
const USER_CHANNEL_WECHAT_MP: &str = "wechat_mp";
const USER_CHANNEL_WEIXIN: &str = "weixin";
const USER_CHANNEL_XMPP: &str = "xmpp";
const DEFAULT_GROUP_PEER_KIND: &str = "group";
const WILDCARD_PEER_ID: &str = "*";
const WEIXIN_QR_SESSION_TTL_MS: u64 = 5 * 60_000;
const WEIXIN_QR_WAIT_POLL_INTERVAL_MS: u64 = 1_000;
const WEIXIN_QR_STATUS_LONG_POLL_TIMEOUT_MS: u64 = 35_000;

fn normalize_weixin_qr_image_value(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    if ((value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\'')))
        && value.len() > 1
    {
        value = value[1..value.len() - 1].trim().to_string();
    }
    value
        .replace("\\r\\n", "")
        .replace("\\n", "")
        .replace("\\r", "")
        .replace("\r\n", "")
        .replace('\n', "")
        .replace('\r', "")
        .trim()
        .to_string()
}

fn looks_like_base64_image(value: &str) -> bool {
    if value.len() <= 64 {
        return false;
    }
    value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'-' | b'_')
    })
}

fn build_absolute_weixin_qr_url(raw: &str, api_base: &str) -> Option<String> {
    let value = normalize_weixin_qr_image_value(raw);
    if value.is_empty() {
        return None;
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        return Some(value);
    }
    if value.starts_with("//") {
        return Some(format!("https:{value}"));
    }
    if value.starts_with('/') {
        let base = reqwest::Url::parse(api_base).ok()?;
        return base.join(&value).ok().map(|url| url.to_string());
    }
    None
}

fn detect_image_mime(bytes: &[u8], content_type: &str) -> Option<String> {
    let mime = content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if mime.starts_with("image/") {
        return Some(mime);
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png".to_string());
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some("image/jpeg".to_string());
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif".to_string());
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp".to_string());
    }
    let text_prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).to_ascii_lowercase();
    if text_prefix.contains("<svg") {
        return Some("image/svg+xml".to_string());
    }
    None
}

fn extract_data_image_uri(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("data:image/")?;
    let tail = &text[start..];
    let mut end = tail.len();
    for marker in ['"', '\'', '<', '>', ' ', '\n', '\r', '\t'] {
        if let Some(index) = tail.find(marker) {
            end = end.min(index);
        }
    }
    let candidate = tail[..end].trim();
    if candidate.starts_with("data:image/") {
        return Some(candidate.to_string());
    }
    None
}

fn extract_first_img_src(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let img_start = lower.find("<img")?;
    let html = &text[img_start..];
    let html_lower = &lower[img_start..];

    if let Some(index) = html_lower.find("src=\"") {
        let start = index + 5;
        let tail = &html[start..];
        let end = tail.find('"')?;
        let src = tail[..end].trim().replace("&amp;", "&");
        if !src.is_empty() {
            return Some(src);
        }
    }
    if let Some(index) = html_lower.find("src='") {
        let start = index + 5;
        let tail = &html[start..];
        let end = tail.find('\'')?;
        let src = tail[..end].trim().replace("&amp;", "&");
        if !src.is_empty() {
            return Some(src);
        }
    }
    None
}

fn build_weixin_qr_png_data_uri(raw_qrcode: &str) -> Option<String> {
    let qrcode_text = normalize_weixin_qr_image_value(raw_qrcode);
    if qrcode_text.is_empty() {
        return None;
    }
    let qrcode = qrcode::QrCode::new(qrcode_text.as_bytes()).ok()?;
    let width = qrcode.width();
    if width == 0 {
        return None;
    }
    let quiet_zone = 4usize;
    let scale = 8u32;
    let image_side = ((width + quiet_zone * 2) as u32).saturating_mul(scale);
    if image_side == 0 {
        return None;
    }

    let mut image = ImageBuffer::from_pixel(image_side, image_side, Luma([255u8]));
    let colors = qrcode.to_colors();
    for y in 0..width {
        for x in 0..width {
            let index = y.saturating_mul(width).saturating_add(x);
            if !matches!(colors.get(index), Some(QrColor::Dark)) {
                continue;
            }
            let px = ((x + quiet_zone) as u32).saturating_mul(scale);
            let py = ((y + quiet_zone) as u32).saturating_mul(scale);
            for dy in 0..scale {
                for dx in 0..scale {
                    image.put_pixel(px + dx, py + dy, Luma([0u8]));
                }
            }
        }
    }

    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(image)
        .write_to(&mut cursor, ImageFormat::Png)
        .ok()?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(cursor.into_inner());
    Some(format!("data:image/png;base64,{encoded}"))
}

async fn resolve_weixin_qr_preview_url(
    http: &reqwest::Client,
    api_base: &str,
    raw_value: &str,
) -> Option<String> {
    let normalized = normalize_weixin_qr_image_value(raw_value);
    if normalized.is_empty() {
        return None;
    }
    if normalized.starts_with("data:image/") {
        return Some(normalized);
    }

    let compact = normalized
        .chars()
        .filter(|char| !char.is_whitespace())
        .collect::<String>();
    let compact_lower = compact.to_ascii_lowercase();
    let base64_candidate = if compact_lower.starts_with("data:image/") {
        compact
            .split_once(',')
            .map(|(_, payload)| payload.to_string())
            .unwrap_or(compact)
    } else {
        compact
    };
    if looks_like_base64_image(&base64_candidate) {
        return Some(format!("data:image/png;base64,{base64_candidate}"));
    }

    let mut current_url = build_absolute_weixin_qr_url(&normalized, api_base)?;
    for _ in 0..2 {
        let response = http.get(&current_url).send().await.ok()?;
        if !response.status().is_success() {
            break;
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = response.bytes().await.ok()?;
        if bytes.is_empty() {
            break;
        }
        if let Some(mime) = detect_image_mime(&bytes, &content_type) {
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            return Some(format!("data:{mime};base64,{encoded}"));
        }
        let text = String::from_utf8_lossy(&bytes);
        if let Some(data_uri) = extract_data_image_uri(&text) {
            return Some(data_uri);
        }
        if let Some(next_src) = extract_first_img_src(&text) {
            if let Some(next_url) = build_absolute_weixin_qr_url(&next_src, api_base) {
                current_url = next_url;
                continue;
            }
        }
        break;
    }

    Some(current_url)
}

async fn resolve_weixin_qr_preview_image(
    http: &reqwest::Client,
    api_base: &str,
    qrcode_text: &str,
    raw_qrcode_url: &str,
) -> String {
    if let Some(png_data_uri) = build_weixin_qr_png_data_uri(qrcode_text) {
        return png_data_uri;
    }
    resolve_weixin_qr_preview_url(http, api_base, raw_qrcode_url)
        .await
        .unwrap_or_else(|| raw_qrcode_url.to_string())
}

#[derive(Debug, Deserialize)]
struct ChannelAccountsQuery {
    #[serde(default)]
    user_id: Option<String>,
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
    #[serde(default)]
    weixin: Option<WeixinAccountPayload>,
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
struct WeixinAccountPayload {
    #[serde(default, alias = "apiBase")]
    api_base: Option<String>,
    #[serde(default, alias = "cdnBase")]
    cdn_base: Option<String>,
    #[serde(default, alias = "botToken")]
    bot_token: Option<String>,
    #[serde(default, alias = "ilinkBotId")]
    ilink_bot_id: Option<String>,
    #[serde(default, alias = "ilinkUserId")]
    ilink_user_id: Option<String>,
    #[serde(default, alias = "botType")]
    bot_type: Option<String>,
    #[serde(default, alias = "longConnectionEnabled")]
    long_connection_enabled: Option<bool>,
    #[serde(default, alias = "pollTimeoutMs")]
    poll_timeout_ms: Option<u64>,
    #[serde(default, alias = "apiTimeoutMs")]
    api_timeout_ms: Option<u64>,
    #[serde(default, alias = "maxConsecutiveFailures")]
    max_consecutive_failures: Option<u64>,
    #[serde(default, alias = "backoffMs")]
    backoff_ms: Option<u64>,
    #[serde(default, alias = "routeTag")]
    route_tag: Option<String>,
    #[serde(default, alias = "allowFrom")]
    allow_from: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ChannelBindingsQuery {
    #[serde(default)]
    user_id: Option<String>,
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

#[derive(Debug, Deserialize)]
struct ChannelActionQuery {
    #[serde(default)]
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WeixinQrStartRequest {
    #[serde(default, alias = "accountId")]
    account_id: Option<String>,
    #[serde(default, alias = "apiBase")]
    api_base: Option<String>,
    #[serde(default, alias = "botType")]
    bot_type: Option<String>,
    #[serde(default)]
    force: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct WeixinQrWaitRequest {
    #[serde(alias = "sessionKey")]
    session_key: String,
    #[serde(default, alias = "apiBase")]
    api_base: Option<String>,
    #[serde(default, alias = "timeoutMs")]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct WeixinQrSession {
    session_key: String,
    user_id: String,
    qrcode: String,
    qrcode_url: String,
    api_base: String,
    bot_type: String,
    route_tag: Option<String>,
    started_at_ms: u64,
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
        .route(
            "/wunder/channels/weixin/qr/start",
            post(start_weixin_qr_login),
        )
        .route(
            "/wunder/channels/weixin/qr/wait",
            post(wait_weixin_qr_login),
        )
}

async fn list_channel_accounts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ChannelAccountsQuery>,
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
    Query(query): Query<ChannelActionQuery>,
    Json(payload): Json<ChannelAccountUpsertRequest>,
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
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_WEIXIN) {
        let existing_weixin = ChannelAccountConfig::from_value(&config_value)
            .weixin
            .unwrap_or_default();

        let api_base = payload
            .weixin
            .as_ref()
            .and_then(|value| value.api_base.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_weixin
                    .api_base
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "https://ilinkai.weixin.qq.com".to_string());
        let cdn_base = payload
            .weixin
            .as_ref()
            .and_then(|value| value.cdn_base.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_weixin
                    .cdn_base
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let bot_token = payload
            .weixin
            .as_ref()
            .and_then(|value| value.bot_token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_weixin
                    .bot_token
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "weixin bot_token is required".to_string(),
                )
            })?;
        let ilink_bot_id = payload
            .weixin
            .as_ref()
            .and_then(|value| value.ilink_bot_id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_weixin
                    .ilink_bot_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "weixin ilink_bot_id is required".to_string(),
                )
            })?;

        let ilink_user_id = payload
            .weixin
            .as_ref()
            .and_then(|value| value.ilink_user_id.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_weixin
                    .ilink_user_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let bot_type = payload
            .weixin
            .as_ref()
            .and_then(|value| value.bot_type.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_weixin
                    .bot_type
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let long_connection_enabled = payload
            .weixin
            .as_ref()
            .and_then(|value| value.long_connection_enabled)
            .or(existing_weixin.long_connection_enabled)
            .or(Some(true));
        let poll_timeout_ms = payload
            .weixin
            .as_ref()
            .and_then(|value| value.poll_timeout_ms)
            .or(existing_weixin.poll_timeout_ms);
        let api_timeout_ms = payload
            .weixin
            .as_ref()
            .and_then(|value| value.api_timeout_ms)
            .or(existing_weixin.api_timeout_ms);
        let max_consecutive_failures = payload
            .weixin
            .as_ref()
            .and_then(|value| value.max_consecutive_failures)
            .or(existing_weixin.max_consecutive_failures);
        let backoff_ms = payload
            .weixin
            .as_ref()
            .and_then(|value| value.backoff_ms)
            .or(existing_weixin.backoff_ms);
        let route_tag = payload
            .weixin
            .as_ref()
            .and_then(|value| value.route_tag.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                existing_weixin
                    .route_tag
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
        let allow_from = payload
            .weixin
            .as_ref()
            .and_then(|value| value.allow_from.as_ref())
            .map(|items| {
                items
                    .iter()
                    .map(|value| value.trim())
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| existing_weixin.allow_from.clone());
        selected_peer_kind = "user".to_string();

        let map = config_value.as_object_mut().ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "invalid channel config".to_string(),
            )
        })?;
        map.insert(
            "weixin".to_string(),
            json!(WeixinConfig {
                api_base: Some(api_base),
                cdn_base,
                bot_token: Some(bot_token),
                ilink_bot_id: Some(ilink_bot_id),
                ilink_user_id,
                bot_type,
                long_connection_enabled,
                allow_from,
                poll_timeout_ms,
                api_timeout_ms,
                max_consecutive_failures,
                backoff_ms,
                typing_enabled: existing_weixin.typing_enabled,
                media_enabled: existing_weixin.media_enabled,
                route_tag,
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
    if channel.eq_ignore_ascii_case(USER_CHANNEL_WEIXIN)
        && !matches!(selected_peer_kind.as_str(), "user")
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "weixin peer_kind must be user".to_string(),
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

    state.channels.record_runtime_info(
        &channel,
        Some(&account_id),
        "account_upserted",
        format!(
            "channel account upserted: status={status}, peer_kind={}, agent_id={}",
            selected_peer_kind,
            agent_id_for_binding.as_deref().unwrap_or("-")
        ),
    );
    if channel.eq_ignore_ascii_case(USER_CHANNEL_QQBOT) {
        let qqbot_cfg = ChannelAccountConfig::from_value(&config_value)
            .qqbot
            .unwrap_or_default();
        let app_id_set = qqbot::resolved_app_id(&qqbot_cfg).is_some();
        let client_secret_set = qqbot::resolved_client_secret(&qqbot_cfg).is_some();
        let token_set = qqbot_cfg
            .token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some();
        let long_connection_enabled = qqbot::long_connection_enabled(&qqbot_cfg);
        if app_id_set && client_secret_set {
            state.channels.record_runtime_info(
                USER_CHANNEL_QQBOT,
                Some(&account_id),
                "qqbot_config_ready",
                format!(
                    "qqbot config ready; callback path=/wunder/channel/qqbot/webhook, long_connection_enabled={long_connection_enabled}, token_set={token_set}"
                ),
            );
        } else {
            state.channels.record_runtime_warn(
                USER_CHANNEL_QQBOT,
                Some(&account_id),
                "qqbot_config_incomplete",
                format!(
                    "qqbot config incomplete: app_id_set={app_id_set}, client_secret_set={client_secret_set}, token_set={token_set}"
                ),
            );
        }
    }
    if channel.eq_ignore_ascii_case(USER_CHANNEL_WEIXIN) {
        let weixin_cfg = ChannelAccountConfig::from_value(&config_value)
            .weixin
            .unwrap_or_default();
        let long_connection_enabled = weixin::long_connection_enabled(&weixin_cfg);
        let configured = weixin::has_long_connection_credentials(&weixin_cfg);
        if configured {
            state.channels.record_runtime_info(
                USER_CHANNEL_WEIXIN,
                Some(&account_id),
                "weixin_config_ready",
                format!(
                    "weixin config ready; runtime_mode=long_poll, long_connection_enabled={long_connection_enabled}"
                ),
            );
        } else {
            state.channels.record_runtime_warn(
                USER_CHANNEL_WEIXIN,
                Some(&account_id),
                "weixin_config_incomplete",
                "weixin config incomplete: api_base/bot_token/ilink_bot_id missing".to_string(),
            );
        }
    }

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

async fn start_weixin_qr_login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ChannelActionQuery>,
    Json(payload): Json<WeixinQrStartRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    let account_id = payload
        .account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let force = payload.force.unwrap_or(false);

    let mut account_route_tag: Option<String> = None;
    let mut account_api_base: Option<String> = None;
    let mut account_bot_type: Option<String> = None;

    if let Some(target_account_id) = account_id.as_deref() {
        if !user_owns_channel_account(&state, &user_id, USER_CHANNEL_WEIXIN, target_account_id)? {
            return Err(error_response(
                StatusCode::FORBIDDEN,
                i18n::t("error.permission_denied"),
            ));
        }
        let record = state
            .storage
            .get_channel_account(USER_CHANNEL_WEIXIN, target_account_id)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .ok_or_else(|| {
                error_response(
                    StatusCode::NOT_FOUND,
                    "channel account not found".to_string(),
                )
            })?;
        if let Some(weixin_cfg) = ChannelAccountConfig::from_value(&record.config).weixin {
            account_route_tag = weixin_cfg
                .route_tag
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            account_api_base = weixin_cfg
                .api_base
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            account_bot_type = weixin_cfg
                .bot_type
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
        }
    }

    let api_base =
        weixin::normalize_api_base(payload.api_base.as_deref().or(account_api_base.as_deref()))
            .unwrap_or_else(|| weixin::DEFAULT_API_BASE.to_string());
    let bot_type =
        weixin::normalize_bot_type(payload.bot_type.as_deref().or(account_bot_type.as_deref()));
    let route_tag = account_route_tag;
    let session_key = account_id
        .clone()
        .unwrap_or_else(|| format!("wxqr_{}", Uuid::new_v4().simple()));
    let http = reqwest::Client::new();

    purge_expired_weixin_qr_sessions();
    if !force {
        if let Some(existing) = load_weixin_qr_session(&session_key) {
            if existing.user_id == user_id && !is_weixin_qr_session_expired(&existing) {
                let preview_qrcode_url = resolve_weixin_qr_preview_image(
                    &http,
                    &existing.api_base,
                    &existing.qrcode,
                    &existing.qrcode_url,
                )
                .await;
                if preview_qrcode_url != existing.qrcode_url {
                    let mut refreshed = existing.clone();
                    refreshed.qrcode_url = preview_qrcode_url.clone();
                    save_weixin_qr_session(refreshed);
                }
                return Ok(Json(json!({ "data": {
                    "session_key": existing.session_key,
                    "qrcode": existing.qrcode,
                    "qrcode_url": preview_qrcode_url,
                    "api_base": existing.api_base,
                    "bot_type": existing.bot_type,
                    "cached": true,
                }})));
            }
        }
    }

    let qr = weixin::get_bot_qrcode(
        &http,
        &api_base,
        &bot_type,
        route_tag.as_deref(),
        WEIXIN_QR_STATUS_LONG_POLL_TIMEOUT_MS,
    )
    .await
    .map_err(|err| error_response(StatusCode::BAD_GATEWAY, err.to_string()))?;
    let qrcode = qr
        .qrcode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_GATEWAY,
                "weixin qr response missing qrcode".to_string(),
            )
        })?;
    let raw_qrcode_url = qr
        .qrcode_img_content
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_GATEWAY,
                "weixin qr response missing qrcode image".to_string(),
            )
        })?;
    let qrcode_url =
        resolve_weixin_qr_preview_image(&http, &api_base, &qrcode, &raw_qrcode_url).await;

    let session = WeixinQrSession {
        session_key: session_key.clone(),
        user_id: user_id.clone(),
        qrcode: qrcode.clone(),
        qrcode_url: qrcode_url.clone(),
        api_base: api_base.clone(),
        bot_type: bot_type.clone(),
        route_tag,
        started_at_ms: now_ms(),
    };
    save_weixin_qr_session(session);

    state.channels.record_runtime_info(
        USER_CHANNEL_WEIXIN,
        account_id.as_deref(),
        "qr_login_started",
        format!("weixin qr login started: session_key={session_key}"),
    );

    Ok(Json(json!({ "data": {
        "session_key": session_key,
        "qrcode": qrcode,
        "qrcode_url": qrcode_url,
        "api_base": api_base,
        "bot_type": bot_type,
    }})))
}

async fn wait_weixin_qr_login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ChannelActionQuery>,
    Json(payload): Json<WeixinQrWaitRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id.clone();
    let session_key = payload.session_key.trim().to_string();
    if session_key.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "session_key is required".to_string(),
        ));
    }

    purge_expired_weixin_qr_sessions();
    let session = load_weixin_qr_session(&session_key).ok_or_else(|| {
        error_response(
            StatusCode::NOT_FOUND,
            "weixin qr session not found or expired".to_string(),
        )
    })?;
    if session.user_id != user_id {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.permission_denied"),
        ));
    }
    if is_weixin_qr_session_expired(&session) {
        remove_weixin_qr_session(&session_key);
        return Err(error_response(
            StatusCode::GONE,
            "weixin qr session expired".to_string(),
        ));
    }

    let api_base = weixin::normalize_api_base(payload.api_base.as_deref())
        .unwrap_or_else(|| session.api_base.clone());
    let timeout_ms = payload.timeout_ms.unwrap_or(120_000).clamp(1_000, 480_000);
    let deadline = now_ms().saturating_add(timeout_ms);

    let http = reqwest::Client::new();
    loop {
        let status_resp = weixin::get_qrcode_status(
            &http,
            &api_base,
            &session.qrcode,
            session.route_tag.as_deref(),
            WEIXIN_QR_STATUS_LONG_POLL_TIMEOUT_MS,
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_GATEWAY, err.to_string()))?;

        let status = status_resp
            .status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("wait")
            .to_ascii_lowercase();

        if status == "confirmed" {
            remove_weixin_qr_session(&session_key);
            state.channels.record_runtime_info(
                USER_CHANNEL_WEIXIN,
                None,
                "qr_login_confirmed",
                format!("weixin qr login confirmed: session_key={session_key}"),
            );
            return Ok(Json(json!({ "data": {
                "connected": true,
                "status": status,
                "session_key": session_key,
                "bot_token": status_resp.bot_token,
                "ilink_bot_id": status_resp.ilink_bot_id,
                "ilink_user_id": status_resp.ilink_user_id,
                "api_base": status_resp.baseurl.unwrap_or(api_base),
            }})));
        }

        if status == "expired" {
            remove_weixin_qr_session(&session_key);
            state.channels.record_runtime_warn(
                USER_CHANNEL_WEIXIN,
                None,
                "qr_login_expired",
                format!("weixin qr login expired: session_key={session_key}"),
            );
            return Ok(Json(json!({ "data": {
                "connected": false,
                "status": status,
                "session_key": session_key,
                "message": "weixin qr expired",
            }})));
        }

        if now_ms() >= deadline {
            return Ok(Json(json!({ "data": {
                "connected": false,
                "status": status,
                "session_key": session_key,
                "message": "waiting for weixin qr confirmation timed out",
            }})));
        }
        sleep(Duration::from_millis(WEIXIN_QR_WAIT_POLL_INTERVAL_MS)).await;
    }
}

async fn delete_channel_account_by_id(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ChannelActionQuery>,
    AxumPath((channel, account_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
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
    Query(query): Query<ChannelActionQuery>,
    AxumPath(channel): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
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
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
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
    Query(query): Query<ChannelActionQuery>,
    Json(payload): Json<ChannelBindingUpsertRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
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
    if channel.eq_ignore_ascii_case(USER_CHANNEL_WEIXIN) && !matches!(peer_kind.as_str(), "user") {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "weixin peer_kind must be user".to_string(),
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
    Query(query): Query<ChannelActionQuery>,
    AxumPath((channel, account_id, peer_kind, peer_id)): AxumPath<(String, String, String, String)>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
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
                "name": item.display_name,
                "label": item.display_name,
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
        || channel.eq_ignore_ascii_case(USER_CHANNEL_WECHAT_MP)
        || channel.eq_ignore_ascii_case(USER_CHANNEL_WEIXIN))
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
        let app_id = qqbot::resolved_app_id(&qqbot).unwrap_or_default();
        let client_secret_set = qqbot::resolved_client_secret(&qqbot).is_some();
        let token_set = qqbot
            .token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some();
        long_connection_enabled = qqbot::long_connection_enabled(&qqbot);
        configured = !app_id.is_empty() && client_secret_set;
        config_preview = json!({
            "qqbot": {
                "app_id": app_id,
                "client_secret_set": client_secret_set,
                "token_set": token_set,
                "markdown_support": qqbot.markdown_support,
                "long_connection_enabled": long_connection_enabled,
                "intents": qqbot.intents,
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
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_WEIXIN) {
        let weixin_cfg = account_cfg.weixin.unwrap_or_default();
        let api_base = weixin_cfg
            .api_base
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("https://ilinkai.weixin.qq.com")
            .to_string();
        let cdn_base = weixin_cfg
            .cdn_base
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("https://novac2c.cdn.weixin.qq.com/c2c")
            .to_string();
        let bot_token_set = weixin_cfg
            .bot_token
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        let ilink_bot_id = weixin_cfg
            .ilink_bot_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let ilink_user_id = weixin_cfg
            .ilink_user_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let bot_type = weixin_cfg
            .bot_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("3")
            .to_string();
        let allow_from_count = weixin_cfg
            .allow_from
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .count();
        long_connection_enabled = weixin::long_connection_enabled(&weixin_cfg);
        configured = weixin::has_long_connection_credentials(&weixin_cfg);
        config_preview = json!({
            "weixin": {
                "api_base": api_base,
                "cdn_base": cdn_base,
                "bot_token_set": bot_token_set,
                "ilink_bot_id": ilink_bot_id,
                "ilink_user_id": ilink_user_id,
                "bot_type": bot_type,
                "allow_from_count": allow_from_count,
                "poll_timeout_ms": weixin_cfg.poll_timeout_ms,
                "api_timeout_ms": weixin_cfg.api_timeout_ms,
                "max_consecutive_failures": weixin_cfg.max_consecutive_failures,
                "backoff_ms": weixin_cfg.backoff_ms,
                "long_connection_enabled": long_connection_enabled,
                "route_tag_set": weixin_cfg
                    .route_tag
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_some(),
            }
        });
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_XMPP) {
        let xmpp = account_cfg.xmpp.unwrap_or_default();
        let jid = xmpp
            .jid
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let password_set = xmpp
            .password
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        let password_env = xmpp
            .password_env
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let password_env_set = is_non_empty_env_var(password_env.as_deref());
        let domain = xmpp
            .domain
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let host = xmpp
            .host
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        long_connection_enabled = xmpp.long_connection_enabled.unwrap_or(true);
        configured = !jid.is_empty() && (password_set || password_env_set);
        config_preview = json!({
            "xmpp": {
                "jid": jid,
                "password_set": password_set,
                "password_env": password_env,
                "password_env_set": password_env_set,
                "domain": domain,
                "host": host,
                "port": xmpp.port,
                "direct_tls": xmpp.direct_tls,
                "trust_self_signed": xmpp.trust_self_signed,
                "tls_enabled": xmpp.tls_enabled,
                "custom_message_format_enabled": xmpp.custom_message_format_enabled,
                "muc_rooms": xmpp.muc_rooms.len(),
                "heartbeat_enabled": xmpp.heartbeat_enabled,
                "heartbeat_interval_s": xmpp.heartbeat_interval_s,
                "heartbeat_timeout_s": xmpp.heartbeat_timeout_s,
                "respond_ping": xmpp.respond_ping,
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
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_QQBOT) {
        if let Some(meta_map) = meta.as_object_mut() {
            meta_map.insert(
                "long_connection_enabled".to_string(),
                Value::Bool(long_connection_enabled),
            );
        }
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_WEIXIN) {
        if let Some(meta_map) = meta.as_object_mut() {
            meta_map.insert(
                "long_connection_enabled".to_string(),
                Value::Bool(long_connection_enabled),
            );
        }
    } else if channel.eq_ignore_ascii_case(USER_CHANNEL_XMPP) {
        if let Some(meta_map) = meta.as_object_mut() {
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
    if channel.eq_ignore_ascii_case(USER_CHANNEL_WEIXIN) {
        return "user".to_string();
    }
    if channel.eq_ignore_ascii_case(USER_CHANNEL_XMPP) {
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
        || channel.trim().eq_ignore_ascii_case(USER_CHANNEL_WECHAT_MP)
        || channel.trim().eq_ignore_ascii_case(USER_CHANNEL_WEIXIN)
        || channel.trim().eq_ignore_ascii_case(USER_CHANNEL_XMPP))
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

fn is_non_empty_env_var(env_name: Option<&str>) -> bool {
    let Some(name) = env_name.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };

    std::env::var(name)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn weixin_qr_sessions() -> &'static Mutex<HashMap<String, WeixinQrSession>> {
    static STORE: OnceLock<Mutex<HashMap<String, WeixinQrSession>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn is_weixin_qr_session_expired(session: &WeixinQrSession) -> bool {
    now_ms().saturating_sub(session.started_at_ms) > WEIXIN_QR_SESSION_TTL_MS
}

fn purge_expired_weixin_qr_sessions() {
    let store = weixin_qr_sessions();
    let mut guard = store.lock().expect("weixin qr session lock poisoned");
    guard.retain(|_, session| !is_weixin_qr_session_expired(session));
}

fn load_weixin_qr_session(session_key: &str) -> Option<WeixinQrSession> {
    let store = weixin_qr_sessions();
    let guard = store.lock().expect("weixin qr session lock poisoned");
    guard.get(session_key).cloned()
}

fn save_weixin_qr_session(session: WeixinQrSession) {
    let store = weixin_qr_sessions();
    let mut guard = store.lock().expect("weixin qr session lock poisoned");
    guard.insert(session.session_key.clone(), session);
}

fn remove_weixin_qr_session(session_key: &str) {
    let store = weixin_qr_sessions();
    let mut guard = store.lock().expect("weixin qr session lock poisoned");
    guard.remove(session_key);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn qr_session_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_qr_sessions() {
        let store = weixin_qr_sessions();
        let mut guard = store.lock().expect("weixin qr session lock poisoned");
        guard.clear();
    }

    fn make_qr_session(session_key: &str, started_at_ms: u64) -> WeixinQrSession {
        WeixinQrSession {
            session_key: session_key.to_string(),
            user_id: "u_test".to_string(),
            qrcode: "qrcode-text".to_string(),
            qrcode_url: "https://example.com/qr".to_string(),
            api_base: "https://api.ilinknetwork.com".to_string(),
            bot_type: "2".to_string(),
            route_tag: Some("test-tag".to_string()),
            started_at_ms,
        }
    }

    #[test]
    fn weixin_qr_session_save_and_load_roundtrip() {
        let _guard = qr_session_test_lock()
            .lock()
            .expect("qr session test lock poisoned");
        clear_qr_sessions();

        let session = make_qr_session("session-roundtrip", now_ms());
        save_weixin_qr_session(session.clone());
        let loaded = load_weixin_qr_session("session-roundtrip").expect("session should exist");

        assert_eq!(loaded.session_key, session.session_key);
        assert_eq!(loaded.user_id, session.user_id);
        assert_eq!(loaded.qrcode, session.qrcode);
        assert_eq!(loaded.qrcode_url, session.qrcode_url);
        assert_eq!(loaded.api_base, session.api_base);
        assert_eq!(loaded.bot_type, session.bot_type);
        assert_eq!(loaded.route_tag, session.route_tag);
        clear_qr_sessions();
    }

    #[test]
    fn weixin_qr_session_remove_deletes_entry() {
        let _guard = qr_session_test_lock()
            .lock()
            .expect("qr session test lock poisoned");
        clear_qr_sessions();

        save_weixin_qr_session(make_qr_session("session-remove", now_ms()));
        assert!(load_weixin_qr_session("session-remove").is_some());

        remove_weixin_qr_session("session-remove");
        assert!(load_weixin_qr_session("session-remove").is_none());
        clear_qr_sessions();
    }

    #[test]
    fn purge_expired_weixin_qr_sessions_keeps_only_active_entries() {
        let _guard = qr_session_test_lock()
            .lock()
            .expect("qr session test lock poisoned");
        clear_qr_sessions();

        let now = now_ms();
        let expired_started_at_ms = now.saturating_sub(WEIXIN_QR_SESSION_TTL_MS + 10);
        save_weixin_qr_session(make_qr_session("session-expired", expired_started_at_ms));
        save_weixin_qr_session(make_qr_session("session-active", now));

        purge_expired_weixin_qr_sessions();
        assert!(load_weixin_qr_session("session-expired").is_none());
        assert!(load_weixin_qr_session("session-active").is_some());
        clear_qr_sessions();
    }

    #[test]
    fn build_weixin_qr_png_data_uri_returns_png_prefix() {
        let uri = build_weixin_qr_png_data_uri("wxqr_test_payload")
            .expect("png data uri should be generated");
        assert!(uri.starts_with("data:image/png;base64,"));
        let encoded = uri
            .strip_prefix("data:image/png;base64,")
            .expect("png data uri prefix should exist");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .expect("png base64 should decode");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]));
    }
}
