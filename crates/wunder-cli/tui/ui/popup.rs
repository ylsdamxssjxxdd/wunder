use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
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
    let mut popup_lines = Vec::with_capacity(lines.len().saturating_add(1));
    if !title.trim().is_empty() {
        popup_lines.push(Line::from(Span::styled(
            title.trim().to_string(),
            theme::block_title(false),
        )));
    }
    popup_lines.extend(lines.iter().enumerate().map(|(index, line)| {
        let marker = if selected.is_some_and(|current| current == index) {
            Span::styled("\u{203a} ", theme::accent_text())
        } else {
            Span::raw("  ")
        };
        let body_style = if selected.is_some_and(|current| current == index) {
            theme::popup_selected()
        } else {
            theme::popup_item()
        };
        Line::from(vec![marker, Span::styled(line.clone(), body_style)])
    }));

    let widget = Paragraph::new(popup_lines).wrap(Wrap { trim: false });
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
