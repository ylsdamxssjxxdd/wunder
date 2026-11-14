//! Desktop layout preview for the phone shell.
//!
//! Runs the exact `ui/mobile.slint` component with representative data, but
//! without the engine, so the transcript, composer and drawer can be reviewed
//! on a workstation (`cargo run -p wunder-android --bin wunder-android-preview`).

use slint::ComponentHandle;

fn main() {
    let ui = wunder_android::build_preview();
    // `WUNDER_PREVIEW_TOAST=<text>` paints the in-app toast so its look can be
    // reviewed here; on the phone the engine raises it and hides it after 2.6s.
    if let Ok(text) = std::env::var("WUNDER_PREVIEW_TOAST") {
        ui.set_toast(text.into());
    }
    // `WUNDER_PREVIEW_SNAPSHOT=<file.ppm>` renders one frame and quits; the
    // phone window cannot be grabbed by the desktop screenshot tools.
    if let Ok(path) = std::env::var("WUNDER_PREVIEW_SNAPSHOT") {
        let weak = ui.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(700),
            move || {
                if let Some(ui) = weak.upgrade() {
                    if let Ok(frame) = ui.window().take_snapshot() {
                        let _ = write_ppm(&path, frame.as_slice(), frame.width(), frame.height());
                    }
                }
                let _ = slint::quit_event_loop();
            },
        );
        std::mem::forget(timer);
    }
    // Optional first argument opens a page for layout review: `settings`,
    // `models`, `editor`, or the first-run setup page (`env-setup`,
    // `env-setup-error`, `env-setup-ready`). `settings-live` opens the sheet
    // after the event loop has started, matching the phone's dynamic path.
    // `env-setup-live` additionally loops the injection animation so the liquid
    // can be watched without the engine. `env-setup-gif` runs one 4% → 100%
    // pass; with `WUNDER_PREVIEW_FRAMES=<dir>` every frame is written as PPM so
    // the effect can be shared as a GIF on machines that hide new windows.
    let page = std::env::args().nth(1);
    if let Some(dir) = std::env::var_os("WUNDER_PREVIEW_FRAMES") {
        let dir = dir.to_string_lossy().into_owned();
        let weak = ui.as_weak();
        let mut index = 0usize;
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(100),
            move || {
                if let Some(ui) = weak.upgrade() {
                    if let Ok(frame) = ui.window().take_snapshot() {
                        let path = format!("{dir}/frame-{index:03}.ppm");
                        let _ = write_ppm(&path, frame.as_slice(), frame.width(), frame.height());
                    }
                }
                index += 1;
                if index >= 120 {
                    let _ = slint::quit_event_loop();
                }
            },
        );
        std::mem::forget(timer);
    }
    if page.as_deref() == Some("env-setup-finish") {
        // 安装中 → 完成的收尾：验证气泡潮；相位要持续推进，气泡才会流动。
        wunder_android::open_preview_page(&ui, "env-setup");
        let weak = ui.as_weak();
        let started = std::time::Instant::now();
        let phase_timer = slint::Timer::default();
        phase_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(33),
            move || {
                if let Some(ui) = weak.upgrade() {
                    let ms = started.elapsed().as_millis() as u64;
                    ui.set_env_setup_phase(ms as f32 / 4000.0);
                }
            },
        );
        std::mem::forget(phase_timer);
        let weak = ui.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(1400),
            move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_settings_ubuntu_state("installed".into());
                    ui.set_settings_ubuntu_version("Ubuntu 24.04.1 LTS".into());
                    ui.set_env_setup_progress(100);
                }
            },
        );
        std::mem::forget(timer);
    } else if page.as_deref() == Some("env-setup-live") || page.as_deref() == Some("env-setup-gif") {
        let one_pass = page.as_deref() == Some("env-setup-gif");
        wunder_android::open_preview_page(&ui, "env-setup");
        let weak = ui.as_weak();
        let started = std::time::Instant::now();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(33),
            move || {
                if let Some(ui) = weak.upgrade() {
                    let ms = started.elapsed().as_millis() as u64;
                    ui.set_env_setup_phase(ms as f32 / 4000.0);
                    let progress = if one_pass {
                        // 4% → 100% in 10.5s, matching the 120-frame capture.
                        4 + (ms as i32 * 96 / 10_500).min(96)
                    } else {
                        // 20s rise, then hold full for 2.5s so the liquid is
                        // seen reaching the top before the loop resets.
                        let phase = ms % 22_500;
                        if phase < 20_000 {
                            4 + phase as i32 / 200
                        } else {
                            100
                        }
                    };
                    ui.set_env_setup_progress(progress.min(100));
                }
            },
        );
        std::mem::forget(timer);
    } else if let Some(page) = page.as_deref() {
        if page == "settings-live" {
            let weak = ui.as_weak();
            let timer = slint::Timer::default();
            timer.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(800),
                move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_settings_page(0);
                        ui.set_settings_open(true);
                    }
                },
            );
            std::mem::forget(timer);
        } else {
            wunder_android::open_preview_page(&ui, page);
        }
    }
    // A portrait phone body; the window manager may resize it, but the first
    // frame should read at the target aspect ratio. `WUNDER_PREVIEW_SIZE=WxH`
    // renders one long frame instead, so a whole list or document can be
    // reviewed in a single snapshot without a gesture.
    let mut size = slint::PhysicalSize::new(412, 915);
    if let Ok(value) = std::env::var("WUNDER_PREVIEW_SIZE") {
        if let Some((width, height)) = value
            .split_once('x')
            .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
        {
            size = slint::PhysicalSize::new(width, height);
        }
    }
    ui.window().set_size(size);
    // `run()` does not map the window on the desktop backend; without an
    // explicit show the process renders offscreen and the screen stays empty.
    ui.show().expect("show the mobile preview");
    ui.run().expect("run the mobile preview");
}

fn write_ppm(
    path: &str,
    pixels: &[slint::Rgba8Pixel],
    width: u32,
    height: u32,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    write!(file, "P6\n{width} {height}\n255\n")?;
    let mut rgb = Vec::with_capacity(pixels.len() * 3);
    for pixel in pixels {
        rgb.extend_from_slice(&[pixel.r, pixel.g, pixel.b]);
    }
    file.write_all(&rgb)
}
