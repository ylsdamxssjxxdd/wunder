use super::layout;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

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
    _inner: Rect,
    app: &mut TuiApp,
    is_zh: bool,
) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(area);
    let input_area = sections[0];
    let footer_area = sections[1];
    let inner = layout::inner_rect(input_area);
    app.set_input_viewport(inner.width);
    let (input_text, cursor_x, cursor_y) = app.input_view(inner.width, inner.height);
    let active = app.input_focus_active();
    let title = if is_zh { " 输入 " } else { " Input " };
    let mut title_spans = vec![Span::styled(title, theme::block_title(active))];
    if let Some(attachment_hint) = app.composer_attachment_hint() {
        title_spans.push(Span::raw("  "));
        title_spans.push(Span::styled(attachment_hint, theme::success_text()));
    }
    let title_line = Line::from(title_spans);

    let body = if app.input_is_empty() {
        let placeholder = if is_zh {
            "直接提问，或使用 / 命令、@ 文件、# 技能、$ 应用；拖入图片或文件即可附加"
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
    frame.render_widget(input, input_area);

    if footer_area.height > 0 {
        if let Some(footer) = build_footer_line(app, footer_area.width) {
            frame.render_widget(Paragraph::new(footer), footer_area);
        }
    }

    if inner.width > 0 && inner.height > 0 {
        let x = inner.x + cursor_x.min(inner.width.saturating_sub(1));
        let y = inner.y + cursor_y.min(inner.height.saturating_sub(1));
        frame.set_cursor_position((x, y));
    }
}

fn build_footer_line(app: &TuiApp, width: u16) -> Option<Line<'static>> {
    if width == 0 {
        return None;
    }

    if let Some(right_text) = app.composer_footer_context() {
        if let Some(line) =
            build_footer_line_with_right(app.composer_footer_items(), right_text, width)
        {
            return Some(line);
        }
    }

    let spans = build_footer_spans(app.composer_footer_items(), width);

    if spans.is_empty() {
        return Some(Line::from(Span::styled(
            app.composer_hint_line(),
            theme::secondary_text(),
        )));
    }
    Some(Line::from(spans))
}

fn build_footer_line_with_right(
    items: Vec<(String, String)>,
    right_text: String,
    width: u16,
) -> Option<Line<'static>> {
    let total_width = usize::from(width.max(1));
    let right_width = UnicodeWidthStr::width(right_text.as_str());
    if right_width >= total_width {
        return None;
    }

    let left_budget = total_width.saturating_sub(right_width + 2);
    let left_spans = build_footer_spans(items, left_budget as u16);
    if left_spans.is_empty() {
        return None;
    }

    let left_width = spans_width(left_spans.as_slice());
    if left_width + right_width >= total_width {
        return None;
    }

    let gap = total_width.saturating_sub(left_width + right_width);
    let mut spans = left_spans;
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(right_text, theme::secondary_text()));
    Some(Line::from(spans))
}

fn spans_width(spans: &[Span<'static>]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn build_footer_spans(items: Vec<(String, String)>, width: u16) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (index, (key, label)) in items.into_iter().enumerate() {
        let gap = if index == 0 { 0 } else { 2 };
        let key_width = UnicodeWidthStr::width(key.as_str());
        let compact_width = gap + key_width;
        let remaining_after_key = usize::from(width)
            .saturating_sub(used)
            .saturating_sub(gap)
            .saturating_sub(key_width);

        let rendered_label = if label.is_empty() || remaining_after_key == 0 {
            None
        } else if remaining_after_key > 1 {
            compact_footer_label(label.as_str(), remaining_after_key.saturating_sub(1))
        } else {
            None
        };

        let rendered_label_width = rendered_label
            .as_deref()
            .map(UnicodeWidthStr::width)
            .unwrap_or(0);
        let full_width =
            gap + key_width + usize::from(rendered_label.is_some()) + rendered_label_width;

        let show_key_only = used.saturating_add(compact_width) <= usize::from(width);
        if rendered_label.is_none() && !show_key_only {
            break;
        }
        let show_label =
            rendered_label.is_some() && used.saturating_add(full_width) <= usize::from(width);

        if gap > 0 {
            spans.push(Span::styled("  ", theme::secondary_text()));
            used = used.saturating_add(gap);
        }
        spans.push(Span::styled(key, theme::accent_text()));
        used = used.saturating_add(key_width);
        if show_label {
            spans.push(Span::raw(" "));
            let label = rendered_label.expect("footer label should exist when shown");
            used = used.saturating_add(1 + rendered_label_width);
            spans.push(Span::styled(label, theme::secondary_text()));
        }
    }
    spans
}

fn compact_footer_label(label: &str, max_width: usize) -> Option<String> {
    if max_width == 0 {
        return None;
    }

    if UnicodeWidthStr::width(label) <= max_width {
        return Some(label.to_string());
    }

    if max_width < 2 {
        return None;
    }

    let ellipsis = '…';
    let ellipsis_width = UnicodeWidthChar::width(ellipsis).unwrap_or(1);
    if max_width <= ellipsis_width {
        return None;
    }

    let mut text = String::new();
    let mut used = 0usize;
    for ch in label.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width + ellipsis_width > max_width {
            break;
        }
        text.push(ch);
        used += ch_width;
    }

    if text.is_empty() {
        None
    } else {
        text.push(ellipsis);
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_spans(spans: Vec<Span<'static>>) -> String {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn footer_uses_labels_when_width_allows() {
        let spans = build_footer_spans(
            vec![
                ("@".to_string(), "files".to_string()),
                ("Tab".to_string(), "complete".to_string()),
            ],
            40,
        );
        let rendered = render_spans(spans);
        assert!(rendered.contains("@ files"));
        assert!(rendered.contains("Tab complete"));
    }

    #[test]
    fn footer_drops_labels_before_keys_when_narrow() {
        let spans = build_footer_spans(
            vec![
                ("@".to_string(), "files".to_string()),
                ("Ctrl+V".to_string(), "images".to_string()),
                ("Tab".to_string(), "complete".to_string()),
            ],
            18,
        );
        let rendered = render_spans(spans);
        assert!(rendered.contains("@ files"));
        assert!(rendered.contains("Ctrl+V"));
        assert!(!rendered.contains("Ctrl+V images"));
    }

    #[test]
    fn footer_truncates_labels_before_dropping_item() {
        let spans = build_footer_spans(
            vec![
                ("@".to_string(), "files".to_string()),
                ("Ctrl+V".to_string(), "images".to_string()),
            ],
            20,
        );
        let rendered = render_spans(spans);
        assert!(rendered.contains("@ files"));
        assert!(rendered.contains("Ctrl+V "));
        assert!(rendered.contains('…'));
        assert!(!rendered.contains("Ctrl+V images"));
    }

    #[test]
    fn footer_can_place_context_on_right() {
        let line = build_footer_line_with_right(
            vec![
                ("@".to_string(), "files".to_string()),
                ("Tab".to_string(), "complete".to_string()),
            ],
            "ctx 72% · att 2".to_string(),
            40,
        )
        .expect("footer line");
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(rendered.contains("@ files"));
        assert!(rendered.ends_with("ctx 72% · att 2"));
    }
}
