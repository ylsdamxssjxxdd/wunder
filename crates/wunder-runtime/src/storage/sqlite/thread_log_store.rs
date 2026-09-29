use super::SqliteStorage;
use crate::storage::StorageLifecycle;
use anyhow::{ensure, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use uuid::Uuid;
pub(super) trait SqliteThreadLogStorage {
    fn upsert_thread_text_block_impl(
        &self,
        user_id: &str,
        session_id: &str,
        block: &Value,
    ) -> Result<()>;
    fn list_thread_text_blocks_impl(
        &self,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;

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
    ) -> Result<()>;
    fn delete_thread_log_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn append_thread_item_impl(&self, user_id: &str, payload: &Value) -> Result<()>;
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
    fn get_thread_turn_impl(
        &self,
        user_id: &str,
        session_id: &str,
        turn_id: &str,
        after: i64,
        limit: i64,
        include_internal: bool,
    ) -> Result<Option<Value>>;
    fn list_thread_changes_impl(
        &self,
        user_id: &str,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
}
impl SqliteThreadLogStorage for SqliteStorage {
    fn upsert_thread_text_block_impl(
        &self,
        user_id: &str,
        session_id: &str,
        block: &Value,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let item_id = block["item_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing text item"))?;
        let index = block["block_index"].as_i64().unwrap_or(0);
        let event_id = block["event_id"].as_i64().unwrap_or(0);
        let text = serde_json::to_string(block)?;
        conn.execute("INSERT INTO thread_item_blocks(session_id,user_id,item_id,block_index,event_id,payload) SELECT ?,?,?,?,?,? WHERE EXISTS(SELECT 1 FROM thread_logs WHERE session_id=? AND user_id=?) ON CONFLICT(session_id,item_id,block_index) DO UPDATE SET event_id=excluded.event_id,payload=excluded.payload WHERE thread_item_blocks.event_id<=excluded.event_id",params![session_id,user_id,item_id,index,event_id,text,session_id,user_id])?;
        Ok(())
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
        tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time) VALUES(?,?,?,?,?,?,?,'user_message','completed',?,?,?)", params![session_id,item_id,turn_id,root_id,visibility,user_id,index,text,now,now])?;
        let change_type = "turn_upsert";
        let change_item: Option<&str> = None;
        let revision = 1i64;
        let seq: i64 = tx.query_row(
            "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=?",
            params![session_id],
            |r| r.get(0),
        )?;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,?,?,?,?,'{}',?)", params![session_id,seq,user_id,change_type,turn_id,change_item,revision,now])?;
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
    ) -> Result<()> {
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
            return Ok(());
        }
        let change_type = "turn_upsert";
        let change_item: Option<&str> = None;
        let revision = 1i64;
        let seq: i64 = tx.query_row(
            "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=?",
            params![session_id],
            |r| r.get(0),
        )?;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,?,?,?,?,'{}',?)", params![session_id,seq,user_id,change_type,turn_id,change_item,revision,now])?;
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
        Ok(())
    }
    fn append_thread_item_impl(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.ensure_initialized()?;
        let session_id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let turn_id = payload.get("turn_id").and_then(Value::as_str).unwrap_or("");
        let item_id = payload.get("item_id").and_then(Value::as_str).unwrap_or("");
        // Non-turn configuration/history records are intentionally outside the timeline.
        if turn_id.is_empty() {
            return Ok(());
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
        let existing: Option<String> = tx
            .query_row(
                "SELECT payload FROM thread_items WHERE session_id=? AND item_id=?",
                params![session_id, item_id],
                |r| r.get(0),
            )
            .optional()?;
        if existing.as_deref() == Some(text.as_str()) {
            tx.commit()?;
            return Ok(());
        }
        let index:i64=tx.query_row("SELECT COALESCE(MAX(item_index),-1)+1 FROM thread_items WHERE session_id=? AND root_turn_id=?", params![session_id,root_id], |r| r.get(0))?;
        tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time) VALUES(?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(session_id,item_id) DO UPDATE SET kind=excluded.kind,status=excluded.status,visibility=excluded.visibility,payload=excluded.payload,revision=thread_items.revision+1,updated_time=excluded.updated_time WHERE thread_items.turn_id=excluded.turn_id", params![session_id,item_id,turn_id,root_id,visibility,user_id,index,kind,status,text,now,now])?;
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
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES(?,?,?,?,?,?,?,'{}',?)", params![session_id,seq,user_id,change_type,turn_id,change_item,revision,now])?;
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
        Ok(())
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
        if earliest.is_some_and(|first| after > 0 && first > after + 1) {
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
}
