use super::context::model_context_entries_from_messages;
use super::execute_support::{
    emit_turn_terminal_event, turn_terminal_status_for_error, TurnTerminalEvent,
};
use super::thread_runtime::ThreadRuntimeStatus;
use super::*;

use super::memory_support::*;
use crate::core::{blocking, long_task};

pub(super) use super::memory_support::{insert_compaction_id, CompactionRunMode};

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
                        let new_content = trim_text_to_tokens(
                            text,
                            target_tokens,
                            COMPACTION_TEXT_TRUNCATION_SUFFIX,
                        );
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
        _request_overhead_tokens: i64,
        force: bool,
        exclude_current_user: bool,
        run_mode: CompactionRunMode,
    ) -> Result<CompactionResult, OrchestratorError> {
        let compaction_profile = CompactionExecutionProfile::new(run_mode);
        let trigger_mode = Some(compaction_profile.trigger_mode());
        let request_overhead_tokens = 0_i64;
        let persisted_context_tokens = persisted_context_tokens.max(0);
        let context_tokens = persisted_context_tokens;
        let projected_request_tokens = resolve_projected_request_tokens(context_tokens);
        let Some(limit) = resolve_compaction_limit(llm_config, projected_request_tokens, force)
        else {
            return Ok(CompactionResult::unchanged(messages));
        };
        let message_budget = resolve_message_budget(limit);
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
        let estimated_message_tokens = estimate_messages_tokens(&messages);
        if !should_compact {
            return Ok(CompactionResult::unchanged(messages));
        }
        let compaction_id = new_compaction_id(compaction_profile.run_mode);
        let compaction_id_ref = Some(compaction_id.as_str());

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
            if max_context > 0 {
                map.insert("max_context".to_string(), json!(max_context));
            }
            insert_compaction_id(map, compaction_id_ref);
            insert_compaction_trigger_mode(map, trigger_mode);
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
        let current_turn_progress = current_user_index
            .map(|index| classify_current_turn_progress(&messages, index))
            .unwrap_or_default();
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
        let _ = self.workspace.flush_writes_async().await;
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
            let guard_stats =
                apply_rebuilt_context_guard(&mut guarded_messages, message_budget, false);
            if guard_stats.applied {
                let mut guard_payload = json!({
                    "stage": "context_guard",
                    "summary": "Context guard trimmed oversized user input before model call.",
                    "tokens_before": guard_stats.tokens_before,
                    "tokens_after": guard_stats.tokens_after,
                    "estimated_message_tokens": guard_stats.tokens_after,
                    "request_overhead_tokens": request_overhead_tokens,
                    "message_budget": message_budget,
                    "current_user_trimmed": guard_stats.current_user_trimmed,
                    "summary_trimmed": guard_stats.summary_trimmed,
                    "summary_removed": guard_stats.summary_removed,
                    "fallback_trim_applied": guard_stats.fallback_trim_applied,
                });
                if let Value::Object(ref mut map) = guard_payload {
                    if max_context > 0 {
                        map.insert("max_context".to_string(), json!(max_context));
                    }
                    insert_compaction_id(map, compaction_id_ref);
                    insert_compaction_trigger_mode(map, trigger_mode);
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
                "observed_context_tokens": history_usage,
                "estimated_message_tokens": estimated_message_tokens,
                "context_usage_source": if history_usage > 0 { "provider_observed" } else { "unobserved" },
                "persisted_context_tokens": persisted_context_tokens,
                "projected_request_tokens": projected_request_tokens,
                "request_overhead_tokens": request_overhead_tokens,
                "history_threshold": history_threshold,
                "limit": limit,
                "presampling_limit": compaction_decision.presampling_limit,
                "message_budget": message_budget,
                "total_tokens": total_tokens,
                "context_guard_applied": guard_stats.applied,
                "context_guard_tokens_before": guard_stats.tokens_before,
                "context_guard_tokens_after": guard_stats.tokens_after,
                "context_guard_current_user_trimmed": guard_stats.current_user_trimmed,
                "context_guard_summary_trimmed": guard_stats.summary_trimmed,
                "context_guard_summary_removed": guard_stats.summary_removed,
                "context_guard_fallback_trim_applied": guard_stats.fallback_trim_applied,
            });
            if let Value::Object(ref mut map) = compaction_payload {
                if max_context > 0 {
                    map.insert("max_context".to_string(), json!(max_context));
                }
                insert_compaction_id(map, compaction_id_ref);
                insert_compaction_trigger_mode(map, trigger_mode);
                compaction_round.insert_into(map);
            }
            emitter.emit("compaction", compaction_payload).await;
            return Ok(CompactionResult::compacted(
                if guard_stats.applied {
                    guarded_messages
                } else {
                    messages
                },
                compaction_id,
            ));
        }

        let compaction_prompt = HistoryManager::load_compaction_prompt();
        let compaction_instruction = build_compaction_instruction(
            &compaction_prompt,
            &artifact_content,
            &question_text,
            &current_user_signature,
        );
        let compaction_message = json!({ "role": "user", "content": compaction_instruction });
        let mut summary_input = build_compaction_summary_input(
            system_message.as_ref(),
            &source_messages,
            compaction_message,
        );

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
            insert_compaction_id(map, compaction_id_ref);
            insert_compaction_trigger_mode(map, trigger_mode);
            compaction_round.insert_into(map);
        }
        emitter.emit("llm_request", request_payload).await;

        let mut summary_fallback = false;
        let mut summary_fallback_reason: Option<&'static str> = None;
        let mut summary_failure_code: Option<String> = None;
        let mut summary_failure_message: Option<String> = None;
        let mut summary_failure_retryable: Option<bool> = None;
        let summary_model_output = match self
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
                if matches!(
                    err.code(),
                    "USER_QUOTA_EXCEEDED" | "USER_TOKEN_INSUFFICIENT"
                ) {
                    return Err(err);
                }
                summary_fallback = true;
                summary_fallback_reason = Some("llm_request_failed");
                summary_failure_code = Some(err.code().to_string());
                summary_failure_message =
                    Some(err.message().trim().chars().take(512).collect::<String>());
                summary_failure_retryable = Some(err.retryable());
                String::new()
            }
        };
        let fallback_content = build_compaction_fallback_summary(&source_messages, &user_content);
        let (fresh_memory_block, fresh_memory_count, fresh_memory_total_count) = self
            .build_fresh_memory_block_for_compaction(
                config,
                user_id,
                agent_id,
                session_id,
                question_candidates.first().map(String::as_str),
            )
            .await;
        let Some((mut summary_text, fresh_memory_injected)) =
            build_committable_compaction_summary(&summary_model_output, &fresh_memory_block)
                .or_else(|| {
                    summary_fallback = true;
                    if summary_fallback_reason.is_none() {
                        summary_fallback_reason = Some(if summary_model_output.trim().is_empty() {
                            "empty_summary"
                        } else {
                            "invalid_summary"
                        });
                    }
                    build_committable_compaction_summary(&fallback_content, &fresh_memory_block)
                })
        else {
            warn!(
                session_id = %session_id,
                user_id = %user_id,
                "skip committing compaction summary because both model output and fallback were invalid"
            );

            let mut guarded_messages = messages.clone();
            let guard_stats =
                apply_rebuilt_context_guard(&mut guarded_messages, message_budget, false);
            let mut compaction_payload = json!({
                "reason": if should_compact_by_history { "history" } else { "overflow" },
                "trigger": compaction_trigger,
                "status": if guard_stats.applied { "guard_only" } else { "skipped" },
                "skip_reason": "invalid_summary_commit",
                "summary_fallback": true,
                "summary_model_output": summary_model_output,
                "fresh_memory_injected": false,
                "fresh_memory_count": fresh_memory_count,
                "fresh_memory_total_count": fresh_memory_total_count,
                "history_usage": history_usage,
                "context_tokens": history_usage,
                "observed_context_tokens": history_usage,
                "estimated_message_tokens": estimated_message_tokens,
                "context_usage_source": if history_usage > 0 { "provider_observed" } else { "unobserved" },
                "persisted_context_tokens": persisted_context_tokens,
                "projected_request_tokens": projected_request_tokens,
                "request_overhead_tokens": request_overhead_tokens,
                "history_threshold": history_threshold,
                "limit": limit,
                "presampling_limit": compaction_decision.presampling_limit,
                "message_budget": message_budget,
                "total_tokens": total_tokens,
                "context_guard_applied": guard_stats.applied,
                "context_guard_tokens_before": guard_stats.tokens_before,
                "context_guard_tokens_after": guard_stats.tokens_after,
                "context_guard_current_user_trimmed": guard_stats.current_user_trimmed,
                "context_guard_summary_trimmed": guard_stats.summary_trimmed,
                "context_guard_summary_removed": guard_stats.summary_removed,
                "context_guard_fallback_trim_applied": guard_stats.fallback_trim_applied,
            });
            if let Value::Object(ref mut map) = compaction_payload {
                if max_context > 0 {
                    map.insert("max_context".to_string(), json!(max_context));
                }
                insert_compaction_id(map, compaction_id_ref);
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
                insert_compaction_trigger_mode(map, trigger_mode);
                compaction_round.insert_into(map);
            }
            emitter.emit("compaction", compaction_payload).await;
            return Ok(CompactionResult::compacted(
                if guard_stats.applied {
                    guarded_messages
                } else {
                    messages
                },
                compaction_id,
            ));
        };
        let resume_action = detect_compaction_resume_action(&summary_text);
        let source_interaction_blocks = collect_normalized_interaction_blocks(&source_messages);
        let source_interaction_messages = source_interaction_blocks
            .iter()
            .map(|block| block.message.clone())
            .collect::<Vec<_>>();
        let source_interaction_block_count = source_interaction_messages.len();
        let source_interaction_tokens = estimate_messages_tokens(&source_interaction_messages);
        let retained_segments = collect_retained_interaction_segments_with_indexes_for_compaction(
            &source_messages,
            current_user_index,
            COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE,
            COMPACTION_RETAINED_HEAD_INTERACTION_TOKENS,
            COMPACTION_RETAINED_TAIL_INTERACTION_TOKENS,
        );
        let retained_head_messages = retained_segments.head_messages;
        let retained_tail_messages = retained_segments.tail_messages;
        let retained_head_message_count = retained_head_messages.len();
        let retained_tail_message_count = retained_tail_messages.len();
        let mut retained_head_messages_for_rebuilt = retained_head_messages.clone();
        mark_retained_interaction_messages(&mut retained_head_messages_for_rebuilt);
        let mut retained_tail_messages_for_rebuilt = retained_tail_messages.clone();
        mark_retained_interaction_messages(&mut retained_tail_messages_for_rebuilt);
        let mut retained_replay_messages = retained_head_messages
            .iter()
            .cloned()
            .chain(retained_tail_messages.iter().cloned())
            .collect::<Vec<_>>();
        clear_compaction_inflight_markers(&mut retained_replay_messages);
        mark_retained_interaction_messages(&mut retained_replay_messages);
        let retained_interaction_message_count = retained_replay_messages.len();
        let retained_interaction_tokens = estimate_messages_tokens(&retained_replay_messages);
        let retained_user_message_count = retained_replay_messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            .count();
        let retained_user_tokens = retained_replay_messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            .map(estimate_message_tokens)
            .sum::<i64>();
        let current_user_replay = build_current_turn_replay_message(
            current_user_message.as_ref(),
            question_text.as_str(),
            message_budget,
            &current_turn_progress,
            resume_action,
            &summary_text,
        );
        let current_user_message_for_history_trimmed = current_user_replay.trimmed;
        let current_user_replay_mode = current_user_replay.mode;
        let current_turn_progress_state = current_turn_progress.state_label();

        let mut base_messages: Vec<Value> = Vec::new();
        if let Some(system_message) = replay_system_message.clone() {
            base_messages.push(system_message);
        }
        base_messages.extend(retained_head_messages.iter().cloned());
        base_messages.extend(retained_tail_messages.iter().cloned());
        if let Some(current_turn_replay_message) = current_user_replay.message.clone() {
            base_messages.push(current_turn_replay_message);
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
            let trimmed = trim_text_to_tokens(
                &summary_text,
                target_tokens,
                COMPACTION_TEXT_TRUNCATION_SUFFIX,
            );
            let committed_trimmed =
                clamp_committed_compaction_summary(&trimmed, COMPACTION_SUMMARY_MAX_CHARS);
            if committed_trimmed == summary_text
                || is_invalid_compaction_summary(&committed_trimmed)
            {
                break;
            }
            summary_text = committed_trimmed;
        }
        let summary_chars = summary_text.chars().count();
        let injected_summary_text = summary_text.clone();
        let mut response_payload = json!({
            "content": summary_text.clone(),
            "reasoning": "",
            "purpose": "compaction_summary",
        });
        if let Value::Object(ref mut map) = response_payload {
            insert_compaction_id(map, compaction_id_ref);
            compaction_round.insert_into(map);
        }
        emitter.emit("llm_response", response_payload).await;

        let mut rebuilt = Vec::new();
        if let Some(system_message) = replay_system_message {
            rebuilt.push(system_message);
        }
        rebuilt.extend(retained_head_messages_for_rebuilt);
        rebuilt.push(json!({ "role": "user", "content": summary_text }));
        rebuilt.extend(retained_tail_messages_for_rebuilt);
        if let Some(current_turn_replay_message) = current_user_replay.message {
            rebuilt.push(current_turn_replay_message);
        }
        let mut rebuilt = self.shrink_messages_to_limit(rebuilt, message_budget);
        let guard_stats = apply_rebuilt_context_guard(
            &mut rebuilt,
            message_budget,
            compaction_profile.prefer_preserving_summary,
        );
        let rebuilt_request_debug = build_compaction_message_debug_entries(&rebuilt);
        let committed_replacement_history =
            build_committed_replacement_history_from_rebuilt(&rebuilt);
        let replacement_history_debug =
            build_compaction_message_debug_entries(&committed_replacement_history);
        clear_retained_interaction_markers(&mut rebuilt);
        clear_compaction_inflight_markers(&mut rebuilt);
        let compacted_model_context_entries = model_context_entries_from_messages(&rebuilt);
        if let Err(err) = self.workspace.replace_model_context_entries(
            user_id,
            session_id,
            &compacted_model_context_entries,
        ) {
            warn!("replace model context after compaction failed for session {session_id}: {err}");
        }
        let _ = self.workspace.flush_writes_async().await;
        let rebuilt_tokens = estimate_messages_tokens(&rebuilt);
        let observed_rebuilt_tokens = 0_i64;
        let committed_replacement_history_tokens =
            estimate_messages_tokens(&committed_replacement_history);

        let mut meta = serde_json::Map::new();
        meta.insert(
            "type".to_string(),
            Value::String(COMPACTION_META_TYPE.to_string()),
        );
        meta.insert(
            "compaction_id".to_string(),
            Value::String(compaction_id.clone()),
        );
        meta.insert(
            COMPACTION_REPLACEMENT_HISTORY_META_KEY.to_string(),
            Value::Array(committed_replacement_history.clone()),
        );
        meta.insert(
            "trigger_mode".to_string(),
            Value::String(compaction_profile.trigger_mode().to_string()),
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
            compaction_round,
        );

        if guard_stats.applied {
            let mut guard_payload = json!({
                "stage": "context_guard",
                "summary": "Context guard trimmed oversized compaction payload.",
                "tokens_before": guard_stats.tokens_before,
                "tokens_after": guard_stats.tokens_after,
                "estimated_message_tokens": guard_stats.tokens_after,
                "request_overhead_tokens": request_overhead_tokens,
                "message_budget": message_budget,
                "current_user_replay_trimmed": current_user_message_for_history_trimmed,
                "current_user_trimmed": guard_stats.current_user_trimmed,
                "summary_trimmed": guard_stats.summary_trimmed,
                "summary_removed": guard_stats.summary_removed,
                "fallback_trim_applied": guard_stats.fallback_trim_applied,
            });
            if let Value::Object(ref mut map) = guard_payload {
                if max_context > 0 {
                    map.insert("max_context".to_string(), json!(max_context));
                }
                insert_compaction_id(map, compaction_id_ref);
                insert_compaction_trigger_mode(map, trigger_mode);
                compaction_round.insert_into(map);
            }
            emitter.emit("progress", guard_payload).await;
        }

        let mut compaction_payload_map = serde_json::Map::new();
        compaction_payload_map.insert(
            "reason".to_string(),
            Value::String(
                if should_compact_by_history {
                    "history"
                } else {
                    "overflow"
                }
                .to_string(),
            ),
        );
        compaction_payload_map.insert(
            "trigger".to_string(),
            Value::String(compaction_trigger.to_string()),
        );
        compaction_payload_map.insert(
            "status".to_string(),
            Value::String(if summary_fallback { "fallback" } else { "done" }.to_string()),
        );
        compaction_payload_map.insert("summary_fallback".to_string(), json!(summary_fallback));
        compaction_payload_map.insert(
            "summary_model_output".to_string(),
            json!(summary_model_output),
        );
        compaction_payload_map.insert("summary_text".to_string(), json!(injected_summary_text));
        compaction_payload_map.insert(
            "fresh_memory_injected".to_string(),
            json!(fresh_memory_injected),
        );
        compaction_payload_map.insert("fresh_memory_count".to_string(), json!(fresh_memory_count));
        compaction_payload_map.insert(
            "fresh_memory_total_count".to_string(),
            json!(fresh_memory_total_count),
        );
        compaction_payload_map.insert(
            "retained_user_message_count".to_string(),
            json!(retained_user_message_count),
        );
        compaction_payload_map.insert(
            "retained_user_tokens".to_string(),
            json!(retained_user_tokens),
        );
        compaction_payload_map.insert(
            "retained_interaction_message_count".to_string(),
            json!(retained_interaction_message_count),
        );
        compaction_payload_map.insert(
            "retained_head_message_count".to_string(),
            json!(retained_head_message_count),
        );
        compaction_payload_map.insert(
            "retained_tail_message_count".to_string(),
            json!(retained_tail_message_count),
        );
        compaction_payload_map.insert(
            "retained_interaction_tokens".to_string(),
            json!(retained_interaction_tokens),
        );
        compaction_payload_map.insert(
            "source_interaction_block_count".to_string(),
            json!(source_interaction_block_count),
        );
        compaction_payload_map.insert(
            "source_interaction_tokens".to_string(),
            json!(source_interaction_tokens),
        );
        compaction_payload_map.insert(
            "source_interaction_messages_debug".to_string(),
            Value::Array(build_compaction_message_debug_entries(
                &source_interaction_messages,
            )),
        );
        compaction_payload_map.insert(
            "retained_head_messages_debug".to_string(),
            Value::Array(build_compaction_message_debug_entries(
                &retained_head_messages,
            )),
        );
        compaction_payload_map.insert(
            "retained_tail_messages_debug".to_string(),
            Value::Array(build_compaction_message_debug_entries(
                &retained_tail_messages,
            )),
        );
        compaction_payload_map.insert(
            "retained_head_block_count_target".to_string(),
            json!(COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE),
        );
        compaction_payload_map.insert(
            "retained_tail_block_count_target".to_string(),
            json!(COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE),
        );
        compaction_payload_map.insert(
            "retained_head_exchange_count_target".to_string(),
            json!(COMPACTION_RETAINED_INTERACTION_EXCHANGE_COUNT_PER_SIDE),
        );
        compaction_payload_map.insert(
            "retained_tail_exchange_count_target".to_string(),
            json!(COMPACTION_RETAINED_INTERACTION_EXCHANGE_COUNT_PER_SIDE),
        );
        compaction_payload_map.insert(
            "summary_tokens".to_string(),
            json!(approx_token_count(&summary_text)),
        );
        compaction_payload_map.insert(
            "replacement_history_message_count".to_string(),
            json!(committed_replacement_history.len()),
        );
        compaction_payload_map.insert(
            "replacement_history_tokens".to_string(),
            json!(committed_replacement_history_tokens),
        );
        compaction_payload_map.insert(
            "replacement_history_debug".to_string(),
            Value::Array(replacement_history_debug),
        );
        compaction_payload_map.insert(
            "rebuilt_request_debug".to_string(),
            Value::Array(rebuilt_request_debug),
        );
        compaction_payload_map.insert("summary_chars".to_string(), json!(summary_chars));
        compaction_payload_map.insert(
            "summary_chars_limit".to_string(),
            json!(COMPACTION_SUMMARY_MAX_CHARS),
        );
        compaction_payload_map.insert("total_tokens".to_string(), json!(total_tokens));
        compaction_payload_map.insert(
            "total_tokens_after".to_string(),
            json!(observed_rebuilt_tokens),
        );
        compaction_payload_map.insert("history_usage".to_string(), json!(history_usage));
        compaction_payload_map.insert("context_tokens".to_string(), json!(history_usage));
        compaction_payload_map.insert("observed_context_tokens".to_string(), json!(history_usage));
        compaction_payload_map.insert(
            "context_tokens_after".to_string(),
            json!(observed_rebuilt_tokens),
        );
        compaction_payload_map.insert(
            "observed_context_tokens_after".to_string(),
            json!(observed_rebuilt_tokens),
        );
        compaction_payload_map.insert(
            "estimated_message_tokens".to_string(),
            json!(estimated_message_tokens),
        );
        compaction_payload_map.insert(
            "estimated_message_tokens_after".to_string(),
            json!(rebuilt_tokens),
        );
        compaction_payload_map.insert(
            "context_usage_source".to_string(),
            Value::String("provider_observed".to_string()),
        );
        compaction_payload_map.insert(
            "context_usage_source_after".to_string(),
            Value::String("unobserved_after_compaction".to_string()),
        );
        compaction_payload_map.insert(
            "persisted_context_tokens".to_string(),
            json!(persisted_context_tokens),
        );
        compaction_payload_map.insert(
            "projected_request_tokens".to_string(),
            json!(projected_request_tokens),
        );
        compaction_payload_map.insert(
            "projected_request_tokens_after".to_string(),
            json!(observed_rebuilt_tokens),
        );
        compaction_payload_map.insert(
            "request_overhead_tokens".to_string(),
            json!(request_overhead_tokens),
        );
        compaction_payload_map.insert("history_threshold".to_string(), json!(history_threshold));
        compaction_payload_map.insert("limit".to_string(), json!(limit));
        compaction_payload_map.insert(
            "presampling_limit".to_string(),
            json!(compaction_decision.presampling_limit),
        );
        compaction_payload_map.insert("message_budget".to_string(), json!(message_budget));
        compaction_payload_map.insert(
            "compaction_run_mode".to_string(),
            Value::String(compaction_profile.trigger_mode().to_string()),
        );
        compaction_payload_map.insert(
            "summary_priority".to_string(),
            Value::Bool(compaction_profile.prefer_preserving_summary),
        );
        compaction_payload_map.insert(
            "current_user_replay_mode".to_string(),
            Value::String(current_user_replay_mode.as_str().to_string()),
        );
        compaction_payload_map.insert(
            "compaction_resume_action".to_string(),
            Value::String(resume_action.as_str().to_string()),
        );
        compaction_payload_map.insert(
            "current_turn_progress_state".to_string(),
            Value::String(current_turn_progress_state.to_string()),
        );
        compaction_payload_map.insert(
            "current_turn_has_post_user_progress".to_string(),
            json!(current_turn_progress.has_post_user_messages),
        );
        compaction_payload_map.insert(
            "current_turn_has_tool_success".to_string(),
            json!(current_turn_progress.has_tool_success),
        );
        compaction_payload_map.insert(
            "current_turn_has_tool_failure".to_string(),
            json!(current_turn_progress.has_tool_failure),
        );
        compaction_payload_map.insert(
            "context_guard_applied".to_string(),
            json!(guard_stats.applied),
        );
        compaction_payload_map.insert(
            "context_guard_tokens_before".to_string(),
            json!(guard_stats.tokens_before),
        );
        compaction_payload_map.insert(
            "context_guard_tokens_after".to_string(),
            json!(guard_stats.tokens_after),
        );
        compaction_payload_map.insert(
            "context_guard_current_user_replay_trimmed".to_string(),
            json!(current_user_message_for_history_trimmed),
        );
        compaction_payload_map.insert(
            "context_guard_current_user_trimmed".to_string(),
            json!(guard_stats.current_user_trimmed),
        );
        compaction_payload_map.insert(
            "context_guard_current_user_tokens_before".to_string(),
            json!(guard_stats.current_user_tokens_before),
        );
        compaction_payload_map.insert(
            "context_guard_current_user_tokens_after".to_string(),
            json!(guard_stats.current_user_tokens_after),
        );
        compaction_payload_map.insert(
            "context_guard_summary_trimmed".to_string(),
            json!(guard_stats.summary_trimmed),
        );
        compaction_payload_map.insert(
            "context_guard_summary_tokens_before".to_string(),
            json!(guard_stats.summary_tokens_before),
        );
        compaction_payload_map.insert(
            "context_guard_summary_tokens_after".to_string(),
            json!(guard_stats.summary_tokens_after),
        );
        compaction_payload_map.insert(
            "context_guard_summary_removed".to_string(),
            json!(guard_stats.summary_removed),
        );
        compaction_payload_map.insert(
            "context_guard_fallback_trim_applied".to_string(),
            json!(guard_stats.fallback_trim_applied),
        );
        let mut compaction_payload = Value::Object(compaction_payload_map);
        if let Value::Object(ref mut map) = compaction_payload {
            if max_context > 0 {
                map.insert("max_context".to_string(), json!(max_context));
            }
            insert_compaction_id(map, compaction_id_ref);
            insert_compaction_trigger_mode(map, trigger_mode);
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

        Ok(CompactionResult::compacted(rebuilt, compaction_id))
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
        preview_skill_override: Option<bool>,
        manual_user_round_override: Option<i64>,
        debug_payload: bool,
        manage_runtime_turn: bool,
    ) -> Result<Value, OrchestratorError> {
        let storage = self.storage.clone();
        let session_id_for_offset = session_id.to_string();
        let start_event_id =
            match blocking::run_db("orchestrator.memory.stream_offset", move || {
                storage.get_max_stream_event_id(&session_id_for_offset)
            })
            .await
            {
                Err(err) => {
                    warn!("failed to load stream event offset for session {session_id}: {err}");
                    0
                }
                Ok(value) => value,
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
        let lifecycle_round_info =
            RoundInfo::user_only(manual_user_round_override.unwrap_or(1).max(1));
        let active_turn_id = if manage_runtime_turn {
            let active_turn = self.active_turns.begin_turn(session_id);
            let turn_id = active_turn.turn_id.clone();
            self.emit_thread_runtime_update(
                &emitter,
                lifecycle_round_info,
                self.thread_runtime.begin_turn(session_id, &turn_id),
            )
            .await;
            Some(turn_id)
        } else {
            None
        };

        let config = self.resolve_config(None).await;
        let log_payload = is_debug_log_level(&config.observability.log_level) || debug_payload;
        let (_llm_name, llm_config) = match self.resolve_llm_config(&config, model_name) {
            Ok(value) => value,
            Err(err) => {
                if manage_runtime_turn {
                    self.emit_manual_compaction_failure(&emitter, lifecycle_round_info, &err)
                        .await;
                    self.finish_manual_compaction_turn(
                        session_id,
                        active_turn_id.as_deref(),
                        &emitter,
                        lifecycle_round_info,
                        Err(&err),
                    )
                    .await;
                    emitter.finish().await;
                }
                return Err(err);
            }
        };
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
        let preview_skill = preview_skill_override.unwrap_or_else(|| {
            agent_id
                .and_then(|id| self.storage.get_user_agent(user_id, id).ok().flatten())
                .map(|record| record.preview_skill)
                .unwrap_or(false)
        });
        let allowed_tool_names =
            self.apply_preview_skill_tool_policy(allowed_tool_names, preview_skill);
        let tool_call_mode =
            self.resolve_frozen_session_tool_call_mode(user_id, session_id, &llm_config);
        let workspace_id = self.resolve_workspace_id(user_id, agent_id, None);
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
                preview_skill,
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
        let manual_user_round = manual_user_round_override.unwrap_or_else(|| {
            messages
                .iter()
                .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
                .count() as i64
                + 1
        });
        let manual_round_info = RoundInfo::user_only(manual_user_round.max(1));

        if let Err(err) = self.ensure_not_cancelled(session_id) {
            if manage_runtime_turn {
                self.emit_manual_compaction_failure(&emitter, manual_round_info, &err)
                    .await;
                self.finish_manual_compaction_turn(
                    session_id,
                    active_turn_id.as_deref(),
                    &emitter,
                    manual_round_info,
                    Err(&err),
                )
                .await;
                emitter.finish().await;
            }
            return Err(err);
        }

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
                CompactionRunMode::Manual,
            )
            .await
        {
            Ok(result) => result.messages,
            Err(err) => {
                self.emit_manual_compaction_failure(&emitter, manual_round_info, &err)
                    .await;
                if manage_runtime_turn {
                    self.finish_manual_compaction_turn(
                        session_id,
                        active_turn_id.as_deref(),
                        &emitter,
                        manual_round_info,
                        Err(&err),
                    )
                    .await;
                }
                emitter.finish().await;
                return Err(err);
            }
        };
        if let Err(err) = self.ensure_not_cancelled(session_id) {
            self.emit_manual_compaction_failure(&emitter, manual_round_info, &err)
                .await;
            if manage_runtime_turn {
                self.finish_manual_compaction_turn(
                    session_id,
                    active_turn_id.as_deref(),
                    &emitter,
                    manual_round_info,
                    Err(&err),
                )
                .await;
            }
            emitter.finish().await;
            return Err(err);
        }
        let messages = context_manager.normalize_messages(messages);
        let context_tokens = 0_i64;
        self.workspace
            .save_session_context_tokens_async(user_id, session_id, context_tokens)
            .await;
        let mut context_payload = json!({
            "context_tokens": context_tokens,
            "observed_context_tokens": context_tokens,
            "context_usage_source": "unobserved_after_manual_compaction",
            "message_count": messages.len(),
        });
        if let Value::Object(ref mut map) = context_payload {
            insert_compaction_trigger_mode(map, Some("manual"));
            manual_round_info.insert_into(map);
        }
        emitter.emit("context_usage", context_payload).await;
        if manage_runtime_turn {
            self.finish_manual_compaction_turn(
                session_id,
                active_turn_id.as_deref(),
                &emitter,
                manual_round_info,
                Ok(()),
            )
            .await;
        }
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
            if let Some(context_tokens) = context_obj.get("context_tokens").and_then(Value::as_i64)
            {
                response_obj.insert("final_context_tokens".to_string(), json!(context_tokens));
            }
            if let Some(max_context) = context_obj.get("max_context").and_then(Value::as_i64) {
                response_obj.insert("max_context".to_string(), json!(max_context));
            }
            if let Some(message_count) = context_obj.get("message_count").and_then(Value::as_i64) {
                response_obj.insert("message_count".to_string(), json!(message_count));
            }
        }

        Ok(response_payload)
    }

    async fn emit_manual_compaction_failure(
        &self,
        emitter: &EventEmitter,
        round_info: RoundInfo,
        err: &OrchestratorError,
    ) {
        let status = if err.code() == "CANCELLED" {
            "cancelled"
        } else {
            "failed"
        };
        let mut compaction_payload = json!({
            "stage": "context_overflow_recovery",
            "reason": "manual",
            "trigger": "force",
            "trigger_mode": "manual",
            "status": status,
            "error_code": err.code(),
            "error_message": err.message(),
        });
        if let Value::Object(ref mut map) = compaction_payload {
            round_info.insert_into(map);
        }
        emitter.emit("compaction", compaction_payload).await;
    }

    async fn finish_manual_compaction_turn(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        emitter: &EventEmitter,
        round_info: RoundInfo,
        outcome: Result<(), &OrchestratorError>,
    ) {
        match outcome {
            Ok(()) => {
                emit_turn_terminal_event(
                    emitter,
                    round_info,
                    TurnTerminalEvent {
                        status: "completed",
                        stop_reason: Some("manual_compaction"),
                        round_usage: None,
                        error: None,
                        waiting_for_user_input: false,
                        stop_meta: None,
                    },
                )
                .await;
            }
            Err(err) => {
                emit_turn_terminal_event(
                    emitter,
                    round_info,
                    TurnTerminalEvent {
                        status: turn_terminal_status_for_error(err),
                        stop_reason: None,
                        round_usage: None,
                        error: Some(err),
                        waiting_for_user_input: false,
                        stop_meta: None,
                    },
                )
                .await;
            }
        }
        if let Some(active_turn_id) = turn_id {
            self.finish_active_turn(
                session_id,
                active_turn_id,
                emitter,
                round_info,
                ThreadRuntimeStatus::Idle,
            )
            .await;
        }
        match outcome {
            Ok(()) => self.monitor.mark_finished(session_id),
            Err(err) if err.code() == "CANCELLED" => {
                let cancel_source = err
                    .detail()
                    .and_then(|detail| detail.get("cancel_source"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if let Some(cancel_source) = cancel_source {
                    self.monitor
                        .mark_cancelled_with_source(session_id, cancel_source);
                } else {
                    self.monitor.mark_cancelled(session_id);
                }
            }
            Err(err) => self.monitor.mark_error(session_id, err.message()),
        }
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
        long_task::spawn("orchestrator.memory.auto_extract", async move {
            i18n::with_language(language, async move {
                let enabled_storage = storage.clone();
                let enabled_user_id = user_id.clone();
                let enabled_agent_id = agent_id.clone();
                let enabled = match blocking::run_db("orchestrator.memory.auto_extract.enabled", move || {
                    Ok(crate::services::memory_agent_settings::AgentMemorySettingsService::new(
                        enabled_storage,
                    )
                    .auto_extract_enabled(&enabled_user_id, enabled_agent_id.as_deref()))
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
                let prepared = match blocking::run_db("orchestrator.memory.auto_extract.prepare", move || {
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
                    Ok(result) => result,
                    Err(err) => {
                        tracing::warn!(
                            target: "wunder_server",
                            user_id = %user_id,
                            agent_id = %agent_id.as_deref().unwrap_or("__default__"),
                            session_id = %session_id,
                            "auto memory extraction job init failed: {err}"
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
                extract_config.temperature = Some(0.1);
                extract_config.stream = Some(false);
                extract_config.stream_include_usage = Some(false);

                if !is_llm_configured(&extract_config) {
                    let finalize_storage = storage.clone();
                    let finalize_job = job.clone();
                    let _ = blocking::run_db("orchestrator.memory.auto_extract.unconfigured", move || {
                        let mut job = finalize_job;
                        crate::services::memory_auto_extract::MemoryAutoExtractService::new(finalize_storage)
                            .finish_job_failed(&mut job, "memory auto extraction llm is not configured");
                        Ok(())
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
                        let _ = blocking::run_db("orchestrator.memory.auto_extract.llm_failed", move || {
                            let mut job = finalize_job;
                            crate::services::memory_auto_extract::MemoryAutoExtractService::new(finalize_storage)
                                .finish_job_failed(&mut job, &error_message);
                            Ok(())
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
                match blocking::run_db("orchestrator.memory.auto_extract.apply", move || {
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
                    Ok(outcome) => {
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
                    Err(err) => {
                        tracing::warn!(
                            target: "wunder_server",
                            user_id = %user_id,
                            agent_id = %agent_id.as_deref().unwrap_or("__default__"),
                            session_id = %session_id,
                            "auto memory extraction failed: {err}"
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
}
