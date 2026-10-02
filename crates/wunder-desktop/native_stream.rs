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

#[path = "native_follow.rs"]
mod follow;

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
    let lease = lease;
    let root_id = request
        .config_overrides
        .as_ref()
        .and_then(|v| v.get("__thread_log_turn_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let stream = state.kernel.orchestrator.stream(request).await?;
    tokio::pin!(stream);
    let mut stopped = false;
    let mut goal_ready = false;
    let mut display_args: std::collections::VecDeque<(String, Value)> =
        std::collections::VecDeque::new();
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
                    let mut value = json!({"event":event.event,"data":event.data,"id":event.id});
                    if matches!(event.event.as_str(), "tool_call" | "tool_start" | "tool_result" | "tool_output") {
                        let payload = value["data"].get("data").filter(|_| value["data"].get("tool").is_none()).unwrap_or(&value["data"]);
                        let tool = payload.get("tool").or_else(|| payload.get("tool_name"))
                            .and_then(Value::as_str).unwrap_or("工具");
                        let tool = tool.to_string();
                        let pending = matches!(event.event.as_str(), "tool_call" | "tool_start");
                        let id = payload.get("tool_call_id").and_then(Value::as_str).unwrap_or_default().to_string();
                        let mut display_payload = payload.clone();
                        if pending && !id.is_empty() {
                            // Retain only short file/command arguments needed to render
                            // a terminal result, never an unbounded full call payload.
                            let args = payload.get("args").or_else(|| payload.get("arguments"));
                            let mut compact = serde_json::Map::new();
                            for key in ["path", "file_path", "command", "cmd", "content", "text"] {
                                if let Some(text) = args.and_then(|v| v.get(key)).and_then(Value::as_str) {
                                    compact.insert(key.into(), json!(wunder_server::tool_result_display::preview(text)));
                                }
                            }
                            if display_args.len() >= 24 { display_args.pop_front(); }
                            display_args.push_back((id.clone(), Value::Object(compact)));
                        } else if let Some(index) = display_args.iter().position(|row| row.0 == id) {
                            if let Some((_, args)) = display_args.remove(index) {
                                display_payload["args"] = args;
                            }
                        }
                        value["display_result"] = json!(wunder_server::tool_result_display::tool_result_display(
                            &tool, &display_payload, pending
                        ));
                    }
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
    drop(lease);
    if stopped {
        let _ = output
            .send(NativeChatEvent::Event(
                json!({"event":"turn_terminal","data":{"status":"cancelled"}}),
            ))
            .await;
    } else if goal_ready {
        // Capture the boundary before scheduling: no continuation write can be lost.
        let storage = state.storage.clone();
        let target = session.clone();
        let cursor = blocking::run_db("native.goal.baseline", move || {
            storage.latest_thread_change_seq_by_session(&target)
        })
        .await?;
        state
            .kernel
            .thread_runtime
            .spawn_goal_continuation_after_cooldown(user_id.clone(), session.clone());
        follow::watch(
            &state, &user_id, &session, &root_id, cursor, output, cancel, true,
        )
        .await?;
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
    Ok(())
}

async fn replay_queue(
    state: &Arc<AppState>,
    user: &str,
    info: &wunder_server::runtime::thread::QueueInfo,
    output: &mpsc::Sender<NativeChatEvent>,
    cancel: &CancellationToken,
) -> Result<()> {
    let store = state.user_store.clone();
    let task_id = info.task_id.clone();
    let task = blocking::run_db("native.queue.identity", move || {
        store.get_agent_task(&task_id)
    })
    .await?
    .ok_or_else(|| anyhow!("queued task disappeared"))?;
    let root = task
        .request_payload
        .pointer("/config_overrides/__thread_log_turn_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("queued turn identity missing"))?;
    let cursor = task
        .request_payload
        .pointer("/config_overrides/__thread_log_resume_from_seq")
        .and_then(Value::as_i64)
        .unwrap_or(info.queue_after_change_seq);
    follow::watch(
        state,
        user,
        &info.session_id,
        root,
        cursor,
        output,
        cancel,
        false,
    )
    .await
}
