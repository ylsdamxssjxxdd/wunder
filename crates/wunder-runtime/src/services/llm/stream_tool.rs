use crate::core::tool_args::{
    normalize_tool_arguments_json as normalize_tool_arguments_json_lossy,
    normalize_tool_arguments_json_with_meta,
};
use crate::tools::{extract_freeform_tool_input, is_freeform_tool_name};
use serde_json::{json, Value};

#[derive(Debug, Default, Clone)]
pub(super) struct StreamToolCall {
    pub(super) id: Option<String>,
    pub(super) source_id: Option<String>,
    pub(super) name_delta: String,
    pub(super) name_snapshot: Option<String>,
    pub(super) arguments_delta: String,
    pub(super) arguments_snapshot: Option<String>,
    pub(super) raw_arguments: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StreamToolFieldMode {
    Delta,
    Snapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolArgumentsCandidateSource {
    Delta,
    Snapshot,
}

#[derive(Clone, Debug)]
struct ToolArgumentsCandidate {
    source: ToolArgumentsCandidateSource,
    normalized: String,
    parsed: Option<Value>,
    quality: u8,
}

pub(super) fn update_stream_tool_calls_delta(acc: &mut Vec<StreamToolCall>, payload: &Value) {
    match payload {
        Value::Array(items) => {
            if items.is_empty() {
                return;
            }
            for item in items {
                merge_stream_tool_call_item(acc, item, StreamToolFieldMode::Delta);
            }
        }
        Value::Object(map) => {
            if let Some(tool_calls) = map.get("tool_calls").or_else(|| map.get("tool_call")) {
                update_stream_tool_calls_delta(acc, tool_calls);
            } else if looks_like_stream_tool_call_item(map) {
                merge_stream_tool_call_item(acc, payload, StreamToolFieldMode::Delta);
            }

            if let Some(function_call) = map.get("function_call") {
                if acc.is_empty() {
                    acc.push(StreamToolCall::default());
                }
                apply_function_fragment(&mut acc[0], function_call, StreamToolFieldMode::Delta);
            }
        }
        _ => {}
    }
}

pub(super) fn update_stream_tool_calls_snapshot(acc: &mut Vec<StreamToolCall>, payload: &Value) {
    match payload {
        Value::Array(items) => {
            if items.is_empty() {
                return;
            }
            for item in items {
                merge_stream_tool_call_item(acc, item, StreamToolFieldMode::Snapshot);
            }
        }
        Value::Object(map) => {
            if let Some(tool_calls) = map.get("tool_calls").or_else(|| map.get("tool_call")) {
                update_stream_tool_calls_snapshot(acc, tool_calls);
            } else if looks_like_stream_tool_call_item(map) {
                merge_stream_tool_call_item(acc, payload, StreamToolFieldMode::Snapshot);
            }

            if let Some(function_call) = map.get("function_call") {
                if acc.is_empty() {
                    acc.push(StreamToolCall::default());
                }
                apply_function_fragment(&mut acc[0], function_call, StreamToolFieldMode::Snapshot);
            }
        }
        _ => {}
    }
}

pub(super) fn update_responses_tool_call_from_item(acc: &mut Vec<StreamToolCall>, item: &Value) {
    let Value::Object(map) = item else {
        return;
    };
    let item_type = map
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if item_type != "function_call" && item_type != "custom_tool_call" {
        return;
    }
    let item_id = map
        .get("id")
        .or_else(|| map.get("item_id"))
        .and_then(Value::as_str);
    let call_id = map
        .get("call_id")
        .or_else(|| map.get("callId"))
        .and_then(Value::as_str);
    let name = map.get("name").and_then(Value::as_str).or_else(|| {
        map.get("function")
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
    });
    let arguments = map.get("arguments").and_then(Value::as_str).or_else(|| {
        map.get("function")
            .and_then(|value| value.get("arguments"))
            .and_then(Value::as_str)
    });
    let custom_input = map.get("input").and_then(Value::as_str);
    let arguments = if item_type == "custom_tool_call" {
        custom_input.or(arguments)
    } else {
        arguments
    };
    let raw_arguments = item_type == "custom_tool_call";
    upsert_response_tool_call(
        acc,
        item_id,
        call_id,
        name,
        arguments,
        StreamToolFieldMode::Snapshot,
        raw_arguments,
    );
}

pub(super) fn update_responses_tool_call_arguments(acc: &mut Vec<StreamToolCall>, payload: &Value) {
    let item_id = payload.get("item_id").and_then(Value::as_str);
    let call_id = payload
        .get("call_id")
        .or_else(|| payload.get("callId"))
        .and_then(Value::as_str);
    let arguments = payload
        .get("delta")
        .or_else(|| payload.get("arguments"))
        .and_then(Value::as_str);
    if arguments.is_none() && call_id.is_none() && item_id.is_none() {
        return;
    }
    upsert_response_tool_call(
        acc,
        item_id,
        call_id,
        None,
        arguments,
        StreamToolFieldMode::Delta,
        false,
    );
}

pub(super) fn upsert_responses_tool_calls(acc: &mut Vec<StreamToolCall>, tool_calls: &[Value]) {
    for call in tool_calls {
        let Value::Object(map) = call else {
            continue;
        };
        let call_id = map
            .get("id")
            .or_else(|| map.get("call_id"))
            .or_else(|| map.get("tool_call_id"))
            .or_else(|| map.get("toolCallId"))
            .and_then(Value::as_str);
        let function = map.get("function").or_else(|| map.get("function_call"));
        let name = function
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str);
        let arguments = function
            .and_then(|value| value.get("arguments"))
            .and_then(Value::as_str);
        if name.is_none() && arguments.is_none() && call_id.is_none() {
            continue;
        }
        upsert_response_tool_call(
            acc,
            None,
            call_id,
            name,
            arguments,
            StreamToolFieldMode::Snapshot,
            name.is_some_and(is_freeform_tool_name),
        );
    }
}

fn upsert_response_tool_call(
    acc: &mut Vec<StreamToolCall>,
    item_id: Option<&str>,
    call_id: Option<&str>,
    name: Option<&str>,
    arguments: Option<&str>,
    mode: StreamToolFieldMode,
    raw_arguments: bool,
) {
    let index = find_response_tool_call_index(acc, item_id, call_id).unwrap_or_else(|| {
        acc.push(StreamToolCall::default());
        acc.len().saturating_sub(1)
    });
    let slot = &mut acc[index];
    if let Some(item_id) = item_id {
        let trimmed = item_id.trim();
        if !trimmed.is_empty() {
            slot.source_id = Some(trimmed.to_string());
        }
    }
    if let Some(call_id) = call_id {
        let trimmed = call_id.trim();
        if !trimmed.is_empty() {
            slot.id = Some(trimmed.to_string());
        }
    }
    if let Some(name) = name {
        apply_stream_text_field(
            match mode {
                StreamToolFieldMode::Delta => &mut slot.name_delta,
                StreamToolFieldMode::Snapshot => {
                    slot.name_snapshot = Some(name.to_string());
                    &mut slot.name_delta
                }
            },
            name,
            mode,
        );
    }
    if let Some(arguments) = arguments {
        apply_stream_arguments_field(slot, arguments, mode);
    }
    if raw_arguments
        || resolve_stream_tool_call_name(slot).is_some_and(|name| is_freeform_tool_name(&name))
    {
        slot.raw_arguments = true;
    }
}

fn find_response_tool_call_index(
    acc: &[StreamToolCall],
    item_id: Option<&str>,
    call_id: Option<&str>,
) -> Option<usize> {
    if let Some(item_id) = item_id {
        if let Some(index) = acc
            .iter()
            .position(|call| call.source_id.as_deref() == Some(item_id))
        {
            return Some(index);
        }
    }
    if let Some(call_id) = call_id {
        if let Some(index) = acc
            .iter()
            .position(|call| call.id.as_deref() == Some(call_id))
        {
            return Some(index);
        }
    }
    None
}

fn looks_like_stream_tool_call_item(map: &serde_json::Map<String, Value>) -> bool {
    if map.contains_key("function") {
        return true;
    }

    let has_name = map.get("name").is_some_and(Value::is_string);
    let has_arguments = map.contains_key("arguments");
    if has_name && has_arguments {
        return true;
    }

    let has_index_or_id = map.contains_key("index")
        || map.contains_key("id")
        || map.contains_key("tool_call_id")
        || map.contains_key("toolCallId")
        || map.contains_key("call_id")
        || map.contains_key("callId");
    let is_function_type = map
        .get("type")
        .and_then(Value::as_str)
        .map(|value| value.eq_ignore_ascii_case("function"))
        .unwrap_or(false);
    has_arguments && (has_index_or_id || is_function_type)
}

pub(super) fn merge_stream_tool_call_item(
    acc: &mut Vec<StreamToolCall>,
    item: &Value,
    mode: StreamToolFieldMode,
) {
    let Value::Object(map) = item else {
        return;
    };

    let index = map.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    while acc.len() <= index {
        acc.push(StreamToolCall::default());
    }

    let slot = &mut acc[index];
    let item_type = map
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if item_type == "custom_tool_call" {
        slot.raw_arguments = true;
    }
    if let Some(id) = map
        .get("id")
        .or_else(|| map.get("call_id"))
        .or_else(|| map.get("callId"))
        .or_else(|| map.get("tool_call_id"))
        .or_else(|| map.get("toolCallId"))
        .and_then(Value::as_str)
        .map(str::trim)
    {
        if !id.is_empty() {
            slot.id = Some(id.to_string());
        }
    }

    if let Some(function) = map.get("function") {
        apply_function_fragment(slot, function, mode);
    } else {
        apply_function_fragment(slot, item, mode);
    }
}

fn apply_function_fragment(slot: &mut StreamToolCall, function: &Value, mode: StreamToolFieldMode) {
    if let Value::Object(map) = function {
        if let Some(name) = map.get("name").and_then(Value::as_str) {
            apply_stream_text_field(&mut slot.name_delta, name, mode);
            if matches!(mode, StreamToolFieldMode::Snapshot) {
                slot.name_snapshot = Some(name.to_string());
            }
            if is_freeform_tool_name(name) {
                slot.raw_arguments = true;
            }
        }
        if let Some(arguments) = map.get("arguments").and_then(Value::as_str) {
            apply_stream_arguments_field(slot, arguments, mode);
        } else if let Some(input) = map.get("input").and_then(Value::as_str) {
            slot.raw_arguments = true;
            apply_stream_arguments_field(slot, input, mode);
        }
    }
}

fn apply_stream_text_field(target: &mut String, fragment: &str, mode: StreamToolFieldMode) {
    match mode {
        StreamToolFieldMode::Delta => merge_stream_delta_field(target, fragment),
        StreamToolFieldMode::Snapshot => {
            target.clear();
            target.push_str(fragment);
        }
    }
}

fn apply_stream_arguments_field(
    slot: &mut StreamToolCall,
    fragment: &str,
    mode: StreamToolFieldMode,
) {
    if fragment.is_empty() {
        return;
    }
    match mode {
        StreamToolFieldMode::Delta => merge_stream_delta_field(&mut slot.arguments_delta, fragment),
        StreamToolFieldMode::Snapshot => slot.arguments_snapshot = Some(fragment.to_string()),
    }
}

pub(super) fn merge_stream_delta_field(target: &mut String, fragment: &str) {
    if fragment.is_empty() {
        return;
    }
    if target.is_empty() {
        target.push_str(fragment);
        return;
    }
    if target == fragment {
        return;
    }
    if fragment.starts_with(target.as_str()) {
        *target = fragment.to_string();
        return;
    }
    if target.starts_with(fragment) {
        return;
    }
    target.push_str(fragment);
}

fn merge_tool_argument_values(current: &mut Value, next: &Value) -> bool {
    match (current, next) {
        (Value::Object(current_map), Value::Object(next_map)) => {
            for (key, next_value) in next_map {
                match current_map.get_mut(key) {
                    Some(current_value) => {
                        if !merge_tool_argument_values(current_value, next_value) {
                            return false;
                        }
                    }
                    None => {
                        current_map.insert(key.clone(), next_value.clone());
                    }
                }
            }
            true
        }
        (Value::Array(current_items), Value::Array(next_items)) => {
            if current_items.len() > next_items.len() {
                return false;
            }
            for (index, next_item) in next_items.iter().enumerate() {
                if let Some(current_item) = current_items.get_mut(index) {
                    if !merge_tool_argument_values(current_item, next_item) {
                        return false;
                    }
                } else {
                    current_items.push(next_item.clone());
                }
            }
            true
        }
        (Value::String(current_text), Value::String(next_text)) => {
            if next_text.starts_with(current_text.as_str()) {
                *current_text = next_text.clone();
                true
            } else {
                current_text.starts_with(next_text.as_str())
            }
        }
        (current_value, next_value) => current_value == next_value,
    }
}

fn build_tool_arguments_candidate(
    raw: &str,
    source: ToolArgumentsCandidateSource,
) -> Option<ToolArgumentsCandidate> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        let normalized = match &parsed {
            Value::Object(_) => trimmed.to_string(),
            _ => normalize_tool_arguments_json_lossy(trimmed),
        };
        let quality = if parsed.is_object() { 4 } else { 1 };
        let parsed = serde_json::from_str::<Value>(&normalized).ok();
        return Some(ToolArgumentsCandidate {
            source,
            normalized,
            parsed,
            quality,
        });
    }

    let (normalized, repair) = normalize_tool_arguments_json_with_meta(trimmed);
    let parsed = serde_json::from_str::<Value>(&normalized).ok();
    let strategy = repair
        .as_ref()
        .and_then(|value| value.get("strategy"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let quality = match parsed.as_ref() {
        Some(Value::Object(map))
            if strategy == "lossy_json_string_repair"
                || strategy == "empty_object_prefix_removed" =>
        {
            if map.contains_key("raw") || map.contains_key("value") {
                1
            } else {
                3
            }
        }
        Some(Value::Object(map)) => {
            if map.contains_key("raw") || map.contains_key("value") {
                1
            } else {
                2
            }
        }
        _ => 0,
    };
    Some(ToolArgumentsCandidate {
        source,
        normalized,
        parsed,
        quality,
    })
}

fn merge_tool_argument_candidates(
    delta: &ToolArgumentsCandidate,
    snapshot: &ToolArgumentsCandidate,
) -> Option<String> {
    let mut merged = delta.parsed.clone()?;
    let snapshot_value = snapshot.parsed.as_ref()?;
    merge_tool_argument_values(&mut merged, snapshot_value)
        .then(|| serde_json::to_string(&merged).unwrap_or_else(|_| snapshot.normalized.clone()))
}

fn choose_tool_arguments_candidate<'a>(
    left: &'a ToolArgumentsCandidate,
    right: &'a ToolArgumentsCandidate,
) -> &'a ToolArgumentsCandidate {
    if right.parsed.as_ref() == Some(&json!({})) && left.quality > 0 {
        return left;
    }
    let left_rank = (
        left.quality,
        left.parsed
            .as_ref()
            .and_then(Value::as_object)
            .map(|map| map.len())
            .unwrap_or(0),
        left.normalized.len(),
        matches!(left.source, ToolArgumentsCandidateSource::Snapshot),
    );
    let right_rank = (
        right.quality,
        right
            .parsed
            .as_ref()
            .and_then(Value::as_object)
            .map(|map| map.len())
            .unwrap_or(0),
        right.normalized.len(),
        matches!(right.source, ToolArgumentsCandidateSource::Snapshot),
    );
    if right_rank > left_rank {
        right
    } else {
        left
    }
}

pub(super) fn resolve_stream_tool_call_name(call: &StreamToolCall) -> Option<String> {
    call.name_snapshot
        .as_deref()
        .or_else(|| (!call.name_delta.is_empty()).then_some(call.name_delta.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) fn resolve_stream_tool_call_arguments(call: &StreamToolCall) -> String {
    if call.raw_arguments {
        if call.arguments_snapshot.as_deref() == Some("{}") && !call.arguments_delta.is_empty() {
            return format!(
                "{}{}",
                call.arguments_snapshot.as_deref().unwrap_or("{}"),
                call.arguments_delta
            );
        }
        return call
            .arguments_snapshot
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| (!call.arguments_delta.is_empty()).then_some(call.arguments_delta.as_str()))
            .unwrap_or("{}")
            .to_string();
    }
    if call.arguments_snapshot.as_deref() == Some("{}")
        && serde_json::from_str::<Value>(&call.arguments_delta)
            .map(|value| value.is_object())
            .unwrap_or(false)
    {
        return call.arguments_delta.clone();
    }
    let delta =
        build_tool_arguments_candidate(&call.arguments_delta, ToolArgumentsCandidateSource::Delta);
    let snapshot = call.arguments_snapshot.as_deref().and_then(|raw| {
        build_tool_arguments_candidate(raw, ToolArgumentsCandidateSource::Snapshot)
    });

    match (delta, snapshot) {
        (Some(delta), Some(snapshot)) => {
            if let Some(merged) = merge_tool_argument_candidates(&delta, &snapshot) {
                merged
            } else {
                choose_tool_arguments_candidate(&delta, &snapshot)
                    .normalized
                    .clone()
            }
        }
        (Some(delta), None) => delta.normalized,
        (None, Some(snapshot)) => snapshot.normalized,
        (None, None) => "{}".to_string(),
    }
}

pub(super) fn finalize_stream_tool_calls(acc: &[StreamToolCall]) -> Option<Value> {
    let mut output = Vec::new();
    for call in acc {
        let Some(name) = resolve_stream_tool_call_name(call) else {
            continue;
        };
        let raw_arguments = resolve_stream_tool_call_arguments(call);
        let arguments = if is_freeform_tool_name(&name) {
            if call.raw_arguments {
                raw_arguments.clone()
            } else {
                let input =
                    extract_freeform_tool_input(&raw_arguments).unwrap_or(raw_arguments.clone());
                serde_json::to_string(&json!({ "input": input }))
                    .unwrap_or_else(|_| "{}".to_string())
            }
        } else {
            raw_arguments
        };
        let mut payload = json!({
            "type": "function",
            "function": {
                "name": name,
                "arguments": arguments,
            }
        });
        if let Some(id) = call.id.as_ref().or(call.source_id.as_ref()) {
            if let Value::Object(ref mut map) = payload {
                map.insert("id".to_string(), Value::String(id.clone()));
            }
        }
        output.push(payload);
    }
    if output.is_empty() {
        None
    } else {
        Some(Value::Array(output))
    }
}
