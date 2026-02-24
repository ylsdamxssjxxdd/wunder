use crate::channels::feishu;
use crate::channels::qqbot;
use crate::channels::types::{ChannelAccountConfig, WechatConfig, WechatMpConfig};
use crate::channels::wechat;
use crate::channels::wechat_mp;
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
use tracing::warn;

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

#[derive(Debug, Deserialize)]
struct WechatVerifyQuery {
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default, alias = "msgsignature")]
    msg_signature: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    echostr: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WechatWebhookQuery {
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default, alias = "msgsignature")]
    msg_signature: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WechatMpVerifyQuery {
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default, alias = "msgsignature")]
    msg_signature: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    echostr: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WechatMpWebhookQuery {
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default, alias = "msgsignature")]
    msg_signature: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    encrypt_type: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/channel/whatsapp/webhook",
            get(whatsapp_webhook_verify).post(whatsapp_webhook),
        )
        .route("/wunder/channel/feishu/webhook", post(feishu_webhook))
        .route(
            "/wunder/channel/wechat/webhook",
            get(wechat_webhook_verify).post(wechat_webhook),
        )
        .route(
            "/wunder/channel/wechat_mp/webhook",
            get(wechat_mp_webhook_verify).post(wechat_mp_webhook),
        )
        .route("/wunder/channel/qqbot/webhook", post(qqbot_webhook))
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
    let configs = load_account_configs_by_channel(&state, whatsapp_cloud::WHATSAPP_CHANNEL).await?;
    let allowed = configs.iter().any(|config| {
        config
            .whatsapp_cloud
            .as_ref()
            .and_then(|cfg| cfg.verify_token.as_deref())
            .map(str::trim)
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
        return Ok(success_response(result));
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
    Ok(success_response(result))
}

async fn feishu_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WebhookQuery>,
    body: Bytes,
) -> Result<Json<Value>, Response> {
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "invalid json payload"))?;
    let account_id =
        resolve_channel_account_id(&headers, &query, feishu::FEISHU_CHANNEL, &payload).await?;
    let account =
        load_account_by_channel_and_id(&state, feishu::FEISHU_CHANNEL, &account_id).await?;
    let config = ChannelAccountConfig::from_value(&account.config);
    let feishu_cfg = config
        .feishu
        .as_ref()
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing feishu config"))?;

    if let Some(challenge) = feishu::challenge_response(&payload) {
        let verify_ok = feishu_cfg
            .verification_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|token| feishu::verify_challenge_token(&payload, token))
            .unwrap_or(true);
        if !verify_ok {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "feishu token mismatch",
            ));
        }
        return Ok(Json(challenge));
    }

    if let (Some(encrypt_key), Some(timestamp), Some(nonce), Some(sign)) = (
        feishu_cfg
            .encrypt_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        header_string(&headers, "x-lark-request-timestamp"),
        header_string(&headers, "x-lark-request-nonce"),
        header_string(&headers, "x-lark-signature"),
    ) {
        if !feishu::verify_sign(encrypt_key, &timestamp, &nonce, &body, &sign) {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "invalid feishu signature",
            ));
        }
    }

    let resolved_payload =
        feishu::decrypt_event_if_needed(payload.clone(), feishu_cfg.encrypt_key.as_deref())
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
    let messages = feishu::extract_inbound_messages(&resolved_payload, &account_id, Some("user"))
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;

    let result = state
        .channels
        .handle_inbound(
            feishu::FEISHU_CHANNEL,
            &headers,
            messages,
            Some(resolved_payload),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
    Ok(success_response(result))
}

async fn wechat_webhook_verify(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WechatVerifyQuery>,
) -> Result<Response, Response> {
    let signature = query
        .msg_signature
        .as_deref()
        .or(query.signature.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing signature"))?;
    let timestamp = query
        .timestamp
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing timestamp"))?;
    let nonce = query
        .nonce
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing nonce"))?;
    let echostr = query
        .echostr
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing echostr"))?;

    let account_hint = query.account_id.clone().or_else(|| {
        headers
            .get("x-channel-account")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    });
    let (_, wechat_cfg) = resolve_wechat_account(
        &state,
        account_hint.as_deref(),
        timestamp,
        nonce,
        Some(signature),
        Some(echostr),
        None,
    )
    .await?;
    let token = wechat_cfg
        .token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "wechat token missing"))?;
    if !wechat::verify_signature(token, timestamp, nonce, echostr, signature) {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "wechat signature mismatch",
        ));
    }
    let encoding_key = wechat_cfg
        .encoding_aes_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            error_response(StatusCode::BAD_REQUEST, "wechat encoding_aes_key missing")
        })?;
    let plain = wechat::decrypt_payload(echostr, encoding_key, wechat_cfg.corp_id.as_deref())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
    Ok((StatusCode::OK, plain).into_response())
}

async fn wechat_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WechatWebhookQuery>,
    body: Bytes,
) -> Result<Response, Response> {
    let raw_xml = std::str::from_utf8(&body)
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "wechat payload is not utf-8"))?
        .trim()
        .to_string();
    if raw_xml.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "empty wechat payload",
        ));
    }
    let outer_fields = wechat::parse_xml_fields(&raw_xml)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
    let encrypted = outer_fields
        .get("Encrypt")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let to_user_name = outer_fields
        .get("ToUserName")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let signature = query
        .msg_signature
        .as_deref()
        .or(query.signature.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing signature"))?;
    let timestamp = query
        .timestamp
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing timestamp"))?;
    let nonce = query
        .nonce
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing nonce"))?;
    let account_hint = query
        .account_id
        .clone()
        .or_else(|| header_string(&headers, "x-channel-account"));

    let (account_id, wechat_cfg) = resolve_wechat_account(
        &state,
        account_hint.as_deref(),
        timestamp,
        nonce,
        Some(signature),
        encrypted.as_deref(),
        to_user_name.as_deref(),
    )
    .await?;

    let xml_payload = if let Some(encrypted_text) = encrypted.as_deref() {
        let token = wechat_cfg
            .token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "wechat token missing"))?;
        if !wechat::verify_signature(token, timestamp, nonce, encrypted_text, signature) {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "wechat signature mismatch",
            ));
        }
        let encoding_key = wechat_cfg
            .encoding_aes_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                error_response(StatusCode::BAD_REQUEST, "wechat encoding_aes_key missing")
            })?;
        wechat::decrypt_payload(encrypted_text, encoding_key, wechat_cfg.corp_id.as_deref())
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?
    } else {
        let token = wechat_cfg
            .token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "wechat token missing"))?;
        if !wechat::verify_callback_signature(token, timestamp, nonce, signature) {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "wechat signature mismatch",
            ));
        }
        raw_xml.clone()
    };

    let messages = wechat::extract_inbound_messages(&xml_payload, &account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
    if messages.is_empty() {
        return Ok((StatusCode::OK, "success").into_response());
    }

    let payload = json!({
        "raw_xml": raw_xml,
        "payload_xml": xml_payload,
    });

    let async_state = state.clone();
    let async_headers = headers.clone();
    let async_account_id = account_id.clone();
    tokio::spawn(async move {
        if let Err(err) = async_state
            .channels
            .handle_inbound(
                wechat::WECHAT_CHANNEL,
                &async_headers,
                messages,
                Some(payload),
            )
            .await
        {
            warn!(
                "wechat async inbound handle failed: account_id={}, error={err}",
                async_account_id
            );
        }
    });

    Ok((StatusCode::OK, "success").into_response())
}

async fn wechat_mp_webhook_verify(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WechatMpVerifyQuery>,
) -> Result<Response, Response> {
    let signature = query
        .msg_signature
        .as_deref()
        .or(query.signature.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing signature"))?;
    let timestamp = query
        .timestamp
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing timestamp"))?;
    let nonce = query
        .nonce
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing nonce"))?;
    let echostr = query
        .echostr
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing echostr"))?;
    let account_hint = query
        .account_id
        .clone()
        .or_else(|| header_string(&headers, "x-channel-account"));
    let (_, wechat_mp_cfg) = resolve_wechat_mp_account(
        &state,
        account_hint.as_deref(),
        timestamp,
        nonce,
        Some(signature),
        Some(echostr),
        None,
    )
    .await?;
    let token = wechat_mp_cfg
        .token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "wechat mp token missing"))?;
    let verified = wechat_mp::verify_message_signature(token, timestamp, nonce, echostr, signature)
        || wechat_mp::verify_callback_signature(token, timestamp, nonce, signature);
    if !verified {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "wechat mp signature mismatch",
        ));
    }
    let decoded = if let Some(encoding_key) = wechat_mp_cfg
        .encoding_aes_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        wechat_mp::decrypt_payload(echostr, encoding_key, wechat_mp_cfg.app_id.as_deref())
            .unwrap_or_else(|_| echostr.to_string())
    } else {
        echostr.to_string()
    };
    Ok((StatusCode::OK, decoded).into_response())
}

async fn wechat_mp_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WechatMpWebhookQuery>,
    body: Bytes,
) -> Result<Response, Response> {
    let _encrypt_type = query
        .encrypt_type
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let raw_xml = std::str::from_utf8(&body)
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "wechat mp payload is not utf-8"))?
        .trim()
        .to_string();
    if raw_xml.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "empty wechat mp payload",
        ));
    }
    let outer_fields = wechat_mp::parse_xml_fields(&raw_xml)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
    let encrypted = outer_fields
        .get("Encrypt")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let to_user_name = outer_fields
        .get("ToUserName")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let signature = query
        .msg_signature
        .as_deref()
        .or(query.signature.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let timestamp = query
        .timestamp
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let nonce = query
        .nonce
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let account_hint = query
        .account_id
        .clone()
        .or_else(|| header_string(&headers, "x-channel-account"));

    let (account_id, wechat_mp_cfg) = resolve_wechat_mp_account(
        &state,
        account_hint.as_deref(),
        timestamp,
        nonce,
        signature,
        encrypted.as_deref(),
        to_user_name.as_deref(),
    )
    .await?;

    let xml_payload = if let Some(encrypted_text) = encrypted.as_deref() {
        let signature = signature
            .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing signature"))?;
        let token = wechat_mp_cfg
            .token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "wechat mp token missing"))?;
        if timestamp.is_empty() || nonce.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "missing timestamp or nonce",
            ));
        }
        if !wechat_mp::verify_message_signature(token, timestamp, nonce, encrypted_text, signature)
        {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "wechat mp signature mismatch",
            ));
        }
        let encoding_key = wechat_mp_cfg
            .encoding_aes_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "wechat mp encoding_aes_key missing",
                )
            })?;
        wechat_mp::decrypt_payload(
            encrypted_text,
            encoding_key,
            wechat_mp_cfg.app_id.as_deref(),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?
    } else {
        if let Some(signature) = signature {
            let token = wechat_mp_cfg
                .token
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    error_response(StatusCode::BAD_REQUEST, "wechat mp token missing")
                })?;
            if timestamp.is_empty() || nonce.is_empty() {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "missing timestamp or nonce",
                ));
            }
            if !wechat_mp::verify_callback_signature(token, timestamp, nonce, signature) {
                return Err(error_response(
                    StatusCode::UNAUTHORIZED,
                    "wechat mp signature mismatch",
                ));
            }
        }
        raw_xml.clone()
    };

    let messages = wechat_mp::extract_inbound_messages(&xml_payload, &account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
    if messages.is_empty() {
        return Ok((StatusCode::OK, "success").into_response());
    }

    let payload = json!({
        "raw_xml": raw_xml,
        "payload_xml": xml_payload,
    });
    let async_state = state.clone();
    let async_headers = headers.clone();
    let async_account_id = account_id.clone();
    tokio::spawn(async move {
        if let Err(err) = async_state
            .channels
            .handle_inbound(
                wechat_mp::WECHAT_MP_CHANNEL,
                &async_headers,
                messages,
                Some(payload),
            )
            .await
        {
            warn!(
                "wechat mp async inbound handle failed: account_id={}, error={err}",
                async_account_id
            );
        }
    });

    Ok((StatusCode::OK, "success").into_response())
}

async fn qqbot_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WebhookQuery>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Response> {
    let account_id =
        resolve_channel_account_id(&headers, &query, qqbot::QQBOT_CHANNEL, &payload).await?;
    let message = payload
        .get("message")
        .or_else(|| payload.get("d"))
        .unwrap_or(&payload)
        .clone();
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let message_id = message
        .get("id")
        .or_else(|| message.get("msg_id"))
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let sender_id = message
        .get("author")
        .and_then(|value| {
            value
                .get("member_openid")
                .or_else(|| value.get("id"))
                .or_else(|| value.get("user_openid"))
        })
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let group_openid = message
        .get("group_openid")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let peer_kind = if group_openid.is_some() {
        "group"
    } else {
        "user"
    };
    let peer_id = group_openid.clone().unwrap_or_else(|| sender_id.clone());
    if peer_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "invalid qqbot payload: missing peer id",
        ));
    }
    let ts = message
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis() as f64 / 1000.0);

    let normalized = ChannelMessage {
        channel: qqbot::QQBOT_CHANNEL.to_string(),
        account_id,
        peer: crate::channels::types::ChannelPeer {
            kind: peer_kind.to_string(),
            id: peer_id,
            name: None,
        },
        thread: None,
        message_id,
        sender: if sender_id.is_empty() {
            None
        } else {
            Some(crate::channels::types::ChannelSender {
                id: sender_id,
                name: message
                    .get("author")
                    .and_then(|value| value.get("username"))
                    .and_then(Value::as_str)
                    .map(|value| value.to_string()),
            })
        },
        message_type: "text".to_string(),
        text: Some(content),
        attachments: Vec::new(),
        location: None,
        ts,
        meta: Some(payload.clone()),
    };

    let result = state
        .channels
        .handle_inbound(
            qqbot::QQBOT_CHANNEL,
            &headers,
            vec![normalized],
            Some(payload),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, &err.to_string()))?;
    Ok(success_response(result))
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
    Ok(success_response(result))
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

fn success_response(result: crate::channels::service::ChannelHandleResult) -> Json<Value> {
    Json(json!({ "data": {
        "accepted": result.accepted,
        "session_ids": result.session_ids,
        "outbox_ids": result.outbox_ids,
        "errors": result.errors,
    }}))
}

fn header_string(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn error_response(status: StatusCode, message: &str) -> Response {
    crate::api::errors::error_response(status, message)
}

async fn load_account_configs_by_channel(
    state: &Arc<AppState>,
    channel: &str,
) -> Result<Vec<ChannelAccountConfig>, Response> {
    let records = load_account_records_by_channel(state, channel).await?;
    Ok(records
        .into_iter()
        .map(|record| ChannelAccountConfig::from_value(&record.config))
        .collect())
}

async fn load_account_records_by_channel(
    state: &Arc<AppState>,
    channel: &str,
) -> Result<Vec<crate::storage::ChannelAccountRecord>, Response> {
    let storage = state.storage.clone();
    let channel = channel.to_string();
    tokio::task::spawn_blocking(move || {
        storage.list_channel_accounts(Some(channel.as_str()), Some("active"))
    })
    .await
    .map_err(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "storage error"))?
    .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()))
}

async fn resolve_wechat_account(
    state: &Arc<AppState>,
    account_hint: Option<&str>,
    timestamp: &str,
    nonce: &str,
    signature: Option<&str>,
    encrypted: Option<&str>,
    to_user_name: Option<&str>,
) -> Result<(String, WechatConfig), Response> {
    if let Some(account_id) = account_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let record =
            load_account_by_channel_and_id(state, wechat::WECHAT_CHANNEL, account_id).await?;
        let account_cfg = ChannelAccountConfig::from_value(&record.config);
        let wechat_cfg = account_cfg
            .wechat
            .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing wechat config"))?;
        return Ok((record.account_id, wechat_cfg));
    }

    let records = load_account_records_by_channel(state, wechat::WECHAT_CHANNEL).await?;
    if records.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "channel account not found",
        ));
    }

    let signature = signature.map(str::trim).filter(|value| !value.is_empty());
    let encrypted = encrypted.map(str::trim).filter(|value| !value.is_empty());
    let timestamp = timestamp.trim();
    let nonce = nonce.trim();
    if let Some(signature) = signature {
        if !timestamp.is_empty() && !nonce.is_empty() {
            let mut signature_matches: Vec<(String, WechatConfig)> = Vec::new();
            for record in &records {
                let account_cfg = ChannelAccountConfig::from_value(&record.config);
                let Some(wechat_cfg) = account_cfg.wechat else {
                    continue;
                };
                let token = wechat_cfg
                    .token
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let matched = token
                    .map(|token| {
                        if let Some(encrypted) = encrypted {
                            wechat::verify_signature(token, timestamp, nonce, encrypted, signature)
                        } else {
                            wechat::verify_callback_signature(token, timestamp, nonce, signature)
                        }
                    })
                    .unwrap_or(false);
                if matched {
                    signature_matches.push((record.account_id.clone(), wechat_cfg));
                }
            }
            if signature_matches.is_empty() {
                return Err(error_response(
                    StatusCode::UNAUTHORIZED,
                    "wechat signature mismatch",
                ));
            }
            if let [item] = signature_matches.as_slice() {
                return Ok(item.clone());
            }
            if signature_matches.len() > 1 {
                if let Some(encrypted) = encrypted {
                    if let Some(selected) =
                        select_wechat_account_by_agent_id(&signature_matches, encrypted)
                    {
                        return Ok(selected);
                    }
                }
                if let Some(to_user_name) = to_user_name
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    if let Some(selected) =
                        select_wechat_account_by_to_user_name(&signature_matches, to_user_name)
                    {
                        return Ok(selected);
                    }
                }
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "multiple wechat accounts matched signature",
                ));
            }
        }
    }

    if let Some(to_user_name) = to_user_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        for record in &records {
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let Some(wechat_cfg) = account_cfg.wechat else {
                continue;
            };
            let corp_id = wechat_cfg
                .corp_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if corp_id
                .map(|corp_id| corp_id.eq_ignore_ascii_case(to_user_name))
                .unwrap_or(false)
            {
                return Ok((record.account_id.clone(), wechat_cfg));
            }
        }
    }

    if records.len() == 1 {
        let record = records.into_iter().next().expect("single record");
        let account_cfg = ChannelAccountConfig::from_value(&record.config);
        let wechat_cfg = account_cfg
            .wechat
            .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing wechat config"))?;
        return Ok((record.account_id, wechat_cfg));
    }

    Err(error_response(
        StatusCode::BAD_REQUEST,
        "missing account_id",
    ))
}

fn select_wechat_account_by_to_user_name(
    candidates: &[(String, WechatConfig)],
    to_user_name: &str,
) -> Option<(String, WechatConfig)> {
    let mut matched = candidates.iter().filter(|(_, cfg)| {
        cfg.corp_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|corp_id| corp_id.eq_ignore_ascii_case(to_user_name))
            .unwrap_or(false)
    });
    let first = matched.next()?.clone();
    if matched.next().is_none() {
        return Some(first);
    }
    None
}

fn select_wechat_account_by_agent_id(
    candidates: &[(String, WechatConfig)],
    encrypted: &str,
) -> Option<(String, WechatConfig)> {
    let mut matched = candidates.iter().filter(|(_, cfg)| {
        let encoding_key = cfg
            .encoding_aes_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let Some(encoding_key) = encoding_key else {
            return false;
        };
        let xml = match wechat::decrypt_payload(encrypted, encoding_key, cfg.corp_id.as_deref()) {
            Ok(value) => value,
            Err(_) => return false,
        };
        let inbound_agent_id = parse_wechat_inbound_agent_id(&xml);
        let expected_agent_id = parse_wechat_config_agent_id(cfg);
        matches!(
            (inbound_agent_id, expected_agent_id),
            (Some(inbound), Some(expected)) if inbound == expected
        )
    });
    let first = matched.next()?.clone();
    if matched.next().is_none() {
        return Some(first);
    }
    None
}

fn parse_wechat_inbound_agent_id(xml_payload: &str) -> Option<i64> {
    let fields = wechat::parse_xml_fields(xml_payload).ok()?;
    fields
        .get("AgentID")
        .or_else(|| fields.get("AgentId"))
        .or_else(|| fields.get("agentid"))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<i64>().ok())
}

fn parse_wechat_config_agent_id(config: &WechatConfig) -> Option<i64> {
    config
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<i64>().ok())
}

async fn resolve_wechat_mp_account(
    state: &Arc<AppState>,
    account_hint: Option<&str>,
    timestamp: &str,
    nonce: &str,
    signature: Option<&str>,
    encrypted: Option<&str>,
    to_user_name: Option<&str>,
) -> Result<(String, WechatMpConfig), Response> {
    if let Some(account_id) = account_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let record =
            load_account_by_channel_and_id(state, wechat_mp::WECHAT_MP_CHANNEL, account_id).await?;
        let account_cfg = ChannelAccountConfig::from_value(&record.config);
        let wechat_mp_cfg = account_cfg
            .wechat_mp
            .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing wechat mp config"))?;
        return Ok((record.account_id, wechat_mp_cfg));
    }

    let records = load_account_records_by_channel(state, wechat_mp::WECHAT_MP_CHANNEL).await?;
    if records.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "channel account not found",
        ));
    }

    let signature = signature.map(str::trim).filter(|value| !value.is_empty());
    let encrypted = encrypted.map(str::trim).filter(|value| !value.is_empty());
    let timestamp = timestamp.trim();
    let nonce = nonce.trim();
    if let Some(signature) = signature {
        if !timestamp.is_empty() && !nonce.is_empty() {
            for record in &records {
                let account_cfg = ChannelAccountConfig::from_value(&record.config);
                let Some(wechat_mp_cfg) = account_cfg.wechat_mp else {
                    continue;
                };
                let token = wechat_mp_cfg
                    .token
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let valid = if let Some(token) = token {
                    let callback_ok =
                        wechat_mp::verify_callback_signature(token, timestamp, nonce, signature);
                    let message_ok = encrypted
                        .map(|encrypted| {
                            wechat_mp::verify_message_signature(
                                token, timestamp, nonce, encrypted, signature,
                            )
                        })
                        .unwrap_or(false);
                    callback_ok || message_ok
                } else {
                    false
                };
                if valid {
                    return Ok((record.account_id.clone(), wechat_mp_cfg));
                }
            }
        }
    }

    if let Some(to_user_name) = to_user_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        for record in &records {
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let Some(wechat_mp_cfg) = account_cfg.wechat_mp else {
                continue;
            };
            let matched = wechat_mp_cfg
                .original_id
                .as_deref()
                .or(wechat_mp_cfg.app_id.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.eq_ignore_ascii_case(to_user_name))
                .unwrap_or(false);
            if matched {
                return Ok((record.account_id.clone(), wechat_mp_cfg));
            }
        }
    }

    if records.len() == 1 {
        let record = records.into_iter().next().expect("single record");
        let account_cfg = ChannelAccountConfig::from_value(&record.config);
        let wechat_mp_cfg = account_cfg
            .wechat_mp
            .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "missing wechat mp config"))?;
        return Ok((record.account_id, wechat_mp_cfg));
    }

    Err(error_response(
        StatusCode::BAD_REQUEST,
        "missing account_id",
    ))
}

async fn load_account_by_channel_and_id(
    state: &Arc<AppState>,
    channel: &str,
    account_id: &str,
) -> Result<crate::storage::ChannelAccountRecord, Response> {
    let storage = state.storage.clone();
    let channel = channel.to_string();
    let account_id = account_id.to_string();
    let record =
        tokio::task::spawn_blocking(move || storage.get_channel_account(&channel, &account_id))
            .await
            .map_err(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "storage error"))?
            .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()))?;
    let record = record
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "channel account not found"))?;
    if !record.status.trim().eq_ignore_ascii_case("active") {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "channel account disabled",
        ));
    }
    Ok(record)
}

async fn resolve_channel_account_id(
    headers: &HeaderMap,
    query: &WebhookQuery,
    channel: &str,
    payload: &Value,
) -> Result<String, Response> {
    if let Some(value) = query
        .account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(value.to_string());
    }
    if let Some(value) = header_string(headers, "x-channel-account") {
        return Ok(value);
    }
    if channel.eq_ignore_ascii_case(feishu::FEISHU_CHANNEL) {
        if let Some(app_id) = payload
            .get("header")
            .and_then(|value| value.get("app_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(app_id.to_string());
        }
    }
    if channel.eq_ignore_ascii_case(qqbot::QQBOT_CHANNEL) {
        if let Some(app_id) = payload
            .get("app_id")
            .or_else(|| payload.get("appid"))
            .or_else(|| payload.get("d").and_then(|value| value.get("app_id")))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(app_id.to_string());
        }
    }
    Err(error_response(
        StatusCode::BAD_REQUEST,
        "missing account_id",
    ))
}

async fn load_whatsapp_app_secrets(state: &Arc<AppState>) -> Result<Vec<String>, Response> {
    let configs = load_account_configs_by_channel(state, whatsapp_cloud::WHATSAPP_CHANNEL).await?;
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
