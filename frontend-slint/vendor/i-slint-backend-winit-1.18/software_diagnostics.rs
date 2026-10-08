// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Opt-in, bounded frame samples for the rcho CPU presentation diagnostic.
//! No file I/O or additional redraws take place on the window thread.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// One attempt to draw and present a software window.
#[derive(Debug)]
pub struct FrameSample {
    /// Process-relative completion timestamp, in milliseconds.
    pub at_ms: f64,
    /// Winit window identity (process-local).
    pub window_id: u64,
    /// Physical surface dimensions.
    pub width: u32,
    /// Physical surface height.
    pub height: u32,
    /// Logical-to-physical scale factor.
    pub scale: f32,
    /// Surface acquisition/resize time, in milliseconds.
    pub prepare_ms: f64,
    /// Software render time (including layout/text work it evaluates).
    pub render_ms: f64,
    /// Native present call duration; not the time a monitor displays the frame.
    pub present_ms: f64,
    /// Bounding box pixels submitted to the native surface.
    pub damage_pixels: u64,
    /// Whether an embedded popup is visible (not necessarily a full repaint).
    pub popup: bool,
    /// Whether presentation returned an error.
    pub failed: bool,
}

/// One windowing event that can schedule or skip frames (resize, move, focus,
/// occlusion, native ShowWindow). `a`/`b` carry per-kind payload (e.g. the new
/// width/height for `resized`); `ms` is an optional measured duration.
#[derive(Debug)]
pub struct EventSample {
    /// Process-relative timestamp, in milliseconds.
    pub at_ms: f64,
    /// Winit window identity (process-local); 0 when unrelated to one window.
    pub window_id: u64,
    /// Stable lowercase tag, e.g. `resized`, `moved`, `focused`, `showwindow`.
    pub kind: String,
    /// Per-kind payload (e.g. new width for `resized`, focus flag for `focused`).
    pub a: i64,
    /// Per-kind secondary payload (e.g. new height for `resized`).
    pub b: i64,
    /// Measured blocking duration of the event, in milliseconds; 0 when untimed.
    pub ms: f64,
}

struct Samples {
    started: Instant,
    frames: Mutex<(Vec<FrameSample>, u64)>,
    events: Mutex<(Vec<EventSample>, u64)>,
}

fn samples() -> Option<&'static Samples> {
    static SAMPLES: OnceLock<Option<Samples>> = OnceLock::new();
    SAMPLES.get_or_init(|| {
        std::env::var_os("RCHO_SLINT_DIAGNOSTICS").filter(|v| !v.is_empty()).map(|_| Samples {
            started: Instant::now(),
            frames: Mutex::new((Vec::new(), 0)),
            events: Mutex::new((Vec::new(), 0)),
        })
    }).as_ref()
}

pub(crate) fn frame_start() -> Option<Instant> {
    samples().map(|_| Instant::now())
}

pub(crate) fn record(mut frame: FrameSample) {
    if let Some(samples) = samples() {
        frame.at_ms = samples.started.elapsed().as_secs_f64() * 1000.0;
        if let Ok(mut frames) = samples.frames.lock() {
            if frames.0.len() < 4096 {
                frames.0.push(frame);
            } else {
                frames.1 += 1;
            }
        }
    }
}

/// Drain pending frames on the diagnostic worker, reporting any dropped samples.
pub fn take_samples() -> (Vec<FrameSample>, u64) {
    samples().and_then(|s| s.frames.lock().ok().map(|mut f| std::mem::take(&mut *f)))
        .unwrap_or_default()
}

/// Record one windowing event. No-op unless diagnostics are enabled, so the
/// window thread can call it unconditionally on hot paths.
pub fn record_event(window_id: u64, kind: &str, a: i64, b: i64, ms: f64) {
    if let Some(samples) = samples() {
        let event = EventSample {
            at_ms: samples.started.elapsed().as_secs_f64() * 1000.0,
            window_id,
            kind: kind.to_string(),
            a,
            b,
            ms,
        };
        if let Ok(mut events) = samples.events.lock() {
            if events.0.len() < 4096 {
                events.0.push(event);
            } else {
                events.1 += 1;
            }
        }
    }
}

/// Drain pending windowing events on the diagnostic worker.
pub fn take_events() -> (Vec<EventSample>, u64) {
    samples().and_then(|s| s.events.lock().ok().map(|mut e| std::mem::take(&mut *e)))
        .unwrap_or_default()
}
