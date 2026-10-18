//! X11 software presenter.
//!
//! This uses an Xlib child window and `XImage`/`XPutImage`, never a GPU
//! surface. Wayland is intentionally excluded: it has no portable foreign
//! child-surface primitive, so the normal Slint image path remains active.

use super::fallback::{rgba_to_bgra_in_place, GestureCallback, PresenterFrame};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::ffi::{c_char, c_int, c_ulong};
use std::ptr::null_mut;
use std::sync::Mutex;
use x11_dl::xlib;

struct State {
    display: *mut xlib::Display,
    window: xlib::Window,
    gc: xlib::GC,
    colormap: xlib::Colormap,
    owns_colormap: bool,
    image: *mut xlib::XImage,
    visual: *mut xlib::Visual,
    depth: c_int,
    pixels: Vec<u8>,
    canvas_width: u32,
    canvas_height: u32,
    position: Option<(i32, i32)>,
    visible: bool,
    frame_width: u32,
    frame_height: u32,
    stride: usize,
    red_mask: c_ulong,
    green_mask: c_ulong,
    blue_mask: c_ulong,
    byte_order: c_int,
    pixel_layout: PixelLayout,
}

#[derive(Clone, Copy)]
enum PixelLayout {
    /// The common little-endian 0x00RRGGBB X11 visual.
    BgraLittle,
    /// The common big-endian 0x00RRGGBB X11 visual.
    ArgbBig,
    /// A less common visual requiring mask scaling.
    Generic,
}

unsafe impl Send for State {}

/// Xlib child-window presenter. It is created and used only from Slint's UI
/// event-loop thread. Rendering workers hand it immutable byte frames only.
pub struct ModelPresenter {
    xlib: xlib::Xlib,
    state: Mutex<State>,
}

impl ModelPresenter {
    pub fn new(window: &slint::Window, on_gesture: GestureCallback) -> Option<Self> {
        // Xlib child windows cannot be embedded in a native Wayland Slint
        // surface. Do not infer X11 merely from DISPLAY: Xwayland's display
        // is not the parent surface's display connection.
        let native_window = window.window_handle();
        let native_display = window.window_handle();
        let parent =
            native_window
                .window_handle()
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    RawWindowHandle::Xlib(handle) => Some(handle.window),
                    _ => None,
                })?;
        let display = native_display
            .display_handle()
            .ok()
            .and_then(|handle| match handle.as_raw() {
                RawDisplayHandle::Xlib(handle) => handle.display.map(|display| display.as_ptr()),
                _ => None,
            })?
            .cast::<xlib::Display>();
        Self::new_for_parent(display, parent, on_gesture)
    }

    /// Build a child surface under an already-known Xlib parent. Keeping this
    /// separate from [`Self::new`] lets the X11 smoke test use the same visual,
    /// XImage and XPutImage path without having to start a full Slint window.
    ///
    /// The gesture callback is accepted for API parity with the Win32
    /// presenter but intentionally unused: the child selects no pointer input,
    /// so X11 propagates drags and wheel scrolls to the Winit parent window
    /// and the Slint `Echo3dOverlay` TouchArea drives the camera. Selecting
    /// input here instead loses the race against Winit's full-queue drain,
    /// which is why dragging only worked intermittently before.
    fn new_for_parent(
        display: *mut xlib::Display,
        parent: xlib::Window,
        _on_gesture: GestureCallback,
    ) -> Option<Self> {
        let xlib = xlib::Xlib::open().ok()?;
        let mut parent_attributes: xlib::XWindowAttributes = unsafe { std::mem::zeroed() };
        if unsafe { (xlib.XGetWindowAttributes)(display, parent, &mut parent_attributes) } == 0
            || parent_attributes.visual.is_null()
        {
            return None;
        }
        // A Winit parent can use a visual that differs from the default X11
        // visual. The child and XImage must use that exact visual/depth.
        let visual = parent_attributes.visual;
        let depth = parent_attributes.depth;
        // `XCreateSimpleWindow` always chooses CopyFromParent/default visual.
        // Winit can pick a non-default TrueColor visual, in which case that
        // helper creates an incompatible child and later XPutImage can fail
        // with BadMatch. Make the child explicitly use the parent's visual
        // and a private colormap for that visual.
        let (colormap, owns_colormap) = if parent_attributes.colormap != 0 {
            (parent_attributes.colormap, false)
        } else {
            let colormap =
                unsafe { (xlib.XCreateColormap)(display, parent, visual, xlib::AllocNone) };
            if colormap == 0 {
                return None;
            }
            (colormap, true)
        };
        let mut attributes = xlib::XSetWindowAttributes {
            background_pixmap: 0,
            // On a depth-32 ARGB visual a zero background is transparent
            // black; fill the alpha bits so the mapped child is never
            // invisible before its first frame.
            background_pixel: alpha_fill_bits(
                unsafe { (*visual).red_mask | (*visual).green_mask | (*visual).blue_mask },
                depth,
            ),
            border_pixmap: 0,
            border_pixel: 0,
            bit_gravity: 0,
            win_gravity: 0,
            backing_store: 0,
            backing_planes: 0,
            backing_pixel: 0,
            save_under: 0,
            event_mask: 0,
            do_not_propagate_mask: 0,
            override_redirect: 0,
            colormap,
            cursor: 0,
        };
        let child = unsafe {
            (xlib.XCreateWindow)(
                display,
                parent,
                0,
                0,
                1,
                1,
                0,
                depth,
                xlib::InputOutput as u32,
                visual,
                xlib::CWBackPixel | xlib::CWBorderPixel | xlib::CWColormap,
                &mut attributes,
            )
        };
        if child == 0 {
            if owns_colormap {
                unsafe { (xlib.XFreeColormap)(display, colormap) };
            }
            return None;
        }
        let gc = unsafe { (xlib.XCreateGC)(display, child, 0, null_mut()) };
        if gc.is_null() {
            unsafe {
                (xlib.XDestroyWindow)(display, child);
                if owns_colormap {
                    (xlib.XFreeColormap)(display, colormap);
                }
            }
            return None;
        }
        unsafe {
            // Only structural/exposure events are selected. Pointer input is
            // deliberately left unselected so it propagates to the Winit
            // parent window and reaches the Slint overlay: Winit drains the
            // shared Xlib queue with an accept-all predicate on every event
            // loop wake, so events selected here were silently swallowed
            // before the 8 ms pump could fetch them.
            (xlib.XSelectInput)(
                display,
                child,
                xlib::ExposureMask | xlib::StructureNotifyMask,
            );
            (xlib.XMapWindow)(display, child);
            (xlib.XFlush)(display);
        }
        let mut presenter = Self {
            xlib,
            state: Mutex::new(State {
                display,
                window: child,
                gc,
                colormap,
                owns_colormap,
                image: null_mut(),
                visual,
                depth,
                pixels: Vec::new(),
                canvas_width: 0,
                canvas_height: 0,
                position: None,
                visible: true,
                frame_width: 0,
                frame_height: 0,
                stride: 0,
                red_mask: 0,
                green_mask: 0,
                blue_mask: 0,
                byte_order: xlib::LSBFirst,
                pixel_layout: PixelLayout::Generic,
            }),
        };
        if !presenter.rebuild_image(1, 1) {
            return None;
        }
        Some(presenter)
    }

    pub fn set_geometry(&self, x: i32, y: i32, width: u32, height: u32) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let width = width.max(1);
        let height = height.max(1);
        if state.position == Some((x, y)) && state.canvas_width == width && state.canvas_height == height {
            return !state.image.is_null();
        }
        unsafe { (self.xlib.XMoveResizeWindow)(state.display, state.window, x, y, width, height); }
        state.position = Some((x, y));
        let ready = if state.canvas_width != width || state.canvas_height != height {
            self.rebuild_image_locked(&mut state, width, height)
        } else {
            !state.image.is_null()
        };
        unsafe { (self.xlib.XFlush)(state.display) };
        ready
    }

    pub fn set_visible(&self, visible: bool) {
        let Ok(mut state) = self.state.lock() else { return };
        if state.visible == visible { return; }
        state.visible = visible;
        unsafe {
            if visible {
                (self.xlib.XMapWindow)(state.display, state.window);
            } else {
                (self.xlib.XUnmapWindow)(state.display, state.window);
            }
            (self.xlib.XFlush)(state.display);
        }
    }

    pub fn present(&self, frame: &mut PresenterFrame) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let width = frame.width.max(1);
        let height = frame.height.max(1);
        let expected = width as usize * height as usize * 4;
        if frame.rgba.len() != expected {
            return false;
        }
        // A failed resize/visual negotiation deliberately leaves the native
        // canvas dimensions at zero. Do not recreate a tiny image here and
        // claim success: the caller must switch back to Slint Image until a
        // later geometry callback can rebuild the native surface.
        if state.canvas_width == 0 || state.canvas_height == 0 {
            return false;
        }
        if state.image.is_null() {
            let canvas_width = state.canvas_width.max(1);
            let canvas_height = state.canvas_height.max(1);
            if !self.rebuild_image_locked(&mut state, canvas_width, canvas_height) {
                return false;
            }
        }
        state.frame_width = width;
        state.frame_height = height;
        // XPutImage does not scale. Resample into the persistent native image
        // so the child preserves Slint Image's fill behavior for a
        // rectangular canvas and a square raster frame.
        let byte_order = state.byte_order;
        let pixel_layout = state.pixel_layout;
        let red_mask = state.red_mask;
        let green_mask = state.green_mask;
        let blue_mask = state.blue_mask;
        let opaque_bits = alpha_fill_bits(red_mask | green_mask | blue_mask, state.depth);
        let same_size = width == state.canvas_width && height == state.canvas_height;
        if same_size
            && matches!(pixel_layout, PixelLayout::BgraLittle)
            && state.stride == width as usize * 4
        {
            if !rgba_to_bgra_in_place(&mut frame.rgba, 0xff) {
                return false;
            }
            state.pixels = std::mem::take(&mut frame.rgba);
            // The XImage borrows this Vec's allocation. A new worker frame
            // has just replaced that allocation, so retarget it before any
            // Xlib call can read the pixels.
            unsafe { (*state.image).data = state.pixels.as_mut_ptr().cast::<c_char>() };
        } else if same_size && matches!(pixel_layout, PixelLayout::ArgbBig) {
            for (src, dst) in frame
                .rgba
                .chunks_exact(4)
                .zip(state.pixels.chunks_exact_mut(4))
            {
                dst.copy_from_slice(&[0xff, src[0], src[1], src[2]]);
            }
        } else {
            for y in 0..state.canvas_height {
                let source_y = (y as u64 * height as u64 / state.canvas_height as u64) as u32;
                let source_row = source_y.min(height - 1) as usize * width as usize * 4;
                let row_start = y as usize * state.stride;
                for x in 0..state.canvas_width {
                    let source_x = (x as u64 * width as u64 / state.canvas_width as u64) as u32;
                    let source = source_row + source_x.min(width - 1) as usize * 4;
                    let dst = &mut state.pixels
                        [row_start + x as usize * 4..row_start + x as usize * 4 + 4];
                    if matches!(pixel_layout, PixelLayout::BgraLittle) {
                        dst.copy_from_slice(&[
                            frame.rgba[source + 2],
                            frame.rgba[source + 1],
                            frame.rgba[source],
                            0xff,
                        ]);
                    } else if matches!(pixel_layout, PixelLayout::ArgbBig) {
                        dst.copy_from_slice(&[
                            0xff,
                            frame.rgba[source],
                            frame.rgba[source + 1],
                            frame.rgba[source + 2],
                        ]);
                    } else {
                        let packed = pack_pixel(
                            &frame.rgba[source..source + 4],
                            red_mask,
                            green_mask,
                            blue_mask,
                        ) | opaque_bits;
                        write_pixel(dst, packed, byte_order);
                    }
                }
            }
        }
        unsafe {
            (self.xlib.XPutImage)(
                state.display,
                state.window,
                state.gc,
                state.image,
                0,
                0,
                0,
                0,
                state.canvas_width,
                state.canvas_height,
            );
            (self.xlib.XFlush)(state.display);
        }
        true
    }

    pub fn snapshot(&self) -> Option<PresenterFrame> {
        let state = self.state.lock().ok()?;
        let mut rgba =
            Vec::with_capacity(state.canvas_width as usize * state.canvas_height as usize * 4);
        for y in 0..state.canvas_height as usize {
            for x in 0..state.canvas_width as usize {
                let offset = y * state.stride + x * 4;
                let bytes: [u8; 4] = state.pixels.get(offset..offset + 4)?.try_into().ok()?;
                let packed = if state.byte_order == xlib::MSBFirst {
                    u32::from_be_bytes(bytes)
                } else {
                    u32::from_le_bytes(bytes)
                } as c_ulong;
                for mask in [state.red_mask, state.green_mask, state.blue_mask] {
                    let v = if mask == 0 {
                        0
                    } else {
                        (((packed & mask) >> mask.trailing_zeros()) as u64 * 255
                            / (mask >> mask.trailing_zeros()) as u64) as u8
                    };
                    rgba.push(v);
                }
                rgba.push(255);
            }
        }
        Some(PresenterFrame {
            width: state.canvas_width,
            height: state.canvas_height,
            rgba,
        })
    }

    /// Submit an already BGRA-converted frame through the native X11 path.
    /// This is used by the cross-platform benchmark so the measured submit
    /// stage does not include a second channel conversion. Production callers
    /// use [`Self::present`] with the Rust renderer's RGBA ownership.
    fn present_bgra(&self, width: u32, height: u32, pixels: Vec<u8>) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let width = width.max(1);
        let height = height.max(1);
        if pixels.len() != width as usize * height as usize * 4
            || state.canvas_width == 0
            || state.canvas_height == 0
        {
            return false;
        }
        if state.image.is_null() {
            let canvas_width = state.canvas_width.max(1);
            let canvas_height = state.canvas_height.max(1);
            if !self.rebuild_image_locked(&mut state, canvas_width, canvas_height) {
                return false;
            }
        }
        state.frame_width = width;
        state.frame_height = height;
        let same_size = width == state.canvas_width && height == state.canvas_height;
        if same_size
            && matches!(state.pixel_layout, PixelLayout::BgraLittle)
            && state.stride == width as usize * 4
        {
            state.pixels = pixels;
            unsafe {
                (*state.image).data = state.pixels.as_mut_ptr().cast::<c_char>();
            }
        } else {
            let byte_order = state.byte_order;
            let pixel_layout = state.pixel_layout;
            let red_mask = state.red_mask;
            let green_mask = state.green_mask;
            let blue_mask = state.blue_mask;
            let depth = state.depth;
            for y in 0..state.canvas_height {
                let source_y = (y as u64 * height as u64 / state.canvas_height as u64) as u32;
                let source_row = source_y.min(height - 1) as usize * width as usize * 4;
                let row_start = y as usize * state.stride;
                for x in 0..state.canvas_width {
                    let source_x = (x as u64 * width as u64 / state.canvas_width as u64) as u32;
                    let source = source_row + source_x.min(width - 1) as usize * 4;
                    let src = &pixels[source..source + 4];
                    let dst = &mut state.pixels
                        [row_start + x as usize * 4..row_start + x as usize * 4 + 4];
                    if matches!(pixel_layout, PixelLayout::BgraLittle) {
                        dst.copy_from_slice(&[src[0], src[1], src[2], 0xff]);
                    } else if matches!(pixel_layout, PixelLayout::ArgbBig) {
                        dst.copy_from_slice(&[0xff, src[2], src[1], src[0]]);
                    } else {
                        let rgba = [src[2], src[1], src[0], 0xff];
                        let packed = pack_pixel(&rgba, red_mask, green_mask, blue_mask)
                            | alpha_fill_bits(red_mask | green_mask | blue_mask, depth);
                        write_pixel(dst, packed, byte_order);
                    }
                }
            }
        }
        unsafe {
            (self.xlib.XPutImage)(
                state.display,
                state.window,
                state.gc,
                state.image,
                0,
                0,
                0,
                0,
                state.canvas_width,
                state.canvas_height,
            );
            (self.xlib.XFlush)(state.display);
        }
        true
    }

    /// Dispatch queued X11 events. Winit still owns the parent event loop;
    /// this drains only the child window's messages after each 3D frame.
    /// Only Expose arrives here: pointer input is not selected on the child
    /// (see `new_for_parent`), so Slint's overlay handles camera gestures.
    pub fn pump_events(&self) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        unsafe {
            // XCheckWindowEvent filters by child window, so this never steals
            // Winit/Slint events from the shared X connection (unlike
            // XNextEvent, which would consume the parent's queue).
            let mut event: xlib::XEvent = std::mem::zeroed();
            while (self.xlib.XCheckWindowEvent)(
                state.display,
                state.window,
                xlib::ExposureMask,
                &mut event,
            ) != 0
            {
                if event.get_type() == xlib::Expose && !state.image.is_null() {
                    (self.xlib.XPutImage)(
                        state.display,
                        state.window,
                        state.gc,
                        state.image,
                        0,
                        0,
                        0,
                        0,
                        state.canvas_width,
                        state.canvas_height,
                    );
                }
            }
        }
    }

    fn rebuild_image(&mut self, width: u32, height: u32) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        self.rebuild_image_locked(&mut state, width, height)
    }

    fn rebuild_image_locked(&self, state: &mut State, width: u32, height: u32) -> bool {
        unsafe {
            // Keep the previous frame across the rebuild: right after a
            // resize the compositor can expose the child before the worker
            // delivers the next frame, and a zeroed buffer is fully
            // transparent on a depth-32 ARGB visual — that is what made the
            // canvas flash or stay blank on Linux.
            let previous = (!state.image.is_null()
                && state.canvas_width > 0
                && state.canvas_height > 0)
                .then(|| {
                    (
                        std::mem::take(&mut state.pixels),
                        state.canvas_width as usize,
                        state.canvas_height as usize,
                        state.stride,
                    )
                });
            self.destroy_image_locked(state);
            let visual = state.visual;
            if visual.is_null() || (state.depth != 24 && state.depth != 32) {
                state.canvas_width = 0;
                state.canvas_height = 0;
                return false;
            }
            // First ask Xlib for the actual scanline stride. It may include
            // padding beyond width*4, and allocating the packed width alone
            // would make XPutImage write past the Vec on unusual servers.
            let image = (self.xlib.XCreateImage)(
                state.display,
                visual,
                state.depth as u32,
                xlib::ZPixmap,
                0,
                null_mut(),
                width,
                height,
                32,
                0,
            );
            if image.is_null() {
                state.canvas_width = 0;
                state.canvas_height = 0;
                return false;
            }
            if (*image).bits_per_pixel != 32
                || (*image).bytes_per_line < (width * 4) as c_int
                || (*image).bytes_per_line <= 0
            {
                (self.xlib.XDestroyImage)(image);
                state.canvas_width = 0;
                state.canvas_height = 0;
                return false;
            }
            let stride = (*image).bytes_per_line as usize;
            let allocation = stride.checked_mul(height as usize);
            let Some(allocation) = allocation else {
                (self.xlib.XDestroyImage)(image);
                state.canvas_width = 0;
                state.canvas_height = 0;
                return false;
            };
            // Same visual and depth on both buffers, so the old pixels can be
            // copied verbatim with nearest-neighbour resampling into the new
            // stride instead of leaving a transparent zeroed surface.
            let mut pixels = vec![0; allocation];
            if let Some((old, old_width, old_height, old_stride)) = previous {
                for y in 0..height as usize {
                    let source_row = (y * old_height / height as usize) * old_stride;
                    let row_start = y * stride;
                    for x in 0..width as usize {
                        let source = source_row + (x * old_width / width as usize) * 4;
                        let dst = row_start + x * 4;
                        pixels[dst..dst + 4].copy_from_slice(&old[source..source + 4]);
                    }
                }
            }
            state.pixels = pixels;
            (*image).data = state.pixels.as_mut_ptr().cast::<c_char>();
            state.image = image;
            state.canvas_width = width;
            state.canvas_height = height;
            state.stride = stride;
            state.red_mask = (*image).red_mask;
            state.green_mask = (*image).green_mask;
            state.blue_mask = (*image).blue_mask;
            state.byte_order = (*image).byte_order;
            state.pixel_layout = pixel_layout(
                state.red_mask,
                state.green_mask,
                state.blue_mask,
                state.byte_order,
            );
            true
        }
    }

    unsafe fn destroy_image_locked(&self, state: &mut State) {
        if !state.image.is_null() {
            // XDestroyImage normally frees `data`; our Vec remains its owner.
            (*state.image).data = null_mut();
            (self.xlib.XDestroyImage)(state.image);
            state.image = null_mut();
        }
    }
}

/// A real X11 child-window target for the command-line native-present
/// benchmark. The display is opened only for this temporary benchmark and is
/// closed after the presenter has destroyed its child resources.
pub(crate) struct X11PresentationBenchmark {
    presenter: Option<ModelPresenter>,
    display: *mut xlib::Display,
}

impl X11PresentationBenchmark {
    pub(crate) fn new(width: u32, height: u32) -> Option<Self> {
        let xlib = xlib::Xlib::open().ok()?;
        let display = unsafe { (xlib.XOpenDisplay)(null_mut()) };
        if display.is_null() {
            return None;
        }
        let screen = unsafe { (xlib.XDefaultScreen)(display) };
        let root = unsafe { (xlib.XRootWindow)(display, screen) };
        let callback: GestureCallback = std::sync::Arc::new(|_, _, _| {});
        let presenter = match ModelPresenter::new_for_parent(display, root, callback) {
            Some(presenter) => presenter,
            None => {
                unsafe { (xlib.XCloseDisplay)(display) };
                return None;
            }
        };
        if !presenter.set_geometry(-10_000, -10_000, width.max(1), height.max(1)) {
            drop(presenter);
            unsafe { (xlib.XCloseDisplay)(display) };
            return None;
        }
        Some(Self {
            presenter: Some(presenter),
            display,
        })
    }

    pub(crate) fn present_bgra(&self, width: u32, height: u32, pixels: Vec<u8>) -> bool {
        let Some(presenter) = self.presenter.as_ref() else {
            return false;
        };
        if !presenter.present_bgra(width, height, pixels) {
            return false;
        }
        // `XFlush` only queues the request to the server. A benchmark must
        // wait for X11 to process the XPutImage copy, otherwise it measures a
        // client-side queue append rather than native presentation work.
        unsafe { (presenter.xlib.XSync)(self.display, 0) != 0 }
    }
}

impl Drop for X11PresentationBenchmark {
    fn drop(&mut self) {
        // Explicitly destroy the presenter before closing the temporary
        // benchmark display connection. `Option::take` prevents Rust from
        // dropping the presenter a second time after this method returns.
        let _ = self.presenter.take();
        if let Ok(xlib) = xlib::Xlib::open() {
            unsafe {
                (xlib.XCloseDisplay)(self.display);
            }
        }
    }
}

impl Drop for ModelPresenter {
    fn drop(&mut self) {
        unsafe {
            let state = self.state.get_mut().expect("presenter state poisoned");
            if !state.image.is_null() {
                (*state.image).data = null_mut();
                (self.xlib.XDestroyImage)(state.image);
                state.image = null_mut();
            }
            (self.xlib.XFreeGC)(state.display, state.gc);
            (self.xlib.XDestroyWindow)(state.display, state.window);
            if state.owns_colormap {
                (self.xlib.XFreeColormap)(state.display, state.colormap);
            }
            (self.xlib.XFlush)(state.display);
        }
    }
}

fn pack_pixel(rgba: &[u8], red_mask: c_ulong, green_mask: c_ulong, blue_mask: c_ulong) -> c_ulong {
    channel_to_mask(rgba[0], red_mask)
        | channel_to_mask(rgba[1], green_mask)
        | channel_to_mask(rgba[2], blue_mask)
}

/// Bits outside the color masks are dead padding for depth<=24 visuals, but
/// they are the per-pixel alpha of a depth-32 ARGB visual. A composited child
/// window whose alpha bits stay zero is fully transparent, so always set them.
fn alpha_fill_bits(color_masks: c_ulong, depth: c_int) -> c_ulong {
    let depth_bits: c_ulong = if depth >= 32 {
        u32::MAX as c_ulong
    } else {
        ((1u64 << depth.max(0).min(31)) - 1) as c_ulong
    };
    !color_masks & depth_bits
}

fn channel_to_mask(value: u8, mask: c_ulong) -> c_ulong {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    ((u64::from(value) * max as u64 + 127) / 255 << shift) as c_ulong
}

fn write_pixel(dst: &mut [u8], pixel: c_ulong, byte_order: c_int) {
    let bytes = if byte_order == xlib::MSBFirst {
        (pixel as u32).to_be_bytes()
    } else {
        (pixel as u32).to_le_bytes()
    };
    dst.copy_from_slice(&bytes);
}

fn pixel_layout(
    red_mask: c_ulong,
    green_mask: c_ulong,
    blue_mask: c_ulong,
    byte_order: c_int,
) -> PixelLayout {
    if byte_order == xlib::LSBFirst
        && red_mask == 0x00ff_0000
        && green_mask == 0x0000_ff00
        && blue_mask == 0x0000_00ff
    {
        PixelLayout::BgraLittle
    } else if byte_order == xlib::MSBFirst
        && red_mask == 0x00ff_0000
        && green_mask == 0x0000_ff00
        && blue_mask == 0x0000_00ff
    {
        PixelLayout::ArgbBig
    } else {
        PixelLayout::Generic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null;

    #[test]
    fn rgb_masks_pack_channels_without_assuming_bgrx() {
        let pixel = pack_pixel(
            &[0x12, 0x34, 0x56, 0xff],
            0x00ff_0000,
            0x0000_ff00,
            0x0000_00ff,
        );
        assert_eq!(pixel, 0x0012_3456);
    }

    #[test]
    fn rgb565_masks_scale_eight_bit_channels() {
        let pixel = pack_pixel(&[255, 128, 0, 255], 0xf800, 0x07e0, 0x001f);
        assert_eq!(pixel, 0xfc00);
    }

    #[test]
    fn alpha_fill_bits_only_target_depth32_visuals() {
        let true_color = 0x00ff_0000 | 0x0000_ff00 | 0x0000_00ff;
        assert_eq!(alpha_fill_bits(true_color, 32), 0xff00_0000);
        assert_eq!(alpha_fill_bits(true_color, 24), 0);
        assert_eq!(alpha_fill_bits(0xf800 | 0x07e0 | 0x001f, 16), 0);
    }

    #[test]
    fn byte_order_controls_native_memory_layout() {
        let mut little = [0; 4];
        let mut big = [0; 4];
        write_pixel(&mut little, 0x0012_3456, xlib::LSBFirst);
        write_pixel(&mut big, 0x0012_3456, xlib::MSBFirst);
        assert_eq!(little, [0x56, 0x34, 0x12, 0]);
        assert_eq!(big, [0, 0x12, 0x34, 0x56]);
    }

    #[test]
    fn common_visuals_use_direct_layouts() {
        assert!(matches!(
            pixel_layout(0x00ff_0000, 0x0000_ff00, 0x0000_00ff, xlib::LSBFirst),
            PixelLayout::BgraLittle
        ));
        assert!(matches!(
            pixel_layout(0x00ff_0000, 0x0000_ff00, 0x0000_00ff, xlib::MSBFirst),
            PixelLayout::ArgbBig
        ));
    }

    #[test]
    fn xputimage_smoke_test_preserves_rgba_colour_when_x11_is_available() {
        // CI and Windows development machines often have no X server. The
        // platform-independent packing tests above still run there; this one
        // exercises the real X11 path when invoked under Ubuntu X11 or Xvfb.
        let xlib = xlib::Xlib::open().expect("load Xlib");
        let display = unsafe { (xlib.XOpenDisplay)(null()) };
        if display.is_null() {
            eprintln!("skipping X11 smoke test: DISPLAY is unavailable");
            return;
        }
        unsafe {
            let screen = (xlib.XDefaultScreen)(display);
            let root = (xlib.XRootWindow)(display, screen);
            let callback: GestureCallback = std::sync::Arc::new(|_, _, _| {});
            let presenter = ModelPresenter::new_for_parent(display, root, callback)
                .expect("create native X11 smoke-test child");
            assert!(presenter.set_geometry(4, 4, 2, 2));
            let mut frame = PresenterFrame {
                width: 1,
                height: 1,
                rgba: vec![0x12, 0x34, 0x56, 0xff],
            };
            assert!(presenter.present(&mut frame));
            (xlib.XSync)(display, 0);

            let child = presenter.state.lock().expect("presenter state").window;
            let image = (xlib.XGetImage)(display, child, 0, 0, 1, 1, !0 as c_ulong, xlib::ZPixmap);
            assert!(!image.is_null(), "read native X11 child pixel");
            let pixel = (xlib.XGetPixel)(image, 0, 0);
            (xlib.XDestroyImage)(image);
            // Common X11 TrueColor uses 0x00RRGGBB. Apply the discovered
            // masks rather than hard-coding byte order so a non-default
            // visual validates the same channel semantics.
            let state = presenter.state.lock().expect("presenter state");
            assert_eq!(
                pixel & state.red_mask,
                channel_to_mask(0x12, state.red_mask)
            );
            assert_eq!(
                pixel & state.green_mask,
                channel_to_mask(0x34, state.green_mask)
            );
            assert_eq!(
                pixel & state.blue_mask,
                channel_to_mask(0x56, state.blue_mask)
            );
            drop(state);
            drop(presenter);
            (xlib.XCloseDisplay)(display);
        }
    }
}

