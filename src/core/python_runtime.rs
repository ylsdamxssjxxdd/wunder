use serde::Deserialize;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;

const DESKTOP_APP_DIR_ENV: &str = "WUNDER_DESKTOP_APP_DIR";
const DESKTOP_SETTINGS_PATH_ENV: &str = "WUNDER_DESKTOP_SETTINGS_PATH";

#[derive(Debug, Deserialize)]
struct DesktopPythonSettings {
    #[serde(default)]
    python_interpreter_path: String,
}

#[derive(Clone, Debug)]
pub struct PythonRuntime {
    pub bin: PathBuf,
    pub embedded: bool,
    pub home: Option<PathBuf>,
    pub lib_dir: Option<PathBuf>,
    pub site_packages: Option<PathBuf>,
    pub ssl_cert: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct PythonInterpreterCandidate {
    pub path: PathBuf,
    pub source: String,
}

pub fn resolve_python_runtime() -> Option<PythonRuntime> {
    if let Ok(raw) = env::var("WUNDER_PYTHON_BIN") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(python_runtime_from_bin(PathBuf::from(trimmed)));
        }
    }

    if let Some(runtime) = resolve_desktop_settings_python_runtime() {
        return Some(runtime);
    }

    let app_dir = resolve_app_dir()?;
    let python_root = app_dir.join("opt/python");
    for python_bin in embedded_python_candidates(&python_root) {
        if python_bin.is_file() {
            return Some(python_runtime_from_home(python_root, python_bin));
        }
    }

    for venv_bin in venv_python_candidates(&app_dir) {
        if venv_bin.is_file() {
            return Some(python_runtime_from_bin(venv_bin));
        }
    }

    None
}

pub fn detect_python_interpreters() -> Vec<PythonInterpreterCandidate> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(raw) = env::var("WUNDER_PYTHON_BIN") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            push_python_candidate(&mut output, &mut seen, PathBuf::from(trimmed), "env");
        }
    }

    if let Some(configured) = resolve_desktop_settings_python_bin() {
        push_python_candidate(&mut output, &mut seen, configured, "settings");
    }

    if let Some(app_dir) = resolve_app_dir() {
        let python_root = app_dir.join("opt/python");
        for candidate in embedded_python_candidates(&python_root) {
            push_python_candidate(&mut output, &mut seen, candidate, "bundled");
        }
        for candidate in venv_python_candidates(&app_dir) {
            push_python_candidate(&mut output, &mut seen, candidate, "venv");
        }
    }

    if let Some(path_env) = env::var_os("PATH") {
        for entry in env::split_paths(&path_env) {
            for binary_name in python_binary_names() {
                push_python_candidate(&mut output, &mut seen, entry.join(binary_name), "path");
            }
        }
    }

    if cfg!(windows) {
        for candidate in common_windows_python_candidates() {
            push_python_candidate(&mut output, &mut seen, candidate, "common");
        }
    } else {
        for candidate in common_unix_python_candidates() {
            push_python_candidate(&mut output, &mut seen, candidate, "common");
        }
    }

    output
}

pub fn apply_python_env(cmd: &mut Command, runtime: &PythonRuntime) {
    if !runtime.embedded {
        return;
    }
    cmd.env(
        "WUNDER_PYTHON_BIN",
        runtime.bin.to_string_lossy().to_string(),
    );
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
    if let Some(bin_dir) = runtime.bin.parent() {
        prepend_path_env(cmd, "PATH", bin_dir);
    }

    if let Some(lib_dir) = &runtime.lib_dir {
        if cfg!(windows) {
            prepend_path_env(cmd, "PATH", lib_dir);
        } else {
            prepend_path_env(cmd, "LD_LIBRARY_PATH", lib_dir);
        }
    }
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

fn prepend_path_env(cmd: &mut Command, key: &str, value: &Path) {
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
}

fn push_python_candidate(
    output: &mut Vec<PythonInterpreterCandidate>,
    seen: &mut HashSet<String>,
    path: PathBuf,
    source: &str,
) {
    if !path.is_file() || is_windows_store_python_stub(&path) {
        return;
    }
    let key = normalize_python_path_key(&path);
    if key.is_empty() || !seen.insert(key) {
        return;
    }
    output.push(PythonInterpreterCandidate {
        path,
        source: source.to_string(),
    });
}

fn normalize_python_path_key(path: &Path) -> String {
    let mut normalized = path.to_string_lossy().trim().replace('\\', "/");
    if cfg!(windows) {
        normalized.make_ascii_lowercase();
    }
    normalized
}

fn python_binary_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["python.exe", "python3.exe"]
    } else {
        &["python3", "python"]
    }
}

fn is_windows_store_python_stub(path: &Path) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let normalized = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    // Windows Store stubs often redirect to the Store instead of a real runtime.
    normalized.contains("/windowsapps/python")
}

fn common_unix_python_candidates() -> Vec<PathBuf> {
    [
        "/usr/bin/python3",
        "/usr/local/bin/python3",
        "/opt/homebrew/bin/python3",
        "/usr/bin/python",
        "/usr/local/bin/python",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

fn common_windows_python_candidates() -> Vec<PathBuf> {
    let mut output = Vec::new();

    if let Some(local_app_data) = env_path("LOCALAPPDATA") {
        output.extend(read_versioned_python_bins(
            &local_app_data.join("Programs/Python"),
        ));
    }

    for env_name in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(base) = env_path(env_name) {
            output.extend(read_prefixed_python_bins(&base, "python"));
            output.extend(read_versioned_python_bins(&base.join("Python")));
        }
    }

    if let Some(user_profile) = env_path("USERPROFILE") {
        output.extend(
            [
                user_profile.join("miniconda3/python.exe"),
                user_profile.join("anaconda3/python.exe"),
                user_profile.join("AppData/Local/Programs/Python/Python311/python.exe"),
                user_profile.join("AppData/Local/Programs/Python/Python310/python.exe"),
                user_profile.join("AppData/Local/Programs/Python/Python39/python.exe"),
                user_profile.join("AppData/Local/Programs/Python/Python38/python.exe"),
            ]
            .into_iter(),
        );
        output.extend(read_versioned_python_bins(
            &user_profile.join(".pyenv/pyenv-win/versions"),
        ));
    }

    output
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn read_versioned_python_bins(base_dir: &Path) -> Vec<PathBuf> {
    let mut output = Vec::new();
    let entries = match fs::read_dir(base_dir) {
        Ok(entries) => entries,
        Err(_) => return output,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        for binary_name in python_binary_names() {
            output.push(path.join(binary_name));
        }
    }
    output
}

fn read_prefixed_python_bins(base_dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut output = Vec::new();
    let entries = match fs::read_dir(base_dir) {
        Ok(entries) => entries,
        Err(_) => return output,
    };
    let prefix = prefix.to_ascii_lowercase();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !name.starts_with(&prefix) {
            continue;
        }
        for binary_name in python_binary_names() {
            output.push(path.join(binary_name));
        }
    }
    output
}

fn resolve_desktop_settings_python_runtime() -> Option<PythonRuntime> {
    let bin = resolve_desktop_settings_python_bin()?;
    if !bin.is_file() {
        return None;
    }
    Some(python_runtime_from_bin(bin))
}

fn resolve_desktop_settings_python_bin() -> Option<PathBuf> {
    let settings_path = env::var(DESKTOP_SETTINGS_PATH_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)?;
    let text = fs::read_to_string(settings_path).ok()?;
    let settings = serde_json::from_str::<DesktopPythonSettings>(&text).ok()?;
    let raw_path = settings.python_interpreter_path.trim();
    if raw_path.is_empty() {
        return None;
    }

    // Desktop users can override the bundled runtime with a custom Python binary.
    resolve_configured_python_bin(raw_path)
}

fn resolve_configured_python_bin(raw_path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(raw_path.trim());
    if path.as_os_str().is_empty() {
        return None;
    }
    if path.is_absolute() {
        return Some(path);
    }
    resolve_app_dir().map(|app_dir| app_dir.join(path))
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
    [home.join("lib"), home.join("Lib")]
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
    use super::{find_site_packages, resolve_python_lib_dir};
    use tempfile::tempdir;

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
}
