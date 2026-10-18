//! `CloudService`: local cloud session lifecycle (login/logout/status), cloud
//! model synthesis into the local config, account snapshot refresh and the
//! device-log reporter loop. One instance is shared per process (desktop and
//! cli forms both run exactly one engine), exposed through `AppState.cloud`
//! and read by the LlmClient cloud channel wrapper. Everything is lazy: when
//! nobody logs in, the service holds no session, touches no file and spawns
//! no task.

use super::reporter::{
    CloudLogEntry, LogReporter, LogUploadResponse, FLUSH_TRIGGER_ITEMS, HEARTBEAT_INTERVAL_SECS,
    HEARTBEAT_RETRY_SECS, INITIAL_BACKOFF_SECS, MAX_BACKOFF_SECS, PERIODIC_FLUSH_SECS,
};
use super::session::{wunder_home_dir, CloudAccountSnapshot, CloudSessionFile};
use super::CloudSessionExpired;
use crate::config::LlmModelConfig;
use crate::config_store::ConfigStore;
use crate::core::process_file_lock::ProcessFileLock;
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};
use tokio::sync::Notify;

/// Provider name for synthesized cloud model entries (alias `cloud`).
pub const CLOUD_PROVIDER: &str = "wunder_cloud";
/// Local config key prefix for synthesized cloud models.
pub const CLOUD_MODEL_PREFIX: &str = "cloud/";
/// Local cap for the total queue wait per model call (server `queue.max_wait_s`).
pub const CLOUD_QUEUE_MAX_WAIT_SECS: u64 = 300;
/// Upper bound of the random jitter added to the queue retry delay.
pub const CLOUD_QUEUE_JITTER_MS: u64 = 500;
/// Connection-state values surfaced on `CloudStatus.connection`.
pub const CONNECTION_ONLINE: &str = "online";
pub const CONNECTION_RECONNECTING: &str = "reconnecting";
pub const CONNECTION_EXPIRED: &str = "expired";
pub const CONNECTION_LOGGED_OUT: &str = "logged_out";
/// Proactive renewal: refresh when the access token has less than 72h left.
pub const TOKEN_RENEW_AHEAD_SECS: f64 = 72.0 * 3600.0;
/// How often the background keeper re-checks the token expiry.
pub const TOKEN_RENEW_CHECK_INTERVAL_SECS: u64 = 300;
const CONNECT_TIMEOUT_SECS: u64 = 5;
const REQUEST_TIMEOUT_SECS: u64 = 20;
const SESSION_LOCK_WAIT_SECS: u64 = 8;

static SHARED: OnceLock<Arc<CloudService>> = OnceLock::new();

/// Process-wide cloud service. Desktop and cli each run one engine per
/// process, so a single shared instance carries the login state.
pub fn shared() -> &'static Arc<CloudService> {
    SHARED.get_or_init(|| Arc::new(CloudService::new()))
}

struct CloudServiceState {
    loaded: bool,
    loaded_base: Option<std::path::PathBuf>,
    session_fingerprint: Option<SessionFingerprint>,
    session: Option<CloudSessionFile>,
    expired: bool,
    reporter_task: Option<tokio::task::JoinHandle<()>>,
    keeper_task: Option<tokio::task::JoinHandle<()>>,
    /// `online | reconnecting` while a session exists; `expired` and
    /// `logged_out` are derived from `expired`/`session` so the invariant
    /// cannot drift.
    connection: &'static str,
    /// Sanitized failure summary; never carries tokens or URL details.
    last_error: Option<String>,
    last_success_at: Option<f64>,
    /// When `reconnecting`: the scheduled reporter retry moment.
    next_retry_at: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionFingerprint {
    len: u64,
    modified_secs: u64,
    modified_nanos: u32,
}

fn session_fingerprint(base: &Path, client: &str) -> Option<SessionFingerprint> {
    let path = CloudSessionFile::path_for(base, client);
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(SessionFingerprint {
        len: metadata.len(),
        modified_secs: duration.as_secs(),
        modified_nanos: duration.subsec_nanos(),
    })
}

/// UI-facing login/account status snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct CloudStatus {
    pub inbound_enabled: bool,
    pub full_shadow: bool,
    pub logged_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota: Option<super::session::CloudQuotaSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent_calls: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency_active: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency_queued: Option<u64>,
    pub expired: bool,
    pub preferences_sync_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// `online | reconnecting | expired | logged_out`.
    pub connection: &'static str,
    /// Sanitized last failure summary (no tokens, no URL details).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<f64>,
    /// Scheduled retry moment while `reconnecting`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_retry_at: Option<f64>,
}

pub struct CloudService {
    http: reqwest::Client,
    state: Mutex<CloudServiceState>,
    reporter: Mutex<Option<Arc<LogReporter>>>,
    session_base_override: Mutex<Option<std::path::PathBuf>>,
    local_client: Mutex<String>,
    /// Single-flight guard for silent token refresh: concurrent 401s queue
    /// here and only one refresh request is issued.
    refresh_lock: tokio::sync::Mutex<()>,
    /// Set while a reporter loop is uploading, so `flush_logs` can await it.
    flush_wake: Notify,
    flush_in_flight: Arc<AtomicBool>,
}

impl CloudService {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .build()
            .unwrap_or_default();
        Self {
            http,
            state: Mutex::new(CloudServiceState {
                loaded: false,
                loaded_base: None,
                session_fingerprint: None,
                session: None,
                expired: false,
                reporter_task: None,
                keeper_task: None,
                connection: CONNECTION_LOGGED_OUT,
                last_error: None,
                last_success_at: None,
                next_retry_at: None,
            }),
            reporter: Mutex::new(None),
            session_base_override: Mutex::new(None),
            local_client: Mutex::new("shared".to_string()),
            refresh_lock: tokio::sync::Mutex::new(()),
            flush_wake: Notify::new(),
            flush_in_flight: Arc::new(AtomicBool::new(false)),
        }
    }

    // ------------------------------------------------------------------
    // Session plumbing
    // ------------------------------------------------------------------

    fn lock_state(&self) -> std::sync::MutexGuard<'_, CloudServiceState> {
        self.state.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// Redirect the session file location. Used by the desktop/cli facades to
    /// pin the session next to their own settings when the process default
    /// differs, and by tests to isolate the session file.
    pub fn set_session_base_dir(&self, dir: std::path::PathBuf) {
        *self
            .session_base_override
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(dir);
    }

    pub fn set_client_kind(&self, client: &str) {
        *self
            .local_client
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = normalize_client(client);
    }

    pub fn client_kind(&self) -> String {
        self.local_client
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn session_base_dir(&self) -> std::path::PathBuf {
        self.session_base_override
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
            .unwrap_or_else(wunder_home_dir)
    }

    fn ensure_loaded(&self) {
        let base = self.session_base_dir();
        let fingerprint = session_fingerprint(&base, &self.client_kind());
        {
            let state = self.lock_state();
            if state.loaded
                && state.loaded_base.as_deref() == Some(base.as_path())
                && state.session_fingerprint == fingerprint
            {
                return;
            }
        };
        let mut state = self.lock_state();
        // Another caller may have refreshed the in-memory state while this
        // thread was computing metadata.
        if state.loaded
            && state.loaded_base.as_deref() == Some(base.as_path())
            && state.session_fingerprint == fingerprint
        {
            return;
        }
        state.loaded = true;
        state.loaded_base = Some(base.clone());
        state.session_fingerprint = fingerprint;
        let next_session = CloudSessionFile::load_for(&base, &self.client_kind());
        let changed = match (&state.session, &next_session) {
            (Some(old), Some(new)) => {
                old.token != new.token || old.refresh_token != new.refresh_token
            }
            (None, None) => false,
            _ => true,
        };
        if let Some(session) = next_session {
            tracing::info!("[cloud] local client session restored");
            state.session = Some(session);
            state.connection = CONNECTION_ONLINE;
        } else {
            state.session = None;
            state.expired = false;
            state.connection = CONNECTION_LOGGED_OUT;
        }
        let has_session = state.session.is_some();
        if changed {
            if let Some(task) = state.reporter_task.take() {
                task.abort();
            }
            if let Some(task) = state.keeper_task.take() {
                task.abort();
            }
        }
        drop(state);
        // Never acquire `reporter` while holding `state`: install_session and
        // clear_session_state update the two mutexes in the opposite phases.
        if changed {
            *self.reporter.lock().unwrap_or_else(|err| err.into_inner()) = None;
        }
        // Background loops are bound to the process singleton; spawn them on
        // the first async touch of a restored session.
        if has_session {
            self.ensure_keeper_task();
        }
    }

    async fn acquire_session_file_lock(&self) -> Result<ProcessFileLock> {
        ProcessFileLock::acquire(
            &self.session_base_dir().join("config/cloud.session.lock"),
            Duration::from_secs(SESSION_LOCK_WAIT_SECS),
        )
        .await
    }

    fn persist_session_fingerprint(&self) {
        let base = self.session_base_dir();
        self.lock_state().session_fingerprint = session_fingerprint(&base, &self.client_kind());
    }

    /// Current session snapshot (lazy-loads the session file once).
    pub fn session(&self) -> Option<CloudSessionFile> {
        self.ensure_loaded();
        self.lock_state().session.clone()
    }

    pub fn device_id(&self) -> Option<String> {
        self.session().map(|session| session.device_id)
    }

    fn reporter(&self) -> Option<Arc<LogReporter>> {
        self.reporter
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// Report one explicit engine event into the bounded upload buffer.
    /// No-op when not logged in; never blocks the caller.
    pub fn report(
        &self,
        level: &str,
        category: &str,
        event: &str,
        message: Option<&str>,
        local_session_id: Option<&str>,
    ) {
        let Some(session) = self.session() else {
            return;
        };
        if !session.log_report.enabled {
            return;
        }
        let Some(reporter) = self.reporter() else {
            return;
        };
        reporter.push(level, category, event, message, local_session_id);
        self.ensure_reporter_task();
    }

    /// Spawn the reporter loop for the current session. Called after login
    /// and on the first touch of a session restored from disk. The loop binds
    /// to the process singleton (`shared()`), which is the only deployment
    /// shape with a live login session.
    fn ensure_reporter_task(&self) {
        {
            let state = self.lock_state();
            if state.reporter_task.is_some() {
                return;
            }
            let Some(session) = state.session.as_ref() else {
                return;
            };
            if !session.log_report.enabled {
                return;
            }
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let reporter = match self.reporter() {
            Some(reporter) => reporter,
            None => {
                let seq = self
                    .session()
                    .map(|session| session.log_report.last_synced_seq)
                    .unwrap_or(0);
                let reporter = Arc::new(LogReporter::new(seq));
                *self.reporter.lock().unwrap_or_else(|err| err.into_inner()) =
                    Some(reporter.clone());
                reporter
            }
        };
        let task = handle.spawn(reporter_loop(reporter, self.flush_in_flight.clone()));
        self.lock_state().reporter_task = Some(task);
    }

    /// Spawn the proactive token-renewal keeper for the current session. The
    /// loop exits by itself once the session is gone (logout / cleared file).
    fn ensure_keeper_task(&self) {
        {
            let state = self.lock_state();
            if state.keeper_task.is_some() || state.session.is_none() {
                return;
            }
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let task = handle.spawn(session_keeper_loop());
        self.lock_state().keeper_task = Some(task);
    }

    // ------------------------------------------------------------------
    // Connection state machine
    // ------------------------------------------------------------------

    /// Record a successful cloud exchange: `online`, no error, fresh
    /// success timestamp. Idempotent; a few field writes under the lock.
    pub fn mark_online(&self) {
        let mut state = self.lock_state();
        if state.session.is_none() {
            return;
        }
        state.connection = CONNECTION_ONLINE;
        state.last_error = None;
        state.next_retry_at = None;
        state.last_success_at = Some(super::reporter::now_unix_seconds());
    }

    /// Record a network-level failure: `reconnecting` with a sanitized error
    /// summary. The session stays valid and retryable.
    pub fn mark_reconnecting(&self, summary: &str) {
        let mut state = self.lock_state();
        if state.session.is_none() {
            return;
        }
        state.connection = CONNECTION_RECONNECTING;
        state.last_error = Some(summary.to_string());
    }

    /// Publish the reporter's next retry moment while reconnecting.
    fn set_next_retry_at(&self, at: Option<f64>) {
        self.lock_state().next_retry_at = at;
    }

    /// Mark the session expired after an auth-level rejection. The session
    /// stays on disk (the user may still be logged in on the server); the UI
    /// surfaces `expired` and asks for a re-login.
    pub fn mark_expired(&self) {
        let mut state = self.lock_state();
        if state.session.is_some() {
            state.expired = true;
            state.connection = CONNECTION_EXPIRED;
            state.next_retry_at = None;
        }
    }

    // ------------------------------------------------------------------
    // Silent token recovery (single-flight)
    // ------------------------------------------------------------------

    /// Refresh the access token with the stored rotating refresh token.
    ///
    /// Returns:
    /// - `Ok(true)`  — the session now holds a fresh access token; the caller
    ///   may safely retry the original request once with the new token.
    /// - `Ok(false)` — auth-level, unrecoverable: refresh rejected
    ///   (INVALID/EXPIRED → marked expired) or reuse detected (forced local
    ///   logout). The caller must surface `CloudSessionExpired`.
    /// - `Err(_)`    — network-level failure: the current token is kept, the
    ///   state moved to `reconnecting`; the call is retryable later.
    ///
    /// Single-flight: concurrent 401 handlers queue on the refresh lock; the
    /// first one issues the refresh request, the rest observe the rotated
    /// token and return `Ok(true)` without another request.
    pub async fn recover_auth(&self, stale_access_token: &str) -> Result<bool> {
        let _guard = self.refresh_lock.lock().await;
        // Rotating refresh tokens are single-use. Serialize the whole exchange
        // across processes, including publication, to prevent family revocation.
        let _file_lock = self.acquire_session_file_lock().await?;
        let (server, refresh_token) = {
            // A different process may have completed the rotation while this
            // process was waiting for the local single-flight lock.
            self.ensure_loaded();
            let state = self.lock_state();
            match state.session.as_ref() {
                // Another waiter already rotated the token for us.
                Some(session) if session.token != stale_access_token => return Ok(true),
                Some(session) => (session.server.clone(), session.refresh_token.clone()),
                None => return Ok(false),
            }
        };
        // Legacy session without a rotating refresh token: only re-login helps.
        let Some(refresh_token) = refresh_token else {
            self.mark_expired();
            return Ok(false);
        };

        let sent = self
            .http
            .post(format!("{server}/wunder/auth/refresh"))
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .json(&json!({ "refresh_token": refresh_token }))
            .send()
            .await;
        let response = match sent {
            Ok(response) => response,
            Err(err) => {
                // Keep the current token; the next exchange can retry.
                let summary = network_error_summary(&err);
                self.mark_reconnecting(&summary);
                return Err(super::CloudTransportError(summary).into());
            }
        };
        let status = response.status();
        if status.as_u16() == 401 {
            let code = response
                .headers()
                .get("x-error-code")
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_ascii_uppercase())
                .unwrap_or_default();
            let _ = read_json(response).await;
            if code == "REFRESH_TOKEN_REUSED" {
                // Replay of a rotated token revokes the whole family
                // server-side; wipe the local session so the stale token can
                // never be replayed again. Log carries no token value.
                tracing::warn!("[cloud] refresh token reuse detected, forced local logout");
                self.clear_session_state();
                return Ok(false);
            }
            tracing::info!("[cloud] token refresh rejected ({code}), session marked expired");
            self.mark_expired();
            return Ok(false);
        }
        if !status.is_success() {
            let summary = format!("token refresh rejected: {}", status.as_u16());
            self.mark_reconnecting(&summary);
            return Err(super::CloudInterlinkError {
                status: status.as_u16(),
                operation: "token refresh",
                message: summary,
            }
            .into());
        }
        let body = read_json(response).await;
        let data = &body["data"];
        let new_access = data["access_token"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("cloud refresh response missing access_token"))?
            .to_string();
        // Rotation semantics: the new refresh token replaces the old one. The
        // old value must be dropped immediately or the next refresh replays a
        // revoked token and the server burns the whole family.
        let new_refresh = data["refresh_token"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let expires_at = data["expires_at"].as_f64();
        let username = data["user"]["username"].as_str().map(str::to_string);

        self.ensure_loaded();
        let mut next = match self.session() {
            Some(session) if session.token == stale_access_token => session,
            session => return Ok(session.is_some()),
        };
        next.token = new_access;
        if let Some(token) = new_refresh {
            next.refresh_token = Some(token);
        }
        if expires_at.is_some() {
            next.token_expires_at = expires_at;
        }
        if let Some(username) = username {
            next.username = username;
        }
        next.save(&self.session_base_dir())?;
        {
            let mut state = self.lock_state();
            state.session = Some(next);
            state.session_fingerprint =
                session_fingerprint(&self.session_base_dir(), &self.client_kind());
            state.expired = false;
            state.connection = CONNECTION_ONLINE;
            state.last_error = None;
            state.next_retry_at = None;
            state.last_success_at = Some(super::reporter::now_unix_seconds());
        }
        Ok(true)
    }

    /// Send a cloud request with the live session token; on 401 run the
    /// single-flight silent refresh and retry the request exactly once with
    /// the fresh token. Success and network outcomes feed the connection
    /// state machine. `build` must be pure (called once per attempt).
    pub async fn authed_send(
        &self,
        build: impl Fn(&CloudSessionFile) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        if self.session().is_none() {
            self.adopt_shared_login().await?;
        }
        let Some(session) = self.session() else {
            return Err(anyhow!(CloudSessionExpired));
        };
        let identity = (
            session.server.clone(),
            session.user_id.clone(),
            session.device_id.clone(),
            session.logged_in_at,
        );
        let stale_token = session.token.clone();
        let attempt = build(&session)
            .header("x-wunder-device-id", &session.device_id)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await;
        match attempt {
            Ok(response) if response.status().as_u16() != 401 => {
                self.settle_connection(&response);
                return Ok(response);
            }
            Ok(_) => {}
            Err(err) => {
                let summary = network_error_summary(&err);
                self.mark_reconnecting(&summary);
                return Err(super::CloudTransportError(summary).into());
            }
        }
        // 401: try the silent refresh, then exactly one retry.
        match self.recover_auth(&stale_token).await {
            Ok(true) => {}
            Ok(false) => return Err(anyhow!(CloudSessionExpired)),
            Err(err) => return Err(err),
        }
        let Some(session) = self.session() else {
            return Err(anyhow!(CloudSessionExpired));
        };
        if (
            session.server.clone(),
            session.user_id.clone(),
            session.device_id.clone(),
            session.logged_in_at,
        ) != identity
        {
            return Err(anyhow!(
                "cloud session changed; retry the operation explicitly"
            ));
        }
        let attempt = build(&session)
            .header("x-wunder-device-id", &session.device_id)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await;
        match attempt {
            Ok(response) if response.status().as_u16() == 401 => {
                self.mark_expired();
                Err(anyhow!(CloudSessionExpired))
            }
            Ok(response) => {
                self.settle_connection(&response);
                Ok(response)
            }
            Err(err) => {
                let summary = network_error_summary(&err);
                self.mark_reconnecting(&summary);
                Err(super::CloudTransportError(summary).into())
            }
        }
    }

    /// Feed a completed HTTP exchange into the connection state machine.
    fn settle_connection(&self, response: &reqwest::Response) {
        if response.status().is_success() {
            self.mark_online();
        }
    }

    /// Heartbeat: empty log batch. The server touches the device after auth
    /// and device validation regardless of the batch content, so an empty
    /// POST keeps `last_seen_at` fresh while idle. Drives the connection
    /// state machine through `authed_send`.
    pub async fn send_heartbeat(&self) -> Result<()> {
        let response = self
            .authed_send(|session| {
                self.http
                    .post(format!("{}/wunder/cloud/logs", session.server))
                    .bearer_auth(&session.token)
                    .header("x-wunder-device-id", &session.device_id)
                    .json(&json!({
                        "device_id": session.device_id,
                        "client": session.client,
                        "logs": [],
                    }))
            })
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = read_json(response).await;
            return Err(anyhow!(
                "cloud heartbeat failed: {status} {}",
                error_message(&body)
            ));
        }
        Ok(())
    }

    /// Proactive renewal: refresh the access token when less than
    /// `TOKEN_RENEW_AHEAD_SECS` remain. Called by the background keeper every
    /// `TOKEN_RENEW_CHECK_INTERVAL_SECS`; network failures surface only in
    /// the connection state machine.
    pub async fn renew_if_needed(&self) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        let Some(expires_at) = session.token_expires_at else {
            return false;
        };
        let remaining = expires_at - super::reporter::now_unix_seconds();
        if remaining >= TOKEN_RENEW_AHEAD_SECS {
            return false;
        }
        tracing::info!(
            "[cloud] access token expires in {:.0}s, renewing proactively",
            remaining.max(0.0)
        );
        matches!(self.recover_auth(&session.token).await, Ok(true))
    }

    /// Startup self-healing probe. Returns immediately; the actual work runs
    /// in the background: refresh the account (401 auto-recovery through
    /// `authed_send`), re-synthesize the cloud models and fire one heartbeat
    /// so the device shows up online right away. Every failure is recorded in
    /// the connection state machine only.
    pub async fn startup_probe(&self, config_store: &ConfigStore) {
        // Sessions written before the connected-workspace settings existed had
        // `enabled=false/full_shadow=false` by deserialization default.  Treat
        // that legacy state as an unconfigured preference once, so upgrading a
        // desktop immediately publishes its real workspace catalog.  A later
        // explicit toggle is marked configured and is preserved.
        if let Ok(_file_lock) = self.acquire_session_file_lock().await {
            let base = self.session_base_dir();
            if let Some(mut session) = CloudSessionFile::load_for(&base, &self.client_kind()) {
                if session.interlink.migrate_workspace_sharing() && session.save(&base).is_ok() {
                    self.install_session(session);
                }
            }
        }
        if self.session().is_none() {
            let config = config_store.clone();
            tokio::spawn(async move {
                let cloud = super::shared();
                if cloud.adopt_shared_login().await.is_ok() {
                    let _ = cloud.synthesize_cloud_models(&config).await;
                }
            });
            return;
        }
        self.ensure_reporter_task();
        self.ensure_keeper_task();
        let config_store = config_store.clone();
        tokio::spawn(async move {
            let cloud = super::shared();
            let _ = cloud.refresh_account().await;
            let _ = cloud.synthesize_cloud_models(&config_store).await;
            let _ = cloud.send_heartbeat().await;
        });
    }

    /// Incremental quota refresh from call response headers. Memory only;
    /// the file is rewritten on refresh/logout to keep the hot path cheap.
    pub fn update_quota_from_headers(&self, balance: Option<i64>, used: Option<i64>) {
        let mut state = self.lock_state();
        let Some(session) = state.session.as_mut() else {
            return;
        };
        let Some(account) = session.account.as_mut() else {
            return;
        };
        if let Some(balance) = balance {
            account.quota.balance = balance;
        }
        if let Some(used) = used {
            account.quota.used_total = used;
        }
    }

    // ------------------------------------------------------------------
    // Login / logout / status
    // ------------------------------------------------------------------

    /// Login flow (§4.1): login with local scope -> register device -> pull
    /// account -> pull preferences -> persist session -> synthesize cloud
    /// models -> start the log reporter.
    pub async fn login(
        &self,
        config_store: &ConfigStore,
        server: &str,
        username: &str,
        password: &str,
        client: &str,
    ) -> Result<CloudStatus> {
        let client_kind = normalize_client(client);
        let _file_lock = self.acquire_session_file_lock().await?;
        let scope = if client_kind == "cli" {
            "local_cli"
        } else {
            "local_desktop"
        };
        let server = normalize_server(server)?;

        // 1. Login (local scope header keeps web sessions untouched).
        let login_url = format!("{server}/wunder/auth/login");
        let response = self
            .http
            .post(&login_url)
            .header("x-wunder-session-scope", scope)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .json(&json!({ "username": username, "password": password }))
            .send()
            .await
            .map_err(|err| anyhow!("cloud server unreachable: {err}"))?;
        let status = response.status();
        let body = read_json(response).await;
        if !status.is_success() {
            return Err(anyhow!(
                "cloud login failed: {status} {}",
                error_message(&body)
            ));
        }
        let token = body["data"]["access_token"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("cloud login response missing access_token"))?
            .to_string();
        // Local-scope logins rotate a refresh token family (§auth refresh);
        // both values plus the access-token expiry are persisted so the
        // engine can renew the session silently.
        let refresh_token = body["data"]["refresh_token"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let token_expires_at = body["data"]["expires_at"].as_f64();
        let user = &body["data"]["user"];
        let user_id = user["user_id"]
            .as_str()
            .or_else(|| user["id"].as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("cloud login response missing user_id"))?;
        let display_name = user["username"].as_str().unwrap_or(username).to_string();

        // 2. Register (or reuse) this device.
        let device_name = local_device_name(&client_kind);
        let mut device_body = json!({
            "device_id": self.installation_device_id(&client_kind, &server, &user_id, false)?,
            "client": client_kind,
            "name": device_name,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "app_version": env!("CARGO_PKG_VERSION"),
        });
        let response = self
            .http
            .post(format!("{server}/wunder/cloud/devices"))
            .bearer_auth(&token)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .json(&device_body)
            .send()
            .await
            .map_err(|err| anyhow!("cloud device registration failed: {err}"))?;
        let mut status = response.status();
        let mut body = read_json(response).await;
        if status.as_u16() == 401 && body["error"]["code"] == "DEVICE_REVOKED" {
            // A fresh password login may register a new identity with default
            // grants. Silent adoption never replaces a revoked device.
            device_body["device_id"] =
                json!(self.installation_device_id(&client_kind, &server, &user_id, true)?);
            let response = self
                .http
                .post(format!("{server}/wunder/cloud/devices"))
                .bearer_auth(&token)
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .json(&device_body)
                .send()
                .await?;
            status = response.status();
            body = read_json(response).await;
        }
        if !status.is_success() {
            return Err(anyhow!(
                "cloud device registration failed: {status} {}",
                error_message(&body)
            ));
        }
        let device_id = body["data"]["device_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("cloud device registration response missing device_id"))?;
        let max_concurrent_calls = body["data"]["max_concurrent_calls"].as_u64().unwrap_or(2);

        // 3. Account overview.
        let mut session = CloudSessionFile {
            server: server.clone(),
            user_id: user_id.clone(),
            username: display_name,
            scope: scope.to_string(),
            token,
            refresh_token,
            token_expires_at,
            device_id: device_id.clone(),
            device_name,
            client: client_kind.to_string(),
            logged_in_at: super::reporter::now_unix_seconds(),
            account: None,
            log_report: Default::default(),
            preferences_sync_enabled: true,
            interlink: super::session::InterlinkSessionConfig::default(),
        };
        // A local form signs in to participate in the connected workspace by
        // default. The published shadow is metadata only (workspace names,
        // relative directory entries and thread summaries); file bodies remain
        // on demand behind command policy and approval. Users or organisation
        // policy can still disable this from settings.
        session.interlink.enabled = true;
        session.interlink.full_shadow = true;
        session.interlink.sharing_configured = true;
        session.account = Some(CloudAccountSnapshot {
            max_concurrent_calls,
            ..self.fetch_account(&session).await?
        });

        // 4. Preferences pull (tolerated failure; applying them is the
        //    desktop/cli facade's job).
        let preferences_synced_at = if self.fetch_preferences(&session).await {
            Some(super::reporter::now_unix_seconds())
        } else {
            None
        };
        if let Some(account) = session.account.as_mut() {
            account.preferences_synced_at = preferences_synced_at;
        }

        // 5. Persist and install; the reporter starts from the persisted cursor.
        let base_dir = self.session_base_dir();
        session.save(&base_dir)?;
        session.publish_shared(&base_dir)?;
        let _ = std::fs::remove_file(self.adoption_block_path());
        self.install_session(session);

        // 6. Synthesize cloud models into the local config.
        self.synthesize_cloud_models(config_store).await?;

        Ok(self.status())
    }

    fn installation_device_id(
        &self,
        client: &str,
        server: &str,
        user: &str,
        replace: bool,
    ) -> Result<String> {
        use sha2::{Digest, Sha256};
        let namespace = format!("{server}\n{user}");
        let digest = format!("{:x}", Sha256::digest(namespace.as_bytes()));
        let path = self
            .session_base_dir()
            .join(format!("config/cloud.{client}.{}.device-id", &digest[..24]));
        if let Some(value) = std::fs::read_to_string(&path).ok().filter(|_| !replace) {
            if uuid::Uuid::parse_str(value.trim()).is_ok() {
                return Ok(value.trim().to_string());
            }
        }
        let value = uuid::Uuid::new_v4().to_string();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::core::atomic_write::atomic_write_text(&path, &value)?;
        Ok(value)
    }

    /// Exchange the shared account credential for a separate local-form
    /// family. No password is retained and the source family stays untouched.
    pub async fn adopt_shared_login(&self) -> Result<()> {
        let client = self.client_kind();
        if client == "shared" || self.session().is_some() {
            return Ok(());
        }
        let _lock = self.acquire_session_file_lock().await?;
        if self.session().is_some() {
            return Ok(());
        }
        if self.adoption_block_path().exists() {
            return Err(anyhow!("logged out; explicit login required"));
        }
        let source = CloudSessionFile::load_any(&self.session_base_dir())
            .ok_or_else(|| anyhow!("not logged in"))?;
        let response = self
            .http
            .post(format!("{}/wunder/auth/local-session", source.server))
            .bearer_auth(&source.token)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .json(&json!({ "client": client }))
            .send()
            .await?;
        let status = response.status();
        let body = read_json(response).await;
        if !status.is_success() {
            return Err(anyhow!("shared login exchange rejected ({status})"));
        }
        let data = &body["data"];
        let token = data["access_token"]
            .as_str()
            .ok_or_else(|| anyhow!("missing access token"))?
            .to_string();
        let device_id =
            self.installation_device_id(&client, &source.server, &source.user_id, false)?;
        let response = self.http.post(format!("{}/wunder/cloud/devices", source.server)).bearer_auth(&token)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS)).json(&json!({
                "device_id": device_id, "client": client, "name": local_device_name(&client),
                "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "app_version": env!("CARGO_PKG_VERSION")
            })).send().await?;
        let status = response.status();
        let registered = read_json(response).await;
        if !status.is_success() {
            return Err(anyhow!("device registration rejected ({status})"));
        }
        let mut session = source;
        session.token = token;
        session.refresh_token = data["refresh_token"].as_str().map(str::to_string);
        session.token_expires_at = data["expires_at"].as_f64();
        session.scope = format!("local_{client}");
        session.client = client;
        session.device_id = registered["data"]["device_id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing device identity"))?
            .to_string();
        session.interlink.node_secret = None;
        session.interlink.secret_version = 0;
        // A newly adopted local form must present the same connected workspace
        // contract as a password login. It gets its own device credential, and
        // inherits no file-content publishing capability.
        session.interlink.enabled = true;
        session.interlink.full_shadow = true;
        session.interlink.sharing_configured = true;
        session.logged_in_at = super::reporter::now_unix_seconds();
        session.save(&self.session_base_dir())?;
        self.install_session(session);
        Ok(())
    }

    /// Install a fully built session (login result or restored snapshot):
    /// resets the connection state to `online`, swaps the reporter buffer and
    /// restarts the background loops. Public so facades and tests can pin a
    /// session without rerunning the login HTTP flow.
    pub fn install_session(&self, session: CloudSessionFile) {
        let reporter = Arc::new(LogReporter::new(session.log_report.last_synced_seq));
        {
            let mut state = self.lock_state();
            state.loaded = true;
            state.loaded_base = Some(self.session_base_dir());
            state.session_fingerprint =
                session_fingerprint(&self.session_base_dir(), &self.client_kind());
            state.expired = false;
            state.connection = CONNECTION_ONLINE;
            state.last_error = None;
            state.last_success_at = Some(super::reporter::now_unix_seconds());
            state.next_retry_at = None;
            state.session = Some(session);
            if let Some(old) = state.reporter_task.take() {
                old.abort();
            }
            if let Some(old) = state.keeper_task.take() {
                old.abort();
            }
        }
        *self.reporter.lock().unwrap_or_else(|err| err.into_inner()) = Some(reporter);
        self.ensure_reporter_task();
        self.ensure_keeper_task();
    }

    /// Logout (§4.1.3): best-effort token invalidation, drop the session file
    /// and every synthesized cloud model entry, stop the reporter. Local
    /// threads and settings stay untouched. The rotating refresh-token fields
    /// die with the file, so nothing stale can be replayed afterwards.
    pub async fn logout(&self, config_store: &ConfigStore) -> Result<()> {
        let session = {
            let _file_lock = self.acquire_session_file_lock().await?;
            self.ensure_loaded();
            let session = self.session();
            self.clear_session_state();
            remove_cloud_models(config_store).await?;
            session
        };
        if let Some(session) = session {
            let _ = self
                .http
                .post(format!("{}/wunder/auth/logout", session.server))
                .bearer_auth(&session.token)
                .timeout(Duration::from_secs(5))
                .send()
                .await;
        }
        Ok(())
    }

    /// Tear down every local trace of the session (memory, file, reporter,
    /// keeper) and surface `logged_out`. Shared by explicit logout and the
    /// forced logout after refresh-token reuse detection. Removing the file
    /// here matters: `loaded_base` is cleared, so a surviving file would be
    /// reloaded on the next touch and resurrect the session.
    fn clear_session_state(&self) {
        // Explicit logout/revocation must survive restart and never auto-adopt
        // a still logged-in companion form's credential.
        let marker = self.adoption_block_path();
        if let Some(parent) = marker.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = crate::core::atomic_write::atomic_write_text(&marker, "logged_out");
        let client = {
            let mut state = self.lock_state();
            let client = state.session.take().map(|session| session.client);
            state.loaded = true;
            state.expired = false;
            state.loaded_base = None;
            state.connection = CONNECTION_LOGGED_OUT;
            state.last_error = None;
            state.last_success_at = None;
            state.next_retry_at = None;
            if let Some(task) = state.reporter_task.take() {
                task.abort();
            }
            if let Some(task) = state.keeper_task.take() {
                task.abort();
            }
            client
        };
        if let Some(client) = client.as_deref() {
            CloudSessionFile::remove(&self.session_base_dir(), client);
        }
        self.lock_state().session_fingerprint =
            session_fingerprint(&self.session_base_dir(), &self.client_kind());
        *self.reporter.lock().unwrap_or_else(|err| err.into_inner()) = None;
    }

    fn adoption_block_path(&self) -> std::path::PathBuf {
        self.session_base_dir()
            .join(format!("config/cloud.{}.logged-out", self.client_kind()))
    }

    /// Status snapshot for UI binding.
    pub fn status(&self) -> CloudStatus {
        self.ensure_loaded();
        let state = self.lock_state();
        let Some(session) = state.session.as_ref() else {
            return CloudStatus {
                inbound_enabled: false,
                full_shadow: false,
                logged_in: false,
                server: None,
                user_id: None,
                username: None,
                quota: None,
                max_concurrent_calls: None,
                concurrency_active: None,
                concurrency_queued: None,
                expired: false,
                preferences_sync_enabled: true,
                device_id: None,
                client: None,
                connection: CONNECTION_LOGGED_OUT,
                last_error: None,
                last_success_at: None,
                next_retry_at: None,
            };
        };
        let account = session.account.as_ref();
        let connection = if state.expired {
            CONNECTION_EXPIRED
        } else {
            state.connection
        };
        CloudStatus {
            inbound_enabled: session.interlink.enabled,
            full_shadow: session.interlink.full_shadow,
            logged_in: true,
            server: Some(session.server.clone()),
            user_id: Some(session.user_id.clone()),
            username: Some(session.username.clone()),
            quota: account.map(|account| account.quota.clone()),
            max_concurrent_calls: account.map(|account| account.max_concurrent_calls),
            concurrency_active: account.and_then(|account| account.concurrency_active),
            concurrency_queued: account.and_then(|account| account.concurrency_queued),
            expired: state.expired,
            preferences_sync_enabled: session.preferences_sync_enabled,
            device_id: Some(session.device_id.clone()),
            client: Some(session.client.clone()),
            connection,
            last_error: state.last_error.clone(),
            last_success_at: state.last_success_at,
            next_retry_at: state.next_retry_at,
        }
    }

    /// Persist explicit consent to inbound access and metadata publication.
    pub async fn set_local_sharing(&self, enabled: bool, full_shadow: bool) -> Result<()> {
        let _lock = self.acquire_session_file_lock().await?;
        let mut session = self.session().ok_or_else(|| anyhow!(CloudSessionExpired))?;
        session.interlink.enabled = enabled;
        session.interlink.full_shadow = full_shadow;
        session.interlink.sharing_configured = true;
        session.save(&self.session_base_dir())?;
        self.install_session(session);
        // Finish the old generation before its next handshake offers new grants.
        if let Some(client) = crate::services::interlink::client::shared() {
            client.stop();
            client.start();
        }
        Ok(())
    }

    pub async fn set_preferences_sync(&self, enabled: bool) -> Result<()> {
        let _file_lock = self.acquire_session_file_lock().await?;
        self.ensure_loaded();
        let snapshot = {
            let mut state = self.lock_state();
            let Some(session) = state.session.as_mut() else {
                return Ok(());
            };
            session.preferences_sync_enabled = enabled;
            session.clone()
        };
        let base_dir = self.session_base_dir();
        snapshot.save(&base_dir)?;
        self.persist_session_fingerprint();
        Ok(())
    }

    // ------------------------------------------------------------------
    // Account / preferences
    // ------------------------------------------------------------------

    /// Raw account fetch with an explicit session; used by the login flow
    /// where the fresh token is not installed yet (so no recovery applies).
    async fn fetch_account(&self, session: &CloudSessionFile) -> Result<CloudAccountSnapshot> {
        let response = self
            .http
            .get(format!("{}/wunder/cloud/account", session.server))
            .bearer_auth(&session.token)
            .header("x-wunder-device-id", &session.device_id)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|err| anyhow!("cloud account fetch failed: {err}"))?;
        let status = response.status();
        let body = read_json(response).await;
        if !status.is_success() {
            return Err(anyhow!(
                "cloud account fetch failed: {status} {}",
                error_message(&body)
            ));
        }
        parse_account_snapshot(&body)
    }

    /// Account fetch through the live session; 401 triggers the single-flight
    /// silent refresh and one retry.
    async fn fetch_cloud_account(&self) -> Result<CloudAccountSnapshot> {
        let response = self
            .authed_send(|session| {
                self.http
                    .get(format!("{}/wunder/cloud/account", session.server))
                    .bearer_auth(&session.token)
                    .header("x-wunder-device-id", &session.device_id)
            })
            .await?;
        let status = response.status();
        let body = read_json(response).await;
        if !status.is_success() {
            return Err(anyhow!(
                "cloud account fetch failed: {status} {}",
                error_message(&body)
            ));
        }
        parse_account_snapshot(&body)
    }

    /// Pull the account overview, refresh the in-memory snapshot and persist.
    pub async fn refresh_account(&self) -> Result<CloudAccountSnapshot> {
        let before = self.session();
        let mut account = self.fetch_cloud_account().await?;
        let _file_lock = self.acquire_session_file_lock().await?;
        self.ensure_loaded();
        if self.session().as_ref().map(|s| (&s.user_id, &s.device_id))
            != before.as_ref().map(|s| (&s.user_id, &s.device_id))
        {
            return Err(anyhow!("cloud session changed during account refresh"));
        }
        let persist = {
            let mut state = self.lock_state();
            if let Some(current) = state.session.as_mut() {
                account.preferences_synced_at = current
                    .account
                    .as_ref()
                    .and_then(|snapshot| snapshot.preferences_synced_at);
                current.account = Some(account.clone());
            }
            state.session.clone()
        };
        if let Some(session) = persist {
            let base_dir = self.session_base_dir();
            session.save(&base_dir)?;
            self.persist_session_fingerprint();
        }
        Ok(account)
    }

    async fn fetch_preferences(&self, session: &CloudSessionFile) -> bool {
        matches!(
            self.http
                .get(format!("{}/wunder/auth/me/preferences", session.server))
                .bearer_auth(&session.token)
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .send()
                .await,
            Ok(response) if response.status().is_success()
        )
    }

    // ------------------------------------------------------------------
    // Interlink user plane (互通方案 I9/I10): the local form drives cloud
    // targets through the same REST endpoints the CLI and the web use.
    // ------------------------------------------------------------------

    async fn interlink_data(
        &self,
        build: impl Fn(&CloudSessionFile) -> reqwest::RequestBuilder,
        what: &'static str,
    ) -> Result<Value> {
        let response = self.authed_send(build).await?;
        let status = response.status();
        let body = read_json(response).await;
        if !status.is_success() {
            return Err(super::CloudInterlinkError {
                status: status.as_u16(),
                operation: what,
                message: error_message(&body),
            }
            .into());
        }
        Ok(body.get("data").cloned().unwrap_or(Value::Null))
    }

    pub async fn cloud_agents(&self) -> Result<Value> {
        self.interlink_data(
            |session| {
                self.http
                    .get(format!("{}/wunder/agents", session.server))
                    .bearer_auth(&session.token)
            },
            "agent catalog",
        )
        .await
    }

    pub async fn cloud_thread_view(&self, thread: &str) -> Result<Value> {
        let thread = urlencode_component(thread);
        self.interlink_data(
            |session| {
                self.http
                    .get(format!(
                        "{}/wunder/chat/sessions/{thread}/view/bootstrap",
                        session.server
                    ))
                    .bearer_auth(&session.token)
            },
            "thread view",
        )
        .await
    }

    pub async fn cloud_thread_models(&self) -> Result<Value> {
        self.interlink_data(
            |session| {
                self.http
                    .get(format!("{}/wunder/agents/models", session.server))
                    .bearer_auth(&session.token)
            },
            "thread models",
        )
        .await
    }

    pub async fn cloud_thread_system_prompt(&self, thread: &str) -> Result<String> {
        let path = if thread.trim().is_empty() {
            "/wunder/chat/system-prompt".to_string()
        } else {
            format!(
                "/wunder/chat/sessions/{}/system-prompt",
                urlencode_component(thread)
            )
        };
        let data = self
            .interlink_data(
                |session| {
                    self.http
                        .post(format!("{}{path}", session.server))
                        .bearer_auth(&session.token)
                        .json(&json!({}))
                },
                "thread system prompt",
            )
            .await?;
        Ok(data["prompt"].as_str().unwrap_or_default().to_string())
    }

    pub async fn cloud_thread_set_model(&self, thread: &str, model: &str) -> Result<()> {
        let thread = urlencode_component(thread);
        self.interlink_data(
            |session| {
                self.http
                    .post(format!(
                        "{}/wunder/chat/sessions/{thread}/model",
                        session.server
                    ))
                    .bearer_auth(&session.token)
                    .json(&json!({"model_name":model}))
            },
            "thread model",
        )
        .await?;
        Ok(())
    }

    pub async fn cloud_thread_set_effort(&self, thread: &str, effort: &str) -> Result<String> {
        let thread = urlencode_component(thread);
        let data = self
            .interlink_data(
                |session| {
                    self.http
                        .post(format!(
                            "{}/wunder/chat/sessions/{thread}/reasoning-effort",
                            session.server
                        ))
                        .bearer_auth(&session.token)
                        .json(&json!({"reasoning_effort":effort}))
                },
                "thread reasoning effort",
            )
            .await?;
        Ok(data["reasoning_effort"]
            .as_str()
            .unwrap_or("default")
            .into())
    }

    pub async fn cloud_thread_chat_page(&self, thread: &str, before: i64) -> Result<Value> {
        let thread = urlencode_component(thread);
        self.interlink_data(
            |session| {
                self.http
                    .get(format!(
                        "{}/wunder/chat/sessions/{thread}/view/chat?before={before}&limit=12",
                        session.server
                    ))
                    .bearer_auth(&session.token)
            },
            "thread history",
        )
        .await
    }

    pub async fn cloud_thread_rename(&self, thread: &str, title: &str) -> Result<()> {
        let thread = urlencode_component(thread);
        let title = title.trim();
        anyhow::ensure!(
            !title.is_empty()
                && title.chars().count() <= 120
                && !title.chars().any(char::is_control),
            "invalid thread title"
        );
        self.interlink_data(
            |session| {
                self.http
                    .post(format!(
                        "{}/wunder/chat/sessions/{thread}/title",
                        session.server
                    ))
                    .bearer_auth(&session.token)
                    .json(&json!({"title":title}))
            },
            "thread rename",
        )
        .await?;
        Ok(())
    }

    pub async fn cloud_thread_archive(&self, thread: &str) -> Result<()> {
        let thread = urlencode_component(thread);
        self.interlink_data(
            |session| {
                self.http
                    .post(format!(
                        "{}/wunder/chat/sessions/{thread}/archive",
                        session.server
                    ))
                    .bearer_auth(&session.token)
            },
            "thread archive",
        )
        .await?;
        Ok(())
    }

    pub async fn cloud_thread_trajectory(
        &self,
        thread: &str,
        before: Option<i64>,
        after: Option<i64>,
        limit: usize,
    ) -> Result<Value> {
        anyhow::ensure!(
            before.is_none() || after.is_none(),
            "ambiguous trajectory cursor"
        );
        let thread = urlencode_component(thread);
        self.interlink_data(
            |session| {
                let mut query = format!("limit={}", limit.clamp(1, 100));
                if let Some(before) = before {
                    query.push_str(&format!("&before={before}"));
                }
                if let Some(after) = after {
                    query.push_str(&format!("&after={after}"));
                }
                self.http
                    .get(format!(
                        "{}/wunder/chat/sessions/{thread}/view/trajectory?{query}",
                        session.server
                    ))
                    .bearer_auth(&session.token)
            },
            "thread trajectory",
        )
        .await
    }

    pub async fn cloud_thread_search(&self, thread: &str, query: &str) -> Result<Value> {
        let thread = urlencode_component(thread);
        let query = urlencode_component(query);
        self.interlink_data(
            |session| {
                self.http
                    .get(format!(
                        "{}/wunder/chat/sessions/{thread}/view/search?q={query}",
                        session.server
                    ))
                    .bearer_auth(&session.token)
            },
            "thread search",
        )
        .await
    }

    pub async fn cloud_thread_process(
        &self,
        thread: &str,
        root: &str,
        before: Option<i64>,
        after: Option<i64>,
    ) -> Result<Value> {
        anyhow::ensure!(
            before.is_none() || after.is_none(),
            "ambiguous process cursor"
        );
        let thread = urlencode_component(thread);
        let root = urlencode_component(root);
        let mut query = "limit=100".to_string();
        if let Some(before) = before.or_else(|| after.is_none().then_some(i64::MAX)) {
            query.push_str(&format!("&item_before={before}"));
        }
        if let Some(after) = after {
            query.push_str(&format!("&item_after={after}"));
        }
        self.interlink_data(
            |session| {
                self.http
                    .get(format!(
                        "{}/wunder/chat/sessions/{thread}/thread-log/turns/{root}?{query}",
                        session.server
                    ))
                    .bearer_auth(&session.token)
            },
            "thread process",
        )
        .await
    }

    /// Stream the authenticated NDJSON export to disk without retaining a
    /// transcript-sized byte buffer in the desktop process.
    pub async fn cloud_export_thread_log(
        &self,
        thread: &str,
        path: &std::path::Path,
    ) -> Result<()> {
        use futures::StreamExt;
        use tokio::io::AsyncWriteExt;
        let thread = urlencode_component(thread);
        let response = self
            .authed_send(|session| {
                self.http
                    .get(format!(
                        "{}/wunder/chat/sessions/{thread}/thread-log/export",
                        session.server
                    ))
                    .bearer_auth(&session.token)
            })
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "cloud thread export failed: {}",
            response.status()
        );
        let mut file = tokio::fs::File::create(path).await?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            file.write_all(&chunk?).await?;
        }
        file.flush().await?;
        Ok(())
    }

    pub async fn cloud_thread_body(
        &self,
        thread: &str,
        body: &str,
        revision: i64,
        cursor: i64,
    ) -> Result<Value> {
        let thread = urlencode_component(thread);
        let body = urlencode_component(body);
        self.interlink_data(|session| self.http.get(format!(
            "{}/wunder/chat/sessions/{thread}/bodies/{body}?revision={revision}&cursor={cursor}", session.server
        )).bearer_auth(&session.token), "thread body").await
    }

    /// Device fleet with tunnel-aware presence, as the user sees it.
    pub async fn interlink_nodes(&self) -> Result<Value> {
        self.interlink_data(
            |session| {
                self.http
                    .get(format!("{}/wunder/interlink/nodes", session.server))
                    .bearer_auth(&session.token)
            },
            "nodes",
        )
        .await
    }

    /// Issue one interlink command to `cloud` or `device:<id>`; returns the
    /// ledger record head (`command_id`, `status`, `approval_state`, ...).
    pub async fn interlink_issue_command(
        &self,
        to_node: &str,
        kind: &str,
        args: Value,
    ) -> Result<Value> {
        let to_node = to_node.to_string();
        let kind = kind.to_string();
        self.interlink_data(
            |session| {
                self.http
                    .post(format!("{}/wunder/interlink/commands", session.server))
                    .bearer_auth(&session.token)
                    .json(&json!({"to": to_node, "kind": kind, "args": args}))
            },
            "command issue",
        )
        .await
    }

    /// One command ledger record (status, approval_state, result, ...).
    pub async fn interlink_command(&self, command_id: &str) -> Result<Value> {
        let command_id = command_id.to_string();
        self.interlink_data(
            |session| {
                self.http
                    .get(format!(
                        "{}/wunder/interlink/commands/{}",
                        session.server,
                        urlencode_component(&command_id)
                    ))
                    .bearer_auth(&session.token)
            },
            "command query",
        )
        .await
    }

    pub async fn interlink_cancel_command(&self, command_id: &str) -> Result<Value> {
        let command_id = urlencode_component(command_id);
        self.interlink_data(
            |session| {
                self.http
                    .post(format!(
                        "{}/wunder/interlink/commands/{command_id}/cancel",
                        session.server
                    ))
                    .bearer_auth(&session.token)
                    .json(&json!({}))
            },
            "command cancel",
        )
        .await
    }

    /// Decide a cloud-target approval ticket as the account owner.
    pub async fn interlink_decide(&self, command_id: &str, approved: bool) -> Result<Value> {
        let command_id = command_id.to_string();
        let decision = if approved { "approved" } else { "rejected" }.to_string();
        self.interlink_data(
            |session| {
                self.http
                    .post(format!(
                        "{}/wunder/interlink/commands/{}/approval",
                        session.server,
                        urlencode_component(&command_id)
                    ))
                    .bearer_auth(&session.token)
                    .json(&json!({"decision": decision}))
            },
            "approval decide",
        )
        .await
    }

    /// Download the cloud workspace (or one subtree) as a zip blob
    /// (`GET /wunder/workspace/archive`). The interlink tunnel cannot carry a
    /// zip, so the desktop asks the cloud REST plane directly.
    pub async fn cloud_workspace_archive(&self, path: &str) -> Result<Vec<u8>> {
        let path = urlencode_component(path.trim());
        let response = self
            .authed_send(|session| {
                self.http
                    .get(format!(
                        "{}/wunder/workspace/archive?path={}",
                        session.server, path
                    ))
                    .bearer_auth(&session.token)
                    .header("x-wunder-device-id", &session.device_id)
            })
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = read_json(response).await;
            return Err(anyhow!(
                "cloud workspace archive failed: {status} {}",
                error_message(&body)
            ));
        }
        Ok(response.bytes().await?.to_vec())
    }

    /// Clear every entry under the cloud workspace root
    /// (`POST /wunder/workspace/clear`).
    pub async fn cloud_workspace_clear(&self) -> Result<()> {
        let response = self
            .authed_send(|session| {
                self.http
                    .post(format!("{}/wunder/workspace/clear", session.server))
                    .bearer_auth(&session.token)
                    .header("x-wunder-device-id", &session.device_id)
                    .json(&json!({}))
            })
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = read_json(response).await;
            return Err(anyhow!(
                "cloud workspace clear failed: {status} {}",
                error_message(&body)
            ));
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Cloud model synthesis (§4.2)
    // ------------------------------------------------------------------

    async fn fetch_cloud_model_ids(&self) -> Result<Vec<String>> {
        let response = self
            .authed_send(|session| {
                self.http
                    .get(format!("{}/wunder/cloud/v1/models", session.server))
                    .bearer_auth(&session.token)
                    .header("x-wunder-device-id", &session.device_id)
            })
            .await?;
        let status = response.status();
        let body = read_json(response).await;
        if !status.is_success() {
            return Err(anyhow!(
                "cloud model discovery failed: {status} {}",
                error_message(&body)
            ));
        }
        let ids = body["data"]
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry["id"].as_str())
                    .map(|id| id.trim().to_string())
                    .filter(|id| !id.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(ids)
    }

    /// Synthesize `cloud/<id>` model entries into the local config. Idempotent:
    /// identical entries are left untouched (no config rewrite), stale
    /// `cloud/` entries are dropped and re-login refreshes tokens in place.
    pub async fn synthesize_cloud_models(&self, config_store: &ConfigStore) -> Result<Vec<String>> {
        // Read the session snapshot after the discovery call: a 401 in between
        // silently rotates the token and the synthesized entries must carry
        // the fresh one.
        let ids = self.fetch_cloud_model_ids().await?;
        let session = self.session().ok_or_else(|| anyhow!("not logged in"))?;
        let desired = desired_cloud_models(&session.server, &session.token, &ids);

        let current = config_store.get().await;
        if cloud_config_matches(&current, &desired) {
            return Ok(desired.into_keys().collect());
        }

        config_store
            .update(|config| {
                let stale: Vec<String> = config
                    .llm
                    .models
                    .keys()
                    .filter(|key| {
                        key.starts_with(CLOUD_MODEL_PREFIX) && !desired.contains_key(key.as_str())
                    })
                    .cloned()
                    .collect();
                for key in stale {
                    config.llm.models.remove(&key);
                }
                for (key, model) in &desired {
                    config.llm.models.insert(key.clone(), model.clone());
                }
                ensure_local_default(config);
            })
            .await?;
        Ok(desired.into_keys().collect())
    }

    // ------------------------------------------------------------------
    // Log reporting
    // ------------------------------------------------------------------

    /// Manual flush hook (cli `cloud logs --flush`).
    pub async fn flush_logs(&self) -> Result<usize> {
        let Some(reporter) = self.reporter() else {
            return Ok(0);
        };
        self.flush_in_flight.store(true, Ordering::Release);
        self.flush_wake.notify_waiters();
        // Give the reporter loop a moment to pick the flush up.
        for _ in 0..40 {
            if reporter.pending() == 0 && !self.flush_in_flight.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(reporter.pending())
    }

    async fn upload_log_batch(&self, batch: Vec<CloudLogEntry>) -> Result<i64> {
        if batch.is_empty() {
            return Ok(self.current_log_cursor());
        }
        let max_seq = batch.last().map(|entry| entry.seq).unwrap_or(0);
        let response = self
            .authed_send(|session| {
                self.http
                    .post(format!("{}/wunder/cloud/logs", session.server))
                    .bearer_auth(&session.token)
                    .header("x-wunder-device-id", &session.device_id)
                    .json(&json!({
                        "device_id": session.device_id,
                        "client": session.client,
                        "logs": batch,
                    }))
            })
            .await?;
        let status = response.status();
        let body = read_json(response).await;
        if !status.is_success() {
            return Err(anyhow!(
                "cloud log upload failed: {status} {}",
                error_message(&body)
            ));
        }
        let parsed: Result<LogUploadResponse> =
            serde_json::from_value(body.get("data").cloned().unwrap_or(Value::Null))
                .map_err(|err| anyhow!("cloud log upload response invalid: {err}"));
        let last_seq = parsed?.last_seq.unwrap_or(max_seq).max(max_seq);
        self.persist_log_cursor(last_seq);
        Ok(last_seq)
    }

    fn current_log_cursor(&self) -> i64 {
        self.session()
            .map(|session| session.log_report.last_synced_seq)
            .unwrap_or(0)
    }

    fn persist_log_cursor(&self, last_seq: i64) {
        let base = self.session_base_dir();
        let Ok(lock) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(base.join("config/cloud.session.lock"))
        else {
            return;
        };
        if lock.try_lock().is_err() {
            return;
        }
        self.ensure_loaded();
        let persist = {
            let mut state = self.lock_state();
            let Some(session) = state.session.as_mut() else {
                return;
            };
            if last_seq <= session.log_report.last_synced_seq {
                return;
            }
            session.log_report.last_synced_seq = last_seq;
            session.clone()
        };
        let base_dir = self.session_base_dir();
        let _ = persist.save(&base_dir);
        self.persist_session_fingerprint();
        self.flush_in_flight.store(false, Ordering::Release);
    }
}

impl Default for CloudService {
    fn default() -> Self {
        Self::new()
    }
}

/// Reporter loop: flush every 30s or on wake (buffer trigger / manual flush),
/// exponential backoff 30s -> 5min while the upload fails. While the buffer
/// stays empty the loop still fires a heartbeat (empty log batch) every
/// `HEARTBEAT_INTERVAL_SECS` so the server sees the device as alive. Uploads
/// go through the process singleton; the loop reads the live session each
/// turn so token rotation is picked up immediately, and it terminates by
/// itself on logout or when a newer reporter takes over.
async fn reporter_loop(reporter: Arc<LogReporter>, in_flight: Arc<AtomicBool>) {
    let service = super::shared();
    let mut backoff_secs = INITIAL_BACKOFF_SECS;
    let mut next_heartbeat =
        std::time::Instant::now() + Duration::from_secs(HEARTBEAT_INTERVAL_SECS);
    loop {
        if service
            .reporter()
            .map(|current| !Arc::ptr_eq(&current, &reporter))
            != Some(false)
        {
            // Replaced by a new session (re-login) or removed (logout).
            return;
        }
        if service.session().is_none() {
            return;
        }
        let pending = reporter.pending();
        let until_heartbeat = next_heartbeat.saturating_duration_since(std::time::Instant::now());
        let wait = if pending >= FLUSH_TRIGGER_ITEMS {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(backoff_secs.min(PERIODIC_FLUSH_SECS)).min(until_heartbeat)
        };
        tokio::select! {
            _ = reporter.wake.notified() => {},
            _ = service.flush_wake.notified() => {},
            _ = tokio::time::sleep(wait) => {},
        }
        let batch = reporter.take_batch();
        if batch.is_empty() {
            in_flight.store(false, Ordering::Release);
            backoff_secs = INITIAL_BACKOFF_SECS;
            if std::time::Instant::now() < next_heartbeat {
                continue;
            }
            // Idle heartbeat: keeps the device `last_seen_at` fresh and
            // doubles as the connectivity probe for the state machine.
            match service.send_heartbeat().await {
                Ok(()) => {
                    next_heartbeat =
                        std::time::Instant::now() + Duration::from_secs(HEARTBEAT_INTERVAL_SECS);
                }
                Err(err) => {
                    next_heartbeat =
                        std::time::Instant::now() + Duration::from_secs(HEARTBEAT_RETRY_SECS);
                    tracing::debug!("cloud heartbeat deferred: {err}");
                }
            }
            continue;
        }
        if let Err(err) = service.upload_log_batch(batch.clone()).await {
            tracing::debug!("cloud log upload deferred: {err}");
            reporter.requeue_front(batch);
            backoff_secs = backoff_secs.saturating_mul(2).min(MAX_BACKOFF_SECS);
            let retry_at = super::reporter::now_unix_seconds() + backoff_secs as f64;
            service.set_next_retry_at(Some(retry_at));
        } else {
            backoff_secs = INITIAL_BACKOFF_SECS;
            service.set_next_retry_at(None);
        }
    }
}

/// Proactive token-renewal keeper: checks the access-token expiry every
/// `TOKEN_RENEW_CHECK_INTERVAL_SECS` (first check right away) and refreshes
/// through the single-flight `recover_auth` when the token is about to
/// expire. Exits once the session is gone.
async fn session_keeper_loop() {
    let service = super::shared();
    loop {
        if service.session().is_none() {
            return;
        }
        service.renew_if_needed().await;
        tokio::time::sleep(Duration::from_secs(TOKEN_RENEW_CHECK_INTERVAL_SECS)).await;
    }
}

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------

async fn read_json(response: reqwest::Response) -> Value {
    match response.text().await {
        Ok(text) => serde_json::from_str(&text).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    }
}

/// Sanitized network-failure summary for the connection state machine and
/// logs: classifies the error without URL details or any token material.
pub(crate) fn network_error_summary(err: &reqwest::Error) -> String {
    if err.is_timeout() {
        "request timeout".to_string()
    } else if err.is_connect() {
        "connection failed".to_string()
    } else {
        "network request failed".to_string()
    }
}

fn parse_account_snapshot(body: &Value) -> Result<CloudAccountSnapshot> {
    let data = &body["data"];
    Ok(CloudAccountSnapshot {
        quota: super::session::CloudQuotaSnapshot {
            balance: data["quota"]["balance"].as_i64().unwrap_or(0),
            granted_total: data["quota"]["granted_total"].as_i64().unwrap_or(0),
            used_total: data["quota"]["used_total"].as_i64().unwrap_or(0),
            daily_grant: data["quota"]["daily_grant"].as_i64().unwrap_or(0),
            last_grant_date: data["quota"]["last_grant_date"]
                .as_str()
                .map(str::to_string),
        },
        max_concurrent_calls: data["concurrency"]["max_per_user"].as_u64().unwrap_or(2),
        concurrency_active: data["concurrency"]["active"].as_u64(),
        concurrency_queued: data["concurrency"]["queued"].as_u64(),
        preferences_synced_at: None,
    })
}

fn error_message(body: &Value) -> String {
    body["error"]["message"]
        .as_str()
        .or_else(|| body["message"].as_str())
        .unwrap_or("(no detail)")
        .chars()
        .take(200)
        .collect()
}

/// Percent-encode one path/query component (RFC 3986 unreserved set passes).
fn urlencode_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn normalize_client(client: &str) -> String {
    let trimmed = client.trim().to_ascii_lowercase();
    if trimmed == "cli" {
        "cli".to_string()
    } else {
        "desktop".to_string()
    }
}

fn normalize_server(server: &str) -> Result<String> {
    let trimmed = server.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err(anyhow!("server address is required"));
    }
    let lowered = trimmed.to_ascii_lowercase();
    if !(lowered.starts_with("http://") || lowered.starts_with("https://")) {
        return Err(anyhow!(
            "server address must start with http:// or https://"
        ));
    }
    Ok(trimmed)
}

fn local_device_name(client: &str) -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("wunder-{client}"))
}

/// Build the desired `cloud/<id>` config entries from the exposed model ids.
pub fn desired_cloud_models(
    server: &str,
    token: &str,
    ids: &[String],
) -> BTreeMap<String, LlmModelConfig> {
    let base_url = format!("{}/wunder/cloud/v1", server.trim_end_matches('/'));
    ids.iter()
        .map(|id| {
            (
                format!("{CLOUD_MODEL_PREFIX}{id}"),
                LlmModelConfig {
                    provider: Some(CLOUD_PROVIDER.to_string()),
                    base_url: Some(base_url.clone()),
                    api_key: Some(token.to_string()),
                    model: Some(id.clone()),
                    model_type: Some("llm".to_string()),
                    enable: Some(true),
                    ..Default::default()
                },
            )
        })
        .collect()
}

/// True when the config already holds exactly the desired cloud entries and a
/// local default is in place — no rewrite needed.
fn cloud_config_matches(
    config: &crate::config::Config,
    desired: &BTreeMap<String, LlmModelConfig>,
) -> bool {
    for (key, _model) in config.llm.models.iter() {
        if key.starts_with(CLOUD_MODEL_PREFIX) && !desired.contains_key(key.as_str()) {
            return false;
        }
    }
    for (key, model) in desired {
        match config.llm.models.get(key) {
            Some(current) if cloud_entry_matches(current, model) => {}
            _ => return false,
        }
    }
    default_rule_satisfied(config)
}

fn cloud_entry_matches(current: &LlmModelConfig, desired: &LlmModelConfig) -> bool {
    current.provider.as_deref() == desired.provider.as_deref()
        && current.base_url.as_deref() == desired.base_url.as_deref()
        && current.api_key.as_deref() == desired.api_key.as_deref()
        && current.model.as_deref() == desired.model.as_deref()
        && current.model_type.as_deref() == desired.model_type.as_deref()
        && current.enable == desired.enable
}

/// `llm.default` never points at a cloud key while any local llm model exists.
fn default_rule_satisfied(config: &crate::config::Config) -> bool {
    if !config.llm.default.starts_with(CLOUD_MODEL_PREFIX) {
        return true;
    }
    first_local_llm_key(config).is_none()
}

fn ensure_local_default(config: &mut crate::config::Config) {
    if !config.llm.default.starts_with(CLOUD_MODEL_PREFIX) {
        return;
    }
    if let Some(key) = first_local_llm_key(config) {
        config.llm.default = key;
    }
}

fn first_local_llm_key(config: &crate::config::Config) -> Option<String> {
    config
        .llm
        .models
        .iter()
        .filter(|(key, model)| {
            !key.starts_with(CLOUD_MODEL_PREFIX)
                && crate::llm::is_llm_model(model)
                && model.enable != Some(false)
        })
        .map(|(key, _)| key.clone())
        .min()
}

/// Remove every synthesized cloud entry and restore a local default.
pub async fn remove_cloud_models(config_store: &ConfigStore) -> Result<()> {
    let current = config_store.get().await;
    let has_cloud = current
        .llm
        .models
        .keys()
        .any(|key| key.starts_with(CLOUD_MODEL_PREFIX));
    let default_is_cloud = current.llm.default.starts_with(CLOUD_MODEL_PREFIX)
        && first_local_llm_key(&current).is_some();
    if !has_cloud && !default_is_cloud {
        return Ok(());
    }
    config_store
        .update(|config| {
            let stale: Vec<String> = config
                .llm
                .models
                .keys()
                .filter(|key| key.starts_with(CLOUD_MODEL_PREFIX))
                .cloned()
                .collect();
            for key in stale {
                config.llm.models.remove(&key);
            }
            ensure_local_default(config);
        })
        .await?;
    Ok(())
}
