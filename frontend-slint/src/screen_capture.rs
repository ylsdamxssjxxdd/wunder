//! Small Win7-safe screen capture primitive used by the tray screenshot action.
//! The UI receives pixels only after the blocking GDI call and PNG encoding
//! complete on a worker thread.

pub struct ScreenCapture {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
pub fn capture_screen() -> Result<ScreenCapture, String> {
    use windows_sys::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
    unsafe {
        let dc = GetDC(0);
        if dc == 0 {
            return Err("无法获取屏幕设备上下文".into());
        }
        let result = capture_from_dc(dc);
        ReleaseDC(0, dc);
        result
    }
}

#[cfg(windows)]
unsafe fn capture_from_dc(dc: isize) -> Result<ScreenCapture, String> {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
        GetDeviceCaps, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HORZRES,
        SRCCOPY, VERTRES,
    };
    let width = GetDeviceCaps(dc, HORZRES as i32);
    let height = GetDeviceCaps(dc, VERTRES as i32);
    if width <= 0 || height <= 0 {
        return Err("无法读取屏幕尺寸".into());
    }
    let (width, height) = (width as u32, height as u32);
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
    let ok = BitBlt(mem, 0, 0, width as i32, height as i32, dc, 0, 0, SRCCOPY);
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
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 0xff;
    }
    Ok(ScreenCapture {
        rgba: bgra,
        width,
        height,
    })
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
