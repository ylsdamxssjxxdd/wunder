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

pub(super) const SEARCH_SCHEMA:&str="CREATE VIRTUAL TABLE IF NOT EXISTS thread_body_search USING fts5(scope,body_key,session_id UNINDEXED,user_id UNINDEXED,body_id UNINDEXED,revision UNINDEXED,chunk_index UNINDEXED,content,tokenize='unicode61');";
fn search_scope(user: &str, session: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("s{:x}",Sha256::digest(format!("{user}|{session}").as_bytes()))
}
fn body_key(id:&str)->String {use sha2::{Digest,Sha256};format!("b{:x}",Sha256::digest(id.as_bytes()))}
pub(super) struct Db<'a>(pub &'a Connection);
impl ViewDb for Db<'_> {
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
        let conn = self.view_connection()?;
        let mut stmt=conn.prepare("SELECT user_id,session_id,rebuild_after FROM thread_view_heads WHERE projection_state='rebuilding' ORDER BY ABS(rebuild_after),session_id LIMIT 8")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub(super) fn view_page(
        &self,
        u: &str,
        s: &str,
        k: &str,
        b: Option<i64>,
        l: usize,
    ) -> Result<ThreadViewPage> {
        let mut conn = self.view_connection()?;
        let tx = conn.transaction()?;
        let page = view::page(&mut Db(&tx), u, s, k, b, l)?;
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
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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
        assert_eq!(trajectory.tiles.iter().map(|t| t.failed).sum::<i64>(), 32);
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
        let dir=tempfile::tempdir().unwrap();
        let db=SqliteStorage::new(dir.path().join("fixture.db").to_string_lossy().into());
        let turn=db.accept_thread_turn("fixture-user","fixture-thread",&serde_json::json!({"content":"fixture input"})).unwrap();
        let turn=turn["turn_id"].as_str().unwrap();
        let old=format!("{turn}:text-1");let latest=format!("{turn}:text-2");
        for (id,content) in [(&old,"fixture old"),(&latest,"fixture sealed")] {
            db.append_thread_item("fixture-user",&serde_json::json!({"session_id":"fixture-thread","turn_id":turn,"item_id":id,"kind":"assistant_message","content":content,"status":"completed"})).unwrap();
        }
        for id in [&old,&latest] {
            db.upsert_thread_text_block("fixture-user","fixture-thread",&serde_json::json!({"item_id":id,"field":"content","block_index":0,"event_id":99,"data":{"content":"fixture late","content_offset":0}})).unwrap();
        }
        let page=db.thread_view_page("fixture-user","fixture-thread","chat",None,12).unwrap();
        assert_eq!(page.chat[0].assistant.content,"fixture sealed");
        assert_eq!(page.chat[0].assistant.item_id,latest);
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
