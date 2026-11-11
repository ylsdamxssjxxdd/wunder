use serde_json::Value;
use wunder_core::storage_records::ThreadSpeedTotals;

pub(crate) fn speed_totals_from_assistant_preview(preview: &str) -> Option<ThreadSpeedTotals> {
    let value: Value = serde_json::from_str(preview).ok()?;
    let payload = value.get("payload").unwrap_or(&value);
    let meta = payload.get("meta").or_else(|| value.get("meta"));
    let stats = payload
        .get("message_stats")
        .or_else(|| payload.get("messageStats"))
        .or_else(|| meta.and_then(|meta| meta.get("message_stats")))
        .or_else(|| meta.and_then(|meta| meta.get("messageStats")))
        .or_else(|| value.get("message_stats"))
        .or_else(|| value.get("messageStats"));
    let usage = stats
        .and_then(|stats| stats.get("usage"))
        .or_else(|| payload.get("usage"))
        .or_else(|| value.get("usage"));
    let timing = stats
        .and_then(|stats| stats.get("stream_timing"))
        .or_else(|| payload.get("stream_timing"))
        .or_else(|| value.get("stream_timing"));

    fn number(source: Option<&Value>, keys: &[&str]) -> Option<f64> {
        let source = source?;
        keys.iter().find_map(|key| {
            let value = source.get(*key)?;
            value
                .as_f64()
                .or_else(|| value.as_i64().map(|number| number as f64))
                .or_else(|| value.as_u64().map(|number| number as f64))
                .or_else(|| {
                    value
                        .as_str()
                        .and_then(|text| text.trim().parse::<f64>().ok())
                })
                .filter(|number| number.is_finite() && *number >= 0.0)
        })
    }

    let prefill_tokens = number(usage, &["input_tokens", "prefill_tokens"])
        .or_else(|| number(stats, &["prefill_tokens"]))
        .or_else(|| number(Some(payload), &["prefill_tokens"]))
        .map(|number| number.trunc() as i64)
        .unwrap_or(0);
    let prefill_duration_s = number(stats, &["prefill_duration_s"])
        .or_else(|| number(Some(payload), &["prefill_duration_s"]))
        .or_else(|| number(timing, &["prefill_ms"]).map(|ms| ms / 1000.0))
        .unwrap_or(0.0);
    let decode_tokens = number(
        stats,
        &[
            "decode_output_tokens",
            "decode_tokens",
            "visible_decode_tokens",
        ],
    )
    .or_else(|| number(usage, &["output_tokens"]))
    .or_else(|| {
        number(
            Some(payload),
            &["decode_output_tokens", "decode_tokens", "output_tokens"],
        )
    })
    .map(|number| number.trunc() as i64)
    .unwrap_or(0);
    let decode_duration_s = number(stats, &["decode_duration_s", "visible_decode_duration_s"])
        .or_else(|| {
            number(
                Some(payload),
                &["decode_duration_s", "visible_decode_duration_s"],
            )
        })
        .or_else(|| number(timing, &["content_decode_ms", "decode_ms"]).map(|ms| ms / 1000.0))
        .unwrap_or(0.0);

    if prefill_duration_s <= 0.0 && decode_duration_s <= 0.0 {
        return None;
    }
    Some(ThreadSpeedTotals {
        prefill_tokens: prefill_tokens.max(0),
        prefill_duration_s: prefill_duration_s.max(0.0),
        decode_tokens: decode_tokens.max(0),
        decode_duration_s: decode_duration_s.max(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::speed_totals_from_assistant_preview;
    use serde_json::json;

    #[test]
    fn reads_persisted_message_stats() {
        let preview = json!({
            "kind": "assistant_message",
            "payload": {
                "kind": "assistant_message",
                "prefill_duration_s": 50.5,
                "message_stats": {
                    "prefill_duration_s": 50.5,
                    "decode_output_tokens": 59,
                    "stream_timing": {"decode_ms": 120},
                    "usage": {"input_tokens": 101000, "output_tokens": 59},
                    "ttft_ms": 50500
                }
            }
        });
        let totals = speed_totals_from_assistant_preview(&preview.to_string()).unwrap();
        assert_eq!(totals.prefill_tokens, 101000);
        assert!((totals.prefill_duration_s - 50.5).abs() < f64::EPSILON);
        assert_eq!(totals.decode_tokens, 59);
        assert!((totals.decode_duration_s - 0.12).abs() < f64::EPSILON);
    }

    #[test]
    fn falls_back_to_item_stream_timing() {
        let preview = json!({
            "payload": {
                "kind": "assistant_message",
                "prefill_duration_s": 0.5,
                "decode_output_tokens": 20,
                "stream_timing": {"decode_ms": 2000}
            }
        });
        let totals = speed_totals_from_assistant_preview(&preview.to_string()).unwrap();
        assert_eq!(totals.prefill_tokens, 0);
        assert_eq!(totals.decode_tokens, 20);
        assert!((totals.decode_duration_s - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ignores_items_without_timing() {
        assert!(speed_totals_from_assistant_preview(
            &json!({"payload": {"kind": "assistant_message"}}).to_string()
        )
        .is_none());
    }
}
