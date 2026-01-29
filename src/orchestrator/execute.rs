use super::tool_calls::ToolCall;
use super::*;

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

impl Orchestrator {
    pub(super) async fn execute_request(
        &self,
        prepared: PreparedRequest,
        emitter: EventEmitter,
    ) -> Result<WunderResponse, OrchestratorError> {
        let mut heartbeat_task: Option<JoinHandle<()>> = None;
        let mut acquired = false;
        let limiter = RequestLimiter::new(
            self.storage.clone(),
            self.config_store.get().await.server.max_active_sessions,
        );
        let session_id = prepared.session_id.clone();
        let user_id = prepared.user_id.clone();
        let question = prepared.question.clone();

        let result = async {
            let ok = limiter
                .acquire(
                    &session_id,
                    &user_id,
                    prepared.agent_id.as_deref().unwrap_or(""),
                )
                .await
                .map_err(|err| OrchestratorError::internal(err.to_string()))?;
            if !ok {
                return Err(OrchestratorError::user_busy(i18n::t(
                    "error.user_session_busy",
                )));
            }
            acquired = true;

            if prepared.stream {
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

            let user_round = self.monitor.register(&session_id, &user_id, &question);
            let request_round = RoundInfo::user_only(user_round);
            let mut start_payload = json!({
                "stage": "start",
                "summary": i18n::t("monitor.summary.received")
            });
            if let Value::Object(ref mut map) = start_payload {
                request_round.insert_into(map);
            }
            emitter.emit("progress", start_payload).await;

            let config = self
                .resolve_config(prepared.config_overrides.as_ref())
                .await;
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
            system_prompt = self.append_memory_prompt(&user_id, system_prompt).await;

            let history_manager = HistoryManager;
            let context_manager = ContextManager;
            let mut messages = vec![json!({ "role": "system", "content": system_prompt })];
            let history_messages = history_manager
                .load_history_messages_async(
                    self.workspace.clone(),
                    user_id.clone(),
                    session_id.clone(),
                    config.workspace.max_history_items,
                )
                .await;
            messages.extend(history_messages);
            let user_message = self.build_user_message(&question, prepared.attachments.as_deref());
            messages.push(user_message.clone());
            self.append_chat(
                &user_id,
                &session_id,
                "user",
                user_message.get("content"),
                None,
                None,
                None,
                None,
            );

            let max_rounds = llm_config.max_rounds.unwrap_or(1).max(1) as i64;
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

            for model_round in 1..=max_rounds {
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
                        round_info,
                        messages,
                        &emitter,
                        &question,
                        log_payload,
                        false,
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
                let (content, reasoning, usage, tool_calls_payload) = self
                    .call_llm(
                        &llm_config,
                        &messages,
                        &user_id,
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
                    .await?;
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
                if !assistant_content.trim().is_empty() || !assistant_reasoning.trim().is_empty() {
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
                        let round_info = round_info;
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
                    workspace: self.workspace.clone(),
                    lsp_manager: self.lsp_manager.clone(),
                    config: &config,
                    a2a_store: &self.a2a_store,
                    skills: &skills_snapshot,
                    user_tool_manager: Some(self.user_tool_manager.as_ref()),
                    user_tool_bindings: Some(&user_tool_bindings),
                    user_tool_store: Some(self.user_tool_manager.store()),
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
                    if stop_reason.is_none() {
                        stop_reason = Some("max_rounds".to_string());
                    }
                }
            }
            if answer.is_empty() {
                answer = i18n::t("error.max_rounds_no_final_answer");
                if stop_reason.is_none() {
                    stop_reason = Some("max_rounds".to_string());
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

    async fn execute_tool_calls_parallel(
        &self,
        calls: Vec<PlannedToolCall>,
        tool_context: &ToolContext<'_>,
        allowed_tool_names: &HashSet<String>,
        session_id: &str,
    ) -> Result<Vec<ToolExecutionOutcome>, OrchestratorError> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        let parallelism = resolve_tool_parallelism(calls.len());
        let mut stream = futures::stream::iter(calls.into_iter().map(|planned| {
            let orchestrator = self;
            let tool_context = tool_context;
            let allowed_tool_names = allowed_tool_names;
            async move {
                let PlannedToolCall { call, name } = planned;
                let args = call.arguments.clone();
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
                    orchestrator.resolve_tool_timeout(tool_context.config, &name, &args);
                let mut result = if !allowed_tool_names.contains(&name) {
                    ToolResultPayload::error(
                        i18n::t("error.tool_disabled_or_unavailable"),
                        json!({ "tool": name.clone() }),
                    )
                } else if let Some(decision) = policy_decision.as_ref() {
                    if !decision.allowed {
                        let mut denied = ToolResultPayload::error(
                            i18n::t("tool.exec.not_allowed"),
                            json!({ "tool": name.clone() }),
                        );
                        if let Some(meta) = policy_meta.clone() {
                            denied.insert_meta("policy", meta);
                        }
                        denied
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
                result = orchestrator.finalize_tool_result(result, started_at);
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

fn accumulate_usage(target: &mut TokenUsage, usage: &TokenUsage) {
    let total = usage.total.max(usage.input.saturating_add(usage.output));
    target.input = target.input.saturating_add(usage.input);
    target.output = target.output.saturating_add(usage.output);
    target.total = target.total.saturating_add(total);
}
