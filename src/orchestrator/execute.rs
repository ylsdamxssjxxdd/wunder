use super::tool_calls::ToolCall;
use super::*;
use crate::core::approval::{
    ApprovalRequest, ApprovalRequestKind, ApprovalRequestTx, ApprovalResponse,
};

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

const DEFAULT_NON_ADMIN_MAX_ROUNDS: u32 = 8;
const MIN_NON_ADMIN_MAX_ROUNDS: u32 = 2;
const MIN_NON_ADMIN_MAX_ROUNDS_WITH_TOOLS: u32 = DEFAULT_NON_ADMIN_MAX_ROUNDS;
const MAX_CONTEXT_OVERFLOW_RECOVERY_ATTEMPTS: u32 = 4;
const MAX_REPEATED_TOOL_FAILURES: u32 = 3;
const TOOL_FAILURE_SIGNATURE_MAX_CHARS: usize = 240;

impl Orchestrator {
    pub(super) async fn execute_request(
        &self,
        prepared: PreparedRequest,
        emitter: EventEmitter,
    ) -> Result<WunderResponse, OrchestratorError> {
        let mut heartbeat_task: Option<JoinHandle<()>> = None;
        let mut acquired = false;
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

            // 心跳续租会话锁，避免长任务被误判超时。
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

            let user_round = self.monitor.register(
                &session_id,
                &user_id,
                prepared.agent_id.as_deref().unwrap_or(""),
                &question,
                is_admin,
                prepared.debug_payload,
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
                is_debug_log_level(&config.observability.log_level) || prepared.debug_payload;
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
            let allowed_tool_names = self.resolve_allowed_tool_names(
                &config,
                prepared.tool_names.as_deref().unwrap_or(&[]),
                &skills_snapshot,
                Some(&user_tool_bindings),
            );
            let tool_call_mode = normalize_tool_call_mode(llm_config.tool_call_mode.as_deref());
            let function_tooling =
                if tool_call_mode == ToolCallMode::FunctionCall && !prepared.skip_tool_calls {
                    self.build_function_tooling(
                        &config,
                        &skills_snapshot,
                        &allowed_tool_names,
                        Some(&user_tool_bindings),
                    )
                } else {
                    None
                };

            let mut system_prompt = self
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
                prepared.agent_prompt.as_deref(),
            )
                .await;
            system_prompt = self
                .append_memory_prompt(&user_id, system_prompt, is_admin)
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
            let mut a2ui_uid: Option<String> = None;
            let mut a2ui_messages: Option<Value> = None;
            let mut last_response: Option<(String, String)> = None;
            let mut last_request_messages: Option<Vec<Value>> = None;
            let mut last_round_info = request_round;

            let mut model_round = 0_i64;
            let mut repeated_tool_failure_signature = String::new();
            let mut repeated_tool_failure_count = 0_u32;
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
                        &session_id,
                        is_admin,
                        round_info,
                        messages,
                        &emitter,
                        &question,
                        log_payload,
                        false,
                        true,
                    )
                    .await?;
                self.ensure_not_cancelled(&session_id)?;
                messages = context_manager.normalize_messages(messages);
                let context_tokens = context_manager.estimate_context_tokens(&messages);
                self.workspace
                    .save_session_context_tokens_async(&user_id, &session_id, context_tokens)
                    .await;
                let mut context_payload = json!({
                    "context_tokens": context_tokens,
                    "message_count": messages.len(),
                });
                if let Value::Object(ref mut map) = context_payload {
                    round_info.insert_into(map);
                }
                emitter.emit("context_usage", context_payload).await;

                last_request_messages =
                    Some(self.sanitize_messages_for_log(
                        messages.clone(),
                        prepared.attachments.as_deref(),
                    ));

                let mut llm_call_payload = json!({
                    "stage": "llm_call",
                    "summary": i18n::t("monitor.summary.model_call"),
                });
                if let Value::Object(ref mut map) = llm_call_payload {
                    round_info.insert_into(map);
                }
                emitter.emit("progress", llm_call_payload).await;

                let tools_payload = function_tooling
                    .as_ref()
                    .map(|tooling| tooling.tools.as_slice());
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
                                    &session_id,
                                    is_admin,
                                    round_info,
                                    messages,
                                    &emitter,
                                    &question,
                                    log_payload,
                                    true,
                                    true,
                                )
                                .await?;
                            messages = context_manager.normalize_messages(messages);
                            let recovered_tokens = context_manager.estimate_context_tokens(&messages);
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
                            });
                            if let Value::Object(ref mut map) = compaction_payload {
                                round_info.insert_into(map);
                            }
                            emitter.emit("compaction", compaction_payload).await;
                        }
                        Err(err) => return Err(err),
                    }
                };
                if !user_message_appended {
                    let user_content = resolve_user_content_for_persist(&messages, &user_message);
                    self.append_chat(
                        &user_id,
                        &session_id,
                        "user",
                        user_content.as_ref(),
                        None,
                        None,
                        None,
                        None,
                    );
                    user_message_appended = true;
                }
                last_response = Some((content.clone(), reasoning.clone()));
                accumulate_usage(&mut round_usage, &usage);

                let tool_calls = if prepared.skip_tool_calls {
                    Vec::new()
                } else {
                    collect_tool_calls_from_output(
                        &content,
                        &reasoning,
                        tool_calls_payload.as_ref(),
                    )
                };
                let tool_calls = if let Some(tooling) = function_tooling.as_ref() {
                    apply_tool_name_map(tool_calls, &tooling.name_map)
                } else {
                    tool_calls
                };
                let planned_calls = build_planned_tool_calls(tool_calls);
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
                    workspace: self.workspace.clone(),
                    lsp_manager: self.lsp_manager.clone(),
                    config: &config,
                    a2a_store: &self.a2a_store,
                    skills: &skills_snapshot,
                    gateway: Some(self.gateway.clone()),
                    user_world: Some(self.user_world.clone()),
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
                    let safe_args = if args.is_object() {
                        args.clone()
                    } else {
                        json!({ "raw": args })
                    };
                    let event_args = if allowed_tool_names.contains(&planned.name) {
                        args.clone()
                    } else {
                        safe_args
                    };
                    let mut tool_payload = json!({ "tool": planned.name, "args": event_args });
                    if let Value::Object(ref mut map) = tool_payload {
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
                        let tool_call_id = if tool_call_mode == ToolCallMode::FunctionCall {
                            id.as_deref()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(ToString::to_string)
                        } else {
                            None
                        };
                        if tool_call_mode == ToolCallMode::FunctionCall {
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
                        self.append_chat(
                            &user_id,
                            &session_id,
                            "tool",
                            Some(&json!(observation)),
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
                            let mut workspace_payload = json!({
                                "workspace_id": prepared.workspace_id.clone(),
                                "agent_id": if agent_id.is_empty() { Value::Null } else { Value::String(agent_id) },
                                "tree_version": tree_version,
                                "tool": name,
                                "reason": "tool_result",
                            });
                            if let Value::Object(ref mut map) = workspace_payload {
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
                            if repeated_tool_failure_count >= MAX_REPEATED_TOOL_FAILURES {
                                answer = build_tool_failure_guard_answer(&name, &result);
                                stop_reason = Some("tool_failure_guard".to_string());
                                let mut guard_payload = json!({
                                    "stage": "tool_failure_guard",
                                    "summary": "Repeated tool failures detected; stopped retries to keep session alive.",
                                    "tool": name.clone(),
                                    "repeat_count": repeated_tool_failure_count,
                                    "threshold": MAX_REPEATED_TOOL_FAILURES,
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
                                    Some(&json!({
                                        "type": "tool_failure_guard",
                                        "tool": name.clone(),
                                        "repeat_count": repeated_tool_failure_count,
                                    })),
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
            if answer.is_empty() {
                let (fallback_answer, fallback_reason) =
                    resolve_empty_answer_fallback(reached_max_rounds);
                answer = fallback_answer;
                if stop_reason.is_none() {
                    stop_reason = Some(fallback_reason.to_string());
                }
            }

            self.enqueue_memory_summary(&prepared, last_request_messages, &answer)
                .await;

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
                last_round_info.insert_into(map);
            }
            emitter.emit("final", final_payload).await;
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
                let PlannedToolCall { call, name } = planned;
                let args = call.arguments.clone();
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
                result = orchestrator.finalize_tool_result(result, started_at, is_admin);
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

fn build_planned_tool_calls(calls: Vec<ToolCall>) -> Vec<PlannedToolCall> {
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
            call.name = resolved.clone();
            Some(PlannedToolCall {
                call,
                name: resolved,
            })
        })
        .collect()
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

fn resolve_empty_answer_fallback(reached_max_rounds: bool) -> (String, &'static str) {
    if reached_max_rounds {
        return (i18n::t("error.max_rounds_no_final_answer"), "max_rounds");
    }
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

fn build_tool_failure_guard_answer(tool_name: &str, result: &ToolResultPayload) -> String {
    let detail = result.error.trim();
    if detail.is_empty() {
        return format!(
            "Tool `{tool_name}` keeps failing repeatedly. I stopped retrying to keep this thread alive. Please adjust the request or tool args and retry."
        );
    }
    let clipped = detail
        .chars()
        .take(TOOL_FAILURE_SIGNATURE_MAX_CHARS)
        .collect::<String>();
    format!(
        "Tool `{tool_name}` failed repeatedly ({clipped}). I stopped retrying to keep this thread alive. Please adjust the request or tool args and retry."
    )
}

fn accumulate_usage(target: &mut TokenUsage, usage: &TokenUsage) {
    let total = usage.total.max(usage.input.saturating_add(usage.output));
    target.input = target.input.saturating_add(usage.input);
    target.output = target.output.saturating_add(usage.output);
    target.total = target.total.saturating_add(total);
}

fn approval_kind_for_tool(tool_name: &str) -> ApprovalRequestKind {
    let exec_tool = resolve_tool_name("execute_command");
    let ptc_tool = resolve_tool_name("ptc");
    if tool_name == exec_tool || tool_name == ptc_tool {
        ApprovalRequestKind::Exec
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
    }
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
    fn resolve_empty_answer_fallback_uses_max_rounds_reason() {
        let (answer, stop_reason) = resolve_empty_answer_fallback(true);
        assert!(!answer.trim().is_empty());
        assert_eq!(stop_reason, "max_rounds");
    }

    #[test]
    fn resolve_empty_answer_fallback_uses_empty_response_reason() {
        let (answer, stop_reason) = resolve_empty_answer_fallback(false);
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
        let signature = build_tool_failure_signature("执行命令", &result);
        assert!(signature.contains("执行命令"));
        assert!(signature.contains("command failed"));
    }

    #[test]
    fn tool_failure_guard_answer_contains_tool_name() {
        let result = ToolResultPayload {
            ok: false,
            data: json!({}),
            error: String::new(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        };
        let answer = build_tool_failure_guard_answer("读取文件", &result);
        assert!(answer.contains("读取文件"));
        assert!(answer.contains("stopped retrying"));
    }
}
