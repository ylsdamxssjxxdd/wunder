use super::{context, recovery, tree};
use crate::storage::*;
use serde_json::json;

fn storage() -> (tempfile::TempDir, SqliteStorage) {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
    db.ensure_initialized().unwrap();
    (dir, db)
}

fn session(db: &SqliteStorage, user: &str, id: &str, parent: Option<&str>, source: Option<&str>) {
    db.upsert_chat_session(&ChatSessionRecord {
        user_id: user.into(),
        session_id: id.into(),
        title: "Fixture".into(),
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
    })
    .unwrap();
}

#[test]
fn task_tree_restores_identity_and_rejects_other_roots_users_and_cycles() {
    let (_dir, db) = storage();
    session(&db, "owner", "root", None, None);
    session(&db, "owner", "a", Some("root"), Some("model"));
    session(&db, "owner", "b", Some("root"), Some("model"));
    session(&db, "owner", "nested", Some("a"), Some("model"));
    session(&db, "owner", "fork", Some("root"), Some("thread_control"));
    session(&db, "owner", "swarm", Some("root"), Some("agent_swarm"));
    session(&db, "owner", "other", None, None);
    assert_eq!(
        tree::authorize(&db, "owner", "b", "nested", false)
            .unwrap()
            .path,
        "/root/a/nested"
    );
    assert_eq!(
        tree::resolve(&db, "owner", "b", "/root/a/nested").unwrap(),
        "nested"
    );
    assert!(tree::resolve(&db, "owner", "b", "/root/b/nested").is_err());
    assert_eq!(
        tree::ancestors(&db, "owner", "nested").unwrap(),
        vec!["root", "a"]
    );
    for target in ["root", "other", "fork", "swarm"] {
        assert!(tree::authorize(&db, "owner", "a", target, false).is_err());
    }
    assert!(tree::authorize(&db, "different-owner", "a", "b", false).is_err());
    // A new storage handle simulates cold reconstruction without a live registry.
    let reopened = SqliteStorage::new(_dir.path().join("fixture.db").to_string_lossy().into());
    assert_eq!(
        tree::identity(&reopened, "owner", "nested").unwrap().path,
        "/root/a/nested"
    );
    session(&db, "owner", "root", Some("nested"), Some("model"));
    assert!(tree::identity(&db, "owner", "nested").is_err());
}

#[test]
fn context_inherits_bounded_recent_turns_without_system_or_tool_roles() {
    let (_dir, db) = storage();
    session(&db, "owner", "root", None, None);
    for index in 1..=3 {
        let turn = db
            .accept_thread_turn("owner", "root", &json!({"content":format!("user-{index}")}))
            .unwrap();
        let turn_id = turn["turn_id"].as_str().unwrap();
        for (role, text) in [
            ("assistant", format!("reply-{index}")),
            ("system", "private-policy".into()),
            ("tool", "private-tool".into()),
        ] {
            db.commit_thread_item(
                "owner",
                &json!({"session_id":"root","turn_id":turn_id,
                "item_id":format!("{turn_id}:{role}"),"kind":format!("{role}_message"),"role":role,
                "visibility":"user","status":"completed","content":text}),
            )
            .unwrap();
        }
    }
    let options = context::ContextOptions {
        fork_turns: Some(1),
        context_summary: Some("Selected constraint".into()),
    };
    let (text, metadata) =
        context::prepare(&db, "owner", "root", "Continue fixture", &options).unwrap();
    assert!(text.contains("reply-3") && text.contains("Selected constraint"));
    assert!(
        !text.contains("reply-2")
            && !text.contains("private-policy")
            && !text.contains("private-tool")
    );
    assert_eq!(metadata["fork_turns"], 1);
    assert_eq!(
        context::prepare(&db, "owner", "root", "Task", &Default::default())
            .unwrap()
            .0,
        "Task"
    );
    assert!(context::prepare(
        &db,
        "owner",
        "root",
        "Task",
        &context::ContextOptions {
            fork_turns: Some(17),
            context_summary: None
        }
    )
    .is_err());
}

#[test]
fn subagent_context_caps_unicode_and_groups_continuations_by_user_round() {
    let (_dir, db) = storage();
    session(&db, "owner", "root", None, None);
    let root = db
        .accept_thread_turn("owner", "root", &json!({"content":"Fixture root"}))
        .unwrap();
    let root_id = root["turn_id"].as_str().unwrap();
    let continuation = db
        .accept_thread_turn(
            "owner",
            "root",
            &json!({"content":"Internal","root_turn_id":root_id,"trigger_kind":"continuation"}),
        )
        .unwrap();
    let turn = continuation["turn_id"].as_str().unwrap();
    for index in 0..6 {
        db.commit_thread_item(
            "owner",
            &json!({"session_id":"root","turn_id":turn,
            "item_id":format!("fixture-{index}"),"kind":"assistant_message","role":"assistant",
            "visibility":"user","status":"completed","content":"文".repeat(30000)}),
        )
        .unwrap();
    }
    let options = context::ContextOptions {
        fork_turns: Some(1),
        context_summary: None,
    };
    let (text, meta) = context::prepare(&db, "owner", "root", "Task", &options).unwrap();
    assert!(text.contains("文"));
    assert_eq!(meta["context_truncated"], true);
    assert!(meta["context_bytes"].as_u64().unwrap() <= 65536);
    assert!(db
        .load_subagent_context("other-owner", "root", 1)
        .unwrap()
        .is_empty());
}

#[test]
fn expired_execution_recovers_once_preserving_history_and_fresh_workers() {
    let (_dir, db) = storage();
    session(&db, "owner", "child", None, None);
    let turn = db
        .accept_thread_turn("owner", "child", &json!({"content":"Fixture task"}))
        .unwrap();
    let turn_id = turn["turn_id"].as_str().unwrap();
    db.update_thread_turn("owner", "child", turn_id, "running", "", &json!({}))
        .unwrap();
    let mut run = SessionRunRecord {
        run_id: "old-run".into(),
        session_id: "child".into(),
        parent_session_id: Some("root".into()),
        user_id: "owner".into(),
        dispatch_id: None,
        run_kind: Some("subagent".into()),
        requested_by: Some("subagent_control".into()),
        agent_id: None,
        model_name: Some("fixture-model".into()),
        status: "running".into(),
        queued_time: 1.0,
        started_time: 1.0,
        finished_time: 0.0,
        elapsed_s: 0.0,
        result: None,
        error: None,
        updated_time: 1.0,
        metadata: Some(
            json!({"subagent_progress":{"child_turn_id":turn_id,"model_request_count":3}}),
        ),
    };
    db.upsert_session_run(&run).unwrap();
    recovery::recover(&db, "owner", "child").unwrap();
    assert_eq!(
        db.get_session_run("old-run").unwrap().unwrap().status,
        "cancelled"
    );
    // A delayed publisher or old executor cannot resurrect an expired lease.
    db.upsert_session_run(&run).unwrap();
    assert_eq!(
        db.get_session_run("old-run").unwrap().unwrap().status,
        "cancelled"
    );
    assert!(!db
        .load_thread_context_items("owner", "child", 10, false)
        .unwrap()
        .is_empty());
    let before = db.thread_snapshot("owner", "child").unwrap();
    recovery::recover(&db, "owner", "child").unwrap();
    assert_eq!(db.thread_snapshot("owner", "child").unwrap(), before);
    run.run_id = "fresh-run".into();
    db.upsert_session_run(&run).unwrap();
    let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    db.touch_session_run("different-owner", "fresh-run", now)
        .unwrap();
    assert_eq!(
        db.get_session_run("fresh-run")
            .unwrap()
            .unwrap()
            .updated_time,
        1.0
    );
    db.touch_session_run("owner", "fresh-run", now).unwrap();
    recovery::recover(&db, "owner", "child").unwrap();
    assert_eq!(
        db.get_session_run("fresh-run").unwrap().unwrap().status,
        "running"
    );
    assert_eq!(
        db.get_session_run("old-run")
            .unwrap()
            .unwrap()
            .metadata
            .unwrap()["subagent_progress"]["model_request_count"],
        3
    );
}
