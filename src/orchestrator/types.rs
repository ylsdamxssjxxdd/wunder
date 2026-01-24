use super::*;

#[derive(Clone)]
pub(super) struct PreparedRequest {
    pub(super) user_id: String,
    pub(super) question: String,
    pub(super) session_id: String,
    pub(super) tool_names: Option<Vec<String>>,
    pub(super) skip_tool_calls: bool,
    pub(super) model_name: Option<String>,
    pub(super) config_overrides: Option<Value>,
    pub(super) stream: bool,
    pub(super) debug_payload: bool,
    pub(super) attachments: Option<Vec<AttachmentPayload>>,
    pub(super) language: String,
}
