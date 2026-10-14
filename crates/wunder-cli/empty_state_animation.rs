//! Static EVA welcome logo rendered from compact, generated half-block colour data.

#[path = "welcome_logo_art.rs"]
mod welcome_logo_art;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub(crate) const BRAND_MARK: char = '\u{2699}';

/// Rows the main layout always spends: transcript, status and composer.
const MIN_STANDING_ROWS: u16 = 14;

pub(crate) fn brand_name(is_zh: bool) -> &'static str {
    if is_zh {
        "wunder 舵机"
    } else {
        "wunder servo"
    }
}

pub(crate) struct Pose {
    pub(crate) width: u16,
    pub(crate) lines: Vec<Line<'static>>,
}

#[derive(Default)]
pub(crate) struct WelcomeLogo {
    eligible: bool,
}

impl WelcomeLogo {
    pub(crate) fn is_eligible(&self) -> bool {
        self.eligible
    }

    pub(crate) fn start_fresh(&mut self) {
        self.eligible = true;
    }

    pub(crate) fn dismiss(&mut self) {
        self.eligible = false;
    }

    pub(crate) fn reserved_rows(&self, screen: Rect) -> u16 {
        if self.eligible {
            logo_fit(screen).map_or(0, |(_, height)| height)
        } else {
            0
        }
    }

    pub(crate) fn paint(&mut self, screen: Rect) -> Option<Pose> {
        let (width, height) = logo_fit(screen)?;
        self.eligible = false;
        Some(Pose {
            width,
            lines: paint_frame(width, height),
        })
    }
}

fn logo_fit(screen: Rect) -> Option<(u16, u16)> {
    let max_width = screen
        .width
        .saturating_sub(2)
        .min(welcome_logo_art::SOURCE_WIDTH as u16);
    let max_height = screen
        .height
        .saturating_sub(MIN_STANDING_ROWS)
        .min((welcome_logo_art::SOURCE_HEIGHT / 2) as u16);
    let height = max_height.min(
        (u32::from(max_width) * (welcome_logo_art::SOURCE_HEIGHT / 2) as u32
            / welcome_logo_art::SOURCE_WIDTH as u32) as u16,
    );
    let width = max_width.min(
        (u32::from(height) * welcome_logo_art::SOURCE_WIDTH as u32
            / (welcome_logo_art::SOURCE_HEIGHT / 2) as u32) as u16,
    );
    (width >= 20 && height >= 6).then_some((width, height))
}

fn paint_frame(width: u16, height: u16) -> Vec<Line<'static>> {
    let colours_enabled = std::env::var_os("NO_COLOR").is_none();
    let mut lines = Vec::with_capacity(usize::from(height));
    for row in 0..usize::from(height) {
        let mut spans = Vec::with_capacity(usize::from(width));
        for column in 0..usize::from(width) {
            let source_x = column * welcome_logo_art::SOURCE_WIDTH / usize::from(width);
            let source_y_top = row * welcome_logo_art::SOURCE_HEIGHT / usize::from(height);
            let source_y_bottom = ((row + 1) * welcome_logo_art::SOURCE_HEIGHT
                / usize::from(height))
            .saturating_sub(1);
            let top = logo_colour(source_x, source_y_top);
            let bottom = logo_colour(source_x, source_y_bottom);
            spans.push(if !colours_enabled {
                if top.is_some() || bottom.is_some() {
                    Span::raw("▀")
                } else {
                    Span::raw(" ")
                }
            } else {
                match (top, bottom) {
                    (None, None) => Span::raw(" "),
                    (Some(foreground), None) => Span::styled("▀", Style::default().fg(foreground)),
                    (None, Some(foreground)) => Span::styled("▄", Style::default().fg(foreground)),
                    (Some(foreground), Some(background)) => {
                        Span::styled("▀", Style::default().fg(foreground).bg(background))
                    }
                }
            });
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn logo_colour(x: usize, y: usize) -> Option<Color> {
    let row = welcome_logo_art::ROWS[y / 2].as_bytes();
    let packed = (hex_nibble(row[x * 2]) << 4) | hex_nibble(row[x * 2 + 1]);
    let index = if y % 2 == 0 {
        packed >> 4
    } else {
        packed & 0x0f
    };
    (index != 15).then(|| {
        let rgb = welcome_logo_art::PALETTE[usize::from(index)];
        Color::Rgb(rgb[0], rgb[1], rgb[2])
    })
}

fn hex_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => 0,
    }
}

/// Product name shown beside the welcome logo.
pub(crate) fn header_lines(version: &str, workdir: &str, is_zh: bool) -> Vec<Line<'static>> {
    let (label, hints) = if is_zh {
        ("工作目录", "/help 查看命令 · @ 提及文件 · ! 直接执行")
    } else {
        (
            "workspace",
            "/help for commands · @ to mention a file · ! to run a command",
        )
    };
    vec![
        Line::from(vec![
            Span::styled(
                format!("{BRAND_MARK} "),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} (v{version})", brand_name(is_zh)),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("   {label} "),
                Style::default().add_modifier(Modifier::DIM),
            ),
            Span::styled(
                workdir.to_string(),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ]),
        Line::from(Span::styled(
            format!("   {hints}"),
            Style::default().add_modifier(Modifier::DIM),
        )),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 100, 40);

    #[test]
    fn fresh_logo_paints_once_without_scheduling_animation() {
        let mut logo = WelcomeLogo::default();
        logo.start_fresh();
        assert!(logo.reserved_rows(SCREEN) > 0);
        let pose = logo.paint(SCREEN).expect("logo");
        assert!(!pose.lines.is_empty());
        assert!(!logo.is_eligible());
        assert_eq!(logo.reserved_rows(SCREEN), 0);
    }

    #[test]
    fn logo_without_room_stays_armed_until_resize() {
        let mut logo = WelcomeLogo::default();
        logo.start_fresh();
        let tight = Rect::new(0, 0, 12, 40);
        assert!(logo.paint(tight).is_none());
        assert!(logo.is_eligible());
        assert_eq!(logo.reserved_rows(tight), 0);
        assert!(logo.reserved_rows(SCREEN) > 0);
    }

    #[test]
    fn logo_pose_is_clamped_to_the_reserved_band() {
        let mut logo = WelcomeLogo::default();
        logo.start_fresh();
        let rows = logo.reserved_rows(SCREEN);
        let pose = logo.paint(SCREEN).expect("stage");
        assert_eq!(pose.lines.len(), usize::from(rows));
        for line in &pose.lines {
            assert!(line.spans.len() <= usize::from(pose.width));
        }
    }

    #[test]
    fn header_carries_the_mark_version_path_and_hints() {
        let lines = header_lines("0.4.0", "/tmp/work", true);
        let first: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(first.contains(BRAND_MARK));
        assert!(first.contains("v0.4.0"));
        assert!(first.contains("舵机"));
        let second: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(second.contains("/tmp/work"));
        let third: String = lines[2].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(third.contains("/help") && third.contains('@') && third.contains('!'));
        let english: String = header_lines("0.4.0", "/tmp/work", false)[2]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(english.contains("/help"));
    }
}
