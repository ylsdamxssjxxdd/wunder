// 用户自建工具 API：MCP、技能、知识库与额外提示词管理。
use crate::config::{KnowledgeBaseConfig, McpServerConfig};
use crate::i18n;
use crate::knowledge;
use crate::path_utils::{
    normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::skills::load_skills;
use crate::state::AppState;
use crate::user_tools::{UserKnowledgeBase, UserMcpServer};
use axum::extract::{Multipart, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
        .route("/wunder/user_tools/extra_prompt", post(user_extra_prompt))
}

async fn user_mcp_get(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserIdQuery>,
) -> Result<Json<Value>, Response> {
    let user_id = query.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let payload = state.user_tool_store.load_user_tools(user_id);
    let servers = payload
        .mcp_servers
        .iter()
        .map(UserMcpServerPayload::from)
        .collect::<Vec<_>>();
    Ok(Json(json!({ "servers": servers })))
}

async fn user_mcp_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserMcpUpdate>,
) -> Result<Json<Value>, Response> {
    let user_id = payload.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let servers = payload
        .servers
        .into_iter()
        .map(UserMcpServer::from)
        .collect();
    let updated = state
        .user_tool_store
        .update_mcp_servers(user_id, servers)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let servers = updated
        .mcp_servers
        .iter()
        .map(UserMcpServerPayload::from)
        .collect::<Vec<_>>();
    Ok(Json(json!({ "servers": servers })))
}

async fn user_mcp_tools(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserMcpToolsRequest>,
) -> Result<Json<Value>, Response> {
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
                return Ok(Json(
                    json!({ "tools": Vec::<Value>::new(), "warning": err.to_string() }),
                ));
            }
            return Err(error_response(StatusCode::BAD_REQUEST, err.to_string()));
        }
    };
    Ok(Json(json!({ "tools": tools })))
}

async fn user_skills_get(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserIdQuery>,
) -> Result<Json<Value>, Response> {
    let user_id = query.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let payload = state.user_tool_store.load_user_tools(user_id);
    let config = state.config_store.get().await;
    let skill_root = state.user_tool_store.get_skill_root(user_id);
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false);
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
        "enabled": payload.skills.enabled,
        "shared": payload.skills.shared,
        "skills": skills
    })))
}

async fn user_skills_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserSkillsUpdate>,
) -> Result<Json<Value>, Response> {
    let user_id = payload.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let updated = state
        .user_tool_store
        .update_skills(user_id, payload.enabled.clone(), payload.shared.clone())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state.user_tool_manager.clear_skill_cache(Some(user_id));
    let config = state.config_store.get().await;
    let skill_root = state.user_tool_store.get_skill_root(user_id);
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false);
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
        "enabled": updated.skills.enabled,
        "shared": updated.skills.shared,
        "skills": skills
    })))
}

async fn user_skills_content(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserSkillContentQuery>,
) -> Result<Json<Value>, Response> {
    let user_id = query.user_id.trim();
    let name = query.name.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let skill_root = state.user_tool_store.get_skill_root(user_id);
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
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
    Ok(Json(
        json!({ "name": name, "path": skill_path.to_string_lossy(), "content": content }),
    ))
}

async fn user_skills_upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut user_id = String::new();
    let mut filename = String::new();
    let mut data = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let field_name = field.name().unwrap_or("");
        if field_name == "user_id" {
            user_id = field.text().await.unwrap_or_default();
            continue;
        }
        filename = field.file_name().unwrap_or("skills.zip").to_string();
        data = field
            .bytes()
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .to_vec();
    }
    if user_id.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    if !filename.to_lowercase().ends_with(".zip") {
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
    Ok(Json(
        json!({ "ok": true, "extracted": extracted, "message": i18n::t("message.upload_success") }),
    ))
}

async fn user_knowledge_get(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserIdQuery>,
) -> Result<Json<Value>, Response> {
    let user_id = query.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let payload = state.user_tool_store.load_user_tools(user_id);
    let bases = build_user_knowledge_payload(&state, user_id, &payload.knowledge_bases, false);
    Ok(Json(json!({ "knowledge": { "bases": bases } })))
}

async fn user_knowledge_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserKnowledgeUpdate>,
) -> Result<Json<Value>, Response> {
    let user_id = payload.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let bases = payload
        .knowledge
        .bases
        .into_iter()
        .map(UserKnowledgeBase::from)
        .collect::<Vec<_>>();
    let updated = state
        .user_tool_store
        .update_knowledge_bases(user_id, bases)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let bases = build_user_knowledge_payload(&state, user_id, &updated.knowledge_bases, true);
    Ok(Json(json!({ "knowledge": { "bases": bases } })))
}

async fn user_knowledge_files(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserKnowledgeFilesQuery>,
) -> Result<Json<Value>, Response> {
    let user_id = query.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root(user_id, &query.base, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let files = list_markdown_files(&root);
    Ok(Json(json!({ "base": query.base, "files": files })))
}

async fn user_knowledge_file(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserKnowledgeFileQuery>,
) -> Result<Json<Value>, Response> {
    let user_id = query.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root(user_id, &query.base, false)
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
    Ok(Json(
        json!({ "base": query.base, "path": query.path, "content": content }),
    ))
}

async fn user_knowledge_file_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserKnowledgeFileUpdate>,
) -> Result<Json<Value>, Response> {
    let user_id = payload.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root(user_id, &payload.base, true)
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
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.saved_and_reindexed") }),
    ))
}

async fn user_knowledge_file_delete(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserKnowledgeFileQuery>,
) -> Result<Json<Value>, Response> {
    let user_id = query.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root(user_id, &query.base, false)
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
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.deleted") }),
    ))
}

async fn user_knowledge_upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut user_id = String::new();
    let mut base = String::new();
    let mut filename = String::new();
    let mut data = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let field_name = field.name().unwrap_or("");
        if field_name == "user_id" {
            user_id = field.text().await.unwrap_or_default();
            continue;
        }
        if field_name == "base" {
            base = field.text().await.unwrap_or_default();
            continue;
        }
        filename = field.file_name().unwrap_or("upload.md").to_string();
        data = field.bytes().await.unwrap_or_default().to_vec();
    }
    if user_id.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    if base.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root(&user_id, &base, true)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = resolve_knowledge_path(&root, &filename)?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(&target, data)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    refresh_user_knowledge_cache(&base, &root).await;
    Ok(Json(json!({
        "ok": true,
        "message": i18n::t("message.upload_converted"),
        "path": filename,
        "converter": "raw",
        "warnings": []
    })))
}

async fn user_extra_prompt(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserExtraPromptRequest>,
) -> Result<Json<Value>, Response> {
    let user_id = payload.user_id.trim();
    if user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let updated = state
        .user_tool_store
        .update_extra_prompt(user_id, payload.extra_prompt.clone())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({ "user_id": user_id, "extra_prompt": updated.extra_prompt }),
    ))
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
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct UserMcpUpdate {
    user_id: String,
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
    user_id: String,
    #[serde(default)]
    enabled: Vec<String>,
    #[serde(default)]
    shared: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UserSkillContentQuery {
    user_id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct UserKnowledgeUpdate {
    user_id: String,
    knowledge: UserKnowledgePayload,
}

#[derive(Debug, Deserialize)]
struct UserKnowledgePayload {
    #[serde(default)]
    bases: Vec<UserKnowledgeBasePayload>,
}

#[derive(Debug, Deserialize)]
struct UserKnowledgeFilesQuery {
    user_id: String,
    base: String,
}

#[derive(Debug, Deserialize)]
struct UserKnowledgeFileQuery {
    user_id: String,
    base: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct UserKnowledgeFileUpdate {
    user_id: String,
    base: String,
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct UserExtraPromptRequest {
    user_id: String,
    extra_prompt: String,
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
