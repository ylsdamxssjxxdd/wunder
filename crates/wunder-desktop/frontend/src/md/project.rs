//! AST → the flat row model the UI consumes.
//!
//! This is where the two things Slint cannot do for us are decided:
//!
//! * **vertical rhythm** — `StyledText` stacks paragraphs with no gap and has
//!   no spacing property, so the space above every block is computed here and
//!   applied as an offset by the Slint block component;
//! * **table columns** — Slint cannot measure a wrapped cell against a
//!   percentage width, so column ratios are derived from content width here
//!   and normalised to sum to 1.
//!
//! Quote nesting and list nesting are flattened here too: they become a
//! `quoted` flag and a `depth` counter on the row, which keeps the Slint side
//! free of recursive components.

use crate::code_highlight;
use crate::md::ast::{self, Align, Block, Inline, List, Table};
use crate::md::block;
use crate::md::inline;
use crate::md::parse;
use crate::md::source;
use crate::{MdBlock, MdCell};
use slint::{ModelRc, SharedString, StyledText, VecModel};
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// One projected document, ready to hand to the Slint row model.
///
/// The renderer is stateless between documents on purpose: the chat timeline
/// parses a message once when it settles, so there is no per-frame projection
/// for a cache to save. The styled-text cache below still pays off inside a
/// single document, where the same run (a bullet, a repeated cell) recurs.
#[derive(Default)]
pub struct MarkdownRenderer {
    cache: Cache,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            cache: Cache {
                capacity: 1024,
                ..Cache::default()
            },
        }
    }

    /// Parse and project one document.
    pub fn render(&mut self, source: &str) -> Vec<MdBlock> {
        let blocks = parse::parse(source);
        self.project(&blocks)
    }

    fn project(&mut self, blocks: &[Block]) -> Vec<MdBlock> {
        let mut rows = Vec::new();
        let mut prev: Option<Meta> = None;
        for block in blocks {
            self.emit(block, 0, false, &mut rows, &mut prev);
        }
        rows
    }

    fn emit(
        &mut self,
        block: &Block,
        depth: usize,
        quoted: bool,
        rows: &mut Vec<MdBlock>,
        prev: &mut Option<Meta>,
    ) {
        match block {
            Block::Paragraph(nodes) => {
                let mut row = self.prose_row(block::PARAGRAPH, nodes, quoted);
                row.space_before = space_before(block::PARAGRAPH, 0, *prev, false);
                rows.push(row);
                *prev = Some(Meta {
                    kind: block::PARAGRAPH,
                });
            }
            Block::Heading { level, inline: nodes } => {
                let markup = inline::markup_bold(nodes);
                let plain = inline::plain(nodes);
                let mut row = blank(block::HEADING);
                row.level = i32::from(*level);
                row.space_before = space_before(block::HEADING, *level, *prev, false);
                row.text = self.styled(&markup, &plain);
                row.plain = plain.into();
                row.quoted = quoted;
                rows.push(row);
                *prev = Some(Meta {
                    kind: block::HEADING,
                });
            }
            Block::Code { language, text } => {
                let mut row = blank(block::CODE);
                row.space_before = space_before(block::CODE, 0, *prev, false);
                row.language = language.as_str().into();
                row.code = text.as_str().into();
                row.code_text = code_highlight::highlight(text, language);
                row.plain = text.as_str().into();
                row.quoted = quoted;
                rows.push(row);
                *prev = Some(Meta { kind: block::CODE });
            }
            Block::Quote(children) => {
                for child in children {
                    self.emit(child, depth, true, rows, prev);
                }
            }
            Block::List(list) => self.emit_list(list, depth, quoted, rows, prev),
            Block::Table(table) => self.emit_table(table, quoted, rows, prev),
            Block::Image(image) => {
                let mut row = blank(block::IMAGE);
                row.space_before = space_before(block::IMAGE, 0, *prev, false);
                row.image_source = image.source.as_str().into();
                row.plain = if image.alt.trim().is_empty() {
                    source::file_name(&image.source, "图片").into()
                } else {
                    image.alt.as_str().into()
                };
                row.quoted = quoted;
                rows.push(row);
                *prev = Some(Meta {
                    kind: block::IMAGE,
                });
            }
            Block::Document(document) => {
                let mut row = blank(block::DOCUMENT);
                row.space_before = space_before(block::DOCUMENT, 0, *prev, false);
                row.image_source = document.source.as_str().into();
                let file_name = source::file_name(&document.source, "文件");
                row.image = crate::file_icons::workspace_file_icon(&file_name, "file");
                row.resource_name = file_name.as_str().into();
                let title = if document.alt.trim().is_empty() {
                    row.resource_name.clone()
                } else {
                    document.alt.as_str().into()
                };
                row.plain = title;
                row.quoted = quoted;
                rows.push(row);
                *prev = Some(Meta {
                    kind: block::DOCUMENT,
                });
            }
            Block::Rule => {
                let mut row = blank(block::RULE);
                row.space_before = space_before(block::RULE, 0, *prev, false);
                row.quoted = quoted;
                rows.push(row);
                *prev = Some(Meta { kind: block::RULE });
            }
        }
    }

    fn emit_list(
        &mut self,
        list: &List,
        depth: usize,
        quoted: bool,
        rows: &mut Vec<MdBlock>,
        prev: &mut Option<Meta>,
    ) {
        let mut number = list.start;
        for (index, item) in list.items.iter().enumerate() {
            if !ast::is_blank(&item.inline) {
                let mut row = self.prose_row(block::LIST_ITEM, &item.inline, quoted);
                row.marker = marker(list.ordered, number, item.checked).into();
                row.task = match item.checked {
                    Some(true) => 2,
                    Some(false) => 1,
                    None => 0,
                };
                row.depth = depth as i32;
                row.space_before = space_before(
                    block::LIST_ITEM,
                    0,
                    *prev,
                    index == 0 && depth == 0,
                );
                rows.push(row);
                *prev = Some(Meta {
                    kind: block::LIST_ITEM,
                });
            }
            for child in &item.blocks {
                self.emit(child, depth + 1, quoted, rows, prev);
            }
            number += 1;
        }
    }

    fn emit_table(
        &mut self,
        table: &Table,
        quoted: bool,
        rows: &mut Vec<MdBlock>,
        prev: &mut Option<Meta>,
    ) {
        let mut grid: Vec<&Vec<Vec<Inline>>> = Vec::new();
        if !table.head.is_empty() {
            grid.push(&table.head);
        }
        grid.extend(table.rows.iter());

        let columns = grid.iter().map(|row| row.len()).max().unwrap_or(0).max(1);
        let mut weights = vec![0.0f32; columns];
        let mut cells = Vec::with_capacity(grid.len() * columns);

        for row in &grid {
            for column in 0..columns {
                let nodes: &[Inline] = row.get(column).map(Vec::as_slice).unwrap_or(&[]);
                let plain = inline::plain(nodes);
                let markup = inline::markup(nodes);
                weights[column] = weights[column].max(display_width(&plain) as f32);
                let align = table.align.get(column).copied().unwrap_or(Align::None);
                cells.push(MdCell {
                    text: self.styled(&markup, &plain),
                    plain: plain.into(),
                    align: align.code(),
                });
            }
        }

        let column_widths = column_pixels(&weights);
        let mut row = blank(block::TABLE);
        row.space_before = space_before(block::TABLE, 0, *prev, false);
        row.columns = columns as i32;
        row.widths = float_model(&normalize(weights));
        row.column_widths = float_model(&column_widths);
        row.table_width = column_widths.iter().sum();
        row.cells = cell_model(cells);
        row.quoted = quoted;
        rows.push(row);
        *prev = Some(Meta {
            kind: block::TABLE,
        });
    }

    fn prose_row(&mut self, kind: i32, nodes: &[Inline], quoted: bool) -> MdBlock {
        let markup = inline::markup(nodes);
        let plain = inline::plain(nodes);
        let mut row = blank(kind);
        row.text = self.styled(&markup, &plain);
        row.plain = plain.into();
        row.quoted = quoted;
        row
    }

    /// Parse markup once per distinct string. A document repeats short runs
    /// (bullets, punctuation, table cells), and the chat regenerates the same
    /// message on every history refresh.
    fn styled(&mut self, markup: &str, plain: &str) -> StyledText {
        let key = hash64(markup);
        if let Some(value) = self.cache.get(key) {
            return value;
        }
        let value = StyledText::from_markdown(markup)
            .unwrap_or_else(|_| StyledText::from_plain_text(plain));
        self.cache.put(key, value.clone());
        value
    }
}

#[derive(Clone, Copy)]
struct Meta {
    kind: i32,
}

fn space_before(kind: i32, level: u8, prev: Option<Meta>, first_in_list: bool) -> f32 {
    let Some(prev) = prev else {
        return 0.0;
    };
    let after_heading = prev.kind == block::HEADING;
    match kind {
        block::HEADING => {
            let base = match level {
                1 => 26.0,
                2 => 22.0,
                3 => 18.0,
                _ => 15.0,
            };
            if after_heading {
                base - 10.0
            } else {
                base
            }
        }
        block::PARAGRAPH => {
            if after_heading {
                10.0
            } else {
                9.0
            }
        }
        block::CODE => {
            if after_heading {
                9.0
            } else {
                12.0
            }
        }
        block::LIST_ITEM => {
            if first_in_list {
                8.0
            } else {
                4.0
            }
        }
        block::TABLE => {
            if after_heading {
                10.0
            } else {
                13.0
            }
        }
        block::IMAGE | block::DOCUMENT => 13.0,
        block::RULE => 18.0,
        _ => 9.0,
    }
}

fn marker(ordered: bool, number: u64, checked: Option<bool>) -> String {
    match checked {
        // The checkbox is drawn by the Slint component: the bundled face has no
        // U+2610/U+2611, and a missing glyph here reads as an empty bullet.
        Some(_) => String::new(),
        None if ordered => format!("{number}."),
        None => "•".to_string(),
    }
}

/// Column weights from content width: one wide column should not squeeze the
/// others to a sliver, so each is clamped before normalising.
fn normalize(mut weights: Vec<f32>) -> Vec<f32> {
    if weights.is_empty() {
        return Vec::new();
    }
    for weight in &mut weights {
        *weight = weight.clamp(3.0, 26.0);
    }
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        let even = 1.0 / weights.len() as f32;
        return vec![even; weights.len()];
    }
    weights.into_iter().map(|weight| weight / total).collect()
}

/// Natural column widths in pixels for the touch table layout: the widest
/// cell at body-font scale, clamped so one long path cannot dominate and a
/// narrow column still shows a few characters.
fn column_pixels(weights: &[f32]) -> Vec<f32> {
    weights
        .iter()
        .map(|weight| (weight * 7.5 + 26.0).clamp(64.0, 260.0))
        .collect()
}

/// Display width in terminal cells: CJK and emoji take two.
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| if is_wide(ch) { 2 } else { 1 })
        .sum()
}

fn is_wide(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1FAFF
    )
}

fn hash64(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn blank(kind: i32) -> MdBlock {
    MdBlock {
        kind,
        level: 0,
        text: StyledText::default(),
        plain: SharedString::default(),
        language: SharedString::default(),
        code: SharedString::default(),
        code_text: StyledText::default(),
        depth: 0,
        marker: SharedString::default(),
        task: 0,
        quoted: false,
        columns: 0,
        widths: float_model(&[]),
        cells: cell_model(Vec::new()),
        column_widths: float_model(&[]),
        table_width: 0.0,
        image: slint::Image::default(),
        image_source: SharedString::default(),
        image_width: 0.0,
        image_height: 0.0,
        image_error: false,
        image_frames: ModelRc::default(),
        frame_delays: ModelRc::default(),
        resource_name: SharedString::default(),
        space_before: 0.0,
    }
}

fn float_model(values: &[f32]) -> ModelRc<f32> {
    ModelRc::from(Rc::new(VecModel::from(values.to_vec())))
}

fn cell_model(values: Vec<MdCell>) -> ModelRc<MdCell> {
    ModelRc::from(Rc::new(VecModel::from(values)))
}

#[derive(Default)]
struct Cache {
    map: HashMap<u64, StyledText>,
    order: VecDeque<u64>,
    capacity: usize,
}

impl Cache {
    fn get(&mut self, key: u64) -> Option<StyledText> {
        let value = self.map.get(&key)?.clone();
        if let Some(position) = self.order.iter().position(|held| *held == key) {
            self.order.remove(position);
        }
        self.order.push_back(key);
        Some(value)
    }

    fn put(&mut self, key: u64, value: StyledText) {
        if self.map.insert(key, value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > self.capacity.max(1) {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    fn kinds(rows: &[MdBlock]) -> Vec<i32> {
        rows.iter().map(|row| row.kind).collect()
    }

    #[test]
    fn block_kinds_and_spacing_follow_the_document() {
        let mut renderer = MarkdownRenderer::new();
        let rows = renderer.render(
            "# 标题\n\n段落。\n\n```rust\nfn main() {}\n```\n\n- 甲\n- 乙\n\n> 引用\n",
        );
        assert_eq!(
            kinds(&rows),
            vec![
                block::HEADING,
                block::PARAGRAPH,
                block::CODE,
                block::LIST_ITEM,
                block::LIST_ITEM,
                block::PARAGRAPH,
            ]
        );
        assert_eq!(rows[0].space_before, 0.0, "the first block has no gap");
        assert!(rows[2].space_before > 0.0);
        assert_eq!(rows[3].marker, "•");
        assert_eq!(rows[4].marker, "•");
        assert!(rows[5].quoted, "a quote child is flagged, not wrapped");
        assert_eq!(rows[2].language, "rust");
    }

    #[test]
    fn nested_lists_increase_depth_and_keep_numbering() {
        let mut renderer = MarkdownRenderer::new();
        let rows = renderer.render("1. 甲\n2. 乙\n   - 乙一\n3. 丙\n");
        let list: Vec<_> = rows
            .iter()
            .filter(|row| row.kind == block::LIST_ITEM)
            .map(|row| (row.marker.to_string(), row.depth))
            .collect();
        assert_eq!(
            list,
            vec![
                ("1.".to_string(), 0),
                ("2.".to_string(), 0),
                ("•".to_string(), 1),
                ("3.".to_string(), 0),
            ]
        );
    }

    #[test]
    fn table_widths_are_normalised_and_columns_are_padded() {
        let mut renderer = MarkdownRenderer::new();
        let rows = renderer.render("| 名称 | 值 |\n| --- | --- |\n| 长一些的名称 | 1 |\n");
        let table = &rows[0];
        assert_eq!(table.kind, block::TABLE);
        assert_eq!(table.columns, 2);
        assert_eq!(table.cells.row_count(), 4);
        let widths: Vec<f32> = table.widths.iter().collect();
        assert!((widths.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert!(
            widths[0] > widths[1],
            "the wider column should get more room: {widths:?}"
        );
        // Touch layout: natural pixels summing to the table width.
        let natural: Vec<f32> = table.column_widths.iter().collect();
        assert_eq!(natural.len(), 2);
        assert!((natural.iter().sum::<f32>() - table.table_width).abs() < 0.01);
        assert!(natural[0] > natural[1]);
    }

    #[test]
    fn an_empty_document_projects_to_no_rows() {
        let mut renderer = MarkdownRenderer::new();
        assert!(renderer.render("").is_empty());
        assert!(renderer.render("   \n\n").is_empty());
    }

    #[test]
    fn workspace_document_links_become_file_cards() {
        let mut renderer = MarkdownRenderer::new();
        let rows = renderer.render("见 [季度报告](notes/q3.pdf)。\n");
        let card = rows
            .iter()
            .find(|row| row.kind == block::DOCUMENT)
            .expect("document card");
        assert_eq!(card.plain, "季度报告");
        assert_eq!(card.resource_name, "q3.pdf");
        assert_eq!(card.image_source, "notes/q3.pdf");
    }

    #[test]
    fn remote_images_stay_inline_text_instead_of_a_broken_card() {
        let mut renderer = MarkdownRenderer::new();
        let rows = renderer.render("前 ![图](https://example.com/a.png) 后\n");
        assert_eq!(kinds(&rows), vec![block::PARAGRAPH]);
        assert!(rows[0].plain.contains("https://example.com/a.png"));
    }
}
