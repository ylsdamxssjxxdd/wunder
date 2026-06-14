use std::sync::Arc;
use wunder_server::goal;
use wunder_server::storage::{ChatSessionRecord, SessionGoalRecord, SqliteStorage, StorageBackend};

fn now_ts() -> f64 {
    1_700_000_000.0
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
        objective: "complete the requested work".to_string(),
        status: goal::STATUS_ACTIVE.to_string(),
        token_budget: Some(100),
        tokens_used: 0,
        time_used_seconds: 0,
        created_at: now_ts(),
        updated_at: now_ts(),
        completed_at: None,
        last_continued_at: None,
        source: goal::SOURCE_API.to_string(),
    }
}

#[test]
fn sqlite_session_goal_crud_and_usage() {
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
    assert_eq!(loaded.status, goal::STATUS_ACTIVE);

    let updated = storage
        .account_session_goal_usage("user_id", "session_id", 42, 3, now_ts() + 1.0)
        .expect("account usage")
        .expect("goal exists");
    assert_eq!(updated.tokens_used, 42);
    assert_eq!(updated.time_used_seconds, 3);

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

#[tokio::test]
async fn sqlite_goal_usage_marks_budget_limited() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("goal_budget.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init");
    storage
        .upsert_chat_session(&session("user_id", "session_id"))
        .expect("session");
    storage
        .upsert_session_goal(&goal_record("user_id", "session_id"))
        .expect("goal");

    let updated = goal::account_turn_usage(storage, "user_id", "session_id", 120, 2)
        .await
        .expect("account usage")
        .expect("goal exists");

    assert_eq!(updated.status, goal::STATUS_BUDGET_LIMITED);
    assert_eq!(updated.tokens_used, 120);
    assert_eq!(updated.time_used_seconds, 2);
}
