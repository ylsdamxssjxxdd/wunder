use super::*;

#[derive(Clone)]
pub(super) struct PreparedRequest {
    pub(super) user_id: String,
    pub(super) workspace_id: String,
    pub(super) question: String,
    pub(super) session_id: String,
    pub(super) tool_names: Option<Vec<String>>,
    pub(super) skip_tool_calls: bool,
    pub(super) model_name: Option<String>,
    pub(super) config_overrides: Option<Value>,
    pub(super) agent_prompt: Option<String>,
    pub(super) agent_id: Option<String>,
    pub(super) stream: bool,
    pub(super) debug_payload: bool,
    pub(super) attachments: Option<Vec<AttachmentPayload>>,
    pub(super) language: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RoundInfo {
    pub(super) user_round: Option<i64>,
    pub(super) model_round: Option<i64>,
}

impl RoundInfo {
    pub(super) fn new(user_round: i64, model_round: i64) -> Self {
        Self {
            user_round: Some(user_round),
            model_round: Some(model_round),
        }
    }

    pub(super) fn user_only(user_round: i64) -> Self {
        Self {
            user_round: Some(user_round),
            model_round: None,
        }
    }

    pub(super) fn insert_into(&self, map: &mut Map<String, Value>) {
        if let Some(user_round) = self.user_round {
            map.insert("user_round".to_string(), json!(user_round));
        }
        if let Some(model_round) = self.model_round {
            map.insert("model_round".to_string(), json!(model_round));
        }
    }
}
