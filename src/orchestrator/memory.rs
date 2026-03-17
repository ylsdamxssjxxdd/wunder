use super::*;

const COMPACTION_MIN_CURRENT_USER_MESSAGE_TOKENS: i64 = 64;
const PROMPT_MEMORY_RECALL_LIMIT: usize = 30;

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
        force: bool,
        exclude_current_user: bool,
    ) -> Result<Vec<Value>, OrchestratorError> {
        if is_admin && !force {
            return Ok(messages);
        }
        let context_tokens = estimate_messages_tokens(&messages);
        let Some(limit) = resolve_compaction_limit(llm_config, context_tokens, force) else {
            return Ok(messages);
        };
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
        let (mut should_compact_by_history, mut should_compact) =
            should_compact_by_context(context_tokens, limit, history_threshold);
        if force && !should_compact {
            should_compact = true;
            if !should_compact_by_history {
                should_compact_by_history = true;
            }
        }
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
        let mut compacting_payload = json!({
            "stage": "compacting",
            "summary": summary_text,
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
        let has_candidates = !source_messages.is_empty();
        if user_content.trim().is_empty() && (!force || !has_candidates) {
            let mut guarded_messages = messages.clone();
            let guard_stats = apply_rebuilt_context_guard(&mut guarded_messages, limit);
            if guard_stats.applied {
                let mut guard_payload = json!({
                    "stage": "context_guard",
                    "summary": "Context guard trimmed oversized user input before model call.",
                    "tokens_before": guard_stats.tokens_before,
                    "tokens_after": guard_stats.tokens_after,
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
                "status": if guard_stats.applied { "guard_only" } else { "skipped" },
                "skip_reason": if has_candidates { "no_candidates" } else { "no_history" },
                "fresh_memory_injected": false,
                "fresh_memory_count": 0,
                "history_usage": history_usage,
                "context_tokens": history_usage,
                "history_threshold": history_threshold,
                "limit": limit,
                "total_tokens": total_tokens,
                "reset_mode": reset_mode,
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
            let per_message_limit = summary_limit.clamp(1, COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
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
        if is_empty_compaction_summary(&summary_text) {
            summary_fallback = true;
            let fallback_content = if user_content.trim().is_empty() {
                i18n::t("compaction.summary_fallback")
            } else {
                user_content.clone()
            };
            summary_text = HistoryManager::format_compaction_summary(&fallback_content);
        }
        let (fresh_memory_block, fresh_memory_count) = self
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
            None,
            Some(&meta_value),
            None,
            None,
            None,
        );

        let mut current_user_message_for_history_trimmed = false;
        let current_user_message_for_history = current_user_message.as_ref().map(|message| {
            if let Some(trimmed) = trim_message_to_fit_tokens(message, limit) {
                current_user_message_for_history_trimmed = true;
                trimmed
            } else {
                message.clone()
            }
        });

        if skipped_question {
            let should_reappend = compacted_until_ts.is_none()
                || current_question_ts.is_none()
                || current_question_ts <= compacted_until_ts;
            if should_reappend {
                if let Some(current_user_message) = current_user_message_for_history.as_ref() {
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
        if let Some(current_user_message) = current_user_message_for_history {
            rebuilt.push(current_user_message);
        } else if !question_text.is_empty() {
            rebuilt.push(json!({ "role": "user", "content": question_text }));
        }
        let mut rebuilt = self.shrink_messages_to_limit(rebuilt, limit);
        let guard_stats = apply_rebuilt_context_guard(&mut rebuilt, limit);
        let rebuilt_tokens = estimate_messages_tokens(&rebuilt);

        if guard_stats.applied {
            let mut guard_payload = json!({
                "stage": "context_guard",
                "summary": "Context guard trimmed oversized compaction payload.",
                "tokens_before": guard_stats.tokens_before,
                "tokens_after": guard_stats.tokens_after,
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
            "status": if summary_fallback { "fallback" } else { "done" },
            "summary_fallback": summary_fallback,
            "fresh_memory_injected": fresh_memory_injected,
            "fresh_memory_count": fresh_memory_count,
            "summary_tokens": approx_token_count(&summary_text),
            "total_tokens": total_tokens,
            "total_tokens_after": rebuilt_tokens,
            "history_usage": history_usage,
            "context_tokens": history_usage,
            "context_tokens_after": rebuilt_tokens,
            "history_threshold": history_threshold,
            "limit": limit,
            "reset_mode": reset_mode,
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
    ) -> (String, usize) {
        let fragment_store =
            crate::services::memory_fragments::MemoryFragmentStore::new(self.storage.clone());
        let hits = fragment_store
            .recall_for_prompt(
                Some(config),
                user_id,
                agent_id,
                Some(session_id),
                None,
                query_text,
                Some(PROMPT_MEMORY_RECALL_LIMIT),
            )
            .await;
        let hit_count = hits.len();
        let block = fragment_store.build_prompt_block(&hits);
        (block, hit_count)
    }

    pub(crate) async fn force_compact_session(
        &self,
        user_id: &str,
        session_id: &str,
        is_admin: bool,
        model_name: Option<&str>,
        agent_id: Option<&str>,
        agent_prompt: Option<&str>,
    ) -> Result<(), OrchestratorError> {
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
        let emitter = EventEmitter::new(
            session_id.to_string(),
            user_id.to_string(),
            None,
            None,
            self.monitor.clone(),
            is_admin,
            0,
        );
        let messages = self
            .maybe_compact_messages(
                &config,
                &llm_config,
                user_id,
                agent_id,
                session_id,
                is_admin,
                RoundInfo::default(),
                messages,
                &emitter,
                "",
                log_payload,
                true,
                false,
            )
            .await?;
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
            RoundInfo::default().insert_into(map);
        }
        emitter.emit("context_usage", context_payload).await;
        Ok(())
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
        let hits = fragment_store
            .recall_for_prompt(
                Some(&config),
                user_id,
                agent_id,
                session_id,
                round_id,
                query_text,
                Some(PROMPT_MEMORY_RECALL_LIMIT),
            )
            .await;
        let block = fragment_store.build_prompt_block(&hits);

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
        if let Some(summary_index) = messages
            .iter()
            .position(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        {
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
        let summary_index = messages
            .iter()
            .position(|message| message.get("role").and_then(Value::as_str) == Some("user"));
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
        let summary_index = messages
            .iter()
            .position(|message| message.get("role").and_then(Value::as_str) == Some("user"));
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
            let remaining_for_last = (limit - (total_tokens - last_tokens)).max(1);
            if let Some(trimmed) =
                trim_message_to_fit_tokens(&messages[last_index], remaining_for_last)
            {
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

fn resolve_compaction_limit(
    llm_config: &LlmModelConfig,
    context_tokens: i64,
    force: bool,
) -> Option<i64> {
    let configured_limit =
        HistoryManager::get_auto_compact_limit(llm_config).map(|limit| limit.max(1));
    if let Some(limit) = configured_limit {
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
        let mut messages = vec![
            json!({ "role": "system", "content": "system prompt" }),
            json!({ "role": "user", "content": "S".repeat(48_000) }),
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
        assert!(stats.summary_trimmed || stats.fallback_trim_applied);
        assert!(estimate_messages_tokens(&messages) <= limit);
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
}
