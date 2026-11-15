use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub(crate) const RUNNING_INDICATOR: &str = "•";
pub(crate) const RUNNING_ANIMATION_FRAME: Duration = Duration::from_millis(48);

static PROCESS_START: OnceLock<Instant> = OnceLock::new();
const BREATH_PERIOD: Duration = Duration::from_millis(1400);
const FALLBACK_PULSE_PERIOD: Duration = Duration::from_millis(560);

/// The colour the breath starts and ends on. A running dot built into a cached
/// line wears this resting colour, and the same colour is what the per-frame
/// paint pass looks for, so it has to stay unique to the running dot: it is not
/// one of the theme's finished-state colours.
const PENDING_REST_RGB: (u8, u8, u8) = (92, 188, 255);

fn elapsed_since_start() -> Duration {
    PROCESS_START.get_or_init(Instant::now).elapsed()
}

fn animation_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

fn blend_channel(base: u8, highlight: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    ((base as f32) + ((highlight as f32) - (base as f32)) * t).round() as u8
}

/// The colour a running dot is built with. Transcript lines are cached for as
/// long as their content is unchanged, so the breathing colour cannot be baked
/// into them — a cached line would hold whatever phase the build happened to
/// see and freeze there. `paint_pending_dots` swaps this resting colour for the
/// live one on every frame instead.
pub(crate) fn pending_indicator_base_style() -> Style {
    if animation_enabled() {
        let (r, g, b) = PENDING_REST_RGB;
        Style::default()
            .fg(Color::Rgb(r, g, b))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    }
}

// Keep the running dot visually close to Codex: true-color terminals get a
// soft cyan-to-white breathing pulse, while low-color terminals fall back to
// a simple bright/dim pulse without changing layout width.
pub(crate) fn pending_indicator_style() -> Style {
    let elapsed = elapsed_since_start();
    if animation_enabled() {
        let period = BREATH_PERIOD.as_secs_f32().max(f32::EPSILON);
        let phase = (elapsed.as_secs_f32() % period) / period;
        let breath = 0.5 * (1.0 - (phase * std::f32::consts::TAU).cos());
        let (rest_r, rest_g, rest_b) = PENDING_REST_RGB;
        let r = blend_channel(rest_r, 232, breath);
        let g = blend_channel(rest_g, 248, breath);
        let b = blend_channel(rest_b, 255, breath);
        return Style::default()
            .fg(Color::Rgb(r, g, b))
            .add_modifier(Modifier::BOLD);
    }

    let pulse_on = (elapsed.as_millis() / FALLBACK_PULSE_PERIOD.as_millis()) % 2 == 0;
    if pulse_on {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    }
}

/// Hand every running dot in a frame the live colour.
///
/// A status row is always built as the leading span of its line, so the pass
/// only inspects that span: the cached frame is cloned per paint already, and
/// one integer comparison per row is all this costs. Resting dots left alone
/// (there are none while a thread is idle) keep their built colour.
pub(crate) fn paint_pending_dots(lines: &mut [Line<'static>]) {
    let base = pending_indicator_base_style();
    let live = pending_indicator_style();
    if base == live {
        return;
    }
    for line in lines.iter_mut() {
        let Some(first) = line.spans.first_mut() else {
            continue;
        };
        if first.content.trim() == RUNNING_INDICATOR && first.style == base {
            first.style = live;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Span;

    #[test]
    fn resting_dots_take_the_live_colour() {
        let mut lines = vec![Line::from(vec![
            Span::styled("• ".to_string(), pending_indicator_base_style()),
            Span::raw("ran the tool"),
        ])];
        // A finished row keeps its own colour: only the running dot animates.
        let mut settled = vec![Line::from(vec![
            Span::styled(
                "• ".to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("done"),
        ])];

        paint_pending_dots(&mut lines);
        paint_pending_dots(&mut settled);

        assert_eq!(lines[0].spans[0].style, pending_indicator_style());
        assert_eq!(
            settled[0].spans[0].style,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn the_command_header_dot_without_a_separator_animates_too() {
        let mut lines = vec![Line::from(vec![
            Span::styled("•".to_string(), pending_indicator_base_style()),
            Span::raw("Ran cargo test"),
        ])];
        paint_pending_dots(&mut lines);
        assert_eq!(lines[0].spans[0].style, pending_indicator_style());
    }
}
