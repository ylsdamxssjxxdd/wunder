//! Document model for the renderer.
//!
//! The tree is deliberately small and owned: the Slint side never sees it.
//! `project` flattens this into a row model, which is why nesting here can be
//! free (`Quote(Vec<Block>)`, nested `ListItem::blocks`) while the UI stays a
//! single flat, cheap-to-clone list.

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        inline: Vec<Inline>,
    },
    Code {
        language: String,
        text: String,
    },
    Quote(Vec<Block>),
    List(List),
    Table(Table),
    Rule,
    Image(ImageRef),
    /// A link or image that points at a workspace document. The chat shell
    /// renders it as a file card with the workspace icon instead of an inline
    /// link, because opening the file is the only thing the user can do with
    /// it.
    Document(ImageRef),
}

#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub ordered: bool,
    pub start: u64,
    pub items: Vec<ListItem>,
}

/// One list item. The first paragraph is lifted into `inline` so a tight list
/// stays a single row; everything else (nested list, code, quote) lands in
/// `blocks` and is projected as following rows at `depth + 1`.
#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    pub inline: Vec<Inline>,
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub align: Vec<Align>,
    pub head: Vec<Vec<Inline>>,
    pub rows: Vec<Vec<Vec<Inline>>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    None,
    Left,
    Center,
    Right,
}

impl Align {
    pub fn from_cmark(align: pulldown_cmark::Alignment) -> Self {
        match align {
            pulldown_cmark::Alignment::Left => Align::Left,
            pulldown_cmark::Alignment::Center => Align::Center,
            pulldown_cmark::Alignment::Right => Align::Right,
            pulldown_cmark::Alignment::None => Align::None,
        }
    }

    /// 0 start, 1 center, 2 end — the Slint side's enum.
    pub fn code(self) -> i32 {
        match self {
            Align::Center => 1,
            Align::Right => 2,
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageRef {
    pub source: String,
    pub title: String,
    pub alt: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Code(String),
    SoftBreak,
    HardBreak,
    Strong(Vec<Inline>),
    Emphasis(Vec<Inline>),
    Strike(Vec<Inline>),
    Link {
        url: String,
        children: Vec<Inline>,
    },
}

impl Inline {
    fn is_blank(&self) -> bool {
        match self {
            Inline::Text(text) => text.trim().is_empty(),
            Inline::SoftBreak | Inline::HardBreak => true,
            _ => false,
        }
    }
}

/// True when a run carries nothing worth laying out (a paragraph that only
/// held whitespace, an empty list item produced by a stray marker).
pub fn is_blank(nodes: &[Inline]) -> bool {
    nodes.iter().all(Inline::is_blank)
}
