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

    let warning = app.warning_notice(is_zh, area.width);
    let cloud = app.cloud_error_notice().map(str::to_string);
    // Bands stack: a cloud failure (plan §6.2) gets its own row above the
    // warning counter so neither signal can erase the other.
    let mut constraints: Vec<Constraint> = Vec::new();
    if cloud.is_some() {
        constraints.push(Constraint::Length(1));
    }
    if warning.is_some() {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(1));
    constraints.push(Constraint::Length(1));
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    let mut next = 0usize;
    if let Some(text) = cloud.as_ref() {
        let cloud_area = sections[next];
        next += 1;
        let cloud_line = Line::from(Span::styled(text.clone(), theme::warning_text()));
        frame.render_widget(Paragraph::new(cloud_line), cloud_area);
    }
    if let Some((count, hint)) = warning.as_ref() {
        let warning_area = sections[next];
        next += 1;
        let notice_line = Line::from(vec![
            Span::styled(count.clone(), theme::warning_text()),
            Span::styled(hint.clone(), theme::secondary_text()),
        ]);
        frame.render_widget(Paragraph::new(notice_line), warning_area);
    }
    let (input_area, footer_area) = (sections[next], sections[next + 1]);

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
        let prompt_style =
            theme::composer_prompt(app.input_focus_active()).patch(theme::composer_surface());
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
            let placeholder = if app.read_only_notice().is_some() {
                crate::tui::app::read_only_placeholder(is_zh)
            } else if is_zh {
                "直接提问，或使用 / 命令、@ 文件、# 技能、$ 应用；拖入图片或文件即可附加"
            } else {
                "Ask directly, or use / commands, @ files, # skills, and $ apps; drop images/files to attach"
            };
            Text::from(vec![Line::from(Span::styled(
                placeholder,
                theme::secondary_text(),
            ))])
        } else {
            let inline_placeholders = app.inline_input_placeholders();
            render_input_text(input_text.as_str(), inline_placeholders.as_slice())
        };

        frame.render_widget(
            Paragraph::new(body)
                .wrap(Wrap { trim: false })
                .style(theme::composer_surface()),
            text_area,
        );
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

fn build_footer_line(app: &mut TuiApp, width: u16) -> Option<Line<'static>> {
    if width == 0 {
        return None;
    }

    let items = app.composer_footer_items();

    if let Some(right_spans) = app.composer_footer_context() {
        if let Some(line) = build_footer_line_with_right(items.clone(), right_spans.clone(), width)
        {
            return Some(line);
        }
        if let Some(line) = build_right_aligned_footer_line(right_spans, width) {
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
    right_spans: Vec<Span<'static>>,
    width: u16,
) -> Option<Line<'static>> {
    let total_width = usize::from(width.max(1));
    let right_width: usize = right_spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
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
    spans.extend(right_spans);
    Some(Line::from(spans))
}

fn build_right_aligned_footer_line(
    right_spans: Vec<Span<'static>>,
    width: u16,
) -> Option<Line<'static>> {
    if width == 0
        || right_spans
            .iter()
            .all(|span| span.content.trim().is_empty())
    {
        return None;
    }

    let total_width = usize::from(width);
    let text_width: usize = right_spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
    let padding = total_width.saturating_sub(text_width);
    let mut spans = vec![Span::raw(" ".repeat(padding))];
    spans.extend(right_spans);
    Some(Line::from(spans))
}

fn spans_width(spans: &[Span<'static>]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn build_footer_spans(items: Vec<(String, String)>, width: u16) -> Vec<Span<'static>> {
    let total_width = usize::from(width);
    let key_floors = footer_key_floors(items.as_slice());
    let mut spans = Vec::new();
    let mut used = 0usize;

    for (index, (key, label)) in items.into_iter().enumerate() {
        let gap = if spans.is_empty() { 0 } else { 2 };
        let remaining = total_width.saturating_sub(used).saturating_sub(gap);
        if remaining == 0 {
            break;
        }
        // A description only earns its space when every shortcut key that follows it can
        // still be shown (plan §3.1: shed text, then low-priority items, never the key).
        let reserved_for_later_keys = key_floors[index];
        let key_width = UnicodeWidthStr::width(key.as_str());

        if key.is_empty() {
            let Some(rendered_label) = compact_footer_label(
                label.as_str(),
                remaining.saturating_sub(reserved_for_later_keys),
            ) else {
                continue;
            };
            let rendered_width = UnicodeWidthStr::width(rendered_label.as_str());
            if rendered_width == 0
                || rendered_width.saturating_add(reserved_for_later_keys) > remaining
            {
                continue;
            }
            push_footer_gap(&mut spans, &mut used, gap);
            spans.push(Span::styled(rendered_label, theme::secondary_text()));
            used = used.saturating_add(rendered_width);
            continue;
        }

        if key_width > remaining {
            break;
        }
        let label_budget = remaining
            .saturating_sub(key_width)
            .saturating_sub(1)
            .saturating_sub(reserved_for_later_keys);
        let rendered_label = if label.is_empty() || label_budget == 0 {
            None
        } else {
            compact_footer_label(label.as_str(), label_budget)
        };

        push_footer_gap(&mut spans, &mut used, gap);
        spans.push(Span::styled(key, theme::accent_text()));
        used = used.saturating_add(key_width);
        if let Some(label) = rendered_label {
            let rendered_width = UnicodeWidthStr::width(label.as_str());
            spans.push(Span::raw(" "));
            spans.push(Span::styled(label, theme::secondary_text()));
            used = used.saturating_add(1 + rendered_width);
        }
    }

    spans
}

/// Width the key-only form of every shortcut item after `index` will still require.
fn footer_key_floors(items: &[(String, String)]) -> Vec<usize> {
    let mut floors = vec![0usize; items.len()];
    let mut running = 0usize;
    for index in (0..items.len()).rev() {
        floors[index] = running;
        let key = items[index].0.as_str();
        if !key.is_empty() {
            running = running.saturating_add(2 + UnicodeWidthStr::width(key));
        }
    }
    floors
}

fn push_footer_gap(spans: &mut Vec<Span<'static>>, used: &mut usize, gap: usize) {
    if gap == 0 {
        return;
    }
    spans.push(Span::styled(" ".repeat(gap), theme::secondary_text()));
    *used = used.saturating_add(gap);
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
        // Every key keeps its place; descriptions give way first, even the leading one.
        assert!(rendered.contains('@'));
        assert!(rendered.contains("Ctrl+V"));
        assert!(rendered.contains("Tab"));
        assert!(!rendered.contains("images"));
        assert!(!rendered.contains("complete"));
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
            vec![Span::styled(
                "ctx 72% · att 2".to_string(),
                theme::secondary_text(),
            )],
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
    fn footer_keeps_the_warning_span_of_the_right_context_intact() {
        // The reconnecting badge arrives as its own span so its warning colour
        // survives the right-alignment path.
        let line = build_right_aligned_footer_line(
            vec![
                Span::styled("72%".to_string(), theme::secondary_text()),
                Span::styled(" · ".to_string(), theme::secondary_text()),
                Span::styled("☁ 重连中".to_string(), theme::warning_text()),
            ],
            32,
        )
        .expect("footer line");
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(rendered.ends_with("72% · ☁ 重连中"));
        let warning = line
            .spans
            .iter()
            .find(|span| span.content.contains("重连中"))
            .expect("badge span");
        assert_eq!(warning.style.fg, Some(ratatui::style::Color::Yellow));
    }

    #[test]
    fn footer_can_render_right_context_without_left_items() {
        let line = build_right_aligned_footer_line(
            vec![Span::styled(
                "100% context left".to_string(),
                theme::secondary_text(),
            )],
            32,
        )
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

    #[test]
    fn footer_keeps_keys_when_only_keys_fit() {
        let spans = build_footer_spans(
            vec![
                (String::new(), "a long working directory".to_string()),
                (
                    "\u{2190}".to_string(),
                    "command center /threads".to_string(),
                ),
                ("?".to_string(), "for shortcuts".to_string()),
            ],
            6,
        );
        let rendered = render_spans(spans);
        assert!(rendered.contains('\u{2190}'), "{rendered}");
        assert!(rendered.contains('?'), "{rendered}");
        assert!(!rendered.contains("command center"), "{rendered}");
        assert!(!rendered.contains("working directory"), "{rendered}");
    }

    #[test]
    fn footer_sheds_descriptions_before_keys_when_crowded() {
        let spans = build_footer_spans(
            vec![
                (
                    String::new(),
                    "a very long working directory path".to_string(),
                ),
                (
                    "\u{2190}".to_string(),
                    "command center /threads".to_string(),
                ),
                ("?".to_string(), "for shortcuts".to_string()),
            ],
            24,
        );
        let rendered = render_spans(spans);
        assert!(rendered.contains('\u{2190}'), "{rendered}");
        assert!(rendered.contains('?'), "{rendered}");
        assert!(!rendered.contains("command center /threads"), "{rendered}");
        assert!(!rendered.contains("for shortcuts"), "{rendered}");
    }
}
