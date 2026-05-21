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
    #[cfg(windows)]
    {
        if prefer_powershell() {
            let mut cmd = Command::new("powershell.exe");
            cmd.arg("-NoLogo").arg("-NoProfile").arg("-Command");
            // Force UTF-8 output for better cross-terminal decoding.
            let wrapped = format!(
                "$Utf8 = [System.Text.UTF8Encoding]::new($false); [Console]::InputEncoding = $Utf8; [Console]::OutputEncoding = $Utf8; $OutputEncoding = $Utf8; {command}"
            );
            cmd.arg(wrapped).current_dir(cwd);
            apply_platform_spawn_options(&mut cmd);
            cmd
        } else {
            let mut cmd = Command::new("cmd.exe");
            cmd.arg("/C").arg(command).current_dir(cwd);
            apply_platform_spawn_options(&mut cmd);
            cmd
        }
    }

    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("bash");
        cmd.arg("-lc").arg(command).current_dir(cwd);
        cmd
    }
}

pub fn resolve_shell_name(command: &str) -> &'static str {
    #[cfg(windows)]
    {
        let _ = command;
        if prefer_powershell() {
            "powershell.exe"
        } else {
            "cmd.exe"
        }
    }

    #[cfg(not(windows))]
    {
        let _ = command;
        "bash"
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
}
