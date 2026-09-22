// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! On-demand frame capture for the rcho workbench screenshot feature.
//!
//! A screenshot request arms a one-shot capture; the next presented frame of
//! the matching window is copied out of the software surface as opaque RGBA8.
//! Arming is a pair of atomic stores, so presented frames are copied only
//! while a capture is pending and the normal render path stays untouched.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// A single presented frame copied out of the software surface.
pub struct CapturedFrame {
    /// Frame width in physical pixels.
    pub width: u32,
    /// Frame height in physical pixels.
    pub height: u32,
    /// RGBA8, row-major, top-down, alpha forced opaque.
    pub rgba: Vec<u8>,
}

static ARMED: AtomicBool = AtomicBool::new(false);
static TARGET_WINDOW: AtomicU64 = AtomicU64::new(0);
static FRAME: OnceLock<Mutex<Option<CapturedFrame>>> = OnceLock::new();

/// Arms the one-shot capture for the given winit window id. Called on the
/// Slint event-loop thread when a screenshot is requested; any stale frame
/// from a previous (timed-out) capture is dropped.
pub fn arm(window_id: u64) {
    if let Ok(mut slot) = FRAME.get_or_init(|| Mutex::new(None)).lock() {
        *slot = None;
    }
    TARGET_WINDOW.store(window_id, Ordering::Release);
    ARMED.store(true, Ordering::Release);
}

/// Disarms a pending capture without taking a frame (timeout path).
pub fn disarm() {
    ARMED.store(false, Ordering::Release);
}

/// Takes the captured frame, if one has arrived since `arm`.
pub fn take() -> Option<CapturedFrame> {
    FRAME.get()?.lock().ok()?.take()
}

/// Called by the software renderer after a successful present. Runs on the
/// event-loop thread; the copy happens only while a capture is armed for this
/// window, and the buffer always holds the complete current frame.
pub(crate) fn maybe_capture(window_id: u64, width: u32, height: u32, pixels: &[u32]) {
    if !ARMED.load(Ordering::Acquire) || TARGET_WINDOW.load(Ordering::Acquire) != window_id {
        return;
    }
    if pixels.len() != width as usize * height as usize {
        return;
    }
    ARMED.store(false, Ordering::Release);
    // Softbuffer pixels are 0xAARRGGBB; expand to RGBA8 with opaque alpha.
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for pixel in pixels {
        rgba.push((pixel >> 16) as u8);
        rgba.push((pixel >> 8) as u8);
        rgba.push(*pixel as u8);
        rgba.push(0xff);
    }
    if let Some(slot) = FRAME.get() {
        if let Ok(mut guard) = slot.lock() {
            *guard = Some(CapturedFrame { width, height, rgba });
        }
    }
}
