//! Braille raster: samples the wheel twice across and four times down each cell, then
//! collapses the lit dots into one glyph and one colour per cell.

use super::geometry::{surface, Sample};
use super::lighting::{Lighting, Shade};
use super::paths::{gear, GEAR_EXTENT};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Dots inside one braille cell.
const CELL_DOTS_X: usize = 2;
const CELL_DOTS_Y: usize = 4;

/// Bit for dot (x, y) of a cell, matching the Unicode braille block layout.
const DOT_BITS: [[u8; CELL_DOTS_Y]; CELL_DOTS_X] =
    [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

const FIRST_BRAILLE: u32 = 0x2800;

/// Subpixel coverage required before a dot is drawn at all.
const DOT_ON: f32 = 0.40;

/// Largest stage the renderer will draw, so a wall-sized terminal cannot inflate the
/// per-frame sample count without bound.
pub(crate) const MAX_COLUMNS: u16 = 56;
pub(crate) const MAX_ROWS: u16 = 14;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Cell {
    pub(crate) dots: u8,
    pub(crate) rgb: [u8; 3],
    pub(crate) shade: Shade,
}

impl Cell {
    const EMPTY: Cell = Cell {
        dots: 0,
        rgb: [0, 0, 0],
        shade: Shade::Base,
    };

    pub(crate) fn is_empty(&self) -> bool {
        self.dots == 0
    }

    pub(crate) fn glyph(&self) -> char {
        if self.is_empty() {
            return ' ';
        }
        char::from_u32(FIRST_BRAILLE + u32::from(self.dots)).unwrap_or(' ')
    }
}

#[derive(Default)]
pub(crate) struct Renderer {
    /// Reused scratch grid; the stage size only changes on resize.
    cells: Vec<Cell>,
}

impl Renderer {
    /// Draw the wheel turned by `angle` into a `width` x `height` cell grid.
    ///
    /// Subpixels are treated as square: a terminal row is about twice as wide as it is
    /// tall in columns, and splitting the row into four dot rows makes them meet.
    pub(crate) fn frame(
        &mut self,
        width: u16,
        height: u16,
        angle: f32,
        light: &Lighting,
    ) -> Vec<Cell> {
        if width == 0 || height == 0 {
            return Vec::new();
        }
        let width = usize::from(width);
        let height = usize::from(height);
        let wheel = gear(angle);
        let dots_x = width * CELL_DOTS_X;
        let dots_y = height * CELL_DOTS_Y;
        let radius = (dots_x.min(dots_y) as f32 * 0.5).floor();
        let scale = radius / GEAR_EXTENT;

        self.cells.clear();
        self.cells.resize(width * height, Cell::EMPTY);
        if scale <= 0.0 {
            return Vec::new();
        }
        let center = [(dots_x as f32 - 1.0) * 0.5, (dots_y as f32 - 1.0) * 0.5];

        for (index, cell) in self.cells.iter_mut().enumerate() {
            let origin_x = (index % width) * CELL_DOTS_X;
            let origin_y = (index / width) * CELL_DOTS_Y;
            let mut acc = Acc::default();
            for dot_y in 0..CELL_DOTS_Y {
                for dot_x in 0..CELL_DOTS_X {
                    let point = [
                        (origin_x + dot_x) as f32 - center[0],
                        (origin_y + dot_y) as f32 - center[1],
                    ];
                    let Some(sample) = surface(&wheel, [point[0] / scale, point[1] / scale]) else {
                        continue;
                    };
                    if sample.coverage < DOT_ON {
                        continue;
                    }
                    acc.add(DOT_BITS[dot_x][dot_y], &sample, light);
                }
            }
            *cell = match acc.lit {
                0 => Cell::EMPTY,
                _ => Cell {
                    dots: acc.dots,
                    rgb: acc.mean_rgb(),
                    shade: Shade::of(acc.strongest),
                },
            };
        }
        self.cells.clone()
    }

    /// Render a grid of cells into ratatui lines, one span run per style change.
    pub(crate) fn lines(cells: &[Cell], width: usize, coloured: bool) -> Vec<Line<'static>> {
        cells
            .chunks(width.max(1))
            .map(|row| {
                let mut spans = Vec::new();
                let mut run = String::new();
                let mut style: Option<Style> = None;
                for cell in row {
                    let target = (!cell.is_empty()).then(|| cell.style(coloured));
                    if target != style && !run.is_empty() {
                        spans.push(Span::styled(
                            std::mem::take(&mut run),
                            style.take().unwrap_or_default(),
                        ));
                    }
                    style = target;
                    run.push(cell.glyph());
                }
                if !run.is_empty() {
                    spans.push(Span::styled(run, style.unwrap_or_default()));
                }
                Line::from(spans)
            })
            .collect()
    }
}

impl Cell {
    /// Truecolour paints the lit shade; a colourless terminal only has weight to work with.
    fn style(&self, coloured: bool) -> Style {
        if coloured {
            return Style::default().fg(Color::Rgb(self.rgb[0], self.rgb[1], self.rgb[2]));
        }
        match self.shade {
            Shade::Dim => Style::default().add_modifier(Modifier::DIM),
            Shade::Base => Style::default(),
            Shade::Bright => Style::default().add_modifier(Modifier::BOLD),
        }
    }
}

/// Running totals for one cell's dots.
#[derive(Default)]
struct Acc {
    dots: u8,
    lit: usize,
    sum: [f32; 3],
    strongest: f32,
}

impl Acc {
    fn add(&mut self, bit: u8, sample: &Sample, light: &Lighting) {
        let lit = light.shade(sample);
        self.dots |= bit;
        self.sum[0] += f32::from(lit.rgb[0]);
        self.sum[1] += f32::from(lit.rgb[1]);
        self.sum[2] += f32::from(lit.rgb[2]);
        self.strongest = self.strongest.max(lit.intensity);
        self.lit += 1;
    }

    fn mean_rgb(&self) -> [u8; 3] {
        let n = self.lit.max(1) as f32;
        [
            (self.sum[0] / n).round() as u8,
            (self.sum[1] / n).round() as u8,
            (self.sum[2] / n).round() as u8,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::empty_state_animation::lighting::Shade;

    fn light() -> Lighting {
        Lighting::terminal([210, 221, 235], [15, 20, 37])
    }

    fn frame(width: usize, height: usize, angle: f32) -> Vec<Cell> {
        Renderer::default().frame(
            u16::try_from(width).unwrap(),
            u16::try_from(height).unwrap(),
            angle,
            &light(),
        )
    }

    #[test]
    fn dot_bits_match_the_unicode_braille_layout() {
        // Dots 1..3 run down the left column, 4..6 down the right, 7..8 are the bottom pair.
        assert_eq!(DOT_BITS[0][0], 0x01);
        assert_eq!(DOT_BITS[0][3], 0x40);
        assert_eq!(DOT_BITS[1][0], 0x08);
        assert_eq!(DOT_BITS[1][3], 0x80);
    }

    #[test]
    fn glyph_is_the_braille_codepoint_or_a_space() {
        assert_eq!(Cell::EMPTY.glyph(), ' ');
        assert_eq!(
            Cell {
                dots: 0xff,
                ..Cell::EMPTY
            }
            .glyph(),
            char::from_u32(FIRST_BRAILLE + 0xff).expect("braille U+28FF is a scalar value")
        );
    }

    #[test]
    fn every_row_of_the_wheel_has_ink_and_mirrors_across_the_axis() {
        let cells = frame(30, 15, 0.0);
        assert_eq!(cells.len(), 30 * 15);
        for row in 0..15 {
            let used = (0..30)
                .filter(|col| !cells[row * 30 + col].is_empty())
                .count();
            assert!(used > 0, "row {row} is blank");
            for col in 0..15 {
                // Mirroring swaps the two dot columns of each cell, not the cells alone.
                assert_eq!(
                    mirrored(cells[row * 30 + col].dots),
                    cells[row * 30 + (29 - col)].dots,
                    "row {row} must mirror about the vertical axis"
                );
            }
        }
    }

    fn mirrored(dots: u8) -> u8 {
        let mut out = 0u8;
        for dot_y in 0..CELL_DOTS_Y {
            for dot_x in 0..CELL_DOTS_X {
                if dots & DOT_BITS[dot_x][dot_y] != 0 {
                    out |= DOT_BITS[1 - dot_x][dot_y];
                }
            }
        }
        out
    }

    #[test]
    fn a_round_wheel_leaves_the_corners_empty() {
        let cells = frame(24, 12, 0.0);
        assert!(cells[0].is_empty());
        assert!(cells[23].is_empty());
        assert!(cells[11 * 24].is_empty());
        assert!(cells[11 * 24 + 23].is_empty());
    }

    #[test]
    fn turning_the_wheel_changes_the_ink() {
        let still = frame(28, 7, 0.0);
        let turned = frame(28, 7, 0.35);
        assert_ne!(
            still.iter().map(|c| c.dots).collect::<Vec<_>>(),
            turned.iter().map(|c| c.dots).collect::<Vec<_>>()
        );
    }

    #[test]
    fn degenerate_grids_render_nothing() {
        assert!(frame(0, 6, 0.0).is_empty());
        assert!(frame(6, 0, 0.0).is_empty());
    }

    #[test]
    fn lines_carry_one_row_per_grid_row_and_fit_the_width() {
        let width = 32usize;
        let cells = frame(width, 8, 0.0);
        let lines = Renderer::lines(&cells, width, true);
        assert_eq!(lines.len(), 8);
        for line in &lines {
            let used: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(used <= width, "line overflows: {used}");
        }
        assert!(lines.iter().any(|line| line.spans.len() > 1));
    }

    #[test]
    fn colourless_ramp_falls_back_to_reset_so_modifiers_carry_the_weight() {
        let cells = frame(24, 6, 0.0);
        let shaded = cells.iter().filter(|cell| !cell.is_empty()).count();
        assert!(shaded > 20, "expected ink to survive: {shaded}");
        assert!(cells.iter().any(|cell| cell.shade == Shade::Bright));
        let lines = Renderer::lines(&cells, 24, false);
        assert!(lines
            .iter()
            .all(|line| line.spans.iter().all(|span| match span.style.fg {
                Some(Color::Reset) | None => true,
                Some(other) => panic!("unexpected colour {other:?} in mono mode"),
            })));
    }
}
