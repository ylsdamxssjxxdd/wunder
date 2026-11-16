//! Bounded asynchronous delivery; dropping a subscriber never cancels a settled turn.
use super::{NativeChatInput, ThreadLeaseHeartbeat};
use anyhow::{Result, anyhow};
use futures::{FutureExt, StreamExt};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{runtime::Runtime, sync::mpsc};
use tokio_util::sync::CancellationToken;
use wunder_server::{
    ThreadSubmitOutcome, api::chat::build_native_chat_request, blocking, state::AppState,
};

#[path = "native_follow.rs"]
mod follow;

const CAPACITY: usize = 128;

#[derive(Debug, Clone)]
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

    /// Await the next stream event. The native shell worker selects this with
    /// its command queue, so an active stream wakes the worker instead of
    /// being polled on a timer.
    pub async fn recv(&mut self) -> Option<NativeChatEvent> {
        self.receiver.recv().await
    }

    /// Block a non-UI worker until the next event or stream close. This is
    /// used only while settling an explicitly cancelled submission: draining
    /// keeps the bounded producer moving until it has observed cancellation
    /// and released the session-wide execution fence.
    pub fn blocking_recv(&mut self) -> Option<NativeChatEvent> {
        self.receiver.blocking_recv()
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
    lease_proof: Value,
    heartbeat: ThreadLeaseHeartbeat,
) -> NativeStream {
    let (output, receiver) = mpsc::channel(CAPACITY);
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    runtime.spawn(async move {
        // Keep the desktop open-thread lease alive through queued hand-off,
        // generation and durable settlement. Dropping this task releases the
        // guard and returns the lease worker to a blocking idle wait.
        let _heartbeat = heartbeat;
        // A panic deep inside the HTTP stack used to unwind this task: the
        // sender was dropped, the subscriber saw a closed channel, and the
        // turn ended with neither a reply nor an error. Convert it into a
        // visible failure instead of silence.
        let result =
            std::panic::AssertUnwindSafe(run(state, user, input, lease_proof, &output, &token))
                .catch_unwind()
                .await;
        match result {
            Ok(Ok(())) => {
                let _ = output.send(NativeChatEvent::Finished).await;
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "native chat failed");
                let _ = output
                    .send(NativeChatEvent::Failed(
                        "聊天执行失败，请检查模型配置或运行时日志".into(),
                    ))
                    .await;
            }
            Err(payload) => {
                let detail = panic_message(payload.as_ref());
                tracing::error!(%detail, "native chat task panicked");
                let _ = output
                    .send(NativeChatEvent::Failed(format!(
                        "聊天执行失败：内部错误（{detail}）"
                    )))
                    .await;
            }
        }
    });
    NativeStream { cancel, receiver }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "未知错误".into()
    }
}

async fn run(
    state: Arc<AppState>,
    user_id: String,
    input: NativeChatInput,
    lease_proof: Value,
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
    let mut request = build_native_chat_request(
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
        input.reasoning_effort,
        input.inquiry_turn_id,
    )
    .await?;
    request.config_overrides.get_or_insert_with(|| json!({}))["__local_open_lease"] = lease_proof;
    // 本地形态的输入框消息是「现在就说」：线程停在问询面板上时，运行时不得
    // 把它排进队列（面板正是这句话要回答的东西）。
    request.config_overrides.get_or_insert_with(|| json!({}))["__interactive_chat"] = json!(true);
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
    let user_round = request
        .config_overrides
        .as_ref()
        .and_then(|v| v["__thread_log_user_round"].as_i64())
        .unwrap_or(0);
    let _ = output.send(NativeChatEvent::Event(json!({"event":"native_turn_identity", "data":{"root_turn_id":root_id,"user_round":user_round}}))).await;
    let stream = state.kernel.orchestrator.stream(request).await?;
    tokio::pin!(stream);
    let mut stopped = false;
    let mut display_args: std::collections::VecDeque<(String, Value)> =
        std::collections::VecDeque::new();
    let mut ledger = TailLedger::default();
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled(), if !stopped => {
                cancel_chat(&state, &user_id, &session).await?;
                stopped = true;
            }
            item = stream.next() => {
                let Some(Ok(event)) = item else { break };
                // The desktop projects a live turn from the semantic frames
                // alone. `thread_item_tail`, `thread_process_item` and
                // `thread_change` are transport duplicates for other clients;
                // forwarding them made this window decode and then ignore the
                // majority of its frames, spending the per-frame UI budget on
                // nothing. Filtering here keeps the bounded channel free for
                // content the pump actually reduces.
                if event.event == "thread_item_tail" {
                    // Second copy of a chunk the delta lane normally delivered
                    // (see `TailLedger`): dropped while it stays inside what
                    // that lane covered, forwarded once when it does not.
                    if ledger.tail_is_redundant(&event.data) {
                        continue;
                    }
                } else if is_transport_duplicate(&event.event) {
                    continue;
                } else {
                    ledger.observe_delta(&event.event, &event.data);
                }
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
                            for key in [
                                "path", "file_path", "command", "cmd", "content", "text", "action",
                                "title", "query", "memory_id",
                            ] {
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
    } else if goal_still_active(&state, &user_id, &session).await {
        // The driver schedules the next goal round internally; keep watching
        // so the UI keeps receiving its output until the goal settles.
        let storage = state.storage.clone();
        let target = session.clone();
        let cursor = blocking::run_db("native.goal.baseline", move || {
            storage.latest_thread_change_seq_by_session(&target)
        })
        .await?;
        follow::watch(
            &state, &user_id, &session, &root_id, cursor, output, cancel, true,
        )
        .await?;
    }
    Ok(())
}

/// A goal keeps the desktop chat stream alive only while it is actively
/// driving rounds; paused/blocked/complete goals end the session here.
async fn goal_still_active(state: &Arc<AppState>, user_id: &str, session: &str) -> bool {
    let storage = state.storage.clone();
    let owner = user_id.to_string();
    let target = session.to_string();
    blocking::run_db("native.goal.check", move || {
        Ok(storage
            .get_session_goal(&owner, &target)?
            .is_some_and(|goal| goal.phase == "active"))
    })
    .await
    .unwrap_or(false)
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
    let goal_service = state.kernel.orchestrator.goal_handle();
    let _ = goal_service
        .clear(state.storage.clone(), user, session)
        .await;
    goal_service.driver().notify_session_cancelled(session);
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

/// Watermarks of the two lanes that carry streamed text.
///
/// The runtime publishes each chunk twice: once as a semantic delta (which the
/// reducer consumes) and once as an offset-addressed `thread_item_tail`. The
/// tail is a second copy, which is why this window used to drop it outright —
/// but "second copy" and "useless" are not the same thing. A chunk the delta
/// lane never delivered leaves a hole nothing can name; the tail can name it
/// (`offset` is UTF-16 units, measured by the sender), so the tail is kept as
/// the repair lane while the common case still costs zero frames:
///
/// * a tail the delta lane already covered is dropped, exactly as before;
/// * a tail that reaches past it is forwarded once, and the reducer applies it
///   by offset, which is idempotent by construction.
///
/// Counting the delta lane here (rather than in the reducer) keeps the frame
/// budget untouched: the ledger only ever asks a JSON object for two lengths.
#[derive(Default)]
struct TailLedger {
    /// UTF-16 units already delivered, per `(item_id, field)`.
    covered: std::collections::HashMap<(String, String), usize>,
}

impl TailLedger {
    /// Count the text one forwarded delta added.
    ///
    /// The runtime collapses every semantic delta into `thread_item_delta` and
    /// keeps the type in `source_event`; the item identity is
    /// `turn_id}:text-model_round}`, the same key its tails use.
    fn observe_delta(&mut self, event: &str, data: &Value) {
        if event != "thread_item_delta" {
            return;
        }
        let payload = data.get("data").unwrap_or(data);
        if payload["source_event"].as_str() != Some("llm_output_delta") {
            return;
        }
        let (Some(turn), Some(round)) = (
            payload["turn_id"].as_str().filter(|id| !id.is_empty()),
            payload["model_round"].as_i64(),
        ) else {
            return;
        };
        let item = format!("{turn}:text-{round}");
        // Only reasoning needs a repair lane: the answer has a complete copy
        // waiting at the round's end (`llm_output` / `final` content), a
        // thought does not.
        for (source, field) in [("reasoning_delta", "reasoning")] {
            let Some(text) = payload[source].as_str().filter(|text| !text.is_empty()) else {
                continue;
            };
            let entry = self
                .covered
                .entry((item.clone(), field.to_string()))
                .or_insert(0);
            *entry = entry.saturating_add(text.encode_utf16().count());
        }
    }

    /// Whether a tail adds nothing the delta lane did not already deliver.
    fn tail_is_redundant(&mut self, data: &Value) -> bool {
        let payload = data.get("data").unwrap_or(data);
        let (Some(item), Some(field), Some(text)) = (
            payload["item_id"].as_str().filter(|id| !id.is_empty()),
            payload["field"].as_str().filter(|field| !field.is_empty()),
            payload["text"].as_str().filter(|text| !text.is_empty()),
        ) else {
            // A tail this window cannot address is not worth a frame; the
            // round's finished text still closes anything it would have said.
            return true;
        };
        let offset = payload["offset"].as_u64().unwrap_or(0) as usize;
        if field != "reasoning" {
            // Nothing downstream would apply it: the answer's own end-of-round
            // text is authoritative, so its tail stays a duplicate.
            return true;
        }
        let end = offset.saturating_add(text.encode_utf16().count());
        let entry = self
            .covered
            .entry((item.to_string(), field.to_string()))
            .or_insert(0);
        if *entry >= end {
            return true;
        }
        *entry = end;
        false
    }
}

///
/// `thread_item_tail` is the one exception the caller handles itself: a tail the
/// delta lane did not cover is a repair, not a duplicate, so it goes through
/// [`TailLedger::tail_is_redundant`] first.
/// Frames the desktop never reduces.
///
/// The runtime publishes each live change three ways: `thread_item_tail` and
/// `thread_process_item` (durable projections for other clients) and
/// `thread_change` (a cursor notification). The desktop reducer reads the
/// semantic frame (`thread_item_delta` and the non-delta events), so handing it
/// the duplicates spent most of every UI frame decoding JSON it discarded — the
/// direct cause of a backed-up live stream. Dropping them here is also where the
/// bounded channel keeps its capacity for real content.
///
/// The tail is the one exception, and the loop below decides it by watermark
/// rather than by name: it is what the repair lane travels on.
fn is_transport_duplicate(event: &str) -> bool {
    matches!(
        event,
        "thread_item_tail" | "thread_process_item" | "thread_change"
    )
}

#[cfg(test)]
mod tests {
    use super::{TailLedger, is_transport_duplicate};
    use serde_json::json;

    /// tail 是第二条通道：delta 已覆盖的那一段照旧丢掉（不占帧），跨过水位的
    /// 那一段才放行一次，好让 reducer 按 offset 幂等补上。
    #[test]
    fn a_tail_only_costs_a_frame_when_it_goes_past_the_delta_lane() {
        let mut ledger = TailLedger::default();
        ledger.observe_delta(
            "thread_item_delta",
            &json!({"data": {
                "source_event": "llm_output_delta",
                "turn_id": "t1",
                "model_round": 1,
                "reasoning_delta": "想了"
            }}),
        );
        let same = json!({"data": {
            "item_id": "t1:text-1", "field": "reasoning", "offset": 0, "text": "想了"
        }});
        assert!(
            ledger.tail_is_redundant(&same),
            "delta 已覆盖的 tail 不该占一帧"
        );
        let ahead = json!({"data": {
            "item_id": "t1:text-1", "field": "reasoning", "offset": 2, "text": "再动手"
        }});
        assert!(!ledger.tail_is_redundant(&ahead), "跨过水位的 tail 要放行");
        assert!(
            ledger.tail_is_redundant(&ahead),
            "放行过的同一段再到达就该丢掉"
        );
    }

    /// 正文与思考两条通道的水位互不干扰；没有 turn_id 的增量不参与记账。
    #[test]
    fn content_and_reasoning_watermarks_are_independent() {
        let mut ledger = TailLedger::default();
        ledger.observe_delta(
            "thread_item_delta",
            &json!({"data": {
                "source_event": "llm_output_delta", "turn_id": "t1", "model_round": 2, "delta": "答案"
            }}),
        );
        assert!(ledger.tail_is_redundant(&json!({"data": {
            "item_id": "t1:text-2", "field": "content", "offset": 0, "text": "答案"
        }})));
        assert!(!ledger.tail_is_redundant(&json!({"data": {
            "item_id": "t1:text-2", "field": "reasoning", "offset": 0, "text": "思考"
        }})));
        ledger.observe_delta(
            "thread_item_delta",
            &json!({"data": {"source_event": "llm_output_delta", "reasoning_delta": "x"}}),
        );
        assert!(!ledger.tail_is_redundant(&json!({"data": {
            "item_id": "t1:text-9", "field": "reasoning", "offset": 0, "text": "x"
        }})));
    }

    #[test]
    fn transport_duplicates_never_reach_the_ui_channel() {
        for duplicate in ["thread_item_tail", "thread_process_item", "thread_change"] {
            assert!(
                is_transport_duplicate(duplicate),
                "{duplicate} must be dropped"
            );
        }
    }

    #[test]
    fn semantic_and_delta_frames_are_kept() {
        for kept in [
            "thread_item_delta",
            "llm_output",
            "final",
            "tool_call",
            "tool_result",
            "reasoning_delta",
            "native_stats",
            "context_usage",
            "question_panel",
            "turn_terminal",
        ] {
            assert!(
                !is_transport_duplicate(kept),
                "{kept} must reach the reducer"
            );
        }
    }
}
