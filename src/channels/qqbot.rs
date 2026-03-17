use crate::channels::adapter::{ChannelAdapter, OutboundContext};
use crate::channels::types::{
    ChannelAttachment, ChannelMessage, ChannelOutboundMessage, ChannelPeer, ChannelSender,
    QqBotConfig,
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use ed25519_dalek::{Signer, SigningKey};
use futures::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use std::future::Future;
use tokio::time::{interval, Duration, MissedTickBehavior};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};

pub const QQBOT_CHANNEL: &str = "qqbot";
const QQ_API_BASE: &str = "https://api.sgroup.qq.com";
const QQ_TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const QQ_GATEWAY_URL: &str = "https://api.sgroup.qq.com/gateway";
pub const QQBOT_CALLBACK_DISPATCH_EVENT_OP: i64 = 0;
const QQBOT_CALLBACK_HEARTBEAT_OP: i64 = 1;
const QQBOT_GATEWAY_IDENTIFY_OP: i64 = 2;
const QQBOT_GATEWAY_RESUME_OP: i64 = 6;
const QQBOT_GATEWAY_RECONNECT_OP: i64 = 7;
const QQBOT_GATEWAY_INVALID_SESSION_OP: i64 = 9;
const QQBOT_GATEWAY_HELLO_OP: i64 = 10;
const QQBOT_CALLBACK_HEARTBEAT_ACK_OP: i64 = 11;
const QQBOT_CALLBACK_ACK_OP: i64 = 12;
const QQBOT_CALLBACK_VALIDATION_OP: i64 = 13;
const QQBOT_DEFAULT_HEARTBEAT_INTERVAL_MS: u64 = 30_000;
const QQBOT_MIN_HEARTBEAT_INTERVAL_MS: u64 = 1_000;
const QQBOT_INTENT_GUILD_MEMBERS: u64 = 1 << 1;
const QQBOT_INTENT_PUBLIC_GUILD_MESSAGES: u64 = 1 << 30;
const QQBOT_INTENT_DIRECT_MESSAGE: u64 = 1 << 12;
const QQBOT_INTENT_GROUP_AND_C2C: u64 = 1 << 25;
const QQBOT_DEFAULT_LONG_CONNECTION_INTENTS: u64 =
    QQBOT_INTENT_PUBLIC_GUILD_MESSAGES | QQBOT_INTENT_DIRECT_MESSAGE | QQBOT_INTENT_GROUP_AND_C2C;
const QQBOT_GROUP_CHANNEL_LONG_CONNECTION_INTENTS: u64 =
    QQBOT_INTENT_PUBLIC_GUILD_MESSAGES | QQBOT_INTENT_GROUP_AND_C2C;
const QQBOT_CHANNEL_ONLY_LONG_CONNECTION_INTENTS: u64 =
    QQBOT_INTENT_PUBLIC_GUILD_MESSAGES | QQBOT_INTENT_GUILD_MEMBERS;
const QQBOT_EVENT_C2C_MESSAGE_CREATE: &str = "C2C_MESSAGE_CREATE";
const QQBOT_EVENT_AT_MESSAGE_CREATE: &str = "AT_MESSAGE_CREATE";
const QQBOT_EVENT_DIRECT_MESSAGE_CREATE: &str = "DIRECT_MESSAGE_CREATE";
const QQBOT_EVENT_GROUP_AT_MESSAGE_CREATE: &str = "GROUP_AT_MESSAGE_CREATE";
const ED25519_SEED_SIZE: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QqBotCredentials {
    pub app_id: String,
    pub client_secret: String,
}

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

pub fn long_connection_enabled(config: &QqBotConfig) -> bool {
    config.long_connection_enabled.unwrap_or(true)
}

pub fn has_long_connection_credentials(config: &QqBotConfig) -> bool {
    resolve_credentials(config).is_ok()
}

pub fn resolved_app_id(config: &QqBotConfig) -> Option<String> {
    resolve_credentials(config).ok().map(|value| value.app_id)
}

pub fn resolved_client_secret(config: &QqBotConfig) -> Option<String> {
    resolve_credentials(config)
        .ok()
        .map(|value| value.client_secret)
}

pub fn resolve_long_connection_intents(config: &QqBotConfig) -> u64 {
    resolve_long_connection_intent_candidates(config)
        .into_iter()
        .next()
        .unwrap_or(QQBOT_DEFAULT_LONG_CONNECTION_INTENTS)
}

pub fn resolve_long_connection_intent_candidates(config: &QqBotConfig) -> Vec<u64> {
    if let Some(intents) = config.intents.filter(|value| *value > 0) {
        return vec![intents];
    }
    vec![
        QQBOT_DEFAULT_LONG_CONNECTION_INTENTS,
        QQBOT_GROUP_CHANNEL_LONG_CONNECTION_INTENTS,
        QQBOT_CHANNEL_ONLY_LONG_CONNECTION_INTENTS,
    ]
}

pub fn should_try_lower_intent_after_error(error: &str) -> bool {
    let normalized = error.trim().to_ascii_lowercase();
    !normalized.is_empty()
        && (normalized.contains("invalid session")
            || normalized.contains("4013")
            || normalized.contains("4014")
            || normalized.contains("4912")
            || normalized.contains("4913"))
}

pub fn dispatch_event_type(payload: &Value) -> Option<String> {
    payload
        .get("t")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn is_supported_dispatch_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        QQBOT_EVENT_C2C_MESSAGE_CREATE
            | QQBOT_EVENT_AT_MESSAGE_CREATE
            | QQBOT_EVENT_DIRECT_MESSAGE_CREATE
            | QQBOT_EVENT_GROUP_AT_MESSAGE_CREATE
    )
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

fn trimmed_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

fn parse_compound_token(token: &str) -> Option<QqBotCredentials> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (app_id, client_secret) = trimmed.split_once(':')?;
    let app_id = app_id.trim();
    let client_secret = client_secret.trim();
    if app_id.is_empty() || client_secret.is_empty() {
        return None;
    }
    Some(QqBotCredentials {
        app_id: app_id.to_string(),
        client_secret: client_secret.to_string(),
    })
}

pub fn resolve_credentials(config: &QqBotConfig) -> Result<QqBotCredentials> {
    let explicit_app_id = trimmed_non_empty(config.app_id.as_deref());
    let explicit_secret = trimmed_non_empty(config.client_secret.as_deref());
    let token_credential = config
        .token
        .as_deref()
        .and_then(parse_compound_token)
        .unwrap_or(QqBotCredentials {
            app_id: String::new(),
            client_secret: String::new(),
        });

    let app_id = explicit_app_id
        .or_else(|| {
            (!token_credential.app_id.is_empty()).then_some(token_credential.app_id.clone())
        })
        .ok_or_else(|| anyhow!("qqbot app_id missing"))?;
    let client_secret = explicit_secret
        .or_else(|| {
            (!token_credential.client_secret.is_empty())
                .then_some(token_credential.client_secret.clone())
        })
        .ok_or_else(|| anyhow!("qqbot client_secret missing"))?;

    Ok(QqBotCredentials {
        app_id,
        client_secret,
    })
}

pub async fn fetch_access_token(http: &Client, config: &QqBotConfig) -> Result<String> {
    let credential = resolve_credentials(config)?;
    let token_resp = http
        .post(QQ_TOKEN_URL)
        .json(&json!({
            "appId": credential.app_id,
            "clientSecret": credential.client_secret,
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
    token_payload
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("qqbot token missing access_token"))
}

pub async fn fetch_gateway_url(http: &Client, access_token: &str) -> Result<String> {
    let response = http
        .get(QQ_GATEWAY_URL)
        .header("Authorization", format!("QQBot {access_token}"))
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("qqbot gateway failed: {status} {body}"));
    }
    let payload: Value = response.json().await?;
    payload
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("qqbot gateway missing url"))
}

async fn send_ws_json<S>(socket: &mut S, payload: Value) -> Result<()>
where
    S: futures::Sink<WsMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    socket
        .send(WsMessage::Text(payload.to_string().into()))
        .await?;
    Ok(())
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
    let credential = resolve_credentials(config)?;
    let app_id = credential.app_id.as_str();
    let access_token = fetch_access_token(http, config).await?;

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
            access_token.as_str(),
            &format!("{QQ_API_BASE}/v2/groups/{}/messages", outbound.peer.id),
            payload,
        )
        .await
    } else if peer_kind == "channel" {
        post_message(
            http,
            app_id,
            access_token.as_str(),
            &format!("{QQ_API_BASE}/channels/{}/messages", outbound.peer.id),
            json!({ "content": text }),
        )
        .await
    } else {
        post_message(
            http,
            app_id,
            access_token.as_str(),
            &format!("{QQ_API_BASE}/v2/users/{}/messages", outbound.peer.id),
            payload,
        )
        .await
    }
}

fn resolve_sender_id(message: &Value) -> Option<String> {
    message
        .get("author")
        .and_then(|value| {
            value
                .get("member_openid")
                .or_else(|| value.get("user_openid"))
                .or_else(|| value.get("id"))
                .or_else(|| value.get("openid"))
                .or_else(|| value.get("union_openid"))
        })
        .and_then(value_to_trimmed_string)
        .or_else(|| message.get("author_id").and_then(value_to_trimmed_string))
}

fn resolve_peer(event_type: &str, message: &Value, sender_id: &str) -> (&'static str, String) {
    let group_openid = message
        .get("group_openid")
        .or_else(|| message.get("group_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let channel_id = message
        .get("channel_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if event_type == QQBOT_EVENT_GROUP_AT_MESSAGE_CREATE {
        return ("group", group_openid.unwrap_or_default());
    }
    if event_type == QQBOT_EVENT_AT_MESSAGE_CREATE {
        return ("channel", channel_id.unwrap_or_default());
    }
    if !sender_id.trim().is_empty() {
        return ("user", sender_id.trim().to_string());
    }
    if let Some(group_openid) = group_openid {
        return ("group", group_openid);
    }
    if let Some(channel_id) = channel_id {
        return ("channel", channel_id);
    }
    ("user", String::new())
}

fn extract_attachments(message: &Value) -> Vec<ChannelAttachment> {
    let Some(items) = message.get("attachments").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut attachments = Vec::new();
    for item in items {
        let Some(url) = item
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let kind = item
            .get("content_type")
            .or_else(|| item.get("type"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("file")
            .to_ascii_lowercase();
        attachments.push(ChannelAttachment {
            kind,
            url: url.to_string(),
            mime: item
                .get("content_type")
                .and_then(Value::as_str)
                .map(str::to_string),
            size: item.get("size").and_then(Value::as_i64),
            name: item
                .get("filename")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    attachments
}

pub fn extract_dispatch_messages(payload: &Value, account_id: &str) -> Result<Vec<ChannelMessage>> {
    if !is_dispatch_event(payload) {
        return Ok(Vec::new());
    }
    let event_type = dispatch_event_type(payload).unwrap_or_default();
    if !is_supported_dispatch_event_type(event_type.as_str()) {
        return Ok(Vec::new());
    }

    let message = inbound_message_payload(payload);
    let sender_id = resolve_sender_id(message).unwrap_or_default();
    let (peer_kind, peer_id) = resolve_peer(event_type.as_str(), message, sender_id.as_str());
    if peer_id.is_empty() {
        return Err(anyhow!("invalid qqbot payload: missing peer id"));
    }

    let attachments = extract_attachments(message);
    let message_type = if attachments.is_empty() {
        "text".to_string()
    } else {
        "mixed".to_string()
    };
    let sender_name = message
        .get("author")
        .and_then(|value| value.get("username").or_else(|| value.get("nickname")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let ts = message
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis() as f64 / 1000.0);
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();

    Ok(vec![ChannelMessage {
        channel: QQBOT_CHANNEL.to_string(),
        account_id: account_id.to_string(),
        peer: ChannelPeer {
            kind: peer_kind.to_string(),
            id: peer_id,
            name: None,
        },
        thread: None,
        message_id: message
            .get("id")
            .or_else(|| message.get("msg_id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        sender: if sender_id.is_empty() {
            None
        } else {
            Some(ChannelSender {
                id: sender_id,
                name: sender_name,
            })
        },
        message_type,
        text: Some(content),
        attachments,
        location: None,
        ts,
        meta: Some(payload.clone()),
    }])
}

pub async fn run_long_connection_session_with_intents<F, Fut>(
    http: &Client,
    config: &QqBotConfig,
    intents: u64,
    mut on_dispatch: F,
) -> Result<()>
where
    F: FnMut(Value) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let access_token = fetch_access_token(http, config).await?;
    let gateway_url = fetch_gateway_url(http, access_token.as_str()).await?;
    let (mut socket, _) = connect_async(gateway_url.as_str())
        .await
        .map_err(|err| anyhow!("qqbot long connection connect failed: {err}"))?;

    let mut seq: Option<i64> = None;
    let mut heartbeat_ticker = interval(Duration::from_millis(QQBOT_DEFAULT_HEARTBEAT_INTERVAL_MS));
    heartbeat_ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut heartbeat_enabled = false;

    loop {
        tokio::select! {
            _ = heartbeat_ticker.tick(), if heartbeat_enabled => {
                let heartbeat = json!({
                    "op": QQBOT_CALLBACK_HEARTBEAT_OP,
                    "d": seq
                });
                send_ws_json(&mut socket, heartbeat)
                    .await
                    .map_err(|err| anyhow!("qqbot long connection heartbeat failed: {err}"))?;
            }
            incoming = socket.next() => {
                let incoming = incoming.ok_or_else(|| anyhow!("qqbot long connection closed"))?;
                let message = incoming
                    .map_err(|err| anyhow!("qqbot long connection recv failed: {err}"))?;
                match message {
                    WsMessage::Text(buffer) => {
                        let payload: Value = serde_json::from_str(buffer.as_ref())
                            .map_err(|err| anyhow!("qqbot long connection payload invalid json: {err}"))?;
                        if let Some(next_seq) = payload.get("s").and_then(Value::as_i64) {
                            seq = Some(next_seq);
                        }
                        match callback_opcode(&payload) {
                            Some(QQBOT_GATEWAY_HELLO_OP) => {
                                let heartbeat_interval_ms = payload
                                    .get("d")
                                    .and_then(|value| value.get("heartbeat_interval"))
                                    .and_then(Value::as_u64)
                                    .unwrap_or(QQBOT_DEFAULT_HEARTBEAT_INTERVAL_MS)
                                    .max(QQBOT_MIN_HEARTBEAT_INTERVAL_MS);
                                heartbeat_ticker = interval(Duration::from_millis(heartbeat_interval_ms));
                                heartbeat_ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
                                heartbeat_enabled = true;
                                send_ws_json(
                                    &mut socket,
                                    json!({
                                        "op": QQBOT_GATEWAY_IDENTIFY_OP,
                                        "d": {
                                            "token": format!("QQBot {access_token}"),
                                            "intents": intents,
                                            "shard": [0, 1],
                                        }
                                    }),
                                )
                                .await
                                .map_err(|err| anyhow!("qqbot long connection identify failed: {err}"))?;
                            }
                            Some(QQBOT_CALLBACK_DISPATCH_EVENT_OP) => {
                                on_dispatch(payload).await?;
                            }
                            Some(QQBOT_CALLBACK_HEARTBEAT_OP) => {
                                let heartbeat = json!({
                                    "op": QQBOT_CALLBACK_HEARTBEAT_OP,
                                    "d": seq
                                });
                                send_ws_json(&mut socket, heartbeat)
                                    .await
                                    .map_err(|err| anyhow!("qqbot long connection heartbeat ack failed: {err}"))?;
                            }
                            Some(QQBOT_CALLBACK_HEARTBEAT_ACK_OP) => {}
                            Some(QQBOT_GATEWAY_RECONNECT_OP) => {
                                return Err(anyhow!("qqbot long connection reconnect requested"));
                            }
                            Some(QQBOT_GATEWAY_INVALID_SESSION_OP) => {
                                return Err(anyhow!("qqbot long connection invalid session"));
                            }
                            Some(QQBOT_GATEWAY_RESUME_OP) => {
                                return Err(anyhow!("qqbot long connection resume required"));
                            }
                            _ => {}
                        }
                    }
                    WsMessage::Binary(buffer) => {
                        let payload: Value = serde_json::from_slice(&buffer)
                            .map_err(|err| anyhow!("qqbot long connection payload invalid json: {err}"))?;
                        if callback_opcode(&payload) == Some(QQBOT_CALLBACK_DISPATCH_EVENT_OP) {
                            on_dispatch(payload).await?;
                        }
                    }
                    WsMessage::Ping(payload) => {
                        socket
                            .send(WsMessage::Pong(payload))
                            .await
                            .map_err(|err| anyhow!("qqbot long connection pong failed: {err}"))?;
                    }
                    WsMessage::Pong(_) => {}
                    WsMessage::Close(frame) => {
                        let reason = frame
                            .map(|item| {
                                if item.reason.is_empty() {
                                    item.code.to_string()
                                } else {
                                    format!("{} {}", item.code, item.reason)
                                }
                            })
                            .unwrap_or_else(|| "remote closed".to_string());
                        return Err(anyhow!("qqbot long connection closed: {reason}"));
                    }
                    _ => {}
                }
            }
        }
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

    #[test]
    fn resolve_credentials_supports_openclaw_style_token() {
        let cfg = QqBotConfig {
            token: Some("1903541657:secret-key".to_string()),
            ..QqBotConfig::default()
        };
        assert_eq!(
            resolve_credentials(&cfg).expect("token credentials"),
            QqBotCredentials {
                app_id: "1903541657".to_string(),
                client_secret: "secret-key".to_string(),
            }
        );
    }

    #[test]
    fn resolve_credentials_prefers_explicit_values() {
        let cfg = QqBotConfig {
            app_id: Some("explicit-app".to_string()),
            client_secret: Some("explicit-secret".to_string()),
            token: Some("token-app:token-secret".to_string()),
            ..QqBotConfig::default()
        };
        assert_eq!(
            resolve_credentials(&cfg).expect("explicit credentials"),
            QqBotCredentials {
                app_id: "explicit-app".to_string(),
                client_secret: "explicit-secret".to_string(),
            }
        );
    }

    #[test]
    fn long_connection_enabled_defaults_true() {
        let config = QqBotConfig::default();
        assert!(long_connection_enabled(&config));

        let disabled = QqBotConfig {
            long_connection_enabled: Some(false),
            ..QqBotConfig::default()
        };
        assert!(!long_connection_enabled(&disabled));
    }

    #[test]
    fn extract_dispatch_messages_supports_group_at_event() {
        let payload = json!({
            "op": 0,
            "t": "GROUP_AT_MESSAGE_CREATE",
            "d": {
                "id": "msg-1",
                "content": "hello group",
                "timestamp": "2026-03-17T10:00:00Z",
                "group_openid": "group-openid-1",
                "author": {
                    "member_openid": "member-1"
                }
            }
        });

        let messages = extract_dispatch_messages(&payload, "uacc_1").expect("dispatch parsed");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].peer.kind, "group");
        assert_eq!(messages[0].peer.id, "group-openid-1");
        assert_eq!(
            messages[0].sender.as_ref().map(|value| value.id.as_str()),
            Some("member-1")
        );
        assert_eq!(messages[0].text.as_deref(), Some("hello group"));
    }

    #[test]
    fn extract_dispatch_messages_returns_error_when_peer_missing() {
        let payload = json!({
            "op": 0,
            "t": "GROUP_AT_MESSAGE_CREATE",
            "d": {
                "id": "msg-2",
                "content": "hello group",
                "timestamp": "2026-03-17T10:00:00Z",
                "author": {}
            }
        });

        let err = extract_dispatch_messages(&payload, "uacc_1").expect_err("peer is required");
        assert!(err.to_string().contains("missing peer id"));
    }

    #[test]
    fn extract_dispatch_messages_ignores_non_message_dispatch_events() {
        let payload = json!({
            "op": 0,
            "t": "READY",
            "d": {
                "session_id": "session-1"
            }
        });

        let messages = extract_dispatch_messages(&payload, "uacc_1").expect("should ignore");
        assert!(messages.is_empty());
    }

    #[test]
    fn resolve_long_connection_intent_candidates_supports_fallback_levels() {
        let default_cfg = QqBotConfig::default();
        assert_eq!(
            resolve_long_connection_intent_candidates(&default_cfg),
            vec![
                QQBOT_DEFAULT_LONG_CONNECTION_INTENTS,
                QQBOT_GROUP_CHANNEL_LONG_CONNECTION_INTENTS,
                QQBOT_CHANNEL_ONLY_LONG_CONNECTION_INTENTS,
            ]
        );

        let custom_cfg = QqBotConfig {
            intents: Some(1234),
            ..QqBotConfig::default()
        };
        assert_eq!(
            resolve_long_connection_intent_candidates(&custom_cfg),
            vec![1234]
        );
    }

    #[test]
    fn should_try_lower_intent_after_error_detects_common_gateway_errors() {
        assert!(should_try_lower_intent_after_error(
            "qqbot long connection invalid session"
        ));
        assert!(should_try_lower_intent_after_error(
            "qqbot long connection closed: 4014 invalid intent"
        ));
        assert!(should_try_lower_intent_after_error(
            "qqbot long connection closed: 4913 internal error"
        ));
        assert!(!should_try_lower_intent_after_error(
            "qqbot long connection connect failed: timeout"
        ));
    }
}
