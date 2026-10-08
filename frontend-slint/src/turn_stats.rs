//! Per-turn statistics footer (§7.3 A).
//!
//! Every finished turn carries a `message_stats` object: the runtime builds it
//! in `build_persisted_message_stats` and the desktop façade either resolves it
//! onto `NativeMessage::stats_*` (durable history) or hands the raw object to
//! the live stream. Both paths land here, so the row a user sees while the
//! answer streams and the row they see after a restart are built by the same
//! code from the same keys.
//!
//! The timeline used to render none of this: `update_active_stats` computed a
//! duration that nothing read, and the history projection ignored `stats_*`
//! entirely. Keeping the projection in one function is what stops the two from
//! drifting again.
//!
//! A footer is a list of metric entries rather than one joined string because
//! the row is icon + value (no labels), matching the web messenger's
//! `MessageStats.vue`: the label survives only as the tooltip.

use serde_json::Value;

/// One rendered metric: the value plus the icon key `ToolIcons.image` maps to an
/// SVG, and the label used as the hover tooltip.
#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    /// `duration` | `speed` | `context` | `quota` | `tools`
    pub key: &'static str,
    /// Icon key understood by `ui/tool_icons.slint`.
    pub icon: &'static str,
    /// Display value, already abbreviated (`12.4s`, `68.2k`, `3`).
    pub value: String,
}

/// Icon per metric, mirroring the web component so the two front ends agree on
/// what each number looks like.
fn icon_for(key: &str) -> &'static str {
    match key {
        "duration" => "stopwatch",
        "speed" => "gauge-high",
        "context" => "layer-group",
        "quota" => "bolt",
        "tools" => "screwdriver-wrench",
        _ => "wrench",
    }
}

fn metric(key: &'static str, value: String) -> Metric {
    Metric {
        key,
        icon: icon_for(key),
        value,
    }
}

/// Metrics from the raw `message_stats` object the runtime emits.
///
/// Order follows the web row: duration, speed, context occupancy, consumed
/// tokens, tool-call count. Anything the runtime did not report is left out
/// rather than shown as a dash.
pub fn metrics(message_stats: &Value) -> Vec<Metric> {
    let mut out = Vec::new();

    if let Some(seconds) =
        number(message_stats, &["interaction_duration_s", "duration_s", "elapsed_s"])
            .filter(|value| *value > 0.0)
    {
        out.push(metric("duration", duration(seconds)));
    }
    if let Some(speed) = speed(message_stats) {
        out.push(metric("speed", format!("{}/s", rate(speed))));
    }
    // 上下文占用: how full the window is, not how much this turn spent.
    if let Some(context) = number(
        message_stats,
        &["contextTokens", "context_occupancy_tokens", "context_tokens"],
    )
    .filter(|value| *value > 0.0)
    {
        out.push(metric("context", count(context)));
    }
    // 上下文消耗: what this turn consumed.
    if let Some(quota) = consumed_tokens(message_stats).filter(|value| *value > 0.0) {
        out.push(metric("quota", count(quota)));
    }
    if let Some(tools) =
        number(message_stats, &["toolCalls", "tool_calls"]).filter(|value| *value > 0.0)
    {
        out.push(metric("tools", count(tools)));
    }

    out
}

/// Metrics from the façade's resolved `NativeMessage::stats_*` fields.
///
/// Durable history hands back display strings rather than the raw object, so the
/// reload path formats the numbers they carry instead of re-parsing JSON. The
/// façade's own suffixes are stripped first, which is what keeps the reloaded
/// row identical to the streamed one.
pub fn metrics_from_parts(
    duration_value: &str,
    speed: &str,
    context: &str,
    quota: &str,
    tools: &str,
) -> Vec<Metric> {
    let mut out = Vec::new();

    if let Some(seconds) = façade_duration(duration_value) {
        out.push(metric("duration", self::duration(seconds)));
    }
    if let Some(value) = number_text(speed).filter(|value| *value > 0.0) {
        out.push(metric("speed", format!("{}/s", rate(value))));
    }
    if let Some(value) = number_text(context).filter(|value| *value > 0.0) {
        out.push(metric("context", count(value)));
    }
    if let Some(value) = number_text(quota).filter(|value| *value > 0.0) {
        out.push(metric("quota", count(value)));
    }
    if let Some(value) = number_text(tools).filter(|value| *value > 0.0) {
        out.push(metric("tools", count(value)));
    }

    out
}

/// Seconds as `12.4s`, or `2m 05s` past a minute.
pub fn duration(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{}m {:02}s", (seconds / 60.0).floor(), (seconds % 60.0).round())
    }
}

/// Decode speed in tokens/second. The runtime reports a per-round average when
/// several rounds streamed and a visible decode speed otherwise; prefer the
/// average exactly as the thread log's overview does.
fn speed(stats: &Value) -> Option<f64> {
    let rounds = number(stats, &["avg_model_round_speed_rounds", "avgModelRoundSpeedRounds"])
        .unwrap_or_default();
    if rounds > 0.0 {
        number(
            stats,
            &["avg_model_round_speed_tps", "avg_model_round_decode_speed_tps"],
        )
        .filter(|speed| *speed > 0.0)
        .or_else(|| number(stats, &["visible_decode_speed_tps", "decode_speed_tps"]))
    } else {
        number(stats, &["visible_decode_speed_tps", "decode_speed_tps"])
    }
}

/// Tokens consumed by the turn. Mirrors the façade: the turn's own consumption
/// first, then the last round's usage.
fn consumed_tokens(stats: &Value) -> Option<f64> {
    if let Some(value) = number(stats, &["request_consumed_tokens", "consumed_tokens"]) {
        return Some(value);
    }
    let round = stats.get("round_usage").or_else(|| stats.get("usage"))?;
    number(round, &["total_tokens", "total"])
}

fn rate(value: f64) -> String {
    if value >= 1000.0 {
        format!("{:.1}k", value / 1000.0)
    } else {
        format!("{value:.1}")
    }
}

fn count(value: f64) -> String {
    if value >= 1_000_000.0 {
        format!("{:.1}m", value / 1_000_000.0)
    } else if value >= 1000.0 {
        format!("{:.1}k", value / 1000.0)
    } else {
        format!("{value:.0}")
    }
}

/// Reads the leading number out of a façade-formatted metric, ignoring any unit
/// it appended (`68234.0/s`, `4.1k`, `12.4s`). Returns `None` for the façade's
/// "not reported" placeholders.
fn number_text(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() || value == "—" {
        return None;
    }
    // Thousands suffixes are part of the façade's own formatting.
    let (digits, scale) = match value.strip_suffix('m') {
        Some(rest) => (rest, 1_000_000.0),
        None => match value.strip_suffix('k') {
            Some(rest) => (rest, 1_000.0),
            None => (value, 1.0),
        },
    };
    let end = digits
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(digits.len());
    digits[..end].parse::<f64>().ok().map(|value| value * scale)
}

/// Seconds from a façade duration, which is either `12.4s` or `2m 05s`.
fn façade_duration(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() || value == "—" {
        return None;
    }
    match value.split_once('m') {
        // `2m 05s`
        Some((minutes, rest)) => {
            let seconds = rest.trim().trim_end_matches('s').trim();
            Some(minutes.trim().parse::<f64>().ok()? * 60.0 + seconds.parse::<f64>().ok()?)
        }
        // `12.4s` or a bare number of seconds
        None => number_text(value),
    }
}

/// Looks a numeric field up across a flat object and its known nested holders
/// (`stats`, `usage`, `round_usage`, `context_usage`), which is where the
/// runtime puts the same numbers in different event shapes.
pub fn number(stats: &Value, keys: &[&str]) -> Option<f64> {
    let direct = keys
        .iter()
        .find_map(|key| stats.get(*key).and_then(Value::as_f64));
    if direct.is_some() {
        return direct;
    }
    for holder in ["stats", "usage", "round_usage", "context_usage"] {
        let Some(nested) = stats.get(holder) else {
            continue;
        };
        if let Some(value) = keys
            .iter()
            .find_map(|key| nested.get(*key).and_then(Value::as_f64))
        {
            return Some(value);
        }
    }
    None
}

/// Digs the `message_stats` object out of an event payload, whether it sits at
/// the top level or under the payload's `meta`.
pub fn message_stats(payload: &Value) -> Option<&Value> {
    payload
        .get("message_stats")
        .or_else(|| payload.get("meta").and_then(|meta| meta.get("message_stats")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(metrics: &[Metric]) -> Vec<(&str, &str)> {
        metrics
            .iter()
            .map(|metric| (metric.key, metric.value.as_str()))
            .collect()
    }

    fn keys(metrics: &[Metric]) -> Vec<&str> {
        metrics.iter().map(|metric| metric.key).collect()
    }

    #[test]
    fn a_full_turn_reports_the_web_rows_five_metrics_in_order() {
        let stats = json!({
            "interaction_duration_s": 12.44,
            "request_consumed_tokens": 4096,
            "toolCalls": 3,
            "contextTokens": 12345,
            "avg_model_round_speed_rounds": 2.0,
            "avg_model_round_speed_tps": 68234.0,
        });
        let metrics = metrics(&stats);
        assert_eq!(
            keys(&metrics),
            vec!["duration", "speed", "context", "quota", "tools"],
        );
        assert_eq!(
            values(&metrics),
            vec![
                ("duration", "12.4s"),
                ("speed", "68.2k/s"),
                ("context", "12.3k"),
                ("quota", "4.1k"),
                ("tools", "3"),
            ],
        );
        // Icon keys must match what the web component uses for the same numbers.
        assert_eq!(
            metrics.iter().map(|m| m.icon).collect::<Vec<_>>(),
            vec!["stopwatch", "gauge-high", "layer-group", "bolt", "screwdriver-wrench"],
        );
    }

    /// Context occupancy and consumed tokens are different numbers and must not
    /// be conflated: one is how full the window is, the other what the turn cost.
    #[test]
    fn context_occupancy_and_consumed_tokens_stay_separate() {
        let stats = json!({
            "contextTokens": 120000,
            "request_consumed_tokens": 900,
        });
        let metrics = metrics(&stats);
        assert_eq!(
            values(&metrics),
            vec![("context", "120.0k"), ("quota", "900")],
        );
    }

    /// A turn with no statistics must render nothing: a row of dashes is worse
    /// than no row.
    #[test]
    fn an_empty_payload_produces_no_metrics() {
        assert!(metrics(&json!({})).is_empty());
        assert!(metrics(&json!({"model_request_count": 1})).is_empty());
    }

    #[test]
    fn zero_valued_counters_are_left_out() {
        let stats = json!({
            "interaction_duration_s": 2.0,
            "toolCalls": 0,
            "contextTokens": 0
        });
        assert_eq!(values(&metrics(&stats)), vec![("duration", "2.0s")]);
    }

    /// The live stream and the durable reload describe the same numbers in
    /// different shapes, so both have to resolve.
    #[test]
    fn nested_usage_shapes_resolve_the_same_numbers() {
        let nested = json!({
            "round_usage": {"total_tokens": 2048},
            "visible_decode_speed_tps": 41.5
        });
        assert_eq!(
            values(&metrics(&nested)),
            vec![("speed", "41.5/s"), ("quota", "2.0k")],
        );

        let flat = json!({
            "interaction_duration_s": 3.0,
            "decode_speed_tps": 10.0,
            "usage": {"total": 100}
        });
        assert_eq!(
            values(&metrics(&flat)),
            vec![("duration", "3.0s"), ("speed", "10.0/s"), ("quota", "100")],
        );
    }

    /// The durable path hands over the façade's resolved fields; they must
    /// produce exactly what the live object produces.
    #[test]
    fn resolved_facade_fields_match_the_live_object() {
        let live = metrics(&json!({
            "interaction_duration_s": 12.44,
            "request_consumed_tokens": 4096,
            "toolCalls": 3,
            "contextTokens": 12345,
            "avg_model_round_speed_rounds": 2.0,
            "avg_model_round_speed_tps": 68234.0,
        }));
        // What the façade actually hands back for the same turn: `format_speed`
        // appends `/s`, the counts are raw numbers.
        let reloaded = metrics_from_parts("12.4s", "68234.0/s", "12345", "4096", "3");
        assert_eq!(live, reloaded);
    }

    /// A turn with nothing reported yields no row, and the façade's em dash
    /// placeholders do not leak into one.
    #[test]
    fn missing_facade_parts_never_produce_a_dash_row() {
        assert!(metrics_from_parts("", "", "", "", "").is_empty());
        assert!(metrics_from_parts("—", "—", "", "", "").is_empty());
        assert_eq!(
            values(&metrics_from_parts("", "", "", "4100", "")),
            vec![("quota", "4.1k")],
        );
        assert_eq!(
            values(&metrics_from_parts("2m 05s", "", "", "", "12")),
            vec![("duration", "2m 05s"), ("tools", "12")],
        );
        assert_eq!(
            values(&metrics_from_parts("2.0s", "", "", "", "0")),
            vec![("duration", "2.0s")],
        );
    }

    #[test]
    fn the_same_event_round_trips_through_both_message_stats_holders() {
        let stats = json!({"interaction_duration_s": 1.0});
        assert!(message_stats(&json!({"message_stats": stats.clone()})).is_some());
        assert!(message_stats(&json!({"meta": {"message_stats": stats}})).is_some());
        assert!(message_stats(&json!({"other": true})).is_none());
    }
}
