use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::tui::activity_indicator;
use crate::tui::app::TuiApp;
use crate::tui::theme;

const INPUT_PROMPT: &str = "› ";

pub(crate) fn draw_activity(frame: &mut Frame, area: Rect, app: &TuiApp) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let line = app.activity_line();
    let text_style = if app.activity_highlighted() {
        theme::accent_text()
    } else {
        theme::secondary_text()
    };

    if let Some(rest) = line.strip_prefix("• ") {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    activity_indicator::RUNNING_INDICATOR,
                    activity_indicator::pending_indicator_style(),
                ),
                Span::raw(" "),
                Span::styled(rest.to_string(), text_style),
            ])),
            area,
        );
        return;
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(line, text_style))),
        area,
    );
}

pub(crate) fn draw_input(frame: &mut Frame, area: Rect, app: &mut TuiApp, is_zh: bool) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let attachment_hint = app.composer_attachment_hint();
    let attachment_height = u16::from(attachment_hint.is_some() && area.height >= 3);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(attachment_height),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    let attachment_area = sections[0];
    let input_area = sections[1];
    let footer_area = sections[2];

    if attachment_area.height > 0 && attachment_area.width > 0 {
        if let Some(hint) = attachment_hint {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(hint, theme::success_text())))
                    .wrap(Wrap { trim: false }),
                attachment_area,
            );
        }
    }

    let prompt_width = UnicodeWidthStr::width(INPUT_PROMPT) as u16;
    let text_area = Rect {
        x: input_area.x.saturating_add(prompt_width),
        y: input_area.y,
        width: input_area.width.saturating_sub(prompt_width),
        height: input_area.height,
    };
    app.set_input_viewport(text_area.width.max(1));
    let (input_text, cursor_x, cursor_y) =
        app.input_view(text_area.width.max(1), text_area.height.max(1));

    if input_area.height > 0 && input_area.width > 0 {
        let prompt_style = if app.input_focus_active() {
            theme::accent_text()
        } else {
            theme::secondary_text()
        };
        let prompt_area = Rect {
            x: input_area.x,
            y: input_area.y,
            width: input_area.width.min(prompt_width.max(1)),
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(INPUT_PROMPT, prompt_style))),
            prompt_area,
        );
    }

    if text_area.height > 0 && text_area.width > 0 {
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
            let large_paste_placeholders = app.large_paste_placeholders();
            render_input_text(input_text.as_str(), large_paste_placeholders.as_slice())
        };

        frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), text_area);
    }

    if footer_area.height > 0 {
        if let Some(footer) = build_footer_line(app, footer_area.width) {
            frame.render_widget(Paragraph::new(footer), footer_area);
        }
    }

    if text_area.width > 0 && text_area.height > 0 {
        let x = text_area.x + cursor_x.min(text_area.width.saturating_sub(1));
        let y = text_area.y + cursor_y.min(text_area.height.saturating_sub(1));
        frame.set_cursor_position((x, y));
    }
}

fn build_footer_line(app: &TuiApp, width: u16) -> Option<Line<'static>> {
    if width == 0 {
        return None;
    }

    let items = app.composer_footer_items();

    if let Some(right_text) = app.composer_footer_context() {
        if let Some(line) = build_footer_line_with_right(items.clone(), right_text.clone(), width) {
            return Some(line);
        }
        if let Some(line) = build_right_aligned_footer_line(right_text, width) {
            return Some(line);
        }
    }

    let spans = build_footer_spans(items, width);

    if spans.is_empty() {
        return Some(Line::from(Span::styled(
            app.composer_hint_line(),
            theme::secondary_text(),
        )));
    }
    Some(Line::from(spans))
}

fn render_input_text(text: &str, large_paste_placeholders: &[String]) -> Text<'static> {
    let lines = text
        .split('\n')
        .map(|line| render_input_line(line, large_paste_placeholders))
        .collect::<Vec<_>>();
    Text::from(lines)
}

fn render_input_line(line: &str, large_paste_placeholders: &[String]) -> Line<'static> {
    if line.is_empty() || large_paste_placeholders.is_empty() {
        return Line::from(line.to_string());
    }

    let mut spans = Vec::new();
    let mut cursor = 0usize;
    while cursor < line.len() {
        let mut next_match: Option<(usize, &str)> = None;
        for placeholder in large_paste_placeholders {
            let Some(offset) = line[cursor..].find(placeholder.as_str()) else {
                continue;
            };
            let match_start = cursor + offset;
            let should_replace = match next_match {
                None => true,
                Some((current_start, current_placeholder)) => {
                    match_start < current_start
                        || (match_start == current_start
                            && placeholder.len() > current_placeholder.len())
                }
            };
            if should_replace {
                next_match = Some((match_start, placeholder.as_str()));
            }
        }

        let Some((match_start, placeholder)) = next_match else {
            spans.push(Span::raw(line[cursor..].to_string()));
            break;
        };

        if match_start > cursor {
            spans.push(Span::raw(line[cursor..match_start].to_string()));
        }
        spans.push(Span::styled(placeholder.to_string(), theme::link_text()));
        cursor = match_start + placeholder.len();
    }

    if spans.is_empty() {
        Line::from(line.to_string())
    } else {
        Line::from(spans)
    }
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

fn build_right_aligned_footer_line(text: String, width: u16) -> Option<Line<'static>> {
    if width == 0 || text.trim().is_empty() {
        return None;
    }

    let total_width = usize::from(width);
    let text_width = UnicodeWidthStr::width(text.as_str());
    let padding = total_width.saturating_sub(text_width);
    Some(Line::from(vec![
        Span::raw(" ".repeat(padding)),
        Span::styled(text, theme::secondary_text()),
    ]))
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
        let remaining_width = usize::from(width).saturating_sub(used);
        if remaining_width == 0 {
            break;
        }

        if key.is_empty() {
            let available = remaining_width.saturating_sub(gap);
            let Some(rendered_label) = compact_footer_label(label.as_str(), available) else {
                continue;
            };
            let rendered_width = UnicodeWidthStr::width(rendered_label.as_str());
            if rendered_width == 0 || gap + rendered_width > remaining_width {
                continue;
            }
            if gap > 0 {
                spans.push(Span::styled("  ", theme::secondary_text()));
                used = used.saturating_add(gap);
            }
            spans.push(Span::styled(rendered_label, theme::secondary_text()));
            used = used.saturating_add(rendered_width);
            continue;
        }

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

    #[test]
    fn footer_can_render_right_context_without_left_items() {
        let line = build_right_aligned_footer_line("100% context left".to_string(), 32)
            .expect("footer line");
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(rendered.ends_with("100% context left"));
    }

    #[test]
    fn footer_supports_label_only_items_for_model_and_cwd() {
        let spans = build_footer_spans(
            vec![
                (String::new(), "gpt-5.1-codex".to_string()),
                (String::new(), "workspace/app".to_string()),
            ],
            80,
        );
        let rendered = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(rendered.contains("gpt-5.1-codex"));
        assert!(rendered.contains("workspace/app"));
    }
}
