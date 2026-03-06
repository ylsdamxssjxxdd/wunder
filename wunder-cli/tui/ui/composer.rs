use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;

use crate::tui::app::TuiApp;
use crate::tui::theme;

pub(crate) fn draw_activity(frame: &mut Frame, area: Rect, app: &TuiApp) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let style = if app.activity_highlighted() {
        theme::accent_text()
    } else {
        theme::secondary_text()
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(app.activity_line(), style))),
        area,
    );
}

pub(crate) fn draw_input(
    frame: &mut Frame,
    area: Rect,
    inner: Rect,
    app: &mut TuiApp,
    is_zh: bool,
) {
    app.set_input_viewport(inner.width);
    let (input_text, cursor_x, cursor_y) = app.input_view(inner.width, inner.height);
    let active = app.input_focus_active();
    let title = if is_zh { " 输入 " } else { " Input " };
    let hint = app.composer_hint_line();
    let mut title_spans = vec![
        Span::styled(title, theme::block_title(active)),
        Span::styled(hint, theme::secondary_text()),
    ];
    if let Some(attachment_hint) = app.composer_attachment_hint() {
        title_spans.push(Span::raw("  "));
        title_spans.push(Span::styled(attachment_hint, theme::success_text()));
    }
    let title_line = Line::from(title_spans);

    let body = if app.input_is_empty() {
        let placeholder = if is_zh {
            "直接提问，或使用 / 命令、@ 文件、# 技能、$ 应用；拖入图片/文件会自动作为附件"
        } else {
            "Ask directly, or use / commands, @ files, # skills, and $ apps; drop images/files to attach"
        };
        Text::from(vec![Line::from(Span::styled(
            placeholder,
            theme::secondary_text(),
        ))])
    } else {
        Text::from(input_text)
    };

    let input = Paragraph::new(body)
        .block(
            Block::default()
                .title(title_line)
                .borders(Borders::ALL)
                .border_style(theme::block_border(active)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(input, area);

    if inner.width > 0 && inner.height > 0 {
        let x = inner.x + cursor_x.min(inner.width.saturating_sub(1));
        let y = inner.y + cursor_y.min(inner.height.saturating_sub(1));
        frame.set_cursor_position((x, y));
    }
}
