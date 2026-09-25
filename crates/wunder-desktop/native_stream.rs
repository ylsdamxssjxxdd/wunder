//! Bounded asynchronous delivery; dropping a subscriber never cancels a settled turn.
use super::NativeChatInput;
use anyhow::{anyhow, Result};
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::{runtime::Runtime, sync::mpsc};
use tokio_util::sync::CancellationToken;
use wunder_server::{
    api::chat::build_native_chat_request, blocking, state::AppState, ThreadSubmitOutcome,
};

const CAPACITY: usize = 128;

#[derive(Debug)]
pub enum NativeChatEvent {
    Event(Value),
    Queued,
    Finished,
    Failed(String),
}

pub struct NativeStream {
    cancel: CancellationToken,
    receiver: mpsc::Receiver<NativeChatEvent>,
}

impl NativeStream {
    /// Cancellation is explicit. A completed stream may be dropped safely.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Nonblocking UI poll. Empty and disconnected have distinct meanings.
    pub fn try_recv(&mut self) -> std::result::Result<Option<NativeChatEvent>, &'static str> {
        match self.receiver.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err("原生事件通道已关闭"),
        }
    }

    pub fn pending_events(&self) -> usize {
        self.receiver.len()
    }
}

pub(super) fn start(
    runtime: &Runtime,
    state: Arc<AppState>,
    user: String,
    input: NativeChatInput,
) -> NativeStream {
    let (output, receiver) = mpsc::channel(CAPACITY);
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    runtime.spawn(async move {
        let result = run(state, user, input, &output, &token).await;
        if let Err(error) = result {
            tracing::warn!(%error, "native chat failed");
            let _ = output
                .send(NativeChatEvent::Failed(
                    "聊天执行失败，请检查模型配置或运行时日志".into(),
                ))
                .await;
        } else {
            let _ = output.send(NativeChatEvent::Finished).await;
        }
    });
    NativeStream { cancel, receiver }
}

async fn run(
    state: Arc<AppState>,
    user_id: String,
    input: NativeChatInput,
    output: &mpsc::Sender<NativeChatEvent>,
    cancel: &CancellationToken,
) -> Result<()> {
    let store = state.user_store.clone();
    let owner = user_id.clone();
    let session = input.session_id.trim().to_string();
    let owned_session = session.clone();
    let user = blocking::run_db("native.chat.user", move || {
        store
            .get_chat_session(&owner, &owned_session)?
            .ok_or_else(|| anyhow!("chat session not found"))?;
        store
            .get_user_by_id(&owner)?
            .ok_or_else(|| anyhow!("desktop user unavailable"))
    })
    .await?;
    let request = build_native_chat_request(
        &state,
        &user,
        &session,
        input.content,
        input.client_message_id,
        input
            .attachments
            .into_iter()
            .map(|attachment| wunder_server::schemas::AttachmentPayload {
                name: Some(attachment.name),
                content: Some(attachment.content),
                content_type: Some(attachment.content_type),
                public_path: None,
            })
            .collect(),
    )
    .await?;
    if cancel.is_cancelled() {
        let _ = output
            .send(NativeChatEvent::Event(
                json!({"event":"turn_terminal","data":{"status":"cancelled"}}),
            ))
            .await;
        return Ok(());
    }
    let outcome = state
        .kernel
        .thread_runtime
        .submit_user_request(request)
        .await?;
    let (request, lease) = match outcome {
        ThreadSubmitOutcome::Run(request, lease) => (*request, lease),
        ThreadSubmitOutcome::Queued(info) => {
            let _ = output.send(NativeChatEvent::Queued).await;
            return replay_queue(&state, &user_id, &info, output, cancel).await;
        }
    };
    // Retain the lease until the stream pump completes, including durable writes.
    let _lease = lease;
    let stream = state.kernel.orchestrator.stream(request).await?;
    tokio::pin!(stream);
    let mut stopped = false;
    let mut goal_ready = false;
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled(), if !stopped => {
                cancel_chat(&state, &user_id, &session).await?;
                stopped = true;
            }
            item = stream.next() => {
                let Some(Ok(event)) = item else { break };
                goal_ready |= event.event == "goal_continuation_ready";
                // Awaiting capacity yields the Tokio worker. Closing the receiver
                // only disables delivery; the stream is still drained to settlement.
                if !output.is_closed() {
                    let value = json!({"event":event.event,"data":event.data,"id":event.id});
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled(), if !stopped => {
                            cancel_chat(&state, &user_id, &session).await?;
                            stopped = true;
                            let _ = output.send(NativeChatEvent::Event(value)).await;
                        }
                        permit = output.reserve() => {
                            if let Ok(permit) = permit {
                                permit.send(NativeChatEvent::Event(value));
                            }
                        }
                    }
                }
            }
        }
    }
    if stopped {
        let _ = output
            .send(NativeChatEvent::Event(
                json!({"event":"turn_terminal","data":{"status":"cancelled"}}),
            ))
            .await;
    } else if goal_ready {
        state
            .kernel
            .thread_runtime
            .spawn_goal_continuation_after_cooldown(user_id, session);
    }
    Ok(())
}

pub(super) async fn cancel_chat(state: &Arc<AppState>, user: &str, session: &str) -> Result<()> {
    let store = state.user_store.clone();
    let owner = user.to_string();
    let id = session.to_string();
    blocking::run_db("native.chat.cancel_owner", move || {
        store
            .get_chat_session(&owner, &id)?
            .ok_or_else(|| anyhow!("chat session not found"))
    })
    .await?;
    wunder_server::goal::clear_goal(state.storage.clone(), user, session).await?;
    state
        .kernel
        .thread_runtime
        .cancel_session_activity(user, session, "native_ui")
        .await?;
    wunder_server::persist_user_cancelled_turn_marker(
        state.workspace.clone(),
        state.user_store.clone(),
        user,
        session,
        "native_ui",
    )
    .await?;
    Ok(())
}

async fn replay_queue(
    state: &Arc<AppState>,
    user: &str,
    info: &wunder_server::runtime::thread::QueueInfo,
    output: &mpsc::Sender<NativeChatEvent>,
    cancel: &CancellationToken,
) -> Result<()> {
    let mut cursor = info.queue_after_event_id;
    let mut started = false;
    loop {
        if cancel.is_cancelled() {
            cancel_chat(state, user, &info.session_id).await?;
            let _ = output
                .send(NativeChatEvent::Event(
                    json!({"event":"turn_terminal","data":{"status":"cancelled"}}),
                ))
                .await;
            return Ok(());
        }
        if output.is_closed() {
            return Ok(());
        }
        let storage = state.storage.clone();
        let session = info.session_id.clone();
        let events = blocking::run_db("native.chat.queue_replay", move || {
            storage.load_stream_events(&session, cursor, CAPACITY as i64)
        })
        .await?;
        let progressed = !events.is_empty();
        for record in events {
            let id = record["event_id"].as_i64().unwrap_or_default();
            if id <= cursor {
                continue;
            }
            cursor = id;
            let kind = record["event"].as_str().unwrap_or_default();
            let data = &record["data"];
            let queue_event = kind.starts_with("queue_");
            let matches_queue = data["queue_id"].as_str() == Some(info.task_id.as_str());
            if queue_event && !matches_queue {
                continue;
            }
            if kind == "queue_start" {
                started = true;
            }
            if !queue_event && !started {
                continue;
            }
            let terminal = matches!(kind, "queue_finish" | "queue_fail" | "queue_cancel");
            // A queued subscriber settles on its own queue terminal, not a
            // neighbouring turn_terminal belonging to another queued request.
            if kind != "turn_terminal" {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    _ = output.send(NativeChatEvent::Event(json!({"event":kind,"data":data,"id":id}))) => {}
                }
            }
            if terminal {
                return Ok(());
            }
        }
        if !progressed {
            tokio::select! {
                _ = cancel.cancelled() => {},
                _ = output.closed() => return Ok(()),
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {},
            }
        }
    }
}
