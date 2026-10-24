use wunder_server::goal;
use wunder_server::storage::{
    ChatSessionRecord, ChatSessionStore, SessionGoalRecord, SessionGoalStore, SqliteStorage,
    StorageLifecycle,
};

fn now_ts() -> f64 {
    1_700_000_000.0
}

#[test]
fn stale_goal_update_cannot_restore_cleared_goal() {
    let temp = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(temp.path().join("goals.db").to_string_lossy().to_string());
    let record = goal_record("owner", "thread");
    db.upsert_session_goal(&record).unwrap();
    db.delete_session_goal("owner", "thread").unwrap();
    assert!(!db.update_session_goal(&record, record.revision).unwrap());
    assert!(db.get_session_goal("owner", "thread").unwrap().is_none());
}

fn session(user_id: &str, session_id: &str) -> ChatSessionRecord {
    ChatSessionRecord {
        session_id: session_id.to_string(),
        user_id: user_id.to_string(),
        title: "session".to_string(),
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
        last_message_at: now_ts(),
        agent_id: None,
        workspace_id: None,
        tool_overrides: Vec::new(),
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    }
}

fn goal_record(user_id: &str, session_id: &str) -> SessionGoalRecord {
    SessionGoalRecord {
        goal_id: "goal_id".to_string(),
        session_id: session_id.to_string(),
        user_id: user_id.to_string(),
        revision: 1,
        objective: "complete the requested work".to_string(),
        phase: goal::PHASE_ACTIVE.to_string(),
        blocked_code: None,
        blocked_message: None,
        max_goal_rounds: 4,
        rounds_started: 0,
        created_at: now_ts(),
        updated_at: now_ts(),
    }
}

#[test]
fn sqlite_session_goal_crud_and_revision_guard() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("goal.db");
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().expect("init");
    storage
        .upsert_chat_session(&session("user_id", "session_id"))
        .expect("session");

    let record = goal_record("user_id", "session_id");
    storage.upsert_session_goal(&record).expect("upsert goal");
    let loaded = storage
        .get_session_goal("user_id", "session_id")
        .expect("load goal")
        .expect("goal exists");
    assert_eq!(loaded.objective, record.objective);
    assert_eq!(loaded.phase, goal::PHASE_ACTIVE);
    assert_eq!(loaded.revision, 1);
    assert_eq!(loaded.max_goal_rounds, 4);

    let mut updated = loaded.clone();
    updated.revision += 1;
    updated.phase = goal::PHASE_PAUSED.into();
    updated.rounds_started = 2;
    updated.updated_at += 1.0;
    assert!(storage
        .update_session_goal(&updated, loaded.revision)
        .unwrap());
    assert!(!storage
        .update_session_goal(&loaded, loaded.revision)
        .unwrap());
    assert_eq!(
        storage.get_session_goal("user_id", "session_id").unwrap(),
        Some(updated)
    );
    assert!(storage
        .get_session_goal("other_user", "session_id")
        .unwrap()
        .is_none());

    assert_eq!(
        storage
            .delete_session_goal("user_id", "session_id")
            .expect("delete"),
        1
    );
    assert!(storage
        .get_session_goal("user_id", "session_id")
        .expect("load after delete")
        .is_none());
}

#[test]
fn sqlite_deleting_session_removes_goal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("goal_delete.db");
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().expect("init");
    storage
        .upsert_chat_session(&session("user_id", "session_id"))
        .expect("session");
    storage
        .upsert_session_goal(&goal_record("user_id", "session_id"))
        .expect("goal");

    assert_eq!(
        storage
            .delete_chat_session("user_id", "session_id")
            .expect("delete session"),
        1
    );
    assert!(storage
        .get_session_goal("user_id", "session_id")
        .expect("load goal")
        .is_none());
}

#[test]
fn sqlite_lists_session_goals_in_batch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("goal_list.db");
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().expect("init");
    for session_id in ["session_a", "session_b", "session_c"] {
        storage
            .upsert_chat_session(&session("user_id", session_id))
            .expect("session");
    }
    let goal_a = goal_record("user_id", "session_a");
    let mut goal_c = goal_record("user_id", "session_c");
    goal_c.goal_id = "goal_id_c".to_string();
    goal_c.objective = "complete requested follow up".to_string();
    storage.upsert_session_goal(&goal_a).expect("goal a");
    storage.upsert_session_goal(&goal_c).expect("goal c");

    let mut goals = storage
        .list_session_goals(
            "user_id",
            &[
                "session_a".to_string(),
                "session_b".to_string(),
                "session_c".to_string(),
            ],
        )
        .expect("list goals");
    goals.sort_by(|left, right| left.session_id.cmp(&right.session_id));

    assert_eq!(goals, vec![goal_a, goal_c]);
}

#[test]
fn sqlite_goal_reopen_preserves_phase_rounds_and_block_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state_1.sqlite");
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().expect("init");
    storage
        .upsert_chat_session(&session("user_id", "session_id"))
        .expect("session");
    let mut record = goal_record("user_id", "session_id");
    record.revision = 3;
    record.rounds_started = 2;
    record.phase = goal::PHASE_BLOCKED.into();
    record.blocked_code = Some("fixture_block".into());
    record.blocked_message = Some("Fixture input required".into());
    storage.upsert_session_goal(&record).expect("goal");
    drop(storage);

    assert!(dir.path().join("goals_1.sqlite").is_file());
    let reopened = SqliteStorage::new(db_path.to_string_lossy().to_string());
    assert_eq!(
        reopened.get_session_goal("user_id", "session_id").unwrap(),
        Some(record)
    );
}
