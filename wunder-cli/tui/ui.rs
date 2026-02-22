use super::app::{LogKind, TuiApp};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &mut TuiApp) {
    let is_zh = app.is_zh_language();
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

    let transcript_viewport = inner_rect(vertical[1]);
    app.set_transcript_viewport(transcript_viewport.width, transcript_viewport.height);
    let selected_transcript = app.selected_transcript_index();
    let render_window = app.transcript_render_window(transcript_viewport.height);
    let transcript_total_lines = render_window.total_lines;
    let transcript_scroll = render_window.local_scroll;
    let transcript_lines: Vec<Line> = render_window
        .entries
        .into_iter()
        .map(|entry| {
            log_line(
                entry.kind,
                entry.text,
                selected_transcript.is_some_and(|selected| selected == entry.global_index),
            )
        })
        .collect();
    let transcript_text = Text::from(transcript_lines);
    let transcript_title = if app.transcript_focus_active() {
        if is_zh {
            " 会话（输出焦点） "
        } else {
            " Conversation (Output Focus) "
        }
    } else if is_zh {
        " 会话 "
    } else {
        " Conversation "
    };
    let transcript = Paragraph::new(transcript_text)
        .block(
            Block::default()
                .title(transcript_title)
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    app.set_transcript_rendered_lines(transcript_total_lines);
    frame.render_widget(transcript.scroll((transcript_scroll, 0)), vertical[1]);

    let input_index = if popup_lines.is_empty() { 2 } else { 3 };

    if !popup_lines.is_empty() {
        let popup = Paragraph::new(popup_lines.join("\n"))
            .block(
                Block::default()
                    .title(app.popup_title())
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
        .block(
            Block::default()
                .title(if is_zh { " 输入 " } else { " Input " })
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(input, input_area);

    if inner.width > 0 && inner.height > 0 {
        let x = inner.x + cursor_x.min(inner.width.saturating_sub(1));
        let y = inner.y + cursor_y.min(inner.height.saturating_sub(1));
        frame.set_cursor_position((x, y));
    }

    if let Some((rows, selected)) = app.resume_picker_rows() {
        draw_resume_modal(frame, frame.area(), rows, selected, is_zh);
    }

    if app.shortcuts_visible() {
        draw_shortcuts_modal(frame, frame.area(), app.shortcuts_lines(), is_zh);
    }

    if let Some(lines) = app.approval_modal_lines() {
        draw_approval_modal(frame, frame.area(), lines, is_zh);
    }
}

fn draw_shortcuts_modal(frame: &mut Frame, area: Rect, lines: Vec<String>, is_zh: bool) {
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
                .title(if is_zh { " 快捷键 " } else { " Shortcuts " })
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

fn draw_resume_modal(
    frame: &mut Frame,
    area: Rect,
    rows: Vec<String>,
    selected: usize,
    is_zh: bool,
) {
    if area.width < 20 || area.height < 8 {
        return;
    }

    let max_line_width = rows
        .iter()
        .map(|line| line.chars().count() as u16)
        .max()
        .unwrap_or(0);
    let width = max_line_width
        .saturating_add(6)
        .max(48)
        .min(area.width.saturating_sub(2));
    let content_height = rows.len() as u16;
    let height = content_height
        .saturating_add(5)
        .max(8)
        .min(area.height.saturating_sub(2));

    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };

    let mut lines = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            if index == selected {
                Line::from(Span::styled(
                    row,
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(row, Style::default().fg(Color::White)))
            }
        })
        .collect::<Vec<_>>();
    lines.push(Line::from(Span::styled(
        "",
        Style::default().fg(Color::White),
    )));
    lines.push(Line::from(Span::styled(
        if is_zh {
            "上下选择，Enter 恢复，Esc 取消"
        } else {
            "Up/Down select, Enter resume, Esc cancel"
        },
        Style::default().fg(Color::Gray),
    )));

    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(if is_zh {
                    " 恢复历史会话 "
                } else {
                    " Resume Sessions "
                })
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::White).bg(Color::Black)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

fn draw_approval_modal(frame: &mut Frame, area: Rect, lines: Vec<String>, is_zh: bool) {
    if area.width < 24 || area.height < 8 {
        return;
    }
    let max_line_width = lines
        .iter()
        .map(|line| line.chars().count() as u16)
        .max()
        .unwrap_or(0);
    let width = max_line_width
        .saturating_add(6)
        .max(52)
        .min(area.width.saturating_sub(2));
    let content_height = lines.len() as u16;
    let height = content_height
        .saturating_add(4)
        .max(10)
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
                .title(if is_zh { " 审批 " } else { " Approval " })
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::White).bg(Color::Black)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

fn log_line(kind: LogKind, text: &str, selected: bool) -> Line<'static> {
    let style = match kind {
        LogKind::Info => Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
        LogKind::User => Style::default().fg(Color::LightBlue),
        LogKind::Assistant => Style::default().fg(Color::Green),
        LogKind::Reasoning => Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::DIM),
        LogKind::Tool => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::ITALIC),
        LogKind::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    };

    let style = if selected {
        style.bg(Color::DarkGray).add_modifier(Modifier::BOLD)
    } else {
        style
    };

    Line::from(Span::styled(
        format!("{}{text}", super::app::log_prefix(kind)),
        style,
    ))
}
