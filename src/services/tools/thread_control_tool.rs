use super::{build_model_tool_success_with_hint, context::ToolContext};
use crate::i18n;
use crate::storage::{AgentThreadRecord, ChatSessionRecord};
use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(crate) const TOOL_THREAD_CONTROL: &str = "会话线程控制";
pub(crate) const TOOL_THREAD_CONTROL_ALIAS: &str = "thread_control";
pub(crate) const TOOL_THREAD_CONTROL_ALIAS_ALT: &str = "session_thread";
pub(crate) const EVENT_THREAD_CONTROL: &str = "thread_control";

const CHAT_SESSION_STATUS_ACTIVE: &str = "active";
const CHAT_SESSION_STATUS_ARCHIVED: &str = "archived";
const DEFAULT_LIST_LIMIT: i64 = 20;
const MAX_LIST_LIMIT: i64 = 200;
const DEFAULT_SESSION_TITLE: &str = "新会话";
const THREAD_STATUS_IDLE: &str = "idle";

#[derive(Debug, Deserialize)]
struct ThreadControlArgs {
    action: String,
    #[serde(
        default,
        alias = "sessionId",
        alias = "session_id",
        alias = "targetSessionId",
        alias = "target_session_id",
        alias = "sessionKey",
        alias = "session_key"
    )]
    session_id: Option<String>,
    #[serde(
        default,
        alias = "parentSessionId",
        alias = "parent_session_id",
        alias = "parentId",
        alias = "parent_id"
    )]
    parent_session_id: Option<String>,
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, alias = "switchTo", alias = "switch_to")]
    switch: Option<bool>,
    #[serde(default, alias = "setMain", alias = "set_main")]
    set_main: Option<bool>,
}

pub(crate) async fn execute_thread_control_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let payload: ThreadControlArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = normalize_action(&payload.action);
    if action.is_empty() {
        return Err(anyhow!(i18n::t("error.content_required")));
    }
    match action.as_str() {
        "list" => list_threads(context, payload).await,
        "info" => info_thread(context, payload).await,
        "create" => create_thread(context, payload).await,
        "switch" => switch_thread(context, payload).await,
        "back" => back_thread(context, payload).await,
        "update_title" => update_thread_title(context, payload).await,
        "archive" => archive_thread(context, payload).await,
        "restore" => restore_thread(context, payload).await,
        "set_main" => set_main_thread(context, payload).await,
        _ => Err(anyhow!("unknown thread_control action: {}", payload.action)),
    }
}

fn normalize_action(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "list" | "ls" | "threads" | "thread_list" | "session_list" | "会话列表" | "线程列表"
        | "列表" => "list",
        "info" | "get" | "detail" | "show" | "thread_info" | "session_info" | "会话信息"
        | "线程信息" | "详情" => "info",
        "create" | "new" | "spawn" | "thread_new" | "会话创建" | "线程创建" | "新建" => {
            "create"
        }
        "switch" | "goto" | "open" | "focus" | "thread_switch" | "会话切换" | "线程切换"
        | "切换" => "switch",
        "back" | "return" | "parent" | "thread_back" | "返回线程" | "回到线程" | "返回" => {
            "back"
        }
        "update_title" | "rename" | "title" | "thread_rename" | "重命名" | "修改标题" => {
            "update_title"
        }
        "archive" | "archived" | "归档" => "archive",
        "restore" | "unarchive" | "恢复" => "restore",
        "set_main" | "main" | "pin_main" | "主线程" | "设为主线程" => "set_main",
        _ => "",
    }
    .to_string()
}

fn normalize_scope(raw: Option<&str>) -> &'static str {
    match raw.unwrap_or("").trim().to_lowercase().as_str() {
        "children" | "child" | "子线程" => "children",
        "roots" | "root" | "根线程" => "roots",
        "all" | "全部" => "all",
        _ => "branch",
    }
}

fn normalize_status_filter(raw: Option<&str>) -> Option<&'static str> {
    match raw.unwrap_or("").trim().to_lowercase().as_str() {
        "" | "active" | "running" | "启用" | "活跃" => Some(CHAT_SESSION_STATUS_ACTIVE),
        "archived" | "archive" | "归档" => Some(CHAT_SESSION_STATUS_ARCHIVED),
        "all" | "全部" => None,
        _ => Some(CHAT_SESSION_STATUS_ACTIVE),
    }
}

fn normalize_optional_string(raw: Option<String>) -> Option<String> {
    raw.and_then(|value| {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn clamp_limit(value: Option<i64>, default_value: i64, max_value: i64) -> i64 {
    value.unwrap_or(default_value).clamp(1, max_value)
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn format_ts(ts: f64) -> String {
    let millis = (ts * 1000.0) as i64;
    chrono::DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|dt| dt.with_timezone(&Local).to_rfc3339())
        .unwrap_or_default()
}

fn require_user_id<'a>(context: &'a ToolContext<'a>) -> Result<&'a str> {
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    Ok(user_id)
}

fn current_session_record(
    context: &ToolContext<'_>,
    user_id: &str,
) -> Result<Option<ChatSessionRecord>> {
    let session_id = context.session_id.trim();
    if session_id.is_empty() {
        return Ok(None);
    }
    context
        .storage
        .get_chat_session(user_id, session_id)
        .map_err(Into::into)
}

fn load_session_record(
    context: &ToolContext<'_>,
    user_id: &str,
    session_id: &str,
) -> Result<ChatSessionRecord> {
    context
        .storage
        .get_chat_session(user_id, session_id)?
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))
}

fn validate_agent_access(context: &ToolContext<'_>, user_id: &str, agent_id: &str) -> Result<()> {
    let cleaned = agent_id.trim();
    if cleaned.is_empty() {
        return Ok(());
    }
    context
        .storage
        .get_user_agent(user_id, cleaned)?
        .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
    Ok(())
}

fn session_agent_key(record: &ChatSessionRecord) -> String {
    record
        .agent_id
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .to_string()
}

fn resolve_agent_scope(
    args: &ThreadControlArgs,
    current: Option<&ChatSessionRecord>,
    target: Option<&ChatSessionRecord>,
    context_agent_id: Option<&str>,
) -> Option<String> {
    if let Some(agent_id) = args.agent_id.as_ref() {
        return Some(agent_id.trim().to_string());
    }
    if let Some(record) = target {
        return Some(session_agent_key(record));
    }
    if let Some(record) = current {
        return Some(session_agent_key(record));
    }
    context_agent_id.map(|value| value.trim().to_string())
}

fn resolve_main_session_id(
    context: &ToolContext<'_>,
    user_id: &str,
    agent_key: &str,
) -> Option<String> {
    context
        .storage
        .get_agent_thread(user_id, agent_key)
        .ok()
        .flatten()
        .map(|record| record.session_id)
}

fn resolve_runtime_status(context: &ToolContext<'_>, session_id: &str) -> Option<String> {
    context
        .monitor
        .as_ref()
        .and_then(|monitor| monitor.get_record(session_id))
        .and_then(|value| {
            value
                .get("status")
                .and_then(Value::as_str)
                .map(|status| status.to_string())
        })
}

fn session_payload(
    context: &ToolContext<'_>,
    user_id: &str,
    record: &ChatSessionRecord,
    relation: Option<&str>,
) -> Value {
    let agent_key = session_agent_key(record);
    let is_main = resolve_main_session_id(context, user_id, &agent_key)
        .map(|value| value == record.session_id)
        .unwrap_or(false);
    let mut payload = json!({
        "id": record.session_id,
        "title": record.title,
        "status": record.status,
        "created_at": format_ts(record.created_at),
        "updated_at": format_ts(record.updated_at),
        "last_message_at": format_ts(record.last_message_at),
        "agent_id": record.agent_id,
        "tool_overrides": record.tool_overrides,
        "parent_session_id": record.parent_session_id,
        "parent_message_id": record.parent_message_id,
        "spawn_label": record.spawn_label,
        "spawned_by": record.spawned_by,
        "is_main": is_main,
        "runtime_status": resolve_runtime_status(context, &record.session_id),
    });
    if let Some(relation) = relation.filter(|value| !value.trim().is_empty()) {
        if let Value::Object(ref mut map) = payload {
            map.insert("relation".to_string(), json!(relation));
        }
    }
    payload
}

fn build_thread_control_event(
    action: &str,
    session: Option<Value>,
    main_session: Option<Value>,
    switch_session: Option<Value>,
    switch: bool,
    set_main: bool,
    previous_session_id: Option<&str>,
) -> Value {
    json!({
        "action": action,
        "session": session,
        "main_session": main_session,
        "switch_session": switch_session,
        "switch": switch,
        "set_main": set_main,
        "previous_session_id": previous_session_id.filter(|value| !value.trim().is_empty()),
    })
}

fn emit_thread_control_event(context: &ToolContext<'_>, payload: Value) {
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(EVENT_THREAD_CONTROL, payload);
    }
}

fn build_thread_control_success(
    action: &str,
    summary: impl Into<String>,
    data: Value,
    next_step_hint: Option<String>,
) -> Value {
    build_model_tool_success_with_hint(action, "completed", summary, data, next_step_hint)
}

fn bind_main_session(
    context: &ToolContext<'_>,
    user_id: &str,
    agent_id: &str,
    session_id: &str,
    reason: &str,
) -> Result<AgentThreadRecord> {
    let session_record = load_session_record(context, user_id, session_id)?;
    let record_agent = session_agent_key(&session_record);
    let cleaned_agent = agent_id.trim();
    if !cleaned_agent.is_empty() && cleaned_agent != record_agent {
        return Err(anyhow!(i18n::t("error.permission_denied")));
    }
    let existing = context.storage.get_agent_thread(user_id, &record_agent)?;
    let now = now_ts();
    let record = AgentThreadRecord {
        thread_id: format!("thread_{session_id}"),
        user_id: user_id.to_string(),
        agent_id: record_agent.clone(),
        session_id: session_id.to_string(),
        status: existing
            .as_ref()
            .map(|item| item.status.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| THREAD_STATUS_IDLE.to_string()),
        created_at: existing.as_ref().map(|item| item.created_at).unwrap_or(now),
        updated_at: now,
    };
    context.storage.upsert_agent_thread(&record)?;
    if let Some(monitor) = context.monitor.as_ref() {
        monitor.record_event(
            session_id,
            "main_thread_changed",
            &json!({
                "session_id": session_id,
                "agent_id": record.agent_id,
                "user_id": user_id,
                "reason": reason,
            }),
        );
    }
    Ok(record)
}

fn list_sessions_by_status(
    context: &ToolContext<'_>,
    user_id: &str,
    agent_id: Option<&str>,
    parent_session_id: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<ChatSessionRecord>> {
    let (items, _) = context.storage.list_chat_sessions_by_status(
        user_id,
        agent_id,
        parent_session_id,
        status,
        0,
        limit,
    )?;
    Ok(items)
}

fn list_direct_children(
    context: &ToolContext<'_>,
    user_id: &str,
    parent_session_id: &str,
    agent_id: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<ChatSessionRecord>> {
    list_sessions_by_status(
        context,
        user_id,
        agent_id,
        Some(parent_session_id),
        status,
        limit,
    )
}

fn collect_branch_records(
    context: &ToolContext<'_>,
    user_id: &str,
    target: &ChatSessionRecord,
    agent_scope: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<(ChatSessionRecord, &'static str)>> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();

    let mut ancestors = Vec::new();
    let mut cursor = target.parent_session_id.clone();
    while let Some(parent_id) = cursor {
        if parent_id.trim().is_empty() {
            break;
        }
        let parent = load_session_record(context, user_id, &parent_id)?;
        cursor = parent.parent_session_id.clone();
        if status.is_none()
            || parent
                .status
                .eq_ignore_ascii_case(status.unwrap_or_default())
        {
            ancestors.push(parent);
        } else if parent.status.trim().is_empty() && status == Some(CHAT_SESSION_STATUS_ACTIVE) {
            ancestors.push(parent);
        }
    }
    ancestors.reverse();
    for record in ancestors {
        if seen.insert(record.session_id.clone()) {
            output.push((record, "ancestor"));
        }
    }
    if seen.insert(target.session_id.clone()) {
        output.push((target.clone(), "current"));
    }

    let children = list_direct_children(
        context,
        user_id,
        &target.session_id,
        agent_scope,
        status,
        limit,
    )?;
    for child in children {
        if seen.insert(child.session_id.clone()) {
            output.push((child, "child"));
        }
    }
    Ok(output)
}

fn resolve_fallback_main_session(
    context: &ToolContext<'_>,
    user_id: &str,
    agent_key: &str,
    exclude_session_id: &str,
) -> Result<Option<ChatSessionRecord>> {
    let items = list_sessions_by_status(
        context,
        user_id,
        Some(agent_key),
        None,
        Some(CHAT_SESSION_STATUS_ACTIVE),
        MAX_LIST_LIMIT,
    )?;
    Ok(items
        .into_iter()
        .find(|record| record.session_id.trim() != exclude_session_id.trim()))
}

fn apply_session_update(
    context: &ToolContext<'_>,
    user_id: &str,
    record: &ChatSessionRecord,
) -> Result<()> {
    context.storage.upsert_chat_session(record)?;
    let now = record.updated_at.max(record.last_message_at);
    let _ = context.storage.touch_chat_session(
        user_id,
        &record.session_id,
        now,
        record.last_message_at,
    );
    Ok(())
}

async fn list_threads(context: &ToolContext<'_>, args: ThreadControlArgs) -> Result<Value> {
    let user_id = require_user_id(context)?;
    let current = current_session_record(context, user_id)?;
    let target = normalize_optional_string(args.session_id.clone())
        .map(|session_id| load_session_record(context, user_id, &session_id))
        .transpose()?
        .or_else(|| current.clone());
    let agent_scope =
        resolve_agent_scope(&args, current.as_ref(), target.as_ref(), context.agent_id);
    if let Some(agent_key) = agent_scope.as_deref() {
        validate_agent_access(context, user_id, agent_key)?;
    }
    let status = normalize_status_filter(args.status.as_deref());
    let scope = normalize_scope(args.scope.as_deref());
    let limit = clamp_limit(args.limit, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT);
    let target_session_id = target.as_ref().map(|record| record.session_id.clone());

    let records = match scope {
        "children" => {
            let target = target
                .as_ref()
                .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
            list_direct_children(
                context,
                user_id,
                &target.session_id,
                agent_scope.as_deref(),
                status,
                limit,
            )?
            .into_iter()
            .map(|record| (record, "child"))
            .collect::<Vec<_>>()
        }
        "roots" => list_sessions_by_status(
            context,
            user_id,
            agent_scope.as_deref(),
            Some(""),
            status,
            limit,
        )?
        .into_iter()
        .map(|record| (record, "root"))
        .collect::<Vec<_>>(),
        "all" => list_sessions_by_status(
            context,
            user_id,
            agent_scope.as_deref(),
            None,
            status,
            limit,
        )?
        .into_iter()
        .map(|record| (record, "session"))
        .collect::<Vec<_>>(),
        _ => {
            let target = target
                .as_ref()
                .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
            collect_branch_records(
                context,
                user_id,
                &target,
                agent_scope.as_deref(),
                status,
                limit,
            )?
        }
    };

    let items = records
        .iter()
        .map(|(record, relation)| session_payload(context, user_id, record, Some(relation)))
        .collect::<Vec<_>>();
    let main_session_id = agent_scope
        .as_deref()
        .and_then(|agent_key| resolve_main_session_id(context, user_id, agent_key));
    Ok(build_thread_control_success(
        "list",
        format!("Listed {} threads.", items.len()),
        json!({
            "scope": scope,
            "status": status.unwrap_or("all"),
            "agent_id": agent_scope,
            "current_session_id": current.as_ref().map(|record| record.session_id.clone()),
            "target_session_id": target_session_id,
            "main_session_id": main_session_id,
            "total": items.len(),
            "items": items,
        }),
        None,
    ))
}

async fn info_thread(context: &ToolContext<'_>, args: ThreadControlArgs) -> Result<Value> {
    let user_id = require_user_id(context)?;
    let current = current_session_record(context, user_id)?;
    let session_id = normalize_optional_string(args.session_id.clone())
        .or_else(|| current.as_ref().map(|record| record.session_id.clone()))
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let record = load_session_record(context, user_id, &session_id)?;
    let agent_key = session_agent_key(&record);
    if let Some(explicit_agent_id) = normalize_optional_string(args.agent_id.clone()) {
        validate_agent_access(context, user_id, &explicit_agent_id)?;
        if explicit_agent_id != agent_key {
            return Err(anyhow!(i18n::t("error.permission_denied")));
        }
    }
    let parent = record
        .parent_session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|parent_id| load_session_record(context, user_id, parent_id))
        .transpose()?;
    let children = list_direct_children(
        context,
        user_id,
        &record.session_id,
        Some(agent_key.as_str()),
        None,
        MAX_LIST_LIMIT,
    )?;
    Ok(build_thread_control_success(
        "info",
        format!("Loaded thread {}.", record.session_id),
        json!({
            "session": session_payload(context, user_id, &record, Some("current")),
            "parent": parent.as_ref().map(|item| session_payload(context, user_id, item, Some("parent"))),
            "children": children
                .iter()
                .map(|item| session_payload(context, user_id, item, Some("child")))
                .collect::<Vec<_>>(),
            "main_session_id": resolve_main_session_id(context, user_id, &agent_key),
        }),
        None,
    ))
}

async fn create_thread(context: &ToolContext<'_>, args: ThreadControlArgs) -> Result<Value> {
    let user_id = require_user_id(context)?;
    let current = current_session_record(context, user_id)?;
    let parent_session_id = normalize_optional_string(args.parent_session_id.clone())
        .or_else(|| current.as_ref().map(|record| record.session_id.clone()));
    let parent = parent_session_id
        .as_deref()
        .map(|session_id| load_session_record(context, user_id, session_id))
        .transpose()?;
    let agent_scope =
        resolve_agent_scope(&args, current.as_ref(), parent.as_ref(), context.agent_id)
            .unwrap_or_default();
    validate_agent_access(context, user_id, &agent_scope)?;
    let title = normalize_optional_string(args.title.clone())
        .or_else(|| normalize_optional_string(args.label.clone()))
        .unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string());
    let now = now_ts();
    let record = ChatSessionRecord {
        session_id: format!("sess_{}", Uuid::new_v4().simple()),
        user_id: user_id.to_string(),
        title,
        status: CHAT_SESSION_STATUS_ACTIVE.to_string(),
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id: if agent_scope.trim().is_empty() {
            None
        } else {
            Some(agent_scope.clone())
        },
        tool_overrides: parent
            .as_ref()
            .map(|item| item.tool_overrides.clone())
            .unwrap_or_default(),
        parent_session_id: parent.as_ref().map(|item| item.session_id.clone()),
        parent_message_id: None,
        spawn_label: normalize_optional_string(args.label.clone()),
        spawned_by: Some(TOOL_THREAD_CONTROL_ALIAS.to_string()),
    };
    context.storage.upsert_chat_session(&record)?;
    let switch = args.switch.unwrap_or(true);
    let set_main = args.set_main.unwrap_or(switch);
    if set_main {
        let _ = bind_main_session(
            context,
            user_id,
            &agent_scope,
            &record.session_id,
            "thread_control_create",
        )?;
    }
    let session = session_payload(context, user_id, &record, Some("current"));
    let main_session = if set_main {
        Some(session.clone())
    } else {
        None
    };
    let switch_session = if switch { Some(session.clone()) } else { None };
    emit_thread_control_event(
        context,
        build_thread_control_event(
            "create",
            Some(session.clone()),
            main_session.clone(),
            switch_session.clone(),
            switch,
            set_main,
            Some(context.session_id),
        ),
    );
    Ok(build_thread_control_success(
        "create",
        format!("Created thread {}.", record.session_id),
        json!({
            "session": session,
            "main_session": main_session,
            "switch_session": switch_session,
            "switch": switch,
            "set_main": set_main,
        }),
        None,
    ))
}

async fn switch_thread(context: &ToolContext<'_>, args: ThreadControlArgs) -> Result<Value> {
    let user_id = require_user_id(context)?;
    let session_id = normalize_optional_string(args.session_id.clone())
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let record = load_session_record(context, user_id, &session_id)?;
    let agent_key = session_agent_key(&record);
    validate_agent_access(context, user_id, &agent_key)?;
    let set_main = args.set_main.unwrap_or(true);
    if set_main {
        let _ = bind_main_session(
            context,
            user_id,
            &agent_key,
            &record.session_id,
            "thread_control_switch",
        )?;
    }
    let session = session_payload(context, user_id, &record, Some("current"));
    let main_session = if set_main {
        Some(session.clone())
    } else {
        None
    };
    emit_thread_control_event(
        context,
        build_thread_control_event(
            "switch",
            Some(session.clone()),
            main_session.clone(),
            Some(session.clone()),
            true,
            set_main,
            Some(context.session_id),
        ),
    );
    Ok(build_thread_control_success(
        "switch",
        format!("Switched to thread {}.", record.session_id),
        json!({
            "session": session,
            "main_session": main_session,
            "switch_session": session,
            "switch": true,
            "set_main": set_main,
        }),
        None,
    ))
}

async fn back_thread(context: &ToolContext<'_>, args: ThreadControlArgs) -> Result<Value> {
    let user_id = require_user_id(context)?;
    let current = current_session_record(context, user_id)?;
    let session_id = normalize_optional_string(args.session_id.clone())
        .or_else(|| current.as_ref().map(|record| record.session_id.clone()))
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let record = load_session_record(context, user_id, &session_id)?;
    let parent_session_id = record
        .parent_session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    switch_thread(
        context,
        ThreadControlArgs {
            action: "switch".to_string(),
            session_id: Some(parent_session_id.to_string()),
            parent_session_id: None,
            agent_id: record.agent_id.clone(),
            title: None,
            label: None,
            scope: None,
            status: None,
            limit: None,
            switch: Some(true),
            set_main: args.set_main,
        },
    )
    .await
    .map(|mut value| {
        if let Value::Object(ref mut map) = value {
            map.insert("action".to_string(), json!("back"));
            if let Some(summary) = map.get_mut("summary") {
                *summary = json!("Switched back to the parent thread.");
            }
        }
        if let Some(data) = value.get_mut("data").and_then(Value::as_object_mut) {
            data.insert("via_back".to_string(), Value::Bool(true));
        }
        value
    })
}

async fn update_thread_title(context: &ToolContext<'_>, args: ThreadControlArgs) -> Result<Value> {
    let user_id = require_user_id(context)?;
    let current = current_session_record(context, user_id)?;
    let session_id = normalize_optional_string(args.session_id.clone())
        .or_else(|| current.as_ref().map(|record| record.session_id.clone()))
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let title = normalize_optional_string(args.title.clone())
        .ok_or_else(|| anyhow!(i18n::t("error.content_required")))?;
    let mut record = load_session_record(context, user_id, &session_id)?;
    record.title = title;
    record.updated_at = now_ts();
    if record.status.trim().is_empty()
        || !record
            .status
            .eq_ignore_ascii_case(CHAT_SESSION_STATUS_ARCHIVED)
    {
        record.status = CHAT_SESSION_STATUS_ACTIVE.to_string();
    }
    apply_session_update(context, user_id, &record)?;
    let session = session_payload(context, user_id, &record, Some("current"));
    emit_thread_control_event(
        context,
        build_thread_control_event(
            "update_title",
            Some(session.clone()),
            None,
            None,
            false,
            false,
            Some(context.session_id),
        ),
    );
    Ok(build_thread_control_success(
        "update_title",
        format!("Updated thread title for {}.", record.session_id),
        json!({
            "session": session,
        }),
        None,
    ))
}

async fn archive_thread(context: &ToolContext<'_>, args: ThreadControlArgs) -> Result<Value> {
    let user_id = require_user_id(context)?;
    let current = current_session_record(context, user_id)?;
    let session_id = normalize_optional_string(args.session_id.clone())
        .or_else(|| current.as_ref().map(|record| record.session_id.clone()))
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let mut record = load_session_record(context, user_id, &session_id)?;
    let agent_key = session_agent_key(&record);
    validate_agent_access(context, user_id, &agent_key)?;
    record.status = CHAT_SESSION_STATUS_ARCHIVED.to_string();
    record.updated_at = now_ts();
    apply_session_update(context, user_id, &record)?;

    let main_session_id = resolve_main_session_id(context, user_id, &agent_key);
    let fallback = if main_session_id.as_deref() == Some(record.session_id.as_str()) {
        resolve_fallback_main_session(context, user_id, &agent_key, &record.session_id)?
    } else {
        None
    };
    let switch = args.switch.unwrap_or(false);
    let main_session = if let Some(fallback) = fallback.as_ref() {
        let _ = bind_main_session(
            context,
            user_id,
            &agent_key,
            &fallback.session_id,
            "thread_control_archive",
        )?;
        Some(session_payload(context, user_id, fallback, Some("current")))
    } else {
        None
    };
    let switch_session = if switch { main_session.clone() } else { None };
    let session = session_payload(context, user_id, &record, Some("current"));
    emit_thread_control_event(
        context,
        build_thread_control_event(
            "archive",
            Some(session.clone()),
            main_session.clone(),
            switch_session.clone(),
            switch_session.is_some(),
            main_session.is_some(),
            Some(context.session_id),
        ),
    );
    Ok(build_thread_control_success(
        "archive",
        format!("Archived thread {}.", record.session_id),
        json!({
            "session": session,
            "main_session": main_session,
            "switch_session": switch_session,
            "switch": switch_session.is_some(),
        }),
        switch_session.is_some().then(|| {
            "The archived thread was also switched away from because a fallback session was chosen."
                .to_string()
        }),
    ))
}

async fn restore_thread(context: &ToolContext<'_>, args: ThreadControlArgs) -> Result<Value> {
    let user_id = require_user_id(context)?;
    let session_id = normalize_optional_string(args.session_id.clone())
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let mut record = load_session_record(context, user_id, &session_id)?;
    let agent_key = session_agent_key(&record);
    validate_agent_access(context, user_id, &agent_key)?;
    record.status = CHAT_SESSION_STATUS_ACTIVE.to_string();
    record.updated_at = now_ts();
    apply_session_update(context, user_id, &record)?;

    let current_main =
        resolve_main_session_id(context, user_id, &agent_key).and_then(|main_session_id| {
            context
                .storage
                .get_chat_session(user_id, &main_session_id)
                .ok()
                .flatten()
        });
    let set_main = args.set_main.unwrap_or_else(|| {
        current_main
            .as_ref()
            .map(|item| {
                item.status
                    .eq_ignore_ascii_case(CHAT_SESSION_STATUS_ARCHIVED)
            })
            .unwrap_or(true)
    });
    if set_main {
        let _ = bind_main_session(
            context,
            user_id,
            &agent_key,
            &record.session_id,
            "thread_control_restore",
        )?;
    }
    let switch = args.switch.unwrap_or(false);
    let session = session_payload(context, user_id, &record, Some("current"));
    let main_session = if set_main {
        Some(session.clone())
    } else {
        None
    };
    let switch_session = if switch { Some(session.clone()) } else { None };
    emit_thread_control_event(
        context,
        build_thread_control_event(
            "restore",
            Some(session.clone()),
            main_session.clone(),
            switch_session.clone(),
            switch,
            set_main,
            Some(context.session_id),
        ),
    );
    Ok(build_thread_control_success(
        "restore",
        format!("Restored thread {}.", record.session_id),
        json!({
            "session": session,
            "main_session": main_session,
            "switch_session": switch_session,
            "switch": switch,
            "set_main": set_main,
        }),
        None,
    ))
}

async fn set_main_thread(context: &ToolContext<'_>, args: ThreadControlArgs) -> Result<Value> {
    let user_id = require_user_id(context)?;
    let current = current_session_record(context, user_id)?;
    let session_id = normalize_optional_string(args.session_id.clone())
        .or_else(|| current.as_ref().map(|record| record.session_id.clone()))
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let record = load_session_record(context, user_id, &session_id)?;
    let agent_key = session_agent_key(&record);
    validate_agent_access(context, user_id, &agent_key)?;
    let _ = bind_main_session(
        context,
        user_id,
        &agent_key,
        &record.session_id,
        "thread_control_set_main",
    )?;
    let switch = args.switch.unwrap_or(false);
    let session = session_payload(context, user_id, &record, Some("current"));
    let switch_session = if switch { Some(session.clone()) } else { None };
    emit_thread_control_event(
        context,
        build_thread_control_event(
            "set_main",
            Some(session.clone()),
            Some(session.clone()),
            switch_session.clone(),
            switch,
            true,
            Some(context.session_id),
        ),
    );
    Ok(build_thread_control_success(
        "set_main",
        format!("Set thread {} as main.", record.session_id),
        json!({
            "session": session,
            "main_session": session,
            "switch_session": switch_session,
            "switch": switch,
            "set_main": true,
        }),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a_store::A2aStore;
    use crate::config::Config;
    use crate::lsp::LspManager;
    use crate::skills::SkillRegistry;
    use crate::storage::{
        SqliteStorage, StorageBackend, UserAgentRecord, DEFAULT_HIVE_ID,
        DEFAULT_SANDBOX_CONTAINER_ID,
    };
    use crate::workspace::WorkspaceManager;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;

    struct TestHarness {
        _dir: tempfile::TempDir,
        config: Config,
        storage: Arc<dyn StorageBackend>,
        workspace: Arc<WorkspaceManager>,
        lsp_manager: Arc<LspManager>,
        a2a_store: A2aStore,
        skills: SkillRegistry,
        http: reqwest::Client,
    }

    impl TestHarness {
        fn new() -> Self {
            let dir = tempdir().expect("tempdir");
            let db_path = dir.path().join("thread-control-tool.db");
            let storage: Arc<dyn StorageBackend> =
                Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
            storage.ensure_initialized().expect("init storage");
            let config = Config::default();
            let workspace = Arc::new(WorkspaceManager::new(
                dir.path().to_string_lossy().as_ref(),
                storage.clone(),
                0,
                &HashMap::new(),
            ));
            let lsp_manager = LspManager::new(workspace.clone());
            Self {
                _dir: dir,
                config,
                storage,
                workspace,
                lsp_manager,
                a2a_store: A2aStore::default(),
                skills: SkillRegistry::default(),
                http: reqwest::Client::new(),
            }
        }

        fn context<'a>(
            &'a self,
            user_id: &'a str,
            session_id: &'a str,
            agent_id: Option<&'a str>,
        ) -> ToolContext<'a> {
            ToolContext {
                user_id,
                session_id,
                workspace_id: "workspace-test",
                agent_id,
                user_round: None,
                model_round: None,
                is_admin: false,
                storage: self.storage.clone(),
                orchestrator: None,
                monitor: None,
                beeroom_realtime: None,
                workspace: self.workspace.clone(),
                lsp_manager: self.lsp_manager.clone(),
                config: &self.config,
                a2a_store: &self.a2a_store,
                skills: &self.skills,
                gateway: None,
                user_world: None,
                cron_wake_signal: None,
                user_tool_manager: None,
                user_tool_bindings: None,
                user_tool_store: None,
                request_config_overrides: None,
                allow_roots: None,
                read_roots: None,
                command_sessions: None,
                event_emitter: None,
                http: &self.http,
            }
        }

        fn upsert_agent(&self, user_id: &str, agent_id: &str) {
            self.storage
                .upsert_user_agent(&UserAgentRecord {
                    agent_id: agent_id.to_string(),
                    user_id: user_id.to_string(),
                    hive_id: DEFAULT_HIVE_ID.to_string(),
                    name: agent_id.to_string(),
                    description: String::new(),
                    system_prompt: String::new(),
                    model_name: None,
                    ability_items: Vec::new(),
                    tool_names: Vec::new(),
                    declared_tool_names: Vec::new(),
                    declared_skill_names: Vec::new(),
                    preset_questions: Vec::new(),
                    access_level: "private".to_string(),
                    approval_mode: "auto".to_string(),
                    is_shared: false,
                    status: "active".to_string(),
                    icon: None,
                    sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
                    created_at: now_ts(),
                    updated_at: now_ts(),
                    preset_binding: None,
                    silent: false,
                    prefer_mother: false,
                })
                .expect("upsert agent");
        }

        fn upsert_session(&self, record: ChatSessionRecord) {
            self.storage
                .upsert_chat_session(&record)
                .expect("upsert session");
        }
    }

    #[test]
    fn normalize_thread_control_action_accepts_aliases() {
        assert_eq!(normalize_action("switch"), "switch");
        assert_eq!(normalize_action("open"), "switch");
        assert_eq!(normalize_action("新建"), "create");
        assert_eq!(normalize_action("设为主线程"), "set_main");
    }

    #[tokio::test]
    async fn create_thread_inherits_parent_agent_and_bind_main() {
        let harness = TestHarness::new();
        harness.upsert_agent("u1", "agent-demo");
        let parent = ChatSessionRecord {
            session_id: "sess_parent".to_string(),
            user_id: "u1".to_string(),
            title: "parent".to_string(),
            status: CHAT_SESSION_STATUS_ACTIVE.to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
            last_message_at: now_ts(),
            agent_id: Some("agent-demo".to_string()),
            tool_overrides: vec!["thread_control".to_string()],
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        harness.upsert_session(parent.clone());
        let context = harness.context("u1", "sess_parent", Some("agent-demo"));

        let result = execute_thread_control_tool(&context, &json!({ "action": "create" }))
            .await
            .expect("create thread");
        let created_id = result
            .pointer("/data/session")
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
            .expect("created id");
        let created = harness
            .storage
            .get_chat_session("u1", created_id)
            .expect("load created")
            .expect("created record");
        assert_eq!(created.parent_session_id.as_deref(), Some("sess_parent"));
        assert_eq!(created.agent_id.as_deref(), Some("agent-demo"));
        assert_eq!(created.tool_overrides, vec!["thread_control".to_string()]);
        let main_thread = harness
            .storage
            .get_agent_thread("u1", "agent-demo")
            .expect("get agent thread")
            .expect("main thread");
        assert_eq!(main_thread.session_id, created.session_id);
    }

    #[tokio::test]
    async fn back_thread_switches_to_parent_session() {
        let harness = TestHarness::new();
        harness.upsert_agent("u1", "agent-demo");
        harness.upsert_session(ChatSessionRecord {
            session_id: "sess_parent".to_string(),
            user_id: "u1".to_string(),
            title: "parent".to_string(),
            status: CHAT_SESSION_STATUS_ACTIVE.to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
            last_message_at: now_ts(),
            agent_id: Some("agent-demo".to_string()),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        });
        harness.upsert_session(ChatSessionRecord {
            session_id: "sess_child".to_string(),
            user_id: "u1".to_string(),
            title: "child".to_string(),
            status: CHAT_SESSION_STATUS_ACTIVE.to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
            last_message_at: now_ts(),
            agent_id: Some("agent-demo".to_string()),
            tool_overrides: Vec::new(),
            parent_session_id: Some("sess_parent".to_string()),
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        });
        let context = harness.context("u1", "sess_child", Some("agent-demo"));

        let result = execute_thread_control_tool(&context, &json!({ "action": "back" }))
            .await
            .expect("back thread");
        let switch_id = result
            .pointer("/data/switch_session")
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str);
        assert_eq!(switch_id, Some("sess_parent"));
    }
}
