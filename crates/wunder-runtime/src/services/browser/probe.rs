//! Local browser discovery for launch mode.
//!
//! The Playwright bridge launches its bundled Chromium by default. When that
//! browser is not installed (the packaged local forms ship without it), the
//! launch fails with an opaque "Executable doesn't exist" error. This module
//! probes, in order, the bundled supplement browser and the system browsers so
//! launch mode can fall back to a real executable instead of failing.

use std::path::{Path, PathBuf};

use wunder_core::local_bootstrap::{resolve_home_from_env_or_user, supplement};

/// One probed browser location with its on-disk verdict, listed in launch
/// fallback guidance so the model can point the user at a concrete fix.
#[derive(Debug, Clone)]
pub struct BrowserCandidate {
    pub source: &'static str,
    pub path: PathBuf,
    pub exists: bool,
}

/// Pure selection over pre-expanded candidate lists. A browser installed with
/// the supplement wins first: a Playwright cache may belong to another
/// Playwright revision and must not hide the copy shipped with this install.
/// Without a supplement browser, an installed Playwright Chromium wins by
/// absence (the bridge resolves it itself), then the first system browser.
pub fn select_fallback_executable(
    playwright_chromium: &[PathBuf],
    supplement_roots: &[PathBuf],
    system_browsers: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if let Some(bundled) = supplement_roots
        .iter()
        .flat_map(|root| supplement_browser_candidates(root))
        .find(|path| exists(path))
    {
        return Some(bundled);
    }
    if playwright_chromium.iter().any(|path| exists(path)) {
        return None;
    }
    system_browsers.iter().cloned().find(|path| exists(path))
}

/// Probe the local machine for a launch executable. Returns `None` when the
/// Playwright default Chromium is installed (no override needed) or when no
/// fallback browser exists.
pub fn probe_launch_executable(browsers_path: Option<&str>) -> Option<PathBuf> {
    let chromium = playwright_chromium_executables(browsers_path);
    select_fallback_executable(
        &chromium,
        &supplement_roots(),
        &system_browser_paths(),
        &|path| path.is_file(),
    )
}

/// Every location the launch fallback knows about, for failure guidance.
pub fn collect_browser_candidates(browsers_path: Option<&str>) -> Vec<BrowserCandidate> {
    let mut candidates: Vec<BrowserCandidate> = playwright_chromium_executables(browsers_path)
        .into_iter()
        .map(|path| BrowserCandidate {
            source: "Playwright 默认 Chromium",
            exists: path.is_file(),
            path,
        })
        .collect();
    candidates.extend(supplement_roots().into_iter().flat_map(|root| {
        supplement_browser_candidates(&root)
            .into_iter()
            .map(|path| BrowserCandidate {
                source: "补充包内置浏览器",
                exists: path.is_file(),
                path,
            })
    }));
    candidates.extend(
        system_browsers()
            .into_iter()
            .map(|(name, path)| BrowserCandidate {
                source: name,
                exists: path.is_file(),
                path,
            }),
    );
    candidates
}

fn supplement_browser_candidates(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("opt/browser/chrome-win/chrome.exe"),
        root.join("opt/browser/chrome-win64/chrome.exe"),
        root.join("opt/browser/chrome-linux/chrome"),
        root.join("opt/browser/chrome-linux64/chrome"),
        root.join("opt/browser/chrome-linux/chrome"),
        root.join("opt/browser/chrome.exe"),
    ]
}

/// Executable candidates of every `chromium-*` build under the Playwright
/// browsers root: config value, then PLAYWRIGHT_BROWSERS_PATH, then the
/// platform default.
fn playwright_chromium_executables(browsers_path: Option<&str>) -> Vec<PathBuf> {
    let Some(root) = playwright_browsers_root(browsers_path) else {
        return Vec::new();
    };
    let mut executables = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("chromium-") {
                let dir = entry.path();
                executables.push(dir.join("chrome-win/chrome.exe"));
                executables.push(dir.join("chrome-win64/chrome.exe"));
                executables.push(dir.join("chrome-linux/chrome"));
            }
        }
    }
    executables
}

fn playwright_browsers_root(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(value) = configured.map(str::trim).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(value));
    }
    if let Some(value) = std::env::var_os("PLAYWRIGHT_BROWSERS_PATH")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return Some(value);
    }
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .map(|home| home.join("AppData/Local"))
            })
            .map(|base| base.join("ms-playwright"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".cache/ms-playwright"))
    }
}

fn supplement_roots() -> Vec<PathBuf> {
    let home = resolve_home_from_env_or_user();
    let app_dir = std::env::var_os("WUNDER_DESKTOP_APP_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(PathBuf::from))
        });
    let appimage_dir = std::env::var_os("APPDIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir());
    match app_dir {
        Some(app_dir) => supplement::roots(&home, &app_dir, appimage_dir),
        None => supplement::roots(&home, &home, None),
    }
}

/// System browsers in probe order: Edge first (present on every supported
/// Windows), then Chrome. Linux relies on the Playwright bundled browser.
#[cfg(windows)]
fn system_browsers() -> Vec<(&'static str, PathBuf)> {
    let program_files_x86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let local_appdata = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let mut browsers = Vec::new();
    for base in [&program_files_x86, &program_files].into_iter().flatten() {
        browsers.push((
            "系统 Edge",
            base.join("Microsoft/Edge/Application/msedge.exe"),
        ));
    }
    for base in [&program_files, &program_files_x86, &local_appdata]
        .into_iter()
        .flatten()
    {
        browsers.push((
            "系统 Chrome",
            base.join("Google/Chrome/Application/chrome.exe"),
        ));
    }
    browsers
}

#[cfg(not(windows))]
fn system_browsers() -> Vec<(&'static str, PathBuf)> {
    Vec::new()
}

fn system_browser_paths() -> Vec<PathBuf> {
    system_browsers()
        .into_iter()
        .map(|(_name, path)| path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        playwright_chromium_executables, probe_launch_executable, select_fallback_executable,
    };
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn installed_playwright_chromium_suppresses_system_fallback() {
        let exists = |path: &Path| path.ends_with("chrome.exe");
        let selected = select_fallback_executable(
            &[PathBuf::from(
                "browsers/chromium-1091/chrome-win/chrome.exe",
            )],
            &[],
            &[PathBuf::from("C:/Edge/msedge.exe")],
            &exists,
        );
        assert_eq!(selected, None);
    }

    #[test]
    fn supplement_browser_wins_over_playwright_cache() {
        let bundled = PathBuf::from("supplement/opt/browser/chrome-win/chrome.exe");
        let cached = PathBuf::from("browsers/chromium-1217/chrome-win64/chrome.exe");
        let exists = |path: &Path| path == bundled.as_path() || path == cached.as_path();
        let selected = select_fallback_executable(
            std::slice::from_ref(&cached),
            &[PathBuf::from("supplement")],
            &[PathBuf::from("C:/Edge/msedge.exe")],
            &exists,
        );
        assert_eq!(selected, Some(bundled));
    }

    #[test]
    fn supplement_browser_wins_over_system_browser() {
        let bundled = PathBuf::from("supplement/opt/browser/chrome-win/chrome.exe");
        let exists = |path: &Path| path == bundled.as_path();
        let selected = select_fallback_executable(
            &[PathBuf::from(
                "browsers/chromium-1091/chrome-win/chrome.exe",
            )],
            &[PathBuf::from("supplement")],
            &[PathBuf::from("C:/Edge/msedge.exe")],
            &exists,
        );
        assert_eq!(selected, Some(bundled));
    }

    #[test]
    fn chrome_win64_supplement_and_cache_are_recognized() {
        let bundled = PathBuf::from("supplement/opt/browser/chrome-win64/chrome.exe");
        let exists = |path: &Path| path == bundled.as_path();
        let selected =
            select_fallback_executable(&[], &[PathBuf::from("supplement")], &[], &exists);
        assert_eq!(selected, Some(bundled));

        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("chromium-1234/chrome-win64/chrome.exe"));
        let cached = playwright_chromium_executables(Some(root.path().to_str().unwrap()));
        assert!(cached
            .iter()
            .any(|path| path.ends_with("chromium-1234/chrome-win64/chrome.exe")));
    }

    #[test]
    fn system_browser_is_the_last_resort() {
        let edge = PathBuf::from("C:/Edge/msedge.exe");
        let exists = |path: &Path| path == edge.as_path();
        let selected = select_fallback_executable(
            &[],
            &[PathBuf::from("supplement")],
            &[edge.clone()],
            &exists,
        );
        assert_eq!(selected, Some(edge));
    }

    #[test]
    fn nothing_found_means_no_override() {
        let selected = select_fallback_executable(
            &[],
            &[PathBuf::from("supplement")],
            &[PathBuf::from("C:/Edge/msedge.exe")],
            &|_| false,
        );
        assert_eq!(selected, None);
    }

    #[test]
    fn real_probe_finds_bundled_supplement_browser() {
        let _guard = lock_env();
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join("opt/browser/chrome.exe"));
        let previous = std::env::var_os("WUNDER_HOME");
        std::env::set_var("WUNDER_HOME", home.path());
        let probed = probe_launch_executable(Some("definitely/missing/browsers/root"));
        match previous {
            Some(value) => std::env::set_var("WUNDER_HOME", value),
            None => std::env::remove_var("WUNDER_HOME"),
        }
        assert_eq!(probed, Some(home.path().join("opt/browser/chrome.exe")));
    }

    #[test]
    fn chrome_linux64_supplement_is_recognized() {
        let bundled = PathBuf::from("supplement/opt/browser/chrome-linux64/chrome");
        let exists = |path: &Path| path == bundled.as_path();
        let selected = select_fallback_executable(&[], &[PathBuf::from("supplement")], &[], &exists);
        assert_eq!(selected, Some(bundled));
    }

    #[test]
    fn real_probe_stays_quiet_when_playwright_chromium_is_installed() {
        let _guard = lock_env();
        let home = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("WUNDER_HOME");
        std::env::set_var("WUNDER_HOME", home.path());
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("chromium-1091/chrome-win/chrome.exe"));
        let probed = probe_launch_executable(Some(root.path().to_str().unwrap()));
        match previous {
            Some(value) => std::env::set_var("WUNDER_HOME", value),
            None => std::env::remove_var("WUNDER_HOME"),
        }
        assert_eq!(probed, None);
    }
}
