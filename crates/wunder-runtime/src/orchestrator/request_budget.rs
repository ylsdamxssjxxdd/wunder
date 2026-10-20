//! Local admission is deliberately separate from provider-reported occupancy.
//! Estimates gate outgoing work; they must never be persisted as billed usage.
use super::*;
use crate::orchestrator_constants::DEFAULT_MAX_OUTPUT_TOKENS;
use serde::Serialize;
use std::io::{self, Write};

#[derive(Debug, Clone, Copy)]
pub(super) struct RequestBudget {
    pub estimated_tokens: i64,
    pub limit_tokens: i64,
    pub output_reserve_tokens: i64,
    pub safety_reserve_tokens: i64,
}

#[derive(Default)]
struct ByteCounter(usize);

impl Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn serialized_tokens<T: Serialize + ?Sized>(value: &T) -> i64 {
    let mut counter = ByteCounter::default();
    if serde_json::to_writer(&mut counter, value).is_err() {
        return i64::MAX;
    }
    i64::try_from(counter.0.saturating_add(3) / 4).unwrap_or(i64::MAX)
}

pub(super) fn tools_tokens(tools: Option<&[Value]>) -> i64 {
    tools.map(serialized_tokens).unwrap_or(0)
}

pub(super) fn estimate_request_tokens(messages: &[ChatMessage], tools: Option<&[Value]>) -> i64 {
    messages
        .iter()
        .fold(tools_tokens(tools).saturating_add(16), |total, message| {
            let reasoning = message
                .reasoning_content
                .as_deref()
                .map(approx_token_count)
                .unwrap_or(0);
            let calls = message
                .tool_calls
                .as_ref()
                .map(serialized_tokens)
                .unwrap_or(0);
            let call_id = message
                .tool_call_id
                .as_deref()
                .map(approx_token_count)
                .unwrap_or(0);
            total
                .saturating_add(crate::token_utils::estimate_content_tokens(
                    &message.content,
                ))
                .saturating_add(reasoning)
                .saturating_add(calls)
                .saturating_add(call_id)
                .saturating_add(4)
        })
}

impl RequestBudget {
    pub(super) fn measure(
        config: &LlmModelConfig,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
    ) -> Self {
        let max_context = i64::from(config.effective_max_context());
        // Keep this identical to the output limit sent by the provider adapter.
        // A configured output reserve is never silently capped to half a window.
        let output_reserve_tokens = config
            .max_output
            .filter(|value| *value > 0)
            .map(i64::from)
            .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS);
        let safety_reserve_tokens = (max_context / 32).clamp(64, 4096).min(max_context / 8);
        Self {
            estimated_tokens: estimate_request_tokens(messages, tools),
            limit_tokens: max_context
                .saturating_sub(output_reserve_tokens)
                .saturating_sub(safety_reserve_tokens)
                .max(0),
            output_reserve_tokens,
            safety_reserve_tokens,
        }
    }

    pub(super) fn admitted(self) -> bool {
        self.estimated_tokens <= self.limit_tokens
    }

    pub(super) fn ensure_admitted(self) -> Result<Self, OrchestratorError> {
        if self.admitted() {
            return Ok(self);
        }
        let error = OrchestratorError::new("CONTEXT_WINDOW_EXCEEDED", format!(
            "Request context estimate {} exceeds input budget {} (output reserve {}, safety reserve {}). Reduce tool definitions or compact conversation history.",
            self.estimated_tokens, self.limit_tokens, self.output_reserve_tokens, self.safety_reserve_tokens,
        ), Some(json!({
            "source":"local_request_admission",
            "estimated_tokens":self.estimated_tokens,
            "input_budget_tokens":self.limit_tokens,
            "output_reserve_tokens":self.output_reserve_tokens,
            "safety_reserve_tokens":self.safety_reserve_tokens,
        })));
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: &str, content: Value) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content,
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn request_budget_counts_tools_calls_reasoning_and_images() {
        let config = LlmModelConfig {
            max_context: Some(4096),
            max_output: Some(256),
            ..Default::default()
        };
        let mut messages = vec![
            message("system", json!("fixed prefix")),
            message("user", json!("input")),
        ];
        let baseline = RequestBudget::measure(&config, &messages, None);
        let mut call = message("assistant", Value::Null);
        call.reasoning_content = Some("reasoning".repeat(32));
        call.tool_calls =
            Some(json!([{"id":"call_1","function":{"name":"sample","arguments":"x".repeat(256)}}]));
        messages.push(call);
        messages.push(message("tool", json!("observation".repeat(32))));
        messages.push(message(
            "user",
            json!([{"type":"image_url","image_url":{"url":"data:image/png;base64,AA=="}}]),
        ));
        let tools = [
            json!({"type":"function","function":{"name":"sample","description":"d".repeat(4096)}}),
        ];
        let with_tools = RequestBudget::measure(&config, &messages, Some(&tools));
        assert!(with_tools.estimated_tokens > baseline.estimated_tokens + 1400);
        assert_eq!(with_tools.output_reserve_tokens, 256);
        assert!(with_tools.admitted());
    }

    #[test]
    fn request_budget_rejects_oversize_first_request_and_large_tool_sets() {
        let config = LlmModelConfig {
            max_context: Some(4096),
            max_output: Some(256),
            ..Default::default()
        };
        let messages = [message("user", json!("字🙂".repeat(4096)))];
        let error = RequestBudget::measure(&config, &messages, None)
            .ensure_admitted()
            .unwrap_err();
        assert_eq!(error.code(), "CONTEXT_WINDOW_EXCEEDED");
        let messages = [message("user", json!("input"))];
        let tools = [json!({"description":"x".repeat(20_000)})];
        assert!(!RequestBudget::measure(&config, &messages, Some(&tools)).admitted());
    }

    #[test]
    fn request_budget_uses_exact_output_reserve_and_serialized_utf8_cost() {
        let config = LlmModelConfig {
            max_context: Some(1024),
            max_output: Some(900),
            ..Default::default()
        };
        let messages = [message("user", json!("input"))];
        let budget = RequestBudget::measure(&config, &messages, None);
        assert_eq!(budget.output_reserve_tokens, 900);
        assert!(budget.limit_tokens < 124);
        let value = json!({"text":"字🙂\"\n".repeat(100)});
        assert_eq!(
            serialized_tokens(&value),
            approx_token_count(&value.to_string())
        );
    }
}
