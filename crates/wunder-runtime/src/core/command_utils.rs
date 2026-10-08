use std::io;
use std::path::Path;
#[cfg(not(windows))]
use std::path::PathBuf;
use std::process::Command as StdCommand;
#[cfg(windows)]
use std::{env, path::PathBuf, sync::OnceLock};
use tokio::process::Command;

const SHELL_BUILTINS: &[&str] = &[
    ".", "alias", "bg", "bind", "break", "builtin", "cd", "command", "continue", "declare", "dirs",
    "disown", "eval", "exec", "exit", "export", "fg", "getopts", "hash", "help", "history", "jobs",
    "local", "logout", "popd", "pushd", "readonly", "return", "set", "shift", "source", "suspend",
    "trap", "typeset", "ulimit", "umask", "unalias", "unset", "wait",
];

const SHELL_META_CHARS: &[char] = &[
    '|', '&', ';', '<', '>', '(', ')', '$', '`', '*', '?', '~', '{', '}', '[', ']', '#', '\n', '\r',
];

#[cfg(windows)]
const WINDOWS_CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn apply_platform_spawn_options(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.as_std_mut().creation_flags(WINDOWS_CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

pub fn apply_platform_spawn_options_std(cmd: &mut StdCommand) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(WINDOWS_CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

// ---------------------------------------------------------------------------
// Shell selection
// ---------------------------------------------------------------------------

/// Shell flavour used for commands that require shell interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    Cmd,
    PowerShell,
    Bash,
}

/// Resolved shell launcher: which flavour, the bash executable path and the
/// Windows PATH entries that must be present so the POSIX toolchain resolves.
#[derive(Clone, Debug)]
pub struct ShellLaunch {
    pub kind: ShellKind,
    pub bash_bin: Option<PathBuf>,
    pub path_dirs: Vec<PathBuf>,
}

impl Default for ShellLaunch {
    fn default() -> Self {
        Self {
            kind: default_shell_kind(),
            bash_bin: None,
            path_dirs: Vec::new(),
        }
    }
}

fn default_shell_kind() -> ShellKind {
    #[cfg(windows)]
    {
        ShellKind::Cmd
    }
    #[cfg(not(windows))]
    {
        ShellKind::Bash
    }
}

/// Platform default shell (PowerShell preferred on Windows, bash on Unix).
pub fn platform_default_shell() -> ShellLaunch {
    #[cfg(windows)]
    {
        let kind = if prefer_powershell() {
            ShellKind::PowerShell
        } else {
            ShellKind::Cmd
        };
        ShellLaunch {
            kind,
            bash_bin: None,
            path_dirs: Vec::new(),
        }
    }
    #[cfg(not(windows))]
    {
        ShellLaunch {
            kind: ShellKind::Bash,
            bash_bin: Some(PathBuf::from("bash")),
            path_dirs: Vec::new(),
        }
    }
}

/// Human-readable shell name recorded in command session metadata.
pub fn shell_display_name(shell: &ShellLaunch) -> String {
    match shell.kind {
        ShellKind::Cmd => "cmd.exe".to_string(),
        ShellKind::PowerShell => "powershell.exe".to_string(),
        ShellKind::Bash => "bash".to_string(),
    }
}

/// Derive the Git for Windows install root from a git executable path.
/// Handles `<root>/cmd/git.exe`, `<root>/bin/git.exe` and
/// `<root>/mingw32/bin/git.exe` layouts.
#[cfg(windows)]
fn git_install_root(git_bin: &Path) -> Option<PathBuf> {
    let bin_dir = git_bin.parent()?;
    let dir_name = bin_dir
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if dir_name == "cmd" || dir_name == "bin" {
        return bin_dir.parent().map(Path::to_path_buf);
    }
    // Layout <root>/mingw32/bin/git.exe (or mingw64).
    let flave_dir = bin_dir.parent()?;
    let flave_name = flave_dir
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if flave_name == "mingw32" || flave_name == "mingw64" {
        return flave_dir.parent().map(Path::to_path_buf);
    }
    bin_dir.parent().map(Path::to_path_buf)
}

/// Resolve the Git Bash launcher from a git executable: locate bash and the
/// PATH directories carrying the POSIX toolchain. Returns None when the Git
/// distribution does not ship bash (e.g. a bare MinGit).
#[cfg(windows)]
pub fn resolve_git_bash_shell(git_bin: &Path) -> Option<ShellLaunch> {
    let git_root = git_install_root(git_bin)?;
    let bash_bin = [
        git_root.join("bin").join("bash.exe"),
        git_root.join("usr").join("bin").join("bash.exe"),
        git_root.join("usr").join("bin").join("sh.exe"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())?;
    let path_dirs = [
        git_root.join("usr").join("bin"),
        git_root.join("mingw32").join("bin"),
        git_root.join("cmd"),
    ]
    .into_iter()
    .filter(|candidate| candidate.is_dir())
    .collect::<Vec<_>>();
    Some(ShellLaunch {
        kind: ShellKind::Bash,
        bash_bin: Some(bash_bin),
        path_dirs,
    })
}

/// Build a shell interpreter command for the resolved shell.
pub fn build_shell_command_with_shell(command: &str, cwd: &Path, shell: &ShellLaunch) -> Command {
    #[cfg(windows)]
    {
        match shell.kind {
            ShellKind::Bash => {
                let bash_bin = shell
                    .bash_bin
                    .as_ref()
                    .expect("bash shell requires bash_bin");
                let mut cmd = Command::new(bash_bin);
                // Non-login shell; PATH carrying the POSIX toolchain is applied
                // by the caller so it works even on stripped MinGit layouts.
                cmd.arg("-c").arg(command).current_dir(cwd);
                apply_platform_spawn_options(&mut cmd);
                cmd
            }
            ShellKind::PowerShell => build_powershell_command(command, cwd),
            ShellKind::Cmd => build_cmd_command(command, cwd),
        }
    }

    #[cfg(not(windows))]
    {
        let _ = shell;
        let mut cmd = Command::new("bash");
        cmd.arg("-lc").arg(command).current_dir(cwd);
        cmd
    }
}

#[cfg(windows)]
fn build_powershell_command(command: &str, cwd: &Path) -> Command {
    let mut cmd = Command::new("powershell.exe");
    cmd.arg("-NoLogo").arg("-NoProfile").arg("-Command");
    // Force UTF-8 output for better cross-terminal decoding.
    let wrapped = format!(
        "$Utf8 = [System.Text.UTF8Encoding]::new($false); [Console]::InputEncoding = $Utf8; [Console]::OutputEncoding = $Utf8; $OutputEncoding = $Utf8; {command}"
    );
    cmd.arg(wrapped).current_dir(cwd);
    apply_platform_spawn_options(&mut cmd);
    cmd
}

#[cfg(windows)]
fn build_cmd_command(command: &str, cwd: &Path) -> Command {
    let mut cmd = Command::new("cmd.exe");
    cmd.arg("/C").arg(command).current_dir(cwd);
    apply_platform_spawn_options(&mut cmd);
    cmd
}

pub fn build_direct_command(command: &str, cwd: &Path) -> Option<Command> {
    let trimmed = command.trim();
    if trimmed.is_empty() || contains_shell_meta(trimmed) {
        return None;
    }
    let parts = shell_words::split(trimmed).ok()?;
    if parts.is_empty() {
        return None;
    }
    let (envs, program_index) = parse_env_prefix(&parts);
    let program = parts.get(program_index)?;
    if is_shell_builtin(program) {
        return None;
    }
    let mut cmd = Command::new(program);
    if program_index + 1 < parts.len() {
        cmd.args(&parts[program_index + 1..]);
    }
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.current_dir(cwd);
    apply_platform_spawn_options(&mut cmd);
    Some(cmd)
}

pub fn build_direct_command_with_python_override(
    command: &str,
    cwd: &Path,
    python_bin: &Path,
) -> Option<Command> {
    build_direct_command_with_overrides(
        command,
        cwd,
        Some(python_bin),
        CommandProgramOverrides::default(),
    )
}

#[derive(Clone, Debug, Default)]
pub struct CommandProgramOverrides {
    pub pip_bin: Option<PathBuf>,
    pub git_bin: Option<PathBuf>,
    pub rg_bin: Option<PathBuf>,
}

pub fn build_direct_command_with_overrides(
    command: &str,
    cwd: &Path,
    python_bin: Option<&Path>,
    overrides: CommandProgramOverrides,
) -> Option<Command> {
    let trimmed = command.trim();
    if trimmed.is_empty() || contains_shell_meta(trimmed) {
        return None;
    }
    let parts = shell_words::split(trimmed).ok()?;
    if parts.is_empty() {
        return None;
    }
    let (envs, program_index) = parse_env_prefix(&parts);
    let program = parts.get(program_index)?;
    if is_shell_builtin(program) {
        return None;
    }
    let pip_invocation = command_is_pip_invocation(program);
    let mut cmd = if is_python_program(program) {
        Command::new(python_bin?)
    } else if pip_invocation {
        if let Some(pip_bin) = overrides.pip_bin.as_ref() {
            command_for_executable_path(pip_bin)
        } else if let Some(python_bin) = python_bin {
            let mut python_cmd = Command::new(python_bin);
            python_cmd.arg("-m").arg("pip");
            python_cmd
        } else {
            Command::new(program)
        }
    } else if is_git_program(program) {
        match overrides.git_bin.as_ref() {
            Some(git_bin) => command_for_executable_path(git_bin),
            None => Command::new(program),
        }
    } else if is_rg_program(program) {
        match overrides.rg_bin.as_ref() {
            Some(rg_bin) => command_for_executable_path(rg_bin),
            None => Command::new(program),
        }
    } else {
        Command::new(program)
    };
    if program_index + 1 < parts.len() {
        cmd.args(&parts[program_index + 1..]);
    }
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.current_dir(cwd);
    apply_platform_spawn_options(&mut cmd);
    Some(cmd)
}

fn command_for_executable_path(path: &Path) -> Command {
    #[cfg(windows)]
    {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension == "cmd" || extension == "bat" {
            let mut cmd = Command::new("cmd.exe");
            cmd.arg("/C").arg(path);
            return cmd;
        }
    }
    Command::new(path)
}

pub fn build_shell_command(command: &str, cwd: &Path) -> Command {
    build_shell_command_with_shell(command, cwd, &platform_default_shell())
}

pub fn resolve_shell_name(command: &str) -> &'static str {
    let _ = command;
    match platform_default_shell().kind {
        ShellKind::Cmd => "cmd.exe",
        ShellKind::PowerShell => "powershell.exe",
        ShellKind::Bash => "bash",
    }
}

#[cfg(windows)]
fn prefer_powershell() -> bool {
    static PREFER_POWERSHELL: OnceLock<bool> = OnceLock::new();
    *PREFER_POWERSHELL.get_or_init(powershell_available)
}

#[cfg(windows)]
fn powershell_available() -> bool {
    if let Some(system_root) = env::var_os("SystemRoot") {
        let default_path = PathBuf::from(system_root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        if default_path.is_file() {
            return true;
        }
    }

    binary_exists_in_path("powershell.exe")
}

#[cfg(windows)]
fn binary_exists_in_path(binary: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|dir| dir.join(binary).is_file())
}

pub fn is_not_found_error(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::NotFound
}

fn contains_shell_meta(command: &str) -> bool {
    command.chars().any(|ch| SHELL_META_CHARS.contains(&ch))
}

fn parse_env_prefix(parts: &[String]) -> (Vec<(String, String)>, usize) {
    let mut envs = Vec::new();
    let mut index = 0;
    for part in parts {
        if let Some((key, value)) = parse_env_assignment(part) {
            envs.push((key, value));
            index += 1;
        } else {
            break;
        }
    }
    if index >= parts.len() {
        (Vec::new(), 0)
    } else {
        (envs, index)
    }
}

fn parse_env_assignment(part: &str) -> Option<(String, String)> {
    let (key, value) = part.split_once('=')?;

    if is_valid_env_key(key) {
        Some((key.to_string(), value.to_string()))
    } else {
        None
    }
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let first = chars.next();
    let Some(first) = first else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_shell_builtin(program: &str) -> bool {
    SHELL_BUILTINS
        .iter()
        .any(|item| item.eq_ignore_ascii_case(program))
}

fn is_python_program(program: &str) -> bool {
    let lower = program.to_ascii_lowercase();
    lower == "python"
        || lower == "python3"
        || lower == "py"
        || lower == "py.exe"
        || lower.starts_with("python3.")
}

fn command_is_pip_invocation(program: &str) -> bool {
    let lower = program.to_ascii_lowercase();
    lower == "pip"
        || lower == "pip.exe"
        || lower == "pip3"
        || lower == "pip3.exe"
        || lower.starts_with("pip3.")
}

fn is_git_program(program: &str) -> bool {
    let lower = program.to_ascii_lowercase();
    lower == "git" || lower == "git.exe"
}

fn is_rg_program(program: &str) -> bool {
    let lower = program.to_ascii_lowercase();
    lower == "rg" || lower == "rg.exe"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[cfg(windows)]
    #[test]
    fn build_shell_command_prefers_windows_shell() {
        let command = build_shell_command("pwd", Path::new("."));
        let program = command
            .as_std()
            .get_program()
            .to_string_lossy()
            .to_ascii_lowercase();

        if powershell_available() {
            assert!(program.ends_with("powershell.exe"));
        } else {
            assert!(program.ends_with("cmd.exe"));
        }
    }

    #[cfg(windows)]
    #[test]
    fn build_shell_command_keeps_powershell_for_chained_commands_when_available() {
        let command = build_shell_command("cargo --version && rustc --version", Path::new("."));
        let program = command
            .as_std()
            .get_program()
            .to_string_lossy()
            .to_ascii_lowercase();
        if powershell_available() {
            assert!(program.ends_with("powershell.exe"));
        } else {
            assert!(program.ends_with("cmd.exe"));
        }
    }

    #[cfg(windows)]
    #[test]
    fn build_shell_command_keeps_powershell_for_stderr_merge_when_available() {
        let command = build_shell_command("cargo test 2>&1", Path::new("."));
        let program = command
            .as_std()
            .get_program()
            .to_string_lossy()
            .to_ascii_lowercase();
        if powershell_available() {
            assert!(program.ends_with("powershell.exe"));
        } else {
            assert!(program.ends_with("cmd.exe"));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn build_shell_command_uses_bash_on_unix() {
        let command = build_shell_command("pwd", Path::new("."));
        let program = command
            .as_std()
            .get_program()
            .to_string_lossy()
            .to_ascii_lowercase();
        assert!(program.ends_with("bash"));
    }

    #[test]
    fn build_direct_command_overrides_python_program() {
        let python_bin = Path::new("/tmp/wunder-python/bin/python3");
        let command =
            build_direct_command_with_python_override("python -V", Path::new("."), python_bin)
                .expect("direct command");
        assert_eq!(
            command.as_std().get_program().to_string_lossy(),
            python_bin.to_string_lossy()
        );
    }

    #[test]
    fn build_direct_command_keeps_non_python_program() {
        let command = build_direct_command_with_python_override(
            "echo hello",
            Path::new("."),
            Path::new("/tmp/wunder-python/bin/python3"),
        )
        .expect("direct command");
        assert_eq!(command.as_std().get_program().to_string_lossy(), "echo");
    }

    #[test]
    fn build_direct_command_recognizes_env_prefix_and_arguments() {
        let command =
            build_direct_command("FOO=bar rustc --version", Path::new(".")).expect("direct");
        let std_command = command.as_std();
        assert_eq!(std_command.get_program().to_string_lossy(), "rustc");
        let args = std_command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(args, vec!["--version"]);
        let envs = std_command.get_envs().collect::<Vec<_>>();
        assert!(envs.iter().any(|(key, value)| {
            key.to_string_lossy() == "FOO"
                && value
                    .as_ref()
                    .map(|item| item.to_string_lossy().to_string())
                    == Some("bar".to_string())
        }));
    }

    #[test]
    fn build_direct_command_rejects_shell_builtins_and_meta_commands() {
        assert!(build_direct_command("cd /tmp", Path::new(".")).is_none());
        assert!(build_direct_command("echo hello && pwd", Path::new(".")).is_none());
    }

    #[test]
    fn build_direct_command_with_overrides_uses_pip_binary_override() {
        let overrides = CommandProgramOverrides {
            pip_bin: Some(PathBuf::from("/tmp/custom-pip")),
            ..Default::default()
        };
        let command = build_direct_command_with_overrides(
            "pip install wunder",
            Path::new("."),
            Some(Path::new("/tmp/python")),
            overrides,
        )
        .expect("direct command");
        let std_command = command.as_std();
        assert_eq!(
            std_command.get_program().to_string_lossy(),
            "/tmp/custom-pip"
        );
        let args = std_command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(args, vec!["install", "wunder"]);
    }

    #[test]
    fn build_direct_command_with_overrides_falls_back_to_python_module_for_pip() {
        let python_bin = Path::new("/tmp/wunder-python/bin/python3");
        let command = build_direct_command_with_overrides(
            "pip install wunder",
            Path::new("."),
            Some(python_bin),
            CommandProgramOverrides::default(),
        )
        .expect("direct command");
        let std_command = command.as_std();
        assert_eq!(
            std_command.get_program().to_string_lossy(),
            python_bin.to_string_lossy()
        );
        let args = std_command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(args, vec!["-m", "pip", "install", "wunder"]);
    }
}
