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

pub(crate) fn draw(frame: &mut Frame, area: Rect, viewport: Rect, app: &mut TuiApp, is_zh: bool) {
    app.set_transcript_viewport(viewport.width, viewport.height);
    let selected_transcript = app.selected_transcript_index();
    let render_window = app.transcript_render_window(viewport.height);
    let transcript_total_lines = render_window.total_lines;
    let transcript_scroll = render_window.local_scroll;
    let mut transcript_lines: Vec<Line> = render_window
        .entries
        .into_iter()
        .flat_map(|entry| {
            app.render_entry_lines(
                entry.global_index,
                selected_transcript.is_some_and(|selected| selected == entry.global_index),
                viewport.width,
            )
        })
        .collect();

    if transcript_lines.is_empty() {
        let placeholder = if is_zh {
            "还没有对话内容，输入提示词开始。"
        } else {
            "No conversation yet. Start by typing a prompt."
        };
        transcript_lines.push(Line::from(Span::styled(
            placeholder,
            theme::secondary_text(),
        )));
    }

    let transcript_title = if app.transcript_focus_active() {
        if is_zh {
            " 对话（输出焦点） "
        } else {
            " Conversation (Output Focus) "
        }
    } else if is_zh {
        " 对话 "
    } else {
        " Conversation "
    };
    let transcript = Paragraph::new(Text::from(transcript_lines))
        .block(
            Block::default()
                .title(Span::styled(
                    transcript_title,
                    theme::block_title(app.transcript_focus_active()),
                ))
                .borders(Borders::ALL)
                .border_style(theme::block_border(app.transcript_focus_active())),
        )
        .wrap(Wrap { trim: false });
    app.set_transcript_rendered_lines(transcript_total_lines);
    frame.render_widget(transcript.scroll((transcript_scroll, 0)), area);
}
