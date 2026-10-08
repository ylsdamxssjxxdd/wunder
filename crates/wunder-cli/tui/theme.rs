use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use std::sync::OnceLock;

use super::app::LogKind;

/// `NO_COLOR` is the de-facto switch for "this terminal cannot carry meaning in
/// hue". Read once so no frame pays for the lookup.
fn colorless() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty() && value != "0")
    })
}

/// One choke point: drop every hue while keeping the weight a style carries, so
/// glyphs and words remain the only signal (方案 §6 可访问性、§11.3 无色变体).
fn degrade(style: Style) -> Style {
    degrade_forced(style, colorless())
}

fn degrade_forced(style: Style, colorless: bool) -> Style {
    if !colorless {
        return style;
    }
    Style::default()
        .fg(Color::Reset)
        .bg(Color::Reset)
        .add_modifier(style.add_modifier)
}

pub(crate) fn secondary_text() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub(crate) fn accent_text() -> Style {
    degrade(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn success_text() -> Style {
    degrade(Style::default().fg(Color::Green))
}

pub(crate) fn link_text() -> Style {
    degrade(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::UNDERLINED),
    )
}

pub(crate) fn brand_text() -> Style {
    degrade(Style::default().fg(Color::Magenta))
}

pub(crate) fn danger_text() -> Style {
    degrade(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
}

/// Amber for the count a warning notice leads with; the sentence next to it stays dim so the
/// number is what a reader's eye lands on.
pub(crate) fn warning_text() -> Style {
    degrade(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn diff_hunk_prefix() -> Style {
    degrade(
        Style::default()
            .bg(Color::Rgb(22, 30, 50))
            .add_modifier(Modifier::DIM),
    )
}

pub(crate) fn diff_hunk_text() -> Style {
    degrade(
        Style::default()
            .fg(Color::Cyan)
            .bg(Color::Rgb(22, 30, 50))
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn diff_added_prefix() -> Style {
    degrade(
        Style::default()
            .bg(Color::Rgb(18, 48, 31))
            .add_modifier(Modifier::DIM),
    )
}

pub(crate) fn diff_added_text() -> Style {
    degrade(
        Style::default()
            .fg(Color::Rgb(157, 230, 188))
            .bg(Color::Rgb(18, 48, 31)),
    )
}

pub(crate) fn diff_added_marker() -> Style {
    degrade(
        Style::default()
            .fg(Color::Rgb(157, 230, 188))
            .bg(Color::Rgb(18, 48, 31))
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn diff_deleted_prefix() -> Style {
    degrade(
        Style::default()
            .bg(Color::Rgb(58, 24, 24))
            .add_modifier(Modifier::DIM),
    )
}

pub(crate) fn diff_deleted_text() -> Style {
    degrade(
        Style::default()
            .fg(Color::Rgb(255, 182, 182))
            .bg(Color::Rgb(58, 24, 24)),
    )
}

pub(crate) fn diff_deleted_marker() -> Style {
    degrade(
        Style::default()
            .fg(Color::Rgb(255, 182, 182))
            .bg(Color::Rgb(58, 24, 24))
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn block_title(active: bool) -> Style {
    if active {
        accent_text()
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

pub(crate) fn popup_item() -> Style {
    secondary_text()
}

pub(crate) fn popup_selected() -> Style {
    degrade(
        Style::default()
            .fg(Color::Cyan)
            .bg(Color::Rgb(26, 42, 54))
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn modal_selected() -> Style {
    degrade(
        Style::default()
            .fg(Color::Cyan)
            .bg(Color::Rgb(26, 42, 54))
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn transcript_selection(base: Style) -> Style {
    degrade(
        Style::default()
            .fg(base.fg.unwrap_or(Color::White))
            .bg(Color::Rgb(24, 36, 48))
            .add_modifier(base.add_modifier | Modifier::BOLD),
    )
}

/// The composer is the only editable region on screen, so it gets the same surface
/// tone the selection states use instead of floating on the terminal background.
pub(crate) fn composer_surface() -> Style {
    degrade(Style::default().bg(Color::Rgb(24, 36, 48)))
}

/// The prompt glyph keeps its weight in both focus states; only its colour yields.
pub(crate) fn composer_prompt(focused: bool) -> Style {
    if focused {
        accent_text()
    } else {
        Style::default()
            .add_modifier(Modifier::DIM)
            .add_modifier(Modifier::BOLD)
    }
}

pub(crate) fn log_style(kind: LogKind) -> Style {
    match kind {
        LogKind::Info => secondary_text(),
        LogKind::User => Style::default(),
        LogKind::Assistant => Style::default(),
        LogKind::Reasoning => secondary_text(),
        LogKind::Tool => secondary_text(),
        LogKind::Approval | LogKind::Inquiry => accent_text(),
        LogKind::Error => danger_text(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The degradation itself is tested through the forced flag: reading `NO_COLOR`
    /// from a snapshot test would mutate process state that other tests in this
    /// binary are already reading in parallel.
    #[test]
    fn degradation_keeps_the_weight_that_carries_the_meaning() {
        // The hue pair is what a colored terminal gets; the reset pair is what a
        // colorless one gets. `secondary_text` has no hue at all, so it is unchanged
        // and stays the dim baseline everywhere else builds on.
        let colored = diff_added_marker();
        assert!(colored.fg.is_some() && colored.bg.is_some());

        let degraded = degrade_forced(colored, true);
        assert_eq!(degraded.fg, Some(Color::Reset));
        assert_eq!(degraded.bg, Some(Color::Reset));
        assert!(
            degraded.add_modifier.contains(Modifier::BOLD),
            "the marker's weight is what survives without color"
        );

        let dimmed = degrade_forced(diff_added_prefix(), true);
        assert_eq!(dimmed.fg, Some(Color::Reset));
        assert!(dimmed.add_modifier.contains(Modifier::DIM));
    }
}
