//! `cloud` interlink subcommands (plan I10): drive the interlink ledger from
//! the terminal. Every command is a thin REST client over the user-plane
//! endpoints — the CLI adds no second execution chain; the server (for
//! `cloud` targets) or the owning node (for `device:<id>` targets) runs the
//! action, the ledger only carries idempotency, approval and the result.

use crate::args::{
    CloudApproveCommand, CloudAuditCommand, CloudSendCommand, CloudThreadCommand,
    CloudThreadShowCommand, CloudThreadsCommand, CloudWatchCommand, CloudWsAction,
    CloudWsCatCommand, CloudWsCommand, CloudWsLsCommand, CloudWsPullCommand, CloudWsPushCommand,
    GlobalArgs,
};
use crate::runtime::CliRuntime;
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue as WsHeaderValue;
use tokio_tungstenite::tungstenite::http::Request as WsRequest;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use wunder_server::cloud::{shared as cloud_shared, CloudSessionFile};

/// Bounded wait for one command to reach a terminal state.
const POLL_TIMEOUT_S: u64 = 90;
const POLL_INTERVAL: Duration = Duration::from_millis(400);

const TERMINAL_STATUSES: [&str; 4] = ["succeeded", "failed", "canceled", "timeout"];

/// The cloud node has no tunnel data plane, so its executor pins one read or
/// write at the inline cap (docs §6.4, §8.1). Checked locally so an oversized
/// push fails before the file is read into the ledger.
pub const CLOUD_FILE_CEILING: usize = 256 * 1024;
/// One preview line never exceeds this many characters: a single-line multi-kB
/// file must not dump itself into the terminal.
pub const CAT_LINE_CHARS: usize = 2000;
/// `--attach` fallback cadence: used only when the `chat_ws` subscription is
/// unavailable (older 舰体, device target, refused handshake). Every poll is a
/// ledger command, so the streaming path is the default (docs §8.1, §8.2).
const ATTACH_POLL: Duration = Duration::from_millis(1500);
/// Transcript window pulled per attach read.
const ATTACH_HISTORY_LIMIT: i64 = 50;
/// Budget for the `chat_ws` websocket handshake; on expiry `--attach` degrades.
const ATTACH_CONNECT_TIMEOUT: Duration = Duration::from_secs(6);
/// Budget for the `connect` -> `ready` round trip on the same socket.
const ATTACH_SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(6);
/// Keepalive cadence: the handler answers an `{"type":"ping"}` envelope with a
/// `pong`, which is what keeps an idle cloud turn from being reaped by a proxy.
const ATTACH_KEEPALIVE: Duration = Duration::from_secs(15);
/// Cap of the staged print queue: durable frames arrive one per committed text
/// block, and the queue is flushed every frame, so it never grows past this.
const ATTACH_PRINT_BATCH: usize = 32;
/// Transcript items the streaming projection retains; fully echoed items ahead
/// of the current turn are dropped along with their watermark entries.
const ATTACH_STREAM_MAX_ITEMS: usize = 120;
/// Request id of the `watch` subscription, used to recognise its error frames.
const ATTACH_WATCH_REQUEST_ID: &str = "cli_attach_watch";
/// Request id of the protocol `connect` frame this client sends once per socket.
const ATTACH_CONNECT_REQUEST_ID: &str = "cli_attach_connect";
/// Kinds `threads.get` serves (`load_thread_context_items` filters on these),
/// so the streamed projection shows exactly the same items as the polling path.
const ATTACH_TRANSCRIPT_KINDS: [&str; 4] = [
    "user_message",
    "assistant_message",
    "tool_message",
    "system_message",
];
/// Audit page used by `cloud status` when it counts open approvals.
const STATUS_AUDIT_PAGE: i64 = 100;

pub(crate) async fn handle_cloud_interlink(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: crate::args::CloudSubcommand,
) -> Result<()> {
    if cloud_shared().session().is_none() {
        cloud_shared()
            .adopt_shared_login()
            .await
            .context("未登录云端（先 cloud login）/ not logged in (run cloud login first)")?;
    }
    match command {
        crate::args::CloudSubcommand::Agents => {
            let data = get_json(&session()?, "/wunder/agents").await?;
            println!("{}", serde_json::to_string_pretty(&data)?);
            Ok(())
        }
        crate::args::CloudSubcommand::Devices => devices(runtime, global).await,
        crate::args::CloudSubcommand::Ws(command) => ws(runtime, global, command).await,
        crate::args::CloudSubcommand::Threads(command) => threads(runtime, global, command).await,
        crate::args::CloudSubcommand::Thread(command) => thread(runtime, global, command).await,
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
    cloud_shared().session().ok_or_else(|| {
        anyhow!("未登录云端（先 cloud login）/ not logged in (run cloud login first)")
    })
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("build http client")
}

/// Short client for the read-only probes a one-shot status line needs; it must
/// never keep `cloud status` waiting on a slow or disabled interlink plane.
fn quick_http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("build http client")
}

fn base_url(session: &CloudSessionFile) -> String {
    session.server.trim().trim_end_matches('/').to_string()
}

/// Same origin as the REST base, in websocket scheme (`http`->`ws`, `https`->`wss`).
fn ws_base(session: &CloudSessionFile) -> String {
    base_url(session).replacen("http", "ws", 1)
}

async fn get_json(session: &CloudSessionFile, path: &str) -> Result<Value> {
    let _ = session;
    let client = http()?;
    let response = cloud_shared()
        .authed_send(|live| {
            client
                .get(format!("{}{}", base_url(live), path))
                .bearer_auth(&live.token)
        })
        .await
        .context("request failed")?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(anyhow!("HTTP {status}: {}", summarize_error(&body)));
    }
    Ok(body)
}

async fn get_bytes(session: &CloudSessionFile, path: &str) -> Result<Vec<u8>> {
    let _ = session;
    let client = http()?;
    let response = cloud_shared()
        .authed_send(|live| {
            client
                .get(format!("{}{}", base_url(live), path))
                .bearer_auth(&live.token)
        })
        .await
        .context("request failed")?;
    let status = response.status();
    if !status.is_success() {
        let body: Value = response.json().await.unwrap_or(Value::Null);
        return Err(anyhow!("HTTP {status}: {}", summarize_error(&body)));
    }
    Ok(response.bytes().await.context("read blob failed")?.to_vec())
}

async fn post_json(session: &CloudSessionFile, path: &str, body: Value) -> Result<Value> {
    let _ = session;
    let client = http()?;
    let response = cloud_shared()
        .authed_send(|live| {
            client
                .post(format!("{}{}", base_url(live), path))
                .bearer_auth(&live.token)
                .json(&body)
        })
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
async fn issue_and_wait(
    session: &CloudSessionFile,
    to_node: &str,
    kind: &str,
    args: Value,
) -> Result<Value> {
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
        let record = get_json(session, &format!("/wunder/interlink/commands/{command_id}")).await?;
        let data = data_of(&record);
        let status = data.get("status").and_then(Value::as_str).unwrap_or("");
        if data.get("approval_state").and_then(Value::as_str) == Some("pending")
            || TERMINAL_STATUSES.contains(&status)
        {
            anyhow::ensure!(
                status != "succeeded" || !data["result"].is_null(),
                "RESULT_EXPIRED: command {command_id} result expired; refresh the view"
            );
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

/// Look one key up through the ledger wrappers. A tunnel result is tracked as a
/// flat summary, while the cloud executor hands the whole report payload to the
/// ledger, so the same reader has to walk both shapes (docs §6.4, §8.1).
fn deep_get<'a>(value: &'a Value, key: &str, depth: usize) -> Option<&'a Value> {
    if let Some(found) = value.get(key) {
        return Some(found);
    }
    if depth == 0 {
        return None;
    }
    value
        .as_object()
        .into_iter()
        .flat_map(|map| map.values())
        .find_map(|child| deep_get(child, key, depth - 1))
}

/// The business payload of one finished command, unwrapped from the ledger
/// envelope layers.
fn result_payload(data: &Value) -> &Value {
    let mut current = data.get("result").unwrap_or(&Value::Null);
    for _ in 0..3 {
        match current.get("result") {
            Some(inner) if inner.is_object() || inner.is_array() => current = inner,
            _ => break,
        }
    }
    current
}

fn command_status(data: &Value) -> &str {
    data.get("status").and_then(Value::as_str).unwrap_or("")
}

fn is_open_status(status: &str) -> bool {
    matches!(status, "issued" | "queued" | "acked" | "running")
}

fn print_command_outcome(record: &Value, is_zh: bool) -> Result<()> {
    let data = data_of(record);
    let status = command_status(data);
    let command_id = data.get("command_id").and_then(Value::as_str).unwrap_or("");
    match status {
        "succeeded" => {
            println!("{}", format_result(data));
            Ok(())
        }
        _ if data.get("approval_state").and_then(Value::as_str) == Some("pending") => Err(anyhow!(
            "命令 {command_id} 等待审批：在蜂巢设备面板或用 `cloud approve {command_id} --yes` 处理 / command {command_id} awaits approval: decide in the web device panel or via `cloud approve {command_id} --yes`"
        )),
        _ if is_open_status(status) => Err(anyhow!(
            "命令 {command_id} 仍在执行（{status}），稍后可重新查询 / command {command_id} is still {status}; query it again later"
        )),
        _ => Err(failure_error(data, is_zh)),
    }
}

/// A terminal failure with its documented reason spelled out, so a script sees
/// why the cloud refused instead of only the code.
fn failure_error(data: &Value, is_zh: bool) -> anyhow::Error {
    let command_id = data
        .get("command_id")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let status = command_status(data);
    let code = data
        .get("error_code")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let summary = data
        .get("error_summary")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let mut message = if is_zh {
        format!("命令 {command_id} 终态 {status}: {code} {summary}")
    } else {
        format!("command {command_id} finished {status}: {code} {summary}")
    };
    if let Some(hint) = failure_hint(code) {
        message.push('\n');
        message.push_str(hint);
    }
    anyhow!(message)
}

/// Refusals that have a local meaning the plan documents (docs §8.1, §9.2).
fn failure_hint(code: &str) -> Option<&'static str> {
    match code {
        "PAYLOAD_TOO_LARGE" => Some(
            "云端目标没有隧道数据面，单次读写上限 256 KiB；请拆分文件，或改用设备目标（--to device:<id>）/ the cloud target has no tunnel data plane: 256 KiB per read or write; split the file or target a device node (--to device:<id>)",
        ),
        "CAP_DENIED" => Some(
            "云端节点不授予 L3 能力，tool.exec / agent.spawn 只能在设备节点执行 / the cloud node grants no L3 capability: tool.exec and agent.spawn only run on a device node",
        ),
        "NOT_A_FILE" | "PATH_NOT_FOUND" | "PATH_REJECTED" => Some(
            "路径需在云端工作区内且指向普通文件；越界与目录都会在这里被拒 / the path must be a regular file inside the cloud workspace; escapes and directories are refused here",
        ),
        _ => None,
    }
}

fn format_result(data: &Value) -> String {
    let result = result_payload(data);
    match result {
        Value::Null => "(no result payload)".to_string(),
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// Both transfer shapes read the same way: a cloud target carries its bytes
/// base64-inline in the ledger result (no tunnel stream), a device target
/// assembles them into the blob endpoint (docs §6.4).
async fn fetch_command_bytes(session: &CloudSessionFile, data: &Value) -> Result<Vec<u8>> {
    if let Some(inline) = deep_get(data, "inline", 3).and_then(Value::as_str) {
        return base64::engine::general_purpose::STANDARD
            .decode(inline.as_bytes())
            .context("内联结果不是合法 base64 / inline payload is not valid base64");
    }
    let command_id = data
        .get("command_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!("命令没有 id，无法取回文件 / command id is missing, cannot fetch the blob")
        })?;
    get_bytes(
        session,
        &format!("/wunder/interlink/commands/{command_id}/blob"),
    )
    .await
}

// ---------------------------------------------------------------------------
// devices
// ---------------------------------------------------------------------------

async fn devices(runtime: &CliRuntime, global: &GlobalArgs) -> Result<()> {
    let is_zh = zh(global);
    let _ = runtime;
    let session = session()?;
    // Ask for the widest page the server allows: this command lists the fleet,
    // it has no follow-up page.
    let body = get_json(&session, "/wunder/interlink/nodes?limit=200").await?;
    let nodes = node_array(&body);
    let online = body
        .pointer("/data/online_count")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = node_total(&body, nodes.len());
    println!(
        "{}",
        if is_zh {
            format!("节点 {online}/{total} 在线")
        } else {
            format!("nodes {online}/{total} online")
        }
    );
    println!("{}", node_table(&nodes, is_zh));
    Ok(())
}

fn node_array(body: &Value) -> Vec<Value> {
    body.pointer("/data/nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// The endpoint pages now, so the row count is the page size and `total` is the
/// filtered universe. A denominator that says "50" when the account owns more
/// is worse than no count, so prefer `total` and fall back to the rows.
fn node_total(body: &Value, rows: usize) -> i64 {
    body.pointer("/data/total")
        .and_then(Value::as_i64)
        .filter(|total| *total >= 0)
        .unwrap_or(rows as i64)
}

/// Fixed-width node table; the widths are the CLI's own, not a terminal probe.
fn node_table(nodes: &[Value], is_zh: bool) -> String {
    let header = if is_zh {
        format!("{:<24} {:<8} {:<12} {:<20}", "节点", "类型", "状态", "标签")
    } else {
        format!(
            "{:<24} {:<8} {:<12} {:<20}",
            "node", "type", "status", "label"
        )
    };
    let mut out = vec![header];
    for node in nodes {
        out.push(format!(
            "{:<24} {:<8} {:<12} {:<20}",
            node.get("node_id").and_then(Value::as_str).unwrap_or("-"),
            node.get("node_type").and_then(Value::as_str).unwrap_or("-"),
            node.get("status").and_then(Value::as_str).unwrap_or("-"),
            node.get("label").and_then(Value::as_str).unwrap_or("-"),
        ));
    }
    out.join("\n")
}

// ---------------------------------------------------------------------------
// status support: the account's own view of the tunnel, without starting one
// ---------------------------------------------------------------------------

/// `cloud status` line set for the interlink plane. A one-shot command must not
/// boot the tunnel client (that only happens in the resident TUI), so everything
/// here is read from the ledger and the presence registry.
pub(crate) async fn status_interlink_lines(is_zh: bool) -> Vec<String> {
    let Ok(session) = session() else {
        return vec![interlink_status_line(None, None, is_zh)];
    };
    let nodes = match quick_json(&session, "/wunder/interlink/nodes").await {
        Ok(body) => body,
        // Unreachable, disabled or refused: the line still states the state
        // instead of leaving the tunnel slot empty.
        Err(_) => return vec![interlink_status_line(None, None, is_zh)],
    };
    let self_status = node_status_of(
        node_array(&nodes).as_slice(),
        &format!("device:{}", session.device_id),
    );
    let pending = match open_approval_count(&session).await {
        Ok(count) => Some(count),
        Err(_) => None,
    };
    vec![
        interlink_status_line(self_status.as_deref(), pending, is_zh),
        account_nodes_line(&nodes, is_zh),
        workspace_usage_line(&session, is_zh).await,
    ]
}

/// The cloud workspace usage line of `cloud status` (docs §8.2). The stats
/// endpoint answers as a bare object, so there is no envelope to unwrap here.
async fn workspace_usage_line(session: &CloudSessionFile, is_zh: bool) -> String {
    match quick_json(session, "/wunder/workspace/stats").await {
        Ok(body) => workspace_usage_text(&body, is_zh),
        Err(_) => workspace_usage_text(&Value::Null, is_zh),
    }
}

fn workspace_usage_text(body: &Value, is_zh: bool) -> String {
    let label = if is_zh {
        "云端工作区用量"
    } else {
        "cloud workspace usage"
    };
    let Some(used) = body.get("used_bytes").and_then(Value::as_u64) else {
        return if is_zh {
            format!("{label}: 未知")
        } else {
            format!("{label}: unknown")
        };
    };
    let files = body.get("files").and_then(Value::as_u64).unwrap_or(0);
    let quota = body
        .get("quota_bytes")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0);
    match (is_zh, quota) {
        (true, Some(quota)) => format!("{label}: {used} / {quota} 字节，{files} 个文件"),
        (true, None) => format!("{label}: {used} 字节，{files} 个文件"),
        (false, Some(quota)) => format!("{label}: {used} / {quota} bytes, {files} files"),
        (false, None) => format!("{label}: {used} bytes, {files} files"),
    }
}

async fn quick_json(session: &CloudSessionFile, path: &str) -> Result<Value, String> {
    let url = format!("{}{}", base_url(session), path);
    let response = quick_http()
        .map_err(|err| err.to_string())?
        .get(url)
        .bearer_auth(&session.token)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(summarize_error(&body));
    }
    Ok(body)
}

/// The status word of one node id, `None` when the account has no such node.
fn node_status_of(nodes: &[Value], node_id: &str) -> Option<String> {
    nodes
        .iter()
        .find(|node| node.get("node_id").and_then(Value::as_str) == Some(node_id))
        .and_then(|node| node.get("status").and_then(Value::as_str))
        .map(str::to_string)
}

/// The tunnel word for this device plus the open-approval count. An unknown
/// state never prints an empty slot: it says 未启用 / 未连接 explicitly.
fn interlink_status_line(node_status: Option<&str>, pending: Option<i64>, is_zh: bool) -> String {
    let state = match node_status {
        Some("online") | Some("busy") | Some("away") | Some("reconnecting") => {
            tunnel_status_word(node_status, is_zh)
        }
        Some("offline") => {
            if is_zh {
                "未连接".to_string()
            } else {
                "not connected".to_string()
            }
        }
        Some(other) => other.to_string(),
        None => {
            if is_zh {
                "未启用/未连接".to_string()
            } else {
                "disabled/not connected".to_string()
            }
        }
    };
    let mut line = if is_zh {
        format!("互通隧道: {state}")
    } else {
        format!("interlink tunnel: {state}")
    };
    let pending_part = match pending {
        Some(count) if is_zh => format!("待决审批 {count}"),
        Some(count) => format!("{count} open approvals"),
        None if is_zh => "待决审批 未知".to_string(),
        None => "open approvals unknown".to_string(),
    };
    line.push_str(" · ");
    line.push_str(pending_part.as_str());
    line
}

fn tunnel_status_word(status: Option<&str>, is_zh: bool) -> String {
    match (status, is_zh) {
        (Some("online"), true) => "已连接".to_string(),
        (Some("online"), false) => "connected".to_string(),
        (Some("busy"), true) => "已连接（忙）".to_string(),
        (Some("busy"), false) => "connected (busy)".to_string(),
        (Some("away"), true) => "已连接（离开）".to_string(),
        (Some("away"), false) => "connected (away)".to_string(),
        (Some("reconnecting"), true) => "重连中".to_string(),
        (Some("reconnecting"), false) => "reconnecting".to_string(),
        (Some(other), _) => other.to_string(),
        (None, true) => "未启用/未连接".to_string(),
        (None, false) => "disabled/not connected".to_string(),
    }
}

fn account_nodes_line(nodes: &Value, is_zh: bool) -> String {
    let list = node_array(nodes);
    let online = nodes
        .pointer("/data/online_count")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = node_total(nodes, list.len());
    if is_zh {
        format!("互通节点: 在线 {online}/{total}")
    } else {
        format!("interlink nodes: {online}/{total} online")
    }
}

/// Approvals minted but never decided, within one bounded audit page per action.
/// The user plane has no approval list endpoint, and `command.issue` rows are
/// the only ones that carry an `approval_id` before a decision exists.
async fn open_approval_count(session: &CloudSessionFile) -> Result<i64> {
    let issues = audit_items(session, "command.issue").await?;
    let decided = audit_items(session, "approval.decide").await?;
    Ok(open_approvals_of(&issues, &decided))
}

fn open_approvals_of(issues: &[Value], decided: &[Value]) -> i64 {
    let decided_ids: Vec<&str> = decided
        .iter()
        .filter_map(|row| row.get("approval_id").and_then(Value::as_str))
        .collect();
    issues
        .iter()
        .filter_map(|row| row.get("approval_id").and_then(Value::as_str))
        .filter(|approval_id| !decided_ids.contains(approval_id))
        .count() as i64
}

async fn audit_items(session: &CloudSessionFile, action: &str) -> Result<Vec<Value>> {
    let path = format!(
        "/wunder/interlink/audit?limit={}&offset=0&action={}",
        STATUS_AUDIT_PAGE,
        urlencode(action)
    );
    let body = get_json(session, path.as_str()).await?;
    Ok(body
        .pointer("/data/items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

// ---------------------------------------------------------------------------
// ws (cloud workspace)
// ---------------------------------------------------------------------------

async fn ws(runtime: &CliRuntime, global: &GlobalArgs, command: CloudWsCommand) -> Result<()> {
    match command.action {
        Some(CloudWsAction::Ls(command)) => ws_list(runtime, global, command).await,
        Some(CloudWsAction::Cat(command)) => ws_cat(runtime, global, command).await,
        Some(CloudWsAction::Pull(command)) => ws_pull(runtime, global, command).await,
        Some(CloudWsAction::Push(command)) => ws_push(runtime, global, command).await,
        // Legacy `cloud ws <PATH>`: the path was the only argument it took.
        None => {
            ws_list(
                runtime,
                global,
                CloudWsLsCommand {
                    path: command.path,
                    offset: None,
                    limit: None,
                },
            )
            .await
        }
    }
}

async fn ws_list(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudWsLsCommand,
) -> Result<()> {
    let is_zh = zh(global);
    let _ = runtime;
    let session = session()?;
    let mut args = json!({"path": command.path.unwrap_or_else(|| ".".to_string())});
    if let Some(offset) = command.offset {
        args["offset"] = json!(offset);
    }
    if let Some(limit) = command.limit {
        args["limit"] = json!(limit);
    }
    let record = issue_and_wait(&session, "cloud", "workspace.list", args).await?;
    let data = data_of(&record);
    if command_status(data) != "succeeded" {
        return print_command_outcome(&record, is_zh);
    }
    let payload = result_payload(data);
    let entries = payload
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let shown = entries.len();
    let total = payload
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or(shown as u64);
    println!(
        "{}",
        if is_zh {
            format!(
                "云端工作区 {}（共 {total} 项，本次 {shown}）",
                workspace_path_of(payload)
            )
        } else {
            format!(
                "cloud workspace {} ({total} entries, {shown} shown)",
                workspace_path_of(payload)
            )
        }
    );
    println!("{}", workspace_table(&entries, is_zh));
    Ok(())
}

fn workspace_path_of(payload: &Value) -> String {
    payload
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .to_string()
}

/// `type size updated path`, the four fields `WorkspaceEntry` always carries.
fn workspace_table(entries: &[Value], is_zh: bool) -> String {
    let header = if is_zh {
        format!(
            "{:<6} {:>10} {:<20} {:<}",
            "类型", "字节", "更新时间", "路径"
        )
    } else {
        format!(
            "{:<6} {:>10} {:<20} {:<}",
            "type", "bytes", "updated", "path"
        )
    };
    let mut rows = vec![header];
    for entry in entries {
        rows.push(format!(
            "{:<6} {:>10} {:<20} {:<}",
            entry
                .get("entry_type")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            entry.get("size").and_then(Value::as_u64).unwrap_or(0),
            entry
                .get("updated_time")
                .and_then(Value::as_str)
                .map(|value| value.chars().take(19).collect::<String>())
                .unwrap_or_else(|| "-".to_string()),
            entry.get("path").and_then(Value::as_str).unwrap_or("-"),
        ));
    }
    rows.join("\n")
}

async fn ws_cat(
    _runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudWsCatCommand,
) -> Result<()> {
    let is_zh = zh(global);
    let session = session()?;
    let record = issue_and_wait(
        &session,
        "cloud",
        "workspace.read",
        json!({"path": command.path}),
    )
    .await?;
    let data = data_of(&record);
    if command_status(data) != "succeeded" {
        return print_command_outcome(&record, is_zh);
    }
    let bytes = fetch_command_bytes(&session, data).await?;
    let preview = preview_text(&bytes, command.lines.max(1));
    print!("{}", preview.text);
    if !preview.text.ends_with('\n') {
        println!();
    }
    if preview.truncated || preview.lossy {
        eprintln!("{}", build_preview_note(&preview, bytes.len(), is_zh));
    }
    Ok(())
}

/// The bounded stdout projection of one downloaded file: at most `max_lines`
/// lines, each cut at [`CAT_LINE_CHARS`], never more than the whole buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub text: String,
    pub shown_lines: usize,
    pub total_lines: usize,
    /// True when some line was cut or lines were dropped at the tail.
    pub truncated: bool,
    /// True when the bytes were not valid UTF-8 and were replaced.
    pub lossy: bool,
}

fn preview_text(bytes: &[u8], max_lines: usize) -> Preview {
    let text = String::from_utf8_lossy(bytes);
    let lossy = std::str::from_utf8(bytes).is_err();
    let mut out = String::with_capacity(bytes.len().min(64 * 1024));
    let mut shown_lines = 0usize;
    let mut total_lines = 0usize;
    let mut truncated = false;
    for line in text.lines() {
        total_lines += 1;
        if shown_lines >= max_lines {
            truncated = true;
            continue;
        }
        shown_lines += 1;
        let mut chars = line.chars();
        let head: String = chars.by_ref().take(CAT_LINE_CHARS).collect();
        if chars.next().is_some() {
            truncated = true;
            out.push_str(head.as_str());
            out.push_str("…\n");
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Preview {
        text: out,
        shown_lines,
        total_lines,
        truncated,
        lossy,
    }
}

fn build_preview_note(preview: &Preview, bytes_len: usize, is_zh: bool) -> String {
    let mut parts = Vec::new();
    if preview.truncated {
        parts.push(if is_zh {
            format!(
                "预览截断：仅前 {} 行（共 {} 行，{} 字节）；取全文用 `cloud ws pull`",
                preview.shown_lines, preview.total_lines, bytes_len
            )
        } else {
            format!(
                "preview cut at {} of {} lines ({bytes_len} bytes); use `cloud ws pull` for the whole file",
                preview.shown_lines, preview.total_lines
            )
        });
    }
    if preview.lossy {
        parts.push(if is_zh {
            "非 UTF-8 内容，已按替换字符显示".to_string()
        } else {
            "not valid UTF-8, shown with replacement characters".to_string()
        });
    }
    parts.join(" · ")
}

async fn ws_pull(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudWsPullCommand,
) -> Result<()> {
    let is_zh = zh(global);
    // The landing path is validated before any request: a typo must fail fast
    // and never leave a half-written file behind.
    let root = runtime.workspace_root();
    let relative = match command.output.as_deref() {
        Some(output) => workspace_relative_path(root, output)?,
        // No `-o`: mirror the remote spelling inside the workspace, so
        // `ws pull docs/a.md` lands as `docs/a.md`.
        None => workspace_relative_path(
            root,
            Path::new(command.remote.trim().trim_start_matches(['/', '\\'])),
        )?,
    };
    let target = resolve_in_root(root, &relative)?;
    if target.exists() && !command.force {
        return Err(anyhow!(
            "{}",
            if is_zh {
                format!("本地已存在 {relative}；加 --force 覆盖 / {relative} already exists; pass --force to overwrite")
            } else {
                format!("{relative} already exists locally; pass --force to overwrite")
            }
        ));
    }
    let session = session()?;
    let record = issue_and_wait(
        &session,
        "cloud",
        "workspace.read",
        json!({"path": command.remote}),
    )
    .await?;
    let data = data_of(&record);
    if command_status(data) != "succeeded" {
        return print_command_outcome(&record, is_zh);
    }
    let bytes = fetch_command_bytes(&session, data).await?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "create local directory failed: {}",
                parent.to_string_lossy()
            )
        })?;
    }
    std::fs::write(&target, &bytes)
        .with_context(|| format!("write local file failed: {}", target.to_string_lossy()))?;
    println!(
        "{}",
        if is_zh {
            format!(
                "已拉取 {} → {relative}（{} 字节）",
                command.remote,
                bytes.len()
            )
        } else {
            format!(
                "pulled {} -> {relative} ({} bytes)",
                command.remote,
                bytes.len()
            )
        }
    );
    Ok(())
}

/// The file name of a remote path, `/`- or `\`-separated alike.
fn base_name(remote: &str) -> String {
    remote
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(remote)
        .trim()
        .to_string()
}

/// Normalize a CLI-supplied local path into a workspace-relative spelling: no
/// absolute path, no drive prefix, no `..`. The result is what gets printed, so
/// an absolute local path never reaches the terminal either.
fn workspace_relative_path(root: &Path, raw: &Path) -> Result<String> {
    let candidate = if raw.is_absolute() {
        // An absolute spelling is only accepted when it already points inside
        // the workspace; anything else is an escape attempt.
        match within_root(root, raw) {
            Some(relative) => PathBuf::from(relative),
            None => {
                return Err(anyhow!(
                    "落点必须在工作区内 / the landing path must stay inside the workspace: {}",
                    raw.display()
                ))
            }
        }
    } else {
        raw.to_path_buf()
    };
    let mut parts: Vec<String> = Vec::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!(
                    "落点不得越出工作区 / the landing path must not leave the workspace: {}",
                    raw.display()
                ))
            }
        }
    }
    if parts.is_empty() {
        return Err(anyhow!(
            "落点不能是工作区根本身 / the landing path must name a file inside the workspace: {}",
            raw.display()
        ));
    }
    Ok(parts.join("/"))
}

/// Join a validated workspace-relative path back onto the root. Because only
/// `Normal` components survive [`workspace_relative_path`], the result cannot
/// resolve outside the root even when the file does not exist yet.
fn resolve_in_root(root: &Path, relative: &str) -> Result<PathBuf> {
    let mut path = normalize_lexical(root);
    for part in relative
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        if part == ".." {
            return Err(anyhow!(
                "落点不得越出工作区 / the landing path must not leave the workspace: {relative}"
            ));
        }
        path.push(part);
    }
    Ok(path)
}

/// Lexical normalization only: `.` and repeated separators drop out, nothing
/// touches the filesystem, so a not-yet-created landing path still resolves.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(part) => out.push(part),
            // Only reached for an absolute spelling; a relative `..` is refused
            // before it gets here.
            Component::ParentDir => {
                out.pop();
            }
        }
    }
    out
}

/// Case-folded comparable spelling: Windows paths are case-insensitive, so
/// `-o D:\WS\a.md` still belongs to a `D:\ws` workspace.
#[cfg(windows)]
fn path_key(path: &Path) -> String {
    normalize_lexical(path)
        .to_string_lossy()
        .to_ascii_lowercase()
}

#[cfg(not(windows))]
fn path_key(path: &Path) -> String {
    normalize_lexical(path).to_string_lossy().to_string()
}

/// The workspace-relative spelling of `candidate` when it sits inside `root`,
/// compared lexically (never via `..`, never via a partial name prefix).
fn within_root(root: &Path, candidate: &Path) -> Option<String> {
    let root_key = path_key(root);
    let candidate_key = path_key(candidate);
    let tail = candidate_key.get(root_key.len()..)?;
    if !tail.starts_with('/') && !tail.starts_with('\\') {
        return None;
    }
    let relative = tail.trim_start_matches(['/', '\\']).replace('\\', "/");
    (!relative.is_empty()).then_some(relative)
}

async fn ws_push(
    _runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudWsPushCommand,
) -> Result<()> {
    let is_zh = zh(global);
    let session = session()?;
    let bytes = std::fs::read(&command.local)
        .with_context(|| format!("read local file failed: {}", command.local.display()))?;
    if bytes.len() > CLOUD_FILE_CEILING {
        return Err(anyhow!(
            "{}",
            if is_zh {
                format!(
                    "文件 {} 字节，超过云端单文件上限 {CLOUD_FILE_CEILING} 字节（云端目标没有隧道数据面）",
                    bytes.len()
                )
            } else {
                format!(
                    "{} bytes exceeds the cloud one-file ceiling of {CLOUD_FILE_CEILING} (a cloud target has no tunnel data plane)",
                    bytes.len()
                )
            }
        ));
    }
    let content = String::from_utf8(bytes.clone()).map_err(|_| {
        anyhow!(
            "{}",
            if is_zh {
                "云端写入只接受 UTF-8 文本文件 / workspace.write only accepts UTF-8 text on the cloud target"
            } else {
                "workspace.write on the cloud target only accepts UTF-8 text"
            }
        )
    })?;
    let dest = match command.dest.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => base_name(
            command
                .local
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default(),
        ),
    };
    if dest.is_empty() {
        return Err(anyhow!("云端目标路径为空 / the destination path is empty"));
    }
    // The digest and byte count are the whole summary: the file body never goes
    // to stdout, only into the command args (docs §7.1 digest rules).
    let fingerprint = fnv1a_hex(&bytes);
    let record = issue_and_wait(
        &session,
        "cloud",
        "workspace.write",
        json!({"path": dest, "content": content}),
    )
    .await?;
    if command_status(data_of(&record)) != "succeeded" {
        return print_command_outcome(&record, is_zh);
    }
    println!(
        "{}",
        if is_zh {
            format!(
                "已推送 {} → {dest}（{} 字节，指纹 {fingerprint}）",
                command.local.display(),
                bytes.len()
            )
        } else {
            format!(
                "pushed {} -> {dest} ({} bytes, digest {fingerprint})",
                command.local.display(),
                bytes.len()
            )
        }
    );
    Ok(())
}

/// A non-cryptographic FNV-1a fingerprint, just enough to tell two uploads
/// apart in a terminal line without echoing the file.
fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

// ---------------------------------------------------------------------------
// threads
// ---------------------------------------------------------------------------

async fn threads(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudThreadsCommand,
) -> Result<()> {
    let _ = runtime;
    let is_zh = zh(global);
    let session = session()?;
    let target = normalize_target(&command.to)?;
    let record = issue_and_wait(
        &session,
        &target,
        "threads.list",
        json!({"limit": command.limit}),
    )
    .await?;
    let data = data_of(&record);
    if command_status(data) != "succeeded" {
        return print_command_outcome(&record, is_zh);
    }
    let payload = result_payload(data);
    let rows = payload
        .get("threads")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        );
        return Ok(());
    }
    println!(
        "{}",
        if is_zh {
            format!("线程 {} 条", rows.len())
        } else {
            format!("{} threads", rows.len())
        }
    );
    println!("{}", thread_table(&rows, is_zh));
    if payload.get("truncated").and_then(Value::as_bool) == Some(true) {
        eprintln!(
            "{}",
            if is_zh {
                "已达投影上限，用 --limit 调整 / projection capped; raise --limit"
            } else {
                "projection capped; raise --limit"
            }
        );
    }
    Ok(())
}

fn thread_table(rows: &[Value], is_zh: bool) -> String {
    let header = if is_zh {
        format!(
            "{:<34} {:<10} {:<20} {:<}",
            "线程", "状态", "更新时间", "标题"
        )
    } else {
        format!(
            "{:<34} {:<10} {:<20} {:<}",
            "thread", "status", "updated", "title"
        )
    };
    let mut out = vec![header];
    for row in rows {
        out.push(format!(
            "{:<34} {:<10} {:<20} {:<}",
            row.get("local_thread_id")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            row.get("status").and_then(Value::as_str).unwrap_or("-"),
            row.get("updated_at")
                .and_then(Value::as_f64)
                .map(fmt_time)
                .unwrap_or_default(),
            row.get("title").and_then(Value::as_str).unwrap_or("-"),
        ));
    }
    out.join("\n")
}

async fn thread(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudThreadCommand,
) -> Result<()> {
    let command = match command.action {
        crate::args::CloudThreadAction::Show(command) => command,
        crate::args::CloudThreadAction::Cancel(command) => {
            let record = issue_and_wait(
                &session()?,
                &normalize_target(&command.to)?,
                "thread.cancel",
                json!({"local_thread_id": command.id}),
            )
            .await?;
            return print_command_outcome(&record, zh(global));
        }
        crate::args::CloudThreadAction::Answer(command) => {
            let record = issue_and_wait(&session()?, &normalize_target(&command.to)?, "thread.answer", json!({"local_thread_id": command.id, "approval_id": command.approval_id, "decision": command.decision})).await?;
            return print_command_outcome(&record, zh(global));
        }
    };
    thread_show(runtime, global, command).await
}

async fn thread_show(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudThreadShowCommand,
) -> Result<()> {
    let is_zh = zh(global);
    let _ = runtime;
    let session = session()?;
    let target = normalize_target(&command.to)?;
    let record = issue_and_wait(
        &session,
        &target,
        "threads.get",
        json!({"local_thread_id": command.id, "limit": command.limit}),
    )
    .await?;
    let data = data_of(&record);
    if command_status(data) != "succeeded" {
        return print_command_outcome(&record, is_zh);
    }
    let payload = result_payload(data);
    let items = transcript_items(payload);
    println!(
        "{}",
        if is_zh {
            format!("线程 {} · 最近 {} 条", command.id, items.len())
        } else {
            format!("thread {} · {} recent items", command.id, items.len())
        }
    );
    for line in transcript_lines(&items, command.raw, is_zh) {
        println!("{line}");
    }
    Ok(())
}

fn transcript_items(payload: &Value) -> Vec<Value> {
    payload
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// One line per transcript item, bodies cut to [`CAT_LINE_CHARS`] unless `raw`.
fn transcript_lines(items: &[Value], raw: bool, is_zh: bool) -> Vec<String> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("item");
        let status = item.get("status").and_then(Value::as_str).unwrap_or("");
        let text = item_text(item);
        let head = if is_zh {
            format!("[{kind} · {status}]")
        } else {
            format!("[{kind} · {status}]")
        };
        if text.trim().is_empty() {
            out.push(head);
            continue;
        }
        if raw {
            out.push(format!("{head}\n{text}"));
            continue;
        }
        let cut: String = text.chars().take(CAT_LINE_CHARS).collect();
        if cut.chars().count() == text.chars().count() {
            out.push(format!("{head} {cut}"));
        } else {
            out.push(format!("{head} {cut}…"));
        }
    }
    out
}

fn item_text(item: &Value) -> String {
    item.get("content")
        .and_then(Value::as_str)
        .or_else(|| item.get("text").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------------------
// send
// ---------------------------------------------------------------------------

async fn send(runtime: &CliRuntime, global: &GlobalArgs, command: CloudSendCommand) -> Result<()> {
    let is_zh = zh(global);
    let _ = runtime;
    let session = session()?;
    let to_node = normalize_target(&command.to)?;
    let mut args = json!({"message": command.message});
    if let Some(agent) = command.agent {
        args["agent"] = json!(agent);
    }
    if let Some(thread) = command
        .thread
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        args["local_thread_id"] = json!(thread);
    }
    if let Some(title) = command
        .title
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        args["title"] = json!(title);
    }
    let record = issue_and_wait(&session, &to_node, "thread.message", args).await?;
    let data = data_of(&record);
    if !command.attach {
        // Without --attach this is still the admit-and-report one-shot, so
        // existing scripts keep the same single result block.
        return print_command_outcome(&record, is_zh);
    }
    if command_status(data) != "succeeded" {
        return print_command_outcome(&record, is_zh);
    }
    let thread_id = deep_get(data, "local_thread_id", 3)
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| command.thread.clone())
        .ok_or_else(|| {
            anyhow!(
                "命令没有返回线程 id，无法接续事件流 / the command returned no thread id to stream"
            )
        })?;
    println!(
        "{}",
        if is_zh {
            format!(
                "已投递到线程 {thread_id}，流式回显 {}s（Ctrl+C 退出）",
                command.seconds
            )
        } else {
            format!(
                "delivered to thread {thread_id}, streaming {}s (Ctrl+C to stop)",
                command.seconds
            )
        }
    );
    attach_stream(&session, &to_node, &thread_id, command.seconds, is_zh).await
}

/// Echo one cloud thread's own turn as it lands (docs §8.2: 流式回显，等价于远程会话视图).
///
/// 本控云 stays on the existing 舰体 API (§8.1): one transcript read seeds the
/// watermark, then the thread's durable change feed is subscribed over
/// `GET /wunder/chat/ws` (`watch`). Only that first read touches the command
/// ledger, so a watch window no longer writes one ledger row per poll. A device
/// target owns its thread log itself (no server-side session to subscribe to),
/// and any handshake or subscribe failure degrades to the previous ledger
/// polling; both paths print the identical line format.
async fn attach_stream(
    session: &CloudSessionFile,
    to_node: &str,
    thread_id: &str,
    seconds: u64,
    is_zh: bool,
) -> Result<()> {
    let color = attach_color(
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").is_some(),
    );
    let window = Duration::from_secs(seconds.max(1));
    let deadline = std::time::Instant::now() + window;
    let mut run = AttachRun::default();
    // Order matters: the durable cursor is read *before* the transcript page, so
    // a change that lands between the two reads is either inside that page
    // (suppressed by the watermark) or ahead of the cursor (streamed).
    // Only a cloud thread lives in the 舰体's own session log, so only a cloud
    // target has a `chat_ws` session to subscribe to; a device node keeps its
    // log locally and is read through the ledger (docs §7.4).
    let mut degraded: Option<String> = None;
    let mut cursor: Option<i64> = None;
    if to_node == "cloud" {
        match attach_change_cursor(session, thread_id).await {
            Ok(Some(value)) => cursor = Some(value.max(0)),
            Ok(None) => degraded = Some(attach_reason(AttachReason::NoCursor, is_zh)),
            Err(err) => degraded = Some(err.to_string()),
        }
    }
    // Seed read: everything before the turn just delivered is history.
    let seed = attach_transcript(session, to_node, thread_id).await?;
    if let Some(cursor) = cursor {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match attach_ws_stream(
            session,
            thread_id,
            cursor,
            seed.clone(),
            color,
            is_zh,
            remaining,
            &mut run,
        )
        .await
        {
            AttachWsStream::Served => {
                return attach_window_outcome(deadline, thread_id, &mut run, is_zh)
            }
            AttachWsStream::Degraded(reason) => degraded = Some(reason),
        }
    }
    // A device target never had a socket to lose, so it says nothing here.
    if let Some(reason) = degraded {
        eprintln!("{}", attach_degraded_line(reason.as_str(), is_zh));
    }
    attach_poll_stream(
        session,
        to_node,
        thread_id,
        seed,
        color,
        is_zh,
        deadline.saturating_duration_since(std::time::Instant::now()),
        &mut run,
    )
    .await;
    attach_window_outcome(deadline, thread_id, &mut run, is_zh)
}

/// The window's closing line plus its script-visible exit status.
fn attach_window_outcome(
    deadline: std::time::Instant,
    thread_id: &str,
    run: &mut AttachRun,
    is_zh: bool,
) -> Result<()> {
    let remaining = deadline
        .saturating_duration_since(std::time::Instant::now())
        .as_secs();
    println!(
        "{}",
        attach_final_line(run.finished, thread_id, run.sections, remaining, is_zh)
    );
    if run.finished {
        Ok(())
    } else {
        Err(anyhow!(
            "{}",
            if is_zh {
                format!("线程 {thread_id} 在窗口内未完成（非零退出可脚本化判断）")
            } else {
                format!("thread {thread_id} did not finish inside the window (non-zero exit for scripts)")
            }
        ))
    }
}

/// How much of one attach window was echoed and how it ended. Shared by the
/// streaming path and the polling fallback so the handover cannot reprint.
#[derive(Debug, Default)]
struct AttachRun {
    finished: bool,
    sections: usize,
    /// Per-item count of already-echoed characters (the polling watermark).
    watermark: HashMap<String, usize>,
    /// Set once a transcript page seeded the watermark, so a degraded handover
    /// keeps history as history instead of echoing the whole thread.
    seeded: bool,
}

impl AttachRun {
    /// Echo the tails that have not been printed yet and take the new watermark.
    fn echo(&mut self, items: &[Value], is_zh: bool, color: bool) -> Vec<String> {
        let (lines, next) = attach_delta_lines(items, &self.watermark, is_zh, color);
        self.watermark = next;
        self.sections += lines.len();
        lines
    }

    /// Drop watermark entries the projection no longer tracks, so a long turn
    /// cannot grow the map without bound.
    fn forget(&mut self, keep: &HashMap<String, usize>) {
        self.watermark.retain(|key, _| keep.contains_key(key));
    }
}

/// One transcript page through the command ledger (`threads.get`).
async fn attach_transcript(
    session: &CloudSessionFile,
    to_node: &str,
    thread_id: &str,
) -> Result<Vec<Value>> {
    let record = issue_and_wait(
        session,
        to_node,
        "threads.get",
        json!({"local_thread_id": thread_id, "limit": ATTACH_HISTORY_LIMIT}),
    )
    .await?;
    let data = data_of(&record);
    if command_status(data) != "succeeded" {
        return Ok(Vec::new());
    }
    Ok(transcript_items(result_payload(data)))
}

/// Fallback stream: poll the ledger transcript and print only what arrived.
async fn attach_poll_stream(
    session: &CloudSessionFile,
    to_node: &str,
    thread_id: &str,
    seed: Vec<Value>,
    color: bool,
    is_zh: bool,
    window: Duration,
    run: &mut AttachRun,
) {
    let deadline = std::time::Instant::now() + window;
    let mut page = seed;
    loop {
        if !run.seeded && !page.is_empty() {
            run.watermark = attach_seed_watermark(page.as_slice());
            run.seeded = true;
        }
        for line in run.echo(page.as_slice(), is_zh, color) {
            println!("{line}");
        }
        if attach_turn_finished(page.as_slice()) {
            run.finished = true;
            return;
        }
        if std::time::Instant::now() >= deadline {
            return;
        }
        tokio::time::sleep(ATTACH_POLL).await;
        if std::time::Instant::now() >= deadline {
            return;
        }
        // A failed page read is not fatal here: the window keeps its watermark
        // and asks for the same transcript again on the next tick.
        page = attach_transcript(session, to_node, thread_id)
            .await
            .unwrap_or_default();
    }
}

/// What one `chat_ws` attach attempt produced.
enum AttachWsStream {
    /// The socket served the window: the turn finished or the deadline passed.
    Served,
    /// The subscription never came up; polling takes the rest of the window.
    Degraded(String),
}

type AttachSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type AttachSink = futures::stream::SplitSink<AttachSocket, WsMessage>;
type AttachStream = futures::stream::SplitStream<AttachSocket>;

/// Echo one cloud thread's durable change feed over `GET /wunder/chat/ws`.
///
/// The subscription is the handler's `watch` kind: it replays every
/// `thread_change` row after `cursor` and keeps emitting them, so the window
/// costs no command-ledger row at all (docs §8.1 keeps 本控云 on the existing
/// 舰体 API). Nothing is spawned here: the socket, the projection and the
/// capped print queue all die with this function, and the deadline branch
/// always ends the loop.
async fn attach_ws_stream(
    session: &CloudSessionFile,
    thread_id: &str,
    cursor: i64,
    seed: Vec<Value>,
    color: bool,
    is_zh: bool,
    window: Duration,
    run: &mut AttachRun,
) -> AttachWsStream {
    let request = match build_attach_ws_request(session) {
        Ok(request) => request,
        Err(reason) => return AttachWsStream::Degraded(reason),
    };
    let socket = match tokio::time::timeout(
        ATTACH_CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async(request),
    )
    .await
    {
        Err(_) => {
            return AttachWsStream::Degraded(attach_reason(AttachReason::HandshakeTimeout, is_zh))
        }
        Ok(Err(err)) => return AttachWsStream::Degraded(err.to_string()),
        Ok(Ok((socket, _))) => socket,
    };
    let (mut sink, mut stream) = socket.split();
    if let Err(reason) = attach_ws_subscribe(&mut sink, &mut stream, thread_id, cursor, is_zh).await
    {
        let _ = sink.close().await;
        return AttachWsStream::Degraded(reason);
    }
    // Live: seed the projection with the transcript page, echo the turn that was
    // just delivered, and stream everything that follows.
    let mut projection = AttachProjection::from_items(seed);
    if !projection.items().is_empty() {
        run.watermark = attach_seed_watermark(projection.items());
        run.seeded = true;
    }
    let mut pending = run.echo(projection.items(), is_zh, color);
    projection.trim_window(run);
    flush_attach_lines(&mut pending);
    if attach_turn_finished(projection.items()) {
        // The turn settled before the socket came up: nothing more will arrive.
        run.finished = true;
        let _ = sink.close().await;
        return AttachWsStream::Served;
    }
    let deadline = tokio::time::Instant::now() + window;
    let mut keepalive = tokio::time::interval(ATTACH_KEEPALIVE);
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    keepalive.tick().await; // an interval's first tick is immediate
    loop {
        let effect = tokio::select! {
            message = stream.next() => match message {
                Some(Ok(WsMessage::Text(text))) => projection.apply_frame(
                    &serde_json::from_str::<Value>(text.as_str()).unwrap_or(Value::Null),
                    ATTACH_WATCH_REQUEST_ID,
                ),
                Some(Ok(WsMessage::Close(_))) | None => {
                    AttachFrameEffect::Failed(attach_reason(AttachReason::Closed, is_zh))
                }
                Some(Ok(_)) => AttachFrameEffect::Ignored,
                Some(Err(err)) => AttachFrameEffect::Failed(err.to_string()),
            },
            _ = tokio::time::sleep_until(deadline) => break,
            _ = keepalive.tick() => {
                if sink
                    .send(WsMessage::Text(ws_envelope("ping", None, Value::Null)))
                    .await
                    .is_err()
                {
                    AttachFrameEffect::Failed(attach_reason(AttachReason::Keepalive, is_zh))
                } else {
                    AttachFrameEffect::Ignored
                }
            }
        };
        if matches!(effect, AttachFrameEffect::Applied) {
            pending = run.echo(projection.items(), is_zh, color);
            projection.trim_window(run);
        }
        if let AttachFrameEffect::Failed(reason) = effect {
            let _ = sink.close().await;
            flush_attach_lines(&mut pending);
            return AttachWsStream::Degraded(reason);
        }
        if matches!(effect, AttachFrameEffect::ReplayRequired) {
            // The durable window moved under this cursor; the polling path
            // reloads a transcript page instead of an atomic snapshot.
            let _ = sink.close().await;
            flush_attach_lines(&mut pending);
            return AttachWsStream::Degraded(attach_reason(AttachReason::Snapshot, is_zh));
        }
        flush_attach_lines(&mut pending);
        if attach_turn_finished(projection.items()) {
            run.finished = true;
            break;
        }
    }
    let _ = sink.close().await;
    AttachWsStream::Served
}

/// The thread's durable cursor, read from the 舰体 session API instead of the
/// command ledger: `GET /wunder/chat/sessions/{id}` answers
/// `data.thread_change_cursor`. `None` on a server that predates the field,
/// which is a degrade signal rather than an error.
async fn attach_change_cursor(session: &CloudSessionFile, thread_id: &str) -> Result<Option<i64>> {
    let path = format!("/wunder/chat/sessions/{}?limit=1", urlencode(thread_id));
    let body = get_json(session, path.as_str()).await?;
    Ok(body
        .pointer("/data/thread_change_cursor")
        .and_then(Value::as_i64))
}

/// `GET /wunder/chat/ws` handshake with bearer auth: the handler reads the
/// `Authorization` header before any query token (`apply_ws_auth_headers`), so
/// the session token never lands in a URL or an access log. The `wunder`
/// subprotocol is the one the handler offers.
fn build_attach_ws_request(session: &CloudSessionFile) -> Result<WsRequest<()>, String> {
    let url = format!("{}/wunder/chat/ws", ws_base(session));
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|err| err.to_string())?;
    let bearer = format!("Bearer {}", session.token.trim());
    let authorized = bearer
        .parse::<WsHeaderValue>()
        .map_err(|err| err.to_string())?;
    request.headers_mut().insert("authorization", authorized);
    let protocol = "wunder"
        .parse::<WsHeaderValue>()
        .map_err(|err| err.to_string())?;
    request
        .headers_mut()
        .insert("sec-websocket-protocol", protocol);
    Ok(request)
}

/// The inbound envelope the handler parses (`ws_helpers::WsEnvelope`):
/// `{"type", "request_id", "payload"}`. An absent payload key is omitted, which
/// the handler reads as `None`.
fn ws_envelope(kind: &str, request_id: Option<&str>, payload: Value) -> String {
    let mut map = serde_json::Map::new();
    map.insert("type".to_string(), json!(kind));
    if let Some(request_id) = request_id {
        map.insert("request_id".to_string(), json!(request_id));
    }
    if !payload.is_null() {
        map.insert("payload".to_string(), payload);
    }
    Value::Object(map).to_string()
}

/// The `connect` payload the handler negotiates (`WsConnectPayload`): protocol
/// version 1, and the client block it only logs.
fn ws_connect_payload() -> Value {
    json!({
        "protocol_version": 1,
        "client": {
            "name": "wunder-cli",
            "version": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
            "mode": "attach",
        },
    })
}

/// The `watch` payload the handler requires (`WsWatchPayload`): the session and
/// the exclusive durable cursor to replay from.
fn ws_watch_payload(thread_id: &str, cursor: i64) -> Value {
    json!({"session_id": thread_id, "after_change_seq": cursor})
}

/// Send `connect`, wait for the `ready` that carries its request id, then send
/// `watch`. `watch` has no ack of its own: the feed either starts emitting
/// events or answers with an error frame for the same request id.
async fn attach_ws_subscribe(
    sink: &mut AttachSink,
    stream: &mut AttachStream,
    thread_id: &str,
    cursor: i64,
    is_zh: bool,
) -> Result<(), String> {
    sink.send(WsMessage::Text(ws_envelope(
        "connect",
        Some(ATTACH_CONNECT_REQUEST_ID),
        ws_connect_payload(),
    )))
    .await
    .map_err(|err| err.to_string())?;
    loop {
        let frame = tokio::time::timeout(ATTACH_SUBSCRIBE_TIMEOUT, stream.next())
            .await
            .map_err(|_| attach_reason(AttachReason::NoReady, is_zh))?
            .ok_or_else(|| attach_reason(AttachReason::Closed, is_zh))?
            .map_err(|err| err.to_string())?;
        let WsMessage::Text(text) = frame else {
            continue;
        };
        let frame = serde_json::from_str::<Value>(text.as_str()).unwrap_or(Value::Null);
        match attach_ready_ack(&frame, ATTACH_CONNECT_REQUEST_ID) {
            AttachAck::Waiting => continue,
            AttachAck::Ready => break,
            AttachAck::Failed(reason) => return Err(reason),
        }
    }
    sink.send(WsMessage::Text(ws_envelope(
        "watch",
        Some(ATTACH_WATCH_REQUEST_ID),
        ws_watch_payload(thread_id, cursor),
    )))
    .await
    .map_err(|err| err.to_string())
}

/// What one frame of the subscribe phase says.
#[derive(Debug, PartialEq, Eq)]
enum AttachAck {
    Waiting,
    Ready,
    Failed(String),
}

fn attach_ready_ack(frame: &Value, request_id: &str) -> AttachAck {
    match frame.get("type").and_then(Value::as_str).unwrap_or("") {
        "ready" if frame.get("request_id").and_then(Value::as_str) == Some(request_id) => {
            AttachAck::Ready
        }
        "error" => AttachAck::Failed(attach_error_reason(frame)),
        _ => AttachAck::Waiting,
    }
}

/// One-line reason of a `{"type":"error"}` frame; `finalize_ws_error_payload`
/// puts `code` and `message` at the root of the payload.
fn attach_error_reason(frame: &Value) -> String {
    let code = frame
        .pointer("/payload/code")
        .and_then(Value::as_str)
        .unwrap_or("ERROR");
    let message = frame
        .pointer("/payload/message")
        .and_then(Value::as_str)
        .unwrap_or("request rejected");
    format!("{code} {message}")
}

/// Why the socket path gave up, in the CLI's own two languages.
#[derive(Debug, Clone, Copy)]
enum AttachReason {
    HandshakeTimeout,
    Closed,
    Keepalive,
    NoReady,
    NoCursor,
    Snapshot,
}

fn attach_reason(reason: AttachReason, is_zh: bool) -> String {
    let language = if is_zh { "zh" } else { "en" };
    let (zh, en) = match reason {
        AttachReason::HandshakeTimeout => ("chat_ws 握手超时", "chat_ws handshake timed out"),
        AttachReason::Closed => ("chat_ws 连接被服务端关闭", "chat_ws closed by the server"),
        AttachReason::Keepalive => ("chat_ws 保活写入失败", "chat_ws keepalive failed"),
        AttachReason::NoReady => ("chat_ws 未返回 ready", "chat_ws sent no ready frame"),
        AttachReason::NoCursor => (
            "舰体未提供线程变更游标（服务端过旧）",
            "the server exposes no thread change cursor (older server)",
        ),
        AttachReason::Snapshot => (
            "线程持久窗口已裁剪，需整组快照恢复",
            "the durable change window was trimmed (snapshot required)",
        ),
    };
    crate::locale::tr(language, zh, en)
}

/// The one-line notice that `--attach` is not streaming (stderr, so the echo
/// format on stdout stays machine-readable).
fn attach_degraded_line(reason: &str, is_zh: bool) -> String {
    crate::locale::tr(
        if is_zh { "zh" } else { "en" },
        format!("流式订阅不可用（{reason}），退回命令台账轮询").as_str(),
        format!("chat_ws unavailable ({reason}), falling back to ledger polling").as_str(),
    )
}

/// Print the staged echo queue in bounded chunks. The queue is flushed every
/// frame, so it only ever holds what one frame produced, capped here.
fn flush_attach_lines(pending: &mut Vec<String>) {
    while !pending.is_empty() {
        let take = pending.len().min(ATTACH_PRINT_BATCH);
        for line in pending.drain(..take) {
            println!("{line}");
        }
    }
}

/// The streaming side of `--attach`: durable `thread_change` frames fold into
/// the same transcript items `threads.get` serves, so both paths print the same
/// lines and the window stays bounded.
#[derive(Debug, Default)]
struct AttachProjection {
    /// Transcript items in arrival order, in the `threads.get` item shape.
    items: Vec<Value>,
    /// `item_key` -> position in `items`; also the bound of the maps below.
    position: HashMap<String, usize>,
    /// Exclusive canonical rollout position; display positions may be sparse.
    cursor: i64,
}

impl AttachProjection {
    fn from_items(items: Vec<Value>) -> Self {
        let mut projection = Self::default();
        for item in items {
            projection.replace_item(item);
        }
        projection
    }

    fn items(&self) -> &[Value] {
        self.items.as_slice()
    }

    /// Fold one inbound text frame. Kept free of I/O so the wire shape stays
    /// unit-tested against what `chat_ws` actually emits.
    fn apply_frame(&mut self, frame: &Value, watch_request_id: &str) -> AttachFrameEffect {
        match frame.get("type").and_then(Value::as_str).unwrap_or("") {
            "event" => {
                let payload = frame.get("payload");
                let name = payload
                    .and_then(|value| value.get("event"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                match name {
                    // Control frames end the durable feed for this cursor; the
                    // polling path reloads the window instead of replaying it.
                    "thread_snapshot_required" | "stream_overflow" => {
                        AttachFrameEffect::ReplayRequired
                    }
                    "thread_change" => {
                        self.apply_change(payload.and_then(|value| value.get("data")))
                    }
                    // `heartbeat`, `stream_started` and the ephemeral tails add
                    // nothing a terminal echo needs.
                    _ => AttachFrameEffect::Ignored,
                }
            }
            "error" => {
                if frame.get("request_id").and_then(Value::as_str) == Some(watch_request_id) {
                    AttachFrameEffect::Failed(attach_error_reason(frame))
                } else {
                    AttachFrameEffect::Ignored
                }
            }
            _ => AttachFrameEffect::Ignored,
        }
    }

    /// `data` is the durable row: `{change_type, turn_id, item_id, revision,
    /// cursor, payload}` from `workspace::try_load_thread_changes`.
    fn apply_change(&mut self, data: Option<&Value>) -> AttachFrameEffect {
        let Some(data) = data else {
            return AttachFrameEffect::Ignored;
        };
        let cursor = data["cursor"].as_i64().unwrap_or(0);
        if cursor <= self.cursor {
            return AttachFrameEffect::Ignored;
        }
        self.cursor = cursor;
        let Some(row) = data.get("payload") else {
            return AttachFrameEffect::Ignored;
        };
        match data
            .get("change_type")
            .and_then(Value::as_str)
            .unwrap_or("")
        {
            "item_upsert" => {
                let item_id = attach_str(row, "item_id")
                    .or_else(|| attach_str(data, "item_id"))
                    .unwrap_or_default();
                let kind = attach_str(row, "kind").unwrap_or_default();
                if item_id.is_empty() || !ATTACH_TRANSCRIPT_KINDS.contains(&kind.as_str()) {
                    return AttachFrameEffect::Ignored;
                }
                let status = attach_str(row, "status").unwrap_or_else(|| "completed".to_string());
                self.upsert_item(&item_id, &kind, &status, Some(attach_row_text(row)));
                AttachFrameEffect::Applied
            }
            // `turn_upsert` and unknown change types carry no echoed text.
            _ => AttachFrameEffect::Ignored,
        }
    }

    fn upsert_item(&mut self, item_id: &str, kind: &str, status: &str, text: Option<String>) {
        let index = self.slot(item_id, kind);
        if let Some(map) = self.items[index].as_object_mut() {
            map.insert("kind".to_string(), json!(kind));
            map.insert("status".to_string(), json!(status));
            if let Some(text) = text {
                map.insert("content".to_string(), json!(text));
            }
        }
    }

    /// Position of one completed transcript item in the bounded display window.
    fn slot(&mut self, item_id: &str, kind: &str) -> usize {
        if let Some(index) = self.position.get(item_id).copied() {
            return index;
        }
        let item = json!({"item_id": item_id, "kind": kind, "status": "running", "content": ""});
        let index = self.items.len();
        self.position.insert(item_id.to_string(), index);
        self.items.push(item);
        index
    }

    fn replace_item(&mut self, item: Value) {
        let key = item_key(&item, self.items.len());
        match self.position.get(&key).copied() {
            Some(index) => self.items[index] = item,
            None => {
                self.position.insert(key, self.items.len());
                self.items.push(item);
            }
        }
    }

    /// Keep the projection bounded: an item that settled and was fully echoed
    /// keeps only its identity (its length stays counted in the watermark), and
    /// the window never holds more than [`ATTACH_STREAM_MAX_ITEMS`] items.
    fn trim_window(&mut self, run: &mut AttachRun) {
        for (index, item) in self.items.iter_mut().enumerate() {
            let status = item.get("status").and_then(Value::as_str).unwrap_or("");
            if !attach_status_settled(status) {
                continue;
            }
            let key = item_key(item, index);
            let len = item_text(item).chars().count();
            if len == 0 || run.watermark.get(key.as_str()).copied().unwrap_or(0) < len {
                continue;
            }
            if let Some(map) = item.as_object_mut() {
                map.insert("content".to_string(), json!(""));
            }
        }
        if self.items.len() <= ATTACH_STREAM_MAX_ITEMS {
            return;
        }
        // History ahead of the current turn goes first; the newest user message
        // stays pinned because the finished-turn test reads it. A key that
        // leaves the window leaves the watermark, and the durable feed never
        // replays an item once it has fallen out of this window.
        let anchor = self
            .items
            .iter()
            .rposition(|item| item.get("kind").and_then(Value::as_str) == Some("user_message"));
        let start = self.items.len() - ATTACH_STREAM_MAX_ITEMS + 1;
        let mut kept: Vec<Value> = Vec::with_capacity(ATTACH_STREAM_MAX_ITEMS);
        if let Some(index) = anchor.filter(|index| *index < start) {
            kept.push(self.items[index].clone());
        }
        kept.extend_from_slice(&self.items[start..]);
        self.items = kept;
        self.position.clear();
        for (index, item) in self.items.iter().enumerate() {
            self.position.insert(item_key(item, index), index);
        }
        run.forget(&self.position);
    }
}

/// What a folded frame means for the attach window.
#[derive(Debug, PartialEq, Eq)]
enum AttachFrameEffect {
    Ignored,
    Applied,
    /// The durable feed ended at a control frame and needs a fresh window.
    ReplayRequired,
    /// The subscription was refused or the transport broke.
    Failed(String),
}

/// A JSON string field as an owned value (the projection keys on it).
fn attach_str(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|text| text.to_string())
}

/// The body of one item row: `threads.get` serves the flat item, while the
/// durable `item_upsert` frame wraps that same item under `payload`.
fn attach_row_text(row: &Value) -> String {
    row.get("payload")
        .and_then(|inner| {
            inner
                .get("content")
                .and_then(Value::as_str)
                .or_else(|| inner.get("text").and_then(Value::as_str))
        })
        .or_else(|| {
            row.get("content")
                .and_then(Value::as_str)
                .or_else(|| row.get("text").and_then(Value::as_str))
        })
        .unwrap_or_default()
        .to_string()
}

/// Statuses that will not grow any more text (the mirror of the running states
/// the finished-turn test refuses).
fn attach_status_settled(status: &str) -> bool {
    matches!(
        status,
        "completed"
            | "succeeded"
            | "failed"
            | "cancelled"
            | "canceled"
            | "rejected"
            | "stopped"
            | "interrupted"
            | "dead"
    )
}

/// Colour only for a real terminal, and never against `NO_COLOR`.
fn attach_color(stdout_tty: bool, no_color: bool) -> bool {
    stdout_tty && !no_color
}

fn attach_final_line(
    finished: bool,
    thread_id: &str,
    sections: usize,
    remaining: u64,
    is_zh: bool,
) -> String {
    if finished {
        if is_zh {
            format!("[完成] 线程 {thread_id} · 回显 {sections} 段")
        } else {
            format!("[done] thread {thread_id} · {sections} sections echoed")
        }
    } else if is_zh {
        format!("[超时] 线程 {thread_id} · 回显 {sections} 段 · 窗口剩余 {remaining}s")
    } else {
        format!("[timeout] thread {thread_id} · {sections} sections echoed · {remaining}s of window left")
    }
}

fn item_key(item: &Value, index: usize) -> String {
    item.get("item_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("item-{index}"))
}

/// Seed the watermark with everything before the newest user message: that is
/// history, and `--attach` only echoes the turn that was just delivered.
fn attach_seed_watermark(items: &[Value]) -> HashMap<String, usize> {
    let cut = items
        .iter()
        .rposition(|item| item.get("kind").and_then(Value::as_str) == Some("user_message"))
        .unwrap_or(items.len());
    items
        .iter()
        .enumerate()
        .take(cut)
        .map(|(index, item)| (item_key(item, index), item_text(item).chars().count()))
        .collect()
}

/// Print only the tails that have not been printed yet; the returned map is the
/// new per-item watermark. A running assistant item keeps growing, so its next
/// poll contributes just the new characters instead of repeating the turn.
fn attach_delta_lines(
    items: &[Value],
    watermark: &HashMap<String, usize>,
    is_zh: bool,
    color: bool,
) -> (Vec<String>, HashMap<String, usize>) {
    let mut lines = Vec::new();
    let mut next = watermark.clone();
    for (index, item) in items.iter().enumerate() {
        let key = item_key(item, index);
        let text = item_text(item);
        let seen = *next.get(key.as_str()).unwrap_or(&0);
        let total = text.chars().count();
        if total <= seen {
            next.insert(key, seen.max(total));
            continue;
        }
        let delta: String = text.chars().skip(seen).collect();
        next.insert(key, total);
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .map(|value| kind_word(value, is_zh))
            .unwrap_or_else(|| {
                if is_zh {
                    "条目".to_string()
                } else {
                    "item".to_string()
                }
            });
        if color {
            lines.push(format!("\x1b[2m<{kind}>\x1b[0m{delta}"));
        } else {
            lines.push(format!("<{kind}>{delta}"));
        }
    }
    (lines, next)
}

fn kind_word(kind: &str, is_zh: bool) -> String {
    match (kind, is_zh) {
        ("user_message", true) => "用户".to_string(),
        ("user_message", false) => "user".to_string(),
        ("assistant_message", true) => "助手".to_string(),
        ("assistant_message", false) => "assistant".to_string(),
        ("tool_message", true) => "工具".to_string(),
        ("tool_message", false) => "tool".to_string(),
        ("system_message", true) => "系统".to_string(),
        ("system_message", false) => "system".to_string(),
        (other, _) => other.to_string(),
    }
}

/// The turn is over once the newest user message has an assistant reply after
/// it and nothing in the window is still running.
fn attach_turn_finished(items: &[Value]) -> bool {
    let Some(last_user) = items
        .iter()
        .rposition(|item| item.get("kind").and_then(Value::as_str) == Some("user_message"))
    else {
        return false;
    };
    let tail = &items[last_user + 1..];
    if tail.iter().any(|item| {
        matches!(
            item.get("status").and_then(Value::as_str).unwrap_or(""),
            "running" | "queued" | "pending" | "started"
        )
    }) {
        return false;
    }
    tail.iter()
        .any(|item| item.get("kind").and_then(Value::as_str) == Some("assistant_message"))
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
    // The relay forwards a device's thread stream; the cloud node keeps its
    // threads in its own durable log, so `--to cloud` has nothing to relay
    // (docs §7.4). `cloud send --attach` subscribes to that log over `chat_ws`.
    if target == "cloud" {
        let cloud = cloud_shared();
        let page = cloud.cloud_thread_view(&command.thread).await?;
        let cursor = page
            .pointer("/page/view_seq")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        println!("{}", serde_json::to_string(&page)?);
        let mut feed = cloud.watch_cloud_thread(command.thread.clone(), cursor);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(command.seconds.max(1));
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                _ = tokio::signal::ctrl_c() => break,
                event = feed.recv() => match event {
                    Some(wunder_server::cloud::CloudThreadEvent::Change(change)) => println!("{}", serde_json::to_string(&change)?),
                    Some(wunder_server::cloud::CloudThreadEvent::Connection(state)) => eprintln!("{state}"),
                    Some(wunder_server::cloud::CloudThreadEvent::Reload) => {
                        let page = cloud.cloud_thread_view(&command.thread).await?;
                        let cursor = page.pointer("/page/view_seq").and_then(Value::as_i64).unwrap_or(0);
                        println!("{}", serde_json::to_string(&page)?);
                        feed = cloud.watch_cloud_thread(command.thread.clone(), cursor);
                    }
                    None => break,
                }
            }
        }
        return Ok(());
    }
    let url = format!(
        "{}/wunder/interlink/remote_ws?target={}&thread={}",
        ws_base(&session),
        urlencode(target.as_str()),
        urlencode(&command.thread),
    );
    let mut request = url.into_client_request()?;
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", session.token).parse()?,
    );
    request
        .headers_mut()
        .insert("sec-websocket-protocol", "wunder-interlink-remote".parse()?);
    request
        .headers_mut()
        .insert("x-wunder-device-id", session.device_id.parse()?);
    let (socket, _response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|err| anyhow!("remote_ws 连接失败 / remote_ws connect failed: {err}"))?;
    let (mut sink, mut stream) = socket.split();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(command.seconds.max(1));
    println!(
        "{}",
        if is_zh {
            format!(
                "旁观 {} （{}s 后自动退出）",
                command.thread, command.seconds
            )
        } else {
            format!(
                "watching {} (auto-exit after {}s)",
                command.thread, command.seconds
            )
        }
    );
    loop {
        tokio::select! {
            message = stream.next() => {
                match message {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        if let Ok(frame) = serde_json::from_str::<Value>(&text) {
                            print_watch_frame(&frame, is_zh);
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

fn print_watch_frame(frame: &Value, is_zh: bool) {
    if let Some((stderr, line)) = watch_frame_line(frame, is_zh) {
        if stderr {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    }
}

/// One line per `remote_ws` frame, plus whether it belongs on stderr.
///
/// The frame types are the server's own wire constants — `snapshot`, `delta`,
/// `command`, `error`, `close` (docs §7.4). This match previously spelled them
/// `remote_frame.*`, which is not what the socket sends, so every frame fell
/// through and `cloud watch` printed only its banner while looking connected.
fn watch_frame_line(frame: &Value, is_zh: bool) -> Option<(bool, String)> {
    let tag = |zh: &str, en: &str| -> String {
        if is_zh {
            zh.to_string()
        } else {
            en.to_string()
        }
    };
    match frame.get("type").and_then(Value::as_str).unwrap_or("") {
        "snapshot" => {
            let items = frame
                .pointer("/payload/items")
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or(0);
            Some((false, format!("[{}] {}", tag("快照", "snapshot"), items)))
        }
        "delta" => {
            let event = frame
                .pointer("/payload/event")
                .and_then(Value::as_str)
                .unwrap_or("change");
            let record = frame
                .pointer("/payload/data")
                .cloned()
                .unwrap_or(Value::Null);
            let text = watch_record_text(&record);
            if text.is_empty() {
                // A status-only change still moves the thread; say what it moved to.
                let status = record
                    .pointer("/payload/status")
                    .or_else(|| record.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                Some((false, format!("[{event}] status={status}")))
            } else {
                Some((false, format!("[{event}] {text}")))
            }
        }
        "command" => Some((
            false,
            format!(
                "[{}] {} {}",
                tag("命令", "command"),
                frame.get("kind").and_then(Value::as_str).unwrap_or(""),
                frame.get("status").and_then(Value::as_str).unwrap_or("")
            ),
        )),
        "error" => Some((
            true,
            format!(
                "error: {}",
                frame
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ),
        )),
        "close" => Some((
            false,
            format!(
                "[{}] {}",
                tag("已关闭", "closed"),
                frame
                    .get("reason")
                    .or_else(|| frame.pointer("/payload/reason"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
            ),
        )),
        _ => None,
    }
}

/// The body of one change record, read the way the cloud view reads it: the
/// item text can ride on the record itself or on its nested payload.
fn watch_record_text(record: &Value) -> String {
    for pointer in [
        "/content",
        "/payload/content",
        "/text",
        "/delta",
        "/payload/text",
    ] {
        if let Some(text) = record.pointer(pointer).and_then(Value::as_str) {
            if !text.is_empty() {
                return clamp_watch_line(text);
            }
        }
    }
    String::new()
}

/// One line never exceeds the same bound the file preview uses, so a long
/// answer cannot flood the terminal.
fn clamp_watch_line(text: &str) -> String {
    let mut out: String = text.chars().take(CAT_LINE_CHARS).collect();
    if text.chars().count() > CAT_LINE_CHARS {
        out.push('…');
    }
    out
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

/// Build the audit query string. The filter names stay exactly as the server
/// spells them (`action`, `device_id`, `since`, `until`), so one query works on
/// both the user plane and the 舰桥 admin surface.
fn audit_query_path(command: &CloudAuditCommand) -> String {
    let mut parts = vec![format!(
        "limit={}",
        command.limit.unwrap_or(50).max(1).min(500)
    )];
    parts.push(format!("offset={}", command.offset.unwrap_or(0).max(0)));
    if let Some(action) = command
        .action
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        parts.push(format!("action={}", urlencode(action)));
    }
    if let Some(device) = command
        .device
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        parts.push(format!("device_id={}", urlencode(device)));
    }
    if let Some(since) = command
        .since
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        parts.push(format!("since={}", urlencode(since)));
    }
    if let Some(until) = command
        .until
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        parts.push(format!("until={}", urlencode(until)));
    }
    if command.csv {
        parts.push("format=csv".to_string());
    }
    format!("/wunder/interlink/audit?{}", parts.join("&"))
}

async fn audit(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    command: CloudAuditCommand,
) -> Result<()> {
    let is_zh = zh(global);
    let _ = runtime;
    let session = session()?;
    let path = audit_query_path(&command);
    let url = format!("{}{}", base_url(&session), path);
    let response = cloud_shared()
        .authed_send(|live| {
            http()
                .expect("http client")
                .get(&url)
                .bearer_auth(&live.token)
        })
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
    println!("{}", audit_table(&items, is_zh));
    Ok(())
}

fn audit_table(items: &[Value], is_zh: bool) -> String {
    let header = if is_zh {
        format!(
            "{:<16} {:<22} {:<20} {:<28}",
            "时间", "动作", "来源", "目标"
        )
    } else {
        format!(
            "{:<16} {:<22} {:<20} {:<28}",
            "time", "action", "from", "to"
        )
    };
    let mut rows = vec![header];
    for item in items {
        rows.push(format!(
            "{:<16} {:<22} {:<20} {:<28}",
            item.get("created_at")
                .and_then(Value::as_f64)
                .map(fmt_time)
                .unwrap_or_default(),
            item.get("action").and_then(Value::as_str).unwrap_or("-"),
            item.get("from_node").and_then(Value::as_str).unwrap_or("-"),
            item.get("to_node").and_then(Value::as_str).unwrap_or("-"),
        ));
    }
    rows.join("\n")
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
        &format!(
            "/wunder/interlink/commands/{}/approval",
            command.command_id.trim()
        ),
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
            format!(
                "command {} approval state: {state}",
                command.command_id.trim()
            )
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("wunder-cli-{label}-{unique}"))
    }

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

    #[test]
    fn queued_is_open_not_closed() {
        // The engine treats `queued` as an unfinished state (docs §4.3).
        assert!(is_open_status("queued"));
        assert!(is_open_status("running"));
        assert!(!is_open_status("succeeded"));
        assert!(!is_open_status("failed"));
    }

    #[test]
    fn the_ledger_result_unwraps_both_transfer_shapes() {
        // Cloud executor: {status, result: report{status, result: payload}}.
        let cloud = json!({
            "status": "succeeded",
            "result": {
                "status": "succeeded",
                "result": {"entries": [{"path": "a.md"}]},
                "inline": "aGk="
            }
        });
        let data = json!({"command_id": "cmd_1", "status": "succeeded", "result": cloud});
        assert_eq!(
            result_payload(&data).pointer("/entries/0/path"),
            Some(&json!("a.md"))
        );
        assert!(deep_get(&data, "inline", 3).is_some());

        // Tunnel relay: one flat summary layer.
        let relay = json!({"status": "succeeded", "result": {"threads": []}, "stream_id": 7});
        let data = json!({"command_id": "cmd_2", "status": "succeeded", "result": relay});
        assert_eq!(result_payload(&data), &json!({"threads": []}));
        assert_eq!(deep_get(&data, "stream_id", 3), Some(&json!(7)));
    }

    #[test]
    fn watch_frame_lines_match_the_real_wire_types() {
        // The match this replaced spelled the types `remote_frame.*`, which is
        // not what remote_ws sends, so every frame fell through and `cloud
        // watch` printed only its banner (docs §7.4).
        let delta = json!({"type":"delta","thread_id":"th_1","seq":7,
            "payload":{"event":"item_upsert","data":{"item_id":"t1:a","kind":"assistant_message","content":"hello"}}});
        assert_eq!(
            watch_frame_line(&delta, false),
            Some((false, "[item_upsert] hello".to_string()))
        );
        let notice =
            json!({"type":"command","kind":"thread.message","status":"acked","command_id":"cmd_1"});
        assert_eq!(
            watch_frame_line(&notice, false),
            Some((false, "[command] thread.message acked".to_string()))
        );
        let close = json!({"type":"close","reason":"unsubscribed"});
        assert_eq!(
            watch_frame_line(&close, false),
            Some((false, "[closed] unsubscribed".to_string()))
        );
        let error = json!({"type":"error","code":"NODE_OFFLINE"});
        assert_eq!(
            watch_frame_line(&error, false),
            Some((true, "error: NODE_OFFLINE".to_string()))
        );
    }

    #[test]
    fn preview_cuts_lines_and_counts_them() {
        let bytes = "line1\nline2\nline3\n".as_bytes().to_vec();
        let preview = preview_text(&bytes, 2);
        assert_eq!(preview.text, "line1\nline2\n");
        assert_eq!((preview.shown_lines, preview.total_lines), (2, 3));
        assert!(preview.truncated);
        assert!(!preview.lossy);

        let whole = preview_text(&bytes, 10);
        assert!(!whole.truncated);
        assert_eq!(whole.text, "line1\nline2\nline3\n");

        let long = format!("{}\ntail", "x".repeat(CAT_LINE_CHARS + 10));
        let preview = preview_text(long.as_bytes(), 10);
        assert!(preview.truncated);
        assert!(preview.text.contains('…'));
    }

    #[test]
    fn preview_marks_non_utf8_bytes_lossy() {
        let preview = preview_text(&[0xf0, 0x28, 0x8c, 0x28], 10);
        assert!(preview.lossy);
        assert!(!preview.text.is_empty());
        let note = build_preview_note(&preview, 4, true);
        assert!(note.contains("UTF-8"), "{note}");
    }

    #[test]
    fn landing_paths_stay_inside_the_workspace() {
        let root = temp_dir("ws-root");
        let root = root.as_path();

        assert_eq!(
            workspace_relative_path(root, Path::new("docs/a.md"))
                .ok()
                .as_deref(),
            Some("docs/a.md")
        );
        assert_eq!(
            workspace_relative_path(root, Path::new("./a.md"))
                .ok()
                .as_deref(),
            Some("a.md")
        );
        assert_eq!(
            workspace_relative_path(root, &root.join("nested").join("b.txt"))
                .ok()
                .as_deref(),
            Some("nested/b.txt")
        );

        assert!(workspace_relative_path(root, Path::new("../escape.txt")).is_err());
        assert!(workspace_relative_path(root, Path::new("a/../../b.txt")).is_err());
        assert!(workspace_relative_path(root, Path::new("/etc/passwd")).is_err());
        assert!(workspace_relative_path(root, Path::new(".")).is_err());
        assert!(workspace_relative_path(root, &root.join("..").join("sibling.txt")).is_err());
        assert!(resolve_in_root(root, "../out.txt").is_err());

        let joined = resolve_in_root(root, "docs/a.md").unwrap();
        assert!(joined.starts_with(root));
        assert_eq!(joined.file_name().unwrap(), "a.md");
    }

    #[test]
    fn the_lexical_normalizer_drops_dots_and_keeps_roots() {
        let normalized = normalize_lexical(Path::new("./a/./b/c.md"));
        assert_eq!(normalized, PathBuf::from("a/b/c.md"));
        assert_eq!(path_key(Path::new("A/B")), path_key(Path::new("a/b")));
        // A sibling whose name merely starts with the root name is not inside it.
        assert_eq!(
            within_root(Path::new("/ws/a"), Path::new("/ws/ab/x.md")),
            None
        );
        assert_eq!(
            within_root(Path::new("/ws/a"), Path::new("/ws/a/x.md")).as_deref(),
            Some("x.md")
        );
    }

    #[test]
    fn the_pull_default_name_is_the_remote_leaf() {
        assert_eq!(base_name("docs/a.md"), "a.md");
        assert_eq!(base_name("docs\\sub\\b.txt"), "b.txt");
        assert_eq!(base_name("c.log"), "c.log");
    }

    #[test]
    fn the_push_digest_is_stable_and_content_free() {
        let first = fnv1a_hex(b"hello".as_slice());
        assert_eq!(first, fnv1a_hex(b"hello".as_slice()));
        assert_ne!(first, fnv1a_hex(b"hellp".as_slice()));
        assert_eq!(first.len(), 16);
    }

    #[test]
    fn the_ceiling_and_its_hint_are_documented_in_one_place() {
        assert_eq!(CLOUD_FILE_CEILING, 256 * 1024);
        let hint = failure_hint("PAYLOAD_TOO_LARGE").unwrap_or_default();
        assert!(hint.contains("256 KiB"), "{hint}");
        assert!(failure_hint("CAP_DENIED")
            .unwrap_or_default()
            .contains("L3"));
        assert!(failure_hint("WHATEVER").is_none());
    }

    #[test]
    fn the_status_usage_line_reads_the_bare_stats_object() {
        assert_eq!(
            workspace_usage_text(
                &json!({"used_bytes": 4096, "files": 7, "quota_bytes": 1048576}),
                true
            ),
            "云端工作区用量: 4096 / 1048576 字节，7 个文件"
        );
        assert_eq!(
            workspace_usage_text(&json!({"used_bytes": 4096, "files": 7}), false),
            "cloud workspace usage: 4096 bytes, 7 files"
        );
        // A missing figure never leaves the status slot blank: it says unknown.
        assert_eq!(
            workspace_usage_text(&Value::Null, true),
            "云端工作区用量: 未知"
        );
        assert_eq!(
            workspace_usage_text(&Value::Null, false),
            "cloud workspace usage: unknown"
        );
    }

    #[test]
    fn attach_delta_lines_only_print_what_arrived() {
        let items = vec![
            json!({"item_id": "t1:user", "kind": "user_message", "content": "ask", "status": "completed"}),
            json!({"item_id": "t1:answer", "kind": "assistant_message", "content": "hel", "status": "running"}),
        ];
        let (lines, mark) = attach_delta_lines(items.as_slice(), &HashMap::new(), false, false);
        assert_eq!(
            lines,
            vec!["<user>ask".to_string(), "<assistant>hel".to_string()]
        );

        // A grown assistant item contributes only its tail; finished items are silent.
        let grown = vec![
            json!({"item_id": "t1:user", "kind": "user_message", "content": "ask", "status": "completed"}),
            json!({"item_id": "t1:answer", "kind": "assistant_message", "content": "hello world", "status": "completed"}),
        ];
        let (lines, mark2) = attach_delta_lines(grown.as_slice(), &mark, false, false);
        assert_eq!(lines, vec!["<assistant>lo world".to_string()]);
        assert_eq!(mark2.get("t1:answer"), Some(&11usize));

        // Color only appears when the caller says the stream is a terminal.
        let (lines, _) = attach_delta_lines(items.as_slice(), &HashMap::new(), true, true);
        assert!(lines[0].starts_with("\x1b[2m"), "{:?}", lines[0]);
        let (plain, _) = attach_delta_lines(items.as_slice(), &HashMap::new(), true, false);
        assert!(plain[0].starts_with("<用户>"), "{:?}", plain[0]);
    }

    #[test]
    fn attach_seeds_history_before_the_delivered_turn() {
        let items = vec![
            json!({"item_id": "old:user", "kind": "user_message", "content": "earlier"}),
            json!({"item_id": "old:answer", "kind": "assistant_message", "content": "done"}),
            json!({"item_id": "new:user", "kind": "user_message", "content": "ask"}),
        ];
        let seed = attach_seed_watermark(items.as_slice());
        assert_eq!(seed.get("old:user"), Some(&7usize));
        assert_eq!(seed.get("old:answer"), Some(&4usize));
        assert!(!seed.contains_key("new:user"), "the new turn still prints");

        let (lines, _) = attach_delta_lines(items.as_slice(), &seed, false, false);
        assert_eq!(lines, vec!["<user>ask".to_string()]);
    }

    #[test]
    fn attach_waits_until_the_reply_is_settled() {
        let running = vec![
            json!({"kind": "user_message", "status": "completed"}),
            json!({"kind": "assistant_message", "status": "running"}),
        ];
        assert!(!attach_turn_finished(running.as_slice()));

        let done = vec![
            json!({"kind": "user_message", "status": "completed"}),
            json!({"kind": "assistant_message", "status": "completed"}),
        ];
        assert!(attach_turn_finished(done.as_slice()));

        // A tool call still in flight keeps the window open.
        let mid_tool = vec![
            json!({"kind": "user_message", "status": "completed"}),
            json!({"kind": "tool_message", "status": "running"}),
            json!({"kind": "assistant_message", "status": "completed"}),
        ];
        assert!(!attach_turn_finished(mid_tool.as_slice()));
        assert!(!attach_turn_finished(&[]));
    }

    #[test]
    fn attach_colour_follows_tty_and_no_color() {
        assert!(!attach_color(false, false));
        assert!(!attach_color(true, true));
        assert!(attach_color(true, false));
    }

    #[test]
    fn the_attach_final_line_states_the_terminal_case() {
        assert_eq!(
            attach_final_line(true, "th_1", 3, 0, true),
            "[完成] 线程 th_1 · 回显 3 段"
        );
        let timeout = attach_final_line(false, "th_1", 1, 12, false);
        assert!(timeout.contains("[timeout]"), "{timeout}");
        assert!(timeout.contains("12s"), "{timeout}");
    }

    #[test]
    fn transcript_lines_cut_bodies_unless_raw() {
        let items = vec![
            json!({"kind": "assistant_message", "status": "completed", "content": format!("{}tail", "x".repeat(CAT_LINE_CHARS))}),
            json!({"kind": "tool_message", "status": "completed", "content": ""}),
        ];
        let lines = transcript_lines(items.as_slice(), false, false);
        assert!(lines[0].ends_with('…'), "{}", lines[0]);
        assert_eq!(lines[1], "[tool_message · completed]");
        let raw = transcript_lines(items.as_slice(), true, false);
        assert!(raw[0].contains("tail"), "{}", raw[0]);
    }

    #[test]
    fn audit_query_paths_carry_the_server_filter_names() {
        let command = CloudAuditCommand {
            limit: Some(20),
            offset: Some(40),
            action: Some("command.issue".to_string()),
            device: Some("dev-1".to_string()),
            since: Some("2026-01-01T00:00:00Z".to_string()),
            until: Some("1700000000".to_string()),
            csv: false,
        };
        let path = audit_query_path(&command);
        assert_eq!(
            path,
            "/wunder/interlink/audit?limit=20&offset=40&action=command.issue&device_id=dev-1&since=2026-01-01T00%3A00%3A00Z&until=1700000000"
        );

        // Defaults: one bounded page, no filters, CSV rides the same query.
        let bare = CloudAuditCommand {
            limit: None,
            offset: None,
            action: None,
            device: None,
            since: None,
            until: None,
            csv: true,
        };
        assert_eq!(
            audit_query_path(&bare),
            "/wunder/interlink/audit?limit=50&offset=0&format=csv"
        );
        // The page bound stays inside what the server will serve.
        let loud = CloudAuditCommand {
            limit: Some(9999),
            ..CloudAuditCommand {
                offset: Some(-3),
                action: Some("  ".to_string()),
                device: None,
                since: None,
                until: None,
                limit: None,
                csv: false,
            }
        };
        let path = audit_query_path(&loud);
        assert!(path.contains("limit=500"), "{path}");
        assert!(path.contains("offset=0"), "{path}");
        assert!(!path.contains("action="), "{path}");
    }

    #[test]
    fn open_approvals_are_counted_against_the_decided_set() {
        let issues = vec![
            json!({"action": "command.issue", "approval_id": "apr_1"}),
            json!({"action": "command.issue", "approval_id": "apr_2"}),
            json!({"action": "command.issue", "approval_id": null}),
        ];
        let decided = vec![json!({"action": "approval.decide", "approval_id": "apr_1"})];
        assert_eq!(open_approvals_of(issues.as_slice(), decided.as_slice()), 1);
        assert_eq!(open_approvals_of(&[], &[]), 0);
    }

    #[test]
    fn the_status_line_never_leaves_an_empty_slot() {
        assert_eq!(
            interlink_status_line(Some("online"), Some(0), true),
            "互通隧道: 已连接 · 待决审批 0"
        );
        assert_eq!(
            interlink_status_line(Some("offline"), Some(2), false),
            "interlink tunnel: not connected · 2 open approvals"
        );
        // No node view at all (interlink off, or the account never connected).
        let unknown = interlink_status_line(None, None, true);
        assert!(unknown.contains("未启用/未连接"), "{unknown}");
        assert!(unknown.contains("未知"), "{unknown}");
        let nodes: Vec<Value> =
            serde_json::from_value(json!([{"node_id": "device:dev-1", "status": "busy"}])).unwrap();
        assert_eq!(
            node_status_of(nodes.as_slice(), "device:dev-1").as_deref(),
            Some("busy")
        );
        let bare: Vec<Value> =
            serde_json::from_value(json!([{"node_id": "device:dev-1"}])).unwrap();
        assert_eq!(node_status_of(bare.as_slice(), "device:dev-9"), None);
    }

    #[test]
    fn tables_stay_fixed_width() {
        let rows = vec![json!({
            "name": "a.md", "path": "docs/a.md", "entry_type": "file", "size": 12, "updated_time": "2026-01-01T00:00:00+08:00"
        })];
        let table = workspace_table(rows.as_slice(), true);
        let lines: Vec<&str> = table.split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("docs/a.md"), "{}", lines[1]);
        assert!(lines[1].contains("12"), "{}", lines[1]);

        let nodes = vec![
            json!({"node_id": "cloud", "node_type": "server", "status": "online", "label": "cloud"}),
        ];
        assert!(node_table(nodes.as_slice(), false).contains("cloud"));

        let threads = vec![json!({
            "local_thread_id": "th_1", "status": "active", "updated_at": 1.0, "title": "t"
        })];
        assert!(thread_table(threads.as_slice(), true).contains("th_1"));

        let items = vec![
            json!({"created_at": 1.0, "action": "command.issue", "from_node": "web", "to_node": "cloud"}),
        ];
        assert!(audit_table(items.as_slice(), true).contains("command.issue"));
    }
    /// The exact outbound envelope of `ws_helpers::send_ws_event`: an event name,
    /// a transport id and the data the durable feeder forwarded.
    fn ws_event_frame(request_id: &str, event: &str, data: Value) -> Value {
        json!({
            "type": "event",
            "request_id": request_id,
            "payload": {"event": event, "id": null, "data": data},
        })
    }

    /// One durable `thread_change` row as `workspace::try_load_thread_changes`
    /// builds it: `payload` is the committed row (for `item_upsert` the whole
    /// item row with its embedded payload copy).
    fn thread_change_frame(change_type: &str, item_id: &str, row: Value, cursor: i64) -> Value {
        ws_event_frame(
            ATTACH_WATCH_REQUEST_ID,
            "thread_change",
            json!({
                "change_type": change_type,
                "turn_id": "turn_1",
                "item_id": item_id,
                "revision": 1,
                "cursor": cursor,
                "payload": row,
            }),
        )
    }

    fn attach_test_session(port: u16) -> CloudSessionFile {
        serde_json::from_value(json!({
            "server": format!("http://127.0.0.1:{port}"),
            "user_id": "user_test",
            "username": "tester",
            "scope": "chat",
            "token": "test-token",
            "device_id": "dev_test",
            "device_name": "test device",
            "client": "cli",
            "logged_in_at": 1.0,
        }))
        .expect("session fixture")
    }

    /// A port nothing listens on: the handshake is refused by the stack, which is
    /// the deterministic version of an unreachable 舰体.
    fn closed_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("free port");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        port
    }

    #[test]
    fn chat_ws_frames_echo_the_same_lines_as_a_transcript_page() {
        let mut run = AttachRun::default();
        let mut projection = AttachProjection::from_items(vec![
            json!({"item_id": "old:answer", "kind": "assistant_message", "status": "completed", "content": "history"}),
            json!({"item_id": "turn_1:user", "kind": "user_message", "status": "completed", "content": "ask"}),
        ]);
        run.watermark = attach_seed_watermark(projection.items());
        let mut lines = run.echo(projection.items(), false, false);
        assert_eq!(lines, vec!["<user>ask".to_string()]);

        // The answer lands as a committed item row on the socket.
        let frame = thread_change_frame(
            "item_upsert",
            "turn_1:text-0",
            json!({"item_id": "turn_1:text-0", "kind": "assistant_message", "status": "completed",
                   "revision": 2, "payload": {"role": "assistant", "content": "hel"}}),
            8,
        );
        assert_eq!(
            projection.apply_frame(&frame, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::Applied
        );
        lines.extend(run.echo(projection.items(), false, false));
        assert_eq!(
            lines,
            vec!["<user>ask".to_string(), "<assistant>hel".to_string()]
        );

        // The same watermark over the equivalent transcript page prints the
        // same thing, so the streaming path cannot change the echo format.
        let page = vec![
            json!({"item_id": "old:answer", "kind": "assistant_message", "status": "completed", "content": "history"}),
            json!({"item_id": "turn_1:user", "kind": "user_message", "status": "completed", "content": "ask"}),
            json!({"item_id": "turn_1:text-0", "kind": "assistant_message", "status": "completed", "content": "hel"}),
        ];
        let seeded = attach_seed_watermark(page.as_slice());
        let (polled, _) = attach_delta_lines(page.as_slice(), &seeded, false, false);
        assert_eq!(
            polled,
            vec!["<user>ask".to_string(), "<assistant>hel".to_string()]
        );
        assert!(attach_turn_finished(projection.items()));
    }

    #[test]
    fn completed_items_echo_once_across_sparse_cursors_and_duplicate_replay() {
        let mut run = AttachRun::default();
        let mut projection = AttachProjection::from_items(vec![
            json!({"item_id": "turn_1:user", "kind": "user_message", "status": "completed", "content": "ask"}),
        ]);
        run.watermark = attach_seed_watermark(projection.items());
        let mut printed = run.echo(projection.items(), false, false);
        let frame = thread_change_frame(
            "item_upsert",
            "turn_1:text-0",
            json!({"item_id":"turn_1:text-0","kind":"assistant_message","status":"completed",
                "payload":{"content":"complete reply 🐣"}}),
            12,
        );
        assert_eq!(
            projection.apply_frame(&frame, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::Applied
        );
        printed.extend(run.echo(projection.items(), false, false));
        assert_eq!(
            projection.apply_frame(&frame, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::Ignored
        );
        printed.extend(run.echo(projection.items(), false, false));
        let stale = thread_change_frame(
            "item_upsert",
            "turn_1:text-0",
            json!({"item_id":"turn_1:text-0","kind":"assistant_message","status":"completed",
                "payload":{"content":"stale"}}),
            8,
        );
        assert_eq!(
            projection.apply_frame(&stale, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::Ignored
        );
        assert_eq!(printed, vec!["<user>ask", "<assistant>complete reply 🐣"]);
        assert_eq!(projection.cursor, 12);
        assert_eq!(item_text(&projection.items()[1]), "complete reply 🐣");
        assert!(run.echo(projection.items(), false, false).is_empty());
    }

    #[test]
    fn the_projection_skips_what_the_transcript_never_shows() {
        let mut projection = AttachProjection::default();
        // `threads.get` filters items by kind, so a tool call stays out of it.
        let tool = thread_change_frame(
            "item_upsert",
            "turn_1:tool-1",
            json!({"item_id": "turn_1:tool-1", "kind": "tool_call", "status": "running",
                   "payload": {"content": "run"}}),
            5,
        );
        assert_eq!(
            projection.apply_frame(&tool, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::Ignored
        );
        let reasoning = thread_change_frame(
            "item_upsert",
            "turn_1:model-0",
            json!({"item_id":"turn_1:model-0","kind":"model_call","status":"completed","payload":{"reasoning":"think"}}),
            6,
        );
        assert_eq!(
            projection.apply_frame(&reasoning, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::Ignored
        );
        let turn = thread_change_frame(
            "turn_upsert",
            "",
            json!({"turn_id": "turn_1", "status": "queued"}),
            4,
        );
        assert_eq!(
            projection.apply_frame(&turn, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::Ignored
        );
        let heartbeat = ws_event_frame(
            ATTACH_WATCH_REQUEST_ID,
            "heartbeat",
            json!({"running": true}),
        );
        assert_eq!(
            projection.apply_frame(&heartbeat, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::Ignored
        );
        // Another request's error is not ours to act on.
        let other = json!({"type": "error", "request_id": "req_other", "payload": {"code": "X", "message": "y"}});
        assert_eq!(
            projection.apply_frame(&other, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::Ignored
        );
        assert!(projection.items().is_empty());
    }

    #[test]
    fn control_and_error_frames_decide_the_degrade_path() {
        let mut projection = AttachProjection::default();
        let snapshot = ws_event_frame(
            ATTACH_WATCH_REQUEST_ID,
            "thread_snapshot_required",
            json!({}),
        );
        assert_eq!(
            projection.apply_frame(&snapshot, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::ReplayRequired
        );
        let overflow = ws_event_frame(
            ATTACH_WATCH_REQUEST_ID,
            "stream_overflow",
            json!({"session_id": "th_1", "cursor": 3, "resume_recommended": true}),
        );
        assert_eq!(
            projection.apply_frame(&overflow, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::ReplayRequired
        );
        let refused = json!({
            "type": "error",
            "request_id": ATTACH_WATCH_REQUEST_ID,
            "payload": {"code": "SESSION_NOT_FOUND", "message": "session not found", "status": 404},
        });
        assert_eq!(
            projection.apply_frame(&refused, ATTACH_WATCH_REQUEST_ID),
            AttachFrameEffect::Failed("SESSION_NOT_FOUND session not found".to_string())
        );
        // Subscribe phase: the first ready has no request id, ours does.
        assert_eq!(
            attach_ready_ack(
                &json!({"type": "ready", "payload": {"connection_id": "ws_1"}}),
                ATTACH_CONNECT_REQUEST_ID
            ),
            AttachAck::Waiting
        );
        assert_eq!(
            attach_ready_ack(
                &json!({"type": "ready", "request_id": ATTACH_CONNECT_REQUEST_ID,
                        "payload": {"protocol": {"version": 1, "min": 1, "max": 1}}}),
                ATTACH_CONNECT_REQUEST_ID
            ),
            AttachAck::Ready
        );
        assert!(matches!(
            attach_ready_ack(&refused, ATTACH_CONNECT_REQUEST_ID),
            AttachAck::Failed(_)
        ));
    }

    #[test]
    fn attach_frames_use_the_handlers_envelope_and_bearer_auth() {
        assert_eq!(
            ws_envelope("ping", None, Value::Null),
            "{\"type\":\"ping\"}"
        );
        let watch: Value = serde_json::from_str(
            ws_envelope(
                "watch",
                Some(ATTACH_WATCH_REQUEST_ID),
                ws_watch_payload("th 1", 42),
            )
            .as_str(),
        )
        .expect("watch envelope");
        assert_eq!(watch["type"], json!("watch"));
        assert_eq!(watch["request_id"], json!(ATTACH_WATCH_REQUEST_ID));
        assert_eq!(watch["payload"]["session_id"], json!("th 1"));
        assert_eq!(watch["payload"]["after_change_seq"], json!(42));
        let connect: Value = serde_json::from_str(
            ws_envelope(
                "connect",
                Some(ATTACH_CONNECT_REQUEST_ID),
                ws_connect_payload(),
            )
            .as_str(),
        )
        .expect("connect envelope");
        assert_eq!(connect["payload"]["protocol_version"], json!(1));
        assert_eq!(connect["payload"]["client"]["name"], json!("wunder-cli"));

        let request = build_attach_ws_request(&attach_test_session(9)).expect("ws request");
        assert_eq!(request.uri().to_string(), "ws://127.0.0.1:9/wunder/chat/ws");
        assert_eq!(
            request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test-token")
        );
        assert_eq!(
            request
                .headers()
                .get("sec-websocket-protocol")
                .and_then(|value| value.to_str().ok()),
            Some("wunder")
        );
    }

    #[test]
    fn a_degraded_attach_says_why_in_one_line() {
        let zh = attach_degraded_line("SESSION_NOT_FOUND session not found", true);
        assert!(
            zh.contains("退回命令台账轮询") && zh.contains("SESSION_NOT_FOUND"),
            "{zh}"
        );
        assert!(!zh.contains('\n'), "{zh}");
        assert_eq!(
            attach_degraded_line("boom", false),
            "chat_ws unavailable (boom), falling back to ledger polling"
        );
        assert!(attach_reason(AttachReason::NoCursor, false).contains("cursor"));
        assert_eq!(
            attach_reason(AttachReason::Snapshot, true),
            attach_reason(AttachReason::Snapshot, true)
        );
    }

    #[test]
    fn the_projection_window_stays_bounded_and_keeps_the_turn_anchor() {
        let mut run = AttachRun::default();
        let mut projection = AttachProjection::default();
        let anchor = thread_change_frame(
            "item_upsert",
            "turn_1:user",
            json!({"item_id": "turn_1:user", "kind": "user_message", "status": "completed",
                   "payload": {"content": "ask"}}),
            1,
        );
        projection.apply_frame(&anchor, ATTACH_WATCH_REQUEST_ID);
        run.watermark = attach_seed_watermark(projection.items());
        for index in 0..(ATTACH_STREAM_MAX_ITEMS + 60) {
            let frame = thread_change_frame(
                "item_upsert",
                format!("turn_1:text-{index}").as_str(),
                json!({"item_id":format!("turn_1:text-{index}"),"kind":"assistant_message",
                    "status":"completed","payload":{"content":"x"}}),
                index as i64 + 2,
            );
            projection.apply_frame(&frame, ATTACH_WATCH_REQUEST_ID);
            let lines = run.echo(projection.items(), false, false);
            projection.trim_window(&mut run);
            assert!(!lines.is_empty() || index > ATTACH_STREAM_MAX_ITEMS);
        }
        assert!(projection.items().len() <= ATTACH_STREAM_MAX_ITEMS);
        assert!(
            projection
                .items()
                .iter()
                .any(|item| item.get("kind").and_then(Value::as_str) == Some("user_message")),
            "the turn anchor stays so the window can still end"
        );
        assert!(run.watermark.len() <= projection.items().len());
    }

    #[tokio::test]
    async fn a_failed_handshake_degrades_to_polling() {
        let session = attach_test_session(closed_port());
        let mut run = AttachRun::default();
        let outcome = attach_ws_stream(
            &session,
            "th_1",
            7,
            vec![json!({"item_id": "turn_1:user", "kind": "user_message", "status": "completed", "content": "ask"})],
            false,
            false,
            Duration::from_secs(30),
            &mut run,
        )
        .await;
        let reason = match outcome {
            AttachWsStream::Degraded(reason) => reason,
            AttachWsStream::Served => panic!("a refused handshake must degrade, not serve"),
        };
        assert!(!reason.is_empty(), "{reason}");
        // Nothing was echoed yet: the polling path still owns the whole window.
        assert_eq!(run.sections, 0);
        assert!(!run.seeded);
        assert!(!run.finished);
    }
}
