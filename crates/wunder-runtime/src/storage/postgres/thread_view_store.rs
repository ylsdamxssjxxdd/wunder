//! Bounded service views over the transactional canonical display index.
use super::PostgresThreadLog;
use crate::storage::postgres::{thread_display, PgTx};
use crate::storage::{thread_chat_summary::ChatSummary, thread_view_projection as p};
use anyhow::{anyhow, bail, ensure, Result};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use wunder_core::thread_view::*;

fn wire(raw: &str, ordinal: i64) -> Result<Value> {
    let item = serde_json::from_str(raw)?;
    let mut item = PostgresThreadLog::item_wire(&item, ordinal.max(0) as u64);
    if item["kind"] == "system_message" {
        p::redact_internal_context(&mut item["payload"]);
    }
    Ok(item)
}

fn turn(row: tokio_postgres::Row) -> Result<Value> {
    let control: Option<String> = row.get("control_json");
    Ok(json!({
        "turn_id":row.get::<_,String>("turn_id"), "root_turn_id":row.get::<_,String>("root_turn_id"),
        "user_round":row.get::<_,i64>("user_round"), "status":row.get::<_,String>("status"),
        "client_message_id":row.get::<_,Option<String>>("client_message_id"),
        "created_time":row.get::<_,i64>("created_at_ms") as f64/1000.0,
        "updated_time":row.get::<_,i64>("updated_at_ms") as f64/1000.0,
        "input":row.get::<_,Option<String>>("input_json").as_deref().map(serde_json::from_str::<Value>).transpose()?,
        "control":control.as_deref().map(serde_json::from_str::<Value>).transpose()?.unwrap_or_default(),
        "summary":row.get::<_,Option<String>>("summary_json").as_deref().map(serde_json::from_str::<Value>).transpose()?.unwrap_or_default(),
    }))
}

fn roots(
    tx: &mut PgTx<'_>,
    session: &str,
    before: Option<i64>,
    after: Option<i64>,
    limit: usize,
    include_input: bool,
) -> Result<Vec<Value>> {
    let order = if after.is_some() { "ASC" } else { "DESC" };
    tx.query(&format!("SELECT head.turn_id,root.root_turn_id,root.user_round,head.status,
            root.client_message_id,root.created_at_ms,head.updated_at_ms,
            CASE WHEN NOT $5::boolean OR root.input_json IS NULL THEN NULL ELSE jsonb_build_object(
              'content',left(body.content,512),'_content_bytes',octet_length(body.content),
              '_content_utf16',length(body.content)+length(regexp_replace(body.content,U&'[^\\+010000-\\+10FFFF]','','g')),
              'attachments',(SELECT COALESCE(jsonb_agg(jsonb_build_object(
                'name',left(part->>'name',128),'public_path',left(part->>'public_path',128),
                'mime_type',left(COALESCE(part->>'mime_type',part->>'content_type'),128))),'[]'::jsonb)
                FROM (SELECT part FROM jsonb_array_elements(CASE WHEN jsonb_typeof(input.doc->'attachments')='array'
                  THEN input.doc->'attachments' ELSE '[]'::jsonb END) part LIMIT 8) cards))::text END AS input_json,
            head.control_json,chat.summary_json
        FROM (SELECT * FROM service_thread_admissions
            WHERE thread_id=$1 AND turn_id=root_turn_id
              AND ($2::bigint IS NULL OR user_round<$2) AND ($3::bigint IS NULL OR user_round>$3)
            ORDER BY user_round {order} LIMIT $4) root
        LEFT JOIN LATERAL (SELECT CASE WHEN $5::boolean THEN root.input_json::jsonb END AS doc) input ON TRUE
        LEFT JOIN LATERAL (SELECT CASE jsonb_typeof(input.doc->'content')
          WHEN 'string' THEN input.doc->>'content'
          WHEN 'array' THEN COALESCE((SELECT string_agg(part->>'text',E'\\n' ORDER BY position)
            FROM jsonb_array_elements(input.doc->'content') WITH ORDINALITY parts(part,position)
            WHERE part->>'type'='text' AND jsonb_typeof(part->'text')='string'),'')
          ELSE '' END AS content) body ON TRUE
        JOIN LATERAL (SELECT turn_id,status,updated_at_ms,control_json FROM service_thread_admissions
            WHERE thread_id=$1 AND root_turn_id=root.root_turn_id
            ORDER BY created_at_ms DESC,turn_id DESC LIMIT 1) head ON TRUE
        LEFT JOIN service_thread_chat_heads_v2 chat ON chat.thread_id=$1 AND chat.root_turn_id=root.root_turn_id
        ORDER BY root.user_round {order}"), &[&session,&before,&after,&((limit+1) as i64),&include_input])?
        .into_iter().map(turn).collect()
}

fn message(item: &Value, seq: i64) -> ViewMessage {
    let body = &item["payload"];
    let text = message_content_text(&body["content"]).unwrap_or_default();
    let mut message = p::message(item["item_id"].as_str().unwrap_or_default(), &text, 1);
    message.created_seq = item["created_seq"].as_i64().unwrap_or(0);
    message.durable_seq = seq;
    message.content_offset = body["_content_utf16"]
        .as_u64()
        .unwrap_or(text.encode_utf16().count() as u64) as usize;
    if let Some(reference) = message.body.as_mut() {
        reference.bytes = body["_content_bytes"].as_u64().unwrap_or(text.len() as u64) as usize;
    }
    p::set_reasoning(&mut message, body);
    message.reasoning_offset = body["_reasoning_utf16"]
        .as_u64()
        .unwrap_or(message.reasoning_offset as u64) as usize;
    message.stats = p::finite_stats(body);
    message.question_panel = p::extract_question_panel(body);
    if let Some(attachments) = body["attachments"].as_array() {
        message.attachments = p::user_attachments(attachments);
    }
    message
}

fn chat(turn: &Value, items: &HashMap<String, Value>, seq: i64) -> Result<ChatViewRow> {
    let root = turn["root_turn_id"].as_str().unwrap_or_default();
    let mut row = p::new_chat(
        root,
        turn["user_round"].as_i64().unwrap_or_default(),
        turn["client_message_id"].as_str().map(str::to_owned),
        turn["status"].as_str().unwrap_or_default(),
        turn["created_time"].as_f64().unwrap_or_default(),
        turn["updated_time"].as_f64().unwrap_or_default(),
    );
    row.execution_turn_id = turn["turn_id"].as_str().unwrap_or(root).into();
    row.revision = seq;
    row.stop_reason = turn
        .pointer("/control/stop_reason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if row.status == "queued" {
        row.queued_input = turn
            .pointer("/control/queued_input")
            .filter(|v| !v.is_null())
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
    }
    if let Some(input) = turn.get("input").filter(|v| v.is_object()) {
        let text = message_content_text(&input["content"]).unwrap_or_default();
        row.user = p::message(&format!("{root}:user"), &text, 1);
        row.user.content_offset = input["_content_utf16"].as_u64().unwrap_or(0) as usize;
        if let Some(reference) = row.user.body.as_mut() {
            reference.bytes = input["_content_bytes"].as_u64().unwrap_or(0) as usize;
        }
        if let Some(attachments) = input["attachments"].as_array() {
            row.user.attachments = p::user_attachments(attachments);
        }
    }
    let summary: ChatSummary = serde_json::from_value(turn["summary"].clone()).unwrap_or_default();
    if let Some(item) = summary.user_item_id.as_ref().and_then(|id| items.get(id)) {
        row.user = message(item, seq);
    }
    if let Some(item) = summary
        .assistant_item_id
        .as_ref()
        .and_then(|id| items.get(id))
    {
        row.assistant = message(item, seq);
    }
    if let Some(item) = summary
        .question_panel_item_id
        .as_ref()
        .and_then(|id| items.get(id))
    {
        row.assistant.question_panel = p::extract_question_panel(&item["payload"]);
    }
    if let Some(item) = summary.plan_item_id.as_ref().and_then(|id| items.get(id)) {
        row.plan = p::plan(
            &item["payload"],
            item["item_id"].as_str().unwrap_or_default(),
            1,
            item["created_seq"].as_i64().unwrap_or_default(),
        );
    }
    row.workflow_count = summary.workflow_count;
    row.process_count =
        Some((summary.process_count - i64::from(summary.assistant_process_text)).max(0));
    row.workspace_change_seq = summary.workspace_change_seq;
    row.subagents = summary.subagents;
    row.reasoning_count = summary.reasoning_count;
    if let Some(item) = summary
        .reasoning_item_id
        .as_ref()
        .filter(|id| **id != row.assistant.item_id)
        .and_then(|id| items.get(id))
    {
        p::set_reasoning(&mut row.assistant, &item["payload"]);
    }
    row.reasoning_item_id = summary.reasoning_item_id.unwrap_or_default();
    p::settle_question_panel(&mut row);
    Ok(row)
}

#[cfg(test)]
mod inquiry_tests {
    use super::*;
    use wunder_core::thread_store::CanonicalItem;

    #[test]
    fn service_chat_reasoning_identity_and_body_stay_paired_across_model_rounds() {
        let summary = ChatSummary {
            assistant_item_id: Some("fixture-turn:text-1".into()),
            reasoning_item_id: Some("fixture-turn:text-2".into()),
            process_count: 3,
            assistant_process_text: true,
            ..Default::default()
        };
        let turn = json!({"turn_id":"fixture-turn","root_turn_id":"fixture-turn",
            "user_round":1,"status":"running","summary":summary});
        let items = HashMap::from([
            (
                "fixture-turn:text-1".into(),
                json!({"item_id":"fixture-turn:text-1","payload":{
                "content":"Fixture answer","reasoning":"Fixture completed thought"}}),
            ),
            (
                "fixture-turn:text-2".into(),
                json!({"item_id":"fixture-turn:text-2","payload":{
                "reasoning":"Fixture next thought"}}),
            ),
        ]);
        let row = chat(&turn, &items, 3).unwrap();
        assert_eq!(row.assistant.item_id, "fixture-turn:text-1");
        assert_eq!(row.reasoning_item_id, "fixture-turn:text-2");
        assert_eq!(row.assistant.reasoning, "Fixture next thought");
        assert_eq!(row.process_count, Some(2));
    }

    #[test]
    fn service_chat_preserves_explicit_tool_counts_without_inventing_unknown_values() {
        for (stats, expected) in [
            (json!({"tool_calls":0}), Some(0)),
            (json!({"toolCalls":2}), Some(2)),
            (json!({"interaction_duration_s":1}), None),
        ] {
            let summary = ChatSummary {
                assistant_item_id: Some("sample-answer".into()),
                ..Default::default()
            };
            let turn = json!({"turn_id":"sample-turn","root_turn_id":"sample-turn",
                "user_round":1,"status":"completed","summary":summary});
            let items = HashMap::from([(
                "sample-answer".into(),
                json!({
                    "item_id":"sample-answer","created_seq":2,
                    "payload":{"content":"sample answer","message_stats":stats}
                }),
            )]);
            let row = chat(&turn, &items, 3).unwrap();
            let projected = row.assistant.stats.as_ref().unwrap();
            assert_eq!(
                projected
                    .get("toolCalls")
                    .or_else(|| projected.get("tool_calls"))
                    .and_then(Value::as_u64),
                expected
            );
        }
    }

    #[test]
    fn service_chat_keeps_completed_panel_pending_alongside_plan() {
        let mut summary = ChatSummary::default();
        let mut items = HashMap::new();
        for (ordinal, event, data) in [
            (
                0,
                "plan_update",
                json!({"plan":[{"step":"Fixture step","status":"in_progress"}]}),
            ),
            (
                1,
                "question_panel",
                json!({"questions":[{"question":"Fixture choice?","options":[{"label":"Option A"}]}]}),
            ),
        ] {
            let mut data = data;
            data["turn_id"] = json!("fixture-turn");
            let event_item =
                crate::services::thread_log::event_item("fixture-thread", event, &data).unwrap();
            let canonical: CanonicalItem = serde_json::from_value(json!({
                "item_id":event_item["item_id"],"turn_id":"fixture-turn","root_turn_id":"fixture-turn",
                "created_at_ms":1,"kind":if event=="plan_update" {"plan"} else {"configuration"},
                "payload":event_item
            })).unwrap();
            let preview = crate::storage::thread_history::item_preview(
                &serde_json::to_value(canonical).unwrap(),
            );
            let id = preview["item_id"].as_str().unwrap();
            let kind = preview["payload"]["kind"].as_str().unwrap();
            crate::storage::thread_chat_summary::apply_item(
                &mut summary,
                "fixture-turn",
                id,
                kind,
                ordinal,
                true,
                &preview,
            );
            items.insert(id.to_owned(), wire(&preview.to_string(), ordinal).unwrap());
        }
        for (status, expected) in [
            ("running", "pending"),
            ("waiting_input", "pending"),
            ("completed", "answered"),
            ("cancelled", "dismissed"),
        ] {
            let row = chat(
                &json!({"turn_id":"fixture-turn","root_turn_id":"fixture-turn",
                "user_round":1,"status":status,"summary":summary}),
                &items,
                2,
            )
            .unwrap();
            assert_eq!(
                row.assistant.question_panel.as_ref().unwrap().status,
                expected
            );
            assert_eq!(row.plan.as_ref().unwrap().steps[0].step, "Fixture step");
        }
    }
}

fn trajectory(item: &Value, ordinal: i64, round: i64) -> TrajectoryViewRow {
    let turn = item["turn_id"].as_str().unwrap_or_default();
    p::trajectory(
        &item["payload"],
        item["item_id"].as_str().unwrap_or_default(),
        turn,
        item["root_turn_id"].as_str().unwrap_or(turn),
        round,
        ordinal,
        ordinal + 1,
        1,
        item["kind"].as_str().unwrap_or_default(),
        item["status"].as_str().unwrap_or("completed"),
        item["created_time"].as_f64().unwrap_or_default(),
        item["updated_time"].as_f64().unwrap_or_default(),
    )
}

impl PostgresThreadLog<'_> {
    pub(crate) fn thread_view_window(
        &self,
        user: &str,
        session: &str,
        kind: &str,
        before: Option<i64>,
        after: Option<i64>,
        root: Option<&str>,
        limit: usize,
    ) -> Result<ThreadViewPage> {
        ensure!(
            before.is_none() || after.is_none(),
            "before and after are mutually exclusive"
        );
        ensure!(matches!(kind, "chat" | "trajectory"), "invalid view kind");
        self.check_owner(user, session)?;
        thread_display::ensure_ready(self.storage, session)?;
        let mut conn = self.storage.conn()?;
        let mut tx = conn.transaction()?;
        tx.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL statement_timeout='2000ms'")?;
        let seq = tx
            .query_opt(
                "SELECT history_revision FROM service_threads WHERE thread_id=$1",
                &[&session],
            )?
            .map(|r| r.get(0))
            .unwrap_or(0);
        let latest = tx
            .query_opt(
                "SELECT latest_user_round FROM service_thread_owners WHERE thread_id=$1",
                &[&session],
            )?
            .map(|r| r.get(0))
            .unwrap_or(0);
        let mut page = ThreadViewPage {
            session_id: session.into(),
            epoch: 1,
            view_seq: seq,
            projection_state: "ready".into(),
            latest_round: latest,
            thread_empty: Some(latest == 0),
            ..Default::default()
        };
        // The execution head is independent of queued inputs on newer roots.
        if let Some(head)=tx.query_opt("SELECT turn_id,status FROM (
            (SELECT turn_id,status,user_round,created_at_ms,1 AS priority FROM service_thread_admissions
              WHERE thread_id=$1 AND status='running' ORDER BY user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1)
            UNION ALL
            (SELECT turn_id,status,user_round,created_at_ms,1 AS priority FROM service_thread_admissions
              WHERE thread_id=$1 AND status='waiting_input' ORDER BY user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1)
            UNION ALL
            (SELECT turn_id,status,user_round,created_at_ms,1 AS priority FROM service_thread_admissions
              WHERE thread_id=$1 AND status='waiting_approval' ORDER BY user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1)
            UNION ALL
            (SELECT turn_id,status,user_round,created_at_ms,0 AS priority FROM service_thread_admissions
              WHERE thread_id=$1 ORDER BY user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1)
            ) heads ORDER BY priority DESC,user_round DESC,created_at_ms DESC,turn_id DESC LIMIT 1",&[&session])? {
            page.runtime_status=head.get(1);
            page.status_turn_id=head.get(0);
            if p::is_running(&page.runtime_status)!=0 { page.active_turn_id=head.get(0); }
        }
        if kind == "chat" {
            let limit = limit.clamp(1, CHAT_PAGE_MAX);
            let mut turns = roots(&mut tx, session, before, after, limit, true)?;
            page.has_more = turns.len() > limit;
            turns.truncate(limit);
            let ids = turns
                .iter()
                .flat_map(|t| {
                    [
                        "user_item_id",
                        "assistant_item_id",
                        "reasoning_item_id",
                        "question_panel_item_id",
                        "plan_item_id",
                    ]
                    .into_iter()
                    .filter_map(|field| t["summary"][field].as_str().map(str::to_owned))
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let mut items = HashMap::new();
            if !ids.is_empty() {
                for row in tx.query("SELECT item_id,ordinal,preview_json FROM service_thread_items_v2 WHERE thread_id=$1 AND item_id=ANY($2) AND visible",&[&session,&ids])? {
                    items.insert(row.get::<_,String>(0),wire(&row.get::<_,String>(2),row.get(1))?);
                }
            }
            let feedback_keys = turns
                .iter()
                .filter_map(|turn| turn["summary"]["assistant_item_id"].as_str())
                .map(|id| format!("thread_feedback:{session}:{id}"))
                .collect::<Vec<_>>();
            let feedback = if feedback_keys.is_empty() {
                HashMap::new()
            } else {
                tx.query(
                    "SELECT key,value FROM meta WHERE key=ANY($1)",
                    &[&feedback_keys],
                )?
                .into_iter()
                .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
                .collect::<HashMap<_, _>>()
            };
            page.chat = turns
                .iter()
                .map(|t| chat(t, &items, seq))
                .collect::<Result<_>>()?;
            for row in &mut page.chat {
                if let Some(vote) = feedback.get(&format!(
                    "thread_feedback:{session}:{}",
                    row.assistant.item_id
                )) {
                    row.assistant.feedback = Some(json!({"vote":vote}));
                }
            }
            page.chat.sort_by_key(|r| r.user_round);
            // Bound the actual wire page as attachment cards and diagnostics
            // can vary in size. Keep the side nearest the requested cursor.
            let mut bytes = serde_json::to_vec(&page)?.len();
            let mut removed = 0;
            while bytes > CHAT_RESPONSE_BYTES - 1024 && page.chat.len() - removed > 1 {
                let index = if after.is_some() {
                    page.chat.len() - removed - 1
                } else {
                    removed
                };
                let row = &page.chat[index];
                bytes = bytes.saturating_sub(serde_json::to_vec(row)?.len() + 1);
                removed += 1;
            }
            if removed > 0 {
                if after.is_some() {
                    page.chat.truncate(page.chat.len() - removed);
                } else {
                    page.chat.drain(..removed);
                }
                page.has_more = true;
            }
            page.next_before = page.chat.first().map(|r| r.user_round);
            page.next_after = page.chat.last().map(|r| r.user_round);
            page.has_newer = page.next_after.is_some_and(|v| v < latest);
        } else {
            let limit = limit.clamp(1, TRAJECTORY_PAGE_MAX);
            let order = if after.is_some() { "ASC" } else { "DESC" };
            let rows = tx.query(
                &format!(
                    "SELECT i.ordinal,i.preview_json,COALESCE(a.user_round,0) AS user_round
                FROM service_thread_items_v2 i LEFT JOIN service_thread_admissions a
                  ON a.thread_id=i.thread_id AND a.turn_id=i.root_turn_id
                WHERE i.thread_id=$1 AND ($2::text IS NULL OR i.root_turn_id=$2) AND i.visible
                  AND ($3::bigint IS NULL OR i.ordinal<$3) AND ($4::bigint IS NULL OR i.ordinal>$4)
                ORDER BY i.ordinal {order} LIMIT $5"
                ),
                &[&session, &root, &before, &after, &((limit + 1) as i64)],
            )?;
            page.has_more = rows.len() > limit;
            for row in rows.into_iter().take(limit) {
                let ordinal = row.get(0);
                let item = wire(&row.get::<_, String>(1), ordinal)?;
                page.trajectory.push(trajectory(&item, ordinal, row.get(2)));
            }
            page.trajectory.sort_by_key(|r| r.item_index);
            page.next_before = page.trajectory.first().map(|r| r.item_index);
            page.next_after = page.trajectory.last().map(|r| r.item_index);
            if let Some(cursor) = page.next_after {
                page.has_newer=tx.query_one("SELECT EXISTS(SELECT 1 FROM service_thread_items_v2 WHERE thread_id=$1 AND ($2::text IS NULL OR root_turn_id=$2) AND visible AND ordinal>$3)",&[&session,&root,&cursor])?.get(0);
            }
        }
        Ok(page)
    }

    pub(crate) fn thread_view_page(
        &self,
        u: &str,
        s: &str,
        k: &str,
        b: Option<i64>,
        l: usize,
    ) -> Result<ThreadViewPage> {
        self.thread_view_window(u, s, k, b, None, None, l)
    }
    pub(crate) fn thread_view_trajectory_page(
        &self,
        u: &str,
        s: &str,
        b: Option<i64>,
        r: Option<&str>,
        l: usize,
    ) -> Result<ThreadViewPage> {
        self.thread_view_window(u, s, "trajectory", b, None, r, l)
    }
    pub(crate) fn thread_view_changes(
        &self,
        u: &str,
        s: &str,
        a: i64,
    ) -> Result<ThreadViewChanges> {
        let page = self.thread_view_page(u, s, "chat", None, CHAT_PAGE_MAX)?;
        Ok(ThreadViewChanges {
            runtime_status: page.runtime_status,
            status_turn_id: page.status_turn_id,
            active_turn_id: page.active_turn_id,
            epoch: page.epoch,
            view_seq: page.view_seq,
            reset_required: a > page.view_seq,
            rows: page.chat,
        })
    }

    pub(crate) fn thread_body_page(
        &self,
        user: &str,
        session: &str,
        id: &str,
        revision: i64,
        cursor: i64,
    ) -> Result<ThreadBodyPage> {
        ensure!(revision >= 0, "invalid body revision");
        let (item_id, field) = id
            .rsplit_once('|')
            .ok_or_else(|| anyhow!("invalid body identity"))?;
        let item = match self.get_thread_item(user, session, item_id, false)? {
            Some(item) => item,
            None if field == "content" && item_id.ends_with(":user") => {
                // Accepted inputs have not reached the canonical item stream yet.
                // Their bounded row preview still needs a complete lazy body read.
                let root = item_id.strip_suffix(":user").unwrap_or_default();
                let pending = self
                    .storage
                    .conn()?
                    .query_opt(
                        "SELECT input_json FROM service_thread_admissions a
                     JOIN service_thread_owners owner ON owner.thread_id=a.thread_id
                     WHERE a.thread_id=$1 AND a.turn_id=$2 AND owner.user_id=$3",
                        &[&session, &root, &user],
                    )?
                    .and_then(|row| row.get::<_, Option<String>>(0))
                    .map(|raw| serde_json::from_str::<Value>(&raw))
                    .transpose()?;
                let pending = pending.ok_or_else(|| anyhow!("item not found"))?;
                json!({"kind":"user_message","visibility":"user","payload":pending})
            }
            None => bail!("item not found"),
        };
        let mut body = item["payload"].clone();
        ensure!(
            item["visibility"] == "user"
                && body.pointer("/meta/hidden").and_then(Value::as_bool) != Some(true),
            "item not found"
        );
        if item["kind"] == "system_message" {
            p::redact_internal_context(&mut body);
        }
        p::attach_tool_schema(&mut body);
        let text = match field {
            "content" => message_content_text(&body["content"]).unwrap_or_default(),
            "reasoning" => body
                .get("reasoning")
                .or_else(|| body.get("reasoning_content"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            "detail" => serde_json::to_string_pretty(&body)?,
            _ => bail!("unknown body field"),
        };
        let mut start = usize::try_from(cursor).map_err(|_| anyhow!("invalid body cursor"))?;
        ensure!(
            start <= text.len() && text.is_char_boundary(start),
            "invalid body cursor"
        );
        let chunks = chunks(&text[start..])
            .take(8)
            .map(|s| {
                start += s.len();
                s.to_owned()
            })
            .collect();
        Ok(ThreadBodyPage {
            body_id: id.into(),
            revision: 1,
            chunks,
            next_cursor: (start < text.len()).then_some(start as i64),
            has_more: start < text.len(),
        })
    }

    pub(crate) fn thread_view_search(
        &self,
        user: &str,
        session: &str,
        q: &str,
    ) -> Result<ThreadSearchPage> {
        self.check_owner(user, session)?;
        if q.trim().is_empty() {
            return Ok(ThreadSearchPage::default());
        }
        ensure!(q.len() <= 1024, "search query exceeds byte limit");
        thread_display::ensure_ready(self.storage, session)?;
        let mut conn = self.storage.conn()?;
        let mut tx = conn.transaction()?;
        tx.batch_execute("SET TRANSACTION READ ONLY; SET LOCAL statement_timeout='300ms'; SET LOCAL lock_timeout='100ms'")?;
        // Decode and search on the database side: serialized JSON can escape
        // Unicode/newlines and contains internal fields that are not message
        // content. Only bounded snippets and scalar lengths cross the wire.
        // A complete display preview can reject a non-match before decoding its
        // canonical body. Missing metadata, wrapped or truncated previews always
        // fall through to full canonical search; preview text is never a hit.
        let rows = tx.query(
            "SELECT i.item_id,i.ordinal,octet_length(text.content) AS bytes,
                substring(text.content FROM strpos(text.content,$2) FOR 256) AS snippet
            FROM (SELECT * FROM service_thread_items_v2
                WHERE thread_id=$1 AND visible AND CASE
                    WHEN strpos(preview_json,$2)>0 THEN true
                    WHEN octet_length(preview_json)<4096 THEN
                        COALESCE(jsonb_typeof(preview_json::jsonb #> '{payload,content}'),'null')<>'string'
                        OR (preview_json::jsonb #> '{payload,_content_bytes}') IS DISTINCT FROM
                            to_jsonb(octet_length(preview_json::jsonb #>> '{payload,content}'))
                        OR strpos(preview_json::jsonb #>> '{payload,content}',$2)>0
                    ELSE true END) i
            JOIN service_thread_owners owner ON owner.thread_id=i.thread_id AND owner.user_id=$3
            JOIN service_thread_records r ON r.thread_id=i.thread_id AND r.ordinal=i.ordinal
            CROSS JOIN LATERAL (SELECT r.record_json::jsonb #> '{payload,payload}' AS item OFFSET 0) canonical
            CROSS JOIN LATERAL (SELECT canonical.item->'payload' AS payload,
                CASE WHEN jsonb_typeof(canonical.item #> '{payload,kind}')='string'
                    THEN canonical.item #>> '{payload,kind}' ELSE canonical.item->>'kind' END AS kind) raw
            CROSS JOIN LATERAL (SELECT CASE
                WHEN raw.payload ? 'item_id' AND jsonb_typeof(raw.payload->'payload')='object'
                    THEN raw.payload->'payload'
                WHEN jsonb_typeof(raw.payload)='object' THEN raw.payload
                ELSE jsonb_build_object('content',raw.payload) END AS payload OFFSET 0) body
            CROSS JOIN LATERAL (SELECT CASE jsonb_typeof(body.payload->'content')
                WHEN 'string' THEN body.payload->>'content'
                WHEN 'array' THEN CASE WHEN jsonb_array_length(body.payload->'content')>0
                    AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(body.payload->'content') part
                        WHERE jsonb_typeof(part)<>'object' OR NOT (part ? 'type'))
                    THEN COALESCE((SELECT string_agg(part->>'text',E'\n' ORDER BY position)
                        FROM jsonb_array_elements(body.payload->'content') WITH ORDINALITY parts(part,position)
                        WHERE part->>'type'='text' AND jsonb_typeof(part->'text')='string'),'') ELSE '' END
                ELSE '' END AS content OFFSET 0) text
            WHERE i.thread_id=$1 AND i.visible AND r.record_type='item_completed'
                AND (CASE WHEN jsonb_typeof(raw.payload->'visibility')='string'
                    THEN raw.payload->>'visibility' ELSE 'user' END)='user'
                AND (CASE WHEN jsonb_typeof(body.payload->'visibility')='string'
                    THEN body.payload->>'visibility' ELSE 'user' END)='user'
                AND (raw.payload #> '{meta,hidden}') IS DISTINCT FROM 'true'::jsonb
                AND (body.payload #> '{meta,hidden}') IS DISTINCT FROM 'true'::jsonb
                AND (raw.kind<>'system_message' OR body.payload #>> '{meta,type}'='compaction_summary'
                    OR (jsonb_typeof(body.payload #> '{meta,compaction_id}')='string'
                        AND body.payload #>> '{meta,compaction_id}'<>''))
                AND strpos(text.content,$2)>0
            ORDER BY i.ordinal DESC LIMIT 21",
            &[&session, &q, &user],
        )?;
        let has_more = rows.len() > 20;
        let hits = rows
            .into_iter()
            .take(20)
            .map(|row| {
                let id: String = row.get("item_id");
                ThreadSearchHit {
                    item_id: id.clone(),
                    body: BodyRef {
                        body_id: format!("{id}|content"),
                        revision: 1,
                        bytes: row.get::<_, i32>("bytes").max(0) as usize,
                    },
                    cursor: row.get("ordinal"),
                    snippet: preview(&row.get::<_, String>("snippet"), 256),
                }
            })
            .collect();
        Ok(ThreadSearchPage { hits, has_more })
    }

    pub(crate) fn rebuild_thread_view_page(
        &self,
        u: &str,
        s: &str,
        _after: i64,
    ) -> Result<(i64, bool)> {
        self.check_owner(u, s)?;
        thread_display::ensure_ready(self.storage, s)?;
        Ok((self.latest_thread_change_seq_by_session(s)?, false))
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::{postgres::PostgresStorage, ThreadLogStore};
    use serde_json::{json, Value};

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_execution_head_retains_status_identity_after_settlement() {
        let storage =
            PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 4).unwrap();
        let store = storage.service_threads().unwrap();
        let admissions = store.admission_store();
        let owner = "fixture-owner";
        let session = "fixture-thread";
        let first = admissions
            .accept(owner, session, &json!({"content":"fixture input"}))
            .unwrap();
        let first_id = first["turn_id"].as_str().unwrap();
        admissions
            .update_with_control(session, first_id, "running", false, &Value::Null)
            .unwrap();
        let next = admissions
            .accept(owner, session, &json!({"content":"fixture next input"}))
            .unwrap();
        let next_id = next["turn_id"].as_str().unwrap();
        admissions
            .update_with_control(session, next_id, "queued", false, &Value::Null)
            .unwrap();
        let page = storage
            .thread_view_page(owner, session, "chat", None, 12)
            .unwrap();
        let tails = storage.thread_turn_tails(owner, &[session.into()]).unwrap();
        assert_eq!(page.runtime_status, "running");
        assert_eq!(page.status_turn_id, first_id);
        assert_eq!(
            tails[session].runtime_head.as_ref().unwrap().status_turn_id,
            first_id
        );
        admissions
            .update_with_control(session, first_id, "completed", false, &Value::Null)
            .unwrap();
        admissions
            .update_with_control(session, next_id, "running", false, &Value::Null)
            .unwrap();
        admissions
            .update_with_control(session, next_id, "completed", false, &Value::Null)
            .unwrap();
        let page = storage
            .thread_view_page(owner, session, "chat", None, 12)
            .unwrap();
        let changes = storage.thread_view_changes(owner, session, 0).unwrap();
        let tails = storage.thread_turn_tails(owner, &[session.into()]).unwrap();
        let head = tails[session].runtime_head.as_ref().unwrap();
        assert_eq!(page.runtime_status, "completed");
        assert_eq!(page.status_turn_id, next_id);
        assert!(page.active_turn_id.is_empty());
        assert_eq!(changes.status_turn_id, next_id);
        assert_eq!(head.status_turn_id, next_id);
        assert!(head.active_turn_id.is_empty());
    }
}
