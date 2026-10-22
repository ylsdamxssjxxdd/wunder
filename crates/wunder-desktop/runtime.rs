use crate::args::DesktopArgs;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::{info, warn};
use wunder_server::admin_skills;
use wunder_server::config::{merge_config_value, Config};
use wunder_server::config_store::ConfigStore;
use wunder_server::desktop_lan::{self, DesktopLanMeshSettings};
use wunder_server::repo_assets;
use wunder_server::state::{AppState, AppStateInitOptions};
use wunder_server::storage::{
    normalize_workspace_container_id, UserTokenRecord, MAX_SANDBOX_CONTAINER_ID,
    USER_PRIVATE_CONTAINER_ID,
};
use wunder_server::user_store::UserStore;

pub const DESKTOP_DEFAULT_USER_ID: &str = "local_user";
const BUILTIN_SKILLS_ROOT_ENV: &str = "WUNDER_BUILTIN_SKILLS_ROOT";
const ADMIN_CUSTOM_SKILLS_ROOT_ENV: &str = "WUNDER_ADMIN_CUSTOM_SKILLS_ROOT";
const DESKTOP_APPROVAL_MODE_MIGRATION_VERSION: &str = "full_auto_v1";
const DESKTOP_WORKSPACE_IDENTITY_MIGRATION_VERSION: &str = "path_identity_v1";
/// Name of the workspace created on first run; migrated legacy threads land
/// here. Mirrored by the workspace façade's docs, never user-configurable.
const DEFAULT_WORKSPACE_NAME: &str = "默认工作区";
const DESKTOP_CONTROL_MIN_NORM_WIDTH: i32 = 1000;
const DESKTOP_CONTROL_MIN_NORM_HEIGHT: i32 = 1000;

#[derive(Clone)]
pub struct DesktopRuntime {
    pub state: Arc<AppState>,
    pub app_dir: PathBuf,
    pub wunder_home: PathBuf,
    pub temp_root: PathBuf,
    pub settings_path: PathBuf,
    pub workspace_root: PathBuf,
    pub frontend_root: Option<PathBuf>,
    pub repo_root: PathBuf,
    pub user_id: String,
    pub desktop_token: String,
    pub lan_mesh: DesktopLanMeshSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopSettings {
    pub workspace_root: String,
    pub desktop_token: String,
    #[serde(default)]
    pub python_path: String,
    #[serde(default)]
    pub pip_path: String,
    #[serde(default)]
    pub git_path: String,
    #[serde(default)]
    pub rg_path: String,
    /// Preferred command shell: "auto" (use Git Bash when Git is configured),
    /// "bash", "cmd" or "powershell".
    #[serde(default)]
    pub shell_mode: String,
    #[serde(default = "default_python_runtime_mode")]
    pub python_runtime_mode: String,
    #[serde(default)]
    pub container_roots: HashMap<i32, String>,
    #[serde(default)]
    pub container_cloud_workspaces: HashMap<i32, String>,
    #[serde(default)]
    pub language: String,
    /// UI accent palette: "terracotta" (the 蜂巢 default; stored legacy names
    /// like "light" render as it), "hula-green", "forest", "honey", "minimal"
    /// or "tech-blue". The settings card offers the first four.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Composer send key: "enter" (default), "ctrl_enter" or "none" (button only).
    #[serde(default = "default_send_key")]
    pub send_key: String,
    /// Chat content font size in px (12..=20, default 14); the UI renders
    /// font tokens scaled by font_size / 14.
    #[serde(default = "default_font_size")]
    pub font_size: i32,
    #[serde(default)]
    pub lan_mesh: DesktopLanMeshSettings,
    pub updated_at: f64,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            workspace_root: String::new(),
            desktop_token: uuid::Uuid::new_v4().simple().to_string(),
            python_path: String::new(),
            pip_path: String::new(),
            git_path: String::new(),
            rg_path: String::new(),
            shell_mode: String::new(),
            python_runtime_mode: default_python_runtime_mode(),
            container_roots: HashMap::new(),
            container_cloud_workspaces: HashMap::new(),
            language: String::new(),
            theme: default_theme(),
            send_key: default_send_key(),
            font_size: default_font_size(),
            lan_mesh: DesktopLanMeshSettings::default(),
            updated_at: now_ts(),
        }
    }
}

impl DesktopRuntime {
    pub async fn init(args: &DesktopArgs) -> Result<Self> {
        wunder_server::local_bootstrap::install_local_home_env();
        let startup_enabled = startup_timing_enabled();
        let startup_boot = Instant::now();
        log_startup_point(
            startup_enabled,
            "bridge-runtime",
            "runtime_init_begin",
            startup_boot,
        );

        let mut step_start = Instant::now();
        let app_dir = resolve_app_dir()?;
        let wunder_home = resolve_wunder_home_dir();
        let repo_root = resolve_repo_root(&app_dir, &wunder_home);
        fs::create_dir_all(&wunder_home)?;
        // Shared configuration, settings and the database always live below
        // WUNDER_HOME. `--temp-root` only selects ephemeral process files.
        let temp_root = resolve_temp_root(args.temp_root.as_deref(), &wunder_home)?;
        let state_root = wunder_home.clone();
        let user_id = normalize_user_id(args.user.as_deref());
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "resolve_paths_and_user",
            step_start,
            startup_boot,
        );

        step_start = Instant::now();
        wunder_server::local_assets::release_embedded_local_assets(&state_root)
            .context("release embedded local assets failed")?;
        ensure_runtime_dirs(&state_root, &temp_root)?;
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "ensure_runtime_dirs",
            step_start,
            startup_boot,
        );

        let settings_path = state_root.join("config/desktop.settings.json");
        step_start = Instant::now();
        let mut settings = load_desktop_settings(&settings_path)?;
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "load_desktop_settings",
            step_start,
            startup_boot,
        );

        step_start = Instant::now();
        let workspace_root = resolve_workspace_root(
            args.workspace.as_deref(),
            &settings.workspace_root,
            &wunder_home,
        )?;
        fs::create_dir_all(&workspace_root).with_context(|| {
            format!(
                "create workspace root failed: {}",
                workspace_root.to_string_lossy()
            )
        })?;
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "prepare_workspace",
            step_start,
            startup_boot,
        );

        step_start = Instant::now();
        if settings.desktop_token.trim().is_empty() {
            settings.desktop_token = uuid::Uuid::new_v4().simple().to_string();
        }
        settings.workspace_root = workspace_root.to_string_lossy().to_string();
        settings.container_roots = normalize_desktop_container_roots(
            &settings.container_roots,
            &workspace_root,
            &app_dir,
            &user_id,
        );
        settings.container_cloud_workspaces =
            normalize_desktop_container_cloud_workspaces(&settings.container_cloud_workspaces);
        settings
            .container_cloud_workspaces
            .retain(|container_id, _| settings.container_roots.contains_key(container_id));
        fs::create_dir_all(&workspace_root).with_context(|| {
            format!(
                "create desktop workspace root failed: {}",
                workspace_root.display()
            )
        })?;
        ensure_container_root_dirs(&settings.container_roots)?;
        settings.lan_mesh = settings.lan_mesh.clone().normalized();
        if settings.lan_mesh.peer_id.trim().is_empty() {
            settings.lan_mesh.peer_id = build_default_lan_peer_id(&user_id);
        }
        if settings.lan_mesh.display_name.trim().is_empty() {
            settings.lan_mesh.display_name = user_id.clone();
        }
        let mut active_lan = settings.lan_mesh.clone();
        if args.native_runtime {
            // Native desktop has no network control listener or bridge.
            active_lan.enabled = false;
        }
        desktop_lan::manager().apply_settings(active_lan).await;
        settings.updated_at = now_ts();
        save_desktop_settings(&settings_path, &settings)?;
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "normalize_settings_and_save",
            step_start,
            startup_boot,
        );

        step_start = Instant::now();
        let config_path = prepare_runtime_config_path(&repo_root, &state_root, &app_dir)?;
        let i18n_path = state_root.join("config/i18n.messages.json");
        let skill_runner = state_root.join("scripts/skill_runner.py");
        let user_tools_root = state_root.join("user_tools");
        let admin_custom_skills_root = state_root.join("admin_skills");
        let vector_root = state_root.join("vector_knowledge");
        let companions_root = state_root.join("config/data/companions/global");
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "prepare_config_paths",
            step_start,
            startup_boot,
        );

        step_start = Instant::now();
        set_env_path("WUNDER_CONFIG_PATH", &config_path);
        set_env_path_if_exists("WUNDER_I18N_MESSAGES_PATH", &i18n_path);
        set_env_prompts_root_if_unset(&state_root);
        set_env_path_if_exists("WUNDER_SKILL_RUNNER_PATH", &skill_runner);
        set_env_path("WUNDER_USER_TOOLS_ROOT", &user_tools_root);
        set_env_path(ADMIN_CUSTOM_SKILLS_ROOT_ENV, &admin_custom_skills_root);
        set_env_path("WUNDER_VECTOR_KNOWLEDGE_ROOT", &vector_root);
        set_env_path("WUNDER_COMPANIONS_ROOT", &companions_root);
        set_env_path("WUNDER_DESKTOP_SETTINGS_PATH", &settings_path);
        set_env_path("WUNDER_DESKTOP_APP_DIR", &app_dir);
        set_env_path("WUNDER_HOME", &wunder_home);
        // All ephemeral artifacts (desktop/browser screenshots, temp-dir
        // downloads and channel exports) share the desktop runtime root.
        // Without this export the runtime helpers fall back to the process
        // directory, which puts screenshots beside the executable under
        // `config/data/temp_dir` instead of the user's `.wunder` directory.
        set_env_path("WUNDER_TEMP_DIR_ROOT", &temp_root);
        refresh_runtime_tool_env(&settings, &app_dir);
        set_env_path("WUNDER_DESKTOP_DEFAULT_WORKSPACE_ROOT", &workspace_root);
        set_env_path(BUILTIN_SKILLS_ROOT_ENV, &state_root.join("skills"));
        std::env::set_var("WUNDER_DESKTOP_USER_ID", user_id.clone());
        std::env::set_var("WUNDER_WORKSPACE_SINGLE_ROOT", "1");
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "apply_environment",
            step_start,
            startup_boot,
        );

        let desktop_token = settings.desktop_token.clone();

        step_start = Instant::now();
        let config_store = ConfigStore::new(config_path);
        let workspace_for_update = workspace_root.clone();
        let state_root_for_update = state_root.clone();
        let repo_root_for_update = repo_root.clone();
        let token_for_update = desktop_token.clone();
        let container_roots_for_update = settings.container_roots.clone();
        let language_for_update = settings.language.clone();
        let _config = config_store
            .update(move |config| {
                apply_desktop_defaults(
                    config,
                    &workspace_for_update,
                    &state_root_for_update,
                    &repo_root_for_update,
                    DesktopDefaultsInput {
                        desktop_token: &token_for_update,
                        container_roots: &container_roots_for_update,
                        language: &language_for_update,
                    },
                );
                // Queue and channel switches are shared by 蛹 and 舵机. The
                // native facade starts its dispatcher below; it must not
                // rewrite the shared YAML according to this process only.
            })
            .await
            .context("apply desktop runtime config failed")?;
        let config = admin_skills::normalize_server_admin_skill_layout(&config_store).await;
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "config_store_update",
            step_start,
            startup_boot,
        );

        step_start = Instant::now();
        let mut state = AppState::new_with_options(
            config_store.clone(),
            config.clone(),
            AppStateInitOptions::desktop_default().with_start_thread_runtime(false),
        )
        .context("initialize desktop state failed")?;
        // Recover stale SQLite tasks before the native queue dispatcher starts.
        state.runtime_capabilities.thread_runtime_active = args.native_runtime;
        let state = Arc::new(state);
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "app_state_init",
            step_start,
            startup_boot,
        );

        if config.lsp.enabled {
            step_start = Instant::now();
            state.lsp_manager.sync_with_config(&config).await;
            log_startup_segment(
                startup_enabled,
                "bridge-runtime",
                "lsp_sync_with_config",
                step_start,
                startup_boot,
            );
        }
        step_start = Instant::now();
        ensure_desktop_identity(state.as_ref(), &user_id, &desktop_token)?;
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "ensure_desktop_identity",
            step_start,
            startup_boot,
        );

        step_start = Instant::now();
        if state.runtime_capabilities.thread_runtime_active {
            // Runtime state is shared with other local instances. Never clear
            // another process' locks or tasks during startup; durable leases
            // expire and are reclaimed by the owning runtime path.
            state.kernel.thread_runtime.clone().start();
        }
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "desktop_runtime_recovery",
            step_start,
            startup_boot,
        );

        step_start = Instant::now();
        migrate_desktop_local_agent_approval_modes(state.as_ref(), &user_id)?;
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "migrate_desktop_agent_approval_modes",
            step_start,
            startup_boot,
        );

        step_start = Instant::now();
        let legacy_user_root = state
            .workspace
            .ensure_user_root(&user_id)
            .unwrap_or_else(|_| workspace_root.join(sanitize_workspace_scope(&user_id)));
        migrate_local_workspaces(state.as_ref(), &user_id, &legacy_user_root)?;
        migrate_workspace_path_identity(state.as_ref(), &user_id)?;
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "migrate_local_workspaces",
            step_start,
            startup_boot,
        );

        step_start = Instant::now();
        let lan_mesh = settings.lan_mesh.clone();
        let frontend_root =
            resolve_frontend_root(args.frontend_root.as_deref(), &repo_root, &app_dir);
        log_startup_segment(
            startup_enabled,
            "bridge-runtime",
            "resolve_remote_and_frontend",
            step_start,
            startup_boot,
        );

        log_startup_point(
            startup_enabled,
            "bridge-runtime",
            "runtime_init_done",
            startup_boot,
        );

        Ok(Self {
            state,
            app_dir,
            wunder_home,
            temp_root,
            settings_path,
            workspace_root,
            frontend_root,
            repo_root,
            user_id,
            desktop_token,
            lan_mesh,
        })
    }
}

fn resolve_app_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("resolve current exe path failed")?;
    exe.parent()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("resolve app dir failed from exe path"))
}

/// Asset root for the local form. A source checkout or a portable resources
/// folder wins so development and hand-assembled layouts keep live files; an
/// installed desktop falls back to WUNDER_HOME, where the embedded assets are
/// projected.
fn resolve_repo_root(app_dir: &Path, wunder_home: &Path) -> PathBuf {
    if let Some(repo_root) = repo_assets::resolve_local_form_repo_root(app_dir, None) {
        return repo_root;
    }
    if let Some(repo_root) =
        repo_assets::resolve_local_form_repo_root(Path::new(env!("CARGO_MANIFEST_DIR")), None)
    {
        return repo_root;
    }
    wunder_home.to_path_buf()
}

fn resolve_temp_root(temp_root: Option<&Path>, wunder_home: &Path) -> Result<PathBuf> {
    match temp_root {
        Some(path) if path.is_absolute() => Ok(path.to_path_buf()),
        Some(path) => Ok(wunder_home.join(path)),
        None => std::env::var_os("WUNDER_DESKTOP_TEMP_ROOT")
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    wunder_home.join(path)
                }
            })
            .map(Ok)
            .unwrap_or_else(|| Ok(wunder_home.to_path_buf())),
    }
}

fn resolve_workspace_root(
    arg_workspace: Option<&Path>,
    settings_workspace: &str,
    wunder_home: &Path,
) -> Result<PathBuf> {
    if let Some(path) = arg_workspace {
        return Ok(if path.is_absolute() {
            path.to_path_buf()
        } else {
            wunder_home.join(path)
        });
    }

    let raw = settings_workspace.trim();
    if raw.is_empty() {
        if let Some(path) = std::env::var_os("WUNDER_DESKTOP_WORKSPACE_ROOT").map(PathBuf::from) {
            let path = if path.is_absolute() {
                path
            } else {
                wunder_home.join(path)
            };
            if !is_transient_build_workspace_root(&path) {
                return Ok(path);
            }
        }
        return Ok(wunder_home.join("workspace"));
    }

    let path = PathBuf::from(raw);
    // Older packaged desktop builds persisted the server seed as a relative
    // `./config/data/workspaces` path. Treat that value as stale state instead
    // of resolving it against the executable directory. Explicit absolute
    // paths and other user-selected relative paths remain supported.
    if is_legacy_desktop_workspace_seed(raw) {
        return Ok(wunder_home.join("workspace"));
    }
    if path.is_absolute() {
        if is_transient_build_workspace_root(&path) {
            return Ok(wunder_home.join("workspace"));
        }
        Ok(path)
    } else if raw.eq_ignore_ascii_case("WUNDER_WORK")
        || raw.eq_ignore_ascii_case("WUNDER_TEMPD")
        || raw.eq_ignore_ascii_case("workspace")
    {
        Ok(wunder_home.join("workspace"))
    } else {
        Ok(wunder_home.join(path))
    }
}

/// Build and smoke-test workspaces are created below a repository's `target`
/// directory. They must never become the persisted desktop workspace after a
/// test process exits; doing so makes the next蛹 launch point tools at a stale
/// checkout path instead of the user's `.wunder` home.
fn is_transient_build_workspace_root(path: &Path) -> bool {
    let mut saw_target = false;
    for ancestor in path.ancestors() {
        let Some(name) = ancestor.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.eq_ignore_ascii_case("target") {
            saw_target = true;
            continue;
        }
        if saw_target && ancestor.join(".git").exists() {
            return true;
        }
    }
    false
}

fn is_legacy_desktop_workspace_seed(raw: &str) -> bool {
    let normalized = raw.trim().replace('\\', "/").to_ascii_lowercase();
    // Desktop settings are JSON rather than YAML, so a historical setting
    // can contain the template literally instead of the expanded fallback.
    // That value means the same server seed when no override was supplied.
    if normalized == "${wunder_workspace_root:-./config/data/workspaces}" {
        return true;
    }
    let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);
    matches!(
        normalized,
        "config/data/workspaces"
            | "config/data/workspaces/"
            | "config/data/workspaces/local_user"
            | "config/data/workspaces/local_user/"
            | "config/data/workspaces/local_user/workspace"
            | "config/data/workspaces/local_user/workspace/"
    )
}

fn resolve_wunder_home_dir() -> PathBuf {
    wunder_server::local_bootstrap::resolve_home_from_env_or_user()
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

    let mut candidates = vec![
        repo_root.join("frontend/dist"),
        repo_root.join("frontend-dist"),
        app_dir.join("frontend/dist"),
        app_dir.join("frontend-dist"),
        app_dir.join("resources/frontend/dist"),
        app_dir.join("resources/frontend-dist"),
    ];
    if let Some(parent) = app_dir.parent() {
        candidates.push(parent.join("Resources/frontend/dist"));
        candidates.push(parent.join("Resources/frontend-dist"));
    }
    candidates.into_iter().find(|candidate| candidate.exists())
}

fn ensure_runtime_dirs(state_root: &Path, temp_root: &Path) -> Result<()> {
    for dir in [
        state_root.to_path_buf(),
        state_root.join("config"),
        state_root.join("logs"),
        state_root.join("user_tools"),
        state_root.join("admin_skills"),
        state_root.join("vector_knowledge"),
        temp_root.to_path_buf(),
        temp_root.join("sessions"),
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
    match serde_json::from_str::<DesktopSettings>(&text) {
        Ok(settings) => Ok(settings),
        Err(primary_err) => {
            let backup_path = desktop_settings_backup_path(path);
            if backup_path.exists() {
                if let Ok(backup_text) = fs::read_to_string(&backup_path) {
                    if !backup_text.trim().is_empty() {
                        if let Ok(settings) = serde_json::from_str::<DesktopSettings>(&backup_text)
                        {
                            warn!(
                                "desktop settings parse failed, recovered from backup: {} -> {}: {primary_err}",
                                path.display(),
                                backup_path.display()
                            );
                            archive_invalid_desktop_settings(path);
                            return Ok(settings);
                        }
                    }
                }
            }
            warn!(
                "desktop settings parse failed and no backup was usable, starting with defaults: {}: {primary_err}",
                path.display()
            );
            archive_invalid_desktop_settings(path);
            Ok(DesktopSettings::default())
        }
    }
}

pub(crate) fn save_desktop_settings(path: &Path, settings: &DesktopSettings) -> Result<()> {
    let text =
        serde_json::to_string_pretty(settings).context("serialize desktop settings failed")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create desktop settings dir failed: {}", parent.display()))?;
    }
    let backup_path = desktop_settings_backup_path(path);
    let temp_path = desktop_settings_temp_path(path);
    fs::write(&temp_path, text).with_context(|| {
        format!(
            "write desktop settings temp failed: {}",
            temp_path.display()
        )
    })?;
    if path.exists() {
        fs::copy(path, &backup_path).with_context(|| {
            format!(
                "backup desktop settings failed: {} -> {}",
                path.display(),
                backup_path.display()
            )
        })?;
    }
    if let Err(initial_err) = fs::rename(&temp_path, path) {
        if let Err(remove_err) = fs::remove_file(path) {
            if remove_err.kind() != std::io::ErrorKind::NotFound {
                return Err(remove_err).with_context(|| {
                    format!("remove old desktop settings failed: {}", path.display())
                });
            }
        }
        fs::rename(&temp_path, path).with_context(|| {
            format!(
                "replace desktop settings failed: {} (initial rename error: {initial_err})",
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

fn desktop_settings_invalid_path(path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("desktop.settings.json");
    path.with_file_name(format!("{file_name}.invalid-{stamp}"))
}

fn archive_invalid_desktop_settings(path: &Path) {
    let archive_path = desktop_settings_invalid_path(path);
    if let Err(err) = fs::rename(path, &archive_path) {
        warn!(
            "archive invalid desktop settings failed: {} -> {}: {err}",
            path.display(),
            archive_path.display()
        );
    }
}

pub(crate) fn normalize_desktop_container_roots(
    source: &HashMap<i32, String>,
    default_workspace_root: &Path,
    app_dir: &Path,
    user_id: &str,
) -> HashMap<i32, String> {
    let normalized_user_id = normalize_user_id(Some(user_id));
    let workspace_root_cmp = normalize_path_for_compare(default_workspace_root);

    let mut explicit = HashMap::new();
    let mut seen_paths = HashSet::new();
    seen_paths.insert(workspace_root_cmp);
    for container_id in USER_PRIVATE_CONTAINER_ID..=MAX_SANDBOX_CONTAINER_ID {
        let default_root =
            build_default_container_root(default_workspace_root, &normalized_user_id, container_id);
        seen_paths.insert(normalize_path_for_compare(&default_root));
    }

    for (container_id, root) in source {
        let normalized_id = normalize_workspace_container_id(*container_id);
        let trimmed = root.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resolved = resolve_workspace_path_input(trimmed, app_dir);
        let resolved_cmp = normalize_path_for_compare(&resolved);
        if resolved_cmp.is_empty() || seen_paths.contains(&resolved_cmp) {
            continue;
        }
        if normalized_id == USER_PRIVATE_CONTAINER_ID {
            seen_paths.insert(resolved_cmp.clone());
            explicit.insert(normalized_id, resolved);
            continue;
        }
        if !(1..=MAX_SANDBOX_CONTAINER_ID).contains(&normalized_id) {
            continue;
        }
        seen_paths.insert(resolved_cmp);
        explicit.insert(normalized_id, resolved);
    }

    let mut output = HashMap::new();
    for container_id in USER_PRIVATE_CONTAINER_ID..=MAX_SANDBOX_CONTAINER_ID {
        let root = explicit.remove(&container_id).unwrap_or_else(|| {
            build_default_container_root(default_workspace_root, &normalized_user_id, container_id)
        });
        output.insert(container_id, root.to_string_lossy().to_string());
    }
    output
}

pub(crate) fn normalize_desktop_container_cloud_workspaces(
    source: &HashMap<i32, String>,
) -> HashMap<i32, String> {
    let mut output = HashMap::new();
    for (container_id, workspace_id) in source {
        let normalized_id = wunder_server::storage::normalize_sandbox_container_id(*container_id);
        let cleaned = workspace_id.trim();
        if cleaned.is_empty() {
            continue;
        }
        output.insert(normalized_id, cleaned.to_string());
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

/// Secret-free tool path status for the runtime settings panel. `source`
/// values: "custom" (configured and valid), "invalid" (configured but the
/// file is missing, runtime falls back), "embedded" (bundled supplement),
/// "system" (system PATH or explicit system preference), "missing".
#[derive(Clone, Debug)]
pub struct RuntimeToolStatus {
    pub tool: String,
    pub configured: String,
    pub effective: String,
    pub source: String,
}

fn tool_status(
    tool: &str,
    configured: &str,
    resolved: Option<PathBuf>,
    auto_label: &str,
) -> RuntimeToolStatus {
    let configured = configured.trim().to_string();
    let (source, effective) = match (configured.is_empty(), resolved) {
        (false, Some(path)) => ("custom", path.to_string_lossy().into_owned()),
        (false, None) => ("invalid", String::new()),
        (true, maybe) => (
            auto_label,
            maybe
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
    };
    RuntimeToolStatus {
        tool: tool.into(),
        configured,
        effective,
        source: source.into(),
    }
}

pub fn runtime_tool_statuses(settings: &DesktopSettings, app_dir: &Path) -> Vec<RuntimeToolStatus> {
    // The python resolver silently falls back to the system interpreter when
    // a configured path is invalid; the status panel must expose that instead.
    let configured_python = settings.python_path.trim();
    let (python_resolved, python_auto_label) = if configured_python.is_empty() {
        match resolve_desktop_python_bin(settings, app_dir) {
            DesktopPythonBin::Auto(path) => (Some(path), "embedded"),
            DesktopPythonBin::System => (None, "system"),
            DesktopPythonBin::None => (None, "missing"),
            DesktopPythonBin::Custom(_) => (None, "missing"),
        }
    } else {
        let candidate = resolve_workspace_path_input(configured_python, app_dir);
        (candidate.is_file().then_some(candidate), "missing")
    };
    // Git and rg resolve like python: configured path wins, then the embedded
    // supplement, then the system PATH. The status panel must label a binary
    // unpacked from the bundled supplement "embedded", not "system PATH".
    let (git_resolved, git_auto_label) = resolve_tool_auto(
        &settings.git_path,
        app_dir,
        resolve_embedded_git_bin,
        || search_path_for("git"),
    );
    let (rg_resolved, rg_auto_label) =
        resolve_tool_auto(&settings.rg_path, app_dir, resolve_embedded_rg_bin, || {
            search_path_for("rg")
        });
    vec![
        tool_status(
            "python",
            configured_python,
            python_resolved,
            python_auto_label,
        ),
        tool_status("git", &settings.git_path, git_resolved, git_auto_label),
        tool_status("rg", &settings.rg_path, rg_resolved, rg_auto_label),
    ]
}

/// Auto-resolution for a runtime tool with no explicit configured path: the
/// embedded supplement first, then the system PATH. Returns the resolved
/// binary plus the source label ("embedded" / "system" / "missing") used by
/// the settings status panel.
fn resolve_tool_auto(
    configured: &str,
    app_dir: &Path,
    embedded: fn(&Path) -> Option<PathBuf>,
    system: fn() -> Option<PathBuf>,
) -> (Option<PathBuf>, &'static str) {
    let trimmed = configured.trim();
    if !trimmed.is_empty() {
        let candidate = resolve_workspace_path_input(trimmed, app_dir);
        return (candidate.is_file().then_some(candidate), "missing");
    }
    if let Some(path) = embedded(app_dir) {
        return (Some(path), "embedded");
    }
    if let Some(path) = system() {
        return (Some(path), "system");
    }
    (None, "missing")
}

fn default_theme() -> String {
    "terracotta".to_string()
}

fn default_send_key() -> String {
    "enter".to_string()
}

fn default_font_size() -> i32 {
    14
}

fn default_python_runtime_mode() -> String {
    "auto".to_string()
}

enum DesktopPythonBin {
    Auto(PathBuf),
    Custom(PathBuf),
    System,
    None,
}

fn resolve_desktop_python_bin(settings: &DesktopSettings, app_dir: &Path) -> DesktopPythonBin {
    let trimmed = settings.python_path.trim();
    if !trimmed.is_empty() {
        let candidate = resolve_workspace_path_input(trimmed, app_dir);
        if candidate.is_file() {
            return DesktopPythonBin::Custom(candidate);
        }
        return DesktopPythonBin::System;
    }
    if settings
        .python_runtime_mode
        .trim()
        .eq_ignore_ascii_case("system")
    {
        return DesktopPythonBin::System;
    }
    resolve_embedded_python_bin(app_dir)
        .map(DesktopPythonBin::Auto)
        .unwrap_or(DesktopPythonBin::None)
}

fn resolve_desktop_tool_bin(raw: &str, app_dir: &Path) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = resolve_workspace_path_input(trimmed, app_dir);
    candidate.is_file().then_some(candidate)
}

/// Re-apply the process environment that routes tool commands to the embedded
/// supplement: PATH prefixes plus `WUNDER_PYTHON_BIN` / `WUNDER_RG_BIN`. Runs
/// at boot and again after a supplement import so newly spawned tool
/// processes pick up the unpacked runtimes without an app restart.
pub(crate) fn refresh_runtime_tool_env(settings: &DesktopSettings, app_dir: &Path) {
    prepend_embedded_tool_paths(app_dir);
    match resolve_desktop_python_bin(settings, app_dir) {
        DesktopPythonBin::Custom(python_bin) | DesktopPythonBin::Auto(python_bin) => {
            set_env_path_if_exists("WUNDER_PYTHON_BIN", &python_bin);
        }
        DesktopPythonBin::System => {
            std::env::remove_var("WUNDER_PYTHON_BIN");
        }
        DesktopPythonBin::None => {}
    }
    if let Some(rg_bin) = resolve_desktop_tool_bin(&settings.rg_path, app_dir)
        .or_else(|| resolve_embedded_rg_bin(app_dir))
    {
        set_env_path_if_exists("WUNDER_RG_BIN", &rg_bin);
    }
}

/// Resolve the tool binaries tool commands will actually use right now, for
/// user-facing status reports.
pub(crate) fn resolve_effective_tool_bins(
    settings: &DesktopSettings,
    app_dir: &Path,
) -> (Option<PathBuf>, Option<PathBuf>, Option<PathBuf>) {
    let python = match resolve_desktop_python_bin(settings, app_dir) {
        DesktopPythonBin::Custom(path) | DesktopPythonBin::Auto(path) => Some(path),
        DesktopPythonBin::System | DesktopPythonBin::None => None,
    };
    let git = resolve_desktop_tool_bin(&settings.git_path, app_dir)
        .or_else(|| resolve_embedded_git_bin(app_dir))
        .or_else(|| search_path_for("git"));
    let rg = resolve_desktop_tool_bin(&settings.rg_path, app_dir)
        .or_else(|| resolve_embedded_rg_bin(app_dir))
        .or_else(|| search_path_for("rg"));
    (python, git, rg)
}

fn search_path_for(program: &str) -> Option<PathBuf> {
    let exe_names: &[String] = if cfg!(windows) {
        &[
            format!("{program}.exe"),
            format!("{program}.bat"),
            format!("{program}.cmd"),
        ]
    } else {
        &[program.to_string()]
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = exe_names
            .iter()
            .map(|name| dir.join(name))
            .find(|candidate| candidate.is_file());
        candidate
    })
}

fn resolve_embedded_git_bin(app_dir: &Path) -> Option<PathBuf> {
    wunder_server::local_bootstrap::supplement::find_tool(
        &embedded_supplement_roots_for_env(app_dir),
        wunder_server::local_bootstrap::supplement::Tool::Git,
    )
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
        DESKTOP_DEFAULT_USER_ID.to_string()
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
        return workspace_root.join(sanitize_workspace_scope(user_id));
    }
    workspace_root.join(format!(
        "{}__c__{container_id}",
        sanitize_workspace_scope(user_id)
    ))
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

fn set_env_path(key: &str, value: &Path) {
    std::env::set_var(key, value.to_string_lossy().to_string());
}

fn set_env_path_if_exists(key: &str, value: &Path) {
    if value.exists() {
        set_env_path(key, value);
    }
}

/// The shared home is authoritative; install/AppImage trees are read-only
/// fallbacks for older manually unpacked supplements.
pub(crate) fn embedded_supplement_roots_for_env(app_dir: &Path) -> Vec<PathBuf> {
    wunder_server::local_bootstrap::supplement::roots(
        &resolve_wunder_home_dir(),
        app_dir,
        resolve_appimage_dir(),
    )
}

fn prepend_embedded_tool_paths(app_dir: &Path) {
    wunder_server::local_bootstrap::supplement::prepend_tool_paths(
        &embedded_supplement_roots_for_env(app_dir),
    );
}

fn resolve_embedded_python_bin(app_dir: &Path) -> Option<PathBuf> {
    wunder_server::local_bootstrap::supplement::find_tool(
        &embedded_supplement_roots_for_env(app_dir),
        wunder_server::local_bootstrap::supplement::Tool::Python,
    )
}

fn resolve_embedded_rg_bin(app_dir: &Path) -> Option<PathBuf> {
    wunder_server::local_bootstrap::supplement::find_tool(
        &embedded_supplement_roots_for_env(app_dir),
        wunder_server::local_bootstrap::supplement::Tool::Rg,
    )
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
    repo_root: &Path,
    state_root: &Path,
    app_dir: &Path,
) -> Result<PathBuf> {
    let runtime_config = state_root.join("config/wunder.yaml");
    if runtime_config.exists() {
        return Ok(runtime_config);
    }
    let parent = runtime_config
        .parent()
        .ok_or_else(|| anyhow!("invalid desktop config path: {}", runtime_config.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create desktop config dir failed: {}", parent.display()))?;

    let repo_config = repo_root.join("config/wunder-example.yaml");
    if repo_config.exists() {
        fs::copy(&repo_config, &runtime_config).with_context(|| {
            format!(
                "copy desktop config failed: {} -> {}",
                repo_config.display(),
                runtime_config.display()
            )
        })?;
    } else {
        ensure_generated_base_config(&runtime_config)?;
    }

    if let Some(source_path) = resolve_desktop_preconfig_path(app_dir, repo_root) {
        merge_desktop_preconfig(&runtime_config, &source_path)?;
    }

    Ok(runtime_config)
}

fn merge_desktop_preconfig(config_path: &Path, source_path: &Path) -> Result<()> {
    let content = fs::read_to_string(source_path)
        .with_context(|| format!("read desktop preconfig failed: {}", source_path.display()))?;
    if content.trim().is_empty() {
        warn!(
            "desktop preconfig is empty, skip seeding: {}",
            source_path.display()
        );
        return Ok(());
    }

    let mut config_value = read_yaml_value_raw(config_path)?;
    let preconfig_value = serde_yaml::from_str(&content)
        .with_context(|| format!("parse desktop preconfig failed: {}", source_path.display()))?;
    merge_config_value(&mut config_value, preconfig_value);
    let merged_text =
        serde_yaml::to_string(&config_value).context("serialize merged desktop config failed")?;
    fs::write(config_path, merged_text).with_context(|| {
        format!(
            "seed desktop config failed: {} -> {}",
            source_path.display(),
            config_path.display()
        )
    })?;
    info!(
        "seed desktop config from {} to {}",
        source_path.display(),
        config_path.display()
    );
    Ok(())
}

fn read_yaml_value_raw(path: &Path) -> Result<serde_yaml::Value> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("read yaml failed: {}", path.display()))?;
    serde_yaml::from_str(&content).with_context(|| format!("parse yaml failed: {}", path.display()))
}

fn resolve_desktop_preconfig_path(app_dir: &Path, repo_root: &Path) -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("WUNDER_DESKTOP_PRECONFIG_PATH") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    if let Some(appimage_dir) = resolve_appimage_dir() {
        if let Some(path) = desktop_preconfig_candidates(&appimage_dir)
            .into_iter()
            .find(|path| path.is_file())
        {
            return Some(path);
        }
    }

    if let Some(path) = desktop_preconfig_candidates(app_dir)
        .into_iter()
        .find(|path| path.is_file())
    {
        return Some(path);
    }

    [
        repo_root.join("config/wunder.desktop.preconfig.yaml"),
        repo_root.join("预配置文件.yml"),
        repo_root.join("docs/分发/预配置文件.yml"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn resolve_appimage_dir() -> Option<PathBuf> {
    std::env::var("APPIMAGE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .filter(|path| path.is_dir())
}

fn desktop_preconfig_candidates(root: &Path) -> Vec<PathBuf> {
    [
        "wunder.yaml",
        "wunder.yml",
        "预配置文件.yml",
        "预配置文件.yaml",
    ]
    .into_iter()
    .map(|name| root.join(name))
    .collect()
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
}

fn apply_desktop_defaults(
    config: &mut Config,
    workspace_root: &Path,
    state_root: &Path,
    repo_root: &Path,
    defaults: DesktopDefaultsInput<'_>,
) {
    config.server.mode = "desktop".to_string();
    config.storage.backend = "sqlite".to_string();
    config.storage.data_root = std::env::var("WUNDER_STORAGE_DATA_ROOT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| state_root.join("data").to_string_lossy().into_owned());
    // `config/wunder-example.yaml` still carries the server-oriented relative
    // seed (`./config/data/workspaces`). If it survives into a desktop home,
    // the relative path is resolved against the executable/launch directory
    // and recreates the old `config/data/workspaces/local_user/workspace`
    // tree. The desktop resolver has already selected the authoritative root
    // (normally `<user>/.wunder/workspace`, or an explicit override), so keep
    // the shared YAML synchronized with that value on every startup.
    config.workspace.root = workspace_root.to_string_lossy().to_string();
    config.workspace.container_roots = defaults.container_roots.clone();
    config.lsp.enabled = false;

    if !defaults.language.trim().is_empty() {
        config.i18n.default_language = defaults.language.trim().to_string();
    }

    // The model list is not restored from `desktop.settings.json`: this config
    // file is shared with 舵机, which writes new entries into it, so the file
    // itself is the source of truth for `llm`.
    // A fresh desktop must expose the virtual replay model immediately, just
    // like the web model catalogue. Existing user models are preserved.
    ensure_default_virtual_replay_model(config);

    // Keep the shared local engine switches identical to 舵机. They are
    // persisted once and are not client-specific startup flags.
    config.channels.enabled = true;
    config.channels.outbox.worker_enabled = true;
    config.gateway.enabled = false;
    config.agent_queue.enabled = true;
    config.cron.enabled = true;
    // New and edited cron jobs wake the local scheduler directly. With no
    // jobs, leave only a long cross-process recovery backstop so an idle
    // desktop does not query its SQLite file every few seconds.
    config.cron.max_idle_sleep_ms = config.cron.max_idle_sleep_ms.max(60_000);
    if let Some(preset_worker_cards_root) = resolve_desktop_preset_worker_cards_root(
        &config.user_agents.worker_cards_root,
        repo_root,
        workspace_root,
    ) {
        config.user_agents.worker_cards_root =
            preset_worker_cards_root.to_string_lossy().to_string();
    }

    if !defaults.desktop_token.trim().is_empty() {
        config.security.api_key = Some(defaults.desktop_token.to_string());
    }

    let repo_skills = state_root.join("skills");
    let admin_custom_skills = state_root.join("admin_skills");
    let mut skill_paths = vec![repo_skills, admin_custom_skills];
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
    for required in resolve_desktop_builtin_skill_names(&config, repo_root) {
        if !config
            .skills
            .enabled
            .iter()
            .any(|name| name.trim() == required)
        {
            config.skills.enabled.push(required.to_string());
        }
    }
    ensure_desktop_builtin_tool(&mut config.tools.builtin.enabled, "计划面板");
    config.tools.desktop_control.enabled = true;
    config.tools.desktop_control.norm_width = config
        .tools
        .desktop_control
        .norm_width
        .max(DESKTOP_CONTROL_MIN_NORM_WIDTH);
    config.tools.desktop_control.norm_height = config
        .tools
        .desktop_control
        .norm_height
        .max(DESKTOP_CONTROL_MIN_NORM_HEIGHT);
    ensure_desktop_builtin_tool(&mut config.tools.builtin.enabled, "桌面控制");
    config.tools.browser.enabled = true;
    // Browser tools now come from the active provider's native surface and are
    // auto-registered, so the legacy Chinese sub-tool whitelist entries are gone.

    let mut allow_paths = config
        .security
        .allow_paths
        .iter()
        .filter(|path| !is_legacy_eva_skills_path(path))
        .cloned()
        .collect::<Vec<_>>();
    allow_paths.push(state_root.join("skills").to_string_lossy().to_string());
    allow_paths.push(
        state_root
            .join("admin_skills")
            .to_string_lossy()
            .to_string(),
    );
    allow_paths.push(workspace_root.to_string_lossy().to_string());
    config.security.allow_paths = dedupe_strings(allow_paths);
    config.security.allow_commands = vec!["*".to_string()];
    config.security.deny_globs.clear();
    config.security.exec_policy_mode = None;
    config.security.approval_mode = Some("full_auto".to_string());
}

fn ensure_default_virtual_replay_model(config: &mut Config) {
    const KEY: &str = "虚拟模型";
    if config.llm.models.contains_key(KEY) {
        let model = config.llm.models.get_mut(KEY).expect("checked above");
        let provider = model.provider.as_deref().unwrap_or_default().trim();
        if provider.is_empty() || provider.eq_ignore_ascii_case("virtual_replay") {
            enrich_virtual_replay_model(model);
            if config.llm.default.trim().is_empty() {
                config.llm.default = KEY.to_string();
            }
        }
        return;
    }
    // Migrate the temporary English key used by an earlier desktop build so
    // upgrades do not leave two identical virtual models in the picker.
    if let Some(legacy) = config.llm.models.remove("virtual-replay") {
        config.llm.models.insert(KEY.to_string(), legacy);
        enrich_virtual_replay_model(config.llm.models.get_mut(KEY).expect("inserted above"));
        if config.llm.default == "virtual-replay" {
            config.llm.default = KEY.to_string();
        } else if config.llm.default.trim().is_empty() {
            config.llm.default = KEY.to_string();
        }
        return;
    }
    let mut model = wunder_server::config::LlmModelConfig {
        provider: Some("virtual_replay".to_string()),
        model_type: Some("llm".to_string()),
        enable: Some(true),
        ..Default::default()
    };
    enrich_virtual_replay_model(&mut model);
    config.llm.models.insert(KEY.to_string(), model);
    if config.llm.default.trim().is_empty() {
        config.llm.default = KEY.to_string();
    }
}

fn enrich_virtual_replay_model(model: &mut wunder_server::config::LlmModelConfig) {
    use wunder_server::stable_core::virtual_model::{VirtualModelOptions, VirtualModelSpeed};
    model.provider = Some("virtual_replay".to_string());
    model.model_type.get_or_insert_with(|| "llm".to_string());
    model.enable = Some(true);
    model
        .simulation_speed
        .get_or_insert(VirtualModelSpeed::Fast);
    model
        .simulation
        .get_or_insert_with(VirtualModelOptions::default);
    model.timeout_s.get_or_insert(120);
    model.max_rounds.get_or_insert(1000);
    model.max_context.get_or_insert(131_072);
    model.max_output.get_or_insert(4096);
    model.support_vision.get_or_insert(true);
    model.stream.get_or_insert(false);
    model.stream_include_usage.get_or_insert(true);
}

fn resolve_desktop_preset_worker_cards_root(
    configured_root: &str,
    repo_root: &Path,
    workspace_root: &Path,
) -> Option<PathBuf> {
    let cleaned = configured_root.trim();
    if cleaned.is_empty() {
        return None;
    }
    let configured_path = resolve_maybe_relative_path(cleaned, repo_root, workspace_root);
    if configured_path.is_dir() {
        Some(configured_path)
    } else {
        None
    }
}

fn resolve_desktop_builtin_skill_names(config: &Config, repo_root: &Path) -> Vec<String> {
    let mut skill_roots = vec![std::env::var_os("WUNDER_HOME")
        .map(PathBuf::from)
        .map(|home| home.join("skills"))
        .unwrap_or_else(|| repo_assets::builtin_skills_root(repo_root))];
    for raw_path in &config.skills.paths {
        let cleaned = raw_path.trim();
        if cleaned.is_empty() {
            continue;
        }
        skill_roots.push(resolve_maybe_relative_path(cleaned, repo_root, repo_root));
    }

    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for root in dedupe_paths(skill_roots) {
        if !root.is_dir() {
            continue;
        }
        for name in read_skill_names_from_root(&root) {
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    }
    names
}

fn read_skill_names_from_root(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let skill_file = entry.path().join("SKILL.md");
        if !skill_file.is_file() {
            continue;
        }
        if let Some(name) = read_skill_name_from_file(&skill_file) {
            names.push(name);
        }
    }
    names
}

fn read_skill_name_from_file(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let normalized = content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_start_matches('\u{feff}')
        .to_string();
    let mut lines = normalized.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut body_lines = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        body_lines.push(line);
    }
    let meta: HashMap<String, serde_yaml::Value> =
        serde_yaml::from_str(&body_lines.join("\n")).ok()?;
    for key in ["name", "名称", "技能名称"] {
        if let Some(name) = meta.get(key).and_then(serde_yaml::Value::as_str) {
            let cleaned = name.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

fn ensure_desktop_builtin_tool(enabled: &mut Vec<String>, required: &str) {
    let required = required.trim();
    if required.is_empty() {
        return;
    }
    let has_required = enabled
        .iter()
        .any(|name| wunder_server::tools::resolve_tool_name(name.trim()) == required);
    if !has_required {
        enabled.push(required.to_string());
    }
}

fn normalize_user_id(raw: Option<&str>) -> String {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return DESKTOP_DEFAULT_USER_ID.to_string();
    };
    UserStore::normalize_user_id(raw).unwrap_or_else(|| DESKTOP_DEFAULT_USER_ID.to_string())
}

fn build_default_lan_peer_id(user_id: &str) -> String {
    let machine = std::env::var("COMPUTERNAME")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "desktop".to_string());
    format!("{machine}-{user_id}")
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

    let now = now_ts();
    let expected_scope = UserStore::default_session_scope();
    if desktop_token_matches_identity(
        state.storage.get_user_token(desktop_token)?.as_ref(),
        user_id,
        expected_scope,
        now,
    ) {
        return Ok(());
    }

    let _ = state.storage.delete_user_token(desktop_token);
    let record = UserTokenRecord {
        token: desktop_token.to_string(),
        user_id: user_id.to_string(),
        session_scope: expected_scope.to_string(),
        family_id: None,
        expires_at: now + 10.0 * 365.0 * 24.0 * 3600.0,
        created_at: now,
        last_used_at: now,
    };
    state.storage.create_user_token(&record)?;
    Ok(())
}

fn desktop_token_matches_identity(
    record: Option<&UserTokenRecord>,
    user_id: &str,
    session_scope: &str,
    now: f64,
) -> bool {
    record.is_some_and(|record| {
        record.user_id == user_id
            && record.session_scope == session_scope
            && record.expires_at > now
    })
}

/// One-time local migration: bind the default workspace to the user
/// directory's own `workspace` subfolder — the working directory — and
/// reattach threads that have no workspace yet. The user directory itself
/// keeps the engine's config folders (agents/global/skills/…), so pointing a
/// workspace there leaked those into the working-directory tree. Safe to call
/// on every startup; existing bindings are repaired once.
fn migrate_local_workspaces(state: &AppState, user_id: &str, legacy_root: &Path) -> Result<()> {
    let storage = state.storage.as_ref();
    let records = storage.list_workspaces(user_id)?;
    // The working directory lives inside the user directory, mirroring the
    // cloud form's `用户目录/workspace` split.
    let workspace_dir = legacy_root.join("workspace");
    let workspace_dir_cmp = normalize_path_for_compare(&workspace_dir);
    let legacy_root_cmp = normalize_path_for_compare(legacy_root);
    let workspace_id = match records.first() {
        Some(first) => {
            // Repair: early builds bound the default workspace to the user
            // directory itself, which made its config folders show up as the
            // working directory. Rebind once, unless another workspace
            // already occupies the subfolder.
            let stale = normalize_path_for_compare(Path::new(&first.root_path)) == legacy_root_cmp;
            let occupied = records.iter().any(|record| {
                normalize_path_for_compare(Path::new(&record.root_path)) == workspace_dir_cmp
            });
            if stale && !occupied {
                fs::create_dir_all(&workspace_dir)?;
                let root = workspace_dir
                    .canonicalize()
                    .unwrap_or_else(|_| workspace_dir.clone());
                let mut record = first.clone();
                record.root_path = root.to_string_lossy().into_owned();
                record.updated_at = now_ts();
                storage.upsert_workspace(&record)?;
                state
                    .workspace
                    .register_workspace_root(&record.workspace_id, &record.root_path);
            }
            first.workspace_id.clone()
        }
        None => {
            fs::create_dir_all(&workspace_dir)?;
            let root = workspace_dir
                .canonicalize()
                .unwrap_or_else(|_| workspace_dir.clone());
            let now = now_ts();
            let record = wunder_server::storage::WorkspaceRecord {
                workspace_id: format!("ws_{}", uuid::Uuid::new_v4().simple()),
                user_id: user_id.to_string(),
                name: DEFAULT_WORKSPACE_NAME.to_string(),
                root_path: root.to_string_lossy().into_owned(),
                icon: "folder".to_string(),
                color: "blue".to_string(),
                sort_index: 0,
                created_at: now,
                updated_at: now,
            };
            storage.upsert_workspace(&record)?;
            state
                .workspace
                .register_workspace_root(&record.workspace_id, &record.root_path);
            record.workspace_id
        }
    };
    // Reattach orphan threads page by page, then rescan from the top: bound
    // rows no longer count as orphans, so the scan strictly shrinks.
    const PAGE: i64 = 200;
    loop {
        let mut orphans = Vec::new();
        let mut offset = 0i64;
        loop {
            let (records, total) = storage.list_chat_sessions(user_id, None, None, offset, PAGE)?;
            let page = records.len();
            for record in records {
                if record.workspace_id.is_none() {
                    orphans.push(record);
                    if orphans.len() >= PAGE as usize {
                        break;
                    }
                }
            }
            offset += page as i64;
            if page == 0 || offset >= total || orphans.len() >= PAGE as usize {
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
            state.storage.upsert_chat_session(&record)?;
        }
    }
    Ok(())
}

/// Merge historic workspace rows which name the same real folder using
/// different serialized paths (most commonly Windows `\\?\\` versus the plain
/// canonical path).  蛹 and 舵机 share this database, so leaving both rows
/// splits the same project in the desktop sidebar and causes new CLI threads
/// to land under a second id. Keep the first row in sidebar order as the stable
/// id and move all threads before deleting its duplicate. Runtime root bindings
/// are process-local and must be restored on every startup.
fn migrate_workspace_path_identity(state: &AppState, user_id: &str) -> Result<()> {
    let migration_key = format!(
        "desktop_workspace_path_identity_migration:{DESKTOP_WORKSPACE_IDENTITY_MIGRATION_VERSION}:{user_id}"
    );
    let storage = state.storage.as_ref();
    if state.user_store.get_meta(&migration_key)?.as_deref() == Some("1") {
        for record in storage.list_workspaces(user_id)? {
            state
                .workspace
                .register_workspace_root(&record.workspace_id, &record.root_path);
        }
        return Ok(());
    }
    let records = storage.list_workspaces(user_id)?;
    let mut canonical_by_key: HashMap<String, String> = HashMap::new();
    // Remove duplicate identities before rewriting roots: otherwise changing a
    // legacy row to the plain spelling can collide with the already canonical
    // row's unique `(user_id, root_path)` index.
    for record in &records {
        let identity = wunder_server::local_bootstrap::local_workspace_identity_key(Path::new(
            &record.root_path,
        ));
        if let Some(keeper) = canonical_by_key.get(&identity).cloned() {
            storage.reassign_chat_sessions_workspace(
                user_id,
                &record.workspace_id,
                &keeper,
                now_ts(),
            )?;
            storage.delete_workspace(user_id, &record.workspace_id)?;
            state
                .workspace
                .unregister_workspace_root(&record.workspace_id);
        }
        canonical_by_key
            .entry(identity)
            .or_insert_with(|| record.workspace_id.clone());
    }
    for mut record in storage.list_workspaces(user_id)? {
        let normalized_root = wunder_server::local_bootstrap::normalize_local_workspace_path(
            Path::new(&record.root_path),
        );
        let normalized_path = normalized_root.to_string_lossy().into_owned();
        if record.root_path != normalized_path {
            record.root_path = normalized_path;
            record.updated_at = now_ts();
            storage.upsert_workspace(&record)?;
        }
        state
            .workspace
            .register_workspace_root(&record.workspace_id, &record.root_path);
    }
    state.user_store.set_meta(&migration_key, "1")?;
    Ok(())
}

fn migrate_desktop_local_agent_approval_modes(state: &AppState, user_id: &str) -> Result<()> {
    let migration_key = format!(
        "desktop_approval_mode_migration:{DESKTOP_APPROVAL_MODE_MIGRATION_VERSION}:{user_id}"
    );
    if state.user_store.get_meta(&migration_key)?.as_deref() == Some("1") {
        return Ok(());
    }

    for mut record in state.user_store.list_user_agents(user_id)? {
        let normalized = record.approval_mode.trim().to_ascii_lowercase();
        if !(normalized.is_empty() || normalized == "auto_edit" || normalized == "auto-edit") {
            continue;
        }
        record.approval_mode = "full_auto".to_string();
        record.updated_at = now_ts();
        state.user_store.upsert_user_agent(&record)?;
    }

    let default_agent_key = format!("default_agent:{user_id}");
    if let Some(raw) = state.user_store.get_meta(&default_agent_key)? {
        let cleaned = raw.trim();
        if let Ok(mut payload) = serde_json::from_str::<serde_json::Value>(cleaned) {
            let approval_mode = payload
                .get("approval_mode")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            let legacy_mode = approval_mode.is_empty()
                || approval_mode == "auto_edit"
                || approval_mode == "auto-edit";
            if let (true, Some(object)) = (legacy_mode, payload.as_object_mut()) {
                object.insert(
                    "approval_mode".to_string(),
                    serde_json::Value::String("full_auto".to_string()),
                );
                state
                    .user_store
                    .set_meta(&default_agent_key, &serde_json::to_string(object)?)?;
            }
        }
    }
    state.user_store.set_meta(&migration_key, "1")?;
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

fn startup_timing_enabled() -> bool {
    match std::env::var("WUNDER_STARTUP_TIMING")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
    {
        Some(value) if matches!(value.as_str(), "1" | "true" | "on" | "yes") => true,
        Some(value) if matches!(value.as_str(), "0" | "false" | "off" | "no") => false,
        Some(_) => true,
        None => true,
    }
}

fn startup_elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn log_startup_segment(
    enabled: bool,
    scope: &str,
    segment: &str,
    started: Instant,
    startup_boot: Instant,
) {
    if !enabled {
        return;
    }
    info!(
        "[startup][{scope}] segment={segment} elapsed_ms={:.1} total_ms={:.1}",
        startup_elapsed_ms(started),
        startup_elapsed_ms(startup_boot),
    );
}

fn log_startup_point(enabled: bool, scope: &str, point: &str, startup_boot: Instant) {
    if !enabled {
        return;
    }
    info!(
        "[startup][{scope}] point={point} total_ms={:.1}",
        startup_elapsed_ms(startup_boot),
    );
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn apply_desktop_defaults_keeps_channels_available() {
        let mut config = Config::default();
        let workspace_root = PathBuf::from("/tmp/wunder-work");
        let temp_root = PathBuf::from("/tmp/wunder-temp");
        let repo_root = PathBuf::from("/tmp/wunder-repo");
        let container_roots = HashMap::new();
        let defaults = DesktopDefaultsInput {
            desktop_token: "desktop-token",
            container_roots: &container_roots,
            language: "",
        };

        apply_desktop_defaults(
            &mut config,
            &workspace_root,
            &temp_root,
            &repo_root,
            defaults,
        );

        assert!(config.channels.enabled);
        assert_eq!(PathBuf::from(&config.workspace.root), workspace_root);
        assert!(config.channels.outbox.worker_enabled);
        assert!(!config.gateway.enabled);
        assert!(config.cron.enabled);
        let virtual_model = config.llm.models.get("虚拟模型").expect("virtual model");
        assert_eq!(virtual_model.provider.as_deref(), Some("virtual_replay"));
        assert_eq!(virtual_model.max_context, Some(131_072));
    }

    #[test]
    fn desktop_token_identity_reuses_only_valid_matching_records() {
        let now = 100.0;
        let record = UserTokenRecord {
            token: "desktop-token".to_string(),
            user_id: "desktop-user".to_string(),
            session_scope: "desktop".to_string(),
            family_id: None,
            expires_at: now + 1.0,
            created_at: now - 1.0,
            last_used_at: now - 1.0,
        };

        assert!(desktop_token_matches_identity(
            Some(&record),
            "desktop-user",
            "desktop",
            now
        ));
        assert!(!desktop_token_matches_identity(
            Some(&record),
            "other-user",
            "desktop",
            now
        ));
        assert!(!desktop_token_matches_identity(
            Some(&record),
            "desktop-user",
            "desktop",
            now + 1.0
        ));
    }

    #[test]
    fn apply_desktop_defaults_restores_controller_capture_precision() {
        let mut config = Config::default();
        config.tools.desktop_control.norm_width = 768;
        config.tools.desktop_control.norm_height = 768;
        let workspace_root = PathBuf::from("/tmp/wunder-work");
        let temp_root = PathBuf::from("/tmp/wunder-temp");
        let repo_root = PathBuf::from("/tmp/wunder-repo");
        let container_roots = HashMap::new();
        let defaults = DesktopDefaultsInput {
            desktop_token: "desktop-token",
            container_roots: &container_roots,
            language: "",
        };

        apply_desktop_defaults(
            &mut config,
            &workspace_root,
            &temp_root,
            &repo_root,
            defaults,
        );

        assert_eq!(config.tools.desktop_control.norm_width, 1000);
        assert_eq!(config.tools.desktop_control.norm_height, 1000);
    }

    #[test]
    fn load_desktop_settings_recovers_from_backup_when_primary_is_invalid() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-settings-recover-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let settings_path = root.join("config/desktop.settings.json");
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings dir");
        fs::write(&settings_path, "{").expect("write invalid settings");
        fs::write(
            desktop_settings_backup_path(&settings_path),
            r#"{"workspace_root":"workspace","desktop_token":"backup-token","updated_at":1.0}"#,
        )
        .expect("write backup settings");

        let settings = load_desktop_settings(&settings_path).expect("load recovered settings");

        assert_eq!(settings.desktop_token, "backup-token");
        assert_eq!(settings.workspace_root, "workspace");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_desktop_settings_uses_defaults_when_invalid_primary_has_no_backup() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-settings-default-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let settings_path = root.join("config/desktop.settings.json");
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings dir");
        fs::write(&settings_path, "{").expect("write invalid settings");

        let settings = load_desktop_settings(&settings_path).expect("load default settings");

        assert_eq!(settings.workspace_root, "");
        assert!(!settings.desktop_token.trim().is_empty());
        assert!(!settings_path.exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn desktop_workspace_root_uses_home_workspace_for_default_and_legacy_seed() {
        let home = std::env::temp_dir()
            .join("desktop-workspace-fixture")
            .join("home/.wunder");

        assert_eq!(
            resolve_workspace_root(None, "", &home).expect("resolve default workspace"),
            home.join("workspace")
        );
        assert_eq!(
            resolve_workspace_root(None, "./config/data/workspaces", &home)
                .expect("migrate legacy workspace seed"),
            home.join("workspace")
        );
        assert_eq!(
            resolve_workspace_root(
                None,
                "${WUNDER_WORKSPACE_ROOT:-./config/data/workspaces}",
                &home,
            )
            .expect("migrate unexpanded legacy workspace seed"),
            home.join("workspace")
        );
        assert_eq!(
            resolve_workspace_root(None, "config\\data\\workspaces\\local_user", &home)
                .expect("migrate legacy scoped workspace seed"),
            home.join("workspace")
        );
    }

    #[test]
    fn desktop_workspace_root_preserves_explicit_user_selection() {
        let fixture = std::env::temp_dir().join("desktop-workspace-fixture");
        let home = fixture.join("home/.wunder");
        let custom = fixture.join("custom-workspace");

        assert_eq!(
            resolve_workspace_root(None, custom.to_string_lossy().as_ref(), &home)
                .expect("keep absolute setting"),
            custom
        );
        assert_eq!(
            resolve_workspace_root(None, "custom-workspace", &home).expect("keep relative setting"),
            home.join("custom-workspace")
        );
        assert_eq!(
            resolve_workspace_root(None, "config/data/workspaces/custom", &home)
                .expect("keep relative setting beneath legacy root"),
            home.join("config/data/workspaces/custom")
        );
        assert_eq!(
            resolve_workspace_root(Some(&custom), "./config/data/workspaces", &home)
                .expect("prefer explicit command-line workspace"),
            custom
        );
    }

    #[test]
    fn desktop_workspace_root_recovers_from_repository_target_workspace() {
        let fixture = std::env::temp_dir().join(format!(
            "desktop-workspace-build-root-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let repository = fixture.join("repository");
        let workspace = repository
            .join("target")
            .join("native-smoke-stack")
            .join("workspace");
        let home = fixture.join("home/.wunder");
        fs::create_dir_all(&workspace).expect("create build workspace");
        fs::create_dir_all(repository.join(".git")).expect("create repository marker");

        assert_eq!(
            resolve_workspace_root(None, workspace.to_string_lossy().as_ref(), &home)
                .expect("recover transient build workspace"),
            home.join("workspace")
        );

        let _ = fs::remove_dir_all(&fixture);
    }

    #[test]
    fn save_desktop_settings_replaces_primary_and_keeps_backup() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-settings-save-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let settings_path = root.join("config/desktop.settings.json");
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings dir");
        fs::write(
            &settings_path,
            r#"{"workspace_root":"old","desktop_token":"old-token","updated_at":1.0}"#,
        )
        .expect("write old settings");
        let settings = DesktopSettings {
            workspace_root: "new".to_string(),
            desktop_token: "new-token".to_string(),
            ..DesktopSettings::default()
        };

        save_desktop_settings(&settings_path, &settings).expect("save settings");

        let primary = load_desktop_settings(&settings_path).expect("load primary settings");
        let backup = load_desktop_settings(&desktop_settings_backup_path(&settings_path))
            .expect("load backup");
        assert_eq!(primary.desktop_token, "new-token");
        assert_eq!(primary.workspace_root, "new");
        assert_eq!(backup.desktop_token, "old-token");
        assert_eq!(backup.workspace_root, "old");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_desktop_defaults_keeps_worker_cards_root_empty_without_explicit_config() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-preset-root-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let workspace_root = root.join("workspace");
        let temp_root = root.join("temp");
        let repo_root = root.join("repo");
        let bundled_root = repo_root.join("config/preset_worker_cards");
        fs::create_dir_all(&bundled_root).expect("create bundled preset root");

        let mut config = Config::default();
        let container_roots = HashMap::new();
        let defaults = DesktopDefaultsInput {
            desktop_token: "desktop-token",
            container_roots: &container_roots,
            language: "",
        };

        apply_desktop_defaults(
            &mut config,
            &workspace_root,
            &temp_root,
            &repo_root,
            defaults,
        );

        assert!(config.user_agents.worker_cards_root.trim().is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_desktop_defaults_respects_explicit_worker_cards_root() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-preset-root-prefer-bundled-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let workspace_root = root.join("workspace");
        let temp_root = root.join("temp");
        let repo_root = root.join("repo");
        let bundled_root = repo_root.join("config/preset_worker_cards");
        let configured_sparse_root = workspace_root.join("custom-preset-cards");

        fs::create_dir_all(&bundled_root).expect("create bundled preset root");
        fs::create_dir_all(&configured_sparse_root).expect("create sparse preset root");
        fs::write(bundled_root.join("preset-a.worker-card.json"), "{}")
            .expect("write bundled preset a");
        fs::write(bundled_root.join("preset-b.worker-card.json"), "{}")
            .expect("write bundled preset b");
        fs::write(bundled_root.join("preset-c.worker-card.json"), "{}")
            .expect("write bundled preset c");
        fs::write(
            configured_sparse_root.join("preset-only.worker-card.json"),
            "{}",
        )
        .expect("write sparse preset");

        let mut config = Config::default();
        config.user_agents.worker_cards_root = configured_sparse_root.to_string_lossy().to_string();
        let container_roots = HashMap::new();
        let defaults = DesktopDefaultsInput {
            desktop_token: "desktop-token",
            container_roots: &container_roots,
            language: "",
        };

        apply_desktop_defaults(
            &mut config,
            &workspace_root,
            &temp_root,
            &repo_root,
            defaults,
        );

        assert_eq!(
            PathBuf::from(&config.user_agents.worker_cards_root),
            configured_sparse_root
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn normalize_desktop_container_roots_uses_isolated_defaults() {
        let workspace_root = PathBuf::from("/tmp/wunder-work");
        let app_dir = PathBuf::from("/tmp/wunder-app");
        let roots =
            normalize_desktop_container_roots(&HashMap::new(), &workspace_root, &app_dir, "alice");

        let user_root = roots
            .get(&USER_PRIVATE_CONTAINER_ID)
            .expect("user root should exist");
        let container_one_root = roots.get(&1).expect("container 1 root should exist");

        assert_eq!(
            user_root,
            &workspace_root.join("alice").to_string_lossy().to_string()
        );
        assert_eq!(
            container_one_root,
            &workspace_root
                .join("alice__c__1")
                .to_string_lossy()
                .to_string()
        );
        assert_ne!(user_root, container_one_root);
    }

    #[test]
    fn normalize_desktop_container_roots_ignores_shared_workspace_root_mapping() {
        let workspace_root = PathBuf::from("/tmp/wunder-work");
        let app_dir = PathBuf::from("/tmp/wunder-app");
        let mut source = HashMap::new();
        source.insert(1, workspace_root.to_string_lossy().to_string());
        source.insert(
            2,
            workspace_root
                .join("alice__c__1")
                .to_string_lossy()
                .to_string(),
        );

        let roots = normalize_desktop_container_roots(&source, &workspace_root, &app_dir, "alice");

        let container_one_root = roots.get(&1).expect("container 1 root should exist");
        let container_two_root = roots.get(&2).expect("container 2 root should exist");

        assert_eq!(
            container_one_root,
            &workspace_root
                .join("alice__c__1")
                .to_string_lossy()
                .to_string()
        );
        assert_eq!(
            container_two_root,
            &workspace_root
                .join("alice__c__2")
                .to_string_lossy()
                .to_string()
        );
        assert_ne!(Path::new(container_one_root), Path::new(container_two_root));
    }

    #[test]
    fn embedded_supplement_roots_prefers_shared_home() {
        let home = PathBuf::from("/tmp/shared-profile");
        let app_dir = PathBuf::from("/tmp/wunder-mount/usr/bin");
        let appimage_dir = PathBuf::from("/tmp/image-fixture");
        let sidecar = Some(appimage_dir.clone());

        let roots = wunder_server::local_bootstrap::supplement::roots(&home, &app_dir, sidecar);
        assert_eq!(
            roots,
            vec![home.clone(), app_dir.clone(), appimage_dir.clone()]
        );

        // A duplicate AppImage root (or no AppImage at all) must not be added twice.
        assert_eq!(
            wunder_server::local_bootstrap::supplement::roots(
                &home,
                &app_dir,
                Some(app_dir.clone())
            ),
            vec![home.clone(), app_dir.clone()]
        );
        assert_eq!(
            wunder_server::local_bootstrap::supplement::roots(&home, &app_dir, None),
            vec![home, app_dir]
        );
    }

    #[test]
    fn resolve_embedded_python_bin_finds_supplement_in_app_dir() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-python-supplement-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let python_bin = if cfg!(windows) {
            root.join("opt/python/python.exe")
        } else {
            root.join("opt/python/bin/python3")
        };
        fs::create_dir_all(python_bin.parent().expect("python parent")).expect("create python dir");
        fs::write(&python_bin, "").expect("create python stub");

        let resolved = resolve_embedded_python_bin(&root).expect("embedded python should resolve");
        assert_eq!(resolved, python_bin);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_embedded_rg_bin_finds_supplement_beside_appimage() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-rg-supplement-{}",
            uuid::Uuid::new_v4().simple()
        ));
        // Simulate the AppImage layout: the executable lives on a read-only
        // mount while the supplement archive was extracted beside the image.
        let app_dir = root.join("mount/usr/bin");
        let appimage_dir = root.join("apps");
        let image_file = appimage_dir.join("wunder-slint.AppImage");
        fs::create_dir_all(&app_dir).expect("create mount dir");
        fs::create_dir_all(&appimage_dir).expect("create apps dir");
        fs::write(&image_file, "").expect("create appimage stub");
        let rg_bin = if cfg!(windows) {
            appimage_dir.join("opt/rg/bin/rg.exe")
        } else {
            appimage_dir.join("opt/rg/bin/rg")
        };
        fs::create_dir_all(rg_bin.parent().expect("rg parent")).expect("create rg dir");
        fs::write(&rg_bin, "").expect("create rg stub");

        let roots = wunder_server::local_bootstrap::supplement::roots(
            &root.join("profile"),
            &app_dir,
            Some(appimage_dir.clone()),
        );
        assert!(roots.contains(&appimage_dir));
        assert_eq!(
            wunder_server::local_bootstrap::supplement::find_tool(
                &roots,
                wunder_server::local_bootstrap::supplement::Tool::Rg
            ),
            Some(rg_bin)
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn runtime_tool_statuses_labels_embedded_git_and_rg() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-tool-status-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let app_dir = root.join("app");
        let exe = if cfg!(windows) { ".exe" } else { "" };
        let python_bin = app_dir.join(if cfg!(windows) {
            "opt/python/python.exe"
        } else {
            "opt/python/bin/python3"
        });
        let git_bin = app_dir.join(format!("opt/git/cmd/git{exe}"));
        let rg_bin = app_dir.join(format!("opt/rg/rg{exe}"));
        fs::create_dir_all(python_bin.parent().expect("python parent")).expect("create python dir");
        fs::write(&python_bin, "").expect("create python stub");
        fs::create_dir_all(git_bin.parent().expect("git parent")).expect("create git dir");
        fs::write(&git_bin, "").expect("create git stub");
        fs::create_dir_all(rg_bin.parent().expect("rg parent")).expect("create rg dir");
        fs::write(&rg_bin, "").expect("create rg stub");

        let settings = DesktopSettings::default();
        let statuses = runtime_tool_statuses(&settings, &app_dir);
        let git = statuses
            .iter()
            .find(|entry| entry.tool == "git")
            .expect("git status");
        assert_eq!(git.source, "embedded");
        assert_eq!(git.effective, git_bin.to_string_lossy());
        let rg = statuses
            .iter()
            .find(|entry| entry.tool == "rg")
            .expect("rg status");
        assert_eq!(rg.source, "embedded");
        assert_eq!(rg.effective, rg_bin.to_string_lossy());
        let python = statuses
            .iter()
            .find(|entry| entry.tool == "python")
            .expect("python status");
        assert_eq!(python.source, "embedded");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn runtime_tool_statuses_prefers_configured_paths() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-custom-tool-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let app_dir = root.join("app");
        fs::create_dir_all(&app_dir).expect("create app dir");
        let python_bin = root.join(if cfg!(windows) {
            "my-python.exe"
        } else {
            "my-python"
        });
        let git_bin = root.join(if cfg!(windows) {
            "my-git.exe"
        } else {
            "my-git"
        });
        let rg_bin = root.join(if cfg!(windows) { "my-rg.exe" } else { "my-rg" });
        fs::write(&python_bin, "").expect("create python stub");
        fs::write(&git_bin, "").expect("create git stub");
        fs::write(&rg_bin, "").expect("create rg stub");

        let mut settings = DesktopSettings::default();
        settings.python_path = python_bin.to_string_lossy().into_owned();
        settings.git_path = git_bin.to_string_lossy().into_owned();
        settings.rg_path = rg_bin.to_string_lossy().into_owned();
        let statuses = runtime_tool_statuses(&settings, &app_dir);
        for entry in &statuses {
            assert_eq!(
                entry.source, "custom",
                "{} should use the configured path",
                entry.tool
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn seed_desktop_config_uses_repo_preconfig_when_missing() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-seed-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let app_dir = root.join("app");
        let repo_root = root.join("repo");
        let config_path = root.join("home/config/wunder.yaml");
        let docs_dir = repo_root.join("docs/分发");

        fs::create_dir_all(&app_dir).expect("create app dir");
        fs::create_dir_all(&docs_dir).expect("create docs dir");
        fs::write(
            docs_dir.join("预配置文件.yml"),
            "llm:\n  default: seeded_model\n",
        )
        .expect("write preconfig");

        prepare_runtime_config_path(&repo_root, &root.join("home"), &app_dir).expect("seed config");

        let content = fs::read_to_string(&config_path).expect("read config");
        assert!(content.contains("seeded_model"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn seed_desktop_config_keeps_existing_config() {
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-seed-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let app_dir = root.join("app");
        let repo_root = root.join("repo");
        let config_path = root.join("home/config/wunder.yaml");
        let docs_dir = repo_root.join("docs/分发");

        fs::create_dir_all(&app_dir).expect("create app dir");
        fs::create_dir_all(&docs_dir).expect("create docs dir");
        fs::create_dir_all(config_path.parent().expect("config parent"))
            .expect("create config dir");
        fs::write(&config_path, "llm:\n  default: existing_model\n")
            .expect("write existing config");
        fs::write(
            docs_dir.join("预配置文件.yml"),
            "llm:\n  default: seeded_model\n",
        )
        .expect("write preconfig");

        prepare_runtime_config_path(&repo_root, &root.join("home"), &app_dir).expect("seed config");

        let content = fs::read_to_string(&config_path).expect("read config");
        assert!(content.contains("existing_model"));
        assert!(!content.contains("seeded_model"));

        let _ = fs::remove_dir_all(&root);
    }
}
