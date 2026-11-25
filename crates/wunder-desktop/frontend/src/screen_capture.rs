//! Small Win7-safe screen capture primitive used by the tray screenshot action.
//! The UI receives pixels only after the blocking GDI call and PNG encoding
//! complete on a worker thread.
//!
//! The captured rectangle is reported back with its desktop origin so the
//! selector window can be placed at exactly that origin and size. Selector
//! coverage and captured pixels therefore always describe the same physical
//! region, which is what keeps a region capture from silently dropping the
//! bottom (or right) of the monitor.

/// Desktop rectangle covered by a capture, in physical pixels. Only the GDI
/// capture path constructs and measures it, so on non-Windows builds it is
/// declared but unused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct MonitorRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl MonitorRect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// Reject degenerate rectangles before any GDI allocation happens.
    pub fn is_usable(&self) -> bool {
        self.width() > 0 && self.height() > 0
    }

    /// Overlap of two desktop rectangles. Empty overlaps come back as `None`
    /// exactly like degenerate monitors, so callers can treat "the window does
    /// not touch the capture monitor" as "nothing to verify".
    pub fn intersection(&self, other: &MonitorRect) -> Option<MonitorRect> {
        let rect = MonitorRect {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        rect.is_usable().then_some(rect)
    }
}

pub struct ScreenCapture {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Physical desktop origin of the captured rectangle. The selector window
    /// is positioned here with exactly `width` x `height`.
    pub origin_x: i32,
    pub origin_y: i32,
}

#[cfg(windows)]
pub fn capture_screen() -> Result<ScreenCapture, String> {
    use windows_sys::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
    let monitor = primary_monitor().ok_or("无法读取屏幕尺寸")?;
    unsafe {
        // The desktop DC origin is the virtual-screen top-left, and monitor
        // rectangles share that coordinate space, so the rect doubles as the
        // BitBlt source offset.
        let dc = GetDC(0);
        if dc == 0 {
            return Err("无法获取屏幕设备上下文".into());
        }
        let result = capture_from_dc(dc, monitor);
        ReleaseDC(0, dc);
        result
    }
}

/// `MONITORINFOF_PRIMARY`. windows-sys 0.52 exposes no name for it.
#[cfg(windows)]
const MONITORINFOF_PRIMARY: u32 = 1;

/// Locate the monitor the selector will cover. The primary monitor is
/// preferred so a secondary panel placed left of or above the desktop origin
/// cannot shift the capture; the first enumerated monitor is the fallback.
#[cfg(windows)]
pub(crate) fn primary_monitor() -> Option<MonitorRect> {
    use windows_sys::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };

    // `EnumDisplayMonitors` is synchronous, so this out-parameter stack slot
    // stays alive for the whole call.
    let mut state: (Option<MonitorRect>, Option<MonitorRect>) = (None, None);

    unsafe extern "system" fn visit(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let state = &mut *(data as *mut (Option<MonitorRect>, Option<MonitorRect>));
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info as *mut MONITORINFO) == 0 {
            return 1;
        }
        let rect = MonitorRect {
            left: info.rcMonitor.left,
            top: info.rcMonitor.top,
            right: info.rcMonitor.right,
            bottom: info.rcMonitor.bottom,
        };
        if !rect.is_usable() {
            return 1;
        }
        if info.dwFlags & MONITORINFOF_PRIMARY != 0 {
            state.0 = Some(rect);
        } else if state.1.is_none() {
            state.1 = Some(rect);
        }
        1
    }

    unsafe {
        EnumDisplayMonitors(
            0,
            std::ptr::null(),
            // `MONITORENUMPROC` is already `Option<fn>`, so the callback is
            // passed directly and coerced at the call site.
            Some(visit),
            &mut state as *mut _ as LPARAM,
        );
    }
    state.0.or(state.1)
}

/// One top-level window rectangle, in absolute desktop pixels (the same space
/// as [`ScreenCapture::origin_x`]). Candidates come back in stacking order —
/// topmost first — so a hit test can stop at the first rectangle that contains
/// the pointer: that is the window the user actually sees there.
///
/// Only what Win32 reports is used, never a painted-screen scan: the selector
/// needs the rectangles just for snapping and highlighting, while the pixels
/// of the region still come from the frozen frame the user confirms.
#[cfg(windows)]
pub fn detect_regions(monitor: MonitorRect) -> Vec<MonitorRect> {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowRect, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible,
    };

    struct Scan {
        monitor: MonitorRect,
        own_pid: u32,
        regions: Vec<MonitorRect>,
    }

    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        let scan = &mut *(data as *mut Scan);
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return 1;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 || pid == scan.own_pid {
            // Our own surfaces are never a capture target, and the selector
            // itself must not offer itself as a snap region.
            return 1;
        }
        let mut class = [0u16; 64];
        let class_len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..class_len.max(0) as usize]);
        if matches!(class.as_str(), "Progman" | "WorkerW") {
            // Desktop background windows cover the whole monitor; offering
            // them would turn every click on empty desktop into a full-screen
            // selection, which is the one case free dragging already serves.
            return 1;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut u32 as *mut std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        ) == 0
            && cloaked != 0
        {
            // UWP leftovers and windows on another virtual desktop are
            // "visible" to Win32 but contribute no pixels to this screen.
            return 1;
        }
        let mut rect: RECT = std::mem::zeroed();
        let mut resolved = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut rect as *mut RECT as *mut std::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        ) == 0;
        if !resolved {
            // Without composition (Win7 basic theme) the classic rectangle,
            // invisible resize border included, is the only answer.
            resolved = GetWindowRect(hwnd, &mut rect) != 0;
        }
        if !resolved {
            return 1;
        }
        let candidate = MonitorRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        if let Some(clipped) = candidate.intersection(&scan.monitor) {
            // Slivers are tooltips and 1px caret windows: they are never what
            // the user means, and snapping to them is pure frustration.
            if clipped.width() >= 8 && clipped.height() >= 8 && scan.regions.len() < 64 {
                scan.regions.push(clipped);
            }
        }
        1
    }

    let mut scan = Scan {
        monitor,
        own_pid: std::process::id(),
        regions: Vec::new(),
    };
    unsafe { EnumWindows(Some(visit), &mut scan as *mut Scan as LPARAM) };
    scan.regions
}

/// X11 counterpart: enumerate the root window's children and read each frame's
/// geometry. Wayland forbids this kind of screen introspection, so a session
/// without a usable X display simply gets an empty list — the selector still
/// supports free dragging and handle adjustment.
#[cfg(target_os = "linux")]
pub fn detect_regions(monitor: MonitorRect) -> Vec<MonitorRect> {
    use std::ffi::c_void;
    use std::os::raw::{c_int, c_uint, c_ulong};
    use x11_dl::xlib;

    let Ok(xlib) = xlib::Xlib::open() else {
        return Vec::new();
    };
    unsafe {
        let display = (xlib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return Vec::new();
        }
        let screen = (xlib.XDefaultScreen)(display);
        let root = (xlib.XRootWindow)(display, screen);
        let mut root_return: c_ulong = 0;
        let mut parent_return: c_ulong = 0;
        let mut children: *mut c_ulong = std::ptr::null_mut();
        let mut count: c_uint = 0;
        let mut regions = Vec::new();
        if (xlib.XQueryTree)(
            display,
            root,
            &mut root_return,
            &mut parent_return,
            &mut children,
            &mut count,
        ) != 0
            && !children.is_null()
        {
            let stack = std::slice::from_raw_parts(children, count as usize);
            // `XQueryTree` lists children bottom-to-top. Walk it backwards so
            // the first hit is the window the user actually sees.
            for &window in stack.iter().rev() {
                let mut attrs: xlib::XWindowAttributes = std::mem::zeroed();
                if (xlib.XGetWindowAttributes)(display, window, &mut attrs) == 0 {
                    continue;
                }
                if attrs.map_state != xlib::IsViewable
                    || attrs.class != xlib::InputOutput
                    || attrs.width < 8
                    || attrs.height < 8
                {
                    continue;
                }
                let mut x: c_int = 0;
                let mut y: c_int = 0;
                let mut child: c_ulong = 0;
                if (xlib.XTranslateCoordinates)(
                    display,
                    window,
                    root,
                    0,
                    0,
                    &mut x,
                    &mut y,
                    &mut child,
                ) == 0
                {
                    continue;
                }
                let candidate = MonitorRect {
                    left: x,
                    top: y,
                    right: x + attrs.width,
                    bottom: y + attrs.height,
                };
                if let Some(clipped) = candidate.intersection(&monitor) {
                    if clipped.width() >= 8 && clipped.height() >= 8 && regions.len() < 64 {
                        regions.push(clipped);
                    }
                }
            }
        }
        if !children.is_null() {
            (xlib.XFree)(children as *mut c_void);
        }
        (xlib.XCloseDisplay)(display);
        regions
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn detect_regions(_monitor: MonitorRect) -> Vec<MonitorRect> {
    Vec::new()
}

#[cfg(windows)]
unsafe fn capture_from_dc(dc: isize, monitor: MonitorRect) -> Result<ScreenCapture, String> {
    let mut bgra = read_region(dc, monitor, monitor.width() as u32, monitor.height() as u32)?;
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 0xff;
    }
    Ok(ScreenCapture {
        rgba: bgra,
        width: monitor.width() as u32,
        height: monitor.height() as u32,
        origin_x: monitor.left,
        origin_y: monitor.top,
    })
}

/// Read one desktop region back from `dc` at exactly its own resolution, as
/// top-down BGRA pixels. This is the delivered screenshot path, so it stays a
/// pixel-exact `BitBlt`.
#[cfg(windows)]
unsafe fn read_region(
    dc: isize,
    rect: MonitorRect,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    blit_region(dc, rect, width, height, false)
}

/// Long edge in pixels of the downscaled read-back used to verify that a hidden
/// window is really off the screen. The exact read of a maximized window hands
/// back a 15.6 MB buffer on the UI thread right before the hide, and the
/// verification loop repeats that every retry; it also stays in flight long
/// enough to be torn by a repaint *during* the blit, and a reference frame torn
/// that way reads as "the pixels changed" while the ghost is still composed.
/// The verdict only needs coarse coverage: measured on a 2560x1528 region the
/// 256x153 grid reads in ~22 ms and 153 KB against ~33 ms and 16 MB for the
/// exact read, and a smaller window already drops to ~6 ms.
#[cfg(windows)]
const PREVIEW_EDGE: u32 = 256;

/// Best-effort downscaled read-back of one desktop region through the screen DC,
/// used to verify that a freshly hidden window no longer contributes pixels to
/// what a capture would see. `None` on any GDI failure; verification then
/// degrades to a fixed settle.
#[cfg(windows)]
pub fn sample_region_preview(rect: MonitorRect) -> Option<Vec<u8>> {
    use windows_sys::Win32::Graphics::Gdi::{GetDC, ReleaseDC};

    let (width, height) = preview_size(rect);
    unsafe {
        let dc = GetDC(0);
        if dc == 0 {
            return None;
        }
        let result = blit_region(dc, rect, width, height, true);
        ReleaseDC(0, dc);
        result.ok()
    }
}

/// Sampling grid of a preview read-back: the region aspect ratio kept, long edge
/// capped. A pure function of the rectangle, so the pre-hide reference and every
/// retry describe the same pixels.
#[cfg(windows)]
pub(crate) fn preview_size(rect: MonitorRect) -> (u32, u32) {
    let long = rect.width().max(rect.height()).max(1) as u32;
    let scale = PREVIEW_EDGE.min(long) as f32 / long as f32;
    let width = ((rect.width().max(1) as f32 * scale).round() as u32).max(1);
    let height = ((rect.height().max(1) as f32 * scale).round() as u32).max(1);
    (width, height)
}

/// Copy `rect` out of `dc` into a `width` x `height` top-down BGRA buffer.
/// `scaled` picks `StretchBlt` — a coarse, cheap read for the hide verification
/// loop — while the exact-size path stays `BitBlt` for delivered captures.
#[cfg(windows)]
unsafe fn blit_region(
    dc: isize,
    rect: MonitorRect,
    width: u32,
    height: u32,
    scaled: bool,
) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
        SelectObject, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, COLORONCOLOR,
        DIB_RGB_COLORS, SRCCOPY,
    };
    if !rect.is_usable() || width == 0 || height == 0 {
        return Err("无法读取屏幕尺寸".into());
    }
    let mem = CreateCompatibleDC(dc);
    if mem == 0 {
        return Err("无法创建屏幕缓存".into());
    }
    let bitmap = CreateCompatibleBitmap(dc, width as i32, height as i32);
    if bitmap == 0 {
        DeleteDC(mem);
        return Err("无法创建屏幕位图".into());
    }
    let old = SelectObject(mem, bitmap);
    let ok = if scaled {
        // COLORONCOLOR drops source rows and columns instead of averaging them:
        // the cheapest filter, and deterministic, which the pixel comparison
        // between the reference and each retry relies on.
        SetStretchBltMode(mem, COLORONCOLOR);
        StretchBlt(
            mem,
            0,
            0,
            width as i32,
            height as i32,
            dc,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            SRCCOPY,
        )
    } else {
        BitBlt(
            mem,
            0,
            0,
            width as i32,
            height as i32,
            dc,
            rect.left,
            rect.top,
            SRCCOPY,
        )
    };
    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: -(height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0,
        ..std::mem::zeroed()
    };
    let mut bgra = vec![0u8; width as usize * height as usize * 4];
    let lines = if ok != 0 {
        GetDIBits(
            mem,
            bitmap,
            0,
            height,
            bgra.as_mut_ptr() as *mut _,
            &mut info,
            DIB_RGB_COLORS,
        )
    } else {
        0
    };
    SelectObject(mem, old);
    DeleteObject(bitmap);
    DeleteDC(mem);
    if lines == 0 {
        return Err("屏幕复制失败".into());
    }
    Ok(bgra)
}

/// Linux X11 fallback. This deliberately captures the primary root window,
/// matching the Win32 path and avoiding multi-monitor coordinate ambiguity.
#[cfg(target_os = "linux")]
pub fn capture_screen() -> Result<ScreenCapture, String> {
    use x11_dl::xlib;

    let xlib = xlib::Xlib::open().map_err(|error| format!("无法加载 Xlib：{error}"))?;
    unsafe {
        let display = (xlib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return Err("无法连接 X 显示服务器".into());
        }
        let result = capture_root(&xlib, display);
        (xlib.XCloseDisplay)(display);
        result
    }
}

#[cfg(target_os = "linux")]
unsafe fn capture_root(
    xlib: &x11_dl::xlib::Xlib,
    display: *mut x11_dl::xlib::Display,
) -> Result<ScreenCapture, String> {
    use x11_dl::xlib;

    let screen = (xlib.XDefaultScreen)(display);
    let root = (xlib.XRootWindow)(display, screen);
    let width = (xlib.XDisplayWidth)(display, screen);
    let height = (xlib.XDisplayHeight)(display, screen);
    if width <= 0 || height <= 0 {
        return Err("无法读取屏幕尺寸".into());
    }
    let visual = (xlib.XDefaultVisual)(display, screen);
    if visual.is_null() || (*visual).class != xlib::TrueColor {
        return Err("仅支持 TrueColor 视觉的显示器".into());
    }
    let (red_mask, green_mask, blue_mask) = (
        (*visual).red_mask,
        (*visual).green_mask,
        (*visual).blue_mask,
    );
    let image = (xlib.XGetImage)(
        display,
        root,
        0,
        0,
        width as u32,
        height as u32,
        !0 as std::ffi::c_ulong,
        xlib::ZPixmap,
    );
    if image.is_null() {
        return Err("XGetImage 失败".into());
    }
    let img = &*image;
    let bytes_per_pixel = (img.bits_per_pixel / 8) as usize;
    if bytes_per_pixel != 3 && bytes_per_pixel != 4 {
        (xlib.XDestroyImage)(image);
        return Err(format!("不支持的像素位深：{}bpp", img.bits_per_pixel));
    }
    let stride = img.bytes_per_line as usize;
    let data = std::slice::from_raw_parts(
        img.data as *const u8,
        stride.saturating_mul(height as usize),
    );
    let little_endian = img.byte_order == xlib::LSBFirst;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in 0..height as usize {
        let line = &data[row * stride..row * stride + width as usize * bytes_per_pixel];
        for pixel in line.chunks_exact(bytes_per_pixel) {
            let value = if bytes_per_pixel == 4 {
                if little_endian {
                    u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]])
                } else {
                    u32::from_be_bytes([pixel[0], pixel[1], pixel[2], pixel[3]])
                }
            } else if little_endian {
                u32::from(pixel[0]) | u32::from(pixel[1]) << 8 | u32::from(pixel[2]) << 16
            } else {
                u32::from(pixel[2]) | u32::from(pixel[1]) << 8 | u32::from(pixel[0]) << 16
            };
            rgba.push(extract_masked(value, red_mask));
            rgba.push(extract_masked(value, green_mask));
            rgba.push(extract_masked(value, blue_mask));
            rgba.push(0xff);
        }
    }
    (xlib.XDestroyImage)(image);
    Ok(ScreenCapture {
        rgba,
        width: width as u32,
        height: height as u32,
        origin_x: 0,
        origin_y: 0,
    })
}

#[cfg(target_os = "linux")]
fn extract_masked(value: u32, mask: std::ffi::c_ulong) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let field = (value as u64 & mask) >> shift;
    let max = mask >> shift;
    ((field * 255 + max / 2) / max) as u8
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn capture_screen() -> Result<ScreenCapture, String> {
    Err("当前平台暂不支持桌面截图".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_rect_reports_its_physical_extent() {
        let rect = MonitorRect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1600,
        };
        assert_eq!(rect.width(), 2560);
        assert_eq!(rect.height(), 1600);
        assert!(rect.is_usable());
    }

    #[test]
    fn a_monitor_offset_from_the_desktop_origin_keeps_its_own_size() {
        // A panel left of the primary monitor has a negative origin; the size
        // must come from the rect extent, never from the origin.
        let rect = MonitorRect {
            left: -1920,
            top: -120,
            right: 0,
            bottom: 1080,
        };
        assert_eq!(rect.width(), 1920);
        assert_eq!(rect.height(), 1200);
        assert!(rect.is_usable());
    }

    #[test]
    fn degenerate_rects_are_rejected() {
        assert!(!MonitorRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0
        }
        .is_usable());
        assert!(!MonitorRect {
            left: 10,
            top: 0,
            right: 10,
            bottom: 900
        }
        .is_usable());
        assert!(!MonitorRect {
            left: 0,
            top: 10,
            right: 1440,
            bottom: 10
        }
        .is_usable());
    }

    #[test]
    fn intersection_clips_to_the_shared_area() {
        let monitor = MonitorRect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1600,
        };
        let window = MonitorRect {
            left: -20,
            top: 1580,
            right: 400,
            bottom: 2200,
        };
        let clipped = monitor
            .intersection(&window)
            .expect("windows overlap the monitor");
        assert_eq!(
            (clipped.left, clipped.top, clipped.right, clipped.bottom),
            (0, 1580, 400, 1600)
        );
    }

    #[test]
    fn a_window_off_the_capture_monitor_has_nothing_to_verify() {
        let monitor = MonitorRect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1600,
        };
        let window = MonitorRect {
            left: 3000,
            top: 0,
            right: 4900,
            bottom: 1200,
        };
        assert!(monitor.intersection(&window).is_none());
        let touching = MonitorRect {
            left: 2560,
            top: 0,
            right: 4900,
            bottom: 1200,
        };
        assert!(monitor.intersection(&touching).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn the_preview_grid_keeps_the_aspect_ratio_within_one_edge() {
        // A maximized window's region reads as 256 x 153 instead of 2560 x 1528:
        // whole-region coverage at a fraction of the GDI cost.
        let rect = MonitorRect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1528,
        };
        assert_eq!(preview_size(rect), (256, 153));
        // Below the cap the region is read at its own resolution, and a sliver
        // still gets a grid instead of an empty buffer.
        assert_eq!(
            preview_size(MonitorRect {
                left: 0,
                top: 0,
                right: 120,
                bottom: 80
            }),
            (120, 80)
        );
        assert_eq!(
            preview_size(MonitorRect {
                left: 0,
                top: 0,
                right: 2560,
                bottom: 1
            }),
            (256, 1)
        );
    }
}
