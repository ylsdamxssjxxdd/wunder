use crate::channels::types::ChannelAccountConfig;
use crate::channels::whatsapp_cloud;
use crate::channels::ChannelMessage;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct WebhookQuery {
    #[serde(default)]
    account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WhatsappVerifyQuery {
    #[serde(rename = "hub.mode")]
    hub_mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    hub_verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    hub_challenge: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/channel/whatsapp/webhook",
            get(whatsapp_webhook_verify).post(whatsapp_webhook),
        )
        .route("/wunder/channel/{provider}/webhook", post(channel_webhook))
}

async fn whatsapp_webhook_verify(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WhatsappVerifyQuery>,
) -> Result<Response, Response> {
    let mode = query
        .hub_mode
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if mode != "subscribe" {
        return Err(error_response(StatusCode::BAD_REQUEST, "invalid hub.mode"));
    }
    let token = query
        .hub_verify_token
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    if token.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "missing hub.verify_token",
        ));
    }
    let challenge = query
        .hub_challenge
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    if challenge.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "missing hub.challenge",
        ));
    }
    let configs = load_whatsapp_account_configs(&state).await?;
    let allowed = configs.iter().any(|config| {
        config
            .whatsapp_cloud
            .as_ref()
            .and_then(|cfg| cfg.verify_token.as_deref())
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            == Some(token.as_str())
    });
    if !allowed {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "verify token mismatch",
        ));
    }
    Ok((StatusCode::OK, challenge).into_response())
}

async fn whatsapp_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WebhookQuery>,
    body: Bytes,
) -> Result<Json<Value>, Response> {
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "invalid json payload"))?;
    let account_override = query
        .account_id
        .clone()
        .or_else(|| header_string(&headers, "x-channel-account"));

    if whatsapp_cloud::is_whatsapp_cloud_payload(&payload) {
        let secrets = load_whatsapp_app_secrets(&state).await?;
        if !secrets.is_empty() {
            let signature = headers
                .get("x-hub-signature-256")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .trim()
                .to_string();
            if signature.is_empty()
                || !whatsapp_cloud::verify_signature_any(&body, &signature, &secrets)
            {
                return Err(error_response(
                    StatusCode::UNAUTHORIZED,
                    "invalid signature",
                ));
            }
        }

        let inbound =
            whatsapp_cloud::extract_inbound_messages(&payload, account_override.as_deref())
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
        let mut messages = Vec::new();
        for item in inbound {
            messages.push(whatsapp_cloud::inbound_to_channel_message(item, Vec::new()));
        }
        let result = state
            .channels
            .handle_inbound(
                whatsapp_cloud::WHATSAPP_CHANNEL,
                &headers,
                messages,
                Some(payload.clone()),
            )
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
        return Ok(Json(json!({ "data": {
            "accepted": result.accepted,
            "session_ids": result.session_ids,
            "outbox_ids": result.outbox_ids,
            "errors": result.errors,
        }})));
    }

    let messages = parse_channel_messages(payload.clone())?;
    let messages = apply_overrides(
        whatsapp_cloud::WHATSAPP_CHANNEL,
        account_override.as_deref(),
        messages,
    );
    let result = state
        .channels
        .handle_inbound(
            whatsapp_cloud::WHATSAPP_CHANNEL,
            &headers,
            messages,
            Some(payload),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
    Ok(Json(json!({ "data": {
        "accepted": result.accepted,
        "session_ids": result.session_ids,
        "outbox_ids": result.outbox_ids,
        "errors": result.errors,
    }})))
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

async fn load_whatsapp_account_configs(
    state: &Arc<AppState>,
) -> Result<Vec<ChannelAccountConfig>, Response> {
    let storage = state.storage.clone();
    let records = tokio::task::spawn_blocking(move || {
        storage.list_channel_accounts(Some(whatsapp_cloud::WHATSAPP_CHANNEL), Some("active"))
    })
    .await
    .map_err(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "storage error"))?
    .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()))?;
    Ok(records
        .into_iter()
        .map(|record| ChannelAccountConfig::from_value(&record.config))
        .collect())
}

async fn load_whatsapp_app_secrets(state: &Arc<AppState>) -> Result<Vec<String>, Response> {
    let configs = load_whatsapp_account_configs(state).await?;
    let mut secrets = Vec::new();
    for config in configs {
        if let Some(secret) = config
            .whatsapp_cloud
            .as_ref()
            .and_then(|cfg| cfg.app_secret.as_deref())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            secrets.push(secret);
        }
    }
    Ok(secrets)
}
