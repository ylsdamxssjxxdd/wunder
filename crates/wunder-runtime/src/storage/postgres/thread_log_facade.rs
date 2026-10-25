//! PostgreSQL adapter for the synchronous `ThreadLogStore` application
//! contract.  The service profile must never open the local canonical SQLite
//! projection; this facade translates the bounded API into `service_threads`,
//! `service_thread_records` and the PostgreSQL admission tables.

use super::{PgTx, PostgresAdmissionStore, PostgresStorage, PostgresThreadStore};
use crate::storage::thread_admission::TurnAdmissions;
use crate::storage::ThreadTurnTail;
use crate::storage::{MetaStore, StorageLifecycle};
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use wunder_core::storage_backend::ThreadSnapshotWindow;
use wunder_core::thread_store::{
    CanonicalEvent, CanonicalItem, CanonicalItemKind, CanonicalRecord, ItemId, RolloutRecord,
    ThreadHistoryReader, ThreadId, ThreadMetadataStore, TurnId,
};
#[path = "thread_view_store.rs"]
mod view;
pub(crate) struct PostgresThreadLog<'a> {
    pub(super) storage: &'a PostgresStorage,
    pub(super) store: Arc<PostgresThreadStore>,
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    #[test]
    fn snapshots_project_execution_span_without_changing_commit_order() {
        let mut item = CanonicalItem {
            item_id: ItemId::new("sample-timing-item").unwrap(),
            turn_id: TurnId::new("sample-timing-turn").unwrap(),
            root_turn_id: None,
            created_at_ms: 10_000,
            kind: CanonicalItemKind::AssistantMessage,
            payload: json!({"content":"sample reply","prefill_duration_s":0.5,"decode_duration_s":1.5}),
            display_output: None,
            display_output_truncated: false,
        };
        let wire = PostgresThreadLog::item_wire(&item, 7);
        assert_eq!(wire["created_time"], 10.0);
        assert_eq!(wire["updated_time"], 10.0);
        assert_eq!(wire["started_at"], 8.0);
        assert_eq!(wire["completed_at"], 10.0);
        assert_eq!(wire["item_index"], 7);

        item.kind = CanonicalItemKind::ToolCall;
        item.payload = json!({"item_id":"sample-timing-item","kind":"tool_call",
            "payload":{"tool":"sample_tool","meta":{"duration_ms":250}}});
        let wire = PostgresThreadLog::item_wire(&item, 8);
        assert_eq!(wire["started_at"], 9.75);
        assert_eq!(wire["completed_at"], 10.0);
        assert_eq!(wire["payload"]["tool"], "sample_tool");

        item.payload = json!({"tool":"sample_tool"});
        let unknown = PostgresThreadLog::item_wire(&item, 9);
        assert!(unknown.get("started_at").is_none());
        assert!(unknown.get("completed_at").is_none());
    }
}

impl<'a> PostgresThreadLog<'a> {
    pub(crate) fn new(storage: &'a PostgresStorage) -> Result<Self> {
        Ok(Self {
            store: storage.service_threads_ref()?.clone(),
            storage,
        })
    }

    fn check_owner(&self, user: &str, session: &str) -> Result<()> {
        self.store.admission_store().check_owner(user, session)
    }

    fn item_kind(value: &str) -> CanonicalItemKind {
        match value {
            "user_message" => CanonicalItemKind::UserMessage,
            "assistant_message" => CanonicalItemKind::AssistantMessage,
            "assistant_reasoning" => CanonicalItemKind::AssistantReasoning,
            "tool_call" => CanonicalItemKind::ToolCall,
            "tool_result" | "tool_message" => CanonicalItemKind::ToolResult,
            "plan" => CanonicalItemKind::Plan,
            "command_result" => CanonicalItemKind::CommandResult,
            _ => CanonicalItemKind::Configuration,
        }
    }

    fn item_wire(item: &CanonicalItem, ordinal: u64) -> Value {
        let kind = item
            .payload
            .get("kind")
            .filter(|v| v.is_string())
            .cloned()
            .unwrap_or_else(|| {
                serde_json::to_value(item.kind).unwrap_or_else(|_| json!("configuration"))
            });
        let wrapped =
            item.payload.get("item_id").is_some() && item.payload.get("payload").is_some();
        let mut payload = item
            .payload
            .get("payload")
            .filter(|v| wrapped && v.is_object())
            .cloned()
            .unwrap_or_else(|| item.payload.clone());
        if !payload.is_object() {
            payload = json!({"content": payload});
        }
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("completed");
        let committed = item.created_at_ms as f64 / 1000.0;
        let span = crate::storage::thread_view_projection::trajectory_span(
            &payload,
            kind.as_str().unwrap_or_default(),
            status,
            committed,
            committed,
        );
        let mut wire = json!({
            "item_id": item.item_id,
            "turn_id": item.turn_id,
            "root_turn_id": item.root_turn_id.as_ref().unwrap_or(&item.turn_id),
            "item_index": ordinal,
            "created_seq": ordinal.saturating_add(1),
            "revision": 1,
            "kind": kind,
            "status": status,
            "visibility": payload.get("visibility").and_then(Value::as_str).unwrap_or("user"),
            "payload": payload,
            "created_time": item.created_at_ms as f64 / 1000.0,
            "updated_time": item.created_at_ms as f64 / 1000.0,
        });
        // Preserve bookkeeping timestamps for ordering. Add real execution
        // boundaries for consumers that otherwise mistake commit time for a
        // zero-duration operation. An unknown span remains absent.
        if span.0 != committed || span.1.is_some_and(|end| end != committed) {
            wire["started_at"] = json!(span.0);
            wire["completed_at"] = json!(span.1);
        }
        wire
    }

    fn record_item(raw: &str, ordinal: i64) -> Result<Option<Value>> {
        let record: RolloutRecord = serde_json::from_str(raw)?;
        let CanonicalRecord::Event(CanonicalEvent::ItemCompleted(item)) = record.record else {
            return Ok(None);
        };
        Ok(Some(Self::item_wire(&item, ordinal.max(0) as u64)))
    }

    fn redact_item(item: &mut Value, internal: bool) {
        if !internal {
            if let Some(body) = item["payload"].as_object_mut() {
                body.remove("model_content");
                body.remove("config_overrides");
            }
            if item["kind"] == "system_message" {
                crate::storage::thread_view_projection::redact_internal_context(
                    &mut item["payload"],
                );
            }
        }
    }

    fn message_payload(item: Value) -> Result<Value> {
        let mut body = item["payload"].clone();
        for key in [
            "item_id",
            "turn_id",
            "root_turn_id",
            "kind",
            "status",
            "created_seq",
            "created_time",
            "revision",
        ] {
            body[key] = item[key].clone();
        }
        if body["role"].is_null() {
            body["role"] = json!(match item["kind"].as_str() {
                Some("user_message") => "user",
                Some("assistant_message" | "agent_message") => "assistant",
                Some("tool_result") => "tool",
                Some("system_message") => "system",
                _ => bail!("completed chat item has no message role"),
            });
        }
        Ok(body)
    }

    fn items_in_tx(
        tx: &mut PgTx<'_>,
        session: &str,
        root: Option<&str>,
        before: Option<i64>,
        after: Option<i64>,
        limit: i64,
        internal: bool,
        chat_only: bool,
        descending: bool,
    ) -> Result<Vec<Value>> {
        let order = if descending { "DESC" } else { "ASC" };
        // Select ordinals and lengths first: an API page must not materialize
        // hundreds of maximum-size records before applying its byte budget.
        let rows = tx.query(&format!("WITH candidates AS MATERIALIZED (
            SELECT i.ordinal FROM service_thread_items i
            WHERE i.thread_id=$1 AND ($2::text IS NULL OR i.root_turn_id=$2)
              AND ($3::bigint IS NULL OR i.ordinal<$3) AND ($4::bigint IS NULL OR i.ordinal>$4)
              AND ($5 OR i.visible)
              AND (NOT $6 OR i.preview_json::jsonb->>'kind' IN ('user_message','assistant_message','agent_message','tool_result','system_message'))
            ORDER BY i.ordinal {order} LIMIT $7
        ), sized AS (
            SELECT c.ordinal,SUM(octet_length(r.record_json)) OVER (ORDER BY c.ordinal {order}) AS bytes,
                ROW_NUMBER() OVER (ORDER BY c.ordinal {order}) AS n
            FROM candidates c JOIN service_thread_records r ON r.thread_id=$1 AND r.ordinal=c.ordinal
        ) SELECT r.ordinal,r.record_json FROM sized s JOIN service_thread_records r
            ON r.thread_id=$1 AND r.ordinal=s.ordinal WHERE s.bytes<=16777216 OR s.n=1
            ORDER BY r.ordinal {order}"),
            &[&session,&root,&before,&after,&internal,&chat_only,&limit.clamp(1,501)])?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(mut item) = Self::record_item(&row.get::<_, String>(1), row.get(0))? {
                item["session_id"] = json!(session);
                Self::redact_item(&mut item, internal);
                items.push(item);
            }
        }
        let feedback_keys: Vec<String> = items
            .iter()
            .filter_map(|item| item["item_id"].as_str())
            .map(|id| format!("thread_feedback:{session}:{id}"))
            .collect();
        if !feedback_keys.is_empty() {
            let feedback: HashMap<String, String> = tx
                .query(
                    "SELECT key,value FROM meta WHERE key=ANY($1)",
                    &[&feedback_keys],
                )?
                .into_iter()
                .map(|row| (row.get(0), row.get(1)))
                .collect();
            for item in &mut items {
                if let Some(id) = item["item_id"].as_str() {
                    if let Some(vote) = feedback.get(&format!("thread_feedback:{session}:{id}")) {
                        item["payload"]["feedback"] = json!({"vote":vote});
                    }
                }
            }
        }
        Ok(items)
    }

    pub(crate) fn accept_thread_turn(
        &self,
        user: &str,
        session: &str,
        input: &Value,
    ) -> Result<Value> {
        self.store.admission_store().accept(user, session, input)
    }

    pub(crate) fn find_thread_turn_id(
        &self,
        user: &str,
        session: &str,
        round: i64,
    ) -> Result<Option<String>> {
        self.check_owner(user, session)?;
        self.store.admission_store().find_turn(session, round)
    }

    pub(crate) fn update_thread_turn(
        &self,
        user: &str,
        session: &str,
        turn: &str,
        status: &str,
        _summary: &str,
        payload: &Value,
    ) -> Result<bool> {
        self.storage.ensure_initialized()?;
        // Repair only this accessed projection, before entering its write lock.
        super::thread_display::ensure_ready(self.storage, session)?;
        let mut conn = self.storage.conn()?;
        let mut tx = conn.transaction()?;
        PostgresAdmissionStore::lock_thread(&mut tx, session)?;
        if PostgresAdmissionStore::owner_in_tx(&mut tx, user, session)?.is_none() {
            return Ok(false);
        }
        let previous = tx.query_opt(
            "SELECT status,root_turn_id,user_round,input_json FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=$2 FOR UPDATE",
            &[&session,&turn],
        )?.ok_or_else(|| anyhow!("turn not found"))?;
        let previous_status: String = previous.get(0);
        if !TurnAdmissions::can_transition(&previous_status, status) {
            return Ok(false);
        }
        let mut committed = false;
        if TurnAdmissions::is_terminal(status) {
            if let Some(thread) = tx.query_opt(
                "SELECT rollout_id,status,history_revision FROM service_threads WHERE thread_id=$1 FOR UPDATE",
                &[&session],
            )? {
                anyhow::ensure!(thread.get::<_,String>(1) == "active", "thread is archived");
                anyhow::ensure!(tx.query_opt("SELECT 1 FROM service_thread_sessions WHERE thread_id=$1", &[&session])?.is_some(), "thread session metadata is missing");
                let turn_id = TurnId::new(turn)?;
                let now = Utc::now().timestamp_millis();
                let mut records = PostgresAdmissionStore::materialize_input(
                    &mut tx,session,turn,&previous.get::<_,String>(1),previous.get(2),
                    previous.get::<_,Option<String>>(3).as_deref(),"",
                )?;
                records.push(CanonicalRecord::Event(if status == "completed" {
                    CanonicalEvent::TurnCompleted {turn_id, completed_at_ms: now}
                } else {
                    CanonicalEvent::TurnAborted {turn_id, aborted_at_ms: now, error: Some(json!({"status":status,"details":payload}))}
                }));
                PostgresAdmissionStore::append_lifecycle(&mut tx, session, thread.get(2), records)?;
                committed = true;
            }
        }
        let changed = PostgresAdmissionStore::update_in_tx(
            &mut tx, session, turn, status, committed, payload,
        )?;
        tx.commit()?;
        Ok(changed)
    }

    pub(crate) fn list_thread_turn_ids_by_status(
        &self,
        user: &str,
        session: &str,
        status: &str,
        limit: usize,
    ) -> Result<Vec<String>> {
        self.check_owner(user, session)?;
        self.store
            .admission_store()
            .ids_by_status(session, status, limit)
    }

    pub(crate) fn commit_thread_item(&self, user: &str, payload: &Value) -> Result<Option<Value>> {
        let session = payload
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("missing thread"))?;
        self.check_owner(user, session)?;
        if payload
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|s| !matches!(s, "completed" | "failed" | "cancelled"))
        {
            return Ok(None);
        }
        let thread = ThreadId::new(session)?;
        let turn = TurnId::new(
            payload
                .get("turn_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("missing turn"))?,
        )?;
        let id = ItemId::new(
            payload
                .get("item_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("missing item"))?,
        )?;
        match crate::storage::canonical_thread_log::wait(self.store.read_item(&thread, &id))? {
            Ok(_) => return Ok(None),
            Err(wunder_core::thread_store::ThreadStoreError::NotFound { .. }) => {}
            Err(error) => return Err(error.into()),
        }
        crate::storage::canonical_thread_log::wait(
            self.store.begin_turn(user, &thread, &turn, ""),
        )??;
        let root = {
            let mut conn = self.storage.conn()?;
            conn.query_opt("SELECT root_turn_id FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=$2", &[&session, &turn.as_str()])?.map(|r| r.get::<_,String>(0)).and_then(|s| TurnId::new(s).ok())
        };
        let item = CanonicalItem {
            item_id: id.clone(),
            turn_id: turn.clone(),
            root_turn_id: root,
            created_at_ms: Utc::now().timestamp_millis(),
            kind: Self::item_kind(payload.get("kind").and_then(Value::as_str).unwrap_or("")),
            payload: payload.clone(),
            display_output: None,
            display_output_truncated: false,
        };
        let outcome = crate::storage::canonical_thread_log::wait(self.store.append_canonical(
            &thread,
            vec![CanonicalRecord::Event(CanonicalEvent::ItemCompleted(item))],
        ))??;
        Ok(Some(
            json!({"change_type":"item_upsert","turn_id":turn,"item_id":id,"revision":1,"cursor":outcome.persisted_through.end_ordinal_exclusive}),
        ))
    }

    pub(crate) fn append_thread_item(&self, user: &str, payload: &Value) -> Result<()> {
        self.commit_thread_item(user, payload).map(|_| ())
    }

    pub(crate) fn get_thread_item(
        &self,
        user: &str,
        session: &str,
        item_id: &str,
        internal: bool,
    ) -> Result<Option<Value>> {
        self.check_owner(user, session)?;
        if !internal {
            super::thread_display::ensure_ready(self.storage, session)?;
            let mut conn = self.storage.conn()?;
            if conn.query_opt("SELECT 1 FROM service_thread_items WHERE thread_id=$1 AND item_id=$2 AND visible", &[&session,&item_id])?.is_none() {
                return Ok(None);
            }
        }
        let item = crate::storage::canonical_thread_log::wait(
            self.store
                .read_item(&ThreadId::new(session)?, &ItemId::new(item_id.to_owned())?),
        )?;
        match item {
            Ok(summary) => {
                let mut item = Self::item_wire(&summary.item, summary.rollout_ordinal);
                item["session_id"] = json!(session);
                if !internal {
                    if item["visibility"] != "user"
                        || item["payload"]
                            .pointer("/meta/hidden")
                            .and_then(Value::as_bool)
                            == Some(true)
                        || item["payload"]
                            .pointer("/payload/meta/hidden")
                            .and_then(Value::as_bool)
                            == Some(true)
                    {
                        return Ok(None);
                    }
                    if let Some(body) = item["payload"].as_object_mut() {
                        body.remove("model_content");
                        body.remove("config_overrides");
                    }
                    if item["kind"] == "system_message" {
                        crate::storage::thread_view_projection::redact_internal_context(
                            &mut item["payload"],
                        );
                    }
                }
                if let Some(vote) = self
                    .storage
                    .get_meta(&format!("thread_feedback:{session}:{item_id}"))?
                {
                    item["payload"]["feedback"] = json!({"vote":vote});
                }
                Ok(Some(item))
            }
            Err(wunder_core::thread_store::ThreadStoreError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub(crate) fn list_thread_visible_messages(
        &self,
        user: &str,
        session: &str,
        before: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.check_owner(user, session)?;
        super::thread_display::ensure_ready(self.storage, session)?;
        let mut conn = self.storage.conn()?;
        let mut tx = conn.transaction()?;
        let items = Self::items_in_tx(
            &mut tx,
            session,
            None,
            before.map(|v| v.saturating_sub(1)),
            None,
            limit,
            false,
            true,
            true,
        )?;
        items.into_iter().map(Self::message_payload).collect()
    }

    pub(crate) fn load_thread_context_items(
        &self,
        user: &str,
        session: &str,
        limit: i64,
        internal: bool,
    ) -> Result<Vec<Value>> {
        self.check_owner(user, session)?;
        super::thread_display::ensure_ready(self.storage, session)?;
        let mut conn = self.storage.conn()?;
        let mut tx = conn.transaction()?;
        let mut items = Self::items_in_tx(
            &mut tx,
            session,
            None,
            None,
            None,
            if limit <= 0 { 500 } else { limit },
            internal,
            true,
            true,
        )?;
        items.reverse();
        items.into_iter().map(Self::message_payload).collect()
    }

    pub(crate) fn load_thread_execution_context(
        &self,
        user: &str,
        session: &str,
        turn: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.check_owner(user, session)?;
        let exclude = if turn.is_empty() {
            None
        } else {
            Some(TurnId::new(turn)?)
        };
        let mut messages = match crate::storage::canonical_thread_log::wait(
            self.store
                .model_messages(&ThreadId::new(session)?, exclude.as_ref()),
        )? {
            Ok(messages) => messages,
            Err(wunder_core::thread_store::ThreadStoreError::NotFound { .. }) => vec![],
            Err(error) => return Err(error.into()),
        };
        if limit > 0 && messages.len() > limit as usize {
            messages.drain(..messages.len() - limit as usize);
        }
        Ok(messages)
    }

    pub(crate) fn latest_thread_user_round_by_session(&self, session: &str) -> Result<i64> {
        let mut c = self.storage.conn()?;
        Ok(c.query_opt(
            "SELECT latest_user_round FROM service_thread_owners WHERE thread_id=$1",
            &[&session],
        )?
        .map(|row| row.get(0))
        .unwrap_or(0))
    }

    pub(crate) fn list_thread_changes(
        &self,
        user: &str,
        session: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.check_owner(user, session)?;
        self.list_thread_changes_by_session(session, after, limit)
    }
    pub(crate) fn list_thread_changes_by_session(
        &self,
        session: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        super::thread_display::ensure_ready(self.storage, session)?;
        let mut conn = self.storage.conn()?;
        let mut tx = conn.transaction()?;
        tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")?;
        let latest = tx
            .query_opt(
                "SELECT history_revision FROM service_threads WHERE thread_id=$1",
                &[&session],
            )?
            .map(|row| row.get::<_, i64>(0))
            .unwrap_or(0);
        if after > latest {
            return Ok(vec![
                json!({"change_type":"snapshot_required","change_seq":latest}),
            ]);
        }
        let rows = tx.query(
            "WITH candidates AS MATERIALIZED (
            SELECT r.ordinal,octet_length(r.record_json) AS bytes FROM service_thread_records r
            WHERE r.thread_id=$1 AND r.ordinal>=$2 AND
              (r.record_type IN ('turn_started','turn_completed','turn_aborted') OR
               (r.record_type='item_completed' AND EXISTS(SELECT 1 FROM service_thread_items i
                 WHERE i.thread_id=r.thread_id AND i.ordinal=r.ordinal AND i.visible)))
            ORDER BY r.ordinal LIMIT $3
        ), sized AS (SELECT ordinal,SUM(bytes) OVER (ORDER BY ordinal) AS bytes,
            ROW_NUMBER() OVER (ORDER BY ordinal) AS n FROM candidates)
        SELECT r.ordinal,r.record_json FROM sized s JOIN service_thread_records r
          ON r.thread_id=$1 AND r.ordinal=s.ordinal WHERE s.bytes<=16777216 OR s.n=1
        ORDER BY r.ordinal",
            &[&session, &after.max(0), &limit.clamp(1, 200)],
        )?;
        let mut changes = Vec::with_capacity(rows.len());
        let mut bytes = 0usize;
        for row in rows {
            let ordinal: i64 = row.get(0);
            let raw: String = row.get(1);
            bytes = bytes.saturating_add(raw.len());
            if bytes > 16 * 1024 * 1024 && !changes.is_empty() {
                break;
            }
            let record: RolloutRecord = serde_json::from_str(&raw)?;
            let (kind, turn, item, payload) = match record.record {
                CanonicalRecord::Event(CanonicalEvent::ItemCompleted(item)) => {
                    let turn = item.turn_id.to_string();
                    let id = item.item_id.to_string();
                    let mut payload = Self::item_wire(&item, ordinal.max(0) as u64);
                    payload["session_id"] = json!(session);
                    Self::redact_item(&mut payload, false);
                    ("item_upsert", turn, Some(id), payload)
                }
                CanonicalRecord::Event(CanonicalEvent::TurnStarted {
                    turn_id,
                    root_turn_id,
                    started_at_ms,
                }) => {
                    let turn = turn_id.to_string();
                    (
                        "turn_upsert",
                        turn.clone(),
                        None,
                        json!({"turn_id":turn,"root_turn_id":root_turn_id,"status":"running","started_time":started_at_ms as f64/1000.0}),
                    )
                }
                CanonicalRecord::Event(CanonicalEvent::TurnCompleted {
                    turn_id,
                    completed_at_ms,
                }) => {
                    let turn = turn_id.to_string();
                    (
                        "turn_status",
                        turn.clone(),
                        None,
                        json!({"turn_id":turn,"status":"completed","completed_time":completed_at_ms as f64/1000.0}),
                    )
                }
                CanonicalRecord::Event(CanonicalEvent::TurnAborted {
                    turn_id,
                    aborted_at_ms,
                    error,
                }) => {
                    let turn = turn_id.to_string();
                    let status = error
                        .as_ref()
                        .and_then(|v| v["status"].as_str())
                        .unwrap_or("aborted");
                    (
                        "turn_status",
                        turn.clone(),
                        None,
                        json!({"turn_id":turn,"status":status,"completed_time":aborted_at_ms as f64/1000.0}),
                    )
                }
                _ => continue,
            };
            changes.push(json!({"change_seq":ordinal+1,"cursor":ordinal+1,"change_type":kind,"turn_id":turn,"item_id":item,"revision":1,"payload":payload}));
        }
        Ok(changes)
    }
    pub(crate) fn latest_thread_change_seq_by_session(&self, session: &str) -> Result<i64> {
        let mut c = self.storage.conn()?;
        Ok(c.query_opt(
            "SELECT history_revision FROM service_threads WHERE thread_id=$1",
            &[&session],
        )?
        .map(|r| r.get(0))
        .unwrap_or(0))
    }

    pub(crate) fn list_thread_turns(
        &self,
        user: &str,
        session: &str,
        before: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.check_owner(user, session)?;
        let mut c = self.storage.conn()?;
        let rows=c.query("WITH candidates AS MATERIALIZED (
            SELECT root.turn_id,root.user_round,
              COALESCE(octet_length(root.input_json),0)+COALESCE(octet_length(root.control_json),0)+2048 AS bytes
            FROM service_thread_admissions root WHERE root.thread_id=$1 AND root.turn_id=root.root_turn_id
              AND ($2::bigint IS NULL OR root.user_round<$2) ORDER BY root.user_round DESC LIMIT $3
        ), sized AS (SELECT turn_id,user_round,SUM(bytes) OVER(ORDER BY user_round DESC) AS bytes,
            ROW_NUMBER() OVER(ORDER BY user_round DESC) AS n FROM candidates)
        SELECT root.turn_id,root.root_turn_id,root.user_round,head.status,root.client_message_id,
            root.created_at_ms,head.updated_at_ms,root.input_json,head.control_json,head.turn_id AS execution_turn_id
        FROM sized s JOIN service_thread_admissions root ON root.thread_id=$1 AND root.turn_id=s.turn_id
        JOIN LATERAL (SELECT turn_id,status,updated_at_ms,control_json FROM service_thread_admissions
            WHERE thread_id=$1 AND root_turn_id=root.root_turn_id ORDER BY created_at_ms DESC,turn_id DESC LIMIT 1) head ON TRUE
        WHERE s.bytes<=16777216 OR s.n=1 ORDER BY root.user_round DESC", &[&session,&before,&limit.clamp(1,201)])?;
        rows.into_iter().map(|r| {
            Ok(json!({"turn_id":r.get::<_,String>(0),"root_turn_id":r.get::<_,String>(1),
                "execution_turn_id":r.get::<_,String>(9),"user_turn_index":r.get::<_,i64>(2),
                "status":r.get::<_,String>(3),"client_message_id":r.get::<_,Option<String>>(4),
                "created_time":r.get::<_,i64>(5) as f64/1000.0,"updated_time":r.get::<_,i64>(6) as f64/1000.0,
                "input":r.get::<_,Option<String>>(7).as_deref().map(serde_json::from_str::<Value>).transpose()?,
                "control":r.get::<_,Option<String>>(8).as_deref().map(serde_json::from_str::<Value>).transpose()?}))
        }).collect()
    }

    pub(crate) fn get_thread_turn(
        &self,
        user: &str,
        session: &str,
        turn: &str,
        after: i64,
        limit: i64,
        internal: bool,
    ) -> Result<Option<Value>> {
        self.get_thread_turn_page(user, session, turn, None, Some(after), limit, internal)
    }
    pub(crate) fn get_thread_turn_page(
        &self,
        user: &str,
        session: &str,
        turn: &str,
        before: Option<i64>,
        after: Option<i64>,
        limit: i64,
        internal: bool,
    ) -> Result<Option<Value>> {
        self.check_owner(user, session)?;
        anyhow::ensure!(
            before.is_none() || after.is_none(),
            "before and after are mutually exclusive"
        );
        super::thread_display::ensure_ready(self.storage, session)?;
        let mut conn = self.storage.conn()?;
        let mut c = conn.transaction()?;
        c.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")?;
        let row=c.query_opt("SELECT turn_id,root_turn_id,user_round,status FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=$2", &[&session,&turn])?;
        let Some(r) = row else { return Ok(None) };
        let root: String = r.get(1);
        let limit = limit.clamp(1, 500) as usize;
        let forward = after.is_some() || before.is_none();
        let mut items = Self::items_in_tx(
            &mut c,
            session,
            Some(&root),
            before,
            after,
            (limit + 1) as i64,
            internal,
            false,
            !forward,
        )?;
        let extra = items.len() > limit;
        items.truncate(limit);
        items.sort_by_key(|i| i["item_index"].as_i64().unwrap_or(0));
        let first = items.first().and_then(|i| i["item_index"].as_i64());
        let last = items.last().and_then(|i| i["item_index"].as_i64());
        let older = if let Some(edge) = first {
            c.query_one("SELECT EXISTS(SELECT 1 FROM service_thread_items WHERE thread_id=$1 AND root_turn_id=$2 AND ordinal<$3 AND ($4 OR visible))",&[&session,&root,&edge,&internal])?.get::<_,bool>(0)
        } else {
            false
        };
        let newer = if let Some(edge) = last {
            c.query_one("SELECT EXISTS(SELECT 1 FROM service_thread_items WHERE thread_id=$1 AND root_turn_id=$2 AND ordinal>$3 AND ($4 OR visible))",&[&session,&root,&edge,&internal])?.get::<_,bool>(0)
        } else {
            false
        };
        Ok(Some(
            json!({"turn_id":r.get::<_,String>(0),"root_turn_id":root,"user_turn_index":r.get::<_,i64>(2),"status":r.get::<_,String>(3),
                "items":items,"next_before":first,"next_after":last,"has_older":older,"has_newer":newer,
                "has_more":extra || if forward {newer}else{older}}),
        ))
    }

    pub(crate) fn get_thread_log_counts(
        &self,
        user: &str,
        session: &str,
        internal: bool,
    ) -> Result<(i64, i64)> {
        self.check_owner(user, session)?;
        super::thread_display::ensure_ready(self.storage, session)?;
        let mut c = self.storage.conn()?;
        let turns:i64=c.query_one("SELECT COUNT(*) FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=root_turn_id", &[&session])?.get(0);
        let items: i64 = c
            .query_one(
                "SELECT COUNT(*) FROM service_thread_items WHERE thread_id=$1 AND ($2 OR visible)",
                &[&session, &internal],
            )?
            .get(0);
        Ok((turns, items))
    }

    pub(crate) fn delete_thread_log_by_session(&self, user: &str, session: &str) -> Result<i64> {
        self.check_owner(user, session)?;
        let count = self.get_thread_log_counts(user, session, true)?.1;
        crate::storage::canonical_thread_log::wait(
            self.store.delete_thread(&ThreadId::new(session)?),
        )??;
        Ok(count)
    }

    pub(crate) fn set_thread_item_feedback(
        &self,
        user: &str,
        session: &str,
        item: &str,
        vote: &str,
    ) -> Result<Option<Value>> {
        self.get_thread_item(user, session, item, false)?
            .ok_or_else(|| anyhow!("item not found"))?;
        anyhow::ensure!(vote.len() <= 64, "feedback vote exceeds byte limit");
        let mut conn = self.storage.conn()?;
        let mut tx = conn.transaction()?;
        PostgresAdmissionStore::lock_thread(&mut tx, session)?;
        anyhow::ensure!(
            PostgresAdmissionStore::owner_in_tx(&mut tx, user, session)?.is_some(),
            "thread owner is missing"
        );
        anyhow::ensure!(
            tx.query_opt(
                "SELECT 1 FROM service_thread_items WHERE thread_id=$1 AND item_id=$2 AND visible",
                &[&session, &item]
            )?
            .is_some(),
            "item not found"
        );
        let changed = tx.execute(
            "INSERT INTO meta(key,value,updated_time) VALUES($1,$2,$3) ON CONFLICT(key) DO NOTHING",
            &[
                &format!("thread_feedback:{session}:{item}"),
                &vote,
                &PostgresStorage::now_ts(),
            ],
        )? > 0;
        tx.commit()?;
        Ok(changed.then(|| json!({"item_id":item,"vote":vote})))
    }

    pub(crate) fn pending_thread_view_rebuilds(&self) -> Result<Vec<(String, String, i64)>> {
        Ok(vec![])
    }
    pub(crate) fn load_subagent_context(
        &self,
        user: &str,
        session: &str,
        turns: i64,
    ) -> Result<Vec<Value>> {
        self.check_owner(user, session)?;
        super::thread_display::ensure_ready(self.storage, session)?;
        let mut conn = self.storage.conn()?;
        let mut tx = conn.transaction()?;
        tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")?;
        let rows=tx.query("SELECT ordinal,preview_json FROM service_thread_items WHERE thread_id=$1 AND visible
            AND root_turn_id IN (SELECT root_turn_id FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=root_turn_id
                ORDER BY user_round DESC LIMIT $2)
            AND preview_json::jsonb->>'kind' IN ('user_message','assistant_message','agent_message')
            ORDER BY ordinal DESC LIMIT 257", &[&session,&turns.clamp(1,16)])?;
        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            let item: CanonicalItem = serde_json::from_str(&row.get::<_, String>(1))?;
            let mut item = Self::item_wire(&item, row.get::<_, i64>(0).max(0) as u64);
            Self::redact_item(&mut item, false);
            let mut body = Self::message_payload(item)?;
            if let Some(content) = body["content"].as_str() {
                let truncated = body["_content_bytes"]
                    .as_u64()
                    .is_some_and(|bytes| bytes > content.len() as u64);
                let preview = wunder_core::thread_view::preview(content, 8192);
                body["content"] = json!(preview);
                if truncated {
                    body["truncated"] = json!(true);
                }
            }
            messages.push(body);
        }
        messages.reverse();
        Ok(messages)
    }
    pub(crate) fn thread_snapshot(&self, user: &str, session: &str) -> Result<Value> {
        self.thread_snapshot_window(user, session, &ThreadSnapshotWindow::default())
    }
    pub(crate) fn thread_snapshot_window(
        &self,
        user: &str,
        session: &str,
        w: &ThreadSnapshotWindow,
    ) -> Result<Value> {
        self.check_owner(user, session)?;
        super::thread_display::ensure_ready(self.storage, session)?;
        let mut conn = self.storage.conn()?;
        let mut tx = conn.transaction()?;
        tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")?;
        let seq = tx
            .query_opt(
                "SELECT history_revision FROM service_threads WHERE thread_id=$1",
                &[&session],
            )?
            .map(|r| r.get::<_, i64>(0))
            .unwrap_or(0);
        let max_turns = w.max_turns.clamp(1, 200);
        let max_items = w.max_items.clamp(1, 500);
        let rows = tx.query("SELECT root.turn_id,root.root_turn_id,root.user_round,head.status,root.client_message_id,root.created_at_ms,head.updated_at_ms
            FROM (SELECT * FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=root_turn_id
                AND ($2::bigint IS NULL OR user_round<$2) ORDER BY user_round DESC LIMIT $3) root
            JOIN LATERAL (SELECT status,updated_at_ms FROM service_thread_admissions WHERE thread_id=$1 AND root_turn_id=root.root_turn_id
                ORDER BY created_at_ms DESC,turn_id DESC LIMIT 1) head ON TRUE
            ORDER BY root.user_round DESC", &[&session,&w.before_turn_index,&((max_turns+1) as i64)])?;
        let mut more = rows.len() > max_turns;
        let mut turns = rows.into_iter().take(max_turns).map(|r|json!({"turn_id":r.get::<_,String>(0),"root_turn_id":r.get::<_,String>(1),"user_turn_index":r.get::<_,i64>(2),"status":r.get::<_,String>(3),"client_message_id":r.get::<_,Option<String>>(4),"created_time":r.get::<_,i64>(5) as f64/1000.0,"updated_time":r.get::<_,i64>(6) as f64/1000.0})).collect::<Vec<_>>();
        let mut items = Vec::new();
        let mut bytes = 0usize;
        for turn in &turns {
            let remaining = max_items.saturating_sub(items.len());
            if remaining == 0 {
                more = true;
                break;
            }
            let root = turn["root_turn_id"].as_str().unwrap_or_default();
            let mut page = Self::items_in_tx(
                &mut tx,
                session,
                Some(root),
                None,
                None,
                remaining as i64 + 1,
                false,
                false,
                true,
            )?;
            if page.len() > remaining {
                page.truncate(remaining);
                more = true;
            }
            if let Some(first) = page.last().and_then(|item| item["item_index"].as_i64()) {
                more |= tx.query_one("SELECT EXISTS(SELECT 1 FROM service_thread_items WHERE thread_id=$1 AND root_turn_id=$2 AND ordinal<$3 AND visible)", &[&session,&root,&first])?.get::<_,bool>(0);
            }
            for item in page {
                let size = serde_json::to_vec(&item)?.len();
                if bytes.saturating_add(size) > 16 * 1024 * 1024 && !items.is_empty() {
                    more = true;
                    break;
                }
                bytes = bytes.saturating_add(size);
                items.push(item);
            }
            if bytes >= 16 * 1024 * 1024 {
                break;
            }
        }
        turns.reverse();
        items.sort_by_key(|i| i["item_index"].as_i64().unwrap_or(0));
        let total: i64 = tx
            .query_one(
                "SELECT COUNT(*) FROM service_thread_items WHERE thread_id=$1 AND visible",
                &[&session],
            )?
            .get(0);
        more |= total > items.len() as i64;
        Ok(
            json!({"cursor":seq,"window_start_turn_index":turns.first().map(|t|&t["user_turn_index"]),"window_end_turn_index":turns.last().map(|t|&t["user_turn_index"]),"turns":turns,"item_total":total,"items":items,"truncated":more,"has_older":more}),
        )
    }
    pub(crate) fn fork_thread_log(
        &self,
        user: &str,
        source: &str,
        target: &str,
        through_round: i64,
    ) -> Result<()> {
        super::thread_fork_store::fork(self.storage, user, source, target, through_round)
    }
    pub(crate) fn thread_turn_tails(
        &self,
        user: &str,
        sessions: &[String],
    ) -> Result<HashMap<String, ThreadTurnTail>> {
        if user.trim().is_empty() || sessions.is_empty() {
            return Ok(HashMap::new());
        }
        let sessions = sessions
            .iter()
            .take(1000)
            .filter(|id| !id.trim().is_empty())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut conn = self.storage.conn()?;
        // Ownership and each per-thread tail share one statement snapshot.
        // An active older turn stays authoritative when newer input is queued.
        let rows = conn.query(
            "SELECT owner.thread_id,owner.latest_user_round,COALESCE(t.history_revision,0) AS revision,
                latest.status AS latest_status,head.turn_id AS head_turn,head.status AS head_status,
                activity.preview_json
             FROM service_thread_owners owner
             LEFT JOIN service_threads t ON t.thread_id=owner.thread_id
             LEFT JOIN LATERAL (SELECT status FROM service_thread_admissions a
                WHERE a.thread_id=owner.thread_id ORDER BY user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1) latest ON TRUE
             LEFT JOIN LATERAL (SELECT turn_id,status FROM (
                (SELECT turn_id,status,user_round,created_at_ms,1 AS priority FROM service_thread_admissions
                 WHERE thread_id=owner.thread_id AND status='running' ORDER BY user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1)
                UNION ALL
                (SELECT turn_id,status,user_round,created_at_ms,1 AS priority FROM service_thread_admissions
                 WHERE thread_id=owner.thread_id AND status='waiting_input' ORDER BY user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1)
                UNION ALL
                (SELECT turn_id,status,user_round,created_at_ms,1 AS priority FROM service_thread_admissions
                 WHERE thread_id=owner.thread_id AND status='waiting_approval' ORDER BY user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1)
                UNION ALL
                (SELECT turn_id,status,user_round,created_at_ms,0 AS priority FROM service_thread_admissions
                 WHERE thread_id=owner.thread_id ORDER BY user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1)
             ) h ORDER BY priority DESC,user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1) head ON TRUE
             LEFT JOIN LATERAL (SELECT preview_json FROM service_thread_items
                WHERE thread_id=owner.thread_id AND visible ORDER BY ordinal DESC LIMIT 1) activity ON TRUE
             WHERE owner.user_id=$1 AND owner.thread_id=ANY($2)",
            &[&user, &sessions],
        )?;
        let mut out = HashMap::with_capacity(rows.len());
        for row in rows {
            let session: String = row.get("thread_id");
            let revision: i64 = row.get("revision");
            let runtime_head = row.get::<_, Option<String>>("head_status").map(|status| {
                let active = crate::storage::thread_view_projection::is_running(&status) != 0;
                wunder_core::thread_view::ThreadExecutionHead {
                    runtime_status: status,
                    active_turn_id: if active {
                        row.get::<_, Option<String>>("head_turn")
                            .unwrap_or_default()
                    } else {
                        String::new()
                    },
                    epoch: 1,
                    view_seq: revision,
                }
            });
            let last_activity = row
                .get::<_, Option<String>>("preview_json")
                .map(|raw| {
                    let item: CanonicalItem = serde_json::from_str(&raw)?;
                    let body = item
                        .payload
                        .get("payload")
                        .filter(|_| item.payload.get("item_id").is_some())
                        .unwrap_or(&item.payload);
                    let text = wunder_core::thread_view::message_content_text(&body["content"])
                        .or_else(|| {
                            body.get("tool")
                                .and_then(Value::as_str)
                                .map(|tool| format!("tool: {tool}"))
                        })
                        .unwrap_or_default();
                    Ok::<_, anyhow::Error>(wunder_core::thread_view::preview(&text, 256))
                })
                .transpose()?
                .unwrap_or_default();
            out.insert(
                session.clone(),
                ThreadTurnTail {
                    session_id: session,
                    user_round: row.get("latest_user_round"),
                    turn_status: row
                        .get::<_, Option<String>>("latest_status")
                        .unwrap_or_default(),
                    runtime_head,
                    last_activity,
                    change_seq: revision,
                },
            );
        }
        Ok(out)
    }
}
