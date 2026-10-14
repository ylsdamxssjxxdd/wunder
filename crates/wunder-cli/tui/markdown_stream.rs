//! Newline-gated incremental markdown renderer for streamed assistant text.
//!
//! ## Why the collector is incremental
//! A streamed answer arrives as many small deltas. The old collector cloned the
//! whole accumulated buffer on every newline and handed all of it back to
//! [`markdown::append_markdown`], then sliced off the lines it had already
//! returned. Rendering a long answer therefore cost `O(n^2)` parse + layout work
//! for content that had not changed, exactly the full recompute the repository
//! performance rules forbid.
//!
//! ## Design
//! The source buffer is split at `frozen_source_len`:
//!
//! * `buffer[..frozen_source_len]` is *frozen*. Its rendered lines are final and
//!   cached in `frozen_lines`; they are never rendered again.
//! * `buffer[frozen_source_len..]` is the *open region*: at most one block that
//!   can still grow. Only this region is handed to the renderer again, so a
//!   commit costs `O(newly frozen bytes) + O(open block bytes)`, never
//!   `O(whole answer)`.
//!
//! Freezing only ever moves the boundary forward to a *safe block boundary*,
//! computed by [`boundary::last_safe_boundary`]. A boundary is a line start that
//! provably begins a new top-level block, so rendering the chunk before it in
//! isolation yields the same lines as rendering the whole document, provided the
//! single blank line the renderer inserts between two top-level blocks is
//! re-attached (see [`MarkdownStreamCollector::drain_view`] and the lazy
//! separator rule in `freeze_up_to`).
//!
//! Lines from the open region are only handed out when re-rendering that region
//! can no longer change an already returned line ([`boundary::classify_open`]).
//! That gate exists because the renderer *does* restyle earlier lines from later
//! source: table columns widen when a long row arrives, an open ordered list
//! renumbers, a loose/tight list switch adds blank lines between items, a
//! ```` ```markdown ```` fence turns into a grid only once its closing fence
//! lands. Those regions commit as a whole when they close. Everything that
//! cannot be classified with confidence simply waits for the block to close —
//! correctness beats increment, and a commit that returns nothing is a normal
//! outcome, not an error.
//!
//! ## Deliberate deviations from the codex reference
//! * No port of `markdown_text_merge`: the source-offset machinery exists there
//!   for TeX math and inline directives. The CLI has neither, so offset
//!   bookkeeping would be dead weight.
//! * No `Range`-returning API: the caller appends lines to a cache and cannot
//!   revise a line it already received, so the collector may only hand out lines
//!   that are final (or provably stable), unlike codex, whose stream controller
//!   owns a replaceable pending cell.
//! * No renderer side channel: `markdown_render.rs` is owned by another
//!   pipeline. Incrementality is achieved by slicing source at a safe boundary
//!   and feeding the existing `append_markdown`. The gap this leaves versus a
//!   true renderer-level incremental API is one extra render pass per commit
//!   over the *open region only* (its provisional render, then its final frozen
//!   render), plus the per-chunk pre-passes of the renderer, which cannot see
//!   cross-chunk context.
//! * Width changes invalidate everything, which the caller already implements
//!   by rebuilding the collector (see `ensure_markdown_stream_state`), so
//!   `new(width)` semantics stay as they were.
//!
//! ## Known residual limits (accepted, streaming continuity comes first)
//! * A setext (`===` / `---` underline) heading that arrives after its paragraph
//!   line was already streamed restyles that line; the finished message is
//!   re-rendered when the entry text is replaced, which corrects it.
//! * A fenced block whose language highlighter is dropped by the size caps in
//!   `highlight.rs` can switch from styled to plain lines mid-stream.
//! * A link reference *used* before its definition block is frozen cannot see
//!   that definition; freezing is withheld for chunks that reference `][`.

use ratatui::text::Line;

use super::markdown;

/// Newline-gated accumulator that renders markdown and commits only fully
/// completed logical lines.
#[derive(Debug)]
pub(crate) struct MarkdownStreamCollector {
    buffer: String,
    /// Byte length of `buffer` whose rendered lines are final.
    frozen_source_len: usize,
    /// Final rendered lines for `buffer[..frozen_source_len]`.
    frozen_lines: Vec<Line<'static>>,
    /// Lines already handed out, counting frozen lines, block separators and
    /// provisional lines from the open region.
    committed_line_count: usize,
    width: Option<usize>,
    #[cfg(test)]
    rerender_calls: usize,
    #[cfg(test)]
    rerender_chars: usize,
}

impl MarkdownStreamCollector {
    pub fn new(width: Option<usize>) -> Self {
        Self {
            buffer: String::new(),
            frozen_source_len: 0,
            frozen_lines: Vec::new(),
            committed_line_count: 0,
            width,
            #[cfg(test)]
            rerender_calls: 0,
            #[cfg(test)]
            rerender_chars: 0,
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.frozen_source_len = 0;
        self.frozen_lines.clear();
        self.committed_line_count = 0;
    }

    pub fn push_delta(&mut self, delta: &str) {
        self.buffer.push_str(delta);
    }

    /// Freeze every block that is provably closed inside the newline-terminated
    /// part of the buffer and return the lines that became final.
    ///
    /// Lines of the still-open block come back only while re-rendering that
    /// block cannot change them any more; otherwise they arrive with the commit
    /// that closes the block, or from [`Self::finalize_and_drain`].
    pub fn commit_complete_lines(&mut self) -> Vec<Line<'static>> {
        let Some(complete_end) = self.buffer.rfind('\n').map(|index| index + 1) else {
            return Vec::new();
        };
        self.freeze_up_to(complete_end);
        let mut out = self.emit_frozen();
        let region_start = self.frozen_source_len;
        if region_start < complete_end {
            let region = boundary::classify_open(&self.buffer, region_start, complete_end);
            let keep_all = region == boundary::OpenRegion::CodeFence;
            if keep_all || region == boundary::OpenRegion::Paragraph {
                let source = self.buffer[region_start..complete_end].to_string();
                let mut rendered = Vec::new();
                self.render(source.as_str(), &mut rendered);
                out.extend(self.drain_view(rendered, !keep_all));
            }
        }
        out
    }

    /// Finalize the stream: emit all remaining lines beyond the last commit.
    /// If the buffer does not end with a newline, a temporary one is appended
    /// for rendering.
    pub fn finalize_and_drain(&mut self) -> Vec<Line<'static>> {
        let end = self.buffer.len();
        self.freeze_up_to(end);
        let region_start = self.frozen_source_len;
        let mut rendered = Vec::new();
        if region_start < end {
            let mut source = self.buffer[region_start..end].to_string();
            if !source.ends_with('\n') {
                source.push('\n');
            }
            self.render(source.as_str(), &mut rendered);
        }
        let mut out = self.emit_frozen();
        out.extend(self.drain_view(rendered, false));
        self.clear();
        out
    }

    /// Move the frozen boundary to the last safe block boundary at or before
    /// `to`, rendering only the newly closed chunk.
    fn freeze_up_to(&mut self, to: usize) {
        let boundary = boundary::last_safe_boundary(&self.buffer, self.frozen_source_len, to);
        if boundary <= self.frozen_source_len {
            return;
        }
        let source = self.buffer[self.frozen_source_len..boundary].to_string();
        let mut rendered = Vec::new();
        self.render(source.as_str(), &mut rendered);
        self.frozen_source_len = boundary;
        if rendered.is_empty() {
            // A chunk that yields no line (blank padding, a bare link
            // definition) must not consume the separator the next real block
            // needs.
            return;
        }
        if !self.frozen_lines.is_empty() {
            self.frozen_lines.push(Line::default());
        }
        self.frozen_lines.extend(rendered);
    }

    fn emit_frozen(&mut self) -> Vec<Line<'static>> {
        if self.committed_line_count >= self.frozen_lines.len() {
            return Vec::new();
        }
        let out = self.frozen_lines[self.committed_line_count..].to_vec();
        self.committed_line_count = self.frozen_lines.len();
        out
    }

    /// Hand out the tail view, that is `[separator]? ++ rendered`, against the
    /// lines already returned. `hold_last` keeps the final rendered line back:
    /// it is the one the next delta can still extend or restyle.
    fn drain_view(&mut self, rendered: Vec<Line<'static>>, hold_last: bool) -> Vec<Line<'static>> {
        if rendered.is_empty() {
            return Vec::new();
        }
        let keep = if hold_last {
            rendered.len() - 1
        } else {
            rendered.len()
        };
        if keep == 0 {
            return Vec::new();
        }
        let base = self.frozen_lines.len();
        let mut view: Vec<Line<'static>> = Vec::with_capacity(keep + 1);
        if base > 0 {
            view.push(Line::default());
        }
        view.extend(rendered.into_iter().take(keep));
        let start = self.committed_line_count.saturating_sub(base);
        let out = if start < view.len() {
            view[start..].to_vec()
        } else {
            Vec::new()
        };
        self.committed_line_count = self.committed_line_count.max(base + view.len());
        out
    }

    /// Single entry point into the renderer so the whole buffer is never
    /// rendered twice.
    fn render(&mut self, source: &str, out: &mut Vec<Line<'static>>) {
        #[cfg(test)]
        {
            self.rerender_calls += 1;
            self.rerender_chars += source.len();
        }
        markdown::append_markdown(source, self.width, out);
    }

    #[cfg(test)]
    fn rerender_stats(&self) -> (usize, usize) {
        (self.rerender_calls, self.rerender_chars)
    }
}

/// Block boundary analysis over raw markdown source.
///
/// Only line-level CommonMark structure is tracked here: fenced code, tables,
/// list and blockquote regions and indented code runs. Anything the scanner
/// cannot attribute to a closed top-level block keeps the region unfrozen.
mod boundary {
    /// How the still-open region at the tail behaves under re-render.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum OpenRegion {
        /// A fenced block: every completed source line is final, and the
        /// highlighter only ever moves forward, so earlier lines keep their
        /// styling as later lines arrive.
        CodeFence,
        /// A plain prose block whose lines cannot turn into another block and
        /// holds no inline markup a later line could close.
        Paragraph,
        /// Nothing safe to hand out early; the region commits when it closes.
        Hold,
    }

    struct Fence {
        marker: char,
        run: usize,
        info: String,
    }

    fn indent_of(line: &str) -> usize {
        let mut width = 0;
        for ch in line.chars() {
            match ch {
                ' ' => width += 1,
                '\t' => width += 4,
                _ => break,
            }
        }
        width
    }

    /// `(start, end, text without the line terminator)` for every line of
    /// `region`, with `start` counted from `base`.
    pub(crate) fn line_spans(region: &str, base: usize) -> Vec<(usize, usize, &str)> {
        let mut out = Vec::new();
        let mut cursor = base;
        for segment in region.split_inclusive('\n') {
            let end = cursor + segment.len();
            out.push((cursor, end, segment.trim_end_matches(['\r', '\n'])));
            cursor = end;
        }
        out
    }

    fn fence_of(line: &str, indent: usize) -> Option<Fence> {
        if indent > 3 {
            return None;
        }
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|ch| *ch == '`' || *ch == '~')?;
        let run = trimmed.chars().take_while(|ch| *ch == marker).count();
        if run < 3 {
            return None;
        }
        Some(Fence {
            marker,
            run,
            info: trimmed[run..].trim().to_string(),
        })
    }

    fn closes_fence(line: &str, indent: usize, open: &Fence) -> bool {
        fence_of(line, indent).is_some_and(|close| {
            close.marker == open.marker && close.run >= open.run && close.info.is_empty()
        })
    }

    fn is_markdown_fence(fence: &Fence) -> bool {
        let token = fence.info.split_whitespace().next().unwrap_or_default();
        matches!(token.to_ascii_lowercase().as_str(), "md" | "markdown")
    }

    /// `-`/`*`/`+` bullets and `1.`/`1)` ordered markers that pulldown reads as a
    /// list item opener for this line.
    fn is_list_marker(line: &str) -> bool {
        let trimmed = line.trim_start();
        let bytes = trimmed.as_bytes();
        if bytes.is_empty() {
            return false;
        }
        if matches!(bytes[0], b'-' | b'*' | b'+') {
            return bytes.len() == 1 || matches!(bytes[1], b' ' | b'\t');
        }
        let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
        if digits == 0 || digits > 9 {
            return false;
        }
        let rest = &trimmed[digits..];
        let Some(delimiter) = rest.chars().next().filter(|ch| *ch == '.' || *ch == ')') else {
            return false;
        };
        let after = &rest[delimiter.len_utf8()..];
        after.is_empty() || after.starts_with(' ') || after.starts_with('\t')
    }

    fn is_container_line(line: &str, indent: usize) -> bool {
        if indent >= 4 {
            return false;
        }
        let trimmed = line.trim_start();
        trimmed.starts_with('>') || is_list_marker(trimmed)
    }

    /// A line made only of `=` or `-`: a setext underline that would promote the
    /// paragraph above it to a heading, or a thematic break. Either one restyles
    /// or re-splits source that may already have been handed out, so a region
    /// holding one is never streamed early.
    fn is_thematic_or_setext(line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return false;
        }
        trimmed.chars().all(|ch| ch == '-') || trimmed.chars().all(|ch| ch == '=')
    }

    fn is_table_delimiter(line: &str) -> bool {
        let trimmed = line.trim();
        if !trimmed.contains('|') || !trimmed.contains('-') {
            return false;
        }
        trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .all(|cell| {
                let value = cell.trim();
                !value.is_empty() && value.chars().all(|ch| ch == '-' || ch == ':')
            })
    }

    /// Inline markup that a later line can still pair up with, restyling lines
    /// already rendered from this region.
    const ACTIVE_INLINE_CHARS: [char; 12] =
        ['`', '*', '_', '[', ']', '!', '<', '>', '&', '~', '#', '|'];

    fn has_active_inline(line: &str) -> bool {
        line.chars().any(|ch| ACTIVE_INLINE_CHARS.contains(&ch))
    }

    /// Every line of a paragraph region must stay prose: no container opener, no
    /// fence, no indented code, no blank line, no table row, no markup that a
    /// later line could close.
    pub(crate) fn paragraph_lines_are_stable(lines: &[(usize, usize, &str)]) -> bool {
        for (_, _, text) in lines {
            let indent = indent_of(text);
            if text.trim().is_empty() {
                return false;
            }
            if indent >= 4 || is_container_line(text, indent) {
                return false;
            }
            if fence_of(text, indent).is_some() || is_thematic_or_setext(text) {
                return false;
            }
            if has_active_inline(text) {
                return false;
            }
        }
        !lines.is_empty()
    }

    /// Classify the open region `src[from..to]`, whose start is a safe boundary.
    pub(crate) fn classify_open(src: &str, from: usize, to: usize) -> OpenRegion {
        let lines = line_spans(&src[from..to], from);
        let Some((_, _, first)) = lines.first().copied() else {
            return OpenRegion::Hold;
        };
        let indent = indent_of(first);
        if let Some(fence) = fence_of(first, indent) {
            // A ```markdown fence is unwrapped into a grid by the renderer as
            // soon as its closing fence arrives, so its early lines are a lie.
            return if is_markdown_fence(&fence) {
                OpenRegion::Hold
            } else {
                OpenRegion::CodeFence
            };
        }
        if paragraph_lines_are_stable(&lines) {
            OpenRegion::Paragraph
        } else {
            OpenRegion::Hold
        }
    }

    /// Offset of the last line start at or before `to` that provably begins a new
    /// top-level block, given that `from` is one. Returns `from` when nothing can
    /// be closed with certainty.
    pub(crate) fn last_safe_boundary(src: &str, from: usize, to: usize) -> usize {
        let lines = line_spans(&src[from..to], from);
        let mut best = from;
        let mut open_fence: Option<Fence> = None;
        let mut in_table = false;
        let mut in_container = false;
        let mut pending_blank = false;
        let mut prev_indent = 0usize;
        let mut prev_seen = false;
        let mut prev_table_cell_line = false;

        for (start, end, text) in lines {
            let indent = indent_of(text);
            if let Some(open) = open_fence.as_ref() {
                if closes_fence(text, indent, open) {
                    open_fence = None;
                    // The fenced block is closed by this very line, so the next
                    // line starts a new block even without a blank line.
                    best = best.max(end);
                    prev_seen = false;
                    prev_table_cell_line = false;
                }
                continue;
            }
            if text.trim().is_empty() {
                // A blank line ends a table region, but never a fenced block
                // (handled above) nor a list/blockquote region, which needs the
                // next non-blank line to prove it is over.
                in_table = false;
                pending_blank = true;
                continue;
            }
            let fence = fence_of(text, indent);
            let container_line = is_container_line(text, indent);
            if pending_blank {
                let inside_indented_code =
                    indent >= 4 && prev_seen && prev_indent >= 4 && !in_container;
                let closes_container =
                    in_container && indent == 0 && !container_line && fence.is_none();
                if (!in_container && !in_table && !inside_indented_code) || closes_container {
                    best = best.max(start);
                }
            }
            match fence {
                Some(open) => {
                    open_fence = Some(open);
                    in_container = false;
                    in_table = false;
                }
                None if container_line => {
                    in_container = true;
                    in_table = false;
                }
                None => {
                    if in_container && pending_blank {
                        in_container = false;
                    }
                }
            }
            let table_line = indent < 4 && text.contains('|');
            if !in_table
                && table_line
                && prev_seen
                && !pending_blank
                && prev_table_cell_line
                && is_table_delimiter(text)
            {
                in_table = true;
            }
            prev_table_cell_line = table_line;
            pending_blank = false;
            prev_indent = indent;
            prev_seen = true;
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;

    const WIDTH: usize = 44;

    fn one_shot(source: &str) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        markdown::append_markdown(source, Some(WIDTH), &mut out);
        out
    }

    /// Feed `source` as deltas cut at `splits`, commit on every delta holding a
    /// newline (the caller in `app.rs` does the same) and finalize at the end.
    fn stream_with_splits(source: &str, splits: &[usize]) -> Vec<Line<'static>> {
        let mut collector = MarkdownStreamCollector::new(Some(WIDTH));
        let mut out: Vec<Line<'static>> = Vec::new();
        let mut previous = 0;
        for &split in splits.iter().chain([source.len()].iter()) {
            let delta = &source[previous..split];
            previous = split;
            if delta.is_empty() {
                continue;
            }
            collector.push_delta(delta);
            if delta.contains('\n') {
                out.extend(collector.commit_complete_lines());
            }
        }
        out.extend(collector.finalize_and_drain());
        out
    }

    fn char_splits(source: &str, size: usize) -> Vec<usize> {
        let boundaries: Vec<usize> = source.char_indices().map(|(index, _)| index).collect();
        boundaries
            .iter()
            .skip(size.max(1))
            .step_by(size.max(1))
            .copied()
            .collect()
    }

    fn line_splits(source: &str) -> Vec<usize> {
        source
            .split_inclusive('\n')
            .scan(0usize, |offset, line| {
                *offset += line.len();
                Some(*offset)
            })
            .take_while(|offset| *offset < source.len())
            .collect()
    }

    fn word_splits(source: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let mut offset = 0;
        while offset < source.len() {
            let step = 7usize.min(source.len() - offset);
            let mut next = offset + step;
            while next > offset && !source.is_char_boundary(next) {
                next -= 1;
            }
            out.push(next);
            offset = next;
        }
        out
    }

    /// Deterministic pseudo-random byte splits, always on char boundaries.
    fn pseudo_random_splits(source: &str) -> Vec<usize> {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut out = Vec::new();
        let mut offset = 0;
        while offset < source.len() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let step = 1 + ((state >> 33) as usize % 11);
            let mut next = (offset + step).min(source.len());
            while next > offset && !source.is_char_boundary(next) {
                next -= 1;
            }
            out.push(next);
            offset = next;
        }
        out
    }

    fn fixture_sources() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "fence-with-blank-line",
                "intro gate\n\n```\nvalue one\n\nvalue two\n```\n\noutro line\n",
            ),
            (
                "table-growing-columns",
                "before\n\n| gate | value |\n| --- | --- |\n| item-1 | short |\n\
                 | item-2 | a much longer value cell here |\n\nafter\n",
            ),
            (
                "nested-list",
                "- top one\n  - nested a\n    - deep\n- top two\n\nafter\n",
            ),
            (
                "loose-ordered-list",
                "1. first\n1. second\n\n1. third\n\nafter\n",
            ),
            (
                "blockquote-with-list",
                "> quoted head\n>\n> - inner item\n> - other item\n\nafter gate\n",
            ),
            (
                "markdown-table-fence",
                "```markdown\n| gate | value |\n| --- | --- |\n| flaky | false |\n```\n",
            ),
            (
                "setext-heading",
                "Title line\n===\n\nbody line\n",
            ),
            (
                "indented-code-with-blank-line",
                "para line\n\n    value one\n\n    value two\n\nafter\n",
            ),
            (
                "blank-line-runs-and-rule",
                "para a\n\n\n\npara b\n\n---\n\npara c\n",
            ),
            (
                "inline-code-across-lines",
                "lead `alpha\nbeta` tail\n\nnext para\n",
            ),
            (
                "emphasis-across-lines",
                "start *open phrase\nstill open* end\n\nlast para\n",
            ),
            (
                "unterminated-last-line",
                "para one\n\npara two ends without a newline",
            ),
            (
                "unclosed-fence-at-end",
                "before gate\n\n```\ncode line one\ncode line two\n",
            ),
            (
                "mixed-document",
                "# Heading\n\nprose line one\nprose line two\n\n- item-1\n- item-2\n\
                 \n> quoted\n\nterm\n\n| gate | value |\n| :-- | --: |\n| a | b |\n\n\
                 ```sh\nvalue = 1\n```\n\nfin\n",
            ),
            (
                "list-lazy-continuation",
                "- item one\nlazy tail text\n- item two\n\nafter\n",
            ),
        ]
    }

    #[test]
    fn property_streamed_lines_equal_one_shot_render() {
        let patterns: Vec<(&str, fn(&str) -> Vec<usize>)> = vec![
            ("per-line", line_splits),
            ("every-two-chars", |source| char_splits(source, 2)),
            ("fixed-7-bytes", word_splits),
            ("pseudo-random", pseudo_random_splits),
            ("single-delta", |_| Vec::new()),
        ];
        for (name, source) in fixture_sources() {
            let expected = one_shot(source);
            for (pattern, splits) in &patterns {
                let streamed = stream_with_splits(source, &splits(source));
                assert_eq!(
                    streamed, expected,
                    "fixture {name} split by {pattern} drifted from the one-shot render"
                );
            }
        }
    }

    #[test]
    fn streaming_a_long_answer_does_not_rerender_the_frozen_prefix() {
        let mut source = String::new();
        for index in 0..40 {
            source.push_str(format!("item-{index} carries a value and a note\n\n").as_str());
        }
        source.push_str("gate ends the answer\n");
        let splits = line_splits(source.as_str());

        let mut collector = MarkdownStreamCollector::new(Some(WIDTH));
        let mut previous = 0;
        let mut naive_chars = 0usize;
        for &split in splits.iter().chain([source.len()].iter()) {
            let delta = &source[previous..split];
            previous = split;
            collector.push_delta(delta);
            if !delta.contains('\n') {
                continue;
            }
            // What the old whole-buffer render would have paid for this commit.
            naive_chars += collector.buffer[..collector.buffer.rfind('\n').unwrap() + 1].len();
            collector.commit_complete_lines();
        }
        let frozen_before_finalize = collector.frozen_source_len;
        collector.finalize_and_drain();

        let (_calls, rendered_chars) = collector.rerender_stats();
        assert!(
            frozen_before_finalize * 10 >= source.len() * 9,
            "the stream froze {frozen_before_finalize} of {} source bytes, so this fixture \
             never exercised the frozen prefix",
            source.len()
        );
        assert!(
            rendered_chars * 4 <= naive_chars,
            "incremental renders covered {rendered_chars} bytes, a full recompute \
             would have covered {naive_chars}"
        );
    }

    #[test]
    fn blank_line_inside_a_fence_is_not_a_block_boundary() {
        let source = "```\nvalue one\n\nvalue two\n```\n";
        let streamed = stream_with_splits(source, &line_splits(source));
        let expected = one_shot(source);
        assert_eq!(streamed, expected);
        assert_eq!(
            expected.iter().filter(|line| line.spans.is_empty()).count(),
            streamed.iter().filter(|line| line.spans.is_empty()).count(),
            "the blank line inside the fenced block moved"
        );
    }

    #[test]
    fn a_table_waits_for_its_rows_before_it_hands_out_grid_lines() {
        let source = "| gate | value |\n| --- | --- |\n| item-1 | a long value cell |\n";
        let mut collector = MarkdownStreamCollector::new(Some(WIDTH));
        let mut emitted: Vec<Line<'static>> = Vec::new();
        for delta in source.split_inclusive('\n') {
            collector.push_delta(delta);
            emitted.extend(collector.commit_complete_lines());
        }
        assert!(
            emitted.is_empty(),
            "an open table must not leak grid lines: {emitted:?}"
        );
        emitted.extend(collector.finalize_and_drain());
        assert_eq!(emitted, one_shot(source));
    }

    #[test]
    fn ordered_list_numbering_survives_item_by_item_commits() {
        let source = "1. first\n1. second\n1. third\n\nafter gate\n";
        let mut collector = MarkdownStreamCollector::new(Some(WIDTH));
        let mut emitted: Vec<Line<'static>> = Vec::new();
        for delta in source.split_inclusive('\n') {
            collector.push_delta(delta);
            emitted.extend(collector.commit_complete_lines());
        }
        emitted.extend(collector.finalize_and_drain());
        assert_eq!(emitted, one_shot(source));
        let joined: Vec<String> = emitted
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        assert!(joined.iter().any(|line| line.starts_with("2. second")), "{joined:?}");
        assert!(joined.iter().any(|line| line.starts_with("3. third")), "{joined:?}");
    }

    #[test]
    fn plain_prose_streams_line_by_line_before_the_paragraph_closes() {
        let source = "alpha line arrives\nbeta line arrives\ngamma line arrives\n";
        let mut collector = MarkdownStreamCollector::new(Some(WIDTH));
        let mut emitted: Vec<Line<'static>> = Vec::new();
        for delta in source.split_inclusive('\n') {
            collector.push_delta(delta);
            emitted.extend(collector.commit_complete_lines());
        }
        assert_eq!(emitted.len(), 2, "only the last line is held back: {emitted:?}");
        emitted.extend(collector.finalize_and_drain());
        assert_eq!(emitted, one_shot(source));
    }

    #[test]
    fn finalize_drains_an_unterminated_last_line_and_resets() {
        let source = "para one\n\npara two ends";
        let mut collector = MarkdownStreamCollector::new(Some(WIDTH));
        collector.push_delta(source);
        let mut emitted = collector.commit_complete_lines();
        emitted.extend(collector.finalize_and_drain());
        assert_eq!(emitted, one_shot("para one\n\npara two ends\n"));
        assert!(collector.buffer.is_empty());
        assert_eq!(collector.frozen_source_len, 0);
        assert_eq!(collector.committed_line_count, 0);
    }

    #[test]
    fn clear_drops_frozen_lines_with_the_buffer() {
        let mut collector = MarkdownStreamCollector::new(Some(WIDTH));
        collector.push_delta("para one\n\npara two\n\n");
        assert!(!collector.commit_complete_lines().is_empty());
        collector.clear();
        assert!(collector.finalize_and_drain().is_empty());
    }

    #[test]
    fn separators_between_blocks_match_the_one_shot_render() {
        let source = "para a\n\npara b\n\npara c\n";
        let expected = one_shot(source);
        assert_eq!(
            expected
                .iter()
                .filter(|line| line.spans.is_empty() || line.spans.iter().all(|span| span
                    .content
                    .chars()
                    .all(|ch| ch == ' ')))
                .count(),
            2
        );
        assert_eq!(stream_with_splits(source, &line_splits(source)), expected);
    }

    #[test]
    fn commit_never_re_renders_bytes_already_frozen() {
        let source = "para a\n\npara b\n\npara c\n\npara d\n";
        let mut collector = MarkdownStreamCollector::new(Some(WIDTH));
        let mut last_frozen = 0usize;
        for delta in source.split_inclusive('\n') {
            collector.push_delta(delta);
            if !delta.contains('\n') {
                continue;
            }
            collector.commit_complete_lines();
            assert!(collector.frozen_source_len >= last_frozen);
            last_frozen = collector.frozen_source_len;
        }
        assert!(
            last_frozen > 0,
            "the frozen boundary should advance as blocks close"
        );
    }
}
