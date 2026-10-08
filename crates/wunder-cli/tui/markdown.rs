use ratatui::text::Line;

pub(crate) fn append_markdown(
    markdown_source: &str,
    width: Option<usize>,
    lines: &mut Vec<Line<'static>>,
) {
    let rendered = super::markdown_render::render_markdown_text_with_width(markdown_source, width);
    super::line_utils::push_owned_lines(&rendered.lines, lines);
}
