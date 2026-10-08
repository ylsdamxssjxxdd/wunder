// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! First-presented-frame notification for the rcho workbench.
//!
//! `Window::set_rendering_notifier` is unsupported by the software renderer,
//! so the backend reports the first successful present here. Application code
//! uses it to defer heavyweight startup services (recent-directory restore,
//! decode warm-up) until the window is actually on screen. All entry points
//! must run on the Slint event-loop thread; the software renderer presents on
//! that same thread.

use std::cell::{Cell, RefCell};

thread_local! {
    static PRESENTED: Cell<bool> = const { Cell::new(false) };
    static OBSERVERS: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}

/// Runs `observer` once the first frame has actually been presented, or
/// immediately when a frame was already presented. Must be called from the
/// Slint event-loop thread; the observer runs on that same thread.
pub fn on_first_frame_presented(observer: impl FnOnce() + 'static) {
    let already = PRESENTED.with(|presented| presented.get());
    if already {
        observer();
        return;
    }
    OBSERVERS.with(|observers| observers.borrow_mut().push(Box::new(observer)));
}

/// The software renderer reports every successful present here; only the
/// first one latched fires the pending observers.
pub(crate) fn notify_presented() {
    let first = PRESENTED.with(|presented| !presented.replace(true));
    if !first {
        return;
    }
    let observers = OBSERVERS.with(|observers| std::mem::take(&mut *observers.borrow_mut()));
    for observer in observers {
        observer();
    }
}
