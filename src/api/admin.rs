// 管理端 API：配置更新、监控查询、知识库与技能管理等。
use crate::api::skill_fs;
use crate::attachment::sanitize_filename_stem;
use crate::auth;
use crate::config::{A2aServiceConfig, Config, LspConfig, McpServerConfig, ToolVisibilityRule};
use crate::i18n;
use crate::llm;
use crate::lsp::{LspDiagnostic, LspManager};
use crate::path_utils::{
    is_within_root, normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::services::admin_skills::{
    build_admin_skill_scan_paths, collect_admin_reserved_skill_top_dirs,
    normalize_admin_skill_paths, resolve_admin_custom_skills_root,
    resolve_admin_uploaded_skills_root, resolve_builtin_skills_root,
};
use crate::services::default_agent_sync::{DEFAULT_AGENT_ID_ALIAS, PRESET_TEMPLATE_USER_ID};
use crate::services::skill_archive::{import_skill_archive, is_supported_skill_archive_filename};
use crate::skills::{load_skills, SkillSpec};
use crate::state::AppState;
use crate::tools::{builtin_aliases, builtin_tool_specs, resolve_tool_name};
use crate::user_store::UserStore;
use crate::{
    org_units,
    storage::{ExternalLinkRecord, OrgUnitRecord, UserAccountRecord},
};
use anyhow::anyhow;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::{HeaderMap as AxumHeaderMap, HeaderValue as AxumHeaderValue, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, routing::put, Json, Router};
use chrono::{Local, TimeZone, Utc};
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

mod channel_admin;
mod gateway_admin;
mod identity_admin;
mod knowledge_admin;
mod monitor_admin;
mod resource_admin;

const MAX_SKILL_UPLOAD_BYTES: usize = 200 * 1024 * 1024;
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

pub fn router() -> Router<Arc<AppState>> {
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
        .merge(channel_admin::router())
        .merge(gateway_admin::router())
        .merge(monitor_admin::router())
        .merge(knowledge_admin::router())
        .merge(identity_admin::router())
        .merge(resource_admin::router())
        .route(
            "/wunder/admin/llm",
            get(admin_llm_get).post(admin_llm_update),
        )
        .route(
            "/wunder/admin/llm/context_window",
            post(admin_llm_context_window),
        )
        .route("/wunder/admin/llm/tts_voices", post(admin_llm_tts_voices))
        .route(
            "/wunder/admin/system",
            get(admin_system_get).post(admin_system_update),
        )
        .route(
            "/wunder/admin/server",
            get(admin_server_get).post(admin_server_update),
        )
        .route("/wunder/admin/security", get(admin_security_get))
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

const MAX_LSP_DIAGNOSTICS: usize = 20;
const ORG_UNIT_NAME_SEPARATOR: &str = " / ";
const MAX_ORG_UNIT_LEVEL: i32 = 4;
pub(super) const DEFAULT_TEST_USER_PASSWORD: &str = "Test@123456";
pub(super) const DEFAULT_TEST_USER_PREFIX: &str = "test_user";
pub(super) const MAX_TEST_USERS_PER_UNIT: i64 = 200;
pub(super) const TEST_USER_CLEANUP_BATCH_SIZE: i64 = 200;

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
    let import_result = tokio::task::spawn_blocking({
        let filename = filename.clone();
        let data = data.clone();
        let skill_root = skill_root.clone();
        move || import_skill_archive(&filename, &data, &skill_root, &reserved_top_dirs)
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
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
    tokio::task::spawn_blocking(move || {
        crate::services::skill_archive::create_skill_archive(
            &root_clone,
            &top_dir_clone,
            &archive_path_clone,
        )
        .map_err(|err| std::io::Error::other(err.to_string()))
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
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

async fn admin_llm_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    Ok(Json(json!({ "llm": config.llm })))
}

async fn admin_llm_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LlmUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let updated = state
        .config_store
        .update(|config| {
            config.llm = payload.llm.clone();
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "llm": updated.llm })))
}

async fn admin_llm_context_window(
    Json(payload): Json<LlmContextProbeRequest>,
) -> Result<Json<Value>, Response> {
    let model = payload.model.trim();
    let provider = llm::normalize_provider(payload.provider.as_deref());
    let inline_base = payload.base_url.trim();
    let base_url = if inline_base.is_empty() {
        llm::provider_default_base_url(&provider).unwrap_or("")
    } else {
        inline_base
    };
    if base_url.is_empty() || model.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.base_url_or_model_required"),
        ));
    }
    if !llm::is_openai_compatible_provider(&provider) {
        return Ok(Json(json!({
            "max_context": Value::Null,
            "message": i18n::t("probe.provider_unsupported")
        })));
    }

    let timeout_s = payload.timeout_s.unwrap_or(15);
    let timeout_s = if timeout_s == 0 { 15 } else { timeout_s };
    let api_key = payload.api_key.as_deref().unwrap_or("");
    let result = llm::probe_openai_context_window(base_url, api_key, model, timeout_s).await;
    let payload = match result {
        Ok(Some(value)) => json!({ "max_context": value, "message": i18n::t("probe.success") }),
        Ok(None) => json!({ "max_context": Value::Null, "message": i18n::t("probe.no_context") }),
        Err(err) => {
            let message = i18n::t_with_params(
                "probe.failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            );
            json!({ "max_context": Value::Null, "message": message })
        }
    };
    Ok(Json(payload))
}

async fn admin_llm_tts_voices(
    Json(payload): Json<LlmContextProbeRequest>,
) -> Result<Json<Value>, Response> {
    let model = payload.model.trim();
    let provider = llm::normalize_provider(payload.provider.as_deref());
    let inline_base = payload.base_url.trim();
    let base_url = if inline_base.is_empty() {
        llm::provider_default_base_url(&provider).unwrap_or("")
    } else {
        inline_base
    };
    if base_url.is_empty() || model.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.base_url_or_model_required"),
        ));
    }
    if !llm::is_openai_compatible_provider(&provider) {
        return Ok(Json(json!({
            "voices": [],
            "message": i18n::t("probe.provider_unsupported")
        })));
    }

    let timeout_s = payload.timeout_s.unwrap_or(15).max(5);
    let api_key = payload.api_key.as_deref().unwrap_or("");
    let result =
        crate::multimodal_models::probe_tts_voices(base_url, api_key, model, timeout_s).await;
    let response = match result {
        Ok(voices) if !voices.is_empty() => {
            json!({ "voices": voices, "message": i18n::t("probe.success") })
        }
        Ok(_) => json!({ "voices": [], "message": i18n::t("probe.no_context") }),
        Err(err) => {
            let message = i18n::t_with_params(
                "probe.failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            );
            json!({ "voices": [], "message": message })
        }
    };
    Ok(Json(response))
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize_optional_config_string(value: String) -> Option<String> {
    let cleaned = value.trim().to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn build_system_settings_payload(config: &Config) -> Value {
    let exec_policy_mode = config
        .security
        .exec_policy_mode
        .clone()
        .unwrap_or_else(|| "allow".to_string());
    json!({
        "server": {
            "max_active_sessions": config.server.max_active_sessions,
            "stream_chunk_size": config.server.stream_chunk_size,
        },
        "security": {
            "api_key": config.api_key(),
            "external_auth_key": config.external_auth_key(),
            "external_embed_preset_agent_name": config.external_embed_preset_agent_name(),
            "external_embed_jwt_secret": config.external_embed_jwt_secret(),
            "external_embed_jwt_user_id_claim": config.external_embed_jwt_user_id_claim(),
            "allow_commands": config.security.allow_commands.clone(),
            "allow_paths": config.security.allow_paths.clone(),
            "deny_globs": config.security.deny_globs.clone(),
            "exec_policy_mode": exec_policy_mode,
        },
        "observability": {
            "log_level": config.observability.log_level.clone(),
            "monitor_event_limit": config.observability.monitor_event_limit,
            "monitor_payload_max_chars": config.observability.monitor_payload_max_chars,
            "monitor_drop_event_types": config.observability.monitor_drop_event_types.clone(),
        },
        "cors": {
            "allow_origins": config.cors.allow_origins.clone(),
            "allow_methods": config.cors.allow_methods.clone(),
            "allow_headers": config.cors.allow_headers.clone(),
            "allow_credentials": config.cors.allow_credentials,
        },
        "onlyoffice": {
            "enabled": config.onlyoffice.enabled,
            "document_server_url": config.onlyoffice.document_server_url.clone().unwrap_or_default(),
            "internal_document_server_url": config.onlyoffice.internal_document_server_url.clone().unwrap_or_default(),
            "api_url": config.onlyoffice.api_url.clone().unwrap_or_default(),
            "public_base_url": config.onlyoffice.public_base_url.clone().unwrap_or_default(),
            "jwt_secret": config.onlyoffice.jwt_secret.clone().unwrap_or_default(),
            "jwt_header": config.onlyoffice.jwt_header.clone(),
            "token_ttl_s": config.onlyoffice.token_ttl_s,
            "request_timeout_s": config.onlyoffice.request_timeout_s,
            "max_download_bytes": config.onlyoffice.max_download_bytes,
        },
        "drawio": {
            "enabled": config.drawio.enabled(),
            "editor_url": config.drawio.editor_url.clone().unwrap_or_default(),
            "max_file_bytes": config.drawio.max_file_bytes,
        },
        "ragflow": {
            "base_url": config.ragflow.base_url.clone(),
            "api_key": config.ragflow.api_key.clone().unwrap_or_default(),
            "timeout_s": config.ragflow.timeout_s,
        },
        "firecrawl": {
            "provider": config.tools.web.fetch.provider(),
            "api_key": config.tools.web.fetch.firecrawl.api_key().unwrap_or_default(),
            "base_url": config.tools.web.fetch.firecrawl.base_url(),
            "timeout_secs": config.tools.web.fetch.firecrawl.timeout_secs,
            "only_main_content": config.tools.web.fetch.firecrawl.only_main_content,
            "max_age_ms": config.tools.web.fetch.firecrawl.max_age_ms,
            "proxy": config.tools.web.fetch.firecrawl.proxy.clone(),
            "store_in_cache": config.tools.web.fetch.firecrawl.store_in_cache,
        }
    })
}

async fn admin_system_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    Ok(Json(build_system_settings_payload(&config)))
}

async fn admin_system_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SystemUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let has_updates = payload.server.is_some()
        || payload.security.is_some()
        || payload.observability.is_some()
        || payload.cors.is_some()
        || payload.onlyoffice.is_some()
        || payload.drawio.is_some()
        || payload.ragflow.is_some()
        || payload.firecrawl.is_some();
    if !has_updates {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    if let Some(server) = payload.server.as_ref() {
        if let Some(max_active_sessions) = server.max_active_sessions {
            if max_active_sessions == 0 {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.max_active_sessions_invalid"),
                ));
            }
        }
    }
    if let Some(security) = payload.security.as_ref() {
        if let Some(mode) = security.exec_policy_mode.as_ref() {
            let cleaned = mode.trim().to_lowercase();
            if !cleaned.is_empty() && !matches!(cleaned.as_str(), "allow" | "audit" | "enforce") {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.exec_policy_mode_invalid"),
                ));
            }
        }
    }
    let updated = state
        .config_store
        .update(|config| {
            if let Some(server) = payload.server {
                if let Some(max_active_sessions) = server.max_active_sessions {
                    config.server.max_active_sessions = max_active_sessions;
                }
                if let Some(stream_chunk_size) = server.stream_chunk_size {
                    config.server.stream_chunk_size = stream_chunk_size;
                }
            }
            if let Some(security) = payload.security {
                if let Some(api_key) = security.api_key {
                    let cleaned = api_key.trim().to_string();
                    if cleaned.is_empty() {
                        config.security.api_key = None;
                    } else {
                        config.security.api_key = Some(cleaned);
                    }
                }
                if let Some(external_auth_key) = security.external_auth_key {
                    let cleaned = external_auth_key.trim().to_string();
                    if cleaned.is_empty() {
                        config.security.external_auth_key = None;
                    } else {
                        config.security.external_auth_key = Some(cleaned);
                    }
                }
                if let Some(agent_name) = security.external_embed_preset_agent_name {
                    let cleaned = agent_name.trim().to_string();
                    if cleaned.is_empty() {
                        config.security.external_embed_preset_agent_name = None;
                    } else {
                        config.security.external_embed_preset_agent_name = Some(cleaned);
                    }
                }
                if let Some(jwt_secret) = security.external_embed_jwt_secret {
                    let cleaned = jwt_secret.trim().to_string();
                    if cleaned.is_empty() {
                        config.security.external_embed_jwt_secret = None;
                    } else {
                        config.security.external_embed_jwt_secret = Some(cleaned);
                    }
                }
                if let Some(user_id_claim) = security.external_embed_jwt_user_id_claim {
                    let cleaned = user_id_claim.trim().to_string();
                    if cleaned.is_empty() {
                        config.security.external_embed_jwt_user_id_claim = None;
                    } else {
                        config.security.external_embed_jwt_user_id_claim = Some(cleaned);
                    }
                }
                if let Some(allow_commands) = security.allow_commands {
                    config.security.allow_commands = normalize_string_list(allow_commands);
                }
                if let Some(allow_paths) = security.allow_paths {
                    config.security.allow_paths = normalize_string_list(allow_paths);
                }
                if let Some(deny_globs) = security.deny_globs {
                    config.security.deny_globs = normalize_string_list(deny_globs);
                }
                if let Some(exec_policy_mode) = security.exec_policy_mode {
                    let cleaned = exec_policy_mode.trim().to_lowercase();
                    if cleaned.is_empty() {
                        config.security.exec_policy_mode = None;
                    } else {
                        config.security.exec_policy_mode = Some(cleaned);
                    }
                }
            }
            if let Some(observability) = payload.observability {
                if let Some(log_level) = observability.log_level {
                    config.observability.log_level = log_level.trim().to_string();
                }
                if let Some(monitor_event_limit) = observability.monitor_event_limit {
                    config.observability.monitor_event_limit = monitor_event_limit;
                }
                if let Some(monitor_payload_max_chars) = observability.monitor_payload_max_chars {
                    config.observability.monitor_payload_max_chars = monitor_payload_max_chars;
                }
                if let Some(drop_event_types) = observability.monitor_drop_event_types {
                    config.observability.monitor_drop_event_types =
                        normalize_string_list(drop_event_types);
                }
            }
            if let Some(cors) = payload.cors {
                if let Some(allow_origins) = cors.allow_origins {
                    config.cors.allow_origins = Some(normalize_string_list(allow_origins));
                }
                if let Some(allow_methods) = cors.allow_methods {
                    config.cors.allow_methods = Some(normalize_string_list(allow_methods));
                }
                if let Some(allow_headers) = cors.allow_headers {
                    config.cors.allow_headers = Some(normalize_string_list(allow_headers));
                }
                if let Some(allow_credentials) = cors.allow_credentials {
                    config.cors.allow_credentials = Some(allow_credentials);
                }
            }
            if let Some(onlyoffice) = payload.onlyoffice {
                if let Some(enabled) = onlyoffice.enabled {
                    config.onlyoffice.enabled = enabled;
                }
                if let Some(document_server_url) = onlyoffice.document_server_url {
                    config.onlyoffice.document_server_url =
                        normalize_optional_config_string(document_server_url);
                }
                if let Some(internal_document_server_url) = onlyoffice.internal_document_server_url
                {
                    config.onlyoffice.internal_document_server_url =
                        normalize_optional_config_string(internal_document_server_url);
                }
                if let Some(api_url) = onlyoffice.api_url {
                    config.onlyoffice.api_url = normalize_optional_config_string(api_url);
                }
                if let Some(public_base_url) = onlyoffice.public_base_url {
                    config.onlyoffice.public_base_url =
                        normalize_optional_config_string(public_base_url);
                }
                if let Some(jwt_secret) = onlyoffice.jwt_secret {
                    config.onlyoffice.jwt_secret = normalize_optional_config_string(jwt_secret);
                }
                if let Some(jwt_header) = onlyoffice.jwt_header {
                    let cleaned = jwt_header.trim();
                    config.onlyoffice.jwt_header = if cleaned.is_empty() {
                        "Authorization".to_string()
                    } else {
                        cleaned.to_string()
                    };
                }
                if let Some(token_ttl_s) = onlyoffice.token_ttl_s {
                    config.onlyoffice.token_ttl_s = token_ttl_s.clamp(60, 24 * 60 * 60);
                }
                if let Some(request_timeout_s) = onlyoffice.request_timeout_s {
                    config.onlyoffice.request_timeout_s = request_timeout_s.clamp(5, 300);
                }
                if let Some(max_download_bytes) = onlyoffice.max_download_bytes {
                    config.onlyoffice.max_download_bytes =
                        max_download_bytes.clamp(1024, 1024 * 1024 * 1024);
                }
            }
            if let Some(drawio) = payload.drawio {
                if let Some(enabled) = drawio.enabled {
                    config.drawio.enabled = enabled;
                }
                if let Some(editor_url) = drawio.editor_url {
                    config.drawio.editor_url = normalize_optional_config_string(editor_url);
                }
                if let Some(max_file_bytes) = drawio.max_file_bytes {
                    config.drawio.max_file_bytes = max_file_bytes.clamp(1024, 200 * 1024 * 1024);
                }
            }
            if let Some(ragflow) = payload.ragflow {
                if let Some(base_url) = ragflow.base_url {
                    let cleaned = base_url.trim();
                    config.ragflow.base_url = cleaned.trim_end_matches('/').to_string();
                }
                if let Some(api_key) = ragflow.api_key {
                    config.ragflow.api_key = normalize_optional_config_string(api_key);
                }
                if let Some(timeout_s) = ragflow.timeout_s {
                    config.ragflow.timeout_s = timeout_s.clamp(1, 600);
                }
            }
            if let Some(firecrawl) = payload.firecrawl {
                if let Some(provider) = firecrawl.provider {
                    config.tools.web.fetch.provider =
                        match provider.trim().to_ascii_lowercase().as_str() {
                            "firecrawl" => "firecrawl".to_string(),
                            "auto" => "auto".to_string(),
                            _ => "direct".to_string(),
                        };
                }
                if let Some(api_key) = firecrawl.api_key {
                    config.tools.web.fetch.firecrawl.api_key =
                        normalize_optional_config_string(api_key);
                }
                if let Some(base_url) = firecrawl.base_url {
                    let cleaned = base_url.trim();
                    config.tools.web.fetch.firecrawl.base_url = if cleaned.is_empty() {
                        "https://api.firecrawl.dev".to_string()
                    } else {
                        cleaned.trim_end_matches('/').to_string()
                    };
                }
                if let Some(timeout_secs) = firecrawl.timeout_secs {
                    config.tools.web.fetch.firecrawl.timeout_secs = timeout_secs.clamp(1, 180);
                }
                if let Some(only_main_content) = firecrawl.only_main_content {
                    config.tools.web.fetch.firecrawl.only_main_content = only_main_content;
                }
                if let Some(max_age_ms) = firecrawl.max_age_ms {
                    config.tools.web.fetch.firecrawl.max_age_ms = max_age_ms.min(86_400_000);
                }
                if let Some(proxy) = firecrawl.proxy {
                    config.tools.web.fetch.firecrawl.proxy =
                        match proxy.trim().to_ascii_lowercase().as_str() {
                            "basic" => "basic".to_string(),
                            "stealth" => "stealth".to_string(),
                            _ => "auto".to_string(),
                        };
                }
                if let Some(store_in_cache) = firecrawl.store_in_cache {
                    config.tools.web.fetch.firecrawl.store_in_cache = store_in_cache;
                }
            }
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(build_system_settings_payload(&updated)))
}

async fn admin_server_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    Ok(Json(json!({
        "server": {
            "max_active_sessions": config.server.max_active_sessions
        }
    })))
}

async fn admin_security_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let api_key = config.api_key();
    Ok(Json(json!({
        "security": {
            "api_key": api_key
        }
    })))
}

async fn admin_server_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ServerUpdateRequest>,
) -> Result<Json<Value>, Response> {
    if let Some(max_active_sessions) = payload.max_active_sessions {
        if max_active_sessions == 0 {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.max_active_sessions_invalid"),
            ));
        }
    }
    if payload.max_active_sessions.is_none() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    let updated = state
        .config_store
        .update(|config| {
            if let Some(max_active_sessions) = payload.max_active_sessions {
                config.server.max_active_sessions = max_active_sessions;
            }
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "server": {
            "max_active_sessions": updated.server.max_active_sessions
        }
    })))
}

pub(super) fn resolve_monitor_session_agent_name(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
) -> Result<Option<String>, Response> {
    let cleaned_user = user_id.trim();
    if cleaned_user.is_empty() {
        return Ok(None);
    }
    let cleaned_agent = agent_id.trim();
    let is_default_agent = cleaned_agent.is_empty()
        || cleaned_agent.eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS)
        || cleaned_agent.eq_ignore_ascii_case("default");
    if is_default_agent {
        let record = crate::user_store::build_default_agent_record_from_storage(
            state.storage.as_ref(),
            cleaned_user,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let name = record.name.trim();
        return if name.is_empty() {
            Ok(None)
        } else {
            Ok(Some(name.to_string()))
        };
    }
    let record = state
        .user_store
        .get_user_agent(cleaned_user, cleaned_agent)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(record.and_then(|item| {
        let name = item.name.trim();
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }))
}

pub(super) fn format_ts(ts: f64) -> String {
    if ts <= 0.0 {
        return String::new();
    }
    let secs = ts.trunc() as i64;
    let nanos = ((ts.fract()) * 1_000_000_000.0) as u32;
    match Local.timestamp_opt(secs, nanos).single() {
        Some(dt) => dt.to_rfc3339(),
        None => String::new(),
    }
}

pub(super) fn empty_user_activity_series(days: i64) -> Vec<Value> {
    let safe_days = days.max(1) as usize;
    let today = Local::now().date_naive();
    (0..safe_days)
        .map(|offset| {
            let day = today - chrono::Duration::days((safe_days - 1 - offset) as i64);
            json!({
                "date": day.format("%Y-%m-%d").to_string(),
                "tokens": 0_i64,
            })
        })
        .collect()
}

pub(super) fn build_user_activity_series_map(
    state: &AppState,
    user_ids: &[String],
    days: i64,
) -> HashMap<String, Vec<Value>> {
    if user_ids.is_empty() {
        return HashMap::new();
    }
    let safe_days = days.clamp(1, 31);
    let today = Local::now().date_naive();
    let start_day = today - chrono::Duration::days(safe_days.saturating_sub(1));
    let since_time = start_day
        .and_hms_opt(0, 0, 0)
        .and_then(|dt| Local.from_local_datetime(&dt).single())
        .map(|dt| dt.timestamp() as f64)
        .unwrap_or_else(now_ts);

    let mut result = user_ids
        .iter()
        .map(|user_id| (user_id.clone(), empty_user_activity_series(safe_days)))
        .collect::<HashMap<_, _>>();

    for user_id in user_ids {
        let records = state.monitor.load_records_by_user(
            user_id,
            None,
            Some(since_time),
            safe_days.saturating_mul(24),
        );
        if records.is_empty() {
            continue;
        }
        let mut day_buckets = HashMap::<String, i64>::new();
        for record in records {
            let updated_time = record
                .get("updated_time")
                .and_then(Value::as_f64)
                .or_else(|| record.get("ended_time").and_then(Value::as_f64))
                .or_else(|| record.get("start_time").and_then(Value::as_f64))
                .unwrap_or(0.0);
            if updated_time <= 0.0 {
                continue;
            }
            let Some(day) = Local.timestamp_opt(updated_time as i64, 0).single() else {
                continue;
            };
            let day_key = day.format("%Y-%m-%d").to_string();
            let tokens = record
                .get("consumed_tokens")
                .and_then(Value::as_i64)
                .or_else(|| record.get("context_tokens_peak").and_then(Value::as_i64))
                .or_else(|| record.get("context_tokens").and_then(Value::as_i64))
                .unwrap_or(0)
                .max(0);
            let entry = day_buckets.entry(day_key).or_insert(0);
            *entry = entry.saturating_add(tokens);
        }
        if let Some(series) = result.get_mut(user_id) {
            for point in series.iter_mut() {
                let date_key = point
                    .get("date")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let tokens = day_buckets.get(&date_key).copied().unwrap_or(0);
                *point = json!({
                    "date": date_key,
                    "tokens": tokens,
                });
            }
        }
    }

    result
}

pub(super) fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

pub(super) struct AdminActor {
    pub(super) scope_unit_ids: Option<HashSet<String>>,
}

pub(super) fn resolve_admin_actor(
    state: &AppState,
    headers: &AxumHeaderMap,
    allow_leader: bool,
    units: &[OrgUnitRecord],
) -> Result<AdminActor, Response> {
    if let Some(token) = auth::extract_bearer_token(headers) {
        if let Ok(Some(user)) = state.user_store.authenticate_token(&token) {
            if UserStore::is_admin(&user) {
                return Ok(AdminActor {
                    scope_unit_ids: None,
                });
            }
            if allow_leader {
                let roots = org_units::resolve_leader_root_ids(&user.user_id, units);
                if roots.is_empty() {
                    return Err(permission_denied());
                }
                let scope = org_units::collect_descendant_unit_ids(units, &roots);
                return Ok(AdminActor {
                    scope_unit_ids: Some(scope),
                });
            }
            return Err(permission_denied());
        }
    }
    Ok(AdminActor {
        scope_unit_ids: None,
    })
}

pub(super) fn ensure_unit_scope(actor: &AdminActor, unit_id: Option<&str>) -> Result<(), Response> {
    let Some(scope) = actor.scope_unit_ids.as_ref() else {
        return Ok(());
    };
    let cleaned = unit_id
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    if let Some(unit_id) = cleaned {
        if scope.contains(unit_id) {
            return Ok(());
        }
    }
    Err(permission_denied())
}

pub(super) fn ensure_user_scope(
    actor: &AdminActor,
    record: &UserAccountRecord,
) -> Result<(), Response> {
    ensure_unit_scope(actor, record.unit_id.as_deref())
}

pub(super) fn filter_units_by_scope(
    units: Vec<OrgUnitRecord>,
    scope: Option<&HashSet<String>>,
) -> Vec<OrgUnitRecord> {
    match scope {
        Some(scope) => units
            .into_iter()
            .filter(|unit| scope.contains(&unit.unit_id))
            .collect(),
        None => units,
    }
}

pub(super) fn build_unit_map(units: &[OrgUnitRecord]) -> HashMap<String, OrgUnitRecord> {
    units
        .iter()
        .map(|unit| (unit.unit_id.clone(), unit.clone()))
        .collect()
}

pub(super) fn org_unit_payload(record: &OrgUnitRecord) -> Value {
    json!({
        "unit_id": record.unit_id,
        "parent_id": record.parent_id,
        "name": record.name,
        "level": record.level,
        "path": record.path,
        "path_name": record.path_name,
        "sort_order": record.sort_order,
        "leader_ids": record.leader_ids,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    })
}

pub(super) fn external_link_payload(record: &ExternalLinkRecord) -> Value {
    json!({
        "link_id": record.link_id,
        "title": record.title,
        "description": record.description,
        "url": record.url,
        "icon": record.icon,
        "allowed_levels": record.allowed_levels,
        "sort_order": record.sort_order,
        "enabled": record.enabled,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    })
}

pub(super) fn normalize_external_link_levels(levels: Vec<i32>) -> Vec<i32> {
    let mut items = levels
        .into_iter()
        .filter(|level| (1..=MAX_ORG_UNIT_LEVEL).contains(level))
        .collect::<Vec<_>>();
    items.sort_unstable();
    items.dedup();
    items
}

pub(super) fn normalize_external_link_icon(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or_default().trim();
    if cleaned.is_empty() {
        return "fa-globe".to_string();
    }

    let mut icon_name = normalize_external_icon_name(cleaned);
    let mut icon_color = None;

    if cleaned.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<Value>(cleaned) {
            if let Some(name) = value.get("name").and_then(Value::as_str) {
                icon_name = normalize_external_icon_name(name);
            }
            icon_color = value
                .get("color")
                .and_then(Value::as_str)
                .and_then(normalize_external_icon_color);
        }
    }

    if let Some(color) = icon_color {
        json!({
            "name": icon_name,
            "color": color,
        })
        .to_string()
    } else {
        icon_name
    }
}

fn normalize_external_icon_name(raw: &str) -> String {
    let normalized = raw
        .trim()
        .trim_start_matches("fa-solid")
        .trim_start_matches(' ')
        .trim();
    if normalized.is_empty() {
        return "fa-globe".to_string();
    }
    let icon = normalized
        .split_whitespace()
        .find(|part| part.starts_with("fa-"))
        .unwrap_or(normalized);
    if icon.starts_with("fa-") {
        icon.to_string()
    } else {
        "fa-globe".to_string()
    }
}

fn normalize_external_icon_color(raw: &str) -> Option<String> {
    let cleaned = raw.trim().trim_start_matches('#');
    let expanded = match cleaned.len() {
        3 if cleaned.chars().all(|ch| ch.is_ascii_hexdigit()) => {
            cleaned.chars().flat_map(|ch| [ch, ch]).collect::<String>()
        }
        6 if cleaned.chars().all(|ch| ch.is_ascii_hexdigit()) => cleaned.to_string(),
        _ => return None,
    };
    Some(format!("#{}", expanded.to_ascii_lowercase()))
}

pub(super) fn normalize_optional_id(raw: Option<&str>) -> Option<String> {
    raw.map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

pub(super) fn normalize_leader_ids(raw: Option<Vec<String>>) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for value in raw.unwrap_or_default() {
        let cleaned = value.trim();
        if cleaned.is_empty() {
            continue;
        }
        if seen.insert(cleaned.to_string()) {
            output.push(cleaned.to_string());
        }
    }
    output
}

pub(super) fn next_unit_sort_order(units: &[OrgUnitRecord], parent_id: Option<&str>) -> i64 {
    units
        .iter()
        .filter(|unit| unit.parent_id.as_deref() == parent_id)
        .map(|unit| unit.sort_order)
        .max()
        .unwrap_or(-1)
        + 1
}

pub(super) fn permission_denied() -> Response {
    error_response(StatusCode::FORBIDDEN, i18n::t("error.permission_denied"))
}

pub(super) fn normalize_user_status(value: Option<&str>) -> String {
    let cleaned = value.unwrap_or("active").trim();
    if cleaned.is_empty() {
        "active".to_string()
    } else {
        cleaned.to_string()
    }
}

pub(super) fn normalize_user_roles(raw: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for role in raw {
        let name = role.trim();
        if name.is_empty() {
            continue;
        }
        if seen.insert(name.to_string()) {
            output.push(name.to_string());
        }
    }
    if output.is_empty() {
        output.push("user".to_string());
    }
    output
}

pub(super) fn normalize_user_email(value: Option<String>) -> Option<String> {
    value.and_then(|email| {
        let cleaned = email.trim();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.to_string())
        }
    })
}

pub(super) fn normalize_tool_access_list(raw: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for name in raw {
        let cleaned = name.trim();
        if cleaned.is_empty() {
            continue;
        }
        let normalized = cleaned.to_string();
        if seen.insert(normalized.clone()) {
            output.push(normalized);
        }
    }
    output
}

pub(super) fn normalize_optional_tool_access_list(raw: Option<Vec<String>>) -> Option<Vec<String>> {
    raw.and_then(|values| {
        let normalized = normalize_tool_access_list(values);
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    })
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

pub(super) fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
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

#[derive(Debug, Deserialize)]
struct LlmUpdateRequest {
    llm: crate::config::LlmConfig,
}

#[derive(Debug, Deserialize)]
struct LlmContextProbeRequest {
    #[serde(default)]
    provider: Option<String>,
    base_url: String,
    #[serde(default)]
    api_key: Option<String>,
    model: String,
    #[serde(default)]
    timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SystemUpdateRequest {
    #[serde(default)]
    server: Option<SystemServerUpdateRequest>,
    #[serde(default)]
    security: Option<SystemSecurityUpdateRequest>,
    #[serde(default)]
    observability: Option<SystemObservabilityUpdateRequest>,
    #[serde(default)]
    cors: Option<SystemCorsUpdateRequest>,
    #[serde(default)]
    onlyoffice: Option<SystemOnlyOfficeUpdateRequest>,
    #[serde(default)]
    drawio: Option<SystemDrawioUpdateRequest>,
    #[serde(default)]
    ragflow: Option<SystemRagflowUpdateRequest>,
    #[serde(default)]
    firecrawl: Option<SystemFirecrawlUpdateRequest>,
}

#[derive(Debug, Deserialize)]
struct SystemServerUpdateRequest {
    #[serde(default)]
    max_active_sessions: Option<usize>,
    #[serde(default)]
    stream_chunk_size: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SystemSecurityUpdateRequest {
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    external_auth_key: Option<String>,
    #[serde(default)]
    external_embed_preset_agent_name: Option<String>,
    #[serde(default)]
    external_embed_jwt_secret: Option<String>,
    #[serde(default)]
    external_embed_jwt_user_id_claim: Option<String>,
    #[serde(default)]
    allow_commands: Option<Vec<String>>,
    #[serde(default)]
    allow_paths: Option<Vec<String>>,
    #[serde(default)]
    deny_globs: Option<Vec<String>>,
    #[serde(default)]
    exec_policy_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SystemObservabilityUpdateRequest {
    #[serde(default)]
    log_level: Option<String>,
    #[serde(default)]
    monitor_event_limit: Option<i64>,
    #[serde(default)]
    monitor_payload_max_chars: Option<i64>,
    #[serde(default)]
    monitor_drop_event_types: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SystemCorsUpdateRequest {
    #[serde(default)]
    allow_origins: Option<Vec<String>>,
    #[serde(default)]
    allow_methods: Option<Vec<String>>,
    #[serde(default)]
    allow_headers: Option<Vec<String>>,
    #[serde(default)]
    allow_credentials: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SystemOnlyOfficeUpdateRequest {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    document_server_url: Option<String>,
    #[serde(default)]
    internal_document_server_url: Option<String>,
    #[serde(default)]
    api_url: Option<String>,
    #[serde(default)]
    public_base_url: Option<String>,
    #[serde(default)]
    jwt_secret: Option<String>,
    #[serde(default)]
    jwt_header: Option<String>,
    #[serde(default)]
    token_ttl_s: Option<u64>,
    #[serde(default)]
    request_timeout_s: Option<u64>,
    #[serde(default)]
    max_download_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SystemDrawioUpdateRequest {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    editor_url: Option<String>,
    #[serde(default)]
    max_file_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SystemRagflowUpdateRequest {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SystemFirecrawlUpdateRequest {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    only_main_content: Option<bool>,
    #[serde(default)]
    max_age_ms: Option<u64>,
    #[serde(default)]
    proxy: Option<String>,
    #[serde(default)]
    store_in_cache: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ServerUpdateRequest {
    #[serde(default)]
    max_active_sessions: Option<usize>,
}
