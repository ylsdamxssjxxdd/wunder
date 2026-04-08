use super::*;

const COMPACTION_MIN_CURRENT_USER_MESSAGE_TOKENS: i64 = 64;
const COMPACTION_RECENT_USER_WINDOW_TOKENS: i64 = 20_000;
const PROMPT_MEMORY_RECALL_LIMIT: usize = 30;
pub(super) const COMPACTION_SKIP_PERSIST_CURRENT_USER_META_KEY: &str =
    "compaction_skip_persist_current_user";
const COMPACTION_SUMMARY_REASONING_EFFORT: &str = "none";
const COMPACTION_SUMMARY_OBSERVATION_MAX_TOKENS: i64 = 256;
const COMPACTION_SUMMARY_TOOL_CALL_MAX_TOKENS: i64 = 96;
const COMPACTION_SUMMARY_MAX_TOOL_NAMES: usize = 4;

#[derive(Debug, Default)]
struct RebuiltContextGuardStats {
    applied: bool,
    tokens_before: i64,
    tokens_after: i64,
    current_user_trimmed: bool,
    current_user_tokens_before: i64,
    current_user_tokens_after: i64,
    summary_trimmed: bool,
    summary_tokens_before: i64,
    summary_tokens_after: i64,
    summary_removed: bool,
    fallback_trim_applied: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
enum CompactionResetMode {
    Zero,
    Current,
    Keep,
}

impl CompactionResetMode {
    fn from_config(raw: Option<&str>) -> Self {
        match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
            "current" => Self::Current,
            "keep" => Self::Keep,
            _ => Self::Zero,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::Current => "current",
            Self::Keep => "keep",
        }
    }

    fn keep_recent_user_window(self) -> bool {
        matches!(self, Self::Keep)
    }

    fn skip_persist_current_user(self) -> bool {
        matches!(self, Self::Zero)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CompactionBoundarySelection {
    boundary_index: Option<usize>,
    boundary_ts_override: Option<f64>,
}

impl Orchestrator {
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
        prepare_compaction_summary_messages(messages, max_tokens)
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

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn maybe_compact_messages(
        &self,
        config: &Config,
        llm_config: &LlmModelConfig,
        user_id: &str,
        agent_id: Option<&str>,
        session_id: &str,
        is_admin: bool,
        round_info: RoundInfo,
        messages: Vec<Value>,
        emitter: &EventEmitter,
        current_question: &str,
        log_payload: bool,
        persisted_context_tokens: i64,
        request_overhead_tokens: i64,
        force: bool,
        exclude_current_user: bool,
    ) -> Result<Vec<Value>, OrchestratorError> {
        let request_overhead_tokens = request_overhead_tokens.max(0);
        let persisted_context_tokens = persisted_context_tokens.max(0);
        let context_tokens = estimate_messages_tokens(&messages);
        let projected_request_tokens = resolve_projected_request_tokens(
            context_tokens,
            persisted_context_tokens,
            request_overhead_tokens,
        );
        let Some(limit) = resolve_compaction_limit(llm_config, projected_request_tokens, force)
        else {
            return Ok(messages);
        };
        let message_budget = resolve_message_budget(limit, request_overhead_tokens);
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
        let compaction_decision = super::compaction_policy::should_compact_by_context(
            projected_request_tokens,
            limit,
            history_threshold,
        );
        let mut should_compact_by_history = compaction_decision.by_history;
        let mut should_compact = compaction_decision.should_compact();
        if force && !should_compact {
            should_compact = true;
            if !should_compact_by_history {
                should_compact_by_history = true;
            }
        }
        let compaction_trigger = if force && !compaction_decision.should_compact() {
            "force"
        } else {
            compaction_decision.trigger()
        };
        let total_tokens = projected_request_tokens;
        let history_usage = context_tokens;
        if !should_compact {
            return Ok(messages);
        }

        let configured_reset_mode =
            CompactionResetMode::from_config(llm_config.history_compaction_reset.as_deref());

        let summary_text = if should_compact_by_history {
            i18n::t("compaction.reason.history_threshold")
        } else {
            i18n::t("compaction.reason.context_too_long")
        };
        let compaction_round = RoundInfo {
            user_round: round_info.user_round,
            model_round: None,
        };
        let mut compacting_payload = json!({
            "stage": "compacting",
            "summary": summary_text,
            "trigger": compaction_trigger,
            "presampling_limit": compaction_decision.presampling_limit,
        });
        if let Value::Object(ref mut map) = compacting_payload {
            compaction_round.insert_into(map);
        }
        emitter.emit("progress", compacting_payload).await;

        let system_message = messages
            .first()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
            .cloned();
        let current_user_index = if exclude_current_user {
            Self::locate_current_user_index(&messages)
        } else {
            None
        };
        let current_user_message = current_user_index
            .and_then(|index| messages.get(index))
            .cloned();
        let question_text = current_question.trim().to_string();
        let current_user_signature = current_user_message
            .as_ref()
            .map(|message| {
                self.extract_memory_summary_text(message.get("content").unwrap_or(&Value::Null))
            })
            .unwrap_or_default();
        let current_user_signature = current_user_signature.trim().to_string();
        let mut question_candidates: Vec<String> = Vec::new();
        if !question_text.is_empty() {
            question_candidates.push(question_text.clone());
        }
        if !current_user_signature.is_empty()
            && !question_candidates
                .iter()
                .any(|candidate| candidate == &current_user_signature)
        {
            question_candidates.push(current_user_signature.clone());
        }
        let current_user_has_non_text = current_user_message
            .as_ref()
            .is_some_and(message_has_non_text_content);
        let reset_mode =
            if configured_reset_mode == CompactionResetMode::Zero && current_user_has_non_text {
                CompactionResetMode::Current
            } else {
                configured_reset_mode
            };
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
        let mut artifact_content = if has_artifact {
            source_messages
                .iter()
                .find_map(|message| {
                    let obj = message.as_object()?;
                    if obj.get("role").and_then(Value::as_str) != Some("system") {
                        return None;
                    }
                    let content = obj
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    if artifact_prefixes
                        .iter()
                        .any(|prefix| content.starts_with(prefix))
                    {
                        Some(content.to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        if artifact_content.is_empty() {
            let history_manager = HistoryManager;
            artifact_content =
                history_manager.load_artifact_index_message(&self.workspace, user_id, session_id);
            if !artifact_content.is_empty() {
                source_messages
                    .push(json!({ "role": "system", "content": artifact_content.clone() }));
            }
        }

        let replay_system_message =
            merge_compaction_system_message(system_message.clone(), &artifact_content);
        let user_content = self.build_compaction_user_content(&source_messages);
        let has_candidates = !source_messages.is_empty();
        if user_content.trim().is_empty() && (!force || !has_candidates) {
            let mut guarded_messages = messages.clone();
            let guard_stats = apply_rebuilt_context_guard(&mut guarded_messages, message_budget);
            if guard_stats.applied {
                let mut guard_payload = json!({
                    "stage": "context_guard",
                    "summary": "Context guard trimmed oversized user input before model call.",
                    "tokens_before": guard_stats.tokens_before,
                    "tokens_after": guard_stats.tokens_after,
                    "request_overhead_tokens": request_overhead_tokens,
                    "message_budget": message_budget,
                    "current_user_trimmed": guard_stats.current_user_trimmed,
                    "summary_trimmed": guard_stats.summary_trimmed,
                    "summary_removed": guard_stats.summary_removed,
                    "fallback_trim_applied": guard_stats.fallback_trim_applied,
                });
                if let Value::Object(ref mut map) = guard_payload {
                    compaction_round.insert_into(map);
                }
                emitter.emit("progress", guard_payload).await;
            }
            let mut compaction_payload = json!({
                "reason": if should_compact_by_history { "history" } else { "overflow" },
                "trigger": compaction_trigger,
                "status": if guard_stats.applied { "guard_only" } else { "skipped" },
                "skip_reason": if has_candidates { "no_candidates" } else { "no_history" },
                "fresh_memory_injected": false,
                "fresh_memory_count": 0,
                "history_usage": history_usage,
                "context_tokens": history_usage,
                "persisted_context_tokens": persisted_context_tokens,
                "projected_request_tokens": projected_request_tokens,
                "request_overhead_tokens": request_overhead_tokens,
                "history_threshold": history_threshold,
                "limit": limit,
                "presampling_limit": compaction_decision.presampling_limit,
                "message_budget": message_budget,
                "total_tokens": total_tokens,
                "reset_mode": reset_mode.as_str(),
                "context_guard_applied": guard_stats.applied,
                "context_guard_tokens_before": guard_stats.tokens_before,
                "context_guard_tokens_after": guard_stats.tokens_after,
                "context_guard_current_user_trimmed": guard_stats.current_user_trimmed,
                "context_guard_summary_trimmed": guard_stats.summary_trimmed,
                "context_guard_summary_removed": guard_stats.summary_removed,
                "context_guard_fallback_trim_applied": guard_stats.fallback_trim_applied,
            });
            if let Value::Object(ref mut map) = compaction_payload {
                compaction_round.insert_into(map);
            }
            emitter.emit("compaction", compaction_payload).await;
            return Ok(if guard_stats.applied {
                guarded_messages
            } else {
                messages
            });
        }

        let compaction_prompt = HistoryManager::load_compaction_prompt();
        let compaction_instruction = build_compaction_instruction(
            &compaction_prompt,
            &artifact_content,
            &question_text,
            &current_user_signature,
        );
        let mut summary_input = messages.clone();
        let compaction_message = json!({ "role": "user", "content": compaction_instruction });
        if let Some(index) = current_user_index {
            summary_input[index] = compaction_message;
        } else {
            summary_input.push(compaction_message);
        }

        let mut compacted_until_ts: Option<f64> = None;
        let mut compacted_until: Option<String> = None;
        let history_limit = if is_admin {
            0
        } else {
            config.workspace.max_history_items
        };
        let history = self
            .workspace
            .load_history_async(user_id, session_id, history_limit)
            .await
            .unwrap_or_default();
        let (history_items, _) = HistoryManager::build_compaction_candidates(&history);
        let current_question_index =
            locate_matching_history_user_index(&history_items, &question_candidates);
        let boundary = select_compaction_boundary(
            &history_items,
            current_question_index,
            reset_mode,
            COMPACTION_RECENT_USER_WINDOW_TOKENS,
        );
        if let Some(value) = boundary.boundary_ts_override {
            compacted_until_ts = Some(value);
        } else if let Some(boundary_item) = boundary
            .boundary_index
            .and_then(|index| history_items.get(index))
        {
            compacted_until_ts = HistoryManager::get_item_timestamp(boundary_item);
            compacted_until = boundary_item
                .get("timestamp")
                .and_then(Value::as_str)
                .map(|value| value.to_string());
        }

        let summary_config = build_compaction_summary_config(llm_config);

        let summary_limit =
            HistoryManager::get_auto_compact_limit(&summary_config).unwrap_or(limit);
        let per_message_limit = summary_limit.clamp(1, COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
        summary_input = self.prepare_summary_messages(summary_input, per_message_limit);
        if estimate_messages_tokens(&summary_input) > summary_limit {
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
        let mut summary_fallback_reason: Option<&'static str> = None;
        let mut summary_failure_code: Option<String> = None;
        let mut summary_failure_message: Option<String> = None;
        let mut summary_failure_retryable: Option<bool> = None;
        let mut summary_model_output = match self
            .call_llm(
                llm_config,
                &summary_input,
                user_id,
                is_admin,
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
            Ok((content, _, _, _, _)) => self.resolve_final_answer(&content),
            Err(err) => {
                if err.code() == "USER_QUOTA_EXCEEDED" {
                    return Err(err);
                }
                summary_fallback = true;
                summary_fallback_reason = Some("llm_request_failed");
                summary_failure_code = Some(err.code().to_string());
                summary_failure_message =
                    Some(err.message().trim().chars().take(512).collect::<String>());
                summary_failure_retryable = Some(err.retryable());
                i18n::t("compaction.summary_fallback")
            }
        };
        let mut summary_text = HistoryManager::format_compaction_summary(&summary_model_output);
        if is_empty_compaction_summary(&summary_text) {
            summary_fallback = true;
            if summary_fallback_reason.is_none() {
                summary_fallback_reason = Some("empty_summary");
            }
            let fallback_content = if user_content.trim().is_empty() {
                i18n::t("compaction.summary_fallback")
            } else {
                user_content.clone()
            };
            summary_model_output = fallback_content.clone();
            summary_text = HistoryManager::format_compaction_summary(&fallback_content);
        }
        let (fresh_memory_block, fresh_memory_count, fresh_memory_total_count) = self
            .build_fresh_memory_block_for_compaction(
                config,
                user_id,
                agent_id,
                session_id,
                question_candidates.first().map(String::as_str),
            )
            .await;
        let (summary_text, fresh_memory_injected) =
            merge_compaction_summary_with_fresh_memory(&summary_text, &fresh_memory_block);
        let mut summary_text = summary_text;
        let recent_user_messages = if reset_mode.keep_recent_user_window() {
            collect_recent_user_messages_for_compaction(
                &source_messages,
                COMPACTION_RECENT_USER_WINDOW_TOKENS,
            )
        } else {
            Vec::new()
        };
        let recent_user_messages_retained = recent_user_messages.len();
        let recent_user_tokens_retained = estimate_messages_tokens(&recent_user_messages);
        let mut base_messages: Vec<Value> = Vec::new();
        if let Some(system_message) = replay_system_message.clone() {
            base_messages.push(system_message);
        }
        base_messages.extend(recent_user_messages.iter().cloned());
        if let Some(current_user_message) = current_user_message.clone() {
            base_messages.push(current_user_message);
        } else if !question_text.is_empty() {
            base_messages.push(json!({ "role": "user", "content": question_text.clone() }));
        }
        let base_tokens = estimate_messages_tokens(&base_messages);
        for _ in 0..3 {
            let summary_message = json!({ "role": "user", "content": summary_text });
            let total_tokens =
                base_tokens + estimate_message_tokens(&summary_message) + request_overhead_tokens;
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
        let injected_summary_text = summary_text.clone();
        let mut response_payload = json!({
            "content": summary_text.clone(),
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
        meta.insert(
            "reset_mode".to_string(),
            Value::String(reset_mode.as_str().to_string()),
        );
        let meta_value = Value::Object(meta);
        self.append_chat(
            user_id,
            session_id,
            "system",
            Some(&Value::String(summary_text.clone())),
            None,
            Some(&meta_value),
            None,
            None,
            None,
        );

        let mut current_user_message_for_history_trimmed = false;
        let current_user_message_for_history = current_user_message.as_ref().map(|message| {
            let mut candidate =
                if let Some(trimmed) = trim_message_to_fit_tokens(message, message_budget) {
                    current_user_message_for_history_trimmed = true;
                    trimmed
                } else {
                    message.clone()
                };
            if reset_mode.skip_persist_current_user() {
                mark_current_user_message_skip_persist(&mut candidate);
            }
            candidate
        });

        let mut rebuilt = Vec::new();
        if let Some(system_message) = replay_system_message {
            rebuilt.push(system_message);
        }
        rebuilt.extend(recent_user_messages);
        rebuilt.push(json!({ "role": "user", "content": summary_text }));
        if let Some(current_user_message) = current_user_message_for_history {
            rebuilt.push(current_user_message);
        } else if !question_text.is_empty() {
            rebuilt.push(json!({ "role": "user", "content": question_text }));
        }
        let mut rebuilt = self.shrink_messages_to_limit(rebuilt, message_budget);
        let guard_stats = apply_rebuilt_context_guard(&mut rebuilt, message_budget);
        let rebuilt_tokens = estimate_messages_tokens(&rebuilt);
        let rebuilt_request_tokens = rebuilt_tokens.saturating_add(request_overhead_tokens);

        if guard_stats.applied {
            let mut guard_payload = json!({
                "stage": "context_guard",
                "summary": "Context guard trimmed oversized compaction payload.",
                "tokens_before": guard_stats.tokens_before,
                "tokens_after": guard_stats.tokens_after,
                "request_overhead_tokens": request_overhead_tokens,
                "message_budget": message_budget,
                "current_user_replay_trimmed": current_user_message_for_history_trimmed,
                "current_user_trimmed": guard_stats.current_user_trimmed,
                "summary_trimmed": guard_stats.summary_trimmed,
                "summary_removed": guard_stats.summary_removed,
                "fallback_trim_applied": guard_stats.fallback_trim_applied,
            });
            if let Value::Object(ref mut map) = guard_payload {
                compaction_round.insert_into(map);
            }
            emitter.emit("progress", guard_payload).await;
        }

        let mut compaction_payload = json!({
            "reason": if should_compact_by_history { "history" } else { "overflow" },
            "trigger": compaction_trigger,
            "status": if summary_fallback { "fallback" } else { "done" },
            "summary_fallback": summary_fallback,
            "summary_model_output": summary_model_output,
            "summary_text": injected_summary_text,
            "fresh_memory_injected": fresh_memory_injected,
            "fresh_memory_count": fresh_memory_count,
            "fresh_memory_total_count": fresh_memory_total_count,
            "recent_user_messages_retained": recent_user_messages_retained,
            "recent_user_tokens_retained": recent_user_tokens_retained,
            "recent_user_window_token_limit": COMPACTION_RECENT_USER_WINDOW_TOKENS,
            "summary_tokens": approx_token_count(&summary_text),
            "total_tokens": total_tokens,
            "total_tokens_after": rebuilt_request_tokens,
            "history_usage": history_usage,
            "context_tokens": history_usage,
            "context_tokens_after": rebuilt_tokens,
            "persisted_context_tokens": persisted_context_tokens,
            "projected_request_tokens": projected_request_tokens,
            "projected_request_tokens_after": rebuilt_request_tokens,
            "request_overhead_tokens": request_overhead_tokens,
            "history_threshold": history_threshold,
            "limit": limit,
            "presampling_limit": compaction_decision.presampling_limit,
            "message_budget": message_budget,
            "reset_mode": reset_mode.as_str(),
            "configured_reset_mode": configured_reset_mode.as_str(),
            "current_user_non_text_preserved": current_user_has_non_text,
            "context_guard_applied": guard_stats.applied,
            "context_guard_tokens_before": guard_stats.tokens_before,
            "context_guard_tokens_after": guard_stats.tokens_after,
            "context_guard_current_user_replay_trimmed": current_user_message_for_history_trimmed,
            "context_guard_current_user_trimmed": guard_stats.current_user_trimmed,
            "context_guard_current_user_tokens_before": guard_stats.current_user_tokens_before,
            "context_guard_current_user_tokens_after": guard_stats.current_user_tokens_after,
            "context_guard_summary_trimmed": guard_stats.summary_trimmed,
            "context_guard_summary_tokens_before": guard_stats.summary_tokens_before,
            "context_guard_summary_tokens_after": guard_stats.summary_tokens_after,
            "context_guard_summary_removed": guard_stats.summary_removed,
            "context_guard_fallback_trim_applied": guard_stats.fallback_trim_applied,
        });
        if let Value::Object(ref mut map) = compaction_payload {
            if let Some(reason) = summary_fallback_reason {
                map.insert(
                    "summary_fallback_reason".to_string(),
                    Value::String(reason.to_string()),
                );
            }
            if let Some(code) = summary_failure_code {
                map.insert("summary_failure_code".to_string(), Value::String(code));
            }
            if let Some(message) = summary_failure_message {
                map.insert(
                    "summary_failure_message".to_string(),
                    Value::String(message),
                );
            }
            if let Some(retryable) = summary_failure_retryable {
                map.insert(
                    "summary_failure_retryable".to_string(),
                    Value::Bool(retryable),
                );
            }
            compaction_round.insert_into(map);
        }
        emitter.emit("compaction", compaction_payload).await;

        Ok(rebuilt)
    }

    async fn build_fresh_memory_block_for_compaction(
        &self,
        config: &Config,
        user_id: &str,
        agent_id: Option<&str>,
        session_id: &str,
        query_text: Option<&str>,
    ) -> (String, usize, usize) {
        let fragment_store =
            crate::services::memory_fragments::MemoryFragmentStore::new(self.storage.clone());
        let inventory = fragment_store
            .recall_for_prompt_inventory(
                Some(config),
                user_id,
                agent_id,
                Some(session_id),
                None,
                query_text,
                Some(PROMPT_MEMORY_RECALL_LIMIT),
            )
            .await;
        let hit_count = inventory.hits.len();
        let total_count = inventory.total_available;
        let block = fragment_store.build_prompt_block(&inventory.hits, total_count);
        (block, hit_count, total_count)
    }

    pub(crate) async fn force_compact_session(
        &self,
        user_id: &str,
        session_id: &str,
        is_admin: bool,
        model_name: Option<&str>,
        agent_id: Option<&str>,
        agent_prompt: Option<&str>,
    ) -> Result<Value, OrchestratorError> {
        let config = self.resolve_config(None).await;
        let log_payload = is_debug_log_level(&config.observability.log_level);
        let (_llm_name, llm_config) = self.resolve_llm_config(&config, model_name)?;
        let skills_snapshot = self.skills.read().await.clone();
        let user_tool_bindings =
            self.user_tool_manager
                .build_bindings(&config, &skills_snapshot, user_id);
        let allowed_tool_names = self.filter_tools_for_model_capability(
            self.resolve_allowed_tool_names(
                &config,
                &[],
                &skills_snapshot,
                Some(&user_tool_bindings),
            ),
            llm_config.support_vision.unwrap_or(false),
        );
        let tool_call_mode = crate::llm::resolve_tool_call_mode(&llm_config);
        let workspace_id = self.resolve_workspace_id(user_id, agent_id);
        let system_prompt = self
            .resolve_session_prompt(
                &config,
                None,
                &allowed_tool_names,
                tool_call_mode,
                &skills_snapshot,
                Some(&user_tool_bindings),
                user_id,
                &workspace_id,
                session_id,
                None,
                agent_id,
                is_admin,
                agent_prompt,
                None,
                None,
            )
            .await;

        let _ = self.workspace.flush_writes_async().await;
        let history_manager = HistoryManager;
        let context_manager = ContextManager;
        let mut messages = vec![json!({ "role": "system", "content": system_prompt })];
        let history_limit = if is_admin {
            0
        } else {
            config.workspace.max_history_items
        };
        let history_messages = history_manager
            .load_history_messages_async(
                self.workspace.clone(),
                user_id.to_string(),
                session_id.to_string(),
                history_limit,
            )
            .await;
        messages.extend(history_messages);
        let messages = context_manager.normalize_messages(messages);
        let manual_user_round = messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            .count() as i64
            + 1;
        let manual_round_info = RoundInfo::user_only(manual_user_round.max(1));
        let storage = self.storage.clone();
        let session_id_for_offset = session_id.to_string();
        let start_event_id = match tokio::task::spawn_blocking(move || {
            storage.get_max_stream_event_id(&session_id_for_offset)
        })
        .await
        {
            Ok(Ok(value)) => value,
            Ok(Err(err)) => {
                warn!("failed to load stream event offset for session {session_id}: {err}");
                0
            }
            Err(err) => {
                warn!("failed to load stream event offset for session {session_id}: {err}");
                0
            }
        };
        let (queue_tx, mut queue_rx) = mpsc::channel::<StreamSignal>(STREAM_EVENT_QUEUE_SIZE);
        let emitter = EventEmitter::new(
            session_id.to_string(),
            user_id.to_string(),
            Some(queue_tx),
            Some(self.storage.clone()),
            self.monitor.clone(),
            is_admin,
            start_event_id,
        );
        self.ensure_not_cancelled(session_id)?;
        let messages = match self
            .maybe_compact_messages(
                &config,
                &llm_config,
                user_id,
                agent_id,
                session_id,
                is_admin,
                manual_round_info,
                messages,
                &emitter,
                "",
                log_payload,
                0,
                0,
                true,
                false,
            )
            .await
        {
            Ok(messages) => messages,
            Err(err) => {
                let status = if err.code() == "CANCELLED" {
                    "cancelled"
                } else {
                    "failed"
                };
                let mut compaction_payload = json!({
                    "stage": "context_overflow_recovery",
                    "reason": "manual",
                    "trigger": "force",
                    "status": status,
                    "error_code": err.code(),
                    "error_message": err.message(),
                });
                if let Value::Object(ref mut map) = compaction_payload {
                    manual_round_info.insert_into(map);
                }
                emitter.emit("compaction", compaction_payload).await;
                emitter.finish().await;
                return Err(err);
            }
        };
        self.ensure_not_cancelled(session_id)?;
        let messages = context_manager.normalize_messages(messages);
        let context_tokens = context_manager.estimate_context_tokens(&messages);
        self.workspace
            .save_session_context_tokens_async(user_id, session_id, context_tokens)
            .await;
        let mut context_payload = json!({
            "context_tokens": context_tokens,
            "message_count": messages.len(),
        });
        if let Value::Object(ref mut map) = context_payload {
            manual_round_info.insert_into(map);
        }
        emitter.emit("context_usage", context_payload).await;
        emitter.finish().await;

        let mut compaction_payload: Option<Value> = None;
        let mut final_context_payload: Option<Value> = None;
        while let Ok(signal) = queue_rx.try_recv() {
            let StreamSignal::Event(event) = signal else {
                continue;
            };
            let payload = event
                .data
                .get("data")
                .cloned()
                .unwrap_or_else(|| event.data.clone());
            match event.event.as_str() {
                "compaction" => compaction_payload = Some(payload),
                "context_usage" => final_context_payload = Some(payload),
                _ => {}
            }
        }

        let mut response_payload = compaction_payload.unwrap_or_else(|| {
            json!({
                "status": "done",
            })
        });
        if let (Some(response_obj), Some(context_obj)) = (
            response_payload.as_object_mut(),
            final_context_payload.as_ref().and_then(Value::as_object),
        ) {
            if let Some(context_tokens) = context_obj.get("context_tokens").and_then(Value::as_i64) {
                response_obj.insert("final_context_tokens".to_string(), json!(context_tokens));
            }
            if let Some(message_count) = context_obj.get("message_count").and_then(Value::as_i64) {
                response_obj.insert("message_count".to_string(), json!(message_count));
            }
        }

        Ok(response_payload)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn append_memory_prompt(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        prompt: String,
        session_id: Option<&str>,
        round_id: Option<&str>,
        query_text: Option<&str>,
    ) -> String {
        if prompt.trim().is_empty() {
            return prompt;
        }

        fn collapse_blank_lines(text: &str) -> String {
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            let mut out = String::with_capacity(normalized.len());
            let mut run = 0usize;
            for ch in normalized.chars() {
                if ch == '\n' {
                    run = run.saturating_add(1);
                    if run <= 2 {
                        out.push('\n');
                    }
                } else {
                    run = 0;
                    out.push(ch);
                }
            }
            out.trim().to_string()
        }
        let prompt = strip_existing_memory_block_text(&prompt);
        let placeholder = crate::prompting::SYSTEM_PROMPT_MEMORY_PLACEHOLDER;
        let has_placeholder = prompt.contains(placeholder);
        let config = self.config_store.get().await;
        let fragment_store =
            crate::services::memory_fragments::MemoryFragmentStore::new(self.storage.clone());
        let inventory = fragment_store
            .recall_for_prompt_inventory(
                Some(&config),
                user_id,
                agent_id,
                session_id,
                round_id,
                query_text,
                Some(PROMPT_MEMORY_RECALL_LIMIT),
            )
            .await;
        let block = fragment_store.build_prompt_block(&inventory.hits, inventory.total_available);

        if has_placeholder {
            let replacement = block.trim();
            let updated = if replacement.is_empty() {
                prompt.replace(placeholder, "")
            } else {
                prompt.replace(placeholder, replacement)
            };
            return collapse_blank_lines(&updated);
        }

        if block.is_empty() {
            return prompt;
        }
        format!("{}\n\n{}", prompt.trim_end(), block)
    }

    pub(super) fn spawn_auto_memory_extraction(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        session_id: &str,
        round_id: Option<&str>,
        question: &str,
        answer: &str,
        llm_config: LlmModelConfig,
    ) {
        let storage = self.storage.clone();
        let config_store = self.config_store.clone();
        let http = self.http.clone();
        let language = i18n::get_language();
        let user_id = user_id.trim().to_string();
        let agent_id = agent_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let session_id = session_id.trim().to_string();
        let round_id = round_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let question = question.trim().to_string();
        let answer = answer.trim().to_string();

        // Auto extraction is strictly opt-in per agent. It never mutates the
        // frozen system prompt of the current thread; it only writes long-term memories.
        tokio::spawn(async move {
            i18n::with_language(language, async move {
                let enabled_storage = storage.clone();
                let enabled_user_id = user_id.clone();
                let enabled_agent_id = agent_id.clone();
                let enabled = match tokio::task::spawn_blocking(move || {
                    crate::services::memory_agent_settings::AgentMemorySettingsService::new(
                        enabled_storage,
                    )
                    .auto_extract_enabled(&enabled_user_id, enabled_agent_id.as_deref())
                })
                .await
                {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::warn!(
                            target: "wunder_server",
                            user_id = %user_id,
                            agent_id = %agent_id.as_deref().unwrap_or("__default__"),
                            session_id = %session_id,
                            "auto memory extraction settings check failed: {err}"
                        );
                        return;
                    }
                };
                if !enabled {
                    return;
                }

                let prep_storage = storage.clone();
                let prep_user_id = user_id.clone();
                let prep_agent_id = agent_id.clone();
                let prep_session_id = session_id.clone();
                let prep_round_id = round_id.clone();
                let prep_question = question.clone();
                let prep_answer = answer.clone();
                let prepared = match tokio::task::spawn_blocking(move || {
                    let service = crate::services::memory_auto_extract::MemoryAutoExtractService::new(prep_storage);
                    let window = service.build_recent_user_window(&prep_user_id, &prep_session_id, &prep_question);
                    let mut job = service.queue_turn_job(
                        &prep_user_id,
                        prep_agent_id.as_deref(),
                        &prep_session_id,
                        prep_round_id.as_deref(),
                        &prep_question,
                        &prep_answer,
                        &window,
                    )?;
                    service.mark_job_running(&mut job)?;
                    Ok::<_, anyhow::Error>((window, job))
                })
                .await
                {
                    Ok(Ok(result)) => result,
                    Ok(Err(err)) => {
                        tracing::warn!(
                            target: "wunder_server",
                            user_id = %user_id,
                            agent_id = %agent_id.as_deref().unwrap_or("__default__"),
                            session_id = %session_id,
                            "auto memory extraction job init failed: {err}"
                        );
                        return;
                    }
                    Err(err) => {
                        tracing::warn!(
                            target: "wunder_server",
                            user_id = %user_id,
                            agent_id = %agent_id.as_deref().unwrap_or("__default__"),
                            session_id = %session_id,
                            "auto memory extraction init task join failed: {err}"
                        );
                        return;
                    }
                };
                let (window, mut job) = prepared;

                let config = config_store.get().await;
                let prompt = crate::services::prompting::read_prompt_template_from_active_pack(
                    &config,
                    Path::new("prompts/memory_auto_extract.txt"),
                );
                let prompt = if prompt.trim().is_empty() {
                    default_auto_memory_extract_prompt().to_string()
                } else {
                    prompt.trim().to_string()
                };

                let mut extract_config = llm_config.clone();
                extract_config.max_rounds = Some(1);
                extract_config.max_output = Some(extract_config.max_output.unwrap_or(768).min(768));
                extract_config.temperature = Some(0.1);
                extract_config.stream = Some(false);
                extract_config.stream_include_usage = Some(false);

                if !is_llm_configured(&extract_config) {
                    let finalize_storage = storage.clone();
                    let finalize_job = job.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let mut job = finalize_job;
                        crate::services::memory_auto_extract::MemoryAutoExtractService::new(finalize_storage)
                            .finish_job_failed(&mut job, "memory auto extraction llm is not configured");
                    })
                    .await;
                    tracing::warn!(
                        target: "wunder_server",
                        user_id = %user_id,
                        agent_id = %agent_id.as_deref().unwrap_or("__default__"),
                        session_id = %session_id,
                        "auto memory extraction skipped because llm is not configured"
                    );
                    return;
                }

                let request_text = build_auto_memory_extract_request(&question, &answer, &window);
                let messages = vec![
                    ChatMessage {
                        role: "system".to_string(),
                        content: json!(prompt),
                        reasoning_content: None,
                        tool_calls: None,
                        tool_call_id: None,
                    },
                    ChatMessage {
                        role: "user".to_string(),
                        content: json!(request_text),
                        reasoning_content: None,
                        tool_calls: None,
                        tool_call_id: None,
                    },
                ];
                let client = build_llm_client(&extract_config, http.clone());
                let raw_output = match client.complete(&messages).await {
                    Ok(response) => response.content,
                    Err(err) => {
                        let finalize_storage = storage.clone();
                        let finalize_job = job.clone();
                        let error_message = err.to_string();
                        let _ = tokio::task::spawn_blocking(move || {
                            let mut job = finalize_job;
                            crate::services::memory_auto_extract::MemoryAutoExtractService::new(finalize_storage)
                                .finish_job_failed(&mut job, &error_message);
                        })
                        .await;
                        tracing::warn!(
                            target: "wunder_server",
                            user_id = %user_id,
                            agent_id = %agent_id.as_deref().unwrap_or("__default__"),
                            session_id = %session_id,
                            "auto memory extraction llm call failed: {err}"
                        );
                        return;
                    }
                };

                let apply_storage = storage.clone();
                let apply_user_id = user_id.clone();
                let apply_agent_id = agent_id.clone();
                let apply_session_id = session_id.clone();
                let apply_round_id = round_id.clone();
                let apply_output = raw_output.clone();
                match tokio::task::spawn_blocking(move || {
                    let service = crate::services::memory_auto_extract::MemoryAutoExtractService::new(apply_storage);
                    let run = (|| -> anyhow::Result<(crate::services::memory_auto_extract::MemoryAutoExtractOutcome, usize)> {
                        let items = crate::services::memory_auto_extract::MemoryAutoExtractService::parse_llm_response(&apply_output)?;
                        let extracted_count = items.len();
                        let outcome = service.apply_llm_candidates(
                            &apply_user_id,
                            apply_agent_id.as_deref(),
                            &apply_session_id,
                            apply_round_id.as_deref(),
                            items,
                        )?;
                        Ok((outcome, extracted_count))
                    })();
                    match run {
                        Ok((outcome, extracted_count)) => {
                            service.finish_job_success(&mut job, &outcome, extracted_count)?;
                            Ok::<_, anyhow::Error>(outcome)
                        }
                        Err(err) => {
                            service.finish_job_failed(&mut job, &err.to_string());
                            Err(err)
                        }
                    }
                })
                .await
                {
                    Ok(Ok(outcome)) => {
                        tracing::debug!(
                            target: "wunder_server",
                            user_id = %user_id,
                            agent_id = %agent_id.as_deref().unwrap_or("__default__"),
                            session_id = %session_id,
                            created = outcome.created,
                            updated = outcome.updated,
                            skipped = outcome.skipped,
                            "auto memory extraction finished"
                        );
                    }
                    Ok(Err(err)) => {
                        tracing::warn!(
                            target: "wunder_server",
                            user_id = %user_id,
                            agent_id = %agent_id.as_deref().unwrap_or("__default__"),
                            session_id = %session_id,
                            "auto memory extraction failed: {err}"
                        );
                    }
                    Err(err) => {
                        tracing::warn!(
                            target: "wunder_server",
                            user_id = %user_id,
                            agent_id = %agent_id.as_deref().unwrap_or("__default__"),
                            session_id = %session_id,
                            "auto memory extraction apply task join failed: {err}"
                        );
                    }
                }
            }).await;
        });
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
        extract_memory_summary_text_value(content)
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

    pub(super) async fn build_user_message(
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
            let trimmed = content.trim();
            if content.trim().is_empty()
                && !attachment
                    .public_path
                    .as_deref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
            {
                continue;
            }
            let name = attachment
                .name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(&attachment_default_name);
            let display_name = Self::display_attachment_name(name);
            if is_image_attachment(attachment, trimmed) {
                if let Some(image_url) =
                    crate::services::chat_media::load_image_attachment_data_url(
                        &self.workspace,
                        attachment,
                    )
                    .await
                {
                    image_parts.push(json!({
                        "type": "image_url",
                        "image_url": { "url": image_url }
                    }));
                } else {
                    let fallback = attachment
                        .public_path
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or(trimmed);
                    if !fallback.is_empty() {
                        attachment_parts.push(format!(
                            "[{attachment_label}{attachment_separator}{display_name}]\n{fallback}"
                        ));
                    }
                }
                continue;
            }
            if trimmed.is_empty() {
                continue;
            }
            attachment_parts.push(format!(
                "[{attachment_label}{attachment_separator}{display_name}]\n{trimmed}"
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

fn apply_rebuilt_context_guard(messages: &mut Vec<Value>, limit: i64) -> RebuiltContextGuardStats {
    let mut stats = RebuiltContextGuardStats {
        tokens_before: estimate_messages_tokens(messages),
        ..Default::default()
    };
    if limit <= 0 || stats.tokens_before <= limit || messages.is_empty() {
        stats.tokens_after = stats.tokens_before;
        return stats;
    }

    stats.applied = true;

    let mut total_tokens = estimate_messages_tokens(messages);

    if total_tokens > limit {
        if let Some(summary_index) = locate_compaction_summary_message_index(messages) {
            stats.summary_tokens_before = estimate_message_tokens(&messages[summary_index]);
            let remaining_for_summary =
                (limit - (total_tokens - stats.summary_tokens_before)).max(1);
            if let Some(trimmed) =
                trim_message_to_fit_tokens(&messages[summary_index], remaining_for_summary)
            {
                stats.summary_tokens_after = estimate_message_tokens(&trimmed);
                stats.summary_trimmed = stats.summary_tokens_after < stats.summary_tokens_before;
                messages[summary_index] = trimmed;
            } else {
                stats.summary_tokens_after = stats.summary_tokens_before;
            }
        }
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        let summary_index = locate_compaction_summary_message_index(messages);
        let current_user_index = messages
            .iter()
            .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"));
        if let (Some(summary_index), Some(current_user_index)) = (summary_index, current_user_index)
        {
            if summary_index != current_user_index && summary_index < messages.len() {
                messages.remove(summary_index);
                stats.summary_removed = true;
                total_tokens = estimate_messages_tokens(messages);
            }
        }
    }

    if total_tokens > limit {
        let summary_index = locate_compaction_summary_message_index(messages);
        let current_user_index = messages
            .iter()
            .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"));
        if let (Some(summary_index), Some(current_user_index)) = (summary_index, current_user_index)
        {
            if current_user_index != summary_index {
                stats.current_user_tokens_before =
                    estimate_message_tokens(&messages[current_user_index]);
                let preserve_floor = stats
                    .current_user_tokens_before
                    .clamp(1, COMPACTION_MIN_CURRENT_USER_MESSAGE_TOKENS);
                let remaining_for_current =
                    limit - (total_tokens - stats.current_user_tokens_before);
                let target_tokens = remaining_for_current
                    .max(preserve_floor)
                    .min(stats.current_user_tokens_before);
                // Keep the active user intent readable whenever the limit still allows it.
                if target_tokens < stats.current_user_tokens_before {
                    if let Some(trimmed) =
                        trim_message_to_fit_tokens(&messages[current_user_index], target_tokens)
                    {
                        stats.current_user_tokens_after = estimate_message_tokens(&trimmed);
                        stats.current_user_trimmed =
                            stats.current_user_tokens_after < stats.current_user_tokens_before;
                        messages[current_user_index] = trimmed;
                        total_tokens = estimate_messages_tokens(messages);
                    } else {
                        stats.current_user_tokens_after = stats.current_user_tokens_before;
                    }
                } else {
                    stats.current_user_tokens_after = stats.current_user_tokens_before;
                }
            }
        }
    }

    if total_tokens > limit {
        if let Some(last_index) = messages.len().checked_sub(1) {
            let last_tokens = estimate_message_tokens(&messages[last_index]);
            let current_user_index = messages
                .iter()
                .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"));
            let trimming_current_user = current_user_index == Some(last_index);
            if trimming_current_user && stats.current_user_tokens_before == 0 {
                stats.current_user_tokens_before = last_tokens;
            }
            let remaining_for_last = (limit - (total_tokens - last_tokens)).max(1);
            if let Some(trimmed) =
                trim_message_to_fit_tokens(&messages[last_index], remaining_for_last)
            {
                let trimmed_tokens = estimate_message_tokens(&trimmed);
                if trimming_current_user {
                    stats.current_user_tokens_after = trimmed_tokens;
                    stats.current_user_trimmed |= trimmed_tokens < stats.current_user_tokens_before;
                }
                messages[last_index] = trimmed;
                total_tokens = estimate_messages_tokens(messages);
            }
        }
    }

    if total_tokens > limit {
        *messages = trim_messages_to_budget(messages, limit);
        stats.fallback_trim_applied = true;
        total_tokens = estimate_messages_tokens(messages);
        if total_tokens > limit {
            if let Some(last_index) = messages.len().checked_sub(1) {
                if let Some(trimmed) =
                    trim_message_to_fit_tokens(&messages[last_index], limit.max(1))
                {
                    *messages = vec![trimmed];
                    total_tokens = estimate_messages_tokens(messages);
                }
            }
        }
    }

    stats.tokens_after = total_tokens;
    stats
}

fn trim_message_to_fit_tokens(message: &Value, max_tokens: i64) -> Option<Value> {
    if max_tokens <= 0 || estimate_message_tokens(message) <= max_tokens {
        return None;
    }
    let mut message_obj = message.as_object().cloned().unwrap_or_else(|| {
        let mut fallback = serde_json::Map::new();
        fallback.insert("role".to_string(), Value::String("user".to_string()));
        fallback.insert("content".to_string(), message.clone());
        fallback
    });
    let source = extract_guard_content_text(message_obj.get("content").unwrap_or(&Value::Null));
    let source = if source.trim().is_empty() {
        i18n::t("compaction.summary_fallback")
    } else {
        source
    };
    let mut target_tokens = max_tokens.max(1);
    let mut trimmed_message: Option<Value> = None;
    for _ in 0..4 {
        let content = trim_text_to_tokens(&source, target_tokens, "...(truncated)");
        message_obj.insert("content".to_string(), Value::String(content));
        message_obj.remove("reasoning_content");
        message_obj.remove("reasoning");
        let candidate = Value::Object(message_obj.clone());
        let cost = estimate_message_tokens(&candidate);
        trimmed_message = Some(candidate.clone());
        if cost <= max_tokens {
            break;
        }
        let overflow = cost - max_tokens;
        let next_target = (target_tokens - overflow).max(1);
        if next_target == target_tokens {
            break;
        }
        target_tokens = next_target;
    }
    trimmed_message
}

fn extract_guard_content_text(content: &Value) -> String {
    extract_memory_summary_text_value(content)
}

fn extract_memory_summary_text_value(content: &Value) -> String {
    match content {
        Value::Null => String::new(),
        Value::String(text) => strip_tool_calls(text).trim().to_string(),
        Value::Array(parts) => {
            let mut segments = Vec::new();
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
                } else if part_type == "image_url" || obj.contains_key("image_url") {
                    segments.push(i18n::t("memory.summary.image_placeholder"));
                } else if let Some(text) = obj.get("text").and_then(Value::as_str) {
                    let cleaned = strip_tool_calls(text).trim().to_string();
                    if !cleaned.is_empty() {
                        segments.push(cleaned);
                    }
                }
            }
            segments.join("\n").trim().to_string()
        }
        other => strip_tool_calls(&other.to_string()).trim().to_string(),
    }
}

fn prepare_compaction_summary_messages(messages: Vec<Value>, max_tokens: i64) -> Vec<Value> {
    if messages.is_empty() {
        return messages;
    }
    let target = max_tokens.max(1);
    let mut prepared = Vec::with_capacity(messages.len());
    let mut merged_system_blocks: Vec<String> = Vec::new();
    for message in messages {
        let Some(obj) = message.as_object() else {
            continue;
        };
        let role = obj
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if role.is_empty() {
            continue;
        }
        let Some(mut content) = extract_compaction_summary_message_text(role.as_str(), obj) else {
            continue;
        };
        let per_message_target = if is_compaction_observation_message(role.as_str(), obj) {
            target.min(COMPACTION_SUMMARY_OBSERVATION_MAX_TOKENS)
        } else if role == "assistant" && has_compaction_summary_tool_calls(obj) {
            target.min(COMPACTION_SUMMARY_TOOL_CALL_MAX_TOKENS)
        } else {
            target
        };
        if approx_token_count(&content) > per_message_target {
            content = trim_text_to_tokens(&content, per_message_target, "...(truncated)");
        }
        let normalized_role = normalize_compaction_summary_role(role.as_str());
        if normalized_role == "system" {
            merged_system_blocks.push(content);
            continue;
        }
        prepared.push(json!({
            "role": normalized_role,
            "content": content,
        }));
    }
    if !merged_system_blocks.is_empty() {
        let merged = merged_system_blocks.join("\n\n");
        let merged = if approx_token_count(&merged) > target {
            trim_text_to_tokens(&merged, target, "...(truncated)")
        } else {
            merged
        };
        prepared.insert(0, json!({ "role": "system", "content": merged }));
    }
    prepared
}

fn normalize_compaction_summary_role(role: &str) -> &'static str {
    match role {
        "system" => "system",
        "assistant" => "assistant",
        _ => "user",
    }
}

fn extract_compaction_summary_message_text(role: &str, obj: &Map<String, Value>) -> Option<String> {
    let content = obj.get("content").unwrap_or(&Value::Null);
    let text = if is_compaction_observation_message(role, obj) {
        summarize_compaction_observation(content)
    } else {
        extract_memory_summary_text_value(content)
    };
    if !text.is_empty() {
        return Some(text);
    }
    if role == "assistant" {
        let tool_summary = summarize_compaction_tool_calls(
            obj.get("tool_calls")
                .or_else(|| obj.get("tool_call"))
                .or_else(|| obj.get("function_call")),
        );
        if !tool_summary.is_empty() {
            return Some(tool_summary);
        }
    }
    None
}

fn is_compaction_observation_message(role: &str, obj: &Map<String, Value>) -> bool {
    let content = obj.get("content").unwrap_or(&Value::Null);
    Orchestrator::is_observation_message(role, content)
}

fn has_compaction_summary_tool_calls(obj: &Map<String, Value>) -> bool {
    obj.get("tool_calls")
        .or_else(|| obj.get("tool_call"))
        .or_else(|| obj.get("function_call"))
        .is_some()
}

fn summarize_compaction_observation(content: &Value) -> String {
    let Some(raw) = content.as_str() else {
        return extract_memory_summary_text_value(content);
    };
    let payload_text = raw.trim_start_matches(OBSERVATION_PREFIX).trim();
    if payload_text.is_empty() {
        return String::new();
    }
    let Ok(payload) = serde_json::from_str::<Value>(payload_text) else {
        return strip_tool_calls(payload_text).trim().to_string();
    };
    let Some(map) = payload.as_object() else {
        return extract_memory_summary_text_value(&payload);
    };
    let tool_name = map
        .get("tool")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let status = match map.get("ok").and_then(Value::as_bool) {
        Some(true) => "success",
        Some(false) => "failed",
        None => "recorded",
    };
    let mut headline = format!("Tool observation ({tool_name}): {status}");
    if let Some(code) = map
        .get("error_code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headline.push_str(&format!(" [{code}]"));
    }
    let detail = extract_compaction_observation_detail(map);
    if detail.is_empty() {
        headline
    } else {
        format!("{headline}\n{detail}")
    }
}

fn extract_compaction_observation_detail(map: &Map<String, Value>) -> String {
    for key in ["error", "message", "summary", "preview"] {
        if let Some(value) = map.get(key) {
            if let Some(text) = extract_compaction_observation_text_candidate(value) {
                return text;
            }
        }
    }
    map.get("data")
        .and_then(extract_compaction_observation_text_candidate)
        .unwrap_or_default()
}

fn extract_compaction_observation_text_candidate(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(_) | Value::Array(_) => {
            let text = extract_memory_summary_text_value(value);
            let cleaned = text.trim();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned.to_string())
            }
        }
        Value::Object(map) => {
            for key in [
                "failure_summary",
                "error_detail_head",
                "summary",
                "preview",
                "result",
                "message",
                "stderr",
                "stdout",
                "content",
                "text",
                "structured_content",
            ] {
                if let Some(text) = map
                    .get(key)
                    .and_then(extract_compaction_observation_text_candidate)
                {
                    return Some(text);
                }
            }
            None
        }
        other => {
            let text = strip_tool_calls(&other.to_string()).trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
    }
}

fn summarize_compaction_tool_calls(tool_calls: Option<&Value>) -> String {
    let Some(tool_calls) = tool_calls else {
        return String::new();
    };
    let mut tool_names = collect_compaction_tool_call_names(tool_calls);
    tool_names.dedup();
    if tool_names.is_empty() {
        return "Assistant issued tool call(s).".to_string();
    }
    let hidden = tool_names
        .len()
        .saturating_sub(COMPACTION_SUMMARY_MAX_TOOL_NAMES);
    tool_names.truncate(COMPACTION_SUMMARY_MAX_TOOL_NAMES);
    let mut summary = format!("Assistant issued tool call(s): {}", tool_names.join(", "));
    if hidden > 0 {
        summary.push_str(&format!(" (+{hidden} more)"));
    }
    summary
}

fn collect_compaction_tool_call_names(tool_calls: &Value) -> Vec<String> {
    match tool_calls {
        Value::Array(items) => items
            .iter()
            .flat_map(collect_compaction_tool_call_names)
            .collect(),
        Value::Object(map) => map
            .get("function")
            .and_then(Value::as_object)
            .and_then(|function| function.get("name"))
            .or_else(|| map.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| vec![value.to_string()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn build_compaction_instruction(
    compaction_prompt: &str,
    artifact_content: &str,
    current_question: &str,
    current_user_signature: &str,
) -> String {
    let mut blocks: Vec<String> = Vec::new();
    let prompt = compaction_prompt.trim();
    if !prompt.is_empty() {
        blocks.push(prompt.to_string());
    }
    let artifact = artifact_content.trim();
    if !artifact.is_empty() {
        blocks.push(artifact.to_string());
    }

    let mut request_candidates: Vec<String> = Vec::new();
    let question = current_question.trim();
    if !question.is_empty() {
        request_candidates.push(question.to_string());
    }
    let signature = current_user_signature.trim();
    if !signature.is_empty()
        && !request_candidates
            .iter()
            .any(|candidate| candidate == signature)
    {
        request_candidates.push(signature.to_string());
    }
    if !request_candidates.is_empty() {
        let request_block = request_candidates.join("\n");
        blocks.push(format!(
            "[Current user request / 当前用户问题]\n{request_block}\n\n[Compaction constraints / 压缩约束]\n- Treat the request above as explicit task context.\n- Do not write placeholder claims such as \"task unspecified\".\n- If evidence is missing, state \"Insufficient evidence in context\"."
        ));
    }

    blocks.join("\n\n")
}

fn merge_compaction_system_message(
    system_message: Option<Value>,
    artifact_content: &str,
) -> Option<Value> {
    let artifact = artifact_content.trim();
    match system_message {
        Some(mut message) => {
            if artifact.is_empty() {
                return Some(message);
            }
            let existing_text = message
                .as_object()
                .and_then(|obj| obj.get("content"))
                .map(flatten_compaction_system_content)
                .unwrap_or_default()
                .trim()
                .to_string();
            let merged_content = if existing_text.is_empty() {
                artifact.to_string()
            } else if existing_text.contains(artifact) {
                existing_text
            } else {
                format!("{existing_text}\n\n{artifact}")
            };
            if let Some(obj) = message.as_object_mut() {
                obj.insert("role".to_string(), Value::String("system".to_string()));
                obj.insert("content".to_string(), Value::String(merged_content));
                Some(message)
            } else {
                Some(json!({ "role": "system", "content": merged_content }))
            }
        }
        None => {
            if artifact.is_empty() {
                None
            } else {
                Some(json!({ "role": "system", "content": artifact }))
            }
        }
    }
}

fn flatten_compaction_system_content(content: &Value) -> String {
    match content {
        Value::String(text) => text.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .or_else(|| item.get("text").and_then(Value::as_str))
                    .or_else(|| item.get("content").and_then(Value::as_str))
                    .map(ToString::to_string)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .or_else(|| map.get("content").and_then(Value::as_str))
            .unwrap_or("")
            .to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn build_compaction_summary_config(llm_config: &LlmModelConfig) -> LlmModelConfig {
    let mut summary_config = llm_config.clone();
    let max_output = llm_config
        .max_output
        .unwrap_or(COMPACTION_SUMMARY_MAX_OUTPUT as u32)
        .min(COMPACTION_SUMMARY_MAX_OUTPUT as u32);
    summary_config.max_output = Some(max_output);
    summary_config.max_rounds = Some(1);
    // Disable reasoning for compaction summaries to keep the auxiliary request lean.
    summary_config.reasoning_effort = Some(COMPACTION_SUMMARY_REASONING_EFFORT.to_string());
    summary_config
}

fn strip_existing_memory_block_text(text: &str) -> String {
    let cleaned = text.trim_end();
    let mut prefixes = i18n::get_known_prefixes("memory.block_prefix");
    if prefixes.is_empty() {
        prefixes.push(i18n::t("memory.block_prefix"));
    }
    let mut cut_index: Option<usize> = None;
    for prefix in prefixes {
        let marker = prefix.trim();
        if marker.is_empty() {
            continue;
        }
        if let Some(index) = cleaned.find(marker) {
            cut_index = Some(cut_index.map_or(index, |current| current.min(index)));
        }
    }
    if let Some(index) = cut_index {
        cleaned[..index].trim_end().to_string()
    } else {
        cleaned.to_string()
    }
}

fn merge_compaction_summary_with_fresh_memory(
    summary_text: &str,
    memory_block: &str,
) -> (String, bool) {
    let summary_without_memory = strip_existing_memory_block_text(summary_text);
    let memory_block = memory_block.trim();
    if memory_block.is_empty() {
        return (summary_without_memory, false);
    }
    let summary_without_memory = summary_without_memory.trim_end();
    if summary_without_memory.is_empty() {
        return (memory_block.to_string(), true);
    }
    (format!("{summary_without_memory}\n\n{memory_block}"), true)
}

fn is_empty_compaction_summary(summary: &str) -> bool {
    let cleaned = summary.trim();
    if cleaned.is_empty() {
        return true;
    }
    let empty_summary = i18n::t("memory.empty_summary");
    if cleaned == empty_summary.trim() {
        return true;
    }
    let mut prefixes = i18n::get_known_prefixes("history.compaction_prefix");
    if prefixes.is_empty() {
        prefixes.push(i18n::t("history.compaction_prefix"));
    }
    for prefix in prefixes {
        if let Some(rest) = cleaned.strip_prefix(prefix.as_str()) {
            let rest = rest.trim();
            if rest.is_empty() || rest == empty_summary.trim() {
                return true;
            }
        }
    }
    false
}

fn compaction_prefixes() -> Vec<String> {
    let mut prefixes = i18n::get_known_prefixes("history.compaction_prefix");
    if prefixes.is_empty() {
        prefixes.push(i18n::t("history.compaction_prefix"));
    }
    prefixes
}

fn starts_with_compaction_prefix(text: &str) -> bool {
    let cleaned = text.trim();
    if cleaned.is_empty() {
        return false;
    }
    compaction_prefixes()
        .iter()
        .map(|prefix| prefix.trim())
        .any(|prefix| !prefix.is_empty() && cleaned.starts_with(prefix))
}

fn message_has_non_text_content(message: &Value) -> bool {
    let content = message.get("content").unwrap_or(&Value::Null);
    match content {
        Value::Array(parts) => parts.iter().any(|part| {
            let Some(obj) = part.as_object() else {
                return false;
            };
            let part_type = obj
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            part_type != "text" && (!part_type.is_empty() || obj.contains_key("image_url"))
        }),
        Value::Object(map) => {
            let part_type = map
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            part_type != "text" && (!part_type.is_empty() || map.contains_key("image_url"))
        }
        _ => false,
    }
}

fn mark_current_user_message_skip_persist(message: &mut Value) {
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
        COMPACTION_SKIP_PERSIST_CURRENT_USER_META_KEY.to_string(),
        Value::Bool(true),
    );
}

fn locate_matching_history_user_index(
    history_items: &[Value],
    question_candidates: &[String],
) -> Option<usize> {
    if question_candidates.is_empty() {
        return None;
    }
    history_items.iter().rposition(|item| {
        let role = item.get("role").and_then(Value::as_str).unwrap_or("");
        if role != "user" {
            return false;
        }
        let content = extract_guard_content_text(item.get("content").unwrap_or(&Value::Null));
        !content.is_empty()
            && question_candidates
                .iter()
                .any(|candidate| candidate.trim() == content.trim())
    })
}

fn select_recent_user_window_start_index(
    history_items: &[Value],
    token_limit: i64,
) -> Option<usize> {
    if token_limit <= 0 {
        return None;
    }
    let mut remaining = token_limit.max(0);
    let mut earliest_index: Option<usize> = None;

    for (index, item) in history_items.iter().enumerate().rev() {
        if remaining <= 0 {
            break;
        }
        if item.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let content = item.get("content").cloned().unwrap_or(Value::Null);
        let text = extract_guard_content_text(&content);
        if text.is_empty() || starts_with_compaction_prefix(&text) {
            continue;
        }
        let message = json!({ "role": "user", "content": content });
        let message_tokens = estimate_message_tokens(&message).max(1);
        earliest_index = Some(index);
        if message_tokens >= remaining {
            break;
        }
        remaining = remaining.saturating_sub(message_tokens);
    }

    earliest_index
}

fn select_compaction_boundary(
    history_items: &[Value],
    current_question_index: Option<usize>,
    reset_mode: CompactionResetMode,
    recent_user_window_tokens: i64,
) -> CompactionBoundarySelection {
    let last_index = history_items.len().checked_sub(1);
    match reset_mode {
        CompactionResetMode::Zero => CompactionBoundarySelection {
            boundary_index: current_question_index.or(last_index),
            boundary_ts_override: None,
        },
        CompactionResetMode::Current => CompactionBoundarySelection {
            boundary_index: current_question_index
                .and_then(|index| index.checked_sub(1))
                .or(last_index),
            boundary_ts_override: None,
        },
        CompactionResetMode::Keep => {
            match select_recent_user_window_start_index(history_items, recent_user_window_tokens) {
                Some(0) => CompactionBoundarySelection {
                    boundary_index: None,
                    boundary_ts_override: history_items
                        .first()
                        .and_then(HistoryManager::get_item_timestamp)
                        .map(|value| value - 0.001),
                },
                Some(index) => CompactionBoundarySelection {
                    boundary_index: index.checked_sub(1),
                    boundary_ts_override: None,
                },
                None => CompactionBoundarySelection {
                    boundary_index: last_index,
                    boundary_ts_override: None,
                },
            }
        }
    }
}

fn locate_compaction_summary_message_index(messages: &[Value]) -> Option<usize> {
    messages.iter().position(|message| {
        if message.get("role").and_then(Value::as_str) != Some("user") {
            return false;
        }
        let content = message.get("content").unwrap_or(&Value::Null);
        let text = extract_guard_content_text(content);
        starts_with_compaction_prefix(&text)
    })
}

fn collect_recent_user_messages_for_compaction(messages: &[Value], token_limit: i64) -> Vec<Value> {
    if token_limit <= 0 {
        return Vec::new();
    }
    let mut remaining = token_limit.max(0);
    let mut selected_rev: Vec<Value> = Vec::new();

    for message in messages.iter().rev() {
        if remaining <= 0 {
            break;
        }
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        if role != "user" {
            continue;
        }
        let content = message.get("content").unwrap_or(&Value::Null);
        if Orchestrator::is_observation_message(role, content) {
            continue;
        }
        let content_text = extract_guard_content_text(content);
        if content_text.is_empty() || starts_with_compaction_prefix(&content_text) {
            continue;
        }

        let message_tokens = estimate_message_tokens(message);
        if message_tokens <= remaining {
            selected_rev.push(message.clone());
            remaining = remaining.saturating_sub(message_tokens);
            continue;
        }

        let target_tokens = remaining.max(1);
        if let Some(trimmed) = trim_message_to_fit_tokens(message, target_tokens) {
            selected_rev.push(trimmed);
        }
        break;
    }

    selected_rev.reverse();
    selected_rev
}

#[cfg(test)]
fn should_compact_by_context(
    context_tokens: i64,
    limit: i64,
    history_threshold: Option<i64>,
) -> (bool, bool) {
    let decision = super::compaction_policy::should_compact_by_context(
        context_tokens,
        limit,
        history_threshold,
    );
    (decision.by_history, decision.should_compact())
}

fn resolve_message_budget(limit: i64, request_overhead_tokens: i64) -> i64 {
    limit.saturating_sub(request_overhead_tokens.max(0)).max(1)
}

fn resolve_projected_request_tokens(
    context_tokens: i64,
    persisted_context_tokens: i64,
    request_overhead_tokens: i64,
) -> i64 {
    context_tokens
        .max(persisted_context_tokens)
        .saturating_add(request_overhead_tokens.max(0))
}

fn resolve_compaction_limit(
    llm_config: &LlmModelConfig,
    context_tokens: i64,
    force: bool,
) -> Option<i64> {
    let configured_limit =
        HistoryManager::get_auto_compact_limit(llm_config).map(|limit| limit.max(1));
    if let Some(limit) = configured_limit {
        if force {
            return Some(resolve_force_compaction_limit(context_tokens, limit));
        }
        return Some(limit.max(1));
    }
    if !force {
        return None;
    }
    let adaptive_limit = (context_tokens / 4).max(COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
    Some(adaptive_limit.clamp(
        COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS,
        COMPACTION_FORCE_FALLBACK_LIMIT,
    ))
}

fn resolve_force_compaction_limit(context_tokens: i64, configured_limit: i64) -> i64 {
    if configured_limit <= 0 {
        return COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS.max(1);
    }
    let adaptive_limit = (context_tokens / 2).max(COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
    adaptive_limit
        .clamp(COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS, configured_limit)
        .max(1)
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

fn default_auto_memory_extract_prompt() -> &'static str {
    r#"You are a long-term memory extraction engine.
Extract up to 4 stable memory fragments from the current user message and the recent user-message window.

Keep only information that is likely to be useful across future turns:
- user identity or stable profile facts
- enduring preferences and constraints
- current ongoing plans that may matter in later turns
- explicit long-term notes the user asked the system to remember

Do not extract:
- temporary process chatter
- one-off execution details
- tool-call details
- facts stated only by the assistant
- guesses or inferred facts
- questions such as “what is my name” or “who am I” as if they were facts

Output only JSON in this exact shape:
<memory_fragments>
{
  "items": [
    {
      "category": "response-preference | profile | plan | preference | working-note",
      "slot": "reply_language | response_style | response_format | name | identity | background | current | generic | custom_stable_slot",
      "title": "",
      "summary": "",
      "content": "",
      "tags": [""],
      "tier": "core | working | peripheral",
      "importance": 0.0,
      "confidence": 0.0
    }
  ]
}
</memory_fragments>"#
}

fn build_auto_memory_extract_request(question: &str, answer: &str, window: &[String]) -> String {
    let mut lines = vec![
        "[Current User Message]".to_string(),
        truncate_auto_memory_extract_text(question, 1200),
    ];
    if !window.is_empty() {
        lines.push(String::new());
        lines.push("[Recent User Message Window]".to_string());
        for (index, item) in window.iter().enumerate() {
            lines.push(format!(
                "{}. {}",
                index + 1,
                truncate_auto_memory_extract_text(item, 600)
            ));
        }
    }
    if !answer.trim().is_empty() {
        lines.push(String::new());
        lines.push("[Latest Assistant Reply For Context Only]".to_string());
        lines.push(
            "Use this only for context disambiguation. Do not turn assistant-only claims into memories."
                .to_string(),
        );
        lines.push(truncate_auto_memory_extract_text(answer, 1000));
    }
    lines.join("\n")
}

fn truncate_auto_memory_extract_text(text: &str, char_limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= char_limit {
        return trimmed.to_string();
    }
    let mut output = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if index >= char_limit {
            break;
        }
        output.push(ch);
    }
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn llm_config(value: Value) -> LlmModelConfig {
        serde_json::from_value(value).expect("parse llm model config")
    }

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

    #[test]
    fn test_resolve_compaction_limit_uses_configured_limit() {
        let cfg = llm_config(json!({
            "max_context": 8000,
            "max_output": 512
        }));
        let limit = resolve_compaction_limit(&cfg, 32000, false).unwrap_or_default();
        assert!(limit > 0);
    }

    #[test]
    fn test_resolve_compaction_limit_skips_without_force_when_unknown() {
        let cfg = llm_config(json!({}));
        assert!(resolve_compaction_limit(&cfg, 32000, false).is_none());
    }

    #[test]
    fn test_resolve_compaction_limit_uses_force_fallback_when_unknown() {
        let cfg = llm_config(json!({}));
        let limit = resolve_compaction_limit(&cfg, 48000, true).unwrap_or_default();
        assert!(limit >= COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
        assert!(limit <= COMPACTION_FORCE_FALLBACK_LIMIT);
    }

    #[test]
    fn test_resolve_compaction_limit_force_uses_adaptive_limit_with_configured_cap() {
        let cfg = llm_config(json!({
            "max_context": 64000,
            "max_output": 1024
        }));
        let configured = resolve_compaction_limit(&cfg, 80_000, false).unwrap_or_default();
        let forced = resolve_compaction_limit(&cfg, 80_000, true).unwrap_or_default();
        assert!(configured > 0);
        assert!(forced > 0);
        assert!(forced <= configured);
    }

    #[test]
    fn test_resolve_message_budget_reserves_tool_overhead() {
        assert_eq!(resolve_message_budget(4096, 512), 3584);
        assert_eq!(resolve_message_budget(256, 4096), 1);
    }

    #[test]
    fn test_build_compaction_summary_config_disables_reasoning_in_payload() {
        let cfg = llm_config(json!({
            "provider": "openai",
            "api_mode": "responses",
            "model": "gpt-5-mini",
            "max_output": 2048,
            "reasoning_effort": "high"
        }));
        let summary_config = build_compaction_summary_config(&cfg);
        let client = build_llm_client(&summary_config, reqwest::Client::new());
        let payload = client.build_request_payload(
            &[ChatMessage {
                role: "user".to_string(),
                content: json!("compress this"),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            false,
        );

        assert_eq!(summary_config.reasoning_effort.as_deref(), Some("none"));
        assert_eq!(summary_config.max_rounds, Some(1));
        assert_eq!(
            summary_config.max_output,
            Some(COMPACTION_SUMMARY_MAX_OUTPUT as u32)
        );
        assert_eq!(payload["reasoning"]["effort"], "none");
    }

    #[test]
    fn test_prepare_compaction_summary_messages_compacts_observation_payload() {
        let preview = "X".repeat(12_000);
        let messages = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({
                "role": "user",
                "content": format!("{OBSERVATION_PREFIX}{}", json!({
                    "tool": "read_file",
                    "ok": true,
                    "data": {
                        "preview": preview,
                        "original_chars": 12000,
                    }
                }))
            }),
        ];
        let prepared = prepare_compaction_summary_messages(messages, 2048);
        assert_eq!(prepared.len(), 2);
        assert_eq!(
            prepared[0].get("role").and_then(Value::as_str),
            Some("system")
        );
        let observation = prepared[1]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(observation.contains("Tool observation (read_file): success"));
        assert!(!observation.contains(OBSERVATION_PREFIX));
        assert!(approx_token_count(observation) <= COMPACTION_SUMMARY_OBSERVATION_MAX_TOKENS);
    }

    #[test]
    fn test_prepare_compaction_summary_messages_merges_system_and_flattens_tool_calls() {
        let messages = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({ "role": "system", "content": "artifact index" }),
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    { "function": { "name": "search_content" } },
                    { "function": { "name": "read_file" } }
                ]
            }),
        ];
        let prepared = prepare_compaction_summary_messages(messages, 2048);
        assert_eq!(prepared.len(), 2);
        assert_eq!(
            prepared[0].get("content").and_then(Value::as_str),
            Some("system prompt\n\nartifact index")
        );
        let assistant = prepared[1]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(assistant.contains("search_content"));
        assert!(assistant.contains("read_file"));
        assert!(prepared[1].get("tool_calls").is_none());
    }

    #[test]
    fn test_resolve_projected_request_tokens_prefers_persisted_peak() {
        assert_eq!(resolve_projected_request_tokens(2000, 3000, 400), 3400);
        assert_eq!(resolve_projected_request_tokens(5000, 3000, 400), 5400);
    }

    #[test]
    fn test_merge_compaction_system_message_merges_artifact_into_existing_system() {
        let merged = merge_compaction_system_message(
            Some(json!({ "role": "system", "content": "system prompt" })),
            "Artifact index",
        )
        .expect("merged system message");
        assert_eq!(merged.get("role").and_then(Value::as_str), Some("system"));
        let content = merged
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(content.contains("system prompt"));
        assert!(content.contains("Artifact index"));
    }

    #[test]
    fn test_merge_compaction_system_message_creates_system_from_artifact_only() {
        let merged =
            merge_compaction_system_message(None, "Artifact index").expect("artifact system");
        assert_eq!(merged.get("role").and_then(Value::as_str), Some("system"));
        assert_eq!(
            merged.get("content").and_then(Value::as_str),
            Some("Artifact index")
        );
    }

    #[test]
    fn test_trim_message_to_fit_tokens_reduces_large_content() {
        let message = json!({
            "role": "user",
            "content": "A".repeat(20_000),
        });
        let before = estimate_message_tokens(&message);
        let target = (before / 8).max(32);
        let trimmed = trim_message_to_fit_tokens(&message, target).expect("trimmed message");
        let after = estimate_message_tokens(&trimmed);
        assert!(after <= target);
        assert!(after < before);
    }

    #[test]
    fn test_apply_rebuilt_context_guard_trims_current_user_message() {
        let mut messages = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({ "role": "user", "content": "summary line" }),
            json!({ "role": "user", "content": "B".repeat(24_000) }),
        ];
        let limit = 800;
        let stats = apply_rebuilt_context_guard(&mut messages, limit);
        assert!(stats.applied);
        assert!(stats.current_user_trimmed || stats.fallback_trim_applied);
        assert!(estimate_messages_tokens(&messages) <= limit);
    }

    #[test]
    fn test_apply_rebuilt_context_guard_preserves_current_question_before_trimming_it() {
        let current_question =
            "Please analyze team size and salaries by department, draw a chart, and summarize in 3 points.";
        let summary = format!(
            "{}\n{}",
            i18n::t("history.compaction_prefix"),
            "S".repeat(48_000)
        );
        let mut messages = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({ "role": "user", "content": summary }),
            json!({ "role": "user", "content": current_question }),
        ];
        let limit = 256;
        let stats = apply_rebuilt_context_guard(&mut messages, limit);
        assert!(stats.applied);
        assert!(stats.summary_removed || stats.summary_trimmed);
        assert!(!stats.current_user_trimmed);
        assert_eq!(
            messages
                .last()
                .and_then(|item| item.get("content"))
                .and_then(Value::as_str),
            Some(current_question)
        );
        assert!(estimate_messages_tokens(&messages) <= limit);
    }

    #[test]
    fn test_apply_rebuilt_context_guard_handles_array_content() {
        let mut messages = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({ "role": "user", "content": "summary line" }),
            json!({
                "role": "user",
                "content": [
                    { "type": "text", "text": "C".repeat(12_000) },
                    { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
                ]
            }),
        ];
        let limit = 900;
        let stats = apply_rebuilt_context_guard(&mut messages, limit);
        assert!(stats.applied);
        assert!(estimate_messages_tokens(&messages) <= limit);
    }

    #[test]
    fn test_apply_rebuilt_context_guard_trims_single_user_message() {
        let mut messages = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({ "role": "user", "content": "D".repeat(36_000) }),
        ];
        let limit = 900;
        let stats = apply_rebuilt_context_guard(&mut messages, limit);
        assert!(stats.applied);
        assert!(stats.current_user_trimmed || stats.fallback_trim_applied);
        assert!(estimate_messages_tokens(&messages) <= limit);
    }

    #[test]
    fn test_locate_compaction_summary_message_index_prefers_prefixed_summary() {
        let summary = format!("{}\nsummary", i18n::t("history.compaction_prefix"));
        let messages = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({ "role": "user", "content": "older user message" }),
            json!({ "role": "user", "content": summary }),
            json!({ "role": "user", "content": "current question" }),
        ];
        assert_eq!(locate_compaction_summary_message_index(&messages), Some(2));
    }

    #[test]
    fn test_collect_recent_user_messages_for_compaction_excludes_summary_and_observation() {
        let summary = format!("{}\nsummary", i18n::t("history.compaction_prefix"));
        let observation = format!("{OBSERVATION_PREFIX}{{\"tool\":\"read_file\"}}");
        let messages = vec![
            json!({ "role": "user", "content": summary }),
            json!({ "role": "user", "content": observation }),
            json!({ "role": "assistant", "content": "ok" }),
        ];
        let kept = collect_recent_user_messages_for_compaction(&messages, 1024);
        assert!(kept.is_empty());
    }

    #[test]
    fn test_collect_recent_user_messages_for_compaction_keeps_latest_user_window() {
        let latest = json!({ "role": "user", "content": "latest question" });
        let previous = json!({ "role": "user", "content": "B".repeat(800) });
        let messages = vec![
            json!({ "role": "assistant", "content": "answer" }),
            previous.clone(),
            latest.clone(),
        ];
        let limit = estimate_message_tokens(&latest).saturating_add(4);
        let kept = collect_recent_user_messages_for_compaction(&messages, limit);
        assert!(!kept.is_empty());
        assert_eq!(kept.last(), Some(&latest));
        assert!(kept.len() <= 2);
    }

    #[test]
    fn test_collect_recent_user_messages_for_compaction_trims_oversized_latest_user_message() {
        let large = json!({ "role": "user", "content": "X".repeat(24_000) });
        let messages = vec![large];
        let token_limit = 512;
        let kept = collect_recent_user_messages_for_compaction(&messages, token_limit);
        assert_eq!(kept.len(), 1);
        let kept_tokens = estimate_message_tokens(&kept[0]);
        assert!(kept_tokens <= token_limit.max(1));
    }

    #[test]
    fn test_select_compaction_boundary_index_zero_mode_targets_current_question() {
        let history_items = vec![
            json!({ "role": "user", "content": "older question" }),
            json!({ "role": "assistant", "content": "older answer" }),
            json!({ "role": "user", "content": "current question" }),
        ];
        let current_index =
            locate_matching_history_user_index(&history_items, &[String::from("current question")]);
        let boundary = select_compaction_boundary(
            &history_items,
            current_index,
            CompactionResetMode::Zero,
            COMPACTION_RECENT_USER_WINDOW_TOKENS,
        );
        assert_eq!(current_index, Some(2));
        assert_eq!(boundary.boundary_index, Some(2));
    }

    #[test]
    fn test_select_compaction_boundary_index_current_mode_stops_before_current_question() {
        let history_items = vec![
            json!({ "role": "user", "content": "older question" }),
            json!({ "role": "assistant", "content": "older answer" }),
            json!({ "role": "user", "content": "current question" }),
        ];
        let current_index =
            locate_matching_history_user_index(&history_items, &[String::from("current question")]);
        let boundary = select_compaction_boundary(
            &history_items,
            current_index,
            CompactionResetMode::Current,
            COMPACTION_RECENT_USER_WINDOW_TOKENS,
        );
        assert_eq!(boundary.boundary_index, Some(1));
    }

    #[test]
    fn test_select_compaction_boundary_index_keep_mode_retains_recent_tail_window() {
        let history_items = vec![
            json!({ "role": "user", "content": "older question" }),
            json!({ "role": "assistant", "content": "older answer" }),
            json!({ "role": "user", "content": "middle question" }),
            json!({ "role": "assistant", "content": "middle answer" }),
            json!({ "role": "user", "content": "current question" }),
        ];
        let keep_tokens = estimate_message_tokens(&json!({
            "role": "user",
            "content": "middle question"
        })) + estimate_message_tokens(&json!({
            "role": "user",
            "content": "current question"
        }));
        let boundary = select_compaction_boundary(
            &history_items,
            locate_matching_history_user_index(&history_items, &[String::from("current question")]),
            CompactionResetMode::Keep,
            keep_tokens,
        );
        assert_eq!(boundary.boundary_index, Some(1));
    }

    #[test]
    fn test_select_compaction_boundary_index_keep_mode_supports_full_window() {
        let history_items = vec![
            json!({
                "role": "user",
                "content": "older question",
                "timestamp": "2026-03-27T00:00:01Z"
            }),
            json!({
                "role": "assistant",
                "content": "older answer",
                "timestamp": "2026-03-27T00:00:02Z"
            }),
            json!({
                "role": "user",
                "content": "current question",
                "timestamp": "2026-03-27T00:00:03Z"
            }),
        ];
        let keep_tokens = estimate_message_tokens(&json!({
            "role": "user",
            "content": "older question"
        })) + estimate_message_tokens(&json!({
            "role": "user",
            "content": "current question"
        })) + 32;
        let boundary = select_compaction_boundary(
            &history_items,
            locate_matching_history_user_index(&history_items, &[String::from("current question")]),
            CompactionResetMode::Keep,
            keep_tokens,
        );
        assert_eq!(boundary.boundary_index, None);
        assert!(boundary.boundary_ts_override.is_some());
    }

    #[test]
    fn test_mark_current_user_message_skip_persist_sets_meta_flag() {
        let mut message = json!({ "role": "user", "content": "current question" });
        mark_current_user_message_skip_persist(&mut message);
        assert_eq!(
            message
                .get("meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get(COMPACTION_SKIP_PERSIST_CURRENT_USER_META_KEY))
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn test_merge_compaction_summary_with_fresh_memory_appends_block() {
        let summary = format!(
            "{}\nKeep this summary",
            i18n::t("history.compaction_prefix")
        );
        let memory_block = format!(
            "{}\n- Remember user prefers markdown",
            i18n::t("memory.block_prefix")
        );
        let (merged, injected) =
            merge_compaction_summary_with_fresh_memory(&summary, &memory_block);
        assert!(injected);
        assert!(merged.contains("Keep this summary"));
        assert!(merged.contains("Remember user prefers markdown"));
    }

    #[test]
    fn test_merge_compaction_summary_with_fresh_memory_replaces_old_block() {
        let summary = format!(
            "{}\nKeep this summary\n\n{}\n- stale memory",
            i18n::t("history.compaction_prefix"),
            i18n::t("memory.block_prefix"),
        );
        let memory_block = format!("{}\n- fresh memory", i18n::t("memory.block_prefix"));
        let (merged, injected) =
            merge_compaction_summary_with_fresh_memory(&summary, &memory_block);
        assert!(injected);
        assert!(merged.contains("Keep this summary"));
        assert!(merged.contains("fresh memory"));
        assert!(!merged.contains("stale memory"));
    }

    #[test]
    fn test_build_compaction_instruction_appends_current_request_constraints() {
        let instruction = build_compaction_instruction(
            "base prompt",
            "",
            "Summarize UNCLOS article 121 for disputed reefs",
            "",
        );
        assert!(instruction.contains("base prompt"));
        assert!(instruction.contains("[Current user request / 当前用户问题]"));
        assert!(instruction.contains("Summarize UNCLOS article 121 for disputed reefs"));
        assert!(instruction.contains("task unspecified"));
    }

    #[test]
    fn test_build_compaction_instruction_deduplicates_request_candidates() {
        let instruction =
            build_compaction_instruction("base prompt", "", "same request", "same request");
        assert_eq!(instruction.matches("same request").count(), 1);
    }
}
