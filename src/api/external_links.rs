use crate::api::user_context::resolve_user;
use crate::state::AppState;
use crate::storage::{ExternalLinkRecord, UserAccountRecord};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

const DEFAULT_ORG_LEVEL: i32 = 1;

#[derive(Debug, Deserialize, Default)]
struct ExternalLinksQuery {
    #[serde(default)]
    link_id: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/external_links", get(list_external_links))
}

async fn list_external_links(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ExternalLinksQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_level = resolve_user_level(&state, &resolved.user)?;
    let records = state
        .storage
        .list_external_links(false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target_link_id = query
        .link_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let items = records
        .iter()
        .filter(|record| is_link_visible(record, user_level))
        .filter(|record| target_link_id.map_or(true, |target| record.link_id == target))
        .map(external_link_payload)
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "data": {
            "items": items,
            "user_level": user_level,
        }
    })))
}

fn resolve_user_level(state: &AppState, user: &UserAccountRecord) -> Result<i32, Response> {
    let Some(unit_id) = user
        .unit_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(DEFAULT_ORG_LEVEL);
    };
    let level = state
        .user_store
        .get_org_unit(unit_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .map(|unit| unit.level)
        .unwrap_or(DEFAULT_ORG_LEVEL)
        .max(DEFAULT_ORG_LEVEL);
    Ok(level)
}

fn is_link_visible(record: &ExternalLinkRecord, user_level: i32) -> bool {
    record.enabled
        && (record.allowed_levels.is_empty() || record.allowed_levels.contains(&user_level))
}

fn external_link_payload(record: &ExternalLinkRecord) -> Value {
    json!({
        "link_id": record.link_id,
        "title": record.title,
        "description": record.description,
        "url": record.url,
        "icon": record.icon,
        "allowed_levels": record.allowed_levels,
        "sort_order": record.sort_order,
        "enabled": record.enabled,
        "updated_at": record.updated_at,
    })
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
