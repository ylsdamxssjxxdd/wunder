use crate::channels::ChannelMessage;
use crate::state::AppState;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct WebhookQuery {
    #[serde(default)]
    account_id: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/channel/{provider}/webhook", post(channel_webhook))
}

async fn channel_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(provider): AxumPath<String>,
    Query(query): Query<WebhookQuery>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    let provider = provider.trim().to_string();
    if provider.is_empty() {
        return Err(error_response(StatusCode::BAD_REQUEST, "missing provider"));
    }
    let account_override = query
        .account_id
        .clone()
        .or_else(|| header_string(&headers, "x-channel-account"));
    let raw_payload = Some(payload.clone());
    let messages = parse_channel_messages(payload)?;
    let messages = apply_overrides(&provider, account_override.as_deref(), messages);
    let result = state
        .channels
        .handle_inbound(&provider, &headers, messages, raw_payload)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
    Ok(Json(json!({ "data": {
        "accepted": result.accepted,
        "session_ids": result.session_ids,
        "outbox_ids": result.outbox_ids,
        "errors": result.errors,
    }})))
}

fn parse_channel_messages(payload: Value) -> Result<Vec<ChannelMessage>, Response> {
    if let Value::Array(items) = payload {
        let mut messages = Vec::new();
        for item in items {
            let msg: ChannelMessage = serde_json::from_value(item)
                .map_err(|_| error_response(StatusCode::BAD_REQUEST, "invalid channel message"))?;
            messages.push(msg);
        }
        return Ok(messages);
    }
    if let Value::Object(map) = &payload {
        if let Some(Value::Array(items)) = map.get("messages") {
            let mut messages = Vec::new();
            for item in items {
                let msg: ChannelMessage = serde_json::from_value(item.clone()).map_err(|_| {
                    error_response(StatusCode::BAD_REQUEST, "invalid channel message")
                })?;
                messages.push(msg);
            }
            return Ok(messages);
        }
    }
    let msg: ChannelMessage = serde_json::from_value(payload)
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "invalid channel message"))?;
    Ok(vec![msg])
}

fn apply_overrides(
    provider: &str,
    account_override: Option<&str>,
    mut messages: Vec<ChannelMessage>,
) -> Vec<ChannelMessage> {
    for message in &mut messages {
        if message.channel.trim().is_empty() {
            message.channel = provider.to_string();
        }
        if let Some(account) = account_override {
            if message.account_id.trim().is_empty() {
                message.account_id = account.to_string();
            }
        }
    }
    messages
}

fn header_string(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "detail": { "message": message } }))).into_response()
}
