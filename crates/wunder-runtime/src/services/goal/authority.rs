//! Execution-time authority checks for the model-facing goal tools.
//!
//! deepseek attests authority through the session log (direct `user` sources
//! and `goal`-sourced round messages). wunder encodes the same facts on the
//! admitted turn: human chat entry points stamp `__source = "human"` onto the
//! request overrides, and the goal-round driver stamps `__goal_round`. The
//! model can never set either, so the attestation stays host-owned.

use super::types::*;
use serde_json::{json, Value};

/// Overrides key stamped on requests submitted by human chat entry points.
pub const SOURCE_HUMAN_CONFIG_KEY: &str = "__source";
pub const SOURCE_HUMAN_VALUE: &str = "human";
/// Overrides key stamped on admitted goal-round requests.
pub const GOAL_ROUND_CONFIG_KEY: &str = "__goal_round";

/// Stamp host-attested human origin onto a request's overrides.
pub fn mark_human_source(config_overrides: &mut Option<Value>) {
    let value = config_overrides.get_or_insert_with(|| json!({}));
    if let Some(map) = value.as_object_mut() {
        map.insert(
            SOURCE_HUMAN_CONFIG_KEY.to_string(),
            json!(SOURCE_HUMAN_VALUE),
        );
    }
}

/// Whether the current turn originated from a direct human request.
pub fn has_direct_human_source(config_overrides: Option<&Value>) -> bool {
    config_overrides
        .and_then(|value| value.get(SOURCE_HUMAN_CONFIG_KEY))
        .and_then(Value::as_str)
        .map(str::trim)
        == Some(SOURCE_HUMAN_VALUE)
}

/// Read the goal-round tag admitted for the current turn, if any.
pub fn read_goal_round_tag(config_overrides: Option<&Value>) -> Option<GoalRoundTag> {
    let value = config_overrides?.get(GOAL_ROUND_CONFIG_KEY)?;
    let tag: GoalRoundTag = serde_json::from_value(value.clone()).ok()?;
    if tag.round < 1 || tag.revision < 1 || tag.goal_id.trim().is_empty() {
        return None;
    }
    Some(tag)
}

/// Merge the goal-round tag into request overrides (auto-wake base included).
pub fn goal_round_overrides(base: Option<&Value>, tag: &GoalRoundTag) -> Value {
    let mut payload = crate::services::subagents::build_auto_wake_request_overrides(base);
    if let Some(map) = payload.as_object_mut() {
        map.insert(
            GOAL_ROUND_CONFIG_KEY.to_string(),
            serde_json::to_value(tag).unwrap_or(Value::Null),
        );
    }
    payload
}

/// Hard authority granted to one state-changing goal tool call.
#[derive(Debug, Clone)]
pub enum GoalToolAuthority {
    DirectHuman,
    /// The current turn is the goal's exact admitted round.
    GoalRound(GoalView),
}

impl GoalToolAuthority {
    pub fn goal(&self) -> Option<&GoalView> {
        match self {
            Self::DirectHuman => None,
            Self::GoalRound(view) => Some(view),
        }
    }
}

fn reject(message: impl Into<String>, code: &'static str) -> GoalError {
    goal_error(message, code)
}

/// Whether the current turn is the current goal's exact admitted round.
fn is_matching_goal_round(tag: &GoalRoundTag, goal: &GoalView) -> bool {
    tag.goal_id == goal.record.goal_id
        && tag.revision == goal.record.revision
        && tag.round == goal.record.rounds_started
}

/// Resolve completion authority from either direct human input or the exact
/// admitted goal round.
pub fn completion_authority(
    goal: Option<&GoalView>,
    config_overrides: Option<&Value>,
) -> Result<GoalToolAuthority, GoalError> {
    if has_direct_human_source(config_overrides) {
        return Ok(GoalToolAuthority::DirectHuman);
    }
    let tag = read_goal_round_tag(config_overrides);
    if let (Some(tag), Some(goal)) = (tag, goal) {
        if is_matching_goal_round(&tag, goal) {
            return Ok(GoalToolAuthority::GoalRound(goal.clone()));
        }
    }
    Err(reject(
        "complete and blocked require a direct human turn or the current goal round",
        ERR_TOOL_AUTHORITY_REQUIRED,
    ))
}

/// Require authority originating in a host-attested human request.
pub fn require_direct_human(config_overrides: Option<&Value>) -> Result<(), GoalError> {
    if has_direct_human_source(config_overrides) {
        return Ok(());
    }
    Err(reject(
        "this goal operation requires a direct human turn on a top-level session",
        ERR_TOOL_AUTHORITY_REQUIRED,
    ))
}
