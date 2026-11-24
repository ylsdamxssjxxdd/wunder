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
    /// Node runtime. The supplement ships no standalone `opt/node`: the only
    /// copy is the one Playwright bundles next to its Python driver, so the
    /// lookup has to follow the interpreter layout instead of a fixed path.
    Node,
    /// POSIX shell for `.sh` syntax checks. It is staged under `opt/bash/bin`
    /// from the same Git for Windows release as MinGit, because MinGit is the
    /// non-interactive distribution and ships no bash at all. The `opt/git`
    /// arms stay for packs and imports that carry a full Git for Windows tree.
    Bash,
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
    if matches!(tool, Tool::Node) {
        return roots.iter().find_map(|root| node_bin(root));
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
        Tool::PowerShell if cfg!(windows) => &[
            "opt/powershell/pwsh.exe",
            "opt/powershell/powershell.exe",
            "opt/powershell/bin/pwsh.exe",
            "opt/powershell/bin/powershell.exe",
        ],
        Tool::PowerShell => &["opt/powershell/pwsh", "opt/powershell/bin/pwsh"],
        Tool::Bash if cfg!(windows) => &[
            "opt/bash/bin/bash.exe",
            "opt/bash/bin/sh.exe",
            "opt/git/bin/bash.exe",
            "opt/git/usr/bin/bash.exe",
            "opt/git/mingw64/bin/bash.exe",
        ],
        Tool::Bash => &["opt/bash/bin/bash", "opt/bash/bash", "opt/git/bin/bash"],
        // Handled by `node_bin` above; kept for match exhaustiveness.
        Tool::Node => &[],
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
        // 打包的 bash 闭包（bash.exe + sh.exe + msys-2.0.dll）：进了 PATH，
        // `bash -n`、技能脚本和用户的 shell 命令才都能解析到同一个 bash。
        "opt/bash/bin",
        "opt/rg",
        "opt/rg/bin",
        "opt/ripgrep",
        "opt/ripgrep/bin",
        "opt/ffmpeg",
        "opt/ffmpeg/bin",
        "opt/7zip",
        "opt/7zip/bin",
        "opt/powershell",
        "opt/powershell/bin",
    ];
    let mut entries: Vec<PathBuf> = roots
        .iter()
        .flat_map(|root| {
            BASE_DIRS
                .iter()
                .map(|name| root.join(name))
                .collect::<Vec<_>>()
        })
        .filter(|path| path.is_dir())
        .collect();
    // The Playwright driver node is the supplement's only Node runtime and it
    // sits in a Python-versioned directory, so it is discovered instead of
    // listed. Without it in PATH no child process (`node --check`, skill
    // scripts, npm-style helpers) can resolve a bare `node` on Win7.
    for root in roots {
        if let Some(dir) = node_bin(root).and_then(|node| node.parent().map(Path::to_path_buf)) {
            if !entries.contains(&dir) {
                entries.push(dir);
            }
        }
    }
    entries
}

/// Playwright's driver node, found under whichever layout the interpreter
/// uses: Windows embeddable (`Lib/site-packages`), python-build-standalone
/// (`lib/python3.X/site-packages`) or a Debian-style `dist-packages`. A
/// standalone `opt/node` tree is accepted too in case a pack ships one later.
pub fn node_bin(root: &Path) -> Option<PathBuf> {
    const FIXED: &[&str] = &[
        "opt/node/node.exe",
        "opt/node/bin/node.exe",
        "opt/nodejs/node.exe",
        "opt/nodejs/bin/node.exe",
        "opt/node/node",
        "opt/node/bin/node",
        "opt/nodejs/bin/node",
        "opt/python/node.exe",
        "opt/python/bin/node",
        "opt/python/Lib/site-packages/playwright/driver/node.exe",
        "opt/python/lib/site-packages/playwright/driver/node",
        "opt/python/lib/site-packages/playwright/driver/node.exe",
    ];
    if let Some(found) = FIXED
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
    {
        return Some(found);
    }
    let packages = [
        "site-packages/playwright/driver/node",
        "site-packages/playwright/driver/node.exe",
        "dist-packages/playwright/driver/node",
        "dist-packages/playwright/driver/node.exe",
    ];
    // Bounded: one directory level under opt/python/lib, no recursive walk.
    for entry in std::fs::read_dir(root.join("opt/python/lib")).ok()?.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("python") {
            continue;
        }
        for leaf in packages {
            let candidate = entry.path().join(leaf);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Supplement roots that apply to this process, resolved from `WUNDER_HOME`
/// and the executable location. Engine-side probes that run outside the
/// desktop settings refresh (syntax checks, CLI, bundled tool lookup) use this
/// instead of re-deriving the layout.
pub fn local_roots() -> Vec<PathBuf> {
    let home = super::resolve_home_from_env_or_user();
    let app_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| home.clone());
    let appimage = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .and_then(|path| path.parent().map(Path::to_path_buf));
    roots(&home, &app_dir, appimage)
}

/// Browser executable shipped in a supplement.
///
/// Win7 stages Chrome 109 inside the Playwright browser-registry layout
/// (`opt/browser/chromium-<revision>/chrome-win/chrome.exe`) so a plain
/// `playwright.chromium.launch()` resolves it; older packs keep the bare
/// `opt/browser/chrome-win/chrome.exe` layout, which stays a fallback.
pub fn bundled_browser_bin(roots: &[PathBuf]) -> Option<PathBuf> {
    let layouts: &[&str] = if cfg!(windows) {
        &[
            "chrome-win/chrome.exe",
            "chrome-win64/chrome.exe",
            "chrome.exe",
        ]
    } else {
        &[
            "chrome-linux64/chrome",
            "chrome-linux-arm64/chrome",
            "chrome-linux/chrome",
            "chrome",
        ]
    };
    roots.iter().find_map(|root| {
        let browser_root = root.join("opt/browser");
        if let Some(path) = layouts
            .iter()
            .map(|layout| browser_root.join(layout))
            .find(|path| path.is_file())
        {
            return Some(path);
        }
        registry_browser_dirs(&browser_root)
            .into_iter()
            .find_map(|dir| {
                layouts
                    .iter()
                    .map(|layout| dir.join(layout))
                    .find(|path| path.is_file())
            })
    })
}

/// Root directory Playwright's browser registry should resolve against: the
/// Playwright install shipped with the supplement first, then an `opt/browser`
/// tree that already uses the `chromium-<revision>` registry layout.
pub fn playwright_browsers_root(roots: &[PathBuf]) -> Option<PathBuf> {
    roots.iter().find_map(|root| {
        let standard = root.join("opt/python/ms-playwright");
        if standard.is_dir() {
            return Some(standard);
        }
        let browser_root = root.join("opt/browser");
        if registry_browser_dirs(&browser_root).is_empty() {
            return None;
        }
        Some(browser_root)
    })
}

fn registry_browser_dirs(browser_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(browser_root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("chromium-"))
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    dirs
}

fn first_file(roots: &[PathBuf], candidates: &[&str]) -> Option<PathBuf> {
    roots.iter().find_map(|root| {
        candidates
            .iter()
            .map(|name| root.join(name))
            .find(|path| path.is_file())
    })
}

/// Media encoder shipped in a supplement (`opt/ffmpeg`).
pub fn find_ffmpeg(roots: &[PathBuf]) -> Option<PathBuf> {
    first_file(
        roots,
        if cfg!(windows) {
            &["opt/ffmpeg/bin/ffmpeg.exe", "opt/ffmpeg/ffmpeg.exe"]
        } else {
            &["opt/ffmpeg/bin/ffmpeg", "opt/ffmpeg/ffmpeg"]
        },
    )
}

/// Media probe shipped next to [`find_ffmpeg`].
pub fn find_ffprobe(roots: &[PathBuf]) -> Option<PathBuf> {
    first_file(
        roots,
        if cfg!(windows) {
            &["opt/ffmpeg/bin/ffprobe.exe", "opt/ffmpeg/ffprobe.exe"]
        } else {
            &["opt/ffmpeg/bin/ffprobe", "opt/ffmpeg/ffprobe"]
        },
    )
}

/// Archive tool shipped in a supplement (`opt/7zip`).
pub fn find_seven_zip(roots: &[PathBuf]) -> Option<PathBuf> {
    first_file(
        roots,
        if cfg!(windows) {
            &[
                "opt/7zip/7z.exe",
                "opt/7zip/bin/7z.exe",
                "opt/7zip/7za.exe",
                "opt/7zip/bin/7za.exe",
            ]
        } else {
            &[
                "opt/7zip/7z",
                "opt/7zip/bin/7z",
                "opt/7zip/7za",
                "opt/7zip/bin/7za",
            ]
        },
    )
}

/// One runtime the supplement ships, named the way tool specs and prompts
/// refer to it. `name` is a stable bare command (`python`, `ffmpeg`, …) so the
/// same inventory can feed both the system prompt and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledRuntime {
    pub name: &'static str,
    pub path: PathBuf,
}

/// Every supplement runtime that actually exists under `roots`.
///
/// The system prompt has to describe the offline toolchain instead of letting
/// the model guess host paths: the bundled runtimes live under `<home>/opt`,
/// which is not in any standard search location. Discovery stays here so the
/// prompt, the process environment and the settings panel agree on one answer.
pub fn inventory(roots: &[PathBuf]) -> Vec<BundledRuntime> {
    let mut runtimes = Vec::new();
    let mut push = |name: &'static str, path: Option<PathBuf>| {
        if let Some(path) = path {
            runtimes.push(BundledRuntime { name, path });
        }
    };
    push("python", find_tool(roots, Tool::Python));
    push("node", find_tool(roots, Tool::Node));
    push("git", find_tool(roots, Tool::Git));
    push("rg", find_tool(roots, Tool::Rg));
    push("bash", find_tool(roots, Tool::Bash));
    push("pwsh", find_tool(roots, Tool::PowerShell));
    push("ffmpeg", find_ffmpeg(roots));
    push("ffprobe", find_ffprobe(roots));
    push("7z", find_seven_zip(roots));
    push("browser", bundled_browser_bin(roots));
    runtimes
}

/// Register the complete local supplement toolchain for child processes.
///
/// PATH is kept as the primary command interface, while explicit variables
/// make media runtime discovery deterministic for services that intentionally
/// launch with a reduced environment.
pub fn configure_runtime_env(roots: &[PathBuf]) {
    prepend_tool_paths(roots);
    // Playwright launches its bundled driver through Node. The Win7 supplement
    // replaces the x64 driver runtime with a 32-bit Node 16 that is outside
    // Node's supported OS matrix, so every child that imports Playwright (the
    // browser bridge and skill scripts alike) needs this gate in its
    // environment or Node refuses to start.
    if cfg!(windows) {
        std::env::set_var("NODE_SKIP_PLATFORM_CHECK", "1");
    }
    // Skills and ad-hoc Python scripts launch Playwright directly, and without
    // this the driver only looks in its per-user cache, never at the browser
    // shipped with the supplement. An explicit caller value always wins.
    if let Some(browsers_root) = playwright_browsers_root(roots) {
        if std::env::var_os("PLAYWRIGHT_BROWSERS_PATH").is_none() {
            std::env::set_var("PLAYWRIGHT_BROWSERS_PATH", &browsers_root);
        }
        if cfg!(windows) && std::env::var_os("PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS").is_none()
        {
            // The Win7 pack registers Chrome 109 under a Playwright revision
            // directory. That build predates the host-dependency gate run on
            // x64 Windows, so skip it; a missing DLL still fails at launch.
            std::env::set_var("PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS", "1");
        }
    }
    if let Some(browser) = bundled_browser_bin(roots) {
        if std::env::var_os("WUNDER_BROWSER_BIN").is_none() {
            std::env::set_var("WUNDER_BROWSER_BIN", &browser);
        }
    }
    if let Some(ffmpeg) = find_ffmpeg(roots) {
        std::env::set_var("WUNDER_FFMPEG_BIN", &ffmpeg);
    }
    if let Some(ffprobe) = find_ffprobe(roots) {
        std::env::set_var("WUNDER_FFPROBE_BIN", &ffprobe);
    }
    // The portable PowerShell is `pwsh.exe`, so PATH alone cannot redirect the
    // command tool's `powershell.exe` lookup. Publishing the resolved absolute
    // path keeps shell selection explicit and lets the runtime probe that exact
    // binary (and fall back to the system one when it refuses to start).
    if let Some(pwsh) = find_tool(roots, Tool::PowerShell) {
        std::env::set_var("WUNDER_POWERSHELL_EXE", &pwsh);
    }
    // Portable git bundles ship `bin/` + `libexec/` but keep the exec path of
    // whatever host assembled them, so every helper loaded from disk
    // (`git-remote-https`, `git-remote-http`, `git-submodule`) is unreachable
    // while builtins still work — `git pull` over https dies with
    // "'remote-https' is not a git command". Publishing the real libexec keeps
    // the bundle relocatable; a caller-provided value always wins.
    if let Some(exec_path) = git_exec_path(roots) {
        if std::env::var_os("GIT_EXEC_PATH").is_none() {
            std::env::set_var("GIT_EXEC_PATH", &exec_path);
        }
        if std::env::var_os("GIT_TEMPLATE_DIR").is_none() {
            if let Some(templates) = git_template_dir(roots) {
                std::env::set_var("GIT_TEMPLATE_DIR", &templates);
            }
        }
    }
}

/// Helpers of a relocatable git live beside the binary, in `libexec/git-core`
/// on Linux/macOS layouts and `mingw64/libexec/git-core` on Git for Windows.
/// A root without that directory is skipped so a system git never inherits a
/// foreign exec path.
pub fn git_exec_path(roots: &[PathBuf]) -> Option<PathBuf> {
    first_dir(
        roots,
        &[
            "opt/git/libexec/git-core",
            "opt/git/mingw64/libexec/git-core",
        ],
    )
}

/// `git init` and `git clone` copy their hooks and sample files from here;
/// without it the bundle warns "unable to copy template" on every new repo.
pub fn git_template_dir(roots: &[PathBuf]) -> Option<PathBuf> {
    first_dir(
        roots,
        &[
            "opt/git/share/git-core/templates",
            "opt/git/mingw64/share/git-core/templates",
        ],
    )
}

fn first_dir(roots: &[PathBuf], candidates: &[&str]) -> Option<PathBuf> {
    roots.iter().find_map(|root| {
        candidates
            .iter()
            .map(|name| root.join(name))
            .find(|path| path.is_dir())
    })
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

    #[test]
    fn portable_git_publishes_exec_path_and_template_dir() {
        // A relocated bundle keeps its helpers next to the binary instead of the
        // build host path compiled into git, so both directories must be found.
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        for dir in [
            "opt/git/libexec/git-core",
            "opt/git/share/git-core/templates",
        ] {
            std::fs::create_dir_all(home.join(dir)).unwrap();
        }
        let roots = roots(&home, &home, None);
        assert_eq!(
            git_exec_path(&roots),
            Some(home.join("opt/git/libexec/git-core"))
        );
        assert_eq!(
            git_template_dir(&roots),
            Some(home.join("opt/git/share/git-core/templates"))
        );
    }

    #[test]
    fn git_exec_path_ignores_a_system_git_without_libexec() {
        // Publishing an exec path for a git that has no libexec would break every
        // subcommand, so a bare bin/ tree has to stay untouched.
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        std::fs::create_dir_all(home.join("opt/git/bin")).unwrap();
        std::fs::write(home.join("opt/git/bin/git"), b"fixture").unwrap();
        let roots = roots(&home, &home, None);
        assert_eq!(git_exec_path(&roots), None);
        assert_eq!(git_template_dir(&roots), None);
    }

    #[test]
    fn playwright_driver_node_is_found_and_reaches_path() {
        // The supplement ships no `opt/node`; Node only exists inside the
        // Playwright driver, and on Linux that path carries the Python version.
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        let node = if cfg!(windows) {
            "opt/python/Lib/site-packages/playwright/driver/node.exe"
        } else {
            "opt/python/lib/python3.8/site-packages/playwright/driver/node"
        };
        std::fs::create_dir_all(home.join(node).parent().unwrap()).unwrap();
        std::fs::write(home.join(node), b"fixture").unwrap();
        let roots = roots(&home, &home, None);
        assert_eq!(
            find_tool(&roots, Tool::Node),
            Some(home.join(node)),
            "driver node must be discovered from the interpreter layout"
        );
        let driver_dir = home.join(node).parent().unwrap().to_path_buf();
        assert!(path_entries(&roots).contains(&driver_dir));
    }

    #[test]
    fn node_lookup_accepts_a_standalone_opt_tree_and_stays_empty_without_one() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        std::fs::create_dir_all(home.join("opt/venv/bin")).unwrap();
        assert_eq!(node_bin(&home), None);
        let node = if cfg!(windows) {
            "opt/node/node.exe"
        } else {
            "opt/node/bin/node"
        };
        std::fs::create_dir_all(home.join(node).parent().unwrap()).unwrap();
        std::fs::write(home.join(node), b"fixture").unwrap();
        assert_eq!(node_bin(&home), Some(home.join(node)));
    }

    #[test]
    fn a_bundled_shell_tree_is_discovered_for_syntax_checks() {
        // MinGit itself ships no bash; this covers a pack that stages one under
        // `opt/bash`, plus the Linux layout, so `.sh` validation can use it.
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        let bash = if cfg!(windows) {
            "opt/git/usr/bin/bash.exe"
        } else {
            "opt/bash/bin/bash"
        };
        std::fs::create_dir_all(home.join(bash).parent().unwrap()).unwrap();
        std::fs::write(home.join(bash), b"fixture").unwrap();
        let roots = roots(&home, &home, None);
        assert_eq!(find_tool(&roots, Tool::Bash), Some(home.join(bash)));
    }

    #[test]
    fn staged_bash_closure_wins_over_a_full_git_tree() {
        // 补充包的 bash 闭包落在 opt/bash/bin：Windows 上要排在 opt/git 之前
        // （MinGit 没有 shell，只有导入过完整 Git 树才有），并且该目录要进 PATH，
        // 裸命令 `bash` 才解析得到。
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        let exe = if cfg!(windows) { "bash.exe" } else { "bash" };
        let staged = home.join("opt/bash/bin").join(exe);
        std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
        std::fs::write(&staged, b"fixture").unwrap();
        if cfg!(windows) {
            let full_git = home.join("opt/git/usr/bin/bash.exe");
            std::fs::create_dir_all(full_git.parent().unwrap()).unwrap();
            std::fs::write(&full_git, b"fixture").unwrap();
        }
        let roots = roots(&home, &home, None);
        assert_eq!(find_tool(&roots, Tool::Bash), Some(staged));
        assert!(path_entries(&roots).contains(&home.join("opt/bash/bin")));
    }

    #[test]
    fn playwright_registry_layout_resolves_the_bundled_browser() {
        // Win7 stages Chrome 109 as a Playwright revision directory so a plain
        // chromium.launch() finds it without any executable_path argument.
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        let exe = if cfg!(windows) {
            "opt/browser/chromium-1091/chrome-win/chrome.exe"
        } else {
            "opt/browser/chromium-1091/chrome-linux64/chrome"
        };
        std::fs::create_dir_all(home.join(exe).parent().unwrap()).unwrap();
        std::fs::write(home.join(exe), b"fixture").unwrap();
        let roots = roots(&home, &home, None);
        assert_eq!(bundled_browser_bin(&roots), Some(home.join(exe)));
        assert_eq!(
            playwright_browsers_root(&roots),
            Some(home.join("opt/browser"))
        );
    }

    #[test]
    fn bare_supplement_browser_keeps_working_without_a_registry_dir() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        let exe = if cfg!(windows) {
            "opt/browser/chrome-win/chrome.exe"
        } else {
            "opt/browser/chrome-linux64/chrome"
        };
        std::fs::create_dir_all(home.join(exe).parent().unwrap()).unwrap();
        std::fs::write(home.join(exe), b"fixture").unwrap();
        let roots = roots(&home, &home, None);
        assert_eq!(bundled_browser_bin(&roots), Some(home.join(exe)));
        assert_eq!(playwright_browsers_root(&roots), None);
    }

    #[test]
    fn bundled_sevenzip_dir_reaches_path() {
        // The archive extractor resolves 7z through PATH alone, so both staged
        // layouts (Windows console pair at the root, Linux 7zz under bin) must
        // be forwarded into PATH.
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("profile");
        let exe = if cfg!(windows) { "7z.exe" } else { "7z" };
        std::fs::create_dir_all(home.join("opt/7zip")).unwrap();
        std::fs::write(home.join("opt/7zip").join(exe), b"fixture").unwrap();
        let roots = roots(&home, &home, None);
        assert!(path_entries(&roots).contains(&home.join("opt/7zip")));
    }
}
