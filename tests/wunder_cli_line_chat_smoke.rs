use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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

fn run_line_chat_slash(lang: &str, slash_command: &str) {
    let temp_root = unique_temp_root("line_chat");
    let mut child = Command::new(wunder_cli_exe())
        .arg("chat")
        .arg("--lang")
        .arg(lang)
        .arg("--user")
        .arg("smoke_user")
        .arg("--temp-root")
        .arg(&temp_root)
        .arg("--config")
        .arg("config/wunder.yaml")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn wunder-cli chat");

    {
        let stdin = child.stdin.as_mut().expect("stdin available");
        writeln!(stdin, "{slash_command}").expect("write slash command");
        writeln!(stdin, "/exit").expect("write exit");
    }

    let output = child.wait_with_output().expect("wait wunder-cli");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    let lowered = combined.to_ascii_lowercase();
    let _ = fs::remove_dir_all(&temp_root);

    assert!(
        output.status.success(),
        "line-chat slash failed: lang={lang}, slash={slash_command}, status={:?}, stderr={stderr}",
        output.status.code()
    );
    assert!(
        !lowered.contains("stack overflow"),
        "line-chat stack overflow detected: lang={lang}, slash={slash_command}"
    );
}

#[test]
fn line_chat_slash_commands_stay_stable() {
    let slash_commands = [
        "/status",
        "/model",
        "/system",
        "/mcp list",
        "/skills list",
        "/apps list",
    ];

    for lang in ["zh-CN", "en-US"] {
        for slash in slash_commands {
            run_line_chat_slash(lang, slash);
        }
    }
}
