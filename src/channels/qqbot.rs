use crate::channels::types::{ChannelOutboundMessage, QqBotConfig};
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};

pub const QQBOT_CHANNEL: &str = "qqbot";
const QQ_API_BASE: &str = "https://api.sgroup.qq.com";
const QQ_TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";

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
            access_token,
            &format!("{QQ_API_BASE}/v2/groups/{}/messages", outbound.peer.id),
            payload,
        )
        .await
    } else if peer_kind == "channel" {
        post_message(
            http,
            access_token,
            &format!("{QQ_API_BASE}/channels/{}/messages", outbound.peer.id),
            json!({ "content": text }),
        )
        .await
    } else {
        post_message(
            http,
            access_token,
            &format!("{QQ_API_BASE}/v2/users/{}/messages", outbound.peer.id),
            payload,
        )
        .await
    }
}

async fn post_message(http: &Client, access_token: &str, url: &str, payload: Value) -> Result<()> {
    let response = http
        .post(url)
        .header("Authorization", format!("QQBot {access_token}"))
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
