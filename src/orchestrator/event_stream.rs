use super::*;

pub(super) enum StreamSignal {
    Event(StreamEvent),
    Done,
}

struct StreamDeltaSegment {
    event_id: i64,
    delta: Option<String>,
    reasoning_delta: Option<String>,
    round: Option<i64>,
}

struct StreamDeltaBuffer {
    segments: Vec<StreamDeltaSegment>,
    total_chars: usize,
    first_event_id: i64,
    last_event_id: i64,
    last_flush: Instant,
}

impl StreamDeltaBuffer {
    fn new() -> Self {
        Self {
            segments: Vec::new(),
            total_chars: 0,
            first_event_id: 0,
            last_event_id: 0,
            last_flush: Instant::now(),
        }
    }

    fn push(&mut self, event_id: i64, data: &Value) {
        let delta = data
            .get("delta")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let reasoning_delta = data
            .get("reasoning_delta")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let round = data.get("round").and_then(Value::as_i64);
        if delta.is_empty() && reasoning_delta.is_empty() && round.is_none() {
            return;
        }
        if event_id > 0 {
            if self.first_event_id == 0 {
                self.first_event_id = event_id;
            }
            self.last_event_id = event_id;
        }
        self.total_chars = self
            .total_chars
            .saturating_add(delta.len())
            .saturating_add(reasoning_delta.len());
        self.segments.push(StreamDeltaSegment {
            event_id,
            delta: if delta.is_empty() { None } else { Some(delta) },
            reasoning_delta: if reasoning_delta.is_empty() {
                None
            } else {
                Some(reasoning_delta)
            },
            round,
        });
    }

    fn should_flush(&self) -> bool {
        if self.segments.is_empty() {
            return false;
        }
        if self.total_chars >= STREAM_EVENT_PERSIST_CHARS {
            return true;
        }
        self.last_flush.elapsed().as_millis() as u64 >= STREAM_EVENT_PERSIST_INTERVAL_MS
    }

    fn take_payload(&mut self) -> Option<(i64, Value)> {
        if self.segments.is_empty() {
            return None;
        }
        let mut segments = Vec::with_capacity(self.segments.len());
        for segment in self.segments.drain(..) {
            let mut item = serde_json::Map::new();
            item.insert("event_id".to_string(), json!(segment.event_id));
            if let Some(delta) = segment.delta {
                item.insert("delta".to_string(), Value::String(delta));
            }
            if let Some(reasoning_delta) = segment.reasoning_delta {
                item.insert(
                    "reasoning_delta".to_string(),
                    Value::String(reasoning_delta),
                );
            }
            if let Some(round) = segment.round {
                item.insert("round".to_string(), json!(round));
            }
            segments.push(Value::Object(item));
        }
        let event_id = self.last_event_id;
        let mut payload = serde_json::Map::new();
        payload.insert("segments".to_string(), Value::Array(segments));
        if self.first_event_id > 0 && self.last_event_id > 0 {
            payload.insert("event_id_start".to_string(), json!(self.first_event_id));
            payload.insert("event_id_end".to_string(), json!(self.last_event_id));
        }
        self.total_chars = 0;
        self.first_event_id = 0;
        self.last_event_id = 0;
        self.last_flush = Instant::now();
        Some((event_id, Value::Object(payload)))
    }
}

fn should_persist_stream_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "progress"
            | "llm_request"
            | "llm_response"
            | "knowledge_request"
            | "compaction"
            | "tool_call"
            | "tool_result"
            | "plan_update"
            | "question_panel"
            | "llm_output_delta"
            | "llm_output"
            | "context_usage"
            | "quota_usage"
            | "final"
            | "error"
    )
}

#[derive(Clone)]
pub(super) struct EventEmitter {
    session_id: String,
    user_id: String,
    queue: Option<mpsc::Sender<StreamSignal>>,
    storage: Option<Arc<dyn StorageBackend>>,
    monitor: Arc<MonitorState>,
    closed: Arc<AtomicBool>,
    next_event_id: Arc<AtomicI64>,
    last_cleanup_ts: Arc<AtomicU64>,
    delta_buffer: Option<Arc<ParkingMutex<StreamDeltaBuffer>>>,
}

impl EventEmitter {
    pub(super) fn new(
        session_id: String,
        user_id: String,
        queue: Option<mpsc::Sender<StreamSignal>>,
        storage: Option<Arc<dyn StorageBackend>>,
        monitor: Arc<MonitorState>,
    ) -> Self {
        let delta_buffer = storage
            .as_ref()
            .map(|_| Arc::new(ParkingMutex::new(StreamDeltaBuffer::new())));
        Self {
            session_id,
            user_id,
            queue,
            storage,
            monitor,
            closed: Arc::new(AtomicBool::new(false)),
            next_event_id: Arc::new(AtomicI64::new(1)),
            last_cleanup_ts: Arc::new(AtomicU64::new(0)),
            delta_buffer,
        }
    }

    fn close(&self) {
        self.closed.store(true, AtomicOrdering::SeqCst);
    }

    pub(super) async fn finish(&self) {
        self.flush_delta_buffer(true);
        let Some(queue) = &self.queue else {
            return;
        };
        if self.closed.load(AtomicOrdering::SeqCst) {
            return;
        }
        let _ = queue.try_send(StreamSignal::Done);
    }

    fn flush_delta_buffer(&self, force: bool) {
        let Some(buffer) = &self.delta_buffer else {
            return;
        };
        let payload = {
            let mut guard = buffer.lock();
            if !force && !guard.should_flush() {
                return;
            }
            guard.take_payload()
        };
        if let Some((event_id, data)) = payload {
            let timestamp = Utc::now();
            self.persist_stream_event(event_id, "llm_output_delta", data, timestamp);
        }
    }

    fn buffer_delta(&self, event_id: i64, data: &Value) {
        let Some(buffer) = &self.delta_buffer else {
            return;
        };
        let payload = {
            let mut guard = buffer.lock();
            guard.push(event_id, data);
            if !guard.should_flush() {
                return;
            }
            guard.take_payload()
        };
        if let Some((event_id, data)) = payload {
            let timestamp = Utc::now();
            self.persist_stream_event(event_id, "llm_output_delta", data, timestamp);
        }
    }

    fn persist_stream_event(
        &self,
        event_id: i64,
        event_type: &str,
        data: Value,
        timestamp: DateTime<Utc>,
    ) {
        let Some(storage) = &self.storage else {
            return;
        };
        if event_id <= 0 || event_type.trim().is_empty() {
            return;
        }
        let payload = json!({
            "event": event_type,
            "data": enrich_event_payload(data, Some(&self.session_id), timestamp),
            "timestamp": timestamp.with_timezone(&Local).to_rfc3339(),
        });
        let session_id = self.session_id.clone();
        let user_id = self.user_id.clone();
        let storage = storage.clone();
        let cleanup_cutoff = self.cleanup_cutoff();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || {
                let _ = storage.append_stream_event(&session_id, &user_id, event_id, &payload);
                if let Some(cutoff) = cleanup_cutoff {
                    let _ = storage.delete_stream_events_before(cutoff);
                }
            });
        } else {
            let _ = storage.append_stream_event(&session_id, &user_id, event_id, &payload);
            if let Some(cutoff) = cleanup_cutoff {
                let _ = storage.delete_stream_events_before(cutoff);
            }
        }
    }

    fn persist_event_on_emit(
        &self,
        event_id: i64,
        event_type: &str,
        data: &Value,
        timestamp: DateTime<Utc>,
    ) -> bool {
        if self.storage.is_none() {
            return false;
        }
        if !should_persist_stream_event(event_type) {
            return false;
        }
        if event_type == "llm_output_delta" {
            self.buffer_delta(event_id, data);
            return true;
        }
        self.persist_stream_event(event_id, event_type, data.clone(), timestamp);
        true
    }

    pub(super) async fn emit(&self, event_type: &str, data: Value) -> StreamEvent {
        let timestamp = Utc::now();
        let event_id = self.next_event_id.fetch_add(1, AtomicOrdering::SeqCst);
        if event_type != "llm_output_delta" {
            self.flush_delta_buffer(true);
        }
        self.monitor
            .record_event(&self.session_id, event_type, &data);
        let persisted = self.persist_event_on_emit(event_id, event_type, &data, timestamp);
        let payload = enrich_event_payload(data, Some(&self.session_id), timestamp);
        let event = StreamEvent {
            event: event_type.to_string(),
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
                Ok(_) => return,
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    if !persisted {
                        self.record_overflow(event).await;
                    }
                    return;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    if !persisted {
                        self.record_overflow(event).await;
                    }
                    return;
                }
            }
        }
    }

    async fn record_overflow(&self, event: &StreamEvent) {
        if self.storage.is_none() {
            return;
        }
        let Some(event_id) = event.id.as_ref().and_then(|text| text.parse::<i64>().ok()) else {
            return;
        };
        if !should_persist_stream_event(&event.event) {
            return;
        }
        let raw_data = event
            .data
            .get("data")
            .cloned()
            .unwrap_or_else(|| event.data.clone());
        let timestamp = event.timestamp.unwrap_or_else(Utc::now);
        if event.event == "llm_output_delta" {
            if self.delta_buffer.is_some() {
                self.buffer_delta(event_id, &raw_data);
                return;
            }
        }
        self.persist_stream_event(event_id, &event.event, raw_data, timestamp);
    }

    fn cleanup_cutoff(&self) -> Option<f64> {
        let now = Utc::now().timestamp_millis() as u64;
        let last = self.last_cleanup_ts.load(AtomicOrdering::SeqCst);
        let interval_ms = (STREAM_EVENT_CLEANUP_INTERVAL_S * 1000.0) as u64;
        if last > 0 && now.saturating_sub(last) < interval_ms {
            return None;
        }
        self.last_cleanup_ts.store(now, AtomicOrdering::SeqCst);
        let cutoff = Utc::now().timestamp_millis() as f64 / 1000.0 - STREAM_EVENT_TTL_S;
        Some(cutoff)
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
        let storage = self.storage.clone();
        tokio::spawn(async move {
            let mut last_event_id: i64 = 0;
            let mut closed = false;
            let poll_interval = std::time::Duration::from_secs_f64(STREAM_EVENT_POLL_INTERVAL_S);

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
                let mut signal: Option<StreamSignal> = None;
                if !closed {
                    match tokio::time::timeout(poll_interval, queue_rx.recv()).await {
                        Ok(value) => signal = value,
                        Err(_) => signal = None,
                    }
                }

                match signal {
                    Some(StreamSignal::Done) => {
                        closed = true;
                        continue;
                    }
                    Some(StreamSignal::Event(event)) => {
                        let event_id = parse_stream_event_id(&event);
                        if let Some(event_id) = event_id {
                            if event_id > last_event_id + 1 {
                                if !drain_until(
                                    storage.clone(),
                                    &session_id,
                                    &mut last_event_id,
                                    event_id - 1,
                                    &event_tx,
                                    &emitter,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            if event_id <= last_event_id {
                                continue;
                            }
                        }
                        if event_tx.send(event).await.is_err() {
                            emitter.close();
                            return;
                        }
                        if let Some(event_id) = event_id {
                            last_event_id = event_id;
                        }
                        continue;
                    }
                    None => {
                        let overflow = load_overflow_events(
                            storage.clone(),
                            session_id.clone(),
                            last_event_id,
                            STREAM_EVENT_FETCH_LIMIT,
                        )
                        .await;
                        if !overflow.is_empty() {
                            for event in overflow {
                                let event_id = parse_stream_event_id(&event);
                                if event_tx.send(event).await.is_err() {
                                    emitter.close();
                                    return;
                                }
                                if let Some(event_id) = event_id {
                                    last_event_id = event_id;
                                }
                            }
                            continue;
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
    match tokio::task::spawn_blocking(move || {
        load_overflow_events_inner(storage.as_ref(), &session_id, after_event_id, limit)
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
    let records = storage
        .load_stream_events(session_id, after_event_id, limit)
        .unwrap_or_default();
    let mut events = Vec::new();
    for record in records {
        let event_id = record.get("event_id").and_then(Value::as_i64);
        let event_type = record.get("event").and_then(Value::as_str).unwrap_or("");
        if event_type.is_empty() {
            continue;
        }
        let mut data = record.get("data").cloned().unwrap_or(Value::Null);
        if event_type == "llm_output_delta" {
            if let Some(filtered) = filter_delta_segments(&data, after_event_id) {
                data = filtered;
            } else {
                continue;
            }
        }
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

fn filter_delta_segments(data: &Value, after_event_id: i64) -> Option<Value> {
    let Some(obj) = data.as_object() else {
        return Some(data.clone());
    };
    let Some(inner) = obj.get("data") else {
        return Some(data.clone());
    };
    let Some(inner_obj) = inner.as_object() else {
        return Some(data.clone());
    };
    let Some(segments) = inner_obj.get("segments").and_then(Value::as_array) else {
        return Some(data.clone());
    };

    let mut content = String::new();
    let mut reasoning = String::new();
    let mut last_round = None;
    let mut first_event_id = None;
    let mut last_event_id = None;

    for segment in segments {
        let Some(segment_obj) = segment.as_object() else {
            continue;
        };
        let event_id = segment_obj
            .get("event_id")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if event_id <= after_event_id {
            continue;
        }
        if let Some(delta) = segment_obj.get("delta").and_then(Value::as_str) {
            if !delta.is_empty() {
                content.push_str(delta);
            }
        }
        if let Some(delta) = segment_obj.get("reasoning_delta").and_then(Value::as_str) {
            if !delta.is_empty() {
                reasoning.push_str(delta);
            }
        }
        if let Some(round) = segment_obj.get("round").and_then(Value::as_i64) {
            last_round = Some(round);
        }
        if first_event_id.is_none() {
            first_event_id = Some(event_id);
        }
        last_event_id = Some(event_id);
    }

    let Some(start_event_id) = first_event_id else {
        return None;
    };

    let mut new_inner = serde_json::Map::new();
    for (key, value) in inner_obj {
        if key != "segments" {
            new_inner.insert(key.clone(), value.clone());
        }
    }
    if !content.is_empty() {
        new_inner.insert("delta".to_string(), Value::String(content));
    }
    if !reasoning.is_empty() {
        new_inner.insert("reasoning_delta".to_string(), Value::String(reasoning));
    }
    if let Some(round) = last_round {
        new_inner.insert("round".to_string(), json!(round));
    }
    new_inner.insert("event_id_start".to_string(), json!(start_event_id));
    if let Some(end_event_id) = last_event_id {
        new_inner.insert("event_id_end".to_string(), json!(end_event_id));
    }

    let mut new_obj = obj.clone();
    new_obj.insert("data".to_string(), Value::Object(new_inner));
    Some(Value::Object(new_obj))
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

pub(super) fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_filter_delta_segments_trims_overlap() {
        let payload = json!({
            "session_id": "s1",
            "timestamp": "2024-01-01T00:00:00Z",
            "data": {
                "segments": [
                    { "event_id": 1, "delta": "a" },
                    { "event_id": 2, "delta": "b", "reasoning_delta": "x", "round": 1 },
                    { "event_id": 3, "delta": "c", "reasoning_delta": "y" }
                ]
            }
        });
        let filtered = filter_delta_segments(&payload, 1).expect("filtered payload");
        let inner = filtered
            .get("data")
            .and_then(Value::as_object)
            .expect("inner data");
        assert_eq!(inner.get("delta").and_then(Value::as_str), Some("bc"));
        assert_eq!(
            inner.get("reasoning_delta").and_then(Value::as_str),
            Some("xy")
        );
        assert_eq!(inner.get("event_id_start").and_then(Value::as_i64), Some(2));
        assert_eq!(inner.get("event_id_end").and_then(Value::as_i64), Some(3));
        assert_eq!(inner.get("round").and_then(Value::as_i64), Some(1));
    }

    #[test]
    fn test_filter_delta_segments_skips_fully_seen() {
        let payload = json!({
            "session_id": "s1",
            "timestamp": "2024-01-01T00:00:00Z",
            "data": {
                "segments": [
                    { "event_id": 1, "delta": "a" },
                    { "event_id": 2, "delta": "b" }
                ]
            }
        });
        assert!(filter_delta_segments(&payload, 2).is_none());
    }
}
