use super::ToolContext;
use crate::channels::types::{
    ChannelAccountConfig, ChannelAttachment, ChannelOutboundMessage, ChannelPeer, ChannelThread,
};
use crate::channels::xmpp;
use crate::storage::{ChannelAccountRecord, ChannelOutboxRecord, ListChannelUserBindingsQuery};
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

const MAX_CONTACT_LIMIT: i64 = 200;
const DEFAULT_CONTACT_LIMIT: i64 = 20;
const MAX_CONTACT_FETCH: i64 = 1000;
const MAX_ACCOUNT_BINDINGS_FETCH: i64 = 2000;
const MAX_SESSION_RESOLUTION_FETCH: i64 = 256;
const DEFAULT_WAIT_TIMEOUT_S: f64 = 8.0;
const MIN_WAIT_TIMEOUT_S: f64 = 1.0;
const MAX_WAIT_TIMEOUT_S: f64 = 30.0;
const WAIT_POLL_INTERVAL_MS: u64 = 250;
const LEGACY_ACCOUNT_PREFIX: &str = "uacc:";
const CHANNEL_TOOL_OPEN_ACCESS_FOR_TEST: bool = true;

pub(super) const TOOL_CHANNEL: &str = "\u{6e20}\u{9053}\u{5de5}\u{5177}";

#[derive(Debug, Deserialize, Default)]
struct ChannelToolArgs {
    action: String,
    #[serde(default, alias = "provider")]
    channel: Option<String>,
    #[serde(default, alias = "account")]
    account_id: Option<String>,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    refresh: Option<bool>,
    #[serde(
        default,
        alias = "target",
        alias = "peer",
        alias = "peer_id",
        alias = "jid",
        alias = "receiver"
    )]
    to: Option<String>,
    #[serde(default, alias = "kind")]
    peer_kind: Option<String>,
    #[serde(default, alias = "thread")]
    thread_id: Option<String>,
    #[serde(default, alias = "message", alias = "body")]
    text: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    attachments: Option<Vec<ChannelAttachmentInput>>,
    #[serde(default)]
    meta: Option<Value>,
    #[serde(default)]
    wait: Option<bool>,
    #[serde(default)]
    wait_timeout_s: Option<f64>,
    #[serde(default)]
    contact: Option<ChannelContactInput>,
}

#[derive(Debug, Deserialize, Default)]
struct ChannelAttachmentInput {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    mime: Option<String>,
    #[serde(default)]
    size: Option<i64>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct ChannelContactInput {
    #[serde(default, alias = "provider")]
    channel: Option<String>,
    #[serde(default, alias = "account")]
    account_id: Option<String>,
    #[serde(
        default,
        alias = "target",
        alias = "peer",
        alias = "peer_id",
        alias = "jid",
        alias = "receiver"
    )]
    to: Option<String>,
    #[serde(default, alias = "kind")]
    peer_kind: Option<String>,
    #[serde(default, alias = "thread")]
    thread_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedChannelAccount {
    channel: String,
    account_id: String,
    source: &'static str,
}

#[derive(Debug, Clone)]
struct ResolvedPeerTarget {
    peer_id: String,
    peer_kind: String,
    thread_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ContactItem {
    channel: String,
    account_id: String,
    peer_kind: String,
    peer_id: String,
    thread_id: Option<String>,
    name: Option<String>,
    subscription: Option<String>,
    ask: Option<String>,
    groups: Vec<String>,
    session_id: String,
    last_message_at: f64,
    from_history: bool,
    from_roster: bool,
}

pub(super) async fn channel_tool(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: ChannelToolArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    match normalize_action(&payload.action).as_str() {
        "list_contacts" | "list" | "contacts" | "list_peers" | "peers" => {
            list_contacts(context, &payload).await
        }
        "send_message" | "send" | "message" => send_message(context, &payload).await,
        action => Err(anyhow!("unknown channel_tool action: {action}")),
    }
}

async fn list_contacts(context: &ToolContext<'_>, payload: &ChannelToolArgs) -> Result<Value> {
    let mut channel =
        normalize_non_empty(payload.channel.as_deref()).map(|value| value.to_ascii_lowercase());
    let mut account_id = normalize_non_empty(payload.account_id.as_deref()).map(str::to_string);
    let keyword =
        normalize_non_empty(payload.keyword.as_deref()).map(|value| value.to_ascii_lowercase());
    let force_refresh = payload.refresh.unwrap_or(false);
    let offset = payload.offset.unwrap_or(0).max(0);
    let limit = payload
        .limit
        .unwrap_or(DEFAULT_CONTACT_LIMIT)
        .clamp(1, MAX_CONTACT_LIMIT);
    let mut resolved_scope: Option<&'static str> = None;

    if channel.is_none() && account_id.is_none() {
        if let Some(account) =
            resolve_account_from_session(context, channel.as_deref(), account_id.as_deref())?
        {
            channel = Some(account.channel);
            account_id = Some(account.account_id);
            resolved_scope = Some(account.source);
        }
    }
    if channel.is_none() && account_id.is_none() {
        if let Some(account) = resolve_account_from_active_accounts(
            context,
            channel.as_deref(),
            account_id.as_deref(),
            false,
        )? {
            channel = Some(account.channel);
            account_id = Some(account.account_id);
            resolved_scope = Some(account.source);
        }
    }

    let account_keys =
        resolve_owned_account_keys(context, channel.as_deref(), account_id.as_deref())?;
    if account_keys.is_empty() {
        return Ok(json!({
            "action": "list_contacts",
            "items": [],
            "total": 0,
            "offset": offset,
            "limit": limit
        }));
    }

    let mut dedupe = HashMap::<String, ContactItem>::new();
    for (item_channel, item_account_id) in &account_keys {
        let (sessions, _) = context.storage.list_channel_sessions(
            Some(item_channel),
            Some(item_account_id),
            None,
            None,
            0,
            MAX_CONTACT_FETCH,
        )?;
        for session in sessions {
            upsert_session_contact(context, &mut dedupe, session);
        }
    }
    let warnings =
        merge_xmpp_roster_contacts_for_accounts(context, &account_keys, force_refresh, &mut dedupe)
            .await?;

    let mut items: Vec<ContactItem> = dedupe
        .into_values()
        .filter(|item| {
            keyword
                .as_ref()
                .is_none_or(|needle| matches_keyword(item, needle))
        })
        .collect();
    items.sort_by(|left, right| {
        right
            .last_message_at
            .partial_cmp(&left.last_message_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total = items.len() as i64;
    let paged = items
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|item| {
            json!({
                "channel": item.channel,
                "account_id": item.account_id,
                "peer_kind": item.peer_kind,
                "peer_id": item.peer_id,
                "thread_id": item.thread_id,
                "contact": build_contact_value(
                    &item.channel,
                    &item.account_id,
                    &item.peer_kind,
                    &item.peer_id,
                    item.thread_id.as_deref(),
                ),
                "name": item.name,
                "subscription": item.subscription,
                "ask": item.ask,
                "groups": item.groups,
                "session_id": item.session_id,
                "last_message_at": item.last_message_at,
                "source": contact_source_label(&item),
            })
        })
        .collect::<Vec<_>>();

    let mut result = json!({
        "action": "list_contacts",
        "items": paged,
        "total": total,
        "offset": offset,
        "limit": limit
    });
    if !warnings.is_empty() {
        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "warnings".to_string(),
                Value::Array(warnings.into_iter().map(Value::String).collect()),
            );
        }
    }
    if let Some(source) = resolved_scope {
        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "resolved_scope".to_string(),
                json!({
                    "source": source,
                    "channel": channel,
                    "account_id": account_id,
                }),
            );
        }
    }
    Ok(result)
}

async fn send_message(context: &ToolContext<'_>, payload: &ChannelToolArgs) -> Result<Value> {
    let contact = payload.contact.as_ref();
    let hinted_channel = normalize_non_empty(payload.channel.as_deref())
        .or_else(|| contact.and_then(|value| normalize_non_empty(value.channel.as_deref())))
        .map(|value| value.to_ascii_lowercase());
    let hinted_account_id = normalize_non_empty(payload.account_id.as_deref())
        .or_else(|| contact.and_then(|value| normalize_non_empty(value.account_id.as_deref())))
        .map(str::to_string);
    let resolved_account = resolve_send_account(
        context,
        hinted_channel.as_deref(),
        hinted_account_id.as_deref(),
    )?;
    let account_source = resolved_account.source;
    let channel = resolved_account.channel;
    let account_id = resolved_account.account_id;
    if !CHANNEL_TOOL_OPEN_ACCESS_FOR_TEST && !user_owns_account(context, &channel, &account_id)? {
        return Err(anyhow!(
            "permission denied: channel account not owned by current user"
        ));
    }
    let account = context
        .storage
        .get_channel_account(&channel, &account_id)?
        .ok_or_else(|| anyhow!("channel account not found"))?;
    if !account.status.trim().eq_ignore_ascii_case("active") {
        return Err(anyhow!("channel account disabled"));
    }
    let session_target = resolve_target_from_session(context, &channel, &account_id)?;
    let default_target =
        resolve_default_target_from_account_config(&account.config, context.user_id);
    let explicit_to = normalize_non_empty(payload.to.as_deref())
        .or_else(|| contact.and_then(|value| normalize_non_empty(value.to.as_deref())))
        .map(str::to_string);
    let to = explicit_to
        .clone()
        .or_else(|| session_target.as_ref().map(|value| value.peer_id.clone()))
        .or_else(|| default_target.as_ref().map(|value| value.peer_id.clone()))
        .ok_or_else(|| {
            anyhow!(
                "to is required; provide to/contact, or call in a channel session with unique peer, or preconfigure channel_tool_default_to"
            )
        })?;
    let target_source = if explicit_to.is_some() {
        "input"
    } else if session_target.is_some() {
        "session"
    } else if default_target.is_some() {
        "account_default"
    } else {
        "unknown"
    };

    let text = normalize_non_empty(payload.text.as_deref())
        .or_else(|| normalize_non_empty(payload.content.as_deref()))
        .map(str::to_string);
    let attachments = normalize_attachments(payload.attachments.as_deref().unwrap_or_default());
    if text.as_deref().unwrap_or("").trim().is_empty() && attachments.is_empty() {
        return Err(anyhow!("text/content or attachments is required"));
    }

    let peer_kind = normalize_peer_kind(
        normalize_non_empty(payload.peer_kind.as_deref())
            .or_else(|| contact.and_then(|value| normalize_non_empty(value.peer_kind.as_deref())))
            .or_else(|| {
                session_target
                    .as_ref()
                    .map(|value| value.peer_kind.as_str())
            })
            .or_else(|| {
                default_target
                    .as_ref()
                    .map(|value| value.peer_kind.as_str())
            }),
    );
    let thread = normalize_non_empty(payload.thread_id.as_deref())
        .or_else(|| contact.and_then(|value| normalize_non_empty(value.thread_id.as_deref())))
        .map(|id| ChannelThread {
            id: id.to_string(),
            topic: None,
        })
        .or_else(|| {
            session_target
                .as_ref()
                .and_then(|value| value.thread_id.as_ref())
                .map(|id| ChannelThread {
                    id: id.clone(),
                    topic: None,
                })
        })
        .or_else(|| {
            default_target
                .as_ref()
                .and_then(|value| value.thread_id.as_ref())
                .map(|id| ChannelThread {
                    id: id.clone(),
                    topic: None,
                })
        });
    let mut meta = build_send_meta(context, payload.meta.as_ref(), &to);
    if channel.eq_ignore_ascii_case("xmpp") {
        let meta_obj = meta
            .as_object_mut()
            .ok_or_else(|| anyhow!("invalid outbound meta"))?;
        if !meta_obj.contains_key("xmpp_to") {
            meta_obj.insert("xmpp_to".to_string(), Value::String(to.clone()));
        }
    }

    let outbound = ChannelOutboundMessage {
        channel: channel.clone(),
        account_id: account_id.clone(),
        peer: ChannelPeer {
            kind: peer_kind,
            id: to.clone(),
            name: None,
        },
        thread,
        text,
        attachments,
        meta: Some(meta),
    };
    let resolved_peer_kind = outbound.peer.kind.clone();
    let resolved_thread_id = outbound.thread.as_ref().map(|value| value.id.clone());

    let outbox_id = format!("outbox_{}", Uuid::new_v4().simple());
    let now = now_ts();
    let record = ChannelOutboxRecord {
        outbox_id: outbox_id.clone(),
        channel: channel.clone(),
        account_id: account_id.clone(),
        peer_kind: resolved_peer_kind.clone(),
        peer_id: outbound.peer.id.clone(),
        thread_id: resolved_thread_id.clone(),
        payload: json!(outbound),
        status: "pending".to_string(),
        retry_count: 0,
        retry_at: now,
        last_error: None,
        created_at: now,
        updated_at: now,
        delivered_at: None,
    };
    context.storage.enqueue_channel_outbox(&record)?;

    let wait = payload.wait.unwrap_or(false);
    if wait {
        let timeout_s = payload
            .wait_timeout_s
            .unwrap_or(DEFAULT_WAIT_TIMEOUT_S)
            .clamp(MIN_WAIT_TIMEOUT_S, MAX_WAIT_TIMEOUT_S);
        let delivery = wait_for_delivery(context, &outbox_id, timeout_s).await?;
        return Ok(json!({
            "action": "send_message",
            "ok": true,
            "outbox_id": outbox_id,
            "delivery": delivery,
            "resolved": {
                "channel": channel,
                "account_id": account_id,
                "to": to,
                "peer_kind": resolved_peer_kind.clone(),
                "thread_id": resolved_thread_id.clone(),
                "account_source": account_source,
                "target_source": target_source,
            }
        }));
    }

    Ok(json!({
        "action": "send_message",
        "ok": true,
        "outbox_id": outbox_id,
        "status": "pending",
        "resolved": {
            "channel": channel,
            "account_id": account_id,
            "to": to,
            "peer_kind": resolved_peer_kind,
            "thread_id": resolved_thread_id,
            "account_source": account_source,
            "target_source": target_source,
        }
    }))
}

fn resolve_send_account(
    context: &ToolContext<'_>,
    channel_hint: Option<&str>,
    account_hint: Option<&str>,
) -> Result<ResolvedChannelAccount> {
    if let (Some(channel), Some(account_id)) = (channel_hint, account_hint) {
        return Ok(ResolvedChannelAccount {
            channel: channel.to_string(),
            account_id: account_id.to_string(),
            source: "input",
        });
    }
    if let Some(account) = resolve_account_from_session(context, channel_hint, account_hint)? {
        return Ok(account);
    }
    if let Some(account) =
        resolve_account_from_active_accounts(context, channel_hint, account_hint, true)?
    {
        return Ok(account);
    }
    Err(anyhow!(
        "channel/account_id unresolved; provide channel+account_id, or set channel_tool_default=true in one active account config"
    ))
}

fn resolve_account_from_session(
    context: &ToolContext<'_>,
    channel_filter: Option<&str>,
    account_filter: Option<&str>,
) -> Result<Option<ResolvedChannelAccount>> {
    let (sessions, _) = context.storage.list_channel_sessions(
        channel_filter,
        account_filter,
        None,
        Some(context.session_id),
        0,
        MAX_SESSION_RESOLUTION_FETCH,
    )?;
    let mut candidates = HashSet::<(String, String)>::new();
    for session in sessions {
        if !session.user_id.eq_ignore_ascii_case(context.user_id.trim()) {
            continue;
        }
        let channel = session.channel.trim().to_ascii_lowercase();
        let account_id = session.account_id.trim().to_string();
        if channel.is_empty() || account_id.is_empty() {
            continue;
        }
        candidates.insert((channel, account_id));
    }
    if candidates.len() != 1 {
        return Ok(None);
    }
    let Some((channel, account_id)) = candidates.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(ResolvedChannelAccount {
        channel,
        account_id,
        source: "session",
    }))
}

fn resolve_account_from_active_accounts(
    context: &ToolContext<'_>,
    channel_filter: Option<&str>,
    account_filter: Option<&str>,
    strict: bool,
) -> Result<Option<ResolvedChannelAccount>> {
    let candidates = collect_active_accessible_accounts(context, channel_filter, account_filter)?;
    if candidates.is_empty() {
        return Ok(None);
    }
    if candidates.len() == 1 {
        let account = &candidates[0];
        return Ok(Some(ResolvedChannelAccount {
            channel: account.channel.trim().to_ascii_lowercase(),
            account_id: account.account_id.trim().to_string(),
            source: "active_single",
        }));
    }
    let defaults = candidates
        .iter()
        .filter(|account| account_marked_channel_tool_default(account, context.user_id))
        .collect::<Vec<_>>();
    if defaults.len() == 1 {
        let account = defaults[0];
        return Ok(Some(ResolvedChannelAccount {
            channel: account.channel.trim().to_ascii_lowercase(),
            account_id: account.account_id.trim().to_string(),
            source: "account_default",
        }));
    }
    if strict {
        return Err(anyhow!(
            "multiple channel accounts available; provide account_id or set channel_tool_default=true on one account"
        ));
    }
    Ok(None)
}

fn collect_active_accessible_accounts(
    context: &ToolContext<'_>,
    channel_filter: Option<&str>,
    account_filter: Option<&str>,
) -> Result<Vec<ChannelAccountRecord>> {
    let owned_keys = resolve_owned_account_keys(context, channel_filter, account_filter)?;
    if owned_keys.is_empty() {
        return Ok(Vec::new());
    }
    let owned_set: HashSet<(String, String)> = owned_keys
        .into_iter()
        .map(|(channel, account_id)| {
            (
                channel.to_ascii_lowercase(),
                account_id.to_ascii_lowercase(),
            )
        })
        .collect();
    let mut candidates = Vec::new();
    for mut account in context
        .storage
        .list_channel_accounts(channel_filter, Some("active"))?
    {
        let channel = account.channel.trim().to_ascii_lowercase();
        let account_id = account.account_id.trim().to_string();
        let account_key = account_id.to_ascii_lowercase();
        if channel.is_empty() || account_id.is_empty() {
            continue;
        }
        if account_filter
            .map(|value| value.eq_ignore_ascii_case(&account_id))
            .unwrap_or(true)
            && owned_set.contains(&(channel.clone(), account_key))
        {
            account.channel = channel;
            account.account_id = account_id;
            candidates.push(account);
        }
    }
    Ok(candidates)
}

fn resolve_target_from_session(
    context: &ToolContext<'_>,
    channel: &str,
    account_id: &str,
) -> Result<Option<ResolvedPeerTarget>> {
    let (sessions, _) = context.storage.list_channel_sessions(
        Some(channel),
        Some(account_id),
        None,
        Some(context.session_id),
        0,
        MAX_SESSION_RESOLUTION_FETCH,
    )?;
    let mut targets = HashMap::<String, ResolvedPeerTarget>::new();
    for session in sessions {
        if !session.user_id.eq_ignore_ascii_case(context.user_id.trim()) {
            continue;
        }
        let peer_id = session.peer_id.trim();
        if peer_id.is_empty() {
            continue;
        }
        let key = contact_key(
            channel,
            account_id,
            session.peer_kind.as_str(),
            peer_id,
            session.thread_id.as_deref(),
        );
        targets.entry(key).or_insert_with(|| ResolvedPeerTarget {
            peer_id: peer_id.to_string(),
            peer_kind: normalize_peer_kind(Some(session.peer_kind.as_str())),
            thread_id: session
                .thread_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        });
    }
    if targets.len() != 1 {
        return Ok(None);
    }
    Ok(targets.into_values().next())
}

fn resolve_default_target_from_account_config(
    config: &Value,
    user_id: &str,
) -> Option<ResolvedPeerTarget> {
    let peer_id = resolve_config_string_with_user_fallback(
        config,
        "channel_tool_default_to",
        "channel_tool_default_to_by_user",
        user_id,
    )?;
    let peer_kind = normalize_peer_kind(
        resolve_config_string_with_user_fallback(
            config,
            "channel_tool_default_peer_kind",
            "channel_tool_default_peer_kind_by_user",
            user_id,
        )
        .as_deref(),
    );
    let thread_id = resolve_config_string_with_user_fallback(
        config,
        "channel_tool_default_thread_id",
        "channel_tool_default_thread_id_by_user",
        user_id,
    );
    Some(ResolvedPeerTarget {
        peer_id,
        peer_kind,
        thread_id,
    })
}

fn resolve_config_string_with_user_fallback(
    config: &Value,
    key: &str,
    per_user_key: &str,
    user_id: &str,
) -> Option<String> {
    let map = config.as_object()?;
    if let Some(per_user) = map.get(per_user_key).and_then(Value::as_object) {
        if let Some(value) = per_user
            .get(user_id)
            .or_else(|| per_user.get("*"))
            .and_then(Value::as_str)
            .and_then(|value| normalize_non_empty(Some(value)))
        {
            return Some(value.to_string());
        }
        if let Some(value) = per_user
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(user_id))
            .and_then(|(_, value)| value.as_str())
            .and_then(|text| normalize_non_empty(Some(text)))
        {
            return Some(value.to_string());
        }
    }
    map.get(key)
        .and_then(Value::as_str)
        .and_then(|value| normalize_non_empty(Some(value)))
        .map(str::to_string)
}

fn account_marked_channel_tool_default(account: &ChannelAccountRecord, user_id: &str) -> bool {
    let Some(map) = account.config.as_object() else {
        return false;
    };
    for key in ["channel_tool_default", "default_for_channel_tool"] {
        if map.get(key).and_then(Value::as_bool).unwrap_or(false) {
            return true;
        }
    }
    if let Some(raw) = map.get("channel_tool_default_users") {
        if config_user_match(raw, user_id) {
            return true;
        }
    }
    false
}

fn config_user_match(value: &Value, user_id: &str) -> bool {
    let target = user_id.trim();
    if target.is_empty() {
        return false;
    }
    if let Some(text) = value.as_str() {
        return text
            .split([',', ';', ' '])
            .map(str::trim)
            .any(|item| !item.is_empty() && (item == "*" || item.eq_ignore_ascii_case(target)));
    }
    if let Some(items) = value.as_array() {
        return items.iter().any(|item| {
            item.as_str().is_some_and(|text| {
                let cleaned = text.trim();
                !cleaned.is_empty() && (cleaned == "*" || cleaned.eq_ignore_ascii_case(target))
            })
        });
    }
    false
}

fn build_contact_value(
    channel: &str,
    account_id: &str,
    peer_kind: &str,
    peer_id: &str,
    thread_id: Option<&str>,
) -> Value {
    json!({
        "channel": channel,
        "account_id": account_id,
        "peer_kind": peer_kind,
        "to": peer_id,
        "thread_id": thread_id,
    })
}

fn upsert_session_contact(
    context: &ToolContext<'_>,
    dedupe: &mut HashMap<String, ContactItem>,
    session: crate::storage::ChannelSessionRecord,
) {
    if !session.user_id.eq_ignore_ascii_case(context.user_id.trim()) {
        return;
    }
    let key = contact_key(
        &session.channel,
        &session.account_id,
        &session.peer_kind,
        &session.peer_id,
        session.thread_id.as_deref(),
    );
    let next = ContactItem {
        channel: session.channel,
        account_id: session.account_id,
        peer_kind: session.peer_kind,
        peer_id: session.peer_id,
        thread_id: session.thread_id,
        name: None,
        subscription: None,
        ask: None,
        groups: Vec::new(),
        session_id: session.session_id,
        last_message_at: session.last_message_at,
        from_history: true,
        from_roster: false,
    };
    if let Some(current) = dedupe.get_mut(&key) {
        if current.last_message_at < next.last_message_at {
            current.last_message_at = next.last_message_at;
            if current.session_id.trim().is_empty() {
                current.session_id = next.session_id;
            }
        }
        current.from_history = true;
        return;
    }
    dedupe.insert(key, next);
}

async fn merge_xmpp_roster_contacts_for_accounts(
    context: &ToolContext<'_>,
    account_keys: &[(String, String)],
    force_refresh: bool,
    dedupe: &mut HashMap<String, ContactItem>,
) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    for (channel, account_id) in account_keys {
        if !channel.eq_ignore_ascii_case(xmpp::XMPP_CHANNEL) {
            continue;
        }
        let Some(account) = context.storage.get_channel_account(channel, account_id)? else {
            continue;
        };
        let account_cfg = ChannelAccountConfig::from_value(&account.config);
        let Some(xmpp_cfg) = account_cfg.xmpp.as_ref() else {
            continue;
        };
        let roster = match xmpp::fetch_roster_contacts(account_id, xmpp_cfg, force_refresh).await {
            Ok(items) => items,
            Err(err) => {
                warnings.push(format!(
                    "xmpp roster unavailable: account_id={}, error={err}",
                    account_id
                ));
                continue;
            }
        };
        for contact in roster {
            upsert_roster_contact(dedupe, channel, account_id, contact);
        }
    }
    Ok(warnings)
}

fn upsert_roster_contact(
    dedupe: &mut HashMap<String, ContactItem>,
    channel: &str,
    account_id: &str,
    contact: xmpp::XmppRosterContact,
) {
    let key = contact_key(channel, account_id, "user", &contact.jid, None);
    let next = ContactItem {
        channel: channel.to_string(),
        account_id: account_id.to_string(),
        peer_kind: "user".to_string(),
        peer_id: contact.jid,
        thread_id: None,
        name: contact.name,
        subscription: Some(contact.subscription),
        ask: contact.ask,
        groups: contact.groups,
        session_id: String::new(),
        last_message_at: 0.0,
        from_history: false,
        from_roster: true,
    };
    if let Some(current) = dedupe.get_mut(&key) {
        if current.name.is_none() {
            current.name = next.name;
        }
        if current.subscription.is_none() {
            current.subscription = next.subscription;
        }
        if current.ask.is_none() {
            current.ask = next.ask;
        }
        if current.session_id.trim().is_empty() {
            current.session_id = next.session_id;
        }
        for group in next.groups {
            if !group.trim().is_empty() && !current.groups.iter().any(|item| item == &group) {
                current.groups.push(group);
            }
        }
        current.groups.sort();
        current.groups.dedup();
        current.from_roster = true;
        return;
    }
    dedupe.insert(key, next);
}

fn resolve_owned_account_keys(
    context: &ToolContext<'_>,
    channel_filter: Option<&str>,
    account_filter: Option<&str>,
) -> Result<Vec<(String, String)>> {
    if CHANNEL_TOOL_OPEN_ACCESS_FOR_TEST {
        return resolve_all_account_keys(context, channel_filter, account_filter);
    }

    let mut keys = HashSet::<(String, String)>::new();
    let query = ListChannelUserBindingsQuery {
        channel: channel_filter,
        account_id: account_filter,
        peer_kind: None,
        peer_id: None,
        user_id: Some(context.user_id),
        offset: 0,
        limit: MAX_ACCOUNT_BINDINGS_FETCH,
    };
    let (bindings, _) = context.storage.list_channel_user_bindings(query)?;
    for binding in bindings {
        let channel = binding.channel.trim().to_ascii_lowercase();
        let account_id = binding.account_id.trim().to_string();
        if channel.is_empty() || account_id.is_empty() {
            continue;
        }
        keys.insert((channel, account_id));
    }

    let accounts = context
        .storage
        .list_channel_accounts(channel_filter, Some("active"))?;
    let current_user = context.user_id.trim();
    for account in accounts {
        let owner = account
            .config
            .get("owner_user_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if !owner.eq_ignore_ascii_case(current_user) {
            continue;
        }
        let channel = account.channel.trim().to_ascii_lowercase();
        let account_id = account.account_id.trim().to_string();
        if channel.is_empty() || account_id.is_empty() {
            continue;
        }
        if channel_filter
            .map(|value| value.eq_ignore_ascii_case(&channel))
            .unwrap_or(true)
            && account_filter
                .map(|value| value.eq_ignore_ascii_case(&account_id))
                .unwrap_or(true)
        {
            keys.insert((channel, account_id));
        }
    }

    if let (Some(channel), Some(account_id)) = (channel_filter, account_filter) {
        if keys.is_empty() && user_owns_account(context, channel, account_id)? {
            keys.insert((channel.to_string(), account_id.to_string()));
        }
    }

    Ok(keys.into_iter().collect())
}

fn resolve_all_account_keys(
    context: &ToolContext<'_>,
    channel_filter: Option<&str>,
    account_filter: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let mut keys = HashSet::<(String, String)>::new();
    let accounts = context
        .storage
        .list_channel_accounts(channel_filter, None)?;
    for account in accounts {
        let channel = account.channel.trim().to_ascii_lowercase();
        let account_id = account.account_id.trim().to_string();
        if channel.is_empty() || account_id.is_empty() {
            continue;
        }
        if account_filter
            .map(|value| value.eq_ignore_ascii_case(&account_id))
            .unwrap_or(true)
        {
            keys.insert((channel, account_id));
        }
    }
    Ok(keys.into_iter().collect())
}

fn user_owns_account(context: &ToolContext<'_>, channel: &str, account_id: &str) -> Result<bool> {
    let channel = channel.trim().to_ascii_lowercase();
    let account_id = account_id.trim();
    if channel.is_empty() || account_id.is_empty() {
        return Ok(false);
    }
    if account_id.eq_ignore_ascii_case(&make_legacy_account_id(context.user_id, &channel)) {
        return Ok(true);
    }

    let query = ListChannelUserBindingsQuery {
        channel: Some(&channel),
        account_id: Some(account_id),
        peer_kind: None,
        peer_id: None,
        user_id: Some(context.user_id),
        offset: 0,
        limit: 1,
    };
    let (bindings, total) = context.storage.list_channel_user_bindings(query)?;
    if total > 0 || !bindings.is_empty() {
        return Ok(true);
    }

    let owned = context
        .storage
        .get_channel_account(&channel, account_id)?
        .and_then(|record| {
            record
                .config
                .get("owner_user_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .is_some_and(|owner| owner.eq_ignore_ascii_case(context.user_id.trim()));
    Ok(owned)
}

fn build_send_meta(context: &ToolContext<'_>, extra_meta: Option<&Value>, to: &str) -> Value {
    let mut meta = Map::new();
    meta.insert(
        "tool".to_string(),
        Value::String("channel_tool".to_string()),
    );
    meta.insert(
        "tool_action".to_string(),
        Value::String("send_message".to_string()),
    );
    meta.insert(
        "session_id".to_string(),
        Value::String(context.session_id.to_string()),
    );
    meta.insert(
        "user_id".to_string(),
        Value::String(context.user_id.to_string()),
    );
    if let Some(agent_id) = context
        .agent_id
        .and_then(|value| normalize_non_empty(Some(value)))
    {
        meta.insert("agent_id".to_string(), Value::String(agent_id.to_string()));
    }
    meta.insert("channel_to".to_string(), Value::String(to.to_string()));
    if let Some(extra) = extra_meta.and_then(Value::as_object) {
        for (key, value) in extra {
            if key.trim().is_empty() {
                continue;
            }
            meta.insert(key.clone(), value.clone());
        }
    }
    Value::Object(meta)
}

async fn wait_for_delivery(
    context: &ToolContext<'_>,
    outbox_id: &str,
    timeout_s: f64,
) -> Result<Value> {
    let deadline = std::time::Instant::now() + Duration::from_secs_f64(timeout_s);
    loop {
        let item = context.storage.get_channel_outbox(outbox_id)?;
        if let Some(record) = item {
            let status = record.status.trim().to_ascii_lowercase();
            if status == "sent" || status == "failed" {
                return Ok(json!({
                    "status": record.status,
                    "retry_count": record.retry_count,
                    "last_error": record.last_error,
                    "delivered_at": record.delivered_at,
                    "updated_at": record.updated_at
                }));
            }
            if std::time::Instant::now() >= deadline {
                return Ok(json!({
                    "status": record.status,
                    "retry_count": record.retry_count,
                    "last_error": record.last_error,
                    "delivered_at": record.delivered_at,
                    "updated_at": record.updated_at,
                    "timeout": true
                }));
            }
        } else {
            return Ok(json!({
                "status": "unknown",
                "error": "outbox record not found"
            }));
        }
        sleep(Duration::from_millis(WAIT_POLL_INTERVAL_MS)).await;
    }
}

fn normalize_attachments(raw: &[ChannelAttachmentInput]) -> Vec<ChannelAttachment> {
    raw.iter()
        .filter_map(|item| {
            let url = normalize_non_empty(item.url.as_deref())?.to_string();
            let kind = normalize_non_empty(item.kind.as_deref())
                .map(str::to_string)
                .unwrap_or_else(|| "file".to_string());
            Some(ChannelAttachment {
                kind,
                url,
                mime: normalize_non_empty(item.mime.as_deref()).map(str::to_string),
                size: item.size,
                name: normalize_non_empty(item.name.as_deref()).map(str::to_string),
            })
        })
        .collect()
}

fn normalize_action(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_peer_kind(value: Option<&str>) -> String {
    if value
        .map(str::trim)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("group"))
    {
        return "group".to_string();
    }
    "user".to_string()
}

fn normalize_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn contact_source_label(item: &ContactItem) -> &'static str {
    match (item.from_history, item.from_roster) {
        (true, true) => "session_history+roster",
        (true, false) => "session_history",
        (false, true) => "roster",
        (false, false) => "unknown",
    }
}

fn matches_keyword(item: &ContactItem, keyword: &str) -> bool {
    item.channel.to_ascii_lowercase().contains(keyword)
        || item.account_id.to_ascii_lowercase().contains(keyword)
        || item.peer_kind.to_ascii_lowercase().contains(keyword)
        || item.peer_id.to_ascii_lowercase().contains(keyword)
        || item
            .name
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains(keyword))
            .unwrap_or(false)
        || item
            .subscription
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains(keyword))
            .unwrap_or(false)
        || item
            .groups
            .iter()
            .any(|value| value.to_ascii_lowercase().contains(keyword))
        || item
            .thread_id
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains(keyword))
            .unwrap_or(false)
}

fn contact_key(
    channel: &str,
    account_id: &str,
    peer_kind: &str,
    peer_id: &str,
    thread_id: Option<&str>,
) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        channel.trim().to_ascii_lowercase(),
        account_id.trim().to_ascii_lowercase(),
        peer_kind.trim().to_ascii_lowercase(),
        peer_id.trim().to_ascii_lowercase(),
        thread_id.unwrap_or("").trim().to_ascii_lowercase()
    )
}

fn make_legacy_account_id(user_id: &str, channel: &str) -> String {
    format!(
        "{LEGACY_ACCOUNT_PREFIX}{}|{}",
        user_id.trim().to_ascii_lowercase(),
        channel.trim().to_ascii_lowercase()
    )
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::{
        contact_key, contact_source_label, make_legacy_account_id, matches_keyword,
        normalize_peer_kind, ContactItem,
    };

    #[test]
    fn normalize_peer_kind_defaults_to_user() {
        assert_eq!(normalize_peer_kind(None), "user");
        assert_eq!(normalize_peer_kind(Some("USER")), "user");
        assert_eq!(normalize_peer_kind(Some("group")), "group");
    }

    #[test]
    fn contact_key_is_case_insensitive() {
        let left = contact_key("xmpp", "Acc", "user", "Alice@Example.com", Some("T1"));
        let right = contact_key("XMPP", "acc", "USER", "alice@example.com", Some("t1"));
        assert_eq!(left, right);
    }

    #[test]
    fn matches_keyword_checks_peer_and_account() {
        let item = ContactItem {
            channel: "xmpp".to_string(),
            account_id: "acc_1".to_string(),
            peer_kind: "user".to_string(),
            peer_id: "alice@example.com".to_string(),
            thread_id: Some("thread-1".to_string()),
            name: Some("Alice".to_string()),
            subscription: Some("both".to_string()),
            ask: None,
            groups: vec!["friends".to_string()],
            session_id: "sess_1".to_string(),
            last_message_at: 1.0,
            from_history: true,
            from_roster: true,
        };
        assert!(matches_keyword(&item, "alice"));
        assert!(matches_keyword(&item, "acc_1"));
        assert!(matches_keyword(&item, "thread"));
        assert!(matches_keyword(&item, "friends"));
        assert!(!matches_keyword(&item, "bob"));
    }

    #[test]
    fn contact_source_label_supports_merged_sources() {
        let item = ContactItem {
            channel: "xmpp".to_string(),
            account_id: "acc".to_string(),
            peer_kind: "user".to_string(),
            peer_id: "alice@example.com".to_string(),
            thread_id: None,
            name: None,
            subscription: None,
            ask: None,
            groups: Vec::new(),
            session_id: String::new(),
            last_message_at: 0.0,
            from_history: true,
            from_roster: true,
        };
        assert_eq!(contact_source_label(&item), "session_history+roster");
    }

    #[test]
    fn make_legacy_account_id_is_stable() {
        assert_eq!(
            make_legacy_account_id("UserA", "XMPP"),
            "uacc:usera|xmpp".to_string()
        );
    }
}
