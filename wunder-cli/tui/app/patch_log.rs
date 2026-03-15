use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use serde_json::Value;
use textwrap::Options;
use unicode_width::UnicodeWidthStr;

use crate::patch_diff::{
    build_patch_diff_preview, PatchDiffBlock, PatchDiffBlockKind, PatchDiffLine, PatchDiffLineKind,
    PatchDiffPreview,
};
use crate::tool_display::summarize_tool_result;
use crate::tui::theme;

#[derive(Debug, Clone)]
pub(super) enum SpecialLogEntry {
    Art(ArtLogEntry),
    Patch(PatchLogEntry),
    Command(CommandLogEntry),
    Tool(GenericToolLogEntry),
}

impl SpecialLogEntry {
    pub(super) fn summary_text(&self) -> String {
        match self {
            Self::Art(entry) => entry.summary_text(),
            Self::Patch(entry) => entry.summary_text(),
            Self::Command(entry) => entry.summary_text(),
            Self::Tool(entry) => entry.summary_text(),
        }
    }

    pub(super) fn line_count(&self, width: u16) -> usize {
        let lines = self.render_lines_for_width(width);
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .line_count(width.max(1))
            .max(1)
    }

    pub(super) fn render_lines_for_width(&self, width: u16) -> Vec<Line<'static>> {
        match self {
            Self::Art(entry) => entry.render_lines_for_width(width),
            Self::Patch(entry) => entry.render_lines_for_width(width),
            Self::Command(entry) => entry.render_lines_for_width(width),
            Self::Tool(entry) => entry.render_lines_for_width(width),
        }
    }

    pub(super) fn is_pending_patch(&self) -> bool {
        matches!(self, Self::Patch(entry) if entry.status == PatchLogStatus::Pending)
    }

    pub(super) fn is_pending_command(&self) -> bool {
        matches!(self, Self::Command(entry) if entry.status == CommandLogStatus::Pending)
    }

    pub(super) fn is_pending_tool_named(&self, tool_name: &str) -> bool {
        matches!(self, Self::Tool(entry) if entry.status == GenericToolLogStatus::Pending && entry.tool_name.eq_ignore_ascii_case(tool_name))
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
}

impl GenericToolLogEntry {
    fn summary_text(&self) -> String {
        let header = self.header_text();
        if let Some(summary) = self.summary.as_ref().filter(|value| !value.is_empty()) {
            format!("{header} {summary}")
        } else {
            header
        }
    }

    fn render_lines_for_width(&self, width: u16) -> Vec<Line<'static>> {
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
        let mut remaining_items = self.details.len();
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
        for detail in &self.details {
            let is_last = remaining_items == 1;
            lines.extend(detail.render_wrapped(width, is_last));
            remaining_items = remaining_items.saturating_sub(1);
        }
        lines
    }

    fn header_text(&self) -> String {
        let tool_is_zh = looks_like_zh_text(self.tool_name.as_str());
        match self.status {
            GenericToolLogStatus::Pending => {
                if tool_is_zh {
                    format!("调用 {}", self.tool_name)
                } else {
                    format!("Calling {}", self.tool_name)
                }
            }
            GenericToolLogStatus::Success => {
                if tool_is_zh {
                    format!("已完成 {}", self.tool_name)
                } else {
                    format!("Called {}", self.tool_name)
                }
            }
            GenericToolLogStatus::Failure => {
                if tool_is_zh {
                    format!("{} 失败", self.tool_name)
                } else {
                    format!("{} failed", self.tool_name)
                }
            }
        }
    }

    fn header_line(&self) -> Line<'static> {
        let (icon, icon_style, text_style) = self.header_parts();
        Line::from(vec![
            Span::styled(format!("{icon} "), icon_style),
            Span::styled(self.header_text(), text_style),
        ])
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
        Some(Line::from(vec![
            Span::styled(format!("{icon} "), icon_style),
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
    sections: Vec<CommandSection>,
}

impl CommandLogEntry {
    fn summary_text(&self) -> String {
        format!("{} {}", self.title, self.command)
    }

    fn render_lines_for_width(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![self.header_line()];
        let command_lines = self
            .command
            .lines()
            .skip(1)
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        for command_line in command_lines {
            lines.extend(render_wrapped_pipe_text(
                command_line.as_str(),
                command_style(),
                width,
            ));
        }

        if let Some(metrics) = self.metrics.as_ref().filter(|value| !value.is_empty()) {
            lines.extend(render_wrapped_pipe_text(
                metrics.as_str(),
                summary_style(),
                width,
            ));
        }

        for section in &self.sections {
            lines.extend(section.render_wrapped(width, false));
        }

        lines
    }

    fn header_line(&self) -> Line<'static> {
        let icon_style = match (self.status, self.success) {
            (CommandLogStatus::Pending, _) => Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            (CommandLogStatus::Completed, true) => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            (CommandLogStatus::Completed, false) => {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            }
        };
        let text_style = match (self.status, self.success) {
            (CommandLogStatus::Pending, _) => theme::accent_text(),
            (CommandLogStatus::Completed, true) => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            (CommandLogStatus::Completed, false) => theme::danger_text(),
        };
        let command = self.command.lines().next().unwrap_or_default().to_string();
        Line::from(vec![
            Span::styled("• ".to_string(), icon_style),
            Span::styled(self.title.clone(), text_style),
            Span::raw(" "),
            Span::styled(command, command_style()),
        ])
    }
}

#[derive(Debug, Clone)]
struct CommandSection {
    label: String,
    lines: Vec<String>,
    style: Style,
}

impl CommandSection {
    fn render_wrapped(&self, width: u16, first_body: bool) -> Vec<Line<'static>> {
        let mut rendered = Vec::new();
        let mut iter = self.lines.iter();
        if let Some(first) = iter.next() {
            rendered.extend(render_wrapped_labeled_tree_text(
                format!("{}{}: ", tree_prefix(first_body), self.label),
                first.as_str(),
                self.style,
                width,
            ));
        }
        for line in iter {
            rendered.extend(render_wrapped_continuation_text(
                line.as_str(),
                self.style,
                width,
            ));
        }
        rendered
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
}

impl PatchLogEntry {
    fn summary_text(&self) -> String {
        if let Some(summary) = self.summary.as_ref().filter(|value| !value.is_empty()) {
            format!("{} {}", self.header, summary)
        } else {
            self.header.clone()
        }
    }

    fn render_lines_for_width(&self, width: u16) -> Vec<Line<'static>> {
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

        if has_diff_preview {
            if let Some(diff_preview) = self.diff_preview.as_ref() {
                lines.extend(render_patch_diff_preview(diff_preview, width));
            }
        } else {
            for file in &self.files {
                let is_last = remaining_items == 1;
                lines.extend(file.render_wrapped(width, is_last));
                remaining_items = remaining_items.saturating_sub(1);
            }
        }

        lines
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
        Line::from(vec![
            Span::styled(format!("{icon} "), icon_style),
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
        Some(Line::from(vec![
            Span::styled(format!("{icon} "), icon_style),
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

fn render_patch_diff_preview(preview: &PatchDiffPreview, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let total_items = preview.item_count();
    let mut rendered_items = 0usize;

    for block in &preview.blocks {
        rendered_items = rendered_items.saturating_add(1);
        let is_last = rendered_items == total_items;
        lines.extend(render_patch_diff_block(block, width, is_last));
    }

    if let Some(text) = preview.omitted_text.as_ref() {
        rendered_items = rendered_items.saturating_add(1);
        let is_last = rendered_items == total_items;
        lines.extend(render_wrapped_tree_text(
            text.as_str(),
            summary_style(),
            is_last,
            width,
        ));
    }

    lines
}

fn render_patch_diff_block(
    block: &PatchDiffBlock,
    width: u16,
    is_last: bool,
) -> Vec<Line<'static>> {
    let mut lines = render_wrapped_prefixed_text(
        tree_item_prefix(is_last).to_string(),
        tree_item_continuation_prefix(is_last).to_string(),
        theme::secondary_text(),
        block.header.as_str(),
        patch_diff_header_style(block.kind),
        width,
    );
    let nested_prefix = tree_item_continuation_prefix(is_last);
    for line in &block.lines {
        lines.extend(render_patch_diff_line(line, nested_prefix, width));
    }
    lines
}

fn render_patch_diff_line(
    line: &PatchDiffLine,
    nested_prefix: &str,
    width: u16,
) -> Vec<Line<'static>> {
    let (marker, prefix_style, marker_style, text_style) = patch_diff_line_display(line.kind);
    let marker_padding = " ".repeat(UnicodeWidthStr::width(marker).saturating_add(1));
    let first_prefix = format!("{nested_prefix}{marker} ");
    let continuation_prefix = format!("{nested_prefix}{marker_padding}");
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
            "\u{2026}",
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

fn pipe_prefix() -> &'static str {
    "  │ "
}

fn continuation_prefix() -> &'static str {
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

fn render_wrapped_pipe_text(text: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    render_wrapped_prefixed_text(
        pipe_prefix().to_string(),
        pipe_prefix().to_string(),
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

fn render_wrapped_continuation_text(text: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    render_wrapped_prefixed_text(
        continuation_prefix().to_string(),
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
        success: true,
        title: if is_zh {
            "正在运行".to_string()
        } else {
            "Running".to_string()
        },
        command,
        metrics: None,
        sections: Vec::new(),
    }))
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

    let mut metrics = vec![format!("exit={returncode}")];
    if let Some(duration_ms) = duration_ms.filter(|value| *value > 0) {
        metrics.push(format!("{duration_ms}ms"));
    }

    let stdout = first
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let stderr = first
        .get("stderr")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let mut sections = Vec::new();
    if success {
        if let Some(section) = build_output_section("stdout", stdout, 6, 900, summary_style()) {
            sections.push(section);
        }
        if let Some(section) =
            build_output_section("stderr", stderr, 4, 300, Style::default().fg(Color::Yellow))
        {
            sections.push(section);
        }
    } else {
        if let Some(section) =
            build_output_section("stderr", stderr, 6, 900, Style::default().fg(Color::Red))
        {
            sections.push(section);
        }
        if let Some(section) = build_output_section("stdout", stdout, 4, 300, summary_style()) {
            sections.push(section);
        }
    }
    if sections.is_empty() {
        sections.push(CommandSection {
            label: if is_zh {
                "输出".to_string()
            } else {
                "output".to_string()
            },
            lines: vec![if is_zh {
                "<空>".to_string()
            } else {
                "<empty>".to_string()
            }],
            style: theme::secondary_text(),
        });
    }

    Some(SpecialLogEntry::Command(CommandLogEntry {
        status: CommandLogStatus::Completed,
        success,
        title: if is_zh {
            "已运行".to_string()
        } else {
            "Ran".to_string()
        },
        command,
        metrics: Some(metrics.join(", ")),
        sections,
    }))
}

pub(super) fn build_pending_tool_log(tool_name: &str, args: &Value) -> SpecialLogEntry {
    SpecialLogEntry::Tool(GenericToolLogEntry {
        tool_name: tool_name.to_string(),
        status: GenericToolLogStatus::Pending,
        summary: Some(summarize_tool_args(args)),
        details: Vec::new(),
    })
}

pub(super) fn build_completed_tool_log(tool_name: &str, payload: &Value) -> SpecialLogEntry {
    let result = payload.get("result").unwrap_or(payload);
    let data = result.get("data").unwrap_or(result);
    let ok = result.get("ok").and_then(Value::as_bool);
    let tool_is_zh = looks_like_zh_text(tool_name);
    let display = summarize_tool_result(tool_name, payload);
    let mut details = Vec::new();
    if let Some(error) = result
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        details.push(PatchNote::new(
            Some(if tool_is_zh {
                "错误:".to_string()
            } else {
                "Error:".to_string()
            }),
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
        Some(compact_json(data))
    } else if details.is_empty() {
        Some("{}".to_string())
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
    })
}

pub(super) fn build_pending_patch_log(args: &Value, is_zh: bool) -> Option<SpecialLogEntry> {
    let patch = extract_patch_input(args)?;
    let summary = summarize_patch_input(patch);
    let diff_preview = {
        let preview = build_patch_diff_preview(patch, 12, 8, is_zh);
        (!preview.is_empty()).then_some(preview)
    };
    let mut files = collect_patch_preview_rows(patch, 12, is_zh);
    let mut header = if is_zh {
        "\u{5e94}\u{7528}\u{8865}\u{4e01}".to_string()
    } else {
        "Applying patch".to_string()
    };
    let mut summary = (!summary.is_empty()).then_some(summary);
    if let [file] = files.as_slice() {
        if file.marker != '\u{2026}' {
            header = format!("{header} {}", file.text);
            summary = file.meta.clone().or(summary);
            files.clear();
        }
    }
    Some(SpecialLogEntry::Patch(PatchLogEntry {
        status: PatchLogStatus::Pending,
        header,
        summary,
        notes: Vec::new(),
        files,
        diff_preview,
    }))
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
        .max(0);

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

    let mut summary_parts = Vec::new();
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
        summary_parts.push(format!("{hunks} hunks"));
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

    if ok {
        if let [file] = files.as_slice() {
            if file.marker != '…' {
                header = single_file_patch_header(file, is_zh);
                summary = file.meta.clone().or(summary);
                files.clear();
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
        diff_preview: None,
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
    if let Some(object) = args.as_object() {
        for key in [
            "path",
            "file_path",
            "filePath",
            "query",
            "q",
            "url",
            "location",
            "ticker",
            "command",
            "content",
            "text",
            "prompt",
            "name",
        ] {
            if let Some(value) = object.get(key).and_then(Value::as_str) {
                let cleaned = value.trim();
                if !cleaned.is_empty() {
                    return format!("{key}={cleaned}");
                }
            }
        }
    }
    compact_json(args)
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

fn compact_json(value: &Value) -> String {
    const MAX_INLINE_JSON_CHARS: usize = 220;
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    if text.len() > MAX_INLINE_JSON_CHARS {
        text.truncate(MAX_INLINE_JSON_CHARS);
        text.push_str("...");
    }
    text
}

fn looks_like_zh_text(text: &str) -> bool {
    text.chars().any(|ch| {
        ('\u{4e00}'..='\u{9fff}').contains(&ch)
            || ('\u{3400}'..='\u{4dbf}').contains(&ch)
            || ('\u{20000}'..='\u{2a6df}').contains(&ch)
    })
}

fn build_output_section(
    label: &str,
    text: &str,
    max_lines: usize,
    max_chars: usize,
    style: Style,
) -> Option<CommandSection> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim_end_matches('\n').trim();
    if trimmed.is_empty() {
        return None;
    }

    let (preview, chars_truncated) = truncate_by_chars(trimmed, max_chars);
    let all_lines = preview.lines().map(ToString::to_string).collect::<Vec<_>>();
    if all_lines.is_empty() {
        return None;
    }
    let lines = collapse_middle_lines(all_lines, max_lines, chars_truncated);

    Some(CommandSection {
        label: label.to_string(),
        lines,
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

fn collapse_middle_lines(
    lines: Vec<String>,
    max_lines: usize,
    chars_truncated: bool,
) -> Vec<String> {
    if max_lines == 0 {
        return vec![ellipsis_suffix(lines.len(), chars_truncated)];
    }
    if lines.len() <= max_lines && !chars_truncated {
        return lines;
    }

    if max_lines == 1 {
        return vec![ellipsis_suffix(lines.len(), chars_truncated)];
    }

    let keep_head = max_lines / 2;
    let keep_tail = max_lines.saturating_sub(keep_head + 1);
    let omitted = lines.len().saturating_sub(keep_head + keep_tail);
    let mut collapsed = Vec::new();
    collapsed.extend(lines.iter().take(keep_head).cloned());
    collapsed.push(ellipsis_suffix(omitted, chars_truncated));
    if keep_tail > 0 {
        let start = lines.len().saturating_sub(keep_tail);
        collapsed.extend(lines.into_iter().skip(start));
    }
    collapsed
}

fn ellipsis_suffix(omitted_lines: usize, chars_truncated: bool) -> String {
    let mut parts = Vec::new();
    if omitted_lines > 0 {
        parts.push(format!("+{omitted_lines} lines"));
    }
    if chars_truncated {
        parts.push("truncated".to_string());
    }
    if parts.is_empty() {
        "...".to_string()
    } else {
        format!("... {}", parts.join(", "))
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
                    existing.meta = increment_patch_delta_meta(existing.meta.take(), 1, 0);
                } else if raw_line.starts_with('-') {
                    existing.meta = increment_patch_delta_meta(existing.meta.take(), 0, 1);
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

    Some(PatchFileRow {
        marker,
        text,
        meta: format_patch_hunk_meta(hunks, is_zh),
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
) -> Option<String> {
    let (added, removed, hunks) = parse_patch_delta_meta(meta);
    format_patch_delta_meta(added + added_delta, removed + removed_delta, hunks)
}

fn increment_patch_hunk_meta(meta: Option<String>, is_zh: bool) -> Option<String> {
    let (added, removed, hunks) = parse_patch_delta_meta(meta);
    if added > 0 || removed > 0 {
        format_patch_delta_meta(added, removed, hunks + 1)
    } else {
        format_patch_hunk_meta(hunks + 1, is_zh)
    }
}

fn format_patch_delta_meta(added: usize, removed: usize, hunks: usize) -> Option<String> {
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
        parts.push(if hunks == 1 {
            "1 hunk".to_string()
        } else {
            format!("{hunks} hunks")
        });
    }
    Some(format!("({})", parts.join(" ")))
}

fn format_patch_hunk_meta(hunks: usize, is_zh: bool) -> Option<String> {
    if hunks == 0 {
        None
    } else if is_zh {
        Some(format!("({hunks} 个块)"))
    } else if hunks == 1 {
        Some("(1 hunk)".to_string())
    } else {
        Some(format!("({hunks} hunks)"))
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
            .render_lines_for_width(80)
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
        assert_eq!(entry.title, "Ran");
        assert_eq!(entry.metrics.as_deref(), Some("exit=1, 28ms"));
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
    fn completed_command_log_collapses_middle_output() {
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
        assert!(stderr_section
            .lines
            .iter()
            .any(|line| line.contains("... +3 lines")));
        assert!(stderr_section
            .lines
            .iter()
            .any(|line| line.contains("line 1")));
        assert!(stderr_section
            .lines
            .iter()
            .any(|line| line.contains("line 8")));
    }

    #[test]
    fn pending_generic_tool_log_uses_args_summary() {
        let entry =
            build_pending_tool_log("读取文件", &serde_json::json!({ "path": "src/main.rs" }));
        let SpecialLogEntry::Tool(entry) = entry else {
            panic!("expected tool log");
        };
        assert_eq!(entry.status, GenericToolLogStatus::Pending);
        assert_eq!(entry.summary.as_deref(), Some("path=src/main.rs"));
    }

    #[test]
    fn generic_tool_header_inlines_summary_when_width_allows() {
        let entry =
            build_pending_tool_log("read_file", &serde_json::json!({ "path": "src/main.rs" }));
        let SpecialLogEntry::Tool(entry) = entry else {
            panic!("expected tool log");
        };
        let rendered = entry
            .render_lines_for_width(80)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].contains("Calling read_file path=src/main.rs"));
    }

    #[test]
    fn generic_tool_summary_moves_to_body_when_width_small() {
        let entry = build_pending_tool_log(
            "read_file",
            &serde_json::json!({ "path": "src/deeply/nested/project/main.rs" }),
        );
        let SpecialLogEntry::Tool(entry) = entry else {
            panic!("expected tool log");
        };
        let rendered = entry
            .render_lines_for_width(24)
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
    fn patch_header_inlines_summary_when_width_allows() {
        let args = serde_json::json!({
            "input": "*** Update File: src/main.rs\n@@\n-old\n+new\n"
        });
        let SpecialLogEntry::Patch(entry) = build_pending_patch_log(&args, false).expect("patch")
        else {
            panic!("expected patch log");
        };
        let rendered = entry
            .render_lines_for_width(80)
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
            .render_lines_for_width(80)
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
            .any(|line| line.contains("diff src/main.rs")));
        assert!(rendered.iter().any(|line| line.contains("@@")));
        assert!(rendered.iter().any(|line| line.contains("- old")));
        assert!(rendered.iter().any(|line| line.contains("+ new")));
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
            .render_lines_for_width(80)
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
            .any(|line| line.contains("diff src/main.rs")));
        assert!(rendered.iter().all(|line| !line.contains("M src/main.rs")));
    }

    #[test]
    fn command_render_lines_use_pipe_gutter() {
        let entry = CommandLogEntry {
            status: CommandLogStatus::Pending,
            success: true,
            title: "Running".to_string(),
            command: "echo first\necho second".to_string(),
            metrics: Some("exit=0".to_string()),
            sections: Vec::new(),
        };
        let lines = entry.render_lines_for_width(80);
        let rendered = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|line| line.contains("  │ echo second")));
        assert!(rendered.iter().any(|line| line.contains("  │ exit=0")));
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
}
