use crate::config::{Config, LlmConfig};
use crate::state::AppState;
use crate::storage::{
    normalize_workspace_container_id, MAX_SANDBOX_CONTAINER_ID, USER_PRIVATE_CONTAINER_ID,
};
use crate::{i18n, llm};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{
    routing::{get, post},
    Json, Router,
};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration, Instant};
use tokio_util::io::ReaderStream;
use url::Url;
use uuid::Uuid;
use walkdir::WalkDir;

const DESKTOP_SETTINGS_PATH_ENV: &str = "WUNDER_DESKTOP_SETTINGS_PATH";
const DESKTOP_APP_DIR_ENV: &str = "WUNDER_DESKTOP_APP_DIR";
const DESKTOP_DEFAULT_WORKSPACE_ROOT_ENV: &str = "WUNDER_DESKTOP_DEFAULT_WORKSPACE_ROOT";
const DESKTOP_USER_ID_ENV: &str = "WUNDER_DESKTOP_USER_ID";
const DEFAULT_SEED_QUERY_LIMIT: usize = 50;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/desktop/settings",
            get(desktop_settings_get).put(desktop_settings_update),
        )
        .route(
            "/wunder/desktop/llm/context_window",
            post(desktop_llm_context_window),
        )
        .route(
            "/wunder/admin/llm/context_window",
            post(desktop_llm_context_window),
        )
        .route("/wunder/desktop/fs/list", get(desktop_fs_list))
        .route("/wunder/desktop/sync/seed/start", post(desktop_seed_start))
        .route("/wunder/desktop/sync/seed/jobs", get(desktop_seed_jobs))
        .route(
            "/wunder/desktop/sync/seed/jobs/{job_id}",
            get(desktop_seed_job_get),
        )
        .route(
            "/wunder/desktop/sync/seed/control",
            post(desktop_seed_control),
        )
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DesktopRemoteGatewaySettings {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    server_base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopSettingsFile {
    workspace_root: String,
    desktop_token: String,
    #[serde(default)]
    container_roots: HashMap<i32, String>,
    #[serde(default)]
    container_cloud_workspaces: HashMap<i32, String>,
    #[serde(default)]
    language: String,
    #[serde(default)]
    llm: Option<LlmConfig>,
    #[serde(default)]
    remote_gateway: DesktopRemoteGatewaySettings,
    updated_at: f64,
}

impl Default for DesktopSettingsFile {
    fn default() -> Self {
        Self {
            workspace_root: String::new(),
            desktop_token: String::new(),
            container_roots: HashMap::new(),
            container_cloud_workspaces: HashMap::new(),
            language: String::new(),
            llm: None,
            remote_gateway: DesktopRemoteGatewaySettings::default(),
            updated_at: now_ts(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DesktopContainerRootItem {
    container_id: i32,
    root: String,
}

#[derive(Debug, Clone, Serialize)]
struct DesktopContainerMountItem {
    container_id: i32,
    root: String,
    cloud_workspace_id: String,
    seed_status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DesktopContainerRootInput {
    container_id: i32,
    root: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DesktopContainerMountInput {
    container_id: i32,
    #[serde(default)]
    root: String,
    #[serde(default)]
    cloud_workspace_id: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct DesktopSettingsUpdateRequest {
    #[serde(default)]
    workspace_root: Option<String>,
    #[serde(default)]
    container_roots: Option<Vec<DesktopContainerRootInput>>,
    #[serde(default)]
    container_mounts: Option<Vec<DesktopContainerMountInput>>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    llm: Option<LlmConfig>,
    #[serde(default)]
    remote_gateway: Option<DesktopRemoteGatewaySettings>,
}

#[derive(Debug, Clone, Deserialize)]
struct DesktopLlmContextProbeRequest {
    #[serde(default)]
    provider: Option<String>,
    base_url: String,
    #[serde(default)]
    api_key: Option<String>,
    model: String,
    #[serde(default)]
    timeout_s: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct DesktopDirectoryItem {
    name: String,
    path: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct DesktopDirectoryListQuery {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct DesktopSeedStartRequest {
    #[serde(default)]
    container_id: Option<i32>,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    local_root: String,
    #[serde(default)]
    remote_api_base: String,
    #[serde(default)]
    cloud_workspace_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DesktopSeedControlRequest {
    #[serde(default)]
    job_id: String,
    #[serde(default)]
    action: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct DesktopSeedJobsQuery {
    #[serde(default)]
    container_id: Option<i32>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
struct DesktopSeedProgress {
    percent: f64,
    processed_files: usize,
    total_files: usize,
    processed_bytes: u64,
    total_bytes: u64,
    speed_bps: f64,
    eta_seconds: Option<u64>,
}

impl Default for DesktopSeedProgress {
    fn default() -> Self {
        Self {
            percent: 0.0,
            processed_files: 0,
            total_files: 0,
            processed_bytes: 0,
            total_bytes: 0,
            speed_bps: 0.0,
            eta_seconds: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DesktopSeedJobSnapshot {
    job_id: String,
    container_id: i32,
    local_root: String,
    cloud_workspace_id: String,
    remote_api_base: String,
    stage: String,
    status: String,
    progress: DesktopSeedProgress,
    #[serde(skip_serializing_if = "String::is_empty")]
    current_item: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
    created_at: f64,
    updated_at: f64,
    started_at: Option<f64>,
    finished_at: Option<f64>,
}

#[derive(Debug, Clone)]
struct DesktopSeedJobConfig {
    container_id: i32,
    local_root: PathBuf,
    cloud_workspace_id: String,
    remote_api_base: String,
    access_token: String,
}

struct DesktopSeedJobHandle {
    snapshot: RwLock<DesktopSeedJobSnapshot>,
    paused: AtomicBool,
    canceled: AtomicBool,
}

impl DesktopSeedJobHandle {
    fn new(snapshot: DesktopSeedJobSnapshot) -> Self {
        Self {
            snapshot: RwLock::new(snapshot),
            paused: AtomicBool::new(false),
            canceled: AtomicBool::new(false),
        }
    }
}

struct DesktopSeedManager {
    jobs: RwLock<HashMap<String, Arc<DesktopSeedJobHandle>>>,
    latest_by_container: RwLock<HashMap<i32, String>>,
    http: reqwest::Client,
}

impl DesktopSeedManager {
    fn new() -> Self {
        Self {
            jobs: RwLock::new(HashMap::new()),
            latest_by_container: RwLock::new(HashMap::new()),
            http: reqwest::Client::new(),
        }
    }

    async fn start_job(
        &self,
        config: DesktopSeedJobConfig,
    ) -> Result<DesktopSeedJobSnapshot, String> {
        let container_id = normalize_workspace_container_id(config.container_id);
        if let Some(existing) = self.latest_job_for_container(container_id).await {
            if is_seed_job_active_status(&existing.status) {
                return Err(format!(
                    "seed job already running for container {container_id}: {}",
                    existing.job_id
                ));
            }
        }

        let now = now_ts();
        let job_id = format!("seed_{}", Uuid::new_v4().simple());
        let snapshot = DesktopSeedJobSnapshot {
            job_id: job_id.clone(),
            container_id,
            local_root: config.local_root.to_string_lossy().to_string(),
            cloud_workspace_id: config.cloud_workspace_id.clone(),
            remote_api_base: config.remote_api_base.clone(),
            stage: "discovering".to_string(),
            status: "running".to_string(),
            progress: DesktopSeedProgress {
                percent: 1.0,
                ..DesktopSeedProgress::default()
            },
            current_item: String::new(),
            error: String::new(),
            created_at: now,
            updated_at: now,
            started_at: Some(now),
            finished_at: None,
        };
        let job = Arc::new(DesktopSeedJobHandle::new(snapshot.clone()));

        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(job_id.clone(), job.clone());
        }
        {
            let mut latest = self.latest_by_container.write().await;
            latest.insert(container_id, job_id);
        }

        let client = self.http.clone();
        tokio::spawn(async move {
            run_seed_job(client, config, job).await;
        });
        Ok(snapshot)
    }

    async fn latest_job_for_container(&self, container_id: i32) -> Option<DesktopSeedJobSnapshot> {
        let job_id = self
            .latest_by_container
            .read()
            .await
            .get(&container_id)
            .cloned()?;
        self.get_job(&job_id).await
    }

    async fn get_job(&self, job_id: &str) -> Option<DesktopSeedJobSnapshot> {
        let handle = self.jobs.read().await.get(job_id).cloned()?;
        let snapshot = handle.snapshot.read().await.clone();
        Some(snapshot)
    }

    async fn list_jobs(
        &self,
        container_id: Option<i32>,
        limit: usize,
    ) -> Vec<DesktopSeedJobSnapshot> {
        let normalized_container = container_id.map(normalize_workspace_container_id);
        let handles = self.jobs.read().await.values().cloned().collect::<Vec<_>>();
        let mut snapshots = Vec::new();
        for handle in handles {
            let snapshot = handle.snapshot.read().await.clone();
            if let Some(target) = normalized_container {
                if snapshot.container_id != target {
                    continue;
                }
            }
            snapshots.push(snapshot);
        }
        snapshots.sort_by(|left, right| right.created_at.total_cmp(&left.created_at));
        snapshots.truncate(limit.max(1));
        snapshots
    }

    async fn container_seed_statuses(&self) -> HashMap<i32, String> {
        let latest = self.latest_by_container.read().await.clone();
        let mut output = HashMap::new();
        for (container_id, job_id) in latest {
            if let Some(snapshot) = self.get_job(&job_id).await {
                output.insert(container_id, snapshot.status);
            }
        }
        output
    }

    async fn control_job(
        &self,
        job_id: &str,
        action: &str,
    ) -> Result<DesktopSeedJobSnapshot, String> {
        let handle = self
            .jobs
            .read()
            .await
            .get(job_id)
            .cloned()
            .ok_or_else(|| format!("seed job not found: {job_id}"))?;
        match action.trim().to_ascii_lowercase().as_str() {
            "pause" => {
                let mut snapshot = handle.snapshot.write().await;
                if !is_seed_job_active_status(&snapshot.status) {
                    return Err("seed job is not active".to_string());
                }
                handle.paused.store(true, Ordering::SeqCst);
                snapshot.status = "paused".to_string();
                snapshot.updated_at = now_ts();
                Ok(snapshot.clone())
            }
            "resume" => {
                let mut snapshot = handle.snapshot.write().await;
                if snapshot.status != "paused" {
                    return Err("seed job is not paused".to_string());
                }
                handle.paused.store(false, Ordering::SeqCst);
                snapshot.status = "running".to_string();
                snapshot.updated_at = now_ts();
                Ok(snapshot.clone())
            }
            "cancel" => {
                handle.canceled.store(true, Ordering::SeqCst);
                handle.paused.store(false, Ordering::SeqCst);
                let mut snapshot = handle.snapshot.write().await;
                if !is_seed_job_terminal_status(&snapshot.status) {
                    snapshot.status = "canceled".to_string();
                    snapshot.stage = "canceled".to_string();
                    snapshot.updated_at = now_ts();
                    snapshot.finished_at = Some(now_ts());
                }
                Ok(snapshot.clone())
            }
            _ => Err(format!("unsupported seed action: {action}")),
        }
    }
}

fn desktop_seed_manager() -> &'static DesktopSeedManager {
    static INSTANCE: OnceLock<DesktopSeedManager> = OnceLock::new();
    INSTANCE.get_or_init(DesktopSeedManager::new)
}

async fn desktop_settings_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let (settings_path, app_dir, default_workspace_root) =
        resolve_desktop_paths().map_err(bad_request)?;
    let user_id = resolve_desktop_user_id();
    let mut settings = load_desktop_settings(&settings_path).map_err(internal_error)?;
    let resolved_workspace_root =
        resolve_desktop_workspace_root(&settings, &default_workspace_root, &app_dir);
    settings.container_roots = normalize_desktop_container_roots(
        &settings.container_roots,
        &resolved_workspace_root,
        &user_id,
        &app_dir,
    );
    settings.container_cloud_workspaces =
        normalize_desktop_container_cloud_workspaces(&settings.container_cloud_workspaces);
    settings
        .container_cloud_workspaces
        .retain(|container_id, _| settings.container_roots.contains_key(container_id));
    settings.workspace_root = resolved_workspace_root.to_string_lossy().to_string();

    let config = state.config_store.get().await;
    let seed_statuses = desktop_seed_manager().container_seed_statuses().await;
    Ok(Json(
        json!({ "data": build_settings_payload(&config, &settings, &seed_statuses) }),
    ))
}

async fn desktop_llm_context_window(
    Json(payload): Json<DesktopLlmContextProbeRequest>,
) -> Result<Json<Value>, Response> {
    let model = payload.model.trim();
    let provider = llm::normalize_provider(payload.provider.as_deref());
    let inline_base = payload.base_url.trim();
    let base_url = if inline_base.is_empty() {
        llm::provider_default_base_url(&provider).unwrap_or("")
    } else {
        inline_base
    };
    if base_url.is_empty() || model.is_empty() {
        return Err(bad_request(i18n::t("error.base_url_or_model_required")));
    }
    if !llm::is_openai_compatible_provider(&provider) {
        return Ok(Json(json!({
            "max_context": Value::Null,
            "message": i18n::t("probe.provider_unsupported")
        })));
    }

    let timeout_s = payload.timeout_s.unwrap_or(15);
    let timeout_s = if timeout_s == 0 { 15 } else { timeout_s };
    let api_key = payload.api_key.as_deref().unwrap_or("");
    let result = llm::probe_openai_context_window(base_url, api_key, model, timeout_s).await;
    let payload = match result {
        Ok(Some(value)) => json!({ "max_context": value, "message": i18n::t("probe.success") }),
        Ok(None) => json!({ "max_context": Value::Null, "message": i18n::t("probe.no_context") }),
        Err(err) => {
            let message = i18n::t_with_params(
                "probe.failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            );
            json!({ "max_context": Value::Null, "message": message })
        }
    };
    Ok(Json(payload))
}

async fn desktop_fs_list(
    State(_state): State<Arc<AppState>>,
    Query(query): Query<DesktopDirectoryListQuery>,
) -> Result<Json<Value>, Response> {
    let (settings_path, app_dir, default_workspace_root) =
        resolve_desktop_paths().map_err(bad_request)?;
    let user_id = resolve_desktop_user_id();
    let mut settings = load_desktop_settings(&settings_path).map_err(internal_error)?;
    let resolved_workspace_root =
        resolve_desktop_workspace_root(&settings, &default_workspace_root, &app_dir);
    settings.container_roots = normalize_desktop_container_roots(
        &settings.container_roots,
        &resolved_workspace_root,
        &user_id,
        &app_dir,
    );
    settings.workspace_root = resolved_workspace_root.to_string_lossy().to_string();

    let current_path = resolve_desktop_list_path(
        query.path.as_deref(),
        &settings,
        &default_workspace_root,
        &app_dir,
    );
    if !current_path.exists() {
        return Err(bad_request(format!(
            "path not found: {}",
            current_path.display()
        )));
    }
    if !current_path.is_dir() {
        return Err(bad_request(format!(
            "path is not a directory: {}",
            current_path.display()
        )));
    }

    let mut items = Vec::new();
    let entries = fs::read_dir(&current_path).map_err(|err| {
        internal_error(format!(
            "read desktop directory failed {}: {err}",
            current_path.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            internal_error(format!(
                "iterate desktop directory failed {}: {err}",
                current_path.display()
            ))
        })?;
        let file_type = entry.file_type().map_err(|err| {
            internal_error(format!(
                "read desktop directory entry type failed {}: {err}",
                entry.path().display()
            ))
        })?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.trim().is_empty() {
            continue;
        }
        items.push(DesktopDirectoryItem {
            name,
            path: entry.path().to_string_lossy().to_string(),
        });
    }
    items.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then(left.name.cmp(&right.name))
    });

    let parent_path = current_path
        .parent()
        .map(|value| value.to_string_lossy().to_string());
    Ok(Json(json!({
        "data": {
            "current_path": current_path.to_string_lossy().to_string(),
            "parent_path": parent_path,
            "roots": list_desktop_directory_roots(),
            "items": items,
        }
    })))
}

async fn desktop_settings_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DesktopSettingsUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let (settings_path, app_dir, default_workspace_root) =
        resolve_desktop_paths().map_err(bad_request)?;
    let user_id = resolve_desktop_user_id();
    let mut settings = load_desktop_settings(&settings_path).map_err(internal_error)?;
    let resolved_workspace_root = if let Some(workspace_root) = payload.workspace_root.as_deref() {
        let trimmed = workspace_root.trim();
        if trimmed.is_empty() {
            resolve_desktop_workspace_root(&settings, &default_workspace_root, &app_dir)
        } else {
            resolve_workspace_path(trimmed, &app_dir)
        }
    } else {
        resolve_desktop_workspace_root(&settings, &default_workspace_root, &app_dir)
    };

    let (mut container_roots, mut container_cloud_workspaces) =
        if let Some(items) = payload.container_mounts.as_ref() {
            let mut roots = HashMap::new();
            let mut clouds = HashMap::new();
            for item in items {
                let container_id = normalize_workspace_container_id(item.container_id);
                let trimmed_root = item.root.trim();
                if !trimmed_root.is_empty() {
                    roots.insert(container_id, trimmed_root.to_string());
                }
                let cloud_workspace_id = item.cloud_workspace_id.trim();
                if !cloud_workspace_id.is_empty() {
                    clouds.insert(container_id, cloud_workspace_id.to_string());
                }
            }
            (roots, clouds)
        } else {
            (
                settings.container_roots.clone(),
                settings.container_cloud_workspaces.clone(),
            )
        };

    if let Some(items) = payload.container_roots.as_ref() {
        let mut next_roots = HashMap::new();
        for item in items {
            let container_id = normalize_workspace_container_id(item.container_id);
            let trimmed = item.root.trim();
            if trimmed.is_empty() {
                continue;
            }
            next_roots.insert(container_id, trimmed.to_string());
        }
        container_roots = next_roots;
    }

    container_roots = normalize_desktop_container_roots(
        &container_roots,
        &resolved_workspace_root,
        &user_id,
        &app_dir,
    );
    fs::create_dir_all(&resolved_workspace_root).map_err(|err| {
        internal_error(format!(
            "create desktop workspace root failed {}: {err}",
            resolved_workspace_root.display()
        ))
    })?;
    ensure_container_root_dirs(&container_roots).map_err(internal_error)?;
    container_cloud_workspaces =
        normalize_desktop_container_cloud_workspaces(&container_cloud_workspaces);
    container_cloud_workspaces.retain(|container_id, _| container_roots.contains_key(container_id));

    settings.container_roots = container_roots.clone();
    settings.container_cloud_workspaces = container_cloud_workspaces.clone();
    settings.workspace_root = resolved_workspace_root.to_string_lossy().to_string();

    if let Some(language) = payload.language.as_deref().map(str::trim) {
        if !language.is_empty() {
            settings.language = language.to_string();
        }
    }
    if let Some(llm) = payload.llm.clone() {
        settings.llm = Some(llm);
    }
    if let Some(remote_gateway) = payload.remote_gateway {
        settings.remote_gateway = remote_gateway;
    }
    settings.updated_at = now_ts();
    save_desktop_settings(&settings_path, &settings).map_err(internal_error)?;

    let next_container_roots = settings.container_roots.clone();
    let next_workspace_root = settings.workspace_root.clone();
    let next_language = settings.language.clone();
    let llm_update = settings.llm.clone();
    let updated_config = state
        .config_store
        .update(move |config| {
            if let Some(ref llm) = llm_update {
                config.llm = llm.clone();
            }
            config.workspace.root = next_workspace_root.clone();
            config.workspace.container_roots = next_container_roots.clone();
            if !next_language.trim().is_empty() {
                if !config
                    .i18n
                    .supported_languages
                    .iter()
                    .any(|value| value == &next_language)
                {
                    config.i18n.supported_languages.push(next_language.clone());
                }
                config.i18n.default_language = next_language.clone();
            }
        })
        .await
        .map_err(|err| bad_request(err.to_string()))?;
    state
        .workspace
        .set_container_roots(settings.container_roots.clone());

    let seed_statuses = desktop_seed_manager().container_seed_statuses().await;
    Ok(Json(
        json!({ "data": build_settings_payload(&updated_config, &settings, &seed_statuses) }),
    ))
}

async fn desktop_seed_start(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<DesktopSeedStartRequest>,
) -> Result<Json<Value>, Response> {
    let (settings_path, app_dir, default_workspace_root) =
        resolve_desktop_paths().map_err(bad_request)?;
    let user_id = resolve_desktop_user_id();
    let mut settings = load_desktop_settings(&settings_path).map_err(internal_error)?;
    let resolved_workspace_root =
        resolve_desktop_workspace_root(&settings, &default_workspace_root, &app_dir);
    settings.container_roots = normalize_desktop_container_roots(
        &settings.container_roots,
        &resolved_workspace_root,
        &user_id,
        &app_dir,
    );
    settings.container_cloud_workspaces =
        normalize_desktop_container_cloud_workspaces(&settings.container_cloud_workspaces);
    settings
        .container_cloud_workspaces
        .retain(|container_id, _| settings.container_roots.contains_key(container_id));

    let access_token = payload.access_token.trim().to_string();
    if access_token.is_empty() {
        return Err(bad_request("access_token is required".to_string()));
    }

    let container_id =
        normalize_workspace_container_id(payload.container_id.unwrap_or(USER_PRIVATE_CONTAINER_ID));
    let local_root = if payload.local_root.trim().is_empty() {
        settings
            .container_roots
            .get(&container_id)
            .cloned()
            .unwrap_or_else(|| {
                build_default_container_root(&resolved_workspace_root, &user_id, container_id)
                    .to_string_lossy()
                    .to_string()
            })
    } else {
        payload.local_root.trim().to_string()
    };
    let local_root_path = resolve_workspace_path(&local_root, &app_dir);
    fs::create_dir_all(&local_root_path).map_err(|err| {
        internal_error(format!(
            "prepare seed local root failed {}: {err}",
            local_root_path.display()
        ))
    })?;

    let remote_candidate = if payload.remote_api_base.trim().is_empty() {
        settings.remote_gateway.server_base_url.clone()
    } else {
        payload.remote_api_base.clone()
    };
    let remote_api_base = normalize_remote_api_base(&remote_candidate).map_err(bad_request)?;

    let cloud_workspace_id = if payload.cloud_workspace_id.trim().is_empty() {
        settings
            .container_cloud_workspaces
            .get(&container_id)
            .cloned()
            .unwrap_or_default()
    } else {
        payload.cloud_workspace_id.trim().to_string()
    };

    let snapshot = desktop_seed_manager()
        .start_job(DesktopSeedJobConfig {
            container_id,
            local_root: local_root_path,
            cloud_workspace_id,
            remote_api_base,
            access_token,
        })
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({ "data": snapshot })))
}

async fn desktop_seed_jobs(
    Query(query): Query<DesktopSeedJobsQuery>,
) -> Result<Json<Value>, Response> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_SEED_QUERY_LIMIT)
        .clamp(1, 500);
    let items = desktop_seed_manager()
        .list_jobs(query.container_id, limit)
        .await;
    Ok(Json(json!({
        "data": {
            "total": items.len(),
            "items": items
        }
    })))
}

async fn desktop_seed_job_get(AxumPath(job_id): AxumPath<String>) -> Result<Json<Value>, Response> {
    let job_id = job_id.trim().to_string();
    if job_id.is_empty() {
        return Err(bad_request("job_id is required".to_string()));
    }
    let snapshot = desktop_seed_manager()
        .get_job(&job_id)
        .await
        .ok_or_else(|| bad_request(format!("seed job not found: {job_id}")))?;
    Ok(Json(json!({ "data": snapshot })))
}

async fn desktop_seed_control(
    Json(payload): Json<DesktopSeedControlRequest>,
) -> Result<Json<Value>, Response> {
    let job_id = payload.job_id.trim().to_string();
    let action = payload.action.trim().to_string();
    if job_id.is_empty() {
        return Err(bad_request("job_id is required".to_string()));
    }
    if action.is_empty() {
        return Err(bad_request("action is required".to_string()));
    }
    let snapshot = desktop_seed_manager()
        .control_job(&job_id, &action)
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({ "data": snapshot })))
}

fn build_settings_payload(
    config: &Config,
    settings: &DesktopSettingsFile,
    seed_statuses: &HashMap<i32, String>,
) -> Value {
    let workspace_root = {
        let trimmed = settings.workspace_root.trim();
        if trimmed.is_empty() {
            config.workspace.root.clone()
        } else {
            trimmed.to_string()
        }
    };

    let mut container_roots = settings
        .container_roots
        .iter()
        .map(|(container_id, root)| DesktopContainerRootItem {
            container_id: *container_id,
            root: root.clone(),
        })
        .collect::<Vec<_>>();
    container_roots.sort_by_key(|item| item.container_id);

    let mut container_mounts = settings
        .container_roots
        .iter()
        .map(|(container_id, root)| DesktopContainerMountItem {
            container_id: *container_id,
            root: root.clone(),
            cloud_workspace_id: settings
                .container_cloud_workspaces
                .get(container_id)
                .cloned()
                .unwrap_or_default(),
            seed_status: seed_statuses
                .get(container_id)
                .cloned()
                .unwrap_or_else(|| "idle".to_string()),
        })
        .collect::<Vec<_>>();
    container_mounts.sort_by_key(|item| item.container_id);

    let language = if settings.language.trim().is_empty() {
        config.i18n.default_language.clone()
    } else {
        settings.language.clone()
    };
    let llm = settings.llm.clone().unwrap_or_else(|| config.llm.clone());

    json!({
        "workspace_root": workspace_root,
        "container_roots": container_roots,
        "container_mounts": container_mounts,
        "language": language,
        "supported_languages": config.i18n.supported_languages,
        "llm": llm,
        "remote_gateway": settings.remote_gateway,
        "updated_at": settings.updated_at,
    })
}

async fn run_seed_job(
    client: reqwest::Client,
    config: DesktopSeedJobConfig,
    job: Arc<DesktopSeedJobHandle>,
) {
    if let Err(err) = run_seed_job_inner(client, config, job.clone()).await {
        if err == "SEED_CANCELED" {
            finalize_seed_job(
                &job,
                "canceled",
                "canceled",
                "seed job canceled".to_string(),
            )
            .await;
            return;
        }
        finalize_seed_job(&job, "failed", "failed", err).await;
    }
}

async fn run_seed_job_inner(
    client: reqwest::Client,
    config: DesktopSeedJobConfig,
    job: Arc<DesktopSeedJobHandle>,
) -> Result<(), String> {
    update_seed_job_stage(&job, "discovering", Some(1.0), None).await;
    wait_if_seed_paused_or_canceled(&job).await?;

    let local_root_for_scan = config.local_root.clone();
    let scan_result = tokio::task::spawn_blocking(move || collect_seed_files(local_root_for_scan))
        .await
        .map_err(|err| format!("scan worker failed: {err}"))??;
    let (files, total_bytes) = scan_result;

    {
        let mut snapshot = job.snapshot.write().await;
        snapshot.stage = "indexing".to_string();
        snapshot.progress.total_files = files.len();
        snapshot.progress.total_bytes = total_bytes;
        snapshot.progress.percent = 8.0;
        snapshot.updated_at = now_ts();
    }
    wait_if_seed_paused_or_canceled(&job).await?;

    update_seed_job_stage(&job, "uploading", Some(10.0), None).await;
    let upload_started = Instant::now();
    let mut processed_files = 0usize;
    let mut processed_bytes = 0u64;

    for file in files {
        wait_if_seed_paused_or_canceled(&job).await?;
        let current_item = file.relative_path.clone();
        update_seed_job_stage(&job, "uploading", None, Some(current_item.clone())).await;
        upload_single_file_to_cloud(&client, &config, &file).await?;

        processed_files += 1;
        processed_bytes = processed_bytes.saturating_add(file.size);
        let elapsed = upload_started.elapsed().as_secs_f64().max(0.001);
        let speed = processed_bytes as f64 / elapsed;
        let total = total_bytes.max(1);
        let remaining = total_bytes.saturating_sub(processed_bytes);
        let percent = 10.0 + (processed_bytes as f64 / total as f64).clamp(0.0, 1.0) * 85.0;
        let eta_seconds = if speed > 1.0 {
            Some((remaining as f64 / speed).ceil() as u64)
        } else {
            None
        };
        {
            let mut snapshot = job.snapshot.write().await;
            snapshot.progress.percent = percent;
            snapshot.progress.processed_files = processed_files;
            snapshot.progress.processed_bytes = processed_bytes;
            snapshot.progress.speed_bps = speed;
            snapshot.progress.eta_seconds = eta_seconds;
            snapshot.current_item = current_item;
            snapshot.updated_at = now_ts();
        }
    }

    update_seed_job_stage(&job, "verifying", Some(97.0), None).await;
    wait_if_seed_paused_or_canceled(&job).await?;

    update_seed_job_stage(&job, "committing", Some(99.0), None).await;
    wait_if_seed_paused_or_canceled(&job).await?;

    finalize_seed_job(&job, "done", "done", String::new()).await;
    Ok(())
}

#[derive(Debug, Clone)]
struct SeedUploadFile {
    absolute_path: PathBuf,
    relative_path: String,
    size: u64,
}

fn collect_seed_files(local_root: PathBuf) -> Result<(Vec<SeedUploadFile>, u64), String> {
    if !local_root.exists() {
        return Err(format!(
            "seed local root not found: {}",
            local_root.display()
        ));
    }
    if !local_root.is_dir() {
        return Err(format!(
            "seed local root is not a directory: {}",
            local_root.display()
        ));
    }
    let mut files = Vec::new();
    let mut total_bytes = 0u64;
    for entry in WalkDir::new(&local_root).follow_links(false) {
        let entry = entry.map_err(|err| format!("scan local root failed: {err}"))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path().to_path_buf();
        let metadata = fs::metadata(&path)
            .map_err(|err| format!("read file metadata failed {}: {err}", path.display()))?;
        let relative = path
            .strip_prefix(&local_root)
            .map_err(|err| format!("build relative path failed {}: {err}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        if relative.trim().is_empty() {
            continue;
        }
        let size = metadata.len();
        total_bytes = total_bytes.saturating_add(size);
        files.push(SeedUploadFile {
            absolute_path: path,
            relative_path: relative,
            size,
        });
    }
    Ok((files, total_bytes))
}

async fn upload_single_file_to_cloud(
    client: &reqwest::Client,
    config: &DesktopSeedJobConfig,
    file: &SeedUploadFile,
) -> Result<(), String> {
    let url = format!(
        "{}/workspace/upload",
        config.remote_api_base.trim_end_matches('/')
    );
    let base_path = if config.cloud_workspace_id.trim().is_empty() {
        ".".to_string()
    } else {
        config.cloud_workspace_id.trim().to_string()
    };
    let filename = file
        .absolute_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("upload")
        .to_string();
    let opened = tokio::fs::File::open(&file.absolute_path)
        .await
        .map_err(|err| {
            format!(
                "open local file failed {}: {err}",
                file.absolute_path.display()
            )
        })?;
    let stream = ReaderStream::new(opened);
    let body = reqwest::Body::wrap_stream(stream);
    let part = Part::stream_with_length(body, file.size).file_name(filename);
    let form = Form::new()
        .text("container_id", config.container_id.to_string())
        .text("path", base_path)
        .text("relative_paths", file.relative_path.clone())
        .part("files", part);
    let response = client
        .post(url)
        .bearer_auth(config.access_token.trim())
        .multipart(form)
        .send()
        .await
        .map_err(|err| format!("upload file to cloud failed: {err}"))?;
    if response.status().is_success() {
        return Ok(());
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let detail = parse_remote_error_message(&body).unwrap_or_else(|| truncate_text(&body, 512));
    Err(format!("upload file failed ({status}): {detail}"))
}

fn parse_remote_error_message(body: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(body).ok()?;
    if let Some(message) = parsed.get("message").and_then(Value::as_str) {
        let cleaned = message.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    if let Some(message) = parsed
        .get("error")
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
    {
        let cleaned = message.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    None
}

fn truncate_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut output = text[..end].to_string();
    output.push_str("...");
    output
}

async fn wait_if_seed_paused_or_canceled(job: &DesktopSeedJobHandle) -> Result<(), String> {
    loop {
        if job.canceled.load(Ordering::SeqCst) {
            return Err("SEED_CANCELED".to_string());
        }
        if !job.paused.load(Ordering::SeqCst) {
            return Ok(());
        }
        sleep(Duration::from_millis(300)).await;
    }
}

async fn update_seed_job_stage(
    job: &DesktopSeedJobHandle,
    stage: &str,
    percent: Option<f64>,
    current_item: Option<String>,
) {
    let mut snapshot = job.snapshot.write().await;
    snapshot.stage = stage.to_string();
    if snapshot.status != "paused" {
        snapshot.status = "running".to_string();
    }
    if let Some(percent) = percent {
        snapshot.progress.percent = percent;
    }
    if let Some(item) = current_item {
        snapshot.current_item = item;
    }
    snapshot.updated_at = now_ts();
}

async fn finalize_seed_job(
    job: &DesktopSeedJobHandle,
    status: &str,
    stage: &str,
    error_message: String,
) {
    let mut snapshot = job.snapshot.write().await;
    snapshot.status = status.to_string();
    snapshot.stage = stage.to_string();
    snapshot.error = error_message;
    snapshot.current_item.clear();
    if status == "done" {
        snapshot.progress.percent = 100.0;
        snapshot.progress.eta_seconds = Some(0);
    }
    snapshot.finished_at = Some(now_ts());
    snapshot.updated_at = now_ts();
}

fn is_seed_job_terminal_status(status: &str) -> bool {
    matches!(status, "done" | "failed" | "canceled")
}

fn is_seed_job_active_status(status: &str) -> bool {
    matches!(status, "running" | "paused")
}

fn normalize_remote_api_base(raw: &str) -> Result<String, String> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return Err("remote api base is required".to_string());
    }
    let candidate = if cleaned.starts_with("http://") || cleaned.starts_with("https://") {
        cleaned.to_string()
    } else {
        format!("http://{cleaned}")
    };
    let mut url = Url::parse(&candidate)
        .map_err(|err| format!("invalid remote api base: {cleaned}: {err}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("remote api base must use http/https".to_string());
    }
    let mut path = url.path().trim_end_matches('/').to_string();
    if path.is_empty() || path == "/" {
        path = "/wunder".to_string();
    } else if !path.ends_with("/wunder") {
        path = format!("{path}/wunder");
    }
    if !path.starts_with('/') {
        path = format!("/{path}");
    }
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn resolve_desktop_paths() -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let settings_path = std::env::var(DESKTOP_SETTINGS_PATH_ENV)
        .map(PathBuf::from)
        .map_err(|_| format!("missing {DESKTOP_SETTINGS_PATH_ENV}"))?;
    let app_dir = std::env::var(DESKTOP_APP_DIR_ENV)
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(PathBuf::from))
                .ok_or(std::env::VarError::NotPresent)
        })
        .map_err(|_| format!("missing {DESKTOP_APP_DIR_ENV}"))?;
    let default_workspace_root = std::env::var(DESKTOP_DEFAULT_WORKSPACE_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| app_dir.join("WUNDER_WORK"));
    Ok((settings_path, app_dir, default_workspace_root))
}

fn load_desktop_settings(path: &Path) -> Result<DesktopSettingsFile, String> {
    if !path.exists() {
        return Ok(DesktopSettingsFile::default());
    }
    let text = fs::read_to_string(path)
        .map_err(|err| format!("read desktop settings failed {}: {err}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(DesktopSettingsFile::default());
    }
    match serde_json::from_str::<DesktopSettingsFile>(&text) {
        Ok(settings) => Ok(settings),
        Err(primary_err) => {
            let backup_path = desktop_settings_backup_path(path);
            if backup_path.exists() {
                if let Ok(backup_text) = fs::read_to_string(&backup_path) {
                    if !backup_text.trim().is_empty() {
                        if let Ok(settings) =
                            serde_json::from_str::<DesktopSettingsFile>(&backup_text)
                        {
                            return Ok(settings);
                        }
                    }
                }
            }
            Err(format!(
                "parse desktop settings failed {}: {primary_err}",
                path.display()
            ))
        }
    }
}

fn save_desktop_settings(path: &Path, settings: &DesktopSettingsFile) -> Result<(), String> {
    let serialized = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("serialize desktop settings failed: {err}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "create desktop settings dir failed {}: {err}",
                parent.display()
            )
        })?;
    }
    let backup_path = desktop_settings_backup_path(path);
    let temp_path = desktop_settings_temp_path(path);
    fs::write(&temp_path, &serialized).map_err(|err| {
        format!(
            "write desktop settings temp failed {}: {err}",
            temp_path.display()
        )
    })?;
    if path.exists() {
        fs::copy(path, &backup_path).map_err(|err| {
            format!(
                "backup desktop settings failed {} -> {}: {err}",
                path.display(),
                backup_path.display()
            )
        })?;
    }
    if let Err(initial_err) = fs::rename(&temp_path, path) {
        if let Err(remove_err) = fs::remove_file(path) {
            if remove_err.kind() != std::io::ErrorKind::NotFound {
                return Err(format!(
                    "remove old desktop settings failed {}: {remove_err}",
                    path.display()
                ));
            }
        }
        fs::rename(&temp_path, path).map_err(|rename_err| {
            format!(
                "replace desktop settings failed {}: {rename_err} (initial rename error: {initial_err})",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn desktop_settings_backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

fn desktop_settings_temp_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

fn resolve_desktop_list_path(
    raw_path: Option<&str>,
    settings: &DesktopSettingsFile,
    default_workspace_root: &Path,
    app_dir: &Path,
) -> PathBuf {
    let fallback = settings
        .container_roots
        .get(&USER_PRIVATE_CONTAINER_ID)
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            let trimmed = settings.workspace_root.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| default_workspace_root.to_string_lossy().to_string());
    let cleaned = raw_path.map(str::trim).filter(|value| !value.is_empty());
    let selected = cleaned.unwrap_or(fallback.as_str());
    resolve_workspace_path(selected, app_dir)
}

fn list_desktop_directory_roots() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        let mut roots = Vec::new();
        for letter in b'A'..=b'Z' {
            let root = format!("{}:\\", letter as char);
            if Path::new(&root).exists() {
                roots.push(root);
            }
        }
        roots
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec!["/".to_string()]
    }
}

fn normalize_desktop_container_roots(
    source: &HashMap<i32, String>,
    workspace_root: &Path,
    user_id: &str,
    app_dir: &Path,
) -> HashMap<i32, String> {
    let normalized_user_id = sanitize_workspace_scope(user_id);
    let workspace_root_cmp = normalize_path_for_compare(workspace_root);
    let mut seen_paths = HashSet::new();
    seen_paths.insert(workspace_root_cmp);
    for container_id in USER_PRIVATE_CONTAINER_ID..=MAX_SANDBOX_CONTAINER_ID {
        let default_root =
            build_default_container_root(workspace_root, &normalized_user_id, container_id);
        seen_paths.insert(normalize_path_for_compare(&default_root));
    }

    let mut explicit = HashMap::new();
    for (container_id, root) in source {
        let normalized_id = normalize_workspace_container_id(*container_id);
        let trimmed = root.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resolved = resolve_workspace_path(trimmed, app_dir);
        let resolved_cmp = normalize_path_for_compare(&resolved);
        if resolved_cmp.is_empty() || seen_paths.contains(&resolved_cmp) {
            continue;
        }
        seen_paths.insert(resolved_cmp);
        explicit.insert(normalized_id, resolved);
    }

    let mut output = HashMap::new();
    for container_id in USER_PRIVATE_CONTAINER_ID..=MAX_SANDBOX_CONTAINER_ID {
        let root = explicit.remove(&container_id).unwrap_or_else(|| {
            build_default_container_root(workspace_root, &normalized_user_id, container_id)
        });
        output.insert(container_id, root.to_string_lossy().to_string());
    }
    output
}

fn normalize_desktop_container_cloud_workspaces(
    source: &HashMap<i32, String>,
) -> HashMap<i32, String> {
    let mut output = HashMap::new();
    for (container_id, cloud_workspace_id) in source {
        let normalized_id = normalize_workspace_container_id(*container_id);
        let cleaned = cloud_workspace_id.trim();
        if cleaned.is_empty() {
            continue;
        }
        output.insert(normalized_id, cleaned.to_string());
    }
    output
}

fn ensure_container_root_dirs(container_roots: &HashMap<i32, String>) -> Result<(), String> {
    for root in container_roots.values() {
        let trimmed = root.trim();
        if trimmed.is_empty() {
            continue;
        }
        fs::create_dir_all(trimmed)
            .map_err(|err| format!("create desktop container workspace failed {trimmed}: {err}"))?;
    }
    Ok(())
}

fn resolve_workspace_path(raw: &str, app_dir: &Path) -> PathBuf {
    let path = PathBuf::from(raw.trim());
    if path.is_absolute() {
        path
    } else {
        app_dir.join(path)
    }
}

fn resolve_desktop_workspace_root(
    settings: &DesktopSettingsFile,
    default_workspace_root: &Path,
    app_dir: &Path,
) -> PathBuf {
    let candidate = settings.workspace_root.trim().to_string();
    if !candidate.is_empty() {
        return resolve_workspace_path(&candidate, app_dir);
    }
    if let Some(legacy_user_root) = settings
        .container_roots
        .get(&USER_PRIVATE_CONTAINER_ID)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return resolve_workspace_path(legacy_user_root, app_dir);
    }
    default_workspace_root.to_path_buf()
}

fn resolve_desktop_user_id() -> String {
    let raw = std::env::var(DESKTOP_USER_ID_ENV).unwrap_or_else(|_| "desktop_user".to_string());
    sanitize_workspace_scope(raw.trim())
}

fn sanitize_workspace_scope(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.trim().is_empty() {
        "desktop_user".to_string()
    } else {
        output
    }
}

fn build_default_container_root(
    workspace_root: &Path,
    user_id: &str,
    container_id: i32,
) -> PathBuf {
    if container_id == USER_PRIVATE_CONTAINER_ID {
        return workspace_root.join(user_id);
    }
    workspace_root.join(format!("{user_id}__c__{container_id}"))
}

fn normalize_path_for_compare(path: &Path) -> String {
    let mut normalized = path.to_string_lossy().replace('\\', "/");
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    #[cfg(target_os = "windows")]
    {
        normalized.make_ascii_lowercase();
    }
    normalized
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn bad_request(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "BAD_REQUEST",
            "message": message,
        })),
    )
        .into_response()
}

fn internal_error(message: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "DESKTOP_SETTINGS_ERROR",
            "message": message,
        })),
    )
        .into_response()
}
