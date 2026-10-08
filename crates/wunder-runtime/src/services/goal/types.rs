//! Goal domain vocabulary: phases, refs, snapshots, views, error codes.
//! Mirrors the deepseek goal domain: full snapshot per mutation, CAS on
//! revision, phase lifecycle `active|paused|blocked|complete`, and a
//! process-local activation that never persists.

use crate::storage::SessionGoalRecord;
use serde::{Deserialize, Serialize};

pub const PHASE_ACTIVE: &str = "active";
pub const PHASE_PAUSED: &str = "paused";
pub const PHASE_BLOCKED: &str = "blocked";
pub const PHASE_COMPLETE: &str = "complete";

/// Stable error codes for rejected goal reads and mutations.
pub const ERR_GOAL_NOT_FOUND: &str = "GOAL_NOT_FOUND";
pub const ERR_GOAL_ALREADY_EXISTS: &str = "GOAL_ALREADY_EXISTS";
pub const ERR_GOAL_STALE_REVISION: &str = "GOAL_STALE_REVISION";
pub const ERR_GOAL_INVALID_OBJECTIVE: &str = "GOAL_INVALID_OBJECTIVE";
pub const ERR_GOAL_INVALID_MAX_ROUNDS: &str = "GOAL_INVALID_MAX_ROUNDS";
pub const ERR_GOAL_INVALID_BLOCK_REASON: &str = "GOAL_INVALID_BLOCK_REASON";
pub const ERR_GOAL_INVALID_EDIT: &str = "GOAL_INVALID_EDIT";
pub const ERR_GOAL_INVALID_TRANSITION: &str = "GOAL_INVALID_TRANSITION";

/// Tool-policy error codes surfaced through goal tool rejections.
pub const ERR_TOOL_AUTHORITY_REQUIRED: &str = "GOAL_TOOL_AUTHORITY_REQUIRED";
pub const ERR_TOOL_INVALID_UPDATE: &str = "GOAL_TOOL_INVALID_UPDATE";
pub const ERR_TOOL_RESUME_PAUSED: &str = "GOAL_TOOL_RESUME_PAUSED";
pub const ERR_TOOL_BLOCK_THRESHOLD: &str = "GOAL_TOOL_BLOCK_THRESHOLD";

/// Minimum admitted goal rounds before the model may self-report `blocked`.
pub const BLOCKED_AFTER_CONSECUTIVE_ROUNDS: i64 = 3;
/// Total rounds used when a create request omits its own cap.
pub const DEFAULT_MAX_GOAL_ROUNDS: i64 = 256;
/// Objective length bound shared by every entry point.
pub const MAX_OBJECTIVE_CHARS: usize = 4000;

/// Durable continuation phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalPhase {
    Active,
    Paused,
    Blocked,
    Complete,
}

impl GoalPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => PHASE_ACTIVE,
            Self::Paused => PHASE_PAUSED,
            Self::Blocked => PHASE_BLOCKED,
            Self::Complete => PHASE_COMPLETE,
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            PHASE_ACTIVE => Some(Self::Active),
            PHASE_PAUSED => Some(Self::Paused),
            PHASE_BLOCKED => Some(Self::Blocked),
            PHASE_COMPLETE => Some(Self::Complete),
            _ => None,
        }
    }
}

/// Machine-routable and human-readable explanation for a blocked goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalBlockReason {
    /// Stable lower-kebab-case classification chosen by the blocking policy.
    pub code: String,
    /// Non-empty explanation shown to humans and models.
    pub message: String,
}

/// Whether this live process may automatically continue an active goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalActivation {
    Armed,
    Disarmed,
}

impl GoalActivation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Armed => "armed",
            Self::Disarmed => "disarmed",
        }
    }
}

/// Compare-and-set identity for one exact goal revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalRef {
    pub goal_id: String,
    pub revision: i64,
}

/// Message attribution for admitted continuation rounds, carried in the
/// request overrides and the durable transcript meta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalRoundTag {
    pub goal_id: String,
    pub revision: i64,
    pub round: i64,
}

/// Current goal projection including the process-local activation and the
/// replay counter `rounds_started`.
#[derive(Debug, Clone)]
pub struct GoalView {
    pub record: SessionGoalRecord,
    pub activation: GoalActivation,
}

impl GoalView {
    pub fn phase(&self) -> GoalPhase {
        GoalPhase::parse(&self.record.phase).unwrap_or(GoalPhase::Active)
    }

    pub fn blocked_reason(&self) -> Option<GoalBlockReason> {
        if self.phase() != GoalPhase::Blocked {
            return None;
        }
        Some(GoalBlockReason {
            code: self
                .record
                .blocked_code
                .clone()
                .unwrap_or_else(|| "blocked".into()),
            message: self.record.blocked_message.clone().unwrap_or_default(),
        })
    }
}

/// Structured domain rejection rendered into tool errors and API responses.
#[derive(Debug, Clone)]
pub struct GoalError {
    pub message: String,
    pub code: &'static str,
}

impl std::fmt::Display for GoalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for GoalError {}

pub fn goal_error(message: impl Into<String>, code: &'static str) -> GoalError {
    GoalError {
        message: message.into(),
        code,
    }
}
