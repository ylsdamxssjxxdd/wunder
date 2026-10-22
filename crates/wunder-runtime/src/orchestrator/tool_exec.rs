use super::context::{normalize_model_context_message, MODEL_CONTEXT_INTERNAL_META_TYPE};
use super::*;

pub(super) const TOOL_TIMEOUT_ERROR: &str = "tool_timeout";

use super::tool_result_payload::{
    estimate_tool_result_chars, extract_exit_code, merge_tool_result_meta,
};

const TOOL_RESULT_TRANSPORT_MAX_BYTES: usize = 1_048_576;
const TOOL_RESULT_TRANSPORT_PREVIEW_BYTES: usize = 32_768;
const TOOL_RESULT_TRANSPORT_ERROR_BYTES: usize = 8_192;
const TOOL_RESULT_TRANSPORT_CONTROL_BYTES: usize = 65_536;
const TOOL_RESULT_TRANSPORT_CONTROL_FIELD_BYTES: usize = 4_096;

/// Counts the entire serialized result while retaining fixed-size UTF-8 ends.
/// The provider value already exists, but this guard never allocates a second
/// full serialization of it.
struct ToolResultTransportWriter {
    total_bytes: usize,
    head: Vec<u8>,
    head_limit: usize,
    tail: Vec<u8>,
    tail_next: usize,
    tail_len: usize,
}

impl ToolResultTransportWriter {
    fn new() -> Self {
        let visible =
            TOOL_RESULT_TRANSPORT_PREVIEW_BYTES.saturating_sub(TOOL_RESULT_TRUNCATION_MARKER.len());
        Self {
            total_bytes: 0,
            head: Vec::with_capacity(visible / 2),
            head_limit: visible / 2,
            tail: vec![0; visible - visible / 2],
            tail_next: 0,
            tail_len: 0,
        }
    }

    fn into_preview(self) -> (String, usize) {
        let head_end = std::str::from_utf8(&self.head)
            .map(str::len)
            .unwrap_or_else(|err| err.valid_up_to());
        let mut tail = Vec::with_capacity(self.tail_len);
        if self.tail_len == self.tail.len() {
            tail.extend_from_slice(&self.tail[self.tail_next..]);
            tail.extend_from_slice(&self.tail[..self.tail_next]);
        } else {
            tail.extend_from_slice(&self.tail[..self.tail_len]);
        }
        // A retained end can start inside a UTF-8 scalar, never more than
        // three bytes from its next valid boundary.
        let tail_start = (0..=tail.len().min(3))
            .find(|start| std::str::from_utf8(&tail[*start..]).is_ok())
            .unwrap_or(tail.len());
        let retained_bytes = head_end + tail.len().saturating_sub(tail_start);
        let mut preview =
            String::with_capacity(retained_bytes + TOOL_RESULT_TRUNCATION_MARKER.len());
        preview.push_str(std::str::from_utf8(&self.head[..head_end]).unwrap_or(""));
        preview.push_str(TOOL_RESULT_TRUNCATION_MARKER);
        preview.push_str(std::str::from_utf8(&tail[tail_start..]).unwrap_or(""));
        (preview, self.total_bytes.saturating_sub(retained_bytes))
    }
}

impl std::io::Write for ToolResultTransportWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len());
        let head_remaining = self.head_limit.saturating_sub(self.head.len());
        self.head
            .extend_from_slice(&bytes[..head_remaining.min(bytes.len())]);
        let capacity = self.tail.len();
        if bytes.len() >= capacity {
            self.tail.copy_from_slice(&bytes[bytes.len() - capacity..]);
            self.tail_next = 0;
            self.tail_len = capacity;
        } else if !bytes.is_empty() {
            let first = bytes.len().min(capacity - self.tail_next);
            self.tail[self.tail_next..self.tail_next + first].copy_from_slice(&bytes[..first]);
            self.tail[..bytes.len() - first].copy_from_slice(&bytes[first..]);
            self.tail_next = (self.tail_next + bytes.len()) % capacity;
            self.tail_len = self.tail_len.saturating_add(bytes.len()).min(capacity);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Stop measuring optional control fields once their small copy budget is
/// exceeded; opaque tokens are either copied exactly or omitted entirely.
struct ToolResultControlCounter {
    bytes: usize,
    limit: usize,
}

impl std::io::Write for ToolResultControlCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "control field limit",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn copy_transport_control_fields(
    source: &Value,
    target: &mut Map<String, Value>,
    keys: &[&str],
    remaining: &mut usize,
) {
    for key in keys {
        let Some(value) = source.get(*key) else {
            continue;
        };
        let overhead = key.len().saturating_add(4);
        let limit =
            TOOL_RESULT_TRANSPORT_CONTROL_FIELD_BYTES.min(remaining.saturating_sub(overhead));
        let mut counter = ToolResultControlCounter { bytes: 0, limit };
        if serde_json::to_writer(&mut counter, value).is_ok() {
            *remaining = remaining.saturating_sub(counter.bytes.saturating_add(overhead));
            target.insert((*key).to_string(), value.clone());
        }
    }
}

/// Provider results normally stay intact for events/audit and are compacted
/// only in the model observation. This guard bounds the whole data/error/meta
/// envelope before it is cloned into events. The execution path preserves
/// oversized output first; storage failures retain an explicit loss marker.
pub(super) fn tool_result_exceeds_transport_limit(result: &ToolResultPayload) -> bool {
    let mut probe = ToolResultControlCounter {
        bytes: 0,
        // Finalization appends only bounded scalar metadata. Preserve just
        // below the transport edge too, before those fields can trigger it.
        limit: TOOL_RESULT_TRANSPORT_MAX_BYTES.saturating_sub(1024),
    };
    serde_json::to_writer(&mut probe, result).is_err()
}

fn enforce_tool_result_transport_guard(result: &mut ToolResultPayload) -> Option<usize> {
    #[derive(serde::Serialize)]
    struct TransportEnvelope<'a> {
        ok: bool,
        sandbox: bool,
        data: &'a Value,
        error: &'a str,
        meta: Option<&'a Value>,
    }
    let envelope = TransportEnvelope {
        ok: result.ok,
        sandbox: result.sandbox,
        data: &result.data,
        error: &result.error,
        meta: result.meta.as_ref(),
    };
    // Normal results need no preview buffers. Stop the allocation-free probe
    // at the limit, then retain ends only for an oversized envelope.
    let mut probe = ToolResultControlCounter {
        bytes: 0,
        limit: TOOL_RESULT_TRANSPORT_MAX_BYTES,
    };
    if serde_json::to_writer(&mut probe, &envelope).is_ok() {
        return None;
    }
    let mut writer = ToolResultTransportWriter::new();
    serde_json::to_writer(&mut writer, &envelope).ok()?;
    let original_bytes = writer.total_bytes;
    let (preview, omitted_bytes) = writer.into_preview();
    let exit_code = extract_exit_code(&result.data);
    let mut remaining = TOOL_RESULT_TRANSPORT_CONTROL_BYTES * 3 / 4;
    let mut data = Map::new();
    copy_transport_control_fields(
        &result.data,
        &mut data,
        &[
            "exit_code",
            "returncode",
            "session_id",
            "command_session_id",
            "status",
            "running",
            "completed_steps",
            "failed_step",
            "action",
            "kind",
            "target_id",
            "path",
            "bytes",
            "changed",
            "changed_files",
            "hunks_applied",
            "changed_paths",
            "workspace_changed_paths",
            "artifact",
            "screenshot_artifact",
            "artifact_id",
            "query_handle",
            "next_cursor",
            "cursor",
            "next_offset",
            "next_page_token",
            "page_token",
            "continuation_token",
            "next_token",
            "next_url",
            "resume_token",
            "has_more",
            "__wrapup",
        ],
        &mut remaining,
    );
    if let Some(exit_code) = exit_code {
        data.insert("exit_code".to_string(), json!(exit_code));
    }
    // Command result rows carry session identity and completion state separately
    // from their large stdout/stderr. Preserve a bounded ordered control list.
    if let Some(rows) = result.data.get("results").and_then(Value::as_array) {
        let mut controls = Vec::new();
        for row in rows.iter().take(32) {
            let mut control = Map::new();
            copy_transport_control_fields(
                row,
                &mut control,
                &[
                    "command",
                    "exit_code",
                    "returncode",
                    "session_id",
                    "command_session_id",
                    "status",
                    "running",
                    "next_offset",
                ],
                &mut remaining,
            );
            if !control.is_empty() {
                controls.push(Value::Object(control));
            }
        }
        if !controls.is_empty() {
            data.insert("results".to_string(), Value::Array(controls));
        }
    }
    for key in ["content", "content_blocks"] {
        let Some(blocks) = result.data.get(key).and_then(Value::as_array) else {
            continue;
        };
        let mut attachments = Vec::new();
        for block in blocks
            .iter()
            .take(8)
            .filter(|block| block.get("attachment").is_some())
        {
            let mut control = Map::new();
            copy_transport_control_fields(
                block,
                &mut control,
                &["type", "attachment"],
                &mut remaining,
            );
            if control.contains_key("attachment") {
                attachments.push(Value::Object(control));
            }
        }
        if !attachments.is_empty() {
            data.insert(key.to_string(), Value::Array(attachments));
        }
    }
    data.insert("truncated".to_string(), Value::Bool(true));
    data.insert("truncation_scope".to_string(), json!("transport_safety"));
    let saved = super::tool_result_payload::saved_result_metadata(result);
    let recoverable = saved.is_some();
    data.insert("recoverable".to_string(), Value::Bool(recoverable));
    data.insert("original_bytes".to_string(), json!(original_bytes));
    data.insert("omitted_bytes".to_string(), json!(omitted_bytes));
    data.insert("preview".to_string(), Value::String(preview));
    if let Some(saved) = &saved {
        data.insert("result_handle".to_string(), saved["handle"].clone());
        data.insert(
            "recovery_hint".to_string(),
            json!("read_file: path=result_handle; offset=0; limit=1024"),
        );
    } else {
        data.insert("recovery_hint".to_string(), json!(
            "Full result was not stored. Repeat a smaller range or narrower query; existing continuation tokens only address later producer pages."
        ));
    }
    result.data = Value::Object(data);
    result.error = truncate_utf8_preview(&result.error, TOOL_RESULT_TRANSPORT_ERROR_BYTES);
    let mut meta = Map::new();
    let mut remaining = TOOL_RESULT_TRANSPORT_CONTROL_BYTES / 4;
    if let Some(source) = &result.meta {
        copy_transport_control_fields(
            source,
            &mut meta,
            &[
                "duration_ms",
                "output_chars",
                "exit_code",
                "error_code",
                "error_retryable",
                "error_detail_head",
                "normalized_transport_ok",
                "normalized_business_ok",
                "normalized_final_ok",
                "business_error_source",
                "workspace_version",
                "workspace_changed",
                "workspace_changed_paths",
                "turn_control",
                "parallel_execution",
            ],
            &mut remaining,
        );
    }
    if let Some(exit_code) = exit_code {
        meta.insert("exit_code".to_string(), json!(exit_code));
    }
    if let Some(saved) = saved {
        meta.insert("saved_result".to_string(), saved);
    }
    if result
        .meta
        .as_ref()
        .and_then(|v| v.get("result_store_failed"))
        == Some(&Value::Bool(true))
    {
        meta.insert("result_store_failed".to_string(), Value::Bool(true));
    }
    meta.insert("transport_truncated".to_string(), Value::Bool(true));
    meta.insert(
        "transport_recoverable".to_string(),
        Value::Bool(recoverable),
    );
    meta.insert("original_output_bytes".to_string(), json!(original_bytes));
    result.meta = Some(Value::Object(meta));
    Some(original_bytes)
}

fn truncate_utf8_preview(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let marker = TOOL_RESULT_TRUNCATION_MARKER;
    if max_bytes <= marker.len() {
        return String::new();
    }
    let visible_bytes = max_bytes.saturating_sub(marker.len());
    let mut head_end = (visible_bytes / 2).min(value.len());
    while head_end > 0 && !value.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = value
        .len()
        .saturating_sub(visible_bytes.saturating_sub(head_end));
    while tail_start < value.len() && !value.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    format!("{}{}{}", &value[..head_end], marker, &value[tail_start..])
}

fn canonical_model_message(payload: &Value) -> Option<Value> {
    if payload
        .pointer("/meta/replacement_history")
        .is_some_and(Value::is_array)
    {
        return Some(payload.clone());
    }
    crate::services::history::build_message_from_item(payload, true)
        .and_then(normalize_model_context_message)
}

impl Orchestrator {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn append_chat(
        &self,
        user_id: &str,
        session_id: &str,
        role: &str,
        content: Option<&Value>,
        attachments: Option<&[AttachmentPayload]>,
        meta: Option<&Value>,
        reasoning: Option<&str>,
        tool_calls: Option<&Value>,
        tool_call_id: Option<&str>,
        model_content: Option<&Value>,
        round_info: RoundInfo,
    ) {
        if let Err(err) = self
            .try_append_chat(
                user_id,
                session_id,
                role,
                content,
                attachments,
                meta,
                reasoning,
                tool_calls,
                tool_call_id,
                model_content,
                round_info,
            )
            .await
        {
            warn!("append chat failed for session {session_id} role {role}: {err}");
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn try_append_chat(
        &self,
        user_id: &str,
        session_id: &str,
        role: &str,
        content: Option<&Value>,
        attachments: Option<&[AttachmentPayload]>,
        meta: Option<&Value>,
        reasoning: Option<&str>,
        tool_calls: Option<&Value>,
        tool_call_id: Option<&str>,
        model_content: Option<&Value>,
        round_info: RoundInfo,
    ) -> Result<()> {
        let timestamp = Local::now().to_rfc3339();
        let content_value = content
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()));
        let content_value = match content_value {
            Value::String(_) | Value::Array(_) | Value::Object(_) => content_value,
            Value::Null => Value::String(String::new()),
            other => Value::String(other.to_string()),
        };
        let mut payload = json!({
            "role": role,
            "content": content_value,
            "session_id": session_id,
            "timestamp": timestamp,
        });
        let has_round_info = round_info.user_round.is_some() || round_info.model_round.is_some();
        if let Value::Object(ref mut map) = payload {
            round_info.insert_into(map);
            if has_round_info {
                map.insert(
                    "round_info_source".to_string(),
                    Value::String("orchestrator".to_string()),
                );
            }
        }
        if let Some(model_content) = model_content {
            if !model_content.is_null() {
                payload["model_content"] = model_content.clone();
            }
        }
        if let Some(reasoning) = reasoning {
            let cleaned = reasoning.trim();
            if !cleaned.is_empty() {
                payload["reasoning_content"] = Value::String(cleaned.to_string());
            }
        }
        if let Some(meta) = meta {
            if !meta.is_null() {
                payload["meta"] = meta.clone();
            }
        }
        if let Some(tool_calls) = tool_calls {
            if !tool_calls.is_null() {
                payload["tool_calls"] = tool_calls.clone();
            }
        }
        if let Some(tool_call_id) = tool_call_id {
            let cleaned = tool_call_id.trim();
            if !cleaned.is_empty() {
                payload["tool_call_id"] = Value::String(cleaned.to_string());
            }
        }
        if let Some(attachments) = attachments {
            if let Some(value) = build_history_attachments(attachments) {
                payload["attachments"] = value;
            }
        }
        let turn_id = match round_info.thread_turn_id {
            Some(id) => Some(id.to_string()),
            None => match round_info.user_round {
                Some(round) => self
                    .storage
                    .find_thread_turn_id(user_id, session_id, round)?,
                None => None,
            },
        };
        if let Some(turn_id) = turn_id {
            // Only an admitted request owns the stable input slot. Commands,
            // summaries and internal observations append independent messages.
            let input_slot = role == "user"
                && round_info.model_round.is_none()
                && round_info.thread_turn_id.is_some()
                && meta
                    .and_then(|value| value.get("type"))
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind == "subagent_hidden_user");
            // Every visible assistant write that belongs to a model round is
            // the same durable output item.  Stats are allowed to arrive in a
            // later write (and tool/question terminal paths do not all carry
            // the stats object), so identity must not depend on metadata.
            // Hidden model-context messages stay append-only and never replace
            // the user-visible stream item.
            let output_slot = role == "assistant"
                && round_info.model_round.is_some()
                && meta
                    .and_then(|value| value.get("hidden"))
                    .and_then(Value::as_bool)
                    != Some(true);
            let item_id = if input_slot {
                format!("{turn_id}:user")
            } else if output_slot {
                // A model round has exactly one visible durable output item.
                // The stream registers it as `{turn}:text-{round}` at request
                // admission; final history and message statistics must update
                // that same item instead of appending a random-id duplicate.
                format!(
                    "{turn_id}:text-{}",
                    round_info
                        .model_round
                        .expect("output slot requires a model round")
                )
            } else {
                Uuid::new_v4().to_string()
            };
            payload["turn_id"] = Value::String(turn_id);
            payload["item_id"] = Value::String(item_id);
        }
        let payload =
            crate::services::chat_payload_sanitizer::sanitize_persisted_chat_payload(&payload);
        use wunder_core::thread_store::{ThreadId, TurnId};
        let turn = payload
            .get("turn_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("model message has no admitted turn"))?;
        let Some(model_message) = canonical_model_message(&payload) else {
            return Ok(());
        };
        self.thread_store
            .append_model_message(
                &ThreadId::new(session_id)?,
                &TurnId::new(turn)?,
                model_message,
            )
            .await?;
        Ok(())
    }

    pub(super) async fn append_internal_model_context_chat(
        &self,
        user_id: &str,
        session_id: &str,
        message: &Value,
        source: &str,
        round_info: RoundInfo,
    ) {
        if let Err(err) = self
            .try_append_internal_model_context_chat(
                user_id, session_id, message, source, round_info,
            )
            .await
        {
            warn!("append internal model context chat failed for session {session_id}: {err}");
        }
    }

    pub(super) async fn try_append_internal_model_context_chat(
        &self,
        user_id: &str,
        session_id: &str,
        message: &Value,
        source: &str,
        round_info: RoundInfo,
    ) -> Result<()> {
        let Some(payload) = normalize_model_context_message(message.clone()) else {
            return Ok(());
        };
        let role = payload
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        let content = payload.get("content").cloned().unwrap_or(Value::Null);
        let mut meta = json!({
            "type": MODEL_CONTEXT_INTERNAL_META_TYPE,
            "hidden": true,
            "internal_user": role == "user",
            "source": source,
        });
        if let Value::Object(ref mut map) = meta {
            if let Some(tool_call_id) = payload.get("tool_call_id").and_then(Value::as_str) {
                map.insert(
                    "tool_call_id".to_string(),
                    Value::String(tool_call_id.to_string()),
                );
            }
        }
        self.try_append_chat(
            user_id,
            session_id,
            role,
            Some(&content),
            None,
            Some(&meta),
            payload.get("reasoning_content").and_then(Value::as_str),
            payload.get("tool_calls"),
            payload.get("tool_call_id").and_then(Value::as_str),
            None,
            round_info,
        )
        .await
    }

    pub(super) fn mark_internal_model_context_message(
        &self,
        message: &mut Value,
        source: &str,
        round_info: RoundInfo,
    ) {
        let Some(obj) = message.as_object_mut() else {
            return;
        };
        let mut meta = json!({
            "type": MODEL_CONTEXT_INTERNAL_META_TYPE,
            "hidden": true,
            "internal_user": obj.get("role").and_then(Value::as_str) == Some("user"),
            "source": source,
        });
        if let Value::Object(ref mut map) = meta {
            if let Some(tool_call_id) = obj.get("tool_call_id").and_then(Value::as_str) {
                map.insert(
                    "tool_call_id".to_string(),
                    Value::String(tool_call_id.to_string()),
                );
            }
            round_info.insert_into(map);
        }
        obj.insert("meta".to_string(), meta);
    }

    pub(super) fn append_tool_log(
        &self,
        user_id: &str,
        session_id: &str,
        tool_name: &str,
        args: &Value,
        result: &ToolResultPayload,
        round_info: RoundInfo,
        tool_call_id: Option<&str>,
        request_context_tokens: Option<i64>,
        request_usage: Option<&TokenUsage>,
        schema_reference: Option<&crate::tools::ToolSchemaReference>,
    ) {
        // Completed tool bodies already passed through the canonical writer.
        // Diagnostics retain numeric facts only, never another copy of input,
        // output, tool schemas or arbitrary metadata.
        let _ = (args, schema_reference);
        let mut payload = json!({
            "tool": tool_name,
            "session_id": session_id,
            "ok": result.ok,
            "meta": {},
        });
        for key in ["duration_ms", "output_chars", "exit_code"] {
            if let Some(value) = result
                .meta
                .as_ref()
                .and_then(|meta| meta.get(key))
                .filter(|value| value.is_number())
            {
                payload["meta"][key] = value.clone();
            }
        }
        if result.data.get("source").and_then(Value::as_str) == Some("skill_read") {
            payload["source"] = json!("skill_read");
        }
        if let Some(tokens) = request_context_tokens {
            payload["request_context_tokens"] = json!(tokens);
        }
        if let Some(usage) = request_usage {
            payload["request_usage"] = serde_json::to_value(usage).unwrap_or(Value::Null);
        }
        if let Some(id) = tool_call_id {
            payload["tool_call_id"] = json!(id);
        }
        round_info.insert_into(payload.as_object_mut().expect("tool metrics object"));
        if let Err(err) = self.storage.append_tool_log(user_id, &payload) {
            warn!("append tool metrics failed for session {session_id} tool {tool_name}: {err}");
        }
    }

    pub(super) fn finalize_tool_result(
        &self,
        mut result: ToolResultPayload,
        started_at: Instant,
    ) -> ToolResultPayload {
        let duration_ms = started_at.elapsed().as_millis() as i64;
        let exit_code = extract_exit_code(&result.data);
        let mut meta = merge_tool_result_meta(result.meta.take());
        meta.insert("duration_ms".to_string(), json!(duration_ms));
        meta.insert(
            "output_chars".to_string(),
            json!(estimate_tool_result_chars(&result.data)),
        );
        if let Some(exit_code) = exit_code {
            meta.insert("exit_code".to_string(), json!(exit_code));
        }
        if meta.is_empty() {
            result.meta = None;
        } else {
            result.meta = Some(Value::Object(meta));
        }
        enforce_tool_result_transport_guard(&mut result);
        result
    }

    pub(super) fn append_artifact_logs(
        &self,
        user_id: &str,
        session_id: &str,
        tool_name: &str,
        args: &Value,
        result: &ToolResultPayload,
    ) {
        let entries = self.build_artifact_entries(tool_name, args, result);
        if entries.is_empty() {
            return;
        }
        let timestamp = Local::now().to_rfc3339();
        for mut entry in entries {
            if let Value::Object(ref mut map) = entry {
                map.entry("tool".to_string())
                    .or_insert_with(|| Value::String(tool_name.to_string()));
                map.entry("ok".to_string())
                    .or_insert_with(|| Value::Bool(result.ok));
                if !result.error.trim().is_empty() {
                    map.entry("error".to_string())
                        .or_insert_with(|| Value::String(result.error.clone()));
                }
                map.insert(
                    "session_id".to_string(),
                    Value::String(session_id.to_string()),
                );
                map.insert("timestamp".to_string(), Value::String(timestamp.clone()));
            }
            if let Err(err) = self.storage.append_artifact_log(user_id, &entry) {
                warn!(
                    "append artifact log failed for session {session_id} tool {tool_name}: {err}"
                );
            }
        }
    }

    pub(super) fn build_artifact_entries(
        &self,
        tool_name: &str,
        args: &Value,
        result: &ToolResultPayload,
    ) -> Vec<Value> {
        let mut entries = Vec::new();
        let file_actions = HashMap::from([
            ("读取文件", "read"),
            ("写入文件", "write"),
            ("文本编辑", "edit"),
            ("编辑", "edit"),
        ]);
        if let Some(action) = file_actions.get(tool_name) {
            let paths = extract_file_paths(args);
            for path in paths {
                let mut meta = serde_json::Map::new();
                if let Value::Object(data) = &result.data {
                    if *action == "write" {
                        if let Some(value) = data.get("bytes") {
                            meta.insert("bytes".to_string(), value.clone());
                        }
                    } else if *action == "patch" {
                        if let Some(value) = data.get("changed_files") {
                            meta.insert("changed_files".to_string(), value.clone());
                        }
                        if let Some(value) = data.get("hunks_applied") {
                            meta.insert("hunks_applied".to_string(), value.clone());
                        }
                    }
                }
                entries.push(json!({
                    "kind": "file",
                    "action": action,
                    "name": path,
                    "meta": Value::Object(meta),
                }));
            }
            return entries;
        }

        if tool_name == "执行命令" {
            let commands = extract_command_lines(args);
            let mut returncode_map = HashMap::new();
            let mut fallback_rc: Option<Value> = None;
            if let Value::Object(data) = &result.data {
                if let Some(Value::Array(items)) = data.get("results") {
                    for item in items {
                        let Some(obj) = item.as_object() else {
                            continue;
                        };
                        let command = obj
                            .get("command")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if command.is_empty() {
                            continue;
                        }
                        returncode_map.insert(
                            command,
                            obj.get("returncode").cloned().unwrap_or(Value::Null),
                        );
                    }
                }
                if data.contains_key("returncode") {
                    fallback_rc = data.get("returncode").cloned();
                }
            }
            for command in commands {
                let returncode = returncode_map
                    .get(&command)
                    .cloned()
                    .or_else(|| fallback_rc.clone());
                let ok = match returncode.as_ref().and_then(Value::as_i64) {
                    Some(code) => code == 0,
                    None => result.ok,
                };
                entries.push(json!({
                    "kind": "command",
                    "action": "execute",
                    "name": command,
                    "ok": ok,
                    "meta": { "returncode": returncode.unwrap_or(Value::Null) },
                }));
            }
            return entries;
        }

        if tool_name == "ptc" {
            let mut script_path = String::new();
            if let Value::Object(data) = &result.data {
                script_path = data
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
            if script_path.is_empty() {
                script_path = args
                    .get("filename")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
            if script_path.is_empty() {
                return entries;
            }
            let returncode = match &result.data {
                Value::Object(data) => data.get("returncode").cloned(),
                _ => None,
            };
            let ok = match returncode.as_ref().and_then(Value::as_i64) {
                Some(code) => code == 0,
                None => result.ok,
            };
            entries.push(json!({
                "kind": "script",
                "action": "run",
                "name": script_path,
                "ok": ok,
                "meta": { "returncode": returncode.unwrap_or(Value::Null) }
            }));
            return entries;
        }

        entries
    }

    pub(super) fn append_skill_usage_logs(
        &self,
        user_id: &str,
        session_id: &str,
        args: &Value,
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        round_info: RoundInfo,
    ) {
        let paths = extract_file_paths(args);
        if paths.is_empty() {
            return;
        }
        let mut specs = skills.list_specs();
        if let Some(bindings) = user_tool_bindings {
            if !bindings.skill_specs.is_empty() {
                specs.extend(bindings.skill_specs.iter().cloned());
            }
        }
        if specs.is_empty() {
            return;
        }

        let mut seen_names = HashSet::new();
        let mut path_map: HashMap<PathBuf, (String, crate::schemas::ToolSpec)> = HashMap::new();
        for spec in specs {
            let name = spec.name.trim();
            if name.is_empty() {
                continue;
            }
            if !seen_names.insert(name.to_string()) {
                continue;
            }
            let Some(spec_path) = resolve_absolute_path(&spec.path) else {
                continue;
            };
            let key = normalize_compare_path(&spec_path);
            let tool_spec = crate::schemas::ToolSpec {
                name: name.to_string(),
                title: None,
                description: spec.description,
                input_schema: spec.input_schema,
            };
            path_map.insert(key, (name.to_string(), tool_spec));
        }
        if path_map.is_empty() {
            return;
        }

        let mut matched = HashMap::new();
        for raw in paths {
            let Some(candidate) = resolve_absolute_path(&raw) else {
                continue;
            };
            let key = normalize_compare_path(&candidate);
            if let Some((name, spec)) = path_map.get(&key) {
                matched.insert(name.clone(), spec.clone());
            }
        }
        if matched.is_empty() {
            return;
        }
        let result = ToolResultPayload::from_value(json!({ "source": "skill_read" }));
        for (name, spec) in matched {
            let reference =
                crate::tools::schema_reference(&name, &crate::tools::tool_schema_document(&spec));
            self.append_tool_log(
                user_id,
                session_id,
                &name,
                args,
                &result,
                round_info,
                None,
                None,
                None,
                Some(&reference),
            );
        }
    }

    pub(super) fn resolve_final_answer(&self, content: &str) -> String {
        strip_tool_calls(content).trim().to_string()
    }

    pub(super) fn reconcile_final_answer_workspace_images(
        &self,
        workspace_id: &str,
        session_id: &str,
        answer: &str,
    ) -> String {
        let cleaned_workspace_id = workspace_id.trim();
        if cleaned_workspace_id.is_empty() || answer.trim().is_empty() {
            return answer.to_string();
        }
        let Some(image_regex) = markdown_image_regex() else {
            return answer.to_string();
        };
        if !image_regex.is_match(answer) {
            return answer.to_string();
        }

        let artifact_candidates = collect_existing_artifact_image_paths(
            &self.workspace,
            cleaned_workspace_id,
            session_id,
        );
        let mut dir_candidates: HashMap<String, Vec<String>> = HashMap::new();
        let mut used_replacements: HashSet<String> = HashSet::new();
        let mut changed = false;

        let rewritten = image_regex.replace_all(answer, |caps: &regex::Captures<'_>| {
            let full = caps.get(0).map(|item| item.as_str()).unwrap_or("");
            let alt = caps.get(1).map(|item| item.as_str()).unwrap_or("");
            let raw_target = caps.get(2).map(|item| item.as_str()).unwrap_or("").trim();
            if raw_target.is_empty() {
                return full.to_string();
            }
            let (path_token, title_suffix) = split_markdown_target(raw_target);
            if path_token.is_empty() {
                return full.to_string();
            }
            let (path_token_clean, wrapper) = unwrap_markdown_path_token(&path_token);
            let (path_without_suffix, _) = split_url_suffix(path_token_clean.as_str());
            let Some(normalized_relative) = normalize_workspace_markdown_relative_path(
                &path_without_suffix,
                cleaned_workspace_id,
            ) else {
                return full.to_string();
            };
            if workspace_relative_path_exists(
                &self.workspace,
                cleaned_workspace_id,
                normalized_relative.as_str(),
            ) {
                return full.to_string();
            }

            let replacement = find_missing_image_replacement(
                &self.workspace,
                cleaned_workspace_id,
                normalized_relative.as_str(),
                &artifact_candidates,
                &mut dir_candidates,
                &mut used_replacements,
            );
            let Some(replacement) = replacement else {
                return full.to_string();
            };
            let replaced_target = format_markdown_path_token(
                path_token_clean.as_str(),
                replacement.as_str(),
                cleaned_workspace_id,
                wrapper,
            );
            let rebuilt_target = if title_suffix.is_empty() {
                replaced_target
            } else {
                format!("{replaced_target} {title_suffix}")
            };
            changed = true;
            format!("![{alt}]({rebuilt_target})")
        });

        if changed {
            rewritten.into_owned()
        } else {
            answer.to_string()
        }
    }

    pub(super) async fn execute_tool_with_timeout(
        &self,
        tool_context: &ToolContext<'_>,
        name: &str,
        args: &Value,
        timeout: Option<Duration>,
    ) -> Result<Value, anyhow::Error> {
        if let Some(timeout) = timeout {
            match tokio::time::timeout(
                timeout,
                crate::tools::execute_tool(tool_context, name, args),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(anyhow!(TOOL_TIMEOUT_ERROR)),
            }
        } else {
            crate::tools::execute_tool(tool_context, name, args).await
        }
    }

    pub(super) fn resolve_tool_timeout(
        &self,
        config: &Config,
        tool_name: &str,
        args: &Value,
    ) -> Option<Duration> {
        let canonical_tool_name = crate::tools::resolve_tool_name(tool_name);
        let _ = canonical_tool_name;
        let mut timeout_s = parse_timeout_secs(args.get("timeout_s")).unwrap_or(0.0);
        if canonical_tool_name == "执行命令" {
            let run_in_background = match args.get("run_in_background") {
                Some(Value::Bool(flag)) => *flag,
                Some(Value::String(text)) => matches!(
                    text.trim().to_ascii_lowercase().as_str(),
                    "true" | "1" | "yes" | "on"
                ),
                _ => false,
            };
            if !run_in_background {
                // Blocking mode: the tool waits for the command up to its own
                // timeout_s (default 120s), so the outer budget must outlast it.
                let explicit = args.get("timeout_s").is_some();
                return if explicit && timeout_s <= 0.0 {
                    None
                } else {
                    let effective = if timeout_s > 0.0 { timeout_s } else { 120.0 };
                    Some(Duration::from_secs_f64(
                        (effective + 30.0).max(MIN_TOOL_TIMEOUT_S),
                    ))
                };
            }
            if timeout_s <= 0.0 {
                timeout_s = DEFAULT_TOOL_TIMEOUT_S;
            }
        } else if canonical_tool_name == "命令会话" {
            // A poll may wait up to 60s server-side before responding.
            let yield_s = crate::tools::command_options::parse_command_session_wait(args)
                .yield_time_ms as f64
                / 1000.0;
            timeout_s = timeout_s.max(yield_s.min(60.0) + 30.0);
        } else if tool_name.contains('@') {
            if timeout_s <= 0.0 {
                let fallback = DEFAULT_TOOL_TIMEOUT_S;
                let configured = config.mcp.timeout_s as f64;
                timeout_s = if configured > 0.0 {
                    configured.max(fallback)
                } else {
                    fallback
                };
            }
        } else if timeout_s <= 0.0 {
            let fallback = DEFAULT_TOOL_TIMEOUT_S;
            let sandbox_timeout = if sandbox::sandbox_enabled(config) {
                sandbox::sandbox_timeout_seconds() as f64
            } else {
                0.0
            };
            timeout_s = if sandbox_timeout > 0.0 {
                sandbox_timeout.max(fallback)
            } else {
                fallback
            };
        }
        if timeout_s <= 0.0 {
            None
        } else {
            Some(Duration::from_secs_f64(timeout_s.max(MIN_TOOL_TIMEOUT_S)))
        }
    }
}

fn build_history_attachments(attachments: &[AttachmentPayload]) -> Option<Value> {
    if attachments.is_empty() {
        return None;
    }
    let mut items = Vec::new();
    for attachment in attachments {
        let name = attachment
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let content_type = attachment
            .content_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let public_path = attachment
            .public_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let content_value = attachment
            .content
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| !value.starts_with("data:"))
            .map(ToString::to_string);
        if name.is_none()
            && content_type.is_none()
            && public_path.is_none()
            && content_value.is_none()
        {
            continue;
        }
        let mut entry = Map::new();
        if let Some(name) = name {
            entry.insert("name".to_string(), Value::String(name.to_string()));
        }
        if let Some(content_type) = content_type {
            entry.insert(
                "content_type".to_string(),
                Value::String(content_type.to_string()),
            );
        }
        if let Some(public_path) = public_path {
            entry.insert(
                "public_path".to_string(),
                Value::String(public_path.to_string()),
            );
        }
        if let Some(content_value) = content_value {
            entry.insert("content".to_string(), Value::String(content_value));
        }
        items.push(Value::Object(entry));
    }
    if items.is_empty() {
        None
    } else {
        Some(Value::Array(items))
    }
}

fn extract_file_paths(args: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    let Some(obj) = args.as_object() else {
        return paths;
    };
    if let Some(Value::Array(files)) = obj.get("files") {
        for item in files {
            let Some(file_obj) = item.as_object() else {
                continue;
            };
            let path = file_obj
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                paths.push(path);
            }
        }
    }
    if let Some(path) = obj.get("path").and_then(Value::as_str) {
        let cleaned = path.trim();
        if !cleaned.is_empty() {
            paths.push(cleaned.to_string());
        }
    }
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();
    for path in paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        ordered.push(path);
    }
    ordered
}

#[derive(Clone, Copy)]
enum MarkdownPathWrapper {
    None,
    Angle,
    DoubleQuote,
    SingleQuote,
}

fn markdown_image_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| {
            // Captures Markdown image links: ![alt](target)
            compile_regex(r"!\[([^\]]*)\]\(([^)]+)\)", "markdown_image_link")
        })
        .as_ref()
}

fn split_markdown_target(raw: &str) -> (String, String) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (String::new(), String::new());
    }
    let mut in_angle = false;
    for (index, ch) in trimmed.char_indices() {
        if ch == '<' {
            in_angle = true;
            continue;
        }
        if ch == '>' {
            in_angle = false;
            continue;
        }
        if ch.is_whitespace() && !in_angle {
            let path = trimmed[..index].trim().to_string();
            let suffix = trimmed[index..].trim().to_string();
            return (path, suffix);
        }
    }
    (trimmed.to_string(), String::new())
}

fn unwrap_markdown_path_token(token: &str) -> (String, MarkdownPathWrapper) {
    let trimmed = token.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('<') && trimmed.ends_with('>') {
        return (
            trimmed[1..trimmed.len().saturating_sub(1)]
                .trim()
                .to_string(),
            MarkdownPathWrapper::Angle,
        );
    }
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return (
            trimmed[1..trimmed.len().saturating_sub(1)]
                .trim()
                .to_string(),
            MarkdownPathWrapper::DoubleQuote,
        );
    }
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        return (
            trimmed[1..trimmed.len().saturating_sub(1)]
                .trim()
                .to_string(),
            MarkdownPathWrapper::SingleQuote,
        );
    }
    (trimmed.to_string(), MarkdownPathWrapper::None)
}

fn split_url_suffix(raw: &str) -> (String, String) {
    if let Some(index) = raw.find(['?', '#']) {
        return (raw[..index].to_string(), raw[index..].to_string());
    }
    (raw.to_string(), String::new())
}

fn normalize_workspace_markdown_relative_path(raw: &str, workspace_id: &str) -> Option<String> {
    let mut value = raw.trim().replace('\\', "/");
    if value.is_empty() {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
        || lower.starts_with("mailto:")
    {
        return None;
    }
    if value.len() >= 2 && value.as_bytes()[1] == b':' {
        return None;
    }

    if let Some(stripped) = value.strip_prefix("/workspaces/") {
        value = stripped.to_string();
    } else if let Some(stripped) = value.strip_prefix("workspaces/") {
        value = stripped.to_string();
    }
    if !value.is_empty() {
        if value == workspace_id {
            return Some(String::new());
        }
        let workspace_prefix = format!("{workspace_id}/");
        if let Some(stripped) = value.strip_prefix(workspace_prefix.as_str()) {
            value = stripped.to_string();
        } else if value.contains("__c__") || value.contains("__a__") || value.contains("__agent__")
        {
            return None;
        }
    }

    if let Some(stripped) = value.strip_prefix("/workspace/") {
        value = stripped.to_string();
    } else if let Some(stripped) = value.strip_prefix("workspace/") {
        value = stripped.to_string();
    }

    if let Some(stripped) = value.strip_prefix('/') {
        value = stripped.to_string();
    }
    if let Some(stripped) = value.strip_prefix("./") {
        value = stripped.to_string();
    }
    let mut normalized_parts = Vec::new();
    for part in value.split('/') {
        let cleaned = part.trim();
        if cleaned.is_empty() || cleaned == "." {
            continue;
        }
        if cleaned == ".." {
            return None;
        }
        normalized_parts.push(cleaned);
    }
    if normalized_parts.is_empty() {
        return Some(String::new());
    }
    Some(normalized_parts.join("/"))
}

fn workspace_relative_path_exists(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    relative_path: &str,
) -> bool {
    if relative_path.trim().is_empty() {
        return false;
    }
    match workspace.resolve_path(workspace_id, relative_path) {
        Ok(path) => path.exists(),
        Err(_) => false,
    }
}

fn is_image_relative_path(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg"
    )
}

fn collect_existing_artifact_image_paths(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    session_id: &str,
) -> Vec<String> {
    let artifacts = workspace
        .load_artifact_logs(workspace_id, session_id, 200)
        .unwrap_or_default();
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for item in artifacts {
        let Some(obj) = item.as_object() else {
            continue;
        };
        if obj.get("kind").and_then(Value::as_str) != Some("file") {
            continue;
        }
        let Some(name) = obj.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(relative) = normalize_workspace_markdown_relative_path(name, workspace_id) else {
            continue;
        };
        if relative.is_empty() || !is_image_relative_path(relative.as_str()) {
            continue;
        }
        if !workspace_relative_path_exists(workspace, workspace_id, relative.as_str()) {
            continue;
        }
        if seen.insert(relative.clone()) {
            output.push(relative);
        }
    }
    output
}

fn load_directory_image_candidates(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    directory: &str,
) -> Vec<String> {
    let relative_dir = directory.trim_matches('/');
    let Ok((entries, _, _, _, _)) =
        workspace.list_workspace_entries(workspace_id, relative_dir, None, 0, 512, "name", "asc")
    else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for entry in entries {
        if entry.entry_type != "file" || !is_image_relative_path(entry.path.as_str()) {
            continue;
        }
        output.push(entry.path);
    }
    output
}

fn find_missing_image_replacement(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    missing_relative: &str,
    artifact_candidates: &[String],
    dir_candidates: &mut HashMap<String, Vec<String>>,
    used_replacements: &mut HashSet<String>,
) -> Option<String> {
    if missing_relative.trim().is_empty() || !is_image_relative_path(missing_relative) {
        return None;
    }
    let missing_path = Path::new(missing_relative);
    let missing_name = missing_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let missing_dir = missing_path
        .parent()
        .map(|value| value.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
        .trim_matches('/')
        .to_string();

    let candidates = dir_candidates
        .entry(missing_dir.clone())
        .or_insert_with(|| {
            load_directory_image_candidates(workspace, workspace_id, missing_dir.as_str())
        });

    if !missing_name.is_empty() {
        for candidate in candidates.iter() {
            let candidate_name = Path::new(candidate)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if candidate_name == missing_name && used_replacements.insert(candidate.clone()) {
                return Some(candidate.clone());
            }
        }
    }
    for candidate in candidates.iter() {
        if used_replacements.insert(candidate.clone()) {
            return Some(candidate.clone());
        }
    }
    for candidate in artifact_candidates {
        let candidate_dir = Path::new(candidate)
            .parent()
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default()
            .trim_matches('/')
            .to_string();
        if !missing_dir.is_empty() && candidate_dir != missing_dir {
            continue;
        }
        if used_replacements.insert(candidate.clone()) {
            return Some(candidate.clone());
        }
    }
    None
}

fn format_markdown_path_token(
    original_clean_path: &str,
    replacement_relative: &str,
    workspace_id: &str,
    wrapper: MarkdownPathWrapper,
) -> String {
    let replacement = if original_clean_path.starts_with("/workspaces/") {
        format!(
            "/workspaces/{workspace_id}/{}",
            replacement_relative.trim_matches('/')
        )
    } else if original_clean_path.starts_with('/') {
        format!("/{}", replacement_relative.trim_matches('/'))
    } else if original_clean_path.starts_with("./") {
        format!("./{}", replacement_relative.trim_matches('/'))
    } else {
        replacement_relative.trim_matches('/').to_string()
    };
    match wrapper {
        MarkdownPathWrapper::None => replacement,
        MarkdownPathWrapper::Angle => format!("<{replacement}>"),
        MarkdownPathWrapper::DoubleQuote => format!("\"{replacement}\""),
        MarkdownPathWrapper::SingleQuote => format!("'{replacement}'"),
    }
}

fn normalize_compare_path(path: &Path) -> PathBuf {
    let normalized = normalize_target_path(path);
    normalize_path_for_compare(&normalized)
}

fn resolve_absolute_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        Some(path)
    } else {
        let cwd = std::env::current_dir().ok()?;
        Some(cwd.join(path))
    }
}

fn extract_command_lines(args: &Value) -> Vec<String> {
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut commands = Vec::new();
    for line in content.lines() {
        let cleaned = line.trim();
        if !cleaned.is_empty() {
            commands.push(cleaned.to_string());
        }
    }
    commands
}

fn parse_timeout_secs(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(num)) => num.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_model_messages_keep_user_assistant_and_tool_input() {
        let user = canonical_model_message(&json!({
            "role":"user", "content":"display", "model_content":"model input",
            "session_id":"thread-a", "turn_id":"turn-a", "timestamp":"sample"
        }))
        .unwrap();
        assert_eq!(user, json!({"role":"user", "content":"model input"}));

        let assistant = canonical_model_message(&json!({
            "role":"assistant", "content":"visible answer", "model_content":"model answer",
            "reasoning_content":"reasoning", "meta":{"message_stats":{"tokens":3}}
        }))
        .unwrap();
        assert_eq!(assistant["content"], "model answer");
        assert_eq!(assistant["reasoning_content"], "reasoning");
        assert!(assistant.get("meta").is_none());

        let tool = canonical_model_message(&json!({
            "role":"tool", "content":"result", "tool_call_id":"call-a"
        }))
        .unwrap();
        assert_eq!(
            tool,
            json!({"role":"tool", "content":"result", "tool_call_id":"call-a"})
        );
        let observation =
            canonical_model_message(&json!({"role":"tool", "content":"result"})).unwrap();
        assert_eq!(observation["role"], "user");
        assert_eq!(
            observation["content"],
            format!("{OBSERVATION_PREFIX}result")
        );
    }

    #[test]
    fn canonical_model_message_preserves_checkpoint_and_filters_ui_commands() {
        let checkpoint = json!({"role":"system", "content":"summary",
            "meta":{"replacement_history":[{"role":"user","content":"retained"}]}});
        assert_eq!(canonical_model_message(&checkpoint), Some(checkpoint));
        assert!(
            canonical_model_message(&json!({"role":"user", "content":"sample",
            "meta":{"type":"manual_compaction_command"}}))
            .is_none()
        );
    }

    fn transport_envelope_bytes(result: &ToolResultPayload) -> usize {
        serde_json::to_vec(&json!({
            "ok": result.ok, "sandbox": result.sandbox,
            "data": result.data, "error": result.error, "meta": result.meta,
        }))
        .unwrap()
        .len()
    }

    #[test]
    fn transport_guard_keeps_small_results_unchanged() {
        let mut result = ToolResultPayload::from_value(json!({
            "ok": true,
            "data": {"exit_code": 0, "stdout": "done"},
            "meta": {"workspace_version": 3},
        }));
        let original_data = result.data.clone();
        let original_meta = result.meta.clone();
        assert_eq!(enforce_tool_result_transport_guard(&mut result), None);
        assert_eq!(result.data, original_data);
        assert_eq!(result.meta, original_meta);
    }

    #[test]
    fn transport_guard_keeps_saved_result_recovery() {
        let handle = "tool-result:0123456789abcdef0123456789abcdef";
        let mut result = ToolResultPayload::from_value(json!({"ok":true,
            "data":{"text":"x".repeat(TOOL_RESULT_TRANSPORT_MAX_BYTES + 100)},
            "meta":{"saved_result":{"handle":handle,"bytes":2_000_000,"expires_at":2_000_000_000}}
        }));
        assert!(tool_result_exceeds_transport_limit(&result));
        enforce_tool_result_transport_guard(&mut result);
        assert_eq!(result.data["result_handle"], handle);
        assert_eq!(result.data["recoverable"], true);
        assert_eq!(
            result.meta.as_ref().unwrap()["saved_result"]["handle"],
            handle
        );
        let observation = result.to_compact_payload_with_token_budget("external", Some(256));
        assert_eq!(observation["result_handle"], handle);
        assert_eq!(observation["recoverable"], true);
        assert!(super::super::tool_result_payload::observation_token_count(&observation) <= 256);
    }

    #[test]
    fn transport_guard_bounds_large_data_error_and_meta() {
        for large_field in ["data", "error", "meta"] {
            let large = "中🙂\\\"\n".repeat(TOOL_RESULT_TRANSPORT_MAX_BYTES / 4);
            let mut value = json!({
                "ok": false,
                "data": {"exit_code": 7, "session_id": "command-1"},
                "error": "Execution failed",
                "meta": {"error_code": "EXECUTION_FAILED", "workspace_version": 4},
            });
            match large_field {
                "data" => value["data"]["stdout"] = Value::String(large),
                "error" => value["error"] = Value::String(large),
                _ => value["meta"]["provider_detail"] = Value::String(large),
            }
            let mut result = ToolResultPayload::from_value(value);
            let original_bytes = transport_envelope_bytes(&result);
            assert_eq!(
                enforce_tool_result_transport_guard(&mut result),
                Some(original_bytes)
            );
            assert!(transport_envelope_bytes(&result) <= TOOL_RESULT_TRANSPORT_MAX_BYTES);
            assert_eq!(result.data["exit_code"], 7);
            assert_eq!(result.data["session_id"], "command-1");
            assert_eq!(result.data["recoverable"], false);
            assert_eq!(result.data["truncation_scope"], "transport_safety");
            assert_eq!(result.meta.as_ref().unwrap()["workspace_version"], 4);
            assert!(result.error.len() <= TOOL_RESULT_TRANSPORT_ERROR_BYTES);
            assert!(
                result.data["preview"].as_str().unwrap().len()
                    <= TOOL_RESULT_TRANSPORT_PREVIEW_BYTES
            );
            assert!(result.data["omitted_bytes"].as_u64().unwrap() > 0);
            assert!(result.data.get("result_handle").is_none());
        }
    }

    #[test]
    fn transport_guard_preserves_bounded_controls_and_exact_cursor() {
        let cursor = "opaque-token/中🙂".repeat(80);
        let mut result = ToolResultPayload::from_value(json!({
            "ok": true,
            "data": {
                "stdout": "x".repeat(TOOL_RESULT_TRANSPORT_MAX_BYTES + 1),
                "next_cursor": cursor,
                "session_id": "command-1",
                "results": [{"returncode": 9, "command_session_id": "command-1", "status": "completed", "stdout": "large"}],
                "content": [{"type":"image", "attachment":{"path":"artifacts/frame.png", "bytes":64}}],
            },
            "meta": {"turn_control":{"kind":"yield", "message":"Waiting"}, "workspace_version":8},
        }));
        assert!(enforce_tool_result_transport_guard(&mut result).is_some());
        assert_eq!(result.data["next_cursor"], cursor);
        assert_eq!(result.data["exit_code"], 9);
        assert_eq!(result.data["results"][0]["returncode"], 9);
        assert_eq!(result.data["results"][0]["command_session_id"], "command-1");
        assert_eq!(
            result.data["content"][0]["attachment"]["path"],
            "artifacts/frame.png"
        );
        assert_eq!(result.meta.as_ref().unwrap()["exit_code"], 9);
        assert_eq!(
            result.meta.as_ref().unwrap()["turn_control"]["kind"],
            "yield"
        );
        assert!(result.data["recovery_hint"]
            .as_str()
            .unwrap()
            .contains("not stored"));
    }

    #[test]
    fn transport_guard_omits_oversized_opaque_controls_without_corruption() {
        let mut result = ToolResultPayload::from_value(json!({
            "ok": true,
            "data": {
                "stdout": "x".repeat(TOOL_RESULT_TRANSPORT_MAX_BYTES + 1),
                "next_cursor": "opaque".repeat(TOOL_RESULT_TRANSPORT_CONTROL_FIELD_BYTES),
            },
        }));
        assert!(enforce_tool_result_transport_guard(&mut result).is_some());
        assert!(result.data.get("next_cursor").is_none());
        assert_eq!(result.data["recoverable"], false);
    }

    #[test]
    fn transport_writer_uses_fixed_retention_and_counts_omission_exactly() {
        use std::io::Write;
        let value = format!("{}{}", "中🙂".repeat(30_000), "tail");
        let mut writer = ToolResultTransportWriter::new();
        // Split inside scalars to exercise arbitrary IO write boundaries.
        for chunk in value.as_bytes().chunks(7) {
            writer.write_all(chunk).unwrap();
            assert!(writer.head.len() <= writer.head_limit);
            assert!(writer.tail_len <= writer.tail.len());
        }
        assert_eq!(writer.total_bytes, value.len());
        let (preview, omitted_bytes) = writer.into_preview();
        assert!(preview.len() <= TOOL_RESULT_TRANSPORT_PREVIEW_BYTES);
        assert!(preview.starts_with("中🙂"));
        assert!(preview.ends_with("tail"));
        assert_eq!(
            omitted_bytes,
            value.len() - (preview.len() - TOOL_RESULT_TRUNCATION_MARKER.len())
        );
    }

    #[cfg(feature = "sqlite-storage")]
    #[tokio::test]
    async fn command_session_outer_deadline_outlasts_the_default_exit_wait() {
        let root = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
        config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
        config.server.mode = "desktop".into();
        let state = crate::state::AppState::new_with_options(
            ConfigStore::new(root.path().join("config.yaml")),
            config.clone(),
            crate::state::AppStateInitOptions::cli_default().with_start_thread_runtime(false),
        )
        .unwrap();
        let orchestrator = &state.kernel.orchestrator;
        assert_eq!(
            orchestrator.resolve_tool_timeout(&config, "command_session", &json!({})),
            Some(Duration::from_secs(90))
        );
        assert_eq!(
            orchestrator.resolve_tool_timeout(
                &config,
                "command_session",
                &json!({"action": "write_stdin"})
            ),
            Some(Duration::from_millis(30_500))
        );
    }

    #[test]
    fn test_split_markdown_target_keeps_title_suffix() {
        let (path, suffix) = split_markdown_target("charts/a.png \"title\"");
        assert_eq!(path, "charts/a.png");
        assert_eq!(suffix, "\"title\"");
    }

    #[test]
    fn test_normalize_workspace_markdown_relative_path_handles_workspace_public_path() {
        let normalized = normalize_workspace_markdown_relative_path(
            "/workspaces/alice__c__2/charts/a.png",
            "alice__c__2",
        );
        assert_eq!(normalized.as_deref(), Some("charts/a.png"));
    }

    #[test]
    fn test_format_markdown_path_token_preserves_leading_slash() {
        let formatted = format_markdown_path_token(
            "/charts/wrong.png",
            "charts/right.png",
            "alice__c__2",
            MarkdownPathWrapper::None,
        );
        assert_eq!(formatted, "/charts/right.png");
    }
}
