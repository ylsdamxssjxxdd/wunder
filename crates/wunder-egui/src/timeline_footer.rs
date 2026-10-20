//! Turn footer projection: status, round, elapsed, speed, context, quota.
//!
//! Formatting only; the timeline paints it.
//!
//! The footer is derived from the presentation model instead of the widget
//! tree so the labels can be unit tested without an egui frame and reused by
//! the web shell. Nothing here imports egui geometry, which keeps the module
//! independent from any layout decision the timeline makes later.

use crate::model::{SessionView, TurnView, WorkflowKind};

/// Raw facts a turn footer can show, before any formatting.
///
/// The painter may override individual fields (for example a duration measured
/// by its own stopwatch) without rebuilding the projection. Every optional
/// field is optional because the underlying model genuinely does not know it;
/// formatting then drops the whole segment instead of printing a placeholder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FooterFacts {
    /// Raw adapter state (`completed`, `running`, `streaming`, ...).
    pub state: String,
    /// 1-based turn ordinal in the visible thread.
    pub round: usize,
    /// Wall-clock duration of the turn, when the caller measured one.
    pub elapsed_ms: Option<u64>,
    /// Output tokens attributed to the assistant message.
    pub output_tokens: Option<u32>,
    /// Context occupancy reported by the runtime.
    pub context_tokens: Option<u32>,
    /// Context window capacity reported by the runtime or the model catalogue.
    pub context_capacity_tokens: Option<u32>,
    /// Used and total quota units, when the account exposes them.
    pub quota: Option<(u32, u32)>,
    /// Number of tool invocations in the turn's process page.
    pub tool_calls: usize,
    /// How many of those tool invocations ended in `failed`.
    pub tool_failures: usize,
    /// `output_tokens` came from [`estimated_tokens`] rather than a real
    /// counter, so the painter can mark the speed figure as approximate.
    pub tokens_estimated: bool,
}

/// One footer chip: the visible label plus its hover explanation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FooterSegment {
    pub text: String,
    pub tooltip: String,
}

/// Derive footer facts for one turn.
///
/// `index` is the zero-based position in `turn`'s list; the public fact is
/// 1-based because "第 1 轮" is what the user sees. `session` supplies the only
/// measured context numbers available today.
///
/// `TurnView` carries no timestamps, and the model has no quota field, so
/// `elapsed_ms` and `quota` stay `None` here; the painter fills them once a
/// duration/account signal exists. Token output has no real counter either, so
/// it falls back to [`estimated_tokens`] and sets `tokens_estimated`.
pub fn facts_from_turn(
    turn: &TurnView,
    index: usize,
    session: Option<&SessionView>,
) -> FooterFacts {
    // The assistant carries the authoritative turn state; a turn without one
    // still reports the user message state instead of an empty chip.
    let state = if turn.assistant.state.is_empty() {
        turn.user.state.as_str()
    } else {
        turn.assistant.state.as_str()
    };

    let mut tool_calls = 0usize;
    let mut tool_failures = 0usize;
    for item in &turn.workflow {
        if item.kind != WorkflowKind::Tool {
            // Reasoning/text rows can also be marked failed; only tool rows
            // belong to the footer's tool summary.
            continue;
        }
        tool_calls += 1;
        if item.state == "failed" {
            tool_failures += 1;
        }
    }

    let output_tokens = if turn.assistant.text.trim().is_empty() {
        None
    } else {
        Some(estimated_tokens(&turn.assistant.text))
    };

    FooterFacts {
        state: state.to_owned(),
        round: index.saturating_add(1),
        elapsed_ms: None,
        output_tokens,
        context_tokens: session.and_then(|session| session.context_tokens),
        context_capacity_tokens: session.and_then(|session| session.context_capacity_tokens),
        quota: None,
        tool_calls,
        tool_failures,
        tokens_estimated: output_tokens.is_some(),
    }
}

/// Build the visible footer chips in painting order.
///
/// Segments whose facts are missing are omitted entirely: a footer that says
/// "—" for every unmeasured field is noisier than a shorter footer.
pub fn segments(facts: &FooterFacts) -> Vec<FooterSegment> {
    let mut out = Vec::with_capacity(7);

    let status = status_label(&facts.state);
    if !status.is_empty() {
        push_unique(
            &mut out,
            FooterSegment {
                text: status.clone(),
                tooltip: format!("执行状态：{status}"),
            },
        );
    }

    if facts.round > 0 {
        let round = format!("第 {} 轮", facts.round);
        push_unique(
            &mut out,
            FooterSegment {
                tooltip: format!("对话轮次：{round}"),
                text: round,
            },
        );
    }

    if let Some(elapsed_ms) = facts.elapsed_ms {
        let elapsed = format_elapsed(elapsed_ms);
        push_unique(
            &mut out,
            FooterSegment {
                tooltip: format!("本轮耗时 {elapsed}"),
                text: elapsed,
            },
        );
    }

    // A rate needs both ends of the division; the elapsed segment above stays
    // visible on its own when tokens are unknown.
    if let (Some(tokens), Some(elapsed_ms)) = (facts.output_tokens, facts.elapsed_ms) {
        if let Some(speed) = format_speed(tokens, elapsed_ms) {
            let mut tooltip = format!("输出速度：{speed}");
            if facts.tokens_estimated {
                tooltip.push_str("（按估算 token 计）");
            }
            push_unique(
                &mut out,
                FooterSegment {
                    text: speed,
                    tooltip,
                },
            );
        }
    }

    if let Some(used) = facts.context_tokens {
        let capacity = facts.context_capacity_tokens.filter(|value| *value > 0);
        let text = match capacity {
            Some(capacity) => format!(
                "{} / {}",
                format_token_count(used),
                format_token_count(capacity)
            ),
            None => format_token_count(used),
        };
        // Percentage and headroom are what the tooltip adds; the chip itself
        // stays short enough to sit beside the other footer facts.
        let tooltip = match capacity {
            Some(capacity) => {
                let percent = context_percent(used, capacity);
                let remaining = capacity.saturating_sub(used);
                format!(
                    "上下文已用 {percent}%，剩余 {}",
                    format_token_count(remaining)
                )
            }
            None => format!("上下文已用 {}", format_token_count(used)),
        };
        push_unique(&mut out, FooterSegment { text, tooltip });
    }

    if let Some((used, total)) = facts.quota {
        let tooltip = if total > used {
            format!("剩余额度 {} 次", total - used)
        } else {
            "额度已用尽".to_owned()
        };
        push_unique(
            &mut out,
            FooterSegment {
                text: format!("额度 {used}/{total}"),
                tooltip,
            },
        );
    }

    // Zero tool calls is the common case for a plain answer; showing
    // "0 个工具" on every such turn only adds noise.
    if facts.tool_calls > 0 {
        let text = if facts.tool_failures > 0 {
            format!("{} 个工具 · {} 失败", facts.tool_calls, facts.tool_failures)
        } else {
            format!("{} 个工具", facts.tool_calls)
        };
        let tooltip = if facts.tool_failures > 0 {
            format!(
                "工具调用 {} 次，失败 {} 次",
                facts.tool_calls, facts.tool_failures
            )
        } else {
            format!("工具调用 {} 次", facts.tool_calls)
        };
        push_unique(&mut out, FooterSegment { text, tooltip });
    }

    out
}

/// Rough token count used when no real counter exists.
///
/// Latin text averages about four characters per token, while CJK is close to
/// one token per character. The number is deliberately labelled "estimated" in
/// [`FooterFacts::tokens_estimated`]; it exists so speed/context chips can
/// degrade gracefully, never to be billed or stored.
pub fn estimated_tokens(text: &str) -> u32 {
    let mut ascii = 0u32;
    let mut non_ascii = 0u32;
    for ch in text.chars() {
        if ch.is_ascii() {
            ascii = ascii.saturating_add(1);
        } else {
            non_ascii = non_ascii.saturating_add(1);
        }
    }
    // Ceil division: a trailing ASCII fragment still costs one token.
    let ascii_tokens = ascii.div_ceil(4);
    ascii_tokens.saturating_add(non_ascii)
}

fn status_label(state: &str) -> String {
    match state {
        "completed" => "已完成".to_owned(),
        // Streaming is the live tail of a running turn; one label is enough.
        "running" | "streaming" => "运行中".to_owned(),
        "queued" => "排队中".to_owned(),
        "failed" => "失败".to_owned(),
        "stopped" | "cancelled" | "canceled" => "已停止".to_owned(),
        // Unknown adapter states stay visible rather than collapsing into an
        // empty chip; stripping underscores makes snake_case readable.
        other => other.replace('_', ""),
    }
}

/// `1.2s` / `1m 05s` / `1h 02m`.
///
/// Sub-minute values round to one decimal because that is the range where
/// users compare turns; longer values use floor-to-unit so the label can only
/// get more precise over time, never jump backwards.
fn format_elapsed(ms: u64) -> String {
    let total_secs = ms / 1000;
    if ms < 60_000 {
        let tenths = ms.saturating_add(50) / 100;
        if tenths >= 600 {
            // 59.95s+ would otherwise render as "60.0s".
            return "1m 00s".to_owned();
        }
        return format!("{}.{}s", tenths / 10, tenths % 10);
    }
    if ms < 3_600_000 {
        return format!("{}m {:02}s", total_secs / 60, total_secs % 60);
    }
    format!("{}h {:02}m", total_secs / 3_600, (total_secs % 3_600) / 60)
}

fn format_speed(tokens: u32, elapsed_ms: u64) -> Option<String> {
    if tokens == 0 || elapsed_ms == 0 {
        // A zero rate is not a measurement; showing "0 tok/s" would claim one.
        return None;
    }
    // Integer half-up rounding; float formatting rounds half to even, which is
    // not 四舍五入. The sub-1 branch is chosen before rounding so 0.5 tok/s
    // keeps its decimal instead of being promoted to "1 tok/s".
    if u64::from(tokens) * 1_000 < elapsed_ms {
        let tenths = (u64::from(tokens) * 10_000 + elapsed_ms / 2) / elapsed_ms;
        // Slow local models would otherwise round down to a useless zero.
        Some(format!("{}.{} tok/s", tenths / 10, tenths % 10))
    } else {
        let whole = (u64::from(tokens) * 1_000 + elapsed_ms / 2) / elapsed_ms;
        Some(format!("{whole} tok/s"))
    }
}

/// Decimal scaling keeps the footer consistent with the composer picker, which
/// labels 128 000 tokens as `128K` rather than `125K` from binary units.
fn format_token_count(value: u32) -> String {
    if value >= 1_000_000 {
        scaled(value, 1_000_000.0, "M")
    } else if value >= 1_000 {
        scaled(value, 1_000.0, "K")
    } else {
        value.to_string()
    }
}

fn scaled(value: u32, divisor: f64, suffix: &str) -> String {
    let rendered = format!("{:.1}", f64::from(value) / divisor);
    // "128.0K" reads worse than "128K"; whole values drop the decimal.
    let trimmed = rendered.strip_suffix(".0").unwrap_or(rendered.as_str());
    format!("{trimmed}{suffix}")
}

fn context_percent(used: u32, capacity: u32) -> u32 {
    let scaled = u64::from(used) * 100 + u64::from(capacity) / 2;
    (scaled / u64::from(capacity)) as u32
}

/// Adjacent chips with the same label would look like a rendering bug, so the
/// second one is dropped. Non-adjacent repeats are kept because they can sit
/// between other facts in a different context.
fn push_unique(out: &mut Vec<FooterSegment>, segment: FooterSegment) {
    if out.last().is_some_and(|last| last.text == segment.text) {
        return;
    }
    out.push(segment);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WorkflowView;

    fn texts(facts: &FooterFacts) -> Vec<String> {
        segments(facts)
            .into_iter()
            .map(|segment| segment.text)
            .collect()
    }

    fn segment(facts: &FooterFacts, needle: &str) -> Option<FooterSegment> {
        segments(facts)
            .into_iter()
            .find(|segment| segment.text.contains(needle))
    }

    fn tool_workflow(kind: WorkflowKind, state: &str) -> WorkflowView {
        WorkflowView {
            kind,
            state: state.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn elapsed_formats_seconds_minutes_and_hours() {
        assert_eq!(format_elapsed(0), "0.0s");
        assert_eq!(format_elapsed(1_200), "1.2s");
        assert_eq!(format_elapsed(65_000), "1m 05s");
        assert_eq!(format_elapsed(3_720_000), "1h 02m");
        // 59.95s+ must promote instead of printing "60.0s".
        assert_eq!(format_elapsed(59_960), "1m 00s");
    }

    #[test]
    fn speed_requires_both_elapsed_and_tokens() {
        let base = FooterFacts {
            elapsed_ms: Some(10_000),
            output_tokens: Some(420),
            ..Default::default()
        };
        assert_eq!(
            segment(&base, "tok/s").map(|s| s.text),
            Some("42 tok/s".to_owned())
        );

        let slow = FooterFacts {
            output_tokens: Some(5),
            ..base.clone()
        };
        assert_eq!(
            segment(&slow, "tok/s").map(|s| s.text),
            Some("0.5 tok/s".to_owned())
        );

        let no_elapsed = FooterFacts {
            elapsed_ms: None,
            ..base.clone()
        };
        assert!(segment(&no_elapsed, "tok/s").is_none());

        let no_tokens = FooterFacts {
            output_tokens: None,
            ..base
        };
        assert!(segment(&no_tokens, "tok/s").is_none());

        // 42.5 tok/s must round up, not to even.
        let half = FooterFacts {
            elapsed_ms: Some(10_000),
            output_tokens: Some(425),
            ..Default::default()
        };
        assert_eq!(
            segment(&half, "tok/s").map(|s| s.text),
            Some("43 tok/s".to_owned())
        );
    }

    #[test]
    fn zero_tokens_or_zero_duration_suppresses_speed() {
        let zero_tokens = FooterFacts {
            elapsed_ms: Some(1_000),
            output_tokens: Some(0),
            ..Default::default()
        };
        assert!(segment(&zero_tokens, "tok/s").is_none());

        let zero_time = FooterFacts {
            elapsed_ms: Some(0),
            output_tokens: Some(10),
            ..Default::default()
        };
        assert!(segment(&zero_time, "tok/s").is_none());
    }

    #[test]
    fn token_scale_boundaries() {
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(1_000), "1K");
        assert_eq!(format_token_count(1_024), "1K");
        assert_eq!(format_token_count(12_800), "12.8K");
        assert_eq!(format_token_count(15_360), "15.4K");
        assert_eq!(format_token_count(128_000), "128K");
        assert_eq!(format_token_count(1_500_000), "1.5M");
    }

    #[test]
    fn context_chip_shows_capacity_and_tooltip_percentage() {
        let facts = FooterFacts {
            context_tokens: Some(12_800),
            context_capacity_tokens: Some(128_000),
            ..Default::default()
        };
        let context = segment(&facts, " / ");
        assert_eq!(
            context.as_ref().map(|s| s.text.as_str()),
            Some("12.8K / 128K")
        );
        let tooltip = context.map(|s| s.tooltip).unwrap_or_default();
        assert!(tooltip.contains("10%"), "tooltip was {tooltip}");
        assert!(tooltip.contains("115.2K"), "tooltip was {tooltip}");
    }

    #[test]
    fn context_without_capacity_shows_used_only() {
        let facts = FooterFacts {
            context_tokens: Some(12_800),
            ..Default::default()
        };
        assert_eq!(
            segment(&facts, "12.8K").map(|s| s.text),
            Some("12.8K".to_owned())
        );
        assert!(segment(&facts, " / ").is_none());
        let tooltip = segment(&facts, "12.8K")
            .map(|s| s.tooltip)
            .unwrap_or_default();
        assert!(tooltip.contains("上下文已用"), "tooltip was {tooltip}");
    }

    #[test]
    fn context_without_used_emits_no_segment() {
        let facts = FooterFacts {
            context_capacity_tokens: Some(128_000),
            ..Default::default()
        };
        assert!(segments(&facts).is_empty());
    }

    #[test]
    fn tool_counts_and_failures_come_from_tool_rows_only() {
        let turn = TurnView {
            assistant: crate::model::MessageView {
                state: "completed".to_owned(),
                text: "完成".to_owned(),
                ..Default::default()
            },
            workflow: vec![
                tool_workflow(WorkflowKind::Tool, "completed"),
                tool_workflow(WorkflowKind::Tool, "failed"),
                tool_workflow(WorkflowKind::Tool, "running"),
                // A failed reasoning row is not a failed tool call.
                tool_workflow(WorkflowKind::Reasoning, "failed"),
                tool_workflow(WorkflowKind::Unknown, "failed"),
            ],
            ..Default::default()
        };
        let facts = facts_from_turn(&turn, 2, None);
        assert_eq!(facts.tool_calls, 3);
        assert_eq!(facts.tool_failures, 1);
        assert_eq!(facts.round, 3);
        assert_eq!(facts.state, "completed");
        assert_eq!(
            segment(&facts, "个工具").map(|s| s.text),
            Some("3 个工具 · 1 失败".to_owned())
        );
    }

    #[test]
    fn tool_segment_omitted_without_calls() {
        let facts = FooterFacts {
            tool_failures: 0,
            ..Default::default()
        };
        assert!(segment(&facts, "个工具").is_none());
    }

    #[test]
    fn facts_from_turn_prefers_session_context_and_estimates_tokens() {
        let turn = TurnView {
            assistant: crate::model::MessageView {
                state: "running".to_owned(),
                text: "你好世界".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        };
        let session = SessionView {
            context_tokens: Some(512),
            context_capacity_tokens: Some(8_192),
            ..Default::default()
        };
        let facts = facts_from_turn(&turn, 0, Some(&session));
        assert_eq!(facts.round, 1);
        assert_eq!(facts.context_tokens, Some(512));
        assert_eq!(facts.context_capacity_tokens, Some(8_192));
        assert_eq!(facts.output_tokens, Some(4));
        assert!(facts.tokens_estimated);
        assert_eq!(facts.elapsed_ms, None);
        assert_eq!(facts.quota, None);
        assert_eq!(
            segment(&facts, "512").map(|s| s.text),
            Some("512 / 8.2K".to_owned())
        );
    }

    #[test]
    fn facts_from_turn_without_session_or_assistant_text_stays_empty() {
        let turn = TurnView::default();
        let facts = facts_from_turn(&turn, 0, None);
        assert_eq!(facts.context_tokens, None);
        assert_eq!(facts.context_capacity_tokens, None);
        assert_eq!(facts.output_tokens, None);
        assert!(!facts.tokens_estimated);
        // Only the round survives; empty state and absent facts paint nothing.
        assert_eq!(texts(&facts), vec!["第 1 轮".to_owned()]);
    }

    #[test]
    fn missing_facts_emit_no_segment() {
        let facts = FooterFacts {
            state: "completed".to_owned(),
            round: 2,
            ..Default::default()
        };
        assert_eq!(
            texts(&facts),
            vec!["已完成".to_owned(), "第 2 轮".to_owned()]
        );
    }

    #[test]
    fn status_labels_cover_known_states_and_fall_back_readably() {
        assert_eq!(status_label("completed"), "已完成");
        assert_eq!(status_label("running"), "运行中");
        assert_eq!(status_label("streaming"), "运行中");
        assert_eq!(status_label("queued"), "排队中");
        assert_eq!(status_label("failed"), "失败");
        assert_eq!(status_label("stopped"), "已停止");
        assert_eq!(status_label("cancelled"), "已停止");
        assert_eq!(status_label("awaiting_approval"), "awaitingapproval");
        assert_eq!(status_label(""), "");
    }

    #[test]
    fn adjacent_duplicate_text_is_dropped() {
        let facts = FooterFacts {
            // An unknown state that happens to render exactly like the round
            // label: the status chip must not repeat it.
            state: "第 1 轮".to_owned(),
            round: 1,
            ..Default::default()
        };
        assert_eq!(texts(&facts), vec!["第 1 轮".to_owned()]);
    }

    #[test]
    fn quota_chip_reports_remaining_or_exhaustion() {
        let remaining = FooterFacts {
            quota: Some((8, 10)),
            ..Default::default()
        };
        let chip = segment(&remaining, "额度");
        assert_eq!(chip.as_ref().map(|s| s.text.as_str()), Some("额度 8/10"));
        assert_eq!(
            chip.as_ref().map(|s| s.tooltip.as_str()),
            Some("剩余额度 2 次")
        );

        let exhausted = FooterFacts {
            quota: Some((10, 10)),
            ..Default::default()
        };
        assert_eq!(
            segment(&exhausted, "额度").map(|s| s.tooltip),
            Some("额度已用尽".to_owned())
        );
    }

    #[test]
    fn estimated_tokens_separates_ascii_and_cjk() {
        assert_eq!(estimated_tokens(""), 0);
        assert_eq!(estimated_tokens("abcd"), 1);
        assert_eq!(estimated_tokens("hello world"), 3);
        assert_eq!(estimated_tokens("你好世界"), 4);
        assert_eq!(estimated_tokens("abc中文"), 3);
        // Same character count, different script, different estimate.
        assert!(estimated_tokens("四个汉字") > estimated_tokens("four"));
    }

    #[test]
    fn estimated_speed_tooltip_marks_the_estimate() {
        let facts = FooterFacts {
            elapsed_ms: Some(10_000),
            output_tokens: Some(420),
            tokens_estimated: true,
            ..Default::default()
        };
        let tooltip = segment(&facts, "tok/s")
            .map(|s| s.tooltip)
            .unwrap_or_default();
        assert!(tooltip.contains("估算"), "tooltip was {tooltip}");
    }
}
