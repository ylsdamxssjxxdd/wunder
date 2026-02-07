use anyhow::{anyhow, Context, Result};
use futures::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

const ADMIN_API_KEY: &str = "gateway-regression-api-key";
const TRUSTED_ORIGIN: &str = "https://trusted.example";
const EVIL_ORIGIN: &str = "https://evil.example";

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct GatewayHarness {
    child: Child,
    temp_dir: PathBuf,
    http_url: String,
    ws_url: String,
    server_log_path: PathBuf,
}

impl GatewayHarness {
    async fn start() -> Result<Self> {
        let binary = resolve_server_binary()?;

        for _ in 0..8 {
            let port = random_test_port();
            let temp_dir = std::env::temp_dir().join(format!(
                "wunder_gateway_regression_{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&temp_dir)
                .with_context(|| format!("create temp dir failed: {}", temp_dir.display()))?;

            let workspace_root = temp_dir.join("workspaces");
            fs::create_dir_all(&workspace_root).with_context(|| {
                format!("create workspace dir failed: {}", workspace_root.display())
            })?;
            let db_path = temp_dir.join("gateway-regression.db");
            let config_path = temp_dir.join("wunder.test.yaml");
            let override_path = temp_dir.join("override.not_used.yaml");

            let config_yaml = build_test_config(port, &workspace_root, &db_path);
            fs::write(&config_path, config_yaml)
                .with_context(|| format!("write config failed: {}", config_path.display()))?;

            let server_log_path = temp_dir.join("gateway-regression-server.log");
            let log_file = fs::File::create(&server_log_path).with_context(|| {
                format!("create server log failed: {}", server_log_path.display())
            })?;
            let log_file_stdout = log_file
                .try_clone()
                .context("clone server log handle for stdout failed")?;
            let child = Command::new(&binary)
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .env("WUNDER_CONFIG_PATH", &config_path)
                .env("WUNDER_CONFIG_OVERRIDE_PATH", &override_path)
                .env("WUNDER_SERVER_MODE", "api")
                .env_remove("WUNDER_PORT")
                .env_remove("WUNDER_HOST")
                .stdout(Stdio::from(log_file_stdout))
                .stderr(Stdio::from(log_file))
                .spawn()
                .context("failed to spawn wunder-server")?;

            let mut harness = Self {
                child,
                temp_dir,
                http_url: format!("http://127.0.0.1:{port}"),
                ws_url: format!("ws://127.0.0.1:{port}/wunder/gateway/ws"),
                server_log_path,
            };
            match harness.wait_until_ready().await {
                Ok(()) => return Ok(harness),
                Err(err) => {
                    let message = err.to_string();
                    harness.shutdown().await;
                    if message.contains("10048") || message.contains("address already in use") {
                        continue;
                    }
                    return Err(err);
                }
            }
        }

        Err(anyhow!("failed to start wunder-server after 8 attempts"))
    }

    async fn wait_until_ready(&mut self) -> Result<()> {
        let addr = self.http_url.trim_start_matches("http://").to_string();
        for _ in 0..120 {
            if let Some(status) = self
                .child
                .try_wait()
                .context("failed to poll child process")?
            {
                let logs = fs::read_to_string(&self.server_log_path)
                    .unwrap_or_else(|_| "<server log unavailable>".to_string());
                return Err(anyhow!("wunder-server exited early: {status}\n{logs}"));
            }
            if timeout(Duration::from_millis(200), TcpStream::connect(&addr))
                .await
                .is_ok_and(|result| result.is_ok())
            {
                return Ok(());
            }
            sleep(Duration::from_millis(100)).await;
        }
        let logs = fs::read_to_string(&self.server_log_path)
            .unwrap_or_else(|_| "<server log unavailable>".to_string());
        Err(anyhow!(
            "wunder-server did not become ready in time
{logs}"
        ))
    }

    async fn shutdown(&mut self) {
        let _ = self.child.start_kill();
        let _ = timeout(Duration::from_secs(2), self.child.wait()).await;
    }
}

impl Drop for GatewayHarness {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

struct WsClient {
    stream: WsStream,
}

impl WsClient {
    async fn connect(ws_url: &str, origin: Option<&str>) -> Result<Self> {
        let mut request = ws_url
            .into_client_request()
            .with_context(|| format!("invalid websocket url: {ws_url}"))?;
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            "wunder-gateway"
                .parse()
                .expect("valid websocket subprotocol"),
        );
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("Origin", origin.parse().context("invalid origin header")?);
        }
        request.headers_mut().insert(
            "x-api-key",
            ADMIN_API_KEY.parse().expect("valid admin api key header"),
        );
        let (stream, _) = connect_async(request)
            .await
            .with_context(|| format!("connect websocket failed: {ws_url}"))?;
        Ok(Self { stream })
    }

    async fn expect_challenge(&mut self) -> Result<()> {
        let message = self.recv_json(Duration::from_secs(3)).await?;
        let kind = message
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let event = message
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if kind != "event" || event != "connect.challenge" {
            return Err(anyhow!("unexpected handshake challenge: {message}"));
        }
        Ok(())
    }

    async fn send_connect(
        &mut self,
        request_id: &str,
        role: &str,
        client_id: &str,
        device_id: Option<&str>,
    ) -> Result<()> {
        let params = if let Some(device_id) = device_id {
            json!({
                "role": role,
                "client": { "id": client_id },
                "device": { "id": device_id }
            })
        } else {
            json!({
                "role": role,
                "client": { "id": client_id }
            })
        };
        self.send_json(json!({
            "type": "req",
            "id": request_id,
            "method": "connect",
            "params": params
        }))
        .await
    }

    async fn send_json(&mut self, message: Value) -> Result<()> {
        self.stream
            .send(Message::Text(message.to_string().into()))
            .await
            .context("send websocket message failed")
    }

    async fn recv_json(&mut self, wait: Duration) -> Result<Value> {
        loop {
            let next = timeout(wait, self.stream.next())
                .await
                .context("websocket receive timed out")?;
            let frame = next.ok_or_else(|| anyhow!("websocket closed"))?;
            let frame = frame.context("websocket frame error")?;
            match frame {
                Message::Text(text) => {
                    let parsed: Value = serde_json::from_str(text.as_ref())
                        .with_context(|| format!("invalid json message: {text}"))?;
                    return Ok(parsed);
                }
                Message::Binary(bytes) => {
                    let text = String::from_utf8(bytes.to_vec())
                        .context("binary frame is not valid utf-8")?;
                    let parsed: Value = serde_json::from_str(&text)
                        .with_context(|| format!("invalid json message: {text}"))?;
                    return Ok(parsed);
                }
                Message::Ping(payload) => {
                    self.stream
                        .send(Message::Pong(payload))
                        .await
                        .context("failed to pong")?;
                }
                Message::Pong(_) => {}
                Message::Close(frame) => {
                    return Err(anyhow!("websocket closed: {frame:?}"));
                }
                _ => {}
            }
        }
    }

    async fn wait_for_response(&mut self, request_id: &str, wait: Duration) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!("response timeout for request id: {request_id}"));
            }
            let message = self.recv_json(remaining).await?;
            let kind = message
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let id = message.get("id").and_then(Value::as_str);
            if kind == "res" && id == Some(request_id) {
                return Ok(message);
            }
        }
    }

    async fn wait_for_error_code(&mut self, code: &str, wait: Duration) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!("did not receive expected error code: {code}"));
            }
            let message = self.recv_json(remaining).await?;
            let error_code = message
                .get("error")
                .and_then(|value| value.get("code"))
                .and_then(Value::as_str);
            if error_code == Some(code) {
                return Ok(message);
            }
        }
    }

    async fn wait_for_invoke_request(&mut self, wait: Duration) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!("did not receive node.invoke request"));
            }
            let message = self.recv_json(remaining).await?;
            let kind = message
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let method = message
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if kind == "req" && method == "node.invoke" {
                return Ok(message);
            }
        }
    }

    async fn close(mut self) -> Result<()> {
        self.stream
            .send(Message::Close(None))
            .await
            .context("close websocket failed")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gateway_regression_suite() -> Result<()> {
    let mut harness = GatewayHarness::start().await?;

    case_handshake_timeout(&harness).await?;
    case_origin_rejection(&harness).await?;
    case_slow_client_eviction(&harness).await?;
    case_invoke_spoof_response_blocked(&harness).await?;

    harness.shutdown().await;
    Ok(())
}

async fn case_handshake_timeout(harness: &GatewayHarness) -> Result<()> {
    let mut ws = WsClient::connect(&harness.ws_url, Some(TRUSTED_ORIGIN)).await?;
    ws.expect_challenge().await?;
    let timeout_message = ws
        .wait_for_error_code("HANDSHAKE_TIMEOUT", Duration::from_secs(13))
        .await?;
    let code = timeout_message
        .get("error")
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str);
    if code != Some("HANDSHAKE_TIMEOUT") {
        return Err(anyhow!(
            "unexpected handshake timeout payload: {timeout_message}"
        ));
    }
    Ok(())
}

async fn case_origin_rejection(harness: &GatewayHarness) -> Result<()> {
    let mut ws = WsClient::connect(&harness.ws_url, Some(EVIL_ORIGIN)).await?;
    ws.expect_challenge().await?;
    ws.send_connect("origin-1", "operator", "origin-check-client", None)
        .await?;

    let response = ws
        .wait_for_response("origin-1", Duration::from_secs(5))
        .await?;
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(true);
    let code = response
        .get("error")
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str);
    if ok || code != Some("ORIGIN_NOT_ALLOWED") {
        return Err(anyhow!(
            "origin validation should reject request: {response}"
        ));
    }
    Ok(())
}

async fn case_slow_client_eviction(harness: &GatewayHarness) -> Result<()> {
    let (mut observer, _) = connect_client(
        &harness.ws_url,
        Some(TRUSTED_ORIGIN),
        "operator",
        "observer-client",
        None,
    )
    .await?;

    let large_client_id = format!("slow-{}", "x".repeat(80_000));
    let (slow_client, slow_connection_id) = connect_client(
        &harness.ws_url,
        Some(TRUSTED_ORIGIN),
        "channel",
        &large_client_id,
        None,
    )
    .await?;

    let mut evicted = false;
    for round in 0..16 {
        for idx in 0..8 {
            let transient_id = format!("transient-{round}-{idx}");
            let (client, _) = connect_client(
                &harness.ws_url,
                Some(TRUSTED_ORIGIN),
                "channel",
                &transient_id,
                None,
            )
            .await?;
            let _ = client.close().await;
        }

        let request_id = format!("presence-{round}");
        observer
            .send_json(json!({
                "type": "req",
                "id": request_id,
                "method": "presence.get"
            }))
            .await?;
        let response = observer
            .wait_for_response(&request_id, Duration::from_secs(8))
            .await?;

        let items = response
            .get("payload")
            .and_then(|value| value.get("items"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let still_online = items.iter().any(|item| {
            item.get("connection_id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == slow_connection_id)
        });
        if !still_online {
            evicted = true;
            break;
        }
    }

    if !evicted {
        return Err(anyhow!(
            "slow client was not evicted after repeated backpressure"
        ));
    }

    let _ = observer.close().await;
    let _ = slow_client.close().await;
    Ok(())
}

async fn case_invoke_spoof_response_blocked(harness: &GatewayHarness) -> Result<()> {
    let (mut node_a, _) = connect_client(
        &harness.ws_url,
        Some(TRUSTED_ORIGIN),
        "node",
        "node-a-client",
        Some("node-a"),
    )
    .await?;
    let (mut node_b, _) = connect_client(
        &harness.ws_url,
        Some(TRUSTED_ORIGIN),
        "node",
        "node-b-client",
        Some("node-b"),
    )
    .await?;

    let invoke_url = format!("{}/wunder/admin/gateway/invoke", harness.http_url);
    let http_client = Client::new();
    let invoke_task = tokio::spawn(async move {
        let response = http_client
            .post(invoke_url)
            .header("x-api-key", ADMIN_API_KEY)
            .json(&json!({
                "node_id": "node-a",
                "command": "diagnose",
                "args": { "case": "spoof-check" },
                "timeout_s": 8
            }))
            .send()
            .await
            .context("admin invoke request failed")?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .context("parse admin invoke response failed")?;
        if !status.is_success() {
            return Err(anyhow!("admin invoke failed {status}: {body}"));
        }
        Ok::<Value, anyhow::Error>(body)
    });

    let invoke_request = node_a
        .wait_for_invoke_request(Duration::from_secs(8))
        .await?;
    let request_id = invoke_request
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("node.invoke request id missing: {invoke_request}"))?
        .to_string();

    node_b
        .send_json(json!({
            "type": "res",
            "id": request_id,
            "ok": true,
            "payload": {
                "source": "node-b-forged"
            }
        }))
        .await?;

    sleep(Duration::from_millis(150)).await;

    node_a
        .send_json(json!({
            "type": "res",
            "id": request_id,
            "ok": true,
            "payload": {
                "source": "node-a-legit"
            }
        }))
        .await?;

    let invoke_response = invoke_task
        .await
        .context("join admin invoke task failed")??;
    let data = invoke_response
        .get("data")
        .cloned()
        .ok_or_else(|| anyhow!("missing data field in invoke response: {invoke_response}"))?;
    let ok = data.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let source = data
        .get("payload")
        .and_then(|value| value.get("source"))
        .and_then(Value::as_str);

    if !ok || source != Some("node-a-legit") {
        return Err(anyhow!(
            "invoke response should only accept node-a payload: {invoke_response}"
        ));
    }

    let _ = node_a.close().await;
    let _ = node_b.close().await;
    Ok(())
}

async fn connect_client(
    ws_url: &str,
    origin: Option<&str>,
    role: &str,
    client_id: &str,
    device_id: Option<&str>,
) -> Result<(WsClient, String)> {
    let mut client = WsClient::connect(ws_url, origin).await?;
    client.expect_challenge().await?;
    let request_id = format!("connect-{}", Uuid::new_v4().simple());
    client
        .send_connect(&request_id, role, client_id, device_id)
        .await?;
    let response = client
        .wait_for_response(&request_id, Duration::from_secs(6))
        .await?;
    let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if !ok {
        return Err(anyhow!("connect rejected for role {role}: {response}"));
    }
    let connection_id = response
        .get("payload")
        .and_then(|value| value.get("connection_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("connection id missing in hello payload: {response}"))?
        .to_string();
    Ok((client, connection_id))
}

fn random_test_port() -> u16 {
    let seed = Uuid::new_v4().as_u128();
    20_000 + (seed % 30_000) as u16
}

fn build_test_config(port: u16, workspace_root: &Path, db_path: &Path) -> String {
    let workspace = normalize_path_for_yaml(workspace_root);
    let db_path = normalize_path_for_yaml(db_path);
    format!(
        r#"server:
  host: "127.0.0.1"
  port: {port}
  stream_chunk_size: 1024
  max_active_sessions: 30
  mode: "api"
security:
  api_key: "{ADMIN_API_KEY}"
gateway:
  enabled: true
  protocol_version: 1
  allow_unpaired_nodes: true
  node_token_required: false
  allow_gateway_token_for_nodes: false
  allowed_origins:
    - "{TRUSTED_ORIGIN}"
  trusted_proxies:
    - "loopback"
storage:
  backend: "sqlite"
  db_path: "{db_path}"
workspace:
  root: "{workspace}"
"#
    )
}

fn resolve_server_binary() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_wunder-server") {
        return Ok(PathBuf::from(path));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let executable = if cfg!(windows) {
        "wunder-server.exe"
    } else {
        "wunder-server"
    };
    let fallback = manifest_dir.join("target").join("debug").join(executable);
    if fallback.exists() {
        return Ok(fallback);
    }

    Err(anyhow!(
        "cannot locate wunder-server binary, run `cargo build --bin wunder-server` first"
    ))
}

fn normalize_path_for_yaml(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
