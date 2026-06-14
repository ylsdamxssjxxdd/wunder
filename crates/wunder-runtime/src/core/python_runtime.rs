use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command as TokioCommand;

const DESKTOP_APP_DIR_ENV: &str = "WUNDER_DESKTOP_APP_DIR";
const DESKTOP_RUNTIME_ROOT_ENV: &str = "WUNDER_DESKTOP_RUNTIME_ROOT";
const DESKTOP_SETTINGS_PATH_ENV: &str = "WUNDER_DESKTOP_SETTINGS_PATH";
const PYTHON_BIN_ENV: &str = "WUNDER_PYTHON_BIN";

#[derive(Clone, Debug)]
pub struct PythonRuntime {
    pub bin: PathBuf,
    pub embedded: bool,
    pub home: Option<PathBuf>,
    pub lib_dir: Option<PathBuf>,
    pub site_packages: Option<PathBuf>,
    pub ssl_cert: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct DesktopPythonSettings {
    #[serde(default)]
    python_path: String,
    #[serde(default)]
    pip_path: String,
    #[serde(default)]
    git_path: String,
    #[serde(default)]
    rg_path: String,
    #[serde(default)]
    python_runtime_mode: String,
}

#[derive(Clone, Debug, Default)]
pub struct DesktopCommandOverrides {
    pub pip_bin: Option<PathBuf>,
    pub git_bin: Option<PathBuf>,
    pub rg_bin: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct DesktopCommandEnv {
    pub python_runtime: Option<PythonRuntime>,
    pub command_overrides: DesktopCommandOverrides,
}

pub fn resolve_python_runtime() -> Option<PythonRuntime> {
    match resolve_desktop_settings_python_preference() {
        DesktopPythonPreference::Custom(configured_bin) => {
            return Some(python_runtime_from_bin(configured_bin));
        }
        DesktopPythonPreference::System => {
            return None;
        }
        DesktopPythonPreference::Auto => {}
    }

    if let Ok(raw) = env::var(PYTHON_BIN_ENV) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(python_runtime_from_bin(PathBuf::from(trimmed)));
        }
    }

    let app_dir = resolve_app_dir()?;
    let runtime_root = resolve_runtime_root().unwrap_or_else(|| app_dir.clone());
    let python_root = runtime_root.join("opt/python");
    for python_bin in embedded_python_candidates(&python_root) {
        if python_bin.is_file() {
            return Some(python_runtime_from_home(python_root, python_bin));
        }
    }

    for venv_bin in venv_python_candidates(&runtime_root) {
        if venv_bin.is_file() {
            return Some(python_runtime_from_bin(venv_bin));
        }
    }

    None
}

pub fn resolve_desktop_command_env() -> DesktopCommandEnv {
    DesktopCommandEnv {
        python_runtime: resolve_python_runtime(),
        command_overrides: resolve_desktop_command_overrides(),
    }
}

pub fn apply_python_env(cmd: &mut TokioCommand, runtime: &PythonRuntime) {
    cmd.env(PYTHON_BIN_ENV, runtime.bin.to_string_lossy().to_string());
    let runtime_path_entries = python_runtime_path_entries(runtime);
    for entry in runtime_path_entries.iter().rev() {
        prepend_path_env(cmd, "PATH", entry);
    }
    if !runtime.embedded {
        return;
    }
    if let Some(home) = &runtime.home {
        cmd.env("PYTHONHOME", home.to_string_lossy().to_string());
    }
    if let Some(site_packages) = &runtime.site_packages {
        cmd.env("PYTHONPATH", site_packages.to_string_lossy().to_string());
    }
    if let Some(cert) = &runtime.ssl_cert {
        cmd.env("SSL_CERT_FILE", cert.to_string_lossy().to_string());
    }
    if let Some(home) = &runtime.home {
        let rc = home.join("etc/matplotlibrc");
        if rc.is_file() {
            cmd.env("MATPLOTLIBRC", rc.to_string_lossy().to_string());
        }
        let cartopy_dir = home.join("share/cartopy");
        if cartopy_dir.is_dir() {
            cmd.env(
                "CARTOPY_DATA_DIR",
                cartopy_dir.to_string_lossy().to_string(),
            );
        }
    }
    cmd.env("PYTHONNOUSERSITE", "1");
    cmd.env("PIP_NO_INDEX", "1");

    if let Some(lib_dir) = &runtime.lib_dir {
        if cfg!(windows) {
            prepend_path_env(cmd, "PATH", lib_dir);
        } else {
            prepend_path_env(cmd, "LD_LIBRARY_PATH", lib_dir);
        }
    }
}

pub fn apply_system_python_env_if_configured(cmd: &mut TokioCommand) {
    if !desktop_python_runtime_mode_is_system() {
        return;
    }
    cmd.env_remove(PYTHON_BIN_ENV);
    cmd.env_remove("PYTHONHOME");
    cmd.env_remove("PYTHONPATH");
    cmd.env_remove("PYTHONNOUSERSITE");
    cmd.env_remove("PIP_NO_INDEX");
    remove_path_env_entries(cmd, "PATH", &bundled_python_path_entries());
}

pub fn apply_desktop_command_env(cmd: &mut TokioCommand, env: &DesktopCommandEnv) {
    if let Some(runtime) = env.python_runtime.as_ref() {
        apply_python_env(cmd, runtime);
    } else {
        apply_system_python_env_if_configured(cmd);
    }
    for bin in [
        env.command_overrides.pip_bin.as_ref(),
        env.command_overrides.git_bin.as_ref(),
        env.command_overrides.rg_bin.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(parent) = bin.parent() {
            prepend_path_env(cmd, "PATH", parent);
        }
    }
}

pub fn desktop_python_runtime_mode_is_system() -> bool {
    matches!(
        resolve_desktop_settings_python_preference(),
        DesktopPythonPreference::System
    )
}

fn python_runtime_path_entries(runtime: &PythonRuntime) -> Vec<PathBuf> {
    let mut entries = Vec::new();
    if let Some(parent) = runtime.bin.parent() {
        entries.push(parent.to_path_buf());
        for child in ["Scripts", "bin"] {
            let candidate = parent.join(child);
            if candidate.is_dir() {
                entries.push(candidate);
            }
        }
    }
    dedupe_paths(entries)
}

fn embedded_python_candidates(python_root: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![
        python_root.join("bin/python3"),
        python_root.join("bin/python"),
    ];
    if cfg!(windows) {
        candidates.extend([
            python_root.join("python.exe"),
            python_root.join("python3.exe"),
            python_root.join("bin/python.exe"),
            python_root.join("bin/python3.exe"),
        ]);
    }
    candidates
}

fn venv_python_candidates(app_dir: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![app_dir.join("opt/venv/bin/python")];
    if cfg!(windows) {
        candidates.extend([
            app_dir.join("opt/venv/Scripts/python.exe"),
            app_dir.join("opt/venv/python.exe"),
        ]);
    }
    candidates
}

fn prepend_path_env(cmd: &mut TokioCommand, key: &str, value: &Path) {
    let mut entries = vec![value.to_path_buf()];
    if let Some(existing) = env::var_os(key) {
        entries.extend(env::split_paths(&existing));
    }
    match env::join_paths(entries) {
        Ok(merged) => {
            cmd.env(key, merged);
        }
        Err(_) => {
            let prefix = value.to_string_lossy();
            let sep = if cfg!(windows) { ';' } else { ':' };
            let merged = match env::var(key) {
                Ok(existing) if !existing.trim().is_empty() => format!("{prefix}{sep}{existing}"),
                _ => prefix.to_string(),
            };
            cmd.env(key, merged);
        }
    };
}

fn remove_path_env_entries(cmd: &mut TokioCommand, key: &str, removals: &[PathBuf]) {
    if removals.is_empty() {
        return;
    }
    let Some(existing) = env::var_os(key) else {
        return;
    };
    let removal_keys = removals
        .iter()
        .map(|path| normalize_path_for_compare(path))
        .collect::<Vec<_>>();
    let entries = env::split_paths(&existing)
        .filter(|entry| {
            let normalized = normalize_path_for_compare(entry);
            !removal_keys.iter().any(|item| item == &normalized)
        })
        .collect::<Vec<_>>();
    if let Ok(joined) = env::join_paths(entries) {
        cmd.env(key, joined);
    }
}

fn bundled_python_path_entries() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(runtime_root) = resolve_runtime_root() {
        roots.push(runtime_root);
    }
    if let Some(app_dir) = resolve_app_dir() {
        roots.push(app_dir);
    }
    let entries = roots
        .into_iter()
        .flat_map(|root| {
            [
                root.join("opt/python"),
                root.join("opt/python/Scripts"),
                root.join("opt/python/bin"),
                root.join("opt/venv"),
                root.join("opt/venv/Scripts"),
                root.join("opt/venv/bin"),
            ]
        })
        .collect::<Vec<_>>();
    dedupe_paths(entries)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = Vec::new();
    let mut output = Vec::new();
    for path in paths {
        let key = normalize_path_for_compare(&path);
        if key.is_empty() || seen.iter().any(|item| item == &key) {
            continue;
        }
        seen.push(key);
        output.push(path);
    }
    output
}

fn normalize_path_for_compare(path: &Path) -> String {
    let mut normalized = path.to_string_lossy().replace('\\', "/");
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    if cfg!(windows) {
        normalized = normalized.to_ascii_lowercase();
    }
    normalized
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DesktopPythonPreference {
    Auto,
    System,
    Custom(PathBuf),
}

fn resolve_desktop_settings_python_preference() -> DesktopPythonPreference {
    let Some(settings) = read_desktop_python_settings() else {
        return DesktopPythonPreference::Auto;
    };
    let mode = settings.python_runtime_mode.trim().to_ascii_lowercase();
    let raw_path = settings.python_path.trim();
    if raw_path.is_empty() {
        if mode == "system" {
            return DesktopPythonPreference::System;
        }
        return DesktopPythonPreference::Auto;
    }
    let candidate = PathBuf::from(raw_path);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        let Some(app_dir) = resolve_app_dir() else {
            return DesktopPythonPreference::System;
        };
        app_dir.join(candidate)
    };
    if resolved.is_file() {
        return DesktopPythonPreference::Custom(resolved);
    }
    DesktopPythonPreference::System
}

fn resolve_desktop_command_overrides() -> DesktopCommandOverrides {
    let Some(settings) = read_desktop_python_settings() else {
        return DesktopCommandOverrides::default();
    };
    DesktopCommandOverrides {
        pip_bin: resolve_desktop_settings_file_path(&settings.pip_path),
        git_bin: resolve_desktop_settings_file_path(&settings.git_path),
        rg_bin: resolve_desktop_settings_file_path(&settings.rg_path),
    }
}

fn read_desktop_python_settings() -> Option<DesktopPythonSettings> {
    let settings_path = env::var(DESKTOP_SETTINGS_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    let text = fs::read_to_string(settings_path).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    serde_json::from_str::<DesktopPythonSettings>(&text).ok()
}

fn resolve_desktop_settings_file_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(trimmed);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        resolve_app_dir()?.join(candidate)
    };
    resolved.is_file().then_some(resolved)
}

fn resolve_app_dir() -> Option<PathBuf> {
    let candidate = env::var(DESKTOP_APP_DIR_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from);
    if let Some(value) = candidate {
        if value.is_dir() {
            return Some(value);
        }
    }

    env::var("APPDIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .filter(|value| value.is_dir())
        .or_else(|| {
            env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(PathBuf::from))
                .filter(|value| value.is_dir())
        })
}

fn resolve_runtime_root() -> Option<PathBuf> {
    env::var(DESKTOP_RUNTIME_ROOT_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .filter(|value| value.is_dir())
}

fn python_runtime_from_bin(bin: PathBuf) -> PythonRuntime {
    let mut runtime = PythonRuntime {
        bin: bin.clone(),
        embedded: false,
        home: None,
        lib_dir: None,
        site_packages: None,
        ssl_cert: None,
    };

    if !bin.is_absolute() {
        return runtime;
    }

    for home in python_home_candidates(&bin) {
        if has_embedded_python_layout(&home) {
            runtime.embedded = is_embedded_home(&home);
            runtime.home = Some(home.clone());
            runtime.lib_dir = resolve_python_lib_dir(&home);
            runtime.site_packages = find_site_packages(&home);
            runtime.ssl_cert = runtime
                .site_packages
                .as_ref()
                .map(|path| path.join("certifi/cacert.pem"))
                .filter(|path| path.is_file());
            break;
        }
    }

    runtime
}

fn python_runtime_from_home(home: PathBuf, bin: PathBuf) -> PythonRuntime {
    let lib_dir = resolve_python_lib_dir(&home);
    let site_packages = find_site_packages(&home);
    let ssl_cert = site_packages
        .as_ref()
        .map(|path| path.join("certifi/cacert.pem"))
        .filter(|path| path.is_file());
    PythonRuntime {
        bin,
        embedded: true,
        home: Some(home),
        lib_dir,
        site_packages,
        ssl_cert,
    }
}

fn python_home_candidates(bin: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(parent) = bin.parent() {
        candidates.push(parent.to_path_buf());
        if let Some(grand_parent) = parent.parent() {
            candidates.push(grand_parent.to_path_buf());
        }
    }
    candidates
}

fn resolve_python_lib_dir(home: &Path) -> Option<PathBuf> {
    [home.join("Lib"), home.join("lib")]
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

fn has_embedded_python_layout(home: &Path) -> bool {
    resolve_python_lib_dir(home).is_some()
        || home.join("python.exe").is_file()
        || home.join("python3.exe").is_file()
        || home.join("bin/python").is_file()
        || home.join("bin/python3").is_file()
}

fn is_embedded_home(home: &Path) -> bool {
    let normalized = home.to_string_lossy().replace('\\', "/");
    if normalized.contains("/opt/python") {
        return true;
    }
    if let Some(app_dir) = resolve_app_dir() {
        if home.starts_with(app_dir) {
            return true;
        }
    }
    false
}

fn find_site_packages(home: &Path) -> Option<PathBuf> {
    let windows_site = home.join("Lib/site-packages");
    if windows_site.is_dir() {
        return Some(windows_site);
    }
    let lib_dir = home.join("lib");
    let entries = fs::read_dir(&lib_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("python3") {
            continue;
        }
        let site = path.join("site-packages");
        if site.is_dir() {
            return Some(site);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        find_site_packages, resolve_desktop_settings_python_preference, resolve_python_lib_dir,
        DesktopPythonPreference, DESKTOP_APP_DIR_ENV, DESKTOP_SETTINGS_PATH_ENV,
    };
    use std::env;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_python_lib_dir_supports_windows_layout() {
        let temp = tempdir().expect("tempdir");
        let lib_dir = temp.path().join("Lib");
        std::fs::create_dir_all(&lib_dir).expect("create Lib");
        assert_eq!(resolve_python_lib_dir(temp.path()), Some(lib_dir));
    }

    #[test]
    fn find_site_packages_supports_windows_layout() {
        let temp = tempdir().expect("tempdir");
        let site_packages = temp.path().join("Lib/site-packages");
        std::fs::create_dir_all(&site_packages).expect("create site-packages");
        assert_eq!(find_site_packages(temp.path()), Some(site_packages));
    }

    fn with_env_lock<T>(body: impl FnOnce() -> T) -> T {
        let _guard = ENV_MUTEX.lock().expect("lock env mutex");
        body()
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => env::set_var(key, value),
            None => env::remove_var(key),
        }
    }

    #[test]
    fn resolve_desktop_settings_python_preference_supports_relative_paths() {
        with_env_lock(|| {
            let temp = tempdir().expect("tempdir");
            let app_dir = temp.path().join("app");
            let python_bin = app_dir.join("runtime/python.exe");
            std::fs::create_dir_all(
                python_bin
                    .parent()
                    .expect("relative python path should have parent"),
            )
            .expect("create python dir");
            std::fs::write(&python_bin, b"").expect("write python stub");

            let settings_path = temp.path().join("desktop.settings.json");
            std::fs::write(&settings_path, r#"{"python_path":"runtime/python.exe"}"#)
                .expect("write settings");

            let previous_app_dir = env::var_os(DESKTOP_APP_DIR_ENV);
            let previous_settings_path = env::var_os(DESKTOP_SETTINGS_PATH_ENV);

            env::set_var(DESKTOP_APP_DIR_ENV, &app_dir);
            env::set_var(DESKTOP_SETTINGS_PATH_ENV, &settings_path);

            let resolved = resolve_desktop_settings_python_preference();

            restore_env(DESKTOP_APP_DIR_ENV, previous_app_dir);
            restore_env(DESKTOP_SETTINGS_PATH_ENV, previous_settings_path);

            assert_eq!(resolved, DesktopPythonPreference::Custom(python_bin));
        });
    }

    #[test]
    fn resolve_desktop_settings_python_preference_system_mode_blocks_fallback() {
        with_env_lock(|| {
            let temp = tempdir().expect("tempdir");
            let settings_path = temp.path().join("desktop.settings.json");
            std::fs::write(
                &settings_path,
                r#"{"python_path":"","python_runtime_mode":"system"}"#,
            )
            .expect("write settings");

            let previous_settings_path = env::var_os(DESKTOP_SETTINGS_PATH_ENV);
            env::set_var(DESKTOP_SETTINGS_PATH_ENV, &settings_path);

            let resolved = resolve_desktop_settings_python_preference();

            restore_env(DESKTOP_SETTINGS_PATH_ENV, previous_settings_path);

            assert_eq!(resolved, DesktopPythonPreference::System);
        });
    }
}
