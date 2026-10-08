use super::*;
use serde_json::json;
use std::sync::{Arc, Barrier};
use std::time::Instant;

fn message_task(thread: &str, id: &str) -> AgentTaskRecord {
    AgentTaskRecord {
        task_id: format!("{thread}_{id}"),
        thread_id: thread.into(),
        user_id: "user".into(),
        agent_id: String::new(),
        session_id: thread.into(),
        status: "pending".into(),
        request_payload: json!({"question":"input"}),
        request_id: None,
        retry_count: 0,
        retry_at: 1.,
        created_at: 1.,
        updated_at: 1.,
        started_at: None,
        finished_at: None,
        last_error: None,
    }
}

fn verify(storage: Arc<dyn StorageBackend>, competitor: Arc<dyn StorageBackend>) {
    storage.ensure_initialized().unwrap();
    competitor.ensure_initialized().unwrap();
    let thread = format!("thread_{}", uuid::Uuid::new_v4().simple());
    let record = message_task(&thread, "same");
    let barrier = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|index| {
            let storage = if index % 2 == 0 {
                storage.clone()
            } else {
                competitor.clone()
            };
            let record = record.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                storage.insert_agent_message_task(&record, 4).unwrap();
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(
        storage
            .list_agent_tasks_by_thread(&thread, None, 20)
            .unwrap()
            .len(),
        1
    );
    let mut conflict = record.clone();
    conflict.request_payload = json!({"question":"different"});
    assert!(storage.insert_agent_message_task(&conflict, 4).is_err());
    let workers: Vec<_> = (0..8)
        .map(|index| {
            let storage = if index % 2 == 0 {
                storage.clone()
            } else {
                competitor.clone()
            };
            let record = message_task(&thread, &index.to_string());
            std::thread::spawn(move || storage.insert_agent_message_task(&record, 4).is_ok())
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .filter_map(|worker| worker.join().ok())
            .filter(|ok| *ok)
            .count(),
        3
    );
    assert_eq!(
        storage
            .list_agent_tasks_by_thread(&thread, None, 20)
            .unwrap()
            .len(),
        4
    );
    assert!(storage.claim_agent_task(&record.task_id, 2.).unwrap());
    storage.insert_agent_message_task(&record, 4).unwrap();
    assert_eq!(
        storage
            .get_agent_task(&record.task_id)
            .unwrap()
            .unwrap()
            .status,
        "running"
    );
}

#[test]
fn agent_message_sqlite_atomic_admission() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db").to_string_lossy().into_owned();
    verify(
        Arc::new(SqliteStorage::new(path.clone())),
        Arc::new(SqliteStorage::new(path)),
    );
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires isolated PostgreSQL via WUNDER_SUBAGENT_TEST_DSN"]
fn agent_message_postgres_atomic_admission() {
    let dsn = std::env::var("WUNDER_SUBAGENT_TEST_DSN").unwrap();
    verify(
        Arc::new(PostgresStorage::new(dsn.clone(), 5, 8).unwrap()),
        Arc::new(PostgresStorage::new(dsn, 5, 8).unwrap()),
    );
}

fn pressure(storage: Arc<dyn StorageBackend>, backend: &str) {
    storage.ensure_initialized().unwrap();
    for concurrency in [1, 2, 4, 8] {
        let barrier = Arc::new(Barrier::new(concurrency));
        let start = Instant::now();
        let workers: Vec<_> = (0..concurrency)
            .map(|_| {
                let storage = storage.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let thread = format!("thread_{}", uuid::Uuid::new_v4().simple());
                    barrier.wait();
                    let mut durations = Vec::new();
                    for id in 0..200 {
                        let record = message_task(&thread, &id.to_string());
                        let start = Instant::now();
                        storage.insert_agent_message_task(&record, 256).unwrap();
                        durations.push(start.elapsed().as_micros() as u64);
                    }
                    assert_eq!(
                        storage
                            .list_agent_tasks_by_thread(&thread, None, 256)
                            .unwrap()
                            .len(),
                        200
                    );
                    durations
                })
            })
            .collect();
        let mut samples: Vec<_> = workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect();
        let elapsed = start.elapsed().as_secs_f64();
        samples.sort_unstable();
        println!("ADMISSION_PRESSURE backend={backend} concurrency={concurrency} messages={} messages_per_s={:.1} p50_us={} p95_us={} p99_us={}",
            samples.len(),samples.len() as f64/elapsed,samples[samples.len()/2],samples[samples.len()*95/100],samples[samples.len()*99/100]);
    }
}

#[test]
#[ignore = "release performance experiment"]
fn agent_message_sqlite_pressure() {
    let dir = tempfile::tempdir().unwrap();
    pressure(
        Arc::new(SqliteStorage::new(
            dir.path().join("test.db").to_string_lossy().into(),
        )),
        "sqlite",
    );
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires isolated PostgreSQL via WUNDER_SUBAGENT_TEST_DSN"]
fn agent_message_postgres_pressure() {
    pressure(
        Arc::new(
            PostgresStorage::new(std::env::var("WUNDER_SUBAGENT_TEST_DSN").unwrap(), 5, 8).unwrap(),
        ),
        "postgres",
    );
}
