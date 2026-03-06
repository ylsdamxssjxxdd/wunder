use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;

use crate::tui::theme;

pub(crate) fn draw(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    lines: &[String],
    selected: Option<usize>,
) {
    let popup_lines = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if selected.is_some_and(|current| current == index) {
                Line::from(Span::styled(line.clone(), theme::popup_selected()))
            } else {
                Line::from(Span::styled(line.clone(), theme::popup_item()))
            }
        })
        .collect::<Vec<_>>();

    let widget = Paragraph::new(popup_lines)
        .block(
            Block::default()
                .title(Span::styled(title, theme::block_title(false)))
                .borders(Borders::ALL)
                .border_style(theme::block_border(false)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_lines(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        let width = buffer.area.width as usize;
        buffer
            .content
            .chunks(width)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect()
    }

    #[test]
    fn draw_popup_renders_title_and_items() {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    Rect::new(0, 0, 40, 8),
                    " Skills ",
                    &["#shell".to_string(), "#files".to_string()],
                    Some(1),
                );
            })
            .expect("draw");

        let lines = buffer_lines(&terminal);
        assert!(lines.iter().any(|line| line.contains("Skills")));
        assert!(lines.iter().any(|line| line.contains("#shell")));
        assert!(lines.iter().any(|line| line.contains("#files")));
    }
}
