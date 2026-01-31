// MCP 服务与客户端：对齐 codex-main 的 rmcp SDK，用于流式 HTTP 与工具调用。
use crate::attachment::{convert_to_markdown, get_supported_extensions, sanitize_filename_stem};
use crate::config::{Config, McpServerConfig};
use crate::i18n;
use crate::schemas::{ToolSpec, WunderRequest};
use crate::state::AppState;
use crate::tools::{builtin_aliases, resolve_tool_name};
use anyhow::{anyhow, Result};
use axum::Router;
use futures::StreamExt;
use parking_lot::Mutex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION};
use rmcp::handler::client::ClientHandler;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParam, CallToolResult, ClientCapabilities, ClientJsonRpcMessage,
    ClientNotification, ClientRequest, ErrorData as McpError, Implementation,
    InitializeRequestParam, InitializedNotification, JsonObject, ListToolsRequest, ListToolsResult,
    PaginatedRequestParam, ProtocolVersion, Request, RequestId, ServerCapabilities, ServerInfo,
    ServerJsonRpcMessage, ServerResult, Tool,
};
use rmcp::service::{serve_client, RequestContext, RoleServer};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{
    StreamableHttpClientTransport, StreamableHttpServerConfig, StreamableHttpService,
};
use serde_json::{json, Value};
use sse_stream::SseStream;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, Instant};
use tracing::{debug, info, warn};
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
            names.insert(resolve_tool_name(name));
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
            for tool in &server.tool_specs {
                if tool.name.is_empty() {
                    continue;
                }
                names.insert(format!("{}@{}", server.name, tool.name));
            }
        }
        for service in &config.a2a.services {
            if !service.enabled {
                continue;
            }
            if service.name.is_empty() {
                continue;
            }
            names.insert(format!("a2a@{}", service.name));
        }
        names.remove(&format!("{}@{}", MCP_SERVER_NAME, MCP_EXECUTE_TOOL_NAME));
        names.remove("a2ui");
        let mut sorted: Vec<String> = names.into_iter().collect();
        sorted.sort();
        sorted
    }
}

impl ServerHandler for WunderMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
            instructions: Some(MCP_INSTRUCTIONS.to_string()),
            ..ServerInfo::default()
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let tools = vec![Self::execute_tool(), Self::doc2md_tool()];
        async move {
            Ok(ListToolsResult {
                tools,
                next_cursor: None,
                meta: None,
            })
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tool_name = request.name.as_ref();
        if tool_name != MCP_EXECUTE_TOOL_NAME && tool_name != MCP_DOC2MD_TOOL_NAME {
            return Err(McpError::invalid_params("未知 MCP 工具", None));
        }
        if tool_name == MCP_DOC2MD_TOOL_NAME {
            return self.handle_doc2md(request.arguments.as_ref()).await;
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
            .orchestrator
            .run(WunderRequest {
                user_id: MCP_USER_ID.to_string(),
                question: task,
                tool_names,
                skip_tool_calls: false,
                stream: false,
                debug_payload: false,
                session_id: None,
                agent_id: None,
                model_name: None,
                language: Some(i18n::get_default_language()),
                config_overrides: None,
                agent_prompt: None,
                attachments: None,
                is_admin: false,
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
        Ok(CallToolResult {
            content: Vec::new(),
            structured_content: Some(payload),
            is_error: Some(false),
            meta: None,
        })
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
            .and_then(|segments| segments.last())
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
        Ok(CallToolResult {
            content: Vec::new(),
            structured_content: Some(json!({
                "ok": true,
                "name": name,
                "content": content,
                "converter": conversion.converter,
                "warnings": conversion.warnings,
            })),
            is_error: Some(false),
            meta: None,
        })
    }
}

async fn create_doc2md_temp_dir() -> Result<PathBuf, std::io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_mcp_doc2md");
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
    let specs = if transport == "sse" {
        fetch_tools_sse(config, server).await?
    } else {
        let transport = build_transport(config, server)?;
        let service = serve_client(NoopClientHandler::default(), transport).await?;
        let tools = service.list_all_tools().await?;
        collect_tool_specs(server, tools)
    };
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
    if transport == "sse" {
        return call_tool_sse(config, server, tool_name, args).await;
    }
    let transport = build_transport(config, server)?;
    let service = serve_client(NoopClientHandler::default(), transport).await?;
    let result = service
        .call_tool(CallToolRequestParam {
            name: Cow::Owned(tool_name.to_string()),
            arguments: normalize_mcp_arguments(args),
        })
        .await?;
    Ok(serialize_tool_result(result))
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
    let content = result
        .content
        .into_iter()
        .map(|block| serde_json::to_value(block).unwrap_or(Value::Null))
        .collect::<Vec<_>>();
    json!({
        "content": content,
        "structured_content": result.structured_content,
        "meta": result.meta,
        "is_error": result.is_error,
    })
}

struct SseClientSession {
    endpoint: url::Url,
    receiver: mpsc::Receiver<ServerJsonRpcMessage>,
    client: reqwest::Client,
    task: tokio::task::JoinHandle<()>,
    timeout: Option<Duration>,
    next_id: i64,
}

fn request_id_matches(expected: &RequestId, actual: &RequestId) -> bool {
    if expected == actual {
        return true;
    }
    match (expected, actual) {
        (RequestId::Number(expected), RequestId::String(actual))
        | (RequestId::String(actual), RequestId::Number(expected)) => actual
            .parse::<i64>()
            .ok()
            .is_some_and(|value| value == *expected),
        _ => false,
    }
}

impl SseClientSession {
    async fn connect(config: &Config, server: &McpServerConfig) -> Result<Self> {
        let headers = build_mcp_headers(config, server)?;
        let timeout_s = if config.mcp.timeout_s > 0 {
            Some(config.mcp.timeout_s)
        } else {
            None
        };
        let timeout = timeout_s.map(Duration::from_secs);
        let client = build_mcp_client(headers, timeout_s)?;
        let response = client
            .get(&server.endpoint)
            .header(ACCEPT, HeaderValue::from_static("text/event-stream"))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("MCP SSE 连接失败: {status} {body}"));
        }

        let base_url = url::Url::parse(&server.endpoint)?;
        let (tx, rx) = mpsc::channel(64);
        let (endpoint_tx, endpoint_rx) = oneshot::channel::<Result<url::Url>>();
        let handle = tokio::spawn(async move {
            let mut endpoint_tx = Some(endpoint_tx);
            let mut stream = SseStream::from_byte_stream(response.bytes_stream());
            while let Some(item) = stream.next().await {
                match item {
                    Ok(event) => {
                        let event_name = event.event.as_deref().unwrap_or("message");
                        match event_name {
                            "endpoint" => {
                                if let Some(tx) = endpoint_tx.take() {
                                    let data = event.data.unwrap_or_default();
                                    let result = resolve_sse_endpoint(&base_url, &data);
                                    let _ = tx.send(result);
                                }
                            }
                            "message" | "" => {
                                let Some(data) = event.data else {
                                    continue;
                                };
                                if data.trim().is_empty() {
                                    continue;
                                }
                                match serde_json::from_str::<ServerJsonRpcMessage>(&data) {
                                    Ok(message) => {
                                        if tx.send(message).await.is_err() {
                                            break;
                                        }
                                    }
                                    Err(err) => {
                                        warn!("MCP SSE 消息解析失败: {err}");
                                    }
                                }
                            }
                            _ => {
                                debug!("忽略 MCP SSE 事件: {event_name}");
                            }
                        }
                    }
                    Err(err) => {
                        warn!("MCP SSE 流解析失败: {err}");
                        break;
                    }
                }
            }
            if let Some(tx) = endpoint_tx.take() {
                let _ = tx.send(Err(anyhow!("MCP SSE 未返回 endpoint 事件")));
            }
        });

        let endpoint = wait_for_endpoint(endpoint_rx, timeout).await?;
        info!("MCP SSE 已连接: {}", endpoint.as_str());
        Ok(SseClientSession {
            endpoint,
            receiver: rx,
            client,
            task: handle,
            timeout,
            next_id: 1,
        })
    }

    async fn initialize(&mut self) -> Result<()> {
        let params = InitializeRequestParam {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ClientCapabilities::default(),
            client_info: Implementation::from_build_env(),
        };
        let request = ClientRequest::InitializeRequest(Request::new(params));
        let result = self.request(request).await?;
        match result {
            ServerResult::InitializeResult(_) => {}
            other => {
                return Err(anyhow!("MCP SSE 初始化返回类型异常: {other:?}"));
            }
        }
        self.notify(ClientNotification::InitializedNotification(
            InitializedNotification::default(),
        ))
        .await?;
        Ok(())
    }

    async fn list_tools(&mut self) -> Result<ListToolsResult> {
        let request = ClientRequest::ListToolsRequest(ListToolsRequest::default());
        let result = self.request(request).await?;
        match result {
            ServerResult::ListToolsResult(tools) => Ok(tools),
            other => Err(anyhow!("MCP SSE tools/list 返回类型异常: {other:?}")),
        }
    }

    async fn call_tool(&mut self, tool_name: &str, args: &Value) -> Result<CallToolResult> {
        let params = CallToolRequestParam {
            name: Cow::Owned(tool_name.to_string()),
            arguments: normalize_mcp_arguments(args),
        };
        let request = ClientRequest::CallToolRequest(Request::new(params));
        let result = self.request(request).await?;
        match result {
            ServerResult::CallToolResult(output) => Ok(output),
            other => Err(anyhow!("MCP SSE tools/call 返回类型异常: {other:?}")),
        }
    }

    async fn request(&mut self, request: ClientRequest) -> Result<ServerResult> {
        let request_id = self.next_request_id();
        let message = ClientJsonRpcMessage::request(request, request_id.clone());
        self.send_message(&message).await?;
        self.await_response(request_id).await
    }

    async fn notify(&self, notification: ClientNotification) -> Result<()> {
        let message = ClientJsonRpcMessage::notification(notification);
        self.send_message(&message).await
    }

    async fn send_message(&self, message: &ClientJsonRpcMessage) -> Result<()> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .header(ACCEPT, HeaderValue::from_static("application/json"))
            .json(message)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("MCP SSE 消息发送失败: {status} {body}"));
        }
        Ok(())
    }

    async fn await_response(&mut self, request_id: RequestId) -> Result<ServerResult> {
        let deadline = self.timeout.map(|timeout| Instant::now() + timeout);
        loop {
            let next = match deadline {
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(anyhow!("MCP SSE 等待响应超时"));
                    }
                    match tokio::time::timeout(remaining, self.receiver.recv()).await {
                        Ok(value) => value,
                        Err(_) => return Err(anyhow!("MCP SSE 等待响应超时")),
                    }
                }
                None => self.receiver.recv().await,
            };
            let Some(message) = next else {
                return Err(anyhow!("MCP SSE 连接已关闭"));
            };

            match message {
                ServerJsonRpcMessage::Response(response) => {
                    if request_id_matches(&request_id, &response.id) {
                        return Ok(response.result);
                    }
                    debug!("忽略 MCP SSE 未匹配响应: {}", response.id);
                }
                ServerJsonRpcMessage::Error(error) => {
                    if request_id_matches(&request_id, &error.id) {
                        return Err(anyhow!(
                            "MCP SSE 响应错误: {} ({:?})",
                            error.error.message,
                            error.error.data
                        ));
                    }
                    debug!("忽略 MCP SSE 未匹配错误: {}", error.id);
                }
                ServerJsonRpcMessage::Request(request) => {
                    warn!(
                        "MCP SSE 收到未处理的服务端请求: {} {:?}",
                        request.id, request.request
                    );
                }
                ServerJsonRpcMessage::Notification(notification) => {
                    debug!("忽略 MCP SSE 通知: {:?}", notification.notification);
                }
            }
        }
    }

    fn next_request_id(&mut self) -> RequestId {
        let id = self.next_id;
        self.next_id += 1;
        RequestId::String(id.to_string().into())
    }
}

impl Drop for SseClientSession {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn wait_for_endpoint(
    receiver: oneshot::Receiver<Result<url::Url>>,
    timeout: Option<Duration>,
) -> Result<url::Url> {
    let result = match timeout {
        Some(timeout) => tokio::time::timeout(timeout, receiver)
            .await
            .map_err(|_| anyhow!("MCP SSE 获取 endpoint 超时"))??,
        None => receiver.await?,
    };
    result
}

fn resolve_sse_endpoint(base_url: &url::Url, raw: &str) -> Result<url::Url> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("MCP SSE endpoint 为空"));
    }
    let endpoint = url::Url::parse(trimmed).or_else(|_| base_url.join(trimmed))?;
    if endpoint.scheme() != base_url.scheme() || endpoint.host_str() != base_url.host_str() {
        return Err(anyhow!(
            "MCP SSE endpoint 域名不匹配: {}",
            endpoint.as_str()
        ));
    }
    Ok(endpoint)
}

async fn fetch_tools_sse(config: &Config, server: &McpServerConfig) -> Result<Vec<ToolSpec>> {
    let mut session = SseClientSession::connect(config, server).await?;
    session.initialize().await?;
    let tools = session.list_tools().await?;
    Ok(collect_tool_specs(server, tools.tools))
}

async fn call_tool_sse(
    config: &Config,
    server: &McpServerConfig,
    tool_name: &str,
    args: &Value,
) -> Result<Value> {
    let mut session = SseClientSession::connect(config, server).await?;
    session.initialize().await?;
    let result = session.call_tool(tool_name, args).await?;
    Ok(serialize_tool_result(result))
}

pub(crate) fn normalize_transport(transport: Option<&str>) -> String {
    let value = transport.unwrap_or("streamable-http").trim();
    if value.is_empty() {
        return "streamable-http".to_string();
    }
    if value.eq_ignore_ascii_case("http") {
        return "streamable-http".to_string();
    }
    value.to_lowercase()
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
                .map(|text| text.to_string())
                .unwrap_or_else(|_| String::from_utf8_lossy(value.as_bytes()).to_string());
            format!("{}={}", key.as_str().to_lowercase(), value_text)
        })
        .collect::<Vec<_>>();
    pairs.sort();
    let timeout = timeout_s.unwrap_or(0);
    format!("timeout={timeout};{}", pairs.join(";"))
}

fn build_mcp_client(headers: HeaderMap, timeout_s: Option<u64>) -> Result<reqwest::Client> {
    let key = build_mcp_client_key(&headers, timeout_s);
    let cache = mcp_client_cache();
    if let Some(client) = cache.lock().get(&key) {
        return Ok(client.clone());
    }
    let mut builder = reqwest::Client::builder().default_headers(headers);
    if let Some(timeout_s) = timeout_s {
        if timeout_s > 0 {
            builder = builder.timeout(Duration::from_secs(timeout_s));
        }
    }
    let client = builder.build()?;
    cache.lock().insert(key, client.clone());
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
