//! Shared discovery for optional native runtimes installed under WUNDER_HOME/opt.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub enum Tool {
    Python,
    Git,
    Rg,
    /// Bundled portable PowerShell 7 shipped for Windows 7 hosts that only
    /// have Windows PowerShell 2.0. It is the command tool's default shell, so
    /// discovery has to be explicit: the portable build is called `pwsh.exe`,
    /// which a bare `powershell.exe` lookup can never resolve.
    PowerShell,
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
        Tool::PowerShell if cfg!(windows) => &[
            "opt/powershell/pwsh.exe",
            "opt/powershell/powershell.exe",
            "opt/powershell/bin/pwsh.exe",
            "opt/powershell/bin/powershell.exe",
        ],
        Tool::PowerShell => &[
            "opt/powershell/pwsh",
            "opt/powershell/bin/pwsh",
        ],
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
        "opt/powershell",
        "opt/powershell/bin",
    ];
    roots
        .iter()
        .flat_map(|root| {
            BASE_DIRS
                .iter()
                .map(|name| root.join(name))
                .collect::<Vec<_>>()
        })
        .filter(|path| path.is_dir())
        .collect()
}

/// Register the complete local supplement toolchain for child processes.
///
/// PATH is kept as the primary command interface, while explicit variables
/// make media runtime discovery deterministic for services that intentionally
/// launch with a reduced environment.
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
    // The portable PowerShell is `pwsh.exe`, so PATH alone cannot redirect the
    // command tool's `powershell.exe` lookup. Publishing the resolved absolute
    // path keeps shell selection explicit and lets the runtime probe that exact
    // binary (and fall back to the system one when it refuses to start).
    if let Some(pwsh) = find_tool(roots, Tool::PowerShell) {
        std::env::set_var("WUNDER_POWERSHELL_EXE", &pwsh);
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

    #[test]
    fn bundled_powershell_resolves_by_its_own_executable_name() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        let pwsh = if cfg!(windows) {
            "opt/powershell/pwsh.exe"
        } else {
            "opt/powershell/pwsh"
        };
        std::fs::create_dir_all(home.join("opt/powershell")).unwrap();
        std::fs::write(home.join(pwsh), b"fixture").unwrap();
        let roots = roots(&home, &home, None);
        assert_eq!(find_tool(&roots, Tool::PowerShell), Some(home.join(pwsh)));
        assert!(path_entries(&roots).contains(&home.join("opt/powershell")));
    }
}
