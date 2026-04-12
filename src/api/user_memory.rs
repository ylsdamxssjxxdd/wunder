use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::services::memory::{
    build_agent_memory_owner, normalize_agent_memory_scope, MemoryStore,
};
use crate::services::memory_agent_settings::AgentMemorySettingsService;
use crate::services::memory_fragments::{
    compact_memory_id_for_model, MemoryFragmentInput, MemoryFragmentListOptions,
    MemoryFragmentStore,
};
use crate::state::AppState;
use crate::storage::MemoryFragmentRecord;
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
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

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
            "/wunder/agents/{agent_id}/memories/migrate",
            post(migrate_memories),
        )
        .route(
            "/wunder/agents/{agent_id}/memories/replicate",
            post(replicate_memories),
        )
        .route(
            "/wunder/agents/{agent_id}/memories/{memory_id}/confirm",
            post(confirm_memory),
        )
        .route(
            "/wunder/agents/{agent_id}/memory-settings",
            get(get_memory_settings).post(update_memory_settings),
        )
}

#[derive(Debug, Deserialize, Default)]
struct AgentUserQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default, alias = "q")]
    query: Option<String>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    category: Option<String>,
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
    tag: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    title_l0: Option<String>,
    #[serde(default)]
    content_l2: Option<String>,
    #[serde(default)]
    supersedes_memory_id: Option<String>,
    #[serde(default)]
    valid_from: Option<f64>,
    #[serde(default)]
    confirmed_by_user: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ToggleRequest {
    #[serde(default)]
    value: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct MemoryAgentSettingsRequest {
    #[serde(default, alias = "autoExtractEnabled", alias = "auto_extract")]
    auto_extract_enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MemoryReplicationRequest {
    target_agent_id: String,
    #[serde(default)]
    overwrite: Option<bool>,
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
            category: query.tag.as_deref().or(query.category.as_deref()),
            limit: query.limit,
            ..Default::default()
        },
    );
    let tags = items
        .iter()
        .map(|item| item.category.clone())
        .filter(|item| !item.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "data": {
            "items": items.iter().map(public_memory_item).collect::<Vec<_>>(),
            "total": items.len(),
            "tags": tags,
        }
    })))
}

async fn get_memory_settings(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, &agent_id)?;
    let settings = AgentMemorySettingsService::new(state.storage.clone())
        .get_settings(&resolved.user.user_id, Some(&agent_id));
    Ok(Json(json!({ "data": { "settings": settings } })))
}

async fn update_memory_settings(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
    Json(payload): Json<MemoryAgentSettingsRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, &agent_id)?;
    let enabled = payload.auto_extract_enabled.unwrap_or(false);
    let settings = AgentMemorySettingsService::new(state.storage.clone())
        .set_auto_extract_enabled(&resolved.user.user_id, Some(&agent_id), enabled)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "settings": settings } })))
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
    Ok(Json(
        json!({ "data": { "item": public_memory_item(&item) } }),
    ))
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
    Ok(Json(
        json!({ "data": { "item": public_memory_item(&item) } }),
    ))
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
    Ok(Json(
        json!({ "data": { "item": public_memory_item(&item) } }),
    ))
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

async fn replicate_memories(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath(agent_id): AxumPath<String>,
    Query(query): Query<AgentUserQuery>,
    Json(payload): Json<MemoryReplicationRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, &agent_id)?;

    let target_agent_id = payload.target_agent_id.trim().to_string();
    if target_agent_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "target_agent_id is required".to_string(),
        ));
    }
    ensure_agent_access(
        &state,
        &resolved.user.user_id,
        &resolved.user,
        &target_agent_id,
    )?;

    let source_scope = normalize_agent_memory_scope(Some(&agent_id));
    let target_scope = normalize_agent_memory_scope(Some(target_agent_id.as_str()));
    if source_scope == target_scope {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "target agent must be different from source agent".to_string(),
        ));
    }
    if payload.overwrite == Some(false) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "overwrite=false is not supported".to_string(),
        ));
    }

    let fragment_store = MemoryFragmentStore::new(state.storage.clone());
    let _ = fragment_store.list_fragments(
        &resolved.user.user_id,
        Some(&agent_id),
        MemoryFragmentListOptions {
            include_invalidated: true,
            limit: Some(1),
            ..Default::default()
        },
    );

    let source_items = state
        .storage
        .list_memory_fragments(&resolved.user.user_id, &source_scope)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let deleted_target = clear_agent_memories(
        &fragment_store,
        &resolved.user.user_id,
        Some(target_agent_id.as_str()),
    );
    let legacy_store = MemoryStore::new(state.storage.clone());
    let deleted_legacy = legacy_store.clear_records(&build_agent_memory_owner(
        &resolved.user.user_id,
        Some(target_agent_id.as_str()),
    ));

    let mut id_map = HashMap::new();
    let mut copied_items = Vec::with_capacity(source_items.len());
    for source_item in &source_items {
        let mut copied = source_item.clone();
        let new_memory_id =
            generate_replicated_memory_id(&state, &resolved.user.user_id, &target_scope);
        id_map.insert(source_item.memory_id.clone(), new_memory_id.clone());
        copied.memory_id = new_memory_id;
        copied.agent_id = target_scope.clone();
        copied.supersedes_memory_id = None;
        copied.superseded_by_memory_id = None;
        state
            .storage
            .upsert_memory_fragment(&copied)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        copied_items.push(copied);
    }

    for copied in &mut copied_items {
        let source_original = source_items
            .iter()
            .find(|item| id_map.get(&item.memory_id) == Some(&copied.memory_id));
        if let Some(source_original) = source_original {
            copied.supersedes_memory_id = source_original
                .supersedes_memory_id
                .as_ref()
                .and_then(|value| id_map.get(value))
                .cloned();
            copied.superseded_by_memory_id = source_original
                .superseded_by_memory_id
                .as_ref()
                .and_then(|value| id_map.get(value))
                .cloned();
            state
                .storage
                .upsert_memory_fragment(copied)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
    }

    Ok(Json(json!({
        "data": {
            "source_agent_id": source_scope,
            "target_agent_id": target_scope,
            "copied": source_items.len(),
            "deleted_target": deleted_target + deleted_legacy,
            "overwrite": true
        }
    })))
}

async fn migrate_memories(
    state: State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    agent_id: AxumPath<String>,
    query: Query<AgentUserQuery>,
    payload: Json<MemoryReplicationRequest>,
) -> Result<Json<Value>, Response> {
    replicate_memories(state, headers, agent_id, query, payload).await
}

async fn confirm_memory(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    AxumPath((agent_id, memory_id)): AxumPath<(String, String)>,
    Query(query): Query<AgentUserQuery>,
    Json(payload): Json<ToggleRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    ensure_agent_access(&state, &resolved.user.user_id, &resolved.user, &agent_id)?;
    let store = MemoryFragmentStore::new(state.storage.clone());
    let item = store
        .set_confirmed(
            &resolved.user.user_id,
            Some(&agent_id),
            &memory_id,
            payload.value.unwrap_or(true),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                "memory fragment not found".to_string(),
            )
        })?;
    Ok(Json(
        json!({ "data": { "item": public_memory_item(&item) } }),
    ))
}

fn map_fragment_input(payload: MemoryFragmentRequest) -> MemoryFragmentInput {
    MemoryFragmentInput {
        memory_id: payload.memory_id,
        source_session_id: payload.source_session_id,
        source_round_id: payload.source_round_id,
        source_type: payload.source_type,
        category: payload.tag.or(payload.category),
        title_l0: payload.title_l0,
        content_l2: payload.content_l2,
        supersedes_memory_id: payload.supersedes_memory_id,
        valid_from: payload.valid_from,
        confirmed_by_user: payload.confirmed_by_user,
        ..Default::default()
    }
}

fn public_memory_item(item: &MemoryFragmentRecord) -> Value {
    json!({
        "memory_id": item.memory_id,
        "title_l0": item.title_l0,
        "content_l2": item.content_l2,
        "tag": item.category,
        "source_type": item.source_type,
        "status": item.status,
        "confirmed_by_user": item.confirmed_by_user,
        "supersedes_memory_id": item.supersedes_memory_id,
        "superseded_by_memory_id": item.superseded_by_memory_id,
        "valid_from": item.valid_from,
        "created_at": item.created_at,
        "updated_at": item.updated_at,
    })
}

fn generate_replicated_memory_id(state: &AppState, user_id: &str, agent_scope: &str) -> String {
    for _ in 0..16 {
        let candidate = compact_memory_id_for_model(&Uuid::new_v4().simple().to_string());
        if state
            .storage
            .get_memory_fragment(user_id, agent_scope, &candidate)
            .ok()
            .flatten()
            .is_none()
        {
            return candidate;
        }
    }
    format!("memf_{}", Uuid::new_v4().simple())
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

fn clear_agent_memories(store: &MemoryFragmentStore, user_id: &str, agent_id: Option<&str>) -> i64 {
    let mut deleted = 0i64;
    loop {
        let batch = store.list_fragments(
            user_id,
            agent_id,
            MemoryFragmentListOptions {
                include_invalidated: true,
                limit: Some(200),
                ..Default::default()
            },
        );
        if batch.is_empty() {
            break;
        }
        for item in &batch {
            if store.delete_fragment(user_id, agent_id, &item.memory_id) {
                deleted += 1;
            }
        }
        if batch.len() < 200 {
            break;
        }
    }
    deleted
}
