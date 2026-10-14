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
//! the display-column range that text projects to. The projection is `text_tabs`' eight-column
//! tab stop and nothing else, because that is what the frame paints and what the highlight
//! marks.
//!
//! Invalidation is decided by **document identity, never by scroll position**: the render
//! revision (which every append, trim, manual disclosure and resize bumps), the fold state the
//! search itself borrowed, the archived-entry boundary, the entry count, the wrap width and the
//! folded query. Scrolling is not in that set — the reader moving the window does not change
//! what the document says, so a finished scan stays finished and a jump is not thrown away by a
//! wheel notch. The fold state is in it, because opening a card changes how many lines that
//! card has: a hit collected against the folded numbering points at a different line once the
//! card is open.
//!
//! The scan is bounded twice over: a byte budget per frame so no single frame can walk a
//! long transcript, and a hard match ceiling so the hit list cannot grow without bound. Past
//! the ceiling the scan stops and the band says `200+`, which is the honest reading — the
//! count shown is what was actually collected, not what a full pass might have found.
//!
//! Out of scope here, deliberately, and the reasons are the shape of this client rather than
//! unfinished work: the scan never reaches further back than the addressable document, because
//! the 舵机 hands older lines to the terminal's own scrollback, where they are no longer
//! addressable — codex's `history_loaded` paging assumes it owns its transcript, which this
//! form cannot do, so `SearchProgress` stays as it is. There is no regex or other `SearchMode`,
//! no OSC 8 change, and the match ceiling and the per-frame budget are unchanged.
//!
//! Two things a hit needs to be *readable* rather than merely counted, both in this slice: the
//! frame marks the hit's own display columns (reversed video, plus bold on the one the reader
//! stepped to), and a hit inside a card that is currently folded opens that card — in the
//! search's own ledger, never in the reader's, and the fold state is part of what a scan result
//! is valid for because unfolding renumbers the card's lines.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::locale::ui_text;
use crate::tui::text_tabs;
use crate::tui::theme;

use super::text_selection::{slice_line_by_display_columns, LineIdentity};
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
    /// Which cards are open. How many lines a card renders, and in what order, is a function
    /// of its fold state, so `line_within_cell` — the address a hit is booked against — only
    /// means the same thing in the same fold state. A search that opens a card therefore
    /// retires its own scan; without this the reader's next step would point at whichever line
    /// the new numbering put at the old index, and a jump and a mark would both land wrong.
    /// Carries the search's own ledger exactly; the reader's ledger is folded in through
    /// `revision`, which every manual disclosure bump moves with it.
    expanded: u64,
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
    /// The hit the reader was standing on, kept across the one restart the search causes
    /// itself. Opening a folded card renumbers its lines, so the hits have to be collected
    /// again; the place is handed back as soon as a collected hit names the same run of the
    /// same card, and dropped if the scan finishes without finding it — then the text the
    /// reader was on is gone from the document, and no mark should pretend otherwise.
    pub(super) resume: Option<ResumePlace>,
    identity: Option<ScanIdentity>,
}

/// What a hit has to come back as for the reader to still be standing on it: the card, and
/// the run of that line the query matched. A fold renumbers lines around a hit, never inside
/// the text of the line it fell on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ResumePlace {
    pub(super) cell_id: u64,
    pub(super) logical: (usize, usize),
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
            resume: None,
            identity: None,
        }
    }
}

impl TranscriptSearch {
    /// Forget everything derived: the next frame rescans from the document's first line.
    /// `keep_place` is for the restart a fold of the search's own caused, where the reader's
    /// place in the list has to come back with the hits.
    fn restart(&mut self, archived: usize, keep_place: bool) {
        self.resume = if keep_place {
            self.current.and_then(|ordinal| {
                self.matches.get(ordinal).map(|hit| ResumePlace {
                    cell_id: hit.identity.cell_id,
                    logical: hit.logical,
                })
            })
        } else {
            None
        };
        self.matches.clear();
        self.current = None;
        self.has_more = false;
        self.scan_entry = archived;
        self.scan_line = 0;
        self.scan_done = false;
    }

    /// Once the hits are collected again, hand the kept place back as an ordinal. `None` while
    /// the scan has not reached the card, and `Some` the moment it has.
    fn take_resumed_place(&mut self) -> Option<usize> {
        let place = self.resume?;
        let found = self.matches.iter().position(|hit| {
            hit.identity.cell_id == place.cell_id && hit.logical == place.logical
        })?;
        self.resume = None;
        Some(found)
    }

    /// Drop the collected hits when the document or the query moved. Scroll position is
    /// deliberately not part of the key.
    fn reconcile(&mut self, identity: ScanIdentity, folded: String) {
        if self.identity == Some(identity) && self.folded == folded {
            return;
        }
        // Only a fold of the search's own is worth carrying a place across: the same document
        // at the same width with the same query, and nothing but the open cards moved. Any
        // other change is a new search, and a new search has no accepted hit.
        let same_document = self.identity.is_some_and(|previous| {
            previous.revision == identity.revision
                && previous.archived == identity.archived
                && previous.entries == identity.entries
                && previous.width == identity.width
        });
        let keep_place = same_document && self.folded == folded;
        self.identity = Some(identity);
        self.folded = folded;
        self.restart(identity.archived, keep_place);
    }
}

fn fold_query(query: &str) -> String {
    query.chars().flat_map(char::to_lowercase).collect()
}

/// The weight a search mark carries: the selection's own reversed video for a hit, and that
/// plus bold for the one hit the reader stepped to. Never a colour, so `NO_COLOR` cannot take
/// the distinction away, and the run keeps the fg and bg its card gave it.
fn search_mark_style(current: bool) -> Style {
    let mark = theme::selection_mark();
    if current {
        mark.add_modifier(Modifier::BOLD)
    } else {
        mark
    }
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
        // A search starts from the document as the reader left it: an opening borrowed by an
        // earlier search is not this one's business.
        self.forget_search_expansions();
        let search = &mut self.transcript_search;
        search.editing = true;
        search.query.clear();
        search.folded.clear();
        search.origin_offset = self.transcript_offset_from_bottom;
        search.restart(self.history_archived_entries.min(self.logs.len()), false);
        search.identity = None;
        self.request_redraw();
    }

    /// Leave the search editor and restore the scroll the reader was at before opening it.
    /// Every card the search opened goes back the way it was found.
    fn cancel_transcript_search(&mut self) {
        self.forget_search_expansions();
        let search = &mut self.transcript_search;
        search.editing = false;
        search.query.clear();
        search.folded.clear();
        search.identity = None;
        search.restart(self.history_archived_entries.min(self.logs.len()), false);
        let viewport = usize::from(self.transcript_viewport_height.max(1));
        let origin = search.origin_offset.min(self.max_transcript_scroll(viewport));
        self.transcript_offset_from_bottom = origin;
        self.request_redraw();
    }

    /// A card the search opened, closed again because the search is over. The reader's own
    /// ledger is never touched, so a card they opened by hand stays open through a search and
    /// out of it.
    pub(super) fn forget_search_expansions(&mut self) {
        if self.search_expanded_cards.is_empty() {
            return;
        }
        self.search_expanded_cards.clear();
        self.search_expand_revision = self.search_expand_revision.wrapping_add(1);
        self.invalidate_transcript_frame();
    }

    /// One card out of the search's ledger: its card left the transcript, so the opening that
    /// was borrowed for it has nothing to point at.
    pub(super) fn retract_search_expansion(&mut self, cell_id: u64) {
        if self.search_expanded_cards.remove(&cell_id) {
            self.search_expand_revision = self.search_expand_revision.wrapping_add(1);
            self.invalidate_transcript_frame();
        }
    }

    /// Open the card a hit is on, for as long as the search lasts, when the reader walked to a
    /// hit that a fold is hiding behind. Returns whether a card was opened.
    ///
    /// The ledger this writes is the search's own: a search must not leave the reader's
    /// disclosure state changed behind it, and a card the reader opened by hand needs nothing
    /// here. The fold is a document fact for the scan, which is why the scan's identity carries
    /// it — see `ScanIdentity::expanded`.
    fn reveal_hit_card(&mut self, identity: LineIdentity) -> bool {
        if self.card_is_expanded_by_search(identity.cell_id) {
            return false;
        }
        let Some(index) = self
            .logs
            .iter()
            .position(|entry| entry.cell_id == identity.cell_id)
        else {
            return false;
        };
        if !self.card_is_collapsible(index) || self.card_id_is_expanded(identity.cell_id) {
            return false;
        }
        self.search_expanded_cards.insert(identity.cell_id);
        self.search_expand_revision = self.search_expand_revision.wrapping_add(1);
        // The rows this card paints change with it, so the frame, the row map and the scroll
        // bounds are rebuilt — without a document revision, which says what the document says,
        // not how a card is folded.
        self.invalidate_transcript_frame();
        true
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
    ///
    /// The reader's place survives only a restart the search's own fold caused, so this also
    /// hands it back as soon as the rebuilt list holds the hit again.
    pub(super) fn advance_transcript_search(&mut self, budget: usize) -> SearchProgress {
        let progress = self.scan_transcript_search(budget);
        // A restart the search's own fold caused leaves the reader standing on nothing until
        // the rebuilt list holds the hit again; when it does, the mark and the window both
        // follow it, so an Enter that opened a card still lands on the line it opened for.
        let resumed = self.transcript_search.take_resumed_place();
        if let Some(ordinal) = resumed {
            self.transcript_search.current = Some(ordinal);
            let identity = self.transcript_search.matches[ordinal].identity;
            if let Some(row) = self.transcript_row_for_line(identity) {
                self.ensure_transcript_row_visible(row);
            }
            self.request_redraw();
        } else if self.transcript_search.scan_done {
            // The list is complete and the hit the reader stood on is not in it: the text it
            // named left the document with the fold. Nothing is accepted, and nothing is marked.
            self.transcript_search.resume = None;
        }
        progress
    }

    fn scan_transcript_search(&mut self, budget: usize) -> SearchProgress {
        if !self.transcript_search.editing {
            return SearchProgress::Idle;
        }
        let width = self.transcript_viewport_width.max(1);
        let archived = self.history_archived_entries.min(self.logs.len());
        let identity = ScanIdentity {
            revision: self.transcript_render_revision,
            expanded: self.search_expand_revision,
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
    ///
    /// A hit the reader walks to is a hit they are owed: if a fold is hiding the card it is
    /// on, that card opens for as long as the search lasts (`reveal_hit_card`), which
    /// renumbers the card's lines and so retires the scan the hit came from. The reader's own
    /// disclosure ledger is not part of any of this.
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
        self.reveal_hit_card(identity);
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

    /// Mark this frame's hits, on the lines the frame is about to paint.
    ///
    /// Weight, not colour: every hit takes `theme::selection_mark()` — reversed video, no fg,
    /// no bg, so the run keeps whatever colour its own card gave it and the mark still reads
    /// in a monochrome terminal — and the hit the reader stepped to takes bold on top of that.
    /// The one accepted hit is therefore heavier than the rest without a second colour, which
    /// is how a reader finds their place in a page full of matches.
    ///
    /// **Drawing order: the pointer selection paints after this and wins.** A search mark is
    /// paint only; the text a reader copies is the selection's, so a hit the live selection
    /// reaches into yields its own mark entirely: the reversed video on the text they picked up
    /// is the reversed video they copy, with no extra weight pretending to be part of it. Where
    /// the two never touch, both show. Painting the other way round — marks over the selection
    /// — would leave text that looks copied and is not in the clipboard, the one thing a
    /// highlight must never do.
    ///
    /// A hit is projected through the row map the last frame built, the same projection a click
    /// uses, so a mark and a click cannot disagree about which line a hit is on. And a mark is
    /// only laid down where the frame's own text still holds the query at those columns: a fold
    /// that moved the numbering between the scan and this frame leaves the hit stale for one
    /// frame, and a stale hit paints nothing rather than marking the wrong text.
    pub(crate) fn paint_transcript_search_marks(&self, lines: &mut [Line<'static>]) {
        let search = &self.transcript_search;
        if !search.editing || search.matches.is_empty() || lines.is_empty() {
            return;
        }
        for (ordinal, hit) in search.matches.iter().enumerate() {
            let Some(&index) = self.transcript_hit.line_slots.get(&hit.identity) else {
                continue;
            };
            let Some(line) = lines.get(index) else {
                continue;
            };
            if self.transcript_selection_covers(index, hit.display.0, hit.display.1) {
                continue;
            }
            let text = search_line_text(line);
            // Display columns, from the same projection the scan booked and the frame paints.
            if !line_matches(text.as_str(), search.folded.as_str())
                .iter()
                .any(|(_, display)| *display == hit.display)
            {
                continue;
            }
            let style = search_mark_style(Some(ordinal) == search.current);
            lines[index] = slice_line_by_display_columns(
                line.clone(),
                hit.display.0,
                hit.display.1,
                style,
            );
        }
    }

    /// The band's one row while the search editor is up: the query, shed from its tail before
    /// anything is dropped, and the hit's ordinal against the count. `None` means even the
    /// shortest rung of the ladder will not fit, and the row goes back to the draft.
    pub(crate) fn transcript_search_line(&self, width: u16, is_zh: bool) -> Option<Line<'static>> {
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
        search.restart(7, false);
        assert!(search.matches.is_empty());
        assert_eq!(search.current, None);
        assert!(!search.has_more);
        assert_eq!((search.scan_entry, search.scan_line), (7, 0));
        assert!(!search.scan_done);
        // A restart the document caused keeps no place: the hit it named may not exist.
        assert_eq!(search.resume, None);
    }

    #[test]
    fn only_a_document_or_query_move_retires_a_finished_scan() {
        let base = ScanIdentity {
            revision: 4,
            expanded: 0,
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

    /// The fold state is part of what a scan is valid for, and only the fold state the search
    /// borrowed is carried exactly: unfolding a card renumbers its lines, so a hit kept from
    /// the folded numbering points at another line. A query refinement or a document move is a
    /// new search and drops the reader's place; a fold alone hands it back as soon as the hit
    /// is collected again.
    #[test]
    fn a_fold_of_the_searchs_own_retires_the_scan_and_keeps_the_readers_place() {
        let base = ScanIdentity {
            revision: 4,
            expanded: 0,
            archived: 0,
            entries: 9,
            width: 80,
        };
        let mut search = TranscriptSearch::default();
        search.reconcile(base, "gate".to_string());
        search.matches.push(SearchMatch {
            identity: LineIdentity {
                cell_id: 3,
                line_within_cell: 4,
            },
            logical: (0, 4),
            display: (0, 4),
        });
        search.current = Some(0);
        search.scan_done = true;

        // The same document, the same query, one more card open: the hits are stale.
        let opened = ScanIdentity { expanded: 1, ..base };
        search.reconcile(opened, "gate".to_string());
        assert!(search.matches.is_empty(), "the scan runs again");
        assert!(!search.scan_done);
        assert_eq!(search.current, None, "nothing is accepted until it lands");
        assert_eq!(
            search.resume,
            Some(ResumePlace {
                cell_id: 3,
                logical: (0, 4),
            }),
            "the place is kept for the hit to come back to"
        );

        // The rebuild holds it: the same card, the same matched run, at whatever line the
        // expanded numbering puts it on.
        search.matches.push(SearchMatch {
            identity: LineIdentity {
                cell_id: 3,
                line_within_cell: 11,
            },
            logical: (0, 4),
            display: (0, 4),
        });
        assert_eq!(search.take_resumed_place(), Some(0));
        assert_eq!(search.resume, None);

        // A different run of a different card is not the reader's place.
        search.current = Some(0);
        search.reconcile(
            ScanIdentity {
                expanded: 2,
                ..base
            },
            "gate".to_string(),
        );
        search.matches.push(SearchMatch {
            identity: LineIdentity {
                cell_id: 7,
                line_within_cell: 0,
            },
            logical: (6, 10),
            display: (6, 10),
        });
        assert_eq!(search.take_resumed_place(), None, "not the same hit");

        // And a document move is a new search: no place survives it.
        search.current = Some(0);
        search.reconcile(
            ScanIdentity {
                revision: 5,
                ..base
            },
            "gate".to_string(),
        );
        assert_eq!(search.resume, None);
    }

    /// The mark is a weight, not a colour: reversed video on a hit, bold on the accepted one,
    /// and nothing on either that paints a colour, so the same reading holds with `NO_COLOR`
    /// set and without it.
    #[test]
    fn a_mark_carries_weight_and_never_a_colour() {
        for current in [false, true] {
            let style = search_mark_style(current);
            assert!(
                style.add_modifier.contains(Modifier::REVERSED),
                "a hit always takes the selection's own mark"
            );
            assert_eq!(
                style.add_modifier.contains(Modifier::BOLD),
                current,
                "only the accepted hit takes the extra weight"
            );
            assert_eq!(
                (style.fg, style.bg),
                (None, None),
                "a colour would be the only thing a monochrome terminal keeps"
            );
        }
        assert!(
            search_mark_style(true).add_modifier.contains(Modifier::REVERSED),
            "the accepted hit is the plain mark plus weight, never a replacement"
        );
    }
}
