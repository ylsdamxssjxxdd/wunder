use super::*;
use std::sync::Barrier;
use std::time::Instant;

fn message(id: usize) -> AgentMessage {
    AgentMessage {
        id: id.to_string(),
        source: "parent".into(),
        kind: "guide".into(),
        text: format!("input {id}"),
        cancellation: None,
    }
}

#[test]
fn mailbox_scopes_deduplicates_and_rejects_conflicting_ids() {
    let registry = Arc::new(Mailboxes::default());
    let guard = registry.open("user", "child").unwrap();
    assert!(!registry.send("other", "child", message(1)).unwrap());
    assert!(registry.send("user", "child", message(1)).unwrap());
    assert!(registry.send("user", "child", message(1)).unwrap());
    let mut conflicting = message(1);
    conflicting.text = "different".into();
    assert!(registry.send("user", "child", conflicting).is_err());
    assert_eq!(
        guard
            .take(false)
            .into_iter()
            .map(|m| m.id)
            .collect::<Vec<_>>(),
        vec!["1"]
    );
    assert!(registry.send("user", "child", message(1)).unwrap());
    assert!(guard.take(true).is_empty());
    assert!(!registry.send("user", "child", message(2)).unwrap());
    drop(guard);
    assert!(registry.entries.lock().is_empty());
}

#[test]
fn mailbox_message_ids_are_scoped_to_sender() {
    let registry = Arc::new(Mailboxes::default());
    let guard = registry.open("user", "parent").unwrap();
    let mut first = message(1);
    first.source = "child_1".into();
    let mut second = first.clone();
    second.source = "child_2".into();
    for item in [first.clone(), second.clone(), first, second] {
        assert!(registry.send("user", "parent", item).unwrap());
    }
    assert_eq!(
        guard
            .take(false)
            .iter()
            .map(|item| item.source.as_str())
            .collect::<Vec<_>>(),
        vec!["child_1", "child_2"]
    );
}

#[test]
fn mailbox_bounds_and_cancelled_messages() {
    let registry = Arc::new(Mailboxes::default());
    let guard = registry.open("user", "child").unwrap();
    for id in 0..MAX_PENDING_MESSAGES {
        assert!(registry.send("user", "child", message(id)).unwrap());
    }
    assert!(registry.send("user", "child", message(100)).is_err());
    assert_eq!(guard.take(false).len(), MAX_PENDING_MESSAGES);
    let mut oversized = message(101);
    oversized.text = "x".repeat(MAX_MESSAGE_BYTES + 1);
    assert!(registry.send("user", "child", oversized).is_err());
    let token = CancellationToken::new();
    let mut cancelled = message(102);
    cancelled.cancellation = Some(token.clone());
    assert!(registry.send("user", "child", cancelled).unwrap());
    token.cancel();
    assert!(guard.take(true).iter().all(AgentMessage::cancelled));
    assert!(!registry.send("user", "child", message(103)).unwrap());
}

#[test]
fn mailbox_send_racing_final_close_never_loses_an_accepted_message() {
    let registry = Arc::new(Mailboxes::default());
    for id in 0..200 {
        let guard = registry.open("user", "child").unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let worker_registry = registry.clone();
        let worker_barrier = barrier.clone();
        let sender = std::thread::spawn(move || {
            worker_barrier.wait();
            worker_registry.send("user", "child", message(id)).unwrap()
        });
        barrier.wait();
        let mut received = guard.take(true);
        let accepted = sender.join().unwrap();
        received.extend(guard.take(true));
        assert_eq!(received.len(), usize::from(accepted));
    }
}

#[tokio::test]
async fn mailbox_run_signal_has_no_snapshot_subscription_gap() {
    let signals = super::super::signals::RunSignals::default();
    let mut subscriber = signals.subscribe("parent").unwrap();
    signals.notify("parent");
    tokio::time::timeout(
        std::time::Duration::from_millis(100),
        subscriber.receiver.changed(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(*subscriber.receiver.borrow_and_update(), 1);
}

#[test]
#[ignore = "release performance experiment"]
fn mailbox_pressure() {
    for workers in [1, 2, 4, 8] {
        let registry = Arc::new(Mailboxes::default());
        let barrier = Arc::new(Barrier::new(workers));
        let started = Instant::now();
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let registry = registry.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let session = format!("child_{worker}");
                    let guard = registry.open("user", &session).unwrap();
                    let mut latencies = Vec::with_capacity(20_000);
                    barrier.wait();
                    for id in 0..20_000 {
                        let start = Instant::now();
                        assert!(registry.send("user", &session, message(id)).unwrap());
                        let batch = guard.take(false);
                        assert_eq!(batch.len(), 1);
                        assert_eq!(batch[0].id, id.to_string());
                        latencies.push(start.elapsed().as_nanos() as u64);
                    }
                    latencies
                })
            })
            .collect();
        let mut samples: Vec<_> = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect();
        let elapsed = started.elapsed().as_secs_f64();
        samples.sort_unstable();
        println!("MAILBOX_PRESSURE {{\"workers\":{workers},\"messages\":{},\"messages_per_s\":{:.0},\"p50_us\":{:.3},\"p95_us\":{:.3},\"p99_us\":{:.3}}}",
            samples.len(), samples.len() as f64/elapsed, samples[samples.len()/2] as f64/1000.,
            samples[samples.len()*95/100] as f64/1000., samples[samples.len()*99/100] as f64/1000.);
        assert!(registry.entries.lock().is_empty());
    }
}
