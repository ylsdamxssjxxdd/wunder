use super::*;
use crate::core::approval::ApprovalRequestTx;
use uuid::Uuid;

#[derive(Clone)]
pub(super) struct PreparedRequest {
    pub(super) user_id: String,
    pub(super) workspace_id: String,
    pub(super) question: String,
    pub(super) client_message_id: Option<String>,
    pub(super) session_id: String,
    pub(super) tool_names: Option<Vec<String>>,
    pub(super) skip_tool_calls: bool,
    pub(super) model_name: Option<String>,
    pub(super) config_overrides: Option<Value>,
    pub(super) agent_prompt: Option<String>,
    pub(super) preview_skill: bool,
    pub(super) agent_id: Option<String>,
    pub(super) stream: bool,
    pub(super) attachments: Option<Vec<AttachmentPayload>>,
    pub(super) language: String,
    pub(super) allow_queue: bool,
    pub(super) is_admin: bool,
    pub(super) enforce_runtime_queue: bool,
    pub(super) approval_tx: Option<ApprovalRequestTx>,
    pub(super) thread_turn_id: Option<Uuid>,
    pub(super) thread_user_round: Option<i64>,
    /// Durable cursor observed immediately before accepting this turn.
    /// Change-stream feeders start here so the accept transaction's changes
    /// are replayed instead of skipped.
    pub(super) thread_resume_from_seq: i64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RoundInfo {
    pub(super) user_round: Option<i64>,
    pub(super) model_round: Option<i64>,
    pub(super) thread_turn_id: Option<Uuid>,
    /// True while the runtime drives an armed goal: the turn was admitted by the
    /// goal driver rather than by a direct human message. Providers use it to
    /// avoid re-issuing goal-management calls that require human authority.
    pub(super) is_goal_round: bool,
}

impl RoundInfo {
    pub(super) fn new(user_round: i64, model_round: i64) -> Self {
        Self {
            user_round: Some(user_round),
            model_round: Some(model_round),
            thread_turn_id: None,
            is_goal_round: false,
        }
    }

    pub(super) fn user_only(user_round: i64) -> Self {
        Self {
            user_round: Some(user_round),
            model_round: None,
            thread_turn_id: None,
            is_goal_round: false,
        }
    }

    pub(super) fn user_only_thread(user_round: i64, thread_turn_id: Uuid) -> Self {
        Self {
            user_round: Some(user_round),
            model_round: None,
            thread_turn_id: Some(thread_turn_id),
            is_goal_round: false,
        }
    }

    pub(super) fn with_model_round(self, model_round: i64) -> Self {
        Self {
            user_round: self.user_round,
            model_round: Some(model_round),
            thread_turn_id: self.thread_turn_id,
            is_goal_round: self.is_goal_round,
        }
    }

    pub(super) fn insert_into(&self, map: &mut Map<String, Value>) {
        if let Some(user_round) = self.user_round {
            map.insert("user_round".to_string(), json!(user_round));
        }
        if let Some(model_round) = self.model_round {
            map.insert("model_round".to_string(), json!(model_round));
        }
        if let Some(turn_id) = self.thread_turn_id {
            map.insert("turn_id".to_string(), json!(turn_id.to_string()));
        }
    }
}
