use super::line_utils::line_to_static;
use super::wrapping::RtOptions;
use super::wrapping::word_wrap_line;
use pulldown_cmark::Alignment;
use pulldown_cmark::CodeBlockKind;
use pulldown_cmark::CowStr;
use pulldown_cmark::Event;
use pulldown_cmark::HeadingLevel;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use unicode_width::UnicodeWidthStr;

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
            code: Style::new().cyan(),
            emphasis: Style::new().italic(),
            strong: Style::new().bold(),
            strikethrough: Style::new().crossed_out(),
            ordered_list_marker: Style::new().light_blue(),
            unordered_list_marker: Style::new(),
            link: Style::new().cyan().underlined(),
            blockquote: Style::new().green(),
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

#[derive(Clone, Debug)]
struct TableState {
    alignments: Vec<Alignment>,
    head_rows: Vec<Vec<String>>,
    body_rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    in_head: bool,
    in_cell: bool,
}

impl TableState {
    fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            head_rows: Vec::new(),
            body_rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
            in_head: false,
            in_cell: false,
        }
    }

    fn start_head(&mut self) {
        self.in_head = true;
    }

    fn end_head(&mut self) {
        self.in_head = false;
    }

    fn start_row(&mut self) {
        self.current_row.clear();
    }

    fn end_row(&mut self) {
        self.end_cell_if_open();
        if self.current_row.is_empty() {
            return;
        }
        if self.in_head {
            self.head_rows.push(std::mem::take(&mut self.current_row));
        } else {
            self.body_rows.push(std::mem::take(&mut self.current_row));
        }
    }

    fn start_cell(&mut self) {
        self.current_cell.clear();
        self.in_cell = true;
    }

    fn end_cell(&mut self) {
        let cell = normalize_table_cell(self.current_cell.as_str());
        self.current_row.push(cell);
        self.current_cell.clear();
        self.in_cell = false;
    }

    fn end_cell_if_open(&mut self) {
        if self.in_cell {
            self.end_cell();
        }
    }

    fn push_text(&mut self, text: &str) {
        if !self.in_cell {
            return;
        }
        self.current_cell.push_str(text);
    }
}

pub(crate) fn render_markdown_text_with_width(input: &str, width: Option<usize>) -> Text<'static> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(input, options);
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
                self.push_line(Line::from("---"));
                self.needs_newline = true;
            }
            Event::Html(html) => self.html(html, false),
            Event::InlineHtml(html) => self.html(html, true),
            Event::FootnoteReference(_) => {}
            Event::TaskListMarker(_) => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'a>) {
        if matches!(
            tag,
            Tag::Table(_) | Tag::TableHead | Tag::TableRow | Tag::TableCell
        ) {
            match tag {
                Tag::Table(alignments) => self.start_table(alignments),
                Tag::TableHead => self.start_table_head(),
                Tag::TableRow => self.start_table_row(),
                Tag::TableCell => self.start_table_cell(),
                _ => {}
            }
            return;
        }
        if self.table_state.is_some() {
            return;
        }
        match tag {
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
            Tag::Emphasis => self.push_inline_style(self.styles.emphasis),
            Tag::Strong => self.push_inline_style(self.styles.strong),
            Tag::Strikethrough => self.push_inline_style(self.styles.strikethrough),
            Tag::Link { dest_url, .. } => self.push_link(dest_url.to_string()),
            Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::Image { .. }
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        if matches!(
            tag,
            TagEnd::Table | TagEnd::TableHead | TagEnd::TableRow | TagEnd::TableCell
        ) {
            match tag {
                TagEnd::Table => self.end_table(),
                TagEnd::TableHead => self.end_table_head(),
                TagEnd::TableRow => self.end_table_row(),
                TagEnd::TableCell => self.end_table_cell(),
                _ => {}
            }
            return;
        }
        if self.table_state.is_some() {
            return;
        }
        match tag {
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Heading(_) => self.end_heading(),
            TagEnd::BlockQuote => self.end_blockquote(),
            TagEnd::CodeBlock => self.end_codeblock(),
            TagEnd::List(_) => self.end_list(),
            TagEnd::Item => {
                self.indent_stack.pop();
                self.pending_marker_line = false;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_inline_style(),
            TagEnd::Link => self.pop_link(),
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
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
        if self.pending_marker_line {
            self.push_line(Line::default());
        }
        self.pending_marker_line = false;
        if self.in_code_block && !self.needs_newline {
            let has_content = self
                .current_line_content
                .as_ref()
                .map(|line| !line.spans.is_empty())
                .unwrap_or_else(|| {
                    self.text
                        .lines
                        .last()
                        .map(|line| !line.spans.is_empty())
                        .unwrap_or(false)
                });
            if has_content {
                self.push_line(Line::default());
            }
        }
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
            self.append_table_text(code.as_ref());
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
            self.append_table_text(" ");
            return;
        }
        self.push_line(Line::default());
    }

    fn soft_break(&mut self) {
        if self.is_table_cell_active() {
            self.append_table_text(" ");
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
                    " ".repeat(width - 1) + "- ",
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

    fn start_codeblock(&mut self, _lang: Option<String>, indent: Option<Span<'static>>) {
        self.flush_current_line();
        if !self.text.lines.is_empty() {
            self.push_blank_line();
        }
        self.in_code_block = true;
        self.indent_stack.push(IndentContext::new(
            vec![indent.unwrap_or_default()],
            None,
            false,
        ));
        self.needs_newline = true;
    }

    fn end_codeblock(&mut self) {
        self.needs_newline = true;
        self.in_code_block = false;
        self.indent_stack.pop();
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
            self.push_span(" (".into());
            self.push_span(Span::styled(link, self.styles.link));
            self.push_span(")".into());
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
            .is_some_and(|state| state.in_cell)
    }

    fn append_table_text(&mut self, text: &str) {
        if let Some(state) = self.table_state.as_mut() {
            state.push_text(text);
        }
    }

    fn start_table(&mut self, alignments: Vec<Alignment>) {
        if self.pending_marker_line {
            self.push_line(Line::default());
        }
        self.pending_marker_line = false;
        if self.needs_newline {
            self.push_blank_line();
            self.needs_newline = false;
        }
        self.flush_current_line();
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
        self.flush_table(state);
        self.needs_newline = true;
    }

    fn flush_table(&mut self, state: TableState) {
        let mut rows = Vec::new();
        for row in &state.head_rows {
            rows.push(row.clone());
        }
        for row in &state.body_rows {
            rows.push(row.clone());
        }
        if rows.is_empty() {
            return;
        }

        let mut col_count = state.alignments.len();
        for row in &rows {
            col_count = col_count.max(row.len());
        }
        if col_count == 0 {
            return;
        }

        let mut widths = vec![3usize; col_count];
        for row in &rows {
            for (idx, cell) in row.iter().enumerate() {
                let width = UnicodeWidthStr::width(cell.as_str());
                widths[idx] = widths[idx].max(width);
            }
        }
        for width in &mut widths {
            if *width < 3 {
                *width = 3;
            }
        }

        let alignments = state.alignments;
        let mut rendered_rows: Vec<(String, Option<Style>)> = Vec::new();
        for row in &state.head_rows {
            let line = format_table_row(row, &widths, &alignments);
            rendered_rows.push((line, Some(self.styles.strong)));
        }
        if !state.head_rows.is_empty() {
            let separator = format_table_separator(&widths, &alignments);
            rendered_rows.push((separator, None));
        }
        for row in &state.body_rows {
            let line = format_table_row(row, &widths, &alignments);
            rendered_rows.push((line, None));
        }

        for (line, style) in rendered_rows {
            let span = if let Some(style) = style {
                Span::styled(line, style)
            } else {
                Span::from(line)
            };
            self.push_line_no_wrap(Line::from(span));
            self.flush_current_line();
        }
    }
}

fn normalize_table_cell(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut last_space = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}

fn format_table_row(row: &[String], widths: &[usize], alignments: &[Alignment]) -> String {
    let mut line = String::new();
    line.push('|');
    for (idx, width) in widths.iter().enumerate() {
        let cell = row.get(idx).map(String::as_str).unwrap_or("");
        let cell_width = UnicodeWidthStr::width(cell);
        let pad = width.saturating_sub(cell_width);
        let align = alignments.get(idx).copied().unwrap_or(Alignment::None);
        let (left_pad, right_pad) = match align {
            Alignment::Right => (pad, 0),
            Alignment::Center => {
                let left = pad / 2;
                (left, pad - left)
            }
            _ => (0, pad),
        };
        line.push(' ');
        if left_pad > 0 {
            line.push_str(&" ".repeat(left_pad));
        }
        line.push_str(cell);
        if right_pad > 0 {
            line.push_str(&" ".repeat(right_pad));
        }
        line.push(' ');
        line.push('|');
    }
    line
}

fn format_table_separator(widths: &[usize], alignments: &[Alignment]) -> String {
    let mut line = String::new();
    line.push('|');
    for (idx, width) in widths.iter().enumerate() {
        let align = alignments.get(idx).copied().unwrap_or(Alignment::None);
        let mut dashes = "-".repeat((*width).max(3));
        match align {
            Alignment::Left => {
                if !dashes.is_empty() {
                    dashes.replace_range(0..1, ":");
                }
            }
            Alignment::Right => {
                if !dashes.is_empty() {
                    let last = dashes.len().saturating_sub(1);
                    dashes.replace_range(last..=last, ":");
                }
            }
            Alignment::Center => {
                if !dashes.is_empty() {
                    dashes.replace_range(0..1, ":");
                    let last = dashes.len().saturating_sub(1);
                    if last > 0 {
                        dashes.replace_range(last..=last, ":");
                    }
                }
            }
            Alignment::None => {}
        }
        line.push(' ');
        line.push_str(&dashes);
        line.push(' ');
        line.push('|');
    }
    line
}
