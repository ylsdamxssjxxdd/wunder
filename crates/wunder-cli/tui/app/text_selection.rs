//! Pointer text selection over the transcript, in the shape codex uses: a drag picks a
//! range of text, releasing it copies that range, and the range stays visible until the
//! next click.
//!
//! A selection is booked in **stable identity**, not in screen rows: each end names the
//! card it belongs to (`cell_id`, the same identity the disclosure state keys on), which
//! logical line of that card, and the display column inside it. Every frame the hit map
//! projects those identities back onto whatever window rows the same lines occupy now, so
//! scrolling, a resize and a streaming append all keep the mark where the reader put it.
//! Only a card that has left the transcript for good — trimmed from the head, or the whole
//! transcript cleared — can drop the mark, because then there is no line left to project.
//!
//! Pointer hits still resolve through the row map the last frame built, so a click answers
//! against what the reader actually saw rather than against a fresh re-wrap that could
//! disagree with it.

use super::helpers::is_word_char;
use super::TuiApp;
use crate::tui::text_tabs;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Mutex;

/// How far a column bound may reach. A whole-line end is never a real column, so it sorts
/// outside every character of its line instead of pretending to be one.
const LINE_END: usize = usize::MAX;

/// The clipboard writer is a seam: a test watches what a release would send instead of
/// overwriting the machine's real clipboard. Outside tests it is never read.
#[cfg(test)]
pub(super) static CLIPBOARD_WRITER: Mutex<Option<fn(&str) -> anyhow::Result<()>>> =
    Mutex::new(None);

/// What a gesture selects, the way codex names it: one character, one word or one whole
/// logical line. The unit also says that a stationary release is still a pick worth
/// copying, which a plain click is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelectionUnit {
    Character,
    Word,
    Line,
}

impl SelectionUnit {
    /// codex's mapping from a click count to a unit.
    pub(super) fn from_clicks(clicks: u8) -> Self {
        match clicks {
            2 => Self::Word,
            3.. => Self::Line,
            _ => Self::Character,
        }
    }
}

/// How many visual rows one logical line takes once the paragraph has wrapped it. The
/// line is measured as projected text — tabs expanded to their eight-column stops — because
/// that is exactly what the frame renders and what the terminal then shows. The renderer
/// measures with this same call, so the row map cannot drift from the frame.
fn paragraph_visual_rows(line: &Line<'static>, width: u16) -> usize {
    Paragraph::new(text_tabs::expand_line(line))
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
        .max(1)
}

/// Where a visual row came from: the logical line under it and which row of that line
/// this is after the paragraph wrapped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RowOrigin {
    pub line: usize,
    pub row_in_line: usize,
}

/// The stable address of one logical line of a card: which card, and which line of it.
/// A card's line order is fixed once the card is drawn, so this pair names the same text
/// across scrolls, resizes and appends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct LineIdentity {
    pub cell_id: u64,
    pub line_within_cell: usize,
}

/// Everything the last frame knows about the addressable part of the transcript.
#[derive(Default)]
pub(super) struct TranscriptHitMap {
    /// Entry owning each addressable visual row, `None` for a row no card owns.
    pub rows: Vec<Option<usize>>,
    /// Logical line and row-of-line behind each addressable visual row, same length.
    pub origins: Vec<RowOrigin>,
    /// Visual rows taken by each logical line of the window.
    pub line_rows: Vec<usize>,
    /// Stable identity of each window line, `None` for a line no card owns (the empty
    /// transcript's placeholder).
    pub line_identities: Vec<Option<LineIdentity>>,
    /// Projected display width of each window line. A pointer right of the text answers
    /// at the end of that line, not at the end of the frame.
    pub line_widths: Vec<u16>,
    /// Where each stable line is shown this frame: the window line it was painted as.
    /// A stable address absent from this map is not on screen right now.
    pub line_slots: BTreeMap<LineIdentity, usize>,
    /// Visual rows the viewport has scrolled away from the top of the window.
    pub scroll: usize,
}

/// Row map for one rendered window: which card owns each visual row, which logical line
/// (and which row of it) produced it, and where each of the window's stable lines sits.
/// Only rows the viewport can address are kept on the visual axis.
pub(super) fn build_transcript_hit_map(
    lines: &[Line<'static>],
    window: &super::TranscriptRenderWindow,
    logs: &[super::LogEntry],
    limit: usize,
    scroll: usize,
    width: u16,
) -> TranscriptHitMap {
    // The window's lines are the cards' logical lines in card order: `window_line_counts`
    // says how many lines each entry rendered, so walking that budget turns a line index
    // back into the card that owns it. `entry_visual_line_count` and the renderer answer
    // with the same number, which is what keeps this walk honest.
    let mut line_identities: Vec<Option<LineIdentity>> = vec![None; lines.len()];
    let mut line_entries: Vec<Option<usize>> = vec![None; lines.len()];
    let mut line_rows: Vec<usize> = Vec::with_capacity(lines.len());
    let mut line_widths: Vec<u16> = Vec::with_capacity(lines.len());
    let mut line_slots: BTreeMap<LineIdentity, usize> = BTreeMap::new();
    let mut entry_cursor = 0usize;
    let mut line_within_cell = 0usize;
    for (line, source) in lines.iter().enumerate() {
        while entry_cursor < window.window_line_counts.len()
            && line_within_cell >= window.window_line_counts[entry_cursor]
        {
            entry_cursor += 1;
            line_within_cell = 0;
        }
        let global_index = window
            .entries
            .get(entry_cursor)
            .map(|entry| entry.global_index);
        let cell_id = global_index.and_then(|index| logs.get(index).map(|entry| entry.cell_id));
        if let (Some(cell_id), Some(index)) = (cell_id, global_index) {
            let identity = LineIdentity {
                cell_id,
                line_within_cell,
            };
            // Two cards never share an id, so the first line to claim a slot keeps it.
            line_slots.entry(identity).or_insert(line);
            line_identities[line] = Some(identity);
            line_entries[line] = Some(index);
            line_within_cell += 1;
        }
        line_rows.push(paragraph_visual_rows(source, width));
        line_widths.push(u16::try_from(text_tabs::line_display_width(source)).unwrap_or(u16::MAX));
    }

    // Visual axis: a wrapped line contributes one entry per row it paints, and the card
    // behind those rows is the card that owns the line.
    let mut rows: Vec<Option<usize>> = Vec::with_capacity(limit);
    let mut origins: Vec<RowOrigin> = Vec::with_capacity(limit);
    for (line, count) in line_rows.iter().enumerate() {
        for row_in_line in 0..*count {
            if origins.len() >= limit {
                break;
            }
            origins.push(RowOrigin { line, row_in_line });
            rows.push(line_entries[line]);
        }
        if origins.len() >= limit {
            break;
        }
    }

    TranscriptHitMap {
        rows,
        origins,
        line_rows,
        line_identities,
        line_widths,
        line_slots,
        scroll,
    }
}

/// One end of a selection, in stable identity space. `column` is `None` when the whole
/// line is meant: a line the paragraph wrapped into several rows cannot be addressed by
/// column, and a line pick wants all of it anyway.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelectionPoint {
    pub cell_id: u64,
    pub line_within_cell: usize,
    pub column: Option<usize>,
}

impl SelectionPoint {
    /// Document order: card, then line inside it, then the column bounds. Cell ids are
    /// handed out as cards are appended, so ordering by id is ordering by position.
    fn order_key(&self) -> (u64, usize, usize, usize) {
        (
            self.cell_id,
            self.line_within_cell,
            self.column.unwrap_or(0),
            self.column.unwrap_or(LINE_END),
        )
    }

    fn identity(&self) -> LineIdentity {
        LineIdentity {
            cell_id: self.cell_id,
            line_within_cell: self.line_within_cell,
        }
    }
}

/// Where the pointer landed on the current window: a document row of that window and a
/// display column inside it. This is only the raw hit; a gesture turns it into a
/// `SelectionPoint` as soon as the frame's row map can name the card behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WindowPoint {
    pub row: usize,
    pub column: usize,
}

/// An open or dragging selection. `dragging` only matters for release, so a stationary
/// release can be read as a click instead of a copy, and `unit` says what the gesture
/// picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TranscriptSelection {
    pub anchor: SelectionPoint,
    pub head: SelectionPoint,
    pub dragging: bool,
    pub unit: SelectionUnit,
}

/// A resolved selection: a run of window lines with optional column bounds at each end.
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

    /// The stable address of the text a pointer row names. Display columns are what a
    /// pointer reports; a wrapped line splits them across its rows, so only a line that
    /// fits on one row can be addressed by column.
    pub(super) fn stable_anchor(&self, point: WindowPoint) -> Option<SelectionPoint> {
        let origin = *self.transcript_hit.origins.get(point.row)?;
        let identity = *self.transcript_hit.line_identities.get(origin.line)?;
        let identity = identity?;
        let rows_in_line = *self.transcript_hit.line_rows.get(origin.line)?;
        let column = if rows_in_line == 1 && origin.row_in_line == 0 {
            // Clamped to the line the reader clicked, so a pointer right of the text
            // answers at its end instead of at the far edge of the frame.
            let line_end = usize::from(
                *self
                    .transcript_hit
                    .line_widths
                    .get(origin.line)
                    .unwrap_or(&0),
            );
            Some(point.column.min(line_end))
        } else {
            None
        };
        Some(SelectionPoint {
            cell_id: identity.cell_id,
            line_within_cell: identity.line_within_cell,
            column,
        })
    }

    /// Where a stable address is painted this frame: the window line that shows it.
    fn project_point(&self, point: SelectionPoint) -> Option<(usize, Option<usize>)> {
        let line = *self.transcript_hit.line_slots.get(&point.identity())?;
        Some((line, point.column))
    }

    /// The two ends of the text a pointer names at one gesture's unit, low end first.
    /// `None` when the pointer is not on addressable text, or when a word pick lands
    /// where there is no word. Only a word pick needs the line's own text, so only that
    /// one reaches for the window's lines.
    fn pick(
        &self,
        point: WindowPoint,
        unit: SelectionUnit,
    ) -> Option<(SelectionPoint, SelectionPoint)> {
        let base = self.stable_anchor(point)?;
        match unit {
            SelectionUnit::Character => Some((base, base)),
            SelectionUnit::Line => {
                let whole = SelectionPoint {
                    column: None,
                    ..base
                };
                Some((whole, whole))
            }
            SelectionUnit::Word => {
                let lines = self.transcript_window_lines();
                let (from, to) = self.word_bounds_at(lines.as_slice(), point)?;
                Some((
                    SelectionPoint {
                        column: Some(from),
                        ..base
                    },
                    SelectionPoint {
                        column: Some(to),
                        ..base
                    },
                ))
            }
        }
    }

    /// A press takes a fresh anchor at the pointer, cut to the gesture's unit: one
    /// character, the word, or the whole logical line.
    pub(super) fn begin_selection_at(&mut self, point: WindowPoint, clicks: u8) -> bool {
        let unit = SelectionUnit::from_clicks(clicks);
        let Some((anchor, head)) = self.pick(point, unit) else {
            return false;
        };
        self.transcript_selection = Some(TranscriptSelection {
            anchor,
            head,
            dragging: true,
            unit,
        });
        true
    }

    /// Move the free end of an open range to the pointer, leaving the anchor where the
    /// reader put it. A drag and a shift+click are the same gesture to the bookkeeping;
    /// the moving end takes the bound that keeps the range whole, so shifting backwards
    /// lands on the near side of a word.
    pub(super) fn extend_selection_at(&mut self, point: WindowPoint) -> bool {
        let Some(selection) = self.transcript_selection else {
            return false;
        };
        let unit = selection.unit;
        let anchor = selection.anchor;
        let Some((low, high)) = self.pick(point, unit) else {
            return false;
        };
        let head = if low.order_key() < anchor.order_key() {
            low
        } else {
            high
        };
        self.transcript_selection = Some(TranscriptSelection {
            anchor,
            head,
            dragging: true,
            unit,
        });
        true
    }

    /// codex's `has_selection_range`: a pointer-down anchor alone is not selected text,
    /// so a shift+click without one starts a selection instead of extending nothing.
    pub(super) fn has_selection_range(&self) -> bool {
        self.transcript_selection
            .is_some_and(|selection| selection.anchor != selection.head)
    }

    /// A mark survives as long as both of its ends still name a card in the transcript.
    /// Trimming the oldest entries, or clearing it, is the only thing that can retire one;
    /// scrolling and resizing cannot, so they must not drop the mark.
    pub(super) fn transcript_selection_is_alive(&self) -> bool {
        let Some(selection) = self.transcript_selection else {
            return false;
        };
        [selection.anchor.cell_id, selection.head.cell_id]
            .iter()
            .all(|cell_id| self.logs.iter().any(|entry| entry.cell_id == *cell_id))
    }

    /// Drop a mark whose card has just left the transcript. Identity makes this exact: a
    /// surviving end keeps its text, and nothing is ever re-pointed at a neighbour.
    pub(super) fn forget_transcript_selection_of(&mut self, cell_id: u64) {
        if self
            .transcript_selection
            .is_some_and(|selection| selection.anchor.cell_id == cell_id)
            || self
                .transcript_selection
                .is_some_and(|selection| selection.head.cell_id == cell_id)
        {
            self.transcript_selection = None;
        }
    }

    fn resolve_selection(&self) -> Option<SelectionCells> {
        let selection = self.transcript_selection?;
        let (first, second) = if selection.anchor.order_key() <= selection.head.order_key() {
            (selection.anchor, selection.head)
        } else {
            (selection.head, selection.anchor)
        };
        let (start_line, start_column) = self.project_point(first)?;
        let (end_line, end_column) = self.project_point(second)?;
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
    /// rows the frame shows, never over the whole transcript. The lines handed in must be
    /// projected the way the frame renders them (`text_tabs::expand_line`): a selection
    /// lives in display columns, so its bounds only line up against tab-expanded text.
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
                range.end_column.unwrap_or(LINE_END)
            } else {
                LINE_END
            };
            if to <= from {
                continue;
            }
            *line = slice_line_by_display_columns(line.clone(), from, to, style);
        }
    }

    /// Whether the live selection reaches into a display-column run of one window line — the
    /// same projection, the same line numbering and the same half-open bounds
    /// `paint_transcript_selection` paints with. A transcript search asks this before it marks
    /// anything: inside the text the reader picked up, the selection's own mark is the only
    /// mark, because that mark is the text the clipboard will carry. `false` when there is no
    /// range the frame can project, which is also when the selection paints nothing.
    pub(crate) fn transcript_selection_covers(&self, line: usize, from: usize, to: usize) -> bool {
        let Some(range) = self.resolve_selection() else {
            return false;
        };
        if line < range.start_line || line > range.end_line {
            return false;
        }
        let low = if line == range.start_line {
            range.start_column.unwrap_or(0)
        } else {
            0
        };
        let high = if line == range.end_line {
            range.end_column.unwrap_or(LINE_END)
        } else {
            LINE_END
        };
        // Touching an edge is not being inside it: a run that starts where the selection ends
        // is the search's own to mark.
        from < high && to > low
    }

    /// Plain text of the selected cells, one line per logical line, trailing padding
    /// trimmed the way a terminal selection would not copy it.
    ///
    /// The lines here are the cache's logical text (tabs intact) while the bounds are
    /// display columns, so each bound maps back through the same tab projection the frame
    /// used and the copy keeps its tabs. A character is included when the display column
    /// it starts at falls inside the range; a line the paragraph wrapped still copies
    /// whole, because its rows cannot be addressed by column at all.
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
                range.end_column.unwrap_or(LINE_END)
            } else {
                LINE_END
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
        point: WindowPoint,
    ) -> Option<String> {
        let origin = *self.transcript_hit.origins.get(point.row)?;
        if self.transcript_hit.line_rows.get(origin.line) != Some(&1) {
            return None;
        }
        let text = line_text(lines.get(origin.line)?);
        url_containing_column(text.as_str(), point.column)
    }

    pub(super) fn word_bounds_at(
        &self,
        lines: &[Line<'static>],
        point: WindowPoint,
    ) -> Option<(usize, usize)> {
        let origin = *self.transcript_hit.origins.get(point.row)?;
        if self.transcript_hit.line_rows.get(origin.line) != Some(&1) {
            return None;
        }
        let text = line_text(lines.get(origin.line)?);
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return None;
        }
        // The pointer column resolves through the shared projection: a column inside a
        // tab's padding or a wide character answers with that character, not its neighbour.
        let mut index = text_tabs::logical_index_at(&text, point.column);
        if index >= chars.len() {
            index = chars.len() - 1;
        }
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
        Some((
            text_tabs::display_column_at(&text, from),
            text_tabs::display_column_at(&text, to),
        ))
    }
}

/// The plain text of a line, spans and indents concatenated.
fn line_text(line: &Line<'_>) -> String {
    let mut text = String::new();
    for span in &line.spans {
        text.push_str(span.content.as_ref());
    }
    text
}

/// The character range of a line whose display columns fall inside `from..to`. A
/// character joins the range when the column it starts at does, so a pointer that landed
/// in the padding of a tab excludes that tab at the front and keeps it at the back. All
/// of the width arithmetic lives in `text_tabs`; this only asks it which character owns
/// a column.
fn char_range_for_columns(text: &str, from: usize, to: usize) -> (usize, usize) {
    let count = text.chars().count();
    let mut start = text_tabs::logical_index_at(text, from);
    if text_tabs::display_column_at(text, start) < from {
        start += 1;
    }
    let mut end = text_tabs::logical_index_at(text, to);
    if text_tabs::display_column_at(text, end) < to {
        end += 1;
    }
    let start = start.min(count);
    (start, end.clamp(start, count))
}

/// Plain text of a display-width range of a line. The line is logical text and the bounds
/// are projected display columns: the range is resolved through the shared projection, so
/// tabs travel to the clipboard as tabs.
fn slice_line_text(line: &Line<'_>, from: usize, to: usize) -> String {
    let text = line_text(line);
    let (start, end) = char_range_for_columns(&text, from, to);
    text.chars().skip(start).take(end - start).collect()
}

/// Rebuild a line as `[before][marked][after]`, keeping every span's own style and patching
/// `style` onto the middle run only. The display-column bounds become character indices first,
/// through `text_tabs`, and the split itself is plain slicing.
///
/// This is the one place a run of a rendered line takes on a style, so the pointer selection
/// and a transcript search mark are painted by the same code and can never disagree about
/// where a display column falls. Callers pass a projected line (`text_tabs::expand_line`)
/// because the bounds are display columns.
pub(super) fn slice_line_by_display_columns(
    line: Line<'static>,
    from: usize,
    to: usize,
    style: ratatui::style::Style,
) -> Line<'static> {
    let base_style = line.style;
    let alignment = line.alignment;
    let text = line_text(&line);
    let (start, end) = char_range_for_columns(&text, from, to);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut cursor = 0usize;
    for span in line.spans {
        let chars: Vec<char> = span.content.chars().collect();
        let span_start = cursor;
        cursor += chars.len();
        let prefix = start.saturating_sub(span_start).min(chars.len());
        let marked_start = start
            .max(span_start)
            .saturating_sub(span_start)
            .min(chars.len());
        let marked_end = end.min(cursor).saturating_sub(span_start).min(chars.len());
        let marked = marked_end.saturating_sub(marked_start);
        let suffix_start = marked_end;
        let push = |spans: &mut Vec<Span<'static>>, text: String, patch: bool| {
            if text.is_empty() {
                return;
            }
            spans.push(Span::styled(
                text,
                if patch {
                    span.style.patch(style)
                } else {
                    span.style
                },
            ));
        };
        push(
            &mut spans,
            chars[..prefix].iter().collect::<String>(),
            false,
        );
        push(
            &mut spans,
            chars[marked_start..marked_start + marked]
                .iter()
                .collect::<String>(),
            true,
        );
        push(
            &mut spans,
            chars[suffix_start..].iter().collect::<String>(),
            false,
        );
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

/// The http(s) or file URL covering a display column of one line. The text is logical
/// (tabs intact) and the column is projected: the walk advances over tab stops the same
/// way the frame and the terminal do, so a pointer on a link past a tab hits the link.
pub(super) fn url_containing_column(text: &str, column: usize) -> Option<String> {
    let mut run_start = 0usize;
    let mut in_run = false;
    let mut current = String::new();
    let mut current_column = 0usize;
    let mut candidates: Vec<(usize, String)> = Vec::new();
    for ch in text.chars() {
        let advance = text_tabs::char_advance(ch, current_column);
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
        current_column += advance;
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
            // A URL never holds a tab or whitespace, so its projected width is its
            // plain display width.
            let end = *start + text_tabs::projected_width(url.as_str());
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
        let marked = slice_line_by_display_columns(line, 5, 11, mark);
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

    #[test]
    fn a_whole_line_bound_copies_every_character_and_marks_all_of_it() {
        let line = Line::from("gate\tvalue");
        assert_eq!(slice_line_text(&line, 0, usize::MAX), "gate\tvalue");
        let marked = slice_line_by_display_columns(
            Line::from("gate    value"),
            0,
            usize::MAX,
            ratatui::style::Style::default(),
        );
        assert_eq!(marked.spans.len(), 1, "one run, no empty spans");
        assert_eq!(marked.spans[0].content.as_ref(), "gate    value");
    }

    #[test]
    fn a_bound_inside_a_tabs_padding_splits_at_the_character_that_owns_it() {
        // gate covers 0..4, the tab 4..8, value 8..13.
        let line = Line::from("gate\tvalue");
        assert_eq!(
            slice_line_text(&line, 6, usize::MAX),
            "value",
            "the tab starts before the range, so it stays out"
        );
        assert_eq!(
            slice_line_text(&line, 0, 6),
            "gate\t",
            "the tab starts inside the range, so its padding travels as a tab"
        );
        assert_eq!(slice_line_text(&line, 4, 8), "\t");
        assert_eq!(slice_line_text(&line, 5, 5), "");
        assert_eq!(slice_line_text(&line, 30, usize::MAX), "");
    }

    #[test]
    fn a_click_count_names_the_unit_codex_uses() {
        assert_eq!(SelectionUnit::from_clicks(0), SelectionUnit::Character);
        assert_eq!(SelectionUnit::from_clicks(1), SelectionUnit::Character);
        assert_eq!(SelectionUnit::from_clicks(2), SelectionUnit::Word);
        assert_eq!(SelectionUnit::from_clicks(3), SelectionUnit::Line);
        assert_eq!(SelectionUnit::from_clicks(9), SelectionUnit::Line);
    }

    #[test]
    fn a_whole_line_end_sorts_outside_the_characters_of_its_line() {
        let whole = SelectionPoint {
            cell_id: 3,
            line_within_cell: 1,
            column: None,
        };
        let at_zero = SelectionPoint {
            cell_id: 3,
            line_within_cell: 1,
            column: Some(0),
        };
        let next_line = SelectionPoint {
            cell_id: 3,
            line_within_cell: 2,
            column: None,
        };
        assert!(
            whole.order_key() > at_zero.order_key(),
            "one line reaches further"
        );
        assert!(
            whole.order_key() < next_line.order_key(),
            "but never past the next line of the card"
        );
        assert!(
            SelectionPoint {
                cell_id: 2,
                line_within_cell: 9,
                column: None
            }
            .order_key()
                < whole.order_key(),
            "an earlier card sorts first whatever its line count"
        );
    }
}
