use super::SqliteStorage;
use crate::storage::StorageLifecycle;
use anyhow::{ensure, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use uuid::Uuid;
pub(super) trait SqliteThreadLogStorage {
    fn load_subagent_context_impl(
        &self,
        user_id: &str,
        session_id: &str,
        turns: i64,
    ) -> Result<Vec<Value>>;
    fn fork_thread_log_impl(
        &self,
        user_id: &str,
        source: &str,
        target: &str,
        through_round: i64,
    ) -> Result<()>;
    fn list_thread_visible_messages_impl(
        &self,
        user_id: &str,
        session_id: &str,
        before_seq: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn load_thread_context_items_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
        include_internal: bool,
        executing_turn: Option<&str>,
    ) -> Result<Vec<Value>>;
    fn upsert_thread_text_block_impl(
        &self,
        user_id: &str,
        session_id: &str,
        block: &Value,
    ) -> Result<i64>;
    fn list_thread_text_blocks_impl(
        &self,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn list_thread_item_blocks_impl(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        from_block: i64,
        limit: i64,
        include_internal: bool,
    ) -> Result<Vec<Value>>;
    fn list_thread_item_blocks_page_impl(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        field: Option<&str>,
        from_block: i64,
        limit: i64,
        include_internal: bool,
    ) -> Result<(Vec<Value>, Option<i64>, bool)>;

    fn find_thread_turn_id_impl(
        &self,
        user_id: &str,
        session_id: &str,
        user_turn_index: i64,
    ) -> Result<Option<String>>;
    fn accept_thread_turn_impl(
        &self,
        user_id: &str,
        session_id: &str,
        input: &Value,
    ) -> Result<Value>;
    fn update_thread_turn_impl(
        &self,
        user_id: &str,
        session_id: &str,
        turn_id: &str,
        status: &str,
        summary: &str,
        payload: &Value,
    ) -> Result<bool>;
    fn delete_thread_log_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn append_thread_item_impl(&self, user_id: &str, payload: &Value) -> Result<Option<Value>>;
    fn list_thread_turns_impl(
        &self,
        user_id: &str,
        session_id: &str,
        before: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn get_thread_log_counts_impl(
        &self,
        user_id: &str,
        session_id: &str,
        include_internal: bool,
    ) -> Result<(i64, i64)>;
    fn latest_thread_user_round_by_session_impl(&self, session_id: &str) -> Result<i64>;
    fn get_thread_turn_impl(
        &self,
        user_id: &str,
        session_id: &str,
        turn_id: &str,
        after: i64,
        limit: i64,
        include_internal: bool,
    ) -> Result<Option<Value>>;
    fn get_thread_item_impl(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        include_internal: bool,
    ) -> Result<Option<Value>>;
    fn set_thread_item_feedback_impl(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        vote: &str,
    ) -> Result<Option<Value>>;
    fn list_thread_changes_by_session_impl(
        &self,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
    fn latest_thread_change_seq_by_session_impl(&self, session_id: &str) -> Result<i64>;
    fn thread_snapshot_impl(&self, user_id: &str, session_id: &str) -> Result<Value>;
    fn list_thread_changes_impl(
        &self,
        user_id: &str,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
}
impl SqliteThreadLogStorage for SqliteStorage {
    fn load_subagent_context_impl(
        &self,
        user_id: &str,
        session_id: &str,
        turns: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT i.root_turn_id, i.kind, substr(json_extract(i.payload,'$.content'),1,16384), length(json_extract(i.payload,'$.content')) FROM thread_items i WHERE i.user_id=?1 AND i.session_id=?2 AND i.visibility='user' AND i.kind IN ('user_message','assistant_message') AND i.root_turn_id IN (SELECT root_turn_id FROM thread_turns WHERE user_id=?1 AND session_id=?2 AND trigger_kind='user' ORDER BY user_turn_index DESC LIMIT ?3) ORDER BY i.created_seq DESC LIMIT 257")?;
        let mut rows = stmt.query_map(params![user_id, session_id, turns.clamp(0,16)], |row| {
            let kind: String = row.get(1)?;
            let content: Option<String> = row.get(2)?;
            let length: Option<i64> = row.get(3)?;
            Ok(json!({"root_turn_id":row.get::<_,String>(0)?, "role":if kind=="user_message" {"user"} else {"assistant"}, "content":content.unwrap_or_default(), "truncated":length.unwrap_or(0)>16384}))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    fn fork_thread_log_impl(
        &self,
        user_id: &str,
        source: &str,
        target: &str,
        through_round: i64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        ensure!(
            !target.trim().is_empty() && source != target && through_round > 0,
            "invalid thread fork"
        );
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT user_id FROM thread_logs WHERE session_id=?",
                params![source],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(owner.as_deref() == Some(user_id), "thread owner mismatch");
        // The root INSERT rejects an existing destination, so retries cannot merge graphs.
        tx.execute("INSERT INTO thread_logs(session_id,user_id,latest_user_turn,latest_change_seq,created_time,updated_time) SELECT ?3,user_id,COALESCE((SELECT MAX(user_turn_index) FROM thread_turns WHERE session_id=?2 AND trigger_kind='user' AND user_turn_index<=?4),0),latest_change_seq,created_time,updated_time FROM thread_logs WHERE user_id=?1 AND session_id=?2", params![user_id,source,target,through_round])?;
        tx.execute("INSERT INTO thread_turns(session_id,turn_id,user_id,root_turn_id,trigger_kind,client_message_id,user_turn_index,status,summary,payload,created_time,updated_time) SELECT ?3,turn_id,user_id,root_turn_id,trigger_kind,client_message_id,user_turn_index,status,summary,json_set(payload, '$.session_id', ?3),created_time,updated_time FROM thread_turns WHERE user_id=?1 AND session_id=?2 AND user_turn_index<=?4", params![user_id,source,target,through_round])?;
        tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,revision,payload,created_time,updated_time,created_seq) SELECT ?3,i.item_id,i.turn_id,i.root_turn_id,i.visibility,i.user_id,i.item_index,i.kind,i.status,i.revision,json_set(i.payload, '$.session_id', ?3),i.created_time,i.updated_time,i.created_seq FROM thread_items i JOIN thread_turns t ON t.session_id=i.session_id AND t.turn_id=i.turn_id WHERE i.user_id=?1 AND i.session_id=?2 AND t.user_turn_index<=?4", params![user_id,source,target,through_round])?;
        tx.execute("INSERT INTO thread_item_blocks(session_id,user_id,item_id,field,block_index,event_id,payload) SELECT ?3,b.user_id,b.item_id,b.field,b.block_index,b.event_id,json_set(b.payload, '$.session_id', ?3) FROM thread_item_blocks b JOIN thread_items i ON i.session_id=?3 AND i.item_id=b.item_id WHERE b.user_id=?1 AND b.session_id=?2", params![user_id,source,target])?;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) SELECT ?3,c.change_seq,c.user_id,c.change_type,c.turn_id,c.item_id,c.revision,c.payload,c.created_time FROM thread_log_changes c JOIN thread_turns t ON t.session_id=?3 AND t.turn_id=c.turn_id WHERE c.user_id=?1 AND c.session_id=?2", params![user_id,source,target])?;
        tx.execute("INSERT INTO thread_log_metrics(session_id,user_id,metric_key,metric_value,updated_time) SELECT session_id,user_id,'user_turn_total',latest_user_turn,updated_time FROM thread_logs WHERE user_id=?1 AND session_id=?3 AND ?2<>?3", params![user_id,source,target])?;
        tx.commit()?;
        Ok(())
    }
    fn list_thread_visible_messages_impl(
        &self,
        user_id: &str,
        session_id: &str,
        before_seq: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let before = before_seq.unwrap_or(i64::MAX);
        let mut stmt = conn.prepare("SELECT i.payload,i.created_seq,i.kind,t.status FROM thread_items i JOIN thread_turns t ON t.session_id=i.session_id AND t.turn_id=i.turn_id WHERE i.user_id=? AND i.session_id=? AND i.visibility='user' AND i.kind IN ('user_message','assistant_message') AND i.created_seq<? ORDER BY i.created_seq DESC LIMIT ?")?;
        let rows = stmt
            .query_map(
                params![user_id, session_id, before, limit.clamp(1, 501)],
                |row| {
                    let text: String = row.get(0)?;
                    let seq: i64 = row.get(1)?;
                    let mut value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    if let Value::Object(map) = &mut value {
                        map.insert("created_seq".into(), json!(seq));
                    }
                    crate::services::thread_log::project_visible_item(
                        &mut value,
                        &row.get::<_, String>(2)?,
                        &row.get::<_, String>(3)?,
                    );
                    Ok(value)
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    fn latest_thread_change_seq_by_session_impl(&self, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        Ok(conn
            .query_row(
                "SELECT COALESCE(latest_change_seq,0) FROM thread_logs WHERE session_id=?",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }
    fn load_thread_context_items_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
        include_internal: bool,
        executing_turn: Option<&str>,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT i.payload FROM thread_items i JOIN thread_turns t ON t.session_id=i.session_id AND t.turn_id=i.turn_id WHERE i.user_id=? AND i.session_id=? AND (? OR i.visibility='user') AND (? IS NULL OR (t.status<>'queued' AND i.item_id<>? || ':user')) AND i.kind IN ('user_message','assistant_message','tool_message','system_message') ORDER BY i.created_seq DESC LIMIT ?")?;
        let rows = stmt
            .query_map(
                params![
                    user_id,
                    session_id,
                    include_internal,
                    executing_turn,
                    executing_turn,
                    if limit > 0 { limit } else { -1 }
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Decode failures must surface instead of silently dropping model messages.
        rows.into_iter()
            .rev()
            .map(|text| serde_json::from_str(&text).map_err(Into::into))
            .collect()
    }
    fn upsert_thread_text_block_impl(
        &self,
        user_id: &str,
        session_id: &str,
        block: &Value,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let mut conn = self.open()?;
        let conn = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let item_id = block["item_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing text item"))?;
        let index = block["block_index"].as_i64().unwrap_or(0);
        let field = block["field"]
            .as_str()
            .or_else(|| block.pointer("/data/field").and_then(Value::as_str))
            .unwrap_or("content");
        let event_id = block["event_id"].as_i64().unwrap_or(0);
        // The change payload reuses the exact block JSON stored in
        // thread_item_blocks so a replayed text_block change is byte-identical
        // to the durable block row (完整文本与 UTF-16 offset 一并携带).
        let text = serde_json::to_string(block)?;
        let item_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM thread_items WHERE session_id=? AND user_id=? AND item_id=?)",
            params![session_id, user_id, item_id],
            |row| row.get(0),
        )?;
        anyhow::ensure!(item_exists, "thread item does not exist for block");
        let written = conn.execute("INSERT INTO thread_item_blocks(session_id,user_id,item_id,field,block_index,event_id,payload) VALUES (?,?,?,?,?,?,?) ON CONFLICT(session_id,item_id,field,block_index) DO UPDATE SET event_id=excluded.event_id,payload=excluded.payload WHERE thread_item_blocks.event_id<=excluded.event_id AND thread_item_blocks.payload<>excluded.payload",params![session_id,user_id,item_id,field,index,event_id,text])?;
        let mut change_seq = 0i64;
        if written > 0 {
            let (seq, turn_id): (i64, String) = conn.query_row(
                "SELECT l.latest_change_seq+1,i.turn_id FROM thread_logs l JOIN thread_items i ON i.session_id=l.session_id WHERE l.session_id=? AND i.item_id=?",
                params![session_id,item_id], |r| Ok((r.get(0)?,r.get(1)?)))?;
            let now = Self::now_ts();
            conn.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,'text_block',?,?,0,?,?)", params![session_id,seq,user_id,turn_id,item_id,text,now])?;
            conn.execute(
                "UPDATE thread_logs SET latest_change_seq=?,updated_time=? WHERE session_id=?",
                params![seq, now, session_id],
            )?;
            conn.execute(
                "DELETE FROM thread_log_changes WHERE session_id=? AND change_seq<=?",
                params![session_id, seq.saturating_sub(4096)],
            )?;
            change_seq = seq;
        }
        conn.commit()?;
        Ok(change_seq)
    }
    fn list_thread_text_blocks_impl(
        &self,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt=conn.prepare("SELECT payload FROM thread_item_blocks WHERE session_id=? AND event_id>? ORDER BY event_id LIMIT ?")?;
        let rows = stmt
            .query_map(params![session_id, after, limit.clamp(1, 500)], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|text| serde_json::from_str(&text).ok())
            .collect())
    }
    fn list_thread_item_blocks_page_impl(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        field: Option<&str>,
        from_block: i64,
        limit: i64,
        include_internal: bool,
    ) -> Result<(Vec<Value>, Option<i64>, bool)> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let requested = limit.clamp(1, 100);
        let field = field.unwrap_or("content");
        let mut stmt = conn.prepare("SELECT b.payload FROM thread_item_blocks b JOIN thread_items i ON i.session_id=b.session_id AND i.item_id=b.item_id WHERE b.user_id=? AND b.session_id=? AND b.item_id=? AND b.field=? AND b.block_index>=? AND (? OR i.visibility='user') ORDER BY b.block_index LIMIT ?")?;
        let rows = stmt
            .query_map(
                params![
                    user_id,
                    session_id,
                    item_id,
                    field,
                    from_block.max(0),
                    include_internal,
                    requested + 1
                ],
                |r| r.get::<_, String>(0),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = rows.len() > requested as usize;
        let blocks: Vec<Value> = rows
            .into_iter()
            .take(requested as usize)
            .filter_map(|text| serde_json::from_str(&text).ok())
            .collect();
        let next = blocks
            .last()
            .and_then(|block| block.get("block_index").and_then(Value::as_i64));
        Ok((blocks, next, has_more))
    }
    fn list_thread_item_blocks_impl(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        from_block: i64,
        limit: i64,
        include_internal: bool,
    ) -> Result<Vec<Value>> {
        Ok(self
            .list_thread_item_blocks_page_impl(
                user_id,
                session_id,
                item_id,
                None,
                from_block,
                limit,
                include_internal,
            )?
            .0)
    }

    fn find_thread_turn_id_impl(
        &self,
        user_id: &str,
        session_id: &str,
        user_turn_index: i64,
    ) -> Result<Option<String>> {
        self.ensure_initialized()?;
        let mut conn = self.open()?;
        Ok(conn.query_row("SELECT turn_id FROM thread_turns WHERE user_id=? AND session_id=? AND user_turn_index=? AND trigger_kind='user'", params![user_id,session_id,user_turn_index], |r| r.get(0)).optional()?)
    }
    fn accept_thread_turn_impl(
        &self,
        user_id: &str,
        session_id: &str,
        input: &Value,
    ) -> Result<Value> {
        self.ensure_initialized()?;
        ensure!(
            !user_id.trim().is_empty() && !session_id.trim().is_empty(),
            "missing thread identity"
        );
        let now = Self::now_ts();
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO thread_logs(session_id,user_id,created_time,updated_time) VALUES(?,?,?,?) ON CONFLICT(session_id) DO NOTHING", params![session_id,user_id,now,now])?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT user_id FROM thread_logs WHERE session_id=?",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(owner.as_deref() == Some(user_id), "thread owner mismatch");
        let client_id = input
            .get("client_message_id")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty());
        if let Some(client_id) = client_id {
            let existing: Option<String> = tx
                .query_row(
                    "SELECT turn_id FROM thread_turns WHERE session_id=? AND client_message_id=?",
                    params![session_id, client_id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(turn_id) = existing {
                let round: i64 = tx.query_row(
                    "SELECT user_turn_index FROM thread_turns WHERE session_id=? AND turn_id=?",
                    params![session_id, turn_id],
                    |r| r.get(0),
                )?;
                tx.commit()?;
                return Ok(json!({"turn_id":turn_id,"user_turn_index":round,"created":false}));
            }
        }
        let turn_id = Uuid::new_v4().to_string();
        let parent = input.get("root_user_round").and_then(Value::as_i64);
        let root: Option<String> = if let Some(round) = parent {
            tx.query_row("SELECT turn_id FROM thread_turns WHERE session_id=? AND user_turn_index=? AND trigger_kind='user'", params![session_id,round], |r| r.get(0)).optional()?
        } else {
            None
        };
        ensure!(
            parent.is_none() || root.is_some(),
            "continuation root is missing"
        );
        let trigger = if root.is_some() {
            "continuation"
        } else {
            "user"
        };
        let round: i64 = if let Some(round) = parent {
            round
        } else {
            tx.execute(
                "UPDATE thread_logs SET latest_user_turn=latest_user_turn+1 WHERE session_id=?",
                params![session_id],
            )?;
            tx.query_row(
                "SELECT latest_user_turn FROM thread_logs WHERE session_id=?",
                params![session_id],
                |r| r.get(0),
            )?
        };
        let root_id = root.unwrap_or_else(|| turn_id.clone());
        let summary: String = input
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(240)
            .collect();
        let mut item = input.clone();
        item["session_id"] = json!(session_id);
        item["turn_id"] = json!(turn_id);
        item["user_round"] = json!(round);
        let item_id = format!("{turn_id}:user");
        item["item_id"] = json!(item_id);
        item["status"] = json!("completed");
        let text = serde_json::to_string(&item)?;
        tx.execute("INSERT INTO thread_turns(session_id,turn_id,root_turn_id,trigger_kind,client_message_id,user_id,user_turn_index,status,summary,payload,created_time,updated_time) VALUES(?,?,?,?,?,?,?,'queued',?,'{}',?,?)", params![session_id,turn_id,root_id,trigger,client_id,user_id,round,summary,now,now])?;
        let index:i64=tx.query_row("SELECT COALESCE(MAX(item_index),-1)+1 FROM thread_items WHERE session_id=? AND root_turn_id=?", params![session_id,root_id], |r| r.get(0))?;
        let visibility = if trigger == "user" {
            "user"
        } else {
            "model_internal"
        };
        tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq) VALUES(?,?,?,?,?,?,?,'user_message','completed',?,?,?,(SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=?))", params![session_id,item_id,turn_id,root_id,visibility,user_id,index,text,now,now,session_id])?;
        let change_type = "turn_upsert";
        let change_item: Option<&str> = None;
        let revision = 1i64;
        let seq: i64 = tx.query_row(
            "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=?",
            params![session_id],
            |r| r.get(0),
        )?;
        let change_payload = serde_json::to_string(&json!({
            "turn_id": turn_id,
            "root_turn_id": root_id,
            "trigger_kind": trigger,
            "status": "queued",
            "user_round": round,
            "client_message_id": client_id,
        }))?;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,?,?,?,?,?,?)", params![session_id,seq,user_id,change_type,turn_id,change_item,revision,change_payload,now])?;
        tx.execute(
            "UPDATE thread_logs SET latest_change_seq=?,updated_time=? WHERE session_id=?",
            params![seq, now, session_id],
        )?;
        let item_seq = seq + 1;
        // The change payload is the complete committed item row so a replayed
        // frame can apply it without consulting the mutable current row.
        let item_change_payload = committed_item_payload(&tx, session_id, &item_id)?;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,?,?,?,?,?,?)", params![session_id,item_seq,user_id,"item_upsert",turn_id,item_id,revision,item_change_payload,now])?;
        tx.execute(
            "UPDATE thread_logs SET latest_change_seq=? WHERE session_id=?",
            params![item_seq, session_id],
        )?;
        tx.execute(
            "INSERT INTO thread_log_metrics(session_id,user_id,metric_key,metric_value,updated_time) SELECT ?,?,?,latest_user_turn,? FROM thread_logs WHERE session_id=? ON CONFLICT(session_id,metric_key) DO UPDATE SET metric_value=excluded.metric_value,updated_time=excluded.updated_time",
            params![session_id, user_id, "user_turn_total", now, session_id],
        )?;
        // Changes are a bounded recovery index; the durable items are never pruned here.
        tx.execute(
            "DELETE FROM thread_log_changes WHERE session_id=? AND change_seq<=?",
            params![session_id, item_seq.saturating_sub(4096)],
        )?;
        tx.commit()?;
        Ok(json!({"turn_id":turn_id,"root_turn_id":root_id,"user_turn_index":round,"created":true}))
    }
    fn update_thread_turn_impl(
        &self,
        user_id: &str,
        session_id: &str,
        turn_id: &str,
        status: &str,
        _summary: &str,
        payload: &Value,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let now = Self::now_ts();
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT user_id FROM thread_logs WHERE session_id=?",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(owner.as_deref() == Some(user_id), "thread owner mismatch");
        let mut payload = payload.clone();
        if let Some(map) = payload.as_object_mut() {
            map.remove("answer");
            map.remove("summary");
        }
        let text = serde_json::to_string(&payload)?;
        let changed=tx.execute("UPDATE thread_turns SET status=?,payload=?,updated_time=? WHERE session_id=? AND turn_id=? AND status IN ('queued','running','waiting_input') AND (status<>? OR payload<>?)", params![status,text,now,session_id,turn_id,status,text])?;
        if changed == 0 {
            tx.commit()?;
            return Ok(false);
        }
        // Keep the visible user bubble in sync with its durable turn.  The
        // bubble is created as running when the request is admitted, so a
        // terminal turn must close that item as well.
        let bubble_status = if matches!(
            status,
            "completed" | "cancelled" | "failed" | "rejected" | "stopped" | "interrupted"
        ) {
            "completed"
        } else {
            "running"
        };
        let input_item_id = format!("{turn_id}:user");
        let input_changed = tx.execute(
            "UPDATE thread_items SET status=?, payload=json_set(payload, '$.status', ?), revision=revision+1, updated_time=? WHERE session_id=? AND item_id=? AND status<>?",
            params![bubble_status, bubble_status, now, session_id, input_item_id, bubble_status],
        )?;
        // A terminal root turn owns the lifecycle of every unfinished item.
        // The turn_upsert notification below invalidates the entire paged turn.
        let mut settled_items = Vec::<String>::new();
        if bubble_status == "completed" {
            let mut stmt = tx.prepare("SELECT item_id FROM thread_items WHERE session_id=? AND turn_id=? AND status IN ('running','queued','waiting_input') ORDER BY created_seq,item_index")?;
            settled_items = stmt
                .query_map(params![session_id, turn_id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            tx.execute("UPDATE thread_items SET status=?1, payload=json_set(payload, '$.status', ?1), revision=revision+1, updated_time=?2 WHERE session_id=?3 AND turn_id=?4 AND status IN ('running','queued','waiting_input')", params![status,now,session_id,turn_id])?;
        }
        let change_type = "turn_upsert";
        let change_item: Option<&str> = None;
        let mut seq: i64 = tx.query_row(
            "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=?",
            params![session_id],
            |r| r.get(0),
        )?;
        let revision = seq;
        let (root_id, trigger, user_round): (String, String, i64) = tx.query_row(
            "SELECT root_turn_id,trigger_kind,user_turn_index FROM thread_turns WHERE session_id=? AND turn_id=?",
            params![session_id, turn_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let change_payload = serde_json::to_string(&json!({"turn_id": turn_id, "status": status,
            "root_turn_id": root_id, "trigger_kind": trigger, "user_round": user_round}))?;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,?,?,?,?,?,?)", params![session_id,seq,user_id,change_type,turn_id,change_item,revision,change_payload,now])?;
        if input_changed > 0 {
            seq += 1;
            let bubble_payload = committed_item_payload(&tx, session_id, &input_item_id)?;
            let bubble_revision = bubble_payload.parse::<Value>()?["revision"]
                .as_i64()
                .unwrap_or(0);
            tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,?,?,?,?,?,?)", params![session_id,seq,user_id,"item_upsert",turn_id,input_item_id,bubble_revision,bubble_payload,now])?;
        }
        let had_settled = !settled_items.is_empty();
        for item_id in settled_items {
            seq += 1;
            let item_payload = committed_item_payload(&tx, session_id, &item_id)?;
            let item_revision = item_payload.parse::<Value>()?["revision"]
                .as_i64()
                .unwrap_or(0);
            tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,?,?,?,?,?,?)", params![session_id,seq,user_id,"item_upsert",turn_id,item_id,item_revision,item_payload,now])?;
        }
        tx.execute(
            "UPDATE thread_logs SET latest_change_seq=?,updated_time=? WHERE session_id=?",
            params![seq, now, session_id],
        )?;
        // Changes are a bounded recovery index; the durable items are never pruned here.
        tx.execute(
            "DELETE FROM thread_log_changes WHERE session_id=? AND change_seq<=?",
            params![session_id, seq.saturating_sub(4096)],
        )?;
        tx.commit()?;
        Ok(changed > 0 || input_changed > 0 || had_settled)
    }
    fn append_thread_item_impl(&self, user_id: &str, payload: &Value) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let session_id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let turn_id = payload.get("turn_id").and_then(Value::as_str).unwrap_or("");
        let item_id = payload.get("item_id").and_then(Value::as_str).unwrap_or("");
        // Non-turn configuration/history records are intentionally outside the timeline.
        if turn_id.is_empty() {
            return Ok(None);
        }
        ensure!(!item_id.is_empty(), "missing stable item identity");
        let now = Self::now_ts();
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT user_id FROM thread_logs WHERE session_id=?",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(owner.as_deref() == Some(user_id), "thread owner mismatch");
        let root_id: Option<String> = tx
            .query_row(
                "SELECT root_turn_id FROM thread_turns WHERE session_id=? AND turn_id=?",
                params![session_id, turn_id],
                |r| r.get(0),
            )
            .optional()?;
        let root_id = root_id.ok_or_else(|| anyhow::anyhow!("unknown thread turn"))?;
        let kind = payload
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("event");
        let status = payload
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("completed");
        let visibility = if payload.pointer("/meta/hidden").and_then(Value::as_bool) == Some(true) {
            "model_internal"
        } else {
            payload
                .get("visibility")
                .and_then(Value::as_str)
                .unwrap_or("user")
        };
        let text = serde_json::to_string(payload)?;
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT turn_id,payload FROM thread_items WHERE session_id=? AND item_id=?",
                params![session_id, item_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((existing_turn, _)) = &existing {
            ensure!(
                existing_turn == turn_id,
                "thread item belongs to another turn"
            );
        }
        if existing.as_ref().map(|(_, value)| value.as_str()) == Some(text.as_str()) {
            tx.commit()?;
            return Ok(None);
        }
        let index:i64=tx.query_row("SELECT COALESCE(MAX(item_index),-1)+1 FROM thread_items WHERE session_id=? AND root_turn_id=?", params![session_id,root_id], |r| r.get(0))?;
        tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,(SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=?)) ON CONFLICT(session_id,item_id) DO UPDATE SET kind=excluded.kind,status=excluded.status,visibility=excluded.visibility,payload=excluded.payload,revision=thread_items.revision+1,updated_time=excluded.updated_time WHERE thread_items.turn_id=excluded.turn_id", params![session_id,item_id,turn_id,root_id,visibility,user_id,index,kind,status,text,now,now,session_id])?;
        let revision: i64 = tx.query_row(
            "SELECT revision FROM thread_items WHERE session_id=? AND item_id=?",
            params![session_id, item_id],
            |r| r.get(0),
        )?;
        let change_type = "item_upsert";
        let change_item = Some(item_id);
        let seq: i64 = tx.query_row(
            "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=?",
            params![session_id],
            |r| r.get(0),
        )?;
        let change_payload = committed_item_payload(&tx, session_id, item_id)?;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,?,?,?,?,?,?)", params![session_id,seq,user_id,change_type,turn_id,change_item,revision,change_payload,now])?;
        tx.execute(
            "UPDATE thread_logs SET latest_change_seq=?,updated_time=? WHERE session_id=?",
            params![seq, now, session_id],
        )?;
        // Changes are a bounded recovery index; the durable items are never pruned here.
        tx.execute(
            "DELETE FROM thread_log_changes WHERE session_id=? AND change_seq<=?",
            params![session_id, seq.saturating_sub(4096)],
        )?;
        tx.commit()?;
        Ok(Some(
            json!({"change_type":"item_upsert","turn_id":turn_id,"item_id":item_id,"revision":revision,"cursor":seq}),
        ))
    }
    fn list_thread_turns_impl(
        &self,
        user_id: &str,
        session_id: &str,
        before: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let mut conn = self.open()?;
        let before = before.unwrap_or(i64::MAX);
        let limit = limit.clamp(1, 101);
        Ok({
            let mut stmt = conn.prepare("SELECT turn_id,user_turn_index,status,summary,payload,updated_time,root_turn_id,trigger_kind FROM thread_turns WHERE user_id=? AND session_id=? AND user_turn_index<? AND trigger_kind='user' ORDER BY user_turn_index DESC LIMIT ?")?;
            let rows = stmt
                .query_map(params![user_id, session_id, before, limit], turn_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        })
    }
    fn get_thread_log_counts_impl(
        &self,
        user_id: &str,
        session_id: &str,
        include_internal: bool,
    ) -> Result<(i64, i64)> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let user_turns = conn.query_row(
            "SELECT COUNT(*) FROM thread_turns WHERE user_id=? AND session_id=? AND trigger_kind='user'",
            params![user_id, session_id],
            |r| r.get(0),
        )?;
        let items = conn.query_row(
            "SELECT COUNT(*) FROM thread_items WHERE user_id=? AND session_id=? AND (? OR visibility='user')",
            params![user_id, session_id, include_internal],
            |r| r.get(0),
        )?;
        Ok((user_turns, items))
    }
    fn latest_thread_user_round_by_session_impl(&self, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        Ok(conn.query_row("SELECT COALESCE(MAX(user_turn_index),0) FROM thread_turns WHERE session_id=? AND trigger_kind='user'", params![session_id], |r| r.get(0))?)
    }
    fn get_thread_turn_impl(
        &self,
        user_id: &str,
        session_id: &str,
        turn_id: &str,
        after: i64,
        limit: i64,
        include_internal: bool,
    ) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let mut conn = self.open()?;
        let limit = limit.clamp(1, 100);
        let fetch = limit + 1;
        let mut turns = {
            let mut stmt = conn.prepare("SELECT turn_id,user_turn_index,status,summary,payload,updated_time,root_turn_id,trigger_kind FROM thread_turns WHERE user_id=? AND session_id=? AND turn_id=?")?;
            let rows = stmt
                .query_map(params![user_id, session_id, turn_id], turn_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let Some(mut turn) = turns.pop() else {
            return Ok(None);
        };
        let root_id = turn["root_turn_id"].as_str().unwrap_or(turn_id);
        let mut items = {
            let mut stmt = conn.prepare("SELECT item_id,item_index,kind,status,revision,payload,created_time,updated_time,turn_id,visibility FROM thread_items WHERE user_id=? AND session_id=? AND root_turn_id=? AND item_index>? AND (? OR visibility='user') ORDER BY item_index LIMIT ?")?;
            let rows = stmt
                .query_map(
                    params![user_id, session_id, root_id, after, include_internal, fetch],
                    item_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next = items
            .last()
            .and_then(|v| v["item_index"].as_i64())
            .unwrap_or(after);
        if !include_internal {
            for item in &mut items {
                if let Some(map) = item["payload"].as_object_mut() {
                    map.remove("model_content");
                    map.remove("config_overrides");
                }
            }
        }
        turn["items"] = json!(items);
        turn["has_more"] = json!(has_more);
        turn["next_after"] = json!(next);
        Ok(Some(turn))
    }
    fn get_thread_item_impl(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        include_internal: bool,
    ) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare("SELECT item_id,item_index,kind,status,revision,payload,created_time,updated_time,turn_id,visibility FROM thread_items WHERE user_id=? AND session_id=? AND item_id=? AND (? OR visibility='user') LIMIT 1")?;
        let mut items = stmt
            .query_map(
                params![user_id, session_id, item_id, include_internal],
                item_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let Some(mut item) = items.pop() else {
            return Ok(None);
        };
        if !include_internal {
            if let Some(map) = item["payload"].as_object_mut() {
                map.remove("model_content");
                map.remove("config_overrides");
            }
        }
        Ok(Some(item))
    }
    fn set_thread_item_feedback_impl(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        vote: &str,
    ) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        ensure!(matches!(vote, "up" | "down"), "invalid feedback vote");
        let now = Self::now_ts();
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let row: Option<(String, String, String, String)> = tx.query_row(
            "SELECT turn_id,kind,visibility,payload FROM thread_items WHERE user_id=? AND session_id=? AND item_id=?",
            params![user_id, session_id, item_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        let Some((turn_id, kind, visibility, payload_text)) = row else {
            return Ok(None);
        };
        ensure!(
            kind == "assistant_message" && visibility == "user",
            "feedback requires visible assistant item"
        );
        let mut payload: Value = serde_json::from_str(&payload_text)?;
        let map = payload
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("invalid thread item payload"))?;
        if map.get("feedback").is_some() {
            return Ok(None);
        }
        let feedback = json!({"vote":vote,"created_at":now,"locked":true});
        map.insert("feedback".to_string(), feedback.clone());
        let text = serde_json::to_string(&payload)?;
        tx.execute("UPDATE thread_items SET payload=?,revision=revision+1,updated_time=? WHERE session_id=? AND item_id=?", params![text,now,session_id,item_id])?;
        let revision: i64 = tx.query_row(
            "SELECT revision FROM thread_items WHERE session_id=? AND item_id=?",
            params![session_id, item_id],
            |row| row.get(0),
        )?;
        let seq: i64 = tx.query_row(
            "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=?",
            params![session_id],
            |row| row.get(0),
        )?;
        let feedback_payload = committed_item_payload(&tx, session_id, item_id)?;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,?,?,?,?,?,?)", params![session_id,seq,user_id,"item_upsert",turn_id,item_id,revision,feedback_payload,now])?;
        tx.execute(
            "UPDATE thread_logs SET latest_change_seq=?,updated_time=? WHERE session_id=?",
            params![seq, now, session_id],
        )?;
        tx.commit()?;
        Ok(Some(feedback))
    }
    fn thread_snapshot_impl(&self, user_id: &str, session_id: &str) -> Result<Value> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let tx = conn.unchecked_transaction()?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT user_id FROM thread_logs WHERE session_id=?",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;
        // A newly created chat session has no ThreadLog until its first turn.
        // The API already checks session ownership; absent logs contain no data.
        if owner.is_none() {
            return Ok(json!({"cursor":0,"turns":[],"items":[],"blocks":[],"item_total":0}));
        }
        ensure!(owner.as_deref() == Some(user_id), "thread owner mismatch");
        let cursor: i64 = tx.query_row(
            "SELECT latest_change_seq FROM thread_logs WHERE session_id=?",
            params![session_id],
            |r| r.get(0),
        )?;
        let mut turns = Vec::new();
        {
            let mut stmt = tx.prepare("SELECT turn_id,user_turn_index,status,summary,payload,updated_time,root_turn_id,trigger_kind FROM thread_turns WHERE user_id=? AND session_id=? ORDER BY user_turn_index ASC, created_time ASC, turn_id ASC")?;
            let rows = stmt.query_map(params![user_id, session_id], turn_row)?;
            for row in rows {
                let mut turn = row?;
                if turn["trigger_kind"] == "continuation" {
                    // Only lifecycle/identity is needed by the render projection.
                    turn["summary"] = json!("");
                }
                turns.push(turn);
            }
        }
        let mut items = Vec::new();
        {
            let mut stmt = tx.prepare("SELECT item_id,item_index,kind,status,revision,payload,created_time,updated_time,turn_id,visibility,root_turn_id,created_seq FROM thread_items WHERE user_id=? AND session_id=? AND visibility='user' ORDER BY created_seq ASC, item_index ASC")?;
            let rows = stmt.query_map(params![user_id, session_id], change_item_row)?;
            for row in rows {
                let mut item = row?;
                if let Some(map) = item["payload"].as_object_mut() {
                    map.remove("model_content");
                    map.remove("config_overrides");
                }
                items.push(item);
            }
        }
        let mut blocks = Vec::new();
        {
            let mut stmt = tx.prepare("SELECT payload FROM thread_item_blocks WHERE user_id=? AND session_id=? AND item_id IN (SELECT item_id FROM thread_items WHERE user_id=? AND session_id=? AND visibility='user') ORDER BY item_id, field, block_index")?;
            let rows = stmt.query_map(params![user_id, session_id, user_id, session_id], |r| {
                Ok(serde_json::from_str::<Value>(&r.get::<_, String>(0)?).unwrap_or(Value::Null))
            })?;
            for row in rows {
                blocks.push(row?);
            }
        }
        tx.commit()?;
        Ok(json!({
            "cursor": cursor,
            "turns": turns,
            "items": items,
            "blocks": blocks,
            "item_total": items.len(),
        }))
    }
    fn list_thread_changes_by_session_impl(
        &self,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let after = after.max(0);
        let earliest: Option<i64> = conn.query_row(
            "SELECT MIN(change_seq) FROM thread_log_changes WHERE session_id=?",
            params![session_id],
            |r| r.get(0),
        )?;
        if earliest.is_some_and(|first| first > after.saturating_add(1)) {
            return Ok(vec![json!({"change_type":"snapshot_required"})]);
        }
        let mut stmt = conn.prepare("SELECT change_seq,change_type,turn_id,item_id,revision,payload,created_time FROM thread_log_changes WHERE session_id=? AND change_seq>? ORDER BY change_seq LIMIT ?")?;
        let rows = stmt
            .query_map(params![session_id, after, limit.clamp(1, 500)], change_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    fn list_thread_changes_impl(
        &self,
        user_id: &str,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let mut conn = self.open()?;
        let limit = limit.clamp(1, 500);
        let after = after.max(0);
        let earliest: Option<i64> = conn.query_row(
            "SELECT MIN(change_seq) FROM thread_log_changes WHERE user_id=? AND session_id=?",
            params![user_id, session_id],
            |r| r.get(0),
        )?;
        if earliest.is_some_and(|first| first > after.saturating_add(1)) {
            return Ok(vec![json!({"change_type":"snapshot_required"})]);
        }
        Ok({
            let mut stmt = conn.prepare("SELECT change_seq,change_type,turn_id,item_id,revision,payload,created_time FROM thread_log_changes WHERE user_id=? AND session_id=? AND change_seq>? ORDER BY change_seq LIMIT ?")?;
            let rows = stmt
                .query_map(params![user_id, session_id, after, limit], change_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        })
    }
    fn delete_thread_log_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut count = 0i64;
        count += tx.execute(
            "DELETE FROM thread_log_metrics WHERE user_id=? AND session_id=?",
            params![user_id, session_id],
        )? as i64;
        count += tx.execute(
            "DELETE FROM thread_item_blocks WHERE user_id=? AND session_id=?",
            params![user_id, session_id],
        )? as i64;
        count += tx.execute(
            "DELETE FROM thread_log_changes WHERE user_id=? AND session_id=?",
            params![user_id, session_id],
        )? as i64;
        count += tx.execute(
            "DELETE FROM thread_items WHERE user_id=? AND session_id=?",
            params![user_id, session_id],
        )? as i64;
        count += tx.execute(
            "DELETE FROM thread_turns WHERE user_id=? AND session_id=?",
            params![user_id, session_id],
        )? as i64;
        count += tx.execute(
            "DELETE FROM thread_logs WHERE user_id=? AND session_id=?",
            params![user_id, session_id],
        )? as i64;
        tx.commit()?;
        Ok(count)
    }
}
fn turn_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"turn_id":r.get::<_,String>(0)?,"user_turn_index":r.get::<_,i64>(1)?,"status":r.get::<_,String>(2)?,"summary":r.get::<_,String>(3)?,"payload":serde_json::from_str::<Value>(&r.get::<_,String>(4)?).unwrap_or(Value::Null),"updated_time":r.get::<_,f64>(5)?,"root_turn_id":r.get::<_,String>(6)?,"trigger_kind":r.get::<_,String>(7)?}),
    )
}
fn item_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"item_id":r.get::<_,String>(0)?,"item_index":r.get::<_,i64>(1)?,"kind":r.get::<_,String>(2)?,"status":r.get::<_,String>(3)?,"revision":r.get::<_,i64>(4)?,"payload":serde_json::from_str::<Value>(&r.get::<_,String>(5)?).unwrap_or(Value::Null),"created_time":r.get::<_,f64>(6)?,"updated_time":r.get::<_,f64>(7)?,"turn_id":r.get::<_,String>(8)?,"visibility":r.get::<_,String>(9)?}),
    )
}
/// Full committed row projection for immutable change payloads and the atomic
/// snapshot. Superset of `item_row`: adds root_turn_id and created_seq.
fn change_item_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"item_id":r.get::<_,String>(0)?,"item_index":r.get::<_,i64>(1)?,"kind":r.get::<_,String>(2)?,"status":r.get::<_,String>(3)?,"revision":r.get::<_,i64>(4)?,"payload":serde_json::from_str::<Value>(&r.get::<_,String>(5)?).unwrap_or(Value::Null),"created_time":r.get::<_,f64>(6)?,"updated_time":r.get::<_,f64>(7)?,"turn_id":r.get::<_,String>(8)?,"visibility":r.get::<_,String>(9)?,"root_turn_id":r.get::<_,String>(10)?,"created_seq":r.get::<_,i64>(11)?}),
    )
}
/// Serialize the committed row of one item inside the writing transaction.
/// I1: change payloads must be complete and immutable at commit time; replay
/// must never re-read the current row, which later revisions overwrite.
fn committed_item_payload(
    tx: &rusqlite::Transaction<'_>,
    session_id: &str,
    item_id: &str,
) -> Result<String> {
    let mut stmt = tx.prepare(
        "SELECT item_id,item_index,kind,status,revision,payload,created_time,updated_time,turn_id,visibility,root_turn_id,created_seq FROM thread_items WHERE session_id=? AND item_id=?",
    )?;
    let mut row = stmt.query_row(params![session_id, item_id], change_item_row)?;
    // User-visible change payloads match get_thread_item's projection: the
    // prompt-side fields never leave the model boundary.
    if row["visibility"] == json!("user") {
        if let Some(map) = row["payload"].as_object_mut() {
            map.remove("model_content");
            map.remove("config_overrides");
        }
    }
    Ok(serde_json::to_string(&row)?)
}
fn change_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"change_seq":r.get::<_,i64>(0)?,"change_type":r.get::<_,String>(1)?,"turn_id":r.get::<_,String>(2)?,"item_id":r.get::<_,Option<String>>(3)?,"revision":r.get::<_,i64>(4)?,"payload":serde_json::from_str::<Value>(&r.get::<_,String>(5)?).unwrap_or(Value::Null),"created_time":r.get::<_,f64>(6)?}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn input(id: usize) -> Value {
        json!({"role":"user","content":format!("message {id}"),"client_message_id":format!("message-{id}")})
    }
    #[test]
    fn thread_snapshot_before_first_turn_is_empty_and_existing_owner_is_checked() {
        use wunder_core::storage_backend::ThreadLogStore;
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("empty.db").to_string_lossy().into_owned());
        assert_eq!(
            db.thread_snapshot("owner", "thread").unwrap(),
            json!({"cursor":0,"turns":[],"items":[],"blocks":[],"item_total":0})
        );
        db.accept_thread_turn("owner", "thread", &input(1)).unwrap();
        assert!(
            db.thread_snapshot("owner", "thread").unwrap()["cursor"]
                .as_i64()
                .unwrap()
                > 0
        );
        assert!(db.thread_snapshot("other", "thread").is_err());
    }
    #[test]
    fn active_text_recovery_pages_fields_and_commits_idempotent_change_cursors() {
        use wunder_core::storage_backend::ThreadLogStore;
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("blocks.db").to_string_lossy().into_owned());
        let accepted = db.accept_thread_turn("owner", "thread", &input(1)).unwrap();
        let mut message = json!({"session_id":"thread", "turn_id":accepted["turn_id"],
            "item_id":"answer", "kind":"assistant_message", "role":"assistant",
            "visibility":"user", "status":"running", "content":"", "reasoning":""});
        db.append_thread_item("owner", &message).unwrap();
        let start = db.latest_thread_change_seq_by_session("thread").unwrap();
        let mut last = Value::Null;
        for index in 0..102 {
            last = json!({"item_id":"answer", "field":"content", "block_index":index,
                "event_id":index+100, "data":{"field":"content", "content":"x😀"}});
            db.upsert_thread_text_block("owner", "thread", &last)
                .unwrap();
        }
        let cursor = db.latest_thread_change_seq_by_session("thread").unwrap();
        assert_eq!(cursor, start + 102);
        db.upsert_thread_text_block("owner", "thread", &last)
            .unwrap();
        assert_eq!(
            db.latest_thread_change_seq_by_session("thread").unwrap(),
            cursor
        );
        let mut stale = last.clone();
        stale["event_id"] = json!(1);
        stale["data"]["content"] = json!("stale");
        db.upsert_thread_text_block("owner", "thread", &stale)
            .unwrap();
        assert_eq!(
            db.latest_thread_change_seq_by_session("thread").unwrap(),
            cursor
        );
        db.upsert_thread_text_block(
            "owner",
            "thread",
            &json!({"item_id":"answer",
            "field":"reasoning", "block_index":0, "event_id":201,
            "data":{"reasoning":" thought "}}),
        )
        .unwrap();
        crate::services::thread_log::hydrate_active_text(&db, "owner", "thread", &mut message)
            .unwrap();
        assert_eq!(message["content"], "x😀".repeat(102));
        assert_eq!(message["reasoning"], " thought ");
        let changes = db
            .list_thread_changes_by_session("thread", cursor, 10)
            .unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0]["change_type"], "text_block");
        assert_eq!(
            changes[0]["payload"],
            json!({"item_id":"answer","field":"reasoning","block_index":0,"event_id":201,"data":{"reasoning":" thought "}})
        );
        message["status"] = json!("completed");
        message["content"] = json!("final answer");
        crate::services::thread_log::hydrate_active_text(&db, "owner", "thread", &mut message)
            .unwrap();
        assert_eq!(message["content"], "final answer");
    }
    #[test]
    fn durable_catalog_survives_more_than_five_hundred_turns() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        for i in 0..601 {
            db.accept_thread_turn_impl("owner", "thread", &input(i))
                .unwrap();
        }
        let mut before = None;
        let mut count = 0;
        loop {
            let page = db
                .list_thread_turns_impl("owner", "thread", before, 73)
                .unwrap();
            if page.is_empty() {
                break;
            }
            before = page.last().unwrap()["user_turn_index"].as_i64();
            count += page.len();
        }
        assert_eq!(count, 601);
        assert_eq!(before, Some(1));
        assert!(db
            .list_thread_turns_impl("other", "thread", None, 100)
            .unwrap()
            .is_empty());
        let repeated = db
            .accept_thread_turn_impl("owner", "thread", &input(20))
            .unwrap();
        assert_eq!(repeated["created"], false);
        assert_eq!(repeated["user_turn_index"], 21);
        assert!(db
            .accept_thread_turn_impl("other", "thread", &input(602))
            .is_err());
    }
    #[test]
    fn late_items_do_not_change_terminal_and_item_pages_filter_internal_context() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let accepted = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let id = accepted["turn_id"].as_str().unwrap();
        db.update_thread_turn_impl("owner", "thread", id, "cancelled", "", &json!({}))
            .unwrap();
        for i in 0..205 {
            db.append_thread_item_impl("owner",&json!({"session_id":"thread","turn_id":id,"item_id":format!("item-{i}"),"kind":"assistant_message","content":"text","model_content":"internal","meta":{"hidden":i%2==0}})).unwrap();
        }
        db.update_thread_turn_impl("owner", "thread", id, "running", "", &json!({}))
            .unwrap();
        let mut after = -1;
        let mut count = 0;
        loop {
            let turn = db
                .get_thread_turn_impl("owner", "thread", id, after, 19, false)
                .unwrap()
                .unwrap();
            assert_eq!(turn["status"], "cancelled");
            let items = turn["items"].as_array().unwrap();
            assert!(items.len() <= 19);
            count += items.len();
            assert!(items
                .iter()
                .all(|v| v.pointer("/payload/model_content").is_none()));
            if turn["has_more"] == false {
                break;
            }
            after = turn["next_after"].as_i64().unwrap();
        }
        assert_eq!(count, 103);
        assert!(db
            .get_thread_turn_impl("other", "thread", id, -1, 100, true)
            .unwrap()
            .is_none());
    }
    #[test]
    fn continuation_has_separate_identity_and_preserves_root() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let root = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let next = db
            .accept_thread_turn_impl(
                "owner",
                "thread",
                &json!({"root_user_round":1,"content":"continue"}),
            )
            .unwrap();
        assert_ne!(root["turn_id"], next["turn_id"]);
        assert_eq!(next["root_turn_id"], root["turn_id"]);
        db.update_thread_turn_impl(
            "owner",
            "thread",
            next["turn_id"].as_str().unwrap(),
            "running",
            "",
            &json!({}),
        )
        .unwrap();
        let changes = db
            .list_thread_changes_impl("owner", "thread", 0, 100)
            .unwrap();
        for status in ["queued", "running"] {
            let change = changes
                .iter()
                .find(|change| {
                    change["turn_id"] == next["turn_id"]
                        && change["change_type"] == "turn_upsert"
                        && change["payload"]["status"] == status
                })
                .unwrap();
            assert_eq!(change["payload"]["root_turn_id"], root["turn_id"]);
            assert_eq!(change["payload"]["trigger_kind"], "continuation");
            assert_eq!(change["payload"]["user_round"], 1);
        }
        let snapshot = db.thread_snapshot_impl("owner", "thread").unwrap();
        assert_eq!(snapshot["turns"].as_array().unwrap().len(), 2);
        let continuation = snapshot["turns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|turn| turn["turn_id"] == next["turn_id"])
            .unwrap();
        assert_eq!(continuation["root_turn_id"], root["turn_id"]);
        assert_eq!(continuation["trigger_kind"], "continuation");
        // Internal continuation input is excluded even though its lifecycle
        // must be present to rebuild the user bubble after reconnect.
        assert_eq!(snapshot["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            db.list_thread_turns_impl("owner", "thread", None, 100)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            db.accept_thread_turn_impl("owner", "thread", &input(2))
                .unwrap()["user_turn_index"],
            2
        );
    }
    #[test]
    fn cancellation_settles_unfinished_items_and_preserves_completed_ones() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let accepted = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let id = accepted["turn_id"].as_str().unwrap();
        for (item, status) in [
            ("partial", "running"),
            ("tool", "running"),
            ("done", "completed"),
        ] {
            db.append_thread_item_impl(
                "owner",
                &json!({"session_id":"thread","turn_id":id,
                "item_id":item,"kind":"assistant_message","role":"assistant","status":status,
                "content":"Retained text"}),
            )
            .unwrap();
        }
        db.update_thread_turn_impl("owner", "thread", id, "cancelled", "", &json!({}))
            .unwrap();
        for (item, expected) in [
            ("partial", "cancelled"),
            ("tool", "cancelled"),
            ("done", "completed"),
        ] {
            let value = db
                .get_thread_item_impl("owner", "thread", item, true)
                .unwrap()
                .unwrap();
            assert_eq!(value["status"], expected);
            assert_eq!(value["payload"]["status"], expected);
            assert_eq!(value["payload"]["content"], "Retained text");
        }
    }

    #[test]
    fn terminal_turn_closes_its_user_message() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let accepted = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let id = accepted["turn_id"].as_str().unwrap();
        db.append_thread_item_impl(
            "owner",
            &json!({
                "session_id":"thread", "turn_id":id, "item_id":format!("{id}:user"),
                "kind":"user_message", "status":"running", "content":"message 1"
            }),
        )
        .unwrap();
        db.update_thread_turn_impl("owner", "thread", id, "completed", "", &json!({}))
            .unwrap();
        let turn = db
            .get_thread_turn_impl("owner", "thread", id, -1, 10, true)
            .unwrap()
            .unwrap();
        assert_eq!(turn["items"][0]["status"], "completed");
        assert_eq!(turn["items"][0]["payload"]["status"], "completed");
    }
    #[test]
    fn item_blocks_respect_visibility_projection() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let accepted = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let id = accepted["turn_id"].as_str().unwrap();
        for (item, visibility) in [("public", "user"), ("internal", "model_internal")] {
            db.append_thread_item_impl("owner", &json!({"session_id":"thread","turn_id":id,"item_id":item,"kind":"assistant_message","visibility":visibility})).unwrap();
            db.upsert_thread_text_block_impl(
                "owner",
                "thread",
                &json!({"item_id":item,"block_index":0,"event_id":1,"content":item}),
            )
            .unwrap();
        }
        let visible = db
            .list_thread_item_blocks_impl("owner", "thread", "internal", 0, 10, false)
            .unwrap();
        assert!(visible.is_empty());
        let internal = db
            .list_thread_item_blocks_impl("owner", "thread", "internal", 0, 10, true)
            .unwrap();
        assert_eq!(internal.len(), 1);
    }
    #[test]
    fn model_context_projection_includes_internal_but_user_projection_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let accepted = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let id = accepted["turn_id"].as_str().unwrap();
        db.append_thread_item_impl("owner", &json!({"session_id":"thread","turn_id":id,"item_id":"internal-context","kind":"assistant_message","visibility":"model_internal","role":"assistant","content":"hidden context"})).unwrap();
        let model = crate::storage::ThreadLogStore::load_thread_context_items(
            &db, "owner", "thread", 0, true,
        )
        .unwrap();
        assert!(model.iter().any(|item| item["content"] == "hidden context"));
        let user = crate::storage::ThreadLogStore::load_thread_context_items(
            &db, "owner", "thread", 0, false,
        )
        .unwrap();
        assert!(!user.iter().any(|item| item["content"] == "hidden context"));
    }
    #[test]
    fn admission_emits_turn_and_initial_item_changes() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let accepted = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let changes = db
            .list_thread_changes_impl("owner", "thread", 0, 10)
            .unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0]["change_type"], "turn_upsert");
        assert_eq!(changes[1]["change_type"], "item_upsert");
        assert_eq!(
            changes[1]["item_id"],
            format!("{}:user", accepted["turn_id"].as_str().unwrap())
        );
    }
    #[test]
    fn model_context_pages_all_items_in_a_dense_turn() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let accepted = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let id = accepted["turn_id"].as_str().unwrap();
        for index in 0..205 {
            db.append_thread_item_impl("owner", &json!({"session_id":"thread","turn_id":id,"item_id":format!("dense-{index}"),"kind":"assistant_message","role":"assistant","content":format!("entry-{index}")})).unwrap();
        }
        let items = crate::storage::ThreadLogStore::load_thread_context_items(
            &db, "owner", "thread", 0, true,
        )
        .unwrap();
        assert!(items.iter().any(|item| item["content"] == "entry-204"));
        assert!(items.len() >= 206);
    }
    #[test]
    fn concurrent_admission_allocates_unique_rounds() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(SqliteStorage::new(
            dir.path().join("log.db").to_string_lossy().into_owned(),
        ));
        db.ensure_initialized().unwrap();
        let handles = (0..8)
            .map(|i| {
                let db = db.clone();
                std::thread::spawn(move || {
                    db.accept_thread_turn_impl("owner", "thread", &input(i))
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let mut rounds = handles
            .into_iter()
            .map(|h| h.join().unwrap()["user_turn_index"].as_i64().unwrap())
            .collect::<Vec<_>>();
        rounds.sort();
        assert_eq!(rounds, (1..=8).collect::<Vec<_>>());
    }
    #[test]
    fn context_preserves_tool_messages_and_compaction_markers_without_diagnostics() {
        use crate::storage::{ConversationLogStore, ThreadLogStore};
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let accepted = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let id = accepted["turn_id"].as_str().unwrap();
        let tool = json!({"session_id":"thread","turn_id":id,"user_round":1,
            "item_id":"result-message","role":"tool","tool_call_id":"call-1","content":"result"});
        let marker = json!({"session_id":"thread","turn_id":id,"user_round":1,
            "item_id":"context-marker","role":"system","content":"",
            "meta":{"hidden":true,"type":"microcompaction","replacement_history":[]}});
        db.append_chat("owner", &tool).unwrap();
        db.append_chat("owner", &marker).unwrap();
        db.append_thread_item_impl(
            "owner",
            &json!({"session_id":"thread","turn_id":id,
            "item_id":"diagnostic","kind":"model_call","content":"diagnostic"}),
        )
        .unwrap();
        let model = db
            .load_thread_context_items("owner", "thread", 0, true)
            .unwrap();
        assert_eq!(model.len(), 3);
        assert_eq!(model[1]["tool_call_id"], "call-1");
        assert_eq!(model[2]["meta"], marker["meta"]);
        let last = db
            .load_thread_context_items("owner", "thread", 1, true)
            .unwrap();
        assert_eq!(last[0]["item_id"], "context-marker");
        let visible = db
            .load_thread_context_items("owner", "thread", 0, false)
            .unwrap();
        assert_eq!(visible.len(), 2);
    }
    #[test]
    fn duplicate_items_are_idempotent_and_changes_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let root = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let id = root["turn_id"].as_str().unwrap();
        let item = json!({"session_id":"thread","turn_id":id,"item_id":"answer","kind":"assistant_message","content":"text"});
        db.append_thread_item_impl("owner", &item).unwrap();
        db.append_thread_item_impl("owner", &item).unwrap();
        let turn = db
            .get_thread_turn_impl("owner", "thread", id, -1, 100, true)
            .unwrap()
            .unwrap();
        assert_eq!(turn["items"][1]["revision"], 1);
        for i in 0..4100 {
            let mut item = item.clone();
            item["content"] = json!(i);
            db.append_thread_item_impl("owner", &item).unwrap();
        }
        assert_eq!(
            db.list_thread_changes_impl("owner", "thread", 1, 100)
                .unwrap()[0]["change_type"],
            "snapshot_required"
        );
        let conn = db.open().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM thread_log_changes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 4096);
    }

    #[test]
    fn committed_item_receipt_matches_durable_revision_and_cursor() {
        use crate::storage::ThreadLogStore;
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let root = db.accept_thread_turn("owner", "thread", &input(1)).unwrap();
        let mut item = json!({"session_id":"thread","turn_id":root["turn_id"],
            "item_id":"receipt-item","kind":"tool_call","status":"running"});
        let first = db.commit_thread_item("owner", &item).unwrap().unwrap();
        assert_eq!(first["revision"], 1);
        assert!(db.commit_thread_item("owner", &item).unwrap().is_none());
        item["status"] = json!("completed");
        let second = db.commit_thread_item("owner", &item).unwrap().unwrap();
        assert_eq!(second["revision"], 2);
        assert_eq!(
            second["cursor"].as_i64().unwrap(),
            first["cursor"].as_i64().unwrap() + 1
        );
        let changes = db
            .list_thread_changes("owner", "thread", first["cursor"].as_i64().unwrap(), 10)
            .unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0]["revision"], second["revision"]);
        // The first change keeps its commit-time row even after the current
        // item was updated. Replaying from its cursor must not re-read the
        // mutable thread_items row and turn a historical running state into
        // the later completed state.
        let first_change = db
            .list_thread_changes("owner", "thread", first["cursor"].as_i64().unwrap() - 1, 1)
            .unwrap()
            .pop()
            .expect("first item change");
        assert_eq!(first_change["revision"], 1);
        assert_eq!(first_change["payload"]["status"], "running");
        assert_eq!(first_change["payload"]["payload"]["status"], "running");
        assert!(db.commit_thread_item("other", &item).is_err());
        assert_eq!(
            db.list_thread_changes("owner", "thread", first["cursor"].as_i64().unwrap(), 10)
                .unwrap(),
            changes
        );
    }
    #[test]
    fn item_identity_cannot_move_between_turns() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let first = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let second = db
            .accept_thread_turn_impl("owner", "thread", &input(2))
            .unwrap();
        let mut item = json!({"session_id":"thread", "turn_id":first["turn_id"], "item_id":"stable-item", "kind":"tool_message", "content":"result"});
        let receipt = db.append_thread_item_impl("owner", &item).unwrap().unwrap();
        item["turn_id"] = second["turn_id"].clone();
        assert!(db.append_thread_item_impl("owner", &item).is_err());
        assert!(db
            .list_thread_changes_impl("owner", "thread", receipt["cursor"].as_i64().unwrap(), 10)
            .unwrap()
            .is_empty());
        let original = db
            .get_thread_turn_impl(
                "owner",
                "thread",
                first["turn_id"].as_str().unwrap(),
                -1,
                10,
                true,
            )
            .unwrap()
            .unwrap();
        assert_eq!(original["items"][1]["revision"], 1);
        assert_eq!(original["items"][1]["payload"]["turn_id"], first["turn_id"]);
    }

    #[test]
    fn continuation_preserves_totals_and_context_execution_order() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        db.accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let second = db
            .accept_thread_turn_impl("owner", "thread", &input(2))
            .unwrap();
        let continuation = db
            .accept_thread_turn_impl(
                "owner",
                "thread",
                &json!({"root_user_round":1,"role":"user","content":"resume"}),
            )
            .unwrap();
        let conn = db.open().unwrap();
        conn.execute("UPDATE thread_items SET created_time=1", [])
            .unwrap();
        conn.execute(
            "UPDATE thread_items SET created_time=2 WHERE turn_id=?",
            params![second["turn_id"].as_str().unwrap()],
        )
        .unwrap();
        conn.execute(
            "UPDATE thread_items SET created_time=3 WHERE turn_id=?",
            params![continuation["turn_id"].as_str().unwrap()],
        )
        .unwrap();
        let context = db
            .load_thread_context_items_impl("owner", "thread", 2, true, None)
            .unwrap();
        assert_eq!(context.len(), 2);
        assert_eq!(context[0]["content"], "message 2");
        assert_eq!(context[1]["content"], "resume");
        assert!(db
            .load_thread_context_items_impl("other", "thread", 0, true, None)
            .unwrap()
            .is_empty());
        let total: f64 = conn
            .query_row("SELECT metric_value FROM thread_log_metrics", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(total, 2.0);
        db.delete_thread_log_by_session_impl("owner", "thread")
            .unwrap();
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM thread_log_metrics", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn execution_context_excludes_pending_inputs_but_keeps_identical_prior_message() {
        use crate::storage::ThreadLogStore;
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let previous = db.accept_thread_turn("owner", "thread", &input(1)).unwrap();
        db.update_thread_turn(
            "owner",
            "thread",
            previous["turn_id"].as_str().unwrap(),
            "completed",
            "",
            &json!({}),
        )
        .unwrap();
        let current = db
            .accept_thread_turn(
                "owner",
                "thread",
                &json!({"role":"user", "content":"message 1", "client_message_id":"current"}),
            )
            .unwrap();
        let id = current["turn_id"].as_str().unwrap();
        db.update_thread_turn("owner", "thread", id, "running", "", &json!({}))
            .unwrap();
        db.accept_thread_turn("owner", "thread", &input(3)).unwrap();
        let continuation = db
            .accept_thread_turn(
                "owner",
                "thread",
                &json!({"root_user_round":1, "role":"user", "content":"pending continuation"}),
            )
            .unwrap();
        let context = db
            .load_thread_execution_context("owner", "thread", id, 0)
            .unwrap();
        assert_eq!(context.len(), 1);
        assert_eq!(context[0]["content"], "message 1");
        assert_eq!(context[0]["turn_id"], previous["turn_id"]);
        assert_eq!(
            db.load_thread_context_items("owner", "thread", 0, true)
                .unwrap()
                .len(),
            4
        );
        assert!(db
            .load_thread_execution_context("other", "thread", id, 0)
            .unwrap()
            .is_empty());
        let resume_id = continuation["turn_id"].as_str().unwrap();
        db.update_thread_turn("owner", "thread", resume_id, "running", "", &json!({}))
            .unwrap();
        let resumed = db
            .load_thread_execution_context("owner", "thread", resume_id, 0)
            .unwrap();
        assert!(!resumed
            .iter()
            .any(|item| item["content"] == "pending continuation"));
    }

    #[test]
    fn context_sequence_survives_clock_ties_updates_and_reopen() {
        use crate::storage::ThreadLogStore;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.db").to_string_lossy().into_owned();
        let db = SqliteStorage::new(path.clone());
        let first = db.accept_thread_turn("owner", "thread", &input(1)).unwrap();
        db.accept_thread_turn("owner", "thread", &input(2)).unwrap();
        db.accept_thread_turn(
            "owner",
            "thread",
            &json!({"root_user_round":1,"role":"user","content":"resume"}),
        )
        .unwrap();
        let mut item = json!({"session_id":"thread","turn_id":first["turn_id"],"item_id":"late-result","kind":"assistant_message","role":"assistant","content":"late"});
        db.append_thread_item("owner", &item).unwrap();
        db.open()
            .unwrap()
            .execute("UPDATE thread_items SET created_time=1", [])
            .unwrap();
        item["content"] = json!("revised");
        db.append_thread_item("owner", &item).unwrap();
        drop(db);
        let reopened = SqliteStorage::new(path);
        let rows = reopened
            .load_thread_context_items("owner", "thread", 0, true)
            .unwrap();
        let contents: Vec<_> = rows
            .iter()
            .map(|row| row["content"].as_str().unwrap())
            .collect();
        assert_eq!(
            contents,
            vec!["message 1", "message 2", "resume", "revised"]
        );
        let latest = reopened
            .load_thread_context_items("owner", "thread", 2, true)
            .unwrap();
        assert_eq!(latest[0]["content"], "resume");
        assert_eq!(latest[1]["content"], "revised");
    }

    #[test]
    fn lifecycle_changes_publish_input_revision_and_leave_internal_messages_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let accepted = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let id = accepted["turn_id"].as_str().unwrap();
        db.append_thread_item_impl("owner", &json!({"session_id":"thread","turn_id":id,"item_id":"internal","kind":"user_message","visibility":"model_internal","status":"completed","content":"observation"})).unwrap();
        db.update_thread_turn_impl("owner", "thread", id, "running", "", &json!({}))
            .unwrap();
        let running = db
            .list_thread_changes_impl("owner", "thread", 3, 10)
            .unwrap();
        assert_eq!(running.len(), 2);
        assert_eq!(running[1]["revision"], 2);
        assert_eq!(running[1]["item_id"], format!("{id}:user"));
        db.update_thread_turn_impl("owner", "thread", id, "interrupted", "", &json!({}))
            .unwrap();
        let terminal = db
            .list_thread_changes_impl("owner", "thread", 5, 10)
            .unwrap();
        assert_eq!(terminal.len(), 2);
        assert_eq!(terminal[1]["revision"], 3);
        assert!(
            terminal[0]["revision"].as_i64().unwrap() > running[0]["revision"].as_i64().unwrap()
        );
        let detail = db
            .get_thread_turn_impl("owner", "thread", id, -1, 10, true)
            .unwrap()
            .unwrap();
        assert_eq!(detail["items"][0]["status"], "completed");
        assert_eq!(detail["items"][1]["revision"], 1);
        db.update_thread_turn_impl("owner", "thread", id, "running", "", &json!({}))
            .unwrap();
        assert!(db
            .list_thread_changes_impl("owner", "thread", 7, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn fork_copies_complete_graph_and_rejects_existing_target_atomically() {
        use crate::storage::ThreadLogStore;
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("log.db").to_string_lossy().into_owned());
        let root = db.accept_thread_turn("owner", "source", &input(1)).unwrap();
        let id = root["turn_id"].as_str().unwrap();
        for index in 0..125 {
            db.append_thread_item("owner", &json!({"session_id":"source","turn_id":id,"item_id":format!("tool-{index}"),"kind":"tool_message","visibility":"model_internal","role":"tool","content":"result"})).unwrap();
        }
        let continuation = db
            .accept_thread_turn(
                "owner",
                "source",
                &json!({"root_user_round":1,"role":"user","content":"resume"}),
            )
            .unwrap();
        db.update_thread_turn("owner", "source", id, "completed", "", &json!({}))
            .unwrap();
        db.upsert_thread_text_block(
            "owner",
            "source",
            &json!({"item_id":"tool-124","block_index":0,"event_id":1,"content":"block"}),
        )
        .unwrap();
        db.accept_thread_turn("owner", "source", &input(2)).unwrap();
        assert!(db.fork_thread_log("other", "source", "denied", 1).is_err());
        db.fork_thread_log("owner", "source", "branch", 1).unwrap();
        let rows = db
            .load_thread_context_items("owner", "branch", 0, true)
            .unwrap();
        assert_eq!(rows.len(), 127);
        assert!(rows.iter().all(|item| item["session_id"] == "branch"));
        let detail = db
            .get_thread_turn("owner", "branch", id, -1, 1, true)
            .unwrap()
            .unwrap();
        assert_eq!(detail["status"], "completed");
        let resume = db
            .get_thread_turn(
                "owner",
                "branch",
                continuation["turn_id"].as_str().unwrap(),
                -1,
                1,
                true,
            )
            .unwrap()
            .unwrap();
        assert_eq!(resume["root_turn_id"], root["turn_id"]);
        assert_eq!(
            db.list_thread_item_blocks("owner", "branch", "tool-124", 0, 10, true)
                .unwrap()[0]["content"],
            "block"
        );
        assert!(db
            .list_thread_item_blocks("owner", "branch", "tool-124", 0, 10, false)
            .unwrap()
            .is_empty());
        assert!(db.fork_thread_log("owner", "source", "branch", 2).is_err());
        assert_eq!(
            db.load_thread_context_items("owner", "branch", 0, true)
                .unwrap(),
            rows
        );
        assert_eq!(
            db.accept_thread_turn("owner", "branch", &input(3)).unwrap()["user_turn_index"],
            2
        );
    }

    #[test]
    fn visible_message_projection_is_cursor_paginated_and_hides_internal_items() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("visible.db").to_string_lossy().into_owned());
        let accepted = db
            .accept_thread_turn_impl("owner", "thread", &input(1))
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        for i in 0..8 {
            db.append_thread_item_impl("owner", &json!({"session_id":"thread","turn_id":turn,"item_id":format!("visible-{i}"),"kind":"assistant_message","content":format!("v{i}")})).unwrap();
        }
        db.append_thread_item_impl("owner", &json!({"session_id":"thread","turn_id":turn,"item_id":"hidden","kind":"assistant_message","visibility":"model_internal","content":"secret"})).unwrap();
        let first = db
            .list_thread_visible_messages_impl("owner", "thread", None, 4)
            .unwrap();
        assert_eq!(first.len(), 4);
        assert!(first.iter().all(|row| row["content"] != "secret"));
        let cursor = first.last().unwrap()["created_seq"].as_i64().unwrap();
        let second = db
            .list_thread_visible_messages_impl("owner", "thread", Some(cursor), 20)
            .unwrap();
        assert!(second
            .iter()
            .all(|row| row["created_seq"].as_i64().unwrap() < cursor));
        assert_eq!(second.len(), 5);
    }

    #[test]
    fn item_blocks_are_field_filtered_paginated_and_require_item_identity() {
        use crate::storage::ThreadLogStore;
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("blocks.db").to_string_lossy().into_owned());
        let accepted = db.accept_thread_turn("owner", "thread", &input(1)).unwrap();
        let turn_id = accepted["turn_id"].as_str().unwrap();
        db.append_thread_item(
            "owner",
            &json!({
                "session_id":"thread", "turn_id":turn_id, "item_id":"answer",
                "kind":"assistant_message", "visibility":"user", "role":"assistant",
                "content":""
            }),
        )
        .unwrap();
        for index in 0..3 {
            db.upsert_thread_text_block(
                "owner",
                "thread",
                &json!({
                    "item_id":"answer", "block_index":index, "event_id":index + 1,
                    "field":"content", "content":format!("c{index}")
                }),
            )
            .unwrap();
        }
        db.upsert_thread_text_block(
            "owner",
            "thread",
            &json!({
                "item_id":"answer", "block_index":0, "event_id":10,
                "field":"reasoning", "reasoning":"thought"
            }),
        )
        .unwrap();
        let first = db
            .list_thread_item_blocks_page("owner", "thread", "answer", Some("content"), 0, 2, false)
            .unwrap();
        assert_eq!(first.0.len(), 2);
        assert_eq!(first.1, Some(1));
        assert!(first.2);
        let second = db
            .list_thread_item_blocks_page("owner", "thread", "answer", Some("content"), 2, 2, false)
            .unwrap();
        assert_eq!(second.0.len(), 1);
        assert!(!second.2);
        let reasoning = db
            .list_thread_item_blocks_page(
                "owner",
                "thread",
                "answer",
                Some("reasoning"),
                0,
                2,
                false,
            )
            .unwrap();
        assert_eq!(reasoning.0.len(), 1);
        assert!(db
            .upsert_thread_text_block(
                "owner",
                "thread",
                &json!({"item_id":"orphan", "block_index":0, "event_id":99, "content":"x"})
            )
            .is_err());
        assert!(db
            .list_thread_item_blocks_page("owner", "thread", "orphan", None, 0, 2, false)
            .unwrap()
            .0
            .is_empty());
        db.append_thread_item(
            "owner",
            &json!({
                "session_id":"thread", "turn_id":turn_id, "item_id":"hidden-answer",
                "kind":"assistant_message", "visibility":"model_internal", "role":"assistant"
            }),
        )
        .unwrap();
        db.upsert_thread_text_block("owner", "thread", &json!({"item_id":"hidden-answer", "block_index":0, "event_id":100, "content":"secret"})).unwrap();
        assert!(db
            .list_thread_item_blocks_page("owner", "thread", "hidden-answer", None, 0, 2, false)
            .unwrap()
            .0
            .is_empty());
        assert_eq!(
            db.list_thread_item_blocks_page("owner", "thread", "hidden-answer", None, 0, 2, true)
                .unwrap()
                .0
                .len(),
            1
        );
    }
}
