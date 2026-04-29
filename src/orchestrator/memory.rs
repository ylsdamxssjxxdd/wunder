use super::execute::{emit_turn_terminal_event, turn_terminal_status_for_error, TurnTerminalEvent};
use super::thread_runtime::ThreadRuntimeStatus;
use super::*;

const COMPACTION_MIN_CURRENT_USER_MESSAGE_TOKENS: i64 = 64;
const COMPACTION_RETAINED_INTERACTION_EXCHANGE_COUNT_PER_SIDE: usize = 2;
const COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE: usize =
    COMPACTION_RETAINED_INTERACTION_EXCHANGE_COUNT_PER_SIDE * 2;
const COMPACTION_RETAINED_HEAD_INTERACTION_TOKENS: i64 = 8_192;
const COMPACTION_RETAINED_TAIL_INTERACTION_TOKENS: i64 = 16_384;
const COMPACTION_RETAINED_INTERACTION_MESSAGE_MAX_TOKENS: i64 = 5_120;
const COMPACTION_RETAINED_INTERACTION_TURN_MAX_CHARS: usize = 20_000;
const PROMPT_MEMORY_RECALL_LIMIT: usize = 30;
const COMPACTION_INFLIGHT_CURRENT_USER_META_KEY: &str = "compaction_inflight_current_user";
const COMPACTION_RETAINED_INTERACTION_META_KEY: &str = "compaction_retained_interaction";
const COMPACTION_SUMMARY_REASONING_EFFORT: &str = "none";
const COMPACTION_SUMMARY_OBSERVATION_MAX_TOKENS: i64 = 256;
const COMPACTION_TEXT_TRUNCATION_SUFFIX: &str = "...(truncated)";
const COMPACTION_SUMMARY_MAX_CHARS: usize = 20_000;
const COMPACTION_MIN_SUMMARY_MEANINGFUL_CHARS: usize = 8;
const COMPACTION_DEBUG_PREVIEW_CHARS: usize = 240;
const COMPACTION_MIN_RETAINED_INTERACTION_TOKENS: i64 = 128;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CompactionRunMode {
    Manual,
    AutoLoop,
    OverflowRecovery,
}

impl CompactionRunMode {
    fn trigger_mode(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::AutoLoop => "auto_loop",
            Self::OverflowRecovery => "overflow_recovery",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CompactionExecutionProfile {
    run_mode: CompactionRunMode,
    prefer_preserving_summary: bool,
}

impl CompactionExecutionProfile {
    fn new(run_mode: CompactionRunMode) -> Self {
        Self {
            run_mode,
            prefer_preserving_summary: true,
        }
    }

    fn trigger_mode(self) -> &'static str {
        self.run_mode.trigger_mode()
    }
}

fn insert_compaction_trigger_mode(
    payload: &mut serde_json::Map<String, Value>,
    trigger_mode: Option<&str>,
) {
    let Some(trigger_mode) = trigger_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    payload
        .entry("trigger_mode".to_string())
        .or_insert_with(|| Value::String(trigger_mode.to_string()));
}

pub(super) fn insert_compaction_id(
    payload: &mut serde_json::Map<String, Value>,
    compaction_id: Option<&str>,
) {
    let Some(compaction_id) = compaction_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    payload
        .entry("compaction_id".to_string())
        .or_insert_with(|| Value::String(compaction_id.to_string()));
}

fn new_compaction_id(run_mode: CompactionRunMode) -> String {
    format!(
        "cmp_{}_{}",
        run_mode.trigger_mode(),
        Uuid::new_v4().simple()
    )
}

#[derive(Debug)]
pub(super) struct CompactionResult {
    pub(super) messages: Vec<Value>,
    pub(super) compaction_id: Option<String>,
}

impl CompactionResult {
    fn unchanged(messages: Vec<Value>) -> Self {
        Self {
            messages,
            compaction_id: None,
        }
    }

    fn compacted(messages: Vec<Value>, compaction_id: String) -> Self {
        Self {
            messages,
            compaction_id: Some(compaction_id),
        }
    }
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
        request_overhead_tokens: i64,
        force: bool,
        exclude_current_user: bool,
        run_mode: CompactionRunMode,
    ) -> Result<CompactionResult, OrchestratorError> {
        let compaction_profile = CompactionExecutionProfile::new(run_mode);
        let trigger_mode = Some(compaction_profile.trigger_mode());
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
            return Ok(CompactionResult::unchanged(messages));
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
        let mut summary_input = messages.clone();
        let compaction_message = json!({ "role": "user", "content": compaction_instruction });
        if let Some(index) = current_user_index {
            summary_input[index] = compaction_message;
        } else {
            summary_input.push(compaction_message);
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
        let source_interaction_blocks = collect_normalized_interaction_blocks(&source_messages);
        let source_interaction_messages = source_interaction_blocks
            .iter()
            .map(|block| block.message.clone())
            .collect::<Vec<_>>();
        let source_interaction_block_count = source_interaction_messages.len();
        let source_interaction_tokens = estimate_messages_tokens(&source_interaction_messages);
        let retained_segments = collect_retained_interaction_segments_with_indexes_for_compaction(
            &source_messages,
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
        let mut base_messages: Vec<Value> = Vec::new();
        if let Some(system_message) = replay_system_message.clone() {
            base_messages.push(system_message);
        }
        base_messages.extend(retained_head_messages.iter().cloned());
        base_messages.extend(retained_tail_messages.iter().cloned());
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

        let mut current_user_message_for_history_trimmed = false;
        let current_user_message_for_request = current_user_message.as_ref().map(|message| {
            let mut candidate =
                if let Some(trimmed) = trim_message_to_fit_tokens(message, message_budget) {
                    current_user_message_for_history_trimmed = true;
                    trimmed
                } else {
                    message.clone()
                };
            mark_current_user_message_inflight(&mut candidate);
            candidate
        });

        let mut rebuilt = Vec::new();
        if let Some(system_message) = replay_system_message {
            rebuilt.push(system_message);
        }
        rebuilt.extend(retained_head_messages_for_rebuilt);
        rebuilt.push(json!({ "role": "user", "content": summary_text }));
        rebuilt.extend(retained_tail_messages_for_rebuilt);
        if let Some(current_user_message) = current_user_message_for_request {
            rebuilt.push(current_user_message);
        } else if !question_text.is_empty() {
            let mut current_user_placeholder = json!({ "role": "user", "content": question_text });
            mark_current_user_message_inflight(&mut current_user_placeholder);
            rebuilt.push(current_user_placeholder);
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
        let rebuilt_tokens = estimate_messages_tokens(&rebuilt);
        let rebuilt_request_tokens = rebuilt_tokens.saturating_add(request_overhead_tokens);
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
        );

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
            json!(rebuilt_request_tokens),
        );
        compaction_payload_map.insert("history_usage".to_string(), json!(history_usage));
        compaction_payload_map.insert("context_tokens".to_string(), json!(history_usage));
        compaction_payload_map.insert("context_tokens_after".to_string(), json!(rebuilt_tokens));
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
            json!(rebuilt_request_tokens),
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
        let context_tokens = context_manager.estimate_context_tokens(&messages);
        self.workspace
            .save_session_context_tokens_async(user_id, session_id, context_tokens)
            .await;
        let mut context_payload = json!({
            "context_tokens": context_tokens,
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
            Err(err) if err.code() == "CANCELLED" => self.monitor.mark_cancelled(session_id),
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

fn reduce_to_summary_priority_context(
    messages: &mut Vec<Value>,
    limit: i64,
    stats: &mut RebuiltContextGuardStats,
) {
    let summary_index = locate_compaction_summary_message_index(messages);
    let current_user_index = locate_rebuilt_current_user_index(messages);

    let mut prioritized = Vec::new();
    if let Some(system_message) = messages
        .first()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .cloned()
    {
        prioritized.push(system_message);
    }
    if let Some(summary_index) = summary_index {
        prioritized.extend(
            messages[..summary_index]
                .iter()
                .filter(|message| is_retained_interaction_message(message))
                .cloned(),
        );
        prioritized.push(messages[summary_index].clone());
    }
    if let Some(summary_index) = summary_index {
        let tail_end = current_user_index.unwrap_or(messages.len());
        if summary_index + 1 < tail_end {
            prioritized.extend(
                messages[summary_index + 1..tail_end]
                    .iter()
                    .filter(|message| is_retained_interaction_message(message))
                    .cloned(),
            );
        }
    }
    if let Some(current_user_index) = current_user_index {
        if Some(current_user_index) != summary_index {
            prioritized.push(messages[current_user_index].clone());
        }
    }

    if prioritized.is_empty() {
        return;
    }

    *messages = prioritized;
    let mut total_tokens = estimate_messages_tokens(messages);

    if total_tokens > limit {
        let current_user_index = locate_rebuilt_current_user_index(messages);
        let summary_index = locate_compaction_summary_message_index(messages);
        if let (Some(summary_index), Some(current_user_index)) = (summary_index, current_user_index)
        {
            if current_user_index != summary_index {
                stats.current_user_tokens_before = stats
                    .current_user_tokens_before
                    .max(estimate_message_tokens(&messages[current_user_index]));
                let remaining_for_current =
                    (limit - (total_tokens - stats.current_user_tokens_before)).max(1);
                if let Some(trimmed) =
                    trim_message_to_fit_tokens(&messages[current_user_index], remaining_for_current)
                {
                    stats.current_user_tokens_after = estimate_message_tokens(&trimmed);
                    stats.current_user_trimmed = stats.current_user_trimmed
                        || stats.current_user_tokens_after < stats.current_user_tokens_before;
                    messages[current_user_index] = trimmed;
                    total_tokens = estimate_messages_tokens(messages);
                }
            }
        }
    }

    if total_tokens > limit {
        if let Some(summary_index) = locate_compaction_summary_message_index(messages) {
            stats.summary_tokens_before = stats
                .summary_tokens_before
                .max(estimate_message_tokens(&messages[summary_index]));
            let remaining_for_summary =
                (limit - (total_tokens - stats.summary_tokens_before)).max(1);
            if let Some(trimmed) = trim_compaction_summary_message_to_fit_tokens(
                &messages[summary_index],
                remaining_for_summary,
            ) {
                let trimmed_summary =
                    extract_guard_content_text(trimmed.get("content").unwrap_or(&Value::Null));
                if !is_invalid_compaction_summary(&trimmed_summary) {
                    stats.summary_tokens_after = estimate_message_tokens(&trimmed);
                    stats.summary_trimmed = stats.summary_trimmed
                        || stats.summary_tokens_after < stats.summary_tokens_before;
                    messages[summary_index] = trimmed;
                }
            }
        }
    }

    if total_tokens > limit {
        rebalance_retained_interaction_context(messages, limit);
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        if tighten_retained_interaction_context(messages, limit) {
            total_tokens = estimate_messages_tokens(messages);
        }
    }

    if total_tokens > limit {
        let current_user_index = locate_rebuilt_current_user_index(messages);
        let summary_index = locate_compaction_summary_message_index(messages);
        *messages = messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                if message.get("role").and_then(Value::as_str) == Some("system")
                    || Some(index) == summary_index
                    || Some(index) == current_user_index
                {
                    Some(message.clone())
                } else {
                    None
                }
            })
            .collect();
    }
}

fn trim_summary_to_preserve_retained_interaction_budget(
    messages: &mut Vec<Value>,
    limit: i64,
    stats: &mut RebuiltContextGuardStats,
) {
    if messages.is_empty() || limit <= 0 {
        return;
    }

    let retained_total_tokens = messages
        .iter()
        .filter(|message| is_retained_interaction_message(message))
        .map(estimate_message_tokens)
        .sum::<i64>();
    if retained_total_tokens <= 0 {
        return;
    }

    let total_tokens = estimate_messages_tokens(messages);
    if total_tokens <= limit {
        return;
    }

    let retained_floor = retained_total_tokens.min(COMPACTION_MIN_RETAINED_INTERACTION_TOKENS);
    let reducible_retained_tokens = retained_total_tokens.saturating_sub(retained_floor);
    let overflow = total_tokens - limit;
    if overflow <= reducible_retained_tokens {
        return;
    }

    let Some(summary_index) = locate_compaction_summary_message_index(messages) else {
        return;
    };
    let summary_tokens_before = estimate_message_tokens(&messages[summary_index]);
    if summary_tokens_before <= 1 {
        return;
    }

    let required_summary_reduction = overflow - reducible_retained_tokens;
    let target_tokens = summary_tokens_before
        .saturating_sub(required_summary_reduction)
        .max(1);
    let Some(trimmed) =
        trim_compaction_summary_message_to_fit_tokens(&messages[summary_index], target_tokens)
    else {
        return;
    };
    let trimmed_summary =
        extract_guard_content_text(trimmed.get("content").unwrap_or(&Value::Null));
    if is_invalid_compaction_summary(&trimmed_summary) {
        return;
    }

    stats.summary_tokens_before = stats.summary_tokens_before.max(summary_tokens_before);
    stats.summary_tokens_after = estimate_message_tokens(&trimmed);
    stats.summary_trimmed |= stats.summary_tokens_after < stats.summary_tokens_before;
    messages[summary_index] = trimmed;
}

fn rebalance_retained_interaction_context(messages: &mut Vec<Value>, limit: i64) {
    if messages.is_empty() || limit <= 0 {
        return;
    }

    let summary_index = locate_compaction_summary_message_index(messages);
    let current_user_index = locate_rebuilt_current_user_index(messages);
    let preserved_tokens = messages
        .iter()
        .enumerate()
        .filter(|(index, message)| {
            !is_retained_interaction_message(message)
                || Some(*index) == summary_index
                || Some(*index) == current_user_index
        })
        .map(|(_, message)| estimate_message_tokens(message))
        .sum::<i64>();
    let remaining = limit.saturating_sub(preserved_tokens);

    let head_messages = summary_index
        .map(|summary_index| {
            messages[..summary_index]
                .iter()
                .filter(|message| is_retained_interaction_message(message))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tail_messages = match summary_index {
        Some(summary_index) => {
            let tail_end = current_user_index.unwrap_or(messages.len());
            if summary_index + 1 >= tail_end {
                Vec::new()
            } else {
                messages[summary_index + 1..tail_end]
                    .iter()
                    .filter(|message| is_retained_interaction_message(message))
                    .cloned()
                    .collect::<Vec<_>>()
            }
        }
        None => Vec::new(),
    };

    let head_tokens_total = estimate_messages_tokens(&head_messages);
    let tail_tokens_total = estimate_messages_tokens(&tail_messages);
    let total_tokens = head_tokens_total.saturating_add(tail_tokens_total);
    let (head_budget, tail_budget) = if remaining <= 0 || total_tokens <= 0 {
        (0, 0)
    } else if total_tokens <= remaining {
        (head_tokens_total, tail_tokens_total)
    } else {
        let mut head_budget = remaining
            .saturating_mul(head_tokens_total)
            .checked_div(total_tokens)
            .unwrap_or(0);
        if head_tokens_total > 0 && head_budget == 0 {
            head_budget = 1;
        }
        let mut tail_budget = remaining.saturating_sub(head_budget);
        if tail_tokens_total > 0 && tail_budget == 0 && remaining > 1 {
            tail_budget = 1;
            head_budget = remaining.saturating_sub(1);
        }
        (head_budget, tail_budget)
    };
    let retained_head =
        collect_retained_interaction_messages_from_window(&head_messages, head_budget, false);
    let retained_tail =
        collect_retained_interaction_messages_from_window(&tail_messages, tail_budget, true);

    let system_message = messages
        .first()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .cloned();
    let summary_message = summary_index.and_then(|index| messages.get(index)).cloned();
    let current_user_message = current_user_index
        .and_then(|index| messages.get(index))
        .cloned();

    let mut rebuilt = Vec::new();
    if let Some(system_message) = system_message {
        rebuilt.push(system_message);
    }
    rebuilt.extend(retained_head);
    if let Some(summary_message) = summary_message {
        rebuilt.push(summary_message);
    }
    rebuilt.extend(retained_tail);
    if let Some(current_user_message) = current_user_message {
        if !is_compaction_inflight_current_user_message(&current_user_message)
            || rebuilt.last() != Some(&current_user_message)
        {
            rebuilt.push(current_user_message);
        }
    }
    *messages = rebuilt;
}

fn tighten_retained_interaction_context(messages: &mut Vec<Value>, limit: i64) -> bool {
    if messages.is_empty() || limit <= 0 {
        return false;
    }

    let mut changed = false;
    loop {
        let total_tokens = estimate_messages_tokens(messages);
        if total_tokens <= limit {
            break;
        }
        let overflow = total_tokens - limit;
        let retained_candidate = messages
            .iter()
            .enumerate()
            .filter(|(_, message)| is_retained_interaction_message(message))
            .max_by_key(|(_, message)| estimate_message_tokens(message))
            .map(|(index, message)| (index, estimate_message_tokens(message)));
        let Some((index, retained_tokens)) = retained_candidate else {
            break;
        };
        if retained_tokens <= 1 {
            messages.remove(index);
            changed = true;
            continue;
        }

        let target_tokens =
            (retained_tokens - overflow).clamp(1, retained_tokens.saturating_sub(1));
        let trimmed = trim_message_to_fit_tokens(&messages[index], target_tokens);
        let next_message =
            trimmed.filter(|candidate| estimate_message_tokens(candidate) < retained_tokens);

        if let Some(next_message) = next_message {
            messages[index] = next_message;
        } else {
            messages.remove(index);
        }
        changed = true;
    }

    changed
}

fn apply_rebuilt_context_guard(
    messages: &mut Vec<Value>,
    limit: i64,
    prefer_preserving_summary: bool,
) -> RebuiltContextGuardStats {
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

    if total_tokens > limit && !prefer_preserving_summary {
        if let Some(summary_index) = locate_compaction_summary_message_index(messages) {
            stats.summary_tokens_before = estimate_message_tokens(&messages[summary_index]);
            let remaining_for_summary =
                (limit - (total_tokens - stats.summary_tokens_before)).max(1);
            if let Some(trimmed) = trim_compaction_summary_message_to_fit_tokens(
                &messages[summary_index],
                remaining_for_summary,
            ) {
                let trimmed_summary =
                    extract_guard_content_text(trimmed.get("content").unwrap_or(&Value::Null));
                if !is_invalid_compaction_summary(&trimmed_summary) {
                    stats.summary_tokens_after = estimate_message_tokens(&trimmed);
                    stats.summary_trimmed =
                        stats.summary_tokens_after < stats.summary_tokens_before;
                    messages[summary_index] = trimmed;
                } else {
                    stats.summary_tokens_after = stats.summary_tokens_before;
                }
            } else {
                stats.summary_tokens_after = stats.summary_tokens_before;
            }
        }
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        loop {
            let summary_index = locate_compaction_summary_message_index(messages);
            let current_user_index = locate_rebuilt_current_user_index(messages);
            let removable_index = messages.iter().enumerate().find_map(|(index, message)| {
                if Some(index) == summary_index || Some(index) == current_user_index {
                    return None;
                }
                if message.get("role").and_then(Value::as_str) == Some("system") {
                    return None;
                }
                if is_retained_interaction_message(message) {
                    return None;
                }
                Some(index)
            });
            let Some(index) = removable_index else {
                break;
            };
            messages.remove(index);
            total_tokens = estimate_messages_tokens(messages);
            if total_tokens <= limit {
                break;
            }
        }
    }

    if total_tokens > limit {
        rebalance_retained_interaction_context(messages, limit);
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit && prefer_preserving_summary {
        trim_summary_to_preserve_retained_interaction_budget(messages, limit, &mut stats);
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        if tighten_retained_interaction_context(messages, limit) {
            total_tokens = estimate_messages_tokens(messages);
        }
    }

    if total_tokens > limit && !prefer_preserving_summary {
        let summary_index = locate_compaction_summary_message_index(messages);
        let current_user_index = locate_rebuilt_current_user_index(messages);
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
        let current_user_index = locate_rebuilt_current_user_index(messages);
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

    if total_tokens > limit && prefer_preserving_summary {
        if let Some(summary_index) = locate_compaction_summary_message_index(messages) {
            stats.summary_tokens_before = stats
                .summary_tokens_before
                .max(estimate_message_tokens(&messages[summary_index]));
            let remaining_for_summary =
                (limit - (total_tokens - stats.summary_tokens_before)).max(1);
            if let Some(trimmed) = trim_compaction_summary_message_to_fit_tokens(
                &messages[summary_index],
                remaining_for_summary,
            ) {
                let trimmed_summary =
                    extract_guard_content_text(trimmed.get("content").unwrap_or(&Value::Null));
                if !is_invalid_compaction_summary(&trimmed_summary) {
                    stats.summary_tokens_after = estimate_message_tokens(&trimmed);
                    stats.summary_trimmed = stats.summary_trimmed
                        || stats.summary_tokens_after < stats.summary_tokens_before;
                    messages[summary_index] = trimmed;
                    total_tokens = estimate_messages_tokens(messages);
                }
            }
        }
    }

    if total_tokens > limit {
        if let Some(last_index) = messages.len().checked_sub(1) {
            let last_tokens = estimate_message_tokens(&messages[last_index]);
            let current_user_index = locate_rebuilt_current_user_index(messages);
            let trimming_current_user = current_user_index == Some(last_index);
            let trimming_summary =
                locate_compaction_summary_message_index(messages) == Some(last_index);
            if trimming_current_user && stats.current_user_tokens_before == 0 {
                stats.current_user_tokens_before = last_tokens;
            }
            let remaining_for_last = (limit - (total_tokens - last_tokens)).max(1);
            let trimmed = if trimming_summary {
                trim_compaction_summary_message_to_fit_tokens(
                    &messages[last_index],
                    remaining_for_last,
                )
            } else {
                trim_message_to_fit_tokens(&messages[last_index], remaining_for_last)
            };
            if let Some(trimmed) = trimmed {
                let trimmed_tokens = estimate_message_tokens(&trimmed);
                if trimming_current_user {
                    stats.current_user_tokens_after = trimmed_tokens;
                    stats.current_user_trimmed |= trimmed_tokens < stats.current_user_tokens_before;
                }
                messages[last_index] = trimmed;
                total_tokens = estimate_messages_tokens(messages);
            } else if trimming_summary && last_tokens > remaining_for_last {
                messages.remove(last_index);
                stats.summary_removed = true;
                total_tokens = estimate_messages_tokens(messages);
            }
        }
    }

    if total_tokens > limit && prefer_preserving_summary {
        reduce_to_summary_priority_context(messages, limit, &mut stats);
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        *messages = trim_messages_to_budget(messages, limit);
        stats.fallback_trim_applied = true;
        total_tokens = estimate_messages_tokens(messages);
        if total_tokens > limit {
            if let Some(last_index) = messages.len().checked_sub(1) {
                let trimming_summary =
                    locate_compaction_summary_message_index(messages) == Some(last_index);
                let trimmed = if trimming_summary {
                    trim_compaction_summary_message_to_fit_tokens(
                        &messages[last_index],
                        limit.max(1),
                    )
                } else {
                    trim_message_to_fit_tokens(&messages[last_index], limit.max(1))
                };
                if let Some(trimmed) = trimmed {
                    *messages = vec![trimmed];
                    total_tokens = estimate_messages_tokens(messages);
                } else if trimming_summary {
                    messages.clear();
                    total_tokens = 0;
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
        let content =
            trim_text_to_tokens(&source, target_tokens, COMPACTION_TEXT_TRUNCATION_SUFFIX);
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

fn trim_compaction_summary_message_to_fit_tokens(
    message: &Value,
    max_tokens: i64,
) -> Option<Value> {
    if max_tokens <= 0 || estimate_message_tokens(message) <= max_tokens {
        return None;
    }

    let summary_text = extract_guard_content_text(message.get("content").unwrap_or(&Value::Null));
    if !starts_with_compaction_prefix(&summary_text) {
        return trim_message_to_fit_tokens(message, max_tokens);
    }

    let mut message_obj = message.as_object().cloned().unwrap_or_else(|| {
        let mut fallback = serde_json::Map::new();
        fallback.insert("role".to_string(), Value::String("user".to_string()));
        fallback.insert("content".to_string(), Value::String(summary_text.clone()));
        fallback
    });

    let prefix = i18n::t("history.compaction_prefix");
    let minimum_chars = prefix
        .chars()
        .count()
        .saturating_add(1)
        .saturating_add(COMPACTION_MIN_SUMMARY_MEANINGFUL_CHARS);
    let mut target_chars = ((max_tokens.max(1) as f64) * 4.0).ceil() as usize;
    target_chars = target_chars.max(minimum_chars);

    for _ in 0..4 {
        let content = clamp_committed_compaction_summary(&summary_text, target_chars);
        if is_invalid_compaction_summary(&content) {
            return None;
        }
        message_obj.insert("content".to_string(), Value::String(content));
        message_obj.remove("reasoning_content");
        message_obj.remove("reasoning");
        let candidate = Value::Object(message_obj.clone());
        let cost = estimate_message_tokens(&candidate);
        if cost <= max_tokens {
            return Some(candidate);
        }
        let overflow_chars = ((cost - max_tokens).max(1) as f64 * 4.0).ceil() as usize;
        let next_target = target_chars
            .saturating_sub(overflow_chars)
            .max(minimum_chars);
        if next_target == target_chars {
            break;
        }
        target_chars = next_target;
    }

    None
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
        } else {
            target
        };
        if approx_token_count(&content) > per_message_target {
            content = trim_text_to_tokens(
                &content,
                per_message_target,
                COMPACTION_TEXT_TRUNCATION_SUFFIX,
            );
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
            trim_text_to_tokens(&merged, target, COMPACTION_TEXT_TRUNCATION_SUFFIX)
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
        strip_compaction_internal_tool_lines(&extract_memory_summary_text_value(content))
    };
    if !text.is_empty() {
        return Some(text);
    }
    None
}

fn is_compaction_observation_message(role: &str, obj: &Map<String, Value>) -> bool {
    let content = obj.get("content").unwrap_or(&Value::Null);
    Orchestrator::is_observation_message(role, content)
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

fn strip_compaction_prefix_text(summary: &str) -> String {
    let cleaned = summary.trim();
    if cleaned.is_empty() {
        return String::new();
    }
    for prefix in compaction_prefixes() {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            continue;
        }
        if let Some(rest) = cleaned.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    cleaned.to_string()
}

fn count_meaningful_chars(text: &str) -> usize {
    text.chars().filter(|ch| ch.is_alphanumeric()).count()
}

fn trim_known_compaction_suffix(text: &str) -> &str {
    text.trim_end()
        .strip_suffix(COMPACTION_TEXT_TRUNCATION_SUFFIX)
        .map(str::trim_end)
        .unwrap_or_else(|| text.trim_end())
}

fn is_placeholder_compaction_summary(summary: &str) -> bool {
    let body = strip_compaction_prefix_text(summary);
    let body = body.trim();
    if body.is_empty() {
        return true;
    }

    let compact: String = body.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.is_empty() {
        return true;
    }

    let compact_ascii = compact.to_ascii_lowercase();
    if matches!(compact_ascii.as_str(), "..." | "...(" | "...(truncated)") {
        return true;
    }

    let meaningful = count_meaningful_chars(trim_known_compaction_suffix(body));
    meaningful < COMPACTION_MIN_SUMMARY_MEANINGFUL_CHARS
}

fn is_invalid_compaction_summary(summary: &str) -> bool {
    is_empty_compaction_summary(summary) || is_placeholder_compaction_summary(summary)
}

fn clamp_committed_compaction_summary(summary: &str, max_chars: usize) -> String {
    let body = strip_compaction_prefix_text(summary);
    let body = body.trim();
    if body.is_empty() || max_chars == 0 {
        return String::new();
    }
    let prefix = i18n::t("history.compaction_prefix");
    let reserved_chars = prefix.chars().count().saturating_add(1);
    let body_limit = max_chars.saturating_sub(reserved_chars).max(1);
    let clamped_body = trim_text_to_chars(body, body_limit, COMPACTION_TEXT_TRUNCATION_SUFFIX);
    HistoryManager::format_compaction_summary(&clamped_body)
}

fn build_committable_compaction_summary(
    summary_candidate: &str,
    memory_block: &str,
) -> Option<(String, bool)> {
    if is_invalid_compaction_summary(summary_candidate) {
        return None;
    }
    let formatted = HistoryManager::format_compaction_summary(summary_candidate);
    let (merged, fresh_memory_injected) =
        merge_compaction_summary_with_fresh_memory(&formatted, memory_block);
    let committed = clamp_committed_compaction_summary(&merged, COMPACTION_SUMMARY_MAX_CHARS);
    if is_invalid_compaction_summary(&committed) {
        return None;
    }
    Some((committed, fresh_memory_injected))
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

fn summarize_compaction_fallback_text(text: &str) -> String {
    let mut selected_lines = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let looks_like_metadata = line.starts_with('[') && line.contains(']');
        if looks_like_metadata {
            continue;
        }
        selected_lines.push(line);
        if selected_lines.len() >= 2 {
            break;
        }
    }

    let candidate = if selected_lines.is_empty() {
        text.trim().to_string()
    } else {
        selected_lines.join(" ")
    };
    let collapsed = candidate.split_whitespace().collect::<Vec<_>>().join(" ");
    trim_text_to_chars(&collapsed, 240, COMPACTION_TEXT_TRUNCATION_SUFFIX)
}

fn build_compaction_fallback_summary(messages: &[Value], default_fallback: &str) -> String {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for message in messages.iter().rev() {
        let Some(obj) = message.as_object() else {
            continue;
        };
        let role = obj.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "system" || is_compaction_observation_message(role, obj) {
            continue;
        }
        let raw_text = extract_guard_content_text(obj.get("content").unwrap_or(&Value::Null));
        if raw_text.is_empty() || starts_with_compaction_prefix(&raw_text) {
            continue;
        }
        let text = summarize_compaction_fallback_text(&raw_text);
        if text.is_empty() || !seen.insert(text.clone()) {
            continue;
        }
        let label = if role == "assistant" {
            "Assistant"
        } else {
            "User"
        };
        entries.push(format!("- {label}: {text}"));
        if entries.len() >= 6 {
            break;
        }
    }

    entries.reverse();
    if entries.is_empty() {
        let fallback = summarize_compaction_fallback_text(default_fallback);
        if fallback.is_empty() {
            return i18n::t("compaction.summary_fallback");
        }
        return format!("Compressed earlier context.\n- User: {fallback}");
    }

    format!("Compressed earlier context.\n{}", entries.join("\n"))
}

fn mark_current_user_message_inflight(message: &mut Value) {
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

fn is_compaction_inflight_current_user_message(message: &Value) -> bool {
    message
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get(COMPACTION_INFLIGHT_CURRENT_USER_META_KEY))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn clear_compaction_inflight_current_user_marker(message: &mut Value) {
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

fn build_committed_replacement_history_from_rebuilt(messages: &[Value]) -> Vec<Value> {
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

fn is_compaction_tool_call_summary_text(text: &str) -> bool {
    let cleaned = text.trim();
    cleaned == "Assistant issued tool call(s)."
        || cleaned.starts_with("Assistant issued tool call(s):")
}

fn strip_compaction_internal_tool_lines(text: &str) -> String {
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

fn normalize_committed_replacement_history_message(message: &Value) -> Option<Value> {
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

fn clear_compaction_inflight_markers(messages: &mut [Value]) {
    for message in messages {
        clear_compaction_inflight_current_user_marker(message);
    }
}

fn mark_retained_interaction_message(message: &mut Value) {
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

fn mark_retained_interaction_messages(messages: &mut [Value]) {
    for message in messages {
        mark_retained_interaction_message(message);
    }
}

fn is_retained_interaction_message(message: &Value) -> bool {
    message
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get(COMPACTION_RETAINED_INTERACTION_META_KEY))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn clear_retained_interaction_marker(message: &mut Value) {
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

fn clear_retained_interaction_markers(messages: &mut [Value]) {
    for message in messages {
        clear_retained_interaction_marker(message);
    }
}

fn locate_rebuilt_current_user_index(messages: &[Value]) -> Option<usize> {
    messages
        .iter()
        .rposition(is_compaction_inflight_current_user_message)
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

#[derive(Clone, Debug)]
struct InteractionBlock {
    indexes: Vec<usize>,
    message: Value,
}

fn normalize_message_for_interaction_block(message: &Value) -> Option<(&'static str, String)> {
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

fn build_interaction_block_message(role: &str, parts: &[String]) -> Option<Value> {
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

fn split_messages_into_interaction_turns(messages: &[Value]) -> Vec<InteractionBlock> {
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

fn collect_normalized_interaction_blocks(messages: &[Value]) -> Vec<InteractionBlock> {
    split_messages_into_interaction_turns(messages)
        .iter()
        .filter_map(normalize_interaction_turn_messages)
        .collect()
}

fn estimate_message_chars(message: &Value) -> usize {
    let content = message.get("content").unwrap_or(&Value::Null);
    match content {
        Value::String(text) => text.chars().count(),
        Value::Null => 0,
        _ => extract_guard_content_text(content).chars().count(),
    }
}

fn trim_message_to_fit_chars(message: &Value, max_chars: usize) -> Option<Value> {
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

fn build_compaction_message_debug_entries(messages: &[Value]) -> Vec<Value> {
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

fn normalize_interaction_turn_messages(turn: &InteractionBlock) -> Option<InteractionBlock> {
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

fn trim_interaction_turn_to_budget(
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
    Some(InteractionBlock {
        indexes: turn.indexes.clone(),
        message,
    })
}

fn collect_interaction_turns_with_budget(
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
            selected_turns.push(turn.clone());
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
fn collect_retained_interaction_messages_for_compaction(
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
fn collect_retained_interaction_segments_for_compaction(
    messages: &[Value],
    retained_turn_count: usize,
    head_token_limit: i64,
    tail_token_limit: i64,
) -> (Vec<Value>, Vec<Value>) {
    let segments = collect_retained_interaction_segments_with_indexes_for_compaction(
        messages,
        retained_turn_count,
        head_token_limit,
        tail_token_limit,
    );
    (segments.head_messages, segments.tail_messages)
}

struct RetainedInteractionSegments {
    head_messages: Vec<Value>,
    tail_messages: Vec<Value>,
}

fn collect_retained_interaction_segments_with_indexes_for_compaction(
    messages: &[Value],
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

    if split_messages_into_interaction_turns(messages).is_empty() {
        return RetainedInteractionSegments {
            head_messages: Vec::new(),
            tail_messages: Vec::new(),
        };
    }
    let normalized_turns = collect_normalized_interaction_blocks(messages);
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
fn collect_retained_interaction_messages_from_window(
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
    if configured_limit <= COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS {
        return configured_limit.max(1);
    }
    let adaptive_limit =
        (context_tokens.saturating_mul(3) / 4).max(COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
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
    fn test_resolve_force_compaction_limit_handles_small_configured_limit() {
        let forced = resolve_force_compaction_limit(4_000, 2_048);
        assert_eq!(forced, 2_048);
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
        assert_eq!(summary_config.max_output, Some(2048));
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
    fn test_prepare_compaction_summary_messages_merges_system_and_skips_tool_only_assistant() {
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
        assert_eq!(prepared.len(), 1);
        assert_eq!(
            prepared[0].get("content").and_then(Value::as_str),
            Some("system prompt\n\nartifact index")
        );
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
        let stats = apply_rebuilt_context_guard(&mut messages, limit, false);
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
        let stats = apply_rebuilt_context_guard(&mut messages, limit, false);
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
        let stats = apply_rebuilt_context_guard(&mut messages, limit, false);
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
        let stats = apply_rebuilt_context_guard(&mut messages, limit, false);
        assert!(stats.applied);
        assert!(stats.current_user_trimmed || stats.fallback_trim_applied);
        assert!(estimate_messages_tokens(&messages) <= limit);
    }

    #[test]
    fn test_apply_rebuilt_context_guard_preserves_summary_in_summary_first_mode() {
        let summary = format!(
            "{}\nCompressed earlier context.\n- User: prior request\n- Assistant: prior answer",
            i18n::t("history.compaction_prefix")
        );
        let mut messages = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({ "role": "user", "content": "edge 1 ".repeat(600) }),
            json!({ "role": "assistant", "content": "edge 2 ".repeat(600) }),
            json!({ "role": "user", "content": summary }),
            json!({ "role": "user", "content": "current question ".repeat(900) }),
        ];
        let limit = 700;
        let stats = apply_rebuilt_context_guard(&mut messages, limit, true);
        assert!(stats.applied);
        assert!(
            messages
                .iter()
                .any(|message| HistoryManager::is_compaction_summary_item(message)),
            "summary-first compaction should keep the committed summary in the rebuilt request"
        );
        assert!(estimate_messages_tokens(&messages) <= limit);
    }

    #[test]
    fn test_apply_rebuilt_context_guard_summary_first_keeps_trimmed_retained_interaction() {
        let summary = format!(
            "{}\n{}",
            i18n::t("history.compaction_prefix"),
            "Compressed earlier context. ".repeat(120)
        );
        let mut retained_user = json!({ "role": "user", "content": "round-1 user marker" });
        mark_retained_interaction_message(&mut retained_user);
        let mut retained_assistant = json!({
            "role": "assistant",
            "content": "round-1 assistant marker ".repeat(120)
        });
        mark_retained_interaction_message(&mut retained_assistant);
        let mut messages = vec![
            json!({ "role": "system", "content": "system prompt ".repeat(220) }),
            retained_user,
            retained_assistant,
            json!({ "role": "user", "content": summary }),
        ];
        let limit = estimate_message_tokens(&messages[0])
            + estimate_message_tokens(&messages[3])
            + estimate_message_tokens(&messages[1])
            + 24;

        let stats = apply_rebuilt_context_guard(&mut messages, limit, true);

        assert!(stats.applied);
        assert!(
            messages
                .iter()
                .any(|message| HistoryManager::is_compaction_summary_item(message)),
            "summary-first compaction should keep the summary"
        );
        assert!(
            messages
                .iter()
                .any(is_retained_interaction_message),
            "summary-first compaction should keep a trimmed retained interaction when budget still allows it"
        );
        assert!(estimate_messages_tokens(&messages) <= limit);
    }

    #[test]
    fn test_apply_rebuilt_context_guard_summary_first_trims_summary_to_keep_retained_budget() {
        let summary = format!(
            "{}\n{}",
            i18n::t("history.compaction_prefix"),
            "summary detail ".repeat(220)
        );
        let mut retained_user = json!({ "role": "user", "content": "round-1 user marker" });
        mark_retained_interaction_message(&mut retained_user);
        let mut retained_assistant = json!({
            "role": "assistant",
            "content": "round-1 assistant marker ".repeat(90)
        });
        mark_retained_interaction_message(&mut retained_assistant);
        let current_user = json!({ "role": "user", "content": "current user marker" });
        let mut messages = vec![
            json!({ "role": "system", "content": "system prompt ".repeat(260) }),
            retained_user,
            retained_assistant,
            json!({ "role": "user", "content": summary.clone() }),
            current_user,
        ];
        let limit = estimate_messages_tokens(&messages) - estimate_message_tokens(&messages[2])
            + COMPACTION_MIN_RETAINED_INTERACTION_TOKENS
            - 16;

        let stats = apply_rebuilt_context_guard(&mut messages, limit, true);

        assert!(stats.applied);
        assert!(
            stats.summary_trimmed,
            "summary-first guard should trim summary before sacrificing the entire retained window"
        );
        assert!(
            messages
                .iter()
                .any(is_retained_interaction_message),
            "summary-first guard should keep some retained interaction content after trimming the summary"
        );
        assert!(
            messages
                .iter()
                .any(|message| HistoryManager::is_compaction_summary_item(message)),
            "summary-first guard should still keep the summary"
        );
        assert!(estimate_messages_tokens(&messages) <= limit);
    }

    #[test]
    fn test_apply_rebuilt_context_guard_never_keeps_partial_compaction_prefix() {
        let summary = format!(
            "{}\n{}",
            i18n::t("history.compaction_prefix"),
            "summary detail ".repeat(220)
        );
        let mut messages = vec![
            json!({ "role": "system", "content": "system prompt ".repeat(260) }),
            json!({ "role": "user", "content": summary }),
        ];
        let limit = estimate_message_tokens(&messages[0]) + 8;

        let stats = apply_rebuilt_context_guard(&mut messages, limit, true);

        assert!(stats.applied);
        assert!(estimate_messages_tokens(&messages) <= limit);
        assert!(
            messages.iter().all(|message| {
                let content = message.get("content").and_then(Value::as_str).unwrap_or("");
                content.is_empty()
                    || !content.starts_with("[上下文")
                    || starts_with_compaction_prefix(content)
            }),
            "guard should not keep a broken partial compaction prefix"
        );
    }

    #[test]
    fn test_trim_compaction_summary_message_to_fit_tokens_preserves_prefix_shape() {
        let summary = json!({
            "role": "user",
            "content": format!(
                "{}\n{}",
                i18n::t("history.compaction_prefix"),
                "summary detail ".repeat(240)
            )
        });

        let trimmed = trim_compaction_summary_message_to_fit_tokens(&summary, 24)
            .expect("trimmed compaction summary");
        let content = trimmed
            .get("content")
            .and_then(Value::as_str)
            .expect("summary content");

        assert!(
            starts_with_compaction_prefix(content),
            "trimmed compaction summary should keep a valid compaction prefix: {content}"
        );
        assert!(
            !is_invalid_compaction_summary(content),
            "trimmed compaction summary should remain a valid summary: {content}"
        );
        assert!(estimate_message_tokens(&trimmed) <= 24);
    }

    #[test]
    fn test_apply_rebuilt_context_guard_summary_first_keeps_valid_summary_and_retained_window() {
        let summary = format!(
            "{}\n{}",
            i18n::t("history.compaction_prefix"),
            "summary detail ".repeat(260)
        );
        let mut retained_user = json!({ "role": "user", "content": "retained user anchor" });
        mark_retained_interaction_message(&mut retained_user);
        let mut retained_assistant = json!({
            "role": "assistant",
            "content": "retained assistant anchor ".repeat(100)
        });
        mark_retained_interaction_message(&mut retained_assistant);
        let mut current_user = json!({ "role": "user", "content": "current follow-up request" });
        mark_current_user_message_inflight(&mut current_user);
        let mut messages = vec![
            json!({ "role": "system", "content": "system prompt ".repeat(120) }),
            retained_user,
            retained_assistant,
            json!({ "role": "user", "content": summary }),
            current_user,
        ];
        let limit = estimate_messages_tokens(&messages).saturating_sub(1);

        let stats = apply_rebuilt_context_guard(&mut messages, limit, true);

        assert!(stats.applied);
        assert!(estimate_messages_tokens(&messages) <= limit);
        assert!(
            messages.iter().any(is_retained_interaction_message),
            "summary-first guard should keep retained interaction content in the rebuilt request"
        );
        let summary_message = messages
            .iter()
            .find(|message| HistoryManager::is_compaction_summary_item(message))
            .expect("summary message");
        let summary_text = summary_message
            .get("content")
            .and_then(Value::as_str)
            .expect("summary text");
        assert!(
            starts_with_compaction_prefix(summary_text),
            "summary-first guard should keep a valid compaction summary prefix: {summary_text}"
        );
        assert!(
            !is_invalid_compaction_summary(summary_text),
            "summary-first guard should keep a valid compaction summary body: {summary_text}"
        );
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
    fn test_collect_retained_interaction_messages_for_compaction_keeps_first_and_recent_blocks() {
        let messages = vec![
            json!({ "role": "user", "content": "round-1 user" }),
            json!({ "role": "assistant", "content": "round-1 assistant" }),
            json!({ "role": "user", "content": "round-2 user" }),
            json!({ "role": "assistant", "content": "round-2 assistant" }),
            json!({ "role": "user", "content": "round-3 user" }),
            json!({ "role": "assistant", "content": "round-3 assistant" }),
            json!({ "role": "user", "content": "round-4 user" }),
            json!({ "role": "assistant", "content": "round-4 assistant" }),
            json!({ "role": "user", "content": "round-5 user" }),
            json!({ "role": "assistant", "content": "round-5 assistant" }),
        ];

        let retained = collect_retained_interaction_messages_for_compaction(
            &messages,
            COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE,
            COMPACTION_RETAINED_HEAD_INTERACTION_TOKENS,
            COMPACTION_RETAINED_TAIL_INTERACTION_TOKENS,
        );
        let contents = retained
            .iter()
            .map(|message| message["content"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            contents,
            vec![
                "round-1 user".to_string(),
                "round-1 assistant".to_string(),
                "round-2 user".to_string(),
                "round-2 assistant".to_string(),
                "round-4 user".to_string(),
                "round-4 assistant".to_string(),
                "round-5 user".to_string(),
                "round-5 assistant".to_string(),
            ]
        );
    }

    #[test]
    fn test_collect_retained_interaction_messages_for_compaction_keeps_oldest_and_latest_task_blocks(
    ) {
        let messages = vec![
            json!({ "role": "user", "content": "[SWARM_CONTEXT]\\nolder task" }),
            json!({ "role": "assistant", "content": "older answer" }),
            json!({ "role": "user", "content": "current question" }),
            json!({ "role": "assistant", "content": "searching current task" }),
            json!({
                "role": "user",
                "content": format!(
                    "{OBSERVATION_PREFIX}{}",
                    json!({ "tool": "search_content", "ok": true, "summary": "11 hits" })
                )
            }),
            json!({ "role": "assistant", "content": "reading current task files" }),
        ];

        let retained = collect_retained_interaction_messages_for_compaction(
            &messages,
            COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE,
            COMPACTION_RETAINED_HEAD_INTERACTION_TOKENS,
            COMPACTION_RETAINED_TAIL_INTERACTION_TOKENS,
        );
        let roles = retained
            .iter()
            .map(|message| message["role"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();
        let contents = retained
            .iter()
            .map(|message| message["content"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            roles,
            vec![
                "user".to_string(),
                "assistant".to_string(),
                "user".to_string(),
                "assistant".to_string(),
                "user".to_string(),
                "assistant".to_string(),
            ]
        );
        assert_eq!(
            contents,
            vec![
                "[SWARM_CONTEXT]\\nolder task".to_string(),
                "older answer".to_string(),
                "current question".to_string(),
                "searching current task".to_string(),
                "Tool observation (search_content): success\n11 hits".to_string(),
                "reading current task files".to_string(),
            ]
        );
    }

    #[test]
    fn test_split_messages_into_interaction_turns_merges_same_side_content_and_observations() {
        let messages = vec![
            json!({ "role": "user", "content": "round-1 user" }),
            json!({
                "role": "assistant",
                "content": "Inspecting\nAssistant issued tool call(s): read_file",
                "tool_calls": [{ "function": { "name": "read_file" } }]
            }),
            json!({
                "role": "assistant",
                "content": "Reading file details",
            }),
            json!({
                "role": "user",
                "content": format!("{OBSERVATION_PREFIX}{}", json!({
                    "tool": "read_file",
                    "ok": true,
                    "summary": "Loaded /tmp/demo.txt"
                }))
            }),
            json!({ "role": "assistant", "content": "round-1 answer" }),
        ];

        let turns = split_messages_into_interaction_turns(&messages);
        let roles = turns
            .iter()
            .map(|turn| turn.message["role"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();
        let contents = turns
            .iter()
            .map(|turn| turn.message["content"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            roles,
            vec![
                "user".to_string(),
                "assistant".to_string(),
                "user".to_string(),
                "assistant".to_string(),
            ]
        );
        assert!(contents
            .iter()
            .all(|content| !content.contains("Assistant issued tool call(s):")));
        assert_eq!(
            contents,
            vec![
                "round-1 user".to_string(),
                "Inspecting\n\nReading file details".to_string(),
                "Tool observation (read_file): success\nLoaded /tmp/demo.txt".to_string(),
                "round-1 answer".to_string(),
            ]
        );
    }

    #[test]
    fn test_collect_retained_interaction_segments_for_compaction_avoids_overlap_duplication() {
        let messages = vec![
            json!({ "role": "user", "content": "round-1 user" }),
            json!({ "role": "assistant", "content": "round-1 assistant" }),
            json!({ "role": "user", "content": "round-2 user" }),
            json!({ "role": "assistant", "content": "round-2 assistant" }),
            json!({ "role": "user", "content": "round-3 user" }),
            json!({ "role": "assistant", "content": "round-3 assistant" }),
        ];

        let (head, tail) = collect_retained_interaction_segments_for_compaction(
            &messages,
            COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE,
            COMPACTION_RETAINED_HEAD_INTERACTION_TOKENS,
            COMPACTION_RETAINED_TAIL_INTERACTION_TOKENS,
        );

        let head_contents = head
            .iter()
            .map(|message| message["content"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();
        let tail_contents = tail
            .iter()
            .map(|message| message["content"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            head_contents,
            vec![
                "round-1 user".to_string(),
                "round-1 assistant".to_string(),
                "round-2 user".to_string(),
                "round-2 assistant".to_string(),
            ]
        );
        assert_eq!(
            tail_contents,
            vec!["round-3 user".to_string(), "round-3 assistant".to_string(),]
        );
    }

    #[test]
    fn test_collect_retained_interaction_messages_for_compaction_trims_large_block_to_budget() {
        let large_assistant = "assistant detail ".repeat(8_000);
        let messages = vec![
            json!({ "role": "user", "content": "round-1 user" }),
            json!({ "role": "assistant", "content": large_assistant }),
            json!({ "role": "user", "content": "round-2 user" }),
            json!({ "role": "assistant", "content": "round-2 assistant" }),
        ];

        let retained = collect_retained_interaction_messages_for_compaction(&messages, 2, 256, 256);
        assert_eq!(retained.len(), 4);
        assert_eq!(retained[0]["content"], json!("round-1 user"));
        assert_eq!(retained[2]["content"], json!("round-2 user"));
        assert_eq!(retained[3]["content"], json!("round-2 assistant"));
        assert!(estimate_message_tokens(&retained[1]) <= 256);
        assert_ne!(retained[1]["content"], json!(large_assistant));
    }

    #[test]
    fn test_build_compaction_message_debug_entries_marks_summary_and_current_user() {
        let mut current_user = json!({ "role": "user", "content": "current question" });
        mark_current_user_message_inflight(&mut current_user);
        let messages = vec![
            json!({
                "role": "user",
                "content": format!("{}\nsummary", i18n::t("history.compaction_prefix"))
            }),
            current_user,
        ];

        let debug = build_compaction_message_debug_entries(&messages);

        assert_eq!(debug.len(), 2);
        assert_eq!(debug[0]["is_summary"], json!(true));
        assert_eq!(debug[0]["is_current_user"], json!(false));
        assert_eq!(debug[1]["is_summary"], json!(false));
        assert_eq!(debug[1]["is_current_user"], json!(true));
    }

    #[test]
    fn test_build_committable_compaction_summary_rejects_placeholder_fragment() {
        assert!(build_committable_compaction_summary("...(truncated)", "").is_none());
        assert!(build_committable_compaction_summary("ok", "").is_none());
    }

    #[test]
    fn test_build_committable_compaction_summary_clamps_to_char_limit() {
        let summary = format!(
            "Project status:\n{}\nNext steps:\n{}",
            "A".repeat(11_000),
            "B".repeat(11_000)
        );
        let memory_block = format!(
            "{}\n- User prefers concise diffs",
            i18n::t("memory.block_prefix")
        );

        let (committed, injected) =
            build_committable_compaction_summary(&summary, &memory_block).expect("summary");
        assert!(injected);
        assert!(committed.starts_with(&i18n::t("history.compaction_prefix")));
        assert!(committed.chars().count() <= COMPACTION_SUMMARY_MAX_CHARS);
        assert!(!is_invalid_compaction_summary(&committed));
    }

    #[test]
    fn test_build_committed_replacement_history_from_rebuilt_strips_system_and_inflight_user() {
        let mut inflight_user = json!({ "role": "user", "content": "current question" });
        mark_current_user_message_inflight(&mut inflight_user);
        let rebuilt = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({ "role": "assistant", "content": "tail answer" }),
            json!({
                "role": "user",
                "content": format!("{}\nsummary", i18n::t("history.compaction_prefix"))
            }),
            inflight_user,
        ];

        let committed = build_committed_replacement_history_from_rebuilt(&rebuilt);

        assert_eq!(committed.len(), 2);
        assert_eq!(committed[0]["role"], json!("assistant"));
        assert_eq!(committed[1]["role"], json!("user"));
        assert!(committed
            .iter()
            .all(|item| !is_compaction_inflight_current_user_message(item)));
    }

    #[test]
    fn test_build_committed_replacement_history_from_rebuilt_strips_internal_compaction_artifacts()
    {
        let mut inflight_user = json!({ "role": "user", "content": "current question" });
        mark_current_user_message_inflight(&mut inflight_user);
        let rebuilt = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({
                "role": "assistant",
                "content": "I will inspect the file.\nAssistant issued tool call(s): read_file",
                "tool_calls": [{
                    "function": { "name": "read_file" }
                }],
                "tool_call_id": "call_1"
            }),
            json!({
                "role": "user",
                "content": format!("{OBSERVATION_PREFIX}{}", json!({
                    "tool": "read_file",
                    "ok": true,
                    "data": {
                        "results_jsonl": "{\"path\":\"/tmp/demo.txt\"}"
                    }
                })),
            }),
            json!({
                "role": "assistant",
                "content": "Assistant issued tool call(s): read_file",
                "tool_calls": [{
                    "function": { "name": "read_file" }
                }]
            }),
            json!({
                "role": "user",
                "content": format!("{}\nsummary", i18n::t("history.compaction_prefix"))
            }),
            inflight_user,
        ];

        let committed = build_committed_replacement_history_from_rebuilt(&rebuilt);

        assert_eq!(
            committed,
            vec![
                json!({ "role": "assistant", "content": "I will inspect the file." }),
                json!({
                    "role": "user",
                    "content": format!("{}\nsummary", i18n::t("history.compaction_prefix"))
                }),
            ]
        );
    }

    #[test]
    fn test_build_committed_replacement_history_from_rebuilt_strips_retained_markers() {
        let mut retained_user = json!({ "role": "user", "content": "head question" });
        mark_retained_interaction_message(&mut retained_user);
        let mut retained_assistant = json!({ "role": "assistant", "content": "head answer" });
        mark_retained_interaction_message(&mut retained_assistant);
        let rebuilt = vec![
            json!({ "role": "system", "content": "system prompt" }),
            retained_user,
            retained_assistant,
            json!({
                "role": "user",
                "content": format!("{}\nsummary", i18n::t("history.compaction_prefix"))
            }),
        ];

        let committed = build_committed_replacement_history_from_rebuilt(&rebuilt);

        assert_eq!(committed.len(), 3);
        assert!(committed.iter().all(|item| {
            item.get("meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get(COMPACTION_RETAINED_INTERACTION_META_KEY))
                .is_none()
        }));
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
