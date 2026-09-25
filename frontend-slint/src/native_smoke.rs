//! Opt-in integration driver; the caller must provide an isolated runtime root.
use crate::MainWindow;
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
use wunder_desktop::{NativeChatEvent, NativeChatInput, NativeDesktop};

pub const DELTA: &str = "增量测试内容。增量测试内容。增量测试内容。增量测试内容。\n";

pub fn check_runtime(desktop: &NativeDesktop) -> Result<(), Box<dyn std::error::Error>> {
    ensure(
        desktop.get_session("missing-session").is_err(),
        "missing session accepted",
    )?;
    ensure(
        desktop.cancel_chat("missing-session").is_err(),
        "unowned cancellation accepted",
    )?;
    ensure(
        desktop
            .create_session_for_agent(Some("missing-agent"))
            .is_err(),
        "missing agent accepted",
    )?;
    let session = desktop.create_session_for_agent(None)?;
    ensure(
        desktop
            .list_sessions()?
            .iter()
            .any(|item| item.id == session.id),
        "created session not listed",
    )?;
    ensure(
        desktop.get_session(&session.id)?.1.is_empty(),
        "new history is not empty",
    )?;
    let mut stream = desktop.send_chat(NativeChatInput {
        session_id: session.id,
        content: "native-cancel".into(),
        client_message_id: Some("native-cancel-before-start".into()),
        attachments: Vec::new(),
    })?;
    stream.cancel();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stopped = false;
    loop {
        ensure(Instant::now() < deadline, "early cancellation timeout")?;
        match stream.try_recv()? {
            Some(NativeChatEvent::Event(event)) => {
                stopped |= event["data"]["status"] == "cancelled"
            }
            Some(NativeChatEvent::Finished) => break,
            Some(NativeChatEvent::Failed(error)) => return Err(error.into()),
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    ensure(stopped, "early cancellation did not settle")?;
    check_queue_and_detach(desktop)?;
    Ok(())
}

fn check_queue_and_detach(desktop: &NativeDesktop) -> Result<(), Box<dyn std::error::Error>> {
    let session = desktop.create_session_for_agent(None)?;
    let mut first = desktop.send_chat(NativeChatInput {
        session_id: session.id.clone(),
        content: "native-queue-first".into(),
        client_message_id: None,
        attachments: Vec::new(),
    })?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        ensure(Instant::now() < deadline, "first queued test timeout")?;
        match first.try_recv()? {
            Some(NativeChatEvent::Event(event)) if event["event"] == "llm_output_delta" => break,
            Some(NativeChatEvent::Failed(error)) => return Err(error.into()),
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let mut second = desktop.send_chat(NativeChatInput {
        session_id: session.id.clone(),
        content: "native-queue-second".into(),
        client_message_id: None,
        attachments: Vec::new(),
    })?;
    let mut queued = false;
    let mut answer = String::new();
    // Detach while the first runner still owns its lease. It must finish and
    // dispatch the queued request, without a synthetic user cancellation.
    drop(first);
    loop {
        ensure(Instant::now() < deadline, "queued request did not settle")?;
        match second.try_recv()? {
            Some(NativeChatEvent::Queued) => queued = true,
            Some(NativeChatEvent::Event(event)) if event["event"] == "final" => {
                let envelope = &event["data"];
                answer = envelope.get("data").unwrap_or(envelope)["answer"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
            }
            Some(NativeChatEvent::Finished) => break,
            Some(NativeChatEvent::Failed(error)) => return Err(error.into()),
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    ensure(queued, "concurrent turn bypassed queue")?;
    ensure(
        answer == "测试第二轮".repeat(10),
        "queue replay mixed neighbouring turns",
    )?;
    let (_, history) = desktop.get_session(&session.id)?;
    ensure(
        history.iter().filter(|row| row.mine).count() == 2,
        "queued user turn missing",
    )?;
    ensure(
        history
            .iter()
            .any(|row| !row.mine && row.text == "测试第一轮".repeat(80)),
        "detached turn was cancelled",
    )?;
    Ok(())
}

pub fn run(app: &MainWindow, directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let result = Rc::new(RefCell::new(None));
    let output = result.clone();
    let weak = app.as_weak();
    let timer = Timer::default();
    let mut step = 0;
    let mut paused_scroll = None;
    let start = Instant::now();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let Some(app) = weak.upgrade() else { return };
        let checked = if start.elapsed() > Duration::from_secs(80) {
            Err(format!("step {step}: timed out; {}", app.get_status()).into())
        } else {
            advance(&app, &directory, &mut step, &mut paused_scroll)
        };
        if matches!(checked, Ok(false)) {
            return;
        }
        let report = match &checked {
            Ok(_) => {
                "PASS: native SQLite/create/send/stream/history/cancel/input/navigation/scroll/agents/tools/settings/files\n"
                    .to_string()
            }
            Err(error) => format!("FAIL: step {step}: {error}\n"),
        };
        let checked = std::fs::write(directory.join("smoke.txt"), report)
            .map_err(|error| error.to_string())
            .and(checked.map(|_| ()).map_err(|error| error.to_string()));
        *output.borrow_mut() = Some(checked);
        let _ = slint::quit_event_loop();
    });
    app.run()?;
    timer.stop();
    result
        .borrow_mut()
        .take()
        .ok_or("native smoke did not run")??;
    Ok(())
}

fn advance(
    app: &MainWindow,
    directory: &std::path::Path,
    step: &mut u8,
    paused_scroll: &mut Option<i32>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if *step == 3 && app.get_busy() && app.get_stream_updates() > 3 {
        if paused_scroll.is_none() {
            app.set_draft("测试草稿".into());
            crate::smoke::click(app, 28.0, 160.0)?;
            ensure(app.get_section() == 2, "navigation blocked")?;
            crate::smoke::click(app, 28.0, 102.0)?;
            ensure(app.get_section() == 0, "return navigation blocked")?;
            app.set_follow_output(false);
            *paused_scroll = Some(app.get_scroll_revision());
        } else {
            ensure(
                *paused_scroll == Some(app.get_scroll_revision()),
                "scrolled while reading history",
            )?;
        }
    }
    if *step == 7 && app.get_busy() && app.get_stream_bytes() > 0 && !app.get_stopping() {
        app.invoke_stop_generation();
    }
    if app.get_chat_loading()
        || app.get_session_loading()
        || app.get_creating_session()
        || app.get_busy()
        || app.get_saving()
        || app.get_agents_loading()
        || app.get_settings_loading()
        || app.get_tools_loading()
        || app.get_files_loading()
        || app.get_preview_loading()
    {
        return Ok(false);
    }
    match *step {
        0 => {
            app.invoke_new_thread();
        }
        1 => {
            ensure(!app.get_active_session_id().is_empty(), "session missing")?;
            ensure(
                app.get_messages().row_count() == 0,
                "new session contains messages",
            )?;
            app.set_draft("native-long".into());
        }
        2 => {
            app.invoke_send_message();
        }
        3 => {
            let expected = DELTA.repeat(600);
            let answer = app
                .get_messages()
                .iter()
                .filter(|message| !message.mine)
                .last()
                .ok_or("answer missing")?;
            ensure(
                answer.text.as_str() == expected.trim_end(),
                "stream text mismatch",
            )?;
            ensure(
                paused_scroll.is_some() && app.get_draft() == "测试草稿",
                "input did not survive stream",
            )?;
            ensure(app.get_stream_updates() > 3, "no continuous UI updates")?;
            std::fs::write(
                directory.join("stream-metrics.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "bytes": app.get_stream_bytes(), "updates": app.get_stream_updates(),
                "max_ui_apply_ms": app.get_stream_max_ui_ms(), "max_backlog": app.get_stream_max_backlog(), "channel_capacity":128
                }))?,
            )?;
            crate::smoke::snapshot(app, &directory.join("native-chat.png"))?;
            app.invoke_select_conversation(0);
        }
        4 => {
            let expected = DELTA.repeat(600);
            ensure(
                app.get_messages()
                    .iter()
                    .any(|row| !row.mine && row.text.as_str() == expected.trim_end()),
                "history differs from stream",
            )?;
            app.invoke_new_thread();
        }
        5 => {
            app.set_draft("native-cancel".into());
        }
        6 => {
            app.invoke_send_message();
        }
        7 => {
            ensure(app.get_status() == "已停止", "cancel did not settle")?;
            crate::smoke::snapshot(app, &directory.join("native-stopped.png"))?;
            app.set_section(2);
            app.invoke_create_agent("test-ui-agent".into());
        }
        8 => {
            ensure(
                app.get_selected_agent_name() == "test-ui-agent",
                "native UI create failed",
            )?;
            app.invoke_save_agent(
                "test-ui-updated".into(),
                "test-description".into(),
                "test-prompt".into(),
                "".into(),
                "spark".into(),
                "#f97316".into(),
            );
        }
        9 => {
            ensure(
                app.get_selected_agent_name() == "test-ui-updated",
                "native UI save failed",
            )?;
            crate::smoke::snapshot(app, &directory.join("native-agents.png"))?;
            app.set_section(3);
            app.invoke_refresh_tools();
        }
        10 => {
            ensure(app.get_tools().row_count() > 0, "native tools empty")?;
            crate::smoke::snapshot(app, &directory.join("native-tools.png"))?;
            app.set_section(4);
            app.invoke_save_model(
                "test-ui-model".into(),
                "openai".into(),
                "test-model".into(),
                "".into(),
                "".into(),
                "embedding".into(),
            );
        }
        11 => {
            ensure(
                app.get_models().iter().any(|m| m.key == "test-ui-model"),
                "native UI model save failed",
            )?;
            app.invoke_set_default_model("test-ui-model".into());
        }
        12 => {
            ensure(
                app.get_models()
                    .iter()
                    .any(|m| m.key == "test-ui-model" && m.is_default),
                "native UI default failed",
            )?;
            crate::smoke::snapshot(app, &directory.join("native-settings.png"))?;
            app.invoke_save_runtime(app.get_workspace_root(), "zh-CN".into());
        }
        13 => {
            app.set_section(0);
            app.invoke_navigate_directory("".into());
        }
        14 => {
            ensure(
                app.get_files().iter().any(|f| f.name == "test.txt"),
                "native UI file list failed",
            )?;
            app.invoke_open_file("test.txt".into());
        }
        15 => {
            ensure(
                app.get_preview_text().starts_with("测试文本"),
                "native UI preview failed",
            )?;
            crate::smoke::snapshot(app, &directory.join("native-workspace.png"))?;
            return Ok(true);
        }
        _ => return Err("unexpected smoke step".into()),
    }
    *step += 1;
    Ok(false)
}

fn ensure(condition: bool, error: &str) -> Result<(), Box<dyn std::error::Error>> {
    if condition {
        Ok(())
    } else {
        Err(error.into())
    }
}
