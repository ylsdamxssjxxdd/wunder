//! 舵机 welcome banner: an analytic Rust-style gear, rendered as braille dots, that turns
//! through a damped sweep and then holds with its W upright.
//!
//! Structured like the reference implementation: `paths` carries the shape, `geometry`
//! samples it, `lighting` gives it a material from the terminal's own colours, `renderer`
//! quantises it into 2x4 dot cells, `sequence` times the sweep and `policy` decides what
//! this terminal may show. This file holds the pose and its lifecycle.

mod geometry;
mod lighting;
mod paths;
mod policy;
mod renderer;
mod sequence;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::time::{Duration, Instant};

use lighting::Lighting;
pub(crate) use policy::Presentation;
use policy::{colours_enabled, fit, motion_reduced, terminal_material};
use renderer::Renderer;
use sequence::{SETTLED_ANGLE, SPIN_DURATION};

/// Redraw cadence while the wheel is still turning.
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(50);

pub(crate) const BRAND_MARK: char = '\u{2699}';

/// Rows the main layout always spends: a transcript floor of 8, the working line and the
/// composer. The banner may only take what is left of the terminal on top of these.
const MIN_STANDING_ROWS: u16 = 14;

/// Product name shown in the banner. 舵机 is the local servo that turns orders into motion.
pub(crate) fn brand_name(is_zh: bool) -> &'static str {
    if is_zh {
        "wunder 舵机"
    } else {
        "wunder servo"
    }
}

/// One painted pose of the wheel.
pub(crate) struct Pose {
    /// Cells across, so the caller can centre the band it reserved.
    pub(crate) width: u16,
    pub(crate) lines: Vec<Line<'static>>,
}

#[derive(Default)]
pub(crate) struct EmptyStateAnimation {
    eligible: bool,
    spin_elapsed: Duration,
    last_frame: Option<Instant>,
    renderer: Renderer,
}

impl EmptyStateAnimation {
    pub(crate) fn is_eligible(&self) -> bool {
        self.eligible
    }

    /// Arm the sweep for a fresh thread.
    pub(crate) fn start_fresh(&mut self) {
        self.eligible = true;
        self.spin_elapsed = Duration::ZERO;
        self.last_frame = None;
    }

    /// Stop drawing and forget the clock, so a later `start_fresh` sweeps again.
    pub(crate) fn dismiss(&mut self) {
        self.eligible = false;
        self.spin_elapsed = Duration::ZERO;
        self.last_frame = None;
    }

    /// Rows the banner wants from a terminal `screen` tall, or `0` when it has no
    /// business being drawn.
    pub(crate) fn reserved_rows(&self, screen: Rect) -> u16 {
        if !self.eligible {
            return 0;
        }
        fit(
            screen.width,
            screen.height.saturating_sub(MIN_STANDING_ROWS),
        )
        .map_or(0, |(_, height)| height)
    }

    /// Paint the current pose for a terminal of `screen` size.
    ///
    /// A banner that merely lacks room stays armed, so growing the terminal brings it
    /// back; a static presentation paints the settled pose once and stops asking.
    pub(crate) fn paint(
        &mut self,
        screen: Rect,
        presentation: Presentation,
        now: Instant,
    ) -> Option<Pose> {
        let (width, height) = fit(
            screen.width,
            screen.height.saturating_sub(MIN_STANDING_ROWS),
        )?;

        let angle = match presentation {
            Presentation::Static => {
                self.eligible = false;
                self.pause_clock();
                SETTLED_ANGLE
            }
            Presentation::Animated => {
                if let Some(previous) = self.last_frame.replace(now) {
                    self.spin_elapsed += now.saturating_duration_since(previous);
                }
                if self.spin_elapsed < SPIN_DURATION {
                    sequence::sweep_angle(self.spin_elapsed.as_secs_f32())
                } else {
                    self.eligible = false;
                    self.pause_clock();
                    SETTLED_ANGLE
                }
            }
        };

        Some(Pose {
            width,
            lines: self.paint_frame(width, height, angle),
        })
    }

    /// Stop advancing visible time but keep the last painted pose.
    fn pause_clock(&mut self) {
        self.last_frame = None;
    }

    fn paint_frame(&mut self, width: u16, height: u16, angle: f32) -> Vec<Line<'static>> {
        let (foreground, background) = terminal_material();
        let cells = self.renderer.frame(
            width,
            height,
            angle,
            &Lighting::terminal(foreground, background),
        );
        Renderer::lines(&cells, usize::from(width), colours_enabled())
    }
}

/// Presentation for this frame: whether the wheel may move is a terminal property.
pub(crate) fn presentation() -> Presentation {
    if motion_reduced() {
        Presentation::Static
    } else {
        Presentation::Animated
    }
}

/// The brand header that stays in history: mark, product, version, the folder
/// this thread belongs to, and the two ways to get moving. The model name is
/// deliberately absent: it arrives with the deferred startup fills, and the
/// footer already reports it. §3.5 keeps this block restrained like codex's
/// blossom empty state.
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
    fn fresh_banner_sweeps_then_holds() {
        let mut banner = EmptyStateAnimation::default();
        banner.start_fresh();
        let mut clock = Instant::now();
        let mut frames = 0usize;
        while banner.is_eligible() {
            let pose = banner
                .paint(SCREEN, Presentation::Animated, clock)
                .expect("stage");
            frames += 1;
            assert!(!pose.lines.is_empty());
            clock += FRAME_INTERVAL;
            assert!(
                frames < 400,
                "the sweep must terminate, took {frames} frames"
            );
        }
        assert!(
            u128::from(frames as u64) * FRAME_INTERVAL.as_millis() >= SPIN_DURATION.as_millis(),
            "the sweep ended early after {frames} frames"
        );
    }

    #[test]
    fn static_presentation_paints_once_and_stops_asking() {
        let mut banner = EmptyStateAnimation::default();
        banner.start_fresh();
        assert!(banner.reserved_rows(SCREEN) > 0);
        assert!(banner
            .paint(SCREEN, Presentation::Static, Instant::now())
            .is_some());
        assert!(!banner.is_eligible(), "a static pose needs no more frames");
        assert_eq!(banner.reserved_rows(SCREEN), 0);
    }

    #[test]
    fn a_banner_without_room_stays_armed() {
        let mut banner = EmptyStateAnimation::default();
        banner.start_fresh();
        let tight = Rect::new(0, 0, 12, 40);
        assert!(banner
            .paint(tight, Presentation::Animated, Instant::now())
            .is_none());
        assert!(
            banner.is_eligible(),
            "resizing wider must bring the wheel back"
        );
        assert_eq!(banner.reserved_rows(tight), 0);
        assert!(banner.reserved_rows(SCREEN) > 0);
    }

    #[test]
    fn a_short_screen_leaves_room_for_the_conversation() {
        let mut banner = EmptyStateAnimation::default();
        banner.start_fresh();
        // 20 rows cannot give the banner more than 6 after the standing chrome.
        assert_eq!(banner.reserved_rows(Rect::new(0, 0, 100, 20)), 0);
        assert!(banner.reserved_rows(Rect::new(0, 0, 100, 24)) <= 24 - MIN_STANDING_ROWS);
    }

    #[test]
    fn dismissed_banner_reserves_nothing() {
        let mut banner = EmptyStateAnimation::default();
        banner.start_fresh();
        banner.dismiss();
        assert_eq!(banner.reserved_rows(SCREEN), 0);
        assert!(!banner.is_eligible());
    }

    #[test]
    fn pose_lines_are_one_per_row_and_never_wider_than_the_stage() {
        let mut banner = EmptyStateAnimation::default();
        banner.start_fresh();
        let rows = banner.reserved_rows(SCREEN);
        let pose = banner
            .paint(SCREEN, Presentation::Static, Instant::now())
            .expect("stage");
        assert_eq!(pose.lines.len(), usize::from(rows));
        for line in &pose.lines {
            let used: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(used <= usize::from(pose.width), "line overflows: {used}");
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
        // §3.5: the empty state names the folder and how to start moving; the
        // model name is not part of it (it lands with the deferred fills and is
        // reported by the footer).
        let third: String = lines[2].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(third.contains("/help"), "{third}");
        assert!(third.contains('@') && third.contains('!'), "{third}");
        let english: String = header_lines("0.4.0", "/tmp/work", false)[2]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(english.contains("/help"), "{english}");
    }
}
