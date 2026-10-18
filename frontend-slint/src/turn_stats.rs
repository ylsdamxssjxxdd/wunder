//! Per-turn statistics footer (§7.3 A).
//!
//! Every finished turn carries a `message_stats` object: the runtime builds it
//! in `build_persisted_message_stats` and the desktop façade either resolves it
//! onto `NativeMessage::stats` (durable history) or hands the raw object to
//! the live stream. Both paths land here, so the row a user sees while the
//! answer streams and the row they see after a restart are built by the same
//! code from the same keys.
//!
//! The timeline used to render none of this: `update_active_stats` computed a
//! duration that nothing read, and the history projection ignored the statistics
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
    /// A web-compatible footer metric or user-round status.
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
        "round" => "hashtag",
        "credits" => "coins",
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

/// The runtime and history both resolve numeric values through the native façade.
pub fn metrics(message_stats: &Value) -> Vec<Metric> {
    metrics_from_native(&wunder_desktop::native::NativeMessageStats::from_payload(
        message_stats,
    ))
}

pub fn metrics_from_native(stats: &wunder_desktop::native::NativeMessageStats) -> Vec<Metric> {
    if stats.is_empty() {
        return Vec::new();
    }
    vec![
        metric(
            "duration",
            stats.duration_s.map(duration).unwrap_or_else(|| "-".into()),
        ),
        metric(
            "speed",
            stats
                .speed_tps
                .map(|value| format!("{value:.1} token/s"))
                .unwrap_or_else(|| "-".into()),
        ),
        metric(
            "context",
            stats
                .context_tokens
                .map(count)
                .unwrap_or_else(|| "--".into()),
        ),
        metric(
            "quota",
            stats
                .consumed_tokens
                .map(count)
                .unwrap_or_else(|| "--".into()),
        ),
        metric(
            "tools",
            stats.tool_calls.map(count).unwrap_or_else(|| "--".into()),
        ),
        metric(
            "credits",
            stats.credits.map(count).unwrap_or_else(|| "--".into()),
        ),
    ]
}

/// The user round is a thread-wide ordinal from the runtime, not a model round
/// or an index in the bounded history window.
pub fn with_user_round(mut metrics: Vec<Metric>, round: i64) -> Vec<Metric> {
    if round > 0 {
        metrics.insert(0, metric("round", round.to_string()));
    }
    metrics
}

/// Web shows the current execution state in place of the settled user-round
/// status. Numeric usage already observed is retained alongside it.
pub fn with_status(metrics: Vec<Metric>, state: &str) -> Vec<Metric> {
    // Runtime states can carry an informative suffix (for example, queue
    // position).  The footer should retain that useful detail while selecting
    // its presentation from the same status family as the web messenger.
    let state = state.trim();
    let (key, label) = match state {
        "已停止" | "cancelled" | "canceled" | "interrupted" | "stopped" => {
            ("status-stopped", "已停止")
        }
        "执行失败" | "failed" | "error" | "rejected" => ("status-error", "任务失败"),
        "等待用户输入" | "waiting_input" | "waiting_user_input" | "等待确认"
        | "waiting_approval" => ("status-waiting", "等待确认"),
        "正在执行工具…" | "使用工具中" | "tool_running" | "tooling" => {
            ("status-tool", "使用工具中")
        }
        "子智能体执行中" | "subagent_running" => ("status-subagent", "子智能体执行中"),
        "正在思考…" | "正在生成…" | "模型输出中" | "model_outputting" | "streaming" => {
            ("status-outputting", "模型输出中")
        }
        "模型响应中" | "请求中" | "requesting" | "waiting_first_output" => {
            ("status-requesting", "请求中")
        }
        "上下文压缩中…" | "上下文压缩中" | "compacting" => {
            ("status-compacting", "上下文压缩中")
        }
        "正在排队" | "任务已排队…" | "queued" | "pending" => ("status-queued", "排队中"),
        "" | "任务完成" | "输出已结束" | "completed" | "done" | "final" => return metrics,
        _ if state.starts_with("模型请求正在排队")
            || state.starts_with("正在排队")
            || state.starts_with("任务已排队") =>
        {
            ("status-queued", state)
        }
        _ if state.starts_with("正在执行工具") => ("status-tool", "使用工具中"),
        _ if state.starts_with("上下文压缩中") => ("status-compacting", "上下文压缩中"),
        _ if state.starts_with("模型响应中") => ("status-requesting", "请求中"),
        _ if state.starts_with("正在思考") || state.starts_with("正在生成") => {
            ("status-outputting", "模型输出中")
        }
        // A goal execution is still a request owned by this assistant bubble.
        // Do not leak its internal scheduling phrase into the reader-facing
        // status line.
        _ if state.starts_with("目标继续执行") => ("status-requesting", "请求中"),
        _ => ("status-running", "处理中"),
    };
    let mut result = vec![Metric {
        key: "status",
        icon: key,
        value: label.into(),
    }];
    result.extend(metrics.into_iter().filter(|metric| metric.key != "round"));
    result
}

/// Child activity only overrides an active parent request/output/tool phase.
/// Terminal, waiting, queued and compacting states retain their precedence.
pub fn resolve_status(state: &str, has_running_subagent: bool) -> &str {
    if has_running_subagent
        && matches!(
            with_status(Vec::new(), state)
                .first()
                .map(|metric| metric.icon),
            Some("status-running" | "status-requesting" | "status-outputting" | "status-tool")
        )
    {
        "subagent_running"
    } else {
        state
    }
}

/// Match the web footer's precision for seconds, minutes and hours.
pub fn duration(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor() as u64;
    let hours = minutes / 60;
    if hours > 0 {
        format!("{hours}h {}m {:.1}s", minutes % 60, seconds % 60.0)
    } else if minutes > 0 {
        format!("{minutes}m {:.1}s", seconds % 60.0)
    } else {
        format!("{seconds:.1}s")
    }
}

fn count(value: f64) -> String {
    let value = value.trunc();
    if value < 1000.0 {
        return format!("{value:.0}");
    }
    let (scaled, unit) = if value >= 1_000_000.0 {
        (value / 1_000_000.0, "m")
    } else {
        (value / 1000.0, "k")
    };
    let text = if scaled >= 100.0 {
        format!("{scaled:.0}")
    } else {
        format!("{scaled:.1}")
    };
    format!("{}{unit}", text.strip_suffix(".0").unwrap_or(&text))
}

/// Digs the `message_stats` object out of an event payload, whether it sits at
/// the top level or under the payload's `meta`.
pub fn message_stats(payload: &Value) -> Option<&Value> {
    payload.get("message_stats").or_else(|| {
        payload
            .get("meta")
            .and_then(|meta| meta.get("message_stats"))
    })
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

    #[test]
    fn complete_footer_matches_web_order_units_and_round_identity() {
        let stats = json!({
            "interaction_duration_s": 3725.25, "request_consumed_tokens": 4096,
            "toolCalls": 0, "context_occupancy_tokens": 120000,
            "avg_model_round_speed_rounds": 2, "avg_model_round_speed_tps": 68234,
            "model_request_count": 4, "account_credits_consumed": 0
        });
        let native = wunder_desktop::native::NativeMessageStats::from_payload(&stats);
        let live = with_user_round(metrics(&stats), 57);
        assert_eq!(live, with_user_round(metrics_from_native(&native), 57));
        assert_eq!(
            values(&live),
            vec![
                ("round", "57"),
                ("duration", "1h 2m 5.2s"),
                ("speed", "68234.0 token/s"),
                ("context", "120k"),
                ("quota", "4.1k"),
                ("tools", "0"),
                ("credits", "4")
            ]
        );
    }

    #[test]
    fn zero_and_missing_are_distinct_and_unreported_metrics_stay_readable() {
        assert!(metrics(&json!({})).is_empty());
        assert_eq!(
            values(&metrics(&json!({"toolCalls": 0, "contextTokens": 0}))),
            vec![
                ("duration", "-"),
                ("speed", "-"),
                ("context", "0"),
                ("quota", "--"),
                ("tools", "0"),
                ("credits", "--")
            ]
        );
        assert_eq!(duration(125.4), "2m 5.4s");
        assert_eq!(count(2048.0), "2k");
    }

    #[test]
    fn the_same_event_round_trips_through_both_message_stats_holders() {
        let stats = json!({"interaction_duration_s": 1.0});
        assert!(message_stats(&json!({"message_stats": stats.clone()})).is_some());
        assert!(message_stats(&json!({"meta": {"message_stats": stats}})).is_some());
    }

    #[test]
    fn child_activity_respects_terminal_and_waiting_status_priority() {
        for state in [
            "stopped",
            "error",
            "waiting_approval",
            "queued",
            "compacting",
            "completed",
        ] {
            assert_eq!(resolve_status(state, true), state);
        }
        assert_eq!(resolve_status("tool_running", true), "subagent_running");
        assert_eq!(resolve_status("requesting", false), "requesting");
    }

    #[test]
    fn live_footer_normalizes_prefixed_queue_and_subagent_states() {
        let queued = with_status(Vec::new(), "模型请求正在排队 · 前方 2 个任务");
        assert_eq!(queued[0].icon, "status-queued");
        assert_eq!(queued[0].value, "模型请求正在排队 · 前方 2 个任务");

        let subagent = with_status(Vec::new(), "subagent_running");
        assert_eq!(subagent[0].icon, "status-subagent");
        assert_eq!(subagent[0].value, "子智能体执行中");
    }
}
