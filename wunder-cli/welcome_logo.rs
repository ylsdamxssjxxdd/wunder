use image::imageops::{crop_imm, resize, FilterType};
use image::{ImageFormat, Rgba, RgbaImage};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use std::fmt::Write as _;

const LOGO_SOURCE_BYTES: &[u8] = include_bytes!("../images/eva01-head.ico");
const FALLBACK_WORDMARK: &str = "wunder-cli";
const ASCII_DENSITY: &[u8] = b"@#S%?*+;:,. ";
const TERMINAL_PADDING: u16 = 4;
const DEFAULT_RENDER_WIDTH: u16 = 44;
const MAX_RENDER_WIDTH: u16 = 56;
const MIN_RENDER_WIDTH: u16 = 18;
const MIN_VISIBLE_ALPHA: u8 = 24;
const ASCII_HEIGHT_RATIO: f32 = 0.50;
const COLOR_QUANT_STEP: u8 = 16;

#[derive(Debug, Clone)]
pub(crate) struct RenderedWelcomeLogo {
    plain_text: String,
    terminal_text: String,
    tui_lines: Vec<Line<'static>>,
}

impl RenderedWelcomeLogo {
    pub(crate) fn plain_text(&self) -> &str {
        self.plain_text.as_str()
    }

    pub(crate) fn terminal_text(&self) -> &str {
        self.terminal_text.as_str()
    }

    pub(crate) fn tui_lines(&self) -> Vec<Line<'static>> {
        self.tui_lines.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StyledAsciiCell {
    ch: char,
    rgb: Option<[u8; 3]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StyledAsciiRun {
    text: String,
    rgb: Option<[u8; 3]>,
}

pub(crate) fn render_for_terminal() -> RenderedWelcomeLogo {
    let width = crossterm::terminal::size()
        .map(|(columns, _)| columns.saturating_sub(TERMINAL_PADDING))
        .unwrap_or(DEFAULT_RENDER_WIDTH);
    render_for_width(width)
}

pub(crate) fn render_for_width(max_width: u16) -> RenderedWelcomeLogo {
    let width = max_width.min(MAX_RENDER_WIDTH);
    if width < MIN_RENDER_WIDTH {
        return fallback_logo();
    }

    build_rendered_logo(width).unwrap_or_else(|_| fallback_logo())
}

fn build_rendered_logo(target_width: u16) -> image::ImageResult<RenderedWelcomeLogo> {
    let icon = image::load_from_memory_with_format(LOGO_SOURCE_BYTES, ImageFormat::Ico)?;
    let rgba = icon.into_rgba8();
    let cropped = crop_alpha_bounds(rgba.clone()).unwrap_or(rgba);
    let target_height = scaled_height(&cropped, target_width);
    let resized = resize(
        &cropped,
        u32::from(target_width),
        target_height,
        FilterType::Nearest,
    );
    let styled_lines = raster_to_runs(&resized);
    if styled_lines.is_empty() {
        return Ok(fallback_logo());
    }
    Ok(compose_logo(styled_lines))
}

fn fallback_logo() -> RenderedWelcomeLogo {
    let text = FALLBACK_WORDMARK.to_string();
    RenderedWelcomeLogo {
        plain_text: text.clone(),
        terminal_text: text.clone(),
        tui_lines: vec![Line::from(Span::raw(text))],
    }
}

fn crop_alpha_bounds(image: RgbaImage) -> Option<RgbaImage> {
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut found = false;

    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel.0[3] < MIN_VISIBLE_ALPHA {
            continue;
        }
        found = true;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    if !found {
        return None;
    }

    Some(
        crop_imm(
            &image,
            min_x,
            min_y,
            max_x.saturating_sub(min_x).saturating_add(1),
            max_y.saturating_sub(min_y).saturating_add(1),
        )
        .to_image(),
    )
}

fn scaled_height(image: &RgbaImage, target_width: u16) -> u32 {
    // Pixel-art icons look better with nearest scaling and a slightly compressed Y axis.
    let aspect_ratio = image.height() as f32 / image.width().max(1) as f32;
    (aspect_ratio * f32::from(target_width) * ASCII_HEIGHT_RATIO)
        .round()
        .max(1.0) as u32
}

fn raster_to_runs(raster: &RgbaImage) -> Vec<Vec<StyledAsciiRun>> {
    let Some((min_darkness, max_darkness)) = darkness_range(raster) else {
        return Vec::new();
    };

    let mut lines = Vec::with_capacity(raster.height() as usize);
    for y in 0..raster.height() {
        let mut cells = Vec::with_capacity(raster.width() as usize);
        for x in 0..raster.width() {
            let pixel = raster.get_pixel(x, y);
            cells.push(pixel_to_cell(pixel, min_darkness, max_darkness));
        }

        let Some(last_visible_index) = cells.iter().rposition(|cell| cell.ch != ' ') else {
            continue;
        };
        lines.push(group_cells_into_runs(&cells[..=last_visible_index]));
    }

    lines
}

fn pixel_to_cell(pixel: &Rgba<u8>, min_darkness: f32, max_darkness: f32) -> StyledAsciiCell {
    let Some(darkness) = pixel_darkness(pixel) else {
        return StyledAsciiCell { ch: ' ', rgb: None };
    };

    StyledAsciiCell {
        ch: darkness_to_char(darkness, min_darkness, max_darkness),
        rgb: Some(boost_eva_palette(pixel)),
    }
}

fn group_cells_into_runs(cells: &[StyledAsciiCell]) -> Vec<StyledAsciiRun> {
    let mut runs = Vec::new();
    let mut current_rgb = None;
    let mut current_text = String::new();

    for cell in cells {
        if cell.rgb != current_rgb && !current_text.is_empty() {
            runs.push(StyledAsciiRun {
                text: std::mem::take(&mut current_text),
                rgb: current_rgb,
            });
        }
        current_rgb = cell.rgb;
        current_text.push(cell.ch);
    }

    if !current_text.is_empty() {
        runs.push(StyledAsciiRun {
            text: current_text,
            rgb: current_rgb,
        });
    }

    runs
}

fn darkness_range(raster: &RgbaImage) -> Option<(f32, f32)> {
    let mut min_darkness = f32::MAX;
    let mut max_darkness = f32::MIN;
    for pixel in raster.pixels() {
        let Some(darkness) = pixel_darkness(pixel) else {
            continue;
        };
        min_darkness = min_darkness.min(darkness);
        max_darkness = max_darkness.max(darkness);
    }

    if min_darkness == f32::MAX || max_darkness == f32::MIN {
        return None;
    }
    Some((min_darkness, max_darkness))
}

fn pixel_darkness(pixel: &Rgba<u8>) -> Option<f32> {
    let [red, green, blue, alpha] = pixel.0;
    if alpha < MIN_VISIBLE_ALPHA {
        return None;
    }

    let alpha_weight = f32::from(alpha) / 255.0;
    let luminance =
        (0.2126 * f32::from(red) + 0.7152 * f32::from(green) + 0.0722 * f32::from(blue)) / 255.0;
    Some((1.0 - luminance) * alpha_weight)
}

fn darkness_to_char(darkness: f32, min_darkness: f32, max_darkness: f32) -> char {
    let range = (max_darkness - min_darkness).max(f32::EPSILON);
    let normalized = ((darkness - min_darkness) / range).clamp(0.0, 1.0);
    let index = (normalized * (ASCII_DENSITY.len().saturating_sub(1)) as f32).round() as usize;
    char::from(ASCII_DENSITY[index.min(ASCII_DENSITY.len().saturating_sub(1))])
}

fn boost_eva_palette(pixel: &Rgba<u8>) -> [u8; 3] {
    let [red, green, blue, _] = pixel.0;
    let luminance =
        (0.2126 * f32::from(red) + 0.7152 * f32::from(green) + 0.0722 * f32::from(blue)) / 255.0;

    let adjusted = if luminance < 0.12 {
        [78, 56, 112]
    } else if green > red.saturating_add(20) && green > blue.saturating_add(8) {
        [
            red.saturating_mul(3) / 4,
            green.saturating_add(28),
            blue.saturating_mul(2) / 3,
        ]
    } else if red > green.saturating_add(40) && red > blue.saturating_add(32) {
        [
            red.saturating_add(20),
            green.saturating_mul(3) / 5,
            blue / 2,
        ]
    } else if red > green && blue > green {
        [
            red.saturating_add(10),
            green.saturating_mul(9) / 10,
            blue.saturating_add(14),
        ]
    } else {
        [red, green, blue]
    };

    [
        quantize_channel(adjusted[0]),
        quantize_channel(adjusted[1]),
        quantize_channel(adjusted[2]),
    ]
}

fn quantize_channel(value: u8) -> u8 {
    let step = u16::from(COLOR_QUANT_STEP);
    let rounded = ((u16::from(value) + step / 2) / step) * step;
    rounded.min(u16::from(u8::MAX)) as u8
}

fn compose_logo(styled_lines: Vec<Vec<StyledAsciiRun>>) -> RenderedWelcomeLogo {
    let plain_text = styled_lines
        .iter()
        .map(|line| line.iter().map(|run| run.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    let terminal_text = if ansi_colors_enabled() {
        styled_lines
            .iter()
            .map(|line| render_terminal_line(line.as_slice()))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        plain_text.clone()
    };

    let tui_lines = styled_lines
        .into_iter()
        .map(|line| {
            let spans = line
                .into_iter()
                .map(|run| match run.rgb {
                    Some([red, green, blue]) => {
                        Span::styled(run.text, Style::default().fg(Color::Rgb(red, green, blue)))
                    }
                    None => Span::raw(run.text),
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect::<Vec<_>>();

    RenderedWelcomeLogo {
        plain_text,
        terminal_text,
        tui_lines,
    }
}

fn ansi_colors_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

fn render_terminal_line(line: &[StyledAsciiRun]) -> String {
    let mut rendered = String::new();
    for run in line {
        match run.rgb {
            Some([red, green, blue]) => {
                let _ = write!(rendered, "\x1b[38;2;{red};{green};{blue}m");
                rendered.push_str(run.text.as_str());
            }
            None => {
                rendered.push_str("\x1b[0m");
                rendered.push_str(run.text.as_str());
            }
        }
    }
    rendered.push_str("\x1b[0m");
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_logo_respects_width_hint() {
        let logo = render_for_width(36);
        assert_ne!(logo.plain_text(), FALLBACK_WORDMARK);
        assert!(logo.plain_text().lines().count() > 3);
        assert!(logo
            .plain_text()
            .lines()
            .all(|line| line.chars().count() <= 36));
    }

    #[test]
    fn narrow_width_uses_wordmark_fallback() {
        let logo = render_for_width(10);
        assert_eq!(logo.plain_text(), FALLBACK_WORDMARK);
    }

    #[test]
    fn tui_logo_contains_multiple_color_spans() {
        let logo = render_for_width(40);
        let color_count = logo
            .tui_lines()
            .into_iter()
            .flat_map(|line| line.spans)
            .filter_map(|span| match span.style.fg {
                Some(Color::Rgb(red, green, blue)) => Some((red, green, blue)),
                _ => None,
            })
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert!(color_count >= 3);
    }
}
