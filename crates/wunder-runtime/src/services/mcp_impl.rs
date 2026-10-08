// MCP 服务与客户端：对齐 codex-main 的 rmcp SDK，用于流式 HTTP 与工具调用。
use crate::attachment::{convert_to_markdown, get_supported_extensions, sanitize_filename_stem};
use crate::config::{Config, McpServerConfig};
use crate::i18n;
use crate::schemas::{ToolSpec, WunderRequest};
use crate::state::AppState;
use crate::tools::{
    browser_tools_available, builtin_aliases, is_browser_tool_name, resolve_tool_name,
};
use anyhow::{anyhow, Result};
use axum::Router;
use futures::StreamExt;
use parking_lot::Mutex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use rmcp::handler::client::ClientHandler;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ErrorData as McpError, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{serve_client, RequestContext, RoleServer};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{
    StreamableHttpClientTransport, StreamableHttpServerConfig, StreamableHttpService,
};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

const MCP_SERVER_NAME: &str = "wunder";
const MCP_EXECUTE_TOOL_NAME: &str = "excute";
const MCP_DOC2MD_TOOL_NAME: &str = "doc2md";
const MCP_USER_ID: &str = "wunder";
const MCP_INSTRUCTIONS: &str = "调用 wunder 智能体执行任务或解析文档并返回结果。";
const MCP_EXECUTE_DESCRIPTION: &str = "执行 wunder 智能体任务并返回最终回复。";
const MCP_DOC2MD_DESCRIPTION: &str = "解析文档并返回 Markdown 文本。";

const MCP_TOOL_CACHE_TTL_S: f64 = 30.0;
const MCP_TOOL_CACHE_MAX_ENTRIES: usize = 128;

pub(crate) fn normalize_transport(transport: Option<&str>) -> String {
    let value = transport.unwrap_or("streamable-http").trim();
    if value.is_empty() || value.eq_ignore_ascii_case("http") {
        return "streamable-http".to_string();
    }
    match value.to_ascii_lowercase().as_str() {
        "streamable-http" | "streamable_http" | "streamablehttp" => "streamable-http".to_string(),
        other => other.to_string(),
    }
}

fn mcp_client_cache() -> &'static Mutex<HashMap<String, reqwest::Client>> {
    static CACHE: OnceLock<Mutex<HashMap<String, reqwest::Client>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn build_mcp_client_key(headers: &HeaderMap, timeout_s: Option<u64>) -> String {
    let mut pairs = headers
        .iter()
        .map(|(key, value)| {
            let value_text = value
                .to_str()
                .map(str::to_string)
                .unwrap_or_else(|_| String::from_utf8_lossy(value.as_bytes()).to_string());
            format!("{}={}", key.as_str().to_lowercase(), value_text)
        })
        .collect::<Vec<_>>();
    pairs.sort();
    format!("timeout={};{}", timeout_s.unwrap_or(0), pairs.join(";"))
}

/// 构建 MCP 服务路由：挂载 /wunder/mcp 的 Streamable HTTP 服务。
pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    let service = StreamableHttpService::new(
        {
            let state = state.clone();
            move || Ok(WunderMcpServer::new(state.clone()))
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    Router::new().nest_service("/wunder/mcp", service)
}

/// MCP 服务器实现：暴露 wunder@excute/doc2md 工具。
#[derive(Clone)]
struct WunderMcpServer {
    state: Arc<AppState>,
}

impl WunderMcpServer {
    fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    fn execute_tool() -> Tool {
        let schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "task": { "type": "string" }
            },
            "required": ["task"]
        }))
        .unwrap_or_default();
        Tool::new(
            Cow::Borrowed(MCP_EXECUTE_TOOL_NAME),
            Cow::Borrowed(MCP_EXECUTE_DESCRIPTION),
            Arc::new(schema),
        )
    }

    fn doc2md_tool() -> Tool {
        let schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "source_url": { "type": "string", "description": "文件下载地址（URL，需包含扩展名）" }
            },
            "required": ["source_url"]
        }))
        .unwrap_or_default();
        Tool::new(
            Cow::Borrowed(MCP_DOC2MD_TOOL_NAME),
            Cow::Borrowed(MCP_DOC2MD_DESCRIPTION),
            Arc::new(schema),
        )
    }

    fn build_allowed_tool_names(config: &Config) -> Vec<String> {
        let mut names: HashSet<String> = HashSet::new();
        for name in &config.tools.builtin.enabled {
            let canonical = resolve_tool_name(name);
            if is_browser_tool_name(&canonical) && !browser_tools_available(config) {
                continue;
            }
            names.insert(canonical);
        }
        let alias_map = builtin_aliases();
        for (alias, canonical) in alias_map {
            if names.contains(&canonical) {
                names.insert(alias);
            }
        }
        for server in &config.mcp.servers {
            if !server.enabled {
                continue;
            }
            if server.packaged {
                names.insert(crate::tools::mcp_pack_runtime_name(&server.name));
                continue;
            }
            for tool in &server.tool_specs {
                if tool.name.is_empty() {
                    continue;
                }
                names.insert(format!("{}@{}", server.name, tool.name));
            }
        }
        names.remove(&format!("{}@{}", MCP_SERVER_NAME, MCP_EXECUTE_TOOL_NAME));
        names.remove("a2ui");
        let mut sorted: Vec<String> = names.into_iter().collect();
        sorted.sort();
        sorted
    }
}

impl ServerHandler for WunderMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
        )
        .with_server_info(Implementation::new(
            MCP_SERVER_NAME,
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(MCP_INSTRUCTIONS.to_string())
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let tools = vec![Self::execute_tool(), Self::doc2md_tool()];
        async move { Ok(ListToolsResult::with_all_items(tools)) }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let tool_name = request.name.as_ref();
        if tool_name == MCP_DOC2MD_TOOL_NAME {
            return self
                .handle_doc2md(request.arguments.as_ref())
                .await
                .map(Into::into);
        }
        if tool_name != MCP_EXECUTE_TOOL_NAME {
            return Err(McpError::invalid_params("未知 MCP 工具", None));
        }
        let task = request
            .arguments
            .as_ref()
            .and_then(|args| args.get("task"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_default();
        if task.trim().is_empty() {
            return Err(McpError::invalid_params("任务内容不能为空", None));
        }
        let config = self.state.config_store.get().await;
        let tool_names = Self::build_allowed_tool_names(&config);
        let response = self
            .state
            .kernel
            .orchestrator
            .run(WunderRequest {
                user_id: MCP_USER_ID.to_string(),
                question: task,
                client_message_id: None,
                tool_names,
                skip_tool_calls: false,
                stream: false,
                session_id: None,
                agent_id: None,
                workspace_container_id: None,
                workspace_id: None,
                model_name: None,
                language: Some(i18n::get_default_language()),
                config_overrides: None,
                agent_prompt: None,
                preview_skill: false,
                attachments: None,
                allow_queue: true,
                is_admin: false,
                enforce_runtime_queue: false,
                approval_tx: None,
            })
            .await
            .map_err(|err| {
                McpError::internal_error(
                    "执行 wunder 任务失败",
                    Some(json!({ "detail": err.to_string() })),
                )
            })?;
        let payload = json!({
            "answer": response.answer,
            "session_id": response.session_id,
            "usage": response.usage,
            "uid": response.uid,
            "a2ui": response.a2ui,
        });
        Ok(CallToolResult::structured(payload).into())
    }
}

impl WunderMcpServer {
    async fn handle_doc2md(
        &self,
        arguments: Option<&JsonObject>,
    ) -> Result<CallToolResult, McpError> {
        let empty_args = JsonObject::new();
        let args = arguments.unwrap_or(&empty_args);
        let source_url = args
            .get("source_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if source_url.is_empty() {
            return Err(McpError::invalid_params("source_url 不能为空", None));
        }
        let parsed_url = url::Url::parse(&source_url)
            .map_err(|err| McpError::invalid_params(format!("source_url 无效: {err}"), None))?;
        let name = parsed_url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .unwrap_or("")
            .trim();
        let name = if name.is_empty() { "document" } else { name }.to_string();
        let extension = Path::new(&name)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();
        if extension.is_empty() {
            return Err(McpError::invalid_params("source_url 缺少文件扩展名", None));
        }
        let extension = format!(".{extension}");
        let supported = get_supported_extensions();
        if !supported
            .iter()
            .any(|item| item.eq_ignore_ascii_case(&extension))
        {
            let message = i18n::t_with_params(
                "error.unsupported_file_type",
                &HashMap::from([("extension".to_string(), extension.clone())]),
            );
            return Err(McpError::invalid_params(message, None));
        }
        let stem = Path::new(&name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("document");
        let stem = sanitize_filename_stem(stem);
        let stem = if stem.trim().is_empty() {
            "document".to_string()
        } else {
            stem
        };
        let temp_dir = create_doc2md_temp_dir().await.map_err(|err| {
            McpError::internal_error(
                "创建临时目录失败",
                Some(json!({ "detail": err.to_string() })),
            )
        })?;
        let input_path = temp_dir.join(format!("{stem}{extension}"));
        let output_path = temp_dir.join(format!("{stem}.md"));
        let result = async {
            let response = reqwest::Client::new()
                .get(&source_url)
                .send()
                .await
                .map_err(|err| {
                    McpError::internal_error(
                        "下载文件失败",
                        Some(json!({ "detail": err.to_string() })),
                    )
                })?;
            let status = response.status();
            if !status.is_success() {
                let detail = response.text().await.unwrap_or_default();
                return Err(McpError::internal_error(
                    "下载文件失败",
                    Some(json!({
                        "status": status.as_u16(),
                        "detail": detail
                    })),
                ));
            }
            {
                let mut file = tokio::fs::File::create(&input_path).await.map_err(|err| {
                    McpError::internal_error(
                        "写入临时文件失败",
                        Some(json!({ "detail": err.to_string() })),
                    )
                })?;
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|err| {
                        McpError::internal_error(
                            "下载文件失败",
                            Some(json!({ "detail": err.to_string() })),
                        )
                    })?;
                    file.write_all(&chunk).await.map_err(|err| {
                        McpError::internal_error(
                            "写入临时文件失败",
                            Some(json!({ "detail": err.to_string() })),
                        )
                    })?;
                }
                file.flush().await.map_err(|err| {
                    McpError::internal_error(
                        "写入临时文件失败",
                        Some(json!({ "detail": err.to_string() })),
                    )
                })?;
            }
            let conversion = convert_to_markdown(&input_path, &output_path, &extension)
                .await
                .map_err(|err| {
                    McpError::internal_error(
                        "文档解析失败",
                        Some(json!({ "detail": err.to_string() })),
                    )
                })?;
            let content = tokio::fs::read_to_string(&output_path)
                .await
                .map_err(|err| {
                    McpError::internal_error(
                        "读取解析结果失败",
                        Some(json!({ "detail": err.to_string() })),
                    )
                })?;
            if content.trim().is_empty() {
                return Err(McpError::internal_error("文档解析结果为空", None));
            }
            Ok((conversion, content))
        }
        .await;
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        let (conversion, content) = result?;
        Ok(CallToolResult::structured(json!({
                "ok": true,
                "name": name,
                "content": content,
                "converter": conversion.converter,
                "warnings": conversion.warnings,
        })))
    }
}

async fn create_doc2md_temp_dir() -> Result<PathBuf, std::io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_builtin_mcp_doc2md");
    root.push(Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&root).await?;
    Ok(root)
}

#[derive(Clone, Default)]
struct NoopClientHandler;

impl ClientHandler for NoopClientHandler {}

#[derive(Clone)]
struct McpToolCacheEntry {
    specs: Vec<ToolSpec>,
    timestamp: f64,
}

fn mcp_tool_cache() -> &'static Mutex<HashMap<String, McpToolCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, McpToolCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn build_mcp_tool_cache_key(server: &McpServerConfig) -> String {
    let mut allow = server.allow_tools.clone();
    allow.sort();
    let allow_key = allow.join(",");

    let mut headers = server
        .headers
        .iter()
        .map(|(key, value)| format!("{}={}", key.to_lowercase(), value))
        .collect::<Vec<_>>();
    headers.sort();
    let headers_key = headers.join(";");

    let auth_key = server
        .auth
        .as_ref()
        .and_then(|value| serde_json::to_string(value).ok())
        .unwrap_or_default();
    let transport = server.transport.clone().unwrap_or_default();
    format!(
        "name={};endpoint={};transport={};allow={};headers={};auth={}",
        server.name.trim(),
        server.endpoint.trim(),
        transport,
        allow_key,
        headers_key,
        auth_key
    )
}

fn get_cached_mcp_tool_specs(key: &str) -> Option<Vec<ToolSpec>> {
    if MCP_TOOL_CACHE_TTL_S <= 0.0 {
        return None;
    }
    let now = now_ts();
    let mut cache = mcp_tool_cache().lock();
    if let Some(entry) = cache.get(key) {
        if now - entry.timestamp <= MCP_TOOL_CACHE_TTL_S {
            return Some(entry.specs.clone());
        }
    }
    cache.remove(key);
    None
}

fn store_mcp_tool_specs(key: String, specs: Vec<ToolSpec>) {
    if MCP_TOOL_CACHE_TTL_S <= 0.0 {
        return;
    }
    let now = now_ts();
    let mut cache = mcp_tool_cache().lock();
    cache.insert(
        key,
        McpToolCacheEntry {
            specs,
            timestamp: now,
        },
    );
    evict_mcp_tool_cache(&mut cache, now);
}

fn evict_mcp_tool_cache(cache: &mut HashMap<String, McpToolCacheEntry>, now: f64) {
    let mut expired = Vec::new();
    for (key, entry) in cache.iter() {
        if now - entry.timestamp > MCP_TOOL_CACHE_TTL_S {
            expired.push(key.clone());
        }
    }
    for key in expired {
        cache.remove(&key);
    }
    if MCP_TOOL_CACHE_MAX_ENTRIES == 0 || cache.len() <= MCP_TOOL_CACHE_MAX_ENTRIES {
        return;
    }
    let mut items = cache
        .iter()
        .map(|(key, entry)| (key.clone(), entry.timestamp))
        .collect::<Vec<_>>();
    items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let overflow = cache.len().saturating_sub(MCP_TOOL_CACHE_MAX_ENTRIES);
    for (key, _) in items.into_iter().take(overflow) {
        cache.remove(&key);
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

/// 查询 MCP 工具列表，转换为 Wunder ToolSpec。
pub async fn fetch_tools(config: &Config, server: &McpServerConfig) -> Result<Vec<ToolSpec>> {
    let cache_key = build_mcp_tool_cache_key(server);
    if let Some(cached) = get_cached_mcp_tool_specs(&cache_key) {
        return Ok(cached);
    }

    let transport = normalize_transport(server.transport.as_deref());
    if transport != "streamable-http" {
        return Err(anyhow!(
            "不再支持旧版 MCP 传输类型: {transport}，请改用 streamable-http"
        ));
    }
    let transport = build_transport(config, server)?;
    let service = serve_client(NoopClientHandler, transport).await?;
    let tools = service.list_all_tools().await?;
    let specs = collect_tool_specs(server, tools);
    store_mcp_tool_specs(cache_key, specs.clone());
    Ok(specs)
}

fn collect_tool_specs(server: &McpServerConfig, tools: Vec<Tool>) -> Vec<ToolSpec> {
    // 统一处理 MCP 工具过滤与描述兜底，避免不同传输分支重复实现。
    let allow_list = server.allow_tools.iter().cloned().collect::<HashSet<_>>();
    let mut items = Vec::new();
    for tool in tools {
        let name = tool.name.to_string();
        if name.is_empty() {
            continue;
        }
        if !allow_list.is_empty() && !allow_list.contains(&name) {
            continue;
        }
        let description = tool.description.as_deref().unwrap_or("").trim().to_string();
        let fallback = server
            .description
            .clone()
            .or_else(|| server.display_name.clone())
            .unwrap_or_default();
        items.push(ToolSpec {
            name,
            title: tool.title.clone(),
            description: if description.is_empty() {
                fallback
            } else {
                description
            },
            input_schema: tool.schema_as_json_value(),
        });
    }
    items
}

pub fn build_tool_specs_from_config(server: &McpServerConfig) -> Vec<ToolSpec> {
    server
        .tool_specs
        .iter()
        .map(|spec| ToolSpec {
            name: spec.name.clone(),
            title: spec.title.clone(),
            description: spec.description.clone(),
            input_schema: serde_json::to_value(&spec.input_schema).unwrap_or(Value::Null),
        })
        .collect()
}

/// 调用 MCP 工具并返回结构化结果。
pub async fn call_tool(
    config: &Config,
    server_name: &str,
    tool_name: &str,
    args: &Value,
) -> Result<Value> {
    let server = config
        .mcp
        .servers
        .iter()
        .find(|item| item.name == server_name)
        .ok_or_else(|| anyhow!("MCP 服务不存在: {server_name}"))?;
    call_tool_with_server(config, server, tool_name, args).await
}

/// 使用指定的 MCP 服务配置调用工具，支持用户自定义服务。
pub async fn call_tool_with_server(
    config: &Config,
    server: &McpServerConfig,
    tool_name: &str,
    args: &Value,
) -> Result<Value> {
    if !server.enabled {
        return Err(anyhow!("MCP 服务已禁用: {}", server.name));
    }
    if !server.allow_tools.is_empty() && !server.allow_tools.contains(&tool_name.to_string()) {
        return Err(anyhow!("MCP 工具不在允许列表中"));
    }
    let transport = normalize_transport(server.transport.as_deref());
    if transport != "streamable-http" {
        return Err(anyhow!(
            "不再支持旧版 MCP 传输类型: {transport}，请改用 streamable-http"
        ));
    }
    let transport = build_transport(config, server)?;
    let service = serve_client(NoopClientHandler, transport).await?;
    let result = service
        .call_tool(build_call_tool_request_params(tool_name, args))
        .await?;
    Ok(serialize_tool_result(result))
}

fn build_call_tool_request_params(tool_name: &str, args: &Value) -> CallToolRequestParams {
    let mut params = CallToolRequestParams::new(Cow::Owned(tool_name.to_string()));
    params.arguments = normalize_mcp_arguments(args);
    params
}

fn normalize_mcp_arguments(args: &Value) -> Option<JsonObject> {
    // MCP 只接受对象参数，其它类型统一视为无参数。
    match args {
        Value::Object(map) => Some(map.clone()),
        Value::Null => None,
        _ => None,
    }
}

fn serialize_tool_result(result: CallToolResult) -> Value {
    // 统一将 MCP 返回内容序列化为结构化 JSON，保持前端解析一致。
    let mut content = result
        .content
        .into_iter()
        .map(|block| serde_json::to_value(block).unwrap_or(Value::Null))
        .collect::<Vec<_>>();
    let structured_content = result.structured_content;
    if should_drop_duplicate_text_content(&content, structured_content.as_ref()) {
        content.clear();
    }
    json!({
        "content": content,
        "structured_content": structured_content,
        "meta": result.meta,
        "is_error": result.is_error,
    })
}

fn should_drop_duplicate_text_content(
    content: &[Value],
    structured_content: Option<&Value>,
) -> bool {
    let Some(structured_content) = structured_content else {
        return false;
    };
    if structured_content.is_null() || content.is_empty() {
        return false;
    }
    parse_json_from_single_text_block(content).as_ref() == Some(structured_content)
}

fn parse_json_from_single_text_block(content: &[Value]) -> Option<Value> {
    if content.len() != 1 {
        return None;
    }
    let block = content.first()?.as_object()?;
    if block.get("type").and_then(Value::as_str) != Some("text") {
        return None;
    }
    let text = block.get("text").and_then(Value::as_str)?.trim();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(text).ok()
}

fn build_mcp_client(headers: HeaderMap, timeout_s: Option<u64>) -> Result<reqwest::Client> {
    let key = build_mcp_client_key(&headers, timeout_s);
    if let Some(client) = mcp_client_cache().lock().get(&key) {
        return Ok(client.clone());
    }
    let mut builder = reqwest::Client::builder().default_headers(headers);
    if let Some(seconds) = timeout_s.filter(|seconds| *seconds > 0) {
        builder = builder.timeout(std::time::Duration::from_secs(seconds));
    }
    let client = builder.build()?;
    mcp_client_cache().lock().insert(key, client.clone());
    Ok(client)
}

fn build_transport(
    config: &Config,
    server: &McpServerConfig,
) -> Result<StreamableHttpClientTransport<reqwest::Client>> {
    let transport = normalize_transport(server.transport.as_deref());
    if transport != "streamable-http" {
        return Err(anyhow!("暂不支持的 MCP 传输类型: {transport}"));
    }
    let headers = build_mcp_headers(config, server)?;
    let timeout_s = if config.mcp.timeout_s > 0 {
        Some(config.mcp.timeout_s)
    } else {
        None
    };
    let client = build_mcp_client(headers, timeout_s)?;
    let http_config = StreamableHttpClientTransportConfig::with_uri(server.endpoint.clone());
    Ok(StreamableHttpClientTransport::with_client(
        client,
        http_config,
    ))
}

fn build_mcp_headers(config: &Config, server: &McpServerConfig) -> Result<HeaderMap> {
    let mut header_map = HeaderMap::new();
    for (key, value) in &server.headers {
        let name = HeaderName::from_bytes(key.as_bytes())?;
        let value = HeaderValue::from_str(value)?;
        header_map.insert(name, value);
    }
    if should_attach_api_key(config, server) {
        let has_auth = header_map
            .keys()
            .any(|key| key.as_str().eq_ignore_ascii_case("authorization"));
        let has_api_key = header_map
            .keys()
            .any(|key| key.as_str().eq_ignore_ascii_case("x-api-key"));
        if !has_auth && !has_api_key {
            if let Some(api_key) = config.api_key() {
                let value = HeaderValue::from_str(&api_key)?;
                header_map.insert(HeaderName::from_static("x-api-key"), value);
            }
        }
    }
    if let Some(auth) = &server.auth {
        let auth_json = serde_json::to_value(auth).unwrap_or(Value::Null);
        let Value::Object(map) = auth_json else {
            return Ok(header_map);
        };
        if let Some(Value::String(token)) = map.get("bearer_token") {
            let header = HeaderValue::from_str(&format!("Bearer {token}"))?;
            header_map.insert(AUTHORIZATION, header);
        }
        if let Some(Value::String(token)) = map.get("token") {
            let header = HeaderValue::from_str(&format!("Bearer {token}"))?;
            header_map.insert(AUTHORIZATION, header);
        }
        if let Some(Value::String(token)) = map.get("api_key") {
            let header = HeaderValue::from_str(token)?;
            header_map.insert(HeaderName::from_static("x-api-key"), header);
        }
    }
    Ok(header_map)
}

fn should_attach_api_key(config: &Config, server: &McpServerConfig) -> bool {
    if config.api_key().is_none() {
        return false;
    }
    if server.name.eq_ignore_ascii_case(MCP_SERVER_NAME) {
        return true;
    }
    if let Ok(parsed) = url::Url::parse(&server.endpoint) {
        let path = parsed.path().trim_end_matches('/');
        return path.ends_with("/wunder/mcp");
    }
    false
}
