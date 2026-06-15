use axum::http::header::AUTHORIZATION;
use axum::http::HeaderMap;

pub use wunder_core::auth::{is_admin_path, is_leader_path};

pub fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    let api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    wunder_core::auth::extract_api_key_values(api_key, authorization)
}

pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    wunder_core::auth::extract_bearer_token_value(value)
}
