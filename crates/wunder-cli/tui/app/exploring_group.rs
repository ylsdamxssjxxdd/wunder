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

use crate::tool_presentation::canonical_tool_name;
use crate::tool_presentation::ToolPresentation;
use crate::tui::theme;

use super::patch_log::continuation_prefix;
use super::patch_log::disclosure_line;
use super::patch_log::pending_status_icon_span;
use super::patch_log::tail_prefix;
use super::patch_log::Disclosure;
use super::ToolCallKey;

/// Children one card can hold. A longer run of reads starts a new card, which bounds both
/// the render cost and what a long exploration keeps in memory.
const CHILDREN_LIMIT: usize = 24;
/// Rows the folded card keeps under its header, the same depth the reference previews an
/// activity before it folds the rest.
const PREVIEW_ROWS: usize = 3;
/// Joins the names inside one coalesced read row.
const NAME_SEPARATOR: &str = ", ";

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

    pub(super) fn interrupt_pending(&mut self) -> bool {
        let mut changed = false;
        for child in &mut self.children {
            if child.status == ExploringChildStatus::Pending {
                child.status = ExploringChildStatus::Failure;
                child.note = Some(
                    if self.is_zh {
                        "未收到工具结果"
                    } else {
                        "tool result unavailable"
                    }
                    .into(),
                );
                changed = true;
            }
        }
        changed
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
                && canonical_tool_name(child.tool_name.as_str()) == canonical_tool_name(tool_name)
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
                && canonical_tool_name(child.tool_name.as_str()) == canonical_tool_name(tool_name)
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
                parts.verb
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

    /// What the card shows: one row per call, except that a run of plain file reads becomes
    /// a single `Read a, b` row naming each file once. Copy keeps every child line, because
    /// a reader who pastes the transcript wants the calls, not the compression.
    fn display_rows(&self) -> Vec<ExploringRow> {
        let mut rows: Vec<ExploringRow> = Vec::with_capacity(self.children.len());
        for child in &self.children {
            let parts = self.child_parts(child);
            // A failed call breaks the run and keeps its own row, exactly as the reference
            // stops grouping at a nonzero exit.
            let read_run = is_file_read_name(&child.tool_name) && parts.failure.is_empty();
            if read_run {
                if let Some(last) = rows.last_mut().filter(|row| row.read_run) {
                    last.push_name(parts.target);
                    continue;
                }
            }
            rows.push(ExploringRow {
                read_run,
                verb: parts.verb,
                names: if parts.target.is_empty() {
                    Vec::new()
                } else {
                    vec![parts.target]
                },
                failure: parts.failure,
                // A settled row shows only what was asked for; the reference puts no output
                // text on these rows. A failure keeps its reason — that is the one note a
                // reader needs before opening the card.
                note: if child.status == ExploringChildStatus::Failure {
                    parts.note
                } else {
                    None
                },
            });
        }
        rows
    }

    /// Rows the preview budget leaves out. Both the disclosure row and the Enter gate read
    /// this, so the count can never name rows the card would not reveal.
    fn hidden_rows(rows: &[ExploringRow]) -> usize {
        rows.len().saturating_sub(PREVIEW_ROWS)
    }

    pub(super) fn card_fold(&self) -> Option<Disclosure> {
        Disclosure::new_lines(Self::hidden_rows(&self.display_rows()))
    }

    pub(super) fn render_lines_for_width(&self, width: u16, expanded: bool) -> Vec<Line<'static>> {
        let rows = self.display_rows();
        let fold = Disclosure::new_lines(Self::hidden_rows(&rows));
        let visible = if expanded {
            rows.len()
        } else {
            rows.len().min(PREVIEW_ROWS)
        };
        let mut lines = vec![self.header_line(width)];
        for (index, row) in rows.iter().take(visible).enumerate() {
            lines.push(self.row_line(row, index, width));
        }
        // The count lives on the disclosure row, where Enter is named next to it: a second
        // marker row carrying the same number only made the card say it twice.
        if let Some(line) = disclosure_line(expanded, fold, width, self.is_zh) {
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

    /// One row per rendered line: tree prefix, action in the accent colour, and as much of
    /// the result note as still fits. Rows never wrap, so a card cannot grow sideways.
    fn row_line(&self, row: &ExploringRow, index: usize, width: u16) -> Line<'static> {
        let prefix = if index == 0 {
            tail_prefix(true)
        } else {
            continuation_prefix()
        };
        let budget = usize::from(width.max(1)).saturating_sub(UnicodeWidthStr::width(prefix));
        let target = row.target();
        let note = row
            .note
            .as_ref()
            .map(|note| format!(" · {note}"))
            .filter(|note| row.full_row_width() + UnicodeWidthStr::width(note.as_str()) <= budget);
        // A note that survives is shown whole or not at all; only the target gives way.
        let target_budget = budget
            .saturating_sub(UnicodeWidthStr::width(row.verb.as_str()))
            .saturating_sub(UnicodeWidthStr::width(row.failure.as_str()))
            .saturating_sub(
                note.as_deref()
                    .map(UnicodeWidthStr::width)
                    .unwrap_or_default(),
            )
            .saturating_sub(usize::from(!target.is_empty()));
        let target = fit_to_width(target.as_str(), target_budget);

        let mut spans = vec![
            Span::styled(prefix, theme::secondary_text()),
            Span::styled(row.verb.clone(), theme::accent_text()),
        ];
        if !target.is_empty() {
            spans.push(Span::raw(" "));
            spans.push(Span::raw(target));
        }
        if !row.failure.is_empty() {
            spans.push(Span::styled(row.failure.clone(), theme::danger_text()));
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

/// A rendered line of an exploring card. A coalesced read run keeps one name per file,
/// so the width maths has exactly one string to fit.
struct ExploringRow {
    read_run: bool,
    verb: String,
    names: Vec<String>,
    failure: String,
    note: Option<String>,
}

impl ExploringRow {
    fn push_name(&mut self, name: String) {
        if name.is_empty() || self.names.iter().any(|held| held == &name) {
            return;
        }
        self.names.push(name);
    }

    fn target(&self) -> String {
        self.names.join(NAME_SEPARATOR)
    }

    fn full_row_width(&self) -> usize {
        UnicodeWidthStr::width(self.verb.as_str())
            + usize::from(!self.names.is_empty())
            + UnicodeWidthStr::width(self.target().as_str())
            + UnicodeWidthStr::width(self.failure.as_str())
    }
}

/// Reads whose row is only a name list; the reference folds a run of them into one line.
fn is_file_read_name(tool_name: &str) -> bool {
    crate::tool_presentation::is_file_read_tool_name(tool_name)
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
        // only one that changed. A settled row states only what it touched.
        assert!(text.contains("Search flaky failed · no matches"), "{text}");
        assert!(
            text.contains("List src\n") || text.ends_with("List src"),
            "{text}"
        );
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
        // Searches each keep a row, so a long card is what the fold is for.
        let children = (0..5)
            .map(|index| {
                child(
                    "search_content",
                    &format!("symbol_{index}"),
                    &format!("call-{index}"),
                )
            })
            .collect();
        let group = group(children);
        assert!(group.card_fold().is_some());

        let folded = group.render_lines_for_width(80, false);
        let folded_text = folded.iter().map(row_text).collect::<Vec<_>>().join("\n");
        // One header, the preview budget, and the single row that counts the rest: the fold
        // used to spend a second row repeating that number.
        assert_eq!(folded_text.lines().count(), 1 + PREVIEW_ROWS + 1);
        assert!(folded_text.contains("+2 lines"), "{folded_text}");
        assert!(!folded_text.contains("symbol_4"), "{folded_text}");

        let open = group.render_lines_for_width(80, true);
        let open_text = open.iter().map(row_text).collect::<Vec<_>>().join("\n");
        assert!(open_text.contains("symbol_4"), "{open_text}");
        assert!(!open_text.contains("Show details"), "{open_text}");
    }

    #[test]
    fn a_run_of_reads_shares_one_row_that_names_each_file_once() {
        let children = vec![
            child("read_file", "src/auth.rs", "call-1"),
            child("read_file", "src/shimmer.rs", "call-2"),
            child("read_file", "src/auth.rs", "call-3"),
            child("list_files", "src", "call-4"),
        ];
        let mut group = group(children);
        for id in ["call-1", "call-2", "call-3", "call-4"] {
            assert!(group.apply_result("read_file", Some(&key(id)), true, None));
        }
        let rows = group.render_lines_for_width(120, false);
        // Header plus one read row plus the listing: four calls, three rows, no fold.
        assert_eq!(
            rows.len(),
            3,
            "{:?}",
            rows.iter().map(row_text).collect::<Vec<_>>()
        );
        assert_eq!(row_text(&rows[1]), "  └ Read src/auth.rs, src/shimmer.rs");
        assert_eq!(row_text(&rows[2]), "    List src");
        assert!(
            group.card_fold().is_none(),
            "a run of reads cannot re-inflate the card"
        );
        // Copy still carries every call; only the display compresses.
        let plain = group.summary_text();
        assert_eq!(plain.lines().count(), 5, "{plain}");
    }

    #[test]
    fn a_failed_read_keeps_its_own_row_and_breaks_the_run() {
        let children = vec![
            child("read_file", "src/alpha.rs", "call-1"),
            child("read_file", "src/beta.rs", "call-2"),
            child("read_file", "src/gamma.rs", "call-3"),
        ];
        let mut group = group(children);
        assert!(group.apply_result(
            "read_file",
            Some(&key("call-2")),
            false,
            Some("no such file".to_string())
        ));
        assert!(group.apply_result("read_file", Some(&key("call-1")), true, None));
        assert!(group.apply_result("read_file", Some(&key("call-3")), true, None));
        let text = group
            .render_lines_for_width(120, false)
            .iter()
            .map(row_text)
            .collect::<Vec<_>>()
            .join("\n");
        // The failed call is not folded into the name list, and the reads on either side of
        // it are two runs, not one.
        assert!(text.contains("Read src/alpha.rs\n"), "{text}");
        assert!(
            text.contains("Read src/beta.rs failed · no such file"),
            "{text}"
        );
        assert!(text.contains("Read src/gamma.rs"), "{text}");
        assert!(!text.contains("alpha.rs, src/gamma"), "{text}");
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
