//! Pointer selection and manual scrolling over the composer draft, in the shape codex's
//! textarea uses: a press drops an anchor, a drag pushes the caret, and the marked run is
//! what the next edit acts on.
//!
//! The mark is booked in **byte offsets of the draft**, not in screen rows. A row index
//! would be re-pointed by every keystroke and every re-wrap; a byte offset names the same
//! text through a scroll, and the composer's own wrap (`build_wrapped_input_lines`) is the
//! only thing that turns a pointer into one. Because the offsets are byte-based, a resize
//! cannot orphan a mark either — the same bytes are simply painted on different rows now.
//!
//! Two ends, one free: the origin stays where the reader pressed and the caret is the end
//! the drag moves, so the marked run is `origin.start.min(caret)..origin.end.max(caret)`.
//! That is what lets a backwards drag still hand the whole run to an edit, and it is why
//! the caret sits at the far end of a word or line pick: typing after a double click keeps
//! going from the end of the word it replaced.
//!
//! Scrolling follows the same split. `input_scroll` is a manual top row, `None` while the
//! band auto-follows the caret. `input_view` honours the offset *before* it records the hit
//! map, so a drag that nudged the draft one row up resolves against the row it just asked
//! for instead of the one the last frame painted — the coupling that makes an edge drag
//! either stick or jump two rows.

use super::helpers::byte_index_for_display_column;
use super::helpers::is_word_char;
use super::helpers::next_char_boundary;
use super::helpers::prev_char_boundary;
use super::text_selection::SelectionUnit;
use super::TuiApp;
use super::WrappedInputLine;
use crate::tui::theme;
use crossterm::event::MouseEvent;
use ratatui::text::{Line, Span};
use std::ops::Range;
use std::time::Instant;

/// Rows one wheel notch moves the draft — the step the transcript band uses too, so the
/// pointer does not change dialects between the two regions.
const INPUT_SCROLL_STEP: usize = 3;

/// How close two presses have to be in time and place to count as one gesture, the same
/// window the transcript's click chain uses.
const INPUT_CLICK_WINDOW_MS: u128 = 400;

/// One composer gesture: where it was anchored and how it was cut. `dragging` says a press
/// is still held down, and `moved` that the drag actually took the caret somewhere, which
/// together tell a plain caret placement from a pick worth copying.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MouseSelection {
    pub origin: Range<usize>,
    pub unit: SelectionUnit,
    pub dragging: bool,
    pub moved: bool,
}

/// Where a pointer answers on the draft: the wrap row under it, the display column inside
/// that row, the byte offset the caret goes to, and the row's own byte span. The span is
/// what tells a pick on a row's trailing padding from one on its text.
#[derive(Clone, Copy, Debug)]
pub(super) struct InputHit {
    pub row: usize,
    pub column: usize,
    pub byte: usize,
    pub line: WrappedInputLine,
}

/// The last wrap row a band can start on: the one that still fills the viewport.
fn last_top_row(total_rows: usize, band_rows: usize) -> usize {
    total_rows.saturating_sub(band_rows.max(1))
}

impl TuiApp {
    // ---------------------------------------------------------------- band geometry

    /// Rows the draft is painted into, at least one.
    fn input_band_rows(&self) -> usize {
        usize::from(self.input_hit_area.height.max(1))
    }

    /// The wrap row the top of the band shows. The manual offset wins while the reader's
    /// wheel or a drag holds one; otherwise it is whatever the last frame settled on, which
    /// is the same number because `input_view` eats the offset before it records the map.
    pub(super) fn input_top_row(&self) -> usize {
        let requested = self.input_scroll.unwrap_or(self.input_hit_start);
        requested.min(last_top_row(
            self.input_hit_lines.len(),
            self.input_band_rows(),
        ))
    }

    /// A manual top row for the wrap the composer last drew, clamped into the band.
    fn clamped_input_top(&self, requested: usize) -> usize {
        requested.min(last_top_row(
            self.input_hit_lines.len(),
            self.input_band_rows(),
        ))
    }

    /// The wheel over the composer: the draft scrolls under the band while the caret keeps
    /// its byte, so the text the caret points at never changes. A draft that already fits
    /// has nothing to scroll and the band goes back to following the caret.
    pub(super) fn scroll_input_rows(&mut self, delta: i32) -> bool {
        if self.input_hit_area.width == 0 {
            return false;
        }
        let total = self.input_hit_lines.len();
        if total <= self.input_band_rows() {
            return self.input_scroll.take().is_some();
        }
        let top = self.input_top_row();
        let next = if delta < 0 {
            top.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            top.saturating_add(delta.max(0) as usize)
        };
        let next = self.clamped_input_top(next);
        if self.input_scroll == Some(next) {
            return false;
        }
        self.input_scroll = Some(next);
        true
    }

    /// Three rows per notch, the transcript's step.
    pub(super) fn scroll_input_at_pointer(&mut self, direction: i32) -> bool {
        self.scroll_input_rows(direction.saturating_mul(INPUT_SCROLL_STEP as i32))
    }

    /// The composer's own click chain: sharing the transcript's counter would let a double
    /// click in one region chain into the other.
    pub(super) fn reset_input_clicks(&mut self) {
        self.input_click_at = None;
        self.input_click_count = 0;
        self.input_click_row = 0;
        self.input_click_column = 0;
    }

    /// Presses within the click window and on the same wrap row and column count up;
    /// anything else restarts at one.
    pub(super) fn count_input_click(&mut self, row: usize, column: usize) -> u8 {
        let repeated = self.input_click_at.is_some_and(|at| {
            Instant::now().saturating_duration_since(at).as_millis() < INPUT_CLICK_WINDOW_MS
                && self.input_click_row == row
                && self.input_click_column == column
        });
        self.input_click_count = if repeated {
            (self.input_click_count + 1).min(3)
        } else {
            1
        };
        self.input_click_at = Some(Instant::now());
        self.input_click_row = row;
        self.input_click_column = column;
        self.input_click_count
    }

    // ---------------------------------------------------------------- hit testing

    /// Whether the pointer is on the draft: the composer band's columns, and only the rows
    /// the draft is painted on — never the footer under them, and never the counter rows
    /// above it. The prompt glyph counts as the draft's own line.
    pub(super) fn mouse_in_composer_region(&self, mouse: &MouseEvent) -> bool {
        if self.input_hit_area.width == 0 || self.input_hit_lines.is_empty() {
            return false;
        }
        if !self.mouse_in_region(mouse, self.input_mouse_region) {
            return false;
        }
        let bottom = self
            .input_hit_area
            .y
            .saturating_add(self.input_hit_area.height);
        mouse.row >= self.input_hit_area.y && mouse.row < bottom
    }

    /// The draft address under a pointer inside the painted band, `None` when the pointer
    /// is not on text the reader can edit. Columns left of the text area — the prompt glyph
    /// and the row's margin — answer with that row's start, which for the draft's first row
    /// is byte zero, exactly where a click on the prompt used to put the caret.
    pub(super) fn input_hit(&self, mouse: &MouseEvent) -> Option<InputHit> {
        let area = self.input_hit_area;
        if area.width == 0 || area.height == 0 || self.input_hit_lines.is_empty() {
            return None;
        }
        if mouse.row < area.y || mouse.row >= area.y.saturating_add(area.height) {
            return None;
        }
        if mouse.column >= area.x.saturating_add(area.width) {
            return None;
        }
        let row = self.input_top_row() + usize::from(mouse.row - area.y);
        let column = usize::from(mouse.column.saturating_sub(area.x));
        self.input_hit_at(row, column)
    }

    /// Byte offset in the draft under a wrap row and a display column inside it. The column
    /// is measured against the projected row — tabs at their eight-column stops — the same
    /// space the frame paints in and the same one the pointer reports.
    fn input_hit_at(&self, row: usize, column: usize) -> Option<InputHit> {
        let line = *self.input_hit_lines.get(row)?;
        Some(InputHit {
            row,
            column,
            byte: byte_index_for_display_column(self.input.as_str(), line.start, line.end, column),
            line,
        })
    }

    // ---------------------------------------------------------------- the gesture

    /// A press anchors the gesture at the pointer: one click places the caret, two take the
    /// word, three the whole logical line.
    pub(super) fn press_input_at(&mut self, hit: InputHit, clicks: u8) {
        // codex: a pick that lands at or past the row's last character is on padding, not
        // on text, so repeated clicks there stay an insert point instead of turning into a
        // word or a line. `line.end` is exclusive, so the padding cell *is* `line.end`.
        let clicks = if hit.byte >= hit.line.end { 1 } else { clicks };
        let (unit, origin) = match SelectionUnit::from_clicks(clicks) {
            SelectionUnit::Word => match self.input_word_bounds(hit.byte) {
                Some((start, end)) => (SelectionUnit::Word, start..end),
                None => (SelectionUnit::Character, hit.byte..hit.byte),
            },
            SelectionUnit::Line => {
                let (start, end) = self.input_logical_line(hit.byte);
                (SelectionUnit::Line, start..end)
            }
            SelectionUnit::Character => (SelectionUnit::Character, hit.byte..hit.byte),
        };
        // The caret goes to the far end of the pick and the origin stays behind; a drag from
        // here moves only the caret.
        self.input_cursor = origin.end;
        self.input_selection = Some(MouseSelection {
            origin,
            unit,
            dragging: true,
            moved: false,
        });
    }

    /// Extend the gesture to the pointer. Past either edge of the band the draft advances one
    /// row and the pick continues on it, which is how a drag over a short viewport reaches
    /// text that was never on screen when the button went down.
    pub(super) fn drag_input_to(&mut self, mouse: &MouseEvent) -> bool {
        let area = self.input_hit_area;
        if area.width == 0 || area.height == 0 || self.input_hit_lines.is_empty() {
            return false;
        }
        if mouse.column < self.input_mouse_region.x {
            return false;
        }
        let column = usize::from(mouse.column.saturating_sub(area.x));
        let top = self.input_top_row();
        let band_rows = usize::from(area.height);
        let row = if mouse.row < area.y {
            if top == 0 {
                return false;
            }
            let next = top - 1;
            self.input_scroll = Some(next);
            // The row above the band's first one *is* the new first row.
            next
        } else if mouse.row >= area.y.saturating_add(area.height) {
            if top.saturating_add(band_rows) >= self.input_hit_lines.len() {
                return false;
            }
            let next = self.clamped_input_top(top + 1);
            self.input_scroll = Some(next);
            // And the row below the band's last one becomes the new last row.
            next + band_rows - 1
        } else {
            top + usize::from(mouse.row - area.y)
        };
        let Some(hit) = self.input_hit_at(row, column) else {
            return false;
        };
        let Some(selection) = self.input_selection.as_mut() else {
            // A drag with no press under it is not a gesture: the button state the terminal
            // reported out of order, and the caret stays put.
            return false;
        };
        // The anchor stays where the reader pressed; only the caret follows the pointer, so
        // a run dragged backwards is still handed whole to the next edit.
        let moved = hit.byte != self.input_cursor;
        selection.moved |= moved;
        selection.dragging = true;
        self.input_cursor = hit.byte;
        true
    }

    /// A release ends the drag and keeps the mark: what the reader picked stays visible until
    /// an edit or a click retires it. A press that never moved is a caret placement, not a
    /// pick worth copying; a word or line is a range even when both ends name one place.
    pub(super) fn release_input_gesture(&mut self) {
        let Some(selection) = self.input_selection.as_mut() else {
            return;
        };
        if !selection.dragging {
            return;
        }
        selection.dragging = false;
        let stationary_character_click =
            selection.unit == SelectionUnit::Character && !selection.moved;
        if stationary_character_click {
            return;
        }
        self.copy_input_selection();
    }

    /// The marked bytes, ordered, clamped to the draft and snapped to character boundaries.
    /// `None` when there is nothing marked. The snap is defensive: a draft rewritten
    /// wholesale under a live gesture must never leave an offset inside a character, which
    /// would take the whole terminal down with a slicing panic.
    pub(super) fn input_selection_range(&self) -> Option<(usize, usize)> {
        let selection = self.input_selection.as_ref()?;
        let text = self.input.as_str();
        let len = text.len();
        let caret = self.input_cursor.min(len);
        let low = selection.origin.start.min(caret).min(len);
        let high = selection.origin.end.max(caret).min(len);
        let low = if text.is_char_boundary(low) {
            low
        } else {
            prev_char_boundary(text, low)
        };
        let high = if text.is_char_boundary(high) {
            high
        } else {
            next_char_boundary(text, high)
        };
        (high > low).then_some((low, high))
    }

    pub(super) fn has_input_selection(&self) -> bool {
        self.input_selection_range().is_some()
    }

    pub(super) fn clear_input_selection(&mut self) {
        self.input_selection = None;
    }

    /// The caret moved by a key: the mark collapses, the caret keeps its place, and the band
    /// follows it again. This is the whole of what an arrow key does to a selection.
    pub(super) fn note_input_caret_motion(&mut self) {
        self.input_selection = None;
        self.input_scroll = None;
    }

    /// The draft is being replaced by something that was not built from it: a submit, a
    /// session switch, a history step, an external editor, a placeholder relabel. Byte
    /// offsets a gesture picked no longer name the same text, so the mark is dropped instead
    /// of re-pointed at bytes that belong to different words now.
    pub(super) fn forget_input_gesture(&mut self) {
        self.input_selection = None;
        self.input_scroll = None;
        self.reset_input_clicks();
    }

    /// Replace the marked run with `text`, the caret ending up right after the replacement.
    /// `false` when nothing was marked, which is the caller's signal to insert normally.
    pub(super) fn replace_input_selection_with(&mut self, text: &str) -> bool {
        let Some((start, end)) = self.input_selection_range() else {
            return false;
        };
        self.input.replace_range(start..end, text);
        self.input_cursor = start + text.len();
        self.input_selection = None;
        self.input_scroll = None;
        self.clamp_input_cursor_outside_attachment_placeholder();
        true
    }

    /// Drop the marked run, the caret landing at its start: what Backspace and Delete do to a
    /// selection instead of eating one character of it.
    pub(super) fn delete_input_selection(&mut self) -> bool {
        self.cut_input_selection_into(false)
    }

    /// Drop the marked run into the kill buffer, so `Ctrl+Y` yanks back exactly what a cut
    /// removed — the same buffer `Ctrl+W` and `Ctrl+K` feed.
    pub(super) fn cut_input_selection(&mut self) -> bool {
        self.cut_input_selection_into(true)
    }

    fn cut_input_selection_into(&mut self, to_kill_buffer: bool) -> bool {
        let Some((start, end)) = self.input_selection_range() else {
            return false;
        };
        if to_kill_buffer {
            let removed = self.input[start..end].to_string();
            self.kill_buffer.push_str(removed.as_str());
        }
        self.input.replace_range(start..end, "");
        self.input_cursor = start;
        self.input_selection = None;
        self.input_scroll = None;
        true
    }

    /// Copy the marked run through the one clipboard channel the transcript uses.
    pub(super) fn copy_input_selection(&mut self) {
        let Some((start, end)) = self.input_selection_range() else {
            return;
        };
        let text = self.input[start..end].to_string();
        self.publish_clipboard_text(text.as_str());
    }

    // ---------------------------------------------------------------- painting

    /// One visible wrap row of the draft as the frame should show it: the logical text, tab
    /// included, with the marked run carrying `theme::selection_mark`. The composer still
    /// owns the last step — placeholder badges and the eight-column projection — and styles
    /// ride the spans through it, so this is the only place a mark has to be applied.
    pub(super) fn draft_row_for_view(&self, line: WrappedInputLine) -> Line<'static> {
        let text = self.input.as_str();
        let row = text[line.start..line.end].to_string();
        let Some((start, end)) = self
            .input_selection_range()
            .filter(|(start, end)| *start < line.end && *end > line.start)
        else {
            return Line::from(row);
        };
        let from = start.max(line.start);
        let to = end.min(line.end);
        let mut spans = Vec::with_capacity(3);
        if from > line.start {
            spans.push(Span::raw(text[line.start..from].to_string()));
        }
        spans.push(Span::styled(
            text[from..to].to_string(),
            theme::selection_mark(),
        ));
        if to < line.end {
            spans.push(Span::raw(text[to..line.end].to_string()));
        }
        Line::from(spans)
    }

    // ---------------------------------------------------------------- word and line

    /// The byte span of the word around a caret position: the character the pointer is on
    /// when there is one, else the character just behind it, so a click right behind a word
    /// still means that word. `None` when the pointer is not on a word.
    ///
    /// Measured in bytes against the draft, not in display columns: the transcript's
    /// `word_bounds_at` answers in the projected column space of a rendered line, and the
    /// composer's own wrap already resolves a column to a byte before this is asked.
    fn input_word_bounds(&self, byte: usize) -> Option<(usize, usize)> {
        let text = self.input.as_str();
        let len = text.len();
        if len == 0 {
            return None;
        }
        let requested = byte.min(len);
        let at = if text.is_char_boundary(requested) {
            requested
        } else {
            prev_char_boundary(text, requested)
        };
        let behind_index = if at > 0 {
            Some(prev_char_boundary(text, at))
        } else {
            None
        };
        let ahead_char = char_at(text, at);
        let behind_char = behind_index.and_then(|index| char_at(text, index));
        let first = if ahead_char.is_some_and(is_word_char) {
            at
        } else if behind_char.is_some_and(is_word_char) {
            behind_index.unwrap_or(at)
        } else {
            return None;
        };
        let mut start = first;
        while start > 0 {
            let before = prev_char_boundary(text, start);
            if !word_at(text, before, start) {
                break;
            }
            start = before;
        }
        let mut end = first;
        while end < len {
            let after = next_char_boundary(text, end);
            if !word_at(text, end, after) {
                break;
            }
            end = after;
        }
        Some((start, end))
    }

    /// The logical line a byte sits on: codex's text-space line, which runs to the hard
    /// newline and not to the end of the wrap row the reader clicked.
    fn input_logical_line(&self, byte: usize) -> (usize, usize) {
        let text = self.input.as_str();
        let at = byte.min(text.len());
        let start = text[..at]
            .rfind('\n')
            .map_or(0, |index| index + '\n'.len_utf8());
        let end = text[at..]
            .find('\n')
            .map_or(text.len(), |offset| at + offset);
        (start, end)
    }
}

/// The character at a byte offset, `None` off the end or off a boundary.
fn char_at(text: &str, byte: usize) -> Option<char> {
    text.get(byte..)?.chars().next()
}

/// Whether the one character of `text[start..end]` is part of a word.
fn word_at(text: &str, start: usize, end: usize) -> bool {
    text.get(start..end)
        .and_then(|slice| slice.chars().next())
        .is_some_and(is_word_char)
}
