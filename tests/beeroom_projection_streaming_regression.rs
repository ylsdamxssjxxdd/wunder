use anyhow::{anyhow, Context, Result};
use futures::{SinkExt, Stream, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;
use wunder_server::{
    build_desktop_router,
    config::Config,
    config_store::ConfigStore,
    state::{AppState, AppStateInitOptions},
};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type BoxedBytesStream = Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;

struct BeeroomProjectionHarness {
    state: Arc<AppState>,
    user_id: String,
    token: String,
    http_base: String,
    ws_url: String,
    _temp_dir: TempDir,
    server_task: JoinHandle<()>,
}

impl BeeroomProjectionHarness {
    async fn start() -> Result<Self> {
        let temp_dir = tempfile::tempdir().context("create temp dir failed")?;
        let mut config = Config::default();
        config.storage.backend = "sqlite".to_string();
        config.storage.db_path = temp_dir
            .path()
            .join("beeroom-realtime-regression.db")
            .to_string_lossy()
            .to_string();
        config.workspace.root = temp_dir
            .path()
            .join("workspaces")
            .to_string_lossy()
            .to_string();

        let config_store = ConfigStore::new(temp_dir.path().join("wunder.override.yaml"));
        let config_snapshot = config.clone();
        config_store
            .update(|current| *current = config_snapshot.clone())
            .await
            .context("update config store failed")?;

        let state = Arc::new(
            AppState::new_with_options(config_store, config, AppStateInitOptions::cli_default())
                .context("create app state failed")?,
        );
        let user = state
            .user_store
            .create_user(
                "beeroom_projection_tester",
                Some("beeroom_projection_tester@example.test".to_string()),
                "password-123",
                Some("A"),
                None,
                vec!["user".to_string()],
                "active",
                false,
            )
            .context("create user failed")?;
        let token = state
            .user_store
            .create_session_token(&user.user_id)
            .context("create session token failed")?
            .token;

        let app = build_desktop_router(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind test listener failed")?;
        let addr = listener.local_addr().context("read listener addr failed")?;
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        wait_until_listening(addr).await?;

        Ok(Self {
            state,
            user_id: user.user_id,
            token,
            http_base: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/wunder/beeroom/ws"),
            _temp_dir: temp_dir,
            server_task,
        })
    }

    async fn create_group(&self, suffix: &str) -> Result<String> {
        let group_id = format!("realtime-{suffix}-{}", Uuid::new_v4().simple());
        let response = Client::new()
            .post(format!("{}/wunder/beeroom/groups", self.http_base))
            .bearer_auth(&self.token)
            .json(&json!({
                "name": format!("Realtime {suffix}"),
                "group_id": group_id,
            }))
            .send()
            .await
            .context("create beeroom group request failed")?;
        let status = response.status();
        let payload: Value = response
            .json()
            .await
            .context("decode create group response failed")?;
        if !status.is_success() {
            return Err(anyhow!(
                "create group failed: status={status}, payload={payload}"
            ));
        }
        let created_id = payload
            .get("data")
            .and_then(|data| data.get("group_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or(group_id);
        Ok(created_id)
    }

    async fn publish_group_event(&self, group_id: &str, event_type: &str, payload: Value) {
        self.state
            .projection
            .beeroom
            .publish_group_event(&self.user_id, group_id, event_type, payload)
            .await;
    }

    async fn latest_event_id(&self, group_id: &str) -> Result<i64> {
        self.state
            .projection
            .beeroom
            .latest_event_id(&self.user_id, group_id)
            .await
            .context("load latest event id failed")
    }

    async fn connect_ws(&self) -> Result<WsStream> {
        let mut request = self
            .ws_url
            .as_str()
            .into_client_request()
            .context("build websocket request failed")?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.token)
                .parse()
                .context("build authorization header failed")?,
        );
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            "wunder".parse().expect("valid ws subprotocol"),
        );
        let (stream, _) = connect_async(request)
            .await
            .context("connect websocket failed")?;
        Ok(stream)
    }
}

impl Drop for BeeroomProjectionHarness {
    fn drop(&mut self) {
        self.server_task.abort();
    }
}

#[derive(Debug, Clone)]
struct ParsedSseEvent {
    event: String,
    id: Option<String>,
    data: String,
}

struct SseEventReader {
    stream: BoxedBytesStream,
    buffer: String,
}

impl SseEventReader {
    fn new(response: reqwest::Response) -> Self {
        Self {
            stream: Box::pin(response.bytes_stream()),
            buffer: String::new(),
        }
    }

    async fn next_event(&mut self, wait: Duration) -> Result<ParsedSseEvent> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(event) = parse_sse_event_from_buffer(&mut self.buffer) {
                return Ok(event);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!("receive sse event timed out"));
            }
            let next = timeout(remaining, self.stream.next())
                .await
                .context("wait sse chunk timed out")?;
            let chunk = next
                .ok_or_else(|| anyhow!("sse stream closed"))?
                .context("read sse chunk failed")?;
            let text = String::from_utf8_lossy(&chunk).replace("\r\n", "\n");
            self.buffer.push_str(&text);
        }
    }
}

fn parse_sse_event_from_buffer(buffer: &mut String) -> Option<ParsedSseEvent> {
    let delimiter_index = buffer.find("\n\n")?;
    let raw_block = buffer[..delimiter_index].to_string();
    buffer.drain(..delimiter_index + 2);
    let mut event_name = String::new();
    let mut event_id: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();
    for line in raw_block.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_name = value.trim().to_string();
            continue;
        }
        if let Some(value) = line.strip_prefix("id:") {
            let normalized = value.trim();
            if !normalized.is_empty() {
                event_id = Some(normalized.to_string());
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
            continue;
        }
    }
    if event_name.is_empty() && data_lines.is_empty() {
        return None;
    }
    Some(ParsedSseEvent {
        event: if event_name.is_empty() {
            "message".to_string()
        } else {
            event_name
        },
        id: event_id,
        data: data_lines.join("\n"),
    })
}

async fn wait_until_listening(addr: std::net::SocketAddr) -> Result<()> {
    for _ in 0..50 {
        if TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(20)).await;
    }
    Err(anyhow!("test server did not become ready in time"))
}

async fn ws_send_watch(
    stream: &mut WsStream,
    request_id: &str,
    group_id: &str,
    after_event_id: i64,
) -> Result<()> {
    let payload = json!({
        "type": "watch",
        "request_id": request_id,
        "payload": {
            "group_id": group_id,
            "after_event_id": after_event_id,
        }
    });
    stream
        .send(Message::Text(payload.to_string()))
        .await
        .context("send watch message failed")
}

async fn ws_recv_json(stream: &mut WsStream, wait: Duration) -> Result<Value> {
    loop {
        let next = timeout(wait, stream.next())
            .await
            .context("wait websocket frame timed out")?;
        let frame = next.ok_or_else(|| anyhow!("websocket closed"))?;
        let frame = frame.context("read websocket frame failed")?;
        match frame {
            Message::Text(text) => {
                let payload = serde_json::from_str::<Value>(text.as_ref())
                    .with_context(|| format!("invalid websocket json: {text}"))?;
                return Ok(payload);
            }
            Message::Binary(bytes) => {
                let text =
                    String::from_utf8(bytes.to_vec()).context("websocket binary is not utf-8")?;
                let payload = serde_json::from_str::<Value>(&text)
                    .with_context(|| format!("invalid websocket json: {text}"))?;
                return Ok(payload);
            }
            Message::Ping(payload) => {
                stream
                    .send(Message::Pong(payload))
                    .await
                    .context("send websocket pong failed")?;
            }
            Message::Pong(_) => {}
            Message::Close(frame) => {
                return Err(anyhow!("websocket closed by server: {frame:?}"));
            }
            _ => {}
        }
    }
}

async fn wait_for_ws_event(
    stream: &mut WsStream,
    request_id: &str,
    event_name: &str,
    wait: Duration,
) -> Result<Value> {
    let deadline = Instant::now() + wait;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(anyhow!(
                "did not receive websocket event {event_name} for request {request_id}"
            ));
        }
        let message = ws_recv_json(stream, remaining).await?;
        let message_type = message
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if message_type == "error" {
            return Err(anyhow!("websocket returned error message: {message}"));
        }
        if message_type != "event" {
            continue;
        }
        if message.get("request_id").and_then(Value::as_str) != Some(request_id) {
            continue;
        }
        let payload = message.get("payload").cloned().unwrap_or(Value::Null);
        if payload.get("event").and_then(Value::as_str) == Some(event_name) {
            return Ok(payload);
        }
    }
}

async fn wait_for_ws_any_event_for_request(
    stream: &mut WsStream,
    request_id: &str,
    wait: Duration,
) -> Result<Option<Value>> {
    let deadline = Instant::now() + wait;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        match ws_recv_json(stream, remaining).await {
            Ok(message) => {
                let message_type = message
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if message_type == "error" {
                    return Err(anyhow!("websocket returned error message: {message}"));
                }
                if message_type != "event" {
                    continue;
                }
                if message.get("request_id").and_then(Value::as_str) != Some(request_id) {
                    continue;
                }
                return Ok(message.get("payload").cloned());
            }
            Err(_) => return Ok(None),
        }
    }
}

#[tokio::test]
async fn ws_watch_replays_gap_then_forwards_new_event() -> Result<()> {
    let harness = BeeroomProjectionHarness::start().await?;
    let group_id = harness.create_group("ws-gap").await?;
    harness
        .publish_group_event(&group_id, "team_task_dispatch", json!({ "seq": 1 }))
        .await;
    harness
        .publish_group_event(&group_id, "team_task_result", json!({ "seq": 2 }))
        .await;
    let latest_event_id = harness.latest_event_id(&group_id).await?;
    assert!(latest_event_id > 1);
    let after_event_id = latest_event_id - 1;

    let mut ws = harness.connect_ws().await?;
    let ready = ws_recv_json(&mut ws, Duration::from_secs(3)).await?;
    assert_eq!(ready.get("type"), Some(&json!("ready")));

    ws_send_watch(&mut ws, "watch-gap", &group_id, after_event_id).await?;

    let replayed = wait_for_ws_event(
        &mut ws,
        "watch-gap",
        "team_task_result",
        Duration::from_secs(3),
    )
    .await?;
    assert_eq!(replayed["data"]["seq"], json!(2));
    assert_eq!(replayed["id"], json!(latest_event_id.to_string()));

    let watching =
        wait_for_ws_event(&mut ws, "watch-gap", "watching", Duration::from_secs(3)).await?;
    assert_eq!(watching["data"]["group_id"], json!(group_id.clone()));
    assert_eq!(watching["data"]["after_event_id"], json!(latest_event_id));

    let no_sync_required =
        wait_for_ws_any_event_for_request(&mut ws, "watch-gap", Duration::from_millis(260)).await?;
    assert!(no_sync_required.is_none());

    harness
        .publish_group_event(&group_id, "team_task_update", json!({ "seq": 3 }))
        .await;
    let update = wait_for_ws_event(
        &mut ws,
        "watch-gap",
        "team_task_update",
        Duration::from_secs(3),
    )
    .await?;
    let forwarded_event_id = update
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("team_task_update missing id"))?
        .parse::<i64>()
        .context("parse forwarded event id failed")?;
    assert!(forwarded_event_id > latest_event_id);
    assert_eq!(update["data"]["seq"], json!(3));
    Ok(())
}

#[tokio::test]
async fn ws_watch_does_not_emit_resume_gap_without_gap_and_ignores_other_group_events() -> Result<()>
{
    let harness = BeeroomProjectionHarness::start().await?;
    let target_group_id = harness.create_group("ws-no-gap-target").await?;
    let other_group_id = harness.create_group("ws-no-gap-other").await?;
    harness
        .publish_group_event(
            &target_group_id,
            "team_task_dispatch",
            json!({ "target": "seed" }),
        )
        .await;
    let latest_event_id = harness.latest_event_id(&target_group_id).await?;

    let mut ws = harness.connect_ws().await?;
    let ready = ws_recv_json(&mut ws, Duration::from_secs(3)).await?;
    assert_eq!(ready.get("type"), Some(&json!("ready")));

    ws_send_watch(&mut ws, "watch-no-gap", &target_group_id, latest_event_id).await?;
    let watching =
        wait_for_ws_event(&mut ws, "watch-no-gap", "watching", Duration::from_secs(3)).await?;
    assert_eq!(watching["data"]["after_event_id"], json!(latest_event_id));

    // No resume gap should be emitted when the cursor already equals latest_event_id.
    let no_gap_event =
        wait_for_ws_any_event_for_request(&mut ws, "watch-no-gap", Duration::from_millis(260))
            .await?;
    assert!(no_gap_event.is_none());

    harness
        .publish_group_event(
            &other_group_id,
            "team_task_update",
            json!({ "target": "other-group" }),
        )
        .await;

    // Cross-group events must not leak into the target watch stream.
    let cross_group_event =
        wait_for_ws_any_event_for_request(&mut ws, "watch-no-gap", Duration::from_millis(260))
            .await?;
    assert!(cross_group_event.is_none());

    harness
        .publish_group_event(
            &target_group_id,
            "team_task_update",
            json!({ "target": "target-group" }),
        )
        .await;
    let target_event = wait_for_ws_event(
        &mut ws,
        "watch-no-gap",
        "team_task_update",
        Duration::from_secs(3),
    )
    .await?;
    assert_eq!(target_event["data"]["target"], json!("target-group"));
    Ok(())
}

#[tokio::test]
async fn sse_stream_replays_gap_and_forwards_new_event() -> Result<()> {
    let harness = BeeroomProjectionHarness::start().await?;
    let group_id = harness.create_group("sse-gap").await?;
    harness
        .publish_group_event(&group_id, "team_task_dispatch", json!({ "seq": 1 }))
        .await;
    harness
        .publish_group_event(&group_id, "team_task_result", json!({ "seq": 2 }))
        .await;
    let latest_event_id = harness.latest_event_id(&group_id).await?;
    let after_event_id = latest_event_id - 1;
    assert!(after_event_id > 0);

    let response = Client::new()
        .get(format!(
            "{}/wunder/beeroom/groups/{group_id}/chat/stream",
            harness.http_base
        ))
        .query(&[
            ("after_event_id", after_event_id.to_string()),
            ("access_token", harness.token.clone()),
        ])
        .send()
        .await
        .context("request beeroom sse stream failed")?;
    assert!(response.status().is_success());

    let mut reader = SseEventReader::new(response);
    let replayed = reader.next_event(Duration::from_secs(3)).await?;
    assert_eq!(replayed.event, "team_task_result");
    let latest_event_id_text = latest_event_id.to_string();
    assert_eq!(replayed.id.as_deref(), Some(latest_event_id_text.as_str()));
    let replayed_data: Value =
        serde_json::from_str(&replayed.data).context("parse replay event data failed")?;
    assert_eq!(replayed_data["seq"], json!(2));

    let watching = reader.next_event(Duration::from_secs(3)).await?;
    assert_eq!(watching.event, "watching");
    let watching_data: Value =
        serde_json::from_str(&watching.data).context("parse watching event data failed")?;
    assert_eq!(watching_data["group_id"], json!(group_id.clone()));
    assert_eq!(watching_data["after_event_id"], json!(latest_event_id));

    harness
        .publish_group_event(&group_id, "team_finish", json!({ "seq": 3 }))
        .await;
    let finish_event = reader.next_event(Duration::from_secs(3)).await?;
    assert_eq!(finish_event.event, "team_finish");
    let finish_data: Value =
        serde_json::from_str(&finish_event.data).context("parse team_finish data failed")?;
    assert_eq!(finish_data["seq"], json!(3));
    let forwarded_event_id = finish_event
        .id
        .as_deref()
        .ok_or_else(|| anyhow!("team_finish missing sse id"))?
        .parse::<i64>()
        .context("parse team_finish sse id failed")?;
    assert!(forwarded_event_id > latest_event_id);
    Ok(())
}

#[tokio::test]
async fn sse_stream_uses_last_event_id_header_for_replay_cursor() -> Result<()> {
    let harness = BeeroomProjectionHarness::start().await?;
    let group_id = harness.create_group("sse-header-cursor").await?;
    harness
        .publish_group_event(&group_id, "team_task_dispatch", json!({ "seq": 1 }))
        .await;
    harness
        .publish_group_event(&group_id, "team_task_result", json!({ "seq": 2 }))
        .await;
    let latest_event_id = harness.latest_event_id(&group_id).await?;
    let header_cursor = latest_event_id - 1;
    assert!(header_cursor > 0);

    let response = Client::new()
        .get(format!(
            "{}/wunder/beeroom/groups/{group_id}/chat/stream",
            harness.http_base
        ))
        .query(&[("access_token", harness.token.clone())])
        .header("Last-Event-ID", header_cursor.to_string())
        .send()
        .await
        .context("request beeroom sse stream with Last-Event-ID failed")?;
    assert!(response.status().is_success());

    let mut reader = SseEventReader::new(response);
    let replayed = reader.next_event(Duration::from_secs(3)).await?;
    assert_eq!(replayed.event, "team_task_result");
    let latest_event_id_text = latest_event_id.to_string();
    assert_eq!(replayed.id.as_deref(), Some(latest_event_id_text.as_str()));
    let replayed_data: Value =
        serde_json::from_str(&replayed.data).context("parse replay event data failed")?;
    assert_eq!(replayed_data["seq"], json!(2));

    let watching = reader.next_event(Duration::from_secs(3)).await?;
    assert_eq!(watching.event, "watching");
    let watching_data: Value =
        serde_json::from_str(&watching.data).context("parse watching event data failed")?;
    assert_eq!(watching_data["group_id"], json!(group_id.clone()));
    assert_eq!(watching_data["after_event_id"], json!(latest_event_id));
    Ok(())
}
