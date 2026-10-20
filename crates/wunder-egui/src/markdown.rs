//! Bounded Markdown projection for assistant messages.
//!
//! Mirrors the Slint shell's approach: while a response streams the body stays
//! plain text, because parsing every frame would re-wrap the whole message on
//! each token. Once the turn settles the visible prefix is parsed once with
//! `pulldown-cmark` — the parser the Slint shell and the runtime already use —
//! into a handful of blocks.
//!
//! Text blocks keep a marker-free projection so the whole run stays
//! drag-selectable: consecutive paragraphs, list items and quotes share one
//! block instead of one widget per line, which is why the Slint shell keeps
//! lists inside their surrounding text block too. Code, tables and rules are
//! separate blocks because they own their own chrome.

use crate::code_highlight::TokenKind;
use egui::text::LayoutJob;
use egui::{
    Align, Color32, FontId, Frame, Label, Layout, Margin, RichText, ScrollArea, Sense, Stroke,
    TextFormat, Ui, Vec2,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

/// Display cap, matching the Slint shell. The copy action keeps the full text.
const DISPLAY_BYTES: usize = 16 * 1024;
const DISPLAY_LINES: usize = 160;
const BODY_SIZE: f32 = 14.0;
/// Table cells keep a 150px floor so a narrow column cannot crush a paragraph.
const TABLE_MIN_COLUMN: f32 = 150.0;
const TABLE_MAX_HEIGHT: f32 = 340.0;
/// Parsed messages kept per thread; a long thread re-parses only new text.
const CACHE_CAPACITY: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct InlineStyle {
    strong: bool,
    emphasis: bool,
    strike: bool,
    code: bool,
}

struct Inline {
    text: String,
    style: InlineStyle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextKind {
    Body,
    Quote,
    Heading(u8),
}

enum Block {
    Text {
        kind: TextKind,
        inlines: Vec<Inline>,
    },
    Code {
        language: String,
        text: String,
    },
    Table {
        columns: usize,
        /// The first row is the header; `pulldown-cmark` hands it over separately.
        rows: Vec<Vec<String>>,
    },
    Rule,
}

fn push_run(run: &mut Vec<Inline>, text: &str) {
    if text.is_empty() {
        return;
    }
    match run.last_mut() {
        Some(last) if last.style == InlineStyle::default() => last.text.push_str(text),
        _ => run.push(Inline {
            text: text.to_owned(),
            style: InlineStyle::default(),
        }),
    }
}

fn push_styled(run: &mut Vec<Inline>, text: &str, style: InlineStyle) {
    if text.is_empty() {
        return;
    }
    match run.last_mut() {
        Some(last) if last.style == style => last.text.push_str(text),
        _ => run.push(Inline {
            text: text.to_owned(),
            style,
        }),
    }
}

fn run_tail_char(run: &[Inline]) -> Option<char> {
    run.iter()
        .rev()
        .find_map(|inline| inline.text.chars().next_back())
}

fn trailing_newlines(run: &[Inline]) -> usize {
    run.iter()
        .rev()
        .map(|inline| {
            inline
                .text
                .chars()
                .rev()
                .take_while(|character| *character == '\n')
                .count()
                .min(inline.text.chars().count())
        })
        .take_while(|count| *count > 0)
        .sum()
}

/// Paragraphs are separated by a blank line in body text and a single break
/// inside quotes and list items: the quote panel already separates itself, and
/// a gap between list items would fight the marker column.
fn ensure_paragraph_gap(run: &mut Vec<Inline>, wanted: usize) {
    for _ in trailing_newlines(run)..wanted {
        push_run(run, "\n");
    }
}

fn flush(blocks: &mut Vec<Block>, run: &mut Vec<Inline>, kind: &mut TextKind) {
    while run
        .last()
        .is_some_and(|inline| inline.text.trim().is_empty() && inline.text.contains('\n'))
    {
        run.pop();
    }
    if !run.is_empty() {
        blocks.push(Block::Text {
            kind: *kind,
            inlines: std::mem::take(run),
        });
    }
    *kind = TextKind::Body;
}

fn code_language(kind: &CodeBlockKind<'_>) -> String {
    match kind {
        CodeBlockKind::Fenced(info) => info
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
        CodeBlockKind::Indented => String::new(),
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[derive(Default)]
struct TableBuilder {
    columns: usize,
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
}

impl TableBuilder {
    fn close_row(&mut self) {
        let row = std::mem::take(&mut self.row);
        if !row.is_empty() {
            self.columns = self.columns.max(row.len());
            self.rows.push(row);
        }
    }

    fn finish(mut self) -> Block {
        if !self.cell.is_empty() {
            self.row.push(std::mem::take(&mut self.cell));
        }
        self.close_row();
        Block::Table {
            columns: self.columns.max(1),
            rows: self.rows,
        }
    }
}

fn parse(source: &str) -> Vec<Block> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut blocks: Vec<Block> = Vec::new();
    let mut run: Vec<Inline> = Vec::new();
    let mut kind = TextKind::Body;
    let mut style = InlineStyle::default();
    let mut link: Option<(String, String)> = None;
    let mut image: Option<(String, String)> = None;
    let mut code: Option<(String, String)> = None;
    let mut table: Option<TableBuilder> = None;
    let mut list_depth = 0usize;
    let mut ordered: Vec<Option<u64>> = Vec::new();
    // A soft break in the source is a wrap point, not a line break: English
    // words need the space back, CJK must not gain one.
    let mut soft_break = false;

    for event in Parser::new_ext(source, options) {
        if let Some((_, text)) = code.as_mut() {
            let mut finished = false;
            match &event {
                Event::End(TagEnd::CodeBlock) => finished = true,
                Event::Text(value)
                | Event::Code(value)
                | Event::Html(value)
                | Event::InlineHtml(value) => text.push_str(value),
                Event::SoftBreak | Event::HardBreak => text.push('\n'),
                _ => {}
            }
            if finished {
                if let Some((language, text)) = code.take() {
                    blocks.push(Block::Code {
                        language,
                        text: text.trim_end_matches('\n').to_owned(),
                    });
                }
            }
            continue;
        }
        if let Some(builder) = table.as_mut() {
            let mut finished = false;
            match &event {
                Event::End(TagEnd::Table) => finished = true,
                Event::Text(value) | Event::Code(value) | Event::Html(value) => {
                    builder.cell.push_str(value)
                }
                Event::SoftBreak | Event::HardBreak => builder.cell.push(' '),
                Event::End(TagEnd::TableCell) => {
                    builder.row.push(std::mem::take(&mut builder.cell))
                }
                Event::End(TagEnd::TableRow) | Event::End(TagEnd::TableHead) => builder.close_row(),
                _ => {}
            }
            if finished {
                if let Some(builder) = table.take() {
                    blocks.push(builder.finish());
                }
            }
            continue;
        }

        match event {
            // The gap belongs before the paragraph: appending it afterwards
            // would only work when another paragraph follows immediately, and
            // would be lost between a list and the prose below it.
            Event::Start(Tag::Paragraph) if !run.is_empty() => {
                let wanted = if list_depth > 0 || kind != TextKind::Body {
                    1
                } else {
                    2
                };
                ensure_paragraph_gap(&mut run, wanted);
            }
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut blocks, &mut run, &mut kind);
                kind = TextKind::Heading(heading_level(level));
            }
            Event::End(TagEnd::Heading(_)) => flush(&mut blocks, &mut run, &mut kind),
            Event::Start(Tag::CodeBlock(fence)) => {
                flush(&mut blocks, &mut run, &mut kind);
                code = Some((code_language(&fence), String::new()));
            }
            Event::Start(Tag::BlockQuote) => {
                flush(&mut blocks, &mut run, &mut kind);
                kind = TextKind::Quote;
            }
            Event::End(TagEnd::BlockQuote) => flush(&mut blocks, &mut run, &mut kind),
            Event::Start(Tag::List(start)) => {
                // A list after prose keeps the paragraph gap a browser would
                // give it; a nested list starts right under its parent marker.
                if list_depth == 0 && !run.is_empty() {
                    let wanted = if kind == TextKind::Body { 2 } else { 1 };
                    ensure_paragraph_gap(&mut run, wanted);
                }
                list_depth += 1;
                ordered.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                ordered.pop();
            }
            Event::Start(Tag::Item) => {
                if !run.is_empty() {
                    ensure_paragraph_gap(&mut run, 1);
                }
                push_run(&mut run, &"  ".repeat(list_depth.saturating_sub(1)));
                match ordered.last_mut() {
                    Some(Some(index)) => {
                        push_run(&mut run, &format!("{index}. "));
                        *index += 1;
                    }
                    _ => push_run(&mut run, "• "),
                }
            }
            Event::Start(Tag::Table(_)) => {
                flush(&mut blocks, &mut run, &mut kind);
                table = Some(TableBuilder::default());
            }
            Event::Start(Tag::Emphasis) => style.emphasis = true,
            Event::End(TagEnd::Emphasis) => style.emphasis = false,
            Event::Start(Tag::Strong) => style.strong = true,
            Event::End(TagEnd::Strong) => style.strong = false,
            Event::Start(Tag::Strikethrough) => style.strike = true,
            Event::End(TagEnd::Strikethrough) => style.strike = false,
            Event::Start(Tag::Link { dest_url, .. }) => {
                link = Some((dest_url.to_string(), String::new()))
            }
            Event::End(TagEnd::Link) => {
                if let Some((url, label)) = link.take() {
                    // The Slint shell keeps the label and appends the target
                    // once, skipping labels that already are the target; the
                    // full Markdown stays available through the copy action.
                    if label.trim() != url {
                        push_run(&mut run, &format!(" ({url})"));
                    }
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                image = Some((dest_url.to_string(), String::new()))
            }
            Event::End(TagEnd::Image) => {
                if let Some((source, alt)) = image.take() {
                    let label = if alt.trim().is_empty() {
                        "图片".to_owned()
                    } else {
                        alt
                    };
                    push_run(&mut run, &format!("{label}（{source}）"));
                }
            }
            Event::Text(value) | Event::Html(value) | Event::InlineHtml(value) => {
                match image.as_mut() {
                    Some((_, alt)) => alt.push_str(&value),
                    None => {
                        if let Some((_, label)) = link.as_mut() {
                            label.push_str(&value);
                        }
                        if soft_break {
                            let separator = match (run_tail_char(&run), value.chars().next()) {
                                (Some(left), Some(right))
                                    if left.is_ascii() && right.is_ascii() =>
                                {
                                    " "
                                }
                                _ => "",
                            };
                            push_styled(&mut run, separator, style);
                            soft_break = false;
                        }
                        push_styled(&mut run, &value, style);
                    }
                }
            }
            Event::Code(value) => {
                let mut code_style = style;
                code_style.code = true;
                if let Some((_, label)) = link.as_mut() {
                    label.push_str(&value);
                }
                push_styled(&mut run, &value, code_style);
            }
            Event::SoftBreak => soft_break = true,
            Event::HardBreak => push_run(&mut run, "\n"),
            Event::Rule => {
                flush(&mut blocks, &mut run, &mut kind);
                blocks.push(Block::Rule);
            }
            Event::TaskListMarker(checked) => push_run(&mut run, if checked { "☑ " } else { "☐ " }),
            Event::FootnoteReference(label) => push_run(&mut run, &format!("[{label}]")),
            _ => {}
        }
    }
    flush(&mut blocks, &mut run, &mut kind);
    blocks
}

thread_local! {
    static BLOCKS: RefCell<VecDeque<(u64, Arc<Vec<Block>>)>> =
        const { RefCell::new(VecDeque::new()) };
}

fn hash_str(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// The Slint shell parses once because its block model is stored per message;
/// egui redraws every frame, so the same "parse once" behaviour needs a small
/// cache keyed by the text itself.
fn blocks_for(source: &str) -> Arc<Vec<Block>> {
    let key = hash_str(source);
    if let Some(hit) = BLOCKS.with(|cache| {
        cache
            .borrow()
            .iter()
            .find(|(hash, _)| *hash == key)
            .map(|(_, blocks)| blocks.clone())
    }) {
        return hit;
    }
    let parsed = Arc::new(parse(source));
    BLOCKS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= CACHE_CAPACITY {
            cache.pop_back();
        }
        cache.push_front((key, parsed.clone()));
    });
    parsed
}

fn char_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn visible_source(text: &str) -> (&str, bool) {
    let byte_end = char_boundary(text, text.len().min(DISPLAY_BYTES));
    let display_end = text[..byte_end]
        .match_indices('\n')
        .nth(DISPLAY_LINES - 1)
        .map_or(byte_end, |(offset, _)| offset + 1);
    (&text[..display_end], text.len() > display_end)
}

fn heading_size(level: u8) -> f32 {
    match level {
        1 => 26.0,
        2 => 21.0,
        3 => 18.0,
        4 => 16.0,
        5 => 15.0,
        _ => BODY_SIZE,
    }
}

fn text_format(size: f32, color: Color32, mono: bool) -> TextFormat {
    TextFormat {
        font_id: if mono {
            FontId::monospace(size - 1.0)
        } else {
            FontId::proportional(size)
        },
        color,
        line_height: Some(size * 1.7),
        ..Default::default()
    }
}

/// Build the layout job for a text block. The shared YaHei collection has no
/// bold face, so strong runs are reported back for a glyph overdraw pass.
fn text_job(inlines: &[Inline], size: f32, color: Color32) -> (LayoutJob, Vec<Range<usize>>) {
    let tokens = crate::theme::tokens();
    let mut job = LayoutJob::default();
    let mut bold = Vec::new();
    for inline in inlines {
        let mut format = text_format(size, color, inline.style.code);
        if inline.style.code {
            format.background = tokens.code_inline;
        }
        if inline.style.emphasis {
            format.italics = true;
        }
        if inline.style.strike {
            format.strikethrough = Stroke::new(1.0, color);
        }
        let start = job.text.chars().count();
        let end = start + inline.text.chars().count();
        job.append(&inline.text, 0.0, format);
        if inline.style.strong && end > start {
            bold.push(start..end);
        }
    }
    (job, bold)
}

/// Paint a laid-out run and keep it selectable. Only runs that need the
/// overdraw take the manual path; everything else stays a plain label so egui
/// keeps caching the galley.
fn show_text(ui: &mut Ui, job: LayoutJob, bold: &[Range<usize>], color: Color32) {
    if job.text.is_empty() {
        return;
    }
    if bold.is_empty() {
        ui.add(Label::new(job).wrap().selectable(true));
        return;
    }
    let (pos, galley, response) = Label::new(job).wrap().selectable(true).layout_in_ui(ui);
    if !ui.is_rect_visible(response.rect) {
        return;
    }
    let mut overlay = (*galley).clone();
    overlay.num_vertices = 0;
    overlay.num_indices = 0;
    let mut bold = bold.iter().peekable();
    let mut char_index = 0;
    for row in &mut overlay.rows {
        let mut mesh = egui::epaint::Mesh::default();
        for glyph in &row.glyphs {
            while bold.peek().is_some_and(|range| range.end <= char_index) {
                bold.next();
            }
            if bold.peek().is_some_and(|range| range.contains(&char_index))
                && !glyph.uv_rect.is_nothing()
            {
                let glyph_pos = glyph.pos + glyph.uv_rect.offset;
                let origin = egui::pos2(
                    (glyph_pos.x * ui.pixels_per_point()).round() / ui.pixels_per_point(),
                    (glyph_pos.y * ui.pixels_per_point()).round() / ui.pixels_per_point(),
                );
                let rect = egui::Rect::from_min_size(origin, glyph.uv_rect.size);
                let uv = egui::Rect::from_min_max(
                    egui::pos2(glyph.uv_rect.min[0] as f32, glyph.uv_rect.min[1] as f32),
                    egui::pos2(glyph.uv_rect.max[0] as f32, glyph.uv_rect.max[1] as f32),
                );
                for offset in [Vec2::new(-0.35, 0.0), Vec2::new(0.35, 0.0)] {
                    mesh.add_rect_with_uv(rect.translate(offset), uv, color);
                }
            }
            char_index += 1;
        }
        char_index += usize::from(row.ends_with_newline);
        overlay.num_vertices += mesh.vertices.len();
        overlay.num_indices += mesh.indices.len();
        let mesh_bounds = mesh.calc_bounds();
        let glyph_vertex_range = 0..mesh.vertices.len();
        Arc::make_mut(&mut row.row).visuals = egui::epaint::text::RowVisuals {
            mesh,
            mesh_bounds,
            glyph_index_start: 0,
            glyph_vertex_range,
        };
    }
    // Keep UVs in texels until egui tessellates: later labels in the same
    // frame may grow the font atlas and invalidate normalized UVs.
    ui.painter().galley(pos, Arc::new(overlay), color);
    egui::text_selection::LabelSelectionState::label_text_selection(
        ui,
        &response,
        pos,
        galley,
        color,
        Stroke::NONE,
    );
}

/// `streaming` keeps the body plain: the Slint shell parses only after the
/// turn settles, so the transcript never re-wraps on every token.
pub fn show(ui: &mut Ui, text: &str, color: Color32, streaming: bool) {
    if text.trim().is_empty() {
        return;
    }
    if streaming {
        ui.add(
            Label::new(RichText::new(text).size(BODY_SIZE).color(color))
                .wrap()
                .selectable(true),
        );
        return;
    }
    let (source, truncated) = visible_source(text);
    let blocks = blocks_for(source);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        for (index, block) in blocks.iter().enumerate() {
            render_block(ui, block, color, index == 0);
        }
        if truncated {
            let tokens = crate::theme::tokens();
            render_quote(
                ui,
                &[Inline {
                    text: "仅展示前 16 KiB 或 160 行；点击气泡下方「复制」获取完整原文。"
                        .to_owned(),
                    style: InlineStyle::default(),
                }],
                tokens.text_secondary,
            );
        }
    });
}

fn render_block(ui: &mut Ui, block: &Block, color: Color32, first: bool) {
    let tokens = crate::theme::tokens();
    match block {
        Block::Text { kind, inlines } => match kind {
            TextKind::Body => {
                let (job, bold) = text_job(inlines, BODY_SIZE, color);
                show_text(ui, job, &bold, color);
                ui.add_space(if first { 0.0 } else { 12.0 });
            }
            TextKind::Quote => {
                if !first {
                    ui.add_space(12.0);
                }
                render_quote(ui, inlines, tokens.text_secondary);
                ui.add_space(12.0);
            }
            TextKind::Heading(level) => {
                if !first {
                    ui.add_space(if *level <= 2 { 16.0 } else { 12.0 });
                }
                let size = heading_size(*level);
                let (mut job, mut bold) = text_job(inlines, size, color);
                let characters = job.text.chars().count();
                if characters > 0 {
                    // A heading reads as one weight: overdraw the whole run.
                    bold.clear();
                    bold.push(0..characters);
                }
                // The browser's h2 is 1.5em = 21px. Keep the measured line box
                // stable while matching the glyph size; changing the CJK face
                // size must not move every following block.
                if *level == 2 {
                    let height = ui.fonts(|fonts| fonts.row_height(&FontId::proportional(22.0)));
                    for section in &mut job.sections {
                        section.format.line_height = Some(height);
                    }
                }
                show_text(ui, job, &bold, color);
                ui.add_space(10.0);
            }
        },
        Block::Code { language, text } => {
            if !first {
                ui.add_space(12.0);
            }
            render_code(ui, language, text, color);
            ui.add_space(12.0);
        }
        Block::Table { columns, rows } => {
            if !first {
                ui.add_space(12.0);
            }
            render_table(ui, *columns, rows, color);
            ui.add_space(12.0);
        }
        Block::Rule => {
            if !first {
                ui.add_space(4.0);
            }
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(line, 0.0, tokens.divider);
            ui.add_space(12.0);
        }
    }
}

/// Quotes are a soft panel in the Slint shell, not a bare left bar.
fn render_quote(ui: &mut Ui, inlines: &[Inline], color: Color32) {
    let tokens = crate::theme::tokens();
    Frame::NONE
        .fill(tokens.primary_tint)
        .corner_radius(8.0)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (job, bold) = text_job(inlines, BODY_SIZE, color);
            show_text(ui, job, &bold, color);
        });
}

/// One run of code: the 13px monospace face the block always used, plus an
/// explicit line rhythm so every token run shares the same leading.
fn code_format(color: Color32) -> TextFormat {
    TextFormat {
        font_id: FontId::monospace(13.0),
        color,
        line_height: Some(13.0 * 1.7),
        ..Default::default()
    }
}

fn syntax_color(kind: TokenKind, color: Color32) -> Color32 {
    let tokens = crate::theme::tokens();
    match kind {
        // Types share the keyword hue: they are the same "declaration" voice
        // in every supported language, and a sixth colour would be noise.
        TokenKind::Keyword | TokenKind::Type => tokens.syntax_keyword,
        TokenKind::String => tokens.syntax_string,
        TokenKind::Comment => tokens.syntax_comment,
        TokenKind::Number => tokens.syntax_number,
        TokenKind::Function => tokens.syntax_function,
        TokenKind::Plain => color,
    }
}

/// One layout job per block: every lexer span becomes one coloured run, so
/// the text stays a single selectable run and keeps its exact bytes. Unknown
/// languages return no spans and therefore one plain-coloured section.
fn code_job(text: &str, language: &str, color: Color32) -> LayoutJob {
    let spans = crate::code_highlight::spans(text, language);
    let mut job = LayoutJob::default();
    if spans.is_empty() {
        job.append(text, 0.0, code_format(color));
        return job;
    }
    for (range, kind) in spans {
        let Some(slice) = text.get(range) else {
            continue;
        };
        if slice.is_empty() {
            continue;
        }
        job.append(slice, 0.0, code_format(syntax_color(kind, color)));
    }
    job
}

fn render_code(ui: &mut Ui, language: &str, text: &str, color: Color32) {
    let tokens = crate::theme::tokens();
    Frame::NONE
        .fill(tokens.code_background)
        .corner_radius(8.0)
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if language.is_empty() {
                        "代码"
                    } else {
                        language
                    })
                    .size(11.0)
                    .color(tokens.text_muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // A copy action per block, like the Slint shell's; the
                    // confirmation lives in egui memory for 1.6s.
                    let id = egui::Id::new(("markdown-code-copy", hash_str(text)));
                    let now = ui.input(|input| input.time);
                    let copied = ui
                        .data(|data| data.get_temp::<f64>(id))
                        .is_some_and(|when| now - when < 1.6);
                    let label = if copied { "已复制" } else { "复制" };
                    let button = ui.add(
                        egui::Button::new(RichText::new(label).size(11.0).color(tokens.text_muted))
                            .frame(false),
                    );
                    if button.clicked() {
                        ui.ctx().copy_text(text.to_owned());
                        ui.data_mut(|data| data.insert_temp(id, now));
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(1600));
                    }
                });
            });
            ui.add_space(6.0);
            // Code must not re-wrap: a wrapped line silently changes the
            // program being shown. Scroll instead, and tell the label to
            // extend past the viewport rather than trimming at its edge.
            ScrollArea::horizontal()
                .id_salt(("markdown-code", hash_str(text)))
                .show(ui, |ui| {
                    ui.add(
                        Label::new(code_job(text, language, color))
                            .wrap_mode(egui::TextWrapMode::Extend)
                            .selectable(true),
                    );
                });
        });
}

fn render_table(ui: &mut Ui, columns: usize, rows: &[Vec<String>], color: Color32) {
    let tokens = crate::theme::tokens();
    if rows.is_empty() || columns == 0 {
        return;
    }
    let pad = 8.0;
    let width = (ui.available_width() / columns as f32).max(TABLE_MIN_COLUMN);
    let lay_out = |ui: &Ui, row: &Vec<String>, header: bool| {
        (0..columns)
            .map(|index| {
                let cell = row.get(index).cloned().unwrap_or_default();
                let mut job = LayoutJob::default();
                job.wrap.max_width = (width - 2.0 * pad).max(8.0);
                job.append(
                    &cell,
                    0.0,
                    text_format(
                        BODY_SIZE,
                        if header { tokens.heading } else { color },
                        false,
                    ),
                );
                ui.fonts(|fonts| fonts.layout_job(job))
            })
            .collect::<Vec<_>>()
    };
    let mut galleys: Vec<(Vec<Arc<egui::Galley>>, f32)> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let laid_out = lay_out(ui, row, index == 0);
        let height = laid_out
            .iter()
            .map(|galley| galley.size().y)
            .fold(0.0, f32::max);
        galleys.push((laid_out, height + 2.0 * pad));
    }
    let total_height: f32 = galleys.iter().map(|(_, height)| *height).sum();
    ScrollArea::both()
        .id_salt(("markdown-table", rows.len(), columns))
        .max_height(TABLE_MAX_HEIGHT)
        .show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(width * columns as f32, total_height),
                Sense::hover(),
            );
            let painter = ui.painter();
            let mut top = rect.top();
            for (row_index, (cells, height)) in galleys.iter().enumerate() {
                for (column, galley) in cells.iter().enumerate() {
                    let cell = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + width * column as f32, top),
                        Vec2::new(width, *height),
                    );
                    painter.rect_filled(
                        cell,
                        0.0,
                        if row_index == 0 {
                            tokens.primary_tint
                        } else {
                            tokens.surface
                        },
                    );
                    painter.rect_stroke(
                        cell,
                        0.0,
                        Stroke::new(1.0, tokens.border),
                        egui::StrokeKind::Inside,
                    );
                    painter.galley(
                        cell.min + Vec2::splat(pad),
                        galley.clone(),
                        if row_index == 0 {
                            tokens.heading
                        } else {
                            color
                        },
                    );
                }
                top += height;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_text(blocks: &[Block]) -> String {
        blocks
            .iter()
            .map(|block| match block {
                Block::Text { inlines, .. } => inlines
                    .iter()
                    .map(|inline| inline.text.as_str())
                    .collect::<String>(),
                Block::Code { text, .. } => format!("[code]{text}"),
                Block::Table { rows, .. } => format!(
                    "[table]{}",
                    rows.iter()
                        .map(|row| row.join("|"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                Block::Rule => "[rule]".to_owned(),
            })
            .collect::<Vec<_>>()
            .join("¦")
    }

    fn body_text(blocks: &[Block]) -> String {
        blocks
            .iter()
            .filter_map(|block| match block {
                Block::Text {
                    kind: TextKind::Body,
                    inlines,
                } => Some(
                    inlines
                        .iter()
                        .map(|inline| inline.text.as_str())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn paragraphs_and_lists_share_one_selectable_run() {
        let blocks = parse("第一段\n\n- 甲\n- 乙\n\n第二段");
        let text_blocks = blocks
            .iter()
            .filter(|block| matches!(block, Block::Text { .. }))
            .count();
        assert_eq!(text_blocks, 1, "正文应合并成一个可选中的块");
        let text = body_text(&blocks);
        assert!(
            text.contains("第一段\n\n• 甲\n• 乙"),
            "列表标记应内联：{text:?}"
        );
        assert!(
            text.contains("乙\n\n第二段"),
            "列表后仍要有段落间距：{text:?}"
        );
    }

    #[test]
    fn soft_breaks_reflow_without_joining_cjk_sentences() {
        let blocks = parse("这是被硬折行的\n中文句子\n\nAnd a wrapped\nenglish line");
        let text = body_text(&blocks);
        assert!(
            text.contains("这是被硬折行的中文句子"),
            "中文软换行不应插入空格：{text:?}"
        );
        assert!(
            text.contains("And a wrapped english line"),
            "英文软换行必须补回空格：{text:?}"
        );
    }

    #[test]
    fn nested_lists_keep_depth_and_order() {
        let blocks = parse("1. 一\n   - 子项\n2. 二");
        let text = body_text(&blocks);
        assert!(text.contains("1. 一"), "{text:?}");
        assert!(text.contains("  • 子项"), "子项缩进两个空格：{text:?}");
        assert!(text.contains("2. 二"), "有序编号应递增：{text:?}");
    }

    #[test]
    fn tables_become_real_grids_instead_of_raw_pipes() {
        let blocks = parse("| 项 | 值 |\n| --- | --- |\n| 甲 | 1 |\n| 乙 | 2 |");
        assert!(
            matches!(blocks[0], Block::Table { .. }),
            "表格应成为独立块：{}",
            run_text(&blocks)
        );
        let Block::Table { columns, rows } = &blocks[0] else {
            return;
        };
        assert_eq!(*columns, 2);
        assert_eq!(rows.len(), 3, "表头计入行数");
        assert_eq!(rows[0], vec!["项".to_owned(), "值".to_owned()]);
        assert_eq!(rows[2], vec!["乙".to_owned(), "2".to_owned()]);
    }

    #[test]
    fn code_quotes_and_headings_become_their_own_blocks() {
        let blocks = parse("## 标题\n\n> 引用一行\n> 引用两行\n\n```rust\nfn main() {}\n```");
        assert!(matches!(
            blocks[0],
            Block::Text {
                kind: TextKind::Heading(2),
                ..
            }
        ));
        assert!(matches!(
            blocks[1],
            Block::Text {
                kind: TextKind::Quote,
                ..
            }
        ));
        assert!(
            matches!(blocks[2], Block::Code { .. }),
            "围栏代码应成为代码块：{}",
            run_text(&blocks)
        );
        let Block::Code { language, text } = &blocks[2] else {
            return;
        };
        assert_eq!(language, "rust");
        assert_eq!(text, "fn main() {}");
    }

    #[test]
    fn inline_styles_and_links_survive_parsing() {
        let blocks = parse("**粗** *斜* ~~删~~ `码` [文](https://example.com)");
        assert!(matches!(blocks[0], Block::Text { .. }), "应解析为文本块");
        let Block::Text { inlines, .. } = &blocks[0] else {
            return;
        };
        let styled = |needle: &str, check: fn(&InlineStyle) -> bool| {
            inlines
                .iter()
                .any(|inline| inline.text.contains(needle) && check(&inline.style))
        };
        assert!(styled("粗", |style| style.strong));
        assert!(styled("斜", |style| style.emphasis));
        assert!(styled("删", |style| style.strike));
        assert!(styled("码", |style| style.code));
        let text = body_text(&blocks);
        assert!(
            text.contains("文 (https://example.com)"),
            "链接保留标签并补上目标：{text:?}"
        );
    }

    #[test]
    fn long_messages_are_capped_with_a_notice() {
        let long = "行\n".repeat(DISPLAY_LINES + 20);
        let (source, truncated) = visible_source(&long);
        assert!(truncated);
        assert!(source.lines().count() <= DISPLAY_LINES);
        let (short, capped) = visible_source("短消息");
        assert!(!capped);
        assert_eq!(short, "短消息");
    }

    #[test]
    fn cjk_emphasis_wraps_into_valid_meshes() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let source = "## 标题\n\n1. **连续中文强调文本与换行**：保留正文。\n2. `sample` 与 **第二段强调**。\n\n\
| 列 | 值 |\n| --- | --- |\n| 甲 | **乙** |\n\n> 引用 **强调**\n\n```rust\nfn a() {}\n```";
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(170.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        super::show(ui, source, egui::Color32::BLACK, false);
                    });
                },
            );
            let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
            assert!(!primitives.is_empty());
            for primitive in primitives {
                if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                    assert!(mesh.is_valid());
                    assert!(mesh.vertices.iter().all(|vertex| {
                        vertex.pos.x.is_finite()
                            && vertex.pos.y.is_finite()
                            && vertex.uv.x.is_finite()
                            && vertex.uv.y.is_finite()
                    }));
                }
            }
        }
    }

    #[test]
    fn code_job_colours_rust_tokens_without_changing_the_text() {
        let source = "fn main() {\n    let n = 42; // 注释\n    call(\"hi\");\n}";
        let job = code_job(source, "rust", egui::Color32::BLACK);
        assert_eq!(job.text, source, "分词不能改写代码字节");
        let tokens = crate::theme::tokens();
        let colours: Vec<Color32> = job
            .sections
            .iter()
            .map(|section| section.format.color)
            .collect();
        assert!(colours.contains(&tokens.syntax_keyword), "关键字应取语法色");
        assert!(colours.contains(&tokens.syntax_string), "字符串应取语法色");
        assert!(colours.contains(&tokens.syntax_number), "数字应取语法色");
        assert!(colours.contains(&tokens.syntax_comment), "注释应取语法色");
        assert!(
            colours.contains(&tokens.syntax_function),
            "`call(` 应识别为函数"
        );
        assert!(
            colours.contains(&egui::Color32::BLACK),
            "标点与空白应保持正文色"
        );
    }

    #[test]
    fn unknown_language_falls_back_to_one_plain_colour() {
        let job = code_job("fn main() {}", "brainfuck", egui::Color32::BLACK);
        assert_eq!(job.sections.len(), 1, "未知语言必须回退成单色");
        assert_eq!(
            job.sections.first().map(|section| section.format.color),
            Some(egui::Color32::BLACK)
        );
        assert_eq!(job.text, "fn main() {}");
    }

    #[test]
    fn highlighted_code_blocks_tessellate_into_valid_meshes() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let source = "```rust\nfn main() {\n    let count = 42; // 注释\n    \
println!(\"中文 {}\", count);\n}\n```\n\n```unknown\n纯文本块\n```";
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(170.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        super::show(ui, source, egui::Color32::BLACK, false);
                    });
                },
            );
            let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
            assert!(!primitives.is_empty());
            for primitive in primitives {
                if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                    assert!(mesh.is_valid());
                    assert!(mesh.vertices.iter().all(|vertex| {
                        vertex.pos.x.is_finite()
                            && vertex.pos.y.is_finite()
                            && vertex.uv.x.is_finite()
                            && vertex.uv.y.is_finite()
                    }));
                }
            }
        }
    }
}
