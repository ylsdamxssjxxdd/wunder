//! Interlink (cloud <-> local) user API. See docs/云端本地互通方案.md §3.2.
//!
//! I2 scope: the unified node listing used by the bridge fleet page, the hive
//! "my devices" panel and local clients. Command/tunnel/shadow endpoints land
//! in later stages (I4-I11).

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, Query as AxumQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use wunder_core::interlink::{
    default_device_capabilities, APPROVAL_APPROVED, APPROVAL_EXPIRED, APPROVAL_PENDING,
    APPROVAL_REJECTED, COMMAND_STATUS_CANCELED, COMMAND_STATUS_FAILED, DIRECTION_C2L,
    DIRECTION_L2C, ERR_APPROVAL_EXPIRED, ERR_APPROVAL_REJECTED, ERR_CAP_DENIED,
    ERR_NODE_BUSY, ERR_NODE_OFFLINE, ERR_QUEUE_FULL, InterlinkNodeView, NODE_STATUS_AWAY,
    NODE_STATUS_BUSY, NODE_STATUS_ONLINE, NODE_TYPE_CLI, NODE_TYPE_DESKTOP, NODE_TYPE_SERVER,
};

use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::core::blocking;
use crate::services::presence::{
    aggregate_status, derive_device_status_with_tunnel, online_count,
};
use crate::services::interlink::{
    alerts, approvals, audit, blob, commands, digest, registry, secret,
};
use crate::state::AppState;
use crate::storage::{
    CloudDeviceInterlinkPatch, CloudDeviceRecord, InterlinkCommandRecord, ListInterlinkAuditQuery,
};
use crate::api::interlink_cloud_exec;
use crate::api::interlink_ws;
// The audit filter helpers are shared with the 舰桥 admin surface on purpose:
// one query string must mean the same thing on both ends.
use crate::api::admin_interlink;

/// User-facing interlink routes. Shared by the web bridge and local clients.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/interlink/nodes", get(list_nodes))
        .route("/wunder/interlink/node_secret", post(rotate_node_secret))
        .route("/wunder/interlink/nodes/{device_id}/shadow", get(get_shadow))
        .route("/wunder/interlink/nodes/{device_id}/purge_shadow", post(purge_shadow))
        .route("/wunder/interlink/nodes/{device_id}/enabled", patch(set_enabled))
        .route("/wunder/interlink/commands", post(issue_command))
        .route("/wunder/interlink/commands/{command_id}", get(get_command))
        .route("/wunder/interlink/commands/{command_id}/cancel", post(cancel_command))
        .route(
            "/wunder/interlink/commands/{command_id}/approval",
            post(decide_command_approval),
        )
        .route("/wunder/interlink/commands/{command_id}/blob", get(get_command_blob))
        .route("/wunder/interlink/audit", get(list_my_audit))
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
    // The live tunnel is the authoritative presence source when it exists
    // (docs §5.1: `online = tunnel up + beat ok`).
    let live_channels: HashMap<String, crate::services::interlink::LiveChannel> = registry()
        .snapshot()
        .into_iter()
        .map(|live| (live.device_id.clone(), live))
        .collect();

    let mut persistent: Vec<InterlinkNodeView> = Vec::with_capacity(devices.len() + 1);
    for device in devices.iter().filter(|device| !device.revoked) {
        let revision = revisions.get(&device.device_id).copied().unwrap_or(0);
        persistent.push(device_node(
            device,
            now,
            ttl,
            revision,
            live_channels.get(&device.device_id),
        ));
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
    live: Option<&crate::services::interlink::LiveChannel>,
) -> InterlinkNodeView {
    let (capabilities, connected, persisted_tunnel) = match &device.interlink {
        Some(patch) => (
            patch
                .capabilities
                .as_deref()
                .map(parse_capabilities)
                .unwrap_or_else(default_device_capabilities),
            patch.tunnel_connected.unwrap_or(false),
            // A device that never opened a tunnel has no tunnel signal at all,
            // so the heartbeat fallback decides (§4.4).
            if patch.last_tunnel_at.unwrap_or(0.0) > 0.0 {
                Some(patch.tunnel_connected.unwrap_or(false))
            } else {
                None
            },
        ),
        None => (default_device_capabilities(), false, None),
    };

    let status = match live {
        // A live tunnel answers immediately; its presence beat carries busy/away
        // (§5.1) and stays `online` when the node never reported otherwise.
        Some(live) => {
            let reported = live.presence_status.as_deref();
            if reported == Some(NODE_STATUS_BUSY) || reported == Some(NODE_STATUS_AWAY) {
                reported.unwrap_or(NODE_STATUS_ONLINE).to_string()
            } else if live.active_threads > 0 {
                NODE_STATUS_BUSY.to_string()
            } else {
                NODE_STATUS_ONLINE.to_string()
            }
        }
        None => derive_device_status_with_tunnel(now, device.last_seen_at, ttl, persisted_tunnel)
            .to_string(),
    };

    InterlinkNodeView {
        node_id: format!("device:{}", device.device_id),
        node_type: map_client(&device.client).to_string(),
        user_id: device.user_id.clone(),
        label: device_label(device),
        status,
        last_seen_at: live
            .map(|live| live.last_seen_at)
            .unwrap_or(device.last_seen_at),
        capabilities,
        shadow_revision,
        connected: live.is_some() || connected,
        meta: json!({
            "os": device.os,
            "arch": device.arch,
            "app_version": device.app_version,
            "device_id": device.device_id,
            "client": device.client,
            "active_threads": live.map(|live| live.active_threads).unwrap_or(0),
            "last_tunnel_at": device
                .interlink
                .as_ref()
                .and_then(|patch| patch.last_tunnel_at)
                .unwrap_or(0.0),
        }),
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

// ---------------------------------------------------------------------------
// POST /wunder/interlink/node_secret - key bootstrap and rotation (docs §9.1)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct NodeSecretRequest {
    #[serde(default)]
    device_id: Option<String>,
    /// Force a new version; the previous one stays valid for 24h.
    #[serde(default)]
    rotate: bool,
}

/// Hand a node its secret. The server derives it from its pepper so it can
/// re-verify the tunnel MAC later, and stores only the fingerprint.
async fn rotate_node_secret(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Option<Json<NodeSecretRequest>>,
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

    let request = body.map(|Json(request)| request);
    let rotate = request.as_ref().is_some_and(|value| value.rotate);
    let Some(device_id) = request
        .and_then(|value| value.device_id)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return error_response(StatusCode::BAD_REQUEST, "device_id is required".to_string());
    };

    let device = match load_device(&state, &device_id).await {
        Ok(Some(device)) => device,
        Ok(None) => return shadow_device_error(),
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    if device.revoked || device.user_id != user_id {
        return shadow_device_error();
    }

    let patch = device.interlink.clone().unwrap_or_default();
    let current_version = patch.secret_version;
    // First issuance pins version 1; a lost secret can be re-derived at the
    // same version, only an explicit rotation advances it.
    let version = if current_version <= 0 {
        1
    } else if rotate {
        current_version + 1
    } else {
        current_version
    };
    let needs_store = rotate || patch.node_secret_hash.is_none();

    let node_secret = {
        let storage = state.storage.clone();
        let device_id = device_id.clone();
        match blocking::run_db("api.interlink.node_secret", move || {
            let pepper = secret::ensure_pepper(&storage)?;
            let derived = secret::derive_secret(&pepper, &device_id, version);
            let hash = secret::secret_hash(&pepper, &derived);
            Ok::<(String, String), anyhow::Error>((derived, hash))
        })
        .await
        {
            Ok(pair) => pair,
            Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
        }
    };
    let (secret_value, secret_hash) = node_secret;

    if needs_store {
        let storage = state.storage.clone();
        let lookup = device_id.clone();
        let now = now_unix_seconds();
        let patch = CloudDeviceInterlinkPatch {
            node_secret_hash: Some(secret_hash),
            secret_version: version,
            secret_rotated_at: Some(now),
            ..Default::default()
        };
        if let Err(err) = blocking::run_db("api.interlink.node_secret_store", move || {
            storage.update_cloud_device_interlink(&lookup, &patch)
        })
        .await
        {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
        }
        // A rotated key invalidates the live tunnel immediately.
        if rotate {
            interlink_ws::close_tunnel(&device_id, "secret_rotated").await;
        }
        audit_write(
            &state,
            if rotate { "secret.rotate" } else { "secret.issue" },
            &user_id,
            &device_id,
            vec![("secret_version", json!(version))],
        )
        .await;
    }

    Json(json!({
        "data": {
            "secret": secret_value,
            "secret_version": version,
            "algorithm": "hmac-sha256",
            "device_id": device_id,
        }
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// POST /wunder/interlink/commands - the single entry for both directions
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct CommandRequest {
    /// `device:<id>` or `cloud`.
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    args: Option<Value>,
    /// Caller-supplied idempotency key (docs §4.3).
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    timeout_s: Option<f64>,
    /// Where the request came from; the server fills it in from the device
    /// header when the caller is a local node.
    #[serde(default)]
    from: Option<String>,
}

async fn issue_command(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Option<Json<CommandRequest>>,
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
    let limits = commands::Limits::from_config(&config.interlink);
    drop(config);

    let Some(request) = body.map(|Json(request)| request) else {
        return error_response(StatusCode::BAD_REQUEST, "a json body is required".to_string());
    };
    let Some(to) = request
        .to
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return error_response(StatusCode::BAD_REQUEST, "to is required".to_string());
    };
    let Some(kind) = request
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return error_response(StatusCode::BAD_REQUEST, "kind is required".to_string());
    };
    let args = request.args.clone().unwrap_or_else(|| json!({}));

    // Where did this come from? A local node sends its own device header; a
    // browser session is `web`.
    let from_node = request
        .from
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("x-wunder-device-id")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| format!("device:{value}"))
        })
        .unwrap_or_else(|| "web".to_string());

    let (direction, node) = if to == "cloud" {
        (DIRECTION_L2C, commands::NodeContext::default())
    } else {
        let Some(device_id) = commands::device_of(to) else {
            return error_response(
                StatusCode::BAD_REQUEST,
                "to must be device:<id> or cloud".to_string(),
            );
        };
        let device = match load_device(&state, &device_id).await {
            Ok(Some(device)) => device,
            Ok(None) => return shadow_device_error(),
            Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
        };
        if device.revoked || device.user_id != user_id {
            return shadow_device_error();
        }
        let patch = device.interlink.clone().unwrap_or_default();
        if !interlink_allowed(&patch) {
            return error_response(
                StatusCode::FORBIDDEN,
                "interlink is disabled for this device".to_string(),
            );
        }
        // The granted set of the live tunnel is authoritative while it lasts;
        // an offline node is judged by its stored authorization.
        let capabilities = registry()
            .by_device(&device_id)
            .map(|live| live.capabilities)
            .unwrap_or_else(|| {
                patch
                    .capabilities
                    .as_deref()
                    .map(parse_capabilities)
                    .unwrap_or_else(default_device_capabilities)
            });
        (
            DIRECTION_C2L,
            commands::NodeContext {
                device_id: Some(device_id.clone()),
                capabilities,
                policy: digest::DevicePolicy::parse(patch.policy_overrides.as_deref()),
            },
        )
    };

    let spec = commands::Spec {
        command_id: request.command_id.as_deref(),
        direction,
        actor_user_id: &user_id,
        from_node: &from_node,
        to_node: to,
        kind,
        args: &args,
        timeout_s: request.timeout_s,
    };
    let now = now_unix_seconds();
    let outcome = match commands::issue(state.storage.clone(), spec, &limits, node.clone(), now).await
    {
        Ok(outcome) => outcome,
        Err(commands::IssueError::Replay(existing)) => {
            return command_response(&existing, "replay", None, StatusCode::OK, &state).await;
        }
        Err(commands::IssueError::UnknownKind(kind)) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("unknown command kind: {kind}"),
            );
        }
        Err(commands::IssueError::Storage(err)) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
        }
    };

    // L3 dispatch goes to the governance hook (docs §9.4): the ledger audit
    // entries of a tunnel command carry no kind, so this is the one place the
    // tier of a device-targeted command is known. A refused dispatch is not an
    // execution and never alerts.
    if !matches!(outcome.dispatch, commands::Dispatch::Rejected(_)) {
        alerts::note_command(&outcome.record, Some(dispatch_name(&outcome.dispatch)));
    }

    // Local (cloud node) targets execute right here with the existing handlers.
    if outcome.dispatch == commands::Dispatch::LocalTarget {
        let state = state.clone();
        let command_id = outcome.record.command_id.clone();
        let kind = kind.to_string();
        let actor = outcome.record.actor_user_id.clone();
        let pending = outcome.record.approval_state == APPROVAL_PENDING;
        tokio::spawn(async move {
            // A pending ticket must be decided first; the waiter keeps the
            // original args in hand because the ledger stores digests only.
            if pending && !wait_for_cloud_approval(&state, &command_id).await {
                return;
            }
            interlink_cloud_exec::run(state, command_id, kind, actor, args).await;
        });
    }

    let status = match &outcome.dispatch {
        commands::Dispatch::Rejected(code) => rejected_status(code),
        _ => StatusCode::OK,
    };
    command_response(
        &outcome.record,
        dispatch_name(&outcome.dispatch),
        outcome
            .approval
            .as_ref()
            .map(|ticket| ticket.approval_id.clone())
            .as_deref(),
        status,
        &state,
    )
    .await
}

fn dispatch_name(dispatch: &commands::Dispatch) -> &'static str {
    match dispatch {
        commands::Dispatch::Sent => "sent",
        commands::Dispatch::Queued => "queued",
        commands::Dispatch::LocalTarget => "local",
        commands::Dispatch::Rejected(_) => "rejected",
    }
}

/// HTTP status of an admission refusal (docs §13.5 17): a capability or policy
/// denial is an authorization failure, while an unreachable or saturated node
/// is a temporary one. The ledger row is still returned in the body, so a
/// client that reads `data.status` keeps working.
fn rejected_status(code: &'static str) -> StatusCode {
    match code {
        ERR_CAP_DENIED => StatusCode::FORBIDDEN,
        ERR_NODE_OFFLINE | ERR_NODE_BUSY | ERR_QUEUE_FULL => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_GATEWAY,
    }
}

/// One response shape for every command entry point.
async fn command_response(
    record: &InterlinkCommandRecord,
    dispatch: &str,
    approval_id: Option<&str>,
    status: StatusCode,
    _state: &AppState,
) -> Response {
    let envelope = json!({
        "data": {
            "command_id": record.command_id,
            "direction": record.direction,
            "kind": record.kind,
            "status": record.status,
            "approval_state": record.approval_state,
            "approval_id": approval_id,
            "dispatch": dispatch,
            "to": record.to_node,
            "from": record.from_node,
            "created_at": record.created_at,
            "acked_at": record.acked_at,
            "finished_at": record.finished_at,
            "error_code": record.error_code,
            "error_summary": record.error_summary,
            "result": commands::hub().result(&record.command_id),
        }
    });
    (status, Json(envelope)).into_response()
}

// ---------------------------------------------------------------------------
// GET /wunder/interlink/commands/{id}
// ---------------------------------------------------------------------------

async fn get_command(
    State(state): State<Arc<AppState>>,
    AxumPath(command_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    let resolved = match resolve_user(&state, &headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return response,
    };
    let record = match load_command(&state, command_id.trim()).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "command not found".to_string());
        }
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    if record.actor_user_id != resolved.user.user_id {
        return error_response(StatusCode::FORBIDDEN, "not your command".to_string());
    }
    let digest_value = serde_json::from_str::<Value>(record.args_digest.as_deref().unwrap_or("{}"))
        .unwrap_or(Value::Null);
    Json(json!({
        "data": {
            "command_id": record.command_id,
            "direction": record.direction,
            "kind": record.kind,
            "status": record.status,
            "approval_state": record.approval_state,
            "created_at": record.created_at,
            "acked_at": record.acked_at,
            "finished_at": record.finished_at,
            "error_code": record.error_code,
            "error_summary": record.error_summary,
            "args_digest": digest_value,
            "result": commands::hub().result(&record.command_id),
            "queued_depth": commands::hub().queued_depth(Some(&record.actor_user_id)),
        }
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// POST /wunder/interlink/commands/{id}/cancel
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct CancelRequest {
    #[serde(default)]
    reason: Option<String>,
}

async fn cancel_command(
    State(state): State<Arc<AppState>>,
    AxumPath(command_id): AxumPath<String>,
    headers: HeaderMap,
    body: Option<Json<CancelRequest>>,
) -> Response {
    let resolved = match resolve_user(&state, &headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return response,
    };
    let reason = body
        .and_then(|Json(request)| request.reason)
        .map(|value| value.chars().take(64).collect::<String>())
        .unwrap_or_else(|| "user_cancel".to_string());
    let now = now_unix_seconds();
    let command_id = command_id.trim();
    match commands::cancel(state.storage.clone(), command_id, &resolved.user.user_id, now).await {
        Ok(commands::CancelOutcome::Canceled) => {
            let _ = reason;
            Json(json!({"data": {"ok": true, "command_id": command_id, "status": COMMAND_STATUS_CANCELED}})).into_response()
        }
        Ok(commands::CancelOutcome::AlreadyTerminal(status)) => Json(json!({
            "data": {"ok": false, "command_id": command_id, "status": status, "code": "ALREADY_FINISHED"}
        }))
        .into_response(),
        Ok(commands::CancelOutcome::Forbidden) => {
            error_response(StatusCode::FORBIDDEN, "not your command".to_string())
        }
        Ok(commands::CancelOutcome::Unknown) => {
            error_response(StatusCode::NOT_FOUND, "command not found".to_string())
        }
        Err(err) => error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

// ---------------------------------------------------------------------------
// POST /wunder/interlink/commands/{id}/approval - decide a pending ticket
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ApprovalDecisionRequest {
    decision: String,
}

/// Owner decision on a pending approval ticket (docs §7.3). The spawned cloud
/// executor polls the ticket; device targets see the same state through the
/// on-node prompt relay.
async fn decide_command_approval(
    State(state): State<Arc<AppState>>,
    AxumPath(command_id): AxumPath<String>,
    headers: HeaderMap,
    Json(body): Json<ApprovalDecisionRequest>,
) -> Response {
    let resolved = match resolve_user(&state, &headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return response,
    };
    let state_str = match body.decision.as_str() {
        "approved" => APPROVAL_APPROVED,
        "rejected" => APPROVAL_REJECTED,
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "decision must be approved or rejected".to_string(),
            )
        }
    };
    let record = match load_command(&state, command_id.trim()).await {
        Ok(Some(record)) => record,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "command not found".to_string()),
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    if record.actor_user_id != resolved.user.user_id {
        return error_response(StatusCode::FORBIDDEN, "not your command".to_string());
    }
    let Some(approval_id) = commands::approval_id_of(&record) else {
        return error_response(
            StatusCode::CONFLICT,
            "command carries no approval ticket".to_string(),
        );
    };
    let now = now_unix_seconds();
    let ticket = match approvals::decide(
        state.storage.clone(),
        &approval_id,
        state_str,
        &resolved.user.user_id,
        now,
    )
    .await
    {
        Ok(Some(ticket)) => ticket,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "approval not found".to_string()),
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    audit_write(
        &state,
        "approval.decide",
        &resolved.user.user_id,
        &ticket.device_id,
        vec![
            ("command_id", Value::String(record.command_id.clone())),
            ("state", Value::String(ticket.state.clone())),
        ],
    )
    .await;
    Json(json!({
        "data": {
            "approval_id": ticket.approval_id,
            "state": ticket.state,
            "command_id": record.command_id,
        }
    }))
    .into_response()
}

/// Poll the approval ticket of a pending cloud command. Returns `true` when
/// execution may proceed; finalizes the ledger row itself on any refusal, so
/// the command never terminates without a terminal state.
async fn wait_for_cloud_approval(state: &Arc<AppState>, command_id: &str) -> bool {
    let record = match load_command(state, command_id).await {
        Ok(Some(record)) => record,
        _ => return false,
    };
    let Some(approval_id) = commands::approval_id_of(&record) else {
        return false;
    };
    // Ticket TTL is 120s; the waiter outlives it so expiry is observed here.
    let deadline = now_unix_seconds() + 180.0;
    loop {
        let now = now_unix_seconds();
        if now > deadline {
            let _ = commands::finalize(
                state.storage.clone(),
                command_id,
                COMMAND_STATUS_FAILED,
                Some(now),
                Some(ERR_APPROVAL_EXPIRED),
                None,
            )
            .await;
            return false;
        }
        let storage = state.storage.clone();
        let lookup = approval_id.clone();
        let ticket = blocking::run_db("api.interlink.approval_poll", move || {
            storage.get_interlink_approval(&lookup)
        })
        .await
        .ok()
        .flatten();
        match ticket.map(|ticket| ticket.state) {
            Some(ticket_state) if ticket_state == APPROVAL_APPROVED => return true,
            Some(ticket_state) if ticket_state == APPROVAL_REJECTED => {
                let _ = commands::finalize(
                    state.storage.clone(),
                    command_id,
                    COMMAND_STATUS_CANCELED,
                    Some(now),
                    Some(ERR_APPROVAL_REJECTED),
                    None,
                )
                .await;
                return false;
            }
            Some(ticket_state) if ticket_state == APPROVAL_EXPIRED => {
                let _ = commands::finalize(
                    state.storage.clone(),
                    command_id,
                    COMMAND_STATUS_FAILED,
                    Some(now),
                    Some(ERR_APPROVAL_EXPIRED),
                    None,
                )
                .await;
                return false;
            }
            _ => tokio::time::sleep(std::time::Duration::from_secs(2)).await,
        }
    }
}

// ---------------------------------------------------------------------------
// GET /wunder/interlink/commands/{id}/blob - assembled file (docs §6.4)
// ---------------------------------------------------------------------------

async fn get_command_blob(
    State(state): State<Arc<AppState>>,
    AxumPath(command_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    let resolved = match resolve_user(&state, &headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return response,
    };
    let command_id = command_id.trim();
    let record = match load_command(&state, command_id).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "command not found".to_string());
        }
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    if record.actor_user_id != resolved.user.user_id {
        return error_response(StatusCode::FORBIDDEN, "not your command".to_string());
    }
    let Some(bytes) = blob::store().get(command_id) else {
        return error_response(
            StatusCode::NOT_FOUND,
            "blob is absent or expired (results are kept 10 minutes)".to_string(),
        );
    };
    let total = bytes.len();
    let mime = blob::store()
        .mime_of(command_id)
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // Range support lets the frontend resume an interrupted download.
    let range = headers
        .get(axum::http::header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_range)
        .filter(|(start, _end)| *start < total);
    let (start, end) = match range {
        Some((start, end)) => (start, end.min(total.saturating_sub(1))),
        None => (0, total.saturating_sub(1)),
    };
    if total == 0 {
        return (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, mime.as_str())],
            Vec::new(),
        )
            .into_response();
    }
    let slice = bytes[start..=end].to_vec();
    let partial = start > 0 || end + 1 < total;
    let status = if partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let mut response_headers = axum::http::HeaderMap::new();
    if let Ok(value) = axum::http::HeaderValue::from_str(mime.as_str()) {
        response_headers.insert(axum::http::header::CONTENT_TYPE, value);
    }
    response_headers.insert(
        axum::http::header::ACCEPT_RANGES,
        axum::http::HeaderValue::from_static("bytes"),
    );
    if partial {
        let range_header = format!("bytes {start}-{end}/{total}");
        if let Ok(value) = axum::http::HeaderValue::from_str(&range_header) {
            response_headers.insert(axum::http::header::CONTENT_RANGE, value);
        }
    }
    (status, response_headers, axum::body::Body::from(slice)).into_response()
}

/// `bytes=START-END` or `bytes=START-`; anything else is ignored.
fn parse_range(raw: &str) -> Option<(usize, usize)> {
    let spec = raw.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start: usize = start.trim().parse().ok()?;
    let end: usize = match end.trim().parse::<usize>() {
        Ok(value) => value,
        Err(_) => usize::MAX,
    };
    Some((start, end))
}

// ---------------------------------------------------------------------------
// POST /wunder/interlink/nodes/{id}/purge_shadow, PATCH .../enabled
// ---------------------------------------------------------------------------

async fn purge_shadow(
    State(state): State<Arc<AppState>>,
    AxumPath(device_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    let resolved = match resolve_user(&state, &headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return response,
    };
    let user_id = resolved.user.user_id.clone();
    let device_id = device_id.trim().to_string();
    match load_device(&state, &device_id).await {
        Ok(Some(device)) if !device.revoked && device.user_id == user_id => {}
        Ok(_) => return shadow_device_error(),
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }

    let storage = state.storage.clone();
    let lookup = device_id.clone();
    if let Err(err) =
        blocking::run_db("api.interlink.purge_shadow", move || storage.delete_interlink_shadow(&lookup))
            .await
    {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
    }
    audit_write(
        &state,
        "shadow.purge",
        &user_id,
        &device_id,
        vec![("by", json!("user"))],
    )
    .await;
    Json(json!({"data": {"ok": true, "device_id": device_id}})).into_response()
}

#[derive(Debug, Deserialize)]
struct EnabledRequest {
    enabled: bool,
}

/// The user-side kill switch for one node (docs §5.3, §13.5 16).
async fn set_enabled(
    State(state): State<Arc<AppState>>,
    AxumPath(device_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<EnabledRequest>,
) -> Response {
    let resolved = match resolve_user(&state, &headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return response,
    };
    let user_id = resolved.user.user_id.clone();
    let device_id = device_id.trim().to_string();
    match load_device(&state, &device_id).await {
        Ok(Some(device)) if !device.revoked && device.user_id == user_id => {}
        Ok(_) => return shadow_device_error(),
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }

    let storage = state.storage.clone();
    let lookup = device_id.clone();
    let patch = CloudDeviceInterlinkPatch {
        interlink_enabled: Some(request.enabled),
        ..Default::default()
    };
    if let Err(err) =
        blocking::run_db("api.interlink.set_enabled", move || {
            storage.update_cloud_device_interlink(&lookup, &patch)
        })
        .await
    {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
    }
    // Turning it off must actually tear the tunnel down, not merely refuse the
    // next connection (docs §5.3).
    if !request.enabled {
        interlink_ws::close_tunnel(&device_id, "kill_switch").await;
    }
    audit_write(
        &state,
        "policy.update",
        &user_id,
        &device_id,
        vec![("interlink_enabled", json!(request.enabled))],
    )
    .await;
    Json(json!({
        "data": {"ok": true, "device_id": device_id, "interlink_enabled": request.enabled}
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Load a cloud device with its interlink columns hydrated.
async fn load_device(
    state: &AppState,
    device_id: &str,
) -> anyhow::Result<Option<crate::storage::CloudDeviceRecord>> {
    let storage = state.storage.clone();
    let lookup = device_id.to_string();
    blocking::run_db("api.interlink.get_device", move || storage.get_cloud_device(&lookup)).await
}

async fn load_command(
    state: &AppState,
    command_id: &str,
) -> anyhow::Result<Option<InterlinkCommandRecord>> {
    let storage = state.storage.clone();
    let lookup = command_id.to_string();
    blocking::run_db("api.interlink.get_command", move || storage.get_interlink_command(&lookup)).await
}

/// `interlink_enabled == None` means "not yet configured" -> treat as on.
fn interlink_allowed(patch: &CloudDeviceInterlinkPatch) -> bool {
    patch.interlink_enabled != Some(false)
}

async fn audit_write(
    state: &AppState,
    action: &str,
    actor: &str,
    device_id: &str,
    detail: Vec<(&str, Value)>,
) {
    let storage = state.storage.clone();
    let row = audit::record(
        action,
        actor,
        Some("web"),
        Some(&format!("device:{device_id}")),
        None,
        None,
        detail,
    );
    let _ = blocking::run_db("api.interlink.audit", move || storage.insert_interlink_audit(&row)).await;
}

// ---------------------------------------------------------------------------
// GET /wunder/interlink/audit - the caller's own interlink audit trail
// ---------------------------------------------------------------------------

/// Self-service audit filters. The names and their interpretation are the
/// 舰桥 admin surface's (`action`, `device_id`, `since`, `until`), so one query
/// string behaves the same on both ends and a client never has to guess.
#[derive(Debug, Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    format: Option<String>,
    action: Option<String>,
    device_id: Option<String>,
    since: Option<String>,
    until: Option<String>,
}

/// Self-service audit view (docs §14 privacy line): a node owner can see every
/// interlink action recorded for their account. Digests only - no command
/// bodies exist in the ledger to leak.
async fn list_my_audit(State(state): State<Arc<AppState>>, headers: HeaderMap, AxumQuery(query): AxumQuery<AuditQuery>) -> Response {
    let resolved = match resolve_user(&state, &headers, None).await {
        Ok(resolved) => resolved,
        Err(response) => return response,
    };
    let user_id = resolved.user.user_id.clone();
    let limit = audit::page_limit(query.limit);
    let offset = audit::page_offset(query.offset);
    let action = admin_interlink::clean_filter(query.action.as_deref());
    let device = admin_interlink::node_filter(admin_interlink::clean_filter(
        query.device_id.as_deref(),
    ));
    let since = admin_interlink::parse_time_filter(query.since.as_deref());
    let until = admin_interlink::parse_time_filter(query.until.as_deref());
    let storage = state.storage.clone();
    let rows = match blocking::run_db(
        "api.interlink.my_audit",
        move || storage.list_interlink_audit(ListInterlinkAuditQuery {
            user_id: Some(&user_id),
            device_id: device.as_deref(),
            action: action.as_deref(),
            since,
            until,
            offset,
            limit,
        }),
    )
    .await
    {
        Ok((rows, _total)) => rows,
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    if query
        .format
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("csv"))
    {
        return (
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/csv; charset=utf-8".to_string(),
            )],
            audit::csv(&rows),
        )
            .into_response();
    }
    let items: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "seq": row.seq,
                "created_at": row.created_at,
                "actor": row.actor,
                "from_node": row.from_node,
                "to_node": row.to_node,
                "action": row.action,
                "command_id": row.command_id,
                "approval_id": row.approval_id,
                "detail_digest": audit::detail_value(row.detail_digest.as_deref()),
            })
        })
        .collect();
    Json(json!({"data": {"items": items, "limit": limit, "offset": offset}})).into_response()
}

fn now_unix_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parser_accepts_open_and_closed_forms() {
        assert_eq!(parse_range("bytes=0-99"), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-").map(|(start, _)| start), Some(10));
        assert_eq!(parse_range("bytes="), None);
        assert_eq!(parse_range("items=1-2"), None);
    }

    #[test]
    fn allowed_switch_is_fail_open_on_unset() {
        assert!(interlink_allowed(&CloudDeviceInterlinkPatch::default()));
        assert!(interlink_allowed(&CloudDeviceInterlinkPatch {
            interlink_enabled: Some(true),
            ..Default::default()
        }));
        assert!(!interlink_allowed(&CloudDeviceInterlinkPatch {
            interlink_enabled: Some(false),
            ..Default::default()
        }));
    }

    #[test]
    fn dispatch_names_match_the_documented_vocabulary() {
        assert_eq!(dispatch_name(&commands::Dispatch::Sent), "sent");
        assert_eq!(dispatch_name(&commands::Dispatch::Queued), "queued");
        assert_eq!(dispatch_name(&commands::Dispatch::LocalTarget), "local");
        assert_eq!(
            dispatch_name(&commands::Dispatch::Rejected("NODE_OFFLINE")),
            "rejected"
        );
    }
}