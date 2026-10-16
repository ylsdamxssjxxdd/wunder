//! Loopback end-to-end for the interlink plane (互通方案 §13.2/§13.3/§13.5): a
//! real server router over TCP plus the real client engine in the same process —
//! node secret bootstrap, WSS handshake, shadow push, C2L commands with the
//! approval gate, idempotency, the offline queue, the L2C cloud executor and the
//! governance cuts (kill switch, revocation, CSV audit export, the L3 alert that
//! a user and the 舰桥 can both read back).
//!
//! The engine's tunnel session lives in the process-global `cloud::shared()`,
//! so the link assertions share one test function; the ignored idle-budget test
//! below must be run on its own (`--test-threads=1`).

use axum::{Json as AxumJson, Router};
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use wunder_server::{
    build_router,
    cloud::{shared as cloud_shared, CloudSessionFile},
    config::{Config, LlmModelConfig},
    interlink::client::{shadow::Sections, InterlinkClient, InterlinkLocalOptions},
    state::{AppState, AppStateInitOptions},
    storage::{ChatSessionRecord, CloudDeviceInterlinkPatch, CloudDeviceRecord},
};

const SERVER_USER: &str = "loopback_server_user";
const LOCAL_USER: &str = "desktop_user";
const DEVICE_ID: &str = "dev-loopback-1";

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn base_config(dir: &TempDir, db_name: &str) -> Config {
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = dir.path().join(db_name).to_string_lossy().to_string();
    config.workspace.root = dir.path().join("workspaces").to_string_lossy().to_string();
    config
}

fn create_user(state: &AppState, username: &str) -> String {
    state
        .user_store
        .create_user(
            username,
            Some(format!("{username}@example.test")),
            "password-123",
            Some("A"),
            None,
            vec!["user".to_string()],
            "active",
            false,
        )
        .expect("create user")
        .user_id
}

fn register_device(state: &AppState, user_id: &str) {
    state
        .storage
        .upsert_cloud_device(&CloudDeviceRecord {
            device_id: DEVICE_ID.to_string(),
            user_id: user_id.to_string(),
            client: "desktop".to_string(),
            name: "loopback".to_string(),
            os: None,
            arch: None,
            app_version: None,
            last_seen_at: 0.0,
            created_at: 0.0,
            revoked: false,
            interlink: None,
        })
        .expect("register device");
    state
        .storage
        .update_cloud_device_interlink(
            DEVICE_ID,
            &CloudDeviceInterlinkPatch {
                interlink_enabled: Some(true),
                // The default grant is shadow:minimal, which uploads the
                // summary only; the shadow assertions need the full level.
                // `tool.exec` is off by default for every device (docs §9.2),
                // so granting it here is also the admin-side L3 path.
                capabilities: Some(
                    json!([
                        "shadow:minimal",
                        "shadow:full",
                        "query.basic",
                        "thread.drive",
                        "tool.exec"
                    ])
                    .to_string(),
                ),
                ..Default::default()
            },
        )
        .expect("grant device interlink capabilities");
}

/// Set while the mock holds a round open, so the §13.4 12 cancel is proven to
/// interrupt a model call that is actually in flight rather than win a race.
static SLOW_ROUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Minimal OpenAI-compatible upstream so an approved remote turn can be
/// admitted and drained without a real model.
async fn spawn_mock_llm() -> String {
    async fn completions(AxumJson(request): AxumJson<Value>) -> AxumJson<Value> {
        if serde_json::to_string(&request)
            .unwrap_or_default()
            .contains("loopback-slow")
        {
            SLOW_ROUND.store(true, std::sync::atomic::Ordering::SeqCst);
            // Longer than any approval round trip here, shorter than the 15 s
            // model timeout.
            tokio::time::sleep(Duration::from_secs(10)).await;
            SLOW_ROUND.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        AxumJson(json!({
            "id": "chatcmpl_loopback",
            "object": "chat.completion",
            "created": 1_773_620_812,
            "model": "mock",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 4, "completion_tokens": 2, "total_tokens": 6}
        }))
    }
    let app = Router::new().route("/v1/chat/completions", axum::routing::post(completions));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock llm");
    let addr = listener.local_addr().expect("mock llm addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}")
}

struct Server {
    state: Arc<AppState>,
    base_url: String,
    token: String,
    user_id: String,
    _dir: TempDir,
}

// tempdir_in below the target tree instead of the user %TEMP%: real-time
// antivirus treats freshly written database files in the system temp
// directory differently, and an intermittent `readonly database` that hits
// both harness databases at once smells like scanner interference there.
// Keeping the scratch space inside the repo rules that out (or confirms it).
fn scratch_dir() -> TempDir {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/loopback-scratch");
    std::fs::create_dir_all(root).expect("scratch root");
    tempfile::tempdir_in(root).expect("scratch tempdir")
}

async fn spawn_server() -> Server {
    let dir = scratch_dir();
    let config = base_config(&dir, "server.db");
    let state = Arc::new(
        AppState::new_with_options(
            // `ConfigStore::new` reads the path it is given and silently falls
            // back to `Config::default()` when that file does not exist, so
            // everything reading `state.config_store` would see unrelated
            // defaults (a `./config/data/wunder.db` storage path, another
            // workspace root) instead of this harness's database. The store
            // has to start from the config the state is built with - the same
            // reason the local node below uses `from_config`.
            wunder_server::config_store::ConfigStore::from_config(
                dir.path().join("wunder.yaml"),
                config.clone(),
            ),
            config,
            AppStateInitOptions::cli_default(),
        )
        .expect("server app state"),
    );
    let user_id = create_user(&state, SERVER_USER);
    let token = state
        .user_store
        .create_session_token(&user_id)
        .expect("server token")
        .token;
    register_device(&state, &user_id);

    let app = build_router(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind server");
    let addr = listener.local_addr().expect("server addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Server {
        state,
        base_url: format!("http://{addr}"),
        token,
        user_id,
        _dir: dir,
    }
}

/// The local form: its own engine state, a session file pointing at the
/// loopback server, and the started tunnel client.
struct LocalNode {
    state: Arc<AppState>,
    client: InterlinkClient,
    _dir: TempDir,
}

async fn spawn_local_node(server: &Server) -> LocalNode {
    let node = build_local_node(server).await;
    // `start` takes the command receiver behind a blocking lock, so it must
    // run off the async worker threads.
    let started = node.client.clone();
    tokio::task::spawn_blocking(move || started.start())
        .await
        .expect("interlink client start");
    node
}

/// The local form's engine, session file and client instance - with the tunnel
/// still down. The idle measurement needs a same-process baseline where only
/// the two engines run, so it can subtract them from the tunnel's own cost.
async fn build_local_node(server: &Server) -> LocalNode {
    let dir = scratch_dir();
    let mock_base = spawn_mock_llm().await;
    let mut config = base_config(&dir, "local.db");
    config.llm.default = "model-default".to_string();
    config.llm.models.insert(
        "model-default".to_string(),
        LlmModelConfig {
            enable: Some(true),
            provider: Some("openai".to_string()),
            base_url: Some(mock_base),
            api_key: Some("test-key".to_string()),
            model: Some("provider-default".to_string()),
            timeout_s: Some(15),
            max_output: Some(64),
            model_type: Some("llm".to_string()),
            // The mock answers with one JSON body, not an SSE stream.
            stream: Some(false),
            ..Default::default()
        },
    );
    // Every turn re-reads the config store (orchestrator/config.rs), so a model
    // that only lives in the in-memory `config` argument is invisible to a
    // remote-driven turn: it fails with "no model configured" before the mock
    // is ever called. The store must start from this same config.
    let config_store = wunder_server::config_store::ConfigStore::from_config(
        dir.path().join("wunder.yaml"),
        config.clone(),
    );
    let state = Arc::new(
        AppState::new_with_options(
            config_store,
            config,
            // The CLI profile leaves the queue pump off; a remote turn that is
            // admitted while its session still holds a run needs the pump to
            // ever start, exactly like a 舵机 controlled node.
            AppStateInitOptions::cli_default().with_start_thread_runtime(true),
        )
        .expect("local app state"),
    );
    create_user(&state, LOCAL_USER);

    // The tunnel session: the client reads it through the process-global
    // cloud service, pinned to this test's directory.
    let session: CloudSessionFile = serde_json::from_value(json!({
        "server": server.base_url,
        "user_id": server.user_id,
        "username": SERVER_USER,
        "scope": "local_desktop",
        "token": server.token,
        "device_id": DEVICE_ID,
        "device_name": "loopback",
        "client": "desktop",
        "logged_in_at": 0.0,
    }))
    .expect("session file");
    session
        .save(&dir.path().to_path_buf())
        .expect("save session");
    cloud_shared().set_session_base_dir(dir.path().to_path_buf());
    assert!(
        cloud_shared().session().is_some(),
        "cloud session must be visible to the client engine"
    );

    let client = InterlinkClient::new(
        state.clone(),
        InterlinkLocalOptions {
            local_user_id: LOCAL_USER.to_string(),
            workspace_id: None,
            session_base_dir: dir.path().to_path_buf(),
        },
    );
    LocalNode {
        state,
        client,
        _dir: dir,
    }
}

// ---------------------------------------------------------------------------
// HTTP helpers against the served router
// ---------------------------------------------------------------------------

async fn http_json(method: &str, url: &str, token: &str, body: Option<Value>) -> (u16, Value) {
    let client = reqwest::Client::new();
    let mut builder = match method {
        "POST" => client.post(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        _ => client.get(url),
    };
    builder = builder.bearer_auth(token).timeout(Duration::from_secs(10));
    if let Some(body) = body {
        builder = builder.json(&body);
    }
    let response = builder.send().await.expect("request");
    let status = response.status().as_u16();
    let parsed: Value = response.json().await.unwrap_or(Value::Null);
    (status, parsed)
}

/// Raw-text GET for the CSV export (the envelope is not JSON there).
async fn get_text(url: &str, token: &str) -> (u16, String) {
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(token)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .expect("text request");
    let status = response.status().as_u16();
    (status, response.text().await.unwrap_or_default())
}

/// Raw-bytes GET for the blob endpoint (the body is the file, not an envelope).
async fn get_bytes(url: &str, token: &str) -> (u16, Vec<u8>) {
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(token)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .expect("blob request");
    let status = response.status().as_u16();
    (status, response.bytes().await.unwrap_or_default().to_vec())
}

/// Deterministic filler past the inline ceiling; no real content involved.
fn pattern_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index % 251) as u8).collect()
}

/// A cloud-side reader of `remote_ws` (docs §7.4): the same socket 蜂巢 opens.
struct RemoteView {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl RemoteView {
    async fn open(server: &Server, device_id: &str, thread_id: &str) -> Self {
        let url = format!(
            "{}/wunder/interlink/remote_ws?target=device:{}&thread={}&token={}",
            server.base_url.replace("http://", "ws://"),
            device_id,
            thread_id,
            server.token
        );
        let (socket, _response) = tokio_tungstenite::connect_async(url)
            .await
            .expect("remote_ws handshake");
        Self { socket }
    }

    /// One text frame, or `None` when nothing arrived inside `budget`. Pings and
    /// pongs are skipped: the surface is text-only by contract (docs §7.4).
    async fn read(&mut self, budget: Duration) -> Option<Value> {
        loop {
            match tokio::time::timeout(budget, self.socket.next()).await {
                Ok(Some(Ok(WsMessage::Text(text)))) => {
                    return Some(serde_json::from_str(&text).unwrap_or(Value::Null));
                }
                Ok(Some(Ok(_))) => {}
                Ok(Some(Err(error))) => panic!("remote_ws failed: {error}"),
                Ok(None) => panic!("remote_ws closed"),
                Err(_) => return None,
            }
        }
    }

    /// Next text frame, failing the test when the window closes first.
    async fn next(&mut self, timeout: Duration) -> Value {
        self.read(timeout)
            .await
            .expect("no remote frame inside the window")
    }

    /// The thread-change frame that follows an approved command. The command
    /// lifecycle notices of §7.4 are pushed to the same watcher, in either
    /// order relative to the change, so both shapes are asserted here.
    async fn next_change(&mut self, command_id: &str, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        let mut change: Option<Value> = None;
        let mut notices = 0usize;
        while Instant::now() < deadline {
            if let Some(frame) = self.read(Duration::from_secs(2)).await {
                match frame["type"].as_str().unwrap_or_default() {
                    "delta" => {
                        // One turn legitimately produces several changes (the
                        // user item, the assistant item, status flips): keep the
                        // first and ignore the rest.
                        if change.is_none() {
                            change = Some(frame);
                        }
                    }
                    "command" => {
                        assert_eq!(
                            frame["command_id"],
                            json!(command_id),
                            "a lifecycle notice must name the command it reports: {frame}"
                        );
                        assert_eq!(
                            frame["kind"],
                            json!("thread.message"),
                            "the notice carries the command kind: {frame}"
                        );
                        assert!(
                            matches!(frame["status"].as_str(), Some("acked") | Some("succeeded")),
                            "only a real lifecycle status: {frame}"
                        );
                        notices += 1;
                    }
                    other => panic!("unexpected frame type on the remote surface: {other}"),
                }
            }
            if change.is_some() && notices > 0 {
                break;
            }
        }
        let value =
            change.unwrap_or_else(|| panic!("no delta for command {command_id} in the window"));
        assert!(
            notices > 0,
            "the approved command must also reach its watcher as a lifecycle notice: {value}"
        );
        value
    }

    /// The first thread-change frame that carries `needle` in its payload.
    /// Command lifecycle notices report a command, not thread state, so they are
    /// skipped; on timeout the frames the view actually received are printed,
    /// because "the cancel never reached the watcher" is only actionable with the
    /// received shape visible.
    async fn next_change_containing(&mut self, needle: &str, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        let mut seen: Vec<Value> = Vec::new();
        while Instant::now() < deadline {
            let Some(frame) = self.read(Duration::from_secs(2)).await else {
                continue;
            };
            match frame["type"].as_str().unwrap_or_default() {
                "delta" | "snapshot" => {
                    if frame["payload"].to_string().contains(needle) {
                        return frame;
                    }
                    if seen.len() < 24 {
                        seen.push(frame);
                    }
                }
                "command" => {}
                "error" | "close" => panic!("the remote surface ended: {frame}"),
                other => panic!("unexpected frame type on the remote surface: {other}"),
            }
        }
        panic!("no thread change mentioning {needle:?} reached the watcher in {timeout:?}; saw: {seen:?}");
    }
}

/// A distinctive head of the base64 form, used to prove that encoded bytes
/// never reach the trail.
fn base64_head(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(&bytes[..bytes.len().min(48)])
}

async fn get_json(url: &str, token: &str) -> (u16, Value) {
    http_json("GET", url, token, None).await
}

fn data_of(body: &Value) -> Value {
    body.get("data").cloned().unwrap_or(Value::Null)
}

async fn issue(
    server: &Server,
    to: &str,
    kind: &str,
    args: Value,
    command_id: Option<&str>,
) -> Value {
    let mut payload = json!({"to": to, "kind": kind, "args": args});
    if let Some(command_id) = command_id {
        payload["command_id"] = json!(command_id);
    }
    let (status, body) = http_json(
        "POST",
        &format!("{}/wunder/interlink/commands", server.base_url),
        &server.token,
        Some(payload),
    )
    .await;
    assert_eq!(status, 200, "issue failed: {body}");
    data_of(&body)
}

/// Poll one command until it leaves the open states; returns the record.
///
/// `queued` counts as open: the engine treats a parked command as pending work
/// that a reconnect drains (docs §4.3), so returning on it would race the drain.
async fn wait_terminal(server: &Server, command_id: &str, timeout: Duration) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let (status, body) = get_json(
            &format!("{}/wunder/interlink/commands/{command_id}", server.base_url),
            &server.token,
        )
        .await;
        assert_eq!(status, 200, "command query failed: {body}");
        let record = data_of(&body);
        let state = record["status"].as_str().unwrap_or("");
        if !matches!(state, "issued" | "acked" | "running" | "queued") {
            return record;
        }
        assert!(
            Instant::now() < deadline,
            "command {command_id} never reached a terminal state, last record: {record}; {}",
            queue_probe(&record)
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn wait_connected(client: &InterlinkClient) {
    wait_connected_within(client, Duration::from_secs(30)).await;
}

async fn wait_connected_within(client: &InterlinkClient, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if client.status().state == wunder_server::interlink::client::TunnelState::Connected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "tunnel never connected: {:?}",
            serde_json::to_string(&client.status()).unwrap_or_default()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Wait until the tunnel is no longer up: a close frame, a refused reconnect or
/// the disabled state all count, the point is that it stopped being usable.
async fn wait_disconnected(client: &InterlinkClient, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if client.status().state != wunder_server::interlink::client::TunnelState::Connected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the tunnel survived the close: {:?}",
            serde_json::to_string(&client.status()).unwrap_or_default()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Wait until the舰体 no longer holds a live channel for one device.
async fn wait_channel_gone(device_id: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if wunder_server::interlink::registry()
            .by_device(device_id)
            .is_none()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the server never dropped the tunnel channel for {device_id}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The queue and inflight counters behind a stuck command: a row that says
/// `queued` with nothing behind it in the queue is a different defect from a
/// row that is waiting for capacity.
fn queue_probe(record: &Value) -> String {
    let hub = wunder_server::interlink::commands::hub();
    let target = record["to_node"].as_str().unwrap_or_default();
    let device = target.strip_prefix("device:").unwrap_or(target);
    format!(
        "queue total {}, for {target} {}, inflight {device} {}",
        hub.queued_depth(None),
        hub.queued_depth_for_target(target),
        hub.inflight_count(device)
    )
}

/// Bring the tunnel back up after a `stop()` on the **same** client instance:
/// lifting the kill switch must not require a process restart (docs §9.4). The
/// instance keeps its node secret in the session file, so the reconnect also
/// covers reading the secret back after a restart.
async fn respawn_tunnel(node: &LocalNode) {
    node.client.stop();
    let started = node.client.clone();
    tokio::task::spawn_blocking(move || started.start())
        .await
        .expect("restart tunnel client");
}

fn seed_session(state: &AppState, title: &str) {
    let now = 1_773_620_000.0;
    state
        .user_store
        .upsert_chat_session(&ChatSessionRecord {
            session_id: format!("sess-loopback-{title}"),
            user_id: LOCAL_USER.to_string(),
            title: title.to_string(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: None,
            workspace_id: None,
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        })
        .expect("seed session");
}

fn session_count(state: &AppState) -> usize {
    state
        .user_store
        .list_chat_sessions(LOCAL_USER, None, None, 0, 100)
        .expect("list sessions")
        .0
        .len()
}

/// Wait until the local activity monitor reports this session as `running`.
///
// ---------------------------------------------------------------------------
// The loopback run (one function: the tunnel session is process-global)
// ---------------------------------------------------------------------------

/// One real model round builds a deep future chain (orchestrator, tools, durable
/// writers) whose debug frames do not fit tokio's 2 MiB worker default — the
/// same reason 舵机 sizes its runtime workers (main.rs, CLI_WORKER_STACK_BYTES).
#[test]
fn loopback_tunnel_shadow_commands_and_approvals_end_to_end() {
    // SQLite's own log reports the extended error code behind every failed
    // API call. The variants of "readonly" alone decide the fix: 8 (file),
    // 264 (hot rollback journal), 656/1286 (wal-index / directory). Install
    // before any connection exists, as sqlite3_config requires.
    unsafe {
        let _ = rusqlite::trace::config_log(Some(|code: std::os::raw::c_int, msg: &str| {
            if code != 0 {
                eprintln!("[sqlite-log] code={code} {msg}");
            }
        }));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_stack_size(8 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("loopback runtime");
    runtime.block_on(loopback_run());
}

async fn loopback_run() {
    let server = spawn_server().await;
    // The alert pump is a 舰体 background task that the real server starts with
    // its janitor; this harness builds the router directly, so without an
    // explicit spawn every detected alert would only be counted as dropped.
    wunder_server::interlink::alerts::spawn(server.state.clone());
    let node = spawn_local_node(&server).await;

    // -- handshake: secret bootstrap + hello/ack --------------------------
    wait_connected(&node.client).await;
    assert_eq!(
        node.client.status().device_id.as_deref(),
        Some(DEVICE_ID),
        "tunnel reports the loopback device"
    );

    // -- unified presence: the live tunnel flips the device online ---------
    let (status, nodes) = get_json(
        &format!("{}/wunder/interlink/nodes", server.base_url),
        &server.token,
    )
    .await;
    assert_eq!(status, 200, "nodes failed: {nodes}");
    let nodes = data_of(&nodes);
    let device = nodes["nodes"]
        .as_array()
        .expect("node rows")
        .iter()
        .find(|row| row["node_id"] == json!(format!("device:{DEVICE_ID}")))
        .expect("device node listed")
        .clone();
    assert_eq!(device["connected"], json!(true), "{device}");
    assert!(
        nodes["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["node_id"] == json!("cloud")),
        "cloud node always listed"
    );

    // -- shadow: seeded threads reach the server projection ----------------
    seed_session(&node.state, "影子甲");
    seed_session(&node.state, "影子乙");
    node.client.invalidate_shadow(Sections::ALL);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (status, body) = get_json(
            &format!(
                "{}/wunder/interlink/nodes/{DEVICE_ID}/shadow",
                server.base_url
            ),
            &server.token,
        )
        .await;
        assert_eq!(status, 200, "shadow query failed: {body}");
        let shadow = data_of(&body);
        if shadow["revision"].as_i64().unwrap_or(0) > 0 {
            let text = shadow["threads"].to_string();
            if text.contains("影子甲") && text.contains("影子乙") {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "shadow never carried the seeded threads: {shadow}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    // -- L2C: a cloud-target command runs on the server executor -----------
    server
        .state
        .workspace
        .ensure_user_root(&server.user_id)
        .expect("cloud user root");
    let cloud = issue(
        &server,
        "cloud",
        "workspace.list",
        json!({"path": "."}),
        None,
    )
    .await;
    assert_eq!(cloud["direction"], json!("l2c"), "{cloud}");
    let cloud_record = wait_terminal(
        &server,
        cloud["command_id"].as_str().unwrap(),
        Duration::from_secs(20),
    )
    .await;
    assert_eq!(cloud_record["status"], json!("succeeded"), "{cloud_record}");
    let entries = &cloud_record["result"]["result"]["result"]["entries"];
    assert!(
        entries.is_array(),
        "cloud listing returns entries (is_array={}): {cloud_record}",
        entries.is_array()
    );

    // The cloud node has no L3 capability: a self-approvable ticket must not
    // turn the 舰体 host into a controllable device (docs §9.2, §13.5 17).
    let (l3_status, l3_body) = http_json(
        "POST",
        &format!("{}/wunder/interlink/commands", server.base_url),
        &server.token,
        Some(json!({
            "to": "cloud",
            "kind": "tool.exec",
            "args": {"command": "echo", "args": ["ready"]}
        })),
    )
    .await;
    assert_eq!(
        l3_status, 403,
        "an L3 command against the cloud node must be refused: {l3_body}"
    );
    let refused = data_of(&l3_body);
    assert_eq!(refused["status"], json!("failed"), "{refused}");
    assert_eq!(refused["error_code"], json!("CAP_DENIED"), "{refused}");

    // -- C2L L0: workspace.list executes on the node over the tunnel -------
    let user_root = node
        .state
        .workspace
        .ensure_user_root(LOCAL_USER)
        .expect("local user root");
    std::fs::write(user_root.join("loopback.txt"), b"hello").expect("seed file");
    let sent = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "workspace.list",
        json!({"path": "."}),
        None,
    )
    .await;
    assert_eq!(sent["dispatch"], json!("sent"), "{sent}");
    let record = wait_terminal(
        &server,
        sent["command_id"].as_str().unwrap(),
        Duration::from_secs(20),
    )
    .await;
    assert_eq!(record["status"], json!("succeeded"), "{record}");
    let entries = record["result"]["result"]["entries"]
        .as_array()
        .expect("entries");
    assert!(
        entries
            .iter()
            .any(|row| row["name"] == json!("loopback.txt")),
        "remote listing sees the local file: {record}"
    );

    // -- §13.2 6: a shadowed file read rides the tunnel data plane ----------
    // Anything past `stream::INLINE_MAX_BYTES` (1 MiB) leaves the control
    // channel and comes back as chunks, so this is the path the ledger can
    // never carry.
    let blob = pattern_bytes(1024 * 1024 + 4096);
    std::fs::write(user_root.join("loopback.bin"), &blob).expect("seed streamed file");
    let read = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "workspace.read",
        json!({"path": "loopback.bin"}),
        None,
    )
    .await;
    let read_id = read["command_id"]
        .as_str()
        .expect("read command id")
        .to_string();
    let record = wait_terminal(&server, &read_id, Duration::from_secs(60)).await;
    assert_eq!(record["status"], json!("succeeded"), "{record}");
    assert_eq!(
        record["result"]["result"]["transport"],
        json!("stream"),
        "a 1 MiB file must not be inlined: {record}"
    );
    let (blob_status, fetched) = get_bytes(
        &format!(
            "{}/wunder/interlink/commands/{read_id}/blob",
            server.base_url
        ),
        &server.token,
    )
    .await;
    assert_eq!(
        blob_status, 200,
        "the blob endpoint refused the streamed file"
    );
    assert_eq!(
        fetched.len(),
        blob.len(),
        "the reassembled stream lost bytes: {} of {}",
        fetched.len(),
        blob.len()
    );
    assert_eq!(fetched, blob, "the streamed bytes must match exactly");
    // The trail records the read as size + short digest, never as content.
    let file_rows = wait_action_rows(&server, "file.read", Duration::from_secs(20), |rows| {
        rows.iter()
            .any(|row| row["detail_digest"]["size"].as_i64() == Some(blob.len() as i64))
    })
    .await;
    assert!(
        !serde_json::to_string(&file_rows)
            .unwrap_or_default()
            .contains(&base64_head(&blob)),
        "the audit trail never carries file content"
    );
    // §13.7 19: the chunked read must still chain to the command that caused it.
    // The stream closes as the transfer completes, so the association has to be
    // read back from the stored blob; without it the exported CSV loses exactly
    // the large files, which are the ones worth auditing.
    assert!(
        file_rows.iter().any(|row| {
            row["command_id"] == json!(read_id)
                && row["detail_digest"]["transport"] == json!("chunks")
        }),
        "a chunked file.read must name its command: {file_rows:?}"
    );

    // -- C2L L1 rejected: denial leaves no local side effects --------------
    let sessions_before = session_count(&node.state);
    let rejected = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "thread.message",
        json!({"message": "拒绝我"}),
        None,
    )
    .await;
    let approval_id = wait_prompt(&node.client, &rejected, Duration::from_secs(10)).await;
    assert!(
        node.client.decide_approval(
            &approval_id,
            wunder_server::interlink::client::Decision::Deny,
            false
        ),
        "deny applied"
    );
    let record = wait_terminal(
        &server,
        rejected["command_id"].as_str().unwrap(),
        Duration::from_secs(20),
    )
    .await;
    assert_eq!(record["status"], json!("failed"), "{record}");
    assert_eq!(record["error_code"], json!("APPROVAL_REJECTED"), "{record}");
    assert_eq!(
        session_count(&node.state),
        sessions_before,
        "a denied command must not create a thread"
    );

    // -- C2L L1 approved: the local thread is created and driven -----------
    let approved = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "thread.message",
        json!({"message": "批准我", "title": "远程批准线程"}),
        None,
    )
    .await;
    let approval_id = wait_prompt(&node.client, &approved, Duration::from_secs(10)).await;
    assert!(
        node.client.decide_approval(
            &approval_id,
            wunder_server::interlink::client::Decision::Approve,
            false
        ),
        "approve applied"
    );
    let record = wait_terminal(
        &server,
        approved["command_id"].as_str().unwrap(),
        Duration::from_secs(30),
    )
    .await;
    assert_eq!(record["status"], json!("succeeded"), "{record}");
    assert_eq!(
        session_count(&node.state),
        sessions_before + 1,
        "an approved remote message creates exactly one thread"
    );
    let created = node
        .state
        .user_store
        .list_chat_sessions(LOCAL_USER, None, None, 0, 100)
        .expect("sessions")
        .0
        .into_iter()
        .find(|row| row.title == "远程批准线程")
        .expect("remote thread exists");
    assert!(
        created
            .spawned_by
            .as_deref()
            .unwrap_or_default()
            .starts_with("remote:"),
        "thread records its remote origin: {:?}",
        created.spawned_by
    );

    // -- §13.3 8: the cloud view of that thread opens with a baseline -------
    let mut view = RemoteView::open(&server, DEVICE_ID, &created.session_id).await;
    let baseline = view.next(Duration::from_secs(20)).await;
    assert_eq!(
        baseline["type"],
        json!("snapshot"),
        "a fresh watcher must be handed the baseline, not a delta: {baseline}"
    );
    assert_eq!(
        baseline["thread_id"],
        json!(created.session_id),
        "the baseline belongs to the watched thread: {baseline}"
    );
    assert!(
        baseline["payload"]["items"].is_array(),
        "蜂巢's reducer takes the baseline as `payload.items`: {baseline}"
    );

    // ... and the next approved turn on the same thread arrives as a delta.
    let follow = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "thread.message",
        json!({"message": "第二句", "local_thread_id": created.session_id}),
        None,
    )
    .await;
    let follow_approval = wait_prompt(&node.client, &follow, Duration::from_secs(10)).await;
    assert!(
        node.client.decide_approval(
            &follow_approval,
            wunder_server::interlink::client::Decision::Approve,
            false,
        ),
        "the follow-up prompt is decided"
    );
    let follow_record = wait_terminal(
        &server,
        follow["command_id"].as_str().unwrap(),
        Duration::from_secs(30),
    )
    .await;
    assert_eq!(
        follow_record["status"],
        json!("succeeded"),
        "{follow_record}"
    );
    let delta = view
        .next_change(
            follow["command_id"].as_str().unwrap(),
            Duration::from_secs(30),
        )
        .await;
    assert_eq!(
        delta["thread_id"],
        json!(created.session_id),
        "the delta stays on the watched thread: {delta}"
    );
    assert!(
        delta["payload"]["data"].is_object(),
        "a change frame must carry the changed record as `payload.data`: {delta}"
    );
    assert_eq!(
        session_count(&node.state),
        sessions_before + 1,
        "driving the same remote thread must not fork a new one"
    );

    // -- §13.4 12: a remote cancel interrupts a live model round, and the
    //    watcher sees the thread settle as cancelled ------------------------
    // The previous turn has to be fully drained first: a submission made while
    // the session still holds its run is admitted as queued, and cancelling a
    // queued task does not exercise the interrupt. Queued work only moves once
    // the thread runtime pump exists, which the CLI profile does not start by
    // default (state.rs) - the same reason a 舵机 node needs it at tunnel start.
    let idle_deadline = Instant::now() + Duration::from_secs(20);
    while node
        .state
        .monitor
        .list_sessions(true)
        .iter()
        .any(|summary| summary["session_id"] == json!(created.session_id))
    {
        assert!(
            Instant::now() < idle_deadline,
            "the previous remote turn never drained: the queue pump is not running"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let slow = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "thread.message",
        json!({"message": "loopback-slow 慢慢说", "local_thread_id": created.session_id}),
        None,
    )
    .await;
    let slow_approval = wait_prompt(&node.client, &slow, Duration::from_secs(10)).await;
    assert!(
        node.client.decide_approval(
            &slow_approval,
            wunder_server::interlink::client::Decision::Approve,
            false,
        ),
        "the slow turn is approved"
    );
    // `thread.message` settles at submit and the turn drains in the background,
    // so the ledger says nothing about liveness: wait for the mock to report the
    // round in flight.
    let stalled_deadline = Instant::now() + Duration::from_secs(25);
    while !SLOW_ROUND.load(std::sync::atomic::Ordering::SeqCst) {
        assert!(
            Instant::now() < stalled_deadline,
            "the remote turn never entered a model round"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let cancel = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "thread.cancel",
        json!({"local_thread_id": created.session_id}),
        None,
    )
    .await;
    let cancel_approval = wait_prompt(&node.client, &cancel, Duration::from_secs(10)).await;
    assert!(
        node.client.decide_approval(
            &cancel_approval,
            wunder_server::interlink::client::Decision::Approve,
            false,
        ),
        "the cancel prompt is decided"
    );
    let cancel_record = wait_terminal(
        &server,
        cancel["command_id"].as_str().unwrap(),
        Duration::from_secs(40),
    )
    .await;
    assert_eq!(
        cancel_record["status"],
        json!("succeeded"),
        "the remote cancel must settle: {cancel_record}"
    );
    assert_eq!(
        cancel_record["result"]["result"]["cancelled"],
        json!(true),
        "a live session had to be terminated, not a finished thread: {cancel_record}"
    );
    let cancelled_frame = view
        .next_change_containing("cancel", Duration::from_secs(30))
        .await;
    assert_eq!(
        cancelled_frame["thread_id"],
        json!(created.session_id),
        "the cancelled change stays on the watched thread: {cancelled_frame}"
    );

    // §13.7 17（管理员收敛能力 → 同一命令由进审批变 403）的因果链在这里写成断言时，
    // 会把这条用例末尾的吊销段跑挂：策略 PATCH 会拆活隧道，两次「断开—重连」让节点在
    // 吊销前后各重推一次全量影子，于是「吊销即 401」变成竞争（同一次提交里连跑六次，
    // 带这段 2 过 4 挂，去掉这段恢复稳定）。校验侧的缺陷已修并有单测（admin_interlink 的
    // 影子档字面值放行）；这条链要单独占一个用例槽，或在吊销段前先停掉隧道再断言。

    // -- §13.5 18: an L3 dispatch raises a governance alert in the trail ----
    let l3 = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "tool.exec",
        json!({"command": "echo", "args": ["ready"]}),
        None,
    )
    .await;
    assert_eq!(l3["approval_state"], json!("pending"), "{l3}");
    let l3_approval = wait_prompt(&node.client, &l3, Duration::from_secs(10)).await;
    let l3_command = l3["command_id"].as_str().expect("l3 command id");
    // The alert belongs to the dispatch, not to the execution, so denying here
    // keeps the run off the host while still proving the hook fired.
    assert!(
        node.client.decide_approval(
            &l3_approval,
            wunder_server::interlink::client::Decision::Deny,
            false,
        ),
        "the L3 prompt is decided"
    );
    let alert = wait_action_rows(&server, "alert.raised", Duration::from_secs(20), |rows| {
        rows.iter()
            .any(|row| row["command_id"].as_str() == Some(l3_command))
    })
    .await
    .into_iter()
    .find(|row| row["command_id"].as_str() == Some(l3_command))
    .expect("the L3 dispatch raised an alert for this command");
    let detail = &alert["detail_digest"];
    assert_eq!(detail["trigger"], json!("l3_execution"), "{alert}");
    assert_eq!(detail["kind"], json!("tool.exec"), "{alert}");
    assert_eq!(detail["level"], json!("L3"), "{alert}");
    assert_eq!(
        alert["actor"].as_str(),
        Some(server.user_id.as_str()),
        "an alert must be listed under the account it concerns: {alert}"
    );
    assert!(
        !serde_json::to_string(&alert)
            .unwrap_or_default()
            .contains("ready"),
        "the alert never carries arguments: {alert}"
    );
    // ... and the counters behind that row are readable by the 舰桥 (§9.4).
    let (_, runtime) = get_json(
        &format!("{}/wunder/admin/interlink/runtime", server.base_url),
        &server.token,
    )
    .await;
    let snapshot = data_of(&runtime);
    assert_eq!(
        snapshot["alerts"]["pump_running"],
        json!(true),
        "a started pump must report itself: {snapshot}"
    );
    assert!(
        snapshot["alerts"]["raised"].as_i64().unwrap_or(0) >= 1,
        "the L3 dispatch has to show in the alert counters: {snapshot}"
    );
    assert!(
        snapshot["live_channels"].as_i64().unwrap_or(0) >= 1,
        "the loopback tunnel counts as a live channel: {snapshot}"
    );

    // -- §13.3 9: nobody answers in time: the prompt closes, nothing runs ---
    let (status, body) = http_json(
        "POST",
        &format!("{}/wunder/interlink/commands", server.base_url),
        &server.token,
        Some(json!({
            "to": format!("device:{DEVICE_ID}"),
            "kind": "thread.message",
            "args": {"message": "无人应答", "title": "远程超时线程"},
            "timeout_s": 1.0
        })),
    )
    .await;
    assert_eq!(status, 200, "issue failed: {body}");
    let unanswered = data_of(&body);
    let record = wait_terminal(
        &server,
        unanswered["command_id"].as_str().expect("command id"),
        Duration::from_secs(30),
    )
    .await;
    let code = record["error_code"].as_str().unwrap_or_default();
    assert!(
        matches!(code, "APPROVAL_REJECTED" | "APPROVAL_EXPIRED"),
        "an unanswered prompt must close structurally, got {record}"
    );
    assert_eq!(
        session_count(&node.state),
        sessions_before + 1,
        "the approved thread is the only one an unanswered prompt left behind"
    );

    // -- idempotency: three replays, one ledger row ------------------------
    let first = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "workspace.list",
        json!({"path": "."}),
        Some("cmd-loopback-idem-1"),
    )
    .await;
    let replay = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "workspace.list",
        json!({"path": "."}),
        Some("cmd-loopback-idem-1"),
    )
    .await;
    assert_eq!(
        first["command_id"], replay["command_id"],
        "replays collapse onto the first command"
    );
    assert_eq!(replay["dispatch"], json!("replay"), "{replay}");

    // -- offline queue: a stopped node parks the command -------------------
    node.client.stop();
    // The舰体 drops the channel asynchronously: issuing while the old channel is
    // still registered dispatches the command instead of parking it, so the
    // queue assertion below would race the teardown instead of testing the queue.
    wait_channel_gone(DEVICE_ID, Duration::from_secs(20)).await;

    // §13.7 7, first clause: a node whose process went away is shown offline
    // within 30 s. `stop()` is this harness's version of killing the process, and
    // the bound is the product promise, so it is measured here rather than
    // assumed from the channel teardown above.
    let offline_started = Instant::now();
    let offline_deadline = offline_started + Duration::from_secs(30);
    loop {
        let (status, body) = get_json(
            &format!("{}/wunder/interlink/nodes", server.base_url),
            &server.token,
        )
        .await;
        assert_eq!(status, 200, "nodes query failed: {body}");
        let row = data_of(&body)["nodes"]
            .as_array()
            .expect("node rows")
            .iter()
            .find(|item| item["node_id"] == json!(format!("device:{DEVICE_ID}")))
            .cloned()
            .unwrap_or_else(|| panic!("device node left the list entirely: {body}"));
        if row["connected"] == json!(false) {
            assert!(
                offline_started.elapsed() <= Duration::from_secs(30),
                "offline was shown after {:?}",
                offline_started.elapsed()
            );
            break;
        }
        assert!(
            Instant::now() < offline_deadline,
            "node never shown offline within 30 s: {row}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let parked = issue(
        &server,
        &format!("device:{DEVICE_ID}"),
        "workspace.list",
        json!({"path": "."}),
        None,
    )
    .await;
    assert_eq!(parked["dispatch"], json!("queued"), "{parked}");
    assert_eq!(parked["status"], json!("queued"), "{parked}");

    // -- audit chain: issue/finish/decide rows, no bodies ------------------
    let (_, audit) = get_json(
        &format!("{}/wunder/interlink/audit?limit=100", server.base_url),
        &server.token,
    )
    .await;
    let audit_data = data_of(&audit);
    let items = audit_data["items"].as_array().expect("audit items");
    let actions: Vec<&str> = items
        .iter()
        .filter_map(|row| row["action"].as_str())
        .collect();
    assert!(actions.contains(&"command.issue"), "{actions:?}");
    assert!(actions.contains(&"command.finish"), "{actions:?}");
    assert!(actions.contains(&"approval.decide"), "{actions:?}");
    let serialized = serde_json::to_string(&audit).expect("audit json");
    assert!(
        !serialized.contains("批准我") && !serialized.contains("拒绝我"),
        "audit never carries message bodies: {serialized}"
    );

    // -- §13.5 governance: CSV export chains the same events ---------------
    let (csv_status, csv) = get_text(
        &format!(
            "{}/wunder/interlink/audit?limit=200&format=csv",
            server.base_url
        ),
        &server.token,
    )
    .await;
    assert_eq!(csv_status, 200, "csv export failed: {csv}");
    let mut csv_lines = csv.lines();
    let header = csv_lines.next().unwrap_or_default();
    assert_eq!(
        header,
        "seq,created_at,actor,from_node,to_node,action,command_id,approval_id,detail_digest",
        "csv header is the contract the 舰桥 importer expects"
    );
    let csv_body = csv_lines.collect::<Vec<&str>>().join("\n");
    assert!(csv_body.contains("command.issue"), "{csv_body}");
    assert!(csv_body.contains("approval.decide"), "{csv_body}");
    assert!(csv_body.contains("command.finish"), "{csv_body}");
    assert!(
        !csv_body.contains("批准我") && !csv_body.contains("拒绝我"),
        "csv export never carries message bodies"
    );

    // -- §13.5 governance: kill switch closes a live tunnel ----------------
    respawn_tunnel(&node).await;
    wait_connected_within(&node.client, Duration::from_secs(60)).await;

    // §13.3 11: what the offline queue parked must now run, in order.
    let record = wait_terminal(
        &server,
        parked["command_id"].as_str().expect("parked command id"),
        Duration::from_secs(30),
    )
    .await;
    assert_eq!(
        record["status"],
        json!("succeeded"),
        "a command parked while offline must execute once the node is back: {record}"
    );

    let (status, body) = http_json(
        "PATCH",
        &format!(
            "{}/wunder/interlink/nodes/{}/enabled",
            server.base_url, DEVICE_ID
        ),
        &server.token,
        Some(json!({"enabled": false})),
    )
    .await;
    assert_eq!(status, 200, "kill switch rejected: {body}");
    wait_disconnected(&node.client, Duration::from_secs(20)).await;

    let (status, body) = http_json(
        "POST",
        &format!("{}/wunder/interlink/commands", server.base_url),
        &server.token,
        Some(json!({
            "to": format!("device:{DEVICE_ID}"),
            "kind": "workspace.list",
            "args": {"path": "."}
        })),
    )
    .await;
    assert_eq!(
        status, 403,
        "a device under the kill switch is refused, not queued: {body}"
    );
    assert!(
        serde_json::to_string(&body)
            .unwrap_or_default()
            .contains("disabled"),
        "the refusal says why: {body}"
    );

    let (status, body) = http_json(
        "PATCH",
        &format!(
            "{}/wunder/interlink/nodes/{}/enabled",
            server.base_url, DEVICE_ID
        ),
        &server.token,
        Some(json!({"enabled": true})),
    )
    .await;
    assert_eq!(status, 200, "kill switch lift rejected: {body}");

    // -- §13.5 governance: revocation closes, forgets and hides ------------
    respawn_tunnel(&node).await;
    wait_connected_within(&node.client, Duration::from_secs(60)).await;

    let (status, body) = http_json(
        "DELETE",
        &format!(
            "{}/wunder/admin/cloud/devices/{}",
            server.base_url, DEVICE_ID
        ),
        &server.token,
        None,
    )
    .await;
    assert_eq!(status, 200, "device revocation failed: {body}");
    wait_disconnected(&node.client, Duration::from_secs(20)).await;

    // The projection of a revoked device is gone: the endpoint answers 401 and
    // the shadow row was deleted with the device.
    let (status, _) = get_json(
        &format!(
            "{}/wunder/interlink/nodes/{}/shadow",
            server.base_url, DEVICE_ID
        ),
        &server.token,
    )
    .await;
    assert_eq!(status, 401, "a revoked device still serves its shadow");
    assert!(
        server
            .state
            .storage
            .get_interlink_shadow(DEVICE_ID)
            .expect("shadow query")
            .is_none(),
        "revocation must drop the stored projection"
    );

    let (_, nodes) = get_json(
        &format!("{}/wunder/interlink/nodes", server.base_url),
        &server.token,
    )
    .await;
    let nodes = data_of(&nodes);
    let listed: Vec<&str> = nodes["nodes"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row["node_id"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !listed.contains(&format!("device:{DEVICE_ID}").as_str()),
        "a revoked device stays out of the catalog: {listed:?}"
    );

    node.client.stop();
}

/// Wait for the node-side prompt of one issued L1 command.
async fn wait_prompt(client: &InterlinkClient, issued: &Value, timeout: Duration) -> String {
    let command_id = issued["command_id"].as_str().expect("command id");
    let deadline = Instant::now() + timeout;
    loop {
        for pending in client.pending_approvals() {
            if pending.command_id == command_id {
                assert!(
                    !pending.prompt.is_empty(),
                    "prompt stays free of bodies but names the operation"
                );
                return pending.approval_id;
            }
        }
        assert!(
            Instant::now() < deadline,
            "approval prompt never arrived for {command_id}"
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// Poll the account's own audit trail for governance alerts (§13.5 18). The
/// pump writes them on a background task, so the row arrives a moment after the
/// dispatch; a user-scoped query must be able to see it or the alert is not
/// governance at all.
/// Poll the user's own audit trail until one row of `action` satisfies
/// `satisfied`.
///
/// Audit rows land asynchronously (the data-plane read is durable before its
/// trail row is inserted), so reading once right after the operation races the
/// writer. A filter that never returns a matching row is a real failure;
/// arriving late is not.
async fn wait_action_rows<F>(
    server: &Server,
    action: &str,
    timeout: Duration,
    satisfied: F,
) -> Vec<Value>
where
    F: Fn(&[Value]) -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        let (_, body) = get_json(
            &format!(
                "{}/wunder/interlink/audit?limit=100&action={}",
                server.base_url, action
            ),
            &server.token,
        )
        .await;
        let rows = data_of(&body)["items"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        // The self-service view must honour `action` like the admin one,
        // otherwise a filter that is silently ignored reads as "no rows".
        for row in &rows {
            assert_eq!(
                row["action"].as_str(),
                Some(action),
                "the action filter leaked other rows: {row}"
            );
        }
        if satisfied(rows.as_slice()) {
            return rows;
        }
        assert!(
            Instant::now() < deadline,
            "no `{action}` row the trail must carry ever reached the user's audit"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Idle cost of a live tunnel (docs §13.6: CPU < 1%, RSS delta < 30 MB).
///
/// CPU and RSS belong to the process, and this process carries two complete
/// engines (the server router and the local form) whose own housekeeping would
/// swamp the tunnel's cost. So the run has two phases: the same process idles
/// first with the tunnel **down**, then with one live tunnel. The sampler
/// watches for the `INTERLINK_TUNNEL_UP` line and reports both rates; only the
/// increment belongs to the tunnel (docs §13.6: CPU < 1%, RSS delta < 30 MB).
///
/// Run it alone: `cargo test -p wunder-runtime --test interlink_loopback
/// --features sqlite-storage -- --ignored --nocapture --test-threads=1`
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "idle tunnel budget measurement; run alone through scripts/interlink-bench/measure-idle.ps1"]
async fn idle_tunnel_stays_open_without_growth() {
    let window_s = std::env::var("INTERLINK_IDLE_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(60);

    let server = spawn_server().await;
    let node = build_local_node(&server).await;

    println!(
        "INTERLINK_IDLE_PID={} BASELINE_SECONDS={window_s} TUNNEL_SECONDS={window_s}",
        std::process::id()
    );
    for _ in 0..window_s {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let started = node.client.clone();
    tokio::task::spawn_blocking(move || started.start())
        .await
        .expect("interlink client start");
    wait_connected(&node.client).await;
    let drops_after_connect = node.client.status().dropped_frames;
    println!("INTERLINK_TUNNEL_UP");
    for _ in 0..window_s {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    // Idle means exactly that: the tunnel is still up and every bounded
    // structure is back to empty.
    let status = node.client.status();
    assert_eq!(
        status.state,
        wunder_server::interlink::client::TunnelState::Connected,
        "the tunnel must survive an idle window: {status:?}"
    );
    assert_eq!(status.pending_approvals, 0, "no approval queued while idle");
    assert_eq!(
        status.inflight_commands, 0,
        "no command in flight while idle"
    );
    assert_eq!(
        status.watched_threads, 0,
        "no remote subscription while idle"
    );
    assert_eq!(
        status.dropped_frames, drops_after_connect,
        "an idle tunnel must not saturate its own bounded queues: {status:?}"
    );
    assert!(
        status.last_error.is_none(),
        "idle tunnel reported {status:?}"
    );

    node.client.stop();
}


#[test]
fn review_shared_device_tunnels_replace_each_other() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4).thread_stack_size(8 * 1024 * 1024)
        .enable_all().build().unwrap();
    runtime.block_on(async {
        let server = spawn_server().await;
        server.state.config_store.update(|config| config.interlink.heartbeat_s = 1).await.unwrap();
        let first = build_local_node(&server).await;
        first.state.config_store.update(|config| config.interlink.heartbeat_s = 1).await.unwrap();
        let client = first.client.clone();
        tokio::task::spawn_blocking(move || client.start()).await.unwrap();
        wait_connected(&first.client).await;
        let initial = wunder_server::interlink::registry().by_device(DEVICE_ID).unwrap().channel_id;
        let mut heartbeat_channels = std::collections::HashSet::from([initial]);
        let deadline = Instant::now() + Duration::from_secs(7);
        while Instant::now() < deadline {
            if let Some(live) = wunder_server::interlink::registry().by_device(DEVICE_ID) {
                heartbeat_channels.insert(live.channel_id);
            }
            if heartbeat_channels.len() >= 2 { break; }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(heartbeat_channels.len() >= 2, "expected watchdog to close despite regular server pings");
        println!("review healthy heartbeat: {} distinct channels within 7 seconds", heartbeat_channels.len());

        let shared_session = cloud_shared().session().unwrap();
        let second = build_local_node(&server).await;
        shared_session.save(second._dir.path()).unwrap();
        let client = second.client.clone();
        tokio::task::spawn_blocking(move || client.start()).await.unwrap();
        wait_connected(&second.client).await;
        let mut channels = std::collections::HashSet::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(live) = wunder_server::interlink::registry().by_device(DEVICE_ID) {
                channels.insert(live.channel_id);
            }
            if channels.len() >= 3 { break; }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        first.client.stop();
        second.client.stop();
        assert!(channels.len() >= 3, "expected repeated replacement with a shared device identity");
        println!("review shared device: {} distinct tunnel channels within 10 seconds", channels.len());
    });
}
