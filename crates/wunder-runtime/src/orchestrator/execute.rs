use super::context::normalize_model_context_message;
use super::retry_governor::RetryGovernor;
use super::tool_calls::ToolCall;
use super::*;
use crate::core::llm_speed::TurnDecodeSpeedAccumulator;
use crate::core::long_task;
use crate::services::chat_attachments::persist_user_chat_attachments;
use crate::services::goal;
use crate::services::orchestration_context::session_orchestration_run_root;
use crate::services::subagents;
use crate::services::tools::sessions_yield_tool;

use super::execute_support::*;

impl Orchestrator {
    pub(super) async fn execute_request(
        &self,
        prepared: PreparedRequest,
        emitter: EventEmitter,
    ) -> Result<WunderResponse, OrchestratorError> {
        let mut heartbeat_task: Option<JoinHandle<()>> = None;
        let mut acquired = false;
        let mut prepared = prepared;
        let request_config = self
            .resolve_config(prepared.config_overrides.as_ref())
            .await;
        let max_active_sessions = if prepared.is_admin {
            i64::MAX as usize
        } else {
            request_config.server.max_active_sessions
        };
        let limiter = RequestLimiter::new(self.storage.clone(), max_active_sessions);
        let session_id = prepared.session_id.clone();
        let user_id = prepared.user_id.clone();
        let question = prepared.question.clone();
        let display_question_override =
            extract_channel_display_question_override(prepared.config_overrides.as_ref());
        let hide_start_question = subagents::config_flag(
            prepared.config_overrides.as_ref(),
            subagents::HIDE_START_QUESTION_CONFIG_KEY,
        );
        // skip_stream_clear is no longer needed: stream events are reclaimed
        // by the TTL cleanup instead of being wiped at the start of each round.
        let hidden_internal_user = subagents::config_flag(
            prepared.config_overrides.as_ref(),
            subagents::HIDDEN_USER_MESSAGE_CONFIG_KEY,
        );
        let skip_auto_memory_extract = subagents::config_flag(
            prepared.config_overrides.as_ref(),
            subagents::SKIP_AUTO_MEMORY_CONFIG_KEY,
        );
        let goal_continuation_turn = goal::is_goal_continuation(prepared.config_overrides.as_ref());
        let display_question = display_question_override
            .clone()
            .unwrap_or_else(|| question.clone());
        let is_admin = prepared.is_admin;
        let mut active_turn_id: Option<String> = None;
        let mut active_turn_round = RoundInfo::default();

        let result = async {
            let mut lock_agent_id = prepared.agent_id.clone().unwrap_or_default();
            if !is_admin {
                let storage = self.storage.clone();
                let lock_user = user_id.clone();
                let lock_session = session_id.clone();
                let lock_session_query = lock_session.clone();
                if let Ok(Some(record)) =
                    crate::core::blocking::run_db("orchestrator.execute.lock_session", move || {
                        storage.get_chat_session(&lock_user, &lock_session_query)
                    })
                    .await
                {
                    if record.parent_session_id.is_some() {
                        lock_agent_id = format!("subagent:{lock_session}");
                    }
                }
            }
            let ok = limiter
                .acquire(&session_id, &user_id, &lock_agent_id, prepared.allow_queue)
                .await
                .map_err(|err| OrchestratorError::internal(err.to_string()))?;
            if !ok {
                return Err(OrchestratorError::user_busy(i18n::t(
                    "error.user_session_busy",
                )));
            }
            acquired = true;

            if let Some(attachments) = prepared.attachments.as_mut() {
                if let Err(err) = persist_user_chat_attachments(
                    self.workspace.as_ref(),
                    &user_id,
                    &session_id,
                    attachments,
                )
                .await
                {
                    warn!(
                        "persist chat attachments failed for user {} session {}: {}",
                        user_id, session_id, err
                    );
                }
            }

            // Stream events are kept across rounds and reclaimed by the TTL
            // cleanup (constants::STREAM_EVENT_TTL_S). Clearing them per round
            // broke the event_id sequence and caused resume gaps on the client.
            // Keep renewing the session lock heartbeat for long-running requests.
            let heartbeat_limiter = limiter.clone();
            if acquired {
                let heartbeat_session = session_id.clone();
                heartbeat_task = Some(long_task::spawn(
                    "orchestrator.execute.session_lock_heartbeat",
                    async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs_f64(
                            SESSION_LOCK_HEARTBEAT_S,
                        ))
                        .await;
                        heartbeat_limiter.touch(&heartbeat_session).await;
                    }
                    },
                ));
            }

            let local_full_event_logs =
                should_enable_local_full_event_logs(&request_config.server.mode);
            let monitor_debug_payload = prepared.debug_payload || local_full_event_logs;
            let user_round = self.monitor.register(
                &session_id,
                &user_id,
                prepared.agent_id.as_deref().unwrap_or(""),
                &display_question,
                is_admin,
                monitor_debug_payload,
            );
            let request_round = RoundInfo::user_only(user_round);
            let active_turn = self.active_turns.begin_turn(&session_id);
            active_turn_id = Some(active_turn.turn_id.clone());
            active_turn_round = request_round;
            self.emit_thread_runtime_update(
                &emitter,
                request_round,
                self.thread_runtime
                    .begin_turn(&session_id, active_turn.turn_id.as_str()),
            )
            .await;
            let mut start_payload = json!({
                "stage": "start",
                "summary": i18n::t("monitor.summary.received"),
            });
            if let Value::Object(ref mut map) = start_payload {
                if !hide_start_question {
                    map.insert("question".to_string(), json!(display_question.clone()));
                }
                if hidden_internal_user {
                    map.insert("user_message".to_string(), json!(question.clone()));
                    map.insert("hidden_internal_user".to_string(), Value::Bool(true));
                }
                request_round.insert_into(map);
            }
            emitter.emit("progress", start_payload).await;

            let config = request_config.clone();
            let log_payload =
                is_debug_log_level(&config.observability.log_level) || monitor_debug_payload;
            let (_llm_name, llm_config) =
                self.resolve_llm_config(&config, prepared.model_name.as_deref())?;
            let skills = if prepared.config_overrides.is_some() {
                Arc::new(RwLock::new(load_skills(&config, true, true, true)))
            } else {
                self.skills.clone()
            };
            let skills_snapshot = skills.read().await.clone();
            let user_tool_bindings =
                self.user_tool_manager
                    .build_bindings(&config, &skills_snapshot, &user_id);
            let private_root = self.inner_visible.private_root(&user_id);
            let mut extra_tool_roots = vec![private_root];
            if let Some(orchestration_run_root) = session_orchestration_run_root(
                self.storage.as_ref(),
                self.workspace.as_ref(),
                &prepared.workspace_id,
                &user_id,
                &session_id,
            ) {
                extra_tool_roots.push(orchestration_run_root);
            }
            let tool_roots = crate::tools::build_tool_roots(
                &config,
                &skills_snapshot,
                Some(&user_tool_bindings),
                &extra_tool_roots,
            );
            let allowed_tool_names = self.filter_tools_for_model_capability(
                self.resolve_allowed_tool_names(
                    &config,
                    prepared.tool_names.as_deref().unwrap_or(&[]),
                    &skills_snapshot,
                    Some(&user_tool_bindings),
                ),
                llm_config.support_vision.unwrap_or(false),
            );
            let allowed_tool_names = self.apply_preview_skill_tool_policy(
                allowed_tool_names,
                prepared.preview_skill,
            );
            let tool_call_mode =
                self.resolve_frozen_session_tool_call_mode(&user_id, &session_id, &llm_config);
            let function_tooling = if uses_native_tool_api(tool_call_mode, &llm_config)
                && !prepared.skip_tool_calls
            {
                self.build_function_tooling(
                    &config,
                    &skills_snapshot,
                    &allowed_tool_names,
                    Some(&user_tool_bindings),
                    tool_call_mode,
                    &user_id,
                    prepared.agent_id.as_deref(),
                    &prepared.workspace_id,
                )
            } else {
                None
            };

            let user_round_id = user_round.to_string();
            let system_prompt = self
                .resolve_session_prompt(
                    &config,
                    prepared.config_overrides.as_ref(),
                    &allowed_tool_names,
                    tool_call_mode,
                    &skills_snapshot,
                    Some(&user_tool_bindings),
                    &user_id,
                    &prepared.workspace_id,
                    &session_id,
                    Some(&prepared.language),
                    prepared.agent_id.as_deref(),
                    is_admin,
                    prepared.agent_prompt.as_deref(),
                    prepared.preview_skill,
                    Some(&question),
                    Some(user_round_id.as_str()),
                )
                .await;

            let history_manager = HistoryManager;
            let context_manager = ContextManager;
            let mut messages = vec![json!({ "role": "system", "content": system_prompt })];
            // Ensure the previous turn's async history/context writes are visible before
            // building this turn's prompt. Otherwise fast follow-up user messages can miss
            // the tail of the append-only model context and break KV cache reuse.
            let _ = self.workspace.flush_writes_async().await;
            let mut model_context_entries = self
                .workspace
                .load_model_context_entries(&user_id, &session_id, 0)
                .unwrap_or_default()
                .into_iter()
                .filter_map(normalize_model_context_message)
                .collect::<Vec<_>>();
            if model_context_entries.is_empty() {
                model_context_entries = history_manager
                    .load_history_messages_async(
                        self.workspace.clone(),
                        user_id.clone(),
                        session_id.clone(),
                        0,
                    )
                    .await
                    .into_iter()
                    .filter_map(normalize_model_context_message)
                    .collect();
                if !model_context_entries.is_empty() {
                    if let Err(err) = self.workspace.replace_model_context_entries(
                        &user_id,
                        &session_id,
                        &model_context_entries,
                    ) {
                        warn!(
                            "replace model context entries failed for session {session_id}: {err}"
                        );
                    }
                    let _ = self.workspace.flush_writes_async().await;
                }
            }
            messages.extend(model_context_entries);
            let context_messages_before_normalize = messages.clone();
            messages = context_manager.normalize_messages(messages);
            if messages != context_messages_before_normalize {
                let repaired_model_context_entries =
                    super::context::model_context_entries_from_messages(&messages);
                if let Err(err) = self.workspace.replace_model_context_entries(
                    &user_id,
                    &session_id,
                    &repaired_model_context_entries,
                ) {
                    warn!(
                        "replace repaired model context entries failed for session {session_id}: {err}"
                    );
                } else {
                    let _ = self.workspace.flush_writes_async().await;
                }
            }
            let user_message = self
                .build_user_message(&question, prepared.attachments.as_deref())
                .await;
            messages.push(user_message.clone());
            let persisted_user_model_message = user_message.clone();
            messages = context_manager.normalize_messages(messages);
            let mut user_message_appended = false;
            let mut user_context_appended = false;

            let desktop_unlimited_rounds =
                config.server.mode.trim().eq_ignore_ascii_case("desktop");
            let max_rounds = if is_admin || desktop_unlimited_rounds {
                None
            } else {
                Some(resolve_non_admin_max_rounds(
                    &llm_config,
                    prepared.skip_tool_calls,
                ))
            };
            let mut reached_max_rounds = false;
            let goal_turn_started_at = Instant::now();
            let mut round_usage = TokenUsage {
                input: 0,
                output: 0,
                total: 0,
            };
            let mut last_model_usage: Option<TokenUsage> = None;
            let mut confirmed_context_occupancy_tokens: Option<i64> = None;
            let mut turn_decode_speed = TurnDecodeSpeedAccumulator::default();
            let mut answer = String::new();
            let mut stop_reason: Option<String> = None;
            let mut stop_meta: Option<Value> = None;
            let mut a2ui_uid: Option<String> = None;
            let mut a2ui_messages: Option<Value> = None;
            let mut last_response: Option<(String, String)> = None;
            let mut last_round_info = request_round;

            let mut model_round = 0_i64;
            let repeated_tool_failure_threshold =
                resolve_tool_failure_guard_threshold(&request_config);
            let mut retry_governor = RetryGovernor::new(repeated_tool_failure_threshold);
            let mut reroute_notice_count = 0_u32;
            let mut reroute_notice_fingerprints: HashSet<String> = HashSet::new();
            let mut invalid_tool_call_reroute_count = 0_u32;
            let mut empty_final_answer_reroute_count = 0_u32;
            let memory_manager_tool_name = resolve_tool_name("memory_manager");
            let tool_budget_limits = ToolBudgetLimits {
                total: DEFAULT_TOOL_CALL_BUDGET_PER_TURN,
                db_query: resolve_db_query_tool_budget(&question),
                memory_recall: DEFAULT_MEMORY_RECALL_BUDGET_PER_TURN,
            };
            let mut tool_budget_usage = ToolBudgetUsage::default();
            let mut memory_recall_cache: HashMap<String, CachedRecallResult> = HashMap::new();
            let mut memory_recall_revision: u64 = 0;
            let tools_payload = function_tooling
                .as_ref()
                .map(|tooling| tooling.tools.as_slice());
            // Context occupancy is authoritative only after the model provider reports usage.
            // Local token estimates are deliberately excluded from compaction decisions.
            let request_overhead_tokens = 0_i64;
            // If the previous turn died on context overflow, force a repair compaction before
            // sampling again so the session can self-heal instead of requiring a new thread.
            let mut force_compaction_on_entry = self
                .workspace
                .load_session_context_overflow_async(&user_id, &session_id)
                .await;
            let mut persisted_context_tokens = self
                .workspace
                .load_session_context_tokens_async(&user_id, &session_id)
                .await;
            let mut context_window_limit_hint = self
                .workspace
                .load_session_context_limit_hint_async(&user_id, &session_id)
                .await;
            loop {
                if let Some(max_rounds) = max_rounds {
                    if model_round >= max_rounds {
                        reached_max_rounds = true;
                        break;
                    }
                }
                model_round += 1;
                let round_info = RoundInfo::new(user_round, model_round);
                last_round_info = round_info;
                let mut adaptive_recovery_limit_hint: Option<i64> = None;
                self.ensure_not_cancelled(&session_id)?;
                let compaction_llm_config = apply_context_window_limit_hint(
                    &llm_config,
                    merge_context_window_limit_hint(
                        context_window_limit_hint,
                        adaptive_recovery_limit_hint,
                    ),
                );
                let compaction_result = self
                    .maybe_compact_messages(
                        &config,
                        &compaction_llm_config,
                        &user_id,
                        prepared.agent_id.as_deref(),
                        &session_id,
                        is_admin,
                        round_info,
                        messages,
                        &emitter,
                        &question,
                        log_payload,
                        persisted_context_tokens,
                        request_overhead_tokens,
                        force_compaction_on_entry,
                        true,
                        super::memory::CompactionRunMode::AutoLoop,
                    )
                    .await?;
                messages = compaction_result.messages;
                if compaction_result.compaction_id.is_some() {
                    user_context_appended = true;
                }
                if compaction_result.compaction_id.is_some() {
                    persisted_context_tokens = 0;
                    self.workspace
                        .save_session_context_tokens_async(&user_id, &session_id, 0)
                        .await;
                }
                if force_compaction_on_entry {
                    let _ = self
                        .workspace
                        .delete_session_context_overflow_async(&user_id, &session_id)
                        .await;
                    force_compaction_on_entry = false;
                }
                self.ensure_not_cancelled(&session_id)?;
                messages = context_manager.normalize_messages(messages);
                let context_tokens = persisted_context_tokens.max(0);
                let projected_request_tokens = context_tokens;
                let mut context_payload = json!({
                    "context_tokens": context_tokens,
                    "persisted_context_tokens": persisted_context_tokens,
                    "observed_context_tokens": context_tokens,
                    "projected_request_tokens": projected_request_tokens,
                    "request_overhead_tokens": request_overhead_tokens,
                    "context_usage_source": if context_tokens > 0 { "provider_observed" } else { "unobserved" },
                    "message_count": messages.len(),
                });
                if let Value::Object(ref mut map) = context_payload {
                    if let Some(max_context) = merge_context_window_limit_hint(
                        llm_config.max_context.map(i64::from),
                        context_window_limit_hint,
                    ) {
                        map.insert("max_context".to_string(), json!(max_context));
                    }
                    round_info.insert_into(map);
                }
                emitter.emit("context_usage", context_payload).await;

                let mut llm_call_payload = json!({
                    "stage": "llm_call",
                    "summary": i18n::t("monitor.summary.model_call"),
                });
                if let Value::Object(ref mut map) = llm_call_payload {
                    round_info.insert_into(map);
                }
                emitter.emit("progress", llm_call_payload).await;

                if !user_message_appended {
                    let user_content = if let Some(display_override) = display_question_override.as_ref() {
                        Some(Value::String(display_override.clone()))
                    } else {
                        resolve_user_content_for_persist(&messages, &user_message)
                    };
                    let hidden_user_meta = if hidden_internal_user {
                        Some(subagents::build_hidden_user_meta())
                    } else {
                        None
                    };
                    if user_content.is_some() {
                        self.append_chat(
                            &user_id,
                            &session_id,
                            "user",
                            user_content.as_ref(),
                            prepared.attachments.as_deref(),
                            hidden_user_meta.as_ref(),
                            None,
                            None,
                            None,
                            request_round,
                        );
                    }
                    user_message_appended = true;
                }
                if !user_context_appended {
                    self.append_model_context_entry(
                        &user_id,
                        &session_id,
                        &persisted_user_model_message,
                    );
                    user_context_appended = true;
                }

                let mut overflow_recovery_attempts = 0_u32;
                let (content, reasoning, usage, tool_calls_payload, round_speed) = loop {
                    match self
                        .call_llm(
                            &llm_config,
                            &messages,
                            &user_id,
                            is_admin,
                            &emitter,
                            &session_id,
                            prepared.stream,
                            round_info,
                            true,
                            true,
                            log_payload,
                            tools_payload,
                            None,
                        )
                        .await
                    {
                        Ok(response) => break response,
                        Err(err)
                            if should_recover_from_context_overflow(&err)
                                && overflow_recovery_attempts
                                    < MAX_CONTEXT_OVERFLOW_RECOVERY_ATTEMPTS =>
                        {
                            overflow_recovery_attempts += 1;
                            let parsed_limit_hint =
                                super::llm::extract_context_window_limit_hint(err.message());
                            if let Some(limit_hint) = parsed_limit_hint {
                                let merged_limit_hint = merge_context_window_limit_hint(
                                    context_window_limit_hint,
                                    Some(limit_hint),
                                );
                                if merged_limit_hint != context_window_limit_hint {
                                    context_window_limit_hint = merged_limit_hint;
                                    self.workspace
                                        .save_session_context_limit_hint_async(
                                            &user_id,
                                            &session_id,
                                            context_window_limit_hint,
                                        )
                                        .await;
                                }
                            } else {
                                let overflow_projected_tokens = persisted_context_tokens.max(0);
                                let fallback_limit_hint =
                                    derive_recovery_context_window_limit_hint(
                                        overflow_projected_tokens,
                                        overflow_recovery_attempts,
                                    );
                                adaptive_recovery_limit_hint = merge_context_window_limit_hint(
                                    adaptive_recovery_limit_hint,
                                    Some(fallback_limit_hint),
                                );
                            }
                            let mut recovery_payload = json!({
                                "stage": "context_overflow_recovery",
                                "summary": i18n::t("compaction.reason.context_too_long"),
                                "attempt": overflow_recovery_attempts,
                                "max_attempts": MAX_CONTEXT_OVERFLOW_RECOVERY_ATTEMPTS,
                            });
                            if let Value::Object(ref mut map) = recovery_payload {
                                round_info.insert_into(map);
                            }
                            emitter.emit("progress", recovery_payload).await;

                            let compaction_llm_config = apply_context_window_limit_hint(
                                &llm_config,
                                merge_context_window_limit_hint(
                                    context_window_limit_hint,
                                    adaptive_recovery_limit_hint,
                                ),
                            );
                            let compaction_result = self
                                .maybe_compact_messages(
                                    &config,
                                    &compaction_llm_config,
                                    &user_id,
                                    prepared.agent_id.as_deref(),
                                    &session_id,
                                    is_admin,
                                    round_info,
                                    messages,
                                    &emitter,
                                    &question,
                                    log_payload,
                                    persisted_context_tokens,
                                    request_overhead_tokens,
                                    true,
                                    true,
                                    super::memory::CompactionRunMode::OverflowRecovery,
                                )
                                .await?;
                            let overflow_recovery_compaction_id =
                                compaction_result.compaction_id.clone();
                            messages = compaction_result.messages;
                            let _ = self
                                .workspace
                                .delete_session_context_overflow_async(&user_id, &session_id)
                                .await;
                            messages = context_manager.normalize_messages(messages);
                            let recovered_tokens = 0_i64;
                            let recovered_request_tokens = recovered_tokens;
                            self.workspace
                                .save_session_context_tokens_async(
                                    &user_id,
                                    &session_id,
                                    recovered_tokens,
                                )
                                .await;
                            persisted_context_tokens = recovered_tokens;
                            let mut compaction_payload = json!({
                                "reason": "overflow_recovery",
                                "status": "done",
                                "attempt": overflow_recovery_attempts,
                                "max_attempts": MAX_CONTEXT_OVERFLOW_RECOVERY_ATTEMPTS,
                                "context_tokens_after": recovered_tokens,
                                "observed_context_tokens_after": recovered_tokens,
                                "projected_request_tokens_after": recovered_request_tokens,
                                "request_overhead_tokens": request_overhead_tokens,
                                "context_usage_source": "unobserved_after_compaction",
                            });
                            if let Value::Object(ref mut map) = compaction_payload {
                                if let Some(max_context) = compaction_llm_config
                                    .max_context
                                    .map(i64::from)
                                    .filter(|value| *value > 0)
                                {
                                    map.insert("max_context".to_string(), json!(max_context));
                                }
                                super::memory::insert_compaction_id(
                                    map,
                                    overflow_recovery_compaction_id.as_deref(),
                                );
                                round_info.insert_into(map);
                            }
                            emitter.emit("compaction", compaction_payload).await;
                        }
                        Err(err) => {
                            if should_recover_from_context_overflow(&err) {
                                if let Some(limit_hint) =
                                    super::llm::extract_context_window_limit_hint(err.message())
                                {
                                    let merged_limit_hint = merge_context_window_limit_hint(
                                        context_window_limit_hint,
                                        Some(limit_hint),
                                    );
                                    if merged_limit_hint != context_window_limit_hint {
                                        context_window_limit_hint = merged_limit_hint;
                                        self.workspace
                                            .save_session_context_limit_hint_async(
                                                &user_id,
                                                &session_id,
                                                context_window_limit_hint,
                                            )
                                            .await;
                                    }
                                }
                                self.workspace
                                    .save_session_context_overflow_async(
                                        &user_id,
                                        &session_id,
                                        true,
                                    )
                                    .await;
                                self.workspace
                                    .save_session_context_tokens_async(
                                        &user_id,
                                        &session_id,
                                        0,
                                    )
                                    .await;
                            }
                            if err.code() == "CANCELLED" {
                                append_cancelled_generation_context_marker(
                                    self,
                                    &user_id,
                                    &session_id,
                                    &messages,
                                    round_info,
                                );
                            }
                            return Err(err);
                        }
                    }
                };
                last_response = Some((content.clone(), reasoning.clone()));
                turn_decode_speed.record_summary(&round_speed);
                update_round_usage_authority(&mut round_usage, &usage);
                last_model_usage = Some(usage.clone());
                if let Some(context_tokens) = resolve_usage_context_occupancy_tokens(&usage) {
                    confirmed_context_occupancy_tokens = Some(context_tokens);
                    persisted_context_tokens = context_tokens;
                    self.workspace
                        .save_session_context_tokens_async(&user_id, &session_id, context_tokens)
                        .await;
                }
                let tool_calls = if prepared.skip_tool_calls {
                    Vec::new()
                } else {
                    collect_tool_calls_from_output(
                        &content,
                        &reasoning,
                        tool_calls_payload.as_ref(),
                        tool_call_mode,
                    )
                };
                let tool_calls = if let Some(tooling) = function_tooling.as_ref() {
                    apply_tool_name_map(tool_calls, &tooling.name_map)
                } else {
                    apply_tool_name_map(
                        tool_calls,
                        &crate::orchestrator::prompt::build_prompt_tool_name_map(
                            &config,
                            &allowed_tool_names,
                        ),
                    )
                };
                let planning_result = build_planned_tool_calls(tool_calls, &allowed_tool_names);
                let planned_calls = planning_result.planned;
                if planned_calls.is_empty()
                    && !planning_result.rejected.is_empty()
                    && !prepared.skip_tool_calls
                    && invalid_tool_call_reroute_count < INVALID_TOOL_CALL_REROUTE_MAX_PER_TURN
                {
                    invalid_tool_call_reroute_count =
                        invalid_tool_call_reroute_count.saturating_add(1);
                    let model_notice = build_invalid_tool_call_model_notice(
                        &planning_result.rejected,
                        &allowed_tool_names,
                    );
                    let mut reroute_payload = json!({
                        "stage": "invalid_tool_call_reroute",
                        "summary": "Model returned tool calls that could not be executed; model instructed to repair the call or answer directly.",
                        "attempt": invalid_tool_call_reroute_count,
                        "max_attempts": INVALID_TOOL_CALL_REROUTE_MAX_PER_TURN,
                        "rejected_tool_calls": rejected_tool_calls_event_payload(
                            &planning_result.rejected
                        ),
                    });
                    if let Value::Object(ref mut map) = reroute_payload {
                        round_info.insert_into(map);
                    }
                    emitter.emit("progress", reroute_payload).await;
                    let model_notice_message = json!({
                        "role": "user",
                        "content": encode_observation_prefixed_json(&model_notice),
                    });
                    messages.push(model_notice_message.clone());
                    self.append_model_context_entry(
                        &user_id,
                        &session_id,
                        &model_notice_message,
                    );
                    continue;
                }
                if planned_calls.is_empty() {
                    if prepared.skip_tool_calls {
                        answer = content.trim().to_string();
                    } else {
                        answer = self.resolve_final_answer(&content);
                    }
                    if answer.trim().is_empty() {
                        if empty_final_answer_reroute_count
                            < EMPTY_FINAL_ANSWER_REROUTE_MAX_PER_TURN
                        {
                            empty_final_answer_reroute_count =
                                empty_final_answer_reroute_count.saturating_add(1);
                            let model_notice = build_empty_final_answer_model_notice(
                                empty_final_answer_reroute_count,
                                EMPTY_FINAL_ANSWER_REROUTE_MAX_PER_TURN,
                                !content.trim().is_empty(),
                                !reasoning.trim().is_empty(),
                                tool_calls_payload.is_some(),
                                !prepared.skip_tool_calls,
                            );
                            let mut reroute_payload = json!({
                                "stage": "empty_final_answer_reroute",
                                "summary": "Model returned no usable final content; model instructed to continue instead of ending the turn.",
                                "attempt": empty_final_answer_reroute_count,
                                "max_attempts": EMPTY_FINAL_ANSWER_REROUTE_MAX_PER_TURN,
                            });
                            if let Value::Object(ref mut map) = reroute_payload {
                                round_info.insert_into(map);
                            }
                            emitter.emit("progress", reroute_payload).await;
                            let model_notice_message = json!({
                                "role": "user",
                                "content": encode_observation_prefixed_json(&model_notice),
                            });
                            messages.push(model_notice_message.clone());
                            self.append_model_context_entry(
                                &user_id,
                                &session_id,
                                &model_notice_message,
                            );
                            continue;
                        }
                        return Err(OrchestratorError::llm_unavailable(
                            build_empty_final_answer_retry_exhausted_error(
                                EMPTY_FINAL_ANSWER_REROUTE_MAX_PER_TURN,
                            ),
                        ));
                    }
                    if !answer.trim().is_empty() {
                        answer = self.reconcile_final_answer_workspace_images(
                            &prepared.workspace_id,
                            &session_id,
                            &answer,
                        );
                    }
                    stop_reason = Some("model_response".to_string());
                    let assistant_content = if answer.is_empty() {
                        content.clone()
                    } else {
                        answer.clone()
                    };
                    let mut assistant_model_message = json!({
                        "role": "assistant",
                        "content": content.clone(),
                    });
                    if !reasoning.trim().is_empty() {
                        assistant_model_message["reasoning_content"] = json!(reasoning.clone());
                    }
                    self.append_model_context_entry(
                        &user_id,
                        &session_id,
                        &assistant_model_message,
                    );
                    if !assistant_content.trim().is_empty() {
                        self.append_chat(
                            &user_id,
                            &session_id,
                            "assistant",
                            Some(&json!(assistant_content)),
                            None,
                            None,
                            Some(&reasoning),
                            None,
                            None,
                            round_info,
                        );
                    }
                    if answer.is_empty() {
                        answer = content.trim().to_string();
                    }
                    break;
                }

                let assistant_content = content.clone();
                let assistant_reasoning = reasoning.clone();
                let assistant_model_tool_calls =
                    build_model_context_tool_calls_snapshot(tool_calls_payload.as_ref(), &allowed_tool_names);
                let has_model_tool_calls_payload = assistant_model_tool_calls
                    .as_ref()
                    .is_some_and(|payload| !matches!(payload, Value::Null));
                if has_model_tool_calls_payload
                    || !assistant_content.trim().is_empty()
                    || !assistant_reasoning.trim().is_empty()
                {
                    let mut assistant_model_message = json!({
                        "role": "assistant",
                        "content": assistant_content.clone(),
                    });
                    if !assistant_reasoning.trim().is_empty() {
                        assistant_model_message["reasoning_content"] =
                            json!(assistant_reasoning.clone());
                    }
                    if let Some(tool_calls_payload) = assistant_model_tool_calls {
                        assistant_model_message["tool_calls"] = tool_calls_payload;
                    }
                    self.append_model_context_entry(
                        &user_id,
                        &session_id,
                        &assistant_model_message,
                    );
                }
                let assistant_history =
                    build_assistant_history_snapshot(tool_calls_payload.as_ref(), &allowed_tool_names);
                let has_tool_calls_payload = assistant_history
                    .tool_calls
                    .as_ref()
                    .is_some_and(|payload| !matches!(payload, Value::Null));
                let should_push_assistant_message = has_tool_calls_payload
                    || !assistant_content.trim().is_empty()
                    || !assistant_reasoning.trim().is_empty();
                if should_push_assistant_message {
                    let mut assistant_message = json!({
                        "role": "assistant",
                        "content": assistant_content.clone(),
                    });
                    if !assistant_reasoning.trim().is_empty() {
                        assistant_message["reasoning_content"] = json!(assistant_reasoning.clone());
                    }
                    if let Some(tool_calls_payload) = assistant_history.tool_calls.clone() {
                        assistant_message["tool_calls"] = tool_calls_payload;
                    }
                    messages.push(assistant_message);
                }
                if has_tool_calls_payload {
                    self.append_chat(
                        &user_id,
                        &session_id,
                        "assistant",
                        Some(&json!(assistant_content)),
                        None,
                        Some(&json!({ "type": "tool_call" })),
                        Some(&assistant_reasoning),
                        assistant_history.persisted_tool_calls.as_ref(),
                        None,
                        round_info,
                    );
                }

                let tool_event_emitter = ToolEventEmitter::new(
                    {
                        let emitter = emitter.clone();
                        move |event_type, mut data| {
                            let emitter = emitter.clone();
                            let event_name = event_type.to_string();
                            if let Value::Object(ref mut map) = data {
                                round_info.insert_into(map);
                            }
                            long_task::spawn(
                                "orchestrator.execute.tool_event_emit",
                                async move {
                                emitter.emit(&event_name, data).await;
                                },
                            );
                        }
                    },
                    prepared.stream,
                );

                let tool_context = ToolContext {
                    user_id: &user_id,
                    session_id: &session_id,
                    workspace_id: &prepared.workspace_id,
                    agent_id: prepared.agent_id.as_deref(),
                    user_round: round_info.user_round,
                    model_round: round_info.model_round,
                    is_admin,
                    storage: self.storage.clone(),
                    orchestrator: Some(Arc::new(self.clone())),
                    monitor: Some(self.monitor.clone()),
                    beeroom_realtime: Some(self.beeroom_realtime.clone()),
                    workspace: self.workspace.clone(),
                    lsp_manager: self.lsp_manager.clone(),
                    config: &config,
                    a2a_store: &self.a2a_store,
                    skills: &skills_snapshot,
                    gateway: Some(self.gateway.clone()),
                    user_world: Some(self.user_world.clone()),
                    cron_wake_signal: self.cron_wake_signal.clone(),
                    user_tool_manager: Some(self.user_tool_manager.clone()),
                    user_tool_bindings: Some(&user_tool_bindings),
                    user_tool_store: Some(self.user_tool_manager.store()),
                    request_config_overrides: prepared.config_overrides.as_ref(),
                    allow_roots: Some(tool_roots.allow_roots.clone()),
                    read_roots: Some(tool_roots.read_roots.clone()),
                    command_sessions: Some(self.command_sessions.clone()),
                    event_emitter: Some(tool_event_emitter.clone()),
                    http: &self.http,
                };

                let final_tool_name = resolve_tool_name("final_response");
                let question_panel_name = resolve_tool_name("question_panel");
                let read_tool_name = resolve_tool_name("read_file");

                let mut exec_calls: Vec<PlannedToolCall> = Vec::new();
                let mut terminal_call: Option<(TerminalTool, PlannedToolCall)> = None;
                let mut stop_after_index: Option<usize> = None;
                for planned in planned_calls {
                    if planned.name == "a2ui" {
                        terminal_call = Some((TerminalTool::A2ui, planned));
                        break;
                    }
                    if planned.name == final_tool_name {
                        terminal_call = Some((TerminalTool::Final, planned));
                        break;
                    }
                    if planned.name == question_panel_name {
                        exec_calls.push(planned);
                        stop_after_index = Some(exec_calls.len().saturating_sub(1));
                        break;
                    }
                    exec_calls.push(planned);
                }

                let mut budget_blocked: Option<ToolBudgetBlock> = None;
                let mut preview_usage = tool_budget_usage;
                let mut budgeted_exec_calls: Vec<PlannedToolCall> = Vec::new();
                for planned in exec_calls {
                    let projected_total = preview_usage.total.saturating_add(1);
                    if projected_total > tool_budget_limits.total {
                        budget_blocked = Some(ToolBudgetBlock {
                            kind: ToolBudgetBlockKind::Total,
                            limit: tool_budget_limits.total,
                            attempted: projected_total,
                            tool: planned.name.clone(),
                        });
                        break;
                    }
                    preview_usage.total = projected_total;

                    if is_db_query_tool_name(&planned.name) {
                        let projected_db = preview_usage.db_query.saturating_add(1);
                        if projected_db > tool_budget_limits.db_query {
                            budget_blocked = Some(ToolBudgetBlock {
                                kind: ToolBudgetBlockKind::DbQuery,
                                limit: tool_budget_limits.db_query,
                                attempted: projected_db,
                                tool: planned.name.clone(),
                            });
                            break;
                        }
                        preview_usage.db_query = projected_db;
                    }

                    if is_memory_recall_tool_call(
                        &planned.name,
                        &planned.call.arguments,
                        &memory_manager_tool_name,
                    ) {
                        let projected_recall = preview_usage.memory_recall.saturating_add(1);
                        if projected_recall > tool_budget_limits.memory_recall {
                            budget_blocked = Some(ToolBudgetBlock {
                                kind: ToolBudgetBlockKind::MemoryRecall,
                                limit: tool_budget_limits.memory_recall,
                                attempted: projected_recall,
                                tool: planned.name.clone(),
                            });
                            break;
                        }
                        preview_usage.memory_recall = projected_recall;
                    }

                    budgeted_exec_calls.push(planned);
                }
                let mut exec_calls = budgeted_exec_calls;
                for planned in &mut exec_calls {
                    let missing_call_id = planned
                        .call
                        .id
                        .as_deref()
                        .map(str::trim)
                        .map(|value| value.is_empty())
                        .unwrap_or(true);
                    if missing_call_id {
                        planned.call.id =
                            Some(format!("call_{}", Uuid::new_v4().simple()));
                    }
                }

                self.ensure_not_cancelled(&session_id)?;

                for planned in &exec_calls {
                    let args = &planned.call.arguments;
                    let tool_call_id = planned
                        .call
                        .id
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    let tool_display_name =
                        crate::tools::resolve_runtime_tool_display_name(&config, &planned.name);
                    let safe_args = if args.is_object() {
                        args.clone()
                    } else {
                        json!({ "raw": args })
                    };
                    let recovered_args =
                        crate::core::tool_args::recover_tool_args_value_with_meta(&safe_args);
                    let event_args = if allowed_tool_names.contains(&planned.name) {
                        recovered_args.value.clone()
                    } else {
                        safe_args
                    };
                    let mut tool_payload = json!({ "tool": planned.name, "args": event_args });
                    if let Value::Object(ref mut map) = tool_payload {
                        map.insert(
                            "tool_runtime_name".to_string(),
                            Value::String(planned.name.clone()),
                        );
                        map.insert(
                            "tool_display_name".to_string(),
                            Value::String(tool_display_name),
                        );
                        map.insert(
                            "tool_function_name".to_string(),
                            Value::String(planned.function_name.clone()),
                        );
                        if let Some(repair) = recovered_args.repair.clone() {
                            map.insert("repair".to_string(), repair);
                        }
                        if let Some(tool_call_id) = tool_call_id {
                            map.insert(
                                "tool_call_id".to_string(),
                                Value::String(tool_call_id.to_string()),
                            );
                        }
                        round_info.insert_into(map);
                    }
                    emitter.emit("tool_call", tool_payload).await;
                }

                let mut should_finish = false;
                let mut failure_reroute_notice: Option<Value> = None;
                if !exec_calls.is_empty() {
                    let mut cached_recall_outcomes = Vec::new();
                    let mut executable_calls = Vec::new();
                    for planned in exec_calls.drain(..) {
                        if let Some(cached) = resolve_cached_memory_recall_result(
                            &planned,
                            &memory_manager_tool_name,
                            &memory_recall_cache,
                            memory_recall_revision,
                        ) {
                            let mut result = cached.to_payload();
                            result.insert_meta("recall_cache_hit", Value::Bool(true));
                            cached_recall_outcomes.push(ToolExecutionOutcome {
                                call: planned.call,
                                name: planned.name,
                                result,
                            });
                        } else {
                            executable_calls.push(planned);
                        }
                    }

                    let mut outcomes = if executable_calls.is_empty() {
                        Vec::new()
                    } else {
                        self.execute_tool_calls_parallel(
                            executable_calls,
                            &tool_context,
                            &allowed_tool_names,
                            &session_id,
                            active_turn.turn_id.as_str(),
                            &emitter,
                            prepared.approval_tx.clone(),
                            round_info,
                        )
                        .await?
                    };
                    outcomes.extend(cached_recall_outcomes);
                    for (index, outcome) in outcomes.into_iter().enumerate() {
                        let ToolExecutionOutcome {
                            call,
                            name,
                            mut result,
                        } = outcome;
                        let ToolCall {
                            id,
                            arguments,
                            function_name,
                            ..
                        } = call;
                        let args = arguments;
                        let tool_function_name = function_name
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .unwrap_or(name.as_str())
                            .to_string();
                        let tool_display_name =
                            crate::tools::resolve_runtime_tool_display_name(&config, &name);

                        tool_budget_usage.total = tool_budget_usage.total.saturating_add(1);
                        if is_db_query_tool_name(&name) {
                            tool_budget_usage.db_query = tool_budget_usage.db_query.saturating_add(1);
                        }
                        if is_memory_recall_tool_call(&name, &args, &memory_manager_tool_name) {
                            tool_budget_usage.memory_recall =
                                tool_budget_usage.memory_recall.saturating_add(1);
                        }

                        if is_memory_manager_tool_name(&name, &memory_manager_tool_name) {
                            if let Some(action) = extract_memory_manager_action(&args) {
                                if is_memory_write_action(action.as_str()) && result.ok {
                                    memory_recall_revision =
                                        memory_recall_revision.saturating_add(1);
                                    memory_recall_cache.clear();
                                } else if is_memory_recall_action(action.as_str()) && result.ok {
                                    if let Some(query_key) = normalize_memory_recall_query(
                                        extract_memory_manager_query(&args).as_deref(),
                                    ) {
                                        memory_recall_cache.insert(
                                            query_key,
                                            CachedRecallResult {
                                                revision: memory_recall_revision,
                                                result: CachedToolResult::from_payload(&result),
                                            },
                                        );
                                    }
                                }
                            }
                        }

                        let question_panel_finished = name == question_panel_name && result.ok;
                        if question_panel_finished {
                            answer = i18n::t("response.question_panel_waiting");
                            stop_reason = Some("question_panel".to_string());
                            should_finish = true;
                        }
                        let turn_yield_message = if result.ok {
                            sessions_yield_tool::extract_turn_yield_message(
                                result.meta.as_ref(),
                                &result.data,
                            )
                        } else {
                            None
                        };
                        if let Some(message) = turn_yield_message.as_ref() {
                            answer = message.clone();
                            stop_reason = Some("yield".to_string());
                            stop_meta = Some(
                                sessions_yield_tool::build_turn_yield_stop_meta(message.as_str()),
                            );
                            should_finish = true;
                        }

                        let observation = self.build_tool_observation(&name, &result);
                        let observation_value = Value::String(observation.clone());
                        let read_image_followup = if result.ok && is_read_image_tool_name(&name) {
                            match build_read_image_followup_user_message(&tool_context, &result.data)
                                .await
                            {
                                Ok(payload) => payload,
                                Err(err) => {
                                    warn!(
                                        "failed to prepare read-image followup for session {session_id}: {err}"
                                    );
                                    None
                                }
                            }
                        } else {
                            None
                        };
                        let desktop_followup = if result.ok && is_desktop_control_tool_name(&name) {
                            match build_desktop_followup_user_message(&result.data).await {
                                Ok(payload) => payload,
                                Err(err) => {
                                    warn!(
                                        "failed to prepare desktop followup for session {session_id}: {err}"
                                    );
                                    None
                                }
                            }
                        } else {
                            None
                        };
                        let event_tool_call_id = id
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(ToString::to_string);
                        let history_tool_call_id = if uses_native_tool_api(tool_call_mode, &llm_config) {
                            event_tool_call_id.clone()
                        } else {
                            None
                        };
                        let observation_model_message = if uses_native_tool_api(tool_call_mode, &llm_config) {
                            if let Some(tool_call_id) = history_tool_call_id.as_ref() {
                                json!({
                                    "role": "tool",
                                    "tool_call_id": tool_call_id,
                                    "content": observation.clone(),
                                })
                            } else {
                                json!({
                                    "role": "user",
                                    "content": format!("{OBSERVATION_PREFIX}{observation}"),
                                })
                            }
                        } else {
                            json!({
                                "role": "user",
                                "content": format!("{OBSERVATION_PREFIX}{observation}"),
                            })
                        };
                        messages.push(observation_model_message.clone());
                        self.append_model_context_entry(
                            &user_id,
                            &session_id,
                            &observation_model_message,
                        );
                        if let Some(followup_message) = read_image_followup {
                            let mut followup_message = followup_message;
                            self.mark_internal_model_context_message(
                                &mut followup_message,
                                "read_image_followup",
                                round_info,
                            );
                            self.append_model_context_entry(
                                &user_id,
                                &session_id,
                                &followup_message,
                            );
                            self.append_internal_model_context_chat(
                                &user_id,
                                &session_id,
                                &followup_message,
                                "read_image_followup",
                                round_info,
                            );
                            messages.push(followup_message);
                        }
                        if let Some(followup_message) = desktop_followup {
                            let mut followup_message = followup_message;
                            self.mark_internal_model_context_message(
                                &mut followup_message,
                                "desktop_followup",
                                round_info,
                            );
                            self.append_model_context_entry(
                                &user_id,
                                &session_id,
                                &followup_message,
                            );
                            self.append_internal_model_context_chat(
                                &user_id,
                                &session_id,
                                &followup_message,
                                "desktop_followup",
                                round_info,
                            );
                            messages.push(followup_message);
                            crate::services::chat_payload_sanitizer::prune_previous_inline_image_followups(
                                &mut messages,
                                "desktop_followup",
                            );
                        }
                        self.append_chat(
                            &user_id,
                            &session_id,
                            "tool",
                            Some(&json!(observation)),
                            None,
                            None,
                            None,
                            None,
                            history_tool_call_id.as_deref(),
                            round_info,
                        );

                        self.append_tool_log(
                            &user_id,
                            &session_id,
                            &name,
                            &args,
                            &result,
                            log_payload,
                        );
                        self.append_artifact_logs(&user_id, &session_id, &name, &args, &result);
                        if name == read_tool_name {
                            self.append_skill_usage_logs(
                                &user_id,
                                &session_id,
                                &args,
                                &skills_snapshot,
                                Some(&user_tool_bindings),
                                log_payload,
                            );
                        }

                        let mut tool_result_payload = result.to_event_payload(&name);
                        if let Value::Object(ref mut map) = tool_result_payload {
                            map.insert(
                                "tool_runtime_name".to_string(),
                                Value::String(name.clone()),
                            );
                            map.insert(
                                "tool_display_name".to_string(),
                                Value::String(tool_display_name),
                            );
                            map.insert(
                                "tool_function_name".to_string(),
                                Value::String(tool_function_name),
                            );
                            if let Some(tool_call_id) = event_tool_call_id.as_ref() {
                                map.insert(
                                    "tool_call_id".to_string(),
                                    Value::String(tool_call_id.clone()),
                                );
                            }
                            map.insert(
                                "model_observation".to_string(),
                                observation_value.clone(),
                            );
                            map.remove("trace_id");
                            map.remove("user_round");
                            map.remove("model_round");
                        }
                        emitter.emit("tool_result", tool_result_payload).await;
                        if let Some(tree_version) = result
                            .meta
                            .as_ref()
                            .and_then(|meta| meta.get("workspace_version"))
                            .and_then(|value| {
                                value.as_u64().or_else(|| {
                                    value
                                        .as_i64()
                                        .and_then(|val| if val >= 0 { Some(val as u64) } else { None })
                                })
                            })
                        {
                            let agent_id = prepared
                                .agent_id
                                .clone()
                                .unwrap_or_default()
                                .trim()
                                .to_string();
                            let changed_paths = extract_workspace_changed_paths(
                                result.meta.as_ref(),
                                &result.data,
                                &args,
                                &prepared.workspace_id,
                            );
                            let mut workspace_payload = json!({
                                "workspace_id": prepared.workspace_id.clone(),
                                "agent_id": if agent_id.is_empty() { Value::Null } else { Value::String(agent_id) },
                                "container_id": extract_container_id_from_workspace_id(&prepared.workspace_id),
                                "tree_version": tree_version,
                                "tool": name,
                                "reason": "tool_result",
                            });
                            if let Value::Object(ref mut map) = workspace_payload {
                                if let Some(first_path) = changed_paths.first() {
                                    map.insert("path".to_string(), Value::String(first_path.clone()));
                                }
                                if !changed_paths.is_empty() {
                                    map.insert(
                                        "changed_paths".to_string(),
                                        Value::Array(
                                            changed_paths
                                                .iter()
                                                .cloned()
                                                .map(Value::String)
                                                .collect(),
                                        ),
                                    );
                                }
                                round_info.insert_into(map);
                            }
                            emitter.emit("workspace_update", workspace_payload).await;
                        }

                        let question_panel_meta = if question_panel_finished {
                            let mut panel = result.data.clone();
                            if let Value::Object(ref mut map) = panel {
                                map.entry("status".to_string())
                                    .or_insert_with(|| Value::String("pending".to_string()));
                                map.entry("keep_open".to_string())
                                    .or_insert_with(|| Value::Bool(true));
                            }
                            Some(json!({ "type": "question_panel", "panel": panel }))
                        } else {
                            None
                        };
                        let sessions_yield_meta = turn_yield_message
                            .as_ref()
                            .map(|message| {
                                sessions_yield_tool::build_turn_yield_stop_meta(message.as_str())
                            });
                        if question_panel_finished {
                            let content = if answer.trim().is_empty() {
                                None
                            } else {
                                Some(&json!(answer.clone()))
                            };
                            self.append_chat(
                                &user_id,
                                &session_id,
                                "assistant",
                                content,
                                None,
                                question_panel_meta.as_ref(),
                                None,
                                None,
                                None,
                                round_info,
                            );
                        }
                        if let Some(meta) = sessions_yield_meta.as_ref() {
                            let content = if answer.trim().is_empty() {
                                None
                            } else {
                                Some(&json!(answer.clone()))
                            };
                            self.append_chat(
                                &user_id,
                                &session_id,
                                "assistant",
                                content,
                                None,
                                Some(meta),
                                None,
                                None,
                                None,
                                round_info,
                            );
                        }

                        if failure_reroute_notice.is_none() {
                            if result.ok {
                                retry_governor.record_success();
                            } else if let Some(stop) =
                                retry_governor.record_failure(&name, &result)
                            {
                                if result.error.trim().is_empty()
                                    && !stop.detail.trim().is_empty()
                                {
                                    result.error = stop.detail.clone();
                                }
                                let stop_reason_key = stop.reason;
                                let stop_fingerprint = stop.fingerprint.clone();
                                let stop_same_tool_failures = stop.same_tool_failures;
                                let stop_retryable = stop.retryable;
                                let stop_error_code = stop.error_code.clone();
                                let repeat_count = stop.repeat_count.max(stop.same_tool_failures);
                                let threshold = stop.threshold.max(1);
                                let legacy_signature =
                                    build_tool_failure_signature(&name, &result);
                                if should_request_tool_failure_reroute(
                                    stop_reason_key,
                                    reroute_notice_count,
                                    stop_fingerprint.as_str(),
                                    &reroute_notice_fingerprints,
                                ) {
                                    reroute_notice_count =
                                        reroute_notice_count.saturating_add(1);
                                    reroute_notice_fingerprints
                                        .insert(stop_fingerprint.clone());
                                    let model_notice = build_tool_failure_reroute_model_notice(
                                        &name,
                                        &stop,
                                        repeat_count,
                                        threshold,
                                        result.error.as_str(),
                                    );
                                    let next_step_hint = build_tool_failure_next_step_hint(
                                        &name,
                                        stop_error_code.as_str(),
                                        result.error.as_str(),
                                    );
                                    let mut reroute_payload = json!({
                                        "stage": "tool_failure_reroute",
                                        "summary": "Tool failure reroute triggered; model instructed to change strategy.",
                                        "tool": name.clone(),
                                        "reason": stop_reason_key,
                                        "fingerprint": stop_fingerprint.clone(),
                                        "legacy_signature": legacy_signature,
                                        "repeat_count": repeat_count,
                                        "same_tool_failures": stop_same_tool_failures,
                                        "threshold": threshold,
                                        "retryable": stop_retryable,
                                        "error_code": stop_error_code.clone(),
                                        "reroute_notice_count": reroute_notice_count,
                                        "tool_error": if result.error.trim().is_empty() {
                                            Value::Null
                                        } else {
                                            Value::String(result.error.clone())
                                        },
                                        "next_step_hint": if next_step_hint.trim().is_empty() {
                                            Value::Null
                                        } else {
                                            Value::String(next_step_hint)
                                        },
                                    });
                                    if let Value::Object(ref mut map) = reroute_payload {
                                        round_info.insert_into(map);
                                    }
                                    emitter.emit("progress", reroute_payload).await;
                                    failure_reroute_notice = Some(model_notice);
                                    retry_governor.record_success();
                                } else {
                                    answer = build_tool_failure_guard_answer(
                                        &name,
                                        &result,
                                        repeat_count,
                                        threshold,
                                    );
                                    let next_step_hint = build_tool_failure_next_step_hint(
                                        &name,
                                        stop_error_code.as_str(),
                                        result.error.as_str(),
                                    );
                                    stop_reason = Some("tool_failure_guard".to_string());
                                    let guard_meta = json!({
                                        "type": "tool_failure_guard",
                                        "tool": name.clone(),
                                        "reason": stop_reason_key,
                                        "fingerprint": stop_fingerprint.clone(),
                                        "legacy_signature": legacy_signature,
                                        "repeat_count": repeat_count,
                                        "same_tool_failures": stop_same_tool_failures,
                                        "threshold": threshold,
                                        "retryable": stop_retryable,
                                        "error_code": stop_error_code.clone(),
                                        "next_step_hint": if next_step_hint.trim().is_empty() {
                                            Value::Null
                                        } else {
                                            Value::String(next_step_hint.clone())
                                        },
                                        "tool_error": if result.error.trim().is_empty() {
                                            Value::Null
                                        } else {
                                            Value::String(result.error.clone())
                                        },
                                    });
                                    stop_meta = Some(guard_meta.clone());
                                    let mut guard_payload = json!({
                                        "stage": "tool_failure_guard",
                                        "summary": "Repeated tool failures detected; stopped retries to keep session alive.",
                                        "tool": name.clone(),
                                        "reason": stop_reason_key,
                                        "fingerprint": stop_fingerprint,
                                        "legacy_signature": legacy_signature,
                                        "repeat_count": repeat_count,
                                        "same_tool_failures": stop_same_tool_failures,
                                        "threshold": threshold,
                                        "retryable": stop_retryable,
                                        "error_code": stop_error_code,
                                        "next_step_hint": if next_step_hint.trim().is_empty() {
                                            Value::Null
                                        } else {
                                            Value::String(next_step_hint)
                                        },
                                        "tool_error": if result.error.trim().is_empty() {
                                            Value::Null
                                        } else {
                                            Value::String(result.error.clone())
                                        },
                                    });
                                    if let Value::Object(ref mut map) = guard_payload {
                                        round_info.insert_into(map);
                                    }
                                    emitter.emit("progress", guard_payload).await;
                                    self.append_chat(
                                        &user_id,
                                        &session_id,
                                        "assistant",
                                        Some(&json!(answer.clone())),
                                        None,
                                        Some(&guard_meta),
                                        None,
                                        None,
                                        None,
                                        round_info,
                                    );
                                    should_finish = true;
                                    break;
                                }
                            }
                        }

                        self.ensure_not_cancelled(&session_id)?;
                        if !answer.is_empty() {
                            break;
                        }
                        if stop_after_index.map(|stop| stop == index).unwrap_or(false) {
                            should_finish = true;
                            break;
                        }
                    }
                }

                if let Some(model_notice) = failure_reroute_notice.take() {
                    if !should_finish && answer.is_empty() {
                        let model_notice = encode_observation_prefixed_json(&model_notice);
                        let model_notice_message = json!({
                            "role": "user",
                            "content": model_notice,
                        });
                        messages.push(model_notice_message.clone());
                        self.append_model_context_entry(
                            &user_id,
                            &session_id,
                            &model_notice_message,
                        );
                        continue;
                    }
                }

                if let Some(blocked) = budget_blocked.as_ref() {
                    if !should_finish && answer.is_empty() {
                        let mut guard_payload = json!({
                            "stage": "tool_budget_guard",
                            "summary": "Tool budget soft-guard reached; model notified to continue with current evidence.",
                            "kind": blocked.kind.as_str(),
                            "limit": blocked.limit,
                            "attempted": blocked.attempted,
                            "tool": blocked.tool.clone(),
                            "usage_total": tool_budget_usage.total,
                            "usage_db_query": tool_budget_usage.db_query,
                            "usage_memory_recall": tool_budget_usage.memory_recall,
                        });
                        if let Value::Object(ref mut map) = guard_payload {
                            round_info.insert_into(map);
                        }
                        emitter.emit("progress", guard_payload).await;
                        let model_notice = build_tool_budget_guard_model_notice(
                            blocked,
                            &tool_budget_limits,
                            &tool_budget_usage,
                        );
                        let model_notice_message = json!({
                            "role": "user",
                            "content": format!("{OBSERVATION_PREFIX}{model_notice}"),
                        });
                        messages.push(model_notice_message.clone());
                        self.append_model_context_entry(
                            &user_id,
                            &session_id,
                            &model_notice_message,
                        );
                        continue;
                    }
                }

                if !should_finish && answer.is_empty() {
                    if let Some((terminal_kind, terminal)) = terminal_call {
                        let name = terminal.name.clone();
                        let args = terminal.call.arguments.clone();
                        match terminal_kind {
                            TerminalTool::A2ui => {
                                let (uid, messages_payload, content) =
                                    self.resolve_a2ui_tool_payload(&args, &user_id, &session_id);
                                append_terminal_tool_context_result(
                                    self,
                                    &user_id,
                                    &session_id,
                                    &terminal.call,
                                    &name,
                                );
                                if let Some(messages_payload) = messages_payload.as_ref() {
                                    let mut a2ui_payload = json!({
                                        "uid": uid,
                                        "messages": messages_payload,
                                        "content": content
                                    });
                                    if let Value::Object(ref mut map) = a2ui_payload {
                                        round_info.insert_into(map);
                                    }
                                    emitter.emit("a2ui", a2ui_payload).await;
                                }
                                a2ui_uid = if uid.trim().is_empty() {
                                    None
                                } else {
                                    Some(uid.clone())
                                };
                                a2ui_messages = messages_payload;
                                answer = if content.trim().is_empty() {
                                    i18n::t("response.a2ui_fallback")
                                } else {
                                    content
                                };
                                stop_reason = Some("a2ui".to_string());
                                self.log_a2ui_tool_call(
                                    &user_id,
                                    &session_id,
                                    &name,
                                    &args,
                                    &uid,
                                    &a2ui_messages,
                                    &answer,
                                    log_payload,
                                );
                                if !answer.trim().is_empty() {
                                    self.append_chat(
                                        &user_id,
                                        &session_id,
                                        "assistant",
                                        Some(&json!(answer.clone())),
                                        None,
                                        None,
                                        None,
                                        None,
                                        None,
                                        round_info,
                                    );
                                }
                                should_finish = true;
                            }
                            TerminalTool::Final => {
                                answer = self.resolve_final_answer_from_tool(&args);
                                append_terminal_tool_context_result(
                                    self,
                                    &user_id,
                                    &session_id,
                                    &terminal.call,
                                    &name,
                                );
                                if !answer.trim().is_empty() {
                                    answer = self.reconcile_final_answer_workspace_images(
                                        &prepared.workspace_id,
                                        &session_id,
                                        &answer,
                                    );
                                }
                                self.log_final_tool_call(
                                    &user_id,
                                    &session_id,
                                    &name,
                                    &args,
                                    log_payload,
                                );
                                if answer.trim().is_empty() {
                                    if empty_final_answer_reroute_count
                                        < EMPTY_FINAL_ANSWER_REROUTE_MAX_PER_TURN
                                    {
                                        empty_final_answer_reroute_count = empty_final_answer_reroute_count
                                            .saturating_add(1);
                                        let model_notice = build_empty_final_answer_model_notice(
                                            empty_final_answer_reroute_count,
                                            EMPTY_FINAL_ANSWER_REROUTE_MAX_PER_TURN,
                                            !args.is_null(),
                                            false,
                                            true,
                                            true,
                                        );
                                        let mut reroute_payload = json!({
                                            "stage": "empty_final_answer_reroute",
                                            "summary": "Model returned no usable final content from final_response; model instructed to continue instead of ending the turn.",
                                            "attempt": empty_final_answer_reroute_count,
                                            "max_attempts": EMPTY_FINAL_ANSWER_REROUTE_MAX_PER_TURN,
                                        });
                                        if let Value::Object(ref mut map) = reroute_payload {
                                            round_info.insert_into(map);
                                        }
                                        emitter.emit("progress", reroute_payload).await;
                                        let model_notice_message = json!({
                                            "role": "user",
                                            "content": encode_observation_prefixed_json(&model_notice),
                                        });
                                        messages.push(model_notice_message.clone());
                                        self.append_model_context_entry(
                                            &user_id,
                                            &session_id,
                                            &model_notice_message,
                                        );
                                        continue;
                                    }
                                    return Err(OrchestratorError::llm_unavailable(
                                        build_empty_final_answer_retry_exhausted_error(
                                            EMPTY_FINAL_ANSWER_REROUTE_MAX_PER_TURN,
                                        ),
                                    ));
                                }
                                stop_reason = Some("final_tool".to_string());
                                if !answer.trim().is_empty() {
                                    self.append_chat(
                                        &user_id,
                                        &session_id,
                                        "assistant",
                                        Some(&json!(answer.clone())),
                                        None,
                                        None,
                                        None,
                                        None,
                                        None,
                                        round_info,
                                    );
                                }
                                should_finish = true;
                            }
                        }
                    }
                }
                if should_finish || !answer.is_empty() {
                    break;
                }
            }

            if answer.is_empty() {
                if let Some((content, _)) = last_response.as_ref() {
                    answer = self.resolve_final_answer(content);
                    if !answer.trim().is_empty() {
                        answer = self.reconcile_final_answer_workspace_images(
                            &prepared.workspace_id,
                            &session_id,
                            &answer,
                        );
                    }
                    if stop_reason.is_none() && reached_max_rounds {
                        stop_reason = Some("max_rounds".to_string());
                    }
                }
            }
            if reached_max_rounds {
                answer = build_max_rounds_user_guidance(max_rounds);
                if stop_reason.is_none() {
                    stop_reason = Some("max_rounds".to_string());
                }
            }
            if answer.is_empty() {
                return Err(OrchestratorError::llm_unavailable(
                    build_empty_final_answer_retry_exhausted_error(
                        EMPTY_FINAL_ANSWER_REROUTE_MAX_PER_TURN,
                    ),
                ));
            }

            let stop_reason = stop_reason.unwrap_or_else(|| "unknown".to_string());
            let waiting_question_panel = stop_reason == "question_panel";
            if let (true, Some(turn_id)) = (waiting_question_panel, active_turn_id.as_deref()) {
                let _ = self
                    .active_turns
                    .mark_waiting_user_input(&session_id, turn_id);
            }
            round_usage.total =
                round_usage
                    .total
                    .max(round_usage.input.saturating_add(round_usage.output));
            let has_round_usage =
                round_usage.total > 0 || round_usage.input > 0 || round_usage.output > 0;
            let round_context_tokens = resolve_round_context_occupancy_tokens(
                confirmed_context_occupancy_tokens,
                persisted_context_tokens,
            );
            if has_round_usage {
                self.emit_and_persist_round_usage(
                    &user_id,
                    &session_id,
                    &emitter,
                    request_round,
                    &round_usage,
                    round_context_tokens,
                )
                .await;
            }

            let response_usage = last_model_usage
                .clone()
                .or_else(|| has_round_usage.then_some(round_usage.clone()));
            let response = WunderResponse {
                session_id: session_id.clone(),
                answer: answer.clone(),
                usage: response_usage.clone(),
                stop_reason: Some(stop_reason.clone()),
                uid: a2ui_uid.clone(),
                a2ui: a2ui_messages.clone(),
            };
            let final_payload = build_final_event_payload(
                &answer,
                response_usage.as_ref(),
                &round_usage,
                round_context_tokens,
                &stop_reason,
                stop_meta.as_ref(),
                last_round_info,
                &turn_decode_speed,
            );
            emitter.emit("final", final_payload).await;
            self.finish_request_success(
                &user_id,
                &session_id,
                prepared.agent_id.as_deref(),
                &user_round_id,
                &display_question,
                &answer,
                &emitter,
                last_round_info,
                active_turn_id.as_deref(),
                goal_continuation_turn,
                goal_turn_started_at,
                waiting_question_panel,
                has_round_usage,
                &round_usage,
                &stop_reason,
                stop_meta.as_ref(),
                skip_auto_memory_extract,
                llm_config.clone(),
            )
            .await;
            Ok(response)
        }
        .await;

        match result {
            Ok(value) => {
                Self::finish_request_resources(
                    &emitter,
                    &limiter,
                    &session_id,
                    acquired,
                    &mut heartbeat_task,
                )
                .await;
                Ok(value)
            }
            Err(err) => {
                self.finish_request_error(
                    &user_id,
                    &session_id,
                    &emitter,
                    active_turn_id.as_deref(),
                    active_turn_round,
                    &err,
                )
                .await;
                Self::finish_request_resources(
                    &emitter,
                    &limiter,
                    &session_id,
                    acquired,
                    &mut heartbeat_task,
                )
                .await;
                Err(err)
            }
        }
    }
}
