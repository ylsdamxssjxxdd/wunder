//! Keep the workbench window where the user left it.
//!
//! The saved frame is read before the engine starts, so a relaunch reopens at
//! the same place instead of re-centering. Moves and resizes are folded into
//! the store after a short idle period; the maximized flag is tracked apart
//! from the normal frame, so unmaximizing after a restart still lands on the
//! size the user actually used.

use crate::MainWindow;
use slint::ComponentHandle;
use slint::winit_030::{winit, WinitWindowAccessor};
use std::cell::Cell;
use std::time::Duration;
pub use wunder_desktop::window_state::WindowGeometry;
use wunder_desktop::window_state;

/// Moves and resizes arrive per frame while dragging; writing each one would
/// keep the placement file churning for the whole gesture.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

thread_local! {
    /// Probes that never call [`install`] keep the pure fit-and-center
    /// behavior and never touch the user's saved placement.
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static SAVED: Cell<Option<WindowGeometry>> = const { Cell::new(None) };
    /// Normal (non-maximized) frame last seen, so maximizing and quitting
    /// still leaves the size the user can restore to.
    static LAST_NORMAL: Cell<Option<WindowGeometry>> = const { Cell::new(None) };
    static SAVE_TIMER: slint::Timer = slint::Timer::default();
}

/// Start tracking the workbench window; must run before the window is shown.
pub fn install() {
    let loaded = window_state::load();
    ENABLED.with(|enabled| enabled.set(true));
    SAVED.with(|saved| saved.set(loaded));
    LAST_NORMAL.with(|normal| normal.set(loaded));
}

/// Placement restored at startup, if the last session left one.
pub fn initial_geometry() -> Option<WindowGeometry> {
    if !ENABLED.with(Cell::get) {
        return None;
    }
    SAVED.with(Cell::get)
}

/// Called from the window event filter for every winit event. Slint keeps a
/// single filter slot per window, so the one installed in `main` forwards
/// every event here instead of this module installing its own.
pub fn observe(app: &MainWindow, event: &winit::event::WindowEvent) {
    if !ENABLED.with(Cell::get) {
        return;
    }
    match event {
        winit::event::WindowEvent::Moved(_)
        | winit::event::WindowEvent::Resized(_)
        | winit::event::WindowEvent::ScaleFactorChanged { .. } => schedule(app),
        winit::event::WindowEvent::CloseRequested => save_now(app),
        _ => {}
    }
}

fn schedule(app: &MainWindow) {
    let weak = app.as_weak();
    SAVE_TIMER.with(|timer| {
        timer.start(slint::TimerMode::SingleShot, SAVE_DEBOUNCE, move || {
            if let Some(app) = weak.upgrade() {
                save_now(&app);
            }
        });
    });
}

/// Write the current placement immediately (window closing / quitting).
pub fn save_now(app: &MainWindow) {
    if !ENABLED.with(Cell::get) {
        return;
    }
    SAVE_TIMER.with(slint::Timer::stop);
    let Some(frame) = current_frame(app) else {
        return;
    };
    // While minimized the position belongs to the window manager's iconified
    // state; the frame saved before minimizing already describes the window.
    if frame.minimized {
        return;
    }
    let placement = if frame.maximized {
        // A maximized frame reports the screen rectangle; keep the last normal
        // frame and only flip the flag.
        let normal = LAST_NORMAL.with(Cell::get).unwrap_or(WindowGeometry {
            x: frame.x,
            y: frame.y,
            width: 0,
            height: 0,
            maximized: false,
        });
        WindowGeometry {
            maximized: true,
            ..normal
        }
    } else {
        let normal = WindowGeometry {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
            maximized: false,
        };
        LAST_NORMAL.with(|slot| slot.set(Some(normal)));
        normal
    };
    if SAVED.with(Cell::get) == Some(placement) {
        return;
    }
    SAVED.with(|slot| slot.set(Some(placement)));
    window_state::save(&placement);
}

struct Frame {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
    minimized: bool,
}

fn current_frame(app: &MainWindow) -> Option<Frame> {
    app.window()
        .with_winit_window(|native| {
            let position = native.outer_position().ok()?;
            let size = native.inner_size();
            Some(Frame {
                x: position.x,
                y: position.y,
                width: size.width,
                height: size.height,
                maximized: native.is_maximized(),
                minimized: native.is_minimized().unwrap_or(false),
            })
        })
        .flatten()
}