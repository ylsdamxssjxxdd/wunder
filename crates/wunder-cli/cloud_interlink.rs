//! `cloud` interlink subcommands (plan I10): drive the interlink ledger from
//! the terminal. Every command is a thin REST client over the user-plane
//! endpoints — the CLI adds no second execution chain; the server (for
//! `cloud` targets) or the owning node (for `device:<id>` targets) runs the
//! action, the ledger only carries idempotency, approval and the result.

use crate::args::{CloudApproveCommand, CloudAuditCommand, CloudSendCommand, CloudWatchCommand, CloudWsCommand, GlobalArgs};
use crate::runtime::CliRuntime;
use anyhow::{anyhow, Context, Result};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use wunder_server::cloud::{shared as cloud_shared, CloudSessionFile};

/// Bounded wait for one command to reach a terminal state.
const POLL_TIMEOUT_S: u64 = 90;
const POLL_INTERVAL: Duration = Duration::from_millis(400);

const TERMINAL_STATUSES: [&str; 4] = ["succeeded", "failed", "canceled", "timeout"];

pub(crate) async fn handle_cloud_interlink(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: crate::args::CloudSubcommand,
) -> Result<()> {
    match command {
        crate::args::CloudSubcommand::Devices => devices(runtime, global).await,
        crate::args::CloudSubcommand::Ws(command) => ws_list(runtime, global, command).await,
        crate::args::CloudSubcommand::Send(command) => send(runtime, global, command).await,
        crate::args::CloudSubcommand::Watch(command) => watch(runtime, global, command).await,
        crate::args::CloudSubcommand::Audit(command) => audit(runtime, global, command).await,
        crate::args::CloudSubcommand::Approve(command) => approve(runtime, global, command).await,
        _ => unreachable!("non-interlink cloud subcommands are handled in cloud_command"),
    }
}

fn language_of(global: &GlobalArgs) -> String {
    crate::locale::resolve_cli_language(global)
}

fn zh(global: &GlobalArgs) -> bool {
    crate::locale::is_zh_language(language_of(global).as_str())
}

/// The cloud session is the only credential source; a logged-out CLI has
/// nothing to present to the interlink plane.
fn session() -> Result<CloudSessionFile> {
    cloud_shared()
        .session()
        .ok_or_else(|| anyhow!("未登录云端（先 cloud login）/ not logged in (run cloud login first)"))
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("build http client")
}

fn base_url(session: &CloudSessionFile) -> String {
    session.server.trim().trim_end_matches('/').to_string()
}

async fn get_json(session: &CloudSessionFile, path: &str) -> Result<Value> {
    let url = format!("{}{}", base_url(session), path);
    let response = http()?
        .get(url)
        .bearer_auth(&session.token)
        .send()
        .await
        .context("request failed")?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(anyhow!("HTTP {status}: {}", summarize_error(&body)));
    }
    Ok(body)
}

async fn post_json(session: &CloudSessionFile, path: &str, body: Value) -> Result<Value> {
    let url = format!("{}{}", base_url(session), path);
    let response = http()?
        .post(url)
        .bearer_auth(&session.token)
        .json(&body)
        .send()
        .await
        .context("request failed")?;
    let status = response.status();
    let text: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(anyhow!("HTTP {status}: {}", summarize_error(&text)));
    }
    Ok(text)
}

fn summarize_error(body: &Value) -> String {
    body.get("error")
        .or_else(|| body.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("request rejected")
        .to_string()
}

/// Issue one interlink command and wait for its terminal state. A pending
/// approval returns immediately so the caller can print the approve hint
/// instead of spinning out the poll window.
async fn issue_and_wait(session: &CloudSessionFile, to_node: &str, kind: &str, args: Value) -> Result<Value> {
    let issue = post_json(
        session,
        "/wunder/interlink/commands",
        json!({"to": to_node, "kind": kind, "args": args}),
    )
    .await?;
    let command_id = issue
        .pointer("/data/command_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("issue returned no command id"))?
        .to_string();
    let deadline = std::time::Instant::now() + Duration::from_secs(POLL_TIMEOUT_S);
    loop {
        let record = get_json(
            session,
            &format!("/wunder/interlink/commands/{command_id}"),
        )
        .await?;
        let data = data_of(&record);
        let status = data.get("status").and_then(Value::as_str).unwrap_or("");
        if data.get("approval_state").and_then(Value::as_str) == Some("pending")
            || TERMINAL_STATUSES.contains(&status)
        {
            return Ok(record);
        }
        if std::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "命令 {command_id} 在 {POLL_TIMEOUT_S}s 内未完成（当前 {status}）；可用 GET /wunder/interlink/commands/{command_id} 继续查询"
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

fn data_of(record: &Value) -> &Value {
    record.get("data").unwrap_or(&Value::Null)
}

fn print_command_outcome(record: &Value) -> Result<()> {
    let data = data_of(record);
    let status = data.get("status").and_then(Value::as_str).unwrap_or("");
    let command_id = data.get("command_id").and_then(Value::as_str).unwrap_or("");
    match status {
        "succeeded" => {
            println!("{}", format_result(data));
            Ok(())
        }
        "issued" | "acked" | "running" => Err(anyhow!(
            "命令 {command_id} 仍在执行（{status}），稍后可重新查询 / command {command_id} is still {status}; query it again later"
        )),
        _ if data.get("approval_state").and_then(Value::as_str) == Some("pending") => Err(anyhow!(
            "命令 {command_id} 等待审批：在蜂巢设备面板或用 `cloud approve {command_id} --yes` 处理 / command {command_id} awaits approval: decide in the web device panel or via `cloud approve {command_id} --yes`"
        )),
        _ => {
            let code = data.get("error_code").and_then(Value::as_str).unwrap_or("-");
            let summary = data
                .get("error_summary")
                .and_then(Value::as_str)
                .unwrap_or("-");
            Err(anyhow!(
                "命令 {command_id} 终态 {status}: {code} {summary} / command {command_id} finished {status}: {code} {summary}"
            ))
        }
    }
}

fn format_result(data: &Value) -> String {
    let result = data.get("result").cloned().unwrap_or(Value::Null);
    match result {
        Value::Null => "(no result payload)".to_string(),
        Value::String(text) => text,
        _ => serde_json::to_string_pretty(&result).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// devices
// ---------------------------------------------------------------------------

async fn devices(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let is_zh = zh(global);
    let _ = runtime;
    let session = session()?;
    let body = get_json(&session, "/wunder/interlink/nodes").await?;
    let nodes = body
        .pointer("/data/nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let online = body
        .pointer("/data/online_count")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    println!(
        "{}",
        if is_zh {
            format!("节点 {}/{} 在线", online, nodes.len())
        } else {
            format!("nodes {online}/{} online", nodes.len())
        }
    );
    println!("{:<24} {:<8} {:<12} {:<20}", "node", "type", "status", "label");
    for node in &nodes {
        println!(
            "{:<24} {:<8} {:<12} {:<20}",
            node.get("node_id").and_then(Value::as_str).unwrap_or("-"),
            node.get("node_type").and_then(Value::as_str).unwrap_or("-"),
            node.get("status").and_then(Value::as_str).unwrap_or("-"),
            node.get("label").and_then(Value::as_str).unwrap_or("-"),
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// ws (cloud workspace listing)
// ---------------------------------------------------------------------------

async fn ws_list(runtime: &CliRuntime, global: &GlobalArgs, command: CloudWsCommand) -> Result<()> {
    let _ = runtime;
    let _ = global;
    let session = session()?;
    let record = issue_and_wait(
        &session,
        "cloud",
        "workspace.list",
        json!({ "path": command.path.unwrap_or_else(|| ".".to_string()) }),
    )
    .await?;
    print_command_outcome(&record)
}

// ---------------------------------------------------------------------------
// send
// ---------------------------------------------------------------------------

async fn send(runtime: &CliRuntime, global: &GlobalArgs, command: CloudSendCommand) -> Result<()> {
    let _ = runtime;
    let _ = global;
    let session = session()?;
    let to_node = normalize_target(&command.to)?;
    let mut args = json!({"message": command.message});
    if let Some(thread) = command.thread.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        args["local_thread_id"] = json!(thread);
    }
    if let Some(title) = command.title.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        args["title"] = json!(title);
    }
    let record = issue_and_wait(&session, &to_node, "thread.message", args).await?;
    print_command_outcome(&record)
}

fn normalize_target(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw == "cloud" || raw.starts_with("device:") {
        return Ok(raw.to_string());
    }
    // A bare device id is accepted for convenience and normalized once.
    if !raw.is_empty() && !raw.contains(':') && !raw.contains('/') {
        return Ok(format!("device:{raw}"));
    }
    Err(anyhow!(
        "目标节点必须是 cloud 或 device:<id> / target must be cloud or device:<id>"
    ))
}

// ---------------------------------------------------------------------------
// watch
// ---------------------------------------------------------------------------

async fn watch(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudWatchCommand,
) -> Result<()> {
    let is_zh = zh(global);
    let _ = runtime;
    let session = session()?;
    let target = normalize_target(&command.to)?;
    if target != "cloud" {
        return Err(anyhow!(
            "{}",
            crate::locale::tr(
                if is_zh { "zh" } else { "en" },
                "watch 目前只支持云端线程（to=cloud）",
                "watch currently supports cloud threads only (to=cloud)",
            )
        ));
    }
    let url = format!(
        "{}/wunder/interlink/remote_ws?target=cloud&thread={}&token={}",
        base_url(&session)
            .replacen("http", "ws", 1),
        urlencode(&command.thread),
        urlencode(&session.token),
    );
    let (socket, _response) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|err| anyhow!("remote_ws 连接失败 / remote_ws connect failed: {err}"))?;
    let (mut sink, mut stream) = socket.split();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(command.seconds.max(1));
    println!(
        "{}",
        if is_zh {
            format!("旁观 {} （{}s 后自动退出）", command.thread, command.seconds)
        } else {
            format!("watching {} (auto-exit after {}s)", command.thread, command.seconds)
        }
    );
    loop {
        tokio::select! {
            message = stream.next() => {
                match message {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        if let Ok(frame) = serde_json::from_str::<Value>(&text) {
                            print_watch_frame(&frame);
                        }
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(err)) => return Err(anyhow!("remote_ws 断开 / remote_ws closed: {err}")),
                }
            }
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }
    let _ = sink.close().await;
    Ok(())
}

fn print_watch_frame(frame: &Value) {
    let kind = frame.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "remote_frame.snapshot" | "remote_frame.delta" => {
            let payload = frame.get("payload").cloned().unwrap_or(Value::Null);
            println!("{}", serde_json::to_string(&payload).unwrap_or_default());
        }
        "remote_frame.error" => {
            let payload = frame.get("payload").cloned().unwrap_or(Value::Null);
            eprintln!("error: {}", payload);
        }
        "remote_frame.close" => {
            let reason = frame.pointer("/payload/reason").and_then(Value::as_str).unwrap_or("");
            println!("[closed] {reason}");
        }
        _ => {}
    }
}

fn urlencode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// audit
// ---------------------------------------------------------------------------

async fn audit(runtime: &CliRuntime, global: &GlobalArgs, command: CloudAuditCommand) -> Result<()> {
    let is_zh = zh(global);
    let _ = runtime;
    let session = session()?;
    let mut path = format!(
        "/wunder/interlink/audit?limit={}",
        command.limit.unwrap_or(50).max(1).min(500)
    );
    if command.csv {
        path.push_str("&format=csv");
    }
    let url = format!("{}{}", base_url(&session), path);
    let response = http()?
        .get(url)
        .bearer_auth(&session.token)
        .send()
        .await
        .context("request failed")?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("HTTP {status}: {text}"));
    }
    if command.csv {
        print!("{text}");
        return Ok(());
    }
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let items = body
        .pointer("/data/items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!(
        "{}",
        if is_zh {
            format!("共 {} 条审计记录", items.len())
        } else {
            format!("{0} audit rows", items.len())
        }
    );
    println!("{:<16} {:<22} {:<20} {:<28}", "time", "action", "from", "to");
    for item in &items {
        println!(
            "{:<16} {:<22} {:<20} {:<28}",
            item.get("created_at").and_then(Value::as_f64).map(fmt_time).unwrap_or_default(),
            item.get("action").and_then(Value::as_str).unwrap_or("-"),
            item.get("from_node").and_then(Value::as_str).unwrap_or("-"),
            item.get("to_node").and_then(Value::as_str).unwrap_or("-"),
        );
    }
    Ok(())
}

fn fmt_time(seconds: f64) -> String {
    let secs = seconds.max(0.0) as i64;
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| format!("{secs}"))
}

// ---------------------------------------------------------------------------
// approve
// ---------------------------------------------------------------------------

async fn approve(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudApproveCommand,
) -> Result<()> {
    let is_zh = zh(global);
    let _ = runtime;
    let session = session()?;
    let decision = if command.yes { "approved" } else { "rejected" };
    let body = post_json(
        &session,
        &format!("/wunder/interlink/commands/{}/approval", command.command_id.trim()),
        json!({"decision": decision}),
    )
    .await?;
    let state = body
        .pointer("/data/state")
        .and_then(Value::as_str)
        .unwrap_or(decision);
    println!(
        "{}",
        if is_zh {
            format!("命令 {} 审批状态：{state}", command.command_id.trim())
        } else {
            format!("command {} approval state: {state}", command.command_id.trim())
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_normalized_once() {
        assert_eq!(normalize_target("cloud").ok().as_deref(), Some("cloud"));
        assert_eq!(
            normalize_target("device:abc").ok().as_deref(),
            Some("device:abc")
        );
        assert_eq!(normalize_target("abc").ok().as_deref(), Some("device:abc"));
        assert!(normalize_target("").is_err());
        assert!(normalize_target("http://x").is_err());
    }

    #[test]
    fn urls_are_percent_encoded_safely() {
        assert_eq!(urlencode("a/b c"), "a%2Fb%20c");
        assert_eq!(urlencode("tok_en-1.~"), "tok_en-1.~");
    }

    #[test]
    fn terminal_statuses_are_enumerated() {
        assert!(TERMINAL_STATUSES.contains(&"succeeded"));
        assert!(!TERMINAL_STATUSES.contains(&"running"));
    }
}
