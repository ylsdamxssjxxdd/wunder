use crate::api::admin::error_response;
use crate::api::skill_fs;
use crate::attachment::sanitize_filename_stem;
use crate::config::{A2aServiceConfig, Config, LspConfig, McpServerConfig, ToolVisibilityRule};
use crate::core::blocking;
use crate::i18n;
use crate::lsp::{LspDiagnostic, LspManager};
use crate::path_utils::{
    is_within_root, normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::services::admin_skills::{
    build_admin_skill_scan_paths, collect_admin_reserved_skill_top_dirs,
    normalize_admin_skill_paths, resolve_admin_custom_skills_root,
    resolve_admin_uploaded_skills_root, resolve_builtin_skills_root,
};
use crate::services::skill_archive::{import_skill_archive, is_supported_skill_archive_filename};
use crate::skills::{load_skills, SkillSpec};
use crate::state::AppState;
use crate::tools::{builtin_aliases, builtin_tool_specs, resolve_tool_name};
use anyhow::anyhow;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::{HeaderValue as AxumHeaderValue, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, routing::put, Json, Router};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;
use url::Url;
use uuid::Uuid;
use walkdir::WalkDir;

const MAX_SKILL_UPLOAD_BYTES: usize = 200 * 1024 * 1024;
const MAX_LSP_DIAGNOSTICS: usize = 20;

#[derive(Clone, Copy, Eq, PartialEq)]
enum AdminSkillSourceKind {
    Builtin,
    Custom,
    External,
}

impl AdminSkillSourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Custom => "custom",
            Self::External => "external",
        }
    }

    fn builtin(self) -> bool {
        matches!(self, Self::Builtin)
    }

    fn editable(self) -> bool {
        true
    }
}

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/admin/mcp",
            get(admin_mcp_list).post(admin_mcp_update),
        )
        .route(
            "/wunder/admin/lsp",
            get(admin_lsp_get).post(admin_lsp_update),
        )
        .route("/wunder/admin/lsp/test", post(admin_lsp_test))
        .route("/wunder/admin/mcp/tools", post(admin_mcp_tools))
        .route("/wunder/admin/mcp/tools/call", post(admin_mcp_tool_call))
        .route(
            "/wunder/admin/a2a",
            get(admin_a2a_list).post(admin_a2a_update),
        )
        .route("/wunder/admin/a2a/card", post(admin_a2a_card))
        .route(
            "/wunder/admin/skills",
            get(admin_skills_list)
                .post(admin_skills_update)
                .delete(admin_skills_delete),
        )
        .route("/wunder/admin/skills/content", get(admin_skills_content))
        .route("/wunder/admin/skills/files", get(admin_skills_files))
        .route(
            "/wunder/admin/skills/file",
            get(admin_skills_file)
                .put(admin_skills_file_update)
                .delete(skill_fs::admin_skills_entry_delete),
        )
        .route(
            "/wunder/admin/skills/fs",
            get(skill_fs::admin_skills_fs_content),
        )
        .route(
            "/wunder/admin/skills/fs/search",
            get(skill_fs::admin_skills_fs_search),
        )
        .route(
            "/wunder/admin/skills/fs/file",
            put(skill_fs::admin_skills_fs_file_update),
        )
        .route(
            "/wunder/admin/skills/dir",
            post(skill_fs::admin_skills_dir_create),
        )
        .route(
            "/wunder/admin/skills/move",
            post(skill_fs::admin_skills_entry_move),
        )
        .route(
            "/wunder/admin/skills/copy",
            post(skill_fs::admin_skills_entry_copy),
        )
        .route(
            "/wunder/admin/skills/batch",
            post(skill_fs::admin_skills_batch),
        )
        .route(
            "/wunder/admin/skills/upload",
            post(admin_skills_upload).layer(DefaultBodyLimit::max(MAX_SKILL_UPLOAD_BYTES)),
        )
        .route("/wunder/admin/skills/export", get(admin_skills_export))
        .route(
            "/wunder/admin/skills/archive",
            get(skill_fs::admin_skills_archive),
        )
        .route(
            "/wunder/admin/skills/download",
            get(skill_fs::admin_skills_download),
        )
        .route(
            "/wunder/admin/skills/fs/upload",
            post(skill_fs::admin_skills_fs_upload)
                .layer(DefaultBodyLimit::max(skill_fs::MAX_SKILL_FS_UPLOAD_BYTES)),
        )
        .route(
            "/wunder/admin/tools",
            get(admin_tools_list).post(admin_tools_update),
        )
}

async fn admin_mcp_list(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    Ok(Json(json!({ "servers": config.mcp.servers })))
}

async fn admin_mcp_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<McpUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let previous = state.config_store.get().await;
    let updated = state
        .config_store
        .update(|config| {
            config.mcp.servers = payload.servers.clone();
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let previous_map: HashMap<String, bool> = previous
        .mcp
        .servers
        .iter()
        .map(|server| (server.name.clone(), server.enabled))
        .collect();
    let updated_map: HashMap<String, bool> = updated
        .mcp
        .servers
        .iter()
        .map(|server| (server.name.clone(), server.enabled))
        .collect();
    let mut added: Vec<String> = updated_map
        .keys()
        .filter(|name| !previous_map.contains_key(*name))
        .cloned()
        .collect();
    let mut removed: Vec<String> = previous_map
        .keys()
        .filter(|name| !updated_map.contains_key(*name))
        .cloned()
        .collect();
    let mut enabled_changed = Vec::new();
    let mut disabled_changed = Vec::new();
    for (name, enabled) in previous_map {
        if let Some(next_enabled) = updated_map.get(&name) {
            if enabled != *next_enabled {
                if *next_enabled {
                    enabled_changed.push(name);
                } else {
                    disabled_changed.push(name);
                }
            }
        }
    }
    added.sort();
    removed.sort();
    enabled_changed.sort();
    disabled_changed.sort();
    if !added.is_empty()
        || !removed.is_empty()
        || !enabled_changed.is_empty()
        || !disabled_changed.is_empty()
    {
        info!(
            "MCP 配置已更新: 新增 +{added_len}, 移除 -{removed_len}, 启用 +{enabled_len}, 停用 -{disabled_len}",
            added_len = added.len(),
            removed_len = removed.len(),
            enabled_len = enabled_changed.len(),
            disabled_len = disabled_changed.len(),
        );
        if !added.is_empty() {
            info!("MCP 服务已新增: {}", added.join(", "));
        }
        if !removed.is_empty() {
            info!("MCP 服务已移除: {}", removed.join(", "));
        }
        if !enabled_changed.is_empty() {
            info!("MCP 服务已启用: {}", enabled_changed.join(", "));
        }
        if !disabled_changed.is_empty() {
            info!("MCP 服务已停用: {}", disabled_changed.join(", "));
        }
    }
    Ok(Json(json!({ "servers": updated.mcp.servers })))
}

async fn admin_lsp_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let status = state.lsp_manager.status();
    Ok(Json(json!({ "lsp": config.lsp, "status": status })))
}

async fn admin_lsp_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LspUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let updated = state
        .config_store
        .update(|config| {
            config.lsp = payload.lsp.clone();
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state.lsp_manager.sync_with_config(&updated).await;
    Ok(Json(json!({
        "lsp": updated.lsp,
        "status": state.lsp_manager.status()
    })))
}

fn format_lsp_diagnostics(diagnostics: &[LspDiagnostic]) -> Option<Value> {
    if diagnostics.is_empty() {
        return None;
    }
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for diag in diagnostics {
        if diag.is_error() {
            errors.push(diag.pretty());
        } else {
            warnings.push(diag.pretty());
        }
    }
    let total = errors.len() + warnings.len();
    let mut items: Vec<String> = errors.iter().chain(warnings.iter()).cloned().collect();
    let truncated = items.len() > MAX_LSP_DIAGNOSTICS;
    if truncated {
        items.truncate(MAX_LSP_DIAGNOSTICS);
    }
    Some(json!({
        "total": total,
        "errors": errors.len(),
        "warnings": warnings.len(),
        "truncated": truncated,
        "items": items
    }))
}

fn lsp_diagnostics_summary(lsp_manager: &LspManager, user_id: &str, path: &Path) -> Option<Value> {
    let diagnostics_map = lsp_manager.diagnostics_for_user(user_id);
    if diagnostics_map.is_empty() {
        return None;
    }
    let target = normalize_target_path(path);
    let target_compare = normalize_path_for_compare(&target);
    for (candidate, diagnostics) in diagnostics_map {
        if normalize_path_for_compare(&candidate) == target_compare {
            return format_lsp_diagnostics(&diagnostics);
        }
    }
    None
}

async fn admin_lsp_test(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LspTestRequest>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    if !config.lsp.enabled {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "LSP 未启用".to_string(),
        ));
    }
    let user_id = payload.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "user_id 不能为空".to_string(),
        ));
    }
    let path = payload.path.trim();
    if path.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "path 不能为空".to_string(),
        ));
    }
    let target = state
        .workspace
        .resolve_path(user_id, path)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !target.exists() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            format!("文件不存在: {path}"),
        ));
    }
    let operation = payload.operation.trim().to_lowercase();
    if operation.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "operation 不能为空".to_string(),
        ));
    }
    let wait_for_diagnostics = operation == "diagnostics";
    state
        .lsp_manager
        .touch_file(&config, user_id, &target, wait_for_diagnostics)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if wait_for_diagnostics {
        let diagnostics = lsp_diagnostics_summary(&state.lsp_manager, user_id, &target);
        return Ok(Json(json!({
            "ok": true,
            "operation": payload.operation,
            "path": payload.path,
            "diagnostics": diagnostics
        })));
    }
    let uri = Url::from_file_path(&target)
        .map(|value| value.to_string())
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "文件路径无效".to_string()))?;
    let timeout_s = if config.lsp.timeout_s == 0 {
        30
    } else {
        config.lsp.timeout_s
    };
    let needs_position = matches!(
        operation.as_str(),
        "definition" | "references" | "hover" | "implementation" | "callhierarchy"
    );
    let position_value = if needs_position {
        let line = payload
            .line
            .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "缺少 line".to_string()))?;
        let character = payload
            .character
            .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "缺少 character".to_string()))?;
        if line == 0 || character == 0 {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "line/character 必须 >= 1".to_string(),
            ));
        }
        Some(json!({ "line": line - 1, "character": character - 1 }))
    } else {
        None
    };
    let query = if operation == "workspacesymbol" {
        Some(payload.query.clone().unwrap_or_default())
    } else {
        None
    };
    if operation == "workspacesymbol" && query.as_deref().unwrap_or("").trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "workspaceSymbol 缺少 query".to_string(),
        ));
    }
    let direction = payload
        .call_hierarchy_direction
        .clone()
        .unwrap_or_else(|| "incoming".to_string())
        .trim()
        .to_lowercase();
    let text_document = json!({ "uri": uri });
    let position_value = position_value.clone();
    let query_value = query.clone();
    let operation_key = operation.clone();
    let direction_key = direction.clone();
    let results = state
        .lsp_manager
        .run_on_clients(&config, user_id, &target, move |client| {
            let text_document = text_document.clone();
            let position = position_value.clone();
            let query = query_value.clone();
            let operation = operation_key.clone();
            let direction = direction_key.clone();
            async move {
                let server_id = client.server_id().to_string();
                let server_name = client.server_name().to_string();
                let result = match operation.as_str() {
                    "definition" => {
                        let position = position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                        client
                            .request(
                                "textDocument/definition",
                                json!({ "textDocument": text_document, "position": position }),
                                timeout_s,
                            )
                            .await?
                    }
                    "references" => {
                        let position = position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                        client
                            .request(
                                "textDocument/references",
                                json!({
                                    "textDocument": text_document,
                                    "position": position,
                                    "context": { "includeDeclaration": true }
                                }),
                                timeout_s,
                            )
                            .await?
                    }
                    "hover" => {
                        let position = position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                        client
                            .request(
                                "textDocument/hover",
                                json!({ "textDocument": text_document, "position": position }),
                                timeout_s,
                            )
                            .await?
                    }
                    "documentsymbol" => {
                        client
                            .request(
                                "textDocument/documentSymbol",
                                json!({ "textDocument": text_document }),
                                timeout_s,
                            )
                            .await?
                    }
                    "workspacesymbol" => {
                        let query = query.unwrap_or_default();
                        client
                            .request("workspace/symbol", json!({ "query": query }), timeout_s)
                            .await?
                    }
                    "implementation" => {
                        let position = position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                        client
                            .request(
                                "textDocument/implementation",
                                json!({ "textDocument": text_document, "position": position }),
                                timeout_s,
                            )
                            .await?
                    }
                    "callhierarchy" => {
                        let position = position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                        let items = client
                            .request(
                                "textDocument/prepareCallHierarchy",
                                json!({ "textDocument": text_document, "position": position }),
                                timeout_s,
                            )
                            .await?;
                        let calls = if let Some(item) =
                            items.as_array().and_then(|items| items.first()).cloned()
                        {
                            let method = if direction == "outgoing" {
                                "callHierarchy/outgoingCalls"
                            } else {
                                "callHierarchy/incomingCalls"
                            };
                            client
                                .request(method, json!({ "item": item }), timeout_s)
                                .await?
                        } else {
                            Value::Null
                        };
                        json!({
                            "items": items,
                            "direction": direction,
                            "calls": calls
                        })
                    }
                    _ => {
                        return Err(anyhow!("未知 LSP operation: {operation}"));
                    }
                };
                Ok(json!({
                    "server_id": server_id,
                    "server_name": server_name,
                    "result": result
                }))
            }
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let diagnostics = lsp_diagnostics_summary(&state.lsp_manager, user_id, &target);
    Ok(Json(json!({
        "ok": true,
        "operation": payload.operation,
        "path": payload.path,
        "results": results,
        "diagnostics": diagnostics
    })))
}

async fn admin_mcp_tools(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<McpToolsRequest>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let server = McpServerConfig {
        name: payload.name,
        endpoint: payload.endpoint,
        allow_tools: Vec::new(),
        packaged: false,
        enabled: true,
        transport: payload.transport,
        description: None,
        display_name: None,
        headers: payload.headers.unwrap_or_default(),
        auth: payload
            .auth
            .and_then(|value| serde_yaml::to_value(value).ok()),
        tool_specs: Vec::new(),
    };
    let timeout_s = if config.mcp.timeout_s > 0 {
        config.mcp.timeout_s.clamp(10, 300)
    } else {
        120
    };
    let tools = match tokio::time::timeout(
        Duration::from_secs(timeout_s),
        crate::mcp::fetch_tools(&config, &server),
    )
    .await
    {
        Ok(Ok(tools)) => tools,
        Ok(Err(err)) => {
            let transport = crate::mcp::normalize_transport(server.transport.as_deref());
            if transport != "streamable-http" {
                let fallback = config
                    .mcp
                    .servers
                    .iter()
                    .find(|item| item.name == server.name)
                    .map(crate::mcp::build_tool_specs_from_config)
                    .unwrap_or_default();
                return Ok(Json(
                    json!({ "tools": fallback, "warning": err.to_string() }),
                ));
            }
            return Err(error_response(StatusCode::BAD_REQUEST, err.to_string()));
        }
        Err(_) => {
            let transport = crate::mcp::normalize_transport(server.transport.as_deref());
            let warning = format!("MCP tools/list timeout after {timeout_s}s");
            if transport != "streamable-http" {
                let fallback = config
                    .mcp
                    .servers
                    .iter()
                    .find(|item| item.name == server.name)
                    .map(crate::mcp::build_tool_specs_from_config)
                    .unwrap_or_default();
                return Ok(Json(json!({ "tools": fallback, "warning": warning })));
            }
            return Err(error_response(StatusCode::BAD_REQUEST, warning));
        }
    };
    Ok(Json(json!({ "tools": tools })))
}

async fn admin_mcp_tool_call(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<McpToolCallRequest>,
) -> Result<Json<Value>, Response> {
    let server_name = payload.server.trim();
    let tool_name = payload.tool.trim();
    if server_name.is_empty() || tool_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "MCP server and tool are required".to_string(),
        ));
    }
    let config = state.config_store.get().await;
    let mut server = config
        .mcp
        .servers
        .iter()
        .find(|item| item.name == server_name)
        .cloned()
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                format!("MCP server not found: {server_name}"),
            )
        })?;
    let mut warnings = Vec::new();
    if !server.enabled {
        warnings.push(format!("server disabled: {server_name}"));
    }
    if !server.allow_tools.is_empty() && !server.allow_tools.contains(&tool_name.to_string()) {
        warnings.push(format!("tool not in allow_tools: {tool_name}"));
    }
    server.enabled = true;
    server.allow_tools = Vec::new();
    let args = payload.args.unwrap_or(Value::Null);
    let result = crate::mcp::call_tool_with_server(&config, &server, tool_name, &args)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if warnings.is_empty() {
        Ok(Json(json!({ "result": result })))
    } else {
        Ok(Json(
            json!({ "result": result, "warning": warnings.join("; ") }),
        ))
    }
}

async fn admin_a2a_list(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    Ok(Json(json!({ "services": config.a2a.services })))
}

async fn admin_a2a_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<A2aUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let previous = state.config_store.get().await;
    let updated = state
        .config_store
        .update(|config| {
            config.a2a.services = payload.services.clone();
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let previous_map: HashMap<String, bool> = previous
        .a2a
        .services
        .iter()
        .map(|service| (service.name.clone(), service.enabled))
        .collect();
    let updated_map: HashMap<String, bool> = updated
        .a2a
        .services
        .iter()
        .map(|service| (service.name.clone(), service.enabled))
        .collect();
    let mut added: Vec<String> = updated_map
        .keys()
        .filter(|name| !previous_map.contains_key(*name))
        .cloned()
        .collect();
    let mut removed: Vec<String> = previous_map
        .keys()
        .filter(|name| !updated_map.contains_key(*name))
        .cloned()
        .collect();
    let mut enabled_changed = Vec::new();
    let mut disabled_changed = Vec::new();
    for (name, enabled) in previous_map {
        if let Some(next_enabled) = updated_map.get(&name) {
            if enabled != *next_enabled {
                if *next_enabled {
                    enabled_changed.push(name);
                } else {
                    disabled_changed.push(name);
                }
            }
        }
    }
    added.sort();
    removed.sort();
    enabled_changed.sort();
    disabled_changed.sort();
    if !added.is_empty()
        || !removed.is_empty()
        || !enabled_changed.is_empty()
        || !disabled_changed.is_empty()
    {
        info!(
            "A2A 服务配置已更新: 新增 +{added_len}, 移除 -{removed_len}, 启用 +{enabled_len}, 停用 -{disabled_len}",
            added_len = added.len(),
            removed_len = removed.len(),
            enabled_len = enabled_changed.len(),
            disabled_len = disabled_changed.len(),
        );
        if !added.is_empty() {
            info!("A2A 服务已新增: {}", added.join(", "));
        }
        if !removed.is_empty() {
            info!("A2A 服务已移除: {}", removed.join(", "));
        }
        if !enabled_changed.is_empty() {
            info!("A2A 服务已启用: {}", enabled_changed.join(", "));
        }
        if !disabled_changed.is_empty() {
            info!("A2A 服务已停用: {}", disabled_changed.join(", "));
        }
    }
    Ok(Json(json!({ "services": updated.a2a.services })))
}

async fn admin_a2a_card(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<A2aCardRequest>,
) -> Result<Json<Value>, Response> {
    let endpoint = payload.endpoint.trim();
    if endpoint.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("tool.a2a.endpoint_required"),
        ));
    }

    let config = state.config_store.get().await;
    let timeout_s = if config.a2a.timeout_s > 0 {
        config.a2a.timeout_s
    } else {
        120
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_s))
        .build()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let headers = apply_auth_headers(build_header_map(payload.headers)?, payload.auth)?;
    let urls = build_a2a_agent_card_urls(endpoint)
        .map_err(|message| error_response(StatusCode::BAD_REQUEST, message))?;

    let mut last_error = String::new();
    for url in urls {
        let response = client.get(&url).headers(headers.clone()).send().await;
        let resp = match response {
            Ok(resp) => resp,
            Err(err) => {
                last_error = format!("{url}: {err}");
                continue;
            }
        };
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            last_error = format!("{url}: {status}");
            continue;
        }
        let parsed = match serde_json::from_str::<Value>(&body) {
            Ok(value) => value,
            Err(_) => {
                last_error = format!("{url}: invalid json");
                continue;
            }
        };
        let value = if parsed.is_object() {
            parsed
        } else {
            json!({ "data": parsed })
        };
        return Ok(Json(json!({ "agent_card": value })));
    }

    Err(error_response(
        StatusCode::BAD_REQUEST,
        format!(
            "AgentCard 获取失败：{}",
            if last_error.is_empty() {
                "-"
            } else {
                &last_error
            }
        ),
    ))
}

fn resolve_admin_skill_source(
    spec: &SkillSpec,
    builtin_root: Option<&Path>,
    custom_root: &Path,
) -> AdminSkillSourceKind {
    let root = normalize_existing_path(&spec.root);
    if let Some(builtin_root) = builtin_root {
        let normalized_builtin_root = normalize_existing_path(builtin_root);
        if is_within_root(&normalized_builtin_root, &root) {
            return AdminSkillSourceKind::Builtin;
        }
    }
    let normalized_custom_root = normalize_existing_path(custom_root);
    if is_within_root(&normalized_custom_root, &root) {
        return AdminSkillSourceKind::Custom;
    }
    AdminSkillSourceKind::External
}

fn normalize_admin_public_path_text(raw: &str) -> String {
    let mut path = raw.to_string();
    if cfg!(windows) {
        if let Some(stripped) = path.strip_prefix("\\\\?\\") {
            path = stripped.to_string();
        }
        if let Some(stripped) = path.strip_prefix("//?/") {
            path = stripped.to_string();
        }
    }
    path.replace('\\', "/")
}

fn normalize_admin_public_path(path: &Path) -> String {
    normalize_admin_public_path_text(&path.to_string_lossy())
}

fn admin_skill_to_value(spec: SkillSpec, enabled_set: &HashSet<String>) -> Value {
    let builtin_root = resolve_builtin_skills_root();
    let custom_root = resolve_admin_custom_skills_root()
        .unwrap_or_else(|| resolve_admin_uploaded_skills_root(true));
    let source = resolve_admin_skill_source(&spec, builtin_root.as_deref(), &custom_root);
    let name = spec.name;
    let description = spec.description;
    let path = normalize_admin_public_path_text(&spec.path);
    let input_schema = spec.input_schema;
    let enabled = enabled_set.contains(&name);
    let editable = source.editable();
    json!({
        "name": name,
        "description": description,
        "path": path,
        "input_schema": input_schema,
        "enabled": enabled,
        "builtin": source.builtin(),
        "source": source.as_str(),
        "readonly": !editable,
        "editable": editable,
    })
}

pub(crate) fn resolve_admin_skill_root(spec: &SkillSpec) -> Result<PathBuf, Response> {
    let root = normalize_existing_path(&spec.root);
    if !root.is_dir() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.skill_not_found"),
        ));
    }
    Ok(root)
}

pub(crate) fn is_admin_skill_editable(spec: &SkillSpec) -> bool {
    let builtin_root = resolve_builtin_skills_root();
    let custom_root = resolve_admin_custom_skills_root()
        .unwrap_or_else(|| resolve_admin_uploaded_skills_root(true));
    resolve_admin_skill_source(spec, builtin_root.as_deref(), &custom_root).editable()
}

pub(crate) fn ensure_admin_skill_editable(spec: &SkillSpec) -> Result<PathBuf, Response> {
    let root = resolve_admin_skill_root(spec)?;
    if !is_admin_skill_editable(spec) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.skill_builtin_readonly"),
        ));
    }
    Ok(root)
}

async fn admin_skills_list(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let scan_paths = build_admin_skill_scan_paths(&config, true);
    let public_paths = scan_paths
        .iter()
        .map(|path| normalize_admin_public_path_text(path))
        .collect::<Vec<_>>();
    let mut scan_config = config.clone();
    scan_config.skills.paths = scan_paths.clone();
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let enabled_set: HashSet<String> = config.skills.enabled.iter().cloned().collect();
    let skills = registry
        .list_specs()
        .into_iter()
        .map(|spec| admin_skill_to_value(spec, &enabled_set))
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "paths": public_paths,
        "enabled": config.skills.enabled,
        "skills": skills,
        "visibility": {
            "rules": config.tools.visibility.rules,
        }
    })))
}

pub(crate) fn resolve_admin_skill_spec(config: &Config, name: &str) -> Result<SkillSpec, Response> {
    let mut scan_config = config.clone();
    scan_config.skills.paths = build_admin_skill_scan_paths(config, true);
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    registry
        .get(name)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.skill_not_found")))
}

fn resolve_skill_file_path(root: &Path, relative_path: &str) -> Result<PathBuf, Response> {
    let rel = Path::new(relative_path);
    if rel.is_absolute() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.absolute_path_forbidden"),
        ));
    }
    if rel
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.path_out_of_bounds"),
        ));
    }
    let target = root.join(rel);
    let normalized = normalize_target_path(&target);
    if !is_within_root(root, &normalized) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.path_out_of_bounds"),
        ));
    }
    Ok(normalized)
}

async fn admin_skills_content(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillContentQuery>,
) -> Result<Json<Value>, Response> {
    let name = query.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let spec = resolve_admin_skill_spec(&config, name)?;
    let skill_path = PathBuf::from(&spec.path);
    if !skill_path.exists() || !skill_path.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.skill_file_not_found"),
        ));
    }
    let content = tokio::fs::read_to_string(&skill_path)
        .await
        .map_err(|err| {
            error_response(
                StatusCode::BAD_REQUEST,
                i18n::t_with_params(
                    "error.skill_file_read_failed",
                    &HashMap::from([("detail".to_string(), err.to_string())]),
                ),
            )
        })?;
    Ok(Json(json!({
        "name": spec.name,
        "path": normalize_admin_public_path(&skill_path),
        "content": content
    })))
}

async fn admin_skills_files(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillFilesQuery>,
) -> Result<Json<Value>, Response> {
    let name = query.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let spec = resolve_admin_skill_spec(&config, name)?;
    let root = normalize_existing_path(&spec.root);
    if !root.exists() || !root.is_dir() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.skill_file_not_found"),
        ));
    }
    let mut entries: Vec<(String, String)> = Vec::new();
    for entry in WalkDir::new(&root).into_iter().filter_map(|item| item.ok()) {
        let path = entry.path();
        if path == root {
            continue;
        }
        let rel = path.strip_prefix(&root).unwrap_or(path);
        let rel_text = rel.to_string_lossy().replace('\\', "/");
        let kind = if entry.file_type().is_dir() {
            "dir"
        } else {
            "file"
        };
        entries.push((rel_text, kind.to_string()));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let payload = entries
        .into_iter()
        .map(|(path, kind)| json!({ "path": path, "kind": kind }))
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "name": spec.name,
        "root": normalize_admin_public_path(&root),
        "entries": payload
    })))
}

async fn admin_skills_file(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillFileQuery>,
) -> Result<Json<Value>, Response> {
    let name = query.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let relative_path = query.path.trim();
    if relative_path.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.file_path_required"),
        ));
    }
    let config = state.config_store.get().await;
    let spec = resolve_admin_skill_spec(&config, name)?;
    let root = normalize_existing_path(&spec.root);
    let target = resolve_skill_file_path(&root, relative_path)?;
    if !target.exists() || !target.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.skill_file_not_found"),
        ));
    }
    let content = tokio::fs::read_to_string(&target).await.map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            i18n::t_with_params(
                "error.skill_file_read_failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            ),
        )
    })?;
    let rel = target.strip_prefix(&root).unwrap_or(&target);
    let rel_text = rel.to_string_lossy().replace('\\', "/");
    Ok(Json(json!({
        "name": spec.name,
        "path": rel_text,
        "content": content
    })))
}

async fn admin_skills_file_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SkillFileUpdate>,
) -> Result<Json<Value>, Response> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let relative_path = payload.path.trim();
    if relative_path.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.file_path_required"),
        ));
    }
    let config = state.config_store.get().await;
    let spec = resolve_admin_skill_spec(&config, name)?;
    let root = ensure_admin_skill_editable(&spec)?;
    let target = resolve_skill_file_path(&root, relative_path)?;
    if !target.exists() || !target.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.skill_file_not_found"),
        ));
    }
    tokio::fs::write(&target, payload.content.as_bytes())
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let should_reload = target
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("SKILL.md"))
        .unwrap_or(false);
    if should_reload {
        state.reload_skills(&config).await;
    }
    let rel = target.strip_prefix(&root).unwrap_or(&target);
    let rel_text = rel.to_string_lossy().replace('\\', "/");
    Ok(Json(json!({
        "ok": true,
        "path": rel_text,
        "reloaded": should_reload
    })))
}

async fn admin_skills_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SkillsUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let previous = state.config_store.get().await;
    let updated = state
        .config_store
        .update(|config| {
            if let Some(paths) = &payload.paths {
                config.skills.paths = normalize_admin_skill_paths(paths.clone(), true);
            } else {
                config.skills.paths =
                    normalize_admin_skill_paths(config.skills.paths.clone(), true);
            }
            config.skills.enabled = payload.enabled.clone();
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let previous_enabled: HashSet<String> = previous.skills.enabled.iter().cloned().collect();
    let updated_enabled: HashSet<String> = updated.skills.enabled.iter().cloned().collect();
    let mut enabled_added: Vec<String> = updated_enabled
        .difference(&previous_enabled)
        .cloned()
        .collect();
    let mut enabled_removed: Vec<String> = previous_enabled
        .difference(&updated_enabled)
        .cloned()
        .collect();
    enabled_added.sort();
    enabled_removed.sort();
    let paths_changed = payload.paths.as_ref().is_some_and(|paths| {
        normalize_admin_skill_paths(paths.clone(), true) != previous.skills.paths
    });
    if !enabled_added.is_empty() || !enabled_removed.is_empty() || paths_changed {
        info!(
            "技能配置已更新: 启用 +{enabled_added_len}, 停用 -{enabled_removed_len}, paths_changed={paths_changed}",
            enabled_added_len = enabled_added.len(),
            enabled_removed_len = enabled_removed.len(),
        );
        if !enabled_added.is_empty() {
            info!("技能已启用: {}", enabled_added.join(", "));
        }
        if !enabled_removed.is_empty() {
            info!("技能已停用: {}", enabled_removed.join(", "));
        }
        if paths_changed {
            info!("技能扫描目录已更新: {:?}", updated.skills.paths);
        }
    }
    state.reload_skills(&updated).await;
    let scan_paths = build_admin_skill_scan_paths(&updated, true);
    let public_paths = scan_paths
        .iter()
        .map(|path| normalize_admin_public_path_text(path))
        .collect::<Vec<_>>();
    let mut scan_config = updated.clone();
    scan_config.skills.paths = scan_paths.clone();
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let enabled_set: HashSet<String> = updated.skills.enabled.iter().cloned().collect();
    let skills = registry
        .list_specs()
        .into_iter()
        .map(|spec| admin_skill_to_value(spec, &enabled_set))
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "paths": public_paths,
        "enabled": updated.skills.enabled,
        "skills": skills,
        "visibility": {
            "rules": updated.tools.visibility.rules,
        }
    })))
}

async fn admin_skills_delete(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillDeleteQuery>,
) -> Result<Json<Value>, Response> {
    let name = query.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let spec = resolve_admin_skill_spec(&config, name)?;
    let skill_dir = ensure_admin_skill_editable(&spec)?;
    tokio::fs::remove_dir_all(&skill_dir).await.map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            i18n::t_with_params(
                "error.skill_delete_failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            ),
        )
    })?;
    let cleaned_enabled: Vec<String> = config
        .skills
        .enabled
        .iter()
        .filter(|&value| value != name)
        .cloned()
        .collect();
    if cleaned_enabled != config.skills.enabled {
        let updated = state
            .config_store
            .update(|config| {
                config.skills.enabled = cleaned_enabled.clone();
            })
            .await
            .map_err(|err| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t_with_params(
                        "error.skill_delete_update_failed",
                        &HashMap::from([("detail".to_string(), err.to_string())]),
                    ),
                )
            })?;
        state.reload_skills(&updated).await;
    } else {
        state.reload_skills(&config).await;
    }
    Ok(Json(
        json!({ "ok": true, "name": name, "message": i18n::t("message.skill_deleted") }),
    ))
}

async fn admin_skills_upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut filename = String::new();
    let mut data = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        filename = field.file_name().unwrap_or("skills.zip").to_string();
        data = field
            .bytes()
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .to_vec();
    }
    if !is_supported_skill_archive_filename(&filename) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_upload_zip_only"),
        ));
    }
    let skill_root = resolve_admin_uploaded_skills_root(true);
    tokio::fs::create_dir_all(&skill_root)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let config = state.config_store.get().await;
    let reserved_top_dirs = collect_admin_reserved_skill_top_dirs(&config, true);
    let import_result = blocking::run_fs("api.admin.integration.import_skill", {
        let filename = filename.clone();
        let data = data.clone();
        let skill_root = skill_root.clone();
        move || import_skill_archive(&filename, &data, &skill_root, &reserved_top_dirs)
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let updated = state
        .config_store
        .update(|config| {
            config.skills.paths = normalize_admin_skill_paths(config.skills.paths.clone(), true);
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state.reload_skills(&updated).await;
    Ok(Json(json!({
        "ok": true,
        "extracted": import_result.extracted,
        "top_level_dirs": import_result.top_level_dirs,
        "final_names": import_result.final_names,
        "message": i18n::t("message.upload_success")
    })))
}

async fn admin_skills_export(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SkillDeleteQuery>,
) -> Result<Response, Response> {
    let name = query.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let spec = resolve_admin_skill_spec(&config, name)?;
    let root = ensure_admin_skill_editable(&spec)?;
    let top_dir = root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.skill_file_not_found"),
            )
        })?;
    let archive_path = create_temp_admin_skill_archive_file()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let archive_path_clone = archive_path.clone();
    let root_clone = root.clone();
    let top_dir_clone = top_dir.to_string();
    blocking::run_fs("api.admin.integration.export_skill", move || {
        Ok(crate::services::skill_archive::create_skill_archive(
            &root_clone,
            &top_dir_clone,
            &archive_path_clone,
        )
        .map_err(|err| std::io::Error::other(err.to_string()))?)
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let filename = format!("{}.zip", sanitize_filename_stem(&spec.name));
    let bytes = tokio::fs::read(&archive_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let _ = tokio::fs::remove_file(&archive_path).await;
    Ok(zip_bytes_response(filename, bytes))
}

async fn admin_tools_list(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let (enabled, tools) = build_builtin_tools_payload(&config);
    Ok(Json(json!({
        "enabled": enabled,
        "tools": tools,
        "visibility": {
            "rules": config.tools.visibility.rules,
        }
    })))
}

fn create_temp_admin_skill_archive_file() -> Result<PathBuf, std::io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_admin_skills");
    std::fs::create_dir_all(&root)?;
    let filename = format!("wunder_admin_skill_{}.zip", Uuid::new_v4().simple());
    Ok(root.join(filename))
}

fn zip_bytes_response(filename: String, bytes: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(bytes.clone()));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        AxumHeaderValue::from_static("application/zip"),
    );
    if let Ok(value) = AxumHeaderValue::from_str(&bytes.len().to_string()) {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_LENGTH, value);
    }
    if let Ok(value) = AxumHeaderValue::from_str(&admin_content_disposition(&filename)) {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_DISPOSITION, value);
    }
    response
}

fn admin_content_disposition(filename: &str) -> String {
    let ascii_name = filename
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!(
        "attachment; filename=\"{}\"",
        ascii_name.trim().trim_matches('"')
    )
}

async fn admin_tools_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ToolsUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let previous = state.config_store.get().await;
    let previous_enabled = admin_enabled_builtin_names(&previous);
    let updated = state
        .config_store
        .update(|config| {
            apply_builtin_tools_update(config, &payload.enabled);
            if let Some(rules) = payload.visibility_rules.clone() {
                config.tools.visibility.rules = rules
                    .into_iter()
                    .map(|rule| ToolVisibilityRule {
                        name: rule.name,
                        visible_unit_ids: rule.visible_unit_ids,
                    })
                    .collect();
            }
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let updated_enabled = admin_enabled_builtin_names(&updated);
    let mut enabled_added: Vec<String> = updated_enabled
        .difference(&previous_enabled)
        .cloned()
        .collect();
    let mut enabled_removed: Vec<String> = previous_enabled
        .difference(&updated_enabled)
        .cloned()
        .collect();
    enabled_added.sort();
    enabled_removed.sort();
    if !enabled_added.is_empty() || !enabled_removed.is_empty() {
        info!(
            "内置工具配置已更新: 启用 +{enabled_added_len}, 停用 -{enabled_removed_len}",
            enabled_added_len = enabled_added.len(),
            enabled_removed_len = enabled_removed.len(),
        );
        if !enabled_added.is_empty() {
            info!("内置工具已启用: {}", enabled_added.join(", "));
        }
        if !enabled_removed.is_empty() {
            info!("内置工具已停用: {}", enabled_removed.join(", "));
        }
    }
    let (enabled, tools) = build_builtin_tools_payload(&updated);
    Ok(Json(json!({
        "enabled": enabled,
        "tools": tools,
        "visibility": {
            "rules": updated.tools.visibility.rules,
        }
    })))
}

fn normalize_builtin_enabled(enabled: &[String]) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for name in enabled {
        let canonical = resolve_tool_name(name.trim());
        if canonical.is_empty() || seen.contains(&canonical) {
            continue;
        }
        seen.insert(canonical.clone());
        output.push(canonical);
    }
    output
}

fn admin_browser_tool_name() -> String {
    resolve_tool_name("browser")
}

fn admin_enabled_builtin_names(config: &Config) -> HashSet<String> {
    let browser_tool_name = admin_browser_tool_name();
    let mut enabled: HashSet<String> = config
        .tools
        .builtin
        .enabled
        .iter()
        .map(|name| resolve_tool_name(name))
        .filter(|name| !name.is_empty() && name != &browser_tool_name)
        .collect();
    if config.tools.browser.enabled {
        enabled.insert(browser_tool_name);
    }
    enabled
}

fn apply_builtin_tools_update(config: &mut Config, enabled: &[String]) {
    let browser_tool_name = admin_browser_tool_name();
    let mut normalized = normalize_builtin_enabled(enabled);
    let browser_enabled = normalized.iter().any(|name| name == &browser_tool_name);
    normalized.retain(|name| name != &browser_tool_name);
    config.tools.browser.enabled = browser_enabled;
    config.tools.builtin.enabled = normalized;
}

fn build_builtin_tools_payload(config: &Config) -> (Vec<String>, Vec<Value>) {
    let enabled_set = admin_enabled_builtin_names(config);
    let mut canonical_aliases: HashMap<String, Vec<String>> = HashMap::new();
    for (alias, canonical) in builtin_aliases() {
        canonical_aliases.entry(canonical).or_default().push(alias);
    }
    let prefer_alias = i18n::get_language().to_lowercase().starts_with("en");
    let mut tools = Vec::new();
    for spec in builtin_tool_specs() {
        let mut display_name = spec.name.clone();
        if prefer_alias {
            if let Some(aliases) = canonical_aliases.get(&spec.name) {
                if let Some(alias) = aliases.first() {
                    display_name = alias.clone();
                }
            }
        }
        let canonical = resolve_tool_name(&display_name);
        tools.push(json!({
            "name": display_name,
            "description": spec.description,
            "input_schema": spec.input_schema,
            "enabled": enabled_set.contains(&canonical),
        }));
    }
    let enabled = tools
        .iter()
        .filter_map(|tool| {
            tool.get("enabled")
                .and_then(Value::as_bool)
                .filter(|value| *value)
                .and_then(|_| tool.get("name").and_then(Value::as_str))
                .map(|name| name.to_string())
        })
        .collect::<Vec<_>>();
    (enabled, tools)
}

#[cfg(test)]
mod tests {
    use super::{admin_browser_tool_name, apply_builtin_tools_update, build_builtin_tools_payload};
    use crate::config::Config;
    use crate::tools::resolve_tool_name;
    use serde_json::Value;

    fn tool_enabled(tools: &[Value], canonical_name: &str) -> bool {
        tools.iter().any(|tool| {
            let Some(name) = tool.get("name").and_then(Value::as_str) else {
                return false;
            };
            resolve_tool_name(name) == canonical_name
                && tool.get("enabled").and_then(Value::as_bool) == Some(true)
        })
    }

    #[test]
    fn apply_builtin_tools_update_moves_browser_toggle_to_dedicated_flag() {
        let mut config = Config::default();
        let browser_tool = admin_browser_tool_name();
        let read_tool = resolve_tool_name("read_file");

        apply_builtin_tools_update(&mut config, &[browser_tool.clone(), read_tool.clone()]);
        assert!(config.tools.browser.enabled);
        assert_eq!(config.tools.builtin.enabled, vec![read_tool.clone()]);

        apply_builtin_tools_update(&mut config, std::slice::from_ref(&read_tool));
        assert!(!config.tools.browser.enabled);
        assert_eq!(config.tools.builtin.enabled, vec![read_tool]);
    }

    #[test]
    fn build_builtin_tools_payload_uses_browser_visibility_flag() {
        let mut config = Config::default();
        let browser_tool = admin_browser_tool_name();

        config.tools.builtin.enabled = vec![browser_tool.clone()];
        config.tools.browser.enabled = false;
        let (enabled, tools) = build_builtin_tools_payload(&config);
        assert!(!enabled
            .iter()
            .any(|name| resolve_tool_name(name) == browser_tool));
        assert!(!tool_enabled(&tools, &browser_tool));

        config.tools.builtin.enabled.clear();
        config.tools.browser.enabled = true;
        let (enabled, tools) = build_builtin_tools_payload(&config);
        assert!(enabled
            .iter()
            .any(|name| resolve_tool_name(name) == browser_tool));
        assert!(tool_enabled(&tools, &browser_tool));
    }
}

fn build_header_map(headers: Option<HashMap<String, String>>) -> Result<HeaderMap, Response> {
    let mut header_map = HeaderMap::new();
    if let Some(headers) = headers {
        for (key, value) in headers {
            let name = HeaderName::from_bytes(key.as_bytes()).map_err(|err| {
                error_response(StatusCode::BAD_REQUEST, format!("Header 名称无效: {err}"))
            })?;
            let value = HeaderValue::from_str(&value).map_err(|err| {
                error_response(StatusCode::BAD_REQUEST, format!("Header 值无效: {err}"))
            })?;
            header_map.insert(name, value);
        }
    }
    Ok(header_map)
}

fn apply_auth_headers(mut headers: HeaderMap, auth: Option<Value>) -> Result<HeaderMap, Response> {
    let Some(auth) = auth else {
        return Ok(headers);
    };
    if let Value::String(token) = auth {
        if token.trim().is_empty() {
            return Ok(headers);
        }
        let has_auth = headers
            .keys()
            .any(|key| key.as_str().eq_ignore_ascii_case("authorization"));
        if !has_auth {
            let value = if token.to_lowercase().starts_with("bearer ") {
                token
            } else {
                format!("Bearer {token}")
            };
            let header = HeaderValue::from_str(&value).map_err(|err| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    format!("Authorization 无效: {err}"),
                )
            })?;
            headers.insert(AUTHORIZATION, header);
        }
        return Ok(headers);
    }
    let Value::Object(map) = auth else {
        return Ok(headers);
    };
    if let Some(Value::String(token)) = map.get("bearer_token") {
        let header = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|err| {
            error_response(
                StatusCode::BAD_REQUEST,
                format!("Authorization 无效: {err}"),
            )
        })?;
        headers.insert(AUTHORIZATION, header);
    }
    if let Some(Value::String(token)) = map.get("token") {
        let header = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|err| {
            error_response(
                StatusCode::BAD_REQUEST,
                format!("Authorization 无效: {err}"),
            )
        })?;
        headers.insert(AUTHORIZATION, header);
    }
    if let Some(Value::String(token)) = map.get("api_key") {
        let header = HeaderValue::from_str(token).map_err(|err| {
            error_response(StatusCode::BAD_REQUEST, format!("x-api-key 无效: {err}"))
        })?;
        headers.insert(HeaderName::from_static("x-api-key"), header);
    }
    Ok(headers)
}

fn build_a2a_agent_card_urls(endpoint: &str) -> Result<Vec<String>, String> {
    let cleaned = endpoint.trim();
    if cleaned.is_empty() {
        return Err(i18n::t("tool.a2a.endpoint_required"));
    }
    let normalized = if cleaned.starts_with("http://") || cleaned.starts_with("https://") {
        cleaned.to_string()
    } else {
        format!("http://{cleaned}")
    };
    let parsed = Url::parse(&normalized).map_err(|err| format!("A2A endpoint 解析失败: {err}"))?;

    let mut base_url = parsed.clone();
    base_url.set_path("");
    base_url.set_query(None);
    base_url.set_fragment(None);
    let base = base_url.as_str().trim_end_matches('/').to_string();
    let endpoint_base = parsed.as_str().trim_end_matches('/').to_string();

    let path = parsed.path().trim_end_matches('/');
    let mut base_path = String::new();
    if !path.is_empty() && path != "/" {
        base_path = path.to_string();
        if base_path.ends_with("/a2a") {
            base_path.truncate(base_path.len().saturating_sub(4));
        }
        base_path = base_path.trim_end_matches('/').to_string();
    }

    let mut urls = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |url: String| {
        if !url.is_empty() && seen.insert(url.clone()) {
            urls.push(url);
        }
    };

    push(format!("{base}/.well-known/agent-card.json"));
    if !base_path.is_empty() {
        push(format!("{base}{base_path}/.well-known/agent-card.json"));
    }
    push(format!("{endpoint_base}/extendedAgentCard"));
    push(format!("{endpoint_base}/agentCard"));
    push(format!("{base}/a2a/extendedAgentCard"));
    push(format!("{base}/a2a/agentCard"));
    if !base_path.is_empty() {
        push(format!("{base}{base_path}/a2a/extendedAgentCard"));
        push(format!("{base}{base_path}/a2a/agentCard"));
    }

    Ok(urls)
}

#[derive(Debug, Deserialize)]
struct McpUpdateRequest {
    servers: Vec<McpServerConfig>,
}

#[derive(Debug, Deserialize)]
struct LspUpdateRequest {
    lsp: LspConfig,
}

#[derive(Debug, Deserialize)]
struct LspTestRequest {
    user_id: String,
    path: String,
    operation: String,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    character: Option<u32>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    call_hierarchy_direction: Option<String>,
}

#[derive(Debug, Deserialize)]
struct McpToolsRequest {
    name: String,
    endpoint: String,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    #[serde(default)]
    auth: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct McpToolCallRequest {
    server: String,
    tool: String,
    #[serde(default)]
    args: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct A2aUpdateRequest {
    services: Vec<A2aServiceConfig>,
}

#[derive(Debug, Deserialize)]
struct A2aCardRequest {
    endpoint: String,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    #[serde(default)]
    auth: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct SkillsUpdateRequest {
    enabled: Vec<String>,
    #[serde(default)]
    paths: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SkillDeleteQuery {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SkillContentQuery {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SkillFilesQuery {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SkillFileQuery {
    name: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct SkillFileUpdate {
    name: String,
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ToolsUpdateRequest {
    enabled: Vec<String>,
    #[serde(default)]
    visibility_rules: Option<Vec<ToolVisibilityRulePayload>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolVisibilityRulePayload {
    name: String,
    #[serde(default)]
    visible_unit_ids: Vec<String>,
}
