use crate::args::GlobalArgs;
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

pub const CLI_DEFAULT_USER_ID: &str = "cli_user";

#[derive(Clone)]
pub struct CliRuntime {
    pub state: Arc<AppState>,
    pub launch_dir: PathBuf,
    pub temp_root: PathBuf,
    pub repo_root: PathBuf,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionMeta {
    session_id: String,
    updated_at: f64,
}

impl CliRuntime {
    pub async fn init(global: &GlobalArgs) -> Result<Self> {
        let launch_dir = std::env::current_dir().context("read current directory failed")?;
        let repo_root = resolve_repo_root(&launch_dir);
        let temp_root = global
            .temp_root
            .clone()
            .unwrap_or_else(|| launch_dir.join("WUNDER_TEMP"));
        ensure_runtime_dirs(&temp_root)?;

        let base_config = prepare_base_config_path(global, &repo_root, &temp_root)?;
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
        std::env::set_var("WUNDER_WORKSPACE_SINGLE_ROOT", "1");

        let config_store = ConfigStore::new(override_path.clone());
        let launch_dir_for_update = launch_dir.clone();
        let temp_root_for_update = temp_root.clone();
        let repo_root_for_update = repo_root.clone();
        let config = config_store
            .update(move |config| {
                apply_cli_defaults(
                    config,
                    &launch_dir_for_update,
                    &temp_root_for_update,
                    &repo_root_for_update,
                );
            })
            .await
            .context("apply cli runtime config failed")?;

        let state = Arc::new(
            AppState::new_with_options(
                config_store.clone(),
                config.clone(),
                AppStateInitOptions::cli_default(),
            )
            .context("initialize cli state failed")?,
        );
        state.lsp_manager.sync_with_config(&config).await;

        let user_id = global
            .user
            .clone()
            .unwrap_or_else(|| CLI_DEFAULT_USER_ID.to_string());
        Ok(Self {
            state,
            launch_dir,
            temp_root,
            repo_root,
            user_id,
        })
    }

    pub fn sessions_file(&self) -> PathBuf {
        self.temp_root.join("sessions/current_session.json")
    }

    pub fn extra_prompt_file(&self) -> PathBuf {
        self.temp_root.join("config/extra_prompt.txt")
    }

    pub fn load_extra_prompt(&self) -> Option<String> {
        let path = self.extra_prompt_file();
        let text = fs::read_to_string(path).ok()?;
        let cleaned = text.trim();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.to_string())
        }
    }

    pub fn save_extra_prompt(&self, prompt: &str) -> Result<()> {
        let cleaned = prompt.trim();
        if cleaned.is_empty() {
            return Err(anyhow!("extra prompt is empty"));
        }
        fs::write(self.extra_prompt_file(), cleaned.as_bytes())?;
        Ok(())
    }

    pub fn clear_extra_prompt(&self) -> Result<()> {
        let path = self.extra_prompt_file();
        if let Err(err) = fs::remove_file(path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                return Err(err.into());
            }
        }
        Ok(())
    }

    pub fn load_saved_session(&self) -> Option<String> {
        let path = self.sessions_file();
        let text = fs::read_to_string(path).ok()?;
        let data: SessionMeta = serde_json::from_str(&text).ok()?;
        let session_id = data.session_id.trim();
        if session_id.is_empty() {
            None
        } else {
            Some(session_id.to_string())
        }
    }

    pub fn save_session(&self, session_id: &str) -> Result<()> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(anyhow!("session id is empty"));
        }
        let payload = SessionMeta {
            session_id: session_id.to_string(),
            updated_at: now_ts(),
        };
        let text = serde_json::to_string_pretty(&payload)?;
        fs::write(self.sessions_file(), text)?;
        Ok(())
    }

    pub fn resolve_session(&self, preferred: Option<&str>) -> String {
        if let Some(value) = preferred.map(str::trim).filter(|value| !value.is_empty()) {
            return value.to_string();
        }
        if let Some(saved) = self.load_saved_session() {
            return saved;
        }
        uuid::Uuid::new_v4().simple().to_string()
    }

    pub async fn resolve_model_name(&self, requested: Option<&str>) -> Option<String> {
        if let Some(value) = requested.map(str::trim).filter(|value| !value.is_empty()) {
            return Some(value.to_string());
        }
        let config = self.state.config_store.get().await;
        if !config.llm.default.trim().is_empty() {
            return Some(config.llm.default.trim().to_string());
        }
        config.llm.models.iter().find_map(|(name, model)| {
            let model_type = model
                .model_type
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if matches!(model_type.as_str(), "embedding" | "embed" | "emb") {
                None
            } else {
                Some(name.clone())
            }
        })
    }
}

fn resolve_repo_root(launch_dir: &Path) -> PathBuf {
    if let Ok(value) = std::env::var("WUNDER_CLI_PROJECT_ROOT") {
        let cleaned = value.trim();
        if !cleaned.is_empty() {
            let candidate = PathBuf::from(cleaned);
            if candidate.is_dir() {
                return candidate;
            }
        }
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if looks_like_repo_root(&manifest) {
        return manifest;
    }

    if looks_like_repo_root(launch_dir) {
        return launch_dir.to_path_buf();
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(app_dir) = exe.parent() {
            if looks_like_repo_root(app_dir) {
                return app_dir.to_path_buf();
            }
        }
    }

    // Fallback: keep the previous behavior as last resort.
    manifest
}

fn looks_like_repo_root(candidate: &Path) -> bool {
    candidate.join("config/wunder.yaml").is_file() || candidate.join("prompts").is_dir()
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

fn prepare_base_config_path(
    global: &GlobalArgs,
    repo_root: &Path,
    temp_root: &Path,
) -> Result<PathBuf> {
    if let Some(path) = global.config_path.clone() {
        return Ok(path);
    }
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
    config.server.mode = "cli".to_string();
    let content =
        serde_yaml::to_string(&config).context("serialize generated cli base config failed")?;
    fs::write(path, content).with_context(|| {
        format!(
            "write generated cli base config failed: {}",
            path.to_string_lossy()
        )
    })?;
    Ok(())
}

fn apply_cli_defaults(config: &mut Config, launch_dir: &Path, temp_root: &Path, repo_root: &Path) {
    config.server.mode = "cli".to_string();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_root
        .join("wunder_cli.sqlite3")
        .to_string_lossy()
        .to_string();
    config.workspace.root = launch_dir.to_string_lossy().to_string();

    config.channels.enabled = false;
    config.gateway.enabled = false;
    config.agent_queue.enabled = false;
    config.cron.enabled = false;

    config.sandbox.mode = "local".to_string();

    let launch_skills = launch_dir.join("skills");
    let repo_skills = repo_root.join("skills");
    let repo_eva_skills = repo_root.join("EVA_SKILLS");
    let mut skill_paths = vec![launch_skills, repo_skills, repo_eva_skills];
    for existing in &config.skills.paths {
        let resolved = resolve_maybe_relative_path(existing, repo_root, launch_dir);
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
    allow_paths.push(launch_dir.to_string_lossy().to_string());
    config.security.allow_paths = dedupe_strings(allow_paths);
}

fn resolve_maybe_relative_path(raw: &str, repo_root: &Path, launch_dir: &Path) -> PathBuf {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return repo_root.to_path_buf();
    }
    let path = PathBuf::from(cleaned);
    if path.is_absolute() {
        return path;
    }
    let launch_candidate = launch_dir.join(&path);
    if launch_candidate.exists() {
        return launch_candidate;
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
