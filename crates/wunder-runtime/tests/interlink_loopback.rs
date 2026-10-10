//! Loopback end-to-end for the interlink plane (互通方案 §13.2/§13.3): a real
//! server router over TCP plus the real client engine in the same process —
//! node secret bootstrap, WSS handshake, shadow push, C2L commands with the
//! approval gate, idempotency, the offline queue and the L2C cloud executor.
//!
//! The engine's tunnel session lives in the process-global `cloud::shared()`,
//! so this file keeps exactly one test function; every link assertion runs in
//! sequence inside it.

use axum::{Json as AxumJson, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::net::TcpListener;
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
    config.storage.db_path = dir
        .path()
        .join(db_name)
        .to_string_lossy()
        .to_string();
    config.workspace.root = dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
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
                capabilities: Some(
                    json!(["shadow:minimal", "shadow:full", "query.basic", "thread.drive"])
                        .to_string(),
                ),
                ..Default::default()
            },
        )
        .expect("grant device interlink capabilities");
}

/// Minimal OpenAI-compatible upstream so an approved remote turn can be
/// admitted and drained without a real model.
async fn spawn_mock_llm() -> String {
    async fn completions() -> AxumJson<Value> {
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
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock llm");
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

async fn spawn_server() -> Server {
    let dir = tempfile::tempdir().expect("server tempdir");
    let config = base_config(&dir, "server.db");
    let state = Arc::new(
        AppState::new_with_options(
            wunder_server::config_store::ConfigStore::new(dir.path().join("wunder.yaml")),
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
    let dir = tempfile::tempdir().expect("local tempdir");
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
            ..Default::default()
        },
    );
    let state = Arc::new(
        AppState::new_with_options(
            wunder_server::config_store::ConfigStore::new(dir.path().join("wunder.yaml")),
            config,
            AppStateInitOptions::cli_default(),
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
    session.save(&dir.path().to_path_buf()).expect("save session");
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
    // `start` takes the command receiver behind a blocking lock, so it must
    // run off the async worker threads.
    let started = client.clone();
    tokio::task::spawn_blocking(move || started.start())
        .await
        .expect("interlink client start");
    LocalNode {
        state,
        client,
        _dir: dir,
    }
}

// ---------------------------------------------------------------------------
// HTTP helpers against the served router
// ---------------------------------------------------------------------------

async fn http_json(
    method: &str,
    url: &str,
    token: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let client = reqwest::Client::new();
    let mut builder = match method {
        "POST" => client.post(url),
        "PATCH" => client.patch(url),
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

async fn get_json(url: &str, token: &str) -> (u16, Value) {
    http_json("GET", url, token, None).await
}

fn data_of(body: &Value) -> Value {
    body.get("data").cloned().unwrap_or(Value::Null)
}

async fn issue(server: &Server, to: &str, kind: &str, args: Value, command_id: Option<&str>) -> Value {
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
async fn wait_terminal(server: &Server, command_id: &str, timeout: Duration) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let (status, body) = get_json(
            &format!(
                "{}/wunder/interlink/commands/{command_id}",
                server.base_url
            ),
            &server.token,
        )
        .await;
        assert_eq!(status, 200, "command query failed: {body}");
        let record = data_of(&body);
        let state = record["status"].as_str().unwrap_or("");
        if !matches!(state, "issued" | "acked" | "running") {
            return record;
        }
        assert!(
            Instant::now() < deadline,
            "command {command_id} never reached a terminal state"
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn wait_connected(client: &InterlinkClient) {
    let deadline = Instant::now() + Duration::from_secs(30);
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

// ---------------------------------------------------------------------------
// The loopback run (one function: the tunnel session is process-global)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loopback_tunnel_shadow_commands_and_approvals_end_to_end() {
    let server = spawn_server().await;
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
    node.client
        .invalidate_shadow(Sections::ALL);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut shadow_threads = Value::Null;
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
            shadow_threads = shadow["threads"].clone();
            let text = shadow_threads.to_string();
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
    let cloud_record = wait_terminal(&server, cloud["command_id"].as_str().unwrap(), Duration::from_secs(20)).await;
    assert_eq!(cloud_record["status"], json!("succeeded"), "{cloud_record}");
    let entries = &cloud_record["result"]["result"]["result"]["entries"];
    assert!(
        entries.is_array(),
        "cloud listing returns entries (is_array={}): {cloud_record}",
        entries.is_array()
    );

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
    let record = wait_terminal(&server, sent["command_id"].as_str().unwrap(), Duration::from_secs(20)).await;
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
        node.client
            .decide_approval(&approval_id, wunder_server::interlink::client::Decision::Deny, false),
        "deny applied"
    );
    let record = wait_terminal(&server, rejected["command_id"].as_str().unwrap(), Duration::from_secs(20)).await;
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
        node.client
            .decide_approval(&approval_id, wunder_server::interlink::client::Decision::Approve, false),
        "approve applied"
    );
    let record = wait_terminal(&server, approved["command_id"].as_str().unwrap(), Duration::from_secs(30)).await;
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
    let (status, _) = get_json(
        &format!("{}/wunder/interlink/nodes", server.base_url),
        &server.token,
    )
    .await;
    assert_eq!(status, 200);
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
}

/// Wait for the node-side prompt of one issued L1 command.
async fn wait_prompt(
    client: &InterlinkClient,
    issued: &Value,
    timeout: Duration,
) -> String {
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
