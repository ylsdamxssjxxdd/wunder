//! Shared discovery for optional native runtimes installed under WUNDER_HOME/opt.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub enum Tool {
    Python,
    Git,
    Rg,
    Node,
}

/// The shared home always wins. Older installations remain read-only fallbacks;
/// importers must write to the home even when a legacy opt tree already exists.
pub fn roots(home: &Path, app_dir: &Path, appimage_dir: Option<PathBuf>) -> Vec<PathBuf> {
    let mut roots = vec![home.to_path_buf()];
    for root in std::iter::once(app_dir.to_path_buf()).chain(appimage_dir) {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

pub fn find_tool(roots: &[PathBuf], tool: Tool) -> Option<PathBuf> {
    if matches!(tool, Tool::Node) && !can_run_bundled_remotion() {
        return None;
    }
    let candidates: &[&str] = match tool {
        Tool::Python if cfg!(windows) => &[
            "opt/python/python.exe",
            "opt/python/python3.exe",
            "opt/python/bin/python.exe",
            "opt/python/bin/python3.exe",
        ],
        Tool::Python => &["opt/python/bin/python3", "opt/python/bin/python"],
        Tool::Git if cfg!(windows) => &["opt/git/cmd/git.exe", "opt/git/bin/git.exe"],
        Tool::Git => &["opt/git/bin/git", "opt/git/cmd/git"],
        Tool::Rg if cfg!(windows) => &[
            "opt/rg/rg.exe",
            "opt/rg/bin/rg.exe",
            "opt/ripgrep/rg.exe",
            "opt/ripgrep/bin/rg.exe",
        ],
        Tool::Rg => &[
            "opt/rg/bin/rg",
            "opt/rg/rg",
            "opt/ripgrep/bin/rg",
            "opt/ripgrep/rg",
        ],
        Tool::Node if cfg!(windows) => &["opt/node/node.exe", "opt/node/bin/node.exe"],
        Tool::Node => &["opt/node/bin/node", "opt/node/node"],
    };
    roots.iter().find_map(|root| {
        candidates
            .iter()
            .map(|name| root.join(name))
            .find(|path| path.is_file())
    })
}

pub fn path_entries(roots: &[PathBuf]) -> Vec<PathBuf> {
    const BASE_DIRS: &[&str] = &[
        "opt/python",
        "opt/python/Scripts",
        "opt/python/bin",
        "opt/venv",
        "opt/venv/Scripts",
        "opt/venv/bin",
        "opt/git/cmd",
        "opt/git/bin",
        "opt/rg",
        "opt/rg/bin",
        "opt/ripgrep",
        "opt/ripgrep/bin",
        "opt/ffmpeg",
        "opt/ffmpeg/bin",
        "opt/r2/bin",
    ];
    const REMOTION_DIRS: &[&str] = &["opt/node", "opt/node/bin", "opt/remotion/bin"];
    roots
        .iter()
        .flat_map(|root| {
            let mut entries = BASE_DIRS
                .iter()
                .map(|name| root.join(name))
                .collect::<Vec<_>>();
            if can_run_bundled_remotion() {
                entries.extend(REMOTION_DIRS.iter().map(|name| root.join(name)));
            }
            entries
        })
        .filter(|path| path.is_dir())
        .collect()
}

/// The Win7 ia32 supplement intentionally contains a second, x64 Node and
/// Remotion runtime. A 32-bit app under WoW64 may launch it, while a genuine
/// 32-bit Windows installation cannot. Never put that runtime on PATH there.
fn can_run_bundled_remotion() -> bool {
    #[cfg(windows)]
    {
        ["PROCESSOR_ARCHITEW6432", "PROCESSOR_ARCHITECTURE"]
            .iter()
            .filter_map(|key| std::env::var(key).ok())
            .map(|value| value.to_ascii_uppercase())
            .any(|value| matches!(value.as_str(), "AMD64" | "IA64" | "ARM64"))
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Register the complete local supplement toolchain for child processes.
///
/// PATH is kept as the primary command interface, while explicit variables
/// make media and JavaScript runtime discovery deterministic for services that
/// intentionally launch with a reduced environment.
pub fn configure_runtime_env(roots: &[PathBuf]) {
    prepend_tool_paths(roots);
    let first_file = |candidates: &[&str]| -> Option<PathBuf> {
        roots.iter().find_map(|root| {
            candidates
                .iter()
                .map(|name| root.join(name))
                .find(|path| path.is_file())
        })
    };
    if can_run_bundled_remotion() {
        if let Some(node) = first_file(if cfg!(windows) {
            &["opt/node/node.exe", "opt/node/bin/node.exe"]
        } else {
            &["opt/node/bin/node", "opt/node/node"]
        }) {
            std::env::set_var("WUNDER_NODE_BIN", &node);
            if let Some(parent) = node.parent() {
                let npm_cli = parent.join("node_modules/npm/bin/npm-cli.js");
                if npm_cli.is_file() {
                    std::env::set_var("WUNDER_NPM_BIN", &npm_cli);
                }
            }
        }
    }
    if can_run_bundled_remotion() {
        if let Some(remotion_root) = roots
            .iter()
            .map(|root| root.join("opt/remotion"))
            .find(|path| path.join("node_modules").is_dir())
        {
            std::env::set_var("WUNDER_REMOTION_ROOT", &remotion_root);
            let remotion_modules = remotion_root.join("node_modules");
            let mut node_paths = vec![remotion_modules];
            if let Some(existing) = std::env::var_os("NODE_PATH") {
                node_paths.extend(std::env::split_paths(&existing));
            }
            if let Ok(node_path) = std::env::join_paths(node_paths) {
                std::env::set_var("NODE_PATH", node_path);
            }
        }
    }
    if let Some(ffmpeg) = first_file(if cfg!(windows) {
        &["opt/ffmpeg/bin/ffmpeg.exe", "opt/ffmpeg/ffmpeg.exe"]
    } else {
        &["opt/ffmpeg/bin/ffmpeg", "opt/ffmpeg/ffmpeg"]
    }) {
        std::env::set_var("WUNDER_FFMPEG_BIN", &ffmpeg);
    }
    if let Some(ffprobe) = first_file(if cfg!(windows) {
        &["opt/ffmpeg/bin/ffprobe.exe", "opt/ffmpeg/ffprobe.exe"]
    } else {
        &["opt/ffmpeg/bin/ffprobe", "opt/ffmpeg/ffprobe"]
    }) {
        std::env::set_var("WUNDER_FFPROBE_BIN", &ffprobe);
    }
}

/// One bounded discovery pass; repeated imports never grow PATH with duplicates.
pub fn prepend_tool_paths(roots: &[PathBuf]) {
    let mut entries = path_entries(roots);
    if entries.is_empty() {
        return;
    }
    if let Some(existing) = std::env::var_os("PATH") {
        for path in std::env::split_paths(&existing) {
            if !entries.contains(&path) {
                entries.push(path);
            }
        }
    }
    if let Ok(joined) = std::env::join_paths(entries) {
        std::env::set_var("PATH", joined);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_home_wins_over_existing_install_and_appimage_tools() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        let app = temp.path().join("install");
        let image = temp.path().join("image");
        let name = if cfg!(windows) {
            "opt/rg/bin/rg.exe"
        } else {
            "opt/rg/bin/rg"
        };
        for root in [&home, &app, &image] {
            std::fs::create_dir_all(root.join("opt/rg/bin")).unwrap();
            std::fs::write(root.join(name), b"fixture").unwrap();
        }
        let roots = roots(&home, &app, Some(image));
        assert_eq!(find_tool(&roots, Tool::Rg), Some(home.join(name)));
        assert!(path_entries(&roots)[0].starts_with(&home));
        std::fs::remove_file(home.join(name)).unwrap();
        assert_eq!(find_tool(&roots, Tool::Rg), Some(app.join(name)));
        assert_eq!(super::roots(&home, &home, Some(home.clone())), vec![home]);
    }
}
