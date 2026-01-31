use crate::config::Config;
use crate::i18n;
use crate::orchestrator::OrchestratorError;
use crate::schemas::{StreamEvent, WunderRequest};
use crate::state::AppState;
use crate::storage::UserQuotaStatus;
use crate::tools::{builtin_aliases, builtin_tool_specs, resolve_tool_name};
use crate::user_store::UserStore;
use anyhow::Error;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use chrono::{Local, TimeZone};
use futures::StreamExt;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

const A2A_PROTOCOL_VERSION: &str = "1.0";
const JSONRPC_VERSION: &str = "2.0";

const JSONRPC_PARSE_ERROR: i64 = -32700;
const JSONRPC_INVALID_REQUEST: i64 = -32600;
const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;
const JSONRPC_INVALID_PARAMS: i64 = -32602;
const JSONRPC_INTERNAL_ERROR: i64 = -32603;

const A2A_TASK_NOT_FOUND: i64 = -32001;
const A2A_TASK_NOT_CANCELABLE: i64 = -32002;
const A2A_PUSH_NOT_SUPPORTED: i64 = -32003;
const A2A_CONTENT_TYPE_NOT_SUPPORTED: i64 = -32005;
const A2A_QUOTA_EXCEEDED: i64 = -32006;
const A2A_VERSION_NOT_SUPPORTED: i64 = -32009;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/.well-known/agent-card.json", get(agent_card))
        .route("/a2a/agentCard", get(agent_card))
        .route("/a2a/extendedAgentCard", get(agent_card_extended))
        .route("/a2a", post(a2a_entry))
}

async fn agent_card(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    Ok(Json(build_agent_card(&state, &headers, false).await))
}

async fn agent_card_extended(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    Ok(Json(build_agent_card(&state, &headers, true).await))
}

async fn build_agent_card(state: &AppState, headers: &HeaderMap, extended: bool) -> Value {
    let config = state.config_store.get().await;
    let base = resolve_request_base_url(headers, &config);
    let description = resolve_agent_description();

    let registry = state.skills.read().await;
    let mut skill_specs = registry.list_specs();
    drop(registry);
    skill_specs.sort_by(|a, b| a.name.cmp(&b.name));

    let skills = build_skill_specs(&skill_specs);
    let blocked_skill_names = skill_specs
        .iter()
        .map(|spec| spec.name.clone())
        .collect::<HashSet<_>>();
    let tooling = build_tooling_specs(&config, &blocked_skill_names);

    let mut card = json!({
        "protocolVersion": A2A_PROTOCOL_VERSION,
        "name": "Wunder",
        "description": description,
        "supportedInterfaces": [
            {"url": format!("{base}/a2a"), "protocolBinding": "JSONRPC"}
        ],
        "provider": {
            "organization": "Wunder",
            "url": base
        },
        "version": env!("CARGO_PKG_VERSION"),
        "capabilities": {
            "streaming": true,
            "pushNotifications": false,
            "stateTransitionHistory": false
        },
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain", "application/json"],
        "supportsExtendedAgentCard": true,
        "skills": skills,
        "tooling": tooling
    });

    if config.api_key().is_some() {
        if let Some(obj) = card.as_object_mut() {
            obj.insert(
                "securitySchemes".to_string(),
                json!({
                    "apiKey": {
                        "type": "apiKey",
                        "in": "header",
                        "name": "X-API-Key",
                        "description": "Wunder API Key"
                    }
                }),
            );
            obj.insert("security".to_string(), json!([{ "apiKey": [] }]));
        }
    }

    if extended {
        if let Some(obj) = card.as_object_mut() {
            obj.insert("documentationUrl".to_string(), json!(format!("{base}/")));
        }
    }

    card
}

fn resolve_agent_description() -> String {
    let language = i18n::get_language().to_lowercase();
    if language.starts_with("en") {
        return "Wunder agent router".to_string();
    }
    "Wunder 智能体路由器".to_string()
}

fn resolve_request_base_url(headers: &HeaderMap, config: &Config) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("http");

    let mut host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| {
            let host = if config.server.host == "0.0.0.0" {
                "127.0.0.1".to_string()
            } else {
                config.server.host.clone()
            };
            format!("{host}:{}", config.server.port)
        });
    if !host.contains(':') {
        if let Some(port) = headers
            .get("x-forwarded-port")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            host = format!("{host}:{port}");
        }
    }

    format!("{scheme}://{host}")
        .trim_end_matches('/')
        .to_string()
}
fn build_skill_specs(skill_specs: &[crate::skills::SkillSpec]) -> Vec<Value> {
    if skill_specs.is_empty() {
        return vec![default_skill_spec()];
    }
    skill_specs
        .iter()
        .map(|spec| {
            json!({
                "id": spec.name,
                "name": spec.name,
                "description": spec.description,
                "tags": [spec.name],
                "examples": [],
                "inputModes": ["text/plain"],
                "outputModes": ["text/plain"]
            })
        })
        .collect()
}

fn default_skill_spec() -> Value {
    let language = i18n::get_language().to_lowercase();
    if language.starts_with("en") {
        return json!({
            "id": "wunder-general",
            "name": "General Chat",
            "description": "General agent capability that supports tool calls and knowledge retrieval.",
            "tags": ["general", "tools"],
            "examples": ["Summarize the current workspace"],
            "inputModes": ["text/plain"],
            "outputModes": ["text/plain"]
        });
    }
    json!({
        "id": "wunder-general",
        "name": "通用对话",
        "description": "支持工具调用与知识检索的通用智能体能力。",
        "tags": ["general", "tools"],
        "examples": ["请帮我总结当前工作区内容"],
        "inputModes": ["text/plain"],
        "outputModes": ["text/plain"]
    })
}

fn build_tooling_specs(config: &Config, blocked_skill_names: &HashSet<String>) -> Value {
    json!({
        "builtin": build_enabled_builtin_tooling(config),
        "mcp": build_mcp_tooling(config),
        "a2a": build_a2a_tooling(config),
        "knowledge": build_knowledge_tooling(config, blocked_skill_names),
    })
}

fn build_enabled_builtin_tooling(config: &Config) -> Vec<Value> {
    let enabled = config
        .tools
        .builtin
        .enabled
        .iter()
        .map(|value| resolve_tool_name(value))
        .collect::<HashSet<_>>();

    let language = i18n::get_language().to_lowercase();
    let mut canonical_aliases: HashMap<String, Vec<String>> = HashMap::new();
    for (alias, canonical) in builtin_aliases() {
        canonical_aliases.entry(canonical).or_default().push(alias);
    }

    builtin_tool_specs()
        .into_iter()
        .filter(|spec| enabled.contains(&spec.name))
        .map(|spec| {
            let mut name = spec.name;
            if language.starts_with("en") {
                if let Some(aliases) = canonical_aliases.get(&name) {
                    if let Some(alias) = aliases.first() {
                        name = alias.clone();
                    }
                }
            }
            json!({
                "name": name,
                "description": spec.description
            })
        })
        .collect()
}

fn build_mcp_tooling(config: &Config) -> Vec<Value> {
    let mut items = Vec::new();
    for server in &config.mcp.servers {
        if !server.enabled {
            continue;
        }
        let allow = server.allow_tools.iter().cloned().collect::<HashSet<_>>();
        for tool in &server.tool_specs {
            if tool.name.is_empty() {
                continue;
            }
            if !allow.is_empty() && !allow.contains(&tool.name) {
                continue;
            }
            let full_name = format!("{}@{}", server.name, tool.name);
            items.push(json!({
                "name": full_name,
                "description": tool.description,
                "server": server.name,
                "tool": tool.name
            }));
        }
    }
    items
}

fn build_a2a_tooling(config: &Config) -> Vec<Value> {
    let mut items = Vec::new();
    for service in &config.a2a.services {
        if !service.enabled {
            continue;
        }
        let name = service.name.trim();
        if name.is_empty() {
            continue;
        }
        items.push(json!({
            "name": format!("a2a@{name}"),
            "description": service.description.clone().unwrap_or_default()
        }));
    }
    items
}

fn build_knowledge_tooling(config: &Config, blocked_skill_names: &HashSet<String>) -> Vec<Value> {
    let mut items = Vec::new();
    for base in &config.knowledge.bases {
        if !base.enabled {
            continue;
        }
        let name = base.name.trim();
        if name.is_empty() || blocked_skill_names.contains(name) {
            continue;
        }
        let description = if base.description.trim().is_empty() {
            i18n::t_with_params(
                "knowledge.tool.description",
                &HashMap::from([("name".to_string(), name.to_string())]),
            )
        } else {
            base.description.clone()
        };
        items.push(json!({
            "name": name,
            "description": description
        }));
    }
    items
}
#[derive(Debug)]
struct A2AError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl A2AError {
    fn new(code: i64, message: String, data: Option<Value>) -> Self {
        Self {
            code,
            message,
            data,
        }
    }

    fn invalid_params(detail: &str) -> Self {
        let message = if detail.is_empty() {
            i18n::t("error.param_required")
        } else {
            format!("{}: {detail}", i18n::t("error.param_required"))
        };
        let data = if detail.is_empty() {
            None
        } else {
            Some(json!({ "parameter": detail }))
        };
        Self::new(JSONRPC_INVALID_PARAMS, message, data)
    }

    fn task_not_found(task_id: &str) -> Self {
        Self::new(
            A2A_TASK_NOT_FOUND,
            i18n::t("error.task_not_found"),
            Some(json!({ "taskId": task_id })),
        )
    }

    fn task_not_cancelable(task_id: &str) -> Self {
        Self::new(
            A2A_TASK_NOT_CANCELABLE,
            "任务不可取消".to_string(),
            Some(json!({ "taskId": task_id })),
        )
    }

    fn content_type_not_supported() -> Self {
        Self::new(
            A2A_CONTENT_TYPE_NOT_SUPPORTED,
            "不支持的内容类型".to_string(),
            None,
        )
    }

    fn push_notification_not_supported() -> Self {
        Self::new(A2A_PUSH_NOT_SUPPORTED, "暂不支持推送通知".to_string(), None)
    }

    fn version_not_supported(version: &str) -> Self {
        Self::new(
            A2A_VERSION_NOT_SUPPORTED,
            "不支持的 A2A 协议版本".to_string(),
            Some(json!({ "version": version })),
        )
    }

    fn internal(detail: &str) -> Self {
        let message = i18n::t_with_params(
            "error.internal_error",
            &HashMap::from([("detail".to_string(), detail.to_string())]),
        );
        Self::new(
            JSONRPC_INTERNAL_ERROR,
            message,
            Some(json!({ "detail": detail })),
        )
    }

    fn quota_exceeded(status: &UserQuotaStatus) -> Self {
        let message = i18n::t("error.user_quota_exceeded");
        Self::new(
            A2A_QUOTA_EXCEEDED,
            message,
            Some(json!({
                "quota": {
                    "daily_quota": status.daily_quota,
                    "used": status.used,
                    "remaining": status.remaining,
                    "date": status.date,
                }
            })),
        )
    }
}

fn quota_status_from_payload(payload: &Value) -> Option<UserQuotaStatus> {
    let detail = payload.get("detail")?.as_object()?;
    let daily_quota = detail
        .get("daily_quota")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let used = detail.get("used").and_then(Value::as_i64).unwrap_or(0);
    let remaining = detail.get("remaining").and_then(Value::as_i64).unwrap_or(0);
    let date = detail
        .get("date")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(UserQuotaStatus {
        daily_quota,
        used,
        remaining,
        date,
        allowed: false,
    })
}

async fn a2a_entry(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return jsonrpc_error(
                Value::Null,
                JSONRPC_PARSE_ERROR,
                "Parse error".to_string(),
                None,
            )
        }
    };
    let Some(obj) = payload.as_object() else {
        return jsonrpc_error(
            Value::Null,
            JSONRPC_INVALID_REQUEST,
            "Invalid Request".to_string(),
            None,
        );
    };

    let request_id = obj.get("id").cloned().unwrap_or(Value::Null);
    if obj.get("jsonrpc").and_then(Value::as_str) != Some(JSONRPC_VERSION) {
        return jsonrpc_error(
            request_id,
            JSONRPC_INVALID_REQUEST,
            "Invalid Request".to_string(),
            None,
        );
    }
    let Some(method) = obj
        .get("method")
        .and_then(Value::as_str)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return jsonrpc_error(
            request_id,
            JSONRPC_INVALID_REQUEST,
            "Invalid Request".to_string(),
            None,
        );
    };

    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return jsonrpc_error(
            request_id,
            JSONRPC_INVALID_PARAMS,
            "Invalid params".to_string(),
            None,
        );
    }

    if let Err(err) = ensure_a2a_version(&headers) {
        return jsonrpc_error(request_id, err.code, err.message, err.data);
    }

    let service = A2aService::new(state.clone());
    match method {
        "SendMessage" => match service.send_message(&params).await {
            Ok(result) => jsonrpc_result(request_id, result),
            Err(err) => jsonrpc_error(request_id, err.code, err.message, err.data),
        },
        "SendStreamingMessage" => match service.send_streaming_message(&params).await {
            Ok(stream) => sse_response(stream),
            Err(err) => jsonrpc_error(request_id, err.code, err.message, err.data),
        },
        "SubscribeToTask" => match service.subscribe_to_task(&params).await {
            Ok(stream) => sse_response(stream),
            Err(err) => jsonrpc_error(request_id, err.code, err.message, err.data),
        },
        "GetTask" => match service.get_task(&params).await {
            Ok(result) => jsonrpc_result(request_id, result),
            Err(err) => jsonrpc_error(request_id, err.code, err.message, err.data),
        },
        "ListTasks" => match service.list_tasks(&params).await {
            Ok(result) => jsonrpc_result(request_id, result),
            Err(err) => jsonrpc_error(request_id, err.code, err.message, err.data),
        },
        "CancelTask" => match service.cancel_task(&params).await {
            Ok(result) => jsonrpc_result(request_id, result),
            Err(err) => jsonrpc_error(request_id, err.code, err.message, err.data),
        },
        "GetExtendedAgentCard" => {
            let result = build_agent_card(&state, &headers, true).await;
            jsonrpc_result(request_id, result)
        }
        "SetTaskPushNotificationConfig"
        | "GetTaskPushNotificationConfig"
        | "ListTaskPushNotificationConfig"
        | "DeleteTaskPushNotificationConfig" => {
            let err = A2AError::push_notification_not_supported();
            jsonrpc_error(request_id, err.code, err.message, err.data)
        }
        _ => jsonrpc_error(
            request_id,
            JSONRPC_METHOD_NOT_FOUND,
            "Method not found".to_string(),
            None,
        ),
    }
}

fn jsonrpc_result(id: Value, result: Value) -> Response {
    Json(json!({ "jsonrpc": JSONRPC_VERSION, "id": id, "result": result })).into_response()
}

fn jsonrpc_error(id: Value, code: i64, message: String, data: Option<Value>) -> Response {
    let mut error = json!({ "code": code, "message": message });
    if let Some(extra) = data {
        if let Value::Object(ref mut map) = error {
            map.insert("data".to_string(), extra);
        }
    }
    Json(json!({ "jsonrpc": JSONRPC_VERSION, "id": id, "error": error })).into_response()
}

fn ensure_a2a_version(headers: &HeaderMap) -> Result<(), A2AError> {
    let version = headers
        .get("a2a-version")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_string();
    if version.is_empty() {
        return Ok(());
    }
    let normalized = version.trim_start_matches(|ch| ch == 'v' || ch == 'V');
    if normalized != A2A_PROTOCOL_VERSION {
        return Err(A2AError::version_not_supported(&version));
    }
    Ok(())
}

fn sse_response(stream: ReceiverStream<Value>) -> Response {
    let mapped = stream.map(|payload| {
        let data = payload.to_string();
        Ok::<Event, Infallible>(Event::default().data(data))
    });
    Sse::new(mapped)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}
#[derive(Clone)]
struct A2aService {
    state: Arc<AppState>,
}

impl A2aService {
    fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    fn is_admin_user(&self, user_id: &str) -> bool {
        self.state
            .user_store
            .get_user_by_id(user_id)
            .ok()
            .flatten()
            .map(|user| UserStore::is_admin(&user))
            .unwrap_or(false)
    }

    fn map_orchestrator_error(&self, err: Error) -> A2AError {
        if let Some(orchestrator_err) = err.downcast_ref::<OrchestratorError>() {
            if orchestrator_err.code() == "USER_QUOTA_EXCEEDED" {
                let payload = orchestrator_err.to_payload();
                if let Some(status) = quota_status_from_payload(&payload) {
                    return A2AError::quota_exceeded(&status);
                }
                return A2AError::new(A2A_QUOTA_EXCEEDED, orchestrator_err.to_string(), None);
            }
            return A2AError::internal(&orchestrator_err.to_string());
        }
        A2AError::internal(&err.to_string())
    }

    async fn send_message(&self, params: &Value) -> Result<Value, A2AError> {
        let message = params
            .get("message")
            .or_else(|| params.get("request"))
            .ok_or_else(|| A2AError::invalid_params("message"))?;
        let message = message
            .as_object()
            .ok_or_else(|| A2AError::invalid_params("message"))?;

        let user_id = resolve_user_id(params);
        let (session_id, context_id) = resolve_session_ids(message)?;
        let question = extract_question(message)?;
        let configuration = params.get("configuration").and_then(Value::as_object);
        let blocking = configuration
            .and_then(|config| config.get("blocking"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let history_length =
            configuration.and_then(|config| parse_history_length(config.get("historyLength")));
        let tool_names = normalize_list(params.get("toolNames")).unwrap_or_default();
        let model_name = normalize_text(params.get("modelName"));

        let request = WunderRequest {
            user_id: user_id.clone(),
            question,
            tool_names,
            skip_tool_calls: false,
            stream: false,
            debug_payload: false,
            session_id: Some(session_id.clone()),
            agent_id: None,
            model_name: if model_name.is_empty() {
                None
            } else {
                Some(model_name)
            },
            language: Some(i18n::get_language()),
            config_overrides: None,
            agent_prompt: None,
            attachments: None,
            is_admin: self.is_admin_user(&user_id),
        };

        if blocking {
            let result = self
                .state
                .orchestrator
                .run(request)
                .await
                .map_err(|err| self.map_orchestrator_error(err))?;
            let usage_value = result
                .usage
                .as_ref()
                .and_then(|usage| serde_json::to_value(usage).ok());
            let task = self
                .build_task_from_result(
                    &result.session_id,
                    &user_id,
                    &result.answer,
                    usage_value,
                    history_length,
                    Some(&context_id),
                )
                .await?;
            return Ok(json!({ "task": task }));
        }

        let orchestrator = self.state.orchestrator.clone();
        tokio::spawn(async move {
            let _ = orchestrator.run(request).await;
        });

        let status = build_status(
            "working",
            Some(&i18n::t("monitor.summary.received")),
            Some(&context_id),
            Some(&session_id),
            None,
            None,
        );
        let task = build_task(
            &session_id,
            &context_id,
            status,
            Some(Vec::new()),
            None,
            Some(json!({ "queued": true })),
        );
        Ok(json!({ "task": task }))
    }

    async fn send_streaming_message(
        &self,
        params: &Value,
    ) -> Result<ReceiverStream<Value>, A2AError> {
        let message = params
            .get("message")
            .or_else(|| params.get("request"))
            .ok_or_else(|| A2AError::invalid_params("message"))?;
        let message = message
            .as_object()
            .ok_or_else(|| A2AError::invalid_params("message"))?;

        let user_id = resolve_user_id(params);
        let (session_id, context_id) = resolve_session_ids(message)?;
        let question = extract_question(message)?;
        let tool_names = normalize_list(params.get("toolNames")).unwrap_or_default();
        let model_name = normalize_text(params.get("modelName"));

        let request = WunderRequest {
            user_id: user_id.clone(),
            question,
            tool_names,
            skip_tool_calls: false,
            stream: true,
            debug_payload: false,
            session_id: Some(session_id.clone()),
            agent_id: None,
            model_name: if model_name.is_empty() {
                None
            } else {
                Some(model_name)
            },
            language: Some(i18n::get_language()),
            config_overrides: None,
            agent_prompt: None,
            attachments: None,
            is_admin: self.is_admin_user(&user_id),
        };

        let (tx, rx) = mpsc::channel(64);
        let state = self.state.clone();
        tokio::spawn(async move {
            let mut stream_state = A2aStreamState {
                session_id: session_id.clone(),
                context_id: context_id.clone(),
                final_sent: false,
            };
            let initial_status = build_status(
                "submitted",
                Some(&i18n::t("monitor.summary.received")),
                Some(&context_id),
                Some(&session_id),
                None,
                None,
            );
            let initial_task = build_task(
                &session_id,
                &context_id,
                initial_status,
                Some(Vec::new()),
                None,
                None,
            );
            if tx.send(json!({ "task": initial_task })).await.is_err() {
                return;
            }

            let stream = match state.orchestrator.stream(request).await {
                Ok(stream) => stream,
                Err(err) => {
                    let status = build_status(
                        "failed",
                        Some(&err.to_string()),
                        Some(&context_id),
                        Some(&session_id),
                        Some(json!({ "error": { "message": err.to_string() } })),
                        None,
                    );
                    let payload = build_task_status_update_event(
                        &session_id,
                        &context_id,
                        status,
                        true,
                        None,
                    );
                    let _ = tx.send(payload).await;
                    return;
                }
            };
            tokio::pin!(stream);
            while let Some(item) = stream.next().await {
                let event = match item {
                    Ok(event) => event,
                    Err(err) => match err {},
                };
                let mapped = map_wunder_event(&mut stream_state, &event);
                for (payload, final_flag) in mapped {
                    if tx.send(payload).await.is_err() {
                        return;
                    }
                    if final_flag {
                        return;
                    }
                }
                if stream_state.final_sent {
                    return;
                }
            }
        });

        Ok(ReceiverStream::new(rx))
    }

    async fn subscribe_to_task(&self, params: &Value) -> Result<ReceiverStream<Value>, A2AError> {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let session_id = parse_task_name(name);
        if session_id.is_empty() {
            return Err(A2AError::invalid_params("name"));
        }
        let record = self
            .state
            .monitor
            .get_record(&session_id)
            .ok_or_else(|| A2AError::task_not_found(&session_id))?;
        let status = build_status_from_record(&record, &session_id);
        let task = build_task(
            &session_id,
            &session_id,
            status.clone(),
            Some(Vec::new()),
            None,
            None,
        );

        let (tx, rx) = mpsc::channel(64);
        let monitor = self.state.monitor.clone();
        tokio::spawn(async move {
            if tx.send(json!({ "task": task })).await.is_err() {
                return;
            }
            if status_is_final(&status) {
                return;
            }
            let mut stream_state = A2aStreamState {
                session_id: session_id.clone(),
                context_id: session_id.clone(),
                final_sent: false,
            };
            let mut last_event_index = 0usize;
            loop {
                if let Some(detail) = monitor.get_detail(&session_id) {
                    if let Some(events) = detail.get("events").and_then(Value::as_array) {
                        if last_event_index < events.len() {
                            let slice = &events[last_event_index..];
                            last_event_index = events.len();
                            for item in slice {
                                let (payloads, final_flag) =
                                    map_monitor_event(&mut stream_state, item);
                                for payload in payloads {
                                    if tx.send(payload).await.is_err() {
                                        return;
                                    }
                                }
                                if final_flag {
                                    return;
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });

        Ok(ReceiverStream::new(rx))
    }
    async fn get_task(&self, params: &Value) -> Result<Value, A2AError> {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let session_id = parse_task_name(name);
        if session_id.is_empty() {
            return Err(A2AError::invalid_params("name"));
        }
        let history_length = parse_history_length(params.get("historyLength"));
        let record = self
            .state
            .monitor
            .get_record(&session_id)
            .ok_or_else(|| A2AError::task_not_found(&session_id))?;
        let user_id = record
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("a2a")
            .trim()
            .to_string();
        self.build_task_from_record(&record, &user_id, true, history_length)
            .await
    }

    async fn list_tasks(&self, params: &Value) -> Result<Value, A2AError> {
        let context_id = normalize_text(params.get("contextId"));
        let status_filter = normalize_text(params.get("status"));
        let page_size = normalize_page_size(params.get("pageSize"));
        let page_token = normalize_text(params.get("pageToken"));
        let history_length = parse_history_length(params.get("historyLength"));
        let include_artifacts = params
            .get("includeArtifacts")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let records = self.state.monitor.list_records();
        let mut filtered = Vec::new();
        for record in records {
            let session_id = record
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !context_id.is_empty() && session_id != context_id {
                continue;
            }
            if !status_filter.is_empty() {
                let mapped = map_task_state(record.get("status"));
                if mapped != status_filter {
                    continue;
                }
            }
            filtered.push(record);
        }
        filtered.sort_by(|a, b| {
            record_updated_time(b)
                .partial_cmp(&record_updated_time(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let offset = decode_page_token(&page_token);
        let total_size = filtered.len();
        let end = std::cmp::min(offset + page_size, total_size);
        let page_records = if offset < total_size {
            &filtered[offset..end]
        } else {
            &[]
        };
        let next_offset = offset + page_records.len();
        let next_token = if next_offset < total_size {
            encode_page_token(next_offset)
        } else {
            String::new()
        };

        let mut tasks = Vec::new();
        for record in page_records {
            let user_id = record
                .get("user_id")
                .and_then(Value::as_str)
                .unwrap_or("a2a")
                .trim()
                .to_string();
            let task = self
                .build_task_from_record(record, &user_id, include_artifacts, history_length)
                .await?;
            tasks.push(task);
        }

        Ok(json!({
            "tasks": tasks,
            "nextPageToken": next_token,
            "pageSize": page_size,
            "totalSize": total_size
        }))
    }

    async fn cancel_task(&self, params: &Value) -> Result<Value, A2AError> {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let session_id = parse_task_name(name);
        if session_id.is_empty() {
            return Err(A2AError::invalid_params("name"));
        }
        let record = self
            .state
            .monitor
            .get_record(&session_id)
            .ok_or_else(|| A2AError::task_not_found(&session_id))?;
        if !self.state.monitor.cancel(&session_id) {
            return Err(A2AError::task_not_cancelable(&session_id));
        }
        let updated = self.state.monitor.get_record(&session_id).unwrap_or(record);
        let user_id = updated
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("a2a")
            .trim()
            .to_string();
        self.build_task_from_record(&updated, &user_id, true, None)
            .await
    }

    async fn build_task_from_result(
        &self,
        session_id: &str,
        user_id: &str,
        answer: &str,
        usage: Option<Value>,
        history_length: Option<i64>,
        context_id: Option<&str>,
    ) -> Result<Value, A2AError> {
        let status = build_status(
            "completed",
            Some(&i18n::t("monitor.summary.finished")),
            context_id,
            Some(session_id),
            None,
            None,
        );
        let mut artifacts = Vec::new();
        if !answer.trim().is_empty() {
            artifacts.push(build_artifact(
                &format!("final-{}", Uuid::new_v4().simple()),
                Some("final-answer"),
                vec![build_text_part(answer)],
                None,
                None,
            ));
        }
        let metadata = usage.map(|value| json!({ "tokenUsage": value }));
        let history = self
            .load_history(user_id, session_id, history_length)
            .await?;
        Ok(build_task(
            session_id,
            context_id.unwrap_or(session_id),
            status,
            Some(artifacts),
            history,
            metadata,
        ))
    }

    async fn build_task_from_record(
        &self,
        record: &Value,
        user_id: &str,
        include_artifacts: bool,
        history_length: Option<i64>,
    ) -> Result<Value, A2AError> {
        let session_id = record
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if session_id.is_empty() {
            return Err(A2AError::internal("missing session id"));
        }
        let status = build_status_from_record(record, &session_id);
        let metadata = build_task_metadata(record);
        let history = self
            .load_history(user_id, &session_id, history_length)
            .await?;
        let artifacts = if include_artifacts {
            Some(
                self.load_artifacts(user_id, &session_id, history.as_ref())
                    .await?,
            )
        } else {
            None
        };
        Ok(build_task(
            &session_id,
            &session_id,
            status,
            artifacts,
            history,
            metadata,
        ))
    }

    async fn load_history(
        &self,
        user_id: &str,
        session_id: &str,
        history_length: Option<i64>,
    ) -> Result<Option<Vec<Value>>, A2AError> {
        let Some(limit) = history_length else {
            return Ok(None);
        };
        if limit <= 0 {
            return Ok(None);
        }
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        let session_id_for_query = session_id.clone();
        let workspace = self.state.workspace.clone();
        let records = tokio::task::spawn_blocking(move || {
            workspace.load_history(&user_id, &session_id_for_query, limit)
        })
        .await
        .map_err(|err| A2AError::internal(&err.to_string()))?
        .map_err(|err| A2AError::internal(&err.to_string()))?;
        let mut messages = Vec::new();
        for item in records {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("");
            if role != "user" && role != "assistant" {
                continue;
            }
            let content = item.get("content").and_then(Value::as_str).unwrap_or("");
            let role = if role == "user" { "user" } else { "agent" };
            messages.push(build_message(
                role,
                vec![build_text_part(content)],
                Some(session_id.as_str()),
                Some(session_id.as_str()),
                None,
            ));
        }
        Ok(Some(messages))
    }

    async fn load_artifacts(
        &self,
        user_id: &str,
        session_id: &str,
        history: Option<&Vec<Value>>,
    ) -> Result<Vec<Value>, A2AError> {
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        let workspace = self.state.workspace.clone();
        let logs = tokio::task::spawn_blocking(move || {
            workspace.load_artifact_logs(&user_id, &session_id, 50)
        })
        .await
        .map_err(|err| A2AError::internal(&err.to_string()))?
        .map_err(|err| A2AError::internal(&err.to_string()))?;
        let mut artifacts = Vec::new();
        for item in logs {
            let artifact_id = item
                .get("artifact_id")
                .and_then(|value| {
                    value
                        .as_str()
                        .map(|text| text.to_string())
                        .or_else(|| value.as_i64().map(|num| num.to_string()))
                })
                .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
            let artifact_id = format!("log-{artifact_id}");
            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
            artifacts.push(build_artifact(
                &artifact_id,
                Some(name),
                vec![build_data_part(item.clone())],
                None,
                None,
            ));
        }
        let final_answer = extract_final_answer(history);
        if !final_answer.is_empty() {
            artifacts.push(build_artifact(
                &format!("final-{}", Uuid::new_v4().simple()),
                Some("final-answer"),
                vec![build_text_part(&final_answer)],
                None,
                None,
            ));
        }
        Ok(artifacts)
    }
}
struct A2aStreamState {
    session_id: String,
    context_id: String,
    final_sent: bool,
}

fn map_wunder_event(state: &mut A2aStreamState, event: &StreamEvent) -> Vec<(Value, bool)> {
    let payload = event.data.as_object();
    let Some(payload) = payload else {
        return Vec::new();
    };
    let data = payload.get("data").cloned().unwrap_or_else(|| json!({}));
    let timestamp = payload.get("timestamp").and_then(Value::as_str);
    map_stream_payload(state, &event.event, &data, timestamp)
}

fn map_monitor_event(state: &mut A2aStreamState, item: &Value) -> (Vec<Value>, bool) {
    let event_type = item.get("type").and_then(Value::as_str).unwrap_or("");
    let data = item.get("data").cloned().unwrap_or_else(|| json!({}));
    let timestamp = item.get("timestamp").and_then(Value::as_str);
    let mapped = map_stream_payload(state, event_type, &data, timestamp);
    let final_flag = mapped.iter().any(|(_, final_flag)| *final_flag);
    let payloads = mapped.into_iter().map(|(payload, _)| payload).collect();
    (payloads, final_flag)
}

fn map_stream_payload(
    state: &mut A2aStreamState,
    event_type: &str,
    data: &Value,
    timestamp: Option<&str>,
) -> Vec<(Value, bool)> {
    if event_type.is_empty() {
        return Vec::new();
    }

    if matches!(event_type, "progress" | "received" | "round_start") {
        let summary = data.get("summary").and_then(Value::as_str).unwrap_or("");
        let message_text = if summary.trim().is_empty() {
            i18n::t("monitor.summary.received")
        } else {
            summary.to_string()
        };
        let metadata = data
            .get("stage")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|stage| json!({ "stage": stage }));
        let status = build_status(
            "working",
            Some(&message_text),
            Some(&state.context_id),
            Some(&state.session_id),
            None,
            timestamp,
        );
        let payload = build_task_status_update_event(
            &state.session_id,
            &state.context_id,
            status,
            false,
            metadata,
        );
        return vec![(payload, false)];
    }

    if event_type == "a2ui" {
        let artifact_payload = json!({
            "uid": data.get("uid"),
            "messages": data.get("messages"),
            "content": data.get("content"),
        });
        let artifact = build_artifact(
            &format!("a2ui-{}", Uuid::new_v4().simple()),
            Some("a2ui"),
            vec![build_data_part(artifact_payload)],
            None,
            None,
        );
        let payload = build_task_artifact_update_event(
            &state.session_id,
            &state.context_id,
            artifact,
            false,
            true,
            None,
        );
        return vec![(payload, false)];
    }

    if event_type == "final" {
        let answer = data.get("answer").and_then(Value::as_str).unwrap_or("");
        let usage = data.get("usage").cloned();
        let mut events = Vec::new();
        if !answer.trim().is_empty() {
            let artifact = build_artifact(
                &format!("final-{}", Uuid::new_v4().simple()),
                Some("final-answer"),
                vec![build_text_part(answer)],
                None,
                None,
            );
            events.push((
                build_task_artifact_update_event(
                    &state.session_id,
                    &state.context_id,
                    artifact,
                    false,
                    true,
                    None,
                ),
                false,
            ));
        }
        let status = build_status(
            "completed",
            Some(&i18n::t("monitor.summary.finished")),
            Some(&state.context_id),
            Some(&state.session_id),
            None,
            timestamp,
        );
        let metadata = usage.map(|value| json!({ "tokenUsage": value }));
        events.push((
            build_task_status_update_event(
                &state.session_id,
                &state.context_id,
                status,
                true,
                metadata,
            ),
            true,
        ));
        state.final_sent = true;
        return events;
    }

    if event_type == "error" {
        let message_text = data
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(&i18n::t("monitor.summary.exception"))
            .to_string();
        let error_code = data
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_uppercase();
        let mapped_state = if error_code == "CANCELLED" {
            "cancelled"
        } else {
            "failed"
        };
        let metadata = if data.is_object() {
            Some(json!({ "error": data }))
        } else {
            None
        };
        let status = build_status(
            mapped_state,
            Some(&message_text),
            Some(&state.context_id),
            Some(&state.session_id),
            None,
            timestamp,
        );
        let payload = build_task_status_update_event(
            &state.session_id,
            &state.context_id,
            status,
            true,
            metadata,
        );
        state.final_sent = true;
        return vec![(payload, true)];
    }

    if event_type == "cancel" {
        let message_text = data
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or(&i18n::t("monitor.summary.cancel_requested"))
            .to_string();
        let status = build_status(
            "working",
            Some(&message_text),
            Some(&state.context_id),
            Some(&state.session_id),
            None,
            timestamp,
        );
        let payload = build_task_status_update_event(
            &state.session_id,
            &state.context_id,
            status,
            false,
            Some(json!({ "cancelRequested": true })),
        );
        return vec![(payload, false)];
    }

    if event_type == "cancelled" {
        let message_text = data
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or(&i18n::t("monitor.summary.cancelled"))
            .to_string();
        let status = build_status(
            "cancelled",
            Some(&message_text),
            Some(&state.context_id),
            Some(&state.session_id),
            None,
            timestamp,
        );
        let payload = build_task_status_update_event(
            &state.session_id,
            &state.context_id,
            status,
            true,
            None,
        );
        state.final_sent = true;
        return vec![(payload, true)];
    }

    if event_type == "finished" {
        let message_text = data
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or(&i18n::t("monitor.summary.finished"))
            .to_string();
        let status = build_status(
            "completed",
            Some(&message_text),
            Some(&state.context_id),
            Some(&state.session_id),
            None,
            timestamp,
        );
        let payload = build_task_status_update_event(
            &state.session_id,
            &state.context_id,
            status,
            true,
            None,
        );
        state.final_sent = true;
        return vec![(payload, true)];
    }

    Vec::new()
}

fn build_text_part(text: &str) -> Value {
    json!({ "text": text })
}

fn build_data_part(data: Value) -> Value {
    json!({ "data": data })
}

fn build_message(
    role: &str,
    parts: Vec<Value>,
    context_id: Option<&str>,
    task_id: Option<&str>,
    metadata: Option<Value>,
) -> Value {
    let mut payload = Map::new();
    payload.insert(
        "messageId".to_string(),
        Value::String(Uuid::new_v4().simple().to_string()),
    );
    payload.insert("role".to_string(), Value::String(role.to_string()));
    payload.insert("parts".to_string(), Value::Array(parts));
    if let Some(context_id) = context_id {
        payload.insert(
            "contextId".to_string(),
            Value::String(context_id.to_string()),
        );
    }
    if let Some(task_id) = task_id {
        payload.insert("taskId".to_string(), Value::String(task_id.to_string()));
    }
    if let Some(metadata) = metadata {
        payload.insert("metadata".to_string(), metadata);
    }
    Value::Object(payload)
}

fn build_status(
    state: &str,
    message_text: Option<&str>,
    context_id: Option<&str>,
    task_id: Option<&str>,
    metadata: Option<Value>,
    timestamp: Option<&str>,
) -> Value {
    let mut payload = Map::new();
    payload.insert("state".to_string(), Value::String(state.to_string()));
    if let Some(message_text) = message_text {
        payload.insert(
            "message".to_string(),
            build_message(
                "agent",
                vec![build_text_part(message_text)],
                context_id,
                task_id,
                None,
            ),
        );
    }
    if let Some(metadata) = metadata {
        payload.insert("metadata".to_string(), metadata);
    }
    let ts = timestamp
        .map(|value| value.to_string())
        .unwrap_or_else(local_now);
    payload.insert("timestamp".to_string(), Value::String(ts));
    Value::Object(payload)
}

fn build_artifact(
    artifact_id: &str,
    name: Option<&str>,
    parts: Vec<Value>,
    description: Option<&str>,
    metadata: Option<Value>,
) -> Value {
    let mut payload = Map::new();
    payload.insert(
        "artifactId".to_string(),
        Value::String(artifact_id.to_string()),
    );
    payload.insert("parts".to_string(), Value::Array(parts));
    if let Some(name) = name {
        if !name.trim().is_empty() {
            payload.insert("name".to_string(), Value::String(name.to_string()));
        }
    }
    if let Some(description) = description {
        if !description.trim().is_empty() {
            payload.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
    }
    if let Some(metadata) = metadata {
        payload.insert("metadata".to_string(), metadata);
    }
    Value::Object(payload)
}

fn build_task(
    task_id: &str,
    context_id: &str,
    status: Value,
    artifacts: Option<Vec<Value>>,
    history: Option<Vec<Value>>,
    metadata: Option<Value>,
) -> Value {
    let mut payload = Map::new();
    payload.insert("id".to_string(), Value::String(task_id.to_string()));
    payload.insert(
        "contextId".to_string(),
        Value::String(context_id.to_string()),
    );
    payload.insert("status".to_string(), status);
    if let Some(artifacts) = artifacts {
        payload.insert("artifacts".to_string(), Value::Array(artifacts));
    }
    if let Some(history) = history {
        payload.insert("history".to_string(), Value::Array(history));
    }
    if let Some(metadata) = metadata {
        payload.insert("metadata".to_string(), metadata);
    }
    Value::Object(payload)
}

fn build_task_status_update_event(
    task_id: &str,
    context_id: &str,
    status: Value,
    final_flag: bool,
    metadata: Option<Value>,
) -> Value {
    let mut payload = Map::new();
    payload.insert("taskId".to_string(), Value::String(task_id.to_string()));
    payload.insert(
        "contextId".to_string(),
        Value::String(context_id.to_string()),
    );
    payload.insert("status".to_string(), status);
    payload.insert("final".to_string(), Value::Bool(final_flag));
    if let Some(metadata) = metadata {
        payload.insert("metadata".to_string(), metadata);
    }
    json!({ "statusUpdate": payload })
}

fn build_task_artifact_update_event(
    task_id: &str,
    context_id: &str,
    artifact: Value,
    append: bool,
    last_chunk: bool,
    metadata: Option<Value>,
) -> Value {
    let mut payload = Map::new();
    payload.insert("taskId".to_string(), Value::String(task_id.to_string()));
    payload.insert(
        "contextId".to_string(),
        Value::String(context_id.to_string()),
    );
    payload.insert("artifact".to_string(), artifact);
    payload.insert("append".to_string(), Value::Bool(append));
    payload.insert("lastChunk".to_string(), Value::Bool(last_chunk));
    if let Some(metadata) = metadata {
        payload.insert("metadata".to_string(), metadata);
    }
    json!({ "artifactUpdate": payload })
}

fn build_status_from_record(record: &Value, session_id: &str) -> Value {
    let summary = record
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let summary = if summary.is_empty() {
        record
            .get("stage")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
    } else {
        summary
    };
    let state = map_task_state(record.get("status"));
    let raw_ts = record.get("updated_time");
    let timestamp = if let Some(Value::String(value)) = raw_ts {
        if value.trim().is_empty() {
            None
        } else {
            Some(value.trim().to_string())
        }
    } else {
        raw_ts
            .and_then(Value::as_f64)
            .and_then(|value| format_timestamp(value))
    };
    let timestamp = timestamp.unwrap_or_else(local_now);
    let message_text = if summary.trim().is_empty() {
        i18n::t("monitor.summary.received")
    } else {
        summary.to_string()
    };
    build_status(
        &state,
        Some(&message_text),
        Some(session_id),
        Some(session_id),
        None,
        Some(&timestamp),
    )
}

fn build_task_metadata(record: &Value) -> Option<Value> {
    let mut map = Map::new();
    if let Some(stage) = record.get("stage").and_then(Value::as_str) {
        if !stage.trim().is_empty() {
            map.insert("stage".to_string(), Value::String(stage.to_string()));
        }
    }
    let context_tokens = record
        .get("context_tokens")
        .or_else(|| record.get("token_usage"));
    if let Some(value) = context_tokens {
        if !value.is_null() {
            map.insert("contextTokens".to_string(), value.clone());
        }
    }
    let context_tokens_peak = record.get("context_tokens_peak").or(context_tokens);
    if let Some(value) = context_tokens_peak {
        if !value.is_null() {
            map.insert("contextTokensPeak".to_string(), value.clone());
        }
    }
    if record.get("cancel_requested").and_then(Value::as_bool) == Some(true) {
        map.insert("cancelRequested".to_string(), Value::Bool(true));
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn status_is_final(status: &Value) -> bool {
    status
        .get("state")
        .and_then(Value::as_str)
        .map(|state| matches!(state, "completed" | "failed" | "cancelled" | "rejected"))
        .unwrap_or(false)
}

fn extract_final_answer(history: Option<&Vec<Value>>) -> String {
    let Some(history) = history else {
        return String::new();
    };
    for item in history.iter().rev() {
        if item.get("role").and_then(Value::as_str) != Some("agent") {
            continue;
        }
        if let Some(parts) = item.get("parts").and_then(Value::as_array) {
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        return trimmed.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

fn format_timestamp(value: f64) -> Option<String> {
    if value <= 0.0 {
        return None;
    }
    let secs = value.floor() as i64;
    let nanos = ((value - secs as f64) * 1_000_000_000.0) as u32;
    Local
        .timestamp_opt(secs, nanos)
        .single()
        .map(|dt| dt.to_rfc3339())
}

fn local_now() -> String {
    Local::now().to_rfc3339()
}
fn resolve_user_id(params: &Value) -> String {
    for key in ["userId", "user_id", "tenant"] {
        if let Some(value) = params.get(key).and_then(Value::as_str) {
            let cleaned = value.trim();
            if !cleaned.is_empty() {
                return cleaned.to_string();
            }
        }
    }
    "a2a".to_string()
}

fn resolve_session_ids(message: &Map<String, Value>) -> Result<(String, String), A2AError> {
    let task_id = message
        .get("taskId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let context_id = message
        .get("contextId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !task_id.is_empty() && !context_id.is_empty() && task_id != context_id {
        return Err(A2AError::invalid_params("contextId"));
    }
    let task_id = if task_id.is_empty() {
        if context_id.is_empty() {
            Uuid::new_v4().simple().to_string()
        } else {
            context_id.clone()
        }
    } else {
        task_id
    };
    let context_id = if context_id.is_empty() {
        task_id.clone()
    } else {
        context_id
    };
    Ok((task_id, context_id))
}

fn extract_question(message: &Map<String, Value>) -> Result<String, A2AError> {
    let parts = message
        .get("parts")
        .and_then(Value::as_array)
        .ok_or_else(|| A2AError::invalid_params("parts"))?;
    if parts.is_empty() {
        return Err(A2AError::invalid_params("parts"));
    }
    let mut texts = Vec::new();
    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            texts.push(text.to_string());
        } else {
            return Err(A2AError::content_type_not_supported());
        }
    }
    let question = texts.join("\n").trim().to_string();
    if question.is_empty() {
        return Err(A2AError::invalid_params("text"));
    }
    Ok(question)
}

fn normalize_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Number(num)) => num.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn normalize_list(value: Option<&Value>) -> Option<Vec<String>> {
    let Some(Value::Array(items)) = value else {
        return None;
    };
    let mut output = Vec::new();
    for item in items {
        let text = match item {
            Value::String(text) => text.trim().to_string(),
            Value::Number(num) => num.to_string(),
            Value::Bool(value) => value.to_string(),
            _ => String::new(),
        };
        if !text.trim().is_empty() {
            output.push(text);
        }
    }
    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}

fn parse_history_length(value: Option<&Value>) -> Option<i64> {
    let value = match value {
        Some(Value::Number(num)) => num.as_i64().or_else(|| num.as_u64().map(|v| v as i64)),
        Some(Value::String(text)) => text.trim().parse::<i64>().ok(),
        _ => None,
    }?;
    if value <= 0 {
        None
    } else {
        Some(value)
    }
}

fn normalize_page_size(value: Option<&Value>) -> usize {
    let size = match value {
        Some(Value::Number(num)) => num.as_i64().unwrap_or(50),
        Some(Value::String(text)) => text.trim().parse::<i64>().unwrap_or(50),
        _ => 50,
    };
    let size = size.max(1) as usize;
    size.min(100)
}

fn encode_page_token(offset: usize) -> String {
    URL_SAFE.encode(offset.to_string().as_bytes())
}

fn decode_page_token(token: &str) -> usize {
    if token.trim().is_empty() {
        return 0;
    }
    let Ok(raw) = URL_SAFE.decode(token.as_bytes()) else {
        return 0;
    };
    let Ok(text) = String::from_utf8(raw) else {
        return 0;
    };
    text.trim().parse::<usize>().unwrap_or(0)
}

fn parse_task_name(name: &str) -> String {
    let cleaned = name.trim();
    if cleaned.is_empty() {
        return String::new();
    }
    if let Some(rest) = cleaned.strip_prefix("tasks/") {
        return rest.trim().to_string();
    }
    cleaned.to_string()
}

fn map_task_state(status: Option<&Value>) -> String {
    let value = status
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if matches!(value.as_str(), "finished" | "final") {
        return "completed".to_string();
    }
    if matches!(value.as_str(), "error" | "failed") {
        return "failed".to_string();
    }
    if matches!(value.as_str(), "cancelled" | "canceled") {
        return "cancelled".to_string();
    }
    if value == "rejected" {
        return "rejected".to_string();
    }
    if value == "input_required" {
        return "input-required".to_string();
    }
    "working".to_string()
}

fn record_updated_time(record: &Value) -> f64 {
    if let Some(value) = record.get("updated_time") {
        if let Some(number) = value.as_f64() {
            return number;
        }
        if let Some(text) = value.as_str() {
            return text.trim().parse::<f64>().unwrap_or(0.0);
        }
    }
    0.0
}
