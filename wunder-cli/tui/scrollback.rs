use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use ratatui::Terminal;
use std::io;

const MAX_SCROLLBACK_INSERT_LINES: usize = 256;

pub(crate) fn insert_history_lines(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    lines: Vec<Line<'static>>,
) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }

    // Chunk insertion so huge transcripts do not allocate oversized temporary buffers.
    for chunk in lines.chunks(MAX_SCROLLBACK_INSERT_LINES) {
        let chunk_lines = chunk.to_vec();
        let height = chunk_lines.len().min(u16::MAX as usize) as u16;
        terminal.insert_before(height, move |buffer: &mut Buffer| {
            Paragraph::new(Text::from(chunk_lines))
                .wrap(Wrap { trim: false })
                .render(buffer.area, buffer);
        })?;
    }

    Ok(())
}
