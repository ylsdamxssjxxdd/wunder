use crate::channels::catalog::find_channel;
use crate::channels::types::ChannelMessage;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

pub const BRIDGE_CENTER_STATUS_ACTIVE: &str = "active";
pub const BRIDGE_CENTER_STATUS_PAUSED: &str = "paused";
pub const BRIDGE_CENTER_STATUS_DISABLED: &str = "disabled";

pub const BRIDGE_ROUTE_STATUS_ACTIVE: &str = "active";
pub const BRIDGE_ROUTE_STATUS_PAUSED: &str = "paused";
pub const BRIDGE_ROUTE_STATUS_BLOCKED: &str = "blocked";
pub const BRIDGE_ROUTE_STATUS_ERROR: &str = "error";

pub const BRIDGE_IDENTITY_STRATEGY_PEER: &str = "peer";
pub const BRIDGE_IDENTITY_STRATEGY_SENDER: &str = "sender";
pub const BRIDGE_IDENTITY_STRATEGY_SENDER_IN_PEER: &str = "sender_in_peer";
pub const BRIDGE_IDENTITY_STRATEGY_PEER_THREAD: &str = "peer_thread";
pub const BRIDGE_IDENTITY_STRATEGY_PLATFORM_USER: &str = "platform_user";
pub const BRIDGE_IDENTITY_STRATEGY_PLATFORM_CONVERSATION: &str = "platform_conversation";

pub const BRIDGE_USERNAME_POLICY_NAMESPACED_GENERATED: &str = "namespaced_generated";
pub const BRIDGE_USERNAME_POLICY_PREFER_RAW_USERNAME: &str = "prefer_raw_username";

pub const BRIDGE_THREAD_STRATEGY_MAIN_THREAD: &str = "main_thread";
pub const BRIDGE_THREAD_STRATEGY_PER_PEER: &str = "per_peer";
pub const BRIDGE_THREAD_STRATEGY_HYBRID: &str = "hybrid";

pub const BRIDGE_REPLY_STRATEGY_REPLY_ONLY: &str = "reply_only";
pub const BRIDGE_REPLY_STRATEGY_PROACTIVE: &str = "proactive";
pub const BRIDGE_REPLY_STRATEGY_PROVIDER_BOUND: &str = "provider_bound";

pub const BRIDGE_FALLBACK_POLICY_FORBID_OWNER: &str = "forbid_owner_fallback";

#[derive(Debug, Clone)]
pub struct BridgeIdentity {
    pub external_identity_key: String,
    pub external_user_key: Option<String>,
    pub external_display_name: Option<String>,
    pub external_peer_id: Option<String>,
    pub external_sender_id: Option<String>,
    pub external_thread_id: Option<String>,
    pub external_profile: Value,
}

pub fn normalize_bridge_center_status(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        BRIDGE_CENTER_STATUS_PAUSED => BRIDGE_CENTER_STATUS_PAUSED.to_string(),
        BRIDGE_CENTER_STATUS_DISABLED => BRIDGE_CENTER_STATUS_DISABLED.to_string(),
        _ => BRIDGE_CENTER_STATUS_ACTIVE.to_string(),
    }
}

pub fn normalize_bridge_route_status(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        BRIDGE_ROUTE_STATUS_PAUSED => BRIDGE_ROUTE_STATUS_PAUSED.to_string(),
        BRIDGE_ROUTE_STATUS_BLOCKED => BRIDGE_ROUTE_STATUS_BLOCKED.to_string(),
        BRIDGE_ROUTE_STATUS_ERROR => BRIDGE_ROUTE_STATUS_ERROR.to_string(),
        _ => BRIDGE_ROUTE_STATUS_ACTIVE.to_string(),
    }
}

pub fn normalize_bridge_identity_strategy(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        BRIDGE_IDENTITY_STRATEGY_PEER => BRIDGE_IDENTITY_STRATEGY_PEER.to_string(),
        BRIDGE_IDENTITY_STRATEGY_SENDER => BRIDGE_IDENTITY_STRATEGY_SENDER.to_string(),
        BRIDGE_IDENTITY_STRATEGY_PEER_THREAD => BRIDGE_IDENTITY_STRATEGY_PEER_THREAD.to_string(),
        BRIDGE_IDENTITY_STRATEGY_PLATFORM_USER => {
            BRIDGE_IDENTITY_STRATEGY_PLATFORM_USER.to_string()
        }
        BRIDGE_IDENTITY_STRATEGY_PLATFORM_CONVERSATION => {
            BRIDGE_IDENTITY_STRATEGY_PLATFORM_CONVERSATION.to_string()
        }
        _ => BRIDGE_IDENTITY_STRATEGY_SENDER_IN_PEER.to_string(),
    }
}

pub fn normalize_bridge_username_policy(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        BRIDGE_USERNAME_POLICY_PREFER_RAW_USERNAME => {
            BRIDGE_USERNAME_POLICY_PREFER_RAW_USERNAME.to_string()
        }
        _ => BRIDGE_USERNAME_POLICY_NAMESPACED_GENERATED.to_string(),
    }
}

pub fn normalize_bridge_thread_strategy(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        BRIDGE_THREAD_STRATEGY_PER_PEER => BRIDGE_THREAD_STRATEGY_PER_PEER.to_string(),
        BRIDGE_THREAD_STRATEGY_HYBRID => BRIDGE_THREAD_STRATEGY_HYBRID.to_string(),
        _ => BRIDGE_THREAD_STRATEGY_MAIN_THREAD.to_string(),
    }
}

pub fn normalize_bridge_reply_strategy(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        BRIDGE_REPLY_STRATEGY_PROACTIVE => BRIDGE_REPLY_STRATEGY_PROACTIVE.to_string(),
        BRIDGE_REPLY_STRATEGY_PROVIDER_BOUND => BRIDGE_REPLY_STRATEGY_PROVIDER_BOUND.to_string(),
        _ => BRIDGE_REPLY_STRATEGY_REPLY_ONLY.to_string(),
    }
}

pub fn normalize_bridge_fallback_policy(value: &str) -> String {
    if value
        .trim()
        .eq_ignore_ascii_case(BRIDGE_FALLBACK_POLICY_FORBID_OWNER)
    {
        BRIDGE_FALLBACK_POLICY_FORBID_OWNER.to_string()
    } else {
        BRIDGE_FALLBACK_POLICY_FORBID_OWNER.to_string()
    }
}

pub fn extract_bridge_identity(
    message: &ChannelMessage,
    configured_strategy: Option<&str>,
) -> Result<BridgeIdentity> {
    let strategy = configured_strategy
        .map(normalize_bridge_identity_strategy)
        .unwrap_or_else(|| infer_bridge_identity_strategy(message));
    let peer_id = trim_opt(Some(message.peer.id.as_str()));
    let sender_id = trim_opt(message.sender.as_ref().map(|sender| sender.id.as_str()));
    let thread_id = trim_opt(message.thread.as_ref().map(|thread| thread.id.as_str()));
    let platform_user_id = extract_meta_text(
        message.meta.as_ref(),
        &[
            "platform_user_id",
            "platformUserId",
            "user_id",
            "userId",
            "from_user_id",
            "sender_user_id",
            "external_user_key",
        ],
    );
    let platform_conversation_id = extract_meta_text(
        message.meta.as_ref(),
        &[
            "conversation_id",
            "conversationId",
            "platform_conversation_id",
            "platformConversationId",
            "chat_id",
        ],
    );

    let (external_user_key, mut key_segments): (Option<String>, Vec<String>) =
        match strategy.as_str() {
            BRIDGE_IDENTITY_STRATEGY_PEER => {
                let peer_id = peer_id
                    .clone()
                    .ok_or_else(|| anyhow!("bridge identity peer_id missing"))?;
                (
                    Some(peer_id.clone()),
                    vec![
                        message.channel.trim().to_ascii_lowercase(),
                        message.account_id.trim().to_string(),
                        message.peer.kind.trim().to_ascii_lowercase(),
                        peer_id,
                    ],
                )
            }
            BRIDGE_IDENTITY_STRATEGY_SENDER => {
                let sender_id = sender_id
                    .clone()
                    .or_else(|| platform_user_id.clone())
                    .ok_or_else(|| anyhow!("bridge identity sender_id missing"))?;
                (
                    Some(sender_id.clone()),
                    vec![
                        message.channel.trim().to_ascii_lowercase(),
                        message.account_id.trim().to_string(),
                        sender_id,
                    ],
                )
            }
            BRIDGE_IDENTITY_STRATEGY_PEER_THREAD => {
                let peer_id = peer_id
                    .clone()
                    .ok_or_else(|| anyhow!("bridge identity peer_id missing"))?;
                let thread_id = thread_id
                    .clone()
                    .ok_or_else(|| anyhow!("bridge identity thread_id missing"))?;
                let sender_key = sender_id.clone().or_else(|| platform_user_id.clone());
                let mut segments = vec![
                    message.channel.trim().to_ascii_lowercase(),
                    message.account_id.trim().to_string(),
                    peer_id.clone(),
                    thread_id.clone(),
                ];
                if let Some(sender_key) = sender_key.clone() {
                    segments.push(sender_key.clone());
                }
                (sender_key.or(Some(peer_id)), segments)
            }
            BRIDGE_IDENTITY_STRATEGY_PLATFORM_USER => {
                let platform_user_id = platform_user_id
                    .clone()
                    .or_else(|| sender_id.clone())
                    .or_else(|| peer_id.clone())
                    .ok_or_else(|| anyhow!("bridge identity platform_user_id missing"))?;
                (
                    Some(platform_user_id.clone()),
                    vec![
                        message.channel.trim().to_ascii_lowercase(),
                        message.account_id.trim().to_string(),
                        platform_user_id,
                    ],
                )
            }
            BRIDGE_IDENTITY_STRATEGY_PLATFORM_CONVERSATION => {
                let conversation_id = platform_conversation_id
                    .clone()
                    .or_else(|| peer_id.clone())
                    .ok_or_else(|| anyhow!("bridge identity conversation_id missing"))?;
                (
                    Some(conversation_id.clone()),
                    vec![
                        message.channel.trim().to_ascii_lowercase(),
                        message.account_id.trim().to_string(),
                        conversation_id,
                    ],
                )
            }
            _ => {
                let sender_id = sender_id
                    .clone()
                    .or_else(|| platform_user_id.clone())
                    .ok_or_else(|| anyhow!("bridge identity sender_id missing"))?;
                let peer_id = peer_id
                    .clone()
                    .ok_or_else(|| anyhow!("bridge identity peer_id missing"))?;
                (
                    Some(sender_id.clone()),
                    vec![
                        message.channel.trim().to_ascii_lowercase(),
                        message.account_id.trim().to_string(),
                        peer_id,
                        sender_id,
                    ],
                )
            }
        };
    key_segments.insert(2, strategy.clone());

    let external_display_name = trim_opt(
        message
            .sender
            .as_ref()
            .and_then(|sender| sender.name.as_deref())
            .or(message.peer.name.as_deref()),
    )
    .or_else(|| external_user_key.clone());

    Ok(BridgeIdentity {
        external_identity_key: key_segments.join(":"),
        external_user_key,
        external_display_name,
        external_peer_id: peer_id,
        external_sender_id: sender_id,
        external_thread_id: thread_id,
        external_profile: json!({
            "channel": message.channel,
            "account_id": message.account_id,
            "peer_kind": message.peer.kind,
            "peer_id": message.peer.id,
            "peer_name": message.peer.name,
            "sender_id": message.sender.as_ref().map(|sender| sender.id.clone()),
            "sender_name": message.sender.as_ref().and_then(|sender| sender.name.clone()),
            "thread_id": message.thread.as_ref().map(|thread| thread.id.clone()),
            "thread_topic": message.thread.as_ref().and_then(|thread| thread.topic.clone()),
            "message_id": message.message_id,
            "strategy": strategy,
            "platform_user_id": platform_user_id,
            "platform_conversation_id": platform_conversation_id,
        }),
    })
}

pub fn build_bridge_provider_caps(channel: &str, adapter_registered: bool) -> Value {
    let catalog = find_channel(channel);
    let runtime_mode = catalog.map(|item| item.webhook_mode).unwrap_or("generic");
    json!({
        "channel": channel.trim().to_ascii_lowercase(),
        "display_name": catalog.map(|item| item.display_name).unwrap_or(channel),
        "adapter_registered": adapter_registered,
        "runtime_mode": runtime_mode,
        "supports_thread": matches!(runtime_mode, "runtime+generic" | "specialized+generic"),
        "supports_proactive": adapter_registered,
        "requires_context_token": channel.trim().eq_ignore_ascii_case("weixin"),
        "supports_group_identity": true,
    })
}

fn infer_bridge_identity_strategy(message: &ChannelMessage) -> String {
    if message.thread.is_some() {
        return BRIDGE_IDENTITY_STRATEGY_PEER_THREAD.to_string();
    }
    if is_direct_peer(&message.peer.kind) {
        return BRIDGE_IDENTITY_STRATEGY_PEER.to_string();
    }
    BRIDGE_IDENTITY_STRATEGY_SENDER_IN_PEER.to_string()
}

fn is_direct_peer(kind: &str) -> bool {
    matches!(
        kind.trim().to_ascii_lowercase().as_str(),
        "dm" | "direct" | "private"
    )
}

fn trim_opt(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn extract_meta_text(meta: Option<&Value>, keys: &[&str]) -> Option<String> {
    let meta = meta?;
    for key in keys {
        let value = meta.get(*key)?;
        if let Some(text) = value.as_str() {
            let cleaned = text.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        } else if let Some(number) = value.as_i64() {
            return Some(number.to_string());
        } else if let Some(number) = value.as_u64() {
            return Some(number.to_string());
        }
    }
    None
}
