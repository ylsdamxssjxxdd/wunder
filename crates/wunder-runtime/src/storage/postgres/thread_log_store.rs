use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use uuid::Uuid;
pub(super) trait PostgresThreadLogStorage {
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
impl PostgresThreadLogStorage for PostgresStorage {
    fn upsert_thread_text_block_impl(
        &self,
        user_id: &str,
        session_id: &str,
        block: &Value,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let item_id = block["item_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing text item"))?;
        let index = block["block_index"].as_i64().unwrap_or(0);
        let event_id = block["event_id"].as_i64().unwrap_or(0);
        let text = serde_json::to_string(block)?;
        conn.execute("INSERT INTO thread_item_blocks(session_id,user_id,item_id,block_index,event_id,payload) SELECT $1,$2,$3,$4,$5,$6 WHERE EXISTS(SELECT 1 FROM thread_logs WHERE session_id=$1 AND user_id=$2) ON CONFLICT(session_id,item_id,block_index) DO UPDATE SET event_id=excluded.event_id,payload=excluded.payload WHERE thread_item_blocks.event_id<=excluded.event_id",&[&session_id,&user_id,&item_id,&index,&event_id,&text])?;
        Ok(())
    }
    fn list_thread_text_blocks_impl(
        &self,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows=conn.query("SELECT payload FROM thread_item_blocks WHERE session_id=$1 AND event_id>$2 ORDER BY event_id LIMIT $3",&[&session_id,&after,&limit.clamp(1,500)])?;
        Ok(rows
            .into_iter()
            .filter_map(|r| serde_json::from_str(&r.get::<_, String>(0)).ok())
            .collect())
    }

    fn find_thread_turn_id_impl(
        &self,
        user_id: &str,
        session_id: &str,
        user_turn_index: i64,
    ) -> Result<Option<String>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        Ok(conn.query_opt("SELECT turn_id FROM thread_turns WHERE user_id=$1 AND session_id=$2 AND user_turn_index=$3 AND trigger_kind='user'", &[&user_id,&session_id,&user_turn_index])?.map(|r| r.get(0)))
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
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        tx.execute("INSERT INTO thread_logs(session_id,user_id,created_time,updated_time) VALUES($1,$2,$3,$4) ON CONFLICT(session_id) DO NOTHING", &[&session_id,&user_id,&now,&now])?;
        let owner: Option<String> = tx
            .query_opt(
                "SELECT user_id FROM thread_logs WHERE session_id=$1 FOR UPDATE",
                &[&session_id],
            )?
            .map(|r| r.get(0));
        ensure!(owner.as_deref() == Some(user_id), "thread owner mismatch");
        let client_id = input
            .get("client_message_id")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty());
        if let Some(client_id) = client_id {
            let existing: Option<String> = tx
                .query_opt(
                    "SELECT turn_id FROM thread_turns WHERE session_id=$1 AND client_message_id=$2",
                    &[&session_id, &client_id],
                )?
                .map(|r| r.get(0));
            if let Some(turn_id) = existing {
                let round:i64=tx.query_one("SELECT user_turn_index FROM thread_turns WHERE session_id=$1 AND turn_id=$2", &[&session_id,&turn_id])?.get(0);
                tx.commit()?;
                return Ok(json!({"turn_id":turn_id,"user_turn_index":round,"created":false}));
            }
        }
        let turn_id = Uuid::new_v4().to_string();
        let parent = input.get("root_user_round").and_then(Value::as_i64);
        let root: Option<String> = if let Some(round) = parent {
            tx.query_opt("SELECT turn_id FROM thread_turns WHERE session_id=$1 AND user_turn_index=$2 AND trigger_kind='user'", &[&session_id,&round])?.map(|r| r.get(0))
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
                "UPDATE thread_logs SET latest_user_turn=latest_user_turn+1 WHERE session_id=$1",
                &[&session_id],
            )?;
            tx.query_one(
                "SELECT latest_user_turn FROM thread_logs WHERE session_id=$1",
                &[&session_id],
            )?
            .get(0)
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
        tx.execute("INSERT INTO thread_turns(session_id,turn_id,root_turn_id,trigger_kind,client_message_id,user_id,user_turn_index,status,summary,payload,created_time,updated_time) VALUES($1,$2,$3,$4,$5,$6,$7,'queued',$8,'{}',$9,$10)", &[&session_id,&turn_id,&root_id,&trigger,&client_id,&user_id,&round,&summary,&now,&now])?;
        let index:i64=tx.query_one("SELECT COALESCE(MAX(item_index),-1)+1 FROM thread_items WHERE session_id=$1 AND root_turn_id=$2", &[&session_id,&root_id])?.get(0);
        let visibility = if trigger == "user" {
            "user"
        } else {
            "model_internal"
        };
        tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time) VALUES($1,$2,$3,$4,$5,$6,$7,'user_message','completed',$8,$9,$10)", &[&session_id,&item_id,&turn_id,&root_id,&visibility,&user_id,&index,&text,&now,&now])?;
        let change_type = "turn_upsert";
        let change_item: Option<&str> = None;
        let revision = 1i64;
        let seq: i64 = tx
            .query_one(
                "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=$1",
                &[&session_id],
            )?
            .get(0);
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES($1,$2,$3,$4,$5,$6,$7,'{}',$8)", &[&session_id,&seq,&user_id,&change_type,&turn_id,&change_item,&revision,&now])?;
        tx.execute(
            "UPDATE thread_logs SET latest_change_seq=$1,updated_time=$2 WHERE session_id=$3",
            &[&seq, &now, &session_id],
        )?;
        // Changes are a bounded recovery index; the durable items are never pruned here.
        tx.execute(
            "DELETE FROM thread_log_changes WHERE session_id=$1 AND change_seq<=$2",
            &[&session_id, &seq.saturating_sub(4096)],
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
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let owner: Option<String> = tx
            .query_opt(
                "SELECT user_id FROM thread_logs WHERE session_id=$1 FOR UPDATE",
                &[&session_id],
            )?
            .map(|r| r.get(0));
        ensure!(owner.as_deref() == Some(user_id), "thread owner mismatch");
        let mut payload = payload.clone();
        if let Some(map) = payload.as_object_mut() {
            map.remove("answer");
            map.remove("summary");
        }
        let text = serde_json::to_string(&payload)?;
        let changed=tx.execute("UPDATE thread_turns SET status=$1,payload=$2,updated_time=$3 WHERE session_id=$4 AND turn_id=$5 AND status IN ('queued','running','waiting_input') AND (status<>$6 OR payload<>$7)", &[&status,&text,&now,&session_id,&turn_id,&status,&text])?;
        if changed == 0 {
            tx.commit()?;
            return Ok(());
        }
        let change_type = "turn_upsert";
        let change_item: Option<&str> = None;
        let revision = 1i64;
        let seq: i64 = tx
            .query_one(
                "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=$1",
                &[&session_id],
            )?
            .get(0);
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES($1,$2,$3,$4,$5,$6,$7,'{}',$8)", &[&session_id,&seq,&user_id,&change_type,&turn_id,&change_item,&revision,&now])?;
        tx.execute(
            "UPDATE thread_logs SET latest_change_seq=$1,updated_time=$2 WHERE session_id=$3",
            &[&seq, &now, &session_id],
        )?;
        // Changes are a bounded recovery index; the durable items are never pruned here.
        tx.execute(
            "DELETE FROM thread_log_changes WHERE session_id=$1 AND change_seq<=$2",
            &[&session_id, &seq.saturating_sub(4096)],
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
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let owner: Option<String> = tx
            .query_opt(
                "SELECT user_id FROM thread_logs WHERE session_id=$1 FOR UPDATE",
                &[&session_id],
            )?
            .map(|r| r.get(0));
        ensure!(owner.as_deref() == Some(user_id), "thread owner mismatch");
        let root_id: Option<String> = tx
            .query_opt(
                "SELECT root_turn_id FROM thread_turns WHERE session_id=$1 AND turn_id=$2",
                &[&session_id, &turn_id],
            )?
            .map(|r| r.get(0));
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
            .query_opt(
                "SELECT payload FROM thread_items WHERE session_id=$1 AND item_id=$2",
                &[&session_id, &item_id],
            )?
            .map(|r| r.get(0));
        if existing.as_deref() == Some(text.as_str()) {
            tx.commit()?;
            return Ok(());
        }
        let index:i64=tx.query_one("SELECT COALESCE(MAX(item_index),-1)+1 FROM thread_items WHERE session_id=$1 AND root_turn_id=$2", &[&session_id,&root_id])?.get(0);
        tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) ON CONFLICT(session_id,item_id) DO UPDATE SET kind=excluded.kind,status=excluded.status,visibility=excluded.visibility,payload=excluded.payload,revision=thread_items.revision+1,updated_time=excluded.updated_time WHERE thread_items.turn_id=excluded.turn_id", &[&session_id,&item_id,&turn_id,&root_id,&visibility,&user_id,&index,&kind,&status,&text,&now,&now])?;
        let revision: i64 = tx
            .query_one(
                "SELECT revision FROM thread_items WHERE session_id=$1 AND item_id=$2",
                &[&session_id, &item_id],
            )?
            .get(0);
        let change_type = "item_upsert";
        let change_item = Some(item_id);
        let seq: i64 = tx
            .query_one(
                "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=$1",
                &[&session_id],
            )?
            .get(0);
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES($1,$2,$3,$4,$5,$6,$7,'{}',$8)", &[&session_id,&seq,&user_id,&change_type,&turn_id,&change_item,&revision,&now])?;
        tx.execute(
            "UPDATE thread_logs SET latest_change_seq=$1,updated_time=$2 WHERE session_id=$3",
            &[&seq, &now, &session_id],
        )?;
        // Changes are a bounded recovery index; the durable items are never pruned here.
        tx.execute(
            "DELETE FROM thread_log_changes WHERE session_id=$1 AND change_seq<=$2",
            &[&session_id, &seq.saturating_sub(4096)],
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
        let mut conn = self.conn()?;
        let before = before.unwrap_or(i64::MAX);
        let limit = limit.clamp(1, 101);
        Ok(conn.query("SELECT turn_id,user_turn_index,status,summary,payload,updated_time,root_turn_id,trigger_kind FROM thread_turns WHERE user_id=$1 AND session_id=$2 AND user_turn_index<$3 AND trigger_kind='user' ORDER BY user_turn_index DESC LIMIT $4", &[&user_id,&session_id,&before,&limit])?.into_iter().map(turn_row).collect::<Vec<_>>())
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
        let mut conn = self.conn()?;
        let limit = limit.clamp(1, 100);
        let fetch = limit + 1;
        let mut turns=conn.query("SELECT turn_id,user_turn_index,status,summary,payload,updated_time,root_turn_id,trigger_kind FROM thread_turns WHERE user_id=$1 AND session_id=$2 AND turn_id=$3", &[&user_id,&session_id,&turn_id])?.into_iter().map(turn_row).collect::<Vec<_>>();
        let Some(mut turn) = turns.pop() else {
            return Ok(None);
        };
        let root_id = turn["root_turn_id"].as_str().unwrap_or(turn_id);
        let mut items=conn.query("SELECT item_id,item_index,kind,status,revision,payload,created_time,updated_time,turn_id,visibility FROM thread_items WHERE user_id=$1 AND session_id=$2 AND root_turn_id=$3 AND item_index>$4 AND ($5 OR visibility='user') ORDER BY item_index LIMIT $6", &[&user_id,&session_id,&root_id,&after,&include_internal,&fetch])?.into_iter().map(item_row).collect::<Vec<_>>();
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
        let mut conn = self.conn()?;
        let limit = limit.clamp(1, 500);
        let after = after.max(0);
        let earliest: Option<i64> = conn
            .query_one(
                "SELECT MIN(change_seq) FROM thread_log_changes WHERE user_id=$1 AND session_id=$2",
                &[&user_id, &session_id],
            )?
            .get(0);
        if earliest.is_some_and(|first| after > 0 && first > after + 1) {
            return Ok(vec![json!({"change_type":"snapshot_required"})]);
        }
        Ok(conn.query("SELECT change_seq,change_type,turn_id,item_id,revision,payload,created_time FROM thread_log_changes WHERE user_id=$1 AND session_id=$2 AND change_seq>$3 ORDER BY change_seq LIMIT $4", &[&user_id,&session_id,&after,&limit])?.into_iter().map(change_row).collect::<Vec<_>>())
    }
    fn delete_thread_log_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let mut count = 0i64;
        count += tx.execute(
            "DELETE FROM thread_item_blocks WHERE user_id=$1 AND session_id=$2",
            &[&user_id, &session_id],
        )? as i64;
        count += tx.execute(
            "DELETE FROM thread_log_changes WHERE user_id=$1 AND session_id=$2",
            &[&user_id, &session_id],
        )? as i64;
        count += tx.execute(
            "DELETE FROM thread_items WHERE user_id=$1 AND session_id=$2",
            &[&user_id, &session_id],
        )? as i64;
        count += tx.execute(
            "DELETE FROM thread_turns WHERE user_id=$1 AND session_id=$2",
            &[&user_id, &session_id],
        )? as i64;
        count += tx.execute(
            "DELETE FROM thread_logs WHERE user_id=$1 AND session_id=$2",
            &[&user_id, &session_id],
        )? as i64;
        tx.commit()?;
        Ok(count)
    }
}
fn turn_row(r: tokio_postgres::Row) -> Value {
    json!({"turn_id":r.get::<_,String>(0),"user_turn_index":r.get::<_,i64>(1),"status":r.get::<_,String>(2),"summary":r.get::<_,String>(3),"payload":serde_json::from_str::<Value>(&r.get::<_,String>(4)).unwrap_or(Value::Null),"updated_time":r.get::<_,f64>(5),"root_turn_id":r.get::<_,String>(6),"trigger_kind":r.get::<_,String>(7)})
}
fn item_row(r: tokio_postgres::Row) -> Value {
    json!({"item_id":r.get::<_,String>(0),"item_index":r.get::<_,i64>(1),"kind":r.get::<_,String>(2),"status":r.get::<_,String>(3),"revision":r.get::<_,i64>(4),"payload":serde_json::from_str::<Value>(&r.get::<_,String>(5)).unwrap_or(Value::Null),"created_time":r.get::<_,f64>(6),"updated_time":r.get::<_,f64>(7),"turn_id":r.get::<_,String>(8),"visibility":r.get::<_,String>(9)})
}
fn change_row(r: tokio_postgres::Row) -> Value {
    json!({"change_seq":r.get::<_,i64>(0),"change_type":r.get::<_,String>(1),"turn_id":r.get::<_,String>(2),"item_id":r.get::<_,Option<String>>(3),"revision":r.get::<_,i64>(4),"payload":serde_json::from_str::<Value>(&r.get::<_,String>(5)).unwrap_or(Value::Null),"created_time":r.get::<_,f64>(6)})
}
