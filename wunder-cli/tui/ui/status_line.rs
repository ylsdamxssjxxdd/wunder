use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::tui::app::TuiApp;
use crate::tui::theme;

pub(crate) fn draw(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let commands = if app.is_zh_language() {
        "/ 命令"
    } else {
        "/ commands"
    };
    let status = app.status_line().trim().to_string();
    let mut spans = vec![Span::styled(commands, theme::secondary_text())];
    let left_width = spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum::<usize>();
    let status_width = UnicodeWidthStr::width(status.as_str());
    let total_width = usize::from(area.width);
    if !status.is_empty() && left_width + status_width + 2 <= total_width {
        spans.push(Span::raw(
            " ".repeat(total_width.saturating_sub(left_width + status_width)),
        ));
        spans.push(Span::styled(status, theme::secondary_text()));
    } else if !status.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(status, theme::secondary_text()));
    }
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}
