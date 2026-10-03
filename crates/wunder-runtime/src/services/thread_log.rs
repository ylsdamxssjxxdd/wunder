//! Shared, bounded timeline projection used by both authenticated API surfaces.
use crate::storage::StorageBackend;
use serde_json::{json, Value};
use std::sync::Arc;

/// Read-time lifecycle projection also repairs logs written before terminal
/// turns began settling their unfinished items. It does not rewrite model context.
pub fn project_visible_item(message: &mut Value, kind: &str, turn_status: &str) {
    if message.get("role").and_then(Value::as_str).is_none() {
        message["role"] = json!(if kind == "user_message" {
            "user"
        } else {
            "assistant"
        });
    }
    if matches!(
        message["status"].as_str(),
        Some("queued" | "running" | "waiting_input")
    ) && matches!(
        turn_status,
        "completed" | "failed" | "cancelled" | "interrupted"
    ) {
        message["status"] = json!(turn_status);
    }
}

/// Hydrate only unfinished visible messages. Completed messages already own their
/// final text; recovery reads the active fields in bounded database pages.
pub fn hydrate_active_text(
    storage: &dyn StorageBackend,
    user_id: &str,
    session_id: &str,
    message: &mut Value,
) -> anyhow::Result<()> {
    if message["role"] != "assistant"
        || !matches!(
            message["status"].as_str(),
            Some(
                "running" | "streaming" | "waiting_input" | "cancelled" | "interrupted" | "failed"
            )
        )
    {
        return Ok(());
    }
    let Some(item_id) = message["item_id"].as_str().map(str::to_owned) else {
        return Ok(());
    };
    for field in ["content", "reasoning"] {
        let mut from = 0;
        let mut text = String::new();
        loop {
            let (blocks, next, more) = storage.list_thread_item_blocks_page(
                user_id,
                session_id,
                &item_id,
                Some(field),
                from,
                100,
                false,
            )?;
            for block in blocks {
                let data = block.get("data").unwrap_or(&block);
                if let Some(part) = data[field].as_str() {
                    text.push_str(part);
                }
            }
            if !more {
                break;
            }
            let next = next
                .map(|next| next.saturating_add(1))
                .filter(|next| *next > from)
                .ok_or_else(|| anyhow::anyhow!("text block cursor did not advance"))?;
            from = next;
        }
        if !text.is_empty() {
            message[field] = json!(text);
        }
    }
    Ok(())
}

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
        "queue_enter" | "queue_update" | "queue_start" => (
            "queue",
            format!(
                "queue-{}",
                data.get("queue_id")
                    .and_then(Value::as_str)
                    .unwrap_or("handoff")
            ),
            if event_type == "queue_start" {
                "running"
            } else {
                "queued"
            },
        ),
        // A mailbox message may be observed repeatedly while an execution is
        // retried. Its producer supplied message_id is the durable identity;
        // a generated key would make a replay look like a new timeline item.
        "subagent_message" => (
            "subagent_message",
            format!("subagent-{}", data.get("message_id")?.as_str()?),
            if data.get("delivery").and_then(Value::as_str) == Some("not_applied") {
                "cancelled"
            } else {
                "completed"
            },
        ),
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
        "llm_request" => ("model_call", format!("model-{model}"), "running"),
        "llm_output" => ("assistant_message", format!("text-{model}"), "completed"),
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
        // Preserve the compaction lifecycle; the generic completed default
        // otherwise turns an in-flight compaction green as soon as it is stored.
        "compaction" => (
            "compaction",
            data.get("compaction_id")
                .and_then(Value::as_str)
                .map(|id| format!("compaction-{id}"))
                .unwrap_or_else(|| format!("compaction-{model}")),
            data.get("status")
                .and_then(Value::as_str)
                .unwrap_or("running"),
        ),
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
        | "thread_turn_started"
        | "quota_usage"
        | "token_usage"
        | "round_usage"
        | "model_request_usage"
        | "llm_response" => return None,
        _ => (event_type, uuid::Uuid::new_v4().to_string(), "completed"),
    };
    let mut item = data.clone();
    item["event_type"] = json!(event_type);
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
    /// Durable cursor the active tail depends on (item registration or the
    /// most recent text block). Ephemeral frames carry this as `base_seq` so
    /// clients never apply text before the corresponding durable item.
    base_seq: i64,
}

/// The number of interleaved text items retained by one emitter is bounded.
/// Each item keeps only its active tail; completed blocks live in ThreadLog.
#[derive(Default)]
pub struct TextTails {
    tails: std::collections::HashMap<String, TextTail>,
}

impl TextTails {
    const MAX_ITEMS: usize = 16;

    fn item_id(data: &Value) -> Option<String> {
        let turn = data.get("turn_id")?.as_str()?;
        let model = data.get("model_round").and_then(Value::as_i64).unwrap_or(0);
        Some(format!("{turn}:text-{model}"))
    }

    pub fn set_base_seq(&mut self, item_id: &str, seq: i64) {
        self.tails
            .entry(item_id.to_string())
            .or_default()
            .set_base_seq(seq);
    }

    pub fn base_seq(&self, item_id: &str) -> i64 {
        self.tails.get(item_id).map(TextTail::base_seq).unwrap_or(0)
    }

    pub fn append(
        &mut self,
        session_id: &str,
        event_id: i64,
        data: &Value,
    ) -> (Vec<Value>, Vec<(String, i64, String, String)>) {
        let Some(item_id) = Self::item_id(data) else {
            return (Vec::new(), Vec::new());
        };
        let mut blocks = Vec::new();
        if !self.tails.contains_key(&item_id) && self.tails.len() >= Self::MAX_ITEMS {
            if let Some(oldest) = self.tails.keys().min().cloned() {
                if let Some(mut old) = self.tails.remove(&oldest) {
                    if let Some(flush) = old.flush(session_id) {
                        blocks.push(flush);
                    }
                }
            }
        }
        let tail = self.tails.entry(item_id.clone()).or_default();
        let (_, content_offset, reasoning_offset) = tail.tail_annotation(data);
        let mut hints = Vec::with_capacity(2);
        if let (Some(offset), Some(text)) =
            (content_offset, data.get("delta").and_then(Value::as_str))
        {
            if !text.is_empty() {
                hints.push((item_id.clone(), offset, "content".into(), text.into()));
            }
        }
        if let (Some(offset), Some(text)) = (
            reasoning_offset,
            data.get("reasoning_delta").and_then(Value::as_str),
        ) {
            if !text.is_empty() {
                hints.push((item_id.clone(), offset, "reasoning".into(), text.into()));
            }
        }
        if let Some(flush) = tail.append(session_id, event_id, data) {
            blocks.push(flush);
        }
        (blocks, hints)
    }

    pub fn flush_item(&mut self, session_id: &str, item_id: &str) -> Option<Value> {
        self.tails.get_mut(item_id)?.flush(session_id)
    }

    pub fn flush_all(&mut self, session_id: &str) -> Vec<Value> {
        self.tails
            .values_mut()
            .filter_map(|tail| tail.flush(session_id))
            .collect()
    }
}
impl TextTail {
    pub fn set_base_seq(&mut self, seq: i64) {
        self.base_seq = self.base_seq.max(seq.max(0));
    }

    pub fn base_seq(&self) -> i64 {
        self.base_seq
    }
    /// UTF-16 offsets at which the incoming delta would append, used by the
    /// ephemeral tail frames of the change stream. Returns
    /// `(item_id, content_offset, reasoning_offset)`; an offset is None when
    /// the payload carries no text for that field. A fresh item (first delta
    /// after a switch) reports offset 0 instead of the previous item's tail.
    pub fn tail_annotation(&self, data: &Value) -> (Option<String>, Option<i64>, Option<i64>) {
        let turn = match data.get("turn_id").and_then(Value::as_str) {
            Some(turn) if !turn.is_empty() => turn,
            _ => return (None, None, None),
        };
        let model = data.get("model_round").and_then(Value::as_i64).unwrap_or(0);
        let incoming = format!("{turn}:text-{model}");
        let fresh = self.item_id != incoming;
        let content_offset = data
            .get("delta")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|_| {
                if fresh {
                    0
                } else {
                    (self.content_offset + self.content.encode_utf16().count()) as i64
                }
            });
        let reasoning_offset = data
            .get("reasoning_delta")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|_| {
                if fresh {
                    0
                } else {
                    (self.reasoning_offset + self.reasoning.encode_utf16().count()) as i64
                }
            });
        (Some(incoming), content_offset, reasoning_offset)
    }

    pub fn append(&mut self, session_id: &str, event_id: i64, data: &Value) -> Option<Value> {
        let turn = data.get("turn_id")?.as_str()?;
        let model = data.get("model_round").and_then(Value::as_i64).unwrap_or(0);
        let item_id = format!("{turn}:text-{model}");
        if self.item_id != item_id {
            // `set_base_seq` can register this tail before its first delta.
            // Keep that durable dependency when the first delta supplies the
            // item's context; otherwise v2 tail frames could incorrectly use
            // base_seq=0 and race ahead of the durable item_upsert.
            let base_seq = self.base_seq;
            *self = Self {
                item_id,
                context: data.clone(),
                base_seq,
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
        // Content and reasoning use independent field/block coordinates.  A
        // shared payload would make the database key overwrite one field when
        // both are present at the same block index.
        let field_block = |field: &str, text: &str, offset: usize| {
            let mut data = self.context.clone();
            data["field"] = json!(field);
            if field == "reasoning" {
                data["reasoning"] = json!(text);
                data["reasoning_offset"] = json!(offset);
            } else {
                data["content"] = json!(text);
                data["content_offset"] = json!(offset);
            }
            data["item_id"] = json!(self.item_id);
            data["block_index"] = json!(self.block_index);
            json!({"event":"thread_item_block","event_id":self.event_id,"item_id":self.item_id,
                "field":field,"block_index":self.block_index,"session_id":session_id,"data":data})
        };
        let mut blocks = Vec::with_capacity(2);
        if !self.content.is_empty() {
            blocks.push(field_block("content", &self.content, self.content_offset));
        }
        if !self.reasoning.is_empty() {
            blocks.push(field_block(
                "reasoning",
                &self.reasoning,
                self.reasoning_offset,
            ));
        }
        (!blocks.is_empty()).then(|| json!({"blocks":blocks}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_tail_separates_content_and_reasoning_block_identity() {
        let mut tail = TextTail::default();
        let flush = tail
            .append(
                "thread",
                1,
                &json!({
                    "turn_id":"turn", "model_round":1,
                    "delta":"answer", "reasoning_delta":"thought"
                }),
            )
            .expect("initial flush");
        let blocks = flush["blocks"].as_array().expect("blocks");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["field"], "content");
        assert_eq!(blocks[0]["data"]["content"], "answer");
        assert!(blocks[0]["data"].get("reasoning").is_none());
        assert_eq!(blocks[1]["field"], "reasoning");
        assert_eq!(blocks[1]["data"]["reasoning"], "thought");
        assert!(blocks[1]["data"].get("content").is_none());
    }
}

/// Compatibility replay reads only durable ThreadLog text snapshots. Lifecycle
/// recovery is served by the change cursor API; stream_events are never a
/// source of historical assistant text or round state.
pub fn replay(
    storage: &dyn StorageBackend,
    session_id: &str,
    after: i64,
    limit: i64,
) -> anyhow::Result<Vec<Value>> {
    let limit = limit.clamp(1, 500);
    let after = after.max(0);
    // The cursor belongs to durable changes. Text blocks carry independent
    // transport offsets, so they are snapshots on every recovery rather than
    // competing for the cursor page.
    let changes = storage.list_thread_changes_by_session(session_id, after, limit)?;
    let mut records = Vec::with_capacity(changes.len() + limit as usize);
    for mut change in changes {
        if change.get("change_type").and_then(Value::as_str) == Some("snapshot_required") {
            records
                .push(json!({"event":"thread_snapshot_required","data":change,"event_id":after+1}));
            return Ok(records);
        }
        let cursor = change
            .get("change_seq")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        change["event"] = json!("thread_change");
        change["event_id"] = json!(cursor);
        change["data"] = json!({"change_type":change.get("change_type").cloned().unwrap_or(Value::Null),"turn_id":change.get("turn_id").cloned().unwrap_or(Value::Null),"item_id":change.get("item_id").cloned().unwrap_or(Value::Null),"revision":change.get("revision").cloned().unwrap_or(Value::Null),"cursor":cursor,"payload":change.get("payload").cloned().unwrap_or(Value::Null)});
        records.push(change);
    }
    // The caller's event cursor does not apply to block event IDs. Return a
    // bounded current snapshot so offsets can overwrite the active tail.
    records.extend(storage.list_thread_text_blocks(session_id, 0, limit)?);
    Ok(records)
}
