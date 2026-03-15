use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::services::memory::normalize_agent_memory_scope;
use crate::services::memory_fragments::{
    MemoryFragmentInput, MemoryFragmentListOptions, MemoryFragmentStore,
};
use crate::state::AppState;
use crate::user_access::is_agent_allowed;
use anyhow::Result;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/agents/{agent_id}/memories",
            get(list_memories).post(create_memory),
        )
        .route(
            "/wunder/agents/{agent_id}/memories/{memory_id}",
            get(get_memory).patch(update_memory).delete(delete_memory),
        )
        .route(
            "/wunder/agents/{agent_id}/memories/{memory_id}/confirm",
            post(confirm_memory),
        )
        .route(
            "/wunder/agents/{agent_id}/memories/{memory_id}/pin",
            post(pin_memory),
        )
        .route(
            "/wunder/agents/{agent_id}/memories/{memory_id}/invalidate",
            post(invalidate_memory),
        )
        .route(
            "/wunder/agents/{agent_id}/memory-hits",
            get(list_memory_hits),
        )
}

#[derive(Debug, Deserialize, Default)]
struct AgentUserQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default, alias = "q")]
    query: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    include_invalidated: Option<bool>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct MemoryFragmentRequest {
    #[serde(default)]
    memory_id: Option<String>,
    #[serde(default)]
    source_session_id: Option<String>,
    #[serde(default)]
    source_round_id: Option<String>,
    #[serde(default)]
    source_type: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    title_l0: Option<String>,
    #[serde(default)]
    summary_l1: Option<String>,
    #[serde(default)]
    content_l2: Option<String>,
    #[serde(default)]
    fact_key: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    entities: Option<Vec<String>>,
    #[serde(default)]
    importance: Option<f64>,
    #[serde(default)]
    confidence: Option<f64>,
    #[serde(default)]
    tier: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    confirmed_by_user: Option<bool>,
    #[serde(default)]
    invalidated: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ToggleRequest {
    #[serde(default)]
    value: Option<bool>,
}

async fn list_memories(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, &agent_id)?;
    let store = MemoryFragmentStore::new(state.storage.clone());
    let items = store.list_fragments(
        &resolved.user.user_id,
        Some(&agent_id),
        MemoryFragmentListOptions {
            query: query.query.as_deref(),
            category: query.category.as_deref(),
            status: query.status.as_deref(),
            pinned: query.pinned,
            include_invalidated: query.include_invalidated.unwrap_or(false),
            limit: query.limit,
        },
    );
    let hits = store.list_hits(
        &resolved.user.user_id,
        Some(&agent_id),
        query.session_id.as_deref(),
        20,
    );
    let recent_jobs = state
        .storage
        .list_memory_jobs(
            &resolved.user.user_id,
            &normalize_agent_memory_scope(Some(&agent_id)),
            12,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let categories = items
        .iter()
        .map(|item| item.category.clone())
        .filter(|item| !item.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "data": {
            "items": items,
            "total": items.len(),
            "categories": categories,
            "recent_hits": hits,
            "recent_jobs": recent_jobs,
        }
    })))
}

async fn get_memory(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((agent_id, memory_id)): AxumPath<(String, String)>,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, &agent_id)?;
    let store = MemoryFragmentStore::new(state.storage.clone());
    let item = store
        .get_fragment(&resolved.user.user_id, Some(&agent_id), &memory_id)
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                "memory fragment not found".to_string(),
            )
        })?;
    Ok(Json(json!({ "data": { "item": item } })))
}

async fn create_memory(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
    Json(payload): Json<MemoryFragmentRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, &agent_id)?;
    let store = MemoryFragmentStore::new(state.storage.clone());
    let item = store
        .save_fragment(
            &resolved.user.user_id,
            Some(&agent_id),
            map_fragment_input(payload),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "item": item } })))
}

async fn update_memory(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((agent_id, memory_id)): AxumPath<(String, String)>,
    Query(query): Query<AgentUserQuery>,
    Json(mut payload): Json<MemoryFragmentRequest>,
) -> Result<Json<Value>, Response> {
    payload.memory_id = Some(memory_id);
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, &agent_id)?;
    let store = MemoryFragmentStore::new(state.storage.clone());
    let item = store
        .save_fragment(
            &resolved.user.user_id,
            Some(&agent_id),
            map_fragment_input(payload),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "item": item } })))
}

async fn delete_memory(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((agent_id, memory_id)): AxumPath<(String, String)>,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, &agent_id)?;
    let store = MemoryFragmentStore::new(state.storage.clone());
    Ok(Json(
        json!({ "data": { "deleted": store.delete_fragment(&resolved.user.user_id, Some(&agent_id), &memory_id) } }),
    ))
}

async fn confirm_memory(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((agent_id, memory_id)): AxumPath<(String, String)>,
    Query(query): Query<AgentUserQuery>,
    Json(payload): Json<ToggleRequest>,
) -> Result<Json<Value>, Response> {
    toggle_memory(
        state,
        headers,
        query,
        &agent_id,
        &memory_id,
        payload.value.unwrap_or(true),
        "confirm",
    )
    .await
}

async fn pin_memory(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((agent_id, memory_id)): AxumPath<(String, String)>,
    Query(query): Query<AgentUserQuery>,
    Json(payload): Json<ToggleRequest>,
) -> Result<Json<Value>, Response> {
    toggle_memory(
        state,
        headers,
        query,
        &agent_id,
        &memory_id,
        payload.value.unwrap_or(true),
        "pin",
    )
    .await
}

async fn invalidate_memory(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((agent_id, memory_id)): AxumPath<(String, String)>,
    Query(query): Query<AgentUserQuery>,
    Json(payload): Json<ToggleRequest>,
) -> Result<Json<Value>, Response> {
    toggle_memory(
        state,
        headers,
        query,
        &agent_id,
        &memory_id,
        payload.value.unwrap_or(true),
        "invalidate",
    )
    .await
}

async fn list_memory_hits(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, &agent_id)?;
    let store = MemoryFragmentStore::new(state.storage.clone());
    let items = store.list_hits(
        &resolved.user.user_id,
        Some(&agent_id),
        query.session_id.as_deref(),
        query.limit.unwrap_or(50) as i64,
    );
    Ok(Json(
        json!({ "data": { "items": items, "total": items.len() } }),
    ))
}

async fn toggle_memory(
    state: Arc<AppState>,
    headers: axum::http::HeaderMap,
    query: AgentUserQuery,
    agent_id: &str,
    memory_id: &str,
    value: bool,
    action: &str,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, agent_id)?;
    let store = MemoryFragmentStore::new(state.storage.clone());
    let item = match action {
        "confirm" => store.set_confirmed(&resolved.user.user_id, Some(agent_id), memory_id, value),
        "pin" => store.set_pinned(&resolved.user.user_id, Some(agent_id), memory_id, value),
        _ => store.set_invalidated(&resolved.user.user_id, Some(agent_id), memory_id, value),
    }
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    .ok_or_else(|| {
        error_response(
            StatusCode::NOT_FOUND,
            "memory fragment not found".to_string(),
        )
    })?;
    Ok(Json(json!({ "data": { "item": item } })))
}

fn map_fragment_input(payload: MemoryFragmentRequest) -> MemoryFragmentInput {
    MemoryFragmentInput {
        memory_id: payload.memory_id,
        source_session_id: payload.source_session_id,
        source_round_id: payload.source_round_id,
        source_type: payload.source_type,
        category: payload.category,
        title_l0: payload.title_l0,
        summary_l1: payload.summary_l1,
        content_l2: payload.content_l2,
        fact_key: payload.fact_key,
        tags: payload.tags,
        entities: payload.entities,
        importance: payload.importance,
        confidence: payload.confidence,
        tier: payload.tier,
        status: payload.status,
        pinned: payload.pinned,
        confirmed_by_user: payload.confirmed_by_user,
        invalidated: payload.invalidated,
    }
}

fn ensure_agent_access(
    state: &AppState,
    owner_user_id: &str,
    user: &crate::storage::UserAccountRecord,
    agent_id: &str,
) -> Result<(), Response> {
    let cleaned = agent_id.trim();
    if cleaned.is_empty()
        || cleaned.eq_ignore_ascii_case("__default__")
        || cleaned.eq_ignore_ascii_case("default")
    {
        return Ok(());
    }
    let record = state
        .user_store
        .get_user_agent_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.agent_not_found")))?;
    let access = state
        .user_store
        .get_user_agent_access(owner_user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !is_agent_allowed(user, access.as_ref(), &record) {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.agent_not_found"),
        ));
    }
    Ok(())
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}
