use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::tui::theme;

pub(crate) fn draw_shortcuts_modal(
    frame: &mut Frame,
    area: Rect,
    lines: Vec<String>,
    _is_zh: bool,
) {
    if area.width < 20 || area.height < 8 {
        return;
    }
    let popup = centered_popup(area, lines.as_slice(), 24, 6);
    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(to_modal_lines(lines)).wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

pub(crate) fn draw_resume_modal(
    frame: &mut Frame,
    area: Rect,
    rows: Vec<String>,
    selected: usize,
    _is_zh: bool,
) {
    if area.width < 20 || area.height < 8 {
        return;
    }
    let modal_lines = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            if index == selected {
                format!("› {}. {row}", index + 1)
            } else {
                format!("  {}. {row}", index + 1)
            }
        })
        .collect::<Vec<_>>();
    let popup = centered_popup(area, modal_lines.as_slice(), 28, 6);
    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(to_modal_lines(modal_lines)).wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

pub(crate) fn draw_approval_modal(
    frame: &mut Frame,
    area: Rect,
    input_area: Rect,
    lines: Vec<String>,
    _is_zh: bool,
) {
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = anchored_popup(area, input_area, lines.as_slice(), 56, 9);
    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(to_modal_lines(lines)).wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

pub(crate) fn draw_inquiry_modal(
    frame: &mut Frame,
    area: Rect,
    input_area: Rect,
    lines: Vec<String>,
    _is_zh: bool,
) {
    if area.width < 24 || area.height < 8 {
        return;
    }
    let popup = anchored_popup(area, input_area, lines.as_slice(), 56, 8);
    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(to_modal_lines(lines)).wrap(Wrap { trim: false });
    frame.render_widget(widget, popup);
}

fn centered_popup(area: Rect, lines: &[String], min_width: u16, min_height: u16) -> Rect {
    let max_line_width = lines
        .iter()
        .map(String::as_str)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0) as u16;
    let width = max_line_width
        .saturating_add(2)
        .max(min_width)
        .min(area.width.saturating_sub(2));
    let height = (lines.len() as u16)
        .saturating_add(2)
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
        .saturating_add(2)
        .max(min_width)
        .min(horizontal_bounds.width.saturating_sub(2))
        .max(1);
    let height = (lines.len() as u16)
        .saturating_add(2)
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
    lines.into_iter().map(style_modal_line).collect()
}

fn style_modal_line(line: String) -> Line<'static> {
    let trimmed = line.trim_start();
    if line.trim().is_empty() {
        return Line::from(Span::raw(String::new()));
    }
    if let Some(rendered) = style_modal_option_line(line.as_str()) {
        return rendered;
    }
    if let Some(rendered) = style_modal_preview_line(line.as_str()) {
        return rendered;
    }
    if let Some(rendered) = style_modal_labeled_line(line.as_str()) {
        return rendered;
    }
    if trimmed.starts_with('>') || trimmed.starts_with('›') {
        return Line::from(Span::styled(line, theme::modal_selected()));
    }
    if trimmed.ends_with('?') || trimmed.ends_with('？') {
        return Line::from(Span::styled(line, theme::accent_text()));
    }
    if is_modal_section_heading(trimmed) {
        return Line::from(Span::styled(line, theme::accent_text()));
    }
    Line::from(Span::styled(line, theme::secondary_text()))
}

fn style_modal_option_line(line: &str) -> Option<Line<'static>> {
    let parsed = parse_modal_option_line(line)?;
    let option_style = if parsed.selected {
        theme::accent_text().add_modifier(ratatui::style::Modifier::REVERSED)
    } else {
        theme::accent_text()
    };
    let body_style = if parsed.selected {
        theme::modal_selected()
    } else {
        theme::secondary_text()
    };
    let mut spans = vec![Span::styled(
        parsed.leading.to_string(),
        theme::secondary_text(),
    )];
    if parsed.selected {
        spans.push(Span::styled("› ".to_string(), theme::modal_selected()));
    }
    spans.push(Span::styled(parsed.option.to_string(), option_style));
    spans.push(Span::styled(" ".to_string(), body_style));
    spans.push(Span::styled(parsed.body.to_string(), body_style));
    if let Some(tag) = parsed.recommended_tag {
        spans.push(Span::styled(" ".to_string(), body_style));
        let tag_style = if parsed.selected {
            theme::brand_text().add_modifier(ratatui::style::Modifier::REVERSED)
        } else {
            theme::brand_text()
        };
        spans.push(Span::styled(tag.to_string(), tag_style));
    }
    Some(Line::from(spans))
}

struct ParsedModalOptionLine<'a> {
    leading: &'a str,
    selected: bool,
    option: &'a str,
    body: &'a str,
    recommended_tag: Option<&'a str>,
}

fn parse_modal_option_line(line: &str) -> Option<ParsedModalOptionLine<'_>> {
    let trimmed = line.trim_start();
    let leading = &line[..line.len().saturating_sub(trimmed.len())];
    let (selected, option_src) = if let Some(rest) = trimmed.strip_prefix('>') {
        (true, rest.trim_start())
    } else if let Some(rest) = trimmed.strip_prefix('›') {
        (true, rest.trim_start())
    } else {
        (false, trimmed)
    };
    let digits = option_src
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    let delimiter = option_src[digits..].chars().next()?;
    if delimiter != ')' && delimiter != '.' {
        return None;
    }
    let delimiter_width = delimiter.len_utf8();
    let option = &option_src[..digits + delimiter_width];
    let rest = option_src[digits + delimiter_width..].trim_start();
    let (body, recommended_tag) = split_recommended_tag(rest);
    Some(ParsedModalOptionLine {
        leading,
        selected,
        option,
        body,
        recommended_tag,
    })
}

fn split_recommended_tag(text: &str) -> (&str, Option<&str>) {
    if let Some(body) = text.strip_suffix(" (recommended)") {
        return (body, Some("(recommended)"));
    }
    if let Some(body) = text.strip_suffix("（推荐）") {
        return (body, Some("（推荐）"));
    }
    (text, None)
}

fn style_modal_preview_line(line: &str) -> Option<Line<'static>> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let leading = line.len().saturating_sub(trimmed.len());
    let prefix = &line[..leading];

    if let Some(rest) = trimmed.strip_prefix("diff ") {
        return Some(Line::from(vec![
            Span::styled(prefix.to_string(), theme::secondary_text()),
            Span::styled("diff".to_string(), theme::accent_text()),
            Span::raw(" "),
            Span::styled(rest.to_string(), theme::secondary_text()),
        ]));
    }

    if let Some(rest) = trimmed.strip_prefix("@@") {
        let body = rest.trim_start();
        let mut spans = vec![
            Span::styled(prefix.to_string(), theme::secondary_text()),
            Span::styled("@@".to_string(), theme::accent_text()),
        ];
        if !body.is_empty() {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(body.to_string(), theme::secondary_text()));
        }
        return Some(Line::from(spans));
    }

    let marker = match trimmed.chars().next()? {
        'A' | 'M' | 'D' | 'R' | '\u{2026}' | '+' | '-' => trimmed.chars().next()?,
        _ => return None,
    };
    let rest = trimmed.get(marker.len_utf8()..)?.trim_start();
    let (prefix_style, marker_style, rest_style) = match marker {
        'A' => (
            theme::secondary_text(),
            theme::success_text().add_modifier(ratatui::style::Modifier::BOLD),
            theme::secondary_text(),
        ),
        'D' => (
            theme::secondary_text(),
            theme::danger_text(),
            theme::secondary_text(),
        ),
        'R' => (
            theme::secondary_text(),
            theme::brand_text().add_modifier(ratatui::style::Modifier::BOLD),
            theme::secondary_text(),
        ),
        'M' => (
            theme::secondary_text(),
            Style::default()
                .fg(ratatui::style::Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD),
            theme::secondary_text(),
        ),
        '+' => (
            theme::diff_added_prefix(),
            theme::diff_added_marker(),
            theme::diff_added_text(),
        ),
        '-' => (
            theme::diff_deleted_prefix(),
            theme::diff_deleted_marker(),
            theme::diff_deleted_text(),
        ),
        _ => (
            theme::secondary_text(),
            theme::secondary_text(),
            theme::secondary_text(),
        ),
    };
    Some(Line::from(vec![
        Span::styled(prefix.to_string(), prefix_style),
        Span::styled(marker.to_string(), marker_style),
        Span::styled(" ", prefix_style),
        Span::styled(rest.to_string(), rest_style),
    ]))
}
fn style_modal_labeled_line(line: &str) -> Option<Line<'static>> {
    let (label, value, delimiter) = split_modal_label(line)?;
    let label_style = if is_modal_primary_label(label.trim()) {
        theme::accent_text()
    } else {
        theme::secondary_text()
    };
    let value_style = if label.trim() == "tool" || label.trim() == "工具" {
        Style::default().add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        theme::secondary_text()
    };
    Some(Line::from(vec![
        Span::styled(format!("{label}{delimiter}"), label_style),
        Span::styled(value.to_string(), value_style),
    ]))
}

fn split_modal_label(line: &str) -> Option<(&str, &str, char)> {
    if let Some((label, value)) = line.split_once('：') {
        return Some((label, value, '：'));
    }
    if let Some((label, value)) = line.split_once(':') {
        return Some((label, value, ':'));
    }
    None
}

fn is_modal_primary_label(label: &str) -> bool {
    matches!(
        label,
        "工具"
            | "摘要"
            | "补丁预览"
            | "队列"
            | "问题"
            | "模式"
            | "tool"
            | "summary"
            | "Patch preview"
            | "queue"
            | "question"
            | "mode"
    )
}

fn is_modal_section_heading(line: &str) -> bool {
    line.starts_with("审批操作")
        || line.starts_with("审批选项")
        || line.starts_with("Approval actions")
        || line.starts_with("Approval options")
        || line == "Options:"
        || line == "选项："
        || line.starts_with("候选路由")
        || line == "Routes:"
        || line.starts_with("routes ")
        || line == "routes (Up/Down select, Enter send, or press number):"
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

    #[test]
    fn to_modal_lines_styles_patch_preview_marker() {
        let lines = to_modal_lines(vec!["  A src/main.rs".to_string()]);
        assert_eq!(lines[0].spans[1].content.as_ref(), "A");
        assert_eq!(lines[0].spans[3].content.as_ref(), "src/main.rs");
    }

    #[test]
    fn to_modal_lines_styles_real_diff_preview_rows() {
        let lines = to_modal_lines(vec![
            "  diff src/main.rs".to_string(),
            "  @@".to_string(),
            "  + new".to_string(),
            "  - old".to_string(),
        ]);
        assert_eq!(lines[0].spans[1].content.as_ref(), "diff");
        assert_eq!(lines[0].spans[3].content.as_ref(), "src/main.rs");
        assert_eq!(lines[1].spans[1].content.as_ref(), "@@");
        assert_eq!(lines[2].spans[1].content.as_ref(), "+");
        assert_eq!(lines[2].spans[3].content.as_ref(), "new");
        assert_eq!(lines[3].spans[1].content.as_ref(), "-");
        assert_eq!(lines[3].spans[3].content.as_ref(), "old");
    }

    #[test]
    fn to_modal_lines_styles_labeled_rows() {
        let lines = to_modal_lines(vec!["工具：read_file".to_string()]);
        assert_eq!(lines[0].spans[0].content.as_ref(), "工具：");
        assert_eq!(lines[0].spans[1].content.as_ref(), "read_file");
    }

    #[test]
    fn selected_option_line_keeps_structured_spans() {
        let lines = to_modal_lines(vec!["› 1. Route (recommended)".to_string()]);
        let rendered = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(rendered, "› 1. Route (recommended)");
        assert!(lines[0].spans.len() >= 5);
    }

    #[test]
    fn parse_modal_option_line_supports_chevron_marker() {
        let parsed = parse_modal_option_line("› 2. Proceed").expect("parsed option");
        assert!(parsed.selected);
        assert_eq!(parsed.option, "2.");
        assert_eq!(parsed.body, "Proceed");
    }
}
