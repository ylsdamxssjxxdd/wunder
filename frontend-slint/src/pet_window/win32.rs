//! Win32 layered-window backing for the floating companion.
//!
//! One thread owns every pet window: it pumps messages, runs the frame clocks,
//! composites sprite + speech bubble into a premultiplied BGRA surface and hands
//! it to `UpdateLayeredWindow`. Fully transparent pixels stay click-through, so
//! the character's own outline is the hit area.

use super::{BubbleTone, PetAnim, PetCommand, PetEvent, PetMenuItem, PetVisual};
use crate::companion_sprite::Sheet;
use crate::pet_window::PetService;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection,
    CreateFontW, DeleteDC, DeleteObject, DIB_RGB_COLORS, DrawTextW, DT_CALCRECT, DT_END_ELLIPSIS,
    DT_WORDBREAK, GetDeviceCaps, GetDC, HFONT, LOGPIXELSX, RGBQUAD, ReleaseDC, SelectObject,
    SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, CREATESTRUCTW, CS_HREDRAW, DefWindowProcW,
    DestroyMenu, DestroyWindow, DispatchMessageW, GetMessageW, GetCursorPos, IDC_ARROW,
    LoadCursorW, MF_CHECKED, MF_SEPARATOR, MF_STRING, MSG, PostQuitMessage, PostThreadMessageW,
    RegisterClassExW, SetForegroundWindow, SetTimer, ShowWindow, SPI_GETWORKAREA, SW_SHOWNOACTIVATE,
    SystemParametersInfoW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WNDCLASSEXW, WM_APP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCDESTROY, WM_RBUTTONUP, WM_TIMER, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// `windows-sys` does not re-export these blend constants for this version.
const AC_SRC_OVER: u8 = 0;
const AC_SRC_ALPHA: u8 = 1;
/// Sprite frame box, matching the web sheet geometry.
const FRAME_WIDTH: i32 = 192;
const FRAME_HEIGHT: i32 = 208;
const TICK_MS: u32 = 30;
/// Web `bottom: calc(100% + 4px)` between the bubble and the sprite.
const GAP_LOGICAL: i32 = 4;
const BUBBLE_PADDING_LOGICAL: i32 = 8;
const BUBBLE_RADIUS_LOGICAL: i32 = 8;
const BUBBLE_FONT_LOGICAL: i32 = 12;
const BUBBLE_MAX_WIDTH_LOGICAL: i32 = 320;
const BUBBLE_MAX_HEIGHT_LOGICAL: i32 = 88;
const DRAG_THRESHOLD_LOGICAL: i32 = 3;

struct Surface {
    dc: isize,
    bitmap: isize,
    bits: *mut u8,
    width: i32,
    height: i32,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            if self.bitmap != 0 {
                DeleteObject(self.bitmap);
            }
            if self.dc != 0 {
                DeleteDC(self.dc);
            }
        }
    }
}

/// Laid-out speech bubble: per-pixel glyph coverage (0..=255) plus its colors.
struct BubbleLayout {
    wanted: (String, BubbleTone),
    width: i32,
    height: i32,
    mask: Vec<u8>,
}

struct WindowState {
    id: String,
    visual: PetVisual,
    /// Physical top-left of the sprite box; the bubble grows above it.
    sprite_x: i32,
    sprite_y: i32,
    dpi: f64,
    surface: Option<Surface>,
    bubble: Option<BubbleLayout>,
    /// Last bubble request laid out, so an unchanged or failed layout is not
    /// retried every tick.
    bubble_wanted: Option<(String, BubbleTone)>,
    frame: usize,
    /// When the current animation started; restarted whenever the animation,
    /// its row or its tempo changes.
    since: Instant,
    /// Folded geometry + frame + bubble, so an unchanged tick costs no repaint.
    key: u64,
    pressed: bool,
    dragging: bool,
    drag_left: bool,
    press_cursor: (i32, i32),
    press_sprite: (i32, i32),
    events: Sender<PetEvent>,
}

thread_local! {
    static STATES: RefCell<HashMap<isize, usize>> = RefCell::new(HashMap::new());
    static FONT: RefCell<Option<HFONT>> = const { RefCell::new(None) };
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A window class belongs to the thread that registered it, so the name carries
/// the thread id: a second overlay thread in the same process can then start
/// while the first one is still winding down.
fn class_name() -> Vec<u16> {
    let thread = unsafe { GetCurrentThreadId() };
    wide(&format!("WunderCompanionPet{thread}"))
}

/// Device scale factor of the primary display.
pub fn scale_factor() -> f64 {
    unsafe {
        let dc = GetDC(0);
        if dc == 0 {
            return 1.0;
        }
        let dpi = GetDeviceCaps(dc, LOGPIXELSX as i32);
        ReleaseDC(0, dc);
        if dpi <= 0 {
            1.0
        } else {
            dpi as f64 / 96.0
        }
    }
}

/// Logical work area (x, y, width, height) of the primary monitor, taskbar
/// excluded, so a pet never lands on top of it.
pub fn work_area() -> Option<(f64, f64, f64, f64)> {
    let dpi = scale_factor();
    unsafe {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let ok = SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rect as *mut RECT as *mut c_void, 0);
        if ok == 0 {
            return None;
        }
        Some((
            rect.left as f64 / dpi,
            rect.top as f64 / dpi,
            (rect.right - rect.left) as f64 / dpi,
            (rect.bottom - rect.top) as f64 / dpi,
        ))
    }
}

/// Start the overlay thread. The UI keeps the receiver and applies each event
/// on the Slint event loop.
pub fn spawn() -> Option<(PetService, Receiver<PetEvent>)> {
    let (events_tx, events_rx) = channel::<PetEvent>();
    let (ready_tx, ready_rx) = channel::<u32>();
    std::thread::Builder::new()
        .name("wunder-companion-pet".to_string())
        .spawn(move || {
            let dpi = scale_factor();
            let _ = ready_tx.send(unsafe { GetCurrentThreadId() });
            unsafe { run(dpi, events_tx) };
        })
        .ok()?;
    let thread_id = ready_rx.recv().ok()?;
    Some((PetService { thread_id }, events_rx))
}

/// Hand one command to the overlay thread. The box crosses over inside the
/// thread message and is reclaimed by the receiver.
pub fn post(thread_id: u32, command: PetCommand) {
    let pointer = Box::into_raw(Box::new(command)) as isize;
    if unsafe { PostThreadMessageW(thread_id, WM_APP, 0, pointer) } == 0 {
        unsafe {
            drop(Box::from_raw(pointer as *mut PetCommand));
        }
    }
}

struct Loop<'a> {
    windows: HashMap<String, isize>,
    events: &'a Sender<PetEvent>,
    dpi: f64,
    instance: isize,
    class: Vec<u16>,
}

unsafe fn run(dpi: f64, events: Sender<PetEvent>) {
    let instance = GetModuleHandleW(std::ptr::null());
    let class = class_name();
    let mut info: WNDCLASSEXW = std::mem::zeroed();
    info.cbSize = std::mem::size_of_val(&info) as u32;
    info.style = CS_HREDRAW;
    info.lpfnWndProc = Some(wndproc);
    info.hInstance = instance;
    info.hCursor = LoadCursorW(0, IDC_ARROW);
    info.hbrBackground = 0;
    info.lpszClassName = class.as_ptr();
    if RegisterClassExW(&info) == 0 {
        return;
    }
    let mut context = Loop {
        windows: HashMap::new(),
        events: &events,
        dpi,
        instance,
        class,
    };
    let mut message: MSG = std::mem::zeroed();
    while GetMessageW(&mut message, 0, 0, 0) > 0 {
        if message.hwnd == 0 && message.message == WM_APP {
            let command = *Box::from_raw(message.lParam as *mut PetCommand);
            handle_command(command, &mut context);
        }
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    for hwnd in context.windows.values().copied() {
        DestroyWindow(hwnd);
    }
    STATES.with(|slot| slot.borrow_mut().clear());
}

unsafe fn handle_command(command: PetCommand, context: &mut Loop<'_>) {
    match command {
        PetCommand::Present { id, x, y, visual } => {
            if let Some(hwnd) = context.windows.get(&id).copied() {
                with_state(hwnd, |state, hwnd| {
                    apply_visual(state, *visual);
                    state.sprite_x = (x * state.dpi).round() as i32;
                    state.sprite_y = (y * state.dpi).round() as i32;
                    render(state, hwnd, true);
                });
                return;
            }
            let sprite_x = (x * context.dpi).round() as i32;
            let sprite_y = (y * context.dpi).round() as i32;
            let width = (visual.width * context.dpi).max(1.0) as i32;
            let height = (visual.height * context.dpi).max(1.0) as i32;
            let state = Box::new(WindowState {
                dpi: context.dpi,
                events: context.events.clone(),
                id: id.clone(),
                visual: *visual,
                sprite_x,
                sprite_y,
                surface: None,
                bubble: None,
                bubble_wanted: None,
                frame: 0,
                since: Instant::now(),
                key: u64::MAX,
                pressed: false,
                dragging: false,
                drag_left: false,
                press_cursor: (0, 0),
                press_sprite: (0, 0),
            });
            // `WM_NCCREATE` claims ownership; a failed create must not leak it.
            let pointer = Box::into_raw(state);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                context.class.as_ptr(),
                std::ptr::null(),
                WS_POPUP,
                sprite_x,
                sprite_y,
                width,
                height,
                0,
                0,
                context.instance,
                pointer as *const c_void,
            );
            if hwnd == 0 {
                drop(Box::from_raw(pointer));
                return;
            }
            SetTimer(hwnd, 1, TICK_MS, None);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            context.windows.insert(id, hwnd);
            with_state(hwnd, |state, hwnd| render(state, hwnd, true));
        }
        PetCommand::Update { id, visual } => {
            let Some(hwnd) = context.windows.get(&id).copied() else {
                return;
            };
            with_state(hwnd, |state, hwnd| {
                apply_visual(state, *visual);
                render(state, hwnd, true);
            });
        }
        PetCommand::Move { id, x, y } => {
            let Some(hwnd) = context.windows.get(&id).copied() else {
                return;
            };
            with_state(hwnd, |state, hwnd| {
                // The pointer owns the position mid-drag; a projection must not
                // snap the pet back under the cursor.
                if state.dragging {
                    return;
                }
                state.sprite_x = (x * state.dpi).round() as i32;
                state.sprite_y = (y * state.dpi).round() as i32;
                render(state, hwnd, false);
            });
        }
        PetCommand::ShowMenu { id, x, y, items } => {
            let Some(hwnd) = context.windows.get(&id).copied() else {
                return;
            };
            show_menu(hwnd, &id, x, y, items, context.dpi, context.events);
        }
        PetCommand::Dismiss { id } => {
            if let Some(hwnd) = context.windows.remove(&id) {
                DestroyWindow(hwnd);
            }
        }
        PetCommand::Shutdown => {
            for hwnd in context.windows.values().copied() {
                DestroyWindow(hwnd);
            }
            context.windows.clear();
            PostQuitMessage(0);
        }
    }
}

/// Replace the visual, restarting the clock when the animation itself changed.
fn apply_visual(state: &mut WindowState, visual: PetVisual) {
    let clock_of =
        |anim: &Option<PetAnim>| anim.as_ref().map(|anim| (anim.row, anim.frames, anim.frame_millis));
    let same_clock = clock_of(&state.visual.anim) == clock_of(&visual.anim);
    let same_size = state.visual.width == visual.width && state.visual.height == visual.height;
    state.visual = visual;
    if !same_clock {
        state.frame = 0;
        state.since = Instant::now();
    }
    if !same_size {
        // Force a repaint even if the folded key happens to match.
        state.key = u64::MAX;
    }
}

unsafe fn show_menu(
    hwnd: HWND,
    id: &str,
    x: f64,
    y: f64,
    items: Vec<PetMenuItem>,
    dpi: f64,
    events: &Sender<PetEvent>,
) {
    let menu = CreatePopupMenu();
    if menu == 0 {
        return;
    }
    for item in &items {
        let label = wide(&item.label);
        let flags = if item.separator {
            MF_SEPARATOR
        } else {
            MF_STRING | if item.checked { MF_CHECKED } else { 0 }
        };
        AppendMenuW(
            menu,
            flags,
            if item.separator { 0 } else { item.code as usize },
            if item.separator {
                std::ptr::null()
            } else {
                label.as_ptr()
            },
        );
    }
    // Modal for this thread; the foreground ping is what lets an outside click
    // dismiss the menu instead of leaving it stuck open.
    SetForegroundWindow(hwnd);
    let picked = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
        (x * dpi) as i32,
        (y * dpi) as i32,
        0,
        hwnd,
        std::ptr::null(),
    );
    DestroyMenu(menu);
    if picked > 0 {
        let _ = events.send(PetEvent::MenuSelected {
            id: id.to_string(),
            code: picked as u16,
        });
    }
}

fn bubble_font(dpi: f64) -> HFONT {
    FONT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let face = wide("Microsoft YaHei");
            unsafe {
                *slot = Some(CreateFontW(
                    -((BUBBLE_FONT_LOGICAL as f64 * dpi).round() as i32),
                    0,
                    0,
                    0,
                    FW_NORMAL,
                    0,
                    0,
                    0,
                    GB2312_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    ANTIALIASED_QUALITY,
                    FIXED_PITCH | FF_DONTCARE,
                    face.as_ptr(),
                ));
            }
        }
        slot.unwrap_or(0)
    })
}

const FW_NORMAL: i32 = 400;
const GB2312_CHARSET: u32 = 128;
const OUT_DEFAULT_PRECIS: u32 = 0;
const CLIP_DEFAULT_PRECIS: u32 = 0;
/// Grayscale antialiasing: the bubble mask reads a single channel as coverage,
/// which subpixel rendering would make colour dependent.
const ANTIALIASED_QUALITY: u32 = 4;
const FIXED_PITCH: u32 = 1;
const FF_DONTCARE: u32 = 0;

fn dib_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative height asks for a top-down bitmap, so the pixels read in
            // screen order.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD {
            rgbRed: 0,
            rgbGreen: 0,
            rgbBlue: 0,
            rgbReserved: 0,
        }],
    }
}

/// 32-bit DIB selected into `dc`, returning the handle and its pixel base.
unsafe fn dib_section(dc: isize, width: i32, height: i32) -> Option<(isize, *mut u8)> {
    let mut bits: *mut c_void = std::ptr::null_mut();
    let bitmap = CreateDIBSection(dc, &dib_info(width, height), DIB_RGB_COLORS, &mut bits, 0, 0);
    (bitmap != 0 && !bits.is_null()).then_some((bitmap, bits as *mut u8))
}

unsafe fn text_metrics(dc: isize, utf: &[u16], rect: &mut RECT, flags: u32) -> i32 {
    DrawTextW(dc, utf.as_ptr(), (utf.len() - 1) as i32, rect, flags)
}

/// Measure and rasterize the bubble text as a white-on-black coverage mask.
unsafe fn layout_bubble(wanted: &(String, BubbleTone), dpi: f64) -> Option<BubbleLayout> {
    let text = wanted.0.trim();
    if text.is_empty() {
        return None;
    }
    let padding = (BUBBLE_PADDING_LOGICAL as f64 * dpi).round() as i32;
    let max_width = (BUBBLE_MAX_WIDTH_LOGICAL as f64 * dpi) as i32;
    let max_height = (BUBBLE_MAX_HEIGHT_LOGICAL as f64 * dpi) as i32;
    let screen = GetDC(0);
    if screen == 0 {
        return None;
    }
    let dc = CreateCompatibleDC(screen);
    if dc == 0 {
        ReleaseDC(0, screen);
        return None;
    }
    // A 1x1 holder bitmap makes the DC usable for text metrics.
    let Some((holder, _)) = dib_section(dc, 1, 1) else {
        DeleteDC(dc);
        ReleaseDC(0, screen);
        return None;
    };
    let previous = SelectObject(dc, holder);
    SelectObject(dc, bubble_font(dpi));
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, 0x00FF_FFFF);
    let utf = wide(text);
    let mut measure = RECT {
        left: 0,
        top: 0,
        right: max_width,
        bottom: max_height,
    };
    text_metrics(dc, &utf, &mut measure, DT_CALCRECT | DT_WORDBREAK | DT_END_ELLIPSIS);
    let text_width = (measure.right - measure.left).clamp(1, max_width);
    let text_height = (measure.bottom - measure.top).clamp(1, max_height);
    let width = text_width + padding * 2;
    let height = text_height + padding * 2;

    let layout = match dib_section(dc, width, height) {
        Some((bitmap, bits)) => {
            SelectObject(dc, bitmap);
            let pixels =
                std::slice::from_raw_parts_mut(bits, (width * height * 4) as usize);
            pixels.fill(0);
            let mut target = RECT {
                left: padding,
                top: padding,
                // Wrap with exactly the width the measurement used so the line
                // breaks match, and let the padding absorb the glyph overhang
                // the tight rectangle leaves out; clipping to the surface edge
                // instead cuts the last glyph of a line in half.
                right: padding + max_width,
                bottom: padding + max_height,
            };
            text_metrics(dc, &utf, &mut target, DT_WORDBREAK | DT_END_ELLIPSIS);
            // Glyph pixels come back white, so one channel is the coverage mask.
            let mask = pixels
                .chunks_exact(4)
                .map(|pixel| pixel[0])
                .collect::<Vec<u8>>();
            SelectObject(dc, previous);
            DeleteObject(bitmap);
            Some(BubbleLayout {
                wanted: wanted.clone(),
                width,
                height,
                mask,
            })
        }
        None => None,
    };
    DeleteObject(holder);
    DeleteDC(dc);
    ReleaseDC(0, screen);
    layout
}

unsafe fn with_state(hwnd: HWND, apply: impl FnOnce(&mut WindowState, HWND)) {
    let pointer = STATES.with(|slot| slot.borrow().get(&hwnd).copied());
    if let Some(pointer) = pointer {
        apply(&mut *(pointer as *mut WindowState), hwnd);
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = &*(lparam as *const CREATESTRUCTW);
            STATES.with(|slot| slot.borrow_mut().insert(hwnd, create.lpCreateParams as usize));
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_NCDESTROY => {
            let pointer = STATES.with(|slot| slot.borrow_mut().remove(&hwnd));
            if let Some(pointer) = pointer {
                drop(Box::from_raw(pointer as *mut WindowState));
            }
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_TIMER => {
            with_state(hwnd, |state, hwnd| tick(state, hwnd));
            0
        }
        WM_LBUTTONDOWN => {
            with_state(hwnd, |state, hwnd| {
                state.pressed = true;
                state.dragging = false;
                state.press_cursor = cursor_position();
                state.press_sprite = (state.sprite_x, state.sprite_y);
                SetCapture(hwnd);
            });
            0
        }
        WM_MOUSEMOVE => {
            with_state(hwnd, |state, hwnd| {
                if !state.pressed {
                    return;
                }
                let cursor = cursor_position();
                let dx = cursor.0 - state.press_cursor.0;
                let dy = cursor.1 - state.press_cursor.1;
                let threshold = (DRAG_THRESHOLD_LOGICAL as f64 * state.dpi) as i32;
                if !state.dragging && dx.abs().max(dy.abs()) <= threshold {
                    return;
                }
                if !state.dragging {
                    state.dragging = true;
                    state.frame = 0;
                    state.since = Instant::now();
                }
                state.drag_left = dx < 0;
                state.sprite_x = state.press_sprite.0 + dx;
                state.sprite_y = state.press_sprite.1 + dy;
                render(state, hwnd, false);
            });
            0
        }
        WM_LBUTTONUP => {
            with_state(hwnd, |state, hwnd| {
                if !state.pressed {
                    return;
                }
                state.pressed = false;
                ReleaseCapture();
                let event = if state.dragging {
                    state.dragging = false;
                    state.frame = 0;
                    state.since = Instant::now();
                    PetEvent::Dragged {
                        id: state.id.clone(),
                        x: state.sprite_x as f64 / state.dpi,
                        y: state.sprite_y as f64 / state.dpi,
                    }
                } else {
                    PetEvent::Clicked {
                        id: state.id.clone(),
                    }
                };
                let _ = state.events.send(event);
                render(state, hwnd, false);
            });
            0
        }
        WM_RBUTTONUP => {
            with_state(hwnd, |state, _| {
                let cursor = cursor_position();
                let _ = state.events.send(PetEvent::RightClicked {
                    id: state.id.clone(),
                    x: cursor.0 as f64 / state.dpi,
                    y: cursor.1 as f64 / state.dpi,
                });
            });
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn cursor_position() -> (i32, i32) {
    unsafe {
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        (point.x, point.y)
    }
}

fn active_anim(state: &WindowState) -> Option<&PetAnim> {
    if state.dragging {
        let index = usize::from(!state.drag_left);
        if state.visual.drag[index].is_some() {
            return state.visual.drag[index].as_ref();
        }
    }
    state.visual.anim.as_ref()
}

/// Advance the frame clock and repaint if anything actually changed.
unsafe fn tick(state: &mut WindowState, hwnd: HWND) {
    let Some(anim) = active_anim(state) else {
        render(state, hwnd, false);
        return;
    };
    let frames = anim.frames.max(1);
    let elapsed = state.since.elapsed().as_millis() as u64;
    let index = ((elapsed / anim.frame_millis.max(1)) as usize) % frames;
    if index != state.frame {
        state.frame = index;
    }
    render(state, hwnd, false);
}

/// Last composited surface per window, kept only for the visual probe.
#[cfg(test)]
fn surfaces(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, (i32, i32, Vec<u8>)>> {
    static SURFACES: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, (i32, i32, Vec<u8>)>>,
    > = std::sync::OnceLock::new();
    SURFACES.get_or_init(Default::default)
}

/// Rebuild the premultiplied surface and present it, unless nothing changed.
unsafe fn render(state: &mut WindowState, hwnd: HWND, force: bool) {
    let dpi = state.dpi;
    let sprite_width = (state.visual.width * dpi).round() as i32;
    let sprite_height = (state.visual.height * dpi).round() as i32;
    if sprite_width <= 0 || sprite_height <= 0 {
        return;
    }
    let wanted = state
        .visual
        .bubble
        .as_ref()
        .map(|bubble| (bubble.text.clone(), bubble.tone));
    if wanted != state.bubble_wanted {
        // Remember the request even when GDI fails, so a broken DC is retried on
        // the next bubble change instead of every animation tick.
        state.bubble_wanted = wanted.clone();
        state.bubble = wanted
            .as_ref()
            .and_then(|wanted| layout_bubble(wanted, dpi));
    }
    let (bubble_width, bubble_height) = state
        .bubble
        .as_ref()
        .map(|layout| (layout.width, layout.height))
        .unwrap_or((0, 0));
    let gap = if bubble_height > 0 {
        (GAP_LOGICAL as f64 * dpi).round() as i32
    } else {
        0
    };
    let width = sprite_width.max(bubble_width).max(1);
    let height = sprite_height + bubble_height + gap;
    // Sprite and bubble each centre themselves in the window; the wider one
    // decides the width, so its own offset is zero.
    let sprite_left = (width - sprite_width) / 2;
    let bubble_left = (width - bubble_width) / 2;
    let sprite_top = bubble_height + gap;
    let frame = state.frame;
    let signature = state
        .visual
        .bubble
        .as_ref()
        .map(|bubble| hash(&bubble.text) ^ (bubble.tone as u64))
        .unwrap_or(0);
    let key = fold_key(
        frame,
        state.sprite_x,
        state.sprite_y,
        width,
        height,
        signature,
        state.dragging,
    );
    if !force && key == state.key {
        return;
    }

    // The buffer comes from a raw pointer, so painting can still read the rest
    // of `state`; the surface is never reallocated while it is in use.
    let Some((dc, bits)) = ensure_surface(state, width, height) else {
        return;
    };
    let buffer = std::slice::from_raw_parts_mut(bits, (width * height * 4) as usize);
    buffer.fill(0);
    if let Some(layout) = state.bubble.as_ref() {
        paint_bubble(buffer, width, layout, bubble_left, dpi);
    }
    if let Some(anim) = active_anim(state) {
        paint_sprite(
            buffer,
            width,
            height,
            sprite_left,
            sprite_top,
            sprite_width,
            sprite_height,
            anim,
            frame,
        );
    }
    // GDI screen grabs do not include these layered windows, so the visual
    // probe photographs the surface that is about to be presented instead.
    #[cfg(test)]
    if let Ok(mut surfaces) = surfaces().lock() {
        surfaces.insert(state.id.clone(), (width, height, buffer.to_vec()));
    }
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA,
    };
    let source = POINT { x: 0, y: 0 };
    let destination = POINT {
        x: state.sprite_x - sprite_left,
        y: state.sprite_y - sprite_top,
    };
    let size = SIZE { cx: width, cy: height };
    UpdateLayeredWindow(
        hwnd,
        0,
        &destination,
        &size,
        dc,
        &source,
        0,
        &blend,
        ULW_ALPHA,
    );
    state.key = key;
}

/// Ensure the cached DIB matches the requested size and return its DC plus the
/// pixel base. Split out so `WindowState` is not borrowed while painting.
unsafe fn ensure_surface(state: &mut WindowState, width: i32, height: i32) -> Option<(isize, *mut u8)> {
    let matches = state
        .surface
        .as_ref()
        .is_some_and(|surface| surface.width == width && surface.height == height);
    if !matches {
        state.surface = None;
        let screen = GetDC(0);
        if screen == 0 {
            return None;
        }
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(0, screen);
        if dc == 0 {
            return None;
        }
        let Some((bitmap, bits)) = dib_section(dc, width, height) else {
            DeleteDC(dc);
            return None;
        };
        SelectObject(dc, bitmap);
        state.surface = Some(Surface {
            dc,
            bitmap,
            bits,
            width,
            height,
        });
    }
    let surface = state.surface.as_ref()?;
    Some((surface.dc, surface.bits))
}

/// Rounded-rect bubble with the laid-out glyph coverage painted on top.
fn paint_bubble(
    buffer: &mut [u8],
    window_width: i32,
    layout: &BubbleLayout,
    left: i32,
    dpi: f64,
) {
    let radius = (BUBBLE_RADIUS_LOGICAL as f64 * dpi).max(1.0) as f32;
    // Channels are written in BGRA order, so these triples are B, G, R.
    let (background, foreground): ([u8; 3], [u8; 3]) = match layout.wanted.1 {
        BubbleTone::Info => ([0xFF, 0xFF, 0xFF], [0x29, 0x23, 0x1F]),
        BubbleTone::Success => ([0xEF, 0xF8, 0xEC], [0x34, 0x65, 0x16]),
        BubbleTone::Warning => ([0xE0, 0xF4, 0xFF], [0x12, 0x34, 0x9A]),
    };
    for y in 0..layout.height {
        for x in 0..layout.width {
            if !inside_rounded(x, y, layout.width, layout.height, radius) {
                continue;
            }
            let target_x = left + x;
            if target_x < 0 || target_x >= window_width || y >= layout.height {
                continue;
            }
            let index = (y * window_width + target_x) as usize * 4;
            if index + 3 >= buffer.len() {
                continue;
            }
            let coverage = layout.mask[(y * layout.width + x) as usize];
            let color = mix(background, foreground, coverage);
            // The bubble body is fully opaque, so premultiplication is identity.
            buffer[index] = color[0];
            buffer[index + 1] = color[1];
            buffer[index + 2] = color[2];
            buffer[index + 3] = 255;
        }
    }
}

fn mix(background: [u8; 3], foreground: [u8; 3], coverage: u8) -> [u8; 3] {
    let weight = coverage as u32;
    let blend = |base: u8, top: u8| ((base as u32 * (255 - weight) + top as u32 * weight) / 255) as u8;
    [
        blend(background[0], foreground[0]),
        blend(background[1], foreground[1]),
        blend(background[2], foreground[2]),
    ]
}

fn inside_rounded(x: i32, y: i32, width: i32, height: i32, radius: f32) -> bool {
    if x < 0 || y < 0 || x >= width || y >= height {
        return false;
    }
    let fx = x as f32;
    let fy = y as f32;
    let right = width as f32 - 1.0 - radius;
    let bottom = height as f32 - 1.0 - radius;
    if fx >= radius && fx <= right {
        return true;
    }
    if fy >= radius && fy <= bottom {
        return true;
    }
    let origin_x = if fx < radius { radius } else { right };
    let origin_y = if fy < radius { radius } else { bottom };
    let dx = fx - origin_x;
    let dy = fy - origin_y;
    dx * dx + dy * dy <= radius * radius
}

/// Bilinear scale of one sprite frame, premultiplied into the destination.
fn paint_sprite(
    buffer: &mut [u8],
    window_width: i32,
    window_height: i32,
    left: i32,
    top: i32,
    target_width: i32,
    target_height: i32,
    anim: &PetAnim,
    frame: usize,
) {
    let sheet = anim.sheet.as_ref();
    let Some((offset, stride)) = sheet.frame(anim.row, frame.min(anim.frames.saturating_sub(1)))
    else {
        return;
    };
    let scale_x = FRAME_WIDTH as f32 / target_width.max(1) as f32;
    let scale_y = FRAME_HEIGHT as f32 / target_height.max(1) as f32;
    for y in 0..target_height {
        let target_y = top + y;
        if target_y < 0 || target_y >= window_height {
            continue;
        }
        let source_y = (y as f32 + 0.5) * scale_y - 0.5;
        let base_y = source_y.max(0.0) as usize;
        let next_y = (base_y + 1).min((FRAME_HEIGHT - 1) as usize);
        let weight_y = (source_y - base_y as f32).clamp(0.0, 1.0);
        for x in 0..target_width {
            let target_x = left + x;
            if target_x < 0 || target_x >= window_width {
                continue;
            }
            let source_x = (x as f32 + 0.5) * scale_x - 0.5;
            let base_x = source_x.max(0.0) as usize;
            let next_x = (base_x + 1).min((FRAME_WIDTH - 1) as usize);
            let weight_x = (source_x - base_x as f32).clamp(0.0, 1.0);
            let near = sample(sheet, stride, offset, base_x, base_y);
            let far_right = sample(sheet, stride, offset, next_x, base_y);
            let far_down = sample(sheet, stride, offset, base_x, next_y);
            let corner = sample(sheet, stride, offset, next_x, next_y);
            let mut channel = [0f32; 4];
            for index in 0..4 {
                let top_edge = near[index] * (1.0 - weight_x) + far_right[index] * weight_x;
                let bottom_edge = far_down[index] * (1.0 - weight_x) + corner[index] * weight_x;
                channel[index] = top_edge * (1.0 - weight_y) + bottom_edge * weight_y;
            }
            let alpha = channel[3] / 255.0;
            let index = (target_y * window_width + target_x) as usize * 4;
            if index + 3 >= buffer.len() {
                continue;
            }
            buffer[index] = (channel[2] * alpha) as u8;
            buffer[index + 1] = (channel[1] * alpha) as u8;
            buffer[index + 2] = (channel[0] * alpha) as u8;
            buffer[index + 3] = channel[3] as u8;
        }
    }
}

fn sample(sheet: &Sheet, stride: usize, offset: usize, x: usize, y: usize) -> [f32; 4] {
    let start = y * stride + offset + x * 4;
    let end = start + 4;
    if end > sheet.data.len() {
        return [0.0; 4];
    }
    [
        sheet.data[start] as f32,
        sheet.data[start + 1] as f32,
        sheet.data[start + 2] as f32,
        sheet.data[start + 3] as f32,
    ]
}

fn hash(text: &str) -> u64 {
    let mut value: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        value = (value ^ *byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    value
}

fn fold_key(
    frame: usize,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    signature: u64,
    motion: bool,
) -> u64 {
    let mut value = signature;
    for part in [frame as i64, x as i64, y as i64, width as i64, height as i64] {
        value = value
            .wrapping_mul(31)
            .wrapping_add((part as u64) ^ ((part >> 32) as u64));
    }
    if motion {
        value = value.wrapping_mul(31).wrapping_add(7);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pet_window::PetBubble;

    /// Every visible window owned by the given thread, with its desktop rect.
    fn thread_windows(thread_id: u32) -> Vec<(isize, RECT)> {
        use windows_sys::Win32::Foundation::BOOL;
        use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetWindowRect};
        let mut found: Vec<(isize, RECT)> = Vec::new();
        unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if GetWindowRect(hwnd, &mut rect) != 0 && rect.right > rect.left {
                let list = &mut *(data as *mut Vec<(isize, RECT)>);
                list.push((hwnd, rect));
            }
            1
        }
        unsafe {
            EnumThreadWindows(
                thread_id,
                Some(visit),
                &mut found as *mut _ as LPARAM,
            );
        }
        found
    }

    /// End-to-end check of the native path: thread, message channel, window
    /// creation, GDI text layout and `UpdateLayeredWindow`. Ignored by default
    /// because it really does put a window on the desktop for a moment; run it
    /// with `cargo test --bins -- --ignored`.
    #[test]
    #[ignore = "shows a real layered window on the desktop"]
    fn a_presented_pet_is_laid_out_on_screen() {
        let Some((service, _events)) = spawn() else {
            panic!("the overlay thread did not start");
        };
        let columns = 2usize;
        let rows = 9usize;
        let sheet = std::sync::Arc::new(Sheet {
            columns,
            rows,
            data: vec![255u8; columns * rows * FRAME_WIDTH as usize * FRAME_HEIGHT as usize * 4],
        });
        service.send(PetCommand::Present {
            id: "probe-pet".to_string(),
            x: 60.0,
            y: 60.0,
            visual: Box::new(PetVisual {
                width: FRAME_WIDTH as f64,
                height: FRAME_HEIGHT as f64,
                anim: Some(PetAnim {
                    sheet,
                    row: 0,
                    frames: columns,
                    frame_millis: 90,
                }),
                drag: [None, None],
                bubble: Some(PetBubble {
                    text: "探针".to_string(),
                    tone: BubbleTone::Info,
                }),
            }),
        });
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // Only this service's own thread is searched, so a window left over from
        // another probe run in the same process cannot be mistaken for ours.
        for _ in 0..50 {
            if let Some((_, found)) = thread_windows(service.thread_id).first() {
                rect = *found;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_ne!(
            (rect.left, rect.right),
            (0, 0),
            "the layered window was never created"
        );
        let dpi = scale_factor();
        // The creation rect is the sprite box only. `UpdateLayeredWindow`
        // replaces it with sprite + gap + bubble, so a taller window that
        // starts above the requested origin proves the composite landed.
        let sprite_height = (FRAME_HEIGHT as f64 * dpi) as i32;
        assert!(
            rect.bottom - rect.top > sprite_height,
            "the bubble was never composited: {:?}",
            (rect.top, rect.bottom, sprite_height)
        );
        assert!(
            rect.top < (60.0 * dpi) as i32,
            "the window did not move to fit the bubble: {:?}",
            (rect.top, rect.left, rect.right)
        );
        service.send(PetCommand::Dismiss {
            id: "probe-pet".to_string(),
        });
        service.send(PetCommand::Shutdown);
    }

    /// Visual check: composites real spritesheets into the layered window and
    /// writes a cropped screenshot per state into the temp directory. Skips when
    /// no companion package is installed, so it stays safe to run anywhere.
    #[test]
    #[ignore = "writes screenshot files and shows windows on the desktop"]
    fn a_composited_pet_can_be_photographed() {
        use crate::companion_sprite;
        use std::path::Path;
        use std::time::Duration;

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../config/data/companions/global");
        let Ok(entries) = std::fs::read_dir(&root) else {
            eprintln!("skipped: no companion packages under {}", root.display());
            return;
        };
        let sheets: Vec<_> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path().join("spritesheet.webp"))
            .filter(|path| path.is_file())
            .take(3)
            .collect();
        assert!(!sheets.is_empty(), "no spritesheet found");
        // Left to right, one window per state: plain idle, a short review
        // bubble, and a long hint at the smallest preset scale.
        let cases = [
            (120.0, companion_sprite::IDLE, 1.0f64, None),
            (
                420.0,
                companion_sprite::REVIEW,
                1.0,
                Some("示例：本轮任务已完成，点开查看结果"),
            ),
            (
                720.0,
                companion_sprite::WAITING,
                0.6,
                Some("较长的消息气泡用来检查换行、圆角遮罩与截断在较小缩放下的排版表现，这里刻意写得长一些以触发多行折行。"),
            ),
        ];
        let Some((service, _events)) = spawn() else {
            panic!("the overlay thread did not start");
        };
        for (index, (x, anim, scale, bubble)) in cases.iter().enumerate() {
            let bytes = std::fs::read(&sheets[index]).expect("read spritesheet");
            let sheet = std::sync::Arc::new(
                companion_sprite::decode_sheet(&bytes).expect("decode spritesheet"),
            );
            service.send(PetCommand::Present {
                id: format!("photo-pet-{index}"),
                x: *x,
                y: 320.0,
                visual: Box::new(PetVisual {
                    width: FRAME_WIDTH as f64 * scale,
                    height: FRAME_HEIGHT as f64 * scale,
                    anim: Some(PetAnim {
                        sheet: sheet.clone(),
                        row: anim.row.min(sheet.rows.saturating_sub(1)),
                        frames: anim.frames.min(sheet.columns),
                        frame_millis: anim.frame_millis(),
                    }),
                    drag: [None, None],
                    bubble: bubble.map(|text| PetBubble {
                        text: text.to_string(),
                        tone: BubbleTone::Info,
                    }),
                }),
            });
        }
        // Let a few animation frames and every bubble layout pass land.
        std::thread::sleep(Duration::from_millis(500));
        let output = std::env::temp_dir();
        for index in 0..cases.len() {
            let id = format!("photo-pet-{index}");
            let surface = surfaces()
                .lock()
                .unwrap()
                .remove(&id)
                .unwrap_or_else(|| panic!("{id} never painted a surface"));
            let file = output.join(format!("wunder-pet-probe-{}.png", index + 1));
            save_surface(&file, surface);
            println!("{}", file.display());
        }
        for index in 0..cases.len() {
            service.send(PetCommand::Dismiss {
                id: format!("photo-pet-{index}"),
            });
        }
        drop(service);
    }

    /// Save a premultiplied BGRA surface as a straight RGBA PNG, plus a copy
    /// flattened over a checkerboard so the bubble body and the click-through
    /// margins are both readable in any viewer.
    fn save_surface(file: &std::path::Path, surface: (i32, i32, Vec<u8>)) {
        let (width, height, mut pixels) = surface;
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = pixel[3];
            if alpha > 0 {
                let restore = 255.0 / alpha as f32;
                for channel in &mut pixel[..3] {
                    *channel = (*channel as f32 * restore).round().min(255.0) as u8;
                }
            }
            pixel.swap(0, 2);
        }
        let encode = |pixels: &[u8]| {
            let mut png = Vec::new();
            let mut encoder = png::Encoder::new(&mut png, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .expect("png header")
                .write_image_data(pixels)
                .expect("png data");
            png
        };
        std::fs::write(file, encode(&pixels)).expect("write the screenshot");
        let mut flat = pixels.clone();
        for (index, pixel) in flat.chunks_exact_mut(4).enumerate() {
            let band_y = index / width as usize / 16;
            let band_x = index % width as usize / 16;
            let base = if (band_x + band_y) % 2 == 0 { 0xFF } else { 0x8A };
            let alpha = pixel[3] as u32;
            for channel in &mut pixel[..3] {
                *channel =
                    ((alpha * *channel as u32 + (255 - alpha) * base as u32) / 255) as u8;
            }
            pixel[3] = 255;
        }
        let view = file.with_extension("view.png");
        std::fs::write(&view, encode(&flat)).expect("write the flattened screenshot");
        println!("{}", view.display());
    }
}
