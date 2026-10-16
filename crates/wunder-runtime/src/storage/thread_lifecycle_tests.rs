//! One lifecycle contract exercised against both storage engines.
//! PostgreSQL: set WUNDER_TEST_POSTGRES_DSN to an isolated database and run
//! cargo test -p wunder-runtime --lib --features postgres-storage thread_lifecycle -- --include-ignored
use super::*;
use serde_json::json;

fn lifecycle_contract(db: &dyn StorageBackend) {
    let user = "fixture-user";
    let session = "fixture-lifecycle";
    let accepted = db
        .accept_thread_turn(user, session, &json!({"content":"fixture input"}))
        .unwrap();
    let turn = accepted["turn_id"].as_str().unwrap();
    assert!(db
        .update_thread_turn(
            user,
            session,
            turn,
            "running",
            "",
            &json!({"status":"running"})
        )
        .unwrap());
    let running = db
        .thread_view_page(user, session, "chat", None, 12)
        .unwrap();
    assert_eq!(running.runtime_status, "running");
    assert_eq!(running.active_turn_id, turn);

    let queued = db
        .accept_thread_turn(
            user,
            session,
            &json!({"content":"fixture next","client_message_id":"fixture-next"}),
        )
        .unwrap();
    let next = queued["turn_id"].as_str().unwrap();
    let parked = json!({"queued_input":{"queue_id":"fixture-task","reason":"input","priority":0}});
    assert!(db
        .update_thread_turn(user, session, next, "queued", "", &parked)
        .unwrap());
    let page = db
        .thread_view_page(user, session, "chat", None, 12)
        .unwrap();
    assert_eq!(page.runtime_status, "running");
    assert_eq!(page.active_turn_id, turn);
    assert!(page
        .chat
        .iter()
        .any(|row| row.root_turn_id == next && row.queued_input.is_some()));

    let tool = json!({"session_id":session,"turn_id":turn,"item_id":"fixture-tool",
        "kind":"tool_call","status":"running","event_type":"tool_call"});
    db.append_thread_item(user, &tool).unwrap();
    let mut parallel = tool.clone();
    parallel["item_id"] = json!("fixture-parallel-tool");
    db.append_thread_item(user, &parallel).unwrap();
    assert_eq!(
        db.thread_view_page(user, session, "chat", None, 12)
            .unwrap()
            .chat[0]
            .workspace_change_seq,
        0
    );
    let mut result = parallel.clone();
    result["status"] = json!("completed");
    result["event_type"] = json!("tool_result");
    result["meta"] = json!({"workspace_changed":true,"workspace_version":2});
    db.append_thread_item(user, &result).unwrap();
    let written = db
        .thread_view_page(user, session, "chat", None, 12)
        .unwrap();
    assert!(written.chat[0].workspace_change_seq > 0);
    assert_eq!(
        written.runtime_status, "running",
        "a tool write is not turn completion"
    );
    let prior_write = written.chat[0].workspace_change_seq;
    result["item_id"] = tool["item_id"].clone();
    db.append_thread_item(user, &result).unwrap();
    assert!(
        db.thread_view_page(user, session, "chat", None, 12)
            .unwrap()
            .chat[0]
            .workspace_change_seq
            > prior_write,
        "an earlier admitted parallel tool completing last must invalidate again"
    );

    assert!(db
        .update_thread_turn(
            user,
            session,
            turn,
            "completed",
            "",
            &json!({"status":"completed"})
        )
        .unwrap());
    assert!(db
        .update_thread_turn(
            user,
            session,
            next,
            "running",
            "",
            &json!({"status":"running"})
        )
        .unwrap());
    assert!(
        !db.update_thread_turn(user, session, next, "queued", "", &parked)
            .unwrap(),
        "stale queue polls cannot restore composer ownership"
    );
    let started = db
        .thread_view_page(user, session, "chat", None, 12)
        .unwrap();
    assert!(started.chat.iter().all(|row| row.queued_input.is_none()));
    assert_eq!(started.active_turn_id, next);
    // JSON null is equivalent to an absent queue payload on both engines.
    assert!(db
        .update_thread_turn(
            user,
            session,
            next,
            "completed",
            "",
            &json!({"queued_input":null})
        )
        .unwrap());
    let finished = db
        .thread_view_page(user, session, "chat", None, 12)
        .unwrap();
    assert_eq!(finished.runtime_status, "completed");
    assert!(finished.active_turn_id.is_empty());
    assert!(finished.chat.iter().all(|row| row.status == "completed"));
    let trajectory = db
        .thread_view_page(user, session, "trajectory", None, 80)
        .unwrap();
    assert!(
        trajectory
            .trajectory
            .iter()
            .all(|item| item.status == "completed"),
        "terminal turn also settles user and tool trajectory rows"
    );
    let cursor = db.latest_thread_change_seq_by_session(session).unwrap();
    assert!(!db
        .update_thread_turn(user, session, next, "completed", "", &json!({}))
        .unwrap());
    assert_eq!(
        db.latest_thread_change_seq_by_session(session).unwrap(),
        cursor
    );
}

fn legacy_completion_contract(db: &dyn StorageBackend) {
    let user = "fixture-user";
    let mut sessions = Vec::new();
    for index in 0..3 {
        let session = format!("fixture-legacy-{index}");
        let accepted = db
            .accept_thread_turn(user, &session, &json!({"content":"fixture input"}))
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        // Actual persisted transcript shape: a later history/stats write
        // replaces the stream payload and does not contain event_type.
        db.append_thread_item(
            user,
            &json!({"session_id":session,"turn_id":turn,
            "item_id":format!("{turn}:text-1"),"kind":"assistant_message","role":"assistant",
            "status":"completed","content":"fixture answer","tool_calls":null,
            "meta":{"message_stats":{"model_request_count":1}}}),
        )
        .unwrap();
        sessions.push(session);
    }
    // Opening one thread repairs its durable state, including the display head.
    let page = db
        .thread_view_page(user, &sessions[0], "chat", None, 12)
        .unwrap();
    assert_eq!(page.runtime_status, "completed");
    assert_eq!(page.chat[0].status, "completed");
    // The catalog also heals older threads that have not been opened.
    let tails = db.thread_turn_tails(user, &sessions).unwrap();
    assert_eq!(tails.len(), 3);
    for tail in tails.values() {
        assert_eq!(tail.turn_status, "completed");
        assert_eq!(
            tail.runtime_head.as_ref().unwrap().runtime_status,
            "completed"
        );
    }
    let cursor = db
        .latest_thread_change_seq_by_session(&sessions[0])
        .unwrap();
    db.thread_turn_tails(user, &sessions).unwrap();
    assert_eq!(
        db.latest_thread_change_seq_by_session(&sessions[0])
            .unwrap(),
        cursor
    );

    let session = "fixture-live-final";
    let accepted = db
        .accept_thread_turn(user, session, &json!({"content":"fixture input"}))
        .unwrap();
    let turn = accepted["turn_id"].as_str().unwrap();
    db.update_thread_turn(user, session, turn, "running", "", &json!({}))
        .unwrap();
    assert!(matches!(
        db.try_acquire_session_lock(session, user, "fixture-agent", 60.0, 8)
            .unwrap(),
        SessionLockStatus::Acquired
    ));
    db.append_thread_item(
        user,
        &json!({"session_id":session,"turn_id":turn,
        "item_id":format!("{turn}:text-1"),"kind":"assistant_message","role":"assistant",
        "status":"completed","content":"fixture answer","meta":{"message_stats":{}}}),
    )
    .unwrap();
    assert_eq!(
        db.thread_view_page(user, session, "chat", None, 12)
            .unwrap()
            .runtime_status,
        "running"
    );
    db.release_session_lock(session).unwrap();
    assert_eq!(
        db.thread_view_page(user, session, "chat", None, 12)
            .unwrap()
            .runtime_status,
        "completed"
    );

    for (suffix, status, meta, calls) in [
        (
            "tool",
            "running",
            json!({"type":"tool_call","message_stats":{}}),
            json!([{"id":"fixture-call"}]),
        ),
        (
            "question",
            "waiting_input",
            json!({"type":"question_panel","message_stats":{}}),
            json!(null),
        ),
        (
            "approval",
            "waiting_approval",
            json!({"message_stats":{}}),
            json!([]),
        ),
        (
            "continued",
            "running",
            json!({"message_stats":{}}),
            json!([]),
        ),
    ] {
        let session = format!("fixture-active-{suffix}");
        let accepted = db
            .accept_thread_turn(user, &session, &json!({"content":"fixture input"}))
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        db.update_thread_turn(user, &session, turn, status, "", &json!({}))
            .unwrap();
        db.append_thread_item(
            user,
            &json!({"session_id":session,"turn_id":turn,
            "item_id":format!("{turn}:text-1"),"kind":"assistant_message","role":"assistant",
            "status":"completed","content":"fixture partial","meta":meta,"tool_calls":calls}),
        )
        .unwrap();
        if suffix == "continued" {
            db.append_thread_item(
                user,
                &json!({"session_id":session,"turn_id":turn,
                "item_id":format!("{turn}:text-2"),"kind":"assistant_message","status":"running"}),
            )
            .unwrap();
        }
        assert_eq!(
            db.thread_view_page(user, &session, "chat", None, 12)
                .unwrap()
                .runtime_status,
            status
        );
    }
}

#[test]
fn sqlite_thread_lifecycle_and_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into_owned());
    lifecycle_contract(&db);
    legacy_completion_contract(&db);
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires isolated WUNDER_TEST_POSTGRES_DSN"]
fn postgres_thread_lifecycle_and_recovery() {
    let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated fixture database");
    assert!(!dsn.contains("://"), "use a key/value fixture DSN");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (client, connection) = runtime
        .block_on(tokio_postgres::connect(&dsn, tokio_postgres::NoTls))
        .unwrap();
    runtime.spawn(async move { connection.await.unwrap() });
    let schema = format!("fixture_lifecycle_{}", uuid::Uuid::new_v4().simple());
    runtime
        .block_on(client.batch_execute(&format!("CREATE SCHEMA {schema}")))
        .unwrap();
    let db =
        PostgresStorage::new(format!("{dsn} options='-c search_path={schema}'"), 5, 2).unwrap();
    lifecycle_contract(&db);
    legacy_completion_contract(&db);
    drop(db);
    runtime
        .block_on(client.batch_execute(&format!("DROP SCHEMA {schema} CASCADE")))
        .unwrap();
}
