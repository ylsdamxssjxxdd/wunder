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
    let editor_x = app.get_composer_editor_x();
    let editor_y = app.get_composer_editor_y();
    let editor_width = app.get_composer_editor_width();
    let editor_height = app.get_composer_editor_height();
    require(
        editor_width > 200.0 && editor_height >= 40.0,
        "composer editor hit area collapsed",
    )?;
    click(
        app,
        editor_x + editor_width * 0.85,
        editor_y + editor_height * 0.5,
    )?;
    require(
        app.get_composer_editor_has_focus(),
        "clicking the blank right side of the composer did not focus the editor",
    )?;
    drag(app, 100.0, 156.0, 800.0, 400.0)?;
    require(
        app.get_split_count() == 2 && app.get_split_session_1() == "t2",
        "thread drag did not open the correct second pane",
    )?;
    snapshot(app, &directory.join("native-split-two.png"))?;
    click(app, app.get_split_close_x(), app.get_split_close_y())?;
    require(
        app.get_split_count() == 1,
        "closing the last other pane did not restore single view",
    )?;
    snapshot(app, &directory.join("native-split-single.png"))?;
    app.invoke_open_thread_in_split(2);
    require(
        app.get_split_count() == 2 && app.get_split_session_1() == "t3",
        "thread context action did not open the selected thread",
    )?;
    app.invoke_split_drop_row(2, 1);
    require(
        app.get_split_count() == 4,
        "third thread did not open the four-grid layout",
    )?;
    snapshot(app, &directory.join("native-split-four.png"))?;
    app.invoke_split_close(2);
    require(
        app.get_split_count() == 2,
        "closing a grid pane did not return to two columns",
    )?;
    app.invoke_split_drop_row(2, 1);
    require(
        app.get_split_count() == 4,
        "re-adding a thread did not restore the grid",
    )?;
    app.invoke_split_close(1);
    require(
        app.get_split_count() == 2
            && app.get_split_session_0() == "t1"
            && app.get_split_session_1() == "t2"
            && app.get_split_session_2().is_empty(),
        "closing a middle pane did not compact the remaining threads",
    )?;
    app.invoke_split_drop_row(2, 2);
    require(
        app.get_split_count() == 4 && app.get_split_session_2() == "t3",
        "dragging after compaction used the wrong pane",
    )?;
    for _ in 0..64 {
        app.invoke_split_layout(2);
        app.invoke_split_layout(4);
    }
    require(
        app.get_split_count() == 4,
        "repeated split layout switching changed state",
    )?;
    app.invoke_split_layout(1);
    click(
        app,
        app.get_settings_button_x(),
        app.get_settings_button_y(),
    )?;
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
    require(
        app.invoke_timeline_counts().rows == count,
        "blank input appended",
    )?;
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
    require(
        app.invoke_timeline_counts().rows <= 100,
        "history exceeded limit",
    )?;
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
        // Reach the opened state the way a user reaches it: open one batch at a
        // time, measuring every click. A fixture that just marks everything
        // open would measure a state the default projection does not produce.
        let mut opened_ms = 0.0f64;
        if unfold {
            let handles: Vec<i32> = timeline
                .model()
                .iter()
                .filter(|row| row.kind == crate::timeline::KIND_GROUP && !row.group_open)
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
        // Slint lays out a row exactly when the projection marked it visible and
        // its batch is open, so this is the row count the frame above paid for.
        let laid_out = model
            .iter()
            .filter(|row| {
                row.visible
                    && (row.kind == crate::timeline::KIND_GROUP
                        || row.group_idx == crate::timeline::NO_GROUP
                        || row.group_open)
            })
            .count();
        let open_turns = model
            .iter()
            .filter(|row| row.kind == crate::timeline::KIND_GROUP && row.open)
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
                println!(
                    "PASS: timeline probe wrote {}",
                    directory_probe.join("timeline-metrics.json").display()
                );
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

/// `--scroll-probe <dir>`: measure the frame cost of a real wheel scroll on the
/// live software renderer. The diagnostics counts only frames that were
/// actually drawn and presented, so fling deceleration pauses do not pollute it.
pub fn run_scroll_probe(
    app: &MainWindow,
    directory: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&directory)?;
    std::env::set_var("RCHO_SLINT_DIAGNOSTICS", "1");
    crate::demo::install(app);
    // Optional fixed viewport so the same gesture can be compared across
    // window sizes, e.g. WUNDER_SCROLL_PROBE_SIZE=1600x1000.
    if let Ok(requested) = std::env::var("WUNDER_SCROLL_PROBE_SIZE") {
        if let Some((width, height)) = requested.split_once('x') {
            if let (Ok(width), Ok(height)) = (width.parse::<f32>(), height.parse::<f32>()) {
                app.window()
                    .set_size(slint::LogicalSize::new(width, height));
            }
        }
    }
    app.show()?;
    let mut timeline = crate::timeline::Timeline::new();
    timeline.set_history(crate::demo::near_limit_rows(50, 24, false));
    app.set_follow_output(false);
    app.set_timeline(timeline.model());
    // Settle layout and glyph caches before the measured frames.
    app.window().take_snapshot()?;

    let timer = Rc::new(slint::Timer::default());
    let timer_weak = Rc::downgrade(&timer);
    let weak = app.as_weak();
    let ticks = Rc::new(RefCell::new(0u32));
    let ticks_cb = ticks.clone();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(16),
        move || {
            let Some(timer) = timer_weak.upgrade() else {
                return;
            };
            let Some(app) = weak.upgrade() else {
                let _ = slint::quit_event_loop();
                return;
            };
            *ticks_cb.borrow_mut() += 1;
            use slint::platform::WindowEvent;
            let _ = app
                .window()
                .dispatch_event_with_result(WindowEvent::PointerScrolled {
                    position: slint::LogicalPosition::new(620.0, 360.0),
                    delta_x: 0.0,
                    // Wheel down scrolls the content towards its end.
                    delta_y: -120.0,
                });
            if *ticks_cb.borrow() >= 190 {
                timer.stop();
                let _ = slint::quit_event_loop();
            }
        },
    );
    app.run()?;

    let (frames, dropped) = i_slint_backend_winit::software_diagnostics::take_samples();
    let mut render: Vec<f64> = frames.iter().map(|f| f.render_ms).collect();
    let mut present: Vec<f64> = frames.iter().map(|f| f.present_ms).collect();
    let mut damage: Vec<u64> = frames.iter().map(|f| f.damage_pixels).collect();
    render.sort_by(f64::total_cmp);
    present.sort_by(f64::total_cmp);
    damage.sort_by(|a, b| a.cmp(b));
    let percentile = |values: &[f64], p: f64| {
        values
            .get(((values.len() as f64 * p) as usize).min(values.len().saturating_sub(1)))
            .copied()
            .unwrap_or_default()
    };
    let report = serde_json::json!({
        "frames": frames.len(),
        "dropped_samples": dropped,
        "window_px": [frames.last().map(|f| f.width).unwrap_or_default(), frames.last().map(|f| f.height).unwrap_or_default()],
        "scale": frames.last().map(|f| f.scale).unwrap_or_default(),
        "render_ms_median": percentile(&render, 0.5),
        "render_ms_p95": percentile(&render, 0.95),
        "render_ms_max": render.last().copied().unwrap_or_default(),
        "present_ms_median": percentile(&present, 0.5),
        "present_ms_p95": percentile(&present, 0.95),
        "damage_px_median": damage.get(damage.len() / 2).copied().unwrap_or_default(),
        "damage_px_max": damage.last().copied().unwrap_or_default(),
    });
    std::fs::write(
        directory.join("scroll-metrics.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "PASS: scroll probe wrote {}",
        directory.join("scroll-metrics.json").display()
    );
    println!("{report}");
    Ok(())
}

/// Scroll a long, non-timeline ListView (the thread sidebar) and report the
/// same frame samples as the transcript probe. ListView creates its rows on
/// demand, so this is where per-row instantiation and text shaping show up as
/// uneven frames rather than as a large median.
pub fn run_list_probe(
    app: &MainWindow,
    directory: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    use slint::{ModelRc, VecModel};
    std::fs::create_dir_all(&directory)?;
    std::env::set_var("RCHO_SLINT_DIAGNOSTICS", "1");
    crate::demo::install(app);
    if let Ok(requested) = std::env::var("WUNDER_SCROLL_PROBE_SIZE") {
        if let Some((width, height)) = requested.split_once('x') {
            if let (Ok(width), Ok(height)) = (width.parse::<f32>(), height.parse::<f32>()) {
                app.window()
                    .set_size(slint::LogicalSize::new(width, height));
            }
        }
    }
    let mut rows = Vec::new();
    rows.push(crate::SidebarRow {
        kind: 0,
        workspace_id: "probe-workspace".into(),
        name: "探针工作区".into(),
        icon: "folder".into(),
        color: "blue".into(),
        expanded: true,
        selected: true,
        ..Default::default()
    });
    let spinner = std::env::var("WUNDER_LIST_PROBE_SPINNER")
        .ok()
        .is_some_and(|value| value == "1");
    for index in 0..320 {
        rows.push(crate::SidebarRow {
            kind: 1,
            workspace_id: "probe-workspace".into(),
            thread_id: format!("probe-{index}").into(),
            title: format!("滚动探针线程 {index}").into(),
            // A running row draws a spinner driven by `animation-tick()`, which
            // keeps the frame loop busy without any input device. Some test
            // environments cannot inject wheel or button events at all.
            status: (if spinner && index == 0 {
                "running"
            } else {
                "done"
            })
            .into(),
            // Phase 2 is `running`; a running row paints the animated spinner.
            phase: if spinner && index == 0 { 2 } else { 4 },
            ..Default::default()
        });
    }
    app.set_sidebar_rows(ModelRc::new(VecModel::from(rows)));
    app.show()?;

    let external = std::env::var("WUNDER_LIST_PROBE_EXTERNAL")
        .ok()
        .is_some_and(|value| value == "1");
    if external {
        // Drive the real winit wheel path from outside (xdotool), because
        // `WindowEvent::PointerScrolled` is delivered as TouchPhase::Cancelled
        // and bypasses the wheel animation entirely. Quitting comes from a
        // background thread: a Slint timer armed before the event loop starts
        // does not fire reliably in this probe.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(7));
            let _ = slint::invoke_from_event_loop(|| {
                let _ = slint::quit_event_loop();
            });
        });
    } else {
        // Feed the wheel through the backend's own event shape, from a plain
        // thread. `WindowEvent::PointerScrolled` is converted to
        // `TouchPhase::Cancelled`, which jumps the Flickable straight to the new
        // position; the real winit path sends `BackendMouseEvent::Wheel` with
        // `TouchPhase::Moved` and that is the only one that runs the scroll
        // animation. A Slint timer armed before `app.run()` does not fire here,
        // so the injector owns its own clock.
        let weak = app.as_weak();
        // A real mouse sends roughly one notch every 60-100 ms, not every frame;
        // firing at the frame period would interleave input redraws with throttled
        // frames and make the measured cadence an artifact of the probe itself.
        let tick_ms = std::env::var("WUNDER_LIST_PROBE_TICK_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(80);
        let total_ticks = std::env::var("WUNDER_LIST_PROBE_TICKS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(60);
        let wheel_delta = std::env::var("WUNDER_LIST_PROBE_DELTA")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(-60.0);
        let idle_ms = std::env::var("WUNDER_LIST_PROBE_IDLE_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1500);
        std::thread::spawn(move || {
            use i_slint_core::input::{BackendMouseEvent, TouchPhase};
            use i_slint_core::lengths::LogicalPoint;
            use std::time::{Duration, Instant};
            let mut next = Instant::now();
            for _ in 0..total_ticks {
                next += Duration::from_millis(tick_ms);
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(app) = weak.upgrade() else { return };
                    let position = LogicalPoint::new(110.0, 300.0);
                    let _ = app.window().dispatch_event_with_result(
                        slint::platform::WindowEvent::internal(BackendMouseEvent::Wheel {
                            position,
                            delta_x: 0.0,
                            delta_y: wheel_delta,
                            phase: TouchPhase::Moved,
                        }),
                    );
                });
            }
            // Let the last notch finish its animation before the loop quits, so
            // the trailing glide is part of the sample.
            std::thread::sleep(Duration::from_millis(idle_ms));
            let _ = slint::invoke_from_event_loop(|| {
                let _ = slint::quit_event_loop();
            });
        });
    }
    app.run()?;

    let (frames, dropped) = i_slint_backend_winit::software_diagnostics::take_samples();
    let mut render: Vec<f64> = frames.iter().map(|f| f.render_ms).collect();
    render.sort_by(f64::total_cmp);
    let percentile = |values: &[f64], p: f64| {
        values
            .get(((values.len() as f64 * p) as usize).min(values.len().saturating_sub(1)))
            .copied()
            .unwrap_or_default()
    };
    let mut intervals: Vec<f64> = frames
        .windows(2)
        .map(|pair| pair[1].at_ms - pair[0].at_ms)
        .collect();
    intervals.sort_by(f64::total_cmp);
    let slow_frames = frames.iter().filter(|f| f.render_ms >= 16.7).count();
    let raw: Vec<_> = frames
        .iter()
        .map(|f| {
            serde_json::json!({
                "at_ms": f.at_ms,
                "render_ms": f.render_ms,
                "present_ms": f.present_ms,
                "damage_px": f.damage_pixels,
            })
        })
        .collect();
    let report = serde_json::json!({
        "frames": frames.len(),
        "dropped_samples": dropped,
        "slow_frames_over_16ms": slow_frames,
        "render_ms_median": percentile(&render, 0.5),
        "render_ms_p95": percentile(&render, 0.95),
        "render_ms_max": render.last().copied().unwrap_or_default(),
        "frame_interval_ms_median": percentile(&intervals, 0.5),
        "frame_interval_ms_p95": percentile(&intervals, 0.95),
        "frame_interval_ms_max": intervals.last().copied().unwrap_or_default(),
        "raw": raw,
    });
    std::fs::write(
        directory.join("list-metrics.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "PASS: list probe wrote {}",
        directory.join("list-metrics.json").display()
    );
    println!(
        "frames={} slow={} render median={:.2} p95={:.2} max={:.2} interval median={:.2} p95={:.2}",
        frames.len(),
        slow_frames,
        percentile(&render, 0.5),
        percentile(&render, 0.95),
        render.last().copied().unwrap_or_default(),
        percentile(&intervals, 0.5),
        percentile(&intervals, 0.95),
    );
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

pub(crate) fn drag(
    app: &MainWindow,
    from_x: f32,
    from_y: f32,
    to_x: f32,
    to_y: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    use slint::platform::{PointerEventButton, WindowEvent};
    let from = slint::LogicalPosition::new(from_x, from_y);
    app.window()
        .dispatch_event_with_result(WindowEvent::PointerMoved { position: from })?;
    app.window()
        .dispatch_event_with_result(WindowEvent::PointerPressed {
            position: from,
            button: PointerEventButton::Left,
        })?;
    for step in 1..=8 {
        let position = slint::LogicalPosition::new(
            from_x + (to_x - from_x) * step as f32 / 8.0,
            from_y + (to_y - from_y) * step as f32 / 8.0,
        );
        app.window()
            .dispatch_event_with_result(WindowEvent::PointerMoved { position })?;
    }
    app.window()
        .dispatch_event_with_result(WindowEvent::PointerReleased {
            position: slint::LogicalPosition::new(to_x, to_y),
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
