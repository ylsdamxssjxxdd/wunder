use crate::api::attachment_convert::{build_ok_conversion_payload, convert_multipart_list};
use crate::state::AppState;
use axum::extract::{DefaultBodyLimit, Multipart};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use std::sync::Arc;

const MAX_DOC2MD_UPLOAD_BYTES: usize = 200 * 1024 * 1024;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/wunder/doc2md/convert",
        post(doc2md_convert).layer(DefaultBodyLimit::max(MAX_DOC2MD_UPLOAD_BYTES)),
    )
}

async fn doc2md_convert(multipart: Multipart) -> Result<Json<serde_json::Value>, Response> {
    let conversions = convert_multipart_list(multipart).await?;
    Ok(Json(build_ok_conversion_payload(conversions)))
}
