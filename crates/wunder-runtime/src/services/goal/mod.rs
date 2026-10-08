//! Same-session goal domain, a full replication of the deepseek goal
//! subsystem: durable snapshots with CAS-on-revision, phase lifecycle
//! `active|paused|blocked|complete`, process-local activation, three
//! model-facing tools with execution-time authority, an idle-driven
//! round driver with per-session round caps, and the `/goal` command face.

pub mod authority;
pub mod command;
pub mod driver;
pub mod prompt;
pub mod service;
pub mod tools;
pub mod types;

#[cfg(test)]
mod tests;

pub use authority::{
    goal_round_overrides, has_direct_human_source, mark_human_source, read_goal_round_tag,
};
pub use command::{execute_goal_command, parse_goal_command, GoalCommand, USAGE};
pub use driver::{GoalDriverHost, TurnEndOutcome};
pub use service::{
    goal_payload, goal_tool_value, goal_tool_value_none, validate_objective, GoalService,
};
pub use tools::{
    execute_goal_tool, goal_tool_specs, is_goal_tool_name, read_wrapup, TOOL_CREATE_GOAL,
    TOOL_GET_GOAL, TOOL_UPDATE_GOAL,
};
pub use types::{
    GoalActivation, GoalBlockReason, GoalPhase, GoalRef, GoalRoundTag, GoalView,
    BLOCKED_AFTER_CONSECUTIVE_ROUNDS, DEFAULT_MAX_GOAL_ROUNDS, ERR_GOAL_ALREADY_EXISTS,
    ERR_GOAL_INVALID_MAX_ROUNDS, ERR_GOAL_INVALID_OBJECTIVE, ERR_GOAL_NOT_FOUND,
    ERR_GOAL_STALE_REVISION, PHASE_ACTIVE, PHASE_BLOCKED, PHASE_COMPLETE, PHASE_PAUSED,
};
