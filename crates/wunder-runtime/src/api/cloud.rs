//! Cloud access surface for locally logged-in desktop/cli clients.
//!
//! User-facing endpoints (bearer token, `local_desktop`/`local_cli` scope only):
//! device registration, account overview, exposed model list, the OpenAI
//! compatible call proxy (quota admission + per-user concurrency queue) and the
//! idempotent device-log upload. Admin endpoints for the bridge are exported as
//! [`admin_router`] and merged into the admin router.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, Query as AxumQuery, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::api::errors::{build_error_meta, error_response, ERROR_CODE_HEADER};
use crate::api::user_context::resolve_user;
use crate::core::blocking;
use crate::i18n;
use crate::llm::{
    build_model_auth_headers, build_openai_model_resource_endpoint, is_llm_model,
    resolve_model_base_url,
};
use crate::state::AppState;
use crate::storage::{
    CloudCallRecord, CloudDeviceLogRecord, CloudDeviceRecord, ListCloudDeviceLogsQuery,
    ListCloudRecordsQuery, UserQuotaStatus,
};
use crate::user_store::UserStore;

const SCOPE_LOCAL_DESKTOP: &str = "local_desktop";
const SCOPE_LOCAL_CLI: &str = "local_cli";

const CODE_CLOUD_SCOPE_FORBIDDEN: &str = "CLOUD_SCOPE_FORBIDDEN";
const CODE_DEVICE_REVOKED: &str = "DEVICE_REVOKED";
const CODE_CLOUD_DISABLED: &str = "CLOUD_DISABLED";
const CODE_MODEL_NOT_EXPOSED: &str = "MODEL_NOT_EXPOSED";
const CODE_CLOUD_BUSY: &str = "CLOUD_BUSY";
const CODE_PAYLOAD_TOO_LARGE: &str = "PAYLOAD_TOO_LARGE";
const CODE_USER_QUOTA_INSUFFICIENT: &str = "USER_QUOTA_INSUFFICIENT";
const CODE_UPSTREAM_ERROR: &str = "UPSTREAM_ERROR";

const CALL_STATUS_ADMITTED: &str = "admitted";
const CALL_STATUS_OK: &str = "ok";
const CALL_STATUS_UPSTREAM_ERROR: &str = "upstream_error";
const CALL_STATUS_QUOTA_BLOCKED: &str = "quota_blocked";

const DEVICE_HEADER: &str = "x-wunder-device-id";
const SESSION_HEADER: &str = "x-wunder-session-id";
const QUEUE_HEADER: &str = "x-wunder-queue-id";
const QUEUE_STARTED_HEADER: &str = "x-wunder-queue-started";
const REQUEST_ID_HEADER: &str = "x-wunder-request-id";
const QUOTA_BALANCE_HEADER: &str = "x-wunder-quota-balance";
const QUOTA_USED_HEADER: &str = "x-wunder-quota-used";

const MAX_LOG_BATCH_ITEMS: usize = 100;
const MAX_LOG_BATCH_BYTES: usize = 256 * 1024;
const MAX_CHAT_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_UPSTREAM_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_UPSTREAM_JSON_BODY_BYTES: usize = 4 * 1024 * 1024;
const ERROR_SUMMARY_MAX_CHARS: usize = 512;
const UPSTREAM_CONNECT_TIMEOUT_S: u64 = 10;

/// Idle TTL for a per-user admission entry with no active call and no waiter.
const ADMISSION_ENTRY_TTL: Duration = Duration::from_secs(600);
/// Sweep interval for reclaiming idle admission entries.
const ADMISSION_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/cloud/devices", post(register_device))
        .route("/wunder/cloud/account", get(account))
        .route("/wunder/cloud/v1/models", get(models))
        .route(
            "/wunder/cloud/v1/chat/completions",
            post(chat_completions).layer(DefaultBodyLimit::max(MAX_CHAT_BODY_BYTES)),
        )
        .route(
            "/wunder/cloud/logs",
            post(upload_logs).layer(DefaultBodyLimit::max(MAX_LOG_BATCH_BYTES + 64 * 1024)),
        )
}

pub(crate) fn admin_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/cloud/devices", get(admin_devices))
        .route("/wunder/admin/cloud/calls", get(admin_calls))
        .route("/wunder/admin/cloud/device_logs", get(admin_device_logs))
        .route(
            "/wunder/admin/cloud/devices/{device_id}",
            delete(admin_revoke_device),
        )
}

// ---------------------------------------------------------------------------
// Per-user concurrency admission with bounded FIFO waiting queue
// ---------------------------------------------------------------------------

struct WaitEntry {
    queue_id: String,
    enqueued_at: std::time::Instant,
}

struct UserAdmissionInner {
    active: usize,
    waiting: VecDeque<WaitEntry>,
    last_active_at: std::time::Instant,
}

/// In-memory admission state for one user: active call slots plus a bounded
/// FIFO waiting queue.
pub struct UserCloudAdmission {
    inner: Mutex<UserAdmissionInner>,
}

/// Process-wide admission controller. Entries are per user; idle entries are
/// TTL-reclaimed on a periodic sweep so the map never grows without bound.
pub struct CloudAdmissionController {
    users: Mutex<HashMap<String, Arc<UserCloudAdmission>>>,
    last_sweep: Mutex<std::time::Instant>,
}

impl CloudAdmissionController {
    pub fn new() -> Self {
        Self {
            users: Mutex::new(HashMap::new()),
            last_sweep: Mutex::new(std::time::Instant::now()),
        }
    }

    fn entry_for(&self, user_id: &str) -> Arc<UserCloudAdmission> {
        let mut users = self.users.lock().unwrap_or_else(|err| err.into_inner());
        let mut last_sweep = self
            .last_sweep
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if last_sweep.elapsed() >= ADMISSION_SWEEP_INTERVAL {
            let now = std::time::Instant::now();
            users.retain(|_, entry| {
                let inner = entry.inner.lock().unwrap_or_else(|err| err.into_inner());
                inner.active > 0
                    || !inner.waiting.is_empty()
                    || now.duration_since(inner.last_active_at) < ADMISSION_ENTRY_TTL
            });
            *last_sweep = now;
        }
        Arc::clone(users.entry(user_id.to_string()).or_insert_with(|| {
            Arc::new(UserCloudAdmission {
                inner: Mutex::new(UserAdmissionInner {
                    active: 0,
                    waiting: VecDeque::new(),
                    last_active_at: std::time::Instant::now(),
                }),
            })
        }))
    }

    /// Try to admit one call for `user_id`.
    ///
    /// - No `queue_id` and a free slot: start immediately.
    /// - `queue_id` equal to the queue front and a free slot: start (the wait
    ///   entry is consumed and reported back as `queue_started`).
    /// - Otherwise: enqueue (reusing the supplied id when present) and report
    ///   the queue position; a full queue yields [`AdmissionDecision::Busy`].
    pub fn admit(
        &self,
        user_id: &str,
        queue_id: Option<String>,
        max_active: usize,
        max_queue: usize,
    ) -> AdmissionDecision {
        let entry = self.entry_for(user_id);
        let mut inner = entry.inner.lock().unwrap_or_else(|err| err.into_inner());
        let now = std::time::Instant::now();
        inner
            .waiting
            .retain(|wait| now.duration_since(wait.enqueued_at) < ADMISSION_ENTRY_TTL);

        if let Some(id) = queue_id.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            if let Some(position) = inner.waiting.iter().position(|wait| wait.queue_id == id) {
                if position == 0 && inner.active < max_active {
                    let wait = inner.waiting.pop_front().expect("front entry exists");
                    inner.active += 1;
                    inner.last_active_at = now;
                    return AdmissionDecision::Start {
                        call_id: wait.queue_id,
                        queue_started: true,
                        waited_ms: now.duration_since(wait.enqueued_at).as_millis() as u64,
                    };
                }
                return AdmissionDecision::Queued {
                    queue_id: id.to_string(),
                    queue_ahead: position as u64,
                    newly_enqueued: false,
                };
            }
            if inner.waiting.len() >= max_queue {
                return AdmissionDecision::Busy;
            }
            inner.waiting.push_back(WaitEntry {
                queue_id: id.to_string(),
                enqueued_at: now,
            });
            inner.last_active_at = now;
            return AdmissionDecision::Queued {
                queue_id: id.to_string(),
                queue_ahead: (inner.waiting.len() - 1) as u64,
                newly_enqueued: true,
            };
        }

        if inner.active < max_active {
            inner.active += 1;
            inner.last_active_at = now;
            return AdmissionDecision::Start {
                call_id: Uuid::new_v4().to_string(),
                queue_started: false,
                waited_ms: 0,
            };
        }
        if inner.waiting.len() >= max_queue {
            return AdmissionDecision::Busy;
        }
        let queue_id = Uuid::new_v4().to_string();
        inner.waiting.push_back(WaitEntry {
            queue_id: queue_id.clone(),
            enqueued_at: now,
        });
        inner.last_active_at = now;
        AdmissionDecision::Queued {
            queue_ahead: (inner.waiting.len() - 1) as u64,
            queue_id,
            newly_enqueued: true,
        }
    }

    /// Release one active slot when a call finishes or fails.
    pub fn release(&self, user_id: &str) {
        if let Some(entry) = self
            .users
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(user_id)
            .cloned()
        {
            let mut inner = entry.inner.lock().unwrap_or_else(|err| err.into_inner());
            inner.active = inner.active.saturating_sub(1);
            inner.last_active_at = std::time::Instant::now();
        }
    }

    /// `(active, queued)` snapshot for the account overview.
    pub fn snapshot(&self, user_id: &str) -> (usize, usize) {
        let users = self.users.lock().unwrap_or_else(|err| err.into_inner());
        match users.get(user_id) {
            Some(entry) => {
                let inner = entry.inner.lock().unwrap_or_else(|err| err.into_inner());
                (inner.active, inner.waiting.len())
            }
            None => (0, 0),
        }
    }
}

impl Default for CloudAdmissionController {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum AdmissionDecision {
    /// The call may proceed now; `call_id` equals the queue id when the call
    /// was previously queued.
    Start {
        call_id: String,
        queue_started: bool,
        waited_ms: u64,
    },
    /// The call stays queued; the client retries later with the same id.
    Queued {
        queue_id: String,
        queue_ahead: u64,
        newly_enqueued: bool,
    },
    /// The waiting queue is full; reject without a queue id.
    Busy,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

struct CloudIdentity {
    user_id: String,
    is_admin: bool,
}

fn is_local_scope(scope: &str) -> bool {
    scope == SCOPE_LOCAL_DESKTOP || scope == SCOPE_LOCAL_CLI
}

async fn authenticate_cloud(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<CloudIdentity, Response> {
    let resolved = resolve_user(state, headers, None).await?;
    let scope_ok = resolved
        .session_scope
        .as_deref()
        .map(is_local_scope)
        .unwrap_or(false);
    if !scope_ok {
        return Err(cloud_error(
            StatusCode::FORBIDDEN,
            CODE_CLOUD_SCOPE_FORBIDDEN,
            "cloud access requires a local_desktop or local_cli session",
            None,
        ));
    }
    let is_admin = UserStore::is_admin(&resolved.user);
    Ok(CloudIdentity {
        user_id: resolved.user.user_id,
        is_admin,
    })
}

async fn require_cloud_enabled(state: &AppState) -> Result<(), Response> {
    let config = state.config_store.get().await;
    if config.cloud.enabled {
        return Ok(());
    }
    Err(cloud_error(
        StatusCode::NOT_FOUND,
        CODE_CLOUD_DISABLED,
        "cloud access is disabled on this server",
        None,
    ))
}

/// Every endpoint requires a live `x-wunder-device-id` owned by the caller and
/// not revoked; any mismatch is reported as `DEVICE_REVOKED` (401).
async fn require_device(
    state: &AppState,
    headers: &HeaderMap,
    user_id: &str,
) -> Result<CloudDeviceRecord, Response> {
    let device_id = headers
        .get(DEVICE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(device_id) = device_id else {
        return Err(device_revoked_error());
    };
    let storage = state.storage.clone();
    let lookup = device_id.to_string();
    let record = blocking::run_db("api.cloud.get_device", move || {
        storage.get_cloud_device(&lookup)
    })
    .await
    .map_err(internal_error)?;
    match record {
        Some(record) if !record.revoked && record.user_id == user_id => Ok(record),
        _ => Err(device_revoked_error()),
    }
}

fn device_revoked_error() -> Response {
    cloud_error(
        StatusCode::UNAUTHORIZED,
        CODE_DEVICE_REVOKED,
        "device is revoked, unknown or not owned by this account",
        None,
    )
}

fn internal_error(err: anyhow::Error) -> Response {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

/// Cloud-specific error envelope: unified error meta fields plus the extra
/// contract fields (queue position / quota account) inside the same `error`
/// object, as required by the cloud contract examples.
fn cloud_error(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    extra: Option<Value>,
) -> Response {
    let meta = build_error_meta(status, Some(code), message, None);
    let mut error = meta.to_value();
    if let (Some(target), Some(extra)) = (
        error.as_object_mut(),
        extra.as_ref().and_then(Value::as_object),
    ) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    let payload = json!({ "ok": false, "error": error });
    let mut response = (status, Json(payload)).into_response();
    if let Ok(value) = HeaderValue::from_str(code) {
        response.headers_mut().insert(ERROR_CODE_HEADER, value);
    }
    response
}

fn insert_call_record(state: &AppState, record: CloudCallRecord) {
    let storage = state.storage.clone();
    tokio::spawn(async move {
        let result = blocking::run_db("api.cloud.insert_call_record", move || {
            storage.insert_cloud_call_record(&record)
        })
        .await;
        if let Err(err) = result {
            tracing::warn!("cloud call record insert failed: {err}");
        }
    });
}

fn finalize_call_record(
    state: &AppState,
    call_id: String,
    status: &str,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    error_summary: Option<String>,
) {
    let storage = state.storage.clone();
    let status = status.to_string();
    tokio::spawn(async move {
        let finished_at = now_unix_seconds();
        let result = blocking::run_db("api.cloud.finalize_call_record", move || {
            storage.finalize_cloud_call_record(
                &call_id,
                &status,
                prompt_tokens,
                completion_tokens,
                finished_at,
                error_summary.as_deref(),
            )
        })
        .await;
        if let Err(err) = result {
            tracing::warn!("cloud call record finalize failed: {err}");
        }
    });
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        return raw.to_string();
    }
    raw.chars().take(max_chars).collect()
}

fn now_unix_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn quota_payload(status: &UserQuotaStatus) -> Value {
    json!({
        "balance": status.balance,
        "granted_total": status.granted_total,
        "used_total": status.used_total,
        "daily_grant": status.daily_grant,
        "last_grant_date": status.last_grant_date,
    })
}

fn set_header_i64(headers: &mut HeaderMap, name: &'static str, value: i64) {
    if let Ok(parsed) = HeaderValue::from_str(&value.to_string()) {
        headers.insert(axum::http::HeaderName::from_static(name), parsed);
    }
}

/// Resolve the exposed model set from config: `model_type == llm`, enabled and
/// either `expose: true` or listed in `cloud.expose_models`.
fn exposed_models<'a>(
    config: &'a wunder_core::config::Config,
) -> Vec<(&'a String, &'a wunder_core::config::LlmModelConfig)> {
    config
        .llm
        .models
        .iter()
        .filter(|(key, model)| {
            is_llm_model(model)
                && model.enable != Some(false)
                && (model.expose == Some(true)
                    || config
                        .cloud
                        .expose_models
                        .iter()
                        .any(|listed| listed == *key))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// POST /wunder/cloud/devices
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RegisterDeviceRequest {
    client: String,
    name: String,
    #[serde(default)]
    os: Option<String>,
    #[serde(default)]
    arch: Option<String>,
    #[serde(default)]
    app_version: Option<String>,
}

async fn register_device(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RegisterDeviceRequest>,
) -> Result<Json<Value>, Response> {
    require_cloud_enabled(&state).await?;
    let identity = authenticate_cloud(&state, &headers).await?;
    let client = payload.client.trim().to_ascii_lowercase();
    let name = payload.name.trim().to_string();
    if !matches!(client.as_str(), "desktop" | "cli") || name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "client must be desktop or cli and name must not be empty",
        ));
    }
    let now = now_unix_seconds();
    let storage = state.storage.clone();
    let user_id = identity.user_id.clone();
    let lookup_client = client.clone();
    let lookup_name = name.clone();
    let existing = blocking::run_db("api.cloud.find_device", move || {
        storage.find_cloud_device_by_identity(&user_id, &lookup_client, &lookup_name)
    })
    .await
    .map_err(internal_error)?;

    let record = match existing {
        Some(mut record) => {
            record.os = payload.os.clone();
            record.arch = payload.arch.clone();
            record.app_version = payload.app_version.clone();
            record.last_seen_at = now;
            record
        }
        None => CloudDeviceRecord {
            device_id: Uuid::new_v4().to_string(),
            user_id: identity.user_id.clone(),
            client: client.clone(),
            name: name.clone(),
            os: payload.os.clone(),
            arch: payload.arch.clone(),
            app_version: payload.app_version.clone(),
            last_seen_at: now,
            created_at: now,
            revoked: false,
        },
    };
    let storage = state.storage.clone();
    let upsert = record.clone();
    blocking::run_db("api.cloud.upsert_device", move || {
        storage.upsert_cloud_device(&upsert)
    })
    .await
    .map_err(internal_error)?;

    let config = state.config_store.get().await;
    Ok(Json(json!({
        "data": {
            "device_id": record.device_id,
            "max_concurrent_calls": config.cloud.max_concurrent_calls_per_user,
        }
    })))
}

// ---------------------------------------------------------------------------
// GET /wunder/cloud/account
// ---------------------------------------------------------------------------

async fn account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    require_cloud_enabled(&state).await?;
    let identity = authenticate_cloud(&state, &headers).await?;
    let device = require_device(&state, &headers, &identity.user_id).await?;

    let config = state.config_store.get().await;
    let storage = state.storage.clone();
    let user_id = identity.user_id.clone();
    let today = UserStore::today_string();
    let daily_grant = UserStore::default_daily_quota();
    let quota = blocking::run_db("api.cloud.prepare_quota", move || {
        storage.prepare_user_quota(&user_id, &today, daily_grant)
    })
    .await
    .map_err(internal_error)?;
    let quota = quota.unwrap_or(UserQuotaStatus {
        balance: 0,
        granted_total: 0,
        used_total: 0,
        daily_grant,
        last_grant_date: Some(UserStore::today_string()),
        allowed: false,
    });

    let (active, queued) = state.cloud_admission.snapshot(&identity.user_id);
    Ok(Json(json!({
        "data": {
            "user_id": identity.user_id,
            "quota": quota_payload(&quota),
            "concurrency": {
                "max_per_user": config.cloud.max_concurrent_calls_per_user,
                "active": active,
                "queued": queued,
            },
            "device": {
                "device_id": device.device_id,
                "revoked": device.revoked,
            },
        }
    })))
}

// ---------------------------------------------------------------------------
// GET /wunder/cloud/v1/models
// ---------------------------------------------------------------------------

async fn models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, Response> {
    require_cloud_enabled(&state).await?;
    let identity = authenticate_cloud(&state, &headers).await?;
    require_device(&state, &headers, &identity.user_id).await?;

    let config = state.config_store.get().await;
    let mut entries = exposed_models(&config);
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let data = entries
        .into_iter()
        .map(|(key, model)| {
            let mut entry = json!({
                "id": key,
                "object": "model",
                "owned_by": "wunder",
                "is_default": config.llm.default == key.as_str(),
            });
            if let Some(max_context) = model.max_context {
                entry["context"] = json!(max_context);
            }
            entry
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "object": "list", "data": data })))
}

// ---------------------------------------------------------------------------
// POST /wunder/cloud/v1/chat/completions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum PumpEnd {
    Completed,
    ClientGone,
    UpstreamError,
}

/// Bounded tail buffer used to recover the `usage` object from the final SSE
/// chunks without buffering the whole stream.
struct UsageTail {
    buf: Vec<u8>,
    cap: usize,
}

impl UsageTail {
    fn new(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(1024),
            cap,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        if chunk.len() >= self.cap {
            self.buf = chunk[chunk.len() - self.cap..].to_vec();
            return;
        }
        let overflow = (self.buf.len() + chunk.len()).saturating_sub(self.cap);
        if overflow > 0 {
            self.buf.drain(..overflow);
        }
        self.buf.extend_from_slice(chunk);
    }

    fn usage(&self) -> (Option<i64>, Option<i64>) {
        let needle = b"\"usage\"";
        let Some(start) = self
            .buf
            .windows(needle.len())
            .rposition(|window| window == needle)
        else {
            return (None, None);
        };
        let Some(brace) = self.buf[start..]
            .iter()
            .position(|byte| *byte == b'{')
            .map(|offset| start + offset)
        else {
            return (None, None);
        };
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        for (offset, byte) in self.buf[brace..].iter().enumerate() {
            let byte = *byte;
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        let slice = &self.buf[brace..=brace + offset];
                        if let Ok(value) = serde_json::from_slice::<Value>(slice) {
                            return (
                                value.get("prompt_tokens").and_then(Value::as_i64),
                                value.get("completion_tokens").and_then(Value::as_i64),
                            );
                        }
                        return (None, None);
                    }
                }
                _ => {}
            }
        }
        (None, None)
    }
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(UPSTREAM_CONNECT_TIMEOUT_S))
            .build()
            .expect("build upstream http client")
    })
}

fn usage_from_body(body: &Value) -> (Option<i64>, Option<i64>) {
    let usage = body.get("usage");
    (
        usage
            .and_then(|value| value.get("prompt_tokens"))
            .and_then(Value::as_i64),
        usage
            .and_then(|value| value.get("completion_tokens"))
            .and_then(Value::as_i64),
    )
}

async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(response) = require_cloud_enabled(&state).await {
        return response;
    }
    let identity = match authenticate_cloud(&state, &headers).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let device = match require_device(&state, &headers, &identity.user_id).await {
        Ok(device) => device,
        Err(response) => return response,
    };
    let config = state.config_store.get().await;
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("invalid JSON payload: {err}"),
            )
        }
    };
    let model_key = payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(model_key) = model_key else {
        return error_response(StatusCode::BAD_REQUEST, "model field is required");
    };
    let Some((_, model_config)) = exposed_models(&config)
        .into_iter()
        .find(|(key, _)| key.as_str() == model_key)
    else {
        return cloud_error(
            StatusCode::NOT_FOUND,
            CODE_MODEL_NOT_EXPOSED,
            "requested model is not exposed to cloud clients",
            None,
        );
    };
    let local_session_id = headers
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    // Quota admission: one debit per model call, admins exempt (same as the
    // web-side orchestrator quota path).
    let mut quota_status: Option<UserQuotaStatus> = None;
    if !identity.is_admin {
        let storage = state.storage.clone();
        let user_id = identity.user_id.clone();
        let today = UserStore::today_string();
        let daily_grant = UserStore::default_daily_quota();
        let consumed = blocking::run_db("api.cloud.consume_quota", move || {
            storage.consume_user_quota(&user_id, &today, daily_grant, 1)
        })
        .await;
        match consumed {
            Ok(status) => quota_status = status,
            Err(err) => return internal_error(err),
        }
        if quota_status.as_ref().is_some_and(|status| !status.allowed) {
            let call_id = Uuid::new_v4().to_string();
            // Persist synchronously on this error path so the record is durable
            // before the 429 reaches the client (no spawn race).
            let record_storage = state.storage.clone();
            let record = CloudCallRecord {
                call_id: call_id.clone(),
                device_id: device.device_id.clone(),
                user_id: identity.user_id.clone(),
                client: device.client.clone(),
                local_session_id: local_session_id.clone(),
                model: model_key.to_string(),
                provider: model_config.provider.clone(),
                status: CALL_STATUS_QUOTA_BLOCKED.to_string(),
                quota_consumed: false,
                queue_waited_ms: 0,
                prompt_tokens: None,
                completion_tokens: None,
                started_at: now_unix_seconds(),
                finished_at: None,
                duration_ms: None,
                error_summary: None,
            };
            if let Err(err) = blocking::run_db("api.cloud.insert_quota_blocked_record", move || {
                record_storage.insert_cloud_call_record(&record)
            })
            .await
            {
                tracing::warn!("cloud quota_blocked record insert failed: {err}");
            }
            let status = quota_status.clone().expect("quota status present");
            return cloud_error(
                StatusCode::TOO_MANY_REQUESTS,
                CODE_USER_QUOTA_INSUFFICIENT,
                i18n::t("error.user_quota_insufficient"),
                Some(json!({
                    "quota_account": {
                        "quota_balance": status.balance,
                        "quota_granted_total": status.granted_total,
                        "quota_used_total": status.used_total,
                        "daily_quota_grant": status.daily_grant,
                        "last_quota_grant_date": status.last_grant_date,
                    }
                })),
            )
            .with_call_id_header(&call_id);
        }
    }

    // Concurrency admission.
    let max_active = config.cloud.max_concurrent_calls_per_user.max(1);
    let max_queue = config.cloud.queue.max_queue_per_user;
    let retry_after_ms = config.cloud.queue.retry_after_ms;
    let queue_id = headers
        .get(QUEUE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let decision = state
        .cloud_admission
        .admit(&identity.user_id, queue_id, max_active, max_queue);
    let (call_id, queue_started, waited_ms) = match decision {
        AdmissionDecision::Start {
            call_id,
            queue_started,
            waited_ms,
        } => (call_id, queue_started, waited_ms),
        AdmissionDecision::Queued {
            queue_id,
            queue_ahead,
            newly_enqueued,
        } => {
            if newly_enqueued {
                // Admission-time accounting: one record per logical call (the
                // queue id doubles as the call id so retries stay idempotent).
                insert_call_record(
                    &state,
                    CloudCallRecord {
                        call_id: queue_id.clone(),
                        device_id: device.device_id.clone(),
                        user_id: identity.user_id.clone(),
                        client: device.client.clone(),
                        local_session_id: local_session_id.clone(),
                        model: model_key.to_string(),
                        provider: model_config.provider.clone(),
                        status: CALL_STATUS_ADMITTED.to_string(),
                        quota_consumed: true,
                        queue_waited_ms: 0,
                        prompt_tokens: None,
                        completion_tokens: None,
                        started_at: now_unix_seconds(),
                        finished_at: None,
                        duration_ms: None,
                        error_summary: None,
                    },
                );
            }
            return cloud_error(
                StatusCode::TOO_MANY_REQUESTS,
                CODE_CLOUD_BUSY,
                "cloud call concurrency limit reached",
                Some(json!({
                    "queue_id": queue_id.clone(),
                    "queue_ahead": queue_ahead,
                    "retry_after_ms": retry_after_ms,
                })),
            )
            .with_queue_id_header(&queue_id);
        }
        AdmissionDecision::Busy => {
            return cloud_error(
                StatusCode::TOO_MANY_REQUESTS,
                CODE_CLOUD_BUSY,
                "cloud call queue is full for this account",
                Some(json!({
                    "queue_full": true,
                    "retry_after_ms": retry_after_ms,
                })),
            );
        }
    };

    // Admission-time accounting for direct starts; queued calls already have
    // their record (keyed by the queue id).
    if !queue_started {
        insert_call_record(
            &state,
            CloudCallRecord {
                call_id: call_id.clone(),
                device_id: device.device_id.clone(),
                user_id: identity.user_id.clone(),
                client: device.client.clone(),
                local_session_id: local_session_id.clone(),
                model: model_key.to_string(),
                provider: model_config.provider.clone(),
                status: CALL_STATUS_ADMITTED.to_string(),
                quota_consumed: true,
                queue_waited_ms: waited_ms as i64,
                prompt_tokens: None,
                completion_tokens: None,
                started_at: now_unix_seconds(),
                finished_at: None,
                duration_ms: None,
                error_summary: None,
            },
        );
    }

    // Build the upstream request from the server-side model config; the caller
    // never sees api_key/base_url.
    let upstream_model = model_config
        .model
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| model_key.to_string());
    let Some(base_url) = resolve_model_base_url(model_config) else {
        return upstream_unavailable(
            &state,
            &identity.user_id,
            &call_id,
            "model base_url is not configured",
            "upstream model is not configured",
        );
    };
    let Some(endpoint) = build_openai_model_resource_endpoint(&base_url, "chat/completions") else {
        return upstream_unavailable(
            &state,
            &identity.user_id,
            &call_id,
            "model base_url is invalid",
            "upstream model endpoint is invalid",
        );
    };
    let mut upstream_payload = payload.clone();
    upstream_payload["model"] = json!(upstream_model);
    let is_stream = upstream_payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut request = http_client()
        .post(&endpoint)
        .headers(build_model_auth_headers(
            model_config.api_key.as_deref().unwrap_or_default(),
        ))
        .json(&upstream_payload);
    if !is_stream {
        let timeout_s = model_config.timeout_s.unwrap_or(120).max(1);
        request = request.timeout(Duration::from_secs(timeout_s));
    }
    let upstream = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            state.cloud_admission.release(&identity.user_id);
            finalize_call_record(
                &state,
                call_id.clone(),
                CALL_STATUS_UPSTREAM_ERROR,
                None,
                None,
                Some(truncate_chars(&err.to_string(), ERROR_SUMMARY_MAX_CHARS)),
            );
            return cloud_error(
                StatusCode::BAD_GATEWAY,
                CODE_UPSTREAM_ERROR,
                "upstream model request failed",
                None,
            )
            .with_call_id_header(&call_id);
        }
    };
    let upstream_status = upstream.status();

    let mut response_headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&call_id) {
        response_headers.insert(REQUEST_ID_HEADER, value);
    }
    if queue_started {
        response_headers.insert(QUEUE_STARTED_HEADER, HeaderValue::from_static("1"));
    }
    if let Some(status) = quota_status.as_ref() {
        set_header_i64(&mut response_headers, QUOTA_BALANCE_HEADER, status.balance);
        set_header_i64(&mut response_headers, QUOTA_USED_HEADER, status.used_total);
    }

    if !upstream_status.is_success() {
        let body_text = read_bounded_body(upstream, MAX_UPSTREAM_ERROR_BODY_BYTES).await;
        state.cloud_admission.release(&identity.user_id);
        finalize_call_record(
            &state,
            call_id.clone(),
            CALL_STATUS_UPSTREAM_ERROR,
            None,
            None,
            Some(truncate_chars(
                &format!("upstream status {upstream_status}: {body_text}"),
                ERROR_SUMMARY_MAX_CHARS,
            )),
        );
        let mut response = Response::builder()
            .status(upstream_status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body_text))
            .expect("static response parts");
        response.headers_mut().extend(response_headers.drain());
        return response;
    }

    if !is_stream {
        let body_bytes = read_bounded_body(upstream, MAX_UPSTREAM_JSON_BODY_BYTES).await;
        let (prompt_tokens, completion_tokens) = serde_json::from_str::<Value>(&body_bytes)
            .map(|body| usage_from_body(&body))
            .unwrap_or((None, None));
        state.cloud_admission.release(&identity.user_id);
        finalize_call_record(
            &state,
            call_id.clone(),
            CALL_STATUS_OK,
            prompt_tokens,
            completion_tokens,
            None,
        );
        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body_bytes))
            .expect("static response parts");
        response.headers_mut().extend(response_headers.drain());
        return response;
    }

    // Stream relay: bounded channel provides backpressure; the pump task owns
    // the upstream stream and finalizes the record on completion, upstream
    // failure or client disconnect.
    let (tx, rx) = mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(16);
    let pump_state = state.clone();
    let pump_user_id = identity.user_id.clone();
    let pump_call_id = call_id.clone();
    tokio::spawn(async move {
        let mut stream = upstream.bytes_stream();
        let mut tail = UsageTail::new(64 * 1024);
        let mut end = PumpEnd::Completed;
        while let Some(item) = stream.next().await {
            match item {
                Ok(chunk) => {
                    tail.push(&chunk);
                    if tx.send(Ok(chunk)).await.is_err() {
                        end = PumpEnd::ClientGone;
                        break;
                    }
                }
                Err(err) => {
                    end = PumpEnd::UpstreamError;
                    let _ = tx.send(Err(std::io::Error::other(err.to_string()))).await;
                    break;
                }
            }
        }
        drop(tx);
        pump_state.cloud_admission.release(&pump_user_id);
        let (prompt_tokens, completion_tokens) = tail.usage();
        match end {
            PumpEnd::Completed => finalize_call_record(
                &pump_state,
                pump_call_id,
                CALL_STATUS_OK,
                prompt_tokens,
                completion_tokens,
                None,
            ),
            PumpEnd::ClientGone => finalize_call_record(
                &pump_state,
                pump_call_id,
                CALL_STATUS_ADMITTED,
                prompt_tokens,
                completion_tokens,
                Some("client disconnected before stream completion".to_string()),
            ),
            PumpEnd::UpstreamError => finalize_call_record(
                &pump_state,
                pump_call_id,
                CALL_STATUS_UPSTREAM_ERROR,
                prompt_tokens,
                completion_tokens,
                Some("upstream stream failed".to_string()),
            ),
        }
    });

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
        .expect("static response parts");
    response.headers_mut().extend(response_headers.drain());
    response
}

fn upstream_unavailable(
    state: &AppState,
    user_id: &str,
    call_id: &str,
    summary: &str,
    message: &str,
) -> Response {
    state.cloud_admission.release(user_id);
    finalize_call_record(
        state,
        call_id.to_string(),
        CALL_STATUS_UPSTREAM_ERROR,
        None,
        None,
        Some(summary.to_string()),
    );
    cloud_error(
        StatusCode::SERVICE_UNAVAILABLE,
        CODE_UPSTREAM_ERROR,
        message,
        None,
    )
    .with_call_id_header(call_id)
}

async fn read_bounded_body(response: reqwest::Response, max_bytes: usize) -> String {
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => {
                if buffer.len() + chunk.len() > max_bytes {
                    let remaining = max_bytes - buffer.len();
                    buffer.extend_from_slice(&chunk[..remaining]);
                    break;
                }
                buffer.extend_from_slice(&chunk);
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buffer).to_string()
}

trait ResponseHeadersExt {
    fn with_call_id_header(self, call_id: &str) -> Response;
    fn with_queue_id_header(self, queue_id: &str) -> Response;
}

impl ResponseHeadersExt for Response {
    fn with_call_id_header(mut self, call_id: &str) -> Response {
        if let Ok(value) = HeaderValue::from_str(call_id) {
            self.headers_mut().insert(REQUEST_ID_HEADER, value);
        }
        self
    }

    fn with_queue_id_header(mut self, queue_id: &str) -> Response {
        if let Ok(value) = HeaderValue::from_str(queue_id) {
            self.headers_mut().insert(QUEUE_HEADER, value);
        }
        self
    }
}

// ---------------------------------------------------------------------------
// POST /wunder/cloud/logs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CloudLogEntry {
    seq: i64,
    level: String,
    category: String,
    event: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    local_session_id: Option<String>,
    created_at: f64,
}

#[derive(Debug, Deserialize)]
struct UploadLogsRequest {
    device_id: String,
    #[serde(default)]
    client: String,
    logs: Vec<CloudLogEntry>,
}

async fn upload_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Json<Value>, Response> {
    require_cloud_enabled(&state).await?;
    let identity = authenticate_cloud(&state, &headers).await?;
    if body.len() > MAX_LOG_BATCH_BYTES {
        return Err(cloud_error(
            StatusCode::BAD_REQUEST,
            CODE_PAYLOAD_TOO_LARGE,
            "log batch exceeds the 256KB payload limit",
            None,
        ));
    }
    let payload: UploadLogsRequest = serde_json::from_slice(&body)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if payload.logs.len() > MAX_LOG_BATCH_ITEMS {
        return Err(cloud_error(
            StatusCode::BAD_REQUEST,
            CODE_PAYLOAD_TOO_LARGE,
            "log batch exceeds 100 entries",
            None,
        ));
    }
    let device_id = payload.device_id.trim().to_string();
    let storage = state.storage.clone();
    let lookup = device_id.clone();
    let record = blocking::run_db("api.cloud.get_device", move || {
        storage.get_cloud_device(&lookup)
    })
    .await
    .map_err(internal_error)?;
    let device = match record {
        Some(record) if !record.revoked && record.user_id == identity.user_id => record,
        _ => return Err(device_revoked_error()),
    };
    let client = if payload.client.trim().is_empty() {
        device.client.clone()
    } else {
        payload.client.trim().to_string()
    };
    let records: Vec<CloudDeviceLogRecord> = payload
        .logs
        .into_iter()
        .map(|entry| CloudDeviceLogRecord {
            seq: entry.seq,
            device_id: device_id.clone(),
            user_id: identity.user_id.clone(),
            client: client.clone(),
            level: entry.level,
            category: entry.category,
            event: entry.event,
            message: entry.message,
            local_session_id: entry.local_session_id,
            created_at: entry.created_at,
        })
        .collect();
    let storage = state.storage.clone();
    let result = blocking::run_db("api.cloud.insert_device_logs", move || {
        storage.insert_cloud_device_logs(&records)
    })
    .await
    .map_err(internal_error)?;
    // Log upload doubles as a heartbeat (§4.1.5).
    let storage = state.storage.clone();
    let heartbeat_device = device_id.clone();
    let now = now_unix_seconds();
    let _ = blocking::run_db("api.cloud.touch_device", move || {
        storage.touch_cloud_device(&heartbeat_device, now)
    })
    .await;
    Ok(Json(json!({
        "data": {
            "accepted": result.accepted,
            "last_seq": result.last_seq.unwrap_or(0),
        }
    })))
}

// ---------------------------------------------------------------------------
// Admin endpoints (bridge)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    device_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

fn page_params(query: &ListQuery) -> (i64, i64) {
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    (offset, limit)
}

fn device_payload(record: &CloudDeviceRecord) -> Value {
    json!({
        "device_id": record.device_id,
        "user_id": record.user_id,
        "client": record.client,
        "name": record.name,
        "os": record.os,
        "arch": record.arch,
        "app_version": record.app_version,
        "last_seen_at": record.last_seen_at,
        "created_at": record.created_at,
        "revoked": record.revoked,
    })
}

async fn admin_devices(
    State(state): State<Arc<AppState>>,
    AxumQuery(query): AxumQuery<ListQuery>,
) -> Result<Json<Value>, Response> {
    let (offset, limit) = page_params(&query);
    let storage = state.storage.clone();
    let user_id = query.user_id.clone();
    let (records, total) = blocking::run_db("api.cloud.list_devices", move || {
        storage.list_cloud_devices(user_id.as_deref(), offset, limit)
    })
    .await
    .map_err(internal_error)?;
    Ok(Json(json!({
        "data": {
            "total": total,
            "items": records.iter().map(device_payload).collect::<Vec<_>>(),
        }
    })))
}

async fn admin_calls(
    State(state): State<Arc<AppState>>,
    AxumQuery(query): AxumQuery<ListQuery>,
) -> Result<Json<Value>, Response> {
    let (offset, limit) = page_params(&query);
    let storage = state.storage.clone();
    let user_id = query.user_id.clone();
    let device_id = query.device_id.clone();
    let model = query.model.clone();
    let status = query.status.clone();
    let (records, total) = blocking::run_db("api.cloud.list_calls", move || {
        let filter = ListCloudRecordsQuery {
            user_id: user_id.as_deref(),
            device_id: device_id.as_deref(),
            model: model.as_deref(),
            status: status.as_deref(),
            offset,
            limit,
        };
        storage.list_cloud_call_records(filter)
    })
    .await
    .map_err(internal_error)?;
    let items = records
        .iter()
        .map(|record| {
            json!({
                "call_id": record.call_id,
                "device_id": record.device_id,
                "user_id": record.user_id,
                "client": record.client,
                "local_session_id": record.local_session_id,
                "model": record.model,
                "provider": record.provider,
                "status": record.status,
                "quota_consumed": record.quota_consumed,
                "queue_waited_ms": record.queue_waited_ms,
                "prompt_tokens": record.prompt_tokens,
                "completion_tokens": record.completion_tokens,
                "started_at": record.started_at,
                "finished_at": record.finished_at,
                "duration_ms": record.duration_ms,
                "error_summary": record.error_summary,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "total": total, "items": items } })))
}

async fn admin_device_logs(
    State(state): State<Arc<AppState>>,
    AxumQuery(query): AxumQuery<ListQuery>,
) -> Result<Json<Value>, Response> {
    let (offset, limit) = page_params(&query);
    let storage = state.storage.clone();
    let user_id = query.user_id.clone();
    let device_id = query.device_id.clone();
    let level = query.level.clone();
    let category = query.category.clone();
    let (records, total) = blocking::run_db("api.cloud.list_device_logs", move || {
        let filter = ListCloudDeviceLogsQuery {
            user_id: user_id.as_deref(),
            device_id: device_id.as_deref(),
            level: level.as_deref(),
            category: category.as_deref(),
            offset,
            limit,
        };
        storage.list_cloud_device_logs(filter)
    })
    .await
    .map_err(internal_error)?;
    let items = records
        .iter()
        .map(|record| {
            json!({
                "seq": record.seq,
                "device_id": record.device_id,
                "user_id": record.user_id,
                "client": record.client,
                "level": record.level,
                "category": record.category,
                "event": record.event,
                "message": record.message,
                "local_session_id": record.local_session_id,
                "created_at": record.created_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "total": total, "items": items } })))
}

/// Revoking keeps the device row (audit trail); its logs expire through the
/// `cloud.log_retention_days` cleanup task instead of a separate delete path.
async fn admin_revoke_device(
    State(state): State<Arc<AppState>>,
    AxumPath(device_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let device_id = device_id.trim().to_string();
    if device_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "device_id is required",
        ));
    }
    let storage = state.storage.clone();
    blocking::run_db("api.cloud.revoke_device", move || {
        storage.set_cloud_device_revoked(&device_id, true)
    })
    .await
    .map_err(internal_error)?;
    Ok(Json(json!({ "data": { "revoked": true } })))
}

// ---------------------------------------------------------------------------
// Unit tests for the admission controller semantics
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn start(decision: AdmissionDecision) -> (String, bool) {
        match decision {
            AdmissionDecision::Start {
                call_id,
                queue_started,
                ..
            } => (call_id, queue_started),
            other => panic!("expected Start, got {other:?}"),
        }
    }

    fn queued(decision: AdmissionDecision) -> (String, u64, bool) {
        match decision {
            AdmissionDecision::Queued {
                queue_id,
                queue_ahead,
                newly_enqueued,
            } => (queue_id, queue_ahead, newly_enqueued),
            other => panic!("expected Queued, got {other:?}"),
        }
    }

    #[test]
    fn direct_start_uses_slots_then_queues() {
        let controller = CloudAdmissionController::new();
        let first = start(controller.admit("user_a", None, 1, 4));
        assert!(!first.1, "direct start is not a queue start");
        let (queue_id, ahead, newly) = queued(controller.admit("user_a", None, 1, 4));
        assert!(newly);
        assert_eq!(ahead, 0, "front of the empty queue");
        let (second_id, ahead, _) = queued(controller.admit("user_a", None, 1, 4));
        assert_eq!(ahead, 1);
        assert_ne!(queue_id, second_id);

        controller.release("user_a");
        let (call_id, queue_started) =
            start(controller.admit("user_a", Some(queue_id.clone()), 1, 4));
        assert!(queue_started, "front retry with a free slot starts");
        assert_eq!(
            call_id, queue_id,
            "queued call keeps the queue id as call id"
        );
    }

    #[test]
    fn full_queue_is_rejected_without_queue_id() {
        let controller = CloudAdmissionController::new();
        let _active = start(controller.admit("user_b", None, 1, 1));
        let _waiting = queued(controller.admit("user_b", None, 1, 1));
        assert!(matches!(
            controller.admit("user_b", None, 1, 1),
            AdmissionDecision::Busy
        ));
    }

    #[test]
    fn non_front_retry_reports_position() {
        let controller = CloudAdmissionController::new();
        let _active = start(controller.admit("user_c", None, 1, 4));
        let (front, _, _) = queued(controller.admit("user_c", None, 1, 4));
        let _rear = queued(controller.admit("user_c", None, 1, 4));
        let (_, ahead, newly) = queued(controller.admit("user_c", Some(front), 1, 4));
        assert!(!newly, "retry with a known id never re-enqueues");
        assert_eq!(ahead, 0, "front retry reports position 0 while still busy");
    }

    #[test]
    fn snapshot_reflects_active_and_waiting() {
        let controller = CloudAdmissionController::new();
        let _active = start(controller.admit("user_d", None, 1, 4));
        let _waiting = queued(controller.admit("user_d", None, 1, 4));
        assert_eq!(controller.snapshot("user_d"), (1, 1));
        assert_eq!(controller.snapshot("unknown_user"), (0, 0));
    }
}
