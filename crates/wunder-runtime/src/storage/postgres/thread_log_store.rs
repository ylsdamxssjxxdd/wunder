use super::PostgresStorage;
use crate::storage::StorageLifecycle;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use uuid::Uuid;
pub(super) trait PostgresThreadLogStorage {
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
    ) -> Result<()>;
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
    ) -> Result<()>;
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
    fn list_thread_changes_impl(
        &self,
        user_id: &str,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>>;
}
impl PostgresThreadLogStorage for PostgresStorage {
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
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let owner: Option<String> = tx
            .query_opt(
                "SELECT user_id FROM thread_logs WHERE session_id=$1 FOR UPDATE",
                &[&source],
            )?
            .map(|r| r.get(0));
        ensure!(owner.as_deref() == Some(user_id), "thread owner mismatch");
        // The root INSERT rejects an existing destination, so retries cannot merge graphs.
        tx.execute("INSERT INTO thread_logs(session_id,user_id,latest_user_turn,latest_change_seq,created_time,updated_time) SELECT $3,user_id,COALESCE((SELECT MAX(user_turn_index) FROM thread_turns WHERE session_id=$2 AND trigger_kind='user' AND user_turn_index<=$4),0),latest_change_seq,created_time,updated_time FROM thread_logs WHERE user_id=$1 AND session_id=$2", &[&user_id,&source,&target,&through_round])?;
        tx.execute("INSERT INTO thread_turns(session_id,turn_id,user_id,root_turn_id,trigger_kind,client_message_id,user_turn_index,status,summary,payload,created_time,updated_time) SELECT $3,turn_id,user_id,root_turn_id,trigger_kind,client_message_id,user_turn_index,status,summary,jsonb_set(payload::jsonb, '{session_id}', to_jsonb($3::text), true)::text,created_time,updated_time FROM thread_turns WHERE user_id=$1 AND session_id=$2 AND user_turn_index<=$4", &[&user_id,&source,&target,&through_round])?;
        tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,revision,payload,created_time,updated_time,created_seq) SELECT $3,i.item_id,i.turn_id,i.root_turn_id,i.visibility,i.user_id,i.item_index,i.kind,i.status,i.revision,jsonb_set(i.payload::jsonb, '{session_id}', to_jsonb($3::text), true)::text,i.created_time,i.updated_time,i.created_seq FROM thread_items i JOIN thread_turns t ON t.session_id=i.session_id AND t.turn_id=i.turn_id WHERE i.user_id=$1 AND i.session_id=$2 AND t.user_turn_index<=$4", &[&user_id,&source,&target,&through_round])?;
        tx.execute("INSERT INTO thread_item_blocks(session_id,user_id,item_id,field,block_index,event_id,payload) SELECT $3,b.user_id,b.item_id,b.field,b.block_index,b.event_id,jsonb_set(b.payload::jsonb, '{session_id}', to_jsonb($3::text), true)::text FROM thread_item_blocks b JOIN thread_items i ON i.session_id=$3 AND i.item_id=b.item_id WHERE b.user_id=$1 AND b.session_id=$2", &[&user_id,&source,&target])?;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) SELECT $3,c.change_seq,c.user_id,c.change_type,c.turn_id,c.item_id,c.revision,c.payload,c.created_time FROM thread_log_changes c JOIN thread_turns t ON t.session_id=$3 AND t.turn_id=c.turn_id WHERE c.user_id=$1 AND c.session_id=$2", &[&user_id,&source,&target])?;
        tx.execute("INSERT INTO thread_log_metrics(session_id,user_id,metric_key,metric_value,updated_time) SELECT session_id,user_id,'user_turn_total',latest_user_turn,updated_time FROM thread_logs WHERE user_id=$1 AND session_id=$3 AND $2<>$3", &[&user_id,&source,&target])?;
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
        let mut conn = self.conn()?;
        let before = before_seq.unwrap_or(i64::MAX);
        Ok(conn.query("SELECT payload,created_seq FROM thread_items WHERE user_id=$1 AND session_id=$2 AND visibility='user' AND kind IN ('user_message','assistant_message') AND created_seq<$3 ORDER BY created_seq DESC LIMIT $4", &[&user_id,&session_id,&before,&limit.clamp(1,501)])?.into_iter().map(|row| { let mut value: Value=serde_json::from_str(row.get(0)).unwrap_or(Value::Null); if let Value::Object(map)=&mut value { map.insert("created_seq".into(), json!(row.get::<_,i64>(1))); } value }).collect())
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
        let mut conn = self.conn()?;
        let limit = (limit > 0).then_some(limit);
        let rows = conn.query("SELECT i.payload FROM thread_items i JOIN thread_turns t ON t.session_id=i.session_id AND t.turn_id=i.turn_id WHERE i.user_id=$1 AND i.session_id=$2 AND ($3 OR i.visibility='user') AND ($5::text IS NULL OR (t.status<>'queued' AND i.item_id<>$5 || ':user')) AND i.kind IN ('user_message','assistant_message','tool_message','system_message') ORDER BY i.created_seq DESC LIMIT $4", &[&user_id, &session_id, &include_internal, &limit, &executing_turn])?
            .into_iter().map(|row| row.get::<_, String>(0)).collect::<Vec<_>>();
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
    ) -> Result<()> {
        self.ensure_initialized()?;
        let mut client = self.conn()?;
        let mut conn = client.transaction()?;
        conn.query_opt(
            "SELECT user_id FROM thread_logs WHERE session_id=$1 FOR UPDATE",
            &[&session_id],
        )?;
        let item_id = block["item_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing text item"))?;
        let index = block["block_index"].as_i64().unwrap_or(0);
        let field = block["field"]
            .as_str()
            .or_else(|| block.pointer("/data/field").and_then(Value::as_str))
            .unwrap_or("content");
        let event_id = block["event_id"].as_i64().unwrap_or(0);
        let text = serde_json::to_string(block)?;
        let item_exists: bool = conn
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM thread_items WHERE session_id=$1 AND user_id=$2 AND item_id=$3)",
                &[&session_id, &user_id, &item_id],
            )?
            .get(0);
        anyhow::ensure!(item_exists, "thread item does not exist for block");
        let written = conn.execute("INSERT INTO thread_item_blocks(session_id,user_id,item_id,field,block_index,event_id,payload) VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(session_id,item_id,field,block_index) DO UPDATE SET event_id=excluded.event_id,payload=excluded.payload WHERE thread_item_blocks.event_id<=excluded.event_id AND thread_item_blocks.payload<>excluded.payload",&[&session_id,&user_id,&item_id,&field,&index,&event_id,&text])?;
        if written > 0 {
            let row = conn.query_one("SELECT l.latest_change_seq+1,i.turn_id FROM thread_logs l JOIN thread_items i ON i.session_id=l.session_id WHERE l.session_id=$1 AND i.item_id=$2", &[&session_id,&item_id])?;
            let seq: i64 = row.get(0);
            let turn_id: String = row.get(1);
            let reference = json!({"field":field,"block_index":index}).to_string();
            let now = Self::now_ts();
            conn.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES($1,$2,$3,'text_block',$4,$5,0,$6,$7)", &[&session_id,&seq,&user_id,&turn_id,&item_id,&reference,&now])?;
            conn.execute(
                "UPDATE thread_logs SET latest_change_seq=$1,updated_time=$2 WHERE session_id=$3",
                &[&seq, &now, &session_id],
            )?;
            conn.execute(
                "DELETE FROM thread_log_changes WHERE session_id=$1 AND change_seq<=$2",
                &[&session_id, &seq.saturating_sub(4096)],
            )?;
        }
        conn.commit()?;
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
        let mut conn = self.conn()?;
        let requested = limit.clamp(1, 100);
        let field = field.unwrap_or("content");
        let rows = conn.query("SELECT b.payload FROM thread_item_blocks b JOIN thread_items i ON i.session_id=b.session_id AND i.item_id=b.item_id WHERE b.user_id=$1 AND b.session_id=$2 AND b.item_id=$3 AND b.field=$4 AND b.block_index>=$5 AND ($6 OR i.visibility='user') ORDER BY b.block_index LIMIT $7", &[&user_id,&session_id,&item_id,&field,&from_block.max(0),&include_internal,&(requested+1)])?;
        let mut blocks: Vec<Value> = rows
            .into_iter()
            .filter_map(|row| serde_json::from_str(&row.get::<_, String>(0)).ok())
            .collect();
        let has_more = blocks.len() > requested as usize;
        blocks.truncate(requested as usize);
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
        tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq) VALUES($1,$2,$3,$4,$5,$6,$7,'user_message','completed',$8,$9,$10,(SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=$1))", &[&session_id,&item_id,&turn_id,&root_id,&visibility,&user_id,&index,&text,&now,&now])?;
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
        let item_seq = seq + 1;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES($1,$2,$3,$4,$5,$6,$7,'{}',$8)", &[&session_id,&item_seq,&user_id,&"item_upsert",&turn_id,&item_id,&revision,&now])?;
        tx.execute(
            "UPDATE thread_logs SET latest_change_seq=$1 WHERE session_id=$2",
            &[&item_seq, &session_id],
        )?;
        tx.execute(
            "INSERT INTO thread_log_metrics(session_id,user_id,metric_key,metric_value,updated_time) SELECT $1,$2,$3,latest_user_turn::double precision,$4 FROM thread_logs WHERE session_id=$1 ON CONFLICT(session_id,metric_key) DO UPDATE SET metric_value=EXCLUDED.metric_value,updated_time=EXCLUDED.updated_time",
            &[&session_id, &user_id, &"user_turn_total", &now],
        )?;
        // Changes are a bounded recovery index; the durable items are never pruned here.
        tx.execute(
            "DELETE FROM thread_log_changes WHERE session_id=$1 AND change_seq<=$2",
            &[&session_id, &item_seq.saturating_sub(4096)],
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
        // Keep the visible user bubble in sync with its durable turn.
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
            "UPDATE thread_items SET status=$1, payload=jsonb_set(payload::jsonb, '{status}', to_jsonb($1::text), true)::text, revision=revision+1, updated_time=$2 WHERE session_id=$3 AND item_id=$4 AND status<>$1",
            &[&bubble_status, &now, &session_id, &input_item_id],
        )?;
        let change_type = "turn_upsert";
        let change_item: Option<&str> = None;
        let mut seq: i64 = tx
            .query_one(
                "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=$1",
                &[&session_id],
            )?
            .get(0);
        let revision = seq;
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES($1,$2,$3,$4,$5,$6,$7,'{}',$8)", &[&session_id,&seq,&user_id,&change_type,&turn_id,&change_item,&revision,&now])?;
        if input_changed > 0 {
            seq += 1;
            tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) SELECT session_id,$1,user_id,'item_upsert',turn_id,item_id,revision,'{}',$2 FROM thread_items WHERE session_id=$3 AND item_id=$4", &[&seq,&now,&session_id,&input_item_id])?;
        }
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
        let existing: Option<(String, String)> = tx
            .query_opt(
                "SELECT turn_id,payload FROM thread_items WHERE session_id=$1 AND item_id=$2",
                &[&session_id, &item_id],
            )?
            .map(|r| (r.get(0), r.get(1)));
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
        let index:i64=tx.query_one("SELECT COALESCE(MAX(item_index),-1)+1 FROM thread_items WHERE session_id=$1 AND root_turn_id=$2", &[&session_id,&root_id])?.get(0);
        tx.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,visibility,user_id,item_index,kind,status,payload,created_time,updated_time,created_seq) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,(SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=$1)) ON CONFLICT(session_id,item_id) DO UPDATE SET kind=excluded.kind,status=excluded.status,visibility=excluded.visibility,payload=excluded.payload,revision=thread_items.revision+1,updated_time=excluded.updated_time WHERE thread_items.turn_id=excluded.turn_id", &[&session_id,&item_id,&turn_id,&root_id,&visibility,&user_id,&index,&kind,&status,&text,&now,&now])?;
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
        let mut conn = self.conn()?;
        let before = before.unwrap_or(i64::MAX);
        let limit = limit.clamp(1, 101);
        Ok(conn.query("SELECT turn_id,user_turn_index,status,summary,payload,updated_time,root_turn_id,trigger_kind FROM thread_turns WHERE user_id=$1 AND session_id=$2 AND user_turn_index<$3 AND trigger_kind='user' ORDER BY user_turn_index DESC LIMIT $4", &[&user_id,&session_id,&before,&limit])?.into_iter().map(turn_row).collect::<Vec<_>>())
    }
    fn get_thread_log_counts_impl(
        &self,
        user_id: &str,
        session_id: &str,
        include_internal: bool,
    ) -> Result<(i64, i64)> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let row = conn.query_one(
            "SELECT \
                (SELECT COUNT(*) FROM thread_turns WHERE user_id=$1 AND session_id=$2 AND trigger_kind='user'), \
                (SELECT COUNT(*) FROM thread_items WHERE user_id=$1 AND session_id=$2 AND ($3 OR visibility='user'))",
            &[&user_id, &session_id, &include_internal],
        )?;
        Ok((row.get(0), row.get(1)))
    }
    fn latest_thread_user_round_by_session_impl(&self, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        Ok(conn.query_one("SELECT COALESCE(MAX(user_turn_index),0) FROM thread_turns WHERE session_id=$1 AND trigger_kind='user'", &[&session_id])?.get(0))
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
    fn get_thread_item_impl(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        include_internal: bool,
    ) -> Result<Option<Value>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut items = conn.query("SELECT item_id,item_index,kind,status,revision,payload,created_time,updated_time,turn_id,visibility FROM thread_items WHERE user_id=$1 AND session_id=$2 AND item_id=$3 AND ($4 OR visibility='user') LIMIT 1", &[&user_id, &session_id, &item_id, &include_internal])?.into_iter().map(item_row).collect::<Vec<_>>();
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
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let row = tx.query_opt("SELECT turn_id,kind,visibility,payload FROM thread_items WHERE user_id=$1 AND session_id=$2 AND item_id=$3 FOR UPDATE", &[&user_id,&session_id,&item_id])?;
        let Some(row) = row else {
            return Ok(None);
        };
        let turn_id: String = row.get(0);
        let kind: String = row.get(1);
        let visibility: String = row.get(2);
        ensure!(
            kind == "assistant_message" && visibility == "user",
            "feedback requires visible assistant item"
        );
        let payload_text: String = row.get(3);
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
        tx.execute("UPDATE thread_items SET payload=$1,revision=revision+1,updated_time=$2 WHERE session_id=$3 AND item_id=$4", &[&text,&now,&session_id,&item_id])?;
        let revision: i64 = tx
            .query_one(
                "SELECT revision FROM thread_items WHERE session_id=$1 AND item_id=$2",
                &[&session_id, &item_id],
            )?
            .get(0);
        let seq: i64 = tx
            .query_one(
                "SELECT latest_change_seq+1 FROM thread_logs WHERE session_id=$1",
                &[&session_id],
            )?
            .get(0);
        tx.execute("INSERT INTO thread_log_changes(session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time) VALUES($1,$2,$3,$4,$5,$6,$7,'{}',$8)", &[&session_id,&seq,&user_id,&"item_upsert",&turn_id,&item_id,&revision,&now])?;
        tx.execute(
            "UPDATE thread_logs SET latest_change_seq=$1,updated_time=$2 WHERE session_id=$3",
            &[&seq, &now, &session_id],
        )?;
        tx.commit()?;
        Ok(Some(feedback))
    }
    fn list_thread_changes_by_session_impl(
        &self,
        session_id: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let after = after.max(0);
        let earliest: Option<i64> = conn
            .query_one(
                "SELECT MIN(change_seq) FROM thread_log_changes WHERE session_id=$1",
                &[&session_id],
            )?
            .get(0);
        if earliest.is_some_and(|first| first > after.saturating_add(1)) {
            return Ok(vec![json!({"change_type":"snapshot_required"})]);
        }
        Ok(conn.query("SELECT change_seq,change_type,turn_id,item_id,revision,payload,created_time FROM thread_log_changes WHERE session_id=$1 AND change_seq>$2 ORDER BY change_seq LIMIT $3", &[&session_id,&after,&limit.clamp(1,500)])?.into_iter().map(change_row).collect())
    }
    fn latest_thread_change_seq_by_session_impl(&self, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        Ok(conn
            .query_opt(
                "SELECT COALESCE(latest_change_seq,0) FROM thread_logs WHERE session_id=$1",
                &[&session_id],
            )?
            .map(|row| row.get(0))
            .unwrap_or(0))
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
        if earliest.is_some_and(|first| first > after.saturating_add(1)) {
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
            "DELETE FROM thread_log_metrics WHERE user_id=$1 AND session_id=$2",
            &[&user_id, &session_id],
        )? as i64;
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
