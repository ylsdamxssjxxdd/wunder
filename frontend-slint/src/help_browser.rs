//! 帮助文档控制器：解析内嵌手册 JSON，用 parley（Slint 自带文字整形器）
//! 排版成绝对定位的文本片段与装饰矩形，再绑定到 HelpDocs 全局状态。
//! 视图层（help_*.slint）只做投影，不持有阅读状态，切页不丢进度。

use crate::{HelpDocs, HelpEntry, HelpRect, HelpTextRun};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontWeight, LayoutContext, LineHeight, StyleProperty,
};
use serde::Deserialize;
use slint::{Color, ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, ops::Range, rc::Rc};

/// 主窗口默认字体，与 main.slint 的 default-font-family 保持一致。
const UI_FAMILY: &str = "Microsoft YaHei";
/// 代码片段字体，与 markdown.slint 的代码块约定保持一致。
const MONO_FAMILY: &str = "Consolas";
/// 卡片双栏排版的最低文档宽度，低于该值退化为单栏。
const SINGLE_COLUMN_WIDTH: f32 = 640.0;

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(rows)).into()
}

fn color(rgb: u32) -> Color {
    Color::from_rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

fn rect(x: f32, y: f32, width: f32, height: f32, fill: u32, border: Option<u32>, radius: f32) -> HelpRect {
    HelpRect {
        x,
        y,
        width,
        height,
        fill: color(fill),
        border: border.map(color).unwrap_or_default(),
        border_width: if border.is_some() { 1.0 } else { 0.0 },
        radius,
    }
}

// ---------------------------------------------------------------------------
// 手册内容模型
// ---------------------------------------------------------------------------

#[derive(Clone, Default, Deserialize)]
pub struct Span {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub code: bool,
    #[serde(default)]
    pub kbd: bool,
    #[serde(default)]
    pub href: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Block {
    H1 { spans: Vec<Span> },
    H2 { spans: Vec<Span> },
    Paragraph { spans: Vec<Span> },
    Lead { spans: Vec<Span> },
    Task { spans: Vec<Span> },
    Tip { spans: Vec<Span> },
    Callout { spans: Vec<Span> },
    List { spans: Vec<Span>, marker: String },
    Table { rows: Vec<TableRow> },
    Cards { cards: Vec<Card> },
}

#[derive(Deserialize)]
pub struct TableRow {
    pub header: bool,
    pub cells: Vec<Vec<Span>>,
}

#[derive(Deserialize)]
pub struct Card {
    pub target: String,
    pub title: Vec<Span>,
    pub summary: Vec<Span>,
}

#[derive(Deserialize)]
pub struct Topic {
    pub id: String,
    pub title: String,
    pub keywords: Vec<String>,
    pub blocks: Vec<Block>,
    #[serde(skip)]
    pub searchable: String,
}

#[derive(Deserialize)]
pub struct IndexEntry {
    pub keyword: String,
    pub topic: usize,
}

#[derive(Deserialize)]
pub struct Manual {
    pub topics: Vec<Topic>,
    pub index: Vec<IndexEntry>,
}

impl Manual {
    pub fn load() -> Self {
        let mut manual: Self = serde_json::from_str(include_str!("../assets/help/manual.json"))
            .expect("内置帮助手册 JSON 已校验");
        for topic in &mut manual.topics {
            let mut text = format!("{} {} ", topic.title, topic.keywords.join(" "));
            for block in &topic.blocks {
                block.visit_text(&mut |spans| {
                    for span in spans {
                        text.push_str(&span.text);
                    }
                    text.push(' ');
                });
            }
            topic.searchable = text.to_lowercase();
        }
        manual
    }

    pub fn search(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        self.topics
            .iter()
            .enumerate()
            .filter(|(_, topic)| topic.searchable.contains(&query))
            .map(|(i, _)| i)
            .collect()
    }
}

impl Block {
    fn visit_text(&self, visit: &mut impl FnMut(&[Span])) {
        match self {
            Self::Table { rows } => rows
                .iter()
                .for_each(|r| r.cells.iter().for_each(|c| visit(c))),
            Self::Cards { cards } => cards.iter().for_each(|c| {
                visit(&c.title);
                visit(&c.summary);
            }),
            Self::H1 { spans }
            | Self::H2 { spans }
            | Self::Paragraph { spans }
            | Self::Lead { spans }
            | Self::Task { spans }
            | Self::Tip { spans }
            | Self::Callout { spans }
            | Self::List { spans, .. } => visit(spans),
        }
    }
}

// ---------------------------------------------------------------------------
// 命中与文档几何
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Hit {
    pub range: Range<usize>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub link: String,
}

#[derive(Default)]
pub struct Document {
    pub runs: Vec<HelpTextRun>,
    pub rects: Vec<HelpRect>,
    pub hits: Vec<Hit>,
    pub text: String,
    pub height: f32,
}

impl Document {
    pub fn hit_offset(&self, x: f32, y: f32) -> usize {
        self.hits
            .iter()
            .filter(|h| !h.range.is_empty())
            .min_by(|a, b| {
                let distance = |h: &Hit| {
                    let dy = (h.y - y).max(0.0).max(y - h.y - h.height);
                    let dx = (h.x - x).max(0.0).max(x - h.x - h.width);
                    dy * 10000.0 + dx
                };
                distance(a).total_cmp(&distance(b))
            })
            .map(|h| if x < h.x + h.width / 2.0 { h.range.start } else { h.range.end })
            .unwrap_or(0)
    }

    pub fn link_at(&self, x: f32, y: f32) -> Option<&str> {
        self.hits
            .iter()
            .rev()
            .find(|h| {
                !h.link.is_empty()
                    && x >= h.x
                    && x <= h.x + h.width
                    && y >= h.y
                    && y <= h.y + h.height
            })
            .map(|h| h.link.as_str())
    }

    pub fn marks(&self, ranges: &[Range<usize>], fill: u32) -> Vec<HelpRect> {
        let mut marks: Vec<HelpRect> = Vec::new();
        for hit in &self.hits {
            if !ranges.iter().any(|r| r.start < hit.range.end && hit.range.start < r.end) {
                continue;
            }
            if let Some(last) = marks.last_mut() {
                if (last.y - hit.y).abs() < 0.1 && (last.x + last.width - hit.x).abs() < 0.5 {
                    last.width += hit.width;
                    continue;
                }
            }
            marks.push(rect(hit.x, hit.y, hit.width, hit.height, fill, None, 0.0));
        }
        marks
    }

    pub fn find(&self, query: &str) -> Vec<Range<usize>> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        // 大小写折叠可能扩宽字符（İ），保留原始 UTF-8 偏移。
        let mut folded = String::new();
        let mut offsets = Vec::new();
        for (offset, ch) in self.text.char_indices() {
            let lower = ch.to_lowercase().to_string();
            offsets.extend(std::iter::repeat_n(
                (offset, offset + ch.len_utf8()),
                lower.len(),
            ));
            folded.push_str(&lower);
        }
        folded
            .match_indices(&query)
            .map(|(start, s)| offsets[start].0..offsets[start + s.len() - 1].1)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 排版器
// ---------------------------------------------------------------------------

pub struct Typesetter {
    fonts: FontContext,
    context: LayoutContext<usize>,
}

impl Typesetter {
    pub fn new() -> Self {
        // Host fonts are never loaded; the bundled YaHei is registered in the
        // shared collection at startup (main.rs).
        let collection = slint::fontique_011::shared_collection();
        Self {
            fonts: FontContext {
                collection,
                ..Default::default()
            },
            context: LayoutContext::new(),
        }
    }

    fn text(
        &mut self,
        doc: &mut Document,
        spans: &[Span],
        x: f32,
        y: f32,
        width: f32,
        size: f32,
        ink: u32,
        bold: bool,
        center: bool,
    ) -> f32 {
        let family = UI_FAMILY;
        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
        if text.is_empty() {
            return 0.0;
        }
        let start = doc.text.len();
        doc.text.push_str(&text);
        doc.text.push('\n');
        let mut builder = self
            .context
            .ranged_builder(&mut self.fonts, &text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(family.into()));
        builder.push_default(StyleProperty::FontSize(size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(if bold {
            650.0
        } else {
            400.0
        })));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.68)));
        builder.push_default(StyleProperty::OverflowWrap(parley::OverflowWrap::Anywhere));
        let mut offset = 0;
        for (i, span) in spans.iter().enumerate() {
            let range = offset..offset + span.text.len();
            builder.push(StyleProperty::Brush(i), range.clone());
            if span.bold {
                builder.push(StyleProperty::FontWeight(FontWeight::new(700.0)), range.clone());
            }
            if span.code || span.kbd {
                builder.push(StyleProperty::FontFamily(MONO_FAMILY.into()), range.clone());
                builder.push(StyleProperty::FontSize(size * 0.85), range.clone());
            }
            offset = range.end;
        }
        let mut layout = builder.build(&text);
        layout.break_all_lines(Some(width.max(1.0)));
        layout.align(
            if center { Alignment::Center } else { Alignment::Start },
            AlignmentOptions::default(),
        );
        for line in layout.lines() {
            let metrics = line.metrics();
            let mut cx = x + metrics.offset;
            for run in line.runs() {
                let mut groups: Vec<(Range<usize>, usize, f32, f32)> = Vec::new();
                for cluster in run.visual_clusters() {
                    let range = cluster.text_range();
                    let index = cluster.first_style().brush;
                    let advance = cluster.advance();
                    doc.hits.push(Hit {
                        range: start + range.start..start + range.end,
                        x: cx,
                        y: y + metrics.block_min_coord,
                        width: advance,
                        height: metrics.line_height,
                        link: spans[index].href.clone(),
                    });
                    if let Some(last) = groups
                        .last_mut()
                        .filter(|g| g.1 == index && g.0.end == range.start)
                    {
                        last.0.end = range.end;
                        last.3 += advance;
                    } else {
                        groups.push((range, index, cx, advance));
                    }
                    cx += advance;
                }
                for (range, index, gx, advance) in groups {
                    // 换行可能带来超出步进的尾随空格：保留复制偏移，
                    // 但不为这段不可见空白生成文本条目。
                    let visible_text = text[range].trim_end();
                    if visible_text.is_empty() {
                        continue;
                    }
                    let span = &spans[index];
                    let mono = span.code || span.kbd;
                    if mono {
                        doc.rects.push(rect(
                            gx,
                            y + metrics.block_min_coord + 2.0,
                            advance,
                            metrics.line_height - 4.0,
                            0xf7f7f7,
                            Some(if span.kbd { 0xc4c4c4 } else { 0xe1e1e1 }),
                            if span.kbd { 3.0 } else { 0.0 },
                        ));
                    }
                    doc.runs.push(HelpTextRun {
                        x: gx,
                        y: y + metrics.block_min_coord,
                        width: (advance + 1.0).min((x + width - gx).max(1.0)),
                        height: metrics.line_height,
                        text: visible_text.into(),
                        size: if mono { size * 0.85 } else { size },
                        weight: if span.bold {
                            700
                        } else if bold {
                            650
                        } else {
                            400
                        },
                        color: color(if span.href.is_empty() { ink } else { 0x17a81a }),
                        family: if mono { MONO_FAMILY } else { family }.into(),
                    });
                }
            }
        }
        layout.height()
    }

    pub fn layout(&mut self, topic: &Topic, width: f32, zoom: f32, single_column: bool) -> Document {
        let mut doc = Document::default();
        let x = 30.0;
        let w = (width - 60.0).max(80.0);
        let mut y = 22.0;
        for block in &topic.blocks {
            match block {
                Block::H1 { spans } => {
                    y += self.text(&mut doc, spans, x, y, w, 26.0 * zoom, 0x17a81a, true, false) + 8.0;
                    doc.rects.push(rect(x, y, w, 1.0, 0xe5e7eb, None, 0.0));
                    y += 16.0;
                }
                Block::H2 { spans } => {
                    y += 10.0;
                    y += self.text(&mut doc, spans, x, y, w, 19.0 * zoom, 0x17a81a, true, false) + 10.0;
                }
                Block::Paragraph { spans } | Block::Lead { spans } => {
                    let lead = matches!(block, Block::Lead { .. });
                    y += self.text(
                        &mut doc,
                        spans,
                        x,
                        y,
                        w,
                        if lead { 16.0 } else { 15.0 } * zoom,
                        if lead { 0x3f4a54 } else { 0x202020 },
                        false,
                        false,
                    ) + 15.0;
                }
                Block::Task { spans } | Block::Tip { spans } | Block::Callout { spans } => {
                    let background_index = doc.rects.len();
                    let (bg, ink, border, side) = match block {
                        Block::Task { .. } => (0xeef6fc, 0x29485d, 0x2f78b7, 4.0),
                        Block::Tip { .. } => (0xfff9e8, 0x765716, 0xe2ad4e, 3.0),
                        _ => (0xf0f8ff, 0x234f6f, 0xa9cce7, 0.0),
                    };
                    let height = self.text(
                        &mut doc,
                        spans,
                        x + 13.0,
                        y + 10.0,
                        w - 26.0,
                        if matches!(block, Block::Callout { .. }) { 13.0 } else { 15.0 } * zoom,
                        ink,
                        false,
                        false,
                    ) + 20.0;
                    // 装饰矩形始终垫在所有文本之下，与插入顺序无关。
                    doc.rects.insert(
                        background_index,
                        rect(
                            x,
                            y,
                            w,
                            height,
                            bg,
                            if side == 0.0 { Some(border) } else { None },
                            if side == 0.0 { 5.0 } else { 0.0 },
                        ),
                    );
                    if side > 0.0 {
                        doc.rects.push(rect(x, y, side, height, border, None, 0.0));
                    }
                    y += height + 18.0;
                }
                Block::List { spans, marker } => {
                    let marker = [Span {
                        text: marker.clone(),
                        ..Default::default()
                    }];
                    self.text(&mut doc, &marker, x + 16.0, y, 24.0, 15.0 * zoom, 0x202020, false, false);
                    y += self.text(&mut doc, spans, x + 40.0, y, w - 40.0, 15.0 * zoom, 0x202020, false, false) + 5.0;
                }
                Block::Table { rows } => {
                    for row in rows {
                        let columns = row.cells.len();
                        let mut height: f32 = 0.0;
                        let mut widths = vec![w / columns as f32; columns];
                        if columns == 2 {
                            widths = vec![w * 0.19, w * 0.81];
                        }
                        let mut cx = x;
                        for (cell, cw) in row.cells.iter().zip(&widths) {
                            height = height.max(
                                self.text(
                                    &mut doc,
                                    cell,
                                    cx + 8.0,
                                    y + 6.0,
                                    cw - 16.0,
                                    14.0 * zoom,
                                    if row.header { 0x26323b } else { 0x202020 },
                                    row.header,
                                    false,
                                ) + 12.0,
                            );
                            cx += cw;
                        }
                        cx = x;
                        for cw in widths {
                            doc.rects.push(rect(
                                cx,
                                y,
                                cw,
                                height,
                                if row.header { 0xf4f6f8 } else { 0xffffff },
                                Some(0xe5e7eb),
                                0.0,
                            ));
                            cx += cw;
                        }
                        y += height - 1.0;
                    }
                    y += 16.0;
                }
                Block::Cards { cards } => {
                    let cols = if single_column { 1 } else { 2 };
                    let cw = (w - (cols - 1) as f32 * 10.0) / cols as f32;
                    for row in cards.chunks(cols) {
                        let mut height: f32 = 0.0;
                        for (i, card) in row.iter().enumerate() {
                            let cx = x + i as f32 * (cw + 10.0);
                            let th = self.text(&mut doc, &card.title, cx + 12.0, y + 10.0, cw - 24.0, 13.0 * zoom, 0x17a81a, true, false);
                            let sh = self.text(&mut doc, &card.summary, cx + 12.0, y + 14.0 + th, cw - 24.0, 13.0 * zoom, 0x5b6872, false, false);
                            height = height.max(th + sh + 24.0);
                        }
                        for (i, card) in row.iter().enumerate() {
                            let cx = x + i as f32 * (cw + 10.0);
                            doc.rects.push(rect(cx, y, cw, height, 0xfbfdfb, Some(0xd7e2d8), 4.0));
                            doc.hits.push(Hit {
                                range: 0..0,
                                x: cx,
                                y,
                                width: cw,
                                height,
                                link: card.target.clone(),
                            });
                        }
                        y += height + 10.0;
                    }
                    y += 10.0;
                }
            }
        }
        doc.height = y + 48.0;
        doc
    }
}

// ---------------------------------------------------------------------------
// 阅读历史与控制器
// ---------------------------------------------------------------------------

#[derive(Default)]
struct History {
    entries: Vec<usize>,
    cursor: usize,
}

impl History {
    fn new() -> Self {
        Self {
            entries: vec![0],
            cursor: 0,
        }
    }
    fn current(&self) -> usize {
        self.entries[self.cursor]
    }
    fn can_back(&self) -> bool {
        self.cursor > 0
    }
    fn can_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }
    fn open(&mut self, topic: usize) {
        self.entries.truncate(self.cursor + 1);
        self.entries.push(topic);
        self.cursor += 1;
    }
    fn go(&mut self, delta: i32) -> bool {
        let next = self.cursor as i64 + i64::from(delta);
        if next < 0 || next >= self.entries.len() as i64 {
            return false;
        }
        self.cursor = next as usize;
        true
    }
}

struct Controller {
    manual: Manual,
    history: History,
    typesetter: Typesetter,
    document: Document,
    selection: Range<usize>,
    press: Option<(f32, f32, usize)>,
    layout_key: Option<(usize, i32, i32)>,
}

impl Controller {
    fn new() -> Self {
        Self {
            manual: Manual::load(),
            history: History::new(),
            typesetter: Typesetter::new(),
            document: Document::default(),
            selection: 0..0,
            press: None,
            layout_key: None,
        }
    }

    fn search(&self, docs: &HelpDocs, query: &str) {
        let rows = self
            .manual
            .search(query)
            .into_iter()
            .map(|i| {
                let topic = &self.manual.topics[i];
                HelpEntry {
                    topic: i as i32,
                    title: topic.title.clone().into(),
                    detail: format!("使用手册 / {}", topic.keywords.join(" / ")).into(),
                }
            })
            .collect();
        docs.set_search_results(model(rows));
    }

    fn render(&mut self, docs: &HelpDocs) {
        let width = docs.get_document_width().max(120.0).round();
        let zoom = docs.get_zoom_percent().clamp(80, 160);
        let key = (self.history.current(), width as i32, zoom);
        if self.layout_key.as_ref() == Some(&key) {
            return;
        }
        let topic = &self.manual.topics[self.history.current()];
        self.document = self.typesetter.layout(
            topic,
            width,
            zoom as f32 / 100.0,
            width < SINGLE_COLUMN_WIDTH,
        );
        self.layout_key = Some(key);
        docs.set_selected_topic(self.history.current() as i32);
        docs.set_topic_title(topic.title.clone().into());
        docs.set_runs(model(self.document.runs.clone()));
        docs.set_decorations(model(self.document.rects.clone()));
        docs.set_document_height(self.document.height);
        self.update_selection(docs);
        self.find(docs, false);
    }

    fn navigate(&mut self, docs: &HelpDocs) {
        self.selection = 0..0;
        self.press = None;
        self.layout_key = None;
        self.render(docs);
        docs.set_can_back(self.history.can_back());
        docs.set_can_forward(self.history.can_forward());
        docs.set_scroll_request(docs.get_scroll_request() + 1);
    }

    fn find(&self, docs: &HelpDocs, reveal: bool) {
        let ranges = self.document.find(docs.get_find_text().as_str());
        let marks = self.document.marks(&ranges, 0xfff176);
        if reveal {
            if let Some(first) = marks.first() {
                docs.set_reveal_y(first.y);
                docs.set_reveal_request(docs.get_reveal_request() + 1);
            }
        }
        docs.set_match_count(ranges.len() as i32);
        docs.set_matches(model(marks));
    }

    fn update_selection(&self, docs: &HelpDocs) {
        docs.set_selection(model(
            self.document
                .marks(std::slice::from_ref(&self.selection), 0xb8d6ef),
        ));
    }

    fn copy_selection(&self, docs: &HelpDocs) {
        let selected = self.document.text.get(self.selection.clone()).unwrap_or("");
        let text = if selected.trim().is_empty() {
            self.document.text.as_str()
        } else {
            selected
        };
        docs.set_copy_text(text.into());
        docs.set_copy_request(docs.get_copy_request() + 1);
    }

    fn pointer(&mut self, docs: &HelpDocs, x: f32, y: f32, kind: i32) {
        match kind {
            0 => {
                let offset = self.document.hit_offset(x, y);
                self.press = Some((x, y, offset));
                self.selection = offset..offset;
                self.update_selection(docs);
            }
            1 => {
                if let Some((_, _, anchor)) = self.press {
                    let offset = self.document.hit_offset(x, y);
                    self.selection = anchor.min(offset)..anchor.max(offset);
                    self.update_selection(docs);
                }
            }
            2 => {
                if let Some((px, py, _)) = self.press.take() {
                    if (px - x).abs() < 4.0 && (py - y).abs() < 4.0 {
                        let target = self.document.link_at(x, y).and_then(|link| {
                            self.manual.topics.iter().position(|topic| topic.id == link)
                        });
                        if let Some(index) = target {
                            self.history.open(index);
                            self.navigate(docs);
                        }
                    }
                }
            }
            3 => docs.set_over_link(self.document.link_at(x, y).is_some()),
            _ => self.press = None,
        }
    }
}

/// 在主窗口创建后调用一次：填充目录/索引并把全部回调绑到 HelpDocs 全局。
pub fn bind(app: &crate::MainWindow) {
    let docs = app.global::<HelpDocs>();
    let controller = Rc::new(RefCell::new(Controller::new()));
    {
        let controller = controller.borrow();
        docs.set_contents(model(
            controller
                .manual
                .topics
                .iter()
                .enumerate()
                .map(|(i, topic)| HelpEntry {
                    topic: i as i32,
                    title: topic.title.clone().into(),
                    detail: topic.keywords.join(" / ").into(),
                })
                .collect(),
        ));
        docs.set_keyword_index(model(
            controller
                .manual
                .index
                .iter()
                .map(|entry| HelpEntry {
                    topic: entry.topic as i32,
                    title: entry.keyword.clone().into(),
                    detail: format!(
                        "使用手册 / {}",
                        controller.manual.topics[entry.topic].title
                    )
                    .into(),
                })
                .collect(),
        ));
        controller.search(&docs, "");
    }
    let app_weak = app.as_weak();
    let weak = app_weak.clone();
    let state = Rc::clone(&controller);
    docs.on_topic_opened(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        let mut state = state.borrow_mut();
        if index >= 0 && (index as usize) < state.manual.topics.len() {
            state.history.open(index as usize);
            state.navigate(&docs);
        }
    });
    let weak = app_weak.clone();
    let state = Rc::clone(&controller);
    docs.on_history_requested(move |delta| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        let mut state = state.borrow_mut();
        if state.history.go(delta) {
            state.navigate(&docs);
        }
    });
    let weak = app_weak.clone();
    let state = Rc::clone(&controller);
    docs.on_search_changed(move |query: slint::SharedString| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        state.borrow().search(&docs, &query);
    });
    let weak = app_weak.clone();
    let state = Rc::clone(&controller);
    docs.on_find_changed(move |_: slint::SharedString| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        state.borrow().find(&docs, true);
    });
    let weak = app_weak.clone();
    let state = Rc::clone(&controller);
    docs.on_layout_changed(move || {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        if let Ok(mut state) = state.try_borrow_mut() {
            state.render(&docs);
        }
    });
    let weak = app_weak.clone();
    let state = Rc::clone(&controller);
    docs.on_pointer(move |x, y, kind| {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        state.borrow_mut().pointer(&docs, x, y, kind);
    });
    let weak = app_weak.clone();
    let state = Rc::clone(&controller);
    docs.on_select_all(move || {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        let mut state = state.borrow_mut();
        state.selection = 0..state.document.text.len();
        state.update_selection(&docs);
    });
    let weak = app_weak.clone();
    let state = Rc::clone(&controller);
    docs.on_copy_requested(move || {
        let Some(app) = weak.upgrade() else { return };
        let docs = app.global::<HelpDocs>();
        state.borrow().copy_selection(&docs);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_parses_and_links_resolve() {
        let manual = Manual::load();
        assert!(manual.topics.len() >= 10);
        for entry in &manual.index {
            assert!(entry.topic < manual.topics.len(), "index 引用越界");
        }
        let ids: Vec<&str> = manual.topics.iter().map(|t| t.id.as_str()).collect();
        for topic in &manual.topics {
            for block in &topic.blocks {
                if let Block::Cards { cards } = block {
                    for card in cards {
                        assert!(ids.contains(&card.target.as_str()), "卡片目标不存在");
                    }
                }
            }
        }
    }

    #[test]
    fn typeset_smoke() {
        // 单测进程没有走 main.rs 的启动路径，先注册内嵌字体再排版。
        crate::register_bundled_font().expect("注册内嵌字体");
        let manual = Manual::load();
        let mut typesetter = Typesetter::new();
        let doc = typesetter.layout(&manual.topics[0], 800.0, 1.0, false);
        assert!(!doc.runs.is_empty());
        assert!(doc.height > 100.0);
        // 正文文本应完整进入可复制缓冲。
        assert!(doc.text.contains("本地 AI 工作台"));
        // 查找应命中中文文本。
        assert!(!doc.find("工作台").is_empty());
    }
}
