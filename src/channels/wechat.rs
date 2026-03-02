use crate::channels::adapter::{ChannelAdapter, OutboundContext};
use crate::channels::types::{
    ChannelMessage, ChannelOutboundMessage, ChannelPeer, ChannelSender, WechatConfig,
};
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, KeyIvInit};
use aes::Aes256;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::Engine;
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use reqwest::Client;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

type Aes256CbcDec = cbc::Decryptor<Aes256>;

pub const WECHAT_CHANNEL: &str = "wechat";
const TOKEN_REFRESH_LEEWAY_S: u64 = 300;
const TOKEN_MIN_REUSE_S: u64 = 60;
const TOKEN_FALLBACK_EXPIRES_S: u64 = 7200;
const WECHAT_TEXT_MAX_BYTES: usize = 2048;

#[derive(Debug, Default)]
pub struct WechatAdapter;

#[async_trait]
impl ChannelAdapter for WechatAdapter {
    fn channel(&self) -> &'static str {
        WECHAT_CHANNEL
    }

    async fn send_outbound(&self, context: OutboundContext<'_>) -> Result<()> {
        let config = context
            .account_config
            .wechat
            .as_ref()
            .ok_or_else(|| anyhow!("wechat config missing"))?;
        send_outbound(context.http, context.outbound, config).await
    }
}

#[derive(Debug, Clone)]
struct CachedToken {
    token: String,
    expires_at: Instant,
}

static WECHAT_TOKEN_CACHE: LazyLock<Mutex<HashMap<String, CachedToken>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn verify_signature(
    token: &str,
    timestamp: &str,
    nonce: &str,
    encrypted: &str,
    signature: &str,
) -> bool {
    let token = token.trim();
    let timestamp = timestamp.trim();
    let nonce = nonce.trim();
    let encrypted = encrypted.trim();
    let signature = signature.trim();
    if token.is_empty()
        || timestamp.is_empty()
        || nonce.is_empty()
        || encrypted.is_empty()
        || signature.is_empty()
    {
        return false;
    }
    let mut parts = [token, timestamp, nonce, encrypted];
    parts.sort_unstable();
    let mut hasher = Sha1::new();
    for part in parts {
        hasher.update(part.as_bytes());
    }
    let digest = hex::encode(hasher.finalize());
    digest.eq_ignore_ascii_case(signature)
}

pub fn verify_callback_signature(
    token: &str,
    timestamp: &str,
    nonce: &str,
    signature: &str,
) -> bool {
    let token = token.trim();
    let timestamp = timestamp.trim();
    let nonce = nonce.trim();
    let signature = signature.trim();
    if token.is_empty() || timestamp.is_empty() || nonce.is_empty() || signature.is_empty() {
        return false;
    }
    let mut parts = [token, timestamp, nonce];
    parts.sort_unstable();
    let mut hasher = Sha1::new();
    for part in parts {
        hasher.update(part.as_bytes());
    }
    let digest = hex::encode(hasher.finalize());
    digest.eq_ignore_ascii_case(signature)
}

pub fn decrypt_payload(
    encrypted: &str,
    encoding_aes_key: &str,
    expected_receive_id: Option<&str>,
) -> Result<String> {
    let key = decode_encoding_aes_key(encoding_aes_key)?;
    let encrypted = encrypted.trim();
    if encrypted.is_empty() {
        return Err(anyhow!("wechat encrypted payload is empty"));
    }
    let buffer = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|_| anyhow!("wechat encrypted payload is not valid base64"))?;
    if buffer.is_empty() {
        return Err(anyhow!("wechat encrypted payload is empty"));
    }

    let iv = &key[..16];
    let mut cipher_text = buffer;
    let plain = Aes256CbcDec::new(key.as_slice().into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut cipher_text)
        .map_err(|_| anyhow!("wechat payload decrypt failed"))?;
    if plain.len() < 20 {
        return Err(anyhow!("wechat payload is too short"));
    }
    let msg_len: [u8; 4] = plain[16..20]
        .try_into()
        .map_err(|_| anyhow!("wechat payload length parse failed"))?;
    let msg_len = u32::from_be_bytes(msg_len) as usize;
    let msg_start = 20;
    let msg_end = msg_start + msg_len;
    if msg_end > plain.len() {
        return Err(anyhow!("wechat payload length out of range"));
    }

    let receive_id = String::from_utf8_lossy(&plain[msg_end..])
        .trim()
        .to_string();
    if let Some(expected) = expected_receive_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if receive_id != expected {
            return Err(anyhow!("wechat payload receive_id mismatch"));
        }
    }

    String::from_utf8(plain[msg_start..msg_end].to_vec())
        .map_err(|_| anyhow!("wechat payload body is not utf-8"))
}

pub fn parse_xml_fields(xml: &str) -> Result<HashMap<String, String>> {
    let mut reader = XmlReader::from_str(xml);
    reader.trim_text(true);
    let mut buffer = Vec::new();
    let mut current_tag: Option<String> = None;
    let mut output = HashMap::new();

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                current_tag =
                    Some(String::from_utf8_lossy(event.local_name().as_ref()).to_string());
            }
            Ok(Event::Text(event)) => {
                if let Some(tag) = current_tag.take() {
                    let text = event
                        .unescape()
                        .map_err(|_| anyhow!("wechat xml text decode failed"))?
                        .trim()
                        .to_string();
                    if !text.is_empty() {
                        output.insert(tag, text);
                    }
                }
            }
            Ok(Event::CData(event)) => {
                if let Some(tag) = current_tag.take() {
                    let text = String::from_utf8_lossy(event.as_ref()).trim().to_string();
                    if !text.is_empty() {
                        output.insert(tag, text);
                    }
                }
            }
            Ok(Event::End(_)) => {
                current_tag = None;
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(anyhow!("wechat xml parse failed: {err}")),
            _ => {}
        }
        buffer.clear();
    }
    Ok(output)
}

pub fn extract_inbound_messages(
    xml_payload: &str,
    account_id: &str,
) -> Result<Vec<ChannelMessage>> {
    let fields = parse_xml_fields(xml_payload)?;
    let msg_type = fields
        .get("MsgType")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let peer_id = fields
        .get("FromUserName")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("invalid wechat payload: missing FromUserName"))?;
    let mut attachments = Vec::new();
    let mut location = None;
    let (message_type, content) = match msg_type.as_str() {
        "text" => {
            let text = fields
                .get("Content")
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            ("text".to_string(), text)
        }
        "voice" => {
            let recognition = fields
                .get("Recognition")
                .or_else(|| fields.get("Content"))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            if recognition.is_none() {
                if let Some(media_id) = fields
                    .get("MediaId")
                    .or_else(|| fields.get("MediaID"))
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                {
                    attachments.push(crate::channels::types::ChannelAttachment {
                        kind: "voice".to_string(),
                        url: format!("wecom://media/{media_id}"),
                        mime: None,
                        size: None,
                        name: None,
                    });
                }
            }
            (
                "text".to_string(),
                recognition.or_else(|| Some("[voice]".to_string())),
            )
        }
        "image" => {
            if let Some(url) = fields
                .get("PicUrl")
                .or_else(|| fields.get("ImageUrl"))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            {
                attachments.push(crate::channels::types::ChannelAttachment {
                    kind: "image".to_string(),
                    url,
                    mime: None,
                    size: None,
                    name: None,
                });
            } else if let Some(media_id) = fields
                .get("MediaId")
                .or_else(|| fields.get("MediaID"))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            {
                attachments.push(crate::channels::types::ChannelAttachment {
                    kind: "image".to_string(),
                    url: format!("wecom://media/{media_id}"),
                    mime: None,
                    size: None,
                    name: None,
                });
            }
            ("image".to_string(), Some("[image]".to_string()))
        }
        "file" => {
            if let Some(media_id) = fields
                .get("MediaId")
                .or_else(|| fields.get("MediaID"))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            {
                attachments.push(crate::channels::types::ChannelAttachment {
                    kind: "file".to_string(),
                    url: format!("wecom://media/{media_id}"),
                    mime: None,
                    size: None,
                    name: fields
                        .get("FileName")
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty()),
                });
            }
            ("file".to_string(), Some("[file]".to_string()))
        }
        "location" => {
            let lat = fields
                .get("Location_X")
                .or_else(|| fields.get("Latitude"))
                .and_then(|value| value.trim().parse::<f64>().ok());
            let lng = fields
                .get("Location_Y")
                .or_else(|| fields.get("Longitude"))
                .and_then(|value| value.trim().parse::<f64>().ok());
            if let (Some(lat), Some(lng)) = (lat, lng) {
                location = Some(crate::channels::types::ChannelLocation {
                    lat,
                    lng,
                    address: fields
                        .get("Label")
                        .or_else(|| fields.get("Poiname"))
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty()),
                });
                (
                    "location".to_string(),
                    Some(format!("[location] {lat},{lng}")),
                )
            } else {
                return Ok(Vec::new());
            }
        }
        "event" => {
            let event = fields
                .get("Event")
                .map(|value| value.trim().to_ascii_lowercase())
                .unwrap_or_default();
            if matches!(event.as_str(), "enter_agent" | "subscribe" | "unsubscribe") {
                return Ok(Vec::new());
            }
            let event_key = fields
                .get("EventKey")
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            if event == "click" {
                (
                    "text".to_string(),
                    event_key.or_else(|| Some("[wechat_click]".to_string())),
                )
            } else if event_key.is_some() {
                ("text".to_string(), event_key)
            } else {
                return Ok(Vec::new());
            }
        }
        _ => return Ok(Vec::new()),
    };
    let content = content.filter(|value| !value.trim().is_empty());
    if content.is_none() && attachments.is_empty() && location.is_none() {
        return Err(anyhow!("invalid wechat payload: missing content"));
    }
    let message_id = fields
        .get("MsgId")
        .or_else(|| fields.get("MsgID"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| build_fallback_message_id(&fields, account_id, &peer_id, &msg_type));
    let ts = fields
        .get("CreateTime")
        .and_then(|value| value.trim().parse::<f64>().ok());

    Ok(vec![ChannelMessage {
        channel: WECHAT_CHANNEL.to_string(),
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
        message_type,
        text: content,
        attachments,
        location,
        ts,
        meta: Some(json!({ "wechat": fields })),
    }])
}

pub async fn send_outbound(
    http: &Client,
    outbound: &ChannelOutboundMessage,
    config: &WechatConfig,
) -> Result<()> {
    let access_token = fetch_access_token(http, config).await?;
    let base_url = resolve_api_base_url(config);
    let agent_id = config
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("wechat agent_id missing"))?;
    let agent_id = agent_id
        .parse::<i64>()
        .map_err(|_| anyhow!("wechat agent_id must be integer"))?;

    let text = outbound
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            outbound
                .attachments
                .first()
                .map(|attachment| format!("[{}] {}", attachment.kind, attachment.url))
        })
        .unwrap_or_else(|| "(empty message)".to_string());
    let chunks = split_text_by_utf8_bytes(&text, WECHAT_TEXT_MAX_BYTES);
    let duplicate_check_enabled = chunks.len() == 1;
    let peer_id = outbound.peer.id.trim();
    if peer_id.is_empty() {
        return Err(anyhow!("wechat outbound peer id missing"));
    }
    let peer_kind = outbound.peer.kind.trim().to_ascii_lowercase();
    let send_url = format!("{base_url}/cgi-bin/message/send");
    for chunk in chunks {
        let mut payload = json!({
            "msgtype": "text",
            "agentid": agent_id,
            "text": { "content": chunk },
            "safe": 0,
        });
        if duplicate_check_enabled {
            if let Some(map) = payload.as_object_mut() {
                map.insert("enable_duplicate_check".to_string(), json!(1));
                map.insert("duplicate_check_interval".to_string(), json!(1800));
            }
        }
        if peer_kind == "group" {
            if let Some(map) = payload.as_object_mut() {
                map.insert("toparty".to_string(), Value::String(peer_id.to_string()));
            }
        } else if peer_kind == "tag" {
            if let Some(map) = payload.as_object_mut() {
                map.insert("totag".to_string(), Value::String(peer_id.to_string()));
            }
        } else if let Some(map) = payload.as_object_mut() {
            map.insert("touser".to_string(), Value::String(peer_id.to_string()));
        }

        let response = http
            .post(&send_url)
            .query(&[("access_token", access_token.as_str())])
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("wechat outbound failed: {status} {body}"));
        }
        let body: Value = response.json().await?;
        let errcode = body.get("errcode").and_then(Value::as_i64).unwrap_or(-1);
        if errcode != 0 {
            let errmsg = body
                .get("errmsg")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(anyhow!("wechat outbound failed: {errmsg}"));
        }
    }
    Ok(())
}

async fn fetch_access_token(http: &Client, config: &WechatConfig) -> Result<String> {
    let corp_id = config
        .corp_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("wechat corp_id missing"))?;
    let secret = config
        .secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("wechat secret missing"))?;
    let base_url = resolve_api_base_url(config);
    let cache_key = build_token_cache_key(&base_url, corp_id, secret);
    if let Some(cached) = load_cached_token(&cache_key) {
        return Ok(cached);
    }

    let token_url = format!("{base_url}/cgi-bin/gettoken");
    let response = http
        .get(token_url)
        .query(&[("corpid", corp_id), ("corpsecret", secret)])
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("wechat token request failed: {status} {body}"));
    }
    let body: Value = response.json().await?;
    let errcode = body.get("errcode").and_then(Value::as_i64).unwrap_or(-1);
    if errcode != 0 {
        let errmsg = body
            .get("errmsg")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(anyhow!("wechat token request failed: {errmsg}"));
    }
    let token = body
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("wechat token missing access_token"))?;
    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(TOKEN_FALLBACK_EXPIRES_S);
    store_cached_token(&cache_key, &token, expires_in);
    Ok(token)
}

fn resolve_api_base_url(config: &WechatConfig) -> String {
    let domain = config
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("qyapi.weixin.qq.com");
    if domain.starts_with("http://") || domain.starts_with("https://") {
        domain.trim_end_matches('/').to_string()
    } else {
        format!("https://{}", domain.trim_end_matches('/'))
    }
}

fn decode_encoding_aes_key(raw: &str) -> Result<Vec<u8>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(anyhow!("wechat encoding_aes_key missing"));
    }
    let padded = if raw.ends_with('=') {
        raw.to_string()
    } else {
        format!("{raw}=")
    };
    let key = base64::engine::general_purpose::STANDARD
        .decode(padded)
        .map_err(|_| anyhow!("wechat encoding_aes_key is invalid base64"))?;
    if key.len() != 32 {
        return Err(anyhow!("wechat encoding_aes_key length invalid"));
    }
    Ok(key)
}

fn build_fallback_message_id(
    fields: &HashMap<String, String>,
    account_id: &str,
    peer_id: &str,
    msg_type: &str,
) -> Option<String> {
    let create_time = fields
        .get("CreateTime")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())?;
    let event = fields
        .get("Event")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("-");
    let event_key = fields
        .get("EventKey")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("-");
    Some(format!(
        "wx:{account}:{peer}:{ty}:{event}:{event_key}:{ts}",
        account = account_id.trim(),
        peer = peer_id.trim(),
        ty = msg_type.trim(),
        ts = create_time
    ))
}

fn build_token_cache_key(base_url: &str, corp_id: &str, secret: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(secret.as_bytes());
    let secret_hash = hex::encode(hasher.finalize());
    format!(
        "{base}:{corp}:{hash}",
        base = base_url,
        corp = corp_id,
        hash = secret_hash
    )
}

fn load_cached_token(cache_key: &str) -> Option<String> {
    let now = Instant::now();
    let guard = WECHAT_TOKEN_CACHE.lock().ok()?;
    let item = guard.get(cache_key)?;
    if item.expires_at > now + Duration::from_secs(TOKEN_MIN_REUSE_S) {
        return Some(item.token.clone());
    }
    None
}

fn store_cached_token(cache_key: &str, token: &str, expires_in: u64) {
    let usable_s = expires_in
        .saturating_sub(TOKEN_REFRESH_LEEWAY_S)
        .max(TOKEN_MIN_REUSE_S);
    let expires_at = Instant::now() + Duration::from_secs(usable_s);
    if let Ok(mut guard) = WECHAT_TOKEN_CACHE.lock() {
        guard.insert(
            cache_key.to_string(),
            CachedToken {
                token: token.to_string(),
                expires_at,
            },
        );
    }
}

fn split_text_by_utf8_bytes(text: &str, max_bytes: usize) -> Vec<String> {
    if max_bytes == 0 {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_bytes = 0usize;
    for ch in text.chars() {
        let ch_bytes = ch.len_utf8();
        if current_bytes + ch_bytes > max_bytes && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_bytes = 0;
        }
        current.push(ch);
        current_bytes += ch_bytes;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_signature_with_sorted_parts() {
        let token = "token";
        let timestamp = "1710000000";
        let nonce = "abc";
        let encrypted = "cipher_text";
        let mut parts = [token, timestamp, nonce, encrypted];
        parts.sort_unstable();
        let mut hasher = Sha1::new();
        for part in parts {
            hasher.update(part.as_bytes());
        }
        let signature = hex::encode(hasher.finalize());
        assert!(verify_signature(
            token, timestamp, nonce, encrypted, &signature
        ));
    }

    #[test]
    fn verify_callback_signature_with_sorted_parts() {
        let token = "token";
        let timestamp = "1710000000";
        let nonce = "abc";
        let mut parts = [token, timestamp, nonce];
        parts.sort_unstable();
        let mut hasher = Sha1::new();
        for part in parts {
            hasher.update(part.as_bytes());
        }
        let signature = hex::encode(hasher.finalize());
        assert!(verify_callback_signature(
            token, timestamp, nonce, &signature
        ));
    }

    #[test]
    fn parse_xml_fields_extracts_text() {
        let xml =
            "<xml><MsgType><![CDATA[text]]></MsgType><Content><![CDATA[hello]]></Content></xml>";
        let fields = parse_xml_fields(xml).expect("xml should parse");
        assert_eq!(fields.get("MsgType").cloned(), Some("text".to_string()));
        assert_eq!(fields.get("Content").cloned(), Some("hello".to_string()));
    }

    #[test]
    fn extract_inbound_messages_only_text() {
        let xml = "<xml><FromUserName><![CDATA[u1]]></FromUserName><MsgType><![CDATA[text]]></MsgType><Content><![CDATA[你好]]></Content><CreateTime>1710000000</CreateTime></xml>";
        let messages = extract_inbound_messages(xml, "acc_1").expect("extract should succeed");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].channel, WECHAT_CHANNEL);
        assert_eq!(messages[0].peer.id, "u1");
        assert_eq!(messages[0].text.as_deref(), Some("你好"));
    }

    #[test]
    fn extract_inbound_messages_supports_voice_recognition() {
        let xml = "<xml><FromUserName><![CDATA[u1]]></FromUserName><MsgType><![CDATA[voice]]></MsgType><Recognition><![CDATA[测试语音]]></Recognition><CreateTime>1710000001</CreateTime></xml>";
        let messages = extract_inbound_messages(xml, "acc_1").expect("voice should parse");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text.as_deref(), Some("测试语音"));
        assert!(messages[0].message_id.is_some());
    }

    #[test]
    fn split_text_by_utf8_bytes_keeps_boundaries() {
        let text = "hello世界";
        let chunks = split_text_by_utf8_bytes(text, 5);
        assert_eq!(
            chunks,
            vec!["hello".to_string(), "世".to_string(), "界".to_string()]
        );
    }
}
