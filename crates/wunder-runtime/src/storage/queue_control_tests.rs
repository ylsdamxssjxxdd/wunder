use super::*;
use serde_json::json;

fn task(id: &str, at: f64) -> AgentTaskRecord {
    AgentTaskRecord {
        task_id: id.into(),
        thread_id: format!("thread-{id}"),
        user_id: "user-a".into(),
        agent_id: "agent-a".into(),
        session_id: id.into(),
        status: "pending".into(),
        request_payload: json!({"question":"input"}),
        request_id: None,
        retry_count: 0,
        retry_at: at,
        created_at: at,
        updated_at: at,
        started_at: None,
        finished_at: None,
        last_error: None,
    }
}

fn verify_queue_control(storage: &dyn StorageBackend) {
    storage.insert_agent_task(&task("queue-a", 1.0)).unwrap();
    storage.insert_agent_task(&task("queue-b", 2.0)).unwrap();
    assert!(storage.promote_agent_task("queue-b", 3.0).unwrap());
    storage
        .update_agent_task_queue_payload("queue-b", &json!({"queue_ahead":0}))
        .unwrap();
    let pending = storage.list_pending_agent_tasks(2).unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|task| task.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["queue-b", "queue-a"]
    );
    assert_eq!(
        pending[0].request_payload,
        json!({"question":"input", "queue_priority":1, "queue_ahead":0})
    );
    assert_eq!(
        storage
            .count_pending_agent_tasks_ahead(2.0, 2.0, "queue-b")
            .unwrap(),
        0
    );
    assert_eq!(
        storage
            .count_pending_agent_tasks_ahead(1.0, 1.0, "queue-a")
            .unwrap(),
        1
    );
    assert!(storage.claim_agent_task("queue-b", 4.0).unwrap());
    assert!(!storage.claim_agent_task("queue-b", 4.0).unwrap());
    storage
        .update_agent_task_status(UpdateAgentTaskStatusParams {
            task_id: "queue-a",
            status: "cancelled",
            retry_count: 0,
            retry_at: 4.0,
            started_at: None,
            finished_at: Some(4.0),
            last_error: Some("cancelled"),
            updated_at: 4.0,
        })
        .unwrap();
    assert!(!storage.claim_agent_task("queue-a", 5.0).unwrap());
    assert!(!storage
        .update_agent_task_queue_payload("queue-a", &json!({"queue_ahead":1}))
        .unwrap());
    assert!(!storage.promote_agent_task("queue-a", 5.0).unwrap());
    storage
        .update_agent_task_status(UpdateAgentTaskStatusParams {
            task_id: "queue-a",
            status: "success",
            retry_count: 0,
            retry_at: 5.0,
            started_at: None,
            finished_at: Some(5.0),
            last_error: None,
            updated_at: 5.0,
        })
        .unwrap();
    assert_eq!(
        storage.get_agent_task("queue-a").unwrap().unwrap().status,
        "cancelled"
    );

    assert_eq!(
        storage
            .try_acquire_session_lock("lock-a", "user-a", "agent-a", 60.0, 1)
            .unwrap(),
        SessionLockStatus::Acquired
    );
    assert!(storage
        .set_session_lock_suspended("lock-a", true, 1)
        .unwrap());
    assert_eq!(
        storage
            .try_acquire_session_lock("lock-a", "user-a", "agent-a", 60.0, 1)
            .unwrap(),
        SessionLockStatus::UserBusy
    );
    assert_eq!(
        storage
            .try_acquire_session_lock("lock-b", "user-a", "agent-a", 60.0, 1)
            .unwrap(),
        SessionLockStatus::Acquired
    );
    assert!(!storage
        .set_session_lock_suspended("lock-a", false, 1)
        .unwrap());
    storage.release_session_lock("lock-b").unwrap();
    assert!(storage
        .set_session_lock_suspended("lock-a", false, 1)
        .unwrap());
    storage.release_session_lock("lock-a").unwrap();
}

#[test]
fn sqlite_queue_control_preserves_order_cancellation_and_suspended_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(dir.path().join("queue.db").to_string_lossy().into());
    storage.ensure_initialized().unwrap();
    verify_queue_control(&storage);
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated PostgreSQL database via WUNDER_QUEUE_TEST_DSN"]
fn postgres_queue_control_preserves_order_cancellation_and_suspended_ownership() {
    let storage =
        PostgresStorage::new(std::env::var("WUNDER_QUEUE_TEST_DSN").unwrap(), 5, 8).unwrap();
    storage.ensure_initialized().unwrap();
    verify_queue_control(&storage);
}
