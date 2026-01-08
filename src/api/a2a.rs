// A2A 接口：JSON-RPC 入口与 AgentCard 发现。
use crate::a2a_store::A2aTask;
use crate::config::Config;
use crate::i18n;
use crate::schemas::WunderRequest;
use crate::state::AppState;
use crate::tools::{builtin_aliases, builtin_tool_specs, resolve_tool_name};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio_stream::StreamExt;
use uuid::Uuid;

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
        "protocolVersion": "1.0",
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
            obj.insert(
                "documentationUrl".to_string(),
                json!(format!("{base}/wunder/web")),
            );
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

async fn a2a_entry(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<JsonRpcRequest>,
) -> Result<Response, Response> {
    if let Some(version) = payload.jsonrpc.as_deref() {
        if version != "2.0" {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "jsonrpc 版本不支持".to_string(),
            ));
        }
    }
    match payload.method.as_str() {
        "SendMessage" => {
            let params = payload.params.unwrap_or(Value::Null);
            let user_id = params
                .get("userId")
                .and_then(Value::as_str)
                .unwrap_or("a2a-user");
            let message = extract_message_text(&params);
            let answer = run_wunder(&state, user_id, &message).await;
            let task_id = Uuid::new_v4().to_string();
            let endpoint = build_local_a2a_endpoint(&state.config_store.get().await);
            let task = build_task(&task_id, user_id, &answer);
            state.a2a_store.insert(A2aTask {
                id: task_id.clone(),
                user_id: user_id.to_string(),
                status: "completed".to_string(),
                context_id: Some(task_id.clone()),
                endpoint: Some(endpoint),
                service_name: Some("wunder".to_string()),
                method: Some("SendMessage".to_string()),
                created_time: Utc::now(),
                updated_time: Utc::now(),
                answer: answer.clone(),
            });
            Ok(Json(json!({
                "jsonrpc": "2.0",
                "id": payload.id,
                "result": { "task": task }
            }))
            .into_response())
        }
        "SendStreamingMessage" => {
            let params = payload.params.unwrap_or(Value::Null);
            let user_id = params
                .get("userId")
                .and_then(Value::as_str)
                .unwrap_or("a2a-user");
            let message = extract_message_text(&params);
            let answer = run_wunder(&state, user_id, &message).await;
            let task_id = Uuid::new_v4().to_string();
            let endpoint = build_local_a2a_endpoint(&state.config_store.get().await);
            state.a2a_store.insert(A2aTask {
                id: task_id.clone(),
                user_id: user_id.to_string(),
                status: "completed".to_string(),
                context_id: Some(task_id.clone()),
                endpoint: Some(endpoint),
                service_name: Some("wunder".to_string()),
                method: Some("SendStreamingMessage".to_string()),
                created_time: Utc::now(),
                updated_time: Utc::now(),
                answer: answer.clone(),
            });
            let events = build_stream_events(&task_id, &answer);
            let stream = tokio_stream::iter(events)
                .map(|event| Ok::<Event, std::convert::Infallible>(event));
            let sse = Sse::new(stream)
                .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)));
            Ok(sse.into_response())
        }
        "GetTask" => {
            let params = payload.params.unwrap_or(Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let task_id = name.trim_start_matches("tasks/");
            let task = state.a2a_store.get(task_id);
            Ok(Json(json!({
                "jsonrpc": "2.0",
                "id": payload.id,
                "result": task
            }))
            .into_response())
        }
        "ListTasks" => {
            let tasks = state.a2a_store.list();
            Ok(Json(json!({
                "jsonrpc": "2.0",
                "id": payload.id,
                "result": { "tasks": tasks }
            }))
            .into_response())
        }
        "CancelTask" => {
            let params = payload.params.unwrap_or(Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let task_id = name.trim_start_matches("tasks/");
            state.a2a_store.cancel(task_id);
            Ok(Json(json!({
                "jsonrpc": "2.0",
                "id": payload.id,
                "result": { "ok": true }
            }))
            .into_response())
        }
        "GetExtendedAgentCard" => Ok(Json(json!({
            "jsonrpc": "2.0",
            "id": payload.id,
            "result": build_agent_card(&state, &headers, true).await
        }))
        .into_response()),
        _ => Err(error_response(
            StatusCode::BAD_REQUEST,
            "未知方法".to_string(),
        )),
    }
}

fn build_task(task_id: &str, user_id: &str, answer: &str) -> Value {
    json!({
        "id": task_id,
        "name": format!("tasks/{task_id}"),
        "status": { "state": "completed", "final": true },
        "artifacts": [
            {
                "name": "final",
                "parts": [
                    { "text": answer }
                ]
            }
        ],
        "userId": user_id
    })
}

fn build_stream_events(task_id: &str, answer: &str) -> Vec<Event> {
    let task_event = json!({ "task": build_task(task_id, "", answer) });
    let status_running =
        json!({ "statusUpdate": { "status": { "state": "running" }, "final": false } });
    let artifact = json!({
        "artifactUpdate": {
            "artifact": { "name": "final", "parts": [ { "text": answer } ] }
        }
    });
    let status_done =
        json!({ "statusUpdate": { "status": { "state": "completed" }, "final": true } });
    vec![
        Event::default().data(task_event.to_string()),
        Event::default().data(status_running.to_string()),
        Event::default().data(artifact.to_string()),
        Event::default().data(status_done.to_string()),
    ]
}

async fn run_wunder(state: &AppState, user_id: &str, message: &str) -> String {
    let request = WunderRequest {
        user_id: user_id.to_string(),
        question: message.to_string(),
        tool_names: Vec::new(),
        stream: false,
        session_id: None,
        model_name: None,
        language: Some(i18n::get_language()),
        config_overrides: None,
        attachments: None,
    };
    match state.orchestrator.run(request).await {
        Ok(response) => response.answer,
        Err(_) => "测试回复".to_string(),
    }
}

fn extract_message_text(params: &Value) -> String {
    if let Some(message) = params.get("message") {
        if let Some(parts) = message.get("parts").and_then(Value::as_array) {
            let mut texts = Vec::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    texts.push(text.to_string());
                }
            }
            if !texts.is_empty() {
                return texts.join("\n");
            }
        }
    }
    "".to_string()
}

fn build_local_a2a_endpoint(config: &Config) -> String {
    let host = if config.server.host == "0.0.0.0" {
        "127.0.0.1".to_string()
    } else {
        config.server.host.clone()
    };
    format!("http://{host}:{}/a2a", config.server.port)
}

fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "detail": { "message": message } }))).into_response()
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}
