//! Write-time display projection, shared by both transactional stores.
use serde_json::{json, Value};
use wunder_core::thread_view::{preview, BodyRef, ChatViewRow, TrajectoryViewRow, ViewMessage};

pub(crate) fn body_id(item_id: &str, field: &str) -> String {
    format!("{item_id}|{field}")
}

pub(crate) fn message(item_id: &str, content: &str, revision: i64) -> ViewMessage {
    ViewMessage {
        item_id: item_id.into(),
        content: preview(content, 2048),
        body: Some(BodyRef {
            body_id: body_id(item_id, "content"),
            revision,
            bytes: content.len(),
        }),
        ..Default::default()
    }
}

pub(crate) fn set_reasoning(message: &mut ViewMessage, payload: &Value) {
    let reasoning = payload
        .get("reasoning")
        .or_else(|| payload.get("reasoning_content"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if !reasoning.is_empty() {
        message.reasoning = preview(reasoning, 2048);
        message.reasoning_offset = reasoning.encode_utf16().count();
    }
}

pub(crate) fn stable_answer(item_id: &str, turn_id: &str, kind: &str) -> bool {
    kind == "assistant_message" && item_id.starts_with(&format!("{turn_id}:text-"))
}

pub(crate) fn is_failed(status: &str) -> i64 {
    i64::from(matches!(status, "failed" | "rejected"))
}
pub(crate) fn is_running(status: &str) -> i64 {
    i64::from(matches!(
        status,
        "running" | "queued" | "waiting_input" | "waiting_approval"
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn trajectory(
    item: &Value,
    item_id: &str,
    turn_id: &str,
    root: &str,
    round: i64,
    index: i64,
    seq: i64,
    revision: i64,
    kind: &str,
    status: &str,
    created: f64,
    updated: f64,
) -> TrajectoryViewRow {
    let content = item.get("content").and_then(Value::as_str).unwrap_or("");
    // 工具条目的事件负载里工具名字段是 `tool`（tool_name 从不存在）。
    let tool = item.get("tool").and_then(Value::as_str);
    let summary = if content.is_empty() {
        if kind == "tool_call" {
            // 工具行必须展示调用的是什么工具，而不是回退成裸 kind。
            tool.unwrap_or(kind)
        } else if kind == "assistant_message" {
            // 工具调用轮的助手条目没有正文；思考摘要是该条目的可视主体。
            item.get("reasoning")
                .or_else(|| item.get("reasoning_content"))
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .unwrap_or(kind)
        } else {
            item.get("summary")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .unwrap_or(kind)
        }
    } else {
        content
    };
    TrajectoryViewRow {
        item_id: item_id.into(),
        turn_id: turn_id.into(),
        root_turn_id: root.into(),
        user_round: round,
        item_index: index,
        created_seq: seq,
        revision,
        kind: preview(kind, 48),
        status: preview(status, 48),
        summary: preview(summary, 256),
        tool_name: tool.map(|v| preview(v, 96)),
        model_round: item.get("model_round").and_then(Value::as_i64).unwrap_or(0),
        started_at: item
            .get("started_at")
            .and_then(Value::as_f64)
            .unwrap_or(created),
        completed_at: (is_running(status) == 0).then_some(updated),
        detail: Some(BodyRef {
            body_id: body_id(item_id, "detail"),
            revision,
            bytes: 0,
        }),
    }
}

pub(crate) fn new_chat(
    root: &str,
    round: i64,
    client: Option<String>,
    status: &str,
    created: f64,
    updated: f64,
) -> ChatViewRow {
    ChatViewRow {
        root_turn_id: root.into(),
        user_round: round,
        client_message_id: client,
        status: status.into(),
        created_time: created,
        updated_time: updated,
        ..Default::default()
    }
}

pub(crate) fn finite_stats(item: &Value) -> Option<Value> {
    let source = item
        .get("message_stats")
        .or_else(|| item.pointer("/meta/message_stats"))?
        .as_object()?;
    // Keep the small nested usage payloads intact.  The previous projection
    // admitted only top-level numbers/booleans, which silently removed
    // `round_usage`/`usage` and left the bubble's token metric as `--` after a
    // reload even though the server had persisted the counts.
    let mut fields = serde_json::Map::new();
    let mut scalar_count = 0usize;
    for (key, value) in source.iter() {
        let keep_scalar = value.is_number() || value.is_boolean();
        let keep_usage = matches!(key.as_str(), "usage" | "round_usage" | "roundUsage")
            && value.as_object().is_some();
        if !keep_scalar && !keep_usage { continue; }
        if keep_scalar {
            scalar_count = scalar_count.saturating_add(1);
            if scalar_count > 64 && !matches!(key.as_str(), "model_request_count" | "account_credits_consumed" | "tool_calls" | "toolCalls") {
                continue;
            }
        }
        if keep_usage {
            let bounded = value.as_object().unwrap().iter()
                .filter(|(_, v)| v.is_number() || v.is_boolean())
                .take(12)
                .map(|(k, v)| (preview(k, 64), v.clone()))
                .collect::<serde_json::Map<_, _>>();
            fields.insert(preview(key, 64), Value::Object(bounded));
        } else {
            fields.insert(preview(key, 64), value.clone());
        }
    }
    // Usage fields are canonical even when a provider serializes them after a
    // large set of optional metrics.  The explicit lookup above is therefore
    // intentionally independent of object iteration order.
    for key in ["usage", "round_usage", "roundUsage"] {
        if fields.contains_key(key) { continue; }
        let Some(value) = source.get(key).and_then(Value::as_object) else { continue };
        let bounded = value.iter()
            .filter(|(_, v)| v.is_number() || v.is_boolean())
            .take(16)
            .map(|(k, v)| (preview(k, 64), v.clone()))
            .collect::<serde_json::Map<_, _>>();
        fields.insert(key.into(), Value::Object(bounded));
    }
    // Preserve the canonical quota aliases when they are present in the
    // nested request usage object.  This is the value shown in the bubble,
    // distinct from context occupancy.
    if !fields.contains_key("request_consumed_tokens") {
        for usage_key in ["round_usage", "usage"] {
            let value = fields
                .get(usage_key)
                .and_then(Value::as_object)
                .and_then(|usage| usage.get("request_consumed_tokens").or_else(|| usage.get("total_tokens")))
                .cloned();
            if let Some(value) = value {
                    fields.insert("request_consumed_tokens".into(), value.clone());
                    fields.insert("consumed_tokens".into(), value);
                    break;
            }
        }
    }
    Some(Value::Object(fields))
}

/// 每个聊天行最多保留的子智能体卡片数：进度卡是展示件，超出窗口的旧运行
/// 由轨迹视图承载。
pub(crate) const MAX_ROW_SUBAGENTS: usize = 8;

/// 子智能体进度卡片（写时投影）：从 `subagent_run` 条目的 `runtime` 负载里
/// 抽出面板直接消费的标量与有界指标，不复制子会话正文或执行配置。
pub(crate) fn subagent_card(item: &Value, id: &str, revision: i64) -> Option<Value> {
    let runtime = item.get("runtime")?.as_object()?;
    let mut card = serde_json::Map::new();
    card.insert("key".into(), json!(preview(id, 96)));
    for field in ["run_id", "session_id", "dispatch_id", "run_kind", "title", "label", "status"] {
        if let Some(v) = runtime
            .get(field)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            card.insert(field.into(), json!(preview(v, 128)));
        }
    }
    for field in ["terminal", "failed"] {
        if let Some(v) = runtime.get(field).and_then(Value::as_bool) {
            card.insert(field.into(), json!(v));
        }
    }
    if let Some(v) = runtime.get("can_terminate").and_then(Value::as_bool) {
        card.insert("canTerminate".into(), json!(v));
    }
    for field in ["latest_message", "summary", "user_message"] {
        if let Some(v) = runtime
            .get(field)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            card.insert(field.into(), json!(preview(v, 256)));
        }
    }
    for field in ["updated_time", "started_time", "queued_time", "elapsed_s"] {
        if let Some(v) = runtime.get(field).and_then(Value::as_f64) {
            card.insert(field.into(), json!(v));
        }
    }
    if let Some(metrics) = runtime.get("metrics").and_then(Value::as_object) {
        let bounded = metrics
            .iter()
            .filter(|(_, v)| v.is_number() || v.is_boolean() || v.is_string())
            .take(16)
            .map(|(k, v)| (preview(k, 64), v.clone()))
            .collect::<serde_json::Map<_, _>>();
        card.insert("metrics".into(), Value::Object(bounded));
    }
    card.insert("updated_seq".into(), json!(revision));
    Some(Value::Object(card))
}

/// 同一 key 原地更新（子智能体进度推进复用同一条目），新运行追加并裁掉最旧。
pub(crate) fn upsert_subagent_card(cards: &mut Vec<Value>, card: Value) {
    let key = card
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if let Some(existing) = cards
        .iter_mut()
        .find(|c| c.get("key").and_then(Value::as_str) == Some(key.as_str()))
    {
        *existing = card;
        return;
    }
    cards.push(card);
    while cards.len() > MAX_ROW_SUBAGENTS {
        cards.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project(kind: &str, item: &Value) -> TrajectoryViewRow {
        trajectory(
            item,
            "item-1",
            "turn-1",
            "turn-1",
            1,
            0,
            1,
            1,
            kind,
            "completed",
            0.0,
            1.0,
        )
    }

    #[test]
    fn tool_rows_surface_the_called_tool_not_the_raw_kind() {
        let row = project(
            "tool_call",
            &json!({"tool":"读取文件","args":{"path":"/tmp/a"},"ok":true}),
        );
        assert_eq!(row.tool_name.as_deref(), Some("读取文件"));
        assert_eq!(row.summary, "读取文件");
    }

    #[test]
    fn empty_assistant_rows_fall_back_to_the_reasoning_preview() {
        let row = project(
            "assistant_message",
            &json!({"content":"","reasoning_content":"正在读取文件以确认路径"}),
        );
        assert_eq!(row.summary, "正在读取文件以确认路径");
    }

    #[test]
    fn context_rows_keep_their_body_text() {
        let row = project(
            "system_message",
            &json!({"content":"你是 WUNDER，一个强大的智能体。","meta":{"type":"system_prompt"}}),
        );
        assert_eq!(row.summary, "你是 WUNDER，一个强大的智能体。");
    }

    #[test]
    fn subagent_cards_expose_the_panel_contract_without_child_payloads() {
        let item = json!({
            "runtime": {
                "session_id": "child-session", "run_id": "run-1", "title": "研究子任务",
                "status": "running", "terminal": false, "failed": false, "can_terminate": true,
                "latest_message": "正在读取资料", "result": {"heavy": true}, "metadata": {"x": 1},
                "metrics": {"tool_calls": 3, "context_tokens": 1024, "child_turn_id": "turn-9"}
            }
        });
        let card = subagent_card(&item, "turn-1:subagent-run-1", 7).expect("card");
        assert_eq!(card["key"], "turn-1:subagent-run-1");
        assert_eq!(card["session_id"], "child-session");
        assert_eq!(card["status"], "running");
        assert_eq!(card["canTerminate"], true);
        assert_eq!(card["latest_message"], "正在读取资料");
        assert_eq!(card["metrics"]["tool_calls"], 3);
        // 面板契约不需要子会话正文副本；结果与元数据永不进卡片。
        assert!(card.get("result").is_none());
        assert!(card.get("metadata").is_none());

        let mut cards = Vec::new();
        upsert_subagent_card(&mut cards, card);
        let updated = subagent_card(
            &json!({"runtime": {"session_id": "child-session", "run_id": "run-1",
                "status": "completed", "terminal": true, "can_terminate": false,
                "metrics": {"tool_calls": 5}}}),
            "turn-1:subagent-run-1",
            9,
        )
        .expect("card");
        upsert_subagent_card(&mut cards, updated);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0]["status"], "completed");
        assert_eq!(cards[0]["updated_seq"], 9);

        for index in 0..(MAX_ROW_SUBAGENTS + 2) {
            let item = json!({"runtime": {"run_id": format!("run-{index}"), "status": "completed"}});
            if let Some(card) = subagent_card(&item, &format!("turn-1:subagent-run-{index}"), index as i64) {
                upsert_subagent_card(&mut cards, card);
            }
        }
        assert_eq!(cards.len(), MAX_ROW_SUBAGENTS);
        // 窗口裁剪丢掉最旧的运行，同 key 的进度推进永远保留最新一份。
        assert!(cards.iter().all(|c| c["key"] != "turn-1:subagent-run-1"));
    }
}
