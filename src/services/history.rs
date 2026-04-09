// 历史管理：加载对话历史、压缩摘要与产物索引。
use crate::config::LlmModelConfig;
use crate::i18n;
use crate::orchestrator_constants::{
    ARTIFACT_INDEX_MAX_ITEMS, COMPACTION_META_TYPE, COMPACTION_OUTPUT_RESERVE, COMPACTION_RATIO,
    COMPACTION_REPLACEMENT_HISTORY_META_KEY, COMPACTION_SAFETY_MARGIN, OBSERVATION_PREFIX,
};
use crate::prompting::read_prompt_template;
use crate::workspace::WorkspaceManager;
use chrono::DateTime;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::task::spawn_blocking;

pub struct HistoryManager;

impl HistoryManager {
    pub fn format_compaction_summary(summary: &str) -> String {
        let cleaned = summary.trim();
        let mut output = if cleaned.is_empty() {
            i18n::t("memory.empty_summary")
        } else {
            cleaned.to_string()
        };
        let prefixes = i18n::get_known_prefixes("history.compaction_prefix");
        if !prefixes.iter().any(|prefix| output.starts_with(prefix)) {
            output = format!("{}\n{output}", i18n::t("history.compaction_prefix"));
        }
        output
    }

    pub fn format_artifact_index(content: &str) -> String {
        let cleaned = content.trim();
        if cleaned.is_empty() {
            return String::new();
        }
        let prefixes = i18n::get_known_prefixes("history.artifact_prefix");
        if prefixes.iter().any(|prefix| cleaned.starts_with(prefix)) {
            return cleaned.to_string();
        }
        format!("{}\n{cleaned}", i18n::t("history.artifact_prefix"))
    }

    pub fn is_compaction_summary_item(item: &Value) -> bool {
        let meta_type = item
            .get("meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("type"))
            .and_then(Value::as_str);
        if meta_type == Some(COMPACTION_META_TYPE) {
            return true;
        }
        let content = item.get("content").and_then(Value::as_str).unwrap_or("");
        let prefixes = i18n::get_known_prefixes("history.compaction_prefix");
        prefixes.iter().any(|prefix| content.starts_with(prefix))
    }

    pub fn load_compaction_prompt() -> String {
        let path = Path::new("prompts/compact_prompt.txt");
        let prompt = read_prompt_template(path).trim().to_string();
        if prompt.is_empty() {
            i18n::t("history.compaction_prompt_fallback")
        } else {
            prompt
        }
    }

    pub fn get_auto_compact_limit(llm_config: &LlmModelConfig) -> Option<i64> {
        let max_context = llm_config.max_context.unwrap_or(0) as i64;
        if max_context <= 0 {
            return None;
        }
        let ratio_limit = (max_context as f64 * COMPACTION_RATIO) as i64;
        let reserve_output = llm_config
            .max_output
            .and_then(|value| if value > 0 { Some(value as i64) } else { None })
            .unwrap_or(COMPACTION_OUTPUT_RESERVE);
        let hard_limit = max_context - reserve_output - COMPACTION_SAFETY_MARGIN;
        if hard_limit <= 0 {
            return Some(max_context.max(1).min(ratio_limit.max(1)));
        }
        Some(hard_limit.min(ratio_limit.max(1)).max(1))
    }

    pub fn get_item_timestamp(item: &Value) -> Option<f64> {
        parse_timestamp(item.get("timestamp"))
    }

    pub fn load_history_messages(
        &self,
        workspace: &WorkspaceManager,
        user_id: &str,
        session_id: &str,
        max_items: i64,
    ) -> Vec<Value> {
        let history = workspace
            .load_history(user_id, session_id, max_items)
            .unwrap_or_default();
        let mut messages = Vec::new();
        for item in materialize_history_items(&history) {
            if let Some(message) = build_message_from_item(&item, true) {
                messages.push(message);
            }
        }
        messages
    }

    pub async fn load_history_messages_async(
        &self,
        workspace: Arc<WorkspaceManager>,
        user_id: String,
        session_id: String,
        max_items: i64,
    ) -> Vec<Value> {
        spawn_blocking(move || {
            let manager = HistoryManager;
            manager.load_history_messages(&workspace, &user_id, &session_id, max_items)
        })
        .await
        .unwrap_or_default()
    }

    pub fn build_compaction_candidates(history: &[Value]) -> (Vec<Value>, Vec<Value>) {
        let mut items = Vec::new();
        let mut messages = Vec::new();
        for item in materialize_history_items(history) {
            if let Some(message) = build_message_from_item(&item, true) {
                items.push(item.clone());
                messages.push(message);
            }
        }
        (items, messages)
    }

    pub fn load_artifact_index_message(
        &self,
        workspace: &WorkspaceManager,
        user_id: &str,
        session_id: &str,
    ) -> String {
        let artifacts = workspace
            .load_artifact_logs(user_id, session_id, ARTIFACT_INDEX_MAX_ITEMS)
            .unwrap_or_default();
        let text = self.build_artifact_index_text(&artifacts);
        Self::format_artifact_index(&text)
    }

    pub fn build_artifact_index_text(&self, artifacts: &[Value]) -> String {
        if artifacts.is_empty() {
            return String::new();
        }
        let mut file_reads = Vec::new();
        let mut file_changes: HashMap<String, Vec<String>> = HashMap::new();
        let mut commands = Vec::new();
        let mut scripts = Vec::new();
        let mut failures = Vec::new();
        let action_labels = HashMap::from([
            ("read", i18n::t("history.action.read")),
            ("write", i18n::t("history.action.write")),
            ("replace", i18n::t("history.action.replace")),
            ("edit", i18n::t("history.action.edit")),
            ("execute", i18n::t("history.action.execute")),
            ("run", i18n::t("history.action.run")),
        ]);

        for entry in artifacts {
            let kind = entry
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let action = entry
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let ok = entry.get("ok").and_then(Value::as_bool).unwrap_or(true);
            let error = entry
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let meta = entry.get("meta").and_then(Value::as_object);
            if !error.is_empty() || !ok {
                let label = if !name.is_empty() {
                    name.to_string()
                } else {
                    entry
                        .get("tool")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string()
                };
                if !label.is_empty() {
                    let failure_text = if error.is_empty() {
                        i18n::t("history.failure.execution")
                    } else {
                        error.to_string()
                    };
                    failures.push(format!("{label}: {failure_text}"));
                }
            }
            if name.is_empty() {
                continue;
            }
            match kind {
                "file" => {
                    if action == "read" {
                        file_reads.push(name.to_string());
                    } else {
                        let actions = file_changes.entry(name.to_string()).or_default();
                        let action_label =
                            action_labels.get(action).cloned().unwrap_or_else(|| {
                                if action.is_empty() {
                                    i18n::t("history.action.unknown")
                                } else {
                                    action.to_string()
                                }
                            });
                        if !actions.contains(&action_label) {
                            actions.push(action_label);
                        }
                    }
                }
                "command" => {
                    let returncode = meta
                        .and_then(|meta| meta.get("returncode"))
                        .and_then(Value::as_i64);
                    let suffix = returncode
                        .map(|rc| format!("(rc={rc})"))
                        .unwrap_or_default();
                    commands.push(format!("{name}{suffix}"));
                }
                "script" => {
                    let returncode = meta
                        .and_then(|meta| meta.get("returncode"))
                        .and_then(Value::as_i64);
                    let suffix = returncode
                        .map(|rc| format!("(rc={rc})"))
                        .unwrap_or_default();
                    scripts.push(format!("{name}{suffix}"));
                }
                _ => {}
            }
        }

        let file_reads = unique_in_order(file_reads);
        let commands = unique_in_order(commands);
        let scripts = unique_in_order(scripts);
        let failures = unique_in_order(failures);
        let mut file_change_items = Vec::new();
        for (path, actions) in file_changes {
            let action_text = if actions.is_empty() {
                i18n::t("history.action.unknown")
            } else {
                actions.join("/")
            };
            file_change_items.push(format!("{path}({action_text})"));
        }
        let file_change_items = unique_in_order(file_change_items);

        let list_limit = 12;
        let mut lines = vec![i18n::t("history.artifact_prefix")];
        if !file_reads.is_empty() {
            lines.push(i18n::t_with_params(
                "history.summary.file_reads",
                &HashMap::from([
                    ("count".to_string(), file_reads.len().to_string()),
                    (
                        "items".to_string(),
                        format_index_items(&file_reads, list_limit),
                    ),
                ]),
            ));
        }
        if !file_change_items.is_empty() {
            lines.push(i18n::t_with_params(
                "history.summary.file_changes",
                &HashMap::from([
                    ("count".to_string(), file_change_items.len().to_string()),
                    (
                        "items".to_string(),
                        format_index_items(&file_change_items, list_limit),
                    ),
                ]),
            ));
        }
        if !commands.is_empty() {
            lines.push(i18n::t_with_params(
                "history.summary.command_runs",
                &HashMap::from([
                    ("count".to_string(), commands.len().to_string()),
                    (
                        "items".to_string(),
                        format_index_items(&commands, list_limit),
                    ),
                ]),
            ));
        }
        if !scripts.is_empty() {
            lines.push(i18n::t_with_params(
                "history.summary.script_runs",
                &HashMap::from([
                    ("count".to_string(), scripts.len().to_string()),
                    (
                        "items".to_string(),
                        format_index_items(&scripts, list_limit),
                    ),
                ]),
            ));
        }
        if !failures.is_empty() {
            lines.push(i18n::t_with_params(
                "history.summary.failures",
                &HashMap::from([
                    ("count".to_string(), failures.len().to_string()),
                    (
                        "items".to_string(),
                        format_index_items(&failures, list_limit),
                    ),
                ]),
            ));
        }
        lines.join("\n")
    }
}

fn unique_in_order(items: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for item in items {
        if item.is_empty() || seen.contains(&item) {
            continue;
        }
        seen.insert(item.clone());
        output.push(item);
    }
    output
}

fn format_index_items(items: &[String], limit: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    let total = items.len();
    let display = items.iter().take(limit).cloned().collect::<Vec<_>>();
    let suffix = if total > limit {
        i18n::t_with_params(
            "history.items_suffix",
            &HashMap::from([("total".to_string(), total.to_string())]),
        )
    } else {
        String::new()
    };
    if suffix.is_empty() {
        display.join(", ")
    } else {
        format!("{}{}", display.join(", "), suffix)
    }
}

fn parse_timestamp(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(num) => num.as_f64(),
        Value::String(text) => {
            let cleaned = text.trim();
            if cleaned.is_empty() {
                return None;
            }
            let normalized = if let Some(prefix) = cleaned.strip_suffix('Z') {
                format!("{prefix}+00:00")
            } else {
                cleaned.to_string()
            };
            DateTime::parse_from_rfc3339(&normalized)
                .ok()
                .map(|dt| dt.timestamp() as f64)
        }
        _ => None,
    }
}

fn build_message_from_item(item: &Value, include_reasoning: bool) -> Option<Value> {
    let role = item.get("role").and_then(Value::as_str)?;
    let content = item.get("content")?.clone();
    if role == "tool" {
        let content_text = match &content {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        if let Some(tool_call_id) = extract_tool_call_id(item) {
            return Some(json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "content": content_text,
            }));
        }
        return Some(json!({
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{content_text}"),
        }));
    }
    let mut message = json!({ "role": role, "content": content });
    if include_reasoning && role == "assistant" {
        let reasoning = item
            .get("reasoning_content")
            .or_else(|| item.get("reasoning"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !reasoning.is_empty() {
            if let Value::Object(ref mut map) = message {
                map.insert("reasoning_content".to_string(), json!(reasoning));
            }
        }
    }
    if role == "assistant" {
        if let Some(tool_calls) = extract_tool_calls_payload(item) {
            if let Value::Object(ref mut map) = message {
                map.insert("tool_calls".to_string(), tool_calls);
            }
        }
        if let Some(tool_call_id) = extract_tool_call_id(item) {
            if let Value::Object(ref mut map) = message {
                map.insert("tool_call_id".to_string(), Value::String(tool_call_id));
            }
        }
    }
    Some(message)
}

fn is_tool_call_item(item: &Value) -> bool {
    item.get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("type"))
        .and_then(Value::as_str)
        .map(|value| value == "tool_call")
        .unwrap_or(false)
}

fn extract_tool_calls_payload(item: &Value) -> Option<Value> {
    let value = item
        .get("tool_calls")
        .or_else(|| item.get("tool_call"))
        .or_else(|| item.get("function_call"))?;
    if value.is_null() {
        None
    } else {
        Some(value.clone())
    }
}

fn extract_tool_call_id(item: &Value) -> Option<String> {
    item.get("tool_call_id")
        .or_else(|| item.get("toolCallId"))
        .or_else(|| item.get("call_id"))
        .or_else(|| item.get("callId"))
        .and_then(|value| match value {
            Value::String(text) => Some(text.clone()),
            Value::Number(num) => Some(num.to_string()),
            _ => None,
        })
        .and_then(|text| {
            let cleaned = text.trim().to_string();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned)
            }
        })
}

fn has_tool_calls_payload(item: &Value) -> bool {
    extract_tool_calls_payload(item).is_some()
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Default)]
struct FilteredHistoryItems {
    head_items: Vec<Value>,
    tail_items: Vec<Value>,
    summary_item: Option<Value>,
    compacted_until_ts: Option<f64>,
    summary_index: i64,
}

impl FilteredHistoryItems {
    #[cfg_attr(not(test), allow(dead_code))]
    fn active_items(self) -> Vec<Value> {
        let mut items = self.head_items;
        items.extend(self.tail_items);
        items
    }
}

fn extract_retained_summary_index(item: Option<&Value>, key: &str) -> Option<usize> {
    item?.get("meta")?.get(key).and_then(|value| match value {
        Value::Number(num) => num.as_u64().and_then(|value| usize::try_from(value).ok()),
        Value::String(text) => text.trim().parse::<usize>().ok(),
        _ => None,
    })
}

fn apply_legacy_summary_boundary(
    active_items: Vec<Value>,
    compacted_until_ts: Option<f64>,
) -> Vec<Value> {
    let Some(boundary) = compacted_until_ts else {
        return Vec::new();
    };
    active_items
        .into_iter()
        .filter(|item| {
            parse_timestamp(item.get("timestamp"))
                .map(|item_ts| item_ts > boundary)
                .unwrap_or(false)
        })
        .collect()
}

fn split_retained_summary_items(
    active_items: &[Value],
    summary_item: Option<&Value>,
) -> Option<(usize, Vec<Value>)> {
    let head_end = extract_retained_summary_index(summary_item, "retained_head_until_index");
    let tail_start = extract_retained_summary_index(summary_item, "retained_tail_from_index");
    if head_end.is_none() && tail_start.is_none() {
        return None;
    }

    let mut next_items = Vec::new();
    let mut head_len = 0usize;

    if let Some(head_end) = head_end {
        let keep = head_end.saturating_add(1).min(active_items.len());
        next_items.extend(active_items.iter().take(keep).cloned());
        head_len = next_items.len();
    }

    let mut effective_tail_start = tail_start
        .unwrap_or(active_items.len())
        .min(active_items.len());
    if head_len > 0 {
        effective_tail_start = effective_tail_start.max(head_len);
    }
    next_items.extend(active_items.iter().skip(effective_tail_start).cloned());
    Some((head_len, next_items))
}

fn build_formatted_compaction_summary_message(summary_item: &Value) -> Value {
    json!({
        "role": "user",
        "content": HistoryManager::format_compaction_summary(
            summary_item
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
        )
    })
}

fn normalize_replacement_history_item(item: &Value) -> Option<Value> {
    let role = item.get("role").and_then(Value::as_str)?.trim();
    if role.is_empty() || role == "system" {
        return None;
    }
    let content = item
        .get("content")
        .cloned()
        .unwrap_or(Value::String(String::new()));
    let mut normalized = serde_json::Map::new();
    normalized.insert("role".to_string(), Value::String(role.to_string()));
    normalized.insert("content".to_string(), content);
    if let Some(reasoning) = item
        .get("reasoning_content")
        .or_else(|| item.get("reasoning"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        normalized.insert(
            "reasoning_content".to_string(),
            Value::String(reasoning.to_string()),
        );
    }
    if let Some(tool_calls) = extract_tool_calls_payload(item) {
        normalized.insert("tool_calls".to_string(), tool_calls);
    }
    if let Some(tool_call_id) = extract_tool_call_id(item) {
        normalized.insert("tool_call_id".to_string(), Value::String(tool_call_id));
    }
    if let Some(meta) = item.get("meta").cloned().filter(|value| !value.is_null()) {
        normalized.insert("meta".to_string(), meta);
    }
    Some(Value::Object(normalized))
}

fn extract_compaction_replacement_history(summary_item: Option<&Value>) -> Option<Vec<Value>> {
    let items = summary_item?
        .get("meta")?
        .get(COMPACTION_REPLACEMENT_HISTORY_META_KEY)?
        .as_array()?;
    let normalized = items
        .iter()
        .filter_map(normalize_replacement_history_item)
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn push_replay_item(filtered: &mut Vec<Value>, item: &Value, skip_next_assistant: &mut bool) {
    let role = item.get("role").and_then(Value::as_str).unwrap_or("");
    if role == "system" {
        return;
    }
    if *skip_next_assistant {
        if role == "assistant" {
            *skip_next_assistant = false;
            return;
        }
        *skip_next_assistant = false;
    }
    if role == "assistant" && is_tool_call_item(item) && !has_tool_calls_payload(item) {
        *skip_next_assistant = true;
    }
    filtered.push(item.clone());
}

fn materialize_history_items(history: &[Value]) -> Vec<Value> {
    let mut active_items = Vec::new();
    let mut skip_next_assistant = false;

    for item in history {
        if HistoryManager::is_compaction_summary_item(item) {
            if let Some(replacement_history) = extract_compaction_replacement_history(Some(item)) {
                active_items = replacement_history;
                skip_next_assistant = false;
                continue;
            }
            let summary_message = build_formatted_compaction_summary_message(item);
            let compacted_until_ts = extract_compacted_until_ts(Some(item));
            if let Some((head_len, retained_items)) =
                split_retained_summary_items(&active_items, Some(item))
            {
                active_items = retained_items;
                let insert_index = head_len.min(active_items.len());
                active_items.insert(insert_index, summary_message);
            } else {
                active_items = apply_legacy_summary_boundary(active_items, compacted_until_ts);
                active_items.insert(0, summary_message);
            }
            skip_next_assistant = false;
            continue;
        }
        push_replay_item(&mut active_items, item, &mut skip_next_assistant);
    }

    active_items
}

#[cfg_attr(not(test), allow(dead_code))]
fn filter_history_items(history: &[Value]) -> FilteredHistoryItems {
    let mut active_items = Vec::new();
    let mut summary_item: Option<Value> = None;
    let mut compacted_until_ts = None;
    let mut summary_index: i64 = -1;
    let mut latest_head_len = 0usize;
    let mut skip_next_assistant = false;

    for (index, item) in history.iter().enumerate() {
        if HistoryManager::is_compaction_summary_item(item) {
            summary_index = index as i64;
            summary_item = Some(item.clone());
            compacted_until_ts = extract_compacted_until_ts(Some(item));
            if let Some((head_len, retained_items)) =
                split_retained_summary_items(&active_items, Some(item))
            {
                active_items = retained_items;
                latest_head_len = head_len.min(active_items.len());
            } else {
                active_items = apply_legacy_summary_boundary(active_items, compacted_until_ts);
                latest_head_len = 0;
            }
            skip_next_assistant = false;
            continue;
        }
        push_replay_item(&mut active_items, item, &mut skip_next_assistant);
    }

    if summary_item.is_none() {
        return FilteredHistoryItems {
            head_items: Vec::new(),
            tail_items: active_items,
            summary_item,
            compacted_until_ts,
            summary_index,
        };
    }

    let head_len = latest_head_len.min(active_items.len());
    let tail_items = active_items.split_off(head_len);
    FilteredHistoryItems {
        head_items: active_items,
        tail_items,
        summary_item,
        compacted_until_ts,
        summary_index,
    }
}

fn extract_compacted_until_ts(item: Option<&Value>) -> Option<f64> {
    let meta = item?.get("meta")?.as_object()?;
    let raw = meta
        .get("compacted_until_ts")
        .or_else(|| meta.get("compacted_until"));
    parse_timestamp(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n;
    use crate::storage::SqliteStorage;
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::runtime::Builder;

    fn with_language<T>(language: &str, f: impl FnOnce() -> T) -> T {
        Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime")
            .block_on(i18n::with_language(
                language.to_string(),
                async move { f() },
            ))
    }

    #[test]
    fn load_compaction_prompt_uses_localized_template() {
        let zh_prompt = with_language("zh-CN", HistoryManager::load_compaction_prompt);
        assert!(zh_prompt.contains("## 当前进展"));
        assert!(zh_prompt.contains("## 剩余待办"));

        let en_prompt = with_language("en-US", HistoryManager::load_compaction_prompt);
        assert!(en_prompt.contains("## Current progress"));
        assert!(en_prompt.contains("## Critical references"));
    }

    #[test]
    fn filter_history_items_discards_pre_summary_history_by_timestamp_boundary() {
        let summary = json!({
            "role": "user",
            "content": "summary",
            "timestamp": "2026-03-26T10:00:02Z",
            "meta": {
                "type": COMPACTION_META_TYPE,
                "compacted_until": "2026-03-26T10:00:01Z"
            }
        });
        let current_user = json!({
            "role": "user",
            "content": "current question",
            "timestamp": "2026-03-26T10:00:03Z"
        });
        let current_assistant = json!({
            "role": "assistant",
            "content": "current answer",
            "timestamp": "2026-03-26T10:00:04Z"
        });
        let history = vec![
            json!({
                "role": "user",
                "content": "old question",
                "timestamp": "2026-03-26T10:00:00Z"
            }),
            json!({
                "role": "assistant",
                "content": "old answer",
                "timestamp": "2026-03-26T10:00:01Z"
            }),
            summary.clone(),
            current_user.clone(),
            current_assistant.clone(),
        ];

        let filtered = filter_history_items(&history);

        assert_eq!(filtered.summary_item, Some(summary));
        assert!(filtered.compacted_until_ts.is_some());
        assert_eq!(filtered.summary_index, 2);
        assert!(filtered.head_items.is_empty());
        assert_eq!(filtered.tail_items, vec![current_user, current_assistant]);
    }

    #[test]
    fn filter_history_items_uses_summary_index_fallback_without_timestamps() {
        let summary = json!({
            "role": "user",
            "content": "summary",
            "meta": {
                "type": COMPACTION_META_TYPE
            }
        });
        let history = vec![
            json!({ "role": "user", "content": "old question" }),
            json!({ "role": "assistant", "content": "old answer" }),
            summary.clone(),
            json!({ "role": "user", "content": "current question" }),
        ];

        let filtered = filter_history_items(&history);

        assert_eq!(filtered.summary_item, Some(summary));
        assert_eq!(filtered.compacted_until_ts, None);
        assert_eq!(filtered.summary_index, 2);
        assert!(filtered.head_items.is_empty());
        assert_eq!(filtered.tail_items.len(), 1);
        assert_eq!(filtered.tail_items[0]["content"], json!("current question"));
    }

    #[test]
    fn filter_history_items_keeps_recent_tail_before_summary_when_boundary_allows_it() {
        let summary = json!({
            "role": "user",
            "content": "summary",
            "timestamp": "2026-03-27T00:00:06Z",
            "meta": {
                "type": COMPACTION_META_TYPE,
                "compacted_until": "2026-03-27T00:00:02Z"
            }
        });
        let tail_user = json!({
            "role": "user",
            "content": "tail question",
            "timestamp": "2026-03-27T00:00:03Z"
        });
        let tail_assistant = json!({
            "role": "assistant",
            "content": "tail answer",
            "timestamp": "2026-03-27T00:00:04Z"
        });
        let current_user = json!({
            "role": "user",
            "content": "current question",
            "timestamp": "2026-03-27T00:00:07Z"
        });
        let history = vec![
            json!({
                "role": "user",
                "content": "old question",
                "timestamp": "2026-03-27T00:00:01Z"
            }),
            json!({
                "role": "assistant",
                "content": "old answer",
                "timestamp": "2026-03-27T00:00:02Z"
            }),
            tail_user.clone(),
            tail_assistant.clone(),
            summary,
            current_user.clone(),
        ];

        let filtered = filter_history_items(&history);

        assert_eq!(filtered.summary_index, 4);
        assert!(filtered.compacted_until_ts.is_some());
        assert!(filtered.head_items.is_empty());
        assert_eq!(
            filtered.tail_items,
            vec![tail_user, tail_assistant, current_user]
        );
    }

    #[test]
    fn materialize_history_items_prefers_committed_replacement_history_snapshot() {
        let summary_prefix = with_language("en-US", || i18n::t("history.compaction_prefix"));
        let summary = json!({
            "role": "system",
            "content": "summary",
            "meta": {
                "type": COMPACTION_META_TYPE,
                COMPACTION_REPLACEMENT_HISTORY_META_KEY: [
                    { "role": "user", "content": "snapshot question" },
                    { "role": "assistant", "content": "snapshot answer" },
                    { "role": "user", "content": format!("{summary_prefix}\nsnapshot summary") }
                ]
            }
        });
        let history = vec![
            json!({ "role": "user", "content": "old question" }),
            json!({ "role": "assistant", "content": "old answer" }),
            summary,
            json!({ "role": "user", "content": "future question" }),
        ];

        let materialized = materialize_history_items(&history);

        assert_eq!(materialized.len(), 4);
        assert_eq!(materialized[0]["content"], json!("snapshot question"));
        assert_eq!(materialized[1]["content"], json!("snapshot answer"));
        assert_eq!(
            materialized[2]["content"],
            json!(format!("{summary_prefix}\nsnapshot summary"))
        );
        assert_eq!(materialized[3]["content"], json!("future question"));
    }

    #[test]
    fn materialize_history_items_prefers_latest_replacement_history_snapshot() {
        let summary_prefix = with_language("en-US", || i18n::t("history.compaction_prefix"));
        let first_summary = json!({
            "role": "system",
            "content": "summary-1",
            "meta": {
                "type": COMPACTION_META_TYPE,
                COMPACTION_REPLACEMENT_HISTORY_META_KEY: [
                    { "role": "user", "content": "snapshot one" },
                    { "role": "user", "content": format!("{summary_prefix}\nsummary one") }
                ]
            }
        });
        let second_summary = json!({
            "role": "system",
            "content": "summary-2",
            "meta": {
                "type": COMPACTION_META_TYPE,
                COMPACTION_REPLACEMENT_HISTORY_META_KEY: [
                    { "role": "assistant", "content": "snapshot two" },
                    { "role": "user", "content": format!("{summary_prefix}\nsummary two") }
                ]
            }
        });
        let history = vec![
            json!({ "role": "user", "content": "old question" }),
            first_summary,
            json!({ "role": "user", "content": "middle question" }),
            second_summary,
            json!({ "role": "assistant", "content": "future answer" }),
        ];

        let materialized = materialize_history_items(&history);

        assert_eq!(materialized.len(), 3);
        assert_eq!(materialized[0]["content"], json!("snapshot two"));
        assert_eq!(
            materialized[1]["content"],
            json!(format!("{summary_prefix}\nsummary two"))
        );
        assert_eq!(materialized[2]["content"], json!("future answer"));
    }

    #[test]
    fn filter_history_items_only_keeps_current_turn_when_boundary_is_after_tail() {
        let summary = json!({
            "role": "user",
            "content": "summary",
            "timestamp": "2026-03-27T00:00:06Z",
            "meta": {
                "type": COMPACTION_META_TYPE,
                "compacted_until": "2026-03-27T00:00:04Z"
            }
        });
        let current_user = json!({
            "role": "user",
            "content": "current question",
            "timestamp": "2026-03-27T00:00:07Z"
        });
        let history = vec![
            json!({
                "role": "user",
                "content": "old question",
                "timestamp": "2026-03-27T00:00:01Z"
            }),
            json!({
                "role": "assistant",
                "content": "old answer",
                "timestamp": "2026-03-27T00:00:02Z"
            }),
            json!({
                "role": "user",
                "content": "tail question",
                "timestamp": "2026-03-27T00:00:03Z"
            }),
            json!({
                "role": "assistant",
                "content": "tail answer",
                "timestamp": "2026-03-27T00:00:04Z"
            }),
            summary,
            current_user.clone(),
        ];

        let filtered = filter_history_items(&history);

        assert_eq!(filtered.summary_index, 4);
        assert!(filtered.compacted_until_ts.is_some());
        assert!(filtered.head_items.is_empty());
        assert_eq!(filtered.tail_items, vec![current_user]);
    }

    #[test]
    fn filter_history_items_retains_head_and_tail_anchors_around_summary() {
        let summary = json!({
            "role": "user",
            "content": "summary",
            "timestamp": "2026-03-27T00:00:07Z",
            "meta": {
                "type": COMPACTION_META_TYPE,
                "compacted_until": "2026-03-27T00:00:06Z",
                "retained_head_until_index": 1,
                "retained_tail_from_index": 4
            }
        });
        let head_user = json!({
            "role": "user",
            "content": "head question",
            "timestamp": "2026-03-27T00:00:01Z"
        });
        let head_assistant = json!({
            "role": "assistant",
            "content": "head answer",
            "timestamp": "2026-03-27T00:00:02Z"
        });
        let middle_user = json!({
            "role": "user",
            "content": "middle question",
            "timestamp": "2026-03-27T00:00:03Z"
        });
        let middle_assistant = json!({
            "role": "assistant",
            "content": "middle answer",
            "timestamp": "2026-03-27T00:00:04Z"
        });
        let tail_user = json!({
            "role": "user",
            "content": "tail question",
            "timestamp": "2026-03-27T00:00:05Z"
        });
        let tail_assistant = json!({
            "role": "assistant",
            "content": "tail answer",
            "timestamp": "2026-03-27T00:00:06Z"
        });
        let current_user = json!({
            "role": "user",
            "content": "current question",
            "timestamp": "2026-03-27T00:00:08Z"
        });
        let history = vec![
            head_user.clone(),
            head_assistant.clone(),
            middle_user,
            middle_assistant,
            tail_user.clone(),
            tail_assistant.clone(),
            summary.clone(),
            current_user.clone(),
        ];

        let filtered = filter_history_items(&history);

        assert_eq!(filtered.summary_item, Some(summary));
        assert_eq!(filtered.summary_index, 6);
        assert_eq!(filtered.head_items, vec![head_user, head_assistant]);
        assert_eq!(
            filtered.tail_items,
            vec![tail_user, tail_assistant, current_user]
        );
    }

    #[test]
    fn filter_history_items_applies_latest_retained_summary_without_reviving_old_turns() {
        let history = vec![
            json!({
                "role": "user",
                "content": "head question",
                "timestamp": "2026-03-27T00:00:01Z"
            }),
            json!({
                "role": "assistant",
                "content": "head answer",
                "timestamp": "2026-03-27T00:00:02Z"
            }),
            json!({
                "role": "user",
                "content": "dropped question",
                "timestamp": "2026-03-27T00:00:03Z"
            }),
            json!({
                "role": "assistant",
                "content": "dropped answer",
                "timestamp": "2026-03-27T00:00:04Z"
            }),
            json!({
                "role": "user",
                "content": "old tail question",
                "timestamp": "2026-03-27T00:00:05Z"
            }),
            json!({
                "role": "assistant",
                "content": "old tail answer",
                "timestamp": "2026-03-27T00:00:06Z"
            }),
            json!({
                "role": "user",
                "content": "older tail question",
                "timestamp": "2026-03-27T00:00:07Z"
            }),
            json!({
                "role": "assistant",
                "content": "older tail answer",
                "timestamp": "2026-03-27T00:00:08Z"
            }),
            json!({
                "role": "user",
                "content": "summary-1",
                "timestamp": "2026-03-27T00:00:09Z",
                "meta": {
                    "type": COMPACTION_META_TYPE,
                    "compacted_until": "2026-03-27T00:00:08Z",
                    "retained_head_until_index": 1,
                    "retained_tail_from_index": 4
                }
            }),
            json!({
                "role": "user",
                "content": "middle question",
                "timestamp": "2026-03-27T00:00:10Z"
            }),
            json!({
                "role": "assistant",
                "content": "middle answer",
                "timestamp": "2026-03-27T00:00:11Z"
            }),
            json!({
                "role": "user",
                "content": "latest tail question",
                "timestamp": "2026-03-27T00:00:12Z"
            }),
            json!({
                "role": "assistant",
                "content": "latest tail answer",
                "timestamp": "2026-03-27T00:00:13Z"
            }),
            json!({
                "role": "user",
                "content": "summary-2",
                "timestamp": "2026-03-27T00:00:14Z",
                "meta": {
                    "type": COMPACTION_META_TYPE,
                    "compacted_until": "2026-03-27T00:00:13Z",
                    "retained_head_until_index": 1,
                    "retained_tail_from_index": 6
                }
            }),
            json!({
                "role": "user",
                "content": "current question",
                "timestamp": "2026-03-27T00:00:15Z"
            }),
        ];

        let filtered = filter_history_items(&history);
        let kept_contents = filtered
            .head_items
            .iter()
            .chain(filtered.tail_items.iter())
            .map(|item| item["content"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();

        assert_eq!(filtered.summary_index, 13);
        assert_eq!(
            kept_contents,
            vec![
                "head question".to_string(),
                "head answer".to_string(),
                "middle question".to_string(),
                "middle answer".to_string(),
                "latest tail question".to_string(),
                "latest tail answer".to_string(),
                "current question".to_string(),
            ]
        );
        assert!(kept_contents
            .iter()
            .all(|content| !content.contains("dropped") && !content.contains("old tail")));
    }

    #[test]
    fn build_compaction_candidates_merge_retained_head_and_tail_without_summary() {
        let history = vec![
            json!({
                "role": "user",
                "content": "head question",
                "timestamp": "2026-03-27T00:00:01Z"
            }),
            json!({
                "role": "assistant",
                "content": "head answer",
                "timestamp": "2026-03-27T00:00:02Z"
            }),
            json!({
                "role": "user",
                "content": "middle question",
                "timestamp": "2026-03-27T00:00:03Z"
            }),
            json!({
                "role": "assistant",
                "content": "middle answer",
                "timestamp": "2026-03-27T00:00:04Z"
            }),
            json!({
                "role": "user",
                "content": "tail question",
                "timestamp": "2026-03-27T00:00:05Z"
            }),
            json!({
                "role": "assistant",
                "content": "tail answer",
                "timestamp": "2026-03-27T00:00:06Z"
            }),
            json!({
                "role": "user",
                "content": "summary",
                "timestamp": "2026-03-27T00:00:07Z",
                "meta": {
                    "type": COMPACTION_META_TYPE,
                    "compacted_until": "2026-03-27T00:00:06Z",
                    "retained_head_until_index": 1,
                    "retained_tail_from_index": 4
                }
            }),
            json!({
                "role": "user",
                "content": "current question",
                "timestamp": "2026-03-27T00:00:08Z"
            }),
        ];

        let (_, messages) = HistoryManager::build_compaction_candidates(&history);
        let contents = messages
            .iter()
            .map(|item| item["content"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            contents,
            vec![
                "head question".to_string(),
                "head answer".to_string(),
                "tail question".to_string(),
                "tail answer".to_string(),
                "current question".to_string(),
            ]
        );
    }

    #[test]
    fn load_history_messages_orders_retained_head_then_summary_then_tail() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("history-retained-head-tail.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = dir.path().join("workspaces");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &HashMap::new(),
        ));
        let user_id = "history-test-user";
        let session_id = "history-test-session";
        let history = vec![
            json!({
                "role": "user",
                "content": "head question",
                "timestamp": "2026-03-27T00:00:01Z"
            }),
            json!({
                "role": "assistant",
                "content": "head answer",
                "timestamp": "2026-03-27T00:00:02Z"
            }),
            json!({
                "role": "user",
                "content": "middle question",
                "timestamp": "2026-03-27T00:00:03Z"
            }),
            json!({
                "role": "assistant",
                "content": "middle answer",
                "timestamp": "2026-03-27T00:00:04Z"
            }),
            json!({
                "role": "user",
                "content": "tail question",
                "timestamp": "2026-03-27T00:00:05Z"
            }),
            json!({
                "role": "assistant",
                "content": "tail answer",
                "timestamp": "2026-03-27T00:00:06Z"
            }),
            json!({
                "role": "user",
                "content": "summary body",
                "timestamp": "2026-03-27T00:00:07Z",
                "meta": {
                    "type": COMPACTION_META_TYPE,
                    "compacted_until": "2026-03-27T00:00:06Z",
                    "retained_head_until_index": 1,
                    "retained_tail_from_index": 4
                }
            }),
            json!({
                "role": "user",
                "content": "current question",
                "timestamp": "2026-03-27T00:00:08Z"
            }),
        ];
        for item in history {
            let payload = json!({
                "role": item.get("role").cloned().unwrap_or(Value::Null),
                "content": item.get("content").cloned().unwrap_or(Value::Null),
                "timestamp": item.get("timestamp").cloned().unwrap_or(Value::Null),
                "meta": item.get("meta").cloned().unwrap_or(Value::Null),
                "session_id": session_id,
            });
            workspace
                .append_chat(user_id, &payload)
                .expect("append history");
        }
        Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime")
            .block_on(workspace.clone().flush_writes_async());

        let messages = HistoryManager.load_history_messages(&workspace, user_id, session_id, 0);
        let contents = messages
            .iter()
            .map(|item| item["content"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();
        assert_eq!(contents.len(), 6);
        assert_eq!(contents[0], "head question");
        assert_eq!(contents[1], "head answer");
        assert!(contents[2].contains("summary body"));
        assert_eq!(contents[3], "tail question");
        assert_eq!(contents[4], "tail answer");
        assert_eq!(contents[5], "current question");
    }
}
