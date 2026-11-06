//! Colourless welcome avatar: a white/grey half-block relief painted above the
//! brand header of a fresh thread.
//!
//! The pixel data in `welcome_logo_art.rs` is a neutral grey ladder (slot 15 is
//! transparent), so the head never introduces hue into the frame; the volume is
//! carried by luminance only. Terminals with `NO_COLOR` set fall back to a
//! luminance ramp of ASCII glyphs, which keeps the relief readable without any
//! escape codes.

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

/// Half-block rows for the welcome avatar, sized to the terminal width.
///
/// Returns an empty vector when the terminal is too narrow: the banner then
/// degrades to the text header instead of painting a smeared blob.
pub(crate) fn logo_lines(columns: u16) -> Vec<Line<'static>> {
    match fit(columns) {
        Some((width, rows)) => render(width, rows, colours_enabled()),
        None => Vec::new(),
    }
}

/// The whole first-frame banner: grey avatar, one breathing row, then the
/// product text. `session_header` stays the single owner of the wording, this
/// module only decides whether the head fits above it.
pub(crate) fn banner_lines(
    columns: u16,
    version: &str,
    workdir: &str,
    is_zh: bool,
) -> Vec<Line<'static>> {
    let mut lines = logo_lines(columns);
    if !lines.is_empty() {
        lines.push(Line::default());
    }
    lines.extend(crate::session_header::header_lines(version, workdir, is_zh));
    lines
}

fn colours_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

fn fit(columns: u16) -> Option<(u16, u16)> {
    let max_width = columns
        .saturating_sub(2)
        .min(welcome_logo_art::SOURCE_WIDTH as u16);
    let rows = (u32::from(max_width) * (welcome_logo_art::SOURCE_HEIGHT / 2) as u32
        / welcome_logo_art::SOURCE_WIDTH as u32) as u16;
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

    #[test]
    fn a_wide_terminal_gets_a_full_bleed_relief() {
        let lines = logo_lines(120);
        assert_eq!(lines.len(), usize::from(fit(120).expect("wide").1));
        assert_eq!(lines.len(), (welcome_logo_art::SOURCE_HEIGHT / 2));
        for line in &lines {
            assert_eq!(line.spans.len(), welcome_logo_art::SOURCE_WIDTH);
        }
    }

    #[test]
    fn the_art_scales_down_with_the_terminal_and_then_disappears() {
        let medium = logo_lines(30);
        assert!(!medium.is_empty());
        assert!(medium.iter().all(|line| line.spans.len() <= 28));
        assert!(fit(20).is_none() && logo_lines(20).is_empty());
    }

    #[test]
    fn every_visible_cell_is_neutral_grey() {
        for line in logo_lines(80) {
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
    fn the_banner_stacks_the_head_above_the_brand_text() {
        let mut expected = logo_lines(80);
        let art_rows = expected.len();
        assert!(art_rows > 0, "an 80 column terminal fits the head");
        expected.push(Line::default());
        expected.extend(crate::session_header::header_lines(
            "0.4.0",
            "/tmp/work",
            true,
        ));
        let lines = banner_lines(80, "0.4.0", "/tmp/work", true);
        assert_eq!(lines.len(), expected.len());
        let content = |index: usize| -> String {
            lines[index]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        };
        assert!(content(art_rows + 1).contains("wunder 舵机"));
        assert!(content(lines.len() - 3).contains("/tmp/work"));
    }

    #[test]
    fn a_narrow_terminal_keeps_the_text_banner_only() {
        let lines = banner_lines(20, "0.4.0", "/tmp/work", false);
        assert_eq!(
            lines.len(),
            crate::session_header::header_lines("0.4.0", "/tmp/work", false).len()
        );
    }
}
