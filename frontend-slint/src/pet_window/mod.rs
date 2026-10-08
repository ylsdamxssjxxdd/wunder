//! OS-level floating overlay windows for companion pets.
//!
//! The desktop build renders Slint with the software backend, which cannot
//! present a per-pixel-alpha window, so a pet is a native layered window owned
//! by its own Win32 message-loop thread. That thread also drives the frame
//! clock and the drag, so an idle chat costs the UI thread a handful of commands
//! per interaction instead of a per-frame copy.

use crate::companion_sprite::Sheet;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

/// One animation cycle source: the decoded sheet, its row and its tempo.
#[derive(Clone)]
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
pub struct PetBubble {
    pub text: String,
    pub tone: BubbleTone,
}

/// Everything the overlay draws for one pet. Sizes are logical pixels; the
/// overlay multiplies them by the monitor scale factor itself.
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

pub struct PetMenuItem {
    pub code: u16,
    pub label: String,
    pub checked: bool,
    pub separator: bool,
}

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
    Move { id: String, x: f64, y: f64 },
    ShowMenu {
        id: String,
        x: f64,
        y: f64,
        items: Vec<PetMenuItem>,
    },
    /// Hide the window but keep the thread alive for the next pet.
    Dismiss { id: String },
    Shutdown,
}

pub enum PetEvent {
    /// Pressed and released without moving.
    Clicked { id: String },
    /// Drag finished; `x`/`y` is the new sprite origin in logical pixels.
    Dragged { id: String, x: f64, y: f64 },
    RightClicked { id: String, x: f64, y: f64 },
    MenuSelected { id: String, code: u16 },
}

/// Whether this platform can show an overlay window at all.
pub fn available() -> bool {
    cfg!(windows)
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
            None
        }
    }

    pub fn send(&self, command: PetCommand) {
        #[cfg(windows)]
        {
            win32::post(self.thread_id, command);
        }
        #[cfg(not(windows))]
        {
            let _ = command;
        }
    }
}

impl Drop for PetService {
    fn drop(&mut self) {
        // Releasing the controller must not leave layered windows on screen.
        self.send(PetCommand::Shutdown);
    }
}

#[cfg(windows)]
mod win32;
