use crate::channels::types::{
    ChannelAttachment, ChannelMessage, ChannelOutboundMessage, ChannelPeer, ChannelSender,
    FeishuConfig,
};
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, KeyIvInit};
use aes::Aes256;
use anyhow::{anyhow, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

type Aes256CbcDec = cbc::Decryptor<Aes256>;

pub const FEISHU_CHANNEL: &str = "feishu";

pub fn verify_challenge_token(payload: &Value, token: &str) -> bool {
    let provided = payload
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if provided.is_empty() {
        return false;
    }
    provided == token.trim()
}

pub fn challenge_response(payload: &Value) -> Option<Value> {
    payload
        .get("challenge")
        .and_then(Value::as_str)
        .map(|challenge| json!({ "challenge": challenge }))
}

pub fn verify_sign(
    encrypt_key: &str,
    timestamp: &str,
    nonce: &str,
    body: &[u8],
    sign: &str,
) -> bool {
    let encrypt_key = encrypt_key.trim();
    if encrypt_key.is_empty() {
        return false;
    }
    let mut payload = Vec::new();
    payload.extend_from_slice(timestamp.as_bytes());
    payload.extend_from_slice(nonce.as_bytes());
    payload.extend_from_slice(encrypt_key.as_bytes());
    payload.extend_from_slice(body);
    let digest = Sha256::digest(payload);
    let expected = hex::encode(digest);
    expected.eq_ignore_ascii_case(sign.trim())
}

pub fn decrypt_event_if_needed(payload: Value, encrypt_key: Option<&str>) -> Result<Value> {
    let has_encrypt = payload.get("encrypt").and_then(Value::as_str).is_some();
    if !has_encrypt {
        return Ok(payload);
    }
    let key = match encrypt_key.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value,
        None => {
            return Err(anyhow!("feishu encrypt payload but encrypt_key missing"));
        }
    };
    let encrypted = payload
        .get("encrypt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("invalid feishu payload: missing encrypt"))?;
    let buffer = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|_| anyhow!("invalid feishu payload: encrypt not base64"))?;
    if buffer.len() < 16 + 32 {
        return Err(anyhow!("invalid feishu payload: encrypted body too short"));
    }
    let key_bytes = Sha256::digest(key.as_bytes());
    let iv = &buffer[..16];
    let mut cipher_text = buffer[16..].to_vec();
    let plain = Aes256CbcDec::new(key_bytes.as_slice().into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut cipher_text)
        .map_err(|_| anyhow!("failed to decrypt feishu payload"))?;
    if plain.len() < 20 {
        return Err(anyhow!("invalid feishu payload: plain body too short"));
    }
    let body_len: [u8; 4] = plain[16..20]
        .try_into()
        .map_err(|_| anyhow!("invalid feishu payload: body length"))?;
    let body_len = u32::from_be_bytes(body_len) as usize;
    let json_start = 20;
    let json_end = json_start + body_len;
    if json_end > plain.len() {
        return Err(anyhow!("invalid feishu payload: body out of range"));
    }
    let json_bytes = &plain[json_start..json_end];
    let json_payload: Value = serde_json::from_slice(json_bytes)
        .map_err(|_| anyhow!("invalid feishu payload: decrypted json invalid"))?;
    Ok(json_payload)
}

pub fn extract_inbound_messages(
    payload: &Value,
    account_id: &str,
    default_peer_kind: Option<&str>,
) -> Result<Vec<ChannelMessage>> {
    let event = payload
        .get("event")
        .ok_or_else(|| anyhow!("invalid feishu payload: missing event"))?;
    let sender = event
        .get("sender")
        .or_else(|| payload.get("sender"))
        .unwrap_or(&Value::Null);
    let sender_id = sender
        .get("sender_id")
        .and_then(|value| {
            value
                .get("open_id")
                .or_else(|| value.get("user_id"))
                .or_else(|| value.get("union_id"))
        })
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let message = event
        .get("message")
        .ok_or_else(|| anyhow!("invalid feishu payload: missing event.message"))?;
    let chat_id = message
        .get("chat_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if chat_id.is_empty() {
        return Err(anyhow!("invalid feishu payload: missing chat_id"));
    }
    let chat_type = message
        .get("chat_type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let peer_kind = if chat_type == "group" {
        "group".to_string()
    } else {
        default_peer_kind
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "user".to_string())
    };
    let message_type = message
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or("text")
        .trim()
        .to_ascii_lowercase();
    let content_raw = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let content: Value = if content_raw.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(content_raw).unwrap_or(Value::Null)
    };
    let mut attachments = Vec::new();
    let text = match message_type.as_str() {
        "text" => content
            .get("text")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        "image" => {
            if let Some(image_key) = content.get("image_key").and_then(Value::as_str) {
                attachments.push(ChannelAttachment {
                    kind: "image".to_string(),
                    url: image_key.to_string(),
                    mime: None,
                    size: None,
                    name: None,
                });
            }
            Some("[image]".to_string())
        }
        "file" | "audio" | "media" => {
            if let Some(file_key) = content
                .get("file_key")
                .or_else(|| content.get("image_key"))
                .and_then(Value::as_str)
            {
                attachments.push(ChannelAttachment {
                    kind: message_type.clone(),
                    url: file_key.to_string(),
                    mime: None,
                    size: None,
                    name: content
                        .get("file_name")
                        .and_then(Value::as_str)
                        .map(|value| value.to_string()),
                });
            }
            Some(format!("[{}]", message_type))
        }
        _ => Some(format!("[{}]", message_type)),
    };
    let message_id = message
        .get("message_id")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let ts = message
        .get("create_time")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .map(|value| value / 1000.0);
    let sender_name = sender
        .get("sender_id")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .map(|value| value.to_string());

    Ok(vec![ChannelMessage {
        channel: FEISHU_CHANNEL.to_string(),
        account_id: account_id.to_string(),
        peer: ChannelPeer {
            kind: peer_kind,
            id: chat_id,
            name: None,
        },
        thread: None,
        message_id,
        sender: if sender_id.is_empty() {
            None
        } else {
            Some(ChannelSender {
                id: sender_id,
                name: sender_name,
            })
        },
        message_type: if attachments.is_empty() {
            "text".to_string()
        } else {
            "mixed".to_string()
        },
        text,
        attachments,
        location: None,
        ts,
        meta: Some(payload.clone()),
    }])
}

pub async fn send_outbound(
    http: &Client,
    outbound: &ChannelOutboundMessage,
    config: &FeishuConfig,
) -> Result<()> {
    let app_id = config
        .app_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("feishu app_id missing"))?;
    let app_secret = config
        .app_secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("feishu app_secret missing"))?;
    let domain = config
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("open.feishu.cn");
    let token_url = format!("https://{domain}/open-apis/auth/v3/tenant_access_token/internal");
    let token_resp = http
        .post(token_url)
        .json(&json!({ "app_id": app_id, "app_secret": app_secret }))
        .send()
        .await?;
    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        return Err(anyhow!("feishu token failed: {status} {body}"));
    }
    let token_payload: Value = token_resp.json().await?;
    let code = token_payload
        .get("code")
        .and_then(Value::as_i64)
        .unwrap_or(-1);
    if code != 0 {
        let message = token_payload
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(anyhow!("feishu token failed: {message}"));
    }
    let tenant_token = token_payload
        .get("tenant_access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("feishu token missing tenant_access_token"))?;
    let receive_id_type = config
        .receive_id_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("chat_id");
    let text = outbound
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| {
            outbound
                .attachments
                .first()
                .map(|attachment| format!("[{}] {}", attachment.kind, attachment.url))
        })
        .unwrap_or_else(|| "(empty message)".to_string());
    let send_url =
        format!("https://{domain}/open-apis/im/v1/messages?receive_id_type={receive_id_type}");
    let send_resp = http
        .post(send_url)
        .bearer_auth(tenant_token)
        .json(&json!({
            "receive_id": outbound.peer.id,
            "msg_type": "text",
            "content": json!({ "text": text }).to_string(),
        }))
        .send()
        .await?;
    if send_resp.status().is_success() {
        return Ok(());
    }
    let status = send_resp.status();
    let body = send_resp.text().await.unwrap_or_default();
    Err(anyhow!("feishu outbound failed: {status} {body}"))
}
