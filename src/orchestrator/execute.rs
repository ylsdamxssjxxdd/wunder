use super::tool_calls::ToolCall;
use super::*;
use crate::core::approval::{
    ApprovalRequest, ApprovalRequestKind, ApprovalRequestTx, ApprovalResponse,
};
use crate::services::chat_attachments::persist_user_chat_attachments;

struct PlannedToolCall {
    call: ToolCall,
    name: String,
}

struct ToolExecutionOutcome {
    call: ToolCall,
    name: String,
    result: ToolResultPayload,
}

enum TerminalTool {
    A2ui,
    Final,
}

const DEFAULT_NON_ADMIN_MAX_ROUNDS: u32 = 1000;
const MIN_NON_ADMIN_MAX_ROUNDS: u32 = 2;
const MIN_NON_ADMIN_MAX_ROUNDS_WITH_TOOLS: u32 = MIN_NON_ADMIN_MAX_ROUNDS;
const MAX_CONTEXT_OVERFLOW_RECOVERY_ATTEMPTS: u32 = 4;
const DEFAULT_REPEATED_TOOL_FAILURE_THRESHOLD: u32 = 5;
const TOOL_FAILURE_SIGNATURE_MAX_CHARS: usize = 240;
const WORKSPACE_UPDATE_MAX_CHANGED_PATHS: usize = 24;
const WORKSPACE_PATH_HINT_KEYS: [&str; 16] = [
    "path",
    "paths",
    "changed_paths",
    "changedPaths",
    "target_path",
    "targetPath",
    "source_path",
    "sourcePath",
    "destination",
    "destination_path",
    "destinationPath",
    "relative_path",
    "relativePath",
    "file",
    "files",
    "to_path",
];
const WORKSPACE_EVENT_NESTED_OBJECT_KEYS: [&str; 5] =
    ["data", "meta", "result", "output", "payload"];

fn should_enable_local_full_event_logs(server_mode: &str) -> bool {
    matches!(
        server_mode.trim().to_ascii_lowercase().as_str(),
        "desktop" | "cli"
    )
}

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
        let is_admin = prepared.is_admin;

        let result = async {
            let mut lock_agent_id = prepared.agent_id.clone().unwrap_or_default();
            if !is_admin {
                let storage = self.storage.clone();
                let lock_user = user_id.clone();
                let lock_session = session_id.clone();
                let lock_session_query = lock_session.clone();
                if let Ok(Ok(Some(record))) = tokio::task::spawn_blocking(move || {
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

            if prepared.stream && !is_admin {
                let cleanup_session = session_id.clone();
                let storage = self.storage.clone();
                match tokio::task::spawn_blocking(move || {
                    storage.delete_stream_events_by_session(&cleanup_session)
                })
                .await
                {
                    Ok(Ok(_)) => {}
                    Ok(Err(err)) => {
                        warn!("failed to clear stream events for session {session_id}: {err}");
                    }
                    Err(err) => {
                        warn!("failed to clear stream events for session {session_id}: {err}");
                    }
                }
            }
            // Keep renewing the session lock heartbeat for long-running requests.
            let heartbeat_limiter = limiter.clone();
            if acquired {
                let heartbeat_session = session_id.clone();
                heartbeat_task = Some(tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs_f64(
                            SESSION_LOCK_HEARTBEAT_S,
                        ))
                        .await;
                        heartbeat_limiter.touch(&heartbeat_session).await;
                    }
                }));
            }

            let local_full_event_logs =
                should_enable_local_full_event_logs(&request_config.server.mode);
            let monitor_debug_payload = prepared.debug_payload || local_full_event_logs;
            let user_round = self.monitor.register(
                &session_id,
                &user_id,
                prepared.agent_id.as_deref().unwrap_or(""),
                &question,
                is_admin,
                monitor_debug_payload,
            );
            let request_round = RoundInfo::user_only(user_round);
            let mut start_payload = json!({
                "stage": "start",
                "summary": i18n::t("monitor.summary.received"),
                "question": question
            });
            if let Value::Object(ref mut map) = start_payload {
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
            let tool_roots = crate::tools::build_tool_roots(
                &config,
                &skills_snapshot,
                Some(&user_tool_bindings),
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
            let tool_call_mode = crate::llm::resolve_tool_call_mode(&llm_config);
            let function_tooling = if uses_native_tool_api(tool_call_mode, &llm_config)
                && !prepared.skip_tool_calls
            {
                self.build_function_tooling(
                    &config,
                    &skills_snapshot,
                    &allowed_tool_names,
                    Some(&user_tool_bindings),
                    tool_call_mode,
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
                    Some(&question),
                    Some(user_round_id.as_str()),
                )
                .await;

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
                    user_id.clone(),
                    session_id.clone(),
                    history_limit,
                )
                .await;
            messages.extend(history_messages);
            let user_message = self.build_user_message(&question, prepared.attachments.as_deref());
            messages.push(user_message.clone());
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
            let mut round_usage = TokenUsage {
                input: 0,
                output: 0,
                total: 0,
            };
            let mut answer = String::new();
            let mut stop_reason: Option<String> = None;
            let mut stop_meta: Option<Value> = None;
            let mut a2ui_uid: Option<String> = None;
            let mut a2ui_messages: Option<Value> = None;
            let mut last_response: Option<(String, String)> = None;
            let mut last_round_info = request_round;

            let mut model_round = 0_i64;
            let mut repeated_tool_failure_signature = String::new();
            let mut repeated_tool_failure_count = 0_u32;
            let repeated_tool_failure_threshold =
                resolve_tool_failure_guard_threshold(&request_config);
            let tools_payload = function_tooling
                .as_ref()
                .map(|tooling| tooling.tools.as_slice());
            // Reserve prompt budget for native tool schema payloads; message-only estimates
            // undercount the real request size and can miss preemptive compaction.
            let request_overhead_tokens = estimate_request_overhead_tokens(tools_payload);
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
                self.ensure_not_cancelled(&session_id)?;
                messages = context_manager.normalize_messages(messages);
                messages = self
                    .maybe_compact_messages(
                        &config,
                        &llm_config,
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
                    )
                    .await?;
                if force_compaction_on_entry {
                    let _ = self
                        .workspace
                        .delete_session_context_overflow_async(&user_id, &session_id)
                        .await;
                    force_compaction_on_entry = false;
                }
                self.ensure_not_cancelled(&session_id)?;
                messages = context_manager.normalize_messages(messages);
                let context_tokens = context_manager.estimate_context_tokens(&messages);
                let projected_request_tokens =
                    context_tokens.saturating_add(request_overhead_tokens);
                self.workspace
                    .save_session_context_tokens_async(&user_id, &session_id, context_tokens)
                    .await;
                persisted_context_tokens = context_tokens;
                let mut context_payload = json!({
                    "context_tokens": context_tokens,
                    "persisted_context_tokens": persisted_context_tokens,
                    "projected_request_tokens": projected_request_tokens,
                    "request_overhead_tokens": request_overhead_tokens,
                    "message_count": messages.len(),
                });
                if let Value::Object(ref mut map) = context_payload {
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
                    let user_content = resolve_user_content_for_persist(&messages, &user_message);
                    self.append_chat(
                        &user_id,
                        &session_id,
                        "user",
                        user_content.as_ref(),
                        prepared.attachments.as_deref(),
                        None,
                        None,
                        None,
                        None,
                    );
                    user_message_appended = true;
                }

                let mut overflow_recovery_attempts = 0_u32;
                let (content, reasoning, usage, tool_calls_payload) = loop {
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

                            messages = self
                                .maybe_compact_messages(
                                    &config,
                                    &llm_config,
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
                                )
                                .await?;
                            let _ = self
                                .workspace
                                .delete_session_context_overflow_async(&user_id, &session_id)
                                .await;
                            messages = context_manager.normalize_messages(messages);
                            let recovered_tokens = context_manager.estimate_context_tokens(&messages);
                            let recovered_request_tokens =
                                recovered_tokens.saturating_add(request_overhead_tokens);
                            self.workspace
                                .save_session_context_tokens_async(
                                    &user_id,
                                    &session_id,
                                    recovered_tokens,
                                )
                                .await;
                            let mut compaction_payload = json!({
                                "reason": "overflow_recovery",
                                "status": "done",
                                "attempt": overflow_recovery_attempts,
                                "max_attempts": MAX_CONTEXT_OVERFLOW_RECOVERY_ATTEMPTS,
                                "context_tokens_after": recovered_tokens,
                                "projected_request_tokens_after": recovered_request_tokens,
                                "request_overhead_tokens": request_overhead_tokens,
                            });
                            if let Value::Object(ref mut map) = compaction_payload {
                                round_info.insert_into(map);
                            }
                            emitter.emit("compaction", compaction_payload).await;
                        }
                        Err(err) => {
                            if should_recover_from_context_overflow(&err) {
                                self.workspace
                                    .save_session_context_overflow_async(
                                        &user_id,
                                        &session_id,
                                        true,
                                    )
                                    .await;
                                let overflow_tokens = llm_config
                                    .max_context
                                    .map(i64::from)
                                    .unwrap_or_else(|| {
                                        context_manager
                                            .estimate_context_tokens(&messages)
                                            .saturating_add(request_overhead_tokens)
                                    });
                                self.workspace
                                    .save_session_context_tokens_async(
                                        &user_id,
                                        &session_id,
                                        overflow_tokens,
                                    )
                                    .await;
                            }
                            return Err(err);
                        }
                    }
                };
                last_response = Some((content.clone(), reasoning.clone()));
                accumulate_usage(&mut round_usage, &usage);

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
                    tool_calls
                };
                let planned_calls = build_planned_tool_calls(tool_calls, &allowed_tool_names);
                if planned_calls.is_empty() {
                    if prepared.skip_tool_calls {
                        answer = content.trim().to_string();
                    } else {
                        answer = self.resolve_final_answer(&content);
                    }
                    stop_reason = Some("model_response".to_string());
                    let assistant_content = if answer.is_empty() {
                        content.clone()
                    } else {
                        answer.clone()
                    };
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
                        );
                    }
                    if answer.is_empty() {
                        answer = content.trim().to_string();
                    }
                    break;
                }

                let assistant_content = content.clone();
                let assistant_reasoning = reasoning.clone();
                let has_tool_calls_payload = tool_calls_payload
                    .as_ref()
                    .is_some_and(|payload| !matches!(payload, Value::Null));
                if has_tool_calls_payload
                    || !assistant_content.trim().is_empty()
                    || !assistant_reasoning.trim().is_empty()
                {
                    let mut assistant_message = json!({
                        "role": "assistant",
                        "content": assistant_content.clone(),
                    });
                    if !assistant_reasoning.trim().is_empty() {
                        assistant_message["reasoning_content"] = json!(assistant_reasoning.clone());
                    }
                    if let Some(tool_calls_payload) = tool_calls_payload.clone() {
                        assistant_message["tool_calls"] = tool_calls_payload;
                    }
                    messages.push(assistant_message);
                    let meta = json!({ "type": "tool_call" });
                    self.append_chat(
                        &user_id,
                        &session_id,
                        "assistant",
                        Some(&json!(assistant_content)),
                        None,
                        Some(&meta),
                        Some(&assistant_reasoning),
                        tool_calls_payload.as_ref(),
                        None,
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
                            tokio::spawn(async move {
                                emitter.emit(&event_name, data).await;
                            });
                        }
                    },
                    prepared.stream,
                );

                let tool_context = ToolContext {
                    user_id: &user_id,
                    session_id: &session_id,
                    workspace_id: &prepared.workspace_id,
                    agent_id: prepared.agent_id.as_deref(),
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

                self.ensure_not_cancelled(&session_id)?;

                for planned in &exec_calls {
                    let args = &planned.call.arguments;
                    let tool_call_id = planned
                        .call
                        .id
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
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
                if !exec_calls.is_empty() {
                    let outcomes = self
                        .execute_tool_calls_parallel(
                            exec_calls,
                            &tool_context,
                            &allowed_tool_names,
                            &session_id,
                            is_admin,
                            &emitter,
                            prepared.approval_tx.clone(),
                        )
                        .await?;
                    for (index, outcome) in outcomes.into_iter().enumerate() {
                        let ToolExecutionOutcome { call, name, result } = outcome;
                        let ToolCall { id, arguments, .. } = call;
                        let args = arguments;
                        let question_panel_finished = name == question_panel_name && result.ok;
                        if question_panel_finished {
                            answer = i18n::t("response.question_panel_waiting");
                            stop_reason = Some("question_panel".to_string());
                            should_finish = true;
                        }

                        let observation = self.build_tool_observation(&name, &result);
                        let read_image_followup = if result.ok && is_read_image_tool_name(&name) {
                            match build_read_image_followup_user_message(&result.data).await {
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
                        let tool_call_id = if uses_native_tool_api(tool_call_mode, &llm_config) {
                            id.as_deref()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(ToString::to_string)
                        } else {
                            None
                        };
                        if uses_native_tool_api(tool_call_mode, &llm_config) {
                            if let Some(tool_call_id) = tool_call_id.as_ref() {
                                messages.push(json!({
                                    "role": "tool",
                                    "tool_call_id": tool_call_id,
                                    "content": observation.clone(),
                                }));
                            } else {
                                messages.push(json!({
                                    "role": "user",
                                    "content": format!("{OBSERVATION_PREFIX}{observation}"),
                                }));
                            }
                        } else {
                            messages.push(json!({
                                "role": "user",
                                "content": format!("{OBSERVATION_PREFIX}{observation}"),
                            }));
                        }
                        if let Some(followup_message) = read_image_followup {
                            messages.push(followup_message);
                        }
                        if let Some(followup_message) = desktop_followup {
                            messages.push(followup_message);
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
                            tool_call_id.as_deref(),
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
                            if let Some(tool_call_id) = tool_call_id.as_ref() {
                                map.insert(
                                    "tool_call_id".to_string(),
                                    Value::String(tool_call_id.clone()),
                                );
                            }
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
                            );
                        }

                        if result.ok {
                            repeated_tool_failure_signature.clear();
                            repeated_tool_failure_count = 0;
                        } else {
                            let signature = build_tool_failure_signature(&name, &result);
                            if signature == repeated_tool_failure_signature {
                                repeated_tool_failure_count =
                                    repeated_tool_failure_count.saturating_add(1);
                            } else {
                                repeated_tool_failure_signature = signature;
                                repeated_tool_failure_count = 1;
                            }
                            if repeated_tool_failure_count >= repeated_tool_failure_threshold {
                                answer = build_tool_failure_guard_answer(
                                    &name,
                                    &result,
                                    repeated_tool_failure_count,
                                    repeated_tool_failure_threshold,
                                );
                                stop_reason = Some("tool_failure_guard".to_string());
                                let guard_meta = json!({
                                    "type": "tool_failure_guard",
                                    "tool": name.clone(),
                                    "repeat_count": repeated_tool_failure_count,
                                    "threshold": repeated_tool_failure_threshold,
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
                                    "repeat_count": repeated_tool_failure_count,
                                    "threshold": repeated_tool_failure_threshold,
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
                                );
                                should_finish = true;
                                break;
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

                if !should_finish && answer.is_empty() {
                    if let Some((terminal_kind, terminal)) = terminal_call {
                        let name = terminal.name.clone();
                        let args = terminal.call.arguments.clone();
                        match terminal_kind {
                            TerminalTool::A2ui => {
                                let (uid, messages_payload, content) =
                                    self.resolve_a2ui_tool_payload(&args, &user_id, &session_id);
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
                                    );
                                }
                                should_finish = true;
                            }
                            TerminalTool::Final => {
                                answer = self.resolve_final_answer_from_tool(&args);
                                stop_reason = Some("final_tool".to_string());
                                self.log_final_tool_call(
                                    &user_id,
                                    &session_id,
                                    &name,
                                    &args,
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
                let (fallback_answer, fallback_reason) =
                    resolve_empty_answer_fallback();
                answer = fallback_answer;
                if stop_reason.is_none() {
                    stop_reason = Some(fallback_reason.to_string());
                }
            }

            let stop_reason = stop_reason.unwrap_or_else(|| "unknown".to_string());
            let waiting_question_panel = stop_reason == "question_panel";
            round_usage.total =
                round_usage
                    .total
                    .max(round_usage.input.saturating_add(round_usage.output));
            let has_round_usage =
                round_usage.total > 0 || round_usage.input > 0 || round_usage.output > 0;
            if has_round_usage {
                let mut usage_payload = json!({
                    "input_tokens": round_usage.input,
                    "output_tokens": round_usage.output,
                    "total_tokens": round_usage.total,
                });
                if let Value::Object(ref mut map) = usage_payload {
                    request_round.insert_into(map);
                }
                emitter.emit("round_usage", usage_payload).await;
            }

            let response_usage = if has_round_usage {
                Some(round_usage.clone())
            } else {
                None
            };
            let response = WunderResponse {
                session_id: session_id.clone(),
                answer: answer.clone(),
                usage: response_usage.clone(),
                stop_reason: Some(stop_reason.clone()),
                uid: a2ui_uid.clone(),
                a2ui: a2ui_messages.clone(),
            };
            let mut final_payload = json!({
                "answer": answer,
                "usage": response_usage.clone().unwrap_or(TokenUsage { input: 0, output: 0, total: 0 }),
                "round_usage": round_usage,
                "stop_reason": stop_reason
            });
            if let Value::Object(ref mut map) = final_payload {
                if let Some(meta) = stop_meta.clone() {
                    map.insert("stop_meta".to_string(), meta);
                }
                last_round_info.insert_into(map);
            }
            emitter.emit("final", final_payload).await;
            if !waiting_question_panel && !answer.trim().is_empty() {
                self.spawn_auto_memory_extraction(
                    &user_id,
                    prepared.agent_id.as_deref(),
                    &session_id,
                    Some(user_round_id.as_str()),
                    &question,
                    &answer,
                    llm_config.clone(),
                );
            }
            if waiting_question_panel {
                self.monitor.mark_question_panel(&session_id);
            } else {
                self.monitor.mark_finished(&session_id);
            }
            Ok(response)
        }
        .await;

        match result {
            Ok(value) => {
                emitter.finish().await;
                if acquired {
                    limiter.release(&session_id).await;
                }
                if let Some(handle) = heartbeat_task.take() {
                    handle.abort();
                }
                Ok(value)
            }
            Err(err) => {
                emitter.emit("error", err.to_payload()).await;
                if !matches!(err.code(), "USER_BUSY" | "CANCELLED") {
                    self.append_chat(
                        &user_id,
                        &session_id,
                        "assistant",
                        Some(&json!(err.message())),
                        None,
                        None,
                        None,
                        None,
                        None,
                    );
                }
                if err.code() == "CANCELLED" {
                    self.monitor.mark_cancelled(&session_id);
                } else if err.code() != "USER_BUSY" {
                    self.monitor.mark_error(&session_id, err.message());
                }
                emitter.finish().await;
                if acquired {
                    limiter.release(&session_id).await;
                }
                if let Some(handle) = heartbeat_task.take() {
                    handle.abort();
                }
                Err(err)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_tool_calls_parallel(
        &self,
        calls: Vec<PlannedToolCall>,
        tool_context: &ToolContext<'_>,
        allowed_tool_names: &HashSet<String>,
        session_id: &str,
        is_admin: bool,
        emitter: &EventEmitter,
        approval_tx: Option<ApprovalRequestTx>,
    ) -> Result<Vec<ToolExecutionOutcome>, OrchestratorError> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        let parallelism = resolve_tool_parallelism(calls.len());
        let mut stream = futures::stream::iter(calls.into_iter().map(|planned| {
            let orchestrator = self;
            let approval_tx = approval_tx.clone();
            let emitter = emitter.clone();
            async move {
                let PlannedToolCall { mut call, name } = planned;
                let recovered_args =
                    crate::core::tool_args::recover_tool_args_value_with_meta(&call.arguments);
                call.arguments = recovered_args.value.clone();
                let args = call.arguments.clone();
                let args_repair = recovered_args.repair.clone();
                let workspace_version_before =
                    tool_context.workspace.get_tree_version(tool_context.workspace_id);
                let policy_decision = crate::exec_policy::evaluate_tool_call(
                    tool_context.config,
                    &name,
                    &args,
                    Some(tool_context.session_id),
                    Some(tool_context.user_id),
                );
                let policy_meta = policy_decision.as_ref().map(|decision| decision.to_value());
                let started_at = Instant::now();
                let tool_timeout =
                    orchestrator.resolve_tool_timeout(tool_context.config, &name, &args, is_admin);
                let mut result = if !allowed_tool_names.contains(&name) {
                    ToolResultPayload::error(
                        i18n::t("error.tool_disabled_or_unavailable"),
                        json!({ "tool": name.clone() }),
                    )
                } else if let Some(decision) = policy_decision.as_ref() {
                    if !decision.allowed {
                        let mut approved = None;
                        let mut approval_id = None::<String>;
                        let mut approval_kind = None::<ApprovalRequestKind>;
                        let mut approval_summary = None::<String>;
                        if decision.requires_approval {
                            if let Some(tx) = approval_tx.clone() {
                                let (respond_to, response_rx) = tokio::sync::oneshot::channel();
                                let kind = approval_kind_for_tool(&name);
                                let summary = approval_summary_for_tool(&name, &args, kind);
                                let request_id = Uuid::new_v4().simple().to_string();
                                let detail = json!({
                                    "policy": policy_meta.clone().unwrap_or(Value::Null),
                                    "reason": decision.reason.clone(),
                                });
                                let request = ApprovalRequest {
                                    id: request_id.clone(),
                                    kind,
                                    tool: name.clone(),
                                    args: args.clone(),
                                    summary: summary.clone(),
                                    detail: detail.clone(),
                                    respond_to,
                                };
                                if tx.send(request).is_ok() {
                                    approval_id = Some(request_id.clone());
                                    approval_kind = Some(kind);
                                    approval_summary = Some(summary.clone());
                                    orchestrator
                                        .monitor
                                        .mark_approval_pending(session_id, Some(summary.as_str()));
                                    let mut event_payload = json!({
                                        "approval_id": request_id,
                                        "kind": kind,
                                        "tool": name.clone(),
                                        "summary": summary.clone(),
                                        "args": args.clone(),
                                        "detail": detail,
                                    });
                                    if let Value::Object(ref mut map) = event_payload {
                                        if let Some(meta) = policy_meta.clone() {
                                            map.insert("policy".to_string(), meta);
                                        }
                                    }
                                    emitter.emit("approval_request", event_payload).await;
                                    approved = tokio::select! {
                                        res = response_rx => res.ok(),
                                        err = orchestrator.wait_for_cancelled(session_id) => {
                                            return Err(err);
                                        }
                                    };
                                    orchestrator.monitor.mark_running(session_id, None);
                                }
                            }
                        }

                        let approval_response = approved.unwrap_or(ApprovalResponse::Deny);
                        if let Some(id) = approval_id {
                            let status = match approval_response {
                                ApprovalResponse::ApproveOnce | ApprovalResponse::ApproveSession => {
                                    "approved"
                                }
                                ApprovalResponse::Deny => "denied",
                            };
                            let scope = match approval_response {
                                ApprovalResponse::ApproveSession => "session",
                                ApprovalResponse::ApproveOnce => "once",
                                ApprovalResponse::Deny => "none",
                            };
                            emitter
                                .emit(
                                    "approval_result",
                                    json!({
                                        "approval_id": id,
                                        "status": status,
                                        "scope": scope,
                                        "kind": approval_kind,
                                        "tool": name.clone(),
                                        "summary": approval_summary.clone().unwrap_or_default(),
                                    }),
                                )
                                .await;
                        }

                        let approved = match approval_response {
                            ApprovalResponse::ApproveOnce => Some(ApprovalResponse::ApproveOnce),
                            ApprovalResponse::ApproveSession => {
                                let args_approved = args_with_approved_flag(&args);
                                let _ = crate::exec_policy::evaluate_tool_call(
                                    tool_context.config,
                                    &name,
                                    &args_approved,
                                    Some(tool_context.session_id),
                                    Some(tool_context.user_id),
                                );
                                Some(ApprovalResponse::ApproveSession)
                            }
                            ApprovalResponse::Deny => None,
                        };

                        if let Some(approval_choice) = approved {
                            let result = tokio::select! {
                                res = orchestrator.execute_tool_with_timeout(tool_context, &name, &args, tool_timeout) => res,
                                err = orchestrator.wait_for_cancelled(session_id) => {
                                    return Err(err);
                                }
                            };
                            let mut executed = match result {
                                Ok(value) => ToolResultPayload::from_value(value),
                                Err(err) => {
                                    let message = if err.to_string() == tool_exec::TOOL_TIMEOUT_ERROR {
                                        i18n::t_with_params(
                                            "error.tool_execution_failed",
                                            &HashMap::from([(
                                                "name".to_string(),
                                                format!("{name} timeout"),
                                            )]),
                                        )
                                    } else {
                                        err.to_string()
                                    };
                                    ToolResultPayload::error(message, json!({ "tool": name.clone() }))
                                }
                            };
                            if let Some(meta) = policy_meta.clone() {
                                executed.insert_meta("policy", meta);
                            }
                            executed.insert_meta(
                                "approval",
                                json!({
                                    "status": "approved",
                                    "scope": if approval_choice == ApprovalResponse::ApproveSession {
                                        "session"
                                    } else {
                                        "once"
                                    }
                                }),
                            );
                            executed
                        } else {
                            let mut denied = ToolResultPayload::error(
                                i18n::t("tool.exec.not_allowed"),
                                json!({ "tool": name.clone() }),
                            );
                            if let Some(meta) = policy_meta.clone() {
                                denied.insert_meta("policy", meta);
                            }
                            denied
                        }
                    } else {
                        let result = tokio::select! {
                            res = orchestrator.execute_tool_with_timeout(tool_context, &name, &args, tool_timeout) => res,
                            err = orchestrator.wait_for_cancelled(session_id) => {
                                return Err(err);
                            }
                        };
                        let mut executed = match result {
                            Ok(value) => ToolResultPayload::from_value(value),
                            Err(err) => {
                                let message = if err.to_string() == tool_exec::TOOL_TIMEOUT_ERROR {
                                    i18n::t_with_params(
                                        "error.tool_execution_failed",
                                        &HashMap::from([(
                                            "name".to_string(),
                                            format!("{name} timeout"),
                                        )]),
                                    )
                                } else {
                                    err.to_string()
                                };
                                ToolResultPayload::error(message, json!({ "tool": name.clone() }))
                            }
                        };
                        if let Some(meta) = policy_meta.clone() {
                            executed.insert_meta("policy", meta);
                        }
                        executed
                    }
                } else {
                    let result = tokio::select! {
                        res = orchestrator.execute_tool_with_timeout(tool_context, &name, &args, tool_timeout) => res,
                        err = orchestrator.wait_for_cancelled(session_id) => {
                            return Err(err);
                        }
                    };
                    match result {
                        Ok(value) => ToolResultPayload::from_value(value),
                        Err(err) => {
                            let message = if err.to_string() == tool_exec::TOOL_TIMEOUT_ERROR {
                                i18n::t_with_params(
                                    "error.tool_execution_failed",
                                    &HashMap::from([(
                                        "name".to_string(),
                                        format!("{name} timeout"),
                                    )]),
                                )
                            } else {
                                err.to_string()
                            };
                            ToolResultPayload::error(message, json!({ "tool": name.clone() }))
                        }
                    }
                };
                let workspace_version_after =
                    tool_context.workspace.get_tree_version(tool_context.workspace_id);
                if workspace_version_after > workspace_version_before {
                    result.insert_meta("workspace_version", json!(workspace_version_after));
                    result.insert_meta("workspace_changed", Value::Bool(true));
                }
                if let Some(repair) = args_repair {
                    result.insert_meta("repair", repair);
                }
                result = orchestrator.finalize_tool_result(&name, result, started_at, is_admin);
                Ok(ToolExecutionOutcome { call, name, result })
            }
        }))
        .buffered(parallelism);

        let mut outcomes = Vec::new();
        while let Some(outcome) = stream.next().await {
            outcomes.push(outcome?);
        }
        Ok(outcomes)
    }
}

fn build_planned_tool_calls(
    calls: Vec<ToolCall>,
    allowed_tool_names: &HashSet<String>,
) -> Vec<PlannedToolCall> {
    calls
        .into_iter()
        .filter_map(|mut call| {
            let name = call.name.trim();
            if name.is_empty() {
                return None;
            }
            let resolved = resolve_tool_name(name);
            if resolved.trim().is_empty() {
                return None;
            }
            if !allowed_tool_names.contains(&resolved) && !allowed_tool_names.contains(name) {
                return None;
            }
            call.name = resolved.clone();
            Some(PlannedToolCall {
                call,
                name: resolved,
            })
        })
        .collect()
}

fn uses_native_tool_api(tool_call_mode: ToolCallMode, llm_config: &LlmModelConfig) -> bool {
    match tool_call_mode {
        ToolCallMode::FunctionCall => true,
        ToolCallMode::FreeformCall => matches!(
            crate::llm::resolve_openai_api_mode(llm_config),
            crate::llm::OpenAiApiMode::Responses
        ),
        ToolCallMode::ToolCall => false,
    }
}

fn resolve_tool_parallelism(total: usize) -> usize {
    let desired = DEFAULT_TOOL_PARALLELISM.max(1);
    total.max(1).min(desired)
}

fn resolve_non_admin_max_rounds(llm_config: &LlmModelConfig, skip_tool_calls: bool) -> i64 {
    let configured = llm_config
        .max_rounds
        .unwrap_or(DEFAULT_NON_ADMIN_MAX_ROUNDS);
    let minimum = if skip_tool_calls {
        MIN_NON_ADMIN_MAX_ROUNDS
    } else {
        MIN_NON_ADMIN_MAX_ROUNDS_WITH_TOOLS
    };
    i64::from(configured.max(minimum))
}

fn should_recover_from_context_overflow(err: &OrchestratorError) -> bool {
    err.code() == "CONTEXT_WINDOW_EXCEEDED"
        || super::llm::is_context_window_error_text(err.message())
}

fn estimate_request_overhead_tokens(tools: Option<&[Value]>) -> i64 {
    let Some(tools) = tools else {
        return 0;
    };
    if tools.is_empty() {
        return 0;
    }
    let payload = serde_json::to_string(tools).unwrap_or_default();
    approx_token_count(&payload).max(0)
}

fn resolve_user_content_for_persist(
    messages: &[Value],
    fallback_user_message: &Value,
) -> Option<Value> {
    if let Some(index) = Orchestrator::locate_current_user_index(messages) {
        if let Some(content) = messages
            .get(index)
            .and_then(|message| message.get("content"))
        {
            return Some(content.clone());
        }
    }
    fallback_user_message.get("content").cloned()
}

fn build_max_rounds_user_guidance(max_rounds: Option<i64>) -> String {
    let mut params = HashMap::new();
    params.insert(
        "max_rounds".to_string(),
        max_rounds.unwrap_or_default().max(0).to_string(),
    );
    i18n::t_with_params("error.max_rounds_user_guidance", &params)
}

fn resolve_tool_failure_guard_threshold(config: &Config) -> u32 {
    let threshold = u32::try_from(config.server.tool_failure_guard_threshold)
        .unwrap_or(DEFAULT_REPEATED_TOOL_FAILURE_THRESHOLD);
    threshold.max(1)
}

fn resolve_empty_answer_fallback() -> (String, &'static str) {
    (i18n::t("error.empty_no_final_answer"), "empty_response")
}

fn build_tool_failure_signature(tool_name: &str, result: &ToolResultPayload) -> String {
    let detail = if !result.error.trim().is_empty() {
        result.error.trim().to_string()
    } else {
        serde_json::to_string(&result.data).unwrap_or_default()
    };
    let normalized = detail
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    let clipped = normalized
        .chars()
        .take(TOOL_FAILURE_SIGNATURE_MAX_CHARS)
        .collect::<String>();
    format!("{tool_name}|{clipped}")
}

fn build_tool_failure_guard_answer(
    tool_name: &str,
    result: &ToolResultPayload,
    repeat_count: u32,
    threshold: u32,
) -> String {
    let mut params = HashMap::new();
    params.insert("tool_name".to_string(), tool_name.to_string());
    params.insert("repeat_count".to_string(), repeat_count.to_string());
    params.insert("threshold".to_string(), threshold.to_string());
    let detail = result.error.trim();
    if detail.is_empty() {
        return i18n::t_with_params("error.tool_failure_guard_user_guidance", &params);
    }
    let clipped = detail
        .chars()
        .take(TOOL_FAILURE_SIGNATURE_MAX_CHARS)
        .collect::<String>();
    params.insert("detail".to_string(), clipped);
    i18n::t_with_params("error.tool_failure_guard_user_guidance_with_error", &params)
}

fn accumulate_usage(target: &mut TokenUsage, usage: &TokenUsage) {
    let total = usage.total.max(usage.input.saturating_add(usage.output));
    target.input = target.input.saturating_add(usage.input);
    target.output = target.output.saturating_add(usage.output);
    target.total = target.total.saturating_add(total);
}

fn extract_workspace_changed_paths(
    meta: Option<&Value>,
    data: &Value,
    args: &Value,
    workspace_id: &str,
) -> Vec<String> {
    let mut output = Vec::new();
    if let Some(meta_obj) = meta.and_then(Value::as_object) {
        collect_workspace_paths_from_object(meta_obj, workspace_id, &mut output);
    }
    if let Some(data_obj) = data.as_object() {
        collect_workspace_paths_from_object(data_obj, workspace_id, &mut output);
    }
    if let Some(args_obj) = args.as_object() {
        collect_workspace_paths_from_object(args_obj, workspace_id, &mut output);
    }
    output
}

fn collect_workspace_paths_from_object(
    source: &Map<String, Value>,
    workspace_id: &str,
    output: &mut Vec<String>,
) {
    for key in WORKSPACE_PATH_HINT_KEYS {
        if output.len() >= WORKSPACE_UPDATE_MAX_CHANGED_PATHS {
            return;
        }
        if let Some(value) = source.get(key) {
            collect_workspace_paths_from_value(value, workspace_id, output);
        }
    }
    for key in WORKSPACE_EVENT_NESTED_OBJECT_KEYS {
        if output.len() >= WORKSPACE_UPDATE_MAX_CHANGED_PATHS {
            return;
        }
        if let Some(value) = source.get(key) {
            collect_workspace_paths_from_value(value, workspace_id, output);
        }
    }
}

fn collect_workspace_paths_from_value(value: &Value, workspace_id: &str, output: &mut Vec<String>) {
    if output.len() >= WORKSPACE_UPDATE_MAX_CHANGED_PATHS {
        return;
    }
    match value {
        Value::String(text) => push_workspace_changed_path(text, workspace_id, output),
        Value::Array(items) => {
            for item in items {
                if output.len() >= WORKSPACE_UPDATE_MAX_CHANGED_PATHS {
                    break;
                }
                collect_workspace_paths_from_value(item, workspace_id, output);
            }
        }
        Value::Object(map) => collect_workspace_paths_from_object(map, workspace_id, output),
        _ => {}
    }
}

fn push_workspace_changed_path(raw: &str, workspace_id: &str, output: &mut Vec<String>) {
    let Some(normalized) = normalize_workspace_changed_path(raw, workspace_id) else {
        return;
    };
    if output.iter().any(|existing| existing == &normalized) {
        return;
    }
    output.push(normalized);
}

fn normalize_workspace_changed_path(raw: &str, workspace_id: &str) -> Option<String> {
    let mut value = raw.trim().replace('\\', "/");
    if value.is_empty() {
        return None;
    }
    if let Some(stripped) = value.strip_prefix("file://") {
        value = stripped.to_string();
    }
    if let Some(index) = value.find(['?', '#']) {
        value.truncate(index);
    }
    if value == "/" || value == "." {
        return Some(String::new());
    }
    if value.len() >= 2 && value.as_bytes()[1] == b':' {
        // Ignore absolute Windows drive paths because they are not stable client hints.
        return None;
    }
    if let Some(stripped) = value.strip_prefix("/workspaces/") {
        value = stripped.to_string();
    }
    if let Some(stripped) = value.strip_prefix("workspaces/") {
        value = stripped.to_string();
        let mut parts = value.splitn(2, '/');
        let owner = parts.next().unwrap_or_default().trim();
        let rest = parts.next().unwrap_or_default();
        if owner == workspace_id {
            value = rest.to_string();
        } else if !owner.is_empty() {
            return None;
        }
    } else if let Some(stripped) = value.strip_prefix("/workspace/") {
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
    if let Some(stripped) = value.strip_prefix(&format!("{workspace_id}/")) {
        value = stripped.to_string();
    }
    if value == workspace_id || value == "." || value == "/" {
        return Some(String::new());
    }
    Some(value.trim_matches('/').to_string())
}

fn extract_container_id_from_workspace_id(workspace_id: &str) -> i32 {
    if let Some((_, suffix)) = workspace_id.rsplit_once("__c__") {
        if let Ok(parsed) = suffix.parse::<i32>() {
            return crate::storage::normalize_workspace_container_id(parsed);
        }
    }
    if workspace_id.contains("__a__") || workspace_id.contains("__agent__") {
        return crate::storage::DEFAULT_SANDBOX_CONTAINER_ID;
    }
    crate::storage::USER_PRIVATE_CONTAINER_ID
}

fn approval_kind_for_tool(tool_name: &str) -> ApprovalRequestKind {
    let exec_tool = resolve_tool_name("execute_command");
    let ptc_tool = resolve_tool_name("ptc");
    let controller_tool = resolve_tool_name("desktop_controller");
    let monitor_tool = resolve_tool_name("desktop_monitor");
    if tool_name == exec_tool || tool_name == ptc_tool {
        ApprovalRequestKind::Exec
    } else if tool_name == controller_tool || tool_name == monitor_tool {
        ApprovalRequestKind::Control
    } else {
        ApprovalRequestKind::Patch
    }
}

fn approval_summary_for_tool(tool_name: &str, args: &Value, kind: ApprovalRequestKind) -> String {
    match kind {
        ApprovalRequestKind::Exec => extract_command_text(args)
            .map(|cmd| format!("{tool_name}: {cmd}"))
            .unwrap_or_else(|| tool_name.to_string()),
        ApprovalRequestKind::Patch => args
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|path| format!("{tool_name}: {path}"))
            .unwrap_or_else(|| tool_name.to_string()),
        ApprovalRequestKind::Control => extract_control_summary(args)
            .map(|summary| format!("{tool_name}: {summary}"))
            .unwrap_or_else(|| tool_name.to_string()),
    }
}

fn extract_control_summary(args: &Value) -> Option<String> {
    let obj = args.as_object()?;
    let action = obj
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(action) = action {
        let desc = obj
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(desc) = desc {
            return Some(format!("action={action} {desc}"));
        }
        return Some(format!("action={action}"));
    }
    if let Some(wait_ms) = obj.get("wait_ms") {
        if let Some(value) = wait_ms
            .as_i64()
            .or_else(|| wait_ms.as_u64().map(|v| v as i64))
        {
            return Some(format!("wait_ms={value}"));
        }
    }
    None
}

fn extract_command_text(args: &Value) -> Option<String> {
    let obj = args.as_object()?;
    for key in ["content", "command", "cmd"] {
        if let Some(Value::String(text)) = obj.get(key) {
            let cleaned = text.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

fn args_with_approved_flag(args: &Value) -> Value {
    if let Some(obj) = args.as_object() {
        let mut updated = obj.clone();
        updated.insert("approved".to_string(), Value::Bool(true));
        return Value::Object(updated);
    }
    json!({ "raw": args, "approved": true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_max_rounds_user_guidance_encourages_continue_or_raise_limit() {
        let answer = build_max_rounds_user_guidance(Some(10));
        assert!(!answer.trim().is_empty());
        assert!(answer.contains("10"));
        assert!(answer.contains("缁х画") || answer.to_ascii_lowercase().contains("continue"));
    }

    #[test]
    fn resolve_empty_answer_fallback_uses_empty_response_reason() {
        let (answer, stop_reason) = resolve_empty_answer_fallback();
        assert!(!answer.trim().is_empty());
        assert_eq!(stop_reason, "empty_response");
    }

    #[test]
    fn recover_from_context_overflow_when_code_matches() {
        let err = OrchestratorError::context_window_exceeded("context length exceeded".to_string());
        assert!(should_recover_from_context_overflow(&err));
    }

    #[test]
    fn recover_from_context_overflow_when_message_matches() {
        let err =
            OrchestratorError::internal("LLM call failed: context_length_exceeded".to_string());
        assert!(should_recover_from_context_overflow(&err));
    }

    #[test]
    fn skip_context_overflow_recovery_for_other_errors() {
        let err = OrchestratorError::internal("LLM call failed: invalid api key".to_string());
        assert!(!should_recover_from_context_overflow(&err));
    }

    #[test]
    fn estimate_request_overhead_tokens_counts_tool_schema_payload() {
        let tools = vec![json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a file from the workspace and return the content.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file"
                        }
                    },
                    "required": ["path"]
                }
            }
        })];
        assert!(estimate_request_overhead_tokens(Some(&tools)) > 0);
        assert_eq!(estimate_request_overhead_tokens(None), 0);
    }

    #[test]
    fn resolve_user_content_for_persist_prefers_trimmed_message_in_context() {
        let messages = vec![
            json!({ "role": "system", "content": "system" }),
            json!({ "role": "user", "content": "trimmed question ...(truncated)" }),
        ];
        let fallback = json!({ "role": "user", "content": "raw giant question" });
        let content = resolve_user_content_for_persist(&messages, &fallback)
            .and_then(|value| value.as_str().map(ToString::to_string))
            .unwrap_or_default();
        assert_eq!(content, "trimmed question ...(truncated)");
    }

    #[test]
    fn resolve_user_content_for_persist_falls_back_to_original_message() {
        let messages = vec![json!({ "role": "assistant", "content": "done" })];
        let fallback = json!({ "role": "user", "content": "raw question" });
        let content = resolve_user_content_for_persist(&messages, &fallback)
            .and_then(|value| value.as_str().map(ToString::to_string))
            .unwrap_or_default();
        assert_eq!(content, "raw question");
    }

    #[test]
    fn tool_failure_signature_prefers_error_text() {
        let result = ToolResultPayload {
            ok: false,
            data: json!({"stderr":"ignored"}),
            error: "command failed".to_string(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        };
        let signature = build_tool_failure_signature("鎵ц鍛戒护", &result);
        assert!(signature.contains("鎵ц鍛戒护"));
        assert!(signature.contains("command failed"));
    }

    #[test]
    fn tool_failure_guard_answer_encourages_continue_from_current_progress() {
        let result = ToolResultPayload {
            ok: false,
            data: json!({}),
            error: String::new(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        };
        let answer = build_tool_failure_guard_answer("read_file", &result, 3, 5);
        assert!(answer.contains("read_file"));
        assert!(answer.contains("5"));
        assert!(answer.contains("缁х画") || answer.to_ascii_lowercase().contains("continue"));
    }

    #[test]
    fn normalize_workspace_changed_path_strips_workspace_public_prefix() {
        let path = normalize_workspace_changed_path(
            "/workspaces/alice__c__2/docs/readme.md",
            "alice__c__2",
        )
        .expect("path");
        assert_eq!(path, "docs/readme.md");
    }

    #[test]
    fn normalize_workspace_changed_path_ignores_windows_absolute_path() {
        let path = normalize_workspace_changed_path("C:/repo/demo.txt", "alice__c__2");
        assert!(path.is_none());
    }

    #[test]
    fn extract_workspace_changed_paths_merges_meta_data_and_args() {
        let meta = json!({
            "changed_paths": [
                "/workspaces/alice__c__2/docs/a.md",
                "docs/b.md"
            ]
        });
        let data = json!({
            "files": [
                { "path": "docs/c.md" },
                { "to_path": "docs/d.md" }
            ]
        });
        let args = json!({
            "destination": "docs/archive",
            "paths": ["docs/e.md"]
        });
        let paths = extract_workspace_changed_paths(Some(&meta), &data, &args, "alice__c__2");
        let expected = HashSet::from([
            "docs/a.md".to_string(),
            "docs/b.md".to_string(),
            "docs/c.md".to_string(),
            "docs/d.md".to_string(),
            "docs/e.md".to_string(),
            "docs/archive".to_string(),
        ]);
        assert_eq!(paths.len(), expected.len());
        let actual = paths.into_iter().collect::<HashSet<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn extract_container_id_from_workspace_id_recovers_suffix() {
        assert_eq!(
            extract_container_id_from_workspace_id("alice__c__7"),
            crate::storage::normalize_workspace_container_id(7)
        );
        assert_eq!(
            extract_container_id_from_workspace_id("alice__agent__demo"),
            crate::storage::DEFAULT_SANDBOX_CONTAINER_ID
        );
        assert_eq!(
            extract_container_id_from_workspace_id("alice"),
            crate::storage::USER_PRIVATE_CONTAINER_ID
        );
    }

    #[test]
    fn build_planned_tool_calls_filters_disallowed_name() {
        let allowed = HashSet::from([resolve_tool_name("read_file")]);
        let calls = vec![
            ToolCall {
                id: None,
                name: "read_file".to_string(),
                arguments: json!({ "path": "Cargo.toml" }),
            },
            ToolCall {
                id: None,
                name: "2026-03-03".to_string(),
                arguments: json!({ "timestamp": "..." }),
            },
        ];
        let planned = build_planned_tool_calls(calls, &allowed);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].name, resolve_tool_name("read_file"));
    }

    #[test]
    fn build_planned_tool_calls_accepts_allowed_alias() {
        let allowed = HashSet::from([resolve_tool_name("final_response")]);
        let calls = vec![ToolCall {
            id: None,
            name: "final_response".to_string(),
            arguments: json!({ "content": "ok" }),
        }];
        let planned = build_planned_tool_calls(calls, &allowed);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].name, resolve_tool_name("final_response"));
    }

    #[test]
    fn uses_native_tool_api_supports_freeform_only_on_responses_api() {
        let base_config = || LlmModelConfig {
            enable: None,
            provider: None,
            api_mode: None,
            base_url: None,
            api_key: None,
            model: None,
            temperature: None,
            timeout_s: None,
            retry: None,
            max_rounds: None,
            max_context: None,
            max_output: None,
            support_vision: None,
            support_hearing: None,
            stream: None,
            stream_include_usage: None,
            history_compaction_ratio: None,
            history_compaction_reset: None,
            tool_call_mode: None,
            model_type: None,
            stop: None,
            mock_if_unconfigured: None,
        };
        let mut function_call_config = base_config();
        function_call_config.tool_call_mode = Some("function_call".to_string());

        let mut freeform_responses_config = base_config();
        freeform_responses_config.provider = Some("openai".to_string());
        freeform_responses_config.model = Some("gpt-5.2".to_string());
        freeform_responses_config.tool_call_mode = Some("freeform_call".to_string());

        let mut freeform_chat_config = base_config();
        freeform_chat_config.provider = Some("openai_compatible".to_string());
        freeform_chat_config.model = Some("gpt-5.2".to_string());
        freeform_chat_config.tool_call_mode = Some("freeform_call".to_string());

        assert!(uses_native_tool_api(
            ToolCallMode::FunctionCall,
            &function_call_config,
        ));
        assert!(!uses_native_tool_api(
            ToolCallMode::ToolCall,
            &function_call_config
        ));
        assert!(uses_native_tool_api(
            ToolCallMode::FreeformCall,
            &freeform_responses_config,
        ));
        assert!(!uses_native_tool_api(
            ToolCallMode::FreeformCall,
            &freeform_chat_config,
        ));
    }
}
