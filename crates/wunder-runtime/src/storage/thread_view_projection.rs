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
        message.reasoning = preview_tail(reasoning, 2048);
        message.reasoning_offset = reasoning.encode_utf16().count();
    }
}

/// UTF-8-safe bounded tail, so capped live previews continue to move.
pub(crate) fn preview_tail(text: &str, budget: usize) -> String {
    let mut start = text.len().saturating_sub(budget);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

pub(crate) fn set_activity(row: &mut ChatViewRow, kind: &str, id: &str, seq: i64, tool: &str) {
    if seq < row.process_activity.created_seq {
        return;
    }
    // Reasoning and answer blocks can flush independently for one model item.
    // Once its answer starts, a late reasoning block cannot regress the phase.
    if row.process_activity.item_id == id
        && row.process_activity.kind == "responding"
        && kind == "thinking"
    {
        return;
    }
    row.process_activity = wunder_core::thread_view::ProcessActivity {
        kind: kind.into(),
        item_id: id.into(),
        tool: preview(tool, 96),
        created_seq: seq,
    };
}

pub(crate) fn stable_answer(item_id: &str, turn_id: &str, kind: &str) -> bool {
    kind == "assistant_message" && item_id.starts_with(&format!("{turn_id}:text-"))
}

/// An assistant item that owns a tool call is model work-process output.  It
/// may contain useful prose, but it is not the terminal answer for the user
/// turn.  Keeping this rule in the shared projection module prevents the
/// durable view and source fallback from choosing different answer identities.
pub(crate) fn has_tool_calls(item: &Value) -> bool {
    item.get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty())
}

pub(crate) fn is_failed(status: &str) -> i64 {
    i64::from(matches!(status, "failed" | "rejected"))
}
pub(crate) fn is_running(status: &str) -> i64 {
    i64::from(matches!(
        status,
        "accepted" | "running" | "queued" | "waiting_input" | "waiting_approval"
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
    let model_context =
        item.pointer("/meta/type").and_then(Value::as_str) == Some("model_context_internal");
    // 工具条目的事件负载里工具名字段是 `tool`（tool_name 从不存在）。
    let tool = item.get("tool").and_then(Value::as_str);
    let runtime = item.get("runtime").and_then(Value::as_object);
    let runtime_label = subagent_label(item);
    let runtime_task = runtime.and_then(|value| {
        ["user_message", "task"].iter().find_map(|key| {
            value
                .get(*key)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
        })
    });
    let runtime_summary = runtime.and_then(|value| {
        ["summary", "latest_message"].iter().find_map(|key| {
            value
                .get(*key)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
        })
    });
    let summary = if kind == "subagent_run" {
        runtime_label
            .or(runtime_task)
            .or(runtime_summary)
            .unwrap_or(kind)
            .to_owned()
    } else if kind == "system_message" {
        // The model receives the frozen prompt from the internal log, while
        // the presentation projection must never disclose it (it can contain
        // instructions, paths, or credentials). Keep only bounded metadata.
        let language = item
            .pointer("/meta/language")
            .and_then(Value::as_str)
            .unwrap_or("");
        if language.is_empty() {
            "系统提示词（内容已隐藏）".to_string()
        } else {
            format!("系统提示词（内容已隐藏，语言：{language}）")
        }
    } else if content.is_empty() {
        if kind == "tool_call" {
            // 工具行必须展示调用的是什么工具，而不是回退成裸 kind。
            tool.unwrap_or(kind).to_owned()
        } else if kind == "assistant_message" {
            // 工具调用轮的助手条目没有正文；思考摘要是该条目的可视主体。
            item.get("reasoning")
                .or_else(|| item.get("reasoning_content"))
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .unwrap_or(kind)
                .to_owned()
        } else {
            item.get("summary")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .unwrap_or(kind)
                .to_owned()
        }
    } else {
        content.to_owned()
    };
    let subagent = if kind == "subagent_run" {
        subagent_runtime_projection(item).or_else(|| {
            runtime.map(|runtime| {
                let mut out = serde_json::Map::new();
                for field in ["label", "spawn_label", "status", "run_id", "session_id"] {
                    if let Some(value) = runtime
                        .get(field)
                        .and_then(Value::as_str)
                        .filter(|v| !v.is_empty())
                    {
                        out.insert(field.into(), Value::String(preview(value, 128)));
                    }
                }
                for field in ["user_message", "task", "summary", "latest_message"] {
                    if let Some(value) = runtime
                        .get(field)
                        .and_then(Value::as_str)
                        .filter(|v| !v.is_empty())
                    {
                        out.insert(field.into(), Value::String(preview(value, 1024)));
                    }
                }
                for field in ["started_time", "finished_time", "updated_time", "elapsed_s"] {
                    if let Some(value) = runtime.get(field).filter(|v| v.is_number()) {
                        out.insert(field.into(), value.clone());
                    }
                }
                Value::Object(out)
            })
        })
    } else {
        None
    };
    let started_at = if kind == "subagent_run" {
        runtime
            .and_then(|value| value.get("started_time").and_then(Value::as_f64))
            .unwrap_or(created)
    } else {
        item.get("started_at")
            .and_then(Value::as_f64)
            .unwrap_or(created)
    };
    let completed_at = if kind == "subagent_run" {
        runtime
            .and_then(|value| value.get("finished_time").and_then(Value::as_f64))
            .or_else(|| (is_running(status) == 0).then_some(updated))
    } else {
        (is_running(status) == 0).then_some(updated)
    };
    TrajectoryViewRow {
        item_id: item_id.into(),
        turn_id: turn_id.into(),
        root_turn_id: root.into(),
        user_round: round,
        item_index: index,
        created_seq: seq,
        revision,
        kind: preview(if model_context { "context" } else { kind }, 48),
        status: preview(status, 48),
        summary: preview(&summary, 256),
        overview_only: false,
        tool_name: tool.or(runtime_label).map(|v| preview(v, 96)),
        model_round: item.get("model_round").and_then(Value::as_i64).unwrap_or(0),
        stats: finite_stats(item),
        started_at,
        completed_at,
        detail: Some(BodyRef {
            body_id: body_id(item_id, "detail"),
            revision,
            bytes: 0,
        }),
        subagent,
    }
}

pub(crate) fn subagent_label(item: &Value) -> Option<&str> {
    item.get("runtime")
        .and_then(Value::as_object)
        .and_then(|runtime| {
            ["label", "spawn_label", "title"].iter().find_map(|key| {
                runtime
                    .get(*key)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
            })
        })
}

/// Small bounded runtime projection used by both source fallback and old
/// persisted trajectory rows. Never copies child result/config payloads.
pub(crate) fn subagent_runtime_projection(item: &Value) -> Option<Value> {
    let runtime = item.get("runtime")?.as_object()?;
    let mut out = serde_json::Map::new();
    for field in [
        "label",
        "spawn_label",
        "title",
        "status",
        "run_id",
        "session_id",
    ] {
        if let Some(value) = runtime
            .get(field)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            out.insert(field.into(), Value::String(preview(value, 128)));
        }
    }
    for field in ["user_message", "task", "summary", "latest_message"] {
        if let Some(value) = runtime
            .get(field)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            out.insert(field.into(), Value::String(preview(value, 1024)));
        }
    }
    for field in ["started_time", "finished_time", "updated_time", "elapsed_s"] {
        if let Some(value) = runtime.get(field).filter(|v| v.is_number()) {
            out.insert(field.into(), value.clone());
        }
    }
    Some(Value::Object(out))
}

pub(crate) fn redact_internal_context(item: &mut Value) {
    if let Some(map) = item.as_object_mut() {
        map.remove("content");
        map.remove("model_content");
        map.insert("content_redacted".into(), Value::Bool(true));
        map.insert(
            "summary".into(),
            Value::String("系统提示词（内容已隐藏）".into()),
        );
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
        .or_else(|| item.get("messageStats"))
        .or_else(|| item.pointer("/meta/message_stats"))
        .or_else(|| item.pointer("/meta/messageStats"))?
        .as_object()?;
    // Keep the small nested usage payloads intact.  The previous projection
    // admitted only top-level numbers/booleans, which silently removed
    // `round_usage`/`usage` and left the bubble's token metric as `--` after a
    // reload even though the server had persisted the counts.
    let mut fields = serde_json::Map::new();
    let mut scalar_count = 0usize;
    let finite_scalar = |value: &Value| {
        value.is_number()
            || value.is_boolean()
            || value
                .as_str()
                .and_then(|text| text.trim().parse::<f64>().ok())
                .is_some_and(|number| number.is_finite() && number >= 0.0)
    };
    for (key, value) in source.iter() {
        let keep_scalar = finite_scalar(value);
        let keep_usage = matches!(key.as_str(), "usage" | "round_usage" | "roundUsage")
            && value.as_object().is_some();
        if !keep_scalar && !keep_usage {
            continue;
        }
        if keep_scalar {
            scalar_count = scalar_count.saturating_add(1);
            if scalar_count > 64
                && !matches!(
                    key.as_str(),
                    "model_request_count" | "account_credits_consumed" | "tool_calls" | "toolCalls"
                )
            {
                continue;
            }
        }
        if keep_usage {
            let bounded = value
                .as_object()
                .unwrap()
                .iter()
                .filter(|(_, v)| finite_scalar(v))
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
        if fields.contains_key(key) {
            continue;
        }
        let Some(value) = source.get(key).and_then(Value::as_object) else {
            continue;
        };
        let bounded = value
            .iter()
            .filter(|(_, v)| finite_scalar(v))
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
                .and_then(|usage| {
                    usage
                        .get("request_consumed_tokens")
                        .or_else(|| usage.get("total_tokens"))
                })
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
    for field in [
        "run_id",
        "session_id",
        "dispatch_id",
        "run_kind",
        "title",
        "label",
        "status",
    ] {
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
        assert!(row.summary.contains("内容已隐藏"));
        assert!(!row.summary.contains("强大的智能体"));
    }

    #[test]
    fn finite_stats_preserves_bounded_camel_case_usage_values() {
        let stats = finite_stats(&json!({
            "messageStats": {
                "usage": {
                    "inputTokens": "1200",
                    "outputTokens": 340,
                    "reasoningTokens": "12"
                }
            }
        }))
        .expect("message stats should be available");
        assert_eq!(stats["usage"]["inputTokens"], "1200");
        assert_eq!(stats["usage"]["outputTokens"], 340);
        assert_eq!(stats["usage"]["reasoningTokens"], "12");
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
            let item =
                json!({"runtime": {"run_id": format!("run-{index}"), "status": "completed"}});
            if let Some(card) =
                subagent_card(&item, &format!("turn-1:subagent-run-{index}"), index as i64)
            {
                upsert_subagent_card(&mut cards, card);
            }
        }
        assert_eq!(cards.len(), MAX_ROW_SUBAGENTS);
        // 窗口裁剪丢掉最旧的运行，同 key 的进度推进永远保留最新一份。
        assert!(cards.iter().all(|c| c["key"] != "turn-1:subagent-run-1"));
    }

    #[test]
    fn trajectory_rows_project_subagent_runtime_as_a_bounded_detail() {
        let row = project(
            "subagent_run",
            &json!({
                "runtime": {
                    "label": "child worker",
                    "user_message": "inspect the workspace",
                    "summary": "found the requested files",
                    "status": "success",
                    "started_time": 10.0,
                    "finished_time": 14.0,
                    "huge_result": {"must_not": "leak"}
                }
            }),
        );
        assert_eq!(row.summary, "child worker");
        assert_eq!(row.tool_name.as_deref(), Some("child worker"));
        let runtime = row.subagent.expect("subagent projection");
        assert_eq!(runtime["label"], "child worker");
        assert_eq!(runtime["user_message"], "inspect the workspace");
        assert_eq!(runtime["summary"], "found the requested files");
        assert_eq!(runtime["started_time"], 10.0);
        assert!(runtime.get("huge_result").is_none());
    }
}
