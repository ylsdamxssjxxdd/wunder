use crate::channels::types::{
    ChannelAttachment, ChannelLocation, ChannelMessage, ChannelOutboundMessage, ChannelPeer,
    ChannelSender, WhatsappCloudConfig,
};
use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::HashMap;
use tracing::warn;

type HmacSha256 = Hmac<Sha256>;

pub const WHATSAPP_CHANNEL: &str = "whatsapp";
const DEFAULT_API_VERSION: &str = "v20.0";

#[derive(Debug, Clone)]
pub struct WhatsappCloudMedia {
    pub id: String,
    pub kind: String,
    pub mime: Option<String>,
    pub caption: Option<String>,
    pub filename: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WhatsappCloudInbound {
    pub account_id: String,
    pub from: String,
    pub name: Option<String>,
    pub message_id: Option<String>,
    pub timestamp: Option<f64>,
    pub message_type: String,
    pub text: Option<String>,
    #[allow(dead_code)]
    pub media: Option<WhatsappCloudMedia>,
    pub location: Option<ChannelLocation>,
    pub meta: Value,
}

pub fn is_whatsapp_cloud_payload(payload: &Value) -> bool {
    payload
        .get("object")
        .and_then(Value::as_str)
        .map(|value| value.eq_ignore_ascii_case("whatsapp_business_account"))
        .unwrap_or(false)
}

pub fn extract_inbound_messages(
    payload: &Value,
    account_override: Option<&str>,
) -> Result<Vec<WhatsappCloudInbound>> {
    let mut items = Vec::new();
    let entries = payload
        .get("entry")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("invalid whatsapp payload: missing entry"))?;

    for entry in entries {
        let entry_id = entry.get("id").cloned().unwrap_or(Value::Null);
        let Some(changes) = entry.get("changes").and_then(Value::as_array) else {
            continue;
        };
        for change in changes {
            let value = change.get("value").unwrap_or(&Value::Null);
            let metadata = value.get("metadata").unwrap_or(&Value::Null);
            let account_id = account_override
                .and_then(|value| {
                    let trimmed = value.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                })
                .or_else(|| {
                    metadata
                        .get("phone_number_id")
                        .and_then(Value::as_str)
                        .map(|value| value.to_string())
                });
            let Some(account_id) = account_id else {
                return Err(anyhow!("invalid whatsapp payload: missing account id"));
            };

            let mut contact_names: HashMap<String, String> = HashMap::new();
            if let Some(contacts) = value.get("contacts").and_then(Value::as_array) {
                for contact in contacts {
                    let Some(wa_id) = contact
                        .get("wa_id")
                        .and_then(Value::as_str)
                        .map(|value| value.to_string())
                    else {
                        continue;
                    };
                    let name = contact
                        .get("profile")
                        .and_then(|profile| profile.get("name"))
                        .and_then(Value::as_str)
                        .map(|value| value.to_string());
                    if let Some(name) = name {
                        contact_names.insert(wa_id, name);
                    }
                }
            }

            let Some(messages) = value.get("messages").and_then(Value::as_array) else {
                continue;
            };
            for message in messages {
                let from = message
                    .get("from")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if from.is_empty() {
                    continue;
                }
                let message_id = message
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string());
                let timestamp = message
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(|value| value.parse::<f64>().ok());
                let message_type = message
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("text")
                    .to_string();

                let mut text: Option<String> = None;
                let mut media: Option<WhatsappCloudMedia> = None;
                let mut location: Option<ChannelLocation> = None;

                match message_type.as_str() {
                    "text" => {
                        text = message
                            .get("text")
                            .and_then(|value| value.get("body"))
                            .and_then(Value::as_str)
                            .map(|value| value.to_string());
                    }
                    "image" | "audio" | "video" | "document" | "sticker" => {
                        if let Some(block) = message.get(&message_type) {
                            media = parse_media(&message_type, block);
                            text = block
                                .get("caption")
                                .and_then(Value::as_str)
                                .map(|value| value.to_string());
                        }
                    }
                    "location" => {
                        if let Some(loc) = message.get("location") {
                            location = parse_location(loc);
                            text = loc
                                .get("name")
                                .or_else(|| loc.get("address"))
                                .and_then(Value::as_str)
                                .map(|value| value.to_string());
                        }
                    }
                    "interactive" => {
                        if let Some(interactive) = message.get("interactive") {
                            text = extract_interactive_text(interactive);
                        }
                    }
                    "button" => {
                        if let Some(button) = message.get("button") {
                            text = button
                                .get("text")
                                .or_else(|| button.get("payload"))
                                .and_then(Value::as_str)
                                .map(|value| value.to_string());
                        }
                    }
                    _ => {}
                }

                if text
                    .as_ref()
                    .map(|value| value.trim().is_empty())
                    .unwrap_or(true)
                    && matches!(
                        message_type.as_str(),
                        "image" | "audio" | "video" | "document" | "sticker"
                    )
                {
                    text = Some(format!("[WhatsApp {} message]", message_type.as_str()));
                }

                let mut meta = json!({
                    "wa": {
                        "business_id": entry_id,
                        "phone_number_id": metadata.get("phone_number_id").cloned().unwrap_or(Value::Null),
                        "message_id": message_id,
                        "type": message_type,
                        "context": message.get("context").cloned().unwrap_or(Value::Null),
                    }
                });
                if let Some(media) = media.as_ref() {
                    if let Some(obj) = meta.as_object_mut() {
                        obj.insert(
                            "media".to_string(),
                            json!({
                                "id": media.id,
                                "kind": media.kind,
                                "mime": media.mime,
                                "caption": media.caption,
                                "filename": media.filename,
                            }),
                        );
                    }
                }

                let name = contact_names.get(&from).cloned();
                items.push(WhatsappCloudInbound {
                    account_id: account_id.clone(),
                    from,
                    name,
                    message_id,
                    timestamp,
                    message_type: message_type.clone(),
                    text,
                    media,
                    location,
                    meta,
                });
            }
        }
    }

    Ok(items)
}

pub fn inbound_to_channel_message(
    inbound: WhatsappCloudInbound,
    attachments: Vec<ChannelAttachment>,
) -> ChannelMessage {
    let peer_kind = if inbound.from.contains("@g.us") {
        "group".to_string()
    } else {
        "dm".to_string()
    };
    ChannelMessage {
        channel: WHATSAPP_CHANNEL.to_string(),
        account_id: inbound.account_id,
        peer: ChannelPeer {
            kind: peer_kind,
            id: inbound.from.clone(),
            name: inbound.name.clone(),
        },
        thread: None,
        message_id: inbound.message_id.clone(),
        sender: Some(ChannelSender {
            id: inbound.from,
            name: inbound.name,
        }),
        message_type: inbound.message_type,
        text: inbound.text,
        attachments,
        location: inbound.location,
        ts: inbound.timestamp,
        meta: Some(inbound.meta),
    }
}

pub async fn send_outbound(
    http: &Client,
    outbound: &ChannelOutboundMessage,
    config: &WhatsappCloudConfig,
) -> Result<()> {
    let access_token = config
        .access_token
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("whatsapp cloud access_token missing"))?;
    let phone_number_id = config
        .phone_number_id
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| outbound.account_id.clone());
    let api_version = config
        .api_version
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_API_VERSION);
    let endpoint = format!("https://graph.facebook.com/{api_version}/{phone_number_id}/messages");
    let to = normalize_recipient(&outbound.peer.id);
    let context_id = outbound
        .meta
        .as_ref()
        .and_then(|value| value.get("message_id"))
        .and_then(Value::as_str)
        .map(|value| value.to_string());

    if let Some(text) = outbound
        .text
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        let mut payload = json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "text",
            "text": { "body": text, "preview_url": false }
        });
        if let Some(context_id) = context_id.as_ref() {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("context".to_string(), json!({ "message_id": context_id }));
            }
        }
        post_whatsapp_message(http, &endpoint, &access_token, payload).await?;
    }

    for attachment in &outbound.attachments {
        let link = attachment.url.trim();
        if link.is_empty() {
            continue;
        }
        let (payload, skip) = match attachment.kind.trim().to_lowercase().as_str() {
            "image" | "photo" | "picture" => (
                json!({
                    "messaging_product": "whatsapp",
                    "to": to,
                    "type": "image",
                    "image": { "link": link }
                }),
                false,
            ),
            "audio" | "voice" => (
                json!({
                    "messaging_product": "whatsapp",
                    "to": to,
                    "type": "audio",
                    "audio": { "link": link }
                }),
                false,
            ),
            "video" => (
                json!({
                    "messaging_product": "whatsapp",
                    "to": to,
                    "type": "video",
                    "video": { "link": link }
                }),
                false,
            ),
            "file" | "document" => (
                json!({
                    "messaging_product": "whatsapp",
                    "to": to,
                    "type": "document",
                    "document": {
                        "link": link,
                        "filename": attachment.name.clone().unwrap_or_else(|| "file".to_string())
                    }
                }),
                false,
            ),
            other => {
                warn!("unsupported whatsapp attachment kind: {}", other);
                (json!({}), true)
            }
        };
        if skip {
            continue;
        }
        post_whatsapp_message(http, &endpoint, &access_token, payload).await?;
    }

    Ok(())
}

#[allow(dead_code)]
pub async fn fetch_media_attachment(
    http: &Client,
    config: &WhatsappCloudConfig,
    media: &WhatsappCloudMedia,
) -> Result<Option<ChannelAttachment>> {
    let access_token = config
        .access_token
        .as_deref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let Some(access_token) = access_token else {
        return Ok(None);
    };
    let api_version = config
        .api_version
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_API_VERSION);
    let info_url = format!("https://graph.facebook.com/{api_version}/{}", media.id);
    let response = http.get(info_url).bearer_auth(access_token).send().await?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let payload: Value = response.json().await.unwrap_or(Value::Null);
    let url = payload
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if url.is_empty() {
        return Ok(None);
    }
    let mime = payload
        .get("mime_type")
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .or_else(|| media.mime.clone());
    let size = payload.get("file_size").and_then(Value::as_i64).or(None);
    Ok(Some(ChannelAttachment {
        kind: media.kind.clone(),
        url,
        mime,
        size,
        name: media.filename.clone(),
    }))
}

pub fn verify_signature_any(body: &[u8], signature: &str, secrets: &[String]) -> bool {
    let signature = signature
        .trim()
        .strip_prefix("sha256=")
        .unwrap_or(signature)
        .trim();
    let expected = match hex::decode(signature) {
        Ok(value) => value,
        Err(_) => return false,
    };
    for secret in secrets {
        if verify_signature(body, secret, &expected) {
            return true;
        }
    }
    false
}

fn verify_signature(body: &[u8], secret: &str, expected: &[u8]) -> bool {
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(value) => value,
        Err(_) => return false,
    };
    mac.update(body);
    mac.verify_slice(expected).is_ok()
}

fn parse_media(kind: &str, value: &Value) -> Option<WhatsappCloudMedia> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(|value| value.to_string())?;
    let mime = value
        .get("mime_type")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let caption = value
        .get("caption")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let filename = value
        .get("filename")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    Some(WhatsappCloudMedia {
        id,
        kind: kind.to_string(),
        mime,
        caption,
        filename,
    })
}

fn parse_location(value: &Value) -> Option<ChannelLocation> {
    let lat = value.get("latitude").and_then(Value::as_f64)?;
    let lng = value.get("longitude").and_then(Value::as_f64)?;
    let address = value
        .get("address")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    Some(ChannelLocation { lat, lng, address })
}

fn extract_interactive_text(value: &Value) -> Option<String> {
    let interactive_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match interactive_type.as_str() {
        "button_reply" => value
            .get("button_reply")
            .and_then(|node| node.get("title").or_else(|| node.get("id")))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        "list_reply" => value
            .get("list_reply")
            .and_then(|node| node.get("title").or_else(|| node.get("id")))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        _ => None,
    }
}

fn normalize_recipient(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.contains('@') {
        return trimmed.to_string();
    }
    trimmed.chars().filter(|c| c.is_ascii_digit()).collect()
}

async fn post_whatsapp_message(
    http: &Client,
    endpoint: &str,
    access_token: &str,
    payload: Value,
) -> Result<()> {
    let response = http
        .post(endpoint)
        .bearer_auth(access_token)
        .json(&payload)
        .send()
        .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(anyhow!("whatsapp outbound failed: {status} {body}"))
    }
}
