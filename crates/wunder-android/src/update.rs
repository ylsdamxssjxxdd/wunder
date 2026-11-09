//! Android online update: check → download → system installer.
//!
//! The public GitHub release is the source. Unlike the desktop updater this
//! does not need a `manifest.json`: the APK file name carries the version and
//! GitHub publishes a sha256 digest per asset. A local override file
//! (`WUNDER_HOME/config/android-update.json`) is honored for self-hosted
//! deployments and offline testing; it takes precedence over GitHub.
//!
//! Everything here blocks. The engine calls these functions from short-lived
//! worker threads and publishes a small state projection for the UI tick.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use wunder_desktop::{is_newer_version, AndroidRelease, NativeDesktop};

/// Public repository used when the build does not inject a release repo.
/// Forks and private deployments override it with
/// `WUNDER_UPDATE_GITHUB_REPO` at compile time.
const DEFAULT_REPO: &str = "ylsdamxssjxxdd/wunder";
/// A nightly keeps the same semantic version. Treat an APK published this
/// long after the installed build as a new nightly, not clock noise.
const SAME_VERSION_GRACE_MS: i64 = 10 * 60 * 1000;
const NOTES_LIMIT: usize = 1200;

#[derive(Clone, Debug)]
pub struct UpdateOffer {
    pub release: AndroidRelease,
    pub source: String,
    pub notes: String,
    pub size_text: String,
}

pub enum CheckOutcome {
    Available(UpdateOffer),
    UpToDate,
    /// The source has no APK for this device architecture.
    NoAsset,
}

pub fn repo() -> &'static str {
    match option_env!("WUNDER_UPDATE_GITHUB_REPO") {
        Some(repo) if !repo.is_empty() => repo,
        _ => DEFAULT_REPO,
    }
}

/// Short source line for the update card, also when no update is available.
pub fn source_label(channel: &str) -> String {
    if channel == "cloud" {
        "云端更新源".into()
    } else if override_path().is_some_and(|path| path.is_file()) {
        "自定义更新源".into()
    } else {
        format!("GitHub · {}", repo())
    }
}

/// Release asset names use the user-facing arch label (`arm64` / `x64`),
/// while the Rust target constants use `aarch64` / `x86_64`.
pub fn arch_label() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        "arm" => "arm",
        "x86" => "x86",
        _ => "",
    }
}

/// Blocking check. The caller sets the `checking` phase first and runs this on
/// a worker thread. `Ok(None)` means the source has no APK for this build.
pub fn check_offer(desktop: &NativeDesktop, channel: &str) -> Result<CheckOutcome> {
    let (release, source) = if channel == "cloud" {
        let release = desktop
            .check_android_update_cloud(std::env::consts::ARCH)
            .context("检查云端更新失败")?;
        match release {
            Some(release) => (release, "云端更新源".to_string()),
            None => return Ok(CheckOutcome::NoAsset),
        }
    } else {
        let override_release = read_override()?;
        match override_release {
            Some(release) => (release, "自定义更新源".to_string()),
            None => {
                let arch = arch_label();
                if arch.is_empty() {
                    bail!("当前设备架构不支持在线更新");
                }
                let release = desktop
                    .check_android_update(repo(), arch)
                    .context("检查在线更新失败")?;
                match release {
                    Some(release) => (release, format!("GitHub · {}", repo())),
                    None => return Ok(CheckOutcome::NoAsset),
                }
            }
        }
    };
    let current = env!("CARGO_PKG_VERSION");
    let installed_at = installed_at_millis();
    let same_version = !is_newer_version(&release.version, current)
        && !is_newer_version(current, &release.version);
    let published_after_install = installed_at > 0
        && release.published_at_ms > installed_at + SAME_VERSION_GRACE_MS;
    let newer = is_newer_version(&release.version, current)
        || (same_version && published_after_install);
    if !newer {
        return Ok(CheckOutcome::UpToDate);
    }
    let notes = bounded_notes(&release.notes);
    let size_text = format_size(release.size);
    Ok(CheckOutcome::Available(UpdateOffer {
        release,
        source,
        notes,
        size_text,
    }))
}

/// Blocking download into the app cache. The caller sets `downloading` and
/// owns the progress arc shared with the UI tick.
pub fn download_offer(
    desktop: &NativeDesktop,
    offer: &UpdateOffer,
    progress: &AtomicU64,
) -> Result<PathBuf> {
    let dir = download_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("创建更新目录失败：{}", dir.display()))?;
    let path = dir.join(format!(
        "wunder-android-{}-{}.apk",
        safe_version(&offer.release.version),
        std::env::consts::ARCH
    ));
    // A stale partial file from an earlier failed attempt must never be
    // mistaken for a verified download.
    let _ = std::fs::remove_file(&path);
    progress.store(0, Ordering::Relaxed);
    let result = if offer.release.url.is_empty() {
        // Cloud releases carry only the authenticated server-side file name.
        desktop.download_android_update_cloud(&offer.release, &path, progress)
    } else {
        desktop.download_android_update(&offer.release, &path, progress)
    };
    result.map_err(|error| {
        let _ = std::fs::remove_file(&path);
        error
    })?;
    Ok(path)
}

/// Hand the verified APK to the Android package installer. Java checks the
/// package name, signing certificate and version code before starting the
/// session; this function only translates its status into the UI phase.
pub fn install(path: &Path) -> Result<String> {
    if !path.is_file() {
        bail!("更新包不存在，请重新下载");
    }
    #[cfg(target_os = "android")]
    {
        Ok(crate::android_host::install_apk(
            &path.to_string_lossy(),
        ))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = path;
        bail!("仅安卓版支持在线安装")
    }
}

fn installed_at_millis() -> i64 {
    #[cfg(target_os = "android")]
    {
        crate::android_host::installed_at_millis()
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

fn download_dir() -> PathBuf {
    std::env::temp_dir().join("android-update")
}

fn bounded_notes(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= NOTES_LIMIT {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(NOTES_LIMIT).collect();
    out.push('…');
    out
}

pub fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return String::new();
    }
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

fn safe_version(version: &str) -> String {
    let cleaned: String = version
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '+') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "latest".to_string()
    } else {
        cleaned
    }
}

fn override_path() -> Option<PathBuf> {
    std::env::var_os("WUNDER_HOME")
        .map(PathBuf::from)
        .map(|home| home.join("config/android-update.json"))
}

/// Optional local update descriptor:
/// `{ "version", "notes", "url", "sha256", "size", "published_at" }`
/// (`published_at` is Unix seconds). Used by self-hosted releases and the
/// emulator update test; absence falls back to GitHub.
fn read_override() -> Result<Option<AndroidRelease>> {
    let Some(path) = override_path() else {
        return Ok(None);
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(anyhow!("读取自定义更新源失败：{error}"));
        }
    };
    let value: Value =
        serde_json::from_str(&text).map_err(|error| anyhow!("自定义更新源不是有效 JSON：{error}"))?;
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if version.is_empty() || url.is_empty() {
        return Err(anyhow!("自定义更新源缺少 version 或 url"));
    }
    let published_at_ms = value
        .get("published_at")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or(0)
        })
        * 1000;
    Ok(Some(AndroidRelease {
        version: version.to_string(),
        notes: value
            .get("notes")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        url: url.to_string(),
        file: String::new(),
        sha256: value
            .get("sha256")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase(),
        size: value.get("size").and_then(Value::as_u64).unwrap_or(0),
        published_at_ms,
        html_url: value
            .get("html_url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_stay_compact() {
        assert_eq!(format_size(0), "");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size(352 * 1024 * 1024), "352 MB");
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    #[test]
    fn notes_are_bounded() {
        let notes = "x".repeat(NOTES_LIMIT + 10);
        let bounded = bounded_notes(&notes);
        assert_eq!(bounded.chars().count(), NOTES_LIMIT + 1);
        assert!(bounded.ends_with('…'));
    }

    #[test]
    fn version_file_names_cannot_escape_the_cache_dir() {
        assert_eq!(safe_version("0.4.1+nightly/../x"), "0.4.1+nightly_.._x");
        assert_eq!(safe_version(""), "latest");
    }
}
