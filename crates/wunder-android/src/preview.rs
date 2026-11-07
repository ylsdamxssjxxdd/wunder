//! Desktop layout preview for the phone shell.
//!
//! Runs the exact `ui/mobile.slint` component with representative data, but
//! without the engine, so the transcript, composer and drawer can be reviewed
//! on a workstation (`cargo run -p wunder-android --bin wunder-android-preview`).

use slint::ComponentHandle;

fn main() {
    let ui = wunder_android::build_preview();
    // Optional first argument opens a settings page for layout review:
    // `settings`, `models` or `editor`. `settings-live` opens the sheet after
    // the event loop has started, matching the phone's dynamic path.
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
