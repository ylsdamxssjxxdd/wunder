//! Local HTTP adapter for the established `/wunder/chat` contract.
//!
//! Desktop bridge traffic never leaves the loopback interface. Keeping this
//! adapter on `std::net` preserves the Win7 x86 offline dependency closure.
use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const API_PREFIX: &str = "/wunder";
const CONFIG_PATH: &str = "/config.json";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub struct ConnectionConfig {
    pub api_base: LocalApiBase,
    pub token: String,
}

#[derive(Clone, Debug)]
pub struct AgentRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub model: String,
    pub system_prompt: String,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct ToolRecord {
    pub name: String,
    pub description: String,
    pub category: String,
}

#[derive(Clone, Debug)]
pub struct FileRecord {
    pub name: String,
    pub path: String,
    pub entry_type: String,
    pub size: String,
}

#[derive(Clone, Debug)]
pub struct ModelRecord {
    pub key: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub model_type: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DesktopSettings {
    pub workspace_root: String,
    pub language: String,
    pub models: Vec<ModelRecord>,
}

#[derive(Clone, Debug)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub agent_id: String,
}

#[derive(Clone, Debug)]
pub struct TranscriptMessage {
    pub text: String,
    pub mine: bool,
    pub time: String,
    pub state: String,
}

#[derive(Clone, Debug)]
pub struct LocalApiBase {
    address: SocketAddrV4,
    host_header: String,
    path_prefix: String,
}

impl ConnectionConfig {
    /// `--connect` takes a local bridge URL such as `http://127.0.0.1:18123`.
    /// An optional `|TOKEN` can be supplied for bridge diagnostics. The token
    /// remains process-local and is never placed into Slint properties.
    pub fn from_process(
        first_argument: Option<OsString>,
        mut arguments: impl Iterator<Item = OsString>,
    ) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let Some(first_argument) = first_argument else {
            return Ok(None);
        };
        if first_argument != "--connect" {
            return Err(format!("unknown argument: {}", first_argument.to_string_lossy()).into());
        }
        let target = arguments
            .next()
            .ok_or("missing --connect target")?
            .to_string_lossy()
            .trim()
            .to_string();
        if target.is_empty() || arguments.next().is_some() {
            return Err("--connect accepts exactly one local bridge target".into());
        }
        Self::from_target(&target).map(Some)
    }

    pub fn from_target(target: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let (base, supplied_token) = target
            .split_once('|')
            .map_or((target, ""), |(base, token)| (base.trim(), token.trim()));
        let api_base = LocalApiBase::parse(base)?;
        if !supplied_token.is_empty() {
            return Ok(Self {
                api_base,
                token: supplied_token.to_string(),
            });
        }
        // Do not probe the bridge during UI construction. The desktop process can
        // publish its listener a little later; the first API operation resolves
        // the token on a worker thread instead of blocking the Slint event loop.
        Ok(Self {
            api_base,
            token: String::new(),
        })
    }
}

impl LocalApiBase {
    pub(crate) fn websocket_target(&self) -> (std::net::SocketAddr, String) {
        (
            self.address.into(),
            format!("ws://{}{}/chat/ws", self.host_header, self.path_prefix),
        )
    }
    fn parse(input: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let cleaned = input.trim().trim_end_matches('/');
        let Some(rest) = cleaned.strip_prefix("http://") else {
            return Err("--connect only supports the local http:// desktop bridge".into());
        };
        let (authority, suffix) = rest
            .split_once('/')
            .map_or((rest, ""), |(host, path)| (host, path));
        let (host, port) = authority.rsplit_once(':').unwrap_or((authority, "80"));
        let ip = host.parse::<Ipv4Addr>()?;
        if !ip.is_loopback() {
            return Err("--connect only accepts a loopback desktop bridge".into());
        }
        let port = port.parse::<u16>()?;
        let path_prefix = if suffix.is_empty() {
            API_PREFIX.to_string()
        } else {
            format!("/{}", suffix.trim_matches('/'))
        };
        if path_prefix != API_PREFIX {
            return Err("--connect target must be the desktop bridge root or /wunder".into());
        }
        Ok(Self {
            address: SocketAddrV4::new(ip, port),
            host_header: format!("{ip}:{port}"),
            path_prefix,
        })
    }

    fn bridge_root(&self) -> Self {
        Self {
            path_prefix: String::new(),
            ..self.clone()
        }
    }

    fn request_path(&self, suffix: &str) -> String {
        format!("{}{}", self.path_prefix, suffix)
    }
}

#[derive(Clone)]
pub struct ChatApi {
    pub(crate) api_base: LocalApiBase,
    token: Arc<Mutex<String>>,
}

impl ChatApi {
    pub fn new(config: ConnectionConfig) -> Result<Self, String> {
        if config.token.contains(['\r', '\n']) {
            return Err("invalid local access token".to_string());
        }
        Ok(Self {
            api_base: config.api_base,
            token: Arc::new(Mutex::new(config.token)),
        })
    }

    pub fn list_sessions(&self) -> Result<Vec<ChatSession>, String> {
        let payload = self.get_json("/chat/sessions?limit=100")?;
        let items = payload
            .pointer("/data/items")
            .and_then(Value::as_array)
            .ok_or("invalid session list response")?;
        Ok(items.iter().filter_map(parse_session).take(100).collect())
    }

    pub fn create_session_for_agent(&self, agent_id: Option<&str>) -> Result<ChatSession, String> {
        let payload = self.post_json("/chat/sessions", json!({ "agent_id": agent_id }))?;
        payload
            .get("data")
            .and_then(parse_session)
            .ok_or_else(|| "invalid create-session response".to_string())
    }

    pub fn get_session(
        &self,
        session_id: &str,
    ) -> Result<(ChatSession, Vec<TranscriptMessage>), String> {
        let path = format!(
            "/chat/sessions/{}?limit=100",
            encode_path_segment(session_id)
        );
        let payload = self.get_json(&path)?;
        let data = payload
            .get("data")
            .ok_or("invalid session detail response")?;
        let session = parse_session(data).ok_or("invalid session detail")?;
        let transcript = data
            .get("transcript")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(parse_transcript_message).collect())
            .unwrap_or_default();
        Ok((session, transcript))
    }

    pub fn cancel(&self, session_id: &str) -> Result<(), String> {
        self.post_json(
            &format!("/chat/sessions/{}/cancel", encode_path_segment(session_id)),
            json!({}),
        )
        .map(|_| ())
    }

    pub fn event_tail(&self, session_id: &str) -> Result<Value, String> {
        self.get_json(&format!(
            "/chat/sessions/{}/events?limit=1",
            encode_path_segment(session_id)
        ))
    }

    pub fn list_agents(&self) -> Result<Vec<AgentRecord>, String> {
        let payload = self.get_json("/agents?limit=100")?;
        let items = payload
            .pointer("/data/items")
            .and_then(Value::as_array)
            .ok_or("invalid agent list response")?;
        let default = self.get_json("/agents/__default__")?;
        let mut agents = Vec::with_capacity(items.len().min(99) + 1);
        agents.extend(default.get("data").and_then(parse_agent));
        agents.extend(items.iter().filter_map(parse_agent).take(99));
        Ok(agents)
    }

    pub fn create_agent(&self, name: &str) -> Result<AgentRecord, String> {
        let payload = self.post_json(
            "/agents",
            json!({
                "name": name,
                "copy_from_agent_id": "__default__",
            }),
        )?;
        payload
            .get("data")
            .and_then(parse_agent)
            .ok_or_else(|| "invalid create-agent response".to_string())
    }

    pub fn update_agent(
        &self,
        id: &str,
        name: &str,
        description: &str,
        system_prompt: &str,
        model_name: &str,
    ) -> Result<AgentRecord, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err("名称不能为空、超过 80 个字符或包含控制字符".into());
        }
        let description = validate_agent_text(description, "description")?;
        let system_prompt = validate_agent_text(system_prompt, "system prompt")?;
        let model_name = model_name.trim();
        if model_name.len() > 512 || model_name.contains(['\r', '\n']) {
            return Err("invalid model name".to_string());
        }
        let payload = self.put_json(
            &format!("/agents/{}", encode_path_segment(id)),
            json!({
                "name": name,
                "description": description,
                "system_prompt": system_prompt,
                "model_name": model_name,
            }),
        )?;
        payload
            .get("data")
            .and_then(parse_agent)
            .ok_or_else(|| "invalid update-agent response".to_string())
    }

    pub fn list_tools(&self) -> Result<Vec<ToolRecord>, String> {
        let payload = self.get_json("/user_tools/catalog")?;
        let data = payload.get("data").ok_or("invalid tool catalog response")?;
        let groups = [
            ("内置工具", "builtin_tools"),
            ("MCP 工具", "mcp_tools"),
            ("A2A 工具", "a2a_tools"),
            ("技能", "skills"),
            ("知识库", "knowledge_tools"),
            ("用户工具", "user_tools"),
            ("共享工具", "shared_tools"),
        ];
        let mut tools = Vec::new();
        for (category, key) in groups {
            if let Some(items) = data.get(key).and_then(Value::as_array) {
                tools.extend(
                    items
                        .iter()
                        .filter_map(|item| parse_tool(item, category))
                        .take(200 - tools.len()),
                );
            }
        }
        tools.truncate(200);
        Ok(tools)
    }

    pub fn get_desktop_settings(&self) -> Result<DesktopSettings, String> {
        let payload = self.get_json("/desktop/settings")?;
        parse_desktop_settings(&payload)
    }

    pub fn save_model(
        &self,
        key: &str,
        provider: &str,
        model: &str,
        base_url: &str,
        api_key: &str,
        model_type: &str,
    ) -> Result<DesktopSettings, String> {
        let key = validate_model_key(key)?;
        let model = validate_model_text(model, "model name")?;
        let provider = validate_model_text(provider, "provider")?;
        let base_url = base_url.trim();
        let model_type = normalize_model_type(model_type)?;
        let current = self.get_json("/desktop/settings")?;
        let mut llm = current
            .pointer("/data/llm")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let models = llm
            .as_object_mut()
            .and_then(|value| value.get_mut("models"))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "invalid desktop model settings".to_string())?;
        let mut next = models.get(&key).cloned().unwrap_or_else(|| json!({}));
        let fields = next
            .as_object_mut()
            .ok_or_else(|| "invalid desktop model entry".to_string())?;
        fields.insert("provider".to_string(), Value::String(provider.to_string()));
        fields.insert("model".to_string(), Value::String(model));
        fields.insert("base_url".to_string(), Value::String(base_url.to_string()));
        fields.insert("model_type".to_string(), Value::String(model_type));
        if !api_key.trim().is_empty() {
            if api_key.contains(['\r', '\n']) {
                return Err("invalid model access key".to_string());
            }
            fields.insert(
                "api_key".to_string(),
                Value::String(api_key.trim().to_string()),
            );
        }
        models.insert(key, next);
        let updated = self.put_json("/desktop/settings", json!({ "llm": llm }))?;
        parse_desktop_settings(&updated)
    }

    pub fn set_default_model(&self, key: &str) -> Result<DesktopSettings, String> {
        let key = validate_model_key(key)?;
        let current = self.get_json("/desktop/settings")?;
        let mut llm = current
            .pointer("/data/llm")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let selected = llm
            .get("models")
            .and_then(|models| models.get(&key))
            .ok_or("模型条目不存在，请刷新后重试")?;
        let model_type = selected
            .get("model_type")
            .and_then(Value::as_str)
            .unwrap_or("llm");
        let default_key = match normalize_model_type(model_type)?.as_str() {
            "embedding" => "default_embedding",
            "asr" => "default_asr",
            "tts" => "default_tts",
            "image" => "default_image",
            "video" => "default_video",
            _ => "default",
        };
        let fields = llm
            .as_object_mut()
            .ok_or_else(|| "invalid desktop model settings".to_string())?;
        fields.insert(default_key.to_string(), Value::String(key));
        let updated = self.put_json("/desktop/settings", json!({ "llm": llm }))?;
        parse_desktop_settings(&updated)
    }

    pub(crate) fn put_json(&self, suffix: &str, payload: Value) -> Result<Value, String> {
        let token = self.resolve_token()?;
        request_json(&self.api_base, "PUT", suffix, Some(&token), Some(payload))
    }
}

fn parse_desktop_settings(payload: &Value) -> Result<DesktopSettings, String> {
    let data = payload
        .get("data")
        .ok_or("invalid desktop settings response")?;
    let llm = data.get("llm").cloned().unwrap_or(Value::Null);
    let defaults = [
        ("llm", value_text(&llm, "default").unwrap_or_default()),
        (
            "embedding",
            value_text(&llm, "default_embedding").unwrap_or_default(),
        ),
        ("asr", value_text(&llm, "default_asr").unwrap_or_default()),
        ("tts", value_text(&llm, "default_tts").unwrap_or_default()),
        (
            "image",
            value_text(&llm, "default_image").unwrap_or_default(),
        ),
        (
            "video",
            value_text(&llm, "default_video").unwrap_or_default(),
        ),
    ];
    let models = llm
        .get("models")
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .take(200)
                .map(|(key, value)| {
                    let model_type =
                        value_text(value, "model_type").unwrap_or_else(|| "llm".to_string());
                    ModelRecord {
                        key: key.clone(),
                        provider: value_text(value, "provider").unwrap_or_default(),
                        model: value_text(value, "model").unwrap_or_default(),
                        base_url: value_text(value, "base_url").unwrap_or_default(),
                        model_type: model_type.clone(),
                        is_default: defaults
                            .iter()
                            .any(|(kind, default_key)| *kind == model_type && default_key == key),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(DesktopSettings {
        workspace_root: value_text(data, "workspace_root").unwrap_or_default(),
        language: value_text(data, "language").unwrap_or_default(),
        models,
    })
}

impl ChatApi {
    pub(crate) fn get_json(&self, suffix: &str) -> Result<Value, String> {
        let token = self.resolve_token()?;
        request_json(&self.api_base, "GET", suffix, Some(&token), None)
    }

    fn post_json(&self, suffix: &str, payload: Value) -> Result<Value, String> {
        let token = self.resolve_token()?;
        request_json(&self.api_base, "POST", suffix, Some(&token), Some(payload))
    }

    pub(crate) fn resolve_token(&self) -> Result<String, String> {
        match self.token.lock() {
            Ok(token) if !token.is_empty() => return Ok(token.clone()),
            _ => {}
        }
        let config = request_json(&self.api_base.bridge_root(), "GET", CONFIG_PATH, None, None)?;
        let token = value_text(&config, "desktop_token")
            .or_else(|| value_text(&config, "token"))
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "desktop bridge did not provide a local access token".to_string())?;
        if let Ok(mut cached) = self.token.lock() {
            *cached = token.clone();
        }
        Ok(token)
    }
}

fn validate_model_key(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() || key.chars().count() > 96 || key.chars().any(char::is_control) {
        return Err("模型配置键不能为空、超过 96 个字符或包含控制字符".to_string());
    }
    Ok(key.to_string())
}

fn validate_model_text(raw: &str, label: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty() || value.len() > 512 || value.contains(['\r', '\n']) {
        return Err(format!("invalid {label}"));
    }
    Ok(value.to_string())
}

fn normalize_model_type(raw: &str) -> Result<String, String> {
    let value = raw.trim().to_ascii_lowercase();
    if matches!(
        value.as_str(),
        "llm" | "embedding" | "asr" | "tts" | "image" | "video"
    ) {
        Ok(value)
    } else {
        Err("model type must be llm, embedding, asr, tts, image or video".to_string())
    }
}

fn request_json(
    base: &LocalApiBase,
    method: &str,
    suffix: &str,
    token: Option<&str>,
    payload: Option<Value>,
) -> Result<Value, String> {
    let has_payload = payload.is_some();
    let body = payload
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    let mut request = format!(
        "{method} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: application/json\r\n",
        base.request_path(suffix),
        base.host_header
    );
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        if token.contains(['\r', '\n']) {
            return Err("invalid local access token".to_string());
        }
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if has_payload {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    request.push_str(&body);

    let mut stream = TcpStream::connect_timeout(&base.address.into(), REQUEST_TIMEOUT)
        .map_err(|error| format!("connect local desktop bridge: {error}"))?;
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|error| format!("set read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|error| format!("set write timeout: {error}"))?;
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("send chat request: {error}"))?;
    let mut response = Vec::new();
    stream
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut response)
        .map_err(|error| format!("read chat response: {error}"))?;
    if response.len() > MAX_RESPONSE_BYTES {
        return Err("chat response exceeded the local UI size limit".to_string());
    }
    let response =
        String::from_utf8(response).map_err(|_| "chat response was not UTF-8".to_string())?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or("invalid HTTP response")?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or("invalid HTTP status")?;
    let payload: Value = serde_json::from_str(body)
        .map_err(|_| format!("chat endpoint returned invalid JSON (HTTP {status})"))?;
    if !(200..300).contains(&status) {
        return Err(response_error_text(&payload, status));
    }
    Ok(payload)
}

fn parse_session(value: &Value) -> Option<ChatSession> {
    let id = value_text(value, "id")?;
    Some(ChatSession {
        title: value_text(value, "title").unwrap_or_else(|| "新会话".to_string()),
        updated_at: value_text(value, "updated_at")
            .or_else(|| value_text(value, "last_message_at"))
            .unwrap_or_default(),
        agent_id: value_text(value, "agent_id").unwrap_or_default(),
        id,
    })
}

fn parse_agent(value: &Value) -> Option<AgentRecord> {
    Some(AgentRecord {
        id: value_text(value, "id")?,
        name: value_text(value, "name").unwrap_or_else(|| "未命名智能体".to_string()),
        description: value_text(value, "description").unwrap_or_default(),
        // Keep inheritance distinct from the effective default model.
        model: value
            .get("configured_model_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        system_prompt: value
            .get("system_prompt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        status: value_text(value, "status").unwrap_or_else(|| "active".to_string()),
    })
}

fn validate_agent_text(raw: &str, label: &str) -> Result<String, String> {
    // Preserve prompt indentation, line endings and trailing newlines verbatim.
    if raw.len() > 131_072 || raw.contains('\0') {
        return Err(format!("{label} 超过 128 KiB 或包含无效字符"));
    }
    Ok(raw.to_string())
}

fn parse_tool(value: &Value, category: &str) -> Option<ToolRecord> {
    Some(ToolRecord {
        name: value_text(value, "name").or_else(|| value_text(value, "title"))?,
        description: value_text(value, "description").unwrap_or_default(),
        category: category.to_string(),
    })
}

pub(crate) fn parse_file(value: &Value) -> Option<FileRecord> {
    let name = value_text(value, "name")?;
    Some(FileRecord {
        path: value_text(value, "path").unwrap_or_else(|| name.clone()),
        entry_type: value_text(value, "type").unwrap_or_else(|| "file".to_string()),
        size: value
            .get("size")
            .and_then(Value::as_u64)
            .map(format_size)
            .unwrap_or_default(),
        name,
    })
}

fn format_size(size: u64) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else if size >= 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{size} B")
    }
}

fn parse_transcript_message(value: &Value) -> Option<TranscriptMessage> {
    let role = value_text(value, "role")?;
    if role != "user" && role != "assistant" {
        return None;
    }
    // Message whitespace (including fenced code indentation) belongs to the user.
    let text = value.get("content")?.as_str()?.to_string();
    let mine = role.eq_ignore_ascii_case("user");
    let state = if mine {
        String::new()
    } else {
        match value_text(value, "status").as_deref() {
            Some("failed") => "执行失败".to_string(),
            Some("cancelled") => "已取消".to_string(),
            _ => "任务完成".to_string(),
        }
    };
    Some(TranscriptMessage {
        text,
        mine,
        time: short_time(
            value_text(value, "created_at")
                .as_deref()
                .unwrap_or_default(),
        ),
        state,
    })
}

fn value_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn response_error_text(payload: &Value, status: u16) -> String {
    let message = payload
        .pointer("/error/message")
        .or_else(|| payload.pointer("/detail/message"))
        .or_else(|| payload.get("message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("chat request rejected");
    format!("{message} (HTTP {status})")
}

fn short_time(value: &str) -> String {
    let trimmed = value.trim();
    trimmed
        .find('T')
        .and_then(|start| trimmed.get(start + 1..start + 6))
        .map(ToString::to_string)
        .unwrap_or_else(|| "刚刚".to_string())
}

fn encode_path_segment(input: &str) -> String {
    input
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub(crate) fn encode_query_value(input: &str) -> String {
    encode_path_segment(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherited_model_and_prompt_whitespace_survive_projection() {
        let record = parse_agent(&json!({"id":"test", "configured_model_name":null,
            "model_name":"effective", "system_prompt":"  line\r\nnext\n"}))
        .unwrap();
        assert_eq!(
            (record.model, record.system_prompt),
            (String::new(), "  line\r\nnext\n".into())
        );
        assert_eq!(
            validate_agent_text("  line\r\nnext\n", "prompt"),
            Ok("  line\r\nnext\n".into())
        );
    }
}
