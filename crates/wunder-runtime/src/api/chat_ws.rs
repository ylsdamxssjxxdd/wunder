use crate::api::chat::{build_chat_request, ChatAttachment, ChatRequestOverrides};
use crate::api::user_context::resolve_user;
use crate::api::ws_helpers::{
    apply_ws_auth_headers, has_ws_protocol_token, negotiate_ws_protocol, parse_connect_payload,
    parse_payload, resolve_session_id, resume_queued_thread_changes_v2, resume_thread_changes_v2,
    send_ws_error, send_ws_error_payload, send_ws_event, send_ws_pong, send_ws_ready,
    send_ws_tail_event, ws_error_payload_from_anyhow, ws_protocol_info, WsEnvelope, WsFeatures,
    WsPolicy, WsQuery, WsReadyPayload, WsSender, WS_MAX_MESSAGE_BYTES, WS_PROTOCOL_VERSION,
};
use crate::api::ws_log::{
    log_ws_close, log_ws_handshake, log_ws_handshake_error, log_ws_message, log_ws_open,
    log_ws_parse_error, log_ws_ready, WsConnMeta,
};
use crate::core::approval::{
    new_channel as new_approval_channel, ApprovalRequestRx, ApprovalResponse,
};
use crate::core::approval_registry::{
    ApprovalSource, PendingApprovalEntry, PendingApprovalRegistry,
};
use crate::core::long_task;
use crate::i18n;
use crate::orchestrator_constants::STREAM_EVENT_QUEUE_SIZE;
use crate::schemas::StreamEvent;
use crate::services::goal::GoalCommand;
use crate::services::runtime::thread::{QueueInfo, ThreadSubmitOutcome};
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, Router};
use chrono::Utc;
use futures::{SinkExt, StreamExt as WsStreamExt};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const WS_ENDPOINT: &str = "/wunder/chat/ws";
/// Node label of a hive browser session on this endpoint (docs §2.4).
const WEB_NODE_LABEL: &str = "web·chat";

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/chat/ws", get(chat_ws))
}

#[derive(Debug, Deserialize)]
struct WsStartPayload {
    content: String,
    #[serde(default, alias = "clientMessageId")]
    client_message_id: Option<String>,
    #[serde(default)]
    stream: Option<bool>,
    // Deprecated compatibility field: parsed for older clients but ignored;
    // request logging is unified to the compact profile.
    #[serde(default, alias = "debugPayload", alias = "debug_payload")]
    #[allow(dead_code)]
    debug_payload: bool,
    #[serde(default)]
    attachments: Option<Vec<ChatAttachment>>,
    #[serde(default)]
    tool_call_mode: Option<String>,
    #[serde(
        default,
        alias = "approvalMode",
        alias = "approval_mode",
        alias = "permissionLevel",
        alias = "permission_level"
    )]
    approval_mode: Option<String>,
    #[serde(default, alias = "reasoningEffort", alias = "reasoning_effort")]
    reasoning_effort: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsResumePayload {
    after_change_seq: Option<i64>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsWatchPayload {
    #[serde(default)]
    after_change_seq: Option<i64>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsCancelPayload {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default, alias = "reason")]
    cancel_source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsApprovalPayload {
    approval_id: String,
    decision: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WsGoalPayload {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    objective: Option<String>,
}

#[derive(Clone)]
struct WsStreamEntry {
    session_id: Option<String>,
    cancel: CancellationToken,
    task_id: String,
    cancel_session: bool,
}

fn build_queued_event_data(info: &QueueInfo) -> serde_json::Value {
    json!({
        "queued": true,
        "queue_id": info.task_id,
        "thread_id": info.thread_id,
        "session_id": info.session_id,
        "queue_ahead": info.queue_ahead,
        "queue_total": info.queue_total,
        "active_ahead": info.active_ahead,
        "wait_ahead": info.wait_ahead,
        "queue_change_seq": info.queue_change_seq,
        "queue_after_change_seq": info.queue_after_change_seq,
    })
}

async fn chat_ws(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, Response> {
    let auth_headers = apply_ws_auth_headers(&headers, &query);
    let resolved = resolve_user(&state, &auth_headers, None).await?;
    let has_protocol_token = has_ws_protocol_token(&headers);
    let conn_meta = WsConnMeta::from_headers(&headers, has_protocol_token);
    let connection_id = format!("ws_{}", Uuid::new_v4().simple());
    let session_scope = resolved.session_scope.clone();
    Ok(ws
        .protocols(["wunder"])
        .max_message_size(WS_MAX_MESSAGE_BYTES)
        .max_frame_size(WS_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| {
            handle_ws(
                socket,
                state,
                resolved.user,
                session_scope,
                connection_id,
                conn_meta,
            )
        }))
}

async fn handle_ws(
    socket: WebSocket,
    state: Arc<AppState>,
    user: crate::storage::UserAccountRecord,
    session_scope: Option<String>,
    connection_id: String,
    conn_meta: WsConnMeta,
) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(STREAM_EVENT_QUEUE_SIZE);
    let ws_tx = WsSender::new(out_tx.clone());
    let started_at = std::time::Instant::now();
    let tasks: Arc<Mutex<HashMap<String, WsStreamEntry>>> = Arc::new(Mutex::new(HashMap::new()));
    let approval_registry = state.control.approval_registry.clone();

    let writer = tokio::spawn(async move {
        while let Some(message) = out_rx.recv().await {
            if ws_sender.send(message).await.is_err() {
                break;
            }
        }
    });

    if let Some(session_scope) = session_scope.as_deref() {
        state.control.auth_sessions.register(
            &user.user_id,
            session_scope,
            &connection_id,
            out_tx.clone(),
        );
    }
    state.control.presence.connect_client(
        &user.user_id,
        &connection_id,
        Utc::now().timestamp_millis() as f64 / 1000.0,
    );
    // I2 unified presence: one volatile `web` node per accepted browser session
    // (docs §2.4). The lease is the liveness signal - holding it here for the
    // whole handler is enough, so presence costs nothing per message.
    let web_node = state.control.presence.nodes().register_web(
        &user.user_id,
        &connection_id,
        WEB_NODE_LABEL,
        Utc::now().timestamp_millis() as f64 / 1000.0,
    );
    let web_node_id = web_node.node_id().to_string();
    let presence = state.control.presence.clone();
    log_ws_open(WS_ENDPOINT, &connection_id, &user.user_id, &conn_meta);
    let now_ts = Utc::now().timestamp_millis() as f64 / 1000.0;
    let protocol = ws_protocol_info();
    let policy = WsPolicy::default_policy();
    let features = WsFeatures {
        multiplex: true,
        resume: true,
        watch: true,
        ping_pong: true,
        goal: true,
    };
    let ready_payload = WsReadyPayload {
        connection_id: connection_id.clone(),
        server_time: now_ts,
        protocol: protocol.clone(),
        policy: policy.clone(),
        features: features.clone(),
    };
    let _ = send_ws_ready(&ws_tx, None, ready_payload.clone()).await;
    log_ws_ready(
        WS_ENDPOINT,
        &connection_id,
        &user.user_id,
        protocol.version,
        protocol.min,
        protocol.max,
    );

    let mut handshake_done = false;
    let mut close_logged = false;

    while let Some(Ok(message)) = WsStreamExt::next(&mut ws_receiver).await {
        match message {
            Message::Text(text) => {
                let envelope: WsEnvelope = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(err) => {
                        log_ws_parse_error(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            &err.to_string(),
                        );
                        let _ = send_ws_error(
                            &ws_tx,
                            None,
                            "INVALID_JSON",
                            format!("invalid payload: {err}"),
                        )
                        .await;
                        continue;
                    }
                };

                let kind = envelope.kind.trim().to_ascii_lowercase();
                if !handshake_done && kind != "connect" && kind != "ping" {
                    handshake_done = true;
                    log_ws_handshake(
                        WS_ENDPOINT,
                        &connection_id,
                        &user.user_id,
                        WS_PROTOCOL_VERSION,
                        WS_PROTOCOL_VERSION,
                        true,
                        None,
                    );
                }

                match kind.as_str() {
                    "connect" => {
                        if handshake_done {
                            let _ = send_ws_error(
                                &ws_tx,
                                envelope.request_id.as_deref(),
                                "ALREADY_CONNECTED",
                                "connection already initialized".to_string(),
                            )
                            .await;
                            continue;
                        }
                        let request_id = resolve_request_id(envelope.request_id.as_deref());
                        let payload = match parse_connect_payload(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                continue;
                            }
                        };
                        match negotiate_ws_protocol(&payload) {
                            Ok(info) => {
                                handshake_done = true;
                                log_ws_handshake(
                                    WS_ENDPOINT,
                                    &connection_id,
                                    &user.user_id,
                                    info.client_min,
                                    info.client_max,
                                    false,
                                    info.client.as_ref(),
                                );
                                let _ =
                                    send_ws_ready(&ws_tx, Some(&request_id), ready_payload.clone())
                                        .await;
                                log_ws_ready(
                                    WS_ENDPOINT,
                                    &connection_id,
                                    &user.user_id,
                                    protocol.version,
                                    protocol.min,
                                    protocol.max,
                                );
                            }
                            Err(err) => {
                                log_ws_handshake_error(
                                    WS_ENDPOINT,
                                    &connection_id,
                                    &user.user_id,
                                    err.code(),
                                    &err.message(),
                                );
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                let _ = out_tx.send(Message::Close(None)).await;
                                close_logged = true;
                                log_ws_close(
                                    WS_ENDPOINT,
                                    &connection_id,
                                    &user.user_id,
                                    None,
                                    Some("handshake_failed"),
                                    Some(started_at.elapsed().as_millis()),
                                );
                                break;
                            }
                        }
                    }
                    "ping" => {
                        let _ = send_ws_pong(&ws_tx).await;
                    }
                    "start" => {
                        let request_id = resolve_request_id(envelope.request_id.as_deref());
                        let payload = match parse_payload::<WsStartPayload>(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                continue;
                            }
                        };
                        let session_id =
                            resolve_session_id(envelope.session_id, payload.session_id);
                        let Some(session_id) = session_id.filter(|value| !value.trim().is_empty())
                        else {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "SESSION_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        };
                        log_ws_message(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            "start",
                            Some(&request_id),
                            Some(&session_id),
                        );
                        // User activity on this session: clears the idle/away
                        // state (docs §5.1) without any per-token work.
                        presence.nodes().touch_web(
                            &connection_id,
                            Utc::now().timestamp_millis() as f64 / 1000.0,
                        );
                        let stream = payload.stream.unwrap_or(true);
                        let mut request = match build_chat_request(
                            &state,
                            &user,
                            &session_id,
                            payload.content,
                            payload.client_message_id,
                            stream,
                            payload.attachments,
                            ChatRequestOverrides {
                                tool_call_mode: payload.tool_call_mode,
                                approval_mode: payload.approval_mode,
                                reasoning_effort: payload.reasoning_effort,
                            },
                        )
                        .await
                        {
                            Ok(request) => request,
                            Err(response) => {
                                let error_code = resolve_ws_error_code(&response);
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    error_code.as_str(),
                                    extract_error_message(response),
                                )
                                .await;
                                continue;
                            }
                        };
                        let (approval_tx, approval_rx) = new_approval_channel();
                        request.approval_tx = Some(approval_tx);

                        let outcome = match state
                            .kernel
                            .thread_runtime
                            .submit_user_request(request)
                            .await
                        {
                            Ok(outcome) => outcome,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    "BAD_REQUEST",
                                    err.to_string(),
                                )
                                .await;
                                continue;
                            }
                        };

                        let (request, lease, approval_rx) = match outcome {
                            ThreadSubmitOutcome::Queued(info) => {
                                let queued_event = StreamEvent {
                                    event: "queued".to_string(),
                                    data: build_queued_event_data(&info),
                                    id: None,
                                    timestamp: Some(Utc::now()),
                                };
                                if send_ws_event(&ws_tx, Some(&request_id), queued_event)
                                    .await
                                    .is_err()
                                {
                                    continue;
                                }
                                // The task was enqueued and will be executed by the
                                // thread runtime's dispatch loop. The queue_enter /
                                // queue_start / queue_finish events (and all stream
                                // events produced during execution) are persisted to
                                // the stream_events table by emit_queue_event and the
                                // orchestrator's stream pump.
                                //
                                // The immediate queued ack updates the current bubble
                                // without closing the request. The queue-scoped resume
                                // loop then forwards persisted queue/start/final events
                                // for this task only.
                                let queue_after_change_seq = info.queue_after_change_seq;
                                let queue_id = info.task_id.clone();
                                let (cancel, task_id) = register_ws_task(
                                    &tasks,
                                    &request_id,
                                    Some(session_id.clone()),
                                    true,
                                )
                                .await;
                                let resume_state = state.clone();
                                let resume_session = session_id.clone();
                                let resume_tx = ws_tx.clone();
                                let resume_request_id = request_id.clone();
                                let resume_task_id = task_id.clone();
                                let resume_tasks = tasks.clone();
                                let resume_request_id_cleanup = request_id.clone();
                                // A queued turn keeps the session busy until its
                                // queue-scoped feeder reaches a terminal event.
                                let queued_busy = presence.nodes().begin_busy(&web_node_id);
                                long_task::spawn("api.chat_ws.queued_auto_resume", async move {
                                    let _queued_busy = queued_busy;
                                    resume_queued_thread_changes_v2(
                                        resume_state,
                                        resume_session,
                                        queue_id,
                                        queue_after_change_seq,
                                        Some(&resume_request_id),
                                        resume_tx,
                                        Some(cancel),
                                    )
                                    .await;
                                    // cleanup: only remove if our task_id still owns
                                    // the entry (avoids clobbering a newer task).
                                    let _ = cleanup_ws_task(
                                        &resume_tasks,
                                        &resume_request_id_cleanup,
                                        &resume_task_id,
                                    )
                                    .await;
                                });
                                continue;
                            }
                            ThreadSubmitOutcome::Run(request, lease) => {
                                (*request, lease, approval_rx)
                            }
                        };

                        let ws_tx_snapshot = ws_tx.clone();
                        let state_snapshot = state.clone();
                        let approval_registry_snapshot = approval_registry.clone();
                        let (cancel, task_id) =
                            register_ws_task(&tasks, &request_id, Some(session_id.clone()), true)
                                .await;
                        let tasks_cleanup = tasks.clone();
                        let request_id_cleanup = request_id.clone();
                        let task_id_cleanup = task_id.clone();
                        let session_id_cleanup = session_id.clone();
                        let approval_forward = long_task::spawn(
                            "api.chat_ws.approval_forward",
                            forward_approval_requests(
                                approval_rx,
                                approval_registry_snapshot.clone(),
                                request_id_cleanup.clone(),
                                session_id_cleanup.clone(),
                            ),
                        );
                        // The turn owns one busy marker; dropping the guard at
                        // the end of the task releases it (docs §5.1 `busy`).
                        let stream_busy = presence.nodes().begin_busy(&web_node_id);
                        long_task::spawn("api.chat_ws.stream_request", async move {
                            let _stream_busy = stream_busy;
                            let _lease = lease;
                            match state_snapshot.kernel.orchestrator.stream(request).await {
                                Ok(stream) => {
                                    tokio::pin!(stream);
                                    let mut feeder_started = false;
                                    // The start request owns its feeder for the
                                    // lifetime of this execution stream.  Once
                                    // the stream has drained, the client creates
                                    // the long-lived watch from its durable
                                    // cursor.  Leaving this feeder alive used to
                                    // make the start feeder and the later watch
                                    // publish the same change rows concurrently.
                                    let feeder_cancel = cancel.child_token();
                                    loop {
                                        tokio::select! {
                                            _ = cancel.cancelled() => {
                                                break;
                                            }
                                            item = tokio_stream::StreamExt::next(&mut stream) => {
                                                let Some(item) = item else {
                                                    break;
                                                };
                                                let event = match item {
                                                    Ok(event) => event,
                                                    Err(_) => continue,
                                                };
                                                if event.event == "thread_turn_started" {
                                                    if !feeder_started {
                                                        feeder_started = true;
                                                        let change_cursor = event.data["resume_from_seq"]
                                                            .as_i64()
                                                            .or_else(|| event.data["change_cursor"].as_i64())
                                                            .unwrap_or(0);
                                                        let ack = StreamEvent {
                                                            event: "stream_started".into(),
                                                            data: event.data.clone(),
                                                            id: None,
                                                            timestamp: Some(Utc::now()),
                                                        };
                                                        let _ = send_ws_event(&ws_tx_snapshot, Some(&request_id_cleanup), ack).await;
                                                        let feeder_state = state_snapshot.clone();
                                                        let feeder_session = session_id_cleanup.clone();
                                                        let feeder_tx = ws_tx_snapshot.clone();
                                                        let feeder_request_id = request_id_cleanup.clone();
                                                        let feeder_cancel = feeder_cancel.clone();
                                                        long_task::spawn("api.chat_ws.change_feeder", async move {
                                                            resume_thread_changes_v2(feeder_state, feeder_session, change_cursor, Some(&feeder_request_id), feeder_tx, Some(feeder_cancel)).await;
                                                        });
                                                    }
                                                    continue;
                                                }
                                                if event.event == "thread_item_tail" {
                                                    let _ = send_ws_tail_event(&ws_tx_snapshot, Some(&request_id_cleanup), event).await;
                                                    continue;
                                                }
                                                // Durable content is delivered only by the feeder.
                                                continue;
                                            }
                                        }
                                    }
                                    // The execution stream only ends after its
                                    // runner and online queue drain, so all
                                    // committed changes are already recoverable
                                    // from the durable cursor.  Stop this
                                    // request-scoped feeder before the frontend
                                    // installs its session watcher.
                                    feeder_cancel.cancel();
                                }
                                Err(err) => {
                                    if !cancel.is_cancelled() {
                                        let _ = send_ws_error_payload(
                                            &ws_tx_snapshot,
                                            Some(&request_id_cleanup),
                                            ws_error_payload_from_anyhow(&err),
                                        )
                                        .await;
                                    }
                                }
                            }
                            approval_forward.abort();
                            clear_pending_approvals(
                                &approval_registry_snapshot,
                                Some(&request_id_cleanup),
                                Some(&session_id_cleanup),
                                ApprovalResponse::Deny,
                            )
                            .await;
                            let _ = cleanup_ws_task(
                                &tasks_cleanup,
                                &request_id_cleanup,
                                &task_id_cleanup,
                            )
                            .await;
                        });
                    }
                    "resume" => {
                        let request_id = resolve_request_id(envelope.request_id.as_deref());
                        let payload = match parse_payload::<WsResumePayload>(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                continue;
                            }
                        };
                        let session_id =
                            resolve_session_id(envelope.session_id, payload.session_id);
                        let Some(session_id) = session_id.filter(|value| !value.trim().is_empty())
                        else {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "SESSION_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        };
                        log_ws_message(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            "resume",
                            Some(&request_id),
                            Some(&session_id),
                        );
                        if payload.after_change_seq.is_none() {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "AFTER_CHANGE_SEQ_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        }
                        if !session_exists(&state, &user.user_id, &session_id) {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "SESSION_NOT_FOUND",
                                i18n::t("error.session_not_found"),
                            )
                            .await;
                            continue;
                        }
                        let ws_tx_snapshot = ws_tx.clone();
                        let state_snapshot = state.clone();
                        let after_change_seq = payload.after_change_seq.unwrap_or(0).max(0);
                        let (cancel, task_id) =
                            register_ws_task(&tasks, &request_id, Some(session_id.clone()), false)
                                .await;
                        let tasks_cleanup = tasks.clone();
                        let request_id_cleanup = request_id.clone();
                        let task_id_cleanup = task_id.clone();
                        long_task::spawn("api.chat_ws.resume_stream", async move {
                            resume_thread_changes_v2(
                                state_snapshot,
                                session_id,
                                after_change_seq,
                                Some(&request_id_cleanup),
                                ws_tx_snapshot,
                                Some(cancel.clone()),
                            )
                            .await;
                            let _ = cleanup_ws_task(
                                &tasks_cleanup,
                                &request_id_cleanup,
                                &task_id_cleanup,
                            )
                            .await;
                        });
                    }
                    "watch" => {
                        let request_id = resolve_request_id(envelope.request_id.as_deref());
                        let payload = match parse_payload::<WsWatchPayload>(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                continue;
                            }
                        };
                        let session_id =
                            resolve_session_id(envelope.session_id, payload.session_id);
                        let Some(session_id) = session_id.filter(|value| !value.trim().is_empty())
                        else {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "SESSION_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        };
                        log_ws_message(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            "watch",
                            Some(&request_id),
                            Some(&session_id),
                        );
                        if payload.after_change_seq.is_none() {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "AFTER_CHANGE_SEQ_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        }
                        if !session_exists(&state, &user.user_id, &session_id) {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "SESSION_NOT_FOUND",
                                i18n::t("error.session_not_found"),
                            )
                            .await;
                            continue;
                        }
                        let after_change_seq = payload.after_change_seq.unwrap_or(0).max(0);
                        let ws_tx_snapshot = ws_tx.clone();
                        let state_snapshot = state.clone();
                        let (cancel, task_id) =
                            register_ws_task(&tasks, &request_id, Some(session_id.clone()), false)
                                .await;
                        let tasks_cleanup = tasks.clone();
                        let request_id_cleanup = request_id.clone();
                        let task_id_cleanup = task_id.clone();
                        long_task::spawn("api.chat_ws.watch_stream", async move {
                            resume_thread_changes_v2(
                                state_snapshot,
                                session_id,
                                after_change_seq,
                                Some(&request_id_cleanup),
                                ws_tx_snapshot,
                                Some(cancel.clone()),
                            )
                            .await;
                            let _ = cleanup_ws_task(
                                &tasks_cleanup,
                                &request_id_cleanup,
                                &task_id_cleanup,
                            )
                            .await;
                        });
                    }
                    "cancel" => {
                        let payload = match envelope.payload {
                            Some(value) => match serde_json::from_value::<WsCancelPayload>(value) {
                                Ok(payload) => payload,
                                Err(err) => {
                                    let _ = send_ws_error(
                                        &ws_tx,
                                        envelope.request_id.as_deref(),
                                        "INVALID_PAYLOAD",
                                        format!("invalid payload: {err}"),
                                    )
                                    .await;
                                    continue;
                                }
                            },
                            None => WsCancelPayload {
                                session_id: None,
                                cancel_source: None,
                            },
                        };
                        let request_id = normalize_request_id(envelope.request_id.as_deref());
                        let session_id =
                            resolve_session_id(envelope.session_id, payload.session_id);
                        let session_id = session_id.filter(|value| !value.trim().is_empty());
                        let cancel_source = payload
                            .cancel_source
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty());
                        if request_id.is_none() && session_id.is_none() {
                            let _ = send_ws_error(
                                &ws_tx,
                                envelope.request_id.as_deref(),
                                "SESSION_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        }
                        log_ws_message(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            "cancel",
                            request_id.as_deref(),
                            session_id.as_deref(),
                        );
                        let mut cancel_session_id = None;
                        {
                            let mut guard = tasks.lock().await;
                            if let Some(request_id) = request_id.as_deref() {
                                if let Some(entry) = guard.remove(request_id) {
                                    entry.cancel.cancel();
                                    if entry.cancel_session {
                                        cancel_session_id = entry.session_id;
                                    }
                                }
                            } else if let Some(session_id) = session_id.as_deref() {
                                let targets = guard
                                    .iter()
                                    .filter_map(|(key, entry)| {
                                        if entry.session_id.as_deref() == Some(session_id) {
                                            Some(key.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect::<Vec<_>>();
                                for key in targets {
                                    if let Some(entry) = guard.remove(&key) {
                                        entry.cancel.cancel();
                                    }
                                }
                                cancel_session_id = Some(session_id.to_string());
                            }
                        }
                        clear_pending_approvals(
                            &approval_registry,
                            request_id.as_deref(),
                            session_id.as_deref(),
                            ApprovalResponse::Deny,
                        )
                        .await;
                        if let Some(session_id) = cancel_session_id {
                            if session_exists(&state, &user.user_id, &session_id) {
                                let _ = state
                                    .kernel
                                    .orchestrator
                                    .goal_handle()
                                    .clear(state.storage.clone(), &user.user_id, &session_id)
                                    .await;
                                let cancel_source = cancel_source.unwrap_or("ws_cancel");
                                let _ = state
                                    .kernel
                                    .thread_runtime
                                    .cancel_session_activity(
                                        &user.user_id,
                                        &session_id,
                                        cancel_source,
                                    )
                                    .await;
                            }
                        }
                    }
                    "goal.get" | "goal.set" | "goal" => {
                        let request_id = resolve_request_id(envelope.request_id.as_deref());
                        let payload = match parse_payload::<WsGoalPayload>(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                continue;
                            }
                        };
                        let session_id =
                            resolve_session_id(envelope.session_id, payload.session_id);
                        let Some(session_id) = session_id.filter(|value| !value.trim().is_empty())
                        else {
                            let _ = send_ws_error(
                                &ws_tx,
                                Some(&request_id),
                                "SESSION_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        };
                        log_ws_message(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            kind.as_str(),
                            Some(&request_id),
                            Some(&session_id),
                        );
                        let _session_record = match state
                            .user_store
                            .get_chat_session(&user.user_id, &session_id)
                        {
                            Ok(Some(record)) => record,
                            Ok(None) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    "SESSION_NOT_FOUND",
                                    i18n::t("error.session_not_found"),
                                )
                                .await;
                                continue;
                            }
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    "BAD_REQUEST",
                                    err.to_string(),
                                )
                                .await;
                                continue;
                            }
                        };
                        let action = payload
                            .action
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .unwrap_or(match kind.as_str() {
                                "goal.set" => "create",
                                _ => "show",
                            });
                        let command = match action {
                            "get" | "show" => GoalCommand::Show,
                            "set" | "create" | "edit" => {
                                let Some(objective) = payload
                                    .objective
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                                else {
                                    let _ = send_ws_error(
                                        &ws_tx,
                                        Some(&request_id),
                                        "OBJECTIVE_REQUIRED",
                                        i18n::t("error.content_required"),
                                    )
                                    .await;
                                    continue;
                                };
                                let objective = objective.to_string();
                                if action == "edit" {
                                    GoalCommand::Edit { objective }
                                } else {
                                    GoalCommand::Create { objective }
                                }
                            }
                            "pause" => GoalCommand::Pause,
                            "resume" => GoalCommand::Resume,
                            "clear" => GoalCommand::Clear,
                            _ => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    "INVALID_GOAL_ACTION",
                                    "invalid goal action".to_string(),
                                )
                                .await;
                                continue;
                            }
                        };
                        let result = crate::api::chat_goal::apply_goal_command(
                            &state,
                            &user,
                            &session_id,
                            command,
                        )
                        .await;
                        match result {
                            Ok(goal_value) => {
                                let _ = crate::api::ws_helpers::send_ws_message(
                                    &ws_tx,
                                    "goal",
                                    Some(&request_id),
                                    Some(json!({
                                        "data": {
                                            "goal": goal_value
                                        }
                                    })),
                                )
                                .await;
                            }
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    Some(&request_id),
                                    "BAD_REQUEST",
                                    err.to_string(),
                                )
                                .await;
                            }
                        }
                    }
                    "approval" => {
                        let request_id = normalize_request_id(envelope.request_id.as_deref());
                        let payload = match parse_payload::<WsApprovalPayload>(envelope.payload) {
                            Ok(payload) => payload,
                            Err(err) => {
                                let _ = send_ws_error(
                                    &ws_tx,
                                    request_id.as_deref(),
                                    err.code(),
                                    err.message(),
                                )
                                .await;
                                continue;
                            }
                        };
                        log_ws_message(
                            WS_ENDPOINT,
                            &connection_id,
                            &user.user_id,
                            "approval",
                            request_id.as_deref(),
                            payload.session_id.as_deref(),
                        );
                        let approval_id = payload.approval_id.trim().to_string();
                        if approval_id.is_empty() {
                            let _ = send_ws_error(
                                &ws_tx,
                                request_id.as_deref(),
                                "APPROVAL_ID_REQUIRED",
                                i18n::t("error.param_required"),
                            )
                            .await;
                            continue;
                        }
                        let Some(decision) = parse_approval_decision(&payload.decision) else {
                            let _ = send_ws_error(
                                &ws_tx,
                                request_id.as_deref(),
                                "INVALID_APPROVAL_DECISION",
                                "invalid approval decision".to_string(),
                            )
                            .await;
                            continue;
                        };
                        let session_scope =
                            resolve_session_id(envelope.session_id, payload.session_id)
                                .map(|value| value.trim().to_string())
                                .filter(|value| !value.is_empty());
                        let mut error_code = "APPROVAL_NOT_FOUND";
                        let mut error_message = "approval request not found".to_string();
                        let entry = match approval_registry.get_snapshot(&approval_id).await {
                            Some(snapshot) if snapshot.source == ApprovalSource::ChatWs => {
                                if let Some(request_id_value) = request_id.as_deref() {
                                    if snapshot.request_id.as_deref() != Some(request_id_value) {
                                        error_code = "APPROVAL_REQUEST_MISMATCH";
                                        error_message = "approval request mismatch".to_string();
                                        None
                                    } else if let Some(session_id_value) = session_scope.as_deref()
                                    {
                                        if snapshot.session_id != session_id_value {
                                            error_code = "APPROVAL_SESSION_MISMATCH";
                                            error_message = "approval session mismatch".to_string();
                                            None
                                        } else {
                                            approval_registry.remove(&approval_id).await
                                        }
                                    } else {
                                        approval_registry.remove(&approval_id).await
                                    }
                                } else if let Some(session_id_value) = session_scope.as_deref() {
                                    if snapshot.session_id != session_id_value {
                                        error_code = "APPROVAL_SESSION_MISMATCH";
                                        error_message = "approval session mismatch".to_string();
                                        None
                                    } else {
                                        approval_registry.remove(&approval_id).await
                                    }
                                } else {
                                    approval_registry.remove(&approval_id).await
                                }
                            }
                            _ => None,
                        };
                        let Some(entry) = entry else {
                            let _ = send_ws_error(
                                &ws_tx,
                                request_id.as_deref(),
                                error_code,
                                error_message,
                            )
                            .await;
                            continue;
                        };
                        let _ = entry.respond_to.send(decision);
                    }
                    _ => {
                        let _ = send_ws_error(
                            &ws_tx,
                            envelope.request_id.as_deref(),
                            "UNSUPPORTED_TYPE",
                            i18n::t("error.param_required"),
                        )
                        .await;
                    }
                }
            }
            Message::Ping(payload) => match out_tx.try_send(Message::Pong(payload)) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_))
                | Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => break,
            },
            Message::Close(frame) => {
                let (code, reason) = frame
                    .map(|value| (Some(value.code), Some(value.reason.to_string())))
                    .unwrap_or((None, None));
                close_logged = true;
                log_ws_close(
                    WS_ENDPOINT,
                    &connection_id,
                    &user.user_id,
                    code,
                    reason.as_deref(),
                    Some(started_at.elapsed().as_millis()),
                );
                break;
            }
            _ => {}
        }
    }

    drop(out_tx);
    state.control.auth_sessions.unregister(&connection_id);
    let _ = writer.await;
    // A transport disconnect is not a user cancellation. Keep approval waits
    // in the session-scoped registry so a reconnected client can decide them;
    // the orchestrator remains the owner of the running turn.
    state.control.presence.disconnect_client(
        &user.user_id,
        &connection_id,
        Utc::now().timestamp_millis() as f64 / 1000.0,
    );
    if !close_logged {
        log_ws_close(
            WS_ENDPOINT,
            &connection_id,
            &user.user_id,
            None,
            Some("eof"),
            Some(started_at.elapsed().as_millis()),
        );
    }
}

async fn forward_approval_requests(
    mut approval_rx: ApprovalRequestRx,
    approval_registry: Arc<PendingApprovalRegistry>,
    request_id: String,
    session_id: String,
) {
    while let Some(request) = approval_rx.recv().await {
        let approval_id = request.id.trim().to_string();
        if approval_id.is_empty() {
            let _ = request.respond_to.send(ApprovalResponse::Deny);
            continue;
        }
        let previous = approval_registry
            .upsert(PendingApprovalEntry {
                approval_id,
                source: ApprovalSource::ChatWs,
                session_id: session_id.clone(),
                request_id: Some(request_id.clone()),
                channel: None,
                account_id: None,
                peer_id: None,
                thread_id: None,
                actor_id: None,
                tool: request.tool,
                summary: request.summary,
                kind: request.kind,
                created_at: Utc::now().timestamp_millis() as f64 / 1000.0,
                respond_to: request.respond_to,
            })
            .await;
        if let Some(previous) = previous {
            let _ = previous.respond_to.send(ApprovalResponse::Deny);
        }
    }
}

fn parse_approval_decision(raw: &str) -> Option<ApprovalResponse> {
    let cleaned = raw.trim().to_ascii_lowercase();
    match cleaned.as_str() {
        "approve_once" | "once" | "approve-once" => Some(ApprovalResponse::ApproveOnce),
        "approve_session" | "session" | "approve-session" => Some(ApprovalResponse::ApproveSession),
        "deny" | "reject" | "cancel" => Some(ApprovalResponse::Deny),
        _ => None,
    }
}

async fn clear_pending_approvals(
    approval_registry: &Arc<PendingApprovalRegistry>,
    request_id: Option<&str>,
    session_id: Option<&str>,
    response: ApprovalResponse,
) {
    let normalized_request_id = request_id.map(str::trim).filter(|value| !value.is_empty());
    let normalized_session_id = session_id.map(str::trim).filter(|value| !value.is_empty());
    let entries = approval_registry
        .remove_matching(|entry| {
            if entry.source != ApprovalSource::ChatWs {
                return false;
            }
            let request_match = normalized_request_id
                .map(|value| entry.request_id.as_deref() == Some(value))
                .unwrap_or(true);
            let session_match = normalized_session_id
                .map(|value| entry.session_id == value)
                .unwrap_or(true);
            request_match && session_match
        })
        .await;
    for entry in entries {
        let _ = entry.respond_to.send(response);
    }
}

fn session_exists(state: &AppState, user_id: &str, session_id: &str) -> bool {
    state
        .user_store
        .get_chat_session(user_id, session_id)
        .ok()
        .flatten()
        .is_some()
}

fn extract_error_message(response: Response) -> String {
    let status = response.status();
    let code = response_error_code(&response)
        .unwrap_or_else(|| map_ws_error_code_by_status(status).to_string());
    match code.as_str() {
        "AUTH_REQUIRED" | "UNAUTHORIZED" => i18n::t("error.auth_required"),
        "SESSION_NOT_FOUND" => i18n::t("error.session_not_found"),
        "USER_QUOTA_INSUFFICIENT" | "USER_QUOTA_EXCEEDED" => {
            i18n::t("error.user_quota_insufficient")
        }
        _ => i18n::t("error.content_required"),
    }
}

fn resolve_ws_error_code(response: &Response) -> String {
    response_error_code(response)
        .unwrap_or_else(|| map_ws_error_code_by_status(response.status()).to_string())
}

fn response_error_code(response: &Response) -> Option<String> {
    response
        .headers()
        .get(crate::api::errors::ERROR_CODE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn map_ws_error_code_by_status(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "AUTH_REQUIRED",
        StatusCode::FORBIDDEN => "PERMISSION_DENIED",
        StatusCode::NOT_FOUND => "SESSION_NOT_FOUND",
        StatusCode::TOO_MANY_REQUESTS => "RATE_LIMITED",
        StatusCode::BAD_REQUEST => "INVALID_REQUEST",
        _ => "BAD_REQUEST",
    }
}

fn normalize_request_id(request_id: Option<&str>) -> Option<String> {
    request_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn resolve_request_id(request_id: Option<&str>) -> String {
    normalize_request_id(request_id).unwrap_or_else(|| format!("req_{}", Uuid::new_v4().simple()))
}

async fn register_ws_task(
    tasks: &Arc<Mutex<HashMap<String, WsStreamEntry>>>,
    request_id: &str,
    session_id: Option<String>,
    cancel_session: bool,
) -> (CancellationToken, String) {
    let cancel = CancellationToken::new();
    let task_id = Uuid::new_v4().simple().to_string();
    let mut guard = tasks.lock().await;
    // `watch` and `resume` are alternate readers of the same durable session
    // log.  They may use different request ids on a multiplexed socket, so
    // request-id replacement alone cannot prevent two feeders from writing
    // the same change range.  Replace a prior recovery subscription for this
    // session, but never cancel a `start`/queued execution task: those own
    // business execution and have `cancel_session == true`.
    if !cancel_session {
        for entry in guard.values() {
            if !entry.cancel_session && entry.session_id == session_id {
                entry.cancel.cancel();
            }
        }
    }
    if let Some(entry) = guard.insert(
        request_id.to_string(),
        WsStreamEntry {
            session_id,
            cancel: cancel.clone(),
            task_id: task_id.clone(),
            cancel_session,
        },
    ) {
        entry.cancel.cancel();
    }
    (cancel, task_id)
}

async fn cleanup_ws_task(
    tasks: &Arc<Mutex<HashMap<String, WsStreamEntry>>>,
    request_id: &str,
    task_id: &str,
) -> bool {
    let mut guard = tasks.lock().await;
    let should_remove = guard
        .get(request_id)
        .map(|entry| entry.task_id == task_id)
        .unwrap_or(false);
    if should_remove {
        guard.remove(request_id);
    }
    should_remove
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_event_data_exposes_wait_position_and_replay_anchor() {
        let info = QueueInfo {
            task_id: "task_test".to_string(),
            thread_id: "thread_test".to_string(),
            session_id: "sess_test".to_string(),
            queue_ahead: 2,
            queue_total: 4,
            active_ahead: 1,
            wait_ahead: 3,
            queue_change_seq: 42,
            queue_after_change_seq: 41,
        };

        let payload = build_queued_event_data(&info);

        assert_eq!(
            payload,
            json!({
                "queued": true,
                "queue_id": "task_test",
                "thread_id": "thread_test",
                "session_id": "sess_test",
                "queue_ahead": 2,
                "queue_total": 4,
                "active_ahead": 1,
                "wait_ahead": 3,
                "queue_change_seq": 42,
                "queue_after_change_seq": 41,
            })
        );
    }

    #[tokio::test]
    async fn recovery_subscription_replaces_another_subscription_for_the_session() {
        let tasks = Arc::new(Mutex::new(HashMap::new()));
        let (first, _) =
            register_ws_task(&tasks, "watch-first", Some("session-a".to_string()), false).await;
        let (_second, _) = register_ws_task(
            &tasks,
            "resume-second",
            Some("session-a".to_string()),
            false,
        )
        .await;
        assert!(first.is_cancelled());
    }

    #[tokio::test]
    async fn recovery_subscription_does_not_cancel_active_start_execution() {
        let tasks = Arc::new(Mutex::new(HashMap::new()));
        let (start, _) =
            register_ws_task(&tasks, "start-request", Some("session-a".to_string()), true).await;
        let (_watch, _) = register_ws_task(
            &tasks,
            "watch-request",
            Some("session-a".to_string()),
            false,
        )
        .await;
        assert!(!start.is_cancelled());
    }
}
