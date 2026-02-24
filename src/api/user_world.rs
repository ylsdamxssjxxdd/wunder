use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::state::AppState;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 500;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/user_world/contacts", get(list_contacts))
        .route(
            "/wunder/user_world/conversations",
            get(list_conversations).post(create_or_get_conversation),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}",
            get(get_conversation),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}/messages",
            get(list_messages).post(send_message),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}/read",
            post(mark_read),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}/events",
            get(stream_events),
        )
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    keyword: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConversationCreateRequest {
    peer_user_id: String,
}

#[derive(Debug, Deserialize)]
struct ListMessageQuery {
    #[serde(default)]
    before_message_id: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SendMessageRequest {
    content: String,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    client_msg_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MarkReadRequest {
    #[serde(default)]
    last_read_message_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct EventQuery {
    #[serde(default)]
    after_event_id: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn list_contacts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let (offset, limit) = normalize_pagination(query.offset, query.limit);
    let keyword = query.keyword.as_deref();
    let (items, total) = state
        .user_world
        .list_contacts(&resolved.user.user_id, keyword, offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit
        }
    })))
}

async fn create_or_get_conversation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ConversationCreateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let item = state
        .user_world
        .resolve_or_create_direct_conversation(
            &resolved.user.user_id,
            payload.peer_user_id.trim(),
            now_ts(),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": item })))
}

async fn list_conversations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let (offset, limit) = normalize_pagination(query.offset, query.limit);
    let (items, total) = state
        .user_world
        .list_conversations(&resolved.user.user_id, offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit
        }
    })))
}

async fn get_conversation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let item = state
        .user_world
        .get_conversation(&resolved.user.user_id, conversation_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(item) = item else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "conversation not found",
        ));
    };
    Ok(Json(json!({ "data": item })))
}

async fn list_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Query(query): Query<ListMessageQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let limit = normalize_limit(query.limit.unwrap_or(DEFAULT_LIMIT));
    let items = state
        .user_world
        .list_messages(
            &resolved.user.user_id,
            conversation_id.trim(),
            query.before_message_id,
            limit,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items,
            "limit": limit
        }
    })))
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let content = payload.content.trim();
    if content.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "content is required",
        ));
    }
    let result = state
        .user_world
        .send_message(
            &resolved.user.user_id,
            conversation_id.trim(),
            content,
            payload.content_type.as_deref().unwrap_or("text"),
            payload.client_msg_id.as_deref(),
            now_ts(),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": result })))
}

async fn mark_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Json(payload): Json<MarkReadRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let result = state
        .user_world
        .mark_read(
            &resolved.user.user_id,
            conversation_id.trim(),
            payload.last_read_message_id,
            now_ts(),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": result })))
}

async fn stream_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Query(query): Query<EventQuery>,
) -> Result<Sse<ReceiverStream<Result<Event, Infallible>>>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let conversation_id = conversation_id.trim().to_string();
    if conversation_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "conversation_id is required",
        ));
    }
    let after_event_id = query.after_event_id.unwrap_or(0).max(0);
    let fetch_limit = normalize_limit(query.limit.unwrap_or(DEFAULT_LIMIT));
    let user_id = resolved.user.user_id.clone();
    let service = state.user_world.clone();
    service
        .list_events(&user_id, &conversation_id, after_event_id, 1)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        let mut current_event_id = after_event_id;
        loop {
            let events = service
                .list_events(&user_id, &conversation_id, current_event_id, fetch_limit)
                .unwrap_or_default();
            for item in events {
                current_event_id = current_event_id.max(item.event_id);
                let payload =
                    serde_json::to_string(&item.payload).unwrap_or_else(|_| "null".to_string());
                let event = Event::default()
                    .event(item.event_type)
                    .id(item.event_id.to_string())
                    .data(payload);
                if tx.send(Ok(event)).await.is_err() {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx)))
}

fn normalize_pagination(offset: Option<i64>, limit: Option<i64>) -> (i64, i64) {
    (
        offset.unwrap_or(0).max(0),
        normalize_limit(limit.unwrap_or(DEFAULT_LIMIT)),
    )
}

fn normalize_limit(limit: i64) -> i64 {
    if limit <= 0 {
        DEFAULT_LIMIT
    } else {
        limit.min(MAX_LIMIT)
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
