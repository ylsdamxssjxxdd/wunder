use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::PopupRow;
use crate::tui::theme;

/// The right column never starts closer than this to the left one: a little
/// air is what makes the list read as two columns instead of one long line.
const COLUMN_GAP: usize = 2;
/// Room reserved for the selection marker in front of every row.
const MARKER_WIDTH: usize = 2;

pub(crate) fn draw(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    rows: &[PopupRow],
    selected: Option<usize>,
) {
    let width = area.width as usize;
    let mut lines = Vec::with_capacity(rows.len().saturating_add(1));
    if !title.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            title.trim().to_string(),
            theme::block_title(false),
        )));
    }

    // One column width for the whole list, so every description starts on the
    // same edge. Capped so a long path in an `@` popup cannot push the
    // descriptions off the screen.
    let label_budget = width.saturating_sub(MARKER_WIDTH + COLUMN_GAP + 8).max(1);
    let label_width = rows
        .iter()
        .map(|row| row.label.chars().count())
        .max()
        .unwrap_or(0)
        .min(label_budget);

    for (index, row) in rows.iter().enumerate() {
        let is_selected = selected.is_some_and(|current| current == index);
        let marker = if is_selected {
            Span::styled("\u{203a} ", theme::accent_text())
        } else {
            Span::raw("  ")
        };
        let label_style = if is_selected {
            theme::popup_selected()
        } else {
            theme::popup_item()
        };
        let label_span = Span::styled(fit(&row.label, label_width), label_style);

        let detail_budget = width.saturating_sub(MARKER_WIDTH + label_width + COLUMN_GAP);
        if row.detail.trim().is_empty() || detail_budget < 4 {
            lines.push(Line::from(vec![marker, label_span]));
            continue;
        }
        lines.push(Line::from(vec![
            marker,
            label_span,
            Span::raw(" ".repeat(COLUMN_GAP)),
            Span::styled(fit(&row.detail, detail_budget), theme::secondary_text()),
        ]));
    }

    // Every row is laid out to the popup's width already; wrapping would move
    // a description under the next row's label and break the column edge.
    let widget = Paragraph::new(lines);
    frame.render_widget(widget, area);
}

/// Pad to exactly `width` characters, or clip with an ellipsis when the text is
/// longer. Padding is what holds the second column still.
fn fit(text: &str, width: usize) -> String {
    let length = text.chars().count();
    if length == width {
        return text.to_string();
    }
    if length > width {
        if width == 0 {
            return String::new();
        }
        let mut clipped: String = text.chars().take(width.saturating_sub(1)).collect();
        clipped.push('\u{2026}');
        return clipped;
    }
    let mut padded = text.to_string();
    padded.push_str(&" ".repeat(width - length));
    padded
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

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
    fn draw_popup_renders_title_and_items() {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    Rect::new(0, 0, 40, 8),
                    " Skills ",
                    &[
                        PopupRow::from_label("#shell".to_string()),
                        PopupRow::from_label("#files".to_string()),
                    ],
                    Some(1),
                );
            })
            .expect("draw");

        let lines = buffer_lines(&terminal);
        assert!(lines.iter().any(|line| line.contains("Skills")));
        assert!(lines.iter().any(|line| line.contains("#shell")));
        assert!(lines.iter().any(|line| line.contains("#files")));
    }

    #[test]
    fn descriptions_start_on_one_column_for_every_row() {
        let backend = TestBackend::new(48, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    Rect::new(0, 0, 48, 8),
                    " Commands ",
                    &[
                        PopupRow {
                            label: "/model".to_string(),
                            detail: "choose the model".to_string(),
                        },
                        PopupRow {
                            label: "/permissions".to_string(),
                            detail: "choose what is allowed".to_string(),
                        },
                    ],
                    Some(0),
                );
            })
            .expect("draw");

        let lines = buffer_lines(&terminal);
        let model = lines
            .iter()
            .find(|line| line.contains("/model"))
            .expect("model row");
        let permissions = lines
            .iter()
            .find(|line| line.contains("/permissions"))
            .expect("permissions row");
        // The marker is a multi-byte glyph, so compare character columns.
        let column_of = |line: &str, needle: &str| {
            let byte = line.find(needle).expect("needle");
            line[..byte].chars().count()
        };
        assert_eq!(
            column_of(model, "choose the model"),
            column_of(permissions, "choose what is allowed"),
            "the second column must not shift with the label length: {model:?} / {permissions:?}"
        );
    }

    #[test]
    fn a_row_without_a_detail_keeps_its_label() {
        assert_eq!(fit("/help", 8), "/help   ");
        assert_eq!(fit("/a-very-long-command", 8), "/a-very\u{2026}");
    }
}
