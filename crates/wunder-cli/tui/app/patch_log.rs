use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use serde_json::Value;
use textwrap::Options;
use unicode_width::UnicodeWidthStr;

use super::exploring_group::ExploringGroupEntry;
use super::plan_panel::PlanLogEntry;
use crate::command_session_display::{CommandSessionDisplayStatus, CommandSessionView};
use crate::patch_diff::{
    build_patch_diff_preview, patch_diff_preview_from_blocks, PatchDiffBlock, PatchDiffBlockKind,
    PatchDiffLine, PatchDiffLineKind, PatchDiffPreview,
};
use crate::tool_display::summarize_tool_result;
use crate::tool_presentation::{
    is_mcp_tool_name, is_subagent_tool, mcp_invocation_label, present, present_subagent_action,
    target_argument, ToolAccess, ToolPresentation,
};
use crate::tui::activity_indicator;
use crate::tui::theme;

pub(super) fn pending_status_icon_span() -> Span<'static> {
    Span::styled(
        format!("{} ", activity_indicator::RUNNING_INDICATOR),
        activity_indicator::pending_indicator_style(),
    )
}

fn status_icon_span(icon: &str, icon_style: Style) -> Span<'static> {
    Span::styled(format!("{icon} "), icon_style)
}

#[derive(Debug, Clone)]
pub(super) enum SpecialLogEntry {
    Art(ArtLogEntry),
    Patch(PatchLogEntry),
    Command(CommandLogEntry),
    Tool(GenericToolLogEntry),
    Exploring(ExploringGroupEntry),
    Plan(PlanLogEntry),
}

impl SpecialLogEntry {
    pub(super) fn interrupt_pending(&mut self) -> bool {
        match self {
            Self::Tool(entry) if entry.status == GenericToolLogStatus::Pending => {
                entry.status = GenericToolLogStatus::Failure;
                entry.summary = Some(
                    if entry.is_zh {
                        "未收到工具结果"
                    } else {
                        "tool result unavailable"
                    }
                    .into(),
                );
                true
            }
            Self::Command(entry) if entry.status == CommandLogStatus::Pending => {
                entry.status = CommandLogStatus::Completed;
                entry.success = false;
                entry.title = if entry.is_zh {
                    "结果未知"
                } else {
                    "Result unavailable"
                }
                .into();
                entry.metrics = None;
                true
            }
            Self::Patch(entry) if entry.status == PatchLogStatus::Pending => {
                entry.status = PatchLogStatus::Failure;
                entry.summary = Some(
                    if entry.is_zh {
                        "未收到工具结果"
                    } else {
                        "tool result unavailable"
                    }
                    .into(),
                );
                true
            }
            Self::Exploring(group) => group.interrupt_pending(),
            _ => false,
        }
    }

    pub(super) fn summary_text(&self) -> String {
        match self {
            Self::Art(entry) => entry.summary_text(),
            Self::Patch(entry) => entry.summary_text(),
            Self::Command(entry) => entry.summary_text(),
            Self::Tool(entry) => entry.summary_text(),
            Self::Exploring(entry) => entry.summary_text(),
            Self::Plan(entry) => entry.summary_text(),
        }
    }

    pub(super) fn line_count(&self, width: u16, expanded: bool) -> usize {
        let lines = self.render_lines_for_width(width, expanded);
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .line_count(width.max(1))
            .max(1)
    }

    /// `expanded` is the transcript overlay: cards drop their folds and show the window
    /// they already hold, without rebuilding anything.
    pub(super) fn render_lines_for_width(&self, width: u16, expanded: bool) -> Vec<Line<'static>> {
        match self {
            Self::Art(entry) => entry.render_lines_for_width(width),
            Self::Patch(entry) => entry.render_lines_for_width(width, expanded),
            Self::Command(entry) => entry.render_lines_for_width(width, expanded),
            Self::Tool(entry) => entry.render_lines_for_width(width, expanded),
            Self::Exploring(entry) => entry.render_lines_for_width(width, expanded),
            Self::Plan(entry) => entry.render_lines_for_width(width, expanded),
        }
    }

    /// Whether the Enter disclosure has anything to reveal for this card.
    /// What Enter can bring back on this card. This is the single source for both the
    /// disclosure row and the key/mouse gate, so a row can never promise a fold that the
    /// gate refuses to open.
    pub(super) fn card_fold(&self, width: u16) -> Option<Disclosure> {
        match self {
            Self::Patch(entry) => entry.card_fold(width),
            Self::Command(entry) => entry.card_fold(),
            Self::Tool(entry) => entry.card_fold(),
            Self::Exploring(entry) => entry.card_fold(),
            Self::Plan(entry) => entry.card_fold(),
            _ => None,
        }
    }

    pub(super) fn is_collapsible(&self, width: u16) -> bool {
        self.card_fold(width).is_some()
    }

    pub(super) fn is_pending_patch(&self) -> bool {
        matches!(self, Self::Patch(entry) if entry.status == PatchLogStatus::Pending)
    }

    pub(super) fn is_pending_command(&self) -> bool {
        matches!(self, Self::Command(entry) if entry.status == CommandLogStatus::Pending)
    }

    pub(super) fn is_pending_tool_named(&self, tool_name: &str) -> bool {
        match self {
            Self::Tool(entry) => {
                entry.status == GenericToolLogStatus::Pending
                    && entry.tool_name.eq_ignore_ascii_case(tool_name)
            }
            Self::Exploring(group) => group.has_pending_child_named(tool_name),
            _ => false,
        }
    }

    pub(super) fn inherit_patch_preview_from(&mut self, previous: &Self) {
        if let (Self::Patch(current), Self::Patch(previous)) = (self, previous) {
            current.inherit_diff_from(previous);
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ArtLogEntry {
    summary: String,
    lines: Vec<Line<'static>>,
}

impl ArtLogEntry {
    fn summary_text(&self) -> String {
        self.summary.clone()
    }

    fn render_lines_for_width(&self, _width: u16) -> Vec<Line<'static>> {
        self.lines.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GenericToolLogStatus {
    Pending,
    Success,
    Failure,
}

#[derive(Debug, Clone)]
pub(super) struct GenericToolLogEntry {
    tool_name: String,
    status: GenericToolLogStatus,
    summary: Option<String>,
    details: Vec<PatchNote>,
    /// The arguments the call was made with, shown on the open card: a remote result is
    /// only readable next to what was actually asked for.
    arguments: Option<String>,
    is_zh: bool,
    pub(super) presentation: ToolPresentation,
}

/// Rows a folded tool card still shows. The reference keeps a preview instead of hiding a
/// result until it is asked for, which is what makes a tool call readable as it lands.
const TOOL_PREVIEW_ROWS: usize = 3;

impl GenericToolLogEntry {
    fn summary_text(&self) -> String {
        let header = self.header_text();
        if self.promoted_summary().is_some() {
            return header;
        }
        if let Some(summary) = self.summary.as_ref().filter(|value| !value.is_empty()) {
            format!("{header} {summary}")
        } else {
            header
        }
    }

    fn render_lines_for_width(&self, width: u16, expanded: bool) -> Vec<Line<'static>> {
        let promoted_summary = self.promoted_summary();
        let summary = self.summary.as_ref().filter(|value| !value.is_empty());
        let body_summary = if promoted_summary.is_some() {
            None
        } else {
            summary
        };
        let mut used_inline_summary = false;
        let mut lines = vec![if let Some(summary) = body_summary {
            if let Some(line) = self.header_line_with_summary(summary.as_str(), width) {
                used_inline_summary = true;
                line
            } else {
                self.header_line()
            }
        } else {
            self.header_line()
        }];
        let visible_details = if expanded {
            self.details.len()
        } else {
            self.details.len().min(TOOL_PREVIEW_ROWS)
        };
        let show_arguments = expanded && self.arguments.is_some();
        let mut remaining_items = visible_details + usize::from(show_arguments);
        if !used_inline_summary && body_summary.is_some() {
            remaining_items += 1;
        }
        if !used_inline_summary {
            if let Some(summary) = body_summary {
                let is_last = remaining_items == 1;
                lines.extend(render_wrapped_tree_text(
                    summary.as_str(),
                    summary_style(),
                    is_last,
                    width,
                ));
                remaining_items = remaining_items.saturating_sub(1);
            }
        }
        for detail in self.details.iter().take(visible_details) {
            let is_last = remaining_items == 1;
            lines.extend(detail.render_wrapped(width, is_last));
            remaining_items = remaining_items.saturating_sub(1);
        }
        if show_arguments {
            let text = format!(
                "{}{}",
                if self.is_zh {
                    "参数: "
                } else {
                    "Arguments: "
                },
                self.arguments.as_deref().unwrap_or_default()
            );
            lines.extend(render_wrapped_tree_text(
                text.as_str(),
                summary_style(),
                remaining_items == 1,
                width,
            ));
        }
        if let Some(line) = disclosure_line(expanded, self.card_fold(), width, self.is_zh) {
            lines.push(line);
        }
        lines
    }

    /// Rows outside the preview are what Enter brings back. When the whole list already
    /// fits, the fold is only worth a row if the arguments still have to appear.
    fn card_fold(&self) -> Option<Disclosure> {
        let hidden = self.details.len().saturating_sub(TOOL_PREVIEW_ROWS);
        if hidden > 0 {
            return Some(Disclosure::Lines(hidden));
        }
        self.arguments.as_ref().map(|_| Disclosure::Generic)
    }

    /// The verb comes from the tool classifier and the language from the session, never
    /// from the shape of the tool name. A tool we do not know keeps its own name visible.
    fn header_text(&self) -> String {
        if let Some(summary) = self.promoted_summary() {
            return summary.to_string();
        }
        let verb = self.presentation.verb(self.is_zh);
        // A remote call must name the tool it reached: the verb alone would leave the reader
        // with "called an external tool" and no idea which one.
        let names_self =
            self.presentation.access == ToolAccess::Other || is_mcp_tool_name(&self.tool_name);
        let subject = || match mcp_invocation_label(self.tool_name.as_str()) {
            Some(label) => label,
            None => self.tool_name.clone(),
        };
        match self.status {
            GenericToolLogStatus::Failure => {
                if self.is_zh {
                    if names_self {
                        format!("{} 失败", subject())
                    } else {
                        format!("{}失败", verb)
                    }
                } else if names_self {
                    format!("{} failed", subject())
                } else {
                    format!("{verb} failed")
                }
            }
            _ => {
                if names_self {
                    format!("{verb} {}", subject())
                } else if self.status == GenericToolLogStatus::Success {
                    self.presentation.verb_done(self.is_zh).to_string()
                } else {
                    verb.to_string()
                }
            }
        }
    }

    fn header_line(&self) -> Line<'static> {
        let (icon, icon_style, text_style) = self.header_parts();
        let icon_span = if self.status == GenericToolLogStatus::Pending {
            pending_status_icon_span()
        } else {
            status_icon_span(icon, icon_style)
        };
        Line::from(vec![
            icon_span,
            Span::styled(self.header_text(), text_style),
        ])
    }

    fn promoted_summary(&self) -> Option<&str> {
        if self.status != GenericToolLogStatus::Success {
            return None;
        }
        self.summary
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "{}")
    }

    fn header_line_with_summary(&self, summary: &str, width: u16) -> Option<Line<'static>> {
        if summary.contains('\n') {
            return None;
        }
        let (icon, icon_style, text_style) = self.header_parts();
        let header = self.header_text();
        let total_width = UnicodeWidthStr::width(format!("{icon} {header} {summary}").as_str());
        if total_width > usize::from(width.max(1)) {
            return None;
        }
        let icon_span = if self.status == GenericToolLogStatus::Pending {
            pending_status_icon_span()
        } else {
            status_icon_span(icon, icon_style)
        };
        Some(Line::from(vec![
            icon_span,
            Span::styled(header, text_style),
            Span::raw(" "),
            Span::styled(summary.to_string(), summary_style()),
        ]))
    }

    fn header_parts(&self) -> (&'static str, Style, Style) {
        match self.status {
            GenericToolLogStatus::Pending => (
                "•",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                theme::accent_text(),
            ),
            GenericToolLogStatus::Success => (
                "•",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            GenericToolLogStatus::Failure => (
                "✘",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                theme::danger_text(),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandLogStatus {
    Pending,
    Completed,
}

#[derive(Debug, Clone)]
pub(super) struct CommandLogEntry {
    status: CommandLogStatus,
    success: bool,
    title: String,
    command: String,
    metrics: Option<String>,
    /// The exit code the footer states once the card is open. `None` while the process runs.
    exit_code: Option<i64>,
    duration_ms: Option<i64>,
    sections: Vec<CommandSection>,
    is_zh: bool,
}

impl CommandLogEntry {
    fn summary_text(&self) -> String {
        format!("{} {}", self.title, self.command)
    }

    /// Rows held for the whole stream. Counting, not collecting: a collapsed card must not
    /// copy the window it keeps in memory.
    fn merged_output_len(&self) -> usize {
        self.sections
            .iter()
            .map(|section| section.lines.len())
            .sum()
    }

    /// What Enter brings back: the output rows outside the tail budget, the command's own
    /// continuation rows, and the settled footer row that names the exit and the elapsed
    /// time. Rows the stored window already dropped are excluded, because no expansion can
    /// reveal what is gone — the storage marker owns those.
    fn card_fold(&self) -> Option<Disclosure> {
        let hidden = self.hidden_output_lines()
            + self
                .command
                .lines()
                .count()
                .saturating_sub(/*header row*/ 1)
            + usize::from(self.exit_code.is_some());
        Disclosure::new_lines(hidden)
    }

    /// Rows the tail budget leaves out of the collapsed card.
    fn hidden_output_lines(&self) -> usize {
        self.merged_output_len()
            .saturating_sub(COMMAND_TAIL_PREVIEW_LINES)
    }

    fn render_lines_for_width(&self, width: u16, expanded: bool) -> Vec<Line<'static>> {
        let mut lines = vec![self.header_line()];
        let mut tail_first = true;
        if expanded {
            for command_line in self.command.lines().skip(1) {
                lines.extend(render_wrapped_tail_text(
                    command_line,
                    command_style(),
                    tail_first,
                    width,
                ));
                tail_first = false;
            }
        }

        if expanded {
            for section in &self.sections {
                for index in 0..section.lines.len() {
                    let rendered = section.render_row(index, tail_first, width);
                    tail_first = tail_first && rendered.is_empty();
                    lines.extend(rendered);
                }
            }
        } else {
            let mut skipped = self.hidden_output_lines();
            for section in &self.sections {
                let take = section.lines.len().saturating_sub(skipped);
                skipped = skipped.saturating_sub(section.lines.len());
                for index in (section.lines.len() - take)..section.lines.len() {
                    let rendered = section.render_row(index, tail_first, width);
                    tail_first = tail_first && rendered.is_empty();
                    lines.extend(rendered);
                }
            }
        }

        if self.sections.iter().any(|section| section.window_truncated) {
            lines.extend(render_wrapped_tail_text(
                window_truncated_note(self.is_zh).as_str(),
                summary_style(),
                tail_first,
                width,
            ));
        }

        if expanded {
            if let Some(line) = self.footer_line() {
                lines.push(line);
            }
        }

        if let Some(line) = disclosure_line(expanded, self.card_fold(), width, self.is_zh) {
            lines.push(line);
        }
        lines
    }

    /// `✓`, or `✗ (code)` when the process came back nonzero, with the elapsed time on the
    /// same row. A collapsed card does not spend a row on an outcome its header already
    /// states; expansion is where the reference puts this.
    fn footer_line(&self) -> Option<Line<'static>> {
        let exit_code = self.exit_code?;
        let mut spans = if exit_code == 0 {
            vec![Span::styled(
                "\u{2713}".to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]
        } else {
            vec![
                Span::styled(
                    "\u{2717}".to_string(),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(" ({exit_code})")),
            ]
        };
        if let Some(duration) = self.duration_text() {
            spans.push(Span::styled(
                format!(" \u{00b7} {duration}"),
                theme::secondary_text(),
            ));
        }
        Some(Line::from(spans))
    }

    fn duration_text(&self) -> Option<String> {
        self.duration_ms
            .filter(|value| *value > 0)
            .map(format_elapsed_ms)
    }

    /// `• Ran <cmd>` on success, `• Failed (exit 1) <cmd>` on failure, with the elapsed
    /// time as a dim tail. While the command is live the bullet breathes instead.
    fn header_line(&self) -> Line<'static> {
        let running = self.status == CommandLogStatus::Pending;
        let text_style = match (running, self.success) {
            (true, _) => theme::accent_text(),
            (false, true) => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            (false, false) => theme::danger_text(),
        };
        let command = self.command.lines().next().unwrap_or_default().to_string();
        let mut spans = vec![
            Span::styled(
                if running {
                    activity_indicator::RUNNING_INDICATOR
                } else {
                    "\u{2022} "
                }
                .to_string(),
                if running {
                    activity_indicator::pending_indicator_style()
                } else {
                    text_style
                },
            ),
            Span::styled(self.title.clone(), text_style),
            Span::raw(" "),
            Span::styled(command, command_style()),
        ];
        // A multi-line command keeps its head in the header; the rest is a fold, exactly as
        // the reference marks it, whether or not this card is open.
        if self.command.lines().nth(1).is_some() {
            spans.push(Span::styled(
                " \u{2026}".to_string(),
                theme::secondary_text(),
            ));
        }
        if let Some(metrics) = self.metrics.as_ref().filter(|value| !value.is_empty()) {
            spans.push(Span::styled(
                format!(" \u{00b7} {metrics}"),
                theme::secondary_text(),
            ));
        }
        Line::from(spans)
    }
}

#[derive(Debug, Clone)]
struct CommandSection {
    label: String,
    /// Bounded window of the real output. Folding into the card tail happens at render
    /// time so the transcript overlay can show this same window without a rebuild.
    lines: Vec<String>,
    window_truncated: bool,
    style: Style,
}

/// Rows of output a collapsed command card keeps: the tail of the stream, which is what
/// the reader is watching while a command runs.
const COMMAND_TAIL_PREVIEW_LINES: usize = 3;

impl CommandSection {
    /// Render one row of this section. Only the section's first row may carry its label,
    /// and a plain output stream never announces `stdout` — the reference keeps silent too.
    fn render_row(&self, index: usize, first_body: bool, width: u16) -> Vec<Line<'static>> {
        let Some(line) = self.lines.get(index) else {
            return Vec::new();
        };
        if hides_command_section_label(self.label.as_str()) || index > 0 {
            return render_wrapped_tail_text(line.as_str(), self.style, first_body, width);
        }
        render_wrapped_labeled_tree_text(
            format!("{}{}: ", tree_prefix(first_body), self.label),
            line.as_str(),
            self.style,
            width,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatchLogStatus {
    Pending,
    Success,
    Failure,
}

#[derive(Debug, Clone)]
pub(super) struct PatchLogEntry {
    status: PatchLogStatus,
    header: String,
    summary: Option<String>,
    notes: Vec<PatchNote>,
    files: Vec<PatchFileRow>,
    diff_preview: Option<PatchDiffPreview>,
    /// The header already names the one file this card changed, so the body does not have
    /// to say the path a second time.
    header_names_file: bool,
    is_zh: bool,
}

impl PatchLogEntry {
    fn summary_text(&self) -> String {
        if let Some(summary) = self.summary.as_ref().filter(|value| !value.is_empty()) {
            format!("{} {}", self.header, summary)
        } else {
            self.header.clone()
        }
    }

    fn render_lines_for_width(&self, width: u16, expanded: bool) -> Vec<Line<'static>> {
        let summary = self.summary.as_ref().filter(|value| !value.is_empty());
        let mut used_inline_summary = false;
        let mut lines = vec![if let Some(summary) = summary {
            if let Some(line) = self.header_line_with_summary(summary.as_str(), width) {
                used_inline_summary = true;
                line
            } else {
                self.header_line()
            }
        } else {
            self.header_line()
        }];
        let has_diff_preview = self
            .diff_preview
            .as_ref()
            .is_some_and(|preview| !preview.is_empty());
        let preview_item_count = self
            .diff_preview
            .as_ref()
            .map_or(0, PatchDiffPreview::item_count);
        let mut remaining_items = self.notes.len()
            + if has_diff_preview {
                preview_item_count
            } else {
                self.files.len()
            };
        if !used_inline_summary && summary.is_some() {
            remaining_items += 1;
        }

        if !used_inline_summary {
            if let Some(summary) = summary {
                let is_last = remaining_items == 1;
                lines.extend(render_wrapped_tree_text(
                    summary.as_str(),
                    summary_style(),
                    is_last,
                    width,
                ));
                remaining_items = remaining_items.saturating_sub(1);
            }
        }

        for note in &self.notes {
            let is_last = remaining_items == 1;
            lines.extend(note.render_wrapped(width, is_last));
            remaining_items = remaining_items.saturating_sub(1);
        }

        let mut fold = None;
        if has_diff_preview {
            let blocks = self.diff_body_blocks(width);
            fold = Disclosure::new_lines(
                count_body_rows(&blocks, None)
                    .saturating_sub(count_body_rows(&blocks, Some(PATCH_PREVIEW_ROWS))),
            );
            lines.extend(render_body_blocks(
                &blocks,
                if expanded {
                    None
                } else {
                    Some(PATCH_PREVIEW_ROWS)
                },
            ));
        } else {
            for file in &self.files {
                let is_last = remaining_items == 1;
                lines.extend(file.render_wrapped(width, is_last));
                remaining_items = remaining_items.saturating_sub(1);
            }
        }

        if let Some(line) = disclosure_line(expanded, fold, width, self.is_zh) {
            lines.push(line);
        }

        lines
    }

    /// The stored diff, rendered once per file into the path row and the body rows, so the
    /// fold can count what it hides without wrapping the same text twice.
    fn diff_body_blocks(&self, width: u16) -> Vec<PatchBodyBlock> {
        let Some(preview) = self.diff_preview.as_ref() else {
            return Vec::new();
        };
        if preview.is_empty() {
            return Vec::new();
        }
        let total_items = preview.item_count();
        let mut rendered_items = 0usize;
        let mut blocks = Vec::with_capacity(preview.blocks.len());
        let hide_header = self.header_names_file && preview.blocks.len() == 1;
        for block in &preview.blocks {
            rendered_items = rendered_items.saturating_add(1);
            let is_last = rendered_items == total_items;
            blocks.push(render_patch_diff_block(block, width, is_last, hide_header));
        }
        if let Some(text) = preview.omitted_text.as_ref() {
            rendered_items = rendered_items.saturating_add(1);
            let is_last = rendered_items == total_items;
            blocks.push(PatchBodyBlock {
                header: render_wrapped_tree_text(text.as_str(), summary_style(), is_last, width),
                body: Vec::new(),
            });
        }
        blocks
    }

    /// A diff longer than the per-file preview is what Enter is for on this card, and the
    /// count is what the row says.
    fn card_fold(&self, width: u16) -> Option<Disclosure> {
        let blocks = self.diff_body_blocks(width);
        Disclosure::new_lines(
            count_body_rows(&blocks, None)
                .saturating_sub(count_body_rows(&blocks, Some(PATCH_PREVIEW_ROWS))),
        )
    }

    pub(super) fn inherit_diff_from(&mut self, previous: &PatchLogEntry) {
        if self.diff_preview.is_none() {
            self.diff_preview = previous.diff_preview.clone();
        }
        if self.diff_preview.is_some() {
            self.files.clear();
        }
    }

    fn header_line(&self) -> Line<'static> {
        let (icon, icon_style, text_style) = self.header_parts();
        let icon_span = if self.status == PatchLogStatus::Pending {
            pending_status_icon_span()
        } else {
            status_icon_span(icon, icon_style)
        };
        Line::from(vec![
            icon_span,
            Span::styled(self.header.clone(), text_style),
        ])
    }

    fn header_line_with_summary(&self, summary: &str, width: u16) -> Option<Line<'static>> {
        if summary.contains('\n') {
            return None;
        }
        let (icon, icon_style, text_style) = self.header_parts();
        let total_width =
            UnicodeWidthStr::width(format!("{icon} {} {summary}", self.header).as_str());
        if total_width > usize::from(width.max(1)) {
            return None;
        }
        let icon_span = if self.status == PatchLogStatus::Pending {
            pending_status_icon_span()
        } else {
            status_icon_span(icon, icon_style)
        };
        Some(Line::from(vec![
            icon_span,
            Span::styled(self.header.clone(), text_style),
            Span::raw(" "),
            Span::styled(summary.to_string(), summary_style()),
        ]))
    }

    fn header_parts(&self) -> (&'static str, Style, Style) {
        match self.status {
            PatchLogStatus::Pending => (
                "\u{2022}",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                theme::accent_text(),
            ),
            PatchLogStatus::Success => (
                "\u{2022}",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            PatchLogStatus::Failure => (
                "\u{2718}",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                theme::danger_text(),
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct PatchNote {
    label: Option<String>,
    text: String,
    style: Style,
}

impl PatchNote {
    fn new(label: Option<String>, text: String, style: Style) -> Self {
        Self { label, text, style }
    }

    fn render_wrapped(&self, width: u16, is_last: bool) -> Vec<Line<'static>> {
        let mut prefix = tree_item_prefix(is_last).to_string();
        if let Some(label) = self.label.as_ref() {
            prefix.push_str(label);
            prefix.push(' ');
        }
        render_wrapped_prefixed_text(
            prefix,
            tree_item_continuation_prefix(is_last).to_string(),
            theme::secondary_text(),
            self.text.as_str(),
            self.style,
            width,
        )
    }
}

#[derive(Debug, Clone)]
struct PatchFileRow {
    marker: char,
    text: String,
    meta: Option<String>,
}

impl PatchFileRow {
    fn render_wrapped(&self, width: u16, is_last: bool) -> Vec<Line<'static>> {
        let text = self.display_text();
        let first_prefix = format!("{}{} ", tree_item_prefix(is_last), self.marker);
        let wrapped = wrap_prefixed_text(
            text.as_str(),
            first_prefix.as_str(),
            tree_item_continuation_prefix(is_last),
            width,
        );
        let marker_style = match self.marker {
            'A' => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            'D' => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            'R' => Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            '…' => theme::secondary_text(),
            _ => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        };
        wrapped
            .into_iter()
            .map(|(is_first, content)| {
                if is_first {
                    Line::from(vec![
                        Span::styled(tree_item_prefix(is_last), theme::secondary_text()),
                        Span::styled(self.marker.to_string(), marker_style),
                        Span::raw(" "),
                        Span::styled(content, summary_style()),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled(
                            tree_item_continuation_prefix(is_last),
                            theme::secondary_text(),
                        ),
                        Span::styled(content, summary_style()),
                    ])
                }
            })
            .collect()
    }

    fn display_text(&self) -> String {
        if let Some(meta) = self.meta.as_ref().filter(|value| !value.is_empty()) {
            format!("{} {}", self.text, meta)
        } else {
            self.text.clone()
        }
    }
}

/// Body rows a folded patch card keeps for each file, the same depth the reference previews
/// an activity with. The stored window, not this number, bounds what expansion can reveal.
const PATCH_PREVIEW_ROWS: usize = 4;

/// Diff rows one card keeps in memory. The fold is a render-time view of this window, and
/// the window is what keeps a large patch from costing the transcript unbounded rows.
const PATCH_DIFF_WINDOW_LINES: usize = 40;

/// One file's rendered diff: the rows naming it, then its body rows.
struct PatchBodyBlock {
    header: Vec<Line<'static>>,
    body: Vec<Line<'static>>,
}

/// Rows a set of blocks yields at a per-file cap. Both the disclosure count and the renderer
/// read this, so a row can never promise a number the card would not reveal.
fn count_body_rows(blocks: &[PatchBodyBlock], per_file: Option<usize>) -> usize {
    blocks
        .iter()
        .map(|block| {
            block.header.len()
                + per_file.map_or(block.body.len(), |limit| block.body.len().min(limit))
        })
        .sum()
}

fn render_body_blocks(blocks: &[PatchBodyBlock], per_file: Option<usize>) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for block in blocks {
        lines.extend(block.header.iter().cloned());
        match per_file {
            Some(limit) => lines.extend(block.body.iter().take(limit).cloned()),
            None => lines.extend(block.body.iter().cloned()),
        }
    }
    lines
}

/// Render one file's diff. `hide_header` is the single-file case: the card header already
/// names the path, so the first body row takes the path row's place in the tree.
fn render_patch_diff_block(
    block: &PatchDiffBlock,
    width: u16,
    is_last: bool,
    hide_header: bool,
) -> PatchBodyBlock {
    let header = if hide_header {
        Vec::new()
    } else {
        render_wrapped_prefixed_text(
            tree_item_prefix(is_last).to_string(),
            tree_item_continuation_prefix(is_last).to_string(),
            theme::secondary_text(),
            block.header.as_str(),
            patch_diff_header_style(block.kind),
            width,
        )
    };
    // A bare `@@` patch carries no ranges, and a permanent blank gutter column would then
    // just eat width, so the column appears only when some line actually has a number.
    let has_gutter = block
        .lines
        .iter()
        .any(|line| line.old_line.is_some() || line.new_line.is_some());
    let body = block
        .lines
        .iter()
        .enumerate()
        .flat_map(|(index, line)| {
            let nested_prefix = if hide_header && index == 0 {
                tree_item_prefix(is_last)
            } else {
                tree_item_continuation_prefix(is_last)
            };
            render_patch_diff_line(line, nested_prefix, width, has_gutter)
        })
        .collect();
    PatchBodyBlock { header, body }
}

/// The `⋮` row that stands where content was folded away.
pub(super) fn fold_marker_line(text: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "  \u{22ee} ".to_string(),
            theme::secondary_text().add_modifier(Modifier::DIM),
        ),
        Span::styled(text, theme::secondary_text()),
    ])
}

/// What a card's fold can hand back. The counted shape only ever covers rows the expansion
/// can actually reveal, so a stored window that already dropped rows never promises them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Disclosure {
    Generic,
    Lines(usize),
}

impl Disclosure {
    pub(super) fn new_lines(hidden: usize) -> Option<Self> {
        (hidden > 0).then_some(Self::Lines(hidden))
    }
}

/// The row that carries a card's fold state: an open card offers to close it, a folded one
/// names either how many rows Enter brings back or what kind of content it reveals. No fold
/// means no row in either direction, so an open card never advertises a fold that is not there.
pub(super) fn disclosure_line(
    expanded: bool,
    disclosure: Option<Disclosure>,
    width: u16,
    is_zh: bool,
) -> Option<Line<'static>> {
    let disclosure = disclosure?;
    if expanded {
        let text = if is_zh {
            "  \u{2212} 收起".to_string()
        } else {
            "  \u{2212} Show less".to_string()
        };
        return Some(Line::from(Span::styled(text, theme::accent_text())));
    }
    let (label, hint) = match disclosure {
        Disclosure::Generic => (
            if is_zh {
                "  + 展开详情".to_string()
            } else {
                "  + Show details".to_string()
            },
            None,
        ),
        Disclosure::Lines(hidden) => {
            let label = if is_zh {
                format!("  +{hidden} 行")
            } else if hidden == 1 {
                "  +1 line".to_string()
            } else {
                format!("  +{hidden} lines")
            };
            let hint = if is_zh {
                "（enter 展开）"
            } else {
                " (enter to expand)"
            };
            (label, Some(hint))
        }
    };
    let mut text = label;
    if let Some(hint) = hint {
        let indent = usize::from(width / 4).min(/*never steal a whole column*/ 4);
        if Line::from(format!("{text}{hint}")).width() + indent <= usize::from(width) {
            text.push_str(hint);
        }
    }
    Some(Line::from(Span::styled(text, theme::accent_text())))
}

fn render_patch_diff_line(
    line: &PatchDiffLine,
    nested_prefix: &str,
    width: u16,
    has_gutter: bool,
) -> Vec<Line<'static>> {
    let (marker, prefix_style, marker_style, text_style) = patch_diff_line_display(line.kind);
    let marker_padding = " ".repeat(UnicodeWidthStr::width(marker).saturating_add(1));
    let gutter = if has_gutter
        && matches!(
            line.kind,
            PatchDiffLineKind::Context | PatchDiffLineKind::Add | PatchDiffLineKind::Delete
        ) {
        let number = match line.kind {
            PatchDiffLineKind::Add => line.new_line,
            PatchDiffLineKind::Delete => line.old_line,
            _ => line.new_line.or(line.old_line),
        };
        number.map_or(" ".repeat(DIFF_GUTTER_WIDTH + 1), |value| {
            format!("{value:>width$} ", width = DIFF_GUTTER_WIDTH)
        })
    } else {
        String::new()
    };
    // codex puts the row number in the gutter and the sign right after it (`33 - text`),
    // so the marker column stays aligned with the number column instead of leading it.
    let first_prefix = format!("{nested_prefix}{gutter}{marker} ");
    let continuation_prefix = format!("{nested_prefix}{marker_padding}{gutter}");
    wrap_prefixed_text(
        line.text.as_str(),
        first_prefix.as_str(),
        continuation_prefix.as_str(),
        width,
    )
    .into_iter()
    .map(|(is_first, content)| {
        if is_first {
            Line::from(vec![
                Span::styled(nested_prefix.to_string(), prefix_style),
                // The number column sits inside a tinted row, so it must carry the row's
                // own background; a plain dim style would leave a stripe-shaped hole.
                Span::styled(gutter.clone(), prefix_style.add_modifier(Modifier::DIM)),
                Span::styled(marker.to_string(), marker_style),
                Span::styled(" ", prefix_style),
                Span::styled(content, text_style),
            ])
        } else {
            Line::from(vec![
                Span::styled(continuation_prefix.clone(), prefix_style),
                Span::styled(content, text_style),
            ])
        }
    })
    .collect()
}

/// Right-aligned line numbers, roomy enough for a five-digit file.
const DIFF_GUTTER_WIDTH: usize = 5;

fn patch_diff_header_style(kind: PatchDiffBlockKind) -> Style {
    match kind {
        PatchDiffBlockKind::Add => theme::success_text().add_modifier(Modifier::BOLD),
        PatchDiffBlockKind::Delete => theme::danger_text(),
        PatchDiffBlockKind::Rename => theme::brand_text().add_modifier(Modifier::BOLD),
        PatchDiffBlockKind::Update => theme::accent_text(),
    }
}

fn patch_diff_line_display(kind: PatchDiffLineKind) -> (&'static str, Style, Style, Style) {
    match kind {
        PatchDiffLineKind::Hunk => (
            "@@",
            theme::diff_hunk_prefix(),
            theme::diff_hunk_text(),
            theme::diff_hunk_text(),
        ),
        PatchDiffLineKind::Context => (" ", summary_style(), summary_style(), summary_style()),
        PatchDiffLineKind::Add => (
            "+",
            theme::diff_added_prefix(),
            theme::diff_added_marker(),
            theme::diff_added_text(),
        ),
        PatchDiffLineKind::Delete => (
            "-",
            theme::diff_deleted_prefix(),
            theme::diff_deleted_marker(),
            theme::diff_deleted_text(),
        ),
        PatchDiffLineKind::Note => (
            "\u{22ee}",
            theme::secondary_text(),
            theme::secondary_text(),
            theme::secondary_text(),
        ),
    }
}

fn summary_style() -> Style {
    theme::secondary_text()
}

fn command_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// First line of a card's tail, then a hanging indent for everything that follows.
pub(super) fn tail_prefix(first: bool) -> &'static str {
    if first {
        "  \u{2514} "
    } else {
        continuation_prefix()
    }
}

pub(super) fn continuation_prefix() -> &'static str {
    "    "
}

fn tree_item_prefix(is_last: bool) -> &'static str {
    if is_last {
        "  └ "
    } else {
        "  ├ "
    }
}

fn tree_item_continuation_prefix(is_last: bool) -> &'static str {
    if is_last {
        continuation_prefix()
    } else {
        "  │ "
    }
}

fn tree_prefix(first_body: bool) -> &'static str {
    if first_body {
        "  └ "
    } else {
        continuation_prefix()
    }
}

fn hides_command_section_label(label: &str) -> bool {
    matches!(label, "stdout" | "stderr" | "output" | "??")
}

fn render_wrapped_tree_text(
    text: &str,
    style: Style,
    is_last: bool,
    width: u16,
) -> Vec<Line<'static>> {
    render_wrapped_prefixed_text(
        tree_item_prefix(is_last).to_string(),
        tree_item_continuation_prefix(is_last).to_string(),
        theme::secondary_text(),
        text,
        style,
        width,
    )
}

fn render_wrapped_tail_text(
    text: &str,
    style: Style,
    first: bool,
    width: u16,
) -> Vec<Line<'static>> {
    render_wrapped_prefixed_text(
        tail_prefix(first).to_string(),
        continuation_prefix().to_string(),
        theme::secondary_text(),
        text,
        style,
        width,
    )
}

fn render_wrapped_labeled_tree_text(
    prefix: String,
    text: &str,
    style: Style,
    width: u16,
) -> Vec<Line<'static>> {
    render_wrapped_prefixed_text(
        prefix,
        continuation_prefix().to_string(),
        theme::secondary_text(),
        text,
        style,
        width,
    )
}

fn render_wrapped_prefixed_text(
    first_prefix: String,
    continuation_prefix: String,
    prefix_style: Style,
    text: &str,
    text_style: Style,
    width: u16,
) -> Vec<Line<'static>> {
    wrap_prefixed_text(
        text,
        first_prefix.as_str(),
        continuation_prefix.as_str(),
        width,
    )
    .into_iter()
    .map(|(is_first, content)| {
        let prefix = if is_first {
            first_prefix.clone()
        } else {
            continuation_prefix.clone()
        };
        Line::from(vec![
            Span::styled(prefix, prefix_style),
            Span::styled(content, text_style),
        ])
    })
    .collect()
}

fn wrap_prefixed_text(
    text: &str,
    first_prefix: &str,
    continuation_prefix: &str,
    width: u16,
) -> Vec<(bool, String)> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let wrap_width = usize::from(width.max(1));
    let mut wrapped_lines = Vec::new();
    let mut first_segment = true;

    for raw_line in normalized.split('\n') {
        if raw_line.is_empty() {
            wrapped_lines.push((first_segment, String::new()));
            first_segment = false;
            continue;
        }

        let options = Options::new(wrap_width)
            .initial_indent(if first_segment {
                first_prefix
            } else {
                continuation_prefix
            })
            .subsequent_indent(continuation_prefix)
            .break_words(true);
        let segments = textwrap::wrap(raw_line, options);
        for (index, segment) in segments.into_iter().enumerate() {
            let segment = segment.into_owned();
            let is_first = first_segment && index == 0;
            let prefix = if is_first {
                first_prefix
            } else {
                continuation_prefix
            };
            let content = segment
                .strip_prefix(prefix)
                .unwrap_or(segment.as_str())
                .to_string();
            wrapped_lines.push((is_first, content));
        }
        first_segment = false;
    }

    if wrapped_lines.is_empty() {
        wrapped_lines.push((true, String::new()));
    }

    wrapped_lines
}

pub(super) fn build_pending_command_log(args: &Value, is_zh: bool) -> Option<SpecialLogEntry> {
    let command = extract_command_input(args)?.to_string();
    Some(SpecialLogEntry::Command(CommandLogEntry {
        status: CommandLogStatus::Pending,
        is_zh,
        success: true,
        title: if is_zh {
            "正在运行".to_string()
        } else {
            "Running".to_string()
        },
        command,
        metrics: None,
        exit_code: None,
        duration_ms: None,
        sections: Vec::new(),
    }))
}

pub(super) fn build_command_session_log(view: &CommandSessionView, is_zh: bool) -> SpecialLogEntry {
    let command = if view.command.trim().is_empty() {
        if is_zh {
            "\u{547d}\u{4ee4}".to_string()
        } else {
            "command".to_string()
        }
    } else {
        view.command.trim().to_string()
    };
    let terminal = matches!(
        view.status,
        CommandSessionDisplayStatus::Exited | CommandSessionDisplayStatus::FailedToStart
    );
    let success = view.success();
    // The reference implementation puts the exit code in the head, not in the output tail.
    let exit_note = view
        .exit_code
        .filter(|code| *code != 0)
        .map(|code| format!(" (exit {code})"))
        .unwrap_or_default();
    let title = match view.status {
        CommandSessionDisplayStatus::FailedToStart => {
            if is_zh {
                "\u{542f}\u{52a8}\u{5931}\u{8d25}".to_string()
            } else {
                "Failed to start".to_string()
            }
        }
        CommandSessionDisplayStatus::Exited => {
            if success {
                if is_zh {
                    "\u{5df2}\u{8fd0}\u{884c}".to_string()
                } else {
                    "Ran".to_string()
                }
            } else if is_zh {
                format!("\u{8fd0}\u{884c}\u{5931}\u{8d25}{exit_note}")
            } else {
                format!("Failed{exit_note}")
            }
        }
        CommandSessionDisplayStatus::Pending | CommandSessionDisplayStatus::Running => {
            if is_zh {
                "\u{6b63}\u{5728}\u{8fd0}\u{884c}".to_string()
            } else {
                "Running".to_string()
            }
        }
    };

    // Elapsed time is not a header detail: it belongs to the footer a settled card shows
    // when it opens. The header keeps only what a reader cannot get from anywhere else.
    let mut metrics = Vec::new();
    if view.timed_out {
        metrics.push("timeout".to_string());
    }
    if !terminal && !view.has_output() {
        metrics.push(if is_zh {
            "\u{7b49}\u{5f85}\u{8f93}\u{51fa}".to_string()
        } else {
            "waiting for output".to_string()
        });
    }

    let mut sections = Vec::new();
    if let Some(error) = view
        .error
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(section) = build_output_section(
            if is_zh { "\u{9519}\u{8bef}" } else { "error" },
            error,
            Style::default().fg(Color::Red),
        ) {
            sections.push(section);
        }
    }

    // One stream with one tail budget. stderr keeps its own colour but no longer gets its
    // own fold row, which is what made a single card promise expansion twice.
    if !view.pty.trim().is_empty() {
        if let Some(section) = build_output_section("output", view.pty.as_str(), summary_style()) {
            sections.push(section);
        }
    } else if !terminal || success {
        if let Some(section) = build_output_section("stdout", view.stdout.as_str(), summary_style())
        {
            sections.push(section);
        }
        if let Some(section) = build_output_section(
            "stderr",
            view.stderr.as_str(),
            Style::default().fg(Color::Yellow),
        ) {
            sections.push(section);
        }
    } else {
        if let Some(section) = build_output_section(
            "stderr",
            view.stderr.as_str(),
            Style::default().fg(Color::Red),
        ) {
            sections.push(section);
        }
        if let Some(section) = build_output_section("stdout", view.stdout.as_str(), summary_style())
        {
            sections.push(section);
        }
    }

    if terminal && sections.is_empty() {
        sections.push(empty_output_section(is_zh));
    }

    SpecialLogEntry::Command(CommandLogEntry {
        status: if terminal {
            CommandLogStatus::Completed
        } else {
            CommandLogStatus::Pending
        },
        success: !terminal || success,
        title,
        command,
        metrics: (!metrics.is_empty()).then(|| metrics.join(", ")),
        exit_code: terminal.then_some(view.exit_code.unwrap_or(if success { 0 } else { 1 })),
        duration_ms: view.duration_ms,
        sections,
        is_zh,
    })
}

pub(super) fn build_static_art_log(summary: String, lines: Vec<Line<'static>>) -> SpecialLogEntry {
    SpecialLogEntry::Art(ArtLogEntry { summary, lines })
}

pub(super) fn build_completed_command_log(payload: &Value, is_zh: bool) -> Option<SpecialLogEntry> {
    let result = payload.get("result").unwrap_or(payload);
    let data = result.get("data").unwrap_or(result);
    let first = data
        .get("results")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object)?;
    let command = first
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| extract_command_input(payload))?
        .to_string();
    let returncode = value_as_i64(first.get("returncode"))
        .or_else(|| value_as_i64(result.get("meta").and_then(|meta| meta.get("exit_code"))))
        .unwrap_or(0);
    let duration_ms = value_as_i64(result.get("meta").and_then(|meta| meta.get("duration_ms")));
    let success = returncode == 0;

    let stdout = first
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stderr = first
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let mut sections = Vec::new();
    let (first_label, first_text, second_label, second_text) = if success {
        ("stdout", stdout, "stderr", stderr)
    } else {
        ("stderr", stderr, "stdout", stdout)
    };
    let first_style = if success {
        summary_style()
    } else {
        Style::default().fg(Color::Red)
    };
    let second_style = if success {
        Style::default().fg(Color::Yellow)
    } else {
        summary_style()
    };
    if let Some(section) = build_output_section(first_label, first_text, first_style) {
        sections.push(section);
    }
    if let Some(section) = build_output_section(second_label, second_text, second_style) {
        sections.push(section);
    }
    let title = if success {
        if is_zh {
            "已运行".to_string()
        } else {
            "Ran".to_string()
        }
    } else if is_zh {
        format!("运行失败 (exit {returncode})")
    } else {
        format!("Failed (exit {returncode})")
    };
    if sections.is_empty() {
        sections.push(empty_output_section(is_zh));
    }

    Some(SpecialLogEntry::Command(CommandLogEntry {
        status: CommandLogStatus::Completed,
        success,
        title,
        command,
        metrics: None,
        exit_code: Some(returncode),
        duration_ms,
        sections,
        is_zh,
    }))
}

pub(super) fn build_pending_tool_log(
    tool_name: &str,
    args: &Value,
    is_zh: bool,
) -> SpecialLogEntry {
    SpecialLogEntry::Tool(GenericToolLogEntry {
        tool_name: tool_name.to_string(),
        status: GenericToolLogStatus::Pending,
        summary: Some(summarize_tool_args(args)),
        details: Vec::new(),
        // A pending card already shows its arguments as the summary; repeating them under
        // a fold would be the same text twice.
        arguments: None,
        is_zh,
        presentation: resolve_presentation(tool_name, args),
    })
}

/// The sub-agent control tool is one tool with many meanings; its action decides
/// the verb. Every other tool keeps the name-driven classifier.
fn resolve_presentation(tool_name: &str, args: &Value) -> ToolPresentation {
    if is_subagent_tool(tool_name) {
        if let Some(presentation) = present_subagent_action(args) {
            return presentation;
        }
    }
    present(tool_name)
}

pub(super) fn build_completed_tool_log(
    tool_name: &str,
    payload: &Value,
    is_zh: bool,
    inherited: Option<ToolPresentation>,
) -> SpecialLogEntry {
    let result = payload.get("result").unwrap_or(payload);
    let data = result.get("data").unwrap_or(result);
    let ok = result.get("ok").and_then(Value::as_bool);
    let display = summarize_tool_result(payload, is_zh);
    let mut details = Vec::new();
    if let Some(error) = result
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        details.push(PatchNote::new(
            Some(if is_zh { "错误:" } else { "Error:" }.to_string()),
            error.to_string(),
            Style::default().fg(Color::Red),
        ));
    }
    if let Some(repair) = result
        .get("meta")
        .and_then(|value| value.get("repair"))
        .or_else(|| payload.get("repair"))
        .and_then(format_repair_summary)
    {
        details.push(PatchNote::new(
            Some("note:".to_string()),
            repair,
            summary_style(),
        ));
    }

    if let Some(display) = display.as_ref() {
        for detail in &display.details {
            details.push(PatchNote::new(
                detail.label.clone(),
                detail.text.clone(),
                summary_style(),
            ));
        }
    }

    let summary = if let Some(display) = display {
        display.summary.filter(|value| !value.is_empty())
    } else if !(data.is_null()
        || data.is_object() && data.as_object().is_some_and(|map| map.is_empty()))
    {
        // The runtime's display helper is written for the Chinese native clients; the TUI
        // owns its wording in both languages, so an unknown tool shows its target or a
        // short preview instead of borrowing another client's prose.
        tool_target_preview(payload, data)
    } else if details.is_empty() {
        None
    } else {
        None
    };

    // A remote call's result only reads properly next to what was asked for, so its
    // arguments ride on the open card. Local tools already name their target in the header.
    let presentation = inherited.unwrap_or_else(|| present(tool_name));
    let arguments = if presentation.access == ToolAccess::Delegate {
        // Live events carry `args` at the top level; a replayed durable row can arrive
        // already wrapped, so read whichever copy holds it.
        payload
            .get("args")
            .or_else(|| payload.get("arguments"))
            .or_else(|| result.get("args"))
            .or_else(|| result.get("arguments"))
            .filter(|value| !value.is_null())
            .map(compact_json)
            .filter(|text| !text.is_empty() && text != "{}")
    } else {
        None
    };

    SpecialLogEntry::Tool(GenericToolLogEntry {
        tool_name: tool_name.to_string(),
        status: if ok == Some(false) {
            GenericToolLogStatus::Failure
        } else {
            GenericToolLogStatus::Success
        },
        summary,
        details,
        arguments,
        is_zh,
        presentation,
    })
}

pub(super) fn build_pending_patch_log(args: &Value, is_zh: bool) -> Option<SpecialLogEntry> {
    let patch = extract_patch_input(args)?;
    let summary = summarize_patch_input(patch);
    let diff_preview = {
        let preview = build_patch_diff_preview(patch, 12, PATCH_DIFF_WINDOW_LINES, is_zh);
        (!preview.is_empty()).then_some(preview)
    };
    let mut files = collect_patch_preview_rows(patch, 12, is_zh);
    let mut header = if is_zh {
        "\u{5e94}\u{7528}\u{8865}\u{4e01}".to_string()
    } else {
        "Applying patch".to_string()
    };
    let mut summary = (!summary.is_empty()).then_some(summary);
    let mut header_names_file = false;
    if let [file] = files.as_slice() {
        if file.marker != '\u{2026}' {
            header = format!("{header} {}", file.text);
            summary = file.meta.clone().or(summary);
            files.clear();
            header_names_file = true;
        }
    }
    Some(SpecialLogEntry::Patch(PatchLogEntry {
        status: PatchLogStatus::Pending,
        header,
        summary,
        notes: Vec::new(),
        files,
        diff_preview,
        header_names_file,
        is_zh,
    }))
}
/// A finished change reads its diff from the engine's own blocks. The pending card had to
/// parse what the model asked for; a write or a literal replace never asked for anything a
/// parser could read, so only the applied result can draw this card at all.
fn completed_diff_preview(data: &Value, is_zh: bool) -> Option<PatchDiffPreview> {
    let files = data.get("files").and_then(Value::as_array)?;
    let preview = patch_diff_preview_from_blocks(files, 12, PATCH_DIFF_WINDOW_LINES, is_zh);
    (!preview.is_empty()).then_some(preview)
}

pub(super) fn build_completed_patch_log(payload: &Value, is_zh: bool) -> SpecialLogEntry {
    let result = payload.get("result").unwrap_or(payload);
    let data = result.get("data").unwrap_or(result);
    let ok = result.get("ok").and_then(Value::as_bool).unwrap_or(true);
    let changed_files = value_as_i64(data.get("changed_files"))
        .or_else(|| value_as_i64(result.get("changed_files")))
        .unwrap_or(0)
        .max(0);
    let added = value_as_i64(data.get("added")).unwrap_or(0).max(0);
    let updated = value_as_i64(data.get("updated")).unwrap_or(0).max(0);
    let deleted = value_as_i64(data.get("deleted")).unwrap_or(0).max(0);
    let moved = value_as_i64(data.get("moved")).unwrap_or(0).max(0);
    let hunks = value_as_i64(data.get("hunks_applied"))
        .or_else(|| value_as_i64(result.get("hunks_applied")))
        .unwrap_or(0)
        .max(0) as usize;

    let mut header = if ok {
        if is_zh {
            format!("已修改 {} 个文件", changed_files.max(0))
        } else if changed_files == 1 {
            "Edited 1 file".to_string()
        } else {
            format!("Edited {changed_files} files")
        }
    } else if is_zh {
        "补丁应用失败".to_string()
    } else {
        "Failed to apply patch".to_string()
    };

    let added_lines = value_as_i64(data.get("added_lines"))
        .or_else(|| value_as_i64(result.get("added_lines")))
        .unwrap_or(0)
        .max(0) as usize;
    let deleted_lines = value_as_i64(data.get("deleted_lines"))
        .or_else(|| value_as_i64(result.get("deleted_lines")))
        .unwrap_or(0)
        .max(0) as usize;

    // Line counts are what the pending card already promised, so the completed
    // card keeps them; the per-status file counts only speak when lines are absent.
    let mut summary_parts = Vec::new();
    if let Some(delta) = format_patch_delta_meta(added_lines, deleted_lines, hunks, is_zh) {
        summary_parts.push(delta);
    } else {
        if added > 0 {
            summary_parts.push(format!("+{added}"));
        }
        if updated > 0 {
            summary_parts.push(format!("~{updated}"));
        }
        if deleted > 0 {
            summary_parts.push(format!("-{deleted}"));
        }
        if moved > 0 {
            summary_parts.push(format!("↦{moved}"));
        }
        if hunks > 0 {
            summary_parts.push(format_patch_hunk_count(hunks, is_zh));
        }
    }

    let mut notes = Vec::new();
    if let Some(error) = result
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        notes.push(PatchNote::new(
            Some(if is_zh {
                "错误:".to_string()
            } else {
                "Error:".to_string()
            }),
            error.to_string(),
            Style::default().fg(Color::Red),
        ));
    }
    if let Some(code) = data
        .get("error_code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        notes.push(PatchNote::new(
            Some("code:".to_string()),
            code.to_string(),
            summary_style(),
        ));
    }
    if let Some(hint) = data
        .get("hint")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        notes.push(PatchNote::new(
            Some(if is_zh {
                "提示:".to_string()
            } else {
                "Hint:".to_string()
            }),
            hint.to_string(),
            summary_style(),
        ));
    }

    let mut files = data
        .get("files")
        .and_then(Value::as_array)
        .map(|items| collect_result_file_rows(items, 24, is_zh))
        .unwrap_or_default();
    let mut summary = (!summary_parts.is_empty()).then(|| summary_parts.join(", "));

    let mut header_names_file = false;
    if ok {
        if let [file] = files.as_slice() {
            if file.marker != '…' {
                header = single_file_patch_header(file, is_zh);
                summary = file.meta.clone().or(summary);
                files.clear();
                header_names_file = true;
            }
        }
    }

    SpecialLogEntry::Patch(PatchLogEntry {
        status: if ok {
            PatchLogStatus::Success
        } else {
            PatchLogStatus::Failure
        },
        header,
        summary,
        notes,
        files,
        diff_preview: completed_diff_preview(data, is_zh),
        header_names_file,
        is_zh,
    })
}

fn single_file_patch_header(file: &PatchFileRow, is_zh: bool) -> String {
    match file.marker {
        'A' => {
            if is_zh {
                format!("已新增 {}", file.text)
            } else {
                format!("Added {}", file.text)
            }
        }
        'D' => {
            if is_zh {
                format!("已删除 {}", file.text)
            } else {
                format!("Deleted {}", file.text)
            }
        }
        'R' => {
            if is_zh {
                format!("已重命名 {}", file.text)
            } else {
                format!("Renamed {}", file.text)
            }
        }
        _ => {
            if is_zh {
                format!("已修改 {}", file.text)
            } else {
                format!("Edited {}", file.text)
            }
        }
    }
}

fn extract_patch_input(args: &Value) -> Option<&str> {
    if let Value::String(value) = args {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    let object = args.as_object()?;
    for key in ["input", "patch", "content", "raw"] {
        if let Some(value) = object.get(key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn extract_command_input(args: &Value) -> Option<&str> {
    if let Value::String(value) = args {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    let object = args.as_object()?;
    for key in ["content", "command", "cmd", "text"] {
        if let Some(value) = object.get(key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn summarize_tool_args(args: &Value) -> String {
    if let Some((key, value)) = target_argument(args) {
        return format!("{key}={value}");
    }
    compact_json(args)
}

/// `(ok, one-line note)` for a child of the `Exploring` card. A row has room for one idea:
/// a success keeps the classifier's summary, a failure keeps the first line of the error.
pub(super) fn exploring_result_note(payload: &Value, is_zh: bool) -> (bool, Option<String>) {
    const NOTE_MAX_CHARS: usize = 90;
    let result = payload.get("result").unwrap_or(payload);
    let ok = result.get("ok").and_then(Value::as_bool) != Some(false);
    let text = if ok {
        summarize_tool_result(payload, is_zh)
            .and_then(|display| display.summary)
            .map(|summary| summary.trim().to_string())
            .filter(|summary| !summary.is_empty() && summary != "{}")
    } else {
        result
            .get("error")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|error| !error.is_empty())
            .map(str::to_string)
    };
    let note = text.map(|text| {
        let first_line = text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_default();
        let mut capped: String = first_line.chars().take(NOTE_MAX_CHARS).collect();
        if first_line.chars().count() > NOTE_MAX_CHARS {
            capped.push('\u{2026}');
        }
        capped
    });
    (ok, note)
}

fn format_repair_summary(repair: &Value) -> Option<String> {
    let strategy = repair.get("strategy").and_then(Value::as_str).unwrap_or("");
    let count = repair.get("count").and_then(Value::as_u64).unwrap_or(0);
    match strategy {
        "sanitize_before_request" if count > 0 => Some(format!(
            "sanitized {count} malformed tool-call argument payload(s)"
        )),
        "lossy_json_string_repair" => {
            Some("recovered malformed JSON arguments before execution".to_string())
        }
        "raw_arguments_wrapped" => {
            Some("wrapped non-JSON arguments before sending them upstream".to_string())
        }
        "non_object_arguments_wrapped" => {
            Some("wrapped non-object arguments into JSON before sending them upstream".to_string())
        }
        _ => None,
    }
}

/// The one line an unknown tool can carry: its target argument, or a short preview of the
/// result data. Both are language-neutral, which is what the transcript needs.
fn tool_target_preview(payload: &Value, data: &Value) -> Option<String> {
    let args = payload
        .get("args")
        .or_else(|| payload.get("arguments"))
        .unwrap_or(&Value::Null);
    ["path", "file_path", "command", "cmd", "query", "url"]
        .iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
        .map(|value| value.lines().next().unwrap_or(value).trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let compact = compact_json(data);
            (!compact.is_empty() && compact != "{}" && compact != "null").then_some(compact)
        })
}

fn compact_json(value: &Value) -> String {
    const MAX_INLINE_JSON_CHARS: usize = 220;
    crate::render::compact_json_within(value, MAX_INLINE_JSON_CHARS)
}

/// What one card keeps in memory. The transcript overlay reads this window, so it is
/// generous, but it must stay bounded: transcripts hold one card per tool call.
const OUTPUT_WINDOW_LINES: usize = 120;
const OUTPUT_WINDOW_CHARS: usize = 12_000;

/// Normalise output down to a head+tail window, reporting whether anything fell outside.
fn output_window(text: &str) -> (Vec<String>, bool) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim_end_matches('\n').trim();
    if trimmed.is_empty() {
        return (Vec::new(), false);
    }
    let (window, chars_cut) = truncate_by_chars(trimmed, OUTPUT_WINDOW_CHARS);
    let lines = window.lines().map(ToString::to_string).collect::<Vec<_>>();
    if lines.is_empty() {
        return (Vec::new(), chars_cut);
    }
    if lines.len() <= OUTPUT_WINDOW_LINES {
        return (lines, chars_cut);
    }
    let keep_head = OUTPUT_WINDOW_LINES / 2;
    let keep_tail = OUTPUT_WINDOW_LINES - keep_head;
    let mut kept = lines[..keep_head].to_vec();
    kept.extend_from_slice(&lines[lines.len() - keep_tail..]);
    (kept, true)
}

/// A terminal command that produced nothing still needs a tail line, or the card reads
/// as if output is still coming.
fn empty_output_section(is_zh: bool) -> CommandSection {
    CommandSection {
        label: if is_zh { "输出" } else { "output" }.to_string(),
        // The reference states a silent run as a sentence, not as a sentinel from a
        // serialiser: `<empty>` read like the stream had a literal value rather than none.
        lines: vec![if is_zh {
            "（无输出）"
        } else {
            "(no output)"
        }
        .to_string()],
        window_truncated: false,
        style: theme::secondary_text(),
    }
}

fn build_output_section(label: &str, text: &str, style: Style) -> Option<CommandSection> {
    let (lines, window_truncated) = output_window(text);
    if lines.is_empty() {
        return None;
    }
    Some(CommandSection {
        label: label.to_string(),
        lines,
        window_truncated,
        style,
    })
}

fn truncate_by_chars(text: &str, max_chars: usize) -> (String, bool) {
    if max_chars == 0 {
        return (String::new(), !text.is_empty());
    }
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let mut output = String::new();
    for ch in text.chars().take(max_chars) {
        output.push(ch);
    }
    (output, true)
}

/// Elapsed time as the reference states it: whole milliseconds under a second, two decimal
/// seconds under a minute, then minutes with a zero-padded remainder.
fn format_elapsed_ms(elapsed_ms: i64) -> String {
    if elapsed_ms < 1_000 {
        return format!("{elapsed_ms}ms");
    }
    if elapsed_ms < 60_000 {
        return format!("{:.2}s", elapsed_ms as f64 / 1000.0);
    }
    let minutes = elapsed_ms / 60_000;
    let seconds = (elapsed_ms % 60_000) / 1_000;
    format!("{minutes}m {seconds:02}s")
}

/// The row that marks output the stored window already dropped. It names no count and no
/// key: those lines are gone, so promising `ctrl+t` would be a lie. Revealable rows are
/// counted by the disclosure control instead.
fn window_truncated_note(is_zh: bool) -> String {
    if is_zh {
        "\u{2026} \u{00b7} 窗口已截断".to_string()
    } else {
        "\u{2026} \u{00b7} window truncated".to_string()
    }
}

fn summarize_patch_input(patch: &str) -> String {
    let line_count = patch.lines().count();
    let metrics = count_patch_input_metrics(patch);
    let file_count = metrics.file_count;

    if file_count > 0 {
        let mut parts = vec![format!("files={file_count}")];
        if metrics.added_lines > 0 || metrics.removed_lines > 0 {
            parts.push(format!("+{}", metrics.added_lines));
            parts.push(format!("-{}", metrics.removed_lines));
        } else if line_count > 0 {
            parts.push(format!("lines={line_count}"));
        }
        parts.join(", ")
    } else if line_count > 0 {
        format!("lines={line_count}")
    } else {
        String::new()
    }
}

fn collect_patch_preview_rows(patch: &str, max_entries: usize, is_zh: bool) -> Vec<PatchFileRow> {
    let mut rows = Vec::new();
    let mut last_update_index = None::<usize>;
    for raw_line in patch.lines() {
        let line = raw_line.trim();
        if let Some(path) = line.strip_prefix("*** Add File:") {
            rows.push(PatchFileRow {
                marker: 'A',
                text: path.trim().to_string(),
                meta: None,
            });
            last_update_index = None;
        } else if let Some(path) = line.strip_prefix("*** Delete File:") {
            rows.push(PatchFileRow {
                marker: 'D',
                text: path.trim().to_string(),
                meta: None,
            });
            last_update_index = None;
        } else if let Some(path) = line.strip_prefix("*** Update File:") {
            rows.push(PatchFileRow {
                marker: 'M',
                text: path.trim().to_string(),
                meta: None,
            });
            last_update_index = Some(rows.len().saturating_sub(1));
        } else if let Some(path) = line.strip_prefix("*** Move to:") {
            if let Some(index) = last_update_index {
                if let Some(existing) = rows.get_mut(index) {
                    existing.marker = 'R';
                    existing.text = format!("{} → {}", existing.text, path.trim());
                }
            }
        } else if let Some(index) = rows.len().checked_sub(1) {
            if let Some(existing) = rows.get_mut(index) {
                if line.starts_with("@@") {
                    existing.meta = increment_patch_hunk_meta(existing.meta.take(), is_zh);
                } else if raw_line.starts_with('+') {
                    existing.meta = increment_patch_delta_meta(existing.meta.take(), 1, 0, is_zh);
                } else if raw_line.starts_with('-') {
                    existing.meta = increment_patch_delta_meta(existing.meta.take(), 0, 1, is_zh);
                }
            }
        }
    }
    let hidden_count = rows.len().saturating_sub(max_entries);
    if rows.len() > max_entries {
        rows.truncate(max_entries);
    }
    if hidden_count > 0 {
        rows.push(omitted_patch_file_row(hidden_count, is_zh));
    }
    rows
}

fn collect_result_file_rows(items: &[Value], max_entries: usize, is_zh: bool) -> Vec<PatchFileRow> {
    let mut rows = items
        .iter()
        .filter_map(|item| extract_result_file_row(item, is_zh))
        .collect::<Vec<_>>();
    let hidden_count = rows.len().saturating_sub(max_entries);
    if rows.len() > max_entries {
        rows.truncate(max_entries);
    }
    if hidden_count > 0 {
        rows.push(omitted_patch_file_row(hidden_count, is_zh));
    }
    rows
}

fn omitted_patch_file_row(hidden_count: usize, is_zh: bool) -> PatchFileRow {
    PatchFileRow {
        marker: '…',
        text: if is_zh {
            format!("还有 {hidden_count} 个文件")
        } else {
            format!("+{hidden_count} more files")
        },
        meta: None,
    }
}

fn extract_result_file_row(file: &Value, is_zh: bool) -> Option<PatchFileRow> {
    let object = file.as_object()?;
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let to_path = object
        .get("to_path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if path.is_empty() && to_path.is_empty() {
        return None;
    }

    let has_move = !path.is_empty() && !to_path.is_empty() && to_path != path;
    let marker = match action.as_str() {
        "add" => 'A',
        "delete" => 'D',
        "update" if has_move => 'R',
        "update" => 'M',
        "move" => 'R',
        _ => 'M',
    };
    let text = if has_move {
        format!("{path} → {to_path}")
    } else if !path.is_empty() {
        path.to_string()
    } else {
        to_path.to_string()
    };
    let hunks = value_as_i64(object.get("hunks")).unwrap_or(0).max(0) as usize;
    let added_lines = value_as_i64(object.get("added_lines")).unwrap_or(0).max(0) as usize;
    let deleted_lines = value_as_i64(object.get("deleted_lines"))
        .unwrap_or(0)
        .max(0) as usize;

    Some(PatchFileRow {
        marker,
        text,
        meta: format_patch_delta_meta(added_lines, deleted_lines, hunks, is_zh)
            .or_else(|| format_patch_hunk_meta(hunks, is_zh)),
    })
}

#[derive(Default)]
struct PatchInputMetrics {
    file_count: usize,
    added_lines: usize,
    removed_lines: usize,
}

fn count_patch_input_metrics(patch: &str) -> PatchInputMetrics {
    let mut metrics = PatchInputMetrics::default();
    let mut in_file = false;
    for raw_line in patch.lines() {
        let line = raw_line.trim();
        if line.starts_with("*** Add File:")
            || line.starts_with("*** Update File:")
            || line.starts_with("*** Delete File:")
        {
            metrics.file_count += 1;
            in_file = true;
            continue;
        }
        if line.starts_with("*** ") {
            in_file = false;
            continue;
        }
        if !in_file {
            continue;
        }
        if raw_line.starts_with('+') {
            metrics.added_lines += 1;
        } else if raw_line.starts_with('-') {
            metrics.removed_lines += 1;
        }
    }
    metrics
}

fn parse_patch_delta_meta(meta: Option<String>) -> (usize, usize, usize) {
    let Some(meta) = meta else {
        return (0, 0, 0);
    };
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut hunks = 0usize;
    for token in meta
        .trim_matches(|ch| ch == '(' || ch == ')')
        .split_whitespace()
    {
        if let Some(value) = token.strip_prefix('+') {
            added = value.parse::<usize>().unwrap_or(added);
        } else if let Some(value) = token.strip_prefix('-') {
            removed = value.parse::<usize>().unwrap_or(removed);
        } else if let Some(value) = token.strip_suffix("hunk") {
            hunks = value.trim().parse::<usize>().unwrap_or(hunks);
        } else if let Some(value) = token.strip_suffix("hunks") {
            hunks = value.trim().parse::<usize>().unwrap_or(hunks);
        } else if let Some(value) = token.strip_suffix("\u{4e2a}\u{5757}") {
            hunks = value.trim().parse::<usize>().unwrap_or(hunks);
        } else if token.chars().all(|ch| ch.is_ascii_digit()) {
            hunks = token.parse::<usize>().unwrap_or(hunks);
        }
    }
    (added, removed, hunks)
}

fn increment_patch_delta_meta(
    meta: Option<String>,
    added_delta: usize,
    removed_delta: usize,
    is_zh: bool,
) -> Option<String> {
    let (added, removed, hunks) = parse_patch_delta_meta(meta);
    format_patch_delta_meta(added + added_delta, removed + removed_delta, hunks, is_zh)
}

fn increment_patch_hunk_meta(meta: Option<String>, is_zh: bool) -> Option<String> {
    let (added, removed, hunks) = parse_patch_delta_meta(meta);
    format_patch_delta_meta(added, removed, hunks + 1, is_zh)
}

fn format_patch_delta_meta(
    added: usize,
    removed: usize,
    hunks: usize,
    is_zh: bool,
) -> Option<String> {
    if added == 0 && removed == 0 && hunks == 0 {
        return None;
    }
    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("+{added}"));
    }
    if removed > 0 {
        parts.push(format!("-{removed}"));
    }
    if hunks > 0 {
        parts.push(format_patch_hunk_count(hunks, is_zh));
    }
    Some(format!("({})", parts.join(" ")))
}

fn format_patch_hunk_count(hunks: usize, is_zh: bool) -> String {
    if is_zh {
        format!("{hunks} \u{4e2a}\u{5757}")
    } else if hunks == 1 {
        "1 hunk".to_string()
    } else {
        format!("{hunks} hunks")
    }
}

fn format_patch_hunk_meta(hunks: usize, is_zh: bool) -> Option<String> {
    if hunks == 0 {
        None
    } else {
        Some(format!("({})", format_patch_hunk_count(hunks, is_zh)))
    }
}

fn value_as_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(|item| {
        item.as_i64()
            .or_else(|| {
                item.as_u64()
                    .map(|number| number.min(i64::MAX as u64) as i64)
            })
            .or_else(|| {
                item.as_str()
                    .and_then(|text| text.trim().parse::<i64>().ok())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_patch_log_collects_preview_rows() {
        let args = serde_json::json!({
            "input": "*** Begin Patch\n*** Update File: src/main.rs\n*** Move to: src/bin/main.rs\n*** Add File: src/lib.rs\n*** End Patch"
        });
        let SpecialLogEntry::Patch(entry) = build_pending_patch_log(&args, false).expect("patch")
        else {
            panic!("expected patch log");
        };
        assert_eq!(entry.status, PatchLogStatus::Pending);
        assert_eq!(entry.summary.as_deref(), Some("files=2, lines=5"));
        assert_eq!(entry.files.len(), 2);
        assert_eq!(entry.files[0].marker, 'R');
        assert!(entry.files[0]
            .text
            .contains("src/main.rs → src/bin/main.rs"));
        assert_eq!(entry.files[1].marker, 'A');
    }

    #[test]
    fn pending_patch_single_file_moves_path_into_header() {
        let args = serde_json::json!({
            "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** End Patch"
        });
        let SpecialLogEntry::Patch(entry) = build_pending_patch_log(&args, false).expect("patch")
        else {
            panic!("expected patch log");
        };
        assert!(entry.header.contains("Applying patch src/main.rs"));
        assert_eq!(entry.files.len(), 0);
        assert!(entry
            .summary
            .as_deref()
            .is_some_and(|value| value.contains("+1")));
    }

    #[test]
    fn completed_patch_log_builds_failure_notes() {
        let payload = serde_json::json!({
            "result": {
                "ok": false,
                "error": "Patch apply failed",
                "data": {
                    "error_code": "PATCH_CONTEXT_NOT_FOUND",
                    "hint": "Read latest file content and regenerate patch"
                }
            }
        });
        let SpecialLogEntry::Patch(entry) = build_completed_patch_log(&payload, false) else {
            panic!("expected patch log");
        };
        assert_eq!(entry.status, PatchLogStatus::Failure);
        assert_eq!(entry.header, "Failed to apply patch");
        assert!(entry
            .notes
            .iter()
            .any(|note| note.text.contains("PATCH_CONTEXT_NOT_FOUND")));
        assert!(entry
            .notes
            .iter()
            .any(|note| note.text.contains("regenerate patch")));
    }

    #[test]
    fn completed_patch_single_file_moves_path_into_header() {
        let payload = serde_json::json!({
            "result": {
                "ok": true,
                "data": {
                    "changed_files": 1,
                    "updated": 1,
                    "hunks_applied": 2,
                    "files": [{
                        "action": "update",
                        "path": "src/main.rs",
                        "hunks": 2
                    }]
                }
            }
        });
        let SpecialLogEntry::Patch(entry) = build_completed_patch_log(&payload, false) else {
            panic!("expected patch log");
        };
        assert_eq!(entry.header, "Edited src/main.rs");
        assert_eq!(entry.files.len(), 0);
        assert_eq!(entry.summary.as_deref(), Some("(2 hunks)"));
    }

    #[test]
    fn completed_patch_single_file_rename_uses_rename_header() {
        let payload = serde_json::json!({
            "result": {
                "ok": true,
                "data": {
                    "changed_files": 1,
                    "moved": 1,
                    "files": [{
                        "action": "move",
                        "path": "src/old.rs",
                        "to_path": "src/new.rs",
                        "hunks": 1
                    }]
                }
            }
        });
        let SpecialLogEntry::Patch(entry) = build_completed_patch_log(&payload, false) else {
            panic!("expected patch log");
        };
        assert_eq!(entry.header, "Renamed src/old.rs → src/new.rs");
        assert_eq!(entry.summary.as_deref(), Some("(1 hunk)"));
    }

    #[test]
    fn multi_file_patch_render_uses_tree_connectors() {
        let payload = serde_json::json!({
            "result": {
                "ok": true,
                "data": {
                    "changed_files": 2,
                    "added": 1,
                    "updated": 1,
                    "files": [
                        {
                            "action": "add",
                            "path": "src/new.rs",
                            "hunks": 1
                        },
                        {
                            "action": "move",
                            "path": "src/old.rs",
                            "to_path": "src/main.rs",
                            "hunks": 2
                        }
                    ]
                }
            }
        });
        let SpecialLogEntry::Patch(entry) = build_completed_patch_log(&payload, false) else {
            panic!("expected patch log");
        };
        let rendered = entry
            .render_lines_for_width(80, false)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered
            .iter()
            .any(|line| line.contains("  ├ A src/new.rs (1 hunk)")));
        assert!(rendered
            .iter()
            .any(|line| line.contains("  └ R src/old.rs → src/main.rs (2 hunks)")));
    }

    #[test]
    fn pending_command_log_uses_running_title() {
        let args = serde_json::json!({ "content": "git status --short" });
        let SpecialLogEntry::Command(entry) =
            build_pending_command_log(&args, false).expect("command")
        else {
            panic!("expected command log");
        };
        assert_eq!(entry.status, CommandLogStatus::Pending);
        assert_eq!(entry.title, "Running");
        assert_eq!(entry.command, "git status --short");
    }

    #[test]
    fn completed_command_log_collects_metrics_and_output() {
        let payload = serde_json::json!({
            "result": {
                "data": {
                    "results": [{
                        "command": "git status --short",
                        "returncode": 1,
                        "stdout": "",
                        "stderr": "fatal: not a git repository\nuse --help for more information"
                    }]
                },
                "meta": {
                    "exit_code": 1,
                    "duration_ms": 28
                }
            }
        });
        let SpecialLogEntry::Command(entry) =
            build_completed_command_log(&payload, false).expect("command")
        else {
            panic!("expected command log");
        };
        assert_eq!(entry.status, CommandLogStatus::Completed);
        assert!(!entry.success);
        assert_eq!(entry.title, "Failed (exit 1)");
        // The elapsed time is a footer detail now: the collapsed header keeps the outcome,
        // and opening the card is what spends a row on how long it took.
        assert_eq!(entry.metrics, None);
        assert_eq!(entry.duration_ms, Some(28));
        assert_eq!(entry.exit_code, Some(1));
        assert!(entry
            .sections
            .iter()
            .any(|section| section.label == "stderr"));
        assert!(entry
            .sections
            .iter()
            .flat_map(|section| section.lines.iter())
            .any(|line| line.contains("fatal: not a git repository")));
    }

    #[test]
    fn completed_command_log_keeps_the_window_and_folds_only_when_rendered() {
        let stderr = (1..=8)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let payload = serde_json::json!({
            "result": {
                "data": {
                    "results": [{
                        "command": "demo",
                        "returncode": 1,
                        "stdout": "",
                        "stderr": stderr
                    }]
                },
                "meta": { "exit_code": 1 }
            }
        });
        let SpecialLogEntry::Command(entry) =
            build_completed_command_log(&payload, false).expect("command")
        else {
            panic!("expected command log");
        };
        let stderr_section = entry
            .sections
            .iter()
            .find(|section| section.label == "stderr")
            .expect("stderr section");
        assert_eq!(
            stderr_section.lines.len(),
            8,
            "the card must keep the window so the transcript can open it"
        );
        assert!(!stderr_section.window_truncated);

        let folded = flat_command_lines(&entry, 80, false);
        // The disclosure counts the folded head *and* the outcome row the open card adds.
        let hidden = 8 - COMMAND_TAIL_PREVIEW_LINES + 1;
        assert!(
            folded
                .iter()
                .any(|line| line.contains(format!("+{hidden} lines").as_str())),
            "the disclosure row counts the folded head: {folded:?}"
        );
        assert!(
            folded.iter().any(|line| line.contains("line 8")),
            "the tail is what stays on the card: {folded:?}"
        );
        assert!(!folded.iter().any(|line| line.contains("line 4")));

        let opened = flat_command_lines(&entry, 80, true);
        assert!(
            opened.iter().any(|line| line.contains("line 1")),
            "the open card shows the whole window: {opened:?}"
        );
        assert!(
            opened.iter().any(|line| line.contains("line 4")),
            "the transcript shows the whole window: {opened:?}"
        );
        assert!(
            !opened.iter().any(|line| line.contains("lines (enter")),
            "an open card has nothing left to promise: {opened:?}"
        );
    }

    #[test]
    fn running_command_session_log_streams_stdout_before_failure_ordering() {
        let view = CommandSessionView {
            primary_id: "cmd_1".to_string(),
            command_session_id: Some("cmd_1".to_string()),
            tool_call_id: Some("call_1".to_string()),
            command: "demo".to_string(),
            cwd: None,
            status: CommandSessionDisplayStatus::Running,
            exit_code: None,
            timed_out: false,
            error: None,
            duration_ms: None,
            stdout: "line 1\nline 2".to_string(),
            stderr: "warning".to_string(),
            pty: String::new(),
        };
        let SpecialLogEntry::Command(entry) = build_command_session_log(&view, false) else {
            panic!("expected command log");
        };
        assert_eq!(entry.status, CommandLogStatus::Pending);
        assert_eq!(entry.sections[0].label, "stdout");
        assert_eq!(entry.sections[1].label, "stderr");
        assert!(entry
            .sections
            .iter()
            .flat_map(|section| section.lines.iter())
            .any(|line| line.contains("line 1")));
    }

    #[test]
    fn pending_generic_tool_log_uses_args_summary() {
        let entry = build_pending_tool_log(
            "读取文件",
            &serde_json::json!({ "path": "src/main.rs" }),
            true,
        );
        let SpecialLogEntry::Tool(entry) = entry else {
            panic!("expected tool log");
        };
        assert_eq!(entry.status, GenericToolLogStatus::Pending);
        assert_eq!(entry.summary.as_deref(), Some("path=src/main.rs"));
    }

    #[test]
    fn an_unknown_tools_summary_stays_in_the_sessions_language() {
        let entry = build_completed_tool_log(
            "vendor_lookup",
            &serde_json::json!({
                "args": { "path": "src/demo.rs" },
                "result": { "ok": true, "data": { "hits": 3 } }
            }),
            false,
            None,
        );
        let SpecialLogEntry::Tool(card) = entry else {
            panic!("tool log");
        };
        let summary = card.summary.unwrap_or_default();
        assert_eq!(summary, "src/demo.rs", "the target carries the card");
        assert!(
            !summary.chars().any(|ch| ('一'..='鿿').contains(&ch)),
            "an English session never shows another client's wording: {summary}"
        );
    }

    #[test]
    fn generic_tool_header_inlines_summary_when_width_allows() {
        let entry = build_pending_tool_log(
            "read_file",
            &serde_json::json!({ "path": "src/main.rs" }),
            false,
        );
        let SpecialLogEntry::Tool(entry) = entry else {
            panic!("expected tool log");
        };
        let rendered = entry
            .render_lines_for_width(80, true)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].contains("Read path=src/main.rs"));
    }

    #[test]
    fn generic_tool_summary_moves_to_body_when_width_small() {
        let entry = build_pending_tool_log(
            "read_file",
            &serde_json::json!({ "path": "src/deeply/nested/project/main.rs" }),
            false,
        );
        let SpecialLogEntry::Tool(entry) = entry else {
            panic!("expected tool log");
        };
        let rendered = entry
            .render_lines_for_width(24, true)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered.len() > 1);
        assert!(rendered.iter().any(|line| line.starts_with("  └ ")));
    }

    #[test]
    fn completed_generic_tool_log_uses_summary_as_header() {
        let entry = build_completed_tool_log(
            "read_file",
            &serde_json::json!({
                "result": {
                    "ok": true,
                    "data": {
                        "content": "fn main() {}",
                        "meta": { "files": [{ "path": "src/main.rs", "bytes": 12 }] }
                    }
                }
            }),
            false,
            None,
        );
        let SpecialLogEntry::Tool(entry) = entry else {
            panic!("expected tool log");
        };
        assert!(entry.header_text().contains("src/main.rs"));
    }

    #[test]
    fn patch_header_inlines_summary_when_width_allows() {
        let args = serde_json::json!({
            "input": "*** Update File: src/main.rs\n@@\n-old\n+new\n"
        });
        let SpecialLogEntry::Patch(entry) = build_pending_patch_log(&args, false).expect("patch")
        else {
            panic!("expected patch log");
        };
        let rendered = entry
            .render_lines_for_width(80, false)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered[0].contains("Applying patch src/main.rs"));
        assert!(rendered[0].contains("+1"));
        assert!(rendered[0].contains("-1"));
        assert!(rendered.iter().all(|line| !line.contains("M src/main.rs")));
    }

    #[test]
    fn pending_patch_log_renders_real_diff_preview() {
        let args = serde_json::json!({
            "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** End Patch"
        });
        let SpecialLogEntry::Patch(entry) = build_pending_patch_log(&args, false).expect("patch")
        else {
            panic!("expected patch log");
        };
        let rendered = entry
            .render_lines_for_width(80, false)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered
            .iter()
            .any(|line| line.contains("Applying patch src/main.rs")));
        // One file, one path: the header names it, so the body does not repeat it the way a
        // multi-file card has to.
        assert!(
            !rendered
                .iter()
                .any(|line| line.contains("diff src/main.rs")),
            "{rendered:?}"
        );
        assert!(rendered.iter().any(|line| line.contains("@@")));
        assert!(rendered.iter().any(|line| line.contains("- old")));
        assert!(rendered.iter().any(|line| line.contains("+ new")));
    }

    #[test]
    fn hunk_ranges_put_line_numbers_in_the_gutter() {
        let args = serde_json::json!({
            "input": "*** Begin Patch\n*** Update File: src/lib.rs\n@@ -10,3 +12,3 @@\n-a\n+b\n ctx\n*** End Patch"
        });
        let SpecialLogEntry::Patch(entry) = build_pending_patch_log(&args, false).expect("patch")
        else {
            panic!("expected patch log");
        };
        let rows = flat_patch_lines(&entry, 80, false);
        // codex puts the number in the gutter and the sign right after it, so the
        // sign column stays aligned under the number column instead of leading it.
        assert!(
            rows.iter().any(|row| row.contains("10 - a")),
            "a removed row is numbered from the old file, before its sign: {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("12 + b")),
            "an added row is numbered from the new file, before its sign: {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("@@ -10,3")),
            "a hunk row is a marker, not a numbered line, so it takes no gutter: {rows:?}"
        );
        // The preview keeps three rows per file, so the fourth only reads on an open card.
        let opened = flat_patch_lines(&entry, 80, true);
        assert!(
            opened.iter().any(|row| row.contains("13   ctx")),
            "context carries the new position with an empty sign: {opened:?}"
        );
    }

    #[test]
    fn a_long_patch_folds_and_asks_for_enter() {
        let body = (1..=14)
            .map(|n| format!("+line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let args = serde_json::json!({
            "input": format!("*** Begin Patch\n*** Update File: src/big.rs\n@@\n{body}\n*** End Patch")
        });
        let SpecialLogEntry::Patch(entry) = build_pending_patch_log(&args, false).expect("patch")
        else {
            panic!("expected patch log");
        };
        let folded = flat_patch_lines(&entry, 80, false);
        assert!(
            folded
                .iter()
                .any(|row| row.contains("+12 lines (enter to expand)")),
            "one row carries both the count and the key: {folded:?}"
        );
        assert!(
            !folded.iter().any(|row| row.contains("\u{22ee} +")),
            "the count may not be said twice on this card: {folded:?}"
        );
        // Three rows per file is what the folded card spends, the same depth the reference
        // previews a change with.
        assert_eq!(
            folded.iter().filter(|row| row.contains("+ line ")).count(),
            2,
            "{folded:?}"
        );
        assert!(!folded.iter().any(|row| row.contains("+ line 3")));
        assert!(!folded.iter().any(|row| row.contains("line 14")));

        let opened = flat_patch_lines(&entry, 80, true);
        assert!(opened.iter().any(|row| row.contains("line 14")));
        assert!(!opened.iter().any(|row| row.contains("Show details")));
    }

    /// A folded *range* inside one file keeps its own marker. `⋮` says the window dropped
    /// part of that file's change, which must not be confused with the card's counted fold.
    #[test]
    fn a_range_that_left_the_window_keeps_its_own_marker() {
        let body = (1..=45)
            .map(|n| format!("+line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let args = serde_json::json!({
            "input": format!("*** Begin Patch\n*** Update File: src/wide.rs\n@@\n{body}\n*** End Patch")
        });
        let SpecialLogEntry::Patch(entry) = build_pending_patch_log(&args, false).expect("patch")
        else {
            panic!("expected patch log");
        };
        let rows = flat_patch_lines(&entry, 80, false);
        assert!(
            rows.iter()
                .any(|row| row.contains("lines (enter to expand)")),
            "the card counts what Enter reveals: {rows:?}"
        );
        // The window's own marker sits past the fold budget, so it belongs to the open card.
        let opened = flat_patch_lines(&entry, 80, true);
        assert!(
            opened
                .iter()
                .any(|row| row.contains("\u{22ee}") && row.contains("more lines")),
            "the range marker names the window, not the fold: {opened:?}"
        );
    }

    #[test]
    fn a_finished_change_draws_the_diff_the_engine_applied() {
        let SpecialLogEntry::Patch(entry) = build_completed_patch_log(
            &serde_json::json!({"result": {"ok": true, "data": {
                "added_lines": 2, "deleted_lines": 1, "changed_files": 1, "hunks_applied": 1,
                "files": [{"action": "update", "path": "src/main.rs", "hunks": 1,
                    "added_lines": 2, "deleted_lines": 1,
                    "diff_blocks": [{"header": "",
                        "start_line_before": 10, "end_line_before": 10,
                        "start_line_after": 12, "end_line_after": 13,
                        "lines": [
                            {"kind": "delete", "old_line": 10, "new_line": null, "text": "let b = 2;"},
                            {"kind": "add", "old_line": null, "new_line": 12, "text": "let b = 3;"},
                            {"kind": "add", "old_line": null, "new_line": 13, "text": "let c = 4;"}
                        ]}]}]
            }}}),
            false,
        ) else {
            panic!("expected a patch card");
        };
        let rows = flat_patch_lines(&entry, 80, false);
        assert!(
            rows.iter().any(|row| row.contains("Edited src/main.rs")),
            "the header names the one file: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.contains("diff src/main.rs")),
            "and the body does not say the path again: {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("-10,1 +12,2")),
            "the hunk row is built from the engine's own range: {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("let b = 3;")),
            "{rows:?}"
        );
        let opened = flat_patch_lines(&entry, 80, true);
        assert!(
            opened.iter().any(|row| row.contains("let c = 4;")),
            "the last row of the range is what Enter is for: {opened:?}"
        );
    }

    /// A write is not a patch the model wrote, so nothing parseable ever arrived: the
    /// applied blocks are the only way this card can exist at all.
    #[test]
    fn a_write_result_renders_as_a_diff_card() {
        let SpecialLogEntry::Patch(entry) = build_completed_patch_log(
            &serde_json::json!({"result": {"ok": true, "data": {
                "added_lines": 2, "deleted_lines": 0, "changed_files": 1,
                "files": [{"action": "add", "path": "notes/new.md",
                    "added_lines": 2, "deleted_lines": 0,
                    "diff_blocks": [{"header": "",
                        "start_line_before": 0, "end_line_before": 0,
                        "start_line_after": 1, "end_line_after": 2,
                        "lines": [
                            {"kind": "add", "old_line": null, "new_line": 1, "text": "# 第一段"},
                            {"kind": "add", "old_line": null, "new_line": 2, "text": "line two"}
                        ]}]}]
            }}}),
            true,
        ) else {
            panic!("expected a patch card");
        };
        let rows = flat_patch_lines(&entry, 80, false);
        assert!(
            rows.iter().any(|row| row.contains("已新增 notes/new.md")),
            "{rows:?}"
        );
        assert!(rows.iter().any(|row| row.contains("# 第一段")), "{rows:?}");
        let numbered = rows
            .iter()
            .find(|row| row.contains("# 第一段"))
            .expect("added row");
        assert!(
            numbered.contains('1'),
            "the gutter carries the new file's own line numbers: {numbered}"
        );
    }

    /// A remote call's rows are the thing worth reading; hiding all of them until Enter
    /// made the card a heading with no content.
    /// Two files in one card: each keeps a three-row preview of its own, so one large file
    /// cannot spend the whole budget and starve the second file's rows.
    #[test]
    fn two_files_each_keep_their_own_preview_rows() {
        fn added(n: u32, text: &str) -> Value {
            serde_json::json!({"kind": "add", "old_line": null, "new_line": n, "text": text})
        }
        fn file(path: &str, marker: &str) -> Value {
            serde_json::json!({"action": "update", "path": path, "hunks": 1,
            "added_lines": 4, "deleted_lines": 0,
            "diff_blocks": [{"header": "",
                "start_line_before": 1, "end_line_before": 1,
                "start_line_after": 1, "end_line_after": 4,
                "lines": [
                    added(1, &format!("{marker} one")),
                    added(2, &format!("{marker} two")),
                    added(3, &format!("{marker} three")),
                    added(4, &format!("{marker} four"))
                ]}]})
        }
        let SpecialLogEntry::Patch(entry) = build_completed_patch_log(
            &serde_json::json!({"result": {"ok": true, "data": {
                "changed_files": 2, "updated": 2, "added_lines": 8, "deleted_lines": 0,
                "files": [file("src/first.rs", "first"), file("src/second.rs", "second")]
            }}}),
            false,
        ) else {
            panic!("expected a patch card");
        };

        // Each block is a range row plus four content rows, so a card of two files previews
        // three of them and holds two back per file.
        let folded = flat_patch_lines(&entry, 100, false);
        assert!(
            folded.iter().any(|row| row.contains("first two")),
            "{folded:?}"
        );
        assert!(
            folded.iter().any(|row| row.contains("second two")),
            "{folded:?}"
        );
        assert!(
            !folded.iter().any(|row| row.contains("first three")),
            "{folded:?}"
        );
        assert!(
            !folded.iter().any(|row| row.contains("second three")),
            "{folded:?}"
        );
        assert!(
            folded
                .iter()
                .any(|row| row.contains("+4 lines (enter to expand)")),
            "{folded:?}"
        );

        let opened = flat_patch_lines(&entry, 100, true);
        assert!(
            opened.iter().any(|row| row.contains("first four")),
            "{opened:?}"
        );
        assert!(
            opened.iter().any(|row| row.contains("second four")),
            "{opened:?}"
        );
        // Two files keep two path rows; only a one-file card folds its path into the header.
        assert!(
            opened.iter().any(|row| row.contains("diff src/first.rs")),
            "{opened:?}"
        );
    }

    #[test]
    fn a_remote_result_previews_rows_and_holds_the_rest_for_enter() {
        fn flat(entry: &SpecialLogEntry, width: u16, expanded: bool) -> Vec<String> {
            entry
                .render_lines_for_width(width, expanded)
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect()
        }

        let content = (1..=7)
            .map(|n| serde_json::json!({ "type": "text", "text": format!("row {n}") }))
            .collect::<Vec<_>>();
        let entry = build_completed_tool_log(
            "crm@lookup",
            &serde_json::json!({ "ok": true, "args": {"customer": "42"}, "data": {
                "server": "crm", "tool": "lookup",
                "result": { "content": content }
            }}),
            false,
            None,
        );

        let folded = flat(&entry, 80, false);
        assert!(
            folded.iter().any(|row| row.contains("row 1")),
            "the card previews its result: {folded:?}"
        );
        assert!(
            !folded.iter().any(|row| row.contains("row 5")),
            "and stops at the preview budget: {folded:?}"
        );
        assert!(
            folded
                .iter()
                .any(|row| row.contains("+4 lines (enter to expand)")),
            "{folded:?}"
        );

        let opened = flat(&entry, 80, true);
        assert!(opened.iter().any(|row| row.contains("row 7")), "{opened:?}");
        assert!(
            opened.iter().any(|row| row.contains("Arguments:"))
                && opened.iter().any(|row| row.contains("customer")),
            "the open card shows what was asked for: {opened:?}"
        );
    }

    fn flat_patch_lines(entry: &PatchLogEntry, width: u16, expanded: bool) -> Vec<String> {
        entry
            .render_lines_for_width(width, expanded)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn completed_patch_log_inherits_pending_diff_preview() {
        let args = serde_json::json!({
            "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** End Patch"
        });
        let SpecialLogEntry::Patch(pending) = build_pending_patch_log(&args, false).expect("patch")
        else {
            panic!("expected patch log");
        };
        let payload = serde_json::json!({
            "result": {
                "ok": true,
                "data": {
                    "changed_files": 1,
                    "updated": 1,
                    "hunks_applied": 1,
                    "files": [
                        {
                            "action": "update",
                            "path": "src/main.rs",
                            "hunks": 1
                        }
                    ]
                }
            }
        });
        let SpecialLogEntry::Patch(mut completed) = build_completed_patch_log(&payload, false)
        else {
            panic!("expected patch log");
        };
        completed.inherit_diff_from(&pending);
        let rendered = completed
            .render_lines_for_width(80, false)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered
            .iter()
            .any(|line| line.contains("Edited src/main.rs")));
        // The inherited diff belongs to the one file the header already names, so the body
        // does not spend a row repeating the path.
        assert!(
            !rendered
                .iter()
                .any(|line| line.contains("diff src/main.rs")),
            "{rendered:?}"
        );
        assert!(rendered.iter().all(|line| !line.contains("M src/main.rs")));
    }

    #[test]
    fn a_completed_card_keeps_the_counts_it_promised_while_pending() {
        let args = serde_json::json!({
            "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-a\n-b\n-c\n+n\n*** End Patch"
        });
        let SpecialLogEntry::Patch(pending) = build_pending_patch_log(&args, false).expect("patch")
        else {
            panic!("expected patch log");
        };
        let pending_header = pending.summary.clone();
        let payload = serde_json::json!({
            "result": {
                "ok": true,
                "data": {
                    "changed_files": 1,
                    "added": 0,
                    "updated": 1,
                    "deleted": 0,
                    "moved": 0,
                    "added_lines": 1,
                    "deleted_lines": 3,
                    "hunks_applied": 1,
                    "files": [
                        {
                            "action": "update",
                            "path": "src/main.rs",
                            "hunks": 1,
                            "added_lines": 1,
                            "deleted_lines": 3
                        }
                    ]
                }
            }
        });
        let SpecialLogEntry::Patch(completed) = build_completed_patch_log(&payload, false) else {
            panic!("expected patch log");
        };
        assert_eq!(completed.header, "Edited src/main.rs");
        assert_eq!(
            completed.summary.as_deref(),
            Some("(+1 -3 1 hunk)"),
            "the applied line counts must survive the fold, not collapse to file counts"
        );
        assert_eq!(
            completed.summary, pending_header,
            "the card keeps the same numbers the user saw while it was pending"
        );

        let SpecialLogEntry::Patch(zh) = build_completed_patch_log(&payload, true) else {
            panic!("expected patch log");
        };
        assert_eq!(
            zh.summary.as_deref(),
            Some("(+1 -3 1 个块)"),
            "the hunk label follows the interface language: {zh:?}"
        );
    }

    #[test]
    fn command_head_folds_a_multi_line_command_and_the_footer_states_the_outcome() {
        let entry = CommandLogEntry {
            status: CommandLogStatus::Completed,
            success: false,
            title: "Failed (exit 1)".to_string(),
            command: "echo first\necho second".to_string(),
            metrics: None,
            exit_code: Some(1),
            duration_ms: Some(28),
            sections: Vec::new(),
            is_zh: false,
        };
        let rendered = flat_command_lines(&entry, 80, false);
        assert_eq!(rendered[0], "• Failed (exit 1) echo first …");
        assert!(
            !rendered[0].contains("28ms"),
            "the elapsed time is not a header detail: {rendered:?}"
        );
        assert!(
            rendered
                .iter()
                .any(|line| line.contains("+2 lines (enter to expand)")),
            "the continuation row and the footer row are both what Enter brings back: {rendered:?}"
        );

        let opened = flat_command_lines(&entry, 80, true);
        assert!(
            opened.iter().any(|line| line.contains("  └ echo second")),
            "the open card hangs the rest of the command: {opened:?}"
        );
        assert!(
            opened.iter().any(|line| line == "✗ (1) • 28ms"),
            "the settled footer names the exit and how long it took: {opened:?}"
        );
        assert!(!opened.iter().any(|line| line.contains('│')));
    }

    /// The reference counts whole seconds with two decimals and switches to minutes past a
    /// minute; a raw millisecond figure is what it never prints.
    #[test]
    fn elapsed_time_reads_the_way_the_reference_reads_it() {
        assert_eq!(format_elapsed_ms(0), "0ms");
        assert_eq!(format_elapsed_ms(999), "999ms");
        assert_eq!(format_elapsed_ms(1_000), "1.00s");
        assert_eq!(format_elapsed_ms(1_500), "1.50s");
        assert_eq!(format_elapsed_ms(59_999), "60.00s");
        assert_eq!(format_elapsed_ms(60_000), "1m 00s");
        assert_eq!(format_elapsed_ms(125_400), "2m 05s");
    }

    #[test]
    fn command_output_folds_to_a_tail_and_the_row_counts_what_enter_reveals() {
        let section = CommandSection {
            label: "stdout".to_string(),
            lines: (1..=9).map(|n| format!("line {n}")).collect(),
            window_truncated: false,
            style: Style::default(),
        };
        let entry = CommandLogEntry {
            status: CommandLogStatus::Completed,
            success: true,
            title: "Ran".to_string(),
            command: "ls".to_string(),
            metrics: None,
            exit_code: None,
            duration_ms: None,
            sections: vec![section],
            is_zh: false,
        };
        let folded = flat_command_lines(&entry, 80, false);
        // The tail is what a running command last printed, so that is what the card keeps.
        assert!(
            folded.iter().any(|line| line.contains("line 7"))
                && folded.iter().any(|line| line.contains("line 9")),
            "the card keeps the tail: {folded:?}"
        );
        assert!(
            !folded.iter().any(|line| line.contains("line 5")),
            "the head is folded away: {folded:?}"
        );
        assert!(
            folded
                .iter()
                .any(|line| line.contains("+6 lines (enter to expand)")),
            "one row carries both the count and the key: {folded:?}"
        );
        assert!(
            !folded.iter().any(|line| line.contains('\u{2026}')),
            "nothing was lost, so no second marker may promise it: {folded:?}"
        );
        let opened = flat_command_lines(&entry, 80, true);
        assert!(opened.iter().any(|line| line.contains("line 1")));
        assert!(opened.iter().any(|line| line.contains("line 5")));
        assert!(
            opened
                .iter()
                .any(|line| line.contains("\u{2212} Show less")),
            "{opened:?}"
        );
        assert!(
            !opened.iter().any(|line| line.contains("Show details")),
            "an open card needs no second hint"
        );
    }

    #[test]
    fn a_truncated_window_says_so_instead_of_promising_the_overlay() {
        let note = window_truncated_note(false);
        assert_eq!(note, "\u{2026} \u{00b7} window truncated");
        assert!(!note.contains("ctrl+t"), "{note}");
        let zh = window_truncated_note(true);
        assert_eq!(zh, "\u{2026} \u{00b7} 窗口已截断");
        assert!(!zh.contains("ctrl+t"), "{zh}");

        // The row must actually appear on a card whose window dropped rows, and it must not
        // pretend there is anything left for Enter to reveal.
        let entry = CommandLogEntry {
            status: CommandLogStatus::Completed,
            success: true,
            title: "Ran".to_string(),
            command: "ls".to_string(),
            metrics: None,
            exit_code: None,
            duration_ms: None,
            sections: vec![CommandSection {
                label: "stdout".to_string(),
                lines: vec!["last".to_string()],
                window_truncated: true,
                style: Style::default(),
            }],
            is_zh: false,
        };
        let folded = flat_command_lines(&entry, 80, false);
        assert!(
            folded.iter().any(|line| line.contains("window truncated")),
            "{folded:?}"
        );
        assert!(
            !folded.iter().any(|line| line.contains("Show details"))
                && !folded.iter().any(|line| line.contains("lines (enter")),
            "a lost row cannot be promised back: {folded:?}"
        );
    }

    /// The disclosure row is never wrapped, so its own width rule has to survive the same
    /// narrow frames the rest of the card already survived.
    #[test]
    fn a_chinese_fold_row_keeps_its_count_and_drops_its_hint_before_it_overflows() {
        let section = CommandSection {
            label: "输出".to_string(),
            lines: (1..=9).map(|n| format!("line {n}")).collect(),
            window_truncated: false,
            style: Style::default(),
        };
        let entry = CommandLogEntry {
            status: CommandLogStatus::Completed,
            success: true,
            title: "已运行".to_string(),
            command: "ls".to_string(),
            metrics: None,
            exit_code: None,
            duration_ms: None,
            sections: vec![section],
            is_zh: true,
        };
        for narrow_width in [24u16, 40] {
            let folded = flat_command_lines(&entry, narrow_width, false);
            assert!(
                folded
                    .iter()
                    .all(|row| UnicodeWidthStr::width(row.as_str()) <= usize::from(narrow_width)),
                "overflow at {narrow_width}: {folded:?}"
            );
            assert!(
                folded.iter().any(|row| row.contains("+6 行")),
                "the count is the point, and it may not be the first thing lost: {folded:?}"
            );
        }
        // At the tightest frame the key hint bows out; the count stays.
        let tight = flat_command_lines(&entry, 24, false);
        assert!(
            !tight.iter().any(|row| row.contains("enter")),
            "the hint must not be squeezed into an overflow: {tight:?}"
        );
        // Given room it returns.
        let roomy = flat_command_lines(&entry, 40, false);
        assert!(
            roomy.iter().any(|row| row.contains("+6 行（enter 展开）")),
            "{roomy:?}"
        );
    }

    fn flat_command_lines(entry: &CommandLogEntry, width: u16, expanded: bool) -> Vec<String> {
        entry
            .render_lines_for_width(width, expanded)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn completed_generic_tool_log_marks_failure_and_error() {
        let entry = build_completed_tool_log(
            "read_file",
            &serde_json::json!({
                "result": {
                    "ok": false,
                    "error": "file not found",
                    "data": { "error_code": "ENOENT" }
                }
            }),
            false,
            None,
        );
        let SpecialLogEntry::Tool(entry) = entry else {
            panic!("expected tool log");
        };
        assert_eq!(entry.status, GenericToolLogStatus::Failure);
        assert!(entry.header_text().contains("failed"));
        assert!(entry
            .details
            .iter()
            .any(|detail| detail.text.contains("file not found")));
    }

    /// §11.1's acceptance shape: no card may push a row past the transcript width, whether
    /// it is folded or open, at any of the three terminal sizes we care about.
    #[test]
    fn cards_stay_inside_the_width_at_every_common_terminal_size() {
        let stdout = "test result: ok. 226 passed\n".repeat(40);
        let command = build_completed_command_log(
            &serde_json::json!({
                "result": { "data": { "results": [{
                    "command": "cargo test --workspace",
                    "returncode": 0,
                    "stdout": stdout,
                    "stderr": "warning: unused import\n"
                }] } }
            }),
            false,
        )
        .expect("command log");
        let patch = build_pending_patch_log(
            &serde_json::json!({
                "input": "*** Begin Patch\n*** Update File: src/lib.rs\n@@ -10,4 +12,5 @@\n let a = 1;\n-let b = 2;\n+let b = 3;\n+let c = \"\u{4e00}\u{4e2a}\u{6bd4}\u{8f83}\u{957f}\u{7684}\u{4e2d}\u{6587}\u{884c}\u{ff0c}\u{7528}\u{6765}\u{9a8c}\u{8bc1}\u{6362}\u{884c}\u{4e0d}\u{8d8a}\u{754c}\u{ff0c}\u{8fd8}\u{6709}\u{66f4}\u{957f}\u{7684}\u{5185}\u{5bb9}\u{ff1b}\";\n*** End Patch"
            }),
            false,
        )
        .expect("patch log");
        let tool = build_completed_tool_log(
            "read_file",
            &serde_json::json!({ "content": "fn main() {}\n".repeat(60) }),
            false,
            None,
        );

        for width in [40u16, 80, 120] {
            for entry in [&command, &patch, &tool] {
                for expanded in [false, true] {
                    for line in entry.render_lines_for_width(width, expanded) {
                        let row_width = line
                            .spans
                            .iter()
                            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                            .sum::<usize>();
                        assert!(
                            row_width <= usize::from(width),
                            "width {width}, expanded {expanded}: {row_width} columns in {line:?}"
                        );
                    }
                }
            }
        }
    }
}
