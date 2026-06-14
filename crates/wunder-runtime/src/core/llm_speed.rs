use crate::schemas::TokenUsage;
use crate::token_utils::approx_token_count;
use chrono::DateTime;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const MIN_PREFILL_DURATION_S: f64 = 0.05;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LlmSpeedSummary {
    pub ttft_ms: Option<u64>,
    pub prefill_tokens: Option<i64>,
    pub prefill_duration_s: Option<f64>,
    pub prefill_speed_tps: Option<f64>,
    pub prefill_speed_lower_bound: bool,
    pub decode_tokens: Option<i64>,
    pub decode_duration_s: Option<f64>,
    pub decode_speed_tps: Option<f64>,
    pub decode_stream_chunk_tokens: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LlmSpeedEvent<'a> {
    pub event_type: &'a str,
    pub timestamp_s: Option<f64>,
    pub data: &'a Value,
}

#[derive(Debug, Default, Clone)]
struct LlmRoundMetrics {
    start_ts: Option<f64>,
    first_output_ts: Option<f64>,
    last_output_ts: Option<f64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    prefill_duration_s: Option<f64>,
    decode_duration_s: Option<f64>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct TurnDecodeSpeedAccumulator {
    prefill_duration_total_s: f64,
    has_prefill_duration: bool,
    decode_duration_total_s: f64,
    decode_tokens_total: u64,
    decode_speed_rounds: u32,
}

impl TurnDecodeSpeedAccumulator {
    pub(crate) fn record_summary(&mut self, summary: &LlmSpeedSummary) {
        if let Some(prefill_duration_s) = normalize_duration(summary.prefill_duration_s) {
            self.prefill_duration_total_s += prefill_duration_s;
            self.has_prefill_duration = true;
        }
        let decode_tokens = summary.decode_tokens.filter(|value| *value > 0);
        let decode_duration_s = normalize_duration(summary.decode_duration_s);
        if let (Some(tokens), Some(duration)) = (decode_tokens, decode_duration_s) {
            self.decode_tokens_total = self.decode_tokens_total.saturating_add(tokens as u64);
            self.decode_duration_total_s += duration;
            self.decode_speed_rounds = self.decode_speed_rounds.saturating_add(1);
        }
    }

    pub(crate) fn insert_into_map(&self, map: &mut Map<String, Value>) {
        let prefill_duration_total_s = self
            .has_prefill_duration
            .then_some(self.prefill_duration_total_s);
        let decode_duration_total_s =
            (self.decode_duration_total_s > 0.0).then_some(self.decode_duration_total_s);
        let avg_model_round_speed_tps =
            if self.decode_tokens_total > 0 && self.decode_duration_total_s > 0.0 {
                Some(self.decode_tokens_total as f64 / self.decode_duration_total_s)
            } else {
                None
            };
        map.insert(
            "prefill_duration_total_s".to_string(),
            json!(prefill_duration_total_s),
        );
        map.insert(
            "decode_duration_total_s".to_string(),
            json!(decode_duration_total_s),
        );
        map.insert(
            "avg_model_round_speed_tps".to_string(),
            json!(avg_model_round_speed_tps),
        );
        map.insert(
            "avg_model_round_speed_rounds".to_string(),
            json!(self.decode_speed_rounds),
        );
    }
}

impl LlmSpeedSummary {
    pub(crate) fn from_usage_and_durations(
        input_tokens: Option<u64>,
        decode_tokens: Option<u64>,
        prefill_duration_s: Option<f64>,
        decode_duration_s: Option<f64>,
    ) -> Self {
        let prefill_duration_s = normalize_prefill_duration(prefill_duration_s);
        let decode_duration_s = normalize_duration(decode_duration_s);
        let prefill_tokens = input_tokens
            .filter(|value| *value > 0)
            .map(|value| value.min(i64::MAX as u64) as i64);
        let decode_tokens = decode_tokens
            .filter(|value| *value > 0)
            .map(|value| value.min(i64::MAX as u64) as i64);
        let prefill_speed_tps = match (prefill_tokens, prefill_duration_s) {
            (Some(tokens), Some(duration)) if duration > 0.0 => Some(tokens as f64 / duration),
            _ => None,
        };
        let decode_speed_tps = match (decode_tokens, decode_duration_s) {
            (Some(tokens), Some(duration)) if duration > 0.0 => Some(tokens as f64 / duration),
            _ => None,
        };
        Self {
            ttft_ms: ttft_ms_from_duration(prefill_duration_s),
            prefill_tokens,
            prefill_duration_s,
            prefill_speed_tps,
            prefill_speed_lower_bound: false,
            decode_tokens,
            decode_duration_s,
            decode_speed_tps,
            decode_stream_chunk_tokens: None,
        }
    }

    pub(crate) fn from_session_payload(payload: Option<&Value>) -> Self {
        let Some(detail) = payload else {
            return Self::default();
        };
        let session = detail.get("session").unwrap_or(detail);
        let prefill_duration_s =
            normalize_prefill_duration(parse_f64_value(session.get("prefill_duration_s")));
        let decode_duration_s =
            normalize_duration(parse_f64_value(session.get("decode_duration_s")));
        let ttft_ms = parse_u64_value(session.get("ttft_ms"))
            .or_else(|| parse_u64_value(session.get("first_token_latency_ms")))
            .or_else(|| ttft_ms_from_duration(prefill_duration_s));
        let decode_stream_chunk_tokens = parse_i64_value(session.get("decode_stream_chunk_tokens"))
            .or_else(|| {
                detail
                    .get("events")
                    .and_then(Value::as_array)
                    .and_then(|events| count_stream_chunk_tokens(events))
            });
        Self {
            ttft_ms,
            prefill_tokens: parse_i64_value(session.get("prefill_tokens")),
            prefill_duration_s,
            prefill_speed_tps: parse_f64_value(session.get("prefill_speed_tps")),
            prefill_speed_lower_bound: session
                .get("prefill_speed_lower_bound")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            decode_tokens: parse_i64_value(session.get("decode_tokens")),
            decode_duration_s,
            decode_speed_tps: parse_f64_value(session.get("decode_speed_tps")),
            decode_stream_chunk_tokens,
        }
    }

    pub(crate) fn insert_into_map(&self, map: &mut Map<String, Value>) {
        map.insert("ttft_ms".to_string(), json!(self.ttft_ms));
        map.insert("prefill_tokens".to_string(), json!(self.prefill_tokens));
        map.insert(
            "prefill_duration_s".to_string(),
            json!(self.prefill_duration_s),
        );
        map.insert(
            "prefill_speed_tps".to_string(),
            json!(self.prefill_speed_tps),
        );
        map.insert(
            "prefill_speed_lower_bound".to_string(),
            json!(self.prefill_speed_lower_bound),
        );
        map.insert("decode_tokens".to_string(), json!(self.decode_tokens));
        map.insert(
            "decode_duration_s".to_string(),
            json!(self.decode_duration_s),
        );
        map.insert("decode_speed_tps".to_string(), json!(self.decode_speed_tps));
        map.insert(
            "decode_stream_chunk_tokens".to_string(),
            json!(self.decode_stream_chunk_tokens),
        );
    }

    pub(crate) fn merge_missing(&mut self, fallback: &Self) {
        if self.ttft_ms.is_none() {
            self.ttft_ms = fallback.ttft_ms;
        }
        if self.prefill_tokens.is_none() {
            self.prefill_tokens = fallback.prefill_tokens;
        }
        if self.prefill_duration_s.is_none() {
            self.prefill_duration_s = fallback.prefill_duration_s;
        }
        if self.prefill_speed_tps.is_none() {
            self.prefill_speed_tps = fallback.prefill_speed_tps;
        }
        if !self.prefill_speed_lower_bound {
            self.prefill_speed_lower_bound = fallback.prefill_speed_lower_bound;
        }
        if self.decode_tokens.is_none() {
            self.decode_tokens = fallback.decode_tokens;
        }
        if self.decode_duration_s.is_none() {
            self.decode_duration_s = fallback.decode_duration_s;
        }
        if self.decode_speed_tps.is_none() {
            self.decode_speed_tps = fallback.decode_speed_tps;
        }
        if self.decode_stream_chunk_tokens.is_none() {
            self.decode_stream_chunk_tokens = fallback.decode_stream_chunk_tokens;
        }
    }

    pub(crate) fn resolve_decode_tokens(&self, usage: Option<&TokenUsage>) -> Option<u64> {
        if let Some(tokens) = self.decode_tokens.filter(|value| *value > 0) {
            return Some(tokens as u64);
        }
        decode_tokens_from_usage(usage)
    }
}

pub(crate) fn build_llm_speed_summary(events: &[LlmSpeedEvent<'_>]) -> LlmSpeedSummary {
    let mut rounds: HashMap<i64, LlmRoundMetrics> = HashMap::new();
    let mut first_round: Option<i64> = None;
    let mut latest_round: Option<i64> = None;
    let mut last_round_seen: Option<i64> = None;
    let mut implicit_round: i64 = 0;
    let mut last_round_number: Option<i64> = None;
    let mut request_start_ts: Option<f64> = None;
    let mut decode_stream_chunk_tokens_total = 0_i64;

    fn reset_request(
        rounds: &mut HashMap<i64, LlmRoundMetrics>,
        first_round: &mut Option<i64>,
        latest_round: &mut Option<i64>,
        last_round_seen: &mut Option<i64>,
        implicit_round: &mut i64,
        last_round_number: &mut Option<i64>,
        request_start_ts: &mut Option<f64>,
        decode_stream_chunk_tokens_total: &mut i64,
    ) {
        rounds.clear();
        *first_round = None;
        *latest_round = None;
        *last_round_seen = None;
        *implicit_round = 0;
        *last_round_number = None;
        *request_start_ts = None;
        *decode_stream_chunk_tokens_total = 0;
    }

    for event in events {
        if is_request_boundary(event) {
            reset_request(
                &mut rounds,
                &mut first_round,
                &mut latest_round,
                &mut last_round_seen,
                &mut implicit_round,
                &mut last_round_number,
                &mut request_start_ts,
                &mut decode_stream_chunk_tokens_total,
            );
            request_start_ts = event.timestamp_s;
            continue;
        }

        if matches!(
            event.event_type,
            "llm_request" | "llm_output_delta" | "llm_output" | "token_usage"
        ) {
            if let Some(round_value) = parse_model_round(event.data) {
                if let Some(last_round) = last_round_number {
                    if round_value < last_round {
                        reset_request(
                            &mut rounds,
                            &mut first_round,
                            &mut latest_round,
                            &mut last_round_seen,
                            &mut implicit_round,
                            &mut last_round_number,
                            &mut request_start_ts,
                            &mut decode_stream_chunk_tokens_total,
                        );
                        request_start_ts = event.timestamp_s;
                    }
                }
                last_round_number = Some(round_value);
            }
        }

        match event.event_type {
            "llm_request" => {
                let round = parse_model_round(event.data).unwrap_or_else(|| {
                    implicit_round += 1;
                    implicit_round
                });
                last_round_seen = Some(round);
                if first_round.is_none() {
                    first_round = Some(round);
                }
                let entry = rounds.entry(round).or_default();
                if entry.start_ts.is_none() {
                    entry.start_ts = event.timestamp_s;
                }
            }
            "llm_output_delta" => {
                if let Some(tokens) = estimate_stream_chunk_tokens(event.data) {
                    decode_stream_chunk_tokens_total =
                        decode_stream_chunk_tokens_total.saturating_add(tokens);
                }
                let round = parse_model_round(event.data).or(last_round_seen);
                if let Some(round) = round {
                    last_round_seen = Some(round);
                    let entry = rounds.entry(round).or_default();
                    if entry.first_output_ts.is_none() {
                        entry.first_output_ts = event.timestamp_s;
                    }
                    entry.last_output_ts = event.timestamp_s;
                }
            }
            "llm_output" => {
                let round = parse_model_round(event.data).or(last_round_seen);
                if let Some(round) = round {
                    last_round_seen = Some(round);
                    let entry = rounds.entry(round).or_default();
                    if entry.first_output_ts.is_none() {
                        entry.first_output_ts = event.timestamp_s;
                    }
                    entry.last_output_ts = event.timestamp_s;
                    let (input_tokens, output_tokens) = parse_usage_tokens(event.data);
                    let decode_output_tokens =
                        parse_decode_output_tokens(event.data).or(output_tokens);
                    if entry.input_tokens.is_none() {
                        entry.input_tokens = input_tokens;
                    }
                    if let Some(tokens) = decode_output_tokens {
                        if entry.output_tokens.is_none_or(|current| tokens > current) {
                            entry.output_tokens = Some(tokens);
                        }
                    }
                    if entry.prefill_duration_s.is_none() {
                        entry.prefill_duration_s =
                            parse_f64_value(event.data.get("prefill_duration_s"));
                    }
                    if entry.decode_duration_s.is_none() {
                        entry.decode_duration_s =
                            parse_f64_value(event.data.get("decode_duration_s"));
                    }
                    if entry.output_tokens.is_some() {
                        latest_round = Some(round);
                    }
                }
            }
            "token_usage" => {
                let round = parse_model_round(event.data).or(last_round_seen);
                if let Some(round) = round {
                    last_round_seen = Some(round);
                    let entry = rounds.entry(round).or_default();
                    if entry.input_tokens.is_none() {
                        entry.input_tokens = parse_i64_value(event.data.get("input_tokens"));
                    }
                    let decode_output_tokens = parse_decode_output_tokens(event.data)
                        .or_else(|| parse_i64_value(event.data.get("output_tokens")));
                    if let Some(tokens) = decode_output_tokens {
                        if entry.output_tokens.is_none_or(|current| tokens > current) {
                            entry.output_tokens = Some(tokens);
                        }
                    }
                    if entry.prefill_duration_s.is_none() {
                        entry.prefill_duration_s =
                            parse_f64_value(event.data.get("prefill_duration_s"));
                    }
                    if entry.decode_duration_s.is_none() {
                        entry.decode_duration_s =
                            parse_f64_value(event.data.get("decode_duration_s"));
                    }
                    if entry.output_tokens.is_some() {
                        latest_round = Some(round);
                    }
                }
            }
            _ => {}
        }
    }

    let mut earliest_start_ts: Option<f64> = None;
    let mut earliest_output_ts: Option<f64> = None;
    let mut earliest_output_round: Option<i64> = None;
    let mut latest_output_ts: Option<f64> = None;
    let mut output_tokens_total: i64 = 0;
    let mut decode_duration_total = 0.0;
    let mut has_decode_duration = false;
    for (round, metrics) in &rounds {
        if let Some(start_ts) = metrics.start_ts {
            earliest_start_ts = Some(match earliest_start_ts {
                Some(value) => value.min(start_ts),
                None => start_ts,
            });
        }
        if let Some(first_output_ts) = metrics.first_output_ts {
            let should_update = match earliest_output_ts {
                Some(value) => first_output_ts < value,
                None => true,
            };
            if should_update {
                earliest_output_ts = Some(first_output_ts);
                earliest_output_round = Some(*round);
            }
        }
        if let Some(last_output_ts) = metrics.last_output_ts {
            latest_output_ts = Some(match latest_output_ts {
                Some(value) => value.max(last_output_ts),
                None => last_output_ts,
            });
        }
        if let Some(tokens) = metrics.output_tokens {
            if tokens > 0 {
                output_tokens_total = output_tokens_total.saturating_add(tokens);
            }
        }
        let decode_duration = normalize_duration(metrics.decode_duration_s);
        if let Some(duration) = decode_duration {
            decode_duration_total += duration;
            has_decode_duration = true;
        } else if let (Some(first_output_ts), Some(last_output_ts)) =
            (metrics.first_output_ts, metrics.last_output_ts)
        {
            let duration = (last_output_ts - first_output_ts).max(0.0);
            if duration > 0.0 {
                decode_duration_total += duration;
                has_decode_duration = true;
            }
        }
    }

    let prefill_round = earliest_output_round
        .or(first_round)
        .or_else(|| rounds.keys().copied().min());
    let decode_round = latest_round
        .or(last_round_seen)
        .or_else(|| rounds.keys().copied().max());
    let prefill_metrics = prefill_round.and_then(|round| rounds.get(&round));
    let prefill_tokens = prefill_metrics.and_then(|metrics| metrics.input_tokens);
    let mut prefill_duration_s = prefill_metrics.and_then(|metrics| metrics.prefill_duration_s);
    let mut prefill_speed_lower_bound = false;
    let mut start_ts = prefill_metrics
        .and_then(|metrics| metrics.start_ts)
        .or(earliest_start_ts);
    if let Some(request_start_ts) = request_start_ts {
        start_ts = Some(match start_ts {
            Some(value) => value.min(request_start_ts),
            None => request_start_ts,
        });
    }
    let first_output_ts = prefill_metrics
        .and_then(|metrics| metrics.first_output_ts)
        .or(earliest_output_ts);
    if let (Some(start_ts), Some(first_output_ts)) = (start_ts, first_output_ts) {
        let observed_duration = (first_output_ts - start_ts).max(0.0);
        if prefill_duration_s.is_none_or(|provided| observed_duration > provided) {
            prefill_duration_s = Some(observed_duration);
            prefill_speed_lower_bound = true;
        }
    }
    let prefill_duration_s = normalize_prefill_duration(prefill_duration_s);
    let prefill_speed_tps = match (prefill_tokens, prefill_duration_s) {
        (Some(tokens), Some(duration)) if tokens > 0 && duration > 0.0 => {
            Some(tokens as f64 / duration)
        }
        _ => None,
    };

    let decode_metrics = decode_round.and_then(|round| rounds.get(&round));
    let decode_tokens = if output_tokens_total > 0 {
        Some(output_tokens_total)
    } else {
        decode_metrics.and_then(|metrics| metrics.output_tokens)
    };
    let mut decode_duration_s = if has_decode_duration && decode_duration_total > 0.0 {
        Some(decode_duration_total)
    } else {
        match (earliest_output_ts, latest_output_ts) {
            (Some(start), Some(end)) => {
                let duration = (end - start).max(0.0);
                (duration > 0.0).then_some(duration)
            }
            _ => None,
        }
    };
    if decode_duration_s.is_none() {
        decode_duration_s = decode_metrics
            .and_then(|metrics| normalize_duration(metrics.decode_duration_s))
            .or_else(|| {
                decode_metrics.and_then(|metrics| {
                    let first_output_ts = metrics.first_output_ts?;
                    let last_output_ts = metrics.last_output_ts?;
                    let duration = (last_output_ts - first_output_ts).max(0.0);
                    (duration > 0.0).then_some(duration)
                })
            });
    }
    let decode_duration_s = normalize_duration(decode_duration_s);
    let decode_speed_tps = match (decode_tokens, decode_duration_s) {
        (Some(tokens), Some(duration)) if tokens > 0 && duration > 0.0 => {
            Some(tokens as f64 / duration)
        }
        _ => None,
    };
    let decode_stream_chunk_tokens =
        (decode_stream_chunk_tokens_total > 0).then_some(decode_stream_chunk_tokens_total);

    LlmSpeedSummary {
        ttft_ms: ttft_ms_from_duration(prefill_duration_s),
        prefill_tokens,
        prefill_duration_s,
        prefill_speed_tps,
        prefill_speed_lower_bound,
        decode_tokens,
        decode_duration_s,
        decode_speed_tps,
        decode_stream_chunk_tokens,
    }
}

pub(crate) fn build_llm_speed_summary_from_value_events(events: &[Value]) -> LlmSpeedSummary {
    let normalized_events = events
        .iter()
        .map(|event| LlmSpeedEvent {
            event_type: event.get("type").and_then(Value::as_str).unwrap_or(""),
            timestamp_s: parse_event_timestamp(event.get("timestamp")),
            data: event.get("data").unwrap_or(&Value::Null),
        })
        .collect::<Vec<_>>();
    build_llm_speed_summary(&normalized_events)
}

pub(crate) fn count_stream_chunk_tokens(events: &[Value]) -> Option<i64> {
    let mut total = 0_i64;
    for event in events {
        if event.get("type").and_then(Value::as_str) != Some("llm_output_delta") {
            continue;
        }
        let data = event.get("data").unwrap_or(&Value::Null);
        if let Some(tokens) = estimate_stream_chunk_tokens(data) {
            total = total.saturating_add(tokens);
        }
    }
    (total > 0).then_some(total)
}

pub(crate) fn ttft_ms_from_duration(duration_s: Option<f64>) -> Option<u64> {
    normalize_duration(duration_s).map(|duration| (duration * 1000.0).round() as u64)
}

fn decode_tokens_from_usage(usage: Option<&TokenUsage>) -> Option<u64> {
    let usage = usage?;
    let decode_tokens = usage.total.saturating_sub(usage.input);
    if decode_tokens > 0 {
        return Some(decode_tokens);
    }
    if usage.output > 0 {
        return Some(usage.output);
    }
    None
}

fn is_request_boundary(event: &LlmSpeedEvent<'_>) -> bool {
    if matches!(event.event_type, "round_start" | "received") {
        return true;
    }
    if event.event_type == "progress" {
        return event
            .data
            .get("stage")
            .and_then(Value::as_str)
            .is_some_and(|stage| stage == "start");
    }
    false
}

fn parse_i64_value(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(Value::as_i64)
        .or_else(|| value.and_then(Value::as_u64).map(|value| value as i64))
}

fn parse_u64_value(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64).or_else(|| {
        value
            .and_then(Value::as_i64)
            .and_then(|value| (value >= 0).then_some(value as u64))
    })
}

fn parse_f64_value(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .or_else(|| value.and_then(Value::as_i64).map(|value| value as f64))
        .or_else(|| value.and_then(Value::as_u64).map(|value| value as f64))
}

fn parse_event_timestamp(value: Option<&Value>) -> Option<f64> {
    if let Some(timestamp) = parse_f64_value(value) {
        return Some(timestamp);
    }
    let text = value.and_then(Value::as_str)?;
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|dt| dt.timestamp_millis() as f64 / 1000.0)
}

fn normalize_duration(value: Option<f64>) -> Option<f64> {
    let value = value?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    Some(value)
}

fn normalize_prefill_duration(value: Option<f64>) -> Option<f64> {
    let value = normalize_duration(value)?;
    if value < MIN_PREFILL_DURATION_S {
        return Some(MIN_PREFILL_DURATION_S);
    }
    Some(value)
}

fn parse_model_round(data: &Value) -> Option<i64> {
    parse_i64_value(data.get("model_round"))
}

fn parse_usage_tokens(data: &Value) -> (Option<i64>, Option<i64>) {
    let Some(usage) = data.get("usage").and_then(Value::as_object) else {
        return (None, None);
    };
    (
        parse_i64_value(usage.get("input_tokens")),
        parse_i64_value(usage.get("output_tokens")),
    )
}

fn parse_decode_output_tokens(data: &Value) -> Option<i64> {
    parse_i64_value(data.get("decode_output_tokens")).or_else(|| {
        data.get("usage")
            .and_then(Value::as_object)
            .and_then(|usage| parse_i64_value(usage.get("decode_output_tokens")))
    })
}

fn estimate_stream_chunk_tokens(data: &Value) -> Option<i64> {
    let parse_non_empty_text = |key: &str| {
        data.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    };
    let mut tokens = 0_i64;
    if let Some(text) = parse_non_empty_text("delta").or_else(|| parse_non_empty_text("content")) {
        tokens = tokens.saturating_add(approx_token_count(text).max(0));
    }
    if let Some(text) =
        parse_non_empty_text("reasoning_delta").or_else(|| parse_non_empty_text("reasoning"))
    {
        tokens = tokens.saturating_add(approx_token_count(text).max(0));
    }
    if tokens > 0 {
        return Some(tokens);
    }
    let has_stream_output_chunk = |value: Option<&Value>| -> bool {
        value
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
    };
    if has_stream_output_chunk(data.get("delta"))
        || has_stream_output_chunk(data.get("reasoning_delta"))
        || has_stream_output_chunk(data.get("reasoning"))
    {
        return Some(1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        build_llm_speed_summary, ttft_ms_from_duration, LlmSpeedEvent, LlmSpeedSummary,
        TurnDecodeSpeedAccumulator,
    };
    use serde_json::{json, Map, Value};

    #[test]
    fn build_summary_prefers_decode_output_tokens() {
        let request = json!({ "model_round": 1 });
        let usage = json!({
            "model_round": 1,
            "input_tokens": 100,
            "output_tokens": 20,
            "decode_output_tokens": 120,
            "prefill_duration_s": 0.5,
            "decode_duration_s": 2.0
        });
        let events = vec![
            LlmSpeedEvent {
                event_type: "llm_request",
                timestamp_s: Some(1.0),
                data: &request,
            },
            LlmSpeedEvent {
                event_type: "token_usage",
                timestamp_s: Some(3.0),
                data: &usage,
            },
        ];
        let summary = build_llm_speed_summary(&events);
        assert_eq!(summary.ttft_ms, Some(500));
        assert_eq!(summary.prefill_tokens, Some(100));
        assert_eq!(summary.decode_tokens, Some(120));
        assert_eq!(summary.decode_duration_s, Some(2.0));
        assert_eq!(summary.decode_speed_tps, Some(60.0));
    }

    #[test]
    fn build_summary_uses_observed_ttft_for_blocking_output() {
        let request = json!({ "model_round": 1 });
        let output = json!({
            "model_round": 1,
            "usage": {
                "input_tokens": 64,
                "output_tokens": 8,
                "total_tokens": 72
            },
            "decode_output_tokens": 8
        });
        let events = vec![
            LlmSpeedEvent {
                event_type: "llm_request",
                timestamp_s: Some(1.0),
                data: &request,
            },
            LlmSpeedEvent {
                event_type: "llm_output",
                timestamp_s: Some(4.0),
                data: &output,
            },
        ];
        let summary = build_llm_speed_summary(&events);
        assert_eq!(summary.ttft_ms, Some(3000));
        assert_eq!(summary.prefill_duration_s, Some(3.0));
        assert_eq!(summary.prefill_tokens, Some(64));
        assert_eq!(summary.decode_duration_s, None);
        assert_eq!(summary.decode_speed_tps, None);
        assert!(summary.prefill_speed_lower_bound);
    }

    #[test]
    fn turn_accumulator_inserts_backend_average_decode_speed() {
        let mut accumulator = TurnDecodeSpeedAccumulator::default();
        accumulator.record_summary(&LlmSpeedSummary::from_usage_and_durations(
            Some(128),
            Some(100),
            Some(0.4),
            Some(2.0),
        ));
        accumulator.record_summary(&LlmSpeedSummary::from_usage_and_durations(
            Some(96),
            Some(50),
            Some(0.6),
            Some(1.0),
        ));
        let mut map = Map::<String, Value>::new();
        accumulator.insert_into_map(&mut map);
        assert_eq!(
            map.get("prefill_duration_total_s").and_then(Value::as_f64),
            Some(1.0)
        );
        assert_eq!(
            map.get("decode_duration_total_s").and_then(Value::as_f64),
            Some(3.0)
        );
        assert_eq!(
            map.get("avg_model_round_speed_tps").and_then(Value::as_f64),
            Some(50.0)
        );
        assert_eq!(
            map.get("avg_model_round_speed_rounds")
                .and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(ttft_ms_from_duration(Some(0.123)), Some(123));
    }
}
