//! Win7 GDI ClearType rasterization for the bundled western glyphs.
//!
//! Slint's software renderer has no Windows ClearType backend.  The compiler
//! still provides the original bitmap metrics, while this module asks the
//! Win7 GDI rasterizer for the RGB coverages of printable ASCII glyphs.  The
//! result is copied out of an opaque white 32-bit DIB and fed back into the
//! normal software blend path.

#![cfg_attr(target_os = "windows", allow(unsafe_code))]

use alloc::rc::Rc;

#[derive(Clone)]
pub(crate) struct NativeGlyph {
    pub data: Rc<[u8]>,
    pub width: u16,
    pub height: u16,
    pub stride: u16,
    /// Position of the bitmap's top-left corner relative to the baseline.
    pub x: i32,
    pub y: i32,
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn register_font(_family: &'static str, _data: &'static [u8]) {}

#[cfg(not(target_os = "windows"))]
pub(crate) fn rasterize(_ch: char, _pixel_size: i16) -> Option<NativeGlyph> {
    None
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn enabled() -> bool {
    false
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::NativeGlyph;
    use alloc::rc::Rc;
    use core::mem;
    use core::ptr;
    use std::sync::{Mutex, OnceLock};
    use windows_sys::Win32::Foundation::{COLORREF, HANDLE};
    use windows_sys::Win32::Graphics::Gdi::{
        AddFontMemResourceEx, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_NATURAL_QUALITY,
        CreateCompatibleDC, CreateDIBSection, CreateFontIndirectW, DEFAULT_CHARSET, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, ExtTextOutW, GetDC, GetTextMetricsW, LOGFONTW, OPAQUE,
        OUT_TT_PRECIS, ReleaseDC, SelectObject, SetBkColor, SetBkMode, SetTextAlign, SetTextColor,
        TA_BASELINE, TA_LEFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FE_FONTSMOOTHINGCLEARTYPE, SPI_GETFONTSMOOTHING, SPI_GETFONTSMOOTHINGORIENTATION,
        SPI_GETFONTSMOOTHINGTYPE, SystemParametersInfoW,
    };

    #[derive(Clone, Copy)]
    struct Registration {
        family: &'static str,
        handle: HANDLE,
    }

    static REGISTRATION: OnceLock<Option<Registration>> = OnceLock::new();
    static RENDER_MUTEX: Mutex<()> = Mutex::new(());
    static FIRST_GLYPH_LOGGED: OnceLock<()> = OnceLock::new();

    fn clear_type_enabled() -> bool {
        unsafe {
            let mut enabled = 0u32;
            let mut smoothing_type = 0u32;
            let mut orientation = 1u32;
            let flags = 0;
            SystemParametersInfoW(
                SPI_GETFONTSMOOTHING,
                0,
                &mut enabled as *mut u32 as *mut core::ffi::c_void,
                flags,
            ) != 0
                && SystemParametersInfoW(
                    SPI_GETFONTSMOOTHINGTYPE,
                    0,
                    &mut smoothing_type as *mut u32 as *mut core::ffi::c_void,
                    flags,
                ) != 0
                && SystemParametersInfoW(
                    SPI_GETFONTSMOOTHINGORIENTATION,
                    0,
                    &mut orientation as *mut u32 as *mut core::ffi::c_void,
                    flags,
                ) != 0
                && enabled != 0
                && smoothing_type == FE_FONTSMOOTHINGCLEARTYPE
                && (orientation == 0 || orientation == 1)
        }
    }

    pub(crate) fn register_font(family: &'static str, data: &'static [u8]) {
        REGISTRATION.get_or_init(|| {
            if !clear_type_enabled() || data.is_empty() || data.len() > u32::MAX as usize {
                log("disabled (ClearType is off or font data is invalid)");
                return None;
            }
            let mut count = 0u32;
            let handle = unsafe {
                AddFontMemResourceEx(
                    data.as_ptr() as *const core::ffi::c_void,
                    data.len() as u32,
                    ptr::null(),
                    &mut count as *mut u32 as *const u32,
                )
            };
            if handle == 0 || count == 0 {
                log("font registration failed");
                None
            } else {
                log("native GDI ClearType enabled");
                Some(Registration { family, handle })
            }
        });
    }

    pub(crate) fn enabled() -> bool {
        REGISTRATION.get().is_some_and(Option::is_some)
    }

    pub(crate) fn rasterize(ch: char, pixel_size: i16) -> Option<NativeGlyph> {
        if !ch.is_ascii() || pixel_size <= 0 {
            return None;
        }
        let registration = REGISTRATION.get()?.as_ref()?;
        let _gdi_guard = RENDER_MUTEX.lock().ok()?;
        let _keep_font_registered = registration.handle;
        let face_name = wide_face_name(registration.family);

        // Padding absorbs the small side bearings and makes scanning robust on
        // both Win7's GDI and newer Windows versions used for development.
        let padding = 8i32;
        let side = (pixel_size as i32).saturating_add(32).max(40);
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: side,
                biHeight: -side,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..unsafe { mem::zeroed() }
            },
            ..unsafe { mem::zeroed() }
        };

        unsafe {
            // A screen-compatible DC is required for Win7 to select the
            // monitor's ClearType mode; a null-compatible DC can silently
            // fall back to grayscale on legacy systems.
            let screen_dc = GetDC(0);
            if screen_dc == 0 {
                return None;
            }
            let dc = CreateCompatibleDC(screen_dc);
            ReleaseDC(0, screen_dc);
            if dc == 0 {
                return None;
            }
            let mut pixels = ptr::null_mut();
            let bitmap = CreateDIBSection(dc, &bitmap_info, DIB_RGB_COLORS, &mut pixels, 0, 0);
            if bitmap == 0 || pixels.is_null() {
                if bitmap != 0 {
                    DeleteObject(bitmap);
                }
                DeleteDC(dc);
                return None;
            }
            let old_bitmap = SelectObject(dc, bitmap);
            let font = LOGFONTW {
                lfHeight: -(pixel_size as i32),
                lfWidth: 0,
                lfEscapement: 0,
                lfOrientation: 0,
                lfWeight: 400,
                lfItalic: 0,
                lfUnderline: 0,
                lfStrikeOut: 0,
                lfCharSet: DEFAULT_CHARSET,
                lfOutPrecision: OUT_TT_PRECIS,
                lfClipPrecision: 0,
                lfQuality: CLEARTYPE_NATURAL_QUALITY as u8,
                lfPitchAndFamily: 0,
                lfFaceName: face_name,
            };
            let hfont = CreateFontIndirectW(&font);
            if hfont == 0 {
                SelectObject(dc, old_bitmap);
                DeleteObject(bitmap);
                DeleteDC(dc);
                return None;
            }
            let old_font = SelectObject(dc, hfont);
            SetBkMode(dc, OPAQUE as i32);
            SetBkColor(dc, 0x00ffffff as COLORREF);
            SetTextColor(dc, 0x00000000 as COLORREF);
            SetTextAlign(dc, TA_LEFT | TA_BASELINE);
            ptr::write_bytes(pixels as *mut u8, 0xff, (side * side * 4) as usize);

            let mut metrics = mem::zeroed();
            if GetTextMetricsW(dc, &mut metrics) == 0 {
                SelectObject(dc, old_font);
                DeleteObject(hfont);
                SelectObject(dc, old_bitmap);
                DeleteObject(bitmap);
                DeleteDC(dc);
                return None;
            }
            let baseline = padding + metrics.tmAscent;
            let text = [ch as u16];
            if ExtTextOutW(
                dc,
                padding,
                baseline,
                0,
                ptr::null(),
                text.as_ptr(),
                1,
                ptr::null(),
            ) == 0
            {
                SelectObject(dc, old_font);
                DeleteObject(hfont);
                SelectObject(dc, old_bitmap);
                DeleteObject(bitmap);
                DeleteDC(dc);
                return None;
            }

            let raw = core::slice::from_raw_parts(pixels as *const u8, (side * side * 4) as usize);
            let mut left = side;
            let mut top = side;
            let mut right = 0i32;
            let mut bottom = 0i32;
            for y in 0..side {
                for x in 0..side {
                    let p = ((y * side + x) * 4) as usize;
                    // DIB_RGB_COLORS is BGRA. ClearType writes independent
                    // channel values against the known white background.
                    if raw[p] != 255 || raw[p + 1] != 255 || raw[p + 2] != 255 {
                        left = left.min(x);
                        top = top.min(y);
                        right = right.max(x + 1);
                        bottom = bottom.max(y + 1);
                    }
                }
            }
            let result = if left < right && top < bottom {
                let width = (right - left) as usize;
                let height = (bottom - top) as usize;
                let mut data = alloc::vec![0u8; width * height * 4];
                for y in 0..height {
                    for x in 0..width {
                        let source = ((top as usize + y) * side as usize + left as usize + x) * 4;
                        let target = (y * width + x) * 4;
                        data[target] = 255 - raw[source + 2];
                        data[target + 1] = 255 - raw[source + 1];
                        data[target + 2] = 255 - raw[source];
                        data[target + 3] = 0;
                    }
                }
                Some(NativeGlyph {
                    data: Rc::from(data),
                    width: width as u16,
                    height: height as u16,
                    stride: width as u16,
                    x: left - padding,
                    y: baseline - bottom,
                })
            } else {
                Some(NativeGlyph {
                    data: Rc::from([]),
                    width: 0,
                    height: 0,
                    stride: 0,
                    x: padding,
                    y: -baseline,
                })
            };

            SelectObject(dc, old_font);
            DeleteObject(hfont);
            SelectObject(dc, old_bitmap);
            DeleteObject(bitmap);
            DeleteDC(dc);
            if let Some(glyph) = result.as_ref()
                && FIRST_GLYPH_LOGGED.set(()).is_ok()
            {
                let native_rgb = glyph
                    .data
                    .chunks_exact(4)
                    .any(|pixel| pixel[0] != pixel[1] || pixel[1] != pixel[2]);
                log(&alloc::format!(
                    "native glyph rasterized: char={ch:?} pixel_size={pixel_size} rgb={native_rgb}"
                ));
            }
            result
        }
    }

    fn wide_face_name(family: &str) -> [u16; 32] {
        let mut result = [0u16; 32];
        for (target, source) in result.iter_mut().take(31).zip(family.encode_utf16()) {
            *target = source;
        }
        result
    }

    fn log(message: &str) {
        let Ok(path) = std::env::var("RCHO_TEXT_RENDER_LOG") else {
            return;
        };
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write;
            let _ = writeln!(file, "{message}");
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) use windows_impl::{enabled, rasterize, register_font};
