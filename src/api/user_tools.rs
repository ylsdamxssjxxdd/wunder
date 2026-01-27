// 用户自建工具 API：MCP、技能、知识库与额外提示词管理。
use crate::api::user_context::resolve_user;
use crate::attachment::{convert_to_markdown, get_supported_extensions, sanitize_filename_stem};
use crate::config::{KnowledgeBaseConfig, McpServerConfig};
use crate::i18n;
use crate::knowledge;
use crate::path_utils::{
    normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::schemas::{AvailableToolsResponse, SharedToolSpec, ToolSpec};
use crate::skills::load_skills;
use crate::state::AppState;
use crate::tools::{a2a_service_schema, builtin_tool_specs};
use crate::user_access::{
    build_user_tool_context, build_user_tool_context_for_catalog, compute_allowed_tool_names,
    UserToolContext,
};
use crate::user_tools::{UserKnowledgeBase, UserMcpServer};
use axum::extract::{Multipart, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tracing::info;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::ZipArchive;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/user_tools/mcp",
            get(user_mcp_get).post(user_mcp_update),
        )
        .route("/wunder/user_tools/mcp/tools", post(user_mcp_tools))
        .route(
            "/wunder/user_tools/skills",
            get(user_skills_get).post(user_skills_update),
        )
        .route(
            "/wunder/user_tools/skills/content",
            get(user_skills_content),
        )
        .route("/wunder/user_tools/skills/upload", post(user_skills_upload))
        .route(
            "/wunder/user_tools/knowledge",
            get(user_knowledge_get).post(user_knowledge_update),
        )
        .route(
            "/wunder/user_tools/knowledge/files",
            get(user_knowledge_files),
        )
        .route(
            "/wunder/user_tools/knowledge/file",
            get(user_knowledge_file)
                .post(user_knowledge_file_update)
                .put(user_knowledge_file_update)
                .delete(user_knowledge_file_delete),
        )
        .route(
            "/wunder/user_tools/knowledge/upload",
            post(user_knowledge_upload),
        )
        .route("/wunder/user_tools/tools", get(user_tools_summary))
        .route("/wunder/user_tools/catalog", get(user_tools_catalog))
        .route(
            "/wunder/user_tools/shared_tools",
            post(user_shared_tools_update),
        )
        .route("/wunder/user_tools/extra_prompt", post(user_extra_prompt))
}

async fn user_mcp_get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserIdQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let servers = payload
        .mcp_servers
        .iter()
        .map(UserMcpServerPayload::from)
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "servers": servers } })))
}

async fn user_mcp_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserMcpUpdate>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let servers = payload
        .servers
        .into_iter()
        .map(UserMcpServer::from)
        .collect();
    let updated = state
        .user_tool_store
        .update_mcp_servers(&user_id, servers)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let servers = updated
        .mcp_servers
        .iter()
        .map(UserMcpServerPayload::from)
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "servers": servers } })))
}

async fn user_mcp_tools(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserMcpToolsRequest>,
) -> Result<Json<Value>, Response> {
    let _resolved = resolve_user(&state, &headers, None).await?;
    let config = state.config_store.get().await;
    let headers = parse_header_map(payload.headers);
    let server = McpServerConfig {
        name: payload.name,
        endpoint: payload.endpoint,
        allow_tools: Vec::new(),
        enabled: true,
        transport: payload.transport,
        description: None,
        display_name: None,
        headers,
        auth: payload
            .auth
            .and_then(|value| serde_yaml::to_value(value).ok()),
        tool_specs: Vec::new(),
    };
    let tools = match crate::mcp::fetch_tools(&config, &server).await {
        Ok(tools) => tools,
        Err(err) => {
            let transport = crate::mcp::normalize_transport(server.transport.as_deref());
            if transport != "streamable-http" {
                return Ok(Json(json!({
                    "data": { "tools": Vec::<Value>::new(), "warning": err.to_string() }
                })));
            }
            return Err(error_response(StatusCode::BAD_REQUEST, err.to_string()));
        }
    };
    Ok(Json(json!({ "data": { "tools": tools } })))
}

async fn user_skills_get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserIdQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let config = state.config_store.get().await;
    let skill_root = state.user_tool_store.get_skill_root(&user_id);
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let enabled_set: std::collections::HashSet<String> =
        payload.skills.enabled.iter().cloned().collect();
    let shared_set: std::collections::HashSet<String> =
        payload.skills.shared.iter().cloned().collect();
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
                "shared": shared_set.contains(&spec.name),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "data": {
            "enabled": payload.skills.enabled,
            "shared": payload.skills.shared,
            "skills": skills
        }
    })))
}

async fn user_skills_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserSkillsUpdate>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let previous = state.user_tool_store.load_user_tools(&user_id);
    let updated = state
        .user_tool_store
        .update_skills(&user_id, payload.enabled.clone(), payload.shared.clone())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let before_enabled: HashSet<String> = previous.skills.enabled.iter().cloned().collect();
    let after_enabled: HashSet<String> = updated.skills.enabled.iter().cloned().collect();
    let mut enabled_added: Vec<String> =
        after_enabled.difference(&before_enabled).cloned().collect();
    let mut enabled_removed: Vec<String> =
        before_enabled.difference(&after_enabled).cloned().collect();
    enabled_added.sort();
    enabled_removed.sort();
    let before_shared: HashSet<String> = previous.skills.shared.iter().cloned().collect();
    let after_shared: HashSet<String> = updated.skills.shared.iter().cloned().collect();
    let mut shared_added: Vec<String> = after_shared.difference(&before_shared).cloned().collect();
    let mut shared_removed: Vec<String> =
        before_shared.difference(&after_shared).cloned().collect();
    shared_added.sort();
    shared_removed.sort();
    if !enabled_added.is_empty()
        || !enabled_removed.is_empty()
        || !shared_added.is_empty()
        || !shared_removed.is_empty()
    {
        info!(
            "用户 {user_id} 技能配置已更新: 启用 +{enabled_added_len}, 停用 -{enabled_removed_len}, 共享 +{shared_added_len}, 取消共享 -{shared_removed_len}",
            enabled_added_len = enabled_added.len(),
            enabled_removed_len = enabled_removed.len(),
            shared_added_len = shared_added.len(),
            shared_removed_len = shared_removed.len(),
        );
        if !enabled_added.is_empty() {
            info!("用户 {user_id} 启用技能: {}", enabled_added.join(", "));
        }
        if !enabled_removed.is_empty() {
            info!("用户 {user_id} 停用技能: {}", enabled_removed.join(", "));
        }
        if !shared_added.is_empty() {
            info!("用户 {user_id} 共享技能: {}", shared_added.join(", "));
        }
        if !shared_removed.is_empty() {
            info!("用户 {user_id} 取消共享技能: {}", shared_removed.join(", "));
        }
    }
    state.user_tool_manager.clear_skill_cache(Some(&user_id));
    let config = state.config_store.get().await;
    let skill_root = state.user_tool_store.get_skill_root(&user_id);
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let enabled_set: std::collections::HashSet<String> =
        updated.skills.enabled.iter().cloned().collect();
    let shared_set: std::collections::HashSet<String> =
        updated.skills.shared.iter().cloned().collect();
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
                "shared": shared_set.contains(&spec.name),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "data": {
            "enabled": updated.skills.enabled,
            "shared": updated.skills.shared,
            "skills": skills
        }
    })))
}

async fn user_skills_content(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserSkillContentQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let name = query.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let skill_root = state.user_tool_store.get_skill_root(&user_id);
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
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
        "data": {
            "name": name,
            "path": skill_path.to_string_lossy(),
            "content": content
        }
    })))
}

async fn user_skills_upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut raw_user_id = String::new();
    let mut filename = String::new();
    let mut data = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let field_name = field.name().unwrap_or("");
        if field_name == "user_id" {
            raw_user_id = field.text().await.unwrap_or_default();
            continue;
        }
        filename = field.file_name().unwrap_or("skills.zip").to_string();
        data = field
            .bytes()
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .to_vec();
    }
    let resolved = resolve_user(
        &state,
        &headers,
        if raw_user_id.trim().is_empty() {
            None
        } else {
            Some(raw_user_id.trim())
        },
    )
    .await?;
    let user_id = resolved.user.user_id;
    let lower_name = filename.to_lowercase();
    if !(lower_name.ends_with(".zip") || lower_name.ends_with(".skill")) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_upload_zip_only"),
        ));
    }
    let skill_root = state.user_tool_store.get_skill_root(&user_id).to_path_buf();
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
    state.user_tool_manager.clear_skill_cache(Some(&user_id));
    Ok(Json(json!({
        "data": {
            "ok": true,
            "extracted": extracted,
            "message": i18n::t("message.upload_success")
        }
    })))
}

async fn user_knowledge_get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserIdQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let bases = build_user_knowledge_payload(&state, &user_id, &payload.knowledge_bases, false);
    Ok(Json(json!({ "data": { "knowledge": { "bases": bases } } })))
}

async fn user_knowledge_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserKnowledgeUpdate>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let bases = payload
        .knowledge
        .bases
        .into_iter()
        .map(UserKnowledgeBase::from)
        .collect::<Vec<_>>();
    let updated = state
        .user_tool_store
        .update_knowledge_bases(&user_id, bases)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let bases = build_user_knowledge_payload(&state, &user_id, &updated.knowledge_bases, true);
    Ok(Json(json!({ "data": { "knowledge": { "bases": bases } } })))
}

async fn user_knowledge_files(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserKnowledgeFilesQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root(&user_id, &query.base, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let files = list_markdown_files(&root);
    Ok(Json(
        json!({ "data": { "base": query.base, "files": files } }),
    ))
}

async fn user_knowledge_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserKnowledgeFileQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root(&user_id, &query.base, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = resolve_knowledge_path(&root, &query.path)?;
    if target
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase()
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
        "data": { "base": query.base, "path": query.path, "content": content }
    })))
}

async fn user_knowledge_file_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserKnowledgeFileUpdate>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root(&user_id, &payload.base, true)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = resolve_knowledge_path(&root, &payload.path)?;
    if target
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase()
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
    refresh_user_knowledge_cache(&payload.base, &root).await;
    Ok(Json(json!({
        "data": { "ok": true, "message": i18n::t("message.saved_and_reindexed") }
    })))
}

async fn user_knowledge_file_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserKnowledgeFileQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root(&user_id, &query.base, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = resolve_knowledge_path(&root, &query.path)?;
    if target
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase()
        != "md"
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.markdown_only"),
        ));
    }
    if target.exists() {
        tokio::fs::remove_file(&target)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        refresh_user_knowledge_cache(&query.base, &root).await;
    }
    Ok(Json(json!({
        "data": { "ok": true, "message": i18n::t("message.deleted") }
    })))
}

async fn user_knowledge_upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut raw_user_id = String::new();
    let mut base = String::new();
    let mut upload: Option<UploadedKnowledgeFile> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let field_name = field.name().unwrap_or("");
        if field_name == "user_id" {
            raw_user_id = field.text().await.unwrap_or_default();
            continue;
        }
        if field_name == "base" {
            base = field.text().await.unwrap_or_default();
            continue;
        }
        if let Some(previous) = upload.take() {
            let _ = tokio::fs::remove_dir_all(&previous.temp_dir).await;
        }
        upload = Some(save_knowledge_upload_field(field).await?);
    }
    let resolved = resolve_user(
        &state,
        &headers,
        if raw_user_id.trim().is_empty() {
            None
        } else {
            Some(raw_user_id.trim())
        },
    )
    .await?;
    let user_id = resolved.user.user_id;
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
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root(&user_id, &base, true)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let output_name = build_markdown_output_path(&upload.filename, &upload.stem);
    let target = resolve_knowledge_path(&root, &output_name)?;
    let temp_dir = upload.temp_dir.clone();
    let result = persist_knowledge_upload(&upload, &target).await;
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    let (converter, warnings) = result?;
    cleanup_non_markdown_upload(&root, &upload.filename, &output_name).await;
    refresh_user_knowledge_cache(&base, &root).await;
    Ok(Json(json!({
        "data": {
            "ok": true,
            "message": i18n::t("message.upload_converted"),
            "path": output_name,
            "converter": converter,
            "warnings": warnings
        }
    })))
}

async fn user_tools_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let context = build_user_tool_context(&state, &user_id).await;
    let allowed = compute_allowed_tool_names(&resolved.user, &context);
    let summary = build_user_tools_summary(&user_id, &allowed, &context);
    Ok(Json(json!({ "data": summary })))
}

async fn user_tools_catalog(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let context = build_user_tool_context_for_catalog(&state, &user_id).await;
    let allowed = compute_allowed_tool_names(&resolved.user, &context);
    let summary = build_user_tools_summary(&user_id, &allowed, &context);
    Ok(Json(json!({ "data": summary })))
}

async fn user_shared_tools_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserSharedToolsUpdate>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let updated = state
        .user_tool_store
        .update_shared_tools(&user_id, payload.shared_tools)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "user_id": user_id,
            "shared_tools": updated.shared_tools
        }
    })))
}

async fn user_extra_prompt(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserExtraPromptRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let updated = state
        .user_tool_store
        .update_extra_prompt(&user_id, payload.extra_prompt.clone())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "user_id": user_id,
            "extra_prompt": updated.extra_prompt
        }
    })))
}

fn build_user_tools_summary(
    user_id: &str,
    allowed: &HashSet<String>,
    context: &UserToolContext,
) -> AvailableToolsResponse {
    let config = &context.config;
    let language = i18n::get_language().to_lowercase();
    let alias_map = crate::tools::builtin_aliases();
    let mut canonical_aliases: HashMap<String, Vec<String>> = HashMap::new();
    for (alias, canonical) in alias_map {
        canonical_aliases.entry(canonical).or_default().push(alias);
    }

    let mut builtin_tools = Vec::new();
    let mut seen_builtin = HashSet::new();
    for spec in builtin_tool_specs() {
        let aliases = canonical_aliases
            .get(&spec.name)
            .map(|value| value.as_slice())
            .unwrap_or(&[]);
        let enabled =
            allowed.contains(&spec.name) || aliases.iter().any(|alias| allowed.contains(alias));
        if !enabled {
            continue;
        }
        let name = if language.starts_with("en") {
            aliases
                .iter()
                .find(|alias| allowed.contains(alias.as_str()))
                .cloned()
                .or_else(|| aliases.first().cloned())
                .unwrap_or_else(|| spec.name.clone())
        } else {
            spec.name.clone()
        };
        if !seen_builtin.insert(name.clone()) {
            continue;
        }
        builtin_tools.push(ToolSpec {
            name,
            description: spec.description.clone(),
            input_schema: spec.input_schema.clone(),
        });
    }

    let mut mcp_tools = Vec::new();
    for server in &config.mcp.servers {
        if !server.enabled {
            continue;
        }
        let allow: HashSet<String> = server.allow_tools.iter().cloned().collect();
        for tool in &server.tool_specs {
            if tool.name.is_empty() {
                continue;
            }
            if !allow.is_empty() && !allow.contains(&tool.name) {
                continue;
            }
            let full_name = format!("{}@{}", server.name, tool.name);
            if !allowed.contains(&full_name) {
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
                name: full_name,
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
        .filter_map(|service| {
            let full_name = format!("a2a@{}", service.name);
            if !allowed.contains(&full_name) {
                return None;
            }
            Some(ToolSpec {
                name: full_name,
                description: service.description.clone().unwrap_or_default(),
                input_schema: a2a_service_schema(),
            })
        })
        .collect::<Vec<_>>();

    let skills = context
        .skills
        .list_specs()
        .into_iter()
        .filter(|spec| allowed.contains(&spec.name))
        .map(|spec| ToolSpec {
            name: spec.name,
            description: spec.description,
            input_schema: spec.input_schema,
        })
        .collect::<Vec<_>>();

    let mut blocked_names: HashSet<String> = builtin_tools
        .iter()
        .map(|item| item.name.clone())
        .chain(mcp_tools.iter().map(|item| item.name.clone()))
        .chain(a2a_tools.iter().map(|item| item.name.clone()))
        .chain(skills.iter().map(|item| item.name.clone()))
        .collect();

    let knowledge_schema = json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": i18n::t("knowledge.tool.query.description") },
            "limit": { "type": "integer", "minimum": 1, "description": i18n::t("knowledge.tool.limit.description") }
        },
        "required": ["query"]
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
        if !allowed.contains(name) {
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
        knowledge_tools.push(ToolSpec {
            name: name.to_string(),
            description,
            input_schema: knowledge_schema.clone(),
        });
        blocked_names.insert(name.to_string());
    }

    let mut alias_specs: HashMap<String, ToolSpec> = context
        .bindings
        .alias_specs
        .iter()
        .map(|(name, spec)| (name.clone(), spec.clone()))
        .collect();
    for spec in &context.bindings.skill_specs {
        alias_specs
            .entry(spec.name.clone())
            .or_insert_with(|| ToolSpec {
                name: spec.name.clone(),
                description: spec.description.clone(),
                input_schema: spec.input_schema.clone(),
            });
    }

    let mut user_tools = Vec::new();
    let mut shared_tools = Vec::new();
    let mut alias_names: Vec<String> = context.bindings.alias_map.keys().cloned().collect();
    alias_names.sort();
    for alias in alias_names {
        if !allowed.contains(&alias) {
            continue;
        }
        let Some(spec) = alias_specs.get(&alias) else {
            continue;
        };
        let Some(alias_info) = context.bindings.alias_map.get(&alias) else {
            continue;
        };
        if alias_info.owner_id == user_id {
            user_tools.push(ToolSpec {
                name: alias.clone(),
                description: spec.description.clone(),
                input_schema: spec.input_schema.clone(),
            });
        } else {
            shared_tools.push(SharedToolSpec {
                name: alias.clone(),
                description: spec.description.clone(),
                input_schema: spec.input_schema.clone(),
                owner_id: alias_info.owner_id.clone(),
            });
        }
    }

    let extra_prompt = if context.bindings.extra_prompt.trim().is_empty() {
        None
    } else {
        Some(context.bindings.extra_prompt.clone())
    };
    let shared_tool_names: HashSet<String> =
        shared_tools.iter().map(|tool| tool.name.clone()).collect();
    let mut shared_tools_selected = context
        .bindings
        .shared_tools_enabled
        .iter()
        .filter(|name| shared_tool_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    shared_tools_selected.sort();

    AvailableToolsResponse {
        builtin_tools,
        mcp_tools,
        a2a_tools,
        skills,
        knowledge_tools,
        user_tools,
        shared_tools,
        extra_prompt,
        shared_tools_selected: Some(shared_tools_selected),
    }
}

fn build_user_knowledge_payload(
    state: &Arc<AppState>,
    user_id: &str,
    bases: &[UserKnowledgeBase],
    create: bool,
) -> Vec<UserKnowledgeBasePayload> {
    bases
        .iter()
        .map(|base| {
            let mut root = String::new();
            if !base.name.trim().is_empty() {
                if let Ok(path) = state
                    .user_tool_store
                    .resolve_knowledge_base_root(user_id, &base.name, create)
                {
                    root = path.to_string_lossy().to_string();
                }
            }
            UserKnowledgeBasePayload::from_with_root(base, root)
        })
        .collect()
}

async fn refresh_user_knowledge_cache(base: &str, root: &Path) {
    let config = KnowledgeBaseConfig {
        name: base.to_string(),
        description: String::new(),
        root: root.to_string_lossy().to_string(),
        enabled: true,
        shared: None,
    };
    let _ = knowledge::refresh_knowledge_cache(&config).await;
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
    let content = tokio::fs::read_to_string(&output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
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
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
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
        if path.extension().and_then(|ext| ext.to_str()).unwrap_or("") != "md" {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(path);
        files.push(rel.to_string_lossy().replace('\\', "/"));
    }
    files.sort();
    files
}

fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "detail": { "message": message } }))).into_response()
}

fn parse_header_map(value: Option<Value>) -> HashMap<String, String> {
    let mut output = HashMap::new();
    let Some(Value::Object(map)) = value else {
        return output;
    };
    for (key, val) in map {
        let key = key.trim().to_string();
        if key.is_empty() {
            continue;
        }
        let value = match val {
            Value::String(text) => text,
            other => other.to_string(),
        };
        if value.trim().is_empty() {
            continue;
        }
        output.insert(key, value);
    }
    output
}

#[derive(Debug, Deserialize)]
struct UserIdQuery {
    #[serde(default)]
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserMcpUpdate {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    servers: Vec<UserMcpServerPayload>,
}

#[derive(Debug, Deserialize)]
struct UserMcpToolsRequest {
    name: String,
    endpoint: String,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default)]
    headers: Option<Value>,
    #[serde(default)]
    auth: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct UserSkillsUpdate {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    enabled: Vec<String>,
    #[serde(default)]
    shared: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UserSkillContentQuery {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
}

#[derive(Debug, Deserialize)]
struct UserKnowledgeUpdate {
    #[serde(default)]
    user_id: Option<String>,
    knowledge: UserKnowledgePayload,
}

#[derive(Debug, Deserialize)]
struct UserKnowledgePayload {
    #[serde(default)]
    bases: Vec<UserKnowledgeBasePayload>,
}

#[derive(Debug, Deserialize)]
struct UserKnowledgeFilesQuery {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
}

#[derive(Debug, Deserialize)]
struct UserKnowledgeFileQuery {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct UserKnowledgeFileUpdate {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct UserExtraPromptRequest {
    #[serde(default)]
    user_id: Option<String>,
    extra_prompt: String,
}

#[derive(Debug, Deserialize)]
struct UserSharedToolsUpdate {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    shared_tools: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct UserMcpServerPayload {
    name: String,
    endpoint: String,
    #[serde(default)]
    allow_tools: Vec<String>,
    #[serde(default)]
    shared_tools: Vec<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    auth: Option<Value>,
    #[serde(default)]
    tool_specs: Vec<Value>,
}

impl From<UserMcpServerPayload> for UserMcpServer {
    fn from(payload: UserMcpServerPayload) -> Self {
        Self {
            name: payload.name,
            endpoint: payload.endpoint,
            allow_tools: payload.allow_tools,
            shared_tools: payload.shared_tools,
            enabled: payload.enabled,
            transport: payload.transport.unwrap_or_default(),
            description: payload.description,
            display_name: payload.display_name,
            headers: payload.headers,
            auth: payload.auth,
            tool_specs: payload.tool_specs,
        }
    }
}

impl From<&UserMcpServer> for UserMcpServerPayload {
    fn from(server: &UserMcpServer) -> Self {
        Self {
            name: server.name.clone(),
            endpoint: server.endpoint.clone(),
            allow_tools: server.allow_tools.clone(),
            shared_tools: server.shared_tools.clone(),
            enabled: server.enabled,
            transport: if server.transport.trim().is_empty() {
                None
            } else {
                Some(server.transport.clone())
            },
            description: server.description.clone(),
            display_name: server.display_name.clone(),
            headers: server.headers.clone(),
            auth: server.auth.clone(),
            tool_specs: server.tool_specs.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct UserKnowledgeBasePayload {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    root: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    shared: bool,
}

impl UserKnowledgeBasePayload {
    fn from_with_root(base: &UserKnowledgeBase, root: String) -> Self {
        Self {
            name: base.name.clone(),
            description: base.description.clone(),
            root,
            enabled: base.enabled,
            shared: base.shared,
        }
    }
}

impl From<UserKnowledgeBasePayload> for UserKnowledgeBase {
    fn from(payload: UserKnowledgeBasePayload) -> Self {
        Self {
            name: payload.name,
            description: payload.description,
            enabled: payload.enabled,
            shared: payload.shared,
        }
    }
}
