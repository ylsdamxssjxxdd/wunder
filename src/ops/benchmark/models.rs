use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct BenchmarkStartRequest {
    pub user_id: String,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub judge_model_name: Option<String>,
    #[serde(default)]
    pub suite_ids: Vec<String>,
    #[serde(default)]
    pub task_ids: Vec<String>,
    #[serde(default)]
    pub runs_per_task: Option<u32>,
    #[serde(default)]
    pub capture_artifacts: Option<bool>,
    #[serde(default)]
    pub capture_transcript: Option<bool>,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub config_overrides: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkEvent {
    pub event: String,
    pub data: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttemptUsage {
    #[serde(default)]
    pub context_tokens: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub request_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub name: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub timestamp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub name: String,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub timestamp: f64,
    #[serde(default)]
    pub raw: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionCapture {
    #[serde(default)]
    pub transcript: Vec<Value>,
    #[serde(default)]
    pub final_answer: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallRecord>,
    #[serde(default)]
    pub tool_results: Vec<ToolResultRecord>,
    #[serde(default)]
    pub usage: AttemptUsage,
    #[serde(default)]
    pub error_code: String,
    #[serde(default)]
    pub error_message: String,
    #[serde(default)]
    pub error_detail: Value,
}
