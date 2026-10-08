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
    /// Durable item identity behind this cell. Only durable replay stamps it, so
    /// a later revision of the same item folds into the cell it already wrote
    /// instead of appending a second copy.
    pub durable_id: Option<String>,
}
