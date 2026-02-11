// desktop API：管理本地桌面设置（容器目录映射、模型配置、语言、远程接入预留）。
use crate::config::{Config, LlmConfig};
use crate::state::AppState;
use crate::storage::normalize_sandbox_container_id;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const DESKTOP_SETTINGS_PATH_ENV: &str = "WUNDER_DESKTOP_SETTINGS_PATH";
const DESKTOP_APP_DIR_ENV: &str = "WUNDER_DESKTOP_APP_DIR";
const DESKTOP_DEFAULT_WORKSPACE_ROOT_ENV: &str = "WUNDER_DESKTOP_DEFAULT_WORKSPACE_ROOT";

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/wunder/desktop/settings",
        get(desktop_settings_get).put(desktop_settings_update),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DesktopRemoteGatewaySettings {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    server_base_url: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    role_name: String,
    #[serde(default)]
    use_remote_sandbox: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopSettingsFile {
    workspace_root: String,
    desktop_token: String,
    #[serde(default)]
    container_roots: HashMap<i32, String>,
    #[serde(default)]
    language: String,
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
            language: String::new(),
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

#[derive(Debug, Clone, Deserialize)]
struct DesktopContainerRootInput {
    container_id: i32,
    root: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct DesktopSettingsUpdateRequest {
    #[serde(default)]
    workspace_root: Option<String>,
    #[serde(default)]
    container_roots: Option<Vec<DesktopContainerRootInput>>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    llm: Option<LlmConfig>,
    #[serde(default)]
    remote_gateway: Option<DesktopRemoteGatewaySettings>,
}

async fn desktop_settings_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let (settings_path, app_dir, default_workspace_root) =
        resolve_desktop_paths().map_err(bad_request)?;
    let mut settings = load_desktop_settings(&settings_path).map_err(internal_error)?;
    settings.container_roots = normalize_desktop_container_roots(
        &settings.container_roots,
        &default_workspace_root,
        &app_dir,
    );
    settings.workspace_root = settings
        .container_roots
        .get(&1)
        .cloned()
        .unwrap_or_else(|| default_workspace_root.to_string_lossy().to_string());

    let config = state.config_store.get().await;
    Ok(Json(
        json!({ "data": build_settings_payload(&config, &settings) }),
    ))
}

async fn desktop_settings_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DesktopSettingsUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let (settings_path, app_dir, default_workspace_root) =
        resolve_desktop_paths().map_err(bad_request)?;
    let mut settings = load_desktop_settings(&settings_path).map_err(internal_error)?;

    let mut container_roots = if let Some(items) = payload.container_roots.as_ref() {
        let mut map = HashMap::new();
        for item in items {
            let container_id = normalize_sandbox_container_id(item.container_id);
            let trimmed = item.root.trim();
            if trimmed.is_empty() {
                continue;
            }
            map.insert(container_id, trimmed.to_string());
        }
        map
    } else {
        settings.container_roots.clone()
    };

    if let Some(workspace_root) = payload.workspace_root.as_deref().map(str::trim) {
        if !workspace_root.is_empty() {
            container_roots.insert(1, workspace_root.to_string());
        }
    }

    container_roots =
        normalize_desktop_container_roots(&container_roots, &default_workspace_root, &app_dir);
    ensure_container_root_dirs(&container_roots).map_err(internal_error)?;

    settings.container_roots = container_roots.clone();
    settings.workspace_root = container_roots
        .get(&1)
        .cloned()
        .unwrap_or_else(|| default_workspace_root.to_string_lossy().to_string());

    if let Some(language) = payload.language.as_deref().map(str::trim) {
        if !language.is_empty() {
            settings.language = language.to_string();
        }
    }

    if let Some(remote_gateway) = payload.remote_gateway {
        settings.remote_gateway = remote_gateway;
    }

    settings.updated_at = now_ts();
    save_desktop_settings(&settings_path, &settings).map_err(internal_error)?;

    let next_container_roots = settings.container_roots.clone();
    let next_language = settings.language.clone();
    let llm_update = payload.llm.clone();
    let updated_config = state
        .config_store
        .update(move |config| {
            if let Some(ref llm) = llm_update {
                config.llm = llm.clone();
            }
            if let Some(root) = next_container_roots.get(&1) {
                config.workspace.root = root.clone();
            }
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

    Ok(Json(
        json!({ "data": build_settings_payload(&updated_config, &settings) }),
    ))
}

fn build_settings_payload(config: &Config, settings: &DesktopSettingsFile) -> Value {
    let workspace_root = settings
        .container_roots
        .get(&1)
        .cloned()
        .or_else(|| {
            let trimmed = settings.workspace_root.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| config.workspace.root.clone());

    let mut container_roots = settings
        .container_roots
        .iter()
        .map(|(container_id, root)| DesktopContainerRootItem {
            container_id: *container_id,
            root: root.clone(),
        })
        .collect::<Vec<_>>();
    container_roots.sort_by_key(|item| item.container_id);

    let language = if settings.language.trim().is_empty() {
        config.i18n.default_language.clone()
    } else {
        settings.language.clone()
    };

    json!({
        "workspace_root": workspace_root,
        "container_roots": container_roots,
        "language": language,
        "supported_languages": config.i18n.supported_languages,
        "llm": config.llm,
        "remote_gateway": settings.remote_gateway,
        "updated_at": settings.updated_at,
    })
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
    serde_json::from_str::<DesktopSettingsFile>(&text)
        .map_err(|err| format!("parse desktop settings failed {}: {err}", path.display()))
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
    fs::write(path, serialized)
        .map_err(|err| format!("write desktop settings failed {}: {err}", path.display()))
}

fn normalize_desktop_container_roots(
    source: &HashMap<i32, String>,
    default_workspace_root: &Path,
    app_dir: &Path,
) -> HashMap<i32, String> {
    let mut output = HashMap::new();
    output.insert(1, default_workspace_root.to_string_lossy().to_string());

    for (container_id, root) in source {
        let normalized_id = normalize_sandbox_container_id(*container_id);
        if normalized_id == 1 {
            continue;
        }
        let trimmed = root.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resolved = resolve_workspace_path(trimmed, app_dir);
        output.insert(normalized_id, resolved.to_string_lossy().to_string());
    }

    if let Some(raw_root) = source.get(&1).map(String::as_str) {
        let resolved = resolve_workspace_path(raw_root, app_dir);
        output.insert(1, resolved.to_string_lossy().to_string());
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
