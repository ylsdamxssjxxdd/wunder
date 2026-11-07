//! Builtin browser providers.
//!
//! Each provider owns a native tool surface and is registered exclusively into
//! the process-wide registry (see [`super::provider::BrowserUseRegistry`]).

pub mod playwright;
pub mod webview;

pub use playwright::PlaywrightProvider;
pub use webview::{install_webview_host, webview_available, WebViewHost, WebViewProvider};
