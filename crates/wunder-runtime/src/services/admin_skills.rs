use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::core::repo_assets;
use crate::path_utils::{
    normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub const BUILTIN_SKILLS_ROOT_ENV: &str = "WUNDER_BUILTIN_SKILLS_ROOT";
pub const ADMIN_CUSTOM_SKILLS_ROOT_ENV: &str = "WUNDER_ADMIN_CUSTOM_SKILLS_ROOT";

const SKILL_FILE_NAME: &str = "SKILL.md";

#[derive(Debug, Default)]
struct LegacyMigrationReport {
    moved: Vec<String>,
    skipped_conflicts: Vec<String>,
}

pub fn resolve_builtin_skills_root_path() -> PathBuf {
    if let Some(path) = std::env::var(BUILTIN_SKILLS_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return path;
    }
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    repo_assets::builtin_skills_root(&repo_root)
}

pub fn resolve_builtin_skills_root() -> Option<PathBuf> {
    let path = resolve_builtin_skills_root_path();
    if path.exists() && path.is_dir() {
        Some(normalize_existing_path(&path))
    } else {
        None
    }
}

pub fn resolve_admin_custom_skills_root_path() -> PathBuf {
    std::env::var(ADMIN_CUSTOM_SKILLS_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config").join("data").join("admin_skills"))
}

pub fn resolve_admin_custom_skills_root() -> Option<PathBuf> {
    let path = resolve_admin_custom_skills_root_path();
    if path.exists() && path.is_dir() {
        Some(normalize_existing_path(&path))
    } else {
        None
    }
}

pub fn resolve_admin_uploaded_skills_root(use_builtin_root: bool) -> PathBuf {
    if use_builtin_root {
        resolve_builtin_skills_root_path()
    } else {
        resolve_admin_custom_skills_root_path()
    }
}

pub fn build_admin_skill_scan_paths(config: &Config, use_builtin_root: bool) -> Vec<String> {
    normalize_admin_skill_paths(config.skills.paths.clone(), use_builtin_root)
}

pub fn normalize_admin_skill_paths(paths: Vec<String>, use_builtin_root: bool) -> Vec<String> {
    let builtin_root = resolve_builtin_skills_root_path();
    let legacy_root = resolve_admin_custom_skills_root_path();
    let include_legacy = if use_builtin_root {
        root_contains_skill_dirs(&legacy_root)
    } else {
        true
    };
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    append_skill_scan_path(&mut output, &mut seen, builtin_root);
    if include_legacy {
        append_skill_scan_path(&mut output, &mut seen, legacy_root.clone());
    }
    for raw in paths {
        let cleaned = raw.trim();
        if cleaned.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(cleaned);
        if path_equals(&candidate, &legacy_root) {
            if include_legacy {
                continue;
            }
            continue;
        }
        if path_equals(&candidate, &resolve_builtin_skills_root_path()) {
            continue;
        }
        append_skill_scan_path(&mut output, &mut seen, candidate);
    }
    output
}

pub fn collect_admin_reserved_skill_top_dirs(
    config: &Config,
    use_builtin_root: bool,
) -> HashSet<String> {
    let mut dir_names = HashSet::new();
    for raw_path in build_admin_skill_scan_paths(config, use_builtin_root) {
        let root = PathBuf::from(raw_path);
        dir_names.extend(list_skill_dir_names(&root));
    }
    dir_names
}

pub async fn normalize_server_admin_skill_layout(config_store: &ConfigStore) -> Config {
    let current = config_store.get().await;
    match normalize_server_admin_skill_layout_inner(config_store, current.clone()).await {
        Ok(config) => config,
        Err(err) => {
            warn!("normalize server admin skill layout failed: {err}");
            current
        }
    }
}

async fn normalize_server_admin_skill_layout_inner(
    config_store: &ConfigStore,
    current: Config,
) -> Result<Config> {
    let builtin_root = resolve_builtin_skills_root_path();
    let legacy_root = resolve_admin_custom_skills_root_path();
    let report = migrate_legacy_admin_skill_dirs(&legacy_root, &builtin_root)?;
    if !report.moved.is_empty() {
        info!(
            "migrated legacy admin skills into builtin root: {}",
            report.moved.join(", ")
        );
    }
    if !report.skipped_conflicts.is_empty() {
        warn!(
            "legacy admin skills kept in compatibility root because builtin targets already exist: {}",
            report.skipped_conflicts.join(", ")
        );
    }
    let normalized_paths = normalize_admin_skill_paths(current.skills.paths.clone(), true);
    if normalized_paths == current.skills.paths {
        return Ok(current);
    }
    let updated = config_store
        .update(|config| {
            config.skills.paths = normalized_paths.clone();
        })
        .await
        .context("persist normalized admin skill paths failed")?;
    Ok(updated)
}

fn migrate_legacy_admin_skill_dirs(
    legacy_root: &Path,
    builtin_root: &Path,
) -> Result<LegacyMigrationReport> {
    let mut report = LegacyMigrationReport::default();
    if !legacy_root.exists() || !legacy_root.is_dir() {
        return Ok(report);
    }
    let legacy_dirs = list_skill_dirs(legacy_root);
    if legacy_dirs.is_empty() {
        return Ok(report);
    }
    fs::create_dir_all(builtin_root).with_context(|| {
        format!(
            "create builtin skills root failed: {}",
            builtin_root.display()
        )
    })?;
    for skill_dir in legacy_dirs {
        let Some(name) = skill_dir.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let target = builtin_root.join(name);
        if target.exists() {
            report.skipped_conflicts.push(name.to_string());
            continue;
        }
        move_dir(skill_dir.as_path(), target.as_path())?;
        report.moved.push(name.to_string());
    }
    if report.skipped_conflicts.is_empty() && root_is_empty(legacy_root) {
        let _ = fs::remove_dir_all(legacy_root);
    }
    Ok(report)
}

fn move_dir(source: &Path, target: &Path) -> Result<()> {
    fs::rename(source, target).with_context(|| {
        format!(
            "move admin skill dir failed: {} -> {}",
            source.display(),
            target.display()
        )
    })
}

fn root_contains_skill_dirs(root: &Path) -> bool {
    !list_skill_dirs(root).is_empty()
}

fn list_skill_dir_names(root: &Path) -> HashSet<String> {
    list_skill_dirs(root)
        .into_iter()
        .filter_map(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.to_string())
        })
        .collect()
}

fn list_skill_dirs(root: &Path) -> Vec<PathBuf> {
    if !root.exists() || !root.is_dir() {
        return Vec::new();
    }
    let mut dirs = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return dirs;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join(SKILL_FILE_NAME).is_file() {
            dirs.push(path);
        }
    }
    dirs.sort_by_key(|path| path.to_string_lossy().to_string());
    dirs
}

fn append_skill_scan_path(paths: &mut Vec<String>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    let normalized = normalize_path_for_compare(&normalize_target_path(&path));
    if !seen.insert(normalized) {
        return;
    }
    paths.push(path.to_string_lossy().to_string());
}

fn path_equals(left: &Path, right: &Path) -> bool {
    normalize_path_for_compare(&normalize_target_path(left))
        == normalize_path_for_compare(&normalize_target_path(right))
}

fn root_is_empty(root: &Path) -> bool {
    match fs::read_dir(root) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_admin_skill_paths, normalize_server_admin_skill_layout_inner,
        resolve_admin_custom_skills_root_path, resolve_builtin_skills_root_path,
        ADMIN_CUSTOM_SKILLS_ROOT_ENV, BUILTIN_SKILLS_ROOT_ENV,
    };
    use crate::config_store::ConfigStore;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        builtin: Option<String>,
        legacy: Option<String>,
    }

    impl EnvGuard {
        fn set(builtin: &Path, legacy: &Path) -> Self {
            let guard = Self {
                builtin: std::env::var(BUILTIN_SKILLS_ROOT_ENV).ok(),
                legacy: std::env::var(ADMIN_CUSTOM_SKILLS_ROOT_ENV).ok(),
            };
            std::env::set_var(BUILTIN_SKILLS_ROOT_ENV, builtin);
            std::env::set_var(ADMIN_CUSTOM_SKILLS_ROOT_ENV, legacy);
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.builtin.as_deref() {
                std::env::set_var(BUILTIN_SKILLS_ROOT_ENV, value);
            } else {
                std::env::remove_var(BUILTIN_SKILLS_ROOT_ENV);
            }
            if let Some(value) = self.legacy.as_deref() {
                std::env::set_var(ADMIN_CUSTOM_SKILLS_ROOT_ENV, value);
            } else {
                std::env::remove_var(ADMIN_CUSTOM_SKILLS_ROOT_ENV);
            }
        }
    }

    #[test]
    fn normalize_admin_skill_paths_prefers_builtin_and_drops_empty_legacy_root() {
        let _lock = env_lock().lock().expect("lock env");
        let dir = tempdir().expect("tempdir");
        let builtin = dir.path().join("builtin");
        let legacy = dir.path().join("legacy");
        std::fs::create_dir_all(&builtin).expect("create builtin");
        std::fs::create_dir_all(&legacy).expect("create legacy");
        let _guard = EnvGuard::set(&builtin, &legacy);

        let paths = normalize_admin_skill_paths(
            vec![
                builtin.to_string_lossy().to_string(),
                legacy.to_string_lossy().to_string(),
                dir.path().join("external").to_string_lossy().to_string(),
            ],
            true,
        );

        assert_eq!(
            paths[0],
            resolve_builtin_skills_root_path().to_string_lossy()
        );
        assert!(!paths.iter().any(|value| value == &legacy.to_string_lossy()));
        assert!(paths
            .iter()
            .any(|value| value == &dir.path().join("external").to_string_lossy()));
    }

    #[tokio::test]
    async fn normalize_server_admin_skill_layout_moves_legacy_skill_and_cleans_path() {
        let _lock = env_lock().lock().expect("lock env");
        let dir = tempdir().expect("tempdir");
        let builtin = dir.path().join("builtin");
        let legacy = dir.path().join("legacy");
        let config_path = dir.path().join("config").join("wunder.yaml");
        std::fs::create_dir_all(builtin.as_path()).expect("create builtin");
        std::fs::create_dir_all(legacy.join("demo-skill")).expect("create legacy skill dir");
        std::fs::write(
            legacy.join("demo-skill").join("SKILL.md"),
            "---\nname: demo-skill\ndescription: demo\n---\n",
        )
        .expect("write skill");
        let _guard = EnvGuard::set(&builtin, &legacy);

        let config_store = ConfigStore::new(config_path);
        let updated = config_store
            .update(|config| {
                config.skills.paths = vec![
                    resolve_builtin_skills_root_path()
                        .to_string_lossy()
                        .to_string(),
                    resolve_admin_custom_skills_root_path()
                        .to_string_lossy()
                        .to_string(),
                ];
            })
            .await
            .expect("seed config");
        let normalized = normalize_server_admin_skill_layout_inner(&config_store, updated)
            .await
            .expect("normalize layout");

        assert!(builtin.join("demo-skill").join("SKILL.md").is_file());
        assert!(!legacy.join("demo-skill").exists());
        assert_eq!(
            normalized.skills.paths,
            vec![resolve_builtin_skills_root_path()
                .to_string_lossy()
                .to_string()]
        );
    }
}
