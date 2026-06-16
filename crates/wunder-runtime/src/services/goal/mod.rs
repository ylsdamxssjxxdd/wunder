use crate::core::blocking;
use crate::schemas::WunderRequest;
use crate::services::stream_events::StreamEventService;
use crate::services::subagents;
use crate::services::tools::ToolContext;
use crate::storage::{ChatSessionRecord, SessionGoalRecord, StorageBackend};
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub const STATUS_ACTIVE: &str = "active";
pub const STATUS_PAUSED: &str = "paused";
pub const STATUS_BUDGET_LIMITED: &str = "budget_limited";
pub const STATUS_COMPLETE: &str = "complete";

pub const SOURCE_API: &str = "api";
#[allow(dead_code)]
pub const SOURCE_CLI: &str = "cli";
pub const SOURCE_MODEL: &str = "model";
pub const SOURCE_SYSTEM: &str = "system";

pub const TOOL_GOAL: &str = "goal";
pub const TOOL_GOAL_GET_LEGACY: &str = "get_goal";
pub const TOOL_GOAL_CREATE_LEGACY: &str = "create_goal";
pub const TOOL_GOAL_UPDATE_LEGACY: &str = "update_goal";

pub const EVENT_GOAL_UPDATED: &str = "goal_updated";
pub const EVENT_GOAL_CLEARED: &str = "goal_cleared";
pub const EVENT_GOAL_CONTINUATION_STARTED: &str = "goal_continuation_started";
pub const EVENT_GOAL_BUDGET_LIMITED: &str = "goal_budget_limited";

pub const GOAL_CONTINUATION_CONFIG_KEY: &str = "_goal_continuation";

const MAX_OBJECTIVE_CHARS: usize = 4000;
const CONTINUATION_COOLDOWN_S: f64 = 1.0;
const DEFAULT_TITLE: &str = "新会话";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    Active,
    Paused,
    BudgetLimited,
    Complete,
}

impl GoalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => STATUS_ACTIVE,
            Self::Paused => STATUS_PAUSED,
            Self::BudgetLimited => STATUS_BUDGET_LIMITED,
            Self::Complete => STATUS_COMPLETE,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::BudgetLimited | Self::Complete)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalCommand {
    Show,
    Set {
        objective: String,
        token_budget: Option<i64>,
    },
    Pause,
    Resume,
    Clear,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoalUpsertPayload {
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default, alias = "tokenBudget", alias = "token_budget")]
    pub token_budget: Option<i64>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoalContinuationPlan {
    pub should_start: bool,
    pub reason: String,
    pub session_id: String,
    pub goal_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GoalContinuationRequest {
    pub request: WunderRequest,
    pub goal: SessionGoalRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalToolAction {
    Get,
    Create,
    Update,
}

pub fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

async fn run_goal_db<T, F>(label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    blocking::run_db(label, task).await
}

pub fn normalize_status(raw: &str) -> Result<GoalStatus> {
    match raw.trim().to_ascii_lowercase().as_str() {
        STATUS_ACTIVE => Ok(GoalStatus::Active),
        STATUS_PAUSED => Ok(GoalStatus::Paused),
        STATUS_BUDGET_LIMITED => Ok(GoalStatus::BudgetLimited),
        STATUS_COMPLETE => Ok(GoalStatus::Complete),
        _ => Err(anyhow!("invalid goal status")),
    }
}

pub fn goal_tool_name() -> &'static str {
    TOOL_GOAL
}

pub fn is_goal_tool_name(name: &str) -> bool {
    matches!(
        name.trim(),
        TOOL_GOAL | TOOL_GOAL_GET_LEGACY | TOOL_GOAL_CREATE_LEGACY | TOOL_GOAL_UPDATE_LEGACY
    )
}

pub fn goal_tool_specs() -> Vec<crate::schemas::ToolSpec> {
    vec![
        crate::schemas::ToolSpec {
            name: TOOL_GOAL.to_string(),
            title: Some("Goal".to_string()),
            description: "Manage the active session goal. Supported actions: get the current goal, create a new goal when none exists, and mark the current goal complete."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["get", "create", "update"],
                        "description": "Goal action to perform."
                    },
                    "objective": {
                        "type": "string",
                        "description": "Concrete objective to keep working toward. Required for action=create."
                    },
                    "token_budget": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Optional token budget for the goal. Allowed only for action=create."
                    },
                    "status": {
                        "type": "string",
                        "enum": [STATUS_COMPLETE],
                        "description": "Required for action=update. Only complete is allowed."
                    },
                    "summary": {
                        "type": "string",
                        "description": "Optional concise completion summary for action=update."
                    }
                },
                "required": ["action"],
                "additionalProperties": false
            }),
        },
    ]
}

#[allow(dead_code)]
pub fn parse_goal_command(args: &str) -> Result<GoalCommand> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(GoalCommand::Show);
    }
    let parts =
        shell_words::split(args).map_err(|err| anyhow!("parse /goal args failed: {err}"))?;
    let Some(first) = parts.first().map(|value| value.trim().to_ascii_lowercase()) else {
        return Ok(GoalCommand::Show);
    };
    match first.as_str() {
        "pause" => return Ok(GoalCommand::Pause),
        "resume" => return Ok(GoalCommand::Resume),
        "clear" => return Ok(GoalCommand::Clear),
        _ => {}
    }

    let mut token_budget = None;
    let mut objective_parts = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        let part = parts[index].trim();
        if part == "--tokens" {
            let Some(raw_budget) = parts.get(index + 1) else {
                return Err(anyhow!("missing token budget"));
            };
            let budget = raw_budget
                .parse::<i64>()
                .map_err(|_| anyhow!("invalid token budget"))?;
            if budget <= 0 {
                return Err(anyhow!("token budget must be positive"));
            }
            token_budget = Some(budget);
            index += 2;
            continue;
        }
        objective_parts.push(parts[index].clone());
        index += 1;
    }
    let objective = validate_objective(objective_parts.join(" "))?;
    Ok(GoalCommand::Set {
        objective,
        token_budget,
    })
}

pub fn validate_objective(objective: impl AsRef<str>) -> Result<String> {
    let cleaned = objective.as_ref().trim();
    if cleaned.is_empty() {
        return Err(anyhow!("goal objective is required"));
    }
    if cleaned.chars().count() > MAX_OBJECTIVE_CHARS {
        return Err(anyhow!("goal objective is too long"));
    }
    Ok(cleaned.to_string())
}

pub fn goal_payload(record: &SessionGoalRecord) -> Value {
    json!({
        "goal_id": record.goal_id,
        "session_id": record.session_id,
        "user_id": record.user_id,
        "objective": record.objective,
        "status": record.status,
        "token_budget": record.token_budget,
        "tokens_used": record.tokens_used,
        "time_used_seconds": record.time_used_seconds,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "completed_at": record.completed_at,
        "last_continued_at": record.last_continued_at,
        "source": record.source,
    })
}

pub async fn get_goal(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_id: &str,
) -> Result<Option<SessionGoalRecord>> {
    let user_id = user_id.trim().to_string();
    let session_id = session_id.trim().to_string();
    run_goal_db("goal.get", move || {
        storage.get_session_goal(&user_id, &session_id)
    })
    .await
}

pub async fn list_goals(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_ids: &[String],
) -> Result<Vec<SessionGoalRecord>> {
    let user_id = user_id.trim().to_string();
    let session_ids = session_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    run_goal_db("goal.list", move || {
        storage.list_session_goals(&user_id, &session_ids)
    })
    .await
}

pub async fn set_goal(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_id: &str,
    objective: &str,
    token_budget: Option<i64>,
    source: &str,
) -> Result<SessionGoalRecord> {
    let objective = validate_objective(objective)?;
    let user_id = clean_required(user_id, "user id")?;
    let session_id = clean_required(session_id, "session id")?;
    let now = now_ts();
    let record = SessionGoalRecord {
        goal_id: Uuid::new_v4().simple().to_string(),
        session_id: session_id.clone(),
        user_id: user_id.clone(),
        objective,
        status: STATUS_ACTIVE.to_string(),
        token_budget: token_budget.filter(|value| *value > 0),
        tokens_used: 0,
        time_used_seconds: 0,
        created_at: now,
        updated_at: now,
        completed_at: None,
        last_continued_at: None,
        source: source.trim().to_string(),
    };
    let storage_for_write = storage.clone();
    let record_for_write = record.clone();
    run_goal_db("goal.set.upsert", move || {
        storage_for_write.upsert_session_goal(&record_for_write)
    })
    .await?;
    emit_goal_event(storage, &record, EVENT_GOAL_UPDATED, None).await;
    Ok(record)
}

pub async fn set_goal_status(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_id: &str,
    status: GoalStatus,
    source: &str,
) -> Result<SessionGoalRecord> {
    let user_id = clean_required(user_id, "user id")?;
    let session_id = clean_required(session_id, "session id")?;
    let storage_for_read = storage.clone();
    let read_user = user_id.clone();
    let read_session = session_id.clone();
    let Some(mut record) = run_goal_db("goal.status.get", move || {
        storage_for_read.get_session_goal(&read_user, &read_session)
    })
    .await?
    else {
        return Err(anyhow!("goal not found"));
    };
    record.status = status.as_str().to_string();
    record.updated_at = now_ts();
    record.source = source.trim().to_string();
    record.completed_at = if status == GoalStatus::Complete {
        Some(record.updated_at)
    } else {
        None
    };
    let storage_for_write = storage.clone();
    let record_for_write = record.clone();
    run_goal_db("goal.status.upsert", move || {
        storage_for_write.upsert_session_goal(&record_for_write)
    })
    .await?;
    let event = if status == GoalStatus::BudgetLimited {
        EVENT_GOAL_BUDGET_LIMITED
    } else {
        EVENT_GOAL_UPDATED
    };
    emit_goal_event(storage, &record, event, None).await;
    Ok(record)
}

pub async fn clear_goal(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_id: &str,
) -> Result<bool> {
    let user_id = clean_required(user_id, "user id")?;
    let session_id = clean_required(session_id, "session id")?;
    let existing = get_goal(storage.clone(), &user_id, &session_id).await?;
    let storage_for_delete = storage.clone();
    let delete_user = user_id.clone();
    let delete_session = session_id.clone();
    let affected = run_goal_db("goal.clear.delete", move || {
        storage_for_delete.delete_session_goal(&delete_user, &delete_session)
    })
    .await?;
    if let Some(record) = existing.as_ref() {
        emit_goal_event(storage, record, EVENT_GOAL_CLEARED, None).await;
    }
    Ok(affected > 0)
}

pub async fn mark_goal_continuation_started(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_id: &str,
) -> Result<Option<SessionGoalRecord>> {
    let user_id = clean_required(user_id, "user id")?;
    let session_id = clean_required(session_id, "session id")?;
    let storage_for_read = storage.clone();
    let read_user = user_id.clone();
    let read_session = session_id.clone();
    let Some(mut record) = run_goal_db("goal.continuation.get", move || {
        storage_for_read.get_session_goal(&read_user, &read_session)
    })
    .await?
    else {
        return Ok(None);
    };
    if normalize_status(&record.status)? != GoalStatus::Active {
        return Ok(Some(record));
    }
    record.last_continued_at = Some(now_ts());
    record.updated_at = record.last_continued_at.unwrap_or(record.updated_at);
    let storage_for_write = storage.clone();
    let record_for_write = record.clone();
    run_goal_db("goal.continuation.upsert", move || {
        storage_for_write.upsert_session_goal(&record_for_write)
    })
    .await?;
    emit_goal_event(storage, &record, EVENT_GOAL_CONTINUATION_STARTED, None).await;
    Ok(Some(record))
}

pub fn should_continue_goal(record: &SessionGoalRecord, waiting_for_user_input: bool) -> bool {
    continuation_delay_seconds(record, waiting_for_user_input)
        .map(|delay| delay <= f64::EPSILON)
        .unwrap_or(false)
}

pub fn continuation_delay_seconds(
    record: &SessionGoalRecord,
    waiting_for_user_input: bool,
) -> Option<f64> {
    if waiting_for_user_input {
        return None;
    }
    if normalize_status(&record.status).ok() != Some(GoalStatus::Active) {
        return None;
    }
    if record
        .token_budget
        .map(|budget| budget > 0 && record.tokens_used >= budget)
        .unwrap_or(false)
    {
        return None;
    }
    let now = now_ts();
    Some(
        record
            .last_continued_at
            .map(|last| (CONTINUATION_COOLDOWN_S - (now - last)).max(0.0))
            .unwrap_or(0.0),
    )
}

pub async fn account_turn_usage(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_id: &str,
    tokens_delta: u64,
    time_delta_seconds: i64,
) -> Result<Option<SessionGoalRecord>> {
    let user_id = clean_required(user_id, "user id")?;
    let session_id = clean_required(session_id, "session id")?;
    let tokens_delta = tokens_delta.min(i64::MAX as u64) as i64;
    let time_delta_seconds = time_delta_seconds.max(0);
    if tokens_delta <= 0 && time_delta_seconds <= 0 {
        return get_goal(storage, &user_id, &session_id).await;
    }
    let storage_for_write = storage.clone();
    let write_user = user_id.clone();
    let write_session = session_id.clone();
    let Some(record) = run_goal_db("goal.account_usage", move || {
        storage_for_write.account_session_goal_usage(
            &write_user,
            &write_session,
            tokens_delta,
            time_delta_seconds,
            now_ts(),
        )
    })
    .await?
    else {
        return Ok(None);
    };
    if normalize_status(&record.status).ok() == Some(GoalStatus::Active)
        && record
            .token_budget
            .map(|budget| budget > 0 && record.tokens_used >= budget)
            .unwrap_or(false)
    {
        return set_goal_status(
            storage,
            &user_id,
            &session_id,
            GoalStatus::BudgetLimited,
            SOURCE_SYSTEM,
        )
        .await
        .map(Some);
    }
    Ok(Some(record))
}

pub fn is_goal_continuation(config_overrides: Option<&Value>) -> bool {
    config_overrides
        .and_then(|value| value.get(GOAL_CONTINUATION_CONFIG_KEY))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn build_goal_continuation_overrides(base: Option<&Value>) -> Value {
    let mut payload = subagents::build_auto_wake_request_overrides(base);
    if let Some(map) = payload.as_object_mut() {
        map.insert(GOAL_CONTINUATION_CONFIG_KEY.to_string(), Value::Bool(true));
    }
    payload
}

pub async fn build_continuation_request_from_session(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session: &ChatSessionRecord,
    tool_names: Vec<String>,
) -> Result<Option<GoalContinuationRequest>> {
    let Some(goal) = get_goal(storage.clone(), user_id, &session.session_id).await? else {
        return Ok(None);
    };
    if !should_continue_goal(&goal, false) {
        return Ok(None);
    }
    let question = build_continuation_prompt(&goal);
    let request = WunderRequest {
        user_id: user_id.trim().to_string(),
        question,
        tool_names,
        skip_tool_calls: false,
        stream: true,
        debug_payload: false,
        session_id: Some(session.session_id.clone()),
        agent_id: session.agent_id.clone(),
        workspace_container_id: None,
        model_name: None,
        language: Some(crate::i18n::get_language()),
        config_overrides: Some(build_goal_continuation_overrides(None)),
        agent_prompt: None,
        preview_skill: false,
        attachments: None,
        allow_queue: true,
        is_admin: false,
        approval_tx: None,
    };
    Ok(Some(GoalContinuationRequest { request, goal }))
}

pub fn build_continuation_prompt(goal: &SessionGoalRecord) -> String {
    format!(
        "[GOAL_CONTINUATION]\nContinue working toward the active goal until it is complete.\nCurrent goal: {}\nWhen the goal is fully complete, call goal with action=update and status=complete. If you are blocked by missing user input, ask one concise question and stop.",
        goal.objective.trim()
    )
}

pub async fn execute_goal_tool(
    context: &ToolContext<'_>,
    name: &str,
    args: &Value,
) -> Result<Value> {
    let action = resolve_goal_tool_action(name, args)?;
    match action {
        GoalToolAction::Get => {
            let goal =
                get_goal(context.storage.clone(), context.user_id, context.session_id).await?;
            Ok(json!({
                "ok": true,
                "data": { "goal": goal.as_ref().map(goal_payload) }
            }))
        }
        GoalToolAction::Create => {
            let objective = args
                .get("objective")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("objective is required"))?;
            let token_budget = args.get("token_budget").and_then(Value::as_i64);
            let existing =
                get_goal(context.storage.clone(), context.user_id, context.session_id).await?;
            if let Some(existing) = existing.as_ref() {
                let status = normalize_status(&existing.status)?;
                return Ok(json!({
                    "ok": false,
                    "error": if status.is_terminal() {
                        "goal already finished; ask the user to start a new goal"
                    } else {
                        "goal already exists"
                    },
                    "data": { "goal": goal_payload(existing) }
                }));
            }
            let record = set_goal(
                context.storage.clone(),
                context.user_id,
                context.session_id,
                objective,
                token_budget,
                SOURCE_MODEL,
            )
            .await?;
            Ok(json!({ "ok": true, "data": { "goal": goal_payload(&record) } }))
        }
        GoalToolAction::Update => {
            let status = args
                .get("status")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("status is required"))?;
            if normalize_status(status)? != GoalStatus::Complete {
                return Ok(json!({
                    "ok": false,
                    "error": "update_goal only allows status=complete"
                }));
            }
            let record = set_goal_status(
                context.storage.clone(),
                context.user_id,
                context.session_id,
                GoalStatus::Complete,
                SOURCE_MODEL,
            )
            .await?;
            Ok(json!({ "ok": true, "data": { "goal": goal_payload(&record) } }))
        }
    }
}

fn resolve_goal_tool_action(name: &str, args: &Value) -> Result<GoalToolAction> {
    match name.trim() {
        TOOL_GOAL_GET_LEGACY => return Ok(GoalToolAction::Get),
        TOOL_GOAL_CREATE_LEGACY => return Ok(GoalToolAction::Create),
        TOOL_GOAL_UPDATE_LEGACY => return Ok(GoalToolAction::Update),
        TOOL_GOAL => {}
        _ => return Err(anyhow!("unknown goal tool: {name}")),
    }
    match args
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some("get") => Ok(GoalToolAction::Get),
        Some("create") => Ok(GoalToolAction::Create),
        Some("update") => Ok(GoalToolAction::Update),
        Some(_) => Err(anyhow!("invalid goal action")),
        None => Ok(GoalToolAction::Get),
    }
}

pub fn tool_names_contain_goal_tool(names: &[String]) -> bool {
    names.iter().any(|name| is_goal_tool_name(name))
}

pub async fn emit_goal_event(
    storage: Arc<dyn StorageBackend>,
    record: &SessionGoalRecord,
    event: &str,
    extra: Option<Value>,
) {
    let mut payload = json!({
        "event": event,
        "data": { "goal": goal_payload(record) },
        "timestamp": Utc::now().to_rfc3339(),
    });
    if let (Some(Value::Object(extra)), Some(map)) = (extra, payload.get_mut("data")) {
        if let Some(target) = map.as_object_mut() {
            for (key, value) in extra {
                target.insert(key, value);
            }
        }
    }
    let service = StreamEventService::new(storage);
    let _ = service
        .append_event(&record.session_id, &record.user_id, payload)
        .await;
}

pub async fn ensure_session(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_id: &str,
    agent_id: Option<&str>,
) -> Result<ChatSessionRecord> {
    let user_id = clean_required(user_id, "user id")?;
    let session_id = clean_required(session_id, "session id")?;
    if let Some(existing) = load_session(storage.clone(), &user_id, &session_id).await? {
        return Ok(existing);
    }
    let now = now_ts();
    let record = ChatSessionRecord {
        session_id,
        user_id,
        title: DEFAULT_TITLE.to_string(),
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id: agent_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        tool_overrides: Vec::new(),
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    };
    let storage_for_write = storage;
    let record_for_write = record.clone();
    run_goal_db("goal.ensure_session.upsert", move || {
        storage_for_write.upsert_chat_session(&record_for_write)
    })
    .await?;
    Ok(record)
}

async fn load_session(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    session_id: &str,
) -> Result<Option<ChatSessionRecord>> {
    let user_id = user_id.trim().to_string();
    let session_id = session_id.trim().to_string();
    run_goal_db("goal.load_session", move || {
        storage.get_chat_session(&user_id, &session_id)
    })
    .await
}

fn clean_required(value: &str, label: &str) -> Result<String> {
    let cleaned = value.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("{label} is required"));
    }
    Ok(cleaned.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_goal_show_when_empty() {
        assert_eq!(parse_goal_command("").unwrap(), GoalCommand::Show);
    }

    #[test]
    fn parse_goal_controls() {
        assert_eq!(parse_goal_command("pause").unwrap(), GoalCommand::Pause);
        assert_eq!(parse_goal_command("resume").unwrap(), GoalCommand::Resume);
        assert_eq!(parse_goal_command("clear").unwrap(), GoalCommand::Clear);
    }

    #[test]
    fn parse_goal_set_with_budget() {
        assert_eq!(
            parse_goal_command("--tokens 120 finish task").unwrap(),
            GoalCommand::Set {
                objective: "finish task".to_string(),
                token_budget: Some(120),
            }
        );
    }

    #[test]
    fn status_normalization_rejects_unknown() {
        assert_eq!(normalize_status("active").unwrap(), GoalStatus::Active);
        assert!(normalize_status("waiting").is_err());
    }

    #[test]
    fn goal_tool_spec_uses_single_goal_tool() {
        let specs = goal_tool_specs();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, TOOL_GOAL);
        assert_eq!(
            specs[0].input_schema["properties"]["action"]["enum"],
            json!(["get", "create", "update"])
        );
    }

    #[test]
    fn goal_tool_name_detection_supports_legacy_aliases() {
        assert!(is_goal_tool_name(TOOL_GOAL));
        assert!(is_goal_tool_name(TOOL_GOAL_GET_LEGACY));
        assert!(is_goal_tool_name(TOOL_GOAL_CREATE_LEGACY));
        assert!(is_goal_tool_name(TOOL_GOAL_UPDATE_LEGACY));
        assert!(!is_goal_tool_name("read_file"));
    }
}
