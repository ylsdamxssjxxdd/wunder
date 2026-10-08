use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::tui::app::TuiApp;
use crate::tui::theme;

pub(crate) fn draw(frame: &mut Frame, area: Rect, viewport: Rect, app: &mut TuiApp, is_zh: bool) {
    let rendered = app.transcript_rendered_view(viewport.width, viewport.height, is_zh);
    let transcript = Paragraph::new(Text::from(rendered.lines)).wrap(Wrap { trim: false });
    frame.render_widget(transcript.scroll((rendered.local_scroll, 0)), area);
}

/// The Ctrl+T view: a status rule, then the same transcript with every card expanded.
pub(crate) fn draw_view(frame: &mut Frame, area: Rect, app: &mut TuiApp, is_zh: bool) {
    let chunks = ratatui::layout::Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let body_height = chunks[1].height;

    let lines = app.transcript_view_frame(chunks[1].width, body_height);
    let status = app.transcript_view_status(chunks[1].width);
    frame.render_widget(
        Paragraph::new(Text::from(view_status_line(status, is_zh))),
        chunks[0],
    );
    frame.render_widget(Paragraph::new(Text::from(lines)), chunks[1]);
}

fn view_status_line(status: String, is_zh: bool) -> Line<'static> {
    let keys = if is_zh {
        "ctrl+t/esc 关闭 · ↑↓ PgUp PgDn Home End 滚动"
    } else {
        "ctrl+t/esc close · ↑↓ PgUp PgDn Home End scroll"
    };
    Line::from(vec![
        Span::styled(format!("{status} "), theme::accent_text()),
        Span::styled(keys, theme::secondary_text()),
    ])
}
