use crate::api::admin::{
    ensure_admin_skill_editable, is_admin_skill_editable, resolve_admin_skill_root,
    resolve_admin_skill_spec,
};
use crate::api::user_context::resolve_user;
use crate::api::user_tools::{error_response, resolve_visible_user_skill};
use crate::i18n;
use crate::path_utils::{is_within_root, normalize_target_path, strip_windows_verbatim_prefix};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Multipart, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::Json;
use bytes::Bytes;
use chrono::{DateTime, Local};
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::write::FileOptions;

pub(crate) const MAX_SKILL_FS_UPLOAD_BYTES: usize = 200 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub(crate) struct UserSkillDirCreate {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    path: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserSkillEntryMove {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    source: String,
    destination: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserSkillEntryCopy {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    source: String,
    destination: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserSkillBatchRequest {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    action: String,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    destination: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserSkillFsQuery {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    include_content: bool,
    #[serde(default)]
    max_bytes: i64,
    #[serde(default)]
    depth: i64,
    #[serde(default)]
    keyword: String,
    #[serde(default)]
    offset: i64,
    #[serde(default)]
    limit: i64,
    #[serde(default = "default_sort_by")]
    sort_by: String,
    #[serde(default = "default_order")]
    order: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserSkillFsSearchQuery {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    keyword: String,
    #[serde(default)]
    offset: i64,
    #[serde(default = "default_search_limit")]
    limit: i64,
    #[serde(default = "default_true")]
    include_files: bool,
    #[serde(default = "default_true")]
    include_dirs: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserSkillArchiveQuery {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserSkillFileQuery {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    path: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserSkillFileUpdate {
    #[serde(default)]
    user_id: Option<String>,
    name: String,
    path: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    create_if_missing: bool,
}

#[derive(Debug, Serialize)]
struct SkillFsEntry {
    name: String,
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
    size: u64,
    updated_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    children: Option<Vec<SkillFsEntry>>,
}

#[derive(Debug, Serialize)]
struct SkillFsContentEntry {
    name: String,
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
    size: u64,
    updated_time: String,
    #[serde(default)]
    children: Vec<SkillFsContentEntry>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SkillFsActionResponse {
    ok: bool,
    message: String,
    tree_version: u64,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SkillFsBatchResponse {
    ok: bool,
    message: String,
    tree_version: u64,
    #[serde(default)]
    succeeded: Vec<String>,
    #[serde(default)]
    failed: Vec<SkillFsBatchFailure>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SkillFsBatchFailure {
    path: String,
    message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SkillFsScope {
    User,
    Admin,
}

struct ResolvedSkillFs {
    user_id: String,
    root: PathBuf,
    readonly: bool,
    skill_name: String,
    scope: SkillFsScope,
}

struct PendingUpload {
    temp_path: PathBuf,
    filename: String,
}

struct ParsedSkillFsUpload {
    raw_user_id: String,
    skill_name: String,
    base_path: String,
    relative_paths: Vec<String>,
    pending_files: Vec<PendingUpload>,
    temp_dir: Option<PathBuf>,
}

impl ParsedSkillFsUpload {
    fn cleanup(&self) {
        cleanup_temp_files(&self.pending_files, self.temp_dir.as_ref());
    }
}

struct TempFileStream {
    path: PathBuf,
    inner: Option<ReaderStream<tokio::fs::File>>,
}

impl TempFileStream {
    fn new(path: PathBuf, inner: ReaderStream<tokio::fs::File>) -> Self {
        Self {
            path,
            inner: Some(inner),
        }
    }
}

impl Stream for TempFileStream {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.inner.as_mut() {
            Some(inner) => Pin::new(inner).poll_next(cx),
            None => Poll::Ready(None),
        }
    }
}

impl Drop for TempFileStream {
    fn drop(&mut self) {
        self.inner.take();
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) async fn user_skills_fs_content(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserSkillFsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved =
        resolve_user_skill_fs(&state, &headers, query.user_id.as_deref(), &query.name).await?;
    skill_fs_content_response(&resolved, &query).await
}

pub(crate) async fn admin_skills_fs_content(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserSkillFsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_admin_skill_fs(&state, &query.name).await?;
    skill_fs_content_response(&resolved, &query).await
}

async fn skill_fs_content_response(
    resolved: &ResolvedSkillFs,
    query: &UserSkillFsQuery,
) -> Result<Json<Value>, Response> {
    let normalized = normalize_relative_path(&query.path);
    let target = resolve_skill_target_path(
        &resolved.root,
        if normalized.is_empty() {
            "."
        } else {
            normalized.as_str()
        },
    )?;
    if !target.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("workspace.error.path_not_found"),
        ));
    }
    let metadata = target
        .metadata()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let updated_time = format_modified_time(&metadata);
    let safe_offset = query.offset.max(0) as u64;
    let safe_limit = query.limit.max(0) as u64;
    if metadata.is_dir() {
        let keyword = query.keyword.trim();
        let (entries, total, current_path, parent) = list_skill_entries(
            &resolved.root,
            &normalized,
            if keyword.is_empty() {
                None
            } else {
                Some(keyword)
            },
            safe_offset,
            safe_limit,
            query.sort_by.trim(),
            query.order.trim(),
        )?;
        let mut content_entries = entries
            .into_iter()
            .map(SkillFsContentEntry::from)
            .collect::<Vec<_>>();
        let depth = query.depth.max(1) as u64;
        if query.include_content && depth > 1 {
            attach_children(
                &resolved.root,
                &mut content_entries,
                depth - 1,
                query.sort_by.trim(),
                query.order.trim(),
            )?;
        }
        return Ok(Json(json!({
            "user_id": resolved.user_id,
            "name": resolved.skill_name,
            "path": current_path,
            "parent": parent,
            "type": "dir",
            "size": 0,
            "updated_time": updated_time,
            "content": null,
            "format": "dir",
            "truncated": false,
            "entries": if query.include_content { content_entries } else { Vec::new() },
            "total": total,
            "offset": safe_offset,
            "limit": safe_limit,
            "readonly": resolved.readonly,
            "tree_version": skill_tree_version(&resolved.root)
        })));
    }

    let mut content = None;
    let mut truncated = false;
    if query.include_content {
        let mut file = tokio::fs::File::open(&target)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let mut buffer = Vec::new();
        let max_bytes = query.max_bytes.max(0) as usize;
        if max_bytes > 0 {
            let mut limited = file.take((max_bytes + 1) as u64);
            limited
                .read_to_end(&mut buffer)
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            if buffer.len() > max_bytes {
                truncated = true;
                buffer.truncate(max_bytes);
            }
        } else {
            file.read_to_end(&mut buffer)
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        content = Some(String::from_utf8_lossy(&buffer).to_string());
    }
    Ok(Json(json!({
        "user_id": resolved.user_id,
        "name": resolved.skill_name,
        "path": normalized,
        "parent": get_parent_path(&normalized),
        "type": "file",
        "size": metadata.len(),
        "updated_time": updated_time,
        "content": content,
        "format": "text",
        "truncated": truncated,
        "entries": Vec::<SkillFsContentEntry>::new(),
        "total": 0,
        "offset": 0,
        "limit": 0,
        "readonly": resolved.readonly,
        "tree_version": skill_tree_version(&resolved.root)
    })))
}

pub(crate) async fn user_skills_fs_search(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserSkillFsSearchQuery>,
) -> Result<Json<Value>, Response> {
    let resolved =
        resolve_user_skill_fs(&state, &headers, query.user_id.as_deref(), &query.name).await?;
    skill_fs_search_response(&resolved, &query)
}

pub(crate) async fn admin_skills_fs_search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserSkillFsSearchQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_admin_skill_fs(&state, &query.name).await?;
    skill_fs_search_response(&resolved, &query)
}

fn skill_fs_search_response(
    resolved: &ResolvedSkillFs,
    query: &UserSkillFsSearchQuery,
) -> Result<Json<Value>, Response> {
    let offset = query.offset.max(0) as u64;
    let limit = query.limit.max(0) as u64;
    let (entries, total) = search_skill_entries(
        &resolved.root,
        &query.keyword,
        offset,
        limit,
        query.include_files,
        query.include_dirs,
    )?;
    Ok(Json(json!({
        "user_id": resolved.user_id,
        "name": resolved.skill_name,
        "keyword": query.keyword,
        "entries": entries,
        "total": total,
        "offset": offset,
        "limit": limit,
        "readonly": resolved.readonly,
        "tree_version": skill_tree_version(&resolved.root)
    })))
}

pub(crate) async fn user_skills_fs_file_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserSkillFileUpdate>,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let resolved =
        resolve_user_skill_fs_writable(&state, &headers, payload.user_id.as_deref(), &payload.name)
            .await?;
    skill_fs_file_update_response(&state, &resolved, &payload).await
}

pub(crate) async fn admin_skills_fs_file_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserSkillFileUpdate>,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let resolved = resolve_admin_skill_fs_writable(&state, &payload.name).await?;
    skill_fs_file_update_response(&state, &resolved, &payload).await
}

async fn skill_fs_file_update_response(
    state: &AppState,
    resolved: &ResolvedSkillFs,
    payload: &UserSkillFileUpdate,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let normalized = normalize_relative_path(&payload.path);
    if normalized.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.file_path_required"),
        ));
    }
    let target = resolve_skill_target_path(&resolved.root, &normalized)?;
    if target.exists() {
        if !target.is_file() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("workspace.error.target_not_file"),
            ));
        }
    } else if !payload.create_if_missing {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.file_not_found"),
        ));
    } else if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    tokio::fs::write(&target, payload.content.as_bytes())
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    refresh_skill_runtime_after_change(state, resolved, &[normalized.clone()], false).await;
    Ok(Json(SkillFsActionResponse {
        ok: true,
        message: i18n::t("workspace.message.file_saved"),
        tree_version: skill_tree_version(&resolved.root),
        files: vec![normalized],
    }))
}

pub(crate) async fn user_skills_fs_upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let upload = parse_skill_fs_upload(&headers, multipart).await?;
    let resolved = resolve_user_skill_fs_writable(
        &state,
        &headers,
        if upload.raw_user_id.is_empty() {
            None
        } else {
            Some(upload.raw_user_id.as_str())
        },
        &upload.skill_name,
    )
    .await
    .map_err(|err| {
        upload.cleanup();
        err
    })?;
    persist_skill_fs_uploads(&state, &resolved, upload).await
}

pub(crate) async fn admin_skills_fs_upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let upload = parse_skill_fs_upload(&headers, multipart).await?;
    let resolved = resolve_admin_skill_fs_writable(&state, &upload.skill_name)
        .await
        .map_err(|err| {
            upload.cleanup();
            err
        })?;
    persist_skill_fs_uploads(&state, &resolved, upload).await
}

async fn parse_skill_fs_upload(
    headers: &HeaderMap,
    mut multipart: Multipart,
) -> Result<ParsedSkillFsUpload, Response> {
    if let Some(length) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        if length > MAX_SKILL_FS_UPLOAD_BYTES as u64 {
            return Err(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                i18n::t("workspace.error.upload_too_large"),
            ));
        }
    }

    let mut raw_user_id = String::new();
    let mut skill_name = String::new();
    let mut base_path = String::new();
    let mut relative_paths = Vec::new();
    let mut pending_files = Vec::new();
    let mut temp_dir = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "user_id" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                raw_user_id = value.trim().to_string();
            }
            "name" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                skill_name = value.trim().to_string();
            }
            "path" => {
                base_path = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            }
            "relative_paths" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                relative_paths.push(value.trim().to_string());
            }
            "files" => {
                let filename = field.file_name().unwrap_or("upload").to_string();
                if temp_dir.is_none() {
                    temp_dir =
                        Some(create_temp_upload_dir().map_err(|err| {
                            error_response(StatusCode::BAD_REQUEST, err.to_string())
                        })?);
                }
                let Some(temp_dir_ref) = temp_dir.as_ref() else {
                    return Err(error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        i18n::t("error.internal"),
                    ));
                };
                let temp_path = temp_dir_ref.join(format!("upload_{}", Uuid::new_v4().simple()));
                save_multipart_file(field, &temp_path).await?;
                pending_files.push(PendingUpload {
                    temp_path,
                    filename,
                });
            }
            _ => {}
        }
    }

    Ok(ParsedSkillFsUpload {
        raw_user_id,
        skill_name,
        base_path,
        relative_paths,
        pending_files,
        temp_dir,
    })
}

async fn persist_skill_fs_uploads(
    state: &AppState,
    resolved: &ResolvedSkillFs,
    upload: ParsedSkillFsUpload,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let normalized_base = normalize_relative_path(&upload.base_path);
    let target_dir = resolve_skill_target_path(
        &resolved.root,
        if normalized_base.is_empty() {
            "."
        } else {
            normalized_base.as_str()
        },
    )
    .map_err(|err| {
        upload.cleanup();
        err
    })?;
    if target_dir.exists() && !target_dir.is_dir() {
        upload.cleanup();
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.target_not_dir"),
        ));
    }
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|err| {
            upload.cleanup();
            error_response(StatusCode::BAD_REQUEST, err.to_string())
        })?;

    let mut uploaded = Vec::new();
    for (index, file) in upload.pending_files.iter().enumerate() {
        let raw_path = upload
            .relative_paths
            .get(index)
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| file.filename.clone());
        let normalized_rel = normalize_relative_path(&raw_path);
        if normalized_rel.is_empty() {
            continue;
        }
        let joined = if normalized_base.is_empty() {
            normalized_rel.clone()
        } else {
            normalize_relative_path(
                &Path::new(&normalized_base)
                    .join(&normalized_rel)
                    .to_string_lossy(),
            )
        };
        let dest = resolve_skill_target_path(&resolved.root, &joined).map_err(|err| {
            upload.cleanup();
            err
        })?;
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|err| {
                upload.cleanup();
                error_response(StatusCode::BAD_REQUEST, err.to_string())
            })?;
        }
        if dest.exists() && dest.is_dir() {
            upload.cleanup();
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("workspace.error.target_not_file"),
            ));
        }
        if dest.exists() {
            let _ = tokio::fs::remove_file(&dest).await;
        }
        if tokio::fs::rename(&file.temp_path, &dest).await.is_err() {
            tokio::fs::copy(&file.temp_path, &dest)
                .await
                .map_err(|err| {
                    upload.cleanup();
                    error_response(StatusCode::BAD_REQUEST, err.to_string())
                })?;
            let _ = tokio::fs::remove_file(&file.temp_path).await;
        }
        uploaded.push(to_relative_path(&resolved.root, &dest));
    }

    upload.cleanup();
    refresh_skill_runtime_after_change(state, resolved, &uploaded, false).await;
    Ok(Json(SkillFsActionResponse {
        ok: true,
        message: i18n::t("message.upload_success"),
        tree_version: skill_tree_version(&resolved.root),
        files: uploaded,
    }))
}

pub(crate) async fn user_skills_dir_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserSkillDirCreate>,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let resolved =
        resolve_user_skill_fs_writable(&state, &headers, payload.user_id.as_deref(), &payload.name)
            .await?;
    skill_fs_dir_create_response(&state, &resolved, &payload).await
}

pub(crate) async fn admin_skills_dir_create(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserSkillDirCreate>,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let resolved = resolve_admin_skill_fs_writable(&state, &payload.name).await?;
    skill_fs_dir_create_response(&state, &resolved, &payload).await
}

async fn skill_fs_dir_create_response(
    state: &AppState,
    resolved: &ResolvedSkillFs,
    payload: &UserSkillDirCreate,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let normalized = normalize_relative_path(&payload.path);
    if normalized.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.dir_path_required"),
        ));
    }
    let target = resolve_skill_target_path(&resolved.root, &normalized)?;
    if target.exists() && !target.is_dir() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.target_exists_not_dir"),
        ));
    }
    tokio::fs::create_dir_all(&target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let created = to_relative_path(&resolved.root, &target);
    refresh_skill_runtime_after_change(state, resolved, std::slice::from_ref(&created), false)
        .await;
    Ok(Json(SkillFsActionResponse {
        ok: true,
        message: i18n::t("workspace.message.dir_created"),
        tree_version: skill_tree_version(&resolved.root),
        files: vec![created],
    }))
}

pub(crate) async fn user_skills_entry_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserSkillFileQuery>,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let resolved =
        resolve_user_skill_fs_writable(&state, &headers, query.user_id.as_deref(), &query.name)
            .await?;
    skill_fs_entry_delete_response(&state, &resolved, &query.path).await
}

pub(crate) async fn admin_skills_entry_delete(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserSkillFileQuery>,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let resolved = resolve_admin_skill_fs_writable(&state, &query.name).await?;
    skill_fs_entry_delete_response(&state, &resolved, &query.path).await
}

async fn skill_fs_entry_delete_response(
    state: &AppState,
    resolved: &ResolvedSkillFs,
    path: &str,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let normalized = normalize_relative_path(path);
    if normalized.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.delete_root_forbidden"),
        ));
    }
    remove_skill_entry(&resolved.root, &normalized).await?;
    refresh_skill_runtime_after_change(state, resolved, &[normalized], true).await;
    Ok(Json(SkillFsActionResponse {
        ok: true,
        message: i18n::t("message.deleted"),
        tree_version: skill_tree_version(&resolved.root),
        files: Vec::new(),
    }))
}

pub(crate) async fn user_skills_entry_move(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserSkillEntryMove>,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let resolved =
        resolve_user_skill_fs_writable(&state, &headers, payload.user_id.as_deref(), &payload.name)
            .await?;
    skill_fs_entry_move_response(&state, &resolved, &payload.source, &payload.destination).await
}

pub(crate) async fn admin_skills_entry_move(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserSkillEntryMove>,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let resolved = resolve_admin_skill_fs_writable(&state, &payload.name).await?;
    skill_fs_entry_move_response(&state, &resolved, &payload.source, &payload.destination).await
}

async fn skill_fs_entry_move_response(
    state: &AppState,
    resolved: &ResolvedSkillFs,
    source: &str,
    destination: &str,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let source = normalize_relative_path(source);
    let destination = normalize_relative_path(destination);
    move_skill_entry(&resolved.root, &source, &destination).await?;
    refresh_skill_runtime_after_change(state, resolved, &[source, destination.clone()], true).await;
    Ok(Json(SkillFsActionResponse {
        ok: true,
        message: i18n::t("workspace.message.moved"),
        tree_version: skill_tree_version(&resolved.root),
        files: vec![destination],
    }))
}

pub(crate) async fn user_skills_entry_copy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserSkillEntryCopy>,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let resolved =
        resolve_user_skill_fs_writable(&state, &headers, payload.user_id.as_deref(), &payload.name)
            .await?;
    skill_fs_entry_copy_response(&state, &resolved, &payload.source, &payload.destination).await
}

pub(crate) async fn admin_skills_entry_copy(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserSkillEntryCopy>,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let resolved = resolve_admin_skill_fs_writable(&state, &payload.name).await?;
    skill_fs_entry_copy_response(&state, &resolved, &payload.source, &payload.destination).await
}

async fn skill_fs_entry_copy_response(
    state: &AppState,
    resolved: &ResolvedSkillFs,
    source: &str,
    destination: &str,
) -> Result<Json<SkillFsActionResponse>, Response> {
    let source = normalize_relative_path(source);
    let destination = normalize_relative_path(destination);
    copy_skill_entry(&resolved.root, &source, &destination).await?;
    refresh_skill_runtime_after_change(state, resolved, std::slice::from_ref(&destination), true)
        .await;
    Ok(Json(SkillFsActionResponse {
        ok: true,
        message: i18n::t("workspace.message.copied"),
        tree_version: skill_tree_version(&resolved.root),
        files: vec![destination],
    }))
}

pub(crate) async fn user_skills_batch(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserSkillBatchRequest>,
) -> Result<Json<SkillFsBatchResponse>, Response> {
    if payload.paths.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.batch_paths_missing"),
        ));
    }
    let resolved =
        resolve_user_skill_fs_writable(&state, &headers, payload.user_id.as_deref(), &payload.name)
            .await?;
    skill_fs_batch_response(&state, &resolved, &payload).await
}

pub(crate) async fn admin_skills_batch(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserSkillBatchRequest>,
) -> Result<Json<SkillFsBatchResponse>, Response> {
    if payload.paths.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.batch_paths_missing"),
        ));
    }
    let resolved = resolve_admin_skill_fs_writable(&state, &payload.name).await?;
    skill_fs_batch_response(&state, &resolved, &payload).await
}

async fn skill_fs_batch_response(
    state: &AppState,
    resolved: &ResolvedSkillFs,
    payload: &UserSkillBatchRequest,
) -> Result<Json<SkillFsBatchResponse>, Response> {
    let action = payload.action.trim().to_string();
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for raw_path in &payload.paths {
        let normalized = normalize_relative_path(raw_path);
        if normalized.is_empty() {
            failed.push(SkillFsBatchFailure {
                path: raw_path.clone(),
                message: i18n::t("workspace.error.path_required"),
            });
            continue;
        }
        let result = match action.as_str() {
            "delete" => remove_skill_entry(&resolved.root, &normalized)
                .await
                .map(|_| normalized.clone()),
            "move" | "copy" => {
                let destination_root =
                    normalize_relative_path(payload.destination.as_deref().unwrap_or(""));
                let source_path = match resolve_skill_target_path(&resolved.root, &normalized) {
                    Ok(path) => path,
                    Err(err) => {
                        failed.push(SkillFsBatchFailure {
                            path: normalized.clone(),
                            message: response_error_fallback(err),
                        });
                        continue;
                    }
                };
                let entry_name = source_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("item");
                let destination = normalize_relative_path(
                    &Path::new(&destination_root)
                        .join(entry_name)
                        .to_string_lossy(),
                );
                if action == "move" {
                    move_skill_entry(&resolved.root, &normalized, &destination).await
                } else {
                    copy_skill_entry(&resolved.root, &normalized, &destination).await
                }
                .map(|_| destination)
            }
            _ => Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("workspace.error.batch_action_unsupported"),
            )),
        };
        match result {
            Ok(path) => succeeded.push(path),
            Err(err) => failed.push(SkillFsBatchFailure {
                path: normalized,
                message: response_error_fallback(err),
            }),
        }
    }

    if action == "delete" || action == "move" || action == "copy" {
        refresh_skill_runtime_after_change(state, resolved, &succeeded, true).await;
    }
    let ok = failed.is_empty();
    Ok(Json(SkillFsBatchResponse {
        ok,
        message: if ok {
            i18n::t("workspace.message.batch_success")
        } else {
            i18n::t("workspace.message.batch_partial")
        },
        tree_version: skill_tree_version(&resolved.root),
        succeeded,
        failed,
    }))
}

pub(crate) async fn user_skills_archive(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserSkillArchiveQuery>,
) -> Result<Response, Response> {
    let resolved =
        resolve_user_skill_fs(&state, &headers, query.user_id.as_deref(), &query.name).await?;
    skill_fs_archive_response(&resolved, query.path.as_deref()).await
}

pub(crate) async fn admin_skills_archive(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserSkillArchiveQuery>,
) -> Result<Response, Response> {
    let resolved = resolve_admin_skill_fs(&state, &query.name).await?;
    skill_fs_archive_response(&resolved, query.path.as_deref()).await
}

async fn skill_fs_archive_response(
    resolved: &ResolvedSkillFs,
    raw_path: Option<&str>,
) -> Result<Response, Response> {
    let normalized = normalize_relative_path(raw_path.unwrap_or(""));
    let (target, base_root, filename_prefix) = if normalized.is_empty() {
        (
            resolved.root.clone(),
            resolved
                .root
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| resolved.root.clone()),
            resolved.skill_name.clone(),
        )
    } else {
        let target = resolve_skill_target_path(&resolved.root, &normalized)?;
        if !target.exists() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                i18n::t("workspace.error.path_not_found"),
            ));
        }
        let base_root = target
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| resolved.root.clone());
        let filename_prefix = target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("skill")
            .to_string();
        (target, base_root, filename_prefix)
    };
    let archive_path = create_temp_archive_file()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let archive_path_clone = archive_path.clone();
    let target_clone = target.clone();
    let base_clone = base_root.clone();
    tokio::task::spawn_blocking(move || {
        build_archive(&archive_path_clone, &target_clone, &base_clone)
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .map_err(|err| {
        let _ = std::fs::remove_file(&archive_path);
        error_response(StatusCode::BAD_REQUEST, err.to_string())
    })?;
    let filename = if filename_prefix.to_lowercase().ends_with(".zip") {
        filename_prefix
    } else {
        format!("{filename_prefix}.zip")
    };
    let file = tokio::fs::File::open(&archive_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let stream = TempFileStream::new(archive_path.clone(), ReaderStream::new(file));
    Ok(stream_response(stream, &filename, "application/zip"))
}

pub(crate) async fn user_skills_download(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserSkillFileQuery>,
) -> Result<Response, Response> {
    let resolved =
        resolve_user_skill_fs(&state, &headers, query.user_id.as_deref(), &query.name).await?;
    skill_fs_download_response(&resolved, &query.path).await
}

pub(crate) async fn admin_skills_download(
    State(state): State<Arc<AppState>>,
    Query(query): Query<UserSkillFileQuery>,
) -> Result<Response, Response> {
    let resolved = resolve_admin_skill_fs(&state, &query.name).await?;
    skill_fs_download_response(&resolved, &query.path).await
}

async fn skill_fs_download_response(
    resolved: &ResolvedSkillFs,
    path: &str,
) -> Result<Response, Response> {
    let normalized = normalize_relative_path(path);
    if normalized.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.path_required"),
        ));
    }
    let target = resolve_skill_target_path(&resolved.root, &normalized)?;
    if !target.exists() || !target.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.file_not_found"),
        ));
    }
    let filename = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    let file = tokio::fs::File::open(&target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(stream_response(
        ReaderStream::new(file),
        filename,
        "application/octet-stream",
    ))
}

async fn resolve_user_skill_fs(
    state: &AppState,
    headers: &HeaderMap,
    user_id: Option<&str>,
    name: &str,
) -> Result<ResolvedSkillFs, Response> {
    let skill_name = name.trim();
    if skill_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let resolved = resolve_user(state, headers, user_id).await?;
    let user_id = resolved.user.user_id;
    let config = state.config_store.get().await;
    let skill_root = state.user_tool_store.get_skill_root(&user_id);
    let resolved_skill = resolve_visible_user_skill(&config, &skill_root, skill_name)?;
    Ok(ResolvedSkillFs {
        user_id,
        root: resolved_skill.root,
        readonly: resolved_skill.source.is_readonly(),
        skill_name: resolved_skill.spec.name,
        scope: SkillFsScope::User,
    })
}

async fn resolve_user_skill_fs_writable(
    state: &AppState,
    headers: &HeaderMap,
    user_id: Option<&str>,
    name: &str,
) -> Result<ResolvedSkillFs, Response> {
    let resolved = resolve_user_skill_fs(state, headers, user_id, name).await?;
    if resolved.readonly {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.skill_builtin_readonly"),
        ));
    }
    Ok(resolved)
}

async fn resolve_admin_skill_fs(state: &AppState, name: &str) -> Result<ResolvedSkillFs, Response> {
    let skill_name = name.trim();
    if skill_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let spec = resolve_admin_skill_spec(&config, skill_name)?;
    let root = resolve_admin_skill_root(&spec)?;
    Ok(ResolvedSkillFs {
        user_id: "admin".to_string(),
        root,
        readonly: !is_admin_skill_editable(&spec),
        skill_name: spec.name,
        scope: SkillFsScope::Admin,
    })
}

async fn resolve_admin_skill_fs_writable(
    state: &AppState,
    name: &str,
) -> Result<ResolvedSkillFs, Response> {
    let skill_name = name.trim();
    if skill_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.skill_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let spec = resolve_admin_skill_spec(&config, skill_name)?;
    let root = ensure_admin_skill_editable(&spec)?;
    Ok(ResolvedSkillFs {
        user_id: "admin".to_string(),
        root,
        readonly: false,
        skill_name: spec.name,
        scope: SkillFsScope::Admin,
    })
}

fn resolve_skill_target_path(root: &Path, relative_path: &str) -> Result<PathBuf, Response> {
    let normalized = normalize_relative_path(relative_path);
    if normalized.is_empty() || normalized == "." {
        return Ok(normalize_target_path(root));
    }
    let rel = Path::new(&normalized);
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
    let target = normalize_target_path(&root.join(rel));
    if !is_within_root(root, &target) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.path_out_of_bounds"),
        ));
    }
    Ok(target)
}

fn normalize_relative_path(value: &str) -> String {
    let trimmed = strip_windows_verbatim_prefix(value).replace('\\', "/");
    let trimmed = trimmed.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == "/" {
        return String::new();
    }
    let trimmed = trimmed.trim_start_matches('/');
    let normalized = Path::new(trimmed)
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().trim().to_string()),
            _ => None,
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    normalized
}

fn list_skill_entries(
    root: &Path,
    relative_path: &str,
    keyword: Option<&str>,
    offset: u64,
    limit: u64,
    sort_by: &str,
    order: &str,
) -> Result<(Vec<SkillFsEntry>, u64, String, Option<String>), Response> {
    let normalized = normalize_relative_path(relative_path);
    let target = resolve_skill_target_path(
        root,
        if normalized.is_empty() {
            "."
        } else {
            normalized.as_str()
        },
    )?;
    if !target.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("workspace.error.path_not_found"),
        ));
    }
    if !target.is_dir() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.path_not_dir"),
        ));
    }
    let keyword = keyword.unwrap_or("").trim().to_lowercase();
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&target)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let entry =
            entry.map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !keyword.is_empty() && !name.to_lowercase().contains(&keyword) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        entries.push((
            entry_from_path(root, &entry.path(), &metadata),
            modified_ts(&metadata),
        ));
    }
    sort_entries(&mut entries, sort_by, order);
    let total = entries.len() as u64;
    let safe_offset = offset as usize;
    let safe_limit = limit as usize;
    let entries = if safe_limit == 0 {
        entries
            .into_iter()
            .skip(safe_offset)
            .map(|(entry, _)| entry)
            .collect()
    } else {
        entries
            .into_iter()
            .skip(safe_offset)
            .take(safe_limit)
            .map(|(entry, _)| entry)
            .collect()
    };
    Ok((
        entries,
        total,
        normalized.clone(),
        get_parent_path(&normalized),
    ))
}

fn search_skill_entries(
    root: &Path,
    keyword: &str,
    offset: u64,
    limit: u64,
    include_files: bool,
    include_dirs: bool,
) -> Result<(Vec<SkillFsEntry>, u64), Response> {
    let keyword = keyword.trim().to_lowercase();
    let mut entries = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        let is_dir = entry.file_type().is_dir();
        if (is_dir && !include_dirs) || (!is_dir && !include_files) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !keyword.is_empty() && !name.to_lowercase().contains(&keyword) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        entries.push((
            entry_from_path(root, entry.path(), &metadata),
            modified_ts(&metadata),
        ));
    }
    sort_entries(&mut entries, "name", "asc");
    let total = entries.len() as u64;
    let safe_offset = offset as usize;
    let safe_limit = limit as usize;
    let entries = if safe_limit == 0 {
        entries
            .into_iter()
            .skip(safe_offset)
            .map(|(entry, _)| entry)
            .collect()
    } else {
        entries
            .into_iter()
            .skip(safe_offset)
            .take(safe_limit)
            .map(|(entry, _)| entry)
            .collect()
    };
    Ok((entries, total))
}

fn attach_children(
    root: &Path,
    entries: &mut [SkillFsContentEntry],
    remaining_depth: u64,
    sort_by: &str,
    order: &str,
) -> Result<(), Response> {
    if remaining_depth == 0 {
        return Ok(());
    }
    for entry in entries {
        if entry.entry_type != "dir" {
            continue;
        }
        let (children, _, _, _) =
            list_skill_entries(root, &entry.path, None, 0, 0, sort_by, order)?;
        let mut converted = children
            .into_iter()
            .map(SkillFsContentEntry::from)
            .collect::<Vec<_>>();
        if remaining_depth > 1 {
            attach_children(root, &mut converted, remaining_depth - 1, sort_by, order)?;
        }
        entry.children = converted;
    }
    Ok(())
}

fn entry_from_path(root: &Path, path: &Path, metadata: &std::fs::Metadata) -> SkillFsEntry {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    SkillFsEntry {
        name,
        path: to_relative_path(root, path),
        entry_type: if metadata.is_dir() { "dir" } else { "file" }.to_string(),
        size: if metadata.is_dir() { 0 } else { metadata.len() },
        updated_time: format_modified_time(metadata),
        children: None,
    }
}

impl From<SkillFsEntry> for SkillFsContentEntry {
    fn from(entry: SkillFsEntry) -> Self {
        Self {
            name: entry.name,
            path: entry.path,
            entry_type: entry.entry_type,
            size: entry.size,
            updated_time: entry.updated_time,
            children: Vec::new(),
        }
    }
}

fn sort_entries(entries: &mut Vec<(SkillFsEntry, f64)>, sort_by: &str, order: &str) {
    let sort_field = match sort_by {
        "size" | "updated_time" | "name" => sort_by,
        _ => "name",
    };
    let reverse = order.eq_ignore_ascii_case("desc");
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for payload in entries.drain(..) {
        if payload.0.entry_type == "dir" {
            dirs.push(payload);
        } else {
            files.push(payload);
        }
    }
    let sort_payloads = |items: &mut Vec<(SkillFsEntry, f64)>| match sort_field {
        "size" => items.sort_by(|a, b| a.0.size.cmp(&b.0.size)),
        "updated_time" => items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal)),
        _ => items.sort_by_key(|payload| payload.0.name.to_lowercase()),
    };
    sort_payloads(&mut dirs);
    sort_payloads(&mut files);
    if reverse {
        dirs.reverse();
        files.reverse();
    }
    entries.extend(dirs);
    entries.extend(files);
}

async fn remove_skill_entry(root: &Path, relative_path: &str) -> Result<(), Response> {
    let target = resolve_skill_target_path(root, relative_path)?;
    if target == normalize_target_path(root) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.delete_root_forbidden"),
        ));
    }
    if !target.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("workspace.error.path_not_found"),
        ));
    }
    if target.is_dir() {
        tokio::fs::remove_dir_all(&target).await
    } else {
        tokio::fs::remove_file(&target).await
    }
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
}

async fn move_skill_entry(root: &Path, source: &str, destination: &str) -> Result<(), Response> {
    if source.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.source_path_required"),
        ));
    }
    if destination.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.destination_path_required"),
        ));
    }
    if source == destination {
        return Ok(());
    }
    let source_path = resolve_skill_target_path(root, source)?;
    let destination_path = resolve_skill_target_path(root, destination)?;
    validate_source_destination(&source_path, &destination_path)?;
    if let Some(parent) = destination_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    tokio::fs::rename(&source_path, &destination_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
}

async fn copy_skill_entry(root: &Path, source: &str, destination: &str) -> Result<(), Response> {
    if source.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.source_path_required"),
        ));
    }
    if destination.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.destination_path_required"),
        ));
    }
    let source_path = resolve_skill_target_path(root, source)?;
    let destination_path = resolve_skill_target_path(root, destination)?;
    validate_source_destination(&source_path, &destination_path)?;
    if let Some(parent) = destination_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    if source_path.is_dir() {
        let source_path = source_path.clone();
        let destination_path = destination_path.clone();
        tokio::task::spawn_blocking(move || copy_dir_all(&source_path, &destination_path))
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
    } else {
        tokio::fs::copy(&source_path, &destination_path)
            .await
            .map(|_| ())
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
    }
}

fn validate_source_destination(source: &Path, destination: &Path) -> Result<(), Response> {
    if !source.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("workspace.error.source_not_found"),
        ));
    }
    if destination.exists() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.destination_exists"),
        ));
    }
    if source.is_dir() && destination.strip_prefix(source).is_ok() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.move_to_self_or_child"),
        ));
    }
    Ok(())
}

async fn refresh_skill_runtime_after_change(
    state: &AppState,
    resolved: &ResolvedSkillFs,
    paths: &[String],
    force: bool,
) {
    match resolved.scope {
        SkillFsScope::User => {
            if force || paths.iter().any(|path| should_clear_skill_cache(path)) {
                state
                    .user_tool_manager
                    .clear_skill_cache(Some(&resolved.user_id));
            }
        }
        SkillFsScope::Admin => {
            let config = state.config_store.get().await;
            state.reload_skills(&config).await;
        }
    }
}

fn should_clear_skill_cache(path: &str) -> bool {
    path.split('/')
        .last()
        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
}

fn skill_tree_version(root: &Path) -> u64 {
    std::fs::metadata(root)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn modified_ts(metadata: &std::fs::Metadata) -> f64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn format_modified_time(metadata: &std::fs::Metadata) -> String {
    metadata
        .modified()
        .ok()
        .map(|time| DateTime::<Local>::from(time).to_rfc3339())
        .unwrap_or_default()
}

fn get_parent_path(path: &str) -> Option<String> {
    let normalized = normalize_relative_path(path);
    if normalized.is_empty() {
        return None;
    }
    let parent = Path::new(&normalized)
        .parent()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    if parent.is_empty() || parent == "." {
        None
    } else {
        Some(parent)
    }
}

fn to_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

async fn save_multipart_file(
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

fn cleanup_temp_files(pending: &[PendingUpload], dir: Option<&PathBuf>) {
    for file in pending {
        let _ = std::fs::remove_file(&file.temp_path);
    }
    if let Some(dir) = dir {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn create_temp_upload_dir() -> Result<PathBuf, io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_skill_uploads");
    root.push(Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&root)?;
    Ok(root)
}

fn create_temp_archive_file() -> Result<PathBuf, io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_skill_archives");
    std::fs::create_dir_all(&root)?;
    let filename = format!("wunder_skill_{}.zip", Uuid::new_v4().simple());
    Ok(root.join(filename))
}

fn build_archive(archive_path: &Path, target: &Path, base_root: &Path) -> Result<(), io::Error> {
    let file = std::fs::File::create(archive_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    write_archive_entries(&mut zip, target, base_root, options)?;
    zip.finish()
        .map(|_| ())
        .map_err(|err| io::Error::other(err.to_string()))
}

fn write_archive_entries(
    zip: &mut zip::ZipWriter<std::fs::File>,
    target: &Path,
    base_root: &Path,
    options: FileOptions,
) -> Result<(), io::Error> {
    if target.is_file() {
        let rel = relative_zip_path(target, base_root);
        zip.start_file(rel, options)?;
        let mut file = std::fs::File::open(target)?;
        std::io::copy(&mut file, zip)?;
        return Ok(());
    }
    let mut file_count = 0;
    for entry in WalkDir::new(target).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let rel = relative_zip_path(path, base_root);
        zip.start_file(rel, options)?;
        let mut file = std::fs::File::open(path)?;
        std::io::copy(&mut file, zip)?;
        file_count += 1;
    }
    if file_count == 0 {
        let mut dir_rel = relative_zip_path(target, base_root);
        if !dir_rel.ends_with('/') {
            dir_rel.push('/');
        }
        if !dir_rel.is_empty() && dir_rel != "./" {
            zip.add_directory(dir_rel, options)?;
        }
    }
    Ok(())
}

fn relative_zip_path(path: &Path, base_root: &Path) -> String {
    path.strip_prefix(base_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), io::Error> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let new_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &new_path)?;
        } else {
            std::fs::copy(entry.path(), new_path)?;
        }
    }
    Ok(())
}

fn stream_response<S>(stream: S, filename: &str, content_type: &'static str) -> Response
where
    S: Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
{
    let disposition = build_content_disposition(filename);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

fn build_content_disposition(filename: &str) -> String {
    let ascii_name = sanitize_filename(filename);
    if ascii_name == filename {
        return format!("attachment; filename=\"{ascii_name}\"");
    }
    let encoded = percent_encode(filename);
    format!("attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{encoded}")
}

fn sanitize_filename(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.trim().is_empty() {
        "download".to_string()
    } else {
        output
    }
}

fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == '~' {
            output.push(ch);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn response_error_fallback(response: Response) -> String {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
        .unwrap_or_else(|| i18n::t("common.requestFailed"))
}

fn default_true() -> bool {
    true
}

fn default_sort_by() -> String {
    "name".to_string()
}

fn default_order() -> String {
    "asc".to_string()
}

fn default_search_limit() -> i64 {
    100
}
