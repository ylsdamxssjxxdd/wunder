//! Region screenshot flow for the native desktop UI.
//!
//! GDI capture and PNG compression never run on Slint's UI thread. The frozen
//! frame is retained only while the selector is open; confirmation puts the
//! result both in the composer as a native chat attachment and on the system
//! clipboard, so it can be pasted into any other program.

use crate::{
    clipboard_image, image_attachment, screen_capture, CaptureRegion, MainWindow, ScreenshotOverlay,
};
use slint::ComponentHandle;
use std::{
    cell::{Cell, RefCell},
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
};

static CAPTURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Opt-in diagnostics for the on-demand detection path. Set
/// `WUNDER_SHOT_DEBUG=1` in the launching environment; normal runs stay quiet.
fn shot_trace_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("WUNDER_SHOT_DEBUG").is_some())
}

macro_rules! shot_trace {
    ($($arg:tt)*) => {
        if shot_trace_on() {
            eprintln!("[shot] {}", format_args!($($arg)*));
        }
    };
}

/// How long the selector gives the window manager to size its surface before
/// verifying it against the frozen frame. A fullscreen request for a window
/// that already exists is applied as a normal property change, so it lands
/// within a frame or two; the check is only a safety net.
const SELECTOR_SETTLE: u64 = 120;

/// Proof that the freshly hidden main window is really off the captured
/// screen. `region` is the window's screen rectangle clipped to the capture
/// monitor; `signature` is the coarse color signature of that region from just
/// before the hide, i.e. what a too-early capture would inherit from the window.
#[cfg(windows)]
struct HideProof {
    region: screen_capture::MonitorRect,
    signature: Vec<u8>,
}

/// Native placement saved while the main HWND is kept out of the desktop.
/// Moving it away before hiding gives DWM an actual surface change to compose;
/// a plain SW_HIDE can otherwise leave the previous software-rendered frame in
/// the screen DC for an unbounded number of reads.
#[cfg(windows)]
struct HideState {
    proof: Option<HideProof>,
    hwnd: isize,
    rect: screen_capture::MonitorRect,
    maximized: bool,
}

#[cfg(not(windows))]
struct HideState {
    /// Outer screen position and inner size immediately before the hide.
    /// Window managers are free to place a remapped toplevel themselves —
    /// Mutter does — so without reasserting this frame the workbench came
    /// back somewhere else after every hidden-window capture.
    position: Option<(i32, i32)>,
    size: Option<(u32, u32)>,
    maximized: bool,
}

/// Hide the main window on the UI thread and pin down what the capture would
/// otherwise inherit from it. Must run on the UI thread, and the reference
/// sample plus the hide happen back to back without an event-loop pass in
/// between, so no render can slip between them: the ghost a premature capture
/// would freeze into the frame is pixel-identical to the reference. The
/// reference is a coarse downscaled read: measured 6–22 ms of UI-thread work
/// and ~150 KB instead of a 15 MB buffer per read, and a short blit is far less
/// likely to be torn by a repaint *during* the blit, which would read as
/// changed pixels while the ghost is still composed.
fn hide_main_window(app: &slint::Weak<MainWindow>) -> (bool, Option<HideState>) {
    #[cfg(windows)]
    {
        use slint::winit_030::winit::raw_window_handle::HasWindowHandle;
        use slint::winit_030::WinitWindowAccessor;
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::Graphics::Dwm::DwmFlush;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowRect, IsWindowVisible, SetWindowPos, HWND_BOTTOM, SWP_HIDEWINDOW,
            SWP_NOACTIVATE, SWP_NOSENDCHANGING, SWP_NOSIZE,
        };

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
            // Maximized windows can report a stale Winit outer rectangle for
            // one message turn after the work-area/DPI update. Query the HWND
            // rectangle immediately before hiding so the proof covers exactly
            // the pixels that the desktop capture can still contain.
            let rect = native
                .window_handle()
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    slint::winit_030::winit::raw_window_handle::RawWindowHandle::Win32(handle) => {
                        let mut native_rect: RECT = unsafe { std::mem::zeroed() };
                        let hwnd = handle.hwnd.get() as isize;
                        (unsafe { GetWindowRect(hwnd, &mut native_rect) } != 0).then_some(
                            screen_capture::MonitorRect {
                                left: native_rect.left,
                                top: native_rect.top,
                                right: native_rect.right,
                                bottom: native_rect.bottom,
                            },
                        )
                    }
                    _ => None,
                })
                .or(rect);
            let hwnd = native
                .window_handle()
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    slint::winit_030::winit::raw_window_handle::RawWindowHandle::Win32(handle) => {
                        Some(handle.hwnd.get() as isize)
                    }
                    _ => None,
                });
            let maximized = hwnd.is_some() && native.is_maximized();
            (on_screen, rect, hwnd, maximized)
        });
        let Some((on_screen, window_rect, hwnd, maximized)) = native else {
            let hidden = window.hide().is_ok();
            return (hidden, None);
        };
        if !on_screen {
            // Already hidden or minimized: there is nothing to hide, and the
            // capture must not re-show the window afterwards either.
            return (false, None);
        }
        let Some(hwnd) = hwnd else {
            let hidden = window.hide().is_ok();
            return (hidden, None);
        };
        let Some(window_rect) = window_rect else {
            let hidden = window.hide().is_ok();
            return (hidden, None);
        };
        let proof = screen_capture::primary_monitor()
            .and_then(|monitor| monitor.intersection(&window_rect))
            .and_then(|region| {
                let preview = screen_capture::sample_region_preview(region)?;
                let (width, height) = screen_capture::preview_size(region);
                Some(HideProof {
                    region,
                    signature: screen_capture::preview_signature(
                        &preview,
                        width,
                        height,
                        screen_capture::SIGNATURE_GRID,
                    ),
                })
            });
        // Move the real HWND outside the virtual desktop before hiding it.
        // This is deliberately done on the UI thread, immediately after the
        // reference sample, so DWM cannot keep serving the old surface while
        // the worker begins its verification loop.
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                window_rect.right.saturating_add(32),
                window_rect.bottom.saturating_add(32),
                0,
                0,
                SWP_HIDEWINDOW | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
            );
            let _ = DwmFlush();
        }
        // The Win32 visibility bit is authoritative. Do not call Slint's
        // window.hide() after this atomic operation: Winit may enqueue a
        // software-surface repaint while applying set_visible(false), which
        // is exactly the gray frame this path is meant to avoid.
        let hidden = unsafe { IsWindowVisible(hwnd) == 0 };
        (
            hidden,
            hidden.then_some(HideState {
                proof,
                hwnd,
                rect: window_rect,
                maximized,
            }),
        )
    }
    #[cfg(not(windows))]
    {
        match app.upgrade() {
            Some(window) => {
                use slint::winit_030::WinitWindowAccessor;
                let frame = window.window().with_winit_window(|native| {
                    let position = native
                        .outer_position()
                        .ok()
                        .map(|position| (position.x, position.y));
                    let size = native.inner_size();
                    (
                        position,
                        Some((size.width, size.height)),
                        native.is_maximized(),
                    )
                });
                let hidden = window.hide().is_ok();
                let state = hidden.then(|| {
                    let (position, size, maximized) = frame.unwrap_or((None, None, false));
                    HideState {
                        position,
                        size,
                        maximized,
                    }
                });
                (hidden, state)
            }
            None => (false, None),
        }
    }
}

/// `--hide-probe` only: trace the post-capture ghost check.
#[cfg(windows)]
static PROBE_VERBOSE: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
fn probe_ms(value: std::time::Duration) -> f64 {
    value.as_secs_f64() * 1000.0
}

/// Extra reads allowed when the window's own pixels are still in the frame.
#[cfg(windows)]
const HIDE_RETRY_LIMIT: usize = 3;

/// Spacing between those reads: about one compositor frame.
#[cfg(windows)]
const HIDE_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

/// Mean per-channel difference under which the newly captured region still
/// looks like the window that was just hidden. Area-averaged cell means of the
/// same pixels land well inside this; a different desktop behind the window
/// lands far above it. A near-identical wallpaper can read as "still there",
/// which only costs a few extra reads before the frame is accepted anyway.
#[cfg(windows)]
const HIDE_SIGNATURE_TOLERANCE: f32 = 14.0;

/// Mean absolute RGB difference between two coarse signatures, `f32::MAX` when
/// they cannot be compared at all.
#[cfg(windows)]
fn signature_difference(sample: &[u8], reference: &[u8]) -> f32 {
    if sample.len() != reference.len() || sample.len() < 3 {
        return f32::MAX;
    }
    let mut sum = 0u64;
    for (left, right) in sample.chunks_exact(3).zip(reference.chunks_exact(3)) {
        sum += (left[0] as i32 - right[0] as i32).unsigned_abs() as u64
            + (left[1] as i32 - right[1] as i32).unsigned_abs() as u64
            + (left[2] as i32 - right[2] as i32).unsigned_abs() as u64;
    }
    sum as f32 / sample.len() as f32
}

/// True when the freshly read region still looks like the window that was just
/// hidden, i.e. the frame has to be read again.
#[cfg(windows)]
fn signatures_match(sample: &[u8], reference: &[u8]) -> bool {
    signature_difference(sample, reference) <= HIDE_SIGNATURE_TOLERANCE
}

/// Capture the desktop with the main window already parked outside it.
///
/// The hide is flushed through DWM on the UI thread before the worker starts,
/// so the first read is normally clean. The pre-hide signature only serves the
/// legacy case where the exposed desktop keeps serving the old surface: if the
/// captured region still reads like the window, read again a frame later. The
/// old implementation polled the live screen *before* capturing with an 800 ms
/// deadline (plus a 240 ms settle) and every F1 paid that wait even when the
/// very first read was already correct.
#[cfg(windows)]
fn capture_after_hide(
    proof: Option<&HideProof>,
) -> Result<screen_capture::ScreenCapture, String> {
    let mut frame = screen_capture::capture_screen()?;
    let Some(proof) = proof else {
        return Ok(frame);
    };
    if proof.signature.is_empty() {
        return Ok(frame);
    }
    let started = std::time::Instant::now();
    for attempt in 0..=HIDE_RETRY_LIMIT {
        let sample = screen_capture::frame_region_signature(
            &frame,
            proof.region,
            screen_capture::SIGNATURE_GRID,
        );
        let still_window = sample
            .as_deref()
            .is_some_and(|sample| signatures_match(sample, &proof.signature));
        if PROBE_VERBOSE.load(Ordering::Relaxed) {
            let difference = sample
                .as_deref()
                .map(|sample| signature_difference(sample, &proof.signature))
                .unwrap_or(f32::MAX);
            println!(
                "probe: attempt={attempt} diff={difference:.1} t={:.0}ms",
                probe_ms(started.elapsed())
            );
        }
        if !still_window || attempt == HIDE_RETRY_LIMIT {
            return Ok(frame);
        }
        std::thread::sleep(HIDE_RETRY_INTERVAL);
        frame = screen_capture::capture_screen()?;
    }
    Ok(frame)
}

/// Give a hidden HWND time to leave the desktop on platforms where the
/// compositor state cannot be read back from the capture worker.
#[cfg(not(windows))]
fn settle_hidden_window(_state: Option<&HideState>) {
    std::thread::sleep(HIDE_SETTLE);
}

/// Placement flags used when the hidden window is mapped again. `SWP_SHOWWINDOW`
/// is not optional: the hide bypassed Winit, so nothing else maps the HWND again.
#[cfg(windows)]
const REMAP_FLAGS: u32 = windows_sys::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW
    | windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
    | windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOSENDCHANGING;

#[cfg(windows)]
fn restore_hidden_window(app: &slint::Weak<MainWindow>, state: &HideState) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsWindowVisible, SetWindowPos, ShowWindow, HWND_TOP, SW_MAXIMIZE, SW_SHOW,
    };
    // The hide went straight to the HWND, so Winit's cached WindowFlags still
    // contain VISIBLE and Slint still believes the window is shown. Both
    // `Window::show()` and `set_visible(true)` therefore diff to nothing and
    // emit no mapping command, which is exactly how a hidden-window capture
    // used to leave the window unmapped for the rest of the session. Cycle the
    // platform state so Winit drops VISIBLE first, then map it for real; the
    // cycle also clears the stale software surface through a full repaint.
    if unsafe { IsWindowVisible(state.hwnd) } == 0 {
        if let Some(window) = app.upgrade() {
            let _ = window.hide();
            let _ = window.show();
        }
    }
    let width = state.rect.width().max(1);
    let height = state.rect.height().max(1);
    unsafe {
        if state.maximized {
            // The hide parked the HWND off the virtual desktop. Remapping it
            // in place and maximizing there resolves the monitor through
            // `MonitorFromRect(MONITOR_DEFAULTTONULL)`, which fails for an
            // off-screen rect: no work area comes back and the maximized
            // window lands on the full monitor, hiding the taskbar. Put the
            // window back on its recorded (on-screen) rectangle first, then
            // reassert the maximized state so WM_NCCALCSIZE clamps to the
            // work area again.
            SetWindowPos(
                state.hwnd,
                HWND_TOP,
                state.rect.left,
                state.rect.top,
                width,
                height,
                REMAP_FLAGS,
            );
            ShowWindow(state.hwnd, SW_MAXIMIZE);
        } else {
            // Put the HWND back at the exact physical rectangle captured
            // before hiding, in the same call that shows it. Nothing can apply
            // a cached position afterwards because the platform state cycle
            // above already ran.
            SetWindowPos(
                state.hwnd,
                HWND_TOP,
                state.rect.left,
                state.rect.top,
                width,
                height,
                REMAP_FLAGS,
            );
        }
        if IsWindowVisible(state.hwnd) == 0 {
            // Last resort against a shell that dropped the show bit while the
            // window was not allowed to take the foreground.
            ShowWindow(state.hwnd, SW_SHOW);
        }
    }
    if let Some(window) = app.upgrade() {
        window.window().request_redraw();
    }
}

#[cfg(not(windows))]
fn restore_hidden_window(app: &slint::Weak<MainWindow>, state: &HideState) {
    let Some(window) = app.upgrade() else {
        return;
    };
    let _ = window.show();
    let placement = (state.position, state.size, state.maximized);
    apply_hidden_placement(app, placement);
    // The map request only starts the remap; a window manager may apply its
    // own placement once it processes it. Reassert the recorded frame one map
    // turn later so the workbench always comes back where it was hidden.
    let retry = app.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(80), move || {
        apply_hidden_placement(&retry, placement);
    });
}

/// Put the remapped workbench back on the frame recorded before the hide.
#[cfg(not(windows))]
fn apply_hidden_placement(
    app: &slint::Weak<MainWindow>,
    placement: (Option<(i32, i32)>, Option<(u32, u32)>, bool),
) {
    use slint::winit_030::{winit, WinitWindowAccessor};
    let (position, size, maximized) = placement;
    let Some(window) = app.upgrade() else {
        return;
    };
    let _ = window.window().with_winit_window(|native| {
        if maximized {
            native.set_maximized(true);
            return;
        }
        if let Some((width, height)) = size.filter(|(width, height)| *width > 0 && *height > 0) {
            let target = winit::dpi::PhysicalSize::new(width, height);
            if native.inner_size() != target {
                let _ = native.request_inner_size(target);
            }
        }
        if let Some((x, y)) = position {
            let target = winit::dpi::PhysicalPosition::new(x, y);
            if native.outer_position().ok() != Some(target) {
                native.set_outer_position(target);
            }
        }
    });
}

/// Settle used on platforms without a readable compositor state.
#[cfg(not(windows))]
const HIDE_SETTLE: std::time::Duration = std::time::Duration::from_millis(240);

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
    slint::Timer::single_shot(std::time::Duration::from_millis(2500), {
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
                slint::Timer::single_shot(std::time::Duration::from_millis(300), move || {
                    probe_occlusion(&window);
                    println!("probe: invoking production capture");
                    capture(window.as_weak(), true);
                });
                return;
            }
            println!("probe: window is gone, nothing to measure");
            let _ = slint::quit_event_loop();
        }
    });
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
                    Some(frame) => {
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
    match image_attachment::encode_png(frame.width, frame.height, &rgba)
        .and_then(|png| std::fs::write(&path, png).map_err(|error| error.to_string()))
    {
        Ok(()) => println!("probe: saved {}", path.display()),
        Err(error) => println!("probe: dump {name} failed {error}"),
    }
}

thread_local! {
    static OVERLAY: RefCell<Option<ScreenshotOverlay>> = const { RefCell::new(None) };
    /// The frozen frame the selector is drawing over, kept only while the
    /// selector is open. The selector returns the crop rectangle already
    /// converted to this frame's physical pixels, so no scale factor has to be
    /// remembered (and none can go stale between showing and confirming).
    static PENDING: RefCell<Option<screen_capture::ScreenCapture>> = const { RefCell::new(None) };
    /// Candidate window trees of the current capture, in frame-local physical
    /// pixels. The selector asks for the rect under the pointer on every move;
    /// keeping the tree here lets the reused selector window always test
    /// against the latest capture without rebuilding its callbacks.
    static HIT_REGIONS: RefCell<Vec<screen_capture::RegionNode>> =
        const { RefCell::new(Vec::new()) };
    /// The candidate tree is collected only when the user asks for automatic
    /// detection (Space). Capturing a screenshot no longer pays for it.
    static HIT_REGIONS_READY: Cell<bool> = const { Cell::new(false) };
    /// Window-scoped UI Automation probe. Created on the first capture and
    /// reused for the rest of the session; `None` when the probe thread or the
    /// UIA client could not be created, in which case hover stays at the
    /// window level.
    #[cfg(windows)]
    static UIA_PROBE: RefCell<Option<crate::uia_probe::Probe>> = const { RefCell::new(None) };
    /// A failed probe-thread start is not retried on every key press; the
    /// selector degrades to window rectangles for the rest of the session.
    #[cfg(windows)]
    static UIA_PROBE_TRIED: Cell<bool> = const { Cell::new(false) };
    /// Bumped for every capture so an answer computed for an older frame is
    /// never applied to the selector that is now showing.
    #[cfg(windows)]
    static PROBE_EPOCH: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Frame origin of the current capture, used to turn UIA's screen
    /// rectangles back into frame-local pixels.
    #[cfg(windows)]
    static PROBE_ORIGIN: std::cell::Cell<(i32, i32)> = const { std::cell::Cell::new((0, 0)) };
}

/// Move one window tree into frame-local coordinates: the selector lays out in
/// the frame's own pixels, so every desktop-space rect is shifted by the frame
/// origin before it can be compared with the pointer.
fn into_frame_local(
    node: screen_capture::RegionNode,
    origin_x: i32,
    origin_y: i32,
) -> screen_capture::RegionNode {
    screen_capture::RegionNode {
        rect: screen_capture::MonitorRect {
            left: node.rect.left - origin_x,
            top: node.rect.top - origin_y,
            right: node.rect.right - origin_x,
            bottom: node.rect.bottom - origin_y,
        },
        children: node
            .children
            .into_iter()
            .map(|child| into_frame_local(child, origin_x, origin_y))
            .collect(),
        hwnd: node.hwnd,
    }
}

/// Install the workbench screenshot shortcuts: F1 hides the window first, F2
/// captures it as it stands. Both stay scoped to the workbench window (see
/// [`crate::hotkey`]), so they never take F1/F2 away from other programs.
pub fn install_shortcuts(app: &MainWindow) {
    let weak = app.as_weak();
    crate::hotkey::install(app, move |action| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = weak.upgrade() {
                let hide_window = action == crate::hotkey::Action::HideAndCapture;
                capture(app.as_weak(), hide_window);
            }
        });
    });
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
        #[cfg(windows)]
        let captured = if hidden {
            capture_after_hide(proof.as_ref().and_then(|state| state.proof.as_ref()))
        } else {
            screen_capture::capture_screen()
        };
        #[cfg(not(windows))]
        let captured = {
            if hidden {
                settle_hidden_window(proof.as_ref());
            }
            screen_capture::capture_screen()
        };
        let result = captured;
        let _ = slint::invoke_from_event_loop(move || {
            if hidden {
                if let Some(state) = proof.as_ref() {
                    restore_hidden_window(&app, state);
                } else if let Some(window) = app.upgrade() {
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
        #[cfg(windows)]
        let captured = if hidden {
            capture_after_hide(proof.as_ref().and_then(|state| state.proof.as_ref()))
        } else {
            screen_capture::capture_screen()
        };
        #[cfg(not(windows))]
        let captured = {
            if hidden {
                settle_hidden_window(proof.as_ref());
            }
            screen_capture::capture_screen()
        };
        let result = captured.map(|frame| {
            let saved = save_attachment(frame.width, frame.height, &frame.rgba);
            let copied = clipboard_image::publish(frame.width, frame.height, frame.rgba);
            (saved, copied)
        });
        let _ = slint::invoke_from_event_loop(move || {
            if hidden {
                if let Some(state) = proof.as_ref() {
                    restore_hidden_window(&app, state);
                } else if let Some(window) = app.upgrade() {
                    let _ = window.show();
                }
            }
            CAPTURE_IN_FLIGHT.store(false, Ordering::Release);
            match result {
                Ok((Ok(draft), copied)) => add_attachment(&app, draft, "全屏截图", copied),
                Ok((Err(error), copied)) => report_unsaved(&app, "全屏截图", error, copied),
                Err(error) => report(&app, format!("截图失败：{}", error)),
            }
        });
    });
}

fn show_selector(app: slint::Weak<MainWindow>, frame: screen_capture::ScreenCapture) {
    let (width, height) = (frame.width, frame.height);
    let (origin_x, origin_y) = (frame.origin_x, frame.origin_y);
    // Detection is on-demand now, so the fresh frame starts without a
    // candidate tree; the first Space press collects it lazily.
    HIT_REGIONS.with(|slot| slot.borrow_mut().clear());
    HIT_REGIONS_READY.with(|ready| ready.set(false));
    // A new frame invalidates every element answer still in flight for the
    // previous frame, and the probe needs the frame origin to answer in the
    // selector's own pixel space.
    #[cfg(windows)]
    {
        PROBE_EPOCH.with(|epoch| epoch.set(epoch.get().wrapping_add(1)));
        PROBE_ORIGIN.with(|origin| origin.set((origin_x, origin_y)));
    }
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
            // Hit testing stays in Rust: the selector only knows its own
            // logical layout, while the candidates are physical-frame
            // rectangles arranged as a window tree.
            overlay.on_region_at(region_at);
            // Window-scoped UI Automation refinement: the selector keeps the
            // window outline while the probe looks for the control inside it.
            #[cfg(windows)]
            overlay.on_probe_at(|x, y, generation| {
                request_element_probe(x, y, generation);
            });
            *slot = Some(overlay);
        }
        let overlay = slot.as_ref().expect("screenshot overlay was initialized");
        overlay.set_screen_image(slint::Image::from_rgba8(
            slint::SharedPixelBuffer::clone_from_slice(&frame.rgba, width, height),
        ));
        // The selector converts its logical selection to these physical pixels
        // with its own laid-out size, so a stale DPI report cannot turn a
        // selection into a fraction of itself.
        overlay.set_frame_width(width as i32);
        overlay.set_frame_height(height as i32);
        // The selector window is reused across captures; drop the drag state
        // and any detection state a previous session may have left behind.
        overlay.set_auto_detect(false);
        overlay.invoke_arm();
        // A borderless top-level window whose size is merely requested can be
        // constrained to the work area. That makes a 1600 px capture render
        // into a 1516 px selector: the preview is scaled and the real taskbar
        // remains outside the dimming layer. Fullscreen owns the whole monitor,
        // including the taskbar strip; the physical placement below selects the
        // same monitor whose pixels were frozen.
        let window = overlay.window();
        // Show windowed at the frame rectangle first, then ask for fullscreen
        // on the mapped window: window managers ignore fullscreen hints sent
        // while the window is unmapped, which is exactly what the reused
        // selector hit on its second and later captures (the dialog then came
        // back as the previous 1860x1011 window and the preview was squeezed).
        window.set_fullscreen(false);
        window.set_position(slint::PhysicalPosition::new(origin_x, origin_y));
        window.set_size(slint::PhysicalSize::new(width, height));
        PENDING.with(|pending| *pending.borrow_mut() = Some(frame));
        overlay
            .show()
            .map_err(|error| format!("无法显示截图选区：{error}"))?;
        window.set_fullscreen(true);
        // Warm the UIA client while the selector opens. This only creates the
        // MTA thread and the automation object; no element is queried until the
        // detection key is pressed, so the capture itself stays free of probes.
        #[cfg(windows)]
        ensure_element_probe();
        // Safety net: if the window manager still refuses, place the surface on
        // the frame rectangle exactly. It may clamp that to the work area, but
        // the selector maps through its own size, so the crop still matches
        // what the dimmed preview showed.
        let weak = overlay.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(SELECTOR_SETTLE), move || {
            let Some(overlay) = weak.upgrade() else {
                return;
            };
            let window = overlay.window();
            let size = window.size();
            if size.width == width && size.height == height {
                return;
            }
            println!(
                "selector: surface {}x{} != frame {}x{}, placing it exactly",
                size.width, size.height, width, height
            );
            window.set_fullscreen(false);
            window.set_position(slint::PhysicalPosition::new(origin_x, origin_y));
            window.set_size(slint::PhysicalSize::new(width, height));
        });
        window.request_redraw();
        Ok(())
    });
    if let Err(error) = result {
        hide_selector();
        report(&app, format!("截图失败：{error}"));
    }
}

/// `x`/`y`/`width`/`height` are already in the frozen frame's physical pixels:
/// the selector maps its own logical selection against its laid-out size, so
/// this side never converts by a scale factor.
fn finish_selection(app: slint::Weak<MainWindow>, x: i32, y: i32, width: i32, height: i32) {
    shot_trace!("finish_selection region=({x},{y},{width},{height})");
    if width < 1 || height < 1 {
        hide_selector();
        return;
    }
    let frame = PENDING.with(|pending| pending.borrow_mut().take());
    hide_selector();
    let Some(frame) = frame else {
        report(&app, "截图失败：选区画面已失效".to_owned());
        return;
    };
    let (width, height, rgba) = crop(&frame, x, y, width, height);
    shot_trace!("finish_selection cropped={width}x{height} bytes={}", rgba.len());
    std::thread::spawn(move || {
        let result = save_attachment(width, height, &rgba);
        // The clipboard is an independent delivery path: the picture the user
        // just framed must be pasteable elsewhere even when the composer is
        // full or its temporary PNG could not be written.
        let copied = clipboard_image::publish(width, height, rgba);
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok(draft) => add_attachment(&app, draft, "截图", copied),
            Err(error) => report_unsaved(&app, "截图", error, copied),
        });
    });
}

/// Hand the picture to the composer and report both delivery paths in one
/// status line: the chip in the composer and the system clipboard are
/// independent, so the user is told which one actually happened.
fn add_attachment(
    app: &slint::Weak<MainWindow>,
    draft: image_attachment::AttachmentDraft,
    kind: &str,
    clipboard: Result<(), String>,
) {
    shot_trace!("add_attachment kind={kind}");
    if !image_attachment::push(app, draft) {
        return;
    }
    let Some(app) = app.upgrade() else { return };
    app.set_status(
        match clipboard {
            Ok(()) => format!("{}已放入输入区，并已复制到剪贴板", kind),
            Err(error) => format!("{}已放入输入区，但复制到剪贴板失败：{}", kind, error),
        }
        .into(),
    );
}

fn hide_selector() {
    shot_trace!("hide_selector");
    OVERLAY.with(|slot| {
        if let Some(overlay) = slot.borrow().as_ref() {
            overlay.invoke_disarm();
            // Leave fullscreen *while the window is still mapped*: a window
            // manager ignores window-state changes on an unmapped window, and
            // the toolkit would then believe the next capture's fullscreen
            // request was already satisfied.
            overlay.window().set_fullscreen(false);
            let _ = overlay.hide();
        }
    });
    PENDING.with(|pending| {
        pending.borrow_mut().take();
    });
    HIT_REGIONS.with(|slot| slot.borrow_mut().clear());
    HIT_REGIONS_READY.with(|ready| ready.set(false));
    #[cfg(windows)]
    PROBE_EPOCH.with(|epoch| epoch.set(epoch.get().wrapping_add(1)));
}

/// Collect the candidate window tree for the current frozen frame, once per
/// selector session. Captures no longer enumerate the desktop; the first
/// detection request pays for one pass while the screen is already frozen.
fn ensure_hit_regions() -> bool {
    if HIT_REGIONS_READY.with(|ready| ready.get()) {
        return true;
    }
    let started = std::time::Instant::now();
    let Some((origin_x, origin_y, width, height)) = PENDING.with(|pending| {
        pending
            .borrow()
            .as_ref()
            .map(|frame| (frame.origin_x, frame.origin_y, frame.width, frame.height))
    }) else {
        return false;
    };
    let monitor = screen_capture::MonitorRect {
        left: origin_x,
        top: origin_y,
        right: origin_x + width as i32,
        bottom: origin_y + height as i32,
    };
    let frame_regions: Vec<screen_capture::RegionNode> = screen_capture::detect_regions(monitor)
        .into_iter()
        .map(|node| into_frame_local(node, origin_x, origin_y))
        .collect();
    shot_trace!(
        "regions nodes={} roots={} elapsed={:?}",
        count_nodes(&frame_regions),
        frame_regions.len(),
        started.elapsed()
    );
    HIT_REGIONS.with(|slot| *slot.borrow_mut() = frame_regions);
    HIT_REGIONS_READY.with(|ready| ready.set(true));
    true
}

fn count_nodes(regions: &[screen_capture::RegionNode]) -> usize {
    regions
        .iter()
        .map(|node| 1 + count_nodes(&node.children))
        .sum()
}

/// Window rectangle under a frame pixel, answered on demand for the selector's
/// automatic detection key.
fn region_at(x: i32, y: i32) -> CaptureRegion {
    let empty = CaptureRegion {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };
    if !ensure_hit_regions() {
        shot_trace!("region_at({x},{y}) no frame");
        return empty;
    }
    HIT_REGIONS.with(|slot| match hit_region(&slot.borrow(), x, y) {
        Some(rect) => {
            shot_trace!(
                "region_at({x},{y}) -> {},{} {}x{}",
                rect.left,
                rect.top,
                rect.width(),
                rect.height()
            );
            CaptureRegion {
                x: rect.left,
                y: rect.top,
                w: rect.width(),
                h: rect.height(),
            }
        }
        None => {
            shot_trace!("region_at({x},{y}) -> none");
            empty
        }
    })
}

/// Rectangle under a frozen-frame pixel: the topmost candidate window that
/// contains the point, narrowed to the deepest nested child that also contains
/// it. `None` when the pointer is over nothing. Roots arrive in stacking order,
/// so the first containment is the window the user sees; children are in their
/// own sibling z-order, so the first containment there is the front one.
fn hit_region(
    regions: &[screen_capture::RegionNode],
    x: i32,
    y: i32,
) -> Option<screen_capture::MonitorRect> {
    hit_node(regions, x, y).map(|node| node.rect)
}

/// Candidate node under a frame pixel, keeping the handle so the element probe
/// can be scoped to the window the pointer is over.
fn hit_node<'a>(
    regions: &'a [screen_capture::RegionNode],
    x: i32,
    y: i32,
) -> Option<&'a screen_capture::RegionNode> {
    regions
        .iter()
        .find(|node| contains(node.rect, x, y))
        .map(|node| deepest_node(node, x, y))
}

fn contains(rect: screen_capture::MonitorRect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn deepest_node<'a>(
    node: &'a screen_capture::RegionNode,
    x: i32,
    y: i32,
) -> &'a screen_capture::RegionNode {
    for child in &node.children {
        if contains(child.rect, x, y) {
            return deepest_node(child, x, y);
        }
    }
    node
}

/// Start the element probe once per session. Failure is silent: the selector
/// keeps using the window tree, exactly as before UIA existed.
#[cfg(windows)]
fn ensure_element_probe() {
    if UIA_PROBE.with(|slot| slot.borrow().is_some()) {
        return;
    }
    // A failed start is remembered: pressing the detection key again must not
    // spawn another dead thread on every press.
    if UIA_PROBE_TRIED.with(|tried| tried.replace(true)) {
        return;
    }
    UIA_PROBE.with(|slot| {
        let probe = crate::uia_probe::spawn(|answer| {
            let _ = slint::invoke_from_event_loop(move || apply_element_answer(answer));
        });
        *slot.borrow_mut() = probe;
    });
}

/// Ask UI Automation for the control under a frozen-frame pixel. The request is
/// scoped to the HWND already found by the window tree, because a desktop-wide
/// hit test would return the selector's own topmost window instead.
#[cfg(windows)]
fn request_element_probe(x: i32, y: i32, generation: i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT};

    ensure_element_probe();
    let hwnd = HIT_REGIONS.with(|slot| {
        hit_node(&slot.borrow(), x, y)
            .map(|node| node.hwnd)
            .filter(|hwnd| *hwnd != 0)
            .map(|hwnd| {
                // Chromium-style apps paint their whole UI inside one child
                // HWND whose own UIA provider is empty, while the top-level
                // window owns the real element tree. Scope the probe to the
                // root ancestor so both native and self-drawn apps work.
                let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
                if root != 0 { root } else { hwnd }
            })
    });
    let Some(hwnd) = hwnd else {
        return;
    };
    let (origin_x, origin_y) = PROBE_ORIGIN.with(|origin| origin.get());
    let epoch = PROBE_EPOCH.with(|epoch| epoch.get());
    let point = (x.saturating_add(origin_x), y.saturating_add(origin_y));
    shot_trace!(
        "probe request hwnd={hwnd} point=({},{}) frame=({x},{y}) generation={generation}",
        point.0,
        point.1
    );
    UIA_PROBE.with(|slot| {
        if let Some(probe) = slot.borrow().as_ref() {
            probe.request(hwnd, point.0, point.1, epoch, generation);
        }
    });
}

/// Deliver a refinement computed off the UI thread. The selector re-checks the
/// generation and keeps the answer inside the window already highlighted.
#[cfg(windows)]
fn apply_element_answer(answer: crate::uia_probe::Answer) {
    if answer.epoch != PROBE_EPOCH.with(|epoch| epoch.get()) {
        return;
    }
    let (origin_x, origin_y) = PROBE_ORIGIN.with(|origin| origin.get());
    let (left, top) = (answer.rect.left - origin_x, answer.rect.top - origin_y);
    let width = answer.rect.right - answer.rect.left;
    let height = answer.rect.bottom - answer.rect.top;
    if width < 8 || height < 8 {
        shot_trace!("probe answer too small {}x{}", width, height);
        return;
    }
    shot_trace!(
        "probe answer generation={} rect={},{},{}x{}",
        answer.generation, left, top, width, height
    );
    let weak = OVERLAY.with(|slot| slot.borrow().as_ref().map(|overlay| overlay.as_weak()));
    let Some(overlay) = weak.and_then(|weak| weak.upgrade()) else {
        return;
    };
    if !overlay.window().is_visible() {
        return;
    }
    overlay.invoke_apply_probe(answer.generation, left, top, width, height);
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

fn save_attachment(
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<image_attachment::AttachmentDraft, String> {
    image_attachment::save(width, height, rgba, "screenshot")
}

/// A capture whose pixels exist but could not become a composer attachment: the
/// picture may still be on the clipboard, so the failure must not be reported as
/// if nothing had happened.
fn report_unsaved(
    app: &slint::Weak<MainWindow>,
    kind: &str,
    error: String,
    clipboard: Result<(), String>,
) {
    match clipboard {
        Ok(()) => report(
            app,
            format!("{}已复制到剪贴板，但无法放入输入区：{}", kind, error),
        ),
        Err(copy_error) => report(
            app,
            format!(
                "{}失败：{}；复制到剪贴板也失败：{}",
                kind, error, copy_error
            ),
        ),
    }
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
    fn remap_flags_map_the_hidden_window_again() {
        // Regression: the hide bypasses Winit's state queue, so a restore that
        // only asks Slint to show the window is a no-op and leaves the window
        // unmapped for the rest of the session. The mapping must be part of
        // the placement call itself.
        assert_ne!(
            REMAP_FLAGS & windows_sys::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW,
            0,
            "restoring a hidden window has to map it again"
        );
    }

    #[cfg(windows)]
    #[test]
    fn hide_check_only_treats_similar_pictures_as_the_window() {
        let reference = vec![10u8, 20, 30, 40, 50, 60];
        // Identical grid: the window is still in the frame.
        assert!(signatures_match(&reference, &reference));
        assert_eq!(signature_difference(&reference, &reference), 0.0);
        // Read-back noise from a different downscale path stays "still there",
        // which only costs a few extra reads before the frame is accepted.
        let noisy = vec![16u8, 26, 34, 44, 56, 64];
        assert!(signatures_match(&noisy, &reference));
        // The desktop behind the window reads as a different picture.
        let desktop = vec![120u8, 140, 160, 180, 200, 210];
        assert!(!signatures_match(&desktop, &reference));
        // Mismatched or empty grids must never stall the capture.
        assert!(!signatures_match(&reference[..3], &reference));
        assert!(!signatures_match(&[], &reference));
        assert!(!signatures_match(&reference, &[]));
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

    fn node(left: i32, top: i32, right: i32, bottom: i32, children: Vec<screen_capture::RegionNode>) -> screen_capture::RegionNode {
        screen_capture::RegionNode {
            rect: screen_capture::MonitorRect {
                left,
                top,
                right,
                bottom,
            },
            children,
            hwnd: 0,
        }
    }

    #[test]
    fn click_selection_descends_into_the_front_child() {
        let regions = vec![
            // Front window with a panel and a nested control inside it.
            node(
                100,
                100,
                300,
                300,
                vec![
                    node(110, 110, 200, 280, vec![node(120, 120, 160, 160, vec![])]),
                    node(210, 210, 290, 290, vec![]),
                ],
            ),
            // Desktop-sized window behind it.
            node(0, 0, 400, 400, vec![]),
        ];
        let hit = |x, y| hit_region(&regions, x, y).map(|rect| {
            (rect.left, rect.top, rect.right, rect.bottom)
        });
        // Window chrome stays the window.
        assert_eq!(hit(105, 105), Some((100, 100, 300, 300)));
        // A nested control wins over its panel and the window.
        assert_eq!(hit(130, 130), Some((120, 120, 160, 160)));
        assert_eq!(hit(150, 200), Some((110, 110, 200, 280)));
        assert_eq!(hit(215, 215), Some((210, 210, 290, 290)));
        // Outside the front window falls through to the one behind it.
        assert_eq!(hit(20, 20), Some((0, 0, 400, 400)));
        // Rectangle edges are exclusive on the right/bottom side, so adjacent
        // windows never both claim the same pixel.
        assert_eq!(hit(399, 399), Some((0, 0, 400, 400)));
        assert_eq!(hit(400, 400), None);
        assert_eq!(hit(-1, 10), None);
        assert_eq!(hit_region(&[], 10, 10), None);
    }
}
