// 工作区 API：文件读写、目录列表、上传下载与压缩。
use crate::state::AppState;
use axum::extract::{Multipart, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/workspace",
            get(list_workspace).delete(delete_workspace),
        )
        .route("/wunder/workspace/content", get(workspace_content))
        .route("/wunder/workspace/search", get(workspace_search))
        .route("/wunder/workspace/upload", post(workspace_upload))
        .route("/wunder/workspace/download", get(workspace_download))
        .route("/wunder/workspace/archive", get(workspace_archive))
        .route("/wunder/workspace/dir", post(workspace_dir))
        .route("/wunder/workspace/move", post(workspace_move))
        .route("/wunder/workspace/copy", post(workspace_copy))
        .route("/wunder/workspace/batch", post(workspace_batch))
        .route("/wunder/workspace/file", post(workspace_file))
}

#[derive(Debug, Deserialize)]
struct WorkspaceQuery {
    user_id: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    refresh_tree: Option<bool>,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    sort_by: Option<String>,
    #[serde(default)]
    order: Option<String>,
}

async fn list_workspace(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<WorkspaceListResponse>, Response> {
    let path = normalize_workspace_path(query.path.as_deref().unwrap_or(""));
    let user_id = query.user_id.clone();
    if let Err(err) = state.workspace.ensure_user_root(&user_id) {
        return Err(error_response(StatusCode::BAD_REQUEST, err.to_string()));
    }
    if query.refresh_tree.unwrap_or(false) {
        state.workspace.refresh_workspace_tree(&user_id);
    }
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(0);
    let sort_by = query.sort_by.as_deref().unwrap_or("name");
    let order = query.order.as_deref().unwrap_or("asc");
    let keyword = query.keyword.as_deref();
    let (entries, tree_version, current_path, parent, total) = state
        .workspace
        .list_workspace_entries(&user_id, &path, keyword, offset, limit, sort_by, order)
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
    Query(query): Query<WorkspaceContentQuery>,
) -> Result<Json<WorkspaceContentResponse>, Response> {
    let path = normalize_workspace_path(query.path.as_deref().unwrap_or(""));
    let include_content = query.include_content.unwrap_or(true);
    let max_bytes = query.max_bytes.unwrap_or(512 * 1024);
    let depth = query.depth.unwrap_or(1).max(1);
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(0);
    let sort_by = query.sort_by.as_deref().unwrap_or("name");
    let order = query.order.as_deref().unwrap_or("asc");
    let keyword = query.keyword.as_deref();

    if let Err(err) = state.workspace.ensure_user_root(&query.user_id) {
        return Err(error_response(StatusCode::BAD_REQUEST, err.to_string()));
    }
    let target = state
        .workspace
        .resolve_path(&query.user_id, if path.is_empty() { "." } else { &path })
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !target.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            crate::i18n::t("workspace.error.path_not_found"),
        ));
    }
    let updated_time = target
        .metadata()
        .ok()
        .and_then(|meta| meta.modified().ok())
        .map(|time| {
            let dt: DateTime<Local> = time.into();
            dt.to_rfc3339()
        })
        .unwrap_or_default();

    if target.is_dir() {
        let (mut entries, _tree_version, current_path, _parent, total) = state
            .workspace
            .list_workspace_entries(
                &query.user_id,
                &path,
                keyword,
                offset,
                limit,
                sort_by,
                order,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if include_content && depth > 1 {
            attach_workspace_children(
                &mut entries,
                &state.workspace,
                &query.user_id,
                depth,
                sort_by,
                order,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        let entries = if include_content { entries } else { Vec::new() };
        return Ok(Json(WorkspaceContentResponse {
            user_id: query.user_id,
            path: current_path,
            entry_type: "dir".to_string(),
            size: 0,
            updated_time,
            content: None,
            format: Some("dir".to_string()),
            truncated: false,
            entries: Some(entries),
            total,
            offset,
            limit,
        }));
    }

    let mut content = None;
    let mut truncated = false;
    if include_content {
        let file = std::fs::File::open(&target)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let mut buffer = Vec::new();
        let read_limit = max_bytes.saturating_add(1);
        file.take(read_limit as u64)
            .read_to_end(&mut buffer)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if buffer.len() > max_bytes {
            truncated = true;
            buffer.truncate(max_bytes);
        }
        content = Some(String::from_utf8_lossy(&buffer).to_string());
    }
    Ok(Json(WorkspaceContentResponse {
        user_id: query.user_id,
        path,
        entry_type: "file".to_string(),
        size: target.metadata().map(|meta| meta.len()).unwrap_or(0),
        updated_time,
        content,
        format: Some("text".to_string()),
        truncated,
        entries: None,
        total: 0,
        offset: 0,
        limit: 0,
    }))
}

async fn workspace_search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceSearchQuery>,
) -> Result<Json<WorkspaceSearchResponse>, Response> {
    let entries = state
        .workspace
        .search(&query.user_id, &query.keyword)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(WorkspaceSearchResponse {
        user_id: query.user_id,
        keyword: query.keyword,
        entries,
        total: 0,
        offset: 0,
        limit: 0,
    }))
}

async fn workspace_upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut user_id = String::new();
    let mut path = String::new();
    let mut saved = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "user_id" {
            user_id = field.text().await.unwrap_or_default();
            continue;
        }
        if name == "path" {
            path = field.text().await.unwrap_or_default();
            continue;
        }
        let filename = field.file_name().unwrap_or("upload.bin").to_string();
        let data = field
            .bytes()
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let target_path = if path.is_empty() {
            filename.clone()
        } else {
            format!("{}/{}", path.trim_end_matches('/'), filename)
        };
        state
            .workspace
            .write_file(
                &user_id,
                &target_path,
                &String::from_utf8_lossy(&data),
                true,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        saved.push(target_path);
    }
    Ok(Json(json!({
        "ok": true,
        "files": saved,
        "tree_version": state.workspace.tree_version(&user_id)
    })))
}

async fn workspace_download(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Response, Response> {
    let path = query.path.unwrap_or_default();
    let target = state
        .workspace
        .resolve_path(&query.user_id, &path)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let data = tokio::fs::read(&target)
        .await
        .map_err(|err| error_response(StatusCode::NOT_FOUND, err.to_string()))?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], data).into_response())
}

async fn workspace_archive(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Response, Response> {
    let data = state
        .workspace
        .archive(&query.user_id, query.path.as_deref())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(([(header::CONTENT_TYPE, "application/zip")], data).into_response())
}

async fn delete_workspace(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceQuery>,
) -> Result<Json<Value>, Response> {
    let path = query.path.unwrap_or_default();
    state
        .workspace
        .delete_path(&query.user_id, &path)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "message": "删除成功",
        "tree_version": state.workspace.tree_version(&query.user_id)
    })))
}

#[derive(Debug, Deserialize)]
struct WorkspaceDirRequest {
    user_id: String,
    path: String,
}

async fn workspace_dir(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WorkspaceDirRequest>,
) -> Result<Json<Value>, Response> {
    state
        .workspace
        .create_dir(&payload.user_id, &payload.path)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "message": "创建成功",
        "tree_version": state.workspace.tree_version(&payload.user_id),
        "files": [payload.path]
    })))
}

#[derive(Debug, Deserialize)]
struct WorkspaceMoveRequest {
    user_id: String,
    source: String,
    destination: String,
}

async fn workspace_move(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WorkspaceMoveRequest>,
) -> Result<Json<Value>, Response> {
    state
        .workspace
        .move_path(&payload.user_id, &payload.source, &payload.destination)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "message": "移动成功",
        "tree_version": state.workspace.tree_version(&payload.user_id),
        "files": [payload.destination]
    })))
}

async fn workspace_copy(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WorkspaceMoveRequest>,
) -> Result<Json<Value>, Response> {
    state
        .workspace
        .copy_path(&payload.user_id, &payload.source, &payload.destination)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "message": "复制成功",
        "tree_version": state.workspace.tree_version(&payload.user_id),
        "files": [payload.destination]
    })))
}

#[derive(Debug, Deserialize)]
struct WorkspaceBatchRequest {
    user_id: String,
    action: String,
    paths: Vec<String>,
    #[serde(default)]
    destination: Option<String>,
}

async fn workspace_batch(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WorkspaceBatchRequest>,
) -> Result<Json<Value>, Response> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for path in payload.paths {
        let result = match payload.action.as_str() {
            "delete" => state.workspace.delete_path(&payload.user_id, &path),
            "move" => {
                if let Some(dest) = &payload.destination {
                    state.workspace.move_path(&payload.user_id, &path, dest)
                } else {
                    Err(anyhow::anyhow!("缺少 destination"))
                }
            }
            "copy" => {
                if let Some(dest) = &payload.destination {
                    state.workspace.copy_path(&payload.user_id, &path, dest)
                } else {
                    Err(anyhow::anyhow!("缺少 destination"))
                }
            }
            _ => Err(anyhow::anyhow!("未知操作")),
        };
        match result {
            Ok(_) => succeeded.push(path),
            Err(err) => failed.push(json!({"path": path, "message": err.to_string()})),
        }
    }
    Ok(Json(json!({
        "ok": failed.is_empty(),
        "message": "批量操作完成",
        "tree_version": state.workspace.tree_version(&payload.user_id),
        "succeeded": succeeded,
        "failed": failed
    })))
}

#[derive(Debug, Deserialize)]
struct WorkspaceFileRequest {
    user_id: String,
    path: String,
    content: String,
    #[serde(default)]
    create_if_missing: Option<bool>,
}

async fn workspace_file(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WorkspaceFileRequest>,
) -> Result<Json<Value>, Response> {
    let create = payload.create_if_missing.unwrap_or(false);
    if let Err(err) =
        state
            .workspace
            .write_file(&payload.user_id, &payload.path, &payload.content, create)
    {
        let message = err.to_string();
        let status = if message.contains("不存在") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        return Err(error_response(status, message));
    }
    Ok(Json(json!({
        "ok": true,
        "message": "保存成功",
        "tree_version": state.workspace.tree_version(&payload.user_id),
        "files": [payload.path]
    })))
}

fn attach_workspace_children(
    entries: &mut [crate::workspace::WorkspaceEntry],
    manager: &crate::workspace::WorkspaceManager,
    user_id: &str,
    depth: u64,
    sort_by: &str,
    order: &str,
) -> anyhow::Result<()> {
    if depth <= 1 {
        return Ok(());
    }
    for entry in entries.iter_mut() {
        if entry.entry_type != "dir" {
            continue;
        }
        let (mut children, _, _, _, _) =
            manager.list_workspace_entries(user_id, &entry.path, None, 0, 0, sort_by, order)?;
        attach_workspace_children(&mut children, manager, user_id, depth - 1, sort_by, order)?;
        entry.children = Some(children);
    }
    Ok(())
}

fn normalize_workspace_path(value: &str) -> String {
    let trimmed = value.replace('\\', "/");
    let trimmed = trimmed.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == "/" {
        return String::new();
    }
    trimmed.trim_start_matches('/').to_string()
}

fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "detail": { "message": message } }))).into_response()
}

#[derive(Debug, Deserialize)]
struct WorkspaceContentQuery {
    user_id: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    include_content: Option<bool>,
    #[serde(default)]
    max_bytes: Option<usize>,
    #[serde(default)]
    depth: Option<u64>,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    sort_by: Option<String>,
    #[serde(default)]
    order: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceSearchQuery {
    user_id: String,
    keyword: String,
}

#[derive(Debug, Serialize)]
struct WorkspaceListResponse {
    user_id: String,
    path: String,
    parent: Option<String>,
    entries: Vec<crate::workspace::WorkspaceEntry>,
    tree_version: u64,
    total: u64,
    offset: u64,
    limit: u64,
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
    format: Option<String>,
    truncated: bool,
    entries: Option<Vec<crate::workspace::WorkspaceEntry>>,
    total: u64,
    offset: u64,
    limit: u64,
}

#[derive(Debug, Serialize)]
struct WorkspaceSearchResponse {
    user_id: String,
    keyword: String,
    entries: Vec<crate::workspace::WorkspaceEntry>,
    total: u64,
    offset: u64,
    limit: u64,
}
