//! OS-level floating overlay windows for companion pets.
//!
//! The desktop build renders Slint with the software backend. Windows presents
//! a pet through a native layered window owned by its own Win32 message-loop
//! thread (GDI drops the alpha channel, so a transparent Slint window is not
//! an option there); Linux uses transparent borderless Slint windows on the
//! shared winit event loop instead (`linux.rs`), where X11 compositors honor
//! per-pixel alpha.

use crate::companion_sprite::Sheet;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

// The types below form the command/event vocabulary shared with the Win32
// overlay backend, which is compiled only on Windows; on other platforms the
// controller still builds them but nothing reads them back.
/// One animation cycle source: the decoded sheet, its row and its tempo.
#[derive(Clone)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct PetAnim {
    pub sheet: Arc<Sheet>,
    pub row: usize,
    pub frames: usize,
    pub frame_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleTone {
    Info,
    Success,
    Warning,
}

#[derive(Clone)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct PetBubble {
    pub text: String,
    pub tone: BubbleTone,
}

/// Everything the overlay draws for one pet. Sizes are logical pixels; the
/// overlay multiplies them by the monitor scale factor itself.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct PetVisual {
    /// Sprite box size in logical pixels (192 x 208 at scale 1).
    pub width: f64,
    pub height: f64,
    /// Animation for the current runtime state. One-shots (the click greeting,
    /// the settle after a drag) are the controller's `anim`, aged out on the UI
    /// thread; the overlay only owns the frame clock.
    pub anim: Option<PetAnim>,
    /// Run cycles used while dragging: `[left, right]`.
    pub drag: [Option<PetAnim>; 2],
    pub bubble: Option<PetBubble>,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub struct PetMenuItem {
    pub code: u16,
    pub label: String,
    pub checked: bool,
    pub separator: bool,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub enum PetCommand {
    /// Create the window, or replace the visual of an existing one.
    Present {
        id: String,
        x: f64,
        y: f64,
        visual: Box<PetVisual>,
    },
    Update {
        id: String,
        visual: Box<PetVisual>,
    },
    /// Reposition without a new visual (clamping after a drag, resize on scale).
    Move {
        id: String,
        x: f64,
        y: f64,
    },
    ShowMenu {
        id: String,
        x: f64,
        y: f64,
        items: Vec<PetMenuItem>,
    },
    /// Hide the window but keep the thread alive for the next pet.
    Dismiss {
        id: String,
    },
    Shutdown,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub enum PetEvent {
    /// Pressed and released without moving.
    Clicked {
        id: String,
    },
    /// Drag finished; `x`/`y` is the new sprite origin in logical pixels.
    Dragged {
        id: String,
        x: f64,
        y: f64,
    },
    RightClicked {
        id: String,
        x: f64,
        y: f64,
    },
    MenuSelected {
        id: String,
        code: u16,
    },
}

thread_local! {
    /// Set while a pet overlay window is being built. The platform hook reads
    /// it once, when the window adapter (and with it the native window
    /// attributes) is created, to mark this window as a taskbar-less,
    /// non-activating overlay instead of an ordinary top-level.
    static OVERLAY_WINDOW_PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Claim the pending pet overlay window for the platform hook. Called from the
/// backend's window-attributes hook, exactly once per created window.
pub(crate) fn take_overlay_window() -> bool {
    OVERLAY_WINDOW_PENDING.with(|pending| pending.replace(false))
}

fn mark_overlay_window() {
    OVERLAY_WINDOW_PENDING.with(|pending| pending.set(true));
}

/// Whether this platform can show an overlay window at all.
pub fn available() -> bool {
    #[cfg(windows)]
    {
        true
    }
    #[cfg(not(windows))]
    {
        #[cfg(target_os = "linux")]
        {
            // Wayland compositors forbid application-side top-level
            // positioning: a pet could not return to its remembered spot or
            // hug the screen edge, so such sessions stay petless.
            !wayland_session()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }
}

/// Whether the winit backend talks to a Wayland compositor. `WINIT_UNIX_BACKEND`
/// keeps its documented meaning: an explicit `x11` overrides the environment.
#[cfg(target_os = "linux")]
pub(crate) fn wayland_session() -> bool {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("WAYLAND_SOCKET").is_some();
    wayland && !matches!(std::env::var("WINIT_UNIX_BACKEND").as_deref(), Ok("x11"))
}

/// Logical work area (x, y, width, height) of the primary monitor.
pub fn work_area() -> Option<(f64, f64, f64, f64)> {
    #[cfg(windows)]
    {
        win32::work_area()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub struct PetService {
    #[cfg(windows)]
    thread_id: u32,
    /// Linux overlay windows live on the shared winit event loop, so the
    /// controller is applied directly on the (UI) thread that sends commands.
    #[cfg(not(windows))]
    controller: Rc<RefCell<linux::Controller>>,
}

impl PetService {
    /// Start the overlay thread. Returns `None` when the platform has no
    /// overlay support or the thread could not be created.
    pub fn start() -> Option<(PetService, Receiver<PetEvent>)> {
        #[cfg(windows)]
        {
            win32::spawn()
        }
        #[cfg(not(windows))]
        {
            linux::spawn()
        }
    }

    pub fn send(&self, command: PetCommand) {
        #[cfg(windows)]
        {
            win32::post(self.thread_id, command);
        }
        #[cfg(not(windows))]
        {
            self.controller.borrow_mut().handle(command);
        }
    }
}

impl Drop for PetService {
    fn drop(&mut self) {
        // Releasing the controller must not leave overlay windows on screen.
        self.send(PetCommand::Shutdown);
    }
}

#[cfg(windows)]
mod win32;
#[cfg(not(windows))]
mod linux;
