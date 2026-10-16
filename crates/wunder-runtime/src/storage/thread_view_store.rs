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
CREATE INDEX IF NOT EXISTS idx_thread_active_view_v2 ON thread_turns(user_id,session_id,user_turn_index DESC,created_time DESC) WHERE status IN ('accepted','queued','running','waiting_input','waiting_approval');
CREATE INDEX IF NOT EXISTS idx_thread_execution_view ON thread_turns(user_id,session_id,root_turn_id,created_time DESC,turn_id DESC);
CREATE INDEX IF NOT EXISTS idx_thread_chat_page ON thread_chat_rows(user_id,session_id,user_round DESC);
CREATE INDEX IF NOT EXISTS idx_thread_chat_changes ON thread_chat_rows(user_id,session_id,revision);
CREATE INDEX IF NOT EXISTS idx_thread_view_rebuild ON thread_view_heads(projection_state,session_id);
CREATE INDEX IF NOT EXISTS idx_thread_blocks_view_rebuild ON thread_item_blocks(user_id,session_id,item_id,field,block_index);
CREATE INDEX IF NOT EXISTS idx_thread_items_view_rebuild ON thread_items(user_id,session_id,created_seq);
CREATE INDEX IF NOT EXISTS idx_thread_items_view_root_seq ON thread_items(user_id,session_id,root_turn_id,created_seq DESC);
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
CREATE INDEX IF NOT EXISTS idx_thread_body_manifests_user_session
 ON thread_body_manifests(user_id,session_id);
CREATE TABLE IF NOT EXISTS thread_body_chunks (
 session_id TEXT NOT NULL, user_id TEXT NOT NULL, body_id TEXT NOT NULL, revision BIGINT NOT NULL,
 chunk_index BIGINT NOT NULL, content TEXT NOT NULL,
 PRIMARY KEY(session_id,body_id,revision,chunk_index)
);
CREATE INDEX IF NOT EXISTS idx_thread_body_chunks_user_session
 ON thread_body_chunks(user_id,session_id);
CREATE TABLE IF NOT EXISTS thread_trajectory_tiles (
 session_id TEXT NOT NULL, level BIGINT NOT NULL, bucket BIGINT NOT NULL,
 count BIGINT NOT NULL, failed BIGINT NOT NULL, running BIGINT NOT NULL,
 start DOUBLE PRECISION NOT NULL, finish DOUBLE PRECISION NOT NULL,
 input_count BIGINT NOT NULL DEFAULT 0, model_count BIGINT NOT NULL DEFAULT 0,
 tool_count BIGINT NOT NULL DEFAULT 0,
 PRIMARY KEY(session_id,level,bucket)
);
"#;

/// All SQL below uses positional `?` parameters; the PG adapter numbers them.
pub(crate) trait ViewDb {
    /// PostgreSQL and SQLite use the same bounded source query shape.
    fn is_postgres(&self) -> bool {
        false
    }
    fn json_text(&self, expression: &str, field: &str) -> String;
    fn text_bytes(&self, expression: &str) -> String;
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
    db.execute("INSERT INTO thread_view_heads(session_id,user_id,projection_state) SELECT session_id,user_id,CASE WHEN latest_change_seq=0 THEN 'ready' WHEN latest_change_seq>? THEN 'source' ELSE 'rebuilding' END FROM thread_logs WHERE session_id=? AND user_id=? ON CONFLICT(session_id) DO NOTHING", &[json!(MAX_SOURCE_CHANGE_SEQ),json!(session),json!(user)])?;
    Ok(())
}

/// Classification used by the bounded overview pyramid.  Keep this beside the
/// write projection so both current and historical readers share exactly the
/// same three-lane semantics as the trajectory UI.
fn tile_lane(kind: &str) -> (i64, i64, i64) {
    match kind {
        "tool_call" | "tool_result" | "tool_message" | "subagent" | "subagent_run" => (0, 0, 1),
        "assistant_message" | "compaction" | "compacted" => (0, 1, 0),
        _ => (1, 0, 0),
    }
}

fn trajectory_visibility(visibility: &str, kind: &str, payload: &Value) -> bool {
    if visibility == "user" || (visibility == "model_internal" && kind == "system_message") {
        return true;
    }
    // Model context is usually hidden from the chat transcript, but a small,
    // semantically named subset explains how an execution received a result
    // from another agent. It is safe to show as a bounded trajectory record;
    // arbitrary hidden prompts and tool observations remain private.
    visibility == "model_internal"
        && kind == "user_message"
        && payload.pointer("/meta/type").and_then(Value::as_str) == Some("model_context_internal")
        && payload.pointer("/meta/source").and_then(Value::as_str) == Some("agent_inbox")
}

/// Historical sessions above this bound stay on the durable source-log path.
/// Replaying millions of legacy items into presentation tables has no user
/// value because source reads are already keyset-paged and bounded.
pub(super) const MAX_SOURCE_CHANGE_SEQ: i64 = 50_000;

pub(crate) fn skip_oversized_rebuilds(db: &mut impl ViewDb) -> Result<()> {
    let rows = db.query("SELECT h.session_id,h.user_id FROM thread_view_heads h JOIN thread_logs l ON l.session_id=h.session_id AND l.user_id=h.user_id WHERE h.projection_state='rebuilding' AND l.latest_change_seq>? ORDER BY h.session_id LIMIT 64", &[json!(MAX_SOURCE_CHANGE_SEQ)])?;
    for row in rows {
        db.execute("UPDATE thread_view_heads SET projection_state='source',rebuild_after=-1 WHERE session_id=? AND user_id=?", &[row[0].clone(), row[1].clone()])?;
    }
    Ok(())
}
/// Discover one bounded keyset page per maintenance iteration. Presentation
/// reads stay read-only and database initialization never scans history.
pub(crate) fn discover_rebuilds(db: &mut impl ViewDb) -> Result<()> {
    const KEY: &str = "thread_view_discovery_v2";
    let marker = one(db, "SELECT value FROM meta WHERE key=?", &[json!(KEY)])?;
    let state: Value = marker
        .as_ref()
        .and_then(|r| serde_json::from_str(text(&r[0])).ok())
        .unwrap_or(Value::Null);
    if state.get("done").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    let after = state.get("after").and_then(Value::as_str).unwrap_or("");
    let rows = db.query("SELECT session_id,user_id,latest_change_seq FROM thread_logs WHERE session_id>? ORDER BY session_id LIMIT 64", &[json!(after)])?;
    for row in &rows {
        if int(&row[2]) > 0 {
            initialize(db, text(&row[1]), text(&row[0]))?;
        }
    }
    let next = rows.last().map(|r| text(&r[0])).unwrap_or(after);
    let marker = json!({"after": next, "done": rows.len() < 64}).to_string();
    db.execute("INSERT INTO meta(key,value,updated_time) VALUES(?,?,0) ON CONFLICT(key) DO UPDATE SET value=excluded.value", &[json!(KEY),json!(marker)])?;
    Ok(())
}

fn hydrate_chat_reasoning(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    row: &mut ChatViewRow,
) -> Result<()> {
    if row.assistant.item_id.is_empty() || !row.assistant.reasoning.is_empty() {
        return Ok(());
    }
    let reasoning = format!(
        "substr(COALESCE(NULLIF({},''),{}),1,2048)",
        db.json_text("payload", "reasoning"),
        db.json_text("payload", "reasoning_content")
    );
    if let Some(value) = one(
        db,
        &format!(
            "SELECT {reasoning} FROM thread_items WHERE user_id=? AND session_id=? AND item_id=? AND visibility='user' LIMIT 1"
        ),
        &[json!(user), json!(session), json!(row.assistant.item_id)],
    )? {
        let reasoning = text(&value[0]);
        if !reasoning.is_empty() {
            row.assistant.reasoning = reasoning.to_owned();
            row.assistant.reasoning_offset = reasoning.encode_utf16().count();
        } else {
            let block_reasoning = format!(
                "COALESCE({}, {})",
                db.json_text(&db.json_text("payload", "data"), "reasoning"),
                db.json_text("payload", "reasoning")
            );
            if let Some(block) = one(
                db,
                &format!("SELECT substr({block_reasoning},1,2048) FROM thread_item_blocks WHERE user_id=? AND session_id=? AND item_id=? AND field='reasoning' ORDER BY block_index LIMIT 1"),
                &[json!(user), json!(session), json!(row.assistant.item_id.clone())],
            )? {
                let text = text(&block[0]);
                if !text.is_empty() {
                    row.assistant.reasoning = text.to_owned();
                    row.assistant.reasoning_offset = text.encode_utf16().count();
                }
            }
        }
    }
    Ok(())
}

/// Persist a round-robin keyset cursor so one very large historical thread
/// cannot keep the first pending page occupied forever. Idle polls only read.
pub(crate) fn pending_rebuilds(db: &mut impl ViewDb) -> Result<Vec<(String, String, i64)>> {
    skip_oversized_rebuilds(db)?;
    const KEY: &str = "thread_view_pending_cursor_v1";
    let marker = one(db, "SELECT value FROM meta WHERE key=?", &[json!(KEY)])?;
    let after = marker.as_ref().map(|row| text(&row[0])).unwrap_or("");
    let mut rows = db.query("SELECT user_id,session_id,rebuild_after FROM thread_view_heads WHERE projection_state='rebuilding' AND session_id>? ORDER BY session_id LIMIT 8", &[json!(after)])?;
    if rows.is_empty() && !after.is_empty() {
        rows = db.query("SELECT user_id,session_id,rebuild_after FROM thread_view_heads WHERE projection_state='rebuilding' ORDER BY session_id LIMIT 8", &[])?;
    }
    if let Some(last) = rows.last() {
        db.execute("INSERT INTO meta(key,value,updated_time) VALUES(?,?,0) ON CONFLICT(key) DO UPDATE SET value=excluded.value", &[json!(KEY),last[1].clone()])?;
    }
    Ok(rows
        .into_iter()
        .map(|row| (text(&row[0]).into(), text(&row[1]).into(), int(&row[2])))
        .collect())
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
    if let Some(m) = one(
        db,
        "SELECT revision,fingerprint FROM thread_body_manifests WHERE session_id=? AND user_id=? AND body_id=?",
        &[json!(session), json!(user), json!(id)],
    )? {
        if text(&m[1]) == fingerprint {
            return Ok(int(&m[0]));
        }
    }
    for (index, chunk) in chunks(content).enumerate() {
        db.execute("INSERT INTO thread_body_chunks(session_id,user_id,body_id,revision,chunk_index,content) VALUES(?,?,?,?,?,?) ON CONFLICT(session_id,body_id,revision,chunk_index) DO NOTHING", &[json!(session),json!(user),json!(id),json!(rev),json!(index as i64),json!(chunk)])?;
        db.index_chunk(user, session, id, rev, index as i64, chunk)?;
    }
    db.execute("INSERT INTO thread_body_manifests(session_id,user_id,body_id,revision,fingerprint) VALUES(?,?,?,?,?) ON CONFLICT(session_id,body_id) DO UPDATE SET revision=excluded.revision,fingerprint=excluded.fingerprint",&[json!(session),json!(user),json!(id),json!(rev),json!(fingerprint)])?;
    Ok(rev)
}

/// The newest user input is not the executor. Select actual execution first,
/// admission second, parked input last; only capacity waiting is a queued thread.
pub(super) fn execution_head(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
) -> Result<(String, String)> {
    let reason = db.json_text(&db.json_text("payload", "queued_input"), "reason");
    let active = one(db, &format!("SELECT status,turn_id,{reason} FROM thread_turns WHERE user_id=? AND session_id=? AND status IN ('accepted','queued','running','waiting_input','waiting_approval') ORDER BY CASE status WHEN 'running' THEN 3 WHEN 'waiting_input' THEN 3 WHEN 'waiting_approval' THEN 3 WHEN 'accepted' THEN 2 WHEN 'queued' THEN 1 ELSE 0 END DESC,user_turn_index DESC,created_time DESC LIMIT 1"), &[json!(user),json!(session)])?;
    if let Some(row) = active {
        let status = match text(&row[0]) {
            "accepted" => "running",
            "queued" if matches!(text(&row[2]), "input" | "scheduler") => "running",
            value => value,
        };
        return Ok((status.into(), text(&row[1]).into()));
    }
    let latest = one(db, "SELECT status FROM thread_turns WHERE user_id=? AND session_id=? ORDER BY user_turn_index DESC,created_time DESC LIMIT 1", &[json!(user),json!(session)])?;
    Ok((
        latest
            .as_ref()
            .map(|row| text(&row[0]))
            .unwrap_or("idle")
            .into(),
        String::new(),
    ))
}

pub(crate) fn project_turn(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    turn: &str,
) -> Result<()> {
    let Some(t) = one(
        db,
        "SELECT root_turn_id,user_turn_index,client_message_id,status,created_time,updated_time,trigger_kind FROM thread_turns WHERE session_id=? AND user_id=? AND turn_id=?",
        &[json!(session), json!(user), json!(turn)],
    )?
    else {
        return Ok(());
    };
    let root = text(&t[0]);
    let root_record=one(db,"SELECT user_turn_index,client_message_id,created_time FROM thread_turns WHERE session_id=? AND user_id=? AND turn_id=?",&[json!(session),json!(user),json!(root)])?.unwrap_or_else(||vec![t[1].clone(),t[2].clone(),t[4].clone()]);
    let queue_field = db.json_text("payload", "queued_input");
    let latest=one(db,&format!("SELECT status,updated_time,turn_id,{queue_field} FROM thread_turns WHERE session_id=? AND user_id=? AND root_turn_id=? ORDER BY created_time DESC,turn_id DESC LIMIT 1"),&[json!(session),json!(user),json!(root)])?.unwrap_or_else(||vec![t[3].clone(),t[5].clone(),json!(turn),Value::Null]);
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
    row.queued_input = if row.status == "queued" {
        serde_json::from_str(text(&latest[3])).ok()
    } else {
        None
    };
    row.updated_time = float(&latest[1]);
    let seq = head(db, user, session)?;
    if projection::is_running(&row.status) == 0 {
        if let Some(body) = row
            .assistant
            .body
            .as_mut()
            .filter(|b| b.body_id.ends_with("|stream") && b.revision == 0)
        {
            // Freeze the committed partial output without concatenating its
            // chunks. Copy/read now binds to an immutable body version.
            db.execute("INSERT INTO thread_body_chunks(session_id,user_id,body_id,revision,chunk_index,content) SELECT session_id,user_id,body_id,?,chunk_index,content FROM thread_body_chunks WHERE session_id=? AND user_id=? AND body_id=? AND revision=0 ON CONFLICT DO NOTHING",&[json!(seq),json!(session),json!(user),json!(body.body_id)])?;
            body.revision = seq;
        }
    }
    save_chat(db, user, session, &mut row, seq)?;
    db.execute("UPDATE thread_view_heads SET latest_round=CASE WHEN latest_round<? THEN ? ELSE latest_round END WHERE session_id=?", &[json!(row.user_round),json!(row.user_round),json!(session)])?;
    let (status, id) = execution_head(db, user, session)?;
    db.execute("UPDATE thread_view_heads SET runtime_status=?,active_turn_id=? WHERE session_id=? AND user_id=?",&[json!(status),json!(id),json!(session),json!(user)])?;
    Ok(())
}

pub(crate) fn project_item(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    id: &str,
) -> Result<()> {
    let Some(i) = one(
        db,
        "SELECT i.turn_id,i.root_turn_id,i.item_index,i.created_seq,i.revision,i.kind,i.status,i.visibility,i.payload,i.created_time,i.updated_time,t.user_turn_index FROM thread_items i JOIN thread_turns t ON t.session_id=i.session_id AND t.turn_id=i.turn_id WHERE i.session_id=? AND i.user_id=? AND i.item_id=?",
        &[json!(session), json!(user), json!(id)],
    )?
    else {
        return Ok(());
    };
    let parsed_item: Value = serde_json::from_str(text(&i[8]))?;
    // System prompts stay as redacted context. Agent inbox deliveries are the
    // other intentional internal exception: they explain an actual handoff,
    // while every other hidden model message remains out of the user ledger.
    if !trajectory_visibility(text(&i[7]), text(&i[5]), &parsed_item) {
        if let Some(old) = one(
            db,
            "SELECT status,created_seq,payload FROM thread_trajectory_rows WHERE session_id=? AND user_id=? AND item_id=?",
            &[json!(session), json!(user), json!(id)],
        )? {
            let old_kind = serde_json::from_str::<Value>(text(&old[2]))
                .ok()
                .and_then(|value| value.get("kind").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_default();
            let (old_input, old_model, old_tool) = tile_lane(&old_kind);
            for level in 0..8 {
                db.execute("UPDATE thread_trajectory_tiles SET count=count-1,failed=failed-?,running=running-?,input_count=input_count-?,model_count=model_count-?,tool_count=tool_count-? WHERE session_id=? AND level=? AND bucket=?",&[json!(projection::is_failed(text(&old[0]))),json!(projection::is_running(text(&old[0]))),json!(old_input),json!(old_model),json!(old_tool),json!(session),json!(level),json!(int(&old[1])/(128i64 << (level*3)))])?;
            }
            db.execute(
                "DELETE FROM thread_trajectory_rows WHERE session_id=? AND user_id=? AND item_id=?",
                &[json!(session), json!(user), json!(id)],
            )?;
            for field in ["content", "detail", "stream"] {
                for table in [
                    "thread_body_search",
                    "thread_body_chunks",
                    "thread_body_manifests",
                ] {
                    db.execute(
                        &format!(
                            "DELETE FROM {table} WHERE session_id=? AND user_id=? AND body_id=?"
                        ),
                        &[
                            json!(session),
                            json!(user),
                            json!(projection::body_id(id, field)),
                        ],
                    )?;
                }
            }
            if let Some(r) = one(
                db,
                "SELECT payload FROM thread_chat_rows WHERE session_id=? AND root_turn_id=?",
                &[json!(session), i[1].clone()],
            )? {
                let mut row: ChatViewRow = serde_json::from_str(text(&r[0]))?;
                if row.user.item_id == id {
                    row.user = ViewMessage::default();
                }
                if row.assistant.item_id == id {
                    row.assistant = ViewMessage::default();
                }
                if matches!(
                    text(&i[5]),
                    "tool_call" | "approval" | "plan" | "compaction" | "queue" | "subagent_run"
                ) || projection::is_compaction_system_message(text(&i[5]), &parsed_item) {
                    row.workflow_count = (row.workflow_count - 1).max(0);
                    row.failed_count =
                        (row.failed_count - projection::is_failed(text(&old[0]))).max(0);
                }
                if text(&i[5]) == "subagent_run" {
                    let removed = id.to_string();
                    row.subagents.retain(|card| {
                        card.get("key").and_then(Value::as_str) != Some(removed.as_str())
                    });
                }
                let seq = head(db, user, session)?;
                save_chat(db, user, session, &mut row, seq)?;
            }
        }
        return Ok(());
    }
    let turn = text(&i[0]);
    let root = text(&i[1]);
    let revision = int(&i[4]);
    let kind = text(&i[5]);
    let status = text(&i[6]);
    let mut item = parsed_item;
    let old = one(
        db,
        "SELECT status,created_seq,revision,payload FROM thread_trajectory_rows WHERE session_id=? AND item_id=?",
        &[json!(session), json!(id)],
    )?;
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
    if kind == "system_message" {
        projection::redact_internal_context(&mut detail_item);
    }
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
    let (input_count, model_count, tool_count) = tile_lane(kind);
    let old_lane = old
        .as_ref()
        .and_then(|row| serde_json::from_str::<Value>(text(&row[3])).ok())
        .and_then(|value| value.get("kind").and_then(Value::as_str).map(tile_lane))
        .unwrap_or((0, 0, 0));
    for level in 0..8 {
        let bucket = int(&i[3]) / (128i64 << (level * 3));
        // `start` is an aggregate lower bound.  The old upsert only advanced
        // `finish`; a legacy zero value therefore survived every later row
        // and made timed overviews span from the Unix epoch.  Keep both bounds
        // monotonic and repair that old sentinel during normal projection.
        db.execute("INSERT INTO thread_trajectory_tiles(session_id,level,bucket,count,failed,running,start,finish,input_count,model_count,tool_count) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(session_id,level,bucket) DO UPDATE SET count=thread_trajectory_tiles.count+excluded.count,failed=thread_trajectory_tiles.failed+excluded.failed,running=thread_trajectory_tiles.running+excluded.running,input_count=thread_trajectory_tiles.input_count+excluded.input_count,model_count=thread_trajectory_tiles.model_count+excluded.model_count,tool_count=thread_trajectory_tiles.tool_count+excluded.tool_count,start=CASE WHEN thread_trajectory_tiles.start<=0 OR excluded.start<thread_trajectory_tiles.start THEN excluded.start ELSE thread_trajectory_tiles.start END,finish=CASE WHEN thread_trajectory_tiles.finish<excluded.finish THEN excluded.finish ELSE thread_trajectory_tiles.finish END", &[json!(session),json!(level),json!(bucket),json!(i64::from(old.is_none())),json!(projection::is_failed(status)-old_status.map(projection::is_failed).unwrap_or(0)),json!(projection::is_running(status)-old_status.map(projection::is_running).unwrap_or(0)),json!(float(&i[9])),json!(float(&i[10])),json!(input_count-old_lane.0),json!(model_count-old_lane.1),json!(tool_count-old_lane.2)])?;
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
        && !projection::has_tool_calls(&item)
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
        projection::set_reasoning(&mut row.assistant, &item);
        row.assistant.created_seq = int(&i[3]);
        row.assistant.durable_seq = one(
            db,
            "SELECT latest_change_seq FROM thread_logs WHERE session_id=? AND user_id=?",
            &[json!(session), json!(user)],
        )?
        .map(|r| int(&r[0]))
        .unwrap_or(0);
        if !row
            .assistant
            .body
            .as_ref()
            .is_some_and(|b| b.body_id.ends_with("|stream"))
        {
            row.assistant.content_offset = content.encode_utf16().count();
        }
        row.assistant.feedback = item.get("feedback").cloned();
        if !row.assistant.reasoning.is_empty() {
            if row.reasoning_item_id != id {
                row.reasoning_count += 1;
                row.reasoning_item_id = id.to_owned();
            }
        }
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
        let calls_tools = projection::has_tool_calls(&item);
        // Tool-call assistant items update process metadata only.  They must
        // never claim or clear the terminal answer slot; a prior committed
        // answer remains stable while the next model round is executing.
        if calls_tools && row.assistant.item_id == id {
            // The same stable item may first arrive as a streaming text row
            // and only later receive its tool_calls decoration.  Revoke that
            // provisional answer identity at the transition, while keeping
            // its durable text in the trajectory/process view.
            row.assistant = ViewMessage::default();
        }
        projection::set_reasoning(&mut row.assistant, &item);
        if let Some(stats) = projection::finite_stats(&item) {
            row.assistant.stats = Some(stats);
        }
        if !row.assistant.reasoning.is_empty() && row.reasoning_item_id != id {
            row.reasoning_count += 1;
            row.reasoning_item_id = id.to_owned();
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
    if kind == "tool_call" {
        if item
            .pointer("/meta/workspace_changed")
            .and_then(Value::as_bool)
            == Some(true)
        {
            // Completion order can differ from tool admission order. Use the
            // commit revision so a slower, earlier parallel write is not lost.
            let committed = one(
                db,
                "SELECT latest_change_seq FROM thread_logs WHERE session_id=? AND user_id=?",
                &[json!(session), json!(user)],
            )?
            .map(|r| int(&r[0]))
            .unwrap_or(0);
            row.workspace_change_seq = row.workspace_change_seq.max(committed);
        }
        projection::set_activity(
            &mut row,
            if projection::is_running(status) != 0 {
                "tool"
            } else {
                "processing"
            },
            id,
            int(&i[3]),
            item.get("tool").and_then(Value::as_str).unwrap_or(""),
        );
    } else if kind == "assistant_message" {
        let calls_tools = projection::has_tool_calls(&item);
        let assistant_empty = row.assistant.content.is_empty();
        projection::set_activity(
            &mut row,
            if calls_tools {
                "processing"
            } else if content.is_empty() && assistant_empty {
                "thinking"
            } else {
                "responding"
            },
            id,
            int(&i[3]),
            "",
        );
    }
    if matches!(
        kind,
        "tool_call" | "approval" | "plan" | "compaction" | "subagent_run"
    ) || projection::is_compaction_system_message(kind, &item)
    {
        row.workflow_count += i64::from(old.is_none());
        row.failed_count +=
            projection::is_failed(status) - old_status.map(projection::is_failed).unwrap_or(0);
    }
    if kind == "subagent_run" {
        // 子智能体进度条目按条目 id 原地推进：每次运行时更新都重写卡片并
        // 抬升行 revision，让 changes 流把新状态推给前端面板。
        if let Some(card) = projection::subagent_card(&item, id, revision) {
            projection::upsert_subagent_card(&mut row.subagents, card);
        }
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
    let Some(r) = one(
        db,
        "SELECT payload FROM thread_chat_rows WHERE session_id=? AND user_id=? AND root_turn_id=(SELECT root_turn_id FROM thread_items WHERE session_id=? AND user_id=? AND item_id=?)",
        &[
            json!(session),
            json!(user),
            json!(session),
            json!(user),
            json!(id),
        ],
    )?
    else {
        return Ok(());
    };
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
    if !matches!(field, "content" | "reasoning") {
        return Ok(());
    }
    let Some(i) = one(
        db,
        "SELECT turn_id,root_turn_id,kind,visibility,created_seq,payload FROM thread_items WHERE session_id=? AND user_id=? AND item_id=?",
        &[json!(session), json!(user), json!(id)],
    )?
    else {
        return Ok(());
    };
    if text(&i[3]) != "user" || !projection::stable_answer(id, text(&i[0]), text(&i[2])) {
        return Ok(());
    }
    // Content blocks for a tool-call assistant item are already part of the
    // durable process timeline.  They must never create a provisional final
    // answer body in the chat row.
    let item = serde_json::from_str::<Value>(text(&i[5]))?;
    if projection::has_tool_calls(&item) {
        return Ok(());
    }
    let data = block.get("data").unwrap_or(block);
    let content = data
        .get(if field == "reasoning" {
            "reasoning"
        } else {
            "content"
        })
        .and_then(Value::as_str)
        .unwrap_or("");
    if content.is_empty() {
        return Ok(());
    }
    let body_id = projection::body_id(
        id,
        if field == "reasoning" {
            "reasoning"
        } else {
            "stream"
        },
    );
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
    if (field == "content" && int(&i[4]) < row.assistant.created_seq)
        || (field == "content"
            && row.assistant.item_id == id
            && row.assistant.body.as_ref().is_some_and(|b| {
                b.bytes > 0 && (!b.body_id.ends_with("|stream") || b.revision > 0)
            }))
    {
        return Ok(());
    }
    // Body block identities are stable across duplicate event delivery.
    let old_bytes=db.query("SELECT content FROM thread_body_chunks WHERE session_id=? AND body_id=? AND revision=0 AND chunk_index>=? AND chunk_index<?",&[json!(session),json!(body_id),json!(index*1024),json!((index+1)*1024)])?.iter().map(|r|text(&r[0]).len()).sum::<usize>();
    db.execute("DELETE FROM thread_body_chunks WHERE session_id=? AND body_id=? AND revision=0 AND chunk_index>=? AND chunk_index<?",&[json!(session),json!(body_id),json!(index*1024),json!((index+1)*1024)])?;
    for (n, c) in chunks(content).enumerate() {
        db.execute("INSERT INTO thread_body_chunks(session_id,user_id,body_id,revision,chunk_index,content) VALUES(?,?,?,0,?,?) ON CONFLICT(session_id,body_id,revision,chunk_index) DO UPDATE SET content=excluded.content",&[json!(session),json!(user),json!(body_id),json!(index*1024+n as i64),json!(c)])?;
    }
    if row.assistant.item_id != id {
        if field == "reasoning"
            && !row.assistant.item_id.is_empty()
            && row.assistant.created_seq >= int(&i[4])
        {
            return Ok(());
        }
        row.assistant = message_for_stream(id, &projection::body_id(id, "stream"));
    }
    let offset = data
        .get(if field == "reasoning" {
            "reasoning_offset"
        } else {
            "content_offset"
        })
        .and_then(Value::as_i64)
        .unwrap_or(0);
    // Durable offsets are UTF-16; the row keeps only a bounded UTF-8 preview.
    if field == "reasoning" {
        let start = offset.max(0) as usize;
        let end = start.saturating_add(content.encode_utf16().count());
        if start <= row.assistant.reasoning_offset && end > row.assistant.reasoning_offset {
            // Blocks can be rewritten with an increasingly complete payload.
            // Append only the unseen UTF-16 suffix, including when the block
            // begins before the current cursor, rather than freezing at its
            // first durable preview or duplicating its existing prefix.
            let overlap = row.assistant.reasoning_offset - start;
            let mut units = 0;
            let unseen = content
                .char_indices()
                .find_map(|(index, ch)| {
                    let at = units;
                    units += ch.len_utf16();
                    (at >= overlap).then_some(index)
                })
                .unwrap_or(content.len());
            row.assistant.reasoning.push_str(&content[unseen..]);
            row.assistant.reasoning = projection::preview_tail(&row.assistant.reasoning, 2048);
        } else if start > row.assistant.reasoning_offset {
            // A missed block is healed by this durable tail, without joining
            // text across a gap or retaining the first capped preview forever.
            row.assistant.reasoning = projection::preview_tail(content, 2048);
        }
        row.assistant.reasoning_offset = row
            .assistant
            .reasoning_offset
            .max(offset.max(0) as usize + content.encode_utf16().count());
    } else if offset as usize == row.assistant.content.encode_utf16().count()
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
    row.assistant.body = if field == "content" {
        Some(BodyRef {
            body_id,
            revision: 0,
            bytes,
        })
    } else {
        row.assistant.body.clone()
    };
    row.assistant.created_seq = int(&i[4]);
    if field == "content" {
        row.assistant.content_offset = row
            .assistant
            .content_offset
            .max(offset.max(0) as usize + content.encode_utf16().count());
    }
    row.assistant.durable_seq = one(
        db,
        "SELECT latest_change_seq FROM thread_logs WHERE session_id=? AND user_id=?",
        &[json!(session), json!(user)],
    )?
    .map(|r| int(&r[0]))
    .unwrap_or(0);
    if field == "content" {
        row.assistant.tail = preview(content, 2048);
        row.assistant.tail_offset = offset.max(0) as usize + content.encode_utf16().count();
    }
    if field == "reasoning" && row.reasoning_item_id != id {
        row.reasoning_count += 1;
        row.reasoning_item_id = id.to_owned();
    }
    projection::set_activity(
        &mut row,
        if field == "reasoning" {
            "thinking"
        } else {
            "responding"
        },
        id,
        int(&i[4]),
        "",
    );
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
    after: Option<i64>,
    root_turn_id: Option<&str>,
    limit: usize,
) -> Result<ThreadViewPage> {
    anyhow::ensure!(
        before.is_none() || after.is_none(),
        "ambiguous page direction"
    );
    let mut page = ThreadViewPage {
        session_id: session.into(),
        epoch: 1,
        projection_state: "ready".into(),
        thread_empty: Some(true),
        ..Default::default()
    };
    let h = one(
        db,
        "SELECT epoch,view_seq,latest_round,latest_item_seq,projection_state,runtime_status,active_turn_id FROM thread_view_heads WHERE session_id=? AND user_id=?",
        &[json!(session), json!(user)],
    )?;
    if let Some(h) = h {
        page.runtime_status = text(&h[5]).into();
        page.active_turn_id = text(&h[6]).into();
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
    if page.projection_state != "ready" {
        return super::thread_view_source::page(
            db,
            user,
            session,
            kind,
            before,
            after,
            root_turn_id,
            limit,
            page,
        );
    }
    let cap = if kind == "trajectory" {
        limit.clamp(1, TRAJECTORY_PAGE_MAX)
    } else {
        limit.clamp(1, CHAT_PAGE_MAX)
    };
    let cursor = after.or(before).unwrap_or(i64::MAX);
    let mut bytes = 4096usize;
    let sql = if kind == "trajectory" {
        if root_turn_id.is_some() {
            "SELECT payload,created_seq FROM thread_trajectory_rows WHERE user_id=? AND session_id=? AND root_turn_id=? AND created_seq<? ORDER BY created_seq DESC LIMIT ?"
        } else {
            "SELECT payload,created_seq FROM thread_trajectory_rows WHERE user_id=? AND session_id=? AND created_seq<? ORDER BY created_seq DESC LIMIT ?"
        }
    } else {
        "SELECT payload,user_round FROM thread_chat_rows WHERE user_id=? AND session_id=? AND user_round<? ORDER BY user_round DESC LIMIT ?"
    };
    let mut query_params = vec![json!(user), json!(session)];
    if kind == "trajectory" && root_turn_id.is_some() {
        query_params.push(json!(root_turn_id.unwrap()));
    }
    query_params.push(json!(cursor));
    query_params.push(json!((cap + 1) as i64));
    let sql = if after.is_some() {
        sql.replace("<?", ">?").replace(" DESC", " ASC")
    } else {
        sql.to_owned()
    };
    let rows = db.query(&sql, &query_params)?;
    page.has_more = rows.len() > cap;
    for r in rows.into_iter().take(cap) {
        if kind == "trajectory" {
            let mut trajectory: TrajectoryViewRow = serde_json::from_str(text(&r[0]))?;
            // Old terminal rows predate diagnostics in the small page schema.
            // Repair just these indexed rows, never scan/rebuild full threads.
            if trajectory.kind == "terminal" && trajectory.diagnostics.is_none() {
                if let Some(item) = one(
                    db,
                    "SELECT payload FROM thread_items WHERE user_id=? AND session_id=? AND item_id=? LIMIT 1",
                    &[json!(user), json!(session), json!(trajectory.item_id)],
                )? {
                    if let Ok(payload) = serde_json::from_str::<Value>(text(&item[0])) {
                        trajectory.diagnostics = crate::services::thread_log::diagnostics::project(&payload, &trajectory.status);
                    }
                }
            }
            // Rows written before the bounded `subagent` projection field was
            // introduced deserialize with `None`.  Backfill only those rare
            // child rows from the indexed item payload so old sessions become
            // useful immediately without rebuilding the entire ledger.
            if trajectory.kind == "subagent_run" && trajectory.subagent.is_none() {
                if let Some(item) = one(
                    db,
                    "SELECT payload FROM thread_items WHERE user_id=? AND session_id=? AND item_id=? LIMIT 1",
                    &[json!(user), json!(session), json!(trajectory.item_id)],
                )? {
                    if let Ok(payload) = serde_json::from_str::<Value>(text(&item[0])) {
                        trajectory.subagent = projection::subagent_runtime_projection(&payload);
                        if let Some(label) = projection::subagent_label(&payload) {
                            trajectory.summary = preview(label, 256);
                            trajectory.tool_name = Some(preview(label, 96));
                        }
                    }
                }
            }
            // Legacy backfills add bounded fields; count the repaired row,
            // otherwise diagnostics can bypass the response byte budget.
            let size = serde_json::to_vec(&trajectory)?.len() + 1;
            if bytes + size > TRAJECTORY_RESPONSE_BYTES {
                page.has_more = true;
                break;
            }
            bytes += size;
            page.trajectory.push(trajectory);
        } else {
            let size = text(&r[0]).len() + 1;
            if bytes + size > CHAT_RESPONSE_BYTES {
                page.has_more = true;
                break;
            }
            bytes += size;
            page.chat.push(serde_json::from_str(text(&r[0]))?);
        }
        page.next_before = Some(int(&r[1]));
    }
    if after.is_none() {
        page.chat.reverse();
    }
    if kind == "chat" {
        for row in &mut page.chat {
            hydrate_chat_reasoning(db, user, session, row)?;
        }
    }
    if after.is_none() {
        page.trajectory.reverse();
    }
    if kind == "trajectory" && page.projection_state == "ready" && page.view_seq > 0 {
        for r in db.query("SELECT bucket,count,failed,running,start,finish,input_count,model_count,tool_count FROM thread_trajectory_tiles WHERE session_id=? AND level=? ORDER BY bucket LIMIT 1024", &[json!(session),json!(page.tile_level as i64)])? {
            let tile=TrajectoryTile {bucket:int(&r[0]),count:int(&r[1]),failed:int(&r[2]),running:int(&r[3]),start:float(&r[4]),end:float(&r[5]),input_count:int(&r[6]),model_count:int(&r[7]),tool_count:int(&r[8])};
            let size=serde_json::to_vec(&tile)?.len()+1;if bytes+size>TRAJECTORY_RESPONSE_BYTES {break;}bytes+=size;page.tiles.push(tile);
        }
    }
    finish_page_cursors(&mut page, kind, before, after);
    Ok(page)
}

// Responses are always chronological, regardless of seek direction. Only the
// requested edge needs a LIMIT+1 probe; the supplied cursor proves the other edge.
pub(super) fn finish_page_cursors(
    page: &mut ThreadViewPage,
    kind: &str,
    before: Option<i64>,
    after: Option<i64>,
) {
    if after.is_some() {
        page.has_newer = page.has_more;
        page.has_more = true;
    } else {
        page.has_newer = before.is_some();
    }
    if kind == "trajectory" {
        page.next_before = page.trajectory.first().map(|row| row.created_seq);
        page.next_after = page.trajectory.last().map(|row| row.created_seq);
    } else {
        page.next_before = page.chat.first().map(|row| row.user_round);
        page.next_after = page.chat.last().map(|row| row.user_round);
    }
}

pub(crate) fn changes(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    after: i64,
) -> Result<ThreadViewChanges> {
    if after < 0 {
        return super::thread_view_source::changes(db, user, session, after);
    }
    let h = one(
        db,
        "SELECT view_seq,epoch,projection_state,runtime_status,active_turn_id FROM thread_view_heads WHERE session_id=? AND user_id=?",
        &[json!(session), json!(user)],
    )?;
    let runtime_status = h
        .as_ref()
        .map(|r| text(&r[3]).to_string())
        .unwrap_or_else(|| "idle".into());
    let active_turn_id = h
        .as_ref()
        .map(|r| text(&r[4]).to_string())
        .unwrap_or_default();
    let seq = h.as_ref().map(|r| int(&r[0])).unwrap_or(0);
    let epoch = h.as_ref().map(|r| int(&r[1])).unwrap_or(1);
    let ready = h.as_ref().is_none_or(|r| text(&r[2]) == "ready");
    let rows=db.query("SELECT payload FROM thread_chat_rows WHERE user_id=? AND session_id=? AND revision>? ORDER BY revision LIMIT 21", &[json!(user),json!(session),json!(after)])?;
    let reset = !ready
        || after > seq
        || rows.len() > 20
        || rows.iter().map(|r| text(&r[0]).len()).sum::<usize>() > 60 * 1024;
    Ok(ThreadViewChanges {
        runtime_status,
        active_turn_id,
        epoch,
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
    if id
        .rsplit_once('|')
        .is_some_and(|(_, field)| field.starts_with("source_"))
    {
        return super::thread_view_source::body(db, user, session, id, rev, cursor);
    }
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
    let started = std::time::Instant::now();
    let budget = std::time::Duration::from_millis(40);
    if after < 0 {
        let position=one(db,"SELECT rebuild_block_item,rebuild_block_field,rebuild_block_index FROM thread_view_heads WHERE session_id=? AND user_id=?",&[json!(session),json!(user)])?.unwrap_or_else(||vec![json!(""),json!(""),json!(-1)]);
        let blocks=db.query("SELECT item_id,field,block_index FROM thread_item_blocks WHERE user_id=? AND session_id=? AND (item_id,field,block_index)>(?,?,?) ORDER BY item_id,field,block_index LIMIT 65",&[json!(user),json!(session),position[0].clone(),position[1].clone(),position[2].clone()])?;
        let cursor = after.saturating_neg() - 1;
        let mut more = blocks.len() > 64;
        let mut last = cursor;
        let mut position = position;
        let mut bytes = 0;
        for r in blocks.into_iter().take(64) {
            if last > cursor && (started.elapsed() >= budget || bytes >= 2 * 1024 * 1024) {
                more = true;
                break;
            }
            let Some(payload) = one(
                db,
                "SELECT payload FROM thread_item_blocks WHERE session_id=? AND user_id=? AND item_id=? AND field=? AND block_index=?",
                &[
                    json!(session),
                    json!(user),
                    r[0].clone(),
                    r[1].clone(),
                    r[2].clone(),
                ],
            )?
            else {
                continue;
            };
            bytes += text(&payload[0]).len();
            let block: Value = serde_json::from_str(text(&payload[0]))?;
            let committed = one(
                db,
                "SELECT payload FROM thread_chat_rows WHERE session_id=? AND user_id=? AND root_turn_id=(SELECT root_turn_id FROM thread_items WHERE session_id=? AND item_id=?)",
                &[json!(session), json!(user), json!(session), r[0].clone()],
            )?;
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
        if cursor > after && started.elapsed() >= budget {
            more = true;
            break;
        }
        let root = one(
            db,
            "SELECT root_turn_id FROM thread_turns WHERE session_id=? AND user_id=? AND turn_id=?",
            &[json!(session), json!(user), r[1].clone()],
        )?;
        if let Some(root) = root {
            if one(
                db,
                "SELECT root_turn_id FROM thread_chat_rows WHERE session_id=? AND root_turn_id=?",
                &[json!(session), root[0].clone()],
            )?
            .is_none()
            {
                project_turn(db, user, session, text(&r[1]))?;
            }
        }
        project_item(db, user, session, text(&r[0]))?;
        cursor = int(&r[2]);
    }
    // A running legacy answer may exist only in durable blocks. Item completion
    // must transition into block replay before the projection becomes ready.
    db.execute("UPDATE thread_view_heads SET rebuild_after=?,projection_state='rebuilding' WHERE session_id=? AND user_id=?", &[json!(if more {cursor}else{-1}),json!(session),json!(user)])?;
    Ok((if more { cursor } else { -1 }, true))
}
