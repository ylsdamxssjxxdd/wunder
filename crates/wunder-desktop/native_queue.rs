//! User-facing queue façade: list, interject, withdraw and reorder parked turns.
//!
//! The engine owns the queue and already parks a turn submitted to a busy
//! thread. This layer only maps typed UI requests onto `ThreadRuntime` calls
//! and keeps the projection bounded, so the composer strip never grows with a
//! stuck feeder.

use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;

/// Rows the strip can render; matches the engine's own list cap.
pub const NATIVE_QUEUE_LIMIT: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeQueueTurn {
    pub queue_id: String,
    pub content: String,
    pub attachment_count: usize,
    pub position: usize,
    pub priority: i64,
    /// The sender's own id for the turn. A native window keeps one subscriber
    /// per parked submission and needs to drop exactly the right one when the
    /// row is withdrawn, which neither `queue_id` nor the text can tell it.
    pub client_message_id: String,
}

fn clean(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

/// Engine projection → strip rows. A turn with neither text nor an attachment
/// is not something the user can act on, so it is dropped here instead of
/// rendering an empty row.
pub fn queue_turns_from_value(value: &Value) -> Vec<NativeQueueTurn> {
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut rows = items
        .iter()
        .filter_map(|item| {
            let queue_id = clean(item.get("queue_id"));
            if queue_id.is_empty() {
                return None;
            }
            let content = clean(item.get("content"));
            let attachment_count = item
                .get("attachments")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            if content.is_empty() && attachment_count == 0 {
                return None;
            }
            Some(NativeQueueTurn {
                queue_id,
                content,
                attachment_count,
                position: item
                    .get("position")
                    .and_then(Value::as_u64)
                    .map_or(0, |value| value as usize),
                priority: item.get("priority").and_then(Value::as_i64).unwrap_or(0),
                client_message_id: clean(item.get("client_message_id")),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| (row.position, row.queue_id.clone()));
    rows.truncate(NATIVE_QUEUE_LIMIT);
    for (index, row) in rows.iter_mut().enumerate() {
        row.position = index;
    }
    rows
}

impl NativeDesktop {
    pub fn list_queue_turns(&self, session_id: &str) -> Result<Vec<NativeQueueTurn>> {
        let session = session_id.trim().to_string();
        if session.is_empty() {
            return Err(anyhow!("session is required"));
        }
        let state = self.state().clone();
        let user = self.user_id().to_string();
        let value = self.runtime.block_on(async move {
            state
                .kernel
                .thread_runtime
                .list_user_queue_tasks(&user, &session)
                .await
        })?;
        Ok(queue_turns_from_value(&value))
    }

    /// 插话：提到队首，在当前动作边界优先执行。
    pub fn interject_queue_turn(&self, session_id: &str, queue_id: &str) -> Result<()> {
        let state = self.state().clone();
        let user = self.user_id().to_string();
        let session = session_id.trim().to_string();
        let queue = queue_id.trim().to_string();
        self.runtime.block_on(async move {
            state
                .kernel
                .thread_runtime
                .prioritize_queued_task(&user, &session, &queue)
                .await
        })?;
        Ok(())
    }

    /// 撤下：取消一条还没开跑的轮次，返回原文供输入框回填。
    pub fn withdraw_queue_turn(&self, session_id: &str, queue_id: &str) -> Result<String> {
        let content = self
            .list_queue_turns(session_id)?
            .into_iter()
            .find(|row| row.queue_id == queue_id.trim())
            .map(|row| row.content)
            .unwrap_or_default();
        let state = self.state().clone();
        let user = self.user_id().to_string();
        let session = session_id.trim().to_string();
        let queue = queue_id.trim().to_string();
        self.runtime.block_on(async move {
            state
                .kernel
                .thread_runtime
                .cancel_queued_task(&user, &session, &queue)
                .await
        })?;
        Ok(content)
    }

    /// 拖拽排序：写回派发位次，返回重写后的顺序。
    pub fn reorder_queue_turns(
        &self,
        session_id: &str,
        ordered_ids: &[String],
    ) -> Result<Vec<NativeQueueTurn>> {
        let cleaned = ordered_ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        if cleaned.len() < 2 {
            return self.list_queue_turns(session_id);
        }
        let state = self.state().clone();
        let user = self.user_id().to_string();
        let session = session_id.trim().to_string();
        let ids: Arc<Vec<String>> = Arc::new(cleaned);
        self.runtime.block_on(async move {
            state
                .kernel
                .thread_runtime
                .reorder_queued_tasks(&user, &session, ids.as_slice())
                .await
        })?;
        self.list_queue_turns(session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn projection_keeps_actionable_rows_in_order() {
        let rows = queue_turns_from_value(&json!({
            "items": [
                {"queue_id":"task-b","content":"  第二条  ","position":1,"priority":0},
                {"queue_id":"task-a","content":"第一条","position":0,"priority":1,
                 "attachments":[{"name":"a.png"}]},
                {"queue_id":"","content":"无名"},
                {"queue_id":"task-c","content":"   "}
            ]
        }));
        assert_eq!(
            rows.iter()
                .map(|row| row.content.as_str())
                .collect::<Vec<_>>(),
            vec!["第一条", "第二条"]
        );
        assert_eq!(rows[0].attachment_count, 1);
        assert_eq!(rows[0].priority, 1);
        assert_eq!(
            rows.iter().map(|row| row.position).collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn projection_is_bounded() {
        let items = (0..64)
            .map(|index| {
                json!({"queue_id": format!("task-{index}"), "content": "x", "position": index})
            })
            .collect::<Vec<_>>();
        assert_eq!(
            queue_turns_from_value(&json!({ "items": items })).len(),
            NATIVE_QUEUE_LIMIT
        );
    }
}
