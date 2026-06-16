// 用户自建工具 API：MCP、技能、知识库与额外提示词管理。
use crate::api::skill_fs;
use crate::api::user_context::resolve_user;
use crate::attachment::sanitize_filename_stem;
use crate::config::{Config, McpServerConfig};
use crate::core::blocking;
use crate::core::repo_assets;
use crate::i18n;
use crate::path_utils::{
    is_within_root, normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::schemas::{AvailableToolsResponse, SharedToolSpec, ToolSpec};
use crate::services::abilities::populate_ability_items;
use crate::services::default_tool_profile::curated_default_tool_candidates;
use crate::services::skill_archive::{
    create_skill_archive, import_skill_archive, is_supported_skill_archive_filename,
};
use crate::skills::{load_skills, SkillRegistry, SkillSpec};
use crate::state::AppState;
use crate::storage::StorageBackend;
use crate::tools::{
    a2a_service_schema, build_mcp_tool_alias_entries_for_names, builtin_tool_specs,
    mcp_pack_spec_for_server, resolve_tool_name,
};
use crate::user_access::{
    build_user_tool_context, build_user_tool_context_for_catalog, compute_allowed_tool_names,
    compute_allowed_tool_names_for_catalog, UserToolContext,
};
use crate::user_tools::{UserMcpServer, UserToolsPayload};
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, routing::put, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::io::ReaderStream;
use tracing::{info, warn};
use uuid::Uuid;
use walkdir::WalkDir;

mod download;
mod knowledge;
mod mcp_payload;

use download::{stream_response, TempFileStream};
use mcp_payload::UserMcpServerPayload;

const MAX_SKILL_UPLOAD_BYTES: usize = 200 * 1024 * 1024;
const BUILTIN_SKILLS_ROOT_ENV: &str = "WUNDER_BUILTIN_SKILLS_ROOT";
const BUILTIN_SKILLS_MANIFEST_NAME: &str = ".wunder_builtin_skills_manifest.json";

#[derive(Default)]
struct BuiltinSkillCatalog {
    names: HashSet<String>,
    dir_names: HashSet<String>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum UserSkillSourceKind {
    Builtin,
    Custom,
    Global,
}

impl UserSkillSourceKind {
    fn is_builtin(self) -> bool {
        matches!(self, Self::Builtin)
    }

    pub(crate) fn is_readonly(self) -> bool {
        !matches!(self, Self::Custom)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Custom => "custom",
            Self::Global => "global",
        }
    }
}

pub(crate) struct ResolvedUserSkill {
    pub(crate) spec: SkillSpec,
    pub(crate) root: PathBuf,
    pub(crate) source: UserSkillSourceKind,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/user_tools/mcp",
            get(user_mcp_get).post(user_mcp_update),
        )
        .route("/wunder/user_tools/mcp/tools", post(user_mcp_tools))
        .route(
            "/wunder/user_tools/skills",
            get(user_skills_get)
                .post(user_skills_update)
                .delete(user_skills_delete),
        )
        .route("/wunder/user_tools/skills/files", get(user_skills_files))
        .route(
            "/wunder/user_tools/skills/file",
            get(user_skills_file)
                .put(user_skills_file_update)
                .delete(skill_fs::user_skills_entry_delete),
        )
        .route(
            "/wunder/user_tools/skills/fs",
            get(skill_fs::user_skills_fs_content),
        )
        .route(
            "/wunder/user_tools/skills/fs/search",
            get(skill_fs::user_skills_fs_search),
        )
        .route(
            "/wunder/user_tools/skills/fs/file",
            put(skill_fs::user_skills_fs_file_update),
        )
        .route(
            "/wunder/user_tools/skills/dir",
            post(skill_fs::user_skills_dir_create),
        )
        .route(
            "/wunder/user_tools/skills/move",
            post(skill_fs::user_skills_entry_move),
        )
        .route(
            "/wunder/user_tools/skills/copy",
            post(skill_fs::user_skills_entry_copy),
        )
        .route(
            "/wunder/user_tools/skills/batch",
            post(skill_fs::user_skills_batch),
        )
        .route(
            "/wunder/user_tools/skills/content",
            get(user_skills_content),
        )
        .route("/wunder/user_tools/skills/export", get(user_skills_export))
        .route(
            "/wunder/user_tools/skills/archive",
            get(skill_fs::user_skills_archive),
        )
        .route(
            "/wunder/user_tools/skills/download",
            get(skill_fs::user_skills_download),
        )
        .route(
            "/wunder/user_tools/skills/upload",
            post(user_skills_upload).layer(DefaultBodyLimit::max(MAX_SKILL_UPLOAD_BYTES)),
        )
        .route(
            "/wunder/user_tools/skills/fs/upload",
            post(skill_fs::user_skills_fs_upload)
                .layer(DefaultBodyLimit::max(skill_fs::MAX_SKILL_FS_UPLOAD_BYTES)),
        )
        .route(
            "/wunder/user_tools/knowledge",
            get(knowledge::user_knowledge_get).post(knowledge::user_knowledge_update),
        )
        .route(
            "/wunder/user_tools/knowledge/files",
            get(knowledge::user_knowledge_files),
        )
        .route(
            "/wunder/user_tools/knowledge/file",
            get(knowledge::user_knowledge_file)
                .post(knowledge::user_knowledge_file_update)
                .put(knowledge::user_knowledge_file_update)
                .delete(knowledge::user_knowledge_file_delete),
        )
        .route(
            "/wunder/user_tools/knowledge/docs",
            get(knowledge::user_knowledge_docs),
        )
        .route(
            "/wunder/user_tools/knowledge/doc",
            get(knowledge::user_knowledge_doc).delete(knowledge::user_knowledge_doc_delete),
        )
        .route(
            "/wunder/user_tools/knowledge/chunks",
            get(knowledge::user_knowledge_chunks),
        )
        .route(
            "/wunder/user_tools/knowledge/chunk/embed",
            post(knowledge::user_knowledge_chunk_embed),
        )
        .route(
            "/wunder/user_tools/knowledge/chunk/delete",
            post(knowledge::user_knowledge_chunk_delete),
        )
        .route(
            "/wunder/user_tools/knowledge/chunk/update",
            post(knowledge::user_knowledge_chunk_update),
        )
        .route(
            "/wunder/user_tools/knowledge/test",
            post(knowledge::user_knowledge_test),
        )
        .route(
            "/wunder/user_tools/knowledge/upload",
            post(knowledge::user_knowledge_upload),
        )
        .route(
            "/wunder/user_tools/knowledge/reindex",
            post(knowledge::user_knowledge_reindex),
        )
        .route("/wunder/user_tools/tools", get(user_tools_summary))
        .route("/wunder/user_tools/catalog", get(user_tools_catalog))
        .route(
            "/wunder/user_tools/shared_tools",
            post(user_shared_tools_update),
        )
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
        packaged: false,
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
    let previous = state.user_tool_store.load_user_tools(&user_id);
    let payload = match state.user_tool_store.sync_skills_from_disk(&user_id) {
        Ok(payload) => {
            if payload.skills.enabled != previous.skills.enabled
                || payload.skills.shared != previous.skills.shared
            {
                state.user_tool_manager.clear_skill_cache(Some(&user_id));
            }
            payload
        }
        Err(err) => {
            warn!("failed to sync user skill config from disk for {user_id}: {err}");
            previous
        }
    };
    let config = state.config_store.get().await;
    let skill_root = state.user_tool_store.get_skill_root(&user_id);
    let (skills, enabled, shared) =
        build_visible_user_skills_payload(&config, &payload, &skill_root);
    Ok(Json(json!({
        "data": {
            "enabled": enabled,
            "shared": shared,
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
    let requested_enabled = normalize_skill_name_list(&payload.enabled);
    let requested_shared = normalize_skill_name_list(&payload.shared);
    let previous = state.user_tool_store.load_user_tools(&user_id);
    let config = state.config_store.get().await;
    let desktop_mode = is_desktop_mode(&config);
    let skill_root = state.user_tool_store.get_skill_root(&user_id);
    let mut response_config = config.clone();
    let builtin_catalog = load_builtin_skill_catalog(&config, Some(&skill_root));
    let builtin_names = builtin_catalog.names.clone();

    let requested_enabled_set: HashSet<String> = requested_enabled.iter().cloned().collect();
    let mut persist_enabled = requested_enabled.clone();
    let mut persist_shared = requested_shared.clone();
    if desktop_mode {
        persist_enabled.retain(|name| !builtin_names.contains(name));
        persist_shared.retain(|name| !builtin_names.contains(name));
        let before_enabled: Vec<String> = config
            .skills
            .enabled
            .iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        let before_builtin_enabled = resolve_builtin_enabled_set(&config, &builtin_catalog);
        let requested_builtin_enabled: HashSet<String> = requested_enabled_set
            .intersection(&builtin_names)
            .cloned()
            .collect();
        let mut next_enabled_set: HashSet<String> = before_enabled.iter().cloned().collect();
        for name in &builtin_names {
            next_enabled_set.remove(name);
        }
        next_enabled_set.extend(requested_builtin_enabled.iter().cloned());
        let mut next_enabled = next_enabled_set.into_iter().collect::<Vec<_>>();
        next_enabled.sort();
        let mut before_enabled_sorted = before_enabled;
        before_enabled_sorted.sort();
        if next_enabled != before_enabled_sorted {
            let apply_enabled = next_enabled.clone();
            response_config = state
                .config_store
                .update(|config| {
                    config.skills.enabled = apply_enabled.clone();
                })
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            state.reload_skills(&response_config).await;
            let mut builtin_added: Vec<String> = requested_builtin_enabled
                .difference(&before_builtin_enabled)
                .cloned()
                .collect();
            let mut builtin_removed: Vec<String> = before_builtin_enabled
                .difference(&requested_builtin_enabled)
                .cloned()
                .collect();
            builtin_added.sort();
            builtin_removed.sort();
            if !builtin_added.is_empty() || !builtin_removed.is_empty() {
                info!(
                    "user {user_id} desktop builtin skill selection changed: +{added} -{removed}",
                    added = builtin_added.join(", "),
                    removed = builtin_removed.join(", "),
                );
            }
        }
    }
    let updated = state
        .user_tool_store
        .update_skills(&user_id, persist_enabled, persist_shared)
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
    let (skills, enabled, shared) =
        build_visible_user_skills_payload(&response_config, &updated, &skill_root);
    Ok(Json(json!({
        "data": {
            "enabled": enabled,
            "shared": shared,
            "skills": skills
        }
    })))
}

fn resolve_user_skill_spec(
    config: &Config,
    skill_root: &Path,
    name: &str,
) -> Result<SkillSpec, Response> {
    let cleaned = name.trim();
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    registry
        .get(cleaned)
        .or_else(|| {
            registry.list_specs().into_iter().find(|spec| {
                resolve_skill_top_dir(skill_root, &spec.root).is_some_and(|top_dir| {
                    if cfg!(windows) {
                        top_dir.eq_ignore_ascii_case(cleaned)
                    } else {
                        top_dir == cleaned
                    }
                })
            })
        })
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.skill_not_found")))
}

pub(crate) fn resolve_skill_file_path(
    root: &Path,
    relative_path: &str,
) -> Result<PathBuf, Response> {
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

fn resolve_builtin_skills_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var(BUILTIN_SKILLS_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        let normalized = normalize_existing_path(&path);
        if normalized.exists() && normalized.is_dir() {
            return Some(normalized);
        }
    }
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let fallback = repo_assets::builtin_skills_root(&repo_root);
    if fallback.exists() && fallback.is_dir() {
        return Some(normalize_existing_path(&fallback));
    }
    None
}

fn resolve_skill_top_dir(base_root: &Path, skill_root: &Path) -> Option<String> {
    if let Ok(relative) = skill_root.strip_prefix(base_root) {
        let mut components = relative.components();
        if let Some(first) = components.next() {
            let value = first.as_os_str().to_string_lossy().trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }

    let base_segments = split_skill_path_segments(base_root);
    let skill_segments = split_skill_path_segments(skill_root);
    if base_segments.is_empty() || skill_segments.len() <= base_segments.len() {
        return None;
    }
    if !path_segments_has_prefix(&skill_segments, &base_segments) {
        return None;
    }
    let value = skill_segments[base_segments.len()].trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn split_skill_path_segments(path: &Path) -> Vec<String> {
    normalize_public_path_text(&path.to_string_lossy())
        .split('/')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn path_segments_has_prefix(full: &[String], prefix: &[String]) -> bool {
    if prefix.len() > full.len() {
        return false;
    }
    full.iter().zip(prefix.iter()).all(|(left, right)| {
        if cfg!(windows) {
            left.eq_ignore_ascii_case(right)
        } else {
            left == right
        }
    })
}

fn read_builtin_skill_manifest(skill_root: &Path) -> HashSet<String> {
    let manifest_path = skill_root.join(BUILTIN_SKILLS_MANIFEST_NAME);
    let Ok(content) = std::fs::read_to_string(&manifest_path) else {
        return HashSet::new();
    };
    serde_json::from_str::<Vec<String>>(&content)
        .unwrap_or_default()
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn load_builtin_skill_catalog_from_manifest(
    config: &Config,
    skill_root: &Path,
) -> BuiltinSkillCatalog {
    let dir_names = read_builtin_skill_manifest(skill_root);
    if dir_names.is_empty() {
        return BuiltinSkillCatalog::default();
    }
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let mut names = HashSet::new();
    for spec in registry.list_specs() {
        if let Some(top_dir) = resolve_skill_top_dir(skill_root, &spec.root) {
            if dir_names.contains(&top_dir) {
                names.insert(spec.name);
            }
        }
    }
    BuiltinSkillCatalog { names, dir_names }
}

fn load_builtin_skill_catalog(config: &Config, skill_root: Option<&Path>) -> BuiltinSkillCatalog {
    let mut catalog = if let Some(builtin_root) = resolve_builtin_skills_root() {
        let mut scan_config = config.clone();
        scan_config.skills.paths = vec![builtin_root.to_string_lossy().to_string()];
        scan_config.skills.enabled = Vec::new();
        let registry = load_skills(&scan_config, false, false, false);
        let mut names = HashSet::new();
        let mut dir_names = HashSet::new();
        for spec in registry.list_specs() {
            names.insert(spec.name.clone());
            if let Some(top_dir) = resolve_skill_top_dir(&builtin_root, &spec.root) {
                dir_names.insert(top_dir);
            }
        }
        BuiltinSkillCatalog { names, dir_names }
    } else {
        BuiltinSkillCatalog::default()
    };

    if let Some(skill_root) = skill_root {
        let manifest_catalog = load_builtin_skill_catalog_from_manifest(config, skill_root);
        if catalog.names.is_empty() && catalog.dir_names.is_empty() {
            return manifest_catalog;
        }
        catalog.names.extend(manifest_catalog.names);
        catalog.dir_names.extend(manifest_catalog.dir_names);
    }
    catalog
}

fn is_desktop_mode(config: &Config) -> bool {
    config.server.mode.trim().eq_ignore_ascii_case("desktop")
}

fn normalize_skill_name_list(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw in values {
        let cleaned = raw.trim();
        if cleaned.is_empty() {
            continue;
        }
        if seen.insert(cleaned.to_string()) {
            output.push(cleaned.to_string());
        }
    }
    output
}

fn resolve_builtin_enabled_set(
    config: &Config,
    builtin_catalog: &BuiltinSkillCatalog,
) -> HashSet<String> {
    config
        .skills
        .enabled
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .filter(|value| builtin_catalog.names.contains(value))
        .collect()
}

fn load_skill_registry_for_root(config: &Config, root: &Path) -> SkillRegistry {
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![root.to_string_lossy().to_string()];
    scan_config.skills.enabled = Vec::new();
    load_skills(&scan_config, false, false, false)
}

fn resolve_enabled_global_skill_spec(config: &Config, name: &str) -> Option<SkillSpec> {
    let cleaned = name.trim();
    if cleaned.is_empty() {
        return None;
    }
    let enabled: HashSet<String> = config
        .skills
        .enabled
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if !enabled.contains(cleaned) {
        return None;
    }
    let registry = load_skills(config, false, true, false);
    registry.get(cleaned)
}

fn load_desktop_builtin_skill_catalog_specs(config: &Config) -> Vec<SkillSpec> {
    if !is_desktop_mode(config) {
        return Vec::new();
    }
    load_builtin_skill_registry(config)
        .map(|registry| registry.list_specs())
        .unwrap_or_default()
}

fn load_builtin_skill_registry(config: &Config) -> Option<SkillRegistry> {
    let builtin_root = resolve_builtin_skills_root()?;
    Some(load_skill_registry_for_root(config, &builtin_root))
}

fn resolve_configured_global_skill_roots(config: &Config) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    for raw_path in &config.skills.paths {
        let cleaned = raw_path.trim();
        if cleaned.is_empty() {
            continue;
        }
        let normalized = normalize_existing_path(&PathBuf::from(cleaned));
        if !normalized.exists() || !normalized.is_dir() {
            continue;
        }
        let key = normalize_path_for_compare(&normalized);
        if seen.insert(key) {
            roots.push(normalized);
        }
    }
    roots
}

fn resolve_user_skill_source(
    skill_root: &Path,
    spec: &SkillSpec,
    builtin_catalog: &BuiltinSkillCatalog,
) -> UserSkillSourceKind {
    if let Some(top_dir) = resolve_skill_top_dir(skill_root, &spec.root) {
        if builtin_catalog.dir_names.contains(&top_dir) {
            return UserSkillSourceKind::Builtin;
        }
    }
    if builtin_catalog.names.contains(&spec.name) {
        return UserSkillSourceKind::Builtin;
    }
    UserSkillSourceKind::Custom
}

pub(crate) fn normalize_public_path_text(raw: &str) -> String {
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

pub(crate) fn normalize_public_path(path: &Path) -> String {
    normalize_public_path_text(&path.to_string_lossy())
}

fn user_skill_to_value(
    spec: SkillSpec,
    enabled_set: &HashSet<String>,
    shared_set: &HashSet<String>,
    skill_root: &Path,
    builtin_catalog: &BuiltinSkillCatalog,
    builtin_enabled_set: Option<&HashSet<String>>,
) -> Value {
    let source = resolve_user_skill_source(skill_root, &spec, builtin_catalog);
    let name = spec.name;
    let description = spec.description;
    let path = normalize_public_path_text(&spec.path);
    let input_schema = spec.input_schema;
    let enabled = if source.is_builtin() {
        builtin_enabled_set
            .map(|items| items.contains(&name))
            .unwrap_or_else(|| enabled_set.contains(&name))
    } else {
        true
    };
    let shared = if source.is_builtin() {
        false
    } else {
        shared_set.contains(&name)
    };
    json!({
        "name": name,
        "description": description,
        "path": path,
        "input_schema": input_schema,
        "enabled": enabled,
        "shared": shared,
        "builtin": source.is_builtin(),
        "source": source.as_str(),
        "readonly": source.is_readonly()
    })
}

fn resolve_user_skill_root_for_source(
    config: &Config,
    skill_root: &Path,
    spec: &SkillSpec,
    source: UserSkillSourceKind,
) -> Result<PathBuf, Response> {
    let root = normalize_existing_path(&spec.root);
    if !root.exists() || !root.is_dir() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.skill_file_not_found"),
        ));
    }
    let allowed = match source {
        UserSkillSourceKind::Custom => is_within_root(skill_root, &root),
        UserSkillSourceKind::Builtin => {
            is_within_root(skill_root, &root)
                || resolve_builtin_skills_root()
                    .map(|builtin_root| {
                        let normalized_builtin_root = normalize_existing_path(&builtin_root);
                        is_within_root(&normalized_builtin_root, &root)
                    })
                    .unwrap_or(false)
        }
        UserSkillSourceKind::Global => resolve_configured_global_skill_roots(config)
            .into_iter()
            .any(|configured_root| is_within_root(&configured_root, &root)),
    };
    if !allowed {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.path_out_of_bounds"),
        ));
    }
    Ok(root)
}

pub(crate) fn resolve_visible_user_skill(
    config: &Config,
    skill_root: &Path,
    name: &str,
) -> Result<ResolvedUserSkill, Response> {
    let builtin_catalog = load_builtin_skill_catalog(config, Some(skill_root));
    if let Ok(spec) = resolve_user_skill_spec(config, skill_root, name) {
        let source = resolve_user_skill_source(skill_root, &spec, &builtin_catalog);
        let root = resolve_user_skill_root_for_source(config, skill_root, &spec, source)?;
        return Ok(ResolvedUserSkill { spec, root, source });
    }
    if builtin_catalog.names.contains(name) {
        if let Some(registry) = load_builtin_skill_registry(config) {
            if let Some(spec) = registry.get(name) {
                let root = resolve_user_skill_root_for_source(
                    config,
                    skill_root,
                    &spec,
                    UserSkillSourceKind::Builtin,
                )?;
                return Ok(ResolvedUserSkill {
                    spec,
                    root,
                    source: UserSkillSourceKind::Builtin,
                });
            }
        }
    }
    if let Some(spec) = resolve_enabled_global_skill_spec(config, name) {
        let source = if resolve_user_skill_source(skill_root, &spec, &builtin_catalog).is_builtin()
        {
            UserSkillSourceKind::Builtin
        } else {
            UserSkillSourceKind::Global
        };
        let root = resolve_user_skill_root_for_source(config, skill_root, &spec, source)?;
        return Ok(ResolvedUserSkill { spec, root, source });
    }
    Err(error_response(
        StatusCode::NOT_FOUND,
        i18n::t("error.skill_not_found"),
    ))
}

fn build_visible_user_skills_payload(
    config: &Config,
    payload: &UserToolsPayload,
    skill_root: &Path,
) -> (Vec<Value>, Vec<String>, Vec<String>) {
    let desktop_mode = is_desktop_mode(config);
    let builtin_catalog = load_builtin_skill_catalog(config, Some(skill_root));
    let builtin_enabled_set = if desktop_mode {
        resolve_builtin_enabled_set(config, &builtin_catalog)
    } else {
        HashSet::new()
    };
    let builtin_enabled_ref = if desktop_mode {
        Some(&builtin_enabled_set)
    } else {
        None
    };
    let enabled_set: HashSet<String> = payload.skills.enabled.iter().cloned().collect();
    let mut shared_set: HashSet<String> = payload.skills.shared.iter().cloned().collect();
    if desktop_mode {
        shared_set.retain(|name| !builtin_catalog.names.contains(name));
    }

    let user_registry = load_skill_registry_for_root(config, skill_root);
    let mut skills = Vec::new();
    let mut seen = HashSet::new();
    for spec in user_registry.list_specs() {
        let source = resolve_user_skill_source(skill_root, &spec, &builtin_catalog);
        if source.is_builtin() {
            if desktop_mode && seen.insert(spec.name.clone()) {
                skills.push(user_skill_to_value(
                    spec,
                    &enabled_set,
                    &shared_set,
                    skill_root,
                    &builtin_catalog,
                    builtin_enabled_ref,
                ));
            }
            continue;
        }
        if seen.insert(spec.name.clone()) {
            skills.push(user_skill_to_value(
                spec,
                &enabled_set,
                &shared_set,
                skill_root,
                &builtin_catalog,
                builtin_enabled_ref,
            ));
        }
    }

    if desktop_mode && !builtin_catalog.names.is_empty() {
        if let Some(builtin_registry) = load_builtin_skill_registry(config) {
            let mut ordered_builtin_names =
                builtin_catalog.names.iter().cloned().collect::<Vec<_>>();
            ordered_builtin_names.sort();
            for name in ordered_builtin_names {
                if !seen.insert(name.clone()) {
                    continue;
                }
                if let Some(spec) = builtin_registry.get(&name) {
                    skills.push(user_skill_to_value(
                        spec,
                        &enabled_set,
                        &shared_set,
                        skill_root,
                        &builtin_catalog,
                        builtin_enabled_ref,
                    ));
                }
            }
        }
    }

    skills.sort_by(|left, right| {
        let left_name = left
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let right_name = right
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        left_name.cmp(&right_name)
    });

    let mut enabled = enabled_set.into_iter().collect::<Vec<_>>();
    if desktop_mode {
        enabled.extend(builtin_enabled_set.iter().cloned());
    }
    enabled.sort();
    enabled.dedup();

    let mut shared = shared_set.into_iter().collect::<Vec<_>>();
    shared.sort();

    (skills, enabled, shared)
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
    let resolved_skill = resolve_visible_user_skill(&config, &skill_root, name)?;
    let spec = resolved_skill.spec;
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
            "path": normalize_public_path(&skill_path),
            "content": content
        }
    })))
}

async fn user_skills_export(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserSkillExportQuery>,
) -> Result<Response, Response> {
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
    let resolved_skill = resolve_visible_user_skill(&config, &skill_root, name)?;
    let root = resolved_skill.root;
    let top_dir = root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.skill_file_not_found"),
            )
        })?
        .to_string();

    let archive_path = create_temp_skill_archive_file()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let archive_path_clone = archive_path.clone();
    let root_clone = root.clone();
    let top_dir_clone = top_dir.clone();
    blocking::run_fs("api.user_tools.archive_skill", move || {
        Ok(
            create_skill_archive(&root_clone, &top_dir_clone, &archive_path_clone)
                .map_err(|err| io::Error::other(err.to_string()))?,
        )
    })
    .await
    .map_err(|err| {
        let _ = std::fs::remove_file(&archive_path);
        error_response(StatusCode::BAD_REQUEST, err.to_string())
    })?;

    let filename = format!("{}.zip", sanitize_filename_stem(&resolved_skill.spec.name));
    let file = tokio::fs::File::open(&archive_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let stream = TempFileStream::new(archive_path.clone(), ReaderStream::new(file));
    Ok(stream_response(stream, &filename, "application/zip"))
}

async fn user_skills_files(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserSkillFilesQuery>,
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
    let resolved_skill = resolve_visible_user_skill(&config, &skill_root, name)?;
    let spec = resolved_skill.spec;
    let root = resolved_skill.root;
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
        "data": {
            "name": spec.name,
            "root": normalize_public_path(&root),
            "entries": payload
        }
    })))
}

async fn user_skills_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserSkillFileQuery>,
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
    let relative_path = query.path.trim();
    if relative_path.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.file_path_required"),
        ));
    }
    let config = state.config_store.get().await;
    let skill_root = state.user_tool_store.get_skill_root(&user_id);
    let resolved_skill = resolve_visible_user_skill(&config, &skill_root, name)?;
    let spec = resolved_skill.spec;
    let root = resolved_skill.root;
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
        "data": {
            "name": spec.name,
            "path": rel_text,
            "content": content
        }
    })))
}

async fn user_skills_file_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserSkillFileUpdate>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
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
    let skill_root = state.user_tool_store.get_skill_root(&user_id);
    let resolved_skill = resolve_visible_user_skill(&config, &skill_root, name)?;
    if resolved_skill.source.is_readonly() {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.skill_builtin_readonly"),
        ));
    }
    let root = resolved_skill.root;
    let target = resolve_skill_file_path(&root, relative_path)?;
    if target.exists() && !target.is_file() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.target_not_file"),
        ));
    }
    if !target.exists() {
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
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
        if let Err(err) = state.user_tool_store.sync_skills_from_disk(&user_id) {
            warn!("failed to sync user skill config after SKILL.md update for {user_id}: {err}");
        }
        state.user_tool_manager.clear_skill_cache(Some(&user_id));
    }
    let rel = target.strip_prefix(&root).unwrap_or(&target);
    let rel_text = rel.to_string_lossy().replace('\\', "/");
    Ok(Json(json!({
        "data": {
            "ok": true,
            "path": rel_text,
            "reloaded": should_reload
        }
    })))
}

async fn user_skills_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserSkillDeleteQuery>,
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
    let resolved_skill = resolve_visible_user_skill(&config, &skill_root, name)?;
    if resolved_skill.source.is_readonly() {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.skill_builtin_readonly"),
        ));
    }
    let root = resolved_skill.root;
    tokio::fs::remove_dir_all(&root).await.map_err(|err| {
        error_response(
            StatusCode::BAD_REQUEST,
            i18n::t_with_params(
                "error.skill_delete_failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            ),
        )
    })?;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let enabled: Vec<String> = payload
        .skills
        .enabled
        .iter()
        .filter(|value| value.as_str() != name)
        .cloned()
        .collect();
    let shared: Vec<String> = payload
        .skills
        .shared
        .iter()
        .filter(|value| value.as_str() != name)
        .cloned()
        .collect();
    if enabled != payload.skills.enabled || shared != payload.skills.shared {
        state
            .user_tool_store
            .update_skills(&user_id, enabled, shared)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    state.user_tool_manager.clear_skill_cache(Some(&user_id));
    Ok(Json(json!({
        "data": {
            "ok": true,
            "name": name,
            "message": i18n::t("message.skill_deleted")
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
    if !is_supported_skill_archive_filename(&filename) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_upload_zip_only"),
        ));
    }
    let skill_root = state.user_tool_store.get_skill_root(&user_id).to_path_buf();
    tokio::fs::create_dir_all(&skill_root)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let config = state.config_store.get().await;
    let builtin_catalog = load_builtin_skill_catalog(&config, Some(&skill_root));
    let import_result = blocking::run_fs("api.user_tools.import_skill", {
        let filename = filename.clone();
        let data = data.clone();
        let skill_root = skill_root.clone();
        let reserved_top_dirs = builtin_catalog.dir_names.clone();
        move || import_skill_archive(&filename, &data, &skill_root, &reserved_top_dirs)
    })
    .await
    .map_err(|err| {
        let message = err.to_string();
        let status = if message.contains("builtin skill directory") {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::BAD_REQUEST
        };
        error_response(status, message)
    })?;
    if import_result.extracted > 0 {
        if let Err(err) = state.user_tool_store.sync_skills_from_disk(&user_id) {
            tracing::warn!("failed to sync user skill config after upload for {user_id}: {err}");
        }
    }
    state.user_tool_manager.clear_skill_cache(Some(&user_id));
    Ok(Json(json!({
        "data": {
            "ok": true,
            "extracted": import_result.extracted,
            "top_level_dirs": import_result.top_level_dirs,
            "final_names": import_result.final_names,
            "message": i18n::t("message.upload_success")
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
    let summary =
        build_user_tools_summary(&user_id, &allowed, &context, false, state.storage.as_ref());
    Ok(Json(json!({ "data": summary })))
}

async fn user_tools_catalog(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let context = build_user_tool_context_for_catalog(&state, &user_id).await;
    let allowed = compute_allowed_tool_names_for_catalog(&resolved.user, &context);
    let summary =
        build_user_tools_summary(&user_id, &allowed, &context, true, state.storage.as_ref());
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

fn build_user_tools_summary(
    user_id: &str,
    allowed: &HashSet<String>,
    context: &UserToolContext,
    include_unavailable_user_skills: bool,
    _storage: &dyn StorageBackend,
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
            title: None,
            description: spec.description.clone(),
            input_schema: spec.input_schema.clone(),
        });
    }

    let mut mcp_tools = Vec::new();
    for server in &config.mcp.servers {
        if !server.enabled {
            continue;
        }
        if server.packaged {
            if let Some(spec) = mcp_pack_spec_for_server(server) {
                if allowed.contains(&spec.name) {
                    mcp_tools.push(spec);
                }
            }
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
                title: tool.title.clone(),
                description,
                input_schema,
            });
        }
    }
    apply_mcp_tool_display_titles(&mut mcp_tools);

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
                title: None,
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
            title: None,
            description: spec.description,
            input_schema: spec.input_schema,
        })
        .collect::<Vec<_>>();
    let mut skills = skills;
    if include_unavailable_user_skills {
        let mut seen_skill_names: HashSet<String> =
            skills.iter().map(|spec| spec.name.clone()).collect();
        for spec in load_desktop_builtin_skill_catalog_specs(config) {
            if !seen_skill_names.insert(spec.name.clone()) {
                continue;
            }
            skills.push(ToolSpec {
                name: spec.name,
                title: None,
                description: spec.description,
                input_schema: spec.input_schema,
            });
        }
    }

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
            "keywords": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": i18n::t("knowledge.tool.keywords.description") },
            "limit": { "type": "integer", "minimum": 1, "description": i18n::t("knowledge.tool.limit.description") }
        },
        "anyOf": [
            { "required": ["query"] },
            { "required": ["keywords"] }
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
            title: None,
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
                title: None,
                description: spec.description.clone(),
                input_schema: spec.input_schema.clone(),
            });
    }

    let mut selected_current_user_skill_names = HashSet::new();
    for allowed_name in allowed {
        let Some(alias_info) = context.bindings.alias_map.get(allowed_name) else {
            continue;
        };
        if !matches!(alias_info.kind, crate::user_tools::UserToolKind::Skill)
            || alias_info.owner_id != user_id
        {
            continue;
        }
        let target_name = alias_info.target.trim();
        if !target_name.is_empty() {
            selected_current_user_skill_names.insert(target_name.to_string());
        }
    }

    let mut user_tools = Vec::new();
    let mut user_mcp_tools = Vec::new();
    let mut user_skills = Vec::new();
    let mut user_knowledge_tools = Vec::new();
    let mut shared_tools = Vec::new();
    let mut alias_names: Vec<String> = context.bindings.alias_map.keys().cloned().collect();
    alias_names.sort();
    for alias in alias_names {
        let Some(alias_info) = context.bindings.alias_map.get(&alias) else {
            continue;
        };
        let target_name = alias_info.target.trim();
        let is_current_user_skill =
            matches!(alias_info.kind, crate::user_tools::UserToolKind::Skill)
                && alias_info.owner_id == user_id;
        let is_primary_current_user_skill = is_current_user_skill && alias == target_name;
        if is_current_user_skill && !is_primary_current_user_skill {
            continue;
        }
        let is_allowed = allowed.contains(&alias)
            || (is_primary_current_user_skill
                && selected_current_user_skill_names.contains(target_name));
        if !is_allowed && !(include_unavailable_user_skills && is_current_user_skill) {
            continue;
        }
        let Some(spec) = alias_specs.get(&alias) else {
            continue;
        };
        if alias_info.owner_id == user_id {
            let tool = ToolSpec {
                name: alias.clone(),
                title: spec.title.clone(),
                description: spec.description.clone(),
                input_schema: spec.input_schema.clone(),
            };
            match alias_info.kind {
                crate::user_tools::UserToolKind::Mcp => user_mcp_tools.push(tool.clone()),
                crate::user_tools::UserToolKind::Skill => user_skills.push(tool.clone()),
                crate::user_tools::UserToolKind::Knowledge => {
                    user_knowledge_tools.push(tool.clone())
                }
            }
            user_tools.push(tool);
        } else {
            shared_tools.push(SharedToolSpec {
                name: alias.clone(),
                description: spec.description.clone(),
                input_schema: spec.input_schema.clone(),
                owner_id: alias_info.owner_id.clone(),
            });
        }
    }

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

    let default_candidates: HashSet<String> =
        curated_default_tool_candidates().into_iter().collect();
    let mut default_agent_tool_names = Vec::new();
    let mut default_seen = HashSet::new();
    for spec in builtin_tools.iter().chain(skills.iter()) {
        let canonical = resolve_tool_name(&spec.name);
        if !(default_candidates.contains(&canonical) || default_candidates.contains(&spec.name)) {
            continue;
        }
        if default_seen.insert(spec.name.clone()) {
            default_agent_tool_names.push(spec.name.clone());
        }
    }

    let mut response = AvailableToolsResponse {
        builtin_tools: builtin_tools.clone(),
        mcp_tools: mcp_tools.clone(),
        a2a_tools: a2a_tools.clone(),
        skills: skills.clone(),
        knowledge_tools: knowledge_tools.clone(),
        user_tools,
        admin_builtin_tools: builtin_tools,
        admin_mcp_tools: mcp_tools,
        admin_a2a_tools: a2a_tools,
        admin_skills: skills,
        admin_knowledge_tools: knowledge_tools,
        user_mcp_tools,
        user_skills,
        user_knowledge_tools,
        default_agent_tool_names,
        shared_tools,
        shared_tools_selected: Some(shared_tools_selected),
        items: Vec::new(),
    };
    populate_ability_items(&mut response);
    response
}

fn apply_mcp_tool_display_titles(tools: &mut [ToolSpec]) {
    let display_names: HashMap<String, String> =
        build_mcp_tool_alias_entries_for_names(tools.iter().map(|item| item.name.as_str()))
            .into_iter()
            .map(|entry| (entry.runtime_name, entry.display_name))
            .collect();
    for tool in tools {
        let has_title = tool
            .title
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        if has_title {
            continue;
        }
        if let Some(display_name) = display_names.get(&tool.name) {
            tool.title = Some(display_name.clone());
        }
    }
}

fn create_temp_skill_archive_file() -> Result<PathBuf, io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_user_skills");
    std::fs::create_dir_all(&root)?;
    let filename = format!("wunder_user_skill_{}.zip", Uuid::new_v4().simple());
    Ok(root.join(filename))
}

pub(crate) fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
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
struct UserSkillExportQuery {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
}

#[derive(Debug, Deserialize)]
struct UserSkillFilesQuery {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
}

#[derive(Debug, Deserialize)]
struct UserSkillFileQuery {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct UserSkillFileUpdate {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    path: String,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize)]
struct UserSkillDeleteQuery {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
}

#[derive(Debug, Deserialize)]
struct UserSharedToolsUpdate {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    shared_tools: Vec<String>,
}

#[cfg(test)]
mod tests;
