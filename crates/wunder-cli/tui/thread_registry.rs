//! Thread-scoped state used by the TUI orchestration layer.
//!
//! The registry deliberately contains only bounded UI projection data. Durable
//! history and complete tool output remain in runtime storage and are loaded on
//! demand when a thread becomes visible.

use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc::Receiver;
use wunder_server::approval::ApprovalRequestRx;
use wunder_server::schemas::StreamEvent;

use super::app::StreamMessage;

pub(crate) const MAX_THREAD_PROJECTIONS: usize = 32;
pub(crate) const MAX_PENDING_EVENTS: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadRunState {
    Ready,
    Working,
    NeedsYou,
    Failed,
    Finished,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct UnreadState {
    pub events: usize,
    pub completed_turns: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ThreadProjection {
    pub session_id: String,
    pub draft: String,
    pub scroll_from_bottom: usize,
    pub last_seen_event_id: i64,
    pub status: ThreadRunState,
    pub unread: UnreadState,
    pub pending_events: VecDeque<StreamEvent>,
    pub needs_replay: bool,
    pub replay_from: Option<i64>,
    applied_event_ids: HashSet<i64>,
    last_applied_event_id: i64,
}

impl ThreadProjection {
    pub(crate) fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            draft: String::new(),
            scroll_from_bottom: 0,
            last_seen_event_id: 0,
            status: ThreadRunState::Ready,
            unread: UnreadState::default(),
            pending_events: VecDeque::new(),
            needs_replay: false,
            replay_from: None,
            applied_event_ids: HashSet::new(),
            last_applied_event_id: 0,
        }
    }

    pub(crate) fn observe_event(&mut self, event_id: i64, active: bool) {
        if event_id > self.last_seen_event_id {
            self.last_seen_event_id = event_id;
        }
        if !active {
            self.unread.events = self.unread.events.saturating_add(1);
        }
    }

    pub(crate) fn mark_visible(&mut self) {
        self.unread = UnreadState::default();
    }

    pub(crate) fn queue_event(&mut self, event: StreamEvent) {
        if self.pending_events.len() >= MAX_PENDING_EVENTS {
            self.pending_events.pop_front();
            self.needs_replay = true;
            // Event IDs may be sparse; received IDs are not applied cursors.
            self.replay_from.get_or_insert(self.last_applied_event_id);
        }
        self.pending_events.push_back(event);
    }
}

#[derive(Default)]
pub(crate) struct ThreadRegistry {
    active_thread_id: Option<String>,
    projections: HashMap<String, ThreadProjection>,
    streams: HashMap<String, Receiver<StreamMessage>>,
    approval_streams: HashMap<String, ApprovalRequestRx>,
    pending_approvals: HashMap<String, VecDeque<wunder_server::approval::ApprovalRequest>>,
}

impl ThreadRegistry {
    pub(crate) fn new(active_thread_id: impl Into<String>) -> Self {
        let active = active_thread_id.into();
        let mut registry = Self::default();
        registry.active_thread_id = Some(active.clone());
        registry
            .projections
            .insert(active.clone(), ThreadProjection::new(active));
        registry
    }

    pub(crate) fn active_thread_id(&self) -> Option<&str> {
        self.active_thread_id.as_deref()
    }

    pub(crate) fn projection(&self, session_id: &str) -> Option<&ThreadProjection> {
        self.projections.get(session_id)
    }

    pub(crate) fn save_view_state(&mut self, session_id: &str, draft: String, scroll: usize) {
        let projection = self.projection_mut(session_id);
        projection.draft = draft;
        projection.scroll_from_bottom = scroll;
    }

    pub(crate) fn view_state(&self, session_id: &str) -> (String, usize) {
        self.projection(session_id)
            .map(|projection| (projection.draft.clone(), projection.scroll_from_bottom))
            .unwrap_or_default()
    }

    pub(crate) fn unread_events(&self, session_id: &str) -> usize {
        self.projection(session_id)
            .map(|projection| projection.unread.events)
            .unwrap_or(0)
    }

    pub(crate) fn needs_replay(&self, session_id: &str) -> bool {
        self.projection(session_id)
            .is_some_and(|projection| projection.needs_replay)
    }

    pub(crate) fn replay_from(&self, session_id: &str) -> Option<i64> {
        self.projection(session_id)
            .and_then(|projection| projection.replay_from)
    }

    pub(crate) fn mark_event_applied(&mut self, session_id: &str, event_id: i64) -> bool {
        if event_id <= 0 {
            return true;
        }
        let projection = self.projection_mut(session_id);
        if !projection.applied_event_ids.insert(event_id) {
            return false;
        }
        projection.last_applied_event_id = projection.last_applied_event_id.max(event_id);
        if projection.applied_event_ids.len() > MAX_PENDING_EVENTS * 2 {
            let floor = event_id.saturating_sub(MAX_PENDING_EVENTS as i64);
            projection.applied_event_ids.retain(|id| *id >= floor);
        }
        true
    }

    pub(crate) fn clear_replay(&mut self, session_id: &str) {
        let projection = self.projection_mut(session_id);
        projection.needs_replay = false;
        projection.replay_from = None;
    }

    pub(crate) fn projection_mut(&mut self, session_id: &str) -> &mut ThreadProjection {
        self.projections
            .entry(session_id.to_string())
            .or_insert_with(|| ThreadProjection::new(session_id))
    }

    pub(crate) fn take_pending_events(
        &mut self,
        session_id: &str,
        limit: usize,
    ) -> Vec<StreamEvent> {
        let projection = self.projection_mut(session_id);
        let count = limit.min(projection.pending_events.len());
        projection.pending_events.drain(..count).collect()
    }

    pub(crate) fn activate(&mut self, session_id: &str) {
        self.projection_mut(session_id).mark_visible();
        self.active_thread_id = Some(session_id.to_string());
        self.evict_inactive();
    }

    pub(crate) fn record_event(&mut self, session_id: &str, event_id: i64) {
        let active = self.active_thread_id() == Some(session_id);
        self.projection_mut(session_id)
            .observe_event(event_id, active);
    }

    pub(crate) fn set_status(&mut self, session_id: &str, status: ThreadRunState) {
        self.projection_mut(session_id).status = status;
    }

    pub(crate) fn insert_stream(&mut self, session_id: String, receiver: Receiver<StreamMessage>) {
        self.streams.insert(session_id, receiver);
    }

    pub(crate) fn stream_session_ids(&self) -> Vec<String> {
        self.streams.keys().cloned().collect()
    }

    pub(crate) fn stream_receiver_mut(
        &mut self,
        session_id: &str,
    ) -> Option<&mut Receiver<StreamMessage>> {
        self.streams.get_mut(session_id)
    }

    pub(crate) fn remove_stream(&mut self, session_id: &str) {
        self.streams.remove(session_id);
    }

    pub(crate) fn has_stream(&self, session_id: &str) -> bool {
        self.streams.contains_key(session_id)
    }

    pub(crate) fn total_stream_depth(&self) -> usize {
        self.streams.values().map(Receiver::len).sum()
    }

    pub(crate) fn insert_approval_stream(
        &mut self,
        session_id: String,
        receiver: ApprovalRequestRx,
    ) {
        self.approval_streams.insert(session_id, receiver);
    }

    pub(crate) fn approval_session_ids(&self) -> Vec<String> {
        self.approval_streams.keys().cloned().collect()
    }

    pub(crate) fn approval_receiver_mut(
        &mut self,
        session_id: &str,
    ) -> Option<&mut ApprovalRequestRx> {
        self.approval_streams.get_mut(session_id)
    }

    pub(crate) fn remove_approval_stream(&mut self, session_id: &str) {
        self.approval_streams.remove(session_id);
    }

    pub(crate) fn queue_approval(
        &mut self,
        session_id: &str,
        request: wunder_server::approval::ApprovalRequest,
    ) {
        self.pending_approvals
            .entry(session_id.to_string())
            .or_default()
            .push_back(request);
    }

    pub(crate) fn take_pending_approval(
        &mut self,
        session_id: &str,
    ) -> Option<wunder_server::approval::ApprovalRequest> {
        self.pending_approvals
            .get_mut(session_id)
            .and_then(VecDeque::pop_front)
    }

    pub(crate) fn pending_approval_count(&self, session_id: &str) -> usize {
        self.pending_approvals
            .get(session_id)
            .map(VecDeque::len)
            .unwrap_or(0)
    }

    fn evict_inactive(&mut self) {
        // Projections hold queued events that were already consumed from the
        // stream receiver. Evicting a thread with a live stream or a pending
        // approval would silently drop that work, so only projections without
        // either are evictable. When every inactive projection is protected,
        // keep the current surface: live streams bound this soft overflow.
        while self.projections.len() > MAX_THREAD_PROJECTIONS {
            let Some(id) = self
                .projections
                .keys()
                .filter(|id| self.active_thread_id.as_deref() != Some(id.as_str()))
                .filter(|id| {
                    !self.streams.contains_key(*id)
                        && !self.approval_streams.contains_key(*id)
                        && !self
                            .pending_approvals
                            .get(*id)
                            .is_some_and(|queue| !queue.is_empty())
                })
                .next()
                .cloned()
            else {
                break;
            };
            self.projections.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn replay_starts_at_applied_cursor_even_when_received_ids_are_sparse() {
        let mut registry = ThreadRegistry::new("a");
        assert!(registry.mark_event_applied("b", 19));
        registry.record_event("b", 900_000);
        for _ in 0..=MAX_PENDING_EVENTS {
            registry.projection_mut("b").queue_event(StreamEvent {
                event: "delta".into(),
                data: serde_json::Value::Null,
                id: None,
                timestamp: None,
            });
        }
        assert_eq!(registry.replay_from("b"), Some(19));
        assert!(!registry.mark_event_applied("b", 19));
        assert!(registry.mark_event_applied("b", 400));
        assert!(!registry.mark_event_applied("b", 400));
    }

    #[test]
    fn inactive_events_are_unread_and_activation_clears_them() {
        let mut registry = ThreadRegistry::new("a");
        registry.record_event("b", 7);
        assert_eq!(registry.projection("b").unwrap().unread.events, 1);
        registry.activate("b");
        assert_eq!(registry.projection("b").unwrap().unread.events, 0);
    }

    #[test]
    fn pending_events_are_thread_scoped_and_bounded() {
        let mut registry = ThreadRegistry::new("a");
        for index in 0..(MAX_PENDING_EVENTS + 3) {
            registry.projection_mut("b").queue_event(StreamEvent {
                event: index.to_string(),
                data: serde_json::Value::Null,
                id: None,
                timestamp: None,
            });
        }
        assert!(registry.take_pending_events("a", 10).is_empty());
        let events = registry.take_pending_events("b", MAX_PENDING_EVENTS + 1);
        assert_eq!(events.len(), MAX_PENDING_EVENTS);
        assert_eq!(events.first().unwrap().event, "3");
        assert!(registry.needs_replay("b"));
    }

    #[test]
    fn stream_receivers_are_isolated_by_session() {
        let mut registry = ThreadRegistry::new("a");
        let (_tx_a, rx_a) = mpsc::channel(1);
        let (_tx_b, rx_b) = mpsc::channel(1);
        registry.insert_stream("a".to_string(), rx_a);
        registry.insert_stream("b".to_string(), rx_b);
        assert!(registry.has_stream("a"));
        assert!(registry.has_stream("b"));
        registry.remove_stream("a");
        assert!(!registry.has_stream("a"));
        assert!(registry.has_stream("b"));
    }

    fn delta_event(sequence: usize) -> StreamEvent {
        StreamEvent {
            event: "llm_output_delta".into(),
            data: serde_json::json!({ "text": format!("t{sequence}") }),
            id: None,
            timestamp: None,
        }
    }

    /// Load gate from the plan (section 6): at least 20 directory threads with
    /// 4 live streams, high-frequency deltas on every stream. Queues stay
    /// bounded, the visible thread keeps every character until the queue
    /// overflows, and overflow flags only the affected thread for replay.
    #[test]
    fn stress_directory_threads_and_live_streams_keep_queues_bounded_and_isolated() {
        let mut registry = ThreadRegistry::new("visible");
        let streaming = ["stream-1", "stream-2", "stream-3", "stream-4"];
        let catalog_threads = 20usize;
        let deltas_per_stream = 600usize;

        // Directory threads without streams only carry catalog metadata.
        for index in 0..catalog_threads {
            let id = format!("idle-{index}");
            registry.projection_mut(&id);
        }
        for id in streaming {
            let (_tx, rx) = mpsc::channel(1);
            registry.insert_stream(id.to_string(), rx);
            registry.projection_mut(id);
        }

        // High-frequency deltas on all four streams; "visible" is one of them.
        let visible_stream = "stream-1";
        for sequence in 0..deltas_per_stream {
            for (stream_index, id) in streaming.iter().enumerate() {
                let event = delta_event(sequence);
                registry.record_event(id, (sequence + 1) as i64);
                registry.projection_mut(id).queue_event(event);
                let _ = stream_index;
            }
        }

        // Bounded queues everywhere.
        for id in streaming {
            let projection = registry.projection(id).expect("streaming projection kept");
            assert!(projection.pending_events.len() <= MAX_PENDING_EVENTS);
        }
        assert!(registry.projections.len() <= MAX_THREAD_PROJECTIONS);

        // The visible thread drains through the per-frame budget; no silent
        // loss while the queue never overflows.
        registry.activate(visible_stream);
        let mut drained = Vec::new();
        loop {
            let batch = registry.take_pending_events(visible_stream, 400);
            if batch.is_empty() {
                break;
            }
            for event in batch {
                if let Some(text) = event.data.get("text").and_then(serde_json::Value::as_str) {
                    drained.push(text.to_string());
                }
            }
        }
        let visible_text: String = drained.join("");
        let expected: String = (0..deltas_per_stream).map(|s| format!("t{s}")).collect();
        assert_eq!(visible_text, expected);
        assert!(!registry.needs_replay(visible_stream));

        // Overflow on one background thread flags replay for that thread only.
        let overflowing = "stream-2";
        for sequence in 0..=(MAX_PENDING_EVENTS + 50) {
            registry.projection_mut(overflowing).queue_event(delta_event(sequence));
            registry.record_event(overflowing, (sequence + 1) as i64);
        }
        assert!(registry.needs_replay(overflowing));
        assert!(registry.replay_from(overflowing).is_some());
        for id in ["stream-3", "stream-4"] {
            assert!(!registry.needs_replay(id), "{id} must not be flagged");
        }

        // Draining one thread never returns another thread's events.
        let batch = registry.take_pending_events(overflowing, 64);
        assert_eq!(batch.len(), 64);
        let other_batch = registry.take_pending_events("stream-3", 16);
        assert_eq!(other_batch.len(), 16);
    }

    /// Eviction keeps streaming and approval-holding projections even when the
    /// directory grows past the cache cap; only quiet threads are evicted.
    #[test]
    fn eviction_protects_streams_and_pending_approvals() {
        let mut registry = ThreadRegistry::new("visible");
        let protected_stream = "protected-stream";
        let (_tx, rx) = mpsc::channel(1);
        registry.insert_stream(protected_stream.to_string(), rx);
        registry.projection_mut(protected_stream);
        let (respond_to, _respond) = tokio::sync::oneshot::channel();
        registry.queue_approval(
            "protected-approval",
            wunder_server::approval::ApprovalRequest {
                id: "approval-1".into(),
                kind: wunder_server::approval::ApprovalRequestKind::Exec,
                tool: "sample_tool".into(),
                args: serde_json::Value::Null,
                summary: "sample approval".into(),
                detail: serde_json::Value::Null,
                respond_to,
            },
        );
        drop(_respond);
        registry.projection_mut("protected-approval");
        let total = MAX_THREAD_PROJECTIONS + 24;
        for index in 0..total {
            registry.projection_mut(&format!("quiet-{index}"));
        }
        registry.activate("visible");
        assert!(registry.projection(protected_stream).is_some());
        assert!(registry.projection("protected-approval").is_some());
        let quiet_remaining = registry
            .projections
            .keys()
            .filter(|id| id.starts_with("quiet-"))
            .count();
        assert!(quiet_remaining < total);
    }
}

#[cfg(test)]
#[path = "thread_registry_load_tests.rs"]
mod thread_registry_load_tests;
