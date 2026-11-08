//! Desktop layout preview for the phone shell.
//!
//! Runs the exact `ui/mobile.slint` component with representative data, but
//! without the engine, so the transcript, composer and drawer can be reviewed
//! on a workstation (`cargo run -p wunder-android --bin wunder-android-preview`).

use slint::ComponentHandle;

fn main() {
    let ui = wunder_android::build_preview();
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
    let page = std::env::args().nth(1);
    if let Some(page) = page.as_deref() {
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
    // frame should read at the target aspect ratio.
    ui.window()
        .set_size(slint::PhysicalSize::new(412, 915));
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
