use super::*;

pub(super) struct ToolResultPayload {
    pub(super) ok: bool,
    pub(super) data: Value,
    pub(super) error: String,
    pub(super) sandbox: bool,
    #[allow(dead_code)]
    pub(super) timestamp: DateTime<Utc>,
    pub(super) meta: Option<Value>,
}

impl ToolResultPayload {
    pub(super) fn from_value(value: Value) -> Self {
        let timestamp = Utc::now();
        if let Value::Object(map) = &value {
            if map.get("ok").and_then(Value::as_bool).is_some() && map.contains_key("data") {
                let ok = map.get("ok").and_then(Value::as_bool).unwrap_or(true);
                let mut data = map.get("data").cloned().unwrap_or_else(|| json!({}));
                if let Some(error_meta) = map.get("error_meta").cloned() {
                    if let Some(obj) = data.as_object_mut() {
                        obj.entry("error_meta".to_string()).or_insert(error_meta);
                    }
                }
                let error = map
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let sandbox = map.get("sandbox").and_then(Value::as_bool).unwrap_or(false);
                let meta = map.get("meta").cloned().filter(|value| !value.is_null());
                return Self {
                    ok,
                    data,
                    error,
                    sandbox,
                    timestamp,
                    meta,
                };
            }
        }

        let data = if value.is_object() {
            value
        } else {
            json!({ "result": value })
        };
        Self {
            ok: true,
            data,
            error: String::new(),
            sandbox: false,
            timestamp,
            meta: None,
        }
    }

    pub(super) fn error(message: String, data: Value) -> Self {
        Self {
            ok: false,
            data: if data.is_object() {
                data
            } else {
                json!({ "detail": data })
            },
            error: message,
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        }
    }

    pub(super) fn insert_meta(&mut self, key: &str, value: Value) {
        let mut meta = merge_tool_result_meta(self.meta.take());
        meta.insert(key.to_string(), value);
        self.meta = Some(Value::Object(meta));
    }

    fn to_observation_payload(&self, tool_name: &str) -> Value {
        let mut payload = json!({
            "tool": tool_name,
            "ok": self.ok,
            "data": self.data,
        });
        if !self.error.trim().is_empty() {
            if let Value::Object(ref mut map) = payload {
                map.insert("error".to_string(), Value::String(self.error.clone()));
            }
        }
        if self.sandbox {
            if let Value::Object(ref mut map) = payload {
                map.insert("sandbox".to_string(), Value::Bool(true));
            }
        }
        if let Some(meta) = &self.meta {
            if let Value::Object(ref mut map) = payload {
                if let Some(duration_ms) = meta.get("duration_ms").and_then(Value::as_i64) {
                    if duration_ms > 0 {
                        map.insert("duration_ms".to_string(), json!(duration_ms));
                    }
                }
                if let Some(code) = meta.get("error_code").and_then(Value::as_str) {
                    let cleaned = code.trim();
                    if !cleaned.is_empty() {
                        map.insert("error_code".to_string(), Value::String(cleaned.to_string()));
                    }
                }
                if let Some(retryable) = meta.get("error_retryable").and_then(Value::as_bool) {
                    map.insert("retryable".to_string(), Value::Bool(retryable));
                }
                if let Some(preflight) = compact_public_preflight(meta) {
                    map.insert("preflight".to_string(), preflight);
                }
            }
        }
        payload
    }

    pub(super) fn to_compact_payload(&self, tool_name: &str) -> Value {
        let mut payload = self.to_observation_payload(tool_name);
        compact_observation_payload(&mut payload, tool_name);
        strip_compact_payload_noise(&mut payload, 0);
        payload
    }

    pub(super) fn to_event_payload(&self, tool_name: &str) -> Value {
        let mut payload = json!({
            "tool": tool_name,
            "ok": self.ok,
            "data": self.data,
        });
        if !self.error.trim().is_empty() {
            if let Value::Object(ref mut map) = payload {
                map.insert("error".to_string(), Value::String(self.error.clone()));
            }
        }
        if self.sandbox {
            if let Value::Object(ref mut map) = payload {
                map.insert("sandbox".to_string(), Value::Bool(true));
            }
        }
        if let Some(meta) = &self.meta {
            if let Value::Object(ref mut map) = payload {
                map.insert("meta".to_string(), meta.clone());
                if let Some(code) = meta.get("error_code").and_then(Value::as_str) {
                    let cleaned = code.trim();
                    if !cleaned.is_empty() {
                        map.insert("error_code".to_string(), Value::String(cleaned.to_string()));
                    }
                }
                if let Some(retryable) = meta.get("error_retryable").and_then(Value::as_bool) {
                    map.insert("retryable".to_string(), Value::Bool(retryable));
                }
                if let Some(preflight) = compact_public_preflight(meta) {
                    map.insert("preflight".to_string(), preflight);
                }
            }
        }
        payload
    }
}

pub(super) fn merge_tool_result_meta(meta: Option<Value>) -> Map<String, Value> {
    match meta {
        Some(Value::Object(map)) => map,
        Some(Value::Null) | None => Map::new(),
        Some(other) => {
            let mut map = Map::new();
            map.insert("value".to_string(), other);
            map
        }
    }
}

fn compact_public_preflight(meta: &Value) -> Option<Value> {
    let preflight = meta
        .as_object()
        .and_then(|meta| meta.get("preflight"))
        .and_then(Value::as_object)?;
    let mut compacted = Map::new();
    if let Some(status) = preflight.get("status").and_then(Value::as_str) {
        let cleaned = status.trim();
        if !cleaned.is_empty() {
            compacted.insert("status".to_string(), Value::String(cleaned.to_string()));
        }
    }
    if let Some(summary) = preflight.get("summary").and_then(Value::as_str) {
        let cleaned = summary.trim();
        if !cleaned.is_empty() {
            compacted.insert("summary".to_string(), Value::String(cleaned.to_string()));
        }
    }
    if let Some(changes) = preflight.get("changes").and_then(Value::as_array) {
        let changes = changes
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| Value::String(value.to_string()))
            .collect::<Vec<_>>();
        if !changes.is_empty() {
            compacted.insert("changes".to_string(), Value::Array(changes));
        }
    }
    if compacted.is_empty() {
        return None;
    }
    Some(Value::Object(compacted))
}

fn compact_failure_observation_payload(map: &mut Map<String, Value>) -> bool {
    let ok = map.get("ok").and_then(Value::as_bool).unwrap_or(true);
    if ok {
        return false;
    }
    let raw_data = map.get("data").cloned().unwrap_or(Value::Null);
    let error = build_compact_failure_error_message(
        map.get("error").and_then(Value::as_str).unwrap_or(""),
        &raw_data,
    );
    map.remove("data");
    map.remove("preflight");
    map.remove("duration_ms");
    map.remove("sandbox");
    map.remove("truncated");
    map.remove("observation_output_chars");
    map.remove("truncation_reasons");
    map.remove("continuation_required");
    map.remove("continuation_hint");
    if !error.is_empty() {
        map.insert("error".to_string(), Value::String(error));
    }
    true
}

fn build_compact_failure_error_message(error: &str, data: &Value) -> String {
    let mut fragments = Vec::new();
    push_compact_error_fragment(&mut fragments, error);
    if let Some(detail) = extract_compact_failure_detail(data) {
        push_compact_error_fragment(&mut fragments, &detail);
    }
    fragments.join(" ")
}

fn push_compact_error_fragment(fragments: &mut Vec<String>, candidate: &str) {
    let cleaned = candidate
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return;
    }
    let normalized = normalize_error_fragment(&cleaned);
    if fragments
        .iter()
        .any(|existing| normalize_error_fragment(existing) == normalized)
    {
        return;
    }
    fragments.push(cleaned);
}

fn normalize_error_fragment(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn extract_compact_failure_detail(data: &Value) -> Option<String> {
    let map = data.as_object()?;
    if let Some(preflight) = map.get("preflight").and_then(Value::as_object) {
        if let Some(message) = extract_preflight_diagnostic_message(preflight) {
            return Some(message);
        }
    }
    if let Some(detail) = extract_execute_command_failure_detail(map) {
        return Some(detail);
    }
    if let Some(message) = map
        .get("detail")
        .or_else(|| map.get("message"))
        .or_else(|| map.get("content"))
        .and_then(Value::as_str)
    {
        return Some(message.trim().to_string());
    }
    let meta = map.get("error_meta").and_then(Value::as_object)?;
    meta.get("hint")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extract_preflight_diagnostic_message(preflight: &Map<String, Value>) -> Option<String> {
    if let Some(items) = preflight.get("diagnostics").and_then(Value::as_array) {
        for item in items {
            if let Some(message) = extract_diagnostic_message(item) {
                return Some(message);
            }
        }
    }
    let diagnostics_jsonl = preflight.get("diagnostics_jsonl").and_then(Value::as_str)?;
    for line in diagnostics_jsonl.lines() {
        if let Ok(parsed) = serde_json::from_str::<Value>(line) {
            if let Some(message) = extract_diagnostic_message(&parsed) {
                return Some(message);
            }
        }
    }
    None
}

fn extract_diagnostic_message(value: &Value) -> Option<String> {
    let item = value.as_object()?;
    item.get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            item.get("hint")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|hint| !hint.is_empty())
                .map(ToString::to_string)
        })
}

fn extract_execute_command_failure_detail(map: &Map<String, Value>) -> Option<String> {
    let output = map
        .get("stderr")
        .or_else(|| map.get("stdout"))
        .and_then(Value::as_str)?;
    let lines = output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    let focus_index = lines
        .iter()
        .rposition(|line| {
            let trimmed = line.trim_start();
            trimmed.contains("Error:")
                || trimmed.contains("Exception:")
                || trimmed.contains("SyntaxError")
                || trimmed.contains("IndentationError")
                || trimmed.contains("NameError")
                || trimmed.contains("TypeError")
                || trimmed.contains("ValueError")
                || trimmed.contains("Traceback")
        })
        .unwrap_or(lines.len().saturating_sub(1));
    let start = focus_index.saturating_sub(2);
    let end = (focus_index + 1).min(lines.len().saturating_sub(1));
    let detail = lines[start..=end]
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");
    let detail = if detail.chars().count() > 320 {
        let head = detail.chars().take(320).collect::<String>();
        format!("{head}...")
    } else {
        detail
    };
    Some(format!("stderr: {detail}"))
}

const OBSERVATION_MAX_CHARS: usize = 20_000;
const OBSERVATION_HEAD_CHARS: usize = 10_000;
const OBSERVATION_TAIL_CHARS: usize = 10_000;
const OBSERVATION_MAX_ARRAY_ITEMS: usize = 32;
const OBSERVATION_ARRAY_HEAD_ITEMS: usize = 20;
const OBSERVATION_ARRAY_TAIL_ITEMS: usize = 8;
const OBSERVATION_TABLE_SAMPLE_ROWS: usize = 4;
const OBSERVATION_SEARCH_HIT_LIMIT: usize = 10;
const OBSERVATION_SEARCH_CONTENT_HEAD_CHARS: usize = 180;
const OBSERVATION_READ_FILE_LIMIT: usize = 8;
const OBSERVATION_JSONL_ITEM_MAX_DEPTH: usize = 8;
const READ_OUTPUT_TRUNCATION_PREFIX: &str = "...(truncated read output, omitted ";
const READ_OUTPUT_TRUNCATION_SUFFIX: &str = " bytes)...";
pub(super) const TRUNCATION_CONTINUATION_HINT: &str =
    "result_truncated_continue_with_pagination_or_narrower_query";
const CONTINUATION_SIGNAL_KEYS: [&str; 10] = [
    "query_handle",
    "next_cursor",
    "cursor",
    "next_page_token",
    "page_token",
    "continuation_token",
    "next_token",
    "next_url",
    "next_offset",
    "resume_token",
];
const CONTINUATION_NESTED_KEYS: [&str; 7] = [
    "meta",
    "data",
    "result",
    "output",
    "payload",
    "structured_content",
    "pagination",
];

fn value_has_continuation_signal(value: &Value, depth: usize) -> bool {
    if depth > 4 {
        return false;
    }
    match value {
        Value::Object(map) => {
            if map.get("continuation_required").and_then(Value::as_bool) == Some(true)
                || map.get("has_more").and_then(Value::as_bool) == Some(true)
            {
                return true;
            }
            if CONTINUATION_SIGNAL_KEYS
                .iter()
                .any(|key| map.get(*key).is_some_and(is_non_empty_continuation_value))
            {
                return true;
            }
            CONTINUATION_NESTED_KEYS.iter().any(|key| {
                map.get(*key)
                    .is_some_and(|nested| value_has_continuation_signal(nested, depth + 1))
            })
        }
        Value::Array(items) => items
            .iter()
            .take(6)
            .any(|item| value_has_continuation_signal(item, depth + 1)),
        _ => false,
    }
}

fn is_non_empty_continuation_value(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.trim().is_empty(),
        Value::Number(_) => true,
        Value::Bool(flag) => *flag,
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        Value::Null => false,
    }
}

fn map_has_continuation_signal(map: &Map<String, Value>) -> bool {
    if map.get("continuation_required").and_then(Value::as_bool) == Some(true)
        || map.get("has_more").and_then(Value::as_bool) == Some(true)
    {
        return true;
    }
    CONTINUATION_SIGNAL_KEYS
        .iter()
        .any(|key| map.get(*key).is_some_and(is_non_empty_continuation_value))
}

fn looks_like_read_file_payload(data: &Value) -> bool {
    let Some(meta) = data.get("meta").and_then(Value::as_object) else {
        return false;
    };
    data.get("content").and_then(Value::as_str).is_some()
        && meta.get("files").and_then(Value::as_array).is_some()
        && meta.get("read").and_then(Value::as_object).is_some()
}

pub(super) fn supports_tool_result_continuation(data: &Value, meta: Option<&Value>) -> bool {
    value_has_continuation_signal(data, 0)
        || meta.is_some_and(|value| value_has_continuation_signal(value, 0))
        || looks_like_read_file_payload(data)
}

fn should_skip_tool_truncation(tool_name: &str) -> bool {
    let canonical = crate::services::tools::resolve_tool_name(tool_name);
    matches!(canonical.as_str(), "技能调用" | "skill_call" | "skill_get")
}

pub(super) fn should_skip_event_payload_truncation(tool_name: &str) -> bool {
    let canonical = crate::services::tools::resolve_tool_name(tool_name);
    should_skip_tool_truncation(canonical.as_str())
        || matches!(canonical.as_str(), "apply_patch" | "应用补丁")
}

fn compact_observation_payload(payload: &mut Value, tool_name: &str) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    let canonical = crate::services::tools::resolve_tool_name(tool_name);
    let keep_path_in_data = matches!(
        canonical.as_str(),
        "写入文件" | "write_file" | "文本编辑" | "edit_file2"
    );
    if matches!(canonical.as_str(), "apply_patch" | "应用补丁") {
        let maybe_compacted = map
            .get("data")
            .and_then(Value::as_object)
            .and_then(compact_apply_patch_observation_data);
        if let Some(mut compacted_data) = maybe_compacted {
            let observation_truncated = truncate_observation_data(
                &mut compacted_data,
                OBSERVATION_HEAD_CHARS,
                OBSERVATION_TAIL_CHARS,
                TOOL_RESULT_TRUNCATION_MARKER,
            );
            let observation_output_chars = estimate_tool_result_chars(&compacted_data);
            map.insert("data".to_string(), compacted_data);
            map.remove("meta");
            if observation_truncated {
                map.insert("truncated".to_string(), Value::Bool(true));
                map.insert(
                    "observation_output_chars".to_string(),
                    json!(observation_output_chars),
                );
            }
            return;
        }
    }
    if should_skip_tool_truncation(tool_name) {
        return;
    }
    if compact_failure_observation_payload(map) {
        return;
    }
    let Some(raw_data) = map.get("data").cloned() else {
        return;
    };
    let continuation_supported = supports_tool_result_continuation(&raw_data, None);
    let mut compacted_data = extract_observation_data(&raw_data);
    compact_tabular_observation_data(&mut compacted_data);
    compact_dense_arrays_to_jsonl(&mut compacted_data);
    if let Some(compacted_map) = compacted_data.as_object_mut() {
        fit_read_file_content_to_observation_budget(compacted_map);
    }
    let mut observation_truncated = truncate_observation_data(
        &mut compacted_data,
        OBSERVATION_HEAD_CHARS,
        OBSERVATION_TAIL_CHARS,
        TOOL_RESULT_TRUNCATION_MARKER,
    );
    let mut truncation_reasons = if observation_truncated {
        collect_truncation_reasons_from_value(&compacted_data, TOOL_RESULT_TRUNCATION_MARKER)
    } else {
        Vec::new()
    };
    let chars_before_compact = estimate_tool_result_chars(&compacted_data);
    if chars_before_compact > OBSERVATION_MAX_CHARS {
        append_truncation_reason(&mut truncation_reasons, "char_budget");
        compacted_data = compact_large_tool_result_data(
            &compacted_data,
            chars_before_compact,
            OBSERVATION_HEAD_CHARS,
            OBSERVATION_TAIL_CHARS,
            TOOL_RESULT_TRUNCATION_MARKER,
            continuation_supported,
            &truncation_reasons,
        );
        observation_truncated = true;
    }
    let observation_output_chars = estimate_tool_result_chars(&compacted_data);
    let data_continuation_required = compacted_data
        .get("continuation_required")
        .and_then(Value::as_bool)
        == Some(true);
    let data_continuation_hint = compacted_data
        .get("continuation_hint")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    map.insert("data".to_string(), compacted_data);
    if observation_truncated {
        map.insert("truncated".to_string(), Value::Bool(true));
        map.insert(
            "observation_output_chars".to_string(),
            json!(observation_output_chars),
        );
        if !truncation_reasons.is_empty() {
            map.insert(
                "truncation_reasons".to_string(),
                Value::Array(
                    truncation_reasons
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect(),
                ),
            );
        }
        if continuation_supported {
            map.insert("continuation_required".to_string(), Value::Bool(true));
            map.insert(
                "continuation_hint".to_string(),
                Value::String(TRUNCATION_CONTINUATION_HINT.to_string()),
            );
        }
    }
    if data_continuation_required {
        map.insert("continuation_required".to_string(), Value::Bool(true));
        map.insert(
            "continuation_hint".to_string(),
            Value::String(
                data_continuation_hint.unwrap_or_else(|| TRUNCATION_CONTINUATION_HINT.to_string()),
            ),
        );
    }
    map.remove("meta");
    if keep_path_in_data {
        restore_compact_observation_path(map, &raw_data);
    }
}

fn compact_dense_arrays_to_jsonl(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                if key.ends_with("_jsonl") || key.ends_with("_count") || key == "truncation_reasons"
                {
                    continue;
                }
                let lines = map.get(&key).and_then(Value::as_array).map(|items| {
                    items
                        .iter()
                        .map(|item| compact_jsonl_item_for_model(item, key.as_str(), 0))
                        .map(|item| value_to_jsonl_line(&item))
                        .collect::<Vec<_>>()
                });
                let Some(lines) = lines else {
                    continue;
                };
                map.insert(format!("{key}_count"), json!(lines.len()));
                map.insert(format!("{key}_jsonl"), Value::String(lines.join("\n")));
                map.remove(&key);
            }
            for nested in map.values_mut() {
                compact_dense_arrays_to_jsonl(nested);
            }
        }
        Value::Array(items) => {
            for item in items {
                compact_dense_arrays_to_jsonl(item);
            }
        }
        _ => {}
    }
}

fn compact_jsonl_item_for_model(value: &Value, parent_key: &str, depth: usize) -> Value {
    if depth >= OBSERVATION_JSONL_ITEM_MAX_DEPTH {
        return value.clone();
    }
    match value {
        Value::Object(map) => {
            if parent_key == "results" {
                if let Some(compacted) = compact_execute_command_result_item(map) {
                    return compacted;
                }
            }
            let mut compacted = Map::new();
            for (key, nested_value) in map {
                if should_drop_jsonl_observation_key(key) {
                    continue;
                }
                let nested = compact_jsonl_item_for_model(nested_value, key, depth + 1);
                if is_empty_observation_value(&nested) {
                    continue;
                }
                compacted.insert(key.clone(), nested);
            }
            Value::Object(compacted)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| compact_jsonl_item_for_model(item, parent_key, depth + 1))
                .filter(|item| !is_empty_observation_value(item))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn compact_execute_command_result_item(map: &Map<String, Value>) -> Option<Value> {
    if !map.contains_key("command") {
        return None;
    }
    let has_exec_result = map.contains_key("returncode")
        || map.get("stdout").and_then(Value::as_str).is_some()
        || map.get("stderr").and_then(Value::as_str).is_some();
    if !has_exec_result {
        return None;
    }
    let mut compacted = Map::new();
    if let Some(command) = map.get("command").cloned() {
        compacted.insert("command".to_string(), command);
    }
    if let Some(returncode) = map.get("returncode").cloned() {
        compacted.insert("returncode".to_string(), returncode);
    }
    if let Some(stdout) = map.get("stdout").and_then(Value::as_str) {
        if !stdout.trim().is_empty() {
            compacted.insert("stdout".to_string(), Value::String(stdout.to_string()));
        }
    }
    if let Some(stderr) = map.get("stderr").and_then(Value::as_str) {
        if !stderr.trim().is_empty() {
            compacted.insert("stderr".to_string(), Value::String(stderr.to_string()));
        }
    }
    if compacted.is_empty() {
        None
    } else {
        Some(Value::Object(compacted))
    }
}

fn should_drop_jsonl_observation_key(key: &str) -> bool {
    if key.ends_with("_session_id") || key.ends_with("_round") {
        return true;
    }
    if key.ends_with("_meta") && key != "error_meta" {
        return true;
    }
    matches!(
        key,
        "meta"
            | "tool_call_id"
            | "trace_id"
            | "timestamp"
            | "log_profile"
            | "transport_ok"
            | "business_ok"
            | "final_ok"
            | "command_index"
            | "output_meta"
            | "elapsed_ms"
            | "duration_ms"
            | "latency_ms"
            | "timing"
            | "timings"
            | "stats"
            | "metrics"
            | "performance"
            | "perf"
    )
}

fn is_empty_observation_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn value_to_jsonl_line(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::String(text) => text.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(num) => num.to_string(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
        }
    }
}

fn strip_compact_payload_noise(value: &mut Value, depth: usize) {
    if depth > 8 {
        return;
    }
    let Value::Object(map) = value else {
        return;
    };
    for key in [
        "meta",
        "tool_call_id",
        "trace_id",
        "model_round",
        "user_round",
        "timestamp",
        "log_profile",
        "business_ok",
        "final_ok",
        "transport_ok",
    ] {
        map.remove(key);
    }
    if let Some(data) = map.get_mut("data").and_then(Value::as_object_mut) {
        for key in [
            "meta",
            "budget",
            "scope",
            "scope_note",
            "resolved_path",
            "query_mode_inferred",
            "query_source",
            "query_used",
            "scanned_files",
            "file_pattern_items",
            "case_sensitive",
            "context_after",
            "context_before",
            "engine",
        ] {
            data.remove(key);
        }
        for nested in data.values_mut() {
            strip_compact_payload_noise(nested, depth + 1);
        }
    }
    for nested in map.values_mut() {
        strip_compact_payload_noise(nested, depth + 1);
    }
}

fn restore_compact_observation_path(compacted_map: &mut Map<String, Value>, raw_data: &Value) {
    let Some(path) = raw_data
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let Some(data) = compacted_map.get_mut("data").and_then(Value::as_object_mut) else {
        return;
    };
    data.entry("path".to_string())
        .or_insert_with(|| Value::String(path.to_string()));
}

fn extract_observation_data(value: &Value) -> Value {
    let Value::Object(map) = value else {
        return value.clone();
    };
    if map.get("truncated").and_then(Value::as_bool) == Some(true) && map.contains_key("preview") {
        return compact_truncated_observation_wrapper(map);
    }
    if let Some(compacted_search) = compact_search_observation_data(map) {
        return compacted_search;
    }
    if let Some(compacted_read) = compact_read_file_observation_data(map) {
        return compacted_read;
    }
    if let Some(structured_content) = map.get("structured_content") {
        if !structured_content.is_null() {
            return structured_content.clone();
        }
    }
    if let Some(parsed) = parse_json_from_content_text_blocks(value) {
        return parsed;
    }
    if let Some(content) = map.get("content").filter(|item| !item.is_null()) {
        return content.clone();
    }
    value.clone()
}

fn compact_search_observation_data(map: &Map<String, Value>) -> Option<Value> {
    if !map.contains_key("hits") || !map.contains_key("query") {
        return None;
    }
    let mut compacted = Map::new();
    for key in [
        "query",
        "query_mode",
        "path",
        "strategy",
        "returned_match_count",
        "matched_file_count",
        "timeout_hit",
        "match_limit_hit",
        "file_limit_hit",
    ] {
        if let Some(value) = map.get(key) {
            compacted.insert(key.to_string(), value.clone());
        }
    }
    if let Some(summary) = map.get("summary").and_then(Value::as_object) {
        let mut summary_compacted = Map::new();
        for key in ["focus_points", "matched_terms", "top_files", "next_hint"] {
            if let Some(value) = summary.get(key) {
                summary_compacted.insert(key.to_string(), value.clone());
            }
        }
        if !summary_compacted.is_empty() {
            compacted.insert("summary".to_string(), Value::Object(summary_compacted));
        }
    }
    if let Some(hits) = map.get("hits").and_then(Value::as_array) {
        let mut compacted_hits = Vec::new();
        for hit in hits.iter().take(OBSERVATION_SEARCH_HIT_LIMIT) {
            let Some(hit_obj) = hit.as_object() else {
                continue;
            };
            let mut item = Map::new();
            for key in ["path", "line", "matched_terms"] {
                if let Some(value) = hit_obj.get(key) {
                    item.insert(key.to_string(), value.clone());
                }
            }
            if let Some(content) = hit_obj.get("content").and_then(Value::as_str) {
                let (content_head, omitted_chars) =
                    truncate_text_head(content, OBSERVATION_SEARCH_CONTENT_HEAD_CHARS);
                if !content_head.trim().is_empty() {
                    item.insert("content_head".to_string(), Value::String(content_head));
                }
                if omitted_chars > 0 {
                    item.insert("content_omitted_chars".to_string(), json!(omitted_chars));
                }
            }
            if !item.is_empty() {
                compacted_hits.push(Value::Object(item));
            }
        }
        if hits.len() > OBSERVATION_SEARCH_HIT_LIMIT {
            compacted_hits.push(build_omitted_items_marker(
                hits.len().saturating_sub(OBSERVATION_SEARCH_HIT_LIMIT),
            ));
        }
        if !compacted_hits.is_empty() {
            compacted.insert("hits".to_string(), Value::Array(compacted_hits));
        }
    }
    if let Some(matches) = map.get("matches").and_then(Value::as_array) {
        let mut compacted_matches = matches
            .iter()
            .take(OBSERVATION_SEARCH_HIT_LIMIT)
            .cloned()
            .collect::<Vec<_>>();
        if matches.len() > OBSERVATION_SEARCH_HIT_LIMIT {
            compacted_matches.push(build_omitted_items_marker(
                matches.len().saturating_sub(OBSERVATION_SEARCH_HIT_LIMIT),
            ));
        }
        if !compacted_matches.is_empty() {
            compacted.insert("matches".to_string(), Value::Array(compacted_matches));
        }
    }
    Some(Value::Object(compacted))
}

fn compact_read_file_observation_data(map: &Map<String, Value>) -> Option<Value> {
    let content = map.get("content").and_then(Value::as_str)?;
    let mut compacted = Map::new();
    let (clean_content, read_output_omitted_bytes) = strip_read_output_truncation_notice(content);
    compacted.insert("content".to_string(), Value::String(clean_content));
    if let Some(hint) = map
        .get("patch_usage_hint")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        compacted.insert(
            "patch_usage_hint".to_string(),
            Value::String(hint.to_string()),
        );
    }
    if map.get("continuation_required").and_then(Value::as_bool) == Some(true) {
        compacted.insert("continuation_required".to_string(), Value::Bool(true));
        if let Some(hint) = map
            .get("continuation_hint")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            compacted.insert(
                "continuation_hint".to_string(),
                Value::String(hint.to_string()),
            );
        }
    }
    if let Some(omitted_bytes) = read_output_omitted_bytes {
        compacted.insert(
            "read_output_omitted_bytes".to_string(),
            json!(omitted_bytes),
        );
        compacted.insert("continuation_required".to_string(), Value::Bool(true));
        compacted.insert(
            "continuation_hint".to_string(),
            Value::String(TRUNCATION_CONTINUATION_HINT.to_string()),
        );
    }
    let files = map.get("files").and_then(Value::as_array).or_else(|| {
        map.get("meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("files"))
            .and_then(Value::as_array)
    });
    if let Some(files) = files {
        let mut file_entries = Vec::new();
        for file in files.iter().take(OBSERVATION_READ_FILE_LIMIT) {
            let Some(file_obj) = file.as_object() else {
                continue;
            };
            let mut item = Map::new();
            for key in ["path", "read_lines", "total_lines", "complete"] {
                if let Some(value) = file_obj.get(key) {
                    item.insert(key.to_string(), value.clone());
                }
            }
            if !item.is_empty() {
                file_entries.push(Value::Object(item));
            }
        }
        if files.len() > OBSERVATION_READ_FILE_LIMIT {
            file_entries.push(build_omitted_items_marker(
                files.len().saturating_sub(OBSERVATION_READ_FILE_LIMIT),
            ));
        }
        if !file_entries.is_empty() {
            compacted.insert("files".to_string(), Value::Array(file_entries));
        }
    }
    fit_read_file_content_to_observation_budget(&mut compacted);
    Some(Value::Object(compacted))
}

fn compact_apply_patch_observation_data(map: &Map<String, Value>) -> Option<Value> {
    if !map.contains_key("changed_files")
        && !map.contains_key("files")
        && !map.contains_key("hunks_applied")
    {
        return None;
    }
    let mut compacted = Map::new();
    for key in [
        "dry_run",
        "changed_files",
        "added",
        "updated",
        "deleted",
        "moved",
        "hunks_applied",
        "no_effect_updates",
    ] {
        if let Some(value) = map.get(key) {
            compacted.insert(key.to_string(), value.clone());
        }
    }
    if let Some(files) = map.get("files").and_then(Value::as_array) {
        let mut compacted_files = Vec::new();
        for file in files.iter().take(12) {
            let Some(file_obj) = file.as_object() else {
                continue;
            };
            let mut item = Map::new();
            for key in ["action", "path", "to_path", "hunks"] {
                if let Some(value) = file_obj.get(key) {
                    item.insert(key.to_string(), value.clone());
                }
            }
            if !item.is_empty() {
                compacted_files.push(Value::Object(item));
            }
        }
        if files.len() > 12 {
            compacted_files.push(build_omitted_items_marker(files.len().saturating_sub(12)));
        }
        if !compacted_files.is_empty() {
            compacted.insert("files".to_string(), Value::Array(compacted_files));
        }
    }
    Some(Value::Object(compacted))
}

fn fit_read_file_content_to_observation_budget(compacted: &mut Map<String, Value>) {
    let Some(content) = compacted
        .get("content")
        .and_then(Value::as_str)
        .map(ToString::to_string)
    else {
        return;
    };
    let mut overhead = compacted.clone();
    overhead.remove("content");
    let content_budget = OBSERVATION_MAX_CHARS
        .saturating_sub(estimate_tool_result_chars(&Value::Object(overhead)))
        .saturating_sub("content".chars().count());
    let minimum_marker_budget = TOOL_RESULT_TRUNCATION_MARKER
        .chars()
        .count()
        .saturating_add(32);
    let content_budget = content_budget.max(minimum_marker_budget);
    let truncated = truncate_tool_result_string_to_budget(
        &content,
        content_budget,
        TOOL_RESULT_TRUNCATION_MARKER,
    );
    if truncated == content {
        return;
    }
    let (content_head, omitted_chars) = truncate_text_head(&content, 1200);
    compacted.insert("content".to_string(), Value::String(truncated));
    if !content_head.trim().is_empty() {
        compacted.insert("content_head".to_string(), Value::String(content_head));
    }
    if omitted_chars > 0 {
        compacted.insert("content_omitted_chars".to_string(), json!(omitted_chars));
    }
    compacted.insert("continuation_required".to_string(), Value::Bool(true));
    compacted
        .entry("continuation_hint".to_string())
        .or_insert_with(|| Value::String(TRUNCATION_CONTINUATION_HINT.to_string()));
}

fn strip_read_output_truncation_notice(content: &str) -> (String, Option<u64>) {
    let Some(start) = content.rfind(READ_OUTPUT_TRUNCATION_PREFIX) else {
        return (content.to_string(), None);
    };
    let notice = &content[start..];
    if !notice.ends_with(READ_OUTPUT_TRUNCATION_SUFFIX) {
        return (content.to_string(), None);
    }
    let number_start = READ_OUTPUT_TRUNCATION_PREFIX.len();
    let number_end = notice
        .len()
        .saturating_sub(READ_OUTPUT_TRUNCATION_SUFFIX.len());
    let omitted = notice
        .get(number_start..number_end)
        .and_then(|value| value.trim().parse::<u64>().ok());
    let mut cleaned = content[..start].trim_end_matches('\n').to_string();
    if cleaned.is_empty() {
        cleaned = content.to_string();
    }
    (cleaned, omitted)
}

fn build_omitted_items_marker(omitted_items: usize) -> Value {
    json!({
        "__truncated": true,
        "omitted_items": omitted_items,
    })
}

fn compact_tabular_observation_data(value: &mut Value) {
    let Value::Object(map) = value else {
        return;
    };
    let Some(rows) = map.get_mut("rows").and_then(Value::as_array_mut) else {
        return;
    };
    if rows.len() <= OBSERVATION_TABLE_SAMPLE_ROWS {
        return;
    }
    let original_len = rows.len();
    rows.truncate(OBSERVATION_TABLE_SAMPLE_ROWS);
    // Keep a tiny head sample only; large tabular payloads should stay in tools/files,
    // not be replayed through the model context page by page.
    map.insert(
        "rows_sampled".to_string(),
        json!(OBSERVATION_TABLE_SAMPLE_ROWS.min(original_len)),
    );
    map.insert(
        "rows_omitted".to_string(),
        json!(original_len.saturating_sub(OBSERVATION_TABLE_SAMPLE_ROWS)),
    );
}

fn compact_truncated_observation_wrapper(map: &Map<String, Value>) -> Value {
    let mut compacted = Map::new();
    for key in [
        "truncated",
        "original_chars",
        "truncation_reasons",
        "continuation_required",
        "continuation_hint",
        "exit_code",
    ] {
        if let Some(value) = map.get(key) {
            compacted.insert(key.to_string(), value.clone());
        }
    }
    if let Some(preview) = map.get("preview").and_then(Value::as_str) {
        // Keep wrapper previews close to the observation budget so the model
        // still sees the overall shape of a large tool result.
        let preview_budget_chars = OBSERVATION_MAX_CHARS.saturating_sub(
            estimate_tool_result_chars(&Value::Object(compacted.clone()))
                .saturating_add("preview".chars().count()),
        );
        compacted.insert(
            "preview".to_string(),
            Value::String(truncate_tool_result_string_to_budget(
                preview,
                preview_budget_chars,
                TOOL_RESULT_TRUNCATION_MARKER,
            )),
        );
    }
    Value::Object(compacted)
}

fn parse_json_from_content_text_blocks(value: &Value) -> Option<Value> {
    let content = value.get("content")?.as_array()?;
    if content.len() != 1 {
        return None;
    }
    let block = content.first()?.as_object()?;
    if block.get("type").and_then(Value::as_str) != Some("text") {
        return None;
    }
    let text = block.get("text").and_then(Value::as_str)?.trim();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(text).ok()
}

pub(super) fn truncate_tool_result_string(
    value: &str,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
) -> String {
    let value_len = value.chars().count();
    if value_len <= head_chars + tail_chars {
        return value.to_string();
    }
    let head_chars = head_chars.min(value_len);
    let tail_chars = tail_chars.min(value_len.saturating_sub(head_chars));
    let mut output = String::new();
    output.extend(value.chars().take(head_chars));
    output.push_str(marker);
    if tail_chars > 0 {
        output.extend(value.chars().skip(value_len - tail_chars).take(tail_chars));
    }
    output
}

fn truncate_tool_result_string_to_budget(value: &str, budget_chars: usize, marker: &str) -> String {
    let value_len = value.chars().count();
    if value_len <= budget_chars {
        return value.to_string();
    }
    let marker_chars = marker.chars().count();
    if budget_chars <= marker_chars {
        return marker.chars().take(budget_chars).collect();
    }
    let visible_chars = budget_chars.saturating_sub(marker_chars);
    let head_chars = visible_chars / 2;
    let tail_chars = visible_chars.saturating_sub(head_chars);
    truncate_tool_result_string(value, head_chars, tail_chars, marker)
}

fn truncate_text_head(value: &str, max_chars: usize) -> (String, usize) {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return (value.to_string(), 0);
    }
    let head = chars.iter().take(max_chars).collect::<String>();
    (head, chars.len().saturating_sub(max_chars))
}

pub(super) fn truncate_tool_result_data(
    value: &mut Value,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
) -> bool {
    truncate_tool_result_data_with_array_limits(
        value,
        head_chars,
        tail_chars,
        marker,
        TOOL_RESULT_MAX_ARRAY_ITEMS,
        TOOL_RESULT_ARRAY_HEAD_ITEMS,
        TOOL_RESULT_ARRAY_TAIL_ITEMS,
    )
}

fn truncate_tool_result_data_with_array_limits(
    value: &mut Value,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
    max_array_items: usize,
    head_array_items: usize,
    tail_array_items: usize,
) -> bool {
    match value {
        Value::String(text) => {
            if text.chars().count() > head_chars + tail_chars {
                *text = truncate_tool_result_string(text, head_chars, tail_chars, marker);
                true
            } else {
                false
            }
        }
        Value::Array(items) => {
            let mut truncated = false;
            if items.len() > max_array_items {
                let original_len = items.len();
                let head_items = head_array_items.min(original_len);
                let tail_items = tail_array_items.min(original_len - head_items);
                let omitted = original_len.saturating_sub(head_items + tail_items);
                let mut compacted = Vec::with_capacity(head_items + tail_items + 1);
                compacted.extend(items.iter().take(head_items).cloned());
                compacted.push(build_omitted_items_marker(omitted));
                if tail_items > 0 {
                    compacted.extend(items.iter().skip(original_len - tail_items).cloned());
                }
                *items = compacted;
                truncated = true;
            }
            for item in items.iter_mut() {
                if truncate_tool_result_data_with_array_limits(
                    item,
                    head_chars,
                    tail_chars,
                    marker,
                    max_array_items,
                    head_array_items,
                    tail_array_items,
                ) {
                    truncated = true;
                }
            }
            truncated
        }
        Value::Object(map) => {
            let mut truncated = false;
            let (next_max_items, next_head_items, next_tail_items) =
                if map_has_continuation_signal(map) {
                    (
                        TOOL_RESULT_PAGINATED_MAX_ARRAY_ITEMS,
                        TOOL_RESULT_PAGINATED_ARRAY_HEAD_ITEMS,
                        TOOL_RESULT_PAGINATED_ARRAY_TAIL_ITEMS,
                    )
                } else {
                    (max_array_items, head_array_items, tail_array_items)
                };
            for value in map.values_mut() {
                if truncate_tool_result_data_with_array_limits(
                    value,
                    head_chars,
                    tail_chars,
                    marker,
                    next_max_items,
                    next_head_items,
                    next_tail_items,
                ) {
                    truncated = true;
                }
            }
            truncated
        }
        _ => false,
    }
}

fn truncate_observation_data(
    value: &mut Value,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
) -> bool {
    match value {
        Value::String(text) => {
            if text.chars().count() > head_chars + tail_chars {
                *text = truncate_tool_result_string(text, head_chars, tail_chars, marker);
                true
            } else {
                false
            }
        }
        Value::Array(items) => {
            let mut truncated = false;
            if items.len() > OBSERVATION_MAX_ARRAY_ITEMS {
                let original_len = items.len();
                let head_items = OBSERVATION_ARRAY_HEAD_ITEMS.min(original_len);
                let tail_items = OBSERVATION_ARRAY_TAIL_ITEMS.min(original_len - head_items);
                let omitted = original_len.saturating_sub(head_items + tail_items);
                let mut compacted = Vec::with_capacity(head_items + tail_items + 1);
                compacted.extend(items.iter().take(head_items).cloned());
                compacted.push(build_omitted_items_marker(omitted));
                if tail_items > 0 {
                    compacted.extend(items.iter().skip(original_len - tail_items).cloned());
                }
                *items = compacted;
                truncated = true;
            }
            for item in items.iter_mut() {
                if truncate_observation_data(item, head_chars, tail_chars, marker) {
                    truncated = true;
                }
            }
            truncated
        }
        Value::Object(map) => {
            let mut truncated = false;
            for inner in map.values_mut() {
                if truncate_observation_data(inner, head_chars, tail_chars, marker) {
                    truncated = true;
                }
            }
            truncated
        }
        _ => false,
    }
}

pub(super) fn compact_large_tool_result_data(
    value: &Value,
    original_chars: usize,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
    continuation_supported: bool,
    truncation_reasons: &[String],
) -> Value {
    let serialized = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
    let preview = truncate_tool_result_string(&serialized, head_chars, tail_chars, marker);
    let mut payload = json!({
        "truncated": true,
        "original_chars": original_chars,
        "preview": preview,
    });
    if !truncation_reasons.is_empty() {
        if let Value::Object(ref mut map) = payload {
            map.insert(
                "truncation_reasons".to_string(),
                Value::Array(
                    truncation_reasons
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect(),
                ),
            );
        }
    }
    if continuation_supported {
        if let Value::Object(ref mut map) = payload {
            map.insert("continuation_required".to_string(), Value::Bool(true));
            map.insert(
                "continuation_hint".to_string(),
                Value::String(TRUNCATION_CONTINUATION_HINT.to_string()),
            );
        }
    }
    if let Some(exit_code) = extract_exit_code(value) {
        if let Value::Object(ref mut map) = payload {
            map.insert("exit_code".to_string(), json!(exit_code));
        }
    }
    payload
}

pub(super) fn append_truncation_reason(reasons: &mut Vec<String>, reason: &str) {
    if reasons.iter().any(|item| item == reason) {
        return;
    }
    reasons.push(reason.to_string());
}

pub(super) fn dedupe_truncation_reasons(reasons: &mut Vec<String>) {
    let mut deduped = Vec::with_capacity(reasons.len());
    for reason in reasons.iter() {
        if deduped.iter().any(|item: &String| item == reason) {
            continue;
        }
        deduped.push(reason.clone());
    }
    *reasons = deduped;
}

pub(super) fn collect_truncation_reasons_from_value(value: &Value, marker: &str) -> Vec<String> {
    let mut reasons = Vec::new();
    if value_contains_omitted_items_marker(value) {
        append_truncation_reason(&mut reasons, "array_items");
    }
    if value_contains_string_truncation_marker(value, marker) {
        append_truncation_reason(&mut reasons, "string_chars");
    }
    reasons
}

fn value_contains_omitted_items_marker(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("__truncated").and_then(Value::as_bool) == Some(true)
                && map
                    .get("omitted_items")
                    .and_then(Value::as_u64)
                    .unwrap_or_default()
                    > 0
            {
                return true;
            }
            map.values().any(value_contains_omitted_items_marker)
        }
        Value::Array(items) => items.iter().any(value_contains_omitted_items_marker),
        _ => false,
    }
}

fn value_contains_string_truncation_marker(value: &Value, marker: &str) -> bool {
    match value {
        Value::String(text) => text.contains(marker),
        Value::Array(items) => items
            .iter()
            .any(|item| value_contains_string_truncation_marker(item, marker)),
        Value::Object(map) => map
            .values()
            .any(|item| value_contains_string_truncation_marker(item, marker)),
        _ => false,
    }
}

pub(super) fn estimate_tool_result_chars(value: &Value) -> usize {
    match value {
        Value::String(text) => text.chars().count(),
        Value::Number(num) => num.to_string().chars().count(),
        Value::Bool(flag) => {
            if *flag {
                4
            } else {
                5
            }
        }
        Value::Null => 4,
        Value::Array(items) => items.iter().map(estimate_tool_result_chars).sum(),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| key.chars().count() + estimate_tool_result_chars(value))
            .sum(),
    }
}

fn parse_exit_code(value: &Value) -> Option<i64> {
    match value {
        Value::Number(num) => num.as_i64().or_else(|| num.as_u64().map(|val| val as i64)),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

pub(super) fn extract_exit_code(value: &Value) -> Option<i64> {
    let obj = value.as_object()?;
    for key in [
        "exit_code",
        "exitCode",
        "returncode",
        "return_code",
        "status_code",
    ] {
        if let Some(code) = obj.get(key).and_then(parse_exit_code) {
            return Some(code);
        }
    }
    if let Some(Value::Array(items)) = obj.get("results") {
        for item in items {
            let Some(result) = item.as_object() else {
                continue;
            };
            for key in [
                "exit_code",
                "exitCode",
                "returncode",
                "return_code",
                "status_code",
            ] {
                if let Some(code) = result.get(key).and_then(parse_exit_code) {
                    return Some(code);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
