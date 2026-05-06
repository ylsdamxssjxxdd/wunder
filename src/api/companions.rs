use crate::api::errors::error_response;
use crate::services::companions::{
    export_global_companion, list_global_companions, load_global_companion,
};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::Path as AxumPath;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/companions/global", get(list_global))
        .route("/wunder/companions/global/{id}", get(get_global))
        .route("/wunder/companions/global/{id}/package", get(export_global))
}

async fn list_global() -> Result<Json<Value>, Response> {
    let items = list_global_companions()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn get_global(AxumPath(id): AxumPath<String>) -> Result<Json<Value>, Response> {
    let Some(item) = load_global_companion(&id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "companion not found".to_string(),
        ));
    };
    Ok(Json(json!({ "data": item })))
}

async fn export_global(AxumPath(id): AxumPath<String>) -> Result<Response, Response> {
    let (filename, bytes) = export_global_companion(&id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(zip_response(filename, bytes))
}

fn zip_response(filename: String, bytes: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(bytes.clone()));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    if let Ok(value) = HeaderValue::from_str(&bytes.len().to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    if let Ok(value) = HeaderValue::from_str(&content_disposition(&filename)) {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

fn content_disposition(filename: &str) -> String {
    let ascii_name = filename
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!(
        "attachment; filename=\"{}\"",
        ascii_name.trim().trim_matches('"')
    )
}
