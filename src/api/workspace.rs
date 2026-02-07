use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::state::AppState;
use crate::workspace::WorkspaceEntry;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use chrono::{DateTime, Local};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::future::Future;
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

const MAX_WORKSPACE_UPLOAD_BYTES: usize = 200 * 1024 * 1024;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/workspace",
            get(workspace_list).delete(workspace_delete),
        )
        .route("/wunder/workspace/content", get(workspace_content))
        .route("/wunder/workspace/search", get(workspace_search))
        .route(
            "/wunder/workspace/upload",
            post(workspace_upload).layer(DefaultBodyLimit::max(MAX_WORKSPACE_UPLOAD_BYTES)),
        )
        .route("/wunder/workspace/dir", post(workspace_dir))
        .route("/wunder/workspace/move", post(workspace_move))
        .route("/wunder/workspace/copy", post(workspace_copy))
        .route("/wunder/workspace/batch", post(workspace_batch))
        .route("/wunder/workspace/file", post(workspace_file_update))
        .route("/wunder/workspace/archive", get(workspace_archive))
        .route("/wunder/workspace/download", get(workspace_download))
}

async fn workspace_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<WorkspaceListQuery>,
) -> Result<Json<WorkspaceListResponse>, Response> {
    let resolved = resolve_user(&state, &headers, params.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(params.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    let _root = state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if params.refresh_tree {
        state.workspace.refresh_workspace_tree(&workspace_id);
    }
    let normalized = normalize_relative_path(&params.path);
    let target_path = if normalized.is_empty() {
        "."
    } else {
        normalized.as_str()
    };
    let keyword = if params.keyword.trim().is_empty() {
        None
    } else {
        Some(params.keyword.trim())
    };
    let offset = params.offset.max(0) as u64;
    let limit = params.limit.max(0) as u64;
    let (entries, tree_version, current_path, parent, total) = state
        .workspace
        .list_workspace_entries_async(
            &workspace_id,
            target_path,
            keyword,
            offset,
            limit,
            params.sort_by.trim(),
            params.order.trim(),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(WorkspaceListResponse {
        user_id,
        path: current_path,
        parent,
        entries,
        tree_version,
        total,
        offset,
        limit,
    }))
}

async fn workspace_content(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<WorkspaceContentQuery>,
) -> Result<Json<WorkspaceContentResponse>, Response> {
    let resolved = resolve_user(&state, &headers, params.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(params.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    let _root = state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let normalized = normalize_relative_path(&params.path);
    let target_path = if normalized.is_empty() {
        ".".to_string()
    } else {
        normalized.clone()
    };
    let target = state
        .workspace
        .resolve_path(&workspace_id, &target_path)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
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
    let safe_depth = params.depth.max(1) as u64;
    let safe_offset = params.offset.max(0) as u64;
    let safe_limit = params.limit.max(0) as u64;

    if metadata.is_dir() {
        let keyword = if params.keyword.trim().is_empty() {
            None
        } else {
            Some(params.keyword.trim())
        };
        let (entries, _tree_version, current_path, _parent, total) = state
            .workspace
            .list_workspace_entries_async(
                &workspace_id,
                &target_path,
                keyword,
                safe_offset,
                safe_limit,
                params.sort_by.trim(),
                params.order.trim(),
            )
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let mut content_entries = entries
            .into_iter()
            .map(WorkspaceContentEntry::from)
            .collect::<Vec<_>>();
        if params.include_content && safe_depth > 1 {
            attach_children(
                &state,
                &workspace_id,
                &mut content_entries,
                safe_depth - 1,
                params.sort_by.trim(),
                params.order.trim(),
            )
            .await?;
        }
        let entries = if params.include_content {
            content_entries
        } else {
            Vec::new()
        };
        return Ok(Json(WorkspaceContentResponse {
            user_id,
            path: current_path,
            entry_type: "dir".to_string(),
            size: 0,
            updated_time,
            content: None,
            format_value: "dir".to_string(),
            truncated: false,
            entries,
            total,
            offset: safe_offset,
            limit: safe_limit,
        }));
    }

    let mut content = None;
    let mut truncated = false;
    if params.include_content {
        let mut file = tokio::fs::File::open(&target)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let mut buffer = Vec::new();
        let max_bytes = params.max_bytes.max(0) as usize;
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
    Ok(Json(WorkspaceContentResponse {
        user_id,
        path: normalized,
        entry_type: "file".to_string(),
        size: metadata.len(),
        updated_time,
        content,
        format_value: "text".to_string(),
        truncated,
        entries: Vec::new(),
        total: 0,
        offset: 0,
        limit: 0,
    }))
}

async fn workspace_search(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<WorkspaceSearchQuery>,
) -> Result<Json<WorkspaceSearchResponse>, Response> {
    let resolved = resolve_user(&state, &headers, params.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(params.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let offset = params.offset.max(0) as u64;
    let limit = params.limit.max(0) as u64;
    let (entries, total) = state
        .workspace
        .search_workspace_entries_async(
            &workspace_id,
            &params.keyword,
            offset,
            limit,
            params.include_files,
            params.include_dirs,
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(WorkspaceSearchResponse {
        user_id,
        keyword: params.keyword,
        entries,
        total,
        offset,
        limit,
    }))
}
async fn workspace_upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<WorkspaceActionResponse>, Response> {
    if let Some(length) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        if length > MAX_WORKSPACE_UPLOAD_BYTES as u64 {
            return Err(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                i18n::t("workspace.error.upload_too_large"),
            ));
        }
    }

    let mut raw_user_id = String::new();
    let mut raw_agent_id = String::new();
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
                if !value.trim().is_empty() {
                    raw_user_id = value.trim().to_string();
                }
            }
            "agent_id" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                if !value.trim().is_empty() {
                    raw_agent_id = value.trim().to_string();
                }
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
                if !value.trim().is_empty() {
                    relative_paths.push(value.trim().to_string());
                }
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

    let resolved = resolve_user(
        &state,
        &headers,
        if raw_user_id.trim().is_empty() {
            None
        } else {
            Some(raw_user_id.as_str())
        },
    )
    .await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(Some(raw_agent_id.as_str()));
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    let root = state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let normalized_base = normalize_relative_path(&base_path);
    let target_path = if normalized_base.is_empty() {
        ".".to_string()
    } else {
        normalized_base.clone()
    };
    let target_dir = match state.workspace.resolve_path(&workspace_id, &target_path) {
        Ok(path) => path,
        Err(err) => {
            cleanup_temp_files(&pending_files, temp_dir.as_ref());
            return Err(error_response(StatusCode::BAD_REQUEST, err.to_string()));
        }
    };
    if target_dir.exists() && !target_dir.is_dir() {
        cleanup_temp_files(&pending_files, temp_dir.as_ref());
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.target_not_dir"),
        ));
    }
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|err| {
            cleanup_temp_files(&pending_files, temp_dir.as_ref());
            error_response(StatusCode::BAD_REQUEST, err.to_string())
        })?;

    let mut uploaded = Vec::new();
    for (index, file) in pending_files.iter().enumerate() {
        let raw_path = relative_paths
            .get(index)
            .cloned()
            .unwrap_or_else(|| file.filename.clone());
        let normalized = raw_path.replace('\\', "/").trim().to_string();
        let normalized = normalized.trim_start_matches('/').to_string();
        if normalized.is_empty() {
            continue;
        }
        let joined = if normalized_base.is_empty() {
            normalized.clone()
        } else {
            Path::new(&normalized_base)
                .join(&normalized)
                .to_string_lossy()
                .replace('\\', "/")
        };
        let dest = match state.workspace.resolve_path(&workspace_id, &joined) {
            Ok(path) => path,
            Err(err) => {
                cleanup_temp_files(&pending_files, temp_dir.as_ref());
                return Err(error_response(StatusCode::BAD_REQUEST, err.to_string()));
            }
        };
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|err| {
                cleanup_temp_files(&pending_files, temp_dir.as_ref());
                error_response(StatusCode::BAD_REQUEST, err.to_string())
            })?;
        }
        if dest.exists() && dest.is_dir() {
            cleanup_temp_files(&pending_files, temp_dir.as_ref());
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
                    cleanup_temp_files(&pending_files, temp_dir.as_ref());
                    error_response(StatusCode::BAD_REQUEST, err.to_string())
                })?;
            let _ = tokio::fs::remove_file(&file.temp_path).await;
        }
        if let Ok(relative) = dest.strip_prefix(&root) {
            uploaded.push(relative.to_string_lossy().replace('\\', "/"));
        } else {
            uploaded.push(joined);
        }
    }

    state.workspace.refresh_workspace_tree(&workspace_id);
    let tree_version = state.workspace.get_tree_version(&workspace_id);
    cleanup_temp_files(&pending_files, temp_dir.as_ref());
    Ok(Json(WorkspaceActionResponse {
        ok: true,
        message: i18n::t("message.upload_success"),
        tree_version,
        files: uploaded,
    }))
}
async fn workspace_dir(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<WorkspaceDirRequest>,
) -> Result<Json<WorkspaceActionResponse>, Response> {
    let normalized = normalize_relative_path(&request.path);
    if normalized.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.dir_path_required"),
        ));
    }
    let resolved = resolve_user(&state, &headers, request.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(request.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target_dir = state
        .workspace
        .resolve_path(&workspace_id, &normalized)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if target_dir.exists() && !target_dir.is_dir() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.target_exists_not_dir"),
        ));
    }
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state.workspace.refresh_workspace_tree(&workspace_id);
    let tree_version = state.workspace.get_tree_version(&workspace_id);
    Ok(Json(WorkspaceActionResponse {
        ok: true,
        message: i18n::t("workspace.message.dir_created"),
        tree_version,
        files: vec![normalized],
    }))
}

async fn workspace_move(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<WorkspaceMoveRequest>,
) -> Result<Json<WorkspaceActionResponse>, Response> {
    let source = normalize_relative_path(&request.source);
    let destination = normalize_relative_path(&request.destination);
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
    let resolved = resolve_user(&state, &headers, request.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(request.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if source == destination {
        let tree_version = state.workspace.get_tree_version(&workspace_id);
        return Ok(Json(WorkspaceActionResponse {
            ok: true,
            message: i18n::t("workspace.message.path_unchanged"),
            tree_version,
            files: vec![destination],
        }));
    }
    let source_path = state
        .workspace
        .resolve_path(&workspace_id, &source)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let destination_path = state
        .workspace
        .resolve_path(&workspace_id, &destination)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !source_path.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("workspace.error.source_not_found"),
        ));
    }
    if destination_path.exists() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.destination_exists"),
        ));
    }
    if let Some(parent) = destination_path.parent() {
        if !parent.exists() || !parent.is_dir() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("workspace.error.destination_parent_missing"),
            ));
        }
    }
    if source_path.is_dir() {
        if destination_path.strip_prefix(&source_path).is_ok() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("workspace.error.move_to_self_or_child"),
            ));
        }
    }
    tokio::fs::rename(&source_path, &destination_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state.workspace.refresh_workspace_tree(&workspace_id);
    let tree_version = state.workspace.get_tree_version(&workspace_id);
    Ok(Json(WorkspaceActionResponse {
        ok: true,
        message: i18n::t("workspace.message.moved"),
        tree_version,
        files: vec![destination],
    }))
}
async fn workspace_copy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<WorkspaceCopyRequest>,
) -> Result<Json<WorkspaceActionResponse>, Response> {
    let source = normalize_relative_path(&request.source);
    let destination = normalize_relative_path(&request.destination);
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
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.source_destination_same"),
        ));
    }
    let resolved = resolve_user(&state, &headers, request.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(request.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let source_path = state
        .workspace
        .resolve_path(&workspace_id, &source)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let destination_path = state
        .workspace
        .resolve_path(&workspace_id, &destination)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !source_path.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("workspace.error.source_not_found"),
        ));
    }
    if destination_path.exists() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.destination_exists"),
        ));
    }
    if let Some(parent) = destination_path.parent() {
        if !parent.exists() || !parent.is_dir() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("workspace.error.destination_parent_missing"),
            ));
        }
    }
    if source_path.is_dir() && destination_path.strip_prefix(&source_path).is_ok() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.copy_to_self_or_child"),
        ));
    }
    if source_path.is_dir() {
        let source_path = source_path.clone();
        let destination_path = destination_path.clone();
        tokio::task::spawn_blocking(move || copy_dir_all(&source_path, &destination_path))
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    } else {
        tokio::fs::copy(&source_path, &destination_path)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    state.workspace.refresh_workspace_tree(&workspace_id);
    let tree_version = state.workspace.get_tree_version(&workspace_id);
    Ok(Json(WorkspaceActionResponse {
        ok: true,
        message: i18n::t("workspace.message.copied"),
        tree_version,
        files: vec![destination],
    }))
}

async fn workspace_batch(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<WorkspaceBatchRequest>,
) -> Result<Json<WorkspaceBatchResponse>, Response> {
    if request.paths.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.batch_paths_missing"),
        ));
    }
    let resolved = resolve_user(&state, &headers, request.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(request.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    let root = state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let action = request.action.trim().to_string();
    let mut destination_path = None;
    if action == "move" || action == "copy" {
        let destination_root =
            normalize_relative_path(request.destination.as_deref().unwrap_or(""));
        let resolved = state
            .workspace
            .resolve_path(
                &workspace_id,
                if destination_root.is_empty() {
                    "."
                } else {
                    &destination_root
                },
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if !resolved.exists() || !resolved.is_dir() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("workspace.error.destination_dir_missing"),
            ));
        }
        destination_path = Some(resolved);
    }

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for raw_path in &request.paths {
        let normalized = normalize_relative_path(raw_path);
        if normalized.is_empty() {
            failed.push(WorkspaceBatchFailure {
                path: raw_path.clone(),
                message: i18n::t("workspace.error.path_required"),
            });
            continue;
        }
        let source_path = match state.workspace.resolve_path(&workspace_id, &normalized) {
            Ok(path) => path,
            Err(err) => {
                failed.push(WorkspaceBatchFailure {
                    path: normalized.clone(),
                    message: err.to_string(),
                });
                continue;
            }
        };
        if !source_path.exists() {
            failed.push(WorkspaceBatchFailure {
                path: normalized.clone(),
                message: i18n::t("workspace.error.path_not_found"),
            });
            continue;
        }

        if action == "delete" {
            let result = if source_path.is_dir() {
                tokio::fs::remove_dir_all(&source_path).await
            } else {
                tokio::fs::remove_file(&source_path).await
            };
            if let Err(err) = result {
                failed.push(WorkspaceBatchFailure {
                    path: normalized.clone(),
                    message: err.to_string(),
                });
            } else {
                succeeded.push(normalized.clone());
            }
            continue;
        }

        if action != "move" && action != "copy" {
            failed.push(WorkspaceBatchFailure {
                path: normalized.clone(),
                message: i18n::t("workspace.error.batch_action_unsupported"),
            });
            continue;
        }

        let destination_base = match destination_path.as_ref() {
            Some(path) => path,
            None => {
                failed.push(WorkspaceBatchFailure {
                    path: normalized.clone(),
                    message: i18n::t("workspace.error.destination_unready"),
                });
                continue;
            }
        };
        let entry_name = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("item");
        let target_path = destination_base.join(entry_name);
        if target_path.exists() {
            failed.push(WorkspaceBatchFailure {
                path: normalized.clone(),
                message: i18n::t("workspace.error.destination_exists"),
            });
            continue;
        }
        if source_path.is_dir() && target_path.strip_prefix(&source_path).is_ok() {
            failed.push(WorkspaceBatchFailure {
                path: normalized.clone(),
                message: i18n::t("workspace.error.move_to_self_or_child"),
            });
            continue;
        }
        let result = if action == "move" {
            tokio::fs::rename(&source_path, &target_path).await
        } else if source_path.is_dir() {
            let source_path = source_path.clone();
            let target_path = target_path.clone();
            tokio::task::spawn_blocking(move || copy_dir_all(&source_path, &target_path))
                .await
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
                .and_then(|result| result)
        } else {
            tokio::fs::copy(&source_path, &target_path)
                .await
                .map(|_| ())
        };
        if let Err(err) = result {
            failed.push(WorkspaceBatchFailure {
                path: normalized.clone(),
                message: err.to_string(),
            });
            continue;
        }
        if let Ok(relative) = target_path.strip_prefix(&root) {
            succeeded.push(relative.to_string_lossy().replace('\\', "/"));
        } else {
            succeeded.push(target_path.to_string_lossy().replace('\\', "/"));
        }
    }

    state.workspace.refresh_workspace_tree(&workspace_id);
    let tree_version = state.workspace.get_tree_version(&workspace_id);
    let ok = failed.is_empty();
    let message = if ok {
        i18n::t("workspace.message.batch_success")
    } else {
        i18n::t("workspace.message.batch_partial")
    };
    Ok(Json(WorkspaceBatchResponse {
        ok,
        message,
        tree_version,
        succeeded,
        failed,
    }))
}
async fn workspace_file_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<WorkspaceFileUpdateRequest>,
) -> Result<Json<WorkspaceActionResponse>, Response> {
    let normalized = normalize_relative_path(&request.path);
    if normalized.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.file_path_required"),
        ));
    }
    let resolved = resolve_user(&state, &headers, request.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(request.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = state
        .workspace
        .resolve_path(&workspace_id, &normalized)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if target.exists() {
        if !target.is_file() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("workspace.error.target_not_file"),
            ));
        }
    } else if !request.create_if_missing {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.file_not_found"),
        ));
    } else if let Some(parent) = target.parent() {
        if !parent.exists() || !parent.is_dir() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("workspace.error.destination_parent_missing"),
            ));
        }
    }
    tokio::fs::write(&target, request.content.as_bytes())
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state.workspace.refresh_workspace_tree(&workspace_id);
    let tree_version = state.workspace.get_tree_version(&workspace_id);
    Ok(Json(WorkspaceActionResponse {
        ok: true,
        message: i18n::t("workspace.message.file_saved"),
        tree_version,
        files: vec![normalized],
    }))
}

async fn workspace_archive(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<WorkspaceArchiveQuery>,
) -> Result<Response, Response> {
    let resolved = resolve_user(&state, &headers, params.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(params.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    let root = state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !root.exists() || !root.is_dir() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("workspace.error.workspace_not_found"),
        ));
    }
    let normalized = normalize_relative_path(&params.path.unwrap_or_default());
    let (target, base_root, filename_prefix) = if normalized.is_empty() {
        (
            root.clone(),
            root.clone(),
            format!("workspace_{workspace_id}"),
        )
    } else {
        let target = state
            .workspace
            .resolve_path(&workspace_id, &normalized)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if !target.exists() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                i18n::t("workspace.error.path_not_found"),
            ));
        }
        let base_root = target
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        let filename_prefix = target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
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

async fn workspace_download(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<WorkspaceDownloadQuery>,
) -> Result<Response, Response> {
    let resolved = resolve_user(&state, &headers, params.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(params.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    let normalized = normalize_relative_path(&params.path);
    if normalized.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.path_required"),
        ));
    }
    state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = state
        .workspace
        .resolve_path(&workspace_id, &normalized)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
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
    let stream = ReaderStream::new(file);
    Ok(stream_response(
        stream,
        filename,
        "application/octet-stream",
    ))
}

async fn workspace_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<WorkspaceDeleteQuery>,
) -> Result<Json<WorkspaceActionResponse>, Response> {
    let resolved = resolve_user(&state, &headers, params.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let agent_id = normalize_agent_id(params.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id);
    let normalized = normalize_relative_path(&params.path);
    if normalized.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.delete_root_forbidden"),
        ));
    }
    state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = state
        .workspace
        .resolve_path(&workspace_id, &normalized)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !target.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("workspace.error.path_not_found"),
        ));
    }
    if target.is_dir() {
        tokio::fs::remove_dir_all(&target)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    } else {
        tokio::fs::remove_file(&target)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    state.workspace.refresh_workspace_tree(&workspace_id);
    let tree_version = state.workspace.get_tree_version(&workspace_id);
    Ok(Json(WorkspaceActionResponse {
        ok: true,
        message: i18n::t("message.deleted"),
        tree_version,
        files: Vec::new(),
    }))
}
fn attach_children<'a>(
    state: &'a Arc<AppState>,
    workspace_id: &'a str,
    entries: &'a mut [WorkspaceContentEntry],
    remaining_depth: u64,
    sort_by: &'a str,
    order: &'a str,
) -> Pin<Box<dyn Future<Output = Result<(), Response>> + Send + 'a>> {
    Box::pin(async move {
        if remaining_depth == 0 {
            return Ok(());
        }
        for entry in entries {
            if entry.entry_type != "dir" {
                continue;
            }
            let (children, _tree_version, _current, _parent, _total) = state
                .workspace
                .list_workspace_entries_async(workspace_id, &entry.path, None, 0, 0, sort_by, order)
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            let mut converted = children
                .into_iter()
                .map(WorkspaceContentEntry::from)
                .collect::<Vec<_>>();
            if remaining_depth > 1 {
                attach_children(
                    state,
                    workspace_id,
                    &mut converted,
                    remaining_depth - 1,
                    sort_by,
                    order,
                )
                .await?;
            }
            entry.children = converted;
        }
        Ok(())
    })
}

fn normalize_relative_path(value: &str) -> String {
    let trimmed = value.replace('\\', "/");
    let trimmed = trimmed.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == "/" {
        return String::new();
    }
    trimmed.trim_start_matches('/').to_string()
}

fn normalize_agent_id(value: Option<&str>) -> Option<&str> {
    value
        .map(|raw| raw.trim())
        .filter(|trimmed| !trimmed.is_empty())
}

fn resolve_workspace_id(state: &AppState, user_id: &str, agent_id: Option<&str>) -> String {
    if let Some(container_id) = state
        .user_store
        .resolve_agent_sandbox_container_id(agent_id)
    {
        return state
            .workspace
            .scoped_user_id_by_container(user_id, container_id);
    }
    state.workspace.scoped_user_id(user_id, agent_id)
}

fn format_modified_time(metadata: &std::fs::Metadata) -> String {
    metadata
        .modified()
        .ok()
        .map(|time| DateTime::<Local>::from(time).to_rfc3339())
        .unwrap_or_default()
}

fn create_temp_upload_dir() -> Result<PathBuf, io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_uploads");
    root.push(Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&root)?;
    Ok(root)
}

fn create_temp_archive_file() -> Result<PathBuf, io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_workspace");
    std::fs::create_dir_all(&root)?;
    let filename = format!("wunder_workspace_{}.zip", Uuid::new_v4().simple());
    Ok(root.join(filename))
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

fn build_archive(archive_path: &Path, target: &Path, base_root: &Path) -> Result<(), io::Error> {
    let file = std::fs::File::create(archive_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    write_archive_entries(&mut zip, target, base_root, options)?;
    zip.finish()
        .map(|_| ())
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
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

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
#[derive(Debug, Deserialize)]
struct WorkspaceListQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    refresh_tree: bool,
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
struct WorkspaceContentQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    path: String,
    #[serde(default = "default_true")]
    include_content: bool,
    #[serde(default = "default_max_bytes")]
    max_bytes: i64,
    #[serde(default = "default_depth")]
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
struct WorkspaceSearchQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
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
struct WorkspaceDownloadQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    path: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceArchiveQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceDeleteQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    path: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceDirRequest {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    path: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceMoveRequest {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    source: String,
    destination: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceCopyRequest {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    source: String,
    destination: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceBatchRequest {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    action: String,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    destination: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceFileUpdateRequest {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    path: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    create_if_missing: bool,
}

#[derive(Debug, Serialize)]
struct WorkspaceListResponse {
    user_id: String,
    path: String,
    parent: Option<String>,
    entries: Vec<WorkspaceEntry>,
    tree_version: u64,
    total: u64,
    offset: u64,
    limit: u64,
}

#[derive(Debug, Serialize)]
struct WorkspaceActionResponse {
    ok: bool,
    message: String,
    tree_version: u64,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct WorkspaceContentResponse {
    user_id: String,
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
    size: u64,
    updated_time: String,
    content: Option<String>,
    #[serde(rename = "format")]
    format_value: String,
    truncated: bool,
    #[serde(default)]
    entries: Vec<WorkspaceContentEntry>,
    total: u64,
    offset: u64,
    limit: u64,
}

#[derive(Debug, Serialize)]
struct WorkspaceSearchResponse {
    user_id: String,
    keyword: String,
    entries: Vec<WorkspaceEntry>,
    total: u64,
    offset: u64,
    limit: u64,
}

#[derive(Debug, Serialize)]
struct WorkspaceBatchResponse {
    ok: bool,
    message: String,
    tree_version: u64,
    #[serde(default)]
    succeeded: Vec<String>,
    #[serde(default)]
    failed: Vec<WorkspaceBatchFailure>,
}

#[derive(Debug, Serialize)]
struct WorkspaceBatchFailure {
    path: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct WorkspaceContentEntry {
    name: String,
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
    size: u64,
    updated_time: String,
    #[serde(default)]
    children: Vec<WorkspaceContentEntry>,
}

impl From<WorkspaceEntry> for WorkspaceContentEntry {
    fn from(entry: WorkspaceEntry) -> Self {
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

struct PendingUpload {
    temp_path: PathBuf,
    filename: String,
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

fn default_true() -> bool {
    true
}

fn default_sort_by() -> String {
    "name".to_string()
}

fn default_order() -> String {
    "asc".to_string()
}

fn default_max_bytes() -> i64 {
    512 * 1024
}

fn default_depth() -> i64 {
    1
}

fn default_search_limit() -> i64 {
    100
}
