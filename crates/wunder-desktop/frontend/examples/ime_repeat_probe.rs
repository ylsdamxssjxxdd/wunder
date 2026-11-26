//! Diagnostic probe: how many characters does the text editor keep when the
//! same key is injected rapidly (remote-control software sends X11 key bursts
//! without a per-key delay)?
//!
//! Run on a live X11 display, focus the window, then inject text, e.g.
//! `xdotool type --delay 0 aaaa`. The final string and its length are printed
//! to stdout and to the file named by `IME_PROBE_OUT` when the window closes.

use std::io::Write;

slint::slint! {
    export component Probe inherits Window {
        preferred-width: 640px;
        preferred-height: 220px;
        title: "ime repeat probe";
        in-out property <string> typed: "";
        in property <bool> input-focused: input.has-focus;
        VerticalLayout {
            padding: 12px;
            spacing: 8px;
            Text {
                text: "typed=[" + root.typed + "]";
                font-size: 18px;
            }
            input := TextInput {
                text <=> root.typed;
                single-line: false;
                min-height: 90px;
                font-size: 18px;
            }
        }
        init => { input.focus(); }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The shell normally registers its bundled faces; the probe borrows an
    // installed family so Slint can shape the draft.
    {
        use slint::fontique_012::fontique;
        let mut collection = slint::fontique_012::shared_collection();
        for path in [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        ] {
            let Ok(bytes) = std::fs::read(path) else { continue };
            let families =
                collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(bytes)), None);
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

    let app = Probe::new()?;
    app.show()?;
    println!("ready");
    let _ = std::io::stdout().flush();

    let weak = app.as_weak();
    let last = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let last_cb = last.clone();
    let poll = slint::Timer::default();
    poll.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(120),
        move || {
            if let Some(app) = weak.upgrade() {
                let text = app.get_typed().to_string();
                println!("focus={} text=[{}]", app.get_input_focused(), text);
                let _ = std::io::stdout().flush();
                let changed = *last_cb.borrow() != text;
                if changed {
                    *last_cb.borrow_mut() = text.clone();
                    let line = format!("typed: [{}] len={}\n", text, text.chars().count());
                    print!("{line}");
                    let _ = std::io::stdout().flush();
                    if let Ok(path) = std::env::var("IME_PROBE_OUT") {
                        if let Ok(mut file) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(path)
                        {
                            let _ = file.write_all(line.as_bytes());
                        }
                    }
                }
            }
        },
    );

    let quit = slint::Timer::default();
    let weak = app.as_weak();
    quit.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_secs(20),
        move || {
            if let Some(app) = weak.upgrade() {
                let text = app.get_typed().to_string();
                let line = format!("final: [{}] chars={}\n", text, text.chars().count());
                print!("{line}");
                if let Ok(path) = std::env::var("IME_PROBE_OUT") {
                    if let Ok(mut file) = std::fs::File::create(path) {
                        let _ = file.write_all(line.as_bytes());
                    }
                }
            }
            let _ = slint::quit_event_loop();
        },
    );

    app.run()?;
    Ok(())
}
