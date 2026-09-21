//! Local, pressure-triggered reduction of old reproducible observations.
use super::context::model_context_entries_from_messages;
use super::memory_support::{parse_compaction_observation_payload, CompactionResult};
use super::*;

const KEEP_RECENT_GROUPS: usize = 5;
const MIN_SAVINGS: i64 = 256;
const PREVIEW_BYTES: usize = 768;

struct MicrocompactionPlan {
    replacements: Vec<(usize, String)>,
    tokens_before: i64,
    tokens_saved: i64,
}

fn protected_payload(value: &Value, depth: usize) -> bool {
    if depth > 8 {
        return true;
    }
    match value {
        Value::Object(map) => {
            map.get("ok").and_then(Value::as_bool) == Some(false)
                || map.get("isError").and_then(Value::as_bool) == Some(true)
                || ["has_more", "continuation_required"]
                    .iter()
                    .any(|key| map.get(*key).and_then(Value::as_bool) == Some(true))
                || [
                    "image_url",
                    "image",
                    "audio",
                    "video",
                    "resource",
                    "next_cursor",
                    "next_page_token",
                    "next_offset",
                    "next_token",
                    "next_url",
                    "continuation_token",
                    "resume_token",
                ]
                .iter()
                .any(|key| map.get(*key).is_some_and(|value| !value.is_null()))
                || map.get("type").and_then(Value::as_str).is_some_and(|kind| {
                    matches!(
                        kind,
                        "image" | "image_url" | "audio" | "video" | "resource" | "file"
                    )
                })
                || map
                    .values()
                    .any(|nested| protected_payload(nested, depth + 1))
        }
        Value::Array(items) => items.iter().any(|item| protected_payload(item, depth + 1)),
        _ => false,
    }
}

fn compact_old_observation(content: &str) -> Option<String> {
    let mut payload = parse_compaction_observation_payload(&Value::String(content.to_string()))?;
    if payload.get("ok").and_then(Value::as_bool) != Some(true)
        || payload.get("microcompacted").and_then(Value::as_bool) == Some(true)
        || protected_payload(&payload, 0)
    {
        return None;
    }
    let tool = payload.get("tool")?.as_str()?;
    let canonical = resolve_tool_name(tool);
    // Only reproducible reads are eligible. Commands, skills, writes and external
    // tools may carry unique state or instructions and must survive unchanged.
    if !matches!(
        canonical.as_str(),
        "读取文件" | "read_file" | "搜索内容" | "search_content" | "列出文件" | "list_files"
    ) {
        return None;
    }
    let data = payload.get("data")?;
    let source = serde_json::to_string(data).ok()?;
    if source.len() <= PREVIEW_BYTES * 2 {
        return None;
    }
    let mut reduced = Map::new();
    for key in [
        "path",
        "files",
        "query",
        "returned_match_count",
        "matched_file_count",
        "truncated",
        "read_output_omitted_bytes",
    ] {
        if let Some(value) = data.get(key) {
            reduced.insert(key.to_string(), value.clone());
        }
    }
    let mut head = PREVIEW_BYTES / 2;
    while !source.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = source.len() - PREVIEW_BYTES / 2;
    while !source.is_char_boundary(tail) {
        tail += 1;
    }
    reduced.insert(
        "preview".to_string(),
        json!(format!(
            "{}\n[older output omitted; repeat the read if needed]\n{}",
            &source[..head],
            &source[tail..]
        )),
    );
    payload["data"] = Value::Object(reduced);
    payload["microcompacted"] = json!(true);
    let prefix = if content.starts_with(OBSERVATION_PREFIX) {
        OBSERVATION_PREFIX
    } else {
        ""
    };
    let replacement = format!("{prefix}{payload}");
    (replacement.len() < content.len()).then_some(replacement)
}

fn plan_microcompaction(
    messages: &[Value],
    observed_tokens: i64,
    trigger: i64,
) -> Option<MicrocompactionPlan> {
    if observed_tokens <= 0 || trigger <= 0 || observed_tokens < trigger {
        return None;
    }
    let tokens_before = estimate_messages_tokens(messages);
    // Estimates select a local reduction, but never replace provider usage in
    // billing or context_usage events. Leave hysteresis for the next response.
    let target = trigger.saturating_mul(4) / 5;
    let required = (observed_tokens - target)
        .max(tokens_before - target)
        .max(MIN_SAVINGS);
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        if matches!(role, "assistant" | "system" | "user")
            && !Orchestrator::is_observation_message(role, &message["content"])
            && !group.is_empty()
        {
            groups.push(std::mem::take(&mut group));
        }
        if role == "tool" || Orchestrator::is_observation_message(role, &message["content"]) {
            group.push(index);
        }
    }
    if !group.is_empty() {
        groups.push(group);
    }
    let mut replacements = Vec::new();
    let mut tokens_saved = 0;
    for group in groups
        .iter()
        .take(groups.len().saturating_sub(KEEP_RECENT_GROUPS))
    {
        for &index in group {
            let Some(content) = messages[index]["content"].as_str() else {
                continue;
            };
            let Some(replacement) = compact_old_observation(content) else {
                continue;
            };
            tokens_saved += (approx_token_count(content) - approx_token_count(&replacement)).max(0);
            replacements.push((index, replacement));
        }
        // Commit whole tool-result groups. Tool call IDs and ordering never change.
        let calibrated_savings = if tokens_before > observed_tokens {
            tokens_saved.saturating_mul(observed_tokens) / tokens_before.max(1)
        } else {
            tokens_saved
        };
        if calibrated_savings >= required {
            return Some(MicrocompactionPlan {
                replacements,
                tokens_before,
                tokens_saved,
            });
        }
    }
    None
}

impl Orchestrator {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn try_microcompact_messages(
        &self,
        messages: &[Value],
        user_id: &str,
        session_id: &str,
        emitter: &EventEmitter,
        round_info: RoundInfo,
        observed_tokens: i64,
        trigger: i64,
    ) -> Option<CompactionResult> {
        let plan = plan_microcompaction(messages, observed_tokens, trigger)?;
        let mut reduced = messages.to_vec();
        for (index, content) in &plan.replacements {
            reduced[*index]["content"] = json!(content);
        }
        let entries = model_context_entries_from_messages(&reduced);
        if let Err(error) = self
            .workspace
            .replace_model_context_entries(user_id, session_id, &entries)
        {
            warn!(%session_id, %error, "local context reduction could not be persisted");
            return None;
        }
        if !self.workspace.flush_writes_async().await {
            warn!(%session_id, "local context reduction flush failed; using summary fallback");
            return None;
        }
        // The shared write queue's barrier acknowledges draining, not SQL success.
        // Verify the replacement before declaring the current user persisted.
        let workspace = Arc::clone(&self.workspace);
        let persisted_user_id = user_id.to_string();
        let persisted_session_id = session_id.to_string();
        let verified = crate::core::blocking::run_db("context.microcompaction.verify", move || {
            Ok(workspace.load_model_context_entries(
                &persisted_user_id,
                &persisted_session_id,
                0,
            )? == entries)
        })
        .await;
        if !matches!(verified, Ok(true)) {
            warn!(%session_id, "local context replacement verification failed; using summary fallback");
            return None;
        }
        let mut payload = json!({
            "stage": "microcompaction", "strategy": "old_tool_preview",
            "observed_context_tokens": observed_tokens,
            "estimated_tokens_before": plan.tokens_before,
            "estimated_tokens_after": plan.tokens_before - plan.tokens_saved,
            "estimated_tokens_saved": plan.tokens_saved,
            "reduced_messages": plan.replacements.len(), "kept_recent_tool_groups": KEEP_RECENT_GROUPS,
        });
        if let Some(map) = payload.as_object_mut() {
            round_info.insert_into(map);
        }
        emitter.emit("progress", payload).await;
        Some(CompactionResult {
            messages: reduced,
            compaction_id: None,
            model_context_replaced: true,
        })
    }
}

#[cfg(test)]
mod tests;
