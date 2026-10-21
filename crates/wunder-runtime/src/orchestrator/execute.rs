use super::context::normalize_model_context_message;
use super::empty_output_guard::EmptyOutputGuard;
use super::retry_governor::{RetryGovernor, SuccessProgressGovernor};
use super::tool_calls::ToolCall;
use super::tool_result_payload::{
    default_observation_token_budget, OBSERVATION_MIN_TOKEN_BUDGET, OBSERVATION_TURN_MAX_TOKENS,
    OBSERVATION_TURN_TARGET_TOKENS,
};
use super::*;
use crate::core::llm_speed::TurnDecodeSpeedAccumulator;
use crate::core::long_task;
use crate::orchestrator_constants::{COMPACTION_SAFETY_MARGIN, DEFAULT_MAX_OUTPUT_TOKENS};
use crate::services::chat_attachments::persist_user_chat_attachments;
use crate::services::goal;
use crate::services::subagents;
use crate::services::tools::sessions_yield_tool;

use super::execute_support::*;

struct ObservationTokenBudgets {
    per_result: Vec<i64>,
    total_tokens: i64,
    protocol_reserve_tokens: i64,
    available_context_tokens: i64,
}

fn resolve_observation_token_budgets<'a>(
    tool_names: impl IntoIterator<Item = &'a str>,
    max_context: i64,
    observed_context: i64,
    configured_max_output: Option<i64>,
) -> ObservationTokenBudgets {
    // Keep only small scalar caps, never copies of the tool results. Allocate
    // the whole batch before rendering so earlier results cannot consume the
    // envelopes that later tool-call/result pairs must retain.
    let mut per_result: Vec<i64> = tool_names
        .into_iter()
        .map(default_observation_token_budget)
        .map(|cap| cap.max(OBSERVATION_MIN_TOKEN_BUDGET))
        .collect();
    let result_count = i64::try_from(per_result.len()).unwrap_or(i64::MAX);
    let protocol_reserve_tokens = result_count.saturating_mul(OBSERVATION_MIN_TOKEN_BUDGET);
    let max_context = max_context.max(0);
    // Preserve room for the next completion and request framing. Provider
    // usage is authoritative when present; this only controls new tool
    // observations before the next model request.
    let output_reserve = configured_max_output
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS)
        .min(max_context.saturating_div(2));
    let safety_reserve = (max_context / 16).clamp(COMPACTION_SAFETY_MARGIN, 8_192);
    let available_context_tokens = max_context
        .saturating_sub(observed_context.max(0))
        .saturating_sub(output_reserve)
        .saturating_sub(safety_reserve)
        .max(0);
    let content_target = available_context_tokens
        .min(OBSERVATION_TURN_TARGET_TOKENS)
        .min(OBSERVATION_TURN_MAX_TOKENS);
    let cap_sum = per_result.iter().copied().fold(0_i64, i64::saturating_add);
    // 8k is the content target and 12k is a soft content ceiling. A very
    // large batch or a full context still needs one minimal protocol
    // envelope per result; this reserve may exceed either soft limit. This
    // allocation is not an admission guarantee for the complete next request.
    let total_tokens = content_target.max(protocol_reserve_tokens).min(cap_sum);

    // Max-min fair allocation: smaller tool caps settle first; the remaining
    // results share a common water level. Binary search bounds work by the
    // largest per-tool cap, with no sorting or second result-sized vector.
    let mut low = 0_i64;
    let mut high = per_result.iter().copied().max().unwrap_or(0);
    while low < high {
        let middle = low + (high - low + 1) / 2;
        let required = per_result
            .iter()
            .map(|cap| (*cap).min(middle))
            .fold(0_i64, i64::saturating_add);
        if required <= total_tokens {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let assigned = per_result
        .iter()
        .map(|cap| (*cap).min(low))
        .fold(0_i64, i64::saturating_add);
    let mut remainder = total_tokens.saturating_sub(assigned);
    for budget in &mut per_result {
        let cap = *budget;
        *budget = cap.min(low);
        if remainder > 0 && cap > low {
            *budget += 1;
            remainder -= 1;
        }
    }
    ObservationTokenBudgets {
        per_result,
        total_tokens,
        protocol_reserve_tokens,
        available_context_tokens,
    }
}

enum ToolEventForward {
    Event { event_type: String, data: Value },
    Flush(tokio::sync::oneshot::Sender<()>),
}

struct ToolEventForwarder {
    tx: mpsc::Sender<ToolEventForward>,
}

impl ToolEventForwarder {
    fn new(emitter: EventEmitter, round_info: RoundInfo) -> Self {
        let (tx, mut rx) = mpsc::channel::<ToolEventForward>(STREAM_EVENT_QUEUE_SIZE);
        long_task::spawn("orchestrator.execute.tool_event_forwarder", async move {
            while let Some(message) = rx.recv().await {
                match message {
                    ToolEventForward::Event {
                        event_type,
                        mut data,
                    } => {
                        if let Value::Object(ref mut map) = data {
                            round_info.insert_into(map);
                        }
                        emitter.emit(&event_type, data).await;
                    }
                    ToolEventForward::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        Self { tx }
    }

    fn emitter(&self, stream: bool) -> ToolEventEmitter {
        let tx = self.tx.clone();
        ToolEventEmitter::new(
            move |event_type, data| {
                let message = ToolEventForward::Event {
                    event_type: event_type.to_string(),
                    data,
                };
                if let Err(err) = tx.try_send(message) {
                    match err {
                        mpsc::error::TrySendError::Full(message) => {
                            let tx = tx.clone();
                            let _ = tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current()
                                    .block_on(async move { tx.send(message).await })
                            });
                        }
                        mpsc::error::TrySendError::Closed(_) => {}
                    }
                }
            },
            stream,
        )
    }

    async fn flush(&self) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if self.tx.send(ToolEventForward::Flush(tx)).await.is_ok() {
            let _ = rx.await;
        }
    }
}

impl Orchestrator {
    pub(super) async fn execute_request(
        &self,
        prepared: PreparedRequest,
        emitter: EventEmitter,
    ) -> Result<WunderResponse, OrchestratorError> {
        let mut heartbeat_task: Option<JoinHandle<()>> = None;
        let mut acquired = false;
        let mut agent_inbox = None;
        let mut _scheduling_guard = None;
        let mut prepared = prepared;
        let request_config = self
            .resolve_config(prepared.config_overrides.as_ref())
            .await;
        let max_active_sessions = if prepared.is_admin && !prepared.enforce_runtime_queue {
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
        let goal_round = goal::read_goal_round_tag(prepared.config_overrides.as_ref());
        let display_question = display_question_override
            .clone()
            .unwrap_or_else(|| question.clone());
        let is_admin = prepared.is_admin;
        let mut active_turn_id: Option<String> = None;
        let mut active_turn_round = match (prepared.thread_turn_id, prepared.thread_user_round) {
            (Some(turn_id), Some(user_round)) => RoundInfo::user_only_thread(user_round, turn_id),
            _ => RoundInfo::default(),
        };

        let result = async {
            // Execution exclusion belongs to a session; one agent can run independent tasks.
            let lock_agent_id = prepared.agent_id.clone().unwrap_or_default();
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
            if prepared.enforce_runtime_queue {
                let priority = prepared.config_overrides.as_ref().and_then(|value| value.get("__queue_priority")).and_then(Value::as_i64).unwrap_or(0) > 0;
                _scheduling_guard = Some(self.scheduling.register(&session_id, prepared.is_admin || priority));
            }

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

            // Stream events are durable across rounds; never clear them from the
            // request path because replay depends on a continuous event sequence.
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

            let user_round = prepared.thread_user_round.expect("accepted user round");
            // Monitoring observes the durable round; it never allocates its identity.
            self.monitor.register_continuation(&session_id, &user_id,
                prepared.agent_id.as_deref().unwrap_or(""), &display_question, is_admin, user_round);
            let mut request_round = prepared
                .thread_turn_id
                .map(|turn_id| RoundInfo::user_only_thread(user_round, turn_id))
                .unwrap_or_else(|| RoundInfo::user_only(user_round));
            // A runtime-driven goal round is not a direct human turn: providers
            // must not repeat human-only goal management calls inside it.
            request_round.is_goal_round = goal_round.is_some();
            if let Some(turn_id) = request_round.thread_turn_id {
                let payload = json!({
                    "session_id": session_id,
                    "turn_id": turn_id.to_string(),
                    "user_round": user_round,
                    "status": "running",
                    "summary": display_question,
                });
                // The unified commit exit persists the turn update and publishes
                // the change cursor only when the write actually changed state.
                self
                    .committer
                    .update_turn(
                        &user_id,
                        &session_id,
                        &turn_id.to_string(),
                        "running",
                        &display_question,
                        &payload,
                    )
                    .await
                    .map_err(|error| OrchestratorError::internal(format!(
                        "persist running thread turn: {error}"
                    )))?;
            }
            // Child cancellation survives monitor registration resetting the turn flags.
            self.ensure_not_cancelled(&session_id)?;
            agent_inbox = Some(self.monitor.mailboxes.open(&user_id, &session_id)
                .map_err(|error| OrchestratorError::internal(error.to_string()))?);
            if let Some(task_id) = prepared.config_overrides.as_ref().and_then(|value| value.get("__queue_task_id")).and_then(Value::as_str) {
                // Cancellation may win after the worker claims a task but before monitor registration.
                let storage = self.storage.clone();
                let task_id = task_id.to_string();
                let active = crate::core::blocking::run_db("queue.validate_claim", move || storage.get_agent_task(&task_id))
                    .await.map_err(|err| OrchestratorError::internal(err.to_string()))?
                    .is_some_and(|task| task.status == "running");
                if !active { return Err(OrchestratorError::cancelled(i18n::t("error.session_cancelled"))); }
            }
            let active_turn = self.active_turns.begin_turn_with_id(&session_id, &prepared.thread_turn_id.expect("accepted turn").to_string());
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
                if let Some(client_message_id) = prepared.client_message_id.as_deref() {
                    map.insert(
                        "client_message_id".to_string(),
                        json!(client_message_id),
                    );
                }
                request_round.insert_into(map);
            }
            emitter.emit("progress", start_payload).await;

            let config = request_config.clone();
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
            let extra_tool_roots = vec![private_root];
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
                llm_config.support_vision,
            );
            let allowed_tool_names = self.apply_preview_skill_tool_policy(
                allowed_tool_names,
                prepared.preview_skill,
            );
            // Freeze only the named tool-profile binding in thread metadata.
            // The system prompt itself was already designed to be immutable
            // after initialization, so it must not be rewritten when this
            // runtime policy is first established or recovered.
            let (tool_profile, frozen_profile_binding) = self
                .resolve_frozen_session_tool_profile(
                    &user_id,
                    &session_id,
                    &config,
                    prepared.config_overrides.as_ref(),
                )
                .await;
            let allowed_tool_names = crate::tools::filter_tool_names_by_profile(
                allowed_tool_names,
                tool_profile,
            );
            // Profile policy happens before the stored surface intersection.
            // Current permissions have already been applied above, so neither
            // a stale binding nor a catalog change can reintroduce a tool.
            let allowed_tool_names = match frozen_profile_binding.as_ref() {
                Some(binding) if !binding.tool_ids.is_empty() => {
                    crate::tools::filter_tool_names_by_frozen_ids(
                        allowed_tool_names,
                        &binding.tool_ids,
                    )
                }
                _ => allowed_tool_names,
            };
            if frozen_profile_binding
                .as_ref()
                .map(|binding| binding.tool_ids.is_empty())
                .unwrap_or(true)
            {
                let specs = collect_prompt_tool_specs_with_language(
                    &config,
                    &skills_snapshot,
                    &allowed_tool_names,
                    Some(&user_tool_bindings),
                    "en-US",
                );
                let snapshot =
                    crate::tools::tool_profile_snapshot_for_profile(&specs, tool_profile);
                self.workspace.save_session_frozen_tool_profile(
                    &user_id,
                    &session_id,
                    &crate::tools::tool_profile_binding(&snapshot),
                );
            }
            let tool_call_mode =
                self.resolve_frozen_session_tool_call_mode(&user_id, &session_id, &llm_config);
            // 虚拟模型不接触真实供应商，工具面只用于合成轨迹：即便当前会话冻结在
            // 文本协议（`tool_call`），也要把工具定义交给模拟器，否则它拿不到可调用
            // 的工具名，随机轨迹会退化成「一轮直接收尾」，数字指定的模型轮次随之失效。
            // 文本协议下 `tool_protocol::adapt` 会把这些调用还原成 `<tool_call>` 文本。
            let virtual_model = crate::services::virtual_llm::is_virtual_replay_provider(
                llm_config.provider.as_deref(),
            );
            let function_tooling = if (uses_native_tool_api(tool_call_mode, &llm_config)
                || virtual_model)
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
            // Ensure the previous turn's async history writes are visible before deriving
            // this turn's model input. Otherwise fast follow-up user messages can miss the
            // tail of the append-only chat history and break KV cache reuse.
            let _ = self.workspace.flush_writes_async().await;
            let model_context_entries = history_manager
                .load_execution_messages_async(
                    self.workspace.clone(),
                    user_id.clone(),
                    session_id.clone(),
                    prepared.thread_turn_id.expect("accepted turn").to_string(),
                )
                .await
                .map_err(|error| OrchestratorError::internal(error.to_string()))?
                .into_iter()
                .filter_map(normalize_model_context_message)
                .collect::<Vec<_>>();
            messages.extend(model_context_entries);
            messages = context_manager.normalize_messages(messages);
            let user_message = self
                .build_user_message(&question, prepared.attachments.as_deref())
                .await;
            messages.push(user_message.clone());
            let persisted_user_model_message = user_message.clone();
            messages = context_manager.normalize_messages(messages);
            let mut user_message_appended = false;

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
            let turn_started_at = Instant::now();
            let mut round_usage: TokenUsage;
            let mut last_model_usage: Option<TokenUsage> = None;
            let mut confirmed_context_occupancy_tokens: Option<i64> = None;
            let mut turn_decode_speed = TurnDecodeSpeedAccumulator::default();
            let mut answer = String::new();
            let mut stop_reason: Option<String> = None;
            let mut stop_meta: Option<Value> = None;
            let mut last_response: Option<(String, String)> = None;
            let mut last_round_info = request_round;

            // Every admitted goal round is its own visible user turn, so the
            // model-round sequence always starts fresh inside that turn.
            let mut model_round = 0;
            let repeated_tool_failure_threshold =
                resolve_tool_failure_guard_threshold(&request_config);
            let mut retry_governor = RetryGovernor::new(repeated_tool_failure_threshold);
            let mut success_progress_governor = SuccessProgressGovernor::default();
            let mut reroute_notice_count = 0_u32;
            let mut reroute_notice_fingerprints: HashSet<String> = HashSet::new();
            let mut invalid_tool_call_reroute_count = 0_u32;
            let mut empty_output_guard = EmptyOutputGuard::default();
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
            // Provider usage drives the persisted occupancy display. Local
            // estimates additionally guard outgoing requests, without being
            // recorded as observed or billed usage.
            let request_overhead_tokens = super::request_budget::tools_tokens(tools_payload);
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
                self.yield_queue_slot(&session_id, &emitter, last_round_info).await?;
                if let Some(max_rounds) = max_rounds {
                    if model_round >= max_rounds {
                        reached_max_rounds = true;
                        break;
                    }
                }
                model_round += 1;
                let round_info = request_round.with_model_round(model_round);
                last_round_info = round_info;
                // Error/cancel recovery must settle the model round that was
                // actually admitted. Keeping the initial user-only identity
                // here creates a second random assistant item and leaves the
                // streamed `{turn}:text-{round}` item running after refresh.
                active_turn_round = round_info;
                let mut adaptive_recovery_limit_hint: Option<i64> = None;
                self.ensure_not_cancelled(&session_id)?;
                let compaction_llm_config = apply_context_window_limit_hint(
                    &llm_config,
                    merge_context_window_limit_hint(
                        context_window_limit_hint,
                        adaptive_recovery_limit_hint,
                    ),
                );
                messages = context_manager.normalize_messages(messages);
                let request_budget = super::request_budget::RequestBudget::measure(
                    &compaction_llm_config,
                    &self.build_chat_messages(&messages),
                    tools_payload,
                );
                let admission_compaction_needed = !request_budget.admitted();
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
                        persisted_context_tokens,
                        request_overhead_tokens,
                        force_compaction_on_entry || admission_compaction_needed,
                        true,
                        super::memory::CompactionRunMode::AutoLoop,
                    )
                    .await?;
                messages = compaction_result.messages;
                if compaction_result.model_context_replaced {
                    persisted_context_tokens = 0;
                    confirmed_context_occupancy_tokens = None;
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
                    let model_content = if display_question_override.is_some() {
                        persisted_user_model_message.get("content").cloned()
                    } else {
                        None
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
                            model_content.as_ref(),
                            request_round,
                        );
                    }
                    user_message_appended = true;
                }

                let mut overflow_recovery_attempts = 0_u32;
                self.apply_agent_messages(agent_inbox.as_ref().expect("active inbox"),
                    &user_id, &session_id, &mut messages, &emitter, round_info, false).await?;
                let (content, reasoning, usage, tool_calls_payload, round_speed, output) = loop {
                    match self
                        .call_llm(
                            &apply_context_window_limit_hint(
                                &llm_config,
                                merge_context_window_limit_hint(context_window_limit_hint, adaptive_recovery_limit_hint),
                            ),
                            &messages,
                            &user_id,
                            is_admin,
                            &emitter,
                            &session_id,
                            prepared.stream,
                            round_info,
                            true,
                            true,
                            tools_payload,
                            empty_output_guard.config_override(&llm_config),
                            Some(question.as_str()),
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
                            } else if err.detail().and_then(|detail| detail.get("source")).and_then(Value::as_str)
                                != Some("local_request_admission") {
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
                            confirmed_context_occupancy_tokens = None;
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
                empty_output_guard.observe(output);
                last_response = Some((content.clone(), reasoning.clone()));
                turn_decode_speed.record_summary(&round_speed);
                round_usage = emitter.accumulated_usage();
                last_model_usage = Some(usage.clone());
                // Provider input counts describe this request's prompt (including
                // cached input). Billing totals also contain generated reasoning/output.
                // Virtual providers mark usage as estimated (nothing is billed or
                // charged against the budget), but their input count is an exact
                // measure of the prompt, so it still drives occupancy + compaction.
                let request_context_tokens =
                    super::usage_accounting::request_context_tokens(&usage).or_else(|| {
                        crate::services::virtual_llm::is_virtual_replay_provider(
                            llm_config.provider.as_deref(),
                        )
                        .then(|| super::usage_accounting::simulated_request_context_tokens(&usage))
                        .flatten()
                    });
                if let Some(tokens) = request_context_tokens {
                    confirmed_context_occupancy_tokens = Some(tokens);
                    persisted_context_tokens = tokens;
                    self.workspace.save_session_context_tokens_async(&user_id, &session_id, tokens).await;
                    let mut snapshot = json!({
                        "context_tokens": tokens,
                        "context_occupancy_tokens": tokens,
                        "context_usage_source": "provider_input",
                        "max_context": merge_context_window_limit_hint(llm_config.max_context.map(i64::from), context_window_limit_hint),
                    });
                    round_info.insert_into(snapshot.as_object_mut().expect("context object"));
                    emitter.emit("context_usage", snapshot).await;
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
                if !planned_calls.is_empty() {
                    self.yield_queue_slot(&session_id, &emitter, round_info).await?;
                }
                if planning_result.rejected.iter().any(|call| call.reason == "TOOL_ARGUMENTS_INCOMPLETE") {
                    empty_output_guard.recover_or_stop(
                        self, &mut messages, &user_id, &session_id, &emitter,
                        round_info, true, "TOOL_ARGUMENTS_INCOMPLETE",
                    ).await?;
                    continue;
                }
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
                    self.append_internal_model_context_chat(
                        &user_id,
                        &session_id,
                        &model_notice_message,
                        "invalid_tool_call_reroute",
                        round_info,
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
                        empty_output_guard.recover_or_stop(
                            self, &mut messages, &user_id, &session_id, &emitter,
                            round_info, !prepared.skip_tool_calls, "empty_output",
                        ).await?;
                        continue;
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
                    messages.push(assistant_model_message.clone());
                    if self.apply_agent_messages(agent_inbox.as_ref().expect("active inbox"),
                        &user_id, &session_id, &mut messages, &emitter, round_info, true).await? {
                        self.append_internal_model_context_chat(
                            &user_id,
                            &session_id,
                            &assistant_model_message,
                            "agent_inbox_interrupted_answer",
                            round_info,
                        );
                        answer.clear();
                        stop_reason = None;
                        continue;
                    }
                    if !assistant_content.trim().is_empty() {
                        let message_stats = build_persisted_message_stats(
                            &usage,
                            &round_usage,
                            confirmed_context_occupancy_tokens,
                            &turn_decode_speed,
                            turn_started_at.elapsed().as_secs_f64(),
                            emitter.accumulated_model_requests(),
                            emitter.accumulated_billable_account_credits(is_admin),
                            i64::from(tool_budget_usage.total),
                        );
                        self.append_chat(
                            &user_id,
                            &session_id,
                            "assistant",
                            Some(&json!(assistant_content)),
                            None,
                            Some(&json!({ "message_stats": message_stats })),
                            Some(&reasoning),
                            None,
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
                    let message_stats = build_persisted_message_stats(
                        &usage,
                        &round_usage,
                        confirmed_context_occupancy_tokens,
                        &turn_decode_speed,
                        turn_started_at.elapsed().as_secs_f64(),
                        emitter.accumulated_model_requests(),
                        emitter.accumulated_billable_account_credits(is_admin),
                        i64::from(tool_budget_usage.total),
                    );
                    self.append_chat(
                        &user_id,
                        &session_id,
                        "assistant",
                        Some(&json!(assistant_content)),
                        None,
                        Some(&json!({ "type": "tool_call", "message_stats": message_stats })),
                        Some(&assistant_reasoning),
                        assistant_history.persisted_tool_calls.as_ref(),
                        None,
                        None,
                        round_info,
                    );
                }

                let tool_event_forwarder =
                    ToolEventForwarder::new(emitter.clone(), round_info);
                let tool_event_emitter = tool_event_forwarder.emitter(prepared.stream);

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
                    workspace: self.workspace.clone(),
                    lsp_manager: self.lsp_manager.clone(),
                    config: &config,
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

                let question_panel_name = resolve_tool_name("question_panel");
                let read_tool_name = resolve_tool_name("read_file");

                let mut exec_calls: Vec<PlannedToolCall> = Vec::new();
                let mut stop_after_index: Option<usize> = None;
                for planned in planned_calls {
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
                    let raw_recovered =
                        crate::core::tool_args::recover_tool_args_value_with_meta(&safe_args);
                    let schema_recovered = crate::tools::current_executable_tool_input_schema(
                        &config,
                        &skills_snapshot,
                        Some(&user_tool_bindings),
                        &planned.name,
                    )
                    .map(|schema| {
                        crate::core::tool_args::recover_tool_args_value_against_schema_with_meta(
                            &raw_recovered.value,
                            &schema,
                        )
                    });
                    let recovered_args = match schema_recovered {
                        Some(schema_recovered) => crate::core::tool_args::ToolArgsRecovery {
                            value: schema_recovered.value,
                            repair: match (raw_recovered.repair, schema_recovered.repair) {
                                (None, repair) | (repair, None) => repair,
                                (Some(first), Some(second)) => Some(json!({
                                    "kind": "tool_args",
                                    "repairs": [first, second],
                                })),
                            },
                        },
                        None => raw_recovered,
                    };
                    let event_args = if allowed_tool_names.contains(&planned.name) {
                        recovered_args.value.clone()
                    } else {
                        safe_args
                    };
                    let mut tool_payload = json!({
                        "tool": planned.name, "args": event_args,
                        "request_context_tokens": request_context_tokens,
                        "request_usage": usage,
                    });
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
                        if let Some(reference) = crate::tools::current_tool_schema_reference(
                            &config,
                            &skills_snapshot,
                            Some(&user_tool_bindings),
                            &planned.name,
                        ) {
                            map.insert(
                                "schema_ref".to_string(),
                                serde_json::to_value(reference).unwrap_or(Value::Null),
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
                        let cache_started_at = Instant::now();
                        if let Some(cached) = resolve_cached_memory_recall_result(
                            &planned,
                            &memory_manager_tool_name,
                            &memory_recall_cache,
                            memory_recall_revision,
                        ) {
                            let mut result = cached.to_payload();
                            result.insert_meta("recall_cache_hit", Value::Bool(true));
                            // A cache hit is a new invocation, not the original tool's latency.
                            result.insert_meta("duration_ms", json!(cache_started_at.elapsed().as_millis() as u64));
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
                        let outcomes = self
                            .execute_tool_calls_parallel(
                            executable_calls,
                            &tool_context,
                            &allowed_tool_names,
                            &session_id,
                            active_turn.turn_id.as_str(),
                            &emitter,
                            prepared.approval_tx.clone(),
                            round_info,
                        )
                            .await;
                        tool_event_forwarder.flush().await;
                        outcomes?
                    };
                    outcomes.extend(cached_recall_outcomes);
                    let observation_budgets = resolve_observation_token_budgets(
                        outcomes.iter().map(|outcome| outcome.name.as_str()),
                        compaction_llm_config.effective_max_context() as i64,
                        persisted_context_tokens,
                        compaction_llm_config.max_output.map(i64::from),
                    );
                    if observation_budgets.protocol_reserve_tokens
                        > observation_budgets.available_context_tokens
                        || observation_budgets.protocol_reserve_tokens > OBSERVATION_TURN_MAX_TOKENS
                    {
                        warn!(
                            %session_id,
                            result_count = outcomes.len(),
                            protocol_reserve_tokens = observation_budgets.protocol_reserve_tokens,
                            available_context_tokens = observation_budgets.available_context_tokens,
                            allocated_observation_tokens = observation_budgets.total_tokens,
                            soft_content_limit_tokens = OBSERVATION_TURN_MAX_TOKENS,
                            "tool-result protocol reserve exceeds the content budget; preserving every tool-call/result pair"
                        );
                    }
                    for (index, outcome) in outcomes.into_iter().enumerate() {
                        // Indexing deliberately fails on an allocator invariant
                        // violation instead of silently dropping a result as
                        // a zip over mismatched lengths would.
                        let observation_budget = observation_budgets.per_result[index];
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
                        let schema_reference = crate::tools::current_tool_schema_reference(
                            &config,
                            &skills_snapshot,
                            Some(&user_tool_bindings),
                            &name,
                        );

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

                        let mut observation_payload = result.to_compact_payload_with_token_budget(
                            &name, Some(observation_budget),
                        );
                        if super::tool_result_payload::observation_has_omissions(&observation_payload, 0)
                            && result.data.get("result_read") != Some(&Value::Bool(true))
                            && result.meta.as_ref().is_none_or(|meta| {
                                meta.get("saved_result").is_none() && meta.get("result_store_failed").is_none()
                            })
                        {
                            result = super::tool_result_recovery::preserve_tool_result(
                                result, &tool_context,
                            ).await;
                            observation_payload = result.to_compact_payload_with_token_budget(
                                &name, Some(observation_budget),
                            );
                        }
                        let observation = serde_json::to_string(&observation_payload)
                            .unwrap_or_else(|_| "{}".to_string());
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
                        // Autonomous terminal updates hand the turn a closing
                        // instruction (deepseek deferContext equivalent).
                        let goal_wrapup = if result.ok && goal::is_goal_tool_name(&name) {
                            goal::read_wrapup(&result.data)
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
                            None,
                            round_info,
                        );
                        if let Some(followup_message) = read_image_followup {
                            let mut followup_message = followup_message;
                            self.mark_internal_model_context_message(
                                &mut followup_message,
                                "read_image_followup",
                                round_info,
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
                        if let Some(wrapup_text) = goal_wrapup {
                            let mut followup_message =
                                json!({"role": "user", "content": wrapup_text});
                            self.mark_internal_model_context_message(
                                &mut followup_message,
                                "goal_wrapup",
                                round_info,
                            );
                            self.append_internal_model_context_chat(
                                &user_id,
                                &session_id,
                                &followup_message,
                                "goal_wrapup",
                                round_info,
                            );
                            messages.push(followup_message);
                        }

                        self.append_tool_log(
                            &user_id,
                            &session_id,
                            &name,
                            &args,
                            &result,
                            round_info,
                            event_tool_call_id.as_deref(),
                            request_context_tokens,
                            Some(&usage),
                            schema_reference.as_ref(),
                        );
                        self.append_artifact_logs(&user_id, &session_id, &name, &args, &result);
                        if name == read_tool_name {
                            self.append_skill_usage_logs(
                                &user_id,
                                &session_id,
                                &args,
                                &skills_snapshot,
                                Some(&user_tool_bindings),
                                round_info,
                            );
                        }

                        let mut tool_result_payload = result.to_event_payload(&name);
                        // The durable result replaces the call item; keep invocation data for replay.
                        tool_result_payload["args"] = args.clone();
                        tool_result_payload["request_context_tokens"] = json!(request_context_tokens);
                        tool_result_payload["request_usage"] = json!(usage);
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
                            if let Some(reference) = schema_reference.as_ref() {
                                map.insert(
                                    "schema_ref".to_string(),
                                    serde_json::to_value(reference).unwrap_or(Value::Null),
                                );
                            }
                            map.insert(
                                "model_observation".to_string(),
                                observation_value.clone(),
                            );
                            map.remove("trace_id");
                            map.remove("user_round");
                            map.remove("model_round");
                            round_info.insert_into(map);
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
                            let message_stats = build_persisted_message_stats(
                                &usage,
                                &round_usage,
                                confirmed_context_occupancy_tokens,
                                &turn_decode_speed,
                                turn_started_at.elapsed().as_secs_f64(),
                                emitter.accumulated_model_requests(),
                                emitter.accumulated_billable_account_credits(is_admin),
                                i64::from(tool_budget_usage.total),
                            );
                            let meta = question_panel_meta.as_ref().map(|value| {
                                merge_persisted_message_stats_meta(value, message_stats)
                            });
                            self.append_chat(
                                &user_id,
                                &session_id,
                                "assistant",
                                content,
                                None,
                                meta.as_ref(),
                                None,
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
                            let message_stats = build_persisted_message_stats(
                                &usage,
                                &round_usage,
                                confirmed_context_occupancy_tokens,
                                &turn_decode_speed,
                                turn_started_at.elapsed().as_secs_f64(),
                                emitter.accumulated_model_requests(),
                                emitter.accumulated_billable_account_credits(is_admin),
                                i64::from(tool_budget_usage.total),
                            );
                            let meta = merge_persisted_message_stats_meta(meta, message_stats);
                            self.append_chat(
                                &user_id,
                                &session_id,
                                "assistant",
                                content,
                                None,
                                Some(&meta),
                                None,
                                None,
                                None,
                                None,
                                round_info,
                            );
                        }

                        if failure_reroute_notice.is_none() {
                            if result.ok {
                                if let Some(stop) =
                                    success_progress_governor.record(&name, &args, &result)
                                {
                                    let detail = stop.detail.clone();
                                    let guard_answer = build_tool_no_progress_guard_answer(
                                        &stop.tool,
                                        stop.repeat_count,
                                        stop.threshold,
                                        &detail,
                                    );
                                    stop_reason = Some("tool_no_progress_guard".to_string());
                                    stop_meta = Some(json!({
                                        "type": "tool_no_progress_guard",
                                        "tool": stop.tool.clone(),
                                        "repeat_count": stop.repeat_count,
                                        "threshold": stop.threshold,
                                        "detail": detail,
                                    }));
                                    let mut guard_payload = json!({
                                        "stage": "tool_no_progress_guard",
                                        "summary": "Repeated successful tool observations produced no new progress; stopped the loop.",
                                        "tool": stop.tool,
                                        "repeat_count": stop.repeat_count,
                                        "threshold": stop.threshold,
                                        "detail": stop.detail,
                                    });
                                    if let Value::Object(ref mut map) = guard_payload {
                                        round_info.insert_into(map);
                                    }
                                    emitter.emit("progress", guard_payload).await;
                                    answer = guard_answer;
                                    should_finish = true;
                                } else {
                                    retry_governor.record_success();
                                }
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
                                    let guard_meta = merge_persisted_message_stats_meta(
                                        &guard_meta,
                                        build_persisted_message_stats(
                                            &usage,
                                            &round_usage,
                                            confirmed_context_occupancy_tokens,
                                            &turn_decode_speed,
                                            turn_started_at.elapsed().as_secs_f64(),
                                            emitter.accumulated_model_requests(),
                                            emitter.accumulated_billable_account_credits(is_admin),
                                            i64::from(tool_budget_usage.total),
                                        ),
                                    );
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
                        self.append_internal_model_context_chat(
                            &user_id,
                            &session_id,
                            &model_notice_message,
                            "tool_failure_reroute",
                            round_info,
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
                        self.append_internal_model_context_chat(
                            &user_id,
                            &session_id,
                            &model_notice_message,
                            "tool_budget_guard",
                            round_info,
                        );
                        continue;
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
                return Err(empty_output_guard.exhausted_error());
            }

            let stop_reason = stop_reason.unwrap_or_else(|| "unknown".to_string());
            let waiting_question_panel = stop_reason == "question_panel";
            if let (true, Some(turn_id)) = (waiting_question_panel, active_turn_id.as_deref()) {
                let _ = self
                    .active_turns
                    .mark_waiting_user_input(&session_id, turn_id);
            }
            round_usage = emitter.accumulated_usage();
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
                uid: None,
                a2ui: None,
            };
            let mut final_payload = build_final_event_payload(
                &answer,
                response_usage.as_ref(),
                &round_usage,
                round_context_tokens,
                &stop_reason,
                stop_meta.as_ref(),
                last_round_info,
                &turn_decode_speed,
            );
            final_payload["interaction_duration_s"] = json!(turn_started_at.elapsed().as_secs_f64());
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
                goal_round.clone(),
                waiting_question_panel,
                has_round_usage,
                &round_usage,
                &stop_reason,
                stop_meta.as_ref(),
                skip_auto_memory_extract,
                llm_config.clone(),
            )
            .await?;
            // A successful transport receipt follows the durable lifecycle.
            emitter.emit("final", final_payload).await;
            Ok(response)
        }
        .await;

        if let Some(inbox) = agent_inbox {
            for message in inbox.close() {
                emitter
                    .emit(
                        "subagent_message",
                        json!({"message_id":message.id,
                    "source_session_id":message.source,"session_id":session_id,
                    "kind":message.kind,"delivery":"not_applied"}),
                    )
                    .await;
            }
        }

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
                // Drive only after the durable turn, monitor and session lock
                // have settled. Earlier notifications race the busy check and
                // leave the goal idle with no later wake.
                let service = self.goal_handle();
                let outcome = match value.stop_reason.as_deref() {
                    Some("question_panel") => goal::TurnEndOutcome::WaitingUserInput,
                    Some("max_tokens") => {
                        if let Err(error) =
                            service.disarm(&self.storage, &user_id, &session_id).await
                        {
                            tracing::warn!(%error, %session_id, "publish disarmed goal projection failed");
                        }
                        goal::TurnEndOutcome::MaxTokens
                    }
                    _ => goal::TurnEndOutcome::Completed,
                };
                service.driver().notify_turn_ended(
                    &user_id,
                    &session_id,
                    goal_round.as_ref(),
                    outcome,
                );
                Ok(value)
            }
            Err(err) => {
                self.finish_request_error(
                    &user_id,
                    &session_id,
                    &emitter,
                    active_turn_id.as_deref(),
                    active_turn_round,
                    goal_round.clone(),
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
                self.goal_handle().driver().notify_turn_ended(
                    &user_id,
                    &session_id,
                    goal_round.as_ref(),
                    if err.code() == "CANCELLED" {
                        goal::TurnEndOutcome::Aborted
                    } else {
                        goal::TurnEndOutcome::Failed
                    },
                );
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod observation_budget_tests {
    use super::*;

    fn document_budgets(count: usize) -> ObservationTokenBudgets {
        resolve_observation_token_budgets(
            std::iter::repeat("read_file").take(count),
            128_000,
            1_000,
            Some(8_192),
        )
    }

    #[test]
    fn observation_budgets_allocate_the_whole_batch_before_rendering() {
        for (count, expected_each, expected_total) in [
            (1, 6_000, 6_000),
            (2, 4_000, 8_000),
            (20, 400, 8_000),
            // Protocol envelopes are mandatory, even beyond the soft content
            // ceiling. Do not omit a result to make this count appear bounded.
            (100, OBSERVATION_MIN_TOKEN_BUDGET, 25_600),
        ] {
            let budgets = document_budgets(count);
            assert_eq!(budgets.per_result.len(), count);
            assert!(budgets
                .per_result
                .iter()
                .all(|budget| *budget == expected_each));
            assert_eq!(budgets.per_result.iter().sum::<i64>(), expected_total);
            assert_eq!(budgets.total_tokens, expected_total);
            assert_eq!(
                budgets.protocol_reserve_tokens,
                count as i64 * OBSERVATION_MIN_TOKEN_BUDGET
            );
            assert!(
                budgets.total_tokens <= OBSERVATION_TURN_MAX_TOKENS
                    || budgets.total_tokens == budgets.protocol_reserve_tokens
            );
        }
    }

    #[test]
    fn observation_budgets_share_capacity_without_exceeding_tool_caps() {
        let budgets = resolve_observation_token_budgets(
            ["self_status", "read_file", "execute_command"],
            128_000,
            1_000,
            Some(8_192),
        );
        assert_eq!(budgets.per_result, [512, 3_744, 3_744]);
        assert_eq!(budgets.total_tokens, OBSERVATION_TURN_TARGET_TOKENS);

        let uneven = document_budgets(3);
        assert_eq!(uneven.per_result, [2_667, 2_667, 2_666]);
        let empty = document_budgets(0);
        assert!(empty.per_result.is_empty());
        assert_eq!(empty.total_tokens, 0);
        assert_eq!(empty.protocol_reserve_tokens, 0);
    }

    #[test]
    fn observation_budgets_respect_tighter_context_or_keep_only_protocol_reserve() {
        let tightened = resolve_observation_token_budgets(
            ["read_file", "read_file"],
            64_000,
            55_000,
            Some(1_024),
        );
        assert_eq!(tightened.available_context_tokens, 3_976);
        assert_eq!(tightened.per_result, [1_988, 1_988]);
        assert_eq!(tightened.total_tokens, 3_976);

        let below_protocol_reserve = resolve_observation_token_budgets(
            std::iter::repeat("read_file").take(20),
            64_000,
            58_000,
            Some(1_024),
        );
        assert_eq!(below_protocol_reserve.available_context_tokens, 976);
        assert_eq!(below_protocol_reserve.total_tokens, 5_120);
        assert_eq!(
            below_protocol_reserve.total_tokens,
            below_protocol_reserve.protocol_reserve_tokens
        );
        assert!(below_protocol_reserve
            .per_result
            .iter()
            .all(|budget| *budget == OBSERVATION_MIN_TOKEN_BUDGET));

        for observed_context in [63_000, 128_000, i64::MAX] {
            let exhausted = resolve_observation_token_budgets(
                std::iter::repeat("read_file").take(20),
                64_000,
                observed_context,
                Some(8_192),
            );
            assert_eq!(exhausted.available_context_tokens, 0);
            assert_eq!(exhausted.per_result.len(), 20);
            assert!(exhausted
                .per_result
                .iter()
                .all(|budget| *budget == OBSERVATION_MIN_TOKEN_BUDGET));
            assert_eq!(exhausted.total_tokens, exhausted.protocol_reserve_tokens);
        }
    }

    #[test]
    fn observation_budgets_fit_every_rendered_result_in_large_batches() {
        let result = ToolResultPayload::from_value(json!({
            "ok": true,
            "data": { "content": "\n\"\\界".repeat(4_096) },
        }));
        for count in [1, 2, 20, 100] {
            let budgets = document_budgets(count);
            let mut rendered_tokens = 0;
            for budget in budgets.per_result {
                let payload =
                    result.to_compact_payload_with_token_budget("read_file", Some(budget));
                let rendered = serde_json::to_string(&payload).expect("JSON observation");
                let tokens = approx_token_count(&rendered);
                assert!(tokens <= budget, "{tokens} tokens exceed budget {budget}");
                assert_eq!(payload["ok"], true);
                assert_eq!(payload["tool"], "read_file");
                rendered_tokens += tokens;
            }
            assert!(rendered_tokens <= budgets.total_tokens);
        }
    }
}
