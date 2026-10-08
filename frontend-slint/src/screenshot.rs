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

/// Proof that the freshly hidden main window is really off the captured
/// screen. `region` is the window's screen rectangle clipped to the capture
/// monitor; `reference` holds the downscaled screen pixels of that region from
/// just before the hide, i.e. exactly what a too-early capture would inherit
/// from the window.
#[cfg(windows)]
struct HideProof {
    region: screen_capture::MonitorRect,
    reference: Vec<u8>,
}

#[cfg(not(windows))]
struct HideProof;

/// Hide the main window on the UI thread and pin down what the capture would
/// otherwise inherit from it. Must run on the UI thread, and the reference
/// sample plus the hide happen back to back without an event-loop pass in
/// between, so no render can slip between them: the ghost a premature capture
/// would freeze into the frame is pixel-identical to the reference. The
/// reference is a coarse downscaled read: measured 6–22 ms of UI-thread work
/// and ~150 KB instead of a 15 MB buffer per read, and a short blit is far less
/// likely to be torn by a repaint *during* the blit, which would read as
/// changed pixels while the ghost is still composed.
fn hide_main_window(app: &slint::Weak<MainWindow>) -> (bool, Option<HideProof>) {
    #[cfg(windows)]
    {
        use slint::winit_030::WinitWindowAccessor;

        let Some(window) = app.upgrade() else {
            return (false, None);
        };
        let native = window.window().with_winit_window(|native| {
            let rect = native.outer_position().ok().map(|position| {
                let size = native.outer_size();
                screen_capture::MonitorRect {
                    left: position.x,
                    top: position.y,
                    right: position.x + size.width as i32,
                    bottom: position.y + size.height as i32,
                }
            });
            let on_screen =
                native.is_visible().unwrap_or(true) && !native.is_minimized().unwrap_or(false);
            (on_screen, rect)
        });
        let Some((on_screen, window_rect)) = native else {
            let hidden = window.hide().is_ok();
            return (hidden, None);
        };
        if !on_screen {
            // Already hidden or minimized: there is nothing to hide, and the
            // capture must not re-show the window afterwards either.
            return (false, None);
        }
        let proof = window_rect
            .and_then(|rect| screen_capture::primary_monitor()?.intersection(&rect))
            .and_then(|region| {
                screen_capture::sample_region_preview(region)
                    .map(|reference| HideProof { region, reference })
            });
        let hidden = window.hide().is_ok();
        (hidden, if hidden { proof } else { None })
    }
    #[cfg(not(windows))]
    {
        match app.upgrade() {
            Some(window) => (window.hide().is_ok(), None),
            None => (false, None),
        }
    }
}

/// Wait on the capture worker until the freshly hidden main window is really
/// gone from the desktop. `ShowWindow(SW_HIDE)` lands synchronously, but what
/// the screen serves afterwards depends on the DWM composition (or, without
/// DWM, on the exposed underlying windows repainting), and neither settles at
/// a predictable time — every fixed wait raced that and froze the window
/// mid-hide into the captured frame. So the worker re-samples the region the
/// window occupied and returns only once its pixels have changed against the
/// pre-hide reference. The deadline only bounds the case where the window was
/// mostly covered by other windows and its own pixels can legitimately never
/// reach half of the region.
///
/// No DwmFlush anywhere: on a static desktop it returns in ~5 ms, and measured
/// right after a hide it came back in 13 ms with the ghost still fully on
/// screen, so it does not force the composition that drops the window — it only
/// delays the first useful read. The change itself lands between 35 ms and
/// 75 ms after the hide for a window covering the whole monitor, and the pixels
/// are the only signal that says so.
#[cfg(windows)]
fn wait_until_window_gone(proof: Option<&HideProof>) {
    let Some(proof) = proof else {
        // No verifiable region: the rect was unavailable or the window does
        // not touch the capture monitor. A short settle is all that is
        // provable, matching the old behavior.
        if PROBE_VERBOSE.load(Ordering::Relaxed) {
            println!("probe: no verifiable region, fixed settle only");
        }
        std::thread::sleep(HIDE_SETTLE);
        return;
    };
    let started = std::time::Instant::now();
    let deadline = started + HIDE_POLL_DEADLINE;
    loop {
        let cleared = match screen_capture::sample_region_preview(proof.region) {
            Some(sample) => {
                let cleared = ghost_cleared(&sample, &proof.reference);
                if PROBE_VERBOSE.load(Ordering::Relaxed) {
                    println!(
                        "probe: t={:.0}ms diff={:.4} cleared={cleared}",
                        probe_ms(started.elapsed()),
                        diff_fraction(&sample, &proof.reference)
                    );
                }
                cleared
            }
            None => {
                if PROBE_VERBOSE.load(Ordering::Relaxed) {
                    println!("probe: sampler returned nothing");
                }
                false
            }
        };
        if cleared {
            return;
        }
        if std::time::Instant::now() >= deadline {
            if PROBE_VERBOSE.load(Ordering::Relaxed) {
                println!("probe: hide verification reached its deadline");
            }
            return;
        }
        std::thread::sleep(HIDE_POLL_INTERVAL);
    }
}

/// `--hide-probe` only: trace the verdict curve of the hide wait.
#[cfg(windows)]
static PROBE_VERBOSE: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
fn probe_ms(value: std::time::Duration) -> f64 {
    value.as_secs_f64() * 1000.0
}

#[cfg(not(windows))]
fn wait_until_window_gone(_proof: Option<&HideProof>) {
    std::thread::sleep(HIDE_SETTLE);
}

/// Settle used when the hide cannot be verified by pixels.
const HIDE_SETTLE: std::time::Duration = std::time::Duration::from_millis(240);

/// Poll pacing: about one frame at 60 Hz, so the verdict follows the real
/// recomposition instead of sitting on a fixed guess.
#[cfg(windows)]
const HIDE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

/// Upper bound on the wait, reached only when the pixels can legitimately never
/// change (the region was covered by other windows the whole time).
#[cfg(windows)]
const HIDE_POLL_DEADLINE: std::time::Duration = std::time::Duration::from_millis(800);

/// The ghost is gone when the region no longer reads as the window: either the
/// sampler returned a different shape (broken read, never stall on it) or more
/// than half of the sampled grid changed. Dropping the window is a single jump —
/// measured 0.001 → 0.970 for a full-monitor window, with all four horizontal
/// bands flipping together (the one band that trails holds the taskbar strip),
/// so a majority costs nothing in latency. A small partial change while the
/// ghost is still composed can only come from windows *above* ours, and a low
/// threshold would let an animated overlap (video, blinking caret) pass the
/// ghost off as the desktop. A mostly covered window legitimately clears under
/// half of the region and is served by the deadline instead: being late is
/// safe, being wrong is not.
#[cfg(windows)]
fn ghost_cleared(sample: &[u8], reference: &[u8]) -> bool {
    sample.len() != reference.len() || diff_fraction(sample, reference) > 0.5
}

/// Fraction of sampled pixels that differ; zero when the lengths disagree so
/// a broken sampler can never read as "cleared".
#[cfg(windows)]
fn diff_fraction(sample: &[u8], reference: &[u8]) -> f64 {
    if sample.len() != reference.len() || sample.len() < 4 {
        return 0.0;
    }
    let total = sample.len() / 4;
    let changed = sample
        .chunks_exact(4)
        .zip(reference.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    changed as f64 / total as f64
}

/// `--hide-probe` diagnostic: drive the real `capture()` production entry —
/// the tray keeps the event loop alive through the hide, exactly like a normal
/// session — and dump the frozen frame the selector would show. A status-line
/// change right before the capture stands in for the composer menu closing, so
/// a pending re-render is in flight at hide time just like in real use.
pub fn run_hide_probe(app: &MainWindow) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    PROBE_VERBOSE.store(true, Ordering::Relaxed);
    let weak = app.as_weak();
    // A timer rather than a sleep inside an event-loop callback: the loop has to
    // keep rendering during the settle, or the window is never really painted and
    // raised and the probe measures a situation production never has.
    slint::Timer::single_shot(
        std::time::Duration::from_millis(2500),
        {
            let weak = weak.clone();
            move || {
                if let Some(window) = weak.upgrade() {
                    // Pending re-render at hide time, mirroring the composer menu
                    // closing right before the production callback runs.
                    window.set_status("hide-probe".into());
                    // A window launched from a terminal is not allowed to take
                    // the foreground, and a covered window contributes no pixels
                    // to the screen, so hiding it changes nothing and the wait
                    // can only reach its deadline. Production always hides a
                    // window the user is looking at, so pin ours on top and give
                    // DWM a moment to composite the new z-order before the
                    // reference sample reads the screen.
                    raise_probe_window(&window);
                    slint::Timer::single_shot(
                        std::time::Duration::from_millis(300),
                        move || {
                            probe_occlusion(&window);
                            println!("probe: invoking production capture");
                            capture(window.as_weak(), true);
                        },
                    );
                    return;
                }
                println!("probe: window is gone, nothing to measure");
                let _ = slint::quit_event_loop();
            }
        },
    );
    std::thread::spawn(move || {
        // Wait for the production chain to finish, then dump the frame the
        // selector shows. This thread starts before the event loop runs
        // `capture()`, so it must first see the in-flight flag raised: polling
        // only for its clearing exited at once and dumped an empty slot.
        // PENDING lives on the UI thread, so inspect it through the event loop.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !CAPTURE_IN_FLIGHT.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // Everything the worker does before it hands the frame over: the hide
        // wait plus the full-screen blit. Well under 800 ms means the pixels
        // verified the hide; at or above it the deadline fallback served it.
        let began = std::time::Instant::now();
        let deadline = began + std::time::Duration::from_secs(20);
        while CAPTURE_IN_FLIGHT.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        println!(
            "probe: capture chain finished in {:.0}ms",
            began.elapsed().as_secs_f64() * 1000.0
        );
        let _ = slint::invoke_from_event_loop(move || {
            PENDING.with(|pending| {
                let borrowed = pending.borrow();
                match borrowed.as_ref() {
                    Some((frame, _)) => {
                        println!(
                            "probe: frozen frame {}x{} — dumping",
                            frame.width, frame.height
                        );
                        dump_frame_png(frame, "wunder-hide-probe-frozen.png");
                    }
                    None => println!("probe: no frozen frame (selector did not open)"),
                }
            });
            hide_selector();
            let _ = slint::quit_event_loop();
        });
    });
    slint::run_event_loop_until_quit()?;
    Ok(())
}

/// `--hide-probe` only: put the window on top of the z-order, so the hide is
/// something the screen sampling can see at all.
#[cfg(windows)]
fn raise_probe_window(window: &MainWindow) {
    use slint::winit_030::WinitWindowAccessor;
    let _ = window.window().with_winit_window(|native| {
        native.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
    });
}

#[cfg(not(windows))]
fn raise_probe_window(_window: &MainWindow) {}

/// `--hide-probe` diagnostic: was the main window really the topmost thing
/// inside its own rectangle? A covered window changes no pixels when hidden, so
/// the verification can only reach its deadline, and a clean frame from that run
/// proves nothing about the verdict.
#[cfg(windows)]
fn probe_occlusion(window: &MainWindow) {
    use slint::winit_030::WinitWindowAccessor;
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
        WindowFromPoint,
    };

    let own = std::process::id();
    let _ = window.window().with_winit_window(|native| {
        let Some(position) = native.outer_position().ok() else {
            println!("probe: no window position");
            return;
        };
        let size = native.outer_size();
        let center = POINT {
            x: position.x + size.width as i32 / 2,
            y: position.y + size.height as i32 / 2,
        };
        let mut hit_pid = 0u32;
        let hit = unsafe { WindowFromPoint(center) };
        unsafe { GetWindowThreadProcessId(hit, &mut hit_pid) };
        let mut class = vec![0u16; 64];
        let mut title = vec![0u16; 64];
        let class_len = unsafe { GetClassNameW(hit, class.as_mut_ptr(), 63) };
        let title_len = unsafe { GetWindowTextW(hit, title.as_mut_ptr(), 63) };
        let mut fore_pid = 0u32;
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut fore_pid) };
        println!(
            "probe: window {}x{} at ({},{}) visible={:?} minimized={:?} foreground-pid={fore_pid} (self {own}) center={} {:?} {:?}",
            size.width,
            size.height,
            position.x,
            position.y,
            native.is_visible(),
            native.is_minimized(),
            hit_pid,
            String::from_utf16_lossy(&class[..class_len.max(0) as usize]),
            String::from_utf16_lossy(&title[..title_len.max(0) as usize]),
        );
        // What the screen really reads where the window should be. If this shows
        // another application, our window never got on top and the hide has no
        // pixels of its own to lose.
        let rect = screen_capture::MonitorRect {
            left: position.x,
            top: position.y,
            right: position.x + size.width as i32,
            bottom: position.y + size.height as i32,
        };
        let region = screen_capture::primary_monitor().and_then(|monitor| monitor.intersection(&rect));
        let Some(region) = region else {
            println!("probe: window outside the capture monitor");
            return;
        };
        let (width, height) = screen_capture::preview_size(region);
        if let Some(bgra) = screen_capture::sample_region_preview(region) {
            dump_frame_png(
                &screen_capture::ScreenCapture {
                    rgba: bgra,
                    width,
                    height,
                    origin_x: region.left,
                    origin_y: region.top,
                },
                "wunder-hide-probe-before.png",
            );
        }
        // The whole monitor, taskbar included: tells a window that is really
        // absent from the interactive desktop apart from a window that is only
        // clipped out of the sampled rectangle.
        if let Ok(frame) = screen_capture::capture_screen() {
            dump_frame_png(&frame, "wunder-hide-probe-desktop.png");
        }
    });
}

#[cfg(not(windows))]
fn probe_occlusion(_window: &MainWindow) {}

/// Write a captured frame back as a PNG for the diagnostic log.
fn dump_frame_png(frame: &screen_capture::ScreenCapture, name: &str) {
    let mut rgba = frame.rgba.clone();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 0xff;
    }
    let path = std::env::temp_dir().join(name);
    match encode_png(frame.width, frame.height, &rgba)
        .and_then(|png| std::fs::write(&path, png).map_err(|error| error.to_string()))
    {
        Ok(()) => println!("probe: saved {}", path.display()),
        Err(error) => println!("probe: dump {name} failed {error}"),
    }
}

thread_local! {
    static OVERLAY: RefCell<Option<ScreenshotOverlay>> = const { RefCell::new(None) };
    /// The frozen frame plus the exact scale its logical selection space was
    /// built with. Cropping reuses that factor so a selection can never be
    /// rescaled against a different one.
    static PENDING: RefCell<Option<(screen_capture::ScreenCapture, f32)>> = const { RefCell::new(None) };
}

/// Start an interactive primary-screen capture. Repeated hotkey presses while
/// the selector is visible are ignored to keep the in-memory frame bounded.
pub fn capture(app: slint::Weak<MainWindow>, hide_window: bool) {
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
    let (hidden, proof) = if hide_window {
        hide_main_window(&app)
    } else {
        (false, None)
    };
    std::thread::spawn(move || {
        if hidden {
            wait_until_window_gone(proof.as_ref());
        }
        let result = screen_capture::capture_screen();
        let _ = slint::invoke_from_event_loop(move || {
            if hidden {
                if let Some(window) = app.upgrade() {
                    let _ = window.show();
                }
            }
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
pub fn capture_fullscreen(app: slint::Weak<MainWindow>, hide_window: bool) {
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
    let (hidden, proof) = if hide_window {
        hide_main_window(&app)
    } else {
        (false, None)
    };
    std::thread::spawn(move || {
        if hidden {
            wait_until_window_gone(proof.as_ref());
        }
        let result = screen_capture::capture_screen()
            .and_then(|frame| save_attachment(frame.width, frame.height, &frame.rgba));
        let _ = slint::invoke_from_event_loop(move || {
            if hidden {
                if let Some(window) = app.upgrade() {
                    let _ = window.show();
                }
            }
            CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
            match result {
                Ok((name, data_url)) => add_attachment(&app, name, data_url, "全屏截图"),
                Err(error) => report(&app, format!("截图失败：{error}")),
            }
        });
    });
}

fn show_selector(app: slint::Weak<MainWindow>, frame: screen_capture::ScreenCapture) {
    let (width, height) = (frame.width, frame.height);
    let (origin_x, origin_y) = (frame.origin_x, frame.origin_y);
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
            slint::SharedPixelBuffer::clone_from_slice(&frame.rgba, width, height),
        ));
        overlay.set_sel_x(0.0);
        overlay.set_sel_y(0.0);
        overlay.set_sel_w(0.0);
        overlay.set_sel_h(0.0);
        // Physical sizing avoids DPI rounding on legacy primary monitors, and
        // the capture origin is reused verbatim so the selector covers exactly
        // the captured pixels instead of a virtual-desktop approximation.
        let window = overlay.window();
        window.set_position(slint::PhysicalPosition::new(origin_x, origin_y));
        window.set_size(slint::PhysicalSize::new(width, height));
        PENDING.with(|pending| *pending.borrow_mut() = Some((frame, 1.0)));
        overlay
            .show()
            .map_err(|error| format!("无法显示截图选区：{error}"))?;
        // Re-assert after the native window exists: creation-time placement can
        // still resolve asynchronously, and the scale factor must describe the
        // monitor the selector actually landed on. Reading it before `show`
        // returned a provisional factor, which left the selection space
        // disagreeing with the frozen frame.
        window.set_position(slint::PhysicalPosition::new(origin_x, origin_y));
        window.set_size(slint::PhysicalSize::new(width, height));
        let scale = window.scale_factor().max(1.0);
        overlay.set_screen_width(width as f32 / scale);
        overlay.set_screen_height(height as f32 / scale);
        PENDING.with(|pending| {
            if let Some(slot) = pending.borrow_mut().as_mut() {
                slot.1 = scale;
            }
        });
        window.request_redraw();
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
    let frame = PENDING.with(|pending| pending.borrow_mut().take());
    hide_selector();
    let Some((frame, scale)) = frame else {
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
    let Some(app) = app.upgrade() else { return };
    if app.window().is_visible() {
        app.set_status(message.into());
    } else {
        // 窗口驻留托盘时状态栏不可见，把结果临时写进托盘 tooltip。
        crate::tray::announce(&message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn ghost_verdict_requires_a_substantial_change() {
        let reference = vec![1u8, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255];
        // Identical pixels: the ghost is still composed.
        assert!(!ghost_cleared(&reference, &reference));
        // A tiny overlay-sized change must not read as "the window is gone".
        let mut ghost = reference.clone();
        ghost[0] = 9;
        assert!(!ghost_cleared(&ghost, &reference));
        // Most pixels changing means the desktop is showing through.
        let mut cleared = reference.clone();
        for (index, pixel) in cleared.chunks_exact_mut(4).enumerate() {
            if index % 4 == 3 {
                continue;
            }
            pixel[0] = pixel[0].wrapping_add(200);
            pixel[1] = pixel[1].wrapping_add(200);
            pixel[2] = pixel[2].wrapping_add(200);
        }
        assert!(ghost_cleared(&cleared, &reference));
        // A sampler that returned a differently sized buffer must never read
        // as "the pixels are still there".
        assert!(ghost_cleared(&reference[..4], &reference));
        assert!(ghost_cleared(&[], &reference));
    }

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
            origin_x: 0,
            origin_y: 0,
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
