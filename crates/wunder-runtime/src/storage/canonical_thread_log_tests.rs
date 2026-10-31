//! Read contracts start with the production canonical writer, never SQL fixtures.
use super::{
    thread_store::LocalThreadStore, SqliteStorage, StorageBackend, StorageLifecycle, ThreadLogStore,
};
use serde_json::{json, Value};
use wunder_core::thread_store::*;

const USER: &str = "user-sample";
const THREAD: &str = "thread-sample";

#[test]
fn process_reasoning_identity_uses_its_own_body_when_answer_is_from_an_older_round() {
    let root = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let admitted = db
        .accept_thread_turn(USER, THREAD, &json!({"content":"Fixture input"}))
        .unwrap();
    let turn = admitted["turn_id"].as_str().unwrap();
    super::test_support::freeze_thread(root.path(), USER, THREAD);
    super::test_support::begin_turn(root.path(), USER, THREAD, turn);
    for (round, content, reasoning, calls) in [
        (1, "Fixture answer", "Fixture completed thought", json!([])),
        (
            2,
            "",
            "Fixture next thought",
            json!([{"id":"fixture-call"}]),
        ),
    ] {
        db.append_thread_item(
            USER,
            &json!({"session_id":THREAD,"turn_id":turn,
            "item_id":format!("{turn}:text-{round}"),"kind":"assistant_message","model_round":round,
            "content":content,"reasoning":reasoning,"tool_calls":calls,"status":"completed"}),
        )
        .unwrap();
    }
    let page = db.thread_view_page(USER, THREAD, "chat", None, 12).unwrap();
    let row = &page.chat[0];
    assert_eq!(row.assistant.item_id, format!("{turn}:text-1"));
    assert_eq!(row.reasoning_item_id, format!("{turn}:text-2"));
    assert_eq!(row.assistant.reasoning, "Fixture next thought");
    assert_eq!(row.process_count, Some(2));
}

fn read_body(db: &dyn StorageBackend, body_id: &str) -> String {
    let mut cursor = 0;
    let mut content = String::new();
    loop {
        let page = db
            .thread_body_page(USER, THREAD, body_id, 1, cursor)
            .unwrap();
        content.extend(page.chunks.into_iter());
        let Some(next) = page.next_cursor else { break };
        assert!(next > cursor, "body pagination must advance");
        cursor = next;
    }
    content
}

#[test]
fn initial_chat_cursor_replays_a_commit_racing_the_page_read() {
    use super::MetaStore;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct CommitDuringRead<'a> {
        db: &'a SqliteStorage,
        turn: String,
        fired: AtomicBool,
    }
    impl MetaStore for CommitDuringRead<'_> {
        fn get_meta(&self, key: &str) -> anyhow::Result<Option<String>> {
            // Feedback is read after the page's transaction captured its
            // durable cursor and selected the reply. Commit a new reply at
            // that exact boundary, without timing-dependent sleeps.
            if key.starts_with("thread_feedback:") && !self.fired.swap(true, Ordering::SeqCst) {
                self.db.append_thread_item(
                    USER,
                    &json!({"session_id":THREAD,"turn_id":self.turn,
                    "item_id":format!("{}:text-2",self.turn),"kind":"assistant_message",
                    "role":"assistant","content":"fixture later answer","status":"completed"}),
                )?;
            }
            self.db.get_meta(key)
        }
        fn set_meta(&self, key: &str, value: &str) -> anyhow::Result<()> {
            self.db.set_meta(key, value)
        }
        fn compare_and_swap_meta(
            &self,
            key: &str,
            expected: Option<&str>,
            value: Option<&str>,
        ) -> anyhow::Result<bool> {
            self.db.compare_and_swap_meta(key, expected, value)
        }
        fn list_meta_prefix(&self, prefix: &str) -> anyhow::Result<Vec<(String, String)>> {
            self.db.list_meta_prefix(prefix)
        }
        fn delete_meta_prefix(&self, prefix: &str) -> anyhow::Result<usize> {
            self.db.delete_meta_prefix(prefix)
        }
    }

    let root = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let admitted = db
        .accept_thread_turn(USER, THREAD, &json!({"content":"fixture input"}))
        .unwrap();
    let turn = admitted["turn_id"].as_str().unwrap();
    super::test_support::freeze_thread(root.path(), USER, THREAD);
    super::test_support::begin_turn(root.path(), USER, THREAD, turn);
    db.append_thread_item(
        USER,
        &json!({"session_id":THREAD,"turn_id":turn,
        "item_id":format!("{turn}:text-1"),"kind":"assistant_message","role":"assistant",
        "content":"fixture prior answer","status":"completed"}),
    )
    .unwrap();
    let before = db.latest_thread_change_seq_by_session(THREAD).unwrap();
    let metadata = CommitDuringRead {
        db: &db,
        turn: turn.into(),
        fired: AtomicBool::new(false),
    };
    let page = super::canonical_thread_log::CanonicalThreadLog::new(
        db.canonical_threads().unwrap(),
        &metadata,
    )
    .thread_view_page(USER, THREAD, "chat", None, 12)
    .unwrap();
    assert!(metadata.fired.load(Ordering::SeqCst));
    assert_eq!(page.view_seq, before);
    assert_eq!(page.chat[0].assistant.content, "fixture prior answer");
    assert!(db
        .list_thread_changes(USER, THREAD, page.view_seq, 20)
        .unwrap()
        .iter()
        .any(|event| event["payload"]["payload"]["content"] == "fixture later answer"));
    assert_eq!(
        db.thread_view_page(USER, THREAD, "chat", None, 12)
            .unwrap()
            .chat[0]
            .assistant
            .content,
        "fixture later answer"
    );
}

#[tokio::test]
async fn canonical_writer_reopens_through_backend_with_complete_chat_and_trajectory() {
    let root = tempfile::tempdir().unwrap();
    let db_path = root.path().join("state_1.sqlite");
    let store = LocalThreadStore::open(root.path()).unwrap();
    let thread = ThreadId::new(THREAD).unwrap();
    let rollout = RolloutId::new("rollout-sample").unwrap();
    let admission = store
        .admission_store()
        .unwrap()
        .accept(
            USER,
            THREAD,
            &json!({"content":"sample input", "client_message_id":"message-sample"}),
        )
        .unwrap();
    let turn = TurnId::new(admission["turn_id"].as_str().unwrap()).unwrap();
    store
        .create_thread(CreateThreadRequest {
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
                thread_id: thread.clone(),
                rollout_id: rollout,
                created_at_ms: 1,
                source: "sample".into(),
                initial_system_prompt: "frozen instructions".into(),
                initial_memory: None,
                parent_thread_id: None,
                history_base: None,
                capabilities: Value::Null,
            },
        })
        .await
        .unwrap();
    store
        .begin_turn(USER, &thread, &turn, "sample-model")
        .await
        .unwrap();
    let answer = format!(
        "{}fixture-search-tail",
        "sample answer 中🙂\n".repeat(9_000)
    );
    let tool_output = "sample tool output 中🙂\n".repeat(8_000);
    let assistant_id = ItemId::new("assistant-sample").unwrap();
    let tool_id = ItemId::new("tool-sample").unwrap();
    store
        .append_model_message(
            &thread,
            &turn,
            json!({"role":"user", "content":"sample input"}),
        )
        .await
        .unwrap();
    store.append_model_message(&thread, &turn,
        json!({"role":"assistant", "content":"invoking tool", "tool_calls":[{"id":"call-sample","type":"function","function":{"name":"sample_tool","arguments":"{}"}}]})).await.unwrap();
    store
        .append_model_message(
            &thread,
            &turn,
            json!({"role":"tool", "tool_call_id":"call-sample", "content":tool_output}),
        )
        .await
        .unwrap();
    store
        .append_model_message(
            &thread,
            &turn,
            json!({"role":"assistant", "content":answer, "reasoning_content":"sample reasoning"}),
        )
        .await
        .unwrap();
    store.append_canonical(&thread, vec![
        CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
            item_id: tool_id.clone(), turn_id: turn.clone(), root_turn_id: Some(turn.clone()),
            created_at_ms: 2, kind: CanonicalItemKind::ToolResult,
            payload: json!({"session_id":THREAD,"turn_id":turn,"item_id":tool_id,
                "kind":"tool_result","status":"completed","visibility":"user",
                "payload":{"tool":"sample_tool","args":{},"content":tool_output,"data":{"result":"sample"}}}),
            display_output: None, display_output_truncated: false,
        })),
        CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
            item_id: assistant_id.clone(), turn_id: turn.clone(), root_turn_id: Some(turn.clone()),
            created_at_ms: 3, kind: CanonicalItemKind::AssistantMessage,
            payload: json!({"session_id":THREAD,"turn_id":turn,"item_id":assistant_id,
                "kind":"assistant_message","status":"completed","visibility":"user",
                "payload":{"role":"assistant","content":answer,"reasoning_content":"sample reasoning"}}),
            display_output: None, display_output_truncated: false,
        })),
        CanonicalRecord::Event(CanonicalEvent::TurnCompleted { turn_id: turn.clone(), completed_at_ms: 4 }),
    ]).await.unwrap();
    store
        .admission_store()
        .unwrap()
        .update(THREAD, turn.as_str(), "completed", true)
        .unwrap();
    drop(store);

    let db = SqliteStorage::new(db_path.to_string_lossy().into_owned());
    db.ensure_initialized().unwrap();
    let backend: &dyn StorageBackend = &db;
    let chat = backend
        .thread_view_page(USER, THREAD, "chat", None, 10)
        .unwrap();
    assert_eq!(chat.chat.len(), 1);
    assert_eq!(chat.runtime_status, "completed");
    assert_eq!(chat.status_turn_id, turn.as_str());
    assert!(chat.active_turn_id.is_empty());
    let changes = backend.thread_view_changes(USER, THREAD, 0).unwrap();
    assert_eq!(changes.status_turn_id, turn.as_str());
    let tails = backend.thread_turn_tails(USER, &[THREAD.into()]).unwrap();
    let head = tails[THREAD].runtime_head.as_ref().unwrap();
    assert_eq!(head.status_turn_id, turn.as_str());
    assert_eq!(head.runtime_status, "completed");
    assert_eq!(head.view_seq, chat.view_seq);
    assert!(head.active_turn_id.is_empty());
    assert_eq!(read_body(backend, "assistant-sample|content"), answer);
    let item = backend
        .get_thread_item(USER, THREAD, assistant_id.as_str(), false)
        .unwrap()
        .unwrap();
    assert_eq!(item["payload"]["content"], answer);
    let search = backend
        .thread_view_search(USER, THREAD, "fixture-search-tail")
        .unwrap();
    assert!(search
        .hits
        .iter()
        .any(|hit| hit.item_id == assistant_id.as_str()));
    let trajectory = backend
        .thread_view_trajectory_page(USER, THREAD, None, Some(turn.as_str()), 20)
        .unwrap();
    assert_eq!(trajectory.trajectory.len(), 3);
    assert!(trajectory
        .trajectory
        .iter()
        .any(|item| item.item_id == "tool-sample"));
    let tool_detail: Value =
        serde_json::from_str(&read_body(backend, "tool-sample|detail")).unwrap();
    assert_eq!(tool_detail["content"], tool_output);
    let context = backend
        .load_thread_execution_context(USER, THREAD, "", 0)
        .unwrap();
    assert_eq!(context.first().unwrap()["content"], "sample input");
    assert_eq!(context.last().unwrap()["content"], answer);
    assert!(context.iter().any(|message| message["role"] == "tool"
        && message["tool_call_id"] == "call-sample"
        && message["content"] == tool_output));
    let changes = backend.list_thread_changes(USER, THREAD, 0, 30).unwrap();
    assert!(changes
        .iter()
        .any(|change| change["change_type"] == "turn_status"));
    assert_eq!(
        backend.get_thread_log_counts(USER, THREAD, false).unwrap(),
        (1, 3)
    );
}

#[tokio::test]
async fn application_chat_append_requires_freeze_and_preserves_model_context() {
    let root = tempfile::tempdir().unwrap();
    let db_path = root.path().join("state_1.sqlite");
    let db = SqliteStorage::new(db_path.to_string_lossy().into_owned());
    db.ensure_initialized().unwrap();
    let backend: &dyn StorageBackend = &db;
    let input = json!({"session_id":THREAD,"role":"user","content":"sample input"});
    assert!(backend.append_chat(USER, &input).is_err());
    assert!(!root.path().join("sessions").exists());

    let store = LocalThreadStore::open(root.path()).unwrap();
    let thread = ThreadId::new(THREAD).unwrap();
    let rollout = RolloutId::new("rollout-sample").unwrap();
    let admission = backend.accept_thread_turn(USER, THREAD, &input).unwrap();
    let turn = TurnId::new(admission["turn_id"].as_str().unwrap()).unwrap();
    store
        .create_thread(CreateThreadRequest {
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
                thread_id: thread.clone(),
                rollout_id: rollout,
                created_at_ms: 1,
                source: "sample".into(),
                initial_system_prompt: "frozen instructions".into(),
                initial_memory: None,
                parent_thread_id: None,
                history_base: None,
                capabilities: Value::Null,
            },
        })
        .await
        .unwrap();
    store
        .begin_turn(USER, &thread, &turn, "sample-model")
        .await
        .unwrap();
    backend
        .append_chat(
            USER,
            &json!({"session_id":THREAD,"turn_id":turn,"item_id":"input-model-sample",
        "role":"user","content":"display input","model_content":"sample input"}),
        )
        .unwrap();
    backend
        .append_chat(
            USER,
            &json!({"session_id":THREAD,"turn_id":turn,"item_id":"assistant-model-sample",
        "role":"assistant","content":"display answer","model_content":"model answer"}),
        )
        .unwrap();
    drop(db);
    let reopened = SqliteStorage::new(db_path.to_string_lossy().into_owned());
    let backend: &dyn StorageBackend = &reopened;
    let context = backend
        .load_thread_execution_context(USER, THREAD, "", 0)
        .unwrap();
    assert_eq!(
        context
            .iter()
            .map(|message| message["content"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["sample input", "model answer"]
    );
    let visible = backend
        .get_thread_item(USER, THREAD, "assistant-model-sample", false)
        .unwrap()
        .unwrap();
    assert_eq!(visible["payload"]["content"], "display answer");
    assert!(visible["payload"].get("model_content").is_none());
    assert_eq!(
        store
            .session_meta(&thread)
            .await
            .unwrap()
            .initial_system_prompt,
        "frozen instructions"
    );
}

#[tokio::test]
async fn completed_compaction_summary_stays_in_history_but_not_model_context() {
    let root = tempfile::tempdir().unwrap();
    let db_path = root.path().join("state_1.sqlite");
    let db = SqliteStorage::new(db_path.to_string_lossy().into_owned());
    db.ensure_initialized().unwrap();
    let backend: &dyn StorageBackend = &db;
    let store = LocalThreadStore::open(root.path()).unwrap();
    let thread = ThreadId::new(THREAD).unwrap();
    let rollout = RolloutId::new("rollout-summary").unwrap();
    let admission = backend
        .accept_thread_turn(USER, THREAD, &json!({"content":"sample input"}))
        .unwrap();
    let turn = TurnId::new(admission["turn_id"].as_str().unwrap()).unwrap();
    store
        .create_thread(CreateThreadRequest {
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
                thread_id: thread.clone(),
                rollout_id: rollout,
                created_at_ms: 1,
                source: "sample".into(),
                initial_system_prompt: "frozen instructions".into(),
                initial_memory: None,
                parent_thread_id: None,
                history_base: None,
                capabilities: Value::Null,
            },
        })
        .await
        .unwrap();
    store
        .begin_turn(USER, &thread, &turn, "sample-model")
        .await
        .unwrap();

    backend
        .append_chat(
            USER,
            &json!({
                "session_id": THREAD,
                "turn_id": turn,
                "item_id": "summary-item",
                "role": "system",
                "content": "Compacted sample history",
                "status": "completed",
                "meta": {
                    "type": "compaction_summary",
                    "compaction_id": "compact-sample",
                    "replacement_history": [
                        {"role": "user", "content": "retained input"},
                        {"role": "assistant", "content": "retained answer"}
                    ]
                }
            }),
        )
        .unwrap();

    // Workflow events have readable text but are not chat messages. Their
    // configuration/tool kind must not manufacture an assistant role.
    store
        .append_canonical(
            &thread,
            vec![
                CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
                    item_id: ItemId::new("workflow-item").unwrap(),
                    turn_id: turn.clone(),
                    root_turn_id: Some(turn.clone()),
                    created_at_ms: 2,
                    kind: CanonicalItemKind::Configuration,
                    payload: json!({"kind":"plan", "status":"completed", "content":"sample plan"}),
                    display_output: None,
                    display_output_truncated: false,
                })),
                CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
                    item_id: ItemId::new("workflow-tool-item").unwrap(),
                    turn_id: turn.clone(),
                    root_turn_id: Some(turn),
                    created_at_ms: 3,
                    kind: CanonicalItemKind::ToolResult,
                    payload: json!({"kind":"tool", "status":"completed", "content":"sample tool result"}),
                    display_output: None,
                    display_output_truncated: false,
                })),
            ],
        )
        .await
        .unwrap();
    drop(store);
    drop(db);
    let reopened = SqliteStorage::new(db_path.to_string_lossy().into_owned());
    reopened.ensure_initialized().unwrap();
    let backend: &dyn StorageBackend = &reopened;
    assert!(backend
        .load_thread_context_items("other-sample", THREAD, 20, true)
        .is_err());
    assert!(backend
        .load_thread_context_items("other-sample", THREAD, 20, false)
        .is_err());
    let history = backend
        .load_thread_context_items(USER, THREAD, 20, true)
        .unwrap();
    assert!(!history.iter().any(|item| matches!(
        item["item_id"].as_str(),
        Some("workflow-item" | "workflow-tool-item")
    )));
    let summary = history
        .iter()
        .find(|item| item["item_id"] == "summary-item")
        .expect("completed compaction summary is a durable history item");
    assert_eq!(summary["meta"]["compaction_id"], "compact-sample");
    assert_eq!(
        summary["meta"]["replacement_history"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let context = backend
        .load_thread_execution_context(USER, THREAD, "", 20)
        .unwrap();
    assert_eq!(
        context,
        vec![
            json!({"role": "user", "content": "retained input"}),
            json!({"role": "assistant", "content": "retained answer"}),
        ]
    );
}

#[tokio::test]
async fn fork_facade_reads_shared_prefix_counts_and_body_after_source_delete() {
    let root = tempfile::tempdir().unwrap();
    let store = LocalThreadStore::open(root.path()).unwrap();
    let spec = super::stress_model::StressThreadSpec {
        session_id: "thread-facade-source".into(),
        title: "sample".into(),
        workspace_id: None,
        user_rounds: 2,
        model_rounds_per_turn: 1,
        seed: None,
        scenario: crate::storage::StressThreadScenario::Complex,
    };
    store
        .generate_stress_thread(USER, &spec, |_, _| {})
        .await
        .unwrap();
    let source = ThreadId::new(spec.session_id.clone()).unwrap();
    let first = store
        .list_turns(TurnListQuery {
            thread_id: source.clone(),
            cursor: None,
            limit: 10,
        })
        .await
        .unwrap()
        .entries
        .into_iter()
        .min_by_key(|turn| turn.rollout_ordinal)
        .unwrap()
        .turn_id;
    let branch = ThreadId::new("thread-facade-branch").unwrap();
    store
        .fork_through_turn(&source, &branch, &first)
        .await
        .unwrap();
    let expected = store
        .list_items(ItemListQuery {
            thread_id: branch.clone(),
            turn_id: None,
            cursor: None,
            limit: 200,
        })
        .await
        .unwrap()
        .entries;
    let assistant = expected
        .iter()
        .find(|entry| entry.item.kind == CanonicalItemKind::AssistantMessage)
        .unwrap()
        .item
        .item_id
        .clone();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let backend: &dyn StorageBackend = &db;
    let chat = backend
        .thread_view_page(USER, branch.as_str(), "chat", None, 10)
        .unwrap();
    assert_eq!(chat.chat.len(), 1);
    assert_eq!(
        backend
            .get_thread_log_counts(USER, branch.as_str(), false)
            .unwrap(),
        (1, expected.len() as i64)
    );
    assert!(backend
        .get_thread_item(USER, branch.as_str(), assistant.as_str(), false)
        .unwrap()
        .is_some());
    let inherited = backend
        .thread_body_page(USER, branch.as_str(), &format!("{assistant}|content"), 4, 0)
        .unwrap();
    assert!(!inherited.chunks.is_empty());
    // Source continuation goes through the same production admission, writer,
    // model-message and semantic completion path as application execution.
    let accepted = store
        .admission_store()
        .unwrap()
        .accept(
            USER,
            source.as_str(),
            &json!({"content":"later sample input"}),
        )
        .unwrap();
    let later = TurnId::new(accepted["turn_id"].as_str().unwrap()).unwrap();
    store
        .begin_turn(USER, &source, &later, "sample-model")
        .await
        .unwrap();
    store
        .append_model_message(
            &source,
            &later,
            json!({"role":"assistant","content":"later sample answer"}),
        )
        .await
        .unwrap();
    store.append_canonical(&source,vec![
        CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
            item_id:ItemId::new("item-facade-later").unwrap(),turn_id:later.clone(),root_turn_id:Some(later.clone()),created_at_ms:10,
            kind:CanonicalItemKind::AssistantMessage,payload:json!({"session_id":source,"turn_id":later,"role":"assistant","content":"later sample answer","status":"completed","visibility":"user"}),display_output:None,display_output_truncated:false,
        })),
        CanonicalRecord::Event(CanonicalEvent::TurnCompleted {turn_id:later,completed_at_ms:10}),
    ]).await.unwrap();
    assert_eq!(
        backend
            .thread_view_page(USER, branch.as_str(), "chat", None, 10)
            .unwrap()
            .chat
            .len(),
        1
    );
    assert!(backend
        .get_thread_item(USER, branch.as_str(), "item-facade-later", false)
        .unwrap()
        .is_none());
    store.delete_thread(&source).await.unwrap();
    assert_eq!(
        backend
            .get_thread_log_counts(USER, branch.as_str(), false)
            .unwrap(),
        (1, expected.len() as i64)
    );
    assert!(backend
        .get_thread_item(USER, branch.as_str(), assistant.as_str(), false)
        .unwrap()
        .is_some());
    assert_eq!(
        backend
            .thread_view_trajectory_page(USER, branch.as_str(), None, Some(first.as_str()), 200)
            .unwrap()
            .trajectory
            .len(),
        expected.len()
    );
}

#[tokio::test]
async fn cancelled_admission_does_not_freeze_empty_instructions() {
    let root = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let accepted = db
        .accept_thread_turn(USER, THREAD, &json!({"content":"queued input"}))
        .unwrap();
    let turn = accepted["turn_id"].as_str().unwrap();
    let page = db.thread_view_page(USER, THREAD, "chat", None, 10).unwrap();
    assert_eq!(page.chat[0].user.content, "queued input");
    assert_eq!(page.view_seq, 0);
    db.update_thread_turn(USER, THREAD, turn, "cancelled", "", &json!({}))
        .unwrap();
    let store = LocalThreadStore::open(root.path()).unwrap();
    assert!(matches!(
        store.get_thread(&ThreadId::new(THREAD).unwrap()).await,
        Err(ThreadStoreError::NotFound { .. })
    ));
    assert!(!root.path().join("sessions").exists());
    assert_eq!(
        db.thread_view_page(USER, THREAD, "chat", None, 10)
            .unwrap()
            .chat[0]
            .status,
        "cancelled"
    );
}

#[tokio::test]
async fn completed_inquiry_control_stays_pending_with_plan_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let db_path = root.path().join("state_1.sqlite");
    let db = SqliteStorage::new(db_path.to_string_lossy().into_owned());
    db.ensure_initialized().unwrap();
    let admitted = db
        .accept_thread_turn(USER, THREAD, &json!({"content":"Fixture input"}))
        .unwrap();
    let turn = admitted["turn_id"].as_str().unwrap();
    super::test_support::freeze_thread(root.path(), USER, THREAD);
    super::test_support::begin_turn(root.path(), USER, THREAD, turn);
    let plan = crate::services::thread_log::event_item(
        THREAD,
        "plan_update",
        &json!({
            "turn_id":turn,"model_round":1,"plan":[{"step":"Fixture step","status":"in_progress"}]
        }),
    )
    .unwrap();
    let inquiry = crate::services::thread_log::event_item(THREAD, "question_panel", &json!({
        "turn_id":turn,"model_round":2,"keep_open":true,
        "questions":[{"question":"Fixture choice?","options":[{"label":"Option A","recommended":true}]}]
    })).unwrap();
    db.append_thread_item(USER, &plan).unwrap();
    db.append_thread_item(USER, &inquiry).unwrap();
    db.update_thread_turn(
        USER,
        THREAD,
        turn,
        "waiting_input",
        "question_panel",
        &json!({}),
    )
    .unwrap();
    // Reopen through the production chat view, not a standalone panel decoder.
    drop(db);
    let reopened = SqliteStorage::new(db_path.to_string_lossy().into_owned());
    reopened.ensure_initialized().unwrap();
    let page = reopened
        .thread_view_page(USER, THREAD, "chat", None, 10)
        .unwrap();
    let row = &page.chat[0];
    assert_eq!(row.status, "waiting_input");
    assert_eq!(
        row.assistant.question_panel.as_ref().unwrap().status,
        "pending"
    );
    assert_eq!(row.plan.as_ref().unwrap().steps[0].step, "Fixture step");
    assert!(row.assistant.question_panel.as_ref().unwrap().questions[0].options[0].recommended);
}

#[tokio::test]
async fn plan_panel_reopen_shows_the_latest_publish_not_the_first_step() {
    let root = tempfile::tempdir().unwrap();
    let db_path = root.path().join("state_1.sqlite");
    let db = SqliteStorage::new(db_path.to_string_lossy().into_owned());
    db.ensure_initialized().unwrap();
    let admitted = db
        .accept_thread_turn(USER, THREAD, &json!({"content":"Fixture input"}))
        .unwrap();
    let turn = admitted["turn_id"].as_str().unwrap().to_string();
    super::test_support::freeze_thread(root.path(), USER, THREAD);
    super::test_support::begin_turn(root.path(), USER, THREAD, &turn);
    // 计划面板每次发布整份清单：同一轮里的三次发布必须各自成条，重新打开线程
    // 后聊天行显示的是最后一次发布，而不是第 1 步。
    for (round, call, steps) in [
        (
            1,
            "fixture-plan-call-a",
            json!([{"step":"Step one","status":"in_progress"},
                   {"step":"Step two","status":"pending"}]),
        ),
        (
            2,
            "fixture-plan-call-b",
            json!([{"step":"Step one","status":"completed"},
                   {"step":"Step two","status":"in_progress"}]),
        ),
        (
            3,
            "fixture-plan-call-c",
            json!([{"step":"Step one","status":"completed"},
                   {"step":"Step two","status":"completed"}]),
        ),
    ] {
        let item = crate::services::thread_log::event_item(
            THREAD,
            "plan_update",
            &json!({
                "turn_id":turn,"model_round":round,"tool_call_id":call,
                "explanation":"Fixture progress","plan":steps
            }),
        )
        .expect("plan snapshot item");
        db.append_thread_item(USER, &item).unwrap();
    }
    drop(db);
    let reopened = SqliteStorage::new(db_path.to_string_lossy().into_owned());
    reopened.ensure_initialized().unwrap();
    let page = reopened
        .thread_view_page(USER, THREAD, "chat", None, 10)
        .unwrap();
    let plan = page.chat[0].plan.as_ref().expect("plan survives reopen");
    assert_eq!(plan.explanation, "Fixture progress");
    assert_eq!(plan.steps.len(), 2);
    assert!(
        plan.steps.iter().all(|step| step.status == "completed"),
        "the panel must show the last publish: {plan:?}"
    );
}

#[tokio::test]
async fn inquiry_continuation_replaces_waiting_head_for_chat_projection() {
    let root = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let admitted = db
        .accept_thread_turn(USER, THREAD, &json!({"content":"Fixture question"}))
        .unwrap();
    let parent = admitted["turn_id"].as_str().unwrap().to_string();
    super::test_support::freeze_thread(root.path(), USER, THREAD);
    super::test_support::begin_turn(root.path(), USER, THREAD, &parent);
    let inquiry = crate::services::thread_log::event_item(
        THREAD,
        "question_panel",
        &json!({
            "turn_id": parent,
            "questions": [{"question":"Fixture choice?","options":[{"label":"Option A"}]}]
        }),
    )
    .unwrap();
    db.append_thread_item(USER, &inquiry).unwrap();
    db.update_thread_turn(
        USER,
        THREAD,
        &parent,
        "waiting_input",
        "question_panel",
        &json!({}),
    )
    .unwrap();

    let child = db
        .accept_thread_turn(
            USER,
            THREAD,
            &json!({"content":"Fixture answer", "root_user_round": 1}),
        )
        .unwrap();
    let child_id = child["turn_id"].as_str().unwrap().to_string();
    super::test_support::begin_turn(root.path(), USER, THREAD, &child_id);
    let answer = crate::services::thread_log::event_item(
        THREAD,
        "llm_output",
        &json!({
            "turn_id": child_id,
            "model_round": 1,
            "content":"Fixture completed answer"
        }),
    )
    .unwrap();
    db.append_thread_item(USER, &answer).unwrap();
    db.update_thread_turn(USER, THREAD, &child_id, "completed", "", &json!({}))
        .unwrap();

    let page = db.thread_view_page(USER, THREAD, "chat", None, 10).unwrap();
    let row = &page.chat[0];
    assert_eq!(row.root_turn_id, parent);
    assert_eq!(row.execution_turn_id, child_id);
    assert_eq!(row.status, "completed");
    assert_eq!(
        row.assistant.question_panel.as_ref().unwrap().status,
        "answered"
    );
    assert_eq!(page.runtime_status, "completed");
    assert_eq!(page.status_turn_id, child_id);
    let tails = db.thread_turn_tails(USER, &[THREAD.into()]).unwrap();
    assert_eq!(
        tails[THREAD].runtime_head.as_ref().unwrap().status_turn_id,
        child_id
    );
}

#[test]
fn execution_head_keeps_older_running_turn_separate_from_newer_queued_input() {
    let root = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let active = db
        .accept_thread_turn(USER, THREAD, &json!({"content":"fixture input"}))
        .unwrap();
    let active_id = active["turn_id"].as_str().unwrap();
    super::test_support::freeze_thread(root.path(), USER, THREAD);
    super::test_support::begin_turn(root.path(), USER, THREAD, active_id);
    let queued = db
        .accept_thread_turn(USER, THREAD, &json!({"content":"fixture next input"}))
        .unwrap();
    let queued_id = queued["turn_id"].as_str().unwrap();
    db.update_thread_turn(USER, THREAD, queued_id, "queued", "", &json!({}))
        .unwrap();
    let page = db.thread_view_page(USER, THREAD, "chat", None, 10).unwrap();
    let tails = db.thread_turn_tails(USER, &[THREAD.into()]).unwrap();
    let head = tails[THREAD].runtime_head.as_ref().unwrap();
    assert_eq!(head.runtime_status, "running");
    assert_eq!(head.status_turn_id, active_id);
    assert_eq!(head.active_turn_id, active_id);
    assert_eq!(page.status_turn_id, head.status_turn_id);
    assert_eq!(page.runtime_status, head.runtime_status);
    assert_eq!(tails[THREAD].turn_status, "queued");
}

#[tokio::test]
async fn inquiry_answer_starts_new_user_round_and_keeps_original_question() {
    let root = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let admitted = db
        .accept_thread_turn(USER, THREAD, &json!({"content":"Fixture question"}))
        .unwrap();
    let parent = admitted["turn_id"].as_str().unwrap().to_string();
    super::test_support::freeze_thread(root.path(), USER, THREAD);
    super::test_support::begin_turn(root.path(), USER, THREAD, &parent);
    let inquiry = crate::services::thread_log::event_item(
        THREAD,
        "question_panel",
        &json!({
            "turn_id": parent,
            "questions": [{"question":"Fixture choice?","options":[{"label":"Option A"}]}]
        }),
    )
    .unwrap();
    db.append_thread_item(USER, &inquiry).unwrap();
    db.update_thread_turn(
        USER,
        THREAD,
        &parent,
        "waiting_input",
        "question_panel",
        &json!({}),
    )
    .unwrap();

    let child = db
        .accept_thread_turn(
            USER,
            THREAD,
            &json!({"content":"Fixture answer", "inquiry_turn_id":parent}),
        )
        .unwrap();
    let child_id = child["turn_id"].as_str().unwrap().to_string();
    assert_ne!(child["root_turn_id"].as_str(), Some(parent.as_str()));
    assert_eq!(child["user_turn_index"].as_i64(), Some(2));
    db.update_thread_turn(
        USER,
        THREAD,
        &parent,
        "completed",
        "",
        &json!({"stop_reason":"inquiry_answered","answered_by_turn_id":child_id}),
    )
    .unwrap();
    super::test_support::begin_turn(root.path(), USER, THREAD, &child_id);
    db.update_thread_turn(USER, THREAD, &child_id, "completed", "", &json!({}))
        .unwrap();

    let page = db.thread_view_page(USER, THREAD, "chat", None, 10).unwrap();
    assert_eq!(page.chat.len(), 2);
    assert_eq!(page.chat[0].user.content, "Fixture question");
    assert_eq!(page.chat[1].user.content, "Fixture answer");
    assert_eq!(page.chat[0].status, "completed");
    assert_eq!(
        page.chat[0]
            .assistant
            .question_panel
            .as_ref()
            .unwrap()
            .status,
        "answered"
    );
    assert_eq!(page.chat[1].root_turn_id, child_id);
}

#[tokio::test]
async fn private_completed_items_do_not_block_public_replay_pagination() {
    let root = tempfile::tempdir().unwrap();
    let store = LocalThreadStore::open(root.path()).unwrap();
    let admitted = store
        .admission_store()
        .unwrap()
        .accept(USER, THREAD, &json!({"content":"sample"}))
        .unwrap();
    let thread = ThreadId::new(THREAD).unwrap();
    let turn = TurnId::new(admitted["turn_id"].as_str().unwrap()).unwrap();
    let rollout = RolloutId::new("rollout-private-sample").unwrap();
    store
        .create_thread(CreateThreadRequest {
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
                thread_id: thread.clone(),
                rollout_id: rollout,
                created_at_ms: 1,
                source: "sample".into(),
                initial_system_prompt: "frozen".into(),
                initial_memory: None,
                parent_thread_id: None,
                history_base: None,
                capabilities: Value::Null,
            },
        })
        .await
        .unwrap();
    store
        .begin_turn(USER, &thread, &turn, "sample")
        .await
        .unwrap();
    for index in 0..8 {
        store
            .append_canonical(
                &thread,
                vec![CanonicalRecord::Event(CanonicalEvent::ItemCompleted(
                    CanonicalItem {
                        item_id: ItemId::new(format!("item-private-{index}")).unwrap(),
                        turn_id: turn.clone(),
                        root_turn_id: Some(turn.clone()),
                        created_at_ms: 2,
                        kind: CanonicalItemKind::AssistantMessage,
                        payload: json!({"content":"private sample","visibility":"model_internal"}),
                        display_output: None,
                        display_output_truncated: false,
                    },
                ))],
            )
            .await
            .unwrap();
    }
    store.append_canonical(&thread,vec![CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
        item_id:ItemId::new("public-sample").unwrap(),turn_id:turn.clone(),root_turn_id:Some(turn.clone()),created_at_ms:3,kind:CanonicalItemKind::AssistantMessage,
        payload:json!({"content":"visible sample","payload":{"result":"nested semantic object"}}),display_output:None,display_output_truncated:false,
    })),CanonicalRecord::Event(CanonicalEvent::TurnCompleted{turn_id:turn,completed_at_ms:4})]).await.unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let mut after = 0;
    let mut kinds = Vec::new();
    loop {
        let changes = db.list_thread_changes(USER, THREAD, after, 1).unwrap();
        if changes.is_empty() {
            break;
        }
        assert!(changes[0]["change_seq"].as_i64().unwrap() > after);
        after = changes[0]["change_seq"].as_i64().unwrap();
        assert!(!changes[0].to_string().contains("private sample"));
        kinds.push(changes[0]["change_type"].as_str().unwrap().to_owned());
    }
    assert_eq!(
        kinds,
        vec!["turn_upsert", "item_upsert", "item_upsert", "turn_status"]
    );
    let item = db
        .get_thread_item(USER, THREAD, "public-sample", false)
        .unwrap()
        .unwrap();
    assert_eq!(item["payload"]["content"], "visible sample");
    assert_eq!(
        item["payload"]["payload"]["result"],
        "nested semantic object"
    );
    assert!(db
        .get_thread_item(USER, THREAD, "item-private-0", false)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn thread_first_user_questions_returns_the_first_visible_user_message() {
    let root = tempfile::tempdir().unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let backend: &dyn StorageBackend = &db;
    let store = LocalThreadStore::open(root.path()).unwrap();
    let thread = ThreadId::new(THREAD).unwrap();
    let first = backend
        .accept_thread_turn(USER, THREAD, &json!({"content":"Fixture first question"}))
        .unwrap();
    let first_turn = TurnId::new(first["turn_id"].as_str().unwrap()).unwrap();
    store
        .create_thread(CreateThreadRequest {
            metadata: ThreadMetadata {
                thread_id: thread.clone(),
                current_rollout_id: RolloutId::new("rollout-question").unwrap(),
                status: ThreadStatus::Active,
                created_at_ms: 1,
                updated_at_ms: 1,
                title: None,
                workspace_id: None,
                pinned: false,
            },
            session_meta: SessionMeta {
                thread_id: thread.clone(),
                rollout_id: RolloutId::new("rollout-question").unwrap(),
                created_at_ms: 1,
                source: "sample".into(),
                initial_system_prompt: "frozen instructions".into(),
                initial_memory: None,
                parent_thread_id: None,
                history_base: None,
                capabilities: Value::Null,
            },
        })
        .await
        .unwrap();
    store
        .begin_turn(USER, &thread, &first_turn, "sample-model")
        .await
        .unwrap();
    backend
        .append_chat(
            USER,
            &json!({"session_id":THREAD,"turn_id":first_turn,"item_id":"first-user",
                "role":"user","content":"Fixture first question"}),
        )
        .unwrap();
    let second = backend
        .accept_thread_turn(USER, THREAD, &json!({"content":"Fixture second question"}))
        .unwrap();
    let second_turn = TurnId::new(second["turn_id"].as_str().unwrap()).unwrap();
    store
        .begin_turn(USER, &thread, &second_turn, "sample-model")
        .await
        .unwrap();
    backend
        .append_chat(
            USER,
            &json!({"session_id":THREAD,"turn_id":second_turn,"item_id":"second-user",
                "role":"user","content":"Fixture second question"}),
        )
        .unwrap();

    let questions = backend
        .thread_first_user_questions(&[THREAD.to_string(), "missing-thread".to_string()])
        .unwrap();
    assert_eq!(
        questions.get(THREAD).map(String::as_str),
        Some("Fixture first question")
    );
    assert!(!questions.contains_key("missing-thread"));
    assert!(backend.thread_first_user_questions(&[]).unwrap().is_empty());
}

/// A thread longer than the bounded active window must still open its oldest
/// chat cards and process timeline. The projection keeps only the newest
/// `ACTIVE_WINDOW_ITEMS` rows; every read below that floor has to revalidate
/// against the canonical rollout instead of reporting "not found".
#[tokio::test]
async fn trimmed_active_window_replays_earliest_chat_and_process_from_rollout() {
    use super::thread_history::ACTIVE_WINDOW_ITEMS;
    use rusqlite::Connection;
    use std::collections::BTreeMap;

    let root = tempfile::tempdir().unwrap();
    let store = LocalThreadStore::open(root.path()).unwrap();
    // 60 user turns with 40 model rounds each keeps the total comfortably above
    // the window while staying modest enough for a unit test.
    let user_rounds = 60;
    let model_rounds = 40;
    let spec = super::stress_model::StressThreadSpec {
        session_id: THREAD.into(),
        title: "fixture".into(),
        workspace_id: None,
        user_rounds,
        model_rounds_per_turn: model_rounds,
        seed: Some(7),
        scenario: crate::storage::StressThreadScenario::Typical,
    };
    store
        .generate_stress_thread(USER, &spec, |_, _| {})
        .await
        .unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();

    // Precondition: the projection really trimmed everything outside its
    // window, so any successful early read below proves the rollout fallback.
    let history = Connection::open(root.path().join("thread_history_1.sqlite")).unwrap();
    let (window_rows, floor) = history
        .query_row(
            "SELECT COUNT(*),MIN(rollout_ordinal) FROM thread_items",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    assert_eq!(window_rows, ACTIVE_WINDOW_ITEMS);
    let anchors: i64 = history
        .query_row(
            "SELECT COUNT(*) FROM thread_item_anchors",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(anchors > 0, "sparse anchors must cover the trimmed prefix");

    let turns = db.list_thread_turns(USER, THREAD, None, 201).unwrap();
    assert_eq!(turns.len(), user_rounds as usize);
    let first = turns.last().expect("oldest turn");
    let first_turn = first["turn_id"].as_str().expect("turn id").to_owned();

    let user = db
        .get_thread_item(USER, THREAD, &format!("{first_turn}:user"), false)
        .unwrap()
        .expect("trimmed user item must replay");
    assert!(
        user["item_index"].as_i64().unwrap() < floor,
        "fixture must keep the oldest turn below the active window"
    );
    assert!(!user["payload"]["content"]
        .as_str()
        .unwrap_or_default()
        .is_empty());
    let answer = db
        .get_thread_item(
            USER,
            THREAD,
            &format!("{first_turn}:text-{model_rounds}"),
            false,
        )
        .unwrap();
    assert!(answer.is_some(), "trimmed answer item must replay");

    // A turn in the middle of the trimmed prefix historically sat between two
    // sparse anchors far beyond one scan budget; its own start record must
    // resolve it without walking the whole rollout.
    let middle = turns
        .iter()
        .find(|turn| turn["user_turn_index"].as_i64() == Some(i64::from(user_rounds) / 2))
        .expect("middle turn");
    let middle_turn = middle["turn_id"].as_str().expect("middle turn id");
    let middle_answer = db
        .get_thread_item(
            USER,
            THREAD,
            &format!("{middle_turn}:text-{model_rounds}"),
            false,
        )
        .unwrap()
        .expect("middle trimmed answer item must replay");
    assert!(middle_answer["item_index"].as_i64().unwrap() < floor);
    let mut cursor = Some(i64::MAX);
    let mut process = BTreeMap::new();
    let mut pages = 0;
    loop {
        let page = db
            .get_thread_turn_page(USER, THREAD, &first_turn, cursor, None, 10, false)
            .unwrap()
            .expect("owned turn exists");
        let items = page["items"].as_array().expect("items array");
        assert!(
            !items.is_empty(),
            "process page below the active window must not be empty"
        );
        for item in items {
            let index = item["item_index"].as_i64().unwrap();
            assert!(index < floor, "trimmed process must come from the rollout");
            process.insert(item["item_id"].as_str().unwrap().to_owned(), index);
        }
        pages += 1;
        if !page["has_older"].as_bool().unwrap() {
            break;
        }
        cursor = page["next_before"].as_i64();
        assert!(pages <= 20, "bounded pagination must terminate");
    }
    assert!(process.len() > 10);
    assert!(process.contains_key(&format!("{first_turn}:user")));
    assert!(process.contains_key(&format!("{first_turn}:text-{model_rounds}")));

    // Oldest-first paging starts at the owned turn's first item and reports a
    // newer neighbour even though every row lives below the window.
    let head = db
        .get_thread_turn_page(USER, THREAD, &first_turn, None, None, 10, false)
        .unwrap()
        .expect("owned turn exists");
    let items = head["items"].as_array().unwrap();
    assert_eq!(items.len(), 10);
    assert_eq!(
        items[0]["item_id"].as_str(),
        Some(format!("{first_turn}:user").as_str())
    );
    assert!(!head["has_older"].as_bool().unwrap());
    assert!(head["has_newer"].as_bool().unwrap());

    // Chat pages read newest to oldest; every round must keep its input and
    // its answer even after its item rows left the active window.
    let mut rows: BTreeMap<i64, (String, String, String)> = BTreeMap::new();
    let mut before = None;
    let mut guards = 0;
    loop {
        let page = db
            .thread_view_page(USER, THREAD, "chat", before, 12)
            .unwrap();
        for row in &page.chat {
            rows.insert(
                row.user_round,
                (
                    row.user.content.clone(),
                    row.assistant.content.clone(),
                    row.assistant.item_id.clone(),
                ),
            );
        }
        guards += 1;
        assert!(guards <= 10, "chat pagination must terminate");
        match (page.has_more, page.next_before) {
            (true, Some(next)) => before = Some(next),
            _ => break,
        }
    }
    assert_eq!(rows.len(), user_rounds as usize);
    assert_eq!(rows.keys().next(), Some(&1), "oldest chat round missing");
    for (round, (user, assistant, _)) in &rows {
        assert!(!user.is_empty(), "round {round} lost its user message");
        assert!(!assistant.is_empty(), "round {round} lost its answer");
    }

    // Anchors are only an optimisation, never a correctness condition: after
    // deleting every sparse anchor, the same reads must still resolve through
    // each turn's own start record inside the scan budget.
    history
        .execute("DELETE FROM thread_item_anchors", [])
        .unwrap();
    let hints: i64 = history
        .query_row(
            "SELECT COUNT(*) FROM thread_turns
             WHERE start_rollout_byte_offset IS NOT NULL AND start_ordinal IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hints, user_rounds as i64, "every turn keeps its start hint");
    let mut before = None;
    let mut guards = 0;
    let mut replayed = 0;
    loop {
        let page = db
            .thread_view_page(USER, THREAD, "chat", before, 12)
            .unwrap();
        for row in &page.chat {
            assert!(
                !row.user.content.is_empty(),
                "anchor-free round {} lost its user message",
                row.user_round
            );
            assert!(
                !row.assistant.content.is_empty(),
                "anchor-free round {} lost its answer",
                row.user_round
            );
            replayed += 1;
        }
        guards += 1;
        assert!(guards <= 10, "anchor-free chat pagination must terminate");
        match (page.has_more, page.next_before) {
            (true, Some(next)) => before = Some(next),
            _ => break,
        }
    }
    assert_eq!(replayed, user_rounds as usize);
    let anchor_free = db
        .get_thread_turn_page(USER, THREAD, middle_turn, None, None, 10, false)
        .unwrap()
        .expect("owned turn exists");
    assert!(
        !anchor_free["items"].as_array().unwrap().is_empty(),
        "anchor-free process page below the window must not be empty"
    );
}

/// A fork keeps the inherited prefix in the parent rollout. Once the inherited
/// turns leave the active window, every read has to bind the parent's byte
/// offsets to the parent physical segment while the page cursor walks toward
/// the child's own segment.
#[tokio::test]
async fn forked_thread_replays_inherited_rounds_beyond_the_active_window() {
    use super::thread_history::ACTIVE_WINDOW_ITEMS;
    use rusqlite::Connection;
    use std::collections::BTreeSet;

    const BRANCH: &str = "thread-sample-branch";

    let root = tempfile::tempdir().unwrap();
    let store = LocalThreadStore::open(root.path()).unwrap();
    let user_rounds = 60i64;
    let model_rounds = 40i64;
    let spec = super::stress_model::StressThreadSpec {
        session_id: THREAD.into(),
        title: "fixture".into(),
        workspace_id: None,
        user_rounds,
        model_rounds_per_turn: model_rounds,
        seed: Some(7),
        scenario: crate::storage::StressThreadScenario::Typical,
    };
    store
        .generate_stress_thread(USER, &spec, |_, _| {})
        .await
        .unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let history_path = root.path().join("thread_history_1.sqlite");
    let history = Connection::open(&history_path).unwrap();
    let window_rows: i64 = history
        .query_row("SELECT COUNT(*) FROM thread_items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(window_rows, ACTIVE_WINDOW_ITEMS);
    let floor: i64 = history
        .query_row(
            "SELECT MIN(rollout_ordinal) FROM thread_items",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let parent = ThreadId::new(THREAD).unwrap();
    let fork_round = user_rounds / 2;
    let turns = db.list_thread_turns(USER, THREAD, None, 201).unwrap();
    let fork_turn = turns
        .iter()
        .find(|turn| turn["user_turn_index"].as_i64() == Some(fork_round))
        .expect("fork turn")
        .clone();
    let fork_turn_id = fold_turn_id(&fork_turn);
    let branch = ThreadId::new(BRANCH).unwrap();
    store
        .fork_through_turn(&parent, &branch, &TurnId::new(fork_turn_id).unwrap())
        .await
        .unwrap();

    // The branch continues through the same production admission and writer
    // path, so its own records live in a second physical rollout.
    store
        .admission_store()
        .unwrap()
        .register_owner(USER, BRANCH)
        .unwrap();
    let admitted = db
        .accept_thread_turn(USER, BRANCH, &json!({"content":"sample branch input"}))
        .unwrap();
    let own_turn = admitted["turn_id"].as_str().unwrap().to_owned();
    super::test_support::begin_turn(root.path(), USER, BRANCH, &own_turn);
    db.append_thread_item(
        USER,
        &json!({"session_id":BRANCH,"turn_id":own_turn,
        "item_id":format!("{own_turn}:text-1"),"kind":"assistant_message","role":"assistant",
        "content":"sample branch answer","status":"completed"}),
    )
    .unwrap();
    // Warm the branch projection, then remove the optional anchors: only each
    // turn's own projected start record may locate the inherited prefix.
    db.thread_view_page(USER, BRANCH, "chat", None, 12).unwrap();
    history
        .execute("DELETE FROM thread_item_anchors", [])
        .unwrap();

    let mut rounds = BTreeSet::new();
    let mut before = None;
    let mut guards = 0;
    loop {
        let page = db
            .thread_view_page(USER, BRANCH, "chat", before, 12)
            .unwrap();
        for row in &page.chat {
            assert!(
                !row.user.content.is_empty(),
                "forked round {} lost its user message",
                row.user_round
            );
            assert!(
                !row.assistant.content.is_empty(),
                "forked round {} lost its answer",
                row.user_round
            );
            rounds.insert(row.user_round);
        }
        guards += 1;
        assert!(guards <= 10, "forked chat pagination must terminate");
        match (page.has_more, page.next_before) {
            (true, Some(next)) => before = Some(next),
            _ => break,
        }
    }
    assert_eq!(rounds.len(), (fork_round + 1) as usize);
    assert_eq!(rounds.iter().next(), Some(&1), "inherited prefix is missing");

    // The oldest inherited turn must still replay below the parent's window
    // floor, and its process page must contain the input plus the last answer.
    let first = turns
        .iter()
        .find(|turn| turn["user_turn_index"].as_i64() == Some(1))
        .expect("oldest inherited turn")
        .clone();
    let first_turn = fold_turn_id(&first);
    let user = db
        .get_thread_item(USER, BRANCH, &format!("{first_turn}:user"), false)
        .unwrap()
        .expect("inherited user item must replay");
    assert!(
        user["item_index"].as_i64().unwrap() < floor,
        "fixture must keep the inherited turn below the parent window"
    );
    let mut cursor = Some(i64::MAX);
    let mut process = Vec::new();
    let mut pages = 0;
    loop {
        let page = db
            .get_thread_turn_page(USER, BRANCH, &first_turn, cursor, None, 10, false)
            .unwrap()
            .expect("inherited turn exists");
        let items = page["items"].as_array().expect("items array");
        assert!(
            !items.is_empty(),
            "inherited process page below the window must not be empty"
        );
        process.extend(items.iter().map(|item| item["item_id"].as_str().unwrap().to_owned()));
        pages += 1;
        if !page["has_older"].as_bool().unwrap() {
            break;
        }
        cursor = page["next_before"].as_i64();
        assert!(pages <= 20, "bounded pagination must terminate");
    }
    assert!(process.contains(&format!("{first_turn}:user")));
    assert!(process.contains(&format!("{first_turn}:text-{model_rounds}")));
}

/// Cold rollouts may be stored compressed. A window-external read must decode
/// into a seekable logical view before applying the same byte-offset contract.
#[tokio::test]
async fn compressed_rollout_replays_turns_beyond_the_active_window() {
    use super::rollout_compression::{compress_rollout, CompressionOptions};
    use super::thread_history::ACTIVE_WINDOW_ITEMS;
    use rusqlite::Connection;

    let root = tempfile::tempdir().unwrap();
    let store = LocalThreadStore::open(root.path()).unwrap();
    let user_rounds = 60i64;
    let model_rounds = 40i64;
    let spec = super::stress_model::StressThreadSpec {
        session_id: THREAD.into(),
        title: "fixture".into(),
        workspace_id: None,
        user_rounds,
        model_rounds_per_turn: model_rounds,
        seed: Some(7),
        scenario: crate::storage::StressThreadScenario::Typical,
    };
    store
        .generate_stress_thread(USER, &spec, |_, _| {})
        .await
        .unwrap();
    let db = SqliteStorage::new(
        root.path()
            .join("state_1.sqlite")
            .to_string_lossy()
            .into_owned(),
    );
    db.ensure_initialized().unwrap();
    let history = Connection::open(root.path().join("thread_history_1.sqlite")).unwrap();
    let window_rows: i64 = history
        .query_row("SELECT COUNT(*) FROM thread_items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(window_rows, ACTIVE_WINDOW_ITEMS);
    history
        .execute("DELETE FROM thread_item_anchors", [])
        .unwrap();
    let state = Connection::open(root.path().join("state_1.sqlite")).unwrap();
    let relative: String = state
        .query_row("SELECT rollout_path FROM canonical_rollouts LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    drop(state);
    let rollout = root.path().join(relative);
    let compressed = compress_rollout(
        &rollout,
        &CompressionOptions {
            enabled: true,
            level: 3,
            min_age_days: 0,
        },
    )
    .unwrap()
    .expect("cold rollout compresses");
    assert!(!rollout.exists(), "compression replaces the plain source");
    assert!(compressed.exists());

    let turns = db.list_thread_turns(USER, THREAD, None, 201).unwrap();
    let first = turns.last().expect("oldest turn").clone();
    let first_turn = fold_turn_id(&first);
    let user = db
        .get_thread_item(USER, THREAD, &format!("{first_turn}:user"), false)
        .unwrap()
        .expect("compressed user item must replay");
    assert!(!user["payload"]["content"]
        .as_str()
        .unwrap_or_default()
        .is_empty());
    let mut before = None;
    let mut guards = 0;
    let mut replayed = 0;
    loop {
        let page = db.thread_view_page(USER, THREAD, "chat", before, 12).unwrap();
        for row in &page.chat {
            assert!(
                !row.user.content.is_empty(),
                "compressed round {} lost its user message",
                row.user_round
            );
            assert!(
                !row.assistant.content.is_empty(),
                "compressed round {} lost its answer",
                row.user_round
            );
            replayed += 1;
        }
        guards += 1;
        assert!(guards <= 10, "compressed chat pagination must terminate");
        match (page.has_more, page.next_before) {
            (true, Some(next)) => before = Some(next),
            _ => break,
        }
    }
    assert_eq!(replayed, user_rounds as usize);
    let page = db
        .get_thread_turn_page(USER, THREAD, &first_turn, Some(i64::MAX), None, 10, false)
        .unwrap()
        .expect("owned turn exists");
    assert!(
        !page["items"].as_array().unwrap().is_empty(),
        "compressed process page below the window must not be empty"
    );
}

fn fold_turn_id(turn: &Value) -> String {
    turn["turn_id"]
        .as_str()
        .or_else(|| turn["id"].as_str())
        .expect("turn id")
        .to_owned()
}
