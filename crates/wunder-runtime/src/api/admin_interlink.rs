//! Admin (舰桥) governance surface for the cloud <-> local interlink fleet.
//!
//! Frozen contract: docs/云端本地互通方案.md §3.2 管理面, §5.2, §7.1, §9.2, §9.4,
//! §10, §13.5.
//!
//! **Authorization.** Every route lives under `/wunder/admin/*`, which the
//! process-wide guard (`auth::is_admin_path`, applied as middleware by
//! `wunder-server::api_key_guard` and `wunder-desktop::desktop_token_guard`)
//! only lets through with the configured API key or a bearer token of an admin
//! account. That is exactly the guard the existing `/wunder/admin/cloud/*`
//! handlers rely on (`api::cloud::admin_router` does no per-handler check), so
//! nothing here trusts the frontend. On top of it the mutating handlers
//! re-validate the target device (unknown / revoked / ownerless are refused)
//! and write an audit row naming the acting admin.
//!
//! **Boundaries** (§10, §13.6). Every listing is `offset`/`limit` bounded; the
//! fleet aggregates are computed from the scanned page plus the totals the
//! storage returns - never by loading the whole table; the CSV export is a
//! single bounded page of [`CSV_PAGE_MAX`] rows.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, Query as AxumQuery, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use wunder_core::interlink::{
    default_device_capabilities, NODE_STATUS_AWAY, NODE_STATUS_BUSY, NODE_STATUS_OFFLINE,
    NODE_STATUS_ONLINE, NODE_STATUS_RECONNECTING,
};

use crate::api::errors::error_response;
use crate::api::interlink_ws;
use crate::auth as guard_auth;
use crate::core::blocking;
use crate::services::interlink::{audit, commands, digest, registry, secret, LiveChannel};
use crate::services::presence::derive_device_status_with_tunnel;
use crate::state::AppState;
use crate::storage::{
    CloudDeviceInterlinkPatch, CloudDeviceRecord, InterlinkAuditRecord, InterlinkChannelRecord,
    InterlinkCommandRecord, ListInterlinkAuditQuery, ListInterlinkCommandsQuery,
};

/// Fleet page: default / hard cap (§13.6 - no unbounded admin query).
const FLEET_PAGE_DEFAULT: i64 = 50;
const FLEET_PAGE_MAX: i64 = 200;
/// Bounded channel window scanned once per fleet page to attach `rtt_ms`,
/// `resumed_count` and the 24h online-rate buckets.
const FLEET_CHANNEL_WINDOW: i64 = 500;

const CHANNEL_PAGE_DEFAULT: i64 = 50;
const CHANNEL_PAGE_MAX: i64 = 200;

const LEDGER_PAGE_DEFAULT: i64 = 50;
const LEDGER_PAGE_MAX: i64 = 200;

/// Audit JSON page bounds (shared with the service layer's own constants).
const AUDIT_PAGE_DEFAULT: i64 = audit::AUDIT_PAGE_DEFAULT;
const AUDIT_PAGE_MAX: i64 = audit::AUDIT_PAGE_MAX;
/// CSV export is **one bounded page** of at most this many rows. The bound is
/// echoed in the `x-wunder-audit-csv-max-rows` response header so the bridge
/// can state it next to the export button.
const CSV_PAGE_MAX: i64 = 2000;

/// 24h online-rate strip: 24 one-hour buckets ending at "now".
const ONLINE_RATE_BUCKETS: usize = 24;
const BUCKET_WIDTH_S: f64 = 3600.0;
/// Reconnecting TopN shown on the fleet overview.
const TOP_RECONNECTING: usize = 10;

/// Bounds for one admin policy patch (a bigger list is a mistake, not a need).
const POLICY_LIST_MAX: usize = 64;
const POLICY_ENTRY_MAX_CHARS: usize = 64;
/// Accepted values of `policy_overrides.shadow_mode`.
const SHADOW_MODES: [&str; 2] = ["minimal", "full"];
/// Largest offset an admin query may ask for (bounds pathological paging).
const OFFSET_MAX: i64 = 1_000_000;

/// Admin interlink routes under `/wunder/admin/interlink/*`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/interlink/fleet", get(get_fleet))
        .route("/wunder/admin/interlink/channels", get(get_channels))
        .route(
            "/wunder/admin/interlink/devices/{device_id}/policy",
            patch(patch_device_policy),
        )
        .route(
            "/wunder/admin/interlink/devices/{device_id}/rotate_secret",
            post(post_rotate_secret),
        )
        .route("/wunder/admin/interlink/commands", get(get_commands))
        .route("/wunder/admin/interlink/audit", get(get_audit))
}

// ---------------------------------------------------------------------------
// Query normalization (pure, unit-tested)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct FleetQuery {
    offset: Option<i64>,
    limit: Option<i64>,
    client: Option<String>,
    status: Option<String>,
    user_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ChannelQuery {
    offset: Option<i64>,
    limit: Option<i64>,
    user_id: Option<String>,
    device_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CommandQuery {
    offset: Option<i64>,
    limit: Option<i64>,
    user_id: Option<String>,
    device_id: Option<String>,
    kind: Option<String>,
    status: Option<String>,
    direction: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AuditQuery {
    offset: Option<i64>,
    limit: Option<i64>,
    user_id: Option<String>,
    device_id: Option<String>,
    action: Option<String>,
    since: Option<String>,
    until: Option<String>,
    format: Option<String>,
}

/// Trimmed non-empty filter; `None` for anything blank.
fn clean_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// Normalized page size, always inside `[1, max]`.
fn bounded_limit(requested: Option<i64>, default: i64, max: i64) -> i64 {
    requested
        .filter(|value| *value > 0)
        .unwrap_or(default)
        .clamp(1, max)
}

/// Normalized offset: never negative, capped so a stray value cannot scan past
/// the end of a table.
fn bounded_offset(requested: Option<i64>) -> i64 {
    requested
        .filter(|value| *value > 0)
        .unwrap_or(0)
        .min(OFFSET_MAX)
}

/// Coarse client family used by the fleet distribution (§5.2 按 client 分布).
fn client_family(client: &str) -> &'static str {
    let text = client.trim().to_ascii_lowercase();
    if text.contains("cli") {
        "cli"
    } else if text.contains("web") || text.contains("browser") {
        "web"
    } else {
        "desktop"
    }
}

/// A `client` filter, normalized to a family. `Some(None)` = no filter,
/// `Some(Some(family))` = valid filter, `None` = structural refusal (400) so a
/// typo cannot silently return the whole fleet.
fn normalize_client_filter(value: Option<&str>) -> Option<Option<String>> {
    let lowered = value.unwrap_or("").trim().to_ascii_lowercase();
    match lowered.as_str() {
        "" => Some(None),
        "desktop" | "cli" | "web" => Some(Some(lowered)),
        _ => None,
    }
}

/// A `status` filter, validated against the §5.1 status model. Same tri-state
/// shape as [`normalize_client_filter`].
fn normalize_status_filter(value: Option<&str>) -> Option<Option<String>> {
    let lowered = value.unwrap_or("").trim().to_ascii_lowercase();
    match lowered.as_str() {
        "" => Some(None),
        NODE_STATUS_ONLINE | NODE_STATUS_BUSY | NODE_STATUS_AWAY | NODE_STATUS_RECONNECTING
        | NODE_STATUS_OFFLINE => Some(Some(lowered)),
        _ => None,
    }
}

/// Ledger `device_id` filter. `interlink_commands` stores the node description
/// (`device:<id>` for a persistent node, `cloud` / `web:conn` otherwise) and the
/// store filters `to_node` by equality, so a bare id is expanded here.
/// Idempotent: an already-prefixed value passes through untouched.
fn node_filter(value: Option<String>) -> Option<String> {
    value.map(|raw| {
        // `cloud` is the server node itself and carries no `device:` prefix.
        if raw.contains(':') || raw == "cloud" {
            raw
        } else {
            format!("device:{raw}")
        }
    })
}

/// Time filter: epoch seconds, epoch milliseconds, or an RFC 3339 stamp.
fn parse_time_filter(value: Option<&str>) -> Option<f64> {
    let text = clean_filter(value)?;
    if let Ok(number) = text.parse::<f64>() {
        // Above 1e11 the value is milliseconds; seconds are still ~1.8e9.
        return Some(if number > 1e11 { number / 1000.0 } else { number });
    }
    chrono::DateTime::parse_from_rfc3339(&text)
        .ok()
        .map(|value| value.timestamp() as f64)
}

/// `format=csv` (case-insensitive) selects the CSV branch.
fn wants_csv(format: Option<&str>) -> bool {
    clean_filter(format)
        .map(|value| value.eq_ignore_ascii_case("csv"))
        .unwrap_or(false)
}

/// Deterministic export file name for a bounded CSV page.
fn csv_filename(now: f64) -> String {
    let stamp = chrono::DateTime::<chrono::Utc>::from_timestamp(now as i64, 0)
        .map(|value| value.format("%Y%m%dT%H%M%SZ").to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!("interlink-audit-{stamp}.csv")
}

// ---------------------------------------------------------------------------
// GET /wunder/admin/interlink/fleet
// ---------------------------------------------------------------------------

/// One fleet row: the persisted device plus its live tunnel and shadow facts.
#[derive(Debug, Clone)]
struct FleetRow {
    device_id: String,
    user_id: String,
    client: String,
    name: String,
    os: String,
    arch: String,
    app_version: String,
    status: String,
    /// Live tunnel in this process (the registry is the authority, §3.1(2)).
    connected: bool,
    /// Persisted `tunnel_connected` shadow column.
    tunnel_connected: Option<bool>,
    last_seen_at: f64,
    last_tunnel_at: Option<f64>,
    secret_version: i64,
    interlink_enabled: bool,
    revoked: bool,
    capabilities: Vec<String>,
    policy_overrides: Value,
    shadow_revision: i64,
    rtt_ms: Option<i64>,
    resumed_count: i64,
}

impl FleetRow {
    fn to_json(&self) -> Value {
        json!({
            "device_id": self.device_id,
            "user_id": self.user_id,
            "client": self.client,
            "name": self.name,
            "os": self.os,
            "arch": self.arch,
            "app_version": self.app_version,
            "status": self.status,
            "connected": self.connected,
            "tunnel_connected": self.tunnel_connected,
            "last_seen_at": self.last_seen_at,
            "last_tunnel_at": self.last_tunnel_at,
            "secret_version": self.secret_version,
            "interlink_enabled": self.interlink_enabled,
            "revoked": self.revoked,
            "capabilities": self.capabilities,
            "policy_overrides": self.policy_overrides,
            "shadow_revision": self.shadow_revision,
            "rtt_ms": self.rtt_ms,
            "resumed_count": self.resumed_count,
        })
    }
}

/// Fleet aggregates (§5.2). Status and per-client counts are page-scoped by
/// construction; `total` is the storage total for the same user filter.
#[derive(Debug, Default, Clone)]
struct FleetAggregates {
    total: i64,
    scanned: i64,
    online: i64,
    busy: i64,
    away: i64,
    reconnecting: i64,
    offline: i64,
    desktop: i64,
    cli: i64,
    web: i64,
    rtt_p50_ms: Option<i64>,
    rtt_p95_ms: Option<i64>,
    reconnecting_top: Vec<(String, i64)>,
}

impl FleetAggregates {
    fn to_json(&self) -> Value {
        json!({
            "total": self.total,
            "scanned": self.scanned,
            "online": self.online,
            "busy": self.busy,
            "away": self.away,
            "reconnecting": self.reconnecting,
            "offline": self.offline,
            "by_client": {
                "desktop": self.desktop,
                "cli": self.cli,
                "web": self.web,
            },
            "rtt_p50_ms": self.rtt_p50_ms,
            "rtt_p95_ms": self.rtt_p95_ms,
            "reconnecting_top": self
                .reconnecting_top
                .iter()
                .map(|(device_id, count)| json!({ "device_id": device_id, "resumed_count": count }))
                .collect::<Vec<Value>>(),
            // Honest scope marker: the counts cover the scanned page, not a
            // full-table pass (§13.6).
            "scope": "page",
        })
    }
}

/// Compute the aggregates over the scanned page. Pure so the rules are testable
/// without a database and without loading every row.
fn compute_aggregates(rows: &[FleetRow], total: i64) -> FleetAggregates {
    let mut out = FleetAggregates {
        total,
        scanned: rows.len() as i64,
        ..Default::default()
    };
    let mut rtts: Vec<i64> = Vec::with_capacity(rows.len());
    let mut reconnecting: Vec<(String, i64)> = Vec::new();
    for row in rows {
        match row.status.as_str() {
            NODE_STATUS_ONLINE => out.online += 1,
            NODE_STATUS_BUSY => out.busy += 1,
            NODE_STATUS_AWAY => out.away += 1,
            NODE_STATUS_RECONNECTING => out.reconnecting += 1,
            _ => out.offline += 1,
        }
        match client_family(&row.client) {
            "cli" => out.cli += 1,
            "web" => out.web += 1,
            _ => out.desktop += 1,
        }
        if let Some(rtt) = row.rtt_ms.filter(|value| *value >= 0) {
            rtts.push(rtt);
        }
        if row.resumed_count > 0 {
            reconnecting.push((row.device_id.clone(), row.resumed_count));
        }
    }
    out.rtt_p50_ms = percentile(&rtts, 0.50);
    out.rtt_p95_ms = percentile(&rtts, 0.95);
    reconnecting.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    reconnecting.truncate(TOP_RECONNECTING);
    out.reconnecting_top = reconnecting;
    out
}

/// Nearest-rank percentile over an unsorted sample; `None` for an empty one.
fn percentile(values: &[i64], quantile: f64) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = ((quantile.clamp(0.0, 1.0) * sorted.len() as f64).ceil() as usize)
        .clamp(1, sorted.len());
    sorted.get(rank - 1).copied()
}

/// One tunnel uptime interval, used only for the 24h strip.
#[derive(Debug, Clone)]
struct TunnelInterval {
    device_id: String,
    from: f64,
    to: f64,
}

/// Hourly online-rate buckets over the bounded channel window (§5.2 "24h 在线率
/// 热图"). `rate` is the share of the tunnel-bearing devices in the window that
/// held a channel during the hour - measured from retained channel history, not
/// a stored counter, so it degrades honestly when the window is short.
fn online_rate_24h(intervals: &[TunnelInterval], now: f64) -> Vec<Value> {
    if intervals.is_empty() {
        return Vec::new();
    }
    let distinct = distinct_device_count(intervals);
    let end = now.floor();
    let mut buckets = Vec::with_capacity(ONLINE_RATE_BUCKETS);
    // Reverse iteration: index 0 is the oldest hour, the last is the current.
    for index in (0..ONLINE_RATE_BUCKETS).rev() {
        let bucket_end = end - (index as f64 * BUCKET_WIDTH_S);
        let bucket_start = bucket_end - BUCKET_WIDTH_S;
        let mut active_ids: Vec<&str> = Vec::new();
        for item in intervals {
            if item.to > bucket_start && item.from < bucket_end {
                if !active_ids.contains(&item.device_id.as_str()) {
                    active_ids.push(item.device_id.as_str());
                }
            }
        }
        buckets.push(json!({
            "hour_start": bucket_start,
            "active": active_ids.len(),
            "rate": (active_ids.len() as f64 / distinct as f64 * 100.0).round() / 100.0,
        }));
    }
    buckets
}

fn distinct_device_count(intervals: &[TunnelInterval]) -> usize {
    let mut ids: Vec<&str> = Vec::with_capacity(intervals.len());
    for item in intervals {
        if !ids.contains(&item.device_id.as_str()) {
            ids.push(item.device_id.as_str());
        }
    }
    ids.len().max(1)
}

async fn get_fleet(
    State(state): State<Arc<AppState>>,
    AxumQuery(query): AxumQuery<FleetQuery>,
) -> Response {
    let config = state.config_store.get().await;
    if !config.interlink.enabled {
        return interlink_disabled_response();
    }
    let ttl = config.interlink.presence_ttl_s as f64;
    drop(config);

    let offset = bounded_offset(query.offset);
    let limit = bounded_limit(query.limit, FLEET_PAGE_DEFAULT, FLEET_PAGE_MAX);
    let Some(client_filter) = normalize_client_filter(query.client.as_deref()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "client filter must be one of desktop / cli / web".to_string(),
        );
    };
    let Some(status_filter) = normalize_status_filter(query.status.as_deref()) else {
        return error_response(
            StatusCode::BAD_REQUEST,
            "status filter must be one of online / busy / away / reconnecting / offline".to_string(),
        );
    };
    let user_filter = clean_filter(query.user_id.as_deref());

    // Live tunnels come from the in-process registry (bounded by connections);
    // everything else is a single bounded database round trip.
    let live: Vec<LiveChannel> = registry().snapshot();
    let live_by_device: HashMap<&str, &LiveChannel> = live
        .iter()
        .map(|channel| (channel.device_id.as_str(), channel))
        .collect();
    let now = now_unix_seconds();

    let storage = state.storage.clone();
    let user_lookup = user_filter.clone();
    let fetched = blocking::run_db("api.admin_interlink.fleet", move || {
        let (devices, total) = storage.list_cloud_devices(user_lookup.as_deref(), offset, limit)?;
        let (channels, _) =
            storage.list_interlink_channels(user_lookup.as_deref(), 0, FLEET_CHANNEL_WINDOW)?;
        let mut revisions = HashMap::with_capacity(devices.len());
        for device in devices.iter() {
            revisions.insert(
                device.device_id.clone(),
                storage.get_interlink_shadow_revision(&device.device_id)?,
            );
        }
        Ok::<_, anyhow::Error>((devices, total, channels, revisions))
    })
    .await;
    let (devices, total, channels, revisions) = match fetched {
        Ok(value) => value,
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };

    // Latest channel per device (`list_interlink_channels` is connected_at DESC).
    let mut channel_by_device: HashMap<&str, &InterlinkChannelRecord> = HashMap::new();
    for channel in channels.iter() {
        channel_by_device
            .entry(channel.device_id.as_str())
            .or_insert(channel);
    }

    let mut rows: Vec<FleetRow> = Vec::with_capacity(devices.len());
    for device in devices.iter() {
        let patch = device.interlink.clone().unwrap_or_default();
        let live_channel = live_by_device.get(device.device_id.as_str()).copied();
        let tunnel_live = if live_channel.is_some() {
            Some(true)
        } else {
            patch.tunnel_connected
        };
        // Same rules as the user-facing node list (§5.1): the tunnel signal
        // first, heartbeat freshness as the fallback; a revoked node is never
        // reported online (§13.5.18).
        let mut status = if device.revoked {
            NODE_STATUS_OFFLINE.to_string()
        } else {
            derive_device_status_with_tunnel(now, device.last_seen_at, ttl, tunnel_live).to_string()
        };
        if let Some(channel) = live_channel.filter(|_| !device.revoked) {
            match channel.presence_status.as_deref() {
                Some(NODE_STATUS_BUSY) => status = NODE_STATUS_BUSY.to_string(),
                Some(NODE_STATUS_AWAY) => status = NODE_STATUS_AWAY.to_string(),
                _ => {}
            }
        }

        let policy = digest::DevicePolicy::parse(patch.policy_overrides.as_deref());
        let authorized = patch
            .capabilities
            .as_deref()
            .map(parse_string_array)
            .unwrap_or_else(default_device_capabilities);
        // Effective caps: the live session's granted set when a tunnel exists,
        // otherwise what a fresh handshake would be granted right now (§9.2).
        let capabilities = match live_channel {
            Some(channel) => channel.capabilities.clone(),
            None => interlink_ws::grant_capabilities(&authorized, &authorized, &policy),
        };

        let channel_row = channel_by_device.get(device.device_id.as_str()).copied();
        let rtt_ms = live_channel
            .and_then(|channel| channel.rtt_ms)
            .or(channel_row.and_then(|row| row.rtt_ms));
        let resumed_count = channel_row.map(|row| row.resumed_count).unwrap_or(0);

        rows.push(FleetRow {
            device_id: device.device_id.clone(),
            user_id: device.user_id.clone(),
            client: device.client.clone(),
            name: device.name.clone(),
            os: device.os.clone().unwrap_or_default(),
            arch: device.arch.clone().unwrap_or_default(),
            app_version: device.app_version.clone().unwrap_or_default(),
            status,
            connected: live_channel.is_some(),
            tunnel_connected: patch.tunnel_connected,
            last_seen_at: device.last_seen_at,
            last_tunnel_at: patch.last_tunnel_at,
            secret_version: patch.secret_version,
            // `None` means "never configured" and reads as on, exactly as the
            // tunnel handler does.
            interlink_enabled: patch.interlink_enabled != Some(false),
            revoked: device.revoked,
            capabilities,
            policy_overrides: serde_json::to_value(&policy).unwrap_or_else(|_| json!({})),
            shadow_revision: revisions.get(&device.device_id).copied().unwrap_or(0),
            rtt_ms,
            resumed_count,
        });
    }

    let aggregates = compute_aggregates(&rows, total);
    let intervals: Vec<TunnelInterval> = channels
        .iter()
        .map(|channel| TunnelInterval {
            device_id: channel.device_id.clone(),
            from: channel.connected_at,
            to: channel.last_seen_at.max(channel.connected_at),
        })
        .collect();
    let online_rate = online_rate_24h(&intervals, now);

    // `client` / `status` are presentation filters over the scanned page: the
    // store has no column for either derived value, so they are applied here
    // while the page bounds stay authoritative.
    let devices_json: Vec<Value> = rows
        .iter()
        .filter(|row| {
            client_filter
                .as_deref()
                .is_none_or(|family| client_family(&row.client) == family)
                && status_filter
                    .as_deref()
                    .is_none_or(|status| row.status == status)
        })
        .map(FleetRow::to_json)
        .collect();

    Json(json!({
        "data": {
            "devices": devices_json,
            "total": total,
            "offset": offset,
            "limit": limit,
            "aggregates": aggregates.to_json(),
            "online_rate_24h": online_rate,
            "live_channels": live.len(),
            "now": now,
        }
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// GET /wunder/admin/interlink/channels
// ---------------------------------------------------------------------------

async fn get_channels(
    State(state): State<Arc<AppState>>,
    AxumQuery(query): AxumQuery<ChannelQuery>,
) -> Response {
    if !interlink_enabled(&state).await {
        return interlink_disabled_response();
    }
    let offset = bounded_offset(query.offset);
    let limit = bounded_limit(query.limit, CHANNEL_PAGE_DEFAULT, CHANNEL_PAGE_MAX);
    let user_filter = clean_filter(query.user_id.as_deref());
    let device_filter = clean_filter(query.device_id.as_deref());

    let storage = state.storage.clone();
    let user_lookup = user_filter.clone();
    let fetched = blocking::run_db("api.admin_interlink.channels", move || {
        storage.list_interlink_channels(user_lookup.as_deref(), offset, limit)
    })
    .await;
    let (channels, total) = match fetched {
        Ok(value) => value,
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };

    let live = registry().snapshot();
    let rows: Vec<Value> = channels
        .iter()
        // `list_interlink_channels` filters by user only, so the device filter
        // applies to the bounded page - one round trip, never a full scan.
        .filter(|channel| {
            device_filter
                .as_deref()
                .is_none_or(|device_id| channel.device_id == device_id)
        })
        .map(|channel| {
            let is_live = live.iter().any(|item| item.channel_id == channel.channel_id);
            json!({
                "channel_id": channel.channel_id,
                "device_id": channel.device_id,
                "user_id": channel.user_id,
                "client": channel.client,
                "instance_id": channel.instance_id,
                "protocol_version": channel.protocol_version,
                "caps": parse_or_json(channel.caps.as_deref()),
                "connected_at": channel.connected_at,
                "last_seen_at": channel.last_seen_at,
                "rtt_ms": channel.rtt_ms,
                "resumed_count": channel.resumed_count,
                "closed_reason": channel.closed_reason,
                "live": is_live,
            })
        })
        .collect();

    Json(json!({
        "data": {
            "channels": rows,
            "total": total,
            "offset": offset,
            "limit": limit,
            "device_filter_scope": "page",
        }
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// PATCH /wunder/admin/interlink/devices/{device_id}/policy
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct PolicyRequest {
    #[serde(default)]
    interlink_enabled: Option<bool>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
    #[serde(default)]
    policy_overrides: Option<PolicyOverridesBody>,
}

#[derive(Debug, Deserialize, Default)]
struct PolicyOverridesBody {
    #[serde(default)]
    disabled_kinds: Option<Vec<String>>,
    #[serde(default)]
    disabled_caps: Option<Vec<String>>,
    #[serde(default)]
    force_approval_kinds: Option<Vec<String>>,
    /// `"minimal"` / `"full"`; an empty string clears the override.
    #[serde(default)]
    shadow_mode: Option<String>,
}

/// A validated policy patch. `None` means "leave the stored value alone";
/// `shadow_mode: Some(None)` clears the override explicitly.
#[derive(Debug, Default, PartialEq)]
struct NormalizedPolicy {
    interlink_enabled: Option<bool>,
    capabilities: Option<Vec<String>>,
    disabled_kinds: Option<Vec<String>>,
    disabled_caps: Option<Vec<String>>,
    force_approval_kinds: Option<Vec<String>>,
    shadow_mode: Option<Option<String>>,
}

impl NormalizedPolicy {
    fn is_empty(&self) -> bool {
        self.interlink_enabled.is_none()
            && self.capabilities.is_none()
            && self.disabled_kinds.is_none()
            && self.disabled_caps.is_none()
            && self.force_approval_kinds.is_none()
            && self.shadow_mode.is_none()
    }

    /// Merge onto the stored overrides; absent lists keep their stored value.
    fn apply_to(&self, base: &digest::DevicePolicy) -> digest::DevicePolicy {
        let mut next = base.clone();
        if let Some(list) = &self.disabled_kinds {
            next.disabled_kinds = list.clone();
        }
        if let Some(list) = &self.disabled_caps {
            next.disabled_caps = list.clone();
        }
        if let Some(list) = &self.force_approval_kinds {
            next.force_approval_kinds = list.clone();
        }
        if let Some(mode) = &self.shadow_mode {
            next.shadow_mode = mode.clone();
        }
        next
    }
}

/// Validate an admin policy body (§9.2). Unknown kinds, capabilities or shadow
/// modes are refused with a readable reason instead of being ignored.
fn validate_policy(request: &PolicyRequest) -> Result<NormalizedPolicy, String> {
    let mut out = NormalizedPolicy {
        interlink_enabled: request.interlink_enabled,
        ..Default::default()
    };
    if let Some(list) = &request.capabilities {
        out.capabilities = Some(clean_caps(list)?);
    }
    let Some(body) = &request.policy_overrides else {
        return Ok(out);
    };
    if let Some(list) = &body.disabled_kinds {
        out.disabled_kinds = Some(clean_kinds(list)?);
    }
    if let Some(list) = &body.force_approval_kinds {
        out.force_approval_kinds = Some(clean_kinds(list)?);
    }
    if let Some(list) = &body.disabled_caps {
        out.disabled_caps = Some(clean_caps(list)?);
    }
    if let Some(mode) = &body.shadow_mode {
        let trimmed = mode.trim().to_ascii_lowercase();
        if trimmed.is_empty() {
            out.shadow_mode = Some(None);
        } else if SHADOW_MODES.contains(&trimmed.as_str()) {
            out.shadow_mode = Some(Some(trimmed));
        } else {
            return Err(format!(
                "shadow_mode must be minimal or full (an empty string clears it), got `{trimmed}`"
            ));
        }
    }
    Ok(out)
}

fn clean_kinds(list: &[String]) -> Result<Vec<String>, String> {
    let cleaned = clean_list(list, "command kind")?;
    for kind in cleaned.iter() {
        if !commands::is_known_kind(kind) {
            return Err(format!("unknown command kind `{kind}`"));
        }
    }
    Ok(cleaned)
}

fn clean_caps(list: &[String]) -> Result<Vec<String>, String> {
    let cleaned = clean_list(list, "capability")?;
    let known = interlink_ws::known_capabilities();
    for cap in cleaned.iter() {
        // `tool.exec:<whitelist>` (§9.2) is the one parameterised form.
        let base = cap.split(':').next().unwrap_or_default();
        if !known.contains(&base) {
            return Err(format!("unknown capability `{cap}`"));
        }
    }
    Ok(cleaned)
}

fn clean_list(list: &[String], label: &str) -> Result<Vec<String>, String> {
    if list.len() > POLICY_LIST_MAX {
        return Err(format!("too many {label} entries (max {POLICY_LIST_MAX})"));
    }
    let mut out: Vec<String> = Vec::with_capacity(list.len());
    for item in list {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            return Err(format!("empty {label} entry"));
        }
        if trimmed.chars().count() > POLICY_ENTRY_MAX_CHARS {
            return Err(format!(
                "{label} entry too long (max {POLICY_ENTRY_MAX_CHARS} characters)"
            ));
        }
        if !out.iter().any(|existing| existing == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    Ok(out)
}

async fn patch_device_policy(
    State(state): State<Arc<AppState>>,
    AxumPath(device_id): AxumPath<String>,
    headers: HeaderMap,
    Json(body): Json<PolicyRequest>,
) -> Response {
    if !interlink_enabled(&state).await {
        return interlink_disabled_response();
    }
    let Some(device_id) = clean_filter(Some(device_id.as_str())) else {
        return error_response(StatusCode::BAD_REQUEST, "device_id is required");
    };
    let normalized = match validate_policy(&body) {
        Ok(value) => value,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, message),
    };
    if normalized.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "empty policy patch: send interlink_enabled, capabilities or policy_overrides"
                .to_string(),
        );
    }

    let device = match load_device(&state, &device_id).await {
        Ok(Some(device)) => device,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "device not found"),
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    if let Err(response) = guard_mutable_device(&device) {
        return response;
    }

    let stored = device.interlink.clone().unwrap_or_default();
    let base_policy = digest::DevicePolicy::parse(stored.policy_overrides.as_deref());
    let next_policy = normalized.apply_to(&base_policy);
    let stored_enabled = stored.interlink_enabled != Some(false);
    let next_enabled = normalized.interlink_enabled.unwrap_or(stored_enabled);
    let stored_capabilities = stored
        .capabilities
        .as_deref()
        .map(parse_string_array)
        .unwrap_or_else(default_device_capabilities);
    let next_capabilities = normalized
        .capabilities
        .clone()
        .unwrap_or_else(|| stored_capabilities.clone());

    let capabilities_json = serde_json::to_string(&next_capabilities)
        .unwrap_or_else(|_| "[]".to_string());
    let policy_json = next_policy.to_json();
    let capabilities_changed =
        normalized.capabilities.is_some() && next_capabilities != stored_capabilities;
    let enabled_changed =
        normalized.interlink_enabled.is_some() && stored_enabled != next_enabled;
    let changed = next_policy != base_policy || capabilities_changed || enabled_changed;

    let patch = CloudDeviceInterlinkPatch {
        interlink_enabled: Some(next_enabled),
        capabilities: Some(capabilities_json),
        policy_overrides: Some(policy_json),
        ..Default::default()
    };
    let storage = state.storage.clone();
    let lookup = device_id.clone();
    if let Err(err) = blocking::run_db("api.admin_interlink.policy_update", move || {
        storage.update_cloud_device_interlink(&lookup, &patch)
    })
    .await
    {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
    }

    // A converged policy must not survive inside the running session
    // (§13.5.17): the live tunnel is dropped so the node re-handshakes against
    // the new grant instead of finishing its life on the old capability set.
    let tunnel_closed = if changed {
        interlink_ws::close_tunnel(&device_id, "policy_changed").await
    } else {
        false
    };

    let now = now_unix_seconds();
    if changed {
        let actor = admin_actor(&state, &headers).await;
        write_audit(
            &state,
            audit::record(
                "policy.update",
                &actor,
                Some("web"),
                Some(&format!("device:{device_id}")),
                None,
                None,
                vec![
                    ("interlink_enabled", json!(next_enabled)),
                    ("capabilities", json!(next_capabilities)),
                    ("disabled_kinds", json!(next_policy.disabled_kinds)),
                    ("disabled_caps", json!(next_policy.disabled_caps)),
                    (
                        "force_approval_kinds",
                        json!(next_policy.force_approval_kinds),
                    ),
                    ("shadow_mode", json!(next_policy.shadow_mode)),
                    ("tunnel_closed", json!(tunnel_closed)),
                ],
            ),
        )
        .await;
    }

    let effective =
        interlink_ws::grant_capabilities(&next_capabilities, &next_capabilities, &next_policy);
    let tunnel_live = registry().by_device(&device_id).is_some();
    Json(json!({
        "data": {
            "ok": true,
            "device_id": device_id,
            "changed": changed,
            "interlink_enabled": next_enabled,
            "capabilities": next_capabilities,
            "policy_overrides": serde_json::to_value(&next_policy).unwrap_or_else(|_| json!({})),
            "effective": {
                "capabilities": effective,
                "shadow_mode": next_policy.shadow_mode,
                "disabled_kinds": next_policy.disabled_kinds,
                "disabled_caps": next_policy.disabled_caps,
                "force_approval_kinds": next_policy.force_approval_kinds,
                "tunnel_live": tunnel_live,
            },
            "tunnel_closed": tunnel_closed,
            "updated_at": now,
        }
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// POST /wunder/admin/interlink/devices/{device_id}/rotate_secret
// ---------------------------------------------------------------------------

async fn post_rotate_secret(
    State(state): State<Arc<AppState>>,
    AxumPath(device_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    if !interlink_enabled(&state).await {
        return interlink_disabled_response();
    }
    let Some(device_id) = clean_filter(Some(device_id.as_str())) else {
        return error_response(StatusCode::BAD_REQUEST, "device_id is required");
    };
    let device = match load_device(&state, &device_id).await {
        Ok(Some(device)) => device,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "device not found"),
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    if let Err(response) = guard_mutable_device(&device) {
        return response;
    }

    let stored = device.interlink.clone().unwrap_or_default();
    // The secret itself is never stored and never returned: the server keeps
    // only `HMAC(pepper, derived)` so a handshake can be re-verified (§9.1).
    let version = stored.secret_version.max(0) + 1;

    let fingerprint = {
        let storage = state.storage.clone();
        let lookup = device_id.clone();
        blocking::run_db("api.admin_interlink.rotate_secret", move || {
            let pepper = secret::ensure_pepper(&storage)?;
            let derived = secret::derive_secret(&pepper, &lookup, version);
            Ok::<String, anyhow::Error>(secret::secret_hash(&pepper, &derived))
        })
        .await
    };
    let node_secret_hash = match fingerprint {
        Ok(value) => value,
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };

    let now = now_unix_seconds();
    let patch = CloudDeviceInterlinkPatch {
        node_secret_hash: Some(node_secret_hash),
        secret_version: version,
        secret_rotated_at: Some(now),
        ..Default::default()
    };
    let storage = state.storage.clone();
    let lookup = device_id.clone();
    if let Err(err) = blocking::run_db("api.admin_interlink.rotate_secret_store", move || {
        storage.update_cloud_device_interlink(&lookup, &patch)
    })
    .await
    {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
    }

    // The previous version stays acceptable inside its grace window, so the
    // tunnel is torn down explicitly rather than left to expire (§13.5.20).
    let tunnel_closed = interlink_ws::close_tunnel(&device_id, "secret_rotated").await;
    let actor = admin_actor(&state, &headers).await;
    write_audit(
        &state,
        audit::record(
            "secret.rotate",
            &actor,
            Some("web"),
            Some(&format!("device:{device_id}")),
            None,
            None,
            vec![
                ("secret_version", json!(version)),
                ("previous_version", json!(stored.secret_version)),
                ("tunnel_closed", json!(tunnel_closed)),
            ],
        ),
    )
    .await;

    Json(json!({
        "data": {
            "secret_version": version,
            "rotated_at": now,
            "tunnel_closed": tunnel_closed,
            "grace_s": secret::ROTATION_GRACE_S,
        }
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// GET /wunder/admin/interlink/commands
// ---------------------------------------------------------------------------

/// Ledger row projection. `args_digest` is parsed into its digest object, which
/// holds no payload by construction (`services::interlink::digest`); an argument
/// body is never part of this API (§9.3).
fn command_row(record: &InterlinkCommandRecord) -> Value {
    let duration_ms = record
        .finished_at
        .filter(|finished| *finished >= record.created_at)
        .map(|finished| ((finished - record.created_at) * 1000.0).round() as i64);
    let ack_latency_ms = record
        .acked_at
        .filter(|acked| *acked >= record.created_at)
        .map(|acked| ((acked - record.created_at) * 1000.0).round() as i64);
    json!({
        "command_id": record.command_id,
        "direction": record.direction,
        "actor_user_id": record.actor_user_id,
        "from_node": record.from_node,
        "to_node": record.to_node,
        "kind": record.kind,
        "args_digest": parse_or_json(record.args_digest.as_deref()),
        "approval_state": record.approval_state,
        "status": record.status,
        "created_at": record.created_at,
        "acked_at": record.acked_at,
        "finished_at": record.finished_at,
        "error_code": record.error_code,
        "error_summary": record.error_summary,
        "duration_ms": duration_ms,
        "ack_latency_ms": ack_latency_ms,
    })
}

async fn get_commands(
    State(state): State<Arc<AppState>>,
    AxumQuery(query): AxumQuery<CommandQuery>,
) -> Response {
    if !interlink_enabled(&state).await {
        return interlink_disabled_response();
    }
    let offset = bounded_offset(query.offset);
    let limit = bounded_limit(query.limit, LEDGER_PAGE_DEFAULT, LEDGER_PAGE_MAX);
    let user_filter = clean_filter(query.user_id.as_deref());
    // Cloud->local rows carry `to_node = device:<id>`; the store has no
    // from_node filter, so a device query covers the node as the target and the
    // response says which column it matched.
    let device_filter = node_filter(clean_filter(query.device_id.as_deref()));
    let kind_filter = clean_filter(query.kind.as_deref());
    let status_filter = clean_filter(query.status.as_deref());
    let direction_filter = clean_filter(query.direction.as_deref());
    if let Some(direction) = direction_filter.as_deref() {
        if !matches!(direction, "c2l" | "l2c") {
            return error_response(
                StatusCode::BAD_REQUEST,
                "direction must be c2l or l2c".to_string(),
            );
        }
    }
    if let Some(kind) = kind_filter.as_deref() {
        if !commands::is_known_kind(kind) {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("unknown command kind `{kind}`"),
            );
        }
    }

    let storage = state.storage.clone();
    let fetched = blocking::run_db("api.admin_interlink.commands", move || {
        let query = ListInterlinkCommandsQuery {
            user_id: user_filter.as_deref(),
            device_id: device_filter.as_deref(),
            kind: kind_filter.as_deref(),
            status: status_filter.as_deref(),
            direction: direction_filter.as_deref(),
            offset,
            limit,
        };
        storage.list_interlink_commands(query)
    })
    .await;
    let (rows, total) = match fetched {
        Ok(value) => value,
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };

    Json(json!({
        "data": {
            "commands": rows.iter().map(command_row).collect::<Vec<Value>>(),
            "total": total,
            "offset": offset,
            "limit": limit,
            // Which column a device filter matched, so the bridge can label it.
            "device_filter_field": "to_node",
        }
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// GET /wunder/admin/interlink/audit  (?format=csv)
// ---------------------------------------------------------------------------

fn audit_row(record: &InterlinkAuditRecord) -> Value {
    json!({
        "seq": record.seq,
        "command_id": record.command_id,
        "approval_id": record.approval_id,
        "actor": record.actor,
        "from_node": record.from_node,
        "to_node": record.to_node,
        "action": record.action,
        "detail_digest": parse_or_json(record.detail_digest.as_deref()),
        "created_at": record.created_at,
    })
}

async fn get_audit(
    State(state): State<Arc<AppState>>,
    AxumQuery(query): AxumQuery<AuditQuery>,
) -> Response {
    if !interlink_enabled(&state).await {
        return interlink_disabled_response();
    }
    let as_csv = wants_csv(query.format.as_deref());
    let default_limit = if as_csv { CSV_PAGE_MAX } else { AUDIT_PAGE_DEFAULT };
    let max_limit = if as_csv { CSV_PAGE_MAX } else { AUDIT_PAGE_MAX };
    let offset = bounded_offset(query.offset);
    let limit = bounded_limit(query.limit, default_limit, max_limit);
    let user_filter = clean_filter(query.user_id.as_deref());
    // Audit rows carry the node description on both ends, matched as
    // `from_node = ? OR to_node = ?`; a bare device id is expanded (§3.1(6)).
    let device_filter = node_filter(clean_filter(query.device_id.as_deref()));
    let action_filter = clean_filter(query.action.as_deref());
    let since = parse_time_filter(query.since.as_deref());
    let until = parse_time_filter(query.until.as_deref());
    if let (Some(since), Some(until)) = (since, until) {
        if until < since {
            return error_response(
                StatusCode::BAD_REQUEST,
                "until must not be earlier than since".to_string(),
            );
        }
    }

    let storage = state.storage.clone();
    let fetched = blocking::run_db("api.admin_interlink.audit", move || {
        let query = ListInterlinkAuditQuery {
            user_id: user_filter.as_deref(),
            device_id: device_filter.as_deref(),
            action: action_filter.as_deref(),
            since,
            until,
            offset,
            limit,
        };
        storage.list_interlink_audit(query)
    })
    .await;
    let (rows, total) = match fetched {
        Ok(value) => value,
        Err(err) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };

    if as_csv {
        // A single bounded page: at most `CSV_PAGE_MAX` rows per export, stated
        // by the `x-wunder-audit-csv-max-rows` header and mirrored in the JS.
        let body = audit::csv(&rows);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/csv; charset=utf-8"),
        );
        let disposition = format!(
            "attachment; filename=\"{}\"",
            csv_filename(now_unix_seconds())
        );
        insert_header(&mut headers, header::CONTENT_DISPOSITION, &disposition);
        insert_header(
            &mut headers,
            HeaderName::from_static("x-wunder-audit-csv-max-rows"),
            &CSV_PAGE_MAX.to_string(),
        );
        insert_header(
            &mut headers,
            HeaderName::from_static("x-wunder-audit-csv-rows"),
            &rows.len().to_string(),
        );
        return (headers, body).into_response();
    }

    Json(json!({
        "data": {
            "events": rows.iter().map(audit_row).collect::<Vec<Value>>(),
            "total": total,
            "offset": offset,
            "limit": limit,
            "csv_max_rows": CSV_PAGE_MAX,
        }
    }))
    .into_response()
}

fn insert_header(headers: &mut HeaderMap, name: HeaderName, value: &str) {
    if let Ok(parsed) = HeaderValue::try_from(value) {
        headers.insert(name, parsed);
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Unknown / revoked / ownerless devices are never mutable (§13.5.18).
fn guard_mutable_device(device: &CloudDeviceRecord) -> Result<(), Response> {
    if device.revoked {
        return Err(error_response(
            StatusCode::CONFLICT,
            "device is revoked: policy and secret operations are refused".to_string(),
        ));
    }
    if device.user_id.trim().is_empty() {
        return Err(error_response(
            StatusCode::CONFLICT,
            "device has no owning account: policy and secret operations are refused".to_string(),
        ));
    }
    Ok(())
}

async fn load_device(
    state: &AppState,
    device_id: &str,
) -> anyhow::Result<Option<CloudDeviceRecord>> {
    let storage = state.storage.clone();
    let lookup = device_id.to_string();
    blocking::run_db("api.admin_interlink.get_device", move || {
        storage.get_cloud_device(&lookup)
    })
    .await
}

async fn interlink_enabled(state: &AppState) -> bool {
    let config = state.config_store.get().await;
    config.interlink.enabled
}

fn interlink_disabled_response() -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "interlink is disabled on this server".to_string(),
    )
}

/// Actor name for an admin mutation. The middleware already proved the caller
/// is an admin or the API-key holder; this only resolves a readable identity.
async fn admin_actor(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(token) = guard_auth::extract_bearer_token(headers) {
        let user_store = state.user_store.clone();
        if let Ok(Some(user)) = blocking::run_db("api.admin_interlink.actor", move || {
            user_store.authenticate_token(&token)
        })
        .await
        {
            return user.user_id;
        }
        return "admin:token".to_string();
    }
    if guard_auth::extract_api_key(headers).is_some() {
        return "admin:api-key".to_string();
    }
    "admin".to_string()
}

async fn write_audit(state: &AppState, record: InterlinkAuditRecord) {
    let storage = state.storage.clone();
    let _ = blocking::run_db("api.admin_interlink.audit_write", move || {
        storage.insert_interlink_audit(&record)
    })
    .await;
}

/// A stored JSON string projected back into the response; a value that is not
/// valid JSON is returned verbatim instead of being dropped.
fn parse_or_json(value: Option<&str>) -> Value {
    match value {
        None => Value::Null,
        Some(text) => {
            serde_json::from_str::<Value>(text).unwrap_or(Value::String(text.to_string()))
        }
    }
}

fn parse_string_array(raw: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(raw) {
        Ok(list) if !list.is_empty() => list,
        _ => default_device_capabilities(),
    }
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

    fn row(device_id: &str, client: &str, status: &str, rtt: Option<i64>, resumed: i64) -> FleetRow {
        FleetRow {
            device_id: device_id.to_string(),
            user_id: "u_1".to_string(),
            client: client.to_string(),
            name: device_id.to_string(),
            os: String::new(),
            arch: String::new(),
            app_version: String::new(),
            status: status.to_string(),
            connected: status == NODE_STATUS_ONLINE,
            tunnel_connected: Some(status == NODE_STATUS_ONLINE),
            last_seen_at: 1_000.0,
            last_tunnel_at: None,
            secret_version: 1,
            interlink_enabled: true,
            revoked: false,
            capabilities: vec!["query.basic".to_string()],
            policy_overrides: json!({}),
            shadow_revision: 0,
            rtt_ms: rtt,
            resumed_count: resumed,
        }
    }

    fn sample_device() -> CloudDeviceRecord {
        CloudDeviceRecord {
            device_id: "d_1".to_string(),
            user_id: "u_1".to_string(),
            client: "desktop".to_string(),
            name: "node".to_string(),
            os: None,
            arch: None,
            app_version: None,
            last_seen_at: 0.0,
            created_at: 0.0,
            revoked: false,
            interlink: None,
        }
    }

    #[test]
    fn paging_is_bounded_and_never_negative() {
        assert_eq!(bounded_limit(None, FLEET_PAGE_DEFAULT, FLEET_PAGE_MAX), 50);
        assert_eq!(bounded_limit(Some(0), FLEET_PAGE_DEFAULT, FLEET_PAGE_MAX), 50);
        assert_eq!(bounded_limit(Some(-3), CHANNEL_PAGE_DEFAULT, CHANNEL_PAGE_MAX), 50);
        assert_eq!(bounded_limit(Some(9_999), FLEET_PAGE_DEFAULT, FLEET_PAGE_MAX), 200);
        assert_eq!(bounded_limit(Some(9_999), LEDGER_PAGE_DEFAULT, LEDGER_PAGE_MAX), 200);
        assert_eq!(bounded_limit(Some(9_999), AUDIT_PAGE_DEFAULT, AUDIT_PAGE_MAX), 500);
        // CSV keeps its own larger - but still bounded - single page.
        assert_eq!(bounded_limit(None, CSV_PAGE_MAX, CSV_PAGE_MAX), CSV_PAGE_MAX);
        assert_eq!(bounded_limit(Some(999_999), CSV_PAGE_MAX, CSV_PAGE_MAX), CSV_PAGE_MAX);
        assert_eq!(bounded_offset(None), 0);
        assert_eq!(bounded_offset(Some(-1)), 0);
        assert_eq!(bounded_offset(Some(120)), 120);
        assert_eq!(bounded_offset(Some(i64::MAX)), OFFSET_MAX);
    }

    #[test]
    fn filters_are_trimmed_and_validated() {
        assert_eq!(clean_filter(Some("  u_1 ")).as_deref(), Some("u_1"));
        assert_eq!(clean_filter(Some("   ")), None);
        assert_eq!(clean_filter(None), None);

        assert_eq!(
            normalize_client_filter(Some(" Desktop ")),
            Some(Some("desktop".to_string()))
        );
        assert_eq!(
            normalize_client_filter(Some("cli")),
            Some(Some("cli".to_string()))
        );
        assert_eq!(normalize_client_filter(None), Some(None));
        // Unknown families are a refusal, never "no filter".
        assert_eq!(normalize_client_filter(Some("phone")), None);
        assert_eq!(normalize_client_filter(Some("local_cli")), None);

        assert_eq!(
            normalize_status_filter(Some("RECONNECTING")),
            Some(Some("reconnecting".to_string()))
        );
        assert_eq!(normalize_status_filter(Some("sleeping")), None);
        assert_eq!(normalize_status_filter(Some("")), Some(None));

        assert_eq!(client_family("local-desktop"), "desktop");
        assert_eq!(client_family("cli"), "cli");
        assert_eq!(client_family("web"), "web");
    }

    #[test]
    fn aggregates_come_from_the_scanned_page_only() {
        let rows = vec![
            row("d_1", "desktop", NODE_STATUS_ONLINE, Some(40), 0),
            row("d_2", "local_cli", NODE_STATUS_BUSY, Some(60), 2),
            row("d_3", "desktop", NODE_STATUS_AWAY, None, 0),
            row("d_4", "web", NODE_STATUS_RECONNECTING, Some(20), 9),
            row("d_5", "desktop", NODE_STATUS_OFFLINE, None, 1),
        ];
        let aggregates = compute_aggregates(&rows, 1234);
        // `total` is the storage total; the counts cover the five scanned rows.
        assert_eq!(aggregates.total, 1234);
        assert_eq!(aggregates.scanned, 5);
        assert_eq!(aggregates.online, 1);
        assert_eq!(aggregates.busy, 1);
        assert_eq!(aggregates.away, 1);
        assert_eq!(aggregates.reconnecting, 1);
        assert_eq!(aggregates.offline, 1);
        assert_eq!((aggregates.desktop, aggregates.cli, aggregates.web), (3, 1, 1));
        // RTT sample is {40, 60, 20} -> nearest rank p50 = 40, p95 = 60.
        assert_eq!(aggregates.rtt_p50_ms, Some(40));
        assert_eq!(aggregates.rtt_p95_ms, Some(60));
        assert_eq!(
            aggregates.reconnecting_top,
            vec![
                ("d_4".to_string(), 9),
                ("d_2".to_string(), 2),
                ("d_5".to_string(), 1),
            ]
        );

        let json = aggregates.to_json();
        assert_eq!(json["by_client"]["desktop"].as_i64(), Some(3));
        assert_eq!(json["total"].as_i64(), Some(1234));
        assert_eq!(json["reconnecting_top"][0]["device_id"].as_str(), Some("d_4"));
        assert_eq!(
            json["reconnecting_top"][0]["resumed_count"].as_i64(),
            Some(9)
        );
        assert_eq!(json["rtt_p50_ms"].as_i64(), Some(40));
        assert_eq!(json["scope"].as_str(), Some("page"));
    }

    #[test]
    fn reconnecting_top_is_truncated_to_topn() {
        let rows: Vec<FleetRow> = (0..(TOP_RECONNECTING + 5))
            .map(|index| {
                row(
                    &format!("d_{index}"),
                    "desktop",
                    NODE_STATUS_ONLINE,
                    None,
                    index as i64 + 1,
                )
            })
            .collect();
        let aggregates = compute_aggregates(&rows, rows.len() as i64);
        assert_eq!(aggregates.reconnecting_top.len(), TOP_RECONNECTING);
        // Highest counts first.
        assert_eq!(
            aggregates.reconnecting_top[0].1,
            (TOP_RECONNECTING + 5) as i64
        );
    }

    #[test]
    fn aggregates_without_samples_report_no_percentiles() {
        let aggregates = compute_aggregates(
            &[row("d_1", "desktop", NODE_STATUS_OFFLINE, None, 0)],
            1,
        );
        assert_eq!(aggregates.rtt_p50_ms, None);
        assert_eq!(aggregates.rtt_p95_ms, None);
        assert!(aggregates.reconnecting_top.is_empty());
        assert_eq!(aggregates.offline, 1);
        assert_eq!(percentile(&[], 0.5), None);
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let sample: Vec<i64> = (1..=100).collect();
        assert_eq!(percentile(&sample, 0.5), Some(50));
        assert_eq!(percentile(&sample, 0.95), Some(95));
        assert_eq!(percentile(&sample, 0.0), Some(1));
        assert_eq!(percentile(&[7], 0.95), Some(7));
    }

    #[test]
    fn online_rate_buckets_the_bounded_channel_window() {
        let now = 100.0 * BUCKET_WIDTH_S;
        let intervals = vec![
            TunnelInterval {
                device_id: "d_1".to_string(),
                from: now - 600.0,
                to: now - 300.0,
            },
            TunnelInterval {
                device_id: "d_2".to_string(),
                from: now - 24.0 * BUCKET_WIDTH_S - 10.0,
                to: now - 24.0 * BUCKET_WIDTH_S + 10.0,
            },
        ];
        let buckets = online_rate_24h(&intervals, now);
        assert_eq!(buckets.len(), ONLINE_RATE_BUCKETS);
        let newest = &buckets[ONLINE_RATE_BUCKETS - 1];
        assert_eq!(newest["hour_start"].as_f64(), Some(now - BUCKET_WIDTH_S));
        assert_eq!(newest["active"].as_i64(), Some(1));
        assert_eq!(newest["rate"].as_f64(), Some(0.5));
        // Oldest bucket holds d_2; the second-newest is a real zero.
        assert_eq!(buckets[0]["active"].as_i64(), Some(1));
        assert_eq!(buckets[ONLINE_RATE_BUCKETS - 2]["active"].as_i64(), Some(0));
        assert!(online_rate_24h(&[], now).is_empty());
        // Two channels of the same device still count once.
        let again = vec![
            TunnelInterval {
                device_id: "d_1".to_string(),
                from: now - 100.0,
                to: now - 50.0,
            },
            TunnelInterval {
                device_id: "d_1".to_string(),
                from: now - 90.0,
                to: now - 10.0,
            },
        ];
        assert_eq!(
            online_rate_24h(&again, now)[ONLINE_RATE_BUCKETS - 1]["rate"]
                .as_f64(),
            Some(1.0)
        );
    }

    #[test]
    fn policy_body_validation_rejects_unknown_values() {
        let ok = PolicyRequest {
            interlink_enabled: Some(true),
            capabilities: Some(vec!["query.basic".to_string()]),
            policy_overrides: Some(PolicyOverridesBody {
                disabled_kinds: Some(vec!["workspace.write".to_string()]),
                disabled_caps: Some(vec!["tool.exec".to_string()]),
                force_approval_kinds: Some(vec!["thread.message".to_string()]),
                shadow_mode: Some("MINIMAL".to_string()),
            }),
        };
        let normalized = validate_policy(&ok).expect("valid policy");
        assert_eq!(normalized.interlink_enabled, Some(true));
        assert_eq!(normalized.shadow_mode, Some(Some("minimal".to_string())));
        assert_eq!(
            normalized.disabled_kinds,
            Some(vec!["workspace.write".to_string()])
        );
        assert_eq!(normalized.disabled_caps, Some(vec!["tool.exec".to_string()]));
        assert_eq!(
            normalized.force_approval_kinds,
            Some(vec!["thread.message".to_string()])
        );

        // Unknown shadow mode.
        let bad_mode = PolicyRequest {
            policy_overrides: Some(PolicyOverridesBody {
                shadow_mode: Some("aggressive".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(validate_policy(&bad_mode)
            .unwrap_err()
            .contains("shadow_mode"));

        // Unknown kinds and capabilities are refused, not silently ignored.
        let bad_kind = PolicyRequest {
            policy_overrides: Some(PolicyOverridesBody {
                disabled_kinds: Some(vec!["workspace.explode".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(validate_policy(&bad_kind)
            .unwrap_err()
            .contains("unknown command kind"));
        let bad_cap = PolicyRequest {
            capabilities: Some(vec!["everything".to_string()]),
            ..Default::default()
        };
        assert!(validate_policy(&bad_cap)
            .unwrap_err()
            .contains("unknown capability"));

        // `tool.exec:<whitelist>` is the accepted parameterised form (§9.2).
        assert!(validate_policy(&PolicyRequest {
            capabilities: Some(vec!["tool.exec:search".to_string()]),
            ..Default::default()
        })
        .is_ok());

        // Blank entries, oversize items and oversize lists are refused.
        assert!(validate_policy(&PolicyRequest {
            capabilities: Some(vec!["   ".to_string()]),
            ..Default::default()
        })
        .unwrap_err()
        .contains("empty capability"));
        let long = "x".repeat(POLICY_ENTRY_MAX_CHARS + 1);
        assert!(validate_policy(&PolicyRequest {
            capabilities: Some(vec![long]),
            ..Default::default()
        })
        .is_err());
        let too_many = vec!["query.basic".to_string(); POLICY_LIST_MAX + 1];
        assert!(validate_policy(&PolicyRequest {
            capabilities: Some(too_many),
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn policy_patch_merges_and_clears_overrides() {
        let base = digest::DevicePolicy {
            disabled_kinds: vec!["workspace.write".to_string()],
            disabled_caps: vec!["tool.exec".to_string()],
            force_approval_kinds: vec![],
            shadow_mode: Some("minimal".to_string()),
        };
        let patch = NormalizedPolicy {
            disabled_kinds: Some(vec!["agent.spawn".to_string()]),
            shadow_mode: Some(None),
            ..Default::default()
        };
        let next = patch.apply_to(&base);
        assert_eq!(next.disabled_kinds, vec!["agent.spawn".to_string()]);
        // Untouched lists keep their stored value (convergence is additive).
        assert_eq!(next.disabled_caps, vec!["tool.exec".to_string()]);
        assert_eq!(next.shadow_mode, None);
        assert!(!next.forces_minimal_shadow());
        assert_ne!(next, base);
        assert!(validate_policy(&PolicyRequest::default())
            .expect("an empty body is well formed")
            .is_empty());
    }

    #[test]
    fn csv_and_json_selection_is_explicit() {
        assert!(wants_csv(Some("csv")));
        assert!(wants_csv(Some("CSV ")));
        assert!(!wants_csv(Some("json")));
        assert!(!wants_csv(Some("")));
        assert!(!wants_csv(None));
    }

    #[test]
    fn node_filter_expands_a_bare_device_id_once() {
        assert_eq!(
            node_filter(Some("d_1".to_string())).as_deref(),
            Some("device:d_1")
        );
        // Already-prefixed and non-device nodes pass through untouched.
        assert_eq!(
            node_filter(Some("device:d_1".to_string())).as_deref(),
            Some("device:d_1")
        );
        assert_eq!(node_filter(Some("cloud".to_string())).as_deref(), Some("cloud"));
        assert_eq!(node_filter(None), None);
    }

    #[test]
    fn time_filters_accept_seconds_millis_and_rfc3339() {
        assert_eq!(parse_time_filter(Some("1700000000")), Some(1_700_000_000.0));
        assert_eq!(
            parse_time_filter(Some("1700000000123")),
            Some(1_700_000_000.123)
        );
        assert_eq!(parse_time_filter(Some("  ")), None);
        assert_eq!(parse_time_filter(Some("nonsense")), None);
        assert_eq!(
            parse_time_filter(Some("2026-01-01T00:00:00+00:00")),
            Some(1_767_225_600.0)
        );
        assert!(csv_filename(1_767_225_600.0).ends_with("20260101T000000Z.csv"));
    }

    #[test]
    fn ledger_projection_exposes_the_digest_object_and_no_body() {
        let record = InterlinkCommandRecord {
            command_id: "cmd_1".to_string(),
            direction: "c2l".to_string(),
            actor_user_id: "u_1".to_string(),
            from_node: "web:conn".to_string(),
            to_node: "device:d_1".to_string(),
            kind: "thread.message".to_string(),
            args_digest: Some(digest::digest_args(
                "thread.message",
                &json!({"local_thread_id": "th_1", "message": "keep me out"}),
            )),
            approval_state: "pending".to_string(),
            status: "acked".to_string(),
            created_at: 100.0,
            acked_at: Some(100.25),
            finished_at: Some(101.0),
            error_code: None,
            error_summary: None,
        };
        let value = command_row(&record);
        assert!(value["args_digest"].is_object());
        assert_eq!(value["args_digest"]["level"].as_str(), Some("L1"));
        assert_eq!(
            value["args_digest"]["fields"]["local_thread_id"].as_str(),
            Some("th_1")
        );
        assert_eq!(value["ack_latency_ms"].as_i64(), Some(250));
        assert_eq!(value["duration_ms"].as_i64(), Some(1000));
        // The payload never reaches the ledger response (§9.3).
        assert!(!value.to_string().contains("keep me out"));
    }

    #[test]
    fn audit_row_keeps_the_detail_digest() {
        let record = InterlinkAuditRecord {
            seq: 7,
            command_id: Some("cmd_1".to_string()),
            approval_id: None,
            actor: "u_1".to_string(),
            from_node: Some("web".to_string()),
            to_node: Some("device:d_1".to_string()),
            action: "policy.update".to_string(),
            detail_digest: Some(audit::detail_json(vec![("shadow_mode", json!("minimal"))])),
            created_at: 12.0,
        };
        let value = audit_row(&record);
        assert_eq!(value["seq"].as_i64(), Some(7));
        assert_eq!(value["action"].as_str(), Some("policy.update"));
        assert_eq!(
            value["detail_digest"]["shadow_mode"].as_str(),
            Some("minimal")
        );
    }

    #[test]
    fn mutable_device_guard_refuses_revoked_and_ownerless() {
        let mut device = sample_device();
        assert!(guard_mutable_device(&device).is_ok());
        device.revoked = true;
        assert_eq!(
            guard_mutable_device(&device).err().unwrap().status(),
            StatusCode::CONFLICT
        );
        device.revoked = false;
        device.user_id = "   ".to_string();
        assert_eq!(
            guard_mutable_device(&device).err().unwrap().status(),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn stored_json_projections_never_drop_a_value() {
        assert!(parse_or_json(None).is_null());
        assert_eq!(parse_or_json(Some("[\"a\"]"))[0].as_str(), Some("a"));
        assert_eq!(parse_or_json(Some("not json")).as_str(), Some("not json"));
        assert_eq!(parse_string_array("[]"), default_device_capabilities());
    }

    #[test]
    fn routes_stay_under_the_admin_guard() {
        // The process-wide guard keys on `/wunder/admin/*`; a route outside it
        // would be silently unauthenticated.
        for path in [
            "/wunder/admin/interlink/fleet",
            "/wunder/admin/interlink/channels",
            "/wunder/admin/interlink/devices/d_1/policy",
            "/wunder/admin/interlink/devices/d_1/rotate_secret",
            "/wunder/admin/interlink/commands",
            "/wunder/admin/interlink/audit",
        ] {
            assert!(guard_auth::is_admin_path(path), "{path} must be admin-gated");
        }
        // The user plane is deliberately not admin-gated (same-account scope).
        assert!(!guard_auth::is_admin_path("/wunder/interlink/nodes"));
    }
}
