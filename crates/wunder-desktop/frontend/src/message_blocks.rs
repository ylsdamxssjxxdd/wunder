//! Bounded native Markdown projection.
//!
//! Streaming output stays plain text and is published as a frozen prefix of
//! completed lines plus one active tail block, so a growing answer costs
//! O(tail) per frame instead of laying out the whole message. Markdown parsing
//! runs **once**, when the model response reaches a terminal state: a 5k-block
//! answer projects in ~13 ms, and re-projecting it on every stream tick would
//! cost more than the model takes to emit the next token.
//!
//! The projected rows are the flat `MdBlock` model of `src/md`, rendered by
//! `ui/markdown-rich.slint`.
use crate::md::block as md;
use crate::md::MarkdownRenderer;
use crate::MdBlock;
use slint::{Model, ModelRc, StyledText, VecModel};
use std::rc::Rc;

const DISPLAY_BYTES: usize = 16 * 1024;
const DISPLAY_LINES: usize = 160;
/// Completed lines are frozen into their own block once the live tail passes
/// this budget; only the tail is re-laid-out on the following frame.
const TAIL_BYTES: usize = 2 * 1024;
/// Upper bound for one frozen block.
const FROZEN_CHUNK_BYTES: usize = 4 * 1024;
/// The display window is re-based in whole steps: past the budget its start
/// moves a few bytes per frame, and rebuilding the prefix on every one of
/// those would cost more than the tail it is meant to bound.
const REBASE_SLACK: usize = 4 * 1024;
/// Shown at the head once the display window drops older lines; streaming and
/// the settled view use the same words so the settle step never reflows it.
const TRUNCATED_NOTICE: &str = "仅展示最近 16 KiB 或 160 行；点击气泡下方“复制”获取完整原文。";
pub const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const KIND_IMAGE: i32 = md::IMAGE;

pub struct Blocks {
    pub raw: String,
    pub model: Rc<VecModel<MdBlock>>,
    renderer: MarkdownRenderer,
    rendered: bool,
    /// Byte offset in `raw` where the published window starts. It only moves
    /// forward when the reply outgrows the display budget.
    window: usize,
    /// Byte offset in `raw` before which text is frozen into completed blocks.
    frozen: usize,
    /// Index of the live tail block, which is always the model's last row.
    tail: Option<usize>,
}

impl Default for Blocks {
    fn default() -> Self {
        Self::new()
    }
}

impl Blocks {
    pub fn new() -> Self {
        Self {
            raw: String::new(),
            model: Rc::new(VecModel::default()),
            renderer: MarkdownRenderer::new(),
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
        let mut blocks = self.renderer.render(source);
        if head_capped {
            blocks.insert(0, notice_block(TRUNCATED_NOTICE));
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
            self.model.push(notice_block(TRUNCATED_NOTICE));
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
            self.model.insert(at + inserted, plain_block(text));
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
            Some(index) => self.model.set_row_data(index, plain_block(text)),
            None if !text.is_empty() => {
                let index = self.model.row_count();
                self.model.push(plain_block(text));
                self.tail = Some(index);
            }
            None => {}
        }
    }
}

/// Parse a finished message straight into a ready-to-render row model.
pub fn from_text(text: &str) -> ModelRc<MdBlock> {
    let mut blocks = Blocks::new();
    blocks.raw = text.to_owned();
    blocks.finalize_markdown();
    ModelRc::from(blocks.model)
}

/// Project a whole document without the streaming display-window cap.
///
/// A chat answer over 16 KiB only shows its newest lines, because the live
/// transcript is a tail view. A static document (the help manual) is not a
/// stream: it must be rendered in full, so it bypasses `Blocks` entirely.
pub fn render_full(source: &str) -> Vec<MdBlock> {
    MarkdownRenderer::new().render(source)
}

/// The user bubble's local echo: the message text plus one image card per
/// composer attachment, sourced from its data URL. The durable projection
/// replaces these cards with the stored paths once the turn publishes.
pub fn with_image_cards(text: &str, images: &[(String, String)]) -> ModelRc<MdBlock> {
    let blocks = from_text(text);
    let Some(model) = blocks.as_any().downcast_ref::<VecModel<MdBlock>>() else {
        return blocks;
    };
    for (name, source) in images {
        model.push(image_card(name, source));
    }
    blocks
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

/// A streaming placeholder: one plain paragraph, never parsed as Markdown.
fn plain_block(text: &str) -> MdBlock {
    MdBlock {
        text: StyledText::from_plain_text(text),
        plain: text.into(),
        ..blank(md::PARAGRAPH)
    }
}

fn notice_block(text: &str) -> MdBlock {
    MdBlock {
        text: StyledText::from_plain_text(text),
        plain: text.into(),
        ..blank(md::PARAGRAPH)
    }
}

fn image_card(alt: &str, source: &str) -> MdBlock {
    let label = if alt.trim().is_empty() {
        crate::md::source::file_name(source, "图片")
    } else {
        alt.to_string()
    };
    MdBlock {
        image_source: source.into(),
        plain: label.as_str().into(),
        ..blank(md::IMAGE)
    }
}

/// Every field spelled out once: a new field on `MdBlock` then fails here
/// instead of silently defaulting at each call site.
fn blank(kind: i32) -> MdBlock {
    MdBlock {
        kind,
        level: 0,
        text: StyledText::default(),
        plain: slint::SharedString::default(),
        language: slint::SharedString::default(),
        code: slint::SharedString::default(),
        code_text: StyledText::default(),
        depth: 0,
        marker: slint::SharedString::default(),
        task: 0,
        quoted: false,
        columns: 0,
        widths: ModelRc::default(),
        cells: ModelRc::default(),
        column_widths: ModelRc::default(),
        table_width: 0.0,
        image: slint::Image::default(),
        image_source: slint::SharedString::default(),
        image_width: 0.0,
        image_height: 0.0,
        image_error: false,
        image_frames: ModelRc::default(),
        frame_delays: ModelRc::default(),
        resource_name: slint::SharedString::default(),
        space_before: 0.0,
    }
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
            .map(|block| block.plain.to_string())
            .collect()
    }

    fn kinds(blocks: &Blocks) -> Vec<i32> {
        blocks.model.iter().map(|block| block.kind).collect()
    }

    #[test]
    fn streaming_freezes_completed_lines_and_reconstructs_the_reply() {
        let mut blocks = Blocks::new();
        let mut source = String::new();
        let mut frozen: Vec<String> = Vec::new();
        for index in 0..40 {
            let line = format!(
                "第 {index} 行，用于撑满尾块预算的示例文本{}\n",
                "·".repeat(60)
            );
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
        assert!(
            !window.contains("第 0 行"),
            "the head leaves the display budget"
        );
        assert!(
            window.contains("第 299 行"),
            "the newest line stays visible"
        );
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
    fn streaming_publishes_plain_paragraphs_only() {
        let source = "# 标题\n\n**粗体** 段落\n\n```rust\nlet value = 1;\n```\n";
        let mut blocks = Blocks::new();
        for line in source.split_inclusive('\n') {
            blocks.append(line).unwrap();
            blocks.flush();
        }
        assert!(
            kinds(&blocks).iter().all(|kind| *kind == md::PARAGRAPH),
            "streaming must not parse Markdown: {:?}",
            kinds(&blocks)
        );
    }

    #[test]
    fn finalizing_projects_the_document_once() {
        let source = "# 标题\n\n**粗体** 段落\n\n```rust\nlet value = 1;\n```\n";
        let mut blocks = Blocks::new();
        for line in source.split_inclusive('\n') {
            blocks.append(line).unwrap();
            blocks.flush();
        }
        blocks.finalize_markdown();
        let kinds = kinds(&blocks);
        assert!(kinds.contains(&md::HEADING));
        assert!(kinds.contains(&md::CODE));
        assert_eq!(blocks.raw, source);
        // A second finalize is a no-op, not a re-parse.
        let before = blocks.model.row_count();
        blocks.finalize_markdown();
        assert_eq!(blocks.model.row_count(), before);
    }

    #[test]
    fn final_render_supports_common_elements() {
        let source = "# 标题\n\n段落含 **粗体**、*斜体*、`代码` 和 [链接](https://example.invalid)。\n\n> 引用\n\n- 项目一\n1. 项目二\n\n```rust\nlet value = 1;\n```\n\n| 名称 | 数值 |\n| --- | ---: |\n| A | 1 |\n\n---\n\n![说明](image.png)";
        let mut blocks = Blocks::new();
        for ch in source.chars() {
            blocks.append(&ch.to_string()).unwrap();
            blocks.flush();
        }
        blocks.finalize_markdown();
        let kinds = kinds(&blocks);
        for expected in [
            md::HEADING,
            md::CODE,
            md::TABLE,
            md::RULE,
            md::IMAGE,
            md::LIST_ITEM,
        ] {
            assert!(
                kinds.contains(&expected),
                "missing kind {expected}: {kinds:?}"
            );
        }
        assert!(blocks.raw == source);
        assert!(
            blocks.model.iter().any(|block| block.quoted),
            "a quote child is flagged"
        );
    }

    #[test]
    fn code_rows_copy_only_code_without_the_fence_language() {
        let mut blocks = Blocks::new();
        blocks.append("```rust\nlet value = 1;\n```").unwrap();
        blocks.finalize_markdown();
        let code = blocks
            .model
            .iter()
            .find(|block| block.kind == md::CODE)
            .expect("code row");
        assert_eq!(code.code, "let value = 1;");
        assert_eq!(code.language, "rust");
    }

    #[test]
    fn local_document_links_become_cards_and_remote_links_stay_text() {
        let mut blocks = Blocks::new();
        blocks
            .append("之前 [**示例文档**](documents/example.pdf) 之后\n\n[远程](https://example.invalid/example.pdf)")
            .unwrap();
        blocks.finalize_markdown();
        let rows = block_texts(&blocks);
        // The space that separated the prose from the link stays in the row:
        // the projection is faithful, and a trailing space is invisible.
        assert_eq!(rows[0].trim(), "之前");
        let card = blocks
            .model
            .iter()
            .find(|block| block.kind == md::DOCUMENT)
            .expect("document card");
        assert_eq!(card.plain, "示例文档");
        assert_eq!(card.image_source, "documents/example.pdf");
        assert!(
            blocks
                .model
                .iter()
                .any(|block| block.plain.contains("https://example.invalid/example.pdf")),
            "a remote link keeps its target in the text"
        );
    }

    #[test]
    fn public_workspace_paths_are_kept_as_local_resource_cards() {
        let mut blocks = Blocks::new();
        blocks
            .append(
                "![图片](/workspaces/scope/workspace/image.png)\n\n[文档](/workspaces/scope/workspace/report.docx)",
            )
            .unwrap();
        blocks.finalize_markdown();
        let image = blocks
            .model
            .iter()
            .find(|block| block.kind == md::IMAGE)
            .expect("image card");
        assert_eq!(image.image_source, "/workspaces/scope/workspace/image.png");
        let document = blocks
            .model
            .iter()
            .find(|block| block.kind == md::DOCUMENT)
            .expect("document card");
        assert_eq!(
            document.image_source,
            "/workspaces/scope/workspace/report.docx"
        );
    }

    #[test]
    fn with_image_cards_appends_local_echo_cards_after_the_text() {
        let model = with_image_cards(
            "Fixture question",
            &[("shot.png".into(), "data:image/png;base64,AAAA".into())],
        );
        let kinds = (0..model.row_count())
            .filter_map(|index| model.row_data(index))
            .map(|block| block.kind)
            .collect::<Vec<_>>();
        assert_eq!(kinds.last(), Some(&md::IMAGE));
        let card = model.row_data(model.row_count() - 1).unwrap();
        assert_eq!(card.plain, "shot.png");
        assert_eq!(card.image_source, "data:image/png;base64,AAAA");
        assert_eq!(card.image.size().width, 0);
    }

    #[test]
    fn library_files_use_the_same_card_as_workspace_links() {
        let mut blocks = Blocks::new();
        blocks
            .append("![交付文档](/workspaces/scope/workspace/report.docx)")
            .unwrap();
        blocks.finalize_markdown();
        let card = blocks
            .model
            .iter()
            .find(|block| block.kind == md::DOCUMENT)
            .expect("document card");
        assert_eq!(card.plain, "交付文档");
        assert!(
            card.image.to_rgba8().is_some(),
            "the file icon is filled in"
        );
    }
}
