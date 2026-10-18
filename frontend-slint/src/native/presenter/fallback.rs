/// A frame handed to a native presenter.  The current renderer produces
/// RGBA8; platform backends convert it to their native upload layout once.
#[derive(Debug)]
pub struct PresenterFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub type GestureCallback = Arc<dyn Fn(&'static str, f32, f32) + Send + Sync>;

/// Convert an owned RGBA8 frame in place before transferring it to a native
/// DIB/XImage. This avoids keeping a second full-size presenter buffer on
/// the common Win32 and little-endian X11 paths.
///
/// `alpha` fills the fourth byte: Win32 BI_RGB ignores it, but an X11 child
/// window created with a depth-32 ARGB visual is alpha-blended by the
/// compositor, so X11 callers must pass 0xff or the surface stays invisible.
pub fn rgba_to_bgra_in_place(pixels: &mut [u8], alpha: u8) -> bool {
    if pixels.len() % 4 != 0 {
        return false;
    }
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = alpha;
    }
    true
}

/// Portable no-op presenter used on platforms where an embeddable native
/// child surface is not available (including Wayland).  The Slint Image path
/// remains the correctness fallback there.
#[derive(Clone, Default)]
#[allow(dead_code)]
pub struct ModelPresenter;

#[allow(dead_code)]
impl ModelPresenter {
    pub fn new(_window: &slint::Window, _on_gesture: GestureCallback) -> Option<Self> {
        None
    }

    pub fn set_geometry(&self, _x: i32, _y: i32, _width: u32, _height: u32) -> bool {
        false
    }

    pub fn set_visible(&self, _visible: bool) {}

    pub fn present(&self, _frame: &mut PresenterFrame) -> bool {
        false
    }

    pub fn snapshot(&self) -> Option<PresenterFrame> {
        None
    }

    pub fn pump_events(&self) {}
}

#[cfg(test)]
mod tests {
    use super::rgba_to_bgra_in_place;

    #[test]
    fn owned_rgba_frame_converts_without_a_second_buffer() {
        let mut pixels = vec![0x12, 0x34, 0x56, 0xff, 0xaa, 0xbb, 0xcc, 0x80];

        assert!(rgba_to_bgra_in_place(&mut pixels, 0));
        assert_eq!(pixels, [0x56, 0x34, 0x12, 0, 0xcc, 0xbb, 0xaa, 0]);
        // Each upload starts with RGBA. Applying the swap twice to the same
        // buffer restores RGB order and does not test the opaque-alpha case.
        let mut pixels = vec![0x12, 0x34, 0x56, 0xff, 0xaa, 0xbb, 0xcc, 0x80];
        assert!(rgba_to_bgra_in_place(&mut pixels, 0xff));
        assert_eq!(pixels, [0x56, 0x34, 0x12, 0xff, 0xcc, 0xbb, 0xaa, 0xff]);
        assert!(!rgba_to_bgra_in_place(&mut [1, 2, 3], 0));
    }
}
use std::sync::Arc;

