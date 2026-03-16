use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

use super::app::LogKind;

pub(crate) fn secondary_text() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub(crate) fn accent_text() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn success_text() -> Style {
    Style::default().fg(Color::Green)
}

pub(crate) fn link_text() -> Style {
    accent_text().add_modifier(Modifier::UNDERLINED)
}

pub(crate) fn brand_text() -> Style {
    Style::default().fg(Color::Magenta)
}

pub(crate) fn danger_text() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

pub(crate) fn diff_hunk_prefix() -> Style {
    Style::default()
        .bg(Color::Rgb(22, 30, 50))
        .add_modifier(Modifier::DIM)
}

pub(crate) fn diff_hunk_text() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .bg(Color::Rgb(22, 30, 50))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn diff_added_prefix() -> Style {
    Style::default()
        .bg(Color::Rgb(18, 48, 31))
        .add_modifier(Modifier::DIM)
}

pub(crate) fn diff_added_text() -> Style {
    Style::default()
        .fg(Color::Rgb(157, 230, 188))
        .bg(Color::Rgb(18, 48, 31))
}

pub(crate) fn diff_added_marker() -> Style {
    diff_added_text().add_modifier(Modifier::BOLD)
}

pub(crate) fn diff_deleted_prefix() -> Style {
    Style::default()
        .bg(Color::Rgb(58, 24, 24))
        .add_modifier(Modifier::DIM)
}

pub(crate) fn diff_deleted_text() -> Style {
    Style::default()
        .fg(Color::Rgb(255, 182, 182))
        .bg(Color::Rgb(58, 24, 24))
}

pub(crate) fn diff_deleted_marker() -> Style {
    diff_deleted_text().add_modifier(Modifier::BOLD)
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
    Style::default()
        .fg(Color::Cyan)
        .bg(Color::Rgb(26, 42, 54))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn modal_selected() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .bg(Color::Rgb(26, 42, 54))
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn transcript_selection(base: Style) -> Style {
    Style::default()
        .fg(base.fg.unwrap_or(Color::White))
        .bg(Color::Rgb(24, 36, 48))
        .add_modifier(base.add_modifier | Modifier::BOLD)
}

pub(crate) fn log_style(kind: LogKind) -> Style {
    match kind {
        LogKind::Info => secondary_text(),
        LogKind::User => Style::default(),
        LogKind::Assistant => Style::default(),
        LogKind::Reasoning => secondary_text(),
        LogKind::Tool => secondary_text(),
        LogKind::Error => danger_text(),
    }
}
