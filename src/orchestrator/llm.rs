use super::*;

#[derive(Default)]
struct OutputTiming {
    first_output_at: Option<Instant>,
    last_output_at: Option<Instant>,
}

impl OutputTiming {
    fn mark_output(&mut self, now: Instant) {
        if self.first_output_at.is_none() {
            self.first_output_at = Some(now);
        }
        self.last_output_at = Some(now);
    }

    fn durations(
        &self,
        request_start: Instant,
        response_end: Instant,
    ) -> (Option<f64>, Option<f64>) {
        let Some(first_output_at) = self.first_output_at else {
            return (None, None);
        };
        let last_output_at = self.last_output_at.unwrap_or(response_end);
        let prefill = first_output_at
            .saturating_duration_since(request_start)
            .as_secs_f64();
        let decode = last_output_at
            .saturating_duration_since(first_output_at)
            .as_secs_f64();
        (Some(prefill), Some(decode))
    }
}

impl Orchestrator {
    pub(super) async fn consume_user_quota(
        &self,
        user_id: &str,
        emitter: &EventEmitter,
        round_info: RoundInfo,
        emit_quota_events: bool,
    ) -> Result<(), OrchestratorError> {
        let today = UserStore::today_string();
        let status = self
            .storage
            .consume_user_quota(user_id, &today)
            .map_err(|err| OrchestratorError::internal(err.to_string()))?;
        let Some(status) = status else {
            return Ok(());
        };
        if !status.allowed {
            return Err(OrchestratorError::user_quota_exceeded(status));
        }
        if emit_quota_events {
            let mut payload = json!({
                "consumed": 1,
                "daily_quota": status.daily_quota,
                "used": status.used,
                "remaining": status.remaining,
                "date": status.date,
            });
            if let Value::Object(ref mut map) = payload {
                round_info.insert_into(map);
            }
            emitter.emit("quota_usage", payload).await;
        }
        Ok(())
    }

    pub(super) fn resolve_llm_config(
        &self,
        config: &Config,
        model_name: Option<&str>,
    ) -> Result<(String, LlmModelConfig), OrchestratorError> {
        let name = model_name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| config.llm.default.as_str());
        if let Some(configured) = config.llm.models.get(name) {
            return Ok((name.to_string(), configured.clone()));
        }
        if let Some((fallback_name, fallback)) = config.llm.models.iter().next() {
            return Ok((fallback_name.clone(), fallback.clone()));
        }
        Err(OrchestratorError::llm_unavailable(i18n::t(
            "error.llm_unavailable",
        )))
    }

    pub(super) fn resolve_tool_call_mode(
        &self,
        config: &Config,
        model_name: Option<&str>,
    ) -> ToolCallMode {
        self.resolve_llm_config(config, model_name)
            .map(|(_, config)| normalize_tool_call_mode(config.tool_call_mode.as_deref()))
            .unwrap_or(ToolCallMode::ToolCall)
    }

    pub(super) fn ensure_not_cancelled(&self, session_id: &str) -> Result<(), OrchestratorError> {
        if self.monitor.is_cancelled(session_id) {
            return Err(OrchestratorError::cancelled(i18n::t(
                "error.session_cancelled",
            )));
        }
        Ok(())
    }

    pub(super) async fn wait_for_cancelled(&self, session_id: &str) -> OrchestratorError {
        let mut interval = tokio::time::interval(Duration::from_millis(200));
        loop {
            interval.tick().await;
            if self.monitor.is_cancelled(session_id) {
                return OrchestratorError::cancelled(i18n::t("error.session_cancelled"));
            }
        }
    }

    pub(super) async fn sleep_or_cancel(
        &self,
        session_id: &str,
        duration: Duration,
    ) -> Result<(), OrchestratorError> {
        let cancel = self.wait_for_cancelled(session_id);
        tokio::select! {
            _ = tokio::time::sleep(duration) => Ok(()),
            err = cancel => Err(err),
        }
    }

    pub(super) async fn await_with_cancel<F, T>(
        &self,
        session_id: &str,
        timeout_s: u64,
        fut: F,
    ) -> Result<Result<T, anyhow::Error>, OrchestratorError>
    where
        F: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        let cancel = self.wait_for_cancelled(session_id);
        if timeout_s > 0 {
            tokio::select! {
                res = tokio::time::timeout(Duration::from_secs(timeout_s), fut) => {
                    Ok(res.map_err(|_| anyhow::anyhow!("timeout")).and_then(|inner| inner))
                }
                err = cancel => Err(err),
            }
        } else {
            tokio::select! {
                res = fut => Ok(res),
                err = cancel => Err(err),
            }
        }
    }

    pub(super) fn build_chat_messages(&self, messages: &[Value]) -> Vec<ChatMessage> {
        messages
            .iter()
            .filter_map(|message| {
                let role = message.get("role").and_then(Value::as_str)?.to_string();
                let content = message.get("content").cloned().unwrap_or(Value::Null);
                let reasoning_content = message
                    .get("reasoning_content")
                    .or_else(|| message.get("reasoning"))
                    .and_then(Value::as_str)
                    .and_then(|text| {
                        if text.trim().is_empty() {
                            None
                        } else {
                            Some(text.to_string())
                        }
                    });
                let tool_calls = message
                    .get("tool_calls")
                    .or_else(|| message.get("tool_call"))
                    .or_else(|| message.get("function_call"))
                    .cloned();
                let tool_call_id = message
                    .get("tool_call_id")
                    .or_else(|| message.get("toolCallId"))
                    .or_else(|| message.get("call_id"))
                    .or_else(|| message.get("callId"))
                    .and_then(|value| match value {
                        Value::String(text) => Some(text.clone()),
                        Value::Number(num) => Some(num.to_string()),
                        _ => None,
                    })
                    .and_then(|text| {
                        let cleaned = text.trim().to_string();
                        if cleaned.is_empty() {
                            None
                        } else {
                            Some(cleaned)
                        }
                    });
                Some(ChatMessage {
                    role,
                    content,
                    reasoning_content,
                    tool_calls,
                    tool_call_id,
                })
            })
            .collect()
    }

    pub(super) fn estimate_token_usage(
        &self,
        messages: &[Value],
        content: &str,
        reasoning: &str,
    ) -> TokenUsage {
        let input = estimate_messages_tokens(messages).max(0) as u64;
        let output = (approx_token_count(content) + approx_token_count(reasoning)).max(0) as u64;
        TokenUsage {
            input,
            output,
            total: input + output,
        }
    }

    pub(super) fn resolve_llm_timeout_s(&self, config: &LlmModelConfig) -> u64 {
        let timeout_s = config.timeout_s.unwrap_or(DEFAULT_LLM_TIMEOUT_S);
        if timeout_s == 0 {
            DEFAULT_LLM_TIMEOUT_S
        } else {
            timeout_s
        }
    }

    pub(super) async fn log_payload_enabled(&self) -> bool {
        let config = self.config_store.get().await;
        is_debug_log_level(&config.observability.log_level)
    }

    pub(super) async fn call_llm(
        &self,
        llm_config: &LlmModelConfig,
        messages: &[Value],
        user_id: &str,
        emitter: &EventEmitter,
        session_id: &str,
        stream: bool,
        round_info: RoundInfo,
        emit_events: bool,
        emit_quota_events: bool,
        log_payload: bool,
        tools: Option<&[Value]>,
        llm_config_override: Option<LlmModelConfig>,
    ) -> Result<(String, String, TokenUsage, Option<Value>), OrchestratorError> {
        self.ensure_not_cancelled(session_id)?;
        let effective_config = llm_config_override.unwrap_or_else(|| llm_config.clone());
        if !is_llm_configured(&effective_config) {
            if effective_config.mock_if_unconfigured.unwrap_or(false) {
                let content = i18n::t("error.llm_not_configured");
                let usage = self.estimate_token_usage(messages, &content, "");
                if emit_events {
                    let mut output_payload =
                        json!({ "content": content, "reasoning": "", "usage": usage });
                    if let Value::Object(ref mut map) = output_payload {
                        round_info.insert_into(map);
                    }
                    emitter.emit("llm_output", output_payload).await;
                    let mut usage_payload = json!({
                        "input_tokens": usage.input,
                        "output_tokens": usage.output,
                        "total_tokens": usage.total,
                    });
                    if let Value::Object(ref mut map) = usage_payload {
                        round_info.insert_into(map);
                    }
                    emitter.emit("token_usage", usage_payload).await;
                }
                return Ok((content, String::new(), usage, None));
            }
            let detail = i18n::t("error.llm_config_missing");
            return Err(OrchestratorError::llm_unavailable(i18n::t_with_params(
                "error.llm_unavailable",
                &HashMap::from([("detail".to_string(), detail)]),
            )));
        }

        self.consume_user_quota(user_id, emitter, round_info, emit_quota_events)
            .await?;

        let client = build_llm_client(&effective_config, self.http.clone());
        let chat_messages = self.build_chat_messages(messages);
        let will_stream = stream;

        if emit_events {
            let mut request_payload = if log_payload {
                let payload_messages = self.sanitize_messages_for_log(messages.to_vec(), None);
                let payload_chat = self.build_chat_messages(&payload_messages);
                let payload =
                    client.build_request_payload_with_tools(&payload_chat, will_stream, tools);
                json!({
                    "provider": effective_config.provider,
                    "model": effective_config.model,
                    "base_url": effective_config.base_url,
                    "stream": will_stream,
                    "payload": payload,
                })
            } else {
                json!({
                    "provider": effective_config.provider,
                    "model": effective_config.model,
                    "base_url": effective_config.base_url,
                    "stream": will_stream,
                    "payload_omitted": true,
                })
            };
            if let Value::Object(ref mut map) = request_payload {
                round_info.insert_into(map);
            }
            emitter.emit("llm_request", request_payload).await;
        }

        let timeout_s = self.resolve_llm_timeout_s(&effective_config);
        let max_attempts = effective_config.retry.unwrap_or(0).saturating_add(1).max(1);
        let mut attempt = 0u32;
        let mut last_err: anyhow::Error;
        loop {
            attempt += 1;
            let request_started_at = Instant::now();
            let output_timing = Arc::new(parking_lot::Mutex::new(OutputTiming::default()));
            let result = if will_stream {
                let emitter_snapshot = emitter.clone();
                let timing_snapshot = Arc::clone(&output_timing);
                let round_info = round_info;
                let on_delta = move |delta: String, reasoning_delta: String| {
                    let emitter = emitter_snapshot.clone();
                    let timing = Arc::clone(&timing_snapshot);
                    async move {
                        if !delta.is_empty() || !reasoning_delta.is_empty() {
                            timing.lock().mark_output(Instant::now());
                        }
                        if emit_events {
                            let mut payload = serde_json::Map::new();
                            if !delta.is_empty() {
                                payload.insert("delta".to_string(), Value::String(delta));
                            }
                            if !reasoning_delta.is_empty() {
                                payload.insert(
                                    "reasoning_delta".to_string(),
                                    Value::String(reasoning_delta),
                                );
                            }
                            round_info.insert_into(&mut payload);
                            emitter
                                .emit("llm_output_delta", Value::Object(payload))
                                .await;
                        }
                        Ok(())
                    }
                };
                let fut = async {
                    if tools.is_some() {
                        client
                            .stream_complete_with_callback_with_tools(
                                &chat_messages,
                                tools,
                                on_delta,
                            )
                            .await
                    } else {
                        client
                            .stream_complete_with_callback(&chat_messages, on_delta)
                            .await
                    }
                };
                self.await_with_cancel(session_id, timeout_s, fut).await?
            } else {
                let fut = client.complete_with_tools(&chat_messages, tools);
                self.await_with_cancel(session_id, timeout_s, fut).await?
            };

            match result {
                Ok(response) => {
                    let response_finished_at = Instant::now();
                    let content = response.content;
                    let reasoning = response.reasoning;
                    let tool_calls = response.tool_calls;
                    let mut usage = response.usage;
                    if let Some(item) = usage.as_mut() {
                        if item.total == 0 {
                            let total = item.input.saturating_add(item.output);
                            if total > 0 {
                                item.total = total;
                            }
                        }
                    }
                    let mut usage = usage.filter(|item| item.total > 0).unwrap_or_else(|| {
                        self.estimate_token_usage(messages, &content, &reasoning)
                    });
                    if (usage.input == 0 || usage.output == 0) && usage.total > 0 {
                        let estimated = self.estimate_token_usage(messages, &content, &reasoning);
                        if estimated.total > 0 {
                            let ratio = usage.total as f64 / estimated.total as f64;
                            let mut input = (estimated.input as f64 * ratio).round() as u64;
                            if input > usage.total {
                                input = usage.total;
                            }
                            let output = usage.total.saturating_sub(input);
                            usage.input = input;
                            usage.output = output;
                        }
                    }
                    let (prefill_duration_s, decode_duration_s) = if will_stream {
                        output_timing
                            .lock()
                            .durations(request_started_at, response_finished_at)
                    } else {
                        (None, None)
                    };
                    if emit_events {
                        let tool_calls_snapshot = tool_calls.clone();
                        let mut output_payload = json!({
                            "content": content,
                            "reasoning": reasoning,
                            "usage": usage,
                            "tool_calls": tool_calls_snapshot,
                            "prefill_duration_s": prefill_duration_s,
                            "decode_duration_s": decode_duration_s,
                        });
                        if let Value::Object(ref mut map) = output_payload {
                            round_info.insert_into(map);
                        }
                        emitter.emit("llm_output", output_payload).await;
                        let mut usage_payload = json!({
                            "input_tokens": usage.input,
                            "output_tokens": usage.output,
                            "total_tokens": usage.total,
                            "prefill_duration_s": prefill_duration_s,
                            "decode_duration_s": decode_duration_s,
                        });
                        if let Value::Object(ref mut map) = usage_payload {
                            round_info.insert_into(map);
                        }
                        emitter.emit("token_usage", usage_payload).await;
                    }
                    return Ok((content, reasoning, usage, tool_calls));
                }
                Err(err) => {
                    last_err = err;
                }
            }

            if attempt >= max_attempts {
                break;
            }
            if emit_events && will_stream {
                let delay_s = (attempt as f64).min(3.0);
                let mut retry_payload = json!({
                    "attempt": attempt,
                    "max_attempts": max_attempts,
                    "delay_s": delay_s,
                    "will_retry": true,
                });
                if let Value::Object(ref mut map) = retry_payload {
                    round_info.insert_into(map);
                }
                emitter.emit("llm_stream_retry", retry_payload).await;
                self.sleep_or_cancel(session_id, Duration::from_secs_f64(delay_s))
                    .await?;
            }
        }

        let detail = last_err.to_string();
        Err(OrchestratorError::internal(i18n::t_with_params(
            "error.llm_call_failed",
            &HashMap::from([("detail".to_string(), detail)]),
        )))
    }
}
