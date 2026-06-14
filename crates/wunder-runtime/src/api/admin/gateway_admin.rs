use crate::api::admin::{error_response, now_ts};
use crate::gateway::GatewayNodeInvokeRequest;
use crate::i18n;
use crate::state::AppState;
use crate::storage::{GatewayNodeRecord, GatewayNodeTokenRecord};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::delete, routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/gateway/status", get(admin_gateway_status))
        .route(
            "/wunder/admin/gateway/presence",
            get(admin_gateway_presence),
        )
        .route("/wunder/admin/gateway/clients", get(admin_gateway_clients))
        .route(
            "/wunder/admin/gateway/nodes",
            get(admin_gateway_nodes).post(admin_gateway_nodes_upsert),
        )
        .route(
            "/wunder/admin/gateway/node_tokens",
            get(admin_gateway_node_tokens).post(admin_gateway_node_tokens_create),
        )
        .route(
            "/wunder/admin/gateway/node_tokens/{token}",
            delete(admin_gateway_node_tokens_delete),
        )
        .route("/wunder/admin/gateway/invoke", post(admin_gateway_invoke))
}

#[derive(Debug, Deserialize)]
struct GatewayClientQuery {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayNodeQuery {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayNodeUpsertRequest {
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    device_fingerprint: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct GatewayNodeTokenQuery {
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayNodeTokenCreateRequest {
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayNodeInvokeRequestPayload {
    node_id: String,
    command: String,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    timeout_s: Option<f64>,
    #[serde(default)]
    metadata: Option<Value>,
}

async fn admin_gateway_status(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let snapshot = state.control.gateway.snapshot().await;
    let nodes = state
        .storage
        .list_gateway_nodes(None)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let online_nodes = snapshot
        .items
        .iter()
        .filter(|item| item.role == "node")
        .count();
    let config = state.config_store.get().await;
    Ok(Json(json!({ "data": {
        "enabled": config.gateway.enabled,
        "protocol_version": config.gateway.protocol_version,
        "state_version": snapshot.state_version,
        "connections": snapshot.items.len(),
        "nodes_total": nodes.len(),
        "nodes_online": online_nodes
    }})))
}

async fn admin_gateway_presence(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, Response> {
    let snapshot = state.control.gateway.snapshot().await;
    Ok(Json(json!({ "data": {
        "state_version": snapshot.state_version,
        "items": snapshot.items
    }})))
}

async fn admin_gateway_clients(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GatewayClientQuery>,
) -> Result<Json<Value>, Response> {
    let status = query.status.as_deref().map(|value| value.trim());
    let records = state
        .storage
        .list_gateway_clients(status)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .into_iter()
        .map(|record| {
            json!({
                "connection_id": record.connection_id,
                "role": record.role,
                "user_id": record.user_id,
                "node_id": record.node_id,
                "scopes": record.scopes,
                "caps": record.caps,
                "commands": record.commands,
                "client_info": record.client_info,
                "status": record.status,
                "connected_at": record.connected_at,
                "last_seen_at": record.last_seen_at,
                "disconnected_at": record.disconnected_at
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_gateway_nodes(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GatewayNodeQuery>,
) -> Result<Json<Value>, Response> {
    let status = query.status.as_deref().map(|value| value.trim());
    let records = state
        .storage
        .list_gateway_nodes(status)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .into_iter()
        .map(|record| gateway_node_payload(&record))
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_gateway_nodes_upsert(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GatewayNodeUpsertRequest>,
) -> Result<Json<Value>, Response> {
    let node_id = payload
        .node_id
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("node_{}", Uuid::new_v4().simple()));
    let now = now_ts();
    let mut record = state
        .storage
        .get_gateway_node(&node_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .unwrap_or(GatewayNodeRecord {
            node_id: node_id.clone(),
            name: None,
            device_fingerprint: None,
            status: "active".to_string(),
            caps: Vec::new(),
            commands: Vec::new(),
            permissions: None,
            metadata: None,
            created_at: now,
            updated_at: now,
            last_seen_at: now,
        });
    if let Some(name) = payload.name {
        let trimmed = name.trim().to_string();
        if !trimmed.is_empty() {
            record.name = Some(trimmed);
        }
    }
    if let Some(status) = payload.status {
        let trimmed = status.trim().to_string();
        if !trimmed.is_empty() {
            record.status = trimmed;
        }
    }
    if let Some(fingerprint) = payload.device_fingerprint {
        let trimmed = fingerprint.trim().to_string();
        if !trimmed.is_empty() {
            record.device_fingerprint = Some(trimmed);
        }
    }
    if payload.metadata.is_some() {
        record.metadata = payload.metadata;
    }
    record.updated_at = now;
    let stored = record.clone();
    state
        .storage
        .upsert_gateway_node(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": gateway_node_payload(&stored) })))
}

async fn admin_gateway_node_tokens(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GatewayNodeTokenQuery>,
) -> Result<Json<Value>, Response> {
    let node_id = query.node_id.as_deref().map(|value| value.trim());
    let status = query.status.as_deref().map(|value| value.trim());
    let records = state
        .storage
        .list_gateway_node_tokens(node_id, status)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .into_iter()
        .map(|record| gateway_node_token_payload(&record))
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_gateway_node_tokens_create(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GatewayNodeTokenCreateRequest>,
) -> Result<Json<Value>, Response> {
    let node_id = payload
        .node_id
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("node_{}", Uuid::new_v4().simple()));
    let now = now_ts();
    if state
        .storage
        .get_gateway_node(&node_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .is_none()
    {
        let record = GatewayNodeRecord {
            node_id: node_id.clone(),
            name: None,
            device_fingerprint: None,
            status: "active".to_string(),
            caps: Vec::new(),
            commands: Vec::new(),
            permissions: None,
            metadata: None,
            created_at: now,
            updated_at: now,
            last_seen_at: now,
        };
        state
            .storage
            .upsert_gateway_node(&record)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let token = format!("gwn_{}", Uuid::new_v4().simple());
    let status = payload
        .status
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("active")
        .to_string();
    let record = GatewayNodeTokenRecord {
        token: token.clone(),
        node_id: node_id.clone(),
        status,
        created_at: now,
        updated_at: now,
        last_used_at: None,
    };
    state
        .storage
        .upsert_gateway_node_token(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": gateway_node_token_payload(&record) })))
}

async fn admin_gateway_node_tokens_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(token): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = token.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let removed = state
        .storage
        .delete_gateway_node_token(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "removed": removed } })))
}

async fn admin_gateway_invoke(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GatewayNodeInvokeRequestPayload>,
) -> Result<Json<Value>, Response> {
    let node_id = payload.node_id.trim();
    if node_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let command = payload.command.trim();
    if command.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let timeout_s = payload.timeout_s.unwrap_or(30.0);
    let result = state
        .control
        .gateway
        .invoke_node(GatewayNodeInvokeRequest {
            node_id: node_id.to_string(),
            command: command.to_string(),
            args: payload.args,
            timeout_s,
            metadata: payload.metadata,
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": {
        "ok": result.ok,
        "payload": result.payload,
        "error": result.error
    } })))
}

fn gateway_node_payload(record: &GatewayNodeRecord) -> Value {
    json!({
        "node_id": record.node_id,
        "name": record.name,
        "device_fingerprint": record.device_fingerprint,
        "status": record.status,
        "caps": record.caps,
        "commands": record.commands,
        "permissions": record.permissions,
        "metadata": record.metadata,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "last_seen_at": record.last_seen_at
    })
}

fn gateway_node_token_payload(record: &GatewayNodeTokenRecord) -> Value {
    json!({
        "token": record.token,
        "node_id": record.node_id,
        "status": record.status,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "last_used_at": record.last_used_at
    })
}
