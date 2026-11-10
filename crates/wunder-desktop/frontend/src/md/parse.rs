//! pulldown-cmark events → [`crate::md::ast`].
//!
//! Unlike the flat projection wunder's `message_blocks.rs` builds, this keeps
//! nesting intact: a quote owns its children, a list item owns the blocks that
//! follow its first paragraph. Flattening happens once, in `project`, where the
//! row spacing is decided — the UI never has to reconstruct structure.

use crate::md::ast::{is_blank, Align, Block, ImageRef, Inline, List, ListItem, Table};
use crate::md::source;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Parse a document into blocks.
///
/// A single newline inside a paragraph is a line break, not a wrap
/// opportunity: chat transcripts are written the way a human presses Enter,
/// not the way a book wraps. (`StyledText` receives it as a real `\n`.)
pub fn parse(source: &str) -> Vec<Block> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let mut builder = Builder {
        frames: vec![Frame::Root(Vec::new())],
        inline: Vec::new(),
        open: Vec::new(),
        link_url: None,
        heading: None,
        code: None,
        image: None,
        image_card: false,
        document: None,
        table: None,
    };

    for event in Parser::new_ext(source, options) {
        builder.event(event);
    }
    builder.flush_paragraph();

    match builder.frames.pop() {
        Some(Frame::Root(blocks)) => blocks,
        _ => Vec::new(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Strong,
    Emphasis,
    Strike,
    Link,
}

struct ListBuilder {
    ordered: bool,
    start: u64,
    items: Vec<ListItem>,
}

#[derive(Default)]
struct ItemBuilder {
    inline: Vec<Inline>,
    checked: Option<bool>,
    blocks: Vec<Block>,
}

enum Frame {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    List(ListBuilder),
    Item(ItemBuilder),
}

#[derive(Default)]
struct TableBuilder {
    align: Vec<Align>,
    head: Vec<Vec<Inline>>,
    rows: Vec<Vec<Vec<Inline>>>,
    row: Vec<Vec<Inline>>,
}

struct Builder {
    frames: Vec<Frame>,
    /// Inline nodes of the paragraph/heading/cell currently being read.
    inline: Vec<Inline>,
    /// Open emphasis frames, with the index in `inline` where they started.
    open: Vec<(Style, usize)>,
    link_url: Option<String>,
    heading: Option<u8>,
    code: Option<(String, String)>,
    image: Option<ImageRef>,
    /// Whether the image being read becomes a card (and therefore owns its
    /// own row) or stays inline text.
    image_card: bool,
    /// A link being read as a workspace file card: its label becomes the card
    /// title, so the inline events have to be captured, not emitted.
    document: Option<ImageRef>,
    table: Option<TableBuilder>,
}

impl Builder {
    fn event(&mut self, event: Event<'_>) {
        // Code block content is raw text: it must bypass every inline rule.
        if let Some((_, buffer)) = self.code.as_mut() {
            if let Event::Text(text) = &event {
                buffer.push_str(text);
                return;
            }
        }

        // A document link's label is a card title, not markup: collect it and
        // emit the card when the link closes.
        if let Some(document) = self.document.as_mut() {
            match &event {
                Event::Text(value)
                | Event::Code(value)
                | Event::Html(value)
                | Event::InlineHtml(value) => document.alt.push_str(value),
                Event::SoftBreak | Event::HardBreak => document.alt.push(' '),
                _ => {}
            }
            if let Event::End(TagEnd::Link) = event {
                let document = self.document.take().expect("document open");
                self.push_block(Block::Document(document));
            }
            return;
        }

        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.push_text(&text),
            Event::Code(value) => {
                if let Some(image) = self.image.as_mut() {
                    image.alt.push_str(&value);
                } else {
                    self.inline.push(Inline::Code(value.to_string()));
                }
            }
            Event::SoftBreak => self.inline.push(Inline::SoftBreak),
            Event::HardBreak => self.inline.push(Inline::HardBreak),
            Event::Rule => {
                self.flush_paragraph();
                self.push_block(Block::Rule);
            }
            Event::TaskListMarker(checked) => {
                if let Some(Frame::Item(item)) = self.frames.last_mut() {
                    item.checked = Some(checked);
                }
            }
            Event::FootnoteReference(label) => self.push_text(&format!("[{label}]")),
            Event::Html(raw) | Event::InlineHtml(raw) => {
                // Tags are dropped (nothing here can render them), but a <br>
                // is the one block-level tag authors actually rely on.
                let tag = raw.trim();
                if tag.eq_ignore_ascii_case("<br>") || tag.eq_ignore_ascii_case("<br/>") {
                    self.inline.push(Inline::HardBreak);
                }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush_paragraph();
                self.heading = Some(heading_level(level));
            }
            Tag::BlockQuote => {
                self.flush_paragraph();
                self.frames.push(Frame::Quote(Vec::new()));
            }
            Tag::CodeBlock(kind) => {
                self.flush_paragraph();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((language, String::new()));
            }
            Tag::List(start) => {
                // A tight list emits its item text without a Paragraph wrapper,
                // so anything still pending belongs to the enclosing item.
                self.flush_paragraph();
                self.frames.push(Frame::List(ListBuilder {
                    ordered: start.is_some(),
                    start: start.unwrap_or(1),
                    items: Vec::new(),
                }));
            }
            Tag::Item => {
                self.flush_paragraph();
                self.frames.push(Frame::Item(ItemBuilder::default()));
            }
            Tag::Emphasis => self.open_style(Style::Emphasis),
            Tag::Strong => self.open_style(Style::Strong),
            Tag::Strikethrough => self.open_style(Style::Strike),
            Tag::Link { dest_url, .. } => {
                // A link to a workspace document is an affordance, not prose:
                // it becomes a file card and the label becomes its title.
                if source::is_document(&dest_url) {
                    self.flush_paragraph();
                    self.document = Some(ImageRef {
                        source: dest_url.to_string(),
                        title: String::new(),
                        alt: String::new(),
                    });
                    return;
                }
                self.link_url = Some(dest_url.to_string());
                self.open_style(Style::Link);
            }
            Tag::Image {
                dest_url, title, ..
            } => {
                if self.table.is_some() {
                    // An image inside a cell cannot be a row: keep its target
                    // as text rather than tearing the table apart.
                    self.push_text(&dest_url);
                    return;
                }
                // Only a target the workspace can serve earns a card. Anything
                // else stays inline, so its paragraph must not be split here.
                let card = source::is_image(&dest_url) || source::is_document(&dest_url);
                if card {
                    self.flush_paragraph();
                }
                self.image_card = card;
                self.image = Some(ImageRef {
                    source: dest_url.to_string(),
                    title: title.to_string(),
                    alt: String::new(),
                });
            }
            Tag::Table(align) => {
                self.flush_paragraph();
                self.table = Some(TableBuilder {
                    align: align.into_iter().map(Align::from_cmark).collect(),
                    ..Default::default()
                });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.row.clear();
                }
            }
            Tag::TableCell => self.inline.clear(),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush_paragraph(),
            TagEnd::Heading(_) => {
                let level = self.heading.take().unwrap_or(1);
                let nodes = self.take_inline();
                if !is_blank(&nodes) {
                    self.push_block(Block::Heading {
                        level,
                        inline: nodes,
                    });
                }
            }
            TagEnd::BlockQuote => {
                self.flush_paragraph();
                if let Some(Frame::Quote(children)) = self.frames.pop() {
                    if !children.is_empty() {
                        self.push_block(Block::Quote(children));
                    }
                }
            }
            TagEnd::CodeBlock => {
                if let Some((language, text)) = self.code.take() {
                    self.push_block(Block::Code {
                        language,
                        text: text.strip_suffix('\n').unwrap_or(&text).to_string(),
                    });
                }
            }
            TagEnd::List(_) => {
                self.flush_paragraph();
                if let Some(Frame::List(list)) = self.frames.pop() {
                    if !list.items.is_empty() {
                        self.push_block(Block::List(List {
                            ordered: list.ordered,
                            start: list.start,
                            items: list.items,
                        }));
                    }
                }
            }
            TagEnd::Item => {
                self.flush_paragraph();
                if let Some(Frame::Item(item)) = self.frames.pop() {
                    if let Some(Frame::List(list)) = self.frames.last_mut() {
                        list.items.push(ListItem {
                            inline: item.inline,
                            checked: item.checked,
                            blocks: item.blocks,
                        });
                    }
                }
            }
            TagEnd::Emphasis => self.close_style(Style::Emphasis),
            TagEnd::Strong => self.close_style(Style::Strong),
            TagEnd::Strikethrough => self.close_style(Style::Strike),
            TagEnd::Link => self.close_style(Style::Link),
            TagEnd::Image => {
                if let Some(image) = self.image.take() {
                    let destination = image.source.trim().to_string();
                    if self.image_card {
                        if source::is_image(&destination) {
                            self.push_block(Block::Image(image));
                        } else {
                            self.push_block(Block::Document(image));
                        }
                    } else {
                        // Nothing can load this target, so it keeps its
                        // Markdown shape as text instead of a broken card.
                        let label = if image.alt.trim().is_empty() {
                            "图片".to_string()
                        } else {
                            image.alt.trim().to_string()
                        };
                        self.push_text(&format!("{label}（{destination}）"));
                    }
                    self.image_card = false;
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.head = std::mem::take(&mut table.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    let row = std::mem::take(&mut table.row);
                    if !row.is_empty() {
                        table.rows.push(row);
                    }
                }
            }
            TagEnd::TableCell => {
                let cell = self.take_inline();
                if let Some(table) = self.table.as_mut() {
                    table.row.push(cell);
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.push_block(Block::Table(Table {
                        align: table.align,
                        head: table.head,
                        rows: table.rows,
                    }));
                }
            }
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        if let Some(image) = self.image.as_mut() {
            image.alt.push_str(text);
            return;
        }
        // Coalescing is only safe while no span is open: merging two text
        // nodes would move the boundary an open span recorded in `open`.
        let merge = self.open.is_empty();
        match self.inline.last_mut() {
            Some(Inline::Text(last)) if merge => last.push_str(text),
            _ => self.inline.push(Inline::Text(text.to_string())),
        }
    }

    fn take_inline(&mut self) -> Vec<Inline> {
        std::mem::take(&mut self.inline)
    }

    /// Close the current paragraph, heading body or list item body.
    fn flush_paragraph(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let nodes = self.take_inline();
        if !is_blank(&nodes) {
            self.push_block(Block::Paragraph(nodes));
        }
    }

    fn push_block(&mut self, block: Block) {
        // A list is not a block container: walk out to the nearest one instead
        // of dropping the block. Tight-list text arrives before its nested
        // list opens, so this path is reachable on ordinary input.
        let mut block = Some(block);
        for index in (0..self.frames.len()).rev() {
            match &mut self.frames[index] {
                Frame::List(_) => continue,
                // The first paragraph of an item is the item's own line:
                // keeping it out of `blocks` is what turns a tight list into
                // one row per item.
                Frame::Item(item) => {
                    match block.take().expect("block placed once") {
                        Block::Paragraph(nodes)
                            if item.inline.is_empty() && item.blocks.is_empty() =>
                        {
                            item.inline = nodes;
                        }
                        other => item.blocks.push(other),
                    }
                    return;
                }
                Frame::Root(blocks) | Frame::Quote(blocks) => {
                    blocks.push(block.take().expect("block placed once"));
                    return;
                }
            }
        }
    }

    fn open_style(&mut self, style: Style) {
        let start = self.inline.len();
        self.open.push((style, start));
    }

    fn close_style(&mut self, style: Style) {
        let Some(position) = self.open.iter().rposition(|(open, _)| *open == style) else {
            // Malformed input: a stray closing marker. Drop it silently rather
            // than producing a half-open span.
            return;
        };
        let (_, start) = self.open.split_off(position).remove(0);
        let start = start.min(self.inline.len());
        let children = self.inline.split_off(start);
        if children.is_empty() {
            return;
        }
        let node = match style {
            Style::Strong => Inline::Strong(children),
            Style::Emphasis => Inline::Emphasis(children),
            Style::Strike => Inline::Strike(children),
            Style::Link => Inline::Link {
                url: self.link_url.take().unwrap_or_default(),
                children,
            },
        };
        self.inline.push(node);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_paragraphs_and_links_keep_their_inline_runs() {
        let blocks = parse("# 标题\n\n正文 **粗体** 和 [链接](https://example.com)。\n");
        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            Block::Heading { level, inline } => {
                assert_eq!(*level, 1);
                assert!(matches!(inline[0], Inline::Text(_)));
            }
            other => panic!("{other:?}"),
        }
        let Block::Paragraph(inline) = &blocks[1] else {
            panic!("expected paragraph");
        };
        assert!(inline.iter().any(|node| matches!(node, Inline::Strong(_))));
        assert!(inline
            .iter()
            .any(|node| matches!(node, Inline::Link { .. })));
    }

    #[test]
    fn a_quote_owns_its_children_instead_of_flattening() {
        let blocks = parse("> 第一段\n>\n> - 甲\n> - 乙\n");
        let Block::Quote(children) = &blocks[0] else {
            panic!("expected quote, got {blocks:?}");
        };
        assert!(matches!(children[0], Block::Paragraph(_)));
        assert!(matches!(children[1], Block::List(_)));
    }

    #[test]
    fn list_items_keep_nesting_and_task_state() {
        let blocks = parse("- 甲\n  - 甲一\n- [x] 已完成\n");
        let Block::List(list) = &blocks[0] else {
            panic!("expected list");
        };
        assert_eq!(list.items.len(), 2);
        assert!(list.items[0]
            .blocks
            .iter()
            .any(|block| matches!(block, Block::List(_))));
        assert_eq!(list.items[1].checked, Some(true));
    }

    #[test]
    fn tables_capture_header_rows_and_alignment() {
        let blocks = parse("| 名称 | 数值 |\n| :--- | ---: |\n| A | 1 |\n");
        let Block::Table(table) = &blocks[0] else {
            panic!("expected table");
        };
        assert_eq!(table.align, vec![Align::Left, Align::Right]);
        assert_eq!(table.head.len(), 2);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].len(), 2);
    }

    /// A table must not swallow the blocks that follow it. Content after a
    /// table used to disappear from the transcript, which looked like the reply
    /// had been cut off mid-message.
    #[test]
    fn blocks_after_a_table_are_kept() {
        let blocks = parse(
            "| 项目 | 表现 |\n| --- | --- |\n| 执行命令 | 正常 |\n\n### 建议修复优先级\n\n1. 先修复 stdout\n\n| 第二张 | 表 |\n| --- | --- |\n| a | b |\n\n结尾段落。\n",
        );
        let kinds = blocks
            .iter()
            .map(|block| match block {
                Block::Table(_) => "table",
                Block::Heading { .. } => "heading",
                Block::List(_) => "list",
                Block::Paragraph(_) => "paragraph",
                _ => "other",
            })
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec!["table", "heading", "list", "table", "paragraph"],
            "blocks: {kinds:?}"
        );
        assert!(
            matches!(blocks.last(), Some(Block::Paragraph(_))),
            "trailing paragraph lost: {kinds:?}"
        );
    }

    #[test]
    fn an_inline_image_splits_the_paragraph_around_it() {
        let blocks = parse("前面 ![图](a.png) 后面\n");
        assert_eq!(blocks.len(), 3);
        assert!(matches!(blocks[0], Block::Paragraph(_)));
        match &blocks[1] {
            Block::Image(image) => {
                assert_eq!(image.source, "a.png");
                assert_eq!(image.alt, "图");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(blocks[2], Block::Paragraph(_)));
    }

    #[test]
    fn code_blocks_stay_raw_and_keep_indentation() {
        let blocks = parse("```rust\n    let x = 1;\n\n```\n");
        match &blocks[0] {
            Block::Code { language, text } => {
                assert_eq!(language, "rust");
                assert_eq!(text, "    let x = 1;\n");
            }
            other => panic!("{other:?}"),
        }
    }
}
