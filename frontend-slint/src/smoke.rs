//! Opt-in native smoke check: validate callbacks and render with embedded fonts.
use crate::MainWindow;
use slint::{ComponentHandle, Model};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

pub fn run(app: &MainWindow, directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&directory)?;
    let result = Rc::new(RefCell::new(None));
    let output = result.clone();
    let weak = app.as_weak();
    app.show()?;
    slint::Timer::single_shot(std::time::Duration::from_millis(150), move || {
        let checked = weak
            .upgrade()
            .ok_or_else(|| "window closed before smoke check".to_string())
            .and_then(|app| check(&app, &directory).map_err(|error| error.to_string()));
        let report = match &checked {
            Ok(()) => "PASS: native rendering, send, blank/oversized input, draft isolation, task switching, bounded history, new task, theme.\n".to_string(),
            Err(error) => format!("FAIL: {error}\n"),
        };
        let checked = std::fs::write(directory.join("smoke.txt"), report)
            .map_err(|error| error.to_string())
            .and(checked);
        *output.borrow_mut() = Some(checked);
        let _ = slint::quit_event_loop();
    });
    app.run()?;
    result
        .borrow_mut()
        .take()
        .ok_or("smoke check did not run")??;
    Ok(())
}

fn check(app: &MainWindow, directory: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    snapshot(app, &directory.join("native-preview.png"))?;
    let count = app.get_messages().row_count();
    app.set_draft("  \n ".into());
    app.invoke_send_message();
    require(
        app.get_messages().row_count() == count,
        "blank input appended",
    )?;
    app.set_draft("a".repeat(16_385).into());
    app.invoke_send_message();
    require(
        app.get_messages().row_count() == count,
        "oversized input appended",
    )?;
    app.set_draft("测试消息".into());
    app.invoke_send_message();
    require(
        app.get_messages().row_count() == count + 2 && app.get_draft().is_empty(),
        "send failed",
    )?;
    require(
        app.get_messages()
            .row_data(count)
            .is_some_and(|message| message.mine && message.text == "测试消息"),
        "sent text changed",
    )?;
    app.set_draft("未发送内容".into());
    app.invoke_select_conversation(1);
    require(
        app.get_messages().row_count() == 1 && app.get_draft().is_empty(),
        "conversation isolation failed",
    )?;
    app.invoke_select_conversation(0);
    require(
        app.get_draft() == "未发送内容" && app.get_messages().row_count() == count + 2,
        "conversation restore failed",
    )?;
    app.invoke_select_task(1);
    require(app.get_messages().row_count() == 1, "task isolation failed")?;
    app.invoke_select_task(0);
    for _ in 0..55 {
        app.set_draft("测试".into());
        app.invoke_send_message();
    }
    require(
        app.get_messages().row_count() <= 100,
        "history exceeded limit",
    )?;
    app.invoke_new_thread();
    require(
        app.get_messages().row_count() == 1 && app.get_draft().is_empty(),
        "new task failed",
    )?;
    app.set_dark(true);
    snapshot(app, &directory.join("native-dark.png"))?;
    Ok(())
}

fn require(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn snapshot(app: &MainWindow, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let pixels = app.window().take_snapshot()?;
    let mut encoder = png::Encoder::new(
        std::fs::File::create(path)?,
        pixels.width(),
        pixels.height(),
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()?
        .write_image_data(pixels.as_bytes())?;
    Ok(())
}
