use crate::channels::types::{
    ChannelMessage, ChannelOutboundMessage, ChannelPeer, ChannelSender, WechatConfig,
};
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, KeyIvInit};
use aes::Aes256;
use anyhow::{anyhow, Result};
use base64::Engine;
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use reqwest::Client;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::HashMap;

type Aes256CbcDec = cbc::Decryptor<Aes256>;

pub const WECHAT_CHANNEL: &str = "wechat";

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
        if !receive_id.is_empty() && !receive_id.eq_ignore_ascii_case(expected) {
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
    if msg_type != "text" {
        return Ok(Vec::new());
    }

    let peer_id = fields
        .get("FromUserName")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("invalid wechat payload: missing FromUserName"))?;
    let content = fields
        .get("Content")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("invalid wechat payload: missing Content"))?;
    let message_id = fields
        .get("MsgId")
        .or_else(|| fields.get("MsgID"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
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
        message_type: "text".to_string(),
        text: Some(content),
        attachments: Vec::new(),
        location: None,
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
    let peer_id = outbound.peer.id.trim();
    if peer_id.is_empty() {
        return Err(anyhow!("wechat outbound peer id missing"));
    }

    let mut payload = json!({
        "msgtype": "text",
        "agentid": agent_id,
        "text": { "content": text },
        "safe": 0
    });
    let peer_kind = outbound.peer.kind.trim().to_ascii_lowercase();
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

    let send_url = format!("{base_url}/cgi-bin/message/send");
    let response = http
        .post(send_url)
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
    body.get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("wechat token missing access_token"))
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
}
