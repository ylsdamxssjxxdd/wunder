use super::app::{LogKind, TuiApp};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &TuiApp) {
    let popup_lines = app.popup_lines();
    let vertical = build_layout(frame.area(), popup_lines.len());

    let status = Paragraph::new(app.status_line())
        .style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::NONE));
    frame.render_widget(status, vertical[0]);

    let transcript_lines: Vec<Line> = app
        .visible_logs(220)
        .into_iter()
        .map(|entry| log_line(entry.kind, entry.text))
        .collect();
    let transcript = Paragraph::new(Text::from(transcript_lines))
        .block(
            Block::default()
                .title(" Conversation ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(transcript, vertical[1]);

    let input_index = if popup_lines.is_empty() { 2 } else { 3 };

    if !popup_lines.is_empty() {
        let popup = Paragraph::new(popup_lines.join("\n"))
            .block(
                Block::default()
                    .title(" Commands ")
                    .borders(Borders::ALL)
                    .style(Style::default().fg(Color::Gray)),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(popup, vertical[2]);
    }

    let input_area = vertical[input_index];
    let input = Paragraph::new(app.input())
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().title(" Input ").borders(Borders::ALL));
    frame.render_widget(input, input_area);

    let inner = inner_rect(input_area);
    if inner.width > 0 {
        let max_cursor = inner.width.saturating_sub(1) as usize;
        let cursor = app.cursor_offset().min(max_cursor) as u16;
        frame.set_cursor_position((inner.x + cursor, inner.y));
    }
}

fn build_layout(area: Rect, popup_len: usize) -> Vec<Rect> {
    if popup_len == 0 {
        return Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(8),
                Constraint::Length(3),
            ])
            .split(area)
            .to_vec();
    }

    let popup_height = (popup_len as u16).min(7).saturating_add(2);
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Length(popup_height),
            Constraint::Length(3),
        ])
        .split(area)
        .to_vec()
}

fn inner_rect(rect: Rect) -> Rect {
    Rect {
        x: rect.x.saturating_add(1),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    }
}

fn log_line(kind: LogKind, text: String) -> Line<'static> {
    let (prefix, style) = match kind {
        LogKind::Info => (
            "• ",
            Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
        ),
        LogKind::User => ("you> ", Style::default().fg(Color::LightBlue)),
        LogKind::Assistant => ("assistant> ", Style::default().fg(Color::Green)),
        LogKind::Tool => (
            "tool> ",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::ITALIC),
        ),
        LogKind::Error => (
            "error> ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    };

    Line::from(Span::styled(format!("{prefix}{text}"), style))
}
