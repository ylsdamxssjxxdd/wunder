//! Pointer text selection over the transcript, in the shape codex uses: a drag picks a
//! range of rendered cells, releasing it copies that range, and the range stays visible
//! until the next click.
//!
//! Coordinates are document rows of the current window, resolved through the hit map the
//! last frame built, so a click answers against what the reader actually saw rather than
//! against a fresh re-wrap that could disagree with it.

use super::helpers::is_word_char;
use super::TuiApp;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
#[cfg(test)]
use std::sync::Mutex;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

/// The clipboard writer is a seam: a test watches what a release would send instead of
/// overwriting the machine's real clipboard. Outside tests it is never read.
#[cfg(test)]
pub(super) static CLIPBOARD_WRITER: Mutex<Option<fn(&str) -> anyhow::Result<()>>> =
    Mutex::new(None);

/// How many visual rows one logical line takes once the paragraph has wrapped it. The
/// renderer measures with this same call, so the row map cannot drift from the frame.
fn paragraph_visual_rows(line: &Line<'static>, width: u16) -> usize {
    Paragraph::new(line.clone())
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
        .max(1)
}

/// Row map for one rendered window: which card owns each visual row, and which logical
/// line (and which row of it) produced it. Only rows the viewport can address are kept.
pub(super) fn build_transcript_hit_map(
    lines: &[Line<'static>],
    window: &super::TranscriptRenderWindow,
    limit: usize,
    scroll: usize,
    width: u16,
) -> TranscriptHitMap {
    let mut rows: Vec<usize> = Vec::with_capacity(limit);
    for (position, count) in window.window_line_counts.iter().enumerate() {
        let global_index = window.entries[position].global_index;
        let take = (*count).min(limit.saturating_sub(rows.len()));
        rows.extend(std::iter::repeat(global_index).take(take));
        if rows.len() >= limit {
            break;
        }
    }

    let mut line_rows: Vec<usize> = Vec::with_capacity(lines.len());
    let mut origins: Vec<RowOrigin> = Vec::with_capacity(limit);
    for (line, source) in lines.iter().enumerate() {
        let count = paragraph_visual_rows(source, width);
        line_rows.push(count);
        let take = count.min(limit.saturating_sub(origins.len()));
        origins.extend((0..take).map(|row_in_line| RowOrigin { line, row_in_line }));
        if origins.len() >= limit {
            break;
        }
    }

    TranscriptHitMap {
        rows,
        origins,
        line_rows,
        scroll,
        width,
    }
}

/// Where a visual row came from: the logical line under it and which row of that line
/// this is after the paragraph wrapped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RowOrigin {
    pub line: usize,
    pub row_in_line: usize,
}

/// Everything the last frame knows about the addressable part of the transcript.
#[derive(Default)]
pub(super) struct TranscriptHitMap {
    /// Entry owning each addressable visual row.
    pub rows: Vec<usize>,
    /// Logical line and row-of-line behind each addressable visual row, same length.
    pub origins: Vec<RowOrigin>,
    /// Visual rows taken by each logical line of the window.
    pub line_rows: Vec<usize>,
    /// Visual rows the viewport has scrolled away from the top of the window.
    pub scroll: usize,
    /// Columns the transcript was painted with.
    pub width: u16,
}

/// One end of a selection: a visual row and a display column inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelectionPoint {
    pub row: usize,
    pub column: usize,
}

/// An open or dragging selection. `dragging` only matters for release, so a stationary
/// release can be read as a click instead of a copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TranscriptSelection {
    pub anchor: SelectionPoint,
    pub head: SelectionPoint,
    pub dragging: bool,
}

/// A resolved selection: a run of logical lines with optional column bounds at each end.
/// A `None` bound means the whole line, which is what a line the paragraph wrapped into
/// several rows has to fall back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SelectionCells {
    pub start_line: usize,
    pub start_column: Option<usize>,
    pub end_line: usize,
    pub end_column: Option<usize>,
}

impl TuiApp {
    /// The document row under a pointer, if the pointer is on the transcript.
    pub(super) fn transcript_row_at(&self, area: Rect, mouse_row: u16) -> Option<usize> {
        if area.width == 0 || area.height == 0 || mouse_row < area.y {
            return None;
        }
        let area_row = usize::from(mouse_row - area.y);
        Some(self.transcript_hit.scroll + area_row)
    }

    /// Display columns are what a pointer reports; a wrapped line splits them across its
    /// rows, so only a line that fits on one row can be addressed by column.
    fn cell_at(&self, point: SelectionPoint) -> Option<(usize, Option<usize>)> {
        let origin = *self.transcript_hit.origins.get(point.row)?;
        let rows_in_line = *self.transcript_hit.line_rows.get(origin.line)?;
        let width = usize::from(self.transcript_hit.width.max(1));
        let column = if rows_in_line == 1 && origin.row_in_line == 0 {
            Some(point.column.min(width))
        } else {
            None
        };
        Some((origin.line, column))
    }

    fn resolve_selection(&self) -> Option<SelectionCells> {
        let selection = self.transcript_selection?;
        let (first, second) = if selection.anchor.row < selection.head.row
            || (selection.anchor.row == selection.head.row
                && selection.anchor.column <= selection.head.column)
        {
            (selection.anchor, selection.head)
        } else {
            (selection.head, selection.anchor)
        };
        let (start_line, start_column) = self.cell_at(first)?;
        let (end_line, end_column) = self.cell_at(second)?;
        if start_line > end_line {
            return None;
        }
        Some(SelectionCells {
            start_line,
            start_column,
            end_line,
            end_column,
        })
    }

    /// Patch the visible window so the selected cells carry a highlight. Runs over the
    /// rows the frame shows, never over the whole transcript.
    pub(crate) fn paint_transcript_selection(&self, lines: &mut [Line<'static>]) {
        let Some(range) = self.resolve_selection() else {
            return;
        };
        let style = crate::tui::theme::selection_mark();
        for (index, line) in lines.iter_mut().enumerate() {
            if index < range.start_line || index > range.end_line {
                continue;
            }
            let from = if index == range.start_line {
                range.start_column.unwrap_or(0)
            } else {
                0
            };
            let to = if index == range.end_line {
                range.end_column.unwrap_or_else(|| line_width(line))
            } else {
                line_width(line)
            };
            if to <= from {
                continue;
            }
            *line = slice_line_with_selection(line.clone(), from, to, style);
        }
    }

    /// Plain text of the selected cells, one line per logical line, trailing padding
    /// trimmed the way a terminal selection would not copy it.
    pub(super) fn transcript_selection_text(&self, lines: &[Line<'static>]) -> String {
        let Some(range) = self.resolve_selection() else {
            return String::new();
        };
        let mut out = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if index < range.start_line || index > range.end_line {
                continue;
            }
            let from = if index == range.start_line {
                range.start_column.unwrap_or(0)
            } else {
                0
            };
            let to = if index == range.end_line {
                range.end_column.unwrap_or(usize::MAX)
            } else {
                usize::MAX
            };
            out.push(slice_line_text(line, from, to).trim_end().to_string());
        }
        while out.last().is_some_and(|line| line.is_empty()) {
            out.pop();
        }
        out.join("\n")
    }

    /// The URL under a pointer column, which is what a Ctrl+click opens. A line the
    /// paragraph wrapped cannot be addressed by column, so it yields nothing.
    pub(super) fn url_at_point(
        &self,
        lines: &[Line<'static>],
        point: SelectionPoint,
    ) -> Option<String> {
        let origin = *self.transcript_hit.origins.get(point.row)?;
        if self.transcript_hit.line_rows.get(origin.line) != Some(&1) {
            return None;
        }
        let text = slice_line_text(lines.get(origin.line)?, 0, usize::MAX);
        url_containing_column(text.as_str(), point.column)
    }

    pub(super) fn word_bounds_at(
        &self,
        lines: &[Line<'static>],
        point: SelectionPoint,
    ) -> Option<(usize, usize)> {
        let origin = *self.transcript_hit.origins.get(point.row)?;
        if self.transcript_hit.line_rows.get(origin.line) != Some(&1) {
            return None;
        }
        let text = slice_line_text(lines.get(origin.line)?, 0, usize::MAX);
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return None;
        }
        let mut columns = Vec::with_capacity(chars.len() + 1);
        let mut column = 0usize;
        for ch in &chars {
            columns.push(column);
            column += ch.width().unwrap_or(0).max(1);
        }
        columns.push(column);

        let mut index = columns
            .iter()
            .position(|at| *at > point.column)
            .unwrap_or(chars.len())
            .min(chars.len() - 1);
        if !is_word_char(chars[index]) {
            // Between words: take the word to the left, the way a terminal double click does.
            if index == 0 {
                return None;
            }
            index -= 1;
            if !is_word_char(chars[index]) {
                return None;
            }
        }
        let mut from = index;
        while from > 0 && is_word_char(chars[from - 1]) {
            from -= 1;
        }
        let mut to = index + 1;
        while to < chars.len() && is_word_char(chars[to]) {
            to += 1;
        }
        Some((columns[from], columns[to]))
    }
}

fn line_width(line: &Line<'_>) -> usize {
    line.spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

/// Plain text of a display-width range of a line.
fn slice_line_text(line: &Line<'_>, from: usize, to: usize) -> String {
    let mut out = String::new();
    let mut column = 0usize;
    for span in &line.spans {
        for ch in span.content.chars() {
            let width = ch.width().unwrap_or(0).max(1);
            if column >= to {
                return out;
            }
            if column >= from {
                out.push(ch);
            }
            column += width;
        }
    }
    out
}

/// Rebuild a line as `[before][selected][after]`, keeping every span's own style and
/// patching the highlight onto the middle run only.
fn slice_line_with_selection(
    line: Line<'static>,
    from: usize,
    to: usize,
    style: ratatui::style::Style,
) -> Line<'static> {
    let base_style = line.style;
    let alignment = line.alignment;
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut column = 0usize;
    for span in line.spans {
        let chars: Vec<char> = span.content.chars().collect();
        let span_style = span.style;
        let mut index = 0usize;
        let mut local = 0usize;
        while index < chars.len() {
            let selected = column + local >= from && column + local < to;
            let mut run = String::new();
            let mut run_width = 0usize;
            while index < chars.len() {
                let at = column + local + run_width;
                if (at >= from && at < to) != selected {
                    break;
                }
                let width = chars[index].width().unwrap_or(0).max(1);
                run.push(chars[index]);
                run_width += width;
                index += 1;
            }
            if run.is_empty() {
                index += 1;
                continue;
            }
            spans.push(Span::styled(
                run,
                if selected {
                    span_style.patch(style)
                } else {
                    span_style
                },
            ));
            local += run_width;
        }
        column += local;
    }
    let mut marked = Line::from(spans).style(base_style);
    marked.alignment = alignment;
    marked
}

/// Characters a URL may hold here. Shell metacharacters, quotes, spaces and backslashes
/// are deliberately absent: the same string travels to the platform opener.
fn is_url_character(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(
            ch,
            '-' | '.'
                | '_'
                | '~'
                | ':'
                | '/'
                | '?'
                | '#'
                | '['
                | ']'
                | '@'
                | '!'
                | '$'
                | '*'
                | '+'
                | ','
                | ';'
                | '='
                | '('
                | ')'
        )
}

/// The http(s) or file URL covering a display column of one line.
pub(super) fn url_containing_column(text: &str, column: usize) -> Option<String> {
    let mut run_start = 0usize;
    let mut in_run = false;
    let mut current = String::new();
    let mut current_column = 0usize;
    let mut candidates: Vec<(usize, String)> = Vec::new();
    for ch in text.chars() {
        let width = ch.width().unwrap_or(0).max(1);
        if is_url_character(ch) {
            if !in_run {
                in_run = true;
                run_start = current_column;
                current.clear();
            }
            current.push(ch);
        } else if in_run {
            candidates.push((run_start, std::mem::take(&mut current)));
            in_run = false;
        }
        current_column += width;
    }
    if in_run {
        candidates.push((run_start, current));
    }
    candidates
        .into_iter()
        .filter(|(_, url)| {
            ["http://", "https://", "file://"]
                .iter()
                .any(|scheme| url.starts_with(scheme))
        })
        .find(|(start, url)| {
            let end = *start
                + url
                    .chars()
                    .map(|ch| ch.width().unwrap_or(0).max(1))
                    .sum::<usize>();
            *start <= column && column < end
        })
        .map(|(_, url)| url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_is_only_found_by_the_column_it_covers() {
        let text = "see https://example.com/a?b=1 and done";
        assert_eq!(
            url_containing_column(text, 8).as_deref(),
            Some("https://example.com/a?b=1")
        );
        assert_eq!(
            url_containing_column(text, 2),
            None,
            "a column before the href is not on a link"
        );
        assert_eq!(
            url_containing_column("mailto:someone@example.com", 3),
            None,
            "only http, https and file hrefs open"
        );
    }

    #[test]
    fn the_selected_run_keeps_its_own_style_and_gains_the_mark() {
        let base = ratatui::style::Style::default().fg(ratatui::style::Color::Green);
        let line = Line::from(vec![
            Span::raw("head "),
            Span::styled("target", base),
            Span::raw(" tail"),
        ]);
        let mark = ratatui::style::Style::default().bg(ratatui::style::Color::Red);
        let marked = slice_line_with_selection(line, 5, 11, mark);
        let text = marked
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(text, "head target tail", "no character may move");
        assert!(
            marked.spans.iter().any(|span| span.content == "target"
                && span.style.bg == Some(ratatui::style::Color::Red)
                && span.style.fg == Some(ratatui::style::Color::Green)),
            "the mark is patched over the run, its own colour survives: {:?}",
            marked.spans
        );
        assert!(
            marked
                .spans
                .iter()
                .any(|span| span.content == " tail" && span.style.bg.is_none()),
            "outside the run nothing is marked: {:?}",
            marked.spans
        );
    }

    #[test]
    fn slicing_counts_display_columns_not_bytes() {
        let line = Line::from("中文 ab");
        assert_eq!(slice_line_text(&line, 0, 4), "中文");
        assert_eq!(slice_line_text(&line, 4, usize::MAX), " ab");
    }
}
