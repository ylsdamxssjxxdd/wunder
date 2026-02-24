use crate::args::DesktopArgs;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;
use url::Url;
use wunder_server::config::{Config, LlmConfig};
use wunder_server::config_store::ConfigStore;
use wunder_server::state::{AppState, AppStateInitOptions};
use wunder_server::storage::UserTokenRecord;
use wunder_server::user_store::UserStore;

pub const DESKTOP_DEFAULT_USER_ID: &str = "desktop_user";

#[derive(Clone)]
pub struct DesktopRuntime {
    pub state: Arc<AppState>,
    pub app_dir: PathBuf,
    pub temp_root: PathBuf,
    pub settings_path: PathBuf,
    pub workspace_root: PathBuf,
    pub frontend_root: Option<PathBuf>,
    pub repo_root: PathBuf,
    pub user_id: String,
    pub desktop_token: String,
    pub remote_gateway: DesktopRemoteGatewaySettings,
    pub remote_api_base: Option<String>,
    pub remote_ws_base: Option<String>,
    pub remote_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DesktopRemoteGatewaySettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub server_base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopSettings {
    pub workspace_root: String,
    pub desktop_token: String,
    #[serde(default)]
    pub container_roots: HashMap<i32, String>,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub llm: Option<LlmConfig>,
    #[serde(default)]
    pub remote_gateway: DesktopRemoteGatewaySettings,
    pub updated_at: f64,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            workspace_root: String::new(),
            desktop_token: uuid::Uuid::new_v4().simple().to_string(),
            container_roots: HashMap::new(),
            language: String::new(),
            llm: None,
            remote_gateway: DesktopRemoteGatewaySettings::default(),
            updated_at: now_ts(),
        }
    }
}

impl DesktopRuntime {
    pub async fn init(args: &DesktopArgs) -> Result<Self> {
        let app_dir = resolve_app_dir()?;
        let repo_root = resolve_repo_root(&app_dir);
        let temp_root = resolve_temp_root(args.temp_root.as_deref(), &app_dir);
        ensure_runtime_dirs(&temp_root)?;

        let settings_path = temp_root.join("config/desktop.settings.json");
        let mut settings = load_desktop_settings(&settings_path)?;

        let workspace_root = resolve_workspace_root(
            args.workspace.as_deref(),
            &settings.workspace_root,
            &app_dir,
        );
        fs::create_dir_all(&workspace_root).with_context(|| {
            format!(
                "create workspace root failed: {}",
                workspace_root.to_string_lossy()
            )
        })?;

        if settings.desktop_token.trim().is_empty() {
            settings.desktop_token = uuid::Uuid::new_v4().simple().to_string();
        }
        settings.workspace_root = workspace_root.to_string_lossy().to_string();
        settings.container_roots =
            normalize_desktop_container_roots(&settings.container_roots, &workspace_root, &app_dir);
        ensure_container_root_dirs(&settings.container_roots)?;
        settings.updated_at = now_ts();
        save_desktop_settings(&settings_path, &settings)?;

        let base_config = prepare_base_config_path(&repo_root, &temp_root)?;
        let override_path = temp_root.join("config/wunder.override.yaml");
        let i18n_path = repo_root.join("config/i18n.messages.json");
        let skill_runner = repo_root.join("scripts/skill_runner.py");
        let user_tools_root = temp_root.join("user_tools");
        let vector_root = temp_root.join("vector_knowledge");

        set_env_path("WUNDER_CONFIG_PATH", &base_config);
        set_env_path("WUNDER_CONFIG_OVERRIDE_PATH", &override_path);
        set_env_path_if_exists("WUNDER_I18N_MESSAGES_PATH", &i18n_path);
        set_env_prompts_root_if_unset(&repo_root);
        set_env_path_if_exists("WUNDER_SKILL_RUNNER_PATH", &skill_runner);
        set_env_path("WUNDER_USER_TOOLS_ROOT", &user_tools_root);
        set_env_path("WUNDER_VECTOR_KNOWLEDGE_ROOT", &vector_root);
        set_env_path("WUNDER_DESKTOP_SETTINGS_PATH", &settings_path);
        set_env_path("WUNDER_DESKTOP_APP_DIR", &app_dir);
        set_env_path("WUNDER_DESKTOP_DEFAULT_WORKSPACE_ROOT", &workspace_root);
        std::env::set_var("WUNDER_WORKSPACE_SINGLE_ROOT", "1");

        let user_id = normalize_user_id(args.user.as_deref());
        let desktop_token = settings.desktop_token.clone();

        let config_store = ConfigStore::new(override_path);
        let workspace_for_update = workspace_root.clone();
        let temp_root_for_update = temp_root.clone();
        let repo_root_for_update = repo_root.clone();
        let token_for_update = desktop_token.clone();
        let container_roots_for_update = settings.container_roots.clone();
        let language_for_update = settings.language.clone();
        let llm_for_update = settings.llm.clone();
        let config = config_store
            .update(move |config| {
                apply_desktop_defaults(
                    config,
                    &workspace_for_update,
                    &temp_root_for_update,
                    &repo_root_for_update,
                    DesktopDefaultsInput {
                        desktop_token: &token_for_update,
                        container_roots: &container_roots_for_update,
                        language: &language_for_update,
                        llm: llm_for_update.as_ref(),
                    },
                );
            })
            .await
            .context("apply desktop runtime config failed")?;

        let state = Arc::new(
            AppState::new_with_options(
                config_store.clone(),
                config.clone(),
                AppStateInitOptions::desktop_default(),
            )
            .context("initialize desktop state failed")?,
        );
        state.lsp_manager.sync_with_config(&config).await;
        ensure_desktop_identity(state.as_ref(), &user_id, &desktop_token)?;

        let remote_gateway = settings.remote_gateway.clone();
        let (remote_api_base, remote_ws_base, remote_error) =
            resolve_remote_endpoints(&remote_gateway);

        let frontend_root =
            resolve_frontend_root(args.frontend_root.as_deref(), &repo_root, &app_dir);

        Ok(Self {
            state,
            app_dir,
            temp_root,
            settings_path,
            workspace_root,
            frontend_root,
            repo_root,
            user_id,
            desktop_token,
            remote_gateway,
            remote_api_base,
            remote_ws_base,
            remote_error,
        })
    }
}

fn resolve_app_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("resolve current exe path failed")?;
    exe.parent()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("resolve app dir failed from exe path"))
}

fn resolve_repo_root(app_dir: &Path) -> PathBuf {
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if candidate.join("config/wunder.yaml").exists() {
        candidate
    } else {
        app_dir.to_path_buf()
    }
}

fn resolve_temp_root(temp_root: Option<&Path>, app_dir: &Path) -> PathBuf {
    match temp_root {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => app_dir.join(path),
        None => app_dir.join("WUNDER_TEMPD"),
    }
}

fn resolve_workspace_root(
    arg_workspace: Option<&Path>,
    settings_workspace: &str,
    app_dir: &Path,
) -> PathBuf {
    if let Some(path) = arg_workspace {
        return if path.is_absolute() {
            path.to_path_buf()
        } else {
            app_dir.join(path)
        };
    }

    let raw = settings_workspace.trim();
    if raw.is_empty() {
        return app_dir.join("WUNDER_WORK");
    }

    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        app_dir.join(path)
    }
}

fn resolve_frontend_root(
    arg_frontend_root: Option<&Path>,
    repo_root: &Path,
    app_dir: &Path,
) -> Option<PathBuf> {
    if let Some(path) = arg_frontend_root {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            app_dir.join(path)
        };
        if resolved.exists() {
            return Some(resolved);
        }
        return None;
    }

    let candidates = [
        repo_root.join("frontend/dist"),
        app_dir.join("frontend/dist"),
    ];
    candidates.into_iter().find(|candidate| candidate.exists())
}

fn ensure_runtime_dirs(temp_root: &Path) -> Result<()> {
    for dir in [
        temp_root.to_path_buf(),
        temp_root.join("config"),
        temp_root.join("logs"),
        temp_root.join("sessions"),
        temp_root.join("user_tools"),
        temp_root.join("vector_knowledge"),
    ] {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

pub(crate) fn load_desktop_settings(path: &Path) -> Result<DesktopSettings> {
    if !path.exists() {
        return Ok(DesktopSettings::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("read desktop settings failed: {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(DesktopSettings::default());
    }
    serde_json::from_str::<DesktopSettings>(&text)
        .with_context(|| format!("parse desktop settings failed: {}", path.display()))
}

pub(crate) fn save_desktop_settings(path: &Path, settings: &DesktopSettings) -> Result<()> {
    let text =
        serde_json::to_string_pretty(settings).context("serialize desktop settings failed")?;
    fs::write(path, text)
        .with_context(|| format!("write desktop settings failed: {}", path.to_string_lossy()))
}

pub(crate) fn normalize_desktop_container_roots(
    source: &HashMap<i32, String>,
    default_workspace_root: &Path,
    app_dir: &Path,
) -> HashMap<i32, String> {
    let mut output = HashMap::new();
    output.insert(1, default_workspace_root.to_string_lossy().to_string());
    for (container_id, root) in source {
        let normalized_id = wunder_server::storage::normalize_sandbox_container_id(*container_id);
        if normalized_id == 1 {
            continue;
        }
        let trimmed = root.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resolved = resolve_workspace_path_input(trimmed, app_dir);
        output.insert(normalized_id, resolved.to_string_lossy().to_string());
    }
    output
}

pub(crate) fn ensure_container_root_dirs(container_roots: &HashMap<i32, String>) -> Result<()> {
    for root in container_roots.values() {
        let trimmed = root.trim();
        if trimmed.is_empty() {
            continue;
        }
        fs::create_dir_all(trimmed)
            .with_context(|| format!("create desktop container workspace failed: {trimmed}"))?;
    }
    Ok(())
}

fn resolve_workspace_path_input(raw: &str, app_dir: &Path) -> PathBuf {
    let path = PathBuf::from(raw.trim());
    if path.is_absolute() {
        path
    } else {
        app_dir.join(path)
    }
}

fn resolve_remote_endpoints(
    remote_gateway: &DesktopRemoteGatewaySettings,
) -> (Option<String>, Option<String>, Option<String>) {
    if !remote_gateway.enabled {
        return (None, None, None);
    }

    match normalize_remote_api_base_url(&remote_gateway.server_base_url).and_then(|api_base_url| {
        let api_base = api_base_url.as_str().trim_end_matches('/').to_string();
        let ws_base = build_remote_ws_base(&api_base_url)?;
        Ok((api_base, ws_base))
    }) {
        Ok((api_base, ws_base)) => (Some(api_base), Some(ws_base), None),
        Err(err) => {
            let message = err.to_string();
            warn!("desktop remote gateway endpoint invalid: {message}");
            (None, None, Some(message))
        }
    }
}

fn normalize_remote_api_base_url(raw: &str) -> Result<Url> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("remote gateway server_base_url is required"));
    }

    let candidate = if cleaned.starts_with("http://") || cleaned.starts_with("https://") {
        cleaned.to_string()
    } else {
        format!("http://{cleaned}")
    };

    let mut url = Url::parse(&candidate)
        .with_context(|| format!("invalid remote gateway server_base_url: {cleaned}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(anyhow!(
            "remote gateway server_base_url must use http/https, got {}",
            url.scheme()
        ));
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
    Ok(url)
}

fn build_remote_ws_base(api_base_url: &Url) -> Result<String> {
    let mut ws_url = api_base_url.clone();
    let ws_scheme = match api_base_url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => {
            return Err(anyhow!(
                "remote gateway server_base_url must use http/https, got {other}"
            ))
        }
    };
    ws_url
        .set_scheme(ws_scheme)
        .map_err(|_| anyhow!("set websocket scheme failed"))?;

    let mut path = api_base_url.path().trim_end_matches('/').to_string();
    if !path.ends_with("/wunder") {
        if path.is_empty() || path == "/" {
            path = "/wunder".to_string();
        } else {
            path = format!("{path}/wunder");
        }
    }
    ws_url.set_path(&format!("{path}/chat/ws"));
    Ok(ws_url.to_string())
}

fn set_env_path(key: &str, value: &Path) {
    std::env::set_var(key, value.to_string_lossy().to_string());
}

fn set_env_path_if_exists(key: &str, value: &Path) {
    if value.exists() {
        set_env_path(key, value);
    }
}

fn set_env_prompts_root_if_unset(repo_root: &Path) {
    if std::env::var("WUNDER_PROMPTS_ROOT")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return;
    }
    if repo_root.join("prompts").is_dir() {
        set_env_path("WUNDER_PROMPTS_ROOT", repo_root);
    }
}

fn prepare_base_config_path(repo_root: &Path, temp_root: &Path) -> Result<PathBuf> {
    let repo_config = repo_root.join("config/wunder.yaml");
    if repo_config.exists() {
        return Ok(repo_config);
    }
    let generated = temp_root.join("config/wunder.base.yaml");
    ensure_generated_base_config(&generated)?;
    Ok(generated)
}

fn ensure_generated_base_config(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("invalid generated base config path: {}", path.display()))?;
    fs::create_dir_all(parent)?;
    let mut config = Config::default();
    config.server.mode = "desktop".to_string();
    let content =
        serde_yaml::to_string(&config).context("serialize generated desktop base config failed")?;
    fs::write(path, content).with_context(|| {
        format!(
            "write generated desktop base config failed: {}",
            path.to_string_lossy()
        )
    })?;
    Ok(())
}

struct DesktopDefaultsInput<'a> {
    desktop_token: &'a str,
    container_roots: &'a HashMap<i32, String>,
    language: &'a str,
    llm: Option<&'a LlmConfig>,
}

fn apply_desktop_defaults(
    config: &mut Config,
    workspace_root: &Path,
    temp_root: &Path,
    repo_root: &Path,
    defaults: DesktopDefaultsInput<'_>,
) {
    config.server.mode = "desktop".to_string();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_root
        .join("wunder_desktop.sqlite3")
        .to_string_lossy()
        .to_string();
    config.workspace.root = workspace_root.to_string_lossy().to_string();
    config.workspace.container_roots = defaults.container_roots.clone();

    if !defaults.language.trim().is_empty() {
        config.i18n.default_language = defaults.language.trim().to_string();
    }

    if let Some(llm) = defaults.llm {
        config.llm = llm.clone();
    }

    config.channels.enabled = false;
    config.gateway.enabled = false;
    config.agent_queue.enabled = false;
    config.cron.enabled = false;
    config.sandbox.mode = "local".to_string();

    if !defaults.desktop_token.trim().is_empty() {
        config.security.api_key = Some(defaults.desktop_token.to_string());
    }

    let launch_skills = workspace_root.join("skills");
    let repo_skills = repo_root.join("skills");
    let mut skill_paths = vec![launch_skills, repo_skills];
    for existing in &config.skills.paths {
        if is_legacy_eva_skills_path(existing) {
            continue;
        }
        let resolved = resolve_maybe_relative_path(existing, repo_root, workspace_root);
        skill_paths.push(resolved);
    }
    config.skills.paths = dedupe_paths(skill_paths)
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();

    let mut allow_paths = config
        .security
        .allow_paths
        .iter()
        .filter(|path| !is_legacy_eva_skills_path(path))
        .cloned()
        .collect::<Vec<_>>();
    allow_paths.push(repo_root.join("skills").to_string_lossy().to_string());
    allow_paths.push(workspace_root.to_string_lossy().to_string());
    config.security.allow_paths = dedupe_strings(allow_paths);
}

fn normalize_user_id(raw: Option<&str>) -> String {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return DESKTOP_DEFAULT_USER_ID.to_string();
    };
    UserStore::normalize_user_id(raw).unwrap_or_else(|| DESKTOP_DEFAULT_USER_ID.to_string())
}

fn ensure_desktop_identity(state: &AppState, user_id: &str, desktop_token: &str) -> Result<()> {
    if let Some(mut existing) = state.user_store.get_user_by_id(user_id)? {
        let mut changed = false;
        if existing.status.trim().to_lowercase() != "active" {
            existing.status = "active".to_string();
            changed = true;
        }
        if !UserStore::is_admin(&existing) {
            existing.roles.push("admin".to_string());
            changed = true;
        }
        if changed {
            existing.updated_at = now_ts();
            state.user_store.update_user(&existing)?;
        }
    } else {
        let password = format!("wunder_desktop_{}", uuid::Uuid::new_v4().simple());
        state.user_store.create_user(
            user_id,
            None,
            &password,
            Some("A"),
            None,
            vec!["admin".to_string()],
            "active",
            false,
        )?;
    }

    if desktop_token.trim().is_empty() {
        return Ok(());
    }

    let _ = state.storage.delete_user_token(desktop_token);
    let now = now_ts();
    let record = UserTokenRecord {
        token: desktop_token.to_string(),
        user_id: user_id.to_string(),
        expires_at: now + 10.0 * 365.0 * 24.0 * 3600.0,
        created_at: now,
        last_used_at: now,
    };
    state.storage.create_user_token(&record)?;
    Ok(())
}

fn resolve_maybe_relative_path(raw: &str, repo_root: &Path, workspace_root: &Path) -> PathBuf {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return repo_root.to_path_buf();
    }
    let path = PathBuf::from(cleaned);
    if path.is_absolute() {
        return path;
    }
    let workspace_candidate = workspace_root.join(&path);
    if workspace_candidate.exists() {
        return workspace_candidate;
    }
    repo_root.join(path)
}

fn is_legacy_eva_skills_path(raw: &str) -> bool {
    let normalized = raw.replace('\\', "/").to_ascii_lowercase();
    let trimmed = normalized.trim();
    trimmed == "eva_skills" || trimmed == "./eva_skills" || trimmed.ends_with("/eva_skills")
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().to_string().to_lowercase();
        if key.trim().is_empty() || !seen.insert(key) {
            continue;
        }
        output.push(path);
    }
    output
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for value in values {
        let cleaned = value.trim();
        if cleaned.is_empty() {
            continue;
        }
        let key = cleaned.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        output.push(cleaned.to_string());
    }
    output
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
