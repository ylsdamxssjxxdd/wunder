use crate::storage::*;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

fn record(user: &str, session: &str) -> ChatSessionRecord {
    ChatSessionRecord {
        session_id: session.into(),
        user_id: user.into(),
        title: "Sample thread".into(),
        status: "active".into(),
        created_at: 1.0,
        updated_at: 3.0,
        last_message_at: 3.0,
        agent_id: None,
        tool_overrides: vec![],
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
        workspace_id: None,
    }
}

fn seed(root: &std::path::Path, user: &str, session: &str) -> Result<()> {
    let store = super::super::thread_store::LocalThreadStore::open(root)?;
    super::super::canonical_thread_log::wait(store.generate_stress_thread(
        user,
        &StressThreadSpec {
            session_id: session.into(),
            title: "Sample thread".into(),
            workspace_id: None,
            user_rounds: 1,
            model_rounds_per_turn: 1,
        },
        |_, _| {},
    ))??;
    Ok(())
}

#[test]
fn diagnostic_cleanup_preserves_thread_directory_and_canonical_history() {
    let root = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(root.path().join("state_1.sqlite").to_string_lossy().into());
    storage
        .upsert_chat_session(&record("sample-user", "sample-thread"))
        .unwrap();
    seed(root.path(), "sample-user", "sample-thread").unwrap();
    storage
        .append_tool_log(
            "sample-user",
            &json!({"session_id":"sample-thread","tool":"sample-tool","ok":true}),
        )
        .unwrap();
    storage.upsert_monitor_record(&json!({"session_id":"sample-thread","user_id":"sample-user","status":"finished","updated_time":3.0})).unwrap();
    storage
        .append_stream_event(
            "sample-thread",
            "sample-user",
            1,
            &json!({"event":"progress","data":{"user_round":1}}),
        )
        .unwrap();
    let deleted = storage.delete_logs_by_time_range(0.0, f64::MAX).unwrap();
    assert_eq!(deleted.get("runtime_tool_metrics"), Some(&1));
    assert_eq!(deleted.get("monitor_sessions"), Some(&1));
    assert_eq!(deleted.get("stream_events"), Some(&1));
    assert!(storage
        .get_chat_session("sample-user", "sample-thread")
        .unwrap()
        .is_some());
    assert!(!storage
        .list_thread_visible_messages("sample-user", "sample-thread", None, 20)
        .unwrap()
        .is_empty());
}

#[test]
fn catalog_only_delete_preserves_history_until_explicit_user_history_delete() {
    let root = tempfile::tempdir().unwrap();
    let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
        root.path().join("state_1.sqlite").to_string_lossy().into(),
    ));
    for (user, session) in [
        ("sample-user", "sample-thread"),
        ("sample-other", "sample-retained"),
    ] {
        storage.upsert_chat_session(&record(user, session)).unwrap();
        seed(root.path(), user, session).unwrap();
    }
    assert_eq!(
        storage.delete_chat_sessions_by_user("sample-user").unwrap(),
        1
    );
    assert!(!storage
        .list_thread_visible_messages("sample-user", "sample-thread", None, 20)
        .unwrap()
        .is_empty());
    assert_eq!(
        storage.delete_thread_logs_by_user("sample-user").unwrap(),
        1
    );
    assert!(storage
        .list_thread_visible_messages("sample-user", "sample-thread", None, 20)
        .unwrap()
        .is_empty());
    assert!(!storage
        .list_thread_visible_messages("sample-other", "sample-retained", None, 20)
        .unwrap()
        .is_empty());
    assert!(storage
        .get_chat_session("sample-other", "sample-retained")
        .unwrap()
        .is_some());
    assert_eq!(
        storage.delete_thread_logs_by_user("sample-user").unwrap(),
        0
    );
}
