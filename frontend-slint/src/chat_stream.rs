//! One worker per active turn. Replay never re-submits a user message.
use crate::chat_api::ChatApi;
use serde_json::{json, Value};
use std::{
    io::ErrorKind,
    net::TcpStream,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
        Arc,
    },
    time::{Duration, Instant},
};
use tungstenite::{
    client::IntoClientRequest, http::HeaderValue, protocol::WebSocketConfig, Message,
};

pub enum Update {
    Event(Value),
    Status(String),
    Finished,
    Failed(String),
}

pub struct Lifetime(pub Arc<AtomicBool>);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub fn run(
    api: ChatApi,
    session: String,
    content: Option<String>,
    tx: SyncSender<Update>,
    closed: Arc<AtomicBool>,
) {
    let result = run_inner(&api, &session, content, &tx, &closed);
    if !closed.load(Ordering::Relaxed) {
        let _ = tx.send(match result {
            Ok(()) => Update::Finished,
            Err(error) => Update::Failed(error),
        });
    }
}

fn run_inner(
    api: &ChatApi,
    session: &str,
    content: Option<String>,
    tx: &SyncSender<Update>,
    closed: &AtomicBool,
) -> Result<(), String> {
    let mut cursor = 0;
    let mut submitted = content.is_none();
    let mut retries = 0;
    let client_id = format!(
        "slint-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    // Capture the pre-turn cursor: an ambiguous disconnect before the first event
    // must replay this turn, never a previous completed response or another start.
    if content.is_some() {
        cursor = api
            .event_tail(session)?
            .pointer("/data/last_event_id")
            .and_then(Value::as_i64)
            .unwrap_or(0);
    }
    let mut reducer = crate::stream_events::EventCursor::new(cursor);
    loop {
        if closed.load(Ordering::Relaxed) {
            return Ok(());
        }
        let (address, url) = api.api_base.websocket_target();
        let connected = (|| {
            let socket = TcpStream::connect_timeout(&address, Duration::from_secs(3))
                .map_err(|e| e.to_string())?;
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .map_err(|e| e.to_string())?;
            socket
                .set_write_timeout(Some(Duration::from_secs(3)))
                .map_err(|e| e.to_string())?;
            socket.set_nodelay(true).map_err(|e| e.to_string())?;
            let mut request = url.into_client_request().map_err(|e| e.to_string())?;
            request.headers_mut().insert(
                "Authorization",
                HeaderValue::from_str(&format!("Bearer {}", api.resolve_token()?))
                    .map_err(|_| "invalid token")?,
            );
            let config = WebSocketConfig::default()
                .read_buffer_size(16 * 1024)
                .write_buffer_size(0)
                .max_write_buffer_size(64 * 1024)
                .max_message_size(Some(2 * 1024 * 1024))
                .max_frame_size(Some(2 * 1024 * 1024));
            let (mut ws, _) =
                tungstenite::client::client_with_config(request, socket, Some(config))
                    .map_err(|e| e.to_string())?;
            ws.get_mut()
                .set_read_timeout(Some(Duration::from_millis(100)))
                .map_err(|e| e.to_string())?;
            let request_id = client_id.clone();
            if !submitted {
                // Mark before sending: even a partial write is an ambiguous submission.
                submitted = true;
                ws.send(Message::Text(
                    json!({"type":"start", "request_id":request_id, "session_id":session,
                    "payload":{"content":content, "stream":true, "client_message_id":client_id}})
                    .to_string()
                    .into(),
                ))
                .map_err(|e| e.to_string())?;
            } else {
                ws.send(Message::Text(
                    json!({"type":"watch", "request_id":request_id, "session_id":session,
                    "payload":{"after_event_id":reducer.last_id}})
                    .to_string()
                    .into(),
                ))
                .map_err(|e| e.to_string())?;
            }
            let mut last_packet = Instant::now();
            let mut last_ping = Instant::now();
            let mut last_probe = Instant::now();
            loop {
                if closed.load(Ordering::Relaxed) {
                    return Ok(true);
                }
                match ws.read() {
                    Ok(Message::Text(text)) => {
                        last_packet = Instant::now();
                        let envelope: Value =
                            serde_json::from_str(&text).map_err(|_| "invalid stream JSON")?;
                        if envelope.get("type").and_then(Value::as_str) == Some("error") {
                            let message = envelope
                                .pointer("/payload/message")
                                .and_then(Value::as_str)
                                .unwrap_or("stream request rejected");
                            tx.send(Update::Failed(message.to_string()))
                                .map_err(|_| "window closed")?;
                            return Ok(true);
                        }
                        if envelope.get("type").and_then(Value::as_str) != Some("event") {
                            continue;
                        }
                        let payload = envelope.get("payload").ok_or("missing stream event")?;
                        if payload.get("event").and_then(Value::as_str) == Some("slow_client") {
                            return Err("replay requested".to_string());
                        }
                        for event in reducer.accept(payload, session) {
                            let terminal = crate::stream_events::is_terminal(&event);
                            tx.send(Update::Event(event)).map_err(|_| "window closed")?;
                            if terminal {
                                return Ok(true);
                            }
                        }
                    }
                    Ok(Message::Ping(_)) => {
                        ws.flush().map_err(|e| e.to_string())?;
                        last_packet = Instant::now();
                    }
                    Ok(Message::Pong(_)) => {
                        last_packet = Instant::now();
                    }
                    Ok(Message::Close(_)) => return Err("connection closed".to_string()),
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(error))
                        if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                    Err(error) => return Err(error.to_string()),
                }
                if last_ping.elapsed() >= Duration::from_secs(10) {
                    ws.send(Message::Text(json!({"type":"ping"}).to_string().into()))
                        .map_err(|e| e.to_string())?;
                    last_ping = Instant::now();
                }
                if last_packet.elapsed() > Duration::from_secs(35) {
                    return Err("stream heartbeat timed out".to_string());
                }
                // watch deliberately stays open when idle. Confirm durable settlement
                // periodically, also covering a terminal event lost to retention.
                if last_probe.elapsed() >= Duration::from_secs(5) {
                    last_probe = Instant::now();
                    if let Ok(tail) = api.event_tail(session) {
                        let data = &tail["data"];
                        let idle = data["running"] == false && data["queued"] == false;
                        let tail_id = data["last_event_id"].as_i64().unwrap_or(0);
                        if idle && tail_id <= reducer.last_id {
                            return Ok(true);
                        }
                    }
                }
            }
        })();
        match connected {
            Ok(true) => return Ok(()),
            _ if retries < 5 => {
                retries += 1;
                tx.send(Update::Status(format!(
                    "连接中断，正在恢复（{retries}/5）…"
                )))
                .map_err(|_| "window closed")?;
                for _ in 0..(retries * 2) {
                    if closed.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Err(error) => return Err(format!("无法恢复连接，请刷新会话确认结果：{error}")),
            Ok(false) => unreachable!(),
        }
    }
}
