//! Block kinds as seen by `ui/markdown-rich.slint`.
//!
//! Kept as `i32` rather than a Slint enum on purpose: the row stays a flat
//! struct that is cheap to clone, and the host can add a kind without matching
//! exhaustively. Must stay in sync with the `BlockKind` global in
//! `ui/markdown-rich.slint`.

pub const PARAGRAPH: i32 = 0;
pub const HEADING: i32 = 1;
pub const CODE: i32 = 2;
pub const LIST_ITEM: i32 = 3;
pub const TABLE: i32 = 4;
pub const IMAGE: i32 = 5;
pub const RULE: i32 = 6;
/// Workspace file card. Not produced by CommonMark: the parser promotes a link
/// or image whose target is a workspace document into this kind.
pub const DOCUMENT: i32 = 7;

