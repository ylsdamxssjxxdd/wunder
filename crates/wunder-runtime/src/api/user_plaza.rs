use crate::api::user_context::resolve_user;
use crate::services::user_plaza::{
    get_item, import_item, list_items, publish_item, unpublish_item, ListUserPlazaItemsQuery,
    PublishUserPlazaItemRequest,
};
use crate::state::AppState;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/plaza/items",
            get(list_user_plaza_items).post(create_user_plaza_item),
        )
        .route(
            "/wunder/plaza/items/{item_id}",
            get(get_user_plaza_item).delete(delete_user_plaza_item),
        )
        .route(
            "/wunder/plaza/items/{item_id}/import",
            post(import_user_plaza_item),
        )
}

#[derive(Debug, Deserialize, Default)]
struct PlazaUserQuery {
    #[serde(default)]
    user_id: Option<String>,
}

async fn list_user_plaza_items(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(user_query): Query<PlazaUserQuery>,
    Query(query): Query<ListUserPlazaItemsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, user_query.user_id.as_deref()).await?;
    let items = list_items(&state, &resolved.user.user_id, &query)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "total": items.len(),
            "items": items
        }
    })))
}

async fn get_user_plaza_item(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(item_id): AxumPath<String>,
    Query(user_query): Query<PlazaUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, user_query.user_id.as_deref()).await?;
    let record = get_item(&state, &item_id)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "plaza item not found".to_string()))?;
    let items = list_items(
        &state,
        &resolved.user.user_id,
        &ListUserPlazaItemsQuery {
            mine_only: false,
            kind: Some(record.kind.clone()),
        },
    )
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let payload = items
        .into_iter()
        .find(|item| item.get("item_id").and_then(Value::as_str) == Some(record.item_id.as_str()))
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "plaza item not found".to_string()))?;
    Ok(Json(json!({ "data": payload })))
}

async fn create_user_plaza_item(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(user_query): Query<PlazaUserQuery>,
    Json(payload): Json<PublishUserPlazaItemRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, user_query.user_id.as_deref()).await?;
    let item = publish_item(&state, &resolved.user, payload)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": item })))
}

async fn delete_user_plaza_item(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(item_id): AxumPath<String>,
    Query(user_query): Query<PlazaUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, user_query.user_id.as_deref()).await?;
    let deleted = unpublish_item(&state, &resolved.user.user_id, &item_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !deleted {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "plaza item not found".to_string(),
        ));
    }
    Ok(Json(json!({
        "data": {
            "ok": true,
            "item_id": item_id
        }
    })))
}

async fn import_user_plaza_item(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(item_id): AxumPath<String>,
    Query(user_query): Query<PlazaUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, user_query.user_id.as_deref()).await?;
    let imported = import_item(&state, &resolved.user, &item_id)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": imported })))
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
