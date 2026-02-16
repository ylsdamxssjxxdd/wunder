use crate::i18n;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Query};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use chrono::{DateTime, Local};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

const MAX_TEMP_DIR_UPLOAD_BYTES: usize = 200 * 1024 * 1024;
const TEMP_DIR_ROOT_ENV: &str = "WUNDER_TEMP_DIR_ROOT";

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/temp_dir/download", get(temp_dir_download))
        .route(
            "/wunder/temp_dir/upload",
            post(temp_dir_upload).layer(DefaultBodyLimit::max(MAX_TEMP_DIR_UPLOAD_BYTES)),
        )
        .route("/wunder/temp_dir/list", get(temp_dir_list))
        .route("/wunder/temp_dir/remove", post(temp_dir_remove))
}

async fn temp_dir_download(
    Query(params): Query<TempDirDownloadQuery>,
) -> Result<Response, Response> {
    let filename = normalize_relative_path(&params.filename, false)?;
    let dir = temp_dir_root()?;
    let target = dir.join(&filename);
    let metadata = tokio::fs::metadata(&target)
        .await
        .map_err(|_| error_response(StatusCode::NOT_FOUND, i18n::t("error.file_not_found")))?;
    if !metadata.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.file_not_found"),
        ));
    }

    let file = tokio::fs::File::open(&target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let stream = ReaderStream::new(file);
    Ok(stream_response(
        stream,
        &filename,
        "application/octet-stream",
    ))
}

async fn temp_dir_upload(
    mut multipart: Multipart,
) -> Result<Json<TempDirUploadResponse>, Response> {
    let mut raw_path = String::new();
    let mut overwrite = true;
    let mut uploaded = Vec::new();
    let mut has_file = false;
    let mut pending_files = Vec::new();
    let mut temp_upload_dir = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "path" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                raw_path = value.trim().to_string();
            }
            "overwrite" => {
                let value = field
                    .text()
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
                overwrite = parse_bool(&value, true);
            }
            _ => {
                let Some(raw_name) = field.file_name().map(str::to_string) else {
                    continue;
                };
                let base_name = Path::new(&raw_name)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                let filename = normalize_relative_path(base_name, false)?;
                has_file = true;
                if temp_upload_dir.is_none() {
                    temp_upload_dir = Some(create_temp_upload_dir()?);
                }
                let Some(temp_upload_dir) = temp_upload_dir.as_ref() else {
                    return Err(error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        i18n::t("error.internal"),
                    ));
                };
                let temp_path = temp_upload_dir.join(format!("upload_{}", Uuid::new_v4().simple()));
                save_multipart_file(field, &temp_path).await?;
                pending_files.push(PendingUpload {
                    temp_path,
                    filename,
                });
            }
        }
    }

    if !has_file {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.temp_dir_upload_no_file"),
        ));
    }

    let base_dir = ensure_temp_dir()?;
    let sub_dir = normalize_relative_path(&raw_path, true)?;
    let target_dir = if sub_dir.is_empty() {
        base_dir
    } else {
        base_dir.join(&sub_dir)
    };
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|err| {
            cleanup_temp_files(&pending_files, temp_upload_dir.as_ref());
            error_response(StatusCode::BAD_REQUEST, err.to_string())
        })?;

    for pending in &pending_files {
        let dest = target_dir.join(&pending.filename);
        if let Ok(metadata) = tokio::fs::metadata(&dest).await {
            if !overwrite {
                cleanup_temp_files(&pending_files, temp_upload_dir.as_ref());
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.temp_dir_file_exists"),
                ));
            }
            if metadata.is_dir() {
                cleanup_temp_files(&pending_files, temp_upload_dir.as_ref());
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.temp_dir_target_not_file"),
                ));
            }
            let _ = tokio::fs::remove_file(&dest).await;
        }
        if tokio::fs::rename(&pending.temp_path, &dest).await.is_err() {
            tokio::fs::copy(&pending.temp_path, &dest)
                .await
                .map_err(|err| {
                    cleanup_temp_files(&pending_files, temp_upload_dir.as_ref());
                    error_response(StatusCode::BAD_REQUEST, err.to_string())
                })?;
            let _ = tokio::fs::remove_file(&pending.temp_path).await;
        }
        let relative = if sub_dir.is_empty() {
            pending.filename.clone()
        } else {
            format!("{}/{}", sub_dir, pending.filename)
        };
        uploaded.push(relative);
    }

    cleanup_temp_files(&pending_files, temp_upload_dir.as_ref());
    Ok(Json(TempDirUploadResponse {
        ok: true,
        files: uploaded,
    }))
}

async fn temp_dir_list() -> Result<Json<TempDirListResponse>, Response> {
    let dir = temp_dir_root()?;
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(Json(TempDirListResponse { ok: true, files }));
    }
    let mut pending_dirs = vec![dir.clone()];
    while let Some(current) = pending_dirs.pop() {
        let mut entries = tokio::fs::read_dir(&current)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        {
            let metadata = entry
                .metadata()
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            let path = entry.path();
            if metadata.is_dir() {
                pending_dirs.push(path);
                continue;
            }
            let name = path
                .strip_prefix(&dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let updated_time = format_modified_time(&metadata);
            files.push(TempDirFileEntry {
                name,
                size: metadata.len(),
                updated_time,
            });
        }
    }
    Ok(Json(TempDirListResponse { ok: true, files }))
}

async fn temp_dir_remove(
    Json(request): Json<TempDirRemoveRequest>,
) -> Result<Json<TempDirRemoveResponse>, Response> {
    if !request.all && request.filename.trim().is_empty() && request.filenames.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.temp_dir_remove_required"),
        ));
    }
    let dir = ensure_temp_dir()?;
    let mut removed = Vec::new();
    let mut missing = Vec::new();

    if request.all {
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                let _ = tokio::fs::remove_dir_all(&path).await;
            } else {
                let _ = tokio::fs::remove_file(&path).await;
            }
            removed.push(name);
        }
        return Ok(Json(TempDirRemoveResponse {
            ok: true,
            removed,
            missing,
        }));
    }

    let mut targets = request.filenames;
    if !request.filename.trim().is_empty() {
        targets.push(request.filename);
    }

    for raw in targets {
        let filename = normalize_relative_path(&raw, false)?;
        let path = dir.join(&filename);
        if !path.exists() {
            missing.push(filename);
            continue;
        }
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if metadata.is_dir() {
            tokio::fs::remove_dir_all(&path)
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        } else {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        removed.push(filename);
    }

    Ok(Json(TempDirRemoveResponse {
        ok: true,
        removed,
        missing,
    }))
}

fn temp_dir_root() -> Result<PathBuf, Response> {
    if let Ok(value) = std::env::var(TEMP_DIR_ROOT_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.is_absolute() {
                return Ok(candidate);
            }
            let root = std::env::current_dir()
                .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
            return Ok(root.join(candidate));
        }
    }
    let root = std::env::current_dir()
        .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(root.join("temp_dir"))
}

fn ensure_temp_dir() -> Result<PathBuf, Response> {
    let dir = temp_dir_root()?;
    std::fs::create_dir_all(&dir)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(dir)
}

fn normalize_relative_path(value: &str, allow_empty: bool) -> Result<String, Response> {
    let normalized = value.trim().replace('\\', "/");
    let normalized = normalized.trim_matches('/');
    if normalized.is_empty() || normalized == "." {
        if allow_empty {
            return Ok(String::new());
        }
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.download_filename_required"),
        ));
    }
    if normalized.contains(':') {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.download_filename_invalid"),
        ));
    }
    for segment in normalized.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.download_filename_invalid"),
            ));
        }
    }
    Ok(normalized.to_string())
}

fn parse_bool(value: &str, default_value: bool) -> bool {
    match value.trim().to_lowercase().as_str() {
        "" => default_value,
        "true" | "1" | "yes" | "on" => true,
        "false" | "0" | "no" | "off" => false,
        _ => default_value,
    }
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

fn format_modified_time(metadata: &std::fs::Metadata) -> String {
    metadata
        .modified()
        .ok()
        .map(|time| DateTime::<Local>::from(time).to_rfc3339())
        .unwrap_or_default()
}

fn create_temp_upload_dir() -> Result<PathBuf, Response> {
    let mut root = std::env::temp_dir();
    root.push("wunder_temp_dir_uploads");
    root.push(Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&root)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(root)
}

fn cleanup_temp_files(pending: &[PendingUpload], dir: Option<&PathBuf>) {
    for file in pending {
        let _ = std::fs::remove_file(&file.temp_path);
    }
    if let Some(dir) = dir {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

#[derive(Debug, Deserialize)]
struct TempDirDownloadQuery {
    #[serde(default)]
    filename: String,
}

#[derive(Debug, Deserialize)]
struct TempDirRemoveRequest {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    filenames: Vec<String>,
    #[serde(default)]
    all: bool,
}

#[derive(Debug, Serialize)]
struct TempDirUploadResponse {
    ok: bool,
    files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TempDirListResponse {
    ok: bool,
    files: Vec<TempDirFileEntry>,
}

#[derive(Debug, Serialize)]
struct TempDirFileEntry {
    name: String,
    size: u64,
    updated_time: String,
}

#[derive(Debug, Serialize)]
struct TempDirRemoveResponse {
    ok: bool,
    removed: Vec<String>,
    missing: Vec<String>,
}

struct PendingUpload {
    temp_path: PathBuf,
    filename: String,
}
