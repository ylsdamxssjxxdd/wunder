use super::super::line_utils::line_to_static;
use super::super::wrapping::{word_wrap_line, RtOptions};
use pulldown_cmark::Alignment;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

const COLUMN_GAP: usize = 2;
const CELL_PADDING: usize = 1;
const MIN_COLUMN_WIDTH: usize = 3;
const HEADER_SEPARATOR: char = '━';
const BODY_SEPARATOR: char = '─';

#[derive(Clone, Debug, Default)]
pub(super) struct TableCell {
    lines: Vec<Line<'static>>,
}

impl TableCell {
    fn ensure_line(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(Line::default());
        }
    }

    pub(super) fn push_span(&mut self, span: Span<'static>) {
        self.ensure_line();
        if let Some(line) = self.lines.last_mut() {
            line.push_span(span);
        }
    }

    pub(super) fn hard_break(&mut self) {
        self.ensure_line();
        self.lines.push(Line::default());
    }

    fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn width(&self) -> usize {
        self.lines.iter().map(Line::width).max().unwrap_or(0)
    }
}

#[derive(Debug)]
pub(super) struct TableState {
    alignments: Vec<Alignment>,
    header: Option<Vec<TableCell>>,
    rows: Vec<Vec<TableCell>>,
    current_row: Option<Vec<TableCell>>,
    current_cell: Option<TableCell>,
    in_header: bool,
}

impl TableState {
    pub(super) fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            header: None,
            rows: Vec::new(),
            current_row: None,
            current_cell: None,
            in_header: false,
        }
    }

    pub(super) fn start_head(&mut self) {
        self.in_header = true;
        self.current_row = Some(Vec::new());
    }

    pub(super) fn end_head(&mut self) {
        self.finish_cell();
        if let Some(row) = self.current_row.take() {
            self.header = Some(row);
        }
        self.in_header = false;
    }

    pub(super) fn start_row(&mut self) {
        self.current_row = Some(Vec::new());
    }

    pub(super) fn end_row(&mut self) {
        self.finish_cell();
        let Some(row) = self.current_row.take() else {
            return;
        };
        if self.in_header {
            self.header = Some(row);
        } else {
            self.rows.push(row);
        }
    }

    pub(super) fn start_cell(&mut self) {
        self.current_cell = Some(TableCell::default());
    }

    pub(super) fn end_cell(&mut self) {
        self.finish_cell();
    }

    fn finish_cell(&mut self) {
        let Some(cell) = self.current_cell.take() else {
            return;
        };
        self.current_row.get_or_insert_with(Vec::new).push(cell);
    }

    pub(super) fn in_cell(&self) -> bool {
        self.current_cell.is_some()
    }

    pub(super) fn cell_mut(&mut self) -> Option<&mut TableCell> {
        self.current_cell.as_mut()
    }
}

pub(super) struct RenderedTable {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) prewrapped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColumnKind {
    Narrative,
    TokenHeavy,
    Compact,
}

#[derive(Clone, Debug)]
struct ColumnMetrics {
    max_width: usize,
    header_token_width: usize,
    body_token_width: usize,
    kind: ColumnKind,
}

pub(super) fn render(
    mut state: TableState,
    wrap_width: Option<usize>,
    prefix_width: usize,
    header_style: Style,
    separator_style: Style,
) -> RenderedTable {
    let column_count = state.alignments.len();
    if column_count == 0 {
        return RenderedTable {
            lines: Vec::new(),
            prewrapped: true,
        };
    }

    let mut header = state
        .header
        .take()
        .unwrap_or_else(|| vec![TableCell::default(); column_count]);
    normalize_row(&mut header, column_count);
    for row in &mut state.rows {
        normalize_row(row, column_count);
    }
    for cell in std::iter::once(&mut header)
        .flatten()
        .chain(state.rows.iter_mut().flatten())
    {
        cell.ensure_line();
    }

    let metrics = collect_metrics(&header, &state.rows, column_count);
    let available = wrap_width.map(|width| {
        width.saturating_sub(
            prefix_width
                + column_count.saturating_sub(1) * COLUMN_GAP
                + column_count * CELL_PADDING * 2,
        )
    });
    let widths = compute_column_widths(&metrics, available);
    let record_width = wrap_width.map(|width| width.saturating_sub(prefix_width));

    let Some(widths) = widths else {
        if !state.rows.is_empty() {
            return RenderedTable {
                lines: render_records(
                    &header,
                    &state.rows,
                    &metrics,
                    record_width,
                    header_style,
                    separator_style,
                ),
                prewrapped: true,
            };
        }
        return RenderedTable {
            lines: pipe_fallback(&header, &state.alignments),
            prewrapped: false,
        };
    };

    if should_render_records(&state.rows, &widths, &metrics) {
        return RenderedTable {
            lines: render_records(
                &header,
                &state.rows,
                &metrics,
                record_width,
                header_style,
                separator_style,
            ),
            prewrapped: true,
        };
    }

    let mut lines = Vec::with_capacity(2 + state.rows.len() * 2);
    lines.extend(render_row(
        &header,
        &widths,
        &state.alignments,
        header_style,
    ));
    lines.push(render_separator(&widths, HEADER_SEPARATOR, separator_style));
    for (index, row) in state.rows.iter().enumerate() {
        lines.extend(render_row(
            row,
            &widths,
            &state.alignments,
            Style::default(),
        ));
        if index + 1 < state.rows.len() {
            lines.push(render_separator(&widths, BODY_SEPARATOR, separator_style));
        }
    }
    RenderedTable {
        lines,
        prewrapped: true,
    }
}

fn normalize_row(row: &mut Vec<TableCell>, column_count: usize) {
    row.truncate(column_count);
    row.resize(column_count, TableCell::default());
}

fn collect_metrics(
    header: &[TableCell],
    rows: &[Vec<TableCell>],
    column_count: usize,
) -> Vec<ColumnMetrics> {
    let mut result = Vec::with_capacity(column_count);
    for column in 0..column_count {
        let header_text = header[column].plain_text();
        let header_token_width = longest_token_width(&header_text);
        let mut max_width = header[column].width();
        let mut body_token_width = 0usize;
        let mut body_token_count = 0usize;
        let mut long_body_token_count = 0usize;
        let mut total_words = 0usize;
        let mut nonempty_cells = 0usize;
        let mut total_cell_width = 0usize;

        for row in rows {
            let cell = &row[column];
            max_width = max_width.max(cell.width());
            let text = cell.plain_text();
            let mut word_count = 0;
            for token in text.split_whitespace() {
                let width = UnicodeWidthStr::width(token);
                body_token_width = body_token_width.max(width);
                long_body_token_count += usize::from(width >= 20);
                word_count += 1;
            }
            if word_count > 0 {
                body_token_count += word_count;
                total_words += word_count;
                nonempty_cells += 1;
                total_cell_width += UnicodeWidthStr::width(text.as_str());
            }
        }

        let average_words = if nonempty_cells == 0 {
            header_text.split_whitespace().count() as f64
        } else {
            total_words as f64 / nonempty_cells as f64
        };
        let average_width = if nonempty_cells == 0 {
            UnicodeWidthStr::width(header_text.as_str()) as f64
        } else {
            total_cell_width as f64 / nonempty_cells as f64
        };
        let kind = if long_body_token_count > 0
            && long_body_token_count >= body_token_count.saturating_sub(long_body_token_count)
        {
            ColumnKind::TokenHeavy
        } else if average_words >= 4.0 || average_width >= 28.0 {
            ColumnKind::Narrative
        } else {
            ColumnKind::Compact
        };
        result.push(ColumnMetrics {
            max_width,
            header_token_width,
            body_token_width,
            kind,
        });
    }
    result
}

fn longest_token_width(text: &str) -> usize {
    text.split_whitespace()
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
}

fn compute_column_widths(
    metrics: &[ColumnMetrics],
    available_width: Option<usize>,
) -> Option<Vec<usize>> {
    let mut widths: Vec<usize> = metrics
        .iter()
        .map(|column| column.max_width.max(MIN_COLUMN_WIDTH))
        .collect();
    let Some(max_width) = available_width else {
        return Some(widths);
    };
    if max_width < metrics.len() * MIN_COLUMN_WIDTH {
        return None;
    }

    let mut floors: Vec<usize> = metrics.iter().map(preferred_floor).collect();
    let floor_total: usize = floors.iter().sum();
    if floor_total > max_width {
        let minimums = vec![MIN_COLUMN_WIDTH; floors.len()];
        shrink_columns(&mut floors, &minimums, metrics, floor_total - max_width);
    }
    let total: usize = widths.iter().sum();
    if total > max_width {
        let remaining = shrink_columns(&mut widths, &floors, metrics, total - max_width);
        if remaining > 0 {
            return None;
        }
    }
    Some(widths)
}

fn preferred_floor(metrics: &ColumnMetrics) -> usize {
    let target = match metrics.kind {
        ColumnKind::Narrative | ColumnKind::TokenHeavy => 16,
        ColumnKind::Compact => metrics
            .header_token_width
            .max(metrics.body_token_width.min(16)),
    };
    target.max(MIN_COLUMN_WIDTH).min(metrics.max_width)
}

fn shrink_columns(
    widths: &mut [usize],
    floors: &[usize],
    metrics: &[ColumnMetrics],
    mut amount: usize,
) -> usize {
    for kind in [
        ColumnKind::TokenHeavy,
        ColumnKind::Narrative,
        ColumnKind::Compact,
    ] {
        let slack: usize = widths
            .iter()
            .enumerate()
            .filter(|(index, _)| metrics[*index].kind == kind)
            .map(|(index, width)| width.saturating_sub(floors[index]))
            .sum();
        let to_remove = amount.min(slack);
        if to_remove == 0 {
            continue;
        }

        let mut low = 0;
        let mut high = widths
            .iter()
            .enumerate()
            .filter(|(index, _)| metrics[*index].kind == kind)
            .map(|(index, width)| width.saturating_sub(floors[index]))
            .max()
            .unwrap_or(0);
        while low < high {
            let cap = low + (high - low) / 2;
            let removed: usize = widths
                .iter()
                .enumerate()
                .filter(|(index, _)| metrics[*index].kind == kind)
                .map(|(index, width)| width.saturating_sub(floors[index]).saturating_sub(cap))
                .sum();
            if removed > to_remove {
                low = cap + 1;
            } else {
                high = cap;
            }
        }

        let cap = low;
        let mut removed = 0;
        for (index, width) in widths.iter_mut().enumerate() {
            if metrics[index].kind != kind {
                continue;
            }
            let reduction = width.saturating_sub(floors[index]).saturating_sub(cap);
            *width -= reduction;
            removed += reduction;
        }
        let mut remainder = to_remove - removed;
        for (index, width) in widths.iter_mut().enumerate() {
            if remainder == 0 {
                break;
            }
            if metrics[index].kind == kind && width.saturating_sub(floors[index]) == cap {
                *width -= 1;
                remainder -= 1;
            }
        }
        amount -= to_remove;
        if amount == 0 {
            break;
        }
    }
    amount
}

fn render_separator(widths: &[usize], marker: char, style: Style) -> Line<'static> {
    let gap = " ".repeat(COLUMN_GAP);
    let text = widths
        .iter()
        .map(|width| marker.to_string().repeat(*width + CELL_PADDING * 2))
        .collect::<Vec<_>>()
        .join(&gap);
    Line::from(Span::styled(text, style))
}

fn render_row(
    row: &[TableCell],
    widths: &[usize],
    alignments: &[Alignment],
    row_style: Style,
) -> Vec<Line<'static>> {
    let wrapped: Vec<Vec<Line<'static>>> = row
        .iter()
        .zip(widths)
        .map(|(cell, width)| wrap_cell(cell, *width))
        .collect();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
    let mut output = Vec::with_capacity(height);

    for line_index in 0..height {
        let Some(last_visible) = wrapped.iter().rposition(|cell| {
            cell.get(line_index)
                .is_some_and(|line| Line::width(line) > 0)
        }) else {
            output.push(Line::default().style(row_style));
            continue;
        };
        let mut spans = Vec::new();
        for column in 0..=last_visible {
            spans.push(Span::raw(" ".repeat(CELL_PADDING)));
            let content = wrapped[column].get(line_index).cloned().unwrap_or_default();
            let remaining = widths[column].saturating_sub(Line::width(&content));
            let (left, right) = match alignments[column] {
                Alignment::Left | Alignment::None => (0, remaining),
                Alignment::Center => (remaining / 2, remaining - remaining / 2),
                Alignment::Right => (remaining, 0),
            };
            if left > 0 {
                spans.push(Span::raw(" ".repeat(left)));
            }
            spans.extend(content.spans);
            if column != last_visible {
                if right > 0 {
                    spans.push(Span::raw(" ".repeat(right)));
                }
                spans.push(Span::raw(" ".repeat(CELL_PADDING + COLUMN_GAP)));
            }
        }
        output.push(Line::from(spans).style(row_style));
    }
    output
}

fn wrap_cell(cell: &TableCell, width: usize) -> Vec<Line<'static>> {
    if cell.lines.is_empty() {
        return vec![Line::default()];
    }
    let mut wrapped = Vec::new();
    for source in &cell.lines {
        let lines = word_wrap_line(source, RtOptions::new(width.max(1)));
        if lines.is_empty() {
            wrapped.push(source.clone());
        } else {
            wrapped.extend(lines.iter().map(line_to_static));
        }
    }
    if wrapped.is_empty() {
        wrapped.push(Line::default());
    }
    wrapped
}

fn should_render_records(
    rows: &[Vec<TableCell>],
    widths: &[usize],
    metrics: &[ColumnMetrics],
) -> bool {
    if rows.is_empty() {
        return false;
    }
    let affected = rows
        .iter()
        .filter(|row| {
            let fragmented = row
                .iter()
                .zip(widths)
                .zip(metrics)
                .any(|((cell, width), metrics)| {
                    let fragmented = cell
                        .plain_text()
                        .split_whitespace()
                        .any(|token| UnicodeWidthStr::width(token) > *width);
                    match metrics.kind {
                        ColumnKind::Compact => fragmented,
                        ColumnKind::TokenHeavy => *width < 12 && fragmented,
                        ColumnKind::Narrative => false,
                    }
                });
            fragmented || expansive_cells_are_starved(row, widths, metrics)
        })
        .count();
    let threshold = if rows.len() == 1 {
        1
    } else {
        2.max(rows.len().div_ceil(3))
    };
    affected >= threshold
}

fn expansive_cells_are_starved(
    row: &[TableCell],
    widths: &[usize],
    metrics: &[ColumnMetrics],
) -> bool {
    let expansive: Vec<(ColumnKind, usize, usize)> = row
        .iter()
        .zip(widths)
        .zip(metrics)
        .filter(|((_, _), metrics)| metrics.kind != ColumnKind::Compact)
        .map(|((cell, width), metrics)| (metrics.kind, *width, wrap_cell(cell, *width).len()))
        .collect();
    expansive
        .iter()
        .filter(|(_, _, height)| *height >= 4)
        .count()
        >= 2
        || expansive.iter().any(|(kind, width, height)| {
            *kind == ColumnKind::Narrative && *width < 12 && *height >= 7
        })
}

fn render_records(
    headers: &[TableCell],
    rows: &[Vec<TableCell>],
    metrics: &[ColumnMetrics],
    available_width: Option<usize>,
    label_style: Style,
    separator_style: Style,
) -> Vec<Line<'static>> {
    let label_width = headers
        .iter()
        .map(|header| UnicodeWidthStr::width(header.plain_text().as_str()))
        .max()
        .unwrap_or(0);
    let minimum_value_width = if metrics
        .iter()
        .any(|metrics| metrics.kind != ColumnKind::Compact)
    {
        24
    } else {
        12
    };
    let aligned =
        available_width.is_none_or(|width| 1 + label_width + 2 + minimum_value_width <= width);
    let mut output = Vec::new();

    for (row_index, row) in rows.iter().enumerate() {
        for (header, value) in headers.iter().zip(row) {
            if aligned {
                render_aligned_field(
                    &mut output,
                    header,
                    value,
                    label_width,
                    available_width,
                    label_style,
                );
            } else {
                render_stacked_field(&mut output, header, value, available_width, label_style);
            }
        }
        if row_index + 1 < rows.len() {
            let width = available_width
                .unwrap_or_else(|| output.iter().map(Line::width).max().unwrap_or(0));
            output.push(Line::from(Span::styled(
                BODY_SEPARATOR.to_string().repeat(width),
                separator_style,
            )));
        }
    }
    output
}

fn render_aligned_field(
    output: &mut Vec<Line<'static>>,
    header: &TableCell,
    value: &TableCell,
    label_width: usize,
    available_width: Option<usize>,
    label_style: Style,
) {
    let value_indent = 1 + label_width + 2;
    let value_width = available_width
        .map(|width| width.saturating_sub(value_indent).max(MIN_COLUMN_WIDTH))
        .unwrap_or_else(|| value.width().max(MIN_COLUMN_WIDTH));
    for (index, value_line) in wrap_cell(value, value_width).into_iter().enumerate() {
        let mut spans = Vec::new();
        if index == 0 {
            let label = header.plain_text();
            spans.push(Span::raw(" "));
            spans.push(Span::styled(label.clone(), label_style));
            spans.push(Span::raw(" ".repeat(
                label_width.saturating_sub(UnicodeWidthStr::width(label.as_str())) + 2,
            )));
        } else {
            spans.push(Span::raw(" ".repeat(value_indent)));
        }
        spans.extend(value_line.spans);
        output.push(Line::from(spans));
    }
}

fn render_stacked_field(
    output: &mut Vec<Line<'static>>,
    header: &TableCell,
    value: &TableCell,
    available_width: Option<usize>,
    label_style: Style,
) {
    let label_width = available_width
        .map(|width| width.saturating_sub(1).max(1))
        .unwrap_or_else(|| UnicodeWidthStr::width(header.plain_text().as_str()).max(1));
    let label = Line::from(Span::styled(header.plain_text(), label_style));
    for label_line in word_wrap_line(&label, RtOptions::new(label_width)) {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(line_to_static(&label_line).spans);
        output.push(Line::from_iter(spans));
    }
    let value_width = available_width
        .map(|width| width.saturating_sub(2).max(1))
        .unwrap_or_else(|| value.width().max(1));
    for value_line in wrap_cell(value, value_width) {
        let mut spans = vec![Span::raw("  ")];
        spans.extend(value_line.spans);
        output.push(Line::from_iter(spans));
    }
}

fn pipe_fallback(header: &[TableCell], alignments: &[Alignment]) -> Vec<Line<'static>> {
    let mut lines = vec![row_to_pipe_line(header)];
    let delimiter = alignments
        .iter()
        .map(|alignment| match alignment {
            Alignment::None => "---",
            Alignment::Left => ":---",
            Alignment::Center => ":---:",
            Alignment::Right => "---:",
        })
        .collect::<Vec<_>>()
        .join(" | ");
    lines.push(Line::from(format!("| {delimiter} |")));
    lines
}

fn row_to_pipe_line(row: &[TableCell]) -> Line<'static> {
    let text = row
        .iter()
        .map(|cell| cell.plain_text().replace('|', "\\|"))
        .collect::<Vec<_>>()
        .join(" | ");
    Line::from(format!("| {text} |"))
}
