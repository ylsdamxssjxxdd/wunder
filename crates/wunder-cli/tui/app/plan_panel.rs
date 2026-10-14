//! The `update_plan` card: one header, then the plan the model is working through.
//!
//! The engine emits `plan_update` with the whole plan each time, so a card is
//! replaced rather than appended to: the reader sees the current plan, not the
//! history of every edit that produced it. Steps keep the three states codex
//! renders — done, in progress, not started — and the header carries the
//! `done/total` count so a folded card still says how far the work has got.

use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::tui::theme;

use super::patch_log::continuation_prefix;
use super::patch_log::disclosure_line;
use super::patch_log::fold_marker_line;
use super::patch_log::tail_prefix;

/// Steps shown before the rest fold. Matches the preview depth the other cards
/// use, so one screen holds the plan and the answer under it.
const VISIBLE_STEPS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlanStepStatus {
    Pending,
    InProgress,
    Completed,
}

impl PlanStepStatus {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or_default().trim().to_lowercase().as_str() {
            "completed" | "complete" | "done" => Self::Completed,
            "in_progress" | "inprogress" | "running" => Self::InProgress,
            _ => Self::Pending,
        }
    }

    /// codex marks a finished step with a check and strikes the text, an active
    /// step with the accent box, and the rest stay dim.
    fn marker(self) -> &'static str {
        match self {
            Self::Completed => "✔ ",
            Self::InProgress => "□ ",
            Self::Pending => "□ ",
        }
    }

    fn style(self) -> Style {
        match self {
            Self::Completed => Style::default()
                .add_modifier(Modifier::CROSSED_OUT)
                .add_modifier(Modifier::DIM),
            Self::InProgress => theme::accent_text().add_modifier(Modifier::BOLD),
            Self::Pending => Style::default().add_modifier(Modifier::DIM),
        }
    }

    /// In-progress first, then pending, then completed: the folded card shows
    /// what is happening now, which is the only part a reader is waiting on.
    fn sort_rank(self) -> u8 {
        match self {
            Self::InProgress => 0,
            Self::Pending => 1,
            Self::Completed => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PlanStep {
    text: String,
    status: PlanStepStatus,
}

#[derive(Debug, Clone)]
pub(super) struct PlanLogEntry {
    steps: Vec<PlanStep>,
    explanation: Option<String>,
    is_zh: bool,
}

impl PlanLogEntry {
    pub(super) fn parse(payload: &Value, is_zh: bool) -> Option<Self> {
        let steps = payload
            .get("plan")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let text = item
                            .get("step")
                            .or_else(|| item.get("text"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())?;
                        Some(PlanStep {
                            text: text.to_string(),
                            status: PlanStepStatus::parse(
                                item.get("status").and_then(Value::as_str),
                            ),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if steps.is_empty() {
            return None;
        }
        let explanation = payload
            .get("explanation")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        Some(Self {
            steps,
            explanation,
            is_zh,
        })
    }

    pub(super) fn summary_text(&self) -> String {
        let mut lines = vec![self.header_text(true)];
        lines.extend(self.ordered_steps().iter().map(|step| {
            format!(
                "{}{}",
                step.status.marker(),
                step.text.lines().next().unwrap_or_default()
            )
        }));
        lines.join("\n")
    }

    pub(super) fn is_collapsible(&self) -> bool {
        self.steps.len() > VISIBLE_STEPS
    }

    pub(super) fn render_lines_for_width(&self, width: u16, expanded: bool) -> Vec<Line<'static>> {
        let steps = self.ordered_steps();
        let visible = if expanded || steps.len() <= VISIBLE_STEPS {
            steps.len()
        } else {
            VISIBLE_STEPS
        };
        let mut lines = vec![self.header_line()];
        let mut index = 0usize;
        if let Some(explanation) = self.explanation.as_deref() {
            lines.push(self.explanation_line(explanation, width));
        }
        for step in steps.iter().take(visible) {
            lines.push(self.step_line(step, index, width));
            index += 1;
        }
        let hidden = steps.len().saturating_sub(visible);
        if hidden > 0 {
            lines.push(fold_marker_line(if self.is_zh {
                format!("+{hidden} 步")
            } else {
                format!("+{hidden} steps")
            }));
        }
        if let Some(line) = disclosure_line(expanded, hidden > 0 || self.explanation.is_some()) {
            lines.push(line);
        }
        lines
    }

    fn ordered_steps(&self) -> Vec<&PlanStep> {
        let mut ordered = self.steps.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|step| step.status.sort_rank());
        ordered
    }

    fn completed_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status == PlanStepStatus::Completed)
            .count()
    }

    fn header_text(&self, with_progress: bool) -> String {
        let title = if self.is_zh {
            "已更新计划"
        } else {
            "Updated Plan"
        };
        if with_progress {
            return if self.is_zh {
                format!(
                    "{title} · 完成 {}/{}",
                    self.completed_count(),
                    self.steps.len()
                )
            } else {
                format!(
                    "{title} · {}/{} complete",
                    self.completed_count(),
                    self.steps.len()
                )
            };
        }
        title.to_string()
    }

    fn header_line(&self) -> Line<'static> {
        Line::from(vec![
            Span::styled("• ", theme::secondary_text()),
            Span::styled(
                self.header_text(true),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ])
    }

    fn explanation_line(&self, explanation: &str, width: u16) -> Line<'static> {
        let budget = usize::from(width.max(1)).saturating_sub(4);
        Line::from(vec![
            Span::styled(tail_prefix(true), theme::secondary_text()),
            Span::styled(
                fit_to_width(explanation.lines().next().unwrap_or_default(), budget),
                Style::default()
                    .add_modifier(Modifier::ITALIC)
                    .add_modifier(Modifier::DIM),
            ),
        ])
    }

    fn step_line(&self, step: &PlanStep, index: usize, width: u16) -> Line<'static> {
        let prefix = if index == 0 && self.explanation.is_none() {
            tail_prefix(true)
        } else {
            continuation_prefix()
        };
        let budget = usize::from(width.max(1))
            .saturating_sub(UnicodeWidthStr::width(prefix))
            .saturating_sub(UnicodeWidthStr::width(step.status.marker()));
        Line::from(vec![
            Span::styled(prefix, theme::secondary_text()),
            Span::styled(step.status.marker(), step.status.style()),
            Span::styled(
                fit_to_width(step.text.as_str(), budget),
                step.status.style(),
            ),
        ])
    }
}

fn fit_to_width(text: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= budget {
        return text.lines().next().unwrap_or_default().to_string();
    }
    let mut output = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            break;
        }
        let mut measure = [0u8; 4];
        let width = UnicodeWidthStr::width(ch.encode_utf8(&mut measure) as &str);
        if used + width + 1 > budget {
            output.push('…');
            return output;
        }
        used += width;
        output.push(ch);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_sort_in_progress_first() {
        let entry = PlanLogEntry::parse(
            &serde_json::json!({
                "plan": [
                    {"step": "done one", "status": "completed"},
                    {"step": "next one", "status": "pending"},
                    {"step": "active one", "status": "in_progress"}
                ]
            }),
            false,
        )
        .expect("plan");
        let ordered = entry
            .ordered_steps()
            .iter()
            .map(|step| step.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ordered, vec!["active one", "next one", "done one"]);
    }

    #[test]
    fn completed_steps_count_toward_the_header() {
        let entry = PlanLogEntry::parse(
            &serde_json::json!({
                "plan": [
                    {"step": "a", "status": "completed"},
                    {"step": "b", "status": "pending"},
                    {"step": "c", "status": "pending"},
                    {"step": "d", "status": "pending"}
                ]
            }),
            false,
        )
        .expect("plan");
        assert_eq!(entry.header_text(true), "Updated Plan · 1/4 complete");
    }

    #[test]
    fn an_empty_plan_is_not_a_card() {
        assert!(PlanLogEntry::parse(&serde_json::json!({"plan": []}), false).is_none());
        assert!(PlanLogEntry::parse(&serde_json::json!({}), false).is_none());
    }

    #[test]
    fn a_folded_plan_card_offers_to_expand() {
        let entry = PlanLogEntry::parse(
            &serde_json::json!({
                "plan": [
                    {"step": "a"}, {"step": "b"}, {"step": "c"}, {"step": "d"}
                ]
            }),
            false,
        )
        .expect("plan");
        assert!(entry.is_collapsible());
        let lines = entry.render_lines_for_width(60, false);
        assert!(lines.len() > VISIBLE_STEPS);
    }
}
