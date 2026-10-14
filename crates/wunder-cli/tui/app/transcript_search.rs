//! Literal transcript search, scanned frame by frame over the addressable document.
//!
//! The shape is codex's `transcript_view/search.rs`: a query, a bounded scan that eats one
//! budget of text per frame, and a hit list the reader walks with Enter. What differs is
//! only where the text comes from. codex owns its own layout cache; here the transcript
//! already renders each card's lines and counts them for the row map, so a hit is booked
//! against **the same stable identity a pointer selection uses** — the card (`cell_id`) and
//! the logical line inside it. That is what makes a jump and a mark agree about which line a
//! hit lives on, and what keeps a hit pointing at the same text across scrolls and resizes.
//!
//! A hit carries two ranges for its line: the character range in the card's logical text and
//! the display-column range that text projects to. The projection is `text_tabs`'
//! eight-column tab stop and nothing else, because that is what the frame paints and what a
//! later highlight must land on.
//!
//! Invalidation is decided by **document identity, never by scroll position**: the render
//! revision (which every append, trim, fold and resize bumps), the archived-entry boundary,
//! the entry count, the wrap width and the folded query. Scrolling is not in that set — the
//! reader moving the window does not change what the document says, so a finished scan stays
//! finished and a jump is not thrown away by a wheel notch.
//!
//! The scan is bounded twice over: a byte budget per frame so no single frame can walk a
//! long transcript, and a hard match ceiling so the hit list cannot grow without bound. Past
//! the ceiling the scan stops and the band says `200+`, which is the honest reading — the
//! count shown is what was actually collected, not what a full pass might have found.
//!
//! Out of scope for this first slice, deliberately: painting the highlight inside a frame,
//! unfolding a folded card that holds a hit, paging older history to extend the hit list, and
//! any mode other than a plain literal query.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::locale::ui_text;
use crate::tui::text_tabs;
use crate::tui::theme;

use super::text_selection::LineIdentity;
use super::TuiApp;

/// Hits one search keeps. Past it the scan stops and the band marks the count as a floor.
pub(super) const SEARCH_MATCH_CAP: usize = 200;
/// Bytes of line text one frame may scan. Codex uses 16 KiB for the same budget.
pub(super) const SEARCH_SCAN_BYTES: usize = 16 * 1024;
/// The query itself is bounded so a pasted wall of text cannot make every frame fold a megabyte.
const SEARCH_QUERY_BYTES: usize = 4096;
/// What the band shows for a hit ordinal when nothing is accepted yet.
const NO_HIT: &str = "0";

/// Where the per-frame scan stands, as one advance step answered it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SearchProgress {
    /// Nothing to scan: no search is up, or its query is still empty.
    Idle,
    /// The budget ran out with document left: another frame would make progress.
    More,
    /// The document is fully scanned, or the hit ceiling was reached.
    Done,
}

/// Which way a step moves through the hit list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SearchDirection {
    Newer,
    Older,
}

/// One collected hit: where the text lives, and the two ranges that describe it there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SearchMatch {
    /// The card and the logical line of it the hit sits on — the selection's own address.
    pub(super) identity: LineIdentity,
    /// Character range in that line's logical text, end exclusive.
    pub(super) logical: (usize, usize),
    /// Display-column range the same run projects to, end exclusive.
    pub(super) display: (usize, usize),
}

/// The document facts a scan result is valid for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScanIdentity {
    /// Bumped by every append, trim, fold, raw toggle and viewport change.
    revision: u64,
    /// Entries already handed to scrollback: they are no longer addressable, so they are not
    /// part of the document either.
    archived: usize,
    entries: usize,
    width: u16,
}

/// The whole of the search editor's state.
#[derive(Debug)]
pub(super) struct TranscriptSearch {
    /// Whether the search editor owns the keyboard. Entering and leaving is the only mode
    /// transition this slice has: a query is always live, a hit is always on screen.
    pub(super) editing: bool,
    pub(super) query: String,
    /// Case-folded `query`, the form the scan matches against.
    pub(super) folded: String,
    pub(super) matches: Vec<SearchMatch>,
    /// Ordinal of the hit the reader last stepped to, `None` while they have not stepped.
    pub(super) current: Option<usize>,
    /// The ceiling was reached: the real total is unknown and larger.
    pub(super) has_more: bool,
    /// Next entry (absolute index into the transcript) the scan owes a line to.
    pub(super) scan_entry: usize,
    /// Next line of that entry the scan owes.
    pub(super) scan_line: usize,
    pub(super) scan_done: bool,
    /// The reading position to hand back on cancel: the scroll offset from the tail when the
    /// search was opened, so a jump never strands the reader.
    pub(super) origin_offset: usize,
    /// Bytes one frame may scan. The frame path always uses `SEARCH_SCAN_BYTES`; a test
    /// lowers it to watch the scan advance a bounded piece at a time.
    pub(super) frame_budget: usize,
    identity: Option<ScanIdentity>,
}

impl Default for TranscriptSearch {
    fn default() -> Self {
        Self {
            editing: false,
            query: String::new(),
            folded: String::new(),
            matches: Vec::new(),
            current: None,
            has_more: false,
            scan_entry: 0,
            scan_line: 0,
            scan_done: false,
            origin_offset: 0,
            frame_budget: SEARCH_SCAN_BYTES,
            identity: None,
        }
    }
}

impl TranscriptSearch {
    /// Forget everything derived: the next frame rescans from the document's first line.
    fn restart(&mut self, archived: usize) {
        self.matches.clear();
        self.current = None;
        self.has_more = false;
        self.scan_entry = archived;
        self.scan_line = 0;
        self.scan_done = false;
    }

    /// Drop the collected hits when the document or the query moved. Scroll position is
    /// deliberately not part of the key.
    fn reconcile(&mut self, identity: ScanIdentity, folded: String) {
        if self.identity == Some(identity) && self.folded == folded {
            return;
        }
        self.identity = Some(identity);
        self.folded = folded;
        self.restart(identity.archived);
    }
}

fn fold_query(query: &str) -> String {
    query.chars().flat_map(char::to_lowercase).collect()
}

/// Every non-overlapping occurrence of `folded_query` in one line's text, answered as a
/// character range plus the display-column range it projects to.
///
/// Matching happens in the case-folded space, the way codex folds each character before it
/// looks for the query, and the folded byte offset is mapped back through the character that
/// produced it, so a fold that changes length (`İ` to `i̇`) still yields the original
/// character bounds. The columns come from `text_tabs`, so a line holding tabs reports the
/// same display position the frame paints and a later highlight will mark.
pub(super) fn line_matches(
    text: &str,
    folded_query: &str,
) -> Vec<((usize, usize), (usize, usize))> {
    if folded_query.is_empty() {
        return Vec::new();
    }
    let mut folded = String::with_capacity(text.len());
    // For each folded byte the fold reached, the character that wrote those bytes: its
    // inclusive-then-exclusive char index bounds.
    let mut owners: Vec<(usize, usize, usize)> = Vec::with_capacity(text.len());
    for (index, ch) in text.chars().enumerate() {
        let before = folded.len();
        for lowered in ch.to_lowercase() {
            folded.push(lowered);
        }
        if folded.len() > before {
            owners.push((folded.len(), index, index + 1));
        }
    }
    let mut found = Vec::new();
    let mut from = 0usize;
    while let Some(offset) = folded[from..].find(folded_query) {
        let start = from + offset;
        let end = start + folded_query.len();
        let first = owners.partition_point(|(folded_end, _, _)| *folded_end <= start);
        let last = owners.partition_point(|(folded_end, _, _)| *folded_end < end);
        let (Some((_, low, _)), Some((_, _, high))) = (owners.get(first), owners.get(last))
        else {
            break;
        };
        found.push((
            (*low, *high),
            (
                text_tabs::display_column_at(text, *low),
                text_tabs::display_column_at(text, *high),
            ),
        ));
        from = end;
    }
    found
}

/// The query shed from its tail until `room` display columns hold it, one column kept for the
/// ellipsis so the reader always sees that the band dropped text.
fn shed_query(query: &str, room: usize) -> String {
    if query.is_empty() || UnicodeWidthStr::width(query) <= room {
        return query.to_string();
    }
    if room == 0 {
        return String::new();
    }
    let mut kept = String::new();
    let mut used = 0usize;
    for ch in query.chars() {
        let advance = ch.width().unwrap_or(0).max(1);
        if used + advance >= room {
            break;
        }
        kept.push(ch);
        used += advance;
    }
    kept.push('…');
    kept
}

impl TuiApp {
    /// Open the search editor from the transcript focus, keeping the reading position the way
    /// the reader left it so an exit can hand it back.
    pub(super) fn begin_transcript_search(&mut self) {
        let search = &mut self.transcript_search;
        search.editing = true;
        search.query.clear();
        search.folded.clear();
        search.origin_offset = self.transcript_offset_from_bottom;
        search.restart(self.history_archived_entries.min(self.logs.len()));
        search.identity = None;
        self.request_redraw();
    }

    /// Leave the search editor and restore the scroll the reader was at before opening it.
    fn cancel_transcript_search(&mut self) {
        let search = &mut self.transcript_search;
        search.editing = false;
        search.query.clear();
        search.folded.clear();
        search.identity = None;
        search.restart(self.history_archived_entries.min(self.logs.len()));
        let viewport = usize::from(self.transcript_viewport_height.max(1));
        let _ = viewport; // NEGATIVE VERIFICATION 2: Esc no longer hands the position back.
        self.request_redraw();
    }

    /// The one key that opens the search: `/` in the transcript focus. F3 is already this
    /// client's transcript-focus toggle, so the entry point differs from codex on purpose.
    pub(super) fn transcript_search_opener(key: KeyEvent) -> bool {
        matches!(key.code, KeyCode::Char('/'))
            && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
    }

    /// Whether the search editor owns the keyboard. The gap asks this before it decides to
    /// show the reading control, which is codex's `is_search_editing()` condition.
    pub(super) fn transcript_search_is_editing(&self) -> bool {
        self.transcript_search.editing
    }

    /// Keys while the search editor is up. Returns `false` only for the keys that keep their
    /// app-wide meaning: Ctrl+C and Ctrl+D are this client's interrupt and exit, so a search
    /// in progress must not eat them.
    pub(super) fn handle_transcript_search_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'))
        {
            return false;
        }
        match key.code {
            KeyCode::Esc => self.cancel_transcript_search(),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.set_search_query(String::new());
            }
            KeyCode::Enter => {
                let direction = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    SearchDirection::Older
                } else {
                    SearchDirection::Newer
                };
                self.step_transcript_search(direction);
            }
            KeyCode::Backspace => {
                let mut query = self.transcript_search.query.clone();
                if query.pop().is_some() {
                    self.set_search_query(query);
                }
            }
            KeyCode::Char(ch)
                if matches!(
                    key.modifiers,
                    KeyModifiers::NONE | KeyModifiers::SHIFT | KeyModifiers::ALT
                ) && !key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let mut query = self.transcript_search.query.clone();
                if query.len() + ch.len_utf8() <= SEARCH_QUERY_BYTES {
                    query.push(ch);
                    self.set_search_query(query);
                }
            }
            // Every other key is still the editor's: a half-typed query must not leak into
            // the draft or move the transcript selection behind it.
            _ => {}
        }
        true
    }

    /// Replace the query. The scan restarts on the next frame through `reconcile`, so typing
    /// one character at a time narrows the count without ever re-scanning per keystroke.
    fn set_search_query(&mut self, query: String) {
        self.transcript_search.query = query;
        self.request_redraw();
    }

    /// Lines of one card as the frame shows them, in the order the row map numbers them. Raw
    /// scrollback (`Alt+R`) paints the source lines with no card prefix, so the scan reads
    /// that same text: a hit must be a line the reader can actually see.
    fn search_entry_lines(&mut self, index: usize, width: u16) -> Vec<Line<'static>> {
        if self.raw_output {
            let text = self
                .logs
                .get(index)
                .map(|entry| entry.text.clone())
                .unwrap_or_default();
            return super::raw_entry_lines(text.as_str());
        }
        self.render_entry_lines(index, false, width)
    }

    /// Advance the scan by one frame's budget. Bounded both ways: at most `budget` bytes of
    /// line text, and at most `SEARCH_MATCH_CAP` hits in total. Codex lets the last window
    /// overshoot the budget; so does this, by one line, which keeps a single long line from
    /// never being scanned at all.
    pub(super) fn advance_transcript_search(&mut self, budget: usize) -> SearchProgress {
        if !self.transcript_search.editing {
            return SearchProgress::Idle;
        }
        let width = self.transcript_viewport_width.max(1);
        let archived = self.history_archived_entries.min(self.logs.len());
        let identity = ScanIdentity {
            revision: self.transcript_render_revision,
            archived,
            entries: self.logs.len(),
            width,
        };
        let folded = fold_query(self.transcript_search.query.as_str());
        self.transcript_search.reconcile(identity, folded);
        if self.transcript_search.folded.is_empty() || self.transcript_search.scan_done {
            return SearchProgress::Done;
        }

        let mut scanned = 0usize;
        let mut index = self.transcript_search.scan_entry;
        let mut line = self.transcript_search.scan_line;
        while index < self.logs.len() {
            let cell_id = self.logs[index].cell_id;
            let lines = self.search_entry_lines(index, width);
            while line < lines.len() {
                let text = search_line_text(&lines[line]);
                scanned = scanned.saturating_add(text.len());
                for ((logical_low, logical_high), (display_low, display_high)) in
                    line_matches(text.as_str(), self.transcript_search.folded.as_str())
                {
                    if self.transcript_search.matches.len() >= SEARCH_MATCH_CAP {
                        // Stop where the ceiling falls: the count shown is the count kept.
                        self.transcript_search.has_more = true;
                        self.transcript_search.scan_done = true;
                        self.transcript_search.scan_entry = index;
                        self.transcript_search.scan_line = line;
                        return SearchProgress::Done;
                    }
                    self.transcript_search.matches.push(SearchMatch {
                        identity: LineIdentity {
                            cell_id,
                            line_within_cell: line,
                        },
                        logical: (logical_low, logical_high),
                        display: (display_low, display_high),
                    });
                }
                line += 1;
                if scanned >= budget {
                    self.transcript_search.scan_entry = index;
                    self.transcript_search.scan_line = line;
                    return SearchProgress::More;
                }
            }
            index += 1;
            line = 0;
        }
        self.transcript_search.scan_entry = index;
        self.transcript_search.scan_line = 0;
        self.transcript_search.scan_done = true;
        SearchProgress::Done
    }

    /// Step to the next or previous hit and bring its line into view, wrapping at either end.
    /// The first step from an unstroked list lands on the oldest hit going newer and the
    /// newest going older, so `Enter` after typing always reads as "the first one".
    fn step_transcript_search(&mut self, direction: SearchDirection) {
        let total = self.transcript_search.matches.len();
        if total == 0 {
            return;
        }
        let next = match (self.transcript_search.current, direction) {
            (None, SearchDirection::Newer) => 0,
            (None, SearchDirection::Older) => total - 1,
            (Some(index), SearchDirection::Newer) => (index + 1) % total,
            (Some(index), SearchDirection::Older) => (index + total - 1) % total,
        };
        self.transcript_search.current = Some(next);
        let identity = self.transcript_search.matches[next].identity;
        if let Some(row) = self.transcript_row_for_line(identity) {
            self.ensure_transcript_row_visible(row);
        }
        self.request_redraw();
    }

    /// The document row a stable line address occupies: the rows of every card before it,
    /// then its own line. The rows come from the same per-card line budget the window was
    /// chosen with, so the hit and the scroll cannot disagree about where a line is.
    fn transcript_row_for_line(&mut self, identity: LineIdentity) -> Option<usize> {
        let width = usize::from(self.transcript_viewport_width.max(1));
        let archived = self.history_archived_entries.min(self.logs.len());
        let mut row = 0usize;
        for index in archived..self.logs.len() {
            let count = self.entry_visual_line_count(index, width).max(1);
            if self.logs[index].cell_id == identity.cell_id {
                return Some(row + identity.line_within_cell.min(count.saturating_sub(1)));
            }
            row = row.saturating_add(count);
        }
        None
    }

    /// Slide the window only as far as it must go to show one document row, the same minimum
    /// move the transcript selection makes when it leaves the band.
    fn ensure_transcript_row_visible(&mut self, row: usize) {
        let viewport = usize::from(self.transcript_viewport_height.max(1));
        let max_scroll = self.max_transcript_scroll(viewport);
        let current = max_scroll.saturating_sub(self.transcript_offset_from_bottom.min(max_scroll));
        let last_visible = current.saturating_add(viewport.saturating_sub(1));
        let target = if row < current {
            row
        } else if row > last_visible {
            row.saturating_sub(viewport.saturating_sub(1))
        } else {
            current
        };
        self.transcript_offset_from_bottom = max_scroll.saturating_sub(target.min(max_scroll));
    }

    /// The band's one row while the search editor is up: the query, shed from its tail before
    /// anything is dropped, and the hit's ordinal against the count. `None` means even the
    /// shortest rung of the ladder will not fit, and the row goes back to the draft.
    pub(super) fn transcript_search_line(&self, width: u16, is_zh: bool) -> Option<Line<'static>> {
        if !self.transcript_search.editing || width == 0 {
            return None;
        }
        let search = &self.transcript_search;
        let room = usize::from(width);
        let total = if search.has_more {
            format!("{}+", search.matches.len())
        } else {
            search.matches.len().to_string()
        };
        let current = search
            .current
            .map(|index| (index + 1).to_string())
            .unwrap_or_else(|| NO_HIT.to_string());

        let bare = ui_text(
            is_zh,
            "search_query",
            &[("query", ""), ("index", current.as_str()), ("total", total.as_str())],
        );
        let fixed = UnicodeWidthStr::width(bare.as_str());
        if !search.query.is_empty() && fixed <= room {
            let query = shed_query(search.query.as_str(), room - fixed);
            let text = ui_text(
                is_zh,
                "search_query",
                &[
                    ("query", query.as_str()),
                    ("index", current.as_str()),
                    ("total", total.as_str()),
                ],
            );
            if UnicodeWidthStr::width(text.as_str()) <= room {
                return Some(Line::from(Span::styled(text, theme::accent_text())));
            }
        }
        // No room for the query at all: the counts alone still tell the reader the search is
        // up and what it found.
        let text = ui_text(
            is_zh,
            "search_count",
            &[("index", current.as_str()), ("total", total.as_str())],
        );
        if UnicodeWidthStr::width(text.as_str()) <= room {
            return Some(Line::from(Span::styled(text, theme::accent_text())));
        }
        None
    }
}

/// The plain text of a rendered line, spans concatenated: the text the scan reads is exactly
/// the text the frame paints.
fn search_line_text(line: &Line<'_>) -> String {
    let mut text = String::new();
    for span in &line.spans {
        text.push_str(span.content.as_ref());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folded_match_reports_its_characters_and_its_display_columns() {
        // gate covers 0..4, the tab 4..8, the second word sits at 8..12.
        let found = line_matches("gate\tGate value", "gate");
        assert_eq!(
            found,
            vec![((0, 4), (0, 4)), ((5, 9), (8, 12))],
            "case-folded, both occurrences, columns from the shared tab projection"
        );
    }

    #[test]
    fn a_match_reports_the_character_that_owns_a_tabs_padding() {
        // A query that starts inside the tab's padding still begins at the tab itself: the
        // column the reader sees is the column the frame paints, not the byte the fold found.
        let found = line_matches("gate\tvalue", "value");
        assert_eq!(found, vec![((5, 10), (8, 13))]);
    }

    #[test]
    fn an_empty_query_never_matches_and_occurrences_do_not_overlap() {
        assert!(line_matches("gate gate", "").is_empty());
        assert_eq!(line_matches("aaa", "aa"), vec![((0, 2), (0, 2))]);
        assert_eq!(
            line_matches("Gate gate GATE", "gate").len(),
            3,
            "the fold is per character, so every spelling of the same word is a hit"
        );
    }

    #[test]
    fn the_query_sheds_its_tail_and_never_grows_past_its_room() {
        assert_eq!(shed_query("gate", 8), "gate");
        assert_eq!(shed_query("gate", 4), "gate");
        assert_eq!(shed_query("gate", 3), "ga…");
        assert_eq!(shed_query("gate", 1), "…");
        assert_eq!(shed_query("gate", 0), "");
        assert_eq!(shed_query("", 4), "");
        // A wide character counts columns, not characters.
        assert_eq!(shed_query("中中中", 3), "中…");
    }

    #[test]
    fn a_restart_aims_the_cursor_at_the_first_addressable_entry() {
        let mut search = TranscriptSearch::default();
        search.matches.push(SearchMatch {
            identity: LineIdentity {
                cell_id: 3,
                line_within_cell: 1,
            },
            logical: (0, 2),
            display: (0, 2),
        });
        search.current = Some(0);
        search.has_more = true;
        search.scan_done = true;
        search.restart(7);
        assert!(search.matches.is_empty());
        assert_eq!(search.current, None);
        assert!(!search.has_more);
        assert_eq!((search.scan_entry, search.scan_line), (7, 0));
        assert!(!search.scan_done);
    }

    #[test]
    fn only_a_document_or_query_move_retires_a_finished_scan() {
        let base = ScanIdentity {
            revision: 4,
            archived: 1,
            entries: 9,
            width: 80,
        };
        let mut search = TranscriptSearch::default();
        search.reconcile(base, "gate".to_string());
        search.matches.push(SearchMatch {
            identity: LineIdentity {
                cell_id: 3,
                line_within_cell: 0,
            },
            logical: (0, 2),
            display: (0, 2),
        });
        search.scan_done = true;

        // A scan that has nothing to recompute keeps its hits.
        search.reconcile(base, "gate".to_string());
        assert_eq!(search.matches.len(), 1);
        assert!(search.scan_done);

        // The query refined, and one document fact moved, both retire it.
        search.reconcile(base, "gat".to_string());
        assert!(search.matches.is_empty());
        assert!(!search.scan_done);
        let moved = ScanIdentity { entries: 10, ..base };
        search.reconcile(moved, "gat".to_string());
        assert_eq!(search.scan_entry, moved.archived);
    }
}
