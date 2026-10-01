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
        "llm_output_delta"
            | "thread_item_delta"
            | "command_session_delta"
            | "command_session_start"
            | "command_session_status"
            | "command_session_exit"
            | "command_session_summary"
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
    text_tail: Arc<ParkingMutex<crate::services::thread_log::TextTails>>,
    /// Serializes the whole emit path (durable commits + queue enqueues) so
    /// change-cursor order == queue order == wire order for every emitter.
    emit_lock: Arc<tokio::sync::Mutex<()>>,
    committer: Option<Arc<super::thread_log_committer::ThreadLogCommitter>>,
    usage: Arc<ParkingMutex<TokenUsage>>,
    model_requests: Arc<ParkingMutex<i64>>,
    account_credits_consumed: Arc<ParkingMutex<i64>>,
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
            text_tail: Arc::new(ParkingMutex::new(Default::default())),
            emit_lock: Arc::new(tokio::sync::Mutex::new(())),
            committer: None,
            usage: Arc::new(ParkingMutex::new(TokenUsage {
                reasoning: Some(0),
                ..Default::default()
            })),
            model_requests: Arc::new(ParkingMutex::new(0)),
            account_credits_consumed: Arc::new(ParkingMutex::new(0)),
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
        let event_id = self.next_event_id.fetch_add(1, AtomicOrdering::SeqCst);
        let mut data = self.with_client_message_id(data);
        if let Some(map) = data.as_object_mut() {
            if let Some(context) = self.turn_context.lock().as_object() {
                for (key, value) in context {
                    map.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
        // A text block is keyed by the stable assistant Item. Register it at
        // model-call admission, never once per token.
        if event_type == "llm_request" {
            if let Some(committer) = self.committer.clone() {
                if let Some(turn_id) = data.get("turn_id").and_then(Value::as_str) {
                    let model_round = data.get("model_round").and_then(Value::as_i64).unwrap_or(0);
                    let item = json!({
                        "session_id": self.session_id,
                        "turn_id": turn_id,
                        "model_round": model_round,
                        "user_round": data.get("user_round").cloned().unwrap_or(Value::Null),
                        "item_id": format!("{turn_id}:text-{model_round}"),
                        "kind": "assistant_message",
                        "status": "running",
                        "visibility": "user",
                        "role": "assistant",
                        "content": "",
                        "reasoning": ""
                    });
                    let owner = self.user_id.clone();
                    match committer.commit_item(&owner, &item).await {
                        Ok(Some(receipt)) => {
                            if let Some(cursor) = receipt.get("cursor").and_then(Value::as_i64) {
                                self.text_tail
                                    .lock()
                                    .set_base_seq(&format!("{turn_id}:text-{model_round}"), cursor);
                            }
                            let change_event = StreamEvent {
                                event: "thread_change".into(),
                                data: enrich_event_payload(
                                    receipt,
                                    Some(&self.session_id),
                                    timestamp,
                                ),
                                id: None,
                                timestamp: Some(timestamp),
                            };
                            self.enqueue_event(&change_event, true).await;
                        }
                        Ok(None) => {}
                        Err(error) => warn!("persist text item failed: {error}"),
                    }
                }
            }
        }
        let (blocks_to_commit, tail_events) = if event_type == "llm_output_delta" {
            let mut tail = self.text_tail.lock();
            let (blocks, hints) = tail.append(&self.session_id, event_id, &data);
            let mut tail_events = Vec::new();
            for (item_id, offset, field, text) in hints {
                tail_events.push(StreamEvent {
                    event: "thread_item_tail".into(),
                    data: json!({"item_id":item_id,"field":field,"offset":offset,"text":text}),
                    id: None,
                    timestamp: Some(timestamp),
                });
            }
            (blocks, tail_events)
        } else if event_type == "llm_output" {
            let item_id = data.get("turn_id").and_then(Value::as_str).map(|turn| {
                format!(
                    "{turn}:text-{}",
                    data.get("model_round").and_then(Value::as_i64).unwrap_or(0)
                )
            });
            let block = item_id
                .as_deref()
                .and_then(|id| self.text_tail.lock().flush_item(&self.session_id, id));
            (block.into_iter().collect(), Vec::new())
        } else if matches!(event_type, "llm_request" | "error" | "turn_terminal") {
            (
                self.text_tail.lock().flush_all(&self.session_id),
                Vec::new(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        // Durable text blocks are committed through the same emitter gate as
        // lifecycle changes. Tail frames are held until their block commit is
        // visible, then carry that commit cursor as `base_seq`.
        if let Some(committer) = self.committer.clone() {
            for flush in blocks_to_commit {
                let blocks = flush
                    .get("blocks")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_else(|| vec![flush]);
                for block in blocks {
                    let owner = self.user_id.clone();
                    let session = self.session_id.clone();
                    let block_for_write = block.clone();
                    match committer
                        .upsert_text_block(&owner, &session, &block_for_write)
                        .await
                    {
                        Ok(cursor) if cursor > 0 => {
                            if let Some(item_id) = block.get("item_id").and_then(Value::as_str) {
                                self.text_tail.lock().set_base_seq(item_id, cursor);
                            }
                            let turn_id = block
                                .pointer("/data/turn_id")
                                .or_else(|| block.get("turn_id"))
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let change_event = StreamEvent {
                                event: "thread_change".into(),
                                data: enrich_event_payload(
                                    json!({
                                        "change_type": "text_block",
                                        "turn_id": turn_id,
                                        "item_id": block.get("item_id"),
                                        "revision": 0,
                                        "cursor": cursor,
                                        "payload": block,
                                    }),
                                    Some(&self.session_id),
                                    timestamp,
                                ),
                                id: None,
                                timestamp: Some(timestamp),
                            };
                            self.enqueue_event(&change_event, true).await;
                        }
                        Ok(_) => {}
                        Err(error) => warn!("persist thread text block failed: {error}"),
                    }
                }
            }
        }
        for mut tail_event in tail_events {
            if let Some(map) = tail_event.data.as_object_mut() {
                let item_id = map
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                map.insert(
                    "base_seq".into(),
                    json!(self.text_tail.lock().base_seq(item_id)),
                );
            }
            self.enqueue_tail(&tail_event).await;
        }
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
        self.enqueue_event(&event, false).await;
        event
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

    /// Ephemeral tail frames are droppable by design: they carry explicit
    /// offsets and the next durable text block heals any gap. Dropping one
    /// must not bump the overflow probe, which exists for lost durable
    /// events, so tails bypass the accounting entirely.
    async fn enqueue_tail(&self, event: &StreamEvent) {
        if self.closed.load(AtomicOrdering::SeqCst) {
            return;
        }
        if let Some(queue) = &self.queue {
            let _ = queue.try_send(StreamSignal::Event(event.clone()));
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
