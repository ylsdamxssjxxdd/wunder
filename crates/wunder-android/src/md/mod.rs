//! Markdown rendering for the chat transcript.
//!
//! Slint's `StyledText` is an inline primitive: it parses a CommonMark subset
//! and rejects headings, lists-as-structure, code blocks, tables, quotes and
//! images, and it exposes no block spacing or per-span font size. Rendering a
//! document therefore needs two layers:
//!
//! | layer | owner | responsibility |
//! |---|---|---|
//! | block | `ui/markdown-rich.slint` + `project.rs` | kind, size, spacing, nesting, table columns |
//! | inline | `StyledText` via `inline.rs` | emphasis, inline code, links, colour |
//!
//! Parsing is deliberately *not* incremental. The chat publishes live output
//! as plain text and calls [`project::MarkdownRenderer::render`] once, when the
//! turn reaches a terminal state: a 300-block answer costs ~13 ms to project,
//! while re-projecting it on every stream tick would cost more than the model
//! takes to emit the next token.

pub mod ast;
pub mod block;
pub mod inline;
pub mod parse;
pub mod project;
pub mod source;

pub use project::MarkdownRenderer;

/// Decode workspace image bytes into a bounded RGBA thumbnail. The transcript
/// never needs full resolution, and a phone photo would otherwise cost tens of
/// MB of texture per message.
pub fn decode_thumbnail(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(640, 400).to_rgba8();
    Some((image.width(), image.height(), image.into_raw()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn thumbnails_keep_pixels_and_reject_garbage() {
        let image = super::decode_thumbnail(include_bytes!(
            "../../../wunder-desktop/frontend/assets/markdown-example.png"
        ))
        .expect("decode the sample image");
        assert_eq!((image.0, image.1), (640, 280));
        assert_eq!(image.2.len(), 640 * 280 * 4);
        assert!(super::decode_thumbnail(b"not an image").is_none());
    }
}
