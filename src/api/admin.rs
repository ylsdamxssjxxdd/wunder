// 管理端 API：配置更新、监控查询、知识库与技能管理等。
use crate::attachment::{convert_to_markdown, get_supported_extensions, sanitize_filename_stem};
use crate::auth;
use crate::channels::ChannelMessage;
use crate::config::{
    normalize_knowledge_base_type, A2aServiceConfig, Config, KnowledgeBaseConfig,
    KnowledgeBaseType, LspConfig, McpServerConfig,
};
use crate::gateway::GatewayNodeInvokeRequest;
use crate::i18n;
use crate::knowledge;
use crate::llm;
use crate::lsp::{LspDiagnostic, LspManager};
use crate::path_utils::{
    is_within_root, normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::performance::{
    run_sample as run_performance_sample, PerformanceSampleRequest, PerformanceSampleResponse,
};
use crate::skills::{load_skills, SkillSpec};
use crate::state::AppState;
use crate::throughput::{
    ThroughputConfig, ThroughputReport, ThroughputSnapshot, ThroughputStatusResponse,
};
use crate::tools::{builtin_aliases, builtin_tool_specs, resolve_tool_name};
use crate::user_store::UserStore;
use crate::vector_knowledge;
use crate::{
    org_units,
    storage::{
        ChannelAccountRecord, ChannelBindingRecord, ExternalLinkRecord, GatewayNodeRecord,
        GatewayNodeTokenRecord, OrgUnitRecord, StorageBackend, UserAccountRecord,
    },
};
use anyhow::anyhow;
use axum::extract::{Multipart, Path as AxumPath, Query, State};
use axum::http::{HeaderMap as AxumHeaderMap, StatusCode};
use axum::response::Response;
use axum::{
    routing::delete, routing::get, routing::patch, routing::post, routing::put, Json, Router,
};
use chrono::{Local, TimeZone, Utc};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tracing::info;
use url::Url;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::ZipArchive;

const MAX_KNOWLEDGE_UPLOAD_BYTES: usize = 20 * 1024 * 1024;
const MAX_KNOWLEDGE_CONTENT_BYTES: usize = 10 * 1024 * 1024;

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
            get(admin_skills_file).put(admin_skills_file_update),
        )
        .route("/wunder/admin/skills/upload", post(admin_skills_upload))
        .route(
            "/wunder/admin/tools",
            get(admin_tools_list).post(admin_tools_update),
        )
        .route(
            "/wunder/admin/channels/accounts",
            get(admin_channel_accounts).post(admin_channel_accounts_upsert),
        )
        .route(
            "/wunder/admin/channels/accounts/{channel}/{account_id}",
            delete(admin_channel_accounts_delete),
        )
        .route(
            "/wunder/admin/channels/bindings",
            get(admin_channel_bindings).post(admin_channel_bindings_upsert),
        )
        .route(
            "/wunder/admin/channels/bindings/{binding_id}",
            delete(admin_channel_bindings_delete),
        )
        .route(
            "/wunder/admin/channels/user_bindings",
            get(admin_channel_user_bindings).post(admin_channel_user_bindings_upsert),
        )
        .route(
            "/wunder/admin/channels/user_bindings/{channel}/{account_id}/{peer_kind}/{peer_id}",
            delete(admin_channel_user_bindings_delete),
        )
        .route(
            "/wunder/admin/channels/sessions",
            get(admin_channel_sessions),
        )
        .route("/wunder/admin/channels/test", post(admin_channel_test))
        .route("/wunder/admin/gateway/status", get(admin_gateway_status))
        .route(
            "/wunder/admin/gateway/presence",
            get(admin_gateway_presence),
        )
        .route("/wunder/admin/gateway/clients", get(admin_gateway_clients))
        .route(
            "/wunder/admin/gateway/nodes",
            get(admin_gateway_nodes).post(admin_gateway_nodes_upsert),
        )
        .route(
            "/wunder/admin/gateway/node_tokens",
            get(admin_gateway_node_tokens).post(admin_gateway_node_tokens_create),
        )
        .route(
            "/wunder/admin/gateway/node_tokens/{token}",
            delete(admin_gateway_node_tokens_delete),
        )
        .route("/wunder/admin/gateway/invoke", post(admin_gateway_invoke))
        .route(
            "/wunder/admin/knowledge",
            get(admin_knowledge_get).post(admin_knowledge_update),
        )
        .route("/wunder/admin/knowledge/files", get(admin_knowledge_files))
        .route(
            "/wunder/admin/knowledge/file",
            get(admin_knowledge_file)
                .put(admin_knowledge_file_update)
                .delete(admin_knowledge_file_delete),
        )
        .route("/wunder/admin/knowledge/docs", get(admin_knowledge_docs))
        .route(
            "/wunder/admin/knowledge/doc",
            get(admin_knowledge_doc).delete(admin_knowledge_doc_delete),
        )
        .route(
            "/wunder/admin/knowledge/chunks",
            get(admin_knowledge_chunks),
        )
        .route(
            "/wunder/admin/knowledge/chunk/update",
            post(admin_knowledge_chunk_update),
        )
        .route(
            "/wunder/admin/knowledge/chunk/embed",
            post(admin_knowledge_chunk_embed),
        )
        .route(
            "/wunder/admin/knowledge/chunk/delete",
            post(admin_knowledge_chunk_delete),
        )
        .route("/wunder/admin/knowledge/test", post(admin_knowledge_test))
        .route(
            "/wunder/admin/knowledge/upload",
            post(admin_knowledge_upload),
        )
        .route(
            "/wunder/admin/knowledge/refresh",
            post(admin_knowledge_refresh),
        )
        .route(
            "/wunder/admin/knowledge/reindex",
            post(admin_knowledge_reindex),
        )
        .route(
            "/wunder/admin/llm",
            get(admin_llm_get).post(admin_llm_update),
        )
        .route(
            "/wunder/admin/llm/context_window",
            post(admin_llm_context_window),
        )
        .route(
            "/wunder/admin/system",
            get(admin_system_get).post(admin_system_update),
        )
        .route(
            "/wunder/admin/server",
            get(admin_server_get).post(admin_server_update),
        )
        .route("/wunder/admin/security", get(admin_security_get))
        .route("/wunder/admin/monitor", get(admin_monitor))
        .route(
            "/wunder/admin/monitor/tool_usage",
            get(admin_monitor_tool_usage),
        )
        .route(
            "/wunder/admin/monitor/{session_id}",
            get(admin_monitor_detail).delete(admin_monitor_delete),
        )
        .route(
            "/wunder/admin/monitor/{session_id}/cancel",
            post(admin_monitor_cancel),
        )
        .route(
            "/wunder/admin/monitor/{session_id}/compaction",
            post(admin_monitor_compaction),
        )
        .route(
            "/wunder/admin/throughput/start",
            post(admin_throughput_start),
        )
        .route("/wunder/admin/throughput/stop", post(admin_throughput_stop))
        .route(
            "/wunder/admin/throughput/status",
            get(admin_throughput_status),
        )
        .route(
            "/wunder/admin/throughput/report",
            get(admin_throughput_report),
        )
        .route(
            "/wunder/admin/performance/sample",
            post(admin_performance_sample),
        )
        .route(
            "/wunder/admin/org_units",
            get(admin_org_units_list).post(admin_org_units_create),
        )
        .route(
            "/wunder/admin/org_units/{unit_id}",
            patch(admin_org_units_update).delete(admin_org_units_delete),
        )
        .route(
            "/wunder/admin/external_links",
            get(admin_external_links_list).post(admin_external_links_upsert),
        )
        .route(
            "/wunder/admin/external_links/{link_id}",
            delete(admin_external_links_delete),
        )
        .route(
            "/wunder/admin/user_accounts",
            get(admin_user_accounts_list).post(admin_user_accounts_create),
        )
        .route(
            "/wunder/admin/user_accounts/test/seed",
            post(admin_user_accounts_seed),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}",
            patch(admin_user_accounts_update).delete(admin_user_accounts_delete),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}/password",
            post(admin_user_accounts_reset_password),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}/tool_access",
            get(admin_user_accounts_tool_access_get).put(admin_user_accounts_tool_access_update),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}/agent_access",
            get(admin_user_accounts_agent_access_get).put(admin_user_accounts_agent_access_update),
        )
        .route(
            "/wunder/admin/users/throughput/cleanup",
            post(admin_users_cleanup_throughput),
        )
        .route("/wunder/admin/users", get(admin_users))
        .route(
            "/wunder/admin/users/{user_id}/sessions",
            get(admin_user_sessions),
        )
        .route("/wunder/admin/users/{user_id}", delete(admin_user_delete))
        .route("/wunder/admin/memory/users", get(admin_memory_users))
        .route("/wunder/admin/memory/status", get(admin_memory_status))
        .route(
            "/wunder/admin/memory/status/{task_id}",
            get(admin_memory_status_detail),
        )
        .route(
            "/wunder/admin/memory/{user_id}",
            get(admin_memory_records).delete(admin_memory_clear),
        )
        .route(
            "/wunder/admin/memory/{user_id}/enabled",
            post(admin_memory_enabled),
        )
        .route(
            "/wunder/admin/memory/{user_id}/{session_id}",
            put(admin_memory_update).delete(admin_memory_delete),
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

const MAX_LSP_DIAGNOSTICS: usize = 20;
const ORG_UNIT_NAME_SEPARATOR: &str = " / ";
const MAX_ORG_UNIT_LEVEL: i32 = 4;
const DEFAULT_TEST_USER_PASSWORD: &str = "Test@123456";
const DEFAULT_TEST_USER_PREFIX: &str = "test_user";
const MAX_TEST_USERS_PER_UNIT: i64 = 200;

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

async fn admin_skills_list(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let mut scan_paths = config.skills.paths.clone();
    let eva_skills = Path::new("EVA_SKILLS");
    if eva_skills.exists() && !scan_paths.iter().any(|item| item == "EVA_SKILLS") {
        scan_paths.push("EVA_SKILLS".to_string());
    }
    let mut scan_config = config.clone();
    scan_config.skills.paths = scan_paths.clone();
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let enabled_set: HashSet<String> = config.skills.enabled.iter().cloned().collect();
    let skills = registry
        .list_specs()
        .into_iter()
        .map(|spec| {
            json!({
                "name": spec.name,
                "description": spec.description,
                "path": spec.path,
                "input_schema": spec.input_schema,
                "enabled": enabled_set.contains(&spec.name),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "paths": scan_paths,
        "enabled": config.skills.enabled,
        "skills": skills
    })))
}

fn resolve_admin_skill_spec(config: &Config, name: &str) -> Result<SkillSpec, Response> {
    let mut scan_paths = config.skills.paths.clone();
    let eva_skills = Path::new("EVA_SKILLS");
    if eva_skills.exists() && !scan_paths.iter().any(|item| item == "EVA_SKILLS") {
        scan_paths.push("EVA_SKILLS".to_string());
    }
    let mut scan_config = config.clone();
    scan_config.skills.paths = scan_paths;
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
    let mut scan_paths = config.skills.paths.clone();
    let eva_skills = Path::new("EVA_SKILLS");
    if eva_skills.exists() && !scan_paths.iter().any(|item| item == "EVA_SKILLS") {
        scan_paths.push("EVA_SKILLS".to_string());
    }
    let mut scan_config = config.clone();
    scan_config.skills.paths = scan_paths;
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let spec = registry
        .get(name)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.skill_not_found")))?;
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
        "path": skill_path.to_string_lossy(),
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
        "root": root.to_string_lossy().replace('\\', "/"),
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
    let root = normalize_existing_path(&spec.root);
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
                config.skills.paths = paths.clone();
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
    let paths_changed = payload
        .paths
        .as_ref()
        .is_some_and(|paths| *paths != previous.skills.paths);
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
    let mut scan_paths = updated.skills.paths.clone();
    let eva_skills = Path::new("EVA_SKILLS");
    if eva_skills.exists() && !scan_paths.iter().any(|item| item == "EVA_SKILLS") {
        scan_paths.push("EVA_SKILLS".to_string());
    }
    let mut scan_config = updated.clone();
    scan_config.skills.paths = scan_paths.clone();
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let enabled_set: HashSet<String> = updated.skills.enabled.iter().cloned().collect();
    let skills = registry
        .list_specs()
        .into_iter()
        .map(|spec| {
            json!({
                "name": spec.name,
                "description": spec.description,
                "path": spec.path,
                "input_schema": spec.input_schema,
                "enabled": enabled_set.contains(&spec.name),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "paths": scan_paths,
        "enabled": updated.skills.enabled,
        "skills": skills
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
    let mut scan_paths = config.skills.paths.clone();
    let eva_skills = Path::new("EVA_SKILLS");
    let eva_root = eva_skills
        .canonicalize()
        .unwrap_or_else(|_| eva_skills.to_path_buf());
    if eva_skills.exists() && !scan_paths.iter().any(|item| item == "EVA_SKILLS") {
        scan_paths.push("EVA_SKILLS".to_string());
    }
    let mut scan_config = config.clone();
    scan_config.skills.paths = scan_paths;
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let spec = registry
        .get(name)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.skill_not_found")))?;
    let skill_path = PathBuf::from(&spec.path);
    if !skill_path.exists() || !skill_path.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.skill_file_not_found"),
        ));
    }
    if !eva_root.exists() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skills_dir_missing"),
        ));
    }
    let skill_dir = skill_path.parent().unwrap_or(&eva_root).to_path_buf();
    let skill_dir = skill_dir.canonicalize().unwrap_or(skill_dir);
    if skill_dir != eva_root && !skill_dir.starts_with(&eva_root) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_delete_restricted"),
        ));
    }
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
    let lower_name = filename.to_lowercase();
    if !(lower_name.ends_with(".zip") || lower_name.ends_with(".skill")) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_upload_zip_only"),
        ));
    }
    let skill_root = Path::new("EVA_SKILLS").to_path_buf();
    tokio::fs::create_dir_all(&skill_root)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, i18n::t("error.zip_invalid")))?;
    let mut extracted = 0;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|_| error_response(StatusCode::BAD_REQUEST, i18n::t("error.zip_invalid")))?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().replace('\\', "/");
        if name.starts_with('/') || name.starts_with('\\') {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.zip_path_invalid"),
            ));
        }
        let path = Path::new(&name);
        if path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.zip_path_illegal"),
            ));
        }
        let dest = skill_root.join(path);
        let dest = dest.canonicalize().unwrap_or(dest);
        if dest != skill_root && !dest.starts_with(&skill_root) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.zip_path_out_of_bounds"),
            ));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        std::fs::write(&dest, buffer)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        extracted += 1;
    }
    let config = state.config_store.get().await;
    state.reload_skills(&config).await;
    Ok(Json(
        json!({ "ok": true, "extracted": extracted, "message": i18n::t("message.upload_success") }),
    ))
}

async fn admin_tools_list(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let (enabled, tools) = build_builtin_tools_payload(&config);
    Ok(Json(json!({
        "enabled": enabled,
        "tools": tools
    })))
}

async fn admin_tools_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ToolsUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let previous = state.config_store.get().await;
    let updated = state
        .config_store
        .update(|config| {
            config.tools.builtin.enabled = normalize_builtin_enabled(&payload.enabled);
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let previous_enabled: HashSet<String> =
        previous.tools.builtin.enabled.iter().cloned().collect();
    let updated_enabled: HashSet<String> = updated.tools.builtin.enabled.iter().cloned().collect();
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
        "tools": tools
    })))
}

async fn admin_knowledge_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    Ok(Json(
        json!({ "knowledge": { "bases": config.knowledge.bases } }),
    ))
}

async fn admin_knowledge_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let normalized = normalize_admin_knowledge_bases(&config, payload.knowledge.bases)?;
    let removed_vector_bases = collect_removed_vector_bases(&config.knowledge.bases, &normalized);
    let updated = state
        .config_store
        .update(|config| {
            config.knowledge.bases = normalized.clone();
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    cleanup_removed_vector_roots(state.storage.clone(), removed_vector_bases).await;
    Ok(Json(
        json!({ "knowledge": { "bases": updated.knowledge.bases } }),
    ))
}

fn collect_removed_vector_bases(
    current: &[KnowledgeBaseConfig],
    next: &[KnowledgeBaseConfig],
) -> Vec<String> {
    let mut next_vector = HashSet::new();
    for base in next {
        if base.is_vector() {
            next_vector.insert(base.name.clone());
        }
    }
    current
        .iter()
        .filter(|base| base.is_vector())
        .filter(|base| !next_vector.contains(&base.name))
        .map(|base| base.name.clone())
        .collect()
}

async fn cleanup_removed_vector_roots(storage: Arc<dyn StorageBackend>, bases: Vec<String>) {
    for name in bases {
        let owner_key = vector_knowledge::resolve_owner_key(None);
        let _ = storage.delete_vector_documents_by_base(&owner_key, &name);
        let root = match vector_knowledge::resolve_vector_root(None, &name, false) {
            Ok(path) => path,
            Err(err) => {
                info!("Failed to resolve vector knowledge root for {name}: {err}");
                continue;
            }
        };
        if let Err(err) = tokio::fs::remove_dir_all(&root).await {
            if err.kind() != ErrorKind::NotFound {
                info!(
                    "Failed to remove vector knowledge root {}: {}",
                    root.to_string_lossy(),
                    err
                );
            }
        }
    }
}

async fn admin_knowledge_files(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeFilesQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    if base.is_vector() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_not_file_based"),
        ));
    }
    let root = resolve_knowledge_root(&base, false)?;
    let files = list_markdown_files(&root);
    Ok(Json(json!({ "base": query.base, "files": files })))
}

async fn admin_knowledge_file(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeFileQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    if base.is_vector() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_not_file_based"),
        ));
    }
    let root = resolve_knowledge_root(&base, false)?;
    let target = resolve_knowledge_path(&root, &query.path)?;
    if target
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        != "md"
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.markdown_only"),
        ));
    }
    if !target.exists() || !target.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.file_not_found"),
        ));
    }
    let content = tokio::fs::read_to_string(&target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "base": query.base,
        "path": query.path,
        "content": content
    })))
}

async fn admin_knowledge_file_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeFileUpdate>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &payload.base)?;
    if base.is_vector() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_not_file_based"),
        ));
    }
    let root = resolve_knowledge_root(&base, true)?;
    let target = resolve_knowledge_path(&root, &payload.path)?;
    if target
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        != "md"
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.markdown_only"),
        ));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(&target, payload.content)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    knowledge::refresh_knowledge_cache(&KnowledgeBaseConfig {
        name: base.name.clone(),
        description: base.description.clone(),
        root: root.to_string_lossy().to_string(),
        enabled: base.enabled,
        shared: base.shared,
        base_type: base.base_type.clone(),
        embedding_model: base.embedding_model.clone(),
        chunk_size: base.chunk_size,
        chunk_overlap: base.chunk_overlap,
        top_k: base.top_k,
        score_threshold: base.score_threshold,
    })
    .await;
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.saved_and_reindexed") }),
    ))
}

async fn admin_knowledge_file_delete(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeFileQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    if base.is_vector() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_not_file_based"),
        ));
    }
    let root = resolve_knowledge_root(&base, true)?;
    let target = resolve_knowledge_path(&root, &query.path)?;
    if target
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        != "md"
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.markdown_only"),
        ));
    }
    if target.exists() && target.is_file() {
        tokio::fs::remove_file(&target)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        knowledge::refresh_knowledge_cache(&KnowledgeBaseConfig {
            name: base.name.clone(),
            description: base.description.clone(),
            root: root.to_string_lossy().to_string(),
            enabled: base.enabled,
            shared: base.shared,
            base_type: base.base_type.clone(),
            embedding_model: base.embedding_model.clone(),
            chunk_size: base.chunk_size,
            chunk_overlap: base.chunk_overlap,
            top_k: base.top_k,
            score_threshold: base.score_threshold,
        })
        .await;
    }
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.deleted") }),
    ))
}

async fn admin_knowledge_docs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeDocsQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let docs =
        vector_knowledge::list_vector_documents(state.storage.as_ref(), None, &base.name, &root)
            .await
            .map_err(vector_error_response)?;
    Ok(Json(json!({ "base": query.base, "docs": docs })))
}

async fn admin_knowledge_doc(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeDocQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let meta = vector_knowledge::read_vector_document_meta(
        state.storage.as_ref(),
        None,
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    let content = vector_knowledge::read_vector_document_content(
        state.storage.as_ref(),
        None,
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    Ok(Json(
        json!({ "base": query.base, "doc": meta, "content": content }),
    ))
}

async fn admin_knowledge_doc_delete(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeDocQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let root_for_lock = root.clone();
    let doc_id = query.doc_id.clone();
    let base_name = base.name.clone();
    let storage = state.storage.clone();
    let (meta, deleted) = vector_knowledge::with_document_lock(&root_for_lock, &doc_id, || {
        let storage = storage.clone();
        let base_name = base_name.clone();
        let root = root.clone();
        let doc_id = doc_id.clone();
        async move {
            let meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let client = vector_knowledge::resolve_weaviate_client(&config)
                .map_err(vector_error_response)?;
            let owner_key = vector_knowledge::resolve_owner_key(None);
            let deleted = client
                .delete_doc_chunks_all(&owner_key, &base_name, &meta.embedding_model, &meta.doc_id)
                .await
                .map_err(vector_error_response)?;
            vector_knowledge::delete_vector_document_files(
                storage.as_ref(),
                None,
                &base_name,
                &root,
                &meta.doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            Ok((meta, deleted))
        }
    })
    .await?;
    Ok(Json(json!({
        "ok": true,
        "deleted": deleted,
        "doc_id": meta.doc_id,
        "doc_name": meta.name
    })))
}

async fn admin_knowledge_chunks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeChunksQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let meta = vector_knowledge::read_vector_document_meta(
        state.storage.as_ref(),
        None,
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    let content = vector_knowledge::read_vector_document_content(
        state.storage.as_ref(),
        None,
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    let chunks = vector_knowledge::build_chunk_previews(&content, &meta).await;
    Ok(Json(json!({
        "base": query.base,
        "doc_id": query.doc_id,
        "chunks": chunks
    })))
}

async fn admin_knowledge_test(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeTestRequest>,
) -> Result<Json<Value>, Response> {
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let query = payload.query.trim();
    if query.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_query_required"),
        ));
    }
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    if base.is_vector() {
        let embedding_name = base.embedding_model.as_deref().unwrap_or("").trim();
        let embed_config = vector_knowledge::resolve_embedding_model(&config, embedding_name)
            .map_err(vector_error_response)?;
        let timeout_s = embed_config.timeout_s.unwrap_or(120);
        let vectors = llm::embed_texts(&embed_config, &[query.to_string()], timeout_s)
            .await
            .map_err(vector_error_response)?;
        let vector = vectors.first().ok_or_else(|| {
            error_response(StatusCode::BAD_REQUEST, i18n::t("error.llm_request_failed"))
        })?;
        let top_k = payload
            .top_k
            .filter(|value| *value > 0)
            .unwrap_or_else(|| vector_knowledge::resolve_top_k(&base));
        let client =
            vector_knowledge::resolve_weaviate_client(&config).map_err(vector_error_response)?;
        let owner_key = vector_knowledge::resolve_owner_key(None);
        let mut hits = client
            .query_chunks(&owner_key, &base.name, embedding_name, vector, top_k)
            .await
            .map_err(vector_error_response)?;
        if let Some(threshold) = base.score_threshold {
            hits.retain(|hit| hit.score.unwrap_or(0.0) >= f64::from(threshold));
        }
        if hits.len() > top_k {
            hits.truncate(top_k);
        }
        let items = hits
            .into_iter()
            .map(|hit| {
                json!({
                    "doc_id": hit.doc_id,
                    "document": hit.doc_name,
                    "chunk_index": hit.chunk_index,
                    "start": hit.start,
                    "end": hit.end,
                    "content": hit.content,
                    "embedding_model": hit.embedding_model,
                    "score": hit.score
                })
            })
            .collect::<Vec<_>>();
        Ok(Json(json!({
            "base": base.name,
            "query": query,
            "embedding_model": embedding_name,
            "top_k": top_k,
            "hits": items
        })))
    } else {
        let _ = resolve_knowledge_root(&base, false)?;
        let llm_config = knowledge::resolve_llm_config(&config, None);
        let (reply, docs) = knowledge::query_knowledge_raw_with_documents(
            query,
            &base,
            llm_config.as_ref(),
            payload.top_k,
            None,
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let items = docs
            .into_iter()
            .map(|doc| {
                let document = if doc.document.trim().is_empty() {
                    doc.name.clone()
                } else {
                    doc.document.clone()
                };
                json!({
                    "doc_id": doc.code,
                    "document": document,
                    "chunk_index": Value::Null,
                    "content": doc.content,
                    "score": doc.score,
                    "section_path": doc.section_path,
                    "reason": doc.reason
                })
            })
            .collect::<Vec<_>>();
        Ok(Json(json!({
            "base": base.name,
            "query": query,
            "text": reply,
            "hits": items
        })))
    }
}

async fn admin_knowledge_chunk_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeChunkUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let doc_id = payload.doc_id.trim();
    if doc_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_document_not_found"),
        ));
    }
    let content_text = payload.content.trim();
    if content_text.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let root_for_lock = root.clone();
    let doc_id = doc_id.to_string();
    let base_name = base.name.clone();
    let storage = state.storage.clone();
    let meta = vector_knowledge::with_document_lock(&root_for_lock, &doc_id, || {
        let storage = storage.clone();
        let base_name = base_name.clone();
        let root = root.clone();
        let doc_id = doc_id.clone();
        async move {
            let mut meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let content = vector_knowledge::read_vector_document_content(
                storage.as_ref(),
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let chunk = meta
                .chunks
                .iter_mut()
                .find(|chunk| chunk.index == payload.chunk_index)
                .ok_or_else(|| {
                    error_response(
                        StatusCode::NOT_FOUND,
                        i18n::t("error.knowledge_chunk_not_found"),
                    )
                })?;
            if chunk.status.as_deref() == Some("deleted") {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.knowledge_chunk_deleted"),
                ));
            }
            chunk.content = Some(content_text.to_string());
            chunk.status = Some("pending".to_string());
            vector_knowledge::refresh_document_meta(&mut meta);
            vector_knowledge::write_vector_document(
                storage.as_ref(),
                None,
                &base_name,
                &meta,
                &content,
            )
            .await
            .map_err(vector_error_response)?;
            Ok(meta)
        }
    })
    .await?;
    Ok(Json(json!({ "ok": true, "doc": meta })))
}

async fn admin_knowledge_chunk_embed(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeChunkActionRequest>,
) -> Result<Json<Value>, Response> {
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let doc_id = payload.doc_id.trim();
    if doc_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_document_not_found"),
        ));
    }
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let root_for_lock = root.clone();
    let doc_id = doc_id.to_string();
    let base_name = base.name.clone();
    let storage = state.storage.clone();
    let meta = vector_knowledge::with_document_lock(&root_for_lock, &doc_id, || {
        let storage = storage.clone();
        let base_name = base_name.clone();
        let root = root.clone();
        let doc_id = doc_id.clone();
        async move {
            let mut meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let content = vector_knowledge::read_vector_document_content(
                storage.as_ref(),
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let chunk = meta
                .chunks
                .iter_mut()
                .find(|chunk| chunk.index == payload.chunk_index)
                .ok_or_else(|| {
                    error_response(
                        StatusCode::NOT_FOUND,
                        i18n::t("error.knowledge_chunk_not_found"),
                    )
                })?;
            if chunk.status.as_deref() == Some("deleted") {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.knowledge_chunk_deleted"),
                ));
            }
            let embedding_name = base
                .embedding_model
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_string();
            let embed_config = vector_knowledge::resolve_embedding_model(&config, &embedding_name)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            meta.embedding_model = embedding_name.clone();
            let content_chars: Vec<char> = content.chars().collect();
            let chunk_content = vector_knowledge::resolve_chunk_content(&content_chars, chunk);
            if chunk_content.trim().is_empty() {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.content_required"),
                ));
            }
            let vector_chunk = vector_knowledge::VectorChunk {
                index: chunk.index,
                start: chunk.start,
                end: chunk.end,
                content: chunk_content,
                chunk_id: vector_knowledge::build_chunk_id(&meta.doc_id, chunk.index),
            };
            let timeout_s = embed_config.timeout_s.unwrap_or(120);
            let vectors = vector_knowledge::embed_chunks(
                &embed_config,
                std::slice::from_ref(&vector_chunk),
                timeout_s,
            )
            .await
            .map_err(vector_error_response)?;
            let client = vector_knowledge::resolve_weaviate_client(&config)
                .map_err(vector_error_response)?;
            let owner_key = vector_knowledge::resolve_owner_key(None);
            let _ = client
                .upsert_chunks(
                    &owner_key,
                    &base_name,
                    &meta.doc_id,
                    &meta.name,
                    &embedding_name,
                    &[vector_chunk],
                    &vectors,
                )
                .await
                .map_err(vector_error_response)?;
            chunk.status = Some("embedded".to_string());
            vector_knowledge::refresh_document_meta(&mut meta);
            vector_knowledge::write_vector_document(
                storage.as_ref(),
                None,
                &base_name,
                &meta,
                &content,
            )
            .await
            .map_err(vector_error_response)?;
            Ok(meta)
        }
    })
    .await?;
    Ok(Json(json!({ "ok": true, "doc": meta })))
}

async fn admin_knowledge_chunk_delete(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeChunkActionRequest>,
) -> Result<Json<Value>, Response> {
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let doc_id = payload.doc_id.trim();
    if doc_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_document_not_found"),
        ));
    }
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let root_for_lock = root.clone();
    let doc_id = doc_id.to_string();
    let base_name = base.name.clone();
    let storage = state.storage.clone();
    let meta = vector_knowledge::with_document_lock(&root_for_lock, &doc_id, || {
        let storage = storage.clone();
        let base_name = base_name.clone();
        let root = root.clone();
        let doc_id = doc_id.clone();
        async move {
            let mut meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let content = vector_knowledge::read_vector_document_content(
                storage.as_ref(),
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let chunk = meta
                .chunks
                .iter_mut()
                .find(|chunk| chunk.index == payload.chunk_index)
                .ok_or_else(|| {
                    error_response(
                        StatusCode::NOT_FOUND,
                        i18n::t("error.knowledge_chunk_not_found"),
                    )
                })?;
            if chunk.status.as_deref() == Some("deleted") {
                return Ok(meta);
            }
            let client = vector_knowledge::resolve_weaviate_client(&config)
                .map_err(vector_error_response)?;
            let _ = client
                .delete_chunk(&vector_knowledge::build_chunk_id(&meta.doc_id, chunk.index))
                .await;
            chunk.status = Some("deleted".to_string());
            vector_knowledge::refresh_document_meta(&mut meta);
            vector_knowledge::write_vector_document(
                storage.as_ref(),
                None,
                &base_name,
                &meta,
                &content,
            )
            .await
            .map_err(vector_error_response)?;
            Ok(meta)
        }
    })
    .await?;
    Ok(Json(json!({ "ok": true, "doc": meta })))
}

async fn admin_knowledge_reindex(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeReindexRequest>,
) -> Result<Json<Value>, Response> {
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, true)?;
    let base_name = base.name.clone();
    let storage = state.storage.clone();
    let mut targets = Vec::new();
    if let Some(doc_id) = payload.doc_id.as_deref() {
        let cleaned = doc_id.trim();
        if !cleaned.is_empty() {
            targets.push(cleaned.to_string());
        }
    }
    if targets.is_empty() {
        let docs =
            vector_knowledge::list_vector_documents(storage.as_ref(), None, &base_name, &root)
                .await
                .map_err(vector_error_response)?;
        targets = docs.into_iter().map(|doc| doc.doc_id).collect();
    }
    let mut reindexed = Vec::new();
    let mut failed = Vec::new();
    for doc_id in targets {
        let meta = match vector_knowledge::read_vector_document_meta(
            storage.as_ref(),
            None,
            &base_name,
            &root,
            &doc_id,
        )
        .await
        {
            Ok(meta) => meta,
            Err(err) => {
                failed.push(json!({ "doc_id": doc_id, "error": err.to_string() }));
                continue;
            }
        };
        let content = match vector_knowledge::read_vector_document_content(
            storage.as_ref(),
            None,
            &base_name,
            &root,
            &doc_id,
        )
        .await
        {
            Ok(content) => content,
            Err(err) => {
                failed.push(json!({ "doc_id": doc_id, "error": err.to_string() }));
                continue;
            }
        };
        match vector_knowledge::index_document(
            &config,
            &base,
            None,
            storage.as_ref(),
            &root,
            &meta.name,
            Some(&meta.doc_id),
            &content,
            Some(&meta),
        )
        .await
        {
            Ok(updated) => reindexed.push(updated.doc_id),
            Err(err) => failed.push(json!({ "doc_id": doc_id, "error": err.to_string() })),
        }
    }
    Ok(Json(json!({
        "ok": failed.is_empty(),
        "reindexed": reindexed,
        "failed": failed
    })))
}

async fn admin_knowledge_upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut base = String::new();
    let mut upload: Option<UploadedKnowledgeFile> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let field_name = field.name().unwrap_or("");
        if field_name == "base" {
            base = field.text().await.unwrap_or_default();
            continue;
        }
        if let Some(previous) = upload.take() {
            let _ = tokio::fs::remove_dir_all(&previous.temp_dir).await;
        }
        upload = Some(save_knowledge_upload_field(field).await?);
    }
    if base.trim().is_empty() {
        if let Some(previous) = upload {
            let _ = tokio::fs::remove_dir_all(&previous.temp_dir).await;
        }
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let upload = match upload {
        Some(value) => value,
        None => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.file_not_found"),
            ))
        }
    };
    let config = state.config_store.get().await;
    let base_config = resolve_knowledge_base(&config, &base)?;
    if base_config.is_vector() {
        let root = resolve_vector_root_for_admin(&base_config, true)?;
        let storage = state.storage.clone();
        let temp_dir = upload.temp_dir.clone();
        let result = convert_upload_to_markdown(&upload).await;
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        let (content, converter, warnings) = result?;
        let doc_name = upload.stem.clone();
        let existing = vector_knowledge::list_vector_documents(
            storage.as_ref(),
            None,
            &base_config.name,
            &root,
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let mut doc_id: Option<String> = None;
        let mut previous_meta = None;
        if let Some(doc) = existing.iter().find(|doc| doc.name == doc_name) {
            doc_id = Some(doc.doc_id.clone());
            previous_meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                None,
                &base_config.name,
                &root,
                &doc.doc_id,
            )
            .await
            .ok();
        }
        let meta = vector_knowledge::index_document(
            &config,
            &base_config,
            None,
            storage.as_ref(),
            &root,
            &doc_name,
            doc_id.as_deref(),
            &content,
            previous_meta.as_ref(),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        return Ok(Json(json!({
            "ok": true,
            "message": i18n::t("message.upload_converted"),
            "doc_id": meta.doc_id,
            "doc_name": meta.name,
            "chunk_count": meta.chunk_count,
            "embedding_model": meta.embedding_model,
            "converter": converter,
            "warnings": warnings
        })));
    }
    let root = resolve_knowledge_root(&base_config, true)?;
    let output_name = build_markdown_output_path(&upload.filename, &upload.stem);
    let target = resolve_knowledge_path(&root, &output_name)?;
    let temp_dir = upload.temp_dir.clone();
    let result = persist_knowledge_upload(&upload, &target).await;
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    let (converter, warnings) = result?;
    cleanup_non_markdown_upload(&root, &upload.filename, &output_name).await;
    knowledge::refresh_knowledge_cache(&KnowledgeBaseConfig {
        name: base_config.name.clone(),
        description: base_config.description.clone(),
        root: root.to_string_lossy().to_string(),
        enabled: base_config.enabled,
        shared: base_config.shared,
        base_type: base_config.base_type.clone(),
        embedding_model: base_config.embedding_model.clone(),
        chunk_size: base_config.chunk_size,
        chunk_overlap: base_config.chunk_overlap,
        top_k: base_config.top_k,
        score_threshold: base_config.score_threshold,
    })
    .await;
    Ok(Json(json!({
        "ok": true,
        "message": i18n::t("message.upload_converted"),
        "path": output_name,
        "converter": converter,
        "warnings": warnings
    })))
}

async fn admin_knowledge_refresh(
    State(state): State<Arc<AppState>>,
    axum::extract::Form(payload): axum::extract::Form<KnowledgeRefreshForm>,
) -> Result<Json<Value>, Response> {
    let base = payload.base.trim();
    if base.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let base_config = resolve_knowledge_base(&config, base)?;
    if base_config.is_vector() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_requires_reindex"),
        ));
    }
    let root = resolve_knowledge_root(&base_config, true)?;
    knowledge::refresh_knowledge_cache(&KnowledgeBaseConfig {
        name: base_config.name.clone(),
        description: base_config.description.clone(),
        root: root.to_string_lossy().to_string(),
        enabled: base_config.enabled,
        shared: base_config.shared,
        base_type: base_config.base_type.clone(),
        embedding_model: base_config.embedding_model.clone(),
        chunk_size: base_config.chunk_size,
        chunk_overlap: base_config.chunk_overlap,
        top_k: base_config.top_k,
        score_threshold: base_config.score_threshold,
    })
    .await;
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.index_refreshed") }),
    ))
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

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn build_system_settings_payload(config: &Config) -> Value {
    let sandbox_enabled = config.sandbox.mode.trim().eq_ignore_ascii_case("sandbox");
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
            "allow_commands": config.security.allow_commands.clone(),
            "allow_paths": config.security.allow_paths.clone(),
            "deny_globs": config.security.deny_globs.clone(),
            "exec_policy_mode": exec_policy_mode,
        },
        "sandbox": {
            "enabled": sandbox_enabled,
            "mode": config.sandbox.mode.clone(),
            "endpoint": config.sandbox.endpoint.clone(),
            "container_root": config.sandbox.container_root.clone(),
            "network": config.sandbox.network.clone(),
            "readonly_rootfs": config.sandbox.readonly_rootfs,
            "idle_ttl_s": config.sandbox.idle_ttl_s,
            "timeout_s": config.sandbox.timeout_s,
            "resources": {
                "cpu": config.sandbox.resources.cpu,
                "memory_mb": config.sandbox.resources.memory_mb,
                "pids": config.sandbox.resources.pids,
            }
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
        || payload.sandbox.is_some()
        || payload.observability.is_some()
        || payload.cors.is_some();
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
            if let Some(sandbox) = payload.sandbox {
                if let Some(enabled) = sandbox.enabled {
                    config.sandbox.mode = if enabled {
                        "sandbox".to_string()
                    } else {
                        "local".to_string()
                    };
                }
                if let Some(endpoint) = sandbox.endpoint {
                    config.sandbox.endpoint = endpoint.trim().to_string();
                }
                if let Some(container_root) = sandbox.container_root {
                    config.sandbox.container_root = container_root.trim().to_string();
                }
                if let Some(network) = sandbox.network {
                    config.sandbox.network = network.trim().to_string();
                }
                if let Some(readonly_rootfs) = sandbox.readonly_rootfs {
                    config.sandbox.readonly_rootfs = readonly_rootfs;
                }
                if let Some(idle_ttl_s) = sandbox.idle_ttl_s {
                    config.sandbox.idle_ttl_s = idle_ttl_s;
                }
                if let Some(timeout_s) = sandbox.timeout_s {
                    config.sandbox.timeout_s = timeout_s;
                }
                if let Some(resources) = sandbox.resources {
                    if let Some(cpu) = resources.cpu {
                        config.sandbox.resources.cpu = cpu;
                    }
                    if let Some(memory_mb) = resources.memory_mb {
                        config.sandbox.resources.memory_mb = memory_mb;
                    }
                    if let Some(pids) = resources.pids {
                        config.sandbox.resources.pids = pids;
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
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(build_system_settings_payload(&updated)))
}

async fn admin_server_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let sandbox_enabled = config.sandbox.mode.trim().eq_ignore_ascii_case("sandbox");
    Ok(Json(json!({
        "server": {
            "max_active_sessions": config.server.max_active_sessions,
            "sandbox_enabled": sandbox_enabled
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
    if payload.max_active_sessions.is_none() && payload.sandbox_enabled.is_none() {
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
            if let Some(sandbox_enabled) = payload.sandbox_enabled {
                config.sandbox.mode = if sandbox_enabled {
                    "sandbox".to_string()
                } else {
                    "local".to_string()
                };
            }
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let sandbox_enabled = updated.sandbox.mode.trim().eq_ignore_ascii_case("sandbox");
    Ok(Json(json!({
        "server": {
            "max_active_sessions": updated.server.max_active_sessions,
            "sandbox_enabled": sandbox_enabled
        }
    })))
}

async fn admin_monitor(
    State(state): State<Arc<AppState>>,
    Query(query): Query<MonitorQuery>,
) -> Result<Json<Value>, Response> {
    state.monitor.warm_history(true);
    let active_only = query.active_only.unwrap_or(true);
    let system = state.monitor.get_system_metrics();
    let sessions = state.monitor.list_sessions(active_only);

    let mut since_time = None;
    let mut until_time = None;
    let mut recent_window_s = None;
    let mut service_now = None;
    let mut start_ts = normalize_ts(query.start_time);
    let mut end_ts = normalize_ts(query.end_time);
    if let (Some(start), Some(end)) = (start_ts, end_ts) {
        if end < start {
            start_ts = Some(end);
            end_ts = Some(start);
        }
    }
    if start_ts.is_some() || end_ts.is_some() {
        since_time = start_ts;
        until_time = end_ts;
        let now = end_ts.unwrap_or_else(now_ts);
        service_now = Some(now);
        if let Some(start) = start_ts {
            recent_window_s = Some((now - start).max(0.0));
        }
    } else if let Some(hours) = query.tool_hours.filter(|value| *value > 0.0) {
        let window = hours * 3600.0;
        recent_window_s = Some(window);
        since_time = Some(now_ts() - window);
    }

    let service = state
        .monitor
        .get_service_metrics(recent_window_s, service_now);
    let tool_stats =
        normalize_tool_stats(state.workspace.get_tool_usage_stats(since_time, until_time));
    Ok(Json(json!({
        "system": system,
        "service": service,
        "sandbox": state.monitor.get_sandbox_metrics(since_time, until_time),
        "sessions": sessions,
        "tool_stats": tool_stats
    })))
}

async fn admin_monitor_tool_usage(
    State(state): State<Arc<AppState>>,
    Query(query): Query<MonitorToolUsageQuery>,
) -> Result<Json<Value>, Response> {
    let cleaned = query.tool.as_deref().unwrap_or("").trim().to_string();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.tool_name_required"),
        ));
    }
    if cleaned.eq_ignore_ascii_case("performance_log") {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.tool_not_found"),
        ));
    }

    let mut since_time = None;
    let mut until_time = None;
    let mut start_ts = normalize_ts(query.start_time);
    let mut end_ts = normalize_ts(query.end_time);
    if let (Some(start), Some(end)) = (start_ts, end_ts) {
        if end < start {
            start_ts = Some(end);
            end_ts = Some(start);
        }
    }
    if start_ts.is_some() || end_ts.is_some() {
        since_time = start_ts;
        until_time = end_ts;
    } else if let Some(hours) = query.tool_hours.filter(|value| *value > 0.0) {
        since_time = Some(now_ts() - hours * 3600.0);
    }

    let canonical = resolve_tool_name(&cleaned);
    let builtin_names = builtin_tool_names();
    let display_map = build_builtin_tool_display_map();
    let mut tool_name = cleaned.clone();
    let usage_records = if builtin_names.contains(&canonical) {
        let mut names = vec![canonical.clone()];
        for (alias, target) in builtin_aliases() {
            if target == canonical && !names.contains(&alias) {
                names.push(alias);
            }
        }
        let mut combined = Vec::new();
        for name in names {
            combined.extend(
                state
                    .workspace
                    .get_tool_session_usage(&name, since_time, until_time),
            );
        }
        tool_name = canonical.clone();
        merge_tool_session_usage(combined)
    } else {
        state
            .workspace
            .get_tool_session_usage(&cleaned, since_time, until_time)
    };

    let display_name = display_map
        .get(&canonical)
        .cloned()
        .unwrap_or_else(|| cleaned.clone());
    let mut session_map = HashMap::new();
    for session in state.monitor.list_sessions(false) {
        if let Some(session_id) = session.get("session_id").and_then(Value::as_str) {
            session_map.insert(session_id.to_string(), session);
        }
    }

    let mut sessions = Vec::new();
    for record in usage_records {
        let session_id = record
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if session_id.is_empty() {
            continue;
        }
        let user_id = record
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let tool_calls = record
            .get("tool_calls")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let last_time = record
            .get("last_time")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let last_time_text = format_ts(last_time);
        let session_info = session_map.get(&session_id);
        let fallback_user = session_info
            .and_then(|value| value.get("user_id").and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_string();
        let final_user = if user_id.is_empty() {
            fallback_user
        } else {
            user_id
        };
        let question = session_info
            .and_then(|value| value.get("question").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();
        let status = session_info
            .and_then(|value| value.get("status").and_then(Value::as_str))
            .unwrap_or("unknown")
            .to_string();
        let stage = session_info
            .and_then(|value| value.get("stage").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();
        let start_time = session_info
            .and_then(|value| value.get("start_time").cloned())
            .unwrap_or(Value::String(String::new()));
        let updated_time = session_info
            .and_then(|value| value.get("updated_time").cloned())
            .unwrap_or(Value::String(last_time_text.clone()));
        let elapsed_s = session_info
            .and_then(|value| value.get("elapsed_s").and_then(Value::as_f64))
            .unwrap_or(0.0);
        let context_tokens = session_info
            .and_then(|value| value.get("context_tokens").and_then(Value::as_i64))
            .unwrap_or(0);
        let context_tokens_peak = session_info
            .and_then(|value| value.get("context_tokens_peak").and_then(Value::as_i64))
            .unwrap_or(context_tokens);
        let prefill_tokens =
            session_info.and_then(|value| value.get("prefill_tokens").and_then(Value::as_i64));
        let prefill_duration_s =
            session_info.and_then(|value| value.get("prefill_duration_s").and_then(Value::as_f64));
        let prefill_speed_tps =
            session_info.and_then(|value| value.get("prefill_speed_tps").and_then(Value::as_f64));
        let prefill_speed_lower_bound = session_info.and_then(|value| {
            value
                .get("prefill_speed_lower_bound")
                .and_then(Value::as_bool)
        });
        let decode_tokens =
            session_info.and_then(|value| value.get("decode_tokens").and_then(Value::as_i64));
        let decode_duration_s =
            session_info.and_then(|value| value.get("decode_duration_s").and_then(Value::as_f64));
        let decode_speed_tps =
            session_info.and_then(|value| value.get("decode_speed_tps").and_then(Value::as_f64));
        sessions.push(json!({
            "session_id": session_id,
            "user_id": final_user,
            "question": question,
            "status": status,
            "stage": stage,
            "start_time": start_time,
            "updated_time": updated_time,
            "elapsed_s": elapsed_s,
            "context_tokens": context_tokens,
            "context_tokens_peak": context_tokens_peak,
            "prefill_tokens": prefill_tokens,
            "prefill_duration_s": prefill_duration_s,
            "prefill_speed_tps": prefill_speed_tps,
            "prefill_speed_lower_bound": prefill_speed_lower_bound,
            "decode_tokens": decode_tokens,
            "decode_duration_s": decode_duration_s,
            "decode_speed_tps": decode_speed_tps,
            "tool_calls": tool_calls,
            "last_time": last_time_text
        }));
    }

    Ok(Json(json!({
        "tool": display_name,
        "tool_name": tool_name,
        "sessions": sessions
    })))
}

async fn admin_monitor_detail(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let detail = state.monitor.get_detail(&session_id);
    match detail {
        Some(value) => Ok(Json(value)),
        None => Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.session_not_found"),
        )),
    }
}

async fn admin_monitor_cancel(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let ok = state.monitor.cancel(&session_id);
    if !ok {
        return Ok(Json(json!({
            "ok": false,
            "message": i18n::t("error.session_not_found_or_finished")
        })));
    }
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.cancel_requested") }),
    ))
}

async fn admin_monitor_compaction(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
    Json(payload): Json<MonitorCompactionRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = session_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    state.monitor.warm_history(false);
    let record = state
        .monitor
        .get_record(cleaned)
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.session_not_found")))?;
    let status = record.get("status").and_then(Value::as_str).unwrap_or("");
    if status == crate::monitor::MonitorState::STATUS_RUNNING
        || status == crate::monitor::MonitorState::STATUS_CANCELLING
    {
        return Err(error_response(
            StatusCode::CONFLICT,
            i18n::t("error.session_not_found_or_running"),
        ));
    }
    let user_id = record
        .get("user_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let session_record = state
        .user_store
        .get_chat_session(&user_id, cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let agent_id = session_record
        .as_ref()
        .and_then(|record| record.agent_id.clone());
    let agent_prompt = agent_id
        .as_deref()
        .and_then(|agent_id| {
            state
                .user_store
                .get_user_agent_by_id(agent_id)
                .ok()
                .flatten()
        })
        .and_then(|record| {
            let prompt = record.system_prompt.trim();
            if prompt.is_empty() {
                None
            } else {
                Some(prompt.to_string())
            }
        });
    let is_admin = state
        .user_store
        .get_user_by_id(&user_id)
        .ok()
        .flatten()
        .map(|user| UserStore::is_admin(&user))
        .unwrap_or(false);
    state
        .orchestrator
        .force_compact_session(
            &user_id,
            cleaned,
            is_admin,
            payload.model_name.as_deref(),
            agent_id.as_deref(),
            agent_prompt.as_deref(),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.updated") }),
    ))
}

async fn admin_monitor_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = session_id.trim();
    let user_id = state.monitor.get_record(cleaned).and_then(|record| {
        record
            .get("user_id")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    if let Some(user_id) = user_id {
        state.workspace.purge_session_data(&user_id, cleaned);
        let _ = state.memory.delete_record(&user_id, cleaned);
        let _ = state.user_store.delete_chat_session(&user_id, cleaned);
    }
    let ok = state.monitor.purge_session(cleaned);
    if !ok {
        return Ok(Json(json!({
            "ok": false,
            "message": i18n::t("error.session_not_found")
        })));
    }
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.deleted") }),
    ))
}

async fn admin_throughput_start(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ThroughputStartRequest>,
) -> Result<Json<ThroughputSnapshot>, Response> {
    let config = ThroughputConfig::new(
        payload.concurrency_list,
        payload.user_id_prefix,
        payload.model_name,
        payload.request_timeout_s,
        payload.max_tokens,
    )
    .map_err(|message| error_response(StatusCode::BAD_REQUEST, message))?;
    let snapshot = state
        .throughput
        .start(state.orchestrator.clone(), state.monitor.clone(), config)
        .await
        .map_err(|message| error_response(StatusCode::CONFLICT, message))?;
    Ok(Json(snapshot))
}

async fn admin_throughput_stop(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ThroughputSnapshot>, Response> {
    let snapshot = state
        .throughput
        .stop()
        .await
        .map_err(|message| error_response(StatusCode::BAD_REQUEST, message))?;
    Ok(Json(snapshot))
}

async fn admin_throughput_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ThroughputStatusResponse>, Response> {
    Ok(Json(state.throughput.status().await))
}

async fn admin_throughput_report(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ThroughputReportQuery>,
) -> Result<Json<ThroughputReport>, Response> {
    let report = state
        .throughput
        .report(query.run_id.as_deref())
        .await
        .map_err(|message| error_response(StatusCode::NOT_FOUND, message))?;
    Ok(Json(report))
}

async fn admin_performance_sample(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PerformanceSampleRequest>,
) -> Result<Json<PerformanceSampleResponse>, Response> {
    let response = run_performance_sample(state, payload)
        .await
        .map_err(|message| error_response(StatusCode::BAD_REQUEST, message))?;
    Ok(Json(response))
}

async fn admin_org_units_list(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
) -> Result<Json<Value>, Response> {
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let filtered = filter_units_by_scope(units, actor.scope_unit_ids.as_ref());
    let tree = org_units::build_unit_tree(&filtered);
    let items = filtered.iter().map(org_unit_payload).collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items, "tree": tree } })))
}

async fn admin_org_units_create(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<OrgUnitCreateRequest>,
) -> Result<Json<Value>, Response> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let parent_id = normalize_optional_id(payload.parent_id.as_deref());
    if actor.scope_unit_ids.is_some() {
        let Some(parent_id) = parent_id.as_deref() else {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.org_unit_parent_required"),
            ));
        };
        ensure_unit_scope(&actor, Some(parent_id))?;
    }
    let parent = parent_id
        .as_ref()
        .and_then(|id| units.iter().find(|unit| unit.unit_id == *id));
    if parent_id.is_some() && parent.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.org_unit_not_found"),
        ));
    }
    let level = parent.map_or(1, |parent| parent.level + 1);
    if level > MAX_ORG_UNIT_LEVEL {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.org_unit_level_exceeded"),
        ));
    }
    let sort_order = payload
        .sort_order
        .unwrap_or_else(|| next_unit_sort_order(&units, parent_id.as_deref()));
    let leader_ids = normalize_leader_ids(payload.leader_ids);
    let unit_id = format!("unit_{}", Uuid::new_v4().simple());
    let path = parent
        .map(|parent| format!("{}/{}", parent.path, unit_id))
        .unwrap_or_else(|| unit_id.clone());
    let path_name = parent
        .map(|parent| format!("{}{}{}", parent.path_name, ORG_UNIT_NAME_SEPARATOR, name))
        .unwrap_or_else(|| name.to_string());
    let now = now_ts();
    let record = OrgUnitRecord {
        unit_id: unit_id.clone(),
        parent_id: parent_id.clone(),
        name: name.to_string(),
        level,
        path,
        path_name,
        sort_order,
        leader_ids,
        created_at: now,
        updated_at: now,
    };
    state
        .user_store
        .upsert_org_unit(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": org_unit_payload(&record) })))
}

async fn admin_org_units_update(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(unit_id): AxumPath<String>,
    Json(payload): Json<OrgUnitUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = unit_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    let mut units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let target_index = units
        .iter()
        .position(|unit| unit.unit_id == cleaned)
        .ok_or_else(|| {
            error_response(StatusCode::NOT_FOUND, i18n::t("error.org_unit_not_found"))
        })?;
    let target = units[target_index].clone();
    ensure_unit_scope(&actor, Some(&target.unit_id))?;

    let name = payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(target.name.as_str())
        .to_string();
    let parent_override = if payload.parent_id.is_some() {
        normalize_optional_id(payload.parent_id.as_deref())
    } else {
        None
    };
    let parent_id = if payload.parent_id.is_some() {
        parent_override
    } else {
        target.parent_id.clone()
    };
    if parent_id.as_deref() == Some(cleaned) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.org_unit_cycle_not_allowed"),
        ));
    }
    if actor.scope_unit_ids.is_some() {
        if parent_id.is_none() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.org_unit_parent_required"),
            ));
        }
        ensure_unit_scope(&actor, parent_id.as_deref())?;
    }
    let parent = parent_id
        .as_ref()
        .and_then(|id| units.iter().find(|unit| unit.unit_id == *id));
    if parent_id.is_some() && parent.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.org_unit_not_found"),
        ));
    }
    if let Some(parent) = parent {
        if parent.path == target.path || parent.path.starts_with(&format!("{}/", target.path)) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.org_unit_cycle_not_allowed"),
            ));
        }
    }

    let parent_changed = parent_id != target.parent_id;
    let name_changed = name != target.name;
    let sort_order = if let Some(sort_order) = payload.sort_order {
        sort_order
    } else if parent_changed {
        next_unit_sort_order(&units, parent_id.as_deref())
    } else {
        target.sort_order
    };
    let leader_ids = if payload.leader_ids.is_some() {
        normalize_leader_ids(payload.leader_ids)
    } else {
        target.leader_ids.clone()
    };

    let now = now_ts();
    let mut updated_units = Vec::new();

    if parent_changed || name_changed {
        let old_path = target.path.clone();
        let old_path_name = target.path_name.clone();
        let old_level = target.level;
        let (new_level, new_path, new_path_name) = match parent {
            Some(parent) => {
                let level = parent.level + 1;
                if level > MAX_ORG_UNIT_LEVEL {
                    return Err(error_response(
                        StatusCode::BAD_REQUEST,
                        i18n::t("error.org_unit_level_exceeded"),
                    ));
                }
                (
                    level,
                    format!("{}/{}", parent.path, target.unit_id),
                    format!("{}{}{}", parent.path_name, ORG_UNIT_NAME_SEPARATOR, name),
                )
            }
            None => (1, target.unit_id.clone(), name.clone()),
        };
        let level_delta = new_level - old_level;
        let max_level = units
            .iter()
            .filter(|unit| unit.path == old_path || unit.path.starts_with(&format!("{old_path}/")))
            .map(|unit| unit.level)
            .max()
            .unwrap_or(old_level);
        if max_level + level_delta > MAX_ORG_UNIT_LEVEL {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.org_unit_level_exceeded"),
            ));
        }
        for unit in units.iter_mut() {
            if unit.path == old_path || unit.path.starts_with(&format!("{old_path}/")) {
                let suffix = unit.path.strip_prefix(&old_path).unwrap_or("");
                let suffix_name = unit.path_name.strip_prefix(&old_path_name).unwrap_or("");
                unit.path = format!("{new_path}{suffix}");
                unit.path_name = format!("{new_path_name}{suffix_name}");
                unit.level = (unit.level + level_delta).max(1);
                unit.updated_at = now;
                if unit.unit_id == cleaned {
                    unit.name = name.clone();
                    unit.parent_id = parent_id.clone();
                    unit.sort_order = sort_order;
                    unit.leader_ids = leader_ids.clone();
                }
                updated_units.push(unit.clone());
            }
        }
    } else {
        let mut updated = target.clone();
        updated.sort_order = sort_order;
        updated.leader_ids = leader_ids;
        updated.updated_at = now;
        updated_units.push(updated);
    }

    for unit in &updated_units {
        state
            .user_store
            .upsert_org_unit(unit)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let response_unit = updated_units
        .iter()
        .find(|unit| unit.unit_id == cleaned)
        .cloned()
        .unwrap_or_else(|| target.clone());
    Ok(Json(json!({ "data": org_unit_payload(&response_unit) })))
}

async fn admin_org_units_delete(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(unit_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = unit_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_unit_scope(&actor, Some(cleaned))?;
    if units
        .iter()
        .any(|unit| unit.parent_id.as_deref() == Some(cleaned))
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.org_unit_has_children"),
        ));
    }
    let unit_ids = vec![cleaned.to_string()];
    let (users, _) = state
        .user_store
        .list_users(None, Some(&unit_ids), 0, 1)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !users.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.org_unit_has_users"),
        ));
    }
    state
        .user_store
        .delete_org_unit(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "unit_id": cleaned } })))
}

async fn admin_external_links_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, Response> {
    let records = state
        .storage
        .list_external_links(true)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .iter()
        .map(external_link_payload)
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_external_links_upsert(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ExternalLinkUpsertRequest>,
) -> Result<Json<Value>, Response> {
    let title = payload.title.trim();
    let description = payload.description.trim();
    let url = payload.url.trim();
    if title.is_empty() || url.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let parsed_url = Url::parse(url)
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "外链 URL 格式无效".to_string()))?;
    if parsed_url.scheme() != "http" && parsed_url.scheme() != "https" {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "外链 URL 仅支持 http/https".to_string(),
        ));
    }
    let link_id = payload
        .link_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("ext_{}", Uuid::new_v4().simple()));
    let now = now_ts();
    let existing = state
        .storage
        .get_external_link(&link_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let created_at = existing
        .as_ref()
        .map(|record| record.created_at)
        .unwrap_or(now);
    let allowed_levels = normalize_external_link_levels(payload.allowed_levels.unwrap_or_default());
    let sort_order = payload.sort_order.unwrap_or(0);
    let icon = normalize_external_link_icon(payload.icon.as_deref());
    let record = ExternalLinkRecord {
        link_id: link_id.clone(),
        title: title.to_string(),
        description: description.to_string(),
        url: parsed_url.to_string(),
        icon,
        allowed_levels,
        sort_order,
        enabled: payload.enabled.unwrap_or(true),
        created_at,
        updated_at: now,
    };
    state
        .storage
        .upsert_external_link(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": external_link_payload(&record) })))
}

async fn admin_external_links_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(link_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = link_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    state
        .storage
        .delete_external_link(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "link_id": cleaned } })))
}

async fn admin_user_accounts_list(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Query(query): Query<UserAccountListQuery>,
) -> Result<Json<Value>, Response> {
    let keyword = query
        .keyword
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let scoped_unit_ids = actor.scope_unit_ids.as_ref().map(|set| {
        let mut items = set.iter().cloned().collect::<Vec<_>>();
        items.sort();
        items
    });
    let (users, total) = state
        .user_store
        .list_users(keyword, scoped_unit_ids.as_deref(), offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let today = UserStore::today_string();
    let active_sessions = state.monitor.list_sessions(true);
    let mut active_map: HashMap<String, i64> = HashMap::new();
    for session in active_sessions {
        let user_id = session
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if user_id.is_empty() {
            continue;
        }
        let entry = active_map.entry(user_id.to_string()).or_insert(0);
        *entry += 1;
    }
    let unit_map = build_unit_map(&units);
    let items = users
        .into_iter()
        .map(|user| {
            let unit = user
                .unit_id
                .as_ref()
                .and_then(|unit_id| unit_map.get(unit_id));
            let profile = UserStore::to_profile_with_unit(&user, unit);
            let active_count = active_map.get(&profile.id).copied().unwrap_or(0);
            let quota_total = user.daily_quota.max(0);
            let quota_used = if user.daily_quota_date.as_deref() == Some(today.as_str()) {
                user.daily_quota_used.max(0)
            } else {
                0
            };
            let quota_remaining = (quota_total - quota_used).max(0);
            let mut value = serde_json::to_value(profile).unwrap_or_else(|_| json!({}));
            if let Value::Object(ref mut map) = value {
                map.insert("active_sessions".to_string(), json!(active_count));
                map.insert("online".to_string(), json!(active_count > 0));
                map.insert("daily_quota".to_string(), json!(quota_total));
                map.insert("daily_quota_used".to_string(), json!(quota_used));
                map.insert("daily_quota_remaining".to_string(), json!(quota_remaining));
                map.insert("daily_quota_date".to_string(), json!(today));
            }
            value
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "total": total, "items": items } })))
}

async fn admin_user_accounts_create(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<UserAccountCreateRequest>,
) -> Result<Json<Value>, Response> {
    let username = payload.username.trim();
    let password = payload.password.trim();
    if username.is_empty() || password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let unit_id = normalize_optional_id(payload.unit_id.as_deref());
    if actor.scope_unit_ids.is_some() {
        let Some(unit_id) = unit_id.as_deref() else {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.org_unit_required"),
            ));
        };
        ensure_unit_scope(&actor, Some(unit_id))?;
    }
    if let Some(unit_id) = unit_id.as_deref() {
        let exists = units.iter().any(|unit| unit.unit_id == unit_id);
        if !exists {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.org_unit_not_found"),
            ));
        }
    }
    let status = normalize_user_status(payload.status.as_deref());
    let roles = normalize_user_roles(payload.roles);
    let email = normalize_user_email(payload.email);
    let record = state
        .user_store
        .create_user(
            username,
            email,
            password,
            None,
            unit_id,
            roles,
            &status,
            payload.is_demo,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let unit = record
        .unit_id
        .as_ref()
        .and_then(|unit_id| units.iter().find(|unit| unit.unit_id == *unit_id));
    Ok(Json(json!({
        "data": UserStore::to_profile_with_unit(&record, unit)
    })))
}

async fn admin_user_accounts_seed(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<UserAccountSeedRequest>,
) -> Result<Json<Value>, Response> {
    let per_unit = payload.per_unit.unwrap_or(0);
    if per_unit <= 0 || per_unit > MAX_TEST_USERS_PER_UNIT {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_seed_count_invalid"),
        ));
    }
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let scoped_units = filter_units_by_scope(units, actor.scope_unit_ids.as_ref());
    let password_hash = UserStore::hash_password(DEFAULT_TEST_USER_PASSWORD)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let now = now_ts();
    let access_level = UserStore::normalize_access_level(None);
    let capacity = scoped_units.len().saturating_mul(per_unit.max(0) as usize);
    let mut records = Vec::with_capacity(capacity);
    for unit in &scoped_units {
        let daily_quota = UserStore::default_daily_quota_by_level(Some(unit.level));
        for _ in 0..per_unit {
            let username = format!(
                "{DEFAULT_TEST_USER_PREFIX}_{unit_id}_{}",
                Uuid::new_v4().simple(),
                unit_id = unit.unit_id
            );
            records.push(UserAccountRecord {
                user_id: username.clone(),
                username,
                email: None,
                password_hash: password_hash.clone(),
                roles: vec!["user".to_string()],
                status: "active".to_string(),
                access_level: access_level.clone(),
                unit_id: Some(unit.unit_id.clone()),
                daily_quota,
                daily_quota_used: 0,
                daily_quota_date: None,
                is_demo: true,
                created_at: now,
                updated_at: now,
                last_login_at: None,
            });
        }
    }
    state
        .user_store
        .upsert_users(&records)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let created = records.len() as i64;
    Ok(Json(json!({
        "data": {
            "created": created,
            "unit_count": scoped_units.len(),
            "per_unit": per_unit,
            "password": DEFAULT_TEST_USER_PASSWORD,
        }
    })))
}

async fn admin_user_accounts_update(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
    Json(payload): Json<UserAccountUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let mut record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let unit_map = build_unit_map(&units);
    let previous_level = record
        .unit_id
        .as_ref()
        .and_then(|unit_id| unit_map.get(unit_id))
        .map(|unit| unit.level);
    if let Some(email) = payload.email {
        record.email = normalize_user_email(Some(email));
    }
    if let Some(status) = payload.status {
        record.status = normalize_user_status(Some(&status));
    }
    if payload.unit_id.is_some() {
        let next_unit_id = normalize_optional_id(payload.unit_id.as_deref());
        if actor.scope_unit_ids.is_some() {
            let Some(unit_id) = next_unit_id.as_deref() else {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.org_unit_required"),
                ));
            };
            ensure_unit_scope(&actor, Some(unit_id))?;
        }
        if let Some(unit_id) = next_unit_id.as_deref() {
            if !unit_map.contains_key(unit_id) {
                return Err(error_response(
                    StatusCode::NOT_FOUND,
                    i18n::t("error.org_unit_not_found"),
                ));
            }
        }
        if next_unit_id != record.unit_id {
            let previous_default = UserStore::default_daily_quota_by_level(previous_level);
            record.unit_id = next_unit_id;
            let next_level = record
                .unit_id
                .as_ref()
                .and_then(|unit_id| unit_map.get(unit_id))
                .map(|unit| unit.level);
            if payload.daily_quota.is_none() && record.daily_quota == previous_default {
                record.daily_quota = UserStore::default_daily_quota_by_level(next_level);
            }
        }
    }
    if let Some(roles) = payload.roles {
        record.roles = normalize_user_roles(roles);
    }
    if let Some(daily_quota) = payload.daily_quota {
        record.daily_quota = daily_quota.max(0);
    }
    record.updated_at = now_ts();
    state
        .user_store
        .update_user(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let unit = record
        .unit_id
        .as_ref()
        .and_then(|unit_id| unit_map.get(unit_id));
    Ok(Json(json!({
        "data": UserStore::to_profile_with_unit(&record, unit)
    })))
}

async fn admin_user_accounts_reset_password(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
    Json(payload): Json<UserAccountPasswordResetRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let password = payload.password.trim();
    if password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    state
        .user_store
        .set_password(cleaned, password)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.updated") }),
    ))
}

async fn admin_user_accounts_tool_access_get(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let allowed = state
        .user_store
        .get_user_tool_access(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let allowed_tools = allowed
        .as_ref()
        .and_then(|record| record.allowed_tools.clone());
    Ok(Json(json!({
        "data": { "allowed_tools": allowed_tools }
    })))
}

async fn admin_user_accounts_tool_access_update(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
    Json(payload): Json<UserAccountToolAccessRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let allowed = payload.allowed_tools.map(normalize_tool_access_list);
    state
        .user_store
        .set_user_tool_access(cleaned, allowed.as_ref())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": { "allowed_tools": allowed }
    })))
}

async fn admin_user_accounts_agent_access_get(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let access = state
        .user_store
        .get_user_agent_access(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let allowed_agent_ids = access
        .as_ref()
        .and_then(|record| record.allowed_agent_ids.clone());
    let blocked_agent_ids = access
        .as_ref()
        .map(|record| record.blocked_agent_ids.clone())
        .unwrap_or_default();
    Ok(Json(json!({
        "data": { "allowed_agent_ids": allowed_agent_ids, "blocked_agent_ids": blocked_agent_ids }
    })))
}

async fn admin_user_accounts_agent_access_update(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
    Json(payload): Json<UserAccountAgentAccessRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let allowed = payload.allowed_agent_ids.map(normalize_tool_access_list);
    let blocked = payload.blocked_agent_ids.map(normalize_tool_access_list);
    state
        .user_store
        .set_user_agent_access(cleaned, allowed.as_ref(), blocked.as_ref())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": { "allowed_agent_ids": allowed, "blocked_agent_ids": blocked }
    })))
}

async fn admin_user_accounts_delete(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    if UserStore::is_default_admin(cleaned) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.user_protected"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let deleted_user = state
        .user_store
        .delete_user(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let _ = state.user_store.set_user_tool_access(cleaned, None);
    let _ = state.user_store.set_user_agent_access(cleaned, None, None);
    let monitor_result = state.monitor.purge_user_sessions(cleaned);
    let purge_result = state.workspace.purge_user_data(cleaned);
    let tool_root = state.user_tool_store.get_user_dir(cleaned);
    let tool_dir_deleted = std::fs::remove_dir_all(&tool_root).is_ok();
    Ok(Json(json!({
        "ok": true,
        "message": i18n::t("message.user_deleted"),
        "deleted_user": deleted_user,
        "cancelled_sessions": monitor_result.get("cancelled").copied().unwrap_or(0),
        "deleted_sessions": monitor_result.get("deleted").copied().unwrap_or(0),
        "deleted_chat_records": purge_result.chat_records,
        "deleted_tool_records": purge_result.tool_records,
        "workspace_deleted": purge_result.workspace_deleted,
        "legacy_history_deleted": purge_result.legacy_history_deleted,
        "user_tools_deleted": tool_dir_deleted
    })))
}

async fn admin_users(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    #[derive(Default)]
    struct UserStats {
        active_sessions: i64,
        history_sessions: i64,
        total_sessions: i64,
        context_tokens: i64,
        chat_records: i64,
        tool_calls: i64,
        agent_ids: HashSet<String>,
    }

    state.monitor.warm_history(true);
    let sessions = state.monitor.list_sessions(false);
    let usage_stats = state.workspace.get_user_usage_stats();
    let active_statuses = HashSet::from(["running", "cancelling"]);
    let mut summary: HashMap<String, UserStats> = HashMap::new();

    for session in sessions {
        let user_id = session
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if user_id.is_empty() {
            continue;
        }
        let entry = summary.entry(user_id.to_string()).or_default();
        entry.total_sessions += 1;
        entry.context_tokens += session
            .get("context_tokens_peak")
            .or_else(|| session.get("context_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let agent_id = session
            .get("agent_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if agent_id.is_empty() {
            entry.agent_ids.insert("__default__".to_string());
        } else {
            entry.agent_ids.insert(agent_id.to_string());
        }
        let status = session.get("status").and_then(Value::as_str).unwrap_or("");
        if active_statuses.contains(status) {
            entry.active_sessions += 1;
        } else {
            entry.history_sessions += 1;
        }
    }

    for (user_id, stats) in usage_stats {
        let entry = summary.entry(user_id).or_default();
        entry.chat_records = *stats.get("chat_records").unwrap_or(&0);
        entry.tool_calls = *stats.get("tool_records").unwrap_or(&0);
    }

    for (user_id, stats) in summary.iter_mut() {
        if let Ok(agent_ids) = state.user_store.list_chat_session_agent_ids(user_id) {
            for agent_id in agent_ids {
                let agent_id = agent_id.trim();
                if !agent_id.is_empty() {
                    stats.agent_ids.insert(agent_id.to_string());
                }
            }
        }
        if let Ok(agents) = state.user_store.list_user_agents(user_id) {
            for agent in agents {
                let agent_id = agent.agent_id.trim();
                if !agent_id.is_empty() {
                    stats.agent_ids.insert(agent_id.to_string());
                }
            }
        }
        stats.agent_ids.insert("__default__".to_string());
    }

    let mut users = summary
        .into_iter()
        .map(|(user_id, stats)| {
            let agent_count = stats.agent_ids.len() as i64;
            json!({
                "user_id": user_id,
                "active_sessions": stats.active_sessions,
                "history_sessions": stats.history_sessions,
                "total_sessions": stats.total_sessions,
                "chat_records": stats.chat_records,
                "tool_calls": stats.tool_calls,
                "context_tokens": stats.context_tokens,
                "agent_count": agent_count
            })
        })
        .collect::<Vec<_>>();
    users.sort_by(|a, b| {
        let left_active = a
            .get("active_sessions")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let right_active = b
            .get("active_sessions")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let left_total = a.get("total_sessions").and_then(Value::as_i64).unwrap_or(0);
        let right_total = b.get("total_sessions").and_then(Value::as_i64).unwrap_or(0);
        let left_id = a.get("user_id").and_then(Value::as_str).unwrap_or("");
        let right_id = b.get("user_id").and_then(Value::as_str).unwrap_or("");
        right_active
            .cmp(&left_active)
            .then_with(|| right_total.cmp(&left_total))
            .then_with(|| left_id.cmp(right_id))
    });
    Ok(Json(json!({ "users": users })))
}

async fn admin_user_sessions(
    State(state): State<Arc<AppState>>,
    AxumPath(user_id): AxumPath<String>,
    Query(query): Query<UserSessionsQuery>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let active_only = query.active_only.unwrap_or(false);
    let sessions = state
        .monitor
        .list_sessions(active_only)
        .into_iter()
        .filter(|session| session.get("user_id").and_then(Value::as_str) == Some(cleaned))
        .collect::<Vec<_>>();
    Ok(Json(json!({ "user_id": cleaned, "sessions": sessions })))
}

async fn admin_user_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    if UserStore::is_default_admin(cleaned) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.user_protected"),
        ));
    }
    let monitor_result = state.monitor.purge_user_sessions(cleaned);
    let purge_result = state.workspace.purge_user_data(cleaned);
    Ok(Json(json!({
        "ok": true,
        "message": i18n::t("message.user_deleted"),
        "cancelled_sessions": monitor_result.get("cancelled").copied().unwrap_or(0),
        "deleted_sessions": monitor_result.get("deleted").copied().unwrap_or(0),
        "deleted_chat_records": purge_result.chat_records,
        "deleted_tool_records": purge_result.tool_records,
        "workspace_deleted": purge_result.workspace_deleted,
        "legacy_history_deleted": purge_result.legacy_history_deleted
    })))
}

fn normalize_throughput_prefix(prefix: Option<String>) -> String {
    let fallback = "throughput_user";
    let cleaned = prefix
        .as_deref()
        .unwrap_or(fallback)
        .trim()
        .trim_end_matches('-');
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

fn is_throughput_user(user_id: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return false;
    }
    let cleaned = user_id.trim();
    if cleaned.len() <= prefix.len() {
        return false;
    }
    cleaned.starts_with(prefix) && cleaned.as_bytes().get(prefix.len()) == Some(&b'-')
}

async fn admin_users_cleanup_throughput(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ThroughputUserCleanupRequest>,
) -> Result<Json<Value>, Response> {
    let prefix = normalize_throughput_prefix(payload.prefix);
    state.monitor.warm_history(true);
    let mut user_ids = HashSet::new();
    for session in state.monitor.list_sessions(false) {
        if let Some(user_id) = session.get("user_id").and_then(Value::as_str) {
            let cleaned = user_id.trim();
            if !cleaned.is_empty() {
                user_ids.insert(cleaned.to_string());
            }
        }
    }
    let usage_stats = state.workspace.get_user_usage_stats();
    user_ids.extend(usage_stats.keys().cloned());
    let mut throughput_users = user_ids
        .into_iter()
        .filter(|user_id| is_throughput_user(user_id, &prefix))
        .collect::<Vec<_>>();
    throughput_users.sort();

    let mut cancelled_sessions = 0;
    let mut deleted_sessions = 0;
    let mut deleted_storage = 0;
    let mut deleted_chat_records = 0;
    let mut deleted_tool_records = 0;
    let mut workspace_deleted = 0;
    for user_id in &throughput_users {
        let monitor_result = state.monitor.purge_user_sessions(user_id);
        cancelled_sessions += monitor_result.get("cancelled").copied().unwrap_or(0);
        deleted_sessions += monitor_result.get("deleted").copied().unwrap_or(0);
        deleted_storage += monitor_result.get("deleted_storage").copied().unwrap_or(0);
        let purge_result = state.workspace.purge_user_data(user_id);
        deleted_chat_records += purge_result.chat_records;
        deleted_tool_records += purge_result.tool_records;
        if purge_result.workspace_deleted {
            workspace_deleted += 1;
        }
    }

    Ok(Json(json!({
        "ok": true,
        "prefix": prefix,
        "users": throughput_users.len(),
        "cancelled_sessions": cancelled_sessions,
        "deleted_sessions": deleted_sessions,
        "deleted_storage": deleted_storage,
        "deleted_chat_records": deleted_chat_records,
        "deleted_tool_records": deleted_tool_records,
        "workspace_deleted": workspace_deleted
    })))
}

async fn admin_memory_users(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    state.monitor.warm_history(true);
    let sessions = state.monitor.list_sessions(false);
    let mut user_ids = HashSet::new();
    for session in sessions {
        if let Some(user_id) = session.get("user_id").and_then(Value::as_str) {
            let cleaned = user_id.trim();
            if !cleaned.is_empty() {
                user_ids.insert(cleaned.to_string());
            }
        }
    }

    let settings = state.memory.list_settings();
    let record_stats = state.memory.list_record_stats();
    user_ids.extend(settings.keys().cloned());
    user_ids.extend(record_stats.keys().cloned());

    let mut users = Vec::new();
    let mut sorted_ids = user_ids.into_iter().collect::<Vec<_>>();
    sorted_ids.sort();
    for user_id in sorted_ids {
        let setting = settings.get(&user_id);
        let stats = record_stats.get(&user_id);
        let last_time = stats.map(|item| item.last_time).unwrap_or(0.0);
        users.push(json!({
            "user_id": user_id,
            "enabled": setting.map(|item| item.enabled).unwrap_or(false),
            "record_count": stats.map(|item| item.record_count).unwrap_or(0),
            "last_updated_time": format_ts(last_time),
            "last_updated_time_ts": last_time
        }));
    }
    Ok(Json(json!({ "users": users })))
}

async fn admin_memory_status(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let status = state.orchestrator.get_memory_queue_status().await;
    Ok(Json(status))
}

async fn admin_memory_status_detail(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = task_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.task_id_required"),
        ));
    }
    let detail = state.orchestrator.get_memory_queue_detail(cleaned).await;
    match detail {
        Some(value) => Ok(Json(value)),
        None => Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.task_not_found"),
        )),
    }
}

async fn admin_memory_records(
    State(state): State<Arc<AppState>>,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let enabled = state.memory.is_enabled(cleaned);
    let is_admin = state
        .user_store
        .get_user_by_id(cleaned)
        .ok()
        .flatten()
        .map(|user| UserStore::is_admin(&user))
        .unwrap_or(false);
    let records = state
        .memory
        .list_records(cleaned, if is_admin { Some(0) } else { None }, true);
    let output = records
        .into_iter()
        .map(|record| {
            json!({
                "session_id": record.session_id,
                "summary": record.summary,
                "created_time": format_ts(record.created_time),
                "updated_time": format_ts(record.updated_time),
                "created_time_ts": record.created_time,
                "updated_time_ts": record.updated_time
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(
        json!({ "user_id": cleaned, "enabled": enabled, "records": output }),
    ))
}

async fn admin_memory_clear(
    State(state): State<Arc<AppState>>,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let deleted = state.memory.clear_records(cleaned);
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.cleared"), "deleted": deleted }),
    ))
}

async fn admin_memory_enabled(
    State(state): State<Arc<AppState>>,
    AxumPath(user_id): AxumPath<String>,
    Json(payload): Json<MemoryEnabledRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    state.memory.set_enabled(cleaned, payload.enabled);
    Ok(Json(
        json!({ "user_id": cleaned, "enabled": payload.enabled }),
    ))
}

async fn admin_memory_update(
    State(state): State<Arc<AppState>>,
    AxumPath((user_id, session_id)): AxumPath<(String, String)>,
    Json(payload): Json<MemoryUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    let cleaned_session = session_id.trim();
    let summary = payload.summary.trim();
    if cleaned.is_empty() || cleaned_session.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    if summary.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let ok = state
        .memory
        .update_record(cleaned, cleaned_session, summary, Some(now_ts()));
    if !ok {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.updated") }),
    ))
}

async fn admin_memory_delete(
    State(state): State<Arc<AppState>>,
    AxumPath((user_id, session_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    let cleaned_session = session_id.trim();
    if cleaned.is_empty() || cleaned_session.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    let deleted = state.memory.delete_record(cleaned, cleaned_session);
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.deleted"), "deleted": deleted }),
    ))
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn normalize_ts(value: Option<f64>) -> Option<f64> {
    value.filter(|ts| *ts > 0.0)
}

fn format_ts(ts: f64) -> String {
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

struct AdminActor {
    scope_unit_ids: Option<HashSet<String>>,
}

fn resolve_admin_actor(
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

fn ensure_unit_scope(actor: &AdminActor, unit_id: Option<&str>) -> Result<(), Response> {
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

fn ensure_user_scope(actor: &AdminActor, record: &UserAccountRecord) -> Result<(), Response> {
    ensure_unit_scope(actor, record.unit_id.as_deref())
}

fn filter_units_by_scope(
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

fn build_unit_map(units: &[OrgUnitRecord]) -> HashMap<String, OrgUnitRecord> {
    units
        .iter()
        .map(|unit| (unit.unit_id.clone(), unit.clone()))
        .collect()
}

fn org_unit_payload(record: &OrgUnitRecord) -> Value {
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

fn external_link_payload(record: &ExternalLinkRecord) -> Value {
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

fn normalize_external_link_levels(levels: Vec<i32>) -> Vec<i32> {
    let mut items = levels
        .into_iter()
        .filter(|level| (1..=MAX_ORG_UNIT_LEVEL).contains(level))
        .collect::<Vec<_>>();
    items.sort_unstable();
    items.dedup();
    items
}

fn normalize_external_link_icon(raw: Option<&str>) -> String {
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

fn normalize_optional_id(raw: Option<&str>) -> Option<String> {
    raw.map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn normalize_leader_ids(raw: Option<Vec<String>>) -> Vec<String> {
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

fn next_unit_sort_order(units: &[OrgUnitRecord], parent_id: Option<&str>) -> i64 {
    units
        .iter()
        .filter(|unit| unit.parent_id.as_deref() == parent_id)
        .map(|unit| unit.sort_order)
        .max()
        .unwrap_or(-1)
        + 1
}

fn permission_denied() -> Response {
    error_response(StatusCode::FORBIDDEN, i18n::t("error.permission_denied"))
}

fn normalize_user_status(value: Option<&str>) -> String {
    let cleaned = value.unwrap_or("active").trim();
    if cleaned.is_empty() {
        "active".to_string()
    } else {
        cleaned.to_string()
    }
}

fn normalize_user_roles(raw: Vec<String>) -> Vec<String> {
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

fn normalize_user_email(value: Option<String>) -> Option<String> {
    value.and_then(|email| {
        let cleaned = email.trim();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.to_string())
        }
    })
}

fn normalize_tool_access_list(raw: Vec<String>) -> Vec<String> {
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

fn builtin_tool_names() -> HashSet<String> {
    builtin_tool_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect()
}

fn build_builtin_aliases_by_name() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (alias, canonical) in builtin_aliases() {
        map.entry(canonical).or_default().push(alias);
    }
    for aliases in map.values_mut() {
        aliases.sort();
    }
    map
}

fn build_builtin_tool_display_map() -> HashMap<String, String> {
    // 按语言偏好选择展示名，英文优先展示别名。
    let prefer_alias = i18n::get_language().to_lowercase().starts_with("en");
    let aliases_by_name = build_builtin_aliases_by_name();
    let mut display_map = HashMap::new();
    for spec in builtin_tool_specs() {
        let name = spec.name;
        let display = if prefer_alias {
            aliases_by_name
                .get(&name)
                .and_then(|aliases| aliases.first())
                .cloned()
                .unwrap_or_else(|| name.clone())
        } else {
            name.clone()
        };
        display_map.insert(name, display);
    }
    display_map
}

fn normalize_tool_stats(tool_stats: Vec<HashMap<String, Value>>) -> Vec<HashMap<String, Value>> {
    let display_map = build_builtin_tool_display_map();
    let builtin_names = builtin_tool_names();
    let mut merged: HashMap<String, i64> = HashMap::new();
    for item in tool_stats {
        let raw_name = item
            .get("tool")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if raw_name.is_empty() {
            continue;
        }
        if raw_name.eq_ignore_ascii_case("performance_log") {
            continue;
        }
        let calls = item
            .get("calls")
            .or_else(|| item.get("count"))
            .or_else(|| item.get("tool_calls"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0);
        let canonical = resolve_tool_name(&raw_name);
        let key = if builtin_names.contains(&canonical) {
            canonical
        } else {
            raw_name
        };
        *merged.entry(key).or_insert(0) += calls;
    }
    let mut merged_list = merged.into_iter().collect::<Vec<_>>();
    merged_list.sort_by(|a, b| b.1.cmp(&a.1));
    merged_list
        .into_iter()
        .map(|(name, calls)| {
            let mut entry = HashMap::new();
            entry.insert(
                "tool".to_string(),
                json!(display_map.get(&name).cloned().unwrap_or(name)),
            );
            entry.insert("calls".to_string(), json!(calls));
            entry
        })
        .collect()
}

fn merge_tool_session_usage(records: Vec<HashMap<String, Value>>) -> Vec<HashMap<String, Value>> {
    #[derive(Default)]
    struct UsageEntry {
        session_id: String,
        user_id: String,
        tool_calls: i64,
        last_time: f64,
    }

    let mut merged: HashMap<(String, String), UsageEntry> = HashMap::new();
    for record in records {
        let session_id = record
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if session_id.is_empty() {
            continue;
        }
        let user_id = record
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let key = (session_id.clone(), user_id.clone());
        let entry = merged.entry(key).or_insert_with(|| UsageEntry {
            session_id: session_id.clone(),
            user_id: user_id.clone(),
            tool_calls: 0,
            last_time: 0.0,
        });
        if entry.user_id.is_empty() && !user_id.is_empty() {
            entry.user_id = user_id.clone();
        }
        let calls = record
            .get("tool_calls")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        entry.tool_calls += calls.max(0);
        let last_time = record
            .get("last_time")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if last_time > entry.last_time {
            entry.last_time = last_time;
        }
    }

    merged
        .into_values()
        .map(|entry| {
            let mut record = HashMap::new();
            record.insert("session_id".to_string(), json!(entry.session_id));
            record.insert("user_id".to_string(), json!(entry.user_id));
            record.insert("tool_calls".to_string(), json!(entry.tool_calls));
            record.insert("last_time".to_string(), json!(entry.last_time));
            record
        })
        .collect()
}

struct UploadedKnowledgeFile {
    filename: String,
    extension: String,
    stem: String,
    temp_dir: PathBuf,
    input_path: PathBuf,
}

async fn save_knowledge_upload_field(
    field: axum::extract::multipart::Field<'_>,
) -> Result<UploadedKnowledgeFile, Response> {
    let filename = field.file_name().unwrap_or("upload").to_string();
    let extension = Path::new(&filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();
    if extension.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.file_extension_missing"),
        ));
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
        return Err(error_response(StatusCode::BAD_REQUEST, message));
    }
    let stem_raw = Path::new(&filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let stem = sanitize_filename_stem(stem_raw);
    let stem = if stem.trim().is_empty() {
        "document".to_string()
    } else {
        stem
    };
    let temp_dir = create_knowledge_temp_dir().await?;
    let input_path = temp_dir.join(format!("{stem}{extension}"));
    save_knowledge_upload_content(field, &input_path).await?;
    Ok(UploadedKnowledgeFile {
        filename,
        extension,
        stem,
        temp_dir,
        input_path,
    })
}

fn build_markdown_output_path(filename: &str, stem: &str) -> String {
    let raw_path = Path::new(filename);
    let output_name = format!("{stem}.md");
    let output = match raw_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        Some(parent) => parent.join(output_name),
        None => PathBuf::from(output_name),
    };
    output.to_string_lossy().replace('\\', "/")
}

async fn persist_knowledge_upload(
    upload: &UploadedKnowledgeFile,
    target: &Path,
) -> Result<(String, Vec<String>), Response> {
    let output_path = upload.temp_dir.join(format!("{}.md", upload.stem));
    let conversion = convert_to_markdown(&upload.input_path, &output_path, &upload.extension)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let metadata = tokio::fs::metadata(&output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if metadata.len() > MAX_KNOWLEDGE_CONTENT_BYTES as u64 {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            i18n::t("tool.read.too_large"),
        ));
    }
    let content = tokio::fs::read_to_string(&output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if content.len() > MAX_KNOWLEDGE_CONTENT_BYTES {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            i18n::t("tool.read.too_large"),
        ));
    }
    if content.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.empty_parse_result"),
        ));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(target, content)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok((conversion.converter, conversion.warnings))
}

async fn create_knowledge_temp_dir() -> Result<PathBuf, Response> {
    let mut root = std::env::temp_dir();
    root.push("wunder_uploads");
    root.push(Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(root)
}

async fn save_knowledge_upload_content(
    mut field: axum::extract::multipart::Field<'_>,
    target: &Path,
) -> Result<(), Response> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let mut file = tokio::fs::File::create(target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut total = 0usize;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        total = total.saturating_add(chunk.len());
        if total > MAX_KNOWLEDGE_UPLOAD_BYTES {
            let _ = tokio::fs::remove_file(target).await;
            return Err(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                i18n::t("workspace.error.upload_too_large"),
            ));
        }
        file.write_all(&chunk)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    Ok(())
}

async fn cleanup_non_markdown_upload(root: &Path, filename: &str, output_name: &str) {
    if filename == output_name {
        return;
    }
    let raw_path = match resolve_knowledge_path(root, filename) {
        Ok(path) => path,
        Err(_) => return,
    };
    let is_markdown = raw_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false);
    if is_markdown {
        return;
    }
    if raw_path.exists() && raw_path.is_file() {
        let _ = tokio::fs::remove_file(raw_path).await;
    }
}

fn list_markdown_files(root: &Path) -> Vec<String> {
    if !root.exists() || !root.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|item| item.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(path);
        files.push(rel.to_string_lossy().replace('\\', "/"));
    }
    files.sort();
    files
}

fn resolve_knowledge_base(
    config: &Config,
    base_name: &str,
) -> Result<KnowledgeBaseConfig, Response> {
    let name = base_name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    config
        .knowledge
        .bases
        .iter()
        .find(|item| item.name == name)
        .cloned()
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.knowledge_base_not_found"),
            )
        })
}

fn resolve_knowledge_root(base: &KnowledgeBaseConfig, create: bool) -> Result<PathBuf, Response> {
    knowledge::resolve_knowledge_root(base, create)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
}

fn resolve_knowledge_path(root: &Path, relative_path: &str) -> Result<PathBuf, Response> {
    let rel = Path::new(relative_path);
    if rel.is_absolute() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.absolute_path_forbidden"),
        ));
    }
    let target = root.join(rel);
    let resolved = normalize_target_path(&target);
    let normalized_root = normalize_existing_path(root);
    // Windows 有时会生成 \\?\ 前缀，这里做统一化比较避免误报路径越界。
    let root_compare = normalize_path_for_compare(&normalized_root);
    let target_compare = normalize_path_for_compare(&resolved);
    if resolved != root && !target_compare.starts_with(&root_compare) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.path_out_of_bounds"),
        ));
    }
    Ok(resolved)
}

fn normalize_admin_knowledge_bases(
    config: &Config,
    bases: Vec<KnowledgeBaseConfig>,
) -> Result<Vec<KnowledgeBaseConfig>, Response> {
    let mut output = Vec::new();
    for mut base in bases {
        base.name = base.name.trim().to_string();
        base.description = base.description.trim().to_string();
        if base.name.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.knowledge_base_name_required"),
            ));
        }
        let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
        if base_type == KnowledgeBaseType::Vector {
            let embedding_model = base
                .embedding_model
                .as_deref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        i18n::t("error.embedding_model_required"),
                    )
                })?;
            vector_knowledge::resolve_embedding_model(config, &embedding_model)
                .map_err(vector_error_response)?;
            let root = resolve_vector_root_for_admin(&base, true)?;
            base.root = root.to_string_lossy().to_string();
            base.base_type = Some("vector".to_string());
            base.embedding_model = Some(embedding_model);
        } else {
            if base.root.trim().is_empty() {
                base.root = format!("./knowledge/{}", base.name);
            } else {
                base.root = base.root.trim().to_string();
            }
            base.base_type = None;
            base.embedding_model = None;
        }
        output.push(base);
    }
    Ok(output)
}

fn resolve_vector_root_for_admin(
    base: &KnowledgeBaseConfig,
    create: bool,
) -> Result<PathBuf, Response> {
    vector_knowledge::resolve_vector_root(None, &base.name, create)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
}

fn ensure_vector_base(base: &KnowledgeBaseConfig) -> Result<(), Response> {
    if !base.is_vector() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_required"),
        ));
    }
    vector_knowledge::ensure_vector_base_config(base)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(())
}

fn vector_error_response(err: anyhow::Error) -> Response {
    if let Some(io_error) = err.downcast_ref::<std::io::Error>() {
        if io_error.kind() == ErrorKind::NotFound {
            return error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.knowledge_document_not_found"),
            );
        }
    }
    error_response(StatusCode::BAD_REQUEST, err.to_string())
}

async fn convert_upload_to_markdown(
    upload: &UploadedKnowledgeFile,
) -> Result<(String, String, Vec<String>), Response> {
    let output_path = upload.temp_dir.join(format!("{}.md", upload.stem));
    let conversion = convert_to_markdown(&upload.input_path, &output_path, &upload.extension)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let metadata = tokio::fs::metadata(&output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if metadata.len() > MAX_KNOWLEDGE_CONTENT_BYTES as u64 {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            i18n::t("tool.read.too_large"),
        ));
    }
    let content = tokio::fs::read_to_string(&output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if content.len() > MAX_KNOWLEDGE_CONTENT_BYTES {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            i18n::t("tool.read.too_large"),
        ));
    }
    if content.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.empty_parse_result"),
        ));
    }
    Ok((content, conversion.converter, conversion.warnings))
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

fn build_builtin_tools_payload(config: &Config) -> (Vec<String>, Vec<Value>) {
    let enabled_set: HashSet<String> = config
        .tools
        .builtin
        .enabled
        .iter()
        .map(|name| resolve_tool_name(name))
        .collect();
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

fn error_response(status: StatusCode, message: String) -> Response {
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
}

#[derive(Debug, Deserialize)]
struct KnowledgeUpdateRequest {
    knowledge: KnowledgePayload,
}

#[derive(Debug, Deserialize)]
struct KnowledgePayload {
    bases: Vec<KnowledgeBaseConfig>,
}

#[derive(Debug, Deserialize)]
struct KnowledgeFilesQuery {
    base: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeDocsQuery {
    base: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeFileQuery {
    base: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeDocQuery {
    base: String,
    doc_id: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeChunksQuery {
    base: String,
    doc_id: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeTestRequest {
    base: String,
    query: String,
    #[serde(default)]
    top_k: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct KnowledgeChunkUpdateRequest {
    base: String,
    doc_id: String,
    chunk_index: usize,
    content: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeChunkActionRequest {
    base: String,
    doc_id: String,
    chunk_index: usize,
}

#[derive(Debug, Deserialize)]
struct KnowledgeFileUpdate {
    base: String,
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeRefreshForm {
    base: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeReindexRequest {
    base: String,
    #[serde(default)]
    doc_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct MonitorQuery {
    active_only: Option<bool>,
    tool_hours: Option<f64>,
    start_time: Option<f64>,
    end_time: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ThroughputStartRequest {
    #[serde(default)]
    concurrency_list: Vec<usize>,
    #[serde(default)]
    user_id_prefix: Option<String>,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    request_timeout_s: Option<f64>,
    #[serde(default)]
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct ThroughputReportQuery {
    run_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct MonitorToolUsageQuery {
    tool: Option<String>,
    tool_hours: Option<f64>,
    start_time: Option<f64>,
    end_time: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct MonitorCompactionRequest {
    #[serde(default)]
    model_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OrgUnitCreateRequest {
    name: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    sort_order: Option<i64>,
    #[serde(default)]
    leader_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct OrgUnitUpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    sort_order: Option<i64>,
    #[serde(default)]
    leader_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ExternalLinkUpsertRequest {
    #[serde(default)]
    link_id: Option<String>,
    title: String,
    #[serde(default)]
    description: String,
    url: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    allowed_levels: Option<Vec<i32>>,
    #[serde(default)]
    sort_order: Option<i64>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct UserAccountListQuery {
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UserAccountCreateRequest {
    username: String,
    #[serde(default)]
    email: Option<String>,
    password: String,
    #[serde(default)]
    unit_id: Option<String>,
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    is_demo: bool,
}

#[derive(Debug, Deserialize)]
struct UserAccountSeedRequest {
    #[serde(default)]
    per_unit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UserAccountUpdateRequest {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    unit_id: Option<String>,
    #[serde(default)]
    roles: Option<Vec<String>>,
    #[serde(default)]
    daily_quota: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UserAccountPasswordResetRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
struct UserAccountToolAccessRequest {
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct UserAccountAgentAccessRequest {
    #[serde(default)]
    allowed_agent_ids: Option<Vec<String>>,
    #[serde(default)]
    blocked_agent_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct UserSessionsQuery {
    active_only: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ThroughputUserCleanupRequest {
    prefix: Option<String>,
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
    sandbox: Option<SystemSandboxUpdateRequest>,
    #[serde(default)]
    observability: Option<SystemObservabilityUpdateRequest>,
    #[serde(default)]
    cors: Option<SystemCorsUpdateRequest>,
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
    allow_commands: Option<Vec<String>>,
    #[serde(default)]
    allow_paths: Option<Vec<String>>,
    #[serde(default)]
    deny_globs: Option<Vec<String>>,
    #[serde(default)]
    exec_policy_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SystemSandboxUpdateRequest {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    container_root: Option<String>,
    #[serde(default)]
    network: Option<String>,
    #[serde(default)]
    readonly_rootfs: Option<bool>,
    #[serde(default)]
    idle_ttl_s: Option<u64>,
    #[serde(default)]
    timeout_s: Option<u64>,
    #[serde(default)]
    resources: Option<SystemSandboxResourceUpdateRequest>,
}

#[derive(Debug, Deserialize)]
struct SystemSandboxResourceUpdateRequest {
    #[serde(default)]
    cpu: Option<f32>,
    #[serde(default)]
    memory_mb: Option<u64>,
    #[serde(default)]
    pids: Option<u64>,
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
struct ServerUpdateRequest {
    #[serde(default)]
    max_active_sessions: Option<usize>,
    #[serde(default)]
    sandbox_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MemoryEnabledRequest {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct MemoryUpdateRequest {
    summary: String,
}

#[derive(Debug, Deserialize)]
struct ChannelAccountQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelAccountUpsertRequest {
    channel: String,
    account_id: String,
    #[serde(default)]
    config: Option<Value>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelBindingQuery {
    #[serde(default)]
    channel: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelBindingUpsertRequest {
    #[serde(default)]
    binding_id: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    peer_kind: Option<String>,
    #[serde(default)]
    peer_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    tool_overrides: Option<Vec<String>>,
    #[serde(default)]
    priority: Option<i64>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ChannelUserBindingQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    peer_kind: Option<String>,
    #[serde(default)]
    peer_id: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ChannelUserBindingUpsertRequest {
    channel: String,
    account_id: String,
    peer_kind: String,
    peer_id: String,
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct ChannelSessionQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    peer_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ChannelTestRequest {
    message: ChannelMessage,
}

async fn admin_channel_accounts(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ChannelAccountQuery>,
) -> Result<Json<Value>, Response> {
    let channel = query.channel.as_deref().map(|value| value.trim());
    let status = query.status.as_deref().map(|value| value.trim());
    let records = state
        .storage
        .list_channel_accounts(channel, status)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .into_iter()
        .map(|record| {
            json!({
                "channel": record.channel,
                "account_id": record.account_id,
                "config": record.config,
                "status": record.status,
                "created_at": record.created_at,
                "updated_at": record.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_channel_accounts_upsert(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChannelAccountUpsertRequest>,
) -> Result<Json<Value>, Response> {
    let channel = payload.channel.trim().to_string();
    let account_id = payload.account_id.trim().to_string();
    if channel.is_empty() || account_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let now = now_ts();
    let existing = state
        .storage
        .get_channel_account(&channel, &account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let created_at = existing
        .as_ref()
        .map(|record| record.created_at)
        .unwrap_or(now);
    let record = ChannelAccountRecord {
        channel: channel.clone(),
        account_id: account_id.clone(),
        config: payload.config.unwrap_or(Value::Object(Default::default())),
        status: payload.status.unwrap_or_else(|| "active".to_string()),
        created_at,
        updated_at: now,
    };
    state
        .storage
        .upsert_channel_account(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": {
        "channel": record.channel,
        "account_id": record.account_id,
        "config": record.config,
        "status": record.status,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    }})))
}

async fn admin_channel_accounts_delete(
    State(state): State<Arc<AppState>>,
    AxumPath((channel, account_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, Response> {
    let channel = channel.trim().to_string();
    let account_id = account_id.trim().to_string();
    if channel.is_empty() || account_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let affected = state
        .storage
        .delete_channel_account(&channel, &account_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "deleted": affected } })))
}

async fn admin_channel_bindings(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ChannelBindingQuery>,
) -> Result<Json<Value>, Response> {
    let channel = query.channel.as_deref().map(|value| value.trim());
    let records = state
        .storage
        .list_channel_bindings(channel)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .into_iter()
        .map(|record| {
            json!({
                "binding_id": record.binding_id,
                "channel": record.channel,
                "account_id": record.account_id,
                "peer_kind": record.peer_kind,
                "peer_id": record.peer_id,
                "agent_id": record.agent_id,
                "tool_overrides": record.tool_overrides,
                "priority": record.priority,
                "enabled": record.enabled,
                "created_at": record.created_at,
                "updated_at": record.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_channel_bindings_upsert(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChannelBindingUpsertRequest>,
) -> Result<Json<Value>, Response> {
    let binding_id = payload
        .binding_id
        .unwrap_or_else(|| format!("bind_{}", Uuid::new_v4().simple()));
    let now = now_ts();
    let record = ChannelBindingRecord {
        binding_id: binding_id.clone(),
        channel: payload.channel.unwrap_or_default(),
        account_id: payload.account_id.unwrap_or_default(),
        peer_kind: payload.peer_kind,
        peer_id: payload.peer_id,
        agent_id: payload.agent_id,
        tool_overrides: payload.tool_overrides.unwrap_or_default(),
        priority: payload.priority.unwrap_or(0),
        enabled: payload.enabled.unwrap_or(true),
        created_at: now,
        updated_at: now,
    };
    state
        .storage
        .upsert_channel_binding(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": {
        "binding_id": record.binding_id,
        "channel": record.channel,
        "account_id": record.account_id,
        "peer_kind": record.peer_kind,
        "peer_id": record.peer_id,
        "agent_id": record.agent_id,
        "tool_overrides": record.tool_overrides,
        "priority": record.priority,
        "enabled": record.enabled,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    }})))
}

async fn admin_channel_bindings_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(binding_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let binding_id = binding_id.trim().to_string();
    if binding_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let affected = state
        .storage
        .delete_channel_binding(&binding_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "deleted": affected } })))
}

async fn admin_channel_user_bindings(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ChannelUserBindingQuery>,
) -> Result<Json<Value>, Response> {
    let (items, total) = state
        .storage
        .list_channel_user_bindings(
            query.channel.as_deref(),
            query.account_id.as_deref(),
            query.peer_kind.as_deref(),
            query.peer_id.as_deref(),
            query.user_id.as_deref(),
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(50),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = items
        .into_iter()
        .map(|record| {
            json!({
                "channel": record.channel,
                "account_id": record.account_id,
                "peer_kind": record.peer_kind,
                "peer_id": record.peer_id,
                "user_id": record.user_id,
                "created_at": record.created_at,
                "updated_at": record.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items, "total": total } })))
}

async fn admin_channel_user_bindings_upsert(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChannelUserBindingUpsertRequest>,
) -> Result<Json<Value>, Response> {
    let channel = payload.channel.trim().to_string();
    let account_id = payload.account_id.trim().to_string();
    let peer_kind = payload.peer_kind.trim().to_string();
    let peer_id = payload.peer_id.trim().to_string();
    let user_id = payload.user_id.trim().to_string();
    if channel.is_empty()
        || account_id.is_empty()
        || peer_kind.is_empty()
        || peer_id.is_empty()
        || user_id.is_empty()
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let now = now_ts();
    let record = crate::storage::ChannelUserBindingRecord {
        channel,
        account_id,
        peer_kind,
        peer_id,
        user_id,
        created_at: now,
        updated_at: now,
    };
    state
        .storage
        .upsert_channel_user_binding(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": {
        "channel": record.channel,
        "account_id": record.account_id,
        "peer_kind": record.peer_kind,
        "peer_id": record.peer_id,
        "user_id": record.user_id,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    }})))
}

async fn admin_channel_user_bindings_delete(
    State(state): State<Arc<AppState>>,
    AxumPath((channel, account_id, peer_kind, peer_id)): AxumPath<(String, String, String, String)>,
) -> Result<Json<Value>, Response> {
    let channel = channel.trim().to_string();
    let account_id = account_id.trim().to_string();
    let peer_kind = peer_kind.trim().to_string();
    let peer_id = peer_id.trim().to_string();
    if channel.is_empty() || account_id.is_empty() || peer_kind.is_empty() || peer_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let affected = state
        .storage
        .delete_channel_user_binding(&channel, &account_id, &peer_kind, &peer_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "deleted": affected } })))
}

async fn admin_channel_sessions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ChannelSessionQuery>,
) -> Result<Json<Value>, Response> {
    let (items, total) = state
        .storage
        .list_channel_sessions(
            query.channel.as_deref(),
            query.account_id.as_deref(),
            query.peer_id.as_deref(),
            query.session_id.as_deref(),
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(50),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = items
        .into_iter()
        .map(|record| {
            json!({
                "channel": record.channel,
                "account_id": record.account_id,
                "peer_kind": record.peer_kind,
                "peer_id": record.peer_id,
                "thread_id": record.thread_id,
                "session_id": record.session_id,
                "agent_id": record.agent_id,
                "user_id": record.user_id,
                "tts_enabled": record.tts_enabled,
                "tts_voice": record.tts_voice,
                "metadata": record.metadata,
                "last_message_at": record.last_message_at,
                "created_at": record.created_at,
                "updated_at": record.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items, "total": total } })))
}

async fn admin_channel_test(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChannelTestRequest>,
) -> Result<Json<Value>, Response> {
    let provider = payload.message.channel.clone();
    let headers = AxumHeaderMap::new();
    let result = state
        .channels
        .handle_inbound(&provider, &headers, vec![payload.message], None)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": {
        "accepted": result.accepted,
        "session_ids": result.session_ids,
        "outbox_ids": result.outbox_ids,
        "errors": result.errors,
    }})))
}

#[derive(Debug, Deserialize)]
struct GatewayClientQuery {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayNodeQuery {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayNodeUpsertRequest {
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    device_fingerprint: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct GatewayNodeTokenQuery {
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayNodeTokenCreateRequest {
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayNodeInvokeRequestPayload {
    node_id: String,
    command: String,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    timeout_s: Option<f64>,
    #[serde(default)]
    metadata: Option<Value>,
}

async fn admin_gateway_status(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let snapshot = state.gateway.snapshot().await;
    let nodes = state
        .storage
        .list_gateway_nodes(None)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let online_nodes = snapshot
        .items
        .iter()
        .filter(|item| item.role == "node")
        .count();
    let config = state.config_store.get().await;
    Ok(Json(json!({ "data": {
        "enabled": config.gateway.enabled,
        "protocol_version": config.gateway.protocol_version,
        "state_version": snapshot.state_version,
        "connections": snapshot.items.len(),
        "nodes_total": nodes.len(),
        "nodes_online": online_nodes
    }})))
}

async fn admin_gateway_presence(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, Response> {
    let snapshot = state.gateway.snapshot().await;
    Ok(Json(json!({ "data": {
        "state_version": snapshot.state_version,
        "items": snapshot.items
    }})))
}

async fn admin_gateway_clients(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GatewayClientQuery>,
) -> Result<Json<Value>, Response> {
    let status = query.status.as_deref().map(|value| value.trim());
    let records = state
        .storage
        .list_gateway_clients(status)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .into_iter()
        .map(|record| {
            json!({
                "connection_id": record.connection_id,
                "role": record.role,
                "user_id": record.user_id,
                "node_id": record.node_id,
                "scopes": record.scopes,
                "caps": record.caps,
                "commands": record.commands,
                "client_info": record.client_info,
                "status": record.status,
                "connected_at": record.connected_at,
                "last_seen_at": record.last_seen_at,
                "disconnected_at": record.disconnected_at
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_gateway_nodes(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GatewayNodeQuery>,
) -> Result<Json<Value>, Response> {
    let status = query.status.as_deref().map(|value| value.trim());
    let records = state
        .storage
        .list_gateway_nodes(status)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .into_iter()
        .map(|record| gateway_node_payload(&record))
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_gateway_nodes_upsert(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GatewayNodeUpsertRequest>,
) -> Result<Json<Value>, Response> {
    let node_id = payload
        .node_id
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("node_{}", Uuid::new_v4().simple()));
    let now = now_ts();
    let mut record = state
        .storage
        .get_gateway_node(&node_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .unwrap_or(GatewayNodeRecord {
            node_id: node_id.clone(),
            name: None,
            device_fingerprint: None,
            status: "active".to_string(),
            caps: Vec::new(),
            commands: Vec::new(),
            permissions: None,
            metadata: None,
            created_at: now,
            updated_at: now,
            last_seen_at: now,
        });
    if let Some(name) = payload.name {
        let trimmed = name.trim().to_string();
        if !trimmed.is_empty() {
            record.name = Some(trimmed);
        }
    }
    if let Some(status) = payload.status {
        let trimmed = status.trim().to_string();
        if !trimmed.is_empty() {
            record.status = trimmed;
        }
    }
    if let Some(fingerprint) = payload.device_fingerprint {
        let trimmed = fingerprint.trim().to_string();
        if !trimmed.is_empty() {
            record.device_fingerprint = Some(trimmed);
        }
    }
    if payload.metadata.is_some() {
        record.metadata = payload.metadata;
    }
    record.updated_at = now;
    let stored = record.clone();
    state
        .storage
        .upsert_gateway_node(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": gateway_node_payload(&stored) })))
}

async fn admin_gateway_node_tokens(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GatewayNodeTokenQuery>,
) -> Result<Json<Value>, Response> {
    let node_id = query.node_id.as_deref().map(|value| value.trim());
    let status = query.status.as_deref().map(|value| value.trim());
    let records = state
        .storage
        .list_gateway_node_tokens(node_id, status)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .into_iter()
        .map(|record| gateway_node_token_payload(&record))
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_gateway_node_tokens_create(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GatewayNodeTokenCreateRequest>,
) -> Result<Json<Value>, Response> {
    let node_id = payload
        .node_id
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("node_{}", Uuid::new_v4().simple()));
    let now = now_ts();
    if state
        .storage
        .get_gateway_node(&node_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .is_none()
    {
        let record = GatewayNodeRecord {
            node_id: node_id.clone(),
            name: None,
            device_fingerprint: None,
            status: "active".to_string(),
            caps: Vec::new(),
            commands: Vec::new(),
            permissions: None,
            metadata: None,
            created_at: now,
            updated_at: now,
            last_seen_at: now,
        };
        state
            .storage
            .upsert_gateway_node(&record)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let token = format!("gwn_{}", Uuid::new_v4().simple());
    let status = payload
        .status
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("active")
        .to_string();
    let record = GatewayNodeTokenRecord {
        token: token.clone(),
        node_id: node_id.clone(),
        status,
        created_at: now,
        updated_at: now,
        last_used_at: None,
    };
    state
        .storage
        .upsert_gateway_node_token(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": gateway_node_token_payload(&record) })))
}

async fn admin_gateway_node_tokens_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(token): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = token.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let removed = state
        .storage
        .delete_gateway_node_token(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "removed": removed } })))
}

async fn admin_gateway_invoke(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GatewayNodeInvokeRequestPayload>,
) -> Result<Json<Value>, Response> {
    let node_id = payload.node_id.trim();
    if node_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let command = payload.command.trim();
    if command.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let timeout_s = payload.timeout_s.unwrap_or(30.0);
    let result = state
        .gateway
        .invoke_node(GatewayNodeInvokeRequest {
            node_id: node_id.to_string(),
            command: command.to_string(),
            args: payload.args,
            timeout_s,
            metadata: payload.metadata,
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": {
        "ok": result.ok,
        "payload": result.payload,
        "error": result.error
    } })))
}

fn gateway_node_payload(record: &GatewayNodeRecord) -> Value {
    json!({
        "node_id": record.node_id,
        "name": record.name,
        "device_fingerprint": record.device_fingerprint,
        "status": record.status,
        "caps": record.caps,
        "commands": record.commands,
        "permissions": record.permissions,
        "metadata": record.metadata,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "last_seen_at": record.last_seen_at
    })
}

fn gateway_node_token_payload(record: &GatewayNodeTokenRecord) -> Value {
    json!({
        "token": record.token,
        "node_id": record.node_id,
        "status": record.status,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "last_used_at": record.last_used_at
    })
}
