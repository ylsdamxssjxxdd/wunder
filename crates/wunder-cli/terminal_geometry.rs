//! Terminal cell geometry: how tall one character cell is compared with its
//! width, which is what decides how many half-block rows the welcome head may
//! use.
//!
//! Three sources, in order: an explicit `WUNDER_CLI_CELL_ASPECT` override, the
//! window size the terminal reports in pixels (`TIOCGWINSZ`), and the terminal's
//! own answer to `CSI 14 t`/`CSI 18 t`. VTE terminals (GNOME Terminal, Tilix,
//! Terminator) only answer the query, and only on their own idle turn — about
//! 100 ms later — so the query runs in a background thread the banner collects
//! from instead of blocking the first frame.
//!
//! A multiplexer is not a measuring instrument: tmux answers both `TIOCGWINSZ`
//! and the query with a hard-coded 16×32 cell for any client that never told it
//! the real pixel size, so that report is filtered out rather than painted.
//! Whatever a real terminal reports is remembered by the caller
//! (`config/cli-terminal.json`) for the runs where nobody can answer.

#[cfg(unix)]
use std::sync::OnceLock;

/// Cell aspect assumed when neither a measurement nor an override is available.
///
/// A monospace cell is taller than it is wide, and by how much depends on the
/// font: DejaVu Sans Mono is 1.93, Ubuntu Mono 2.00, Noto Sans Mono 2.27, and
/// the Ubuntu/GNOME Terminal default (Ubuntu Sans Mono 13) is 2.10. Windows
/// consoles use 8×16 or 9×18 raster cells, i.e. exactly 2.0, which is also the
/// shape the pixel data was generated for.
#[cfg(windows)]
pub(crate) const FALLBACK_CELL_ASPECT: f32 = 2.0;
#[cfg(not(windows))]
pub(crate) const FALLBACK_CELL_ASPECT: f32 = 2.1;
/// Plausibility window for a cell aspect. Terminals that report a stale pixel
/// size (or a size from before a font change) fall back instead of painting a
/// head distorted the other way.
pub(crate) const CELL_ASPECT_RANGE: std::ops::RangeInclusive<f32> = 1.2..=4.0;

/// Explicit override for terminals that cannot be measured, e.g. a font whose
/// line height is unusual.
const CELL_ASPECT_ENV: &str = "WUNDER_CLI_CELL_ASPECT";

/// Queries answered by xterm-compatible terminals: the text area in pixels and
/// the same area in cells. VTE answers both; it ignores `CSI 16 t`, which would
/// have given the cell size directly.
#[cfg(unix)]
const CELL_SIZE_QUERY: &[u8] = b"\x1b[14t\x1b[18t";
/// Total time the probe waits for those replies.
///
/// A VTE terminal (GNOME Terminal, Tilix, Terminator) does not answer from the
/// input handler: it computes the text extent on its own idle turn and sends
/// both reports about 100 ms later. The old 120 ms budget therefore raced the
/// terminal and lost on a loaded desktop — and a lost answer is not a blank
/// screen but a head painted for the 2.1 font guess, i.e. a fifth too tall on a
/// 10×26 cell, which reads as the head being squeezed sideways. The probe runs
/// in the background (`start_cell_aspect_probe`), so this budget costs the
/// first frame nothing.
#[cfg(unix)]
const CELL_SIZE_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(800);
/// Longest single wait inside the probe, so the loop re-checks the deadline and
/// the stop flag.
#[cfg(unix)]
const CELL_SIZE_POLL_SLICE: std::time::Duration = std::time::Duration::from_millis(20);
/// How long the banner waits for the background probe before it paints.
///
/// The probe is started before the engine finishes loading the session, so its
/// answer is normally already there; this cap only bounds a terminal that never
/// answers at all.
#[cfg(unix)]
const CELL_ASPECT_COLLECT_WAIT: std::time::Duration = std::time::Duration::from_millis(250);
/// Cell shape of the multiplexer's own default (`tmux`), which it reports for
/// any client that does not tell it the real pixel size.
const FABRICATED_CELL_WIDTH: u32 = 16;
const FABRICATED_CELL_HEIGHT: u32 = 32;

/// Whether a terminal multiplexer sits between this process and the real
/// terminal. Their cell reports are the *client's* geometry as the multiplexer
/// remembers it, which for a client that reports no pixels is a hard-coded
/// default.
pub(crate) fn behind_multiplexer() -> bool {
    ["TMUX", "STY", "ZELLIJ"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
        || std::env::var("TERM").is_ok_and(|term| {
            term.starts_with("tmux") || term.starts_with("screen") || term.starts_with("zellij")
        })
}

/// A report that is exactly the multiplexer's own default cell. A real font
/// with the same 16×32 raster exists, but it would not be reported *through* a
/// multiplexer, so this is unambiguous there.
#[cfg_attr(not(test), allow(dead_code))]
fn is_fabricated_cell(columns: u16, rows: u16, width_px: u16, height_px: u16) -> bool {
    u32::from(width_px) == u32::from(columns) * FABRICATED_CELL_WIDTH
        && u32::from(height_px) == u32::from(rows) * FABRICATED_CELL_HEIGHT
}

/// The background geometry probe, started once per process.
#[cfg(unix)]
struct CellAspectProbe {
    /// `Receiver` is `Send` but not `Sync`, so the handle is locked; only the
    /// banner ever collects the answer.
    answer: std::sync::Mutex<std::sync::mpsc::Receiver<Option<f32>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(unix)]
static CELL_ASPECT_PROBE: OnceLock<CellAspectProbe> = OnceLock::new();

/// Starts asking the terminal for its cell shape, off the startup path.
///
/// Called once the terminal is in raw mode and before the engine builds the
/// session: the probe blocks on stdin, so it must not race the event stream, and
/// it must not hold up the first frame either. Terminals that need no probing
/// (an explicit override, a multiplexer, a terminal that reports pixels) spawn
/// nothing.
#[cfg(unix)]
pub(crate) fn start_cell_aspect_probe() {
    if CELL_ASPECT_PROBE.get().is_some()
        || env_cell_aspect().is_some()
        || behind_multiplexer()
        || terminal_cell_aspect().is_some()
    {
        return;
    }
    let (sender, answer) = std::sync::mpsc::channel();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_stop = std::sync::Arc::clone(&stop);
    let spawned = std::thread::Builder::new()
        .name("cli-cell-aspect".to_string())
        .spawn(move || {
            let _ = sender.send(query_cell_aspect(&thread_stop));
        });
    if spawned.is_ok() {
        let _ = CELL_ASPECT_PROBE.set(CellAspectProbe {
            answer: std::sync::Mutex::new(answer),
            stop,
        });
    }
}

/// Windows consoles answer none of these queries, so there is nothing to ask.
#[cfg(not(unix))]
pub(crate) fn start_cell_aspect_probe() {}

/// The probe's answer, waiting at most `CELL_ASPECT_COLLECT_WAIT` for it.
///
/// The thread is stopped either way: a terminal that never answers must not keep
/// polling stdin once the TUI owns it.
#[cfg(unix)]
pub(crate) fn probed_cell_aspect() -> Option<f32> {
    let probe = CELL_ASPECT_PROBE.get()?;
    let answer = match probe.answer.lock() {
        Ok(receiver) => receiver.recv_timeout(CELL_ASPECT_COLLECT_WAIT).ok(),
        Err(_) => None,
    };
    probe.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    answer.flatten()
}

/// Cell aspect as answered by the terminal itself, `None` on platforms that are
/// not asked (Windows consoles never answer these queries).
#[cfg(not(unix))]
pub(crate) fn probed_cell_aspect() -> Option<f32> {
    None
}

/// Ask the terminal for its text area in pixels (`CSI 14 t`) and in cells
/// (`CSI 18 t`); their quotient is the real cell aspect.
///
/// This is the only way to learn the font geometry of a VTE-based terminal
/// (GNOME Terminal, Tilix, Terminator): those answer `TIOCGWINSZ` with zero
/// `ws_xpixel`/`ws_ypixel`, so nothing else can measure them. VTE answers about
/// 100 ms after the query, on its own idle turn, which is why this runs in a
/// background thread with a generous deadline instead of inside the startup
/// path.
#[cfg(unix)]
fn query_cell_aspect(stop: &std::sync::atomic::AtomicBool) -> Option<f32> {
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
    while stdin_readable(deadline, stop) {
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
/// whole probe, and lets the banner stop the thread once it has painted.
#[cfg(unix)]
fn stdin_readable(deadline: std::time::Instant, stop: &std::sync::atomic::AtomicBool) -> bool {
    use std::os::fd::AsRawFd;

    if stop.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
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

pub(crate) fn env_cell_aspect() -> Option<f32> {
    let raw = std::env::var(CELL_ASPECT_ENV).ok()?;
    let value: f32 = raw.trim().parse().ok()?;
    CELL_ASPECT_RANGE.contains(&value).then_some(value)
}

#[cfg(not(windows))]
pub(crate) fn terminal_cell_aspect() -> Option<f32> {
    let size = crossterm::terminal::window_size().ok()?;
    // Inside a multiplexer a report that is exactly the multiplexer's default
    // cell is not a measurement of this font; treat it as "unknown" so the last
    // real measurement is used instead.
    if behind_multiplexer() && is_fabricated_cell(size.columns, size.rows, size.width, size.height)
    {
        return None;
    }
    measured_cell_aspect(size.columns, size.rows, size.width, size.height)
}

/// Windows consoles answer `ws_xpixel`/`ws_ypixel` with zero, so the fallback
/// is the rule there rather than a measurement.
#[cfg(windows)]
pub(crate) fn terminal_cell_aspect() -> Option<f32> {
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
#[cfg(test)]
mod tests {
    use super::*;

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

    /// tmux reports a hard-coded 16×32 cell for any client that never told it
    /// the real pixel size, both through `TIOCGWINSZ` and through the escape
    /// query. Painting a 10×26 font for that shape is 30 % too tall, which the
    /// reader sees as the head squeezed sideways.
    #[test]
    fn the_multiplexer_default_cell_is_recognised_as_no_measurement() {
        assert!(is_fabricated_cell(80, 24, 1280, 768));
        assert!(is_fabricated_cell(110, 40, 1760, 1280));
        // Anything else is a real report and stays a measurement.
        assert!(!is_fabricated_cell(80, 24, 800, 624));
        assert!(!is_fabricated_cell(80, 24, 1280, 624));
        assert!(!is_fabricated_cell(80, 24, 800, 768));
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
}
