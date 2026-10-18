//! Thread trajectory controller: folds the thread-log snapshot into
//! turn → group → cell ledger rows plus the three-lane timeline, mirroring the
//! web trajectory model one rule at a time. All text is projected in Rust so
//! Slint only lays out bounded rows, and every row and span is keyed by the
//! global record index so hover and selection survive filtering.
use crate::{
    I18n, MainWindow, TrajBoundary, TrajRow, TrajSearchHit, TrajSection, TrajSpan, TrajTab,
    TrajTick, TrajTiming,
};
use chrono::{Local, TimeZone};
use serde_json::{json, Value};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::sync_channel;
use std::time::{SystemTime, UNIX_EPOCH};
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
/// Detail rows kept in the native ledger. Older pages replace the newest tail
/// once this bound is reached; the overview tiles remain global, so scrolling
/// upward never requires retaining the whole thread in the Slint model.
const MAX_VIEW_ROWS: usize = 600;
const ROW_PX: f32 = 30.0;
const ROW_SUMMARY_PX: f32 = 20.0;
const USER_TURN_KEYS: [&str; 4] = ["user_turn_index", "user_round", "turn_index", "turn"];
const STARTED_KEYS: [&str; 3] = ["started_at", "start_time", "created_at"];
const COMPLETED_KEYS: [&str; 3] = ["completed_at", "end_time", "finished_at"];
const CONTEXT_RAW_KINDS: [&str; 8] = [
    "queue",
    "plan",
    "approval",
    "terminal",
    "context",
    "note",
    "status",
    "context_message",
];
const TICK_STEPS_MS: [f64; 14] = [
    100.0,
    250.0,
    500.0,
    1_000.0,
    2_000.0,
    5_000.0,
    10_000.0,
    30_000.0,
    60_000.0,
    120_000.0,
    300_000.0,
    600_000.0,
    1_800_000.0,
    3_600_000.0,
];

/// Return the same deterministic shape as the web export download:
/// `<session-id>-<Date.now()>.jsonl`.
///
/// The native save dialog uses this as its initial filename.  Keep the
/// sanitization at this boundary because a cloud/session identifier can come
/// from a remote service and may contain characters rejected by Windows.
fn trajectory_export_filename(session_id: &str, timestamp_ms: i128) -> String {
    let value = session_id.trim();
    let mut safe = String::with_capacity(value.len());
    let mut previous_invalid = false;
    let mut previous_whitespace = false;
    for ch in value.chars() {
        let invalid =
            ch.is_control() || matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|');
        if invalid {
            if !previous_invalid {
                safe.push('_');
            }
            previous_invalid = true;
            previous_whitespace = false;
        } else if ch.is_whitespace() {
            if !previous_whitespace {
                safe.push('_');
            }
            previous_whitespace = true;
            previous_invalid = false;
        } else {
            safe.push(ch);
            previous_invalid = false;
            previous_whitespace = false;
        }
    }
    let prefix = if safe.is_empty() { "thread" } else { &safe };
    format!("{prefix}-{}.jsonl", timestamp_ms.max(0))
}

fn trajectory_export_filename_now(session_id: &str) -> String {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    trajectory_export_filename(session_id, timestamp_ms as i128)
}

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
}

#[derive(Clone)]
struct Metrics {
    step_start: Option<f64>,
    first_token: Option<f64>,
    completed: Option<f64>,
    output_tokens: Option<f64>,
}

#[derive(Clone)]
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
    detail_ref: Option<(String, i64)>,
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
    /// Turn-local identity of the group that issued the call; shared by every
    /// record of that step so a request can be located from any of its rows.
    identity: String,
    assistant_index: usize,
    first_record: usize,
}

struct Record {
    turn: Option<i64>,
    group: String,
    turn_start: bool,
    cell: usize,
}

#[derive(Clone)]
struct SearchHit {
    item_id: String,
    body_id: String,
    revision: i64,
    cursor: i64,
    snippet: String,
}

struct SpanBase {
    start: f64,
    end: f64,
    record: usize,
    kind: i32,
    lane: usize,
    ttft: Option<f64>,
    /// Sequence-domain coordinates for an overview tile. Detail rows use
    /// their record index instead; keeping the two domains separate avoids
    /// pretending that a 100-row page represents the whole thread.
    sequence_start: Option<f64>,
    sequence_end: Option<f64>,
}

/// Parsed snapshot plus derived projections, built as a unit on the worker
/// thread and handed to the UI state in one piece.
struct SnapshotModel {
    cells: Vec<Cell>,
    records: Vec<Record>,
    requests: Vec<Request>,
    spans: Vec<SpanBase>,
    // (turn, wall-clock start in ms, first record index)
    boundaries: Vec<(i64, f64, usize)>,
    tl_start: f64,
    tl_end: f64,
    /// Session-wide aggregate tiles are retained separately from detail rows.
    /// They are projected even when the first page is bounded, so a large
    /// thread keeps the same segmented overview as the web client.
    overview_spans: Vec<SpanBase>,
    overview_start: f64,
    overview_end: f64,
    overview_sequence_end: f64,
    has_overview: bool,
    /// Complete timing metadata, independent of the bounded ledger window.
    overview_exact: bool,
    overview_error_buckets: HashSet<i64>,
}

#[derive(Default)]
struct State {
    session: String,
    cloud: bool,
    generation: u64,
    model: Option<SnapshotModel>,
    page: Option<Value>,
    next_before: Option<i64>,
    has_more: bool,
    next_after: Option<i64>,
    has_newer: bool,
    loading_page: bool,
    page_failed: bool,
    detail_generation: u64,
    detail_loading: bool,
    /// Only the selected body is hydrated; restore its small projection on
    /// selection changes instead of retaining up to 600 large payloads.
    detail_original: Option<Cell>,
    older_edge_latched: bool,
    newer_edge_latched: bool,
    view_start: f64,
    view_width: f64,
    selected: i32,
    tab: i32,
    show_duration: bool,
    query: String,
    search_generation: u64,
    search_hits: Vec<SearchHit>,
    search_text: String,
    selected_search: Option<usize>,
    selection: Option<(f64, f64)>,
    search_timer: Option<slint::Timer>,
    collapsed_turns: HashSet<i64>,
    collapsed_assistants: HashSet<String>,
}

impl State {
    fn reset(&mut self, session: String) {
        let show_duration = self.show_duration;
        let detail_generation = self.detail_generation.wrapping_add(1);
        let generation = self.generation.wrapping_add(1);
        let search_generation = self.search_generation.wrapping_add(1);
        let detail_loading = self.detail_loading;
        *self = State {
            session,
            show_duration,
            view_width: 1.0,
            selected: -1,
            detail_generation,
            generation,
            search_generation,
            detail_loading,
            ..State::default()
        };
    }
    fn identity_key(turn: Option<i64>, group: &str) -> String {
        format!(
            "{}\u{0}{}",
            turn.map(|t| t.to_string()).unwrap_or_else(|| "none".into()),
            group
        )
    }
}

fn to_model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

fn project_search(app: &MainWindow, state: &State) {
    let hits = state
        .search_hits
        .iter()
        .map(|hit| TrajSearchHit {
            item_id: hit.item_id.clone().into(),
            body_id: hit.body_id.clone().into(),
            revision: hit.revision.to_string().into(),
            cursor: hit.cursor.to_string().into(),
            snippet: hit.snippet.clone().into(),
        })
        .collect();
    app.set_traj_search_hits(to_model(hits));
    app.set_traj_search_body(state.search_text.clone().into());
}

// ------------------------------------------------------------------ */
// Snapshot parsing (pure; runs on the worker thread)                  */
// ------------------------------------------------------------------ */
fn payload_of(item: &Value) -> &Value {
    item.get("payload")
        .filter(|v| v.is_object())
        .unwrap_or(&Value::Null)
}

fn meta_of<'a>(payload: &'a Value) -> &'a Value {
    payload
        .get("meta")
        .filter(|v| v.is_object())
        .unwrap_or(&Value::Null)
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
    meta_of(payload)
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
}

/// 时间字段统一为毫秒；秒级数值按 1e11 阈值启发式放大。
fn timestamp_ms(value: &Value) -> Option<f64> {
    if let Some(number) = value.as_f64() {
        if number.is_finite() {
            return Some(if number.abs() < 1e11 {
                number * 1000.0
            } else {
                number
            });
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

fn add_usage_source<'a>(value: Option<&'a Value>, sources: &mut Vec<&'a Value>) {
    let Some(source) = value.filter(|v| v.is_object()) else {
        return;
    };
    sources.push(source);
    for key in [
        "usage",
        "round_usage",
        "roundUsage",
        "message_stats",
        "messageStats",
    ] {
        if let Some(nested) = source.get(key).filter(|v| v.is_object()) {
            sources.push(nested);
            // Some providers use data.messageStats.roundUsage (and older
            // writers use the snake_case equivalent). Keep this bounded to
            // known usage containers rather than recursively walking payloads.
            for nested_key in ["usage", "round_usage", "roundUsage"] {
                if let Some(deep) = nested.get(nested_key).filter(|v| v.is_object()) {
                    sources.push(deep);
                }
            }
        }
    }
}

fn extract_usage(payload: &Value) -> Option<Usage> {
    // Providers and persisted history use both snake_case and camelCase. The
    // writer may also wrap usage in stats/data/message_stats/round_usage, so
    // inspect only this bounded set of known containers.
    let mut sources: Vec<&Value> = Vec::new();
    add_usage_source(payload.get("usage"), &mut sources);
    add_usage_source(payload.get("round_usage"), &mut sources);
    add_usage_source(payload.get("roundUsage"), &mut sources);
    add_usage_source(payload.get("stats"), &mut sources);
    add_usage_source(payload.get("message_stats"), &mut sources);
    add_usage_source(payload.get("messageStats"), &mut sources);
    add_usage_source(payload.get("data"), &mut sources);
    add_usage_source(meta_of(payload).get("message_stats"), &mut sources);
    add_usage_source(meta_of(payload).get("messageStats"), &mut sources);
    add_usage_source(Some(payload), &mut sources);
    if sources.is_empty() {
        return None;
    }
    let numeric = |value: &Value| {
        value
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|text| text.trim().parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.0)
            })
    };
    let pick = |keys: &[&str]| {
        for source in &sources {
            for key in keys {
                if let Some(value) = source.get(*key).and_then(numeric) {
                    return Some(value);
                }
            }
        }
        None
    };
    let usage = Usage {
        input: pick(&[
            "input_tokens",
            "prompt_tokens",
            "inputTokens",
            "promptTokens",
            "input",
            "prompt",
        ]),
        cache_read: pick(&[
            "cached_input_tokens",
            "cache_read_input_tokens",
            "cached_tokens",
            "cacheReadTokens",
            "cacheReadInputTokens",
            "cache_read_tokens",
        ]),
        cache_write: pick(&[
            "cache_creation_input_tokens",
            "cacheWriteTokens",
            "cacheCreationInputTokens",
        ]),
        output: pick(&[
            "output_tokens",
            "completion_tokens",
            "outputTokens",
            "completionTokens",
            "output",
            "completion",
        ]),
        think: pick(&["reasoning_tokens", "reasoningTokens", "reasoning"]),
    };
    usage.any().then_some(usage)
}

/// 历史遗留的噪音条目：传输态与统计通知（`*_delta`/heartbeat/context_usage/
/// progress/workspace_update/model_usage/llm_stream_retry 不产生条目，但旧库
/// 里可能残留）、队列瞬态（含 user_withdrew 取消回执）、会话写入器重复落账
/// 的工具结果（tool_call 条目已合并参数与结果）、命令子会话生命周期、知识
/// 检索与 hook 通知、轮次开始回执与 admin 模型请求 —— 对轨迹都没有可读价值。
fn is_skipped_raw_kind(raw_kind: &str) -> bool {
    matches!(
        raw_kind,
        "progress"
            | "workspace_update"
            | "model_usage"
            | "llm_stream_retry"
            | "thread_turn_started"
            | "model_call"
            | "queue"
            | "tool_message"
            | "knowledge_request"
    ) || raw_kind.starts_with("queue_")
        || raw_kind.starts_with("command_session")
        || raw_kind.starts_with("hook/")
}

fn merge_page_json(current: &Value, incoming: &Value, older: bool) -> Value {
    let mut merged = current.clone();
    let mut rows = if older {
        incoming
            .get("trajectory")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    } else {
        current
            .get("trajectory")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    if older {
        rows.extend(
            current
                .get("trajectory")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
    } else {
        rows.extend(
            incoming
                .get("trajectory")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
    }
    let evicted_newer = older && rows.len() > MAX_VIEW_ROWS;
    if rows.len() > MAX_VIEW_ROWS {
        if older {
            rows.truncate(MAX_VIEW_ROWS);
        } else {
            rows = rows.split_off(rows.len() - MAX_VIEW_ROWS);
        }
    }
    if let Some(map) = merged.as_object_mut() {
        let first_seq = rows
            .first()
            .and_then(|row| row.get("created_seq"))
            .cloned()
            .unwrap_or(Value::Null);
        let last_seq = rows
            .last()
            .and_then(|row| row.get("created_seq"))
            .cloned()
            .unwrap_or(Value::Null);
        map.insert("trajectory".into(), Value::Array(rows));
        // Tiles describe the whole thread and are repeated on every page. Keep
        // the newest response's bounded overview rather than duplicating spans.
        map.insert(
            "tiles".into(),
            current
                .get("tiles")
                .cloned()
                .unwrap_or(Value::Array(Vec::new())),
        );
        map.insert(
            "tile_level".into(),
            current.get("tile_level").cloned().unwrap_or(Value::from(0)),
        );
        for field in ["timeline", "timeline_has_more"] {
            if let Some(value) = incoming.get(field) {
                map.insert(field.into(), value.clone());
            }
        }
        map.insert("next_before".into(), first_seq);
        map.insert("next_after".into(), last_seq);
        map.insert(
            "has_more".into(),
            if older {
                incoming
                    .get("has_more")
                    .cloned()
                    .unwrap_or(Value::Bool(false))
            } else {
                current
                    .get("has_more")
                    .cloned()
                    .unwrap_or(Value::Bool(false))
            },
        );
        map.insert(
            "has_newer".into(),
            if older {
                Value::Bool(
                    evicted_newer
                        || current
                            .get("has_newer")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                )
            } else {
                incoming
                    .get("has_newer")
                    .cloned()
                    .unwrap_or(Value::Bool(false))
            },
        );
    }
    merged
}

/// Convert the shared bounded presentation page into the same compact model
/// used by the existing Slint projection. The page contains only a keyset
/// window of detail rows; its tiles are retained separately so the overview
/// timeline still covers the complete thread.
fn parse_view_page(page: &Value) -> SnapshotModel {
    let rows = page
        .get("trajectory")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut turns = Vec::new();
    let mut turn_index: HashMap<String, usize> = HashMap::new();
    let mut cells = Vec::new();
    for row in &rows {
        let turn_id = row.get("turn_id").and_then(Value::as_str).unwrap_or("");
        if turn_id.is_empty() {
            continue;
        }
        let turn_no = row.get("user_round").and_then(Value::as_i64);
        let ti = *turn_index.entry(turn_id.to_string()).or_insert_with(|| {
            turns.push(TurnModel {
                turn: turn_no,
                groups: Vec::new(),
            });
            turns.len() - 1
        });
        let raw_kind = row.get("kind").and_then(Value::as_str).unwrap_or("context");
        let kind = match raw_kind {
            "system_message" | "system" => KIND_SYSTEM,
            "user_message" | "user" => KIND_USER,
            "assistant_message" | "assistant" => KIND_MESSAGE,
            "tool_call" | "tool_result" | "tool" => KIND_TOOL,
            "subagent" | "subagent_message" | "subagent_run" => KIND_SUBTOOL,
            "compaction" | "compacted" => KIND_COMPACTED,
            _ => KIND_CONTEXT,
        };
        let summary = row.get("summary").and_then(Value::as_str).unwrap_or("");
        let mut payload = json!({
            "content": summary,
            "tool": row.get("tool_name").and_then(Value::as_str).unwrap_or(""),
            "model_round": row.get("model_round").and_then(Value::as_i64).unwrap_or(0),
            "started_at": row.get("started_at").and_then(Value::as_f64),
            "completed_at": row.get("completed_at").and_then(Value::as_f64),
        });
        if let Some(stats) = row.get("stats").filter(|value| value.is_object()) {
            payload["message_stats"] = stats.clone();
        }
        if kind == KIND_SYSTEM {
            payload["meta"] = json!({"type":"system_prompt"});
        }
        let item = json!({
            "item_id": row.get("item_id").and_then(Value::as_str).unwrap_or(""),
            "turn_id": turn_id,
            "kind": raw_kind,
            "status": row.get("status").and_then(Value::as_str).unwrap_or("completed"),
            "payload": payload,
            "created_time": row.get("started_at").and_then(Value::as_f64).unwrap_or(0.0),
            "updated_time": row.get("completed_at").and_then(Value::as_f64).unwrap_or(0.0),
            "created_seq": row.get("created_seq").and_then(Value::as_i64).unwrap_or(0),
            "item_index": row.get("item_index").and_then(Value::as_i64).unwrap_or(0),
            "detail": row.get("detail"),
        });
        // Constructing a small synthetic snapshot keeps all existing grouping,
        // request numbering and detail formatting rules in one place.
        cells.push((item, ti, kind));
    }
    let mut snapshot_turns = Vec::new();
    let mut snapshot_items = Vec::new();
    for (item, ti, _) in cells {
        let turn_id = item.get("turn_id").and_then(Value::as_str).unwrap_or("");
        let turn_no = turns.get(ti).and_then(|turn| turn.turn).unwrap_or(0);
        if !snapshot_turns
            .iter()
            .any(|turn: &Value| turn["turn_id"] == turn_id)
        {
            snapshot_turns.push(json!({"turn_id": turn_id, "user_turn_index": turn_no}));
        }
        snapshot_items.push(item);
    }
    let mut model = parse_snapshot(&json!({"turns": snapshot_turns, "items": snapshot_items}));
    // The detail parser assigns sequence positions relative to the current
    // bounded page. Restore the durable session sequence before combining it
    // with aggregate buckets, otherwise every page appears at the origin.
    let sequence_by_item: HashMap<&str, f64> = rows
        .iter()
        .filter_map(|row| {
            Some((
                row.get("item_id")?.as_str()?,
                row.get("created_seq")?.as_i64()? as f64,
            ))
        })
        .collect();
    for span in &mut model.spans {
        if let Some(record) = model.records.get(span.record) {
            if let Some(cell) = model.cells.get(record.cell) {
                if let Some(sequence) = sequence_by_item.get(cell.record_id.as_str()) {
                    span.sequence_start = Some(*sequence);
                    span.sequence_end = Some(*sequence + 1.0);
                }
            }
        }
    }
    if apply_timing_overview(&mut model, page) {
        return model;
    }
    let mut overview_start = f64::MAX;
    let mut overview_end = f64::MIN;
    let mut overview_sequence_end: f64 = 1.0;
    let mut overview_error_buckets = HashSet::new();
    let tiles = page
        .get("tiles")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_tiles = !tiles.is_empty();
    let level = page.get("tile_level").and_then(Value::as_u64).unwrap_or(0) as f64;
    let bucket_width = 128.0 * 8.0_f64.powf(level);
    // Subtract the exact geometry which will be painted, rather than raw
    // persistence timestamps. Context/input rows are instantaneous in the
    // detail model even if their persisted completion timestamp is later.
    // Cutting the raw interval would remove coverage that no detail span
    // restores, changing the compressed domain as more pages are loaded.
    let detailed_ranges: Vec<(f64, f64)> = model
        .spans
        .iter()
        .map(|span| (span.start, span.end.max(span.start)))
        .collect();
    let mut overview = Vec::new();
    let mut start = f64::MAX;
    let mut end = f64::MIN;
    let per_tile_limit = (4096 / tiles.len().max(1)).clamp(1, 16);
    let overview_span_cap = 4096usize.saturating_sub(model.spans.len());
    'tiles: for tile in tiles {
        let bucket_index = tile.get("bucket").and_then(Value::as_i64).unwrap_or(0);
        let bucket = bucket_index as f64;
        let count = tile
            .get("count")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0);
        if count == 0 {
            continue;
        }
        let lane_counts = [
            tile.get("input_count").and_then(Value::as_i64).unwrap_or(0),
            tile.get("model_count").and_then(Value::as_i64).unwrap_or(0),
            tile.get("tool_count").and_then(Value::as_i64).unwrap_or(0),
        ];
        let total = lane_counts.iter().sum::<i64>();
        let counts = if total > 0 {
            lane_counts
        } else {
            [count / 5, count / 2, count - count / 5 - count / 2]
        };
        // Storage timestamps are unix seconds; the detail parser accepts both
        // seconds and milliseconds, so normalize here before joining spans.
        let ts = |key: &str| {
            let value = tile.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            if value.abs() < 1e11 {
                value * 1000.0
            } else {
                value
            }
        };
        let tile_start = ts("start");
        let tile_end = ts("end").max(tile_start + 1.0);
        start = start.min(tile_start);
        end = end.max(tile_end);
        overview_sequence_end = overview_sequence_end.max((bucket + 1.0) * bucket_width);
        if tile.get("failed").and_then(Value::as_i64).unwrap_or(0) > 0 {
            overview_error_buckets.insert(bucket_index);
        }
        // Loaded records are drawn with their exact event type and timing.
        // Subtract their sequence/time windows from each aggregate segment to
        // avoid painting a broad block beneath detailed marks.
        let total_count = counts.iter().sum::<i64>().max(count);
        let segment_count = (total_count as usize).clamp(1, per_tile_limit);
        let segment_start = bucket * bucket_width;
        let segment_span = bucket_width / segment_count as f64;
        let total_lane_count = counts
            .iter()
            .map(|value| (*value).max(0))
            .sum::<i64>()
            .max(1);
        for segment in 0..segment_count {
            let quantile = (segment as f64 + 0.5) / segment_count as f64 * total_lane_count as f64;
            let mut accumulated = 0i64;
            let mut lane = 2usize;
            for candidate in 0..3 {
                accumulated += counts[candidate].max(0);
                if quantile <= accumulated as f64 {
                    lane = candidate;
                    break;
                }
            }
            let start_fraction = segment as f64 / segment_count as f64;
            let end_fraction = (segment + 1) as f64 / segment_count as f64;
            let raw_time_start = tile_start + (tile_end - tile_start) * start_fraction;
            let raw_time_end = tile_start + (tile_end - tile_start) * end_fraction;
            let raw_sequence_start = segment_start + segment_span * segment as f64;
            let raw_sequence_end = segment_start + segment_span * (segment + 1) as f64;
            let kind = if lane == 0 {
                KIND_CONTEXT
            } else if lane == 1 {
                KIND_MESSAGE
            } else {
                KIND_TOOL
            };
            let mut fragments = vec![(
                raw_time_start,
                raw_time_end,
                raw_sequence_start,
                raw_sequence_end,
            )];
            for &(detail_time_start, detail_time_end) in &detailed_ranges {
                if detail_time_start <= raw_time_end && detail_time_end >= raw_time_start {
                    let cut_start = detail_time_start.max(raw_time_start);
                    let cut_end = detail_time_end.min(raw_time_end);
                    let mut next = Vec::new();
                    for (a, b, sa, sb) in fragments {
                        if cut_end <= a || cut_start >= b {
                            next.push((a, b, sa, sb));
                            continue;
                        }
                        let to_sequence =
                            |time: f64| sa + (time - a) / (b - a).max(0.001) * (sb - sa);
                        if cut_start > a {
                            next.push((a, cut_start, sa, to_sequence(cut_start)));
                        }
                        if cut_end < b {
                            next.push((cut_end, b, to_sequence(cut_end), sb));
                        }
                    }
                    fragments = next;
                    if fragments.is_empty() {
                        break;
                    }
                }
            }
            for row in &rows {
                let Some(sequence) = row
                    .get("created_seq")
                    .and_then(Value::as_i64)
                    .map(|value| value as f64)
                else {
                    continue;
                };
                if sequence < raw_sequence_start || sequence >= raw_sequence_end {
                    continue;
                }
                let mut next = Vec::new();
                for (a, b, sa, sb) in fragments {
                    if sequence < sa || sequence >= sb {
                        next.push((a, b, sa, sb));
                        continue;
                    }
                    if sequence > sa {
                        next.push((a, b, sa, sequence));
                    }
                    if sequence + 1.0 < sb {
                        next.push((a, b, sequence + 1.0, sb));
                    }
                }
                fragments = next;
            }
            for (time_start, time_end, sequence_start, sequence_end) in fragments {
                if time_end <= time_start || sequence_end <= sequence_start {
                    continue;
                }
                if overview.len() >= overview_span_cap {
                    break 'tiles;
                }
                overview.push(SpanBase {
                    start: time_start,
                    end: time_end,
                    record: usize::MAX,
                    kind: if overview_error_buckets.contains(&bucket_index) {
                        KIND_TOOL
                    } else {
                        kind
                    },
                    lane,
                    ttft: None,
                    sequence_start: Some(sequence_start),
                    sequence_end: Some(sequence_end),
                });
            }
        }
    }
    // Even when aggregate marks are fully subtracted by current detail,
    // retain the complete wall-clock domain for correct axis/zoom bounds.
    for row in &rows {
        let Some(raw_start) = row.get("started_at").and_then(Value::as_f64) else {
            continue;
        };
        let raw_end = row
            .get("completed_at")
            .and_then(Value::as_f64)
            .unwrap_or(raw_start);
        let start = if raw_start.abs() < 1e11 {
            raw_start * 1000.0
        } else {
            raw_start
        };
        let finish = if raw_end.abs() < 1e11 {
            raw_end * 1000.0
        } else {
            raw_end
        };
        overview_start = overview_start.min(start);
        overview_end = overview_end.max(finish.max(start + 1.0));
    }
    if start.is_finite() && end.is_finite() {
        overview_start = overview_start.min(start);
        overview_end = overview_end.max(end.max(start + 1.0));
    }
    if !overview_start.is_finite() || !overview_end.is_finite() {
        overview_start = 0.0;
        overview_end = 1.0;
    }
    let has_overview = has_tiles && start.is_finite() && end.is_finite();

    // Tiles and detail rows share one wall-clock coordinate system.  Applying
    // idle compression to only one side makes the overview look sparse and,
    // more seriously, makes a selected detail row land at a different x
    // coordinate after the overview is painted.  Build one mapping from the
    // union of both sets, then apply the same offset to every span.  A tile is
    // deliberately part of the coverage set: when the ledger page contains a
    // row outside the bounded tile range, its gap is still collapsed in the
    // same way as the aggregate projection.
    let mut overview_spans = overview;
    if has_overview {
        compress_idle_span_sets(&mut model.spans, &mut overview_spans);
        refresh_timing_domain(&mut model);
        let (compressed_start, compressed_end) = span_domain(&model.spans, &overview_spans);
        if compressed_start.is_finite() && compressed_end.is_finite() {
            overview_start = compressed_start;
            overview_end = compressed_end.max(compressed_start + 1.0);
        }
    }
    model.overview_spans = overview_spans;
    model.overview_start = overview_start;
    model.overview_end = overview_end;
    model.overview_sequence_end = overview_sequence_end;
    model.has_overview = has_overview;
    model.overview_error_buckets = overview_error_buckets;
    if !model.has_overview {
        compress_idle_spans(&mut model.spans);
        refresh_timing_domain(&mut model);
    }
    model
}

fn lane_for(kind: i32) -> usize {
    match kind {
        KIND_TOOL | KIND_SUBTOOL => 2,
        KIND_MESSAGE | KIND_COMPACTED => 1,
        _ => 0,
    }
}

fn compress_idle_spans(spans: &mut [SpanBase]) {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by(|&left, &right| {
        spans[left]
            .start
            .total_cmp(&spans[right].start)
            .then_with(|| spans[left].end.total_cmp(&spans[right].end))
    });
    let mut removed = 0.0;
    let mut covered: Option<f64> = None;
    for index in order {
        let span = &mut spans[index];
        if let Some(end) = covered {
            removed += (span.start - end).max(0.0);
        }
        covered = Some(covered.map_or(span.end, |end| end.max(span.end)));
        span.start -= removed;
        span.end -= removed;
    }
}

/// Compress idle gaps in two projections with one shared mapping.
///
/// `overview_spans` are aggregate tile fragments while `detail_spans` are the
/// currently loaded ledger rows.  They can overlap and are intentionally
/// sorted together so an aggregate tile also contributes to the covered
/// interval.  The offset is sampled at each span's start (the same rule as the
/// single-slice compressor), preserving the duration of every span.
fn compress_idle_span_sets(detail_spans: &mut [SpanBase], overview_spans: &mut [SpanBase]) {
    #[derive(Clone, Copy)]
    struct Entry {
        start: f64,
        end: f64,
        source: u8,
        index: usize,
    }

    let mut entries = Vec::with_capacity(detail_spans.len() + overview_spans.len());
    entries.extend(detail_spans.iter().enumerate().map(|(index, span)| Entry {
        start: span.start,
        end: span.end.max(span.start),
        source: 0,
        index,
    }));
    entries.extend(
        overview_spans
            .iter()
            .enumerate()
            .map(|(index, span)| Entry {
                start: span.start,
                end: span.end.max(span.start),
                source: 1,
                index,
            }),
    );
    entries.sort_by(|left, right| {
        left.start
            .total_cmp(&right.start)
            .then_with(|| left.end.total_cmp(&right.end))
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.index.cmp(&right.index))
    });

    let mut detail_offsets = vec![0.0; detail_spans.len()];
    let mut overview_offsets = vec![0.0; overview_spans.len()];
    let mut removed = 0.0;
    let mut covered: Option<f64> = None;
    for entry in entries {
        if let Some(end) = covered {
            removed += (entry.start - end).max(0.0);
        }
        if entry.source == 0 {
            detail_offsets[entry.index] = removed;
        } else {
            overview_offsets[entry.index] = removed;
        }
        covered = Some(covered.map_or(entry.end, |end| end.max(entry.end)));
    }
    for (span, offset) in detail_spans.iter_mut().zip(detail_offsets) {
        span.start -= offset;
        span.end -= offset;
    }
    for (span, offset) in overview_spans.iter_mut().zip(overview_offsets) {
        span.start -= offset;
        span.end -= offset;
    }
}

fn span_domain(detail_spans: &[SpanBase], overview_spans: &[SpanBase]) -> (f64, f64) {
    detail_spans
        .iter()
        .chain(overview_spans.iter())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), span| {
            (lo.min(span.start), hi.max(span.end.max(span.start)))
        })
}

fn refresh_timing_domain(model: &mut SnapshotModel) {
    let (lo, hi) = model
        .spans
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), span| {
            (lo.min(span.start), hi.max(span.end))
        });
    if lo.is_finite() && hi.is_finite() {
        model.tl_start = lo;
        model.tl_end = hi.max(lo + 1.0);
    }
    for (_, time, record) in &mut model.boundaries {
        if let Some(span) = model.spans.iter().find(|span| span.record == *record) {
            *time = span.start;
        }
    }
}

/// The shared engine caps this stream at 1024 timing-only rows. Never mistake
/// an incomplete tail for the whole thread, or add the detail window twice.
fn apply_timing_overview(model: &mut SnapshotModel, page: &Value) -> bool {
    if page.get("timeline_has_more").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    let Some(rows) = page
        .get("timeline")
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty() && rows.len() <= 1024)
    else {
        return false;
    };
    let record_by_sequence: HashMap<i64, usize> = model
        .spans
        .iter()
        .filter_map(|span| Some((span.sequence_start? as i64, span.record)))
        .collect();
    let mut spans: Vec<SpanBase> = rows
        .iter()
        .filter_map(|row| {
            let start = timestamp_ms(row.get("started_at")?).filter(|value| *value > 0.0)?;
            let sequence = row.get("created_seq")?.as_i64()?;
            let kind = resolve_kind(
                row.get("kind").and_then(Value::as_str).unwrap_or("context"),
                &Value::Null,
            );
            let lane = lane_for(kind);
            let end = if lane == 0 {
                start
            } else {
                row.get("completed_at")
                    .and_then(timestamp_ms)
                    .unwrap_or(start)
                    .max(start)
            };
            Some(SpanBase {
                start,
                end,
                record: record_by_sequence
                    .get(&sequence)
                    .copied()
                    .unwrap_or(usize::MAX),
                kind,
                lane,
                ttft: None,
                sequence_start: Some(sequence as f64),
                sequence_end: Some(sequence as f64 + 1.0),
            })
        })
        .collect();
    if spans.is_empty() {
        return false;
    }
    compress_idle_spans(&mut spans);
    let times: HashMap<i64, (f64, f64)> = spans
        .iter()
        .filter_map(|span| Some((span.sequence_start? as i64, (span.start, span.end))))
        .collect();
    for span in &mut model.spans {
        if let Some((start, end)) = span.sequence_start.and_then(|seq| times.get(&(seq as i64))) {
            span.start = *start;
            span.end = *end;
        }
    }
    refresh_timing_domain(model);
    model.overview_start = spans
        .iter()
        .map(|span| span.start)
        .fold(f64::INFINITY, f64::min);
    model.overview_end = spans
        .iter()
        .map(|span| span.end)
        .fold(f64::NEG_INFINITY, f64::max)
        .max(model.overview_start + 1.0);
    model.overview_sequence_end = spans
        .iter()
        .filter_map(|span| span.sequence_end)
        .fold(1.0, f64::max);
    model.overview_spans = spans;
    model.has_overview = true;
    model.overview_exact = true;
    true
}

fn resolve_kind(raw_kind: &str, payload: &Value) -> i32 {
    if meta_of(payload).get("type").and_then(Value::as_str) == Some("system_prompt") {
        return KIND_SYSTEM;
    }
    match raw_kind {
        "assistant_message" | "assistant" => KIND_MESSAGE,
        "subagent" | "subagent_run" | "subagent_message" => KIND_SUBTOOL,
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
        text.chars()
            .take(TEXT_PREVIEW_SOURCE_LIMIT)
            .collect::<String>()
            + "…"
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
    let plain = plain
        .replace("***", "")
        .replace("**", "")
        .replace("__", "")
        .replace("~~", "");
    if plain.chars().count() > TEXT_PREVIEW_LIMIT {
        plain.chars().take(TEXT_PREVIEW_LIMIT).collect::<String>() + "…"
    } else {
        plain.trim().to_string()
    }
}

/// Keep a ledger row's hot-path preview on one physical line.  The detail
/// inspector still receives the original multiline payload, but fixed-height
/// rows must never let source newlines change their measured height and overlap
/// the next row.
fn single_line_preview(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() > TEXT_PREVIEW_LIMIT {
        compact.chars().take(TEXT_PREVIEW_LIMIT).collect::<String>() + "…"
    } else {
        compact
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

fn parse_duration_cell(
    payload: &Value,
    started_at: Option<f64>,
    completed_at: Option<f64>,
) -> Option<f64> {
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
    let record_id = item
        .get("item_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
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
        detail_ref: item.get("detail").and_then(|detail| {
            Some((
                detail.get("body_id")?.as_str()?.to_owned(),
                detail.get("revision")?.as_i64()?,
            ))
        }),
    };

    match kind {
        KIND_MESSAGE => {
            let content_text = content_to_text(payload.get("content").unwrap_or(&Value::Null));
            // 流式壳写入 `reasoning`；会话写入器覆盖 payload 后字段名为
            // `reasoning_content`，两个来源都要读。
            let reasoning_text = content_to_text(payload.get("reasoning").unwrap_or(&Value::Null));
            let thinking_text = if reasoning_text.trim().is_empty() {
                content_to_text(payload.get("reasoning_content").unwrap_or(&Value::Null))
            } else {
                reasoning_text
            };
            let tool_calls = payload.get("tool_calls");
            if content_text.trim().is_empty() && tool_calls.map(Value::is_array).unwrap_or(false) {
                cell.tool_call_only = true;
            } else {
                cell.text = markdown_preview_text(&content_text);
            }
            cell.output_detail = non_empty(&content_text);
            cell.thinking_detail = non_empty(&thinking_text);
            let ttft = payload
                .get("ttft_ms")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite());
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
                    payload
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|v| !v.trim().is_empty())
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
                Some(value @ Value::Object(_)) => serde_json::to_string_pretty(value)
                    .unwrap_or_default()
                    .into(),
                _ => None,
            };
            cell.is_error = error_value == Some(&json!(true))
                || error_value
                    .map(|v| !v.is_null() && *v != json!(false))
                    .unwrap_or(false)
                || status == "failed"
                || status == "error";
            if let (Some(start), Some(end)) = (started_at, completed_at) {
                cell.time_seconds = Some(((end - start) / 1000.0).max(0.0));
            }
        }
        KIND_COMPACTED => {
            let summary_text = [
                "summary_text",
                "summaryText",
                "summary_model_output",
                "summaryModelOutput",
                "summary",
                "content",
            ]
            .iter()
            .filter_map(|key| payload.get(*key))
            .map(content_to_text)
            .find(|value| !value.trim().is_empty() && !value.trim().eq_ignore_ascii_case("history"))
            .unwrap_or_default();
            cell.text = single_line_preview(&summary_text);
            cell.output_detail = non_empty(&summary_text);
        }
        _ => {
            let content_text = content_to_text(payload.get("content").unwrap_or(&Value::Null));
            cell.text = markdown_preview_text(&content_text);
            cell.input_detail = non_empty(&content_text);
            cell.time_seconds = started_at.map(|_| 0.0);
        }
    }
    cell
}

/// 把快照折叠成 轮次 → 步骤组 → 记录；items 按 turn_id 归组进各自的轮次。
fn parse_snapshot(snapshot: &Value) -> SnapshotModel {
    let empty = Vec::new();
    let turns_raw = snapshot
        .get("turns")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let items_raw = snapshot
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or(&empty);

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
            raw_turn
                .get(*key)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .map(|v| v as i64)
        });
        let turn_id = raw_turn
            .get("turn_id")
            .and_then(Value::as_str)
            .unwrap_or("");
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
            // 传输态/统计通知、队列瞬态、重复落账等噪音条目不进时间线；
            // 后端已白名单化不再入账，这里跳过历史遗留数据（不占记录索引）。
            if is_skipped_raw_kind(&raw_kind) {
                continue;
            }
            // 成功的轮次终止只是生命周期回执；失败终止保留（携带错误详情）。
            let item_status = item
                .get("status")
                .and_then(Value::as_str)
                .or_else(|| payload.get("status").and_then(Value::as_str));
            if matches!(raw_kind.as_str(), "terminal" | "turn_terminal")
                && item_status == Some("completed")
                && payload.get("error").map_or(true, Value::is_null)
            {
                continue;
            }
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
                        if candidate_payload
                            .get("tool_call_id")
                            .and_then(Value::as_str)
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
                let step = payload
                    .get("model_round")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite());
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
                let step = payload
                    .get("model_round")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite());
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
        turns.push(TurnModel {
            turn: turn_no,
            groups,
        });
    }

    // requests: one per model-call group, numbered in ledger order
    let mut requests: Vec<Request> = Vec::new();
    for turn in &turns {
        for group in &turn.groups {
            if group.compaction || group.step.is_none() {
                continue;
            }
            let Some(message_cell) = group
                .cells
                .iter()
                .map(|&i| &cells[i])
                .find(|cell| cell.kind == KIND_MESSAGE)
            else {
                continue;
            };
            requests.push(Request {
                number: requests.len() + 1,
                identity: State::identity_key(turn.turn, &group.identity),
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
        // Reused request groups must not move later tools ahead of context rows.
        records[turn_start..].sort_by_key(|record| record.cell);
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
        let Some(start) = cell.started_at else {
            continue;
        };
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
            sequence_start: Some(record_index as f64),
            sequence_end: Some(record_index as f64 + 1.0),
        });
    }
    let (lo, hi) = spans.iter().fold((f64::MAX, f64::MIN), |(lo, hi), span| {
        (lo.min(span.start), hi.max(span.end))
    });
    let (tl_start, tl_end) = if spans.is_empty() {
        (0.0, 1.0)
    } else {
        (lo, hi.max(lo + 1.0))
    };

    SnapshotModel {
        cells,
        records,
        requests,
        spans,
        boundaries,
        tl_start,
        tl_end,
        overview_spans: Vec::new(),
        overview_start: 0.0,
        overview_end: 1.0,
        overview_sequence_end: 1.0,
        has_overview: false,
        overview_exact: false,
        overview_error_buckets: HashSet::new(),
    }
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
            KIND_SYSTEM => {
                if zh {
                    "系统"
                } else {
                    "System"
                }
            }
            KIND_USER => {
                if zh {
                    "用户"
                } else {
                    "User"
                }
            }
            KIND_CONTEXT => {
                if zh {
                    "上下文"
                } else {
                    "Context"
                }
            }
            KIND_COMPACTED => {
                if zh {
                    "已压缩"
                } else {
                    "Compacted"
                }
            }
            KIND_MESSAGE => {
                if zh {
                    "助手"
                } else {
                    "Assistant"
                }
            }
            KIND_TOOL => {
                if zh {
                    "工具"
                } else {
                    "Tool"
                }
            }
            KIND_SUBTOOL => {
                if zh {
                    "子智能体"
                } else {
                    "Subagent"
                }
            }
            _ => "—",
        }
    }
    fn state(&self, cell: &Cell) -> &'static str {
        let zh = self.zh;
        if cell.is_error {
            if zh {
                "失败"
            } else {
                "Failed"
            }
        } else if cell
            .metrics
            .as_ref()
            .map(|m| m.completed.is_none())
            .unwrap_or(false)
        {
            if zh {
                "运行中"
            } else {
                "Running"
            }
        } else {
            if zh {
                "已完成"
            } else {
                "Completed"
            }
        }
    }
    fn not_recorded(&self) -> &'static str {
        if self.zh {
            "未记录"
        } else {
            "Not recorded"
        }
    }
    fn not_reported(&self) -> &'static str {
        if self.zh {
            "未上报"
        } else {
            "Not reported"
        }
    }
    fn millis(&self, value: f64) -> String {
        format!(
            "{} {}",
            thousands(value),
            if self.zh { "毫秒" } else { "ms" }
        )
    }
    fn seconds(&self, value: Option<f64>) -> String {
        value
            .map(|v| self.millis(v * 1000.0))
            .unwrap_or_else(|| self.not_recorded().to_string())
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
            format!("… {} 个步骤 · {} 个工具调用", steps, tools)
        } else {
            format!("… {} steps · {} tool calls", steps, tools)
        }
    }
    fn summary_tools(&self, count: usize) -> String {
        if self.zh {
            format!("… {} 个工具调用", count)
        } else {
            format!("… {} tool calls", count)
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
    Local
        .timestamp_millis_opt(ms.round() as i64)
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

#[derive(Default)]
struct FoldTurn {
    first: usize,
    hidden: usize,
    steps: HashSet<String>,
    tools: usize,
}
#[derive(Default)]
struct FoldCall {
    end: usize,
    count: usize,
    names: Vec<String>,
}
#[derive(Default)]
struct FoldGroups {
    turns: HashMap<i64, FoldTurn>,
    calls: HashMap<String, FoldCall>,
    owners: HashMap<usize, String>,
}
fn assistant_key(model: &SnapshotModel, index: usize) -> String {
    let cell = &model.cells[model.records[index].cell];
    if cell.record_id.is_empty() {
        cell.index.to_string()
    } else {
        cell.record_id.clone()
    }
}
/// One linear pass over the bounded ledger; labels never serve as fold keys.
fn fold_groups(model: &SnapshotModel) -> FoldGroups {
    let mut groups = FoldGroups::default();
    let request_groups: HashSet<String> = model
        .requests
        .iter()
        .map(|request| request.identity.clone())
        .collect();
    let mut assistant: Option<(String, Option<i64>)> = None;
    for (index, record) in model.records.iter().enumerate() {
        let kind = model.cells[record.cell].kind;
        let tool = matches!(kind, KIND_TOOL | KIND_SUBTOOL);
        if let Some(turn) = record.turn.filter(|_| kind != KIND_SYSTEM) {
            if let Some(group) = groups.turns.get_mut(&turn) {
                group.hidden += 1;
                if !record.group.is_empty() {
                    group.steps.insert(record.group.clone());
                }
                if tool {
                    group.tools += 1;
                }
            } else {
                groups.turns.insert(
                    turn,
                    FoldTurn {
                        first: index,
                        ..Default::default()
                    },
                );
            }
        }
        if let Some((key, _)) = assistant
            .as_ref()
            .filter(|(_, turn)| tool && *turn == record.turn)
        {
            let group = groups.calls.entry(key.clone()).or_default();
            group.end = index + 1;
            group.count += 1;
            let cell = &model.cells[record.cell];
            let name = cell
                .tool_name
                .as_deref()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| cell.text.split(" · ").next().unwrap_or(""));
            if !name.is_empty()
                && group.names.len() < 8
                && !group.names.iter().any(|entry| entry == name)
            {
                group.names.push(name.to_owned());
            }
            groups.owners.insert(index, key.clone());
        } else {
            assistant = (kind == KIND_MESSAGE).then(|| (assistant_key(model, index), record.turn));
        }
    }
    for (turn, group) in &mut groups.turns {
        group
            .steps
            .retain(|step| request_groups.contains(&State::identity_key(Some(*turn), step)));
    }
    groups
}
fn prune_folds(state: &mut State) {
    let groups = state.model.as_ref().map(fold_groups).unwrap_or_default();
    state
        .collapsed_turns
        .retain(|turn| groups.turns.get(turn).is_some_and(|group| group.hidden > 0));
    state
        .collapsed_assistants
        .retain(|key| groups.calls.contains_key(key));
}
fn all_turns_folded(groups: &FoldGroups, state: &State) -> bool {
    let mut eligible = groups
        .turns
        .iter()
        .filter(|(_, turn)| turn.hidden > 0)
        .peekable();
    eligible.peek().is_some() && eligible.all(|(turn, _)| state.collapsed_turns.contains(turn))
}
fn all_calls_folded(groups: &FoldGroups, state: &State) -> bool {
    !groups.calls.is_empty()
        && groups
            .calls
            .keys()
            .all(|key| state.collapsed_assistants.contains(key))
}

/// Search bypasses folds. Summary actions carry explicit kinds and normal indices.
fn project_rows(app: &MainWindow, state: &State, model: &SnapshotModel, texts: &Texts) {
    let groups = fold_groups(model);
    app.set_traj_show_turns(all_turns_folded(&groups, state));
    app.set_traj_show_calls(all_calls_folded(&groups, state));
    app.set_traj_rows(to_model(build_rows(state, model, texts)));
}
fn build_rows(state: &State, model: &SnapshotModel, texts: &Texts) -> Vec<TrajRow> {
    let needle = state.query.trim().to_lowercase();
    let groups = fold_groups(model);
    let request_numbers: HashMap<usize, usize> = model
        .requests
        .iter()
        .map(|request| (request.assistant_index, request.number))
        .collect();
    let mut rows: Vec<TrajRow> = Vec::new();
    let mut skip_until = 0;
    for (record_index, record) in model.records.iter().enumerate() {
        if record_index < skip_until {
            continue;
        }
        if !needle.is_empty() && !record_needle(model, record_index).contains(&needle) {
            continue;
        }
        let turn = record.turn.and_then(|turn| groups.turns.get(&turn));
        let turn_folded = needle.is_empty()
            && record
                .turn
                .is_some_and(|turn| state.collapsed_turns.contains(&turn))
            && turn.is_some_and(|group| group.hidden > 0);
        let mut summary: Option<(i32, String)> = None;
        if turn_folded && model.cells[record.cell].kind != KIND_SYSTEM {
            let group = turn.unwrap();
            if group.first != record_index {
                continue;
            }
            summary = Some((7, texts.summary_turn(group.steps.len(), group.tools)));
        } else if needle.is_empty() && model.cells[record.cell].kind == KIND_MESSAGE {
            let key = assistant_key(model, record_index);
            if let Some(group) = groups
                .calls
                .get(&key)
                .filter(|_| state.collapsed_assistants.contains(&key))
            {
                skip_until = group.end;
                let mut text = texts.summary_tools(group.count);
                if !group.names.is_empty() {
                    text.push_str(&format!(" · {}", group.names.join(", ")));
                }
                summary = Some((8, text));
            }
        }
        let cell = &model.cells[record.cell];
        let number = request_numbers.get(&cell.index).copied().unwrap_or(0);
        let mut text = single_line_preview(&cell.text);
        if text.trim().is_empty() {
            let zh = texts.zh;
            text = if cell.tool_call_only {
                if zh {
                    "仅工具调用"
                } else {
                    "Tool calls only"
                }
            } else if cell.kind == KIND_TOOL || cell.kind == KIND_SUBTOOL {
                if zh {
                    "未命名工具"
                } else {
                    "Unnamed tool"
                }
            } else if cell.kind == KIND_COMPACTED {
                if zh {
                    "无摘要"
                } else {
                    "No summary"
                }
            } else {
                if zh {
                    "无内容"
                } else {
                    "No content"
                }
            }
            .to_string();
        }
        let (arguments, result) = if cell.kind == KIND_TOOL || cell.kind == KIND_SUBTOOL {
            let args = cell
                .input_detail
                .as_deref()
                .unwrap_or("")
                .replace('\n', " ");
            let result = cell.result.as_deref().unwrap_or("").replace('\n', " ");
            let arguments = single_line_preview(&args.chars().take(120).collect::<String>());
            let result = single_line_preview(&result.chars().take(120).collect::<String>());
            (arguments, result)
        } else {
            (String::new(), String::new())
        };
        rows.push(TrajRow {
            kind: cell.kind,
            badge: if number > 0 {
                format!("#{}", number).into()
            } else {
                "".into()
            },
            text: text.into(),
            arguments: arguments.into(),
            result: result.into(),
            error: cell.is_error,
            request: if number > 0 { 1 } else { 0 },
            turn: record.turn.unwrap_or(0) as i32,
            turn_start: record.turn_start,
            turn_end: false,
            row_height: ROW_PX,
            index: record_index as i32,
            foldable: turn.is_some_and(|group| group.hidden > 0),
            collapsed: turn_folded,
        });
        if let Some((kind, text)) = summary {
            rows.push(TrajRow {
                kind,
                badge: "".into(),
                text: text.into(),
                arguments: "".into(),
                result: "".into(),
                error: false,
                request: 0,
                turn: record.turn.unwrap_or(0) as i32,
                turn_start: false,
                turn_end: kind == 7,
                row_height: ROW_SUMMARY_PX,
                index: record_index as i32,
                foldable: false,
                collapsed: true,
            });
        }
    }
    rows
}

/// 时间线投影：时长模式压缩空闲，序列模式等宽；span/边界按当前视口裁剪归一。
fn use_overview(state: &State, model: &SnapshotModel) -> bool {
    model.has_overview && (model.overview_exact || state.has_more || state.has_newer)
}

fn timeline_domain(state: &State, model: &SnapshotModel) -> (f64, f64) {
    let overview = use_overview(state, model);
    if state.show_duration {
        if overview {
            (model.overview_start, model.overview_end)
        } else {
            (model.tl_start, model.tl_end)
        }
    } else {
        let detail_end = model
            .spans
            .iter()
            .filter_map(|span| span.sequence_end)
            .fold(model.records.len().max(1) as f64, f64::max);
        (
            0.0,
            if overview {
                detail_end.max(model.overview_sequence_end)
            } else {
                detail_end
            },
        )
    }
}

fn visible_span(start: f64, end: f64, window_lo: f64, window_total: f64) -> Option<(f32, f32)> {
    if end < window_lo || start > window_lo + window_total {
        return None;
    }
    // Preserve the time geometry even for instantaneous events.  Slint owns
    // the minimum pixel width; a ratio-based minimum grows with the window
    // and makes short events look longer than their source timing.
    let left = ((start - window_lo) / window_total).clamp(0.0, 1.0);
    let right = ((end - window_lo) / window_total).clamp(0.0, 1.0);
    Some((left as f32, (right - left).max(0.0) as f32))
}

/// Return the records which belong to the selected, stable trajectory step.
///
/// Timeline coordinates are deliberately not part of this decision. Adjacent
/// requests often overlap in wall-clock time (and legacy rows may have a zero
/// duration), so interval intersection makes unrelated entries look selected.
/// The record's turn plus normalized group identity is the durable link shared
/// by an assistant entry, its thinking/body, and the tools issued for that
/// model step.
fn focused_record_indexes(model: &SnapshotModel, selected: i32) -> Option<HashSet<usize>> {
    let selected = usize::try_from(selected).ok()?;
    let selected_record = model.records.get(selected)?;
    let selected_key = State::identity_key(selected_record.turn, &selected_record.group);
    Some(
        model
            .records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| {
                (State::identity_key(record.turn, &record.group) == selected_key).then_some(index)
            })
            .collect(),
    )
}

fn project_timeline(app: &MainWindow, state: &State, model: &SnapshotModel, texts: &Texts) {
    let view_start = state.view_start.clamp(0.0, 1.0);
    let view_width = state.view_width.clamp(0.02, 1.0);
    let timed = state.show_duration;
    let use_overview = use_overview(state, model);
    let (total_lo, total_hi) = timeline_domain(state, model);
    let total = (total_hi - total_lo).max(1.0);
    let window_lo = total_lo + view_start * total;
    let window_total = view_width * total;
    let clamp_pos = |value: f64| ((value - window_lo) / window_total).clamp(0.0, 1.0) as f32;
    let focused_records = focused_record_indexes(model, state.selected);
    let needle = state.query.trim().to_lowercase();
    let overview_bucket_width = state
        .page
        .as_ref()
        .and_then(|page| page.get("tile_level"))
        .and_then(Value::as_u64)
        .map_or(128.0, |level| 128.0 * 8.0_f64.powf(level as f64));

    let mut spans: Vec<TrajSpan> = Vec::new();
    let visible_detail_ranges: Vec<(f64, f64, Option<f64>, Option<f64>)> = model
        .spans
        .iter()
        .map(|span| {
            (
                span.start,
                span.end.max(span.start + 0.001),
                span.sequence_start,
                span.sequence_end,
            )
        })
        .collect();
    let sources: Vec<(&SpanBase, bool)> = if use_overview {
        model
            .overview_spans
            .iter()
            .map(|span| (span, true))
            .chain(
                model
                    .spans
                    .iter()
                    .filter(|_| !model.overview_exact)
                    .map(|span| (span, false)),
            )
            .collect()
    } else {
        model.spans.iter().map(|span| (span, false)).collect()
    };
    for (base, aggregate) in sources {
        let (start, end) = if timed {
            (base.start, base.end)
        } else if let (Some(start), Some(end)) = (base.sequence_start, base.sequence_end) {
            (start, end)
        } else {
            (base.record as f64, base.record as f64 + 1.0)
        };
        // Aggregate overview tiles have no stable source record. Keep them at
        // their normal overview opacity while a concrete record is focused;
        // only spans tied to a real record participate in focus dimming.
        let outside = focused_records.as_ref().is_some_and(|focused| {
            base.record != usize::MAX && !focused.contains(&base.record)
        });
        if aggregate && !model.overview_exact {
            let overlaps = visible_detail_ranges.iter().any(
                |(detail_start, detail_end, sequence_start, sequence_end)| {
                    if timed {
                        *detail_start <= end && *detail_end >= start
                    } else {
                        sequence_start.zip(*sequence_end).is_some_and(|(lo, hi)| {
                            lo <= base.sequence_end.unwrap_or(start)
                                && hi >= base.sequence_start.unwrap_or(end)
                        })
                    }
                },
            );
            if overlaps {
                continue;
            }
        }
        let Some((start, width)) = visible_span(start, end, window_lo, window_total) else {
            continue;
        };
        let cell = model
            .records
            .get(base.record)
            .and_then(|record| model.cells.get(record.cell));
        let error = cell.map(|cell| cell.is_error).unwrap_or_else(|| {
            base.sequence_start.is_some_and(|sequence| {
                model
                    .overview_error_buckets
                    .contains(&((sequence / overview_bucket_width).floor() as i64))
            })
        });
        let dimmed = !needle.is_empty()
            && (cell.is_none() || !record_needle(model, base.record).contains(&needle));
        spans.push(TrajSpan {
            lane: base.lane as i32,
            start,
            width,
            kind: base.kind,
            index: if base.record == usize::MAX {
                -1
            } else {
                base.record as i32
            },
            ttft: base.ttft.unwrap_or(-1.0) as f32,
            selected: base.record != usize::MAX
                && state.selected >= 0
                && base.record == state.selected as usize,
            outside,
            overview: aggregate,
            aggregate: aggregate && !model.overview_exact,
            error,
            dimmed,
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
                label: format_offset(tick - total_lo).into(),
            });
            tick += step;
        }
    }

    let boundaries: Vec<TrajBoundary> = model
        .boundaries
        .iter()
        .map(|(turn, time, record_index)| TrajBoundary {
            pos: if timed {
                clamp_pos(*time)
            } else {
                let sequence = model
                    .spans
                    .iter()
                    .find(|span| span.record == *record_index)
                    .and_then(|span| span.sequence_start)
                    .unwrap_or(*record_index as f64);
                clamp_pos(sequence)
            },
            label: texts.turn_label(*turn).into(),
        })
        .collect();

    app.set_traj_spans(to_model(spans));
    app.set_traj_ticks(to_model(ticks));
    app.set_traj_boundaries(to_model(boundaries));
    app.set_traj_view_start(view_start as f32);
    app.set_traj_view_width(view_width as f32);
    if let Some((start, end)) = state.selection {
        app.set_traj_selection_active(true);
        app.set_traj_selection_start(start as f32);
        app.set_traj_selection_end(end as f32);
    } else {
        app.set_traj_selection_active(false);
    }
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
        TAB_OVERVIEW => {
            if zh {
                "概述"
            } else {
                "Overview"
            }
        }
        TAB_RAW => {
            if zh {
                "原始数据"
            } else {
                "Raw"
            }
        }
        TAB_PARAMS => {
            if zh {
                "参数"
            } else {
                "Params"
            }
        }
        TAB_RESULT => {
            if zh {
                "结果"
            } else {
                "Result"
            }
        }
        TAB_SCHEMA => "Schema",
        _ => {
            if zh {
                "计时"
            } else {
                "Timing"
            }
        }
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
    let Some(record) = model
        .records
        .get(selected.max(0) as usize)
        .filter(|_| selected >= 0)
    else {
        clear(app);
        return;
    };
    let cell = &model.cells[record.cell];
    let zh = texts.zh;
    let group_label = texts.group_label(&record.group);
    let header = texts.header(record.turn.unwrap_or(0), &record.group);

    let mut tabs: Vec<TrajTab> = vec![TrajTab {
        title: tab_title(texts, TAB_OVERVIEW).into(),
        kind: TAB_OVERVIEW,
    }];
    if cell.kind == KIND_TOOL || cell.kind == KIND_SUBTOOL {
        tabs.push(TrajTab {
            title: tab_title(texts, TAB_PARAMS).into(),
            kind: TAB_PARAMS,
        });
        tabs.push(TrajTab {
            title: tab_title(texts, TAB_RESULT).into(),
            kind: TAB_RESULT,
        });
        if cell.schema_detail.is_some() {
            tabs.push(TrajTab {
                title: tab_title(texts, TAB_SCHEMA).into(),
                kind: TAB_SCHEMA,
            });
        }
    }
    if cell.metrics.is_some() {
        tabs.push(TrajTab {
            title: tab_title(texts, TAB_TIMING).into(),
            kind: TAB_TIMING,
        });
    }
    tabs.push(TrajTab {
        title: tab_title(texts, TAB_RAW).into(),
        kind: TAB_RAW,
    });

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
                    value: metrics
                        .step_start
                        .map(format_started_at)
                        .unwrap_or_default()
                        .into(),
                });
                timing.push(TrajTiming {
                    label: label(zh, "总时长", "Total duration").into(),
                    value: texts.seconds(cell.time_seconds).into(),
                });
                if let (Some(step_start), Some(first_token)) =
                    (metrics.step_start, metrics.first_token)
                {
                    timing.push(TrajTiming {
                        label: label(zh, "首 token 延迟", "First token").into(),
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
                value: format!(
                    "{} · {}",
                    texts.turn_label(record.turn.unwrap_or(0)),
                    group_label
                )
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
                value: cell
                    .started_at
                    .map(format_started_at)
                    .unwrap_or_default()
                    .into(),
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
                    parts.push(format!(
                        "{} {}",
                        label(zh, "缓存读", "cache"),
                        thousands(read)
                    ));
                }
                if let Some(write) = usage.cache_write {
                    parts.push(format!(
                        "{} {}",
                        label(zh, "缓存写", "cache+"),
                        thousands(write)
                    ));
                }
                if let Some(output) = usage.output {
                    parts.push(format!(
                        "{} {}",
                        label(zh, "输出", "out"),
                        thousands(output)
                    ));
                }
                if let Some(think) = usage.think {
                    parts.push(format!(
                        "{} {}",
                        label(zh, "思考", "think"),
                        thousands(think)
                    ));
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
            if let Some(thinking) = &cell.thinking_detail {
                sections.push(TrajSection {
                    title: label(zh, "思考", "Thinking").into(),
                    body: bounded_body(thinking).into(),
                });
            }
            if let Some(output) = &cell.output_detail {
                if matches!(
                    cell.kind,
                    KIND_MESSAGE | KIND_TOOL | KIND_SUBTOOL | KIND_COMPACTED
                ) {
                    sections.push(TrajSection {
                        title: label(zh, "输出", "Output").into(),
                        body: bounded_body(output).into(),
                    });
                }
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
    let Some(record) = model
        .records
        .get(record_index.max(0) as usize)
        .filter(|_| record_index >= 0)
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
            lines.push(format!(
                "{} {}",
                label(zh, "输出", "out"),
                thousands(output)
            ));
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
    project_history_hint(app, state);
    app.set_traj_loading(false);
    app.set_traj_failed(false);
    project_rows(app, state, snapshot, &texts);
    project_timeline(app, state, snapshot, &texts);
    project_details(app, state, snapshot, &texts);
    project_search(app, state);
}

fn project_history_hint(app: &MainWindow, state: &State) {
    let i18n = app.global::<I18n>();
    app.set_traj_history_hint(if state.loading_page {
        i18n.get_traj_history_loading()
    } else if state.page_failed {
        i18n.get_traj_history_retry()
    } else if state.has_more {
        i18n.get_traj_history_older()
    } else {
        i18n.get_traj_history_start()
    });
}

fn apply_detail(cell: &mut Cell, detail: &Value) {
    if !detail.is_object() {
        return;
    }
    let mut payload = detail.clone();
    if let Some(nested) = detail.get("payload").and_then(Value::as_object) {
        if let Some(map) = payload.as_object_mut() {
            map.extend(nested.clone());
        }
    }
    let item = json!({"item_id": cell.record_id, "status": "completed"});
    let hydrated = build_cell(&item, &payload, cell.kind, cell.index);
    cell.input_detail = hydrated.input_detail;
    cell.output_detail = hydrated.output_detail.or(cell.output_detail.take());
    cell.thinking_detail = hydrated.thinking_detail.or(cell.thinking_detail.take());
    cell.schema_detail = hydrated.schema_detail;
    cell.result = hydrated.result.or(cell.result.take());
    cell.usage = hydrated.usage.or(cell.usage);
    cell.raw = payload;
}

fn restore_detail(state: &mut State) {
    if let Some(original) = state.detail_original.take() {
        if let Some(cell) = state.model.as_mut().and_then(|model| {
            model
                .cells
                .iter_mut()
                .find(|cell| cell.record_id == original.record_id)
        }) {
            *cell = original;
        }
    }
}

fn replace_model(state: &mut State, model: SnapshotModel) {
    let selected_id = state.model.as_ref().and_then(|previous| {
        previous
            .records
            .get(state.selected as usize)
            .map(|record| previous.cells[record.cell].record_id.as_str())
    });
    state.selected = match selected_id {
        Some(id) => model
            .records
            .iter()
            .position(|record| model.cells[record.cell].record_id == id)
            .map(|index| index as i32)
            .unwrap_or(-1),
        None if state.model.is_none() && !model.records.is_empty() => 0,
        None => -1,
    };
    state.detail_generation = state.detail_generation.wrapping_add(1);
    state.detail_original = None;
    state.model = Some(model);
}

fn hydrate_selected(app: &MainWindow, current: &Rc<RefCell<State>>, backend: &Arc<NativeDesktop>) {
    let request = {
        let mut state = current.borrow_mut();
        state.detail_generation = state.detail_generation.wrapping_add(1);
        if state.detail_loading || !app.get_traj_open() {
            return;
        }
        let Some(cell) = state.model.as_ref().and_then(|model| {
            model
                .records
                .get(state.selected as usize)
                .map(|record| &model.cells[record.cell])
        }) else {
            return;
        };
        let Some((body, revision)) = cell.detail_ref.as_ref() else {
            return;
        };
        let request = (
            state.session.clone(),
            state.cloud,
            cell.clone(),
            body.clone(),
            *revision,
            state.detail_generation,
        );
        state.detail_loading = true;
        request
    };
    let (session, cloud, mut selected_cell, body, revision, generation) = request;
    let item = selected_cell.record_id.clone();
    let next_backend = backend.clone();
    let backend = backend.clone();
    let (tx, rx) = sync_channel(1);
    std::thread::spawn(move || {
        let result = (if cloud {
            backend.cloud_thread_trajectory_detail(&session, &body, revision)
        } else {
            backend.thread_trajectory_detail(&session, &body, revision)
        })
        .map(|detail| {
            // Parse and format the selected payload off the UI thread too.
            apply_detail(&mut selected_cell, &detail);
            selected_cell.detail_ref = None;
            selected_cell
        });
        let _ = tx.send(result);
    });
    let weak = app.as_weak();
    let current = current.clone();
    poll(move || {
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                current.borrow_mut().detail_loading = false;
                return true;
            }
        };
        let Some(app) = weak.upgrade() else {
            return true;
        };
        let mut state = current.borrow_mut();
        state.detail_loading = false;
        if state.detail_generation != generation || !app.get_traj_open() {
            drop(state);
            hydrate_selected(&app, &current, &next_backend);
            return true;
        }
        let selected = state.selected;
        let Some(model) = state.model.as_mut() else {
            return true;
        };
        let Some(record) = model.records.get(selected as usize) else {
            return true;
        };
        let cell = &mut model.cells[record.cell];
        if cell.record_id != item {
            return true;
        }
        match result {
            Ok(hydrated) => {
                let original = std::mem::replace(cell, hydrated);
                state.detail_original = Some(original);
                let texts = Texts::new(app.global::<I18n>().get_zh());
                project_details(&app, &state, state.model.as_ref().unwrap(), &texts);
            }
            Err(error) => app.set_status(format!("无法读取轨迹详情：{error}").into()),
        }
        true
    });
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let state = Rc::new(RefCell::new(State {
        show_duration: app.get_traj_show_duration(),
        view_width: 1.0,
        selected: -1,
        ..State::default()
    }));
    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    let (weak_edge, current_edge, backend_edge) = (app.as_weak(), state.clone(), api.clone());
    app.on_traj_edge(move |older| {
        if !older {
            let mut state = current_edge.borrow_mut();
            state.older_edge_latched = false;
            if state.loading_page || !state.has_newer || state.newer_edge_latched {
                return;
            }
            state.newer_edge_latched = true;
            state.loading_page = true;
            state.page_failed = false;
            if let Some(app) = weak_edge.upgrade() {
                project_history_hint(&app, &state);
            }
            let session = state.session.clone();
            let cloud = state.cloud;
            let generation = state.generation;
            let after = state.next_after;
            drop(state);
            let (tx, rx) = sync_channel(1);
            let backend = backend_edge.clone();
            std::thread::spawn(move || {
                let result = match after {
                    Some(cursor) => {
                        if cloud {
                            backend.cloud_thread_trajectory_page(&session, None, Some(cursor), 100)
                        } else {
                            backend.thread_trajectory_page_after(&session, Some(cursor), 100)
                        }
                    }
                    None => Err(anyhow::anyhow!("missing newer trajectory cursor")),
                };
                let _ = tx.send((session, result));
            });
            let (weak_edge, current_edge, detail_backend) = (
                weak_edge.clone(),
                current_edge.clone(),
                backend_edge.clone(),
            );
            poll(move || {
                let Ok((session, result)) = rx.try_recv() else {
                    return false;
                };
                let Some(app) = weak_edge.upgrade() else {
                    return true;
                };
                let mut state = current_edge.borrow_mut();
                if state.session != session || state.cloud != cloud || state.generation != generation {
                    return true;
                }
                match result {
                    Ok(page) => {
                        state.page_failed = false;
                        let merged = state
                            .page
                            .as_ref()
                            .map(|current| merge_page_json(current, &page, false))
                            .unwrap_or(page);
                        state.next_before = merged.get("next_before").and_then(Value::as_i64);
                        state.next_after = merged.get("next_after").and_then(Value::as_i64);
                        state.has_more = merged
                            .get("has_more")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        state.has_newer = merged
                            .get("has_newer")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        state.page = Some(merged);
                        if let Some(page) = &state.page {
                            let model = parse_view_page(page);
                            replace_model(&mut state, model);
                            prune_folds(&mut state);
                            apply_model(&app, &state);
                        }
                    }
                    Err(error) => {
                        state.page_failed = true;
                        app.set_status(format!("无法读取更新轨迹：{error}").into());
                        state.newer_edge_latched = false;
                    }
                }
                state.newer_edge_latched = false;
                state.loading_page = false;
                project_history_hint(&app, &state);
                drop(state);
                hydrate_selected(&app, &current_edge, &detail_backend);
                true
            });
            return;
        }
        let (session, cloud, generation, before) = {
            let mut state = current_edge.borrow_mut();
            if state.loading_page || !state.has_more || state.older_edge_latched {
                return;
            }
            state.older_edge_latched = true;
            state.loading_page = true;
            state.page_failed = false;
            if let Some(app) = weak_edge.upgrade() {
                project_history_hint(&app, &state);
            }
            (state.session.clone(), state.cloud, state.generation, state.next_before)
        };
        let (tx, rx) = sync_channel(1);
        let backend = backend_edge.clone();
        std::thread::spawn(move || {
            let result = match before {
                Some(cursor) => {
                    if cloud {
                        backend.cloud_thread_trajectory_page(&session, Some(cursor), None, 100)
                    } else {
                        backend.thread_trajectory_page(&session, Some(cursor), 100)
                    }
                }
                None => Err(anyhow::anyhow!("missing trajectory cursor")),
            };
            let _ = tx.send((session, result));
        });
        let (weak_edge, current_edge, detail_backend) = (
            weak_edge.clone(),
            current_edge.clone(),
            backend_edge.clone(),
        );
        poll(move || {
            let Ok((session, result)) = rx.try_recv() else {
                return false;
            };
            let Some(app) = weak_edge.upgrade() else {
                return true;
            };
            let mut state = current_edge.borrow_mut();
            if state.session != session || state.cloud != cloud || state.generation != generation {
                return true;
            }
            match result {
                Ok(page) => {
                    state.page_failed = false;
                    let merged = state
                        .page
                        .as_ref()
                        .map(|current| merge_page_json(current, &page, true))
                        .unwrap_or(page);
                    state.next_before = merged.get("next_before").and_then(Value::as_i64);
                    state.next_after = merged.get("next_after").and_then(Value::as_i64);
                    state.has_more = merged
                        .get("has_more")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    state.has_newer = merged
                        .get("has_newer")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    state.page = Some(merged);
                    if let Some(page) = &state.page {
                        let model = parse_view_page(page);
                        replace_model(&mut state, model);
                        prune_folds(&mut state);
                        apply_model(&app, &state);
                    }
                }
                Err(error) => {
                    state.page_failed = true;
                    app.set_status(format!("无法读取更早轨迹：{error}").into());
                    state.older_edge_latched = false;
                }
            }
            state.older_edge_latched = false;
            state.loading_page = false;
            project_history_hint(&app, &state);
            drop(state);
            hydrate_selected(&app, &current_edge, &detail_backend);
            true
        });
    });

    let open = move |index: i32, cloud_id: String| {
        let Some(app) = weak.upgrade() else { return };
        // index < 0 means "the active thread" (the settings-page entry point).
        let cloud = !cloud_id.is_empty();
        let session = if cloud {
            cloud_id
        } else if index < 0 {
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
        current.borrow_mut().cloud = cloud;
        let generation = current.borrow().generation;
        app.set_traj_open(true);
        app.set_traj_loading(true);
        app.set_traj_failed(false);
        app.set_traj_rows(to_model(vec![]));
        app.set_traj_spans(to_model(vec![]));
        app.set_traj_ticks(to_model(vec![]));
        app.set_traj_boundaries(to_model(vec![]));
        let (tx, rx) = sync_channel(1);
        let detail_backend = backend.clone();
        let backend = backend.clone();
        std::thread::spawn(move || {
            // Read the shared bounded presentation projection.  It returns a
            // small detail window plus the complete overview tile pyramid;
            // unlike thread_snapshot this never materializes the raw history.
            let parsed = (if cloud {
                backend.cloud_thread_trajectory_page(&session, None, None, 100)
            } else {
                backend.thread_trajectory_page(&session, None, 100)
            })
            .map(|page| (page.clone(), parse_view_page(&page)));
            let _ = tx.send((session, parsed));
        });
        let (weak, current) = (weak.clone(), current.clone());
        poll(move || {
            let Ok((session, result)) = rx.try_recv() else {
                return false;
            };
            let Some(app) = weak.upgrade() else {
                return true;
            };
            if current.borrow().session != session || current.borrow().cloud != cloud || current.borrow().generation != generation {
                return true;
            }
            match result {
                Ok((page, snapshot)) => {
                    let mut state = current.borrow_mut();
                    state.next_before = page.get("next_before").and_then(Value::as_i64);
                    state.next_after = page.get("next_after").and_then(Value::as_i64);
                    state.has_more = page
                        .get("has_more")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    state.has_newer = page
                        .get("has_newer")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    state.page = Some(page);
                    replace_model(&mut state, snapshot);
                    prune_folds(&mut state);
                    apply_model(&app, &state);
                    drop(state);
                    hydrate_selected(&app, &current, &detail_backend);
                }
                Err(error) => {
                    app.set_traj_loading(false);
                    app.set_traj_failed(true);
                    app.set_status(format!("无法读取线程轨迹：{error}").into());
                }
            }
            true
        });
    };
    let open_local = open.clone();
    app.on_open_trajectory(move |index| open_local(index, String::new()));
    app.on_cloud_open_trajectory(move |id| open(-1, id.to_string()));

    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    app.on_traj_select_row(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let mut s = current.borrow_mut();
        restore_detail(&mut s);
        let mut kind = None;
        if let Some(model) = &s.model {
            if let Some(record) = model
                .records
                .get(index.max(0) as usize)
                .filter(|_| index >= 0)
            {
                kind = Some(model.cells[record.cell].kind);
            }
        }
        s.selected = index;
        s.tab = if matches!(kind, Some(KIND_TOOL | KIND_SUBTOOL)) {
            TAB_PARAMS
        } else {
            TAB_OVERVIEW
        };
        drop(s);
        // Keep row instances alive so the second click can become a double click.
        let s = current.borrow();
        if let Some(model) = &s.model {
            let texts = Texts::new(app.global::<I18n>().get_zh());
            project_details(&app, &s, model, &texts);
            project_timeline(&app, &s, model, &texts);
        }
        drop(s);
        hydrate_selected(&app, &current, &backend);
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_fold_row(move |index, action| {
        let Some(app) = weak.upgrade() else { return };
        let mut s = current.borrow_mut();
        let Some(model) = s.model.as_ref() else {
            return;
        };
        let Some(record) = model
            .records
            .get(index.max(0) as usize)
            .filter(|_| index >= 0)
        else {
            return;
        };
        let groups = fold_groups(model);
        let turn = record.turn;
        let key = assistant_key(model, index as usize);
        let turn_eligible = turn.is_some_and(|turn| {
            groups
                .turns
                .get(&turn)
                .is_some_and(|group| group.hidden > 0)
        });
        let turn_action = action == 1
            || (action == 0
                && (turn.is_some_and(|turn| s.collapsed_turns.contains(&turn))
                    || (record.turn_start && !groups.calls.contains_key(&key))));
        if turn_action && turn_eligible {
            let turn = turn.unwrap();
            if !s.collapsed_turns.remove(&turn) {
                s.collapsed_turns.insert(turn);
            }
        } else if groups.calls.contains_key(&key) {
            if !s.collapsed_assistants.remove(&key) {
                s.collapsed_assistants.insert(key);
            }
        }
        drop(s);
        apply_model(&app, &current.borrow());
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_select_span(move |index| {
        if let Some(app) = weak.upgrade() {
            let mut s = current.borrow_mut();
            if let Some(model) = &s.model {
                if let Some(record) = model
                    .records
                    .get(index.max(0) as usize)
                    .filter(|_| index >= 0)
                {
                    let turn = record.turn;
                    let owner = fold_groups(model).owners.get(&(index as usize)).cloned();
                    if let Some(turn) = turn {
                        s.collapsed_turns.remove(&turn);
                    }
                    if let Some(owner) = owner {
                        s.collapsed_assistants.remove(&owner);
                    }
                }
            }
            drop(s);
            app.invoke_traj_select_row(index);
            apply_model(&app, &current.borrow());
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
        let mut s = current.borrow_mut();
        let groups = s.model.as_ref().map(fold_groups).unwrap_or_default();
        if all_turns_folded(&groups, &s) {
            s.collapsed_turns.clear();
        } else {
            s.collapsed_turns = groups
                .turns
                .into_iter()
                .filter(|(_, turn)| turn.hidden > 0)
                .map(|(turn, _)| turn)
                .collect();
        }
        drop(s);
        apply_model(&app, &current.borrow());
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_toggle_calls(move || {
        let Some(app) = weak.upgrade() else { return };
        let mut s = current.borrow_mut();
        let groups = s.model.as_ref().map(fold_groups).unwrap_or_default();
        if all_calls_folded(&groups, &s) {
            s.collapsed_assistants.clear();
        } else {
            s.collapsed_assistants = groups.calls.into_keys().collect();
        }
        drop(s);
        apply_model(&app, &current.borrow());
    });

    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    app.on_traj_search_changed(move || {
        let Some(app) = weak.upgrade() else { return };
        let query = app.get_traj_query().to_string();
        let (session, cloud, generation) = {
            let mut state = current.borrow_mut();
            state.query = query.clone();
            state.search_generation = state.search_generation.wrapping_add(1);
            state.search_hits.clear();
            state.search_text.clear();
            state.selected_search = None;
            (state.session.clone(), state.cloud, state.search_generation)
        };
        app.set_traj_search_hint("".into());
        apply_model(&app, &current.borrow());
        if query.trim().is_empty() {
            return;
        }
        let (tx, rx) = sync_channel(1);
        let backend = backend.clone();
        let needle = query.trim().chars().take(128).collect::<String>();
        let weak_done = app.as_weak();
        let current_done = current.clone();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(250),
            move || {
                let backend = backend.clone();
                let session = session.clone();
                let needle = needle.clone();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let result = if cloud {
                        backend.cloud_thread_search(&session, &needle)
                    } else {
                        backend.thread_trajectory_search(&session, &needle)
                    };
                    let _ = tx.send((session, generation, result));
                });
            },
        );
        current.borrow_mut().search_timer = Some(timer);
        let receiver_current = current.clone();
        poll(move || {
            let Ok((session, generation, result)) = rx.try_recv() else {
                return false;
            };
            receiver_current.borrow_mut().search_timer = None;
            let Some(app) = weak_done.upgrade() else {
                return true;
            };
            let state = current_done.borrow();
            if state.session != session || state.cloud != cloud || state.search_generation != generation {
                return true;
            }
            match result {
                Ok(value) => {
                    let hits = value
                        .get("hits")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(|hit| SearchHit {
                            item_id: hit
                                .get("item_id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            body_id: hit
                                .pointer("/body/body_id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            revision: hit
                                .pointer("/body/revision")
                                .and_then(Value::as_i64)
                                .unwrap_or(0),
                            cursor: hit.get("cursor").and_then(Value::as_i64).unwrap_or(0),
                            snippet: hit
                                .get("snippet")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                        .collect::<Vec<_>>();
                    let more = value
                        .get("has_more")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    drop(state);
                    let mut state = current_done.borrow_mut();
                    if state.session != session || state.cloud != cloud || state.search_generation != generation {
                        return true;
                    }
                    state.search_hits = hits;
                    project_search(&app, &state);
                    app.set_traj_search_hint(
                        if app.global::<I18n>().get_zh() {
                            format!(
                                "搜索命中 {} 条{}",
                                state.search_hits.len(),
                                if more {
                                    "（缩小关键词可查看更多）"
                                } else {
                                    ""
                                }
                            )
                        } else {
                            format!(
                                "{} search matches{}",
                                state.search_hits.len(),
                                if more {
                                    " (narrow the query for more)"
                                } else {
                                    ""
                                }
                            )
                        }
                        .into(),
                    );
                }
                Err(error) => {
                    drop(state);
                    app.set_traj_search_hint(
                        if app.global::<I18n>().get_zh() {
                            format!("搜索失败：{error}")
                        } else {
                            format!("Search failed: {error}")
                        }
                        .into(),
                    );
                }
            }
            true
        });
    });

    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    app.on_traj_search_hit(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let (session, cloud, generation, hit) = {
            let mut state = current.borrow_mut();
            if index < 0 || index as usize >= state.search_hits.len() {
                return;
            }
            let hit = state.search_hits[index as usize].clone();
            state.selected_search = Some(index as usize);
            if let Some(snapshot) = &state.model {
                if let Some(record) = snapshot
                    .records
                    .iter()
                    .position(|record| snapshot.cells[record.cell].record_id == hit.item_id)
                {
                    state.selected = record as i32;
                }
            }
            (
                state.session.clone(),
                state.cloud,
                state.search_generation,
                hit,
            )
        };
        let (tx, rx) = sync_channel(1);
        let backend = backend.clone();
        std::thread::spawn(move || {
            let result = if cloud {
                backend.cloud_thread_body(&session, &hit.body_id, hit.revision, hit.cursor)
            } else {
                backend.thread_trajectory_body(&session, &hit.body_id, hit.revision, hit.cursor)
            };
            let _ = tx.send((session, generation, result));
        });
        let weak_done = app.as_weak();
        let current_done = current.clone();
        poll(move || {
            let Ok((session, generation, result)) = rx.try_recv() else {
                return false;
            };
            let Some(app) = weak_done.upgrade() else {
                return true;
            };
            if current_done.borrow().session != session || current_done.borrow().cloud != cloud || current_done.borrow().search_generation != generation { return true; }
            let Ok(page) = result else {
                app.set_traj_search_hint(
                    if app.global::<I18n>().get_zh() {
                        "读取命中正文失败"
                    } else {
                        "Failed to read matching text"
                    }
                    .into(),
                );
                return true;
            };
            let mut state = current_done.borrow_mut();
            if state.session != session || state.cloud != cloud || state.search_generation != generation {
                return true;
            }
            state.search_text = page
                .get("chunks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<String>();
            project_search(&app, &state);
            true
        });
    });

    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    app.on_traj_export(move || {
        let Some(app) = weak.upgrade() else { return };
        let session = current.borrow().session.clone();
        let cloud = current.borrow().cloud;
        if session.is_empty() {
            return;
        }
        let default_name = trajectory_export_filename_now(&session);
        let Some(path) = crate::file_dialog::save_file("导出线程日志", &default_name) else {
            return;
        };
        let (tx, rx) = sync_channel(1);
        let backend = backend.clone();
        std::thread::spawn(move || {
            let result = if cloud {
                backend.cloud_export_thread_log(&session, std::path::Path::new(&path))
            } else {
                backend.export_thread_log(&session, std::path::Path::new(&path))
            };
            let _ = tx.send((path, result));
        });
        let weak_done = app.as_weak();
        poll(move || {
            let Ok((path, result)) = rx.try_recv() else {
                return false;
            };
            if let Some(app) = weak_done.upgrade() {
                match result {
                    Ok(()) => app.set_status(format!("线程日志已导出：{path}").into()),
                    Err(error) => app.set_status(format!("导出线程日志失败：{error}").into()),
                }
            }
            true
        });
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_zoom(move |factor, anchor| {
        let Some(app) = weak.upgrade() else { return };
        {
            let mut s = current.borrow_mut();
            let anchor =
                (s.view_start + (anchor as f64).clamp(0.0, 1.0) * s.view_width).clamp(0.0, 1.0);
            let fraction = (anchor - s.view_start) / s.view_width.max(0.02);
            s.view_width = (s.view_width * factor as f64).clamp(0.02, 1.0);
            s.view_start = (anchor - fraction * s.view_width).clamp(0.0, 1.0 - s.view_width);
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

    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    app.on_traj_select_range(move |start, end| {
        let Some(app) = weak.upgrade() else { return };
        let mut state = current.borrow_mut();
        restore_detail(&mut state);
        let start = (start as f64).clamp(0.0, 1.0);
        let end = (end as f64).clamp(0.0, 1.0);
        state.selection = Some((start.min(end), start.max(end)));
        if let Some(snapshot) = &state.model {
            let bounds = |span: &SpanBase| {
                if state.show_duration {
                    (span.start, span.end)
                } else {
                    (
                        span.sequence_start.unwrap_or(span.record as f64),
                        span.sequence_end.unwrap_or(span.record as f64 + 1.0),
                    )
                }
            };
            let (domain_start, domain_end) = timeline_domain(&state, snapshot);
            let lo = domain_start + state.selection.unwrap().0 * (domain_end - domain_start);
            let hi = domain_start + state.selection.unwrap().1 * (domain_end - domain_start);
            let selected_record = snapshot
                .spans
                .iter()
                .min_by(|a, b| {
                    let (a_start, a_end) = bounds(a);
                    let (b_start, b_end) = bounds(b);
                    let da = if a_end < lo {
                        lo - a_end
                    } else if a_start > hi {
                        a_start - hi
                    } else {
                        0.0
                    };
                    let db = if b_end < lo {
                        lo - b_end
                    } else if b_start > hi {
                        b_start - hi
                    } else {
                        0.0
                    };
                    da.total_cmp(&db)
                })
                .map(|span| span.record)
                .filter(|record| *record < snapshot.records.len());
            if let Some(record) = selected_record {
                state.selected = record as i32;
            }
        }
        drop(state);
        apply_model(&app, &current.borrow());
        hydrate_selected(&app, &current, &backend);
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_clear_range(move || {
        let Some(app) = weak.upgrade() else { return };
        current.borrow_mut().selection = None;
        apply_model(&app, &current.borrow());
    });

    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    app.on_traj_select_position(move |position| {
        let Some(app) = weak.upgrade() else { return };
        let mut state = current.borrow_mut();
        restore_detail(&mut state);
        let Some(snapshot) = &state.model else { return };
        let position = position as f64;
        let timed = state.show_duration;
        let (lo, hi) = timeline_domain(&state, snapshot);
        let absolute = lo + position.clamp(0.0, 1.0) * (hi - lo);
        let selected_record = snapshot
            .spans
            .iter()
            .min_by(|a, b| {
                let at = if timed {
                    (a.start + a.end) / 2.0
                } else {
                    a.sequence_start.unwrap_or(a.record as f64)
                };
                let bt = if timed {
                    (b.start + b.end) / 2.0
                } else {
                    b.sequence_start.unwrap_or(b.record as f64)
                };
                (at - absolute).abs().total_cmp(&(bt - absolute).abs())
            })
            .map(|span| span.record)
            .filter(|record| *record < snapshot.records.len());
        if let Some(record) = selected_record {
            state.selected = record as i32;
        }
        state.selection = None;
        drop(state);
        apply_model(&app, &current.borrow());
        hydrate_selected(&app, &current, &backend);
    });

    let (weak, current) = (app.as_weak(), state.clone());
    app.on_traj_hover_position(move |position| {
        let Some(app) = weak.upgrade() else { return };
        let state = current.borrow();
        let Some(snapshot) = &state.model else { return };
        let timed = state.show_duration;
        let (lo, hi) = timeline_domain(&state, snapshot);
        let absolute = lo + (position as f64).clamp(0.0, 1.0) * (hi - lo);
        let span = snapshot.spans.iter().min_by(|a, b| {
            let value = |item: &SpanBase| {
                if timed {
                    (item.start + item.end) / 2.0
                } else {
                    item.sequence_start.unwrap_or(item.record as f64)
                }
            };
            (value(a) - absolute)
                .abs()
                .total_cmp(&(value(b) - absolute).abs())
        });
        let Some(span) = span else { return };
        app.set_traj_hover(span.record as i32);
        let texts = Texts::new(app.global::<I18n>().get_zh());
        app.set_traj_tooltip(tooltip_for(snapshot, &texts, span.record as i32).into());
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
    fn export_filename_matches_web_shape_and_sanitizes_windows_names() {
        assert_eq!(
            trajectory_export_filename("sess_demo", 1_792_000_000_123),
            "sess_demo-1792000000123.jsonl"
        );
        assert_eq!(
            trajectory_export_filename(r#"  sess:demo/part  "#, 42),
            "sess_demo_part-42.jsonl"
        );
        assert_eq!(trajectory_export_filename("   ", 7), "thread-7.jsonl");
    }

    #[test]
    fn complete_timing_stays_stable_across_ledger_pages_and_selection() {
        let rows = json!([
            {"item_id":"fixture-user","turn_id":"fixture-turn","user_round":1,"created_seq":1,
                "kind":"user_message","started_at":1800000000.0,"completed_at":1800003600.0},
            {"item_id":"fixture-first","turn_id":"fixture-turn","user_round":1,"created_seq":2,
                "kind":"assistant_message","started_at":1800000000.0,"completed_at":1800000002.0},
            {"item_id":"fixture-late","turn_id":"fixture-turn","user_round":1,"created_seq":3,
                "kind":"assistant_message","started_at":1800003600.0,"completed_at":1800003603.0}
        ]);
        let timing = rows.as_array().unwrap().iter().map(|row| json!({
            "created_seq":row["created_seq"],"kind":row["kind"],"started_at":row["started_at"],"completed_at":row["completed_at"]
        })).collect::<Vec<_>>();
        let first = json!({"trajectory":[rows[2]],"timeline":timing,"timeline_has_more":false,"has_more":true,
            "tiles":[{"bucket":0,"count":3,"start":1800000000.0,"end":1800003603.0}]});
        let model = parse_view_page(&first);
        assert!(model.overview_exact);
        assert_eq!(model.overview_end - model.overview_start, 5000.0);
        assert_eq!(model.overview_spans[0].start, model.overview_spans[0].end);
        assert_eq!(model.spans[0].start, model.overview_start + 2000.0);
        let final_page = json!({"trajectory":[rows[0],rows[1]],"timeline":timing,"timeline_has_more":false,"has_more":false});
        let complete = parse_view_page(&merge_page_json(&first, &final_page, true));
        assert_eq!(complete.overview_end - complete.overview_start, 5000.0);
        assert_eq!(complete.overview_spans.len(), 3);
        for (initial, loaded) in model.overview_spans.iter().zip(&complete.overview_spans) {
            assert_eq!((initial.start, initial.end), (loaded.start, loaded.end));
        }
        let state = State {
            show_duration: true,
            selected: 0,
            ..Default::default()
        };
        assert_eq!(
            timeline_domain(&state, &model),
            timeline_domain(&state, &complete)
        );
        assert!(use_overview(&state, &complete));
        let mut incomplete = first.clone();
        incomplete["timeline_has_more"] = json!(true);
        assert!(!parse_view_page(&incomplete).overview_exact);
        assert_eq!(visible_span(0.0, 0.0, 0.0, 5000.0), Some((0.0, 0.0)));
        let (start, width) = visible_span(5000.0, 5000.0, 0.0, 5000.0).unwrap();
        assert_eq!((start, width), (1.0, 0.0));
        assert_eq!(
            visible_span(2500.0, 2500.5, 0.0, 5000.0),
            Some((0.5, 0.0001))
        );
        assert!(visible_span(6000.0, 6000.0, 0.0, 5000.0).is_none());
    }

    #[test]
    fn selected_detail_restores_compaction_summary_and_assistant_reasoning() {
        let mut compacted = build_cell(
            &json!({"item_id":"fixture-compaction"}),
            &json!({"content":"history"}),
            KIND_COMPACTED,
            0,
        );
        assert!(compacted.output_detail.is_none());
        apply_detail(
            &mut compacted,
            &json!({"payload":{"content":"history","summary_text":"Fixture retained context.",
            "summary_model_output":"Fixture model summary."}}),
        );
        assert_eq!(
            compacted.output_detail.as_deref(),
            Some("Fixture retained context.")
        );
        assert!(compacted.input_detail.is_none());
        let mut assistant = build_cell(
            &json!({"item_id":"fixture-model","created_time":1800000000.0,"updated_time":1800000002.0}),
            &json!({"content":"Fixture response","message_stats":{"interaction_duration_s":400}}),
            KIND_MESSAGE,
            1,
        );
        assert_eq!(assistant.time_seconds, Some(2.0));
        assert!(assistant.input_detail.is_none());
        apply_detail(
            &mut assistant,
            &json!({"content":"Fixture complete response","reasoning_content":"Fixture reasoning"}),
        );
        assert_eq!(
            assistant.output_detail.as_deref(),
            Some("Fixture complete response")
        );
        assert_eq!(
            assistant.thinking_detail.as_deref(),
            Some("Fixture reasoning")
        );
        assert!(assistant.input_detail.is_none());
        let mut tool = build_cell(
            &json!({"item_id":"fixture-tool"}),
            &json!({"tool":"fixture_tool"}),
            KIND_TOOL,
            2,
        );
        apply_detail(
            &mut tool,
            &json!({"tool":"fixture_tool","args":{"count":2},
            "schema":{"type":"object"},"result":"Fixture result"}),
        );
        assert!(tool.input_detail.as_deref().unwrap().contains("count"));
        assert!(tool.schema_detail.as_deref().unwrap().contains("object"));
        assert_eq!(tool.result.as_deref(), Some("Fixture result"));
    }

    #[test]
    fn paging_preserves_selected_identity_and_releases_hydrated_body() {
        let first = json!({"trajectory":[
            {"item_id":"fixture-first","turn_id":"fixture-turn","created_seq":10,
                "kind":"assistant_message","summary":"Fixture preview","started_at":100.0,
                "detail":{"body_id":"fixture-first|detail","revision":1}}
        ]});
        let older = json!({"trajectory":[
            {"item_id":"fixture-older","turn_id":"fixture-turn","created_seq":9,
                "kind":"user_message","summary":"Fixture input","started_at":99.0}
        ]});
        let mut state = State::default();
        replace_model(&mut state, parse_view_page(&first));
        let original = state.model.as_ref().unwrap().cells[0].clone();
        assert!(original.detail_ref.is_some());
        state.detail_original = Some(original);
        apply_detail(
            &mut state.model.as_mut().unwrap().cells[0],
            &json!({"content":"Fixture complete response"}),
        );
        restore_detail(&mut state);
        assert_eq!(
            state.model.as_ref().unwrap().cells[0]
                .output_detail
                .as_deref(),
            Some("Fixture preview")
        );
        let generation = state.detail_generation;
        replace_model(
            &mut state,
            parse_view_page(&merge_page_json(&first, &older, true)),
        );
        assert_eq!(state.selected, 1);
        assert!(state.detail_generation > generation);
        assert!(state.detail_original.is_none());
        replace_model(&mut state, parse_view_page(&older));
        assert_eq!(state.selected, -1);
        state.show_duration = true;
        state.view_width = 0.02;
        state.detail_loading = true;
        let generation = state.detail_generation;
        state.reset("fixture-next-thread".into());
        assert_eq!(state.view_width, 1.0);
        assert_eq!(state.view_start, 0.0);
        assert!(state.show_duration && state.detail_loading);
        assert!(state.detail_generation > generation);
        assert_eq!(state.selected, -1);
    }

    #[test]
    fn fold_groups_keep_assistants_separate_and_ignore_orphan_tools() {
        let model = parse_view_page(&json!({ "trajectory": [
            {"item_id":"s", "user_round":1, "root_turn_id":"r", "turn_id":"r", "created_seq":1, "kind":"system_message", "summary":"instructions"},
            {"item_id":"u", "user_round":1, "root_turn_id":"r", "turn_id":"r", "created_seq":2, "kind":"user_message", "summary":"question"},
            {"item_id":"a1", "user_round":1, "root_turn_id":"r", "turn_id":"r", "created_seq":3, "kind":"assistant_message", "model_round":1, "summary":"working"},
            {"item_id":"t1", "user_round":1, "root_turn_id":"r", "turn_id":"r", "created_seq":4, "kind":"tool_call", "model_round":1, "tool_name":"read_file"},
            {"item_id":"a2", "user_round":1, "root_turn_id":"r", "turn_id":"r", "created_seq":5, "kind":"assistant_message", "model_round":1, "summary":"again"},
            {"item_id":"t2", "user_round":1, "root_turn_id":"r", "turn_id":"r", "created_seq":6, "kind":"tool_call", "model_round":1, "tool_name":"write_file"},
            {"item_id":"c", "user_round":1, "root_turn_id":"r", "turn_id":"r", "created_seq":7, "kind":"context", "summary":"context"},
            {"item_id":"orphan", "user_round":1, "root_turn_id":"r", "turn_id":"r", "created_seq":8, "kind":"tool_call", "model_round":1, "tool_name":"read_file"},
            {"item_id":"single", "user_round":2, "root_turn_id":"r2", "turn_id":"r2", "created_seq":9, "kind":"user_message", "summary":"question"}
        ]}));
        let groups = fold_groups(&model);
        assert_eq!(groups.calls.len(), 2);
        assert_eq!(groups.calls["a1"].count, 1);
        assert_eq!(groups.calls["a2"].count, 1);
        assert_eq!(groups.owners.len(), 2);
        assert_eq!(groups.turns[&1].hidden, 6);
        assert_eq!(groups.turns[&2].hidden, 0);
        let mut state = State::default();
        assert!(!all_turns_folded(&groups, &state));
        state.collapsed_turns.insert(1);
        assert!(all_turns_folded(&groups, &state));
        state.collapsed_assistants.insert("a1".into());
        assert!(!all_calls_folded(&groups, &state));
        state.collapsed_assistants.insert("a2".into());
        assert!(all_calls_folded(&groups, &state));
        state.collapsed_assistants.remove("a1");
        assert!(!all_calls_folded(&groups, &state));
        let texts = Texts::new(true);
        let rows = build_rows(&state, &model, &texts);
        assert_eq!(rows.iter().filter(|row| row.kind == 7).count(), 1);
        assert_eq!(rows.iter().filter(|row| row.kind == KIND_USER).count(), 2);
        assert!(rows.iter().all(|row| row.index >= 0));
        state.collapsed_turns.clear();
        state.collapsed_assistants.insert("a1".into());
        let rows = build_rows(&state, &model, &texts);
        assert_eq!(rows.iter().filter(|row| row.kind == 8).count(), 2);
        assert_eq!(
            rows.iter().filter(|row| row.kind == KIND_MESSAGE).count(),
            2
        );
        assert_eq!(rows.iter().filter(|row| row.kind == KIND_TOOL).count(), 1);
        assert_eq!(
            rows.iter().filter(|row| row.kind == KIND_CONTEXT).count(),
            1
        );
        state.collapsed_turns.insert(1);
        state.query = "context".into();
        let rows = build_rows(&state, &model, &texts);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, KIND_CONTEXT);
    }

    #[test]
    fn focus_uses_turn_and_model_step_instead_of_overlapping_time() {
        // Two model steps deliberately overlap in wall-clock time. Timeline
        // selection must retain only the selected step and its tool, rather
        // than every interval that intersects the assistant's duration.
        let model = parse_view_page(&json!({ "trajectory": [
            {"item_id":"fixture-user","turn_id":"fixture-turn","root_turn_id":"fixture-turn",
             "user_round":1,"created_seq":1,"kind":"user_message","summary":"input",
             "started_at":100.0,"completed_at":100.0},
            {"item_id":"fixture-a1","turn_id":"fixture-turn","root_turn_id":"fixture-turn",
             "user_round":1,"created_seq":2,"kind":"assistant_message","model_round":1,"summary":"first",
             "started_at":101.0,"completed_at":120.0},
            {"item_id":"fixture-t1","turn_id":"fixture-turn","root_turn_id":"fixture-turn",
             "user_round":1,"created_seq":3,"kind":"tool_call","model_round":1,"tool_name":"inspect",
             "started_at":105.0,"completed_at":110.0},
            {"item_id":"fixture-a2","turn_id":"fixture-turn","root_turn_id":"fixture-turn",
             "user_round":1,"created_seq":4,"kind":"assistant_message","model_round":2,"summary":"second",
             "started_at":108.0,"completed_at":125.0},
            {"item_id":"fixture-t2","turn_id":"fixture-turn","root_turn_id":"fixture-turn",
             "user_round":1,"created_seq":5,"kind":"tool_call","model_round":2,"tool_name":"write",
             "started_at":110.0,"completed_at":121.0}
        ]}));
        let selected = model
            .records
            .iter()
            .position(|record| model.cells[record.cell].record_id == "fixture-a1")
            .expect("first assistant record");
        let focused = focused_record_indexes(&model, selected as i32).expect("focus set");
        let record_id = |index: usize| model.cells[model.records[index].cell].record_id.as_str();
        assert!(focused
            .iter()
            .any(|&index| record_id(index) == "fixture-a1"));
        assert!(focused
            .iter()
            .any(|&index| record_id(index) == "fixture-t1"));
        assert!(!focused
            .iter()
            .any(|&index| record_id(index) == "fixture-a2"));
        assert!(!focused
            .iter()
            .any(|&index| record_id(index) == "fixture-t2"));
        let texts = Texts::new(true);
        assert_eq!(texts.kind(KIND_SYSTEM), "系统");
        assert_eq!(texts.kind(KIND_USER), "用户");
        assert_eq!(texts.kind(KIND_CONTEXT), "上下文");
        assert_eq!(texts.kind(KIND_COMPACTED), "已压缩");
        assert_eq!(texts.kind(KIND_MESSAGE), "助手");
        assert_eq!(texts.kind(KIND_TOOL), "工具");
        assert_eq!(texts.kind(KIND_SUBTOOL), "子智能体");
    }

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
        // turn 1 owns its user row, the answer and the merged tool; turn 2
        // holds only its own user row
        let turn_of = |turn: i64| {
            model
                .records
                .iter()
                .filter(|record| record.turn == Some(turn))
                .count()
        };
        assert_eq!(turn_of(1), 3); // messages + step:1 + step:2
        assert_eq!(turn_of(2), 1);
        assert_eq!(model.requests.len(), 1);
        assert_eq!(model.requests[0].number, 1);
        // the assistant cell carries the round's own token usage
        assert_eq!(model.cells[1].usage.unwrap().output, Some(340.0));
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
        assert_eq!(
            markdown_preview_text("# 标题\n- 甲 **粗** 乙"),
            "标题\n甲 粗 乙"
        );
        assert_eq!(
            single_line_preview("  第一行\r\n第二行\t含有  多余空格  "),
            "第一行 第二行 含有 多余空格"
        );
    }

    #[test]
    fn shared_view_page_keeps_context_rows_and_global_tile_spans() {
        let page = json!({
            "trajectory": [
                {"item_id":"ctx-1","turn_id":"turn-1","root_turn_id":"turn-1",
                 "user_round":1,"item_index":0,"created_seq":1,"revision":1,
                 "kind":"context","status":"completed","summary":"代理结果已注入",
                 "model_round":1,"started_at":1000.0,"completed_at":1001.0},
                {"item_id":"assistant-1","turn_id":"turn-1","root_turn_id":"turn-1",
                 "user_round":1,"item_index":1,"created_seq":2,"revision":1,
                "kind":"assistant_message","status":"completed","summary":"已完成",
                 "stats":{"usage":{"inputTokens":"1234","outputTokens":56,"reasoningTokens":"7"}},
                 "model_round":1,"started_at":1001.0,"completed_at":1002.0}
            ],
            "tiles": [{"bucket":0,"count":4096,"failed":0,"running":0,
                       "start":1000.0,"end":1100.0,"input_count":1024,
                       "model_count":2048,"tool_count":1024}],
            "tile_level":0
        });
        let model = parse_view_page(&page);
        assert!(model
            .records
            .iter()
            .any(|record| model.cells[record.cell].kind == KIND_CONTEXT));
        assert!(model
            .overview_spans
            .iter()
            .any(|span| span.record == usize::MAX && span.lane == 1));
        let assistant = model
            .cells
            .iter()
            .find(|cell| cell.kind == KIND_MESSAGE)
            .expect("assistant row");
        let usage = assistant
            .usage
            .expect("bounded page usage must be projected");
        assert_eq!(usage.input, Some(1234.0));
        assert_eq!(usage.output, Some(56.0));
        assert_eq!(usage.think, Some(7.0));
        assert_eq!(model.tl_start, 1_000_000.0);
        assert_eq!(model.tl_end, 1_002_000.0);
        assert_eq!(model.overview_start, 1_000_000.0);
        assert_eq!(model.overview_end, 1_100_000.0);
    }

    #[test]
    fn view_page_restores_global_sequence_and_segments_aggregate_tiles() {
        let page = json!({
            "tile_level": 0,
            "trajectory": [{
                "item_id": "detail-900", "root_turn_id": "root-1", "turn_id": "turn-1",
                "user_round": 1, "item_index": 0, "created_seq": 900,
                "kind": "assistant_message", "status": "completed", "summary": "bounded detail",
                "model_round": 1, "started_at": 10.0, "completed_at": 11.0
            }],
            "tiles": [{
                "bucket": 0, "count": 64, "failed": 0, "running": 0,
                "start": 1.0, "end": 2.0,
                "input_count": 12, "model_count": 40, "tool_count": 12
            }]
        });
        let model = parse_view_page(&page);
        assert_eq!(model.spans.len(), 1);
        assert_eq!(model.spans[0].sequence_start, Some(900.0));
        assert_eq!(model.spans[0].sequence_end, Some(901.0));
        assert_eq!(model.overview_spans.len(), 16);
        assert!(model
            .overview_spans
            .windows(2)
            .all(|pair| pair[0].end <= pair[1].start));
        assert!(model.has_overview);
        assert_eq!(model.overview_sequence_end, 128.0);
    }

    #[test]
    fn aggregate_and_detail_spans_share_idle_compression_mapping() {
        let mut detail = vec![
            SpanBase {
                start: 0.0,
                end: 1.0,
                record: 1,
                kind: KIND_MESSAGE,
                lane: 1,
                ttft: None,
                sequence_start: Some(1.0),
                sequence_end: Some(2.0),
            },
            SpanBase {
                start: 10.0,
                end: 11.0,
                record: 2,
                kind: KIND_TOOL,
                lane: 2,
                ttft: None,
                sequence_start: Some(2.0),
                sequence_end: Some(3.0),
            },
        ];
        let mut overview = vec![
            SpanBase {
                start: 0.0,
                end: 5.0,
                record: usize::MAX,
                kind: KIND_MESSAGE,
                lane: 1,
                ttft: None,
                sequence_start: None,
                sequence_end: None,
            },
            SpanBase {
                start: 10.0,
                end: 20.0,
                record: usize::MAX,
                kind: KIND_TOOL,
                lane: 2,
                ttft: None,
                sequence_start: None,
                sequence_end: None,
            },
        ];
        compress_idle_span_sets(&mut detail, &mut overview);
        // The five-unit idle interval is removed in both projections.  The
        // detail row and aggregate tile therefore remain clickable at the
        // same normalized x coordinate.
        assert_eq!((detail[1].start, detail[1].end), (5.0, 6.0));
        assert_eq!((overview[1].start, overview[1].end), (5.0, 15.0));
        assert_eq!(detail[0].end - detail[0].start, 1.0);
        assert_eq!(overview[0].end - overview[0].start, 5.0);
    }

    #[test]
    fn tile_fallback_compresses_global_idle_and_keeps_detail_aligned_across_pages() {
        let earlier = json!({
            "item_id":"fixture-earlier", "turn_id":"fixture-first", "user_round":1,
            "created_seq":1, "kind":"assistant_message", "summary":"Earlier detail",
            "started_at":1800000000.0, "completed_at":1800000002.0
        });
        let later = json!({
            "item_id":"fixture-later", "turn_id":"fixture-second", "user_round":2,
            "created_seq":129, "kind":"assistant_message", "summary":"Later detail",
            "started_at":1800003600.0, "completed_at":1800003602.0
        });
        let tiles = json!([
            {"bucket":0, "count":16, "start":1800000000.0, "end":1800000016.0,
             "input_count":0, "model_count":16, "tool_count":0},
            {"bucket":1, "count":16, "start":1800003600.0, "end":1800003616.0,
             "input_count":0, "model_count":16, "tool_count":0}
        ]);
        let first_page = json!({
            "trajectory":[later], "tiles":tiles, "tile_level":0, "timeline_has_more":true,
            "has_more":true, "next_before":129
        });
        let initial = parse_view_page(&first_page);
        assert!(initial.has_overview && !initial.overview_exact);
        assert_eq!(initial.overview_end - initial.overview_start, 32000.0);
        assert_eq!(initial.spans[0].start, initial.overview_start + 16000.0);
        assert_eq!(initial.spans[0].end, initial.overview_start + 18000.0);
        assert_eq!(initial.boundaries[0].1, initial.spans[0].start);

        let older_page = json!({"trajectory":[earlier], "tiles":tiles, "tile_level":0,
            "timeline_has_more":true, "has_more":false});
        let loaded = parse_view_page(&merge_page_json(&first_page, &older_page, true));
        assert_eq!(
            (loaded.overview_start, loaded.overview_end),
            (initial.overview_start, initial.overview_end)
        );
        let later_span = loaded
            .spans
            .iter()
            .find(|span| span.sequence_start == Some(129.0))
            .unwrap();
        assert_eq!(
            (later_span.start, later_span.end),
            (initial.spans[0].start, initial.spans[0].end)
        );
        let later_boundary = loaded
            .boundaries
            .iter()
            .find(|boundary| boundary.0 == 2)
            .unwrap();
        assert_eq!(later_boundary.1, later_span.start);
        // Three thousand seconds of inactivity are removed globally, while
        // exact row durations and aggregate durations keep their geometry.
        let later_aggregate = loaded
            .overview_spans
            .iter()
            .find(|span| {
                span.sequence_start
                    .is_some_and(|sequence| sequence >= 128.0)
            })
            .unwrap();
        assert!(later_aggregate.start >= later_span.end);
        assert!(later_aggregate.start <= later_span.end + 1.0);
        let total = loaded.overview_end - loaded.overview_start;
        let projected = visible_span(
            later_span.start,
            later_span.end,
            loaded.overview_start,
            total,
        )
        .unwrap();
        assert_eq!(projected, (0.5, 0.0625));
    }

    #[test]
    fn extracts_camel_case_and_nested_string_token_usage() {
        let usage = extract_usage(&json!({
            "data": {"messageStats": {"roundUsage": {
                "inputTokens": "1200",
                "cacheReadTokens": 300,
                "cacheWriteTokens": "40",
                "outputTokens": 80,
                "reasoningTokens": "12"
            }}}
        }))
        .expect("nested camelCase usage must surface");
        assert_eq!(usage.input, Some(1200.0));
        assert_eq!(usage.cache_read, Some(300.0));
        assert_eq!(usage.cache_write, Some(40.0));
        assert_eq!(usage.output, Some(80.0));
        assert_eq!(usage.think, Some(12.0));
    }

    #[test]
    fn aligns_with_web_trajectory_rules() {
        // 与 web 端 trajectoryModel 同一条规则集：会话写入器覆盖后的 payload
        // 读 reasoning_content 与 meta.message_stats 内嵌用量；队列瞬态、重复
        // 落账的工具结果、命令子会话、hook 通知与成功轮次终止不进时间线。
        let snapshot = json!({
            "turns": [
                {"turn_id": "t1", "user_turn_index": 1, "status": "finished", "summary": "s", "payload": {}, "updated_time": 100.0, "root_turn_id": "t1", "trigger_kind": "user"}
            ],
            "items": [
                {"item_id": "i1", "item_index": 0, "kind": "assistant_message", "status": "completed", "revision": 0,
                 "payload": {"content": "回复", "reasoning_content": "思考中", "model_round": 1,
                             "meta": {"message_stats": {"usage": {"input_tokens": 5565, "output_tokens": 491}}}},
                 "created_time": 10.0, "updated_time": 10.0,
                 "turn_id": "t1", "visibility": "user", "root_turn_id": "t1", "created_seq": 1},
                {"item_id": "n1", "item_index": 1, "kind": "queue", "status": "cancelled", "revision": 0,
                 "payload": {"reason": "user_withdrew"}, "created_time": 10.5, "updated_time": 10.5,
                 "turn_id": "t1", "visibility": "user", "root_turn_id": "t1", "created_seq": 2},
                {"item_id": "n2", "item_index": 2, "kind": "tool_message", "status": "completed", "revision": 0,
                 "payload": {"role": "tool", "content": "重复结果"}, "created_time": 10.6, "updated_time": 10.6,
                 "turn_id": "t1", "visibility": "user", "root_turn_id": "t1", "created_seq": 3},
                {"item_id": "n3", "item_index": 3, "kind": "command_session_start", "status": "completed", "revision": 0,
                 "payload": {"stage": "guard"}, "created_time": 10.7, "updated_time": 10.7,
                 "turn_id": "t1", "visibility": "user", "root_turn_id": "t1", "created_seq": 4},
                {"item_id": "n4", "item_index": 4, "kind": "hook/invoked", "status": "completed", "revision": 0,
                 "payload": {"summary": "hook"}, "created_time": 10.8, "updated_time": 10.8,
                 "turn_id": "t1", "visibility": "user", "root_turn_id": "t1", "created_seq": 5},
                {"item_id": "n5", "item_index": 5, "kind": "turn_terminal", "status": "completed", "revision": 0,
                 "payload": {"status": "completed"}, "created_time": 10.9, "updated_time": 10.9,
                 "turn_id": "t1", "visibility": "user", "root_turn_id": "t1", "created_seq": 6}
            ],
            "blocks": [],
            "item_total": 6
        });
        let model = parse_snapshot(&snapshot);
        // 六条入账里只有助手条目留下，其余全是被跳过的噪音。
        assert_eq!(model.cells.len(), 1);
        assert_eq!(model.records.len(), 1);
        let message = &model.cells[0];
        assert_eq!(message.kind, KIND_MESSAGE);
        assert_eq!(message.thinking_detail.as_deref(), Some("思考中"));
        let usage = message
            .usage
            .expect("nested message_stats.usage must surface");
        assert_eq!(usage.input, Some(5565.0));
        assert_eq!(usage.output, Some(491.0));
    }

    #[test]
    fn merges_bounded_pages_in_both_directions() {
        let current = json!({
            "trajectory": (600..1200).map(|seq| json!({"created_seq": seq})).collect::<Vec<_>>(),
            "has_more": true,
            "has_newer": false
        });
        let older = json!({
            "trajectory": (400..500).map(|seq| json!({"created_seq": seq})).collect::<Vec<_>>(),
            "has_more": true
        });
        let merged = merge_page_json(&current, &older, true);
        let rows = merged["trajectory"].as_array().unwrap();
        assert_eq!(rows.len(), MAX_VIEW_ROWS);
        assert_eq!(rows.first().unwrap()["created_seq"], 400);
        assert_eq!(rows.last().unwrap()["created_seq"], 1099);
        assert_eq!(merged["has_newer"], true);

        let newer = json!({
            "trajectory": (1000..1100).map(|seq| json!({"created_seq": seq})).collect::<Vec<_>>(),
            "has_newer": false
        });
        let forward = merge_page_json(&merged, &newer, false);
        let rows = forward["trajectory"].as_array().unwrap();
        assert_eq!(rows.len(), MAX_VIEW_ROWS);
        assert_eq!(rows.first().unwrap()["created_seq"], 600);
        assert_eq!(rows.last().unwrap()["created_seq"], 1099);
    }
}
