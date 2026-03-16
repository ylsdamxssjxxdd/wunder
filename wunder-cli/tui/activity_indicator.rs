use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub(crate) const RUNNING_INDICATOR: &str = "•";
pub(crate) const RUNNING_ANIMATION_FRAME: Duration = Duration::from_millis(48);

static PROCESS_START: OnceLock<Instant> = OnceLock::new();
const BREATH_PERIOD: Duration = Duration::from_millis(1400);
const FALLBACK_PULSE_PERIOD: Duration = Duration::from_millis(560);

fn elapsed_since_start() -> Duration {
    PROCESS_START.get_or_init(Instant::now).elapsed()
}

fn blend_channel(base: u8, highlight: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    ((base as f32) + ((highlight as f32) - (base as f32)) * t).round() as u8
}

// Keep the running dot visually close to Codex: true-color terminals get a
// soft cyan-to-white breathing pulse, while low-color terminals fall back to
// a simple bright/dim pulse without changing layout width.
pub(crate) fn pending_indicator_style() -> Style {
    let elapsed = elapsed_since_start();
    if std::env::var_os("NO_COLOR").is_none() {
        let period = BREATH_PERIOD.as_secs_f32().max(f32::EPSILON);
        let phase = (elapsed.as_secs_f32() % period) / period;
        let breath = 0.5 * (1.0 - (phase * std::f32::consts::TAU).cos());
        let r = blend_channel(92, 232, breath);
        let g = blend_channel(188, 248, breath);
        let b = blend_channel(255, 255, breath);
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
