// API 请求与响应数据结构，保持与现有接口字段一致。
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct WunderRequest {
    pub user_id: String,
    pub question: String,
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
    pub model_name: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub config_overrides: Option<Value>,
    #[serde(default)]
    pub agent_prompt: Option<String>,
    #[serde(default)]
    pub attachments: Option<Vec<AttachmentPayload>>,
}

fn default_stream() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AttachmentPayload {
    pub name: Option<String>,
    pub content: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WunderResponse {
    pub session_id: String,
    pub answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a2ui: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WunderPromptResponse {
    pub prompt: String,
    pub build_time_ms: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WunderPromptRequest {
    pub user_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub config_overrides: Option<Value>,
    #[serde(default)]
    pub agent_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AvailableToolsResponse {
    pub builtin_tools: Vec<ToolSpec>,
    pub mcp_tools: Vec<ToolSpec>,
    pub a2a_tools: Vec<ToolSpec>,
    pub skills: Vec<ToolSpec>,
    pub knowledge_tools: Vec<ToolSpec>,
    pub user_tools: Vec<ToolSpec>,
    pub shared_tools: Vec<SharedToolSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_tools_selected: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct SharedToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub owner_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    #[serde(rename = "input_tokens")]
    pub input: u64,
    #[serde(rename = "output_tokens")]
    pub output: u64,
    #[serde(rename = "total_tokens")]
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct I18nConfigResponse {
    pub default_language: String,
    pub supported_languages: Vec<String>,
    pub aliases: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    pub event: String,
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
}
