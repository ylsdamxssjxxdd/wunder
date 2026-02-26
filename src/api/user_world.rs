use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use bytes::Bytes;
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;
use walkdir::WalkDir;
use zip::write::FileOptions;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 500;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/user_world/contacts", get(list_contacts))
        .route(
            "/wunder/user_world/groups",
            get(list_groups).post(create_group),
        )
        .route(
            "/wunder/user_world/conversations",
            get(list_conversations).post(create_or_get_conversation),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}",
            get(get_conversation),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}/messages",
            get(list_messages).post(send_message),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}/read",
            post(mark_read),
        )
        .route(
            "/wunder/user_world/files/download",
            get(download_user_world_file),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}/events",
            get(stream_events),
        )
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    keyword: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConversationCreateRequest {
    peer_user_id: String,
}

#[derive(Debug, Deserialize)]
struct GroupCreateRequest {
    group_name: String,
    #[serde(default)]
    member_user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ListMessageQuery {
    #[serde(default)]
    before_message_id: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SendMessageRequest {
    content: String,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    client_msg_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MarkReadRequest {
    #[serde(default)]
    last_read_message_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct EventQuery {
    #[serde(default)]
    after_event_id: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UserWorldFileDownloadQuery {
    conversation_id: String,
    #[serde(default)]
    owner_user_id: Option<String>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    check: Option<bool>,
}

async fn list_contacts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let (offset, limit) = normalize_pagination(query.offset, query.limit);
    let keyword = query.keyword.as_deref();
    let (items, total) = state
        .user_world
        .list_contacts(&resolved.user.user_id, keyword, offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit
        }
    })))
}

async fn create_or_get_conversation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ConversationCreateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let item = state
        .user_world
        .resolve_or_create_direct_conversation(
            &resolved.user.user_id,
            payload.peer_user_id.trim(),
            now_ts(),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": item })))
}

async fn list_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let (offset, limit) = normalize_pagination(query.offset, query.limit);
    let (items, total) = state
        .user_world
        .list_groups(&resolved.user.user_id, offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit
        }
    })))
}

async fn create_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<GroupCreateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let item = state
        .user_world
        .create_group(
            &resolved.user.user_id,
            payload.group_name.trim(),
            &payload.member_user_ids,
            now_ts(),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": item })))
}

async fn list_conversations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let (offset, limit) = normalize_pagination(query.offset, query.limit);
    let (items, total) = state
        .user_world
        .list_conversations(&resolved.user.user_id, offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit
        }
    })))
}

async fn get_conversation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let item = state
        .user_world
        .get_conversation(&resolved.user.user_id, conversation_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(item) = item else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "conversation not found",
        ));
    };
    Ok(Json(json!({ "data": item })))
}

async fn list_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Query(query): Query<ListMessageQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let limit = normalize_limit(query.limit.unwrap_or(DEFAULT_LIMIT));
    let items = state
        .user_world
        .list_messages(
            &resolved.user.user_id,
            conversation_id.trim(),
            query.before_message_id,
            limit,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items,
            "limit": limit
        }
    })))
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let content = payload.content.trim();
    if content.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "content is required",
        ));
    }
    let result = state
        .user_world
        .send_message(
            &resolved.user.user_id,
            conversation_id.trim(),
            content,
            payload.content_type.as_deref().unwrap_or("text"),
            payload.client_msg_id.as_deref(),
            now_ts(),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": result })))
}

async fn mark_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Json(payload): Json<MarkReadRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let result = state
        .user_world
        .mark_read(
            &resolved.user.user_id,
            conversation_id.trim(),
            payload.last_read_message_id,
            now_ts(),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": result })))
}

async fn stream_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Query(query): Query<EventQuery>,
) -> Result<Sse<ReceiverStream<Result<Event, Infallible>>>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let conversation_id = conversation_id.trim().to_string();
    if conversation_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "conversation_id is required",
        ));
    }
    let after_event_id = query.after_event_id.unwrap_or(0).max(0);
    let fetch_limit = normalize_limit(query.limit.unwrap_or(DEFAULT_LIMIT));
    let user_id = resolved.user.user_id.clone();
    let service = state.user_world.clone();
    service
        .list_events(&user_id, &conversation_id, after_event_id, 1)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        let mut current_event_id = after_event_id;
        loop {
            let events = service
                .list_events(&user_id, &conversation_id, current_event_id, fetch_limit)
                .unwrap_or_default();
            for item in events {
                current_event_id = current_event_id.max(item.event_id);
                let payload =
                    serde_json::to_string(&item.payload).unwrap_or_else(|_| "null".to_string());
                let event = Event::default()
                    .event(item.event_type)
                    .id(item.event_id.to_string())
                    .data(payload);
                if tx.send(Ok(event)).await.is_err() {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx)))
}

async fn download_user_world_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<UserWorldFileDownloadQuery>,
) -> Result<Response, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let requester_id = resolved.user.user_id;
    let conversation_id = params.conversation_id.trim();
    if conversation_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "conversation_id is required".to_string(),
        ));
    }
    let owner_user_id = params
        .owner_user_id
        .as_deref()
        .unwrap_or(&requester_id)
        .trim()
        .to_string();
    if owner_user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "owner_user_id is required".to_string(),
        ));
    }
    let normalized = normalize_relative_path(&params.path);
    if normalized.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.path_required"),
        ));
    }
    let check_only = params.check.unwrap_or(false);
    let conversation = state
        .storage
        .get_user_world_conversation(conversation_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if conversation.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "conversation not found".to_string(),
        ));
    }
    let member = state
        .storage
        .get_user_world_member(conversation_id, &requester_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if member.is_none() {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "forbidden".to_string(),
        ));
    }
    let owner_member = state
        .storage
        .get_user_world_member(conversation_id, &owner_user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if owner_member.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "owner not in conversation".to_string(),
        ));
    }

    let workspace_id = state.workspace.scoped_user_id(&owner_user_id, None);
    let root = state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !root.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("workspace.error.workspace_not_found"),
        ));
    }
    let target = state
        .workspace
        .resolve_path(&workspace_id, &normalized)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !target.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.file_not_found"),
        ));
    }
    if target.is_dir() {
        let filename_prefix = target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("archive")
            .to_string();
        let filename = if filename_prefix.to_lowercase().ends_with(".zip") {
            filename_prefix
        } else {
            format!("{filename_prefix}.zip")
        };
        if check_only {
            return Ok(empty_response(&filename, "application/zip"));
        }
        let base_root = target
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
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
        let file = tokio::fs::File::open(&archive_path)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let stream = TempFileStream::new(archive_path.clone(), ReaderStream::new(file));
        return Ok(stream_response(stream, &filename, "application/zip"));
    }
    let filename = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    if check_only {
        return Ok(empty_response(filename, "application/octet-stream"));
    }
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

fn normalize_pagination(offset: Option<i64>, limit: Option<i64>) -> (i64, i64) {
    (
        offset.unwrap_or(0).max(0),
        normalize_limit(limit.unwrap_or(DEFAULT_LIMIT)),
    )
}

fn normalize_limit(limit: i64) -> i64 {
    if limit <= 0 {
        DEFAULT_LIMIT
    } else {
        limit.min(MAX_LIMIT)
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
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

fn empty_response(filename: &str, content_type: &'static str) -> Response {
    let disposition = build_content_disposition(filename);
    let mut response = Response::new(Body::empty());
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

fn normalize_relative_path(value: &str) -> String {
    let trimmed = value.replace('\\', "/");
    let trimmed = trimmed.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == "/" {
        return String::new();
    }
    trimmed.trim_start_matches('/').to_string()
}

fn create_temp_archive_file() -> Result<PathBuf, io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_user_world");
    std::fs::create_dir_all(&root)?;
    let filename = format!("wunder_user_world_{}.zip", uuid::Uuid::new_v4().simple());
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
