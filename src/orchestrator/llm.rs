use super::*;
use crate::core::llm_speed::LlmSpeedSummary;

#[derive(Default)]
struct OutputTiming {
    first_output_at: Option<Instant>,
    last_output_at: Option<Instant>,
}

struct ChatMessageRepairReport {
    messages: Vec<ChatMessage>,
    repair: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LlmFailureKind {
    Other,
    ContextWindow,
    Unavailable,
}

const LLM_UNAVAILABLE_MIN_RETRIES: u32 = 5;
const LLM_UNAVAILABLE_RETRY_DELAYS_MS: [u64; 5] = [1_200, 3_000, 6_000, 12_000, 20_000];
const DEFAULT_LLM_MAX_ATTEMPTS: u32 = 2;

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

fn sanitize_chat_messages_for_request(messages: &[ChatMessage]) -> ChatMessageRepairReport {
    let mut repaired_count = 0usize;
    let messages = messages
        .iter()
        .map(|message| {
            let tool_calls = message.tool_calls.as_ref().map(|payload| {
                let sanitized =
                    crate::core::tool_args::sanitize_tool_call_payload_with_meta(payload);
                repaired_count = repaired_count.saturating_add(
                    sanitized
                        .repair
                        .as_ref()
                        .and_then(|value| value.get("count"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize,
                );
                sanitized.value
            });
            ChatMessage {
                role: message.role.clone(),
                content: message.content.clone(),
                reasoning_content: message.reasoning_content.clone(),
                tool_calls,
                tool_call_id: message.tool_call_id.clone(),
            }
        })
        .collect();
    let repair = (repaired_count > 0).then(|| {
        json!({
            "kind": "chat_messages",
            "source": "tool_calls",
            "strategy": "sanitize_before_request",
            "count": repaired_count,
        })
    });
    ChatMessageRepairReport { messages, repair }
}

impl Orchestrator {
    fn resolve_user_daily_token_grant(&self, user_id: &str) -> Result<i64, OrchestratorError> {
        let user = self
            .storage
            .get_user_account(user_id)
            .map_err(|err| OrchestratorError::internal(err.to_string()))?;
        let unit_level = user
            .as_ref()
            .and_then(|record| record.unit_id.as_deref())
            .and_then(|unit_id| {
                self.storage
                    .get_org_unit(unit_id)
                    .ok()
                    .flatten()
                    .map(|unit| unit.level)
            });
        Ok(UserStore::default_daily_token_grant_by_level(unit_level))
    }

    pub(super) async fn ensure_user_token_balance(
        &self,
        user_id: &str,
        _emitter: &EventEmitter,
        _round_info: RoundInfo,
        _emit_quota_events: bool,
    ) -> Result<(), OrchestratorError> {
        let today = UserStore::today_string();
        let daily_grant = self.resolve_user_daily_token_grant(user_id)?;
        let status = self
            .storage
            .prepare_user_token_balance(user_id, &today, daily_grant)
            .map_err(|err| OrchestratorError::internal(err.to_string()))?;
        let Some(status) = status else {
            return Ok(());
        };
        if !status.allowed {
            return Err(OrchestratorError::user_token_insufficient(status));
        }
        Ok(())
    }

    pub(super) async fn consume_user_tokens(
        &self,
        user_id: &str,
        consumed_tokens: i64,
        emitter: &EventEmitter,
        round_info: RoundInfo,
        emit_quota_events: bool,
    ) -> Result<(), OrchestratorError> {
        let safe_consumed = consumed_tokens.max(0);
        if safe_consumed <= 0 {
            return Ok(());
        }
        let today = UserStore::today_string();
        let daily_grant = self.resolve_user_daily_token_grant(user_id)?;
        let status = self
            .storage
            .consume_user_tokens(user_id, &today, daily_grant, safe_consumed)
            .map_err(|err| OrchestratorError::internal(err.to_string()))?;
        let Some(status) = status else {
            return Ok(());
        };
        if emit_quota_events {
            let mut payload = json!({
                "consumed": safe_consumed,
                "token_balance": status.balance,
                "token_granted_total": status.granted_total,
                "token_used_total": status.used_total,
                "daily_token_grant": status.daily_grant,
                "last_token_grant_date": status.last_grant_date,
                "overspent_tokens": status.overspent_tokens,
                // Legacy aliases kept for existing clients during migration.
                "daily_quota": status.granted_total,
                "used": status.used_total,
                "remaining": status.balance,
                "date": status.last_grant_date,
            });
            if let Value::Object(ref mut map) = payload {
                round_info.insert_into(map);
            }
            emitter.emit("token_balance", payload.clone()).await;
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
            .unwrap_or(config.llm.default.as_str());
        if !name.trim().is_empty() {
            if let Some(configured) = config
                .llm
                .models
                .get(name)
                .filter(|model| is_llm_model(model))
            {
                return Ok((name.to_string(), configured.clone()));
            }
        }
        if let Some((fallback_name, fallback)) = config
            .llm
            .models
            .iter()
            .find(|(_, model)| is_llm_model(model))
        {
            return Ok((fallback_name.clone(), fallback.clone()));
        }
        let detail = i18n::t("error.llm_config_required");
        Err(OrchestratorError::llm_unavailable(i18n::t_with_params(
            "error.llm_unavailable",
            &HashMap::from([("detail".to_string(), detail)]),
        )))
    }

    pub(super) fn resolve_tool_call_mode(
        &self,
        config: &Config,
        model_name: Option<&str>,
    ) -> ToolCallMode {
        self.resolve_llm_config(config, model_name)
            .map(|(_, config)| crate::llm::resolve_tool_call_mode(&config))
            .unwrap_or(ToolCallMode::FunctionCall)
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
        let output = approx_token_count(content).max(0) as u64;
        let reasoning_tokens = approx_token_count(reasoning).max(0) as u64;
        TokenUsage {
            input,
            output,
            total: input
                .saturating_add(output)
                .saturating_add(reasoning_tokens),
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

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn call_llm(
        &self,
        llm_config: &LlmModelConfig,
        messages: &[Value],
        user_id: &str,
        is_admin: bool,
        emitter: &EventEmitter,
        session_id: &str,
        stream: bool,
        round_info: RoundInfo,
        emit_events: bool,
        emit_quota_events: bool,
        log_payload: bool,
        tools: Option<&[Value]>,
        llm_config_override: Option<LlmModelConfig>,
    ) -> Result<(String, String, TokenUsage, Option<Value>, LlmSpeedSummary), OrchestratorError>
    {
        self.ensure_not_cancelled(session_id)?;
        let effective_config = llm_config_override.unwrap_or_else(|| llm_config.clone());
        if !is_llm_configured(&effective_config) {
            if effective_config.mock_if_unconfigured.unwrap_or(false) {
                let content = i18n::t("error.llm_not_configured");
                let usage = self.estimate_token_usage(messages, &content, "");
                let decode_output_tokens = usage.total.saturating_sub(usage.input);
                let round_speed = LlmSpeedSummary::from_usage_and_durations(
                    Some(usage.input),
                    Some(decode_output_tokens),
                    None,
                    None,
                );
                if emit_events {
                    let mut output_payload = json!({
                        "content": content,
                        "reasoning": "",
                        "usage": usage,
                        "decode_output_tokens": decode_output_tokens,
                    });
                    if let Value::Object(ref mut map) = output_payload {
                        round_info.insert_into(map);
                        round_speed.insert_into_map(map);
                    }
                    emitter.emit("llm_output", output_payload).await;
                    let mut usage_payload = json!({
                        "input_tokens": usage.input,
                        "output_tokens": usage.output,
                        "total_tokens": usage.total,
                        "decode_output_tokens": decode_output_tokens,
                    });
                    if let Value::Object(ref mut map) = usage_payload {
                        round_info.insert_into(map);
                        round_speed.insert_into_map(map);
                    }
                    emitter.emit("token_usage", usage_payload).await;
                }
                return Ok((content, String::new(), usage, None, round_speed));
            }
            let detail = i18n::t("error.llm_config_missing");
            return Err(OrchestratorError::llm_unavailable(i18n::t_with_params(
                "error.llm_unavailable",
                &HashMap::from([("detail".to_string(), detail)]),
            )));
        }

        if !is_admin {
            self.ensure_user_token_balance(user_id, emitter, round_info, emit_quota_events)
                .await?;
        }

        let client = build_llm_client(&effective_config, self.http.clone());
        let chat_messages = sanitize_chat_messages_for_request(&self.build_chat_messages(messages));
        let will_stream = stream;

        if emit_events {
            let mut request_payload = if log_payload {
                let payload_messages = self.sanitize_messages_for_log(messages.to_vec(), None);
                let payload_chat = sanitize_chat_messages_for_request(
                    &self.build_chat_messages(&payload_messages),
                );
                let payload = client.build_request_payload_with_tools(
                    &payload_chat.messages,
                    will_stream,
                    tools,
                );
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
                if let Some(repair) = chat_messages.repair.clone() {
                    map.insert("repair".to_string(), repair);
                }
                round_info.insert_into(map);
            }
            emitter.emit("llm_request", request_payload).await;
        }

        let timeout_s = if is_admin {
            0
        } else {
            self.resolve_llm_timeout_s(&effective_config)
        };
        let mut attempt = 0u32;
        let mut last_err: anyhow::Error;
        loop {
            attempt += 1;
            let request_started_at = Instant::now();
            let output_timing = Arc::new(parking_lot::Mutex::new(OutputTiming::default()));
            let result = if will_stream {
                let emitter_snapshot = emitter.clone();
                let timing_snapshot = Arc::clone(&output_timing);
                let on_delta = move |delta: String, reasoning_delta: String| {
                    let emitter = emitter_snapshot.clone();
                    let timing = Arc::clone(&timing_snapshot);
                    async move {
                        timing.lock().mark_output(Instant::now());
                        if emit_events && (!delta.is_empty() || !reasoning_delta.is_empty()) {
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
                                &chat_messages.messages,
                                tools,
                                on_delta,
                            )
                            .await
                    } else {
                        client
                            .stream_complete_with_callback(&chat_messages.messages, on_delta)
                            .await
                    }
                };
                self.await_with_cancel(session_id, timeout_s, fut).await?
            } else {
                let fut = client.complete_with_tools(&chat_messages.messages, tools);
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
                    let decode_output_tokens = usage.total.saturating_sub(usage.input);
                    let round_speed = LlmSpeedSummary::from_usage_and_durations(
                        Some(usage.input),
                        Some(decode_output_tokens),
                        prefill_duration_s,
                        decode_duration_s,
                    );
                    if emit_events {
                        let tool_calls_snapshot = tool_calls.clone();
                        let mut output_payload = json!({
                            "content": content,
                            "reasoning": reasoning,
                            "usage": usage,
                            "decode_output_tokens": decode_output_tokens,
                            "tool_calls": tool_calls_snapshot,
                            "prefill_duration_s": prefill_duration_s,
                            "decode_duration_s": decode_duration_s,
                        });
                        if let Value::Object(ref mut map) = output_payload {
                            round_info.insert_into(map);
                            round_speed.insert_into_map(map);
                        }
                        emitter.emit("llm_output", output_payload).await;
                        let mut usage_payload = json!({
                            "input_tokens": usage.input,
                            "output_tokens": usage.output,
                            "total_tokens": usage.total,
                            "decode_output_tokens": decode_output_tokens,
                            "prefill_duration_s": prefill_duration_s,
                            "decode_duration_s": decode_duration_s,
                        });
                        if let Value::Object(ref mut map) = usage_payload {
                            round_info.insert_into(map);
                            round_speed.insert_into_map(map);
                        }
                        emitter.emit("token_usage", usage_payload).await;
                    }
                    if !is_admin {
                        let consumed_tokens = usage.total.min(i64::MAX as u64) as i64;
                        self.consume_user_tokens(
                            user_id,
                            consumed_tokens,
                            emitter,
                            round_info,
                            emit_quota_events,
                        )
                        .await?;
                    }
                    return Ok((content, reasoning, usage, tool_calls, round_speed));
                }
                Err(err) => {
                    let failure_kind = classify_llm_error(&err);
                    let max_attempts = resolve_llm_max_attempts(failure_kind);
                    let should_retry = attempt < max_attempts;
                    let retry_delay = resolve_llm_retry_delay(attempt, failure_kind);
                    if emit_events && should_retry {
                        let mut retry_payload = json!({
                            "attempt": attempt,
                            "max_attempts": max_attempts,
                            "delay_s": retry_delay.as_secs_f64(),
                            "retry_reason": llm_retry_reason(failure_kind),
                            "stream": will_stream,
                            "will_retry": true,
                            "error": err.to_string(),
                        });
                        if let Value::Object(ref mut map) = retry_payload {
                            round_info.insert_into(map);
                        }
                        emitter.emit("llm_stream_retry", retry_payload).await;
                    }
                    last_err = err;
                    if !should_retry {
                        break;
                    }
                    if !retry_delay.is_zero() {
                        self.sleep_or_cancel(session_id, retry_delay).await?;
                    }
                }
            }
        }

        let detail = last_err.to_string();
        let failure_kind = classify_llm_error(&last_err);
        let message_key = if matches!(failure_kind, LlmFailureKind::Unavailable) {
            "error.llm_unavailable"
        } else {
            "error.llm_call_failed"
        };
        let message = i18n::t_with_params(
            message_key,
            &HashMap::from([("detail".to_string(), detail)]),
        );
        match failure_kind {
            LlmFailureKind::ContextWindow => {
                Err(OrchestratorError::context_window_exceeded(message))
            }
            LlmFailureKind::Unavailable => Err(OrchestratorError::llm_unavailable(message)),
            LlmFailureKind::Other => Err(OrchestratorError::internal(message)),
        }
    }
}

#[cfg(test)]
fn classify_llm_failure(message: &str) -> LlmFailureKind {
    if is_context_window_error_text(message) {
        return LlmFailureKind::ContextWindow;
    }
    if is_llm_unavailable_error_text(message) {
        return LlmFailureKind::Unavailable;
    }
    LlmFailureKind::Other
}

fn classify_llm_error(error: &anyhow::Error) -> LlmFailureKind {
    let message = error.to_string();
    if is_context_window_error_text(&message) {
        return LlmFailureKind::ContextWindow;
    }
    if is_llm_request_transport_error(error) || is_llm_unavailable_error_text(&message) {
        return LlmFailureKind::Unavailable;
    }
    LlmFailureKind::Other
}

fn is_llm_request_transport_error(error: &anyhow::Error) -> bool {
    error.chain().any(|source| {
        source.downcast_ref::<reqwest::Error>().is_some_and(|err| {
            err.is_timeout()
                || err.is_connect()
                || err.is_request()
                || err.is_body()
                || err.is_decode()
        })
    })
}

fn is_llm_unavailable_error_text(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    [
        "error sending request",
        "error trying to connect",
        "connection refused",
        "connection reset",
        "connection aborted",
        "connection closed",
        "server disconnected",
        "broken pipe",
        "failed to connect",
        "timed out",
        "timeout",
        "dns error",
        "too many requests",
        "rate limit",
        "bad gateway",
        "gateway timeout",
        "internal server error",
        "temporarily unavailable",
        "service unavailable",
        "unavailable_error",
        "error decoding response body",
        "read llm response body",
        "read body failed",
        "error reading a body from connection",
        "unexpected eof",
        "unexpected end of file",
        "end of file before message length reached",
        "loading model",
        "429",
        "500 internal server error",
        "502 bad gateway",
        "503",
        "504 gateway timeout",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn resolve_llm_max_attempts(failure_kind: LlmFailureKind) -> u32 {
    if matches!(failure_kind, LlmFailureKind::Unavailable) {
        DEFAULT_LLM_MAX_ATTEMPTS.max(LLM_UNAVAILABLE_MIN_RETRIES.saturating_add(1))
    } else {
        DEFAULT_LLM_MAX_ATTEMPTS
    }
}

fn resolve_llm_retry_delay(attempt: u32, failure_kind: LlmFailureKind) -> Duration {
    if matches!(failure_kind, LlmFailureKind::Unavailable) {
        let index = attempt.saturating_sub(1) as usize;
        let delay_ms = LLM_UNAVAILABLE_RETRY_DELAYS_MS
            .get(index)
            .copied()
            .unwrap_or(*LLM_UNAVAILABLE_RETRY_DELAYS_MS.last().unwrap_or(&30_000));
        Duration::from_millis(delay_ms)
    } else {
        Duration::from_secs_f64((attempt as f64).min(3.0))
    }
}

fn llm_retry_reason(failure_kind: LlmFailureKind) -> &'static str {
    match failure_kind {
        LlmFailureKind::ContextWindow => "context_window",
        LlmFailureKind::Unavailable => "llm_unavailable",
        LlmFailureKind::Other => "provider_error",
    }
}

pub(super) fn is_context_window_error_text(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    [
        "context_length_exceeded",
        "context_window_exceeded",
        "context length",
        "context window",
        "context window of this model",
        "maximum context",
        "maximum context length",
        "max context",
        "context size",
        "too many tokens",
        "your input exceeds the context window",
        "input exceeds the context window",
        "input exceeds the model's context window",
        "prompt is too long",
        "prompt too long",
        "input is too long",
        "input length should be",
        "range of input length",
        "range of prompt length",
        "input length exceeds",
        "input token count",
        "prompt token count",
        "maximum number of tokens",
        "requested tokens",
        "requested token count",
        "context overflow",
        "token limit exceeded",
        "reduce the length",
        "exceeds the model's context window",
        "exceeds the available context size",
        "this model's maximum context length is",
        "requested tokens exceed",
        "maximum input length",
        "input too large",
        "prompt exceeds",
        "上下文窗口",
        "上下文长度",
        "超出模型上下文",
        "超过模型上下文",
        "超出最大上下文",
        "超过最大上下文",
        "提示词过长",
        "输入太长",
        "输入长度应在",
        "长度范围",
        "最大输入长度",
        "最大上下文长度",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

pub(super) fn extract_context_window_limit_hint(message: &str) -> Option<i64> {
    let text = message.trim();
    if text.is_empty() {
        return None;
    }
    let regexes = context_window_limit_hint_regexes();
    for regex in regexes {
        if let Some(captures) = regex.captures(text) {
            let raw = captures.get(1).map(|matched| matched.as_str());
            if let Some(raw) = raw {
                if let Some(value) = parse_context_limit_number(raw) {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn context_window_limit_hint_regexes() -> &'static Vec<Regex> {
    static REGEXES: OnceLock<Vec<Regex>> = OnceLock::new();
    REGEXES.get_or_init(|| {
        let patterns = [
            r"(?i)this model['’]s maximum context length is\s*([0-9][0-9_,]*)",
            r"(?i)maximum\s+context(?:\s+window)?\s+length\s+is\s*([0-9][0-9_,]*)",
            r"(?i)context window(?: of this model)?(?: is|:)?\s*([0-9][0-9_,]*)",
            r"(?i)input length should be\s*\[\s*\d+\s*[,，]\s*([0-9][0-9_,]*)\s*\]",
            r"(?i)range of input length should be\s*\[\s*\d+\s*[,，]\s*([0-9][0-9_,]*)\s*\]",
            r"(?i)range of prompt length should be\s*\[\s*\d+\s*[,，]\s*([0-9][0-9_,]*)\s*\]",
            r"(?i)tokens?\s*\+\s*max_new_tokens\s*must\s*be\s*<=\s*([0-9][0-9_,]*)",
            r"(?i)at most\s*([0-9][0-9_,]*)\s*tokens",
            r"最大(?:上下文|输入)(?:长度|窗口)?[^0-9]{0,12}([0-9][0-9_,]*)",
            r"上下文(?:长度|窗口)[^0-9]{0,12}([0-9][0-9_,]*)",
            r"长度范围[^\[]*\[\s*\d+\s*[,，]\s*([0-9][0-9_,]*)\s*\]",
        ];
        patterns
            .iter()
            .filter_map(|pattern| compile_regex(pattern, "context_window_limit_hint"))
            .collect()
    })
}

fn parse_context_limit_number(raw: &str) -> Option<i64> {
    let digits = raw
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok().filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::{
        classify_llm_failure, extract_context_window_limit_hint, is_context_window_error_text,
        is_llm_unavailable_error_text, llm_retry_reason, resolve_llm_max_attempts,
        resolve_llm_retry_delay, LlmFailureKind, DEFAULT_LLM_MAX_ATTEMPTS,
        LLM_UNAVAILABLE_MIN_RETRIES,
    };

    #[test]
    fn detects_context_window_error_from_common_phrases() {
        assert!(is_context_window_error_text(
            "LLM call failed: context_length_exceeded"
        ));
        assert!(is_context_window_error_text(
            "Prompt is too long and exceeds the model's context window."
        ));
        assert!(is_context_window_error_text(
            "This model's maximum context length is 16384 tokens, but you requested 19000 tokens."
        ));
        assert!(is_context_window_error_text(
            "input exceeds the available context size"
        ));
        assert!(is_context_window_error_text(
            "Your input exceeds the context window of this model. Please adjust your input and try again."
        ));
        assert!(is_context_window_error_text(
            "InternalError.Algo.InvalidParameter: Range of input length should be [1, 258048]"
        ));
        assert!(is_context_window_error_text(
            "模型调用失败: prompt too long"
        ));
    }

    #[test]
    fn ignores_non_context_window_error_messages() {
        assert!(!is_context_window_error_text(
            "LLM call failed: invalid api key"
        ));
        assert!(!is_context_window_error_text("network timeout"));
    }

    #[test]
    fn detects_context_window_error_from_chinese_phrases() {
        assert!(is_context_window_error_text(
            "模型调用失败：超过模型上下文窗口，请缩短输入后重试。"
        ));
        assert!(is_context_window_error_text(
            "提示词过长，超出最大上下文长度。"
        ));
    }

    #[test]
    fn extracts_context_window_limit_hint_from_common_errors() {
        assert_eq!(
            extract_context_window_limit_hint(
                "This model's maximum context length is 16384 tokens, but you requested 19000 tokens."
            ),
            Some(16384)
        );
        assert_eq!(
            extract_context_window_limit_hint(
                "InternalError.Algo.InvalidParameter: Range of input length should be [1, 258048]"
            ),
            Some(258048)
        );
        assert_eq!(
            extract_context_window_limit_hint(
                "MindIE request failed: Range of prompt length should be [1, 12288]"
            ),
            Some(12288)
        );
        assert_eq!(
            extract_context_window_limit_hint(
                "提示词过长：最大上下文长度为 32768，当前请求 40012。"
            ),
            Some(32768)
        );
        assert_eq!(extract_context_window_limit_hint("network timeout"), None);
    }

    #[test]
    fn detects_llm_unavailable_transport_errors() {
        assert!(is_llm_unavailable_error_text(
            "error sending request for url (http://127.0.0.1:8001/v1/chat/completions)"
        ));
        assert!(is_llm_unavailable_error_text(
            "LLM stream request failed: 503 {\"error\":{\"message\":\"Loading model\"}}"
        ));
        assert!(is_llm_unavailable_error_text(
            "error decoding response body"
        ));
        assert!(is_llm_unavailable_error_text(
            "LLM stream request failed: 200 OK (read body failed: error decoding response body)"
        ));
        assert!(is_llm_unavailable_error_text("read llm response body"));
        assert!(is_llm_unavailable_error_text(
            "LLM request failed: 429 Too Many Requests"
        ));
        assert!(is_llm_unavailable_error_text(
            "LLM stream request failed: 500 Internal Server Error"
        ));
        assert!(matches!(
            classify_llm_failure("connection refused"),
            LlmFailureKind::Unavailable
        ));
        assert!(matches!(
            classify_llm_failure("error decoding response body"),
            LlmFailureKind::Unavailable
        ));
        assert!(matches!(
            classify_llm_failure("LLM request failed: 500 Internal Server Error"),
            LlmFailureKind::Unavailable
        ));
    }

    #[test]
    fn llm_unavailable_retries_use_floor_and_long_backoff() {
        let attempts = resolve_llm_max_attempts(LlmFailureKind::Unavailable);
        assert_eq!(attempts, LLM_UNAVAILABLE_MIN_RETRIES + 1);
        assert_eq!(
            resolve_llm_retry_delay(1, LlmFailureKind::Unavailable).as_millis(),
            1_200
        );
        assert_eq!(
            resolve_llm_retry_delay(5, LlmFailureKind::Unavailable).as_secs(),
            20
        );
        assert_eq!(
            llm_retry_reason(LlmFailureKind::Unavailable),
            "llm_unavailable"
        );
    }

    #[test]
    fn non_unavailable_llm_failures_use_fixed_internal_attempt_budget() {
        assert_eq!(
            resolve_llm_max_attempts(LlmFailureKind::Other),
            DEFAULT_LLM_MAX_ATTEMPTS
        );
        assert_eq!(
            resolve_llm_max_attempts(LlmFailureKind::ContextWindow),
            DEFAULT_LLM_MAX_ATTEMPTS
        );
    }
}
