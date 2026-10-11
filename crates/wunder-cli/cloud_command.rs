//! `cloud` command group (plan §6.2): login/logout/status/models/refresh/logs
//! against the engine-level `CloudService`. The CLI adds no second execution
//! chain here: every command delegates to `wunder_server::cloud::shared()` and
//! the process config store, exactly like the desktop facade.

use crate::args::{CloudCommand, CloudLoginCommand, CloudLogsCommand, CloudSubcommand, GlobalArgs};
use crate::runtime::CliRuntime;
use anyhow::{anyhow, Context, Result};
use std::io::{self, IsTerminal, Write};
use wunder_server::cloud::{
    shared as cloud_shared, CloudStatus, CLOUD_MODEL_PREFIX, CONNECTION_EXPIRED,
    CONNECTION_LOGGED_OUT, CONNECTION_ONLINE, CONNECTION_RECONNECTING,
};

pub(crate) async fn handle_cloud(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudCommand,
) -> Result<()> {
    match command.command {
        CloudSubcommand::Login(command) => cloud_login(runtime, global, command).await,
        CloudSubcommand::Logout => cloud_logout(runtime, global).await,
        CloudSubcommand::Status => cloud_status(runtime, global).await,
        CloudSubcommand::Models => cloud_models(runtime, global).await,
        CloudSubcommand::Refresh => cloud_refresh(runtime, global).await,
        CloudSubcommand::Logs(command) => cloud_logs(runtime, global, command).await,
        interlink @ (CloudSubcommand::Devices
        | CloudSubcommand::Ws(_)
        | CloudSubcommand::Threads(_)
        | CloudSubcommand::Thread(_)
        | CloudSubcommand::Send(_)
        | CloudSubcommand::Watch(_)
        | CloudSubcommand::Audit(_)
        | CloudSubcommand::Approve(_)) => {
            crate::cloud_interlink::handle_cloud_interlink(runtime, global, interlink).await
        }
    }
}

async fn cloud_login(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudLoginCommand,
) -> Result<()> {
    let language = crate::locale::resolve_cli_language(global);
    let is_zh = crate::locale::is_zh_language(language.as_str());
    let interactive = io::stdin().is_terminal();

    let server = command.server.trim().to_string();
    let username = match command
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(username) => username.to_string(),
        None if interactive => prompt_line(if is_zh { "用户名: " } else { "username: " })?,
        None => {
            return Err(anyhow!(crate::locale::tr(
                language.as_str(),
                "非交互环境需要 -u/--username 指定用户名",
                "non-interactive runs need -u/--username",
            )))
        }
    };

    // The password is read once and never stored: the env override is removed
    // immediately, the interactive prompt keeps echo off and nothing is logged.
    let password = match std::env::var("WUNDER_PASSWORD") {
        Ok(password) => {
            std::env::remove_var("WUNDER_PASSWORD");
            password
        }
        Err(_) if interactive => {
            read_password_hidden(if is_zh { "密码: " } else { "password: " })?
        }
        Err(_) => {
            return Err(anyhow!(crate::locale::tr(
                language.as_str(),
                "非交互环境请通过 WUNDER_PASSWORD 环境变量提供密码",
                "non-interactive runs need the WUNDER_PASSWORD environment variable",
            )))
        }
    };
    if password.is_empty() {
        return Err(anyhow!(crate::locale::tr(
            language.as_str(),
            "密码不能为空",
            "the password must not be empty",
        )));
    }

    let status = cloud_shared()
        .login(
            &runtime.state.config_store,
            &server,
            &username,
            &password,
            "cli",
        )
        .await?;

    let mut lines = cloud_status_lines(&status, is_zh);
    let cloud_models = count_cloud_models(runtime).await;
    lines.push(if is_zh {
        format!("云模型: {cloud_models} 个（-m cloud/<模型名> 可直接使用）")
    } else {
        format!("cloud models: {cloud_models} (use -m cloud/<name> directly)")
    });
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

async fn cloud_logout(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let language = crate::locale::resolve_cli_language(global);
    cloud_shared().logout(&runtime.state.config_store).await?;
    println!(
        "{}",
        crate::locale::tr(language.as_str(), "已登出", "logged out")
    );
    Ok(())
}

async fn cloud_status(_runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let language = crate::locale::resolve_cli_language(global);
    let is_zh = crate::locale::is_zh_language(language.as_str());
    let status = cloud_shared().status();
    for line in cloud_status_lines(&status, is_zh) {
        println!("{line}");
    }
    // The tunnel line is the account's own view (nodes + open approvals). This
    // command never starts the tunnel client, which only runs in the TUI.
    for line in crate::cloud_interlink::status_interlink_lines(is_zh).await {
        println!("{line}");
    }
    Ok(())
}

async fn cloud_models(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let language = crate::locale::resolve_cli_language(global);
    let is_zh = crate::locale::is_zh_language(language.as_str());
    let status = cloud_shared().status();
    if !status.logged_in {
        return Err(anyhow!(crate::locale::tr(
            language.as_str(),
            "未登录；先运行 wunder-cli cloud login --server http://ip:port",
            "not logged in; run wunder-cli cloud login --server http://ip:port first",
        )));
    }
    let names = list_cloud_models(runtime).await;
    if names.is_empty() {
        println!(
            "{}",
            crate::locale::tr(
                language.as_str(),
                "无云模型；运行 wunder-cli cloud refresh 重新拉取",
                "no cloud models; run wunder-cli cloud refresh to re-sync",
            )
        );
        return Ok(());
    }
    println!(
        "{}",
        if is_zh {
            format!("云模型 ({}):", names.len())
        } else {
            format!("cloud models ({}):", names.len())
        }
    );
    for name in names {
        println!("- {name}");
    }
    Ok(())
}

async fn cloud_refresh(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let language = crate::locale::resolve_cli_language(global);
    let is_zh = crate::locale::is_zh_language(language.as_str());
    let service = cloud_shared();
    if !service.status().logged_in {
        return Err(anyhow!(crate::locale::tr(
            language.as_str(),
            "未登录；先运行 wunder-cli cloud login --server http://ip:port",
            "not logged in; run wunder-cli cloud login --server http://ip:port first",
        )));
    }
    let account = service.refresh_account().await?;
    let models = service
        .synthesize_cloud_models(&runtime.state.config_store)
        .await?;
    println!(
        "{}",
        if is_zh {
            format!(
                "账户已刷新：余额 {}（累计发放 {}，已用 {}）；云模型 {} 个",
                account.quota.balance,
                account.quota.granted_total,
                account.quota.used_total,
                models.len()
            )
        } else {
            format!(
                "account refreshed: balance {} (granted {}, used {}); {} cloud models",
                account.quota.balance,
                account.quota.granted_total,
                account.quota.used_total,
                models.len()
            )
        }
    );
    Ok(())
}

async fn cloud_logs(
    _runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudLogsCommand,
) -> Result<()> {
    let language = crate::locale::resolve_cli_language(global);
    let is_zh = crate::locale::is_zh_language(language.as_str());
    if !command.flush {
        return Err(anyhow!(crate::locale::tr(
            language.as_str(),
            "用法: wunder-cli cloud logs --flush",
            "usage: wunder-cli cloud logs --flush",
        )));
    }
    let service = cloud_shared();
    if !service.status().logged_in {
        println!(
            "{}",
            crate::locale::tr(
                language.as_str(),
                "未登录，无上报缓冲",
                "not logged in, nothing buffered"
            )
        );
        return Ok(());
    }
    let pending = service.flush_logs().await?;
    println!(
        "{}",
        if is_zh {
            format!("日志上报缓冲已冲刷，剩余待上报 {pending} 条")
        } else {
            format!("log buffer flushed, {pending} entries pending")
        }
    );
    Ok(())
}

async fn count_cloud_models(runtime: &CliRuntime) -> usize {
    list_cloud_models(runtime).await.len()
}

async fn list_cloud_models(runtime: &CliRuntime) -> Vec<String> {
    let config = runtime.state.config_store.get().await;
    let mut names: Vec<String> = config
        .llm
        .models
        .keys()
        .filter(|key| key.starts_with(CLOUD_MODEL_PREFIX))
        .cloned()
        .collect();
    names.sort();
    names
}

/// Multi-line human-readable status. Stable `label: value` lines keep the
/// output parseable for scripts and tests.
pub(crate) fn cloud_status_lines(status: &CloudStatus, is_zh: bool) -> Vec<String> {
    cloud_status_lines_at(status, is_zh, now_unix_seconds())
}

/// `cloud_status_lines` against an injected clock so tests stay deterministic.
fn cloud_status_lines_at(status: &CloudStatus, is_zh: bool, now: f64) -> Vec<String> {
    if !status.logged_in {
        return vec![if is_zh {
            "未登录（未连接云端）".to_string()
        } else {
            "not logged in (no cloud connection)".to_string()
        }];
    }

    let mut lines = Vec::new();
    let username = status.username.clone().unwrap_or_default();
    lines.push(if is_zh {
        format!("已登录: {username}")
    } else {
        format!("logged in: {username}")
    });
    if let Some(server) = status.server.as_deref() {
        lines.push(if is_zh {
            format!("服务: {server}")
        } else {
            format!("server: {server}")
        });
    }
    if let Some(device_id) = status.device_id.as_deref() {
        lines.push(if is_zh {
            format!("设备: {device_id}")
        } else {
            format!("device: {device_id}")
        });
    }
    lines.extend(cloud_connection_lines(status, is_zh, now));
    if let Some(max) = status.max_concurrent_calls {
        let active = status.concurrency_active.unwrap_or(0);
        let queued = status.concurrency_queued.unwrap_or(0);
        lines.push(if is_zh {
            format!("并发: {active}/{max}（排队 {queued}）")
        } else {
            format!("concurrency: {active}/{max} (queued {queued})")
        });
    }
    lines.push(if status.expired {
        if is_zh {
            "会话状态: 已过期（请重新登录）".to_string()
        } else {
            "session: expired (please log in again)".to_string()
        }
    } else if is_zh {
        "会话状态: 正常".to_string()
    } else {
        "session: ok".to_string()
    });
    lines.push(if status.preferences_sync_enabled {
        if is_zh {
            "偏好同步: 开启".to_string()
        } else {
            "preferences sync: on".to_string()
        }
    } else if is_zh {
        "偏好同步: 关闭".to_string()
    } else {
        "preferences sync: off".to_string()
    });
    if let Some(quota) = status.quota.as_ref() {
        lines.push(if is_zh {
            format!(
                "余额: {}（累计发放 {}，已用 {}，每日发放 {}）",
                quota.balance, quota.granted_total, quota.used_total, quota.daily_grant
            )
        } else {
            format!(
                "balance: {} (granted {}, used {}, daily grant {})",
                quota.balance, quota.granted_total, quota.used_total, quota.daily_grant
            )
        });
    }
    lines
}

/// The `连接:` / `最近成功:` lines: the engine connection state machine already
/// holds the live answer, so the CLI only words it. Missing reason parts
/// degrade gracefully instead of printing empty slots.
fn cloud_connection_lines(status: &CloudStatus, is_zh: bool, now: f64) -> Vec<String> {
    let value = match status.connection {
        CONNECTION_ONLINE => {
            if is_zh {
                "在线".to_string()
            } else {
                "online".to_string()
            }
        }
        CONNECTION_EXPIRED => {
            if is_zh {
                "已过期".to_string()
            } else {
                "expired".to_string()
            }
        }
        CONNECTION_LOGGED_OUT => {
            if is_zh {
                "未登录".to_string()
            } else {
                "logged out".to_string()
            }
        }
        CONNECTION_RECONNECTING => {
            let base = if is_zh { "重连中" } else { "reconnecting" };
            let error_part = status.last_error.as_deref().map(|error| {
                if is_zh {
                    format!("上次错误: {error}")
                } else {
                    format!("last error: {error}")
                }
            });
            let retry_part = status.next_retry_at.map(|at| {
                let secs = ((at - now).ceil() as i64).max(0);
                if is_zh {
                    format!("{secs} 秒后重试")
                } else {
                    format!("retry in {secs}s")
                }
            });
            match (error_part, retry_part) {
                (Some(error), Some(retry)) => {
                    let detail = if is_zh {
                        format!("（{error}，{retry}）")
                    } else {
                        format!(" ({error}, {retry})")
                    };
                    format!("{base}{detail}")
                }
                (Some(error), None) => {
                    let detail = if is_zh {
                        format!("（{error}）")
                    } else {
                        format!(" ({error})")
                    };
                    format!("{base}{detail}")
                }
                (None, Some(retry)) => {
                    let detail = if is_zh {
                        format!("（{retry}）")
                    } else {
                        format!(" ({retry})")
                    };
                    format!("{base}{detail}")
                }
                (None, None) => base.to_string(),
            }
        }
        other => other.to_string(),
    };
    let mut lines = vec![if is_zh {
        format!("连接: {value}")
    } else {
        format!("connection: {value}")
    }];
    if let Some(at) = status.last_success_at {
        let minutes = ((now - at) / 60.0).floor().max(0.0) as u64;
        lines.push(if is_zh {
            format!("最近成功: {minutes} 分钟前")
        } else {
            format!("last success: {minutes} min ago")
        });
    }
    lines
}

/// Wall-clock unix seconds, matching the engine's `reporter::now_unix_seconds`.
fn now_unix_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

/// Composer-bar text for the status bar badge (plan §6.2): `☁ alice · 980`
/// while logged in, `☁ reconnecting` while the engine retries the link,
/// `☁ expired` after a 401, nothing while logged out.
pub(crate) fn cloud_badge_text(status: &CloudStatus, is_zh: bool) -> Option<String> {
    if !status.logged_in {
        return None;
    }
    if status.expired || status.connection == CONNECTION_EXPIRED {
        return Some(if is_zh {
            "☁ 已过期".to_string()
        } else {
            "☁ expired".to_string()
        });
    }
    if status.connection == CONNECTION_RECONNECTING {
        return Some(if is_zh {
            "☁ 重连中".to_string()
        } else {
            "☁ reconnecting".to_string()
        });
    }
    let username = status.username.clone().unwrap_or_default();
    let balance = status.quota.as_ref().map(|quota| quota.balance.to_string());
    match (username.is_empty(), balance) {
        (false, Some(balance)) => Some(format!("☁ {username} · {balance}")),
        (false, None) => Some(format!("☁ {username}")),
        (true, Some(balance)) => Some(format!("☁ · {balance}")),
        (true, None) => Some("☁".to_string()),
    }
}

/// Whether the status-bar badge needs the warning colour: only the
/// reconnecting state is a transient signal the user can wait on, so the
/// footer paints it in the warning tone instead of the dim default.
pub(crate) fn cloud_badge_is_reconnecting(status: &CloudStatus) -> bool {
    status.connection == CONNECTION_RECONNECTING
}

/// Map a model-call failure onto the composer notice text. The engine
/// stringifies cloud errors before they reach the stream, so the signatures
/// match the engine error messages (`services/cloud/mod.rs`, `services/llm.rs`).
/// `connection` is the engine's live connection state: while the engine is
/// already retrying in the background, a network failure reads as a transient
/// interruption rather than a hard outage.
pub(crate) fn cloud_error_notice(language: &str, error: &str, connection: &str) -> Option<String> {
    let is_zh = crate::locale::is_zh_language(language);
    let lowered = error.to_ascii_lowercase();
    if lowered.contains("cloud session expired") {
        return Some(if is_zh {
            "云端会话已过期，请重新登录".to_string()
        } else {
            "cloud session expired, please log in again".to_string()
        });
    }
    if lowered.contains("cloud quota insufficient") {
        let balance = lowered.find("balance").and_then(|at| {
            error[at + "balance".len()..]
                .trim_start_matches([':', ' '])
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
                .collect::<String>()
                .parse::<i64>()
                .ok()
        });
        return Some(match balance {
            Some(balance) if is_zh => format!("云端额度不足（余额 {balance}）"),
            Some(balance) => format!("cloud quota insufficient (balance {balance})"),
            None if is_zh => "云端额度不足".to_string(),
            None => "cloud quota insufficient".to_string(),
        });
    }
    if lowered.contains("cloud queue timeout") {
        return Some(if is_zh {
            "云端排队超时".to_string()
        } else {
            "cloud queue timeout".to_string()
        });
    }
    if lowered.contains("cloud server unreachable")
        || lowered.contains("cloud llm request failed")
        || lowered.contains("cloud device registration failed")
        || lowered.contains("cloud account fetch failed")
        || lowered.contains("cloud model discovery failed")
        || lowered.contains("cloud log upload failed")
    {
        // Distinguish the actively-reconnecting link from a hard outage; the
        // queue-timeout wording above is unaffected by the connection state.
        if connection == CONNECTION_RECONNECTING {
            return Some(if is_zh {
                "云端连接中断，自动重试中…".to_string()
            } else {
                "cloud connection lost, retrying automatically…".to_string()
            });
        }
        return Some(if is_zh {
            "云端不可达".to_string()
        } else {
            "cloud unreachable".to_string()
        });
    }
    None
}

/// Prompt for one visible line on the terminal.
fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush().context("flush prompt failed")?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("read input failed")?;
    Ok(line.trim().to_string())
}

/// Hidden password read without echo, built on crossterm (already the TUI's
/// input backend, so no new dependency). Characters are masked with `*`;
/// Backspace edits, Enter submits, Ctrl+C/Esc cancel.
fn read_password_hidden(prompt: &str) -> Result<String> {
    use crossterm::event::{read, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    let mut stdout = io::stdout();
    print!("{prompt}");
    stdout.flush().context("flush prompt failed")?;
    enable_raw_mode().context("enable terminal raw mode failed")?;
    let result = (|| -> Result<String> {
        let mut password = String::new();
        loop {
            let event = read().context("read password key failed")?;
            let Event::Key(key) = event else {
                continue;
            };
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            match key.code {
                KeyCode::Enter => break,
                KeyCode::Backspace => {
                    if password.pop().is_some() {
                        // Erase one mask cell from the line.
                        write!(stdout, "\x08 \x08").ok();
                        stdout.flush().ok();
                    }
                }
                KeyCode::Esc => return Err(anyhow!("login cancelled")),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Err(anyhow!("login cancelled"))
                }
                KeyCode::Char(ch)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    password.push(ch);
                    write!(stdout, "*").ok();
                    stdout.flush().ok();
                }
                _ => {}
            }
        }
        Ok(password)
    })();
    disable_raw_mode().context("disable terminal raw mode failed")?;
    writeln!(stdout).ok();
    stdout.flush().ok();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use wunder_server::cloud::CloudQuotaSnapshot;

    fn quota_snapshot(balance: i64) -> CloudQuotaSnapshot {
        CloudQuotaSnapshot {
            balance,
            granted_total: 1000,
            used_total: 20,
            daily_grant: 100,
            last_grant_date: None,
        }
    }

    fn logged_out_status() -> CloudStatus {
        CloudStatus {
            logged_in: false,
            server: None,
            user_id: None,
            username: None,
            quota: None,
            max_concurrent_calls: None,
            concurrency_active: None,
            concurrency_queued: None,
            expired: false,
            preferences_sync_enabled: true,
            device_id: None,
            client: None,
            connection: "logged_out",
            last_error: None,
            last_success_at: None,
            next_retry_at: None,
        }
    }

    fn logged_in_status(balance: i64) -> CloudStatus {
        CloudStatus {
            logged_in: true,
            server: Some("http://127.0.0.1:8000".to_string()),
            user_id: Some("u1".to_string()),
            username: Some("alice".to_string()),
            quota: Some(quota_snapshot(balance)),
            max_concurrent_calls: Some(2),
            concurrency_active: Some(1),
            concurrency_queued: Some(0),
            expired: false,
            preferences_sync_enabled: true,
            device_id: Some("dev-1".to_string()),
            client: Some("cli".to_string()),
            connection: "online",
            last_error: None,
            last_success_at: None,
            next_retry_at: None,
        }
    }

    #[test]
    fn status_lines_stay_stable_and_labeled() {
        let lines = cloud_status_lines(&logged_in_status(980), true);
        let text = lines.join("\n");
        assert!(text.contains("已登录: alice"), "{text}");
        assert!(text.contains("服务: http://127.0.0.1:8000"), "{text}");
        assert!(text.contains("设备: dev-1"), "{text}");
        assert!(text.contains("连接: 在线"), "{text}");
        assert!(text.contains("并发: 1/2（排队 0）"), "{text}");
        assert!(text.contains("会话状态: 正常"), "{text}");
        assert!(text.contains("偏好同步: 开启"), "{text}");
        assert!(text.contains("余额: 980"), "{text}");
        assert_eq!(
            cloud_status_lines(&logged_out_status(), true),
            vec!["未登录（未连接云端）"]
        );
    }

    /// A logged-in status forced onto one engine connection state, with the
    /// reconnecting reason parts filled in.
    fn status_with_connection(connection: &'static str) -> CloudStatus {
        let mut status = logged_in_status(980);
        status.connection = connection;
        if connection == CONNECTION_RECONNECTING {
            status.last_error = Some("connection refused".to_string());
            status.next_retry_at = Some(1120.0);
        }
        status
    }

    #[test]
    fn the_connection_line_words_the_engine_state_machine() {
        // Injected clock keeps the retry countdown and the success age stable.
        let now = 1000.0;

        let mut online = status_with_connection(CONNECTION_ONLINE);
        online.last_success_at = Some(700.0);
        let text = cloud_status_lines_at(&online, true, now).join("\n");
        assert!(text.contains("连接: 在线"), "{text}");
        assert!(text.contains("最近成功: 5 分钟前"), "{text}");
        let english = cloud_status_lines_at(&online, false, now).join("\n");
        assert!(english.contains("connection: online"), "{english}");
        assert!(english.contains("last success: 5 min ago"), "{english}");

        let reconnecting = status_with_connection(CONNECTION_RECONNECTING);
        let text = cloud_status_lines_at(&reconnecting, true, now).join("\n");
        assert!(
            text.contains("连接: 重连中（上次错误: connection refused，120 秒后重试）"),
            "{text}"
        );
        let english = cloud_status_lines_at(&reconnecting, false, now).join("\n");
        assert!(
            english.contains(
                "connection: reconnecting (last error: connection refused, retry in 120s)"
            ),
            "{english}"
        );

        // Missing reason parts degrade instead of printing empty slots.
        let mut bare = status_with_connection(CONNECTION_RECONNECTING);
        bare.last_error = None;
        bare.next_retry_at = None;
        assert!(cloud_status_lines_at(&bare, true, now).contains(&"连接: 重连中".to_string()));
        let mut retry_only = status_with_connection(CONNECTION_RECONNECTING);
        retry_only.last_error = None;
        assert!(cloud_status_lines_at(&retry_only, true, now)
            .contains(&"连接: 重连中（120 秒后重试）".to_string()));

        let expired = status_with_connection(CONNECTION_EXPIRED);
        let text = cloud_status_lines_at(&expired, true, now).join("\n");
        assert!(text.contains("连接: 已过期"), "{text}");

        // The logged-out mapping exists even though the command short-circuits
        // before this line can be reached.
        let line = cloud_connection_lines(&logged_out_status(), true, now).remove(0);
        assert_eq!(line, "连接: 未登录");
    }

    #[test]
    fn the_badge_shows_login_quota_and_expiry() {
        assert_eq!(cloud_badge_text(&logged_out_status(), true), None);
        assert_eq!(
            cloud_badge_text(&logged_in_status(980), true).as_deref(),
            Some("☁ alice · 980")
        );
        let mut expired = logged_in_status(980);
        expired.expired = true;
        assert_eq!(
            cloud_badge_text(&expired, true).as_deref(),
            Some("☁ 已过期")
        );
        assert_eq!(
            cloud_badge_text(&expired, false).as_deref(),
            Some("☁ expired")
        );
        let reconnecting = status_with_connection(CONNECTION_RECONNECTING);
        assert_eq!(
            cloud_badge_text(&reconnecting, true).as_deref(),
            Some("☁ 重连中")
        );
        assert_eq!(
            cloud_badge_text(&reconnecting, false).as_deref(),
            Some("☁ reconnecting")
        );
        assert!(cloud_badge_is_reconnecting(&reconnecting));
        assert!(!cloud_badge_is_reconnecting(&logged_in_status(980)));
    }

    #[test]
    fn cloud_errors_map_onto_the_composer_wording() {
        assert_eq!(
            cloud_error_notice(
                "zh-CN",
                "cloud session expired, please log in again",
                CONNECTION_ONLINE
            )
            .as_deref(),
            Some("云端会话已过期，请重新登录")
        );
        assert_eq!(
            cloud_error_notice(
                "zh-CN",
                "cloud quota insufficient: balance 12 (granted 1000, used 988, daily grant 100)",
                CONNECTION_ONLINE,
            )
            .as_deref(),
            Some("云端额度不足（余额 12）")
        );
        assert_eq!(
            cloud_error_notice(
                "zh-CN",
                "cloud queue timeout: waited over 300s (position 1)",
                CONNECTION_RECONNECTING
            )
            .as_deref(),
            Some("云端排队超时")
        );
        // While the engine is already reconnecting, a network failure reads as
        // a transient interruption; otherwise it stays a hard outage.
        assert_eq!(
            cloud_error_notice(
                "zh-CN",
                "cloud LLM request failed: connection refused",
                CONNECTION_RECONNECTING
            )
            .as_deref(),
            Some("云端连接中断，自动重试中…")
        );
        assert_eq!(
            cloud_error_notice(
                "zh-CN",
                "cloud LLM request failed: connection refused",
                CONNECTION_ONLINE
            )
            .as_deref(),
            Some("云端不可达")
        );
        assert_eq!(
            cloud_error_notice("en-US", "cloud server unreachable", CONNECTION_RECONNECTING)
                .as_deref(),
            Some("cloud connection lost, retrying automatically…")
        );
        assert_eq!(
            cloud_error_notice("zh-CN", "some other failure", CONNECTION_ONLINE),
            None
        );
        assert_eq!(
            cloud_error_notice(
                "en-US",
                "cloud session expired, please log in again",
                CONNECTION_ONLINE
            )
            .as_deref(),
            Some("cloud session expired, please log in again")
        );
        // A quota message without a parsable balance still maps.
        assert_eq!(
            cloud_error_notice("zh-CN", "cloud quota insufficient", CONNECTION_ONLINE).as_deref(),
            Some("云端额度不足")
        );
    }
}
