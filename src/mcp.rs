// MCP 服务与客户端：对齐 codex-main 的 rmcp SDK，用于流式 HTTP 与工具调用。
use crate::config::{Config, McpServerConfig};
use crate::i18n;
use crate::schemas::{ToolSpec, WunderRequest};
use crate::state::AppState;
use crate::tools::{builtin_aliases, resolve_tool_name};
use anyhow::{anyhow, Result};
use axum::Router;
use futures::StreamExt;
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
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, Instant};
use tracing::{debug, info, warn};

const MCP_SERVER_NAME: &str = "wunder";
const MCP_TOOL_NAME: &str = "run";
const MCP_USER_ID: &str = "wunder";
const MCP_INSTRUCTIONS: &str = "调用 wunder 智能体执行任务并返回最终回复。";
const MCP_RUN_DESCRIPTION: &str = "执行 wunder 智能体任务并返回最终回复。";

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

/// MCP 服务器实现：仅暴露 wunder@run 工具。
#[derive(Clone)]
struct WunderMcpServer {
    state: Arc<AppState>,
}

impl WunderMcpServer {
    fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    fn run_tool() -> Tool {
        let schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "task": { "type": "string" }
            },
            "required": ["task"]
        }))
        .unwrap_or_default();
        Tool::new(
            Cow::Borrowed(MCP_TOOL_NAME),
            Cow::Borrowed(MCP_RUN_DESCRIPTION),
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
        names.remove(&format!("{}@{}", MCP_SERVER_NAME, MCP_TOOL_NAME));
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
        let tools = vec![Self::run_tool()];
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
        if request.name.as_ref() != MCP_TOOL_NAME {
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
            .orchestrator
            .run(WunderRequest {
                user_id: MCP_USER_ID.to_string(),
                question: task,
                tool_names,
                stream: false,
                session_id: None,
                model_name: None,
                language: Some(i18n::get_default_language()),
                config_overrides: None,
                attachments: None,
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

#[derive(Clone, Default)]
struct NoopClientHandler;

impl ClientHandler for NoopClientHandler {}

/// 查询 MCP 工具列表，转换为 Wunder ToolSpec。
pub async fn fetch_tools(config: &Config, server: &McpServerConfig) -> Result<Vec<ToolSpec>> {
    let transport = normalize_transport(server.transport.as_deref());
    if transport == "sse" {
        return fetch_tools_sse(config, server).await;
    }
    let transport = build_transport(config, server)?;
    let service = serve_client(NoopClientHandler::default(), transport).await?;
    let tools = service.list_all_tools().await?;
    Ok(collect_tool_specs(server, tools))
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
        let timeout = if config.mcp.timeout_s > 0 {
            Some(Duration::from_secs(config.mcp.timeout_s))
        } else {
            None
        };
        let mut builder = reqwest::Client::builder().default_headers(headers);
        if let Some(timeout) = timeout {
            builder = builder.timeout(timeout);
        }
        let client = builder.build()?;
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
        Some(Duration::from_secs(config.mcp.timeout_s))
    } else {
        None
    };
    let mut builder = reqwest::Client::builder().default_headers(headers);
    if let Some(timeout) = timeout_s {
        builder = builder.timeout(timeout);
    }
    let client = builder.build()?;
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
