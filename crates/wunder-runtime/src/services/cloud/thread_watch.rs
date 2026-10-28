//! Bounded, reconnecting view subscription shared by local cloud consumers.
use super::CloudService;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use tokio_util::sync::CancellationToken;
use wunder_core::thread_view::ThreadViewChanges;
pub type CloudThreadCancellation = CancellationToken;

#[derive(Debug)]
pub enum CloudThreadEvent {
    Change(ThreadViewChanges),
    ProcessItem(Value),
    Tail(Value),
    Reload,
    Connection(String),
}

pub struct CloudThreadWatch {
    receiver: mpsc::Receiver<CloudThreadEvent>,
    cancel: CancellationToken,
}
impl CloudThreadWatch {
    pub fn blocking_recv(&mut self) -> Option<CloudThreadEvent> {
        self.receiver.blocking_recv()
    }
    pub async fn recv(&mut self) -> Option<CloudThreadEvent> {
        self.receiver.recv().await
    }
    pub fn try_recv(&mut self) -> Option<CloudThreadEvent> {
        self.receiver.try_recv().ok()
    }
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}
impl Drop for CloudThreadWatch {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl CloudService {
    pub fn watch_cloud_thread(self: &Arc<Self>, thread: String, after: i64) -> CloudThreadWatch {
        let (sender, receiver) = mpsc::channel(32);
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let cloud = self.clone();
        tokio::spawn(async move {
            tokio::select! { biased; _ = stop.cancelled() => {}, _ = watch(cloud, thread, after, sender) => {} }
        });
        CloudThreadWatch { receiver, cancel }
    }
}

async fn watch(
    cloud: Arc<CloudService>,
    thread: String,
    mut after: i64,
    sender: mpsc::Sender<CloudThreadEvent>,
) {
    let Some(identity) = cloud.session() else {
        return;
    };
    let mut failures = 0u32;
    let mut epoch = None;
    loop {
        let _ = cloud.renew_if_needed().await;
        let Some(session) = cloud.session() else {
            return;
        };
        if session.server != identity.server
            || session.user_id != identity.user_id
            || session.device_id != identity.device_id
            || session.logged_in_at != identity.logged_in_at
        {
            return;
        }
        let result: anyhow::Result<()> = async {
            let mut url = url::Url::parse(&format!("{}/wunder/chat/ws", session.server))?;
            let scheme = if url.scheme() == "https" { "wss" } else { "ws" }; url.set_scheme(scheme).map_err(|_| anyhow::anyhow!("invalid server scheme"))?;
            let mut request = url.as_str().into_client_request()?;
            request.headers_mut().insert("sec-websocket-protocol", "wunder".parse()?);
            request.headers_mut().insert("authorization", format!("Bearer {}", session.token).parse()?);
            request.headers_mut().insert("x-wunder-device-id", session.device_id.parse()?);
            let mut config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
            config.max_message_size = Some(512 * 1024);
            config.max_frame_size = Some(512 * 1024);
            let (socket, _) = tokio::time::timeout(Duration::from_secs(8), tokio_tungstenite::connect_async_with_config(request, Some(config), false)).await??;
            let (mut sink, mut stream) = socket.split();
            sink.send(Message::Text(json!({"type":"connect","request_id":"cloud_connect","payload":{"protocol_version":1}}).to_string().into())).await?;
            let mut subscribed = false;
            let opened = tokio::time::Instant::now();
            let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
            let mut last_received = tokio::time::Instant::now();
            loop {
                tokio::select! {
                    _ = heartbeat.tick() => {
                        if !subscribed && opened.elapsed() > Duration::from_secs(10) { anyhow::bail!("cloud thread handshake timeout"); }
                        if last_received.elapsed() > Duration::from_secs(60) { anyhow::bail!("cloud thread heartbeat timeout"); }
                        sink.send(Message::Text(json!({"type":"ping"}).to_string().into())).await?;
                    }
                    incoming = stream.next() => {
                        let Some(incoming) = incoming else { anyhow::bail!("cloud thread connection closed") };
                        match incoming? {
                            Message::Ping(bytes) => { last_received = tokio::time::Instant::now(); sink.send(Message::Pong(bytes)).await?; }
                            Message::Close(_) => anyhow::bail!("cloud thread connection closed"),
                            Message::Text(text) => {
                                let frame: Value = serde_json::from_str(&text)?;
                                last_received = tokio::time::Instant::now();
                                if frame["type"] == "ready" && frame["request_id"] == "cloud_connect" && !subscribed {
                                    subscribed = true; failures = 0;
                                    sink.send(Message::Text(json!({"type":"watch","request_id":"cloud_watch","payload":{"session_id":thread,"view_seq":after}}).to_string().into())).await?;
                                    if sender.send(CloudThreadEvent::Connection("connected".into())).await.is_err() { return Ok(()); }
                                }
                                if frame["type"] == "error" {
                                    let code = frame.pointer("/payload/code").and_then(Value::as_str).unwrap_or("SUBSCRIPTION_REJECTED");
                                    if matches!(code, "SESSION_NOT_FOUND" | "PERMISSION_DENIED" | "INVALID_REQUEST" | "INVALID_PROTOCOL_RANGE" | "PROTOCOL_MISMATCH") {
                                        let _ = sender.send(CloudThreadEvent::Connection(code.into())).await;
                                        return Ok(());
                                    }
                                    anyhow::bail!("cloud thread subscription rejected: {code}");
                                }
                                let event = frame.pointer("/payload/event").and_then(Value::as_str).unwrap_or("");
                                if event == "thread_view_required" || event == "stream_overflow" {
                                    let _ = sender.send(CloudThreadEvent::Reload).await; return Ok(());
                                }
                                if matches!(event, "thread_process_item" | "thread_item_tail") {
                                    let data = frame["payload"]["data"].get("data").unwrap_or(&frame["payload"]["data"]).clone();
                                    let live = if event == "thread_process_item" { CloudThreadEvent::ProcessItem(data) } else { CloudThreadEvent::Tail(data) };
                                    if sender.send(live).await.is_err() { return Ok(()); }
                                }
                                if event == "thread_view_change" {
                                    let change: ThreadViewChanges = serde_json::from_value(frame["payload"]["data"].clone())?;
                                    if change.reset_required || epoch.is_some_and(|value| value != change.epoch) || (epoch.is_none() && change.view_seq < after) { let _ = sender.send(CloudThreadEvent::Reload).await; return Ok(()); }
                                    epoch = Some(change.epoch);
                                    if change.view_seq > after {
                                        after = change.view_seq;
                                        if sender.send(CloudThreadEvent::Change(change)).await.is_err() { return Ok(()); }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }.await;
        if result.is_ok() || sender.is_closed() {
            return;
        }
        if result.as_ref().err().and_then(|error| error.downcast_ref::<tokio_tungstenite::tungstenite::Error>())
            .is_some_and(|error| matches!(error, tokio_tungstenite::tungstenite::Error::Http(response) if matches!(response.status().as_u16(), 403 | 404))) {
            let _ = sender.send(CloudThreadEvent::Connection("PERMISSION_DENIED".into())).await;
            return;
        }
        // One refresh/reconnect path, bounded backoff; cursor advances only on accepted changes.
        let _ = cloud.refresh_account().await;
        if cloud.session().is_none() || cloud.status().expired {
            let _ = sender
                .send(CloudThreadEvent::Connection("expired".into()))
                .await;
            return;
        }
        failures = failures.saturating_add(1);
        if sender
            .send(CloudThreadEvent::Connection("reconnecting".into()))
            .await
            .is_err()
        {
            return;
        }
        tokio::time::sleep(Duration::from_secs((1u64 << failures.min(5)).min(30))).await;
    }
}
