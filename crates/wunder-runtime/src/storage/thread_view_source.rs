//! Read bounded windows before the optional historical projection is complete.
//! These reads never enqueue migration or select an entire execution payload.
use super::thread_view_projection as projection;
use super::thread_view_store::ViewDb;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use wunder_core::thread_view::*;

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}
fn int(value: &Value) -> i64 {
    value.as_i64().unwrap_or(0)
}
fn float(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}
fn one(db: &mut impl ViewDb, sql: &str, values: &[Value]) -> Result<Option<Vec<Value>>> {
    Ok(db.query(sql, values)?.into_iter().next())
}
fn content(db: &impl ViewDb) -> String {
    format!("COALESCE({},'')", db.json_text("payload", "content"))
}

fn block_content(db: &impl ViewDb) -> String {
    format!(
        "COALESCE({}, {}, '')",
        db.json_text(&db.json_text("payload", "data"), "content"),
        db.json_text("payload", "content")
    )
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(",")
}

fn source_chat_rows(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    roots: &[Vec<Value>],
    page: &mut ThreadViewPage,
    bytes: &mut usize,
    budget: usize,
) -> Result<()> {
    if roots.is_empty() {
        return Ok(());
    }
    let root_ids = roots
        .iter()
        .map(|row| text(&row[0]).to_owned())
        .collect::<Vec<_>>();
    let mut params = vec![json!(user), json!(session)];
    params.extend(root_ids.iter().cloned().map(Value::String));
    let in_roots = placeholders(root_ids.len());

    let mut latest_turns = std::collections::HashMap::new();
    let latest = db.query(
        &format!("SELECT root_turn_id,turn_id,status,updated_time FROM thread_turns WHERE user_id=? AND session_id=? AND root_turn_id IN ({in_roots}) ORDER BY created_time DESC,turn_id DESC"),
        &params,
    )?;
    for row in latest {
        latest_turns.entry(text(&row[0]).to_owned()).or_insert(row);
    }

    let mut counts = std::collections::HashMap::<String, (i64, i64, i64)>::new();
    let reasoning = format!(
        "COALESCE(NULLIF({},''),{})",
        db.json_text("payload", "reasoning"),
        db.json_text("payload", "reasoning_content")
    );
    // Do not aggregate every item in a large root here.  A legacy stress
    // thread can contain thousands of workflow rows in one user turn; the
    // unbounded GROUP BY made the "load older history" request exceed the
    // 1.5s presentation transaction deadline.  The chat card only needs a
    // bounded indicator, so count a small indexed prefix per root.  The
    // trajectory page remains the source of the complete workflow ledger.
    for root in &root_ids {
        let row = one(
            db,
            &format!("SELECT SUM(CASE WHEN kind IN ('tool_call','approval','plan','compaction','queue','subagent_run') THEN 1 ELSE 0 END),SUM(CASE WHEN kind IN ('tool_call','approval','plan','compaction','queue','subagent_run') AND status IN ('failed','rejected') THEN 1 ELSE 0 END),SUM(CASE WHEN kind='assistant_message' AND {reasoning} IS NOT NULL AND {reasoning}<>'' THEN 1 ELSE 0 END) FROM (SELECT status,kind,payload FROM thread_items WHERE user_id=? AND session_id=? AND root_turn_id=? AND visibility='user' LIMIT 512) bounded"),
            &[json!(user), json!(session), json!(root)],
        )?;
        if let Some(row) = row {
            counts.insert(root.clone(), (int(&row[0]), int(&row[1]), int(&row[2])));
        }
    }

    let content = db.json_text("payload", "content");
    let item_bytes = db.text_bytes(&content);
    let stats = format!(
        "COALESCE({}, {})",
        db.json_text("payload", "message_stats"),
        db.json_text(&db.json_text("payload", "meta"), "message_stats")
    );
    // Resolve the latest user and stable assistant rows with index seeks. A
    // window over every item in a selected root can spill to disk for a giant
    // turn, even when the response only contains a dozen chat rows.
    let mut message_rows = Vec::with_capacity(roots.len().saturating_mul(2));
    for root in roots {
        for kind in ["user_message", "assistant_message"] {
            let answer_filter = if kind == "assistant_message" {
                " AND item_id LIKE turn_id || ':text-%'"
            } else {
                ""
            };
            let sql = format!(
                "SELECT root_turn_id,item_id,revision,created_seq,kind,substr(COALESCE({content},''),1,2048),{item_bytes},substr({reasoning},1,2048),substr({stats},1,4096) FROM thread_items WHERE user_id=? AND session_id=? AND root_turn_id=? AND visibility='user' AND kind=?{answer_filter} ORDER BY item_index DESC,created_seq DESC LIMIT 1"
            );
            if let Some(row) = one(
                db,
                &sql,
                &[json!(user), json!(session), root[0].clone(), json!(kind)],
            )? {
                message_rows.push(row);
            }
        }
    }

    let mut block_fallback = Vec::new();
    for row in &message_rows {
        if text(&row[4]) == "assistant_message" && int(&row[6]) == 0 {
            block_fallback.push(text(&row[1]).to_owned());
        }
    }
    let mut first_blocks = std::collections::HashMap::<String, (String, usize)>::new();
    if !block_fallback.is_empty() {
        let mut block_params = block_fallback
            .iter()
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>();
        block_params.extend([json!(user), json!(session)]);
        let block_value = block_content(db);
        let block_size = db.text_bytes(&block_value);
        for row in db.query(
            &format!("WITH ids(item_id) AS (VALUES {}) SELECT b.item_id,substr({block_value},1,2048),{block_size} FROM ids JOIN thread_item_blocks b ON b.user_id=? AND b.session_id=? AND b.item_id=ids.item_id AND b.field='content' AND b.block_index=(SELECT MIN(x.block_index) FROM thread_item_blocks x WHERE x.user_id=b.user_id AND x.session_id=b.session_id AND x.item_id=b.item_id AND x.field=b.field)", std::iter::repeat_n("(?)", block_fallback.len()).collect::<Vec<_>>().join(",")),
            &block_params,
        )? {
            first_blocks.insert(text(&row[0]).to_owned(), (text(&row[1]).to_owned(), int(&row[2]).max(0) as usize));
        }
    }

    let mut messages =
        std::collections::HashMap::<String, (Option<ViewMessage>, Option<ViewMessage>)>::new();
    for row in message_rows.drain(..) {
        let root = text(&row[0]).to_owned();
        let is_assistant = text(&row[4]) == "assistant_message";
        let mut message = ViewMessage {
            item_id: text(&row[1]).into(),
            content: preview(text(&row[5]), 2048),
            created_seq: int(&row[3]),
            ..Default::default()
        };
        let mut body_bytes = int(&row[6]).max(0) as usize;
        let field = if is_assistant && body_bytes == 0 {
            if let Some((block, block_bytes)) = first_blocks.get(text(&row[1])) {
                message.content = preview(block, 2048);
                message.content_offset = message.content.encode_utf16().count();
                body_bytes = block_bytes.saturating_add(1);
                "source_stream"
            } else {
                "source_content"
            }
        } else {
            "source_content"
        };
        message.body = Some(BodyRef {
            body_id: projection::body_id(text(&row[1]), field),
            revision: int(&row[2]),
            bytes: body_bytes,
        });
        if is_assistant {
            message.reasoning = text(&row[7]).to_owned();
            message.reasoning_offset = message.reasoning.encode_utf16().count();
            if let Ok(stats) = serde_json::from_str::<Value>(text(&row[8])) {
                message.stats = projection::finite_stats(&json!({"message_stats": stats}));
            }
            if message.reasoning.is_empty() {
                let reasoning_value = format!(
                    "COALESCE({}, {})",
                    db.json_text(&db.json_text("b.payload", "data"), "reasoning"),
                    db.json_text("b.payload", "reasoning")
                );
                if let Some(reasoning) = one(
                    db,
                    &format!("SELECT substr({reasoning_value},1,2048) FROM thread_item_blocks b WHERE b.user_id=? AND b.session_id=? AND b.item_id=? AND b.field='reasoning' ORDER BY b.block_index LIMIT 1"),
                    &[json!(user), json!(session), json!(row[1].clone())],
                )? {
                    message.reasoning = text(&reasoning[0]).to_owned();
                    message.reasoning_offset = message.reasoning.encode_utf16().count();
                }
            }
        }
        let entry = messages.entry(root).or_default();
        if is_assistant {
            entry.1 = Some(message);
        } else {
            entry.0 = Some(message);
        }
    }

    for root in roots {
        let id = text(&root[0]);
        let mut row = projection::new_chat(
            id,
            int(&root[1]),
            root[2].as_str().map(str::to_owned),
            text(&root[3]),
            float(&root[4]),
            float(&root[5]),
        );
        if let Some(latest) = latest_turns.get(id) {
            row.execution_turn_id = text(&latest[1]).into();
            row.status = text(&latest[2]).into();
            row.updated_time = float(&latest[3]);
        }
        (row.workflow_count, row.failed_count, row.reasoning_count) = counts.get(id).copied().unwrap_or_default();
        row.revision = page.view_seq;
        if let Some((user, assistant)) = messages.remove(id) {
            row.user = user.unwrap_or_default();
            row.assistant = assistant.unwrap_or_default();
        }
        row.assistant.durable_seq = page.view_seq;
        let size = serde_json::to_vec(&row)?.len() + 1;
        if *bytes + size > budget {
            page.has_more = true;
            break;
        }
        *bytes += size;
        page.next_before = Some(row.user_round);
        page.chat.push(row);
    }
    Ok(())
}

pub(super) fn page(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    kind: &str,
    before: Option<i64>,
    root_turn_id: Option<&str>,
    limit: usize,
    mut page: ThreadViewPage,
) -> Result<ThreadViewPage> {
    let envelope = one(
        db,
        "SELECT latest_user_turn,latest_change_seq FROM thread_logs WHERE user_id=? AND session_id=?",
        &[json!(user), json!(session)],
    )?;
    let Some(envelope) = envelope else {
        return Ok(page);
    };
    // A distinct epoch binds the client's cursor to durable source changes.
    // Finishing the projection causes one atomic refresh into its own epoch.
    page.epoch = -1;
    page.view_seq = int(&envelope[1]);
    page.latest_round = int(&envelope[0]);
    page.projection_state = "source".into();
    page.thread_empty = Some(page.latest_round == 0);
    if let Some(active) = one(
        db,
        "SELECT status,turn_id FROM thread_turns WHERE user_id=? AND session_id=? AND status IN ('queued','running','waiting_input','waiting_approval') ORDER BY user_turn_index DESC,created_time DESC LIMIT 1",
        &[json!(user), json!(session)],
    )? {
        page.runtime_status = text(&active[0]).into();
        page.active_turn_id = text(&active[1]).into();
    } else {
        page.runtime_status = "idle".into();
        page.active_turn_id.clear();
    }
    let cursor = before.unwrap_or(i64::MAX);
    let cap = if kind == "trajectory" {
        limit.clamp(1, TRAJECTORY_PAGE_MAX)
    } else {
        limit.clamp(1, CHAT_PAGE_MAX)
    };
    let budget = if kind == "trajectory" {
        TRAJECTORY_RESPONSE_BYTES
    } else {
        CHAT_RESPONSE_BYTES
    };
    let mut bytes = serde_json::to_vec(&page)?.len() + 1024;
    if kind == "trajectory" {
        if root_turn_id.is_none() && page.view_seq > super::thread_view_store::MAX_SOURCE_CHANGE_SEQ
        {
            let roots = db.query("SELECT turn_id,user_turn_index FROM thread_turns WHERE user_id=? AND session_id=? AND trigger_kind='user' AND user_turn_index<? ORDER BY user_turn_index DESC LIMIT ?", &[json!(user), json!(session), json!(cursor), json!((cap + 1) as i64)])?;
            page.has_more = roots.len() > cap;
            let selected = roots.into_iter().take(cap).collect::<Vec<_>>();
            page.next_before = selected.first().map(|root| int(&root[1]));
            if !selected.is_empty() {
                // A source-backed overview must still expose the shape of a
                // turn.  Picking only the newest row made every large-thread
                // overview look like an assistant-only transcript because
                // the newest item is normally the final assistant message.
                // Keep at most three bounded representatives per user turn:
                // input, first workflow/tool event, and final assistant.
                let root_ids = selected
                    .iter()
                    .map(|root| text(&root[0]).to_owned())
                    .collect::<Vec<_>>();
                let root_placeholders = placeholders(root_ids.len());
                let row_content = db.json_text("i.payload", "content");
                let row_reasoning = format!(
                    "COALESCE(NULLIF({},''),{})",
                    db.json_text("i.payload", "reasoning"),
                    db.json_text("i.payload", "reasoning_content")
                );
                let row_tool = db.json_text("i.payload", "tool");
                let row_model = db.json_text("i.payload", "model_round");
                let seek_visibility = "x.visibility='user' OR (x.visibility='model_internal' AND x.kind='system_message')";
                let representative_types = if root_turn_id.is_some() {
                    vec![
                        ("system_message", "ASC"),
                        ("user_message", "ASC"),
                        ("tool_call", "ASC"),
                        ("tool_message", "ASC"),
                        ("assistant_message", "DESC"),
                    ]
                } else {
                    // A summary page stays small even when a single user turn
                    // contains thousands of tool and model rows.
                    vec![
                        ("system_message", "ASC"),
                        ("user_message", "ASC"),
                        ("tool_call", "ASC"),
                        ("assistant_message", "DESC"),
                    ]
                };
                // Use one indexed keyset query per representative kind. The
                // correlated subquery resolves the item id from
                // (user,session,root,created_seq) before the payload is read;
                // unlike a window over the whole set it never parses every
                // legacy workflow payload in the selected turns.
                for (kind, order) in representative_types {
                    let sql = format!(
                        "SELECT i.item_id,i.turn_id,i.root_turn_id,i.item_index,i.created_seq,i.revision,i.kind,i.status,i.created_time,i.updated_time,t.user_turn_index,substr(COALESCE(NULLIF({row_content},''),NULLIF({row_reasoning},''),''),1,256),substr(COALESCE({row_content},''),1,256),substr({row_reasoning},1,256),substr(COALESCE({row_tool},''),1,96),COALESCE({row_model},'0') FROM thread_turns t JOIN thread_items i ON i.session_id=t.session_id AND i.item_id=(SELECT x.item_id FROM thread_items x WHERE x.user_id=t.user_id AND x.session_id=t.session_id AND x.root_turn_id=t.turn_id AND x.kind=? AND ({seek_visibility}) ORDER BY x.created_seq {order} LIMIT 1) WHERE t.user_id=? AND t.session_id=? AND t.turn_id IN ({root_placeholders})"
                    );
                    let mut params = vec![json!(kind), json!(user), json!(session)];
                    params.extend(root_ids.iter().cloned().map(Value::String));
                    for row in db.query(&sql, &params)? {
                        let detail = json!({
                            "content": text(&row[12]),
                            "reasoning": text(&row[13]),
                            "tool": text(&row[14]),
                            "model_round": text(&row[15]).parse::<i64>().unwrap_or(0)
                        });
                        let item_id = text(&row[0]);
                        let mut projected = projection::trajectory(
                            &detail,
                            item_id,
                            text(&row[1]),
                            text(&row[2]),
                            int(&row[10]),
                            int(&row[3]),
                            int(&row[4]),
                            int(&row[5]),
                            text(&row[6]),
                            text(&row[7]),
                            float(&row[8]),
                            float(&row[9]),
                        );
                        projected.detail = Some(BodyRef {
                            body_id: projection::body_id(item_id, "source_detail"),
                            revision: int(&row[5]),
                            bytes: 0,
                        });
                        page.trajectory.push(projected);
                    }
                }
            }
            page.trajectory.sort_by(|left, right| {
                left.user_round
                    .cmp(&right.user_round)
                    .then_with(|| left.item_index.cmp(&right.item_index))
                    .then_with(|| left.created_seq.cmp(&right.created_seq))
            });
            return Ok(page);
        }
        let root_filter = if root_turn_id.is_some() {
            " AND i.root_turn_id=?"
        } else {
            ""
        };
        let mut params = vec![json!(user), json!(session)];
        if let Some(root) = root_turn_id {
            params.push(json!(root));
        }
        params.extend([json!(cursor), json!((cap + 1) as i64)]);
        let tool = db.json_text("i.payload", "tool");
        let model = db.json_text("i.payload", "model_round");
        let payload_bytes = db.text_bytes("i.payload");
        let source_content = db.json_text("i.payload", "content");
        let source_reasoning = format!(
            "COALESCE(NULLIF({},''),{})",
            db.json_text("i.payload", "reasoning"),
            db.json_text("i.payload", "reasoning_content")
        );
        let rows = {
            let sql = format!(
                "SELECT i.item_id,i.turn_id,i.root_turn_id,i.item_index,i.created_seq,i.revision,i.kind,i.status,i.created_time,i.updated_time,t.user_turn_index,substr(COALESCE({source_content},''),1,256),substr({source_reasoning},1,256),substr(COALESCE({tool},''),1,96),COALESCE({model},'0'),{payload_bytes} FROM thread_items i JOIN thread_turns t ON t.user_id=i.user_id AND t.session_id=i.session_id AND t.turn_id=i.turn_id WHERE i.user_id=? AND i.session_id=?{root_filter} AND i.created_seq<? AND (i.visibility='user' OR (i.visibility='model_internal' AND i.kind='system_message')) ORDER BY i.created_seq DESC LIMIT ?"
            );
            db.query(&sql, &params)?
        };
        page.has_more = rows.len() > cap;
        for row in rows.into_iter().take(cap) {
            let payload = json!({"content":text(&row[11]),"reasoning":text(&row[12]),"tool":text(&row[13]),"model_round":row[14].as_i64().unwrap_or_else(||text(&row[14]).parse::<i64>().unwrap_or(0))});
            let mut projected = projection::trajectory(
                &payload,
                text(&row[0]),
                text(&row[1]),
                text(&row[2]),
                int(&row[10]),
                int(&row[3]),
                int(&row[4]),
                int(&row[5]),
                text(&row[6]),
                text(&row[7]),
                float(&row[8]),
                float(&row[9]),
            );
            projected.detail = Some(BodyRef {
                body_id: projection::body_id(text(&row[0]), "source_detail"),
                revision: int(&row[5]),
                bytes: int(&row[15]).max(0) as usize,
            });
            let size = serde_json::to_vec(&projected)?.len() + 1;
            if bytes + size > budget {
                page.has_more = true;
                break;
            }
            bytes += size;
            page.next_before = Some(int(&row[4]));
            page.trajectory.push(projected);
        }
        page.trajectory.reverse();
    } else {
        let roots=db.query("SELECT turn_id,user_turn_index,client_message_id,status,created_time,updated_time FROM thread_turns WHERE user_id=? AND session_id=? AND trigger_kind='user' AND user_turn_index<? ORDER BY user_turn_index DESC LIMIT ?",&[json!(user),json!(session),json!(cursor),json!((cap+1) as i64)])?;
        page.has_more = roots.len() > cap;
        source_chat_rows(
            db,
            user,
            session,
            &roots.into_iter().take(cap).collect::<Vec<_>>(),
            &mut page,
            &mut bytes,
            budget,
        )?;
        page.chat.reverse();
    }
    if kind == "trajectory"
        && page.view_seq > 0
        && root_turn_id.is_none()
        && page.view_seq > super::thread_view_store::MAX_SOURCE_CHANGE_SEQ
    {
        while page.view_seq / (128i64 << (page.tile_level * 3)) > 1023 && page.tile_level < 7 {
            page.tile_level += 1;
        }
        for row in db.query("SELECT bucket,count,failed,running,start,finish FROM thread_trajectory_tiles WHERE session_id=? AND level=? ORDER BY bucket LIMIT 1024", &[json!(session), json!(page.tile_level as i64)])? {
            let tile = TrajectoryTile { bucket: int(&row[0]), count: int(&row[1]), failed: int(&row[2]), running: int(&row[3]), start: float(&row[4]), end: float(&row[5]) };
            let size = serde_json::to_vec(&tile)?.len() + 1;
            if bytes + size > TRAJECTORY_RESPONSE_BYTES { break; }
            bytes += size;
            page.tiles.push(tile);
        }
    }
    ensure!(
        serde_json::to_vec(&page)?.len() <= budget,
        "source window exceeds response budget"
    );
    Ok(page)
}

pub(super) fn changes(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    after: i64,
) -> Result<ThreadViewChanges> {
    let row = one(
        db,
        "SELECT latest_change_seq FROM thread_logs WHERE user_id=? AND session_id=?",
        &[json!(user), json!(session)],
    )?;
    let seq = row.map(|row| int(&row[0])).unwrap_or(0);
    let ready = one(
        db,
        "SELECT projection_state FROM thread_view_heads WHERE user_id=? AND session_id=?",
        &[json!(user), json!(session)],
    )?
    .is_some_and(|row| text(&row[0]) == "ready");
    Ok(ThreadViewChanges {
        epoch: -1,
        view_seq: seq,
        reset_required: ready || seq != after.saturating_neg().saturating_sub(1),
        ..Default::default()
    })
}

pub(super) fn body(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    id: &str,
    rev: i64,
    cursor: i64,
) -> Result<ThreadBodyPage> {
    let (item, field) = id
        .rsplit_once('|')
        .ok_or_else(|| anyhow::anyhow!("invalid source body"))?;
    if field == "source_stream" {
        return stream_body(db, user, session, item, id, rev, cursor);
    }
    let expression = match field {
        "source_content" => content(db),
        "source_detail" => "payload".into(),
        _ => anyhow::bail!("invalid source body field"),
    };
    let sql = format!(
        "SELECT substr({expression},CAST(CAST(? AS BIGINT) AS INTEGER),2049) FROM thread_items WHERE user_id=? AND session_id=? AND item_id=? AND revision=? AND (visibility='user' OR (visibility='model_internal' AND kind='system_message')) LIMIT 1"
    );
    let row = one(
        db,
        &sql,
        &[
            json!(cursor.saturating_add(1)),
            json!(user),
            json!(session),
            json!(item),
            json!(rev),
        ],
    )?;
    let mut page = ThreadBodyPage {
        body_id: id.into(),
        revision: rev,
        ..Default::default()
    };
    if let Some(row) = row {
        let value = text(&row[0]);
        let count = value.chars().count();
        page.has_more = count > 2048;
        page.chunks.push(value.chars().take(2048).collect());
        page.next_cursor = Some(cursor.saturating_add(count.min(2048) as i64));
    }
    Ok(page)
}

fn stream_body(
    db: &mut impl ViewDb,
    user: &str,
    session: &str,
    item: &str,
    id: &str,
    rev: i64,
    cursor: i64,
) -> Result<ThreadBodyPage> {
    const STRIDE: i64 = 1_000_000_000;
    let mut page = ThreadBodyPage {
        body_id: id.into(),
        revision: rev,
        ..Default::default()
    };
    if one(db,"SELECT item_id FROM thread_items WHERE user_id=? AND session_id=? AND item_id=? AND revision=? AND visibility='user' LIMIT 1",&[json!(user),json!(session),json!(item),json!(rev)])?.is_none() {return Ok(page);}
    let block = cursor / STRIDE;
    let offset = cursor % STRIDE;
    let expression = block_content(db);
    let sql = format!(
        "SELECT block_index,substr({expression},CASE WHEN block_index=? THEN CAST(CAST(? AS BIGINT) AS INTEGER) ELSE 1 END,2049) FROM thread_item_blocks WHERE user_id=? AND session_id=? AND item_id=? AND field='content' AND block_index>=? ORDER BY block_index LIMIT 2"
    );
    let rows = db.query(
        &sql,
        &[
            json!(block),
            json!(offset + 1),
            json!(user),
            json!(session),
            json!(item),
            json!(block),
        ],
    )?;
    if let Some(row) = rows.first() {
        let index = int(&row[0]);
        let value = text(&row[1]);
        let count = value.chars().count();
        page.chunks.push(value.chars().take(2048).collect());
        page.has_more = count > 2048 || rows.len() > 1;
        page.next_cursor = Some(if count > 2048 {
            index.saturating_mul(STRIDE) + (if index == block { offset } else { 0 }) + 2048
        } else {
            (index + 1).saturating_mul(STRIDE)
        });
    }
    Ok(page)
}
