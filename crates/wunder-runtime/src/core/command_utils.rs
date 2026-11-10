#[cfg(not(windows))]
use std::env;
use std::io;
use std::path::Path;
#[cfg(not(windows))]
use std::path::PathBuf;
use std::process::Command as StdCommand;
#[cfg(windows)]
use std::process::Stdio;
#[cfg(windows)]
use std::sync::{Mutex, RwLock};
#[cfg(windows)]
use std::{env, path::PathBuf};
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
    {
        use std::os::unix::process::CommandExt;
        // 独立进程组让「终止」可以按树 kill(-pgid)，孙进程不再脱管。
        cmd.as_std_mut().process_group(0);
    }
}

pub fn apply_platform_spawn_options_std(cmd: &mut StdCommand) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(WINDOWS_CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
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
    /// Android's system POSIX shell (`/system/bin/sh`, mksh + toybox).
    Sh,
}

/// Resolved shell launcher: which flavour, the POSIX shell executable path and
/// the Windows PATH entries that must be present so the POSIX toolchain resolves.
#[derive(Clone, Debug)]
pub struct ShellLaunch {
    pub kind: ShellKind,
    /// POSIX shell binary. Android uses the system `/system/bin/sh`; desktop
    /// Unix resolves bash from PATH or the supplement.
    pub posix_bin: Option<PathBuf>,
    pub path_dirs: Vec<PathBuf>,
}

impl Default for ShellLaunch {
    fn default() -> Self {
        Self {
            kind: default_shell_kind(),
            posix_bin: None,
            path_dirs: Vec::new(),
        }
    }
}

fn default_shell_kind() -> ShellKind {
    #[cfg(windows)]
    {
        ShellKind::Cmd
    }
    #[cfg(target_os = "android")]
    {
        ShellKind::Sh
    }
    #[cfg(all(not(windows), not(target_os = "android")))]
    {
        ShellKind::Bash
    }
}

/// Platform default shell (PowerShell preferred on Windows, bash on desktop
/// Unix, Android's `/system/bin/sh` on the phone).
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
            posix_bin: None,
            path_dirs: Vec::new(),
        }
    }
    #[cfg(target_os = "android")]
    {
        ShellLaunch {
            kind: ShellKind::Sh,
            posix_bin: Some(PathBuf::from("/system/bin/sh")),
            path_dirs: Vec::new(),
        }
    }
    #[cfg(all(not(windows), not(target_os = "android")))]
    {
        ShellLaunch {
            kind: ShellKind::Bash,
            posix_bin: Some(PathBuf::from("bash")),
            path_dirs: Vec::new(),
        }
    }
}

/// Human-readable shell name recorded in command session metadata.
pub fn shell_display_name(shell: &ShellLaunch) -> String {
    match shell.kind {
        ShellKind::Cmd => "cmd.exe".to_string(),
        ShellKind::PowerShell => powershell_display_name(),
        ShellKind::Bash => "bash".to_string(),
        ShellKind::Sh => "sh".to_string(),
    }
}

/// Prefix POSIX commands with `set -o pipefail` so a failing middle stage of a
/// pipeline propagates its exit code instead of being masked by the last
/// command's success. Commands that already manage pipefail are kept as-is.
/// Android's mksh supports this option.
fn posix_command_with_pipefail(command: &str) -> String {
    if command.contains("pipefail") {
        command.to_string()
    } else {
        format!("set -o pipefail; {command}")
    }
}

/// Build a shell interpreter command for the resolved shell.
pub fn build_shell_command_with_shell(command: &str, cwd: &Path, shell: &ShellLaunch) -> Command {
    #[cfg(windows)]
    {
        match shell.kind {
            ShellKind::Bash => {
                let posix_bin = shell
                    .posix_bin
                    .as_ref()
                    .expect("bash shell requires posix_bin");
                let mut cmd = Command::new(posix_bin);
                // Non-login shell; PATH carrying the POSIX toolchain is applied
                // by the caller so it works even on stripped MinGit layouts.
                cmd.arg("-c")
                    .arg(posix_command_with_pipefail(command))
                    .current_dir(cwd);
                apply_platform_spawn_options(&mut cmd);
                cmd
            }
            ShellKind::PowerShell => build_powershell_command(command, cwd),
            ShellKind::Cmd => build_cmd_command(command, cwd),
            ShellKind::Sh => {
                // Android never reaches the Windows branch; keep the match
                // exhaustive for cross-target builds.
                unreachable!("Sh shell is not used on Windows")
            }
        }
    }

    #[cfg(not(windows))]
    {
        let posix_bin = env::var_os(SHELL_BIN_ENV)
            .map(PathBuf::from)
            .or_else(|| shell.posix_bin.clone())
            .unwrap_or_else(default_posix_shell);
        let mut cmd = Command::new(posix_bin);
        // A wrapper script cannot be executed directly inside an Android app
        // sandbox (SELinux denies execve on app-private files), but the system
        // shell may still read it. This form runs `<shell> <script> -lc <cmd>`
        // and leaves the script in charge of entering its own runtime.
        if let Some(script) = env::var_os(SHELL_SCRIPT_ENV) {
            cmd.arg(PathBuf::from(script));
        }
        // `-l` keeps the desktop Unix behaviour where a login shell publishes
        // the PATH assembled by the host profile. Android's mksh accepts the
        // same flags and has no profile to run, so one branch covers both.
        cmd.arg("-lc")
            .arg(posix_command_with_pipefail(command))
            .current_dir(cwd);
        apply_platform_spawn_options(&mut cmd);
        cmd
    }
}

/// Fallback shell name when no explicit binary was resolved. Android puts the
/// system shell on PATH; desktop Unix expects bash.
#[cfg(not(windows))]
fn default_posix_shell() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        PathBuf::from("sh")
    }
    #[cfg(all(not(windows), not(target_os = "android")))]
    {
        PathBuf::from("bash")
    }
}

/// Prelude run ahead of every Windows PowerShell command.
///
/// Three host realities force this shape:
/// * Windows 7 ships Windows PowerShell 2.0, whose parser rejects `::new()`
///   (a 5.0 construct), so the old one-liner printed a red parse error on every
///   call before the user's own command even started.
/// * Children are spawned with `CREATE_NO_WINDOW`, so there is no console to
///   retarget; the encoding setters can throw. Everything therefore stays
///   inside one `try` — a failed prelude must never fail the command.
/// * With stderr redirected, PowerShell serialises progress records to stderr
///   as a `#< CLIXML` blob (module warm-up alone is enough). Suppressing the
///   progress stream keeps captured stderr readable for both the user and the
///   model.
#[cfg(windows)]
const POWERSHELL_PRELUDE: &str = "$ProgressPreference = 'SilentlyContinue'; try { $u = New-Object System.Text.UTF8Encoding $false; [Console]::OutputEncoding = $u; $OutputEncoding = $u } catch { }";

/// Script handed to the interpreter: prelude on its own line, then the command,
/// so a prelude failure can never swallow or corrupt the caller's text.
#[cfg(windows)]
fn powershell_script(command: &str) -> String {
    format!("{POWERSHELL_PRELUDE}\n{command}")
}

/// `-EncodedCommand` payload: base64 of the UTF-16LE script.
///
/// Encoding the script instead of passing it inline removes three classes of
/// breakage: quoting layering (`cmd` → interpreter), `$`/`%`/`&` being re-parsed
/// on the way in, and non-ASCII text being flattened through the ANSI code page.
#[cfg(windows)]
fn encode_powershell_script(script: &str) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    let mut bytes = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    STANDARD.encode(bytes)
}

/// Roughly the point where base64 (×2.67) plus the rest of the command line
/// would run into the 32767-character Windows limit.
#[cfg(windows)]
const MAX_ENCODED_SCRIPT: usize = 30_000;

#[cfg(windows)]
fn build_powershell_command(command: &str, cwd: &Path) -> Command {
    let script = powershell_script(command);
    let encoded = encode_powershell_script(script.as_str());
    // Launch the exact binary the host probe accepted. Resolving a bare
    // `powershell.exe` through PATH would silently pick the system 2.0 even
    // when the supplement ships a portable 7.x next to it.
    let mut cmd = Command::new(powershell_program());
    cmd.arg("-NoLogo").arg("-NoProfile").arg("-NonInteractive");
    if encoded.len() <= MAX_ENCODED_SCRIPT {
        cmd.arg("-EncodedCommand").arg(encoded);
    } else {
        // Oversized script: base64 would no longer fit on the command line, so
        // hand the text over directly. Long scripts keep the older quoting
        // rules; short ones — the common case — get the clean path.
        cmd.arg("-Command").arg(script);
    }
    cmd.current_dir(cwd);
    apply_platform_spawn_options(&mut cmd);
    cmd
}

#[cfg(windows)]
fn powershell_program() -> std::ffi::OsString {
    powershell_host()
        .map(|host| host.exe.clone().into_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("powershell.exe"))
}

/// `cmd.exe` path for hosts without PowerShell (stripped images, policy blocks,
/// or an explicit cmd preference in settings).
///
/// `/D` skips `CommandLine\AutoRun` registry hooks, which would otherwise run
/// startup commands (doskey macros, vendor shims) inside every call.
#[cfg(windows)]
fn build_cmd_command(command: &str, cwd: &Path) -> Command {
    let mut cmd = Command::new("cmd.exe");
    cmd.arg("/D").arg("/C").arg(command).current_dir(cwd);
    apply_platform_spawn_options(&mut cmd);
    cmd
}

/// Tokenise a command line the way the host shell would.
///
/// POSIX `shell_words::split` treats `\` as an escape character, so on Windows
/// `python C:\tools\export.py` becomes `python C:toolsexport.py`. The direct
/// launch path (used to rewrite a bare `python` into the bundled interpreter)
/// must use Windows `CommandLineToArgvW` semantics instead, where a backslash
/// only escapes a quote and is otherwise an ordinary path character.
fn split_command_words(command: &str) -> Option<Vec<String>> {
    #[cfg(windows)]
    {
        split_windows_command_words(command)
    }
    #[cfg(not(windows))]
    {
        shell_words::split(command).ok()
    }
}

/// Windows `CommandLineToArgvW` tokenisation, kept platform-independent so the
/// path-preservation regression can be tested on every build host.
pub(crate) fn split_windows_command_words(command: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut in_quotes = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let mut count = 1usize;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    count += 1;
                }
                if chars.peek() == Some(&'"') {
                    for _ in 0..(count / 2) {
                        current.push('\\');
                    }
                    if count % 2 == 1 {
                        current.push('"');
                        chars.next();
                    }
                    // With an even count the quote stays as a delimiter and is
                    // handled on the next iteration.
                } else {
                    for _ in 0..count {
                        current.push('\\');
                    }
                }
                started = true;
            }
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
                started = true;
            }
            ch if ch.is_whitespace() && !in_quotes => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            ch => {
                current.push(ch);
                started = true;
            }
        }
    }
    if in_quotes {
        return None;
    }
    if started {
        args.push(current);
    }
    Some(args)
}

/// Defence in depth: never spawn a direct command whose Windows path
/// separators vanished during tokenisation. Falling back to the shell keeps
/// the caller's original text intact and produces a meaningful path error
/// instead of a confusing `C:Users...` file-not-found.
fn direct_args_preserve_paths(command: &str, parts: &[String]) -> bool {
    if !command.contains('\\') {
        return true;
    }
    let parsed_backslashes = parts
        .iter()
        .map(|part| part.matches('\\').count())
        .sum::<usize>();
    if parsed_backslashes == 0 {
        return false;
    }
    !parts.iter().any(|part| {
        let bytes = part.as_bytes();
        bytes.len() > 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] != b'\\'
            && bytes[2] != b'/'
    })
}

pub fn build_direct_command(command: &str, cwd: &Path) -> Option<Command> {
    let trimmed = command.trim();
    if trimmed.is_empty() || contains_shell_meta(trimmed) {
        return None;
    }
    let parts = split_command_words(trimmed)?;
    if !direct_args_preserve_paths(trimmed, &parts) {
        return None;
    }
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
    let parts = split_command_words(trimmed)?;
    if parts.is_empty() || !direct_args_preserve_paths(trimmed, &parts) {
        return None;
    }
    let (envs, program_index) = parse_env_prefix(&parts);
    let program = parts.get(program_index)?;
    if is_shell_builtin(program) {
        return None;
    }
    let pip_invocation = command_is_pip_invocation(program);
    // Only the program token is rewritten. Every token after `program_index`
    // is passed through verbatim, so Windows paths in the arguments keep
    // their backslashes.
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
        // The concrete basename (`pwsh.exe` for the bundled portable build) is
        // allocated per process, so this static accessor cannot report it.
        // Callers that need the real name use `shell_display_name`.
        ShellKind::PowerShell => "powershell.exe",
        ShellKind::Bash => "bash",
        ShellKind::Sh => "sh",
    }
}

#[cfg(windows)]
fn prefer_powershell() -> bool {
    // Do NOT cache this boolean. `powershell_host()` already caches the probe,
    // and a supplement import can publish `pwsh.exe` after boot and call
    // `reset_powershell_host()`. A separate `OnceLock<bool>` kept the boot-time
    // answer forever, so an imported PowerShell stayed invisible and every
    // command kept running through cmd.exe until the app restarted.
    powershell_host().is_some()
}

/// A `pwsh.exe` that exists on disk but was not chosen as the shell.
///
/// When probing fails (missing VC runtime, policy block, a hung startup) the
/// command tool falls back to cmd.exe, and the model has no way to learn that a
/// portable PowerShell is sitting right next to it. Reporting the path is only
/// for that explanation — it never changes which shell runs.
#[cfg(windows)]
pub fn unused_pwsh() -> Option<PathBuf> {
    if powershell_host().is_some() {
        return None;
    }
    pwsh_on_disk()
}

/// 磁盘上是否存在某个 pwsh.exe（不启动它）。
///
/// 与 `powershell_host()` 分开，避免在探测过程中递归调用自己。
#[cfg(windows)]
fn pwsh_on_disk() -> Option<PathBuf> {
    let published = env::var_os(POWERSHELL_EXE_ENV).map(PathBuf::from);
    if let Some(path) = published.filter(|path| path.is_file()) {
        return Some(path);
    }
    find_in_path("pwsh.exe")
}

#[cfg(not(windows))]
pub fn unused_pwsh() -> Option<std::path::PathBuf> {
    None
}

/// Environment variable published by the supplement bootstrap: the absolute
/// path of the bundled portable PowerShell.
///
/// The supplement ships `pwsh.exe`, so prepending its directory to PATH can
/// never satisfy a `powershell.exe` lookup. Publishing the resolved path keeps
/// shell selection explicit instead of relying on PATH order.
pub const POWERSHELL_EXE_ENV: &str = "WUNDER_POWERSHELL_EXE";

/// Unix counterpart of the shell override: absolute path of the POSIX shell
/// binary. The desktop settings field is auto-filled with the system bash;
/// Android publishes `/system/bin/sh` at startup.
pub const SHELL_BIN_ENV: &str = "WUNDER_SHELL_BIN";

/// Optional shell script executed through `SHELL_BIN_ENV`. Used on Android,
/// where the app sandbox forbids execve on private files: the system shell
/// reads the wrapper and the wrapper enters the bundled runtime.
pub const SHELL_SCRIPT_ENV: &str = "WUNDER_SHELL_SCRIPT";

/// A PowerShell host that answered its own version query.
///
/// Resolving the binary and detecting the version are deliberately one step:
/// a file that exists but cannot start (a portable PowerShell 7 on a Windows 7
/// image without the Universal CRT, an AppLocker-blocked binary) must fall
/// through to the next candidate instead of failing every command.
#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowerShellHost {
    pub exe: PathBuf,
    pub major: u32,
    /// True when the executable comes from the supplement `opt/powershell`
    /// tree, i.e. the portable PowerShell 7 rather than an OS install.
    pub bundled: bool,
}

/// Probe state of the PowerShell host.
///
/// The probe result has to be invalidated when the supplement is imported at
/// runtime: the portable `pwsh.exe` only exists after the import, so a cache
/// filled at boot would keep the command tool on the system's PowerShell 2.0
/// until the next restart.
#[cfg(windows)]
enum ShellProbe {
    Pending,
    Done(Option<PowerShellHost>),
    /// 探测失败，但磁盘上还有可试的 pwsh：冷启动可能被杀软拖过预算，隔一会儿
    /// 再探一次，别让这台机器永久停在 cmd.exe 上。
    Failed { at: std::time::Instant },
}

/// 一次失败之后多久允许重探。
#[cfg(windows)]
const PROBE_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

#[cfg(windows)]
static POWERSHELL_HOST: RwLock<ShellProbe> = RwLock::new(ShellProbe::Pending);

/// The PowerShell host the command tool will use.
///
/// Resolving the binary and detecting the version are deliberately one step:
/// a file that exists but cannot start (a portable PowerShell 7 on a Windows 7
/// image without the Universal CRT, an AppLocker-blocked binary) must fall
/// through to the next candidate instead of failing every command.
#[cfg(windows)]
pub fn powershell_host() -> Option<PowerShellHost> {
    if let Some(host) = cached_powershell_host() {
        return host;
    }
    // Resolve once per process, not once per racing caller. The probe blocks
    // the calling thread for up to a few seconds per candidate; without this
    // gate every concurrent first command would each spawn the same probes
    // and pin another worker thread. The cache is re-checked under the lock
    // because the winner may have finished while we waited.
    static PROBE_LOCK: Mutex<()> = Mutex::new(());
    let _serial = PROBE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(host) = cached_powershell_host() {
        return host;
    }
    let probed = probe_powershell_host();
    let Ok(mut guard) = POWERSHELL_HOST.write() else {
        return probed;
    };
    // Another thread won the race; keep its answer.
    if let ShellProbe::Done(host) = &*guard {
        return host.clone();
    }
    *guard = match &probed {
        Some(host) => ShellProbe::Done(Some(host.clone())),
        // 还有 pwsh 可以再试，就别把结论钉死。
        None if pwsh_on_disk().is_some() => ShellProbe::Failed {
            at: std::time::Instant::now(),
        },
        None => ShellProbe::Done(None),
    };
    probed
}

/// `Some(host)` when the probe has completed (including "no usable host"),
/// `None` while it is still pending.
#[cfg(windows)]
fn cached_powershell_host() -> Option<Option<PowerShellHost>> {
    let guard = POWERSHELL_HOST.read().ok()?;
    match &*guard {
        ShellProbe::Done(host) => Some(host.clone()),
        ShellProbe::Failed { at } if at.elapsed() < PROBE_RETRY_AFTER => Some(None),
        // 过期的失败当作「还没探过」，让调用方再试一次。
        ShellProbe::Failed { .. } | ShellProbe::Pending => None,
    }
}

/// Forget the cached probe so the next call re-resolves the shell.
///
/// Called after a supplement import publishes a new `WUNDER_POWERSHELL_EXE`,
/// so the newly unpacked portable PowerShell takes effect without a restart.
#[cfg(windows)]
pub fn reset_powershell_host() {
    if let Ok(mut guard) = POWERSHELL_HOST.write() {
        *guard = ShellProbe::Pending;
    }
}

#[cfg(not(windows))]
pub fn reset_powershell_host() {}

#[cfg(windows)]
fn probe_powershell_host() -> Option<PowerShellHost> {
    // Total budget, not per candidate: a Windows image can expose several
    // candidates (override, System32, PATH, pwsh) and a broken .NET stack can
    // make every one of them hang at startup. Eight seconds bounds the first
    // command's stall; three seconds per candidate keeps a single black hole
    // from consuming the whole budget.
    const TOTAL_BUDGET: std::time::Duration = std::time::Duration::from_secs(8);
    const PER_CANDIDATE: std::time::Duration = std::time::Duration::from_secs(3);
    let deadline = std::time::Instant::now() + TOTAL_BUDGET;
    for candidate in powershell_candidates() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            tracing::warn!("powershell host probe budget exhausted before a host answered");
            break;
        }
        let budget = remaining.min(PER_CANDIDATE);
        if let Some(major) = probe_powershell_major(&candidate.exe, budget) {
            return Some(PowerShellHost {
                exe: candidate.exe,
                major,
                bundled: candidate.bundled,
            });
        }
    }
    // Falling back to cmd.exe is the one outcome users notice ("why is it still
    // cmd when pwsh is installed"), so leave a line naming the published path:
    // a bundled pwsh that will not start is a packaging problem, not a missing
    // feature.
    tracing::warn!(
        published_pwsh = ?env::var_os(POWERSHELL_EXE_ENV),
        "no PowerShell host answered; commands will run through cmd.exe"
    );
    None
}

#[cfg(windows)]
struct PowerShellCandidate {
    exe: PathBuf,
    bundled: bool,
}

/// Candidate order: explicit override, then the OS install, then whatever PATH
/// exposes. `pwsh.exe` is only considered when no Windows PowerShell exists at
/// all — a separately installed PowerShell 7 must not hijack a healthy 5.1
/// host, because most automation modules only load in Windows PowerShell.
#[cfg(windows)]
fn powershell_candidates() -> Vec<PowerShellCandidate> {
    powershell_candidates_with(env::var_os(POWERSHELL_EXE_ENV).map(PathBuf::from))
}

#[cfg(windows)]
fn powershell_candidates_with(override_path: Option<PathBuf>) -> Vec<PowerShellCandidate> {
    let mut candidates = Vec::new();
    push_powershell_candidate(&mut candidates, override_path, true);
    push_powershell_candidate(
        &mut candidates,
        env::var_os("SystemRoot").map(|root| {
            PathBuf::from(root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe")
        }),
        false,
    );
    push_powershell_candidate(&mut candidates, find_in_path("powershell.exe"), false);
    if candidates.is_empty() {
        push_powershell_candidate(&mut candidates, find_in_path("pwsh.exe"), false);
    }
    candidates
}

#[cfg(windows)]
fn push_powershell_candidate(
    candidates: &mut Vec<PowerShellCandidate>,
    path: Option<PathBuf>,
    bundled: bool,
) {
    let Some(path) = path else { return };
    if !path.is_file() {
        return;
    }
    let duplicate = candidates
        .iter()
        .any(|known| same_path(&known.exe, &path));
    if !duplicate {
        candidates.push(PowerShellCandidate {
            exe: path,
            bundled,
        });
    }
}

#[cfg(windows)]
fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(windows)]
fn find_in_path(binary: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .map(|dir| dir.join(binary))
        .find(|path| path.is_file())
}

/// Ask a candidate for its own major version. `None` means "this binary cannot
/// run here", not just "unknown version": the caller moves on to the next one.
///
/// The probe runs once on a startup path, directly on the calling thread, so it
/// must carry its own deadline: a PowerShell that hangs at startup (antivirus
/// scan, broken .NET) would otherwise stall the first command indefinitely.
/// The version answer is a few bytes and can never fill the pipe buffer, so
/// poll-then-read cannot deadlock.
#[cfg(windows)]
fn probe_powershell_major(exe: &Path, timeout: std::time::Duration) -> Option<u32> {
    let mut command = StdCommand::new(exe);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$PSVersionTable.PSVersion.Major",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    apply_platform_spawn_options_std(&mut command);
    let mut child = command.spawn().ok()?;
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    };
    let mut stdout = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        use std::io::Read as _;
        let _ = pipe.read_to_end(&mut stdout);
    }
    if !status.success() {
        return None;
    }
    String::from_utf8_lossy(&stdout)
        .trim()
        .parse::<u32>()
        .ok()
}

/// Detected Windows PowerShell major version: 2 on a stock Windows 7, 5 on
/// Windows 10/11, 7 for the bundled portable PowerShell, `None` when the host
/// has no usable PowerShell at all.
///
/// Probed once per process and cached. It only shapes the hint handed to the
/// model, never the launcher — the prelude works on every version — but the
/// program name comes from the same probe so a hint can never describe a
/// different shell than the one that runs.
#[cfg(windows)]
pub fn powershell_major_version() -> Option<u32> {
    powershell_host().map(|host| host.major)
}

#[cfg(not(windows))]
pub fn powershell_major_version() -> Option<u32> {
    None
}

/// Basename of the executable that will run PowerShell commands, for session
/// metadata and the tool description (`powershell.exe` vs `pwsh.exe`).
#[cfg(windows)]
pub fn powershell_display_name() -> String {
    powershell_host()
        .and_then(|host| {
            host.exe
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "powershell.exe".to_string())
}

#[cfg(not(windows))]
pub fn powershell_display_name() -> String {
    "powershell.exe".to_string()
}

/// Absolute path of the PowerShell executable the command tool would use.
///
/// Callers that spawn PowerShell themselves (the archive extractor, for
/// instance) must launch *this* binary rather than a bare `powershell.exe`
/// from PATH: the version gate and the launched program have to describe the
/// same interpreter, otherwise a host can decide "PowerShell 5 is available"
/// and then run the system 2.0 that has no `Expand-Archive`.
#[cfg(windows)]
pub fn powershell_executable() -> Option<String> {
    powershell_host().map(|host| host.exe.to_string_lossy().into_owned())
}

#[cfg(not(windows))]
pub fn powershell_executable() -> Option<String> {
    None
}

/// True when the host only has a pre-5.0 Windows PowerShell.
///
/// 2.0 through 4.0 share one trap: no `::new()`, no `Get-FileHash`, no
/// `Get-Content -Raw`, no `-AsHashtable`, no `??` or ternary. Callers use this
/// to hand the model a stricter syntax hint up front instead of letting it
/// discover the limit through red errors.
#[cfg(windows)]
pub fn powershell_is_legacy() -> bool {
    matches!(powershell_major_version(), Some(version) if version < 5)
}

#[cfg(not(windows))]
pub fn powershell_is_legacy() -> bool {
    false
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

    #[test]
    fn windows_tokenizer_preserves_backslash_paths() {
        let parts = split_windows_command_words(
            r"python C:\tools\scripts\export_png.py --out D:\proj\out.png",
        )
        .expect("tokenise command");
        assert_eq!(
            parts,
            vec![
                "python",
                r"C:\tools\scripts\export_png.py",
                "--out",
                r"D:\proj\out.png"
            ]
        );
    }

    #[test]
    fn windows_tokenizer_handles_quoted_paths_and_escaped_quotes() {
        let parts = split_windows_command_words(r#"python "C:\Program Files\tool.py" -c "a b""#)
            .expect("tokenise command");
        assert_eq!(
            parts,
            vec!["python", r"C:\Program Files\tool.py", "-c", "a b"]
        );
        let parts = split_windows_command_words(r#"python -c \"quoted\""#)
            .expect("tokenise command");
        assert_eq!(parts, vec!["python", "-c", "\"quoted\""]);
        assert!(split_windows_command_words(r#"python "unterminated"#).is_none());
    }

    #[test]
    fn direct_launch_rejects_stripped_windows_paths() {
        assert!(!direct_args_preserve_paths(
            r"python C:\tools\a.py",
            &["python".to_string(), "C:toolsa.py".to_string()]
        ));
        assert!(direct_args_preserve_paths(
            r"python C:\tools\a.py",
            &["python".to_string(), r"C:\tools\a.py".to_string()]
        ));
    }

    #[cfg(windows)]
    #[test]
    fn python_override_keeps_windows_path_arguments() {
        let cmd = build_direct_command_with_python_override(
            r"python C:\tools\export_png.py --out D:\proj\out.png",
            Path::new("."),
            Path::new(r"C:\runtime\python.exe"),
        )
        .expect("direct command with interpreter override");
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert!(args.iter().any(|arg| arg == r"C:\tools\export_png.py"));
        assert!(args.iter().any(|arg| arg == r"D:\proj\out.png"));
        assert!(!args.iter().any(|arg| arg.starts_with("C:tools")));
    }

    /// The launcher and the version probe must agree on one binary. Asserting
    /// against `powershell_host()` instead of a literal `powershell.exe` keeps
    /// the bundled `pwsh.exe` a first-class outcome on a Win7 host.
    #[cfg(windows)]
    fn assert_launcher_matches_host(command: &Command) {
        let program = command
            .as_std()
            .get_program()
            .to_string_lossy()
            .to_ascii_lowercase();
        match powershell_host() {
            Some(host) => assert_eq!(
                Path::new(&program),
                Path::new(&host.exe.to_string_lossy().to_ascii_lowercase()),
                "shell launcher diverged from the probed PowerShell host"
            ),
            None => assert!(program.ends_with("cmd.exe")),
        }
    }

    #[cfg(windows)]
    #[test]
    fn build_shell_command_prefers_windows_shell() {
        let command = build_shell_command("pwd", Path::new("."));
        assert_launcher_matches_host(&command);
    }

    #[cfg(windows)]
    #[test]
    fn build_shell_command_keeps_powershell_for_chained_commands_when_available() {
        let command = build_shell_command("cargo --version && rustc --version", Path::new("."));
        assert_launcher_matches_host(&command);
    }

    #[cfg(windows)]
    #[test]
    fn build_shell_command_keeps_powershell_for_stderr_merge_when_available() {
        let command = build_shell_command("cargo test 2>&1", Path::new("."));
        assert_launcher_matches_host(&command);
    }

    /// The supplement publishes an absolute `pwsh.exe` path; it must be the
    /// first candidate so a portable PowerShell 7 wins over the system 2.0.
    #[cfg(windows)]
    #[test]
    fn supplement_powershell_override_is_the_first_candidate() {
        let temp = tempfile::tempdir().unwrap();
        let pwsh = temp.path().join("opt/powershell/pwsh.exe");
        std::fs::create_dir_all(pwsh.parent().unwrap()).unwrap();
        std::fs::write(&pwsh, b"fixture").unwrap();

        let candidates = powershell_candidates_with(Some(pwsh.clone()));
        assert_eq!(candidates.first().map(|item| item.exe.clone()), Some(pwsh));
        assert!(candidates[0].bundled);

        // A stale override must not shadow a working OS install.
        let missing = temp.path().join("opt/powershell/gone.exe");
        let candidates = powershell_candidates_with(Some(missing.clone()));
        assert!(candidates.iter().all(|item| item.exe != missing));
    }

    /// A binary that exists but cannot run has to fall through, not fail the
    /// command: this is exactly the Win7 + portable-7-without-UCRT case.
    #[cfg(windows)]
    #[test]
    fn unstartable_powershell_candidates_are_skipped() {
        let temp = tempfile::tempdir().unwrap();
        let broken = temp.path().join("pwsh.exe");
        std::fs::write(&broken, b"not a pe image").unwrap();
        assert_eq!(
            probe_powershell_major(&broken, std::time::Duration::from_secs(2)),
            None
        );
    }

    /// The host probe and the launcher program name must describe the same
    /// binary; otherwise the model gets a syntax hint for a shell it never runs.
    #[cfg(windows)]
    #[test]
    fn powershell_display_name_matches_the_probed_host() {
        match powershell_host() {
            Some(host) => {
                let expected = host.exe.file_name().unwrap().to_string_lossy().to_string();
                assert_eq!(powershell_display_name(), expected);
                assert_eq!(powershell_major_version(), Some(host.major));
            }
            None => assert_eq!(powershell_display_name(), "powershell.exe"),
        }
    }

    /// Importing the supplement publishes a new `WUNDER_POWERSHELL_EXE` while
    /// the process is already running. The cached probe has to be droppable and
    /// rebuildable: after a reset the host must be re-probed, not left empty,
    /// and the launcher must still agree with it.
    #[cfg(windows)]
    #[test]
    fn resetting_the_host_probe_rebuilds_a_consistent_cache() {
        let before = powershell_host();
        reset_powershell_host();
        let after = powershell_host();

        // Re-probing the same machine yields the same answer; a reset that
        // left the cache empty would answer `None` where it used to answer
        // `Some`, and a reset that kept the cache would not exercise the path.
        assert_eq!(
            before.as_ref().map(|host| host.exe.clone()),
            after.as_ref().map(|host| host.exe.clone()),
            "the probe cache was not rebuilt consistently after a reset"
        );
        match after {
            Some(host) => {
                assert_eq!(powershell_major_version(), Some(host.major));
                assert_eq!(
                    powershell_display_name(),
                    host.exe.file_name().unwrap().to_string_lossy().to_string()
                );
            }
            None => assert_eq!(powershell_display_name(), "powershell.exe"),
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

    #[test]
    fn posix_command_with_pipefail_injects_once() {
        assert_eq!(
            posix_command_with_pipefail("echo ok | head"),
            "set -o pipefail; echo ok | head"
        );
        assert_eq!(
            posix_command_with_pipefail("set -o pipefail; echo ok | head"),
            "set -o pipefail; echo ok | head"
        );
    }

    fn command_args(command: &Command) -> Vec<String> {
        command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect()
    }

    #[cfg(windows)]
    #[test]
    fn build_shell_command_with_bash_injects_pipefail() {
        let shell = ShellLaunch {
            kind: ShellKind::Bash,
            posix_bin: Some(PathBuf::from("bash")),
            path_dirs: Vec::new(),
        };
        let command = build_shell_command_with_shell("echo ok | head", Path::new("."), &shell);
        assert_eq!(
            command_args(&command),
            vec!["-c", "set -o pipefail; echo ok | head"]
        );

        let prefixed = build_shell_command_with_shell(
            "set -o pipefail; echo ok | head",
            Path::new("."),
            &shell,
        );
        assert_eq!(
            command_args(&prefixed),
            vec!["-c", "set -o pipefail; echo ok | head"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn build_shell_command_keeps_powershell_and_cmd_text_untouched() {
        // PowerShell hands the script over as a base64 payload, so the text is
        // not literally among the args any more — decode it back and compare.
        for kind in [ShellKind::PowerShell, ShellKind::Cmd] {
            let shell = ShellLaunch {
                kind,
                posix_bin: None,
                path_dirs: Vec::new(),
            };
            let command = build_shell_command_with_shell("echo ok | head", Path::new("."), &shell);
            let args = command_args(&command);
            assert!(
                args.iter().all(|arg| !arg.contains("pipefail")),
                "{kind:?} args should not be rewritten: {args:?}"
            );
            match kind {
                ShellKind::PowerShell => {
                    assert!(args.iter().any(|arg| arg == "-EncodedCommand"));
                    let decoded = decode_powershell_args(&args);
                    assert!(decoded.contains("echo ok | head"), "{decoded}");
                }
                _ => assert!(args.iter().any(|arg| arg.contains("echo ok | head"))),
            }
        }
    }

    /// Base64 (UTF-16LE) payload PowerShell would run, decoded for assertions.
    #[cfg(windows)]
    fn decode_powershell_args(args: &[String]) -> String {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;

        let payload = args
            .iter()
            .position(|arg| arg == "-EncodedCommand")
            .and_then(|index| args.get(index + 1))
            .expect("encoded command payload");
        let bytes = STANDARD.decode(payload).expect("base64 payload");
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units).expect("utf16 script")
    }

    /// Windows PowerShell 2.0 (stock Windows 7) rejects `::new()` outright, so
    /// the prelude must never reintroduce it — that construct was printing a
    /// red parse error before every single command.
    #[cfg(windows)]
    #[test]
    fn powershell_prelude_stays_on_2_0_syntax() {
        let script = powershell_script("Get-Location");
        assert!(!script.contains("::new("), "{script}");
        assert!(script.contains("New-Object System.Text.UTF8Encoding"), "{script}");
        // The command keeps its own line so a prelude failure cannot swallow it.
        assert!(script.ends_with("\nGet-Location"), "{script}");
    }

    /// Chinese text and quotes have to survive the trip: base64 of UTF-16LE is
    /// the only form that does not depend on the console code page.
    #[cfg(windows)]
    #[test]
    fn encoded_powershell_round_trips_unicode_and_quotes() {
        let shell = ShellLaunch {
            kind: ShellKind::PowerShell,
            posix_bin: None,
            path_dirs: Vec::new(),
        };
        let command = "Get-Content \"C:\\资料\\报告 1.txt\" | Select-String '错误'";
        let built = build_shell_command_with_shell(command, Path::new("."), &shell);
        let decoded = decode_powershell_args(&command_args(&built));
        assert!(decoded.contains(command), "{decoded}");
        assert!(decoded.contains("资料"), "{decoded}");
    }

    /// End-to-end on the host: the encoded payload has to actually run, and a
    /// console-less child must not print prelude noise. This is the assertion
    /// that would have caught the old `::new()` one-liner.
    #[cfg(windows)]
    #[test]
    fn powershell_encoded_command_runs_without_prelude_noise() {
        if powershell_host().is_none() {
            return;
        }
        let shell = ShellLaunch {
            kind: ShellKind::PowerShell,
            posix_bin: None,
            path_dirs: Vec::new(),
        };
        let mut command =
            build_shell_command_with_shell("Write-Output 'wunder-ok'", Path::new("."), &shell);
        let output = command.as_std_mut().output().expect("spawn interpreter");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "stderr={stderr}");
        assert!(stdout.contains("wunder-ok"), "stdout={stdout:?}");
        assert!(stderr.trim().is_empty(), "prelude leaked to stderr: {stderr}");
    }

    /// `/D` keeps `cmd /C` deterministic: it skips AutoRun hooks that would
    /// otherwise inject vendor startup commands into every call.
    #[cfg(windows)]
    #[test]
    fn cmd_launcher_skips_autorun_hooks() {
        let shell = ShellLaunch {
            kind: ShellKind::Cmd,
            posix_bin: None,
            path_dirs: Vec::new(),
        };
        let command = build_shell_command_with_shell("dir", Path::new("."), &shell);
        let args = command_args(&command);
        assert_eq!(args, vec!["/D", "/C", "dir"]);
    }

    /// A script whose base64 payload would blow past the Windows command-line
    /// limit must still run: it degrades to inline `-Command` instead of
    /// failing to spawn.
    #[cfg(windows)]
    #[test]
    fn oversized_powershell_script_falls_back_to_inline_command() {
        let shell = ShellLaunch {
            kind: ShellKind::PowerShell,
            posix_bin: None,
            path_dirs: Vec::new(),
        };
        let command = "Write-Output 1\n".repeat(3_000);
        let built = build_shell_command_with_shell(command.as_str(), Path::new("."), &shell);
        let args = command_args(&built);
        assert!(!args.iter().any(|arg| arg == "-EncodedCommand"), "{args:?}");
        assert!(args.iter().any(|arg| arg == "-Command"));
    }

    #[cfg(not(windows))]
    #[test]
    fn build_shell_command_injects_pipefail_on_unix() {
        let command = build_shell_command("echo ok | head", Path::new("."));
        assert_eq!(
            command_args(&command),
            vec!["-lc", "set -o pipefail; echo ok | head"]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn android_sh_keeps_pipefail_contract() {
        let shell = ShellLaunch {
            kind: ShellKind::Sh,
            posix_bin: Some(PathBuf::from("/system/bin/sh")),
            path_dirs: Vec::new(),
        };
        assert_eq!(shell_display_name(&shell), "sh");
        let command = build_shell_command_with_shell("echo ok | head", Path::new("."), &shell);
        // The concrete binary may be overridden by WUNDER_SHELL_BIN in the test
        // environment; the command contract is what Android relies on.
        assert_eq!(
            command_args(&command),
            vec!["-lc", "set -o pipefail; echo ok | head"]
        );
    }
}
