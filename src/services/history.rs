// 历史管理：加载对话历史、压缩摘要与产物索引。
use crate::config::LlmModelConfig;
use crate::i18n;
use crate::orchestrator_constants::{
    ARTIFACT_INDEX_MAX_ITEMS, COMPACTION_META_TYPE, COMPACTION_OUTPUT_RESERVE, COMPACTION_RATIO,
    COMPACTION_SAFETY_MARGIN, OBSERVATION_PREFIX,
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
        let (filtered_items, summary_item, _, _) = filter_history_items(&history);
        let mut messages = Vec::new();
        if let Some(summary_item) = summary_item {
            let summary_content = Self::format_compaction_summary(
                summary_item
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            );
            messages.push(json!({ "role": "user", "content": summary_content }));
        }
        for item in filtered_items {
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
        let (filtered_items, _, _, _) = filter_history_items(history);
        let mut items = Vec::new();
        let mut messages = Vec::new();
        for item in filtered_items {
            if let Some(message) = build_message_from_item(&item, true) {
                items.push(item);
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

fn filter_history_items(history: &[Value]) -> (Vec<Value>, Option<Value>, Option<f64>, i64) {
    let mut summary_index: i64 = -1;
    let mut summary_item: Option<Value> = None;
    for (index, item) in history.iter().enumerate() {
        if HistoryManager::is_compaction_summary_item(item) {
            summary_index = index as i64;
            summary_item = Some(item.clone());
        }
    }
    let compacted_until_ts = extract_compacted_until_ts(summary_item.as_ref());
    let mut filtered = Vec::new();
    let mut skip_next_assistant = false;
    for (index, item) in history.iter().enumerate() {
        if HistoryManager::is_compaction_summary_item(item) {
            continue;
        }
        let role = item.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "system" {
            continue;
        }
        if let Some(boundary) = compacted_until_ts {
            let item_ts = parse_timestamp(item.get("timestamp"));
            if item_ts.is_none() && summary_index >= 0 && index as i64 <= summary_index {
                continue;
            }
            if let Some(item_ts) = item_ts {
                if item_ts <= boundary {
                    continue;
                }
            }
        } else if summary_index >= 0 && index as i64 <= summary_index {
            continue;
        }
        if skip_next_assistant {
            if role == "assistant" {
                skip_next_assistant = false;
                continue;
            }
            skip_next_assistant = false;
        }
        if role == "assistant" && is_tool_call_item(item) && !has_tool_calls_payload(item) {
            skip_next_assistant = true;
        }
        filtered.push(item.clone());
    }
    (filtered, summary_item, compacted_until_ts, summary_index)
}

fn extract_compacted_until_ts(item: Option<&Value>) -> Option<f64> {
    let meta = item?.get("meta")?.as_object()?;
    let raw = meta
        .get("compacted_until_ts")
        .or_else(|| meta.get("compacted_until"));
    parse_timestamp(raw)
}
