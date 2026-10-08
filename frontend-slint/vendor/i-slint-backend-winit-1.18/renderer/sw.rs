// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Delegate the rendering to the [`i_slint_renderer_software::SoftwareRenderer`]

use core::num::NonZeroU32;
use core::ops::DerefMut;
use i_slint_core::graphics::Rgb8Pixel;
use i_slint_core::platform::PlatformError;
use i_slint_core::renderer::DrawOutcome;
pub use i_slint_renderer_software::SoftwareRenderer;
use i_slint_core::Color;
use i_slint_renderer_software::{PremultipliedRgbaColor, RepaintBufferType, TargetPixel};
#[cfg(target_os = "windows")]
use i_slint_renderer_software::SubpixelOrder;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use winit::event_loop::ActiveEventLoop;

use super::WinitCompatibleRenderer;

mod popup_repaint;

pub struct WinitSoftwareRenderer {
    renderer: SoftwareRenderer,
    _context: RefCell<Option<softbuffer::Context<Arc<winit::window::Window>>>>,
    surface: RefCell<
        Option<softbuffer::Surface<Arc<winit::window::Window>, Arc<winit::window::Window>>>,
    >,
    resize_retry_count: Cell<u8>,
    /// Win32 can erase a hidden or moved GDI surface without updating the
    /// softbuffer age. The next visible frame must therefore ignore the age
    /// and render every pixel once.
    force_full_repaint: Cell<bool>,
    popup_repaint: popup_repaint::PopupRepaint,
}

#[repr(transparent)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct SoftBufferPixel(pub u32);

impl From<SoftBufferPixel> for PremultipliedRgbaColor {
    #[inline]
    fn from(pixel: SoftBufferPixel) -> Self {
        let v = pixel.0;
        PremultipliedRgbaColor {
            red: (v >> 16) as u8,
            green: (v >> 8) as u8,
            blue: v as u8,
            alpha: (v >> 24) as u8,
        }
    }
}

impl From<PremultipliedRgbaColor> for SoftBufferPixel {
    #[inline]
    fn from(pixel: PremultipliedRgbaColor) -> Self {
        Self(
            (pixel.alpha as u32) << 24
                | ((pixel.red as u32) << 16)
                | ((pixel.green as u32) << 8)
                | (pixel.blue as u32),
        )
    }
}

impl TargetPixel for SoftBufferPixel {
    fn blend(&mut self, color: PremultipliedRgbaColor) {
        let mut x = PremultipliedRgbaColor::from(*self);
        x.blend(color);
        *self = x.into();
    }

    fn blend_subpixel(&mut self, color: Color, coverage: [u8; 3]) {
        let blend_channel = |dst: u8, src: u8, coverage: u8| {
            let alpha = coverage as u16 * color.alpha() as u16 / 255;
            (dst as u16 * (255 - alpha) / 255 + src as u16 * alpha / 255)
                as u8
        };
        let value = self.0;
        *self = Self(
            0xff000000
                | ((blend_channel((value >> 16) as u8, color.red(), coverage[0]) as u32) << 16)
                | ((blend_channel((value >> 8) as u8, color.green(), coverage[1]) as u32) << 8)
                | (blend_channel(value as u8, color.blue(), coverage[2]) as u32),
        );
    }

    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self(0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    fn background() -> Self {
        Self(0)
    }
}

impl WinitSoftwareRenderer {
    pub fn new_suspended(
        _shared_backend_data: &Rc<crate::SharedBackendData>,
    ) -> Result<Box<dyn WinitCompatibleRenderer>, PlatformError> {
        #[allow(unused_mut)] // mut is only used by the Windows subpixel-order setup
        let mut renderer = SoftwareRenderer::new();
        #[cfg(target_os = "windows")]
        renderer.set_subpixel_order(system_subpixel_order());
        Ok(Box::new(Self {
            renderer,
            _context: RefCell::new(None),
            surface: RefCell::new(None),
            resize_retry_count: Default::default(),
            force_full_repaint: Default::default(),
            popup_repaint: Default::default(),
        }))
    }
}

/// Query the user's ClearType mode through the Win7-era User32 API. It has no
/// WinRT/DirectWrite dependency. A disabled setting or failed query uses
/// grayscale blending, which is safe for every monitor arrangement.
#[cfg(target_os = "windows")]
fn system_subpixel_order() -> SubpixelOrder {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, FE_FONTSMOOTHINGCLEARTYPE, FE_FONTSMOOTHINGORIENTATIONBGR,
        SPI_GETFONTSMOOTHING, SPI_GETFONTSMOOTHINGORIENTATION, SPI_GETFONTSMOOTHINGTYPE,
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    let query = |action, value: &mut u32| unsafe {
        SystemParametersInfoW(
            action,
            0,
            Some(value as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
    };

    let mut enabled = 0u32;
    let mut smoothing_type = 0u32;
    let mut orientation = 1u32;
    if !query(SPI_GETFONTSMOOTHING, &mut enabled)
        || !query(SPI_GETFONTSMOOTHINGTYPE, &mut smoothing_type)
        || !query(SPI_GETFONTSMOOTHINGORIENTATION, &mut orientation)
    {
        return SubpixelOrder::None;
    }

    if enabled == 0 || smoothing_type != FE_FONTSMOOTHINGCLEARTYPE {
        SubpixelOrder::None
    } else if orientation == FE_FONTSMOOTHINGORIENTATIONBGR {
        SubpixelOrder::Bgr
    } else {
        SubpixelOrder::Rgb
    }
}

impl super::WinitCompatibleRenderer for WinitSoftwareRenderer {
    fn render(&self, window: &i_slint_core::api::Window) -> Result<DrawOutcome, PlatformError> {
        let size = window.size();

        let Some((width, height)) = size.width.try_into().ok().zip(size.height.try_into().ok())
        else {
            // Nothing to render
            return Ok(DrawOutcome::Success);
        };

        let mut borrowed_surface = self.surface.borrow_mut();
        let Some(surface) = borrowed_surface.as_mut() else {
            // Nothing to render
            return Ok(DrawOutcome::Success);
        };

        let winit_window = surface.window().clone();
        let diagnostic_start = crate::software_diagnostics::frame_start();

        // On legacy Win32, GDI can transiently reject a DIB allocation while
        // the user drags a window edge. Rendering is best effort here: keep
        // the event loop alive and let the next redraw retry instead of
        // returning an error that terminates it.
        if let Err(error) = surface.resize(width, height) {
            eprintln!("Slint software surface resize deferred: {error}");
            self.retry_deferred_resize(&winit_window);
            return Ok(DrawOutcome::Success);
        }

        let mut target_buffer = match surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(error) => {
                eprintln!("Slint software surface buffer deferred: {error}");
                self.retry_deferred_resize(&winit_window);
                return Ok(DrawOutcome::Success);
            }
        };
        self.resize_retry_count.set(0);

        let (has_embedded_popup, popup_changed) = self.popup_repaint.prepare(window, &self.renderer);
        let age = target_buffer.age();
        // Opening/closing/moving a popup retains a complete repaint. Stable
        // popups explicitly dirty their own rectangle, so their background is
        // composited again over the toolbar without clearing every UI cache.
        // Consume the flag even when a popup changed, preserving retry state.
        let force_full_repaint = self.force_full_repaint.take();
        self.renderer
            .set_repaint_buffer_type(if popup_changed || force_full_repaint {
                RepaintBufferType::NewBuffer
            } else {
                match age {
                    1 => RepaintBufferType::ReusedBuffer,
                    2 => RepaintBufferType::SwappedBuffers,
                    _ => RepaintBufferType::NewBuffer,
                }
            });

        let prepare_ms = diagnostic_start.map(|t| t.elapsed().as_secs_f64() * 1000.0);
        let render_start = diagnostic_start.map(|_| std::time::Instant::now());
        let region = if std::env::var_os("SLINT_LINE_BY_LINE").is_none() {
            let buffer: &mut [SoftBufferPixel] =
                bytemuck::cast_slice_mut(target_buffer.deref_mut());
            self.renderer.render(buffer, width.get() as usize)
        } else {
            // SLINT_LINE_BY_LINE is set and this is a debug mode where we also render in a Rgb565Pixel
            struct FrameBuffer<'a> {
                buffer: &'a mut [u32],
                line: Vec<i_slint_renderer_software::Rgb565Pixel>,
            }
            impl i_slint_renderer_software::LineBufferProvider for FrameBuffer<'_> {
                type TargetPixel = i_slint_renderer_software::Rgb565Pixel;
                fn process_line(
                    &mut self,
                    line: usize,
                    range: core::ops::Range<usize>,
                    render_fn: impl FnOnce(&mut [Self::TargetPixel]),
                ) {
                    let line_begin = line * self.line.len();
                    let sub = &mut self.line[..range.len()];
                    render_fn(sub);
                    for (dst, src) in self.buffer[line_begin..][range].iter_mut().zip(sub) {
                        let p = Rgb8Pixel::from(*src);
                        *dst =
                            0xff000000 | ((p.r as u32) << 16) | ((p.g as u32) << 8) | (p.b as u32);
                    }
                }
            }
            self.renderer.render_by_line(FrameBuffer {
                buffer: &mut target_buffer,
                line: vec![Default::default(); width.get() as usize],
            })
        };

        let render_ms = render_start.map(|t| t.elapsed().as_secs_f64() * 1000.0);
        let present_start = diagnostic_start.map(|_| std::time::Instant::now());
        let mut present_failed = false;
        // Present each disjoint damage rectangle instead of the single bounding
        // box: a workbench frame change dirties distant regions (radar canvas,
        // file list row, status bar, side panels) whose bounding box spans the
        // entire window, forcing a full-surface BitBlt of unchanged pixels on
        // every navigation. Slint 1.18 upstream presents per-rectangle as well;
        // the local addition is the deferred-error handling and diagnostics.
        let damage_rects: Vec<softbuffer::Rect> = region
            .iter()
            .filter_map(|(origin, size)| {
                Option::zip(
                    NonZeroU32::new(size.width),
                    NonZeroU32::new(size.height),
                )
                .map(|(w, h)| softbuffer::Rect {
                    width: w,
                    height: h,
                    x: origin.x as u32,
                    y: origin.y as u32,
                })
            })
            .collect();
        let damage_pixels: u64 = damage_rects
            .iter()
            .map(|rect| u64::from(rect.width.get()) * u64::from(rect.height.get()))
            .sum();
        // One-shot screenshot hook: the buffer already holds the complete new
        // frame here, and `present_with_damage` consumes it below. The copy
        // happens only while a capture is armed, so the steady-state cost is
        // one atomic load.
        crate::frame_capture::maybe_capture(
            winit_window.id().into(),
            width.get(),
            height.get(),
            &target_buffer,
        );
        if !damage_rects.is_empty() {
            winit_window.pre_present_notify();
            if let Err(error) = target_buffer.present_with_damage(&damage_rects) {
                present_failed = true;
                eprintln!("Slint software surface present deferred: {error}");
                // `force_full_repaint.take()` was consumed before rendering.
                // A failed Win32 BitBlt did not publish that frame, so the
                // retry must redraw every pixel instead of trusting damage or
                // buffer age from the failed presentation.
                self.force_full_repaint.set(true);
                self.retry_deferred_resize(&winit_window);
            }
        }
        if !present_failed && !damage_rects.is_empty() {
            crate::first_frame::notify_presented();
        }
        if let Some(present_start) = present_start {
            crate::software_diagnostics::record(crate::software_diagnostics::FrameSample {
                at_ms: 0.0,
                window_id: winit_window.id().into(),
                width: width.get(),
                height: height.get(),
                scale: window.scale_factor(),
                prepare_ms: prepare_ms.unwrap_or_default(),
                render_ms: render_ms.unwrap_or_default(),
                present_ms: present_start.elapsed().as_secs_f64() * 1000.0,
                damage_pixels,
                popup: has_embedded_popup,
                failed: present_failed,
            });
        }
        Ok(DrawOutcome::Success)
    }

    fn as_core_renderer(&self) -> &dyn i_slint_core::renderer::Renderer {
        &self.renderer
    }

    fn occluded(&self, _: bool) {
        // On X11 and Windows, the buffer is completely cleared when the window is hidden
        // and the buffer age doesn't respect that, so clean the partial rendering cache
        self.force_full_repaint.set(true);
    }

    fn resume(
        &self,
        active_event_loop: &ActiveEventLoop,
        window_attributes: winit::window::WindowAttributes,
        _window_adapter_weak: std::rc::Weak<crate::winitwindowadapter::WinitWindowAdapter>,
    ) -> Result<Arc<winit::window::Window>, PlatformError> {
        let winit_window =
            active_event_loop.create_window(window_attributes).map_err(|winit_os_error| {
                PlatformError::from(format!(
                    "Error creating native window for software rendering: {winit_os_error}"
                ))
            })?;
        let winit_window = Arc::new(winit_window);

        let context = softbuffer::Context::new(winit_window.clone())
            .map_err(|e| format!("Error creating softbuffer context: {e}"))?;

        let surface = softbuffer::Surface::new(&context, winit_window.clone()).map_err(
            |softbuffer_error| format!("Error creating softbuffer surface: {softbuffer_error}"),
        )?;

        *self._context.borrow_mut() = Some(context);
        *self.surface.borrow_mut() = Some(surface);

        Ok(winit_window)
    }

    fn suspend(&self) -> Result<(), PlatformError> {
        drop(self.surface.borrow_mut().take());
        drop(self._context.borrow_mut().take());
        Ok(())
    }
}

impl WinitSoftwareRenderer {
    fn retry_deferred_resize(&self, winit_window: &winit::window::Window) {
        // A permanent allocation failure must not turn into a continuous
        // redraw loop. Later WM_SIZE and paint events still retry normally.
        let count = self.resize_retry_count.get().saturating_add(1);
        self.resize_retry_count.set(count);
        if count <= 2 {
            winit_window.request_redraw();
        }
    }
}
