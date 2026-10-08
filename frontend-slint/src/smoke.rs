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
            Ok(()) => "PASS: native rendering, send, blank/oversized input, draft isolation, task switching, bounded history, new task, fixed light theme, agent creation, model editing/default, entity pages.\n".to_string(),
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
    // Dispatch pointer events through Slint hit testing. Invoking callbacks
    // directly cannot detect focus-only first clicks or decorative overlays.
    // The two-column shell replaced the icon rail, so the round trip that used
    // to prove this is the sidebar's settings button plus the sheet's own back
    // action. Both coordinates come from the layout, not from repeated pixels.
    require(
        app.get_page() == crate::DesktopPage::Messages,
        "shell did not start on the chat page",
    )?;
    click(app, app.get_settings_button_x(), app.get_settings_button_y())?;
    require(
        app.get_settings_open(),
        "settings window did not open on the sidebar click",
    )?;
    click(app, app.get_settings_back_x(), app.get_settings_back_y())?;
    require(
        !app.get_settings_open(),
        "closing the settings window required another click",
    )?;
    let count = app.invoke_timeline_counts().rows;
    app.set_draft("  \n ".into());
    app.invoke_send_message();
    require(app.invoke_timeline_counts().rows == count, "blank input appended")?;
    app.set_draft("a".repeat(16_385).into());
    app.invoke_send_message();
    require(
        app.invoke_timeline_counts().rows == count,
        "oversized input appended",
    )?;
    app.set_draft("测试消息".into());
    app.invoke_send_message();
    require(
        app.invoke_timeline_counts().rows > count && app.get_draft().is_empty(),
        "send failed",
    )?;
    require(
        app.invoke_timeline_last_answer().contains("测试消息"),
        "sent text changed",
    )?;
    app.set_draft("未发送内容".into());
    app.invoke_select_conversation(1);
    require(
        app.invoke_timeline_counts().rows == 0 && app.get_draft().is_empty(),
        "conversation isolation failed",
    )?;
    app.invoke_select_conversation(0);
    require(
        app.get_draft() == "未发送内容" && app.invoke_timeline_counts().rows > count,
        "conversation restore failed",
    )?;
    for _ in 0..55 {
        app.set_draft("测试".into());
        app.invoke_send_message();
    }
    require(app.invoke_timeline_counts().rows <= 100, "history exceeded limit")?;
    app.invoke_new_thread();
    require(
        app.invoke_timeline_counts().rows == 0 && app.get_draft().is_empty(),
        "new task failed",
    )?;
    // The prototype follows the original light visual and no longer exposes
    // a theme switch. Keep a light snapshot for regression.
    snapshot(app, &directory.join("native-light.png"))?;
    // The settings sheet owns the agent and tool categories now; opening the
    // category is what makes these snapshots show their content.
    app.set_settings_open(true);
    app.set_settings_active_panel(3);
    // Single-agent: the fixed default agent must exist; no creation flow.
    require(
        app.get_agents().row_count() >= 1 && app.get_selected_agent_name() != "",
        "default agent missing",
    )?;
    snapshot(app, &directory.join("native-agents.png"))?;
    app.set_settings_active_panel(2);
    snapshot(app, &directory.join("native-tools.png"))?;
    // The workspace stays in the chat dock; the standalone files rail entry
    // was removed to match the web messenger information architecture.
    snapshot(app, &directory.join("native-workspace.png"))?;
    app.set_settings_open(true);
    app.set_model_key_draft("测试模型".into());
    app.set_model_provider_draft("openai".into());
    app.set_model_name_draft("test-model".into());
    app.set_model_base_url_draft("".into());
    app.set_model_token_draft("".into());
    app.set_model_type_draft("llm".into());
    app.invoke_save_model();
    app.invoke_set_default_model("测试模型".into());
    require(
        app.get_selected_model_key() == "测试模型" && app.get_selected_model_is_default(),
        "model/default update failed",
    )?;
    snapshot(app, &directory.join("native-settings.png"))?;
    // The standalone profile page is gone; the sheet's general category is
    // where its settings live.
    app.set_settings_active_panel(0);
    snapshot(app, &directory.join("native-settings-general.png"))?;
    app.set_dialog_title("操作失败".into());
    app.set_dialog_text("请检查模型配置后重试。".into());
    app.set_dialog_open(true);
    snapshot(app, &directory.join("native-form-error.png"))?;
    measure_timeline(app, directory)?;
    Ok(())
}
/// §十二.2 evidence on the real renderer: the same software rasterizer and the
/// same embedded font the desktop build uses. Reports how many rows the
/// projection can produce, what the first frame costs after the model is
/// published, and what a steady-state frame costs. It returns before anything
/// else can change the window, so `--timeline-probe` can print the numbers.
pub fn measure_timeline(
    app: &MainWindow,
    directory: &std::path::Path,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let mut report = Vec::new();
    for (turns, unfold) in [(50usize, false), (50, true)] {
        let rows = crate::demo::near_limit_rows(turns, 24, unfold);
        let published = rows.len();
        let mut timeline = crate::timeline::Timeline::new();
        timeline.set_history(rows);
        // Reach the opened state the way a user reaches it: open one turn at a
        // time, measuring every click. A fixture that just marks everything
        // visible would measure a state the bounded reducer can no longer produce.
        let mut opened_ms = 0.0f64;
        if unfold {
            let handles: Vec<i32> = timeline
                .model()
                .iter()
                .filter(|row| row.kind == crate::timeline::KIND_DIVIDER && row.visible)
                .map(|row| row.payload)
                .collect();
            for handle in &handles {
                let started = std::time::Instant::now();
                timeline.toggle(*handle);
                opened_ms += started.elapsed().as_secs_f64() * 1000.0;
            }
        }
        // Measure where the app actually sits: following the newest output, at
        // the end of the column. Both states are measured identically, so the
        // difference between them is the cost the state adds. Anything the frame
        // below pays for is this projection being laid out.
        let started = std::time::Instant::now();
        app.set_follow_output(true);
        app.set_timeline(timeline.model());
        // `take_snapshot` renders the window, so it lays the new model out and
        // returns only once the frame is really done.
        app.window().take_snapshot()?;
        let first_frame = started.elapsed().as_secs_f64() * 1000.0;
        // Read the counts back through the live model, not through the callback:
        // the callback still sees the model the demo installed.
        let model = app.get_timeline();
        let settled = slint::Model::row_count(&model);
        let visible = model
            .iter()
            .filter(|row| slint::Model::row_count(&row.blocks) > 0 || row.visible)
            .count();
        // Slint lays out a row exactly when the projection marked it visible, so
        // this is the row count the frame above actually paid for.
        let laid_out = model.iter().filter(|row| row.visible).count();
        let open_turns = model
            .iter()
            .filter(|row| row.kind == crate::timeline::KIND_DIVIDER && row.open)
            .count();
        drop(model);
        let mut frames = Vec::new();
        for _ in 0..5 {
            let started = std::time::Instant::now();
            app.window().take_snapshot()?;
            frames.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        // The median of the repeated frames is the number to compare across
        // builds; the minimum only shows the best case.
        frames.sort_by(f64::total_cmp);
        report.push(serde_json::json!({
            "turns": turns,
            "entries_per_turn": 24,
            "unfolded": unfold,
            "published_rows": published,
            "model_rows": settled,
            "rows_with_content": visible,
            "laid_out_rows": laid_out,
            "open_turns": open_turns,
            "opening_every_turn_ms": opened_ms,
            "first_frame_ms": first_frame,
            "frame_ms_min": frames.first().copied().unwrap_or_default(),
            "frame_ms_median": frames[frames.len() / 2],
            "frame_ms_max": frames.last().copied().unwrap_or_default(),
            "viewport_px": [app.window().size().width, app.window().size().height],
        }));
    }
    std::fs::write(
        directory.join("timeline-metrics.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

/// `--timeline-probe <dir>`: the §十二.2 measurement on its own, so it does not
/// depend on the rest of the smoke check staying green while other pages move.
pub fn run_timeline_probe(
    app: &MainWindow,
    directory: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&directory)?;
    crate::demo::install(app);
    app.show()?;
    // A readable artifact for the worst-case row shapes. The 50-turn column
    // cannot be photographed usefully: the timeline follows the newest output,
    // and the end of a folded column is empty space by construction.
    {
        let mut probe = crate::timeline::Timeline::new();
        probe.set_history(crate::demo::near_limit_rows(1, 3, false));
        app.set_timeline(probe.model());
        let pixels = app.window().take_snapshot()?;
        let mut encoder = png::Encoder::new(
            std::fs::File::create(directory.join("timeline-rows.png"))?,
            pixels.width(),
            pixels.height(),
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()?
            .write_image_data(pixels.as_bytes())?;
    }
    let result = Rc::new(RefCell::new(None));
    let output = result.clone();
    let weak = app.as_weak();
    let directory_probe = directory.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(150), move || {
        let measured = weak
            .upgrade()
            .ok_or_else(|| "window closed before the timeline probe".to_string())
            .and_then(|app| {
                measure_timeline(&app, &directory_probe).map_err(|error| error.to_string())
            });
        match &measured {
            Ok(report) => {
                println!("PASS: timeline probe wrote {}", directory_probe.join("timeline-metrics.json").display());
                for entry in report {
                    println!("{entry}");
                }
            }
            Err(error) => println!("FAIL: timeline probe: {error}"),
        }
        *output.borrow_mut() = Some(measured.map(|_| ()));
        let _ = slint::quit_event_loop();
    });
    app.run()?;
    result
        .borrow_mut()
        .take()
        .ok_or("timeline probe did not run")??;
    Ok(())
}

pub(crate) fn click(app: &MainWindow, x: f32, y: f32) -> Result<(), Box<dyn std::error::Error>> {
    use slint::platform::{PointerEventButton, WindowEvent};
    let position = slint::LogicalPosition::new(x, y);
    app.window()
        .dispatch_event_with_result(WindowEvent::PointerMoved { position })?;
    app.window()
        .dispatch_event_with_result(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        })?;
    app.window()
        .dispatch_event_with_result(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        })?;
    Ok(())
}

fn require(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

pub(crate) fn snapshot(
    app: &MainWindow,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
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
