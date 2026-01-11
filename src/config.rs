// 配置读取与覆盖合并，保持与现有 YAML 配置格式兼容。
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::Path;
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
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub a2a: A2aConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub knowledge: KnowledgeConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub sandbox: SandboxConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityConfig {
    pub api_key: Option<String>,
    #[serde(default)]
    pub allow_commands: Vec<String>,
    #[serde(default)]
    pub allow_paths: Vec<String>,
    #[serde(default)]
    pub deny_globs: Vec<String>,
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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8000,
            stream_chunk_size: 1024,
            max_active_sessions: 30,
        }
    }
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
pub struct LlmModelConfig {
    #[serde(default, alias = "enabled")]
    pub enable: Option<bool>,
    pub provider: Option<String>,
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
    pub stream: Option<bool>,
    #[serde(default)]
    pub stream_include_usage: Option<bool>,
    #[serde(default)]
    pub history_compaction_ratio: Option<f32>,
    #[serde(default)]
    pub history_compaction_reset: Option<String>,
    #[serde(default)]
    pub stop: Option<Vec<String>>,
    #[serde(default)]
    pub mock_if_unconfigured: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    #[serde(default)]
    pub builtin: BuiltinToolsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuiltinToolsConfig {
    #[serde(default)]
    pub enabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub root: String,
    #[serde(default)]
    pub max_history_items: i64,
    #[serde(default)]
    pub retention_days: i64,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            root: "./data/workspaces".to_string(),
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
pub struct StorageConfig {
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub db_path: String,
    #[serde(default)]
    pub postgres: PostgresConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PostgresConfig {
    pub dsn: String,
    #[serde(default)]
    pub connect_timeout_s: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxConfig {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub image: String,
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
    // 统一归一化 API Key，避免空白字符导致鉴权误判。
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

pub fn load_config() -> Config {
    // 读取基础配置与覆盖配置，优先使用管理端覆盖内容。
    let base_path =
        env::var("WUNDER_CONFIG_PATH").unwrap_or_else(|_| "config/wunder.yaml".to_string());
    let override_path = env::var("WUNDER_CONFIG_OVERRIDE_PATH")
        .unwrap_or_else(|_| "data/config/wunder.override.yaml".to_string());

    let mut merged = read_yaml(&base_path);
    if Path::new(&override_path).exists() {
        let override_value = read_yaml(&override_path);
        // 只对非空字段做递归覆盖，避免误清空已有配置。
        merge_yaml(&mut merged, override_value);
    }

    expand_yaml_env(&mut merged);

    serde_yaml::from_value::<Config>(merged).unwrap_or_else(|err| {
        warn!("配置解析失败，使用默认配置: {err}");
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
    // 配置文件允许不存在，避免开发环境首次启动失败。
    let content = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            warn!("读取配置失败: {path}, {err}");
            return Value::Null;
        }
    };
    serde_yaml::from_str(&content).unwrap_or_else(|err| {
        warn!("解析 YAML 失败: {path}, {err}");
        Value::Null
    })
}

fn merge_yaml(base: &mut Value, override_value: Value) {
    match (base, override_value) {
        (Value::Mapping(base_map), Value::Mapping(override_map)) => {
            // 递归合并 Mapping，保留原始层级结构。
            for (key, value) in override_map {
                match base_map.get_mut(&key) {
                    Some(existing) => merge_yaml(existing, value),
                    None => {
                        base_map.insert(key, value);
                    }
                }
            }
        }
        (base_slot, override_value) => {
            if !override_value.is_null() {
                *base_slot = override_value;
            }
        }
    }
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
}
