use super::NativeDesktop;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
use std::process::Command;

#[cfg(windows)]
#[path = "native_cli/windows.rs"]
mod windows;

impl NativeDesktop {
    pub fn launch_cli(&self, workspace_root: &str) -> Result<(), String> {
        let roots = wunder_server::local_bootstrap::supplement::roots(
            &self.desktop.wunder_home,
            &self.desktop.app_dir,
            None,
        );
        let workdir = PathBuf::from(workspace_root.trim());
        let workdir = if workdir.is_dir() {
            workdir
        } else {
            self.desktop.workspace_root.clone()
        };
        let child_path = super::terminal_child_path(&roots, &self.desktop.app_dir)
            .map_err(|error| format!("构造舵机 PATH 失败: {error}"))?;
        let current_exe = std::env::current_exe().ok();
        let executable = resolve_cli(
            &self.desktop.app_dir,
            current_exe.as_deref().and_then(Path::parent),
            &child_path,
        )
        .ok_or_else(|| "未找到舵机程序，请安装舵机组件后重试".to_string())?;
        let arguments = [
            OsString::from("--cd"),
            workdir.as_os_str().to_owned(),
            OsString::from("--user"),
            OsString::from(self.user_id()),
        ];
        spawn_cli(
            &executable,
            &arguments,
            &workdir,
            &self.desktop.wunder_home,
            &child_path,
        )
        .map_err(|error| format!("打开舵机失败: {error}"))
    }
}

fn resolve_cli(app_dir: &Path, executable_dir: Option<&Path>, path: &OsStr) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "wunder-cli.exe"
    } else {
        "wunder-cli"
    };
    executable_dir
        .map(Path::to_path_buf)
        .into_iter()
        .chain([app_dir.join("bin"), app_dir.to_path_buf()])
        .chain(app_dir.parent().map(|parent| parent.join("bin")))
        .chain(std::env::split_paths(path))
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn cli_command(
    program: &OsStr,
    arguments: &[OsString],
    workdir: &Path,
    home: &Path,
    path: &OsStr,
) -> Command {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(workdir)
        .env("WUNDER_HOME", home)
        .env("PATH", path);
    command
}

#[cfg(windows)]
fn spawn_cli(
    executable: &Path,
    arguments: &[OsString],
    workdir: &Path,
    home: &Path,
    path: &OsStr,
) -> std::io::Result<()> {
    windows::spawn(executable, arguments, workdir, home, path)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn spawn_cli(
    executable: &Path,
    arguments: &[OsString],
    workdir: &Path,
    home: &Path,
    path: &OsStr,
) -> std::io::Result<()> {
    let mut last_error = std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "未找到可用的系统终端，请安装 x-terminal-emulator 或 xterm",
    );
    for (program, prefix) in [
        ("x-terminal-emulator", &["-e"][..]),
        ("gnome-terminal", &["--window", "--wait", "--"][..]),
        ("konsole", &["--separate", "-e"][..]),
        ("xfce4-terminal", &["--disable-server", "--execute"][..]),
        ("xterm", &["-e"][..]),
    ] {
        let terminal_arguments = prefix
            .iter()
            .map(|argument| OsString::from(*argument))
            .chain([executable.as_os_str().to_owned()])
            .chain(arguments.iter().cloned())
            .collect::<Vec<_>>();
        match cli_command(
            OsStr::new(program),
            &terminal_arguments,
            workdir,
            home,
            path,
        )
        .spawn()
        {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn spawn_cli(
    _executable: &Path,
    _arguments: &[OsString],
    _workdir: &Path,
    _home: &Path,
    _path: &OsStr,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "请在系统终端中运行 wunder-cli",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_cli_is_preferred_and_missing_component_is_explicit() {
        let directory = tempfile::tempdir().unwrap();
        let app = directory.path().join("app");
        let binary = app.join("bin");
        std::fs::create_dir_all(&binary).unwrap();
        let path = std::env::join_paths([directory.path().join("tools")]).unwrap();
        assert!(resolve_cli(&app, Some(&binary), &path).is_none());
        let name = if cfg!(windows) {
            "wunder-cli.exe"
        } else {
            "wunder-cli"
        };
        let cli = binary.join(name);
        std::fs::write(&cli, []).unwrap();
        assert_eq!(resolve_cli(&app, Some(&binary), &path), Some(cli));
    }

    #[test]
    fn cli_arguments_preserve_workspace_and_environment_without_shell_evaluation() {
        let workspace = Path::new("sample workspace & directory");
        let home = Path::new("sample-home");
        let arguments = [OsString::from("--cd"), workspace.as_os_str().to_owned()];
        let command = cli_command(
            OsStr::new("wunder-cli"),
            &arguments,
            workspace,
            home,
            OsStr::new("sample-bin"),
        );
        assert_eq!(command.get_program(), "wunder-cli");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            arguments
                .iter()
                .map(OsString::as_os_str)
                .collect::<Vec<_>>()
        );
        assert_eq!(command.get_current_dir(), Some(workspace));
        assert!(command
            .get_envs()
            .any(|(key, value)| key == "WUNDER_HOME" && value == Some(home.as_os_str())));
    }

    #[cfg(windows)]
    #[test]
    fn cli_console_fixture() {
        use std::io::IsTerminal;
        let Some(root) = std::env::var_os("WUNDER_HOME") else {
            return;
        };
        let root = PathBuf::from(root);
        if !root.join("sample-cli-fixture").exists() {
            return;
        }
        let result = serde_json::json!({
            "stdin_terminal": std::io::stdin().is_terminal(),
            "stdout_terminal": std::io::stdout().is_terminal(),
            "stderr_terminal": std::io::stderr().is_terminal(),
            "workspace_matches_home": std::env::current_dir().unwrap().canonicalize().unwrap() == root.canonicalize().unwrap(),
        });
        let report = root.join(format!("{}.pending", std::process::id()));
        std::fs::write(&report, result.to_string()).unwrap();
        std::fs::rename(report, root.join(format!("{}.json", std::process::id()))).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn repeated_cli_launches_create_independent_interactive_consoles() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("sample-cli-fixture"), []).unwrap();
        let executable = std::env::current_exe().unwrap();
        let arguments = [
            OsString::from("--exact"),
            OsString::from("native::cli::tests::cli_console_fixture"),
            OsString::from("--nocapture"),
        ];
        let path = std::env::var_os("PATH").unwrap_or_default();
        for _attempt in 0..2 {
            spawn_cli(&executable, &arguments, root.path(), root.path(), &path).unwrap();
        }
        let started = std::time::Instant::now();
        let reports = loop {
            let reports = std::fs::read_dir(root.path())
                .unwrap()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "json")
                })
                .collect::<Vec<_>>();
            if reports.len() == 2 {
                break reports;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(20),
                "CLI console fixture timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        for report in reports {
            let result: serde_json::Value =
                serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
            assert_eq!(result["stdin_terminal"], true);
            assert_eq!(result["stdout_terminal"], true);
            assert_eq!(result["stderr_terminal"], true);
            assert_eq!(result["workspace_matches_home"], true);
        }
    }
}
