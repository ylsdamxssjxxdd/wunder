use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::TuiApp;
use crate::tui::theme;

pub(crate) fn draw(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let help = if app.is_zh_language() {
        "? 快捷键"
    } else {
        "? shortcuts"
    };
    let commands = if app.is_zh_language() {
        "/ 命令"
    } else {
        "/ commands"
    };
    let status = app.status_line().trim().to_string();
    let line = Line::from(vec![
        Span::styled(help, theme::accent_text()),
        Span::raw("  "),
        Span::styled(commands, theme::secondary_text()),
        Span::raw("  "),
        Span::styled(status, theme::secondary_text()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
