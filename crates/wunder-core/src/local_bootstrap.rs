//! Shared bootstrap contract for the packaged local forms.
//!
//! The executable contains the immutable runtime assets; this module defines
//! the mutable on-disk projection both 蛹 and 舵机 must create before the
//! engine starts. Keeping the manifest here prevents the first launched form
//! from choosing a client-specific layout.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const HOME_ENV: &str = "WUNDER_HOME";
pub const CONFIG_FILE: &str = "config/wunder.yaml";
pub const DATABASE_FILE: &str = "data/wunder.db";

/// Mutable directories shared by the two local forms. Immutable bundled
/// prompts and skills are projected to the same home so runtime path
/// resolution is identical after the first launch.
pub const SHARED_DIRS: &[&str] = &[
    "config",
    "config/prompts",
    "config/skills",
    "config/data",
    "data",
    "logs",
    "skills",
    "user_tools",
    "vector_knowledge",
    "workspace",
    "temp",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalHomeLayout {
    pub home: PathBuf,
    pub config: PathBuf,
    pub database: PathBuf,
    pub logs: PathBuf,
    pub skills: PathBuf,
    pub user_tools: PathBuf,
    pub vector_knowledge: PathBuf,
    pub workspace: PathBuf,
    pub temp: PathBuf,
}

impl LocalHomeLayout {
    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            config: home.join("config"),
            database: home.join(DATABASE_FILE),
            logs: home.join("logs"),
            skills: home.join("skills"),
            user_tools: home.join("user_tools"),
            vector_knowledge: home.join("vector_knowledge"),
            workspace: home.join("workspace"),
            temp: home.join("temp"),
            home,
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.home.join(CONFIG_FILE)
    }

    /// Create only bounded, predictable directories. No recursive scan or
    /// client-specific sub-root is involved in the hot startup path.
    pub fn ensure_dirs(&self) -> Result<()> {
        for relative in SHARED_DIRS {
            fs::create_dir_all(self.home.join(relative))
                .with_context(|| format!("create local runtime directory: {}", relative))?;
        }
        Ok(())
    }
}

pub fn resolve_home_from_env_or_user() -> PathBuf {
    if let Some(value) = std::env::var_os(HOME_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return value;
    }
    #[cfg(windows)]
    let user_home = std::env::var_os("USERPROFILE").or_else(|| {
        let drive = std::env::var_os("HOMEDRIVE")?;
        let path = std::env::var_os("HOMEPATH")?;
        Some(format!("{}{}", drive.to_string_lossy(), path.to_string_lossy()).into())
    });
    #[cfg(not(windows))]
    let user_home = std::env::var_os("HOME");
    user_home
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".wunder")
}

/// Copy one immutable bundled file into the mutable home only when absent.
/// Existing user edits always win and are never overwritten on startup.
pub fn release_file_if_missing(target: &Path, bytes: &[u8]) -> Result<bool> {
    if target.exists() {
        return Ok(false);
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::atomic_write::atomic_write_bytes(target, bytes)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn layout_is_identical_for_both_clients() {
        let home = tempdir().unwrap().path().to_path_buf();
        let layout = LocalHomeLayout::from_home(&home);
        assert_eq!(layout.config_file(), home.join("config/wunder.yaml"));
        assert_eq!(layout.database, home.join("data/wunder.db"));
        assert_eq!(layout.skills, home.join("skills"));
        assert_eq!(layout.user_tools, home.join("user_tools"));
        assert_eq!(layout.vector_knowledge, home.join("vector_knowledge"));
    }

    #[test]
    fn release_does_not_overwrite_user_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("config/wunder.yaml");
        assert!(release_file_if_missing(&file, b"first").unwrap());
        assert!(!release_file_if_missing(&file, b"second").unwrap());
        assert_eq!(fs::read(file).unwrap(), b"first");
    }
}
