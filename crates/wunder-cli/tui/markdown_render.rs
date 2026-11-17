use super::highlight::highlight_code_to_lines;
use super::line_utils::line_to_static;
use super::wrapping::word_wrap_line;
use super::wrapping::RtOptions;
use pulldown_cmark::Alignment;
use pulldown_cmark::CodeBlockKind;
use pulldown_cmark::CowStr;
use pulldown_cmark::Event;
use pulldown_cmark::HeadingLevel;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use std::borrow::Cow;
use unicode_width::UnicodeWidthStr;

struct FenceLine {
    marker: char,
    run: usize,
    info: String,
    source: String,
}

fn fence_parts(line: &str) -> Option<FenceLine> {
    let without_indent = line.trim_start_matches(' ');
    let indent = line.len() - without_indent.len();
    if indent > 3 {
        return None;
    }
    let marker = without_indent
        .chars()
        .next()
        .filter(|ch| *ch == '`' || *ch == '~')?;
    let run = without_indent
        .chars()
        .take_while(|ch| *ch == marker)
        .count();
    if run < 3 {
        return None;
    }
    Some(FenceLine {
        marker,
        run,
        info: without_indent[run..].trim().to_string(),
        source: line.to_string(),
    })
}

fn is_markdown_fence(fence: &FenceLine) -> bool {
    let token = fence.info.split_whitespace().next().unwrap_or_default();
    matches!(token.to_ascii_lowercase().as_str(), "md" | "markdown")
}

fn is_table_delimiter_line(line: &str) -> bool {
    table_segments(line).is_some_and(|segments| {
        segments.len() >= 2
            && segments.iter().all(|cell| {
                let cell = cell.trim();
                let cell = cell.strip_prefix(':').unwrap_or(cell);
                let cell = cell.strip_suffix(':').unwrap_or(cell);
                !cell.is_empty() && cell.chars().all(|ch| ch == '-')
            })
    })
}

fn is_table_header_line(line: &str) -> bool {
    table_segments(line).is_some_and(|segments| {
        segments.len() >= 2 && segments.iter().any(|cell| !cell.trim().is_empty())
    }) && !is_table_delimiter_line(line)
}

fn table_segments(line: &str) -> Option<Vec<&str>> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let outer = trimmed.starts_with('|') || trimmed.ends_with('|');
    let body = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let body = body.strip_suffix('|').unwrap_or(body);
    let mut segments = Vec::new();
    let mut start = 0;
    let bytes = body.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = index.saturating_add(2);
        } else if bytes[index] == b'|' {
            segments.push(body[start..index].trim());
            start = index + 1;
            index += 1;
        } else {
            index += 1;
        }
    }
    segments.push(body[start..].trim());
    if !outer && segments.len() < 2 {
        return None;
    }
    Some(segments)
}

/// Accept the pipe-table dialects commonly emitted by models (`|-|-|`, missing outer
/// pipes, and escaped cell pipes) while keeping fenced source code untouched. The parser
/// still renders a normal GFM table after this small source normalization.
fn normalize_pipe_tables(input: &str) -> Cow<'_, str> {
    let mut out = String::with_capacity(input.len());
    let mut lines = input.split_inclusive('\n').peekable();
    let mut in_fence: Option<FenceLine> = None;
    let mut changed = false;
    while let Some(line) = lines.next() {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(fence) = fence_parts(trimmed) {
            if let Some(open) = in_fence.take() {
                if fence.marker != open.marker || fence.run < open.run || !fence.info.is_empty() {
                    in_fence = Some(open);
                }
            } else {
                in_fence = Some(fence);
            }
            out.push_str(line);
            continue;
        }
        if in_fence.is_some() {
            out.push_str(line);
            continue;
        }
        let Some(next) = lines.peek().copied() else {
            out.push_str(line);
            continue;
        };
        if is_table_header_line(trimmed)
            && is_table_delimiter_line(next.trim_end_matches(['\r', '\n']))
        {
            let delimiter = next.trim_end_matches(['\r', '\n']);
            let normalized = table_segments(delimiter).map(|cells| {
                let cells = cells
                    .iter()
                    .map(|cell| {
                        let cell = cell.trim();
                        match (cell.starts_with(':'), cell.ends_with(':')) {
                            (true, true) => ":---:",
                            (true, false) => ":---",
                            (false, true) => "---:",
                            (false, false) => "---",
                        }
                    })
                    .collect::<Vec<_>>();
                format!("| {} |\n", cells.join(" | "))
            });
            out.push_str(line);
            if let Some(normalized) = normalized {
                let _ = lines.next();
                out.push_str(normalized.as_str());
                changed = true;
            }
            continue;
        }
        out.push_str(line);
    }
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(input)
    }
}

fn body_contains_table(body: &str) -> bool {
    let mut previous: Option<&str> = None;
    for line in body.lines() {
        if line.trim().is_empty() {
            previous = None;
            continue;
        }
        if let Some(candidate) = previous {
            if is_table_header_line(candidate) && is_table_delimiter_line(line) {
                return true;
            }
        }
        previous = Some(line);
    }
    false
}

/// Re-emits a ` ```markdown `/`~~~markdown` fence as plain markdown when its body really
/// is a table, and verbatim otherwise, so genuine code samples stay code.
fn unwrap_markdown_table_fences(input: &str) -> Cow<'_, str> {
    if !input.contains("```") && !input.contains("~~~") {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut open: Option<FenceLine> = None;
    let mut body = String::new();
    for line in input.split_inclusive('\n') {
        match open.take() {
            None => match fence_parts(line) {
                Some(fence) if is_markdown_fence(&fence) => {
                    open = Some(fence);
                }
                _ => out.push_str(line),
            },
            Some(fence) => {
                let closes = fence_parts(line).is_some_and(|close| {
                    close.marker == fence.marker && close.run >= fence.run && close.info.is_empty()
                });
                if !closes {
                    open = Some(fence);
                    body.push_str(line);
                    continue;
                }
                if body_contains_table(body.as_str()) {
                    out.push_str(body.as_str());
                } else {
                    out.push_str(fence.source.as_str());
                    out.push_str(body.as_str());
                    out.push_str(line);
                }
                body.clear();
            }
        }
    }
    if let Some(fence) = open {
        // Never closed: leave the source as written so it degrades to a code block.
        out.push_str(fence.source.as_str());
        out.push_str(body.as_str());
    }
    Cow::Owned(out)
}

struct MarkdownStyles {
    h1: Style,
    h2: Style,
    h3: Style,
    h4: Style,
    h5: Style,
    h6: Style,
    code: Style,
    emphasis: Style,
    strong: Style,
    strikethrough: Style,
    ordered_list_marker: Style,
    unordered_list_marker: Style,
    link: Style,
    blockquote: Style,
}

impl Default for MarkdownStyles {
    fn default() -> Self {
        use ratatui::style::Stylize;

        Self {
            h1: Style::new().bold().underlined(),
            h2: Style::new().bold(),
            h3: Style::new().bold().italic(),
            h4: Style::new().italic(),
            h5: Style::new().italic(),
            h6: Style::new().italic(),
            // Reading surfaces stay quiet: code, list numbers and quotes carry
            // structure, not emphasis, so they step back from the answer text.
            code: super::theme::code_text(),
            emphasis: Style::new().italic(),
            strong: Style::new().bold(),
            strikethrough: Style::new().crossed_out(),
            ordered_list_marker: super::theme::secondary_text(),
            unordered_list_marker: Style::new(),
            link: super::theme::link_text(),
            blockquote: super::theme::secondary_text().add_modifier(Modifier::ITALIC),
        }
    }
}

#[derive(Clone, Debug)]
struct IndentContext {
    prefix: Vec<Span<'static>>,
    marker: Option<Vec<Span<'static>>>,
    is_list: bool,
}

impl IndentContext {
    fn new(prefix: Vec<Span<'static>>, marker: Option<Vec<Span<'static>>>, is_list: bool) -> Self {
        Self {
            prefix,
            marker,
            is_list,
        }
    }
}

mod table;

use table::{RenderedTable, TableState};

pub(crate) fn render_markdown_text_with_width(input: &str, width: Option<usize>) -> Text<'static> {
    // Models wrap tables in ```markdown fences; parsed as code they lose their shape,
    // so a fence whose body is a table is handed to the parser as bare markdown.
    let source = unwrap_markdown_table_fences(input);
    let source = normalize_pipe_tables(source.as_ref());
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(source.as_ref(), options);
    let mut w = Writer::new(parser, width);
    w.run();
    w.text
}

struct Writer<'a, I>
where
    I: Iterator<Item = Event<'a>>,
{
    iter: I,
    text: Text<'static>,
    styles: MarkdownStyles,
    inline_styles: Vec<Style>,
    indent_stack: Vec<IndentContext>,
    list_indices: Vec<Option<u64>>,
    link: Option<String>,
    needs_newline: bool,
    pending_marker_line: bool,
    in_paragraph: bool,
    in_code_block: bool,
    code_block_buffer: String,
    code_block_lang: Option<String>,
    wrap_width: Option<usize>,
    current_line_content: Option<Line<'static>>,
    current_initial_indent: Vec<Span<'static>>,
    current_subsequent_indent: Vec<Span<'static>>,
    current_line_style: Style,
    current_line_in_code_block: bool,
    current_line_no_wrap: bool,
    table_state: Option<TableState>,
}

impl<'a, I> Writer<'a, I>
where
    I: Iterator<Item = Event<'a>>,
{
    fn new(iter: I, wrap_width: Option<usize>) -> Self {
        Self {
            iter,
            text: Text::default(),
            styles: MarkdownStyles::default(),
            inline_styles: Vec::new(),
            indent_stack: Vec::new(),
            list_indices: Vec::new(),
            link: None,
            needs_newline: false,
            pending_marker_line: false,
            in_paragraph: false,
            in_code_block: false,
            code_block_buffer: String::new(),
            code_block_lang: None,
            wrap_width,
            current_line_content: None,
            current_initial_indent: Vec::new(),
            current_subsequent_indent: Vec::new(),
            current_line_style: Style::default(),
            current_line_in_code_block: false,
            current_line_no_wrap: false,
            table_state: None,
        }
    }

    fn run(&mut self) {
        while let Some(ev) = self.iter.next() {
            self.handle_event(ev);
        }
        self.flush_current_line();
    }

    fn handle_event(&mut self, event: Event<'a>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.text(text),
            Event::Code(code) => self.code(code),
            Event::SoftBreak => self.soft_break(),
            Event::HardBreak => self.hard_break(),
            Event::Rule => {
                if self.is_table_cell_active() {
                    self.append_table_text("---");
                    return;
                }
                self.flush_current_line();
                if !self.text.lines.is_empty() {
                    self.push_blank_line();
                }
                self.push_line(Line::from("\u{2014}\u{2014}\u{2014}"));
                self.needs_newline = true;
            }
            Event::Html(html) => self.html(html, false),
            Event::InlineHtml(html) => self.html(html, true),
            Event::FootnoteReference(_) => {}
            Event::TaskListMarker(checked) => self.apply_task_marker(checked),
        }
    }

    fn start_tag(&mut self, tag: Tag<'a>) {
        match tag {
            Tag::Table(alignments) => self.start_table(alignments),
            Tag::TableHead => self.start_table_head(),
            Tag::TableRow => self.start_table_row(),
            Tag::TableCell => self.start_table_cell(),
            Tag::Emphasis => self.push_inline_style(self.styles.emphasis),
            Tag::Strong => self.push_inline_style(self.styles.strong),
            Tag::Strikethrough => self.push_inline_style(self.styles.strikethrough),
            Tag::Link { dest_url, .. } => self.push_link(dest_url.to_string()),
            _ if self.table_state.is_some() => {}
            Tag::Paragraph => self.start_paragraph(),
            Tag::Heading { level, .. } => self.start_heading(level),
            Tag::BlockQuote => self.start_blockquote(),
            Tag::CodeBlock(kind) => {
                let indent = match kind {
                    CodeBlockKind::Fenced(_) => None,
                    CodeBlockKind::Indented => Some(Span::from(" ".repeat(4))),
                };
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => Some(lang.to_string()),
                    CodeBlockKind::Indented => None,
                };
                self.start_codeblock(lang, indent)
            }
            Tag::List(start) => self.start_list(start),
            Tag::Item => self.start_item(),
            Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::Image { .. }
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Table => self.end_table(),
            TagEnd::TableHead => self.end_table_head(),
            TagEnd::TableRow => self.end_table_row(),
            TagEnd::TableCell => self.end_table_cell(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_inline_style(),
            TagEnd::Link => self.pop_link(),
            _ if self.table_state.is_some() => {}
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Heading(_) => self.end_heading(),
            TagEnd::BlockQuote => self.end_blockquote(),
            TagEnd::CodeBlock => self.end_codeblock(),
            TagEnd::List(_) => self.end_list(),
            TagEnd::Item => {
                self.indent_stack.pop();
                self.pending_marker_line = false;
            }
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::Image
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    fn start_paragraph(&mut self) {
        if self.needs_newline {
            self.push_blank_line();
        }
        self.push_line(Line::default());
        self.needs_newline = false;
        self.in_paragraph = true;
    }

    fn end_paragraph(&mut self) {
        self.needs_newline = true;
        self.in_paragraph = false;
        self.pending_marker_line = false;
    }

    fn start_heading(&mut self, level: HeadingLevel) {
        if self.needs_newline {
            self.push_line(Line::default());
            self.needs_newline = false;
        }
        let heading_style = match level {
            HeadingLevel::H1 => self.styles.h1,
            HeadingLevel::H2 => self.styles.h2,
            HeadingLevel::H3 => self.styles.h3,
            HeadingLevel::H4 => self.styles.h4,
            HeadingLevel::H5 => self.styles.h5,
            HeadingLevel::H6 => self.styles.h6,
        };
        let content = format!("{} ", "#".repeat(level as usize));
        self.push_line(Line::from(vec![Span::styled(content, heading_style)]));
        self.push_inline_style(heading_style);
        self.needs_newline = false;
    }

    fn end_heading(&mut self) {
        self.needs_newline = true;
        self.pop_inline_style();
    }

    fn start_blockquote(&mut self) {
        if self.needs_newline {
            self.push_blank_line();
            self.needs_newline = false;
        }
        self.indent_stack
            .push(IndentContext::new(vec![Span::from("> ")], None, false));
    }

    fn end_blockquote(&mut self) {
        self.indent_stack.pop();
        self.needs_newline = true;
    }

    fn text(&mut self, text: CowStr<'a>) {
        if self.is_table_cell_active() {
            self.append_table_text(text.as_ref());
            return;
        }
        if self.in_code_block {
            self.code_block_buffer.push_str(text.as_ref());
            self.needs_newline = false;
            return;
        }
        if self.pending_marker_line {
            self.push_line(Line::default());
        }
        self.pending_marker_line = false;
        for (i, line) in text.lines().enumerate() {
            if self.needs_newline {
                self.push_line(Line::default());
                self.needs_newline = false;
            }
            if i > 0 {
                self.push_line(Line::default());
            }
            let content = line.to_string();
            let span = Span::styled(
                content,
                self.inline_styles.last().copied().unwrap_or_default(),
            );
            self.push_span(span);
        }
        self.needs_newline = false;
    }

    fn code(&mut self, code: CowStr<'a>) {
        if self.is_table_cell_active() {
            self.append_table_span(Span::from(code.into_string()).style(self.styles.code));
            return;
        }
        if self.pending_marker_line {
            self.push_line(Line::default());
            self.pending_marker_line = false;
        }
        let span = Span::from(code.into_string()).style(self.styles.code);
        self.push_span(span);
    }

    fn html(&mut self, html: CowStr<'a>, inline: bool) {
        if self.is_table_cell_active() {
            self.append_table_text(html.as_ref());
            return;
        }
        self.pending_marker_line = false;
        for (i, line) in html.lines().enumerate() {
            if self.needs_newline {
                self.push_line(Line::default());
                self.needs_newline = false;
            }
            if i > 0 {
                self.push_line(Line::default());
            }
            let style = self.inline_styles.last().copied().unwrap_or_default();
            self.push_span(Span::styled(line.to_string(), style));
        }
        self.needs_newline = !inline;
    }

    fn hard_break(&mut self) {
        if self.is_table_cell_active() {
            if let Some(cell) = self.table_state.as_mut().and_then(TableState::cell_mut) {
                cell.hard_break();
            }
            return;
        }
        if self.in_code_block {
            self.code_block_buffer.push('\n');
            return;
        }
        self.push_line(Line::default());
    }

    fn soft_break(&mut self) {
        if self.is_table_cell_active() {
            self.append_table_text(" ");
            return;
        }
        if self.in_code_block {
            self.code_block_buffer.push('\n');
            return;
        }
        self.push_line(Line::default());
    }

    fn start_list(&mut self, index: Option<u64>) {
        if self.list_indices.is_empty() && self.needs_newline {
            self.push_line(Line::default());
        }
        self.list_indices.push(index);
    }

    fn end_list(&mut self) {
        self.list_indices.pop();
        self.needs_newline = true;
    }

    fn start_item(&mut self) {
        self.pending_marker_line = true;
        let depth = self.list_indices.len();
        let is_ordered = self
            .list_indices
            .last()
            .map(Option::is_some)
            .unwrap_or(false);
        let width = depth * 4 - 3;
        let marker = if let Some(last_index) = self.list_indices.last_mut() {
            match last_index {
                None => Some(vec![Span::styled(
                    " ".repeat(width - 1) + "\u{2022} ",
                    self.styles.unordered_list_marker,
                )]),
                Some(index) => {
                    *index += 1;
                    Some(vec![Span::styled(
                        format!("{:width$}. ", *index - 1),
                        self.styles.ordered_list_marker,
                    )])
                }
            }
        } else {
            None
        };
        let indent_prefix = if depth == 0 {
            Vec::new()
        } else {
            let indent_len = if is_ordered { width + 2 } else { width + 1 };
            vec![Span::from(" ".repeat(indent_len))]
        };
        self.indent_stack
            .push(IndentContext::new(indent_prefix, marker, true));
        self.needs_newline = false;
    }

    fn start_codeblock(&mut self, lang: Option<String>, indent: Option<Span<'static>>) {
        self.flush_current_line();
        if !self.text.lines.is_empty() {
            self.push_blank_line();
        }
        self.in_code_block = true;
        self.code_block_buffer.clear();
        self.code_block_lang = lang
            .as_deref()
            .and_then(normalize_code_block_language)
            .map(str::to_string);
        self.indent_stack.push(IndentContext::new(
            vec![indent.unwrap_or_default()],
            None,
            false,
        ));
        self.needs_newline = false;
    }

    fn end_codeblock(&mut self) {
        let code = std::mem::take(&mut self.code_block_buffer);
        let lang = self.code_block_lang.take();
        if !code.is_empty() {
            let lines = lang
                .as_deref()
                .and_then(|language| highlight_code_to_lines(code.as_str(), language))
                .unwrap_or_else(|| plain_code_block_lines(code.as_str(), self.styles.code));
            for line in lines {
                self.push_line_no_wrap(line);
            }
            self.flush_current_line();
        }
        self.needs_newline = true;
        self.in_code_block = false;
        self.indent_stack.pop();
    }

    /// A task item trades its bullet for a checkbox and keeps any numbering ahead of it,
    /// the same shape codex marks up. The hanging indent follows the new marker width.
    fn apply_task_marker(&mut self, checked: bool) {
        let checkbox = if checked { "\u{2611} " } else { "\u{2610} " };
        let Some(context) = self
            .indent_stack
            .iter_mut()
            .rev()
            .find(|context| context.marker.is_some())
        else {
            return;
        };
        let Some(marker) = context.marker.as_mut() else {
            return;
        };
        let Some(span) = marker.last_mut() else {
            return;
        };
        let content = span.content.as_ref();
        let head = content
            .strip_suffix("\u{2022} ")
            .or_else(|| content.strip_suffix("- "))
            .unwrap_or(content);
        span.content = if head == content {
            format!("{content}{checkbox}").into()
        } else {
            format!("{head}{checkbox}").into()
        };
        if checked {
            span.style = span.style.add_modifier(Modifier::BOLD);
        }
        let marker_width: usize = marker
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum();
        context.prefix = vec![Span::from(" ".repeat(marker_width))];
    }

    fn push_inline_style(&mut self, style: Style) {
        let current = self.inline_styles.last().copied().unwrap_or_default();
        let merged = current.patch(style);
        self.inline_styles.push(merged);
    }

    fn pop_inline_style(&mut self) {
        self.inline_styles.pop();
    }

    fn push_link(&mut self, dest_url: String) {
        self.link = Some(dest_url);
    }

    fn pop_link(&mut self) {
        if let Some(link) = self.link.take() {
            if self.is_table_cell_active() {
                self.append_table_span(Span::raw(" ("));
                self.append_table_span(Span::styled(link, self.styles.link));
                self.append_table_span(Span::raw(")"));
            } else {
                self.push_span(" (".into());
                self.push_span(Span::styled(link, self.styles.link));
                self.push_span(")".into());
            }
        }
    }

    fn flush_current_line(&mut self) {
        if let Some(line) = self.current_line_content.take() {
            let style = self.current_line_style;
            // Do not wrap code in code blocks, to preserve whitespace for copy/paste.
            if !self.current_line_in_code_block && !self.current_line_no_wrap {
                if let Some(width) = self.wrap_width {
                    let opts = RtOptions::new(width)
                        .initial_indent(self.current_initial_indent.clone().into())
                        .subsequent_indent(self.current_subsequent_indent.clone().into());
                    for wrapped in word_wrap_line(&line, opts) {
                        let owned = line_to_static(&wrapped).style(style);
                        self.text.lines.push(owned);
                    }
                } else {
                    let mut spans = self.current_initial_indent.clone();
                    let mut line = line;
                    spans.append(&mut line.spans);
                    self.text.lines.push(Line::from_iter(spans).style(style));
                }
            } else {
                let mut spans = self.current_initial_indent.clone();
                let mut line = line;
                spans.append(&mut line.spans);
                self.text.lines.push(Line::from_iter(spans).style(style));
            }
            self.current_initial_indent.clear();
            self.current_subsequent_indent.clear();
            self.current_line_in_code_block = false;
            self.current_line_no_wrap = false;
        }
    }

    fn push_line(&mut self, line: Line<'static>) {
        self.push_line_with_wrap(line, false);
    }

    fn push_line_no_wrap(&mut self, line: Line<'static>) {
        self.push_line_with_wrap(line, true);
    }

    fn push_line_with_wrap(&mut self, line: Line<'static>, no_wrap: bool) {
        self.flush_current_line();
        let blockquote_active = self
            .indent_stack
            .iter()
            .any(|ctx| ctx.prefix.iter().any(|s| s.content.contains('>')));
        let style = if blockquote_active {
            self.styles.blockquote
        } else {
            line.style
        };
        let was_pending = self.pending_marker_line;

        self.current_initial_indent = self.prefix_spans(was_pending);
        self.current_subsequent_indent = self.prefix_spans(false);
        self.current_line_style = style;
        self.current_line_content = Some(line);
        self.current_line_in_code_block = self.in_code_block;
        self.current_line_no_wrap = no_wrap;

        self.pending_marker_line = false;
    }

    fn push_span(&mut self, span: Span<'static>) {
        if let Some(line) = self.current_line_content.as_mut() {
            line.push_span(span);
        } else {
            self.push_line(Line::from(vec![span]));
        }
    }

    fn push_blank_line(&mut self) {
        self.flush_current_line();
        if self.indent_stack.iter().all(|ctx| ctx.is_list) {
            self.text.lines.push(Line::default());
        } else {
            self.push_line(Line::default());
            self.flush_current_line();
        }
    }

    fn prefix_spans(&self, pending_marker_line: bool) -> Vec<Span<'static>> {
        let mut prefix: Vec<Span<'static>> = Vec::new();
        let last_marker_index = if pending_marker_line {
            self.indent_stack
                .iter()
                .enumerate()
                .rev()
                .find_map(|(i, ctx)| if ctx.marker.is_some() { Some(i) } else { None })
        } else {
            None
        };
        let last_list_index = self.indent_stack.iter().rposition(|ctx| ctx.is_list);

        for (i, ctx) in self.indent_stack.iter().enumerate() {
            if pending_marker_line {
                if Some(i) == last_marker_index {
                    if let Some(marker) = &ctx.marker {
                        prefix.extend(marker.iter().cloned());
                        continue;
                    }
                }
                if ctx.is_list && last_marker_index.is_some_and(|idx| idx > i) {
                    continue;
                }
            } else if ctx.is_list && Some(i) != last_list_index {
                continue;
            }
            prefix.extend(ctx.prefix.iter().cloned());
        }

        prefix
    }

    fn is_table_cell_active(&self) -> bool {
        self.table_state
            .as_ref()
            .is_some_and(|state| state.in_cell())
    }

    fn append_table_span(&mut self, span: Span<'static>) {
        if let Some(cell) = self.table_state.as_mut().and_then(TableState::cell_mut) {
            cell.push_span(span);
        }
    }

    fn append_table_text(&mut self, text: &str) {
        let style = self.inline_styles.last().copied().unwrap_or_default();
        for (index, line) in text.lines().enumerate() {
            if index > 0 {
                if let Some(cell) = self.table_state.as_mut().and_then(TableState::cell_mut) {
                    cell.hard_break();
                }
            }
            self.append_table_span(Span::styled(line.to_string(), style));
        }
    }

    fn start_table(&mut self, alignments: Vec<Alignment>) {
        self.flush_current_line();
        if self.needs_newline {
            self.push_blank_line();
            self.needs_newline = false;
        }
        self.table_state = Some(TableState::new(alignments));
    }

    fn start_table_head(&mut self) {
        if let Some(state) = self.table_state.as_mut() {
            state.start_head();
        }
    }

    fn start_table_row(&mut self) {
        if let Some(state) = self.table_state.as_mut() {
            state.start_row();
        }
    }

    fn start_table_cell(&mut self) {
        if let Some(state) = self.table_state.as_mut() {
            state.start_cell();
        }
    }

    fn end_table_cell(&mut self) {
        if let Some(state) = self.table_state.as_mut() {
            state.end_cell();
        }
    }

    fn end_table_row(&mut self) {
        if let Some(state) = self.table_state.as_mut() {
            state.end_row();
        }
    }

    fn end_table_head(&mut self) {
        if let Some(state) = self.table_state.as_mut() {
            state.end_head();
        }
    }

    fn end_table(&mut self) {
        let Some(state) = self.table_state.take() else {
            return;
        };
        let prefix_width = self
            .prefix_spans(self.pending_marker_line)
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum();
        let RenderedTable { lines, prewrapped } = table::render(
            state,
            self.wrap_width,
            prefix_width,
            self.styles.strong,
            super::theme::secondary_text(),
        );
        for line in lines {
            if prewrapped {
                self.push_line_no_wrap(line);
            } else {
                self.push_line(line);
            }
            self.flush_current_line();
        }
        self.pending_marker_line = false;
        self.needs_newline = true;
    }
}

fn normalize_code_block_language(raw: &str) -> Option<&str> {
    raw.split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
        .find(|value| !value.trim().is_empty())
        .map(str::trim)
}

fn plain_code_block_lines(code: &str, style: Style) -> Vec<Line<'static>> {
    let mut lines = code
        .split_inclusive('\n')
        .map(|line| line.trim_end_matches(['\r', '\n']))
        .map(|line| Line::from(Span::styled(line.to_string(), style)))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(input: &str, width: usize) -> Vec<String> {
        render_markdown_text_with_width(input, Some(width))
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn task_items_trade_the_bullet_for_a_checkbox() {
        let lines = rendered("- [ ] open\n- [x] done\n", 40);
        assert!(
            lines.iter().any(|line| line.starts_with("\u{2610} open")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.starts_with("\u{2611} done")),
            "{lines:?}"
        );
        assert!(!lines.iter().any(|line| line.contains("[ ]")), "{lines:?}");
    }

    #[test]
    fn unordered_items_use_the_reference_bullet_and_nest_by_four() {
        let lines = rendered("- top\n  - nested\n", 40);
        assert!(lines.iter().any(|line| line == "\u{2022} top"), "{lines:?}");
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("    \u{2022} nested")),
            "{lines:?}"
        );
    }

    #[test]
    fn a_rule_renders_as_a_bar_not_source_punctuation() {
        assert!(
            rendered("before\n\n---\n\nafter\n", 40)
                .iter()
                .any(|line| line == "\u{2014}\u{2014}\u{2014}"),
            "{:?}",
            rendered("before\n\n---\n\nafter\n", 40)
        );
    }

    #[test]
    fn a_markdown_fence_holding_a_table_is_rendered_as_a_table() {
        let source = "```markdown\n| gate | value |\n| --- | --- |\n| flaky | false |\n```\n";
        let lines = rendered(source, 40);
        let header = lines
            .iter()
            .find(|line| line.contains("gate") && line.contains("value"))
            .unwrap_or_else(|| panic!("the fenced table became a grid: {lines:?}"));
        assert!(lines.iter().any(|line| line.contains("flaky")), "{lines:?}");
        assert!(lines.iter().any(|line| line.contains('━')), "{lines:?}");
        assert!(!header.contains("markdown"), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("```")), "{lines:?}");
    }

    #[test]
    fn a_pipe_table_without_outer_pipes_is_rendered_as_a_grid() {
        let lines = rendered("Name | Value\n--- | ---\nalpha | 42\n", 40);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Name") && line.contains("Value")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("alpha") && line.contains("42")),
            "{lines:?}"
        );
        assert!(!lines.iter().any(|line| line.contains('|')), "{lines:?}");
    }

    #[test]
    fn a_fence_that_is_not_a_table_stays_code() {
        let source = "```markdown\nnot a table at all\n```\n";
        let lines = rendered(source, 40);
        assert!(
            lines.iter().any(|line| line.contains("not a table at all")),
            "{lines:?}"
        );
        assert_eq!(lines.len(), 1, "{lines:?}");
    }

    #[test]
    fn a_wide_table_shrinks_and_wraps_inside_the_frame() {
        let source = "| column one | column two | column three |\n| --- | --- | --- |\n\
                      | a very long cell that will not fit | another long cell here | third |\n";
        for width in [20usize, 24, 40, 80, 120] {
            let lines = rendered(source, width);
            assert!(
                lines
                    .iter()
                    .all(|line| UnicodeWidthStr::width(line.as_str()) <= width),
                "width {width}: a row escaped the frame: {lines:?}"
            );
            let joined = lines.join(" ");
            let words = joined.split_whitespace().collect::<Vec<_>>();
            for word in [
                "column", "one", "two", "three", "a", "very", "long", "cell", "that", "will",
                "not", "fit", "another", "here", "third",
            ] {
                assert!(
                    words.iter().any(|seen| *seen == word),
                    "width {width}: lost {word}"
                );
            }
        }
    }

    #[test]
    fn a_narrow_table_falls_back_to_key_value_records() {
        let source = "| gate | value | note |\n| --- | --- | --- |\n| a | b | c |\n";
        let lines = rendered(source, 12);
        let joined = lines.join("\n");
        for token in ["gate", "value", "note", "a", "b", "c"] {
            assert!(joined.contains(token), "lost {token}: {lines:?}");
        }
        assert!(!joined.contains("| ---"), "{lines:?}");
        assert!(
            lines
                .iter()
                .any(|line| line.trim_start().starts_with("gate")),
            "the first field name should introduce the record: {lines:?}"
        );
    }

    #[test]
    fn table_cells_keep_inline_styles() {
        let source = "| State | Result |\n| --- | --- |\n| **ready** | `ok` |\n";
        let text = render_markdown_text_with_width(source, Some(40));
        let row = text
            .lines
            .iter()
            .find(|line| line.spans.iter().any(|span| span.content == "ready"))
            .expect("styled table row");
        let ready = row
            .spans
            .iter()
            .find(|span| span.content == "ready")
            .expect("bold cell span");
        assert!(ready.style.add_modifier.contains(Modifier::BOLD));
        let code = row
            .spans
            .iter()
            .find(|span| span.content == "ok")
            .expect("code cell span");
        assert_eq!(code.style, crate::tui::theme::code_text());
    }

    #[test]
    fn unicode_and_long_tokens_stay_inside_the_selected_layout() {
        let source = "| 名称 | 说明 |\n| --- | --- |\n| 中文单元格 | 一段需要换行的中文内容 |\n| 路径 | /a/very/long/path/that/cannot/fit |\n";
        for width in [16usize, 24, 40] {
            let lines = rendered(source, width);
            assert!(
                lines
                    .iter()
                    .all(|line| UnicodeWidthStr::width(line.as_str()) <= width),
                "width {width}: a row escaped the frame: {lines:?}"
            );
            let compact = lines
                .concat()
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>();
            for token in [
                "中文单元格",
                "一段需要换行的中文内容",
                "/a/very/long/path/that/cannot/fit",
            ] {
                assert!(
                    compact.contains(token),
                    "width {width}: lost {token}: {lines:?}"
                );
            }
        }
    }

    #[test]
    fn code_block_lines_keep_their_leading_whitespace() {
        let lines = rendered("```rust\nfn main() {\n    let x = 1;\n}\n```\n", 40);
        assert!(
            lines.iter().any(|line| line.starts_with("    let x = 1;")),
            "{lines:?}"
        );
    }
}
