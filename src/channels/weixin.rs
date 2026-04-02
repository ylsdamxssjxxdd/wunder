use crate::channels::adapter::{ChannelAdapter, OutboundContext};
use crate::channels::outbound_attachments::{
    merge_attachments_with_text_links, OutboundLinkExtractionMode,
};
use crate::channels::types::{
    ChannelAttachment, ChannelMessage, ChannelOutboundMessage, ChannelPeer, ChannelSender,
    WeixinConfig,
};
use aes::cipher::{Block, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use base64::Engine;
use regex::Regex;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs;
use tracing::warn;
use url::{form_urlencoded::byte_serialize, Url};
use uuid::Uuid;

pub const WEIXIN_CHANNEL: &str = "weixin";
pub const DEFAULT_API_BASE: &str = "https://ilinkai.weixin.qq.com";
pub const DEFAULT_CDN_BASE: &str = "https://novac2c.cdn.weixin.qq.com/c2c";
pub const DEFAULT_QR_BOT_TYPE: &str = "3";
const DEFAULT_POLL_TIMEOUT_MS: u64 = 35_000;
const DEFAULT_API_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_BACKOFF_MS: u64 = 8_000;
const DEFAULT_MAX_CONSECUTIVE_FAILURES: u64 = 2;
const MAX_MEDIA_BYTES: usize = 100 * 1024 * 1024;
const AES_BLOCK_SIZE: usize = 16;

#[derive(Debug, Default)]
pub struct WeixinAdapter;

#[async_trait]
impl ChannelAdapter for WeixinAdapter {
    fn channel(&self) -> &'static str {
        WEIXIN_CHANNEL
    }

    async fn send_outbound(&self, context: OutboundContext<'_>) -> Result<()> {
        let config = context
            .account_config
            .weixin
            .as_ref()
            .ok_or_else(|| anyhow!("weixin config missing"))?;
        send_outbound(context.http, context.outbound, config).await
    }

    async fn health_check(
        &self,
        _http: &Client,
        account_config: &crate::channels::types::ChannelAccountConfig,
    ) -> Result<Value> {
        let status = match account_config.weixin.as_ref() {
            Some(config) if has_long_connection_credentials(config) => "configured",
            Some(_) => "missing_credentials",
            None => "not_configured",
        };
        Ok(json!({
            "status": status,
            "long_connection_enabled": account_config
                .weixin
                .as_ref()
                .map(long_connection_enabled)
                .unwrap_or(false),
            "media_enabled": account_config
                .weixin
                .as_ref()
                .map(media_enabled)
                .unwrap_or(true),
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinTextItem {
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinCdnMedia {
    #[serde(default, alias = "encryptQueryParam")]
    pub encrypt_query_param: Option<String>,
    #[serde(default, alias = "aesKey")]
    pub aes_key: Option<String>,
    #[serde(default, alias = "encryptType")]
    pub encrypt_type: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinImageItem {
    #[serde(default)]
    pub media: Option<WeixinCdnMedia>,
    #[serde(default, alias = "thumbMedia")]
    pub thumb_media: Option<WeixinCdnMedia>,
    #[serde(default, alias = "aes_key", alias = "aesKey")]
    pub aeskey: Option<String>,
    #[serde(default, alias = "midSize")]
    pub mid_size: Option<u64>,
    #[serde(default, alias = "thumbSize")]
    pub thumb_size: Option<u64>,
    #[serde(default, alias = "hdSize")]
    pub hd_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinVoiceItem {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub media: Option<WeixinCdnMedia>,
    #[serde(default, alias = "encodeType")]
    pub encode_type: Option<i64>,
    #[serde(default, alias = "sampleRate")]
    pub sample_rate: Option<i64>,
    #[serde(default)]
    pub playtime: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinFileItem {
    #[serde(default)]
    pub media: Option<WeixinCdnMedia>,
    #[serde(default, alias = "fileName")]
    pub file_name: Option<String>,
    #[serde(default)]
    pub md5: Option<String>,
    #[serde(default)]
    pub len: Option<String>,
    #[serde(default, alias = "encryptQueryParam")]
    pub encrypt_query_param: Option<String>,
    #[serde(default, alias = "aesKey")]
    pub aes_key: Option<String>,
    #[serde(default, alias = "aesHexKey")]
    pub aeskey: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinVideoItem {
    #[serde(default)]
    pub media: Option<WeixinCdnMedia>,
    #[serde(default, alias = "videoSize")]
    pub video_size: Option<u64>,
    #[serde(default, alias = "thumbMedia")]
    pub thumb_media: Option<WeixinCdnMedia>,
    #[serde(default, alias = "encryptQueryParam")]
    pub encrypt_query_param: Option<String>,
    #[serde(default, alias = "aesKey")]
    pub aes_key: Option<String>,
    #[serde(default, alias = "aesHexKey")]
    pub aeskey: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinMessageItem {
    #[serde(default, rename = "type")]
    pub item_type: Option<i64>,
    #[serde(default, alias = "textItem")]
    pub text_item: Option<WeixinTextItem>,
    #[serde(default, alias = "voiceItem")]
    pub voice_item: Option<WeixinVoiceItem>,
    #[serde(default, alias = "imageItem")]
    pub image_item: Option<WeixinImageItem>,
    #[serde(default, alias = "fileItem")]
    pub file_item: Option<WeixinFileItem>,
    #[serde(default, alias = "videoItem")]
    pub video_item: Option<WeixinVideoItem>,
    #[serde(default, alias = "msgId")]
    pub msg_id: Option<Value>,
    #[serde(default, alias = "createTimeMs")]
    pub create_time_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinInboundMessage {
    #[serde(default)]
    pub seq: Option<i64>,
    #[serde(default, alias = "messageId")]
    pub message_id: Option<Value>,
    #[serde(default, alias = "fromUserId")]
    pub from_user_id: Option<String>,
    #[serde(default, alias = "toUserId")]
    pub to_user_id: Option<String>,
    #[serde(default, alias = "clientId")]
    pub client_id: Option<String>,
    #[serde(default, alias = "createTimeMs")]
    pub create_time_ms: Option<i64>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub message_type: Option<i64>,
    #[serde(default)]
    pub message_state: Option<i64>,
    #[serde(default)]
    pub item_list: Vec<WeixinMessageItem>,
    #[serde(default)]
    pub context_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinGetUpdatesResponse {
    #[serde(default)]
    pub ret: i64,
    #[serde(default)]
    pub errcode: Option<i64>,
    #[serde(default)]
    pub errmsg: Option<String>,
    #[serde(default)]
    pub msgs: Vec<WeixinInboundMessage>,
    #[serde(default)]
    pub get_updates_buf: Option<String>,
    #[serde(default)]
    pub longpolling_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinQrCodeResponse {
    #[serde(default)]
    pub ret: Option<i64>,
    #[serde(default)]
    pub errcode: Option<i64>,
    #[serde(default)]
    pub errmsg: Option<String>,
    #[serde(default)]
    pub qrcode: Option<String>,
    #[serde(default)]
    pub qrcode_img_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinQrStatusResponse {
    #[serde(default)]
    pub ret: Option<i64>,
    #[serde(default)]
    pub errcode: Option<i64>,
    #[serde(default)]
    pub errmsg: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub bot_token: Option<String>,
    #[serde(default)]
    pub ilink_bot_id: Option<String>,
    #[serde(default)]
    pub baseurl: Option<String>,
    #[serde(default)]
    pub ilink_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeixinInboundMediaEntry {
    pub kind: String,
    pub encrypt_query_param: String,
    #[serde(default)]
    pub aes_key: Option<String>,
    #[serde(default)]
    pub aes_hex_key: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub mime_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeixinOutboundMediaKind {
    Image,
    Video,
    File,
    Voice,
}

impl WeixinOutboundMediaKind {
    fn upload_media_type(self) -> i64 {
        match self {
            Self::Image => 1,
            Self::Video => 2,
            Self::File => 3,
            Self::Voice => 4,
        }
    }
}

#[derive(Debug)]
struct LoadedAttachment {
    bytes: Vec<u8>,
    filename: Option<String>,
    media_kind: WeixinOutboundMediaKind,
}

#[derive(Debug)]
struct UploadedMediaRef {
    download_encrypted_query_param: String,
    aes_key_hex: String,
    raw_size: usize,
    cipher_size: usize,
}

pub fn long_connection_enabled(config: &WeixinConfig) -> bool {
    config.long_connection_enabled.unwrap_or(true)
}

pub fn media_enabled(config: &WeixinConfig) -> bool {
    config.media_enabled.unwrap_or(true)
}

pub fn has_long_connection_credentials(config: &WeixinConfig) -> bool {
    resolve_api_base_url(config).is_some()
        && trimmed_non_empty(config.bot_token.as_deref()).is_some()
        && trimmed_non_empty(config.ilink_bot_id.as_deref()).is_some()
}

pub fn resolve_poll_timeout_ms(config: &WeixinConfig) -> u64 {
    config
        .poll_timeout_ms
        .unwrap_or(DEFAULT_POLL_TIMEOUT_MS)
        .max(1_000)
}

pub fn resolve_api_timeout_ms(config: &WeixinConfig) -> u64 {
    config
        .api_timeout_ms
        .unwrap_or(DEFAULT_API_TIMEOUT_MS)
        .max(1_000)
}

pub fn resolve_backoff_ms(config: &WeixinConfig) -> u64 {
    config.backoff_ms.unwrap_or(DEFAULT_BACKOFF_MS).max(1_000)
}

pub fn resolve_max_consecutive_failures(config: &WeixinConfig) -> u64 {
    config
        .max_consecutive_failures
        .unwrap_or(DEFAULT_MAX_CONSECUTIVE_FAILURES)
        .max(1)
}

pub fn normalize_api_base(raw: Option<&str>) -> Option<String> {
    normalize_base_url(raw, DEFAULT_API_BASE)
}

pub fn normalize_cdn_base(raw: Option<&str>) -> Option<String> {
    normalize_base_url(raw, DEFAULT_CDN_BASE)
}

pub fn normalize_bot_type(raw: Option<&str>) -> String {
    trimmed_non_empty(raw)
        .filter(|value| !value.eq_ignore_ascii_case("0"))
        .unwrap_or_else(|| DEFAULT_QR_BOT_TYPE.to_string())
}

pub fn resolve_cdn_base_url(config: &WeixinConfig) -> String {
    normalize_cdn_base(config.cdn_base.as_deref()).unwrap_or_else(|| DEFAULT_CDN_BASE.to_string())
}

pub fn build_cdn_download_url(config: &WeixinConfig, encrypted_query_param: &str) -> String {
    let base = resolve_cdn_base_url(config);
    let encoded = encode_query_value(encrypted_query_param);
    format!("{base}/download?encrypted_query_param={encoded}")
}

pub async fn get_updates(
    http: &Client,
    config: &WeixinConfig,
    get_updates_buf: &str,
    timeout_ms: u64,
) -> Result<WeixinGetUpdatesResponse> {
    let payload = post_json(
        http,
        config,
        "ilink/bot/getupdates",
        json!({
            "get_updates_buf": get_updates_buf,
            "base_info": build_base_info(),
        }),
        timeout_ms,
    )
    .await?;
    serde_json::from_value(payload).map_err(|err| anyhow!("weixin getupdates decode failed: {err}"))
}

pub async fn get_bot_qrcode(
    http: &Client,
    api_base: &str,
    bot_type: &str,
    route_tag: Option<&str>,
    timeout_ms: u64,
) -> Result<WeixinQrCodeResponse> {
    let normalized_base = normalize_api_base(Some(api_base))
        .ok_or_else(|| anyhow!("weixin api_base missing for get_bot_qrcode"))?;
    let encoded_bot_type = encode_query_value(bot_type);
    let url = format!("{normalized_base}/ilink/bot/get_bot_qrcode?bot_type={encoded_bot_type}");
    let mut request = http
        .get(&url)
        .timeout(Duration::from_millis(timeout_ms.max(1_000)));
    let mut headers = HeaderMap::new();
    if let Some(tag) = trimmed_non_empty(route_tag) {
        headers.insert(
            HeaderName::from_static("skroutetag"),
            HeaderValue::from_str(&tag)
                .map_err(|err| anyhow!("invalid SKRouteTag header: {err}"))?,
        );
    }
    if !headers.is_empty() {
        request = request.headers(headers);
    }

    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "weixin get_bot_qrcode failed: status={status}, body={}",
            truncate_text(&body, 512)
        ));
    }

    let payload = if body.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(&body).map_err(|err| {
            anyhow!(
                "weixin get_bot_qrcode decode failed: error={err}, body={}",
                truncate_text(&body, 512)
            )
        })?
    };
    ensure_weixin_ret_ok(&payload, "weixin get_bot_qrcode")?;
    serde_json::from_value(payload)
        .map_err(|err| anyhow!("weixin get_bot_qrcode parse failed: {err}"))
}

pub async fn get_qrcode_status(
    http: &Client,
    api_base: &str,
    qrcode: &str,
    route_tag: Option<&str>,
    timeout_ms: u64,
) -> Result<WeixinQrStatusResponse> {
    let normalized_base = normalize_api_base(Some(api_base))
        .ok_or_else(|| anyhow!("weixin api_base missing for get_qrcode_status"))?;
    let encoded_qrcode = encode_query_value(qrcode);
    let url = format!("{normalized_base}/ilink/bot/get_qrcode_status?qrcode={encoded_qrcode}");

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("ilink-app-clientversion"),
        HeaderValue::from_static("1"),
    );
    if let Some(tag) = trimmed_non_empty(route_tag) {
        headers.insert(
            HeaderName::from_static("skroutetag"),
            HeaderValue::from_str(&tag)
                .map_err(|err| anyhow!("invalid SKRouteTag header: {err}"))?,
        );
    }

    let response = http
        .get(&url)
        .headers(headers)
        .timeout(Duration::from_millis(timeout_ms.max(1_000)))
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "weixin get_qrcode_status failed: status={status}, body={}",
            truncate_text(&body, 512)
        ));
    }

    let payload = if body.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(&body).map_err(|err| {
            anyhow!(
                "weixin get_qrcode_status decode failed: error={err}, body={}",
                truncate_text(&body, 512)
            )
        })?
    };
    ensure_weixin_ret_ok(&payload, "weixin get_qrcode_status")?;
    serde_json::from_value(payload)
        .map_err(|err| anyhow!("weixin get_qrcode_status parse failed: {err}"))
}

pub fn extract_inbound_messages(
    source: &[WeixinInboundMessage],
    account_id: &str,
    config: &WeixinConfig,
) -> Vec<ChannelMessage> {
    let allow_from = config
        .allow_from
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    let mut output = Vec::new();
    for message in source {
        let peer_id = message
            .from_user_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let Some(peer_id) = peer_id else {
            continue;
        };
        if !allow_from.is_empty() && !allow_from.iter().any(|value| value == &peer_id) {
            continue;
        }

        let (attachments, media_entries) = extract_media_attachments(&message.item_list, config);
        let text = extract_item_text(&message.item_list)
            .or_else(|| first_item_type(&message.item_list).map(placeholder_for_item_type));
        let text = text
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if text.is_none() && attachments.is_empty() {
            continue;
        }

        let message_id = message
            .message_id
            .as_ref()
            .and_then(value_to_trimmed_string)
            .or_else(|| extract_item_message_id(&message.item_list))
            .or_else(|| trimmed_non_empty(message.client_id.as_deref()))
            .or_else(|| fallback_message_id(message, account_id, &peer_id));

        let ts = message
            .create_time_ms
            .or_else(|| extract_item_create_time_ms(&message.item_list))
            .map(|value| (value as f64 / 1000.0).max(0.0));

        output.push(ChannelMessage {
            channel: WEIXIN_CHANNEL.to_string(),
            account_id: account_id.to_string(),
            peer: ChannelPeer {
                kind: "user".to_string(),
                id: peer_id.clone(),
                name: None,
            },
            thread: None,
            message_id,
            sender: Some(ChannelSender {
                id: peer_id,
                name: None,
            }),
            message_type: "text".to_string(),
            text,
            attachments,
            location: None,
            ts,
            meta: Some(json!({
                "weixin": message,
                "context_token": message.context_token,
                "session_id": message.session_id,
                "weixin_media_entries": media_entries,
            })),
        });
    }
    output
}

pub async fn send_outbound(
    http: &Client,
    outbound: &ChannelOutboundMessage,
    config: &WeixinConfig,
) -> Result<()> {
    let to_user_id = outbound.peer.id.trim();
    if to_user_id.is_empty() {
        return Err(anyhow!("weixin outbound peer id missing"));
    }

    let context_token = extract_context_token_from_meta(outbound.meta.as_ref())
        .ok_or_else(|| anyhow!("weixin outbound context_token missing"))?;

    let text = outbound
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let outbound_attachments = collect_outbound_attachments(&outbound.attachments, text.as_deref());

    if outbound_attachments.is_empty() {
        let text = text.unwrap_or_else(|| "(empty message)".to_string());
        send_text_message(http, config, to_user_id, &context_token, &text).await?;
        return Ok(());
    }

    if !media_enabled(config) {
        let fallback = render_attachment_fallback(text.as_deref(), &outbound_attachments);
        send_text_message(http, config, to_user_id, &context_token, &fallback).await?;
        return Ok(());
    }

    let text_item = text_item_for_outbound_with_attachments(text.as_deref(), &outbound_attachments);
    let mut item_list = Vec::new();
    if let Some(text_item) = text_item.as_deref() {
        item_list.push(build_text_message_item(text_item));
    }
    let mut skipped_attachments: Vec<ChannelAttachment> = Vec::new();
    for attachment in &outbound_attachments {
        match build_attachment_message_item(http, config, to_user_id, attachment).await {
            Ok(item) => {
                item_list.push(item);
            }
            Err(err) if should_skip_attachment_delivery_error(&err) => {
                warn!(
                    "weixin outbound attachment skipped: to_user_id={}, source={}, error={err}",
                    to_user_id, attachment.url
                );
                skipped_attachments.push(attachment.clone());
            }
            Err(err) => return Err(err),
        }
    }
    if item_list.is_empty() {
        let fallback = render_missing_attachment_notice(&skipped_attachments)
            .or_else(|| text_item.clone())
            .unwrap_or_else(|| "(empty message)".to_string());
        item_list.push(build_text_message_item(&fallback));
    }
    send_message_items(http, config, to_user_id, &context_token, item_list).await?;
    Ok(())
}

fn collect_outbound_attachments(
    outbound_attachments: &[ChannelAttachment],
    text: Option<&str>,
) -> Vec<ChannelAttachment> {
    merge_attachments_with_text_links(
        outbound_attachments,
        text,
        OutboundLinkExtractionMode::WorkspaceResource,
    )
}

fn text_item_for_outbound_with_attachments(
    text: Option<&str>,
    attachments: &[ChannelAttachment],
) -> Option<String> {
    let trimmed = text.map(str::trim).filter(|value| !value.is_empty())?;
    if attachments.is_empty() {
        return Some(trimmed.to_string());
    }

    let mut cleaned = trimmed.to_string();
    for attachment in attachments {
        let source = attachment.url.trim();
        if source.is_empty() {
            continue;
        }
        cleaned = strip_markdown_link_for_source(&cleaned, source);
        cleaned = cleaned.replace(source, "");
    }

    let normalized_lines = cleaned
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if normalized_lines.is_empty() {
        None
    } else {
        Some(normalized_lines.join("\n"))
    }
}

fn strip_markdown_link_for_source(text: &str, source: &str) -> String {
    let escaped = regex::escape(source);
    let image_pattern = format!(r#"!\[[^\]]*]\(\s*{escaped}\s*(?:\"[^\"]*\")?\)"#);
    let link_pattern = format!(r#"\[[^\]]+]\(\s*{escaped}\s*(?:\"[^\"]*\")?\)"#);
    let mut rewritten = text.to_string();
    if let Ok(image_re) = Regex::new(&image_pattern) {
        rewritten = image_re.replace_all(&rewritten, "").into_owned();
    }
    if let Ok(link_re) = Regex::new(&link_pattern) {
        rewritten = link_re.replace_all(&rewritten, "").into_owned();
    }
    rewritten
}

pub fn extract_context_token_from_meta(meta: Option<&Value>) -> Option<String> {
    let meta = meta?;
    let direct = meta
        .get("weixin_context_token")
        .or_else(|| meta.get("context_token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if direct.is_some() {
        return direct;
    }
    meta.get("weixin")
        .and_then(|value| value.get("context_token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn extract_inbound_context_token(message: &ChannelMessage) -> Option<String> {
    extract_context_token_from_meta(message.meta.as_ref())
}

pub fn extract_media_entries_from_message_meta(
    meta: Option<&Value>,
) -> Vec<WeixinInboundMediaEntry> {
    let Some(meta) = meta else {
        return Vec::new();
    };
    let Some(raw_entries) = meta.get("weixin_media_entries") else {
        return Vec::new();
    };
    serde_json::from_value(raw_entries.clone()).unwrap_or_default()
}

pub fn parse_aes_key_base64(aes_key_base64: &str) -> Result<[u8; 16]> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(aes_key_base64.as_bytes())
        .map_err(|err| anyhow!("invalid base64 aes key: {err}"))?;
    if decoded.len() == 16 {
        let mut key = [0_u8; 16];
        key.copy_from_slice(&decoded);
        return Ok(key);
    }
    if decoded.len() == 32 {
        let text = std::str::from_utf8(&decoded)
            .map_err(|err| anyhow!("invalid aes key utf8 text: {err}"))?
            .trim();
        if text.len() == 32 && text.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return parse_hex_aes_key(text);
        }
    }
    Err(anyhow!(
        "aes key decode failed: expected 16 raw bytes or 32-byte hex text, got {} bytes",
        decoded.len()
    ))
}

pub fn parse_hex_aes_key(aes_key_hex: &str) -> Result<[u8; 16]> {
    let trimmed = aes_key_hex.trim();
    if trimmed.len() != 32 || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(anyhow!("invalid aes hex key"));
    }
    let raw = hex::decode(trimmed).map_err(|err| anyhow!("invalid aes hex key: {err}"))?;
    if raw.len() != 16 {
        return Err(anyhow!("invalid aes hex key length"));
    }
    let mut key = [0_u8; 16];
    key.copy_from_slice(&raw);
    Ok(key)
}

pub fn decrypt_inbound_media_bytes(
    ciphertext: &[u8],
    aes_key_base64: Option<&str>,
    aes_hex_key: Option<&str>,
) -> Result<Vec<u8>> {
    if let Some(hex_key) = trimmed_non_empty(aes_hex_key) {
        let key = parse_hex_aes_key(&hex_key)?;
        return decrypt_aes_128_ecb_pkcs7(ciphertext, &key);
    }
    if let Some(base64_key) = trimmed_non_empty(aes_key_base64) {
        let key = parse_aes_key_base64(&base64_key)?;
        return decrypt_aes_128_ecb_pkcs7(ciphertext, &key);
    }
    Ok(ciphertext.to_vec())
}

fn build_base_info() -> Value {
    json!({
        "channel_version": "wunder-rust",
    })
}

fn first_item_type(items: &[WeixinMessageItem]) -> Option<i64> {
    items
        .iter()
        .filter_map(|item| item.item_type)
        .find(|item_type| *item_type > 0)
}

fn placeholder_for_item_type(item_type: i64) -> String {
    match item_type {
        2 => "[image]".to_string(),
        3 => "[voice]".to_string(),
        4 => "[file]".to_string(),
        5 => "[video]".to_string(),
        _ => "[unsupported]".to_string(),
    }
}

fn extract_item_text(items: &[WeixinMessageItem]) -> Option<String> {
    for item in items {
        if item.item_type == Some(1) {
            if let Some(text) = item
                .text_item
                .as_ref()
                .and_then(|value| value.text.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Some(text.to_string());
            }
        }
        if item.item_type == Some(3) {
            if let Some(text) = item
                .voice_item
                .as_ref()
                .and_then(|value| value.text.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn extract_media_attachments(
    items: &[WeixinMessageItem],
    config: &WeixinConfig,
) -> (Vec<ChannelAttachment>, Vec<WeixinInboundMediaEntry>) {
    let mut attachments = Vec::new();
    let mut entries = Vec::new();

    for item in items {
        let Some(entry) = build_media_entry(item) else {
            continue;
        };
        let attachment = ChannelAttachment {
            kind: entry.kind.clone(),
            url: build_cdn_download_url(config, &entry.encrypt_query_param),
            mime: entry.mime_hint.clone(),
            size: None,
            name: entry.file_name.clone(),
        };
        attachments.push(attachment);
        entries.push(entry);
    }

    (attachments, entries)
}

fn build_media_entry(item: &WeixinMessageItem) -> Option<WeixinInboundMediaEntry> {
    match item.item_type {
        Some(2) => {
            let image = item.image_item.as_ref()?;
            let encrypt_query_param = image
                .media
                .as_ref()
                .and_then(|media| trimmed_non_empty(media.encrypt_query_param.as_deref()))
                .or_else(|| {
                    image
                        .thumb_media
                        .as_ref()
                        .and_then(|media| trimmed_non_empty(media.encrypt_query_param.as_deref()))
                })?;
            Some(WeixinInboundMediaEntry {
                kind: "image".to_string(),
                encrypt_query_param,
                aes_key: image
                    .media
                    .as_ref()
                    .and_then(|media| trimmed_non_empty(media.aes_key.as_deref()))
                    .or_else(|| {
                        image
                            .thumb_media
                            .as_ref()
                            .and_then(|media| trimmed_non_empty(media.aes_key.as_deref()))
                    }),
                aes_hex_key: trimmed_non_empty(image.aeskey.as_deref()),
                file_name: None,
                mime_hint: Some("image/*".to_string()),
            })
        }
        Some(3) => {
            let voice = item.voice_item.as_ref()?;
            let media = voice.media.as_ref()?;
            let encrypt_query_param = trimmed_non_empty(media.encrypt_query_param.as_deref())?;
            Some(WeixinInboundMediaEntry {
                kind: "audio".to_string(),
                encrypt_query_param,
                aes_key: trimmed_non_empty(media.aes_key.as_deref()),
                aes_hex_key: None,
                file_name: None,
                mime_hint: Some("audio/silk".to_string()),
            })
        }
        Some(4) => {
            let file = item.file_item.as_ref()?;
            let encrypt_query_param = file
                .media
                .as_ref()
                .and_then(|media| trimmed_non_empty(media.encrypt_query_param.as_deref()))
                .or_else(|| trimmed_non_empty(file.encrypt_query_param.as_deref()))?;
            let file_name = trimmed_non_empty(file.file_name.as_deref());
            let mime_hint = infer_mime_from_filename(file_name.as_deref());
            Some(WeixinInboundMediaEntry {
                kind: "file".to_string(),
                encrypt_query_param,
                aes_key: file
                    .media
                    .as_ref()
                    .and_then(|media| trimmed_non_empty(media.aes_key.as_deref()))
                    .or_else(|| trimmed_non_empty(file.aes_key.as_deref())),
                aes_hex_key: trimmed_non_empty(file.aeskey.as_deref()),
                file_name,
                mime_hint,
            })
        }
        Some(5) => {
            let video = item.video_item.as_ref()?;
            let encrypt_query_param = video
                .media
                .as_ref()
                .and_then(|media| trimmed_non_empty(media.encrypt_query_param.as_deref()))
                .or_else(|| trimmed_non_empty(video.encrypt_query_param.as_deref()))
                .or_else(|| {
                    video
                        .thumb_media
                        .as_ref()
                        .and_then(|media| trimmed_non_empty(media.encrypt_query_param.as_deref()))
                })?;
            Some(WeixinInboundMediaEntry {
                kind: "video".to_string(),
                encrypt_query_param,
                aes_key: video
                    .media
                    .as_ref()
                    .and_then(|media| trimmed_non_empty(media.aes_key.as_deref()))
                    .or_else(|| trimmed_non_empty(video.aes_key.as_deref()))
                    .or_else(|| {
                        video
                            .thumb_media
                            .as_ref()
                            .and_then(|media| trimmed_non_empty(media.aes_key.as_deref()))
                    }),
                aes_hex_key: trimmed_non_empty(video.aeskey.as_deref()),
                file_name: None,
                mime_hint: Some("video/mp4".to_string()),
            })
        }
        _ => None,
    }
}

fn fallback_message_id(
    message: &WeixinInboundMessage,
    account_id: &str,
    peer_id: &str,
) -> Option<String> {
    let ts = message.create_time_ms?;
    Some(format!(
        "wxilink:{account}:{peer}:{ts}",
        account = account_id.trim(),
        peer = peer_id.trim(),
        ts = ts
    ))
}

async fn send_text_message(
    http: &Client,
    config: &WeixinConfig,
    to_user_id: &str,
    context_token: &str,
    text: &str,
) -> Result<()> {
    let item = build_text_message_item(text);
    send_message_items(http, config, to_user_id, context_token, vec![item]).await
}

async fn build_attachment_message_item(
    http: &Client,
    config: &WeixinConfig,
    to_user_id: &str,
    attachment: &ChannelAttachment,
) -> Result<Value> {
    let loaded = load_attachment(http, attachment, resolve_api_timeout_ms(config)).await?;
    let uploaded = upload_media_to_cdn(http, config, to_user_id, &loaded).await?;
    Ok(build_outbound_media_item(&loaded, &uploaded))
}

fn build_text_message_item(text: &str) -> Value {
    json!({
        "type": 1,
        "text_item": {
            "text": text,
        }
    })
}

fn build_outbound_media_item(loaded: &LoadedAttachment, uploaded: &UploadedMediaRef) -> Value {
    let media_payload = json!({
        "encrypt_query_param": uploaded.download_encrypted_query_param,
        "aes_key": encode_weixin_media_aes_key(uploaded.aes_key_hex.as_str()),
        "encrypt_type": 1,
    });

    match loaded.media_kind {
        WeixinOutboundMediaKind::Image => json!({
            "type": 2,
            "image_item": {
                "media": media_payload,
                "mid_size": uploaded.cipher_size,
            }
        }),
        WeixinOutboundMediaKind::Voice => json!({
            "type": 3,
            "voice_item": {
                "media": media_payload,
            }
        }),
        WeixinOutboundMediaKind::File => json!({
            "type": 4,
            "file_item": {
                "media": media_payload,
                "file_name": loaded
                    .filename
                    .clone()
                    .unwrap_or_else(|| "file.bin".to_string()),
                "len": uploaded.raw_size.to_string(),
            }
        }),
        WeixinOutboundMediaKind::Video => json!({
            "type": 5,
            "video_item": {
                "media": media_payload,
                "video_size": uploaded.cipher_size,
            }
        }),
    }
}

async fn send_message_items(
    http: &Client,
    config: &WeixinConfig,
    to_user_id: &str,
    context_token: &str,
    item_list: Vec<Value>,
) -> Result<()> {
    // Align with openclaw behavior: send each item in an isolated request.
    // Mixed item_list payloads can be accepted by API but unstable in some Weixin clients.
    for item in item_list {
        let payload = post_json(
            http,
            config,
            "ilink/bot/sendmessage",
            json!({
                "msg": {
                    "from_user_id": "",
                    "to_user_id": to_user_id,
                    "client_id": format!("wunder-weixin-{}", Uuid::new_v4().simple()),
                    "message_type": 2,
                    "message_state": 2,
                    "item_list": [item],
                    "context_token": context_token,
                },
                "base_info": build_base_info(),
            }),
            resolve_api_timeout_ms(config),
        )
        .await?;

        ensure_weixin_ret_ok(&payload, "weixin outbound sendmessage")?;
    }
    Ok(())
}

fn render_attachment_fallback(text: Option<&str>, attachments: &[ChannelAttachment]) -> String {
    let mut lines = Vec::new();
    if let Some(text) = text.map(str::trim).filter(|value| !value.is_empty()) {
        lines.push(text.to_string());
    }
    for attachment in attachments {
        let kind = attachment.kind.trim();
        let url = attachment.url.trim();
        if url.is_empty() {
            continue;
        }
        lines.push(format!("[{kind}] {url}"));
    }
    if lines.is_empty() {
        "(empty message)".to_string()
    } else {
        lines.join("\n")
    }
}

fn render_missing_attachment_notice(attachments: &[ChannelAttachment]) -> Option<String> {
    if attachments.is_empty() {
        return None;
    }
    let names = attachments
        .iter()
        .filter_map(|attachment| {
            attachment
                .name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Some("(attachment unavailable)".to_string());
    }
    Some(format!("(attachment unavailable: {})", names.join(", ")))
}

fn should_skip_attachment_delivery_error(err: &anyhow::Error) -> bool {
    let error_text = err.to_string().to_ascii_lowercase();
    error_text.contains("weixin attachment file not found")
        || error_text.contains("weixin attachment path is not a file")
}

async fn upload_media_to_cdn(
    http: &Client,
    config: &WeixinConfig,
    to_user_id: &str,
    loaded: &LoadedAttachment,
) -> Result<UploadedMediaRef> {
    let raw_size = loaded.bytes.len();
    if raw_size > MAX_MEDIA_BYTES {
        return Err(anyhow!(
            "weixin attachment exceeds max bytes: {raw_size} > {MAX_MEDIA_BYTES}"
        ));
    }

    let filekey = Uuid::new_v4().simple().to_string();
    let mut aes_key = [0_u8; 16];
    aes_key.copy_from_slice(&Uuid::new_v4().as_bytes()[..16]);
    let aes_key_hex = hex::encode(aes_key);

    let ciphertext = encrypt_aes_128_ecb_pkcs7(&loaded.bytes, &aes_key)?;
    let cipher_size = ciphertext.len();
    let rawfilemd5 = format!("{:x}", md5::compute(&loaded.bytes));

    let upload_payload = post_json(
        http,
        config,
        "ilink/bot/getuploadurl",
        json!({
            "filekey": filekey,
            "media_type": loaded.media_kind.upload_media_type(),
            "to_user_id": to_user_id,
            "rawsize": raw_size,
            "rawfilemd5": rawfilemd5,
            "filesize": cipher_size,
            "no_need_thumb": true,
            "aeskey": aes_key_hex,
            "base_info": build_base_info(),
        }),
        resolve_api_timeout_ms(config),
    )
    .await?;

    ensure_weixin_ret_ok(&upload_payload, "weixin getuploadurl")?;

    let upload_url = resolve_upload_url(config, &upload_payload, &filekey).ok_or_else(|| {
        anyhow!(
            "weixin getuploadurl missing upload target: {}",
            truncate_text(&upload_payload.to_string(), 512)
        )
    })?;

    let download_encrypted_query_param =
        upload_ciphertext_to_cdn(http, config, &upload_url, &ciphertext).await?;

    Ok(UploadedMediaRef {
        download_encrypted_query_param,
        aes_key_hex: hex::encode(aes_key),
        raw_size,
        cipher_size,
    })
}

fn encode_weixin_media_aes_key(aes_key_hex: &str) -> String {
    // Keep compatibility with openclaw-weixin-cli and existing client behavior:
    // encode hex-text bytes as base64 (instead of raw 16-byte key).
    base64::engine::general_purpose::STANDARD.encode(aes_key_hex.as_bytes())
}

async fn upload_ciphertext_to_cdn(
    http: &Client,
    config: &WeixinConfig,
    upload_url: &str,
    ciphertext: &[u8],
) -> Result<String> {
    let url = normalize_upload_url(config, upload_url)?;

    let response = http
        .post(&url)
        .header(CONTENT_TYPE, "application/octet-stream")
        .timeout(Duration::from_millis(resolve_api_timeout_ms(config)))
        .body(ciphertext.to_vec())
        .send()
        .await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await.unwrap_or_default();

    if status != reqwest::StatusCode::OK {
        return Err(anyhow!(
            "weixin cdn upload failed: status={status}, body={}",
            truncate_text(&body, 256)
        ));
    }

    headers
        .get("x-encrypted-param")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("weixin cdn upload missing x-encrypted-param"))
}

async fn load_attachment(
    http: &Client,
    attachment: &ChannelAttachment,
    timeout_ms: u64,
) -> Result<LoadedAttachment> {
    let source = attachment.url.trim();
    if source.is_empty() {
        return Err(anyhow!("weixin attachment url is empty"));
    }

    let (bytes, _discovered_mime, discovered_name) =
        if source.starts_with("http://") || source.starts_with("https://") {
            download_remote_attachment(http, source, timeout_ms).await?
        } else if source.starts_with("data:") {
            parse_data_url_attachment(source)?
        } else {
            read_local_attachment(source).await?
        };

    if bytes.is_empty() {
        return Err(anyhow!("weixin attachment bytes is empty"));
    }
    if bytes.len() > MAX_MEDIA_BYTES {
        return Err(anyhow!(
            "weixin attachment exceeds max bytes: {} > {}",
            bytes.len(),
            MAX_MEDIA_BYTES
        ));
    }

    let filename = attachment
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or(discovered_name)
        .map(|name| sanitize_filename(&name));
    let media_kind = classify_outbound_media_kind(
        attachment.kind.as_str(),
        attachment.mime.as_deref(),
        filename.as_deref(),
        source,
    );

    Ok(LoadedAttachment {
        bytes,
        filename,
        media_kind,
    })
}

async fn download_remote_attachment(
    http: &Client,
    source_url: &str,
    timeout_ms: u64,
) -> Result<(Vec<u8>, Option<String>, Option<String>)> {
    let response = http
        .get(source_url)
        .timeout(Duration::from_millis(timeout_ms.max(1_000)))
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(
            "weixin attachment download failed: status={status}"
        ));
    }
    if response.content_length().unwrap_or(0) > MAX_MEDIA_BYTES as u64 {
        return Err(anyhow!("weixin attachment exceeds max bytes"));
    }

    let headers = response.headers().clone();
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let filename = headers
        .get("content-disposition")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition_filename)
        .or_else(|| filename_from_url(source_url));
    let bytes = response.bytes().await?.to_vec();

    if bytes.len() > MAX_MEDIA_BYTES {
        return Err(anyhow!("weixin attachment exceeds max bytes"));
    }

    Ok((bytes, content_type, filename))
}

async fn read_local_attachment(source: &str) -> Result<(Vec<u8>, Option<String>, Option<String>)> {
    let path = resolve_local_path(source)?;
    let metadata = fs::metadata(&path)
        .await
        .with_context(|| format!("weixin attachment file not found: {}", path.display()))?;
    if !metadata.is_file() {
        return Err(anyhow!(
            "weixin attachment path is not a file: {}",
            path.display()
        ));
    }
    if metadata.len() > MAX_MEDIA_BYTES as u64 {
        return Err(anyhow!("weixin attachment exceeds max bytes"));
    }

    let bytes = fs::read(&path)
        .await
        .with_context(|| format!("read weixin attachment failed: {}", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string);
    let mime = infer_mime_from_filename(filename.as_deref());
    Ok((bytes, mime, filename))
}

fn parse_data_url_attachment(source: &str) -> Result<(Vec<u8>, Option<String>, Option<String>)> {
    let rest = source
        .strip_prefix("data:")
        .ok_or_else(|| anyhow!("invalid data url"))?;
    let (meta, data_part) = rest
        .split_once(',')
        .ok_or_else(|| anyhow!("invalid data url"))?;

    let mime = meta
        .split(';')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let is_base64 = meta
        .split(';')
        .any(|segment| segment.trim().eq_ignore_ascii_case("base64"));

    let bytes = if is_base64 {
        base64::engine::general_purpose::STANDARD
            .decode(data_part.as_bytes())
            .map_err(|err| anyhow!("invalid data url base64 payload: {err}"))?
    } else {
        percent_decode_to_bytes(data_part)
    };

    Ok((bytes, mime, None))
}

fn classify_outbound_media_kind(
    kind: &str,
    mime: Option<&str>,
    filename: Option<&str>,
    source: &str,
) -> WeixinOutboundMediaKind {
    let normalized_kind = kind.trim().to_ascii_lowercase();
    if matches!(normalized_kind.as_str(), "image" | "photo" | "sticker") {
        return WeixinOutboundMediaKind::Image;
    }
    if matches!(normalized_kind.as_str(), "video" | "movie") {
        return WeixinOutboundMediaKind::Video;
    }
    if matches!(normalized_kind.as_str(), "audio" | "voice") {
        return WeixinOutboundMediaKind::Voice;
    }

    if let Some(mime) = mime.map(|value| value.trim().to_ascii_lowercase()) {
        if mime.starts_with("image/") {
            return WeixinOutboundMediaKind::Image;
        }
        if mime.starts_with("video/") {
            return WeixinOutboundMediaKind::Video;
        }
        if mime.starts_with("audio/") {
            return WeixinOutboundMediaKind::Voice;
        }
    }

    let extension = filename
        .map(Path::new)
        .and_then(|value| value.extension())
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .or_else(|| extension_from_url(source));

    if let Some(ext) = extension.as_deref() {
        if matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg") {
            return WeixinOutboundMediaKind::Image;
        }
        if matches!(ext, "mp4" | "mov" | "mkv" | "avi" | "webm") {
            return WeixinOutboundMediaKind::Video;
        }
        if matches!(ext, "mp3" | "wav" | "ogg" | "m4a" | "aac" | "silk" | "amr") {
            return WeixinOutboundMediaKind::Voice;
        }
    }

    WeixinOutboundMediaKind::File
}

fn resolve_local_path(source: &str) -> Result<PathBuf> {
    if source.starts_with("file://") {
        let parsed = Url::parse(source).map_err(|err| anyhow!("invalid file url: {err}"))?;
        let path = parsed
            .to_file_path()
            .map_err(|_| anyhow!("invalid file url path"))?;
        return Ok(path);
    }
    Ok(PathBuf::from(source))
}

fn parse_content_disposition_filename(value: &str) -> Option<String> {
    let mut filename = None;
    for raw in value.split(';') {
        let part = raw.trim();
        if let Some(rest) = part.strip_prefix("filename*=") {
            let cleaned = rest.trim_matches('"');
            if let Some(encoded) = cleaned.split("''").nth(1) {
                filename = Some(percent_decode_to_string(encoded));
                break;
            }
            filename = Some(percent_decode_to_string(cleaned));
            break;
        }
        if let Some(rest) = part.strip_prefix("filename=") {
            filename = Some(rest.trim_matches('"').to_string());
            break;
        }
    }
    filename
}

fn filename_from_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let segment = parsed
        .path_segments()
        .and_then(|mut items| items.next_back())?
        .trim();
    if segment.is_empty() {
        return None;
    }
    Some(percent_decode_to_string(segment))
}

fn extension_from_url(url: &str) -> Option<String> {
    filename_from_url(url)
        .and_then(|name| {
            Path::new(&name)
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        })
        .map(|value| value.to_ascii_lowercase())
}

fn infer_mime_from_filename(filename: Option<&str>) -> Option<String> {
    let ext = filename
        .map(Path::new)
        .and_then(|value| value.extension())
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())?;

    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "silk" => "audio/silk",
        "amr" => "audio/amr",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "txt" => "text/plain",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => return None,
    };
    Some(mime.to_string())
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
}

fn ensure_weixin_ret_ok(payload: &Value, action: &str) -> Result<()> {
    let ret = payload.get("ret").and_then(Value::as_i64).unwrap_or(0);
    let errcode = payload.get("errcode").and_then(Value::as_i64).unwrap_or(0);
    if ret == 0 && errcode == 0 {
        return Ok(());
    }
    let errmsg = payload
        .get("errmsg")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    Err(anyhow!(
        "{action} failed: ret={ret}, errcode={errcode}, errmsg={errmsg}"
    ))
}

fn encrypt_aes_128_ecb_pkcs7(plaintext: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    let cipher = Aes128::new_from_slice(key).map_err(|err| anyhow!("invalid aes key: {err}"))?;
    let mut output = pkcs7_pad(plaintext, AES_BLOCK_SIZE);
    for chunk in output.chunks_exact_mut(AES_BLOCK_SIZE) {
        let block = Block::<Aes128>::from_mut_slice(chunk);
        cipher.encrypt_block(block);
    }
    Ok(output)
}

fn decrypt_aes_128_ecb_pkcs7(ciphertext: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(AES_BLOCK_SIZE) {
        return Err(anyhow!("invalid aes ciphertext size"));
    }
    let cipher = Aes128::new_from_slice(key).map_err(|err| anyhow!("invalid aes key: {err}"))?;
    let mut output = ciphertext.to_vec();
    for chunk in output.chunks_exact_mut(AES_BLOCK_SIZE) {
        let block = Block::<Aes128>::from_mut_slice(chunk);
        cipher.decrypt_block(block);
    }
    pkcs7_unpad(&output, AES_BLOCK_SIZE)
}

fn pkcs7_pad(input: &[u8], block_size: usize) -> Vec<u8> {
    let pad_len = block_size - (input.len() % block_size);
    let mut output = Vec::with_capacity(input.len() + pad_len);
    output.extend_from_slice(input);
    output.extend(std::iter::repeat_n(pad_len as u8, pad_len));
    output
}

fn pkcs7_unpad(input: &[u8], block_size: usize) -> Result<Vec<u8>> {
    if input.is_empty() {
        return Err(anyhow!("invalid pkcs7 payload"));
    }
    let pad_len = *input.last().unwrap_or(&0) as usize;
    if pad_len == 0 || pad_len > block_size || pad_len > input.len() {
        return Err(anyhow!("invalid pkcs7 padding length"));
    }
    if !input[input.len() - pad_len..]
        .iter()
        .all(|value| *value as usize == pad_len)
    {
        return Err(anyhow!("invalid pkcs7 padding bytes"));
    }
    Ok(input[..input.len() - pad_len].to_vec())
}

async fn post_json(
    http: &Client,
    config: &WeixinConfig,
    endpoint: &str,
    body: Value,
    timeout_ms: u64,
) -> Result<Value> {
    let endpoint = endpoint.trim().to_string();
    let body_text = serde_json::to_string(&body)
        .map_err(|err| anyhow!("weixin request encode failed: {err}"))?;
    let url = resolve_endpoint_url(config, &endpoint)
        .ok_or_else(|| anyhow!("weixin api_base missing"))?;
    let headers = build_request_headers(config, &body_text)?;
    let response = http
        .post(&url)
        .headers(headers)
        .timeout(Duration::from_millis(timeout_ms.max(1_000)))
        .body(body_text)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "weixin api request failed: endpoint={endpoint}, status={status}, body={}",
            truncate_text(&body, 512)
        ));
    }
    if body.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str::<Value>(&body).map_err(|err| {
        anyhow!(
            "weixin api response decode failed: endpoint={endpoint}, error={err}, body={}",
            truncate_text(&body, 512)
        )
    })
}

fn resolve_endpoint_url(config: &WeixinConfig, endpoint: &str) -> Option<String> {
    let base = resolve_api_base_url(config)?;
    let endpoint = endpoint.trim().trim_start_matches('/');
    if endpoint.is_empty() {
        return Some(base);
    }
    Some(format!("{base}/{endpoint}"))
}

fn resolve_api_base_url(config: &WeixinConfig) -> Option<String> {
    normalize_api_base(config.api_base.as_deref())
}

fn normalize_base_url(raw: Option<&str>, default_value: &str) -> Option<String> {
    let value = raw.map(str::trim).filter(|value| !value.is_empty());
    let raw = value.unwrap_or(default_value).trim();
    if raw.is_empty() {
        return None;
    }
    let normalized = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    Some(normalized.trim_end_matches('/').to_string())
}

fn build_request_headers(config: &WeixinConfig, body: &str) -> Result<HeaderMap> {
    let token = config
        .bot_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("weixin bot_token missing"))?;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&body.len().to_string())
            .map_err(|err| anyhow!("invalid content-length header: {err}"))?,
    );
    headers.insert(
        HeaderName::from_static("authorizationtype"),
        HeaderValue::from_static("ilink_bot_token"),
    );
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|err| anyhow!("invalid authorization header: {err}"))?,
    );
    headers.insert(
        HeaderName::from_static("x-wechat-uin"),
        HeaderValue::from_str(&random_wechat_uin())
            .map_err(|err| anyhow!("invalid x-wechat-uin header: {err}"))?,
    );

    if let Some(route_tag) = trimmed_non_empty(config.route_tag.as_deref()) {
        headers.insert(
            HeaderName::from_static("skroutetag"),
            HeaderValue::from_str(&route_tag)
                .map_err(|err| anyhow!("invalid SKRouteTag header: {err}"))?,
        );
    }

    Ok(headers)
}

fn random_wechat_uin() -> String {
    let bytes = Uuid::new_v4();
    let raw = bytes.as_bytes();
    let value = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
    base64::engine::general_purpose::STANDARD.encode(value.to_string().as_bytes())
}

fn trimmed_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

fn encode_query_value(value: &str) -> String {
    byte_serialize(value.as_bytes()).collect()
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut iter = value.chars();
    let truncated: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn value_to_trimmed_string(value: &Value) -> Option<String> {
    match value {
        Value::String(item) => trimmed_non_empty(Some(item)),
        Value::Number(item) => Some(item.to_string()),
        Value::Bool(item) => Some(item.to_string()),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value)
            .ok()
            .and_then(|serialized| trimmed_non_empty(Some(serialized.as_str()))),
        _ => None,
    }
}

fn extract_item_message_id(items: &[WeixinMessageItem]) -> Option<String> {
    items
        .iter()
        .filter_map(|item| item.msg_id.as_ref().and_then(value_to_trimmed_string))
        .find(|value| !value.is_empty())
}

fn extract_item_create_time_ms(items: &[WeixinMessageItem]) -> Option<i64> {
    items
        .iter()
        .filter_map(|item| item.create_time_ms)
        .find(|value| *value > 0)
}

fn extract_upload_param(payload: &Value) -> Option<String> {
    const CANDIDATE_PATHS: &[&[&str]] = &[
        &["upload_param"],
        &["uploadParam"],
        &["data", "upload_param"],
        &["data", "uploadParam"],
        &["result", "upload_param"],
        &["result", "uploadParam"],
        &["data", "result", "upload_param"],
        &["data", "result", "uploadParam"],
    ];
    extract_string_by_paths(payload, CANDIDATE_PATHS)
}

fn extract_upload_full_url(payload: &Value) -> Option<String> {
    const CANDIDATE_PATHS: &[&[&str]] = &[
        &["upload_full_url"],
        &["uploadFullUrl"],
        &["data", "upload_full_url"],
        &["data", "uploadFullUrl"],
        &["result", "upload_full_url"],
        &["result", "uploadFullUrl"],
        &["data", "result", "upload_full_url"],
        &["data", "result", "uploadFullUrl"],
    ];
    extract_string_by_paths(payload, CANDIDATE_PATHS)
}

fn resolve_upload_url(config: &WeixinConfig, payload: &Value, filekey: &str) -> Option<String> {
    if let Some(upload_full_url) = extract_upload_full_url(payload) {
        return Some(upload_full_url);
    }
    let upload_param = extract_upload_param(payload)?;
    Some(build_upload_url_from_param(config, &upload_param, filekey))
}

fn build_upload_url_from_param(config: &WeixinConfig, upload_param: &str, filekey: &str) -> String {
    let cdn_base = resolve_cdn_base_url(config);
    let encoded_upload_param = encode_query_value(upload_param);
    let encoded_filekey = encode_query_value(filekey);
    format!(
        "{cdn_base}/upload?encrypted_query_param={encoded_upload_param}&filekey={encoded_filekey}"
    )
}

fn normalize_upload_url(config: &WeixinConfig, upload_url: &str) -> Result<String> {
    let trimmed = upload_url.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("weixin upload url is empty"));
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(trimmed.to_string());
    }
    let cdn_base = resolve_cdn_base_url(config);
    if trimmed.starts_with('/') {
        return Ok(format!("{cdn_base}{trimmed}"));
    }
    let normalized_path = trimmed.trim_start_matches('/');
    Ok(format!("{cdn_base}/{normalized_path}"))
}

fn extract_string_by_paths(payload: &Value, paths: &[&[&str]]) -> Option<String> {
    for path in paths {
        let mut current = payload;
        let mut found = true;
        for segment in *path {
            let Some(next) = current.get(*segment) else {
                found = false;
                break;
            };
            current = next;
        }
        if found {
            if let Some(value) = value_to_trimmed_string(current) {
                return Some(value);
            }
        }
    }
    None
}

fn percent_decode_to_bytes(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex_text) = std::str::from_utf8(&bytes[index + 1..index + 3]) {
                if let Ok(raw) = u8::from_str_radix(hex_text, 16) {
                    output.push(raw);
                    index += 3;
                    continue;
                }
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    output
}

fn percent_decode_to_string(value: &str) -> String {
    String::from_utf8_lossy(&percent_decode_to_bytes(value)).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Bytes,
        extract::State,
        http::{HeaderName, HeaderValue, StatusCode},
        response::IntoResponse,
        routing::post,
        Json, Router,
    };
    use serde_json::Value;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::{net::TcpListener, sync::Mutex};

    #[derive(Clone)]
    struct OutboundServerState {
        upload_full_url: String,
        getuploadurl_payloads: Arc<Mutex<Vec<Value>>>,
        upload_bodies: Arc<Mutex<Vec<Vec<u8>>>>,
        sendmessage_payloads: Arc<Mutex<Vec<Value>>>,
    }

    struct OutboundServerFixture {
        base_url: String,
        state: OutboundServerState,
        _handle: tokio::task::JoinHandle<()>,
    }

    async fn start_outbound_server_fixture() -> OutboundServerFixture {
        async fn getuploadurl_handler(
            State(state): State<OutboundServerState>,
            Json(payload): Json<Value>,
        ) -> Json<Value> {
            state.getuploadurl_payloads.lock().await.push(payload);
            Json(json!({
                "ret": 0,
                "upload_full_url": state.upload_full_url,
            }))
        }

        async fn upload_direct_handler(
            State(state): State<OutboundServerState>,
            body: Bytes,
        ) -> impl IntoResponse {
            state.upload_bodies.lock().await.push(body.to_vec());
            (
                StatusCode::OK,
                [(
                    HeaderName::from_static("x-encrypted-param"),
                    HeaderValue::from_static("enc-uploaded-1"),
                )],
            )
        }

        async fn sendmessage_handler(
            State(state): State<OutboundServerState>,
            Json(payload): Json<Value>,
        ) -> Json<Value> {
            state.sendmessage_payloads.lock().await.push(payload);
            Json(json!({ "ret": 0 }))
        }

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind outbound fixture listener");
        let addr = listener.local_addr().expect("fixture local addr");
        let base_url = format!("http://{addr}");
        let state = OutboundServerState {
            upload_full_url: format!("{base_url}/upload/direct?token=abc"),
            getuploadurl_payloads: Arc::new(Mutex::new(Vec::new())),
            upload_bodies: Arc::new(Mutex::new(Vec::new())),
            sendmessage_payloads: Arc::new(Mutex::new(Vec::new())),
        };
        let app = Router::new()
            .route("/ilink/bot/getuploadurl", post(getuploadurl_handler))
            .route("/upload/direct", post(upload_direct_handler))
            .route("/ilink/bot/sendmessage", post(sendmessage_handler))
            .with_state(state.clone());
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve outbound fixture app");
        });

        OutboundServerFixture {
            base_url,
            state,
            _handle: handle,
        }
    }

    fn write_outbound_fixture_file(file_name: &str, bytes: &[u8]) -> (TempDir, String) {
        let temp_dir = tempfile::tempdir().expect("create outbound temp dir");
        let file_path = temp_dir.path().join(file_name);
        std::fs::write(&file_path, bytes).expect("write outbound fixture file");
        (temp_dir, file_path.to_string_lossy().to_string())
    }

    #[test]
    fn extract_context_token_from_meta_supports_multiple_shapes() {
        let direct = json!({ "weixin_context_token": "ctx-1" });
        assert_eq!(
            extract_context_token_from_meta(Some(&direct)).as_deref(),
            Some("ctx-1")
        );

        let nested = json!({ "weixin": { "context_token": "ctx-2" } });
        assert_eq!(
            extract_context_token_from_meta(Some(&nested)).as_deref(),
            Some("ctx-2")
        );
    }

    #[test]
    fn extract_inbound_messages_parses_text_items() {
        let config = WeixinConfig::default();
        let messages = extract_inbound_messages(
            &[WeixinInboundMessage {
                from_user_id: Some("u_1".to_string()),
                create_time_ms: Some(1_710_000_000_000),
                context_token: Some("ctx-1".to_string()),
                item_list: vec![WeixinMessageItem {
                    item_type: Some(1),
                    text_item: Some(WeixinTextItem {
                        text: Some("hello".to_string()),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            "acc_1",
            &config,
        );
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text.as_deref(), Some("hello"));
        assert_eq!(
            extract_context_token_from_meta(messages[0].meta.as_ref()).as_deref(),
            Some("ctx-1")
        );
    }

    #[test]
    fn extract_inbound_messages_parses_media_entries() {
        let config = WeixinConfig::default();
        let messages = extract_inbound_messages(
            &[WeixinInboundMessage {
                from_user_id: Some("u_2".to_string()),
                create_time_ms: Some(1_710_000_000_001),
                item_list: vec![WeixinMessageItem {
                    item_type: Some(4),
                    file_item: Some(WeixinFileItem {
                        media: Some(WeixinCdnMedia {
                            encrypt_query_param: Some("enc-1".to_string()),
                            aes_key: Some(
                                base64::engine::general_purpose::STANDARD.encode([1_u8; 16]),
                            ),
                            ..Default::default()
                        }),
                        file_name: Some("report.pdf".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            "acc_2",
            &config,
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text.as_deref(), Some("[file]"));
        assert_eq!(messages[0].attachments.len(), 1);
        assert_eq!(messages[0].attachments[0].kind, "file");

        let entries = extract_media_entries_from_message_meta(messages[0].meta.as_ref());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].encrypt_query_param, "enc-1");
    }

    #[test]
    fn extract_inbound_messages_supports_camel_case_media_shapes() {
        let config = WeixinConfig::default();
        let messages = extract_inbound_messages(
            &[WeixinInboundMessage {
                from_user_id: Some("u_camel".to_string()),
                create_time_ms: None,
                item_list: vec![WeixinMessageItem {
                    item_type: Some(4),
                    msg_id: Some(json!("msg-item-1")),
                    create_time_ms: Some(1_710_000_000_123),
                    file_item: Some(WeixinFileItem {
                        media: None,
                        file_name: Some("notes.txt".to_string()),
                        encrypt_query_param: Some("enc-camel".to_string()),
                        aes_key: Some("file-key".to_string()),
                        aeskey: Some("00112233445566778899aabbccddeeff".to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            "acc_camel",
            &config,
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message_id.as_deref(), Some("msg-item-1"));
        assert_eq!(messages[0].attachments.len(), 1);
        assert_eq!(
            messages[0].attachments[0].name.as_deref(),
            Some("notes.txt")
        );
        let entries = extract_media_entries_from_message_meta(messages[0].meta.as_ref());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].encrypt_query_param, "enc-camel");
        assert_eq!(
            entries[0].aes_hex_key.as_deref(),
            Some("00112233445566778899aabbccddeeff")
        );
    }

    #[test]
    fn parse_aes_key_base64_supports_raw_and_hex_payload() {
        let expected = [7_u8; 16];
        let raw_b64 = base64::engine::general_purpose::STANDARD.encode(expected);
        let parsed_raw = parse_aes_key_base64(&raw_b64).expect("raw key parse");
        assert_eq!(parsed_raw, expected);

        let hex_text = hex::encode(expected);
        let hex_b64 = base64::engine::general_purpose::STANDARD.encode(hex_text.as_bytes());
        let parsed_hex = parse_aes_key_base64(&hex_b64).expect("hex key parse");
        assert_eq!(parsed_hex, expected);
    }

    #[test]
    fn aes_ecb_encrypt_decrypt_roundtrip() {
        let key = [3_u8; 16];
        let plain = b"hello weixin";
        let encrypted = encrypt_aes_128_ecb_pkcs7(plain, &key).expect("encrypt ok");
        assert_ne!(encrypted, plain);

        let decrypted = decrypt_aes_128_ecb_pkcs7(&encrypted, &key).expect("decrypt ok");
        assert_eq!(decrypted, plain);
    }

    #[test]
    fn normalize_helpers_use_defaults_and_trim() {
        assert_eq!(normalize_api_base(None).as_deref(), Some(DEFAULT_API_BASE));
        assert_eq!(
            normalize_api_base(Some("ilink.weixin.qq.com/")).as_deref(),
            Some("https://ilink.weixin.qq.com")
        );
        assert_eq!(
            normalize_cdn_base(Some(" https://cdn.example.com/c2c/ ")).as_deref(),
            Some("https://cdn.example.com/c2c")
        );
        assert_eq!(normalize_bot_type(None), DEFAULT_QR_BOT_TYPE);
        assert_eq!(normalize_bot_type(Some("0")), DEFAULT_QR_BOT_TYPE);
        assert_eq!(normalize_bot_type(Some(" 2 ")), "2");
    }

    #[test]
    fn build_cdn_download_url_encodes_query_param() {
        let config = WeixinConfig {
            cdn_base: Some("https://cdn.example.com/c2c/".to_string()),
            ..Default::default()
        };

        let url = build_cdn_download_url(&config, "A+B /?=");
        let parsed = Url::parse(&url).expect("url parse");
        assert_eq!(
            parsed
                .as_str()
                .starts_with("https://cdn.example.com/c2c/download?"),
            true
        );
        let encrypted_query_param = parsed
            .query_pairs()
            .find(|(key, _)| key == "encrypted_query_param")
            .map(|(_, value)| value.to_string())
            .unwrap_or_default();
        assert_eq!(encrypted_query_param, "A+B /?=");
    }

    #[test]
    fn extract_upload_param_supports_nested_shapes() {
        assert_eq!(
            extract_upload_param(&json!({ "upload_param": "direct-upload" })).as_deref(),
            Some("direct-upload")
        );
        assert_eq!(
            extract_upload_param(&json!({ "data": { "uploadParam": "nested-upload" } })).as_deref(),
            Some("nested-upload")
        );
        assert_eq!(
            extract_upload_param(
                &json!({ "data": { "result": { "upload_param": "deep-upload" } } })
            )
            .as_deref(),
            Some("deep-upload")
        );
    }

    #[test]
    fn resolve_upload_url_prefers_upload_full_url_when_available() {
        let config = WeixinConfig {
            cdn_base: Some("https://cdn.example.com/c2c".to_string()),
            ..Default::default()
        };
        assert_eq!(
            resolve_upload_url(
                &config,
                &json!({
                    "upload_full_url": "https://upload.example.com/direct?token=abc"
                }),
                "filekey-1"
            )
            .as_deref(),
            Some("https://upload.example.com/direct?token=abc")
        );
        assert_eq!(
            resolve_upload_url(
                &config,
                &json!({
                    "data": {
                        "uploadParam": "legacy-upload"
                    }
                }),
                "filekey-2"
            )
            .as_deref(),
            Some(
                "https://cdn.example.com/c2c/upload?encrypted_query_param=legacy-upload&filekey=filekey-2"
            )
        );
    }

    #[tokio::test]
    async fn send_outbound_uploads_media_via_upload_full_url_and_sends_file_reference() {
        let fixture = start_outbound_server_fixture().await;
        let raw_bytes = b"quarterly report bytes".to_vec();
        let (_temp_dir, file_path) = write_outbound_fixture_file("report.pdf", &raw_bytes);
        let outbound = ChannelOutboundMessage {
            channel: WEIXIN_CHANNEL.to_string(),
            account_id: "acc_weixin".to_string(),
            peer: ChannelPeer {
                kind: "user".to_string(),
                id: "wx_peer_1".to_string(),
                name: None,
            },
            thread: None,
            text: Some("Please review the attached report.".to_string()),
            attachments: vec![ChannelAttachment {
                kind: "file".to_string(),
                url: file_path,
                mime: Some("application/pdf".to_string()),
                size: Some(raw_bytes.len() as i64),
                name: Some("report.pdf".to_string()),
            }],
            meta: Some(json!({
                "context_token": "ctx-upload-1"
            })),
        };
        let config = WeixinConfig {
            api_base: Some(fixture.base_url.clone()),
            bot_token: Some("bot-token-1".to_string()),
            media_enabled: Some(true),
            ..Default::default()
        };

        send_outbound(&Client::new(), &outbound, &config)
            .await
            .expect("send outbound with upload_full_url");

        let getuploadurl_payloads = fixture.state.getuploadurl_payloads.lock().await.clone();
        assert_eq!(getuploadurl_payloads.len(), 1);
        let getuploadurl_payload = &getuploadurl_payloads[0];
        assert_eq!(
            getuploadurl_payload
                .get("to_user_id")
                .and_then(Value::as_str),
            Some("wx_peer_1")
        );
        assert_eq!(
            getuploadurl_payload.get("rawsize").and_then(Value::as_u64),
            Some(raw_bytes.len() as u64)
        );

        let upload_bodies = fixture.state.upload_bodies.lock().await.clone();
        assert_eq!(upload_bodies.len(), 1);
        assert!(!upload_bodies[0].is_empty());
        assert_ne!(upload_bodies[0], raw_bytes);

        let sendmessage_payloads = fixture.state.sendmessage_payloads.lock().await.clone();
        assert_eq!(sendmessage_payloads.len(), 2);

        let text_payload = sendmessage_payloads
            .iter()
            .find(|payload| {
                payload["msg"]["item_list"][0]["type"].as_i64() == Some(1)
                    && payload["msg"]["item_list"][0]["text_item"]["text"].as_str()
                        == Some("Please review the attached report.")
            })
            .expect("text payload should be sent");
        assert_eq!(
            text_payload["msg"]["context_token"].as_str(),
            Some("ctx-upload-1")
        );

        let file_payload = sendmessage_payloads
            .iter()
            .find(|payload| payload["msg"]["item_list"][0]["type"].as_i64() == Some(4))
            .expect("file payload should be sent");
        assert_eq!(
            file_payload["msg"]["item_list"][0]["file_item"]["file_name"].as_str(),
            Some("report.pdf")
        );
        assert_eq!(
            file_payload["msg"]["item_list"][0]["file_item"]["media"]["encrypt_query_param"]
                .as_str(),
            Some("enc-uploaded-1")
        );
    }

    #[test]
    fn extract_inbound_messages_parses_image_voice_video_entries() {
        let config = WeixinConfig::default();
        let messages = extract_inbound_messages(
            &[WeixinInboundMessage {
                from_user_id: Some("u_media".to_string()),
                create_time_ms: Some(1_710_000_000_100),
                item_list: vec![
                    WeixinMessageItem {
                        item_type: Some(2),
                        image_item: Some(WeixinImageItem {
                            media: Some(WeixinCdnMedia {
                                encrypt_query_param: Some("enc-image".to_string()),
                                aes_key: Some("image-key".to_string()),
                                ..Default::default()
                            }),
                            aeskey: Some("00112233445566778899aabbccddeeff".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    WeixinMessageItem {
                        item_type: Some(3),
                        voice_item: Some(WeixinVoiceItem {
                            media: Some(WeixinCdnMedia {
                                encrypt_query_param: Some("enc-voice".to_string()),
                                aes_key: Some("voice-key".to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    WeixinMessageItem {
                        item_type: Some(5),
                        video_item: Some(WeixinVideoItem {
                            media: Some(WeixinCdnMedia {
                                encrypt_query_param: Some("enc-video".to_string()),
                                aes_key: Some("video-key".to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            "acc_media",
            &config,
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text.as_deref(), Some("[image]"));
        assert_eq!(messages[0].attachments.len(), 3);
        assert_eq!(messages[0].attachments[0].kind, "image");
        assert_eq!(messages[0].attachments[1].kind, "audio");
        assert_eq!(messages[0].attachments[2].kind, "video");

        let entries = extract_media_entries_from_message_meta(messages[0].meta.as_ref());
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, "image");
        assert_eq!(
            entries[0].aes_hex_key.as_deref(),
            Some("00112233445566778899aabbccddeeff")
        );
        assert_eq!(entries[1].kind, "audio");
        assert_eq!(entries[1].mime_hint.as_deref(), Some("audio/silk"));
        assert_eq!(entries[2].kind, "video");
        assert_eq!(entries[2].mime_hint.as_deref(), Some("video/mp4"));
    }

    #[test]
    fn extract_inbound_messages_respects_allow_from_filter() {
        let config = WeixinConfig {
            allow_from: vec!["u_allowed".to_string()],
            ..Default::default()
        };
        let source = vec![
            WeixinInboundMessage {
                from_user_id: Some("u_allowed".to_string()),
                create_time_ms: Some(1_710_000_000_200),
                item_list: vec![WeixinMessageItem {
                    item_type: Some(1),
                    text_item: Some(WeixinTextItem {
                        text: Some("allowed".to_string()),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            },
            WeixinInboundMessage {
                from_user_id: Some("u_blocked".to_string()),
                create_time_ms: Some(1_710_000_000_201),
                item_list: vec![WeixinMessageItem {
                    item_type: Some(1),
                    text_item: Some(WeixinTextItem {
                        text: Some("blocked".to_string()),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ];

        let messages = extract_inbound_messages(&source, "acc_allow", &config);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].peer.id, "u_allowed");
        assert_eq!(messages[0].text.as_deref(), Some("allowed"));
    }

    #[test]
    fn extract_inbound_messages_builds_fallback_message_id() {
        let config = WeixinConfig::default();
        let messages = extract_inbound_messages(
            &[WeixinInboundMessage {
                from_user_id: Some("u_fallback".to_string()),
                create_time_ms: Some(1_710_000_000_321),
                message_id: None,
                client_id: None,
                item_list: vec![WeixinMessageItem {
                    item_type: Some(1),
                    text_item: Some(WeixinTextItem {
                        text: Some("hello".to_string()),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            "acc_fb",
            &config,
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].message_id.as_deref(),
            Some("wxilink:acc_fb:u_fallback:1710000000321")
        );
    }

    #[test]
    fn collect_outbound_attachments_extracts_workspace_markdown_links() {
        let text = "![chart](/workspaces/user__c__0/reports/chart.png)\n[report](/workspaces/user__c__0/reports/result.pdf)";
        let attachments = collect_outbound_attachments(&[], Some(text));
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].kind, "image");
        assert_eq!(
            attachments[0].url,
            "/workspaces/user__c__0/reports/chart.png"
        );
        assert_eq!(attachments[1].kind, "file");
        assert_eq!(
            attachments[1].url,
            "/workspaces/user__c__0/reports/result.pdf"
        );
    }

    #[test]
    fn collect_outbound_attachments_infers_kind_from_temp_download_filename() {
        let text = "![preview](https://example.com/wunder/temp_dir/download?filename=channels%2Fweixin%2Fu1%2Fabc_image.jpg)";
        let attachments = collect_outbound_attachments(&[], Some(text));
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].kind, "image");
        assert_eq!(
            attachments[0].url,
            "https://example.com/wunder/temp_dir/download?filename=channels%2Fweixin%2Fu1%2Fabc_image.jpg"
        );
    }

    #[test]
    fn collect_outbound_attachments_deduplicates_existing_and_text_extracted_sources() {
        let existing = ChannelAttachment {
            kind: "file".to_string(),
            url: "/workspaces/user__c__0/reports/result.pdf".to_string(),
            mime: Some("application/pdf".to_string()),
            size: Some(128),
            name: Some("result.pdf".to_string()),
        };
        let text = "[report](/workspaces/user__c__0/reports/result.pdf)";
        let attachments = collect_outbound_attachments(&[existing], Some(text));
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].kind, "file");
        assert_eq!(
            attachments[0].url,
            "/workspaces/user__c__0/reports/result.pdf"
        );
        assert_eq!(attachments[0].name.as_deref(), Some("result.pdf"));
    }

    #[test]
    fn text_item_for_outbound_with_attachments_drops_attachment_only_markdown() {
        let attachments = vec![ChannelAttachment {
            kind: "image".to_string(),
            url: "/workspaces/admin__c__1/heart.png".to_string(),
            mime: None,
            size: None,
            name: None,
        }];
        let text = "![爱心](/workspaces/admin__c__1/heart.png)";
        assert_eq!(
            text_item_for_outbound_with_attachments(Some(text), &attachments),
            None
        );
    }

    #[test]
    fn text_item_for_outbound_with_attachments_keeps_non_attachment_text() {
        let attachments = vec![ChannelAttachment {
            kind: "image".to_string(),
            url: "/workspaces/admin__c__1/heart.png".to_string(),
            mime: None,
            size: None,
            name: None,
        }];
        let text = "爱心图片已生成成功！\n![爱心](/workspaces/admin__c__1/heart.png)";
        assert_eq!(
            text_item_for_outbound_with_attachments(Some(text), &attachments).as_deref(),
            Some("爱心图片已生成成功！")
        );
    }

    #[test]
    fn render_missing_attachment_notice_uses_safe_attachment_names() {
        let attachments = vec![
            ChannelAttachment {
                kind: "image".to_string(),
                url: "/workspaces/admin__c__1/inbox/weixin/1/missing-image.jpg".to_string(),
                mime: None,
                size: None,
                name: Some("photo.jpg".to_string()),
            },
            ChannelAttachment {
                kind: "file".to_string(),
                url: "/workspaces/admin__c__1/inbox/weixin/1/missing-report.pdf".to_string(),
                mime: None,
                size: None,
                name: Some("report.pdf".to_string()),
            },
        ];
        assert_eq!(
            render_missing_attachment_notice(&attachments).as_deref(),
            Some("(attachment unavailable: photo.jpg, report.pdf)")
        );
    }

    #[test]
    fn should_skip_attachment_delivery_error_matches_missing_local_file_errors() {
        assert!(should_skip_attachment_delivery_error(&anyhow!(
            "weixin attachment file not found: /workspaces/admin__c__1/inbox/weixin/demo.jpg"
        )));
        assert!(should_skip_attachment_delivery_error(&anyhow!(
            "weixin attachment path is not a file: /workspaces/admin__c__1/inbox/weixin/demo.jpg"
        )));
        assert!(!should_skip_attachment_delivery_error(&anyhow!(
            "weixin cdn upload failed: status=500"
        )));
    }

    #[tokio::test]
    async fn send_outbound_skips_missing_local_attachment_and_keeps_text_payload() {
        let fixture = start_outbound_server_fixture().await;
        let outbound = ChannelOutboundMessage {
            channel: WEIXIN_CHANNEL.to_string(),
            account_id: "acc_weixin".to_string(),
            peer: ChannelPeer {
                kind: "user".to_string(),
                id: "wx_peer_missing".to_string(),
                name: None,
            },
            thread: None,
            text: Some("Please review the generated summary.".to_string()),
            attachments: vec![ChannelAttachment {
                kind: "image".to_string(),
                url: "/workspaces/admin__c__1/inbox/weixin/missing-image.jpg".to_string(),
                mime: Some("image/jpeg".to_string()),
                size: None,
                name: Some("missing-image.jpg".to_string()),
            }],
            meta: Some(json!({
                "context_token": "ctx-missing-attachment"
            })),
        };
        let config = WeixinConfig {
            api_base: Some(fixture.base_url.clone()),
            bot_token: Some("bot-token-1".to_string()),
            media_enabled: Some(true),
            ..Default::default()
        };

        send_outbound(&Client::new(), &outbound, &config)
            .await
            .expect("send outbound should degrade missing local attachments to text");

        let sendmessage_payloads = fixture.state.sendmessage_payloads.lock().await.clone();
        assert_eq!(sendmessage_payloads.len(), 1);
        assert_eq!(
            sendmessage_payloads[0]["msg"]["item_list"][0]["type"].as_i64(),
            Some(1)
        );
        assert_eq!(
            sendmessage_payloads[0]["msg"]["item_list"][0]["text_item"]["text"].as_str(),
            Some("Please review the generated summary.")
        );
    }
}
