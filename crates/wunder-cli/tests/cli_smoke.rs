use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn wunder_cli_exe() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_wunder-cli")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut fallback = PathBuf::from("target");
            fallback.push("debug");
            #[cfg(windows)]
            {
                fallback.push("wunder-cli.exe");
            }
            #[cfg(not(windows))]
            {
                fallback.push("wunder-cli");
            }
            fallback
        })
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("wunder-cli crate should live under crates/")
        .to_path_buf()
}

fn unique_temp_root(tag: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "wunder_cli_smoke_{tag}_{}_{}",
        std::process::id(),
        stamp
    ));
    fs::create_dir_all(&dir).expect("create temp root");
    dir
}

/// Every non-interactive entry point must stay runnable without a model and
/// must never overflow the stack: the agent turn future is deep enough that a
/// regression here is silent until runtime.
///
/// 不要给 CLI 传 `--config <repo>/config/wunder.yaml`：CLI 会把生效后的配置**回写**到
/// 它读到的那份文件，于是跑一次测试就把开发者本机的 `config/wunder.yaml` 改写成
/// `--temp-root` 里的临时库/工作区路径（`config/wunder.yaml` 是 gitignore 的本地文件，
/// 被改写后无声无息）。不传 `--config` 时 `prepare_runtime_config_path` 会把仓库配置
/// **复制**到 `<temp-root>/config/wunder.yaml` 再用副本，回写只落在临时目录里。
fn run_cli(args: &[&str], lang: &str) {
    let repo_root = repo_root();
    let temp_root = unique_temp_root("cli");
    let output = Command::new(wunder_cli_exe())
        .current_dir(&repo_root)
        .env("WUNDER_CLI_PROJECT_ROOT", &repo_root)
        .args(args)
        .arg("--lang")
        .arg(lang)
        .arg("--user")
        .arg("smoke_user")
        .arg("--temp-root")
        .arg(&temp_root)
        .output()
        .expect("run wunder-cli");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    let _ = fs::remove_dir_all(&temp_root);

    assert!(
        output.status.success(),
        "cli command failed: lang={lang}, args={args:?}, status={:?}, stderr={stderr}",
        output.status.code()
    );
    assert!(
        !combined.contains("stack overflow"),
        "stack overflow detected: lang={lang}, args={args:?}"
    );
    assert!(
        !combined.contains("panicked at"),
        "panic detected: lang={lang}, args={args:?}, output={combined}"
    );
}

#[test]
fn non_interactive_entry_points_stay_stable() {
    let cases: [&[&str]; 5] = [
        &["tool", "list"],
        &["mcp", "list"],
        &["skills", "list"],
        &["config", "show"],
        &["doctor"],
    ];

    for lang in ["zh-CN", "en-US"] {
        for args in cases {
            run_cli(args, lang);
        }
    }
}

#[test]
fn the_command_surface_matches_the_codex_shaped_contract() {
    let repo_root = repo_root();
    let temp_root = unique_temp_root("cli_help");
    let output = Command::new(wunder_cli_exe())
        .current_dir(&repo_root)
        .env("WUNDER_CLI_PROJECT_ROOT", &repo_root)
        .arg("--help")
        .arg("--user")
        .arg("smoke_user")
        .arg("--temp-root")
        .arg(&temp_root)
        .output()
        .expect("run wunder-cli");
    let _ = fs::remove_dir_all(&temp_root);
    let stdout = String::from_utf8_lossy(&output.stdout);

    for expected in [
        "exec",
        "resume",
        "tool",
        "mcp",
        "skills",
        "config",
        "doctor",
        "cloud",
        "completion",
    ] {
        assert!(
            stdout.contains(expected),
            "help is missing `{expected}`: {stdout}"
        );
    }
    // The prompt subcommands were removed; they must not come back silently.
    assert!(
        !stdout.contains("\n  ask ") && !stdout.contains("\n  chat "),
        "removed prompt subcommands are still advertised: {stdout}"
    );
    // The codex-shaped global flags: `-C`/`-s`/`-p` and the config knobs.
    for expected in [
        "--cd",
        "--sandbox",
        "--approval-mode",
        "--profile",
        "--strict-config",
    ] {
        assert!(
            stdout.contains(expected),
            "help is missing `{expected}`: {stdout}"
        );
    }
    assert!(
        stdout.contains("read-only") && stdout.contains("danger-full-access"),
        "the sandbox words must be discoverable from --help: {stdout}"
    );
    assert!(
        stdout.contains("on-request") && stdout.contains("never"),
        "the approval words must be discoverable from --help: {stdout}"
    );
}

/// `-s read-only` is a real policy switch, and the engine must never be handed
/// a word it does not know. `config show` reports both dimensions, which makes
/// it the cheapest end-to-end check of the mapping.
#[test]
fn the_sandbox_flag_reaches_the_effective_config() {
    let repo_root = repo_root();
    let temp_root = unique_temp_root("cli_sandbox");
    let output = Command::new(wunder_cli_exe())
        .current_dir(&repo_root)
        .env("WUNDER_CLI_PROJECT_ROOT", &repo_root)
        .args(["config", "show"])
        .args(["-s", "read-only"])
        .arg("--user")
        .arg("smoke_user")
        .arg("--temp-root")
        .arg(&temp_root)
        .output()
        .expect("run wunder-cli");
    let _ = fs::remove_dir_all(&temp_root);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "config show -s read-only failed: {stdout}"
    );
    assert!(
        stdout.contains("\"sandbox_mode\": \"read-only\""),
        "the reported sandbox word must be the codex word: {stdout}"
    );
    assert!(
        stdout.contains("\"approval_policy\": \"suggest\""),
        "read-only must reach the engine as an approval gate: {stdout}"
    );
    assert!(
        stdout.contains("\"sandbox_mode_source\": \"cli\""),
        "the CLI flag must be reported as the deciding layer: {stdout}"
    );
}

/// The cloud commands run against the developer's real `~/.wunder` when the
/// env var is left alone, so these cases pin an isolated `WUNDER_HOME` first:
/// `cloud status` must report the logged-out state deterministically and never
/// depend on the host session.
fn run_cli_isolated_home(args: &[&str], lang: &str) -> (bool, String, String) {
    let repo_root = repo_root();
    let temp_root = unique_temp_root("cli_cloud");
    let output = Command::new(wunder_cli_exe())
        .current_dir(&repo_root)
        .env("WUNDER_CLI_PROJECT_ROOT", &repo_root)
        .env("WUNDER_HOME", &temp_root)
        .args(args)
        .arg("--lang")
        .arg(lang)
        .arg("--user")
        .arg("smoke_user")
        .arg("--temp-root")
        .arg(&temp_root)
        .output()
        .expect("run wunder-cli");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let success = output.status.success();
    let _ = fs::remove_dir_all(&temp_root);
    (success, stdout, stderr)
}

#[test]
fn cloud_status_reports_a_parseable_logged_out_state() {
    for lang in ["zh-CN", "en-US"] {
        let (success, stdout, stderr) = run_cli_isolated_home(&["cloud", "status"], lang);
        assert!(success, "cloud status failed: lang={lang}, stderr={stderr}");
        let expected = if lang == "zh-CN" {
            "未登录"
        } else {
            "not logged in"
        };
        assert!(
            stdout.contains(expected),
            "the logged-out status must be stated in stable wording: lang={lang}, stdout={stdout}"
        );
    }
}

#[test]
fn cloud_models_refresh_and_logout_require_a_session() {
    for lang in ["zh-CN", "en-US"] {
        let expected = if lang == "zh-CN" {
            "未登录"
        } else {
            "not logged in"
        };
        for args in [["cloud", "models"], ["cloud", "refresh"]] {
            let (success, stdout, stderr) = run_cli_isolated_home(&args, lang);
            assert!(
                !success,
                "{args:?} must fail while logged out: lang={lang}, stdout={stdout}"
            );
            let combined = format!("{stdout}\n{stderr}");
            assert!(
                combined.contains(expected),
                "{args:?} must say why it failed: lang={lang}, output={combined}"
            );
        }

        // Logout is idempotent: a missing session is still a clean exit.
        let (success, stdout, stderr) = run_cli_isolated_home(&["cloud", "logout"], lang);
        assert!(success, "cloud logout failed: lang={lang}, stderr={stderr}");
        let expected = if lang == "zh-CN" {
            "已登出"
        } else {
            "logged out"
        };
        assert!(
            stdout.contains(expected),
            "logout must confirm the clean state: lang={lang}, stdout={stdout}"
        );
    }
}

/// 端控云 surface (plan §8.2) has to parse and exit cleanly with no session:
/// every command is one-shot, so a logged-out run must refuse in the user's
/// language instead of hanging on the network or failing on argument syntax.
#[test]
fn the_cloud_interlink_commands_parse_and_refuse_without_a_session() {
    const CASES: &[&[&str]] = &[
        &["cloud", "ws", "ls", "docs", "--limit", "10"],
        &["cloud", "ws", "cat", "docs/a.md", "--lines", "5"],
        &["cloud", "ws", "pull", "docs/a.md", "-o", "copies/a.md", "--force"],
        &["cloud", "ws", "push", "docs/a.md", "-d", "in/a.md"],
        &["cloud", "threads", "--limit", "5"],
        &["cloud", "thread", "show", "th_1", "--raw"],
        &["cloud", "send", "--attach", "--seconds", "3", "hello"],
        &[
            "cloud",
            "audit",
            "--action",
            "command.issue",
            "--device",
            "dev-1",
            "--since",
            "1700000000",
            "--csv",
        ],
        &["cloud", "watch", "--to", "dev-1", "--thread", "th_1", "--seconds", "1"],
    ];
    for lang in ["zh-CN", "en-US"] {
        let expected = if lang == "zh-CN" {
            "未登录"
        } else {
            "not logged in"
        };
        for args in CASES {
            let (success, stdout, stderr) = run_cli_isolated_home(args, lang);
            assert!(
                !success,
                "{args:?} must fail while logged out: lang={lang}, stdout={stdout}"
            );
            let combined = format!("{stdout}\n{stderr}");
            assert!(
                combined.contains(expected),
                "{args:?} must say why it refused: lang={lang}, output={combined}"
            );
        }
    }
}

/// `ws pull` refuses a landing path outside the workspace before it reads
/// anything, so a typo can never write next to the repo.
#[test]
fn cloud_ws_pull_refuses_a_landing_path_outside_the_workspace() {
    for lang in ["zh-CN", "en-US"] {
        let args: &[&str] = &["cloud", "ws", "pull", "docs/a.md", "-o", "../../out.md"];
        let (success, stdout, stderr) = run_cli_isolated_home(args, lang);
        assert!(!success, "the escape attempt must fail: lang={lang}");
        let combined = format!("{stdout}\n{stderr}");
        let expected = if lang == "zh-CN" {
            "工作区"
        } else {
            "workspace"
        };
        assert!(
            combined.contains(expected),
            "the refusal must name the workspace bound: lang={lang}, output={combined}"
        );
    }
}
