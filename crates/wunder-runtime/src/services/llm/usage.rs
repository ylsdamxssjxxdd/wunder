use crate::schemas::TokenUsage;
use serde_json::{Map, Value};

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

/// Read the first present nested numeric field. Providers disagree on whether
/// the cache breakdown lives at the top level or under a `*_details` object.
fn nested_count(map: &Map<String, Value>, parents: &[&str], children: &[&str]) -> Option<u64> {
    for parent in parents {
        let Some(details) = map.get(*parent).and_then(Value::as_object) else {
            continue;
        };
        for child in children {
            if let Some(value) = count(details.get(*child)) {
                return Some(value);
            }
        }
    }
    None
}

/// Split a provider usage object into three disjoint prompt buckets:
/// `input` (uncached), `cache_read`, `cache_write`.
///
/// Anthropic reports uncached input separately, so its `input_tokens` is used
/// as-is. OpenAI-compatible providers and DeepSeek fold cached tokens into
/// `prompt_tokens`; the cached counts are therefore *subsets* and must be
/// subtracted rather than added. Keeping the buckets disjoint is what makes
/// `cache_hit_rate()` meaningful and what lets the UI sum them without double
/// counting.
pub(super) fn normalize_usage(raw: Option<&Value>) -> Option<TokenUsage> {
    let map = raw?.as_object()?;
    let responses_input_details = map
        .get("input_tokens_details")
        .or_else(|| map.get("inputTokensDetails"))
        .and_then(Value::as_object);
    let responses_cache_read = nested_count(
        map,
        &["input_tokens_details", "inputTokensDetails"],
        &["cached_tokens", "cachedTokens"],
    );
    let inclusive_cached_input = count(map.get("cached_input_tokens"));
    let cache_read = count(map.get("cache_read_input_tokens"))
        .or(inclusive_cached_input)
        .or_else(|| count(map.get("prompt_cache_hit_tokens")))
        .or_else(|| count(map.get("prompt_cache_hit")))
        .or(responses_cache_read)
        .or_else(|| {
            nested_count(
                map,
                &["prompt_tokens_details", "promptTokensDetails"],
                &["cached_tokens", "cachedTokens"],
            )
        })
        .unwrap_or(0);
    let cache_write = count(map.get("cache_creation_input_tokens"))
        .or_else(|| count(map.get("cacheCreationInputTokens")))
        .or_else(|| count(map.get("cache_write_tokens")))
        .or_else(|| {
            nested_count(
                map,
                &[
                    "input_tokens_details",
                    "inputTokensDetails",
                    "prompt_tokens_details",
                    "promptTokensDetails",
                ],
                &["cache_write_tokens", "cacheWriteTokens"],
            )
        })
        .unwrap_or(0);
    let (input, cache_read, cache_write, prompt_total) = match (
        count(map.get("input_tokens")),
        count(map.get("prompt_tokens")),
    ) {
        // Responses reports an inclusive input total. Its cache details can
        // be nested under input_tokens_details or exposed as cached_input_tokens.
        (Some(total), _)
            if responses_input_details.is_some() || inclusive_cached_input.is_some() =>
        {
            let cache_read = cache_read.min(total);
            let cache_write = cache_write.min(total.saturating_sub(cache_read));
            let uncached = total.saturating_sub(cache_read).saturating_sub(cache_write);
            (uncached, cache_read, cache_write, total)
        }
        // Anthropic: `input_tokens` already excludes both cache buckets.
        (Some(uncached), _) => {
            let prompt_total = uncached
                .saturating_add(cache_read)
                .saturating_add(cache_write);
            (uncached, cache_read, cache_write, prompt_total)
        }
        // OpenAI-compatible / DeepSeek: `prompt_tokens` includes the cache
        // buckets, so carve them out and clamp to the reported total.
        (None, Some(total)) => {
            let cache_read = cache_read.min(total);
            let cache_write = cache_write.min(total.saturating_sub(cache_read));
            let uncached = total.saturating_sub(cache_read).saturating_sub(cache_write);
            (uncached, cache_read, cache_write, total)
        }
        (None, None) => (0, cache_read, cache_write, cache_read + cache_write),
    };
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
        Some(total) => total.max(prompt_total.saturating_add(raw_output)),
        None => prompt_total.saturating_add(raw_output.max(reasoning.unwrap_or(0))),
    };
    (total > 0 || prompt_total > 0).then_some(TokenUsage {
        input,
        output: raw_output.saturating_sub(reasoning.unwrap_or(0)),
        reasoning,
        total,
        cache_read,
        cache_write,
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
        // The prompt buckets only arrive on message_start, so a delta must not
        // reset them to zero: doing so would erase the cache breakdown that the
        // hit-rate metric is derived from.
        if raw.get("input_tokens").is_none() && raw.get("prompt_tokens").is_none() {
            next.input = previous.input;
            next.cache_read = previous.cache_read;
            next.cache_write = previous.cache_write;
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
        next.total = next
            .total
            .max(next.prompt_tokens().saturating_add(next.output));
    }
    *target = Some(next);
}

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
                cache_read: 0,
                cache_write: 0,
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
                cache_read: 0,
                cache_write: 0,
                estimated: false,
            })
        );
    }

    #[test]
    fn anthropic_keeps_cache_buckets_disjoint_from_input() {
        let usage = normalize_usage(Some(&json!({
            "input_tokens": 10,
            "cache_read_input_tokens": 80,
            "cache_creation_input_tokens": 10,
            "output_tokens": 20
        })))
        .expect("usage");
        assert_eq!(usage.input, 10);
        assert_eq!(usage.cache_read, 80);
        assert_eq!(usage.cache_write, 10);
        assert_eq!(usage.prompt_tokens(), 100);
        assert_eq!(usage.total, 120);
        assert_eq!(usage.cache_hit_rate(), Some(0.8));
    }

    #[test]
    fn openai_cached_tokens_are_carved_out_of_prompt_tokens() {
        let usage = normalize_usage(Some(&json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "prompt_tokens_details": {"cached_tokens": 80}
        })))
        .expect("usage");
        // 80 of the 100 prompt tokens were cached; nothing is double counted.
        assert_eq!(usage.input, 20);
        assert_eq!(usage.cache_read, 80);
        assert_eq!(usage.prompt_tokens(), 100);
        assert_eq!(usage.total, 120);
        assert_eq!(usage.cache_hit_rate(), Some(0.8));
    }

    #[test]
    fn responses_cached_tokens_are_carved_out_of_input_tokens() {
        let usage = normalize_usage(Some(&json!({
            "input_tokens": 100,
            "input_tokens_details": {"cached_tokens": 80, "cache_write_tokens": 5},
            "output_tokens": 20,
            "total_tokens": 120
        })))
        .expect("usage");
        assert_eq!(usage.input, 15);
        assert_eq!(usage.cache_read, 80);
        assert_eq!(usage.cache_write, 5);
        assert_eq!(usage.prompt_tokens(), 100);
        assert_eq!(usage.total, 120);
        assert_eq!(usage.cache_hit_rate(), Some(0.8));
    }

    #[test]
    fn responses_top_level_cached_input_is_not_added_twice() {
        let usage = normalize_usage(Some(&json!({
            "input_tokens": 100,
            "cached_input_tokens": 60,
            "output_tokens": 20
        })))
        .expect("usage");
        assert_eq!(usage.input, 40);
        assert_eq!(usage.cache_read, 60);
        assert_eq!(usage.prompt_tokens(), 100);
        assert_eq!(usage.total, 120);
    }

    #[test]
    fn deepseek_cache_hit_tokens_are_carved_out_of_prompt_tokens() {
        let usage = normalize_usage(Some(&json!({
            "prompt_tokens": 1000,
            "completion_tokens": 30,
            "prompt_cache_hit_tokens": 960,
            "prompt_cache_miss_tokens": 40
        })))
        .expect("usage");
        assert_eq!(usage.input, 40);
        assert_eq!(usage.cache_read, 960);
        assert_eq!(usage.prompt_tokens(), 1000);
        assert_eq!(usage.cache_hit_rate(), Some(0.96));
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
                input: 10,
                output: 20,
                total: 120,
                reasoning: None,
                cache_read: 80,
                cache_write: 10,
                estimated: false
            })
        );
        // OpenAI reports the same prompt total with cached tokens folded in:
        // 20 uncached + 80 cached == the 100 tokens Anthropic split as 10+80+10
        // (the extra 10 are this request's own cache write).
        assert_eq!(
            normalize_usage(Some(&json!({"prompt_tokens": 100, "completion_tokens": 20,
            "prompt_tokens_details": {"cached_tokens": 80}}))),
            Some(TokenUsage {
                input: 20,
                output: 20,
                total: 120,
                reasoning: None,
                cache_read: 80,
                cache_write: 0,
                estimated: false,
            })
        );
    }
}
