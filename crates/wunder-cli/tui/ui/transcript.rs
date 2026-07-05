use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;

use crate::tui::app::TuiApp;

pub(crate) fn draw(frame: &mut Frame, area: Rect, viewport: Rect, app: &mut TuiApp, is_zh: bool) {
    let rendered = app.transcript_rendered_view(viewport.width, viewport.height, is_zh);
    let transcript = Paragraph::new(Text::from(rendered.lines)).wrap(Wrap { trim: false });
    frame.render_widget(transcript.scroll((rendered.local_scroll, 0)), area);
}
