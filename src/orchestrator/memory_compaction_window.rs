use super::memory_support::*;
use super::*;

pub(super) fn mark_current_user_message_inflight(message: &mut Value) {
    let Some(map) = message.as_object_mut() else {
        return;
    };
    let meta = map
        .entry("meta".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(meta_obj) = meta.as_object_mut() else {
        return;
    };
    meta_obj.insert(
        COMPACTION_INFLIGHT_CURRENT_USER_META_KEY.to_string(),
        Value::Bool(true),
    );
}

pub(super) fn is_compaction_inflight_current_user_message(message: &Value) -> bool {
    message
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get(COMPACTION_INFLIGHT_CURRENT_USER_META_KEY))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn clear_compaction_inflight_current_user_marker(message: &mut Value) {
    let Some(map) = message.as_object_mut() else {
        return;
    };
    let Some(meta_obj) = map.get_mut("meta").and_then(Value::as_object_mut) else {
        return;
    };
    meta_obj.remove(COMPACTION_INFLIGHT_CURRENT_USER_META_KEY);
    if meta_obj.is_empty() {
        map.remove("meta");
    }
}

pub(super) fn build_committed_replacement_history_from_rebuilt(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|message| {
            if message.get("role").and_then(Value::as_str) == Some("system")
                || is_compaction_inflight_current_user_message(message)
            {
                return None;
            }
            let mut cloned = message.clone();
            clear_compaction_inflight_current_user_marker(&mut cloned);
            clear_retained_interaction_marker(&mut cloned);
            normalize_committed_replacement_history_message(&cloned)
        })
        .collect()
}

pub(super) fn is_compaction_tool_call_summary_text(text: &str) -> bool {
    let cleaned = text.trim();
    cleaned == "Assistant issued tool call(s)."
        || cleaned.starts_with("Assistant issued tool call(s):")
}

pub(super) fn strip_compaction_internal_tool_lines(text: &str) -> String {
    let stripped = strip_tool_calls(text);
    let cleaned = stripped.trim();
    if cleaned.is_empty() {
        return String::new();
    }
    let filtered = cleaned
        .lines()
        .filter(|line| !is_compaction_tool_call_summary_text(line))
        .collect::<Vec<_>>()
        .join("\n");
    filtered.trim().to_string()
}

pub(super) fn normalize_committed_replacement_history_message(message: &Value) -> Option<Value> {
    let obj = message.as_object()?;
    let role = obj
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if role.is_empty() || role == "system" || role == "tool" {
        return None;
    }

    let content = obj.get("content").unwrap_or(&Value::Null);
    if Orchestrator::is_observation_message(role.as_str(), content) {
        return None;
    }

    let cleaned_text = strip_compaction_internal_tool_lines(&extract_guard_content_text(content));
    let has_non_text = message_has_non_text_content(message);
    if cleaned_text.is_empty() && !has_non_text {
        return None;
    }

    let normalized_role = if role == "assistant" {
        "assistant"
    } else {
        "user"
    };
    let normalized_content = if !cleaned_text.is_empty() {
        Value::String(cleaned_text)
    } else if normalized_role == "user" {
        content.clone()
    } else {
        return None;
    };

    Some(json!({
        "role": normalized_role,
        "content": normalized_content,
    }))
}

pub(super) fn clear_compaction_inflight_markers(messages: &mut [Value]) {
    for message in messages {
        clear_compaction_inflight_current_user_marker(message);
    }
}

pub(super) fn mark_retained_interaction_message(message: &mut Value) {
    let Some(obj) = message.as_object_mut() else {
        return;
    };
    let meta = obj
        .entry("meta".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(meta_obj) = meta.as_object_mut() else {
        return;
    };
    meta_obj.insert(
        COMPACTION_RETAINED_INTERACTION_META_KEY.to_string(),
        Value::Bool(true),
    );
}

pub(super) fn mark_retained_interaction_messages(messages: &mut [Value]) {
    for message in messages {
        mark_retained_interaction_message(message);
    }
}

pub(super) fn is_retained_interaction_message(message: &Value) -> bool {
    message
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get(COMPACTION_RETAINED_INTERACTION_META_KEY))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn clear_retained_interaction_marker(message: &mut Value) {
    let Some(obj) = message.as_object_mut() else {
        return;
    };
    let Some(meta) = obj.get_mut("meta").and_then(Value::as_object_mut) else {
        return;
    };
    meta.remove(COMPACTION_RETAINED_INTERACTION_META_KEY);
    if meta.is_empty() {
        obj.remove("meta");
    }
}

pub(super) fn clear_retained_interaction_markers(messages: &mut [Value]) {
    for message in messages {
        clear_retained_interaction_marker(message);
    }
}

pub(super) fn locate_rebuilt_current_user_index(messages: &[Value]) -> Option<usize> {
    if let Some(index) = messages
        .iter()
        .rposition(is_compaction_inflight_current_user_message)
    {
        return Some(index);
    }

    messages.iter().rposition(|message| {
        if message.get("role").and_then(Value::as_str) != Some("user") {
            return false;
        }
        if HistoryManager::is_compaction_summary_item(message) {
            return false;
        }
        let content = message.get("content").unwrap_or(&Value::Null);
        !Orchestrator::is_observation_message("user", content)
    })
}

pub(super) fn locate_compaction_summary_message_index(messages: &[Value]) -> Option<usize> {
    messages.iter().position(|message| {
        if message.get("role").and_then(Value::as_str) != Some("user") {
            return false;
        }
        let content = message.get("content").unwrap_or(&Value::Null);
        let text = extract_guard_content_text(content);
        starts_with_compaction_prefix(&text)
    })
}

#[derive(Clone, Debug)]
pub(super) struct InteractionBlock {
    pub(super) indexes: Vec<usize>,
    pub(super) message: Value,
}

pub(super) fn normalize_message_for_interaction_block(
    message: &Value,
) -> Option<(&'static str, String)> {
    let obj = message.as_object()?;
    let role = obj
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if role.is_empty() || role == "system" || role == "tool" {
        return None;
    }
    let content = obj.get("content").unwrap_or(&Value::Null);
    let text = if is_compaction_observation_message(role.as_str(), obj) {
        summarize_compaction_observation(content)
    } else {
        strip_compaction_internal_tool_lines(&extract_guard_content_text(content))
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized_role = if role == "assistant" {
        "assistant"
    } else {
        "user"
    };
    Some((normalized_role, trimmed.to_string()))
}

pub(super) fn build_interaction_block_message(role: &str, parts: &[String]) -> Option<Value> {
    let merged = parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .fold(Vec::<String>::new(), |mut acc, part| {
            if acc.last().map(String::as_str) != Some(part) {
                acc.push(part.to_string());
            }
            acc
        })
        .join("\n\n");
    let cleaned = merged.trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(json!({
        "role": role,
        "content": cleaned,
    }))
}

pub(super) fn split_messages_into_interaction_turns_with_boundary(
    messages: &[Value],
    boundary_index: Option<usize>,
) -> Vec<InteractionBlock> {
    let mut turns: Vec<InteractionBlock> = Vec::new();
    let mut current_role: Option<&'static str> = None;
    let mut current_indexes: Vec<usize> = Vec::new();
    let mut current_parts: Vec<String> = Vec::new();

    let flush_current = |turns: &mut Vec<InteractionBlock>,
                         current_role: &mut Option<&'static str>,
                         current_indexes: &mut Vec<usize>,
                         current_parts: &mut Vec<String>| {
        let Some(role) = *current_role else {
            return;
        };
        if current_indexes.is_empty() || current_parts.is_empty() {
            current_indexes.clear();
            current_parts.clear();
            *current_role = None;
            return;
        }
        if let Some(message) = build_interaction_block_message(role, current_parts) {
            turns.push(InteractionBlock {
                indexes: std::mem::take(current_indexes),
                message,
            });
        } else {
            current_indexes.clear();
        }
        current_parts.clear();
        *current_role = None;
    };

    for (index, message) in messages.iter().enumerate() {
        if boundary_index == Some(index) {
            flush_current(
                &mut turns,
                &mut current_role,
                &mut current_indexes,
                &mut current_parts,
            );
        }
        if HistoryManager::is_compaction_summary_item(message) {
            continue;
        }
        let Some((normalized_role, text)) = normalize_message_for_interaction_block(message) else {
            continue;
        };
        if current_role != Some(normalized_role) {
            flush_current(
                &mut turns,
                &mut current_role,
                &mut current_indexes,
                &mut current_parts,
            );
            current_role = Some(normalized_role);
        }
        current_indexes.push(index);
        current_parts.push(text);
    }

    flush_current(
        &mut turns,
        &mut current_role,
        &mut current_indexes,
        &mut current_parts,
    );
    turns
}

pub(super) fn split_messages_into_interaction_turns(messages: &[Value]) -> Vec<InteractionBlock> {
    split_messages_into_interaction_turns_with_boundary(messages, None)
}

pub(super) fn collect_normalized_interaction_blocks(messages: &[Value]) -> Vec<InteractionBlock> {
    split_messages_into_interaction_turns(messages)
        .iter()
        .filter_map(normalize_interaction_turn_messages)
        .collect()
}

pub(super) fn collect_normalized_interaction_blocks_with_boundary(
    messages: &[Value],
    boundary_index: Option<usize>,
) -> Vec<InteractionBlock> {
    split_messages_into_interaction_turns_with_boundary(messages, boundary_index)
        .iter()
        .filter_map(normalize_interaction_turn_messages)
        .collect()
}

pub(super) fn estimate_message_chars(message: &Value) -> usize {
    let content = message.get("content").unwrap_or(&Value::Null);
    match content {
        Value::String(text) => text.chars().count(),
        Value::Null => 0,
        _ => extract_guard_content_text(content).chars().count(),
    }
}

pub(super) fn trim_message_to_fit_chars(message: &Value, max_chars: usize) -> Option<Value> {
    if max_chars == 0 {
        return None;
    }
    if estimate_message_chars(message) <= max_chars {
        return Some(message.clone());
    }
    let mut cloned = message.clone();
    let Some(obj) = cloned.as_object_mut() else {
        return Some(cloned);
    };
    let content = obj.get("content").cloned().unwrap_or(Value::Null);
    let trimmed_content = match content {
        Value::String(text) => Value::String(trim_text_to_chars(
            &text,
            max_chars,
            COMPACTION_TEXT_TRUNCATION_SUFFIX,
        )),
        _ => {
            let max_tokens = ((max_chars as f64) / 4.0).ceil() as i64;
            return trim_message_to_fit_tokens(&cloned, max_tokens.max(1));
        }
    };
    if extract_guard_content_text(&trimmed_content)
        .trim()
        .is_empty()
    {
        return None;
    }
    obj.insert("content".to_string(), trimmed_content);
    Some(cloned)
}

pub(super) fn build_compaction_message_debug_entries(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let content = message.get("content").unwrap_or(&Value::Null);
            let preview = trim_text_to_chars(
                &extract_guard_content_text(content),
                COMPACTION_DEBUG_PREVIEW_CHARS,
                COMPACTION_TEXT_TRUNCATION_SUFFIX,
            );
            json!({
                "index": index,
                "role": role,
                "tokens": estimate_message_tokens(message),
                "chars": estimate_message_chars(message),
                "is_summary": HistoryManager::is_compaction_summary_item(message),
                "is_current_user": is_compaction_inflight_current_user_message(message),
                "preview": preview,
            })
        })
        .collect()
}

pub(super) fn normalize_interaction_turn_messages(
    turn: &InteractionBlock,
) -> Option<InteractionBlock> {
    let capped = trim_message_to_fit_tokens(
        &turn.message,
        COMPACTION_RETAINED_INTERACTION_MESSAGE_MAX_TOKENS,
    )
    .unwrap_or_else(|| turn.message.clone());
    if COMPACTION_RETAINED_INTERACTION_TURN_MAX_CHARS == 0 {
        return None;
    }
    let chars = estimate_message_chars(&capped);
    let message = if chars <= COMPACTION_RETAINED_INTERACTION_TURN_MAX_CHARS {
        capped
    } else {
        trim_message_to_fit_chars(&capped, COMPACTION_RETAINED_INTERACTION_TURN_MAX_CHARS)?
    };
    Some(InteractionBlock {
        indexes: turn.indexes.clone(),
        message,
    })
}

pub(super) fn trim_interaction_turn_to_budget(
    turn: &InteractionBlock,
    token_limit: i64,
) -> Option<InteractionBlock> {
    if token_limit <= 0 {
        return None;
    }
    let message_tokens = estimate_message_tokens(&turn.message);
    let message = if message_tokens <= token_limit {
        turn.message.clone()
    } else {
        trim_message_to_fit_tokens(&turn.message, token_limit.max(1))?
    };
    let mut message = message;
    mark_retained_interaction_message(&mut message);
    Some(InteractionBlock {
        indexes: turn.indexes.clone(),
        message,
    })
}

pub(super) fn collect_interaction_turns_with_budget(
    turns: &[InteractionBlock],
    token_limit: i64,
    from_end: bool,
) -> Vec<InteractionBlock> {
    if token_limit <= 0 || turns.is_empty() {
        return Vec::new();
    }
    let mut remaining = token_limit;
    let mut selected_turns: Vec<InteractionBlock> = Vec::new();

    let iter: Box<dyn Iterator<Item = &InteractionBlock>> = if from_end {
        Box::new(turns.iter().rev())
    } else {
        Box::new(turns.iter())
    };

    for turn in iter {
        if remaining <= 0 {
            break;
        }
        let turn_tokens = estimate_message_tokens(&turn.message);
        if turn_tokens <= remaining {
            let mut selected_turn = turn.clone();
            mark_retained_interaction_message(&mut selected_turn.message);
            selected_turns.push(selected_turn);
            remaining = remaining.saturating_sub(turn_tokens);
            continue;
        }
        if let Some(trimmed_turn) = trim_interaction_turn_to_budget(turn, remaining) {
            selected_turns.push(trimmed_turn);
        }
        break;
    }

    if from_end {
        selected_turns.reverse();
    }
    selected_turns
}

#[cfg(test)]
pub(super) fn collect_retained_interaction_messages_for_compaction(
    messages: &[Value],
    retained_turn_count: usize,
    head_token_limit: i64,
    tail_token_limit: i64,
) -> Vec<Value> {
    let (head_messages, tail_messages) = collect_retained_interaction_segments_for_compaction(
        messages,
        retained_turn_count,
        head_token_limit,
        tail_token_limit,
    );
    head_messages.into_iter().chain(tail_messages).collect()
}

#[allow(dead_code)]
pub(super) fn collect_retained_interaction_segments_for_compaction(
    messages: &[Value],
    retained_turn_count: usize,
    head_token_limit: i64,
    tail_token_limit: i64,
) -> (Vec<Value>, Vec<Value>) {
    let segments = collect_retained_interaction_segments_with_indexes_for_compaction(
        messages,
        None,
        retained_turn_count,
        head_token_limit,
        tail_token_limit,
    );
    (segments.head_messages, segments.tail_messages)
}

pub(super) struct RetainedInteractionSegments {
    pub(super) head_messages: Vec<Value>,
    pub(super) tail_messages: Vec<Value>,
}

pub(super) fn collect_retained_interaction_segments_with_indexes_for_compaction(
    messages: &[Value],
    boundary_index: Option<usize>,
    retained_turn_count: usize,
    head_token_limit: i64,
    tail_token_limit: i64,
) -> RetainedInteractionSegments {
    if retained_turn_count == 0 {
        return RetainedInteractionSegments {
            head_messages: Vec::new(),
            tail_messages: Vec::new(),
        };
    }

    if split_messages_into_interaction_turns_with_boundary(messages, boundary_index).is_empty() {
        return RetainedInteractionSegments {
            head_messages: Vec::new(),
            tail_messages: Vec::new(),
        };
    }
    let normalized_turns =
        collect_normalized_interaction_blocks_with_boundary(messages, boundary_index);
    if normalized_turns.is_empty() {
        return RetainedInteractionSegments {
            head_messages: Vec::new(),
            tail_messages: Vec::new(),
        };
    }

    let turn_count = normalized_turns.len();
    let head_len = retained_turn_count.min(turn_count);
    let tail_start = turn_count.saturating_sub(retained_turn_count).max(head_len);
    let head_messages = collect_interaction_turns_with_budget(
        &normalized_turns[..head_len],
        head_token_limit,
        false,
    );
    let tail_messages = if tail_start >= turn_count {
        Vec::new()
    } else {
        collect_interaction_turns_with_budget(
            &normalized_turns[tail_start..],
            tail_token_limit,
            true,
        )
    };
    RetainedInteractionSegments {
        head_messages: head_messages
            .into_iter()
            .map(|block| block.message)
            .collect(),
        tail_messages: tail_messages
            .into_iter()
            .map(|block| block.message)
            .collect(),
    }
}

#[allow(dead_code)]
pub(super) fn collect_retained_interaction_messages_from_window(
    messages: &[Value],
    token_limit: i64,
    from_end: bool,
) -> Vec<Value> {
    if token_limit <= 0 || messages.is_empty() {
        return Vec::new();
    }
    if split_messages_into_interaction_turns(messages).is_empty() {
        return Vec::new();
    }
    let normalized_turns = collect_normalized_interaction_blocks(messages);
    if normalized_turns.is_empty() {
        return Vec::new();
    }
    collect_interaction_turns_with_budget(&normalized_turns, token_limit, from_end)
        .into_iter()
        .map(|block| block.message)
        .collect()
}
