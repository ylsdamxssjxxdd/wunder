use crate::args::DesktopArgs;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use wunder_server::config::Config;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopSettings {
    pub workspace_root: String,
    pub desktop_token: String,
    pub updated_at: f64,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            workspace_root: String::new(),
            desktop_token: uuid::Uuid::new_v4().simple().to_string(),
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
        settings.updated_at = now_ts();
        save_desktop_settings(&settings_path, &settings)?;

        let base_config = prepare_base_config_path(&repo_root, &temp_root)?;
        let override_path = temp_root.join("config/wunder.override.yaml");
        let i18n_path = repo_root.join("config/i18n.messages.json");
        let prompts_root = repo_root.join("prompts");
        let skill_runner = repo_root.join("scripts/skill_runner.py");
        let user_tools_root = temp_root.join("user_tools");
        let vector_root = temp_root.join("vector_knowledge");

        set_env_path("WUNDER_CONFIG_PATH", &base_config);
        set_env_path("WUNDER_CONFIG_OVERRIDE_PATH", &override_path);
        set_env_path_if_exists("WUNDER_I18N_MESSAGES_PATH", &i18n_path);
        set_env_path_if_exists("WUNDER_PROMPTS_ROOT", &prompts_root);
        set_env_path_if_exists("WUNDER_SKILL_RUNNER_PATH", &skill_runner);
        set_env_path("WUNDER_USER_TOOLS_ROOT", &user_tools_root);
        set_env_path("WUNDER_VECTOR_KNOWLEDGE_ROOT", &vector_root);
        std::env::set_var("WUNDER_WORKSPACE_SINGLE_ROOT", "1");

        let user_id = normalize_user_id(args.user.as_deref());
        let desktop_token = settings.desktop_token.clone();

        let config_store = ConfigStore::new(override_path);
        let workspace_for_update = workspace_root.clone();
        let temp_root_for_update = temp_root.clone();
        let repo_root_for_update = repo_root.clone();
        let token_for_update = desktop_token.clone();
        let config = config_store
            .update(move |config| {
                apply_desktop_defaults(
                    config,
                    &workspace_for_update,
                    &temp_root_for_update,
                    &repo_root_for_update,
                    &token_for_update,
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

fn load_desktop_settings(path: &Path) -> Result<DesktopSettings> {
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

fn save_desktop_settings(path: &Path, settings: &DesktopSettings) -> Result<()> {
    let text =
        serde_json::to_string_pretty(settings).context("serialize desktop settings failed")?;
    fs::write(path, text)
        .with_context(|| format!("write desktop settings failed: {}", path.to_string_lossy()))
}

fn set_env_path(key: &str, value: &Path) {
    std::env::set_var(key, value.to_string_lossy().to_string());
}

fn set_env_path_if_exists(key: &str, value: &Path) {
    if value.exists() {
        set_env_path(key, value);
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

fn apply_desktop_defaults(
    config: &mut Config,
    workspace_root: &Path,
    temp_root: &Path,
    repo_root: &Path,
    desktop_token: &str,
) {
    config.server.mode = "desktop".to_string();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_root
        .join("wunder_desktop.sqlite3")
        .to_string_lossy()
        .to_string();
    config.workspace.root = workspace_root.to_string_lossy().to_string();

    config.channels.enabled = false;
    config.gateway.enabled = false;
    config.agent_queue.enabled = false;
    config.cron.enabled = false;
    config.sandbox.mode = "local".to_string();

    if !desktop_token.trim().is_empty() {
        config.security.api_key = Some(desktop_token.to_string());
    }

    let launch_skills = workspace_root.join("skills");
    let repo_skills = repo_root.join("skills");
    let repo_eva_skills = repo_root.join("EVA_SKILLS");
    let mut skill_paths = vec![launch_skills, repo_skills, repo_eva_skills];
    for existing in &config.skills.paths {
        let resolved = resolve_maybe_relative_path(existing, repo_root, workspace_root);
        skill_paths.push(resolved);
    }
    config.skills.paths = dedupe_paths(skill_paths)
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();

    let mut allow_paths = Vec::new();
    allow_paths.extend(config.security.allow_paths.iter().cloned());
    allow_paths.push(repo_root.join("EVA_SKILLS").to_string_lossy().to_string());
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
    if state.user_store.get_user_by_id(user_id)?.is_none() {
        let password = format!("wunder_desktop_{}", uuid::Uuid::new_v4().simple());
        state.user_store.create_user(
            user_id,
            None,
            &password,
            Some("A"),
            None,
            vec!["user".to_string()],
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
