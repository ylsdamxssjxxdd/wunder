//! Runtime commit boundary: canonical rollout first, derived history second.
use crate::core::blocking;
use crate::storage::{RuntimeThreadStore, StorageBackend};
use anyhow::{bail, Result};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tokio::sync::Mutex as AsyncMutex;
use wunder_core::thread_store::*;

pub(crate) struct ThreadLogCommitter {
    storage: Arc<dyn StorageBackend>,
    threads: Arc<RuntimeThreadStore>,
    hub: Arc<super::thread_change_hub::ThreadChangeHub>,
    gates: Mutex<HashMap<String, Weak<AsyncMutex<()>>>>,
}

impl ThreadLogCommitter {
    pub fn new(
        storage: Arc<dyn StorageBackend>,
        hub: Arc<super::thread_change_hub::ThreadChangeHub>,
        threads: Arc<RuntimeThreadStore>,
    ) -> Self {
        Self {
            storage,
            threads,
            hub,
            gates: Mutex::new(HashMap::new()),
        }
    }
    fn gate(&self, session: &str) -> Result<Arc<AsyncMutex<()>>> {
        let mut gates = self.gates.lock();
        gates.retain(|_, g| g.strong_count() > 0);
        if let Some(g) = gates.get(session).and_then(Weak::upgrade) {
            return Ok(g);
        }
        if gates.len() >= 256 {
            bail!("active commit limit reached");
        }
        let gate = Arc::new(AsyncMutex::new(()));
        gates.insert(session.to_owned(), Arc::downgrade(&gate));
        Ok(gate)
    }
    pub(crate) fn publish_context_usage(&self, session: &str, payload: Value) {
        self.hub.publish_context_usage(session, payload);
    }
    pub(crate) async fn latest_position(&self, session: &str) -> Result<i64> {
        match self
            .threads
            .history_position(&ThreadId::new(session)?)
            .await
        {
            Ok(p) => Ok(i64::try_from(p.end_ordinal_exclusive)?),
            Err(ThreadStoreError::NotFound { .. }) => Ok(0),
            Err(e) => Err(e.into()),
        }
    }
    pub(crate) async fn accept_turn(
        &self,
        user: &str,
        session: &str,
        input: &Value,
    ) -> Result<Value> {
        let gate = self.gate(session)?;
        let _gate = gate.lock().await;
        let admissions = self.threads.admission_store()?;
        let storage = self.storage.clone();
        let user = user.to_owned();
        let accepted_session = session.to_owned();
        let input = input.clone();
        let accepted = blocking::run_db("thread_store.accept", move || {
            if let Some(proof) = input.get("local_open_lease").filter(|v| !v.is_null()) {
                let lease = storage.get_thread_open_lease(&accepted_session, &user)?;
                let valid = lease.is_some_and(|lease| {
                    lease.owner_instance_id == proof["owner"].as_str().unwrap_or_default()
                        && lease.fencing_token == proof["token"].as_u64().unwrap_or_default()
                        && lease.expires_at > chrono::Utc::now().timestamp_millis() as f64 / 1000.0
                });
                if !valid {
                    bail!("THREAD_IN_USE: stale thread lease");
                }
            }
            admissions.accept(&user, &accepted_session, &input)
        })
        .await?;
        // Admission is control state and can precede the first rollout. Every
        // committed edge wakes subscribers, including an unchanged cursor.
        if accepted["created"] == true {
            self.hub
                .publish(session, self.latest_position(session).await?);
        }
        Ok(accepted)
    }
    pub(crate) async fn freeze_session(
        &self,
        user: &str,
        session: &str,
        prompt: &str,
        workspace: &str,
    ) -> Result<String> {
        let gate = self.gate(session)?;
        let _gate = gate.lock().await;
        let admissions = self.threads.admission_store()?;
        let owner = user.to_owned();
        let id = session.to_owned();
        blocking::run_db("thread_store.register_owner", move || {
            admissions.register_owner(&owner, &id)
        })
        .await?;
        let thread = ThreadId::new(session)?;
        match self.threads.session_meta(&thread).await {
            Ok(meta) => return Ok(meta.initial_system_prompt),
            Err(ThreadStoreError::NotFound { .. }) => {}
            Err(e) => return Err(e.into()),
        }
        let now = chrono::Utc::now().timestamp_millis();
        let rollout = RolloutId::new(uuid::Uuid::new_v4().to_string())?;
        let created = self
            .threads
            .create_thread(CreateThreadRequest {
                metadata: ThreadMetadata {
                    thread_id: thread.clone(),
                    current_rollout_id: rollout.clone(),
                    status: ThreadStatus::Active,
                    created_at_ms: now,
                    updated_at_ms: now,
                    title: None,
                    workspace_id: Some(workspace.to_owned()),
                    pinned: false,
                },
                session_meta: SessionMeta {
                    thread_id: thread.clone(),
                    rollout_id: rollout,
                    created_at_ms: now,
                    source: "runtime".into(),
                    initial_system_prompt: prompt.to_owned(),
                    initial_memory: None,
                    parent_thread_id: None,
                    history_base: None,
                    capabilities: Value::Null,
                },
            })
            .await;
        match created {
            Ok(_) => Ok(prompt.to_owned()),
            Err(error @ ThreadStoreError::Conflict { .. }) => {
                // Another process may freeze the same newly admitted thread
                // after our initial read. Reuse only its valid owned snapshot.
                let admissions = self.threads.admission_store()?;
                let owner = user.to_owned();
                let id = session.to_owned();
                blocking::run_db("thread_store.freeze_owner", move || {
                    admissions.check_owner(&owner, &id)
                })
                .await?;
                let metadata = self.threads.get_thread(&thread).await;
                let frozen = self.threads.session_meta(&thread).await;
                match (metadata, frozen) {
                    (Ok(metadata), Ok(frozen))
                        if metadata.thread_id == thread
                            && frozen.thread_id == thread
                            && metadata.current_rollout_id == frozen.rollout_id =>
                    {
                        Ok(frozen.initial_system_prompt)
                    }
                    _ => Err(error.into()),
                }
            }
            Err(error) => Err(error.into()),
        }
    }
    pub(crate) async fn begin_turn(
        &self,
        user: &str,
        session: &str,
        turn: &str,
        model: &str,
    ) -> Result<()> {
        self.threads
            .begin_turn(user, &ThreadId::new(session)?, &TurnId::new(turn)?, model)
            .await?;
        self.hub
            .publish(session, self.latest_position(session).await?);
        Ok(())
    }
    pub(crate) async fn update_turn(
        &self,
        user: &str,
        session: &str,
        turn: &str,
        status: &str,
        _summary: &str,
        payload: &Value,
    ) -> Result<bool> {
        let gate = self.gate(session)?;
        let _gate = gate.lock().await;
        if let Some(changed) = self
            .threads
            .update_service_turn(user, session, turn, status, _summary, payload)
            .await?
        {
            if changed {
                self.hub
                    .publish(session, self.latest_position(session).await?);
            }
            return Ok(changed);
        }
        let admissions = self.threads.admission_store()?;
        let owner = user.to_owned();
        let id = session.to_owned();
        let tid = turn.to_owned();
        let previous = blocking::run_db("thread_store.turn_state", move || {
            admissions.check_owner(&owner, &id)?;
            admissions.turn(&id, &tid)
        })
        .await?;
        let Some(previous) = previous else {
            bail!("accepted turn not found");
        };
        if !crate::storage::thread_admission::TurnAdmissions::can_transition(
            previous["status"].as_str().unwrap_or_default(),
            status,
        ) {
            return Ok(false);
        }
        let terminal = matches!(
            status,
            "completed" | "cancelled" | "failed" | "interrupted" | "rejected"
        );
        let thread = ThreadId::new(session)?;
        let committed_cursor = if terminal && self.threads.get_thread(&thread).await.is_ok() {
            // The accepted user body must commit before its terminal event.
            // This is idempotent for a turn which already began execution.
            self.threads
                .begin_turn(user, &thread, &TurnId::new(turn)?, "")
                .await?;
            let now = chrono::Utc::now().timestamp_millis();
            let event = if status == "completed" {
                CanonicalEvent::TurnCompleted {
                    turn_id: TurnId::new(turn)?,
                    completed_at_ms: now,
                }
            } else {
                let mut error = if payload.is_object() {
                    payload.clone()
                } else {
                    json!({"error":payload})
                };
                error["status"] = json!(status);
                CanonicalEvent::TurnAborted {
                    turn_id: TurnId::new(turn)?,
                    aborted_at_ms: now,
                    error: Some(error),
                }
            };
            let outcome = self
                .threads
                .append_canonical(&thread, vec![CanonicalRecord::Event(event)])
                .await?;
            Some(i64::try_from(
                outcome.persisted_through.end_ordinal_exclusive,
            )?)
        } else {
            None
        };
        let admissions = self.threads.admission_store()?;
        let id = session.to_owned();
        let tid = turn.to_owned();
        let status = status.to_owned();
        let control = payload.clone();
        let changed = blocking::run_db("thread_store.update_turn", move || {
            admissions.update_with_control(&id, &tid, &status, committed_cursor.is_some(), &control)
        })
        .await?;
        // Publish only after the queue/control transaction is visible. Waking
        // on the canonical append first can expose the old running state and
        // then strand event-driven subscribers without a second notification.
        if changed || committed_cursor.is_some() {
            let cursor = match committed_cursor {
                Some(cursor) => cursor,
                None => self.latest_position(session).await?,
            };
            self.hub.publish(session, cursor);
        }
        Ok(changed)
    }
    pub(crate) async fn cancel_active_turns(
        &self,
        user: &str,
        session: &str,
        source: &str,
    ) -> Result<usize> {
        let mut total = 0;
        for status in [
            "accepted",
            "queued",
            "running",
            "waiting_input",
            "waiting_approval",
        ] {
            loop {
                let admissions = self.threads.admission_store()?;
                let id = session.to_owned();
                let status = status.to_owned();
                let ids = blocking::run_db("thread_store.active_turns", move || {
                    admissions.ids_by_status(&id, &status, 32)
                })
                .await?;
                if ids.is_empty() {
                    break;
                }
                for turn in ids {
                    total += usize::from(
                        self.update_turn(
                            user,
                            session,
                            &turn,
                            "cancelled",
                            "",
                            &json!({"stop_reason":source}),
                        )
                        .await?,
                    );
                }
            }
        }
        Ok(total)
    }
    pub(crate) async fn commit_item(&self, user: &str, payload: &Value) -> Result<Option<Value>> {
        // Running items, token patches and diagnostics belong to the live stream.
        if !matches!(
            payload["status"].as_str(),
            Some("completed" | "failed" | "cancelled")
        ) {
            return Ok(None);
        }
        let session = payload["session_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("item has no thread"))?;
        let gate = self.gate(session)?;
        let _gate = gate.lock().await;
        let admissions = self.threads.admission_store()?;
        let owner = user.to_owned();
        let id = session.to_owned();
        let turn_id = payload["turn_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("item has no turn"))?
            .to_owned();
        let turn_metadata = blocking::run_db("thread_store.item_owner", move || {
            admissions.check_owner(&owner, &id)?;
            admissions.turn(&id, &turn_id)
        })
        .await?;
        let thread = ThreadId::new(session)?;
        let item_id = ItemId::new(
            payload["item_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("item has no id"))?,
        )?;
        if let Ok(existing) = self.threads.read_item(&thread, &item_id).await {
            if existing.item.payload == *payload {
                return Ok(None);
            }
            // Completed semantic items are immutable. Later accounting belongs
            // to usage/control state and cannot append a replacement body.
            return Ok(None);
        }
        let kind = match payload["kind"].as_str().unwrap_or_default() {
            "user_message" => CanonicalItemKind::UserMessage,
            "assistant_message" => CanonicalItemKind::AssistantMessage,
            "tool_call" | "tool_result" => CanonicalItemKind::ToolResult,
            "plan" => CanonicalItemKind::Plan,
            _ => CanonicalItemKind::Configuration,
        };
        let turn = TurnId::new(
            payload["turn_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("item has no turn"))?,
        )?;
        let root = turn_metadata
            .as_ref()
            .and_then(|metadata| metadata["root_turn_id"].as_str())
            .or_else(|| payload["root_turn_id"].as_str())
            .unwrap_or(turn.as_str());
        let root = TurnId::new(root)?;
        let now = chrono::Utc::now().timestamp_millis();
        let mut stored_payload = payload.clone();
        stored_payload["root_turn_id"] = json!(root.as_str());
        if stored_payload["kind"].as_str().is_none() {
            stored_payload["kind"] = serde_json::to_value(kind)?;
        }
        let item = CanonicalItem {
            item_id: item_id.clone(),
            turn_id: turn.clone(),
            root_turn_id: Some(root.clone()),
            created_at_ms: now,
            kind,
            payload: stored_payload.clone(),
            display_output: None,
            display_output_truncated: false,
        };
        let outcome = self
            .threads
            .append_canonical(
                &thread,
                vec![CanonicalRecord::Event(CanonicalEvent::ItemCompleted(item))],
            )
            .await?;
        let cursor = i64::try_from(outcome.persisted_through.end_ordinal_exclusive)?;
        self.hub.publish(session, cursor);
        // Use the same item DTO as replay: transport payload wraps item
        // metadata, and the semantic body lives in its own payload field.
        let mut body = stored_payload
            .get("payload")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| stored_payload.clone());
        if let Some(map) = body.as_object_mut() {
            map.remove("model_content");
            map.remove("config_overrides");
        }
        let wire_item = json!({
            "session_id":session,"turn_id":turn.as_str(),"root_turn_id":root.as_str(),
            "item_id":item_id.as_str(),"item_index":cursor-1,"created_seq":cursor,"revision":1,
            "kind":stored_payload["kind"],"status":stored_payload["status"],
            "visibility":stored_payload["visibility"].as_str().unwrap_or("user"),
            "payload":body,"created_time":now as f64/1000.0,"updated_time":now as f64/1000.0,
        });
        Ok(Some(
            json!({"change_type":"item_upsert","turn_id":turn.as_str(),"item_id":item_id.as_str(),"revision":1,"cursor":cursor,"payload":wire_item}),
        ))
    }
    pub(crate) async fn append_item(&self, user: &str, payload: &Value) -> Result<Option<Value>> {
        self.commit_item(user, payload).await
    }
    pub(crate) async fn set_feedback(
        &self,
        user: &str,
        session: &str,
        item: &str,
        vote: &str,
    ) -> Result<Option<Value>> {
        let admissions = self.threads.admission_store()?;
        let owner = user.to_owned();
        let thread = session.to_owned();
        blocking::run_db("thread_store.feedback_owner", move || {
            admissions.check_owner(&owner, &thread)
        })
        .await?;
        self.threads
            .read_item(&ThreadId::new(session)?, &ItemId::new(item)?)
            .await?;
        let storage = self.storage.clone();
        let (owner, target, item, vote) = (
            user.to_owned(),
            session.to_owned(),
            item.to_owned(),
            vote.to_owned(),
        );
        let changed = blocking::run_db("thread_store.feedback", move || {
            storage.set_thread_item_feedback(&owner, &target, &item, &vote)
        })
        .await?;
        if changed.is_some() {
            self.hub
                .publish(session, self.latest_position(session).await?);
        }
        Ok(changed)
    }
}

#[cfg(all(test, feature = "postgres-storage"))]
mod postgres_tests {
    use super::*;
    use crate::storage::{PostgresStorage, ThreadLogStore};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    async fn postgres_concurrent_prompt_freeze_reuses_the_winner() {
        let storage = Arc::new(
            PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 8).unwrap(),
        );
        let store = storage.service_threads().unwrap();
        let session = format!("fixture-{}", uuid::Uuid::new_v4());
        let first = ThreadLogCommitter::new(
            storage.clone(),
            Arc::new(super::super::thread_change_hub::ThreadChangeHub::new()),
            Arc::new(RuntimeThreadStore::postgres(store.clone())),
        );
        let second = ThreadLogCommitter::new(
            storage.clone(),
            Arc::new(super::super::thread_change_hub::ThreadChangeHub::new()),
            Arc::new(RuntimeThreadStore::postgres(store.clone())),
        );
        let (a, b) = tokio::join!(
            first.freeze_session("fixture-user", &session, "fixture prompt a", ""),
            second.freeze_session("fixture-user", &session, "fixture prompt b", "")
        );
        let a = a.unwrap();
        let b = b.unwrap();
        assert_eq!(a, b);
        let thread = ThreadId::new(&session).unwrap();
        let frozen = store.storage_session_meta(&thread).await.unwrap();
        assert_eq!(frozen.initial_system_prompt, a);
        assert!(first
            .freeze_session(
                "fixture-other-owner",
                &session,
                "different instructions",
                ""
            )
            .await
            .is_err());
        assert_eq!(
            first
                .freeze_session("fixture-user", &session, "later instructions", "")
                .await
                .unwrap(),
            a
        );
        // A frozen empty thread has exactly its initial SessionMeta record.
        assert_eq!(first.latest_position(&session).await.unwrap(), 1);
        store.delete_thread(&thread).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    async fn postgres_production_turn_committer_finalizes_atomically() {
        let storage = Arc::new(
            PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 4).unwrap(),
        );
        let service = storage.service_threads().unwrap();
        let runtime = Arc::new(RuntimeThreadStore::postgres(service.clone()));
        let hub = Arc::new(super::super::thread_change_hub::ThreadChangeHub::new());
        let committer = ThreadLogCommitter::new(storage.clone(), hub.clone(), runtime);
        let session = format!("fixture-{}", uuid::Uuid::new_v4());
        committer
            .freeze_session("fixture-user", &session, "fixed fixture instructions", "")
            .await
            .unwrap();
        let accepted = committer
            .accept_turn(
                "fixture-user",
                &session,
                &json!({"content":"fixture question"}),
            )
            .await
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        let mut wake = hub.subscribe(&session);
        let before = committer.latest_position(&session).await.unwrap();
        assert!(committer
            .update_turn(
                "fixture-user",
                &session,
                turn,
                "failed",
                "",
                &json!({"error":"x".repeat(crate::storage::rollout::ROLLOUT_MAX_RECORD_BYTES)})
            )
            .await
            .is_err());
        assert_eq!(committer.latest_position(&session).await.unwrap(), before);
        assert!(!wake.has_changed().unwrap());
        let admissions = service.admission_store();
        let id = session.clone();
        let tid = turn.to_owned();
        let state = blocking::run_db("fixture.turn", move || admissions.turn(&id, &tid))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state["status"], "accepted");
        assert!(committer
            .update_turn(
                "fixture-user",
                &session,
                turn,
                "completed",
                "",
                &Value::Null
            )
            .await
            .unwrap());
        wake.changed().await.unwrap();
        let cursor = *wake.borrow_and_update();
        assert_eq!(cursor, committer.latest_position(&session).await.unwrap());
        assert_eq!(cursor, before + 4);
        assert!(!committer
            .update_turn("fixture-user", &session, turn, "failed", "", &Value::Null)
            .await
            .unwrap());
        assert!(!wake.has_changed().unwrap());
        let state = storage
            .get_thread_turn("fixture-user", &session, turn, 0, 100, true)
            .unwrap()
            .unwrap();
        assert_eq!(state["status"], "completed");
        assert_eq!(state["items"].as_array().unwrap().len(), 1);
        let payload = service.admission_store().pending(&session, turn).unwrap();
        assert!(payload.is_none());
        service
            .delete_thread(&ThreadId::new(session).unwrap())
            .await
            .unwrap();
    }
}
