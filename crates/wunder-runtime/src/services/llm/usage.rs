use crate::schemas::TokenUsage;
use serde_json::Value;

fn count(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn reasoning_count(value: Option<&Value>) -> Option<u64> {
    let details = value?.as_object()?;
    count(details.get("reasoning_tokens"))
        .or_else(|| count(details.get("reasoningTokens")))
        .or_else(|| {
            let reasoning = details.get("reasoning")?.as_object()?;
            count(reasoning.get("tokens")).or_else(|| count(reasoning.get("token_count")))
        })
}

pub(super) fn normalize_usage(raw: Option<&Value>) -> Option<TokenUsage> {
    let map = raw?.as_object()?;
    // Anthropic reports cache reads/writes separately from uncached input.
    // OpenAI cached_tokens is already a subset of prompt_tokens; do not add it.
    let input = count(map.get("input_tokens"))
        .or_else(|| count(map.get("prompt_tokens")))
        .unwrap_or(0)
        .saturating_add(count(map.get("cache_read_input_tokens")).unwrap_or(0))
        .saturating_add(count(map.get("cache_creation_input_tokens")).unwrap_or(0));
    let raw_output = count(map.get("output_tokens"))
        .or_else(|| count(map.get("completion_tokens")))
        .unwrap_or(0);
    // A top-level reasoning count is sometimes reported separately from
    // completion_tokens (including when completion_tokens is zero). Nested
    // provider details are treated as a subset of raw output tokens.
    let reasoning = count(map.get("reasoning_tokens"))
        .or_else(|| count(map.get("reasoningTokens")))
        .or_else(|| {
            reasoning_count(map.get("output_tokens_details")).map(|tokens| tokens.min(raw_output))
        })
        .or_else(|| {
            reasoning_count(map.get("outputTokensDetails")).map(|tokens| tokens.min(raw_output))
        })
        .or_else(|| {
            reasoning_count(map.get("completion_tokens_details"))
                .map(|tokens| tokens.min(raw_output))
        })
        .or_else(|| {
            reasoning_count(map.get("completionTokensDetails")).map(|tokens| tokens.min(raw_output))
        });
    let total = match count(map.get("total_tokens")) {
        Some(total) => total.max(input.saturating_add(raw_output)),
        None => input.saturating_add(raw_output.max(reasoning.unwrap_or(0))),
    };
    (total > 0).then_some(TokenUsage {
        input,
        output: raw_output.saturating_sub(reasoning.unwrap_or(0)),
        reasoning,
        total,
        estimated: false,
    })
}

pub(super) fn merge_anthropic_usage(target: &mut Option<TokenUsage>, raw: Option<&Value>) {
    let Some(raw) = raw else { return };
    let Some(mut next) = normalize_usage(Some(raw)).or_else(|| {
        raw.get("output_tokens")
            .and_then(Value::as_u64)
            .map(|_| TokenUsage::default())
    }) else {
        return;
    };
    if let Some(previous) = target.as_ref() {
        // message_delta contains cumulative output, not a new complete usage object.
        if raw.get("input_tokens").is_none() && raw.get("prompt_tokens").is_none() {
            next.input = previous.input;
        }
        if raw.get("output_tokens").is_none() && raw.get("completion_tokens").is_none() {
            next.output = previous.output;
            next.reasoning = previous.reasoning;
        } else if next.output.saturating_add(next.reasoning.unwrap_or(0))
            < previous
                .output
                .saturating_add(previous.reasoning.unwrap_or(0))
        {
            next.output = previous.output;
            next.reasoning = previous.reasoning;
        }
        next.total = next.total.max(
            next.input
                .saturating_add(next.output)
                .saturating_add(next.reasoning.unwrap_or(0)),
        );
    }
    *target = Some(next);
}

#[derive(Debug)]
pub(super) struct FailedResponseUsage {
    pub usage: TokenUsage,
    pub message: &'static str,
}

impl std::fmt::Display for FailedResponseUsage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for FailedResponseUsage {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reasoning_is_explicit_and_not_counted_twice() {
        assert_eq!(
            normalize_usage(Some(&json!({"prompt_tokens": 100, "completion_tokens": 40,
            "completion_tokens_details": {"reasoning_tokens": 30}}))),
            Some(TokenUsage {
                input: 100,
                output: 10,
                total: 140,
                reasoning: Some(30),
                estimated: false,
            })
        );
        assert_eq!(
            normalize_usage(Some(&json!({"input_tokens": 10, "output_tokens": 30,
            "total_tokens": 1, "reasoning_tokens": 30}))),
            Some(TokenUsage {
                input: 10,
                output: 0,
                total: 40,
                reasoning: Some(30),
                estimated: false,
            })
        );
    }

    #[test]
    fn streaming_cache_input_survives_cumulative_output_updates() {
        let mut usage = None;
        merge_anthropic_usage(
            &mut usage,
            Some(&json!({"input_tokens": 10,
            "cache_read_input_tokens": 80, "cache_creation_input_tokens": 10, "output_tokens": 0})),
        );
        for output in [5, 20, 20] {
            merge_anthropic_usage(&mut usage, Some(&json!({"output_tokens": output})));
        }
        assert_eq!(
            usage,
            Some(TokenUsage {
                input: 100,
                output: 20,
                total: 120,
                reasoning: None,
                estimated: false
            })
        );
        assert_eq!(
            normalize_usage(Some(&json!({"prompt_tokens": 100, "completion_tokens": 20,
            "prompt_tokens_details": {"cached_tokens": 80}}))),
            usage
        );
    }
}
