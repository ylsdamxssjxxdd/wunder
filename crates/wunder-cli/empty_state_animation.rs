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
//! aspect the terminal reports; `terminal_geometry` owns that measurement and
//! the fallbacks (an explicit override, the last real measurement, then the
//! font's usual ratio). Painting 1:1 pixels into cells stretches the head
//! vertically by that same factor, which reads as a head squeezed horizontally.

#[path = "welcome_logo_art.rs"]
mod welcome_logo_art;

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::terminal_geometry::{
    behind_multiplexer, env_cell_aspect, probed_cell_aspect, terminal_cell_aspect,
    CELL_ASPECT_RANGE, FALLBACK_CELL_ASPECT,
};

/// 启动时欢迎头像从块底升进画面：新行在底部出现，已有的行被顶上去，
/// 整块高度不变，最先露出来的是头尖。
///
/// 约 0.7 s 升到位，之后渲染结果与静止画面逐字节一致，不再排任何帧。
const REVEAL_DURATION: Duration = Duration::from_millis(680);
/// 动画期间的刷新节奏，接近一帧一行的观感。结束之后不再安排帧。
pub(crate) const REVEAL_FRAME: Duration = Duration::from_millis(28);

static REVEAL_START: OnceLock<Instant> = OnceLock::new();

/// 开始计时。首帧之前调用一次，重复调用无副作用（线程切换不会重播）。
pub(crate) fn start_reveal() {
    REVEAL_START.get_or_init(Instant::now);
}

/// 动画是否还在进行；结束后调用方停止为此排帧。
pub(crate) fn reveal_active() -> bool {
    reveal_frame().is_some()
}

/// 动画帧号，静止画面时为 `None`。
///
/// 头像的相位是画出来的，不在转录的缓存键里，所以整帧复用会把画面冻在首帧
/// （和呼吸点、进度条那类随帧变化的样式同一个坑）。转录缓存拿这个值当键的
/// 一部分：动画期间每换一帧重建一次，停下来的那一帧正好是 `None`，重建出
/// 静止画面并就此稳定。
pub(crate) fn reveal_frame() -> Option<u64> {
    let elapsed = reveal_elapsed()?;
    Some(elapsed.as_millis() as u64 / REVEAL_FRAME.as_millis().max(1) as u64)
}

/// 归一化进度 `0.0..1.0`；计时未开始或已经结束都是 `None`（= 静止画面）。
fn reveal_phase() -> Option<f32> {
    let elapsed = reveal_elapsed()?;
    Some(elapsed.as_secs_f32() / REVEAL_DURATION.as_secs_f32())
}

/// 动画已经跑了多久；没开始或跑完了都是 `None`。
fn reveal_elapsed() -> Option<Duration> {
    let started = REVEAL_START.get()?;
    let elapsed = started.elapsed();
    (elapsed < REVEAL_DURATION).then_some(elapsed)
}

/// Below this many columns the art is dropped instead of being squeezed.
const MIN_COLUMNS: u16 = 24;
/// The art is 2 source rows per terminal cell; six cells still read as a head.
const MIN_ROWS: u16 = 6;
/// Luminance ramp used when the terminal refuses colour.
const SHADE_RAMP: [char; 10] = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];

/// Half-block rows for the welcome avatar, sized to the terminal width and to
/// the cell aspect the terminal reports.
///
/// Returns an empty vector when the terminal is too narrow: the banner then
/// degrades to the text header instead of painting a smeared blob.
pub(crate) fn logo_lines_for_cell(columns: u16, cell_aspect: f32) -> Vec<Line<'static>> {
    match fit(columns, cell_aspect) {
        Some((width, rows)) => render(width, rows, colours_enabled(), reveal_phase()),
        None => Vec::new(),
    }
}

/// The cell shape the welcome head is painted for.
///
/// Precedence, and why: an explicit `WUNDER_CLI_CELL_ASPECT` always wins; then
/// a measurement from the terminal in front of the user; then the last real
/// measurement remembered from an earlier run, which is the only honest answer
/// behind a multiplexer; and only then the font family's usual ratio. Every
/// step after the first is about *not* silently painting the head for a cell
/// shape nobody measured: that fallback is what a user reads as the head being
/// squeezed.
fn cell_aspect(home: &Path) -> f32 {
    static RESOLVED: OnceLock<f32> = OnceLock::new();
    *RESOLVED.get_or_init(|| {
        if let Some(value) = env_cell_aspect() {
            return value;
        }
        // A multiplexer only knows the client's *grid*. The pixel fields and the
        // `CSI 14 t`/`18 t` reports it returns are its own default cell (tmux:
        // 16×32) unless the client told it the real pixel size, and
        // `terminal_cell_aspect` filters exactly that default out. Asking the
        // multiplexer through the escape query only repeats the same answer, so
        // the probe is skipped there.
        let measured = if behind_multiplexer() {
            terminal_cell_aspect()
        } else {
            terminal_cell_aspect().or_else(probed_cell_aspect)
        };
        if let Some(value) = measured {
            remember_cell_aspect(home, value);
            return value;
        }
        remembered_cell_aspect(home).unwrap_or(FALLBACK_CELL_ASPECT)
    })
}

/// The geometry the welcome head is drawn at for a terminal of `columns`
/// columns: the cell aspect actually used, the head's width in cells and its
/// height in cells. `None` when the terminal is too narrow for the head.
///
/// `doctor` reports this so a head that looks wrong can be traced to the
/// geometry instead of guessed at.
pub(crate) fn avatar_geometry(columns: u16, home: &Path) -> Option<(f32, u16, u16)> {
    let aspect = cell_aspect(home);
    fit(columns, aspect).map(|(width, rows)| (aspect, width, rows))
}


/// `WUNDER_CLI_CELL_ASPECT` lets a terminal that reports nothing still get the
/// geometry right; an implausible value is ignored rather than obeyed.

/// Where the last *real* measurement is kept between runs.
///
/// A terminal multiplexer only knows the client's grid, never the font's shape:
/// tmux answers both `TIOCGWINSZ` and `CSI 14 t`/`18 t` with a hard-coded 16×32
/// cell, and a terminal that answers nothing at all has the same problem. In
/// both cases the last measurement taken from a real terminal is the only
/// honest geometry available, so it is remembered and reused instead of
/// guessing the font's ratio again.
const CELL_ASPECT_STATE_FILE: &str = "config/cli-terminal.json";

/// The last cell shape measured from a terminal that could actually report it.
///
/// Read once per process; a malformed or implausible file is ignored rather than
/// trusted, exactly like a malformed measurement.
fn remembered_cell_aspect(home: &Path) -> Option<f32> {
    let raw = std::fs::read_to_string(home.join(CELL_ASPECT_STATE_FILE)).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(raw.as_str()).ok()?;
    let value = parsed.get("cell_aspect")?.as_f64()? as f32;
    CELL_ASPECT_RANGE.contains(&value).then_some(value)
}

/// Remembers a measurement for the runs where the terminal cannot answer.
///
/// Best effort by design: a read-only home must not fail the startup path, and a
/// stale file only has to be no worse than the font guess it replaces.
fn remember_cell_aspect(home: &Path, aspect: f32) {
    if remembered_cell_aspect(home).is_some_and(|known| (known - aspect).abs() < 0.01) {
        return;
    }
    let path = home.join(CELL_ASPECT_STATE_FILE);
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    // Thousandths are finer than any measurement and keep the file readable.
    let aspect = (aspect * 1000.0).round() / 1000.0;
    let value = serde_json::json!({ "cell_aspect": aspect });
    let _ = std::fs::write(path, format!("{value}\n"));
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
    /// The cell shape is resolved once, at startup, so painting a frame never
    /// has to ask the terminal for its pixel size again. `home` is where the
    /// last real measurement is remembered for terminals and multiplexers that
    /// cannot answer.
    pub(crate) fn new(version: &str, workdir: &str, is_zh: bool, home: &Path) -> Self {
        Self {
            version: version.to_string(),
            workdir: workdir.to_string(),
            is_zh,
            cell_aspect: cell_aspect(home),
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

/// `phase` 为 `None` 时画静止的头像；`Some(0.0..1.0)` 让头像从块底升上来。
///
/// 整块高度从头到尾就是 `rows`，一帧里只画出已经升进画面的那一截，缺的行在
/// 它上面留空。于是新行出现在最下面、已有的行被顶上去，头尖先露头，再逐渐
/// 看全；块高不变，正文和输入框不会跟着上下跳。
fn render(width: u16, rows: u16, colours: bool, phase: Option<f32>) -> Vec<Line<'static>> {
    let row_count = usize::from(rows);
    let visible = match phase {
        Some(phase) => reveal_rows(row_count, phase),
        None => row_count,
    };
    let mut lines = vec![Line::default(); row_count.saturating_sub(visible)];
    for row in 0..visible {
        let mut spans = Vec::with_capacity(usize::from(width));
        for column in 0..usize::from(width) {
            let source_x = column * welcome_logo_art::SOURCE_WIDTH / usize::from(width);
            let source_y_top = row * welcome_logo_art::SOURCE_HEIGHT / row_count;
            let source_y_bottom =
                ((row + 1) * welcome_logo_art::SOURCE_HEIGHT / row_count).saturating_sub(1);
            let top = shade(source_x, source_y_top);
            let bottom = shade(source_x, source_y_bottom);
            spans.push(cell(top, bottom, colours));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// 指定进度下已经升进画面的行数：起点只露头尖一行，终点是整幅。
fn reveal_rows(rows: usize, phase: f32) -> usize {
    if rows == 0 {
        return 0;
    }
    let steps = rows.saturating_sub(1) as f32;
    let grown = (phase.clamp(0.0, 1.0) * steps).round() as usize;
    (grown + 1).min(rows)
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



    /// A terminal or multiplexer that cannot answer still gets the shape the
    /// last real measurement saw, instead of the font guess.
    #[test]
    fn a_remembered_measurement_survives_the_runs_that_cannot_measure() {
        let home = std::env::temp_dir().join(format!(
            "wunder-cli-cell-aspect-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(remembered_cell_aspect(&home), None, "nothing remembered yet");

        remember_cell_aspect(&home, 2.6);
        assert_eq!(remembered_cell_aspect(&home), Some(2.6));

        // A file that cannot be trusted is ignored, not obeyed.
        std::fs::write(home.join(CELL_ASPECT_STATE_FILE), "{\"cell_aspect\": 9.0}").ok();
        assert_eq!(remembered_cell_aspect(&home), None);
        std::fs::write(home.join(CELL_ASPECT_STATE_FILE), "not json").ok();
        assert_eq!(remembered_cell_aspect(&home), None);
        let _ = std::fs::remove_dir_all(&home);
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
        let lines = render(40, 17, true, None);
        let painted = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .filter(|span| span.content.as_ref() != " ")
            .count();
        assert!(painted > 100, "the head should cover a real area");
    }

    /// 从块底升起来：整块高度不变，新行在底部出现，旧行被顶上去。
    #[test]
    fn the_head_rises_with_the_tip_leading() {
        let rows = 17u16;
        let settled = render(40, rows, true, None);
        let mut previous = 0usize;
        for step in 0..=10 {
            let phase = step as f32 / 10.0;
            let frame = render(40, rows, true, Some(phase));
            assert_eq!(frame.len(), usize::from(rows), "整块高度不变，正文不跟着跳");
            let visible = reveal_rows(usize::from(rows), phase);
            assert!(visible >= previous, "已经露出来的行不会再退回去");
            assert!(visible <= usize::from(rows));
            previous = visible;
            // 没升上来的行留在上面且是空的；露出来的那一截贴着块底，
            // 并且与终态逐字节一致。
            for line in &frame[..usize::from(rows) - visible] {
                assert_eq!(line.width(), 0, "还没升上来的行是空的");
            }
            assert_same_rows(&frame[usize::from(rows) - visible..], &settled[..visible]);
        }
        assert_eq!(previous, usize::from(rows));
    }

    /// 起点只有头尖一行，它贴在块底；之后整幅往上浮，头尖先露头再看全。
    #[test]
    fn the_first_visible_row_is_the_tip_of_the_head() {
        let rows = 17u16;
        let settled = render(40, rows, true, None);
        let first = render(40, rows, true, Some(0.0));
        assert_eq!(first.len(), usize::from(rows));
        for line in &first[..usize::from(rows) - 1] {
            assert_eq!(line.width(), 0, "头尖上面全是空的");
        }
        let tip = &first[usize::from(rows) - 1];
        assert!(tip.width() > 0, "the tip is painted");
        assert_same_rows(std::slice::from_ref(tip), &settled[..1]);
        assert_eq!(reveal_rows(usize::from(rows), 0.0), 1);
        assert_eq!(reveal_rows(usize::from(rows), 1.0), usize::from(rows));
    }

    /// 动画停下来的那一帧必须就是静止画面，否则欢迎块会抖一下。
    #[test]
    fn the_settled_frame_is_the_still_picture() {
        let settled = render(40, 17, true, None);
        assert_same_rows(&render(40, 17, true, Some(1.0)), &settled);
    }

    fn assert_same_rows(left: &[Line<'static>], right: &[Line<'static>]) {
        assert_eq!(left.len(), right.len());
        for (left, right) in left.iter().zip(right) {
            assert_eq!(left.width(), right.width());
            assert_eq!(left.spans.len(), right.spans.len());
            for (left, right) in left.spans.iter().zip(&right.spans) {
                assert_eq!(left.content, right.content);
                assert_eq!(left.style, right.style);
            }
        }
    }

    #[test]
    fn the_no_colour_ramp_never_emits_escape_styling() {
        let lines = render(40, 17, false, None);
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
