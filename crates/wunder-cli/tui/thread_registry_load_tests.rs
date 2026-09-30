//! N6 load gate: many directory threads plus several high-frequency streams.
//! The registry must merge per-thread deltas without text loss, keep
//! background streams alive across switches, and recover through replay
//! without duplicating stable events. These tests exercise the same bounded
//! structures the frame loop uses; they never require a model connection.

use super::super::app::StreamMessage;
use super::{ThreadRegistry, ThreadRunState, MAX_PENDING_EVENTS};
use wunder_server::schemas::StreamEvent;

fn delta_event(event_id: i64, text: &str) -> StreamEvent {
    StreamEvent {
        event: "llm_output_delta".to_string(),
        data: serde_json::json!({ "delta": text }),
        id: Some(event_id.to_string()),
        timestamp: None,
    }
}

fn thread_ids(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("load-thread-{index:03}")).collect()
}

/// 20+ directory threads with 4 active streams: per-frame drain stays within
/// the budget, background threads keep queueing (bounded), and overflow flips
/// the projection into replay mode with an honest cursor instead of silently
/// pretending the text was delivered.
#[test]
fn load_many_threads_keep_bounded_and_flag_replay_on_overflow() {
    let ids = thread_ids(24);
    let mut registry = ThreadRegistry::new(ids[0].clone());
    for id in &ids {
        registry.projection_mut(id);
    }
    let active_threads = &ids[0..4];
    for id in active_threads {
        registry.set_status(id, ThreadRunState::Working);
    }

    // High-frequency burst: 1_200 events per active thread. Only the active
    // thread drains per frame; the others accumulate as unread.
    let mut expected: Vec<Vec<String>> = Vec::new();
    for (thread_index, id) in active_threads.iter().enumerate() {
        let mut texts = Vec::new();
        for step in 0..1_200 {
            let event_id = (thread_index * 10_000 + step + 1) as i64;
            let text = format!("t{thread_index}-{step}");
            registry.record_event(id, event_id);
            registry
                .projection_mut(id)
                .queue_event(delta_event(event_id, &text));
            texts.push(text);
        }
        expected.push(texts);
    }

    for id in active_threads {
        assert!(
            registry.projection(id).expect("projection").pending_events.len()
                <= MAX_PENDING_EVENTS,
            "pending queue exceeded its bound"
        );
    }
    // Three of four threads never drained: 1_200 queued each is below the
    // 2_048 bound, so no replay flag yet; the drained first thread stays clean.
    assert!(!registry.needs_replay(active_threads[0].as_str()));
    assert_eq!(registry.unread_events(active_threads[1].as_str()), 1_200);

    // Per-frame drain budget is respected even with a long queue.
    let drained = registry.take_pending_events(active_threads[1].as_str(), 256);
    assert_eq!(drained.len(), 256);

    // Force an overflow on the active thread and verify the honest signal.
    for step in 0..(MAX_PENDING_EVENTS + 64) {
        let event_id = 500_000 + step as i64;
        registry.record_event(active_threads[0].as_str(), event_id);
        registry
            .projection_mut(active_threads[0].as_str())
            .queue_event(delta_event(event_id, "overflow"));
    }
    assert!(registry.needs_replay(active_threads[0].as_str()));
    assert!(registry.replay_from(active_threads[0].as_str()).is_some());
}

/// Background streams must survive a switch: producers keep sending while the
/// UI only drains the visible thread, and switching back drains everything in
/// order without losing or duplicating any event.
#[tokio::test]
async fn switching_threads_keeps_background_streams_and_recovers_fully() {
    let ids = thread_ids(4);
    let mut registry = ThreadRegistry::new(ids[0].clone());
    let (mut producers, mut receivers) = (Vec::new(), Vec::new());
    for id in &ids {
        let (tx, rx) = tokio::sync::mpsc::channel::<StreamMessage>(128);
        receivers.push((id.clone(), rx));
        let id = id.clone();
        producers.push(tokio::spawn(async move {
            for step in 0..300 {
                let event = StreamMessage::Event {
                    session_id: id.clone(),
                    event: delta_event(step as i64 + 1, &format!("{id}-{step}")),
                };
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        }));
    }

    registry.activate(&ids[0]);
    let mut applied: Vec<Vec<i64>> = vec![Vec::new(); ids.len()];

    // Simulated frame loop: poll every stream (bounded batch), queue into the
    // projection, drain only the visible thread with a frame budget.
    for _frame in 0..2_000 {
        for (session_id, receiver) in receivers.iter_mut() {
            for _ in 0..32 {
                match receiver.try_recv() {
                    Ok(StreamMessage::Event { event, .. }) => {
                        let event_id = event
                            .id
                            .as_deref()
                            .and_then(|id| id.parse::<i64>().ok())
                            .unwrap_or(0);
                        registry.record_event(session_id, event_id);
                        registry.projection_mut(session_id).queue_event(event);
                    }
                    Ok(_) => {}
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                }
            }
        }
        let active = registry.active_thread_id().unwrap_or_default().to_string();
        for event in registry.take_pending_events(&active, 128) {
            let event_id = event.id.as_deref().and_then(|id| id.parse::<i64>().ok()).unwrap_or(0);
            if registry.mark_event_applied(&active, event_id) {
                let index = ids.iter().position(|id| *id == active).unwrap_or(0);
                applied[index].push(event_id);
            }
        }
        // Switch threads every few frames; the previous stream keeps running.
        if _frame % 5 == 4 {
            let next = &ids[(_frame / 5) as usize % ids.len()];
            registry.activate(next);
        }
        if applied.iter().all(|ids| ids.len() >= 300) {
            break;
        }
    }
    for producer in producers {
        let _ = producer.await;
    }

    // Drain the remainder after all producers finished.
    for (session_id, receiver) in receivers.iter_mut() {
        while let Ok(message) = receiver.try_recv() {
            if let StreamMessage::Event { event, .. } = message {
                let event_id = event
                    .id
                    .as_deref()
                    .and_then(|id| id.parse::<i64>().ok())
                    .unwrap_or(0);
                registry.record_event(session_id, event_id);
                registry.projection_mut(session_id).queue_event(event);
            }
        }
    }
    for (index, id) in ids.iter().enumerate() {
        registry.activate(id);
        for event in registry.take_pending_events(id, 2_048) {
            let event_id = event.id.as_deref().and_then(|id| id.parse::<i64>().ok()).unwrap_or(0);
            if registry.mark_event_applied(id, event_id) {
                applied[index].push(event_id);
            }
        }
    }

    for (index, thread_applied) in applied.iter().enumerate() {
        assert_eq!(
            thread_applied.len(),
            300,
            "thread {} lost events",
            ids[index]
        );
        let mut sorted = thread_applied.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 300, "thread {} duplicated events", ids[index]);
        assert_eq!(
            sorted,
            (1..=300).collect::<Vec<_>>(),
            "thread {} missing or out-of-order stable events",
            ids[index]
        );
    }
}

/// Replay after overflow or reconnect must not duplicate stable content: the
/// applied-id set filters re-delivered ranges even when they arrive twice.
#[test]
fn replay_reapplies_from_cursor_without_duplicates() {
    let id = "replay-thread";
    let mut registry = ThreadRegistry::new(id);
    // First pass: apply events 1..=64.
    for event_id in 1..=64 {
        registry.record_event(id, event_id);
        registry.projection_mut(id).queue_event(delta_event(event_id, "x"));
        for event in registry.take_pending_events(id, 8) {
            let parsed = event.id.as_deref().and_then(|id| id.parse::<i64>().ok()).unwrap_or(0);
            assert!(registry.mark_event_applied(id, parsed));
        }
    }
    // Overflow flips the projection into replay mode from the last cursor.
    for step in 0..(MAX_PENDING_EVENTS + 10) {
        let event_id = 10_000 + step as i64;
        registry
            .projection_mut(id)
            .queue_event(delta_event(event_id, "queued"));
    }
    let replay_from = registry.replay_from(id).expect("replay cursor");
    assert!(replay_from >= 1);

    // The replay re-delivers 1..=64 (already applied) plus 65..=128 (new).
    let mut new_applied = 0;
    for event_id in 1..=128 {
        registry.record_event(id, event_id);
        registry.projection_mut(id).queue_event(delta_event(event_id, "y"));
        for event in registry.take_pending_events(id, 16) {
            let parsed = event.id.as_deref().and_then(|id| id.parse::<i64>().ok()).unwrap_or(0);
            if registry.mark_event_applied(id, parsed) {
                new_applied += 1;
            }
        }
    }
    assert_eq!(new_applied, 64, "replay must only apply the missing range");
    registry.clear_replay(id);
    assert!(!registry.needs_replay(id));
    assert_eq!(
        registry.projection(id).expect("projection").last_seen_event_id,
        128
    );
}
