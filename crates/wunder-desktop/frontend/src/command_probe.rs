//! `--command-fold-probe <dir>`: render one expanded command entry on the real
//! renderer so the capped detail panel's scroll geometry can be inspected as a
//! picture instead of inferred from the bindings.
//!
//! Environment:
//! - `WUNDER_COMMAND_PROBE_SIZE=WxH` window size (default 720x520)
//! - `WUNDER_COMMAND_PROBE_LINES=N` output lines appended to the entry

use std::path::PathBuf;

use slint::ComponentHandle;

const COMMAND: &str = "cd /work/demo && cargo test --lib -- \
compact_command_result_for_model services::tools::command 2>&1 | tail -8";

/// The card projects a command call through the engine, not by handing the
/// frontend one display string: the section prints the command as its title,
/// the state as its meta chip and the streamed tail as its body. Building the
/// payload the same way keeps the probe on that path.
fn entry(lines: usize) -> wunder_desktop::NativeWorkflowEntry {
    let output = (0..lines)
        .map(|index| {
            format!(
                "test services::tools::command::case_{index} ... ok  第 {index} 行把面板撑长一点"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let payload = serde_json::json!({
        "args": { "command": COMMAND, "cwd": "/work/demo" },
        "result": { "data": {
            "command": COMMAND, "cwd": "/work/demo",
            "output": output, "status": "running"
        }}
    });
    let detail =
        wunder_server::tool_result_display::tool_result_display("execute_command", &payload, true);
    wunder_desktop::NativeWorkflowEntry::from_payload(
        "probe",
        "execute_command",
        detail,
        "running",
        &payload,
    )
}

pub fn run(app: &crate::MainWindow, directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&directory)?;
    crate::demo::install(app);
    if let Ok(requested) = std::env::var("WUNDER_COMMAND_PROBE_SIZE") {
        if let Some((width, height)) = requested.split_once('x') {
            if let (Ok(width), Ok(height)) = (width.parse::<f32>(), height.parse::<f32>()) {
                app.window()
                    .set_size(slint::LogicalSize::new(width, height));
            }
        }
    }
    crate::show_main_window(app)?;

    let lines = std::env::var("WUNDER_COMMAND_PROBE_LINES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);
    let process = 1;
    let mut rows = vec![crate::demo::fixture_row(
        crate::timeline::KIND_USER,
        0,
        "跑一下命令并看完整输出",
    )];
    let mut fold = crate::demo::fixture_row(
        crate::timeline::KIND_PROCESS,
        rows.len(),
        &crate::timeline_text::process_label(2),
    );
    fold.process_idx = process;
    fold.open = true;
    fold.process_open = true;
    fold.summary = "执行中 · 1 项".into();
    rows.push(fold);
    let base = rows.len();
    let mut bar = crate::demo::fixture_row(crate::timeline::KIND_GROUP, base, "执行工具 2 次");
    bar.payload = base as i32;
    bar.open = true;
    bar.group_open = true;
    bar.process_idx = process;
    bar.process_open = true;
    rows.push(bar);
    let index = rows.len();
    let entry = entry(lines);
    let mut command = crate::demo::fixture_row(crate::timeline::KIND_TOOL, index, "");
    command.tool_name = "执行".into();
    command.tool_icon = "terminal".into();
    command.summary = COMMAND.chars().take(48).collect::<String>().into();
    command.target = "/work/demo".into();
    command.detail = crate::timeline_text::copy_text(&entry.sections, entry.detail.as_str());
    command.sections = crate::timeline_text::sections(&entry.sections);
    command.command_meta = entry.command_meta.as_str().into();
    command.status_kind = crate::timeline::STATUS_RUNNING;
    command.tokens = "159k".into();
    command.model_round = 13;
    command.foldable = true;
    command.open = true;
    command.group_idx = base as i32;
    command.group_open = true;
    command.process_idx = process;
    command.process_open = true;
    command.payload = index as i32;
    rows.push(command);
    crate::timeline_text::process_models(&mut rows);

    let mut timeline = crate::timeline::Timeline::new();
    timeline.set_history(rows);
    app.set_timeline(timeline.model());
    app.set_follow_output(false);

    // Shoot from inside the event loop: the panel height comes from a
    // preferred-height chain that only settles after a layout pass, which is
    // exactly where the reported clipping appears.
    let directory_probe = directory.clone();
    let weak = app.as_weak();
    let result = std::rc::Rc::new(std::cell::RefCell::new(None));
    let output = result.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(300), move || {
        let frame = (|| -> Result<PathBuf, Box<dyn std::error::Error>> {
            let app = weak.upgrade().ok_or("window closed before the probe")?;
            let pixels = app.window().take_snapshot()?;
            let size = app.window().size();
            let name = [
                "command-fold-",
                &size.width.to_string(),
                "x",
                &size.height.to_string(),
                ".png",
            ]
            .concat();
            let path = directory_probe.join(name);
            let mut encoder = png::Encoder::new(
                std::fs::File::create(&path)?,
                pixels.width(),
                pixels.height(),
            );
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()?
                .write_image_data(pixels.as_bytes())?;
            Ok(path)
        })();
        *output.borrow_mut() = Some(frame);
        let _ = slint::quit_event_loop();
    });
    app.run()?;
    let path = result
        .borrow_mut()
        .take()
        .ok_or("command fold probe did not run")??;
    println!("PASS: command fold probe wrote {}", path.display());
    Ok(())
}
