//! Bounded native Markdown projection.
//!
//! Streaming output remains plain text. Once a model response is terminal we
//! parse its visible prefix once with pulldown-cmark. Text blocks carry a
//! marker-free plain projection so the read-only text controls in the bubble
//! stay selectable and copyable with the mouse; code blocks keep Slint's
//! StyledText rendering for syntax colours.
use crate::TextBlock;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use slint::{ModelRc, StyledText, VecModel};
use std::rc::Rc;

const DISPLAY_BYTES: usize = 16 * 1024;
const DISPLAY_LINES: usize = 160;
pub const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
const KIND_TEXT: i32 = 0;
const KIND_CODE: i32 = 1;
const KIND_QUOTE: i32 = 4;
const KIND_TABLE: i32 = 5;
const KIND_IMAGE: i32 = 6;
const KIND_H1: i32 = 10;

pub struct Blocks {
    pub raw: String,
    pub model: Rc<VecModel<TextBlock>>,
    rendered: bool,
}

impl Blocks {
    pub fn new() -> Self {
        Self {
            raw: String::new(),
            model: Rc::new(VecModel::default()),
            rendered: false,
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
        Ok(())
    }

    /// Update the streaming view. No Markdown parsing takes place here.
    pub fn flush(&mut self) {
        if self.rendered {
            return;
        }
        self.model
            .set_vec(plain_blocks(visible_source(&self.raw).0));
    }

    /// Parse exactly once after the model output reaches a terminal state.
    pub fn finalize_markdown(&mut self) {
        if self.rendered {
            return;
        }
        let (source, capped) = visible_source(&self.raw);
        let mut blocks = markdown_blocks(source);
        if capped {
            blocks.push(text_block(
                "仅展示前 16 KiB 或 160 行；点击气泡下方“复制”获取完整原文。",
                KIND_H1,
            ));
        }
        self.model.set_vec(blocks);
        self.rendered = true;
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
    matches!(extension.as_str(), "png" | "jpg" | "jpeg").then_some(path)
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
            | "png"
            | "jpg"
            | "jpeg"
    )
}

fn visible_source(text: &str) -> (&str, bool) {
    let byte_end = boundary(text, text.len().min(DISPLAY_BYTES));
    let display_end = text[..byte_end]
        .match_indices('\n')
        .nth(DISPLAY_LINES - 1)
        .map_or(byte_end, |(offset, _)| offset + 1);
    (&text[..display_end], text.len() > display_end)
}

fn boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
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
        language: "".into(),
    }
}

fn image_block(alt: &str, source: &str) -> TextBlock {
    TextBlock {
        text: format!("图片：{alt}").into(),
        styled: StyledText::from_plain_text(alt),
        kind: KIND_IMAGE,
        table_cells: string_model(Vec::new()),
        table_columns: 0,
        table_rows: string_model(Vec::new()),
        image: slint::Image::default(),
        image_source: source.into(),
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

fn plain_blocks(source: &str) -> Vec<TextBlock> {
    if source.is_empty() {
        Vec::new()
    } else {
        vec![plain_text_block(source)]
    }
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
                    let mut block = plain_text_block(if label.is_empty() { target } else { label });
                    block.kind = 7;
                    block.image_source = target.as_str().into();
                    block.image = crate::file_icons::workspace_file_icon(target, "file");
                    blocks.push(block);
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
                if list_depth == 0 {
                    flush_block(&mut blocks, &mut text, &mut kind);
                }
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
        assert!(blocks[2].text.contains("https://example.invalid/example.pdf"));
        let code = markdown_blocks("```python\n# 示例\nvalue = 42\n```");
        assert_eq!(code[0].language, "python");
        assert_eq!(code[0].text, "# 示例\nvalue = 42\n");
    }
}
