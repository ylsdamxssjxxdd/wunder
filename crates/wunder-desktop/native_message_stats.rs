//! Numeric turn statistics shared by live native events and durable messages.
//! Presentation belongs to the frontend; never round values before projection.
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NativeMessageStats {
    pub duration_s: Option<f64>,
    pub speed_tps: Option<f64>,
    pub context_tokens: Option<f64>,
    pub consumed_tokens: Option<f64>,
    pub tool_calls: Option<f64>,
    /// Admitted model requests, matching the web bubble's quota counter.
    pub credits: Option<f64>,
}

impl NativeMessageStats {
    pub fn from_payload(stats: &Value) -> Self {
        let duration_s = number(
            stats,
            &[
                "interaction_duration_s",
                "interactionDurationS",
                "interactionDuration",
                "duration_s",
                "elapsed_s",
            ],
        )
        .or_else(|| {
            let prefill = number(stats, &["prefill_duration_total_s", "prefill_duration_s"]);
            let decode = number(stats, &["decode_duration_total_s", "decode_duration_s"]);
            (prefill.is_some() || decode.is_some())
                .then(|| prefill.unwrap_or_default() + decode.unwrap_or_default())
        });
        let explicit = number(
            stats,
            &[
                "request_consumed_tokens",
                "requestConsumedTokens",
                "consumed_tokens",
                "consumedTokens",
            ],
        )
        .or_else(|| {
            number(
                &stats["quotaConsumed"],
                &[
                    "request_consumed_tokens",
                    "consumed_tokens",
                    "consumed",
                    "used",
                    "count",
                ],
            )
        })
        .or_else(|| number(stats, &["quotaConsumed", "quota_consumed"]));
        let fallback = [
            "roundUsage",
            "round_usage",
            "round_usage_total",
            "billedUsage",
            "billed_usage",
            "usage",
        ]
        .into_iter()
        .find_map(|key| {
            number(
                &stats[key],
                &[
                    "total",
                    "total_tokens",
                    "totalTokens",
                    "input",
                    "input_tokens",
                ],
            )
            .or_else(|| numeric(&stats[key]))
        })
        .or_else(|| {
            number(
                stats,
                &[
                    "partialQuotaConsumed",
                    "partial_quota_consumed",
                    "partialConsumedTokens",
                    "partial_consumed_tokens",
                ],
            )
        });
        let credits = maximum(
            stats,
            &[
                "model_request_count",
                "modelRequestCount",
                "turn_request_count",
                "turnRequestCount",
            ],
        )
        .or_else(|| {
            number(
                stats,
                &["account_credits_consumed", "accountCreditsConsumed"],
            )
        })
        .or_else(|| {
            maximum(
                stats,
                &[
                    "creditsConsumed",
                    "credits_consumed",
                    "turn_quota_used",
                    "turnQuotaUsed",
                    "consumed",
                    "count",
                ],
            )
        });
        Self {
            duration_s: duration_s.filter(|value| *value > 0.0),
            speed_tps: speed(stats),
            context_tokens: number(
                stats,
                &[
                    "context_occupancy_tokens",
                    "contextOccupancyTokens",
                    "contextTokens",
                    "context_tokens",
                ],
            )
            .or_else(|| {
                number(
                    &stats["context_usage"],
                    &[
                        "context_occupancy_tokens",
                        "contextOccupancyTokens",
                        "contextTokens",
                        "context_tokens",
                    ],
                )
            }),
            consumed_tokens: explicit
                .filter(|value| *value > 1.0)
                .or_else(|| fallback.filter(|value| *value > 1.0)),
            tool_calls: number(stats, &["toolCalls", "tool_calls"]),
            credits,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.duration_s.is_none()
            && self.speed_tps.is_none()
            && self.context_tokens.is_none()
            && self.consumed_tokens.is_none()
            && self.tool_calls.is_none()
            && self.credits.is_none()
    }
}

fn numeric(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn number(stats: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| numeric(&stats[*key]))
}

fn maximum(stats: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .filter_map(|key| numeric(&stats[*key]))
        .reduce(f64::max)
        .map(f64::trunc)
}

fn speed(stats: &Value) -> Option<f64> {
    let positive = |keys: &[&str]| number(stats, keys).filter(|value| *value > 0.0);
    if positive(&["avg_model_round_speed_rounds", "avgModelRoundSpeedRounds"]).is_some() {
        if let Some(value) = positive(&[
            "avg_model_round_speed_tps",
            "avg_model_round_decode_speed_tps",
            "avgModelRoundSpeedTps",
            "avgModelRoundDecodeSpeedTps",
        ]) {
            return Some(value);
        }
    }
    let measured = stats
        .get("visible_decode_measured")
        .or_else(|| stats.get("visibleDecodeMeasured"))
        .and_then(Value::as_bool);
    if measured != Some(false) {
        if let Some(value) = positive(&["visible_decode_speed_tps", "visibleDecodeSpeedTps"]) {
            return Some(value);
        }
    }
    if let Some(value) = positive(&["decode_speed_tps", "decodeSpeedTps"]) {
        return Some(value);
    }
    let timing = &stats["stream_timing"];
    let visible_chars = number(timing, &["content_delta_chars", "contentDeltaChars"]);
    let reasoning_chars = number(timing, &["reasoning_delta_chars", "reasoningDeltaChars"]);
    let visible_metrics = [
        "visible_decode_tokens",
        "visibleDecodeTokens",
        "visible_decode_duration_s",
        "visibleDecodeDurationS",
    ]
    .iter()
    .any(|key| stats.get(*key).is_some());
    let decode_tokens = || {
        positive(&[
            "decode_output_tokens",
            "decodeOutputTokens",
            "decode_tokens",
            "decodeTokens",
        ])
    };
    let tokens = if measured == Some(false) {
        decode_tokens()
    } else {
        positive(&["visible_decode_tokens", "visibleDecodeTokens"])
    }
    .or_else(|| {
        ((visible_chars.is_some_and(|value| value > 0.0)
            && reasoning_chars.unwrap_or_default() <= 0.0)
            || (visible_chars.is_none() && reasoning_chars.is_none()))
        .then(decode_tokens)
        .flatten()
    })?;
    let duration = if measured == Some(false) {
        None
    } else {
        positive(&["visible_decode_duration_s", "visibleDecodeDurationS"])
    }
    .or_else(|| {
        (!visible_metrics || measured == Some(false))
            .then(|| positive(&["decode_duration_s", "decodeDurationS"]))
            .flatten()
    })
    .or_else(|| {
        number(timing, &["content_decode_ms", "decode_ms"])
            .filter(|value| *value > 0.0)
            .map(|value| value / 1000.0)
    })?;
    Some(tokens / duration).filter(|value| value.is_finite() && *value > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn turn_aggregate_and_request_count_win_over_last_round_and_account_exemption() {
        let stats = NativeMessageStats::from_payload(&json!({
            "avg_model_round_speed_rounds": 2, "avg_model_round_speed_tps": 42.75,
            "visible_decode_speed_tps": 90, "model_request_count": 4,
            "account_credits_consumed": 0, "context_occupancy_tokens": 0, "toolCalls": 0
        }));
        assert_eq!(stats.speed_tps, Some(42.75));
        assert_eq!(stats.credits, Some(4.0));
        assert_eq!(stats.context_tokens, Some(0.0));
        assert_eq!(stats.tool_calls, Some(0.0));
    }

    #[test]
    fn unmeasured_visible_tokens_never_borrow_an_earlier_rounds_duration() {
        let stats = NativeMessageStats::from_payload(&json!({
            "visible_decode_tokens": 40, "visible_decode_duration_s": 0,
            "decode_duration_s": 2, "prefill_duration_total_s": 3, "decode_duration_total_s": 4
        }));
        assert_eq!(stats.speed_tps, None);
        assert_eq!(stats.duration_s, Some(7.0));
        let stats = NativeMessageStats::from_payload(
            &json!({"decode_output_tokens": 60, "stream_timing": {"content_decode_ms": 2000}}),
        );
        assert_eq!(stats.speed_tps, Some(30.0));
    }
}
