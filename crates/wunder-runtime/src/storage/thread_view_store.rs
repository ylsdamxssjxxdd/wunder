//! Small transactional display tables. Reads never decode execution payloads.
use super::thread_view_projection as projection;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use wunder_core::thread_view::*;

pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS thread_view_heads (
 session_id TEXT PRIMARY KEY, user_id TEXT NOT NULL, epoch BIGINT NOT NULL DEFAULT 1,
 view_seq BIGINT NOT NULL DEFAULT 0, latest_round BIGINT NOT NULL DEFAULT 0,
 runtime_status TEXT NOT NULL DEFAULT 'idle',active_turn_id TEXT NOT NULL DEFAULT '',
 latest_item_seq BIGINT NOT NULL DEFAULT 0, rebuild_after BIGINT NOT NULL DEFAULT 0,
 rebuild_block_item TEXT NOT NULL DEFAULT '', rebuild_block_field TEXT NOT NULL DEFAULT '', rebuild_block_index BIGINT NOT NULL DEFAULT -1,
 projection_state TEXT NOT NULL DEFAULT 'ready'
);
CREATE TABLE IF NOT EXISTS thread_chat_rows (
 session_id TEXT NOT NULL, user_id TEXT NOT NULL, root_turn_id TEXT NOT NULL,
 user_round BIGINT NOT NULL, revision BIGINT NOT NULL, payload TEXT NOT NULL,
 PRIMARY KEY(session_id,root_turn_id)
);
CREATE INDEX IF NOT EXISTS idx_thread_active_view ON thread_turns(user_id,session_id,user_turn_index DESC,created_time DESC) WHERE status IN ('queued','running','waiting_input','waiting_approval');
CREATE INDEX IF NOT EXISTS idx_thread_execution_view ON thread_turns(user_id,session_id,root_turn_id,created_time DESC,turn_id DESC);
CREATE INDEX IF NOT EXISTS idx_thread_chat_page ON thread_chat_rows(user_id,session_id,user_round DESC);
CREATE INDEX IF NOT EXISTS idx_thread_chat_changes ON thread_chat_rows(user_id,session_id,revision);
CREATE INDEX IF NOT EXISTS idx_thread_view_rebuild ON thread_view_heads(projection_state,session_id);
CREATE INDEX IF NOT EXISTS idx_thread_blocks_view_rebuild ON thread_item_blocks(user_id,session_id,item_id,field,block_index);
CREATE INDEX IF NOT EXISTS idx_thread_items_view_rebuild ON thread_items(user_id,session_id,created_seq);
CREATE TABLE IF NOT EXISTS thread_trajectory_rows (
 session_id TEXT NOT NULL, user_id TEXT NOT NULL, item_id TEXT NOT NULL,
 root_turn_id TEXT NOT NULL, created_seq BIGINT NOT NULL, revision BIGINT NOT NULL,
 status TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(session_id,item_id)
);
CREATE INDEX IF NOT EXISTS idx_thread_trajectory_page ON thread_trajectory_rows(user_id,session_id,created_seq DESC);
CREATE INDEX IF NOT EXISTS idx_thread_trajectory_root ON thread_trajectory_rows(user_id,session_id,root_turn_id,created_seq DESC);
CREATE TABLE IF NOT EXISTS thread_body_manifests (
 session_id TEXT NOT NULL, user_id TEXT NOT NULL, body_id TEXT NOT NULL, revision BIGINT NOT NULL,
 fingerprint TEXT NOT NULL, PRIMARY KEY(session_id,body_id)
);
CREATE TABLE IF NOT EXISTS thread_body_chunks (
 session_id TEXT NOT NULL, user_id TEXT NOT NULL, body_id TEXT NOT NULL, revision BIGINT NOT NULL,
 chunk_index BIGINT NOT NULL, content TEXT NOT NULL,
 PRIMARY KEY(session_id,body_id,revision,chunk_index)
);
CREATE TABLE IF NOT EXISTS thread_trajectory_tiles (
 session_id TEXT NOT NULL, level BIGINT NOT NULL, bucket BIGINT NOT NULL,
 count BIGINT NOT NULL, failed BIGINT NOT NULL, running BIGINT NOT NULL,
 start DOUBLE PRECISION NOT NULL, finish DOUBLE PRECISION NOT NULL,
 PRIMARY KEY(session_id,level,bucket)
);
"#;

/// All SQL below uses positional `?` parameters; the PG adapter numbers them.
pub(crate) trait ViewDb {
    fn query(&mut self, sql: &str, params: &[Value]) -> Result<Vec<Vec<Value>>>;
    fn execute(&mut self, sql: &str, params: &[Value]) -> Result<u64>;
    fn index_chunk(
        &mut self,
        user: &str,
        session: &str,
        id: &str,
        revision: i64,
        index: i64,
        content: &str,
    ) -> Result<()>;
    fn search(&mut self, user: &str, session: &str, query: &str) -> Result<ThreadSearchPage>;
}
fn one(db: &mut impl ViewDb, sql: &str, p: &[Value]) -> Result<Option<Vec<Value>>> {
    Ok(db.query(sql, p)?.into_iter().next())
}
fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn int(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}
fn float(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

pub(crate) fn initialize(db: &mut impl ViewDb, user: &str, session: &str) -> Result<()> {
    db.execute("INSERT INTO thread_view_heads(session_id,user_id,projection_state) SELECT session_id,user_id,CASE WHEN latest_change_seq>0 THEN 'rebuilding' ELSE 'ready' END FROM thread_logs WHERE session_id=? AND user_id=? ON CONFLICT(session_id) DO NOTHING", &[json!(session),json!(user)])?;
    Ok(())
}
fn head(db: &mut impl ViewDb, user: &str, session: &str) -> Result<i64> {
    initialize(db, user, session)?;
    db.execute(
        "UPDATE thread_view_heads SET view_seq=view_seq+1 WHERE session_id=? AND user_id=?",
        &[json!(session), json!(user)],
    )?;
    Ok(one(
        db,
        "SELECT view_seq FROM thread_view_heads WHERE session_id=? AND user_id=?",
        &[json!(session), json!(user)],
    )?
    .map(|r| int(&r[0]))
    .unwrap_or(0))
}

fn save_chat(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    row: &mut ChatViewRow,
    seq: i64,
) -> Result<()> {
    row.revision = seq;
    db.execute("INSERT INTO thread_chat_rows(session_id,user_id,root_turn_id,user_round,revision,payload) VALUES(?,?,?,?,?,?) ON CONFLICT(session_id,root_turn_id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload", &[json!(session),json!(user),json!(row.root_turn_id),json!(row.user_round),json!(seq),json!(serde_json::to_string(row)?)])?;
    Ok(())
}

/// Immutable body versions. A stream writes small durable blocks; full bodies
/// are chunked once when their item is committed, never selected by page reads.
fn save_body(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    id: &str,
    rev: i64,
    content: &str,
) -> Result<i64> {
    let fingerprint = format!("{:x}", Sha256::digest(content.as_bytes()));
    if let Some(m)=one(db,"SELECT revision,fingerprint FROM thread_body_manifests WHERE session_id=? AND user_id=? AND body_id=?",&[json!(session),json!(user),json!(id)])? {
        if text(&m[1])==fingerprint {return Ok(int(&m[0]));}
    }
    for (index, chunk) in chunks(content).enumerate() {
        db.execute("INSERT INTO thread_body_chunks(session_id,user_id,body_id,revision,chunk_index,content) VALUES(?,?,?,?,?,?) ON CONFLICT(session_id,body_id,revision,chunk_index) DO NOTHING", &[json!(session),json!(user),json!(id),json!(rev),json!(index as i64),json!(chunk)])?;
        db.index_chunk(user, session, id, rev, index as i64, chunk)?;
    }
    db.execute("INSERT INTO thread_body_manifests(session_id,user_id,body_id,revision,fingerprint) VALUES(?,?,?,?,?) ON CONFLICT(session_id,body_id) DO UPDATE SET revision=excluded.revision,fingerprint=excluded.fingerprint",&[json!(session),json!(user),json!(id),json!(rev),json!(fingerprint)])?;
    Ok(rev)
}

pub(crate) fn project_turn(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    turn: &str,
) -> Result<()> {
    let Some(t) = one(db,"SELECT root_turn_id,user_turn_index,client_message_id,status,created_time,updated_time,trigger_kind FROM thread_turns WHERE session_id=? AND user_id=? AND turn_id=?", &[json!(session),json!(user),json!(turn)])? else { return Ok(()); };
    let root = text(&t[0]);
    let root_record=one(db,"SELECT user_turn_index,client_message_id,created_time FROM thread_turns WHERE session_id=? AND user_id=? AND turn_id=?",&[json!(session),json!(user),json!(root)])?.unwrap_or_else(||vec![t[1].clone(),t[2].clone(),t[4].clone()]);
    let latest=one(db,"SELECT status,updated_time,turn_id FROM thread_turns WHERE session_id=? AND user_id=? AND root_turn_id=? ORDER BY created_time DESC,turn_id DESC LIMIT 1",&[json!(session),json!(user),json!(root)])?.unwrap_or_else(||vec![t[3].clone(),t[5].clone(),json!(turn)]);
    let existing = one(
        db,
        "SELECT payload FROM thread_chat_rows WHERE session_id=? AND root_turn_id=?",
        &[json!(session), json!(root)],
    )?;
    let mut row = if let Some(r) = existing {
        serde_json::from_str::<ChatViewRow>(text(&r[0]))?
    } else {
        projection::new_chat(
            root,
            int(&root_record[0]),
            root_record[1].as_str().map(str::to_string),
            text(&latest[0]),
            float(&root_record[2]),
            float(&latest[1]),
        )
    };
    row.execution_turn_id = text(&latest[2]).into();
    row.status = text(&latest[0]).into();
    row.updated_time = float(&latest[1]);
    let seq = head(db, user, session)?;
    if projection::is_running(&row.status)==0 {
        if let Some(body)=row.assistant.body.as_mut().filter(|b|b.body_id.ends_with("|stream") && b.revision==0) {
            // Freeze the committed partial output without concatenating its
            // chunks. Copy/read now binds to an immutable body version.
            db.execute("INSERT INTO thread_body_chunks(session_id,user_id,body_id,revision,chunk_index,content) SELECT session_id,user_id,body_id,?,chunk_index,content FROM thread_body_chunks WHERE session_id=? AND user_id=? AND body_id=? AND revision=0 ON CONFLICT DO NOTHING",&[json!(seq),json!(session),json!(user),json!(body.body_id)])?;
            body.revision=seq;
        }
    }
    save_chat(db, user, session, &mut row, seq)?;
    db.execute("UPDATE thread_view_heads SET latest_round=CASE WHEN latest_round<? THEN ? ELSE latest_round END WHERE session_id=?", &[json!(row.user_round),json!(row.user_round),json!(session)])?;
    let active=one(db,"SELECT status,turn_id FROM thread_turns WHERE user_id=? AND session_id=? AND status IN ('queued','running','waiting_input','waiting_approval') ORDER BY user_turn_index DESC,created_time DESC LIMIT 1",&[json!(user),json!(session)])?;
    let latest_root=one(db,"SELECT payload FROM thread_chat_rows WHERE user_id=? AND session_id=? ORDER BY user_round DESC LIMIT 1",&[json!(user),json!(session)])?.and_then(|r|serde_json::from_str::<ChatViewRow>(text(&r[0])).ok());
    let status=active.as_ref().map(|r|text(&r[0])).unwrap_or_else(||latest_root.as_ref().map(|r|r.status.as_str()).unwrap_or("idle"));
    let id=active.as_ref().map(|r|text(&r[1])).unwrap_or("");
    db.execute("UPDATE thread_view_heads SET runtime_status=?,active_turn_id=? WHERE session_id=? AND user_id=?",&[json!(status),json!(id),json!(session),json!(user)])?;
    Ok(())
}

pub(crate) fn project_item(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    id: &str,
) -> Result<()> {
    let Some(i)=one(db,"SELECT i.turn_id,i.root_turn_id,i.item_index,i.created_seq,i.revision,i.kind,i.status,i.visibility,i.payload,i.created_time,i.updated_time,t.user_turn_index FROM thread_items i JOIN thread_turns t ON t.session_id=i.session_id AND t.turn_id=i.turn_id WHERE i.session_id=? AND i.user_id=? AND i.item_id=?", &[json!(session),json!(user),json!(id)])? else { return Ok(()); };
    if text(&i[7]) != "user" {
        if let Some(old)=one(db,"SELECT status,created_seq FROM thread_trajectory_rows WHERE session_id=? AND user_id=? AND item_id=?",&[json!(session),json!(user),json!(id)])? {
            for level in 0..8 { db.execute("UPDATE thread_trajectory_tiles SET count=count-1,failed=failed-?,running=running-? WHERE session_id=? AND level=? AND bucket=?",&[json!(projection::is_failed(text(&old[0]))),json!(projection::is_running(text(&old[0]))),json!(session),json!(level),json!(int(&old[1])/(128i64 << (level*3)))])?; }
            db.execute("DELETE FROM thread_trajectory_rows WHERE session_id=? AND user_id=? AND item_id=?",&[json!(session),json!(user),json!(id)])?;
            for field in ["content","detail","stream"] {for table in ["thread_body_search","thread_body_chunks","thread_body_manifests"] {db.execute(&format!("DELETE FROM {table} WHERE session_id=? AND user_id=? AND body_id=?"),&[json!(session),json!(user),json!(projection::body_id(id,field))])?;}}
            if let Some(r)=one(db,"SELECT payload FROM thread_chat_rows WHERE session_id=? AND root_turn_id=?",&[json!(session),i[1].clone()])? {
                let mut row:ChatViewRow=serde_json::from_str(text(&r[0]))?;
                if row.user.item_id==id {row.user=ViewMessage::default();}
                if row.assistant.item_id==id {row.assistant=ViewMessage::default();}
                if matches!(text(&i[5]),"tool_call"|"approval"|"plan"|"compaction"|"queue"|"subagent_run") {
                    row.workflow_count=(row.workflow_count-1).max(0);
                    row.failed_count=(row.failed_count-projection::is_failed(text(&old[0]))).max(0);
                }
                let seq=head(db,user,session)?;save_chat(db,user,session,&mut row,seq)?;
            }
        }
        return Ok(());
    }
    let turn = text(&i[0]);
    let root = text(&i[1]);
    let revision = int(&i[4]);
    let kind = text(&i[5]);
    let status = text(&i[6]);
    let mut item: Value = serde_json::from_str(text(&i[8]))?;
    let old=one(db,"SELECT status,created_seq,revision FROM thread_trajectory_rows WHERE session_id=? AND item_id=?", &[json!(session),json!(id)])?;
    if old.as_ref().is_some_and(|r| int(&r[2]) >= revision) {
        return Ok(());
    }
    let seq = head(db, user, session)?;
    let mut traj = projection::trajectory(
        &item,
        id,
        turn,
        root,
        int(&i[11]),
        int(&i[2]),
        int(&i[3]),
        revision,
        kind,
        status,
        float(&i[9]),
        float(&i[10]),
    );
    let mut detail_item = std::mem::take(&mut item);
    let mut original_content = None;
    let mut content_revision = None;
    if let Some(map) = detail_item.as_object_mut() {
        map.remove("model_content");
        map.remove("config_overrides");
        if matches!(kind, "assistant_message" | "user_message") {
            if let Some(Value::String(content)) = map.remove("content") {
                let content_rev = save_body(
                    db,
                    user,
                    session,
                    &projection::body_id(id, "content"),
                    revision,
                    &content,
                )?;
                map.insert(
                    "content_body".into(),
                    json!(BodyRef {
                        body_id: projection::body_id(id, "content"),
                        revision: content_rev,
                        bytes: content.len()
                    }),
                );
                original_content = Some(content);
                content_revision = Some(content_rev);
            }
        }
    }
    let detail = serde_json::to_string(&detail_item)?;
    item = detail_item;
    if let Some(content) = original_content {
        if let Some(map) = item.as_object_mut() {
            map.insert("content".into(), Value::String(content));
            map.remove("content_body");
        }
    }
    if let Some(body) = &mut traj.detail {
        body.bytes = detail.len();
    }
    let detail_rev = save_body(
        db,
        user,
        session,
        &projection::body_id(id, "detail"),
        revision,
        &detail,
    )?;
    if let Some(body) = &mut traj.detail {
        body.revision = detail_rev;
    }
    db.execute("INSERT INTO thread_trajectory_rows(session_id,user_id,item_id,root_turn_id,created_seq,revision,status,payload) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(session_id,item_id) DO UPDATE SET revision=excluded.revision,status=excluded.status,payload=excluded.payload", &[json!(session),json!(user),json!(id),json!(root),json!(int(&i[3])),json!(revision),json!(status),json!(serde_json::to_string(&traj)?)])?;
    let old_status = old.as_ref().map(|r| text(&r[0]));
    for level in 0..8 {
        let bucket = int(&i[3]) / (128i64 << (level * 3));
        db.execute("INSERT INTO thread_trajectory_tiles(session_id,level,bucket,count,failed,running,start,finish) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(session_id,level,bucket) DO UPDATE SET count=thread_trajectory_tiles.count+excluded.count,failed=thread_trajectory_tiles.failed+excluded.failed,running=thread_trajectory_tiles.running+excluded.running,finish=CASE WHEN thread_trajectory_tiles.finish<excluded.finish THEN excluded.finish ELSE thread_trajectory_tiles.finish END", &[json!(session),json!(level),json!(bucket),json!(i64::from(old.is_none())),json!(projection::is_failed(status)-old_status.map(projection::is_failed).unwrap_or(0)),json!(projection::is_running(status)-old_status.map(projection::is_running).unwrap_or(0)),json!(float(&i[9])),json!(float(&i[10]))])?;
    }
    db.execute("UPDATE thread_view_heads SET latest_item_seq=CASE WHEN latest_item_seq<? THEN ? ELSE latest_item_seq END WHERE session_id=?", &[json!(int(&i[3])),json!(int(&i[3])),json!(session)])?;
    let Some(r) = one(
        db,
        "SELECT payload FROM thread_chat_rows WHERE session_id=? AND root_turn_id=?",
        &[json!(session), json!(root)],
    )?
    else {
        return Ok(());
    };
    let mut row: ChatViewRow = serde_json::from_str(text(&r[0]))?;
    let content = item.get("content").and_then(Value::as_str).unwrap_or("");
    if kind == "user_message" && turn == root {
        row.user = projection::message(id, content, revision);
        let content_rev = if let Some(rev) = content_revision {
            rev
        } else {
            save_body(
                db,
                user,
                session,
                &projection::body_id(id, "content"),
                revision,
                content,
            )?
        };
        if let Some(body) = &mut row.user.body {
            body.revision = content_rev;
        }
        row.user.attachments = item
            .get("attachments")
            .and_then(Value::as_array)
            .map(|v| {
                v.iter()
                    .take(8)
                    .map(|a| {
                        let mut out = serde_json::Map::new();
                        for key in ["name", "mime_type", "public_path"] {
                            if let Some(s) = a.get(key).and_then(Value::as_str) {
                                out.insert(key.into(), json!(preview(s, 128)));
                            }
                        }
                        Value::Object(out)
                    })
                    .collect()
            })
            .unwrap_or_default();
    } else if projection::stable_answer(id, turn, kind)
        && (row.assistant.item_id == id || int(&i[3]) >= row.assistant.created_seq)
    {
        // The latest committed execution output is the one answer for a root.
        let stream = row
            .assistant
            .body
            .as_ref()
            .is_some_and(|b| b.body_id.ends_with("|stream"));
        if !(content.is_empty() && row.assistant.item_id == id && stream) {
            row.assistant = projection::message(id, content, revision);
        }
        row.assistant.created_seq = int(&i[3]);
        row.assistant.durable_seq=one(db,"SELECT latest_change_seq FROM thread_logs WHERE session_id=? AND user_id=?",&[json!(session),json!(user)])?.map(|r|int(&r[0])).unwrap_or(0);
        if !row.assistant.body.as_ref().is_some_and(|b|b.body_id.ends_with("|stream")) {
            row.assistant.content_offset=content.encode_utf16().count();
        }
        if item
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|v| !v.is_empty())
        {
            row.assistant.content.clear();
        }
        row.assistant.feedback = item.get("feedback").cloned();
        row.assistant.stats = projection::finite_stats(&item);
        let content_rev = if let Some(rev) = content_revision {
            rev
        } else {
            save_body(
                db,
                user,
                session,
                &projection::body_id(id, "content"),
                revision,
                content,
            )?
        };
        if let Some(body) = &mut row.assistant.body {
            if !body.body_id.ends_with("|stream") {
                body.revision = content_rev;
            }
        }
    } else if kind == "assistant_message" {
        if let Some(stats) = projection::finite_stats(&item) {
            row.assistant.stats = Some(stats);
        }
    }
    if kind == "terminal" {
        row.assistant.failure = preview(
            item.pointer("/error/message")
                .and_then(Value::as_str)
                .or_else(|| item.get("content").and_then(Value::as_str))
                .unwrap_or(""),
            256,
        );
    }
    if matches!(
        kind,
        "tool_call" | "approval" | "plan" | "compaction" | "queue" | "subagent_run"
    ) {
        row.workflow_count += i64::from(old.is_none());
        row.failed_count +=
            projection::is_failed(status) - old_status.map(projection::is_failed).unwrap_or(0);
    }
    row.updated_time = float(&i[10]);
    save_chat(db, user, session, &mut row, seq)
}

/// Feedback changes never reread or duplicate a large immutable body.
pub(crate) fn project_feedback(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    id: &str,
    feedback: &Value,
) -> Result<()> {
    let Some(r)=one(db,"SELECT payload FROM thread_chat_rows WHERE session_id=? AND user_id=? AND root_turn_id=(SELECT root_turn_id FROM thread_items WHERE session_id=? AND user_id=? AND item_id=?)",&[json!(session),json!(user),json!(session),json!(user),json!(id)])?else{return Ok(());};
    let mut row: ChatViewRow = serde_json::from_str(text(&r[0]))?;
    if row.assistant.item_id == id {
        row.assistant.feedback = Some(feedback.clone());
        let seq = head(db, user, session)?;
        save_chat(db, user, session, &mut row, seq)?;
    }
    Ok(())
}

pub(crate) fn project_block(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    id: &str,
    field: &str,
    index: i64,
    block: &Value,
) -> Result<()> {
    if field != "content" {
        return Ok(());
    }
    let Some(i)=one(db,"SELECT turn_id,root_turn_id,kind,visibility,created_seq FROM thread_items WHERE session_id=? AND user_id=? AND item_id=?",&[json!(session),json!(user),json!(id)])? else {return Ok(());};
    if text(&i[3]) != "user" || !projection::stable_answer(id, text(&i[0]), text(&i[2])) {
        return Ok(());
    }
    let data = block.get("data").unwrap_or(block);
    let content = data.get("content").and_then(Value::as_str).unwrap_or("");
    let body_id = projection::body_id(id, "stream");
    let Some(r) = one(
        db,
        "SELECT payload FROM thread_chat_rows WHERE session_id=? AND root_turn_id=?",
        &[json!(session), json!(text(&i[1]))],
    )?
    else {
        return Ok(());
    };
    let mut row: ChatViewRow = serde_json::from_str(text(&r[0]))?;
    // Old executions and delayed blocks cannot replace a newer sealed answer.
    if int(&i[4])<row.assistant.created_seq || (row.assistant.item_id==id && row.assistant.body.as_ref().is_some_and(|b|b.bytes>0 && (!b.body_id.ends_with("|stream")||b.revision>0))) {
        return Ok(());
    }
    // Body block identities are stable across duplicate event delivery.
    let old_bytes=db.query("SELECT content FROM thread_body_chunks WHERE session_id=? AND body_id=? AND revision=0 AND chunk_index>=? AND chunk_index<?",&[json!(session),json!(body_id),json!(index*1024),json!((index+1)*1024)])?.iter().map(|r|text(&r[0]).len()).sum::<usize>();
    db.execute("DELETE FROM thread_body_chunks WHERE session_id=? AND body_id=? AND revision=0 AND chunk_index>=? AND chunk_index<?",&[json!(session),json!(body_id),json!(index*1024),json!((index+1)*1024)])?;
    for (n, c) in chunks(content).enumerate() {
        db.execute("INSERT INTO thread_body_chunks(session_id,user_id,body_id,revision,chunk_index,content) VALUES(?,?,?,0,?,?) ON CONFLICT(session_id,body_id,revision,chunk_index) DO UPDATE SET content=excluded.content",&[json!(session),json!(user),json!(body_id),json!(index*1024+n as i64),json!(c)])?;
    }
    if row.assistant.item_id != id {
        row.assistant = message_for_stream(id, &body_id);
    }
    let offset = data
        .get("content_offset")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    // Legacy durable offsets are UTF-16; preview is a byte bounded prefix.
    if offset as usize == row.assistant.content.encode_utf16().count()
        && row.assistant.content.len() < 2048
    {
        let remaining = 2048 - row.assistant.content.len();
        row.assistant.content.push_str(&preview(content, remaining));
    }
    let bytes = row
        .assistant
        .body
        .as_ref()
        .filter(|b| b.body_id == body_id)
        .map(|b| b.bytes)
        .unwrap_or(0)
        .saturating_sub(old_bytes)
        + content.len();
    row.assistant.body = Some(BodyRef {
        body_id,
        revision: 0,
        bytes,
    });
    row.assistant.created_seq=int(&i[4]);
    row.assistant.content_offset=row.assistant.content_offset.max(offset.max(0) as usize+content.encode_utf16().count());
    row.assistant.durable_seq=one(db,"SELECT latest_change_seq FROM thread_logs WHERE session_id=? AND user_id=?",&[json!(session),json!(user)])?.map(|r|int(&r[0])).unwrap_or(0);
    row.assistant.tail = preview(content, 2048);
    let seq = head(db, user, session)?;
    save_chat(db, user, session, &mut row, seq)
}
fn message_for_stream(id: &str, body: &str) -> ViewMessage {
    ViewMessage {
        item_id: id.into(),
        body: Some(BodyRef {
            body_id: body.into(),
            revision: 0,
            bytes: 0,
        }),
        ..Default::default()
    }
}

pub(crate) fn page(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    kind: &str,
    before: Option<i64>,
    limit: usize,
) -> Result<ThreadViewPage> {
    let mut page = ThreadViewPage {
        session_id: session.into(),
        epoch: 1,
        projection_state: "ready".into(),
        thread_empty: Some(true),
        ..Default::default()
    };
    let h=one(db,"SELECT epoch,view_seq,latest_round,latest_item_seq,projection_state,runtime_status,active_turn_id FROM thread_view_heads WHERE session_id=? AND user_id=?", &[json!(session),json!(user)])?;
    if let Some(h) = h {
        page.runtime_status=text(&h[5]).into();page.active_turn_id=text(&h[6]).into();
        page.epoch = int(&h[0]);
        page.view_seq = int(&h[1]);
        page.latest_round = int(&h[2]);
        page.projection_state = text(&h[4]).into();
        page.thread_empty = (page.projection_state == "ready").then_some(page.latest_round == 0);
        while int(&h[3]) / (128i64 << (page.tile_level * 3)) > 1023 && page.tile_level < 7 {
            page.tile_level += 1;
        }
    } else if one(
        db,
        "SELECT latest_user_turn FROM thread_logs WHERE session_id=? AND user_id=?",
        &[json!(session), json!(user)],
    )?
    .is_some_and(|r| int(&r[0]) > 0)
    {
        page.projection_state = "rebuilding".into();
        page.thread_empty = None;
    }
    let cap = if kind == "trajectory" {
        limit.clamp(1, TRAJECTORY_PAGE_MAX)
    } else {
        limit.clamp(1, CHAT_PAGE_MAX)
    };
    let cursor = before.unwrap_or(i64::MAX);
    let sql = if kind == "trajectory" {
        "SELECT payload,created_seq FROM thread_trajectory_rows WHERE user_id=? AND session_id=? AND created_seq<? ORDER BY created_seq DESC LIMIT ?"
    } else {
        "SELECT payload,user_round FROM thread_chat_rows WHERE user_id=? AND session_id=? AND user_round<? ORDER BY user_round DESC LIMIT ?"
    };
    let rows = db.query(
        sql,
        &[
            json!(user),
            json!(session),
            json!(cursor),
            json!((cap + 1) as i64),
        ],
    )?;
    page.has_more = rows.len() > cap;
    let mut bytes = 4096;
    for r in rows.into_iter().take(cap) {
        let size = text(&r[0]).len();
        let budget = if kind == "trajectory" {
            TRAJECTORY_RESPONSE_BYTES
        } else {
            CHAT_RESPONSE_BYTES
        };
        if bytes + size > budget {
            page.has_more = true;
            break;
        }
        bytes += size;
        page.next_before = Some(int(&r[1]));
        if kind == "trajectory" {
            page.trajectory.push(serde_json::from_str(text(&r[0]))?);
        } else {
            page.chat.push(serde_json::from_str(text(&r[0]))?);
        }
    }
    page.chat.reverse();
    page.trajectory.reverse();
    if kind == "trajectory" && page.projection_state == "ready" && page.view_seq > 0 {
        for r in db.query("SELECT bucket,count,failed,running,start,finish FROM thread_trajectory_tiles WHERE session_id=? AND level=? ORDER BY bucket LIMIT 1024", &[json!(session),json!(page.tile_level as i64)])? {
            let tile=TrajectoryTile {bucket:int(&r[0]),count:int(&r[1]),failed:int(&r[2]),running:int(&r[3]),start:float(&r[4]),end:float(&r[5])};
            let size=serde_json::to_vec(&tile)?.len()+1;if bytes+size>TRAJECTORY_RESPONSE_BYTES {break;}bytes+=size;page.tiles.push(tile);
        }
    }
    Ok(page)
}

pub(crate) fn changes(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    after: i64,
) -> Result<ThreadViewChanges> {
    let h=one(db,"SELECT view_seq,epoch,projection_state,runtime_status,active_turn_id FROM thread_view_heads WHERE session_id=? AND user_id=?", &[json!(session),json!(user)])?;
    let runtime_status=h.as_ref().map(|r|text(&r[3]).to_string()).unwrap_or_else(||"idle".into());
    let active_turn_id=h.as_ref().map(|r|text(&r[4]).to_string()).unwrap_or_default();
    let seq = h.as_ref().map(|r| int(&r[0])).unwrap_or(0);
    let epoch = h.as_ref().map(|r| int(&r[1])).unwrap_or(1);
    let ready = h.as_ref().is_none_or(|r| text(&r[2]) == "ready");
    let rows=db.query("SELECT payload FROM thread_chat_rows WHERE user_id=? AND session_id=? AND revision>? ORDER BY revision LIMIT 21", &[json!(user),json!(session),json!(after)])?;
    let reset = !ready
        || after > seq
        || rows.len() > 20
        || rows.iter().map(|r| text(&r[0]).len()).sum::<usize>() > 60 * 1024;
    Ok(ThreadViewChanges {
        runtime_status,active_turn_id,epoch,
        view_seq: seq,
        reset_required: reset,
        rows: if reset {
            vec![]
        } else {
            rows.into_iter()
                .map(|r| serde_json::from_str(text(&r[0])))
                .collect::<std::result::Result<_, _>>()?
        },
    })
}

pub(crate) fn body(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    id: &str,
    rev: i64,
    cursor: i64,
) -> Result<ThreadBodyPage> {
    ensure!(cursor >= 0 && rev >= 0, "invalid body cursor");
    let rows=db.query("SELECT chunk_index,content FROM thread_body_chunks WHERE session_id=? AND user_id=? AND body_id=? AND revision=? AND chunk_index>=? ORDER BY chunk_index LIMIT 9", &[json!(session),json!(user),json!(id),json!(rev),json!(cursor)])?;
    let mut page = ThreadBodyPage {
        body_id: id.into(),
        revision: rev,
        has_more: false,
        ..Default::default()
    };
    let mut bytes = 1024;
    for r in rows {
        let size = serde_json::to_string(&r[1])?.len();
        if bytes + size > 120 * 1024 || page.chunks.len() == 8 {
            page.has_more = true;
            break;
        }
        bytes += size;
        page.next_cursor = Some(int(&r[0]) + 1);
        page.chunks.push(text(&r[1]).into());
    }
    Ok(page)
}

/// Stable created_seq seek, one bounded transaction per background iteration.
pub(crate) fn rebuild(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    after: i64,
) -> Result<(i64, bool)> {
    let started=std::time::Instant::now();
    let budget=std::time::Duration::from_millis(40);
    if after < 0 {
        let position=one(db,"SELECT rebuild_block_item,rebuild_block_field,rebuild_block_index FROM thread_view_heads WHERE session_id=? AND user_id=?",&[json!(session),json!(user)])?.unwrap_or_else(||vec![json!(""),json!(""),json!(-1)]);
        let blocks=db.query("SELECT item_id,field,block_index FROM thread_item_blocks WHERE user_id=? AND session_id=? AND (item_id,field,block_index)>(?,?,?) ORDER BY item_id,field,block_index LIMIT 65",&[json!(user),json!(session),position[0].clone(),position[1].clone(),position[2].clone()])?;
        let cursor = after.saturating_neg() - 1;
        let mut more = blocks.len() > 64;
        let mut last = cursor;
        let mut position = position;
        let mut bytes=0;
        for r in blocks.into_iter().take(64) {
            if last>cursor && (started.elapsed()>=budget||bytes>=2*1024*1024){more=true;break;}
            let Some(payload)=one(db,"SELECT payload FROM thread_item_blocks WHERE session_id=? AND user_id=? AND item_id=? AND field=? AND block_index=?",&[json!(session),json!(user),r[0].clone(),r[1].clone(),r[2].clone()])?else{continue;};
            bytes+=text(&payload[0]).len();
            let block: Value = serde_json::from_str(text(&payload[0]))?;
            let committed=one(db,"SELECT payload FROM thread_chat_rows WHERE session_id=? AND user_id=? AND root_turn_id=(SELECT root_turn_id FROM thread_items WHERE session_id=? AND item_id=?)",&[json!(session),json!(user),json!(session),r[0].clone()])?;
            if committed.is_some_and(|c| {
                serde_json::from_str::<ChatViewRow>(text(&c[0]))
                    .ok()
                    .is_some_and(|row| {
                        row.assistant.item_id == text(&r[0])
                            && row
                                .assistant
                                .body
                                .is_none_or(|b| b.body_id.ends_with("|stream") || b.bytes == 0)
                    })
            }) {
                project_block(
                    db,
                    user,
                    session,
                    text(&r[0]),
                    text(&r[1]),
                    int(&r[2]),
                    &block,
                )?;
            }
            last += 1;
            position = vec![r[0].clone(), r[1].clone(), r[2].clone()];
        }
        let next = -(last + 1);
        db.execute("UPDATE thread_view_heads SET rebuild_after=?,projection_state=?,rebuild_block_item=?,rebuild_block_field=?,rebuild_block_index=? WHERE session_id=? AND user_id=?",&[json!(next),json!(if more{"rebuilding"}else{"ready"}),position[0].clone(),position[1].clone(),position[2].clone(),json!(session),json!(user)])?;
        return Ok((next, more));
    }
    let ids=db.query("SELECT item_id,turn_id,created_seq FROM thread_items WHERE user_id=? AND session_id=? AND created_seq>? ORDER BY created_seq LIMIT 65", &[json!(user),json!(session),json!(after)])?;
    let mut more = ids.len() > 64;
    let mut cursor = after;
    for r in ids.into_iter().take(64) {
        if cursor>after && started.elapsed()>=budget {more=true;break;}
        let root=one(db,"SELECT root_turn_id FROM thread_turns WHERE session_id=? AND user_id=? AND turn_id=?",&[json!(session),json!(user),r[1].clone()])?;
        if let Some(root)=root{if one(db,"SELECT root_turn_id FROM thread_chat_rows WHERE session_id=? AND root_turn_id=?",&[json!(session),root[0].clone()])?.is_none(){project_turn(db,user,session,text(&r[1]))?;}}
        project_item(db, user, session, text(&r[0]))?;
        cursor = int(&r[2]);
    }
    db.execute("UPDATE thread_view_heads SET rebuild_after=?,projection_state=? WHERE session_id=? AND user_id=?", &[json!(if more {cursor}else{-1}),json!("rebuilding"),json!(session),json!(user)])?;
    Ok((if more { cursor } else { -1 }, true))
}
