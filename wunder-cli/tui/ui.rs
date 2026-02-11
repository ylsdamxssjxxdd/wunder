use super::app::{LogKind, TuiApp};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &mut TuiApp) {
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
        .visible_logs(usize::MAX)
        .into_iter()
        .map(|entry| log_line(entry.kind, entry.text))
        .collect();
    let transcript_viewport = inner_rect(vertical[1]);
    app.set_transcript_viewport(transcript_viewport.width, transcript_viewport.height);
    let transcript_text = Text::from(transcript_lines);
    let transcript = Paragraph::new(transcript_text)
        .block(
            Block::default()
                .title(" Conversation ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    app.set_transcript_rendered_lines(transcript.line_count(transcript_viewport.width));
    let transcript_scroll = app.transcript_scroll(transcript_viewport.height);
    frame.render_widget(transcript.scroll((transcript_scroll, 0)), vertical[1]);

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
    let inner = inner_rect(input_area);
    app.set_input_viewport(inner.width);
    let (input_text, cursor_x, cursor_y) = app.input_view(inner.width, inner.height);
    let input = Paragraph::new(input_text)
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().title(" Input ").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(input, input_area);

    if inner.width > 0 && inner.height > 0 {
        let x = inner.x + cursor_x.min(inner.width.saturating_sub(1));
        let y = inner.y + cursor_y.min(inner.height.saturating_sub(1));
        frame.set_cursor_position((x, y));
    }

    if app.shortcuts_visible() {
        draw_shortcuts_modal(frame, frame.area(), app.shortcuts_lines());
    }
}

fn draw_shortcuts_modal(frame: &mut Frame, area: Rect, lines: Vec<String>) {
    if area.width < 8 || area.height < 6 {
        return;
    }

    let max_line_width = lines
        .iter()
        .map(|line| line.chars().count() as u16)
        .max()
        .unwrap_or(0);
    let width = max_line_width
        .saturating_add(6)
        .max(26)
        .min(area.width.saturating_sub(2));
    let content_height = lines.len() as u16;
    let height = content_height
        .saturating_add(4)
        .max(8)
        .min(area.height.saturating_sub(2));

    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(lines.join("\n"))
        .block(
            Block::default()
                .title(" Shortcuts ")
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::White).bg(Color::Black)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

fn build_layout(area: Rect, popup_len: usize) -> Vec<Rect> {
    if popup_len == 0 {
        return Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(8),
                Constraint::Length(6),
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
            Constraint::Length(6),
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
    let style = match kind {
        LogKind::Info => Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
        LogKind::User => Style::default().fg(Color::LightBlue),
        LogKind::Assistant => Style::default().fg(Color::Green),
        LogKind::Reasoning => Style::default().fg(Color::LightYellow).add_modifier(Modifier::DIM),
        LogKind::Tool => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::ITALIC),
        LogKind::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    };

    Line::from(Span::styled(
        format!("{}{text}", super::app::log_prefix(kind)),
        style,
    ))
}
