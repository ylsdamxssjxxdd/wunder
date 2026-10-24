use super::thread_runtime::{thread_closed_payload, thread_not_loaded_payload};
use super::*;
use crate::core::long_task;

pub(super) enum StreamSignal {
    Event(StreamEvent),
    Done,
}

fn should_backpressure_online_stream_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "thread_item_tail"
            | "llm_output_delta"
            | "thread_item_delta"
            | "command_session_delta"
            | "command_session_start"
            | "command_session_status"
            | "command_session_exit"
            | "command_session_summary"
            // The question panel is the turn's only hand-off to the user: the
            // turn stops waiting for it, so dropping it under a full queue
            // leaves the client with an answer nobody can submit.
            | "question_panel"
    )
}

#[derive(Clone)]
pub(super) struct EventEmitter {
    session_id: String,
    user_id: String,
    queue: Option<mpsc::Sender<StreamSignal>>,
    storage: Option<Arc<dyn StorageBackend>>,
    monitor: Arc<MonitorState>,
    is_admin: bool,
    closed: Arc<AtomicBool>,
    next_event_id: Arc<AtomicI64>,
    client_message_id: Option<String>,
    turn_context: Arc<ParkingMutex<Value>>,
    /// Serializes the whole emit path (durable commits + queue enqueues) so
    /// change-cursor order == queue order == wire order for every emitter.
    emit_lock: Arc<tokio::sync::Mutex<()>>,
    committer: Option<Arc<super::thread_log_committer::ThreadLogCommitter>>,
    usage: Arc<ParkingMutex<TokenUsage>>,
    model_requests: Arc<ParkingMutex<i64>>,
    account_credits_consumed: Arc<ParkingMutex<i64>>,
    /// UTF-16 cursors for ephemeral assistant tails in the active request.
    /// Completed items remain the durable source of truth.
    tail_offsets: Arc<ParkingMutex<HashMap<(String, String), usize>>>,
}

impl EventEmitter {
    pub(super) fn bind_turn(&self, turn_id: &str, user_round: i64) {
        *self.turn_context.lock() = json!({"turn_id":turn_id,"user_round":user_round});
    }

    pub(super) fn with_committer(
        mut self,
        committer: Arc<super::thread_log_committer::ThreadLogCommitter>,
    ) -> Self {
        self.committer = Some(committer);
        self
    }

    pub(super) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(super) fn new(
        session_id: String,
        user_id: String,
        queue: Option<mpsc::Sender<StreamSignal>>,
        storage: Option<Arc<dyn StorageBackend>>,
        monitor: Arc<MonitorState>,
        is_admin: bool,
        client_message_id: Option<String>,
    ) -> Self {
        Self {
            session_id,
            user_id,
            queue,
            storage,
            monitor,
            is_admin,
            closed: Arc::new(AtomicBool::new(false)),
            // IDs only order frames on this live connection. They are never
            // persisted or used to resume a chat stream.
            next_event_id: Arc::new(AtomicI64::new(1)),
            client_message_id,
            turn_context: Arc::new(ParkingMutex::new(json!({}))),
            emit_lock: Arc::new(tokio::sync::Mutex::new(())),
            committer: None,
            usage: Arc::new(ParkingMutex::new(TokenUsage {
                reasoning: Some(0),
                ..Default::default()
            })),
            model_requests: Arc::new(ParkingMutex::new(0)),
            account_credits_consumed: Arc::new(ParkingMutex::new(0)),
            tail_offsets: Arc::new(ParkingMutex::new(HashMap::new())),
        }
    }

    fn with_client_message_id(&self, data: Value) -> Value {
        inject_client_message_id(data, self.client_message_id.as_deref())
    }

    pub(super) fn record_usage(&self, usage: &TokenUsage) -> TokenUsage {
        let mut total = self.usage.lock();
        super::usage_accounting::accumulate_usage(&mut total, usage);
        total.clone()
    }

    pub(super) fn accumulated_usage(&self) -> TokenUsage {
        self.usage.lock().clone()
    }

    pub(super) fn record_model_request(&self, count: i64) -> i64 {
        let mut total = self.model_requests.lock();
        *total = total.saturating_add(count.max(0));
        *total
    }

    pub(super) fn record_account_credit_consumption(&self, count: i64) -> i64 {
        let mut total = self.account_credits_consumed.lock();
        *total = total.saturating_add(count.max(0));
        *total
    }

    pub(super) fn accumulated_model_requests(&self) -> i64 {
        *self.model_requests.lock()
    }

    pub(super) fn accumulated_account_credit_consumption(&self) -> i64 {
        *self.account_credits_consumed.lock()
    }

    /// Return the billable request count used by persisted assistant stats.
    ///
    /// Account debit is recorded after quota admission. A few completion and
    /// recovery paths can persist stats before the debit counter is observed;
    /// for normal users the dispatched model request count is the safe lower
    /// bound because every admitted request consumes one credit. Administrators
    /// are explicitly exempt from account billing.
    pub(super) fn accumulated_billable_account_credits(&self, is_admin: bool) -> i64 {
        if is_admin {
            0
        } else {
            self.accumulated_account_credit_consumption()
                .max(self.accumulated_model_requests())
        }
    }

    fn close(&self) {
        self.closed.store(true, AtomicOrdering::SeqCst);
    }

    pub(super) async fn finish(&self) {
        let Some(queue) = &self.queue else {
            return;
        };
        if self.closed.load(AtomicOrdering::SeqCst) {
            return;
        }
        let _ = queue.try_send(StreamSignal::Done);
    }

    pub(super) async fn emit(&self, event_type: &str, data: Value) -> StreamEvent {
        // Hold across the durable commits and the queue enqueues below so a
        // concurrent emitter (tool forwarder) can never interleave a change
        // cursor ahead of its own event in the wire queue.
        let _emit_guard = self.emit_lock.lock().await;
        let timestamp = Utc::now();
        let mut data = self.with_client_message_id(data);
        if let Some(map) = data.as_object_mut() {
            if let Some(context) = self.turn_context.lock().as_object() {
                for (key, value) in context {
                    map.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
        // The immutable assistant item is committed from the `llm_output`
        // event before the executor appends its model-context snapshot.  The
        // latter may carry the final turn aggregate, but it intentionally
        // cannot replace a completed item.  Put the bounded presentation
        // metrics on the output event itself so a reopened chat bubble keeps
        // context, quota, timing and speed information.
        self.enrich_llm_output_message_stats(event_type, &mut data);
        // Incremental text and progress are live-only. The completed event
        // below contains the semantic body used to restore a reopened thread.
        if let Some(item) =
            crate::services::thread_log::event_item(&self.session_id, event_type, &data)
        {
            if let Some(committer) = self.committer.clone() {
                let owner = self.user_id.clone();
                match committer.commit_item(&owner, &item).await {
                    Ok(Some(receipt)) => {
                        let change_event = StreamEvent {
                            event: "thread_change".into(),
                            data: enrich_event_payload(receipt, Some(&self.session_id), timestamp),
                            // Change cursor and transport event IDs are distinct.
                            // Reusing the following event's ID would deduplicate it.
                            id: None,
                            timestamp: Some(timestamp),
                        };
                        self.enqueue_event(&change_event, true).await;
                    }
                    Ok(None) => {}
                    Err(error) => warn!("persist thread item failed: {error}"),
                }
            }
        }
        if !event_type.ends_with("_delta") {
            self.monitor
                .record_event(&self.session_id, event_type, &data);
        }
        if event_type.ends_with("_delta") {
            if let Some(map) = data.as_object_mut() {
                // The shared envelope must retain the semantic stream type.
                // Otherwise command/tool output is rendered as assistant text.
                map.insert("source_event".into(), json!(event_type));
            }
        }
        // The generic delta envelope is retained for CLI and legacy consumers.
        // Chat additionally receives a stable item identity and an append
        // cursor, so it can render reasoning and answer text before the final
        // immutable assistant item is committed.
        for tail_data in self.llm_output_tails(event_type, &data) {
            let tail_event = StreamEvent {
                event: "thread_item_tail".into(),
                data: enrich_event_payload(tail_data, Some(&self.session_id), timestamp),
                id: Some(
                    self.next_event_id
                        .fetch_add(1, AtomicOrdering::SeqCst)
                        .to_string(),
                ),
                timestamp: Some(timestamp),
            };
            self.enqueue_event(&tail_event, false).await;
        }
        let event_id = self.next_event_id.fetch_add(1, AtomicOrdering::SeqCst);
        let payload = enrich_event_payload(data, Some(&self.session_id), timestamp);
        let online_event_type = if event_type.ends_with("_delta") {
            "thread_item_delta"
        } else {
            event_type
        };
        let event = StreamEvent {
            event: online_event_type.to_string(),
            data: payload,
            id: Some(event_id.to_string()),
            timestamp: Some(timestamp),
        };
        if matches!(event_type, "context_usage" | "round_usage") {
            if let Some(committer) = self.committer.as_ref() {
                committer.publish_context_usage(&self.session_id, event.data.clone());
            }
        }
        self.enqueue_event(&event, false).await;
        event
    }

    fn enrich_llm_output_message_stats(&self, event_type: &str, data: &mut Value) {
        if event_type != "llm_output" {
            return;
        }
        let Some(map) = data.as_object_mut() else {
            return;
        };
        let usage = map.get("usage").cloned().unwrap_or(Value::Null);
        let cumulative = self.accumulated_usage();
        // Mock/replay providers emit the output before their diagnostic usage
        // event. Keep the current response as a useful round snapshot rather
        // than persisting an empty aggregate in that ordering.
        let round_usage = if cumulative.total > 0 || cumulative.input > 0 || cumulative.output > 0 {
            json!(cumulative)
        } else {
            usage.clone()
        };
        let mut stats = map
            .get("message_stats")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        stats.insert("usage".into(), usage.clone());
        stats.insert("round_usage".into(), round_usage);
        stats.insert(
            "model_request_count".into(),
            json!(self.accumulated_model_requests().max(0)),
        );
        stats.insert(
            "account_credits_consumed".into(),
            json!(self
                .accumulated_billable_account_credits(self.is_admin)
                .max(0)),
        );
        if let Some(input) = usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .or_else(|| usage.get("input").and_then(Value::as_u64))
        {
            stats.insert("contextTokens".into(), json!(input));
            stats.insert("context_occupancy_tokens".into(), json!(input));
        }
        for key in [
            "interaction_duration_s",
            "prefill_duration_s",
            "decode_duration_s",
            "decode_output_tokens",
            "decode_tokens",
            "decode_speed_tps",
            "prefill_speed_tps",
            "visible_decode_tokens",
            "visible_decode_duration_s",
            "visible_decode_speed_tps",
            "visible_decode_measured",
            "stream_timing",
        ] {
            if let Some(value) = map.get(key) {
                stats.insert(key.into(), value.clone());
            }
        }
        // A provider may expose only prefill/decode spans.  Their sum is the
        // best available per-round elapsed time and keeps the footer useful.
        if !stats.contains_key("interaction_duration_s") {
            let prefill = stats
                .get("prefill_duration_s")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let decode = stats
                .get("decode_duration_s")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            if prefill + decode > 0.0 {
                stats.insert("interaction_duration_s".into(), json!(prefill + decode));
            }
        }
        map.insert("message_stats".into(), Value::Object(stats));
    }

    /// Translate a provider delta into independent content/reasoning tail
    /// frames. Browser strings use UTF-16 indices, so Rust byte lengths and
    /// scalar counts are both wrong for a supplementary Unicode character.
    fn llm_output_tails(&self, event_type: &str, data: &Value) -> Vec<Value> {
        if event_type != "llm_output_delta" {
            return Vec::new();
        }
        let Some(turn_id) = data.get("turn_id").and_then(Value::as_str) else {
            return Vec::new();
        };
        let turn_id = turn_id.trim();
        if turn_id.is_empty() {
            return Vec::new();
        }
        let model_round = data.get("model_round").and_then(Value::as_i64).unwrap_or(0);
        let item_id = format!("{turn_id}:text-{model_round}");
        let mut offsets = self.tail_offsets.lock();
        let mut tails = Vec::with_capacity(2);
        for (source, field) in [("delta", "content"), ("reasoning_delta", "reasoning")] {
            let Some(text) = data
                .get(source)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            let key = (item_id.clone(), field.to_string());
            let offset = offsets.get(&key).copied().unwrap_or(0);
            offsets.insert(key, offset.saturating_add(text.encode_utf16().count()));
            // No assistant message has been committed while it is streaming;
            // the first durable item will replace the transient tail.
            tails.push(json!({
                "item_id": item_id,
                "field": field,
                "offset": offset,
                "base_seq": 0,
                "text": text,
            }));
        }
        tails
    }

    async fn enqueue_event(&self, event: &StreamEvent, persisted: bool) {
        if self.closed.load(AtomicOrdering::SeqCst) {
            if !persisted {
                self.record_overflow(event).await;
            }
            return;
        }
        if let Some(queue) = &self.queue {
            match queue.try_send(StreamSignal::Event(event.clone())) {
                Ok(_) => (),
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    if !persisted {
                        self.record_overflow(event).await;
                    }
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    if should_backpressure_online_stream_event(&event.event) {
                        if queue
                            .send(StreamSignal::Event(event.clone()))
                            .await
                            .is_err()
                        {
                            if !persisted {
                                self.record_overflow(event).await;
                            }
                        }
                        return;
                    }
                    if !persisted {
                        self.record_overflow(event).await;
                    }
                }
            }
        }
    }

    async fn record_overflow(&self, _event: &StreamEvent) {
        // The online queue is bounded. A dropped diagnostic frame never creates
        // a second durable event log; clients recover committed state by cursor.
    }
}

impl Orchestrator {
    pub(super) fn spawn_stream_pump(
        &self,
        session_id: String,
        mut queue_rx: mpsc::Receiver<StreamSignal>,
        event_tx: mpsc::Sender<StreamEvent>,
        emitter: EventEmitter,
        runner: JoinHandle<()>,
    ) {
        let thread_runtime = self.thread_runtime.clone();
        long_task::spawn("orchestrator.stream_pump", async move {
            let mut closed = false;
            loop {
                if !closed {
                    match queue_rx.recv().await {
                        Some(StreamSignal::Done) => {
                            closed = true;
                            continue;
                        }
                        Some(StreamSignal::Event(event)) => {
                            if event_tx.send(event).await.is_err() {
                                emitter.close();
                                break;
                            }
                        }
                        None => {
                            closed = true;
                        }
                    }
                }
                if closed && runner.is_finished() {
                    break;
                }
                if closed && queue_rx.is_closed() {
                    break;
                }
                if runner.is_finished() && queue_rx.is_empty() {
                    break;
                }
            }
            let detach = thread_runtime.detach_subscriber(&session_id);
            if let Some(closed_event) = detach.closed {
                emitter
                    .emit("thread_status", thread_not_loaded_payload(&closed_event))
                    .await;
                emitter
                    .emit("thread_closed", thread_closed_payload(&closed_event))
                    .await;
            }
            emitter.close();
        });
    }
}

fn enrich_event_payload(data: Value, session_id: Option<&str>, timestamp: DateTime<Utc>) -> Value {
    let mut map = serde_json::Map::new();
    if let Some(session_id) = session_id {
        let cleaned = session_id.trim();
        if !cleaned.is_empty() {
            map.insert("session_id".to_string(), Value::String(cleaned.to_string()));
        }
    }
    map.insert(
        "timestamp".to_string(),
        Value::String(timestamp.with_timezone(&Local).to_rfc3339()),
    );
    map.insert("data".to_string(), data);
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_llm_delta_uses_backpressure_instead_of_lossy_overflow() {
        assert!(should_backpressure_online_stream_event("thread_item_delta"));
        assert!(should_backpressure_online_stream_event("llm_output_delta"));
        assert!(should_backpressure_online_stream_event(
            "command_session_delta"
        ));
        assert!(!should_backpressure_online_stream_event("progress"));
    }
}

fn inject_client_message_id(data: Value, client_message_id: Option<&str>) -> Value {
    let Some(client_message_id) = client_message_id else {
        return data;
    };
    let trimmed = client_message_id.trim();
    if trimmed.is_empty() {
        return data;
    }
    match data {
        Value::Object(mut map) => {
            map.entry("client_message_id".to_string())
                .or_insert_with(|| json!(trimmed));
            Value::Object(map)
        }
        other => other,
    }
}
