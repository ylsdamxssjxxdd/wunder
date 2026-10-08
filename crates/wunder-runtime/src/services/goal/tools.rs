//! Model-facing `get_goal`, `create_goal`, and `update_goal` tools over the
//! persisted same-session goal domain.

use super::authority::{
    completion_authority, read_goal_round_tag, require_direct_human, GoalToolAuthority,
};
use super::prompt::render_wrapup_context;
use super::service::GoalService;
use super::types::*;
use crate::schemas::ToolSpec;
use crate::services::tools::ToolContext;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

pub const TOOL_GET_GOAL: &str = "get_goal";
pub const TOOL_CREATE_GOAL: &str = "create_goal";
pub const TOOL_UPDATE_GOAL: &str = "update_goal";

const CREATE_DESCRIPTION: &str = "Create a persisted goal that keeps this session working across \
automatic continuation rounds. Use it when the direct human request is a long-running objective, \
even if the user did not say \"goal\"; not for single-turn work.";

const GET_DESCRIPTION: &str =
    "Read the current session goal, including the id and revision that update_goal requires.";

const UPDATE_DESCRIPTION: &str = "Update the current goal.";

pub fn is_goal_tool_name(name: &str) -> bool {
    matches!(
        name.trim(),
        TOOL_GET_GOAL | TOOL_CREATE_GOAL | TOOL_UPDATE_GOAL
    )
}

pub fn goal_tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: TOOL_GET_GOAL.to_string(),
            title: Some("Get Goal".to_string()),
            description: GET_DESCRIPTION.to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: TOOL_CREATE_GOAL.to_string(),
            title: Some("Create Goal".to_string()),
            description: CREATE_DESCRIPTION.to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "objective": {
                        "type": "string",
                        "description": "The concrete completion objective inferred from the direct human request."
                    },
                    "max_goal_rounds": {
                        "type": "integer",
                        "description": "Optional positive safe-integer limit on automatic continuation rounds."
                    }
                },
                "required": ["objective"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: TOOL_UPDATE_GOAL.to_string(),
            title: Some("Update Goal".to_string()),
            description: UPDATE_DESCRIPTION.to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "goal_id": {
                        "type": "string",
                        "description": "Exact id returned by get_goal."
                    },
                    "revision": {
                        "type": "integer",
                        "description": "Exact positive revision returned by get_goal."
                    },
                    "action": {
                        "type": "string",
                        "enum": ["edit", "pause", "resume", "complete", "blocked"],
                        "description": "edit, pause, and resume require a direct top-level human request. complete and blocked are also allowed during an automatic continuation of this goal; blocked is rejected before the configured minimum round count."
                    },
                    "objective": {
                        "type": "string",
                        "description": "Replacement objective; valid only with action edit."
                    },
                    "max_goal_rounds": {
                        "type": "integer",
                        "description": "Replacement cap; valid only with action edit."
                    },
                    "blocked_reason": {
                        "type": "string",
                        "description": "Required only with action blocked: the concrete condition that persisted across rounds and blocks progress."
                    }
                },
                "required": ["goal_id", "revision", "action"],
                "additionalProperties": false
            }),
        },
    ]
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
    let service = goal_service_from_context(context)?;
    let storage = context.storage.clone();
    let current = service
        .get_view(&storage, context.user_id, context.session_id)
        .await
        .ok()
        .flatten();
    let overrides = context.request_config_overrides;
    match name.trim() {
        TOOL_GET_GOAL => Ok(current
            .as_ref()
            .map(super::service::goal_tool_value)
            .unwrap_or_else(super::service::goal_tool_value_none)),
        TOOL_CREATE_GOAL => {
            require_direct_human(overrides)?;
            let objective = arg_str(args, "objective")
                .ok_or_else(|| GoalError {
                    message: "objective is required".to_string(),
                    code: ERR_GOAL_INVALID_OBJECTIVE,
                })?
                .to_string();
            let max_goal_rounds = args.get("max_goal_rounds").and_then(Value::as_i64);
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
        TOOL_UPDATE_GOAL => {
            execute_update_goal(
                &service,
                storage,
                context,
                args,
                current.as_ref(),
                overrides,
            )
            .await
        }
        other => Err(anyhow::anyhow!("unknown goal tool: {other}")),
    }
}

async fn execute_update_goal(
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
                    args.get("max_goal_rounds").and_then(Value::as_i64),
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
                            message: arg_str(args, "blocked_reason")
                                .unwrap_or_default()
                                .to_string(),
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
                    arg_str(args, "blocked_reason"),
                ));
            }
            view
        }
        other => {
            return Err(GoalError {
                message: format!("invalid update_goal action: {other}"),
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
