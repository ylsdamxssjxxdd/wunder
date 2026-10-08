use crate::args::GlobalArgs;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use wunder_server::admin_skills;
use wunder_server::config::Config;
use wunder_server::config_store::ConfigStore;
use wunder_server::repo_assets;
use wunder_server::state::{AppState, AppStateInitOptions};

pub const CLI_DEFAULT_USER_ID: &str = "cli_user";
/// Bumped when the legacy-thread adoption rule changes.
const CLI_WORKSPACE_MIGRATION_VERSION: u32 = 1;

/// The workspace this process works in: the launch directory, bound to a real
/// row in `workspaces` so threads, tools and the thread directory all agree on
/// one root. No sandbox copy is involved.
#[derive(Clone, Debug)]
pub struct CliWorkspace {
    pub workspace_id: String,
    pub root_path: PathBuf,
    pub name: String,
}

#[derive(Clone)]
pub struct CliRuntime {
    pub state: Arc<AppState>,
    pub launch_dir: PathBuf,
    pub temp_root: PathBuf,
    pub wunder_home: PathBuf,
    pub repo_root: PathBuf,
    pub user_id: String,
    pub workspace: CliWorkspace,
    /// Merged user configuration (`config.toml` layers) plus provenance.
    pub user_config: crate::user_config::UserConfigReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnNotificationConfig {
    #[default]
    Off,
    Bell {
        #[serde(default)]
        when: TurnNotificationWhen,
    },
    Osc9 {
        #[serde(default)]
        when: TurnNotificationWhen,
    },
    Command {
        argv: Vec<String>,
        #[serde(default)]
        when: TurnNotificationWhen,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TurnNotificationWhen {
    #[default]
    Always,
    Unfocused,
}

impl CliRuntime {
    pub async fn init(global: &GlobalArgs) -> Result<Self> {
        let launch_dir = resolve_launch_dir(global)?;
        let repo_root = resolve_repo_root(&launch_dir);
        let wunder_home = resolve_wunder_home_dir(&launch_dir);
        migrate_legacy_cli_temp_root(&wunder_home)?;
        let temp_root = global
            .temp_root
            .clone()
            .unwrap_or_else(|| default_cli_temp_root(&wunder_home));
        let user_tools_root = wunder_home.join("user_tools");
        let vector_root = wunder_home.join("vector_knowledge");
        let companions_root = wunder_home.join("config/data/companions/global");
        ensure_runtime_dirs(&temp_root, &wunder_home, &user_tools_root, &vector_root)?;

        let config_path = prepare_runtime_config_path(global, &repo_root, &temp_root)?;
        let i18n_path = repo_root.join("config/i18n.messages.json");
        let skill_runner = repo_root.join("scripts/skill_runner.py");

        set_env_path("WUNDER_CONFIG_PATH", &config_path);
        set_env_path_if_exists("WUNDER_I18N_MESSAGES_PATH", &i18n_path);
        set_env_prompts_root_if_unset(&repo_root);
        set_env_path(
            "WUNDER_BUILTIN_SKILLS_ROOT",
            &repo_assets::builtin_skills_root(&repo_root),
        );
        set_env_path_if_exists("WUNDER_SKILL_RUNNER_PATH", &skill_runner);
        set_env_path("WUNDER_HOME", &wunder_home);
        set_env_path("WUNDER_USER_TOOLS_ROOT", &user_tools_root);
        set_env_path("WUNDER_VECTOR_KNOWLEDGE_ROOT", &vector_root);
        set_env_path("WUNDER_COMPANIONS_ROOT", &companions_root);
        set_env_path("WUNDER_TEMP_DIR_ROOT", &temp_root);
        std::env::set_var("WUNDER_WORKSPACE_SINGLE_ROOT", "1");

        // Users configure the CLI through `~/.wunder/config.toml`; the engine
        // YAML below is generated. The template is created once so the file is
        // discoverable, then every layer is projected onto the engine config.
        crate::user_config::ensure_user_config_template(&wunder_home)
            .context("write user config template failed")?;
        let user_config = match crate::user_config::load_report(
            &wunder_home,
            &launch_dir,
            global.profile.as_deref(),
            global.strict_config,
        ) {
            Ok(report) => report,
            Err(err) if global.strict_config => {
                return Err(err).context("load user config failed (strict mode)")
            }
            Err(err) => {
                // A broken user file must not make the tool unusable; say what
                // is wrong and run on defaults. `config validate` fails loudly.
                eprintln!("[warn] ignoring user config: {err}");
                crate::user_config::fallback_report(
                    &wunder_home,
                    &launch_dir,
                    global.profile.as_deref(),
                )
            }
        };

        // §6.5 leftover: the engine YAML is generated, never hand-edited, so it is
        // rebuilt from the shipped template on every start. A key the user removed
        // from config.toml (or a [provider] they deleted) therefore cannot keep
        // living in the generated file.
        let engine_template = load_engine_template(&repo_root);
        let template_model_for_update = engine_template
            .as_ref()
            .ok()
            .map(|config| config.llm.default.trim().to_string())
            .filter(|value| !value.is_empty());
        let config_store = match engine_template {
            Ok(template) => ConfigStore::from_config(config_path.clone(), template),
            Err(err) => {
                // A missing or broken template must not make the CLI unusable;
                // the previous generated file is the next best base.
                eprintln!("[warn] rebuilding the engine config from the template failed: {err}");
                ConfigStore::new(config_path.clone())
            }
        };
        let launch_dir_for_update = launch_dir.clone();
        let temp_root_for_update = temp_root.clone();
        let repo_root_for_update = repo_root.clone();
        let wunder_home_for_update = wunder_home.clone();
        let user_values_for_update = user_config.values.clone();
        let sandbox_for_update = global.sandbox;
        let approval_for_update = global.approval_mode;
        let _config = config_store
            .update(move |config| {
                apply_cli_defaults(
                    config,
                    &launch_dir_for_update,
                    &temp_root_for_update,
                    &repo_root_for_update,
                    &wunder_home_for_update,
                );
                apply_user_config(
                    config,
                    &user_values_for_update,
                    template_model_for_update.as_deref(),
                );
                // Command-line policy flags are the last layer: they sit above
                // every config file, and the sandbox word only decides the
                // approval default when no explicit approval was requested.
                apply_cli_policy(config, sandbox_for_update, approval_for_update);
            })
            .await
            .context("apply cli runtime config failed")?;
        let config = admin_skills::normalize_server_admin_skill_layout(&config_store).await;
        warn_on_dangerous_sandbox(&config, global.sandbox);

        let state = Arc::new(
            AppState::new_with_options(
                config_store.clone(),
                config.clone(),
                AppStateInitOptions::cli_default(),
            )
            .context("initialize cli state failed")?,
        );
        state.lsp_manager.sync_with_config(&config).await;

        // Cloud self-healing: refresh the session, re-synthesize the cloud
        // models and fire one heartbeat in the background. The engine probes
        // without blocking, so both the TUI and command mode start instantly.
        wunder_server::cloud::shared()
            .startup_probe(&config_store)
            .await;

        let user_id = global
            .user
            .clone()
            .unwrap_or_else(|| CLI_DEFAULT_USER_ID.to_string());
        let workspace = ensure_workspace_binding(state.clone(), user_id.as_str(), &launch_dir)
            .await
            .context("bind cli workspace failed")?;
        // Tools resolve their root through the workspace registry, exactly like
        // the desktop runtime; the request only carries the workspace id.
        state.workspace.register_workspace_root(
            workspace.workspace_id.as_str(),
            &workspace.root_path.to_string_lossy(),
        );
        adopt_legacy_threads(
            state.clone(),
            user_id.as_str(),
            workspace.workspace_id.as_str(),
        )
        .await
        .context("adopt legacy threads failed")?;

        let runtime = Self {
            state,
            launch_dir,
            temp_root,
            wunder_home,
            repo_root,
            user_id,
            workspace,
            user_config,
        };
        // `notify`/`notify_when` in config.toml are the single source for turn
        // notifications, so they are projected onto the runtime file every start.
        if let Err(err) = runtime.apply_user_notify() {
            eprintln!("[warn] ignoring notify setting: {err}");
        }
        Ok(runtime)
    }

    pub fn workspace_id(&self) -> &str {
        self.workspace.workspace_id.as_str()
    }

    pub fn workspace_root(&self) -> &Path {
        self.workspace.root_path.as_path()
    }

    pub fn turn_notification_file(&self) -> PathBuf {
        self.temp_root.join("config/turn_notification.json")
    }

    pub fn load_turn_notification_config(&self) -> TurnNotificationConfig {
        let path = self.turn_notification_file();
        let Ok(text) = fs::read_to_string(path) else {
            return TurnNotificationConfig::Off;
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save_turn_notification_config(&self, config: &TurnNotificationConfig) -> Result<()> {
        let path = self.turn_notification_file();
        let payload = serde_json::to_vec_pretty(config)?;
        fs::write(path, payload)?;
        Ok(())
    }

    pub fn clear_turn_notification_config(&self) -> Result<()> {
        let path = self.turn_notification_file();
        if let Err(err) = fs::remove_file(path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                return Err(err.into());
            }
        }
        Ok(())
    }

    /// Project `notify` / `notify_when` from the user config onto the runtime
    /// notification file. The file has no other writer, so an absent key means
    /// "off": a setting the user removed must not keep living in the file.
    fn apply_user_notify(&self) -> Result<()> {
        match notify_config_for(&self.user_config.values)? {
            Some(config) => self.save_turn_notification_config(&config),
            None => self.clear_turn_notification_config(),
        }
    }

    /// Every entry point opens a fresh thread unless the caller names one.
    /// The old "last session" pointer file is gone: resuming is an explicit
    /// choice (`--session`, `resume`, `/resume`), never an implicit one.
    pub fn resolve_session(&self, preferred: Option<&str>) -> String {
        if let Some(value) = preferred.map(str::trim).filter(|value| !value.is_empty()) {
            return value.to_string();
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

/// `-C/--cd` wins over the process directory; the result is an absolute,
/// canonical path because it becomes a workspace key.
fn resolve_launch_dir(global: &GlobalArgs) -> Result<PathBuf> {
    let requested = match global.cd.as_ref() {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().context("read current directory failed")?,
    };
    if !requested.is_dir() {
        return Err(anyhow!(
            "working directory does not exist: {}",
            requested.to_string_lossy()
        ));
    }
    Ok(normalize_workspace_root(&requested))
}

/// Canonicalize the workspace folder. Windows canonical paths carry a verbatim
/// prefix and the repo stores the plain form, so strip it for a stable key.
fn normalize_workspace_root(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let plain = raw.strip_prefix(r"\\?\").unwrap_or(raw.as_ref());
    Path::new(plain)
        .canonicalize()
        .map(|canonical| {
            let text = canonical.to_string_lossy();
            let stripped = text.strip_prefix(r"\\?\").unwrap_or(text.as_ref());
            PathBuf::from(stripped)
        })
        .unwrap_or_else(|_| PathBuf::from(plain))
}

/// Find (or create) the workspace row bound to `dir`. Creation is idempotent:
/// `(user_id, root_path)` is unique, so a concurrent second process loses the
/// insert race and then reads the winner's row.
async fn ensure_workspace_binding(
    state: Arc<AppState>,
    user_id: &str,
    dir: &Path,
) -> Result<CliWorkspace> {
    let root = normalize_workspace_root(dir);
    let root_path = root.to_string_lossy().to_string();
    let owner = user_id.to_string();
    let lookup_root = root_path.clone();
    let existing = tokio::task::spawn_blocking({
        let state = state.clone();
        let owner = owner.clone();
        move || state.storage.find_workspace_by_root(&owner, &lookup_root)
    })
    .await
    .map_err(|err| anyhow!("workspace lookup cancelled: {err}"))??;
    if let Some(record) = existing {
        return Ok(CliWorkspace {
            workspace_id: record.workspace_id,
            root_path: PathBuf::from(record.root_path),
            name: record.name,
        });
    }

    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("workspace")
        .to_string();
    let record = wunder_server::storage::WorkspaceRecord {
        workspace_id: format!("ws_{}", uuid::Uuid::new_v4().simple()),
        user_id: owner.clone(),
        name: name.clone(),
        root_path: root_path.clone(),
        icon: "folder".to_string(),
        color: "blue".to_string(),
        sort_index: 0,
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    let created = tokio::task::spawn_blocking({
        let state = state.clone();
        let record = record.clone();
        move || -> Result<bool> {
            if state
                .storage
                .find_workspace_by_root(&record.user_id, &record.root_path)?
                .is_some()
            {
                return Ok(false);
            }
            let mut record = record;
            record.sort_index = state.storage.next_workspace_sort_index(&record.user_id)?;
            state.storage.upsert_workspace(&record)?;
            Ok(true)
        }
    })
    .await
    .map_err(|err| anyhow!("workspace create cancelled: {err}"))??;
    if !created {
        // Another process bound this folder first; use its row.
        let owner = user_id.to_string();
        let lookup_root = root_path.clone();
        let existing = tokio::task::spawn_blocking(move || {
            state.storage.find_workspace_by_root(&owner, &lookup_root)
        })
        .await
        .map_err(|err| anyhow!("workspace lookup cancelled: {err}"))??;
        if let Some(record) = existing {
            return Ok(CliWorkspace {
                workspace_id: record.workspace_id,
                root_path: PathBuf::from(record.root_path),
                name: record.name,
            });
        }
    }
    Ok(CliWorkspace {
        workspace_id: record.workspace_id,
        root_path: root,
        name,
    })
}

/// Threads created before workspaces existed have no binding. Adopt them once
/// into this workspace so the resume list and the thread directory agree on a
/// single home; the meta flag keeps later startups from rescanning.
async fn adopt_legacy_threads(
    state: Arc<AppState>,
    user_id: &str,
    workspace_id: &str,
) -> Result<()> {
    let owner = user_id.to_string();
    let workspace_id = workspace_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let migration_key =
            format!("cli_workspace_migration:{CLI_WORKSPACE_MIGRATION_VERSION}:{owner}");
        if state.user_store.get_meta(&migration_key)?.as_deref() == Some("1") {
            return Ok(());
        }
        const PAGE: i64 = 200;
        loop {
            let mut adopted = 0usize;
            let mut orphans = Vec::new();
            let mut offset = 0i64;
            loop {
                let (records, total) = state
                    .user_store
                    .list_chat_sessions(&owner, None, None, offset, PAGE)?;
                let page_len = records.len();
                for record in records {
                    if record.workspace_id.is_none() {
                        orphans.push(record);
                        if orphans.len() >= PAGE as usize {
                            break;
                        }
                    }
                }
                offset += page_len as i64;
                if page_len == 0 || offset >= total || orphans.len() >= PAGE as usize {
                    break;
                }
            }
            if orphans.is_empty() {
                break;
            }
            let now = now_ts();
            for mut record in orphans {
                record.workspace_id = Some(workspace_id.clone());
                record.updated_at = now;
                state.user_store.upsert_chat_session(&record)?;
                adopted += 1;
            }
            if adopted == 0 {
                break;
            }
        }
        state.user_store.set_meta(&migration_key, "1")?;
        Ok(())
    })
    .await
    .map_err(|err| anyhow!("legacy thread adoption cancelled: {err}"))?
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
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

    if let Some(repo_root) = find_repo_root_at_or_above(launch_dir) {
        return repo_root;
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(app_dir) = exe.parent() {
            let resources_dir = app_dir.join("resources");
            for candidate in [app_dir, resources_dir.as_path()] {
                if let Some(repo_root) = find_repo_root_at_or_above(candidate) {
                    return repo_root;
                }
            }
        }
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(repo_root) = find_repo_root_at_or_above(&manifest) {
        return repo_root;
    }

    // Fallback: keep the previous behavior as last resort.
    manifest
}

fn find_repo_root_at_or_above(candidate: &Path) -> Option<PathBuf> {
    for path in candidate.ancestors() {
        let normalized = repo_assets::normalize_repo_root_candidate(path);
        if repo_assets::looks_like_repo_root(&normalized) {
            return Some(normalized);
        }
    }
    None
}

fn ensure_runtime_dirs(
    temp_root: &Path,
    wunder_home: &Path,
    user_tools_root: &Path,
    vector_root: &Path,
) -> Result<()> {
    for dir in [
        temp_root.to_path_buf(),
        temp_root.join("config"),
        temp_root.join("logs"),
        wunder_home.to_path_buf(),
        wunder_home.join("skills"),
        user_tools_root.to_path_buf(),
        vector_root.to_path_buf(),
    ] {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn default_cli_temp_root(wunder_home: &Path) -> PathBuf {
    wunder_home.to_path_buf()
}

/// Older CLI builds placed mutable state one level below `cli/WUNDER_TEMP`.
/// Merge only files that do not already exist at the canonical destination so
/// an interrupted migration never overwrites newer state.
fn migrate_legacy_cli_temp_root(wunder_home: &Path) -> Result<()> {
    let target = wunder_home.join("cli");
    let legacy = target.join("WUNDER_TEMP");
    if !legacy.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(&target)?;
    for entry in fs::read_dir(&legacy)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if destination.exists() {
            continue;
        }
        fs::rename(entry.path(), destination)?;
    }
    if fs::read_dir(&legacy)?.next().is_none() {
        fs::remove_dir(&legacy)?;
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
    if repo_assets::builtin_prompts_root(repo_root).is_dir() {
        set_env_path("WUNDER_PROMPTS_ROOT", repo_root);
    }
}

fn prepare_runtime_config_path(
    global: &GlobalArgs,
    repo_root: &Path,
    temp_root: &Path,
) -> Result<PathBuf> {
    if let Some(path) = global.config_path.clone() {
        return Ok(path);
    }
    let runtime_config = temp_root.join("config/wunder.yaml");
    if runtime_config.exists() {
        return Ok(runtime_config);
    }
    let repo_config = repo_root.join("config/wunder.yaml");
    if repo_config.exists() {
        if let Some(parent) = runtime_config.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&repo_config, &runtime_config).with_context(|| {
            format!(
                "copy cli config failed: {} -> {}",
                repo_config.display(),
                runtime_config.display()
            )
        })?;
        return Ok(runtime_config);
    }
    let generated = runtime_config;
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

fn apply_cli_defaults(
    config: &mut Config,
    launch_dir: &Path,
    temp_root: &Path,
    repo_root: &Path,
    wunder_home: &Path,
) {
    config.server.mode = "cli".to_string();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = temp_root
        .join("wunder_cli.sqlite3")
        .to_string_lossy()
        .to_string();
    // Runtime-owned data belongs under the user's single Wunder home. The
    // launch directory remains an input workspace only when explicitly
    // selected by the caller; it is never used as a storage root.
    config.workspace.root = wunder_home.join("workspace").to_string_lossy().to_string();

    config.channels.enabled = false;
    config.gateway.enabled = false;
    config.agent_queue.enabled = false;
    config.cron.enabled = false;

    // 舵机 is a local form like 蜂窝: the workspace folder is the boundary, so
    // commands are not filtered by a per-call whitelist and the approval gate
    // starts open. An explicit user choice lives in the approval sidecar
    // (config.toml takes over this role in the config-projection step).
    config.security.allow_commands = vec!["*".to_string()];
    config.security.deny_globs.clear();
    config.security.exec_policy_mode = None;
    config.security.approval_mode = Some("full_auto".to_string());

    let user_skills = wunder_home.join("skills");
    let repo_skills = repo_assets::builtin_skills_root(repo_root);
    let mut skill_paths = vec![user_skills, repo_skills];
    for existing in &config.skills.paths {
        if is_eva_skills_path(existing) {
            continue;
        }
        let resolved = resolve_maybe_relative_path(existing, repo_root, launch_dir);
        skill_paths.push(resolved);
    }
    config.skills.paths = dedupe_paths(skill_paths)
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();

    // Tool roots are the real boundary of the local form: the launch folder,
    // the Wunder home (skills, temp scratch) and the builtin skill assets.
    // Wildcards inherited from the server template are dropped here, otherwise
    // every drive would be writable and the workspace would mean nothing.
    let mut allow_paths = config
        .security
        .allow_paths
        .iter()
        .filter(|path| !is_eva_skills_path(path))
        .filter(|path| !is_allow_all_path_token(path))
        .cloned()
        .collect::<Vec<_>>();
    allow_paths.push(wunder_home.to_string_lossy().to_string());
    allow_paths.push(launch_dir.join(".wunder").to_string_lossy().to_string());
    allow_paths.push(
        repo_assets::builtin_skills_root(repo_root)
            .to_string_lossy()
            .to_string(),
    );
    allow_paths.push(launch_dir.to_string_lossy().to_string());
    config.security.allow_paths = dedupe_strings(allow_paths);
}

fn is_allow_all_path_token(value: &str) -> bool {
    value.trim() == "*"
}

/// The sandbox word in engine terms. The engine has one boundary (the allowed
/// tool roots) and one gate (the approval mode), so a sandbox word is expressed
/// as those two knobs and nothing new is invented.
///
/// `approval_is_explicit` keeps an approval policy the user stated in the same
/// layer: the sandbox word then only decides the boundary.
pub(crate) fn apply_sandbox_mode(
    config: &mut Config,
    mode: crate::args::SandboxModeArg,
    approval_is_explicit: bool,
) {
    use crate::args::SandboxModeArg;
    match mode {
        SandboxModeArg::ReadOnly => {
            // Reads keep the roots; writes and execution go through the gate.
            // A non-interactive run then refuses instead of prompting.
            if !approval_is_explicit {
                config.security.approval_mode = Some("suggest".to_string());
            }
        }
        SandboxModeArg::WorkspaceWrite => {
            // The CLI default: free inside the workspace, refused outside it.
        }
        SandboxModeArg::DangerFullAccess => {
            if !config
                .security
                .allow_paths
                .iter()
                .any(|path| is_allow_all_path_token(path))
            {
                config.security.allow_paths.push("*".to_string());
            }
        }
    }
}

/// Command-line policy flags (`-s/--sandbox`, `--approval-mode`). They are the
/// highest layer, and an explicit approval word always beats a sandbox-implied
/// default.
pub(crate) fn apply_cli_policy(
    config: &mut Config,
    sandbox: Option<crate::args::SandboxModeArg>,
    approval: Option<crate::args::ApprovalModeArg>,
) {
    if let Some(mode) = sandbox {
        apply_sandbox_mode(config, mode, approval.is_some());
    }
    if let Some(mode) = approval {
        config.security.approval_mode = Some(mode.as_str().to_string());
    }
}

/// What `notify` / `notify_when` in config.toml mean for the runtime file.
/// `None` means "no notification file at all", which is also what an absent key
/// says: config.toml is the single source, so a removed setting must not linger.
pub(crate) fn notify_config_for(
    values: &crate::user_config::UserConfigValues,
) -> Result<Option<TurnNotificationConfig>> {
    let Some(kind) = values
        .notify
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let config = parse_notify_setting(kind, values.notify_when.as_deref())?;
    if config == TurnNotificationConfig::Off {
        return Ok(None);
    }
    Ok(Some(config))
}

/// `notify` / `notify_when` from config.toml, parsed into the runtime shape.
/// The grammar is the one the removed `/notify` command accepted, minus the
/// statefulness that a single config value cannot express.
pub(crate) fn parse_notify_setting(
    kind: &str,
    when: Option<&str>,
) -> Result<TurnNotificationConfig> {
    let when = match when.map(str::trim).filter(|value| !value.is_empty()) {
        None => TurnNotificationWhen::Always,
        Some(raw) => match raw.to_ascii_lowercase().as_str() {
            "always" => TurnNotificationWhen::Always,
            "unfocused" => TurnNotificationWhen::Unfocused,
            other => {
                return Err(anyhow!(
                    "notify_when must be always|unfocused, got `{other}`"
                ))
            }
        },
    };

    let cleaned = kind.trim();
    let config = match cleaned.to_ascii_lowercase().as_str() {
        "off" | "none" | "clear" => TurnNotificationConfig::Off,
        "bell" => TurnNotificationConfig::Bell { when },
        "osc9" => TurnNotificationConfig::Osc9 { when },
        _ => {
            let argv = cleaned
                .split_whitespace()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            if argv.is_empty() {
                return Err(anyhow!("notify must be off|bell|osc9|<command...>"));
            }
            TurnNotificationConfig::Command { argv, when }
        }
    };
    Ok(config)
}

/// `danger-full-access` removes the only guard the local form has, so it is
/// always announced. The explicit flag is the confirmation; an interactive
/// prompt would hang the non-interactive entry points.
fn warn_on_dangerous_sandbox(config: &Config, sandbox: Option<crate::args::SandboxModeArg>) {
    if sandbox != Some(crate::args::SandboxModeArg::DangerFullAccess) {
        return;
    }
    let language = config.i18n.default_language.trim();
    eprintln!(
        "[warn] {}",
        if language.eq_ignore_ascii_case("en-US") {
            "sandbox danger-full-access: the workspace boundary is off, every path on this machine is writable"
        } else {
            "沙箱 danger-full-access：工作区边界已关闭，本机任意路径都可读写"
        }
    );
}

/// The shipped engine template (`config/wunder.yaml`). It is the base the local
/// forms rebuild their generated YAML from on every start.
fn load_engine_template(repo_root: &Path) -> Result<Config> {
    let path = repo_root.join("config/wunder.yaml");
    let text = fs::read_to_string(&path)
        .with_context(|| format!("read engine config template failed: {}", path.display()))?;
    serde_yaml::from_str::<Config>(&text)
        .with_context(|| format!("parse engine config template failed: {}", path.display()))
}

/// Project the merged user configuration onto the engine config. The engine YAML
/// is generated, so this runs on every start and is the single write direction.
/// `template_default_model` is the model the shipped template configures; it is
/// restored when the user layer stops naming one, otherwise a removed setting
/// would keep living in the generated file.
pub(crate) fn apply_user_config(
    config: &mut Config,
    values: &crate::user_config::UserConfigValues,
    template_default_model: Option<&str>,
) {
    if let Some(model) = values
        .model
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        config.llm.default = model.to_string();
        // An existing engine-configured model keeps its connection details; a
        // name with no entry is only materialized when [provider] supplies the
        // endpoint, so a typo cannot fabricate an unusable model.
        if let Some(entry) = config.llm.models.get_mut(model) {
            entry.enable = Some(true);
            if let Some(effort) = values.model_reasoning_effort.as_deref() {
                entry.reasoning_effort = Some(effort.to_string());
            }
        } else if values.provider.is_some() {
            let entry = config.llm.models.entry(model.to_string()).or_default();
            entry.enable = Some(true);
            entry.model = Some(model.to_string());
            if let Some(effort) = values.model_reasoning_effort.as_deref() {
                entry.reasoning_effort = Some(effort.to_string());
            }
        }
    } else if let Some(base) = template_default_model
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        config.llm.default = base.to_string();
    }

    // An effort without a model name still applies to the effective default.
    if values.model.is_none() {
        if let Some(effort) = values.model_reasoning_effort.as_deref() {
            let default = config.llm.default.trim().to_string();
            if !default.is_empty() {
                config
                    .llm
                    .models
                    .entry(default)
                    .or_default()
                    .reasoning_effort = Some(effort.to_string());
            }
        }
    }

    if let Some(provider) = values.provider.as_ref() {
        let model_name = config.llm.default.trim().to_string();
        if !model_name.is_empty() {
            let base_url = provider.base_url.clone().unwrap_or_default();
            let api_key = provider.api_key.clone().unwrap_or_default();
            let inferred = crate::infer_provider_from_base_url(base_url.as_str());
            let entry = config.llm.models.entry(model_name.clone()).or_default();
            entry.enable = Some(true);
            if entry
                .model
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            {
                entry.model = Some(model_name);
            }
            if !inferred.trim().is_empty() {
                entry.provider = Some(inferred);
            }
            if !base_url.trim().is_empty() {
                entry.base_url = Some(base_url);
            }
            if !api_key.trim().is_empty() {
                entry.api_key = Some(api_key);
            }
            if let Some(max_context) = provider.max_context {
                entry.max_context = Some(max_context.max(1));
            }
            entry.tool_call_mode = entry
                .tool_call_mode
                .clone()
                .or_else(|| Some("tool_call".to_string()));
        }
    }

    if let Some(policy) = values.approval_policy.as_deref() {
        if let Some(mode) = crate::map_approval_policy(policy) {
            config.security.approval_mode = Some(mode.to_string());
        }
    }

    if let Some(mode) = values
        .sandbox_mode
        .as_deref()
        .map(str::trim)
        .and_then(crate::args::SandboxModeArg::from_word)
    {
        // An explicit `approval_policy` in the same layer states the approval
        // dimension on purpose, so the sandbox word must not overwrite it.
        apply_sandbox_mode(config, mode, values.approval_policy.is_some());
    }

    if let Some(enabled) = values.project_doc {
        config.project_doc.enabled = enabled;
    }
    if let Some(max_bytes) = values.project_doc_max_bytes {
        config.project_doc.max_bytes = max_bytes.max(1);
    }

    if let Some(language) = values
        .language
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        config.i18n.default_language = language.to_string();
    }
}

fn resolve_wunder_home_dir(_launch_dir: &Path) -> PathBuf {
    if let Some(path) = read_non_empty_env_path("WUNDER_HOME") {
        return path;
    }
    if let Some(home) = resolve_user_home_dir() {
        return home.join(".wunder");
    }
    // A missing OS home is exceptional (for example a restricted service
    // account); keep the fallback inside the platform temp directory rather
    // than polluting the launch directory.
    std::env::temp_dir().join("wunder-user")
}

fn resolve_user_home_dir() -> Option<PathBuf> {
    if let Some(path) = read_non_empty_env_path("HOME") {
        return Some(path);
    }
    if let Some(path) = read_non_empty_env_path("USERPROFILE") {
        return Some(path);
    }
    let drive = std::env::var("HOMEDRIVE").ok().unwrap_or_default();
    let home_path = std::env::var("HOMEPATH").ok().unwrap_or_default();
    let combined = format!("{drive}{home_path}");
    let cleaned = combined.trim();
    if cleaned.is_empty() {
        None
    } else {
        Some(PathBuf::from(cleaned))
    }
}

fn read_non_empty_env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn is_eva_skills_path(raw: &str) -> bool {
    let normalized = raw.replace('\\', "/").to_ascii_lowercase();
    let trimmed = normalized.trim();
    trimmed == "eva_skills" || trimmed == "./eva_skills" || trimmed.ends_with("/eva_skills")
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

#[cfg(test)]
mod workspace_tests {
    use super::*;
    use wunder_server::config_store::ConfigStore;
    use wunder_server::state::{AppState, AppStateInitOptions};
    use wunder_server::storage::ChatSessionRecord;

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let mut root = std::env::temp_dir();
            root.push(format!(
                "wunder_cli_ws_{tag}_{}_{}",
                std::process::id(),
                stamp
            ));
            fs::create_dir_all(root.join("config")).expect("create config dir");
            Self(root)
        }

        fn path(&self) -> &Path {
            self.0.as_path()
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0.as_path());
        }
    }

    fn fixture_state(root: &Path) -> Arc<AppState> {
        let mut config = Config::default();
        config.server.mode = "cli".to_string();
        config.storage.backend = "sqlite".to_string();
        config.storage.db_path = root.join("state.sqlite3").to_string_lossy().to_string();
        config.workspace.root = root.join("workspace").to_string_lossy().to_string();
        let config_path = root.join("config/wunder.yaml");
        fs::write(
            &config_path,
            serde_yaml::to_string(&config).expect("serialize fixture config"),
        )
        .expect("write fixture config");
        let store = ConfigStore::new(config_path);
        Arc::new(
            AppState::new_with_options(store, config, AppStateInitOptions::cli_default())
                .expect("fixture state"),
        )
    }

    #[test]
    fn workspace_root_drops_the_windows_verbatim_prefix() {
        let temp = TempRoot::new("verbatim");
        let dir = temp.path().join("folder");
        fs::create_dir_all(&dir).expect("create folder");
        let verbatim = PathBuf::from(format!(r"\\?\{}", dir.to_string_lossy()));
        let normalized = normalize_workspace_root(&verbatim);
        assert!(
            !normalized.to_string_lossy().starts_with(r"\\?\"),
            "verbatim prefix survived: {}",
            normalized.to_string_lossy()
        );
        assert_eq!(
            normalize_workspace_root(&dir),
            normalize_workspace_root(&normalized)
        );
    }

    #[test]
    fn binding_the_same_folder_twice_reuses_one_workspace_row() {
        let temp = TempRoot::new("idempotent");
        let work = temp.path().join("project");
        fs::create_dir_all(&work).expect("create project dir");
        // AppState spawns runtime-owned tasks, so it must be built inside a
        // reactor; the binding calls then run on that same runtime.
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(async {
                let state = fixture_state(temp.path());
                let first = ensure_workspace_binding(state.clone(), "u1", &work)
                    .await
                    .expect("first binding");
                let second = ensure_workspace_binding(state.clone(), "u1", &work)
                    .await
                    .expect("second binding");

                assert_eq!(first.workspace_id, second.workspace_id);
                let rows = state
                    .storage
                    .list_workspaces("u1")
                    .expect("list workspaces");
                assert_eq!(rows.len(), 1, "a folder must bind to exactly one workspace");
                assert_eq!(rows[0].root_path, first.root_path.to_string_lossy());
                assert_eq!(rows[0].name, "project");
            });
    }

    #[test]
    fn legacy_threads_are_adopted_once_into_the_bound_workspace() {
        let temp = TempRoot::new("adopt");
        let work = temp.path().join("project");
        fs::create_dir_all(&work).expect("create project dir");
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(async {
                let state = fixture_state(temp.path());
                let now = now_ts();
                state
                    .user_store
                    .upsert_chat_session(&ChatSessionRecord {
                        session_id: "legacy-thread".to_string(),
                        user_id: "u1".to_string(),
                        title: "legacy".to_string(),
                        status: "active".to_string(),
                        created_at: now,
                        updated_at: now,
                        last_message_at: now,
                        agent_id: None,
                        workspace_id: None,
                        tool_overrides: Vec::new(),
                        parent_session_id: None,
                        parent_message_id: None,
                        spawn_label: None,
                        spawned_by: None,
                    })
                    .expect("seed legacy thread");

                adopt_legacy_threads(state.clone(), "u1", "ws_bound")
                    .await
                    .expect("adopt");
                let record = state
                    .user_store
                    .get_chat_session("u1", "legacy-thread")
                    .expect("read thread")
                    .expect("thread exists");
                assert_eq!(record.workspace_id.as_deref(), Some("ws_bound"));

                // A thread created after adoption keeps its own (absent)
                // binding: the migration is a one-shot pass, not a rewriter.
                state
                    .user_store
                    .upsert_chat_session(&ChatSessionRecord {
                        session_id: "later-thread".to_string(),
                        user_id: "u1".to_string(),
                        title: "later".to_string(),
                        status: "active".to_string(),
                        created_at: now,
                        updated_at: now,
                        last_message_at: now,
                        agent_id: None,
                        workspace_id: None,
                        tool_overrides: Vec::new(),
                        parent_session_id: None,
                        parent_message_id: None,
                        spawn_label: None,
                        spawned_by: None,
                    })
                    .expect("seed later thread");
                adopt_legacy_threads(state.clone(), "u1", "ws_bound")
                    .await
                    .expect("second adopt");
                let later = state
                    .user_store
                    .get_chat_session("u1", "later-thread")
                    .expect("read later thread")
                    .expect("later thread exists");
                assert!(
                    later.workspace_id.is_none(),
                    "the adoption pass must not run twice"
                );
            });
    }

    #[test]
    fn cd_flag_selects_the_launch_directory() {
        let temp = TempRoot::new("cd");
        let work = temp.path().join("elsewhere");
        fs::create_dir_all(&work).expect("create dir");
        let mut global = crate::args::GlobalArgs {
            model: None,
            tool_call_mode: None,
            approval_mode: None,
            sandbox: None,
            session: None,
            cd: Some(work.clone()),
            profile: None,
            strict_config: false,
            attachments: Vec::new(),
            json: false,
            language: None,
            config_path: None,
            temp_root: None,
            user: None,
            no_stream: false,
        };
        let resolved = resolve_launch_dir(&global).expect("resolve -C");
        assert_eq!(resolved, normalize_workspace_root(&work));

        global.cd = Some(temp.path().join("missing"));
        assert!(
            resolve_launch_dir(&global).is_err(),
            "a missing -C directory must be rejected"
        );
    }

    #[test]
    fn user_config_maps_policies_onto_the_engine() {
        use crate::user_config::{ProviderValues, UserConfigValues};

        let mut config = Config::default();
        config.llm.default = "demo".to_string();
        let values = UserConfigValues {
            model: Some("demo".to_string()),
            model_reasoning_effort: Some("high".to_string()),
            approval_policy: Some("on-request".to_string()),
            sandbox_mode: Some("workspace-write".to_string()),
            project_doc: Some(false),
            project_doc_max_bytes: Some(4096),
            language: Some("en-US".to_string()),
            provider: Some(ProviderValues {
                base_url: Some("https://api.openai.com/v1".to_string()),
                api_key: Some("test-key".to_string()),
                max_context: Some(64_000),
            }),
            notify: None,
            notify_when: None,
        };

        apply_user_config(&mut config, &values, None);

        assert_eq!(config.llm.default, "demo");
        assert_eq!(config.security.approval_mode.as_deref(), Some("auto_edit"));
        assert_eq!(config.project_doc.enabled, false);
        assert_eq!(config.project_doc.max_bytes, 4096);
        assert_eq!(config.i18n.default_language, "en-US");
        let entry = config.llm.models.get("demo").expect("model entry created");
        assert_eq!(entry.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(entry.provider.as_deref(), Some("openai"));
        assert_eq!(entry.base_url.as_deref(), Some("https://api.openai.com/v1"));
        assert_eq!(entry.max_context, Some(64_000));
        assert!(
            !config
                .security
                .allow_paths
                .iter()
                .any(|path| path.trim() == "*"),
            "the default sandbox stays bounded"
        );
    }

    #[test]
    fn a_removed_model_setting_falls_back_to_the_template_default() {
        use crate::user_config::UserConfigValues;

        let mut config = Config::default();
        config.llm.default = "stale-from-previous-run".to_string();

        apply_user_config(
            &mut config,
            &UserConfigValues::default(),
            Some("template-model"),
        );

        assert_eq!(
            config.llm.default, "template-model",
            "the generated config must not keep a value the user removed"
        );
    }

    #[test]
    fn danger_full_access_widens_the_workspace_boundary() {
        use crate::user_config::UserConfigValues;

        let mut config = Config::default();
        let values = UserConfigValues {
            sandbox_mode: Some("danger-full-access".to_string()),
            ..UserConfigValues::default()
        };
        apply_user_config(&mut config, &values, None);
        assert!(
            config
                .security
                .allow_paths
                .iter()
                .any(|path| path.trim() == "*"),
            "full access must be explicit and visible in allow_paths"
        );

        let mut read_only = Config::default();
        let values = UserConfigValues {
            sandbox_mode: Some("read-only".to_string()),
            ..UserConfigValues::default()
        };
        apply_user_config(&mut read_only, &values, None);
        assert_eq!(
            read_only.security.approval_mode.as_deref(),
            Some("suggest"),
            "read-only means every mutation needs approval"
        );
    }

    #[test]
    fn an_explicit_approval_policy_survives_a_read_only_sandbox() {
        use crate::user_config::UserConfigValues;

        let mut config = Config::default();
        let values = UserConfigValues {
            sandbox_mode: Some("read-only".to_string()),
            approval_policy: Some("never".to_string()),
            ..UserConfigValues::default()
        };
        apply_user_config(&mut config, &values, None);
        assert_eq!(
            config.security.approval_mode.as_deref(),
            Some("full_auto"),
            "a policy stated in the same layer owns the approval dimension"
        );
    }

    #[test]
    fn the_sandbox_flag_is_the_last_layer_and_the_approval_flag_beats_it() {
        use crate::args::{ApprovalModeArg, SandboxModeArg};

        let mut config = Config::default();
        config.security.allow_paths = vec!["/workspace".to_string()];

        apply_cli_policy(&mut config, Some(SandboxModeArg::ReadOnly), None);
        assert_eq!(config.security.approval_mode.as_deref(), Some("suggest"));

        // `-s read-only --approval-mode never` keeps the boundary but opens the
        // gate: the more specific flag wins.
        let mut explicit = Config::default();
        apply_cli_policy(
            &mut explicit,
            Some(SandboxModeArg::ReadOnly),
            Some(ApprovalModeArg::Never),
        );
        assert_eq!(
            explicit.security.approval_mode.as_deref(),
            Some("full_auto")
        );

        // `workspace-write` (the default) changes nothing.
        let mut bounded = Config::default();
        bounded.security.allow_paths = vec!["/workspace".to_string()];
        apply_cli_policy(&mut bounded, Some(SandboxModeArg::WorkspaceWrite), None);
        assert_eq!(
            bounded.security.allow_paths,
            vec!["/workspace".to_string()],
            "the default sandbox keeps the workspace boundary exactly as configured"
        );
        assert_eq!(bounded.security.approval_mode, None);
    }

    #[test]
    fn the_approval_word_table_maps_onto_the_engine_modes() {
        use crate::args::ApprovalModeArg;

        assert_eq!(ApprovalModeArg::OnRequest.as_str(), "auto_edit");
        assert_eq!(ApprovalModeArg::Never.as_str(), "full_auto");
        assert_eq!(ApprovalModeArg::Suggest.as_str(), "suggest");
        assert_eq!(ApprovalModeArg::Never.policy_word(), "never");
        assert_eq!(ApprovalModeArg::OnRequest.policy_word(), "on-request");
        // Round trip: whatever the engine holds is reported in codex words.
        for word in ["never", "on-request", "suggest"] {
            let engine = crate::map_approval_policy(word).expect("word maps");
            assert_eq!(
                ApprovalModeArg::from_engine_mode(engine).policy_word(),
                word
            );
        }
        // The legacy words keep working.
        assert_eq!(
            crate::parse_approval_mode("auto_edit"),
            Some(ApprovalModeArg::OnRequest)
        );
        assert_eq!(
            crate::parse_approval_mode("full_auto"),
            Some(ApprovalModeArg::Never)
        );
    }

    #[test]
    fn notify_settings_become_the_runtime_notification_file() {
        use crate::user_config::UserConfigValues;

        // No key at all means "off": the file must not keep a setting the user
        // removed from config.toml.
        assert_eq!(
            notify_config_for(&UserConfigValues::default()).expect("absent key is fine"),
            None
        );
        assert_eq!(
            notify_config_for(&UserConfigValues {
                notify: Some("off".to_string()),
                ..UserConfigValues::default()
            })
            .expect("explicit off"),
            None
        );
        assert_eq!(
            notify_config_for(&UserConfigValues {
                notify: Some("bell".to_string()),
                notify_when: Some("unfocused".to_string()),
                ..UserConfigValues::default()
            })
            .expect("bell"),
            Some(TurnNotificationConfig::Bell {
                when: TurnNotificationWhen::Unfocused,
            })
        );
        assert!(
            notify_config_for(&UserConfigValues {
                notify: Some("bell".to_string()),
                notify_when: Some("sometimes".to_string()),
                ..UserConfigValues::default()
            })
            .is_err(),
            "an unknown timing word must be reported, not ignored"
        );
    }
}
