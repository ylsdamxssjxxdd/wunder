//! Rebuildable, bounded service presentation index. Canonical records remain
//! authoritative; index, chat heads and watermark commit in the same transaction.
use crate::storage::postgres::{PgTx, PostgresStorage};
use crate::storage::{
    thread_chat_summary::{self, ChatSummary},
    thread_history::item_preview,
};
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use wunder_core::thread_store::{CanonicalEvent, CanonicalRecord, RolloutRecord};

const REBUILD_BATCH_BYTES: usize = 2 * 1024 * 1024;

fn rebuild_batch_end(
    position: i64,
    target: i64,
    sizes: impl IntoIterator<Item = (i64, i32)>,
) -> Result<i64> {
    ensure!(
        position >= 0 && position < target,
        "invalid display rebuild position"
    );
    let mut next = position;
    let mut bytes = 0usize;
    for (ordinal, size) in sizes {
        ensure!(
            ordinal == next && ordinal < target,
            "canonical history has an ordinal gap"
        );
        let size = usize::try_from(size)?;
        ensure!(
            size > 0 && size <= crate::storage::rollout::ROLLOUT_MAX_RECORD_BYTES,
            "canonical record exceeds display rebuild byte budget"
        );
        // One valid record may exceed the batch budget, but never fetch a
        // second body with it. Inspect lengths before materializing any body.
        if next > position && bytes.saturating_add(size) > REBUILD_BATCH_BYTES {
            break;
        }
        bytes += size;
        next = next
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("canonical ordinal overflow"))?;
        if bytes >= REBUILD_BATCH_BYTES {
            break;
        }
    }
    ensure!(
        next > position,
        "canonical history is missing before its committed boundary"
    );
    Ok(next)
}

pub(super) fn initialize(conn: &mut PgTx<'_>) -> Result<()> {
    conn.batch_execute("CREATE TABLE IF NOT EXISTS service_thread_items (
        thread_id TEXT NOT NULL REFERENCES service_threads(thread_id) ON DELETE CASCADE,
        item_id TEXT NOT NULL, turn_id TEXT NOT NULL, root_turn_id TEXT NOT NULL,
        ordinal BIGINT NOT NULL, visible BOOLEAN NOT NULL, preview_json TEXT NOT NULL,
        PRIMARY KEY(thread_id,item_id), UNIQUE(thread_id,ordinal)
    );
    CREATE INDEX IF NOT EXISTS service_thread_items_root ON service_thread_items(thread_id,root_turn_id,ordinal);
    CREATE INDEX IF NOT EXISTS service_thread_items_visible ON service_thread_items(thread_id,ordinal) WHERE visible;
    CREATE TABLE IF NOT EXISTS service_thread_chat_heads (
        thread_id TEXT NOT NULL REFERENCES service_threads(thread_id) ON DELETE CASCADE,
        root_turn_id TEXT NOT NULL, summary_json TEXT NOT NULL,
        PRIMARY KEY(thread_id,root_turn_id)
    );
    CREATE TABLE IF NOT EXISTS service_thread_display_state (
        thread_id TEXT PRIMARY KEY REFERENCES service_threads(thread_id) ON DELETE CASCADE,
        next_ordinal BIGINT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS service_thread_admission_root
      ON service_thread_admissions(thread_id,root_turn_id,created_at_ms DESC,turn_id DESC);
    CREATE INDEX IF NOT EXISTS service_thread_admission_roots
      ON service_thread_admissions(thread_id,user_round) WHERE turn_id=root_turn_id;
    CREATE INDEX IF NOT EXISTS service_thread_admission_head
      ON service_thread_admissions(thread_id,user_round DESC,created_at_ms DESC,turn_id DESC);
    CREATE INDEX IF NOT EXISTS service_thread_admission_active
      ON service_thread_admissions(thread_id,status,user_round DESC,created_at_ms DESC,turn_id DESC)
      WHERE status IN ('running','waiting_input','waiting_approval');")
}

pub(super) fn project_batch(
    tx: &mut PgTx<'_>,
    thread: &str,
    records: &[RolloutRecord],
    next: i64,
) -> Result<()> {
    let mut heads: HashMap<String, ChatSummary> = HashMap::new();
    for record in records {
        let CanonicalRecord::Event(CanonicalEvent::ItemCompleted(item)) = &record.record else {
            continue;
        };
        let turn = item.turn_id.as_str();
        let root = match &item.root_turn_id {
            Some(root) => root.to_string(),
            None => tx.query_opt("SELECT root_turn_id FROM service_thread_admissions WHERE thread_id=$1 AND turn_id=$2", &[&thread,&turn])?.map(|r|r.get(0)).unwrap_or_else(||turn.into()),
        };
        let mut value = serde_json::to_value(item)?;
        value["root_turn_id"] = json!(root);
        let payload = &value["payload"];
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .or_else(|| value["kind"].as_str())
            .unwrap_or("configuration")
            .to_owned();
        let payload = &value["payload"];
        let body = if payload.get("item_id").is_some() && payload.get("payload").is_some() {
            &payload["payload"]
        } else {
            payload
        };
        let visible = payload
            .get("visibility")
            .and_then(Value::as_str)
            .unwrap_or("user")
            == "user"
            && body
                .get("visibility")
                .and_then(Value::as_str)
                .unwrap_or("user")
                == "user"
            && payload.pointer("/meta/hidden").and_then(Value::as_bool) != Some(true)
            && payload
                .pointer("/payload/meta/hidden")
                .and_then(Value::as_bool)
                != Some(true)
            && (kind != "system_message"
                || crate::storage::thread_view_projection::is_compaction_system_message(
                    &kind, body,
                ));
        let preview = item_preview(&value);
        let ordinal = record.ordinal as i64;
        let inserted = tx.execute("INSERT INTO service_thread_items(thread_id,item_id,turn_id,root_turn_id,ordinal,visible,preview_json)
            VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(thread_id,item_id) DO NOTHING",
            &[&thread,&item.item_id.as_str(),&turn,&root,&ordinal,&visible,&serde_json::to_string(&preview)?])?;
        if inserted == 0 {
            continue;
        }
        if !heads.contains_key(&root) {
            let raw: Option<String> = tx.query_opt("SELECT summary_json FROM service_thread_chat_heads WHERE thread_id=$1 AND root_turn_id=$2", &[&thread,&root])?.map(|r|r.get(0));
            heads.insert(
                root.clone(),
                raw.as_deref()
                    .map(serde_json::from_str)
                    .transpose()?
                    .unwrap_or_default(),
            );
        }
        thread_chat_summary::apply_item(
            heads.get_mut(&root).expect("loaded head"),
            turn,
            item.item_id.as_str(),
            &kind,
            ordinal,
            visible,
            &preview,
        );
        // Interactive panels can be carried by an otherwise ordinary tool result.
        if visible
            && crate::storage::thread_view_projection::extract_question_panel(&preview["payload"])
                .is_some()
        {
            heads
                .get_mut(&root)
                .expect("loaded head")
                .question_panel_item_id = Some(item.item_id.to_string());
        }
    }
    for (root, summary) in heads {
        tx.execute("INSERT INTO service_thread_chat_heads(thread_id,root_turn_id,summary_json) VALUES($1,$2,$3)
            ON CONFLICT(thread_id,root_turn_id) DO UPDATE SET summary_json=excluded.summary_json", &[&thread,&root,&serde_json::to_string(&summary)?])?;
    }
    tx.execute("INSERT INTO service_thread_display_state(thread_id,next_ordinal) VALUES($1,$2)
        ON CONFLICT(thread_id) DO UPDATE SET next_ordinal=GREATEST(service_thread_display_state.next_ordinal,excluded.next_ordinal)", &[&thread,&next])?;
    Ok(())
}

/// Upgrade only an accessed thread in resumable, bounded transactions. Pin
/// the target revision so readers do not chase concurrently appended history.
pub(super) fn ensure_ready(storage: &PostgresStorage, thread: &str) -> Result<()> {
    let mut conn = storage.conn()?;
    let Some(row) = conn.query_opt(
        "SELECT t.history_revision,COALESCE(p.next_ordinal,0) FROM service_threads t
        LEFT JOIN service_thread_display_state p ON p.thread_id=t.thread_id WHERE t.thread_id=$1",
        &[&thread],
    )?
    else {
        return Ok(());
    };
    let target: i64 = row.get(0);
    if row.get::<_, i64>(1) >= target {
        return Ok(());
    }
    loop {
        let mut tx = conn.transaction()?;
        tx.query_one(
            "SELECT history_revision FROM service_threads WHERE thread_id=$1 FOR UPDATE",
            &[&thread],
        )?;
        let position = tx
            .query_opt(
                "SELECT next_ordinal FROM service_thread_display_state WHERE thread_id=$1",
                &[&thread],
            )?
            .map(|r| r.get::<_, i64>(0))
            .unwrap_or(0);
        if position >= target {
            return Ok(());
        }
        let sizes = tx.query("SELECT ordinal,octet_length(record_json) FROM service_thread_records WHERE thread_id=$1 AND ordinal>=$2 AND ordinal<$3 ORDER BY ordinal LIMIT 128", &[&thread,&position,&target])?;
        let next = rebuild_batch_end(
            position,
            target,
            sizes.iter().map(|row| (row.get(0), row.get(1))),
        )?;
        let rows = tx.query("SELECT ordinal,record_json FROM service_thread_records WHERE thread_id=$1 AND ordinal>=$2 AND ordinal<$3 ORDER BY ordinal", &[&thread,&position,&next])?;
        ensure!(
            rows.len() as i64 == next - position,
            "canonical history changed during display rebuild"
        );
        let mut records = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let ordinal: i64 = row.get(0);
            ensure!(
                ordinal == position + index as i64,
                "canonical history has an ordinal gap"
            );
            let raw: &str = row.get(1);
            ensure!(
                raw.len() <= crate::storage::rollout::ROLLOUT_MAX_RECORD_BYTES,
                "canonical record exceeds display rebuild byte budget"
            );
            let record: RolloutRecord = serde_json::from_str(raw)?;
            ensure!(
                record.ordinal == ordinal as u64,
                "canonical record ordinal does not match its index"
            );
            records.push(record);
        }
        project_batch(&mut tx, thread, &records, next)?;
        tx.commit()?;
        if next >= target {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{canonical_thread_log::wait, ThreadLogStore};
    use wunder_core::thread_store::*;

    #[test]
    #[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
    fn postgres_views_page_complete_chat_and_rebuild_without_local_history() {
        let root = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(
            PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 4).unwrap(),
        );
        let store = storage.service_threads().unwrap();
        let thread = ThreadId::new(format!("fixture-{}", uuid::Uuid::new_v4().simple())).unwrap();
        let owner = "fixture-owner";
        let admissions = store.admission_store();
        admissions.register_owner(owner, thread.as_str()).unwrap();
        let rollout = RolloutId::new(uuid::Uuid::new_v4().to_string()).unwrap();
        wait(store.create_thread(CreateThreadRequest {
            metadata: ThreadMetadata {
                thread_id: thread.clone(),
                current_rollout_id: rollout.clone(),
                status: ThreadStatus::Active,
                created_at_ms: 1,
                updated_at_ms: 1,
                title: None,
                workspace_id: None,
                pinned: false,
            },
            session_meta: SessionMeta {
                thread_id: thread.clone(),
                rollout_id: rollout,
                created_at_ms: 1,
                source: "fixture".into(),
                initial_system_prompt: "fixture instructions".into(),
                initial_memory: None,
                parent_thread_id: None,
                history_base: None,
                capabilities: Value::Null,
            },
        }))
        .unwrap()
        .unwrap();
        let full_text = "fixture 中文🙂 ".repeat(9000);
        let mut last = String::new();
        for round in 1..=25 {
            let accepted = admissions
                .accept(
                    owner,
                    thread.as_str(),
                    &json!({"content":format!("fixture input {round}")}),
                )
                .unwrap();
            let turn = TurnId::new(accepted["turn_id"].as_str().unwrap()).unwrap();
            last = turn.to_string();
            let mut records = Vec::new();
            for action in 0..=105 {
                let user = action == 0;
                records.push(CanonicalRecord::Event(CanonicalEvent::ItemCompleted(
                    CanonicalItem {
                        item_id: ItemId::new(format!(
                            "{turn}:{}",
                            if user {
                                "user".into()
                            } else {
                                format!("text-{action}")
                            }
                        ))
                        .unwrap(),
                        turn_id: turn.clone(),
                        root_turn_id: Some(turn.clone()),
                        created_at_ms: round,
                        kind: if user {
                            CanonicalItemKind::UserMessage
                        } else {
                            CanonicalItemKind::AssistantMessage
                        },
                        payload: if user {
                            json!({"content":format!("fixture input {round}")})
                        } else if action == 105 {
                            json!({"content":full_text,"reasoning":"fixture reasoning"})
                        } else {
                            json!({"content":"fixture process","tool_calls":[{}]})
                        },
                        display_output: None,
                        display_output_truncated: false,
                    },
                )));
            }
            wait(store.append_canonical(&thread, records))
                .unwrap()
                .unwrap();
            admissions
                .update_with_control(
                    thread.as_str(),
                    turn.as_str(),
                    "completed",
                    true,
                    &Value::Null,
                )
                .unwrap();
        }
        let page = storage
            .thread_view_page(owner, thread.as_str(), "chat", None, 12)
            .unwrap();
        assert_eq!(page.session_id, thread.as_str());
        assert_eq!(page.latest_round, 25);
        assert_eq!(page.chat.len(), 12);
        assert_eq!(page.chat.last().unwrap().user_round, 25);
        assert_eq!(page.chat.last().unwrap().workflow_count, 105);
        assert_eq!(page.chat.last().unwrap().reasoning_count, 1);
        assert_eq!(page.chat.last().unwrap().user.content, "fixture input 25");
        assert!(page.has_more);
        assert!(serde_json::to_vec(&page).unwrap().len() < 128 * 1024);
        let older = storage
            .thread_view_page(owner, thread.as_str(), "chat", page.next_before, 12)
            .unwrap();
        let forward = storage
            .thread_view_window(
                owner,
                thread.as_str(),
                "chat",
                None,
                older.next_after,
                None,
                12,
            )
            .unwrap();
        assert_eq!(
            page.chat.iter().map(|r| r.user_round).collect::<Vec<_>>(),
            forward
                .chat
                .iter()
                .map(|r| r.user_round)
                .collect::<Vec<_>>()
        );
        let body = &page
            .chat
            .last()
            .unwrap()
            .assistant
            .body
            .as_ref()
            .unwrap()
            .body_id;
        let mut cursor = 0;
        let mut text = String::new();
        loop {
            let chunk = storage
                .thread_body_page(owner, thread.as_str(), body, 1, cursor)
                .unwrap();
            text.push_str(&chunk.chunks.concat());
            if !chunk.has_more {
                break;
            }
            assert!(chunk.next_cursor.unwrap() > cursor);
            cursor = chunk.next_cursor.unwrap();
        }
        assert_eq!(text, full_text);
        assert!(storage
            .thread_body_page(owner, thread.as_str(), body, 1, -1)
            .is_err());
        assert!(storage
            .thread_body_page("fixture-other", thread.as_str(), body, 1, 0)
            .is_err());
        let overview = storage
            .thread_view_page(owner, thread.as_str(), "trajectory", None, 10)
            .unwrap();
        assert_eq!(overview.trajectory.len(), 10);
        assert!(overview.trajectory.iter().all(|r| r.overview_only));
        let detail = storage
            .thread_view_trajectory_page(owner, thread.as_str(), None, Some(&last), 20)
            .unwrap();
        let previous = storage
            .thread_view_trajectory_page(
                owner,
                thread.as_str(),
                detail.next_before,
                Some(&last),
                20,
            )
            .unwrap();
        assert!(detail.has_more && previous.has_more);
        assert!(previous.next_after.unwrap() < detail.next_before.unwrap());
        let final_assistant = &page.chat.last().unwrap().assistant.item_id;
        conn_feedback(&storage, thread.as_str(), final_assistant);
        let feedback_page = storage
            .thread_view_page(owner, thread.as_str(), "chat", None, 1)
            .unwrap();
        assert_eq!(
            feedback_page.chat[0].assistant.feedback.as_ref().unwrap()["vote"],
            "up"
        );
        let durable = storage
            .get_thread_turn_page(owner, thread.as_str(), &last, None, None, 20, false)
            .unwrap()
            .unwrap();
        assert_eq!(durable["items"].as_array().unwrap().len(), 20);
        assert_eq!(durable["has_more"], true);
        let search = storage
            .thread_view_search(owner, thread.as_str(), "中文")
            .unwrap();
        assert!(!search.hits.is_empty());
        // Size metadata selects the contiguous prefix before bodies are read.
        let budget = REBUILD_BATCH_BYTES as i32;
        let max = crate::storage::rollout::ROLLOUT_MAX_RECORD_BYTES as i32;
        assert_eq!(
            rebuild_batch_end(0, 3, [(0, budget - 1), (1, 2), (2, 1)]).unwrap(),
            1
        );
        assert_eq!(rebuild_batch_end(0, 2, [(0, max), (1, 1)]).unwrap(), 1);
        assert_eq!(
            rebuild_batch_end(0, 2, [(0, budget - 1), (1, 1)]).unwrap(),
            2
        );
        assert!(rebuild_batch_end(0, 1, [(0, max + 1)]).is_err());
        assert!(rebuild_batch_end(0, 1, []).is_err());
        assert!(rebuild_batch_end(0, 3, [(0, 1), (2, 1)]).is_err());
        // Lose a rebuildable index, then resume projection and replay it twice.
        let mut conn = storage.conn().unwrap();
        conn.execute(
            "DELETE FROM service_thread_items WHERE thread_id=$1",
            &[&thread.as_str()],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM service_thread_chat_heads WHERE thread_id=$1",
            &[&thread.as_str()],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM service_thread_display_state WHERE thread_id=$1",
            &[&thread.as_str()],
        )
        .unwrap();
        let restored = storage
            .thread_view_page(owner, thread.as_str(), "chat", None, 12)
            .unwrap();
        let repeated = storage
            .thread_view_page(owner, thread.as_str(), "chat", None, 12)
            .unwrap();
        assert_eq!(restored.chat.last().unwrap().workflow_count, 105);
        assert_eq!(restored.view_seq, page.view_seq);
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(repeated).unwrap()
        );
        for filename in [
            "state_1.sqlite",
            "queue_1.sqlite",
            "thread_history_1.sqlite",
            "logs_1.sqlite",
        ] {
            assert!(!root.path().join(filename).exists());
        }
        assert!(!root.path().join("sessions").exists());
        let control_turn = TurnId::new(last.clone()).unwrap();
        let compaction_id = format!("{last}:compaction");
        let system_id = format!("{last}:internal-system");
        let system_item = |id: &str, payload: Value| {
            CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
                item_id: ItemId::new(id).unwrap(),
                turn_id: control_turn.clone(),
                root_turn_id: Some(control_turn.clone()),
                created_at_ms: 26,
                kind: CanonicalItemKind::Configuration,
                payload,
                display_output: None,
                display_output_truncated: false,
            }))
        };
        wait(store.append_canonical(&thread, vec![
            system_item(&compaction_id, json!({"kind":"system_message","content":"sample compaction summary","model_content":"sample internal model context","meta":{"type":"compaction_summary","replacement_history":[{"content":"sample retained model context"}]}})),
            system_item(&system_id, json!({"kind":"system_message","content":"sample frozen instructions"})),
        ])).unwrap().unwrap();
        let compaction = storage
            .thread_view_trajectory_page(owner, thread.as_str(), None, Some(&last), 10)
            .unwrap();
        assert!(compaction
            .trajectory
            .iter()
            .any(|item| item.item_id == compaction_id));
        assert!(compaction
            .trajectory
            .iter()
            .all(|item| item.item_id != system_id));
        let detail = storage
            .thread_body_page(
                owner,
                thread.as_str(),
                &format!("{compaction_id}|detail"),
                1,
                0,
            )
            .unwrap()
            .chunks
            .concat();
        assert!(detail.contains("sample compaction summary"));
        assert!(!detail.contains("sample internal model context"));
        assert!(!detail.contains("sample retained model context"));
        // Corruption must leave the watermark at its last valid boundary;
        // repairing the canonical row allows the same projection to resume.
        let revision = storage
            .latest_thread_change_seq_by_session(thread.as_str())
            .unwrap();
        let broken_ordinal = revision - 1;
        let original: String = conn
            .query_one(
                "SELECT record_json FROM service_thread_records WHERE thread_id=$1 AND ordinal=$2",
                &[&thread.as_str(), &broken_ordinal],
            )
            .unwrap()
            .get(0);
        conn.execute(
            "UPDATE service_thread_display_state SET next_ordinal=$2 WHERE thread_id=$1",
            &[&thread.as_str(), &broken_ordinal],
        )
        .unwrap();
        conn.execute("UPDATE service_thread_records SET record_json=repeat('x',$3::integer) WHERE thread_id=$1 AND ordinal=$2", &[&thread.as_str(), &broken_ordinal, &(max + 1)]).unwrap();
        assert!(ensure_ready(&storage, thread.as_str()).is_err());
        assert_eq!(
            conn.query_one(
                "SELECT next_ordinal FROM service_thread_display_state WHERE thread_id=$1",
                &[&thread.as_str()]
            )
            .unwrap()
            .get::<_, i64>(0),
            broken_ordinal
        );
        conn.execute(
            "DELETE FROM service_thread_records WHERE thread_id=$1 AND ordinal=$2",
            &[&thread.as_str(), &broken_ordinal],
        )
        .unwrap();
        assert!(ensure_ready(&storage, thread.as_str()).is_err());
        assert_eq!(
            conn.query_one(
                "SELECT next_ordinal FROM service_thread_display_state WHERE thread_id=$1",
                &[&thread.as_str()]
            )
            .unwrap()
            .get::<_, i64>(0),
            broken_ordinal
        );
        conn.execute("INSERT INTO service_thread_records(thread_id,ordinal,timestamp_ms,record_json,record_type,turn_id,item_id) VALUES($1,$2,26,$3,'item_completed',$4,$5)", &[&thread.as_str(), &broken_ordinal, &original, &last, &system_id]).unwrap();
        ensure_ready(&storage, thread.as_str()).unwrap();
        assert_eq!(
            conn.query_one(
                "SELECT next_ordinal FROM service_thread_display_state WHERE thread_id=$1",
                &[&thread.as_str()]
            )
            .unwrap()
            .get::<_, i64>(0),
            revision
        );
        wait(store.delete_thread(&thread)).unwrap().unwrap();
        conn.execute(
            "DELETE FROM meta WHERE key=$1",
            &[&format!("thread_feedback:{thread}:{final_assistant}")],
        )
        .unwrap();
        assert_eq!(
            conn.query_one(
                "SELECT count(*) FROM service_thread_items WHERE thread_id=$1",
                &[&thread.as_str()]
            )
            .unwrap()
            .get::<_, i64>(0),
            0
        );
    }

    fn conn_feedback(storage: &PostgresStorage, session: &str, item: &str) {
        storage.conn().unwrap().execute(
            "INSERT INTO meta(key,value,updated_time) VALUES($1,'up',0) ON CONFLICT(key) DO UPDATE SET value='up'",
            &[&format!("thread_feedback:{session}:{item}")],
        ).unwrap();
    }
}
