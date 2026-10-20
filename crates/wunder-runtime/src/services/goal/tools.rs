//! Model-facing `goal` action tool over the persisted same-session goal domain.

use super::authority::{
    completion_authority, read_goal_round_tag, require_direct_human, GoalToolAuthority,
};
use super::prompt::render_wrapup_context;
use super::service::{validate_blocked_reason, GoalService};
use super::types::*;
use crate::schemas::ToolSpec;
use crate::services::tools::ToolContext;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

pub const TOOL_GOAL: &str = "goal";

const GOAL_DESCRIPTION: &str = "Manage the persisted goal for this session. Select exactly one action: get reads the current goal; create starts a long-running objective; edit changes its objective; pause/resume controls automatic continuation; complete marks verified work done; blocked reports a persistent blocker. State-changing actions require the current goal_id and revision returned by get, except create. Human-only actions are checked by the runtime; complete and blocked may also be used in the admitted goal round.";

pub fn is_goal_tool_name(name: &str) -> bool {
    name.trim() == TOOL_GOAL
}

pub fn goal_tool_specs() -> Vec<ToolSpec> {
    let action = |action: &str, properties: Value, required: &[&str]| {
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
            "description": format!("goal action={action}"),
        })
    };
    let base_ref = json!({
        "goal_id": {"type":"string", "minLength":1, "description":"Exact id returned by action=get."},
        "revision": {"type":"integer", "minimum":1, "description":"Exact positive revision returned by action=get."},
    });
    let mut edit_props = base_ref.clone();
    edit_props["objective"] = json!({"type":"string", "minLength":1, "maxLength":MAX_OBJECTIVE_CHARS, "description":"Replacement objective."});
    edit_props["max_goal_rounds"] = json!({"type":"integer", "minimum":1, "maximum":10000, "description":"Optional replacement continuation-round cap."});
    let mut blocked_props = base_ref.clone();
    blocked_props["blocked_reason"] = json!({"type":"string", "minLength":1, "maxLength":MAX_BLOCK_REASON_CHARS, "description":"Concrete blocker that persisted across goal rounds."});
    let create_props = json!({
        "objective": {"type":"string", "minLength":1, "maxLength":MAX_OBJECTIVE_CHARS, "description":"Concrete completion objective."},
        "max_goal_rounds": {"type":"integer", "minimum":1, "maximum":10000, "description":"Optional continuation-round cap."}
    });
    vec![ToolSpec {
        name: TOOL_GOAL.to_string(),
        title: Some("Goal".to_string()),
        description: GOAL_DESCRIPTION.to_string(),
        input_schema: json!({
            "type": "object",
            "oneOf": [
                action("get", json!({"action":{"const":"get"}}), &["action"]),
                action("create", json!({"action":{"const":"create"}, "objective": create_props["objective"], "max_goal_rounds": create_props["max_goal_rounds"]}), &["action", "objective"]),
                action("edit", json!({"action":{"const":"edit"}, "goal_id": edit_props["goal_id"], "revision": edit_props["revision"], "objective": edit_props["objective"], "max_goal_rounds": edit_props["max_goal_rounds"]}), &["action", "goal_id", "revision", "objective"]),
                action("pause", json!({"action":{"const":"pause"}, "goal_id": base_ref["goal_id"], "revision": base_ref["revision"]}), &["action", "goal_id", "revision"]),
                action("resume", json!({"action":{"const":"resume"}, "goal_id": base_ref["goal_id"], "revision": base_ref["revision"]}), &["action", "goal_id", "revision"]),
                action("complete", json!({"action":{"const":"complete"}, "goal_id": base_ref["goal_id"], "revision": base_ref["revision"]}), &["action", "goal_id", "revision"]),
                action("blocked", json!({"action":{"const":"blocked"}, "goal_id": blocked_props["goal_id"], "revision": blocked_props["revision"], "blocked_reason": blocked_props["blocked_reason"]}), &["action", "goal_id", "revision", "blocked_reason"]),
            ]
        }),
    }]
}

/// Resolve the goal domain behind the current tool execution.
pub fn goal_service_from_context(context: &ToolContext<'_>) -> Result<Arc<GoalService>> {
    let orchestrator = context
        .orchestrator
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("goal tools require the orchestrator"))?;
    Ok(orchestrator.goal_handle())
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn has_text(args: &Value, key: &str) -> bool {
    arg_str(args, key).is_some()
}

fn has_round_cap(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_i64).is_some()
}

fn goal_ref(args: &Value) -> Result<GoalRef> {
    let goal_id = arg_str(args, "goal_id").unwrap_or_default();
    let revision = args.get("revision").and_then(Value::as_i64).unwrap_or(0);
    if goal_id.is_empty() || goal_id != goal_id.trim() || revision < 1 {
        return Err(GoalError {
            message: "goal_id must be non-empty and revision must be a positive safe integer"
                .to_string(),
            code: ERR_TOOL_INVALID_UPDATE,
        }
        .into());
    }
    Ok(GoalRef {
        goal_id: goal_id.to_string(),
        revision,
    })
}

/// Execute one goal tool. Output is the compact deepseek-shaped JSON value;
/// wrap-up notices ride the returned payload via `__wrapup`.
pub async fn execute_goal_tool(
    context: &ToolContext<'_>,
    name: &str,
    args: &Value,
) -> Result<Value> {
    if name.trim() != TOOL_GOAL {
        return Err(anyhow::anyhow!("unknown goal tool: {}", name.trim()));
    }
    let service = goal_service_from_context(context)?;
    let storage = context.storage.clone();
    let current = service
        .get_view(&storage, context.user_id, context.session_id)
        .await
        .ok()
        .flatten();
    let overrides = context.request_config_overrides;
    match args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "get" => {
            ensure_action_fields(args, &["action"])?;
            Ok(current
                .as_ref()
                .map(super::service::goal_tool_value)
                .unwrap_or_else(super::service::goal_tool_value_none))
        }
        "create" => {
            ensure_action_fields(args, &["action", "objective", "max_goal_rounds"])?;
            require_direct_human(overrides)?;
            let objective = arg_str(args, "objective")
                .ok_or_else(|| GoalError {
                    message: "objective is required".to_string(),
                    code: ERR_GOAL_INVALID_OBJECTIVE,
                })?
                .to_string();
            let max_goal_rounds = optional_i64(args, "max_goal_rounds")?;
            let view = service
                .create(
                    storage,
                    context.user_id,
                    context.session_id,
                    &objective,
                    max_goal_rounds,
                )
                .await?;
            Ok(super::service::goal_tool_value(&view))
        }
        "edit" | "pause" | "resume" | "complete" | "blocked" => {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let mut allowed = vec!["action", "goal_id", "revision"];
            match action {
                "edit" => allowed.extend(["objective", "max_goal_rounds"]),
                "blocked" => allowed.push("blocked_reason"),
                _ => {}
            }
            ensure_action_fields(args, &allowed)?;
            execute_goal_action(
                &service,
                storage,
                context,
                args,
                current.as_ref(),
                overrides,
            )
            .await
        }
        other => Err(GoalError {
            message: format!("invalid goal action: {other}"),
            code: ERR_TOOL_INVALID_UPDATE,
        }
        .into()),
    }
}

fn ensure_action_fields(args: &Value, allowed: &[&str]) -> Result<()> {
    let Some(map) = args.as_object() else {
        return Err(anyhow::anyhow!("goal arguments must be an object"));
    };
    if let Some(extra) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(GoalError {
            message: format!("field {extra} is not valid for this goal action"),
            code: ERR_TOOL_INVALID_UPDATE,
        }
        .into());
    }
    Ok(())
}

fn optional_i64(args: &Value, key: &str) -> Result<Option<i64>> {
    match args.get(key) {
        None => Ok(None),
        Some(value) => value.as_i64().map(Some).ok_or_else(|| {
            GoalError {
                message: format!("{key} must be an integer"),
                code: ERR_GOAL_INVALID_MAX_ROUNDS,
            }
            .into()
        }),
    }
}

async fn execute_goal_action(
    service: &Arc<GoalService>,
    storage: std::sync::Arc<dyn crate::storage::StorageBackend>,
    context: &ToolContext<'_>,
    args: &Value,
    current: Option<&GoalView>,
    overrides: Option<&Value>,
) -> Result<Value> {
    let reference = goal_ref(args)?;
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let view = match action.as_str() {
        "edit" => {
            require_direct_human(overrides)?;
            if has_text(args, "blocked_reason") {
                return Err(GoalError {
                    message: "blocked_reason is valid only with action blocked".to_string(),
                    code: ERR_TOOL_INVALID_UPDATE,
                }
                .into());
            }
            service
                .edit(
                    storage,
                    context.user_id,
                    context.session_id,
                    &reference,
                    arg_str(args, "objective").unwrap_or_default(),
                    optional_i64(args, "max_goal_rounds")?,
                )
                .await?
        }
        "pause" | "resume" => {
            require_direct_human(overrides)?;
            if has_text(args, "objective")
                || has_round_cap(args, "max_goal_rounds")
                || has_text(args, "blocked_reason")
            {
                return Err(GoalError {
                    message: "objective and max_goal_rounds are valid only with action edit; blocked_reason is valid only with action blocked".to_string(),
                    code: ERR_TOOL_INVALID_UPDATE,
                }
                .into());
            }
            if action == "resume" {
                if let Some(view) = current {
                    if view.record.goal_id == reference.goal_id
                        && view.record.revision == reference.revision
                        && view.record.phase == PHASE_PAUSED
                    {
                        return Err(GoalError {
                            message:
                                "the model cannot resume a paused goal; the user must resume it"
                                    .to_string(),
                            code: ERR_TOOL_RESUME_PAUSED,
                        }
                        .into());
                    }
                }
            }
            if action == "pause" {
                service
                    .pause(storage, context.user_id, context.session_id, &reference)
                    .await?
            } else {
                service
                    .resume(storage, context.user_id, context.session_id, &reference)
                    .await?
            }
        }
        "complete" | "blocked" => {
            let authority = completion_authority(current, overrides)?;
            if has_text(args, "objective") || has_round_cap(args, "max_goal_rounds") {
                return Err(GoalError {
                    message: "objective and max_goal_rounds are valid only with action edit"
                        .to_string(),
                    code: ERR_TOOL_INVALID_UPDATE,
                }
                .into());
            }
            if action == "complete" && has_text(args, "blocked_reason") {
                return Err(GoalError {
                    message: "blocked_reason is valid only with action blocked".to_string(),
                    code: ERR_TOOL_INVALID_UPDATE,
                }
                .into());
            }
            if action == "blocked" && !has_text(args, "blocked_reason") {
                return Err(GoalError {
                    message: "blocked_reason is required with action blocked".to_string(),
                    code: ERR_TOOL_INVALID_UPDATE,
                }
                .into());
            }
            if action == "blocked" {
                if let GoalToolAuthority::GoalRound(view) = &authority {
                    if view.record.rounds_started < BLOCKED_AFTER_CONSECUTIVE_ROUNDS {
                        return Err(GoalError {
                            message: format!(
                                "blocked requires at least {BLOCKED_AFTER_CONSECUTIVE_ROUNDS} consecutive goal rounds; current round is {}",
                                view.record.rounds_started
                            ),
                            code: ERR_TOOL_BLOCK_THRESHOLD,
                        }
                        .into());
                    }
                }
            }
            let blocked_reason = if action == "blocked" {
                Some(validate_blocked_reason(
                    arg_str(args, "blocked_reason").unwrap_or_default(),
                )?)
            } else {
                None
            };
            let view = if action == "complete" {
                service
                    .complete(storage, context.user_id, context.session_id, &reference)
                    .await?
            } else {
                service
                    .block(
                        storage,
                        context.user_id,
                        context.session_id,
                        &reference,
                        GoalBlockReason {
                            code: "model-reported".to_string(),
                            message: blocked_reason.clone().unwrap_or_default(),
                        },
                    )
                    .await?
            };
            if matches!(authority, GoalToolAuthority::GoalRound(_)) {
                let value = super::service::goal_tool_value(&view);
                return Ok(attach_wrapup(
                    value,
                    &view,
                    &action,
                    blocked_reason.as_deref(),
                ));
            }
            view
        }
        other => {
            return Err(GoalError {
                message: format!("invalid goal action: {other}"),
                code: ERR_TOOL_INVALID_UPDATE,
            }
            .into())
        }
    };
    Ok(super::service::goal_tool_value(&view))
}

/// Attach the wrap-up instruction the orchestrator injects as a follow-up
/// user message for the current turn (deepseek deferContext equivalent).
fn attach_wrapup(
    mut value: Value,
    view: &GoalView,
    action: &str,
    blocked_reason: Option<&str>,
) -> Value {
    if let Some(map) = value.as_object_mut() {
        map.insert(
            "__wrapup".to_string(),
            json!({
                "kind": action,
                "objective": view.record.objective,
                "blocked_reason": blocked_reason,
                "text": render_wrapup_context(&view.record.objective, blocked_reason),
            }),
        );
    }
    value
}

/// Whether a tool result carries a goal wrap-up instruction.
pub fn read_wrapup(result: &Value) -> Option<String> {
    result
        .get("__wrapup")
        .and_then(|wrapup| wrapup.get("text"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Whether the current turn is an admitted goal round.
pub fn is_goal_round_turn(context: &ToolContext<'_>) -> bool {
    read_goal_round_tag(context.request_config_overrides).is_some()
}
