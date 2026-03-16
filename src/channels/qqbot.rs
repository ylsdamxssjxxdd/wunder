use crate::channels::adapter::{ChannelAdapter, OutboundContext};
use crate::channels::types::{ChannelOutboundMessage, QqBotConfig};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use ed25519_dalek::{Signer, SigningKey};
use reqwest::Client;
use serde_json::{json, Value};

pub const QQBOT_CHANNEL: &str = "qqbot";
const QQ_API_BASE: &str = "https://api.sgroup.qq.com";
const QQ_TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
pub const QQBOT_CALLBACK_DISPATCH_EVENT_OP: i64 = 0;
const QQBOT_CALLBACK_HEARTBEAT_OP: i64 = 1;
const QQBOT_CALLBACK_HEARTBEAT_ACK_OP: i64 = 11;
const QQBOT_CALLBACK_ACK_OP: i64 = 12;
const QQBOT_CALLBACK_VALIDATION_OP: i64 = 13;
const ED25519_SEED_SIZE: usize = 32;

#[derive(Debug, Default)]
pub struct QqBotAdapter;

#[async_trait]
impl ChannelAdapter for QqBotAdapter {
    fn channel(&self) -> &'static str {
        QQBOT_CHANNEL
    }

    async fn send_outbound(&self, context: OutboundContext<'_>) -> Result<()> {
        let config = context
            .account_config
            .qqbot
            .as_ref()
            .ok_or_else(|| anyhow!("qqbot config missing"))?;
        send_outbound(context.http, context.outbound, config).await
    }
}

pub fn callback_opcode(payload: &Value) -> Option<i64> {
    payload.get("op").and_then(Value::as_i64)
}

pub fn is_validation_event(payload: &Value) -> bool {
    callback_opcode(payload) == Some(QQBOT_CALLBACK_VALIDATION_OP)
}

pub fn is_dispatch_event(payload: &Value) -> bool {
    callback_opcode(payload) == Some(QQBOT_CALLBACK_DISPATCH_EVENT_OP)
}

pub fn dispatch_ack(success: bool) -> Value {
    json!({
        "op": QQBOT_CALLBACK_ACK_OP,
        "d": if success { 0 } else { 1 }
    })
}

pub fn heartbeat_ack(payload: &Value) -> Option<Value> {
    if callback_opcode(payload) != Some(QQBOT_CALLBACK_HEARTBEAT_OP) {
        return None;
    }
    let seq = payload.get("d").and_then(Value::as_u64).unwrap_or_default();
    Some(json!({
        "op": QQBOT_CALLBACK_HEARTBEAT_ACK_OP,
        "d": seq
    }))
}

pub fn inbound_message_payload(payload: &Value) -> &Value {
    payload
        .get("message")
        .or_else(|| payload.get("d"))
        .unwrap_or(payload)
}

fn value_to_trimmed_string(value: &Value) -> Option<String> {
    match value {
        Value::String(item) => {
            let text = item.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        }
        Value::Number(item) => Some(item.to_string()),
        _ => None,
    }
}

pub fn validation_response(payload: &Value, client_secret: Option<&str>) -> Result<Option<Value>> {
    if !is_validation_event(payload) {
        return Ok(None);
    }
    let data = payload
        .get("d")
        .ok_or_else(|| anyhow!("invalid qqbot callback: missing d"))?;
    let plain_token = data
        .get("plain_token")
        .and_then(value_to_trimmed_string)
        .ok_or_else(|| anyhow!("invalid qqbot callback: missing plain_token"))?;
    let event_ts = data
        .get("event_ts")
        .and_then(value_to_trimmed_string)
        .ok_or_else(|| anyhow!("invalid qqbot callback: missing event_ts"))?;
    let secret = client_secret
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("qqbot client_secret missing for callback validation"))?;
    let signature = sign_validation_payload(secret, &event_ts, &plain_token)?;
    Ok(Some(json!({
        "plain_token": plain_token,
        "signature": signature
    })))
}

fn sign_validation_payload(
    client_secret: &str,
    event_ts: &str,
    plain_token: &str,
) -> Result<String> {
    let seed = derive_seed(client_secret)?;
    let signing_key = SigningKey::from_bytes(&seed);
    let mut payload = Vec::with_capacity(event_ts.len() + plain_token.len());
    payload.extend_from_slice(event_ts.as_bytes());
    payload.extend_from_slice(plain_token.as_bytes());
    let signature = signing_key.sign(&payload);
    Ok(hex::encode(signature.to_bytes()))
}

fn derive_seed(secret: &str) -> Result<[u8; ED25519_SEED_SIZE]> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(anyhow!("qqbot client_secret missing"));
    }
    // Tencent botgo derives the seed by repeating secret bytes until 32 bytes, then truncating.
    let mut seed_bytes = secret.as_bytes().to_vec();
    while seed_bytes.len() < ED25519_SEED_SIZE {
        seed_bytes.extend_from_within(..);
    }
    let mut seed = [0_u8; ED25519_SEED_SIZE];
    seed.copy_from_slice(&seed_bytes[..ED25519_SEED_SIZE]);
    Ok(seed)
}

pub async fn send_outbound(
    http: &Client,
    outbound: &ChannelOutboundMessage,
    config: &QqBotConfig,
) -> Result<()> {
    let app_id = config
        .app_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("qqbot app_id missing"))?;
    let client_secret = config
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("qqbot client_secret missing"))?;
    let token_resp = http
        .post(QQ_TOKEN_URL)
        .json(&json!({
            "appId": app_id,
            "clientSecret": client_secret,
        }))
        .send()
        .await?;
    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        return Err(anyhow!("qqbot token failed: {status} {body}"));
    }
    let token_payload: Value = token_resp.json().await?;
    if token_payload
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|value| value != 0)
    {
        let code = token_payload
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        let message = token_payload
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("");
        return Err(anyhow!("qqbot token failed: code={code} message={message}"));
    }
    let access_token = token_payload
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("qqbot token missing access_token"))?;

    let peer_kind = outbound.peer.kind.trim().to_ascii_lowercase();
    let mut text = outbound
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_default();
    if text.is_empty() {
        text = outbound
            .attachments
            .first()
            .map(|attachment| format!("[{}] {}", attachment.kind, attachment.url))
            .unwrap_or_else(|| "(empty message)".to_string());
    }
    let payload = if config.markdown_support.unwrap_or(false) {
        json!({
            "msg_type": 2,
            "markdown": { "content": text },
            "msg_seq": 1,
        })
    } else {
        json!({
            "msg_type": 0,
            "content": text,
            "msg_seq": 1,
        })
    };

    if peer_kind == "group" {
        post_message(
            http,
            app_id,
            access_token,
            &format!("{QQ_API_BASE}/v2/groups/{}/messages", outbound.peer.id),
            payload,
        )
        .await
    } else if peer_kind == "channel" {
        post_message(
            http,
            app_id,
            access_token,
            &format!("{QQ_API_BASE}/channels/{}/messages", outbound.peer.id),
            json!({ "content": text }),
        )
        .await
    } else {
        post_message(
            http,
            app_id,
            access_token,
            &format!("{QQ_API_BASE}/v2/users/{}/messages", outbound.peer.id),
            payload,
        )
        .await
    }
}

async fn post_message(
    http: &Client,
    app_id: &str,
    access_token: &str,
    url: &str,
    payload: Value,
) -> Result<()> {
    let response = http
        .post(url)
        .header("Authorization", format!("QQBot {access_token}"))
        .header("X-Union-Appid", app_id)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(anyhow!("qqbot outbound failed: {status} {body}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_response_returns_signature_for_opcode_13() {
        let payload = json!({
            "op": 13,
            "d": {
                "plain_token": "plain-token",
                "event_ts": "1700000000"
            }
        });

        let response = validation_response(&payload, Some("secret-key"))
            .expect("validation should succeed")
            .expect("validation response should exist");
        assert_eq!(
            response.get("plain_token").and_then(Value::as_str),
            Some("plain-token")
        );
        let signature = response
            .get("signature")
            .and_then(Value::as_str)
            .expect("signature should exist");
        assert_eq!(signature.len(), 128);
        assert!(signature.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn validation_response_requires_secret_for_opcode_13() {
        let payload = json!({
            "op": 13,
            "d": {
                "plain_token": "plain-token",
                "event_ts": "1700000000"
            }
        });

        let err = validation_response(&payload, None).expect_err("secret should be required");
        assert!(err
            .to_string()
            .contains("client_secret missing for callback validation"));
    }

    #[test]
    fn validation_response_supports_numeric_event_ts() {
        let payload = json!({
            "op": 13,
            "d": {
                "plain_token": "plain-token",
                "event_ts": 1700000000
            }
        });

        let response = validation_response(&payload, Some("secret-key"))
            .expect("validation should succeed")
            .expect("validation response should exist");
        assert_eq!(
            response.get("plain_token").and_then(Value::as_str),
            Some("plain-token")
        );
        let signature = response
            .get("signature")
            .and_then(Value::as_str)
            .expect("signature should exist");
        assert_eq!(signature.len(), 128);
        assert!(signature.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn callback_opcode_helpers_match_qq_contract() {
        let dispatch = json!({"op": 0});
        assert!(is_dispatch_event(&dispatch));
        assert_eq!(dispatch_ack(true), json!({"op": 12, "d": 0}));
        assert_eq!(dispatch_ack(false), json!({"op": 12, "d": 1}));

        let heartbeat = json!({"op": 1, "d": 9});
        assert_eq!(heartbeat_ack(&heartbeat), Some(json!({"op": 11, "d": 9})));

        let non_callback = json!({"content": "hello"});
        assert_eq!(callback_opcode(&non_callback), None);
        assert!(!is_dispatch_event(&non_callback));
        assert!(!is_validation_event(&non_callback));
    }

    #[test]
    fn inbound_message_payload_prefers_message_field() {
        let payload = json!({
            "message": { "content": "from-message" },
            "d": { "content": "from-d" }
        });
        assert_eq!(
            inbound_message_payload(&payload)
                .get("content")
                .and_then(Value::as_str),
            Some("from-message")
        );
    }
}
