use super::*;

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
                .acquire(&session_id, &user_id)
                .await
                .map_err(|err| OrchestratorError::internal(err.to_string()))?;
            if !ok {
                return Err(OrchestratorError::user_busy(i18n::t("error.user_session_busy")));
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
                        warn!(
                            "failed to clear stream events for session {session_id}: {err}"
                        );
                    }
                    Err(err) => {
                        warn!(
                            "failed to clear stream events for session {session_id}: {err}"
                        );
                    }
                }
            }

            // 心跳续租会话锁，避免长任务被误判超时。
            let heartbeat_limiter = limiter.clone();
            let heartbeat_session = session_id.clone();
            heartbeat_task = Some(tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs_f64(SESSION_LOCK_HEARTBEAT_S)).await;
                    heartbeat_limiter.touch(&heartbeat_session).await;
                }
            }));

            self.monitor.register(&session_id, &user_id, &question);
            emitter
                .emit(
                    "progress",
                    json!({
                        "stage": "start",
                        "summary": i18n::t("monitor.summary.received")
                    }),
                )
                .await;

            let config = self.resolve_config(prepared.config_overrides.as_ref()).await;
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
            let user_tool_bindings = self
                .user_tool_manager
                .build_bindings(&config, &skills_snapshot, &user_id);
            let tool_roots =
                crate::tools::build_tool_roots(&config, &skills_snapshot, Some(&user_tool_bindings));
            let allowed_tool_names = self.resolve_allowed_tool_names(
                &config,
                prepared.tool_names.as_deref().unwrap_or(&[]),
                &skills_snapshot,
                Some(&user_tool_bindings),
            );
            let tool_call_mode = normalize_tool_call_mode(llm_config.tool_call_mode.as_deref());
            let function_tooling = if tool_call_mode == ToolCallMode::FunctionCall
                && !prepared.skip_tool_calls
            {
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
                    &session_id,
                    Some(&prepared.language),
                )
                .await;
            system_prompt = self.append_memory_prompt(&user_id, system_prompt).await;

            let history_manager = HistoryManager;
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
            let mut last_usage: Option<TokenUsage> = None;
            let mut answer = String::new();
            let mut stop_reason: Option<String> = None;
            let mut a2ui_uid: Option<String> = None;
            let mut a2ui_messages: Option<Value> = None;
            let mut last_response: Option<(String, String)> = None;
            let mut last_request_messages: Option<Vec<Value>> = None;

            for round in 1..=max_rounds {
                self.ensure_not_cancelled(&session_id)?;
                messages = self
                    .maybe_compact_messages(
                        &config,
                        &llm_config,
                        &user_id,
                    &session_id,
                    messages,
                    &emitter,
                    &question,
                    log_payload,
                )
                .await?;
                self.ensure_not_cancelled(&session_id)?;

                last_request_messages = Some(self.sanitize_messages_for_log(
                    messages.clone(),
                    prepared.attachments.as_deref(),
                ));

                emitter
                    .emit(
                        "progress",
                        json!({
                            "stage": "llm_call",
                            "summary": i18n::t("monitor.summary.model_call"),
                            "round": round
                        }),
                    )
                    .await;

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
                        round,
                        true,
                        true,
                        log_payload,
                        tools_payload,
                        None,
                    )
                    .await?;
                last_response = Some((content.clone(), reasoning.clone()));
                last_usage = Some(usage.clone());
                self.workspace
                    .save_session_token_usage_async(&user_id, &session_id, usage.total as i64)
                    .await;

                let tool_calls = if prepared.skip_tool_calls {
                    Vec::new()
                } else {
                    collect_tool_calls_from_output(&content, &reasoning, tool_calls_payload.as_ref())
                };
                let tool_calls = if let Some(tooling) = function_tooling.as_ref() {
                    apply_tool_name_map(tool_calls, &tooling.name_map)
                } else {
                    tool_calls
                };
                if tool_calls.is_empty() {
                    if prepared.skip_tool_calls {
                        answer = content.trim().to_string();
                    } else {
                        answer = self.resolve_final_answer(&content);
                    }
                    stop_reason = Some("model_response".to_string());
                    let assistant_content = if answer.is_empty() { content.clone() } else { answer.clone() };
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

                let tool_event_emitter = ToolEventEmitter::new({
                    let emitter = emitter.clone();
                    move |event_type, data| {
                        let emitter = emitter.clone();
                        let event_name = event_type.to_string();
                        tokio::spawn(async move {
                            emitter.emit(&event_name, data).await;
                        });
                    }
                }, prepared.stream);

                let mut should_finish = false;
                for call in tool_calls {
                    let mut name = call.name.clone();
                    let args = call.arguments.clone();
                    if name.trim().is_empty() {
                        continue;
                    }
                    name = resolve_tool_name(&name);

                    self.ensure_not_cancelled(&session_id)?;
                    if name == "a2ui" {
                        let (uid, messages_payload, content) =
                            self.resolve_a2ui_tool_payload(&args, &user_id, &session_id);
                        if let Some(messages_payload) = messages_payload.as_ref() {
                            emitter
                                .emit(
                                    "a2ui",
                                    json!({
                                        "uid": uid,
                                        "messages": messages_payload,
                                        "content": content
                                    }),
                                )
                                .await;
                        }
                        a2ui_uid = if uid.trim().is_empty() { None } else { Some(uid.clone()) };
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
                        break;
                    }
                    if name == "最终回复" {
                        answer = self.resolve_final_answer_from_tool(&args);
                        stop_reason = Some("final_tool".to_string());
                        self.log_final_tool_call(&user_id, &session_id, &name, &args, log_payload);
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
                        break;
                    }

                    let tool_context = ToolContext {
                        user_id: &user_id,
                        session_id: &session_id,
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

                    let tool_timeout = self.resolve_tool_timeout(&config, &name, &args);
                    let tool_result = if !allowed_tool_names.contains(&name) {
                        let safe_args = if args.is_object() { args.clone() } else { json!({ "raw": args }) };
                        emitter
                            .emit("tool_call", json!({ "tool": name, "args": safe_args }))
                            .await;
                        ToolResultPayload::error(
                            i18n::t("error.tool_disabled_or_unavailable"),
                            json!({ "tool": name }),
                        )
                    } else {
                        emitter
                            .emit("tool_call", json!({ "tool": name, "args": args }))
                            .await;
                        let result = tokio::select! {
                            res = self.execute_tool_with_timeout(&tool_context, &name, &args, tool_timeout) => res,
                            err = self.wait_for_cancelled(&session_id) => {
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
                                ToolResultPayload::error(message, json!({ "tool": name }))
                            }
                        }
                    };
                    let question_panel_finished = name == "问询面板" && tool_result.ok;
                    if question_panel_finished {
                        answer = i18n::t("response.question_panel_waiting");
                        stop_reason = Some("question_panel".to_string());
                        should_finish = true;
                    }

                    let observation = self.build_tool_observation(&name, &tool_result);
                    let tool_call_id = if tool_call_mode == ToolCallMode::FunctionCall {
                        call.id
                            .as_ref()
                            .map(|value| value.trim())
                            .filter(|value| !value.is_empty())
                            .map(|value| value.to_string())
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
                        &tool_result,
                        log_payload,
                    );
                    self.append_artifact_logs(
                        &user_id,
                        &session_id,
                        &name,
                        &args,
                        &tool_result,
                    );
                    if name == "读取文件" {
                        self.append_skill_usage_logs(
                            &user_id,
                            &session_id,
                            &args,
                            &skills_snapshot,
                            Some(&user_tool_bindings),
                            log_payload,
                        );
                    }

                    emitter
                        .emit(
                            "tool_result",
                            tool_result.to_event_payload(&name),
                        )
                        .await;

                    if question_panel_finished && !answer.trim().is_empty() {
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

                    self.ensure_not_cancelled(&session_id)?;
                    if !answer.is_empty() {
                        break;
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
            let response = WunderResponse {
                session_id: session_id.clone(),
                answer: answer.clone(),
                usage: last_usage.clone(),
                stop_reason: Some(stop_reason.clone()),
                uid: a2ui_uid.clone(),
                a2ui: a2ui_messages.clone(),
            };
            emitter
                .emit(
                    "final",
                    json!({
                        "answer": answer,
                        "usage": last_usage.clone().unwrap_or(TokenUsage { input: 0, output: 0, total: 0 }),
                        "stop_reason": stop_reason
                    }),
                )
                .await;
            self.monitor.mark_finished(&session_id);
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
}
