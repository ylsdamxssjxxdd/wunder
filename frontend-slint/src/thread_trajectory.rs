//! Thread trajectory controller: folds the thread-log snapshot into
//! turn → group → cell ledger rows plus the three-lane timeline, mirroring the
//! web trajectory model one rule at a time. All text is projected in Rust so
//! Slint only lays out bounded rows, and every row and span is keyed by the
//! global record index so hover and selection survive filtering.
use crate::{I18n, MainWindow, TrajBoundary, TrajRow, TrajSpan, TrajTab, TrajTick, TrajTiming, TrajSection};
use chrono::{Local, TimeZone};
use serde_json::{json, Value};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::sync_channel;
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wunder_desktop::NativeDesktop;

// Ledger row kinds; mirrors TrajRow.kind in theme.slint.
const KIND_SYSTEM: i32 = 0;
const KIND_USER: i32 = 1;
const KIND_CONTEXT: i32 = 2;
const KIND_COMPACTED: i32 = 3;
const KIND_MESSAGE: i32 = 4;
const KIND_TOOL: i32 = 5;
const KIND_SUBTOOL: i32 = 6;

// Detail tabs; mirrors TrajTab.kind usage in thread_trajectory.slint.
const TAB_OVERVIEW: i32 = 0;
const TAB_RAW: i32 = 1;
const TAB_PARAMS: i32 = 2;
const TAB_RESULT: i32 = 3;
const TAB_SCHEMA: i32 = 4;
const TAB_TIMING: i32 = 5;

const GROUP_MESSAGES: &str = "messages";
const TEXT_PREVIEW_SOURCE_LIMIT: usize = 2048;
const TEXT_PREVIEW_LIMIT: usize = 512;
const SECTION_BODY_LIMIT: usize = 16_384;
const ROW_PX: f32 = 30.0;
const ROW_SUMMARY_PX: f32 = 20.0;
const USER_TURN_KEYS: [&str; 4] = ["user_turn_index", "user_round", "turn_index", "turn"];
const STARTED_KEYS: [&str; 3] = ["started_at", "start_time", "created_at"];
const COMPLETED_KEYS: [&str; 3] = ["completed_at", "end_time", "finished_at"];
const CONTEXT_RAW_KINDS: [&str; 8] = [
    "queue", "plan", "approval", "terminal", "context", "note", "status", "context_message",
];
const TICK_STEPS_MS: [f64; 14] = [
    100.0, 250.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 30_000.0, 60_000.0, 120_000.0,
    300_000.0, 600_000.0, 1_800_000.0, 3_600_000.0,
];

#[derive(Default, Clone, Copy)]
struct Usage {
    input: Option<f64>,
    cache_read: Option<f64>,
    cache_write: Option<f64>,
    output: Option<f64>,
    think: Option<f64>,
}

impl Usage {
    fn any(self) -> bool {
        self.input.is_some()
            || self.cache_read.is_some()
            || self.cache_write.is_some()
            || self.output.is_some()
            || self.think.is_some()
    }
    fn add(&mut self, other: Usage) {
        for (target, value) in [
            (&mut self.input, other.input),
            (&mut self.cache_read, other.cache_read),
            (&mut self.cache_write, other.cache_write),
            (&mut self.output, other.output),
            (&mut self.think, other.think),
        ] {
            if let Some(value) = value {
                *target = Some(target.unwrap_or(0.0) + value);
            }
        }
    }
}

struct Metrics {
    step_start: Option<f64>,
    first_token: Option<f64>,
    completed: Option<f64>,
    output_tokens: Option<f64>,
}

struct Cell {
    index: usize,
    record_id: String,
    kind: i32,
    text: String,
    input_detail: Option<String>,
    output_detail: Option<String>,
    thinking_detail: Option<String>,
    schema_detail: Option<String>,
    result: Option<String>,
    tool_name: Option<String>,
    is_error: bool,
    tool_call_only: bool,
    time_seconds: Option<f64>,
    started_at: Option<f64>,
    metrics: Option<Metrics>,
    usage: Option<Usage>,
    raw: Value,
}

struct Group {
    /// Language-neutral identity ("messages" / "step:N" / "compaction:N");
    /// localized labels are applied at projection time.
    identity: String,
    cells: Vec<usize>,
    step: Option<i64>,
    compaction: bool,
}

struct TurnModel {
    turn: Option<i64>,
    groups: Vec<Group>,
}

struct Request {
    number: usize,
    turn: Option<i64>,
    identity: String,
    started_at: Option<f64>,
    completed_at: Option<f64>,
    usage: Option<Usage>,
    cumulative: Usage,
    assistant_index: usize,
    first_record: usize,
}

struct Record {
    turn: Option<i64>,
    group: String,
    turn_start: bool,
    cell: usize,
}

struct SpanBase {
    start: f64,
    end: f64,
    record: usize,
    kind: i32,
    lane: usize,
    ttft: Option<f64>,
}

/// Parsed snapshot plus derived projections, built as a unit on the worker
/// thread and handed to the UI state in one piece.
struct SnapshotModel {
    cells: Vec<Cell>,
    turns: Vec<TurnModel>,
    records: Vec<Record>,
    requests: Vec<Request>,
    spans: Vec<SpanBase>,
    // (turn, wall-clock start in ms, first record index)
    boundaries: Vec<(i64, f64, usize)>,
    tl_start: f64,
    tl_end: f64,
}

#[derive(Default)]
struct State {
    session: String,
    model: Option<SnapshotModel>,
    view_start: f64,
    view_width: f64,
    selected: i32,
    tab: i32,
    show_duration: bool,
    show_turns: bool,
    show_calls: bool,
    query: String,
    collapsed_turns: HashSet<i64>,
    collapsed_assistants: HashSet<String>,
}

impl State {
    fn reset(&mut self, session: String) {
        let show_duration = self.show_duration;
        *self = State {
            session,
            show_duration,
            ..State::default()
        };
    }
    fn identity_key(turn: Option<i64>, group: &str) -> String {
        format!("{}\u{0}{}", turn.map(|t| t.to_string()).unwrap_or_else(|| "none".into()), group)
    }
}

fn to_model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

// ------------------------------------------------------------------ */
// Snapshot parsing (pure; runs on the worker thread)                  */
// ------------------------------------------------------------------ */

fn payload_of(item: &Value) -> &Value {
    item.get("payload").filter(|v| v.is_object()).unwrap_or(&Value::Null)
}

fn meta_of<'a>(payload: &'a Value) -> &'a Value {
    payload.get("meta").filter(|v| v.is_object()).unwrap_or(&Value::Null)
}

fn raw_kind_of<'a>(item: &'a Value, payload: &'a Value) -> &'a str {
    if let Some(kind) = item.get("kind").and_then(Value::as_str) {
        if !kind.is_empty() {
            return kind;
        }
    }
    if let Some(event) = payload.get("event_type").and_then(Value::as_str) {
        if !event.is_empty() {
            return event;
        }
    }
    meta_of(payload).get("type").and_then(Value::as_str).unwrap_or("")
}

/// 时间字段统一为毫秒；秒级数值按 1e11 阈值启发式放大。
fn timestamp_ms(value: &Value) -> Option<f64> {
    if let Some(number) = value.as_f64() {
        if number.is_finite() {
            return Some(if number.abs() < 1e11 { number * 1000.0 } else { number });
        }
    }
    if let Some(text) = value.as_str() {
        if !text.is_empty() {
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(text) {
                return Some(parsed.timestamp_millis() as f64);
            }
        }
    }
    None
}

fn first_time(source: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| timestamp_ms(source.get(*key)?))
}

fn content_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .map(|block| match block {
                Value::String(text) => text.clone(),
                Value::Object(_) => block
                    .get("text")
                    .or_else(|| block.get("content"))
                    .or_else(|| block.get("thinking"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                _ => String::new(),
            })
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) => value
            .get("text")
            .or_else(|| value.get("content"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default()),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn summarize_tool_result(payload: &Value) -> String {
    if let Some(data) = payload.get("data") {
        match data {
            Value::String(text) => return text.clone(),
            Value::Object(_) => {
                for key in ["result", "output", "content", "text"] {
                    if let Some(nested) = data.get(key).and_then(Value::as_str) {
                        return nested.to_string();
                    }
                }
                if let Some(summary) = data.get("summary").and_then(Value::as_str) {
                    return summary.to_string();
                }
            }
            _ => {}
        }
    }
    let fallback = payload
        .get("result")
        .or_else(|| payload.get("output"))
        .or_else(|| payload.get("content"));
    fallback.map(content_to_text).unwrap_or_default()
}

fn extract_usage(payload: &Value) -> Option<Usage> {
    let mut sources: Vec<&Value> = Vec::new();
    if let Some(stats) = payload.get("stats").filter(|v| v.is_object()) {
        sources.push(stats);
    }
    if let Some(stats) = meta_of(payload).get("message_stats").filter(|v| v.is_object()) {
        sources.push(stats);
    }
    if let Some(usage) = payload.get("usage").filter(|v| v.is_object()) {
        sources.push(usage);
    }
    if sources.is_empty() {
        return None;
    }
    let pick = |keys: &[&str]| {
        for source in &sources {
            for key in keys {
                if let Some(value) = source.get(*key).and_then(Value::as_f64) {
                    if value.is_finite() {
                        return Some(value);
                    }
                }
            }
        }
        None
    };
    let usage = Usage {
        input: pick(&["input_tokens", "prompt_tokens"]),
        cache_read: pick(&["cached_input_tokens", "cache_read_input_tokens", "cached_tokens"]),
        cache_write: pick(&["cache_creation_input_tokens"]),
        output: pick(&["output_tokens", "completion_tokens"]),
        think: pick(&["reasoning_tokens"]),
    };
    usage.any().then_some(usage)
}

fn resolve_kind(raw_kind: &str, payload: &Value) -> i32 {
    if meta_of(payload).get("type").and_then(Value::as_str) == Some("system_prompt") {
        return KIND_SYSTEM;
    }
    match raw_kind {
        "assistant_message" | "assistant" => KIND_MESSAGE,
        "subagent" | "subagent_message" => KIND_SUBTOOL,
        "tool_message" | "tool_call" | "tool_result" | "tool" => KIND_TOOL,
        "compaction" | "compacted" | "compaction_summary" => KIND_COMPACTED,
        "user_message" | "user" => KIND_USER,
        "system_message" | "system" => KIND_SYSTEM,
        other if CONTEXT_RAW_KINDS.contains(&other) => KIND_CONTEXT,
        _ => KIND_CONTEXT,
    }
}

/// 有界纯文本预览：手写剥离 markdown 标记，避免为预览引入正则依赖。
fn markdown_preview_text(text: &str) -> String {
    let bounded: String = if text.chars().count() > TEXT_PREVIEW_SOURCE_LIMIT {
        text.chars().take(TEXT_PREVIEW_SOURCE_LIMIT).collect::<String>() + "…"
    } else {
        text.to_string()
    };
    let mut plain = String::with_capacity(bounded.len());
    let mut in_fence = false;
    for line in bounded.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            plain.push_str(line);
            plain.push('\n');
            continue;
        }
        let mut work = strip_inline_markup(line);
        let head = work.trim_start();
        if head.starts_with('#') {
            work = head.trim_start_matches('#').trim_start().to_string();
        } else if let Some(rest) = head.strip_prefix('>') {
            work = rest.strip_prefix(' ').unwrap_or(rest).to_string();
        } else {
            for marker in ["- ", "* ", "+ "] {
                if let Some(rest) = head.strip_prefix(marker) {
                    work = rest.to_string();
                    break;
                }
            }
            let head = work.trim_start();
            let digits = head.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 {
                let rest = &head[digits..];
                if let Some(after) = rest.strip_prefix(". ") {
                    work = after.to_string();
                }
            }
        }
        plain.push_str(&work);
        plain.push('\n');
    }
    let plain = plain.replace("***", "").replace("**", "").replace("__", "").replace("~~", "");
    if plain.chars().count() > TEXT_PREVIEW_LIMIT {
        plain.chars().take(TEXT_PREVIEW_LIMIT).collect::<String>() + "…"
    } else {
        plain.trim().to_string()
    }
}

fn strip_inline_markup(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut index = 0usize;
    while index < line.len() {
        let rest = &line[index..];
        if let Some(remaining) = rest.strip_prefix("![") {
            // image: keep the alt text
            if let Some(close) = remaining.find(']') {
                out.push_str(&remaining[..close]);
                let after = remaining[close..].strip_prefix("](");
                let advance = match after.and_then(|paren| paren.find(')')) {
                    Some(end) => close + 1 + 1 + end + 1,
                    None => close + 1,
                };
                index += 2 + advance;
                continue;
            }
        }
        if let Some(remaining) = rest.strip_prefix('[') {
            // link: keep the label
            if let Some(close) = remaining.find("](") {
                out.push_str(&remaining[..close]);
                let after = &remaining[close + 1..];
                let advance = after.find(')').map(|end| end + 1).unwrap_or(after.len());
                index += 1 + close + 1 + advance;
                continue;
            }
        }
        if rest.starts_with('<') {
            if let Some(close) = rest.find('>') {
                index += close + 1;
                continue;
            }
        }
        if rest.starts_with('`') {
            index += 1;
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn parse_duration_cell(payload: &Value, started_at: Option<f64>, completed_at: Option<f64>) -> Option<f64> {
    if let (Some(start), Some(end)) = (started_at, completed_at) {
        return Some(((end - start) / 1000.0).max(0.0));
    }
    let prefill = payload.get("prefill_duration_s").and_then(Value::as_f64);
    let decode = payload.get("decode_duration_s").and_then(Value::as_f64);
    match (prefill, decode) {
        (Some(prefill), Some(decode)) if prefill.is_finite() && decode.is_finite() => {
            Some(prefill + decode)
        }
        _ => None,
    }
}

fn non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn build_cell(item: &Value, payload: &Value, kind: i32, index: usize) -> Cell {
    let status = item.get("status").and_then(Value::as_str).unwrap_or("");
    let record_id = item.get("item_id").and_then(Value::as_str).unwrap_or_default().to_string();
    let created_ms =
        first_time(item, &["created_time"]).or_else(|| first_time(payload, &["created_time"]));
    let updated_ms = first_time(item, &["updated_time"]);
    let started_at = first_time(payload, &STARTED_KEYS).or(created_ms);
    let completed_at = first_time(payload, &COMPLETED_KEYS).or(updated_ms);
    let usage = extract_usage(payload);

    let mut cell = Cell {
        index,
        record_id,
        kind,
        text: String::new(),
        input_detail: None,
        output_detail: None,
        thinking_detail: None,
        schema_detail: None,
        result: None,
        tool_name: None,
        is_error: false,
        tool_call_only: false,
        time_seconds: parse_duration_cell(payload, started_at, completed_at),
        started_at,
        metrics: None,
        usage,
        raw: payload.clone(),
    };

    match kind {
        KIND_MESSAGE => {
            let content_text = content_to_text(payload.get("content").unwrap_or(&Value::Null));
            let thinking_text = content_to_text(payload.get("reasoning").unwrap_or(&Value::Null));
            let tool_calls = payload.get("tool_calls");
            if content_text.trim().is_empty() && tool_calls.map(Value::is_array).unwrap_or(false) {
                cell.tool_call_only = true;
            } else {
                cell.text = markdown_preview_text(&content_text);
            }
            cell.input_detail = non_empty(&content_text);
            cell.output_detail = cell.input_detail.clone();
            cell.thinking_detail = non_empty(&thinking_text);
            let ttft = payload.get("ttft_ms").and_then(Value::as_f64).filter(|v| v.is_finite());
            cell.metrics = Some(Metrics {
                step_start: started_at,
                first_token: ttft.zip(started_at).map(|(ttft, start)| start + ttft),
                completed: completed_at,
                output_tokens: cell.usage.and_then(|usage| usage.output),
            });
            cell.is_error = status == "failed" || status == "error";
        }
        KIND_TOOL | KIND_SUBTOOL => {
            let name = payload
                .get("tool")
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .or_else(|| {
                    payload.get("name").and_then(Value::as_str).filter(|v| !v.trim().is_empty())
                })
                .unwrap_or_default();
            let args_raw = match payload.get("args") {
                Some(Value::String(text)) => text.clone(),
                Some(value) if value.is_object() || value.is_array() => {
                    serde_json::to_string_pretty(value).unwrap_or_default()
                }
                _ => String::new(),
            };
            let result_raw = summarize_tool_result(payload);
            let error_value = payload.get("error").or_else(|| payload.get("is_error"));
            cell.text = name.to_string();
            cell.tool_name = non_empty(name);
            cell.input_detail = non_empty(&args_raw);
            cell.output_detail = non_empty(&result_raw);
            cell.result = cell.output_detail.clone();
            let schema = payload.get("schema").or_else(|| payload.get("tool_schema"));
            cell.schema_detail = match schema {
                Some(Value::String(text)) if !text.trim().is_empty() => Some(text.clone()),
                Some(value @ Value::Object(_)) => {
                    serde_json::to_string_pretty(value).unwrap_or_default().into()
                }
                _ => None,
            };
            cell.is_error = error_value == Some(&json!(true))
                || error_value.map(|v| !v.is_null() && *v != json!(false)).unwrap_or(false)
                || status == "failed"
                || status == "error";
            if let (Some(start), Some(end)) = (started_at, completed_at) {
                cell.time_seconds = Some(((end - start) / 1000.0).max(0.0));
            }
        }
        KIND_COMPACTED => {
            let summary_text = content_to_text(
                payload.get("summary").or_else(|| payload.get("content")).unwrap_or(&Value::Null),
            );
            cell.text = summary_text.trim().to_string();
            cell.input_detail = non_empty(&summary_text);
        }
        _ => {
            let content_text = content_to_text(payload.get("content").unwrap_or(&Value::Null));
            cell.text = markdown_preview_text(&content_text);
            cell.input_detail = non_empty(&content_text);
        }
    }
    cell
}

/// 把快照折叠成 轮次 → 步骤组 → 记录；items 按 turn_id 归组进各自的轮次。
fn parse_snapshot(snapshot: &Value) -> SnapshotModel {
    let empty = Vec::new();
    let turns_raw = snapshot.get("turns").and_then(Value::as_array).unwrap_or(&empty);
    let items_raw = snapshot.get("items").and_then(Value::as_array).unwrap_or(&empty);

    let mut buckets: HashMap<&str, Vec<&Value>> = HashMap::new();
    for item in items_raw {
        if let Some(turn_id) = item.get("turn_id").and_then(Value::as_str) {
            buckets.entry(turn_id).or_default().push(item);
        }
    }

    let mut cells: Vec<Cell> = Vec::new();
    let mut turns: Vec<TurnModel> = Vec::new();
    for raw_turn in turns_raw {
        if !raw_turn.is_object() {
            continue;
        }
        let turn_no = USER_TURN_KEYS.iter().find_map(|key| {
            raw_turn.get(*key).and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v as i64)
        });
        let turn_id = raw_turn.get("turn_id").and_then(Value::as_str).unwrap_or("");
        let mut groups: Vec<Group> = Vec::new();
        let mut compaction_seq = 0usize;

        let items = buckets.get(turn_id).cloned().unwrap_or_default();
        let mut consumed = vec![false; items.len()];
        for (i, item) in items.iter().enumerate() {
            if consumed[i] {
                continue;
            }
            let payload = payload_of(item);
            let raw_kind = raw_kind_of(item, payload).to_string();
            let kind = resolve_kind(&raw_kind, payload);
            let index = cells.len();
            cells.push(build_cell(item, payload, kind, index));

            // tool_call 与紧随其后的 tool_result 合并成一条记录。
            if kind == KIND_TOOL && raw_kind == "tool_call" {
                let call_id = payload.get("tool_call_id").and_then(Value::as_str);
                for (j, candidate) in items.iter().enumerate().skip(i + 1) {
                    if consumed[j] {
                        continue;
                    }
                    let candidate_payload = payload_of(candidate);
                    if raw_kind_of(candidate, candidate_payload) != "tool_result" {
                        continue;
                    }
                    if let Some(call_id) = call_id {
                        if candidate_payload.get("tool_call_id").and_then(Value::as_str)
                            != Some(call_id)
                        {
                            continue;
                        }
                    }
                    let result_raw = summarize_tool_result(candidate_payload);
                    if let Some(result) = non_empty(&result_raw) {
                        cells[index].result = Some(result.clone());
                        cells[index].output_detail = Some(result);
                    }
                    if candidate_payload.get("error") == Some(&json!(true)) {
                        cells[index].is_error = true;
                    }
                    let candidate_start = first_time(candidate_payload, &STARTED_KEYS)
                        .or_else(|| first_time(candidate, &["created_time"]));
                    let candidate_end = first_time(candidate_payload, &COMPLETED_KEYS)
                        .or_else(|| first_time(candidate, &["updated_time"]));
                    if let (Some(start), Some(end)) = (candidate_start, candidate_end) {
                        cells[index].time_seconds = Some(((end - start) / 1000.0).max(0.0));
                    }
                    consumed[j] = true;
                    break;
                }
            }

            if kind == KIND_MESSAGE {
                let step = payload.get("model_round").and_then(Value::as_f64).filter(|v| v.is_finite());
                let group = match step {
                    Some(step) => {
                        let step = step as i64;
                        match groups.iter_mut().find(|group| group.step == Some(step)) {
                            Some(found) => found,
                            None => {
                                groups.push(Group {
                                    identity: format!("step:{step}"),
                                    cells: Vec::new(),
                                    step: Some(step),
                                    compaction: false,
                                });
                                groups.last_mut().unwrap()
                            }
                        }
                    }
                    None => {
                        let reuse = groups
                            .last()
                            .map(|last: &Group| !last.compaction && last.step.is_none())
                            .unwrap_or(false);
                        if !reuse {
                            groups.push(Group {
                                identity: GROUP_MESSAGES.to_string(),
                                cells: Vec::new(),
                                step: None,
                                compaction: false,
                            });
                        }
                        groups.last_mut().unwrap()
                    }
                };
                group.cells.push(index);
            } else if kind == KIND_COMPACTED {
                compaction_seq += 1;
                groups.push(Group {
                    identity: format!("compaction:{compaction_seq}"),
                    cells: vec![index],
                    step: None,
                    compaction: true,
                });
            } else if kind == KIND_TOOL || kind == KIND_SUBTOOL {
                let step = payload.get("model_round").and_then(Value::as_f64).filter(|v| v.is_finite());
                let group = match step {
                    Some(step) => {
                        let step = step as i64;
                        match groups.iter_mut().find(|group| group.step == Some(step)) {
                            Some(found) => found,
                            None => {
                                groups.push(Group {
                                    identity: format!("step:{step}"),
                                    cells: Vec::new(),
                                    step: Some(step),
                                    compaction: false,
                                });
                                groups.last_mut().unwrap()
                            }
                        }
                    }
                    None => {
                        let last_is_step = groups
                            .last()
                            .map(|last: &Group| last.step.is_some() && !last.compaction)
                            .unwrap_or(false);
                        if last_is_step {
                            groups.last_mut().unwrap()
                        } else {
                            groups.push(Group {
                                identity: "step:1".to_string(),
                                cells: Vec::new(),
                                step: Some(1),
                                compaction: false,
                            });
                            groups.last_mut().unwrap()
                        }
                    }
                };
                group.cells.push(index);
            } else {
                let reuse = groups
                    .last()
                    .map(|last: &Group| !last.compaction && last.step.is_none())
                    .unwrap_or(false);
                if !reuse {
                    groups.push(Group {
                        identity: GROUP_MESSAGES.to_string(),
                        cells: Vec::new(),
                        step: None,
                        compaction: false,
                    });
                }
                groups.last_mut().unwrap().cells.push(index);
            }
        }
        turns.push(TurnModel { turn: turn_no, groups });
    }

    // requests: numbered model-call groups with cumulative usage
    let mut requests: Vec<Request> = Vec::new();
    let mut cumulative = Usage::default();
    for turn in &turns {
        for group in &turn.groups {
            if group.compaction || group.step.is_none() {
                continue;
            }
            let Some(message_cell) =
                group.cells.iter().map(|&i| &cells[i]).find(|cell| cell.kind == KIND_MESSAGE)
            else {
                continue;
            };
            if let Some(usage) = message_cell.usage {
                cumulative.add(usage);
            }
            requests.push(Request {
                number: requests.len() + 1,
                turn: turn.turn,
                identity: State::identity_key(turn.turn, &group.identity),
                started_at: message_cell.started_at,
                completed_at: message_cell.metrics.as_ref().and_then(|m| m.completed),
                usage: message_cell.usage,
                cumulative,
                assistant_index: message_cell.index,
                first_record: 0,
            });
        }
    }

    let mut records: Vec<Record> = Vec::new();
    for turn in &turns {
        let turn_start = records.len();
        for group in &turn.groups {
            for &cell in &group.cells {
                records.push(Record {
                    turn: turn.turn,
                    group: group.identity.clone(),
                    turn_start: false,
                    cell,
                });
            }
        }
        if records.len() > turn_start {
            records[turn_start].turn_start = true;
        }
    }
    // first record per assistant group + request numbering by record position
    let mut first_of_group: HashMap<String, usize> = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        let key = State::identity_key(record.turn, &record.group);
        first_of_group.entry(key).or_insert(index);
    }
    for request in &mut requests {
        request.first_record = first_of_group.get(&request.identity).copied().unwrap_or(0);
    }

    // timeline bases in wall-clock milliseconds
    let mut spans: Vec<SpanBase> = Vec::new();
    let mut boundaries: Vec<(i64, f64, usize)> = Vec::new();
    for (record_index, record) in records.iter().enumerate() {
        let cell = &cells[record.cell];
        if record.turn_start {
            if let Some(turn) = record.turn {
                boundaries.push((turn, cell.started_at.unwrap_or(0.0), record_index));
            }
        }
        let Some(start) = cell.started_at else { continue };
        let duration_ms = cell.time_seconds.unwrap_or(0.0).max(0.0) * 1000.0;
        let lane = match cell.kind {
            KIND_TOOL | KIND_SUBTOOL => 2,
            KIND_MESSAGE | KIND_COMPACTED => 1,
            _ => 0,
        };
        let ttft = cell.metrics.as_ref().and_then(|metrics| {
            let (Some(step_start), Some(first_token), Some(completed)) =
                (metrics.step_start, metrics.first_token, metrics.completed)
            else {
                return None;
            };
            let total = completed - step_start;
            if total <= 0.0 {
                return None;
            }
            Some(((first_token - step_start) / total).clamp(0.0, 1.0))
        });
        spans.push(SpanBase {
            start,
            end: start + duration_ms,
            record: record_index,
            kind: cell.kind,
            lane,
            ttft,
        });
    }
    let (lo, hi) = spans
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), span| (lo.min(span.start), hi.max(span.end)));
    let (tl_start, tl_end) =
        if spans.is_empty() { (0.0, 1.0) } else { (lo, hi.max(lo + 1.0)) };

    SnapshotModel { cells, turns, records, requests, spans, boundaries, tl_start, tl_end }
}

// ------------------------------------------------------------------ */
// Bilingual text                                                      */
// ------------------------------------------------------------------ */

struct Texts {
    zh: bool,
}

impl Texts {
    fn new(zh: bool) -> Self {
        Texts { zh }
    }
    fn kind(&self, kind: i32) -> &'static str {
        let zh = self.zh;
        match kind {
            KIND_SYSTEM => if zh { "系统" } else { "System" },
            KIND_USER => if zh { "用户" } else { "User" },
            KIND_CONTEXT => if zh { "上下文" } else { "Context" },
            KIND_COMPACTED => if zh { "已压缩" } else { "Compacted" },
            KIND_MESSAGE => if zh { "消息" } else { "Message" },
            KIND_TOOL => if zh { "工具" } else { "Tool" },
            KIND_SUBTOOL => if zh { "子智能体" } else { "Subagent" },
            _ => "—",
        }
    }
    fn state(&self, cell: &Cell) -> &'static str {
        let zh = self.zh;
        if cell.is_error {
            if zh { "失败" } else { "Failed" }
        } else if cell.metrics.as_ref().map(|m| m.completed.is_none()).unwrap_or(false) {
            if zh { "运行中" } else { "Running" }
        } else {
            if zh { "已完成" } else { "Completed" }
        }
    }
    fn not_recorded(&self) -> &'static str {
        if self.zh { "未记录" } else { "Not recorded" }
    }
    fn not_reported(&self) -> &'static str {
        if self.zh { "未上报" } else { "Not reported" }
    }
    fn millis(&self, value: f64) -> String {
        format!("{} {}", thousands(value), if self.zh { "毫秒" } else { "ms" })
    }
    fn seconds(&self, value: Option<f64>) -> String {
        value.map(|v| self.millis(v * 1000.0)).unwrap_or_else(|| self.not_recorded().to_string())
    }
    fn header(&self, turn: i64, group: &str) -> String {
        format!("{} · {}", self.turn_label(turn), self.group_label(group))
    }
    fn turn_label(&self, turn: i64) -> String {
        if self.zh {
            format!("第 {} 轮", turn)
        } else {
            format!("Turn {}", turn)
        }
    }
    /// Group identity → localized label.
    fn group_label(&self, identity: &str) -> String {
        let zh = self.zh;
        if identity == GROUP_MESSAGES {
            return if zh { "消息" } else { "Messages" }.to_string();
        }
        if let Some(rest) = identity.strip_prefix("step:") {
            return match rest.parse::<i64>() {
                Ok(step) => {
                    if zh {
                        format!("第 {} 步", step)
                    } else {
                        format!("Step {}", step)
                    }
                }
                Err(_) => identity.to_string(),
            };
        }
        if let Some(rest) = identity.strip_prefix("compaction:") {
            return match rest.parse::<usize>() {
                Ok(seq) => {
                    if zh {
                        format!("压缩 {}", seq)
                    } else {
                        format!("Compaction {}", seq)
                    }
                }
                Err(_) => identity.to_string(),
            };
        }
        identity.to_string()
    }
    fn request_label(&self, number: usize) -> String {
        if self.zh {
            format!("请求 #{}", number)
        } else {
            format!("Request #{}", number)
        }
    }
    fn summary_turn(&self, steps: usize, tools: usize) -> String {
        if self.zh {
            format!("{} 步 · {} 次工具调用", steps, tools)
        } else {
            format!("{} steps · {} tool calls", steps, tools)
        }
    }
    fn summary_tools(&self, count: usize) -> String {
        if self.zh {
            format!("{} 次工具调用", count)
        } else {
            format!("{} tool calls", count)
        }
    }
}

fn thousands(value: f64) -> String {
    let rounded = value.round() as i64;
    let text = rounded.abs().to_string();
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i > 0 && (text.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if rounded < 0 {
        format!("-{out}")
    } else {
        out
    }
}

fn format_started_at(ms: f64) -> String {
    Local.timestamp_millis_opt(ms.round() as i64)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
        .unwrap_or_default()
}

/// 时间轴刻度标签：从时间线起点计的偏移。
fn format_offset(ms: f64) -> String {
    let abs = ms.abs();
    if abs < 1_000.0 {
        return format!("{} ms", abs.round());
    }
    if abs < 60_000.0 {
        if abs < 10_000.0 {
            return format!("{:.1} s", abs / 1_000.0);
        }
        return format!("{:.0} s", abs / 1_000.0);
    }
    if abs < 3_600_000.0 {
        let minutes = (abs / 60_000.0).floor();
        let seconds = ((abs % 60_000.0) / 1_000.0).round();
        return if seconds > 0.0 {
            format!("{}m {:02}s", minutes, seconds)
        } else {
            format!("{}m", minutes)
        };
    }
    let hours = (abs / 3_600_000.0).floor();
    let minutes = ((abs % 3_600_000.0) / 60_000.0).round();
    format!("{}h {:02}m", hours, minutes)
}

// ------------------------------------------------------------------ */
// Projection                                                          */
// ------------------------------------------------------------------ */

fn request_number_of(model: &SnapshotModel, record_index: usize) -> usize {
    let record = &model.records[record_index];
    let cell = &model.cells[record.cell];
    if cell.kind != KIND_MESSAGE {
        return 0;
    }
    model
        .requests
        .iter()
        .find(|request| request.assistant_index == cell.index)
        .map(|request| request.number)
        .unwrap_or(0)
}

fn record_needle(model: &SnapshotModel, record_index: usize) -> String {
    let cell = &model.cells[model.records[record_index].cell];
    [
        cell.text.as_str(),
        cell.tool_name.as_deref().unwrap_or(""),
        cell.input_detail.as_deref().unwrap_or(""),
        cell.output_detail.as_deref().unwrap_or(""),
        cell.result.as_deref().unwrap_or(""),
    ]
    .join("\n")
    .to_lowercase()
}

/// 台账行投影：搜索过滤 + 轮次/助手折叠；行携带全局记录索引。
fn project_rows(app: &MainWindow, state: &State, model: &SnapshotModel, texts: &Texts) {
    let needle = state.query.trim().to_lowercase();
    let mut rows: Vec<TrajRow> = Vec::new();
    let mut current_turn: Option<i64> = None;
    let mut turn_started = false;
    let mut turn_collapsed = false;
    let mut emitted_group: HashSet<String> = HashSet::new();
    for (record_index, record) in model.records.iter().enumerate() {
        if !turn_started || record.turn != current_turn {
            turn_started = true;
            current_turn = record.turn;
            turn_collapsed = state.show_turns
                && state.collapsed_turns.contains(&record.turn.unwrap_or(i64::MIN));
        }
        if !needle.is_empty() && !record_needle(model, record_index).contains(&needle) {
            continue;
        }
        if turn_collapsed {
            if !record.turn_start {
                continue;
            }
            let bucket: Vec<usize> = model
                .records
                .iter()
                .enumerate()
                .filter(|(_, other)| other.turn == record.turn)
                .map(|(index, _)| index)
                .collect();
            let steps = bucket
                .iter()
                .map(|&index| model.records[index].group.clone())
                .collect::<HashSet<_>>()
                .len();
            let tools = bucket
                .iter()
                .filter(|&&index| {
                    let kind = model.cells[model.records[index].cell].kind;
                    kind == KIND_TOOL || kind == KIND_SUBTOOL
                })
                .count();
            rows.push(TrajRow {
                kind: 7,
                badge: "".into(),
                text: texts.summary_turn(steps, tools).into(),
                detail: "".into(),
                error: false,
                request: 0,
                turn: record.turn.unwrap_or(0) as i32,
                turn_start: true,
                turn_end: false,
                row_height: ROW_SUMMARY_PX,
                index: record_index as i32,
            });
            continue;
        }
        let group_key = State::identity_key(record.turn, &record.group);
        if state.show_calls && state.collapsed_assistants.contains(&group_key) {
            if !emitted_group.insert(group_key) {
                continue;
            }
            let tools = model
                .records
                .iter()
                .filter(|other| {
                    other.turn == record.turn
                        && other.group == record.group
                        && {
                            let kind = model.cells[other.cell].kind;
                            kind == KIND_TOOL || kind == KIND_SUBTOOL
                        }
                })
                .count();
            rows.push(TrajRow {
                kind: 8,
                badge: "".into(),
                text: texts.summary_tools(tools).into(),
                detail: "".into(),
                error: false,
                request: 0,
                turn: record.turn.unwrap_or(0) as i32,
                turn_start: record.turn_start,
                turn_end: false,
                row_height: ROW_SUMMARY_PX,
                index: record_index as i32,
            });
            continue;
        }
        let cell = &model.cells[record.cell];
        let number = request_number_of(model, record_index);
        let mut text = cell.text.clone();
        if text.trim().is_empty() {
            let zh = texts.zh;
            text = if cell.tool_call_only {
                if zh { "仅工具调用" } else { "Tool calls only" }
            } else if cell.kind == KIND_TOOL || cell.kind == KIND_SUBTOOL {
                if zh { "未命名工具" } else { "Unnamed tool" }
            } else if cell.kind == KIND_COMPACTED {
                if zh { "无摘要" } else { "No summary" }
            } else {
                if zh { "无内容" } else { "No content" }
            }
            .to_string();
        }
        let detail = if cell.kind == KIND_TOOL || cell.kind == KIND_SUBTOOL {
            let args = cell.input_detail.as_deref().unwrap_or("").replace('\n', " ");
            let result = cell.result.as_deref().unwrap_or("").replace('\n', " ");
            let mut detail = String::new();
            if !args.is_empty() {
                detail.push_str(&args.chars().take(120).collect::<String>());
            }
            if !result.is_empty() {
                if !detail.is_empty() {
                    detail.push_str(" → ");
                }
                detail.push_str(&result.chars().take(120).collect::<String>());
            }
            detail
        } else {
            String::new()
        };
        rows.push(TrajRow {
            kind: cell.kind,
            badge: if number > 0 { format!("#{}", number).into() } else { "".into() },
            text: text.into(),
            detail: detail.into(),
            error: cell.is_error,
            request: if number > 0 { 1 } else { 0 },
            turn: record.turn.unwrap_or(0) as i32,
            turn_start: record.turn_start,
            turn_end: false,
            row_height: ROW_PX,
            index: record_index as i32,
        });
    }
    app.set_traj_rows(to_model(rows));
}

/// 时间线投影：时长模式压缩空闲，序列模式等宽；span/边界按当前视口裁剪归一。
fn project_timeline(app: &MainWindow, state: &State, model: &SnapshotModel, texts: &Texts) {
    let view_start = state.view_start.clamp(0.0, 1.0);
    let view_width = state.view_width.clamp(0.02, 1.0);
    let timed = state.show_duration;
    let (total_lo, total_hi) = if timed {
        (model.tl_start, model.tl_end)
    } else {
        (0.0, model.records.len().max(1) as f64)
    };
    let total = (total_hi - total_lo).max(1.0);
    let window_lo = total_lo + view_start * total;
    let window_total = view_width * total;
    let clamp_pos = |value: f64| ((value - window_lo) / window_total).clamp(0.0, 1.0) as f32;

    let mut spans: Vec<TrajSpan> = Vec::new();
    for base in &model.spans {
        let (start, end) = if timed {
            (base.start, base.end)
        } else {
            (base.record as f64, base.record as f64 + 1.0)
        };
        let pos = (start - window_lo) / window_total;
        let width = (end - start) / window_total;
        let x0 = pos.max(0.0);
        let x1 = (pos + width).min(1.0);
        if x1 <= 0.0 || x0 >= 1.0 {
            continue;
        }
        spans.push(TrajSpan {
            lane: base.lane as i32,
            start: x0 as f32,
            width: ((x1 - x0).max(0.002)) as f32,
            kind: base.kind,
            index: base.record as i32,
            ttft: base.ttft.unwrap_or(-1.0) as f32,
            selected: state.selected >= 0 && base.record == state.selected as usize,
        });
    }

    let mut ticks: Vec<TrajTick> = Vec::new();
    if timed {
        let step = TICK_STEPS_MS
            .iter()
            .find(|step| window_total / **step <= 6.0)
            .unwrap_or(&TICK_STEPS_MS[TICK_STEPS_MS.len() - 1]);
        let mut tick = (window_lo / step).ceil() * step;
        while tick <= window_lo + window_total {
            ticks.push(TrajTick {
                pos: clamp_pos(tick),
                label: format_offset(tick - model.tl_start).into(),
            });
            tick += step;
        }
    }

    let boundaries: Vec<TrajBoundary> = model
        .boundaries
        .iter()
        .map(|(turn, time, record_index)| TrajBoundary {
            pos: if timed { clamp_pos(*time) } else { clamp_pos(*record_index as f64) },
            label: texts.turn_label(*turn).into(),
        })
        .collect();

    app.set_traj_spans(to_model(spans));
    app.set_traj_ticks(to_model(ticks));
    app.set_traj_boundaries(to_model(boundaries));
    app.set_traj_view_start(view_start as f32);
    app.set_traj_view_width(view_width as f32);
}

fn bounded_body(text: &str) -> String {
    if text.chars().count() > SECTION_BODY_LIMIT {
        text.chars().take(SECTION_BODY_LIMIT).collect::<String>() + "…"
    } else {
        text.to_string()
    }
}

fn tab_title(texts: &Texts, tab: i32) -> &'static str {
    let zh = texts.zh;
    match tab {
        TAB_OVERVIEW => if zh { "概述" } else { "Overview" },
        TAB_RAW => if zh { "原始数据" } else { "Raw" },
        TAB_PARAMS => if zh { "参数" } else { "Params" },
        TAB_RESULT => if zh { "结果" } else { "Result" },
        TAB_SCHEMA => "Schema",
        _ => if zh { "计时" } else { "Timing" },
    }
}

fn label(zh: bool, zh_text: &'static str, en_text: &'static str) -> &'static str {
    if zh {
        zh_text
    } else {
        en_text
    }
}

/// 详情面板投影：按当前页签填充键值、内容区与计时行。
fn project_details(app: &MainWindow, state: &State, model: &SnapshotModel, texts: &Texts) {
    let selected = state.selected;
    app.set_traj_selected(selected);
    app.set_traj_detail_tab(state.tab);
    let clear = |app: &MainWindow| {
        app.set_traj_detail_header("".into());
        app.set_traj_detail_badge("".into());
        app.set_traj_detail_kind(1);
        app.set_traj_detail_tabs(to_model(vec![]));
        app.set_traj_overview(to_model(vec![]));
        app.set_traj_sections(to_model(vec![]));
        app.set_traj_timing(to_model(vec![]));
        app.set_traj_raw("".into());
    };
    let Some(record) = model.records.get(selected.max(0) as usize).filter(|_| selected >= 0) else {
        clear(app);
        return;
    };
    let cell = &model.cells[record.cell];
    let zh = texts.zh;
    let group_label = texts.group_label(&record.group);
    let header = texts.header(record.turn.unwrap_or(0), &record.group);

    let mut tabs: Vec<TrajTab> =
        vec![TrajTab { title: tab_title(texts, TAB_OVERVIEW).into(), kind: TAB_OVERVIEW }];
    if cell.kind == KIND_TOOL || cell.kind == KIND_SUBTOOL {
        tabs.push(TrajTab { title: tab_title(texts, TAB_PARAMS).into(), kind: TAB_PARAMS });
        tabs.push(TrajTab { title: tab_title(texts, TAB_RESULT).into(), kind: TAB_RESULT });
        if cell.schema_detail.is_some() {
            tabs.push(TrajTab { title: tab_title(texts, TAB_SCHEMA).into(), kind: TAB_SCHEMA });
        }
    }
    if cell.metrics.is_some() {
        tabs.push(TrajTab { title: tab_title(texts, TAB_TIMING).into(), kind: TAB_TIMING });
    }
    tabs.push(TrajTab { title: tab_title(texts, TAB_RAW).into(), kind: TAB_RAW });

    let mut overview: Vec<TrajTiming> = Vec::new();
    let mut sections: Vec<TrajSection> = Vec::new();
    let mut timing: Vec<TrajTiming> = Vec::new();
    let mut raw = String::new();

    match state.tab {
        TAB_RAW => {
            raw = serde_json::to_string_pretty(&cell.raw).unwrap_or_default();
        }
        TAB_TIMING => {
            if let Some(metrics) = &cell.metrics {
                timing.push(TrajTiming {
                    label: label(zh, "开始时间", "Started at").into(),
                    value: metrics.step_start.map(format_started_at).unwrap_or_default().into(),
                });
                timing.push(TrajTiming {
                    label: label(zh, "总时长", "Total duration").into(),
                    value: texts.seconds(cell.time_seconds).into(),
                });
                if let (Some(step_start), Some(first_token)) =
                    (metrics.step_start, metrics.first_token)
                {
                    timing.push(TrajTiming {
                        label: label(zh, "首字延迟", "First token").into(),
                        value: texts.millis(first_token - step_start).into(),
                    });
                    if let Some(completed) = metrics.completed {
                        let generation = (completed - first_token).max(0.0);
                        timing.push(TrajTiming {
                            label: label(zh, "生成", "Generation").into(),
                            value: texts.millis(generation).into(),
                        });
                        if let Some(output) = metrics.output_tokens.filter(|_| generation > 0.0) {
                            timing.push(TrajTiming {
                                label: label(zh, "吞吐", "Throughput").into(),
                                value: format!(
                                    "{:.1} {}",
                                    output / (generation / 1000.0),
                                    if zh { "token/秒" } else { "tok/s" }
                                )
                                .into(),
                            });
                        }
                    }
                }
            }
        }
        TAB_PARAMS => {
            if let Some(input) = &cell.input_detail {
                sections.push(TrajSection {
                    title: label(zh, "参数", "Params").into(),
                    body: bounded_body(input).into(),
                });
            }
        }
        TAB_RESULT => {
            let body = cell
                .result
                .as_deref()
                .or(cell.output_detail.as_deref())
                .unwrap_or_default();
            if !body.is_empty() {
                sections.push(TrajSection {
                    title: label(zh, "结果", "Result").into(),
                    body: bounded_body(body).into(),
                });
            }
        }
        TAB_SCHEMA => {
            if let Some(schema) = &cell.schema_detail {
                sections.push(TrajSection {
                    title: "Schema".into(),
                    body: bounded_body(schema).into(),
                });
            }
        }
        _ => {
            let number = request_number_of(model, selected as usize);
            overview.push(TrajTiming {
                label: label(zh, "状态", "Status").into(),
                value: texts.state(cell).into(),
            });
            overview.push(TrajTiming {
                label: label(zh, "层级", "Hierarchy").into(),
                value: format!("{} · {}", texts.turn_label(record.turn.unwrap_or(0)), group_label)
                    .into(),
            });
            if number > 0 {
                overview.push(TrajTiming {
                    label: label(zh, "请求", "Request").into(),
                    value: texts.request_label(number).into(),
                });
            }
            overview.push(TrajTiming {
                label: label(zh, "开始时间", "Started at").into(),
                value: cell.started_at.map(format_started_at).unwrap_or_default().into(),
            });
            overview.push(TrajTiming {
                label: label(zh, "总时长", "Total duration").into(),
                value: texts.seconds(cell.time_seconds).into(),
            });
            if let Some(usage) = cell.usage {
                let mut parts: Vec<String> = Vec::new();
                if let Some(input) = usage.input {
                    parts.push(format!("{} {}", label(zh, "输入", "in"), thousands(input)));
                }
                if let Some(read) = usage.cache_read {
                    parts.push(format!("{} {}", label(zh, "缓存读", "cache"), thousands(read)));
                }
                if let Some(write) = usage.cache_write {
                    parts.push(format!("{} {}", label(zh, "缓存写", "cache+"), thousands(write)));
                }
                if let Some(output) = usage.output {
                    parts.push(format!("{} {}", label(zh, "输出", "out"), thousands(output)));
                }
                if let Some(think) = usage.think {
                    parts.push(format!("{} {}", label(zh, "推理", "think"), thousands(think)));
                }
                overview.push(TrajTiming {
                    label: label(zh, "Token", "Tokens").into(),
                    value: if parts.is_empty() {
                        texts.not_reported().into()
                    } else {
                        parts.join(" · ").into()
                    },
                });
            }
            if let Some(input) = &cell.input_detail {
                sections.push(TrajSection {
                    title: label(zh, "输入", "Input").into(),
                    body: bounded_body(input).into(),
                });
            }
            if let Some(output) = &cell.output_detail {
                if cell.kind == KIND_MESSAGE || cell.kind == KIND_TOOL || cell.kind == KIND_SUBTOOL
                {
                    sections.push(TrajSection {
                        title: label(zh, "输出", "Output").into(),
                        body: bounded_body(output).into(),
                    });
                }
            }
            if let Some(thinking) = &cell.thinking_detail {
                sections.push(TrajSection {
                    title: label(zh, "思考", "Thinking").into(),
                    body: bounded_body(thinking).into(),
                });
            }
        }
    }

    app.set_traj_detail_header(header.into());
    app.set_traj_detail_badge(texts.kind(cell.kind).into());
    app.set_traj_detail_kind(cell.kind);
    app.set_traj_detail_tabs(to_model(tabs));
    app.set_traj_overview(to_model(overview));
    app.set_traj_sections(to_model(sections));
    app.set_traj_timing(to_model(timing));
    app.set_traj_raw(raw.into());
}

fn tooltip_for(model: &SnapshotModel, texts: &Texts, record_index: i32) -> String {
    let Some(record) = model.records.get(record_index.max(0) as usize).filter(|_| record_index >= 0)
    else {
        return String::new();
    };
    let cell = &model.cells[record.cell];
    let zh = texts.zh;
    let mut lines: Vec<String> = Vec::new();
    let number = request_number_of(model, record_index.max(0) as usize);
    if number > 0 {
        lines.push(texts.request_label(number));
    }
    lines.push(texts.kind(cell.kind).to_string());
    lines.push(texts.seconds(cell.time_seconds));
    if let Some(usage) = cell.usage {
        if let Some(output) = usage.output {
            lines.push(format!("{} {}", label(zh, "输出", "out"), thousands(output)));
        }
    }
    lines.join("\n")
}

// ------------------------------------------------------------------ */
// Controller                                                          */
// ------------------------------------------------------------------ */

fn apply_model(app: &MainWindow, state: &State) {
    let Some(snapshot) = &state.model else { return };
    let texts = Texts::new(app.global::<I18n>().get_zh());
    app.set_traj_loading(false);
    app.set_traj_failed(false);
    project_rows(app, state, snapshot, &texts);
    project_timeline(app, state, snapshot, &texts);
    project_details(app, state, snapshot, &texts);
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let state = Rc::new(RefCell::new(State::default()));
    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    app.on_open_trajectory(move |index| {
        let Some(app) = weak.upgrade() else { return };
        // index < 0 means "the active thread" (the settings-page entry point).
        let session = if index < 0 {
            app.get_active_session_id().to_string()
        } else {
            app.get_conversations()
                .row_data(index as usize)
                .map(|row| row.id.to_string())
                .unwrap_or_default()
        };
        if session.is_empty() {
            return;
        }
        current.borrow_mut().reset(session.clone());
        app.set_traj_open(true);
        app.set_traj_loading(true);
        app.set_traj_failed(false);
        app.set_traj_rows(to_model(vec![]));
        app.set_traj_spans(to_model(vec![]));
        app.set_traj_ticks(to_model(vec![]));
        app.set_traj_boundaries(to_model(vec![]));
        let (tx, rx) = sync_channel(1);
        let backend = backend.clone();
        std::thread::spawn(move || {
            let parsed = backend.thread_snapshot(&session).map(|snapshot| parse_snapshot(&snapshot));
            let _ = tx.send((session, parsed));
        });
        let (weak, current) = (weak.clone(), current.clone());
        poll(move || {
            let Ok((session, result)) = rx.try_recv() else { return false };
            let Some(app) = weak.upgrade() else { return true };
            if current.borrow().session != session {
                return true;
            }
            match result {
                Ok(snapshot) => {
                    current.borrow_mut().model = Some(snapshot);
                    apply_model(&app, &current.borrow());
                }
                Err(error) => {
                    app.set_traj_loading(false);
                    app.set_traj_failed(true);
                    app.set_status(format!("无法读取线程轨迹：{error}").into());
                }
            }
            true
        });
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_select_row(move |index| {
        let Some(app) = weak.upgrade() else { return };
        // summary rows expand their group instead of selecting; read the
        // group facts first so the state borrow can be dropped before writes
        let group_key = current.borrow().model.as_ref().and_then(|snapshot| {
            snapshot.records.get(index as usize).filter(|_| index >= 0).map(|record| {
                (
                    record.turn,
                    State::identity_key(record.turn, &record.group),
                    snapshot
                        .records
                        .get(index.max(0) as usize)
                        .map(|record| snapshot.cells[record.cell].kind),
                )
            })
        });
        let mut s = current.borrow_mut();
        let mut open_kind: Option<Option<i32>> = None;
        if let Some((turn, key, kind)) = group_key {
            open_kind = Some(kind);
            let turn_key = turn.unwrap_or(i64::MIN);
            if s.show_turns && s.collapsed_turns.contains(&turn_key) {
                s.collapsed_turns.remove(&turn_key);
                drop(s);
                apply_model(&app, &current.borrow());
                return;
            }
            if s.show_calls && s.collapsed_assistants.contains(&key) {
                s.collapsed_assistants.remove(&key);
                drop(s);
                apply_model(&app, &current.borrow());
                return;
            }
        }
        s.selected = index;
        // tool/subtool records open on the params tab, the rest on overview
        s.tab = match open_kind.flatten() {
            Some(kind) if kind == KIND_TOOL || kind == KIND_SUBTOOL => TAB_PARAMS,
            _ => TAB_OVERVIEW,
        };
        drop(s);
        apply_model(&app, &current.borrow());
    });

    let weak = app.as_weak();
    app.on_traj_select_span(move |index| {
        if let Some(app) = weak.upgrade() {
            app.invoke_traj_select_row(index);
        }
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_hover_span(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let s = current.borrow();
        app.set_traj_hover(index);
        if let Some(snapshot) = &s.model {
            let texts = Texts::new(app.global::<I18n>().get_zh());
            app.set_traj_tooltip(tooltip_for(snapshot, &texts, index).into());
        }
    });

    let (weak, _current) = (app.as_weak(), state.clone());
    app.on_traj_leave(move || {
        if let Some(app) = weak.upgrade() {
            app.set_traj_hover(-1);
            app.set_traj_tooltip("".into());
        }
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_select_tab(move |tab| {
        let Some(app) = weak.upgrade() else { return };
        current.borrow_mut().tab = tab;
        let s = current.borrow();
        if let Some(snapshot) = &s.model {
            let texts = Texts::new(app.global::<I18n>().get_zh());
            project_details(&app, &s, snapshot, &texts);
        }
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_toggle_duration(move || {
        let Some(app) = weak.upgrade() else { return };
        {
            let mut s = current.borrow_mut();
            s.show_duration = !s.show_duration;
            s.view_start = 0.0;
            s.view_width = 1.0;
            app.set_traj_show_duration(s.show_duration);
        }
        apply_model(&app, &current.borrow());
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_toggle_turns(move || {
        let Some(app) = weak.upgrade() else { return };
        {
            let mut s = current.borrow_mut();
            s.show_turns = !s.show_turns;
            s.collapsed_turns.clear();
            if s.show_turns {
                let turns: Vec<Option<i64>> = s
                    .model
                    .as_ref()
                    .map(|snapshot| snapshot.turns.iter().map(|turn| turn.turn).collect())
                    .unwrap_or_default();
                for turn in turns {
                    s.collapsed_turns.insert(turn.unwrap_or(i64::MIN));
                }
            }
            app.set_traj_show_turns(s.show_turns);
        }
        apply_model(&app, &current.borrow());
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_toggle_calls(move || {
        let Some(app) = weak.upgrade() else { return };
        {
            let mut s = current.borrow_mut();
            s.show_calls = !s.show_calls;
            s.collapsed_assistants.clear();
            if s.show_calls {
                let identities: Vec<String> = s
                    .model
                    .as_ref()
                    .map(|snapshot| snapshot.requests.iter().map(|r| r.identity.clone()).collect())
                    .unwrap_or_default();
                s.collapsed_assistants.extend(identities);
            }
            app.set_traj_show_calls(s.show_calls);
        }
        apply_model(&app, &current.borrow());
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_search_changed(move || {
        let Some(app) = weak.upgrade() else { return };
        current.borrow_mut().query = app.get_traj_query().to_string();
        apply_model(&app, &current.borrow());
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_zoom(move |factor| {
        let Some(app) = weak.upgrade() else { return };
        {
            let mut s = current.borrow_mut();
            let center = s.view_start + s.view_width / 2.0;
            s.view_width = (s.view_width * factor as f64).clamp(0.02, 1.0);
            s.view_start = (center - s.view_width / 2.0).clamp(0.0, 1.0 - s.view_width);
        }
        let s = current.borrow();
        if let Some(snapshot) = &s.model {
            let texts = Texts::new(app.global::<I18n>().get_zh());
            project_timeline(&app, &s, snapshot, &texts);
        }
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_pan(move |start| {
        let Some(app) = weak.upgrade() else { return };
        {
            let mut s = current.borrow_mut();
            s.view_start = (start as f64).clamp(0.0, 1.0 - s.view_width.max(0.02));
        }
        let s = current.borrow();
        if let Some(snapshot) = &s.model {
            let texts = Texts::new(app.global::<I18n>().get_zh());
            project_timeline(&app, &s, snapshot, &texts);
        }
    });
}

// Single-shot continuations own the response without a timer/callback cycle.
fn poll(mut receive: impl FnMut() -> bool + 'static) {
    slint::Timer::single_shot(std::time::Duration::from_millis(33), move || {
        if !receive() {
            poll(receive);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_folds_turns_groups_and_requests() {
        let snapshot = json!({
            "turns": [
                {"turn_id": "t1", "user_turn_index": 1, "status": "finished", "summary": "s", "payload": {}, "updated_time": 100.0, "root_turn_id": "t1", "trigger_kind": "user"},
                {"turn_id": "t2", "user_turn_index": 2, "status": "finished", "summary": "", "payload": {}, "updated_time": 200.0, "root_turn_id": "t2", "trigger_kind": "user"}
            ],
            "items": [
                {"item_id": "i1", "item_index": 0, "kind": "user_message", "status": "ok", "revision": 0,
                 "payload": {"content": "输入文本", "created_at": 10.0}, "created_time": 10.0, "updated_time": 10.0,
                 "turn_id": "t1", "visibility": "user", "root_turn_id": "t1", "created_seq": 1},
                {"item_id": "i2", "item_index": 1, "kind": "assistant_message", "status": "ok", "revision": 0,
                 "payload": {"content": "回复文本", "model_round": 1, "started_at": 11.0, "completed_at": 12.5,
                             "ttft_ms": 240.0, "usage": {"input_tokens": 1200, "output_tokens": 340}},
                 "created_time": 12.5, "updated_time": 12.5,
                 "turn_id": "t1", "visibility": "user", "root_turn_id": "t1", "created_seq": 2},
                {"item_id": "i3", "item_index": 2, "kind": "tool_call", "status": "ok", "revision": 0,
                 "payload": {"tool": "demo_tool", "args": {"path": "a"}, "tool_call_id": "c1", "model_round": 2, "started_at": 13.0},
                 "created_time": 13.0, "updated_time": 13.0,
                 "turn_id": "t1", "visibility": "user", "root_turn_id": "t1", "created_seq": 3},
                {"item_id": "i4", "item_index": 3, "kind": "tool_result", "status": "ok", "revision": 0,
                 "payload": {"tool_call_id": "c1", "result": "完成", "completed_at": 13.75},
                 "created_time": 13.75, "updated_time": 13.75,
                 "turn_id": "t1", "visibility": "user", "root_turn_id": "t1", "created_seq": 4},
                {"item_id": "i5", "item_index": 0, "kind": "user_message", "status": "ok", "revision": 0,
                 "payload": {"content": "第二轮输入"}, "created_time": 20.0, "updated_time": 20.0,
                 "turn_id": "t2", "visibility": "user", "root_turn_id": "t2", "created_seq": 5}
            ],
            "blocks": [],
            "item_total": 5
        });
        let model = parse_snapshot(&snapshot);
        // the tool_result item folds into its tool_call cell
        assert_eq!(model.cells.len(), 4);
        assert_eq!(model.turns.len(), 2);
        assert_eq!(model.turns[0].groups.len(), 3); // messages + step:1 + step:2
        assert_eq!(model.turns[1].groups.len(), 1);
        assert_eq!(model.requests.len(), 1);
        assert_eq!(model.requests[0].number, 1);
        assert_eq!(model.requests[0].usage.unwrap().output, Some(340.0));
        assert_eq!(model.records.len(), 4);
        // merged tool call carries the result and the real duration
        let tool = &model.cells[2];
        assert_eq!(tool.kind, KIND_TOOL);
        assert_eq!(tool.result.as_deref(), Some("完成"));
        // merged duration comes from the result item's own timestamps
        assert!((tool.time_seconds.unwrap()).abs() < 1e-9);
        // span bases are wall-clock, one per record with a start time
        assert_eq!(model.spans.len(), 4);
        assert_eq!(model.boundaries.len(), 2);
        let texts = Texts::new(true);
        assert_eq!(texts.group_label("step:2"), "第 2 步");
        assert_eq!(texts.seconds(None), "未记录");
        assert_eq!(thousands(11049.0), "11,049");
        assert_eq!(markdown_preview_text("# 标题\n- 甲 **粗** 乙"), "标题\n甲 粗 乙");
    }
}
