// 核心 API：/wunder 入口、系统提示词、工具清单与 i18n 配置。
use crate::api::attachment_convert::{build_ok_conversion_payload, convert_multipart_list};
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::orchestrator::OrchestratorError;
use crate::schemas::{
    AvailableToolsResponse, I18nConfigResponse, SharedToolSpec, ToolSpec, WunderPromptRequest,
    WunderPromptResponse, WunderRequest,
};
use crate::services::agent_runtime::AgentSubmitOutcome;
use crate::skills::load_skills;
use crate::state::AppState;
use crate::tools::{a2a_service_schema, builtin_tool_specs};
use crate::user_store::UserStore;
use crate::user_tools::{UserMcpServer, UserToolStore, UserToolsPayload};
use anyhow::Error;
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tracing::error;

const MAX_ATTACHMENT_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder", post(wunder_entry))
        .route("/wunder/system_prompt", post(wunder_system_prompt))
        .route("/wunder/tools", get(wunder_tools))
        .route("/wunder/i18n", get(wunder_i18n))
        .route(
            "/wunder/attachments/convert",
            post(wunder_attachment_convert)
                .layer(DefaultBodyLimit::max(MAX_ATTACHMENT_UPLOAD_BYTES)),
        )
}

async fn wunder_entry(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut request): Json<WunderRequest>,
) -> Result<Response, Response> {
    let _ = request.config_overrides.as_ref();
    let _ = request.attachments.as_ref();
    let resolved = resolve_user(&state, &headers, Some(&request.user_id)).await?;
    request.is_admin = UserStore::is_admin(&resolved.user);
    if request
        .language
        .as_ref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        request.language = Some(i18n::get_language());
    }
    let wants_stream = request.stream;
    let outcome = state
        .agent_runtime
        .submit_user_request(request)
        .await
        .map_err(|err| {
            orchestrator_error_response(
                StatusCode::BAD_REQUEST,
                json!({"code": "INVALID_REQUEST", "message": err.to_string()}),
            )
        })?;
    match outcome {
        AgentSubmitOutcome::Queued(info) => {
            let payload = json!({
                "queued": true,
                "queue_id": info.task_id,
                "thread_id": info.thread_id,
                "session_id": info.session_id,
            });
            if wants_stream {
                let mapped = tokio_stream::iter(vec![Ok::<Event, std::convert::Infallible>(
                    Event::default().event("queued").data(payload.to_string()),
                )]);
                let sse = Sse::new(mapped)
                    .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)));
                Ok(sse.into_response())
            } else {
                Ok((StatusCode::ACCEPTED, Json(json!({ "data": payload }))).into_response())
            }
        }
        AgentSubmitOutcome::Run(request, lease) => {
            if request.stream {
                let stream = state
                    .orchestrator
                    .stream(request)
                    .await
                    .map_err(map_orchestrator_error)?;
                let lease_guard = lease;
                let mapped = stream.map(move |event| {
                    let _keep = &lease_guard;
                    match event {
                        Ok(event) => {
                            let mut builder = Event::default()
                                .event(event.event)
                                .data(event.data.to_string());
                            if let Some(id) = event.id {
                                builder = builder.id(id);
                            }
                            Ok::<Event, std::convert::Infallible>(builder)
                        }
                        Err(err) => {
                            error!("sse stream error: {err}");
                            let payload = json!({ "event": "error", "message": err.to_string() });
                            Ok::<Event, std::convert::Infallible>(
                                Event::default().event("error").data(payload.to_string()),
                            )
                        }
                    }
                });
                let sse = Sse::new(mapped)
                    .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)));
                Ok(sse.into_response())
            } else {
                let response = state
                    .orchestrator
                    .run(request)
                    .await
                    .map_err(map_orchestrator_error)?;
                Ok(Json(response).into_response())
            }
        }
    }
}

async fn wunder_system_prompt(
    State(state): State<Arc<AppState>>,
    Json(mut request): Json<WunderPromptRequest>,
) -> Result<Json<WunderPromptResponse>, Response> {
    let _ = &request.user_id;
    let _ = &request.session_id;
    let _ = &request.config_overrides;
    if request
        .language
        .as_ref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        request.language = Some(i18n::get_language());
    }
    let start = Utc::now();
    let config = state.config_store.get().await;
    let skills_snapshot = state.skills.read().await.clone();
    let user_tool_bindings =
        state
            .user_tool_manager
            .build_bindings(&config, &skills_snapshot, &request.user_id);
    let workspace_id = state.workspace.scoped_user_id(&request.user_id, None);
    let prompt = state
        .orchestrator
        .build_system_prompt(
            &config,
            &request.tool_names,
            &skills_snapshot,
            Some(&user_tool_bindings),
            &request.user_id,
            false,
            &workspace_id,
            request.config_overrides.as_ref(),
            request.agent_prompt.as_deref(),
        )
        .await;
    let elapsed = Utc::now() - start;
    Ok(Json(WunderPromptResponse {
        prompt,
        build_time_ms: elapsed.num_milliseconds() as f64,
    }))
}

async fn wunder_tools(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ToolsQuery>,
) -> Result<Json<AvailableToolsResponse>, Response> {
    let user_id = params.user_id.as_deref().unwrap_or("").trim();
    let config = state.config_store.get().await;
    let language = i18n::get_language().to_lowercase();

    let enabled_builtin: std::collections::HashSet<String> = config
        .tools
        .builtin
        .enabled
        .iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    let mut builtin_tools = Vec::new();
    let alias_map = crate::tools::builtin_aliases();
    let mut canonical_aliases: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for (alias, canonical) in alias_map {
        canonical_aliases.entry(canonical).or_default().push(alias);
    }
    for spec in builtin_tool_specs() {
        if !enabled_builtin.contains(&spec.name) {
            continue;
        }
        if language.starts_with("en") {
            if let Some(aliases) = canonical_aliases.get(&spec.name) {
                if let Some(alias) = aliases.first() {
                    builtin_tools.push(ToolSpec {
                        name: alias.clone(),
                        description: spec.description.clone(),
                        input_schema: spec.input_schema.clone(),
                    });
                    continue;
                }
            }
        }
        builtin_tools.push(spec);
    }

    let mut mcp_tools = Vec::new();
    for server in &config.mcp.servers {
        if !server.enabled {
            continue;
        }
        let allow: std::collections::HashSet<String> = server.allow_tools.iter().cloned().collect();
        for tool in &server.tool_specs {
            if tool.name.is_empty() {
                continue;
            }
            if !allow.is_empty() && !allow.contains(&tool.name) {
                continue;
            }
            let input_schema =
                serde_json::to_value(&tool.input_schema).unwrap_or_else(|_| json!({}));
            let description = if tool.description.trim().is_empty() {
                server
                    .description
                    .clone()
                    .or_else(|| server.display_name.clone())
                    .unwrap_or_default()
            } else {
                tool.description.clone()
            };
            mcp_tools.push(ToolSpec {
                name: format!("{}@{}", server.name, tool.name),
                description,
                input_schema,
            });
        }
    }

    let a2a_tools = config
        .a2a
        .services
        .iter()
        .filter(|service| service.enabled)
        .map(|service| ToolSpec {
            name: format!("a2a@{}", service.name),
            description: service.description.clone().unwrap_or_default(),
            input_schema: a2a_service_schema(),
        })
        .collect::<Vec<_>>();

    let skills_snapshot = state.skills.read().await.clone();
    let skills = skills_snapshot
        .list_specs()
        .into_iter()
        .map(|spec| ToolSpec {
            name: spec.name,
            description: spec.description,
            input_schema: spec.input_schema,
        })
        .collect::<Vec<_>>();

    let mut blocked_names: std::collections::HashSet<String> = builtin_tools
        .iter()
        .map(|item| item.name.clone())
        .chain(mcp_tools.iter().map(|item| item.name.clone()))
        .chain(a2a_tools.iter().map(|item| item.name.clone()))
        .chain(skills.iter().map(|item| item.name.clone()))
        .collect();

    let knowledge_schema = json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": i18n::t("knowledge.tool.query.description")},
            "keywords": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": i18n::t("knowledge.tool.keywords.description")},
            "limit": {"type": "integer", "minimum": 1, "description": i18n::t("knowledge.tool.limit.description")}
        },
        "anyOf": [
            {"required": ["query"]},
            {"required": ["keywords"]}
        ]
    });
    let mut knowledge_tools = Vec::new();
    for base in &config.knowledge.bases {
        if !base.enabled {
            continue;
        }
        let name = base.name.trim();
        if name.is_empty() || blocked_names.contains(name) {
            continue;
        }
        let description = if base.description.trim().is_empty() {
            i18n::t_with_params(
                "knowledge.tool.description",
                &std::collections::HashMap::from([("name".to_string(), name.to_string())]),
            )
        } else {
            base.description.clone()
        };
        let spec = ToolSpec {
            name: name.to_string(),
            description,
            input_schema: knowledge_schema.clone(),
        };
        blocked_names.insert(name.to_string());
        knowledge_tools.push(spec);
    }

    let mut user_tools = Vec::new();
    let mut shared_tools = Vec::new();
    if !user_id.is_empty() {
        let payload = state.user_tool_store.load_user_tools(user_id);
        let mut used_names = blocked_names.clone();

        {
            let mut append_user_tool =
                |owner_id: &str, tool_name: &str, description: String, input_schema: Value| {
                    let alias = state.user_tool_store.build_alias_name(owner_id, tool_name);
                    if used_names.contains(&alias) {
                        return;
                    }
                    used_names.insert(alias.clone());
                    user_tools.push(ToolSpec {
                        name: alias,
                        description,
                        input_schema,
                    });
                };

            let owner_id = if payload.user_id.trim().is_empty() {
                user_id.to_string()
            } else {
                payload.user_id.clone()
            };
            collect_user_mcp_tools(&payload, &owner_id, false, &mut append_user_tool);
            collect_user_skill_tools(
                &payload,
                &owner_id,
                false,
                &mut append_user_tool,
                &config,
                state.user_tool_store.as_ref(),
            );
            collect_user_knowledge_tools(&payload, &owner_id, false, &mut append_user_tool);
        }

        for shared_payload in state.user_tool_store.list_shared_payloads(user_id) {
            let shared_owner = if shared_payload.user_id.trim().is_empty() {
                user_id.to_string()
            } else {
                shared_payload.user_id.clone()
            };
            let mut append_shared_tool =
                |owner_id: &str, tool_name: &str, description: String, input_schema: Value| {
                    let alias = state.user_tool_store.build_alias_name(owner_id, tool_name);
                    if used_names.contains(&alias) {
                        return;
                    }
                    used_names.insert(alias.clone());
                    shared_tools.push(SharedToolSpec {
                        name: alias,
                        description,
                        input_schema,
                        owner_id: owner_id.to_string(),
                    });
                };
            collect_user_mcp_tools(
                &shared_payload,
                &shared_owner,
                true,
                &mut append_shared_tool,
            );
            collect_user_skill_tools(
                &shared_payload,
                &shared_owner,
                true,
                &mut append_shared_tool,
                &config,
                state.user_tool_store.as_ref(),
            );
            collect_user_knowledge_tools(
                &shared_payload,
                &shared_owner,
                true,
                &mut append_shared_tool,
            );
        }
    }

    let response = AvailableToolsResponse {
        builtin_tools,
        mcp_tools,
        a2a_tools,
        skills,
        knowledge_tools,
        user_tools,
        shared_tools,
        shared_tools_selected: None,
    };
    Ok(Json(response))
}

fn collect_user_mcp_tools<F>(
    payload: &UserToolsPayload,
    owner_id: &str,
    shared_only: bool,
    append: &mut F,
) where
    F: FnMut(&str, &str, String, Value),
{
    for server in &payload.mcp_servers {
        let server_name = server.name.trim();
        if server_name.is_empty() || server_name.contains('@') {
            continue;
        }
        if !server.enabled {
            continue;
        }
        if server.tool_specs.is_empty() {
            continue;
        }
        let allow_tools: HashSet<String> = server
            .allow_tools
            .iter()
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .collect();
        let shared_tools: HashSet<String> = server
            .shared_tools
            .iter()
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .collect();
        let tool_pool: HashSet<String> = server
            .tool_specs
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        let mut enabled_names = if allow_tools.is_empty() {
            tool_pool
        } else {
            allow_tools
        };
        if shared_only {
            enabled_names = enabled_names
                .into_iter()
                .filter(|name| shared_tools.contains(name))
                .collect();
        }
        if enabled_names.is_empty() {
            continue;
        }
        for tool in &server.tool_specs {
            let tool_name = tool
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if tool_name.is_empty() || !enabled_names.contains(tool_name) {
                continue;
            }
            let description = resolve_user_mcp_description(server, tool);
            let schema = normalize_mcp_input_schema(tool);
            append(
                owner_id,
                &format!("{server_name}@{tool_name}"),
                description,
                schema,
            );
        }
    }
}

fn collect_user_skill_tools<F>(
    payload: &UserToolsPayload,
    owner_id: &str,
    shared_only: bool,
    append: &mut F,
    config: &crate::config::Config,
    store: &UserToolStore,
) where
    F: FnMut(&str, &str, String, Value),
{
    let skill_root = store.get_skill_root(owner_id);
    if !skill_root.exists() {
        return;
    }
    let enabled_set: HashSet<String> = payload
        .skills
        .enabled
        .iter()
        .cloned()
        .filter(|name| !name.trim().is_empty())
        .collect();
    let shared_set: HashSet<String> = payload
        .skills
        .shared
        .iter()
        .cloned()
        .filter(|name| !name.trim().is_empty())
        .collect();
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    for spec in registry.list_specs() {
        if shared_only {
            if !shared_set.contains(&spec.name) {
                continue;
            }
        } else if !enabled_set.contains(&spec.name) {
            continue;
        }
        append(
            owner_id,
            &spec.name,
            spec.description.clone(),
            spec.input_schema.clone(),
        );
    }
}

fn collect_user_knowledge_tools<F>(
    payload: &UserToolsPayload,
    owner_id: &str,
    shared_only: bool,
    append: &mut F,
) where
    F: FnMut(&str, &str, String, Value),
{
    let schema = build_knowledge_schema();
    for base in &payload.knowledge_bases {
        let name = base.name.trim();
        if name.is_empty() || !base.enabled {
            continue;
        }
        if shared_only && !base.shared {
            continue;
        }
        let description = if base.description.trim().is_empty() {
            i18n::t_with_params(
                "knowledge.tool.description",
                &std::collections::HashMap::from([("name".to_string(), name.to_string())]),
            )
        } else {
            base.description.clone()
        };
        append(owner_id, name, description, schema.clone());
    }
}

fn normalize_mcp_input_schema(tool: &Value) -> Value {
    if let Some(schema) = tool.get("inputSchema").or_else(|| tool.get("input_schema")) {
        if schema.is_object() {
            return schema.clone();
        }
    }
    json!({"type": "object", "properties": {}})
}

fn resolve_user_mcp_description(server: &UserMcpServer, tool: &Value) -> String {
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !description.is_empty() {
        return description.to_string();
    }
    if !server.description.trim().is_empty() {
        return server.description.clone();
    }
    if !server.display_name.trim().is_empty() {
        return server.display_name.clone();
    }
    "".to_string()
}

fn build_knowledge_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": i18n::t("knowledge.tool.query.description")},
            "keywords": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": i18n::t("knowledge.tool.keywords.description")},
            "limit": {"type": "integer", "minimum": 1, "description": i18n::t("knowledge.tool.limit.description")}
        },
        "anyOf": [
            {"required": ["query"]},
            {"required": ["keywords"]}
        ]
    })
}

async fn wunder_i18n(
    State(state): State<Arc<AppState>>,
) -> Result<Json<I18nConfigResponse>, Response> {
    let _ = state;
    let default_language = i18n::get_default_language();
    let supported_languages = i18n::get_supported_languages();
    let aliases = serde_json::to_value(i18n::get_language_aliases())
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    Ok(Json(I18nConfigResponse {
        default_language,
        supported_languages,
        aliases,
    }))
}

async fn wunder_attachment_convert(multipart: Multipart) -> Result<Json<Value>, Response> {
    let conversions = convert_multipart_list(multipart).await?;
    Ok(Json(build_ok_conversion_payload(conversions)))
}

fn map_orchestrator_error(err: Error) -> Response {
    if let Some(orchestrator_err) = err.downcast_ref::<OrchestratorError>() {
        let status = match orchestrator_err.code() {
            "USER_BUSY" | "USER_QUOTA_EXCEEDED" => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::BAD_REQUEST,
        };
        return orchestrator_error_response(status, orchestrator_err.to_payload());
    }
    orchestrator_error_response(
        StatusCode::BAD_REQUEST,
        json!({
            "code": "INTERNAL_ERROR",
            "message": err.to_string(),
        }),
    )
}

fn orchestrator_error_response(status: StatusCode, payload: Value) -> Response {
    let code = payload
        .get("code")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("request failed")
        .to_string();
    crate::api::errors::error_response_with_detail(
        status,
        code.as_deref(),
        message,
        None,
        Some(payload),
    )
}

#[derive(Debug, Deserialize)]
struct ToolsQuery {
    #[serde(default)]
    user_id: Option<String>,
}
