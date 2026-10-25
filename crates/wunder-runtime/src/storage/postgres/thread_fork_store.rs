//! Atomic service forks. PostgreSQL copies the immutable prefix internally;
//! neither local history files nor a client-sized history buffer are involved.

use super::{PostgresAdmissionStore, PostgresStorage};
use crate::storage::rollout::ROLLOUT_MAX_RECORD_BYTES;
use anyhow::{anyhow, ensure, Result};
use chrono::Utc;
use wunder_core::thread_store::{CanonicalRecord, RolloutId, RolloutRecord, SessionMeta, ThreadId};

/// Rebind application envelope metadata only. The wrapped envelope is recognized
/// by its item identity, so arbitrary message content and tool JSON stay intact.
fn rebind_envelope(document: &str, path: &str) -> String {
    let direct = format!(
        "CASE WHEN {document}#>>'{{{path},session_id}}'=$1 THEN \
         jsonb_set({document},'{{{path},session_id}}',to_jsonb($2::text),false) \
         ELSE {document} END"
    );
    format!(
        "CASE WHEN ({direct})#>'{{{path},item_id}}' IS NOT NULL \
         AND ({direct})#>>'{{{path},payload,session_id}}'=$1 THEN \
         jsonb_set(({direct}),'{{{path},payload,session_id}}',to_jsonb($2::text),false) \
         ELSE ({direct}) END"
    )
}

pub(super) fn fork(
    storage: &PostgresStorage,
    user: &str,
    source: &str,
    target: &str,
    through_round: i64,
) -> Result<()> {
    let source_id = ThreadId::new(source)?;
    let target_id = ThreadId::new(target)?;
    ensure!(
        source_id != target_id,
        "fork target must differ from source"
    );
    ensure!(through_round > 0, "source user round must be positive");
    storage
        .service_threads_ref()?
        .admission_store()
        .check_owner(user, source)?;

    // Repair only the accessed source before acquiring the mutation locks.
    // Normal appends maintain this projection in their canonical transaction.
    super::thread_display::ensure_ready(storage, source)?;
    let mut conn = storage.conn()?;
    let mut tx = conn.transaction()?;
    let (first, second) = if source < target {
        (source, target)
    } else {
        (target, source)
    };
    PostgresAdmissionStore::lock_thread(&mut tx, first)?;
    PostgresAdmissionStore::lock_thread(&mut tx, second)?;
    ensure!(
        PostgresAdmissionStore::owner_in_tx(&mut tx, user, source)?.is_some(),
        "source thread owner is missing"
    );
    ensure!(
        PostgresAdmissionStore::owner_in_tx(&mut tx, user, target)?.is_none(),
        "fork target already exists"
    );
    ensure!(
        !tx.query_one(
            "SELECT EXISTS(SELECT 1 FROM service_threads WHERE thread_id=$1) OR \
             EXISTS(SELECT 1 FROM service_thread_admissions WHERE thread_id=$1)",
            &[&target],
        )?
        .get::<_, bool>(0),
        "fork target already exists"
    );
    let source_thread = tx
        .query_opt(
            "SELECT rollout_id,history_revision FROM service_threads WHERE thread_id=$1 FOR UPDATE",
            &[&source],
        )?
        .ok_or_else(|| anyhow!("source thread not found"))?;
    let source_rollout: String = source_thread.get(0);
    let source_revision: i64 = source_thread.get(1);
    ensure!(
        tx.query_opt(
            "SELECT 1 FROM service_thread_admissions WHERE thread_id=$1 \
             AND user_round=$2 AND turn_id=root_turn_id",
            &[&source, &through_round],
        )?
        .is_some(),
        "source user round not found"
    );

    // Include every completed continuation of each selected user round. A
    // queued/running continuation or a missing terminal fact is never silently
    // turned into completed history in the fork.
    let prefix = tx.query_one(
        "SELECT count(*) AS turns, \
          count(*) FILTER (WHERE a.status NOT IN \
            ('completed','failed','cancelled','interrupted','rejected','aborted') \
            OR terminal.records<>1 OR terminal.kind<>CASE WHEN a.status='completed' \
              THEN 'turn_completed' ELSE 'turn_aborted' END \
            OR NOT EXISTS(SELECT 1 FROM service_thread_admissions root \
              WHERE root.thread_id=a.thread_id AND root.turn_id=a.root_turn_id \
                AND root.turn_id=root.root_turn_id AND root.user_round=a.user_round)) AS invalid, \
          max(terminal.ordinal)+1 AS boundary, \
          count(DISTINCT a.user_round) FILTER (WHERE a.turn_id=a.root_turn_id) AS roots, \
          min(a.user_round) AS first_round \
        FROM service_thread_admissions a LEFT JOIN LATERAL ( \
          SELECT count(*) AS records,max(ordinal) AS ordinal,max(record_type) AS kind \
          FROM service_thread_records r WHERE r.thread_id=a.thread_id AND r.turn_id=a.turn_id \
            AND r.record_type IN ('turn_completed','turn_aborted') \
        ) terminal ON TRUE WHERE a.thread_id=$1 AND a.user_round<=$2",
        &[&source, &through_round],
    )?;
    ensure!(
        prefix.get::<_, i64>("turns") > 0
            && prefix.get::<_, i64>("invalid") == 0
            && prefix.get::<_, i64>("roots") == through_round
            && prefix.get::<_, Option<i64>>("first_round") == Some(1),
        "fork requires a completed user-round prefix"
    );
    let boundary = prefix
        .get::<_, Option<i64>>("boundary")
        .ok_or_else(|| anyhow!("completed source boundary is missing"))?;
    ensure!(
        boundary > 1 && boundary <= source_revision,
        "invalid source history boundary"
    );
    let valid = tx.query_one(
        "SELECT \
          (SELECT count(*) FROM service_thread_records WHERE thread_id=$1 AND ordinal<$3)=$3 \
          AND NOT EXISTS(SELECT 1 FROM service_thread_records WHERE thread_id=$1 AND ordinal<0) \
          AND NOT EXISTS(SELECT 1 FROM service_thread_records r \
            LEFT JOIN service_thread_admissions a ON a.thread_id=r.thread_id AND a.turn_id=r.turn_id \
            WHERE r.thread_id=$1 AND r.ordinal<$3 AND r.turn_id IS NOT NULL \
              AND (a.turn_id IS NULL OR a.user_round>$2)) \
          AND NOT EXISTS(SELECT 1 FROM service_thread_records r \
            JOIN service_thread_admissions a ON a.thread_id=r.thread_id AND a.turn_id=r.turn_id \
            WHERE r.thread_id=$1 AND r.ordinal>=$3 AND a.user_round<=$2) \
          AND (SELECT count(*) FROM service_thread_records \
            WHERE thread_id=$1 AND record_type='session_meta' AND ordinal=0)=1 \
          AND NOT EXISTS(SELECT 1 FROM service_thread_records \
            WHERE thread_id=$1 AND record_type='session_meta' AND ordinal<>0) \
          AND EXISTS(SELECT 1 FROM service_thread_display_state WHERE thread_id=$1 AND next_ordinal>=$3)",
        &[&source, &through_round, &boundary],
    )?.get::<_, bool>(0);
    ensure!(
        valid,
        "source history is not a complete isolated user-round prefix"
    );

    let metadata = tx
        .query_opt(
            "SELECT s.session_json FROM service_thread_sessions s \
         JOIN service_thread_records r ON r.thread_id=s.thread_id AND r.ordinal=0 \
         WHERE s.thread_id=$1 AND octet_length(s.session_json)<=$2::bigint \
           AND s.session_json::jsonb=r.record_json::jsonb->'payload'",
            &[&source, &(ROLLOUT_MAX_RECORD_BYTES as i64)],
        )?
        .ok_or_else(|| anyhow!("source session metadata is missing or invalid"))?;
    let mut session: SessionMeta = serde_json::from_str(metadata.get::<_, String>(0).as_str())?;
    ensure!(
        session.thread_id == source_id && session.rollout_id.as_str() == source_rollout,
        "source session metadata identity mismatch"
    );
    let now = Utc::now().timestamp_millis();
    session.thread_id = target_id;
    session.rollout_id = RolloutId::new(uuid::Uuid::new_v4().to_string())?;
    session.created_at_ms = now;
    session.parent_thread_id = Some(source_id);
    // This service fork owns a physical prefix copy and has no live dependency
    // on the source's rollout or lifetime. Frozen prompt/memory stay untouched.
    session.history_base = None;
    let session_json = serde_json::to_string(&session)?;
    let record = RolloutRecord {
        timestamp_ms: now,
        ordinal: 0,
        record: CanonicalRecord::SessionMeta(session.clone()),
    };
    let record_json = serde_json::to_string(&record)?;
    ensure!(
        record_json.len() <= ROLLOUT_MAX_RECORD_BYTES,
        "fork session metadata exceeds byte budget"
    );

    tx.execute(
        "INSERT INTO service_threads(thread_id,rollout_id,status,created_at_ms,updated_at_ms, \
          title,workspace_id,pinned,history_revision) \
         SELECT $2,$3,'active',$4,$4,title,workspace_id,FALSE,$5 \
         FROM service_threads WHERE thread_id=$1",
        &[
            &source,
            &target,
            &session.rollout_id.as_str(),
            &now,
            &boundary,
        ],
    )?;
    tx.execute(
        "INSERT INTO service_thread_sessions(thread_id,rollout_id,session_json) VALUES($1,$2,$3)",
        &[&target, &session.rollout_id.as_str(), &session_json],
    )?;
    tx.execute(
        "INSERT INTO service_thread_owners(thread_id,user_id,latest_user_round) VALUES($1,$2,$3)",
        &[&target, &user, &through_round],
    )?;
    tx.execute(
        "INSERT INTO service_thread_admissions(thread_id,turn_id,root_turn_id,user_round, \
           client_message_id,status,input_json,control_json,created_at_ms,updated_at_ms) \
         SELECT $2,turn_id,root_turn_id,user_round,client_message_id,status,NULL,control_json,created_at_ms,updated_at_ms \
         FROM service_thread_admissions WHERE thread_id=$1 AND user_round<=$3",
        &[&source, &target, &through_round],
    )?;
    tx.execute(
        "INSERT INTO service_thread_records(thread_id,ordinal,timestamp_ms,record_json,record_type) \
         VALUES($1,0,$2,$3,'session_meta')",
        &[&target, &now, &record_json],
    )?;
    let item_rebound = rebind_envelope("record_json::jsonb", "payload,payload,payload");
    let response_rebound = rebind_envelope("record_json::jsonb", "payload,payload");
    tx.execute(
        &format!("INSERT INTO service_thread_records(thread_id,ordinal,timestamp_ms,record_json, \
           record_type,turn_id,item_id) SELECT $2,ordinal,timestamp_ms, \
           CASE WHEN record_type='item_completed' THEN ({item_rebound})::text \
             WHEN record_type='response_item' THEN ({response_rebound})::text ELSE record_json END, \
           record_type,turn_id,item_id FROM service_thread_records \
           WHERE thread_id=$1 AND ordinal>0 AND ordinal<$3"),
        &[&source, &target, &boundary],
    )?;
    ensure!(
        !tx.query_one(
            "SELECT EXISTS(SELECT 1 FROM service_thread_records \
             WHERE thread_id=$1 AND octet_length(record_json)>$2::bigint)",
            &[&target, &(ROLLOUT_MAX_RECORD_BYTES as i64)],
        )?
        .get::<_, bool>(0),
        "fork canonical record exceeds byte budget"
    );
    let preview_rebound = rebind_envelope("preview_json::jsonb", "payload");
    tx.execute(
        &format!("INSERT INTO service_thread_items(thread_id,item_id,turn_id,root_turn_id,ordinal,visible,preview_json) \
          SELECT $2,item_id,turn_id,root_turn_id,ordinal,visible,({preview_rebound})::text \
          FROM service_thread_items WHERE thread_id=$1 AND ordinal<$3"),
        &[&source, &target, &boundary],
    )?;
    tx.execute(
        "INSERT INTO service_thread_chat_heads(thread_id,root_turn_id,summary_json) \
         SELECT $2,h.root_turn_id,h.summary_json FROM service_thread_chat_heads h \
         JOIN service_thread_admissions a ON a.thread_id=h.thread_id AND a.turn_id=h.root_turn_id \
         WHERE h.thread_id=$1 AND a.user_round<=$3",
        &[&source, &target, &through_round],
    )?;
    tx.execute(
        "INSERT INTO service_thread_display_state(thread_id,next_ordinal) VALUES($1,$2)",
        &[&target, &boundary],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{canonical_thread_log::wait, ThreadLogStore};
    use serde_json::json;
    use std::sync::{Arc, Barrier};
    use wunder_core::thread_store::*;

    const OWNER: &str = "fixture-owner";

    struct Fixture {
        storage: PostgresStorage,
        store: Arc<crate::storage::postgres::PostgresThreadStore>,
        threads: Vec<String>,
    }

    impl Fixture {
        fn new() -> Self {
            let storage = PostgresStorage::new(
                std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated PostgreSQL DSN"),
                5,
                8,
            )
            .unwrap();
            let store = storage.service_threads().unwrap();
            Self {
                storage,
                store,
                threads: vec![],
            }
        }

        fn id(&mut self) -> String {
            let id = format!("fixture-{}", uuid::Uuid::new_v4());
            self.threads.push(id.clone());
            id
        }

        fn source(&mut self) -> String {
            let id = self.id();
            self.store
                .admission_store()
                .register_owner(OWNER, &id)
                .unwrap();
            let thread = ThreadId::new(id.clone()).unwrap();
            let rollout = RolloutId::new(uuid::Uuid::new_v4().to_string()).unwrap();
            wait(self.store.create_thread(CreateThreadRequest {
                metadata: ThreadMetadata {
                    thread_id: thread.clone(),
                    current_rollout_id: rollout.clone(),
                    status: ThreadStatus::Active,
                    created_at_ms: 1,
                    updated_at_ms: 1,
                    title: Some("fixture title".into()),
                    workspace_id: Some("fixture-workspace".into()),
                    pinned: true,
                },
                session_meta: SessionMeta {
                    thread_id: thread,
                    rollout_id: rollout,
                    created_at_ms: 1,
                    source: "fixture".into(),
                    initial_system_prompt: "frozen fixture instructions".into(),
                    initial_memory: Some(json!({"content":"frozen fixture memory"})),
                    parent_thread_id: None,
                    history_base: None,
                    capabilities: json!({"fixture":true}),
                },
            }))
            .unwrap()
            .unwrap();
            id
        }

        fn start(&self, source: &str, root: Option<i64>) -> String {
            let admissions = self.store.admission_store();
            let mut input = json!({"content":"fixture question"});
            if let Some(root) = root {
                input["root_user_round"] = json!(root);
            }
            let admitted = admissions.accept(OWNER, source, &input).unwrap();
            let turn = admitted["turn_id"].as_str().unwrap().to_owned();
            admissions
                .begin_turn(OWNER, source, &turn, "fixture-model")
                .unwrap();
            turn
        }

        fn complete(&self, source: &str, turn: &str, text: &str) {
            wait(self.store.append_chat_payload(OWNER, &json!({
                "session_id":source,"turn_id":turn,"item_id":format!("{turn}:assistant"),
                "role":"assistant","kind":"assistant_message","status":"completed","content":text,
            }))).unwrap().unwrap();
            assert!(self
                .storage
                .update_thread_turn(
                    OWNER,
                    source,
                    turn,
                    "completed",
                    "",
                    &json!({"stop_reason":"fixture-stop"}),
                )
                .unwrap());
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Ok(mut conn) = self.storage.conn() {
                for thread in &self.threads {
                    let _ =
                        conn.execute("DELETE FROM service_threads WHERE thread_id=$1", &[thread]);
                    let _ = conn.execute(
                        "DELETE FROM service_thread_admissions WHERE thread_id=$1",
                        &[thread],
                    );
                    let _ = conn.execute(
                        "DELETE FROM service_thread_owners WHERE thread_id=$1",
                        &[thread],
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_fork_copies_completed_user_round_with_frozen_context_and_survives_source_delete() {
        let mut fixture = Fixture::new();
        let source = fixture.source();
        let target = fixture.id();
        let first = fixture.start(&source, None);
        let body = "fixture 中文🙂 ".repeat(600);
        fixture.complete(&source, &first, &body);
        let continuation = fixture.start(&source, Some(1));
        fixture.complete(&source, &continuation, "fixture continuation");
        let boundary = fixture
            .storage
            .latest_thread_change_seq_by_session(&source)
            .unwrap();
        let second = fixture.start(&source, None);
        fixture.complete(&source, &second, "fixture later answer");
        fixture
            .storage
            .fork_thread_log(OWNER, &source, &target, 1)
            .unwrap();

        let source_meta = wait(
            fixture
                .store
                .storage_session_meta(&ThreadId::new(&source).unwrap()),
        )
        .unwrap()
        .unwrap();
        let target_id = ThreadId::new(&target).unwrap();
        let meta = wait(fixture.store.storage_session_meta(&target_id))
            .unwrap()
            .unwrap();
        assert_eq!(meta.thread_id, target_id);
        assert_ne!(meta.rollout_id, source_meta.rollout_id);
        assert_eq!(meta.parent_thread_id, Some(ThreadId::new(&source).unwrap()));
        assert!(meta.history_base.is_none());
        assert_eq!(
            meta.initial_system_prompt,
            source_meta.initial_system_prompt
        );
        assert_eq!(meta.initial_memory, source_meta.initial_memory);
        assert_eq!(meta.capabilities, source_meta.capabilities);
        let metadata = wait(fixture.store.get_thread(&target_id)).unwrap().unwrap();
        assert_eq!(metadata.title.as_deref(), Some("fixture title"));
        assert_eq!(metadata.workspace_id.as_deref(), Some("fixture-workspace"));
        assert!(!metadata.pinned);
        assert_eq!(
            fixture
                .storage
                .latest_thread_change_seq_by_session(&target)
                .unwrap(),
            boundary
        );
        let page = fixture
            .storage
            .thread_view_page(OWNER, &target, "chat", None, 12)
            .unwrap();
        assert_eq!(page.chat.len(), 1);
        assert_eq!(page.latest_round, 1);
        assert_eq!(page.chat[0].execution_turn_id, continuation);
        assert_eq!(page.chat[0].assistant.content, "fixture continuation");
        assert_eq!(page.chat[0].stop_reason.as_deref(), Some("fixture-stop"));
        let first_item = fixture
            .storage
            .get_thread_item(OWNER, &target, &format!("{first}:assistant"), false)
            .unwrap()
            .unwrap();
        assert_eq!(first_item["payload"]["content"], body);
        assert_eq!(first_item["payload"]["session_id"], target);
        let user_item = fixture
            .storage
            .get_thread_item(OWNER, &target, &format!("{first}:user"), false)
            .unwrap()
            .unwrap();
        assert_eq!(user_item["payload"]["session_id"], target);
        assert!(fixture
            .storage
            .get_thread_item(OWNER, &target, &format!("{second}:assistant"), false)
            .unwrap()
            .is_none());
        let mut conn = fixture.storage.conn().unwrap();
        let copied = conn.query_one("SELECT count(*),count(input_json) FROM service_thread_admissions WHERE thread_id=$1", &[&target]).unwrap();
        assert_eq!(copied.get::<_, i64>(0), 2);
        assert_eq!(copied.get::<_, i64>(1), 0);
        conn.execute(
            "DELETE FROM service_thread_items WHERE thread_id=$1",
            &[&target],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM service_thread_chat_heads WHERE thread_id=$1",
            &[&target],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM service_thread_display_state WHERE thread_id=$1",
            &[&target],
        )
        .unwrap();
        drop(conn);
        let rebuilt = fixture
            .storage
            .thread_view_page(OWNER, &target, "chat", None, 12)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&rebuilt).unwrap(),
            serde_json::to_value(&page).unwrap()
        );
        wait(
            fixture
                .store
                .delete_thread(&ThreadId::new(&source).unwrap()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            fixture
                .storage
                .get_thread_item(OWNER, &target, &format!("{first}:assistant"), false)
                .unwrap()
                .unwrap()["payload"]["content"],
            body
        );
        let next = fixture
            .store
            .admission_store()
            .accept(OWNER, &target, &json!({"content":"fixture next question"}))
            .unwrap();
        assert_eq!(next["user_turn_index"], 2);
    }

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_fork_rejects_owner_target_and_incomplete_or_interleaved_prefix_without_partial_state(
    ) {
        let mut fixture = Fixture::new();
        let source = fixture.source();
        let target = fixture.id();
        let first = fixture.start(&source, None);
        assert!(fixture
            .storage
            .fork_thread_log(OWNER, &source, &target, 1)
            .is_err());
        assert!(fixture
            .storage
            .conn()
            .unwrap()
            .query_opt(
                "SELECT 1 FROM service_thread_owners WHERE thread_id=$1",
                &[&target]
            )
            .unwrap()
            .is_none());
        fixture.complete(&source, &first, "fixture first answer");
        assert!(fixture
            .storage
            .fork_thread_log("fixture-other-owner", &source, &target, 1)
            .is_err());
        assert!(fixture
            .storage
            .fork_thread_log(OWNER, &source, &source, 1)
            .is_err());
        assert!(fixture
            .storage
            .fork_thread_log(OWNER, &source, &target, 0)
            .is_err());
        assert!(fixture
            .storage
            .fork_thread_log(OWNER, &source, &target, 2)
            .is_err());
        fixture
            .store
            .admission_store()
            .register_owner(OWNER, &target)
            .unwrap();
        assert!(fixture
            .storage
            .fork_thread_log(OWNER, &source, &target, 1)
            .is_err());

        let target = fixture.id();
        let second = fixture.start(&source, None);
        let continuation = fixture.start(&source, Some(1));
        fixture.complete(&source, &continuation, "fixture late continuation");
        fixture.complete(&source, &second, "fixture second answer");
        assert!(fixture
            .storage
            .fork_thread_log(OWNER, &source, &target, 1)
            .is_err());
        assert!(fixture
            .storage
            .conn()
            .unwrap()
            .query_opt(
                "SELECT 1 FROM service_threads WHERE thread_id=$1",
                &[&target]
            )
            .unwrap()
            .is_none());
        fixture
            .storage
            .fork_thread_log(OWNER, &source, &target, 2)
            .unwrap();
        assert_eq!(
            fixture
                .storage
                .thread_view_page(OWNER, &target, "chat", None, 12)
                .unwrap()
                .chat
                .len(),
            2
        );
    }

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_fork_concurrent_same_target_has_one_atomic_winner() {
        let mut fixture = Fixture::new();
        let source = fixture.source();
        let target = fixture.id();
        let first = fixture.start(&source, None);
        fixture.complete(&source, &first, "fixture answer");
        let gate = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let storage = fixture.storage.clone();
                let source = source.clone();
                let target = target.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    storage.fork_thread_log(OWNER, &source, &target, 1)
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(Result::is_ok)
                .count(),
            1
        );
        assert_eq!(
            fixture
                .storage
                .thread_view_page(OWNER, &target, "chat", None, 12)
                .unwrap()
                .chat
                .len(),
            1
        );
    }

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_fork_preserves_aborted_history_and_excludes_later_pending_input() {
        let mut fixture = Fixture::new();
        let source = fixture.source();
        let target = fixture.id();
        let first = fixture.start(&source, None);
        assert!(fixture
            .storage
            .update_thread_turn(
                OWNER,
                &source,
                &first,
                "cancelled",
                "",
                &serde_json::Value::Null
            )
            .unwrap());
        let pending = fixture
            .store
            .admission_store()
            .accept(
                OWNER,
                &source,
                &json!({"content":"fixture pending question"}),
            )
            .unwrap();
        fixture
            .storage
            .fork_thread_log(OWNER, &source, &target, 1)
            .unwrap();
        let page = fixture
            .storage
            .thread_view_page(OWNER, &target, "chat", None, 12)
            .unwrap();
        assert_eq!(page.latest_round, 1);
        assert_eq!(page.chat[0].status, "cancelled");
        assert!(fixture
            .storage
            .get_thread_turn(
                OWNER,
                &target,
                pending["turn_id"].as_str().unwrap(),
                0,
                20,
                false
            )
            .unwrap()
            .is_none());
        assert!(fixture
            .storage
            .fork_thread_log(OWNER, &source, &target, 1)
            .is_err());
    }
}
