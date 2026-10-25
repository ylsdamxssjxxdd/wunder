use super::{PgTx, PostgresStorage, PostgresThreadStore};
use crate::storage::{
    thread_admission::{bounded_control_json, prepare_input_json, TurnAdmissions},
    StorageLifecycle,
};
use anyhow::{anyhow, bail, ensure, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::Arc;
use wunder_core::thread_store::{
    CanonicalEvent, CanonicalItem, CanonicalItemKind, CanonicalRecord, ItemId, RolloutRecord,
    TurnId,
};

/// Durable service admissions share a transaction boundary with canonical
/// lifecycle records. Accepted bodies disappear only after their first item
/// commits; immutable identity and bounded control remain in PostgreSQL.
#[derive(Clone)]
pub(crate) struct PostgresAdmissionStore {
    storage: Arc<PostgresStorage>,
}

impl PostgresAdmissionStore {
    pub(crate) fn new(storage: Arc<PostgresStorage>) -> Self {
        Self { storage }
    }

    fn conn(&self) -> Result<super::PgConn<'_>> {
        self.storage.ensure_initialized()?;
        self.storage.conn()
    }

    pub(super) fn lock_thread(tx: &mut PgTx<'_>, thread: &str) -> Result<()> {
        ensure!(!thread.trim().is_empty(), "missing thread identity");
        // Serialize first-owner creation, where there is no existing row to lock.
        // Canonical append/delete use this same transaction advisory key.
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            &[&thread],
        )?;
        Ok(())
    }

    pub(super) fn owner_in_tx(
        tx: &mut PgTx<'_>,
        user: &str,
        thread: &str,
    ) -> Result<Option<String>> {
        ensure!(!user.trim().is_empty(), "missing thread identity");
        let owner = tx.query_opt(
            "SELECT user_id,mutation_pending FROM service_thread_owners WHERE thread_id=$1 FOR UPDATE",
            &[&thread],
        )?;
        match owner {
            Some(row) => {
                let owner: String = row.get(0);
                ensure!(owner == user, "thread owner mismatch");
                ensure!(
                    row.get::<_, Option<String>>(1).is_none(),
                    "thread mutation is pending; retry after recovery"
                );
                Ok(Some(owner))
            }
            None => {
                ensure!(
                    !tx.query_one(
                        "SELECT EXISTS(SELECT 1 FROM service_threads WHERE thread_id=$1)",
                        &[&thread]
                    )?
                    .get::<_, bool>(0),
                    "thread owner is missing"
                );
                Ok(None)
            }
        }
    }

    pub(crate) fn check_owner(&self, user: &str, thread: &str) -> Result<()> {
        ensure!(
            !user.trim().is_empty() && !thread.trim().is_empty(),
            "missing thread identity"
        );
        let mut c = self.conn()?;
        let row = c.query_one(
            "SELECT (SELECT user_id FROM service_thread_owners WHERE thread_id=$1),
             EXISTS(SELECT 1 FROM service_threads WHERE thread_id=$1)",
            &[&thread],
        )?;
        let owner: Option<String> = row.get(0);
        match owner.as_deref() {
            Some(owner) if owner != user => bail!("thread owner mismatch"),
            None if row.get::<_, bool>(1) => bail!("thread owner is missing"),
            _ => Ok(()),
        }
    }

    pub(crate) fn register_owner(&self, user: &str, thread: &str) -> Result<bool> {
        let mut c = self.conn()?;
        let mut tx = c.transaction()?;
        Self::lock_thread(&mut tx, thread)?;
        let exists = Self::owner_in_tx(&mut tx, user, thread)?.is_some();
        if !exists {
            tx.execute(
                "INSERT INTO service_thread_owners(thread_id,user_id,latest_user_round) VALUES($1,$2,0)",
                &[&thread, &user],
            )?;
        }
        tx.commit()?;
        Ok(!exists)
    }

    pub(crate) fn accept(&self, user: &str, thread: &str, input: &Value) -> Result<Value> {
        ensure!(
            !user.trim().is_empty() && !thread.trim().is_empty(),
            "missing thread identity"
        );
        let body = prepare_input_json(input)?;
        // Service threads have no local execution lease. Never trust a proof
        // merely because the desktop-only lease implementation is absent.
        ensure!(
            input.get("local_open_lease").is_none_or(Value::is_null),
            "local execution leases are not supported by service storage"
        );
        let mut c = self.conn()?;
        let mut tx = c.transaction()?;
        Self::lock_thread(&mut tx, thread)?;
        if Self::owner_in_tx(&mut tx, user, thread)?.is_none() {
            tx.execute("INSERT INTO service_thread_owners(thread_id,user_id,latest_user_round) VALUES($1,$2,0)", &[&thread, &user])?;
        }
        let client = input
            .get("client_message_id")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty());
        if let Some(client) = client {
            if let Some(row) = tx.query_opt(
                "SELECT turn_id,root_turn_id,user_round,status FROM service_thread_admissions WHERE thread_id=$1 AND client_message_id=$2",
                &[&thread, &client],
            )? {
                let accepted = json!({"turn_id":row.get::<_,String>(0),"root_turn_id":row.get::<_,String>(1),"user_turn_index":row.get::<_,i64>(2),"status":row.get::<_,String>(3),"created":false,"client_message_id":client});
                tx.commit()?;
                return Ok(accepted);
            }
        }
        let turn = uuid::Uuid::new_v4().to_string();
        let (root, round) = if let Some(round) =
            input.get("root_user_round").and_then(Value::as_i64)
        {
            let row = tx.query_opt(
                "SELECT root_turn_id FROM service_thread_admissions WHERE thread_id=$1 AND user_round=$2 AND turn_id=root_turn_id",
                &[&thread, &round],
            )?.ok_or_else(|| anyhow::anyhow!("continuation root user round not found"))?;
            (row.get::<_, String>(0), round)
        } else {
            let round = tx.query_one("UPDATE service_thread_owners SET latest_user_round=latest_user_round+1 WHERE thread_id=$1 RETURNING latest_user_round", &[&thread])?.get::<_, i64>(0);
            (turn.clone(), round)
        };
        let now = chrono::Utc::now().timestamp_millis();
        tx.execute("INSERT INTO service_thread_admissions(thread_id,turn_id,root_turn_id,user_round,client_message_id,status,input_json,created_at_ms,updated_at_ms) VALUES($1,$2,$3,$4,$5,'accepted',$6,$7,$7)", &[&thread,&turn,&root,&round,&client,&body,&now])?;
        tx.commit()?;
        Ok(
            json!({"turn_id":turn,"root_turn_id":root,"user_turn_index":round,"status":"accepted","created":true,"client_message_id":client}),
        )
    }

    // Lifecycle finalization is deliberately a narrow append path: owner,
    // admission, immutable records and read projection commit together.
    pub(super) fn append_lifecycle(
        tx: &mut PgTx<'_>,
        session: &str,
        revision: i64,
        records: Vec<CanonicalRecord>,
    ) -> Result<()> {
        use crate::storage::rollout::{ROLLOUT_MAX_APPEND_BYTES, ROLLOUT_MAX_RECORD_BYTES};
        anyhow::ensure!(revision >= 0, "invalid thread history revision");
        let now = Utc::now().timestamp_millis();
        let mut prepared = Vec::with_capacity(records.len());
        let mut bytes = 0usize;
        for record in records {
            let ordinal = revision
                .checked_add(prepared.len() as i64)
                .ok_or_else(|| anyhow!("thread history revision overflow"))?;
            let record = RolloutRecord {
                ordinal: ordinal as u64,
                timestamp_ms: now,
                record,
            };
            let Some(record) = wunder_core::storage_policy::prepare_for_persistence(&record) else {
                continue;
            };
            let raw = serde_json::to_string(&record)?;
            anyhow::ensure!(
                raw.len() <= ROLLOUT_MAX_RECORD_BYTES,
                "canonical record exceeds persistence budget"
            );
            bytes = bytes
                .checked_add(raw.len())
                .ok_or_else(|| anyhow!("canonical append size overflow"))?;
            prepared.push((record, raw));
        }
        anyhow::ensure!(
            prepared.len() <= 1 || bytes <= ROLLOUT_MAX_APPEND_BYTES,
            "canonical append exceeds persistence budget"
        );
        let next = revision
            .checked_add(prepared.len() as i64)
            .ok_or_else(|| anyhow!("thread history revision overflow"))?;
        for (record, raw) in &prepared {
            let (kind, turn, item) = PostgresThreadStore::record_type(&record.record);
            tx.execute(
                "INSERT INTO service_thread_records(thread_id,ordinal,timestamp_ms,record_json,record_type,turn_id,item_id) VALUES($1,$2,$3,$4,$5,$6,$7)",
                &[&session,&(record.ordinal as i64),&record.timestamp_ms,raw,&kind,&turn,&item],
            )?;
        }
        super::thread_display::project_batch(
            tx,
            session,
            &prepared
                .iter()
                .map(|(record, _)| record.clone())
                .collect::<Vec<_>>(),
            next,
        )?;
        tx.execute(
            "UPDATE service_threads SET history_revision=$2,updated_at_ms=$3 WHERE thread_id=$1",
            &[&session, &next, &now],
        )?;
        Ok(())
    }

    pub(super) fn materialize_input(
        tx: &mut PgTx<'_>,
        thread: &str,
        turn: &str,
        root: &str,
        round: i64,
        input: Option<&str>,
        model: &str,
    ) -> Result<Vec<CanonicalRecord>> {
        let Some(raw) = input else { return Ok(vec![]) };
        let item_id = format!("{turn}:user");
        if tx.query_opt("SELECT 1 FROM service_thread_records WHERE thread_id=$1 AND item_id=$2 AND record_type='item_completed'", &[&thread,&item_id])?.is_some() {
            return Ok(vec![]);
        }
        let turn_id = TurnId::new(turn)?;
        let root = Some(TurnId::new(root)?);
        let now = Utc::now().timestamp_millis();
        let mut input: Value = serde_json::from_str(raw)?;
        input["role"] = json!("user");
        input["user_round"] = json!(round);
        input["session_id"] = json!(thread);
        input["turn_id"] = json!(turn);
        input["item_id"] = json!(item_id);
        input["status"] = json!("completed");
        Ok(vec![
            CanonicalRecord::TurnContext(wunder_core::thread_store::TurnContext {
                turn_id: turn_id.clone(),
                root_turn_id: root.clone(),
                model: model.into(),
                execution_policy: Value::Null,
            }),
            CanonicalRecord::Event(CanonicalEvent::TurnStarted {
                turn_id: turn_id.clone(),
                root_turn_id: root.clone(),
                started_at_ms: now,
            }),
            CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
                item_id: ItemId::new(item_id)?,
                turn_id,
                root_turn_id: root,
                created_at_ms: now,
                kind: CanonicalItemKind::UserMessage,
                payload: input,
                display_output: None,
                display_output_truncated: false,
            })),
        ])
    }

    pub(crate) fn begin_turn(
        &self,
        user: &str,
        thread: &str,
        turn: &str,
        model: &str,
    ) -> Result<()> {
        super::thread_display::ensure_ready(&self.storage, thread)?;
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        Self::lock_thread(&mut tx, thread)?;
        ensure!(
            Self::owner_in_tx(&mut tx, user, thread)?.is_some(),
            "thread owner is missing"
        );
        let admitted=tx.query_opt("SELECT status,root_turn_id,user_round,input_json FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=$2 FOR UPDATE", &[&thread,&turn])?.ok_or_else(||anyhow!("turn not found"))?;
        let status: String = admitted.get(0);
        if !TurnAdmissions::can_transition(&status, "running") {
            return Ok(());
        }
        let canonical = tx
            .query_opt(
                "SELECT status,history_revision FROM service_threads WHERE thread_id=$1 FOR UPDATE",
                &[&thread],
            )?
            .ok_or_else(|| anyhow!("thread not found"))?;
        ensure!(
            canonical.get::<_, String>(0) == "active",
            "thread is archived"
        );
        ensure!(
            tx.query_opt(
                "SELECT 1 FROM service_thread_sessions WHERE thread_id=$1",
                &[&thread]
            )?
            .is_some(),
            "thread session metadata is missing"
        );
        let records = Self::materialize_input(
            &mut tx,
            thread,
            turn,
            &admitted.get::<_, String>(1),
            admitted.get(2),
            admitted.get::<_, Option<String>>(3).as_deref(),
            model,
        )?;
        if !records.is_empty() {
            Self::append_lifecycle(&mut tx, thread, canonical.get(1), records)?;
        }
        Self::update_in_tx(&mut tx, thread, turn, "running", true, &Value::Null)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn pending(&self, thread: &str, turn: &str) -> Result<Option<Value>> {
        let mut c = self.conn()?;
        c.query_opt(
            "SELECT input_json FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=$2",
            &[&thread, &turn],
        )?
        .and_then(|row| row.get::<_, Option<String>>(0))
        .map(|raw| serde_json::from_str(&raw).map_err(Into::into))
        .transpose()
    }

    pub(crate) fn find_turn(&self, thread: &str, round: i64) -> Result<Option<String>> {
        let mut c = self.conn()?;
        Ok(c.query_opt("SELECT turn_id FROM service_thread_admissions WHERE thread_id=$1 AND user_round=$2 AND turn_id=root_turn_id", &[&thread,&round])?.map(|row|row.get(0)))
    }

    pub(crate) fn turn(&self, thread: &str, turn: &str) -> Result<Option<Value>> {
        let mut c = self.conn()?;
        c.query_opt("SELECT turn_id,root_turn_id,user_round,status,client_message_id,control_json,created_at_ms,updated_at_ms FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=$2", &[&thread,&turn])?
            .map(|row| {
                let mut value = json!({"turn_id":row.get::<_,String>(0),"root_turn_id":row.get::<_,String>(1),"user_turn_index":row.get::<_,i64>(2),"status":row.get::<_,String>(3),"client_message_id":row.get::<_,Option<String>>(4),"created_time":row.get::<_,i64>(6) as f64/1000.0,"updated_time":row.get::<_,i64>(7) as f64/1000.0});
                if let Some(raw) = row.get::<_,Option<String>>(5) {
                    let control: Value = serde_json::from_str(&raw)?;
                    for key in ["queued_input", "stop_reason"] {
                        if let Some(field) = control.get(key) { value[key] = field.clone(); }
                    }
                }
                Ok(value)
            }).transpose()
    }

    pub(crate) fn update_with_control(
        &self,
        thread: &str,
        turn: &str,
        status: &str,
        committed: bool,
        payload: &Value,
    ) -> Result<bool> {
        let mut c = self.conn()?;
        let mut tx = c.transaction()?;
        Self::lock_thread(&mut tx, thread)?;
        let changed = Self::update_in_tx(&mut tx, thread, turn, status, committed, payload)?;
        tx.commit()?;
        Ok(changed)
    }

    pub(super) fn update_in_tx(
        tx: &mut PgTx<'_>,
        thread: &str,
        turn: &str,
        status: &str,
        committed: bool,
        payload: &Value,
    ) -> Result<bool> {
        let owner = tx.query_opt(
            "SELECT mutation_pending FROM service_thread_owners WHERE thread_id=$1 FOR UPDATE",
            &[&thread],
        )?;
        let Some(owner) = owner else { return Ok(false) };
        ensure!(
            owner.get::<_, Option<String>>(0).is_none(),
            "thread mutation is pending; retry after recovery"
        );
        let previous = tx.query_opt("SELECT status,control_json,input_json IS NOT NULL FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=$2 FOR UPDATE", &[&thread,&turn])?;
        let Some(previous) = previous else {
            return Ok(false);
        };
        let previous_status: String = previous.get(0);
        if !TurnAdmissions::can_transition(&previous_status, status) {
            return Ok(false);
        }
        let previous_control: Option<String> = previous.get(1);
        let control = bounded_control_json(previous_control.as_deref(), status, payload)?;
        if previous_status == status
            && previous_control == control
            && !(committed && previous.get::<_, bool>(2))
        {
            return Ok(false);
        }
        tx.execute("UPDATE service_thread_admissions SET status=$3,control_json=$4,updated_at_ms=$5,input_json=CASE WHEN $6 THEN NULL ELSE input_json END WHERE thread_id=$1 AND turn_id=$2", &[&thread,&turn,&status,&control,&chrono::Utc::now().timestamp_millis(),&committed])?;
        Ok(true)
    }

    pub(crate) fn ids_by_status(
        &self,
        thread: &str,
        status: &str,
        limit: usize,
    ) -> Result<Vec<String>> {
        let mut c = self.conn()?;
        Ok(c.query("SELECT turn_id FROM service_thread_admissions WHERE thread_id=$1 AND status=$2 ORDER BY user_round DESC,created_at_ms DESC,turn_id DESC LIMIT $3", &[&thread,&status,&(limit.clamp(1,100) as i64)])?.into_iter().map(|row|row.get(0)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    struct Fixture {
        storage: PostgresStorage,
        _store: Arc<super::super::PostgresThreadStore>,
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
                _store: store,
                threads: vec![],
            }
        }
        fn thread(&mut self) -> String {
            let id = format!("fixture-{}", uuid::Uuid::new_v4());
            self.threads.push(id.clone());
            id
        }
        fn admissions(&self) -> PostgresAdmissionStore {
            PostgresAdmissionStore::new(Arc::new(self.storage.clone()))
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
    fn postgres_admission_concurrent_owner_and_retry_are_atomic() {
        let mut fixture = Fixture::new();
        let thread = fixture.thread();
        let gate = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let admissions = fixture.admissions();
                let thread = thread.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    admissions
                        .accept(
                            "fixture-user",
                            &thread,
                            &json!({"content":"question","client_message_id":"fixture-message"}),
                        )
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results[0]["turn_id"], results[1]["turn_id"]);
        assert_eq!(results.iter().filter(|v| v["created"] == true).count(), 1);
        assert_eq!(results[0]["user_turn_index"], 1);

        let thread = fixture.thread();
        let gate = Arc::new(Barrier::new(2));
        let handles: Vec<_> = ["fixture-owner-a", "fixture-owner-b"]
            .into_iter()
            .map(|user| {
                let admissions = fixture.admissions();
                let thread = thread.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    admissions.accept(user, &thread, &json!({"content":"question"}))
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .filter(Result::is_ok)
                .count(),
            1
        );
        let mut conn = fixture.storage.conn().unwrap();
        assert_eq!(
            conn.query_one(
                "SELECT COUNT(*) FROM service_thread_admissions WHERE thread_id=$1",
                &[&thread]
            )
            .unwrap()
            .get::<_, i64>(0),
            1
        );
        assert_eq!(
            conn.query_one(
                "SELECT latest_user_round FROM service_thread_owners WHERE thread_id=$1",
                &[&thread]
            )
            .unwrap()
            .get::<_, i64>(0),
            1
        );
    }

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_admission_continuations_control_and_rejections_match_local() {
        let mut fixture = Fixture::new();
        let admissions = fixture.admissions();
        let thread = fixture.thread();
        assert!(admissions
            .accept(
                "fixture-user",
                &thread,
                &json!({"content":"question","root_user_round":42})
            )
            .is_err());
        assert!(admissions
            .accept(
                "fixture-user",
                &thread,
                &json!({"content":"question","local_open_lease":{"lease_id":"unverified"}})
            )
            .is_err());
        assert_eq!(
            fixture
                .storage
                .conn()
                .unwrap()
                .query_one(
                    "SELECT COUNT(*) FROM service_thread_owners WHERE thread_id=$1",
                    &[&thread]
                )
                .unwrap()
                .get::<_, i64>(0),
            0
        );
        let root = admissions
            .accept(
                "fixture-user",
                &thread,
                &json!({"content":"question","local_open_lease":null}),
            )
            .unwrap();
        let continued = admissions
            .accept(
                "fixture-user",
                &thread,
                &json!({"content":"continue","root_user_round":1}),
            )
            .unwrap();
        assert_eq!(continued["root_turn_id"], root["turn_id"]);
        assert_eq!(continued["user_turn_index"], 1);
        assert_eq!(
            admissions.find_turn(&thread, 1).unwrap().as_deref(),
            root["turn_id"].as_str()
        );
        let turn = root["turn_id"].as_str().unwrap();
        assert!(admissions
            .pending(&thread, turn)
            .unwrap()
            .unwrap()
            .get("local_open_lease")
            .is_none());
        let huge = "x".repeat(16 * 1024);
        assert!(admissions.update_with_control(&thread,turn,"queued",false,&json!({"queued_input":{"queue_id":huge,"reason":huge,"priority":2,"content":huge},"content":huge,"stop_reason":huge})).unwrap());
        let control = admissions.turn(&thread, turn).unwrap().unwrap();
        assert_eq!(
            control["queued_input"]["queue_id"].as_str().unwrap().len(),
            256
        );
        assert_eq!(
            control["queued_input"]["reason"].as_str().unwrap().len(),
            1024
        );
        assert!(control["queued_input"].get("content").is_none());
        assert!(admissions
            .update_with_control(&thread, turn, "running", false, &Value::Null)
            .unwrap());
        assert!(!admissions
            .update_with_control(&thread, turn, "queued", false, &Value::Null)
            .unwrap());
        assert!(!admissions
            .update_with_control(&thread, turn, "unknown", false, &Value::Null)
            .unwrap());
        assert!(admissions
            .update_with_control(&thread, turn, "cancelled", false, &Value::Null)
            .unwrap());
        assert!(!admissions
            .update_with_control(&thread, turn, "running", false, &Value::Null)
            .unwrap());
        let terminal = admissions.turn(&thread, turn).unwrap().unwrap();
        assert!(terminal.get("queued_input").is_none());
        assert_eq!(terminal["stop_reason"].as_str().unwrap().len(), 1024);
        assert!(admissions.pending(&thread, turn).unwrap().is_some());
        let next = admissions
            .accept("fixture-user", &thread, &json!({"content":"next"}))
            .unwrap();
        assert_eq!(next["user_turn_index"], 2);
    }

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_admission_turn_pages_have_a_byte_budget() {
        let mut fixture = Fixture::new();
        let thread = fixture.thread();
        let admissions = fixture.admissions();
        let metadata = "x".repeat(3 * 1024 * 1024);
        for _ in 0..6 {
            admissions
                .accept(
                    "fixture-user",
                    &thread,
                    &json!({"content":"question","metadata":metadata}),
                )
                .unwrap();
        }
        let facade =
            super::super::thread_log_facade::PostgresThreadLog::new(&fixture.storage).unwrap();
        let page = facade
            .list_thread_turns("fixture-user", &thread, None, 200)
            .unwrap();
        assert_eq!(page.len(), 5);
        assert_eq!(page[0]["user_turn_index"], 6);
        assert!(serde_json::to_vec(&page).unwrap().len() <= 16 * 1024 * 1024);
        let before = page.last().unwrap()["user_turn_index"].as_i64();
        let next = facade
            .list_thread_turns("fixture-user", &thread, before, 200)
            .unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0]["user_turn_index"], 1);
    }

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_lifecycle_is_atomic_and_views_resume_from_tail() {
        use wunder_core::thread_store::{
            CreateThreadRequest, RolloutId, SessionMeta, ThreadId, ThreadMetadata,
            ThreadMetadataStore, ThreadStatus,
        };
        let mut fixture = Fixture::new();
        let admissions = fixture.admissions();
        let thread = fixture.thread();
        admissions.register_owner("fixture-user", &thread).unwrap();
        let id = ThreadId::new(&thread).unwrap();
        let rollout = RolloutId::new(uuid::Uuid::new_v4().to_string()).unwrap();
        let now = Utc::now().timestamp_millis();
        crate::storage::canonical_thread_log::wait(fixture._store.create_thread(
            CreateThreadRequest {
                metadata: ThreadMetadata {
                    thread_id: id.clone(),
                    current_rollout_id: rollout.clone(),
                    status: ThreadStatus::Active,
                    created_at_ms: now,
                    updated_at_ms: now,
                    title: None,
                    workspace_id: None,
                    pinned: false,
                },
                session_meta: SessionMeta {
                    thread_id: id,
                    rollout_id: rollout,
                    created_at_ms: now,
                    source: "fixture".into(),
                    initial_system_prompt: "fixed instructions".into(),
                    initial_memory: None,
                    parent_thread_id: None,
                    history_base: None,
                    capabilities: Value::Null,
                },
            },
        ))
        .unwrap()
        .unwrap();
        let first = admissions
            .accept(
                "fixture-user",
                &thread,
                &json!({"content":"first question"}),
            )
            .unwrap();
        let turn = first["turn_id"].as_str().unwrap().to_owned();
        let gate = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let admissions = admissions.clone();
                let thread = thread.clone();
                let turn = turn.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    admissions
                        .begin_turn("fixture-user", &thread, &turn, "fixture-model")
                        .unwrap()
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let mut conn = fixture.storage.conn().unwrap();
        for kind in ["turn_context", "turn_started", "item_completed"] {
            assert_eq!(conn.query_one("SELECT COUNT(*) FROM service_thread_records WHERE thread_id=$1 AND record_type=$2", &[&thread,&kind]).unwrap().get::<_,i64>(0),1);
        }
        assert!(admissions.pending(&thread, &turn).unwrap().is_none());
        assert_eq!(
            admissions.turn(&thread, &turn).unwrap().unwrap()["status"],
            "running"
        );
        drop(conn);
        let facade =
            super::super::thread_log_facade::PostgresThreadLog::new(&fixture.storage).unwrap();
        // Oversized terminal details must roll back both history and status.
        let before = facade.latest_thread_change_seq_by_session(&thread).unwrap();
        assert!(facade
            .update_thread_turn(
                "fixture-user",
                &thread,
                &turn,
                "failed",
                "",
                &json!({"details":"x".repeat(crate::storage::rollout::ROLLOUT_MAX_RECORD_BYTES)})
            )
            .is_err());
        assert_eq!(
            facade.latest_thread_change_seq_by_session(&thread).unwrap(),
            before
        );
        assert_eq!(
            admissions.turn(&thread, &turn).unwrap().unwrap()["status"],
            "running"
        );
        assert!(facade
            .update_thread_turn(
                "fixture-user",
                &thread,
                &turn,
                "completed",
                "",
                &Value::Null
            )
            .unwrap());
        assert!(!facade
            .update_thread_turn("fixture-user", &thread, &turn, "failed", "", &Value::Null)
            .unwrap());
        let second = admissions
            .accept(
                "fixture-user",
                &thread,
                &json!({"content":"second question"}),
            )
            .unwrap();
        let second = second["turn_id"].as_str().unwrap();
        // A terminal update before begin must materialize and clear input too.
        assert!(facade
            .update_thread_turn(
                "fixture-user",
                &thread,
                second,
                "cancelled",
                "",
                &json!({"stop_reason":"fixture stop"})
            )
            .unwrap());
        assert!(admissions.pending(&thread, second).unwrap().is_none());
        let tail = facade
            .load_thread_context_items("fixture-user", &thread, 1, false)
            .unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0]["content"], "second question");
        let changes = facade
            .list_thread_changes("fixture-user", &thread, before, 200)
            .unwrap();
        assert_eq!(changes.first().unwrap()["change_seq"], before + 1);
        assert!(changes.iter().any(|v| v["change_type"] == "turn_status"));
        let latest = facade.latest_thread_change_seq_by_session(&thread).unwrap();
        assert_eq!(
            facade
                .list_thread_changes("fixture-user", &thread, latest + 1, 200)
                .unwrap()[0]["change_type"],
            "snapshot_required"
        );
        let window = wunder_core::storage_backend::ThreadSnapshotWindow {
            max_turns: 1,
            max_items: 1,
            before_turn_index: None,
        };
        let snapshot = facade
            .thread_snapshot_window("fixture-user", &thread, &window)
            .unwrap();
        assert_eq!(snapshot["turns"].as_array().unwrap().len(), 1);
        assert_eq!(
            snapshot["items"][0]["payload"]["content"],
            "second question"
        );
        assert_eq!(snapshot["has_older"], true);
        let item = format!("{second}:user");
        assert!(facade
            .set_thread_item_feedback("fixture-user", &thread, &item, "up")
            .unwrap()
            .is_some());
        assert!(facade
            .set_thread_item_feedback("fixture-user", &thread, &item, "down")
            .unwrap()
            .is_none());
        assert_eq!(
            facade
                .get_thread_item("fixture-user", &thread, &item, false)
                .unwrap()
                .unwrap()["payload"]["feedback"]["vote"],
            "up"
        );
        fixture
            .storage
            .conn()
            .unwrap()
            .execute(
                "DELETE FROM meta WHERE key=$1",
                &[&format!("thread_feedback:{thread}:{item}")],
            )
            .unwrap();
    }
}
