//! Bounded native Markdown projection.
//!
//! Streaming output remains plain text and is published as a frozen prefix of
//! completed lines plus one active tail block, so a growing answer costs
//! O(tail) per frame instead of re-shaping the whole message. Once a model
//! response is terminal we parse its visible window once with pulldown-cmark.
//! Text blocks carry a marker-free plain projection so the read-only text
//! controls in the bubble stay selectable and copyable with the mouse; code
//! blocks keep Slint's StyledText rendering for syntax colours.
use crate::TextBlock;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use slint::{Model, ModelRc, StyledText, VecModel};
use std::rc::Rc;

const DISPLAY_BYTES: usize = 16 * 1024;
const DISPLAY_LINES: usize = 160;
/// Completed lines are frozen into their own block once the live tail passes
/// this budget; only the tail is re-shaped on the following frame.
const TAIL_BYTES: usize = 2 * 1024;
/// Upper bound for one frozen block.
const FROZEN_CHUNK_BYTES: usize = 4 * 1024;
/// The display window is re-based in whole steps: past the budget its start
/// moves a few bytes per frame, and rebuilding the prefix on every one of
/// those would cost more than the tail it is meant to bound.
const REBASE_SLACK: usize = 4 * 1024;
/// Shown at the head once the display window drops older lines; streaming and
/// the settled view use the same words so the settle step never reshapes it.
const TRUNCATED_NOTICE: &str = "仅展示最近 16 KiB 或 160 行；点击气泡下方“复制”获取完整原文。";
pub const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
const KIND_TEXT: i32 = 0;
const KIND_CODE: i32 = 1;
const KIND_QUOTE: i32 = 4;
const KIND_TABLE: i32 = 5;
pub(crate) const KIND_IMAGE: i32 = 6;
const KIND_H1: i32 = 10;

pub struct Blocks {
    pub raw: String,
    pub model: Rc<VecModel<TextBlock>>,
    rendered: bool,
    /// Byte offset in `raw` where the published window starts. It only moves
    /// forward when the reply outgrows the display budget.
    window: usize,
    /// Byte offset in `raw` before which text is frozen into completed blocks.
    frozen: usize,
    /// Index of the live tail block, which is always the model's last row.
    tail: Option<usize>,
}

impl Blocks {
    pub fn new() -> Self {
        Self {
            raw: String::new(),
            model: Rc::new(VecModel::default()),
            rendered: false,
            window: 0,
            frozen: 0,
            tail: None,
        }
    }

    pub fn append(&mut self, delta: &str) -> Result<(), String> {
        if self.raw.len() + delta.len() > MAX_TEXT_BYTES {
            return Err("回复超过显示缓冲限制，请在历史中查看完整结果".into());
        }
        self.raw.push_str(delta);
        self.rendered = false;
        Ok(())
    }

    pub fn replace(&mut self, text: &str) -> Result<(), String> {
        if self.raw == text {
            return Ok(());
        }
        if text.starts_with(&self.raw) {
            return self.append(&text[self.raw.len()..]);
        }
        if text.len() > MAX_TEXT_BYTES {
            return Err("回复超过显示缓冲限制".into());
        }
        self.raw = text.into();
        self.rendered = false;
        self.reset_stream_view();
        Ok(())
    }

    /// Update the streaming view. No Markdown parsing takes place here.
    ///
    /// Completed lines are frozen once and never touched again; only the live
    /// tail block is replaced, which keeps the per-frame text layout bounded
    /// by the tail instead of the whole message.
    pub fn flush(&mut self) {
        if self.rendered {
            return;
        }
        self.sync_window();
        self.freeze_completed();
        self.publish_tail();
    }

    /// Parse exactly once after the model output reaches a terminal state.
    pub fn finalize_markdown(&mut self) {
        if self.rendered {
            return;
        }
        let (source, head_capped) = visible_source(&self.raw);
        let mut blocks = markdown_blocks(source);
        if head_capped {
            blocks.insert(0, text_block(TRUNCATED_NOTICE, KIND_H1));
        }
        self.model.set_vec(blocks);
        self.tail = None;
        self.frozen = self.raw.len();
        self.rendered = true;
    }

    fn reset_stream_view(&mut self) {
        self.window = 0;
        self.frozen = 0;
        self.tail = None;
        self.model.set_vec(Vec::new());
    }

    /// Rebase the published window when the reply outgrows the display budget.
    /// The head is dropped rather than the tail: a long answer must keep
    /// showing what the model is writing right now.
    fn sync_window(&mut self) {
        let start = window_start(&self.raw);
        if start == 0 {
            return;
        }
        // Re-base only in whole steps; a per-frame slide would rebuild the
        // frozen prefix every stream tick once the reply crosses the budget.
        if self.window != 0 && start <= self.window.saturating_add(REBASE_SLACK) {
            return;
        }
        self.window = start;
        self.frozen = start;
        self.tail = None;
        self.model.set_vec(Vec::new());
        if start > 0 {
            self.model.push(plain_text_block(TRUNCATED_NOTICE));
        }
    }

    fn freeze_completed(&mut self) {
        let limit = self.raw.len().saturating_sub(TAIL_BYTES);
        if limit <= self.frozen {
            return;
        }
        let Some(cut) = line_boundary_before(&self.raw, limit) else {
            return;
        };
        if cut <= self.frozen {
            return;
        }
        let chunk = self.raw[self.frozen..cut].to_owned();
        // The tail block, when present, is always the last row: completed
        // blocks are inserted in front of it and it keeps its identity.
        let at = self.tail.unwrap_or_else(|| self.model.row_count());
        let mut inserted = 0usize;
        for part in split_chunks(&chunk) {
            // The block boundary stands in for the line terminator: a block
            // starts on its own line, so keeping the newline would double-space.
            let text = part.strip_suffix('\n').unwrap_or(part);
            if text.is_empty() {
                continue;
            }
            self.model.insert(at + inserted, plain_text_block(text));
            inserted += 1;
        }
        if inserted > 0 {
            self.tail = Some(at + inserted);
        }
        self.frozen = cut;
    }

    fn publish_tail(&mut self) {
        let text = &self.raw[self.frozen..];
        match self.tail {
            Some(index) => self.model.set_row_data(index, plain_text_block(text)),
            None if !text.is_empty() => {
                let index = self.model.row_count();
                self.model.push(plain_text_block(text));
                self.tail = Some(index);
            }
            None => {}
        }
    }
}

pub fn from_text(text: &str) -> ModelRc<TextBlock> {
    let mut blocks = Blocks::new();
    blocks.raw = text.to_owned();
    blocks.finalize_markdown();
    ModelRc::from(blocks.model)
}

/// Classify supported image references without reading the filesystem.
pub fn workspace_image_source(source: &str) -> Option<&str> {
    let path = source.split(['?', '#']).next()?.trim();
    // Final authorization (including absolute/file/public workspace paths)
    // belongs to NativeDesktop; never open these paths in the UI.
    if path.is_empty()
        || path.starts_with("//")
        || path.contains("://") && !path.starts_with("file://")
    {
        return None;
    }
    let extension = path.rsplit('.').next()?.to_ascii_lowercase();
    matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp").then_some(path)
}

fn workspace_document_source(source: &str) -> bool {
    let path = source.split(['?', '#']).next().unwrap_or_default();
    if path.starts_with('#')
        || path.starts_with("//")
        || path.contains("://") && !path.starts_with("file://")
    {
        return false;
    }
    let extension = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "pdf"
            | "doc"
            | "docx"
            | "xls"
            | "xlsx"
            | "csv"
            | "ppt"
            | "pptx"
            | "odt"
            | "ods"
            | "txt"
            | "md"
            | "markdown"
            | "rtf"
            | "json"
            | "yaml"
            | "yml"
            | "xml"
            | "drawio"
    )
}

/// The published window keeps the newest lines, so a long answer never freezes
/// mid-stream: the head leaves the display budget and the live tail stays
/// visible. Copy and the durable history always carry the complete text.
fn visible_source(text: &str) -> (&str, bool) {
    let start = window_start(text);
    (&text[start..], start > 0)
}

fn window_start(text: &str) -> usize {
    let mut start = char_boundary_down(text, text.len().saturating_sub(DISPLAY_BYTES));
    // Open the window on a line boundary so it never begins mid-line; when the
    // byte cap already landed right after one, keep that whole line. A single
    // very long line has no usable boundary, so it keeps its newest bytes.
    if start > 0 && text.as_bytes().get(start - 1) != Some(&b'\n') {
        if let Some(offset) = text[start..].find('\n') {
            let line_start = start + offset + 1;
            if line_start < text.len() {
                start = line_start;
            }
        }
    }
    // Bound the visible line count as well; the scan stays inside the byte
    // window because `start` already moved past everything older.
    let mut newlines = text[start..].bytes().filter(|byte| *byte == b'\n').count();
    while newlines > DISPLAY_LINES {
        let Some(offset) = text[start..].find('\n') else {
            break;
        };
        start += offset + 1;
        newlines -= 1;
    }
    start
}

fn styled(text: &str) -> StyledText {
    StyledText::from_markdown(text).unwrap_or_else(|_| StyledText::from_plain_text(text))
}

fn text_block(text: &str, kind: i32) -> TextBlock {
    TextBlock {
        text: text.into(),
        styled: styled(text),
        kind,
        table_cells: string_model(Vec::new()),
        table_columns: 0,
        table_rows: string_model(Vec::new()),
        image: slint::Image::default(),
        image_source: "".into(),
        resource_name: "".into(),
        language: "".into(),
    }
}

fn table_block(rows: &[String], columns: usize) -> TextBlock {
    let text = rows.join("\n");
    TextBlock {
        text: text.clone().into(),
        styled: StyledText::from_plain_text(&text),
        kind: KIND_TABLE,
        table_cells: string_model(
            rows.iter()
                .flat_map(|row| row.split("\u{1f}").map(str::to_owned))
                .collect(),
        ),
        table_columns: columns as i32,
        table_rows: string_model(
            rows.iter()
                .map(|row| row.replace('\u{1f}', "  |  "))
                .collect(),
        ),
        image: slint::Image::default(),
        image_source: "".into(),
        resource_name: "".into(),
        language: "".into(),
    }
}

fn image_block(alt: &str, source: &str) -> TextBlock {
    TextBlock {
        text: if alt.trim().is_empty() {
            source
                .split(['?', '#'])
                .next()
                .and_then(|value| value.rsplit(['/', '\\']).next())
                .filter(|value| !value.is_empty())
                .unwrap_or("图片")
                .into()
        } else {
            alt.into()
        },
        styled: StyledText::from_plain_text(alt),
        kind: KIND_IMAGE,
        table_cells: string_model(Vec::new()),
        table_columns: 0,
        table_rows: string_model(Vec::new()),
        image: slint::Image::default(),
        image_source: source.into(),
        resource_name: "".into(),
        language: "".into(),
    }
}

fn document_block(label: &str, source: &str) -> TextBlock {
    let fallback = source
        .split(['?', '#'])
        .next()
        .and_then(|value| value.rsplit(['/', '\\']).next())
        .filter(|value| !value.is_empty())
        .unwrap_or("文件");
    let title = if label.trim().is_empty() {
        fallback
    } else {
        label
    };
    TextBlock {
        text: title.into(),
        styled: StyledText::from_plain_text(title),
        kind: 7,
        table_cells: string_model(Vec::new()),
        table_columns: 0,
        table_rows: string_model(Vec::new()),
        image: crate::file_icons::workspace_file_icon(fallback, "file"),
        image_source: source.into(),
        resource_name: fallback.into(),
        language: "".into(),
    }
}

fn plain_text_block(text: &str) -> TextBlock {
    TextBlock {
        text: text.into(),
        styled: StyledText::from_plain_text(text),
        kind: KIND_TEXT,
        table_cells: string_model(Vec::new()),
        table_columns: 0,
        table_rows: string_model(Vec::new()),
        image: slint::Image::default(),
        image_source: "".into(),
        resource_name: "".into(),
        language: "".into(),
    }
}

fn string_model(values: Vec<String>) -> slint::ModelRc<slint::SharedString> {
    let values = values
        .into_iter()
        .map(Into::into)
        .collect::<Vec<slint::SharedString>>();
    slint::ModelRc::from(Rc::new(VecModel::from(values)))
}

/// Split a completed run of lines into bounded frozen blocks, preferring blank
/// line boundaries and falling back to any line break.
fn split_chunks(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        if text.len() - start <= FROZEN_CHUNK_BYTES {
            parts.push(&text[start..]);
            break;
        }
        let limit = start + FROZEN_CHUNK_BYTES;
        // Never cut mid-line: a block boundary already stands for one dropped
        // line terminator, and a mid-line cut would invent a visible break.
        let cut = blank_line_before(text, limit)
            .or_else(|| line_boundary_before(text, limit))
            .filter(|cut| *cut > start)
            .unwrap_or(text.len());
        parts.push(&text[start..cut]);
        start = cut;
    }
    parts
}

fn blank_line_before(text: &str, limit: usize) -> Option<usize> {
    let end = char_boundary_down(text, limit);
    text[..end].rfind("\n\n").map(|offset| offset + 2)
}

/// Index just after the last line terminator at or before `limit`.
fn line_boundary_before(text: &str, limit: usize) -> Option<usize> {
    let end = char_boundary_down(text, limit);
    text[..end].rfind('\n').map(|offset| offset + 1)
}

fn char_boundary_down(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn flush_block(blocks: &mut Vec<TextBlock>, text: &mut String, kind: &mut i32) {
    let value = if *kind == KIND_CODE {
        text.as_str()
    } else {
        text.trim()
    };
    if !value.is_empty() {
        blocks.push(text_block(value, *kind));
    }
    text.clear();
    *kind = KIND_TEXT;
}

fn markdown_blocks(source: &str) -> Vec<TextBlock> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let mut blocks = Vec::new();
    let mut text = String::new();
    let mut kind = KIND_TEXT;
    let mut code = false;
    let mut language = String::new();
    let mut document: Option<(String, String)> = None;
    let mut quote = false;
    let mut list_depth = 0usize;
    let mut ordered: Vec<Option<u64>> = Vec::new();
    let mut links: Vec<String> = Vec::new();
    let mut link_starts: Vec<usize> = Vec::new();
    let mut table = false;
    let mut table_cells: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut table_rows: Vec<String> = Vec::new();
    let mut image_target: Option<String> = None;
    let mut image_alt = String::new();

    for event in Parser::new_ext(source, options) {
        if let Some((target, label)) = document.as_mut() {
            match &event {
                Event::End(TagEnd::Link) => {
                    blocks.push(document_block(label, target));
                    document = None;
                }
                Event::Text(value) | Event::Code(value) => label.push_str(value),
                Event::SoftBreak | Event::HardBreak => label.push(' '),
                _ => {}
            }
            continue;
        }
        if table {
            match &event {
                Event::Code(value) => {
                    cell.push_str(value);
                    continue;
                }
                Event::Start(
                    Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. },
                )
                | Event::End(
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link,
                ) => continue,
                Event::Start(Tag::Image { dest_url, .. }) => {
                    cell.push_str(dest_url);
                    continue;
                }
                Event::End(TagEnd::Image) => continue,
                _ => {}
            }
        }
        match event {
            Event::End(TagEnd::Paragraph) => {
                if list_depth == 0 && !table && !text.ends_with('\n') {
                    text.push('\n');
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush_block(&mut blocks, &mut text, &mut kind);
                kind = KIND_H1 + level as i32 - 1;
            }
            Event::End(TagEnd::Heading(_)) => flush_block(&mut blocks, &mut text, &mut kind),
            Event::Start(Tag::CodeBlock(kind_info)) => {
                flush_block(&mut blocks, &mut text, &mut kind);
                code = true;
                kind = KIND_CODE;
                language = match kind_info {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                code = false;
                let mut block = plain_text_block(&text);
                block.kind = KIND_CODE;
                block.styled = crate::code_highlight::highlight(&text, &language);
                block.language = language.as_str().into();
                blocks.push(block);
                text.clear();
                kind = KIND_TEXT;
            }
            Event::Start(Tag::BlockQuote) => {
                flush_block(&mut blocks, &mut text, &mut kind);
                quote = true;
                kind = KIND_QUOTE;
            }
            Event::End(TagEnd::BlockQuote) => {
                quote = false;
                flush_block(&mut blocks, &mut text, &mut kind);
            }
            Event::Start(Tag::List(start)) => {
                list_depth += 1;
                ordered.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                ordered.pop();
                // The list stays in the surrounding text block: splitting at
                // the list boundary made drag selection stop there, and item
                // lines already carry their own newlines.
            }
            Event::Start(Tag::Item) => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&"  ".repeat(list_depth.saturating_sub(1)));
                if let Some(Some(index)) = ordered.last_mut() {
                    text.push_str(&format!("{index}. "));
                    *index += 1;
                } else {
                    text.push_str("• ");
                }
            }
            Event::End(TagEnd::Item) => text.push('\n'),
            Event::Start(Tag::Emphasis) => {}
            Event::End(TagEnd::Emphasis) => {}
            Event::Start(Tag::Strong) => {}
            Event::End(TagEnd::Strong) => {}
            Event::Start(Tag::Strikethrough) => {}
            Event::End(TagEnd::Strikethrough) => {}
            Event::Start(Tag::Link { dest_url, .. }) => {
                if workspace_document_source(&dest_url) {
                    flush_block(&mut blocks, &mut text, &mut kind);
                    document = Some((dest_url.to_string(), String::new()));
                    continue;
                }
                link_starts.push(text.len());
                links.push(dest_url.to_string());
            }
            Event::End(TagEnd::Link) => {
                let url = links.pop().unwrap_or_default();
                let start = link_starts.pop().unwrap_or(text.len());
                // Render remote links as readable text: keep the label and
                // append the target once, skipping labels that already are the
                // target. The full markdown stays available via message copy.
                let labeled = text.len() > start && &text[start..] != url;
                if labeled {
                    text.push_str(" (");
                }
                text.push_str(&url);
                if labeled {
                    text.push(')');
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                image_target = Some(dest_url.to_string());
                image_alt.clear();
            }
            Event::End(TagEnd::Image) => {
                if let Some(source) = image_target.take() {
                    if workspace_image_source(&source).is_some() {
                        flush_block(&mut blocks, &mut text, &mut kind);
                        blocks.push(image_block(&image_alt, &source));
                    } else if workspace_document_source(&source) {
                        flush_block(&mut blocks, &mut text, &mut kind);
                        blocks.push(document_block(&image_alt, &source));
                    } else {
                        if image_alt.is_empty() {
                            text.push_str("图片 ");
                        } else {
                            text.push_str(&image_alt);
                            text.push(' ');
                        }
                        text.push('（');
                        text.push_str(&source);
                        text.push('）');
                    }
                }
            }
            Event::Code(value) => {
                text.push_str(&value);
            }
            Event::Text(value) | Event::Html(value) | Event::InlineHtml(value) => {
                if image_target.is_some() {
                    image_alt.push_str(&value);
                } else if table {
                    cell.push_str(&value);
                } else {
                    text.push_str(&value);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if table {
                    cell.push(' ');
                } else {
                    text.push('\n');
                }
            }
            Event::Rule => {
                flush_block(&mut blocks, &mut text, &mut kind);
                blocks.push(text_block("────────────────", KIND_QUOTE));
            }
            Event::Start(Tag::Table(_)) => {
                flush_block(&mut blocks, &mut text, &mut kind);
                table = true;
                kind = KIND_TABLE;
            }
            Event::End(TagEnd::Table) => {
                if !table_cells.is_empty() {
                    table_rows.push(table_cells.join("\u{1f}"));
                }
                blocks.push(table_block(
                    &table_rows,
                    table_rows
                        .first()
                        .map_or(0, |row| row.split('\u{1f}').count()),
                ));
                table_cells.clear();
                table_rows.clear();
                text.clear();
                table = false;
                kind = KIND_TEXT;
            }
            Event::Start(Tag::TableRow) => {}
            Event::End(TagEnd::TableRow) | Event::End(TagEnd::TableHead) => {
                text.push_str(&table_cells.join("  |  "));
                text.push('\n');
                table_rows.push(table_cells.join("\u{1f}"));
                table_cells.clear();
            }
            Event::Start(Tag::TableCell) => cell.clear(),
            Event::End(TagEnd::TableCell) => {
                table_cells.push(std::mem::take(&mut cell));
            }
            Event::TaskListMarker(checked) => text.push_str(if checked { "☑ " } else { "☐ " }),
            Event::FootnoteReference(label) => {
                text.push('[');
                text.push_str(&label);
                text.push(']');
            }
            _ => {}
        }
    }
    flush_block(&mut blocks, &mut text, &mut kind);
    let _ = (code, quote);
    blocks
}

/// Match the web's tail-only single-line thinking preview.
pub fn reasoning_preview(text: &str) -> slint::SharedString {
    let tail: Vec<char> = text.chars().rev().take(640).collect();
    let shortened = text.len() > tail.iter().map(|c| c.len_utf8()).sum::<usize>();
    let mut value = if shortened {
        "…".to_string()
    } else {
        String::new()
    };
    value.extend(
        tail.into_iter()
            .rev()
            .map(|c| if c.is_whitespace() { ' ' } else { c }),
    );
    value.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    fn block_texts(blocks: &Blocks) -> Vec<String> {
        blocks
            .model
            .iter()
            .map(|block| block.text.to_string())
            .collect()
    }

    #[test]
    fn streaming_freezes_completed_lines_and_reconstructs_the_reply() {
        let mut blocks = Blocks::new();
        let mut source = String::new();
        let mut frozen: Vec<String> = Vec::new();
        for index in 0..40 {
            let line = format!("第 {index} 行，用于撑满尾块预算的示例文本{}\n", "·".repeat(60));
            source.push_str(&line);
            blocks.append(&line).unwrap();
            blocks.flush();
            let texts = block_texts(&blocks);
            // A dropped line terminator is represented by the block boundary,
            // so the published blocks always reconstruct the raw text.
            assert_eq!(texts.join("\n"), source);
            let prefix = &texts[..texts.len().saturating_sub(1)];
            assert!(
                prefix.starts_with(frozen.as_slice()),
                "a frozen block changed after it was published: {:?} -> {:?}",
                frozen,
                prefix
            );
            frozen = prefix.to_vec();
        }
        assert!(blocks.model.row_count() > 1, "completed lines stay frozen");
    }

    #[test]
    fn a_long_reply_keeps_its_newest_lines_visible() {
        let mut blocks = Blocks::new();
        let mut source = String::new();
        for index in 0..300 {
            source.push_str(&format!("第 {index} 行\n"));
        }
        blocks.append(&source).unwrap();
        blocks.flush();
        let texts = block_texts(&blocks);
        assert_eq!(texts.first().map(String::as_str), Some(TRUNCATED_NOTICE));
        let window = texts[1..].join("\n");
        assert!(!window.contains("第 0 行"), "the head leaves the display budget");
        assert!(window.contains("第 299 行"), "the newest line stays visible");
        assert!(window.matches('\n').count() <= DISPLAY_LINES);
        assert_eq!(blocks.raw, source, "copy keeps the complete text");
    }

    #[test]
    fn the_display_window_rebases_in_coarse_steps() {
        let mut blocks = Blocks::new();
        let mut source = String::new();
        while source.len() <= DISPLAY_BYTES {
            let line = format!("{}\n", "x".repeat(199));
            source.push_str(&line);
            blocks.append(&line).unwrap();
            blocks.flush();
        }
        assert!(blocks.window > 0, "the byte budget must have re-based once");
        let window = blocks.window;
        blocks.append("y\n").unwrap();
        blocks.flush();
        assert_eq!(
            blocks.window, window,
            "a tiny append must not tear down the published window"
        );
    }

    #[test]
    fn a_single_long_line_still_publishes_its_newest_bytes() {
        let mut blocks = Blocks::new();
        let source = "·".repeat(DISPLAY_BYTES / 2 + 100) + "\n";
        blocks.append(&source).unwrap();
        blocks.flush();
        let texts = block_texts(&blocks);
        assert_eq!(texts.first().map(String::as_str), Some(TRUNCATED_NOTICE));
        assert!(
            texts.iter().skip(1).any(|text| !text.is_empty()),
            "a long single line must not publish an empty window"
        );
        assert_eq!(blocks.raw, source, "copy keeps the complete text");
    }

    #[test]
    fn replacing_the_live_body_drops_the_frozen_prefix() {
        let mut blocks = Blocks::new();
        blocks.append("旧的第一行\n旧的第二行\n").unwrap();
        blocks.flush();
        blocks.replace("完全替换后的正文").unwrap();
        blocks.flush();
        assert_eq!(block_texts(&blocks), vec!["完全替换后的正文".to_string()]);
    }

    #[test]
    fn finalize_after_streaming_parses_the_window_once() {
        let source = "# 标题\n\n**粗体** 段落\n\n```rust\nlet value = 1;\n```\n";
        let mut blocks = Blocks::new();
        for line in source.split_inclusive('\n') {
            blocks.append(line).unwrap();
            blocks.flush();
        }
        blocks.finalize_markdown();
        assert!(blocks.model.iter().any(|block| block.kind == KIND_H1));
        assert!(blocks.model.iter().any(|block| block.kind == KIND_CODE));
        assert_eq!(blocks.raw, source);
    }

    #[test]
    fn final_render_supports_common_elements_and_stream_is_plain() {
        let source = "# 标题\n\n段落含 **粗体**、*斜体*、`代码` 和 [链接](https://example.invalid)。\n\n> 引用\n\n- 项目一\n1. 项目二\n\n```rust\nlet value = 1;\n```\n\n| 名称 | 数值 |\n| --- | ---: |\n| A | 1 |\n\n---\n\n![说明](image.png)";
        let mut blocks = Blocks::new();
        for ch in source.chars() {
            blocks.append(&ch.to_string()).unwrap();
            blocks.flush();
        }
        assert!(blocks.model.iter().all(|block| block.kind == KIND_TEXT));
        blocks.finalize_markdown();
        assert!(blocks.model.iter().any(|block| block.kind == KIND_H1));
        assert!(blocks.model.iter().any(|block| block.kind == KIND_CODE));
        assert!(blocks.model.iter().any(|block| block.kind == KIND_TABLE));
        assert!(blocks.raw == source);
    }

    #[test]
    fn code_blocks_copy_only_code_without_the_fence_language() {
        let blocks = markdown_blocks("```rust\nlet value = 1;\n```");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, KIND_CODE);
        assert_eq!(blocks[0].text, "let value = 1;\n");
    }

    #[test]
    fn paragraph_list_paragraph_stays_one_selectable_text_block() {
        // One block per contiguous prose run keeps drag selection continuous;
        // splitting at the list boundary created an uncrossable gap.
        let blocks = markdown_blocks("段落一\n\n- 甲\n- 乙\n\n段落二\n");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, KIND_TEXT);
        assert_eq!(blocks[0].text, "段落一\n\n• 甲\n\n• 乙\n段落二");
        // A list inside a quote no longer splits the quote card either.
        let quote = markdown_blocks("> 前文\n>\n> - 甲\n> - 乙\n");
        assert_eq!(quote.len(), 1);
        assert_eq!(quote[0].kind, KIND_QUOTE);
        assert!(quote[0].text.contains("• 乙"), "{}", quote[0].text);
    }

    #[test]
    fn table_header_empty_cells_and_inline_code_keep_their_columns() {
        let blocks = markdown_blocks("| Key | Value |\n| --- | --- |\n| `item` | **value** |\n| empty | |\n\n| Next |\n| --- |\n| row |");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].table_columns, 2);
        let cells: Vec<_> = blocks[0]
            .table_cells
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(cells, ["Key", "Value", "item", "value", "empty", ""]);
        assert_eq!(blocks[1].table_columns, 1);
    }

    #[test]
    fn code_copy_preserves_indentation_blank_lines_and_final_newline() {
        let blocks = markdown_blocks("```text\n\n    indented\n\n```\n");
        assert_eq!(blocks[0].text, "\n    indented\n\n");
    }

    #[test]
    fn local_document_links_become_cards_and_remote_links_stay_text() {
        let blocks = markdown_blocks("之前 [**示例文档**](documents/example.pdf) 之后\n\n[远程](https://example.invalid/example.pdf)");
        assert_eq!(blocks[0].text, "之前");
        assert_eq!(blocks[1].kind, 7);
        assert_eq!(blocks[1].text, "示例文档");
        assert_eq!(blocks[1].image_source, "documents/example.pdf");
        assert!(blocks[2].text.contains("之后"));
        assert!(blocks[2]
            .text
            .contains("https://example.invalid/example.pdf"));
        let code = markdown_blocks("```python\n# 示例\nvalue = 42\n```");
        assert_eq!(code[0].language, "python");
        assert_eq!(code[0].text, "# 示例\nvalue = 42\n");
    }

    #[test]
    fn public_workspace_paths_are_kept_as_local_resource_cards() {
        let blocks = markdown_blocks(
            "![图片](/workspaces/scope/workspace/image.png)\n\n[文档](/workspaces/scope/workspace/report.docx)",
        );
        assert_eq!(blocks[0].kind, KIND_IMAGE);
        assert_eq!(
            blocks[0].image_source,
            "/workspaces/scope/workspace/image.png"
        );
        assert_eq!(blocks[1].kind, 7);
        assert_eq!(
            blocks[1].image_source,
            "/workspaces/scope/workspace/report.docx"
        );
    }

    #[test]
    fn image_syntax_for_documents_uses_the_same_file_card_as_links() {
        let blocks = markdown_blocks(
            "![交付文档](/workspaces/scope/workspace/report.docx)\n\n![图片](/workspaces/scope/workspace/output.webp)",
        );
        assert_eq!(blocks[0].kind, 7);
        assert_eq!(blocks[0].text, "交付文档");
        assert!(blocks[0].image.to_rgba8().is_some());
        assert_eq!(blocks[1].kind, KIND_IMAGE);
        assert_eq!(
            blocks[1].image_source,
            "/workspaces/scope/workspace/output.webp"
        );
    }
}
