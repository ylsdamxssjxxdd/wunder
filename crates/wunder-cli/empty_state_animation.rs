//! Colourless welcome avatar: a white/grey half-block relief painted under the
//! brand header of a fresh thread. The wording keeps the terminal's left margin
//! like any other CLI output; only the relief is centred on the terminal axis.
//!
//! The pixel data in `welcome_logo_art.rs` is a neutral grey ladder (slot 15 is
//! transparent), so the head never introduces hue into the frame; the volume is
//! carried by luminance only. Terminals with `NO_COLOR` set fall back to a
//! luminance ramp of ASCII glyphs, which keeps the relief readable without any
//! escape codes.
//!
//! The relief is sampled per *cell*, not per character: a cell is two pixels
//! wide and two pixels tall, and how tall it is in real pixels depends on the
//! terminal font. A cell is never square, so the art is fitted to the cell
//! aspect the terminal reports: first `ws_xpixel`/`ws_ypixel`, then the reply to
//! `CSI 14 t`/`CSI 18 t` (the only source VTE terminals give), and only when
//! nothing answers to the aspect of a typical monospace font. Painting 1:1
//! pixels into cells stretches the head vertically by that same factor, which
//! reads as a head squeezed horizontally.

#[path = "welcome_logo_art.rs"]
mod welcome_logo_art;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Below this many columns the art is dropped instead of being squeezed.
const MIN_COLUMNS: u16 = 24;
/// The art is 2 source rows per terminal cell; six cells still read as a head.
const MIN_ROWS: u16 = 6;
/// Luminance ramp used when the terminal refuses colour.
const SHADE_RAMP: [char; 10] = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];
/// Cell aspect assumed when neither a measurement nor an override is available.
///
/// A monospace cell is taller than it is wide, and by how much depends on the
/// font: DejaVu Sans Mono is 1.93, Ubuntu Mono 2.00, Noto Sans Mono 2.27, and
/// the Ubuntu/GNOME Terminal default (Ubuntu Sans Mono 13) is 2.10. Windows
/// consoles use 8×16 or 9×18 raster cells, i.e. exactly 2.0, which is also the
/// shape the pixel data was generated for.
#[cfg(windows)]
const FALLBACK_CELL_ASPECT: f32 = 2.0;
#[cfg(not(windows))]
const FALLBACK_CELL_ASPECT: f32 = 2.1;
/// Plausibility window for a cell aspect. Terminals that report a stale pixel
/// size (or a size from before a font change) fall back instead of painting a
/// head distorted the other way.
const CELL_ASPECT_RANGE: std::ops::RangeInclusive<f32> = 1.2..=4.0;
/// Explicit override for terminals that cannot be measured, e.g. a font whose
/// line height is unusual.
const CELL_ASPECT_ENV: &str = "WUNDER_CLI_CELL_ASPECT";
/// Queries answered by xterm-compatible terminals: the text area in pixels and
/// the same area in cells. VTE answers both; it ignores `CSI 16 t`, which would
/// have given the cell size directly.
#[cfg(unix)]
const CELL_SIZE_QUERY: &[u8] = b"\x1b[14t\x1b[18t";
/// Total time the probe waits for those replies. A terminal answers within
/// microseconds, so a silent one only costs the user this much at startup.
#[cfg(unix)]
const CELL_SIZE_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(120);
/// Longest single wait inside the probe, so the loop re-checks the deadline.
#[cfg(unix)]
const CELL_SIZE_POLL_SLICE: std::time::Duration = std::time::Duration::from_millis(20);

/// Half-block rows for the welcome avatar, sized to the terminal width and to
/// the cell aspect the terminal reports.
///
/// Returns an empty vector when the terminal is too narrow: the banner then
/// degrades to the text header instead of painting a smeared blob.
pub(crate) fn logo_lines_for_cell(columns: u16, cell_aspect: f32) -> Vec<Line<'static>> {
    match fit(columns, cell_aspect) {
        Some((width, rows)) => render(width, rows, colours_enabled()),
        None => Vec::new(),
    }
}

/// How many cell-heights one cell is, and therefore how the head has to be
/// sampled to stay round on screen: an explicit override wins, then a measured
/// pixel size, then the terminal's own answer about its cell size, then the
/// font family's usual ratio.
fn cell_aspect() -> f32 {
    env_cell_aspect()
        .or_else(terminal_cell_aspect)
        .or_else(probed_cell_aspect)
        .unwrap_or(FALLBACK_CELL_ASPECT)
}

/// Cell aspect as answered by the terminal itself, `None` on platforms that are
/// not asked (Windows consoles never answer these queries).
#[cfg(not(unix))]
fn probed_cell_aspect() -> Option<f32> {
    None
}

/// Ask the terminal for its text area in pixels (`CSI 14 t`) and in cells
/// (`CSI 18 t`); their quotient is the real cell aspect.
///
/// This is the only way to learn the font geometry of a VTE-based terminal
/// (GNOME Terminal, Tilix, Terminator): those answer `TIOCGWINSZ` with zero
/// `ws_xpixel`/`ws_ypixel`, so the measurement above never fires and the head
/// used to be painted for a 2.1 cell — on a 10×26 cell that is a fifth too
/// tall, which reads as a head squeezed sideways. The replies are consumed from
/// the raw input here; unrecognised escape sequences are ignored by the event
/// loop if one ever arrives late.
#[cfg(unix)]
fn probed_cell_aspect() -> Option<f32> {
    static PROBED: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *PROBED.get_or_init(query_cell_aspect)
}

#[cfg(unix)]
fn query_cell_aspect() -> Option<f32> {
    use std::io::{IsTerminal, Read, Write};

    // A piped or redirected run has no terminal to answer, and writing queries
    // into someone's file would be worse than a slightly wrong head.
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return None;
    }
    // `TERM=dumb` is a terminal that answers nothing by contract; waiting for it
    // would only slow the first frame down.
    if std::env::var("TERM").is_ok_and(|term| term.eq_ignore_ascii_case("dumb")) {
        return None;
    }
    let mut stdout = std::io::stdout();
    stdout.write_all(CELL_SIZE_QUERY).ok()?;
    stdout.flush().ok()?;

    let mut stdin = std::io::stdin();
    let mut replies = Vec::with_capacity(48);
    let deadline = std::time::Instant::now() + CELL_SIZE_PROBE_TIMEOUT;
    while stdin_readable(deadline) {
        let mut chunk = [0u8; 32];
        match stdin.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => replies.extend_from_slice(&chunk[..read]),
        }
        if let Some(aspect) = cell_aspect_from_replies(&replies) {
            return Some(aspect);
        }
    }
    None
}

/// Waits for more bytes on `stdin`, never past `deadline`. Polling in slices
/// keeps a terminal that answers only one of the two queries from stalling the
/// whole probe.
#[cfg(unix)]
fn stdin_readable(deadline: std::time::Instant) -> bool {
    use std::os::fd::AsRawFd;

    let now = std::time::Instant::now();
    if now >= deadline {
        return false;
    }
    let slice = (deadline - now).min(CELL_SIZE_POLL_SLICE);
    let mut fd = libc::pollfd {
        fd: std::io::stdin().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid `pollfd` for the borrowed stdin descriptor, and the
    // timeout is a plain millisecond count.
    let ready = unsafe { libc::poll(&mut fd, 1, slice.as_millis() as libc::c_int) };
    ready > 0 && fd.revents & libc::POLLIN != 0
}

/// The two replies as the terminal sent them: `ESC [ 4 ; height ; width t` for
/// the text area in pixels and `ESC [ 8 ; rows ; columns t` for the grid. Both
/// are needed; a single one is not enough to know a cell's shape.
#[cfg(any(unix, test))]
#[cfg_attr(all(not(unix), test), allow(dead_code))]
fn cell_aspect_from_replies(replies: &[u8]) -> Option<f32> {
    let (mut pixels, mut cells) = (None, None);
    for params in csi_reports(replies) {
        match params.as_slice() {
            [4, height, width] => pixels = Some((*height, *width)),
            [8, rows, columns] => cells = Some((*rows, *columns)),
            _ => {}
        }
    }
    let (height, width) = pixels?;
    let (rows, columns) = cells?;
    measured_cell_aspect(columns, rows, width, height)
}

/// Every `ESC [ <params> t` in `replies`, as parsed parameters. Anything that
/// is not such a report — including half-arrived ones — is skipped instead of
/// guessed at.
#[cfg(any(unix, test))]
fn csi_reports(replies: &[u8]) -> Vec<Vec<u16>> {
    let mut reports = Vec::new();
    let mut index = 0;
    while index + 2 < replies.len() {
        if replies[index] != 0x1b || replies[index + 1] != b'[' {
            index += 1;
            continue;
        }
        let start = index + 2;
        let mut end = start;
        while end < replies.len() && (replies[end].is_ascii_digit() || replies[end] == b';') {
            end += 1;
        }
        if end < replies.len() && replies[end] == b't' {
            if let Some(params) = parse_report_parameters(&replies[start..end]) {
                reports.push(params);
            }
            index = end + 1;
        } else {
            index = start;
        }
    }
    reports
}

#[cfg(any(unix, test))]
fn parse_report_parameters(raw: &[u8]) -> Option<Vec<u16>> {
    let text = std::str::from_utf8(raw).ok()?;
    text.split(';')
        .map(|part| part.parse::<u16>().ok())
        .collect()
}

/// `WUNDER_CLI_CELL_ASPECT` lets a terminal that reports nothing still get the
/// geometry right; an implausible value is ignored rather than obeyed.
fn env_cell_aspect() -> Option<f32> {
    let raw = std::env::var(CELL_ASPECT_ENV).ok()?;
    let value: f32 = raw.trim().parse().ok()?;
    CELL_ASPECT_RANGE.contains(&value).then_some(value)
}

#[cfg(not(windows))]
fn terminal_cell_aspect() -> Option<f32> {
    let size = crossterm::terminal::window_size().ok()?;
    measured_cell_aspect(size.columns, size.rows, size.width, size.height)
}

/// Windows consoles answer `ws_xpixel`/`ws_ypixel` with zero, so the fallback
/// is the rule there rather than a measurement.
#[cfg(windows)]
fn terminal_cell_aspect() -> Option<f32> {
    None
}

/// Cell aspect from a window size, `None` when the numbers cannot be trusted.
/// VTE-based terminals (GNOME Terminal, Tilix) report zeroes here, which is why
/// the terminal query and the fallback matter as much as the measurement.
/// Windows consoles never reach this path, only the tests do.
#[cfg_attr(all(windows, not(test)), allow(dead_code))]
fn measured_cell_aspect(columns: u16, rows: u16, width_px: u16, height_px: u16) -> Option<f32> {
    if columns == 0 || rows == 0 || width_px == 0 || height_px == 0 {
        return None;
    }
    let cell_width = f32::from(width_px) / f32::from(columns);
    let cell_height = f32::from(height_px) / f32::from(rows);
    let aspect = cell_height / cell_width;
    CELL_ASPECT_RANGE.contains(&aspect).then_some(aspect)
}

/// The welcome banner a fresh thread opens with, kept as a description rather
/// than as painted rows: the transcript paints it at whatever width it has, so
/// the head is re-fitted and re-centred on every resize instead of staying at
/// the column count of the first frame.
#[derive(Debug, Clone)]
pub(crate) struct Banner {
    version: String,
    workdir: String,
    is_zh: bool,
    cell_aspect: f32,
}

impl Banner {
    /// The cell shape is measured once, at startup, so painting a frame never
    /// has to ask the terminal for its pixel size again.
    pub(crate) fn new(version: &str, workdir: &str, is_zh: bool) -> Self {
        Self {
            version: version.to_string(),
            workdir: workdir.to_string(),
            is_zh,
            cell_aspect: cell_aspect(),
        }
    }

    /// The product text at the left margin, one breathing row, then the grey
    /// avatar centred in `columns`. `session_header` stays the single owner of
    /// the wording.
    pub(crate) fn lines(&self, columns: u16) -> Vec<Line<'static>> {
        banner_lines_for_cell(
            columns,
            self.cell_aspect,
            self.version.as_str(),
            self.workdir.as_str(),
            self.is_zh,
        )
    }
}

/// The banner for an explicit cell aspect, so the geometry can be pinned in
/// tests without a terminal.
///
/// The wording leads and the head closes the block: the first lines a reader
/// lands on are the brand, the directory and the hint, and the relief reads as
/// the logo of the empty state rather than as a header pushing the text down.
///
/// The two parts are placed differently on purpose. The text is ordinary CLI
/// output and stays at the left margin; the relief is a picture and sits on the
/// terminal axis. Centring the text with the art pushed the brand a third of the
/// screen inwards, which made the opening frame hard to scan.
pub(crate) fn banner_lines_for_cell(
    columns: u16,
    cell_aspect: f32,
    version: &str,
    workdir: &str,
    is_zh: bool,
) -> Vec<Line<'static>> {
    let mut lines = crate::session_header::header_lines(version, workdir, is_zh);
    let mut art = logo_lines_for_cell(columns, cell_aspect);
    if !art.is_empty() {
        centre_lines(art.as_mut_slice(), columns);
        lines.push(Line::default());
        lines.extend(art);
    }
    lines
}

/// Centres a block of rows in the terminal.
///
/// The block is as wide as its widest row. Rows are then centred inside that
/// block rather than pinned to its left edge, so a narrow row keeps the block's
/// axis. Used for the relief, whose rows are all the same width.
fn centre_lines(lines: &mut [Line<'static>], columns: u16) {
    let available = usize::from(columns);
    let block = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .min(available);
    if block == 0 {
        return;
    }
    // A block that does not fit gains nothing from an offset; it is clipped by
    // the transcript anyway.
    let left = (available - block) / 2;
    for line in lines.iter_mut() {
        let width = line.width();
        if width == 0 {
            continue;
        }
        let pad = left + block.saturating_sub(width) / 2;
        if pad > 0 {
            line.spans.insert(0, Span::raw(" ".repeat(pad)));
        }
    }
}

fn colours_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

/// Cells the relief occupies for a terminal of `columns` columns.
///
/// One cell carries two source pixels vertically and two horizontally, so a
/// source pixel lands on screen as `cell_width × (cell_height / 2)` — square
/// only when the cell happens to be twice as tall as it is wide. The head keeps
/// the source proportions when
///
/// ```text
/// columns           source width
/// ---------------- = ------------
/// rows · cell_aspect  source height
/// ```
///
/// i.e. `rows = columns · source_height / (source_width · cell_aspect)`.
/// Assuming a cell aspect of exactly 2 is the same as the old fixed 17 rows;
/// a taller cell (HiDPI, larger line height) needs fewer rows, which is what
/// stops the head from being stretched vertically and read as squeezed
/// horizontally. Rows never exceed `SOURCE_HEIGHT / 2`: that is all the
/// resolution a half-block cell can show.
fn fit(columns: u16, cell_aspect: f32) -> Option<(u16, u16)> {
    let max_width = columns
        .saturating_sub(2)
        .min(welcome_logo_art::SOURCE_WIDTH as u16);
    let aspect = cell_aspect.max(1.0);
    let exact_rows = f32::from(max_width) * welcome_logo_art::SOURCE_HEIGHT as f32
        / (welcome_logo_art::SOURCE_WIDTH as f32 * aspect);
    // Nearest row: one row is a 5% error at this size, so rounding keeps the
    // head closer to the source proportions than always rounding up.
    let rows = (exact_rows.round() as u16).min((welcome_logo_art::SOURCE_HEIGHT / 2) as u16);
    (max_width >= MIN_COLUMNS && rows >= MIN_ROWS).then_some((max_width, max(rows, MIN_ROWS)))
}

fn max(left: u16, right: u16) -> u16 {
    if left > right {
        left
    } else {
        right
    }
}

fn render(width: u16, rows: u16, colours: bool) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(usize::from(rows));
    for row in 0..usize::from(rows) {
        let mut spans = Vec::with_capacity(usize::from(width));
        for column in 0..usize::from(width) {
            let source_x = column * welcome_logo_art::SOURCE_WIDTH / usize::from(width);
            let source_y_top = row * welcome_logo_art::SOURCE_HEIGHT / usize::from(rows);
            let source_y_bottom =
                ((row + 1) * welcome_logo_art::SOURCE_HEIGHT / usize::from(rows)).saturating_sub(1);
            let top = shade(source_x, source_y_top);
            let bottom = shade(source_x, source_y_bottom);
            spans.push(cell(top, bottom, colours));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn cell(top: Option<u8>, bottom: Option<u8>, colours: bool) -> Span<'static> {
    if colours {
        return match (top, bottom) {
            (None, None) => Span::raw(" "),
            (Some(grey), None) => Span::styled("▀", Style::default().fg(grey_colour(grey))),
            (None, Some(grey)) => Span::styled("▄", Style::default().fg(grey_colour(grey))),
            (Some(upper), Some(lower)) => Span::styled(
                "▀",
                Style::default()
                    .fg(grey_colour(upper))
                    .bg(grey_colour(lower)),
            ),
        };
    }
    // Without colour support one cell collapses to a single luminance glyph.
    let grey = match (top, bottom) {
        (None, None) => return Span::raw(" "),
        (Some(grey), None) | (None, Some(grey)) => grey,
        (Some(upper), Some(lower)) => (u16::from(upper) + u16::from(lower)) as u8 / 2,
    };
    Span::raw(ramp_glyph(grey).to_string())
}

fn ramp_glyph(grey: u8) -> char {
    let index = usize::from(grey) * (SHADE_RAMP.len() - 1) / 255;
    SHADE_RAMP[index.min(SHADE_RAMP.len() - 1)]
}

fn grey_colour(grey: u8) -> Color {
    Color::Rgb(grey, grey, grey)
}

/// Grey value of one source pixel; `None` is the transparent slot.
fn shade(x: usize, y: usize) -> Option<u8> {
    let row = welcome_logo_art::ROWS[y / 2].as_bytes();
    let packed = (hex_nibble(row[x * 2]) << 4) | hex_nibble(row[x * 2 + 1]);
    let index = if y % 2 == 0 {
        packed >> 4
    } else {
        packed & 0x0f
    };
    (index != 15).then(|| welcome_logo_art::PALETTE[usize::from(index)][0])
}

fn hex_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cell exactly twice as tall as it is wide: the pixel data was generated
    /// for this shape (two source rows per cell, square-ish on screen).
    const SQUARE_PIXEL_CELL: f32 = 2.0;

    #[test]
    fn a_wide_terminal_gets_a_full_bleed_relief() {
        let lines = logo_lines_for_cell(120, SQUARE_PIXEL_CELL);
        assert_eq!(
            lines.len(),
            usize::from(fit(120, SQUARE_PIXEL_CELL).expect("wide").1)
        );
        assert_eq!(lines.len(), (welcome_logo_art::SOURCE_HEIGHT / 2));
        for line in &lines {
            assert_eq!(line.spans.len(), welcome_logo_art::SOURCE_WIDTH);
        }
    }

    #[test]
    fn the_art_scales_down_with_the_terminal_and_then_disappears() {
        let medium = logo_lines_for_cell(30, SQUARE_PIXEL_CELL);
        assert!(!medium.is_empty());
        assert!(medium.iter().all(|line| line.spans.len() <= 28));
        assert!(
            fit(20, SQUARE_PIXEL_CELL).is_none()
                && logo_lines_for_cell(20, SQUARE_PIXEL_CELL).is_empty()
        );
    }

    #[test]
    fn a_taller_cell_needs_fewer_rows_so_the_head_stays_round() {
        // A 2:1 cell is the 17-row reference; a HiDPI or large-line-height
        // terminal pushes the cell taller, and keeping 17 rows there stretched
        // the head vertically, which reads as squeezed horizontally.
        // 38 columns (the width cap keeps a 2-column margin) × 34 / (40 × 2.0).
        assert_eq!(fit(40, SQUARE_PIXEL_CELL).expect("wide"), (38, 16));
        // 38 × 34 / (40 × 2.2) = 14.7 → 15 rows.
        assert_eq!(fit(40, 2.2).expect("wide"), (38, 15));
        // 38 × 34 / (40 × 2.6) = 12.4 → 12 rows.
        assert_eq!(fit(40, 2.6).expect("wide"), (38, 12));
        // A squatter cell cannot gain resolution: a half-block cell never shows
        // more than two source rows.
        assert_eq!(fit(40, 1.6).expect("wide").1, 17);
    }

    #[test]
    fn a_tighter_terminal_keeps_the_head_round_by_rows_alone() {
        // The row count is the only vertical lever: the width stays at the cap,
        // so a taller cell has to give up rows or the head grows a long chin.
        let reference = logo_lines_for_cell(100, SQUARE_PIXEL_CELL);
        let tall = logo_lines_for_cell(100, 2.4);
        assert_eq!(reference.len(), 17);
        assert_eq!(tall.len(), 14);
        for line in &tall {
            assert_eq!(line.spans.len(), welcome_logo_art::SOURCE_WIDTH);
        }
    }

    #[test]
    fn the_cell_aspect_comes_from_the_reported_pixel_size() {
        // 80×24 cells in 800×480 pixels: 10 px wide, 20 px tall.
        assert_eq!(measured_cell_aspect(80, 24, 800, 480), Some(2.0));
        // A terminal that reports nothing, or a stale size, is not trusted.
        assert_eq!(measured_cell_aspect(80, 24, 0, 0), None);
        assert_eq!(measured_cell_aspect(0, 24, 800, 480), None);
        assert_eq!(measured_cell_aspect(80, 24, 800, 40), None);
        assert_eq!(measured_cell_aspect(80, 24, 800, 9600), None);
    }

    /// A VTE terminal (GNOME Terminal) answers `CSI 14 t` with its text area in
    /// pixels and `CSI 18 t` with the same area in cells, while `TIOCGWINSZ`
    /// keeps `ws_xpixel`/`ws_ypixel` at zero. These are the numbers a real
    /// 80×24 window reported: 800×624 px, i.e. a 10×26 cell.
    #[cfg(unix)]
    const VTE_WINDOW_REPORT: &[u8] = b"\x1b[4;624;800t\x1b[8;24;80t";

    #[cfg(unix)]
    #[test]
    fn the_terminal_itself_tells_the_cell_aspect_when_the_window_size_has_no_pixels()
    {
        // 26 / 10: a cell taller than any monospace font metric, which is why
        // the 2.1 fallback painted the head with a fifth too many rows.
        assert_eq!(cell_aspect_from_replies(VTE_WINDOW_REPORT), Some(2.6));
        // A half-arrived reply is not a measurement; neither is a zero size nor
        // an unrelated report.
        assert_eq!(cell_aspect_from_replies(b"\x1b[4;624;800t"), None);
        assert_eq!(cell_aspect_from_replies(b"\x1b[8;24;80t"), None);
        assert_eq!(cell_aspect_from_replies(b"\x1b[4;624;0t\x1b[8;24;80t"), None);
        assert_eq!(
            cell_aspect_from_replies(b"\x1b[6;10;20t\x1b[4;624;800t\x1b[8;24;80t"),
            Some(2.6)
        );
        // Bytes before and after the reports (a stray key, a split read) do not
        // hide them.
        assert_eq!(
            cell_aspect_from_replies(b"q\x1b[4;624;800t\x1b[8;24;80t\x1b["),
            Some(2.6)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_vte_cell_keeps_the_head_round_at_the_same_width() {
        // 40 × 34 / (40 × 2.6) = 13.1 → 13 rows: 400 px wide by 338 px tall on
        // screen, the 1.18 of the source. The 2.1 fallback drew 16 rows, i.e.
        // 416 px tall, and the head read as squeezed sideways.
        let art = logo_lines_for_cell(80, 2.6);
        assert_eq!(art.len(), 13);
        assert!(art
            .iter()
            .all(|line| line.spans.len() == welcome_logo_art::SOURCE_WIDTH));
    }

    #[test]
    fn an_unmeasured_terminal_falls_back_to_a_round_head() {
        // The platform fallback is the cell shape of the fonts that platform
        // ships: 2.0 for Windows raster cells, 2.1 for the Ubuntu/GNOME mono.
        assert!(FALLBACK_CELL_ASPECT >= 2.0 && FALLBACK_CELL_ASPECT <= 2.1);
        let rows = fit(40, FALLBACK_CELL_ASPECT).expect("wide").1;
        assert!((15..=17).contains(&rows), "fallback rows: {rows}");
        // A 100-column terminal reaches the 40-column cap; 40 × 34 / (40 × 2.0).
        assert_eq!(fit(100, 2.0).expect("wide"), (40, 17));
    }

    #[test]
    fn every_visible_cell_is_neutral_grey() {
        for line in logo_lines_for_cell(80, FALLBACK_CELL_ASPECT) {
            for span in line.spans {
                match span.style.fg {
                    Some(Color::Rgb(red, green, blue)) => {
                        assert_eq!(red, green);
                        assert_eq!(green, blue);
                    }
                    Some(other) => panic!("unexpected colour {other:?}"),
                    None => {}
                }
                if let Some(Color::Rgb(red, green, blue)) = span.style.bg {
                    assert_eq!(red, green);
                    assert_eq!(green, blue);
                }
            }
        }
    }

    #[test]
    fn the_palette_ladder_is_grey_and_spans_shadow_to_white() {
        for entry in welcome_logo_art::PALETTE {
            assert!(
                entry[0] == entry[1] && entry[1] == entry[2],
                "coloured slot"
            );
        }
        assert_eq!(welcome_logo_art::PALETTE[0][0], 48);
        assert_eq!(welcome_logo_art::PALETTE[14][0], 255);
    }

    #[test]
    fn transparent_cells_paint_nothing() {
        let lines = render(40, 17, true);
        let painted = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .filter(|span| span.content.as_ref() != " ")
            .count();
        assert!(painted > 100, "the head should cover a real area");
    }

    #[test]
    fn the_no_colour_ramp_never_emits_escape_styling() {
        let lines = render(40, 17, false);
        for line in lines {
            for span in line.spans {
                assert!(span.style.fg.is_none() && span.style.bg.is_none());
            }
        }
    }

    #[test]
    fn the_banner_stacks_the_brand_text_above_the_head() {
        let header = crate::session_header::header_lines("0.4.0", "/tmp/work", true);
        let art_rows = logo_lines_for_cell(80, FALLBACK_CELL_ASPECT).len();
        assert!(art_rows > 0, "an 80 column terminal fits the head");
        let lines = banner_lines_for_cell(80, FALLBACK_CELL_ASPECT, "0.4.0", "/tmp/work", true);
        // The wording leads, one breathing row follows, then the relief.
        assert_eq!(lines.len(), header.len() + 1 + art_rows);
        let content = |index: usize| -> String {
            lines[index]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        };
        assert!(content(0).contains("wunder 命令行"));
        assert!(content(1).contains("/tmp/work"));
        assert_eq!(lines[header.len()].width(), 0, "a breathing row");
        // 80 columns: the 40 column head, centred.
        assert_eq!(
            lines[header.len() + 1].width(),
            20 + welcome_logo_art::SOURCE_WIDTH
        );
    }

    #[test]
    fn a_narrow_terminal_keeps_the_text_banner_only() {
        let lines = banner_lines_for_cell(20, FALLBACK_CELL_ASPECT, "0.4.0", "/tmp/work", false);
        assert_eq!(
            lines.len(),
            crate::session_header::header_lines("0.4.0", "/tmp/work", false).len()
        );
    }

    #[test]
    fn the_banner_sits_in_the_middle_of_a_wide_terminal() {
        // 100 columns: the head caps at 40, so the block is 40 wide and 30
        // columns of air go either side. Pinned to the left it read as a stray
        // corner glyph rather than as a welcome.
        let art_rows = usize::from(fit(100, SQUARE_PIXEL_CELL).expect("wide").1);
        let lines = banner_lines_for_cell(100, SQUARE_PIXEL_CELL, "0.4.0", "/tmp/work", true);
        // Brand, workdir, one breathing row, then the relief.
        assert_eq!(art_rows, lines.len() - 3);
        for line in &lines[lines.len() - art_rows..] {
            assert_eq!(line.width(), 30 + welcome_logo_art::SOURCE_WIDTH);
        }
    }

    #[test]
    fn the_wording_keeps_the_left_margin_while_the_head_is_centred() {
        // The text is ordinary CLI output and never moves; only the relief is
        // placed on the terminal axis.
        let lines = banner_lines_for_cell(100, SQUARE_PIXEL_CELL, "0.4.0", "/tmp/work", true);
        let header = crate::session_header::header_lines("0.4.0", "/tmp/work", true);
        let rows = &lines[..header.len()];
        let text = |line: &Line<'static>| -> String {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        };
        for (row, expected) in rows.iter().zip(header.iter()) {
            assert_eq!(
                text(row),
                text(expected),
                "the banner must not re-place the wording"
            );
            assert_eq!(row.width(), expected.width());
        }
        // 100 columns: the 40 column head keeps 30 columns of air either side.
        let art_rows = lines.len() - header.len() - 1;
        assert!(art_rows > 0);
        for line in &lines[lines.len() - art_rows..] {
            assert_eq!(line.width(), 30 + welcome_logo_art::SOURCE_WIDTH);
        }
    }

    #[test]
    fn a_narrow_welcome_still_starts_at_column_zero() {
        let lines = banner_lines_for_cell(20, FALLBACK_CELL_ASPECT, "0.4.0", "/tmp/work", false);
        let first: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(first.starts_with("  >_ "), "unexpected lead: {first:?}");
    }

    #[test]
    fn the_block_is_centred_as_one_unit_not_row_by_row() {
        // Rows share the block's axis: the narrow ones are centred inside it
        // rather than pinned to its left edge.
        let wide_row = "  a row wider than the art itself";
        let mut lines = vec![
            Line::from("h".repeat(40)),
            Line::from("short"),
            Line::default(),
            Line::from(wide_row),
        ];
        centre_lines(lines.as_mut_slice(), 100);
        assert_eq!(lines[0].width(), 30 + 40);
        assert_eq!(lines[1].width(), 30 + (40 - 5) / 2 + 5);
        // A breathing row stays empty rather than becoming trailing padding.
        assert_eq!(lines[2].width(), 0);
        assert_eq!(
            lines[3].width(),
            30 + (40 - wide_row.len()) / 2 + wide_row.len()
        );
    }

    #[test]
    fn a_block_wider_than_the_terminal_is_not_pushed_off_it() {
        let mut lines = vec![Line::from("h".repeat(120)), Line::from("tail")];
        centre_lines(lines.as_mut_slice(), 80);
        // The over-wide row cannot be centred, so it keeps its own left edge…
        assert_eq!(lines[0].width(), 120);
        // …and the short row is centred in the columns that are visible.
        assert_eq!(lines[1].width(), 38 + 4);
    }
}
