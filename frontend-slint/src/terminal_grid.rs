//! Terminal grid: a VT byte stream projected into positioned colour runs.
//!
//! The panel used to convert ANSI into Markdown for Slint's `StyledText`, which
//! can only append text: it had no cursor, no screen, and any program that
//! redrew in place produced garbage. Here the bytes go through `vt100`, which
//! owns the live screen model (cursor addressing, scroll regions, alternate
//! screen, wide characters, SGR/256/truecolour), and each flush projects the
//! viewport into horizontal runs that the view places on an absolute grid.
//! Blank runs on the default background are dropped, so the item count tracks
//! styled content rather than rows x columns.
//!
//! Scrollback is ours, because the parser's cannot be shown: its rows are only
//! reachable through the scrolled view, which is anchored to the live screen and
//! reaches exactly one screenful back (`visible_rows` subtracts the offset from
//! the row count, and underflows past it). That is still enough to *recover* what
//! just left the screen, and so what the panel keeps is a copy of those rows
//! taken at the moment they leave, in the order they left, down to the depth of
//! `MAX_HISTORY`. How many left is read from two signals that each cover the
//! other's blind spot: cursor arithmetic counts explicit line feeds, the
//! parser's scrollback length counts those plus wraps. Feed input is sliced in
//! both newlines and bytes, so no slice moves more rows than the view can name.

use std::collections::VecDeque;
use vt100::{Color, Parser};

/// Rows of output kept above the prompt, bounded so a long-lived shell cannot
/// grow memory without limit. Older output stays readable through the persisted
/// transcript rather than in RAM.
const MAX_HISTORY: usize = 1000;
/// Largest number of lines one feed slice may scroll, as a fraction of the
/// screen. Staying under a full screen keeps the captured rows inside what the
/// previous projection can name.
const SLICE_DIVISOR: usize = 2;

/// Classic 16-colour ANSI palette, then the 6x6x6 cube and 24-step gray ramp
/// fill 16..=255 (standard xterm values).
const ANSI16: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0xcd, 0x31, 0x31],
    [0x0d, 0xbc, 0x79],
    [0xe5, 0xe5, 0x10],
    [0x24, 0x72, 0xc8],
    [0xbc, 0x3f, 0xbc],
    [0x11, 0xa8, 0xcd],
    [0xe5, 0xe5, 0xe5],
    [0x66, 0x66, 0x66],
    [0xf1, 0x4c, 0x4c],
    [0x23, 0xd1, 0x8b],
    [0xf5, 0xf5, 0x43],
    [0x3b, 0x8e, 0xea],
    [0xd6, 0x70, 0xd6],
    [0x29, 0xb8, 0xdb],
    [0xff, 0xff, 0xff],
];

const CUBE_RAMP: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Palette entry for an indexed colour.
fn palette(index: u8) -> [u8; 3] {
    match index {
        0..=15 => ANSI16[index as usize],
        16..=231 => {
            let value = index - 16;
            [
                CUBE_RAMP[(value / 36) as usize],
                CUBE_RAMP[((value / 6) % 6) as usize],
                CUBE_RAMP[(value % 6) as usize],
            ]
        }
        _ => {
            let gray = 8 + (index as u16 - 232) * 10;
            [gray as u8, gray as u8, gray as u8]
        }
    }
}

/// Foreground colour resolved against the panel theme, so the view never has to
/// know what "default" means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ink {
    Default,
    Rgb([u8; 3]),
}

/// One horizontal run of identically styled cells.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Span {
    pub text: String,
    pub fg: Ink,
    /// `None` is the panel background: such runs are dropped entirely.
    pub bg: Option<[u8; 3]>,
    pub bold: bool,
    pub underline: bool,
    /// Row within the rendered viewport, `0` at the top.
    pub row: i32,
    pub col: i32,
    /// Width in cells; wide characters occupy two.
    pub cells: i32,
}

/// A rendered line: runs in column order, with `row` unused until the line is
/// placed in a viewport.
type Line = Vec<Span>;

/// Screen model plus the history and viewport needed to render it.
pub struct TerminalGrid {
    parser: Parser,
    rows: u16,
    cols: u16,
    history: VecDeque<Line>,
    /// Rows the viewport is scrolled up from the bottom.
    view_offset: usize,
    /// Rows the parser has pushed into its own scrollback deque so far, used as
    /// the second scroll signal.
    scrolled_seen: usize,
}

impl TerminalGrid {
    pub fn new(rows: u16, cols: u16) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        Self {
            parser: Parser::new(rows, cols, MAX_HISTORY),
            rows,
            cols,
            history: VecDeque::new(),
            view_offset: 0,
            scrolled_seen: 0,
        }
    }

    pub fn size(&self) -> (u16, u16) {
        (self.rows, self.cols)
    }

    /// Resize the screen. `vt100` truncates or pads rows in place (it cannot
    /// reflow), which is acceptable for a panel whose width changes rarely.
    pub fn set_size(&mut self, rows: u16, cols: u16) {
        if rows == 0 || cols == 0 || (rows == self.rows && cols == self.cols) {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.parser.set_size(rows, cols);
    }

    /// Feed decoded output bytes. The parser carries a partial escape sequence
    /// across calls, so chunk boundaries never matter.
    pub fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let max_lines = self.slice_rows();
        let max_bytes = max_lines.saturating_mul(self.cols as usize).max(1);
        let mut offset = 0;
        while offset < bytes.len() {
            let end = next_slice(bytes, offset, max_lines, max_bytes);
            let slice = &bytes[offset..end];
            let cursor_before = self.parser.screen().cursor_position().0 as usize;
            let feeds = slice.iter().filter(|byte| **byte == b'\n').count();
            self.parser.process(slice);
            let departed = self.scrolled_rows(cursor_before, feeds);
            self.capture_history(departed);
            offset = end;
        }
    }

    /// Forget the screen and the history (panel clear, or a fresh shell).
    pub fn reset(&mut self) {
        self.parser = Parser::new(self.rows, self.cols, MAX_HISTORY);
        self.history.clear();
        self.view_offset = 0;
        self.scrolled_seen = 0;
    }

    /// Lines one feed slice may scroll. The scroll view reaches one screenful
    /// back, so a slice is kept small enough that everything it moves is still
    /// recoverable from it.
    fn slice_rows(&self) -> usize {
        (self.rows as usize / SLICE_DIVISOR).max(1)
    }

    /// Rows that left the screen for the slice just processed. Cursor arithmetic
    /// counts explicit line feeds but not the scrolls a long line causes when it
    /// wraps at the bottom row, while the parser's own scrollback counter sees
    /// both yet stops growing once its deque is full; the larger of the two wins.
    /// A slice whose cursor did not land where its feeds predict contained a
    /// sequence that moved the cursor, so its arithmetic is discarded.
    fn scrolled_rows(&mut self, cursor_before: usize, feeds: usize) -> usize {
        // The alternate screen is a transient surface: its rows are not
        // scrollback, and its deque belongs to a different grid, so counting
        // either would pollute history with frames of a program that left.
        if self.parser.screen().alternate_screen() {
            return 0;
        }
        let last_row = self.rows as usize - 1;
        let after = self.parser.screen().cursor_position().0 as usize;
        let predicted = cursor_before + feeds;
        let arithmetic = if after == predicted.min(last_row) {
            predicted.saturating_sub(last_row)
        } else {
            0
        };
        // The parser exposes its scrollback length only through the clamp applied
        // to a view offset: ask for everything, read the clamp, put the view back.
        self.parser.set_scrollback(usize::MAX);
        let depth = self.parser.screen().scrollback();
        self.parser.set_scrollback(0);
        let counted = depth.saturating_sub(self.scrolled_seen);
        self.scrolled_seen = depth;
        arithmetic.max(counted).min(self.rows as usize)
    }

    /// Move the rows that left the screen into history.
    ///
    /// They are read back from `vt100` rather than from a projection of our own:
    /// by the time a slice has been processed the rows it scrolled off are gone
    /// from the live screen, but they are the top of its scroll view, and the
    /// view reaches exactly as far back as one screenful, which is the most any
    /// slice is allowed to move. Only those rows are read, so a feed costs in
    /// proportion to what scrolled and not to the size of the screen.
    fn capture_history(&mut self, departed: usize) {
        if departed == 0 {
            return;
        }
        self.parser.set_scrollback(departed);
        {
            let screen = self.parser.screen();
            let default_bg = screen.bgcolor();
            for row in 0..departed as u16 {
                self.history.push_back(line_at(&screen, row, self.cols, default_bg));
            }
        }
        self.parser.set_scrollback(0);
        while self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
        if !self.at_bottom() {
            // Keep the text under a scrolled-back viewport steady.
            self.view_offset += departed;
        }
    }

    /// Project the live screen into lines of runs, top row first.
    fn snapshot(&self) -> Vec<Line> {
        let screen = self.parser.screen();
        let default_bg = screen.bgcolor();
        (0..self.rows)
            .map(|row| line_at(&screen, row, self.cols, default_bg))
            .collect()
    }

    pub fn view_offset(&self) -> usize {
        self.view_offset
    }

    pub fn at_bottom(&self) -> bool {
        self.view_offset == 0
    }

    pub fn history_depth(&self) -> usize {
        self.history.len()
    }

    /// Total lines the viewport can address: history plus the live screen.
    pub fn total_lines(&self) -> usize {
        self.history.len() + self.rows as usize
    }

    /// Scroll the viewport, positive going up into history.
    pub fn scroll_by(&mut self, rows: i32) {
        let limit = self.history.len() as i64;
        let next = (self.view_offset as i64 + rows as i64).clamp(0, limit);
        self.view_offset = next as usize;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.view_offset = 0;
    }

    /// Put absolute row `top` (counted from the oldest line still held) at the
    /// top of the viewport, as a scrollbar thumb does.
    pub fn scroll_to_row(&mut self, top: usize) {
        let offset = self
            .total_lines()
            .saturating_sub(self.rows as usize)
            .saturating_sub(top);
        self.view_offset = offset.min(self.history.len());
    }

    /// Runs for the current viewport: history rows first, then live rows,
    /// renumbered so the view places them without knowing about the split.
    pub fn render(&self) -> Vec<Span> {
        let height = self.rows as usize;
        let total = self.total_lines();
        let top = total.saturating_sub(height + self.view_offset);
        let live = self.snapshot();
        let mut spans = Vec::new();
        for window_row in 0..height {
            let absolute = top + window_row;
            let line = match absolute.cmp(&self.history.len()) {
                std::cmp::Ordering::Less => &self.history[absolute],
                _ => match live.get(absolute - self.history.len()) {
                    Some(line) => line,
                    None => continue,
                },
            };
            for span in line {
                let mut span = span.clone();
                span.row = window_row as i32;
                spans.push(span);
            }
        }
        spans
    }

    /// Cursor position within the viewport, or `None` when hidden or scrolled
    /// away from the live screen.
    pub fn caret(&self) -> Option<(i32, i32)> {
        let screen = self.parser.screen();
        if screen.hide_cursor() || !self.at_bottom() {
            return None;
        }
        let (row, col) = screen.cursor_position();
        (row < self.rows).then_some((row as i32, col as i32))
    }
}

/// One row of the screen as styled runs. The spans come back with `row: 0`,
/// since the caller knows which row it asked for and the viewport assigns the
/// display row when history and live screen are stitched together.
fn line_at(screen: &vt100::Screen, row: u16, cols: u16, default_bg: Color) -> Line {
    let mut line: Line = Vec::new();
    let mut col = 0u16;
    while col < cols {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        if cell.is_wide_continuation() {
            col += 1;
            continue;
        }
        let width = if cell.is_wide() { 2 } else { 1 };
        let bold = cell.bold();
        let underline = cell.underline();
        let bg = background(cell.bgcolor(), default_bg);
        let fg = ink(cell.fgcolor());
        // Reading a cell's text allocates, so an empty cell that carries no
        // style at all is stepped over without asking for it.
        if !cell.has_contents() && bg.is_none() && fg == Ink::Default && !bold && !underline {
            col += width;
            continue;
        }
        let text = cell.contents();
        let blank = text.is_empty() || text == " ";
        // A styled blank keeps one space instead of an empty run, so the next
        // cell still merges and any text after it stays under its column.
        let text = if blank { " ".to_string() } else { text };
        match line.last_mut() {
            // Same style and adjacent cells: one run instead of one item per
            // character.
            Some(last)
                if last.fg == fg
                    && last.bg == bg
                    && last.bold == bold
                    && last.underline == underline
                    && last.col + last.cells == col as i32 =>
            {
                last.text.push_str(&text);
                last.cells += width as i32;
            }
            _ => line.push(Span {
                text,
                fg,
                bg,
                bold,
                underline,
                row: 0,
                col: col as i32,
                cells: width as i32,
            }),
        }
        col += width;
    }
    // A run's leading and trailing blanks paint nothing, so they are dropped and
    // the run narrowed to the text that remains. Interior blanks stay, or every
    // word of a sentence would become its own item.
    for span in line.iter_mut() {
        if span.bg.is_some() {
            continue;
        }
        let text = &span.text;
        let leading = text.len() - text.trim_start_matches(' ').len();
        let body = text[leading..].trim_end_matches(' ');
        let trailing = text.len() - leading - body.len();
        if leading + trailing > 0 {
            span.text = body.to_string();
            span.col += leading as i32;
            span.cells -= (leading + trailing) as i32;
        }
    }
    line.retain(|span| !span.text.is_empty());
    line
}

/// End of the next feed slice: just past `max_lines` line feeds or `max_bytes`
/// of input, whichever comes first, or the end of the buffer. Splitting anywhere
/// is safe because the parser is a state machine that carries a partial escape
/// sequence across calls.
fn next_slice(bytes: &[u8], from: usize, max_lines: usize, max_bytes: usize) -> usize {
    let mut seen = 0;
    let mut index = from;
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            seen += 1;
            if seen >= max_lines {
                return index + 1;
            }
        }
        index += 1;
        if index - from >= max_bytes {
            return index;
        }
    }
    bytes.len()
}

/// Cell foreground as an ink value; the palette lives here so the view stays
/// dumb about terminal internals.
fn ink(color: Color) -> Ink {
    match color {
        Color::Default => Ink::Default,
        Color::Idx(index) => Ink::Rgb(palette(index)),
        Color::Rgb(red, green, blue) => Ink::Rgb([red, green, blue]),
    }
}

/// Cell background, with the default background folded in as `None` so blank
/// runs can be dropped.
fn background(color: Color, default_bg: Color) -> Option<[u8; 3]> {
    match color {
        Color::Default if default_bg == Color::Default => None,
        other => match ink(other) {
            Ink::Rgb(rgb) => Some(rgb),
            // A background asking for the default ink is not producible by real
            // output; treat it as the panel colour.
            Ink::Default => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: u16, cols: u16) -> TerminalGrid {
        TerminalGrid::new(rows, cols)
    }

    /// Viewport text with column gaps restored, since blank cells produce no
    /// run. One row per line, trailing blanks trimmed.
    fn viewport(g: &TerminalGrid) -> String {
        let mut rows: Vec<String> = Vec::new();
        for row in 0..g.rows {
            let mut line = String::new();
            for span in g.render().into_iter().filter(|span| span.row == row as i32) {
                while line.len() < span.col as usize {
                    line.push(' ');
                }
                line.push_str(&span.text);
            }
            rows.push(line.trim_end().to_string());
        }
        while rows.last().map(String::is_empty).unwrap_or(false) {
            rows.pop();
        }
        rows.join("\n")
    }

    fn span_at(spans: &[Span], col: i32) -> Option<&Span> {
        spans.iter().find(|span| span.col == col)
    }

    /// Feed numbered lines without a trailing break, so the cursor stays on the
    /// last written row.
    fn fill(g: &mut TerminalGrid, from: usize, to: usize) {
        let text: Vec<String> = (from..to).map(|n| format!("line {n}")).collect();
        g.feed(text.join("\r\n").as_bytes());
    }

    #[test]
    fn plain_text_wraps_at_the_column_count() {
        let mut g = grid(4, 10);
        g.feed(b"hello");
        assert_eq!(viewport(&g), "hello");
        g.feed(&[b'x'; 20]);
        assert_eq!(viewport(&g), "helloxxxxx\nxxxxxxxxxx\nxxxxx");
    }

    #[test]
    fn cursor_addressing_rewrites_in_place() {
        let mut g = grid(3, 20);
        g.feed(b"one two three\n");
        // Row 1 column 5 is index (0, 4). The markdown path could not express an
        // overwrite at all.
        g.feed(b"\x1b[1;5HXXXX");
        assert_eq!(viewport(&g), "one XXXXthree");
    }

    #[test]
    fn erase_and_redraw_produce_clean_lines() {
        let mut g = grid(3, 20);
        g.feed(b"junk line\nsecond\n");
        g.feed(b"\x1b[2J\x1b[Hfresh");
        assert_eq!(viewport(&g), "fresh");
    }

    #[test]
    fn progress_redraw_overwrites_the_same_line() {
        let mut g = grid(3, 30);
        // A carriage-return redraw: the old path dropped the CR and left one
        // line per frame; a terminal overwrites in place.
        g.feed(b"downloading\r[\x1b[32m==\x1b[0m 100%\n");
        assert_eq!(viewport(&g), "[== 100%ing");
    }

    #[test]
    fn sgr_colours_become_runs_and_blank_gaps_cost_no_item() {
        let mut g = grid(2, 40);
        g.feed(b"\x1b[31mred\x1b[0m plain");
        let spans = g.render();
        let red = span_at(&spans, 0).expect("red run");
        assert_eq!(red.text, "red");
        assert_eq!(red.fg, Ink::Rgb(ANSI16[1]));
        assert_eq!(red.bg, None);
        assert_eq!(red.cells, 3);
        // The default-ink word still renders; only the blank gap is dropped.
        assert_eq!(span_at(&spans, 4).expect("plain run").text, "plain");
        assert_eq!(spans.iter().filter(|span| span.col == 3).count(), 0);
    }

    #[test]
    fn indexed_and_truecolour_are_resolved() {
        let mut g = grid(2, 40);
        g.feed(b"\x1b[38;5;46mA\x1b[38;2;10;20;30mB");
        let spans = g.render();
        assert_eq!(span_at(&spans, 0).unwrap().fg, Ink::Rgb([0, 255, 0]));
        assert_eq!(span_at(&spans, 1).unwrap().fg, Ink::Rgb([10, 20, 30]));
    }

    #[test]
    fn palette_cube_and_gray_ramp_are_standard() {
        assert_eq!(palette(16), [0, 0, 0]);
        assert_eq!(palette(46), [0, 255, 0]);
        assert_eq!(palette(231), [255, 255, 255]);
        assert_eq!(palette(232), [8, 8, 8]);
        assert_eq!(palette(255), [238, 238, 238]);
    }

    #[test]
    fn background_and_bold_and_underline_survive() {
        let mut g = grid(2, 40);
        g.feed(b"\x1b[44;1;4mHI");
        let spans = g.render();
        let run = span_at(&spans, 0).expect("styled run");
        assert_eq!(run.bg, Some(ANSI16[4]));
        assert_eq!(run.bold, true);
        assert_eq!(run.underline, true);
        assert_eq!(run.text, "HI");
    }

    #[test]
    fn background_paint_reaches_blank_cells_as_one_run() {
        let mut g = grid(2, 10);
        g.feed(b"a\x1b[47m  b");
        let painted = g
            .render()
            .into_iter()
            .find(|span| span.bg == Some(ANSI16[7]))
            .expect("painted run");
        // The painted gap and the word after it share a style, so they are one
        // run whose text keeps the following character under its column.
        assert_eq!(painted.col, 1);
        assert_eq!(painted.cells, 3);
        assert_eq!(painted.text, "  b");
    }

    #[test]
    fn adjacent_equal_styles_coalesce_into_one_run() {
        let mut g = grid(2, 40);
        g.feed(b"\x1b[31ma\x1b[31mb\x1b[31mc");
        let spans = g.render();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "abc");
        assert_eq!(spans[0].cells, 3);
    }

    #[test]
    fn wide_characters_advance_two_cells() {
        let mut g = grid(2, 40);
        g.feed("中文ab".as_bytes());
        let spans = g.render();
        assert_eq!(spans[0].text, "中文ab");
        // Two wide glyphs (4 cells) plus two ascii cells.
        assert_eq!(spans[0].cells, 6);
    }

    #[test]
    fn rows_that_leave_the_screen_become_history() {
        let mut g = grid(3, 20);
        fill(&mut g, 0, 6);
        assert_eq!(viewport(&g), "line 3\nline 4\nline 5");
        assert_eq!(g.history_depth(), 3);
        assert!(g.at_bottom());
    }

    #[test]
    fn scrolling_up_shows_history_and_new_output_keeps_it_in_place() {
        let mut g = grid(3, 20);
        fill(&mut g, 0, 6);
        g.scroll_by(2);
        assert_eq!(g.view_offset(), 2);
        assert_eq!(viewport(&g), "line 1\nline 2\nline 3");
        // More output arrives while scrolled back: the viewport must not slide.
        g.feed(b"\r\nline 6");
        assert_eq!(g.view_offset(), 3);
        assert_eq!(viewport(&g), "line 1\nline 2\nline 3");
        g.scroll_to_bottom();
        assert_eq!(viewport(&g), "line 4\nline 5\nline 6");
    }

    #[test]
    fn scroll_to_row_positions_the_viewport_by_history_row() {
        let mut g = grid(3, 20);
        fill(&mut g, 0, 9);
        assert_eq!(g.history_depth(), 6);
        g.scroll_to_row(0);
        assert_eq!(viewport(&g), "line 0\nline 1\nline 2");
        g.scroll_to_row(3);
        assert_eq!(viewport(&g), "line 3\nline 4\nline 5");
        // The newest row that can sit at the top is the first live one.
        g.scroll_to_row(6);
        assert_eq!(g.view_offset(), 0);
        assert_eq!(viewport(&g), "line 6\nline 7\nline 8");
        g.scroll_to_row(999);
        assert_eq!(g.view_offset(), 0);
    }

    #[test]
    fn scroll_limits_are_clamped() {
        let mut g = grid(2, 20);
        g.feed(b"a\r\nb\r\nc");
        g.scroll_by(999);
        assert_eq!(g.view_offset(), g.history_depth());
        g.scroll_by(-999);
        assert_eq!(g.view_offset(), 0);
        assert!(g.at_bottom());
    }

    #[test]
    fn alternate_screen_does_not_pollute_history() {
        let mut g = grid(3, 20);
        g.feed(b"kept one\nkept two\nkept three\nkept four");
        let before = g.history_depth();
        // vim-style: enter the alternate screen, overflow it, leave it again.
        g.feed(b"\x1b[?1049hfull screen\nline two\nline three\nline four\x1b[?1049l");
        assert_eq!(
            g.history_depth(),
            before,
            "full-screen redraws must not append to scrollback"
        );
    }

    #[test]
    fn a_burst_larger_than_the_screen_keeps_every_line() {
        let mut g = grid(4, 20);
        let burst: Vec<String> = (0..40).map(|n| format!("b{n}")).collect();
        g.feed(burst.join("\r\n").as_bytes());
        assert_eq!(viewport(&g), "b36\nb37\nb38\nb39");
        // Sliced feeding keeps every row that left, not just one screenful.
        assert_eq!(g.history_depth(), 36);
        let depth = g.history_depth();
        g.scroll_by(depth as i32);
        assert_eq!(viewport(&g).lines().next(), Some("b0"));
    }

    #[test]
    fn history_is_bounded() {
        let mut g = grid(2, 20);
        let written = MAX_HISTORY + 50;
        let burst: Vec<String> = (0..written).map(|n| format!("l{n}")).collect();
        g.feed(burst.join("\r\n").as_bytes());
        assert_eq!(g.history_depth(), MAX_HISTORY);
        // Everything older than the cap is gone: the oldest surviving line is
        // the number of departed rows minus the cap.
        let depth = g.history_depth();
        g.scroll_by(depth as i32);
        let departed = written - g.rows as usize;
        assert_eq!(
            viewport(&g).lines().next(),
            Some(format!("l{}", departed - MAX_HISTORY).as_str())
        );
    }

    #[test]
    fn a_wrapped_line_scrolls_history_too() {
        let mut g = grid(2, 10);
        // Twenty characters on a ten-column two-row screen fill it without
        // scrolling anything off.
        g.feed(b"0123456789abcdefghij");
        assert_eq!(viewport(&g), "0123456789\nabcdefghij");
        assert_eq!(g.history_depth(), 0);
        // The twenty-first character wraps, and the row it pushes off becomes
        // history even though no line feed was ever fed.
        g.feed(b"klmnopqrst");
        assert_eq!(viewport(&g), "abcdefghij\nklmnopqrst");
        assert_eq!(g.history_depth(), 1);
        g.scroll_by(1);
        assert_eq!(viewport(&g), "0123456789\nabcdefghij");
    }

    #[test]
    fn a_wrapped_burst_keeps_its_content() {
        let mut g = grid(4, 20);
        // One long unbroken line: a screenful and a half of wrapped text with no
        // newline anywhere in it.
        g.feed(&[b'x'; 120]);
        assert_eq!(g.history_depth(), 2);
        let depth = g.history_depth();
        g.scroll_by(depth as i32);
        // The rows above the viewport are real wrapped content, not the blanks
        // a mis-detected shift would have captured.
        let spans = g.render();
        let full_rows = (0..2)
            .filter(|row| {
                spans
                    .iter()
                    .any(|span| span.row == *row && span.cells == 20)
            })
            .count();
        assert_eq!(full_rows, 2);
    }

    #[test]
    fn resize_reprojects_without_losing_the_viewport() {
        let mut g = grid(3, 20);
        g.feed(b"some output");
        g.set_size(5, 40);
        assert_eq!(g.size(), (5, 40));
        assert_eq!(viewport(&g), "some output");
        g.set_size(0, 0);
        assert_eq!(g.size(), (5, 40), "a zero size is not a resize");
    }

    #[test]
    fn reset_clears_screen_and_history() {
        let mut g = grid(2, 20);
        g.feed(b"a\r\nb\r\nc");
        g.reset();
        assert_eq!(g.history_depth(), 0);
        assert_eq!(viewport(&g), "");
        assert_eq!(g.view_offset(), 0);
    }

    #[test]
    fn caret_reports_position_and_hides_when_scrolled_back() {
        let mut g = grid(3, 20);
        g.feed(b"abc");
        assert_eq!(g.caret(), Some((0, 3)));
        g.feed(b"\x1b[?25l");
        assert_eq!(g.caret(), None);
        g.feed(b"\x1b[?25h");
        fill(&mut g, 0, 6);
        g.scroll_by(1);
        assert_eq!(g.caret(), None, "the caret belongs to the live screen");
    }

    #[test]
    fn control_sequences_never_leak_into_rendered_text() {
        let mut g = grid(3, 30);
        g.feed(b"\x1b]0;window title\x07real \x1b[1;32mtext\x1b[0m");
        assert_eq!(viewport(&g), "real text");
    }
}
