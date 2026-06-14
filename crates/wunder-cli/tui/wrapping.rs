use ratatui::text::Line;
use ratatui::text::Span;
use std::ops::Range;
use textwrap::Options;

/// Like `wrap_ranges` but returns ranges without trailing whitespace and
/// without the sentinel extra byte. Suitable for general wrapping where
/// trailing spaces should not be preserved.
fn wrap_ranges_trim<'a, O>(text: &str, width_or_options: O) -> Vec<Range<usize>>
where
    O: Into<Options<'a>>,
{
    let opts = width_or_options.into();
    let mut lines: Vec<Range<usize>> = Vec::new();
    for line in textwrap::wrap(text, opts).iter() {
        match line {
            std::borrow::Cow::Borrowed(slice) => {
                let start = unsafe { slice.as_ptr().offset_from(text.as_ptr()) as usize };
                let end = start + slice.len();
                lines.push(start..end);
            }
            std::borrow::Cow::Owned(_) => panic!("wrap_ranges_trim: unexpected owned string"),
        }
    }
    lines
}

#[derive(Debug, Clone)]
pub struct RtOptions<'a> {
    /// The width in columns at which the text will be wrapped.
    pub width: usize,
    /// Line ending used for breaking lines.
    pub line_ending: textwrap::LineEnding,
    /// Indentation used for the first line of output.
    pub initial_indent: Line<'a>,
    /// Indentation used for subsequent lines of output.
    pub subsequent_indent: Line<'a>,
    /// Allow long words to be broken if they cannot fit on a line.
    pub break_words: bool,
    /// Wrapping algorithm to use.
    pub wrap_algorithm: textwrap::WrapAlgorithm,
    /// The line breaking algorithm to use.
    pub word_separator: textwrap::WordSeparator,
    /// The method for splitting words.
    pub word_splitter: textwrap::WordSplitter,
}

impl From<usize> for RtOptions<'_> {
    fn from(width: usize) -> Self {
        RtOptions::new(width)
    }
}

impl<'a> RtOptions<'a> {
    pub fn new(width: usize) -> Self {
        RtOptions {
            width,
            line_ending: textwrap::LineEnding::LF,
            initial_indent: Line::default(),
            subsequent_indent: Line::default(),
            break_words: true,
            word_separator: textwrap::WordSeparator::new(),
            wrap_algorithm: textwrap::WrapAlgorithm::FirstFit,
            word_splitter: textwrap::WordSplitter::HyphenSplitter,
        }
    }

    pub fn initial_indent(self, initial_indent: Line<'a>) -> Self {
        RtOptions {
            initial_indent,
            ..self
        }
    }

    pub fn subsequent_indent(self, subsequent_indent: Line<'a>) -> Self {
        RtOptions {
            subsequent_indent,
            ..self
        }
    }
}

#[must_use]
pub(crate) fn word_wrap_line<'a, O>(line: &'a Line<'a>, width_or_options: O) -> Vec<Line<'a>>
where
    O: Into<RtOptions<'a>>,
{
    // Flatten the line and record span byte ranges.
    let mut flat = String::new();
    let mut span_bounds = Vec::new();
    let mut acc = 0usize;
    for s in &line.spans {
        let text = s.content.as_ref();
        let start = acc;
        flat.push_str(text);
        acc += text.len();
        span_bounds.push((start..acc, s.style));
    }

    let rt_opts: RtOptions<'a> = width_or_options.into();
    let opts = Options::new(rt_opts.width)
        .line_ending(rt_opts.line_ending)
        .break_words(rt_opts.break_words)
        .wrap_algorithm(rt_opts.wrap_algorithm)
        .word_separator(rt_opts.word_separator)
        .word_splitter(rt_opts.word_splitter);

    let mut out: Vec<Line<'a>> = Vec::new();

    // Compute first line range with reduced width due to initial indent.
    let initial_width_available = opts
        .width
        .saturating_sub(rt_opts.initial_indent.width())
        .max(1);
    let initial_wrapped = wrap_ranges_trim(&flat, opts.clone().width(initial_width_available));
    let Some(first_line_range) = initial_wrapped.first() else {
        return vec![rt_opts.initial_indent.clone()];
    };

    // Build first wrapped line with initial indent.
    let mut first_line = rt_opts.initial_indent.clone().style(line.style);
    {
        let sliced = slice_line_spans(line, &span_bounds, first_line_range);
        let mut spans = first_line.spans;
        spans.append(
            &mut sliced
                .spans
                .into_iter()
                .map(|s| s.patch_style(line.style))
                .collect(),
        );
        first_line.spans = spans;
        out.push(first_line);
    }

    // Wrap the remainder using subsequent indent width and map back to original indices.
    let base = first_line_range.end;
    let skip_leading_spaces = flat[base..].chars().take_while(|c| *c == ' ').count();
    let base = base + skip_leading_spaces;
    let subsequent_width_available = opts
        .width
        .saturating_sub(rt_opts.subsequent_indent.width())
        .max(1);
    let remaining_wrapped = wrap_ranges_trim(&flat[base..], opts.width(subsequent_width_available));
    for r in &remaining_wrapped {
        if r.is_empty() {
            continue;
        }
        let mut subsequent_line = rt_opts.subsequent_indent.clone().style(line.style);
        let offset_range = (r.start + base)..(r.end + base);
        let sliced = slice_line_spans(line, &span_bounds, &offset_range);
        let mut spans = subsequent_line.spans;
        spans.append(
            &mut sliced
                .spans
                .into_iter()
                .map(|s| s.patch_style(line.style))
                .collect(),
        );
        subsequent_line.spans = spans;
        out.push(subsequent_line);
    }

    out
}

fn slice_line_spans<'a>(
    original: &'a Line<'a>,
    span_bounds: &[(Range<usize>, ratatui::style::Style)],
    range: &Range<usize>,
) -> Line<'a> {
    let start_byte = range.start;
    let end_byte = range.end;
    let mut acc: Vec<Span<'a>> = Vec::new();
    for (i, (range, style)) in span_bounds.iter().enumerate() {
        let s = range.start;
        let e = range.end;
        if e <= start_byte {
            continue;
        }
        if s >= end_byte {
            break;
        }
        let seg_start = start_byte.max(s);
        let seg_end = end_byte.min(e);
        if seg_end > seg_start {
            let local_start = seg_start - s;
            let local_end = seg_end - s;
            let content = original.spans[i].content.as_ref();
            let slice = &content[local_start..local_end];
            acc.push(Span {
                style: *style,
                content: std::borrow::Cow::Borrowed(slice),
            });
        }
        if e >= end_byte {
            break;
        }
    }
    Line {
        style: original.style,
        alignment: original.alignment,
        spans: acc,
    }
}
