//! Interlink (cloud <-> local) user API. See docs/云端本地互通方案.md §3.2.
//!
//! I2 scope: the unified node listing used by the bridge fleet page, the hive
//! "my devices" panel and local clients. Command/tunnel/shadow endpoints land
//! in later stages (I4-I11).

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use wunder_core::interlink::{
    default_device_capabilities, InterlinkNodeView, NODE_STATUS_ONLINE, NODE_TYPE_CLI,
    NODE_TYPE_DESKTOP, NODE_TYPE_SERVER,
};

use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::core::blocking;
use crate::services::presence::{aggregate_status, derive_device_status, online_count};
use crate::state::AppState;
use crate::storage::CloudDeviceRecord;

/// User-facing interlink routes. Shared by the web bridge and local clients.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/interlink/nodes", get(list_nodes))
        .route("/wunder/interlink/nodes/{device_id}/shadow", get(get_shadow))
}

/// `GET /wunder/interlink/nodes` - unified presence for the caller's account.
async fn list_nodes(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let resolved = match resolve_user(&state, &headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return response,
    };
    let user_id = resolved.user.user_id.clone();

    let config = state.config_store.get().await;
    if !config.interlink.enabled {
        return error_response(
            StatusCode::NOT_FOUND,
            "interlink is disabled on this server".to_string(),
        );
    }
    let ttl = config.interlink.presence_ttl_s as f64;

    let storage = state.storage.clone();
    let lookup = user_id.clone();
    let devices = match blocking::run_db("api.interlink.list_devices", move || {
        storage.list_cloud_devices(Some(&lookup), 0, 500)
    })
    .await
    {
        Ok((devices, _total)) => devices,
        Err(err) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
        }
    };

    let now = now_unix_seconds();

    // Backfill each node's shadow revision (0 = never synced). One blocking
    // batch read so a page stays a single round trip.
    let device_ids: Vec<String> = devices
        .iter()
        .filter(|device| !device.revoked)
        .map(|device| device.device_id.clone())
        .collect();
    let revisions = shadow_revisions(&state, device_ids).await;

    let mut persistent: Vec<InterlinkNodeView> = Vec::with_capacity(devices.len() + 1);
    for device in devices.iter().filter(|device| !device.revoked) {
        let revision = revisions.get(&device.device_id).copied().unwrap_or(0);
        persistent.push(device_node(device, now, ttl, revision));
    }
    persistent.push(server_node(&user_id, now));

    let registry = state.control.presence.nodes();
    let nodes = registry.compose_user_nodes(&user_id, now, ttl, persistent);
    let aggregate = aggregate_status(nodes.iter());
    let online = online_count(nodes.iter());
    let total = nodes.len();

    Json(json!({
        "data": {
            "nodes": nodes,
            "aggregate_status": aggregate,
            "online_count": online,
            "total": total,
        }
    }))
    .into_response()
}

/// Build the node view for a persistent cloud device.
fn device_node(
    device: &CloudDeviceRecord,
    now: f64,
    ttl: f64,
    shadow_revision: i64,
) -> InterlinkNodeView {
    let (capabilities, connected) = match &device.interlink {
        Some(patch) => (
            patch
                .capabilities
                .as_deref()
                .map(parse_capabilities)
                .unwrap_or_else(default_device_capabilities),
            patch.tunnel_connected.unwrap_or(false),
        ),
        None => (default_device_capabilities(), false),
    };

    InterlinkNodeView {
        node_id: format!("device:{}", device.device_id),
        node_type: map_client(&device.client).to_string(),
        user_id: device.user_id.clone(),
        label: device_label(device),
        status: derive_device_status(now, device.last_seen_at, ttl).to_string(),
        last_seen_at: device.last_seen_at,
        capabilities,
        shadow_revision,
        connected,
        meta: json!({}),
    }
}

/// The special `cloud` node representing the server itself (§8.1).
fn server_node(user_id: &str, now: f64) -> InterlinkNodeView {
    InterlinkNodeView {
        node_id: "cloud".to_string(),
        node_type: NODE_TYPE_SERVER.to_string(),
        user_id: user_id.to_string(),
        label: "cloud".to_string(),
        status: NODE_STATUS_ONLINE.to_string(),
        last_seen_at: now,
        capabilities: vec![
            "query.basic".to_string(),
            "thread.drive".to_string(),
            "workspace.write".to_string(),
        ],
        shadow_revision: 0,
        connected: true,
        meta: json!({}),
    }
}

fn map_client(client: &str) -> &'static str {
    if client.contains("cli") {
        NODE_TYPE_CLI
    } else {
        NODE_TYPE_DESKTOP
    }
}

fn device_label(device: &CloudDeviceRecord) -> String {
    let name = device.name.trim();
    if name.is_empty() {
        device.device_id.clone()
    } else {
        name.to_string()
    }
}

fn parse_capabilities(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_else(|_| default_device_capabilities())
}

/// Batch-read the shadow revision for each device (0 when never synced).
async fn shadow_revisions(state: &AppState, device_ids: Vec<String>) -> HashMap<String, i64> {
    let storage = state.storage.clone();
    blocking::run_db("api.interlink.shadow_revisions", move || {
        let mut map = HashMap::with_capacity(device_ids.len());
        for device_id in device_ids {
            let revision = storage.get_interlink_shadow_revision(&device_id)?;
            map.insert(device_id, revision);
        }
        Ok::<HashMap<String, i64>, anyhow::Error>(map)
    })
    .await
    .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// GET /wunder/interlink/nodes/{device_id}/shadow
// ---------------------------------------------------------------------------

/// Read one node's workspace shadow (docs 3.2). A node that never synced
/// answers the empty shadow (`revision:0`, null fields) rather than a 404,
/// which is easier for the frontend node switcher to render.
async fn get_shadow(
    State(state): State<Arc<AppState>>,
    AxumPath(device_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    let resolved = match resolve_user(&state, &headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return response,
    };
    let user_id = resolved.user.user_id.clone();

    let config = state.config_store.get().await;
    if !config.interlink.enabled {
        drop(config);
        return error_response(
            StatusCode::NOT_FOUND,
            "interlink is disabled on this server".to_string(),
        );
    }
    drop(config);

    // The shadow is readable only by the device owner; a missing, revoked or
    // foreign device answers the same 401 as the ticket endpoint.
    let storage = state.storage.clone();
    let lookup = device_id.clone();
    let device = match blocking::run_db("api.interlink.get_shadow_device", move || {
        storage.get_cloud_device(&lookup)
    })
    .await
    {
        Ok(device) => device,
        Err(err) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
        }
    };
    match device {
        Some(device) if !device.revoked && device.user_id == user_id => {}
        _ => return shadow_device_error(),
    }

    let storage = state.storage.clone();
    let lookup = device_id.clone();
    let shadow = match blocking::run_db("api.interlink.get_shadow", move || {
        storage.get_interlink_shadow(&lookup)
    })
    .await
    {
        Ok(shadow) => shadow,
        Err(err) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
        }
    };

    let (revision, summary, threads, tasks, workspace, synced_at) = match shadow {
        Some(record) => (
            record.revision,
            record.summary,
            record.threads,
            record.tasks,
            record.workspace,
            record.synced_at,
        ),
        None => (0, None, None, None, None, 0.0),
    };

    Json(json!({
        "data": {
            "device_id": device_id,
            "revision": revision,
            "summary": parse_or_string(summary),
            "threads": parse_or_string(threads),
            "tasks": parse_or_string(tasks),
            "workspace": parse_or_string(workspace),
            "synced_at": synced_at,
        }
    }))
    .into_response()
}

fn shadow_device_error() -> Response {
    error_response(
        StatusCode::UNAUTHORIZED,
        "device is revoked, unknown or not owned by this account".to_string(),
    )
}

/// Parse a stored JSON string back into a JSON value for the response; a plain
/// string that is not valid JSON is returned verbatim (never dropped).
fn parse_or_string(value: Option<String>) -> Value {
    match value {
        None => Value::Null,
        Some(text) => serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text)),
    }
}

fn now_unix_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[allow(dead_code)]
fn _assert_meta_is_value(_: Value) {}