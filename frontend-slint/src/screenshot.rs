//! Region screenshot flow for the native desktop UI.
//!
//! GDI capture and PNG compression never run on Slint's UI thread. The frozen
//! frame is retained only while the selector is open; confirmation adds the
//! result to the composer as a native chat attachment.

use crate::{screen_capture, ChatAttachment, MainWindow, ScreenshotOverlay};
use base64::Engine;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING_ATTACHMENTS: usize = 4;

static CAPTURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

thread_local! {
    static OVERLAY: RefCell<Option<ScreenshotOverlay>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<screen_capture::ScreenCapture>> = const { RefCell::new(None) };
}

/// Start an interactive primary-screen capture. Repeated hotkey presses while
/// the selector is visible are ignored to keep the in-memory frame bounded.
pub fn capture(app: slint::Weak<MainWindow>) {
    if OVERLAY.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|overlay| overlay.window().is_visible())
    }) {
        return;
    }
    if CAPTURE_IN_FLIGHT.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        let result = screen_capture::capture_screen();
        let _ = slint::invoke_from_event_loop(move || {
            CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
            match result {
                Ok(frame) => show_selector(app, frame),
                Err(error) => report(&app, format!("截图失败：{error}")),
            }
        });
    });
}

/// Capture the complete primary screen without opening the selector. This is
/// used by the tray menu and follows the same attachment path as a region
/// capture, so the result is immediately visible in the composer.
pub fn capture_fullscreen(app: slint::Weak<MainWindow>) {
    if OVERLAY.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|overlay| overlay.window().is_visible())
    }) {
        return;
    }
    if CAPTURE_IN_FLIGHT.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        let result = screen_capture::capture_screen()
            .and_then(|frame| save_attachment(frame.width, frame.height, &frame.rgba));
        let _ = slint::invoke_from_event_loop(move || {
            CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
            match result {
                Ok((name, data_url)) => add_attachment(&app, name, data_url, "全屏截图"),
                Err(error) => report(&app, format!("截图失败：{error}")),
            }
        });
    });
}

fn show_selector(app: slint::Weak<MainWindow>, frame: screen_capture::ScreenCapture) {
    let result = OVERLAY.with(|slot| -> Result<(), String> {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let overlay =
                ScreenshotOverlay::new().map_err(|error| format!("无法创建截图选区：{error}"))?;
            let confirmed_app = app.clone();
            overlay.on_confirmed(move |x, y, width, height| {
                finish_selection(confirmed_app.clone(), x, y, width, height);
            });
            overlay.on_canceled(hide_selector);
            *slot = Some(overlay);
        }
        let overlay = slot.as_ref().expect("screenshot overlay was initialized");
        overlay.set_screen_image(slint::Image::from_rgba8(
            slint::SharedPixelBuffer::clone_from_slice(&frame.rgba, frame.width, frame.height),
        ));
        let scale = overlay.window().scale_factor().max(1.0);
        overlay.set_screen_width(frame.width as f32 / scale);
        overlay.set_screen_height(frame.height as f32 / scale);
        overlay.set_sel_x(0.0);
        overlay.set_sel_y(0.0);
        overlay.set_sel_w(0.0);
        overlay.set_sel_h(0.0);
        let (width, height) = (frame.width, frame.height);
        PENDING.with(|pending| *pending.borrow_mut() = Some(frame));
        overlay
            .show()
            .map_err(|error| format!("无法显示截图选区：{error}"))?;
        // Physical sizing avoids DPI rounding on legacy primary monitors.
        overlay
            .window()
            .set_size(slint::PhysicalSize::new(width, height));
        overlay
            .window()
            .set_position(slint::PhysicalPosition::new(0, 0));
        overlay.window().request_redraw();
        Ok(())
    });
    if let Err(error) = result {
        hide_selector();
        report(&app, format!("截图失败：{error}"));
    }
}

fn finish_selection(app: slint::Weak<MainWindow>, x: i32, y: i32, width: i32, height: i32) {
    if width < 1 || height < 1 {
        hide_selector();
        return;
    }
    let scale = OVERLAY.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|overlay| overlay.window().scale_factor().max(1.0))
            .unwrap_or(1.0)
    });
    let frame = PENDING.with(|pending| pending.borrow_mut().take());
    hide_selector();
    let Some(frame) = frame else {
        report(&app, "截图失败：选区画面已失效".to_owned());
        return;
    };
    let (width, height, rgba) = crop(
        &frame,
        (x as f32 * scale).round() as i32,
        (y as f32 * scale).round() as i32,
        (width as f32 * scale).round() as i32,
        (height as f32 * scale).round() as i32,
    );
    std::thread::spawn(move || {
        let result = save_attachment(width, height, &rgba);
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok((name, data_url)) => add_attachment(&app, name, data_url, "截图"),
            Err(error) => report(&app, format!("截图失败：{error}")),
        });
    });
}

fn add_attachment(app: &slint::Weak<MainWindow>, name: String, data_url: String, kind: &str) {
    let Some(app) = app.upgrade() else { return };
    let mut attachments = app.get_pending_attachments().iter().collect::<Vec<_>>();
    if attachments.len() >= MAX_PENDING_ATTACHMENTS {
        app.set_status(format!("最多可附加 {MAX_PENDING_ATTACHMENTS} 张截图，请先移除一张").into());
        return;
    }
    attachments.push(ChatAttachment {
        name: name.into(),
        mime_type: "image/png".into(),
        data_url: data_url.into(),
    });
    app.set_pending_attachments(ModelRc::new(VecModel::from(attachments)));
    app.set_status(format!("{kind}已放入输入区，发送即可交给模型").into());
}

fn hide_selector() {
    OVERLAY.with(|slot| {
        if let Some(overlay) = slot.borrow().as_ref() {
            let _ = overlay.hide();
        }
    });
    PENDING.with(|pending| {
        pending.borrow_mut().take();
    });
}

fn crop(
    frame: &screen_capture::ScreenCapture,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> (u32, u32, Vec<u8>) {
    let frame_width = frame.width as i32;
    let frame_height = frame.height as i32;
    let x = x.clamp(0, frame_width.saturating_sub(1));
    let y = y.clamp(0, frame_height.saturating_sub(1));
    let width = width.clamp(1, frame_width - x);
    let height = height.clamp(1, frame_height - y);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in 0..height {
        let start = ((y + row) * frame_width + x) as usize * 4;
        rgba.extend_from_slice(&frame.rgba[start..start + width as usize * 4]);
    }
    (width as u32, height as u32, rgba)
}

fn save_attachment(width: u32, height: u32, rgba: &[u8]) -> Result<(String, String), String> {
    let png = encode_png(width, height, rgba)?;
    if png.len() > MAX_PNG_BYTES {
        return Err("截图压缩后超过 8 MiB，未放入输入区".into());
    }
    let directory = std::env::temp_dir().join("wunder-screenshots");
    std::fs::create_dir_all(&directory).map_err(|error| format!("无法创建截图目录：{error}"))?;
    let name = format!("wunder-screenshot-{}.png", timestamp());
    std::fs::write(directory.join(&name), &png)
        .map_err(|error| format!("无法保存截图：{error}"))?;
    let data_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    );
    Ok((name, data_url))
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| format!("PNG 头写入失败：{error}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|error| format!("PNG 编码失败：{error}"))?;
    writer
        .finish()
        .map_err(|error| format!("PNG 收尾失败：{error}"))?;
    Ok(bytes)
}

fn timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default()
}

fn report(app: &slint::Weak<MainWindow>, message: String) {
    if let Some(app) = app.upgrade() {
        app.set_status(message.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_frame() -> screen_capture::ScreenCapture {
        let mut rgba = Vec::new();
        for row in 0..3u8 {
            for column in 0..4u8 {
                rgba.extend_from_slice(&[row * 10 + column, 0, 0, 0xff]);
            }
        }
        screen_capture::ScreenCapture {
            rgba,
            width: 4,
            height: 3,
        }
    }

    #[test]
    fn crop_preserves_the_requested_pixels() {
        let (_, _, rgba) = crop(&sample_frame(), 1, 1, 2, 2);
        assert_eq!(
            rgba.chunks_exact(4)
                .map(|pixel| pixel[0])
                .collect::<Vec<_>>(),
            vec![11, 12, 21, 22]
        );
    }

    #[test]
    fn crop_clamps_to_the_frame() {
        let (width, height, rgba) = crop(&sample_frame(), -3, -2, 99, 99);
        assert_eq!((width, height), (4, 3));
        assert_eq!(rgba.len(), 4 * 3 * 4);
    }
}
