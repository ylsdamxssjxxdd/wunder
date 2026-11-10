//! Linux pet overlay, the counterpart of `win32.rs`: instead of a native
//! layered window, each pet is a transparent borderless Slint window on the
//! shared winit event loop. X11 gives it a 32-bit ARGB visual, so the software
//! renderer's alpha reaches the compositor and the pet floats without a
//! background; the window is mapped as a Utility so it stays out of the
//! taskbar. Everything runs on the UI thread — commands arrive already there
//! via `PetService::send`, an animation tick re-blits one 192x208 frame, and
//! native events travel back through the same mpsc channel win32 uses.
//!
//! Wayland sessions never reach this module: `available()` reports false
//! because the compositor forbids top-level positioning, so a pet could not
//! return to its remembered spot.

use super::{mark_overlay_window, BubbleTone, PetAnim, PetBubble, PetCommand, PetEvent, PetVisual};
use crate::{companion_sprite, PetOverlay};
use slint::{ComponentHandle, Image, SharedPixelBuffer};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// Edge margin the pet keeps from the monitor edge, matching the projection's
/// clamp semantics (`companion_pet::SCREEN_MARGIN`).
const SCREEN_MARGIN: f64 = 8.0;
/// Widest the bubble strip makes the window: the text wrap width in
/// `pet-overlay.slint` (220px) plus the card's side padding.
const BUBBLE_WINDOW_WIDTH: f64 = 236.0;
/// A drag shorter than this is a click, mirroring the window-chrome threshold.
const CLICK_THRESHOLD_PX: f32 = 4.0;
/// Menu codes, kept in sync with `companion_pet::MENU_*`.
const MENU_OPEN_CHAT: u16 = 1;
const MENU_TOGGLE_SHOW: u16 = 2;

type ArcSheet = Arc<companion_sprite::Sheet>;

/// One floating pet: the Slint window plus its frame clock.
struct PetWindow {
    comp: PetOverlay,
    timer: slint::Timer,
    /// Animation currently loaded, so an unchanged tick is silent: the sheet
    /// identity plus its row and frame count.
    playing: Option<(ArcSheet, usize, usize)>,
    /// Logical position of the sprite box (its top-left), i.e. the `x`/`y`
    /// companion_pet persists. The window grows upward around it for bubbles.
    sprite_x: f64,
    sprite_y: f64,
}

pub(super) struct Controller {
    windows: HashMap<String, PetWindow>,
    events: Sender<PetEvent>,
}

/// Start the UI-thread controller. Commands sent later are applied directly;
/// events come back through the returned receiver.
pub(super) fn spawn() -> Option<(super::PetService, Receiver<PetEvent>)> {
    let (sender, receiver) = channel();
    let controller = Rc::new(RefCell::new(Controller {
        windows: HashMap::new(),
        events: sender,
    }));
    Some((super::PetService { controller }, receiver))
}

impl Controller {
    pub(super) fn handle(&mut self, command: PetCommand) {
        match command {
            PetCommand::Present { id, x, y, visual } => self.present(&id, x, y, *visual),
            PetCommand::Update { id, visual } => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.apply_visual(*visual);
                    window.place_and_reveal();
                }
            }
            PetCommand::Move { id, x, y } => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.sprite_x = x;
                    window.sprite_y = y;
                    window.place_and_reveal();
                }
            }
            // The context menu lives inside the window (ContextMenuArea), so
            // there is nothing to show on command.
            PetCommand::ShowMenu { .. } => {}
            PetCommand::Dismiss { id } => {
                // Hide explicitly: dropping the component only unregisters the
                // adapter, the winit window itself lives in an Arc the event
                // loop may still hold, so the pet would linger on screen.
                if let Some(window) = self.windows.remove(&id) {
                    let _ = window.comp.hide();
                }
            }
            PetCommand::Shutdown => {
                for (_, window) in self.windows.drain() {
                    let _ = window.comp.hide();
                }
            }
        }
    }

    fn present(&mut self, id: &str, x: f64, y: f64, visual: PetVisual) {
        if self.windows.get_mut(id).is_none() {
            let Some(window) = PetWindow::create(id, self.events.clone()) else {
                return;
            };
            self.windows.insert(id.to_string(), window);
        }
        let window = self.windows.get_mut(id).expect("present");
        window.apply_visual(visual);
        window.sprite_x = x;
        window.sprite_y = y;
        window.place_and_reveal();
    }
}

impl PetWindow {
    fn create(id: &str, events: Sender<PetEvent>) -> Option<PetWindow> {
        // The platform hook must know before the adapter is created that this
        // window is a pet overlay, not the workbench.
        mark_overlay_window();
        let created = PetOverlay::new();
        // Whether the hook consumed the claim or the creation failed, clear
        // what is left so a later workbench window is not misattributed.
        let comp = match created {
            Ok(comp) => comp,
            Err(_) => {
                super::take_overlay_window();
                return None;
            }
        };
        super::take_overlay_window();
        wire_callbacks(&comp, id, events);
        Some(PetWindow {
            comp,
            timer: slint::Timer::default(),
            playing: None,
            sprite_x: 0.0,
            sprite_y: 0.0,
        })
    }

    /// Push one projection: sprite geometry, animation and bubble.
    fn apply_visual(&mut self, visual: PetVisual) {
        let PetVisual {
            width,
            height,
            anim,
            bubble,
            ..
        } = visual;
        self.comp.set_sprite_width(width as f32);
        self.comp.set_sprite_height(height as f32);
        self.comp
            .set_window_width(width.max(BUBBLE_WINDOW_WIDTH) as f32);
        match anim {
            Some(anim) => self.play(anim),
            None => {
                self.timer.stop();
                self.playing = None;
            }
        }
        let (text, tone) = match bubble {
            Some(PetBubble { text, tone }) => (text, tone),
            None => (String::new(), BubbleTone::Info),
        };
        self.comp.set_show_bubble(!text.is_empty());
        self.comp.set_bubble_text(text.into());
        self.comp.set_bubble_tone(match tone {
            BubbleTone::Info => 0,
            BubbleTone::Success => 1,
            BubbleTone::Warning => 2,
        });
    }

    /// Advance to one animation row, cropping its frames out of the sheet once.
    fn play(&mut self, anim: PetAnim) {
        let matches_current = self.playing.as_ref().is_some_and(|(sheet, row, frames)| {
            Arc::ptr_eq(sheet, &anim.sheet) && *row == anim.row && *frames == anim.frames
        });
        if matches_current {
            return;
        }
        let frames = extract_frames(&anim.sheet, anim.row, anim.frames);
        if frames.is_empty() {
            self.timer.stop();
            self.playing = None;
            return;
        }
        self.comp.set_sprite(frames[0].clone());
        let weak = self.comp.as_weak();
        let mut index = 0usize;
        let len = frames.len();
        self.timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(anim.frame_millis),
            move || {
                index = (index + 1) % len;
                if let Some(comp) = weak.upgrade() {
                    comp.set_sprite(frames[index].clone());
                }
            },
        );
        self.playing = Some((anim.sheet, anim.row, anim.frames));
    }

    /// Anchor the window around the sprite origin — horizontally centred on
    /// the sprite, bottom edge on the sprite's bottom edge — then make sure it
    /// is on screen. The height comes from the window's own content binding.
    fn place_and_reveal(&mut self) {
        let width = f64::from(self.comp.get_window_width());
        clamp_sprite(&self.comp, width, &mut self.sprite_x, &mut self.sprite_y);
        let (x, y) = window_pos_for(&self.comp, self.sprite_x, self.sprite_y);
        self.comp
            .window()
            .set_position(slint::LogicalPosition::new(x, y));
        // Re-showing an already-mapped overlay makes mutter re-raise the
        // demands-attention state on every projection, leaving the dock icon
        // bouncing; map only once and clear the state after the WM sees it.
        if !self.comp.window().is_visible() {
            let _ = self.comp.show();
            let weak = self.comp.as_weak();
            slint::Timer::single_shot(Duration::from_millis(100), move || {
                if let Some(comp) = weak.upgrade() {
                    crate::native_window::clear_demands_attention(comp.window());
                }
            });
        }
    }
}

/// Wire the window's native interactions. The closures are self-contained:
/// they read geometry back from the component's properties and report through
/// the event channel, so the controller map is never borrowed re-entrantly.
fn wire_callbacks(comp: &PetOverlay, id: &str, events: Sender<PetEvent>) {
    let id = id.to_string();
    // Last dragged sprite origin, shared between the move and finish handlers.
    let dragged: Rc<RefCell<Option<(f64, f64)>>> = Rc::new(RefCell::new(None));

    let weak = comp.as_weak();
    let moved = Rc::new(RefCell::new(false));
    comp.on_drag_move({
        let moved = Rc::clone(&moved);
        let dragged_move = Rc::clone(&dragged);
        move |dx, dy| {
            let Some(comp) = weak.upgrade() else { return };
            if dx.abs() > CLICK_THRESHOLD_PX || dy.abs() > CLICK_THRESHOLD_PX {
                *moved.borrow_mut() = true;
            }
            if !*moved.borrow() {
                return;
            }
            // Self-correcting drag: the window goes where the pointer's screen
            // position says, anchored on the press point. Reading the actual
            // position back from winit keeps the rule stable even when a
            // pending move has not landed yet.
            use slint::winit_030::WinitWindowAccessor;
            let Some((wx, wy)) = comp
                .window()
                .with_winit_window(|native| {
                    let scale = f64::from(native.scale_factor());
                    let pos = native.outer_position().ok()?;
                    Some((f64::from(pos.x) / scale, f64::from(pos.y) / scale))
                })
                .flatten()
            else {
                return;
            };
            let sprite_w = f64::from(comp.get_sprite_width());
            let width = f64::from(comp.get_window_width());
            let band = f64::from(comp.get_bubble_band());
            let mut sx = wx + f64::from(dx) + (width - sprite_w) / 2.0;
            let mut sy = wy + f64::from(dy) + band;
            clamp_sprite(&comp, width, &mut sx, &mut sy);
            *dragged_move.borrow_mut() = Some((sx, sy));
            let (x, y) = window_pos_for(&comp, sx, sy);
            comp.window()
                .set_position(slint::LogicalPosition::new(x, y));
        }
    });

    comp.on_drag_finished({
        let moved = Rc::clone(&moved);
        let dragged_end = Rc::clone(&dragged);
        let id = id.clone();
        let events = events.clone();
        move || {
            let finished = dragged_end.borrow_mut().take();
            let event = match (finished, *moved.borrow()) {
                (Some((x, y)), true) => PetEvent::Dragged {
                    id: id.clone(),
                    x,
                    y,
                },
                _ => PetEvent::Clicked { id: id.clone() },
            };
            *moved.borrow_mut() = false;
            let _ = events.send(event);
        }
    });

    comp.on_menu_open_chat({
        let id = id.clone();
        let events = events.clone();
        move || {
            let _ = events.send(PetEvent::MenuSelected {
                id: id.clone(),
                code: MENU_OPEN_CHAT,
            });
        }
    });
    comp.on_menu_hide({
        let id = id.clone();
        move || {
            let _ = events.send(PetEvent::MenuSelected {
                id: id.clone(),
                code: MENU_TOGGLE_SHOW,
            });
        }
    });
}

/// Crop one animation row into per-frame textures. Real packages ship fewer
/// columns than a row declares, so missing frames are skipped.
fn extract_frames(sheet: &ArcSheet, row: usize, frames: usize) -> Vec<Image> {
    let width = companion_sprite::FRAME_WIDTH as usize;
    let height = companion_sprite::FRAME_HEIGHT as usize;
    let pitch = width * 4;
    (0..frames)
        .filter_map(|index| {
            let (offset, stride) = sheet.frame(row, index)?;
            let mut buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::new(
                companion_sprite::FRAME_WIDTH,
                companion_sprite::FRAME_HEIGHT,
            );
            let target = buffer.make_mut_bytes();
            for line in 0..height {
                let source = line * stride + offset;
                target[line * pitch..(line + 1) * pitch]
                    .copy_from_slice(&sheet.data[source..source + pitch]);
            }
            Some(Image::from_rgba8(buffer))
        })
        .collect()
}

/// Keep the sprite origin inside the window's monitor.
fn clamp_sprite(comp: &PetOverlay, width: f64, sx: &mut f64, sy: &mut f64) {
    use slint::winit_030::WinitWindowAccessor;
    let height = f64::from(comp.get_bubble_band()) + f64::from(comp.get_sprite_height());
    let _ = comp.window().with_winit_window(|native| {
        let Some(monitor) = native.current_monitor() else {
            return;
        };
        let scale = f64::from(native.scale_factor());
        let origin = monitor.position();
        let size = monitor.size();
        let (ox, oy) = (f64::from(origin.x) / scale, f64::from(origin.y) / scale);
        let (mw, mh) = (
            f64::from(size.width) / scale,
            f64::from(size.height) / scale,
        );
        let max_x = (ox + mw - width - SCREEN_MARGIN).max(ox + SCREEN_MARGIN);
        let max_y = (oy + mh - height - SCREEN_MARGIN).max(oy + SCREEN_MARGIN);
        *sx = (*sx).clamp(ox + SCREEN_MARGIN, max_x).round();
        *sy = (*sy).clamp(oy + SCREEN_MARGIN, max_y).round();
    });
}

/// Window position that anchors the sprite box at `(sx, sy)`.
fn window_pos_for(comp: &PetOverlay, sx: f64, sy: f64) -> (f32, f32) {
    let sprite_w = f64::from(comp.get_sprite_width());
    let width = f64::from(comp.get_window_width());
    let band = f64::from(comp.get_bubble_band());
    ((sx - (width - sprite_w) / 2.0) as f32, (sy - band) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use companion_sprite::{FRAME_HEIGHT, FRAME_WIDTH};

    /// A two-column sheet whose pixels encode their column index in red.
    fn sheet() -> ArcSheet {
        let row_stride = 2 * FRAME_WIDTH as usize * 4;
        let mut data = vec![0u8; row_stride * FRAME_HEIGHT as usize];
        for column in 0..2usize {
            for line in 0..FRAME_HEIGHT as usize {
                let base = line * row_stride + column * FRAME_WIDTH as usize * 4;
                for pixel in 0..FRAME_WIDTH as usize {
                    data[base + pixel * 4] = column as u8 * 255;
                    data[base + pixel * 4 + 3] = 255;
                }
            }
        }
        Arc::new(companion_sprite::Sheet {
            columns: 2,
            rows: 1,
            data,
        })
    }

    #[test]
    fn frames_come_out_of_the_requested_row() {
        let sheet = sheet();
        assert_eq!(extract_frames(&sheet, 0, 2).len(), 2);
        // A row beyond the sheet's height yields nothing.
        assert!(extract_frames(&sheet, 3, 2).is_empty());
    }

    #[test]
    fn missing_frames_are_skipped() {
        let sheet = sheet();
        // The sheet has two columns; asking for three clamps to the two present.
        assert_eq!(extract_frames(&sheet, 0, 3).len(), 2);
    }
}
