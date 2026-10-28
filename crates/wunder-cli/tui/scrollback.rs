use ratatui::backend::Backend;
use ratatui::buffer::Buffer;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use ratatui::Terminal;
use std::io;

use super::text_tabs;

const MAX_SCROLLBACK_INSERT_LINES: usize = 256;

pub(crate) fn insert_history_lines<B: Backend>(
    terminal: &mut Terminal<B>,
    lines: Vec<Line<'static>>,
) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }

    terminal.autoresize()?;
    let width = terminal.get_frame().area().width.max(1);
    let mut rows = Vec::with_capacity(MAX_SCROLLBACK_INSERT_LINES);
    for line in lines {
        let expanded = text_tabs::expand_line(&line);
        if expanded.width() <= usize::from(width) {
            rows.push(expanded);
            if rows.len() == MAX_SCROLLBACK_INSERT_LINES {
                insert_rows(terminal, std::mem::take(&mut rows))?;
            }
            continue;
        }
        let paragraph = Paragraph::new(expanded).wrap(Wrap { trim: false });
        let height = paragraph.line_count(width).max(1);
        let mut offset = 0usize;
        while offset < height {
            let count = (height - offset).min(MAX_SCROLLBACK_INSERT_LINES);
            let mut buffer = Buffer::empty(ratatui::layout::Rect::new(0, 0, width, count as u16));
            paragraph
                .clone()
                .scroll((offset.min(u16::MAX as usize) as u16, 0))
                .render(buffer.area, &mut buffer);
            for row in 0..count as u16 {
                let mut spans = Vec::new();
                let mut column = 0u16;
                while column < width {
                    let cell = &buffer[(column, row)];
                    spans.push(ratatui::text::Span::styled(
                        cell.symbol().to_string(),
                        cell.style(),
                    ));
                    column += unicode_width::UnicodeWidthStr::width(cell.symbol()).max(1) as u16;
                }
                rows.push(Line::from(spans));
                if rows.len() == MAX_SCROLLBACK_INSERT_LINES {
                    insert_rows(terminal, std::mem::take(&mut rows))?;
                }
            }
            offset += count;
        }
    }
    insert_rows(terminal, rows)
}

fn insert_rows<B: Backend>(terminal: &mut Terminal<B>, rows: Vec<Line<'static>>) -> io::Result<()> {
    if !rows.is_empty() {
        terminal.insert_before(rows.len() as u16, move |buffer: &mut Buffer| {
            Paragraph::new(Text::from(rows)).render(buffer.area, buffer);
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::{TerminalOptions, Viewport};

    #[test]
    fn wrapped_history_keeps_every_row_across_insert_chunks() {
        let mut terminal = Terminal::with_options(
            TestBackend::new(20, 6),
            TerminalOptions {
                viewport: Viewport::Inline(6),
            },
        )
        .expect("terminal");
        let lines = (0..300)
            .map(|index| Line::from(format!("row-{index:03} {} end-{index:03}", "x".repeat(25))))
            .collect();
        insert_history_lines(&mut terminal, lines).expect("insert history");
        let text = terminal
            .backend()
            .scrollback()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        for index in 0..300 {
            assert!(
                text.contains(&format!("row-{index:03}")),
                "missing row {index}"
            );
            assert!(
                text.contains(&format!("end-{index:03}")),
                "missing tail {index}"
            );
        }
    }
}
