//! Rebuildable chat checkpoints. Reads select one small checkpoint per root
//! and lineage segment, independently of the number or size of model actions.
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ChatSummary {
    pub user_item_id: Option<String>,
    pub assistant_item_id: Option<String>,
    /// Latest durable question-panel event for this user turn.  The panel is
    /// a visible control item rather than assistant prose, so it needs its
    /// own identity in the compact chat checkpoint.
    #[serde(default)]
    pub question_panel_item_id: Option<String>,
    pub plan_item_id: Option<String>,
    pub reasoning_item_id: Option<String>,
    pub reasoning_count: i64,
    pub workflow_count: i64,
    pub workspace_change_seq: i64,
}

pub(super) fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS thread_chat_heads (
            rollout_id TEXT NOT NULL, root_turn_id TEXT NOT NULL,
            rollout_ordinal INTEGER NOT NULL, summary_json TEXT NOT NULL,
            PRIMARY KEY(rollout_id,root_turn_id)
         );
         CREATE TABLE IF NOT EXISTS thread_chat_checkpoints (
            rollout_id TEXT NOT NULL, root_turn_id TEXT NOT NULL,
            rollout_ordinal INTEGER NOT NULL, summary_json TEXT NOT NULL,
            PRIMARY KEY(rollout_id,root_turn_id,rollout_ordinal)
         );
         CREATE TABLE IF NOT EXISTS thread_chat_projection_state (
            rollout_id TEXT PRIMARY KEY NOT NULL, next_ordinal INTEGER NOT NULL
         );",
    )?;
    Ok(())
}

pub(super) fn save_position(conn: &Connection, rollout: &str, next: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO thread_chat_projection_state(rollout_id,next_ordinal) VALUES(?1,?2)
         ON CONFLICT(rollout_id) DO UPDATE SET next_ordinal=excluded.next_ordinal",
        params![rollout, next],
    )?;
    Ok(())
}

pub(super) fn project_item(
    tx: &Transaction<'_>,
    rollout: &str,
    root: &str,
    turn: &str,
    id: &str,
    kind: &str,
    ordinal: i64,
    visible: bool,
    preview: &Value,
) -> Result<()> {
    let previous: Option<String> = tx
        .query_row(
            "SELECT summary_json FROM thread_chat_heads WHERE rollout_id=?1 AND root_turn_id=?2",
            params![rollout, root],
            |row| row.get(0),
        )
        .optional()?;
    let mut summary: ChatSummary = previous
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();
    apply_item(&mut summary, turn, id, kind, ordinal, visible, preview);
    let encoded = serde_json::to_string(&summary)?;
    if previous.as_deref() == Some(&encoded) {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO thread_chat_checkpoints(rollout_id,root_turn_id,rollout_ordinal,summary_json)
         VALUES(?1,?2,?3,?4)",
        params![rollout, root, ordinal, encoded],
    )?;
    tx.execute(
        "INSERT INTO thread_chat_heads(rollout_id,root_turn_id,rollout_ordinal,summary_json)
         VALUES(?1,?2,?3,?4) ON CONFLICT(rollout_id,root_turn_id) DO UPDATE SET
         rollout_ordinal=excluded.rollout_ordinal,summary_json=excluded.summary_json",
        params![rollout, root, ordinal, encoded],
    )?;
    Ok(())
}

/// Shared compact projection semantics for local and service history.
pub(super) fn apply_item(
    summary: &mut ChatSummary,
    turn: &str,
    id: &str,
    kind: &str,
    ordinal: i64,
    visible: bool,
    preview: &Value,
) {
    let payload = preview.get("payload").unwrap_or(preview);
    let body = if payload.get("item_id").and_then(Value::as_str).is_some()
        && payload.get("payload").is_some()
    {
        payload
            .get("payload")
            .filter(|value| value.is_object())
            .unwrap_or(payload)
    } else {
        payload
    };
    if visible {
        summary.workflow_count += i64::from(kind != "user_message");
        match kind {
            "user_message" => summary.user_item_id = Some(id.into()),
            "assistant_message" => {
                if !super::thread_view_projection::has_tool_calls(body)
                    && !super::thread_view_projection::has_tool_calls(payload)
                {
                    summary.assistant_item_id = Some(id.into());
                }
                let reasoning = body
                    .get("reasoning")
                    .or_else(|| body.get("reasoning_content"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if super::thread_view_projection::stable_answer(id, turn, kind)
                    && !reasoning.trim().is_empty()
                {
                    summary.reasoning_count += 1;
                    summary.reasoning_item_id = Some(id.into());
                }
            }
            "question_panel" => summary.question_panel_item_id = Some(id.into()),
            "plan" => summary.plan_item_id = Some(id.into()),
            _ => {}
        }
        // Track the actual panel carrier independently of assistant prose and
        // plans. Both local and service projections consume this same head.
        if super::thread_view_projection::extract_question_panel(body).is_some() {
            summary.question_panel_item_id = Some(id.into());
        }
    }
    // Workspace invalidation has always included private completed items.
    if [
        payload.pointer("/meta/workspace_changed"),
        payload.pointer("/payload/meta/workspace_changed"),
    ]
    .into_iter()
    .flatten()
    .any(|value| value == true || value == 1)
    {
        summary.workspace_change_seq = ordinal + 1;
    }
}

/// Upgrade only the accessed rollout, with bounded transactions and resumable
/// watermarks. No startup scan or new canonical writes are involved.
pub(super) fn ensure_ready(conn: &mut Connection, rollout: &str) -> Result<()> {
    let (through, position): (i64,i64) = conn.query_row(
        "SELECT p.next_rollout_ordinal,COALESCE(c.next_ordinal,0) FROM thread_history_projection_state p
         LEFT JOIN thread_chat_projection_state c ON c.rollout_id=p.rollout_id WHERE p.rollout_id=?1",
        [rollout], |row| Ok((row.get(0)?,row.get(1)?)),
    ).optional()?.unwrap_or_default();
    if position >= through {
        return Ok(());
    }
    // Fix this upgrade's target before taking write locks. Concurrent writers
    // maintain their suffix themselves; a reader must not chase them forever.
    loop {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let position: i64 = tx
            .query_row(
                "SELECT next_ordinal FROM thread_chat_projection_state WHERE rollout_id=?1",
                [rollout],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if position >= through {
            return Ok(());
        }
        let mut statement = tx.prepare(
            "SELECT root_turn_id,turn_id,item_id,item_type,rollout_ordinal,visible,preview_json
             FROM thread_items WHERE rollout_id=?1 AND rollout_ordinal>=?2 AND rollout_ordinal<?3
             ORDER BY rollout_ordinal LIMIT 128",
        )?;
        let mut rows = statement.query(params![rollout, position, through])?;
        let mut batch = Vec::new();
        let mut bytes = 0;
        while let Some(row) = rows.next()? {
            let raw: String = row.get(6)?;
            bytes += raw.len();
            batch.push((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, bool>(5)?,
                raw,
            ));
            if bytes >= 2 * 1024 * 1024 {
                break;
            }
        }
        drop(rows);
        drop(statement);
        let next = batch.last().map(|row| row.4 + 1).unwrap_or(through);
        for (root, turn, id, kind, ordinal, visible, raw) in batch {
            if let Some(turn) = turn {
                project_item(
                    &tx,
                    rollout,
                    root.as_deref().unwrap_or(&turn),
                    &turn,
                    &id,
                    &kind,
                    ordinal,
                    visible,
                    &serde_json::from_str(&raw)?,
                )?;
            }
        }
        save_position(&tx, rollout, next)?;
        tx.commit()?;
        if next == through {
            return Ok(());
        }
    }
}

pub(super) fn read(conn: &Connection, rollout: &str, root: &str) -> Result<ChatSummary> {
    // Scoped views return at most one checkpoint per lineage segment. Global
    // ordinals order those segments; each checkpoint counts only its own file.
    let mut statement = conn.prepare(
        "SELECT summary_json FROM thread_chat_heads WHERE rollout_id=?1 AND root_turn_id=?2 ORDER BY rollout_ordinal",
    )?;
    let rows = statement.query_map(params![rollout, root], |row| row.get::<_, String>(0))?;
    let mut result = ChatSummary::default();
    for row in rows {
        let current: ChatSummary = serde_json::from_str(&row?)?;
        if current.user_item_id.is_some() {
            result.user_item_id = current.user_item_id;
        }
        if current.assistant_item_id.is_some() {
            result.assistant_item_id = current.assistant_item_id;
        }
        if current.question_panel_item_id.is_some() {
            result.question_panel_item_id = current.question_panel_item_id;
        }
        if current.plan_item_id.is_some() {
            result.plan_item_id = current.plan_item_id;
        }
        if current.reasoning_item_id.is_some() {
            result.reasoning_item_id = current.reasoning_item_id;
        }
        result.reasoning_count += current.reasoning_count;
        result.workflow_count += current.workflow_count;
        result.workspace_change_seq = result
            .workspace_change_seq
            .max(current.workspace_change_seq);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::thread_history::{HistorySegment, ThreadHistoryStore};
    use serde_json::json;

    fn record(ordinal: u64, turn: &str, id: &str, kind: &str, payload: Value) -> String {
        json!({"ordinal":ordinal,"type":"item_completed","payload":{
        "turn_id":turn,"root_turn_id":"fixture-root","item":{
            "item_id":id,"turn_id":turn,"root_turn_id":"fixture-root",
            "kind":kind,"created_at_ms":1,"payload":payload
        }}})
        .to_string()
            + "\n"
    }

    #[test]
    fn inquiry_carrier_and_plan_keep_independent_checkpoint_identities() {
        let mut summary = ChatSummary::default();
        let panel = json!({"kind":"tool_call","status":"completed","result":{"data":{
            "action":"question_panel","state":"awaiting_input",
            "data":{"questions":[{"question":"Fixture choice?","options":[{"label":"Option A"}]}]}
        }}});
        apply_item(
            &mut summary,
            "fixture-turn",
            "fixture-carrier",
            "tool_call",
            0,
            true,
            &panel,
        );
        apply_item(
            &mut summary,
            "fixture-turn",
            "fixture-plan",
            "plan",
            1,
            true,
            &json!({"plan":[]}),
        );
        apply_item(
            &mut summary,
            "fixture-turn",
            "fixture-text",
            "assistant_message",
            2,
            true,
            &json!({"content":"Fixture waiting"}),
        );
        apply_item(
            &mut summary,
            "fixture-turn",
            "fixture-hidden",
            "question_panel",
            3,
            false,
            &panel,
        );
        assert_eq!(
            summary.question_panel_item_id.as_deref(),
            Some("fixture-carrier")
        );
        assert_eq!(summary.plan_item_id.as_deref(), Some("fixture-plan"));
        assert_eq!(summary.assistant_item_id.as_deref(), Some("fixture-text"));
    }

    #[test]
    fn summary_is_incremental_idempotent_and_respects_visibility() {
        let directory = tempfile::tempdir().unwrap();
        let store = ThreadHistoryStore::open(directory.path().join("history.sqlite")).unwrap();
        let mut bytes = record(
            0,
            "fixture-turn",
            "fixture-user",
            "user_message",
            json!({"content":"input"}),
        );
        bytes += &record(
            1,
            "fixture-turn",
            "fixture-turn:text-1",
            "assistant_message",
            json!({"content":"work","reasoning_content":"thought","tool_calls":[{}]}),
        );
        bytes += &record(
            2,
            "fixture-turn",
            "fixture-turn:text-2",
            "assistant_message",
            json!({"content":"answer","reasoning_content":"thought"}),
        );
        bytes += &record(
            3,
            "fixture-turn",
            "fixture-hidden",
            "assistant_message",
            json!({"content":"private","visibility":"internal","meta":{"workspace_changed":true}}),
        );
        bytes += &record(
            4,
            "fixture-turn",
            "fixture-import",
            "assistant_message",
            json!({"content":"imported answer","reasoning_content":"unbound thought"}),
        );
        store
            .project_jsonl_bytes("fixture-rollout", bytes.as_bytes())
            .unwrap();
        let conn = Connection::open(store.db_path()).unwrap();
        let summary = read(&conn, "fixture-rollout", "fixture-root").unwrap();
        assert_eq!(summary.user_item_id.as_deref(), Some("fixture-user"));
        assert_eq!(summary.assistant_item_id.as_deref(), Some("fixture-import"));
        assert_eq!(
            summary.reasoning_item_id.as_deref(),
            Some("fixture-turn:text-2")
        );
        assert_eq!(
            (
                summary.reasoning_count,
                summary.workflow_count,
                summary.workspace_change_seq
            ),
            (2, 3, 4)
        );
        // Repeated projection cannot add counts or move the summary waterline.
        store
            .project_jsonl_bytes("fixture-rollout", bytes.as_bytes())
            .unwrap();
        assert_eq!(
            read(&conn, "fixture-rollout", "fixture-root").unwrap(),
            summary
        );
        // Same immutable item committed again at a later ordinal is idempotent.
        let mut duplicate: Value = serde_json::from_str(bytes.lines().nth(2).unwrap()).unwrap();
        duplicate["ordinal"] = json!(5);
        bytes += &(duplicate.to_string() + "\n");
        store
            .project_jsonl_bytes("fixture-rollout", bytes.as_bytes())
            .unwrap();
        assert_eq!(
            read(&conn, "fixture-rollout", "fixture-root").unwrap(),
            summary
        );
    }

    #[test]
    fn branch_cutoff_uses_checkpoint_before_later_source_continuations() {
        let directory = tempfile::tempdir().unwrap();
        let store = ThreadHistoryStore::open(directory.path().join("history.sqlite")).unwrap();
        let mut bytes = record(
            0,
            "fixture-turn",
            "fixture-user",
            "user_message",
            json!({"content":"input"}),
        );
        bytes += &record(
            1,
            "fixture-turn",
            "fixture-turn:text-1",
            "assistant_message",
            json!({"content":"retained","reasoning_content":"thought"}),
        );
        bytes += &record(
            2,
            "fixture-turn",
            "fixture-tool",
            "tool_result",
            json!({"content":"result","meta":{"workspace_changed":true}}),
        );
        store
            .project_jsonl_bytes("fixture-source", bytes.as_bytes())
            .unwrap();
        // A later source continuation changes the very same root; the branch
        // must still resolve its earlier reply, counts and workspace sequence.
        bytes += &record(
            3,
            "fixture-later",
            "fixture-later:text-1",
            "assistant_message",
            json!({"content":"excluded","reasoning_content":"thought"}),
        );
        bytes += &record(
            4,
            "fixture-later",
            "fixture-later-tool",
            "tool_result",
            json!({"meta":{"workspace_changed":true}}),
        );
        store
            .project_jsonl_bytes("fixture-source", bytes.as_bytes())
            .unwrap();
        let conn = Connection::open(store.db_path()).unwrap();
        ThreadHistoryStore::apply_scope(
            &conn,
            "fixture-branch",
            &[
                HistorySegment {
                    rollout_id: "fixture-source".into(),
                    start_ordinal: 0,
                    end_ordinal: Some(3),
                },
                HistorySegment {
                    rollout_id: "fixture-branch".into(),
                    start_ordinal: 3,
                    end_ordinal: None,
                },
            ],
        )
        .unwrap();
        let inherited = read(&conn, "fixture-branch", "fixture-root").unwrap();
        assert_eq!(
            inherited.assistant_item_id.as_deref(),
            Some("fixture-turn:text-1")
        );
        assert_eq!(
            (
                inherited.reasoning_count,
                inherited.workflow_count,
                inherited.workspace_change_seq
            ),
            (1, 2, 3)
        );
        drop(conn);
        let suffix = record(
            3,
            "fixture-child",
            "fixture-child:text-1",
            "assistant_message",
            json!({"content":"branch answer","reasoning_content":"thought"}),
        );
        store
            .project_jsonl_bytes("fixture-branch", suffix.as_bytes())
            .unwrap();
        let conn = Connection::open(store.db_path()).unwrap();
        ThreadHistoryStore::apply_scope(
            &conn,
            "fixture-branch",
            &[
                HistorySegment {
                    rollout_id: "fixture-source".into(),
                    start_ordinal: 0,
                    end_ordinal: Some(3),
                },
                HistorySegment {
                    rollout_id: "fixture-branch".into(),
                    start_ordinal: 3,
                    end_ordinal: None,
                },
            ],
        )
        .unwrap();
        let current = read(&conn, "fixture-branch", "fixture-root").unwrap();
        assert_eq!(
            current.assistant_item_id.as_deref(),
            Some("fixture-child:text-1")
        );
        assert_eq!(
            (
                current.reasoning_count,
                current.workflow_count,
                current.workspace_change_seq
            ),
            (2, 3, 3)
        );
    }

    #[test]
    fn old_projection_backfill_resumes_and_deletion_rebuilds_all_summaries() {
        let directory = tempfile::tempdir().unwrap();
        let store = ThreadHistoryStore::open(directory.path().join("history.sqlite")).unwrap();
        let mut bytes = String::new();
        for ordinal in 0..260 {
            bytes += &record(
                ordinal,
                "fixture-turn",
                &format!("fixture-turn:text-{ordinal}"),
                "assistant_message",
                json!({"content":"answer","reasoning_content":"thought"}),
            );
        }
        store
            .project_jsonl_bytes("fixture-rollout", bytes.as_bytes())
            .unwrap();
        let mut conn = Connection::open(store.db_path()).unwrap();
        let expected = read(&conn, "fixture-rollout", "fixture-root").unwrap();
        // Simulate a resumable old-data upgrade interrupted after checkpoint 127.
        let checkpoint: String = conn
            .query_row(
                "SELECT summary_json FROM thread_chat_checkpoints
            WHERE rollout_id='fixture-rollout' AND rollout_ordinal=127",
                [],
                |row| row.get(0),
            )
            .unwrap();
        conn.execute(
            "DELETE FROM thread_chat_checkpoints WHERE rollout_ordinal>=128",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE thread_chat_heads SET rollout_ordinal=127,summary_json=?1",
            [checkpoint],
        )
        .unwrap();
        save_position(&conn, "fixture-rollout", 128).unwrap();
        ensure_ready(&mut conn, "fixture-rollout").unwrap();
        ensure_ready(&mut conn, "fixture-rollout").unwrap();
        assert_eq!(
            read(&conn, "fixture-rollout", "fixture-root").unwrap(),
            expected
        );
        // Old databases have no summaries at all. Populate only this rollout,
        // and prove that the upgrade leaves the log projection unchanged.
        conn.execute_batch(
            "DELETE FROM thread_chat_heads; DELETE FROM thread_chat_checkpoints;
            DELETE FROM thread_chat_projection_state;",
        )
        .unwrap();
        let before: i64 = conn
            .query_row(
                "SELECT next_rollout_ordinal FROM thread_history_projection_state",
                [],
                |row| row.get(0),
            )
            .unwrap();
        ensure_ready(&mut conn, "fixture-rollout").unwrap();
        assert_eq!(
            read(&conn, "fixture-rollout", "fixture-root").unwrap(),
            expected
        );
        let after: i64 = conn
            .query_row(
                "SELECT next_rollout_ordinal FROM thread_history_projection_state",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
        store.delete_rollout("fixture-rollout").unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM thread_chat_heads", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        store
            .project_jsonl_bytes("fixture-rollout", bytes.as_bytes())
            .unwrap();
        assert_eq!(
            read(&conn, "fixture-rollout", "fixture-root").unwrap(),
            expected
        );
    }
}
