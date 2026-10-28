use crate::core::blocking;
use crate::i18n;
use crate::orchestrator_constants::{
    STREAM_EVENT_QUEUE_SIZE, STREAM_EVENT_SLOW_CLIENT_QUEUE_WATERMARK,
    STREAM_EVENT_SLOW_CLIENT_WARN_INTERVAL_S,
};
use crate::schemas::StreamEvent;
use crate::state::AppState;
use anyhow::Error;
use axum::extract::ws::Message;
use axum::http::header::AUTHORIZATION;
use axum::http::header::SEC_WEBSOCKET_PROTOCOL;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub(crate) const WS_PROTOCOL_VERSION: i32 = 1;
pub(crate) const WS_PROTOCOL_MIN_VERSION: i32 = 1;
pub(crate) const WS_PROTOCOL_MAX_VERSION: i32 = 1;
pub(crate) const WS_MAX_MESSAGE_BYTES: usize = 512 * 1024;

/// Command tails outlive a model turn. Subscribe independently of its online
/// queue; initial snapshots and lag recovery heal the bounded live channel.
pub(crate) async fn watch_command_sessions(
    broker: Arc<crate::services::tools::command_sessions::CommandSessionBroker>,
    user_id: String,
    session_id: String,
    request_id: Option<String>,
    tx: WsSender,
    cancel: CancellationToken,
) {
    let mut events = broker.subscribe();
    let mut cursors = std::collections::HashMap::new();
    loop {
        cursors.clear();
        for snapshot in broker.list_session_snapshots(&user_id, &session_id) {
            if cursors.len() < 256 {
                cursors.insert(snapshot.command_session_id.clone(), snapshot.seq);
            }
            let event = StreamEvent {
                event: "command_session_summary".into(),
                data: serde_json::to_value(snapshot).unwrap_or(Value::Null),
                id: None,
                timestamp: Some(Utc::now()),
            };
            if tokio::select! {
                _ = cancel.cancelled() => return,
                result = send_ws_event(&tx, request_id.as_deref(), event) => result,
            }
            .is_err()
            {
                return;
            }
        }
        loop {
            let event = tokio::select! {
                _ = cancel.cancelled() => return,
                event = events.recv() => event,
            };
            let event = match event {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            };
            if event.user_id != user_id || event.session_id != session_id {
                continue;
            }
            let id = event
                .data
                .get("command_session_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let seq = event.data.get("seq").and_then(Value::as_u64).unwrap_or(0);
            if cursors.get(id).is_some_and(|cursor| seq <= *cursor) {
                continue;
            }
            // The receiver and deduplication window are both bounded.
            if cursors.len() >= 256 {
                cursors.clear();
            }
            cursors.insert(id.to_string(), seq);
            let event = StreamEvent {
                event: event.event_type.into(),
                data: event.data,
                id: None,
                timestamp: Some(Utc::now()),
            };
            if tokio::select! {
                _ = cancel.cancelled() => return,
                result = send_ws_event(&tx, request_id.as_deref(), event) => result,
            }
            .is_err()
            {
                return;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct WsQuery {
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WsEnvelope {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub payload: Option<Value>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct WsClientInfo {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct WsConnectPayload {
    #[serde(default)]
    pub protocol_version: Option<i32>,
    #[serde(default)]
    pub min_protocol_version: Option<i32>,
    #[serde(default)]
    pub max_protocol_version: Option<i32>,
    #[serde(default)]
    pub client: Option<WsClientInfo>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct WsProtocolInfo {
    pub version: i32,
    pub min: i32,
    pub max: i32,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct WsPolicy {
    pub max_message_bytes: usize,
    pub stream_queue_size: usize,
    pub slow_client_queue_watermark: usize,
}

impl WsPolicy {
    pub(crate) fn default_policy() -> Self {
        Self {
            max_message_bytes: WS_MAX_MESSAGE_BYTES,
            stream_queue_size: STREAM_EVENT_QUEUE_SIZE,
            slow_client_queue_watermark: STREAM_EVENT_SLOW_CLIENT_QUEUE_WATERMARK,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct WsFeatures {
    pub multiplex: bool,
    pub resume: bool,
    pub watch: bool,
    pub ping_pong: bool,
    pub goal: bool,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct WsReadyPayload {
    pub connection_id: String,
    pub server_time: f64,
    pub protocol: WsProtocolInfo,
    pub policy: WsPolicy,
    pub features: WsFeatures,
}

#[derive(Debug, Clone)]
pub(crate) struct WsHandshakeInfo {
    pub client: Option<WsClientInfo>,
    pub client_min: i32,
    pub client_max: i32,
}

#[derive(Debug)]
pub(crate) enum WsHandshakeError {
    InvalidRange { min: i32, max: i32 },
    ProtocolMismatch { min: i32, max: i32, server: i32 },
}

impl WsHandshakeError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::InvalidRange { .. } => "INVALID_PROTOCOL_RANGE",
            Self::ProtocolMismatch { .. } => "PROTOCOL_MISMATCH",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::InvalidRange { min, max } => {
                format!("invalid protocol range: min={min}, max={max}")
            }
            Self::ProtocolMismatch { min, max, server } => {
                format!("protocol mismatch: server={server}, client_range={min}..{max}")
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct WsSender {
    tx: mpsc::Sender<Message>,
    slow_warn_at: Arc<AtomicU64>,
}

impl WsSender {
    pub(crate) fn new(tx: mpsc::Sender<Message>) -> Self {
        Self {
            tx,
            slow_warn_at: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn remaining_capacity(&self) -> usize {
        self.tx.capacity()
    }
}

/// v2 durable feeder. Every frame comes from completed canonical history and is
/// sent through the connection's bounded writer queue. If the writer is slow,
/// stop this subscription; the client resumes from its last applied seq.
pub(crate) async fn resume_thread_changes_v2(
    state: Arc<AppState>,
    session_id: String,
    after_change_seq: i64,
    request_id: Option<&str>,
    tx: WsSender,
    cancel: Option<CancellationToken>,
) {
    use crate::services::thread_change_feeder::{watch_thread_changes, ThreadChangeFrame};
    let Ok(mut frames) =
        watch_thread_changes(state, session_id.clone(), after_change_seq, cancel.clone()).await
    else {
        let _ = send_ws_error(
            &tx,
            request_id,
            "THREAD_RECOVERY_FAILED",
            "change feed unavailable".into(),
        )
        .await;
        return;
    };
    let mut sent_seq = after_change_seq.max(0);
    loop {
        let frame = if let Some(token) = cancel.as_ref() {
            tokio::select! {
                _ = token.cancelled() => return,
                frame = frames.recv() => frame,
            }
        } else {
            frames.recv().await
        };
        let Some(frame) = frame else {
            return;
        };
        if tx.remaining_capacity() <= STREAM_EVENT_SLOW_CLIENT_QUEUE_WATERMARK {
            let overflow = StreamEvent {
                event: "stream_overflow".into(),
                data: json!({"session_id":session_id,"cursor":sent_seq,"resume_recommended":true}),
                id: None,
                timestamp: Some(Utc::now()),
            };
            // The queue has already crossed the durable-feed watermark.
            // This is best effort only: waiting here would keep an obsolete
            // subscription alive behind a slow writer.
            let _ = try_send_ws_event(&tx, request_id, overflow);
            return;
        }
        let (event, frame_seq) = match frame {
            ThreadChangeFrame::Change { seq, event, data } => (
                StreamEvent {
                    event,
                    data,
                    id: None,
                    timestamp: Some(Utc::now()),
                },
                seq,
            ),
            ThreadChangeFrame::SnapshotRequired { data } => (
                StreamEvent {
                    event: "thread_snapshot_required".into(),
                    data,
                    id: None,
                    timestamp: Some(Utc::now()),
                },
                0,
            ),
            ThreadChangeFrame::Overflow {
                cursor,
                resume_recommended,
            } => (
                StreamEvent {
                    event: "stream_overflow".into(),
                    data: json!({"session_id":session_id,"cursor":cursor,"resume_recommended":resume_recommended}),
                    id: None,
                    timestamp: Some(Utc::now()),
                },
                0,
            ),
        };
        let control = matches!(
            event.event.as_str(),
            "thread_snapshot_required" | "stream_overflow"
        );
        if try_send_ws_event(&tx, request_id, event).is_err() {
            // The capacity check above and this enqueue are intentionally
            // separate. A concurrent socket task may fill the last slot, in
            // which case stop immediately; the durable cursor remains the
            // recovery point and an overflow frame is only best effort.
            let overflow = StreamEvent {
                event: "stream_overflow".into(),
                data: json!({"session_id":session_id,"cursor":sent_seq,"resume_recommended":true}),
                id: None,
                timestamp: Some(Utc::now()),
            };
            let _ = try_send_ws_event(&tx, request_id, overflow);
            return;
        }
        if frame_seq > 0 {
            sent_seq = frame_seq;
        }
        if control {
            return;
        }
    }
}

pub(crate) async fn send_ws_display_event(
    tx: &WsSender,
    request_id: Option<&str>,
    event: StreamEvent,
    cancel: Option<&CancellationToken>,
) -> Result<(), ()> {
    tokio::select! {
        result = send_ws_event(tx, request_id, event) => result,
        _ = async { match cancel {
            Some(cancel) => cancel.cancelled().await,
            None => std::future::pending().await,
        }} => Err(()),
    }
}

/// Browser display subscription: bounded row revisions, no raw log replay.
pub(crate) async fn resume_thread_view(
    state: Arc<AppState>,
    session_id: String,
    user_id: String,
    mut after: i64,
    live_events: bool,
    request_id: Option<&str>,
    tx: WsSender,
    cancel: Option<CancellationToken>,
) {
    let mut wake = state.kernel.orchestrator.change_hub.subscribe(&session_id);
    let mut live = live_events.then(|| {
        state
            .kernel
            .orchestrator
            .change_hub
            .subscribe_live(&session_id)
    });
    let mut goal_wake = state
        .kernel
        .orchestrator
        .change_hub
        .subscribe_goal_updates(&session_id);
    let mut last_goal = None;
    let mut last_runtime = None;
    let mut last_row_controls = None;
    loop {
        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            return;
        }
        let storage = state.storage.clone();
        let session = session_id.clone();
        let user = user_id.clone();
        let result = blocking::run_view("api.ws.thread_view", move || {
            let page = storage.thread_view_changes(&user, &session, after)?;
            let goal = storage.get_session_goal(&user, &session)?;
            Ok((page, goal))
        })
        .await;
        // Goal snapshots are session controls rather than timeline rows. The
        // display subscription must carry them as well, including terminal
        // changes after the last chat row and disarmed state after reconnect.
        let result = match result {
            Ok((page, record)) => {
                let service = state.kernel.orchestrator.goal_handle();
                let goal = record
                    .map(|record| {
                        crate::services::goal::goal_payload(&crate::services::goal::GoalView {
                            activation: service.activation(&session_id),
                            record,
                        })
                    })
                    .unwrap_or(Value::Null);
                if last_goal.as_ref() != Some(&goal) {
                    if send_ws_display_event(
                        &tx,
                        request_id,
                        StreamEvent {
                            event: "goal_updated".into(),
                            data: json!({"session_id": session_id, "goal": goal}),
                            id: None,
                            timestamp: Some(Utc::now()),
                        },
                        cancel.as_ref(),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                    last_goal = Some(goal);
                }
                Ok(page)
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(page) if page.reset_required => {
                let _ = send_ws_display_event(
                    &tx,
                    request_id,
                    StreamEvent {
                        event: "thread_view_required".into(),
                        data: json!({"session_id":session_id}),
                        id: None,
                        timestamp: Some(Utc::now()),
                    },
                    cancel.as_ref(),
                )
                .await;
                return;
            }
            Ok(page) => {
                // Queue metadata and row status can change without advancing
                // completed history or changing the active execution. Compare
                // only bounded control fields; body changes use view_seq.
                let row_controls = page
                    .rows
                    .iter()
                    .map(|row| {
                        json!([
                            row.root_turn_id,
                            row.execution_turn_id,
                            row.status,
                            row.queued_input,
                            row.stop_reason,
                            row.assistant.feedback,
                        ])
                    })
                    .collect::<Vec<_>>();
                let runtime = (
                    page.runtime_status.clone(),
                    page.status_turn_id.clone(),
                    page.active_turn_id.clone(),
                );
                let changed = page.view_seq > after
                    || last_runtime.as_ref() != Some(&runtime)
                    || last_row_controls.as_ref() != Some(&row_controls);
                if page.epoch >= 0 && changed {
                    after = page.view_seq;
                    last_runtime = Some(runtime);
                    last_row_controls = Some(row_controls);
                    if send_ws_display_event(
                        &tx,
                        request_id,
                        StreamEvent {
                            event: "thread_view_change".into(),
                            data: serde_json::to_value(page).unwrap_or(Value::Null),
                            id: None,
                            timestamp: Some(Utc::now()),
                        },
                        cancel.as_ref(),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
            }
            Err(_) => {
                let _ = send_ws_display_event(
                    &tx,
                    request_id,
                    StreamEvent {
                        event: "thread_view_required".into(),
                        data: json!({"session_id":session_id}),
                        id: None,
                        timestamp: Some(Utc::now()),
                    },
                    cancel.as_ref(),
                )
                .await;
                return;
            }
        }
        let cancellation = async {
            if let Some(c) = cancel.as_ref() {
                c.cancelled().await
            } else {
                std::future::pending::<()>().await
            }
        };
        tokio::pin!(cancellation);
        loop {
            tokio::select! {
                _ = &mut cancellation => return,
                _ = tx.tx.closed() => return,
                changed = wake.changed() => { if changed.is_err() { return; } break; },
                changed = goal_wake.changed() => { if changed.is_err() { return; } break; },
                incoming = async { match live.as_mut() {
                    Some(receiver) => receiver.recv().await,
                    None => std::future::pending().await,
                }} => match incoming {
                    Ok(event) => {
                        let structural = event.event == "thread_process_item";
                        if send_ws_tail_event(&tx, request_id, event).await.is_err() && structural {
                            tokio::select! {
                            _ = &mut cancellation => return,
                            result = send_ws_event(&tx, request_id, StreamEvent {
                                event: "thread_view_required".into(), data: json!({"session_id":session_id}),
                                id: None, timestamp: Some(Utc::now()),
                            }) => if result.is_err() { return; },
                            }
                            if let Some(receiver) = live.as_mut() { *receiver = receiver.resubscribe(); }
                            break;
                        }
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        tokio::select! {
                        _ = &mut cancellation => return,
                        result = send_ws_event(&tx, request_id, StreamEvent {
                            event: "thread_view_required".into(), data: json!({"session_id":session_id}),
                            id: None, timestamp: Some(Utc::now()),
                        }) => if result.is_err() { return; },
                        }
                        if let Some(receiver) = live.as_mut() { *receiver = receiver.resubscribe(); }
                        break;
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                },
            }
        }
    }
}

/// The queue owns execution. The browser receives only bounded display rows.
pub(crate) async fn resume_queued_thread_view(
    state: Arc<AppState>,
    session: String,
    user: String,
    queue: String,
    after: i64,
    request: Option<&str>,
    tx: WsSender,
    cancel: Option<CancellationToken>,
) {
    let feeder_cancel = cancel.clone().unwrap_or_default().child_token();
    let feeder = resume_thread_view(
        state.clone(),
        session.clone(),
        user.clone(),
        after,
        true,
        request,
        tx.clone(),
        Some(feeder_cancel.clone()),
    );
    tokio::pin!(feeder);
    let mut feeder_finished = false;
    let mut last_task_status = String::new();
    let mut wake = state.kernel.orchestrator.change_hub.subscribe(&session);
    loop {
        wake.borrow_and_update();
        let users = state.user_store.clone();
        let id = queue.clone();
        let task = tokio::select! {
            _ = &mut feeder, if !feeder_finished => {
                feeder_finished = true;
                continue;
            },
            _ = tx.tx.closed() => return,
            _ = async {
                if let Some(token) = cancel.as_ref() {
                    token.cancelled().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => return,
            result = blocking::run_db("api.ws.thread_view.queue", move || {
                users.get_agent_task(&id)
            }) => result,
        };
        let task = match task {
            Ok(task) => task,
            Err(_) => {
                tokio::select! {
                    _ = tx.tx.closed() => return,
                    _ = async {
                        if let Some(token) = cancel.as_ref() {
                            token.cancelled().await;
                        } else {
                            std::future::pending::<()>().await;
                        }
                    } => return,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => continue,
                }
            }
        };
        if let Some(task) = task {
            if task.session_id != session {
                return;
            }
            if matches!(
                task.status.as_str(),
                "success" | "failed" | "cancelled" | "dead"
            ) {
                feeder_cancel.cancel();
                send_final_thread_view(&state, &session, &user, request, &tx).await;
                let _ = try_send_ws_event(
                    &tx,
                    request,
                    StreamEvent {
                        event: "transport_final".into(),
                        data: json!({"session_id":session}),
                        id: None,
                        timestamp: Some(Utc::now()),
                    },
                );
                return;
            }
            // Running is control state and may keep the same canonical
            // cursor. Send its bounded snapshot immediately; subsequent
            // canonical commits wake the feeder independently.
            if task.status == "running" && last_task_status != "running" {
                send_final_thread_view(&state, &session, &user, request, &tx).await;
            }
            last_task_status = task.status.clone();
        }
        tokio::select! {
            _ = &mut feeder, if !feeder_finished => {
                feeder_finished = true;
                continue;
            },
            _ = tx.tx.closed() => return,
            _ = async {
                if let Some(token) = cancel.as_ref() {
                    token.cancelled().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => return,
            changed = wake.changed() => if changed.is_err() { return; },
        }
    }
}

pub(crate) async fn send_final_thread_view(
    state: &Arc<AppState>,
    session: &str,
    user: &str,
    request: Option<&str>,
    tx: &WsSender,
) {
    let storage = state.storage.clone();
    let session = session.to_string();
    let user = user.to_string();
    if let Ok(page) = blocking::run_view("api.ws.thread_view.final", move || {
        storage.thread_view_page(&user, &session, "chat", None, 12)
    })
    .await
    {
        let mut data = json!({"runtime_status":page.runtime_status,"status_turn_id":page.status_turn_id,"active_turn_id":page.active_turn_id,"epoch":page.epoch,"view_seq":page.view_seq,"rows":page.chat,"reset_required":page.projection_state!="ready"});
        if serde_json::to_vec(&data)
            .map(|v| v.len() > 60 * 1024)
            .unwrap_or(true)
        {
            data["rows"] = json!([]);
            data["reset_required"] = json!(true);
        }
        let _ = send_ws_event(
            tx,
            request,
            StreamEvent {
                event: "thread_view_change".into(),
                data,
                id: None,
                timestamp: Some(Utc::now()),
            },
        )
        .await;
    }
}

/// Queued v2 start uses the same durable feeder as normal turns, but stops
/// after the matching queue Item reaches a terminal state. All frames are
/// still forwarded to the session reducer so queue transitions and any
/// interleaved durable work remain ordered by `change_seq`.
pub(crate) async fn resume_queued_thread_changes_v2(
    state: Arc<AppState>,
    session_id: String,
    queue_id: String,
    after_change_seq: i64,
    request_id: Option<&str>,
    tx: WsSender,
    cancel: Option<CancellationToken>,
) {
    use crate::services::thread_change_feeder::{watch_thread_changes, ThreadChangeFrame};
    let Ok(mut frames) =
        watch_thread_changes(state, session_id.clone(), after_change_seq, cancel.clone()).await
    else {
        let _ = send_ws_error(
            &tx,
            request_id,
            "THREAD_RECOVERY_FAILED",
            "change feed unavailable".into(),
        )
        .await;
        return;
    };
    loop {
        let frame = if let Some(token) = cancel.as_ref() {
            tokio::select! {
                _ = token.cancelled() => return,
                frame = frames.recv() => frame,
            }
        } else {
            frames.recv().await
        };
        let Some(frame) = frame else { return };
        if tx.remaining_capacity() <= STREAM_EVENT_SLOW_CLIENT_QUEUE_WATERMARK {
            let _ = try_send_ws_event(
                &tx,
                request_id,
                StreamEvent {
                    event: "stream_overflow".into(),
                    data: json!({"session_id":session_id,"resume_recommended":true}),
                    id: None,
                    timestamp: Some(Utc::now()),
                },
            );
            return;
        }
        let (event, terminal) = match frame {
            ThreadChangeFrame::Change { event, data, .. } => {
                let payload = data.get("payload").unwrap_or(&data);
                let terminal = payload.get("kind").and_then(Value::as_str) == Some("queue")
                    && payload
                        .get("item_id")
                        .and_then(Value::as_str)
                        .is_some_and(|item| item.ends_with(&format!(":queue-{queue_id}")))
                    && matches!(
                        payload.get("status").and_then(Value::as_str),
                        Some("completed" | "failed" | "cancelled" | "dead" | "success")
                    );
                (
                    StreamEvent {
                        event,
                        data,
                        id: None,
                        timestamp: Some(Utc::now()),
                    },
                    terminal,
                )
            }
            ThreadChangeFrame::SnapshotRequired { data } => (
                StreamEvent {
                    event: "thread_snapshot_required".into(),
                    data,
                    id: None,
                    timestamp: Some(Utc::now()),
                },
                true,
            ),
            ThreadChangeFrame::Overflow {
                cursor,
                resume_recommended,
            } => (
                StreamEvent {
                    event: "stream_overflow".into(),
                    data: json!({"session_id":session_id,"cursor":cursor,"resume_recommended":resume_recommended}),
                    id: None,
                    timestamp: Some(Utc::now()),
                },
                true,
            ),
        };
        if try_send_ws_event(&tx, request_id, event).is_err() || terminal {
            return;
        }
    }
}

pub(crate) fn apply_ws_auth_headers(headers: &HeaderMap, query: &WsQuery) -> HeaderMap {
    let mut auth_headers = headers.clone();
    if auth_headers.get(AUTHORIZATION).is_none() {
        let token = extract_ws_protocol_token(headers).or_else(|| {
            query
                .access_token
                .as_deref()
                .or(query.token.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        });
        if let Some(token) = token {
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
                auth_headers.insert(AUTHORIZATION, value);
            }
        }
    }
    auth_headers
}

pub(crate) fn resolve_session_id(
    primary: Option<String>,
    secondary: Option<String>,
) -> Option<String> {
    primary
        .and_then(|value| {
            if value.trim().is_empty() {
                None
            } else {
                Some(value)
            }
        })
        .or_else(|| {
            secondary.and_then(|value| {
                if value.trim().is_empty() {
                    None
                } else {
                    Some(value)
                }
            })
        })
}

pub(crate) fn ws_protocol_info() -> WsProtocolInfo {
    WsProtocolInfo {
        version: WS_PROTOCOL_VERSION,
        min: WS_PROTOCOL_MIN_VERSION,
        max: WS_PROTOCOL_MAX_VERSION,
    }
}

pub(crate) fn parse_connect_payload(
    payload: Option<Value>,
) -> Result<WsConnectPayload, WsPayloadError> {
    match payload {
        Some(payload) => serde_json::from_value(payload)
            .map_err(|err| WsPayloadError::Invalid(format!("invalid payload: {err}"))),
        None => Ok(WsConnectPayload::default()),
    }
}

pub(crate) fn negotiate_ws_protocol(
    payload: &WsConnectPayload,
) -> Result<WsHandshakeInfo, WsHandshakeError> {
    let mut min = payload.min_protocol_version;
    let mut max = payload.max_protocol_version;
    if min.is_none() && max.is_none() {
        if let Some(version) = payload.protocol_version {
            min = Some(version);
            max = Some(version);
        }
    }
    let min = min.unwrap_or(WS_PROTOCOL_VERSION);
    let max = max.unwrap_or(WS_PROTOCOL_VERSION);
    if min <= 0 || max <= 0 || min > max {
        return Err(WsHandshakeError::InvalidRange { min, max });
    }
    if WS_PROTOCOL_VERSION < min || WS_PROTOCOL_VERSION > max {
        return Err(WsHandshakeError::ProtocolMismatch {
            min,
            max,
            server: WS_PROTOCOL_VERSION,
        });
    }
    Ok(WsHandshakeInfo {
        client: payload.client.clone(),
        client_min: min,
        client_max: max,
    })
}

#[derive(Debug)]
pub(crate) enum WsPayloadError {
    Missing,
    Invalid(String),
}

impl WsPayloadError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Missing => "PAYLOAD_REQUIRED",
            Self::Invalid(_) => "INVALID_PAYLOAD",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::Missing => i18n::t("error.param_required"),
            Self::Invalid(message) => message.clone(),
        }
    }
}

pub(crate) fn parse_payload<T: for<'de> Deserialize<'de>>(
    payload: Option<Value>,
) -> Result<T, WsPayloadError> {
    let Some(payload) = payload else {
        return Err(WsPayloadError::Missing);
    };
    serde_json::from_value(payload)
        .map_err(|err| WsPayloadError::Invalid(format!("invalid payload: {err}")))
}

fn build_ws_text(kind: &str, request_id: Option<&str>, payload: Option<Value>) -> String {
    let mut map = serde_json::Map::new();
    map.insert("type".to_string(), json!(kind));
    if let Some(request_id) = request_id {
        map.insert("request_id".to_string(), json!(request_id));
    }
    if let Some(payload) = payload {
        map.insert("payload".to_string(), payload);
    }
    Value::Object(map).to_string()
}

fn should_warn_slow_client(sender: &WsSender) -> bool {
    let now = Utc::now().timestamp().max(0) as u64;
    let last = sender.slow_warn_at.load(Ordering::Relaxed);
    if now.saturating_sub(last) < STREAM_EVENT_SLOW_CLIENT_WARN_INTERVAL_S {
        return false;
    }
    sender.slow_warn_at.store(now, Ordering::Relaxed);
    true
}

enum WsTrySendError {
    Full,
    Closed,
}

fn try_send_text_lossy(sender: &WsSender, text: String) -> Result<(), WsTrySendError> {
    match sender.tx.try_send(Message::Text(text.into())) {
        Ok(()) => Ok(()),
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Ok(()),
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Err(WsTrySendError::Closed),
    }
}

fn try_send_text_strict(sender: &WsSender, text: String) -> Result<(), WsTrySendError> {
    match sender.tx.try_send(Message::Text(text.into())) {
        Ok(()) => Ok(()),
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => Err(WsTrySendError::Full),
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => Err(WsTrySendError::Closed),
    }
}

async fn send_text_backpressured(sender: &WsSender, text: String) -> Result<(), WsTrySendError> {
    sender
        .tx
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| WsTrySendError::Closed)
}

fn send_slow_client_warning(
    sender: &WsSender,
    request_id: Option<&str>,
    reason: &str,
    queue_capacity: usize,
    strict: bool,
) -> Result<(), WsTrySendError> {
    if !should_warn_slow_client(sender) {
        return Ok(());
    }
    let warning_payload = json!({
        "event": "slow_client",
        "data": {
            "reason": reason,
            "queue_capacity": queue_capacity,
            "resume_recommended": true,
            "ts": Utc::now().to_rfc3339(),
        },
    });
    let warning_text = build_ws_text("event", request_id, Some(warning_payload));
    if strict {
        try_send_text_strict(sender, warning_text)
    } else {
        try_send_text_lossy(sender, warning_text)
    }
}

pub(crate) async fn send_ws_ready(
    tx: &WsSender,
    request_id: Option<&str>,
    payload: WsReadyPayload,
) -> Result<(), ()> {
    send_ws_message(tx, "ready", request_id, Some(json!(payload))).await
}

pub(crate) async fn send_ws_pong(tx: &WsSender) -> Result<(), ()> {
    send_ws_message(
        tx,
        "pong",
        None,
        Some(json!({ "ts": Utc::now().timestamp() })),
    )
    .await
}

/// Attempt one event enqueue without waiting for writer capacity. Durable
/// feeders use this after their watermark check so a competing sender cannot
/// turn a slow WebSocket into an unbounded task stall.
pub(crate) fn try_send_ws_event(
    tx: &WsSender,
    request_id: Option<&str>,
    event: StreamEvent,
) -> Result<(), ()> {
    let event_name = event.event.clone();
    let event_id = event.id.clone();
    let data = enrich_ws_event_data(event.data, event_id.as_deref());
    let payload = json!({
        "event": event_name,
        "id": event_id,
        "data": data,
    });
    let text = build_ws_text("event", request_id, Some(payload));
    try_send_text_strict(tx, text).map_err(|_| ())
}

pub(crate) async fn send_ws_event(
    tx: &WsSender,
    request_id: Option<&str>,
    event: StreamEvent,
) -> Result<(), ()> {
    let event_name = event.event.clone();
    let is_delta = event_name == "llm_output_delta";
    let event_id = event.id.clone();
    let data = enrich_ws_event_data(event.data, event_id.as_deref());
    let payload = json!({
        "event": event_name,
        "id": event_id,
        "data": data,
    });
    if is_delta {
        let text = build_ws_text("event", request_id, Some(payload));
        return send_text_backpressured(tx, text).await.map_err(|_| ());
    }
    let text = build_ws_text("event", request_id, Some(payload.clone()));
    match try_send_text_strict(tx, text) {
        Ok(()) => Ok(()),
        Err(WsTrySendError::Full) => {
            let _ = send_slow_client_warning(
                tx,
                request_id,
                "queue_full_resume_required",
                tx.tx.capacity(),
                false,
            );
            let retry_text = build_ws_text("event", request_id, Some(payload));
            send_text_backpressured(tx, retry_text)
                .await
                .map_err(|_| ())
        }
        Err(WsTrySendError::Closed) => Err(()),
    }
}

/// Text tails are best-effort and healed by offset-aware durable snapshots.
/// Structural process frames instead wait for bounded queue capacity.
pub(crate) async fn send_ws_tail_event(
    tx: &WsSender,
    request_id: Option<&str>,
    event: StreamEvent,
) -> Result<(), ()> {
    let event_name = event.event.clone();
    let event_id = event.id.clone();
    let data = enrich_ws_event_data(event.data, event_id.as_deref());
    let payload = json!({
        "event": event_name,
        "id": event_id,
        "data": data,
    });
    let text = build_ws_text("event", request_id, Some(payload));
    if event_name == "thread_process_item" {
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            send_text_backpressured(tx, text),
        )
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
    } else {
        try_send_text_strict(tx, text).map(|_| ()).map_err(|_| ())
    }
}

pub(crate) async fn send_ws_live_event(
    tx: &WsSender,
    request_id: &str,
    event: StreamEvent,
) -> Result<(), ()> {
    if tx.tx.capacity() <= STREAM_EVENT_SLOW_CLIENT_QUEUE_WATERMARK {
        // Detach this live delivery with an explicit replay signal. Replay itself
        // stays lossless; silently dropping deltas there would skip its cursor.
        let warning = build_ws_text(
            "event",
            Some(request_id),
            Some(json!({
                "event": "slow_client",
                "data": {
                    "reason": "queue_full_resume_required",
                    "resume_recommended": true
                }
            })),
        );
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            send_text_backpressured(tx, warning),
        )
        .await;
        return Err(());
    }
    // A competing request on the same socket must not stall the model forever.
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        send_ws_event(tx, Some(request_id), event),
    )
    .await
    .map_err(|_| ())?
}

fn enrich_ws_event_data(data: Value, event_id: Option<&str>) -> Value {
    let Some(parsed_id) = event_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<i64>().ok())
    else {
        return data;
    };
    let mut data = data;
    if let Value::Object(ref mut map) = data {
        map.entry("event_id".to_string())
            .or_insert_with(|| json!(parsed_id));
        map.entry("event_seq".to_string())
            .or_insert_with(|| json!(parsed_id));
        if let Some(Value::Object(inner)) = map.get_mut("data") {
            inner
                .entry("event_id".to_string())
                .or_insert_with(|| json!(parsed_id));
            inner
                .entry("event_seq".to_string())
                .or_insert_with(|| json!(parsed_id));
        }
    }
    data
}

pub(crate) async fn send_ws_error(
    tx: &WsSender,
    request_id: Option<&str>,
    code: &str,
    message: String,
) -> Result<(), ()> {
    let payload = json!({
        "code": code,
        "message": message,
    });
    send_ws_error_payload(tx, request_id, payload).await
}

pub(crate) async fn send_ws_error_payload(
    tx: &WsSender,
    request_id: Option<&str>,
    payload: Value,
) -> Result<(), ()> {
    let payload = finalize_ws_error_payload(payload);
    send_ws_message(tx, "error", request_id, Some(payload)).await
}

pub(crate) fn ws_error_payload_from_anyhow(err: &Error) -> Value {
    if let Some(orchestrator_err) = err.downcast_ref::<crate::orchestrator::OrchestratorError>() {
        return orchestrator_err.to_payload();
    }
    json!({
        "status": StatusCode::BAD_REQUEST.as_u16(),
        "code": "INTERNAL_ERROR",
        "message": err.to_string(),
    })
}

fn finalize_ws_error_payload(payload: Value) -> Value {
    let status = payload
        .get("status")
        .and_then(Value::as_u64)
        .and_then(|raw| u16::try_from(raw).ok())
        .and_then(|raw| StatusCode::from_u16(raw).ok())
        .or_else(|| {
            payload
                .get("code")
                .and_then(Value::as_str)
                .map(crate::api::errors::status_for_error_code)
        })
        .unwrap_or(StatusCode::BAD_REQUEST);
    let code = payload.get("code").and_then(Value::as_str);
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| status.to_string());
    let hint = payload
        .get("hint")
        .and_then(Value::as_str)
        .or_else(|| code.and_then(crate::api::errors::hint_for_error_code));
    let mut merged = crate::api::errors::build_error_meta(status, code, message, hint).to_value();
    if let (Value::Object(target), Value::Object(source)) = (&mut merged, &payload) {
        for key in ["detail", "error_meta", "trace_id", "timestamp"] {
            if let Some(value) = source.get(key) {
                target.insert(key.to_string(), value.clone());
            }
        }
    }
    merged
}

pub(crate) async fn send_ws_message(
    tx: &WsSender,
    kind: &str,
    request_id: Option<&str>,
    payload: Option<Value>,
) -> Result<(), ()> {
    let text = build_ws_text(kind, request_id, payload);
    try_send_text_strict(tx, text).map_err(|_| ())
}

pub(crate) async fn resume_stream_events(
    state: Arc<AppState>,
    session_id: String,
    after_change_seq: i64,
    request_id: Option<&str>,
    tx: WsSender,
    cancel: Option<CancellationToken>,
    _keep_alive: bool,
    _notify: Option<tokio::sync::watch::Receiver<i64>>,
) {
    resume_thread_changes_v2(state, session_id, after_change_seq, request_id, tx, cancel).await;
}

pub(crate) async fn resume_queued_stream_events(
    state: Arc<AppState>,
    session_id: String,
    queue_id: String,
    after_change_seq: i64,
    request_id: Option<&str>,
    tx: WsSender,
    cancel: Option<CancellationToken>,
) {
    // Queue lifecycle entries are immutable canonical items, so they use the
    // same event-driven replay as normal history. The queue id narrows only
    // the terminal boundary; unrelated thread events retain their order.
    resume_queued_thread_changes_v2(
        state,
        session_id,
        queue_id,
        after_change_seq,
        request_id,
        tx,
        cancel,
    )
    .await;
}

pub(crate) fn has_ws_protocol_token(headers: &HeaderMap) -> bool {
    extract_ws_protocol_token(headers).is_some()
}

fn extract_ws_protocol_token(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(SEC_WEBSOCKET_PROTOCOL)?.to_str().ok()?;
    for raw in header.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        if let Some(rest) = item.strip_prefix("wunder-auth.") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Some(rest) = item.strip_prefix("access_token.") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Some(rest) = item.strip_prefix("token.") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config_store::ConfigStore;
    use crate::state::{AppState, AppStateInitOptions};
    use serde_json::json;
    use std::time::Duration;

    async fn build_test_state(name: &str) -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_dir.path().to_path_buf();
        let mut config = Config::default();
        config.storage.backend = "sqlite".to_string();
        config.storage.data_root = root.join(format!("{name}")).to_string_lossy().to_string();
        config.workspace.root = root.join("workspaces").to_string_lossy().to_string();
        config.skills.enabled.clear();
        let config_store = ConfigStore::new(root.join("wunder.yaml"));
        let config_for_store = config.clone();
        config_store
            .update(|current| *current = config_for_store.clone())
            .await
            .expect("write config");
        let state = Arc::new(
            AppState::new_with_options(
                config_store,
                config,
                AppStateInitOptions::cli_default().with_start_thread_runtime(false),
            )
            .expect("create app state"),
        );
        (state, temp_dir)
    }

    fn parse_ws_event_type(raw: &str) -> String {
        let payload: Value = serde_json::from_str(raw).expect("parse ws payload");
        payload
            .get("payload")
            .and_then(|value| value.get("event"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queued_transport_final_survives_display_feeder_reset() {
        let (state, _dir) = build_test_state("queue-reset-fixture").await;
        state
            .storage
            .insert_agent_task(&crate::storage::AgentTaskRecord {
                task_id: "fixture-task".into(),
                thread_id: "thread_fixture-thread".into(),
                user_id: "fixture-user".into(),
                agent_id: String::new(),
                session_id: "fixture-thread".into(),
                status: "pending".into(),
                request_payload: json!({}),
                request_id: None,
                retry_count: 0,
                retry_at: 1.,
                created_at: 1.,
                updated_at: 1.,
                started_at: None,
                finished_at: None,
                last_error: None,
            })
            .unwrap();
        let (sender, mut receiver) = mpsc::channel(64);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(resume_queued_thread_view(
            state.clone(),
            "fixture-thread".into(),
            "fixture-user".into(),
            "fixture-task".into(),
            i64::MAX,
            Some("fixture-request"),
            WsSender::new(sender),
            Some(cancel.clone()),
        ));
        let read_event = async {
            loop {
                let Some(Message::Text(raw)) = receiver.recv().await else {
                    panic!("queue watcher closed before display recovery");
                };
                if parse_ws_event_type(&raw) == "thread_view_required" {
                    break;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), read_event)
            .await
            .expect("display reset must be observable");
        state
            .storage
            .update_agent_task_status(crate::storage::UpdateAgentTaskStatusParams {
                task_id: "fixture-task",
                status: "success",
                retry_count: 0,
                retry_at: 1.,
                started_at: Some(1.),
                finished_at: Some(2.),
                last_error: None,
                updated_at: 2.,
            })
            .unwrap();
        state
            .kernel
            .orchestrator
            .change_hub
            .publish("fixture-thread", 0);
        let read_final = async {
            loop {
                let Some(Message::Text(raw)) = receiver.recv().await else {
                    panic!("queue watcher closed without final");
                };
                if parse_ws_event_type(&raw) == "transport_final" {
                    break;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(5), read_final)
            .await
            .expect("queue control must outlive display recovery");
        task.await.unwrap();
        cancel.cancel();
    }

    #[tokio::test]
    async fn display_watch_projects_goal_changes_without_chat_rows() {
        let (state, _dir) = build_test_state("goal-display-fixture").await;
        let user = "fixture-user";
        let session = "fixture-thread";
        state
            .storage
            .upsert_chat_session(&crate::storage::ChatSessionRecord {
                session_id: session.into(),
                user_id: user.into(),
                title: "fixture".into(),
                status: "active".into(),
                created_at: 1.,
                updated_at: 1.,
                last_message_at: 1.,
                agent_id: None,
                workspace_id: None,
                tool_overrides: vec![],
                parent_session_id: None,
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            })
            .unwrap();
        let mut record = crate::storage::SessionGoalRecord {
            goal_id: "fixture-goal".into(),
            user_id: user.into(),
            session_id: session.into(),
            revision: 1,
            objective: "Complete the example task".into(),
            phase: "active".into(),
            blocked_code: None,
            blocked_message: None,
            max_goal_rounds: 8,
            rounds_started: 0,
            created_at: 1.,
            updated_at: 1.,
        };
        state.storage.upsert_session_goal(&record).unwrap();
        let page = state
            .storage
            .thread_view_page(user, session, "chat", None, 12)
            .unwrap();
        let (tx, mut rx) = mpsc::channel(64);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(resume_thread_view(
            state.clone(),
            session.into(),
            user.into(),
            page.view_seq,
            true,
            None,
            WsSender::new(tx),
            Some(cancel.clone()),
        ));
        async fn next_goal(rx: &mut mpsc::Receiver<Message>) -> Value {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let Some(Message::Text(raw)) = rx.recv().await else {
                        panic!("watch closed");
                    };
                    let value: Value = serde_json::from_str(&raw).unwrap();
                    if value["payload"]["event"] == "goal_updated" {
                        return value["payload"]["data"]["goal"].clone();
                    }
                }
            })
            .await
            .expect("goal snapshot delivered")
        }
        let initial = next_goal(&mut rx).await;
        assert_eq!(initial["phase"], "active");
        assert_eq!(initial["activation"], "disarmed");
        record.phase = "paused".into();
        record.revision = 2;
        state.storage.upsert_session_goal(&record).unwrap();
        state
            .kernel
            .orchestrator
            .change_hub
            .publish_goal_update(session, json!({"phase":"paused"}));
        assert_eq!(next_goal(&mut rx).await["phase"], "paused");
        state.storage.delete_session_goal(user, session).unwrap();
        state
            .kernel
            .orchestrator
            .change_hub
            .publish_goal_update(session, Value::Null);
        assert!(next_goal(&mut rx).await.is_null());
        cancel.cancel();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn display_watch_wakes_for_control_status_with_unchanged_rollout_cursor() {
        let (state, _dir) = build_test_state("control-display-fixture").await;
        let committer = &state.kernel.orchestrator.committer;
        let accepted = committer
            .accept_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"sample"}),
            )
            .await
            .unwrap();
        committer
            .freeze_session(
                "fixture-user",
                "fixture-thread",
                "sample prompt",
                "fixture-workspace",
            )
            .await
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        committer
            .begin_turn("fixture-user", "fixture-thread", turn, "fixture-model")
            .await
            .unwrap();
        let after = state
            .storage
            .latest_thread_change_seq_by_session("fixture-thread")
            .unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(resume_thread_view(
            state.clone(),
            "fixture-thread".into(),
            "fixture-user".into(),
            after,
            true,
            None,
            WsSender::new(tx),
            Some(cancel.clone()),
        ));
        async fn next_status(rx: &mut mpsc::Receiver<Message>, status: &str) -> Value {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let Some(Message::Text(raw)) = rx.recv().await else {
                        panic!("display closed")
                    };
                    let value: Value = serde_json::from_str(&raw).unwrap();
                    let payload = &value["payload"];
                    if payload["event"] == "thread_view_change"
                        && payload["data"]["runtime_status"] == status
                    {
                        return payload["data"].clone();
                    }
                }
            })
            .await
            .unwrap()
        }
        assert_eq!(next_status(&mut rx, "running").await["view_seq"], after);
        state.kernel.orchestrator.change_hub.publish_live(
            "fixture-thread",
            &StreamEvent {
                event: "thread_process_item".into(),
                data: json!({"item_id":"fixture-tool","status":"running"}),
                id: None,
                timestamp: None,
            },
        );
        let live = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let Some(Message::Text(raw)) = rx.recv().await else {
                    panic!("display closed")
                };
                let frame: Value = serde_json::from_str(&raw).unwrap();
                if frame["payload"]["event"] == "thread_process_item" {
                    break frame;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(live["payload"]["data"]["status"], "running");
        committer
            .update_turn(
                "fixture-user",
                "fixture-thread",
                turn,
                "waiting_input",
                "",
                &json!({}),
            )
            .await
            .unwrap();
        assert_eq!(
            next_status(&mut rx, "waiting_input").await["view_seq"],
            after
        );
        let queued = committer
            .accept_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"next sample"}),
            )
            .await
            .unwrap();
        let queued_turn = queued["turn_id"].as_str().unwrap();
        for (reason, priority) in [("wait", 0), ("promoted", 1)] {
            assert!(committer
                .update_turn(
                    "fixture-user",
                    "fixture-thread",
                    queued_turn,
                    "queued",
                    "",
                    &json!({"queued_input":{"queue_id":"fixture-task",
                    "reason":reason,"priority":priority}})
                )
                .await
                .unwrap());
            let page = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let Some(Message::Text(raw)) = rx.recv().await else {
                        panic!("display closed")
                    };
                    let value: Value = serde_json::from_str(&raw).unwrap();
                    let payload = &value["payload"];
                    if payload["event"] == "thread_view_change"
                        && payload["data"]["rows"].as_array().is_some_and(|rows| {
                            rows.iter().any(|row| {
                                row["root_turn_id"] == queued_turn
                                    && row["queued_input"]["reason"] == reason
                                    && row["queued_input"]["priority"] == priority
                            })
                        })
                    {
                        break payload["data"].clone();
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(page["view_seq"], after);
            assert_eq!(page["runtime_status"], "waiting_input");
            assert_eq!(page["active_turn_id"], turn);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), rx.recv())
                .await
                .is_err(),
            "idle view must emit no timer updates"
        );
        cancel.cancel();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn display_watch_recovers_live_overflow_without_ending_the_subscription() {
        let (state, _dir) = build_test_state("process-overflow-fixture").await;
        state
            .kernel
            .orchestrator
            .committer
            .accept_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"Fixture input"}),
            )
            .await
            .unwrap();
        let (sender, mut receiver) = mpsc::channel(1);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(resume_thread_view(
            state.clone(),
            "fixture-thread".into(),
            "fixture-user".into(),
            0,
            true,
            None,
            WsSender::new(sender),
            Some(cancel.clone()),
        ));
        async fn next_event(receiver: &mut mpsc::Receiver<Message>, expected: &str) {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let Message::Text(raw) = receiver.recv().await.unwrap() else {
                        continue;
                    };
                    if parse_ws_event_type(&raw) == expected {
                        return;
                    }
                }
            })
            .await
            .unwrap();
        }
        next_event(&mut receiver, "thread_view_change").await;
        for index in 0..200 {
            state.kernel.orchestrator.change_hub.publish_live(
                "fixture-thread",
                &StreamEvent {
                    event: "thread_process_item".into(),
                    data: json!({"order":index}),
                    id: None,
                    timestamp: None,
                },
            );
        }
        next_event(&mut receiver, "thread_view_required").await;
        tokio::task::yield_now().await;
        state.kernel.orchestrator.change_hub.publish_live(
            "fixture-thread",
            &StreamEvent {
                event: "thread_process_item".into(),
                data: json!({"item_id":"fixture-next","status":"running"}),
                id: None,
                timestamp: None,
            },
        );
        next_event(&mut receiver, "thread_process_item").await;
        assert!(!task.is_finished());
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn live_process_structure_waits_for_capacity_while_text_tails_remain_bounded() {
        let (tx, mut rx) = mpsc::channel::<Message>(1);
        tx.try_send(Message::Text("fixture busy".into())).unwrap();
        let sender = WsSender::new(tx);
        let text = StreamEvent {
            event: "thread_item_tail".into(),
            data: json!({"text":"fixture tail"}),
            id: None,
            timestamp: None,
        };
        assert_eq!(send_ws_tail_event(&sender, None, text).await, Err(()));
        let worker = tokio::spawn(async move {
            send_ws_tail_event(
                &sender,
                None,
                StreamEvent {
                    event: "thread_process_item".into(),
                    data: json!({"item_id":"fixture-tool","status":"running"}),
                    id: None,
                    timestamp: None,
                },
            )
            .await
        });
        tokio::task::yield_now().await;
        assert!(!worker.is_finished());
        rx.recv().await.unwrap();
        assert_eq!(worker.await.unwrap(), Ok(()));
        let Message::Text(raw) = rx.recv().await.unwrap() else {
            panic!("expected structure")
        };
        assert_eq!(parse_ws_event_type(&raw), "thread_process_item");
    }

    #[tokio::test]
    async fn non_delta_event_waits_for_queue_capacity_and_succeeds() {
        let (tx, mut rx) = mpsc::channel::<Message>(1);
        tx.try_send(Message::Text("busy".into()))
            .expect("fill queue");
        let sender = WsSender::new(tx);
        let drain = tokio::spawn(async move {
            let first = rx.recv().await.expect("first queued message");
            assert!(matches!(first, Message::Text(_)));
            rx.recv().await.expect("second queued message")
        });
        let event = StreamEvent {
            event: "tool_call".to_string(),
            data: json!({"tool":"read_file"}),
            id: Some("1".to_string()),
            timestamp: None,
        };
        assert!(send_ws_event(&sender, Some("req-1"), event).await.is_ok());
        let second = drain.await.expect("drain task should finish");
        let Message::Text(raw) = second else {
            panic!("expected tool_call message");
        };
        assert!(raw.contains("\"event\":\"tool_call\""));
    }

    #[tokio::test]
    async fn ws_event_data_exposes_event_sequence_for_projection_replay() {
        let (tx, mut rx) = mpsc::channel::<Message>(4);
        let sender = WsSender::new(tx);
        let event = StreamEvent {
            event: "final".to_string(),
            data: json!({"data":{"answer":"ok"}}),
            id: Some("7".to_string()),
            timestamp: None,
        };
        assert!(send_ws_event(&sender, Some("req-seq"), event).await.is_ok());
        let message = rx.recv().await.expect("ws event");
        let Message::Text(raw) = message else {
            panic!("expected text ws event");
        };
        let payload: Value = serde_json::from_str(&raw).expect("ws json");
        let data = payload
            .get("payload")
            .and_then(|value| value.get("data"))
            .expect("event data");
        assert_eq!(data.get("event_id").and_then(Value::as_i64), Some(7));
        assert_eq!(data.get("event_seq").and_then(Value::as_i64), Some(7));
        assert_eq!(
            data.get("data")
                .and_then(|value| value.get("event_seq"))
                .and_then(Value::as_i64),
            Some(7)
        );
    }

    #[tokio::test]
    async fn live_event_requests_replay_before_queue_saturation() {
        let (tx, mut rx) = mpsc::channel::<Message>(4);
        for _ in 0..2 {
            tx.try_send(Message::Text("busy".into()))
                .expect("fill queue");
        }
        let sender = WsSender::new(tx);
        let event = StreamEvent {
            event: "llm_output_delta".to_string(),
            data: json!({"reasoning_delta":"x"}),
            id: Some("2".to_string()),
            timestamp: None,
        };
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            send_ws_live_event(&sender, "req-2", event),
        )
        .await
        .expect("delivery must detach promptly");
        assert_eq!(result, Err(()));
        rx.recv().await.unwrap();
        rx.recv().await.unwrap();
        let Message::Text(raw) = rx.recv().await.unwrap() else {
            panic!("expected replay warning");
        };
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            payload,
            json!({
                "type": "event", "request_id": "req-2",
                "payload": {"event": "slow_client", "data": {
                    "reason": "queue_full_resume_required", "resume_recommended": true
                }}
            })
        );
    }

    #[tokio::test]
    async fn replay_delta_waits_for_queue_capacity_and_succeeds() {
        let (tx, mut rx) = mpsc::channel::<Message>(1);
        tx.try_send(Message::Text("busy".into())).unwrap();
        let sender = WsSender::new(tx);
        let send = tokio::spawn(async move {
            send_ws_event(
                &sender,
                Some("req-2"),
                StreamEvent {
                    event: "llm_output_delta".to_string(),
                    data: json!({"delta":"x"}),
                    id: Some("2".to_string()),
                    timestamp: None,
                },
            )
            .await
        });
        tokio::task::yield_now().await;
        assert!(!send.is_finished());
        rx.recv().await.unwrap();
        assert_eq!(send.await.unwrap(), Ok(()));
        let Message::Text(raw) = rx.recv().await.unwrap() else {
            panic!("expected replay delta");
        };
        assert_eq!(parse_ws_event_type(&raw), "llm_output_delta");
    }

    #[tokio::test]
    async fn structured_error_payload_preserves_detail_fields() {
        let (tx, mut rx) = mpsc::channel::<Message>(4);
        let sender = WsSender::new(tx);
        send_ws_error_payload(
            &sender,
            Some("req-structured"),
            json!({
                "code": "INVALID_REQUEST",
                "message": "input too long",
                "detail": {
                    "field": "input_text",
                    "max_chars": 10,
                    "actual_chars": 12
                }
            }),
        )
        .await
        .expect("send error payload");

        let Some(Message::Text(raw)) = rx.recv().await else {
            panic!("expected ws text message");
        };
        let payload: Value = serde_json::from_str(raw.as_str()).expect("parse ws envelope");
        assert_eq!(payload["type"], json!("error"));
        assert_eq!(payload["request_id"], json!("req-structured"));
        assert_eq!(payload["payload"]["code"], json!("INVALID_REQUEST"));
        assert_eq!(payload["payload"]["status"], json!(400));
        assert_eq!(payload["payload"]["detail"]["field"], json!("input_text"));
        assert_eq!(payload["payload"]["detail"]["actual_chars"], json!(12));
    }

    #[tokio::test]
    async fn exception_structured_error_payload_preserves_error_meta() {
        let (tx, mut rx) = mpsc::channel::<Message>(4);
        let sender = WsSender::new(tx);
        send_ws_error_payload(
            &sender,
            Some("req-meta"),
            json!({
                "code": "LLM_UNAVAILABLE",
                "message": "provider unavailable",
                "error_meta": {
                    "category": "provider",
                    "severity": "error",
                    "retryable": true,
                    "retry_after_ms": 500,
                    "source_stage": "llm",
                    "recovery_action": "retry_later"
                }
            }),
        )
        .await
        .expect("send error payload");

        let Some(Message::Text(raw)) = rx.recv().await else {
            panic!("expected ws text message");
        };
        let payload: Value = serde_json::from_str(raw.as_str()).expect("parse ws envelope");
        assert_eq!(payload["payload"]["code"], json!("LLM_UNAVAILABLE"));
        assert_eq!(payload["payload"]["status"], json!(503));
        assert_eq!(
            payload["payload"]["error_meta"]["category"],
            json!("provider")
        );
        assert_eq!(payload["payload"]["error_meta"]["retryable"], json!(true));
        assert_eq!(
            payload["payload"]["error_meta"]["retry_after_ms"],
            json!(500)
        );
    }
}
