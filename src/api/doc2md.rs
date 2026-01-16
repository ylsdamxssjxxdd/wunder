use crate::api::attachment_convert::convert_multipart;
use crate::state::AppState;
use axum::extract::{DefaultBodyLimit, Multipart};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use std::sync::Arc;

const MAX_DOC2MD_UPLOAD_BYTES: usize = 200 * 1024 * 1024;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/wunder/doc2md/convert",
        post(doc2md_convert).layer(DefaultBodyLimit::max(MAX_DOC2MD_UPLOAD_BYTES)),
    )
}

async fn doc2md_convert(multipart: Multipart) -> Result<Json<serde_json::Value>, Response> {
    let conversion = convert_multipart(multipart).await?;
    Ok(Json(json!({
        "ok": true,
        "name": conversion.name,
        "content": conversion.content,
        "converter": conversion.converter,
        "warnings": conversion.warnings,
    })))
}
