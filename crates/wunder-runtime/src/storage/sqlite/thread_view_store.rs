use super::thread_log_store::SqliteThreadLogStorage;
use super::SqliteStorage;
use crate::storage::{
    thread_view_store::{self as view, ViewDb},
    StorageLifecycle,
};
use anyhow::Result;
use rusqlite::{
    types::{Value as SqlValue, ValueRef},
    Connection,
};
use serde_json::Value;
use wunder_core::thread_view::*;

pub(super) const SEARCH_SCHEMA: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS thread_body_search USING fts5(scope,body_key,session_id UNINDEXED,user_id UNINDEXED,body_id UNINDEXED,revision UNINDEXED,chunk_index UNINDEXED,content,tokenize='unicode61');";
fn search_scope(user: &str, session: &str) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "s{:x}",
        Sha256::digest(format!("{user}|{session}").as_bytes())
    )
}
fn body_key(id: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("b{:x}", Sha256::digest(id.as_bytes()))
}
pub(super) struct Db<'a>(pub &'a Connection);
impl ViewDb for Db<'_> {
    fn json_text(&self, expression: &str, field: &str) -> String {
        format!("json_extract({expression},'$.{field}')")
    }
    fn text_bytes(&self, expression: &str) -> String {
        format!("length(CAST(({expression}) AS BLOB))")
    }
    fn index_chunk(
        &mut self,
        user: &str,
        session: &str,
        id: &str,
        revision: i64,
        index: i64,
        content: &str,
    ) -> Result<()> {
        if index == 0 {
            self.0.execute("DELETE FROM thread_body_search WHERE rowid IN (SELECT rowid FROM thread_body_search WHERE thread_body_search MATCH ?)",rusqlite::params![format!("scope: {} AND body_key: {}",search_scope(user,session),body_key(id))])?;
        }
        self.0.execute("INSERT INTO thread_body_search(scope,body_key,session_id,user_id,body_id,revision,chunk_index,content) VALUES(?,?,?,?,?,?,?,?)",rusqlite::params![search_scope(user,session),body_key(id),session,user,id,revision,index,content])?;
        Ok(())
    }
    fn search(&mut self, user: &str, session: &str, query: &str) -> Result<ThreadSearchPage> {
        let terms = query
            .split_whitespace()
            .take(8)
            .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        if terms.is_empty() {
            return Ok(ThreadSearchPage::default());
        }
        let expression = format!(
            "scope: {} AND content: ({terms})",
            search_scope(user, session)
        );
        let mut stmt=self.0.prepare("SELECT s.body_id,s.revision,s.chunk_index,s.content FROM thread_body_search s JOIN thread_body_manifests m ON m.session_id=s.session_id AND m.body_id=s.body_id AND m.revision=s.revision JOIN thread_items i ON i.session_id=s.session_id AND i.item_id=substr(s.body_id,1,instr(s.body_id,'|')-1) AND i.visibility='user' WHERE thread_body_search MATCH ? AND s.session_id=? AND s.user_id=? LIMIT 21")?;
        let rows = stmt
            .query_map(rusqlite::params![expression, session, user], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = rows.len() > 20;
        Ok(ThreadSearchPage {
            has_more: more,
            hits: rows
                .into_iter()
                .take(20)
                .map(|(id, revision, cursor, content)| ThreadSearchHit {
                    item_id: id.split('|').next().unwrap_or("").into(),
                    body: BodyRef {
                        body_id: id,
                        revision,
                        bytes: content.len(),
                    },
                    cursor,
                    snippet: preview(&content, 256),
                })
                .collect(),
        })
    }
    fn execute(&mut self, sql: &str, p: &[Value]) -> Result<u64> {
        Ok(self
            .0
            .execute(sql, rusqlite::params_from_iter(p.iter().map(param)))? as u64)
    }
    fn query(&mut self, sql: &str, p: &[Value]) -> Result<Vec<Vec<Value>>> {
        let mut stmt = self.0.prepare(sql)?;
        let count = stmt.column_count();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(p.iter().map(param)), |r| {
                (0..count)
                    .map(|n| {
                        Ok(match r.get_ref(n)? {
                            ValueRef::Null => Value::Null,
                            ValueRef::Integer(v) => v.into(),
                            ValueRef::Real(v) => serde_json::json!(v),
                            ValueRef::Text(v) => String::from_utf8_lossy(v).into_owned().into(),
                            ValueRef::Blob(_) => Value::Null,
                        })
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
fn param(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Number(n) if n.is_i64() => SqlValue::Integer(n.as_i64().unwrap()),
        Value::Number(n) => SqlValue::Real(n.as_f64().unwrap_or(0.0)),
        Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
        Value::String(s) => SqlValue::Text(s.clone()),
        _ => SqlValue::Text(v.to_string()),
    }
}
impl SqliteStorage {
    fn view_connection(&self) -> Result<Connection> {
        self.ensure_initialized()?;
        let conn = Connection::open_with_flags(
            &self.db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(250))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        conn.progress_handler(1000, Some(move || std::time::Instant::now() > deadline));
        Ok(conn)
    }
    pub(super) fn view_search(&self, u: &str, s: &str, q: &str) -> Result<ThreadSearchPage> {
        self.ensure_initialized()?;
        Db(&self.view_connection()?).search(u, s, q)
    }
    pub(super) fn view_pending(&self) -> Result<Vec<(String, String, i64)>> {
        self.ensure_initialized()?;
        let mut conn = self.open()?;
        conn.busy_timeout(std::time::Duration::from_millis(100))?;
        // A deferred transaction keeps idle polling free of SQLite write locks.
        let tx = conn.transaction()?;
        view::discover_rebuilds(&mut Db(&tx))?;
        let rows = view::pending_rebuilds(&mut Db(&tx))?;
        tx.commit()?;
        Ok(rows)
    }

    pub(super) fn view_page(
        &self,
        u: &str,
        s: &str,
        k: &str,
        b: Option<i64>,
        a: Option<i64>,
        r: Option<&str>,
        l: usize,
    ) -> Result<ThreadViewPage> {
        // Repair legacy lifecycle writes before opening the read-only snapshot.
        // Catalog reads do the same for threads that are not currently open.
        self.reconcile_terminal_turns_impl(u, s)?;
        let mut conn = self.view_connection()?;
        let tx = conn.transaction()?;
        let page = view::page(&mut Db(&tx), u, s, k, b, a, r, l)?;
        tx.commit()?;
        Ok(page)
    }
    pub(super) fn view_changes(&self, u: &str, s: &str, a: i64) -> Result<ThreadViewChanges> {
        let mut conn = self.view_connection()?;
        let tx = conn.transaction()?;
        let page = view::changes(&mut Db(&tx), u, s, a)?;
        tx.commit()?;
        Ok(page)
    }
    pub(super) fn view_body(
        &self,
        u: &str,
        s: &str,
        id: &str,
        r: i64,
        c: i64,
    ) -> Result<ThreadBodyPage> {
        view::body(&mut Db(&self.view_connection()?), u, s, id, r, c)
    }
    pub(super) fn view_rebuild(&self, u: &str, s: &str, a: i64) -> Result<(i64, bool)> {
        self.ensure_initialized()?;
        let mut conn = self.open()?;
        conn.busy_timeout(std::time::Duration::from_millis(100))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        conn.progress_handler(1000, Some(move || std::time::Instant::now() > deadline));
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // Maintenance callers may have read their cursor before another
        // process advanced it. Ordinary explicit rebuilds remain supported.
        let current = tx.query_row("SELECT rebuild_after,projection_state FROM thread_view_heads WHERE session_id=? AND user_id=?", rusqlite::params![s,u], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)));
        if let Ok((cursor, state)) = current {
            if state == "rebuilding" && cursor != a {
                return Ok((cursor, true));
            }
        }
        let result = view::rebuild(&mut Db(&tx), u, s, a)?;
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::ThreadLogStore;
    use serde_json::json;

    #[test]
    fn trajectory_timing_is_independent_of_ledger_cursor_and_compaction_keeps_summary() {
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let accepted = storage.accept_thread_turn("fixture-user", "fixture-thread", &json!({"content":"fixture input"})).unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        for index in 0..3 {
            storage.append_thread_item("fixture-user", &json!({
                "session_id":"fixture-thread", "turn_id":turn,
                "item_id":format!("fixture-model-{index}"),
                "kind":"assistant_message", "status":"completed", "content":"fixture response"
            })).unwrap();
        }
        storage.append_thread_item("fixture-user", &json!({
            "session_id":"fixture-thread", "turn_id":turn, "item_id":"fixture-compaction",
            "kind":"compaction", "status":"completed", "content":"history",
            "summary_text":"Fixture retained context."
        })).unwrap();
        // Reproduce an already persisted legacy projection without rebuilding
        // the entire thread or changing its execution payload.
        storage.open().unwrap().execute(
            "UPDATE thread_trajectory_rows SET payload=json_set(payload,'$.summary','history') WHERE item_id='fixture-compaction'", []
        ).unwrap();
        for state in ["ready", "source"] {
            storage.open().unwrap().execute("UPDATE thread_view_heads SET projection_state=?", [state]).unwrap();
            let first = storage.thread_view_trajectory_page("fixture-user", "fixture-thread", None, None, 2).unwrap();
            let older = storage.thread_view_trajectory_page("fixture-user", "fixture-thread", first.next_before, None, 2).unwrap();
            assert!(first.has_more);
            assert_eq!(serde_json::to_value(&first.timeline).unwrap(), serde_json::to_value(&older.timeline).unwrap());
            assert!(!first.timeline_has_more);
            assert!(first.timeline.len() >= 4);
            assert!(first.timeline.iter().all(|row| row.started_at > 0.0));
            let row = first.trajectory.iter().find(|row| row.item_id == "fixture-compaction").unwrap();
            assert_eq!(row.summary, "Fixture retained context.");
            let body = row.detail.as_ref().unwrap();
            let detail = storage.thread_body_page("fixture-user", "fixture-thread", &body.body_id, body.revision, 0).unwrap();
            assert!(detail.chunks.join("").contains("Fixture retained context."));
            assert!(serde_json::to_vec(&first).unwrap().len() < TRAJECTORY_RESPONSE_BYTES);
            assert!(storage.thread_view_trajectory_page("fixture-other", "fixture-thread", None, None, 2).unwrap().timeline.is_empty());
        }
    }

    #[test]
    fn chat_plan_projects_live_updates_reload_legacy_and_source() {
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let accepted = storage.accept_thread_turn("fixture-user", "fixture-thread", &json!({"content":"Fixture input"})).unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        let id = format!("{turn}:plan");
        let first = json!({"session_id":"fixture-thread", "turn_id":turn, "item_id":id,
            "kind":"plan", "status":"completed", "event_type":"plan_update", "explanation":"Fixture plan",
            "plan":[{"step":"First step","status":"in_progress"},{"step":"Second step","status":"pending"}]});
        let read = || storage.thread_view_page("fixture-user", "fixture-thread", "chat", None, 12).unwrap();
        let before = read().view_seq;
        storage.append_thread_item("fixture-user", &first).unwrap();
        let initial = read();
        let plan = initial.chat[0].plan.as_ref().expect("plan without assistant output");
        assert_eq!(plan.item_id, id);
        assert_eq!(plan.steps[0].status, "in_progress");
        assert!(initial.chat[0].assistant.content.is_empty());
        assert_eq!(storage.thread_view_changes("fixture-user", "fixture-thread", before).unwrap().rows[0].plan.as_ref(), Some(plan));
        let mut second = first.clone();
        second["plan"][0]["status"] = json!("completed");
        second["plan"][1]["status"] = json!("in_progress");
        storage.append_thread_item("fixture-user", &second).unwrap();
        let updated = read();
        let plan = updated.chat[0].plan.as_ref().unwrap();
        assert!(plan.revision > initial.chat[0].plan.as_ref().unwrap().revision);
        assert_eq!(plan.steps[0].status, "completed");
        assert_eq!(plan.steps[1].status, "in_progress");
        storage.append_thread_item("fixture-user", &json!({"session_id":"fixture-thread", "turn_id":turn,
            "item_id":format!("{turn}:text-2"),"kind":"assistant_message","status":"completed","model_round":2,"content":"Fixture result"})).unwrap();
        assert_eq!(read().chat[0].plan.as_ref(), Some(plan));
        // Simulate a chat card persisted before plan became part of the schema.
        let conn = storage.open().unwrap();
        let mut legacy = serde_json::to_value(&read().chat[0]).unwrap();
        legacy.as_object_mut().unwrap().remove("plan");
        conn.execute("UPDATE thread_chat_rows SET payload=?", [legacy.to_string()]).unwrap();
        assert_eq!(read().chat[0].plan.as_ref(), Some(plan));
        assert_eq!(storage.thread_view_changes("fixture-user", "fixture-thread", before).unwrap().rows[0].plan.as_ref(), Some(plan));
        storage.append_thread_item("fixture-user", &json!({"session_id":"fixture-thread", "turn_id":turn,
            "item_id":format!("{turn}:text-3"),"kind":"assistant_message","status":"completed","model_round":3,"content":"Next fixture result"})).unwrap();
        assert_eq!(read().chat[0].plan.as_ref(), Some(plan));
        conn.execute("UPDATE thread_view_heads SET projection_state='source'", []).unwrap();
        assert_eq!(read().chat[0].plan.as_ref(), Some(plan));
        // A fresh storage instance reads the same plan from disk.
        let reloaded = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        assert_eq!(reloaded.thread_view_page("fixture-user", "fixture-thread", "chat", None, 12).unwrap().chat[0].plan.as_ref(), Some(plan));
    }

    #[test]
    fn trajectory_diagnostics_match_projection_and_source_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let accepted = storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"input"}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        storage.append_thread_item("fixture-user", &json!({
            "session_id":"fixture-thread", "turn_id":turn, "item_id":"fixture-model",
            "kind":"assistant_message", "status":"failed", "model_round":1,
            "trajectory_retry":{"attempt":1,"max_attempts":3,"message":"fixture retry","will_retry":false},
            "trajectory_failure":{"status":"failed","code":"UPSTREAM_TIMEOUT","message":"fixture timeout","retryable":true,"recovery_action":"retry_later"},
        })).unwrap();
        storage.append_thread_item("fixture-user", &json!({
            "session_id":"fixture-thread", "turn_id":turn, "item_id":"fixture-tool",
            "kind":"tool_call", "status":"failed", "tool":"fixture_tool", "error":"fixture tool error",
        })).unwrap();
        let live = storage
            .thread_view_trajectory_page("fixture-user", "fixture-thread", None, Some(turn), 12)
            .unwrap();
        let conn = storage.open().unwrap();
        conn.execute("UPDATE thread_view_heads SET projection_state='source'", [])
            .unwrap();
        let source = storage
            .thread_view_trajectory_page("fixture-user", "fixture-thread", None, Some(turn), 12)
            .unwrap();
        for row in &live.trajectory {
            let fallback = source
                .trajectory
                .iter()
                .find(|other| other.item_id == row.item_id)
                .unwrap();
            assert_eq!(fallback.diagnostics, row.diagnostics);
        }
    }

    #[test]
    fn trajectory_diagnostics_legacy_backfill_respects_page_budget_and_cursors() {
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let accepted = storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"input"}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        for index in 0..100 {
            storage.append_thread_item("fixture-user", &json!({
                "session_id":"fixture-thread", "turn_id":turn,
                "item_id":format!("fixture-error-{index}"), "kind":"terminal", "status":"failed",
                "error":{"code":"c".repeat(128), "message":"e".repeat(2048)},
                "source_stage":"s".repeat(128), "recovery_action":"r".repeat(128), "retryable":true,
            })).unwrap();
        }
        storage.open().unwrap().execute(
            "UPDATE thread_trajectory_rows SET payload=json_remove(payload,'$.diagnostics') WHERE session_id='fixture-thread'",
            [],
        ).unwrap();
        let first = storage
            .thread_view_trajectory_page("fixture-user", "fixture-thread", None, Some(turn), 100)
            .unwrap();
        assert!(first.has_more);
        assert!(first.trajectory.len() < 100);
        let mut before = None;
        let mut seen = std::collections::HashSet::new();
        let mut failures = 0;
        for _ in 0..10 {
            let page = storage
                .thread_view_trajectory_page(
                    "fixture-user",
                    "fixture-thread",
                    before,
                    Some(turn),
                    100,
                )
                .unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len() < TRAJECTORY_RESPONSE_BYTES);
            for row in &page.trajectory {
                assert!(seen.insert(row.item_id.clone()));
                if row.kind == "terminal" {
                    assert_eq!(
                        row.diagnostics.as_ref().unwrap()["failure"]["message"],
                        "e".repeat(2048)
                    );
                    failures += 1;
                }
            }
            if !page.has_more {
                break;
            }
            assert_ne!(page.next_before, before);
            before = page.next_before;
        }
        assert_eq!(failures, 100);
    }

    #[test]
    fn automatic_compaction_is_counted_in_live_and_source_process_views() {
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let accepted = storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"input"}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        storage
            .append_thread_item(
                "fixture-user",
                &json!({
                    "session_id":"fixture-thread", "turn_id":turn, "item_id":"fixture-prompt",
                    "kind":"system_message", "status":"completed", "content":"internal prompt",
                    "visibility":"model_internal", "meta":{"type":"system_prompt"}
                }),
            )
            .unwrap();
        for index in 0..2 {
            let meta = if index == 0 {
                json!({"type":"compaction_summary", "replacement_history":[{"content":"internal context"}]})
            } else {
                json!({"compaction_id":"fixture-compaction-id"})
            };
            let item = json!({
                "session_id":"fixture-thread", "turn_id":turn,
                "item_id":format!("fixture-compaction-{index}"), "kind":"system_message",
                "status":"completed", "content":"Retained compacted history.", "meta":meta
            });
            storage.append_thread_item("fixture-user", &item).unwrap();
            // Repeated persistence must not duplicate the process count.
            storage.append_thread_item("fixture-user", &item).unwrap();
        }
        let live = storage
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(live.chat[0].workflow_count, 2);
        assert!(live.chat[0].assistant.content.is_empty());
        let trajectory = storage
            .thread_view_trajectory_page("fixture-user", "fixture-thread", None, Some(turn), 12)
            .unwrap();
        let compaction = trajectory
            .trajectory
            .iter()
            .find(|item| item.item_id == "fixture-compaction-0")
            .unwrap();
        assert_eq!(compaction.summary, "上下文压缩");
        let conn = storage.open().unwrap();
        conn.execute("UPDATE thread_view_heads SET projection_state='source'", [])
            .unwrap();
        let source = storage
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(source.chat[0].workflow_count, live.chat[0].workflow_count);
        assert!(source.chat[0].assistant.content.is_empty());
    }

    #[test]
    fn admission_queue_reason_and_execution_share_one_durable_head() {
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let accepted = storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"fixture input"}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        let page = storage
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.chat[0].status, "accepted");
        assert_eq!(page.runtime_status, "running");
        assert!(page.chat[0].queued_input.is_none());
        let admission_seq = page.view_seq;
        for (reason, expected) in [
            ("input", "running"),
            ("scheduler", "running"),
            ("capacity", "queued"),
        ] {
            let payload =
                json!({"queued_input":{"queue_id":"fixture-queue","reason":reason,"priority":1}});
            assert!(storage
                .update_thread_turn(
                    "fixture-user",
                    "fixture-thread",
                    turn,
                    "queued",
                    "",
                    &payload
                )
                .unwrap());
            let page = storage
                .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
                .unwrap();
            assert_eq!(page.runtime_status, expected);
            assert_eq!(page.chat[0].queued_input.as_ref().unwrap().reason, reason);
            let changes = storage
                .thread_view_changes("fixture-user", "fixture-thread", admission_seq)
                .unwrap();
            assert_eq!(changes.runtime_status, expected);
            assert_eq!(
                changes.rows[0].queued_input.as_ref().unwrap().queue_id,
                "fixture-queue"
            );
            assert!(!storage
                .update_thread_turn(
                    "fixture-user",
                    "fixture-thread",
                    turn,
                    "queued",
                    "",
                    &payload
                )
                .unwrap());
        }
        storage
            .update_thread_turn(
                "fixture-user",
                "fixture-thread",
                turn,
                "running",
                "",
                &json!({}),
            )
            .unwrap();
        // A stale scheduler scan cannot reclaim composer ownership after dispatch.
        assert!(!storage
            .update_thread_turn(
                "fixture-user",
                "fixture-thread",
                turn,
                "queued",
                "",
                &json!({"queued_input":{"queue_id":"fixture-queue","reason":"input","priority":0}})
            )
            .unwrap());
        let page = storage
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.runtime_status, "running");
        assert!(page.chat[0].queued_input.is_none());
        let parked = storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"parked input"}),
            )
            .unwrap();
        let parked_turn = parked["turn_id"].as_str().unwrap();
        storage
            .update_thread_turn(
                "fixture-user",
                "fixture-thread",
                parked_turn,
                "queued",
                "",
                &json!({"queued_input":{"queue_id":"fixture-queue-2","reason":"input","priority":0}}),
            )
            .unwrap();
        // A newer composer-parked input is not the executor.  The running
        // turn remains the one durable session head used by shell controls.
        let page = storage
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.runtime_status, "running");
        assert_eq!(page.active_turn_id, turn);
        storage
            .open()
            .unwrap()
            .execute("UPDATE thread_view_heads SET projection_state='source'", [])
            .unwrap();
        let page = storage
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.active_turn_id, turn);
        assert_eq!(page.runtime_status, "running");
        assert!(page.chat[0].queued_input.is_none());
    }

    #[test]
    fn queued_input_preserves_active_head_and_answer_phase() {
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let first = storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"first"}),
            )
            .unwrap();
        let root = first["turn_id"].as_str().unwrap();
        storage
            .update_thread_turn(
                "fixture-user",
                "fixture-thread",
                root,
                "running",
                "",
                &json!({}),
            )
            .unwrap();
        storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"second"}),
            )
            .unwrap();
        let page = storage
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.runtime_status, "running");
        assert_eq!(page.active_turn_id, root);
        let id = format!("{root}:text-1");
        storage.append_thread_item("fixture-user", &json!({"session_id":"fixture-thread","turn_id":root,"item_id":id,"kind":"assistant_message","content":"","status":"running"})).unwrap();
        for (field, data) in [
            ("content", json!({"content":"answer"})),
            ("reasoning", json!({"reasoning":"late thought"})),
        ] {
            storage
                .upsert_thread_text_block(
                    "fixture-user",
                    "fixture-thread",
                    &json!({"item_id":id,"field":field,"block_index":0,"event_id":1,"data":data}),
                )
                .unwrap();
        }
        let page = storage
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.chat[0].process_activity.kind, "responding");
        assert_eq!(page.chat[0].assistant.reasoning, "late thought");
        storage
            .open()
            .unwrap()
            .execute("UPDATE thread_view_heads SET projection_state='source'", [])
            .unwrap();
        let page = storage
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.runtime_status, "running");
        assert_eq!(page.active_turn_id, root);
    }

    #[test]
    fn live_process_phase_and_reasoning_tail_advance_past_preview_limit() {
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let turn = storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-live",
                &json!({"content":"fixture input"}),
            )
            .unwrap();
        let root = turn["turn_id"].as_str().unwrap();
        let id = format!("{root}:text-1");
        storage.append_thread_item("fixture-user", &json!({"session_id":"fixture-live","turn_id":root,"item_id":id,"kind":"assistant_message","content":"","status":"running"})).unwrap();
        let first = "思考".repeat(1000);
        for (index, offset, text) in [(0, 0, first.as_str()), (1, 2000, " fixture newest thought")]
        {
            storage.upsert_thread_text_block("fixture-user", "fixture-live", &json!({"item_id":id,"field":"reasoning","block_index":index,"event_id":index+1,"data":{"reasoning":text,"reasoning_offset":offset}})).unwrap();
        }
        let page = storage
            .thread_view_page("fixture-user", "fixture-live", "chat", None, 12)
            .unwrap();
        let row = &page.chat[0];
        assert!(row.assistant.reasoning.len() <= 2048);
        assert!(row.assistant.reasoning.ends_with("fixture newest thought"));
        assert_eq!(row.process_activity.kind, "thinking");
        assert_eq!(row.assistant.reasoning_offset, 2023);
        let cumulative = " fixture newest thought rewritten tail";
        storage.upsert_thread_text_block("fixture-user", "fixture-live", &json!({"item_id":id,"field":"reasoning","block_index":1,"event_id":3,"data":{"reasoning":cumulative,"reasoning_offset":2000}})).unwrap();
        let page = storage
            .thread_view_page("fixture-user", "fixture-live", "chat", None, 12)
            .unwrap();
        assert!(page.chat[0].assistant.reasoning.ends_with(cumulative));
        assert_eq!(
            page.chat[0].assistant.reasoning_offset,
            2000 + cumulative.len()
        );
        storage.append_thread_item("fixture-user", &json!({"session_id":"fixture-live","turn_id":root,"item_id":"fixture-tool","kind":"tool_call","tool":"fixture_tool","status":"running"})).unwrap();
        let page = storage
            .thread_view_page("fixture-user", "fixture-live", "chat", None, 12)
            .unwrap();
        assert_eq!(page.chat[0].process_activity.kind, "tool");
        assert_eq!(page.chat[0].process_activity.tool, "fixture_tool");
    }

    #[test]
    fn source_chat_batches_page_queries_and_preserves_block_fallbacks() {
        struct Counted<'a> {
            db: Db<'a>,
            queries: usize,
            rows: usize,
        }
        impl ViewDb for Counted<'_> {
            fn json_text(&self, expression: &str, field: &str) -> String {
                self.db.json_text(expression, field)
            }
            fn text_bytes(&self, expression: &str) -> String {
                self.db.text_bytes(expression)
            }
            fn query(&mut self, sql: &str, p: &[Value]) -> Result<Vec<Vec<Value>>> {
                self.queries += 1;
                let rows = self.db.query(sql, p)?;
                self.rows += rows.len();
                Ok(rows)
            }
            fn execute(&mut self, _: &str, _: &[Value]) -> Result<u64> {
                panic!("page reads must not write")
            }
            fn index_chunk(
                &mut self,
                _: &str,
                _: &str,
                _: &str,
                _: i64,
                _: i64,
                _: &str,
            ) -> Result<()> {
                unreachable!()
            }
            fn search(&mut self, _: &str, _: &str, _: &str) -> Result<ThreadSearchPage> {
                unreachable!()
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        for n in 1..=21 {
            let turn = storage
                .accept_thread_turn(
                    "fixture-user",
                    "fixture-thread",
                    &json!({"content":format!("input {n}")}),
                )
                .unwrap();
            let root = turn["turn_id"].as_str().unwrap();
            storage.append_thread_item("fixture-user", &json!({"session_id":"fixture-thread", "turn_id":root,"item_id":format!("{root}:tool"),"kind":"tool_call","status":"failed","tool":"fixture_tool","meta":{"workspace_changed":true}})).unwrap();
            let item = format!("{root}:text-1");
            storage.append_thread_item("fixture-user", &json!({"session_id":"fixture-thread", "turn_id":root,"item_id":item,"kind":"assistant_message","content":"","status":"completed"})).unwrap();
            for (field, data) in [
                ("content", json!({"content":format!("answer {n}")})),
                ("reasoning", json!({"reasoning":format!("reason {n}")})),
            ] {
                storage.upsert_thread_text_block("fixture-user","fixture-thread",&json!({"item_id":item,"field":field,"block_index":0,"event_id":n,"data":data})).unwrap();
            }
        }
        let conn = storage.open().unwrap();
        conn.execute("UPDATE thread_view_heads SET projection_state='source'", [])
            .unwrap();
        let mut db = Counted {
            db: Db(&conn),
            queries: 0,
            rows: 0,
        };
        let small = view::page(
            &mut db,
            "fixture-user",
            "fixture-thread",
            "chat",
            None,
            None,
            None,
            1,
        )
        .unwrap();
        let small_queries = db.queries;
        db.queries = 0;
        db.rows = 0;
        let large = view::page(
            &mut db,
            "fixture-user",
            "fixture-thread",
            "chat",
            None,
            None,
            None,
            20,
        )
        .unwrap();
        assert_eq!(large.chat.len(), 20);
        assert_eq!(
            db.queries, small_queries,
            "round trips must not grow with page size"
        );
        assert!(db.queries <= 10);
        assert!(
            db.rows <= 150,
            "do not return every execution or workflow item"
        );
        assert!(large.has_more);
        assert_eq!(large.next_before, Some(2));
        assert_eq!(small.chat[0].assistant.content, "answer 21");
        for row in &large.chat {
            assert!(
                row.workspace_change_seq > 0,
                "source pages retain file invalidations"
            );
            assert_eq!(row.user.content, format!("input {}", row.user_round));
            assert_eq!(row.assistant.content, format!("answer {}", row.user_round));
            assert_eq!(
                row.assistant.reasoning,
                format!("reason {}", row.user_round)
            );
            assert_eq!((row.workflow_count, row.failed_count), (1, 1));
        }
        let older = view::page(
            &mut db,
            "fixture-user",
            "fixture-thread",
            "chat",
            large.next_before,
            None,
            None,
            20,
        )
        .unwrap();
        assert_eq!(older.chat.len(), 1);
        assert_eq!(older.chat[0].user_round, 1);
        assert!(!older.has_more);
        let newer = view::page(
            &mut db,
            "fixture-user",
            "fixture-thread",
            "chat",
            None,
            Some(1),
            None,
            20,
        )
        .unwrap();
        assert_eq!(newer.chat.first().map(|row| row.user_round), Some(2));
        assert_eq!(newer.chat.last().map(|row| row.user_round), Some(21));
        assert!(!newer.has_newer);
    }

    #[test]
    fn oversized_legacy_threads_use_source_pages_without_entering_rebuild_queue() {
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        storage.ensure_initialized().unwrap();
        let conn = storage.open().unwrap();
        conn.execute("INSERT INTO thread_logs(session_id,user_id,latest_user_turn,latest_change_seq,created_time,updated_time) VALUES('fixture-large','fixture-user',1,50001,1,1)", []).unwrap();
        {
            let mut db = Db(&conn);
            view::initialize(&mut db, "fixture-user", "fixture-large").unwrap();
            assert!(view::pending_rebuilds(&mut db).unwrap().is_empty());
        }
        let state: String = conn
            .query_row(
                "SELECT projection_state FROM thread_view_heads WHERE session_id='fixture-large'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "source");
        let turn = storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-large",
                &json!({"content":"fixture input"}),
            )
            .unwrap();
        let root = turn["turn_id"].as_str().unwrap();
        storage
            .append_thread_item(
                "fixture-user",
                &json!({
                    "session_id":"fixture-large",
                    "turn_id":root,
                    "item_id":format!("{root}:system"),
                    "kind":"system_message",
                    "visibility":"model_internal",
                    "content":"fixture context",
                    "status":"completed"
                }),
            )
            .unwrap();
        storage
            .append_thread_item(
                "fixture-user",
                &json!({
                    "session_id":"fixture-large",
                    "turn_id":root,
                    "item_id":format!("{root}:tool"),
                    "kind":"tool_call",
                    "tool":"fixture_tool",
                    "visibility":"user",
                    "content":"fixture tool call",
                    "status":"completed"
                }),
            )
            .unwrap();
        storage.append_thread_item("fixture-user", &json!({"session_id":"fixture-large","turn_id":root,"item_id":format!("{root}:text-1"),"kind":"assistant_message","content":"fixture answer","status":"completed"})).unwrap();
        let page = storage
            .thread_view_page("fixture-user", "fixture-large", "chat", None, 12)
            .unwrap();
        assert_eq!(page.projection_state, "source");
        assert_eq!(page.chat.len(), 1);
        let overview = storage
            .thread_view_trajectory_page("fixture-user", "fixture-large", None, None, 12)
            .unwrap();
        assert_eq!(overview.trajectory.len(), 4);
        assert_eq!(overview.trajectory[0].root_turn_id, root);
        assert!(overview.trajectory[0].detail.is_some());
        assert!(overview
            .trajectory
            .iter()
            .any(|row| row.kind == "system_message"));
        assert!(overview
            .trajectory
            .iter()
            .any(|row| row.kind == "user_message"));
        assert!(overview
            .trajectory
            .iter()
            .any(|row| row.kind == "tool_call"));
        assert!(overview
            .trajectory
            .iter()
            .any(|row| row.kind == "assistant_message"));
        let detail = storage
            .thread_view_trajectory_page("fixture-user", "fixture-large", None, Some(root), 12)
            .unwrap();
        assert_eq!(detail.trajectory.len(), 4);
        let context = detail
            .trajectory
            .iter()
            .find(|row| row.kind == "system_message")
            .unwrap();
        let body = context.detail.as_ref().unwrap();
        let body_page = storage
            .thread_body_page(
                "fixture-user",
                "fixture-large",
                &body.body_id,
                body.revision,
                0,
            )
            .unwrap();
        // Internal system prompts remain visible as a bounded timeline row,
        // but their body is deliberately redacted at the source-read
        // boundary.  The trajectory must not turn the debug view into a
        // prompt/credential disclosure channel.
        assert!(body_page.chunks.is_empty());
        assert!(detail.trajectory[0]
            .detail
            .as_ref()
            .unwrap()
            .body_id
            .ends_with("|source_detail"));
    }

    #[test]
    fn source_trajectory_pages_every_item_in_created_sequence_order() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        db.ensure_initialized().unwrap();
        let conn = db.open().unwrap();
        conn.execute("INSERT INTO thread_logs(session_id,user_id,latest_user_turn,latest_change_seq,created_time,updated_time) VALUES('fixture-source-pages','fixture-user',1,50001,1,1)", []).unwrap();
        {
            let mut view_db = Db(&conn);
            view::initialize(&mut view_db, "fixture-user", "fixture-source-pages").unwrap();
        }
        let receipt = db
            .accept_thread_turn(
                "fixture-user",
                "fixture-source-pages",
                &json!({"content":"fixture input"}),
            )
            .unwrap();
        let turn = receipt["turn_id"].as_str().unwrap().to_owned();
        for index in 0..25 {
            db.append_thread_item(
                "fixture-user",
                &json!({
                    "session_id":"fixture-source-pages",
                    "turn_id":turn,
                    "item_id":format!("{turn}:tool-{index}"),
                    "kind":"tool_call",
                    "tool":format!("fixture_tool_{index}"),
                    "visibility":"user",
                    "status":"completed"
                }),
            )
            .unwrap();
        }

        let first = db
            .thread_view_trajectory_page("fixture-user", "fixture-source-pages", None, None, 10)
            .unwrap();
        assert_eq!(first.projection_state, "source");
        assert_eq!(first.trajectory.len(), 10);
        assert!(first.has_more);
        let before = first.next_before.expect("source page cursor");
        assert!(first.trajectory.iter().all(|row| !row.overview_only));
        assert!(first.trajectory.iter().all(|row| row.kind == "tool_call"));
        assert!(first.timeline.len() >= 25);
        assert!(first.timeline.windows(2).all(|rows| rows[0].created_seq < rows[1].created_seq));
        assert!(first.timeline.iter().any(|row| row.kind == "tool_call"));

        let second = db
            .thread_view_trajectory_page(
                "fixture-user",
                "fixture-source-pages",
                Some(before),
                None,
                10,
            )
            .unwrap();
        assert_eq!(second.trajectory.len(), 10);
        assert!(second.has_more);
        assert!(second.trajectory.iter().all(|row| !row.overview_only));
        assert!(second.trajectory.iter().all(|row| row.created_seq < before));

        let third = db
            .thread_view_trajectory_page(
                "fixture-user",
                "fixture-source-pages",
                second.next_before,
                None,
                10,
            )
            .unwrap();
        assert_eq!(third.trajectory.len(), 6);
        assert!(!third.has_more);
        assert!(third.trajectory.iter().all(|row| !row.overview_only));
    }

    #[test]
    fn rebuilding_threads_read_tail_windows_and_original_bodies_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let mut turns = Vec::new();
        for n in 0..15 {
            let receipt = db
                .accept_thread_turn(
                    "fixture-user",
                    "fixture-thread",
                    &json!({"content":format!("fixture input {n}")}),
                )
                .unwrap();
            let turn = receipt["turn_id"].as_str().unwrap().to_owned();
            db.append_thread_item("fixture-user",&json!({"session_id":"fixture-thread","turn_id":turn,"item_id":format!("{turn}:text-1"),"kind":"assistant_message","content":"😀fixture answer".repeat(700),"status":"completed"})).unwrap();
            db.update_thread_turn(
                "fixture-user",
                "fixture-thread",
                &turn,
                "completed",
                "",
                &json!({}),
            )
            .unwrap();
            turns.push(turn);
        }
        let conn = db.open().unwrap();
        conn.execute(
            "UPDATE thread_view_heads SET projection_state='rebuilding',rebuild_after=1",
            [],
        )
        .unwrap();
        let before: i64 = conn
            .query_row("SELECT view_seq FROM thread_view_heads", [], |r| r.get(0))
            .unwrap();
        let page = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.projection_state, "source");
        assert_eq!(page.epoch, -1);
        assert_eq!(page.chat.len(), 12);
        assert_eq!(page.chat.last().unwrap().user_round, 15);
        assert!(page.has_more);
        assert_eq!(page.next_before, Some(4));
        assert_eq!(page.chat.last().unwrap().user.content, "fixture input 14");
        let older = db
            .thread_view_page(
                "fixture-user",
                "fixture-thread",
                "chat",
                page.next_before,
                12,
            )
            .unwrap();
        assert_eq!(older.chat.len(), 3);
        assert!(!older.has_more);
        let body = page.chat.last().unwrap().assistant.body.as_ref().unwrap();
        let mut cursor = 0;
        let mut full = String::new();
        loop {
            let chunk = db
                .thread_body_page(
                    "fixture-user",
                    "fixture-thread",
                    &body.body_id,
                    body.revision,
                    cursor,
                )
                .unwrap();
            assert!(serde_json::to_vec(&chunk).unwrap().len() < 128 * 1024);
            full.push_str(&chunk.chunks.concat());
            if !chunk.has_more {
                break;
            }
            cursor = chunk.next_cursor.unwrap();
        }
        assert_eq!(full, "😀fixture answer".repeat(700));
        assert!(db
            .thread_body_page(
                "fixture-other",
                "fixture-thread",
                &body.body_id,
                body.revision,
                0
            )
            .unwrap()
            .chunks
            .is_empty());
        let trajectory = db
            .thread_view_page("fixture-user", "fixture-thread", "trajectory", None, 10)
            .unwrap();
        assert_eq!(trajectory.trajectory.len(), 10);
        assert!(trajectory.has_more);
        assert!(trajectory
            .trajectory
            .last()
            .unwrap()
            .detail
            .as_ref()
            .unwrap()
            .body_id
            .ends_with("|source_detail"));
        assert!(
            !db.thread_view_changes("fixture-user", "fixture-thread", -page.view_seq - 1)
                .unwrap()
                .reset_required
        );
        conn.execute("UPDATE thread_view_heads SET projection_state='ready'", [])
            .unwrap();
        assert!(
            db.thread_view_changes("fixture-user", "fixture-thread", -page.view_seq - 1)
                .unwrap()
                .reset_required
        );
        assert_eq!(
            conn.query_row("SELECT view_seq FROM thread_view_heads", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            before
        );
    }

    #[test]
    fn source_stream_pages_preserve_unicode_and_every_block() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let receipt = db
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"fixture input"}),
            )
            .unwrap();
        let turn = receipt["turn_id"].as_str().unwrap();
        let id = format!("{turn}:text-1");
        db.append_thread_item("fixture-user",&json!({"session_id":"fixture-thread","turn_id":turn,"item_id":id,"kind":"assistant_message","content":"","status":"running"})).unwrap();
        let blocks = [
            "😀fixture first".repeat(400),
            "fixture second".into(),
            "".into(),
            "😀fixture last".repeat(200),
        ];
        for (index, content) in blocks.iter().enumerate() {
            db.upsert_thread_text_block("fixture-user","fixture-thread",&json!({"item_id":id,"field":"content","block_index":index,"event_id":index+1,"data":{"content":content}})).unwrap();
        }
        let conn = db.open().unwrap();
        conn.execute(
            "UPDATE thread_view_heads SET projection_state='rebuilding'",
            [],
        )
        .unwrap();
        let page = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        let message = &page.chat[0].assistant;
        let body = message.body.as_ref().unwrap();
        assert!(body.body_id.ends_with("|source_stream"));
        assert!(body.bytes > message.content.len());
        let mut cursor = 0;
        let mut full = String::new();
        let mut reads = 0;
        loop {
            let chunk = db
                .thread_body_page(
                    "fixture-user",
                    "fixture-thread",
                    &body.body_id,
                    body.revision,
                    cursor,
                )
                .unwrap();
            full.push_str(&chunk.chunks.concat());
            reads += 1;
            assert!(reads < 20);
            if !chunk.has_more {
                break;
            }
            let next = chunk.next_cursor.unwrap();
            assert!(next > cursor);
            cursor = next;
        }
        assert_eq!(full, blocks.concat());
        conn.execute(
            "UPDATE thread_items SET visibility='internal' WHERE item_id=?",
            [id],
        )
        .unwrap();
        assert!(db
            .thread_body_page(
                "fixture-user",
                "fixture-thread",
                &body.body_id,
                body.revision,
                0
            )
            .unwrap()
            .chunks
            .is_empty());
    }

    #[test]
    fn historical_discovery_is_bounded_resumable_and_reads_do_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.db").to_string_lossy().into_owned();
        let db = SqliteStorage::new(path.clone());
        db.ensure_initialized().unwrap();
        {
            let mut c = db.open().unwrap();
            let tx = c.transaction().unwrap();
            for n in 0..130 {
                tx.execute("INSERT INTO thread_logs(session_id,user_id,latest_user_turn,latest_change_seq,created_time,updated_time) VALUES(?,'fixture-user',1,1,0,0)", [format!("fixture-{n:04}")]).unwrap();
            }
            tx.commit().unwrap();
        }
        // Reopening performs schema checks, not a history-wide discovery.
        let db = SqliteStorage::new(path.clone());
        db.ensure_initialized().unwrap();
        let heads = |db: &SqliteStorage| {
            db.open()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM thread_view_heads", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap()
        };
        assert_eq!(heads(&db), 0);
        let page = db
            .thread_view_page("fixture-user", "fixture-0000", "chat", None, 12)
            .unwrap();
        assert_eq!(page.projection_state, "source");
        assert_eq!(
            heads(&db),
            0,
            "a display read must never initialize a writable head"
        );
        let first = db.pending_thread_view_rebuilds().unwrap();
        assert_eq!(first.len(), 8);
        assert_eq!(heads(&db), 64);
        let db = SqliteStorage::new(path);
        let second = db.pending_thread_view_rebuilds().unwrap();
        assert_eq!(second.len(), 8);
        assert!(
            first.iter().all(|a| second.iter().all(|b| a.1 != b.1)),
            "large threads must not starve later pending threads"
        );
        assert_eq!(
            heads(&db),
            128,
            "discovery resumes from its durable keyset cursor"
        );
        db.pending_thread_view_rebuilds().unwrap();
        assert_eq!(heads(&db), 130);
        db.pending_thread_view_rebuilds().unwrap();
        assert_eq!(heads(&db), 130);
    }
    #[test]
    fn thread_view_preserves_user_identity_and_bounded_complete_bodies() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let receipt = db
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"fixture input","client_message_id":"fixture-client"}),
            )
            .unwrap();
        let turn = receipt["turn_id"].as_str().unwrap();
        let id = format!("{turn}:text-1");
        let content = "\u{1f600}\n\"\u{0001}".repeat(60000);
        db.append_thread_item("fixture-user",&json!({"session_id":"fixture-thread","turn_id":turn,"item_id":id,"kind":"assistant_message","content":content,"status":"completed"})).unwrap();
        for n in 0..32 {
            db.append_thread_item("fixture-user",&json!({"session_id":"fixture-thread","turn_id":turn,"item_id":format!("fixture-tool-{n}"),"kind":"tool_call","status":"failed","args":{"value":"fixture"}})).unwrap();
        }
        let page = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.projection_state, "ready");
        assert_eq!(page.thread_empty, Some(false));
        assert_eq!(page.chat.len(), 1);
        let row = &page.chat[0];
        assert_eq!(row.user.content, "fixture input");
        assert_eq!(row.workflow_count, 32);
        assert_eq!(row.failed_count, 32);
        assert!(serde_json::to_vec(&page).unwrap().len() < CHAT_RESPONSE_BYTES);
        let body = row.assistant.body.as_ref().unwrap();
        let mut cursor = 0;
        let mut full = String::new();
        loop {
            let p = db
                .thread_body_page(
                    "fixture-user",
                    "fixture-thread",
                    &body.body_id,
                    body.revision,
                    cursor,
                )
                .unwrap();
            assert!(serde_json::to_vec(&p).unwrap().len() < 128 * 1024);
            for c in p.chunks {
                assert!(c.len() <= BODY_CHUNK_BYTES);
                full.push_str(&c);
            }
            if !p.has_more {
                break;
            }
            cursor = p.next_cursor.unwrap();
        }
        assert_eq!(full, content);
        let search = db
            .thread_view_search("fixture-user", "fixture-thread", "fixture")
            .unwrap();
        assert!(!search.hits.is_empty());
        assert!(db
            .thread_view_search("fixture-other", "fixture-thread", "fixture")
            .unwrap()
            .hits
            .is_empty());
        let denied = db
            .thread_body_page(
                "fixture-other",
                "fixture-thread",
                &body.body_id,
                body.revision,
                0,
            )
            .unwrap();
        assert!(denied.chunks.is_empty());
        let trajectory = db
            .thread_view_page("fixture-user", "fixture-thread", "trajectory", None, 80)
            .unwrap();
        assert!(db
            .thread_view_page("fixture-other", "fixture-thread", "trajectory", None, 80)
            .unwrap()
            .tiles
            .is_empty());
        assert_eq!(trajectory.trajectory.len(), 34);
        assert!(trajectory
            .trajectory
            .iter()
            .all(|row| row.root_turn_id == turn));
        assert!(trajectory.trajectory.iter().all(|row| row.detail.is_some()));
        assert!(trajectory.trajectory.iter().all(|row| !row.overview_only));
        assert!(trajectory
            .trajectory
            .iter()
            .any(|row| row.kind == "user_message"));
        assert!(trajectory
            .trajectory
            .iter()
            .any(|row| row.kind == "tool_call"));
        assert!(trajectory
            .trajectory
            .iter()
            .any(|row| row.kind == "assistant_message"));
        assert!(
            trajectory.tiles.len() <= 1024,
            "source overview reads only bounded precomputed tiles"
        );
        let details = db
            .thread_view_trajectory_page("fixture-user", "fixture-thread", None, Some(turn), 80)
            .unwrap();
        assert_eq!(details.trajectory.len(), 34);
        assert!(details.trajectory.iter().all(|row| !row.overview_only));
        assert!(details.has_more == false);
        let cursor = page.view_seq;
        assert!(db
            .thread_view_changes("fixture-user", "fixture-thread", cursor)
            .unwrap()
            .rows
            .is_empty());
        db.update_thread_turn(
            "fixture-user",
            "fixture-thread",
            turn,
            "completed",
            "",
            &json!({}),
        )
        .unwrap();
        assert_eq!(
            db.thread_view_changes("fixture-user", "fixture-thread", cursor)
                .unwrap()
                .rows[0]
                .status,
            "completed"
        );
        let conn = db.open().unwrap();
        let plan:String=conn.query_row("EXPLAIN QUERY PLAN SELECT payload FROM thread_chat_rows WHERE user_id='fixture-user' AND session_id='fixture-thread' AND user_round<100 ORDER BY user_round DESC LIMIT 13",[],|r|r.get(3)).unwrap();
        assert!(plan.contains("idx_thread_chat_page"));
    }
    #[test]
    fn thread_view_stream_blocks_are_idempotent_and_rebuild_is_seek_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let receipt = db
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"fixture input"}),
            )
            .unwrap();
        let turn = receipt["turn_id"].as_str().unwrap();
        let id = format!("{turn}:text-1");
        db.append_thread_item("fixture-user",&json!({"session_id":"fixture-thread","turn_id":turn,"item_id":id,"kind":"assistant_message","status":"running"})).unwrap();
        for n in 0..2 {
            let b = json!({"item_id":id,"field":"content","block_index":n,"event_id":n+1,"data":{"content":"fixture delta","content_offset":n*13}});
            db.upsert_thread_text_block("fixture-user", "fixture-thread", &b)
                .unwrap();
            db.upsert_thread_text_block("fixture-user", "fixture-thread", &b)
                .unwrap();
        }
        let p = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(p.chat[0].assistant.content, "fixture deltafixture delta");
        let body = p.chat[0].assistant.body.as_ref().unwrap();
        let full = db
            .thread_body_page("fixture-user", "fixture-thread", &body.body_id, 0, 0)
            .unwrap()
            .chunks
            .concat();
        assert_eq!(full, "fixture deltafixture delta");
        let (cursor, more) = db
            .rebuild_thread_view_page("fixture-user", "fixture-thread", 0)
            .unwrap();
        assert_eq!(cursor, -1);
        assert!(more);
        let (_, more) = db
            .rebuild_thread_view_page("fixture-user", "fixture-thread", cursor)
            .unwrap();
        assert!(!more);
        let p = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(p.chat[0].user.content, "fixture input");
        assert_eq!(p.chat[0].assistant.content, "fixture deltafixture delta");
    }
    #[test]
    fn thread_view_feedback_keeps_body_version_and_visibility_revokes_access() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let receipt = db
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"fixture input"}),
            )
            .unwrap();
        let turn = receipt["turn_id"].as_str().unwrap();
        let id = format!("{turn}:text-1");
        db.append_thread_item("fixture-user",&json!({"session_id":"fixture-thread","turn_id":turn,"item_id":id,"kind":"assistant_message","content":"fixture searchable answer","status":"completed"})).unwrap();
        let before = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        let body = before.chat[0].assistant.body.clone().unwrap();
        db.set_thread_item_feedback("fixture-user", "fixture-thread", &id, "up")
            .unwrap();
        let after = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(
            after.chat[0].assistant.body.as_ref().unwrap().revision,
            body.revision
        );
        assert_eq!(
            after.chat[0].assistant.feedback.as_ref().unwrap()["vote"],
            "up"
        );
        assert!(!db
            .thread_view_search("fixture-user", "fixture-thread", "searchable")
            .unwrap()
            .hits
            .is_empty());
        db.append_thread_item("fixture-user",&json!({"session_id":"fixture-thread","turn_id":turn,"item_id":id,"kind":"assistant_message","content":"fixture searchable answer","visibility":"internal","status":"completed"})).unwrap();
        assert!(db
            .thread_body_page(
                "fixture-user",
                "fixture-thread",
                &body.body_id,
                body.revision,
                0
            )
            .unwrap()
            .chunks
            .is_empty());
        assert!(db
            .thread_view_search("fixture-user", "fixture-thread", "searchable")
            .unwrap()
            .hits
            .is_empty());
        assert!(db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap()
            .chat[0]
            .assistant
            .item_id
            .is_empty());
    }

    #[test]
    fn thread_view_delayed_blocks_cannot_replace_a_newer_or_sealed_answer() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let turn = db
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"fixture input"}),
            )
            .unwrap();
        let turn = turn["turn_id"].as_str().unwrap();
        let old = format!("{turn}:text-1");
        let latest = format!("{turn}:text-2");
        for (id, content) in [(&old, "fixture old"), (&latest, "fixture sealed")] {
            db.append_thread_item("fixture-user",&serde_json::json!({"session_id":"fixture-thread","turn_id":turn,"item_id":id,"kind":"assistant_message","content":content,"status":"completed"})).unwrap();
        }
        for id in [&old, &latest] {
            db.upsert_thread_text_block("fixture-user","fixture-thread",&serde_json::json!({"item_id":id,"field":"content","block_index":0,"event_id":99,"data":{"content":"fixture late","content_offset":0}})).unwrap();
        }
        let page = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(page.chat[0].assistant.content, "fixture sealed");
        assert_eq!(page.chat[0].assistant.item_id, latest);
    }

    #[test]
    fn tool_call_assistant_prose_stays_in_process_until_terminal_answer() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let receipt = db
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"fixture input"}),
            )
            .unwrap();
        let turn = receipt["turn_id"].as_str().unwrap();
        let intermediate = format!("{turn}:text-1");
        db.append_thread_item(
            "fixture-user",
            &json!({
                "session_id":"fixture-thread", "turn_id":turn,
                "item_id":intermediate, "kind":"assistant_message",
                "status":"completed", "content":"fixture intermediate"
            }),
        )
        .unwrap();
        // The stream can publish prose first and attach tool_calls in a later
        // revision.  That transition must revoke the provisional answer slot.
        let provisional = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(provisional.chat[0].assistant.item_id, intermediate);
        db.append_thread_item(
            "fixture-user",
            &json!({
                "session_id":"fixture-thread", "turn_id":turn,
                "item_id":intermediate, "kind":"assistant_message",
                "status":"completed", "content":"fixture intermediate",
                "tool_calls":[{"id":"call-1","type":"function","function":{"name":"fixture_tool","arguments":"{}"}}]
            }),
        )
        .unwrap();
        let during_tool = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert!(during_tool.chat[0].assistant.item_id.is_empty());
        assert!(during_tool.chat[0].assistant.content.is_empty());
        let process = db
            .thread_view_trajectory_page("fixture-user", "fixture-thread", None, Some(turn), 12)
            .unwrap();
        assert!(process
            .trajectory
            .iter()
            .any(|row| row.item_id == intermediate && row.summary == "fixture intermediate"));

        db.append_thread_item(
            "fixture-user",
            &json!({
                "session_id":"fixture-thread", "turn_id":turn,
                "item_id":format!("{turn}:tool-1"), "kind":"tool_call",
                "status":"completed", "tool":"fixture_tool"
            }),
        )
        .unwrap();
        let final_id = format!("{turn}:text-2");
        db.append_thread_item(
            "fixture-user",
            &json!({
                "session_id":"fixture-thread", "turn_id":turn,
                "item_id":final_id, "kind":"assistant_message",
                "status":"completed", "content":"fixture final"
            }),
        )
        .unwrap();
        let settled = db
            .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
            .unwrap();
        assert_eq!(settled.chat[0].assistant.item_id, final_id);
        assert_eq!(settled.chat[0].assistant.content, "fixture final");
    }

    #[test]
    #[ignore = "explicit disk benchmark: 500000 fixture rows"]
    fn thread_view_disk_read_comparison() {
        use crate::storage::ThreadSnapshotWindow;
        use std::time::Instant;
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        db.ensure_initialized().unwrap();
        let mut conn = db.open().unwrap();
        let mut prior = 0i64;
        for count in [20000i64, 200000, 500000] {
            let tx = conn.transaction().unwrap();
            tx.execute("INSERT INTO thread_logs(session_id,user_id,latest_user_turn,latest_change_seq,created_time,updated_time) VALUES('fixture-thread','fixture-user',0,0,0,0) ON CONFLICT(session_id) DO NOTHING",[]).unwrap();
            {
                let mut turn=tx.prepare("INSERT INTO thread_turns(session_id,turn_id,user_id,root_turn_id,trigger_kind,user_turn_index,status,summary,payload,created_time,updated_time) VALUES('fixture-thread',?,'fixture-user',?,'user',?,'completed','','{}',?,?)").unwrap();
                let mut item=tx.prepare("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,user_id,visibility,item_index,kind,status,payload,created_time,updated_time,created_seq) VALUES('fixture-thread',?,?,?,'fixture-user','user',?,?,'completed',?,?,?,?)").unwrap();
                let mut chat=tx.prepare("INSERT INTO thread_chat_rows(session_id,user_id,root_turn_id,user_round,revision,payload) VALUES('fixture-thread','fixture-user',?,?,?,?)").unwrap();
                let mut trajectory=tx.prepare("INSERT INTO thread_trajectory_rows(session_id,user_id,item_id,root_turn_id,created_seq,revision,status,payload) VALUES('fixture-thread','fixture-user',?,?,?,1,'completed',?)").unwrap();
                for n in prior..count {
                    let round = n / 100 + 1;
                    let root = format!("fixture-turn-{round}");
                    let id = format!("fixture-item-{n}");
                    let index = n % 100;
                    let payload=json!({"content":"fixture text ".repeat(64),"role":if index==0{"user"}else{"assistant"}}).to_string();
                    if index == 0 {
                        turn.execute(rusqlite::params![root, root, round, n as f64, n as f64])
                            .unwrap();
                        let row = ChatViewRow {
                            root_turn_id: root.clone(),
                            user_round: round,
                            revision: round,
                            status: "completed".into(),
                            user: ViewMessage {
                                item_id: id.clone(),
                                content: "fixture input".into(),
                                ..Default::default()
                            },
                            assistant: ViewMessage {
                                item_id: format!("{root}:text-1"),
                                content: "fixture answer".into(),
                                ..Default::default()
                            },
                            workflow_count: 98,
                            ..Default::default()
                        };
                        chat.execute(rusqlite::params![
                            root,
                            round,
                            round,
                            serde_json::to_string(&row).unwrap()
                        ])
                        .unwrap();
                    }
                    item.execute(rusqlite::params![
                        id,
                        root,
                        root,
                        index,
                        if index == 0 {
                            "user_message"
                        } else {
                            "tool_call"
                        },
                        payload,
                        n as f64,
                        n as f64,
                        n + 1
                    ])
                    .unwrap();
                    let row = TrajectoryViewRow {
                        item_id: id.clone(),
                        root_turn_id: root.clone(),
                        turn_id: root.clone(),
                        created_seq: n + 1,
                        kind: "tool_call".into(),
                        summary: "fixture summary".into(),
                        status: "completed".into(),
                        ..Default::default()
                    };
                    trajectory
                        .execute(rusqlite::params![
                            id,
                            root,
                            n + 1,
                            serde_json::to_string(&row).unwrap()
                        ])
                        .unwrap();
                }
            }
            tx.execute("UPDATE thread_logs SET latest_user_turn=?,latest_change_seq=? WHERE session_id='fixture-thread'",rusqlite::params![count/100,count]).unwrap();
            tx.execute("INSERT INTO thread_view_heads(session_id,user_id,latest_round,latest_item_seq,view_seq) VALUES('fixture-thread','fixture-user',?,?,?) ON CONFLICT(session_id) DO UPDATE SET latest_round=excluded.latest_round,latest_item_seq=excluded.latest_item_seq,view_seq=excluded.view_seq",rusqlite::params![count/100,count,count]).unwrap();
            tx.commit().unwrap();
            let window = ThreadSnapshotWindow {
                max_turns: 12,
                max_items: 2000,
                max_blocks: 4000,
                before_turn_index: None,
            };
            db.thread_snapshot_window("fixture-user", "fixture-thread", &window)
                .unwrap();
            db.thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
                .unwrap();
            let mut old = vec![];
            let mut new = vec![];
            let mut trace = vec![];
            let mut old_bytes = 0;
            let mut new_bytes = 0;
            let mut trajectory_bytes = 0;
            for _ in 0..30 {
                let t = Instant::now();
                let p = db
                    .thread_snapshot_window("fixture-user", "fixture-thread", &window)
                    .unwrap();
                old_bytes = serde_json::to_vec(&p).unwrap().len();
                old.push(t.elapsed().as_secs_f64() * 1000.);
                let t = Instant::now();
                let p = db
                    .thread_view_page("fixture-user", "fixture-thread", "chat", None, 12)
                    .unwrap();
                new_bytes = serde_json::to_vec(&p).unwrap().len();
                new.push(t.elapsed().as_secs_f64() * 1000.);
                let t = Instant::now();
                let p = db
                    .thread_view_page("fixture-user", "fixture-thread", "trajectory", None, 80)
                    .unwrap();
                trajectory_bytes = serde_json::to_vec(&p).unwrap().len();
                trace.push(t.elapsed().as_secs_f64() * 1000.);
            }
            for v in [&mut old, &mut new, &mut trace] {
                v.sort_by(f64::total_cmp);
            }
            println!(
                "{}",
                json!({"items":count,"samples":30,"old_p50_ms":old[14],"old_p95_ms":old[28],"chat_p50_ms":new[14],"chat_p95_ms":new[28],"trajectory_p95_ms":trace[28],"old_bytes":old_bytes,"chat_bytes":new_bytes,"trajectory_bytes":trajectory_bytes})
            );
            prior = count;
        }
    }
}
