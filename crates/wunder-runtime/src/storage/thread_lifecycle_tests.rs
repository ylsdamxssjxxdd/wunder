//! Local admission/freeze/begin/completion lifecycle. The PostgreSQL service
//! contract is verified against PostgreSQL in `postgres::thread_display`.
use super::*;
use serde_json::{json, Value};
use wunder_core::thread_store::*;

const USER: &str = "fixture-user";
const THREAD: &str = "fixture-lifecycle";

fn wait<T: Send>(future: impl std::future::Future<Output = T> + Send) -> T {
    canonical_thread_log::wait(future).unwrap()
}

fn freeze(store: &thread_store::LocalThreadStore) {
    let thread = ThreadId::new(THREAD).unwrap();
    let rollout = RolloutId::new("fixture-rollout").unwrap();
    wait(store.create_thread(CreateThreadRequest {
        metadata: ThreadMetadata {
            thread_id: thread.clone(),
            current_rollout_id: rollout.clone(),
            status: ThreadStatus::Active,
            created_at_ms: 1,
            updated_at_ms: 1,
            title: None,
            workspace_id: None,
            pinned: false,
        },
        session_meta: SessionMeta {
            thread_id: thread,
            rollout_id: rollout,
            created_at_ms: 1,
            source: "fixture".into(),
            initial_system_prompt: "frozen fixture instructions".into(),
            initial_memory: None,
            parent_thread_id: None,
            history_base: None,
            capabilities: Value::Null,
        },
    }))
    .unwrap();
}

fn begin(store: &thread_store::LocalThreadStore, turn: &str) {
    wait(store.begin_turn(
        USER,
        &ThreadId::new(THREAD).unwrap(),
        &TurnId::new(turn).unwrap(),
        "fixture-model",
    ))
    .unwrap();
}

fn completed_tool(turn: &str, item: &str, workspace_version: i64) -> Value {
    json!({
        "session_id":THREAD,"turn_id":turn,"item_id":item,
        "kind":"tool_call","status":"completed","event_type":"tool_result",
        "tool":"fixture_tool","args":{},"data":{"result":"fixture"},
        "meta":{"workspace_changed":true,"workspace_version":workspace_version}
    })
}

fn lifecycle_contract(db: &dyn StorageBackend, store: &thread_store::LocalThreadStore) {
    let input = json!({"content":"fixture input","client_message_id":"fixture-first"});
    let accepted = db.accept_thread_turn(USER, THREAD, &input).unwrap();
    let turn = accepted["turn_id"].as_str().unwrap();
    let replay = db.accept_thread_turn(USER, THREAD, &input).unwrap();
    assert_eq!(replay["turn_id"], turn);
    assert_eq!(replay["created"], false);
    assert!(
        !store.root().join("sessions").exists(),
        "accepting input must not freeze an empty system prompt"
    );

    freeze(store);
    begin(store, turn);
    let running = db.thread_view_page(USER, THREAD, "chat", None, 12).unwrap();
    assert_eq!(running.runtime_status, "running");
    assert_eq!(running.active_turn_id, turn);
    assert_eq!(
        store
            .admission_store()
            .unwrap()
            .pending(THREAD, turn)
            .unwrap(),
        None,
        "the committed user body must leave the queue"
    );

    let queued = db
        .accept_thread_turn(
            USER,
            THREAD,
            &json!({"content":"fixture next","client_message_id":"fixture-next"}),
        )
        .unwrap();
    let next = queued["turn_id"].as_str().unwrap();
    let parked = json!({"queued_input":{"queue_id":"fixture-task","reason":"input","priority":0}});
    assert!(db
        .update_thread_turn(USER, THREAD, next, "queued", "", &parked)
        .unwrap());
    let page = db.thread_view_page(USER, THREAD, "chat", None, 12).unwrap();
    assert_eq!(page.runtime_status, "running");
    assert_eq!(page.active_turn_id, turn);
    assert!(page
        .chat
        .iter()
        .any(|row| row.root_turn_id == next && row.queued_input.is_some()));
    assert!(db
        .thread_view_page("fixture-other", THREAD, "chat", None, 12)
        .is_err());

    let cursor = db.latest_thread_change_seq_by_session(THREAD).unwrap();
    let tool = json!({"session_id":THREAD,"turn_id":turn,"item_id":"fixture-tool",
        "kind":"tool_call","status":"running","event_type":"tool_call"});
    db.append_thread_item(USER, &tool).unwrap();
    let mut parallel = tool.clone();
    parallel["item_id"] = json!("fixture-parallel-tool");
    db.append_thread_item(USER, &parallel).unwrap();
    assert_eq!(
        db.latest_thread_change_seq_by_session(THREAD).unwrap(),
        cursor,
        "activity starts and tails do not create durable history"
    );
    assert!(db
        .get_thread_item(USER, THREAD, "fixture-tool", false)
        .unwrap()
        .is_none());

    db.append_thread_item(USER, &completed_tool(turn, "fixture-parallel-tool", 2))
        .unwrap();
    let written = db.thread_view_page(USER, THREAD, "chat", None, 12).unwrap();
    let first = written
        .chat
        .iter()
        .find(|row| row.root_turn_id == turn)
        .unwrap();
    assert!(first.workspace_change_seq > 0);
    let prior_write = first.workspace_change_seq;
    assert_eq!(
        written.runtime_status, "running",
        "tool completion is not turn completion"
    );
    db.append_thread_item(USER, &completed_tool(turn, "fixture-tool", 3))
        .unwrap();
    let written = db.thread_view_page(USER, THREAD, "chat", None, 12).unwrap();
    assert!(
        written
            .chat
            .iter()
            .find(|row| row.root_turn_id == turn)
            .unwrap()
            .workspace_change_seq
            > prior_write,
        "a parallel tool completing later must invalidate the workspace again"
    );

    db.append_thread_item(
        USER,
        &json!({"session_id":THREAD,"turn_id":turn,
        "item_id":"fixture-answer","kind":"assistant_message","status":"completed",
        "role":"assistant","content":"fixture answer"}),
    )
    .unwrap();
    assert_eq!(
        db.thread_view_page(USER, THREAD, "chat", None, 12)
            .unwrap()
            .runtime_status,
        "running",
        "a completed assistant item cannot infer terminal execution state"
    );
    let before_read = db.latest_thread_change_seq_by_session(THREAD).unwrap();
    db.thread_turn_tails(USER, &[THREAD.to_owned()]).unwrap();
    db.thread_view_page(USER, THREAD, "chat", None, 12).unwrap();
    assert_eq!(
        db.latest_thread_change_seq_by_session(THREAD).unwrap(),
        before_read,
        "history reads must never repair or write execution state"
    );

    assert!(db
        .update_thread_turn(USER, THREAD, turn, "completed", "", &json!({}))
        .unwrap());
    begin(store, next);
    assert!(
        !db.update_thread_turn(USER, THREAD, next, "queued", "", &parked)
            .unwrap(),
        "a stale queue poll cannot roll a running turn back or reclaim composer ownership"
    );
    let started = db.thread_view_page(USER, THREAD, "chat", None, 12).unwrap();
    assert!(started.chat.iter().all(|row| row.queued_input.is_none()));
    assert_eq!(started.active_turn_id, next);
    assert!(db
        .update_thread_turn(
            USER,
            THREAD,
            next,
            "completed",
            "",
            &json!({"queued_input":null})
        )
        .unwrap());
    let finished = db.thread_view_page(USER, THREAD, "chat", None, 12).unwrap();
    assert_eq!(finished.runtime_status, "completed");
    assert!(finished.active_turn_id.is_empty());
    assert!(finished.chat.iter().all(|row| row.status == "completed"));
    let trajectory = db
        .thread_view_page(USER, THREAD, "trajectory", None, 80)
        .unwrap();
    assert!(trajectory
        .trajectory
        .iter()
        .all(|item| item.status == "completed"));

    let cursor = db.latest_thread_change_seq_by_session(THREAD).unwrap();
    assert!(!db
        .update_thread_turn(USER, THREAD, next, "completed", "", &json!({}))
        .unwrap());
    assert!(
        !db.update_thread_turn(USER, THREAD, next, "queued", "", &parked)
            .unwrap(),
        "terminal admissions are immutable under stale queue writes"
    );
    assert_eq!(
        db.latest_thread_change_seq_by_session(THREAD).unwrap(),
        cursor
    );
    let meta = wait(store.session_meta(&ThreadId::new(THREAD).unwrap())).unwrap();
    assert_eq!(meta.initial_system_prompt, "frozen fixture instructions");
}

#[test]
fn sqlite_thread_lifecycle_and_recovery() {
    let root = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    lifecycle_contract(&db, db.canonical_threads().unwrap());
}

#[test]
fn sqlite_begin_turn_does_not_duplicate_the_user_message() {
    let root = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    let store = db.canonical_threads().unwrap();
    let input = json!({"content":"fixture input","client_message_id":"fixture-begin-once"});
    let accepted = db.accept_thread_turn(USER, THREAD, &input).unwrap();
    let turn = accepted["turn_id"].as_str().unwrap();
    freeze(&store);
    begin(&store, turn);
    let first = db.thread_view_page(USER, THREAD, "chat", None, 12).unwrap();
    let count = first
        .chat
        .iter()
        .filter(|row| row.root_turn_id == turn)
        .count();
    // 重放执行开始（崩溃后重试派发/重复领取）不得再次物化用户消息。
    begin(&store, turn);
    let second = db.thread_view_page(USER, THREAD, "chat", None, 12).unwrap();
    let count_after = second
        .chat
        .iter()
        .filter(|row| row.root_turn_id == turn)
        .count();
    assert!(count >= 1);
    assert_eq!(count, count_after);
}
