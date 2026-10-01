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
    overflow_version: Arc<AtomicU64>,
    client_message_id: Option<String>,
    turn_context: Arc<ParkingMutex<Value>>,
    text_tail: Arc<ParkingMutex<crate::services::thread_log::TextTail>>,
    /// Serializes the whole emit path (durable commits + queue enqueues) so
    /// change-cursor order == queue order == wire order for every emitter.
    emit_lock: Arc<tokio::sync::Mutex<()>>,
    change_hub: Option<Arc<crate::orchestrator::thread_change_hub::ThreadChangeHub>>,
    /// Change-stream v2 marker: enables ephemeral tail frames. v1 clients
    /// never receive them.
    change_stream: bool,
    usage: Arc<ParkingMutex<TokenUsage>>,
    model_requests: Arc<ParkingMutex<i64>>,
    account_credits_consumed: Arc<ParkingMutex<i64>>,
}

impl EventEmitter {
    pub(super) fn bind_turn(&self, turn_id: &str, user_round: i64) {
        *self.turn_context.lock() = json!({"turn_id":turn_id,"user_round":user_round});
    }

    pub(super) fn with_change_hub(
        mut self,
        hub: Arc<crate::orchestrator::thread_change_hub::ThreadChangeHub>,
    ) -> Self {
        self.change_hub = Some(hub);
        self
    }

    pub(super) fn with_change_stream(mut self) -> Self {
        self.change_stream = true;
        self
    }

    fn publish_change_cursor(&self, receipt: &Value) {
        if let Some(hub) = &self.change_hub {
            hub.publish(
                &self.session_id,
                receipt.get("cursor").and_then(Value::as_i64).unwrap_or(0),
            );
        }
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
        start_event_id: i64,
        client_message_id: Option<String>,
    ) -> Self {
        let start_event_id = start_event_id.max(0);
        Self {
            session_id,
            user_id,
            queue,
            storage,
            monitor,
            is_admin,
            closed: Arc::new(AtomicBool::new(false)),
            next_event_id: Arc::new(AtomicI64::new(start_event_id.saturating_add(1))),
            overflow_version: Arc::new(AtomicU64::new(0)),
            client_message_id,
            turn_context: Arc::new(ParkingMutex::new(json!({}))),
            text_tail: Arc::new(ParkingMutex::new(Default::default())),
            emit_lock: Arc::new(tokio::sync::Mutex::new(())),
            change_hub: None,
            change_stream: false,
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

    fn note_overflow(&self) {
        self.overflow_version.fetch_add(1, AtomicOrdering::SeqCst);
    }

    fn overflow_version(&self) -> u64 {
        self.overflow_version.load(AtomicOrdering::SeqCst)
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

    fn persist_event_on_emit(
        &self,
        _event_id: i64,
        _event_type: &str,
        _data: &Value,
        _timestamp: DateTime<Utc>,
    ) -> bool {
        // ThreadLog commits and text blocks carry all durable chat state.
        false
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
            if let Some(storage) = self.storage.clone() {
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
                    match crate::core::blocking::run_db("thread_log.text_item", move || {
                        storage.commit_thread_item(&owner, &item)
                    })
                    .await
                    {
                        Ok(Some(receipt)) => {
                            self.publish_change_cursor(&receipt);
                            if let Some(cursor) = receipt.get("cursor").and_then(Value::as_i64) {
                                self.text_tail.lock().set_base_seq(cursor);
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
        let (block, tail_events) = if event_type == "llm_output_delta" {
            let mut tail = self.text_tail.lock();
            let (item_id, content_offset, reasoning_offset) = tail.tail_annotation(&data);
            let block = tail.append(&self.session_id, event_id, &data);
            let mut tail_events = Vec::new();
            if self.change_stream {
                if let Some(item_id) = item_id {
                    if let (Some(offset), Some(text)) = (
                        content_offset,
                        data.get("delta").and_then(Value::as_str),
                    ) {
                        tail_events.push(StreamEvent {
                            event: "thread_item_tail".into(),
                            data: json!({"item_id":item_id,"field":"content","offset":offset,"text":text}),
                            id: None,
                            timestamp: Some(timestamp),
                        });
                    }
                    if let (Some(offset), Some(text)) = (
                        reasoning_offset,
                        data.get("reasoning_delta").and_then(Value::as_str),
                    ) {
                        tail_events.push(StreamEvent {
                            event: "thread_item_tail".into(),
                            data: json!({"item_id":item_id,"field":"reasoning","offset":offset,"text":text}),
                            id: None,
                            timestamp: Some(timestamp),
                        });
                    }
                }
            }
            (block, tail_events)
        } else if matches!(
            event_type,
            "llm_output" | "llm_request" | "error" | "turn_terminal"
        ) {
            (self.text_tail.lock().flush(&self.session_id), Vec::new())
        } else {
            (None, Vec::new())
        };
        // Durable text blocks are committed through the same emitter gate as
        // lifecycle changes. Tail frames are held until their block commit is
        // visible, then carry that commit cursor as `base_seq`.
        if let (Some(block), Some(storage)) = (block, self.storage.clone()) {
            let blocks = block
                .get("blocks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_else(|| vec![block]);
            for block in blocks {
                let owner = self.user_id.clone();
                let session = self.session_id.clone();
                let block_for_write = block.clone();
                match crate::core::blocking::run_db("thread_log.text_block", move || {
                    storage.upsert_thread_text_block(&owner, &session, &block_for_write)
                })
                .await
                {
                    Ok(cursor) if cursor > 0 => {
                        self.text_tail.lock().set_base_seq(cursor);
                        self.publish_change_cursor(&json!({"cursor": cursor}));
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
        for mut tail_event in tail_events {
            if let Some(map) = tail_event.data.as_object_mut() {
                map.insert("base_seq".into(), json!(self.text_tail.lock().base_seq()));
            }
            self.enqueue_tail(&tail_event).await;
        }
        if let Some(item) =
            crate::services::thread_log::event_item(&self.session_id, event_type, &data)
        {
            if let Some(storage) = self.storage.clone() {
                let owner = self.user_id.clone();
                match crate::core::blocking::run_db("thread_log.event", move || {
                    storage.commit_thread_item(&owner, &item)
                })
                .await
                {
                    Ok(Some(receipt)) => {
                        self.publish_change_cursor(&receipt);
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
        let persisted = self.persist_event_on_emit(event_id, event_type, &data, timestamp);
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
        self.enqueue_event(&event, persisted).await;
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
        self.note_overflow();
    }
}

fn reset_stream_poll_state(
    poll_interval: &mut Duration,
    idle_rounds: &mut usize,
    base_interval: Duration,
) {
    *idle_rounds = 0;
    *poll_interval = base_interval;
}

fn backoff_stream_poll_interval(
    poll_interval: &mut Duration,
    idle_rounds: &mut usize,
    base_interval: Duration,
) {
    *idle_rounds = idle_rounds.saturating_add(1);
    if *idle_rounds <= STREAM_EVENT_RESUME_POLL_BACKOFF_AFTER {
        *poll_interval = base_interval;
        return;
    }
    let next = poll_interval.as_secs_f64() * STREAM_EVENT_RESUME_POLL_BACKOFF_FACTOR;
    *poll_interval = Duration::from_secs_f64(
        next.max(base_interval.as_secs_f64())
            .min(STREAM_EVENT_RESUME_POLL_MAX_INTERVAL_S),
    );
}

impl Orchestrator {
    pub(super) fn spawn_stream_pump(
        &self,
        session_id: String,
        mut queue_rx: mpsc::Receiver<StreamSignal>,
        event_tx: mpsc::Sender<StreamEvent>,
        emitter: EventEmitter,
        runner: JoinHandle<()>,
        start_event_id: i64,
    ) {
        let storage = self.storage.clone();
        let thread_runtime = self.thread_runtime.clone();
        long_task::spawn("orchestrator.stream_pump", async move {
            let mut last_event_id: i64 = start_event_id.max(0);
            let mut closed = false;
            let mut client_open = true;
            let base_interval = Duration::from_secs_f64(STREAM_EVENT_RESUME_POLL_INTERVAL_S);
            let mut poll_interval = base_interval;
            let mut idle_rounds: usize = 0;
            let mut overflow_probe_pending = false;
            let mut seen_overflow_version: u64 = 0;

            async fn drain_until(
                storage: Arc<dyn StorageBackend>,
                session_id: &str,
                last_event_id: &mut i64,
                target_event_id: i64,
                event_tx: &mpsc::Sender<StreamEvent>,
                emitter: &EventEmitter,
            ) -> bool {
                if target_event_id <= *last_event_id {
                    return true;
                }
                let mut current = *last_event_id;
                while current < target_event_id {
                    let events = load_overflow_events(
                        storage.clone(),
                        session_id.to_string(),
                        current,
                        STREAM_EVENT_FETCH_LIMIT,
                    )
                    .await;
                    if events.is_empty() {
                        break;
                    }
                    let mut progressed = false;
                    for event in events {
                        let Some(event_id) = parse_stream_event_id(&event) else {
                            continue;
                        };
                        if event_id <= current {
                            continue;
                        }
                        if event_tx.send(event).await.is_err() {
                            emitter.close();
                            return false;
                        }
                        current = event_id;
                        progressed = true;
                        if current >= target_event_id {
                            break;
                        }
                    }
                    if !progressed {
                        break;
                    }
                }
                *last_event_id = current;
                true
            }

            loop {
                let current_overflow_version = emitter.overflow_version();
                if current_overflow_version > seen_overflow_version {
                    seen_overflow_version = current_overflow_version;
                    overflow_probe_pending = true;
                }

                if !closed {
                    match tokio::time::timeout(poll_interval, queue_rx.recv()).await {
                        Ok(Some(StreamSignal::Done)) => {
                            closed = true;
                            continue;
                        }
                        Ok(Some(StreamSignal::Event(event))) => {
                            let event_id = parse_stream_event_id(&event);
                            if client_open {
                                if let Some(event_id) = event_id {
                                    if event_id > last_event_id + 1
                                        && !drain_until(
                                            storage.clone(),
                                            &session_id,
                                            &mut last_event_id,
                                            event_id - 1,
                                            &event_tx,
                                            &emitter,
                                        )
                                        .await
                                    {
                                        client_open = false;
                                        emitter.close();
                                    }
                                    if event_id <= last_event_id {
                                        reset_stream_poll_state(
                                            &mut poll_interval,
                                            &mut idle_rounds,
                                            base_interval,
                                        );
                                        continue;
                                    }
                                }
                                if let Err(_err) = event_tx.send(event).await {
                                    client_open = false;
                                    emitter.close();
                                } else {
                                    if let Some(event_id) = event_id {
                                        last_event_id = event_id;
                                    }
                                    reset_stream_poll_state(
                                        &mut poll_interval,
                                        &mut idle_rounds,
                                        base_interval,
                                    );
                                    continue;
                                }
                            }
                            if let Some(event_id) = event_id {
                                last_event_id = event_id;
                            }
                            reset_stream_poll_state(
                                &mut poll_interval,
                                &mut idle_rounds,
                                base_interval,
                            );
                            continue;
                        }
                        Ok(None) => {
                            closed = true;
                        }
                        Err(_) => {}
                    }
                }

                if overflow_probe_pending {
                    let overflow = load_overflow_events(
                        storage.clone(),
                        session_id.clone(),
                        last_event_id,
                        STREAM_EVENT_FETCH_LIMIT,
                    )
                    .await;
                    if !overflow.is_empty() {
                        let fetched = overflow.len();
                        for event in overflow {
                            let event_id = parse_stream_event_id(&event);
                            if client_open && event_tx.send(event).await.is_err() {
                                client_open = false;
                                emitter.close();
                            }
                            if let Some(event_id) = event_id {
                                last_event_id = event_id;
                            }
                        }
                        overflow_probe_pending = fetched as i64 >= STREAM_EVENT_FETCH_LIMIT;
                        reset_stream_poll_state(
                            &mut poll_interval,
                            &mut idle_rounds,
                            base_interval,
                        );
                        continue;
                    }
                    overflow_probe_pending = false;
                }

                if closed && runner.is_finished() && !overflow_probe_pending {
                    break;
                }
                if closed && queue_rx.is_closed() && !overflow_probe_pending {
                    break;
                }
                if runner.is_finished() && queue_rx.is_empty() && !overflow_probe_pending {
                    break;
                }

                backoff_stream_poll_interval(&mut poll_interval, &mut idle_rounds, base_interval);
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

fn parse_stream_event_id(event: &StreamEvent) -> Option<i64> {
    event.id.as_ref().and_then(|text| text.parse::<i64>().ok())
}

async fn load_overflow_events(
    storage: Arc<dyn StorageBackend>,
    session_id: String,
    after_event_id: i64,
    limit: i64,
) -> Vec<StreamEvent> {
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() || limit <= 0 {
        return Vec::new();
    }
    let after_event_id = after_event_id.max(0);
    let session_id_for_log = session_id.clone();
    match crate::core::blocking::run_db("orchestrator.event_stream.overflow_load", move || {
        Ok(load_overflow_events_inner(
            storage.as_ref(),
            &session_id,
            after_event_id,
            limit,
        ))
    })
    .await
    {
        Ok(events) => events,
        Err(err) => {
            warn!("failed to load overflow events for session {session_id_for_log}: {err}");
            Vec::new()
        }
    }
}

fn load_overflow_events_inner(
    storage: &dyn StorageBackend,
    session_id: &str,
    after_event_id: i64,
    limit: i64,
) -> Vec<StreamEvent> {
    let records = crate::services::thread_log::replay(storage, session_id, after_event_id, limit)
        .unwrap_or_default();
    let mut events = Vec::new();
    for record in records {
        let event_id = record.get("event_id").and_then(Value::as_i64);
        let event_type = record.get("event").and_then(Value::as_str).unwrap_or("");
        if event_type.is_empty() {
            continue;
        }
        // Recovery blocks replace text at their stored offsets. They must not
        // become append-only deltas, including when only the active tail changed.
        let data = record.get("data").cloned().unwrap_or(Value::Null);
        let timestamp = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let event = StreamEvent {
            event: event_type.to_string(),
            data,
            id: event_id.map(|value| value.to_string()),
            timestamp,
        };
        events.push(event);
    }
    events
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
    use crate::storage::ThreadLogStore;

    #[test]
    fn replay_preserves_block_snapshot_offsets_and_reasoning() {
        let dir = tempfile::tempdir().unwrap();
        let storage = crate::storage::SqliteStorage::new(
            dir.path()
                .join("snapshot.db")
                .to_string_lossy()
                .into_owned(),
        );
        let accepted = storage
            .accept_thread_turn("owner", "thread", &json!({"content":"request"}))
            .unwrap();
        storage
            .append_thread_item(
                "owner",
                &json!({
                    "session_id":"thread", "turn_id":accepted["turn_id"],
                    "item_id":"text-item", "kind":"assistant_message", "visibility":"user",
                    "content":"tail"
                }),
            )
            .unwrap();
        let data = json!({"item_id":"text-item","block_index":1,
            "field":"content","content":"tail","content_offset":8});
        storage
            .upsert_thread_text_block(
                "owner",
                "thread",
                &json!({
                    "event":"thread_item_block","event_id":7,"item_id":"text-item",
                    "block_index":1,"data":data
                }),
            )
            .unwrap();
        let events = load_overflow_events_inner(&storage, "thread", 0, 10);
        let block = events
            .iter()
            .find(|event| event.event == "thread_item_block")
            .expect("block snapshot");
        assert_eq!(block.data, data);
        assert!(block.data.get("delta").is_none());
        assert!(events.iter().any(|event| event.event == "thread_change"));
        let later = load_overflow_events_inner(&storage, "thread", 7, 10);
        assert!(later.iter().any(|event| event.event == "thread_item_block"));
        assert!(!later.iter().any(|event| event.event == "thread_change"));
    }

    #[test]
    fn replay_restores_committed_lifecycle_changes() {
        let dir = tempfile::tempdir().unwrap();
        let storage = crate::storage::SqliteStorage::new(
            dir.path()
                .join("recovery.db")
                .to_string_lossy()
                .into_owned(),
        );
        let accepted = storage
            .accept_thread_turn("owner", "thread", &json!({"content":"request"}))
            .unwrap();
        let turn_id = accepted["turn_id"].as_str().unwrap();
        storage
            .update_thread_turn("owner", "thread", turn_id, "running", "", &json!({}))
            .unwrap();
        let events = load_overflow_events_inner(&storage, "thread", 0, 10);
        assert!(events
            .iter()
            .any(|event| event.event == "thread_change" && event.data["turn_id"] == turn_id));
    }

    #[test]
    fn test_backoff_stream_poll_interval_starts_from_base_interval() {
        let base_interval = Duration::from_secs_f64(STREAM_EVENT_RESUME_POLL_INTERVAL_S);
        let mut poll_interval = base_interval;
        let mut idle_rounds = 0_usize;

        backoff_stream_poll_interval(&mut poll_interval, &mut idle_rounds, base_interval);

        assert_eq!(idle_rounds, 1);
        assert_eq!(poll_interval, base_interval);
    }

    #[test]
    fn test_backoff_stream_poll_interval_caps_at_max_interval() {
        let base_interval = Duration::from_secs_f64(STREAM_EVENT_RESUME_POLL_INTERVAL_S);
        let mut poll_interval = base_interval;
        let mut idle_rounds = 0_usize;

        for _ in 0..12 {
            backoff_stream_poll_interval(&mut poll_interval, &mut idle_rounds, base_interval);
        }

        assert!(poll_interval.as_secs_f64() <= STREAM_EVENT_RESUME_POLL_MAX_INTERVAL_S);
        assert!(poll_interval.as_secs_f64() >= base_interval.as_secs_f64());
    }

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
