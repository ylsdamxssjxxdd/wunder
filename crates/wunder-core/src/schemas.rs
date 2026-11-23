use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AttachmentPayload {
    pub name: Option<String>,
    pub content: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "publicPath")]
    pub public_path: Option<String>,
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
    #[serde(default)]
    pub preview_skill: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AvailableToolsResponse {
    pub builtin_tools: Vec<ToolSpec>,
    pub mcp_tools: Vec<ToolSpec>,
    pub skills: Vec<ToolSpec>,
    pub knowledge_tools: Vec<ToolSpec>,
    pub user_tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub admin_builtin_tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub admin_mcp_tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub admin_skills: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub admin_knowledge_tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_mcp_tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_skills: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_knowledge_tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_agent_tool_names: Vec<String>,
    pub shared_tools: Vec<SharedToolSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_tools_selected: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<AbilityDescriptor>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AbilityKind {
    #[default]
    Tool,
    Skill,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AbilityGroupKey {
    Skills,
    Mcp,
    Knowledge,
    User,
    Shared,
    Builtin,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AbilitySourceKey {
    Builtin,
    Mcp,
    Skill,
    Knowledge,
    UserMcp,
    UserSkill,
    UserKnowledge,
    Shared,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct AbilityDescriptor {
    pub id: String,
    pub name: String,
    pub runtime_name: String,
    pub display_name: String,
    pub description: String,
    pub input_schema: Value,
    pub group: AbilityGroupKey,
    pub source: AbilitySourceKey,
    pub kind: AbilityKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Uncached prompt tokens. Providers that fold cache reads into the prompt
    /// total (OpenAI / DeepSeek) are split here so the three prompt buckets stay
    /// disjoint and a hit rate is computable.
    #[serde(rename = "input_tokens")]
    pub input: u64,
    #[serde(rename = "output_tokens")]
    pub output: u64,
    #[serde(rename = "total_tokens")]
    pub total: u64,
    /// Known reasoning tokens are excluded from output but included in total.
    /// None means the provider did not supply a separate reasoning count.
    #[serde(
        default,
        rename = "reasoning_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning: Option<u64>,
    /// Prompt tokens served from the provider cache (billed at the cached rate).
    #[serde(
        default,
        rename = "cache_read_tokens",
        skip_serializing_if = "crate::schemas::is_zero_u64"
    )]
    pub cache_read: u64,
    /// Prompt tokens written into the provider cache this request (Anthropic).
    #[serde(
        default,
        rename = "cache_write_tokens",
        skip_serializing_if = "crate::schemas::is_zero_u64"
    )]
    pub cache_write: u64,
    #[serde(default)]
    pub estimated: bool,
}

pub(crate) fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

impl TokenUsage {
    /// Full prompt size as the provider saw it, cache buckets included.
    /// Context occupancy must use this: `input` alone is the uncached part.
    pub fn prompt_tokens(&self) -> u64 {
        self.input
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    /// Share of the billed prompt served from cache, or `None` when the
    /// provider reported no prompt tokens at all.
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let prompt = self.prompt_tokens();
        (prompt > 0).then(|| self.cache_read as f64 / prompt as f64)
    }
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
