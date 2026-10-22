//! Shared bootstrap contract for the packaged local forms.
//!
//! The executable contains the immutable runtime assets; this module defines
//! the mutable on-disk projection both 蛹 and 舵机 must create before the
//! engine starts. Keeping the manifest here prevents the first launched form
//! from choosing a client-specific layout.

use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub mod supplement;

pub const HOME_ENV: &str = "WUNDER_HOME";
/// Sentinel written by the installers at the installation root. Its presence
/// makes a local form portable: the installation root also becomes WUNDER_HOME
/// (unless the user explicitly set WUNDER_HOME), so binaries, config, sessions,
/// workspaces and bundled tools never silently fall back to the user profile.
pub const HOME_MARKER: &str = ".wunder-home";
pub const CONFIG_FILE: &str = "config/wunder.yaml";
pub const STORAGE_ROOT: &str = "data";

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
    pub storage_root: PathBuf,
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
            storage_root: home.join(STORAGE_ROOT),
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

fn home_env() -> Option<PathBuf> {
    std::env::var_os(HOME_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// Resolve a portable installation root from an application directory. The
/// installers place the binaries in `<root>/bin` and the marker at `<root>`, so
/// both `<app_dir>/.wunder-home` (flat layout) and the parent are accepted.
pub fn portable_home_from_app_dir(app_dir: &Path) -> Option<PathBuf> {
    if app_dir.join(HOME_MARKER).is_file() {
        return Some(app_dir.to_path_buf());
    }
    app_dir
        .parent()
        .filter(|parent| parent.join(HOME_MARKER).is_file())
        .map(Path::to_path_buf)
}

fn portable_home_from_exe() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(app_dir) = exe.parent() {
            if let Some(home) = portable_home_from_app_dir(app_dir) {
                return Some(home);
            }
        }
    }
    // An AppImage runs from a temporary squashfs mount, so current_exe is not
    // the installed file. APPIMAGE points at the real AppImage path.
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        let path = PathBuf::from(appimage);
        if path.is_absolute() {
            if let Some(app_dir) = path.parent() {
                if let Some(home) = portable_home_from_app_dir(app_dir) {
                    return Some(home);
                }
            }
        }
    }
    None
}

/// Adopt the portable installation root as WUNDER_HOME before any runtime path
/// is resolved. An explicit non-empty WUNDER_HOME always wins.
pub fn install_local_home_env() {
    if home_env().is_some() {
        return;
    }
    if let Some(home) = portable_home_from_exe() {
        std::env::set_var(HOME_ENV, home);
    }
}

pub fn resolve_home_from_env_or_user() -> PathBuf {
    if let Some(value) = home_env() {
        return value;
    }
    if let Some(value) = portable_home_from_exe() {
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

/// Canonical identity for a local workspace folder shared by 蛹 and 舵机.
///
/// Windows `std::fs::canonicalize` may return the extended `\\\\?\\` spelling,
/// while command-line arguments and persisted settings normally use the plain
/// spelling.  Those paths address the same directory but SQLite uniqueness is
/// textual, so retaining the prefix creates duplicate workspace records.  Keep
/// this normalization at the shared local-form boundary instead of letting
/// each client serialize its own path representation.
pub fn normalize_local_workspace_path(path: &Path) -> PathBuf {
    let plain = strip_windows_verbatim_prefix(path);
    plain
        .canonicalize()
        .map(|canonical| strip_windows_verbatim_prefix(&canonical))
        .unwrap_or(plain)
}

/// Stable comparison key for a local workspace. The filesystem identity is
/// canonicalized first; Windows comparisons are case-insensitive.
pub fn local_workspace_identity_key(path: &Path) -> String {
    let mut value = normalize_local_workspace_path(path)
        .to_string_lossy()
        .into_owned();
    // A backslash is a legal Unix filename character. Folding it there could
    // merge two distinct folders such as a literal `a\b` and nested `a/b`.
    #[cfg(windows)]
    {
        value = value.replace('\\', "/");
        value.make_ascii_lowercase();
    }
    while value.len() > 1 && value.ends_with('/') {
        value.pop();
    }
    value
}

fn strip_windows_verbatim_prefix(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc}"));
    }
    value
        .strip_prefix(r"\\?\")
        .map(PathBuf::from)
        .unwrap_or_else(|| path.to_path_buf())
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
    // Publish a complete staging inode with create-if-absent semantics. A
    // check followed by rename would overwrite another client's first write.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staging = target.with_file_name(format!(
        ".asset-{}-{stamp}-{sequence}.tmp",
        std::process::id()
    ));
    // These bytes are immutable bundled seeds. The destination is linked only
    // after the complete staging write, so competing processes cannot observe
    // a partial publication. These reconstructible seeds do not promise fsync
    // durability across power loss. Avoiding a per-asset fsync is material on
    // the first launch with hundreds of small files. Canonical history uses
    // its own flush barriers; credentials still use strict atomic writes.
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&staging)?;
    file.write_all(bytes)?;
    file.flush()?;
    drop(file);
    let result = fs::hard_link(&staging, target);
    let _ = fs::remove_file(&staging);
    match result {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        // Windows can report AccessDenied while the winning publisher removes
        // its staging link. A complete destination is already published then;
        // preserve it just as in the initial existence check. Do not hide a
        // permission error when no regular destination file exists.
        Err(_) if target.is_file() => Ok(false),
        Err(error) => Err(error.into()),
    }
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
        assert_eq!(layout.storage_root, home.join("data"));
        assert_eq!(layout.skills, home.join("skills"));
        assert_eq!(layout.user_tools, home.join("user_tools"));
        assert_eq!(layout.vector_knowledge, home.join("vector_knowledge"));
    }

    #[test]
    fn portable_home_marker_uses_installation_root() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("wunder");
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        assert_eq!(portable_home_from_app_dir(&bin), None);
        fs::write(root.join(HOME_MARKER), b"portable\n").unwrap();
        assert_eq!(portable_home_from_app_dir(&bin), Some(root));
    }

    #[test]
    fn portable_home_marker_accepts_flat_layout() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().join("flat");
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(app_dir.join(HOME_MARKER), b"").unwrap();
        assert_eq!(portable_home_from_app_dir(&app_dir), Some(app_dir));
    }

    #[test]
    fn release_does_not_overwrite_user_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("config/wunder.yaml");
        assert!(release_file_if_missing(&file, b"first").unwrap());
        assert!(!release_file_if_missing(&file, b"second").unwrap());
        assert_eq!(fs::read(file).unwrap(), b"first");
    }

    #[test]
    fn workspace_identity_trims_windows_verbatim_prefix_without_requiring_disk_access() {
        let extended = PathBuf::from(r"\\?\C:\work\project");
        let plain = PathBuf::from(r"C:\work\project");
        assert_eq!(normalize_local_workspace_path(&extended), plain);
        assert_eq!(
            local_workspace_identity_key(&extended),
            local_workspace_identity_key(&plain)
        );
        #[cfg(windows)]
        assert_eq!(
            local_workspace_identity_key(Path::new(r"\\?\c:\WORK\project\\")),
            local_workspace_identity_key(&plain)
        );
    }

    #[test]
    fn workspace_identity_preserves_unc_roots() {
        let extended = PathBuf::from(r"\\?\UNC\fixture-host\fixture-share\folder");
        let plain = PathBuf::from(r"\\fixture-host\fixture-share\folder");
        // Test spelling conversion directly; normalizing an unavailable UNC
        // location would otherwise involve a network filesystem probe.
        assert_eq!(strip_windows_verbatim_prefix(&extended), plain);
    }

    #[cfg(not(windows))]
    #[test]
    fn workspace_identity_preserves_unix_backslash_names() {
        let root = tempdir().unwrap();
        assert_ne!(
            local_workspace_identity_key(&root.path().join(r"fixture\folder")),
            local_workspace_identity_key(&root.path().join("fixture/folder"))
        );
    }

    #[test]
    fn concurrent_publishers_preserve_one_complete_file() {
        let dir = tempdir().unwrap();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let run = |payload: &[u8]| {
                let mut results = Vec::new();
                for index in 0..128 {
                    let file = dir.path().join(format!("asset-{index}"));
                    barrier.wait();
                    results.push(release_file_if_missing(&file, payload));
                }
                results
            };
            let left = scope.spawn(move || run(b"first complete payload"));
            let right = scope.spawn(move || run(b"second complete payload"));
            let left = left.join().unwrap();
            let right = right.join().unwrap();
            let published: usize = left
                .into_iter()
                .chain(right)
                .map(|r| usize::from(r.unwrap()))
                .sum();
            assert_eq!(published, 128);
        });
        for index in 0..128 {
            let bytes = fs::read(dir.path().join(format!("asset-{index}"))).unwrap();
            assert!(bytes == b"first complete payload" || bytes == b"second complete payload");
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 128);
    }
}
