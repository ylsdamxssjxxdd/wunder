//! Shared bootstrap contract for the packaged local forms.
//!
//! The executable contains the immutable runtime assets; this module defines
//! the mutable on-disk projection both 蛹 and 舵机 must create before the
//! engine starts. Keeping the manifest here prevents the first launched form
//! from choosing a client-specific layout.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    let result = publish_staged(&staging, target);
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

/// Publish a completely written staging inode at `target`.
///
/// A hard link lets two competing publishers serialize on the destination path
/// without either ever overwriting the other. Android's SELinux
/// `untrusted_app` domain denies `link()` on app data (API 36), while rename
/// stays permitted, so the mobile form publishes with rename instead; the
/// single-activity app has no second process racing the same home directory.
#[cfg(not(target_os = "android"))]
fn publish_staged(staging: &Path, target: &Path) -> std::io::Result<()> {
    fs::hard_link(staging, target)
}

#[cfg(target_os = "android")]
fn publish_staged(staging: &Path, target: &Path) -> std::io::Result<()> {
    if target.exists() {
        return Err(std::io::ErrorKind::AlreadyExists.into());
    }
    fs::rename(staging, target)
}

// ---------------------------------------------------------------------------
// Version-tracked release.
//
// The seed above is create-if-absent only: once a bundled prompt, skill or
// companion exists on disk it can never follow a new binary. That protects user
// edits but also freezes stale system content forever. The release manifest
// below remembers the exact bytes that were released, so startup can tell three
// states apart:
//
//   * target missing              -> release the bundled bytes
//   * target == released bytes    -> untouched system copy, replace with the new
//                                    binary's bytes (system always follows the
//                                    build)
//   * target != released bytes    -> user edited, never overwrite; if the new
//                                    build changed too, export the new version to
//                                    temp/asset-conflicts/ for comparison
//
// Assets removed from a build are deleted only when they are still untouched.
// User deletions are remembered as tombstones so a deleted prompt does not
// reappear on every restart.
// ---------------------------------------------------------------------------

pub const RELEASE_MANIFEST_FILE: &str = ".release-manifest.json";
pub const RELEASE_MANIFEST_LOCK: &str = ".release-manifest.lock";
/// Single-file anchor for the whole released asset set. When it matches the
/// running build, startup reads one small JSON instead of stat-ing every asset.
pub const RELEASE_ANCHOR_FILE: &str = ".release-anchor.json";
const RELEASE_MANIFEST_SCHEMA: u32 = 1;
const RELEASE_CONFLICT_DIR: &str = "temp/asset-conflicts";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReleaseManifest {
    #[serde(default = "release_manifest_schema")]
    pub schema: u32,
    #[serde(default)]
    pub assets: BTreeMap<String, ReleaseRecord>,
}

fn release_manifest_schema() -> u32 {
    RELEASE_MANIFEST_SCHEMA
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReleaseRecord {
    /// Hash of the bytes this build released as the system baseline.
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub deleted_by_user: bool,
    /// The on-disk copy is a user version and must never be replaced.
    #[serde(default)]
    pub user_modified: bool,
    /// Hash of the user version, so repeat starts can skip reading it.
    #[serde(default)]
    pub user_sha256: String,
    /// Last observed file size, used as a cheap "unchanged" fast path.
    #[serde(default)]
    pub size: u64,
    /// Last observed modification time in nanoseconds since the Unix epoch.
    #[serde(default)]
    pub mtime_ns: u64,
}

fn file_signature(path: &Path) -> Option<(u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime_ns = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos() as u64;
    Some((meta.len(), mtime_ns))
}

fn system_record(path: &Path, sha256: String) -> ReleaseRecord {
    let (size, mtime_ns) = file_signature(path).unwrap_or((0, 0));
    ReleaseRecord {
        sha256,
        deleted_by_user: false,
        user_modified: false,
        user_sha256: String::new(),
        size,
        mtime_ns,
    }
}

fn user_record(path: &Path, baseline: String, user_sha256: String) -> ReleaseRecord {
    let (size, mtime_ns) = file_signature(path).unwrap_or((0, 0));
    ReleaseRecord {
        sha256: baseline,
        deleted_by_user: false,
        user_modified: true,
        user_sha256,
        size,
        mtime_ns,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseAction {
    /// Target did not exist and the bundled bytes were published.
    Created,
    /// Target held the previous release verbatim and was replaced.
    Updated,
    /// Target differs from the previous release; the user copy was kept.
    KeptUserModified,
    /// Target was previously deleted by the user; nothing was released.
    KeptUserDeleted,
    /// Bundled asset no longer exists and an untouched copy was removed.
    Removed,
    /// Nothing to do.
    Unchanged,
    /// Seed-only asset already existed.
    Seeded,
}

#[derive(Debug, Clone)]
pub struct ReleaseOutcome {
    pub path: String,
    pub action: ReleaseAction,
    pub conflict: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct ReleaseReport {
    pub outcomes: Vec<ReleaseOutcome>,
    pub conflicts: Vec<PathBuf>,
    /// The build anchor matched the released tree, so no per-file stat was done.
    pub anchor_hit: bool,
}

impl ReleaseReport {
    pub fn count(&self, action: ReleaseAction) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| outcome.action == action)
            .count()
    }

    pub fn changed(&self) -> usize {
        self.count(ReleaseAction::Created)
            + self.count(ReleaseAction::Updated)
            + self.count(ReleaseAction::Removed)
    }

    pub fn conflict_count(&self) -> usize {
        self.conflicts.len()
    }
}

#[derive(Debug, Clone)]
pub struct ReleasePolicy {
    /// Honour a file the user deleted: do not re-release it on the next start.
    pub respect_user_deletes: bool,
}

impl Default for ReleasePolicy {
    fn default() -> Self {
        Self {
            respect_user_deletes: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ReleaseAnchor {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    asset_set_id: String,
}

fn anchor_matches(home: &Path, asset_set_id: &str) -> bool {
    let Ok(bytes) = fs::read(home.join(RELEASE_ANCHOR_FILE)) else {
        return false;
    };
    let Ok(anchor) = serde_json::from_slice::<ReleaseAnchor>(&bytes) else {
        return false;
    };
    anchor.schema == RELEASE_MANIFEST_SCHEMA
        && !anchor.asset_set_id.is_empty()
        && anchor.asset_set_id == asset_set_id
}

fn save_anchor(home: &Path, asset_set_id: &str) -> Result<()> {
    let anchor = ReleaseAnchor {
        schema: RELEASE_MANIFEST_SCHEMA,
        asset_set_id: asset_set_id.to_string(),
    };
    replace_atomic(
        &home.join(RELEASE_ANCHOR_FILE),
        &serde_json::to_vec_pretty(&anchor)?,
    )
}

fn release_seeds(
    home: &Path,
    seeds: &[(String, &'static [u8])],
    report: &mut ReleaseReport,
) -> Result<()> {
    for (relative, bytes) in seeds {
        let published = release_file_if_missing(&home.join(relative), bytes)?;
        report.outcomes.push(ReleaseOutcome {
            path: relative.clone(),
            action: if published {
                ReleaseAction::Created
            } else {
                ReleaseAction::Seeded
            },
            conflict: None,
        });
    }
    Ok(())
}

/// Publish immutable bundled assets with three-state semantics.
///
/// `managed` entries follow the binary forever unless the on-disk copy differs
/// from the last released bytes. `seeds` are create-if-absent only (the user
/// configuration is never replaced).
pub fn release_assets(
    home: &Path,
    managed: &[(String, &'static [u8])],
    seeds: &[(String, &'static [u8])],
    policy: &ReleasePolicy,
) -> Result<ReleaseReport> {
    release_assets_with_id(home, managed, seeds, policy, None)
}

/// Release assets with an optional build anchor.
///
/// With `asset_set_id` present, a matching `.release-anchor.json` plus an intact
/// manifest proves that the whole managed set already matches this binary. The
/// pass then costs one small read and the seed check instead of one stat (and
/// possibly one hash) per bundled file.
pub fn release_assets_with_id(
    home: &Path,
    managed: &[(String, &'static [u8])],
    seeds: &[(String, &'static [u8])],
    policy: &ReleasePolicy,
    asset_set_id: Option<&str>,
) -> Result<ReleaseReport> {
    let mut report = ReleaseReport::default();

    if let Some(asset_set_id) = asset_set_id {
        if home.join(RELEASE_MANIFEST_FILE).is_file() && anchor_matches(home, asset_set_id) {
            report.anchor_hit = true;
            release_seeds(home, seeds, &mut report)?;
            return Ok(report);
        }
    }
    release_seeds(home, seeds, &mut report)?;

    // Concurrent 蛹/舵机 launches share one manifest. When the lock is held by
    // another process this pass still releases files (idempotent) but leaves the
    // bookkeeping to the winner.
    let lock = ReleaseManifestLock::acquire(home);
    let mut manifest = load_release_manifest(home);
    let conflict_root = home.join(RELEASE_CONFLICT_DIR);
    let mut managed_paths = HashSet::new();

    for (relative, bytes) in managed {
        managed_paths.insert(relative.clone());
        let target = home.join(relative);
        let new_hash = sha256_hex(bytes);
        let record = manifest.assets.get(relative.as_str()).cloned();

        if !target.is_file() {
            // A file that was released before and is now gone was deleted by the
            // user. Remember that instead of resurrecting it on every start.
            if policy.respect_user_deletes {
                if let Some(record) = record.as_ref() {
                    if !record.deleted_by_user && lock.is_some() {
                        let mut tombstone = record.clone();
                        tombstone.deleted_by_user = true;
                        manifest.assets.insert(relative.clone(), tombstone);
                    }
                    report.outcomes.push(ReleaseOutcome {
                        path: relative.clone(),
                        action: ReleaseAction::KeptUserDeleted,
                        conflict: None,
                    });
                    continue;
                }
            }
            create_dir_all_for(&target)?;
            let published = release_file_if_missing(&target, bytes)?;
            if lock.is_some() {
                manifest
                    .assets
                    .insert(relative.clone(), system_record(&target, new_hash));
            }
            report.outcomes.push(ReleaseOutcome {
                path: relative.clone(),
                action: if published {
                    ReleaseAction::Created
                } else {
                    ReleaseAction::Unchanged
                },
                conflict: None,
            });
            continue;
        }

        let (size, mtime_ns) = file_signature(&target).unwrap_or((0, 0));

        // A copy already known to be user-owned is never replaced. The recorded
        // size+mtime lets a normal start skip reading it entirely.
        if let Some(record) = record.as_ref().filter(|record| record.user_modified) {
            let signature_matches = record.size == size
                && record.mtime_ns == mtime_ns
                && !record.user_sha256.is_empty();
            let current_hash = if signature_matches {
                record.user_sha256.clone()
            } else {
                sha256_hex(
                    fs::read(&target)
                        .with_context(|| format!("read released asset: {}", target.display()))?
                        .as_slice(),
                )
            };
            if lock.is_some() {
                manifest.assets.insert(
                    relative.clone(),
                    user_record(&target, record.sha256.clone(), current_hash.clone()),
                );
            }
            let mut conflict = None;
            if current_hash != new_hash {
                conflict = export_conflict(&conflict_root, relative, bytes).ok();
                if let Some(path) = conflict.clone() {
                    report.conflicts.push(path);
                }
            }
            report.outcomes.push(ReleaseOutcome {
                path: relative.clone(),
                action: ReleaseAction::KeptUserModified,
                conflict,
            });
            continue;
        }

        // System copy: an unchanged signature proves the bytes are still what the
        // previous build released, so the content never has to be read.
        let signature_proves_untouched = record.as_ref().is_some_and(|record| {
            !record.user_modified
                && !record.sha256.is_empty()
                && record.size == size
                && record.mtime_ns == mtime_ns
        });
        let current_hash = if signature_proves_untouched {
            record
                .as_ref()
                .map(|record| record.sha256.clone())
                .unwrap_or_default()
        } else {
            sha256_hex(
                fs::read(&target)
                    .with_context(|| format!("read released asset: {}", target.display()))?
                    .as_slice(),
            )
        };

        let is_system_copy = match record.as_ref() {
            Some(record) if !record.user_modified && !record.sha256.is_empty() => {
                current_hash == record.sha256
            }
            // First launch with this mechanism, or a corrupted record: compare
            // against this build and keep anything different.
            _ => current_hash == new_hash,
        };

        if is_system_copy {
            if current_hash == new_hash {
                if lock.is_some() {
                    manifest
                        .assets
                        .insert(relative.clone(), system_record(&target, new_hash));
                }
                report.outcomes.push(ReleaseOutcome {
                    path: relative.clone(),
                    action: ReleaseAction::Unchanged,
                    conflict: None,
                });
            } else {
                replace_atomic(&target, bytes)?;
                if lock.is_some() {
                    manifest
                        .assets
                        .insert(relative.clone(), system_record(&target, new_hash));
                }
                report.outcomes.push(ReleaseOutcome {
                    path: relative.clone(),
                    action: ReleaseAction::Updated,
                    conflict: None,
                });
            }
            continue;
        }

        // Different from the released baseline: promote to a user version that
        // will never be overwritten again.
        if lock.is_some() {
            let baseline = record
                .as_ref()
                .map(|record| record.sha256.clone())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| new_hash.clone());
            manifest.assets.insert(
                relative.clone(),
                user_record(&target, baseline, current_hash.clone()),
            );
        }
        let mut conflict = None;
        if current_hash != new_hash {
            conflict = export_conflict(&conflict_root, relative, bytes).ok();
            if let Some(path) = conflict.clone() {
                report.conflicts.push(path);
            }
        }
        report.outcomes.push(ReleaseOutcome {
            path: relative.clone(),
            action: ReleaseAction::KeptUserModified,
            conflict,
        });
    }

    // Assets that disappeared from the build.
    let retired: Vec<String> = manifest
        .assets
        .keys()
        .filter(|relative| !managed_paths.contains(relative.as_str()))
        .cloned()
        .collect();
    for relative in retired {
        let Some(record) = manifest.assets.get(&relative).cloned() else {
            continue;
        };
        let target = home.join(&relative);
        if !target.is_file() {
            manifest.assets.remove(&relative);
            continue;
        }
        let current_hash = fs::read(&target)
            .map(|bytes| sha256_hex(&bytes))
            .unwrap_or_default();
        if record.deleted_by_user
            || (!record.user_modified && current_hash == record.sha256)
        {
            // Untouched system copy (or an already deleted tombstone).
            if record.deleted_by_user {
                manifest.assets.remove(&relative);
                continue;
            }
            match fs::remove_file(&target) {
                Ok(()) => {
                    manifest.assets.remove(&relative);
                    report.outcomes.push(ReleaseOutcome {
                        path: relative,
                        action: ReleaseAction::Removed,
                        conflict: None,
                    });
                }
                Err(_) => {}
            }
        } else {
            // User modified an asset the build no longer ships: hand it back to
            // the user instead of managing it.
            manifest.assets.remove(&relative);
        }
    }

    if lock.is_some() {
        save_release_manifest(home, &manifest)?;
        // Only the process that actually updated the manifest may move the
        // anchor; otherwise a concurrent loser could pin a half-applied state.
        if let Some(asset_set_id) = asset_set_id {
            save_anchor(home, asset_set_id)?;
        }
    }
    drop(lock);
    Ok(report)
}

fn create_dir_all_for(target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn staging_path(target: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    target.with_file_name(format!(
        ".asset-{}-{stamp}-{sequence}.tmp",
        std::process::id()
    ))
}

/// Replace an existing file with complete new bytes. The staging file is fully
/// written first, then published by rename (atomic on the same volume; on
/// Windows `fs::rename` replaces the destination).
fn replace_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    create_dir_all_for(target)?;
    let staging = staging_path(target);
    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging)
            .with_context(|| format!("create staging asset: {}", staging.display()))?;
        file.write_all(bytes)?;
        file.flush()?;
    }
    match fs::rename(&staging, target) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&staging);
            // Some Windows configurations refuse replace-by-rename. Fall back to
            // a delete+rename, which is still safe because the old bytes are
            // either the previous release or already backed up as a conflict.
            if target.exists() {
                fs::remove_file(target)?;
                return fs::rename(&staging, target).map_err(|_| error.into());
            }
            Err(error.into())
        }
    }
}

fn unique_conflict_path(root: &Path, relative: &str) -> PathBuf {
    let candidate = root.join(relative);
    if !candidate.exists() {
        return candidate;
    }
    let mut index = 1usize;
    loop {
        let mut name = candidate.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{index}"));
        let next = candidate.with_file_name(name);
        if !next.exists() {
            return next;
        }
        index += 1;
    }
}

fn export_conflict(root: &Path, relative: &str, bytes: &[u8]) -> Result<PathBuf> {
    let path = unique_conflict_path(root, relative);
    create_dir_all_for(&path)?;
    fs::write(&path, bytes)
        .with_context(|| format!("export updated bundled asset: {}", path.display()))?;
    Ok(path)
}

fn load_release_manifest(home: &Path) -> ReleaseManifest {
    let path = home.join(RELEASE_MANIFEST_FILE);
    let Ok(bytes) = fs::read(&path) else {
        return ReleaseManifest::default();
    };
    serde_json::from_slice::<ReleaseManifest>(&bytes).unwrap_or_default()
}

fn save_release_manifest(home: &Path, manifest: &ReleaseManifest) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest)?;
    replace_atomic(&home.join(RELEASE_MANIFEST_FILE), &bytes)
}

struct ReleaseManifestLock {
    path: PathBuf,
}

impl ReleaseManifestLock {
    fn acquire(home: &Path) -> Option<Self> {
        let path = home.join(RELEASE_MANIFEST_LOCK);
        for _ in 0..250 {
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(mut file) => {
                    let _ = write!(file, "{}", std::process::id());
                    return Some(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    // A crashed process must not block every later launch.
                    let stale = fs::metadata(&path)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|modified| modified.elapsed().ok())
                        .is_some_and(|age| age > Duration::from_secs(30));
                    if stale {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for ReleaseManifestLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Configuration merge.
//
// `config/wunder.yaml` is user-owned and must never be replaced, but a new build
// may add options. Inserting only keys the user config lacks keeps every default
// current while preserving explicit user values.
// ---------------------------------------------------------------------------

pub const DEFAULT_CONFIG_FILE: &str = "config/wunder-example.yaml";

#[derive(Debug, Clone, Default)]
pub struct ConfigMergeReport {
    pub changed: bool,
    pub inserted_keys: Vec<String>,
    pub backup: Option<PathBuf>,
}

pub fn merge_missing_config_keys(home: &Path) -> Result<ConfigMergeReport> {
    let user_path = home.join(CONFIG_FILE);
    let default_path = home.join(DEFAULT_CONFIG_FILE);
    if !user_path.is_file() || !default_path.is_file() {
        return Ok(ConfigMergeReport::default());
    }
    let user_text = fs::read_to_string(&user_path)
        .with_context(|| format!("read user config: {}", user_path.display()))?;
    let default_text = fs::read_to_string(&default_path)
        .with_context(|| format!("read default config: {}", default_path.display()))?;
    let Ok(mut user) = serde_yaml::from_str::<serde_yaml::Value>(&user_text) else {
        // Never rewrite a config we cannot parse; the loader reports the error.
        return Ok(ConfigMergeReport::default());
    };
    let Ok(default) = serde_yaml::from_str::<serde_yaml::Value>(&default_text) else {
        return Ok(ConfigMergeReport::default());
    };
    if !user.is_mapping() || !default.is_mapping() {
        return Ok(ConfigMergeReport::default());
    }

    let mut inserted_keys = Vec::new();
    insert_missing_keys(&mut user, &default, "", &mut inserted_keys);
    if inserted_keys.is_empty() {
        return Ok(ConfigMergeReport::default());
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let backup = user_path.with_file_name(format!("wunder.yaml.bak.{stamp}"));
    fs::copy(&user_path, &backup)
        .with_context(|| format!("backup user config: {}", user_path.display()))?;

    let merged = serde_yaml::to_string(&user).context("serialize merged config failed")?;
    replace_atomic(&user_path, merged.as_bytes())?;
    Ok(ConfigMergeReport {
        changed: true,
        inserted_keys,
        backup: Some(backup),
    })
}

fn insert_missing_keys(
    user: &mut serde_yaml::Value,
    default: &serde_yaml::Value,
    prefix: &str,
    inserted: &mut Vec<String>,
) {
    let (serde_yaml::Value::Mapping(user_map), serde_yaml::Value::Mapping(default_map)) =
        (user, default)
    else {
        return;
    };
    for (key, default_value) in default_map {
        let key_name = match key {
            serde_yaml::Value::String(name) => name.clone(),
            other => format!("{other:?}"),
        };
        let full = if prefix.is_empty() {
            key_name
        } else {
            format!("{prefix}.{key_name}")
        };
        match user_map.get_mut(key) {
            Some(user_value) => insert_missing_keys(user_value, default_value, &full, inserted),
            None => {
                user_map.insert(key.clone(), default_value.clone());
                inserted.push(full);
            }
        }
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

    #[test]
    fn config_merge_backfills_missing_keys_only() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        fs::create_dir_all(home.join("config")).unwrap();
        fs::write(home.join("config/wunder.yaml"), "server:\n  port: 8123\n").unwrap();
        fs::write(
            home.join("config/wunder-example.yaml"),
            "server:\n  host: 127.0.0.1\n  port: 8000\nstorage:\n  backend: sqlite\n",
        )
        .unwrap();

        let report = merge_missing_config_keys(home).unwrap();
        assert!(report.changed);
        let mut inserted = report.inserted_keys.clone();
        inserted.sort();
        assert_eq!(inserted, vec!["server.host".to_string(), "storage".to_string()]);
        assert!(report.backup.is_some());
        assert!(report.backup.as_ref().unwrap().is_file());

        let merged = fs::read_to_string(home.join("config/wunder.yaml")).unwrap();
        let value: serde_yaml::Value = serde_yaml::from_str(&merged).unwrap();
        // Explicit user values win; only absent keys are added.
        assert_eq!(value["server"]["port"].as_i64(), Some(8123));
        assert_eq!(value["server"]["host"].as_str(), Some("127.0.0.1"));
        assert_eq!(value["storage"]["backend"].as_str(), Some("sqlite"));

        // A second start has nothing left to add.
        let report = merge_missing_config_keys(home).unwrap();
        assert!(!report.changed);
        assert!(report.inserted_keys.is_empty());
    }

    #[test]
    fn config_merge_never_rewrites_unparsable_user_config() {
        let dir = tempdir().unwrap();
        let home = dir.path();
        fs::create_dir_all(home.join("config")).unwrap();
        let broken = "server: [unclosed\n";
        fs::write(home.join("config/wunder.yaml"), broken).unwrap();
        fs::write(
            home.join("config/wunder-example.yaml"),
            "server:\n  host: 127.0.0.1\n",
        )
        .unwrap();

        let report = merge_missing_config_keys(home).unwrap();
        assert!(!report.changed);
        assert_eq!(
            fs::read_to_string(home.join("config/wunder.yaml")).unwrap(),
            broken
        );
    }

    #[test]
    fn anchor_skips_the_per_file_pass_until_the_build_changes() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let v1: &'static [u8] = b"v1";
        let managed = vec![("config/prompts/a.txt".to_string(), v1)];

        let first = release_assets_with_id(
            root,
            &managed,
            &[],
            &ReleasePolicy::default(),
            Some("set-1"),
        )
        .unwrap();
        assert!(!first.anchor_hit);
        assert_eq!(first.count(ReleaseAction::Created), 1);
        assert!(root.join(RELEASE_ANCHOR_FILE).is_file());

        // Same build: the anchor short-circuits the whole per-file pass, so an
        // edited file is not inspected at all.
        fs::write(root.join("config/prompts/a.txt"), b"user edit").unwrap();
        let second = release_assets_with_id(
            root,
            &managed,
            &[],
            &ReleasePolicy::default(),
            Some("set-1"),
        )
        .unwrap();
        assert!(second.anchor_hit);
        assert!(second.outcomes.is_empty());

        // A different build id re-runs the full pass and finds the user edit.
        let third = release_assets_with_id(
            root,
            &managed,
            &[],
            &ReleasePolicy::default(),
            Some("set-2"),
        )
        .unwrap();
        assert!(!third.anchor_hit);
        assert_eq!(third.count(ReleaseAction::KeptUserModified), 1);
    }
}
