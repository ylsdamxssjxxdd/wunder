use crate::api::user_context::resolve_user;
use crate::core::atomic_write::atomic_write_bytes;
use crate::onlyoffice as onlyoffice_service;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Local};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tokio_util::io::ReaderStream;
use tracing::warn;
use url::Url;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/workspace/onlyoffice/config", get(editor_config))
        .route("/wunder/workspace/onlyoffice/file", get(file_download))
        .route("/wunder/workspace/onlyoffice/callback", post(save_callback))
}

async fn editor_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<OnlyOfficeEditorConfigQuery>,
) -> Result<Json<OnlyOfficeEditorConfigResponse>, Response> {
    let resolved_user = resolve_user(&state, &headers, params.user_id.as_deref()).await?;
    let user_id = resolved_user.user.user_id;
    let agent_id = normalize_agent_id(params.agent_id.as_deref());
    let workspace_id = resolve_workspace_id(&state, &user_id, agent_id, params.container_id);
    let normalized_path = normalize_relative_path(&params.path);
    if normalized_path.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "file path is required".to_string(),
        ));
    }

    let app_config = state.config_store.get().await;
    let request_base = resolve_request_base_url(&headers, &app_config);
    let Some(office_config) = onlyoffice_service::resolve_config(&app_config, Some(&request_base))
    else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "OnlyOffice is not configured".to_string(),
        ));
    };
    if office_config
        .jwt_secret
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "OnlyOffice JWT secret is required".to_string(),
        ));
    }

    state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = state
        .workspace
        .resolve_path(&workspace_id, &normalized_path)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !target.exists() || !target.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "file not found".to_string(),
        ));
    }
    let extension = extension_from_path(&target);
    if !onlyoffice_service::is_supported_extension(&extension) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "OnlyOffice does not support this file type".to_string(),
        ));
    }
    let metadata = tokio::fs::metadata(&target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let updated_epoch_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let filename = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document");
    let editor = onlyoffice_service::build_editor_config(
        &office_config,
        &user_id,
        &workspace_id,
        &normalized_path,
        filename,
        &extension,
        metadata.len(),
        updated_epoch_ms,
        params.lang.as_deref().unwrap_or("zh-CN"),
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    Ok(Json(OnlyOfficeEditorConfigResponse {
        enabled: true,
        api_url: office_config.api_url,
        config: editor,
        path: normalized_path,
        updated_time: format_modified_time(&metadata),
    }))
}

async fn file_download(
    State(state): State<Arc<AppState>>,
    Query(params): Query<OnlyOfficeTokenQuery>,
) -> Result<Response, Response> {
    let app_config = state.config_store.get().await;
    let Some(secret) = app_config.onlyoffice.jwt_secret() else {
        return Err(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid token".to_string(),
        ));
    };
    let token = onlyoffice_service::verify_access_token(
        &secret,
        &params.token,
        onlyoffice_service::TOKEN_KIND_FILE,
    )
    .map_err(|err| error_response(StatusCode::UNAUTHORIZED, format!("invalid token: {err}")))?;
    let target = state
        .workspace
        .resolve_path(&token.workspace_id, &token.path)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !target.exists() || !target.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "file not found".to_string(),
        ));
    }
    let file = tokio::fs::File::open(&target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let filename = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document");
    let stream = ReaderStream::new(file);
    Ok(stream_response(
        stream,
        filename,
        onlyoffice_service::content_type(&extension_from_path(&target)),
        false,
    ))
}

async fn save_callback(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<OnlyOfficeTokenQuery>,
    Json(payload): Json<Value>,
) -> Result<Json<OnlyOfficeCallbackResponse>, Response> {
    let app_config = state.config_store.get().await;
    let Some(secret) = app_config.onlyoffice.jwt_secret() else {
        return Ok(Json(OnlyOfficeCallbackResponse::error(1)));
    };
    let token = match onlyoffice_service::verify_access_token(
        &secret,
        &params.token,
        onlyoffice_service::TOKEN_KIND_CALLBACK,
    ) {
        Ok(token) => token,
        Err(_) => return Ok(Json(OnlyOfficeCallbackResponse::error(1))),
    };
    let status = payload.get("status").and_then(Value::as_i64).unwrap_or(0);
    if status != 2 && status != 6 {
        return Ok(Json(OnlyOfficeCallbackResponse::ok()));
    }
    let Some(download_url) = payload.get("url").and_then(Value::as_str) else {
        return Ok(Json(OnlyOfficeCallbackResponse::error(1)));
    };
    if download_url.trim().is_empty() {
        return Ok(Json(OnlyOfficeCallbackResponse::error(1)));
    }
    let request_base = resolve_request_base_url(&headers, &app_config);
    let Some(office_config) = onlyoffice_service::resolve_config(&app_config, Some(&request_base))
    else {
        return Ok(Json(OnlyOfficeCallbackResponse::error(1)));
    };
    let target = match state
        .workspace
        .resolve_path(&token.workspace_id, &token.path)
    {
        Ok(path) => path,
        Err(_) => return Ok(Json(OnlyOfficeCallbackResponse::error(1))),
    };
    if !target.exists() || !target.is_file() {
        return Ok(Json(OnlyOfficeCallbackResponse::error(1)));
    }
    let extension = extension_from_path(&target);
    if !onlyoffice_service::is_editable_extension(&extension) {
        return Ok(Json(OnlyOfficeCallbackResponse::error(1)));
    }
    let bytes = match download_onlyoffice_document(&office_config, download_url).await {
        Ok(bytes) => bytes,
        Err(err) => {
            warn!(
                "OnlyOffice callback download failed for {}: {err}",
                token.path
            );
            return Ok(Json(OnlyOfficeCallbackResponse::error(1)));
        }
    };
    if bytes.len() > office_config.max_download_bytes {
        warn!(
            "OnlyOffice callback download exceeded limit for {}: {} > {}",
            token.path,
            bytes.len(),
            office_config.max_download_bytes
        );
        return Ok(Json(OnlyOfficeCallbackResponse::error(1)));
    }
    if let Err(err) = atomic_write_bytes(&target, &bytes) {
        warn!("OnlyOffice callback write failed for {}: {err}", token.path);
        return Ok(Json(OnlyOfficeCallbackResponse::error(1)));
    }
    state.workspace.mark_tree_dirty(&token.workspace_id);
    Ok(Json(OnlyOfficeCallbackResponse::ok()))
}

async fn download_onlyoffice_document(
    config: &onlyoffice_service::OnlyOfficeResolvedConfig,
    url: &str,
) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.request_timeout_s))
        .build()?;
    let download_url = resolve_onlyoffice_download_url(config, url);
    let response = client.get(download_url).send().await?.error_for_status()?;
    let mut stream = response.bytes_stream();
    let mut output = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if output.len().saturating_add(chunk.len()) > config.max_download_bytes {
            anyhow::bail!("OnlyOffice document exceeds configured download limit");
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

fn resolve_onlyoffice_download_url(
    config: &onlyoffice_service::OnlyOfficeResolvedConfig,
    url: &str,
) -> String {
    let trimmed = url.trim();
    let Some(document_server_url) = config.document_server_url.as_deref() else {
        return trimmed.to_string();
    };
    rewrite_url_origin(trimmed, document_server_url).unwrap_or_else(|| trimmed.to_string())
}

fn rewrite_url_origin(source: &str, target_origin: &str) -> Option<String> {
    let mut source_url = Url::parse(source).ok()?;
    let target_url = Url::parse(target_origin).ok()?;
    source_url.set_scheme(target_url.scheme()).ok()?;
    source_url.set_host(target_url.host_str()).ok()?;
    source_url.set_port(target_url.port()).ok()?;
    Some(source_url.to_string())
}

fn resolve_request_base_url(headers: &HeaderMap, config: &crate::config::Config) -> String {
    if let Some(configured) = config.onlyoffice.public_base_url() {
        return configured;
    }
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("http");
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            let host = if config.server.host == "0.0.0.0" {
                "127.0.0.1".to_string()
            } else {
                config.server.host.clone()
            };
            format!("{host}:{}", config.server.port)
        });
    format!("{proto}://{host}")
}

fn normalize_agent_id(value: Option<&str>) -> Option<&str> {
    value
        .map(|raw| raw.trim())
        .filter(|trimmed| !trimmed.is_empty())
}

fn normalize_relative_path(value: &str) -> String {
    let trimmed = value.replace('\\', "/");
    let trimmed = trimmed.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == "/" {
        return String::new();
    }
    trimmed.trim_start_matches('/').to_string()
}

fn resolve_workspace_id(
    state: &AppState,
    user_id: &str,
    agent_id: Option<&str>,
    container_id: Option<i32>,
) -> String {
    if let Some(explicit_container_id) =
        container_id.map(crate::storage::normalize_workspace_container_id)
    {
        return state
            .workspace
            .scoped_user_id_by_container(user_id, explicit_container_id);
    }
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

fn extension_from_path(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn format_modified_time(metadata: &std::fs::Metadata) -> String {
    metadata
        .modified()
        .ok()
        .map(|time| DateTime::<Local>::from(time).to_rfc3339())
        .unwrap_or_default()
}

fn stream_response<S>(
    stream: S,
    filename: &str,
    content_type: &'static str,
    attachment: bool,
) -> Response
where
    S: futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + 'static,
{
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(value) = HeaderValue::from_str(&build_content_disposition(filename, attachment)) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

fn build_content_disposition(filename: &str, attachment: bool) -> String {
    let kind = if attachment { "attachment" } else { "inline" };
    let ascii_name = sanitize_filename(filename);
    if ascii_name == filename {
        return format!("{kind}; filename=\"{ascii_name}\"");
    }
    let encoded = percent_encode(filename);
    format!("{kind}; filename=\"{ascii_name}\"; filename*=UTF-8''{encoded}")
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
        "document".to_string()
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
struct OnlyOfficeEditorConfigQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    container_id: Option<i32>,
    path: String,
    #[serde(default)]
    lang: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OnlyOfficeTokenQuery {
    token: String,
}

#[derive(Debug, Serialize)]
struct OnlyOfficeEditorConfigResponse {
    enabled: bool,
    api_url: String,
    config: Value,
    path: String,
    updated_time: String,
}

#[derive(Debug, Serialize)]
struct OnlyOfficeCallbackResponse {
    error: i32,
}

impl OnlyOfficeCallbackResponse {
    fn ok() -> Self {
        Self { error: 0 }
    }

    fn error(code: i32) -> Self {
        Self { error: code }
    }
}
