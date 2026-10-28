use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::tui::activity_indicator;
use crate::tui::app::TuiApp;
use crate::tui::text_tabs;
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
    // The gap row between the transcript and the draft exists only while it has something
    // to say, so an idle composer keeps its full text band (codex composer_gap).
    let gap = app.composer_gap_line(area.width, is_zh);
    // Bands stack: a cloud failure (plan §6.2) gets its own row above the
    // warning counter so neither signal can erase the other.
    let mut constraints: Vec<Constraint> = Vec::new();
    if cloud.is_some() {
        constraints.push(Constraint::Length(1));
    }
    if warning.is_some() {
        constraints.push(Constraint::Length(1));
    }
    // The gap never wraps: its label ladder already shed words to fit the width, so the
    // band asks for the one row it can fill. Anything else would eat the draft's rows.
    if gap.is_some() {
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
    if let Some(gap_line) = gap {
        let gap_area = sections[next];
        next += 1;
        render_gap(frame, app, gap_area, gap_line);
    } else {
        // A hidden control owns nothing: an empty rectangle answers every pointer.
        app.set_gap_hit_area(Rect::default());
    }
    let (band, footer_area) = (sections[next], sections[next + 1]);
    frame.render_widget(Block::default().style(theme::composer_surface()), band);
    let input_area = Rect::new(
        band.x.saturating_add(1),
        band.y.saturating_add(1),
        band.width.saturating_sub(2),
        band.height
            .saturating_sub(2)
            .max(u16::from(band.height > 0)),
    );

    let prompt_width = UnicodeWidthStr::width(INPUT_PROMPT) as u16;
    let text_area = Rect {
        x: input_area.x.saturating_add(prompt_width),
        y: input_area.y,
        width: input_area.width.saturating_sub(prompt_width),
        height: input_area.height,
    };
    app.set_input_viewport(text_area.width.max(1));
    app.set_input_hit_area(text_area);
    let view = app.input_view(text_area.width.max(1), text_area.height.max(1));

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
                "输入消息，@ 提及文件，/ 查看命令"
            } else {
                "Ask anything, @ to mention files, / for commands"
            };
            Text::from(vec![Line::from(Span::styled(
                placeholder,
                theme::secondary_text(),
            ))])
        } else {
            let inline_placeholders = app.inline_input_placeholders();
            let lines = view
                .lines
                .iter()
                .map(|line| project_input_line(line, inline_placeholders.as_slice()))
                .collect::<Vec<_>>();
            Text::from(lines)
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
        let x = text_area.x + view.cursor_x.min(text_area.width.saturating_sub(1));
        let y = text_area.y + view.cursor_y.min(text_area.height.saturating_sub(1));
        frame.set_cursor_position((x, y));
    }
}

/// Paint the gap's single row centred over the draft, the way codex places the reading
/// control, and hand back the rectangle it owns for the next pointer event. A control the
/// reader can hover must also be a control they can press, so the rectangle and the painted
/// label are derived from the same line here.
fn render_gap(frame: &mut Frame, app: &mut TuiApp, area: Rect, line: Line<'static>) {
    if area.height == 0 || area.width == 0 {
        app.set_gap_hit_area(Rect::default());
        return;
    }
    let width = line.width().min(usize::from(area.width)) as u16;
    if width == 0 {
        app.set_gap_hit_area(Rect::default());
        return;
    }
    let target = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y,
        width,
        height: 1,
    };
    // Hover is reversed video plus weight: under `NO_COLOR` every hue degrades to Reset,
    // so the state a pointer leaves behind has to be carried by the modifier alone.
    let style = if app.gap_hovered(target) {
        theme::selection_mark().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    app.set_gap_hit_area(target);
    frame.render_widget(Paragraph::new(line.style(style)), target);
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

/// Project one visible draft line for the frame. First the placeholder split, so a
/// pasted-content badge keeps its link styling across the projection; then `expand_line`
/// carries every tab to `text_tabs`' eight-column stop. Ratatui measures the *projected*
/// line with plain `UnicodeWidthStr` maths (a tab reads zero-width there), so projecting
/// before `Paragraph` is what makes its wrap agree with the composer's own wrap and the
/// pointer hit test. The draft itself is never projected — `self.input`, copy and submit
/// keep the user's literal tabs.
///
/// The split works span by span and never rejoins the line first: the mark a pointer
/// gesture leaves on the draft rides the span's own style, so flattening the spans into one
/// string here would silently erase the selection the frame is supposed to show.
fn project_input_line(line: &Line<'_>, large_paste_placeholders: &[String]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for span in &line.spans {
        push_span_around_badges(
            &mut spans,
            span.content.as_ref(),
            span.style,
            large_paste_placeholders,
        );
    }
    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    text_tabs::expand_line(&Line::from(spans).style(line.style))
}

/// Cut one span's text at every badge, keeping the span's style on the plain parts and
/// patching the badge style onto the badge itself, so a selection mark and a link badge can
/// share a row without one of them losing.
fn push_span_around_badges(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    style: Style,
    large_paste_placeholders: &[String],
) {
    if text.is_empty() {
        return;
    }
    if large_paste_placeholders.is_empty() {
        spans.push(Span::styled(text.to_string(), style));
        return;
    }

    let mut cursor = 0usize;
    while cursor < text.len() {
        let mut next_match: Option<(usize, &str)> = None;
        for placeholder in large_paste_placeholders {
            let Some(offset) = text[cursor..].find(placeholder.as_str()) else {
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
            spans.push(Span::styled(text[cursor..].to_string(), style));
            break;
        };

        if match_start > cursor {
            spans.push(Span::styled(text[cursor..match_start].to_string(), style));
        }
        spans.push(Span::styled(
            placeholder.to_string(),
            style.patch(theme::link_text()),
        ));
        cursor = match_start + placeholder.len();
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
    fn projection_expands_tabs_to_stops_and_keeps_the_badge_span() {
        // gate 0..4, tab pads to 8, the badge (5 cells) sits at 8..13, its tab pads to 16.
        let line = Line::from("gate\t[p-1]\tend".to_string());
        let projected = project_input_line(&line, &["[p-1]".to_string()]);
        let text = render_spans(projected.spans.clone());
        assert_eq!(text, "gate    [p-1]   end");
        assert!(!text.contains('\t'), "the frame text carries no tab cell");
        let badge = projected
            .spans
            .iter()
            .find(|span| span.content.contains("[p-1]"))
            .expect("the badge keeps its own span");
        assert_eq!(
            badge.style.fg,
            theme::link_text().fg,
            "projection may not repaint the placeholder"
        );
        assert_eq!(
            line.spans.iter().next().unwrap().content.as_ref(),
            "gate\t[p-1]\tend",
            "projection never rewrites the logical line it was handed"
        );
    }

    #[test]
    fn the_projection_keeps_a_selection_mark_on_its_own_run() {
        use ratatui::style::Modifier;

        // The draft line arrives with the mark already on the selected bytes: the projection
        // may not flatten the spans, or the selection the reader dragged out disappears.
        let line = Line::from(vec![
            Span::raw("gate\t"),
            Span::styled(
                "sel\tected",
                crate::tui::theme::selection_mark().patch(theme::link_text()),
            ),
            Span::raw(" tail"),
        ]);
        let projected = project_input_line(&line, &[]);
        let marked = projected
            .spans
            .iter()
            .filter(|span| span.style.add_modifier.contains(Modifier::REVERSED))
            .collect::<Vec<_>>();
        assert_eq!(marked.len(), 1, "exactly the marked run carries the style");
        let text = marked[0].content.as_ref();
        assert!(
            text.contains("sel") && text.contains("ected"),
            "the run keeps its characters: {text:?}"
        );
        assert!(
            !text.contains('\t'),
            "and the tab inside it was projected like every other cell: {text:?}"
        );
        assert!(
            !projected.spans[0]
                .style
                .add_modifier
                .contains(Modifier::REVERSED),
            "the run before it stays unmarked"
        );
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
        assert_eq!(warning.style.fg, theme::warning_text().fg);
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
