use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::tui::theme;

pub(crate) fn draw_shortcuts_modal(frame: &mut Frame, area: Rect, lines: Vec<String>, is_zh: bool) {
    if area.width < 20 || area.height < 8 {
        return;
    }
    let popup = centered_popup(area, lines.as_slice(), 26, 8);
    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(to_modal_lines(lines))
        .block(modal_block(
            if is_zh { " 快捷键 " } else { " Shortcuts " },
            false,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

pub(crate) fn draw_resume_modal(
    frame: &mut Frame,
    area: Rect,
    rows: Vec<String>,
    selected: usize,
    is_zh: bool,
) {
    if area.width < 20 || area.height < 8 {
        return;
    }
    let mut modal_lines = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            if index == selected {
                format!("> {row}")
            } else {
                format!("  {row}")
            }
        })
        .collect::<Vec<_>>();
    modal_lines.push(String::new());
    modal_lines.push(if is_zh {
        "上下选择，Enter 恢复，Esc 取消".to_string()
    } else {
        "Up/Down select, Enter resume, Esc cancel".to_string()
    });
    let popup = centered_popup(area, modal_lines.as_slice(), 30, 8);
    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(to_modal_lines(modal_lines))
        .block(modal_block(
            if is_zh {
                " 恢复会话 "
            } else {
                " Resume Sessions "
            },
            false,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

pub(crate) fn draw_approval_modal(
    frame: &mut Frame,
    area: Rect,
    input_area: Rect,
    lines: Vec<String>,
    is_zh: bool,
) {
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = anchored_popup(area, input_area, lines.as_slice(), 52, 10);
    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(to_modal_lines(lines))
        .block(modal_block(
            if is_zh { " 审批 " } else { " Approval " },
            true,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

pub(crate) fn draw_inquiry_modal(
    frame: &mut Frame,
    area: Rect,
    input_area: Rect,
    lines: Vec<String>,
    is_zh: bool,
) {
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = anchored_popup(area, input_area, lines.as_slice(), 56, 10);
    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(to_modal_lines(lines))
        .block(modal_block(
            if is_zh {
                " 问询面板 "
            } else {
                " Inquiry Panel "
            },
            true,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

fn modal_block(title: &str, active: bool) -> Block<'static> {
    Block::default()
        .title(Span::styled(title.to_string(), theme::block_title(active)))
        .borders(Borders::ALL)
        .border_style(theme::block_border(active))
}

fn centered_popup(area: Rect, lines: &[String], min_width: u16, min_height: u16) -> Rect {
    let max_line_width = lines
        .iter()
        .map(String::as_str)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0) as u16;
    let width = max_line_width
        .saturating_add(6)
        .max(min_width)
        .min(area.width.saturating_sub(2));
    let height = (lines.len() as u16)
        .saturating_add(4)
        .max(min_height)
        .min(area.height.saturating_sub(2));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn anchored_popup(
    area: Rect,
    anchor: Rect,
    lines: &[String],
    min_width: u16,
    min_height: u16,
) -> Rect {
    let horizontal_bounds = if anchor.width > 0 { anchor } else { area };
    let max_line_width = lines
        .iter()
        .map(String::as_str)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0) as u16;
    let width = max_line_width
        .saturating_add(6)
        .max(min_width)
        .min(horizontal_bounds.width.saturating_sub(2))
        .max(1);
    let height = (lines.len() as u16)
        .saturating_add(4)
        .max(min_height)
        .min(area.height.saturating_sub(2));
    Rect {
        x: horizontal_bounds.x + horizontal_bounds.width.saturating_sub(width) / 2,
        y: anchor.y.saturating_sub(height),
        width,
        height,
    }
}

fn to_modal_lines(lines: Vec<String>) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('>') {
                Line::from(Span::styled(line, theme::modal_selected()))
            } else if line.trim().is_empty() {
                Line::from(Span::raw(String::new()))
            } else {
                Line::from(Span::raw(line))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_popup_respects_minimum_size() {
        let area = Rect::new(0, 0, 80, 24);
        let popup = centered_popup(area, &["短文本".to_string()], 30, 8);
        assert_eq!(popup.width, 30);
        assert_eq!(popup.height, 8);
        assert_eq!(popup.x, 25);
    }

    #[test]
    fn anchored_popup_positions_above_anchor() {
        let area = Rect::new(0, 0, 80, 24);
        let anchor = Rect::new(10, 18, 60, 6);
        let popup = anchored_popup(
            area,
            anchor,
            &["工具：read_file".to_string(), "摘要：读取文件".to_string()],
            52,
            10,
        );
        assert_eq!(popup.y, 8);
        assert_eq!(popup.height, 10);
        assert!(popup.x >= anchor.x);
        assert!(popup.width <= anchor.width.saturating_sub(2));
    }

    #[test]
    fn to_modal_lines_keeps_selected_marker() {
        let lines = to_modal_lines(vec!["> selected".to_string(), "plain".to_string()]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content.as_ref(), "> selected");
        assert_eq!(lines[1].spans[0].content.as_ref(), "plain");
    }
}
