//! Reproduction probe: continuous UI updates + a native context menu.
//!
//! Run with `RCHO_SLINT_DIAGNOSTICS=1` to collect one frame sample per draw.
//! The window prints a ticking counter; an external driver screenshots it and
//! decides whether the frame is still being presented after the menu closes.

use std::io::Write;
use std::sync::{Arc, Mutex};

slint::slint! {
    export component Probe inherits Window {
        in-out property <int> tick: 0;
        in-out property <string> log: "ready";
        in-out property <bool> panel-open: false;
        callback action(string);

        VerticalLayout {
            Text {
                text: "TICK " + root.tick;
                font-size: 44px;
                height: 72px;
            }
            Rectangle {
                height: 140px;
                background: #cfd8e3;
                ContextMenuArea {
                    Menu {
                        MenuItem { title: "noop"; activated => { root.action("noop"); } }
                        MenuItem { title: "text"; activated => { root.action("text"); } }
                        MenuItem { title: "popup"; activated => { root.action("popup"); } }
                    }
                    Rectangle {
                        width: 100%;
                        height: 100%;
                        background: transparent;
                        Text {
                            text: "RIGHT CLICK HERE";
                            font-size: 20px;
                            vertical-alignment: center;
                            horizontal-alignment: center;
                        }
                    }
                }
            }
            if root.panel-open : Rectangle {
                height: 120px;
                background: #fff3c4;
                Text { text: "EMBEDDED POPUP"; font-size: 24px; }
            }
            Text { text: root.log; font-size: 18px; }
        }
    }
}

type Log = Arc<Mutex<std::io::BufWriter<std::fs::File>>>;

fn line(log: &Log, text: String) {
    if let Ok(mut file) = log.lock() {
        let _ = writeln!(file, "{:?} {text}", std::time::SystemTime::now());
        let _ = file.flush();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The shell normally registers its bundled faces; the probe borrows any
    // installed face so the software text stack has a family to resolve.
    {
        use slint::fontique_012::fontique;
        let mut collection = slint::fontique_012::shared_collection();
        for path in ["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/arial.ttf"] {
            if let Ok(bytes) = std::fs::read(path) {
                let families = collection
                    .register_fonts(fontique::Blob::new(std::sync::Arc::new(bytes)), None);
                let chain: Vec<_> = families.iter().map(|(family, _)| *family).collect();
                if chain.is_empty() {
                    continue;
                }
                for generic in [
                    fontique::GenericFamily::SansSerif,
                    fontique::GenericFamily::SystemUi,
                    fontique::GenericFamily::UiSansSerif,
                ] {
                    collection.set_generic_families(generic, chain.iter().copied());
                }
                break;
            }
        }
    }

    let app = Probe::new()?;
    // `PROBE_INWINDOW_MENU=1` switches context menus from the Win32 native popup
    // to Slint's own popup, the configuration the shell now uses.
    if std::env::var("PROBE_INWINDOW_MENU").is_ok_and(|value| value == "1") {
        app.window().set_native_popup_menus_enabled(false);
    }

    let path = std::env::var("PROBE_LOG").unwrap_or_else(|_| "menu-freeze-probe.log".into());
    let log: Log = Arc::new(Mutex::new(std::io::BufWriter::new(std::fs::File::create(path)?)));
    let started = std::time::Instant::now();

    // Stream pump stand-in: the real desktop reprojects the transcript from a
    // 33 ms Slint timer while a turn runs.
    let counter = Arc::new(Mutex::new(0i32));
    let weak = app.as_weak();
    let timer = slint::Timer::default();
    let tick_counter = counter.clone();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(33), move || {
        let Some(app) = weak.upgrade() else { return };
        let mut value = tick_counter.lock().unwrap();
        *value += 1;
        app.set_tick(*value);
    });

    // Passive-update stand-in: the observer thread posts changes through the
    // event loop, exactly where a blocked UI would back up.
    {
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let mut seq = 0u64;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
                seq += 1;
                let value = seq;
                let _ = weak.clone().upgrade_in_event_loop(move |app| {
                    app.set_log(format!("worker {value}").into());
                });
            }
        });
    }

    // Liveness: a closure posted from a worker only runs when the UI thread is
    // pumping the event loop.
    {
        let weak = app.as_weak();
        let log = log.clone();
        std::thread::spawn(move || {
            let mut seq = 0u64;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                seq += 1;
                let value = seq;
                let log = log.clone();
                let _ = weak.clone().upgrade_in_event_loop(move |app| {
                    line(&log, format!("alive {value} tick={}", app.get_tick()));
                });
            }
        });
    }

    // Diagnostics drain: one line per second with the frames actually presented
    // and the windowing events that scheduled them.
    {
        let log = log.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let (frames, dropped) = i_slint_backend_winit::software_diagnostics::take_samples();
            let (events, _) = i_slint_backend_winit::software_diagnostics::take_events();
            if frames.is_empty() && events.is_empty() {
                continue;
            }
            let mut text = String::from("frames ");
            for frame in &frames {
                text.push_str(&format!(
                    "[{:.0}ms dmg={} render={:.1} present={:.1}] ",
                    frame.at_ms, frame.damage_pixels, frame.render_ms, frame.present_ms
                ));
            }
            text.push_str(&format!("dropped={dropped} events "));
            for event in &events {
                text.push_str(&format!("[{:.0}ms {}]", event.at_ms, event.kind));
            }
            line(&log, text);
        });
    }

    let action_log = log.clone();
    app.on_action(move |kind| {
        line(&action_log, format!("action {kind}"));
    });

    let shown = log.clone();
    line(&shown, format!("shown {started:?}"));
    app.run()?;
    Ok(())
}

