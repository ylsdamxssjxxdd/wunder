//! Write-time display projection, shared by both transactional stores.
use serde_json::{json, Value};
use wunder_core::thread_view::{preview, BodyRef, ChatViewRow, TrajectoryViewRow, ViewMessage};

/// Only bounded plan presentation belongs in a chat page; tool arguments and
/// results remain in the process ledger. The item status describes the update
/// operation, while each step owns its own progress status.
pub(crate) fn plan(
    item: &Value,
    id: &str,
    revision: i64,
    created_seq: i64,
) -> Option<wunder_core::thread_view::ThreadPlanView> {
    use wunder_core::thread_view::{ThreadPlanStep, ThreadPlanView};
    let raw = item.get("plan").or_else(|| item.get("steps"))?.as_array()?;
    let mut active = false;
    let steps = raw
        .iter()
        .take(32)
        .filter_map(|entry| {
            let step = entry
                .get("step")
                .or_else(|| entry.get("title"))
                .and_then(Value::as_str)
                .or_else(|| entry.as_str())?
                .trim();
            if step.is_empty() {
                return None;
            }
            let status = entry
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_lowercase()
                .replace(['-', ' '], "_");
            let status = match status.as_str() {
                "completed" | "complete" | "done" => "completed",
                "in_progress" | "inprogress" if !active => {
                    active = true;
                    "in_progress"
                }
                _ => "pending",
            };
            Some(ThreadPlanStep {
                step: preview(step, 256),
                status: status.into(),
            })
        })
        .collect::<Vec<_>>();
    if steps.is_empty() {
        return None;
    }
    Some(ThreadPlanView {
        item_id: id.into(),
        revision,
        created_seq,
        explanation: preview(
            item.get("explanation")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim(),
            512,
        ),
        steps,
    })
}

pub(crate) fn body_id(item_id: &str, field: &str) -> String {
    format!("{item_id}|{field}")
}

pub(crate) fn question_panel(
    payload: &Value,
) -> Option<wunder_core::thread_view::ThreadQuestionPanel> {
    use wunder_core::thread_view::{ThreadQuestion, ThreadQuestionOption, ThreadQuestionPanel};
    let raw = payload.get("questions").and_then(Value::as_array)?;
    let questions = raw
        .iter()
        .take(4)
        .filter_map(|question| {
            let options = question
                .get("options")?
                .as_array()?
                .iter()
                .take(4)
                .filter_map(|option| {
                    let label = option
                        .get("label")
                        .and_then(Value::as_str)
                        .or_else(|| option.as_str())?
                        .trim();
                    if label.is_empty() {
                        return None;
                    }
                    Some(ThreadQuestionOption {
                        label: preview(label, 256),
                        description: preview(
                            option
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .trim(),
                            512,
                        ),
                        recommended: option
                            .get("recommended")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect::<Vec<_>>();
            if options.is_empty() {
                return None;
            }
            Some(ThreadQuestion {
                question: preview(
                    question
                        .get("question")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim(),
                    1024,
                ),
                multiple: question
                    .get("multiple")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                options,
            })
        })
        .collect::<Vec<_>>();
    if questions.is_empty() {
        return None;
    }
    Some(ThreadQuestionPanel {
        questions,
        keep_open: payload
            .get("keep_open")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        status: match payload.get("status").and_then(Value::as_str) {
            Some("answered") => "answered",
            Some("dismissed") => "dismissed",
            _ => "pending",
        }
        .into(),
    })
}

/// Recover a question panel from the bounded durable item shape.  Depending on
/// the writer/version it may live in assistant `meta.panel`, directly in a
/// tool result, or under `data`/`payload` as the compact tool carrier.
pub(crate) fn extract_question_panel(
    payload: &Value,
) -> Option<wunder_core::thread_view::ThreadQuestionPanel> {
    fn visit(
        value: &Value,
        inherited_status: Option<&str>,
        depth: usize,
    ) -> Option<wunder_core::thread_view::ThreadQuestionPanel> {
        if depth > 6 || !value.is_object() {
            return None;
        }
        let status = value
            .get("status")
            .or_else(|| value.get("state"))
            .and_then(Value::as_str)
            .or(inherited_status);
        if value.get("questions").and_then(Value::as_array).is_some() {
            let mut panel = question_panel(value)?;
            panel.status = match status
                .unwrap_or("pending")
                .trim()
                .to_ascii_lowercase()
                .as_str()
            {
                "answered" | "completed" => "answered",
                "dismissed" | "cancelled" | "canceled" => "dismissed",
                _ => "pending",
            }
            .into();
            return Some(panel);
        }
        for key in [
            "panel",
            "question_panel",
            "questionPanel",
            "data",
            "result",
            "payload",
            "meta",
        ] {
            if let Some(nested) = value.get(key) {
                if let Some(panel) = visit(nested, status, depth + 1) {
                    return Some(panel);
                }
            }
        }
        None
    }
    visit(payload, None, 0)
}

pub(crate) fn settle_question_panel(row: &mut ChatViewRow) {
    if let Some(panel) = row.assistant.question_panel.as_mut() {
        if matches!(
            row.status.as_str(),
            "cancelled" | "failed" | "rejected" | "stopped" | "interrupted"
        ) {
            panel.status = "dismissed".into();
        } else if row.status == "completed" {
            panel.status = "answered".into();
        }
    }
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

/// Bounded attachment cards for a user message. Source items record
/// `content_type`; the view contract exposes it as `mime_type`.
pub(crate) fn user_attachments(attachments: &[Value]) -> Vec<Value> {
    attachments
        .iter()
        .take(8)
        .filter_map(|a| {
            let mut out = serde_json::Map::new();
            for key in ["name", "public_path"] {
                if let Some(s) = a.get(key).and_then(Value::as_str) {
                    out.insert(key.into(), json!(preview(s, 128)));
                }
            }
            let mime = a
                .get("mime_type")
                .or_else(|| a.get("content_type"))
                .and_then(Value::as_str);
            if let Some(s) = mime {
                out.insert("mime_type".into(), json!(preview(s, 128)));
            }
            (!out.is_empty()).then_some(Value::Object(out))
        })
        .collect()
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

/// Automatic context compaction is stored as a system replacement message so
/// it can reset the model history without becoming an assistant reply.  The
/// presentation projection treats only that explicitly marked system row as a
/// workflow activity; ordinary system prompts remain internal context.
pub(crate) fn is_compaction_system_message(kind: &str, item: &Value) -> bool {
    if kind != "system_message" {
        return false;
    }
    let meta = item.get("meta").and_then(Value::as_object);
    matches!(
        meta.and_then(|value| value.get("type"))
            .and_then(Value::as_str),
        Some("compaction_summary")
    ) || meta
        .and_then(|value| value.get("compaction_id"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
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
    } else if is_compaction_system_message(kind, item) {
        "上下文压缩".to_string()
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
    } else if kind == "compaction" || kind == "compacted" {
        [
            "summary_text",
            "summaryText",
            "summary_model_output",
            "summaryModelOutput",
            "summary",
            "content",
        ]
        .iter()
        .find_map(|key| {
            item.get(*key)
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty() && *value != "history")
        })
        .unwrap_or("上下文压缩")
        .to_owned()
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
    let mut started_at = if kind == "subagent_run" {
        runtime
            .and_then(|value| value.get("started_time").and_then(Value::as_f64))
            .unwrap_or(created)
    } else {
        item.get("started_at")
            .and_then(Value::as_f64)
            .unwrap_or(created)
    };
    let mut completed_at = if kind == "subagent_run" {
        runtime
            .and_then(|value| value.get("finished_time").and_then(Value::as_f64))
            .or_else(|| (is_running(status) == 0).then_some(updated))
    } else {
        (is_running(status) == 0).then_some(updated)
    };

    // Canonical item rows keep their bookkeeping timestamps (`created` and
    // `updated`) at the admission/commit edge.  For streamed model and tool
    // items those timestamps are often equal, even though the payload carries
    // the actual request/tool duration.  Passing the bookkeeping instant
    // through as both ends collapses the corresponding timeline span to a
    // one-pixel marker after reload.  Recover the own-step interval here so
    // every consumer (web, desktop and CLI) receives one consistent range.
    // Explicit start/end fields always win; this fallback only runs when the
    // canonical row has no usable duration of its own.
    if kind != "subagent_run"
        && completed_at.is_some()
        && item.get("started_at").and_then(Value::as_f64).is_none()
        && item.get("completed_at").and_then(Value::as_f64).is_none()
    {
        if let Some(duration_s) = trajectory_duration_seconds(item, kind) {
            let end = completed_at.unwrap_or(updated);
            if end.is_finite() && duration_s.is_finite() && duration_s > 0.0 {
                started_at = end - duration_s;
                completed_at = Some(end);
            }
        }
    }
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
        request_token_breakdown: request_token_breakdown(item),
        started_at,
        completed_at,
        detail: Some(BodyRef {
            body_id: body_id(item_id, "detail"),
            revision,
            bytes: 0,
        }),
        subagent,
        diagnostics: matches!(
            kind,
            "assistant_message" | "tool_call" | "terminal" | "compaction" | "subagent_run"
        )
        .then(|| crate::services::thread_log::diagnostics::project(item, status))
        .flatten(),
    }
}

/// Return the bounded own-step duration recorded in a canonical item payload.
/// Provider usage metadata can contain a cumulative interaction duration, so
/// it is deliberately excluded here.  The resulting interval is only used to
/// paint the trajectory overview; no request content is copied into the view.
fn trajectory_duration_seconds(item: &Value, kind: &str) -> Option<f64> {
    fn non_negative(value: Option<&Value>) -> Option<f64> {
        let value = value
            .and_then(Value::as_f64)
            .or_else(|| value?.as_str()?.trim().parse::<f64>().ok())?;
        value.is_finite().then_some(value.max(0.0))
    }
    fn first_non_negative(item: &Value, keys: &[&str]) -> Option<f64> {
        keys.iter().find_map(|key| non_negative(item.get(*key)))
    }

    if kind == "assistant_message" {
        // A persisted message_stats value is the preferred own-request metric
        // for providers that report it.  `interaction_duration_s` is
        // intentionally absent because it spans the whole user turn.
        for source in [
            item.get("message_stats"),
            item.get("messageStats"),
            item.pointer("/meta/message_stats"),
            item.pointer("/meta/messageStats"),
        ] {
            if let Some(source) = source {
                if let Some(value) = first_non_negative(
                    source,
                    &[
                        "model_duration_s",
                        "request_duration_s",
                        "duration_s",
                        "durationSeconds",
                    ],
                ) {
                    if value > 0.0 {
                        return Some(value);
                    }
                }
            }
        }
        let prefill = first_non_negative(item, &["prefill_duration_s"]);
        let decode = first_non_negative(item, &["decode_duration_s"]);
        if prefill.is_some() || decode.is_some() {
            let value = prefill.unwrap_or(0.0) + decode.unwrap_or(0.0);
            if value > 0.0 {
                return Some(value);
            }
        }
        if let Some(timing) = item.get("stream_timing") {
            let prefill_ms = first_non_negative(timing, &["prefill_ms"]);
            let decode_ms = first_non_negative(timing, &["decode_ms"]);
            let value = (prefill_ms.unwrap_or(0.0) + decode_ms.unwrap_or(0.0)) / 1000.0;
            if value > 0.0 {
                return Some(value);
            }
        }
    }

    // Tool result metadata records elapsed milliseconds on the result event;
    // accepting a top-level duration as well keeps imported ledgers useful.
    let milliseconds = first_non_negative(item, &["duration_ms", "durationMs"]).or_else(|| {
        item.pointer("/meta")
            .and_then(|meta| first_non_negative(meta, &["duration_ms", "durationMs"]))
    });
    milliseconds
        .filter(|value| *value > 0.0)
        .map(|value| value / 1000.0)
}

/// Keep the trajectory page compact and safe: this local estimate is numeric
/// only, and never carries any part of the request body or provider settings.
fn request_token_breakdown(item: &Value) -> Option<Value> {
    let source = item.get("request_token_breakdown")?.as_object()?;
    let keys = [
        "native_tool_tokens",
        "system_prompt_tokens",
        "history_tokens",
        "latest_user_tokens",
        "image_tokens",
        "reasoning_tokens",
        "tool_call_tokens",
        "tool_call_id_tokens",
        "message_envelope_tokens",
        "fixed_protocol_tokens",
        "message_tokens",
        "estimated_input_tokens",
        "tool_count",
        "message_count",
    ];
    let mut out = serde_json::Map::new();
    for key in keys {
        let value = source.get(key)?;
        if !(value.is_i64() || value.is_u64()) {
            return None;
        }
        out.insert(key.into(), value.clone());
    }
    Some(Value::Object(out))
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
    // The compaction replacement is user-visible execution history.  It is
    // stored as a system message only because that is how it replaces model
    // context; keep its bounded summary available to the process-card view.
    if is_compaction_system_message("system_message", item) {
        if let Some(map) = item.as_object_mut() {
            map.remove("model_content");
            // This is the model's replacement context, not part of the
            // summary detail. Do not copy the entire retained history into
            // the presentation body for a single process entry.
            if let Some(meta) = map.get_mut("meta").and_then(Value::as_object_mut) {
                meta.remove("replacement_history");
            }
        }
        return;
    }
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
    // Canonical assistant rows written by the executor carry the compact
    // `message_stats` object in `meta`.  Older/streamed rows (and the
    // append-chat snapshots used by the service) carry the same timing and
    // usage fields directly on the assistant payload.  Read both shapes here;
    // dropping the direct shape makes a re-opened bubble lose all timing,
    // speed and quota metrics even though they are still persisted.
    let persisted_source = [
        item.get("message_stats"),
        item.get("messageStats"),
        item.pointer("/meta/message_stats"),
        item.pointer("/meta/messageStats"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_object);
    // Do not turn arbitrary trajectory rows (especially tool result payloads)
    // into message metrics merely because they contain a numeric field.  A
    // direct source is valid only for the assistant-output schema.
    let direct_source = item.as_object().filter(|source| {
        [
            "prefill_duration_s",
            "decode_duration_s",
            "decode_output_tokens",
            "decode_tokens",
            "decode_speed_tps",
            "prefill_tokens",
            "prefill_speed_tps",
            "stream_timing",
            "usage",
        ]
        .iter()
        .any(|key| source.contains_key(*key))
    });
    let source = persisted_source.or(direct_source)?;
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
    // Preserve direct output metadata when a compact message_stats object is
    // present as well.  The canonical row may combine the two shapes: timing
    // is emitted on the llm_output payload while quota is emitted in meta.
    if persisted_source.is_some() {
        for key in [
            "interaction_duration_s",
            "prefill_duration_s",
            "prefill_duration_total_s",
            "decode_duration_s",
            "decode_duration_total_s",
            "decode_output_tokens",
            "decode_tokens",
            "decode_speed_tps",
            "prefill_speed_tps",
            "visible_decode_tokens",
            "visible_decode_duration_s",
            "visible_decode_speed_tps",
            "visible_decode_measured",
            "avg_model_round_speed_tps",
            "avg_model_round_decode_speed_tps",
            "avg_model_round_speed_rounds",
            "ttft_ms",
            "context_occupancy_tokens",
            "contextTokens",
            "model_request_count",
            "account_credits_consumed",
            "tool_calls",
            "toolCalls",
        ] {
            if fields.contains_key(key) {
                continue;
            }
            let Some(value) = item.get(key) else { continue };
            if finite_scalar(value) {
                fields.insert(key.into(), value.clone());
            }
        }
        if !fields.contains_key("stream_timing") {
            if let Some(value) = item.get("stream_timing").and_then(Value::as_object) {
                let bounded = value
                    .iter()
                    .filter(|(_, value)| finite_scalar(value))
                    .take(16)
                    .map(|(key, value)| (preview(key, 64), value.clone()))
                    .collect::<serde_json::Map<_, _>>();
                fields.insert("stream_timing".into(), Value::Object(bounded));
            }
        }
    }
    // Some providers expose tool calls as the full OpenAI array on the
    // assistant item instead of the persisted numeric `toolCalls` counter.
    // Keep only its bounded count for the compact message footer.
    if !fields.contains_key("toolCalls") && !fields.contains_key("tool_calls") {
        if let Some(count) = item
            .get("tool_calls")
            .or_else(|| source.get("tool_calls"))
            .and_then(Value::as_array)
            .map(|v| v.len())
        {
            fields.insert("toolCalls".into(), json!(count));
            fields.insert("tool_calls".into(), json!(count));
        }
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
    use wunder_core::thread_view::{ThreadQuestion, ThreadQuestionPanel};

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
    fn trajectory_recovers_assistant_span_from_stream_durations() {
        let row = project(
            "assistant_message",
            &json!({
                "content": "完成",
                "prefill_duration_s": 0.8,
                "decode_duration_s": 1.2,
            }),
        );
        assert_eq!(row.completed_at, Some(1.0));
        assert!((row.started_at - (-1.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn trajectory_recovers_tool_span_from_duration_metadata() {
        let row = project(
            "tool_call",
            &json!({
                "tool": "write_file",
                "meta": {"duration_ms": 250},
            }),
        );
        assert_eq!(row.completed_at, Some(1.0));
        assert!((row.started_at - 0.75).abs() < f64::EPSILON);
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
    fn compaction_system_rows_are_distinguished_from_system_prompts() {
        let mut compaction = json!({
            "content":"Retained compacted history.",
            "meta":{"type":"compaction_summary","compaction_id":"cmp-fixture",
                "replacement_history":[{"content":"internal context"}]}
        });
        assert!(is_compaction_system_message("system_message", &compaction));
        redact_internal_context(&mut compaction);
        assert_eq!(compaction["content"], "Retained compacted history.");
        assert!(compaction.pointer("/meta/replacement_history").is_none());

        let mut prompt = json!({"content":"internal prompt","meta":{"type":"system_prompt"}});
        assert!(!is_compaction_system_message("system_message", &prompt));
        redact_internal_context(&mut prompt);
        assert!(prompt.get("content").is_none());
    }

    #[test]
    fn question_panel_extraction_unwraps_carrier_and_preserves_status() {
        let pending = extract_question_panel(&json!({
            "action": "question_panel",
            "state": "awaiting_input",
            "data": {"questions": [{"question": "Choose", "options": [{"label": "A"}]}]}
        }))
        .expect("carrier panel");
        assert_eq!(pending.status, "pending");
        assert_eq!(pending.questions[0].options[0].label, "A");

        let answered = extract_question_panel(&json!({
            "data": {"state": "answered", "panel": {
                "questions": [{"question": "Choose", "options": [{"label": "A"}]}]
            }}
        }))
        .expect("answered panel");
        assert_eq!(answered.status, "answered");
    }

    #[test]
    fn question_panel_settles_from_terminal_turn_status() {
        let mut row = ChatViewRow {
            status: "cancelled".into(),
            assistant: ViewMessage {
                question_panel: Some(ThreadQuestionPanel {
                    questions: vec![ThreadQuestion::default()],
                    keep_open: true,
                    status: "pending".into(),
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        settle_question_panel(&mut row);
        assert_eq!(
            row.assistant.question_panel.as_ref().unwrap().status,
            "dismissed"
        );

        row.status = "completed".into();
        row.assistant.question_panel.as_mut().unwrap().status = "pending".into();
        settle_question_panel(&mut row);
        assert_eq!(
            row.assistant.question_panel.as_ref().unwrap().status,
            "answered"
        );
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
    fn finite_stats_recovers_direct_assistant_output_metadata() {
        let stats = finite_stats(&json!({
            "content": "done",
            "prefill_duration_s": 1.2,
            "decode_duration_s": 0.8,
            "decode_output_tokens": 40,
            "decode_speed_tps": 50.0,
            "usage": {"input_tokens": 100, "output_tokens": 40, "total_tokens": 140},
            "tool_calls": [{"id": "call-1"}]
        }))
        .expect("direct assistant metadata should be available");
        assert_eq!(stats["prefill_duration_s"], json!(1.2));
        assert_eq!(stats["decode_duration_s"], json!(0.8));
        assert_eq!(stats["decode_output_tokens"], json!(40));
        assert_eq!(stats["toolCalls"], json!(1));
        assert_eq!(stats["usage"]["total_tokens"], json!(140));
    }

    #[test]
    fn trajectory_projects_only_numeric_request_token_breakdown() {
        let row = project(
            "assistant_message",
            &json!({
                "content":"response", "request_token_breakdown": {
                    "native_tool_tokens":12,"system_prompt_tokens":8,"history_tokens":4,
                    "latest_user_tokens":2,"image_tokens":0,"reasoning_tokens":0,
                    "tool_call_tokens":0,"tool_call_id_tokens":0,"message_envelope_tokens":8,
                    "fixed_protocol_tokens":16,"message_tokens":22,"estimated_input_tokens":58,
                    "tool_count":1,"message_count":2,"unexpected":"omitted"
                }
            }),
        );
        let breakdown = row
            .request_token_breakdown
            .expect("numeric request estimate");
        assert_eq!(breakdown["estimated_input_tokens"], 58);
        assert!(breakdown.get("unexpected").is_none());
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
