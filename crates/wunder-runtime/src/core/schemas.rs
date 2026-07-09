use crate::core::approval::ApprovalRequestTx;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use wunder_core::schemas::{
    AbilityDescriptor, AbilityGroupKey, AbilityKind, AbilitySourceKey, AttachmentPayload,
    AvailableToolsResponse, I18nConfigResponse, SharedToolSpec, StreamEvent, TokenUsage, ToolSpec,
    WunderPromptRequest, WunderPromptResponse, WunderResponse,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WunderRequest {
    pub user_id: String,
    pub question: String,
    #[serde(default, alias = "clientMessageId")]
    pub client_message_id: Option<String>,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub skip_tool_calls: bool,
    #[serde(default = "default_stream")]
    pub stream: bool,
    #[serde(default)]
    pub debug_payload: bool,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default, alias = "workspaceContainerId")]
    pub workspace_container_id: Option<i32>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub config_overrides: Option<Value>,
    #[serde(default)]
    pub agent_prompt: Option<String>,
    #[serde(default)]
    pub preview_skill: bool,
    #[serde(default)]
    pub attachments: Option<Vec<AttachmentPayload>>,
    #[serde(default = "default_allow_queue")]
    pub allow_queue: bool,
    #[serde(skip)]
    pub is_admin: bool,
    #[serde(skip)]
    pub enforce_runtime_queue: bool,
    #[serde(skip)]
    pub approval_tx: Option<ApprovalRequestTx>,
}

fn default_stream() -> bool {
    true
}

fn default_allow_queue() -> bool {
    true
}
