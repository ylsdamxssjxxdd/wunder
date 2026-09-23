use crate::config::Config;
use crate::monitor::MonitorState;
use crate::storage::{ChatSessionRecord, ChatSessionStore, SqliteStorage, StorageBackend};
use std::sync::Arc;

fn record(id: &str, parent: Option<&str>, source: Option<&str>) -> ChatSessionRecord {
    ChatSessionRecord {
        session_id: id.into(),
        user_id: "user".into(),
        title: "thread".into(),
        status: "active".into(),
        created_at: 1.0,
        updated_at: 1.0,
        last_message_at: 1.0,
        agent_id: None,
        tool_overrides: vec![],
        parent_session_id: parent.map(str::to_string),
        parent_message_id: None,
        spawn_label: None,
        spawned_by: source.map(str::to_string),
    }
}

#[test]
fn child_cancellation_survives_monitor_registration_and_keeps_history_identity() {
    let dir = tempfile::tempdir().unwrap();
    let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
        dir.path().join("test.db").to_string_lossy().into(),
    ));
    let child = record("child", Some("parent"), Some("model"));
    storage.upsert_chat_session(&child).unwrap();
    let monitor = MonitorState::new(
        storage.clone(),
        Config::default().observability,
        dir.path().to_string_lossy().into(),
    );
    monitor.register("parent", "user", "", "task", false, false);
    let guard = monitor.register_child_run("child", "parent").unwrap();
    let nested = monitor.register_child_run("nested", "child").unwrap();
    assert!(monitor.cancel("parent"));
    monitor.register("child", "user", "", "task", false, false);
    assert!(monitor.is_cancelled("child"));
    assert!(monitor.is_cancelled("nested"));
    assert!(monitor.register_child_run("late", "parent").is_err());
    assert_eq!(
        storage
            .get_chat_session("user", "child")
            .unwrap()
            .unwrap()
            .status,
        "active"
    );
    drop(nested);
    drop(guard);
    monitor.register("parent", "user", "", "next", false, false);
    let resumed = monitor.register_child_run("child", "parent").unwrap();
    monitor.register("child", "user", "", "next", false, false);
    assert!(!monitor.is_cancelled("child"));
    assert!(!resumed.token.is_cancelled());
    assert_eq!(
        storage
            .get_chat_session("user", "child")
            .unwrap()
            .unwrap()
            .session_id,
        child.session_id
    );
}

#[test]
fn child_directory_traversal_pages_and_preserves_independent_forks() {
    let dir = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(dir.path().join("test.db").to_string_lossy().into());
    for index in 0..260 {
        storage
            .upsert_chat_session(&record(
                &format!("child-{index}"),
                Some("parent"),
                Some("model"),
            ))
            .unwrap();
    }
    storage
        .upsert_chat_session(&record("nested", Some("child-0"), Some("model")))
        .unwrap();
    storage
        .upsert_chat_session(&record("fork", Some("parent"), Some("thread_control")))
        .unwrap();
    storage
        .upsert_chat_session(&record("fork-child", Some("fork"), Some("model")))
        .unwrap();
    let children = super::descendants::collect(&storage, "user", "parent").unwrap();
    assert_eq!(children.len(), 261);
    assert!(!children.contains(&"fork".to_string()));
    assert!(!children.contains(&"fork-child".to_string()));
    assert_eq!(
        super::descendants::collect(&storage, "other", "parent").unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn child_catalog_filters_before_pagination_and_preserves_explicit_queries() {
    let dir = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(dir.path().join("test.db").to_string_lossy().into());
    for item in [
        record("root", None, None),
        record("child", Some("root"), Some("model")),
        record("fork", Some("root"), Some("thread_control")),
        record("swarm", Some("root"), Some("agent_swarm")),
    ] {
        storage.upsert_chat_session(&item).unwrap();
    }
    let (page, total) = storage
        .list_work_chat_sessions("user", None, Some("active"), 0, 1)
        .unwrap();
    assert_eq!((page.len(), total), (1, 3));
    let (page, total) = storage
        .list_work_chat_sessions("user", None, Some("active"), 1, 10)
        .unwrap();
    assert_eq!(
        (
            page.into_iter()
                .map(|record| record.session_id)
                .collect::<Vec<_>>(),
            total
        ),
        (vec!["root".into(), "fork".into()], 3)
    );
    let (_, total) = storage
        .list_chat_sessions_by_status("user", None, Some("root"), Some("all"), 0, 10)
        .unwrap();
    assert_eq!(total, 3);
}
