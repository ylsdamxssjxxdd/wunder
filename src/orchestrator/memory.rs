use super::*;

const MEMORY_SUMMARY_PROMPT_PATH: &str = "prompts/memory_summary.txt";

#[derive(Clone)]
pub(super) struct MemorySummaryTask {
    task_id: String,
    user_id: String,
    session_id: String,
    queued_time: f64,
    config_overrides: Option<Value>,
    model_name: Option<String>,
    attachments: Option<Vec<AttachmentPayload>>,
    request_messages: Option<Vec<Value>>,
    language: String,
    status: String,
    start_time: f64,
    end_time: f64,
    request_payload: Option<Value>,
    final_answer: String,
    summary_result: String,
    error: String,
}

pub(super) struct MemoryQueue {
    state: Mutex<MemoryQueueState>,
    notify: Notify,
}

struct MemoryQueueState {
    queue: std::collections::BinaryHeap<MemoryQueueItem>,
    seq: u64,
    active: Option<MemorySummaryTask>,
    history: VecDeque<MemorySummaryTask>,
    worker: Option<JoinHandle<()>>,
}

struct MemoryQueueItem {
    queued_time: f64,
    seq: u64,
    task: MemorySummaryTask,
}

impl Ord for MemoryQueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        let time_cmp = other
            .queued_time
            .partial_cmp(&self.queued_time)
            .unwrap_or(Ordering::Equal);
        if time_cmp != Ordering::Equal {
            return time_cmp;
        }
        other.seq.cmp(&self.seq)
    }
}

impl PartialOrd for MemoryQueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for MemoryQueueItem {
    fn eq(&self, other: &Self) -> bool {
        self.queued_time == other.queued_time && self.seq == other.seq
    }
}

impl Eq for MemoryQueueItem {}

impl MemoryQueue {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(MemoryQueueState {
                queue: std::collections::BinaryHeap::new(),
                seq: 0,
                active: None,
                history: VecDeque::with_capacity(100),
                worker: None,
            }),
            notify: Notify::new(),
        }
    }
}

impl Orchestrator {
    pub async fn get_memory_queue_status(&self) -> Value {
        let now = now_ts();
        let (active, queued, history_fallback) = {
            let state = self.memory_queue.state.lock().await;
            let active = state.active.clone();
            let queued = state
                .queue
                .iter()
                .map(|item| item.task.clone())
                .collect::<Vec<_>>();
            let history = state.history.iter().cloned().collect::<Vec<_>>();
            (active, queued, history)
        };

        let mut active_items = Vec::new();
        if let Some(task) = active {
            active_items.push(self.format_memory_task(&task, now));
        }
        let mut queued_sorted = queued;
        queued_sorted.sort_by(|a, b| {
            let time_cmp = a
                .queued_time
                .partial_cmp(&b.queued_time)
                .unwrap_or(Ordering::Equal);
            if time_cmp != Ordering::Equal {
                return time_cmp;
            }
            a.task_id.cmp(&b.task_id)
        });
        for task in queued_sorted {
            active_items.push(self.format_memory_task(&task, now));
        }

        let storage_history = self
            .memory_store
            .list_task_logs_async(None)
            .await
            .into_iter()
            .map(|payload| Value::Object(payload.into_iter().collect::<Map<String, Value>>()))
            .collect::<Vec<_>>();
        let history = if storage_history.is_empty() {
            history_fallback
                .into_iter()
                .map(|task| self.format_memory_task(&task, now))
                .collect::<Vec<_>>()
        } else {
            storage_history
        };

        json!({
            "active": active_items,
            "history": history,
        })
    }

    pub async fn get_memory_queue_detail(&self, task_id: &str) -> Option<Value> {
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return None;
        }
        if let Some(task) = self.find_memory_task(cleaned).await {
            let mut detail = self.format_memory_task(&task, now_ts());
            let log_payload = self.log_payload_enabled().await;
            let mut request_payload = task.request_payload.clone();
            if log_payload && request_payload.is_none() {
                if let Ok(payload) = self.build_memory_summary_request_payload(&task).await {
                    request_payload = Some(payload);
                }
            }
            if let Value::Object(ref mut map) = detail {
                if let Some(payload) = request_payload {
                    map.insert("request".to_string(), payload);
                }
                map.insert("result".to_string(), json!(task.summary_result));
                if !task.error.is_empty() {
                    map.insert("error".to_string(), json!(task.error));
                }
            }
            return Some(detail);
        }
        self.memory_store
            .get_task_log_async(cleaned)
            .await
            .map(|payload| Value::Object(payload.into_iter().collect::<Map<String, Value>>()))
    }

    pub(super) fn shrink_messages_to_limit(&self, messages: Vec<Value>, limit: i64) -> Vec<Value> {
        let total_tokens = estimate_messages_tokens(&messages);
        if total_tokens <= limit {
            return messages;
        }
        let mut overflow = total_tokens - limit;
        let mut trimmed = messages;
        for index in 0..trimmed.len() {
            if overflow <= 0 {
                break;
            }
            let changed = if let Some(obj) = trimmed[index].as_object_mut() {
                let role = obj.get("role").and_then(Value::as_str).unwrap_or("");
                let content = obj.get("content").unwrap_or(&Value::Null);
                if !Self::is_observation_message(role, content) {
                    false
                } else if let Value::String(text) = content {
                    let current_tokens = approx_token_count(text);
                    if current_tokens <= COMPACTION_MIN_OBSERVATION_TOKENS {
                        false
                    } else {
                        let target_tokens =
                            (current_tokens - overflow).max(COMPACTION_MIN_OBSERVATION_TOKENS);
                        let new_content =
                            trim_text_to_tokens(text, target_tokens, "...(truncated)");
                        if new_content == *text {
                            false
                        } else {
                            obj.insert("content".to_string(), Value::String(new_content));
                            true
                        }
                    }
                } else {
                    false
                }
            } else {
                false
            };
            if changed {
                overflow = (estimate_messages_tokens(&trimmed) - limit).max(0);
            }
        }
        trimmed
    }

    pub(super) fn prepare_summary_messages(
        &self,
        messages: Vec<Value>,
        max_tokens: i64,
    ) -> Vec<Value> {
        if messages.is_empty() {
            return messages;
        }
        let mut trimmed = Vec::with_capacity(messages.len());
        for message in messages {
            let Some(obj) = message.as_object() else {
                trimmed.push(message);
                continue;
            };
            let role = obj.get("role").and_then(Value::as_str).unwrap_or("");
            let content = obj.get("content").cloned().unwrap_or(Value::Null);
            let mut new_message = obj.clone();
            if let Value::String(text) = &content {
                let target = max_tokens.max(1);
                if approx_token_count(text) > target {
                    new_message.insert(
                        "content".to_string(),
                        Value::String(trim_text_to_tokens(text, target, "...(truncated)")),
                    );
                }
            }
            if role == "assistant" {
                new_message.remove("reasoning_content");
                new_message.remove("reasoning");
            }
            trimmed.push(Value::Object(new_message));
        }
        trimmed
    }

    pub(super) fn locate_current_user_index(messages: &[Value]) -> Option<usize> {
        messages.iter().rposition(|message| {
            let role = message.get("role").and_then(Value::as_str).unwrap_or("");
            if role != "user" {
                return false;
            }
            let content = message.get("content").unwrap_or(&Value::Null);
            !Self::is_observation_message(role, content)
        })
    }

    pub(super) async fn maybe_compact_messages(
        &self,
        config: &Config,
        llm_config: &LlmModelConfig,
        user_id: &str,
        session_id: &str,
        round_info: RoundInfo,
        messages: Vec<Value>,
        emitter: &EventEmitter,
        current_question: &str,
        log_payload: bool,
    ) -> Result<Vec<Value>, OrchestratorError> {
        let Some(limit) = HistoryManager::get_auto_compact_limit(llm_config) else {
            return Ok(messages);
        };

        let context_tokens = estimate_messages_tokens(&messages);
        let max_context = llm_config.max_context.unwrap_or(0) as i64;
        let mut ratio = llm_config
            .history_compaction_ratio
            .unwrap_or(COMPACTION_HISTORY_RATIO as f32) as f64;
        if ratio <= 0.0 {
            ratio = COMPACTION_HISTORY_RATIO;
        } else if ratio > 1.0 {
            ratio = if ratio <= 100.0 { ratio / 100.0 } else { 1.0 };
        }
        let history_threshold = if max_context > 0 {
            Some((max_context as f64 * ratio) as i64)
        } else {
            None
        };
        let (should_compact_by_history, should_compact) =
            should_compact_by_context(context_tokens, limit, history_threshold);
        let total_tokens = context_tokens;
        let history_usage = context_tokens;
        if !should_compact {
            return Ok(messages);
        }

        let reset_mode = if should_compact_by_history {
            let mode = llm_config
                .history_compaction_reset
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_lowercase();
            if matches!(mode.as_str(), "zero" | "current" | "keep") {
                mode
            } else {
                "zero".to_string()
            }
        } else {
            String::new()
        };

        let summary_text = if should_compact_by_history {
            i18n::t("compaction.reason.history_threshold")
        } else {
            i18n::t("compaction.reason.context_too_long")
        };
        let compaction_round = RoundInfo {
            user_round: round_info.user_round,
            model_round: None,
        };
        let mut compacting_payload = json!({ "stage": "compacting", "summary": summary_text });
        if let Value::Object(ref mut map) = compacting_payload {
            compaction_round.insert_into(map);
        }
        emitter.emit("progress", compacting_payload).await;

        let system_message = messages
            .first()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
            .cloned();
        let current_user_index = Self::locate_current_user_index(&messages);
        let current_user_message = current_user_index
            .and_then(|index| messages.get(index))
            .cloned();
        let mut source_messages: Vec<Value> = Vec::new();
        for (index, message) in messages.iter().enumerate() {
            if system_message.is_some() && index == 0 {
                continue;
            }
            if current_user_index.is_some() && Some(index) == current_user_index {
                continue;
            }
            source_messages.push(message.clone());
        }

        let mut artifact_prefixes = i18n::get_known_prefixes("history.artifact_prefix");
        if artifact_prefixes.is_empty() {
            artifact_prefixes.push(i18n::t("history.artifact_prefix"));
        }
        let has_artifact = source_messages.iter().any(|message| {
            let Some(obj) = message.as_object() else {
                return false;
            };
            if obj.get("role").and_then(Value::as_str) != Some("system") {
                return false;
            }
            let content = obj.get("content").and_then(Value::as_str).unwrap_or("");
            artifact_prefixes
                .iter()
                .any(|prefix| content.trim().starts_with(prefix))
        });
        let mut artifact_content = String::new();
        if !has_artifact {
            let history_manager = HistoryManager;
            artifact_content =
                history_manager.load_artifact_index_message(&self.workspace, user_id, session_id);
            if !artifact_content.is_empty() {
                source_messages
                    .push(json!({ "role": "system", "content": artifact_content.clone() }));
            }
        }

        let user_content = self.build_compaction_user_content(&source_messages);
        if user_content.trim().is_empty() {
            emitter
                .emit(
                    "compaction",
                    json!({
                        "reason": if should_compact_by_history { "history" } else { "overflow" },
                        "status": "skipped",
                        "skip_reason": "no_candidates",
                        "history_usage": history_usage,
                        "context_tokens": history_usage,
                        "history_threshold": history_threshold,
                        "limit": limit,
                        "total_tokens": total_tokens,
                        "reset_mode": reset_mode,
                    }),
                )
                .await;
            return Ok(messages);
        }

        let compaction_prompt = HistoryManager::load_compaction_prompt();
        let compaction_instruction = if artifact_content.is_empty() {
            compaction_prompt.clone()
        } else {
            format!("{compaction_prompt}\n\n{artifact_content}")
        };
        let mut summary_input = messages.clone();
        let compaction_message = json!({ "role": "user", "content": compaction_instruction });
        if let Some(index) = current_user_index {
            summary_input[index] = compaction_message;
        } else {
            summary_input.push(compaction_message);
        }

        let mut compacted_until_ts: Option<f64> = None;
        let mut compacted_until: Option<String> = None;
        let mut current_question_ts: Option<f64> = None;
        let mut skipped_question = false;
        let question_text = current_question.trim();
        let current_user_signature = current_user_message
            .as_ref()
            .map(|message| {
                self.extract_memory_summary_text(message.get("content").unwrap_or(&Value::Null))
            })
            .unwrap_or_default();
        let mut question_candidates: Vec<String> = Vec::new();
        if !question_text.is_empty() {
            question_candidates.push(question_text.to_string());
        }
        let current_user_signature = current_user_signature.trim();
        if !current_user_signature.is_empty()
            && !question_candidates
                .iter()
                .any(|candidate| candidate == current_user_signature)
        {
            question_candidates.push(current_user_signature.to_string());
        }
        let history = self
            .workspace
            .load_history_async(user_id, session_id, config.workspace.max_history_items)
            .await
            .unwrap_or_default();
        let (history_items, _) = HistoryManager::build_compaction_candidates(&history);
        let mut boundary_item: Option<Value> = None;
        for item in history_items.iter().rev() {
            if !skipped_question && !question_candidates.is_empty() {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("");
                let content =
                    self.extract_memory_summary_text(item.get("content").unwrap_or(&Value::Null));
                if role == "user"
                    && !content.is_empty()
                    && question_candidates
                        .iter()
                        .any(|candidate| candidate == content.trim())
                {
                    skipped_question = true;
                    current_question_ts = HistoryManager::get_item_timestamp(item);
                    continue;
                }
            }
            boundary_item = Some(item.clone());
            break;
        }
        if let Some(boundary_item) = boundary_item {
            compacted_until_ts = HistoryManager::get_item_timestamp(&boundary_item);
            compacted_until = boundary_item
                .get("timestamp")
                .and_then(Value::as_str)
                .map(|value| value.to_string());
        }

        let mut summary_config = llm_config.clone();
        let max_output = llm_config
            .max_output
            .unwrap_or(COMPACTION_SUMMARY_MAX_OUTPUT as u32)
            .min(COMPACTION_SUMMARY_MAX_OUTPUT as u32);
        summary_config.max_output = Some(max_output);
        summary_config.max_rounds = Some(1);

        let summary_limit =
            HistoryManager::get_auto_compact_limit(&summary_config).unwrap_or(limit);
        if estimate_messages_tokens(&summary_input) > summary_limit {
            let per_message_limit = summary_limit
                .min(COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS)
                .max(1);
            summary_input = self.prepare_summary_messages(summary_input, per_message_limit);
            summary_input = self.shrink_messages_to_limit(summary_input, summary_limit);
        }
        if estimate_messages_tokens(&summary_input) > summary_limit {
            let mut trimmed = Vec::new();
            let head_len = if summary_input.first().is_some_and(|message| {
                message.get("role").and_then(Value::as_str) == Some("system")
            }) {
                trimmed.push(summary_input[0].clone());
                1
            } else {
                0
            };
            let remaining = (summary_limit - estimate_messages_tokens(&trimmed)).max(1);
            let tail =
                trim_messages_to_budget(summary_input.get(head_len..).unwrap_or(&[]), remaining);
            trimmed.extend(tail);
            summary_input = trimmed;
        }

        let mut request_payload = if log_payload {
            let payload_messages = self.sanitize_messages_for_log(summary_input.clone(), None);
            let payload = build_llm_client(&summary_config, self.http.clone())
                .build_request_payload(&self.build_chat_messages(&payload_messages), false);
            json!({
                "provider": summary_config.provider,
                "model": summary_config.model,
                "base_url": summary_config.base_url,
                "payload": payload,
                "purpose": "compaction_summary",
            })
        } else {
            json!({
                "provider": summary_config.provider,
                "model": summary_config.model,
                "base_url": summary_config.base_url,
                "payload_omitted": true,
                "purpose": "compaction_summary",
            })
        };
        if let Value::Object(ref mut map) = request_payload {
            compaction_round.insert_into(map);
        }
        emitter.emit("llm_request", request_payload).await;

        let mut summary_fallback = false;
        let summary_text = match self
            .call_llm(
                llm_config,
                &summary_input,
                user_id,
                emitter,
                session_id,
                false,
                compaction_round,
                false,
                true,
                log_payload,
                None,
                Some(summary_config),
            )
            .await
        {
            Ok((content, _, _, _)) => self.resolve_final_answer(&content),
            Err(err) => {
                if err.code() == "USER_QUOTA_EXCEEDED" {
                    return Err(err);
                }
                summary_fallback = true;
                i18n::t("compaction.summary_fallback")
            }
        };
        let mut summary_text = HistoryManager::format_compaction_summary(&summary_text);
        let mut base_messages: Vec<Value> = Vec::new();
        if let Some(system_message) = system_message.clone() {
            base_messages.push(system_message);
        }
        if let Some(current_user_message) = current_user_message.clone() {
            base_messages.push(current_user_message);
        } else if !question_text.is_empty() {
            base_messages.push(json!({ "role": "user", "content": question_text }));
        }
        let base_tokens = estimate_messages_tokens(&base_messages);
        for _ in 0..3 {
            let summary_message = json!({ "role": "user", "content": summary_text });
            let total_tokens = base_tokens + estimate_message_tokens(&summary_message);
            if total_tokens <= limit {
                break;
            }
            let overflow = total_tokens - limit;
            let summary_tokens = approx_token_count(&summary_text);
            if summary_tokens <= 1 {
                break;
            }
            let target_tokens = (summary_tokens - overflow).max(1);
            let trimmed = trim_text_to_tokens(&summary_text, target_tokens, "...(truncated)");
            if trimmed == summary_text {
                break;
            }
            summary_text = trimmed;
        }
        let mut response_payload = json!({
            "content": summary_text,
            "reasoning": "",
            "purpose": "compaction_summary",
        });
        if let Value::Object(ref mut map) = response_payload {
            compaction_round.insert_into(map);
        }
        emitter.emit("llm_response", response_payload).await;

        let mut meta = serde_json::Map::new();
        meta.insert(
            "type".to_string(),
            Value::String(COMPACTION_META_TYPE.to_string()),
        );
        if let Some(value) = compacted_until_ts {
            meta.insert("compacted_until_ts".to_string(), json!(value));
        }
        if let Some(value) = compacted_until.clone() {
            meta.insert("compacted_until".to_string(), Value::String(value));
        }
        let meta_value = Value::Object(meta);
        self.append_chat(
            user_id,
            session_id,
            "system",
            Some(&Value::String(summary_text.clone())),
            Some(&meta_value),
            None,
            None,
            None,
        );

        if skipped_question {
            let should_reappend = compacted_until_ts.is_none()
                || current_question_ts.is_none()
                || current_question_ts <= compacted_until_ts;
            if should_reappend {
                if let Some(current_user_message) = current_user_message.as_ref() {
                    if let Some(content) = current_user_message.get("content") {
                        self.append_chat(
                            user_id,
                            session_id,
                            "user",
                            Some(content),
                            None,
                            None,
                            None,
                            None,
                        );
                    }
                } else if !question_text.is_empty() {
                    let question_value = Value::String(question_text.to_string());
                    self.append_chat(
                        user_id,
                        session_id,
                        "user",
                        Some(&question_value),
                        None,
                        None,
                        None,
                        None,
                    );
                }
            }
        }

        let mut rebuilt = Vec::new();
        if let Some(system_message) = system_message {
            rebuilt.push(system_message);
        }
        rebuilt.push(json!({ "role": "user", "content": summary_text }));
        if let Some(current_user_message) = current_user_message {
            rebuilt.push(current_user_message);
        } else if !question_text.is_empty() {
            rebuilt.push(json!({ "role": "user", "content": question_text }));
        }
        let rebuilt = self.shrink_messages_to_limit(rebuilt, limit);
        let rebuilt_tokens = estimate_messages_tokens(&rebuilt);

        let mut compaction_payload = json!({
            "reason": if should_compact_by_history { "history" } else { "overflow" },
            "status": if summary_fallback { "fallback" } else { "done" },
            "summary_fallback": summary_fallback,
            "summary_tokens": approx_token_count(&summary_text),
            "total_tokens": total_tokens,
            "total_tokens_after": rebuilt_tokens,
            "history_usage": history_usage,
            "context_tokens": history_usage,
            "context_tokens_after": rebuilt_tokens,
            "history_threshold": history_threshold,
            "limit": limit,
            "reset_mode": reset_mode,
        });
        if let Value::Object(ref mut map) = compaction_payload {
            compaction_round.insert_into(map);
        }
        emitter.emit("compaction", compaction_payload).await;

        Ok(rebuilt)
    }

    pub(super) async fn append_memory_prompt(&self, user_id: &str, prompt: String) -> String {
        if prompt.trim().is_empty() {
            return prompt;
        }
        if !self.memory_store.is_enabled_async(user_id).await {
            return prompt;
        }
        let records = self
            .memory_store
            .list_records_async(user_id, None, false)
            .await;
        let block = self.memory_store.build_prompt_block(&records);
        if block.is_empty() {
            return prompt;
        }
        format!("{}\n\n{}", prompt.trim_end(), block)
    }

    pub(super) fn load_memory_summary_prompt(&self) -> String {
        let prompt = read_prompt_template(Path::new(MEMORY_SUMMARY_PROMPT_PATH))
            .trim()
            .to_string();
        if prompt.is_empty() {
            i18n::t("memory.summary_prompt_fallback")
        } else {
            prompt
        }
    }

    pub(super) fn trim_attachments_for_memory(
        &self,
        attachments: Option<&[AttachmentPayload]>,
    ) -> Option<Vec<AttachmentPayload>> {
        let Some(attachments) = attachments else {
            return None;
        };
        if attachments.is_empty() {
            return None;
        }
        Some(
            attachments
                .iter()
                .map(|item| AttachmentPayload {
                    name: item.name.clone(),
                    content: None,
                    content_type: item.content_type.clone(),
                })
                .collect(),
        )
    }

    pub(super) fn format_memory_task(&self, task: &MemorySummaryTask, now_ts: f64) -> Value {
        let queued_ts = task.queued_time.max(0.0);
        let start_ts = task.start_time.max(0.0);
        let end_ts = task.end_time.max(0.0);
        let mut status = task.status.trim().to_string();
        if status.is_empty() {
            status = if end_ts > 0.0 {
                i18n::t("memory.status.done")
            } else if start_ts > 0.0 {
                i18n::t("memory.status.running")
            } else {
                i18n::t("memory.status.queued")
            };
        } else {
            let normalized = match status.to_lowercase().as_str() {
                "queued" | "排队中" => Some("queued"),
                "running" | "processing" | "正在处理" => Some("running"),
                "done" | "completed" | "已完成" => Some("done"),
                "failed" | "失败" => Some("failed"),
                _ => None,
            };
            if let Some(normalized) = normalized {
                status = match normalized {
                    "queued" => i18n::t("memory.status.queued"),
                    "running" => i18n::t("memory.status.running"),
                    "done" => i18n::t("memory.status.done"),
                    "failed" => i18n::t("memory.status.failed"),
                    _ => status,
                };
            }
        }

        fn format_ts(ts: f64) -> String {
            if ts <= 0.0 {
                return String::new();
            }
            Local
                .timestamp_opt(ts as i64, 0)
                .single()
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default()
        }

        let elapsed_s = if end_ts > 0.0 {
            let base_ts = if start_ts > 0.0 { start_ts } else { queued_ts };
            if base_ts > 0.0 {
                (end_ts - base_ts).max(0.0)
            } else {
                0.0
            }
        } else if start_ts > 0.0 {
            (now_ts - start_ts).max(0.0)
        } else if queued_ts > 0.0 {
            (now_ts - queued_ts).max(0.0)
        } else {
            0.0
        };

        json!({
            "task_id": task.task_id,
            "user_id": task.user_id,
            "session_id": task.session_id,
            "status": status,
            "queued_time": format_ts(queued_ts),
            "queued_time_ts": queued_ts,
            "started_time": format_ts(start_ts),
            "started_time_ts": start_ts,
            "finished_time": format_ts(end_ts),
            "finished_time_ts": end_ts,
            "elapsed_s": elapsed_s,
        })
    }

    pub(super) async fn find_memory_task(&self, task_id: &str) -> Option<MemorySummaryTask> {
        let state = self.memory_queue.state.lock().await;
        if let Some(active) = &state.active {
            if active.task_id == task_id {
                return Some(active.clone());
            }
        }
        for item in state.queue.iter() {
            if item.task.task_id == task_id {
                return Some(item.task.clone());
            }
        }
        for task in state.history.iter() {
            if task.task_id == task_id {
                return Some(task.clone());
            }
        }
        None
    }

    pub(super) async fn ensure_memory_worker(&self) {
        let mut state = self.memory_queue.state.lock().await;
        let should_spawn = state
            .worker
            .as_ref()
            .map(|handle| handle.is_finished())
            .unwrap_or(true);
        if !should_spawn {
            return;
        }
        let orchestrator = self.clone();
        state.worker = Some(tokio::spawn(async move {
            orchestrator.memory_worker_loop().await;
        }));
    }

    pub(super) async fn enqueue_memory_summary(
        &self,
        prepared: &PreparedRequest,
        request_messages: Option<Vec<Value>>,
        final_answer: &str,
    ) {
        if !self.memory_store.is_enabled_async(&prepared.user_id).await {
            return;
        }
        self.ensure_memory_worker().await;

        let task = MemorySummaryTask {
            task_id: Uuid::new_v4().simple().to_string(),
            user_id: prepared.user_id.clone(),
            session_id: prepared.session_id.clone(),
            queued_time: now_ts(),
            config_overrides: prepared.config_overrides.clone(),
            model_name: prepared.model_name.clone(),
            attachments: self.trim_attachments_for_memory(prepared.attachments.as_deref()),
            request_messages,
            language: prepared.language.clone(),
            status: "queued".to_string(),
            start_time: 0.0,
            end_time: 0.0,
            request_payload: None,
            final_answer: final_answer.trim().to_string(),
            summary_result: String::new(),
            error: String::new(),
        };

        {
            let mut state = self.memory_queue.state.lock().await;
            state.seq = state.seq.saturating_add(1);
            let seq = state.seq;
            state.queue.push(MemoryQueueItem {
                queued_time: task.queued_time,
                seq,
                task,
            });
        }
        self.memory_queue.notify.notify_one();
    }

    pub(super) async fn memory_worker_loop(self) {
        loop {
            let mut task = loop {
                let next = {
                    let mut state = self.memory_queue.state.lock().await;
                    state.queue.pop().map(|item| item.task)
                };
                match next {
                    Some(task) => break task,
                    None => self.memory_queue.notify.notified().await,
                }
            };

            let stored = i18n::with_language(task.language.clone(), async {
                task.start_time = now_ts();
                task.status = "running".to_string();
                {
                    let mut state = self.memory_queue.state.lock().await;
                    state.active = Some(task.clone());
                }

                match self.run_memory_summary_task(&mut task).await {
                    Ok(stored) => {
                        task.status = "done".to_string();
                        stored
                    }
                    Err(err) => {
                        task.status = "failed".to_string();
                        task.error = err.to_string();
                        warn!("记忆总结任务失败: {}", err);
                        false
                    }
                }
            })
            .await;

            task.end_time = now_ts();
            {
                let mut state = self.memory_queue.state.lock().await;
                state.active = None;
                state.history.push_front(task.clone());
                while state.history.len() > 100 {
                    state.history.pop_back();
                }
            }

            if stored {
                let base_ts = if task.start_time > 0.0 {
                    task.start_time
                } else {
                    task.queued_time
                };
                let elapsed_s = if base_ts > 0.0 && task.end_time > 0.0 {
                    (task.end_time - base_ts).max(0.0)
                } else {
                    0.0
                };
                self.memory_store
                    .upsert_task_log_async(
                        &task.user_id,
                        &task.session_id,
                        &task.task_id,
                        &task.status,
                        task.queued_time,
                        task.start_time,
                        task.end_time,
                        elapsed_s,
                        task.request_payload.as_ref(),
                        &task.summary_result,
                        &task.error,
                        Some(task.end_time),
                    )
                    .await;
            }
        }
    }

    pub(super) async fn run_memory_summary_task(
        &self,
        task: &mut MemorySummaryTask,
    ) -> Result<bool, OrchestratorError> {
        if !self.memory_store.is_enabled_async(&task.user_id).await {
            return Ok(false);
        }
        let config = self.resolve_config(task.config_overrides.as_ref()).await;
        let log_payload = is_debug_log_level(&config.observability.log_level);
        let (llm_name, llm_config) =
            self.resolve_llm_config(&config, task.model_name.as_deref())?;
        let mut summary_config = llm_config.clone();
        let max_output = summary_config.max_output.unwrap_or(0);
        if max_output == 0 || max_output as i64 > COMPACTION_SUMMARY_MAX_OUTPUT {
            summary_config.max_output = Some(COMPACTION_SUMMARY_MAX_OUTPUT as u32);
        }
        summary_config.max_rounds = Some(1);

        let messages = self
            .build_memory_summary_messages(task, &summary_config, &config)
            .await;
        if log_payload {
            let payload_messages =
                self.sanitize_messages_for_log(messages.clone(), task.attachments.as_deref());
            task.request_payload =
                Some(self.build_memory_summary_payload(task, &llm_name, payload_messages));
        } else {
            task.request_payload = None;
        }

        let emitter = EventEmitter::new(
            task.session_id.clone(),
            task.user_id.clone(),
            None,
            None,
            self.monitor.clone(),
        );
        let (content, _, _, _) = self
            .call_llm(
                &llm_config,
                &messages,
                &task.user_id,
                &emitter,
                &task.session_id,
                false,
                RoundInfo::default(),
                false,
                false,
                log_payload,
                None,
                Some(summary_config),
            )
            .await?;
        let summary_text = strip_tool_calls(&content);
        let normalized = MemoryStore::normalize_summary(&summary_text);
        task.summary_result = normalized.clone();
        Ok(self
            .memory_store
            .upsert_record_async(
                &task.user_id,
                &task.session_id,
                &normalized,
                Some(task.queued_time),
            )
            .await)
    }

    pub(super) async fn build_memory_summary_request_payload(
        &self,
        task: &MemorySummaryTask,
    ) -> Result<Value, OrchestratorError> {
        i18n::with_language(task.language.clone(), async {
            let config = self.resolve_config(task.config_overrides.as_ref()).await;
            if !is_debug_log_level(&config.observability.log_level) {
                return Err(OrchestratorError::internal(
                    "memory payload logging disabled".to_string(),
                ));
            }
            let (llm_name, llm_config) =
                self.resolve_llm_config(&config, task.model_name.as_deref())?;
            let mut summary_config = llm_config.clone();
            let max_output = summary_config.max_output.unwrap_or(0);
            if max_output == 0 || max_output as i64 > COMPACTION_SUMMARY_MAX_OUTPUT {
                summary_config.max_output = Some(COMPACTION_SUMMARY_MAX_OUTPUT as u32);
            }
            summary_config.max_rounds = Some(1);
            let messages = self
                .build_memory_summary_messages(task, &summary_config, &config)
                .await;
            let payload_messages =
                self.sanitize_messages_for_log(messages, task.attachments.as_deref());
            Ok(self.build_memory_summary_payload(task, &llm_name, payload_messages))
        })
        .await
    }

    pub(super) async fn build_memory_summary_messages(
        &self,
        task: &MemorySummaryTask,
        summary_llm_config: &LlmModelConfig,
        config: &Config,
    ) -> Vec<Value> {
        let summary_instruction = self.load_memory_summary_prompt();
        let source_messages = if let Some(request_messages) = &task.request_messages {
            request_messages.clone()
        } else {
            let history_manager = HistoryManager;
            history_manager.load_history_messages(
                &self.workspace,
                &task.user_id,
                &task.session_id,
                config.workspace.max_history_items,
            )
        };
        let user_content =
            self.build_memory_summary_user_content(&source_messages, &task.final_answer);
        let mut messages = vec![
            json!({ "role": "system", "content": summary_instruction }),
            json!({ "role": "user", "content": user_content }),
        ];
        messages = self.prepare_summary_messages(messages, COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
        if let Some(limit) = HistoryManager::get_auto_compact_limit(summary_llm_config) {
            if estimate_messages_tokens(&messages) > limit && messages.len() > 1 {
                let system_tokens = estimate_message_tokens(&messages[0]);
                let remaining = (limit - system_tokens).max(1);
                let tail = trim_messages_to_budget(messages.get(1..).unwrap_or(&[]), remaining);
                messages = vec![messages[0].clone()];
                messages.extend(tail);
            }
        }
        messages
    }

    pub(super) fn build_memory_summary_user_content(
        &self,
        messages: &[Value],
        final_answer: &str,
    ) -> String {
        let separator = i18n::t("memory.summary.role.separator");
        let user_label = i18n::t("memory.summary.role.user");
        let assistant_label = i18n::t("memory.summary.role.assistant");
        let mut lines: Vec<String> = Vec::new();
        let mut last_assistant = String::new();
        for message in messages {
            let Some(obj) = message.as_object() else {
                continue;
            };
            let role = obj.get("role").and_then(Value::as_str).unwrap_or("").trim();
            if role.is_empty() || role == "system" || role == "tool" {
                continue;
            }
            if Self::is_observation_message(role, obj.get("content").unwrap_or(&Value::Null)) {
                continue;
            }
            let content =
                self.extract_memory_summary_text(obj.get("content").unwrap_or(&Value::Null));
            if content.is_empty() {
                continue;
            }
            let label = if role == "user" {
                user_label.as_str()
            } else if role == "assistant" {
                assistant_label.as_str()
            } else {
                role
            };
            lines.push(format!("{label}{separator}{content}"));
            if role == "assistant" {
                last_assistant = content;
            }
        }
        let final_text = final_answer.trim();
        if !final_text.is_empty() && final_text != last_assistant {
            lines.push(format!("{assistant_label}{separator}{final_text}"));
        }
        lines.join("\n").trim().to_string()
    }

    pub(super) fn build_compaction_user_content(&self, messages: &[Value]) -> String {
        let separator = i18n::t("memory.summary.role.separator");
        let user_label = i18n::t("memory.summary.role.user");
        let assistant_label = i18n::t("memory.summary.role.assistant");
        let mut lines: Vec<String> = Vec::new();
        for message in messages {
            let Some(obj) = message.as_object() else {
                continue;
            };
            let role = obj.get("role").and_then(Value::as_str).unwrap_or("").trim();
            if role.is_empty() {
                continue;
            }
            let content =
                self.extract_memory_summary_text(obj.get("content").unwrap_or(&Value::Null));
            if content.is_empty() {
                continue;
            }
            let label = if role == "user" {
                user_label.as_str()
            } else if role == "assistant" {
                assistant_label.as_str()
            } else {
                role
            };
            lines.push(format!("{label}{separator}{content}"));
        }
        lines.join("\n").trim().to_string()
    }

    pub(super) fn extract_memory_summary_text(&self, content: &Value) -> String {
        match content {
            Value::Null => String::new(),
            Value::String(text) => strip_tool_calls(text).trim().to_string(),
            Value::Array(parts) => {
                let mut segments: Vec<String> = Vec::new();
                for part in parts {
                    let Some(obj) = part.as_object() else {
                        continue;
                    };
                    let part_type = obj
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_lowercase();
                    if part_type == "text" {
                        let text = obj.get("text").and_then(Value::as_str).unwrap_or("");
                        let cleaned = strip_tool_calls(text).trim().to_string();
                        if !cleaned.is_empty() {
                            segments.push(cleaned);
                        }
                        continue;
                    }
                    if part_type == "image_url" || obj.contains_key("image_url") {
                        segments.push(i18n::t("memory.summary.image_placeholder"));
                    }
                }
                segments.join("\n").trim().to_string()
            }
            other => strip_tool_calls(&other.to_string()).trim().to_string(),
        }
    }

    pub(super) fn is_observation_message(role: &str, content: &Value) -> bool {
        if role != "user" {
            return false;
        }
        let Value::String(text) = content else {
            return false;
        };
        text.starts_with(OBSERVATION_PREFIX)
    }

    pub(super) fn build_memory_summary_payload(
        &self,
        task: &MemorySummaryTask,
        llm_name: &str,
        messages: Vec<Value>,
    ) -> Value {
        let mut payload = json!({
            "user_id": task.user_id,
            "session_id": task.session_id,
            "model_name": llm_name,
            "tool_names": [],
            "messages": messages,
        });
        if let Some(overrides) = &task.config_overrides {
            if let Value::Object(ref mut map) = payload {
                map.insert("config_overrides".to_string(), overrides.clone());
            }
        }
        payload
    }

    pub(super) fn build_user_message(
        &self,
        question: &str,
        attachments: Option<&[AttachmentPayload]>,
    ) -> Value {
        let Some(attachments) = attachments else {
            return json!({ "role": "user", "content": question });
        };
        if attachments.is_empty() {
            return json!({ "role": "user", "content": question });
        }
        let attachment_label = i18n::t("attachment.label");
        let attachment_separator = i18n::t("attachment.label.separator");
        let attachment_default_name = i18n::t("attachment.default_name");
        let mut attachment_parts: Vec<String> = Vec::new();
        let mut image_parts: Vec<Value> = Vec::new();
        for attachment in attachments {
            let content = attachment.content.as_deref().unwrap_or("");
            if content.trim().is_empty() {
                continue;
            }
            let name = attachment
                .name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(&attachment_default_name);
            let display_name = Self::display_attachment_name(name);
            if is_image_attachment(attachment, content) {
                image_parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": content }
                }));
                continue;
            }
            attachment_parts.push(format!(
                "[{attachment_label}{attachment_separator}{display_name}]\n{content}"
            ));
        }
        let mut text_content = String::new();
        if !attachment_parts.is_empty() {
            text_content.push_str(&attachment_parts.join("\n\n"));
        }
        if !question.is_empty() {
            if !text_content.is_empty() {
                text_content.push_str("\n\n");
            }
            text_content.push_str(question);
        }
        if !image_parts.is_empty() {
            let text_payload = if text_content.trim().is_empty() {
                i18n::t("attachment.image_prompt")
            } else {
                text_content
            };
            let mut parts = vec![json!({ "type": "text", "text": text_payload })];
            parts.extend(image_parts);
            return json!({ "role": "user", "content": parts });
        }
        json!({ "role": "user", "content": text_content })
    }

    pub(super) fn sanitize_messages_for_log(
        &self,
        messages: Vec<Value>,
        attachments: Option<&[AttachmentPayload]>,
    ) -> Vec<Value> {
        if messages.is_empty() {
            return messages;
        }
        let image_names = attachments
            .unwrap_or(&[])
            .iter()
            .filter(|item| is_image_attachment(item, item.content.as_deref().unwrap_or("")))
            .map(|item| {
                item.name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or("image")
                    .to_string()
            })
            .collect::<Vec<_>>();
        let mut image_index = 0usize;
        let pattern = data_url_regex();

        let mut replace_data_url = |text: &str| {
            if !text.contains("data:image/") {
                return text.to_string();
            }
            let Some(pattern) = pattern else {
                return text.to_string();
            };
            let mut output = String::with_capacity(text.len());
            let mut last = 0usize;
            for m in pattern.find_iter(text) {
                output.push_str(&text[last..m.start()]);
                image_index += 1;
                let name = image_names
                    .get(image_index - 1)
                    .cloned()
                    .unwrap_or_else(|| format!("image-{image_index}"));
                output.push_str("attachment://");
                output.push_str(&name);
                last = m.end();
            }
            if last == 0 {
                return text.to_string();
            }
            output.push_str(&text[last..]);
            output
        };

        let mut sanitized = Vec::new();
        for message in messages {
            let Some(obj) = message.as_object() else {
                sanitized.push(message);
                continue;
            };
            let content = obj.get("content");
            if let Some(Value::String(text)) = content {
                let replaced = replace_data_url(text);
                if replaced != *text {
                    let mut new_message = obj.clone();
                    new_message.insert("content".to_string(), Value::String(replaced));
                    sanitized.push(Value::Object(new_message));
                } else {
                    sanitized.push(message);
                }
                continue;
            }
            if let Some(Value::Array(parts)) = content {
                let mut new_parts = Vec::new();
                let mut changed = false;
                for part in parts {
                    if let Some(part_obj) = part.as_object() {
                        let mut new_part = part_obj.clone();
                        let part_type = part_obj
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_lowercase();
                        if part_type == "image_url" || part_obj.contains_key("image_url") {
                            if let Some(url_value) = part_obj.get("image_url") {
                                let url = if let Some(obj) = url_value.as_object() {
                                    obj.get("url").and_then(Value::as_str)
                                } else {
                                    url_value.as_str()
                                };
                                if let Some(url) = url {
                                    if url.contains("data:image/") {
                                        let replaced = replace_data_url(url);
                                        if replaced == url {
                                            continue;
                                        }
                                        let mut image_obj = url_value.clone();
                                        if let Some(obj) = image_obj.as_object_mut() {
                                            obj.insert(
                                                "url".to_string(),
                                                Value::String(replaced.clone()),
                                            );
                                        } else {
                                            image_obj = json!({ "url": replaced });
                                        }
                                        new_part.insert("image_url".to_string(), image_obj);
                                        changed = true;
                                    }
                                }
                            }
                        }
                        if part_type == "text" {
                            if let Some(Value::String(text)) = part_obj.get("text") {
                                let replaced = replace_data_url(text);
                                if replaced != *text {
                                    new_part.insert("text".to_string(), Value::String(replaced));
                                    changed = true;
                                }
                            }
                        }
                        new_parts.push(Value::Object(new_part));
                    } else {
                        new_parts.push(part.clone());
                    }
                }
                if changed {
                    let mut new_message = obj.clone();
                    new_message.insert("content".to_string(), Value::Array(new_parts));
                    sanitized.push(Value::Object(new_message));
                } else {
                    sanitized.push(message);
                }
                continue;
            }
            sanitized.push(message);
        }
        sanitized
    }

    pub(super) fn display_attachment_name(name: &str) -> &str {
        let stem = Path::new(name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(name);
        if stem.is_empty() {
            name
        } else {
            stem
        }
    }
}

fn should_compact_by_context(
    context_tokens: i64,
    limit: i64,
    history_threshold: Option<i64>,
) -> (bool, bool) {
    let should_compact_by_history = history_threshold
        .map(|threshold| context_tokens >= threshold)
        .unwrap_or(false);
    let should_compact_by_overflow = context_tokens >= limit;
    (
        should_compact_by_history,
        should_compact_by_history || should_compact_by_overflow,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_compact_by_context_threshold() {
        let (by_history, should_compact) = should_compact_by_context(90, 100, Some(80));
        assert!(by_history);
        assert!(should_compact);
    }

    #[test]
    fn test_should_compact_by_context_overflow_only() {
        let (by_history, should_compact) = should_compact_by_context(120, 100, None);
        assert!(!by_history);
        assert!(should_compact);
    }

    #[test]
    fn test_should_compact_by_context_no_compaction() {
        let (by_history, should_compact) = should_compact_by_context(50, 100, Some(80));
        assert!(!by_history);
        assert!(!should_compact);
    }
}

fn is_image_attachment(attachment: &AttachmentPayload, content: &str) -> bool {
    let content_type = attachment
        .content_type
        .as_deref()
        .unwrap_or("")
        .to_lowercase();
    if content_type.starts_with("image") {
        return true;
    }
    if content_type.contains("image") {
        return true;
    }
    if content.starts_with("data:image/") {
        return true;
    }
    let name = attachment.name.as_deref().unwrap_or("").to_lowercase();
    matches!(
        Path::new(&name)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or(""),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    )
}

fn data_url_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| {
            compile_regex(
                r"data:image/[a-zA-Z0-9+.-]+;base64,[A-Za-z0-9+/=\r\n]+",
                "data_url",
            )
        })
        .as_ref()
}
