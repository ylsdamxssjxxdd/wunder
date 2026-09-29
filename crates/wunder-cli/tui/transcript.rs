//! Canonical transcript cell shared by streaming updates and rendering.
//!
//! Structured tool output and width-dependent Markdown caches stay with the
//! cell. Thread switching moves these cells without cloning their text or
//! maintaining a second, stale text-only projection.

pub(crate) const MAX_TRANSCRIPT_CELLS: usize = 1200;
pub(crate) const MAX_TRANSCRIPT_CHARS: usize = 320_000;

#[derive(Debug, Clone)]
pub(crate) struct TranscriptCell<Kind, Special, Cache> {
    pub kind: Kind,
    pub text: String,
    pub special: Option<Special>,
    pub markdown_cache: Option<Cache>,
}
