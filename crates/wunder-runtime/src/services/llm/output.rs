use super::{disable_thinking_requested, resolved_max_output, resolved_thinking_token_budget};
use crate::config::LlmModelConfig;
use crate::schemas::TokenUsage;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct OutputDiagnostics {
    pub finish_reason: Option<String>,
    pub output_limit_reached: bool,
    pub max_output: u32,
    pub thinking_token_budget: Option<u32>,
    pub thinking_disabled: bool,
}

impl OutputDiagnostics {
    pub(crate) fn new(
        config: &LlmModelConfig,
        finish_reason: Option<String>,
        usage: &TokenUsage,
    ) -> Self {
        let max_output = resolved_max_output(config);
        // Normalized output excludes reasoning; estimated usage cannot prove exhaustion.
        let output_limit_reached = matches!(
            finish_reason.as_deref(),
            Some("length" | "max_tokens" | "max_output_tokens")
        ) || (!usage.estimated
            && usage.output.saturating_add(usage.reasoning.unwrap_or(0)) >= u64::from(max_output));
        Self {
            finish_reason,
            output_limit_reached,
            max_output,
            thinking_token_budget: resolved_thinking_token_budget(config),
            thinking_disabled: disable_thinking_requested(config),
        }
    }

    pub(crate) fn insert_into(&self, payload: &mut Value) {
        if let (Some(target), Ok(Value::Object(fields))) =
            (payload.as_object_mut(), serde_json::to_value(self))
        {
            target.extend(fields);
        }
    }
}

// One extractor covers Chat Completions, Anthropic and Responses, including SSE envelopes.
pub(super) fn finish_reason(payload: &Value) -> Option<String> {
    let body = payload.get("response").unwrap_or(payload);
    [
        payload.pointer("/choices/0/finish_reason"),
        payload.pointer("/delta/stop_reason"),
        body.pointer("/incomplete_details/reason"),
        body.get("finish_reason"),
        body.get("stop_reason"),
        body.get("stopReason"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .map(str::trim)
    .find(|reason| !reason.is_empty())
    .map(|reason| reason.chars().take(64).collect())
    .or_else(|| {
        (body.get("status").and_then(Value::as_str) == Some("completed"))
            .then(|| "stop".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finish_reason_supports_all_transports() {
        for payload in [
            json!({"choices":[{"finish_reason":"length"}]}),
            json!({"stop_reason":"length"}),
            json!({"delta":{"stop_reason":"length"}}),
            json!({"response":{"status":"incomplete","incomplete_details":{"reason":"length"}}}),
        ] {
            assert_eq!(finish_reason(&payload).as_deref(), Some("length"));
        }
        assert_eq!(finish_reason(&json!({"usage":{}})), None);
    }

    #[test]
    fn output_limit_includes_reasoning_without_trusting_estimates() {
        let config = LlmModelConfig {
            max_output: Some(100),
            ..Default::default()
        };
        let mut usage = TokenUsage {
            output: 20,
            reasoning: Some(80),
            ..Default::default()
        };
        assert!(OutputDiagnostics::new(&config, None, &usage).output_limit_reached);
        usage.estimated = true;
        assert!(!OutputDiagnostics::new(&config, None, &usage).output_limit_reached);
        assert!(
            OutputDiagnostics::new(&config, Some("length".into()), &usage).output_limit_reached
        );
    }

    #[tokio::test]
    async fn stream_retains_finish_reason_across_usage_only_tail() {
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut usage = None;
        let mut calls = Vec::new();
        let mut state = super::super::StreamOutputState::default();
        let mut callback = |_, _| std::future::ready(Ok(()));
        for payload in [
            json!({"choices":[{"delta":{"reasoning_content":"analysis"},"finish_reason":"length"}]}),
            json!({"choices":[],"usage":{"completion_tokens":8192}}),
        ] {
            super::super::process_stream_payload(
                &payload.to_string(),
                &mut content,
                &mut reasoning,
                &mut usage,
                &mut calls,
                &mut state,
                &mut callback,
            )
            .await
            .unwrap();
        }
        assert_eq!(
            (
                state.finish_reason.as_deref(),
                content.as_str(),
                reasoning.as_str()
            ),
            (Some("length"), "", "analysis")
        );
    }

    #[tokio::test]
    async fn responses_incomplete_preserves_usage_and_stop_reason() {
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut usage = None;
        let mut calls = Vec::new();
        let mut state = super::super::StreamOutputState::default();
        let mut callback = |_, _| std::future::ready(Ok(()));
        let payload = json!({"type":"response.incomplete", "response":{
            "status":"incomplete", "incomplete_details":{"reason":"max_output_tokens"},
            "usage":{"input_tokens":10,"output_tokens":20}, "output":[]
        }});
        assert!(super::super::process_stream_payload(
            &payload.to_string(),
            &mut content,
            &mut reasoning,
            &mut usage,
            &mut calls,
            &mut state,
            &mut callback
        )
        .await
        .unwrap());
        assert_eq!(
            (state.finish_reason.as_deref(), usage.unwrap().output),
            (Some("max_output_tokens"), 20)
        );
    }
}
