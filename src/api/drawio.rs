use crate::api::user_context::resolve_user;
use crate::drawio as drawio_service;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/workspace/drawio/config", get(editor_config))
}

async fn editor_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<DrawioEditorConfigQuery>,
) -> Result<Json<DrawioEditorConfigResponse>, Response> {
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
    let Some(drawio_config) = drawio_service::resolve_config(&app_config) else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "draw.io is not configured".to_string(),
        ));
    };

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
    let filename = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("diagram.drawio");
    if !drawio_service::is_supported_filename(filename) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "draw.io does not support this file type".to_string(),
        ));
    }
    let metadata = tokio::fs::metadata(&target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if metadata.len() as usize > drawio_config.max_file_bytes {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            format!(
                "draw.io file is too large: {} > {}",
                metadata.len(),
                drawio_config.max_file_bytes
            ),
        ));
    }

    Ok(Json(DrawioEditorConfigResponse {
        enabled: true,
        editor_url: drawio_service::editor_url_with_params(
            &drawio_config.editor_url,
            params.lang.as_deref().unwrap_or("zh-CN"),
        ),
        path: normalized_path,
        max_file_bytes: drawio_config.max_file_bytes,
        updated_time: format_modified_time(&metadata),
    }))
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

fn format_modified_time(metadata: &std::fs::Metadata) -> String {
    metadata
        .modified()
        .ok()
        .map(|time| DateTime::<Local>::from(time).to_rfc3339())
        .unwrap_or_default()
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

#[derive(Debug, Deserialize)]
struct DrawioEditorConfigQuery {
    path: String,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    container_id: Option<i32>,
    #[serde(default)]
    lang: Option<String>,
}

#[derive(Debug, Serialize)]
struct DrawioEditorConfigResponse {
    enabled: bool,
    editor_url: String,
    path: String,
    max_file_bytes: usize,
    updated_time: String,
}
