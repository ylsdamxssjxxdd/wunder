//! Synchronous application facade over the canonical rollout projection.
//!
//! The application APIs keep their transport DTOs, but never own a second
//! history database. Reads are bounded SQLite snapshots; mutations enter the
//! same ThreadStore writer used by the executor.
use super::thread_history_body::{ItemBodyReader, ItemBodyRef, ITEM_BODY_COLUMNS};
use super::{
    thread_history::*, thread_store::LocalThreadStore, MetaStore, ThreadLogStore, ThreadTurnTail,
};
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{collections::HashMap, future::Future, time::Duration};
use wunder_core::{storage_backend::ThreadSnapshotWindow, thread_store::*, thread_view::*};

pub(crate) const VISIBLE: &str = "visible=1";

pub(crate) struct CanonicalThreadLog<'a> {
    threads: &'a LocalThreadStore,
    metadata: &'a dyn MetaStore,
}

pub(crate) fn wait<T: Send>(future: impl Future<Output = T> + Send) -> Result<T> {
    let runtime = super::runtime::runtime()?;
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            Ok(tokio::task::block_in_place(|| runtime.block_on(future)))
        }
        Ok(_) => std::thread::scope(|scope| {
            scope
                .spawn(|| runtime.block_on(future))
                .join()
                .map_err(|_| anyhow!("canonical storage worker failed"))
        }),
        Err(_) => Ok(runtime.block_on(future)),
    }
}

impl<'a> CanonicalThreadLog<'a> {
    pub(crate) fn new(threads: &'a LocalThreadStore, metadata: &'a dyn MetaStore) -> Self {
        Self { threads, metadata }
    }
    fn admissions(&self) -> Result<super::thread_admission::TurnAdmissions> {
        self.threads.admission_store()
    }
    fn execution_head(
        conn: &Connection,
        session: &str,
        seq: i64,
    ) -> Result<Option<ThreadExecutionHead>> {
        let active: Option<String> = conn.query_row(
            "SELECT turn_id FROM queue.canonical_turn_admissions WHERE thread_id=?1 AND status IN ('running','waiting_input','waiting_approval') ORDER BY user_round DESC,created_at_ms DESC LIMIT 1",
            [session], |row| row.get(0),
        ).optional()?;
        let turn = if let Some(id) = active {
            Self::turn(conn, session, &id)?
        } else {
            Self::turns(conn, session, None, None, 1)?.pop()
        };
        let Some(turn) = turn else { return Ok(None) };
        let root = turn["root_turn_id"]
            .as_str()
            .or_else(|| turn["turn_id"].as_str())
            .unwrap_or("");
        let id: String = conn.query_row(
            "SELECT turn_id FROM queue.canonical_turn_admissions WHERE thread_id=?1 AND root_turn_id=?2 ORDER BY created_at_ms DESC,rowid DESC LIMIT 1",
            params![session, root], |row| row.get(0),
        )?;
        let current = Self::turn(conn, session, &id)?.unwrap_or(turn);
        let status = current["status"].as_str().unwrap_or("").to_owned();
        Ok(Some(ThreadExecutionHead {
            active_turn_id: if super::thread_view_projection::is_running(&status) != 0 {
                id.clone()
            } else {
                String::new()
            },
            status_turn_id: id,
            runtime_status: status,
            epoch: 1,
            view_seq: seq,
        }))
    }
    fn check(&self, user: &str, session: &str) -> Result<()> {
        self.admissions()?.check_owner(user, session)
    }
    fn connection(&self, session: Option<&str>) -> Result<Connection> {
        self.connection_for(session, None)
    }
    fn connection_for(&self, session: Option<&str>, user: Option<&str>) -> Result<Connection> {
        let root = self.threads.root();
        let conn = Connection::open_with_flags(
            root.join("thread_history_1.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::from_secs(2))?;
        super::thread_history::prepare_item_view(&conn)?;
        conn.execute(
            "ATTACH DATABASE ?1 AS state",
            [root.join("state_1.sqlite").to_string_lossy().as_ref()],
        )?;
        conn.execute(
            "ATTACH DATABASE ?1 AS queue",
            [root.join("queue_1.sqlite").to_string_lossy().as_ref()],
        )?;
        if let (Some(session), Some(user)) = (session, user) {
            let owner: Option<String> = conn
                .query_row(
                    "SELECT user_id FROM queue.canonical_thread_owners WHERE thread_id=?1",
                    [session],
                    |row| row.get(0),
                )
                .optional()?;
            if user.trim().is_empty()
                || session.trim().is_empty()
                || owner.as_deref().is_some_and(|owner| owner != user)
                || (owner.is_none() && Self::rollout(&conn, session)?.is_some())
            {
                bail!("thread owner mismatch or missing");
            }
        }
        let mut prepared_segments = None;
        if let Some(session) = session {
            if Self::rollout(&conn, session)?.is_some() {
                let deleting: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM state.canonical_thread_deletions WHERE thread_id=?1)",
                    [session], |row| row.get(0),
                )?;
                if deleting {
                    bail!("thread deletion is pending; retry deletion to finish reclamation");
                }
                let thread = ThreadId::new(session)?;
                let pending: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM state.canonical_pending_queue_reverts WHERE thread_id=?1)
                     OR EXISTS(SELECT 1 FROM queue.canonical_thread_owners WHERE thread_id=?1 AND mutation_pending IS NOT NULL)",
                    [session], |row| row.get(0),
                )?;
                if pending {
                    wait(self.threads.ensure_history(&thread))??;
                    prepared_segments =
                        Some(super::thread_store::resolve_lineage(&conn, root, &thread)?);
                } else {
                    let segments = super::thread_store::resolve_lineage(&conn, root, &thread)?;
                    wait(
                        self.threads
                            .ensure_history_segments(&thread, segments.clone()),
                    )??;
                    prepared_segments = Some(segments);
                }
            }
        }
        conn.execute_batch("BEGIN")?;
        if let Some(session) = session {
            if let Some(current) = Self::rollout(&conn, session)? {
                let segments = prepared_segments
                    .ok_or_else(|| anyhow!("history_changed: retry this history snapshot"))?;
                if segments.last().map(|s| s.rollout_id.as_str()) != Some(current.as_str()) {
                    bail!("history_changed: retry this history snapshot");
                }
                let segments = segments
                    .into_iter()
                    .map(|s| HistorySegment {
                        rollout_id: s.rollout_id.to_string(),
                        start_ordinal: s.start_ordinal,
                        end_ordinal: s.end.map(|e| e.end_ordinal_exclusive),
                    })
                    .collect::<Vec<_>>();
                ThreadHistoryStore::apply_scope(&conn, &current, &segments)?;
            }
        }
        Ok(conn)
    }
    fn rollout(conn: &Connection, session: &str) -> Result<Option<String>> {
        Ok(conn
            .query_row(
                "SELECT rollout_id FROM state.canonical_threads WHERE thread_id=?1",
                [session],
                |r| r.get(0),
            )
            .optional()?)
    }
    fn position(conn: &Connection, rollout: Option<&str>) -> Result<i64> {
        Ok(conn.query_row("SELECT next_rollout_ordinal FROM thread_history_projection_state WHERE rollout_id=?1", [rollout], |r| r.get(0)).optional()?.unwrap_or(0))
    }
    fn item(raw: String, ordinal: i64, session: &str, internal: bool) -> Result<Value> {
        let item: CanonicalItem = serde_json::from_str(&raw)?;
        let wrapped = item
            .payload
            .get("item_id")
            .and_then(Value::as_str)
            .is_some()
            && item.payload.get("payload").is_some();
        let mut payload = item
            .payload
            .get("payload")
            .filter(|v| wrapped && v.is_object())
            .cloned()
            .unwrap_or(item.payload.clone());
        if !payload.is_object() {
            payload = json!({"content":payload});
        }
        let kind = item
            .payload
            .get("kind")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or(
                serde_json::to_value(item.kind)?
                    .as_str()
                    .unwrap_or("configuration")
                    .to_owned(),
            );
        let visibility = item
            .payload
            .get("visibility")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if !internal {
            if let Some(map) = payload.as_object_mut() {
                map.remove("model_content");
                map.remove("config_overrides");
            }
        }
        if let Some(output) = item.display_output {
            payload["display_output"] = json!(output);
        }
        if item.display_output_truncated {
            payload["display_output_truncated"] = json!(true);
        }
        let status = item
            .payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("completed");
        Ok(
            json!({"session_id":session,"turn_id":item.turn_id,"root_turn_id":item.root_turn_id.as_ref().unwrap_or(&item.turn_id),
            "item_id":item.item_id,"item_index":ordinal,"created_seq":ordinal+1,"revision":1,"kind":kind,
            "status":status,"visibility":visibility,"payload":payload,"created_time":item.created_at_ms as f64/1000.0,
            "updated_time":item.created_at_ms as f64/1000.0}),
        )
    }
    fn items(
        conn: &Connection,
        session: &str,
        turn: Option<&str>,
        before: Option<i64>,
        after: Option<i64>,
        limit: i64,
        internal: bool,
        messages: bool,
    ) -> Result<Vec<Value>> {
        Self::item_page(
            conn, session, turn, before, after, limit, internal, messages, false,
        )
    }
    fn item_page(
        conn: &Connection,
        session: &str,
        turn: Option<&str>,
        before: Option<i64>,
        after: Option<i64>,
        limit: i64,
        internal: bool,
        messages: bool,
        summary: bool,
    ) -> Result<Vec<Value>> {
        let Some(rollout) = Self::rollout(conn, session)? else {
            return Ok(vec![]);
        };
        let order = if after.is_some() { "ASC" } else { "DESC" };
        let root = if turn.is_some() {
            "AND COALESCE(root_turn_id,turn_id)=?2"
        } else {
            "AND ?2 IS NULL"
        };
        let message_filter = if !messages {
            String::new()
        } else if internal {
            "AND message_context=1".to_owned()
        } else {
            "AND item_type IN ('user_message','assistant_message','agent_message')".to_owned()
        };
        let sql=format!("SELECT preview_json,rollout_ordinal,item_id,{ITEM_BODY_COLUMNS} FROM thread_items WHERE rollout_id=?1 {root} AND (?3 IS NULL OR rollout_ordinal<?3) AND (?4 IS NULL OR rollout_ordinal>?4) {} {message_filter} ORDER BY rollout_ordinal {order} LIMIT ?5",
            if internal {String::new()}else{format!("AND {VISIBLE}")});
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![rollout, turn, before, after, limit.clamp(1, 501)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    ItemBodyRef::from_row(row, 3)?,
                ))
            },
        )?;
        let mut bytes = 0;
        let mut output = Vec::new();
        let mut bodies = ItemBodyReader::for_connection(conn)?;
        for row in rows {
            let (preview, ordinal, id, reference) = row?;
            let size = if summary {
                preview.len()
            } else {
                reference.item_bytes
            };
            if bytes + size > 16 * 1024 * 1024 && !output.is_empty() {
                break;
            }
            bytes += size;
            let raw = if summary {
                preview
            } else {
                serde_json::to_string(&bodies.read(&reference, u64::try_from(ordinal)?, &id)?)?
            };
            output.push(Self::item(raw, ordinal, session, internal)?);
        }
        Ok(output)
    }
    fn turn(conn: &Connection, session: &str, turn: &str) -> Result<Option<Value>> {
        let row=conn.query_row("SELECT a.root_turn_id,a.user_round,CASE WHEN t.status='aborted' THEN COALESCE(json_extract(t.error_json,'$.status'),a.status,'aborted') WHEN t.status IN ('completed','failed') THEN t.status ELSE a.status END,a.client_message_id,a.created_at_ms,a.updated_at_ms,t.error_json,a.control_json FROM queue.canonical_turn_admissions a LEFT JOIN state.canonical_threads d ON d.thread_id=a.thread_id LEFT JOIN thread_turns t ON t.rollout_id=d.rollout_id AND t.turn_id=a.turn_id WHERE a.thread_id=?1 AND a.turn_id=?2",params![session,turn],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,Option<String>>(7)?))).optional()?;
        row.map(|(root,round,status,client,created,updated,error,control)|->Result<Value>{
            let control:Value=control.as_deref().map(serde_json::from_str).transpose()?.unwrap_or_else(||json!({}));
            Ok(json!({"turn_id":turn,"root_turn_id":root,"user_turn_index":round,"user_round":round,"status":status,"client_message_id":client,"summary":"",
                "payload":error.as_deref().map(serde_json::from_str::<Value>).transpose()?.unwrap_or_else(||json!({})),
                "queued_input":if status=="queued"{control["queued_input"].clone()}else{Value::Null},"stop_reason":control["stop_reason"],
                "trigger_kind":if root==turn{"user"}else{"continuation"},"created_time":created as f64/1000.0,"updated_time":updated as f64/1000.0}))
        }).transpose()
    }
    fn turns(
        conn: &Connection,
        session: &str,
        before: Option<i64>,
        after: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        let order = if after.is_some() { "ASC" } else { "DESC" };
        let mut stmt=conn.prepare(&format!("SELECT turn_id FROM queue.canonical_turn_admissions WHERE thread_id=?1 AND turn_id=root_turn_id AND (?2 IS NULL OR user_round<?2) AND (?3 IS NULL OR user_round>?3) ORDER BY user_round {order} LIMIT ?4"))?;
        let ids = stmt
            .query_map(params![session, before, after, limit.clamp(1, 201)], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter()
            .map(|id| {
                Self::turn(conn, session, &id)?
                    .ok_or_else(|| anyhow!("turn disappeared from snapshot"))
            })
            .collect()
    }
    fn chat_row(
        &self,
        conn: &Connection,
        session: &str,
        turn: &Value,
        seq: i64,
    ) -> Result<ChatViewRow> {
        use super::thread_view_projection as p;
        let root = turn["root_turn_id"].as_str().unwrap_or_default();
        let round = turn["user_turn_index"].as_i64().unwrap_or(0);
        let mut row = p::new_chat(
            root,
            round,
            turn["client_message_id"].as_str().map(str::to_owned),
            turn["status"].as_str().unwrap_or(""),
            turn["created_time"].as_f64().unwrap_or(0.0),
            turn["updated_time"].as_f64().unwrap_or(0.0),
        );
        let latest = conn.query_row("SELECT turn_id FROM queue.canonical_turn_admissions WHERE thread_id=?1 AND root_turn_id=?2 ORDER BY created_at_ms DESC,rowid DESC LIMIT 1",params![session,root],|r|r.get::<_,String>(0)).optional()?;
        let head = latest
            .as_deref()
            .map(|id| Self::turn(conn, session, id))
            .transpose()?
            .flatten();
        let head = head.as_ref().unwrap_or(turn);
        row.status = head["status"].as_str().unwrap_or("").into();
        row.execution_turn_id = head["turn_id"].as_str().unwrap_or(root).into();
        let control:Option<String>=conn.query_row("SELECT control_json FROM queue.canonical_turn_admissions WHERE thread_id=?1 AND turn_id=?2",params![session,row.execution_turn_id],|r|r.get(0)).optional()?.flatten();
        if let Some(control) = control {
            let control: Value = serde_json::from_str(&control)?;
            if row.status == "queued" {
                row.queued_input = control
                    .get("queued_input")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()?;
            }
            row.stop_reason = control["stop_reason"].as_str().map(str::to_owned);
        }
        row.revision = seq;
        // Accepted input remains queue control state until execution starts.
        let pending: Option<String> = conn.query_row("SELECT input_json FROM queue.canonical_turn_admissions WHERE thread_id=?1 AND turn_id=?2", params![session,root],|r|r.get(0)).optional()?.flatten();
        if let Some(pending) = pending {
            let body: Value = serde_json::from_str(&pending)?;
            let text = message_content_text(&body["content"]).unwrap_or_default();
            row.user = p::message(&format!("{root}:user"), &text, 1);
            row.user.body = None;
            if let Some(a) = body["attachments"].as_array() {
                row.user.attachments = p::user_attachments(a);
            }
        }
        let Some(rollout) = Self::rollout(conn, session)? else {
            return Ok(row);
        };
        let summary = super::thread_chat_summary::read(conn, &rollout, root)?;
        for (kind, id) in [
            ("user_message", summary.user_item_id.as_deref()),
            ("assistant_message", summary.assistant_item_id.as_deref()),
            ("question_panel", summary.question_panel_item_id.as_deref()),
            ("plan", summary.plan_item_id.as_deref()),
        ] {
            let Some(id) = id else { continue };
            let item = conn
                .query_row("SELECT preview_json,rollout_ordinal FROM thread_items WHERE rollout_id=?1 AND item_id=?2", params![rollout, id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                })
                .optional()?;
            let Some((raw, ordinal)) = item else { continue };
            let item = Self::item(raw, ordinal, session, false)?;
            let body = &item["payload"];
            if kind == "question_panel" {
                row.assistant.question_panel = p::extract_question_panel(body);
                continue;
            }
            if kind == "plan" {
                row.plan = p::plan(body, item["item_id"].as_str().unwrap_or(""), 1, ordinal + 1);
                continue;
            }
            let text = message_content_text(&body["content"]).unwrap_or_default();
            let mut message = p::message(item["item_id"].as_str().unwrap_or(""), &text, 1);
            if let Some(body_ref) = message.body.as_mut() {
                body_ref.bytes =
                    body["_content_bytes"].as_u64().unwrap_or(text.len() as u64) as usize;
            }
            message.created_seq = ordinal + 1;
            message.durable_seq = seq;
            message.content_offset = body["_content_utf16"]
                .as_u64()
                .unwrap_or(message.content.encode_utf16().count() as u64)
                as usize;
            if kind == "user_message" {
                if let Some(a) = body["attachments"].as_array() {
                    message.attachments = p::user_attachments(a);
                }
                row.user = message;
            } else {
                p::set_reasoning(&mut message, body);
                message.reasoning_offset = body["_reasoning_utf16"]
                    .as_u64()
                    .unwrap_or(message.reasoning_offset as u64)
                    as usize;
                if let Some(vote) = self
                    .metadata
                    .get_meta(&format!("thread_feedback:{session}:{}", message.item_id))?
                {
                    message.feedback = Some(json!({"vote":vote}));
                }
                message.stats = p::finite_stats(body);
                // The preferred location is assistant meta, but a completed
                // tool result can be the only durable owner when the model
                // item was committed before the question-panel hand-off.
                message.question_panel = p::extract_question_panel(body);
                row.assistant = message;
            }
        }
        // Older/live writers may not be able to revise the already-completed
        // assistant item with meta.panel. Recover the bounded panel directly
        // from the latest visible question-panel tool result so the composer
        // still gets an independent interaction surface after reload.
        if row.assistant.question_panel.is_none() {
            let mut statement = conn.prepare(
                "SELECT preview_json,rollout_ordinal FROM thread_items
                 WHERE rollout_id=?1 AND COALESCE(root_turn_id,turn_id)=?2
                   AND visible=1 AND item_type IN ('tool_call','tool_result','question_panel')
                 ORDER BY rollout_ordinal DESC LIMIT 64",
            )?;
            let rows = statement.query_map(params![rollout, root], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?;
            for row_value in rows {
                let (raw, ordinal) = row_value?;
                let item = Self::item(raw, ordinal, session, false)?;
                if let Some(panel) = p::extract_question_panel(&item["payload"]) {
                    row.assistant.question_panel = Some(panel);
                    break;
                }
            }
        }
        row.reasoning_count = summary.reasoning_count;
        row.reasoning_item_id = summary.reasoning_item_id.unwrap_or_default();
        row.workflow_count = summary.workflow_count;
        row.workspace_change_seq = summary.workspace_change_seq;
        p::settle_question_panel(&mut row);
        Ok(row)
    }
}

impl ThreadLogStore for CanonicalThreadLog<'_> {
    fn thread_view_window(
        &self,
        u: &str,
        s: &str,
        k: &str,
        b: Option<i64>,
        a: Option<i64>,
        root: Option<&str>,
        limit: usize,
    ) -> Result<ThreadViewPage> {
        if b.is_some() && a.is_some() {
            bail!("before and after are mutually exclusive");
        }
        let conn = self.connection_for(Some(s), Some(u))?;
        let rollout = Self::rollout(&conn, s)?;
        let seq = Self::position(&conn, rollout.as_deref())?;
        let latest = conn
            .query_row(
                "SELECT latest_user_round FROM queue.canonical_thread_owners WHERE thread_id=?1",
                [s],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let mut page = ThreadViewPage {
            session_id: s.into(),
            epoch: 1,
            view_seq: seq,
            projection_state: "ready".into(),
            latest_round: latest,
            thread_empty: Some(latest == 0),
            ..Default::default()
        };
        if let Some(head) = Self::execution_head(&conn, s, seq)? {
            page.runtime_status = head.runtime_status;
            page.status_turn_id = head.status_turn_id;
            page.active_turn_id = head.active_turn_id;
        }
        if k == "chat" {
            let limit = limit.clamp(1, CHAT_PAGE_MAX);
            let mut turns = Self::turns(&conn, s, b, a, limit as i64 + 1)?;
            page.has_more = turns.len() > limit;
            turns.truncate(limit);
            for turn in turns {
                page.chat.push(self.chat_row(&conn, s, &turn, seq)?);
            }
            page.chat.sort_by_key(|row| row.user_round);
            page.next_before = page.chat.first().map(|r| r.user_round);
            page.next_after = page.chat.last().map(|r| r.user_round);
            page.has_newer = page.next_after.is_some_and(|n| n < latest);
        } else {
            let limit = limit.clamp(1, TRAJECTORY_PAGE_MAX);
            let mut items =
                Self::item_page(&conn, s, root, b, a, limit as i64 + 1, false, false, true)?;
            page.has_more = items.len() > limit;
            items.truncate(limit);
            for item in items {
                let id = item["item_id"].as_str().unwrap_or("");
                let turn = item["turn_id"].as_str().unwrap_or("");
                let root = item["root_turn_id"].as_str().unwrap_or(turn);
                let round = self
                    .admissions()?
                    .turn(s, turn)?
                    .and_then(|v| v["user_turn_index"].as_i64())
                    .unwrap_or(0);
                page.trajectory
                    .push(super::thread_view_projection::trajectory(
                        &item["payload"],
                        id,
                        turn,
                        root,
                        round,
                        item["item_index"].as_i64().unwrap_or(0),
                        item["created_seq"].as_i64().unwrap_or(0),
                        1,
                        item["kind"].as_str().unwrap_or(""),
                        item["status"].as_str().unwrap_or("completed"),
                        item["created_time"].as_f64().unwrap_or(0.0),
                        item["updated_time"].as_f64().unwrap_or(0.0),
                    ));
            }
            page.trajectory.sort_by_key(|r| r.item_index);
            page.next_before = page.trajectory.first().map(|r| r.item_index);
            page.next_after = page.trajectory.last().map(|r| r.item_index);
        }
        Ok(page)
    }
    fn thread_view_page(
        &self,
        u: &str,
        s: &str,
        k: &str,
        b: Option<i64>,
        l: usize,
    ) -> Result<ThreadViewPage> {
        self.thread_view_window(u, s, k, b, None, None, l)
    }
    fn thread_view_trajectory_page(
        &self,
        u: &str,
        s: &str,
        b: Option<i64>,
        r: Option<&str>,
        l: usize,
    ) -> Result<ThreadViewPage> {
        self.thread_view_window(u, s, "trajectory", b, None, r, l)
    }
    fn thread_view_changes(&self, u: &str, s: &str, after: i64) -> Result<ThreadViewChanges> {
        let page = self.thread_view_page(u, s, "chat", None, CHAT_PAGE_MAX)?;
        Ok(ThreadViewChanges {
            runtime_status: page.runtime_status,
            status_turn_id: page.status_turn_id,
            active_turn_id: page.active_turn_id,
            epoch: page.epoch,
            view_seq: page.view_seq,
            reset_required: after > page.view_seq,
            // Control state can change without adding a history record.
            // Callers invoke this only on a committed edge notification.
            rows: page.chat,
        })
    }
    fn thread_body_page(
        &self,
        u: &str,
        s: &str,
        id: &str,
        revision: i64,
        cursor: i64,
    ) -> Result<ThreadBodyPage> {
        // Completed canonical items are immutable.  The transport still
        // carries a revision for clients that cache body pages, but there is
        // no mutable body version to invalidate here.  Accept any nonnegative
        // compatibility cursor and return the canonical revision below.
        if revision < 0 {
            bail!("invalid body revision");
        }
        let (item_id, field) = id
            .rsplit_once('|')
            .ok_or_else(|| anyhow!("invalid body identity"))?;
        let item = self
            .get_thread_item(u, s, item_id, false)?
            .ok_or_else(|| anyhow!("item not found"))?;
        let body = &item["payload"];
        let text = if field == "detail" {
            serde_json::to_string_pretty(body)?
        } else if field == "reasoning" {
            body.get("reasoning")
                .or_else(|| body.get("reasoning_content"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        } else if field == "content" {
            message_content_text(&body["content"]).unwrap_or_default()
        } else {
            bail!("unknown body field");
        };
        let mut start = usize::try_from(cursor).map_err(|_| anyhow!("invalid body cursor"))?;
        if start > text.len() || !text.is_char_boundary(start) {
            bail!("invalid body cursor");
        }
        let mut chunks = Vec::with_capacity(8);
        for _ in 0..8 {
            if start == text.len() {
                break;
            }
            let mut end = (start + BODY_CHUNK_BYTES).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            chunks.push(text[start..end].to_owned());
            start = end;
        }
        Ok(ThreadBodyPage {
            body_id: id.into(),
            revision: 1,
            chunks,
            next_cursor: (start < text.len()).then_some(start as i64),
            has_more: start < text.len(),
        })
    }
    fn thread_view_search(&self, u: &str, s: &str, q: &str) -> Result<ThreadSearchPage> {
        self.check(u, s)?;
        let conn = self.connection(Some(s))?;
        let Some(rollout) = Self::rollout(&conn, s)? else {
            return Ok(ThreadSearchPage::default());
        };
        if q.trim().is_empty() {
            return Ok(ThreadSearchPage::default());
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(300);
        let mut stmt=conn.prepare(&format!("SELECT item_id,rollout_ordinal,{ITEM_BODY_COLUMNS} FROM thread_items WHERE rollout_id=?1 AND {VISIBLE} ORDER BY rollout_ordinal DESC LIMIT 8193"))?;
        let mut rows = stmt.query([rollout])?;
        let mut bodies = ItemBodyReader::for_connection(&conn)?;
        let mut hits = Vec::new();
        let mut bytes = 0usize;
        let mut scanned = 0usize;
        let mut more = false;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let ordinal: i64 = row.get(1)?;
            let reference = ItemBodyRef::from_row(row, 2)?;
            if scanned >= 8192
                || bytes.saturating_add(reference.item_bytes) > 64 * 1024 * 1024
                || std::time::Instant::now() > deadline
            {
                more = true;
                break;
            }
            scanned += 1;
            bytes += reference.item_bytes;
            let raw = bodies.read(&reference, u64::try_from(ordinal)?, &id)?;
            let item = Self::item(serde_json::to_string(&raw)?, ordinal, s, false)?;
            let content = message_content_text(&item["payload"]["content"]).unwrap_or_default();
            if content.contains(q) {
                hits.push(ThreadSearchHit {
                    body: BodyRef {
                        body_id: format!("{id}|content"),
                        revision: 1,
                        bytes: content.len(),
                    },
                    item_id: id,
                    cursor: ordinal,
                    snippet: preview(&content, 256),
                });
                if hits.len() > 20 {
                    more = true;
                    break;
                }
            }
        }
        hits.truncate(20);
        Ok(ThreadSearchPage {
            hits,
            has_more: more,
        })
    }
    fn rebuild_thread_view_page(&self, u: &str, s: &str, _a: i64) -> Result<(i64, bool)> {
        self.check(u, s)?;
        let outcome = wait(self.threads.recover(&ThreadId::new(s)?))??;
        Ok((
            outcome.persisted_through.end_ordinal_exclusive as i64,
            outcome.projection_pending,
        ))
    }
    fn pending_thread_view_rebuilds(&self) -> Result<Vec<(String, String, i64)>> {
        Ok(vec![])
    }
    fn load_subagent_context(&self, u: &str, s: &str, turns: i64) -> Result<Vec<Value>> {
        self.check(u, s)?;
        let conn = self.connection(Some(s))?;
        let Some(rollout) = Self::rollout(&conn, s)? else {
            return Ok(vec![]);
        };
        // A user round can contain several model actions. Select the newest
        // root turns first, then retain bounded completed visible messages
        // from those turns, keeping the quoted context faithful to the thread.
        let roots = conn
            .prepare(
                "SELECT root_turn_id FROM queue.canonical_turn_admissions
                 WHERE thread_id=?1 AND turn_id=root_turn_id
                 ORDER BY user_round DESC, created_at_ms DESC, rowid DESC
                 LIMIT ?2",
            )?
            .query_map(params![s, turns.clamp(1, 16)], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if roots.is_empty() {
            return Ok(vec![]);
        }
        let mut messages = Vec::new();
        let mut statement = conn.prepare(
            "SELECT preview_json FROM thread_items
             WHERE rollout_id=?1
               AND COALESCE(root_turn_id, turn_id) IN (SELECT value FROM json_each(?2))
               AND visible=1
               AND item_type IN ('user_message', 'assistant_message', 'agent_message')
             ORDER BY rollout_ordinal DESC
             LIMIT 257",
        )?;
        let roots = serde_json::to_string(&roots)?;
        let rows = statement.query_map(params![rollout, roots], |row| row.get::<_, String>(0))?;
        for raw in rows {
            let item = Self::item(raw?, 0, s, false)?;
            let mut body = item["payload"].clone();
            body["turn_id"] = item["turn_id"].clone();
            body["root_turn_id"] = item["root_turn_id"].clone();
            body["kind"] = item["kind"].clone();
            if body["role"].is_null() {
                body["role"] = json!(if item["kind"] == "user_message" {
                    "user"
                } else {
                    "assistant"
                });
            }
            if let Some(content) = body["content"].as_str() {
                let truncated = body["_content_bytes"]
                    .as_u64()
                    .is_some_and(|bytes| bytes > content.len() as u64);
                let content = preview(content, 8192);
                if truncated {
                    body["truncated"] = json!(true);
                }
                body["content"] = json!(content);
            }
            messages.push(body);
        }
        messages.reverse();
        Ok(messages)
    }
    fn fork_thread_log(&self, u: &str, s: &str, t: &str, round: i64) -> Result<()> {
        self.check(u, s)?;
        self.check(u, t)?;
        let turn = self
            .admissions()?
            .find_turn(s, round)?
            .ok_or_else(|| anyhow!("source round not found"))?;
        wait(self.threads.fork_through_turn(
            &ThreadId::new(s)?,
            &ThreadId::new(t)?,
            &TurnId::new(turn)?,
        ))??;
        Ok(())
    }
    fn load_thread_context_items(
        &self,
        u: &str,
        s: &str,
        limit: i64,
        internal: bool,
    ) -> Result<Vec<Value>> {
        self.check(u, s)?;
        // This is durable completed-item history for transcript, export and
        // legacy callers.  It must never be substituted with model context:
        // a Compacted record deliberately expands to its replacement context
        // there and would hide the persisted compaction summary/item metadata.
        // `internal` only controls whether completed hidden items may appear.
        let conn = self.connection(Some(s))?;
        let mut items = Self::items(
            &conn,
            s,
            None,
            None,
            None,
            if limit <= 0 { 500 } else { limit },
            internal,
            true,
        )?;
        items.reverse();
        items
            .into_iter()
            .map(|item| {
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
                        _ => bail!("completed chat item has no message role"),
                    });
                }
                Ok(body)
            })
            .collect()
    }
    fn load_thread_execution_context(
        &self,
        u: &str,
        s: &str,
        turn: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.check(u, s)?;
        let id = ThreadId::new(s)?;
        let exclude = if turn.is_empty() {
            None
        } else {
            Some(TurnId::new(turn)?)
        };
        let mut messages = match wait(self.threads.model_messages(&id, exclude.as_ref()))? {
            Ok(m) => m,
            Err(ThreadStoreError::NotFound { .. }) => vec![],
            Err(e) => return Err(e.into()),
        };
        if limit > 0 && messages.len() > limit as usize {
            messages.drain(..messages.len() - limit as usize);
        }
        Ok(messages)
    }
    fn list_thread_visible_messages(
        &self,
        u: &str,
        s: &str,
        b: Option<i64>,
        l: i64,
    ) -> Result<Vec<Value>> {
        self.check(u, s)?;
        let conn = self.connection(Some(s))?;
        Self::items(&conn, s, None, b.map(|n| n - 1), None, l, false, true)?
            .into_iter()
            .map(|item| {
                let mut body = item["payload"].clone();
                for key in [
                    "item_id",
                    "turn_id",
                    "kind",
                    "status",
                    "created_seq",
                    "created_time",
                    "revision",
                ] {
                    body[key] = item[key].clone();
                }
                if body["role"].is_null() {
                    body["role"] = json!(if item["kind"] == "user_message" {
                        "user"
                    } else {
                        "assistant"
                    });
                }
                Ok(body)
            })
            .collect()
    }
    fn find_thread_turn_id(&self, u: &str, s: &str, round: i64) -> Result<Option<String>> {
        self.check(u, s)?;
        self.admissions()?.find_turn(s, round)
    }
    fn accept_thread_turn(&self, u: &str, s: &str, input: &Value) -> Result<Value> {
        self.admissions()?.accept(u, s, input)
    }
    fn update_thread_turn(
        &self,
        u: &str,
        s: &str,
        t: &str,
        status: &str,
        _summary: &str,
        payload: &Value,
    ) -> Result<bool> {
        self.check(u, s)?;
        let admissions = self.admissions()?;
        let previous = admissions
            .turn(s, t)?
            .ok_or_else(|| anyhow!("turn not found"))?;
        if !super::thread_admission::TurnAdmissions::can_transition(
            previous["status"].as_str().unwrap_or(""),
            status,
        ) {
            return Ok(false);
        }
        let terminal = matches!(
            status,
            "completed" | "failed" | "cancelled" | "interrupted" | "rejected"
        );
        let conn = self.connection(Some(s))?;
        let exists = Self::rollout(&conn, s)?.is_some();
        drop(conn);
        if terminal && exists {
            wait(
                self.threads
                    .begin_turn(u, &ThreadId::new(s)?, &TurnId::new(t)?, ""),
            )??;
            let now = chrono::Utc::now().timestamp_millis();
            let event = if status == "completed" {
                CanonicalEvent::TurnCompleted {
                    turn_id: TurnId::new(t)?,
                    completed_at_ms: now,
                }
            } else {
                CanonicalEvent::TurnAborted {
                    turn_id: TurnId::new(t)?,
                    aborted_at_ms: now,
                    error: Some(json!({"status":status,"details":payload})),
                }
            };
            wait(
                self.threads
                    .append_canonical(&ThreadId::new(s)?, vec![CanonicalRecord::Event(event)]),
            )??;
        }
        admissions.update_with_control(s, t, status, terminal && exists, payload)
    }
    fn list_thread_turn_ids_by_status(
        &self,
        u: &str,
        s: &str,
        status: &str,
        l: usize,
    ) -> Result<Vec<String>> {
        self.check(u, s)?;
        self.admissions()?.ids_by_status(s, status, l)
    }
    fn append_thread_item(&self, u: &str, p: &Value) -> Result<()> {
        self.commit_thread_item(u, p).map(|_| ())
    }
    fn commit_thread_item(&self, u: &str, p: &Value) -> Result<Option<Value>> {
        let s = p["session_id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing thread"))?;
        self.check(u, s)?;
        if p["status"]
            .as_str()
            .is_some_and(|s| !matches!(s, "completed" | "failed" | "cancelled"))
        {
            return Ok(None);
        }
        if Self::rollout(&self.connection(Some(s))?, s)?.is_none() {
            bail!("thread instructions must be frozen before appending history");
        }
        let thread = ThreadId::new(s)?;
        let turn = TurnId::new(
            p["turn_id"]
                .as_str()
                .ok_or_else(|| anyhow!("missing turn"))?,
        )?;
        wait(self.threads.begin_turn(u, &thread, &turn, ""))??;
        let id = ItemId::new(
            p["item_id"]
                .as_str()
                .ok_or_else(|| anyhow!("missing item"))?,
        )?;
        if wait(self.threads.read_item(&thread, &id))?.is_ok() {
            return Ok(None);
        }
        let kind = match p["kind"].as_str().unwrap_or("") {
            "user_message" => CanonicalItemKind::UserMessage,
            "assistant_message" => CanonicalItemKind::AssistantMessage,
            "tool_call" => CanonicalItemKind::ToolCall,
            "tool_message" | "tool_result" => CanonicalItemKind::ToolResult,
            "plan" => CanonicalItemKind::Plan,
            _ => CanonicalItemKind::Configuration,
        };
        let root = self
            .admissions()?
            .turn(s, turn.as_str())?
            .and_then(|v| v["root_turn_id"].as_str().map(str::to_owned))
            .map(TurnId::new)
            .transpose()?;
        let outcome = wait(self.threads.append_canonical(
            &thread,
            vec![CanonicalRecord::Event(CanonicalEvent::ItemCompleted(
                CanonicalItem {
                    item_id: id.clone(),
                    turn_id: turn.clone(),
                    root_turn_id: root,
                    created_at_ms: chrono::Utc::now().timestamp_millis(),
                    kind,
                    payload: p.clone(),
                    display_output: None,
                    display_output_truncated: false,
                },
            ))],
        ))??;
        Ok(Some(
            json!({"change_type":"item_upsert","turn_id":turn,"item_id":id,"revision":1,"cursor":outcome.persisted_through.end_ordinal_exclusive}),
        ))
    }
    fn list_thread_turns(&self, u: &str, s: &str, b: Option<i64>, l: i64) -> Result<Vec<Value>> {
        self.check(u, s)?;
        Self::turns(&self.connection(Some(s))?, s, b, None, l)
    }
    fn get_thread_log_counts(&self, u: &str, s: &str, internal: bool) -> Result<(i64, i64)> {
        self.check(u, s)?;
        let conn = self.connection(Some(s))?;
        let rollout = Self::rollout(&conn, s)?;
        let turns=conn.query_row("SELECT COUNT(*) FROM queue.canonical_turn_admissions WHERE thread_id=?1 AND turn_id=root_turn_id",[s],|r|r.get(0))?;
        let items = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM thread_items WHERE rollout_id=?1 {}",
                if internal {
                    String::new()
                } else {
                    format!("AND {VISIBLE}")
                }
            ),
            [rollout],
            |r| r.get(0),
        )?;
        Ok((turns, items))
    }
    fn latest_thread_user_round_by_session(&self, s: &str) -> Result<i64> {
        self.admissions()?.latest_round(s)
    }
    fn thread_turn_tails(
        &self,
        u: &str,
        sessions: &[String],
    ) -> Result<HashMap<String, ThreadTurnTail>> {
        let mut output = HashMap::new();
        let conn = self.connection(None)?;
        for s in sessions.iter().take(1000) {
            if self.check(u, s).is_err() {
                continue;
            }
            if let Some(turn) = Self::turns(&conn, s, None, None, 1)?.pop() {
                let rollout = Self::rollout(&conn, s)?;
                let seq = Self::position(&conn, rollout.as_deref())?;
                output.insert(
                    s.clone(),
                    ThreadTurnTail {
                        session_id: s.clone(),
                        user_round: turn["user_turn_index"].as_i64().unwrap_or(0),
                        turn_status: turn["status"].as_str().unwrap_or("").into(),
                        change_seq: seq,
                        runtime_head: Self::execution_head(&conn, s, seq)?,
                        ..Default::default()
                    },
                );
            }
        }
        Ok(output)
    }
    fn get_thread_turn(
        &self,
        u: &str,
        s: &str,
        t: &str,
        a: i64,
        l: i64,
        internal: bool,
    ) -> Result<Option<Value>> {
        self.get_thread_turn_page(u, s, t, None, Some(a), l, internal)
    }
    fn get_thread_turn_page(
        &self,
        u: &str,
        s: &str,
        t: &str,
        b: Option<i64>,
        a: Option<i64>,
        l: i64,
        internal: bool,
    ) -> Result<Option<Value>> {
        self.check(u, s)?;
        if b.is_some() && a.is_some() {
            bail!("before and after are mutually exclusive");
        }
        let conn = self.connection(Some(s))?;
        let Some(mut turn) = Self::turn(&conn, s, t)? else {
            return Ok(None);
        };
        let limit = l.clamp(1, 500);
        let mut items = Self::items(
            &conn,
            s,
            Some(t),
            b,
            a.or_else(|| b.is_none().then_some(-1)),
            limit + 1,
            internal,
            false,
        )?;
        let more = items.len() > limit as usize;
        items.truncate(limit as usize);
        items.sort_by_key(|i| i["item_index"].as_i64().unwrap_or(0));
        turn["next_before"] = items
            .first()
            .map(|i| i["item_index"].clone())
            .unwrap_or(Value::Null);
        turn["next_after"] = items
            .last()
            .map(|i| i["item_index"].clone())
            .unwrap_or(Value::Null);
        turn["has_more"] = json!(more);
        let rollout = Self::rollout(&conn, s)?;
        let first = items.first().and_then(|i| i["item_index"].as_i64());
        let last = items.last().and_then(|i| i["item_index"].as_i64());
        let exists = |edge: Option<i64>, op: &str| -> Result<bool> {
            let Some(edge) = edge else { return Ok(false) };
            Ok(conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM thread_items WHERE rollout_id=?1 AND COALESCE(root_turn_id,turn_id)=?2 AND rollout_ordinal{op}?3 {})",if internal{String::new()}else{format!("AND {VISIBLE}")}),params![rollout,t,edge],|r|r.get(0))?)
        };
        let older = exists(first, "<")?;
        let newer = exists(last, ">")?;
        turn["has_older"] = json!(older);
        turn["has_newer"] = json!(newer);
        turn["has_more"] = json!(
            more || if a.is_some() || b.is_none() {
                newer
            } else {
                older
            }
        );
        turn["items"] = json!(items);
        Ok(Some(turn))
    }
    fn get_thread_item(&self, u: &str, s: &str, id: &str, internal: bool) -> Result<Option<Value>> {
        self.check(u, s)?;
        let conn = self.connection(Some(s))?;
        let rollout = Self::rollout(&conn, s)?;
        let sql=format!("SELECT {ITEM_BODY_COLUMNS},rollout_ordinal FROM thread_items WHERE rollout_id=?1 AND item_id=?2 {}",if internal{String::new()}else{format!("AND {VISIBLE}")});
        let mut result = conn
            .query_row(&sql, params![rollout, id], |r| {
                Ok((ItemBodyRef::from_row(r, 0)?, r.get::<_, i64>(5)?))
            })
            .optional()?
            .map(|(reference, ordinal)| {
                let raw = ItemBodyReader::for_connection(&conn)?.read(
                    &reference,
                    u64::try_from(ordinal)?,
                    id,
                )?;
                Self::item(serde_json::to_string(&raw)?, ordinal, s, internal)
            })
            .transpose()?;
        if let Some(item) = result.as_mut() {
            if let Some(vote) = self
                .metadata
                .get_meta(&format!("thread_feedback:{s}:{id}"))?
            {
                item["payload"]["feedback"] = json!({"vote":vote});
            }
        }
        Ok(result)
    }
    fn set_thread_item_feedback(
        &self,
        u: &str,
        s: &str,
        id: &str,
        vote: &str,
    ) -> Result<Option<Value>> {
        self.get_thread_item(u, s, id, false)?
            .ok_or_else(|| anyhow!("item not found"))?;
        let key = format!("thread_feedback:{s}:{id}");
        if !self
            .metadata
            .compare_and_swap_meta(&key, None, Some(vote))?
        {
            return Ok(None);
        }
        Ok(Some(json!({"item_id":id,"vote":vote})))
    }
    fn list_thread_changes(&self, u: &str, s: &str, a: i64, l: i64) -> Result<Vec<Value>> {
        self.check(u, s)?;
        self.list_thread_changes_by_session(s, a, l)
    }
    fn list_thread_changes_by_session(&self, s: &str, after: i64, l: i64) -> Result<Vec<Value>> {
        let conn = self.connection(Some(s))?;
        let Some(rollout) = Self::rollout(&conn, s)? else {
            return Ok(vec![]);
        };
        let latest = Self::position(&conn, Some(&rollout))?;
        if after > latest {
            return Ok(vec![
                json!({"change_type":"snapshot_required","change_seq":latest}),
            ]);
        }
        self.threads.history_store().scoped(&rollout,wait(self.threads.lineage_segments(&ThreadId::new(s)?))??.into_iter().map(|s|HistorySegment{rollout_id:s.rollout_id.to_string(),start_ordinal:s.start_ordinal,end_ordinal:s.end.map(|p|p.end_ordinal_exclusive)}).collect()).forward_visible_events(&rollout,if after<=0{None}else{Some((after-1) as u64)},l.clamp(1,200) as usize)?.into_iter().map(|event| {
            (|| {
            let seq=event.ordinal+1;let(kind,turn,id,payload)=match event.kind{
                ThreadHistoryEventKind::ItemCompleted(item)=>{let id=item.item_id.clone();let turn=item.turn_id.clone().unwrap_or_default();("item_upsert",turn,Some(id),Self::item(serde_json::to_string(&item.item_json)?,event.ordinal as i64,s,false)?)},
                ThreadHistoryEventKind::TurnStarted(turn)=>{let id=turn.turn_id;let mut p=Self::turn(&conn,s,&id)?.unwrap_or(json!({"turn_id":id,"root_turn_id":turn.root_turn_id}));p["status"]=json!("running");p["started_time"]=json!(turn.started_at_ms as f64/1000.0);("turn_upsert",id,None,p)},
                ThreadHistoryEventKind::TurnFinished(turn)=>{let id=turn.turn_id;let mut p=Self::turn(&conn,s,&id)?.unwrap_or(json!({"turn_id":id,"root_turn_id":turn.root_turn_id,"status":turn.status}));p["completed_time"]=json!(turn.completed_at_ms.map(|ms|ms as f64/1000.0));("turn_status",id,None,p)}
            };Ok(json!({"change_seq":seq,"change_type":kind,"turn_id":turn,"item_id":id,"revision":1,"payload":payload}))
            })()
        }).collect()
    }
    fn latest_thread_change_seq_by_session(&self, s: &str) -> Result<i64> {
        let conn = self.connection(Some(s))?;
        Self::position(&conn, Self::rollout(&conn, s)?.as_deref())
    }
    fn thread_snapshot(&self, u: &str, s: &str) -> Result<Value> {
        self.thread_snapshot_window(u, s, &ThreadSnapshotWindow::default())
    }
    fn thread_snapshot_window(&self, u: &str, s: &str, w: &ThreadSnapshotWindow) -> Result<Value> {
        self.check(u, s)?;
        let conn = self.connection(Some(s))?;
        let rollout = Self::rollout(&conn, s)?;
        let seq = Self::position(&conn, rollout.as_deref())?;
        let mut turns = Self::turns(
            &conn,
            s,
            w.before_turn_index,
            None,
            w.max_turns.min(200) as i64 + 1,
        )?;
        let max_turns = w.max_turns.clamp(1, 200);
        let max_items = w.max_items.clamp(1, 500);
        let mut more = turns.len() > max_turns;
        turns.truncate(max_turns);
        let mut items = Vec::new();
        let mut item_bytes = 0usize;
        for turn in &turns {
            let id = turn["turn_id"].as_str().unwrap_or("");
            let remaining = max_items.saturating_sub(items.len());
            if remaining == 0 {
                more = true;
                break;
            }
            let mut page = Self::items(
                &conn,
                s,
                Some(id),
                None,
                None,
                remaining as i64 + 1,
                false,
                false,
            )?;
            if page.len() > remaining {
                page.truncate(remaining);
                more = true;
            }
            // The per-turn reader also has a byte cap. Consult the index so
            // a short byte-limited page never claims the turn was complete.
            if let Some(first) = page
                .iter()
                .filter_map(|item| item["item_index"].as_i64())
                .min()
            {
                let older:bool=conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM thread_items WHERE rollout_id=?1 AND COALESCE(root_turn_id,turn_id)=?2 AND rollout_ordinal<?3 AND {VISIBLE})"),params![rollout,id,first],|r|r.get(0))?;
                more |= older;
            }
            for item in page {
                let bytes = serde_json::to_vec(&item)?.len();
                if item_bytes.saturating_add(bytes) > 16 * 1024 * 1024 && !items.is_empty() {
                    more = true;
                    break;
                }
                item_bytes += bytes;
                items.push(item);
            }
            if item_bytes >= 16 * 1024 * 1024 {
                break;
            }
        }
        turns.reverse();
        items.sort_by_key(|i| i["item_index"].as_i64().unwrap_or(0));
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM thread_items WHERE rollout_id=?1 AND {VISIBLE}"),
            [rollout],
            |r| r.get(0),
        )?;
        more |= total > items.len() as i64;
        Ok(
            json!({"cursor":seq,"window_start_turn_index":turns.first().map(|t|&t["user_turn_index"]),"window_end_turn_index":turns.last().map(|t|&t["user_turn_index"]),"turns":turns,"item_total":total,"items":items,"truncated":more,"has_older":more}),
        )
    }
    fn delete_thread_log_by_session(&self, u: &str, s: &str) -> Result<i64> {
        self.check(u, s)?;
        let thread = ThreadId::new(s)?;
        let count = if wait(self.threads.deletion_pending(&thread))?? {
            0
        } else {
            self.get_thread_log_counts(u, s, true)?.1
        };
        wait(self.threads.delete_thread(&thread))??;
        Ok(count)
    }
}
