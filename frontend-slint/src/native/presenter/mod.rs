//! Shared native CPU canvas for echo, training world and equipment scenes.
//!
//! Slint remains the owner of the workbench UI.  This module only owns the
//! small rectangle that displays the already-rasterized 3D frame.  Keeping
//! that rectangle outside Slint's `Image` item avoids a second full-frame
//! upload, resample, and composition on every camera gesture.

mod fallback;

#[cfg(windows)]
mod win32;

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
mod x11;

pub use fallback::{GestureCallback, PresenterFrame};

#[cfg(windows)]
pub use win32::ModelPresenter;

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
pub use x11::ModelPresenter;

#[cfg(any(not(any(windows, all(target_os = "linux", not(target_arch = "wasm32"))))))]
pub use fallback::ModelPresenter;

