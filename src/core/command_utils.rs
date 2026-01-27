use std::io;
use std::path::Path;
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
    Some(cmd)
}

pub fn build_shell_command(command: &str, cwd: &Path) -> Command {
    let mut cmd = Command::new("bash");
    cmd.arg("-lc").arg(command).current_dir(cwd);
    cmd
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
    let mut split = part.splitn(2, '=');
    let key = split.next()?;
    let value = split.next()?;
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
