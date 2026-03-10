use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::services::hive_pack::{
    get_job_for_user, job_payload, resolve_export_artifact_path, run_export_job, run_import_job,
    HivePackExportOptions, HivePackImportOptions,
};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::io::ReaderStream;

const MAX_HIVEPACK_UPLOAD_BYTES: usize = 200 * 1024 * 1024;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/beeroom/packs/import",
            post(import_hive_pack).layer(DefaultBodyLimit::max(MAX_HIVEPACK_UPLOAD_BYTES)),
        )
        .route("/wunder/beeroom/packs/import/{job_id}", get(get_import_job))
        .route("/wunder/beeroom/packs/export", post(export_hive_pack))
        .route("/wunder/beeroom/packs/export/{job_id}", get(get_export_job))
        .route(
            "/wunder/beeroom/packs/export/{job_id}/download",
            get(download_export_pack),
        )
}

async fn import_hive_pack(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let mut filename = String::new();
    let mut data = Vec::new();
    let mut options = HivePackImportOptions::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let field_name = field.name().unwrap_or("").trim().to_string();
        if field_name.eq_ignore_ascii_case("options") {
            let text = field
                .text()
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            if !text.trim().is_empty() {
                options = serde_json::from_str(&text).map_err(|err| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        format!("invalid import options json: {err}"),
                    )
                })?;
            }
            continue;
        }
        if matches!(
            field_name.as_str(),
            "group_id" | "groupId" | "hive_id" | "hiveId"
        ) {
            let text = field
                .text()
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            if !text.trim().is_empty() {
                options.group_id = Some(text.trim().to_string());
            }
            continue;
        }
        let Some(raw_name) = field.file_name().map(str::to_string) else {
            continue;
        };
        filename = raw_name;
        data = field
            .bytes()
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .to_vec();
    }
    if data.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "hivepack file is required".to_string(),
        ));
    }
    if filename.trim().is_empty() {
        filename = "import.hivepack".to_string();
    }
    let job = run_import_job(state.as_ref(), &resolved.user, &filename, data, options)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": job_payload(&job) })))
}

async fn get_import_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let Some(job) = get_job_for_user(state.as_ref(), &resolved.user.user_id, &job_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "import job not found".to_string(),
        ));
    };
    if job.job_type != "import" {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "import job not found".to_string(),
        ));
    }
    Ok(Json(json!({ "data": job_payload(&job) })))
}

async fn export_hive_pack(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ExportHivePackRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let options = HivePackExportOptions {
        group_id: payload.group_id.trim().to_string(),
        mode: payload.mode.clone(),
    };
    if options.group_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "group_id is required".to_string(),
        ));
    }
    let job = run_export_job(state.as_ref(), &resolved.user, options)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": job_payload(&job) })))
}

async fn get_export_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let Some(job) = get_job_for_user(state.as_ref(), &resolved.user.user_id, &job_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "export job not found".to_string(),
        ));
    };
    if job.job_type != "export" {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "export job not found".to_string(),
        ));
    }
    Ok(Json(json!({ "data": job_payload(&job) })))
}

async fn download_export_pack(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Response, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let Some(job) = get_job_for_user(state.as_ref(), &resolved.user.user_id, &job_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "export job not found".to_string(),
        ));
    };
    if job.job_type != "export" || job.status != "completed" {
        return Err(error_response(
            StatusCode::CONFLICT,
            "export job is not completed".to_string(),
        ));
    }
    let Some(path) = resolve_export_artifact_path(&job) else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "export artifact missing".to_string(),
        ));
    };
    if !path.exists() || !path.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "export artifact missing".to_string(),
        ));
    }
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let stream = ReaderStream::new(file);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    if let Ok(value) = HeaderValue::from_str(&metadata.len().to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    let filename = job
        .artifact
        .as_ref()
        .map(|item| item.filename.as_str())
        .unwrap_or("hivepack.zip");
    let content_disposition = build_content_disposition(filename);
    if let Ok(content_disposition) = HeaderValue::from_str(&content_disposition) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, content_disposition);
    }
    Ok(response)
}

#[derive(Debug, Deserialize)]
struct ExportHivePackRequest {
    #[serde(alias = "groupId", alias = "hive_id", alias = "hiveId")]
    group_id: String,
    #[serde(default)]
    mode: Option<String>,
}

fn build_content_disposition(filename: &str) -> String {
    let ascii_name = sanitize_filename_ascii(filename);
    if ascii_name == filename {
        return format!("attachment; filename=\"{ascii_name}\"");
    }
    let encoded = percent_encode(filename);
    format!("attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{encoded}")
}

fn sanitize_filename_ascii(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.trim().is_empty() {
        "hivepack.zip".to_string()
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
