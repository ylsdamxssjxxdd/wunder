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

pub(crate) fn brand_text() -> Style {
    Style::default().fg(Color::Magenta)
}

pub(crate) fn danger_text() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

pub(crate) fn block_border(active: bool) -> Style {
    if active {
        accent_text()
    } else {
        secondary_text()
    }
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
    accent_text().add_modifier(Modifier::REVERSED)
}

pub(crate) fn modal_selected() -> Style {
    accent_text().add_modifier(Modifier::REVERSED)
}

pub(crate) fn transcript_selection(base: Style) -> Style {
    base.patch(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD))
}

pub(crate) fn log_style(kind: LogKind) -> Style {
    match kind {
        LogKind::Info => secondary_text(),
        LogKind::User => accent_text(),
        LogKind::Assistant => success_text(),
        LogKind::Reasoning => secondary_text(),
        LogKind::Tool => brand_text(),
        LogKind::Error => danger_text(),
    }
}
