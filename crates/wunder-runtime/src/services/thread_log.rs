//! Shared, bounded timeline projection used by both authenticated API surfaces.
use crate::storage::StorageBackend;
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) mod diagnostics;

/// Read-time lifecycle projection settles display state after terminal
/// turns have finished. It does not rewrite model context.
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

/// Normalize the completed item's reasoning alias in memory. The item body
/// already contains its full original content; history reads never query a
/// second text-block log or perform storage repair.
pub fn hydrate_active_text(
    _storage: &dyn StorageBackend,
    _user_id: &str,
    _session_id: &str,
    message: &mut Value,
) -> anyhow::Result<()> {
    if message["reasoning"].as_str().is_none_or(str::is_empty) {
        if let Some(reasoning) = message["reasoning_content"]
            .as_str()
            .filter(|text| !text.is_empty())
        {
            message["reasoning"] = json!(reasoning);
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
    let rx = export_stream(storage, user_id, session_id, include_internal);
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

/// Shared export for HTTP and local clients. Dropping the receiver cancels the
/// producer; the two-record channel keeps database reads under backpressure.
pub fn export_stream(
    storage: Arc<dyn StorageBackend>,
    user_id: String,
    session_id: String,
    include_internal: bool,
) -> tokio::sync::mpsc::Receiver<Result<String, std::io::Error>> {
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
    rx
}

/// Stable lifecycle projection. Transport-only deltas never enter the timeline.
///
/// 严格白名单：只有携带时间线信息量的事件才入账（对齐 dsh 只记录可读轨迹）。
/// 传输态与统计通知（`*_delta`、heartbeat、context_usage、progress、
/// workspace_update、model_usage）、生命周期回执
/// （final/thread_status/…/llm_response；失败详情由 turn_terminal 携带，error
/// 事件只是流式通知）以及一切未知事件（hook 通知、子会话生命周期等）一律
/// 不入账。queue_* 的排队/运行状态只走实时控制通道；结束时才保留完成条目。
/// llm_stream_retry 通过 diagnostics 更新稳定模型条目，不新增事件行。
pub fn event_item(session_id: &str, event_type: &str, data: &Value) -> Option<Value> {
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
        // The item envelope uses the canonical lifecycle vocabulary. A
        // compaction's result (fallback/guard_only/skipped) is separate from
        // whether that operation completed and remains in its semantic body.
        "compaction" => (
            "compaction",
            data.get("compaction_id")
                .and_then(Value::as_str)
                .map(|id| format!("compaction-{id}"))
                .unwrap_or_else(|| format!("compaction-{model}")),
            match data.get("status").and_then(Value::as_str) {
                Some("done" | "completed" | "fallback" | "guard_only" | "skipped") => "completed",
                Some("failed" | "error") => "failed",
                Some("cancelled" | "canceled") => "cancelled",
                _ => "running",
            },
        ),
        // 成功终止只是生命周期回执（stop_reason=「model_response」之类对时间线
        // 没有信息量，轮次边界已由台账分组表达，对齐 dsh 无终止记录的做法）；
        // 只有失败终止携带错误详情，聊天视图也依赖它渲染失败原因。
        "turn_terminal" => {
            let failed = data.get("error").is_some()
                || matches!(
                    data.get("status").and_then(Value::as_str),
                    Some("failed") | Some("cancelled") | Some("rejected")
                );
            if !failed {
                return None;
            }
            (
                "terminal",
                "terminal".into(),
                data.get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("failed"),
            )
        }
        _ => return None,
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
    // 工具事件与 append_tool_log 写同一时间线条目，二者最后写入者不定；
    // 在持久化必经点补稳定规格引用，避免调用级定义被覆盖或重复保存。
    if kind == "tool_call" {
        if let Some(tool) = item.get("tool").and_then(Value::as_str) {
            if let Some(schema) = crate::tools::runtime_tool_schema(tool) {
                item["schema_ref"] =
                    serde_json::to_value(crate::tools::schema_reference(tool, &schema))
                        .unwrap_or(Value::Null);
            }
        }
    }
    // 上下文类事件（plan/approval/terminal/compaction/subagent_message）只有
    // 结构化字段、没有可读正文；轨迹 UI 渲染 `content`，这里补出最贴切的文本，
    // 对齐 dsh 中每种 kind 都有可读内容的做法。
    if item.get("content").is_none() {
        if let Some(text) = derive_context_text(event_type, data) {
            item["content"] = json!(text);
        }
    }
    if event_type == "compaction" {
        let mut body = data.clone();
        body["event_type"] = json!(event_type);
        if body.get("content").is_none() {
            body["content"] = item["content"].clone();
        }
        // Store the semantic body once. Restored workflow events unwrap it,
        // preserving the same result status as the live event.
        item = json!({
            "session_id": session_id,
            "turn_id": turn,
            "root_turn_id": data.get("root_turn_id"),
            "user_round": data.get("user_round"),
            "model_round": data.get("model_round"),
            "item_id": item["item_id"],
            "kind": kind,
            "status": status,
            "payload": body,
        });
    }
    Some(item)
}

/// 从上下文类事件的结构化字段中提取一段人类可读文本。白名单之外的
/// 事件类型不会走到这里。
fn derive_context_text(event_type: &str, data: &Value) -> Option<String> {
    let pick = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .find_map(|key| data.get(*key).and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    match event_type {
        "queue_enter" | "queue_update" | "queue_start" => {
            pick(&["reason", "summary", "queue_state"])
        }
        "plan_update" => pick(&["summary", "title", "reason"]).or_else(|| {
            data.get("plan").map(|value| match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
        }),
        "approval_request" | "approval_result" | "approval_resolved" => {
            pick(&["summary", "reason", "status"])
        }
        "turn_terminal" => {
            // 失败终止优先展示错误详情；error 是嵌套对象，逐层取可读字段。
            if let Some(error) = data.get("error") {
                let nested = |key: &str| -> Option<String> {
                    error
                        .get(key)
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                };
                if let Some(text) = nested("message").or_else(|| nested("code")) {
                    return Some(text);
                }
            }
            pick(&["status", "reason"])
        }
        "subagent_message" => pick(&["message", "summary", "content"]),
        "compaction" => pick(&["summary", "reason", "status"]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_status_events_never_enter_the_timeline() {
        for event_type in [
            "progress",
            "workspace_update",
            "heartbeat",
            "context_usage",
            "model_usage",
            "llm_stream_retry",
        ] {
            let item = event_item(
                "s",
                event_type,
                &json!({"turn_id":"turn","model_round":1,"summary":"noise"}),
            );
            assert!(item.is_none(), "{event_type} must stay out of the timeline");
        }
    }

    #[test]
    fn successful_turn_terminal_stays_out_failed_keeps_error_detail() {
        let success = event_item(
            "s",
            "turn_terminal",
            &json!({"turn_id":"turn","model_round":1,"status":"completed","stop_reason":"model_response"}),
        );
        assert!(
            success.is_none(),
            "successful terminals are lifecycle receipts"
        );

        let failed = event_item(
            "s",
            "turn_terminal",
            &json!({
                "turn_id":"turn",
                "model_round":1,
                "status":"failed",
                "error":{"code":"UPSTREAM_TIMEOUT","message":"upstream timeout"}
            }),
        )
        .expect("failed terminals keep error detail for the chat view");
        assert_eq!(failed["kind"], "terminal");
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["content"], "upstream timeout");
    }

    #[test]
    fn compaction_result_status_is_separate_from_completed_item_lifecycle() {
        for result in ["done", "completed", "fallback", "guard_only", "skipped"] {
            let item = event_item(
                "thread-a",
                "compaction",
                &json!({"turn_id":"turn-a","compaction_id":"compact-a","status":result,
                    "summary_text":"Retained context"}),
            )
            .expect("completed compaction becomes a durable item");
            assert_eq!(item["status"], "completed", "{result}");
            assert_eq!(item["payload"]["status"], result);
            assert_eq!(item["payload"]["summary_text"], "Retained context");
            assert_eq!(item["payload"]["event_type"], "compaction");
            assert!(item.get("summary_text").is_none());
        }
        let running = event_item(
            "thread-a",
            "compaction",
            &json!({"turn_id":"turn-a","status":"running"}),
        )
        .unwrap();
        assert_eq!(running["status"], "running");
    }

    #[test]
    fn error_and_unknown_events_stay_out_of_the_timeline() {
        // error 只是流式通知；失败详情由 turn_terminal 携带（见上方测试）。
        let error_item = event_item(
            "s",
            "error",
            &json!({"turn_id":"turn","model_round":1,"code":"UPSTREAM_TIMEOUT","message":"upstream timeout"}),
        );
        assert!(
            error_item.is_none(),
            "error is a stream notification, not a record"
        );

        // 白名单之外的一切未知事件（hook 通知、子会话生命周期等）一律不入账，
        // 避免 catch-all 制造「无内容」的上下文噪音。
        for event_type in ["unknown_note", "hook_invoked", "command_session_start"] {
            let item = event_item(
                "s",
                event_type,
                &json!({"turn_id":"turn","model_round":2,"stage":"guard","summary":"guard passed"}),
            );
            assert!(item.is_none(), "{event_type} must stay out of the timeline");
        }
    }

    #[test]
    fn tool_events_carry_schema_reference_regardless_of_writer_order() {
        for event_type in ["tool_call", "tool_result"] {
            let item = event_item(
                "s",
                event_type,
                &json!({"turn_id":"turn","model_round":1,"tool":"读取文件","tool_call_id":"call_1","ok":true}),
            )
            .expect("tool events enter the timeline");
            let schema_ref = item
                .get("schema_ref")
                .expect("durable tool items keep a call schema reference");
            assert!(schema_ref["schema_ref"]
                .as_str()
                .is_some_and(|value| value.starts_with("tool-schema:")));
            assert_eq!(
                schema_ref["catalog_version"],
                crate::tools::TOOL_CATALOG_VERSION
            );
        }
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
    let changes = storage.list_thread_changes_by_session(session_id, after, limit)?;
    let mut records = Vec::with_capacity(changes.len());
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
    Ok(records)
}
