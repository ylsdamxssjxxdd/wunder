use super::fallback::{rgba_to_bgra_in_place, GestureCallback, PresenterFrame};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use std::ptr::null;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, StretchDIBits, UpdateWindow, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, PAINTSTRUCT, SRCCOPY,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetDesktopWindow, GetWindowLongPtrW,
    GetWindowRect, IsWindow, IsWindowVisible, RegisterClassW, SendMessageW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, SWP_NOACTIVATE,
    SWP_NOSENDCHANGING, SWP_NOZORDER, SW_HIDE, SW_SHOWNA, WM_ERASEBKGND, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCHITTEST, WM_PAINT, WNDCLASSW,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WS_CHILD, WS_VISIBLE,
};

const CLASS_NAME: &[u16] = &[
    b'R' as u16,
    b'c' as u16,
    b'h' as u16,
    b'o' as u16,
    b'3' as u16,
    b'D' as u16,
    b'P' as u16,
    b'r' as u16,
    b'e' as u16,
    b's' as u16,
    b'e' as u16,
    b'n' as u16,
    b't' as u16,
    0,
];

struct State {
    frame_sequence: u64,
    painted_sequence: u64,
    geometry: Option<(i32, i32, u32, u32)>,
    visible: bool,
    /// Physical size of the child window. This is the destination rectangle
    /// for StretchDIBits and must not be replaced by the incoming frame size.
    canvas_width: u32,
    canvas_height: u32,
    /// Physical size of the last rasterized frame. Normally equals the canvas;
    /// during resize it remains independent until the matching frame arrives.
    frame_width: u32,
    frame_height: u32,
    pixels: Vec<u8>,
    bitmap: BITMAPINFO,
    drag_origin: Option<(i32, i32)>,
    drag_pan: bool,
    click_origin: Option<(i32, i32)>,
    on_gesture: GestureCallback,
}

/// Windows presenter backed by a persistent 32-bit top-down DIB.  `WM_PAINT`
/// only copies that DIB; all projection and raster work stays in Rust.
pub struct ModelPresenter {
    hwnd: HWND,
    parent: HWND,
    state: *mut State,
}

unsafe impl Send for ModelPresenter {}

impl ModelPresenter {
    /// Used only by the opt-in event-loop check: exercise the native pointer
    /// route, including its asynchronous Slint delivery, without moving the
    /// user's real cursor or capturing input outside this window.
    #[allow(dead_code)]
    pub(crate) fn diagnostic_drag(&self) {
        unsafe {
            (*self.state).drag_origin = Some((100, 100));
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                self.hwnd,
                WM_MOUSEMOVE,
                1,
                ((100 << 16) | 103) as LPARAM,
            );
        }
    }

    pub fn new(window: &slint::Window, on_gesture: GestureCallback) -> Option<Self> {
        let parent = window
            .window_handle()
            .window_handle()
            .ok()
            .and_then(|handle| match handle.as_raw() {
                RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                _ => None,
            })?;
        register_class();
        Self::new_for_parent(parent, on_gesture)
    }

    /// Allocate the native child after the caller has verified that `parent`
    /// is a Win32 surface. Keeping the raw-parent constructor separate also
    /// lets the Windows-only smoke test exercise the real `WM_PAINT`/GDI
    /// route without having to bootstrap a full Slint application.
    fn new_for_parent(parent: HWND, on_gesture: GestureCallback) -> Option<Self> {
        let mut state = Box::new(State {
            frame_sequence: 0,
            painted_sequence: 0,
            geometry: None,
            visible: true,
            canvas_width: 1,
            canvas_height: 1,
            frame_width: 1,
            frame_height: 1,
            pixels: vec![0; 4],
            bitmap: bitmap_info(1, 1),
            drag_origin: None,
            drag_pan: false,
            click_origin: None,
            on_gesture,
        });
        let state_ptr = state.as_mut() as *mut State;
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                CLASS_NAME.as_ptr(),
                CLASS_NAME.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                1,
                1,
                parent,
                0,
                GetModuleHandleW(null()),
                null(),
            )
        };
        if hwnd == 0 {
            return None;
        }
        unsafe {
            // `LONG_PTR` is `isize` on 64-bit Windows and `i32` on Win32;
            // let windows-sys infer the target-width integer here so the
            // same source compiles for the supported Win7 x86 build.
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as usize as _);
            // `WM_MOUSEWHEEL` is delivered to the focus window (the Slint
            // top-level), never to this unfocusable child, and the winit
            // backend would dispatch the scroll at a stale cursor position.
            // Subclass the parent so wheel rotations over the visible child
            // reach its own `WM_MOUSEWHEEL` handler instead.
            SetWindowSubclass(parent, Some(parent_wheel_subclass), hwnd as usize, hwnd as usize);
        }
        let _ = Box::into_raw(state);
        Some(Self {
            hwnd,
            parent,
            state: state_ptr,
        })
    }

    pub fn set_geometry(&self, x: i32, y: i32, width: u32, height: u32) -> bool {
        let width = width.max(1);
        let height = height.max(1);
        unsafe {
            if IsWindow(self.hwnd) == 0 {
                return false;
            }
            let state = &mut *self.state;
            if state.geometry == Some((x, y, width, height)) {
                return true;
            }
            if state.canvas_width != width || state.canvas_height != height {
                state.canvas_width = width;
                state.canvas_height = height;
            }
            if SetWindowPos(
                self.hwnd,
                0,
                x,
                y,
                width as i32,
                height as i32,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
            ) == 0
            {
                return false;
            }
            if InvalidateRect(self.hwnd, null(), 0) == 0 {
                return false;
            }
            // SetWindowPos may synchronously enter window_proc. Do not retain
            // a mutable Rust reference across a re-entrant native call.
            (*self.state).geometry = Some((x, y, width, height));
        }
        true
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe {
            if (*self.state).visible != visible {
                (*self.state).visible = visible;
                ShowWindow(self.hwnd, if visible { SW_SHOWNA } else { SW_HIDE });
                if visible {
                    // Hidden windows can consume the invalidation submitted
                    // with the first DIB. Repaint the retained frame on reveal.
                    InvalidateRect(self.hwnd, null(), 0);
                    UpdateWindow(self.hwnd);
                }
            }
        }
    }

    /// Rare modal transition only: reconstruct opaque RGBA for Slint stacking.
    #[allow(dead_code)]
    pub fn snapshot(&self) -> Option<PresenterFrame> {
        unsafe {
            let state = &*self.state;
            let mut rgba = state.pixels.clone();
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.swap(0, 2);
                pixel[3] = 255;
            }
            Some(PresenterFrame {
                width: state.frame_width,
                height: state.frame_height,
                rgba,
            })
        }
    }

    pub fn present(&self, frame: &mut PresenterFrame) -> bool {
        let frame_width = frame.width.max(1);
        let frame_height = frame.height.max(1);
        let expected = frame_width as usize * frame_height as usize * 4;
        if frame.rgba.len() != expected {
            return false;
        }
        unsafe {
            if IsWindow(self.hwnd) == 0 {
                return false;
            }
            let state = &mut *self.state;
            state.frame_width = frame_width;
            state.frame_height = frame_height;
            if !rgba_to_bgra_in_place(&mut frame.rgba, 0) {
                return false;
            }
            state.pixels = std::mem::take(&mut frame.rgba);
            state.frame_sequence = state.frame_sequence.wrapping_add(1);
            state.bitmap = bitmap_info(state.frame_width, state.frame_height);
            if InvalidateRect(self.hwnd, null(), 0) == 0 {
                frame.rgba = std::mem::take(&mut state.pixels);
                // Current 3D raster frames are opaque RGBA8. Channel swapping
                // is symmetric, while alpha was cleared for BI_RGB and must
                // be restored before handing the frame to Slint's fallback.
                for pixel in frame.rgba.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                    pixel[3] = 0xff;
                }
                return false;
            }
        }
        true
    }

    pub fn pump_events(&self) {}
}

/// A minimal real-window presentation target for the command-line benchmark.
/// It deliberately bypasses Slint and user input so its time represents only
/// the final native GDI copy, not the full workbench's layout/composition.
/// The regular presenter still owns production lifecycle and gestures.
#[allow(dead_code)]
pub(crate) struct GdiPresentationBenchmark {
    presenter: ModelPresenter,
}

impl GdiPresentationBenchmark {
    #[allow(dead_code)]
    pub(crate) fn new(width: u32, height: u32) -> Option<Self> {
        register_class();
        let callback: GestureCallback = std::sync::Arc::new(|_, _, _| {});
        let presenter = ModelPresenter::new_for_parent(unsafe { GetDesktopWindow() }, callback)?;
        // Keep the benchmark's temporary child outside the virtual desktop.
        // It remains mapped so `UpdateWindow` synchronously dispatches the
        // normal paint route, without flashing a benchmark surface to users.
        if !presenter.set_geometry(-10_000, -10_000, width.max(1), height.max(1)) {
            return None;
        }
        Some(Self { presenter })
    }

    /// Copy an already-converted BGRA frame through the exact `WM_PAINT` /
    /// `StretchDIBits` path used in production. `UpdateWindow` makes the
    /// timing synchronous, which is appropriate for a local-copy metric.
    #[allow(dead_code)]
    pub(crate) fn present_bgra(&self, width: u32, height: u32, pixels: Vec<u8>) -> bool {
        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            return false;
        }
        unsafe {
            let state = &mut *self.presenter.state;
            if InvalidateRect(self.presenter.hwnd, null(), 0) == 0 {
                return false;
            }
            state.frame_width = width;
            state.frame_height = height;
            state.pixels = pixels;
            state.frame_sequence = state.frame_sequence.wrapping_add(1);
            state.bitmap = bitmap_info(width, height);
            UpdateWindow(self.presenter.hwnd) != 0
        }
    }
}

impl Drop for ModelPresenter {
    fn drop(&mut self) {
        unsafe {
            RemoveWindowSubclass(self.parent, Some(parent_wheel_subclass), self.hwnd as usize);
            DestroyWindow(self.hwnd);
            drop(Box::from_raw(self.state));
        }
    }
}

fn bitmap_info(width: u32, height: u32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: width.saturating_mul(height).saturating_mul(4),
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [windows_sys::Win32::Graphics::Gdi::RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        }],
    }
}

fn register_class() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(null()),
            hIcon: 0,
            hCursor: 0,
            hbrBackground: 0,
            lpszMenuName: null(),
            lpszClassName: CLASS_NAME.as_ptr(),
        };
        let _ = RegisterClassW(&class);
    });
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    match msg {
        windows_sys::Win32::UI::WindowsAndMessaging::WM_CAPTURECHANGED => {
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                let dragging = state.drag_origin.take().is_some();
                state.drag_pan = false;
                state.click_origin = None;
                if dragging {
                    (state.on_gesture)("settle", 0., 0.);
                }
            }
            0
        }
        WM_ERASEBKGND => 1,
        WM_NCHITTEST => 1, // HTCLIENT: retain normal pointer delivery.
        WM_LBUTTONDOWN => {
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                state.drag_origin = Some(mouse_position(lparam));
                state.drag_pan = false;
                state.click_origin = state.drag_origin;
                let (x,y)=mouse_position(lparam);
                (state.on_gesture)("pointer_down",x as f32,y as f32);
                SetCapture(hwnd);
            }
            0
        }
        WM_LBUTTONUP => {
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                state.drag_origin = None;
                state.drag_pan = false;
                let (x,y)=mouse_position(lparam);
                (state.on_gesture)("pointer_up",x as f32,y as f32);
                if state.click_origin.take().is_some() {
                    let (x, y) = mouse_position(lparam);
                    (state.on_gesture)("click", x as f32, y as f32);
                }
                (state.on_gesture)("settle", 0.0, 0.0);
                ReleaseCapture();
            }
            0
        }
        WM_RBUTTONDOWN => {
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                state.drag_origin = Some(mouse_position(lparam));
                state.drag_pan = true;
                state.click_origin = None;
                SetCapture(hwnd);
            }
            0
        }
        WM_RBUTTONUP => {
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                state.drag_origin = None;
                state.drag_pan = false;
                (state.on_gesture)("settle", 0.0, 0.0);
                ReleaseCapture();
            }
            0
        }
        WM_LBUTTONDBLCLK => {
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                state.drag_origin = None;
                state.click_origin = None;
                ReleaseCapture();
                (state.on_gesture)("reset", 0.0, 0.0);
            }
            0
        }
        WM_MOUSEMOVE => {
            if !state_ptr.is_null() {
                use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE};
                let mut track=TRACKMOUSEEVENT {cbSize:std::mem::size_of::<TRACKMOUSEEVENT>() as u32,dwFlags:TME_LEAVE,hwndTrack:hwnd,dwHoverTime:0};
                TrackMouseEvent(&mut track);
                let state = &mut *state_ptr;
                let (x,y)=mouse_position(lparam);
                (state.on_gesture)("pointer_move",x as f32,y as f32);
                if let Some(previous) = state.drag_origin {
                    let current = mouse_position(lparam);
                    if state
                        .click_origin
                        .is_some_and(|(x, y)| (current.0 - x).abs() + (current.1 - y).abs() >= 4)
                    {
                        state.click_origin = None;
                    }
                    state.drag_origin = Some(current);
                    (state.on_gesture)(
                        if state.drag_pan { "pan" } else { "rotate" },
                        (current.0 - previous.0) as f32,
                        (current.1 - previous.1) as f32,
                    );
                }
            }
            0
        }
        0x02a3 /* WM_MOUSELEAVE */ => {
            if !state_ptr.is_null() {((*state_ptr).on_gesture)("pointer_leave",0.,0.);}
            0
        }
        WM_MOUSEWHEEL => {
            if !state_ptr.is_null() {
                let delta = ((wparam >> 16) as u16 as i16) as f32;
                let state = &mut *state_ptr;
                (state.on_gesture)("zoom", 0.0, -delta);
            }
            0
        }
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut paint);
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                let copied = StretchDIBits(
                    hdc,
                    0,
                    0,
                    state.canvas_width as i32,
                    state.canvas_height as i32,
                    0,
                    0,
                    state.frame_width as i32,
                    state.frame_height as i32,
                    state.pixels.as_ptr().cast::<c_void>(),
                    &state.bitmap,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                if copied > 0 && state.painted_sequence != state.frame_sequence {
                    state.painted_sequence = state.frame_sequence;
                }
            }
            EndPaint(hwnd, &paint);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn mouse_position(lparam: LPARAM) -> (i32, i32) {
    (
        (lparam as u32 & 0xffff) as u16 as i16 as i32,
        ((lparam as u32 >> 16) & 0xffff) as u16 as i16 as i32,
    )
}

/// Parent-side subclass forwarding wheel rotations to the presenter child
/// under the cursor. Only wheel messages are intercepted; everything else
/// continues to the Slint/winit window procedure unchanged. A hit forwards
/// with `SendMessageW` and swallows the original so Slint does not also
/// dispatch a scroll at its stale cursor position.
unsafe extern "system" fn parent_wheel_subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    child: usize,
) -> LRESULT {
    if msg == WM_MOUSEWHEEL {
        let child = child as HWND;
        if IsWindow(child) != 0 && IsWindowVisible(child) != 0 {
            let (x, y) = mouse_position(lparam);
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(child, &mut rect) != 0
                && x >= rect.left
                && x < rect.right
                && y >= rect.top
                && y < rect.bottom
            {
                SendMessageW(child, WM_MOUSEWHEEL, wparam, lparam);
                return 0;
            }
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};

    #[test]
    fn gdi_paint_smoke_test_preserves_rgba_colour() {
        // This is intentionally a real GDI test rather than a DIB metadata
        // assertion: it runs the native child through `WM_PAINT` and samples
        // the displayed client pixel. The desktop is used only as a transient
        // 2×2 parent; the presenter Drop implementation removes the child.
        register_class();
        let callback: GestureCallback = std::sync::Arc::new(|_, _, _| {});
        let presenter = ModelPresenter::new_for_parent(unsafe { GetDesktopWindow() }, callback)
            .expect("create native GDI smoke-test child");
        assert!(presenter.set_geometry(4, 4, 2, 2));
        let mut frame = PresenterFrame {
            width: 1,
            height: 1,
            rgba: vec![0x12, 0x34, 0x56, 0xff],
        };
        assert!(presenter.present(&mut frame));
        assert!(
            frame.rgba.is_empty(),
            "native canvas takes the pixel allocation"
        );
        assert_eq!(presenter.snapshot().unwrap().rgba, [0x12, 0x34, 0x56, 0xff]);
        unsafe {
            assert_ne!(UpdateWindow(presenter.hwnd), 0, "dispatch WM_PAINT");
            let hdc = GetDC(presenter.hwnd);
            assert_ne!(hdc, 0, "obtain child device context");
            // COLORREF is 0x00bbggrr. The presenter writes B,G,R,0 bytes for
            // BI_RGB, so the sampled colour must retain the input RGBA RGB.
            assert_eq!(GetPixel(hdc, 0, 0), 0x0056_3412);
            assert_ne!(ReleaseDC(presenter.hwnd, hdc), 0);
        }
    }

    #[test]
    fn revealing_hidden_frame_paints_without_a_second_submit() {
        register_class();
        let presenter = ModelPresenter::new_for_parent(unsafe { GetDesktopWindow() }, std::sync::Arc::new(|_,_,_|{})).unwrap();
        presenter.set_visible(false);
        assert!(presenter.set_geometry(4, 4, 2, 2));
        let mut frame = PresenterFrame { width:1, height:1, rgba:vec![0x21,0x43,0x65,0xff] };
        assert!(presenter.present(&mut frame));
        presenter.set_visible(true);
        unsafe {
            assert_eq!((*presenter.state).painted_sequence, (*presenter.state).frame_sequence);
            let hdc=GetDC(presenter.hwnd);
            assert_eq!(GetPixel(hdc,0,0),0x0065_4321);
            ReleaseDC(presenter.hwnd,hdc);
        }
    }

    #[test]
    fn native_pointer_distinguishes_click_drag_and_capture_loss() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_CAPTURECHANGED};
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = events.clone();
        register_class();
        let presenter = ModelPresenter::new_for_parent(
            unsafe { GetDesktopWindow() },
            std::sync::Arc::new(move |kind, x, y| output.lock().unwrap().push((kind, x, y))),
        )
        .unwrap();
        presenter.set_geometry(-10000, -10000, 128, 96);
        unsafe {
            SendMessageW(presenter.hwnd, WM_LBUTTONDOWN, 1, (20 << 16) | 30);
            SendMessageW(presenter.hwnd, WM_LBUTTONUP, 0, (20 << 16) | 30);
            SendMessageW(presenter.hwnd, WM_LBUTTONDOWN, 1, (20 << 16) | 30);
            SendMessageW(presenter.hwnd, WM_MOUSEMOVE, 1, (25 << 16) | 40);
            SendMessageW(presenter.hwnd, WM_LBUTTONUP, 0, (25 << 16) | 40);
            SendMessageW(presenter.hwnd, WM_LBUTTONDOWN, 1, (20 << 16) | 30);
            SendMessageW(presenter.hwnd, WM_CAPTURECHANGED, 0, 0);
            SendMessageW(presenter.hwnd, WM_MOUSEMOVE, 0, (30 << 16) | 50);
            ReleaseCapture();
        }
        let events = events.lock().unwrap();
        assert_eq!(events.iter().filter(|(k, _, _)| *k == "click").count(), 1);
        assert!(events.contains(&("click", 30., 20.)));
        assert_eq!(events.iter().filter(|(k, _, _)| *k == "rotate").count(), 1);
        assert!(events.contains(&("rotate", 10., 5.)));
    }

    #[test]
    fn parent_wheel_message_reaches_the_child_under_the_cursor() {
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetClientRect, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        };
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = events.clone();
        register_class();
        // A real visible top-level window plays the Slint focus window: wheel
        // messages arrive here and the subclass must forward them down.
        let parent = unsafe {
            CreateWindowExW(
                0,
                CLASS_NAME.as_ptr(),
                CLASS_NAME.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                24,
                24,
                320,
                240,
                0,
                0,
                GetModuleHandleW(null()),
                null(),
            )
        };
        assert_ne!(parent, 0, "create wheel-test parent window");
        let presenter = ModelPresenter::new_for_parent(
            parent,
            std::sync::Arc::new(move |kind, x, y| output.lock().unwrap().push((kind, x, y))),
        )
        .unwrap();
        presenter.set_geometry(0, 0, 64, 48);
        unsafe {
            let mut child_rect: RECT = std::mem::zeroed();
            assert_ne!(GetWindowRect(presenter.hwnd, &mut child_rect), 0);
            let inside = ((child_rect.left + 8) as u32) | (((child_rect.top + 8) as u32) << 16);
            SendMessageW(parent, WM_MOUSEWHEEL, ((120i32 as u32) << 16) as WPARAM, inside as LPARAM);
            // Outside the child (but still over the parent) must not forward.
            let mut client: RECT = std::mem::zeroed();
            assert_ne!(GetClientRect(parent, &mut client), 0);
            let mut corner = windows_sys::Win32::Foundation::POINT {
                x: client.right - 4,
                y: client.bottom - 4,
            };
            assert_ne!(ClientToScreen(parent, &mut corner), 0);
            let outside = (corner.x as u32) | ((corner.y as u32) << 16);
            SendMessageW(parent, WM_MOUSEWHEEL, ((120i32 as u32) << 16) as WPARAM, outside as LPARAM);
        }
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[("zoom", 0., -120.)],
            "exactly one forwarded wheel rotation"
        );
        drop(presenter);
        unsafe {
            DestroyWindow(parent);
        }
    }
}

