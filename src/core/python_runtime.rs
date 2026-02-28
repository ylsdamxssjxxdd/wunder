use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Clone, Debug)]
pub struct PythonRuntime {
    pub bin: PathBuf,
    pub embedded: bool,
    pub home: Option<PathBuf>,
    pub lib_dir: Option<PathBuf>,
    pub site_packages: Option<PathBuf>,
    pub ssl_cert: Option<PathBuf>,
}

pub fn resolve_python_runtime() -> Option<PythonRuntime> {
    if let Ok(raw) = env::var("WUNDER_PYTHON_BIN") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(python_runtime_from_bin(PathBuf::from(trimmed)));
        }
    }

    let app_dir = resolve_app_dir()?;
    let python_root = app_dir.join("opt/python");
    let python_bin = python_root.join("bin/python3");
    if python_bin.is_file() {
        return Some(python_runtime_from_home(python_root, python_bin));
    }
    let python_bin = python_root.join("bin/python");
    if python_bin.is_file() {
        return Some(python_runtime_from_home(python_root, python_bin));
    }

    let venv_bin = app_dir.join("opt/venv/bin/python");
    if venv_bin.is_file() {
        return Some(python_runtime_from_bin(venv_bin));
    }

    None
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
    cmd.env("PYTHONNOUSERSITE", "1");
    cmd.env("PIP_NO_INDEX", "1");
    if let Some(bin_dir) = runtime.bin.parent() {
        prepend_path_env(cmd, "PATH", bin_dir);
    }

    if let Some(lib_dir) = &runtime.lib_dir {
        prepend_path_env(cmd, "LD_LIBRARY_PATH", lib_dir);
    }
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
    let candidate = env::var("WUNDER_DESKTOP_APP_DIR")
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

    if let Some(home) = bin.parent().and_then(Path::parent).map(PathBuf::from) {
        if home.join("lib").is_dir() {
            runtime.embedded = is_embedded_home(&home);
            runtime.home = Some(home.clone());
            runtime.lib_dir = Some(home.join("lib"));
            runtime.site_packages = find_site_packages(&home);
            runtime.ssl_cert = runtime
                .site_packages
                .as_ref()
                .map(|path| path.join("certifi/cacert.pem"))
                .filter(|path| path.is_file());
        }
    }

    runtime
}

fn python_runtime_from_home(home: PathBuf, bin: PathBuf) -> PythonRuntime {
    let lib_dir = home.join("lib");
    let site_packages = find_site_packages(&home);
    let ssl_cert = site_packages
        .as_ref()
        .map(|path| path.join("certifi/cacert.pem"))
        .filter(|path| path.is_file());
    PythonRuntime {
        bin,
        embedded: true,
        home: Some(home),
        lib_dir: lib_dir.is_dir().then_some(lib_dir),
        site_packages,
        ssl_cert,
    }
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
