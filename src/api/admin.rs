// 管理端 API：配置更新、监控查询、知识库与技能管理等。
use crate::config::Config;
use crate::config::{A2aServiceConfig, KnowledgeBaseConfig, McpServerConfig};
use crate::i18n;
use crate::knowledge;
use crate::llm;
use crate::path_utils::{
    normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::performance::{
    run_sample as run_performance_sample, PerformanceSampleRequest, PerformanceSampleResponse,
};
use crate::skills::load_skills;
use crate::state::AppState;
use crate::throughput::{
    ThroughputConfig, ThroughputReport, ThroughputSnapshot, ThroughputStatusResponse,
};
use crate::tools::{builtin_aliases, builtin_tool_specs, resolve_tool_name};
use axum::extract::{Multipart, Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{routing::delete, routing::get, routing::post, routing::put, Json, Router};
use chrono::{TimeZone, Utc};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;
use url::Url;
use walkdir::WalkDir;
use zip::ZipArchive;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/admin/mcp",
            get(admin_mcp_list).post(admin_mcp_update),
        )
        .route("/wunder/admin/mcp/tools", post(admin_mcp_tools))
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
        .route("/wunder/admin/skills/upload", post(admin_skills_upload))
        .route(
            "/wunder/admin/tools",
            get(admin_tools_list).post(admin_tools_update),
        )
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
        .route(
            "/wunder/admin/knowledge/upload",
            post(admin_knowledge_upload),
        )
        .route(
            "/wunder/admin/knowledge/refresh",
            post(admin_knowledge_refresh),
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
            "/wunder/admin/server",
            get(admin_server_get).post(admin_server_update),
        )
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
        config.mcp.timeout_s.max(10).min(300)
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
    let registry = load_skills(&scan_config, false, false);
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
    let registry = load_skills(&scan_config, false, false);
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
        .map_or(false, |paths| *paths != previous.skills.paths);
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
    let registry = load_skills(&scan_config, false, false);
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
    let registry = load_skills(&scan_config, false, false);
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
        .cloned()
        .filter(|value| value != name)
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
    if !filename.to_lowercase().ends_with(".zip") {
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
    let updated = state
        .config_store
        .update(|config| {
            config.knowledge.bases = payload.knowledge.bases.clone();
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({ "knowledge": { "bases": updated.knowledge.bases } }),
    ))
}

async fn admin_knowledge_files(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeFilesQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
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
        })
        .await;
    }
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.deleted") }),
    ))
}

async fn admin_knowledge_upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut base = String::new();
    let mut name = String::new();
    let mut data = Vec::new();
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
        name = field.file_name().unwrap_or("upload.md").to_string();
        data = field.bytes().await.unwrap_or_default().to_vec();
    }
    if base.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let base_config = resolve_knowledge_base(&config, &base)?;
    let root = resolve_knowledge_root(&base_config, true)?;
    let target = resolve_knowledge_path(&root, &name)?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(&target, data)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    knowledge::refresh_knowledge_cache(&KnowledgeBaseConfig {
        name: base_config.name.clone(),
        description: base_config.description.clone(),
        root: root.to_string_lossy().to_string(),
        enabled: base_config.enabled,
        shared: base_config.shared,
    })
    .await;
    Ok(Json(json!({
        "ok": true,
        "message": i18n::t("message.upload_converted"),
        "path": name,
        "converter": "raw",
        "warnings": []
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
    let root = resolve_knowledge_root(&base_config, true)?;
    knowledge::refresh_knowledge_cache(&KnowledgeBaseConfig {
        name: base_config.name.clone(),
        description: base_config.description.clone(),
        root: root.to_string_lossy().to_string(),
        enabled: base_config.enabled,
        shared: base_config.shared,
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
    let base_url = payload.base_url.trim();
    let model = payload.model.trim();
    if base_url.is_empty() || model.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.base_url_or_model_required"),
        ));
    }
    let provider = payload.provider.as_deref().unwrap_or("openai_compatible");
    if provider != "openai_compatible" {
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
        let token_usage = session_info
            .and_then(|value| value.get("token_usage").and_then(Value::as_i64))
            .unwrap_or(0);
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
            "token_usage": token_usage,
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

async fn admin_monitor_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let ok = state.monitor.delete_session(&session_id);
    if !ok {
        return Ok(Json(json!({
            "ok": false,
            "message": i18n::t("error.session_not_found_or_running")
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
        payload.users,
        payload.duration_s,
        payload.question,
        payload.questions,
        payload.user_id_prefix,
        payload.request_timeout_s,
    )
    .map_err(|message| error_response(StatusCode::BAD_REQUEST, message))?;
    let snapshot = state
        .throughput
        .start(state.orchestrator.clone(), config)
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

async fn admin_users(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    #[derive(Default)]
    struct UserStats {
        active_sessions: i64,
        history_sessions: i64,
        total_sessions: i64,
        token_usage: i64,
        chat_records: i64,
        tool_calls: i64,
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
        entry.token_usage += session
            .get("token_usage")
            .and_then(Value::as_i64)
            .unwrap_or(0);
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

    let mut users = summary
        .into_iter()
        .map(|(user_id, stats)| {
            json!({
                "user_id": user_id,
                "active_sessions": stats.active_sessions,
                "history_sessions": stats.history_sessions,
                "total_sessions": stats.total_sessions,
                "chat_records": stats.chat_records,
                "tool_calls": stats.tool_calls,
                "token_usage": stats.token_usage
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
    let records = state.memory.list_records(cleaned, None, true);
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
    match Utc.timestamp_opt(secs, nanos).single() {
        Some(dt) => dt.to_rfc3339(),
        None => String::new(),
    }
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
    (status, Json(json!({ "detail": { "message": message } }))).into_response()
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
struct KnowledgeFileQuery {
    base: String,
    path: String,
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

#[derive(Debug, Deserialize, Default)]
struct MonitorQuery {
    active_only: Option<bool>,
    tool_hours: Option<f64>,
    start_time: Option<f64>,
    end_time: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ThroughputStartRequest {
    users: usize,
    duration_s: f64,
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    questions: Option<Vec<String>>,
    #[serde(default)]
    user_id_prefix: Option<String>,
    #[serde(default)]
    request_timeout_s: Option<f64>,
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

#[derive(Debug, Deserialize, Default)]
struct UserSessionsQuery {
    active_only: Option<bool>,
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
