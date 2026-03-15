// Config loading and YAML override merging.
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tracing::warn;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub cors: CorsConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub i18n: I18nConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub cron: CronConfig,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub lsp: LspConfig,
    #[serde(default)]
    pub a2a: A2aConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub knowledge: KnowledgeConfig,
    #[serde(default)]
    pub vector_store: VectorStoreConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub sandbox: SandboxConfig,
    #[serde(default)]
    pub agent_queue: AgentQueueConfig,
    #[serde(default)]
    pub user_agents: UserAgentsConfig,
    #[serde(default)]
    pub prompt_templates: PromptTemplatesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplatesConfig {
    /// Active prompt template pack id. `"default"` uses the built-in `./prompts` directory.
    #[serde(default = "default_prompt_templates_active")]
    pub active: String,
    /// Root directory that stores admin-managed prompt template packs.
    #[serde(default = "default_prompt_templates_root")]
    pub root: String,
}

impl Default for PromptTemplatesConfig {
    fn default() -> Self {
        Self {
            active: default_prompt_templates_active(),
            root: default_prompt_templates_root(),
        }
    }
}

fn default_prompt_templates_active() -> String {
    "default".to_string()
}

fn default_prompt_templates_root() -> String {
    "./data/prompt_templates".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityConfig {
    pub api_key: Option<String>,
    pub external_auth_key: Option<String>,
    pub external_embed_preset_agent_name: Option<String>,
    pub external_embed_jwt_secret: Option<String>,
    pub external_embed_jwt_user_id_claim: Option<String>,
    #[serde(default)]
    pub allow_commands: Vec<String>,
    #[serde(default)]
    pub allow_paths: Vec<String>,
    #[serde(default)]
    pub deny_globs: Vec<String>,
    #[serde(default)]
    pub exec_policy_mode: Option<String>,
    /// CLI-only: approval mode for write/exec tools (suggest/auto_edit/full_auto).
    /// Server deployments typically leave this unset and rely on allow_paths/deny_globs + auth.
    #[serde(default)]
    pub approval_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CorsConfig {
    pub allow_origins: Option<Vec<String>>,
    pub allow_methods: Option<Vec<String>>,
    pub allow_headers: Option<Vec<String>>,
    pub allow_credentials: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    #[serde(deserialize_with = "deserialize_u16_from_any")]
    pub port: u16,
    pub stream_chunk_size: usize,
    pub max_active_sessions: usize,
    #[serde(default = "default_chat_stream_channel")]
    pub chat_stream_channel: String,
    #[serde(
        default = "default_tool_failure_guard_threshold",
        deserialize_with = "deserialize_usize_from_any"
    )]
    pub tool_failure_guard_threshold: usize,
    #[serde(default = "default_server_mode")]
    pub mode: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8000,
            stream_chunk_size: 1024,
            max_active_sessions: 300,
            chat_stream_channel: default_chat_stream_channel(),
            tool_failure_guard_threshold: default_tool_failure_guard_threshold(),
            mode: "api".to_string(),
        }
    }
}

pub const CHAT_STREAM_CHANNEL_WS: &str = "ws";
pub const CHAT_STREAM_CHANNEL_SSE: &str = "sse";

pub fn normalize_chat_stream_channel(value: &str) -> String {
    if value.trim().eq_ignore_ascii_case(CHAT_STREAM_CHANNEL_SSE) {
        CHAT_STREAM_CHANNEL_SSE.to_string()
    } else {
        CHAT_STREAM_CHANNEL_WS.to_string()
    }
}

fn default_chat_stream_channel() -> String {
    CHAT_STREAM_CHANNEL_WS.to_string()
}

fn default_tool_failure_guard_threshold() -> usize {
    5
}

fn default_server_mode() -> String {
    "api".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I18nConfig {
    pub default_language: String,
    pub supported_languages: Vec<String>,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
}

impl Default for I18nConfig {
    fn default() -> Self {
        Self {
            default_language: "zh-CN".to_string(),
            supported_languages: vec!["zh-CN".to_string(), "en-US".to_string()],
            aliases: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmConfig {
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub models: HashMap<String, LlmModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentQueueConfig {
    #[serde(default = "default_agent_queue_enabled")]
    pub enabled: bool,
    #[serde(default = "default_agent_queue_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_agent_queue_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_agent_queue_task_ttl_s")]
    pub task_ttl_s: u64,
}

impl Default for AgentQueueConfig {
    fn default() -> Self {
        Self {
            enabled: default_agent_queue_enabled(),
            poll_interval_ms: default_agent_queue_poll_interval_ms(),
            max_retries: default_agent_queue_max_retries(),
            task_ttl_s: default_agent_queue_task_ttl_s(),
        }
    }
}

fn default_agent_queue_enabled() -> bool {
    true
}

fn default_agent_queue_poll_interval_ms() -> u64 {
    1500
}

fn default_agent_queue_max_retries() -> u32 {
    2
}

fn default_agent_queue_task_ttl_s() -> u64 {
    24 * 60 * 60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAgentsConfig {
    #[serde(default = "default_user_agent_presets")]
    pub presets: Vec<UserAgentPresetConfig>,
}

impl Default for UserAgentsConfig {
    fn default() -> Self {
        Self {
            presets: default_user_agent_presets(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAgentPresetConfig {
    #[serde(default)]
    pub preset_id: String,
    #[serde(default = "default_user_agent_preset_revision")]
    pub revision: u64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default = "default_user_agent_preset_icon_name")]
    pub icon_name: String,
    #[serde(default = "default_user_agent_preset_icon_color")]
    pub icon_color: String,
    #[serde(default = "default_user_agent_preset_sandbox_container_id")]
    pub sandbox_container_id: i32,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub declared_tool_names: Vec<String>,
    #[serde(default)]
    pub declared_skill_names: Vec<String>,
    #[serde(default)]
    pub preset_questions: Vec<String>,
    #[serde(default = "default_user_agent_preset_approval_mode")]
    pub approval_mode: String,
    #[serde(default = "default_user_agent_preset_status")]
    pub status: String,
}

fn default_user_agent_preset_revision() -> u64 {
    1
}

fn default_user_agent_preset_icon_name() -> String {
    "spark".to_string()
}

fn default_user_agent_preset_icon_color() -> String {
    "#94a3b8".to_string()
}

fn default_user_agent_preset_sandbox_container_id() -> i32 {
    1
}

fn default_user_agent_preset_approval_mode() -> String {
    "full_auto".to_string()
}

fn default_user_agent_preset_status() -> String {
    "active".to_string()
}

fn default_user_agent_presets() -> Vec<UserAgentPresetConfig> {
    vec![
        UserAgentPresetConfig {
            preset_id: String::new(),
            revision: default_user_agent_preset_revision(),
            name: "????".to_string(),
            description: "??????????????????".to_string(),
            system_prompt: "????????????????????????????????????".to_string(),
            icon_name: "spark".to_string(),
            icon_color: "#fbbf24".to_string(),
            sandbox_container_id: 2,
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            approval_mode: default_user_agent_preset_approval_mode(),
            status: default_user_agent_preset_status(),
        },
        UserAgentPresetConfig {
            preset_id: String::new(),
            revision: default_user_agent_preset_revision(),
            name: "????".to_string(),
            description: "????????????????".to_string(),
            system_prompt: "??????????????????????????????????".to_string(),
            icon_name: "chart".to_string(),
            icon_color: "#60a5fa".to_string(),
            sandbox_container_id: 3,
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            approval_mode: default_user_agent_preset_approval_mode(),
            status: default_user_agent_preset_status(),
        },
        UserAgentPresetConfig {
            preset_id: String::new(),
            revision: default_user_agent_preset_revision(),
            name: "????".to_string(),
            description: "?????????????".to_string(),
            system_prompt: "????????????????????????????????????".to_string(),
            icon_name: "chart".to_string(),
            icon_color: "#22d3ee".to_string(),
            sandbox_container_id: 4,
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            approval_mode: default_user_agent_preset_approval_mode(),
            status: default_user_agent_preset_status(),
        },
        UserAgentPresetConfig {
            preset_id: String::new(),
            revision: default_user_agent_preset_revision(),
            name: "????".to_string(),
            description: "????????????".to_string(),
            system_prompt: "?????????????????????????????????".to_string(),
            icon_name: "briefcase".to_string(),
            icon_color: "#f97316".to_string(),
            sandbox_container_id: 5,
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            approval_mode: default_user_agent_preset_approval_mode(),
            status: default_user_agent_preset_status(),
        },
        UserAgentPresetConfig {
            preset_id: String::new(),
            revision: default_user_agent_preset_revision(),
            name: "????".to_string(),
            description: "???????????????".to_string(),
            system_prompt: "??????????????????????????????".to_string(),
            icon_name: "shield".to_string(),
            icon_color: "#94a3b8".to_string(),
            sandbox_container_id: 6,
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            approval_mode: default_user_agent_preset_approval_mode(),
            status: default_user_agent_preset_status(),
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmModelConfig {
    #[serde(default, alias = "enabled")]
    pub enable: Option<bool>,
    pub provider: Option<String>,
    #[serde(default)]
    pub api_mode: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub timeout_s: Option<u64>,
    #[serde(default)]
    pub retry: Option<u32>,
    #[serde(default)]
    pub max_rounds: Option<u32>,
    #[serde(default)]
    pub max_context: Option<u32>,
    #[serde(default)]
    pub max_output: Option<u32>,
    #[serde(default)]
    pub support_vision: Option<bool>,
    #[serde(default)]
    pub support_hearing: Option<bool>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub stream_include_usage: Option<bool>,
    #[serde(default)]
    pub history_compaction_ratio: Option<f32>,
    #[serde(default)]
    pub history_compaction_reset: Option<String>,
    #[serde(default)]
    pub tool_call_mode: Option<String>,
    #[serde(default)]
    pub model_type: Option<String>,
    #[serde(default)]
    pub stop: Option<Vec<String>>,
    #[serde(default)]
    pub mock_if_unconfigured: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    #[serde(default)]
    pub builtin: BuiltinToolsConfig,
    #[serde(default)]
    pub swarm: AgentSwarmConfig,
    #[serde(default)]
    pub browser: BrowserToolConfig,
    #[serde(default)]
    pub desktop_controller: DesktopControllerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, deserialize_with = "deserialize_usize_from_any")]
    pub max_concurrent_runs: usize,
    #[serde(default, deserialize_with = "deserialize_u64_from_any")]
    pub poll_interval_ms: u64,
    #[serde(default, deserialize_with = "deserialize_u64_from_any")]
    pub max_idle_sleep_ms: u64,
    #[serde(default, deserialize_with = "deserialize_u64_from_any")]
    pub idle_retry_ms: u64,
    #[serde(default, deserialize_with = "deserialize_u64_from_any")]
    pub max_busy_wait_ms: u64,
    #[serde(default, deserialize_with = "deserialize_u64_from_any")]
    pub lease_ttl_ms: u64,
    #[serde(default, deserialize_with = "deserialize_u64_from_any")]
    pub lease_heartbeat_ms: u64,
    #[serde(default, deserialize_with = "deserialize_usize_from_any")]
    pub max_consecutive_failures: usize,
}

impl Default for CronConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_runs: 1,
            poll_interval_ms: 1000,
            max_idle_sleep_ms: 5000,
            idle_retry_ms: 2000,
            max_busy_wait_ms: 120_000,
            lease_ttl_ms: 300_000,
            lease_heartbeat_ms: 60_000,
            max_consecutive_failures: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuiltinToolsConfig {
    #[serde(default)]
    pub enabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserToolConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_browser_headless")]
    pub headless: bool,
    #[serde(default = "default_browser_viewport_width")]
    pub viewport_width: u32,
    #[serde(default = "default_browser_viewport_height")]
    pub viewport_height: u32,
    #[serde(default = "default_browser_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_browser_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_browser_max_sessions")]
    pub max_sessions: usize,
    #[serde(default)]
    pub python_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopControllerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_desktop_controller_norm_width")]
    pub norm_width: i32,
    #[serde(default = "default_desktop_controller_norm_height")]
    pub norm_height: i32,
    #[serde(default = "default_desktop_controller_max_frames")]
    pub max_frames: usize,
    #[serde(default = "default_desktop_controller_capture_timeout_ms")]
    pub capture_timeout_ms: u64,
}

impl Default for BrowserToolConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            headless: default_browser_headless(),
            viewport_width: default_browser_viewport_width(),
            viewport_height: default_browser_viewport_height(),
            timeout_secs: default_browser_timeout_secs(),
            idle_timeout_secs: default_browser_idle_timeout_secs(),
            max_sessions: default_browser_max_sessions(),
            python_path: None,
        }
    }
}

impl Default for DesktopControllerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            norm_width: default_desktop_controller_norm_width(),
            norm_height: default_desktop_controller_norm_height(),
            max_frames: default_desktop_controller_max_frames(),
            capture_timeout_ms: default_desktop_controller_capture_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSwarmConfig {
    #[serde(default = "default_agent_swarm_runner")]
    pub runner: String,
    #[serde(default = "default_agent_swarm_max_active_team_runs")]
    pub max_active_team_runs: usize,
    #[serde(default = "default_agent_swarm_max_parallel_tasks_per_team")]
    pub max_parallel_tasks_per_team: usize,
    #[serde(default = "default_agent_swarm_default_timeout_s")]
    pub default_timeout_s: u64,
    #[serde(default = "default_agent_swarm_max_retry")]
    pub max_retry: u32,
    #[serde(default = "default_agent_swarm_max_depth")]
    pub max_depth: u32,
}

impl Default for AgentSwarmConfig {
    fn default() -> Self {
        Self {
            runner: default_agent_swarm_runner(),
            max_active_team_runs: default_agent_swarm_max_active_team_runs(),
            max_parallel_tasks_per_team: default_agent_swarm_max_parallel_tasks_per_team(),
            default_timeout_s: default_agent_swarm_default_timeout_s(),
            max_retry: default_agent_swarm_max_retry(),
            max_depth: default_agent_swarm_max_depth(),
        }
    }
}

fn default_agent_swarm_runner() -> String {
    "legacy".to_string()
}

fn default_agent_swarm_max_active_team_runs() -> usize {
    256
}

fn default_agent_swarm_max_parallel_tasks_per_team() -> usize {
    256
}

fn default_agent_swarm_default_timeout_s() -> u64 {
    180
}

fn default_agent_swarm_max_retry() -> u32 {
    2
}

fn default_agent_swarm_max_depth() -> u32 {
    2
}

fn default_browser_headless() -> bool {
    true
}

fn default_browser_viewport_width() -> u32 {
    1280
}

fn default_browser_viewport_height() -> u32 {
    720
}

fn default_browser_timeout_secs() -> u64 {
    30
}

fn default_browser_idle_timeout_secs() -> u64 {
    300
}

fn default_browser_max_sessions() -> usize {
    5
}

fn default_desktop_controller_norm_width() -> i32 {
    1000
}

fn default_desktop_controller_norm_height() -> i32 {
    1000
}

fn default_desktop_controller_max_frames() -> usize {
    6
}

fn default_desktop_controller_capture_timeout_ms() -> u64 {
    5000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub root: String,
    #[serde(default)]
    pub container_roots: HashMap<i32, String>,
    #[serde(default)]
    pub max_history_items: i64,
    #[serde(default)]
    pub retention_days: i64,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            root: "./workspaces".to_string(),
            container_roots: HashMap::new(),
            max_history_items: 0,
            retention_days: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub timeout_s: u64,
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LspConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub timeout_s: u64,
    #[serde(default)]
    pub diagnostics_debounce_ms: u64,
    #[serde(default)]
    pub idle_ttl_s: u64,
    #[serde(default)]
    pub servers: Vec<LspServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LspServerConfig {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub root_markers: Vec<String>,
    #[serde(default)]
    pub initialization_options: Option<Value>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpServerConfig {
    pub name: String,
    pub endpoint: String,
    #[serde(default)]
    pub allow_tools: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub auth: Option<Value>,
    #[serde(default)]
    pub tool_specs: Vec<McpToolSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpToolSpec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct A2aConfig {
    #[serde(default)]
    pub timeout_s: u64,
    #[serde(default)]
    pub services: Vec<A2aServiceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct A2aServiceConfig {
    pub name: String,
    pub endpoint: String,
    #[serde(default)]
    pub service_type: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allow_self: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub auth: Option<Value>,
    #[serde(default)]
    pub agent_card: Option<Value>,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub default_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsConfig {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub enabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KnowledgeConfig {
    #[serde(default)]
    pub bases: Vec<KnowledgeBaseConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KnowledgeBaseConfig {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub root: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub shared: Option<bool>,
    #[serde(default)]
    pub base_type: Option<String>,
    #[serde(default)]
    pub embedding_model: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_usize_from_any")]
    pub chunk_size: Option<usize>,
    #[serde(default, deserialize_with = "deserialize_optional_usize_from_any")]
    pub chunk_overlap: Option<usize>,
    #[serde(default, deserialize_with = "deserialize_optional_usize_from_any")]
    pub top_k: Option<usize>,
    #[serde(default)]
    pub score_threshold: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeBaseType {
    Literal,
    Vector,
}

pub fn normalize_knowledge_base_type(value: Option<&str>) -> KnowledgeBaseType {
    let raw = value.unwrap_or("").trim();
    if raw.is_empty() {
        return KnowledgeBaseType::Literal;
    }
    match raw.to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
        "vector" | "embedding" => KnowledgeBaseType::Vector,
        _ => KnowledgeBaseType::Literal,
    }
}

impl KnowledgeBaseConfig {
    pub fn base_type(&self) -> KnowledgeBaseType {
        normalize_knowledge_base_type(self.base_type.as_deref())
    }

    pub fn is_vector(&self) -> bool {
        self.base_type() == KnowledgeBaseType::Vector
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObservabilityConfig {
    #[serde(default)]
    pub log_level: String,
    #[serde(default)]
    pub monitor_event_limit: i64,
    #[serde(default)]
    pub monitor_payload_max_chars: i64,
    #[serde(default)]
    pub monitor_drop_event_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VectorStoreConfig {
    #[serde(default)]
    pub weaviate: WeaviateConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeaviateConfig {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub timeout_s: u64,
    #[serde(default, deserialize_with = "deserialize_usize_from_any")]
    pub batch_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub db_path: String,
    #[serde(default)]
    pub postgres: PostgresConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allow_unknown_accounts: bool,
    #[serde(default = "default_channel_session_strategy")]
    pub session_strategy: String,
    #[serde(default)]
    pub default_agent_id: Option<String>,
    #[serde(default)]
    pub default_tool_overrides: Vec<String>,
    #[serde(default)]
    pub rate_limit: ChannelRateLimitConfig,
    #[serde(default)]
    pub outbox: ChannelOutboxConfig,
    #[serde(default)]
    pub media: ChannelMediaConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default = "default_gateway_protocol_version")]
    pub protocol_version: i32,
    #[serde(default)]
    pub allow_unpaired_nodes: bool,
    #[serde(default)]
    pub node_token_required: bool,
    #[serde(default)]
    pub allow_gateway_token_for_nodes: bool,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub trusted_proxies: Vec<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auth_token: None,
            protocol_version: default_gateway_protocol_version(),
            allow_unpaired_nodes: false,
            node_token_required: false,
            allow_gateway_token_for_nodes: false,
            allowed_origins: Vec::new(),
            trusted_proxies: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelRateLimitConfig {
    #[serde(default)]
    pub default_qps: u32,
    #[serde(default)]
    pub default_concurrency: u32,
    #[serde(default)]
    pub by_channel: HashMap<String, ChannelRateLimitOverride>,
}

fn default_channel_session_strategy() -> String {
    "main_thread".to_string()
}

fn default_gateway_protocol_version() -> i32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelRateLimitOverride {
    #[serde(default)]
    pub qps: Option<u32>,
    #[serde(default)]
    pub concurrency: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelOutboxConfig {
    #[serde(default)]
    pub worker_enabled: bool,
    #[serde(default)]
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub max_batch: usize,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub retry_base_s: f64,
    #[serde(default)]
    pub retry_max_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelMediaConfig {
    #[serde(default)]
    pub asr: ChannelAsrConfig,
    #[serde(default)]
    pub tts: ChannelTtsConfig,
    #[serde(default)]
    pub ocr: ChannelOcrConfig,
    #[serde(default)]
    pub geocode: ChannelGeocodeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelAsrConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub timeout_s: u64,
    #[serde(default)]
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelTtsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub timeout_s: u64,
    #[serde(default)]
    pub max_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelOcrConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub timeout_s: u64,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelGeocodeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub timeout_s: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PostgresConfig {
    pub dsn: String,
    #[serde(default)]
    pub connect_timeout_s: u64,
    #[serde(default, deserialize_with = "deserialize_usize_from_any")]
    pub pool_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxConfig {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub container_root: String,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub readonly_rootfs: bool,
    #[serde(default)]
    pub idle_ttl_s: u64,
    #[serde(default)]
    pub timeout_s: u64,
    #[serde(default)]
    pub resources: SandboxResources,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxResources {
    #[serde(default)]
    pub cpu: f32,
    #[serde(default)]
    pub memory_mb: u64,
    #[serde(default)]
    pub pids: u64,
}

impl Config {
    // Normalize API key values and ignore blank placeholders.
    pub fn api_key(&self) -> Option<String> {
        let inline = self
            .security
            .api_key
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        if let Some(value) = inline {
            if value.starts_with("${") && value.ends_with('}') {
                return env::var("WUNDER_API_KEY")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
            }
            return Some(value.to_string());
        }
        env::var("WUNDER_API_KEY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    // Resolve external auth key with environment fallback.
    pub fn external_auth_key(&self) -> Option<String> {
        let inline = self
            .security
            .external_auth_key
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        if let Some(value) = inline {
            if value.starts_with("${") && value.ends_with('}') {
                return env::var("WUNDER_EXTERNAL_AUTH_KEY")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .or_else(|| self.api_key());
            }
            return Some(value.to_string());
        }
        env::var("WUNDER_EXTERNAL_AUTH_KEY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| self.api_key())
    }

    // Resolve the default preset agent name for external embed flows.
    pub fn external_embed_preset_agent_name(&self) -> Option<String> {
        let inline = self
            .security
            .external_embed_preset_agent_name
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        if let Some(value) = inline {
            if value.starts_with("${") && value.ends_with('}') {
                return env::var("WUNDER_EXTERNAL_EMBED_PRESET_AGENT_NAME")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
            }
            return Some(value.to_string());
        }
        env::var("WUNDER_EXTERNAL_EMBED_PRESET_AGENT_NAME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    pub fn external_embed_jwt_secret(&self) -> Option<String> {
        let inline = self
            .security
            .external_embed_jwt_secret
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        if let Some(value) = inline {
            if value.starts_with("${") && value.ends_with('}') {
                return env::var("WUNDER_EXTERNAL_EMBED_JWT_SECRET")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
            }
            return Some(value.to_string());
        }
        env::var("WUNDER_EXTERNAL_EMBED_JWT_SECRET")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    pub fn external_embed_jwt_user_id_claim(&self) -> String {
        let inline = self
            .security
            .external_embed_jwt_user_id_claim
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        if let Some(value) = inline {
            if value.starts_with("${") && value.ends_with('}') {
                return env::var("WUNDER_EXTERNAL_EMBED_JWT_USER_ID_CLAIM")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "sub".to_string());
            }
            return value.to_string();
        }
        env::var("WUNDER_EXTERNAL_EMBED_JWT_USER_ID_CLAIM")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "sub".to_string())
    }
}

pub fn is_debug_log_level(raw: &str) -> bool {
    let level = raw.trim().to_ascii_lowercase();
    matches!(level.as_str(), "debug" | "trace")
}

fn deserialize_u16_from_any<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    struct U16Visitor;

    impl<'de> Visitor<'de> for U16Visitor {
        type Value = u16;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("u16 or numeric string")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            u16::try_from(value).map_err(|_| E::custom("u16 out of range"))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value < 0 {
                return Err(E::custom("u16 must be non-negative"));
            }
            self.visit_u64(value as u64)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(E::custom("u16 string is empty"));
            }
            trimmed
                .parse::<u16>()
                .map_err(|_| E::custom("invalid u16 string"))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&value)
        }
    }

    deserializer.deserialize_any(U16Visitor)
}

fn deserialize_u64_from_any<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    struct U64Visitor;

    impl<'de> Visitor<'de> for U64Visitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("u64 or numeric string")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value < 0 {
                return Err(E::custom("u64 must be non-negative"));
            }
            Ok(value as u64)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(E::custom("u64 string is empty"));
            }
            trimmed
                .parse::<u64>()
                .map_err(|_| E::custom("invalid u64 string"))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&value)
        }
    }

    deserializer.deserialize_any(U64Visitor)
}

fn deserialize_usize_from_any<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    struct UsizeVisitor;

    impl Visitor<'_> for UsizeVisitor {
        type Value = usize;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("usize or string")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value as usize)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value < 0 {
                return Err(E::custom("invalid usize value"));
            }
            Ok(value as usize)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value
                .trim()
                .parse::<usize>()
                .map_err(|_| E::custom("invalid usize string"))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&value)
        }
    }

    deserializer.deserialize_any(UsizeVisitor)
}

fn deserialize_optional_usize_from_any<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptionalUsizeVisitor;

    impl<'de> Visitor<'de> for OptionalUsizeVisitor {
        type Value = Option<usize>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("optional usize or string")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_usize_from_any(deserializer).map(Some)
        }
    }

    deserializer.deserialize_option(OptionalUsizeVisitor)
}

pub fn load_config() -> Config {
    // Load base config and then apply override config if present.
    let base_path =
        env::var("WUNDER_CONFIG_PATH").unwrap_or_else(|_| "config/wunder.yaml".to_string());
    let override_path = env::var("WUNDER_CONFIG_OVERRIDE_PATH")
        .unwrap_or_else(|_| "data/config/wunder.override.yaml".to_string());
    let override_path = resolve_yaml_variant_path(Path::new(&override_path));

    let mut merged = read_yaml(&base_path);
    if override_path.exists() {
        let override_value = read_yaml_path(&override_path);
        // Apply override values without blanking existing keys.
        merge_yaml(&mut merged, override_value);
    }

    expand_yaml_env(&mut merged);

    serde_yaml::from_value::<Config>(merged).unwrap_or_else(|err| {
        warn!("閰嶇疆瑙ｆ瀽澶辫触锛屼娇鐢ㄩ粯璁ら厤缃? {err}");
        Config::default()
    })
}

pub fn load_base_config_value() -> Value {
    let base_path =
        env::var("WUNDER_CONFIG_PATH").unwrap_or_else(|_| "config/wunder.yaml".to_string());
    let mut base = read_yaml(&base_path);
    expand_yaml_env(&mut base);
    base
}

fn read_yaml(path: &str) -> Value {
    // Missing config files are allowed during bootstrap.
    let content = match read_yaml_content_with_fallback(path) {
        Ok(text) => text,
        Err(err) => {
            warn!("璇诲彇閰嶇疆澶辫触: {path}, {err}");
            return Value::Null;
        }
    };
    serde_yaml::from_str(&content).unwrap_or_else(|err| {
        warn!("瑙ｆ瀽 YAML 澶辫触: {path}, {err}");
        Value::Null
    })
}

fn read_yaml_path(path: &Path) -> Value {
    let path_display = path.display().to_string();
    let content = match read_yaml_content_with_fallback(&path_display) {
        Ok(text) => text,
        Err(err) => {
            warn!("璇诲彇閰嶇疆澶辫触: {path_display}, {err}");
            return Value::Null;
        }
    };
    serde_yaml::from_str(&content).unwrap_or_else(|err| {
        warn!("瑙ｆ瀽 YAML 澶辫触: {path_display}, {err}");
        Value::Null
    })
}

fn read_yaml_content_with_fallback(path: &str) -> Result<String, std::io::Error> {
    let resolved_path = resolve_yaml_variant_path(Path::new(path));
    match fs::read_to_string(&resolved_path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            let Some(example_path) = resolve_example_config_path(&resolved_path) else {
                return Err(err);
            };
            let text = fs::read_to_string(&example_path)?;
            warn!(
                "閰嶇疆鏂囦欢涓嶅瓨鍦紝鍥為€€浣跨敤绀轰緥閰嶇疆: {} -> {}",
                resolved_path.display(),
                example_path.display()
            );
            Ok(text)
        }
        Err(err) => Err(err),
    }
}

fn resolve_example_config_path(path: &Path) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_str()?;
    match file_name {
        "wunder.yaml" | "wunder.yml" => Some(path.with_file_name("wunder-example.yaml")),
        _ => None,
    }
}

fn resolve_yaml_variant_path(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    let Some(swapped) = swap_yaml_extension(path) else {
        return path.to_path_buf();
    };
    if swapped.exists() {
        return swapped;
    }
    path.to_path_buf()
}

fn swap_yaml_extension(path: &Path) -> Option<PathBuf> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "yaml" => Some(path.with_extension("yml")),
        "yml" => Some(path.with_extension("yaml")),
        _ => None,
    }
}

fn merge_yaml(base: &mut Value, override_value: Value) {
    match (base, override_value) {
        (Value::Mapping(base_map), Value::Mapping(override_map)) => {
            // Merge nested mappings recursively and keep the original structure.
            for (key, value) in override_map {
                match base_map.get_mut(&key) {
                    Some(existing) => merge_yaml(existing, value),
                    None => {
                        if !is_blank_yaml_string(&value) {
                            base_map.insert(key, value);
                        }
                    }
                }
            }
        }
        (base_slot, override_value) => {
            if !override_value.is_null() && !is_blank_yaml_string(&override_value) {
                *base_slot = override_value;
            }
        }
    }
}

fn is_blank_yaml_string(value: &Value) -> bool {
    matches!(value, Value::String(text) if text.trim().is_empty())
}

fn expand_yaml_env(value: &mut Value) {
    match value {
        Value::String(text) => {
            *text = expand_env_placeholders(text);
        }
        Value::Sequence(items) => {
            for item in items {
                expand_yaml_env(item);
            }
        }
        Value::Mapping(map) => {
            for (_, value) in map.iter_mut() {
                expand_yaml_env(value);
            }
        }
        _ => {}
    }
}

fn expand_env_placeholders(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        let Some(end) = rest.find('}') else {
            output.push_str("${");
            output.push_str(rest);
            return output;
        };
        let inner = &rest[..end];
        rest = &rest[end + 1..];
        let (name, default_value) = match inner.split_once(":-") {
            Some((name, default_value)) => (name.trim(), Some(default_value)),
            None => (inner.trim(), None),
        };
        if name.is_empty() {
            output.push_str("${");
            output.push_str(inner);
            output.push('}');
            continue;
        }
        let resolved = env::var(name).ok().filter(|value| !value.is_empty());
        match (resolved, default_value) {
            (Some(value), _) => output.push_str(&value),
            (None, Some(default_value)) => output.push_str(default_value),
            (None, None) => {}
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_expand_env_placeholders() {
        std::env::remove_var("WUNDER_TEST_PLACEHOLDER");
        assert_eq!(
            expand_env_placeholders("${WUNDER_TEST_PLACEHOLDER:-default}"),
            "default"
        );
        assert_eq!(
            expand_env_placeholders("prefix-${WUNDER_TEST_PLACEHOLDER:-d}-suffix"),
            "prefix-d-suffix"
        );

        std::env::set_var("WUNDER_TEST_PLACEHOLDER", "value");
        assert_eq!(
            expand_env_placeholders("${WUNDER_TEST_PLACEHOLDER:-default}"),
            "value"
        );
        assert_eq!(
            expand_env_placeholders("prefix-${WUNDER_TEST_PLACEHOLDER}-suffix"),
            "prefix-value-suffix"
        );

        std::env::remove_var("WUNDER_TEST_PLACEHOLDER");
        assert_eq!(expand_env_placeholders("${WUNDER_TEST_PLACEHOLDER}"), "");
    }

    #[test]
    fn test_user_agent_presets_have_expected_defaults() {
        let config = Config::default();
        assert_eq!(config.user_agents.presets.len(), 5);
        assert_eq!(config.user_agents.presets[0].name, "文稿校对");
        assert_eq!(config.user_agents.presets[0].sandbox_container_id, 2);
    }

    #[test]
    fn test_user_agent_presets_support_yaml_override() {
        let parsed: Config = serde_yaml::from_str(
            r#"
user_agents:
  presets:
    - name: 自定义应用
      description: 自定义描述
      system_prompt: 你是自定义助手
      icon_name: spark
      icon_color: '#123456'
      sandbox_container_id: 8
"#,
        )
        .expect("parse user_agents override");

        assert_eq!(parsed.user_agents.presets.len(), 1);
        let preset = &parsed.user_agents.presets[0];
        assert_eq!(preset.name, "自定义应用");
        assert_eq!(preset.description, "自定义描述");
        assert_eq!(preset.system_prompt, "你是自定义助手");
        assert_eq!(preset.icon_name, "spark");
        assert_eq!(preset.icon_color, "#123456");
        assert_eq!(preset.sandbox_container_id, 8);
    }
    #[test]
    fn test_merge_yaml_keeps_base_when_override_has_blank_strings() {
        let mut base = serde_yaml::from_str::<Value>(
            r#"
storage:
  backend: auto
  db_path: ./data/wunder.db
  postgres:
    dsn: postgresql://wunder:wunder@postgres:5432/wunder
    connect_timeout_s: 5
    pool_size: 64
sandbox:
  endpoint: http://sandbox:9001
"#,
        )
        .expect("parse base yaml");

        let override_value = serde_yaml::from_str::<Value>(
            r#"
storage:
  backend: ''
  db_path: ''
  postgres:
    dsn: ''
sandbox:
  endpoint: ''
"#,
        )
        .expect("parse override yaml");

        merge_yaml(&mut base, override_value);
        let merged: Config = serde_yaml::from_value(base).expect("parse merged config");

        assert_eq!(merged.storage.backend, "auto");
        assert_eq!(merged.storage.db_path, "./data/wunder.db");
        assert_eq!(
            merged.storage.postgres.dsn,
            "postgresql://wunder:wunder@postgres:5432/wunder"
        );
        assert_eq!(merged.sandbox.endpoint, "http://sandbox:9001");
    }

    #[test]
    fn test_external_auth_key_prefers_dedicated_key() {
        let mut config = Config::default();
        config.security.api_key = Some("api-key".to_string());
        assert_eq!(config.external_auth_key(), Some("api-key".to_string()));

        config.security.external_auth_key = Some("external-key".to_string());
        assert_eq!(config.external_auth_key(), Some("external-key".to_string()));
    }

    #[test]
    fn test_external_embed_jwt_user_id_claim_defaults_to_sub() {
        let config = Config::default();
        assert_eq!(config.external_embed_jwt_user_id_claim(), "sub");
    }

    #[test]
    fn test_read_yaml_content_falls_back_to_yml_variant() {
        let root = std::env::temp_dir().join(format!(
            "wunder-config-yml-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).expect("create temp dir");
        let yaml_path = root.join("wunder.override.yaml");
        let yml_path = root.join("wunder.override.yml");
        fs::write(&yml_path, "observability:\n  log_level: DEBUG\n").expect("write yml config");

        let content = read_yaml_content_with_fallback(&yaml_path.display().to_string())
            .expect("read yaml variant content");
        assert!(content.contains("DEBUG"));

        let _ = fs::remove_dir_all(&root);
    }
}
