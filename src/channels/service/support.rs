use super::{
    ChannelCommand, CHANNEL_DISPLAY_QUESTION_OVERRIDE_KEY, CHANNEL_MODEL_ERROR_FALLBACK_TEXT,
    CHANNEL_OPEN_APPROVAL_FOR_TEST, DEFAULT_SESSION_TITLE, TOOL_OVERRIDE_NONE,
};
use crate::channels::binding::BindingResolution;
use crate::channels::rate_limit::RateLimitConfig;
use crate::channels::types::{ChannelAccountConfig, ChannelMessage, ChannelOutboundMessage};
use crate::channels::weixin;
use crate::config::{ChannelRateLimitConfig, Config};
use crate::core::approval::ApprovalResponse;
use crate::services::bridge::{append_bridge_meta, BridgeRouteResolution};
use crate::storage::{ChannelAccountRecord, ChannelBindingRecord, UserAgentRecord};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use axum::http::{HeaderMap, HeaderValue as AxumHeaderValue};
use reqwest::header::{HeaderMap as ReqHeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(super) fn channels_runtime_enabled(config: &Config) -> bool {
    config.channels.enabled || config.gateway.enabled
}

pub(super) fn channel_test_request_overrides() -> Option<Value> {
    if !CHANNEL_OPEN_APPROVAL_FOR_TEST {
        return None;
    }
    Some(json!({
        "security": {
            "approval_mode": "full_auto",
            "exec_policy_mode": "allow"
        }
    }))
}

pub(super) fn merge_channel_request_overrides(
    base: Option<Value>,
    display_question: Option<&str>,
) -> Option<Value> {
    let display_question = display_question
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let Some(display_question) = display_question else {
        return base;
    };
    match base {
        Some(Value::Object(mut map)) => {
            map.insert(
                CHANNEL_DISPLAY_QUESTION_OVERRIDE_KEY.to_string(),
                Value::String(display_question),
            );
            Some(Value::Object(map))
        }
        Some(value) => Some(value),
        None => Some(json!({
            CHANNEL_DISPLAY_QUESTION_OVERRIDE_KEY: display_question
        })),
    }
}

pub(super) fn build_bridge_session_metadata(resolution: &BridgeRouteResolution) -> Value {
    let mut meta = json!({});
    append_bridge_meta(&mut meta, resolution);
    if let Some(meta_obj) = meta.as_object_mut() {
        meta_obj.insert(
            "bridge_channel".to_string(),
            Value::String(resolution.center_account.channel.clone()),
        );
        meta_obj.insert(
            "bridge_account_id".to_string(),
            Value::String(resolution.center_account.account_id.clone()),
        );
    }
    meta
}

pub(super) fn extract_bridge_meta_ids(meta: Option<&Value>) -> Option<(String, String, String)> {
    let meta = meta?;
    let center_id = meta
        .get("bridge_center_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let center_account_id = meta
        .get("bridge_center_account_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let route_id = meta
        .get("bridge_route_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some((
        center_id.to_string(),
        center_account_id.to_string(),
        route_id.to_string(),
    ))
}

pub(super) fn merge_object_values(base: Option<Value>, overlay: Option<Value>) -> Option<Value> {
    match (base, overlay) {
        (None, None) => None,
        (Some(value), None) => Some(value),
        (None, Some(value)) => Some(value),
        (Some(mut value), Some(overlay)) => {
            merge_object_value_into(&mut value, overlay);
            Some(value)
        }
    }
}

pub(super) fn merge_object_value_into(target: &mut Value, extra: Value) {
    let Some(target_obj) = target.as_object_mut() else {
        return;
    };
    let Some(extra_obj) = extra.as_object() else {
        return;
    };
    for (key, value) in extra_obj {
        target_obj.insert(key.clone(), value.clone());
    }
}

pub(super) fn build_internal_channel_headers(inbound_token: Option<&str>) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    if let Some(token) = inbound_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let header_value = AxumHeaderValue::from_str(token)
            .map_err(|err| anyhow!("invalid inbound token header value: {err}"))?;
        headers.insert("x-channel-token", header_value);
    }
    Ok(headers)
}

pub(super) fn append_weixin_context_token_from_message(meta: &mut Value, message: &ChannelMessage) {
    if let Some(token) = weixin::extract_inbound_context_token(message) {
        append_weixin_context_token(meta, Some(token.as_str()));
    }
}

pub(super) fn append_weixin_context_token(meta: &mut Value, context_token: Option<&str>) {
    let Some(token) = context_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    if let Some(meta_obj) = meta.as_object_mut() {
        meta_obj.insert(
            "weixin_context_token".to_string(),
            Value::String(token.to_string()),
        );
    }
}

pub(super) fn resolve_weixin_workspace_public_source_to_local(
    workspace: &WorkspaceManager,
    source: &str,
) -> Option<String> {
    let trimmed = source.trim();
    let workspace_path = trimmed.strip_prefix("/workspaces/")?;
    let workspace_id = workspace_path
        .split('/')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let local = workspace.resolve_path(workspace_id, trimmed).ok()?;
    Some(local.to_string_lossy().replace('\\', "/"))
}

pub(super) fn normalize_message(provider: &str, message: &mut ChannelMessage) -> Result<()> {
    if message.channel.trim().is_empty() {
        message.channel = provider.trim().to_string();
    }
    if message.channel.trim().is_empty() {
        return Err(anyhow!("missing channel"));
    }
    if message.account_id.trim().is_empty() {
        return Err(anyhow!("missing account_id"));
    }
    if message.peer.id.trim().is_empty() {
        return Err(anyhow!("missing peer id"));
    }
    if message.peer.kind.trim().is_empty() {
        message.peer.kind = "dm".to_string();
    }
    if message.message_type.trim().is_empty() {
        message.message_type = if message.attachments.is_empty() {
            "text".to_string()
        } else {
            "mixed".to_string()
        };
    }
    Ok(())
}

pub(super) fn parse_channel_command(text: Option<&str>) -> Option<ChannelCommand> {
    let raw = text?.trim();
    if !raw.starts_with('/') {
        return None;
    }
    let token = raw
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('/');
    if token.is_empty() {
        return None;
    }
    match token.to_ascii_lowercase().as_str() {
        "new" | "reset" => Some(ChannelCommand::NewThread),
        "stop" | "cancel" => Some(ChannelCommand::Stop),
        "help" | "?" => Some(ChannelCommand::Help),
        _ => None,
    }
}

pub(super) fn extract_chat_content(payload: &Value) -> Option<String> {
    let content = payload.get("content").unwrap_or(payload);
    match content {
        Value::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.as_str() {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                    continue;
                }
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                    continue;
                }
                if let Some(text) = item.get("content").and_then(Value::as_str) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                }
            }
            if !parts.is_empty() {
                return Some(parts.join(""));
            }
            Some(content.to_string())
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            if let Some(text) = map.get("content").and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            Some(content.to_string())
        }
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

pub(super) fn message_preview_text(message: &ChannelMessage) -> String {
    if let Some(text) = message.text.as_deref().map(str::trim) {
        if !text.is_empty() {
            return truncate_text(text, 200);
        }
    }
    if !message.attachments.is_empty() {
        if let Some(kind) = message
            .attachments
            .first()
            .map(|item| item.kind.trim())
            .filter(|value| !value.is_empty())
        {
            return format!("[{kind}]");
        }
        return "[attachment]".to_string();
    }
    let message_type = message.message_type.trim();
    if !message_type.is_empty() {
        return format!("[{message_type}]");
    }
    "[empty message]".to_string()
}

pub(super) fn outbound_preview_text(outbound: &ChannelOutboundMessage) -> String {
    if let Some(text) = outbound.text.as_deref().map(str::trim) {
        if !text.is_empty() {
            return truncate_text(text, 200);
        }
    }
    if !outbound.attachments.is_empty() {
        if let Some(kind) = outbound
            .attachments
            .first()
            .map(|item| item.kind.trim())
            .filter(|value| !value.is_empty())
        {
            return format!("[{kind}]");
        }
        return "[attachment]".to_string();
    }
    "[empty message]".to_string()
}

pub(super) fn resolve_channel_agent_display_name(
    agent: Option<&UserAgentRecord>,
    agent_id: Option<&str>,
) -> String {
    if let Some(record) = agent {
        let name = record.name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    if let Some(agent_id) = agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    {
        return agent_id;
    }
    "智能体".to_string()
}

pub(super) fn resolve_agent_id_by_account(
    bindings: &[ChannelBindingRecord],
    message: &ChannelMessage,
) -> Option<String> {
    let channel = message.channel.trim();
    let account_id = message.account_id.trim();
    if channel.is_empty() || account_id.is_empty() {
        return None;
    }
    let mut resolved: Option<String> = None;
    for binding in bindings {
        if !binding.enabled {
            continue;
        }
        if !binding.channel.is_empty() && !binding.channel.trim().eq_ignore_ascii_case(channel) {
            continue;
        }
        if !binding.account_id.is_empty()
            && !binding.account_id.trim().eq_ignore_ascii_case(account_id)
        {
            continue;
        }
        let Some(agent_id) = binding
            .agent_id
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        match resolved.as_ref() {
            None => resolved = Some(agent_id.to_string()),
            Some(existing) => {
                if !existing.eq_ignore_ascii_case(agent_id) {
                    return None;
                }
            }
        }
    }
    resolved
}

pub(super) fn equivalent_peer_kinds(kind: &str) -> Vec<String> {
    let normalized = kind.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return vec![String::new()];
    }
    if is_direct_peer(&normalized) {
        return ["user", "dm", "direct", "single"]
            .into_iter()
            .map(str::to_string)
            .collect();
    }
    vec![normalized]
}

pub(super) fn is_direct_peer(kind: &str) -> bool {
    matches!(
        kind.trim().to_ascii_lowercase().as_str(),
        "dm" | "direct" | "single" | "user"
    )
}

pub(super) fn validate_inbound_account(
    headers: &HeaderMap,
    account: &ChannelAccountRecord,
    config: &ChannelAccountConfig,
) -> Result<()> {
    if account.status.trim().to_lowercase() != "active" {
        return Err(anyhow!("channel account disabled"));
    }
    let token = config
        .inbound_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(expected) = token {
        let provided = headers
            .get("x-channel-token")
            .and_then(|value| value.to_str().ok())
            .or_else(|| {
                headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
            })
            .unwrap_or("");
        let cleaned = provided.trim().trim_start_matches("Bearer ").trim();
        if cleaned != expected {
            return Err(anyhow!("invalid channel token"));
        }
    }
    Ok(())
}

pub(super) fn enforce_allowlist(
    message: &ChannelMessage,
    config: &ChannelAccountConfig,
) -> Result<()> {
    let peer_id = message.peer.id.trim().to_lowercase();
    let sender_id = message
        .sender
        .as_ref()
        .map(|sender| sender.id.trim().to_lowercase())
        .unwrap_or_default();
    let normalize = |items: &[String]| {
        items
            .iter()
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<HashSet<_>>()
    };
    let deny_peers = normalize(&config.deny_peers);
    if !deny_peers.is_empty() && deny_peers.contains(&peer_id) {
        return Err(anyhow!("peer blocked"));
    }
    let deny_senders = normalize(&config.deny_senders);
    if !deny_senders.is_empty() && deny_senders.contains(&sender_id) {
        return Err(anyhow!("sender blocked"));
    }
    let allow_peers = normalize(&config.allow_peers);
    if !allow_peers.is_empty() && !allow_peers.contains(&peer_id) {
        return Err(anyhow!("peer not allowed"));
    }
    let allow_senders = normalize(&config.allow_senders);
    if !allow_senders.is_empty() && !allow_senders.contains(&sender_id) {
        return Err(anyhow!("sender not allowed"));
    }
    Ok(())
}

pub(super) fn resolve_rate_limit(
    config: &ChannelRateLimitConfig,
    channel: &str,
) -> RateLimitConfig {
    let normalized = channel.trim().to_lowercase();
    let override_cfg = config.by_channel.get(&normalized);
    let qps = override_cfg
        .and_then(|value| value.qps)
        .unwrap_or(config.default_qps);
    let concurrency = override_cfg
        .and_then(|value| value.concurrency)
        .unwrap_or(config.default_concurrency);
    RateLimitConfig { qps, concurrency }
}

pub(super) fn resolve_tool_names(
    binding: Option<&BindingResolution>,
    account: &ChannelAccountConfig,
    agent: Option<&UserAgentRecord>,
    config: &Config,
) -> Vec<String> {
    let mut names = if let Some(binding) = binding {
        if !binding.tool_overrides.is_empty() {
            binding.tool_overrides.clone()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    if names.is_empty() && !account.tool_overrides.is_empty() {
        names = account.tool_overrides.clone();
    }
    if names.is_empty() {
        if let Some(agent) = agent {
            if !agent.tool_names.is_empty() {
                names = agent.tool_names.clone();
            }
        }
    }
    if names.is_empty() && !config.channels.default_tool_overrides.is_empty() {
        names = config.channels.default_tool_overrides.clone();
    }
    if names.is_empty() {
        return Vec::new();
    }
    normalize_tool_overrides(names)
}

pub(super) fn normalize_tool_overrides(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let mut has_none = false;
    for raw in values {
        let name = raw.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        if name == TOOL_OVERRIDE_NONE {
            has_none = true;
        }
        seen.insert(name.clone());
        output.push(name);
    }
    if has_none {
        vec![TOOL_OVERRIDE_NONE.to_string()]
    } else {
        output
    }
}

pub(super) fn resolve_channel_actor_id(message: &ChannelMessage) -> String {
    message
        .sender
        .as_ref()
        .and_then(|sender| {
            let value = sender.id.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        })
        .or_else(|| {
            let value = message.peer.id.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        })
        .unwrap_or_default()
}

pub(super) fn parse_channel_approval_decision(text: Option<&str>) -> Option<ApprovalResponse> {
    let raw = text?.trim();
    if raw.is_empty() {
        return None;
    }
    let token = raw
        .split_whitespace()
        .next()
        .map(|value| {
            value.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '.' | '。' | '、' | ')' | '）' | '(' | '（' | '[' | ']' | '【' | '】'
                )
            })
        })
        .unwrap_or(raw);
    let normalized = token.to_ascii_lowercase();
    match normalized.as_str() {
        "1" | "once" | "approve_once" | "approve-once" => Some(ApprovalResponse::ApproveOnce),
        "2" | "session" | "approve_session" | "approve-session" => {
            Some(ApprovalResponse::ApproveSession)
        }
        "3" | "deny" | "reject" => Some(ApprovalResponse::Deny),
        _ => match token {
            "同意一次" => Some(ApprovalResponse::ApproveOnce),
            "同意会话" | "同意本会话" => Some(ApprovalResponse::ApproveSession),
            "拒绝" | "不同意" => Some(ApprovalResponse::Deny),
            _ => None,
        },
    }
}

pub(super) fn normalize_optional_key(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .unwrap_or_default()
}

pub(super) fn build_outbound_headers(config: &ChannelAccountConfig) -> Result<ReqHeaderMap> {
    let mut headers = ReqHeaderMap::new();
    if let Some(Value::Object(map)) = config.outbound_headers.as_ref() {
        for (key, value) in map {
            let Some(text) = value.as_str() else {
                continue;
            };
            let name = HeaderName::from_bytes(key.as_bytes())?;
            let value = HeaderValue::from_str(text)?;
            headers.insert(name, value);
        }
    }
    if let Some(token) = config
        .outbound_token
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {token}").parse::<HeaderValue>()?,
        );
    }
    Ok(headers)
}

pub(super) fn extract_session_id(payload: &Value) -> Option<String> {
    payload
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("session_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn is_compacting_progress_event(
    event_name: &str,
    event_payload: &Value,
    raw_event_data: &Value,
) -> bool {
    if !event_name.eq_ignore_ascii_case("progress") {
        return false;
    }
    progress_stage_is_compacting(event_payload) || progress_stage_is_compacting(raw_event_data)
}

fn progress_stage_is_compacting(payload: &Value) -> bool {
    payload
        .get("stage")
        .and_then(Value::as_str)
        .map(|stage| stage.trim().eq_ignore_ascii_case("compacting"))
        .unwrap_or(false)
}

pub(super) fn truncate_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut output = text[..end].to_string();
    output.push_str("...");
    output
}

pub(super) fn should_auto_title(title: &str) -> bool {
    let cleaned = title.trim();
    cleaned.is_empty()
        || cleaned == "\u{65b0}\u{4f1a}\u{8bdd}"
        || cleaned == "\u{672a}\u{547d}\u{540d}\u{4f1a}\u{8bdd}"
        || cleaned.eq_ignore_ascii_case(DEFAULT_SESSION_TITLE)
}

pub(super) fn build_session_title(content: Option<&str>) -> Option<String> {
    let raw = content?.trim();
    if parse_channel_command(Some(raw)).is_some() {
        return None;
    }
    let cleaned = raw.replace('\n', " ");
    if cleaned.is_empty() {
        return None;
    }
    let mut output = cleaned;
    if output.chars().count() > 20 {
        output = output.chars().take(20).collect::<String>();
        output.push_str("...");
    }
    Some(output)
}

pub(super) fn normalize_channel_model_error_text(raw: &str) -> String {
    let mut detail = raw.trim();
    if let Some(stripped) = detail.strip_prefix("channel stream run failed:") {
        detail = stripped.trim();
    }
    if detail.is_empty() {
        return CHANNEL_MODEL_ERROR_FALLBACK_TEXT.to_string();
    }
    truncate_text(detail, 220)
}

pub(super) fn format_channel_model_error_detail(err: &anyhow::Error) -> String {
    normalize_channel_model_error_text(&err.to_string())
}

pub(super) fn format_channel_model_error_reply(err: &anyhow::Error) -> String {
    let detail = format_channel_model_error_detail(err);
    if detail == CHANNEL_MODEL_ERROR_FALLBACK_TEXT {
        return detail;
    }
    format!("模型请求失败：{detail}")
}

pub(super) fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_channel_approval_decision_supports_numbers_and_keywords() {
        assert_eq!(
            parse_channel_approval_decision(Some("1")),
            Some(ApprovalResponse::ApproveOnce)
        );
        assert_eq!(
            parse_channel_approval_decision(Some("2")),
            Some(ApprovalResponse::ApproveSession)
        );
        assert_eq!(
            parse_channel_approval_decision(Some("3")),
            Some(ApprovalResponse::Deny)
        );
        assert_eq!(
            parse_channel_approval_decision(Some("同意一次")),
            Some(ApprovalResponse::ApproveOnce)
        );
        assert_eq!(
            parse_channel_approval_decision(Some("同意本会话")),
            Some(ApprovalResponse::ApproveSession)
        );
        assert_eq!(
            parse_channel_approval_decision(Some("拒绝")),
            Some(ApprovalResponse::Deny)
        );
        assert_eq!(parse_channel_approval_decision(Some("continue")), None);
    }

    #[test]
    fn compacting_progress_event_detects_stage_in_payload() {
        let payload = json!({ "stage": "compacting" });
        assert!(is_compacting_progress_event(
            "progress",
            &payload,
            &json!({})
        ));
    }

    #[test]
    fn compacting_progress_event_detects_stage_in_raw_event() {
        let payload = json!({});
        let raw = json!({ "stage": "compacting" });
        assert!(is_compacting_progress_event("progress", &payload, &raw));
        assert!(!is_compacting_progress_event("final", &raw, &raw));
    }

    #[test]
    fn resolve_channel_actor_id_prefers_sender() {
        let message = ChannelMessage {
            channel: "generic".to_string(),
            account_id: "account".to_string(),
            peer: crate::channels::types::ChannelPeer {
                kind: "group".to_string(),
                id: "peer_1".to_string(),
                name: None,
            },
            thread: None,
            message_id: None,
            sender: Some(crate::channels::types::ChannelSender {
                id: "sender_1".to_string(),
                name: None,
            }),
            message_type: "text".to_string(),
            text: Some("hello".to_string()),
            attachments: Vec::new(),
            location: None,
            ts: None,
            meta: None,
        };
        assert_eq!(resolve_channel_actor_id(&message), "sender_1".to_string());
    }

    #[test]
    fn build_internal_channel_headers_skips_empty_token() {
        let headers = build_internal_channel_headers(None).expect("headers");
        assert!(headers.get("x-channel-token").is_none());

        let headers = build_internal_channel_headers(Some("   ")).expect("headers");
        assert!(headers.get("x-channel-token").is_none());
    }

    #[test]
    fn build_internal_channel_headers_sets_trimmed_token() {
        let headers = build_internal_channel_headers(Some("  token-123  ")).expect("headers");
        let token = headers
            .get("x-channel-token")
            .and_then(|value| value.to_str().ok());
        assert_eq!(token, Some("token-123"));
    }

    #[test]
    fn build_internal_channel_headers_rejects_invalid_token_value() {
        let err = build_internal_channel_headers(Some("token\nbad")).expect_err("invalid header");
        let message = err.to_string();
        assert!(message.contains("invalid inbound token header value"));
    }

    #[test]
    fn append_weixin_context_token_trims_and_sets_meta() {
        let mut meta = json!({});
        append_weixin_context_token(&mut meta, Some("  ctx-1  "));
        assert_eq!(
            meta.get("weixin_context_token").and_then(Value::as_str),
            Some("ctx-1")
        );
    }

    #[test]
    fn append_weixin_context_token_ignores_empty_or_non_object_meta() {
        let mut empty_token_meta = json!({});
        append_weixin_context_token(&mut empty_token_meta, Some("   "));
        assert!(empty_token_meta.get("weixin_context_token").is_none());

        let mut non_object_meta = json!("text");
        append_weixin_context_token(&mut non_object_meta, Some("ctx-2"));
        assert_eq!(non_object_meta, json!("text"));
    }

    #[test]
    fn append_weixin_context_token_from_message_reads_inbound_meta() {
        let mut message = ChannelMessage {
            channel: "weixin".to_string(),
            account_id: "account".to_string(),
            peer: crate::channels::types::ChannelPeer {
                kind: "user".to_string(),
                id: "user_1".to_string(),
                name: None,
            },
            thread: None,
            message_id: None,
            sender: None,
            message_type: "text".to_string(),
            text: Some("hello".to_string()),
            attachments: Vec::new(),
            location: None,
            ts: None,
            meta: Some(json!({ "weixin": { "context_token": "ctx-from-msg" } })),
        };
        let mut outbound_meta = json!({});
        append_weixin_context_token_from_message(&mut outbound_meta, &message);
        assert_eq!(
            outbound_meta
                .get("weixin_context_token")
                .and_then(Value::as_str),
            Some("ctx-from-msg")
        );

        message.meta = None;
        let mut outbound_meta_without_ctx = json!({});
        append_weixin_context_token_from_message(&mut outbound_meta_without_ctx, &message);
        assert!(outbound_meta_without_ctx
            .get("weixin_context_token")
            .is_none());
    }

    #[test]
    fn bridge_metadata_helpers_roundtrip_ids() {
        let resolution = BridgeRouteResolution {
            center: crate::storage::BridgeCenterRecord {
                center_id: "bc_1".to_string(),
                name: "Bridge".to_string(),
                code: "bridge".to_string(),
                description: None,
                owner_user_id: "owner".to_string(),
                status: "active".to_string(),
                default_preset_agent_name: "preset".to_string(),
                target_unit_id: None,
                default_identity_strategy: "sender_in_peer".to_string(),
                username_policy: "namespaced_generated".to_string(),
                password_policy: "fixed_default_123456".to_string(),
                settings: json!({}),
                created_at: 1.0,
                updated_at: 1.0,
            },
            center_account: crate::storage::BridgeCenterAccountRecord {
                center_account_id: "bca_1".to_string(),
                center_id: "bc_1".to_string(),
                channel: "xmpp".to_string(),
                account_id: "shared".to_string(),
                enabled: true,
                default_preset_agent_name_override: None,
                identity_strategy: None,
                thread_strategy: Some("main_thread".to_string()),
                reply_strategy: Some("reply_only".to_string()),
                fallback_policy: "forbid_owner_fallback".to_string(),
                provider_caps: None,
                status_reason: None,
                created_at: 1.0,
                updated_at: 1.0,
            },
            route: crate::storage::BridgeUserRouteRecord {
                route_id: "brt_1".to_string(),
                center_id: "bc_1".to_string(),
                center_account_id: "bca_1".to_string(),
                channel: "xmpp".to_string(),
                account_id: "shared".to_string(),
                external_identity_key: "xmpp:shared:user".to_string(),
                external_user_key: Some("user".to_string()),
                external_display_name: Some("User".to_string()),
                external_peer_id: Some("peer".to_string()),
                external_sender_id: Some("sender".to_string()),
                external_thread_id: None,
                external_profile: Some(json!({})),
                wunder_user_id: "user_1".to_string(),
                agent_id: "agent_1".to_string(),
                agent_name: "Preset".to_string(),
                user_created: true,
                agent_created: true,
                status: "active".to_string(),
                last_session_id: None,
                last_error: None,
                first_seen_at: 1.0,
                last_seen_at: 1.0,
                last_inbound_at: Some(1.0),
                last_outbound_at: None,
                created_at: 1.0,
                updated_at: 1.0,
            },
            session_strategy: "main_thread".to_string(),
        };
        let meta = build_bridge_session_metadata(&resolution);
        assert_eq!(
            extract_bridge_meta_ids(Some(&meta)),
            Some(("bc_1".to_string(), "bca_1".to_string(), "brt_1".to_string()))
        );
    }

    #[test]
    fn merge_object_values_prefers_overlay_keys() {
        let merged = merge_object_values(
            Some(json!({ "a": 1, "b": 2 })),
            Some(json!({ "b": 3, "c": 4 })),
        )
        .expect("merged");
        assert_eq!(merged, json!({ "a": 1, "b": 3, "c": 4 }));
    }

    #[test]
    fn merge_channel_request_overrides_injects_display_question() {
        let merged = merge_channel_request_overrides(
            Some(json!({ "security": { "approval_mode": "full_auto" } })),
            Some("  summarize this item  "),
        )
        .expect("merged");
        assert_eq!(
            merged
                .get(CHANNEL_DISPLAY_QUESTION_OVERRIDE_KEY)
                .and_then(Value::as_str),
            Some("summarize this item")
        );
        assert!(merged.get("security").is_some());
    }

    #[test]
    fn should_auto_title_accepts_channel_and_chat_placeholders() {
        assert!(should_auto_title(""));
        assert!(should_auto_title("\u{65b0}\u{4f1a}\u{8bdd}"));
        assert!(should_auto_title(
            "\u{672a}\u{547d}\u{540d}\u{4f1a}\u{8bdd}"
        ));
        assert!(should_auto_title(DEFAULT_SESSION_TITLE));
        assert!(!should_auto_title("manual title"));
    }

    #[test]
    fn build_session_title_uses_inbound_text_preview() {
        assert_eq!(
            build_session_title(Some("  first inbound title  ")),
            Some("first inbound title".to_string())
        );
    }

    #[test]
    fn build_session_title_skips_channel_commands() {
        assert_eq!(build_session_title(Some("/help")), None);
    }

    #[test]
    fn build_session_title_truncates_long_text() {
        assert_eq!(
            build_session_title(Some("12345678901234567890extra")),
            Some("12345678901234567890...".to_string())
        );
    }

    #[test]
    fn normalize_channel_model_error_text_strips_internal_prefix() {
        assert_eq!(
            normalize_channel_model_error_text("channel stream run failed: upstream timeout"),
            "upstream timeout"
        );
    }

    #[test]
    fn normalize_channel_model_error_text_falls_back_when_blank() {
        assert_eq!(
            normalize_channel_model_error_text("   "),
            CHANNEL_MODEL_ERROR_FALLBACK_TEXT
        );
    }

    #[test]
    fn format_channel_model_error_reply_adds_user_facing_prefix() {
        let err = anyhow!("channel stream run failed: provider unavailable");
        assert_eq!(
            format_channel_model_error_reply(&err),
            "模型请求失败：provider unavailable"
        );
    }

    #[test]
    fn resolve_channel_agent_display_name_prefers_agent_name() {
        let record = UserAgentRecord {
            agent_id: "agent_1".to_string(),
            user_id: "user_1".to_string(),
            hive_id: "hive_1".to_string(),
            name: "assistant_display".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            preview_skill: false,
            model_name: None,
            ability_items: Vec::new(),
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            visible_unit_ids: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "private".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
            created_at: 0.0,
            updated_at: 0.0,
            preset_binding: None,
            silent: false,
            prefer_mother: false,
        };
        assert_eq!(
            resolve_channel_agent_display_name(Some(&record), Some("agent_fallback")),
            "assistant_display"
        );
        assert_eq!(
            resolve_channel_agent_display_name(None, Some("agent_fallback")),
            "agent_fallback"
        );
        assert_eq!(resolve_channel_agent_display_name(None, None), "智能体");
    }
}
