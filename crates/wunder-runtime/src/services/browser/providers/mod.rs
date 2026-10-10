//! Builtin browser providers.
//!
//! Each provider owns a native tool surface and is registered exclusively into
//! the process-wide registry (see [`super::provider::BrowserUseRegistry`]).

pub mod playwright;

pub use playwright::PlaywrightProvider;