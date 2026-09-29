//! Shared, bounded timeline projection used by both authenticated API surfaces.
use crate::storage::StorageBackend;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn export_response(
    storage: Arc<dyn StorageBackend>,
    user_id: String,
    session_id: String,
    include_internal: bool,
) -> axum::response::Response {
    // Backpressure propagates to the database producer, including download cancellation.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(2);
    crate::core::long_task::spawn("thread_log.export", async move {
        let result: anyhow::Result<()> = async {
            let header=json!({"record_type":"thread_meta","export_schema_version":5,"session_id":session_id});
            tx.send(Ok(format!("{header}\n"))).await?;
            let mut before=None;
            loop {
                let db=storage.clone();let owner=user_id.clone();let thread=session_id.clone();
                let turns=crate::core::blocking::run_db("thread_log.export.turns",move ||
                    db.list_thread_turns(&owner,&thread,before,50)).await?;
                if turns.is_empty(){break;}
                before=turns.last().and_then(|v|v["user_turn_index"].as_i64());
                for turn in turns {
                    let turn_id=turn["turn_id"].as_str().unwrap_or_default().to_string();
                    tx.send(Ok(format!("{}\n",json!({"record_type":"turn","turn":turn})))).await?;
                    let mut after=-1;
                    loop {
                        let db=storage.clone();let owner=user_id.clone();let thread=session_id.clone();let id=turn_id.clone();
                        let page=crate::core::blocking::run_db("thread_log.export.items",move ||
                            db.get_thread_turn(&owner,&thread,&id,after,50,include_internal)).await?
                            .ok_or_else(||anyhow::anyhow!("turn deleted during export"))?;
                        for item in page["items"].as_array().into_iter().flatten() {
                            tx.send(Ok(format!("{}\n",json!({"record_type":"item","item":item})))).await?;
                        }
                        if page["has_more"]!=true {break;}
                        after=page["next_after"].as_i64().ok_or_else(||anyhow::anyhow!("missing item cursor"))?;
                    }
                }
            }
            // Consumers can distinguish a complete export from an interrupted download.
            tx.send(Ok(format!("{}\n",json!({"record_type":"export_complete"})))).await?;
            Ok(())
        }.await;
        if let Err(err) = result {
            let _ = tx.send(Err(std::io::Error::other(err.to_string()))).await;
        }
    });
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let mut response = axum::response::Response::new(axum::body::Body::from_stream(stream));
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/x-ndjson;charset=utf-8"),
    );
    response.headers_mut().insert(
        axum::http::header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_static("attachment; filename=thread-log.jsonl"),
    );
    response
}

/// Stable lifecycle projection. Transport-only deltas never enter the timeline.
pub fn event_item(session_id: &str, event_type: &str, data: &Value) -> Option<Value> {
    if event_type.ends_with("_delta") || matches!(event_type, "heartbeat" | "context_usage") {
        return None;
    }
    let turn = data.get("turn_id")?.as_str()?;
    let model = data.get("model_round").and_then(Value::as_i64).unwrap_or(0);
    let (kind, key, status) = match event_type {
        "tool_call" | "tool_result" => (
            "tool_call",
            format!("tool-{}", data.get("tool_call_id")?.as_str()?),
            if event_type == "tool_call" {
                "running"
            } else if data.get("ok") == Some(&Value::Bool(false)) {
                "failed"
            } else {
                "completed"
            },
        ),
        "llm_request" | "llm_output" => (
            "model_call",
            format!("model-{model}"),
            if event_type == "llm_request" {
                "running"
            } else {
                "completed"
            },
        ),
        "approval_request" | "approval_result" | "approval_resolved" => (
            "approval",
            format!(
                "approval-{}",
                data.get("approval_id")
                    .or_else(|| data.get("request_id"))?
                    .as_str()?
            ),
            if event_type == "approval_request" {
                "running"
            } else {
                "completed"
            },
        ),
        "plan_update" => ("plan", "plan".into(), "completed"),
        "turn_terminal" => (
            "terminal",
            "terminal".into(),
            data.get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed"),
        ),
        "final"
        | "thread_status"
        | "thread_closed"
        | "quota_usage"
        | "token_usage"
        | "round_usage"
        | "model_request_usage"
        | "llm_response" => return None,
        _ => (event_type, uuid::Uuid::new_v4().to_string(), "completed"),
    };
    let mut item = data.clone();
    item["session_id"] = json!(session_id);
    item["item_id"] = json!(format!("{turn}:{key}"));
    item["kind"] = json!(kind);
    item["status"] = json!(status);
    if event_type == "llm_request" {
        item["visibility"] = json!("admin");
    }
    Some(item)
}

/// Only the active tail is copied on a flush. Completed blocks stay immutable.
#[derive(Default)]
pub struct TextTail {
    item_id: String,
    block_index: i64,
    content: String,
    reasoning: String,
    content_offset: usize,
    reasoning_offset: usize,
    event_id: i64,
    context: Value,
    last_flush: Option<std::time::Instant>,
    dirty: bool,
}
impl TextTail {
    pub fn append(&mut self, session_id: &str, event_id: i64, data: &Value) -> Option<Value> {
        let turn = data.get("turn_id")?.as_str()?;
        let model = data.get("model_round").and_then(Value::as_i64).unwrap_or(0);
        let item_id = format!("{turn}:text-{model}");
        if self.item_id != item_id {
            *self = Self {
                item_id,
                context: data.clone(),
                ..Default::default()
            };
            if let Some(map) = self.context.as_object_mut() {
                map.remove("delta");
                map.remove("reasoning_delta");
            }
        }
        self.content
            .push_str(data.get("delta").and_then(Value::as_str).unwrap_or(""));
        self.reasoning.push_str(
            data.get("reasoning_delta")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        self.event_id = event_id;
        self.dirty = true;
        let full = self.content.len() + self.reasoning.len() >= 8192;
        let due = self
            .last_flush
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(150));
        if !full && !due {
            return None;
        }
        let block = self.flush(session_id);
        if full {
            self.content_offset += self.content.encode_utf16().count();
            self.reasoning_offset += self.reasoning.encode_utf16().count();
            self.content.clear();
            self.reasoning.clear();
            self.block_index += 1;
        }
        block
    }
    pub fn flush(&mut self, session_id: &str) -> Option<Value> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        self.last_flush = Some(std::time::Instant::now());
        let mut data = self.context.clone();
        data["content"] = json!(self.content);
        data["reasoning"] = json!(self.reasoning);
        data["content_offset"] = json!(self.content_offset);
        data["reasoning_offset"] = json!(self.reasoning_offset);
        data["item_id"] = json!(self.item_id);
        data["block_index"] = json!(self.block_index);
        Some(
            json!({"event":"thread_item_block","event_id":self.event_id,"item_id":self.item_id,
            "block_index":self.block_index,"session_id":session_id,"data":data}),
        )
    }
}

/// Merge short-lived boundary events with block snapshots for queued and reconnect readers.
pub fn replay(
    storage: &dyn StorageBackend,
    session_id: &str,
    after: i64,
    limit: i64,
) -> anyhow::Result<Vec<Value>> {
    let limit = limit.clamp(1, 500);
    let mut records = storage.load_stream_events(session_id, after, limit)?;
    records.extend(storage.list_thread_text_blocks(session_id, after, limit)?);
    records.sort_by_key(|v| v["event_id"].as_i64().unwrap_or(0));
    records.truncate(limit as usize);
    Ok(records)
}
