//! 居中选择面板：对齐 codex 的居中对话框与 Panel 表面。
//!
//! 确认与选择类视图浮在整个视口中央并保留底层内容为背景；对话框宽度封顶
//! 72 列、两侧各留 2 列视口边距，面板底色为终端底色的浅色叠层。列表行是
//! `› N. 名称  描述` 的稳定列布局，描述列变暗、过窄时整列隐藏，选中行整行
//! 填充；列表上下沿在溢出时渲染 ↑/↓ 指示，底部给出 dim 的按键提示。

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::tui::theme;

/// 对话框最大列宽，与 codex 的居中对话框一致。
const DIALOG_MAX_WIDTH: u16 = 72;
/// 对话框两侧各留出的视口边距。
const DIALOG_VIEWPORT_INSET: u16 = 4;
/// 面板内容两侧内边距（各 2 列）。
const CONTENT_HORIZONTAL_INSET: u16 = 2;
/// 描述列之后至少要剩下的列数，低于该值整列隐藏。
const MIN_DESCRIPTION_WIDTH: usize = 24;
/// 列表默认可见行数，与 codex 的 MAX_POPUP_ROWS 一致。
pub(crate) const MAX_VISIBLE_ROWS: usize = 8;

#[derive(Debug, Clone)]
pub(crate) struct PickerRow {
    /// 行首编号（如 `1`）；`None` 渲染为等宽空槽位。
    pub(crate) gutter: Option<String>,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

impl PickerRow {
    pub(crate) fn numbered(index: usize, name: String, description: Option<String>) -> Self {
        Self {
            gutter: Some((index + 1).to_string()),
            name,
            description,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BodyLine {
    pub(crate) text: String,
    pub(crate) style: Style,
}

impl BodyLine {
    pub(crate) fn accent(text: String) -> Self {
        Self {
            text,
            style: theme::accent_text(),
        }
    }

    pub(crate) fn dim(text: String) -> Self {
        Self {
            text,
            style: theme::secondary_text(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CenteredListSpec {
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) body: Vec<BodyLine>,
    pub(crate) rows: Vec<PickerRow>,
    pub(crate) selected: usize,
    pub(crate) footer_note: Option<String>,
    pub(crate) footer_hint: Option<String>,
    pub(crate) max_visible_rows: usize,
}

/// codex 的居中对话框几何：宽度 = 视口宽 - 4，封顶 72 列，双轴居中。
pub(crate) fn dialog_width(viewport: Rect) -> u16 {
    viewport
        .width
        .saturating_sub(DIALOG_VIEWPORT_INSET)
        .max(viewport.width.min(8))
        .min(DIALOG_MAX_WIDTH)
}

/// 在视口中为一段内容求居中对话框矩形（信息类面板用，如快捷键/警告）。
pub(crate) fn centered_surface_rect(viewport: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(dialog_width(viewport)).max(1);
    let height = height.min(viewport.height.saturating_sub(2)).max(1);
    Rect {
        x: viewport.x + viewport.width.saturating_sub(width) / 2,
        y: viewport.y + viewport.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

/// 清空并铺上 Panel 表面底色，返回内容区（上下各 1 行、左右各 2 列内边距）。
pub(crate) fn paint_panel(frame: &mut Frame, dialog: Rect) -> Rect {
    frame.render_widget(Clear, dialog);
    frame.render_widget(
        Block::default().style(theme::panel_surface()),
        dialog,
    );
    Rect::new(
        dialog.x + CONTENT_HORIZONTAL_INSET,
        dialog.y + 1,
        dialog.width.saturating_sub(CONTENT_HORIZONTAL_INSET * 2),
        dialog.height.saturating_sub(2),
    )
}

pub(crate) fn draw(frame: &mut Frame, viewport: Rect, spec: &CenteredListSpec) {
    if viewport.width < 24 || viewport.height < 8 {
        return;
    }
    let width = dialog_width(viewport);
    let content_width = width.saturating_sub(CONTENT_HORIZONTAL_INSET * 2).max(1) as usize;

    let title_h = usize::from(!spec.title.trim().is_empty());
    let subtitle_h = usize::from(
        spec.subtitle
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty()),
    );
    let body_heights: Vec<usize> = spec
        .body
        .iter()
        .map(|line| wrapped_line_count(line.text.as_str(), content_width))
        .collect();
    let note_h = spec
        .footer_note
        .as_ref()
        .map(|text| wrapped_line_count(text, content_width))
        .unwrap_or(0);
    let hint_h = usize::from(
        spec.footer_hint
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty()),
    );

    let has_header = title_h + subtitle_h > 0;
    let has_rows = !spec.rows.is_empty();
    let has_footer = note_h + hint_h > 0;
    let max_content_height = usize::from(viewport.height).saturating_sub(2);
    let rows_len = spec.rows.len();
    let rows_wanted = if has_rows {
        spec.max_visible_rows.max(1).min(rows_len)
    } else {
        0
    };

    // 高度与下方 y 游标走同一套表达式：标题/副标题、一行空隙、正文、
    // 一行空隙、可见行、一行空隙、底注与按键提示。
    // 列表上下的空隙行同时承担滚动指示（codex 的做法）。
    let total_for = |body_lines: usize, rows: usize| -> usize {
        let body_sum: usize = body_heights.iter().take(body_lines).sum();
        let gap_above_list = usize::from(rows > 0 && (has_header || body_lines > 0));
        let gap_before_footer = usize::from(has_footer && (rows > 0 || body_lines > 0));
        2 + title_h
            + subtitle_h
            + usize::from(has_header)
            + body_sum
            + gap_above_list
            + gap_before_footer
            + note_h
            + hint_h
            + rows
    };

    // 短屏兜底：先截正文尾部（保留首行，提示与选项行不动），再压可见行数，
    // 保证面板不超出视口、底部提示不被裁掉。
    let mut body_keep = spec.body.len();
    let mut visible = rows_wanted;
    while total_for(body_keep, visible) > max_content_height {
        if body_keep > 1 {
            body_keep -= 1;
        } else if visible > 1 {
            visible -= 1;
        } else {
            break;
        }
    }

    let height = total_for(body_keep, visible);
    let dialog = Rect {
        x: viewport.x + viewport.width.saturating_sub(width) / 2,
        y: viewport.y + viewport.height.saturating_sub(height as u16) / 2,
        width,
        height: height as u16,
    };
    let content = paint_panel(frame, dialog);

    let selected = has_rows.then(|| spec.selected.min(rows_len - 1));
    let start = selected.map_or(0, |selected| {
        if selected < visible {
            0
        } else {
            (selected + 1 - visible).min(rows_len - visible)
        }
    });
    let has_above = has_rows && start > 0;
    let has_below = has_rows && start + visible < rows_len;

    let mut y = content.y;
    let paint_text = |frame: &mut Frame, y: u16, line: Line<'static>| {
        frame.render_widget(
            Paragraph::new(line).wrap(Wrap { trim: false }),
            Rect::new(content.x, y, content.width, 1),
        );
        1
    };

    if title_h > 0 {
        y += paint_text(
            frame,
            y,
            Line::from(Span::styled(
                spec.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )),
        );
    }
    if subtitle_h > 0 {
        if let Some(subtitle) = spec.subtitle.as_deref() {
            y += paint_text(
                frame,
                y,
                Line::from(Span::styled(subtitle.to_string(), theme::secondary_text())),
            );
        }
    }
    if has_header {
        y += 1;
    }
    for (line, line_height) in spec.body.iter().zip(body_heights.iter()).take(body_keep) {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(line.text.clone(), line.style)))
                .wrap(Wrap { trim: false }),
            Rect::new(content.x, y, content.width, *line_height as u16),
        );
        y += *line_height as u16;
    }
    if visible > 0 && (has_header || body_keep > 0) {
        y += 1;
    }

    // 列表：滚动指示画在列表上方/下方的空隙行行首，与 codex 一致。
    // 描述列起点按全部行测量（codex 的 AutoAllRows），滚动时列保持稳定。
    let gutter_width = spec
        .rows
        .iter()
        .filter_map(|row| row.gutter.as_deref().map(UnicodeWidthStr::width))
        .max()
        .unwrap_or(0);
    let prefix_width = 4 + gutter_width;
    let desc_col = spec
        .rows
        .iter()
        .map(|row| {
            prefix_width
                + UnicodeWidthStr::width(
                    truncate_with_ellipsis(
                        row.name.as_str(),
                        content_width.saturating_sub(prefix_width),
                    )
                    .as_str(),
                )
                + 2
        })
        .max()
        .unwrap_or(prefix_width);
    let show_description = desc_col <= content_width
        && content_width.saturating_sub(desc_col) >= MIN_DESCRIPTION_WIDTH;

    let indicator_y_above = y.saturating_sub(1);
    let layout = RowLayout {
        prefix_width,
        desc_col,
        show_description,
    };
    for (offset, row) in spec.rows.iter().skip(start).take(visible).enumerate() {
        paint_row(
            frame,
            content,
            y + offset as u16,
            row,
            Some(start + offset) == selected,
            &layout,
        );
    }
    y += visible as u16;
    let indicator_y_below = y;
    if has_footer && (visible > 0 || body_keep > 0) {
        y += 1;
    }
    if let Some(note) = spec.footer_note.as_deref() {
        for wrapped in textwrap::wrap(note, content_width.max(1)) {
            y += paint_text(
                frame,
                y,
                Line::from(Span::styled(wrapped.to_string(), theme::secondary_text())),
            );
        }
    }
    if let Some(hint) = spec.footer_hint.as_deref() {
        if !hint.trim().is_empty() {
            paint_text(
                frame,
                y,
                Line::from(Span::styled(hint.to_string(), theme::secondary_text())),
            );
        }
    }

    if has_above {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("↑", theme::accent_text()))),
            Rect::new(content.x, indicator_y_above, 1, 1),
        );
    }
    if has_below {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("↓", theme::accent_text()))),
            Rect::new(content.x, indicator_y_below, 1, 1),
        );
    }
}

/// 一行渲染所需的共享列度量：编号槽宽、描述列起点、描述列是否可见。
struct RowLayout {
    prefix_width: usize,
    desc_col: usize,
    show_description: bool,
}

fn paint_row(frame: &mut Frame, content: Rect, y: u16, row: &PickerRow, selected: bool, layout: &RowLayout) {
    let RowLayout {
        prefix_width,
        desc_col,
        show_description,
    } = *layout;
    let row_rect = Rect::new(content.x, y, content.width, 1);
    // 先铺行底：未选中行吃面板底色，选中行整行填充高亮。
    frame.render_widget(
        Block::default().style(if selected {
            theme::picker_selected()
        } else {
            theme::panel_surface()
        }),
        row_rect,
    );

    let gutter_text = match row.gutter.as_deref() {
        Some(gutter) => format!("{gutter}. "),
        None => " ".repeat(prefix_width - 2),
    };
    let content_width = content.width as usize;

    let (prefix_style, name_style, desc_style) = if selected {
        (
            theme::picker_selected(),
            theme::picker_selected(),
            theme::picker_selected().remove_modifier(Modifier::BOLD),
        )
    } else {
        (
            theme::secondary_text(),
            Style::default(),
            theme::secondary_text(),
        )
    };

    let name_limit = if show_description {
        desc_col.saturating_sub(prefix_width).saturating_sub(1)
    } else {
        content_width.saturating_sub(prefix_width)
    };
    let name = truncate_with_ellipsis(row.name.as_str(), name_limit);

    let mut spans = vec![
        Span::styled(if selected { "›" } else { " " }, prefix_style),
        Span::styled(" ", prefix_style),
        Span::styled(gutter_text, prefix_style),
        Span::styled(name.clone(), name_style),
    ];
    if let Some(description) = row
        .description
        .as_deref()
        .filter(|text| show_description && !text.trim().is_empty())
    {
        let name_width = UnicodeWidthStr::width(name.as_str());
        let desc_limit = content_width.saturating_sub(desc_col).max(MIN_DESCRIPTION_WIDTH);
        let pad = " ".repeat(desc_col.saturating_sub(prefix_width + name_width));
        spans.push(Span::styled(pad, desc_style));
        spans.push(Span::styled(
            truncate_with_ellipsis(description, desc_limit),
            desc_style,
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), row_rect);
}

fn truncate_with_ellipsis(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let display_width = |text: &str| -> usize {
        text.chars()
            .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
            .sum()
    };
    if display_width(text) <= limit {
        return text.to_string();
    }
    // 截断后以省略号收尾，总宽不超限。
    let mut width = 0usize;
    let mut result = String::new();
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > limit.saturating_sub(1) {
            break;
        }
        width += ch_width;
        result.push(ch);
    }
    result.push('…');
    result
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    if text.trim().is_empty() {
        return 1;
    }
    textwrap::wrap(text, width.max(1)).len().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn spec(rows: Vec<PickerRow>) -> CenteredListSpec {
        CenteredListSpec {
            title: "标题".to_string(),
            subtitle: None,
            body: Vec::new(),
            rows,
            selected: 0,
            footer_note: None,
            footer_hint: Some("enter select · esc back".to_string()),
            max_visible_rows: MAX_VISIBLE_ROWS,
        }
    }

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
    fn dialog_width_follows_codex_geometry() {
        let viewport = Rect::new(0, 0, 120, 30);
        assert_eq!(dialog_width(viewport), 72);
        let viewport = Rect::new(0, 0, 60, 30);
        assert_eq!(dialog_width(viewport), 56);
    }

    #[test]
    fn centered_surface_rect_clamps_to_viewport() {
        let rect = centered_surface_rect(Rect::new(0, 0, 80, 24), 200, 40);
        assert_eq!(rect.width, 72);
        assert_eq!(rect.height, 22);
        assert_eq!(rect.x, 4);
        assert_eq!(rect.y, 1);
    }

    #[test]
    fn selected_row_carries_chevron_and_full_width_hint_rows_render() {
        let rows = vec![
            PickerRow::numbered(0, "alpha".to_string(), None),
            PickerRow::numbered(1, "beta".to_string(), Some("second".to_string())),
        ];
        let mut listed = spec(rows);
        listed.selected = 1;
        let backend = TestBackend::new(60, 16);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| draw(frame, frame.area(), &listed))
            .expect("draw");

        let lines = buffer_lines(&terminal);
        assert!(
            lines.iter().any(|line| line.contains("1. alpha")),
            "unselected rows keep numbers: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("› 2. beta")),
            "the selected row keeps its number next to the chevron: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("second")),
            "the description column renders: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("enter select")),
            "the footer hint renders: {lines:?}"
        );
    }

    #[test]
    fn overflow_renders_scroll_indicators() {
        let rows = (0..12)
            .map(|index| PickerRow::numbered(index, format!("row {index}"), None))
            .collect();
        let listed = spec(rows);
        let backend = TestBackend::new(60, 16);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| draw(frame, frame.area(), &listed))
            .expect("draw");

        let lines = buffer_lines(&terminal);
        assert!(
            lines.iter().any(|line| line.contains('↓')),
            "a below indicator shows while more rows follow: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains('↑')),
            "no above indicator while the list sits at the top: {lines:?}"
        );
    }

    #[test]
    fn truncation_keeps_display_width() {
        assert_eq!(truncate_with_ellipsis("hello", 10), "hello");
        assert_eq!(truncate_with_ellipsis("hello", 4), "hel…");
        assert_eq!(truncate_with_ellipsis("你好世界", 5), "你好…");
    }
}

