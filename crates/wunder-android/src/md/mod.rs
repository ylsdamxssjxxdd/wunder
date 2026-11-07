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
