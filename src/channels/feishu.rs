use crate::channels::adapter::{ChannelAdapter, OutboundContext};
use crate::channels::types::{
    ChannelAttachment, ChannelMessage, ChannelOutboundMessage, ChannelPeer, ChannelSender,
    FeishuConfig,
};
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockDecryptMut, KeyIvInit};
use aes::Aes256;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::time::{Duration, Instant};
use tokio::time::{interval, MissedTickBehavior};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use tracing::warn;
use url::Url;

type Aes256CbcDec = cbc::Decryptor<Aes256>;

pub const FEISHU_CHANNEL: &str = "feishu";

const FEISHU_LONG_CONN_ENDPOINT_URI: &str = "/callback/ws/endpoint";
const FEISHU_LONG_CONN_DEFAULT_PING_INTERVAL_S: u64 = 120;
const FEISHU_LONG_CONN_DEFAULT_RECONNECT_INTERVAL_S: u64 = 120;
const FEISHU_LONG_CONN_FRAGMENT_TTL_S: u64 = 5;

const FEISHU_WS_METHOD_CONTROL: i32 = 0;
const FEISHU_WS_METHOD_DATA: i32 = 1;
const FEISHU_WS_TYPE_EVENT: &str = "event";
const FEISHU_WS_TYPE_PING: &str = "ping";
const FEISHU_WS_TYPE_PONG: &str = "pong";
const FEISHU_WS_HEADER_TYPE: &str = "type";
const FEISHU_WS_HEADER_MESSAGE_ID: &str = "message_id";
const FEISHU_WS_HEADER_SUM: &str = "sum";
const FEISHU_WS_HEADER_SEQ: &str = "seq";
const FEISHU_WS_HEADER_BIZ_RT: &str = "biz_rt";
const FEISHU_WS_SERVICE_ID_QUERY: &str = "service_id";

#[derive(Debug, Default)]
pub struct FeishuAdapter;

#[async_trait]
impl ChannelAdapter for FeishuAdapter {
    fn channel(&self) -> &'static str {
        FEISHU_CHANNEL
    }

    async fn send_outbound(&self, context: OutboundContext<'_>) -> Result<()> {
        let config = context
            .account_config
            .feishu
            .as_ref()
            .ok_or_else(|| anyhow!("feishu config missing"))?;
        let _ = send_outbound(context.http, context.outbound, config).await?;
        if !is_processing_ack_outbound(context.outbound) {
            if let Some(message_id) = extract_processing_ack_message_id_outbound(context.outbound) {
                if let Err(err) = delete_message(context.http, &message_id, config).await {
                    warn!(
                        "cleanup feishu processing ack failed: account_id={}, message_id={}, error={err}",
                        context.account.account_id, message_id
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FeishuLongConnectionEndpoint {
    pub url: String,
    pub service_id: i32,
    pub ping_interval_s: u64,
    pub reconnect_interval_s: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct FeishuLongConnectionClientConfigPayload {
    #[serde(default, rename = "ReconnectInterval")]
    reconnect_interval: Option<i64>,
    #[serde(default, rename = "PingInterval")]
    ping_interval: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct FeishuLongConnectionEndpointResponse {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<FeishuLongConnectionEndpointPayload>,
}

#[derive(Debug, Deserialize, Default)]
struct FeishuLongConnectionEndpointPayload {
    #[serde(default, rename = "URL")]
    url: Option<String>,
    #[serde(default, rename = "ClientConfig")]
    client_config: Option<FeishuLongConnectionClientConfigPayload>,
}

#[derive(Clone, PartialEq, ProstMessage)]
struct FeishuWsHeader {
    #[prost(string, tag = "1")]
    key: String,
    #[prost(string, tag = "2")]
    value: String,
}

#[derive(Clone, PartialEq, ProstMessage)]
struct FeishuWsFrame {
    #[prost(uint64, tag = "1")]
    seq_id: u64,
    #[prost(uint64, tag = "2")]
    log_id: u64,
    #[prost(int32, tag = "3")]
    service: i32,
    #[prost(int32, tag = "4")]
    method: i32,
    #[prost(message, repeated, tag = "5")]
    headers: Vec<FeishuWsHeader>,
    #[prost(string, tag = "6")]
    payload_encoding: String,
    #[prost(string, tag = "7")]
    payload_type: String,
    #[prost(bytes, tag = "8")]
    payload: Vec<u8>,
    #[prost(string, tag = "9")]
    log_id_new: String,
}

#[derive(Debug)]
struct FeishuPayloadChunks {
    expires_at: Instant,
    chunks: Vec<Option<Vec<u8>>>,
}

#[derive(Debug, Default)]
struct FeishuPayloadAssembler {
    chunks: HashMap<String, FeishuPayloadChunks>,
}

impl FeishuPayloadAssembler {
    fn merge(
        &mut self,
        message_id: &str,
        sum: usize,
        seq: usize,
        payload: Vec<u8>,
    ) -> Option<Vec<u8>> {
        if message_id.trim().is_empty() || sum <= 1 || seq >= sum {
            return Some(payload);
        }
        self.prune_expired();
        let expires_at = Instant::now() + Duration::from_secs(FEISHU_LONG_CONN_FRAGMENT_TTL_S);
        let entry = self
            .chunks
            .entry(message_id.to_string())
            .or_insert_with(|| FeishuPayloadChunks {
                expires_at,
                chunks: vec![None; sum],
            });
        if entry.chunks.len() != sum {
            entry.chunks = vec![None; sum];
        }
        entry.expires_at = expires_at;
        entry.chunks[seq] = Some(payload);

        if entry.chunks.iter().any(Option::is_none) {
            return None;
        }

        let mut output = Vec::new();
        for part in &mut entry.chunks {
            if let Some(buffer) = part.take() {
                output.extend(buffer);
            }
        }
        self.chunks.remove(message_id);
        Some(output)
    }

    fn prune_expired(&mut self) {
        let now = Instant::now();
        self.chunks.retain(|_, item| item.expires_at > now);
    }
}

pub fn long_connection_enabled(config: &FeishuConfig) -> bool {
    config.long_connection_enabled.unwrap_or(true)
}

pub fn has_long_connection_credentials(config: &FeishuConfig) -> bool {
    app_credentials(config).is_ok()
}

pub fn is_message_event(payload: &Value) -> bool {
    payload
        .get("event")
        .and_then(|event| event.get("message"))
        .is_some()
}

pub async fn fetch_long_connection_endpoint(
    http: &Client,
    config: &FeishuConfig,
) -> Result<FeishuLongConnectionEndpoint> {
    let (app_id, app_secret) = app_credentials(config)?;
    let endpoint_url = format!(
        "{}{FEISHU_LONG_CONN_ENDPOINT_URI}",
        resolve_openapi_base_url(config)
    );
    let response = http
        .post(endpoint_url)
        .header("locale", "zh")
        .json(&json!({
            "AppID": app_id,
            "AppSecret": app_secret,
        }))
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "feishu long connection endpoint failed: {status} {body}"
        ));
    }

    let payload: FeishuLongConnectionEndpointResponse = response.json().await?;
    if payload.code != 0 {
        let message = payload.msg.as_deref().unwrap_or("unknown");
        return Err(anyhow!("feishu long connection endpoint failed: {message}"));
    }

    let endpoint = payload
        .data
        .ok_or_else(|| anyhow!("feishu long connection endpoint missing data"))?;
    let url = endpoint
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("feishu long connection endpoint missing url"))?
        .to_string();
    let service_id = parse_service_id(&url)?;
    let client_config = endpoint.client_config.unwrap_or_default();

    Ok(FeishuLongConnectionEndpoint {
        url,
        service_id,
        ping_interval_s: parse_positive_i64(
            client_config.ping_interval,
            FEISHU_LONG_CONN_DEFAULT_PING_INTERVAL_S,
        ),
        reconnect_interval_s: parse_positive_i64(
            client_config.reconnect_interval,
            FEISHU_LONG_CONN_DEFAULT_RECONNECT_INTERVAL_S,
        ),
    })
}

pub async fn run_long_connection_session<F, Fut>(
    http: &Client,
    config: &FeishuConfig,
    mut on_event: F,
) -> Result<FeishuLongConnectionEndpoint>
where
    F: FnMut(Value) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let endpoint = fetch_long_connection_endpoint(http, config).await?;
    let (mut socket, _) = connect_async(endpoint.url.as_str())
        .await
        .map_err(|err| anyhow!("feishu long connection connect failed: {err}"))?;

    let mut payload_assembler = FeishuPayloadAssembler::default();
    let mut ping_interval_s = endpoint.ping_interval_s.max(1);
    let mut ticker = interval(Duration::from_secs(ping_interval_s));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let ping_frame = build_ping_frame(endpoint.service_id);
                socket
                    .send(WsMessage::Binary(encode_ws_frame(&ping_frame)?))
                    .await
                    .map_err(|err| anyhow!("feishu long connection ping failed: {err}"))?;
            }
            incoming = socket.next() => {
                let incoming = incoming.ok_or_else(|| anyhow!("feishu long connection closed"))?;
                let message = incoming
                    .map_err(|err| anyhow!("feishu long connection recv failed: {err}"))?;
                match message {
                    WsMessage::Binary(buffer) => {
                        let mut frame = decode_ws_frame(&buffer)?;
                        if frame.method == FEISHU_WS_METHOD_CONTROL {
                            let control_type = header_value(&frame.headers, FEISHU_WS_HEADER_TYPE)
                                .map(|value| value.to_ascii_lowercase())
                                .unwrap_or_default();
                            if control_type == FEISHU_WS_TYPE_PONG {
                                if let Some(next_ping_interval_s) = resolve_pong_ping_interval(&frame.payload) {
                                    if next_ping_interval_s != ping_interval_s {
                                        ping_interval_s = next_ping_interval_s;
                                        ticker = interval(Duration::from_secs(ping_interval_s));
                                        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
                                    }
                                }
                            }
                            continue;
                        }
                        if frame.method != FEISHU_WS_METHOD_DATA {
                            continue;
                        }

                        let message_type = header_value(&frame.headers, FEISHU_WS_HEADER_TYPE)
                            .map(|value| value.to_ascii_lowercase())
                            .unwrap_or_default();
                        if message_type == FEISHU_WS_TYPE_PING {
                            continue;
                        }
                        if message_type != FEISHU_WS_TYPE_EVENT {
                            continue;
                        }

                        let sum = parse_header_usize(&frame.headers, FEISHU_WS_HEADER_SUM).unwrap_or(1);
                        let seq = parse_header_usize(&frame.headers, FEISHU_WS_HEADER_SEQ).unwrap_or(0);
                        let message_id = header_value(&frame.headers, FEISHU_WS_HEADER_MESSAGE_ID)
                            .unwrap_or_default()
                            .to_string();
                        let payload = if sum > 1 {
                            payload_assembler.merge(
                                &message_id,
                                sum,
                                seq,
                                std::mem::take(&mut frame.payload),
                            )
                        } else {
                            Some(std::mem::take(&mut frame.payload))
                        };
                        let Some(payload) = payload else {
                            continue;
                        };

                        let started = Instant::now();
                        let status_code = match serde_json::from_slice::<Value>(&payload) {
                            Ok(payload_json) => match on_event(payload_json).await {
                                Ok(()) => 200,
                                Err(err) => {
                                    warn!(
                                        "feishu long connection event handler failed: message_id={}, error={err}",
                                        message_id
                                    );
                                    500
                                }
                            },
                            Err(err) => {
                                warn!(
                                    "feishu long connection event payload invalid json: message_id={}, error={err}",
                                    message_id
                                );
                                400
                            }
                        };

                        append_header(
                            &mut frame.headers,
                            FEISHU_WS_HEADER_BIZ_RT,
                            started.elapsed().as_millis().to_string(),
                        );
                        frame.payload = serde_json::to_vec(&json!({ "code": status_code }))?;
                        socket
                            .send(WsMessage::Binary(encode_ws_frame(&frame)?))
                            .await
                            .map_err(|err| anyhow!("feishu long connection ack failed: {err}"))?;
                    }
                    WsMessage::Ping(payload) => {
                        socket
                            .send(WsMessage::Pong(payload))
                            .await
                            .map_err(|err| anyhow!("feishu long connection pong failed: {err}"))?;
                    }
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
                        return Err(anyhow!("feishu long connection closed: {reason}"));
                    }
                    WsMessage::Text(_) | WsMessage::Pong(_) => {}
                    _ => {}
                }
            }
        }
    }
}

fn resolve_pong_ping_interval(payload: &[u8]) -> Option<u64> {
    if payload.is_empty() {
        return None;
    }
    let client_config =
        serde_json::from_slice::<FeishuLongConnectionClientConfigPayload>(payload).ok()?;
    Some(parse_positive_i64(
        client_config.ping_interval,
        FEISHU_LONG_CONN_DEFAULT_PING_INTERVAL_S,
    ))
}

fn build_ping_frame(service_id: i32) -> FeishuWsFrame {
    FeishuWsFrame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: FEISHU_WS_METHOD_CONTROL,
        headers: vec![FeishuWsHeader {
            key: FEISHU_WS_HEADER_TYPE.to_string(),
            value: FEISHU_WS_TYPE_PING.to_string(),
        }],
        payload_encoding: String::new(),
        payload_type: String::new(),
        payload: Vec::new(),
        log_id_new: String::new(),
    }
}

fn decode_ws_frame(bytes: &[u8]) -> Result<FeishuWsFrame> {
    FeishuWsFrame::decode(bytes).map_err(|err| anyhow!("invalid feishu ws frame: {err}"))
}

fn encode_ws_frame(frame: &FeishuWsFrame) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    frame
        .encode(&mut buffer)
        .map_err(|err| anyhow!("encode feishu ws frame failed: {err}"))?;
    Ok(buffer)
}

fn append_header(headers: &mut Vec<FeishuWsHeader>, key: &str, value: String) {
    headers.push(FeishuWsHeader {
        key: key.to_string(),
        value,
    });
}

fn header_value<'a>(headers: &'a [FeishuWsHeader], key: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|header| header.key.eq_ignore_ascii_case(key))
        .map(|header| header.value.as_str())
}

fn parse_header_usize(headers: &[FeishuWsHeader], key: &str) -> Option<usize> {
    header_value(headers, key)
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0 || key.eq_ignore_ascii_case(FEISHU_WS_HEADER_SEQ))
}

fn parse_service_id(url: &str) -> Result<i32> {
    let parsed = Url::parse(url).with_context(|| format!("invalid feishu ws url: {url}"))?;
    parsed
        .query_pairs()
        .find(|(key, _)| key.eq_ignore_ascii_case(FEISHU_WS_SERVICE_ID_QUERY))
        .and_then(|(_, value)| value.trim().parse::<i32>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| anyhow!("feishu ws url missing service_id"))
}

fn parse_positive_i64(raw: Option<i64>, fallback: u64) -> u64 {
    raw.filter(|value| *value > 0)
        .map(|value| value as u64)
        .unwrap_or(fallback)
}

fn resolve_openapi_base_url(config: &FeishuConfig) -> String {
    let domain = config
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("open.feishu.cn");
    if domain.starts_with("http://") || domain.starts_with("https://") {
        domain.trim_end_matches('/').to_string()
    } else {
        format!("https://{}", domain.trim_end_matches('/'))
    }
}

fn app_credentials(config: &FeishuConfig) -> Result<(&str, &str)> {
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
    Ok((app_id, app_secret))
}

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

fn is_processing_ack_outbound(outbound: &ChannelOutboundMessage) -> bool {
    outbound
        .meta
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("processing_ack"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn extract_processing_ack_message_id_outbound(outbound: &ChannelOutboundMessage) -> Option<String> {
    outbound
        .meta
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("processing_ack_message_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, Default)]
pub struct FeishuSendResult {
    pub message_id: Option<String>,
}

pub async fn send_outbound(
    http: &Client,
    outbound: &ChannelOutboundMessage,
    config: &FeishuConfig,
) -> Result<FeishuSendResult> {
    let tenant_token = fetch_tenant_access_token(http, config).await?;
    let base_url = resolve_openapi_base_url(config);
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
    let send_url = format!("{base_url}/open-apis/im/v1/messages?receive_id_type={receive_id_type}");
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
    if !send_resp.status().is_success() {
        let status = send_resp.status();
        let body = send_resp.text().await.unwrap_or_default();
        return Err(anyhow!("feishu outbound failed: {status} {body}"));
    }
    let payload: Value = send_resp.json().await?;
    let code = payload.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 0 {
        let message = payload
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(anyhow!("feishu outbound failed: {message}"));
    }
    let message_id = payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("message_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok(FeishuSendResult { message_id })
}

pub async fn delete_message(http: &Client, message_id: &str, config: &FeishuConfig) -> Result<()> {
    let cleaned_message_id = message_id.trim();
    if cleaned_message_id.is_empty() {
        return Ok(());
    }
    let tenant_token = fetch_tenant_access_token(http, config).await?;
    let base_url = resolve_openapi_base_url(config);
    let delete_url = format!("{base_url}/open-apis/im/v1/messages/{cleaned_message_id}");
    let delete_resp = http
        .delete(delete_url)
        .bearer_auth(tenant_token)
        .send()
        .await?;
    if !delete_resp.status().is_success() {
        let status = delete_resp.status();
        let body = delete_resp.text().await.unwrap_or_default();
        return Err(anyhow!("feishu delete message failed: {status} {body}"));
    }
    let payload: Value = delete_resp.json().await?;
    let code = payload.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 0 {
        let message = payload
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(anyhow!("feishu delete message failed: {message}"));
    }
    Ok(())
}

async fn fetch_tenant_access_token(http: &Client, config: &FeishuConfig) -> Result<String> {
    let (app_id, app_secret) = app_credentials(config)?;
    let base_url = resolve_openapi_base_url(config);
    let token_url = format!("{base_url}/open-apis/auth/v3/tenant_access_token/internal");
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
    token_payload
        .get("tenant_access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("feishu token missing tenant_access_token"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn long_connection_enabled_defaults_to_true() {
        let config = FeishuConfig::default();
        assert!(long_connection_enabled(&config));
        let disabled = FeishuConfig {
            long_connection_enabled: Some(false),
            ..FeishuConfig::default()
        };
        assert!(!long_connection_enabled(&disabled));
    }

    #[test]
    fn parse_service_id_from_ws_url_works() {
        let service_id =
            parse_service_id("wss://open.feishu.cn/ws/abc?device_id=dev&service_id=12345")
                .expect("service_id should parse");
        assert_eq!(service_id, 12345);
    }

    #[test]
    fn payload_assembler_combines_fragments() {
        let mut assembler = FeishuPayloadAssembler::default();
        assert!(assembler
            .merge("mid", 2, 0, br#"{"text":"#.to_vec())
            .is_none());
        let output = assembler
            .merge("mid", 2, 1, br#""hello"}"#.to_vec())
            .expect("fragment payload should be merged");
        assert_eq!(output, br#"{"text":"hello"}"#.to_vec());
    }

    #[test]
    fn detect_message_event_payload() {
        let payload = json!({ "event": { "message": { "chat_id": "oc_1" } } });
        assert!(is_message_event(&payload));
        let payload = json!({ "event": { "sender": { "id": "u_1" } } });
        assert!(!is_message_event(&payload));
    }
}
