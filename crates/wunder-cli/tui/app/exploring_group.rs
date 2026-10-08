//! Consecutive workspace reads share one `Exploring` card.
//!
//! A model that reads five files makes five calls; as five standalone cards they push the
//! reader's answer off screen. Codex groups a run of read/list/search calls behind one
//! header, and each child keeps its own `ToolCallKey` so a result updates exactly one row
//! instead of overwriting its siblings.

use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use serde_json::Value;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::tool_presentation::ToolPresentation;
use crate::tui::theme;

use super::patch_log::continuation_prefix;
use super::patch_log::disclosure_line;
use super::patch_log::fold_marker_line;
use super::patch_log::pending_status_icon_span;
use super::patch_log::tail_prefix;
use super::ToolCallKey;

/// Children one card can hold. A longer run of reads starts a new card, which bounds both
/// the render cost and what a long exploration keeps in memory.
const CHILDREN_LIMIT: usize = 24;
/// Rows the compact transcript shows before folding the rest.
const VISIBLE_CHILDREN: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExploringChildStatus {
    Pending,
    Success,
    Failure,
}

#[derive(Debug, Clone)]
pub(super) struct ExploringChild {
    key: Option<ToolCallKey>,
    tool_name: String,
    presentation: ToolPresentation,
    target: String,
    status: ExploringChildStatus,
    note: Option<String>,
}

impl ExploringChild {
    pub(super) fn new(
        tool_name: &str,
        args: &Value,
        key: Option<ToolCallKey>,
        presentation: ToolPresentation,
    ) -> Self {
        Self {
            key,
            tool_name: tool_name.to_string(),
            presentation,
            target: crate::tool_presentation::target_argument(args)
                .map(|(_, value)| value)
                .unwrap_or_default(),
            status: ExploringChildStatus::Pending,
            note: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ExploringGroupEntry {
    children: Vec<ExploringChild>,
    is_zh: bool,
}

impl ExploringGroupEntry {
    pub(super) fn new(child: ExploringChild, is_zh: bool) -> Self {
        Self {
            children: vec![child],
            is_zh,
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.children
            .iter()
            .any(|child| child.status == ExploringChildStatus::Pending)
    }

    /// A group only takes the next read while it has room; beyond that a new card starts.
    pub(super) fn can_accept(&self) -> bool {
        self.children.len() < CHILDREN_LIMIT
    }

    pub(super) fn push_child(&mut self, child: ExploringChild) -> bool {
        if !self.can_accept() {
            return false;
        }
        self.children.push(child);
        true
    }

    pub(super) fn has_pending_child_named(&self, tool_name: &str) -> bool {
        self.children.iter().any(|child| {
            child.status == ExploringChildStatus::Pending
                && child.tool_name.eq_ignore_ascii_case(tool_name)
        })
    }

    /// Attach a result to the child that asked for it. A stable `ToolCallKey` always wins;
    /// an id-less result takes the oldest pending child for the same tool, which is arrival
    /// order. A settled child is never rewritten, and no search crosses into other cards.
    pub(super) fn apply_result(
        &mut self,
        tool_name: &str,
        key: Option<&ToolCallKey>,
        ok: bool,
        note: Option<String>,
    ) -> bool {
        let Some(position) = self.locate_child(tool_name, key) else {
            return false;
        };
        let child = &mut self.children[position];
        child.status = if ok {
            ExploringChildStatus::Success
        } else {
            ExploringChildStatus::Failure
        };
        child.note = note.filter(|value| !value.trim().is_empty());
        true
    }

    fn locate_child(&self, tool_name: &str, key: Option<&ToolCallKey>) -> Option<usize> {
        if let Some(position) = key.and_then(|key| {
            self.children
                .iter()
                .position(|child| child.key.as_ref() == Some(key))
        }) {
            return Some(position);
        }
        self.children.iter().position(|child| {
            child.status == ExploringChildStatus::Pending
                && child.tool_name.eq_ignore_ascii_case(tool_name)
        })
    }

    pub(super) fn summary_text(&self) -> String {
        let mut lines = vec![match self.failure_count_suffix() {
            Some(count) => format!("{}{count}", self.header_text()),
            None => self.header_text().to_string(),
        }];
        lines.extend(self.children.iter().map(|child| {
            let parts = self.child_parts(child);
            let mut text = if parts.target.is_empty() {
                parts.verb.clone()
            } else {
                format!("{} {}", parts.verb, parts.target)
            };
            text.push_str(parts.failure.as_str());
            if let Some(note) = parts.note.as_ref() {
                text.push_str(" · ");
                text.push_str(note);
            }
            text
        }));
        lines.join("\n")
    }

    pub(super) fn is_collapsible(&self) -> bool {
        self.children.len() > VISIBLE_CHILDREN
    }

    pub(super) fn render_lines_for_width(&self, width: u16, expanded: bool) -> Vec<Line<'static>> {
        let visible = if expanded || self.children.len() <= VISIBLE_CHILDREN {
            self.children.len()
        } else {
            VISIBLE_CHILDREN
        };
        let mut lines = vec![self.header_line(width)];
        for (index, child) in self.children.iter().take(visible).enumerate() {
            lines.push(self.child_line(child, index, width));
        }
        let hidden = self.children.len().saturating_sub(visible);
        if hidden > 0 {
            lines.push(fold_marker_line(if self.is_zh {
                format!("+{hidden} 行")
            } else {
                format!("+{hidden} lines")
            }));
        }
        if let Some(line) = disclosure_line(expanded, hidden > 0) {
            lines.push(line);
        }
        lines
    }

    fn header_text(&self) -> &'static str {
        match (self.is_active(), self.is_zh) {
            (true, true) => "探索中",
            (true, false) => "Exploring",
            (false, true) => "已探索",
            (false, false) => "Explored",
        }
    }

    /// How many calls in this group ended in failure; the header names the count so a
    /// reader does not have to scan every child row to find out.
    fn failed_children(&self) -> usize {
        self.children
            .iter()
            .filter(|child| child.status == ExploringChildStatus::Failure)
            .count()
    }

    fn failure_count_suffix(&self) -> Option<String> {
        let failed = self.failed_children();
        if failed == 0 {
            return None;
        }
        Some(if self.is_zh {
            format!(" · 失败 {failed}")
        } else {
            format!(" · {failed} failed")
        })
    }

    fn header_line(&self, width: u16) -> Line<'static> {
        let title = Span::styled(
            self.header_text(),
            Style::default().add_modifier(Modifier::BOLD),
        );
        let mut spans = if self.is_active() {
            vec![pending_status_icon_span(), Span::raw(" "), title]
        } else {
            vec![Span::styled("• ", theme::secondary_text()), title]
        };
        let header_width = spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum::<usize>();
        // The count is the first thing to go on a narrow frame: the title states what
        // the card is, and no row may exceed the columns it was given.
        if let Some(count) = self.failure_count_suffix() {
            if header_width + UnicodeWidthStr::width(count.as_str()) <= usize::from(width.max(1)) {
                spans.push(Span::styled(count, theme::danger_text()));
            }
        }
        Line::from(spans)
    }

    /// The four pieces every child row is made of, phrased for the session language.
    fn child_parts(&self, child: &ExploringChild) -> ChildParts {
        ChildParts {
            verb: child.presentation.verb(self.is_zh).to_string(),
            target: child.target.clone(),
            failure: if child.status == ExploringChildStatus::Failure {
                if self.is_zh {
                    " 失败"
                } else {
                    " failed"
                }
            } else {
                ""
            }
            .to_string(),
            note: child
                .note
                .as_deref()
                .and_then(|note| note.lines().next())
                .map(str::trim)
                .filter(|note| !note.is_empty())
                .map(|note| note.to_string()),
        }
    }

    /// One row per child: tree prefix, action in the accent colour, and as much of the
    /// result note as still fits. Children never wrap, so a card cannot grow sideways.
    fn child_line(&self, child: &ExploringChild, index: usize, width: u16) -> Line<'static> {
        let prefix = if index == 0 {
            tail_prefix(true)
        } else {
            continuation_prefix()
        };
        let parts = self.child_parts(child);
        let budget = usize::from(width.max(1)).saturating_sub(UnicodeWidthStr::width(prefix));
        let note = parts
            .note
            .as_ref()
            .map(|note| format!(" · {note}"))
            .filter(|note| {
                parts.full_row_width() + UnicodeWidthStr::width(note.as_str()) <= budget
            });
        // A note that survives is shown whole or not at all; only the target gives way.
        let target_budget = budget
            .saturating_sub(UnicodeWidthStr::width(parts.verb.as_str()))
            .saturating_sub(UnicodeWidthStr::width(parts.failure.as_str()))
            .saturating_sub(
                note.as_deref()
                    .map(UnicodeWidthStr::width)
                    .unwrap_or_default(),
            )
            .saturating_sub(usize::from(!parts.target.is_empty()));
        let target = fit_to_width(parts.target.as_str(), target_budget);

        let mut spans = vec![
            Span::styled(prefix, theme::secondary_text()),
            Span::styled(parts.verb, theme::accent_text()),
        ];
        if !parts.target.is_empty() && !target.is_empty() {
            spans.push(Span::raw(" "));
            spans.push(Span::raw(target));
        }
        if !parts.failure.is_empty() {
            spans.push(Span::styled(parts.failure, theme::danger_text()));
        }
        if let Some(note) = note {
            spans.push(Span::styled(note, theme::secondary_text()));
        }
        Line::from(spans)
    }
}

struct ChildParts {
    verb: String,
    target: String,
    failure: String,
    note: Option<String>,
}

impl ChildParts {
    fn full_row_width(&self) -> usize {
        UnicodeWidthStr::width(self.verb.as_str())
            + usize::from(!self.target.is_empty())
            + UnicodeWidthStr::width(self.target.as_str())
            + UnicodeWidthStr::width(self.failure.as_str())
    }
}

fn fit_to_width(text: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= budget {
        return text.to_string();
    }
    if budget < 2 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + width + 1 > budget {
            break;
        }
        out.push(ch);
        used += width;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_presentation::present;
    use serde_json::json;

    fn key(id: &str) -> ToolCallKey {
        ToolCallKey {
            turn_id: Some("turn-1".to_string()),
            tool_call_id: id.to_string(),
        }
    }

    fn child(tool: &str, target: &str, id: &str) -> ExploringChild {
        ExploringChild::new(
            tool,
            &json!({ "path": target }),
            Some(key(id)),
            present(tool),
        )
    }

    fn group(children: Vec<ExploringChild>) -> ExploringGroupEntry {
        let mut group = ExploringGroupEntry::new(children[0].clone(), false);
        for child in children.into_iter().skip(1) {
            assert!(group.push_child(child));
        }
        group
    }

    fn row_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn a_result_only_touches_the_child_that_asked_for_it() {
        let mut group = group(vec![
            child("read_file", "src/alpha.rs", "call-1"),
            child("list_files", "src", "call-2"),
            child("search_content", "flaky", "call-3"),
        ]);

        assert!(group.apply_result(
            "search_content",
            Some(&key("call-3")),
            false,
            Some("no matches".to_string())
        ));
        assert!(group.apply_result(
            "list_files",
            Some(&key("call-2")),
            true,
            Some("9 entries".to_string())
        ));

        let rows = group.render_lines_for_width(80, false);
        let text = rows.iter().map(row_text).collect::<Vec<_>>().join("\n");
        // Children keep their own order and their own outcomes; the asked-for row is the
        // only one that changed.
        assert!(text.contains("Search flaky failed · no matches"), "{text}");
        assert!(text.contains("List src · 9 entries"), "{text}");
        assert!(
            text.contains("Read src/alpha.rs\n") || text.ends_with("Read src/alpha.rs"),
            "{text}"
        );
        assert!(!text.contains("alpha.rs ·"), "{text}");
        assert!(group.is_active(), "the first read is still open");
        // The header carries the failure count the child rows add up to, in the
        // danger colour, so a reader does not scan every row to find the breakage.
        let header = row_text(&rows[0]);
        assert!(
            header.ends_with("Exploring · 1 failed"),
            "running icon is animated, only the tail is stable: {header}"
        );
        assert_eq!(
            rows[0].spans.last().map(|span| span.style),
            Some(theme::danger_text()),
            "the count is the danger colour, not plain text"
        );
    }

    #[test]
    fn a_clean_group_header_names_no_failures() {
        let mut group = group(vec![
            child("read_file", "src/alpha.rs", "call-1"),
            child("list_files", "src", "call-2"),
        ]);
        assert!(group.apply_result("read_file", Some(&key("call-1")), true, None));
        assert!(group.apply_result("list_files", Some(&key("call-2")), true, None));
        let rows = group.render_lines_for_width(80, false);
        assert_eq!(row_text(&rows[0]), "• Explored");
        assert!(!group.summary_text().contains("failed"), "{rows:?}");
    }

    #[test]
    fn a_settled_group_reads_explored_and_refuses_stragglers() {
        let mut group = group(vec![
            child("read_file", "src/alpha.rs", "call-1"),
            child("read_file", "src/beta.rs", "call-2"),
        ]);
        assert!(group.apply_result("read_file", None, true, None));
        assert!(group.apply_result("read_file", None, true, None));
        assert!(!group.is_active());
        assert_eq!(group.header_text(), "Explored");
        // An orphan end after both children settled matches nothing.
        assert!(!group.apply_result("read_file", Some(&key("call-9")), true, None));
    }

    #[test]
    fn a_full_group_stops_growing() {
        let mut group =
            ExploringGroupEntry::new(child("read_file", "src/alpha.rs", "call-0"), true);
        for index in 1..CHILDREN_LIMIT {
            assert!(group.push_child(child("read_file", "src/alpha.rs", "call-x")));
            assert_eq!(group.children.len(), index + 1);
        }
        assert!(!group.can_accept());
        assert!(!group.push_child(child("read_file", "src/alpha.rs", "call-y")));
    }

    #[test]
    fn a_long_group_folds_and_reopens_for_the_transcript_view() {
        let children = (0..8)
            .map(|index| {
                child(
                    "read_file",
                    &format!("src/module_{index}.rs"),
                    &format!("call-{index}"),
                )
            })
            .collect();
        let group = group(children);
        assert!(group.is_collapsible());

        let folded = group.render_lines_for_width(80, false);
        let folded_text = folded.iter().map(row_text).collect::<Vec<_>>().join("\n");
        assert_eq!(folded_text.lines().count(), 1 + VISIBLE_CHILDREN + 2);
        assert!(folded_text.contains("+2 lines"), "{folded_text}");
        assert!(!folded_text.contains("module_7"), "{folded_text}");

        let open = group.render_lines_for_width(80, true);
        let open_text = open.iter().map(row_text).collect::<Vec<_>>().join("\n");
        assert!(open_text.contains("module_7"), "{open_text}");
        assert!(!open_text.contains("Show details"), "{open_text}");
    }

    #[test]
    fn every_child_row_fits_the_frame_it_is_given() {
        let children = vec![
            child(
                "read_file",
                "src/a_fairly_long_module_name_for_the_flaky_gate.rs",
                "call-1",
            ),
            child(
                "search_content",
                "a query longer than the terminal wants to show",
                "call-2",
            ),
        ];
        let mut group = group(children);
        assert!(group.apply_result(
            "search_content",
            Some(&key("call-2")),
            false,
            Some("permission denied while reading the index".to_string())
        ));
        for width in [20u16, 24, 40, 80, 120] {
            for expanded in [false, true] {
                for line in group.render_lines_for_width(width, expanded) {
                    let text = row_text(&line);
                    assert!(
                        UnicodeWidthStr::width(text.as_str()) <= usize::from(width),
                        "row {text:?} exceeds {width} columns"
                    );
                }
            }
        }

        // The counted header is the first thing a narrow frame gives up; the title stays.
        let narrow = row_text(&group.render_lines_for_width(20, false)[0]);
        assert!(
            !narrow.contains("failed"),
            "narrow drops the count: {narrow}"
        );
        let wide = row_text(&group.render_lines_for_width(24, false)[0]);
        assert!(
            wide.ends_with("Exploring · 1 failed"),
            "wide states it: {wide}"
        );
    }

    #[test]
    fn the_plain_form_keeps_every_child_for_copy() {
        let group = group(vec![
            child("read_file", "src/alpha.rs", "call-1"),
            child("list_files", "src", "call-2"),
        ]);
        let text = group.summary_text();
        assert!(text.starts_with("Exploring\n"), "{text}");
        assert!(text.contains("Read src/alpha.rs"), "{text}");
        assert!(text.contains("List src"), "{text}");

        let zh = ExploringGroupEntry::new(child("读取文件", "src/中文文件.rs", "call-3"), true);
        let zh_text = zh.summary_text();
        assert!(zh_text.starts_with("探索中\n"), "{zh_text}");
        assert!(zh_text.contains("读取 src/中文文件.rs"), "{zh_text}");
    }
}
