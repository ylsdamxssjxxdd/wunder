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

/// The runtime collapses online *_delta frames into thread_item_delta and
/// keeps the semantic stream type in data.source_event.
fn is_llm_text_delta(event: &serde_json::Value) -> bool {
    match event["event"].as_str() {
        Some("llm_output_delta") => true,
        Some("thread_item_delta") => {
            let envelope = &event["data"];
            let data = envelope.get("data").unwrap_or(envelope);
            data["source_event"].as_str() == Some("llm_output_delta")
        }
        _ => false,
    }
}

pub fn check_runtime(desktop: &NativeDesktop) -> Result<(), Box<dyn std::error::Error>> {
    ensure(
        desktop.get_session("missing-session").is_err(),
        "missing session accepted",
    )?;
    ensure(
        desktop.cancel_chat("missing-session").is_err(),
        "unowned cancellation accepted",
    )?;
    let session = desktop.create_session(None)?;
    ensure(
        desktop
            .list_sessions(None)?
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
        reasoning_effort: None,
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
    let session = desktop.create_session(None)?;
    let mut first = desktop.send_chat(NativeChatInput {
        session_id: session.id.clone(),
        content: "native-queue-first".into(),
        reasoning_effort: None,
        client_message_id: None,
        attachments: Vec::new(),
    })?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        ensure(Instant::now() < deadline, "first queued test timeout")?;
        match first.try_recv()? {
            Some(NativeChatEvent::Event(event)) if is_llm_text_delta(&event) => break,
            Some(NativeChatEvent::Failed(error)) => return Err(error.into()),
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let mut second = desktop.send_chat(NativeChatInput {
        session_id: session.id.clone(),
        content: "native-queue-second".into(),
        reasoning_effort: None,
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
            // The durable follower replays the queued turn's committed item as
            // one authoritative llm_output; each commit overwrites the answer.
            Some(NativeChatEvent::Event(event)) if event["event"] == "llm_output" => {
                let envelope = &event["data"];
                let data = envelope.get("data").unwrap_or(envelope);
                answer = data["content"].as_str().unwrap_or_default().to_string();
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
    let mut paused_bytes = 0;
    // The session the long stream ran in. Step 4 reloads history through the
    // conversation list, and the harness has already used other sessions by
    // then, so the turn under test has to be identified by id, not by index.
    let mut streamed_session: Option<String> = None;
    let start = Instant::now();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let Some(app) = weak.upgrade() else { return };
        let checked = if start.elapsed() > Duration::from_secs(80) {
            Err(format!("step {step}: timed out; {}", app.get_status()).into())
        } else {
            advance(
                &app,
                &directory,
                &mut step,
                &mut paused_scroll,
                &mut paused_bytes,
                &mut streamed_session,
            )
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
    paused_scroll: &mut Option<f32>,
    paused_bytes: &mut i32,
    streamed_session: &mut Option<String>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if *step == 3 && app.get_busy() && app.get_stream_updates() > 3 {
        if paused_scroll.is_none() {
            app.set_draft("测试草稿".into());
            // The two-column shell replaced the old icon rail, so navigation is
            // driven by the sidebar's settings button and the settings sheet's
            // own back action. Both coordinates come from the layout itself
            // (`MainWindow.settings-*`), because the button sits against the
            // bottom edge and a hard-coded pixel would only hold for one window
            // size.
            crate::smoke::click(app, app.get_settings_button_x(), app.get_settings_button_y())?;
            ensure(
                app.get_settings_open(),
                "settings window did not open",
            )?;
            crate::smoke::click(app, app.get_settings_back_x(), app.get_settings_back_y())?;
            ensure(
                !app.get_settings_open(),
                "closing the settings window blocked",
            )?;
            app.set_follow_output(false);
            *paused_scroll = Some(app.get_scroll_y());
            *paused_bytes = app.get_stream_bytes();
        } else {
            // The stream keeps growing while the user reads back, so this branch
            // runs again and again. The invariant is that new content does not
            // move the viewport -- so it is only asserted on ticks where the
            // answer actually grew. A tick with no growth cannot distinguish
            // "held" from "the view clamped because the column got shorter",
            // and a page transition can clamp it once.
            let grew = app.get_stream_bytes() > *paused_bytes;
            if grew {
                ensure(
                    *paused_scroll == Some(app.get_scroll_y()),
                    &format!(
                        "new stream content moved the paused viewport: {:?} -> {:?} ({} -> {} bytes)",
                        paused_scroll,
                        app.get_scroll_y(),
                        paused_bytes,
                        app.get_stream_bytes(),
                    ),
                )?;
                println!(
                    "paused viewport held at {:?} while the answer grew {} -> {} bytes",
                    paused_scroll,
                    paused_bytes,
                    app.get_stream_bytes(),
                );
                *paused_bytes = app.get_stream_bytes();
            } else if paused_scroll.is_none() {
                *paused_scroll = Some(app.get_scroll_y());
            }
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
    {
        return Ok(false);
    }
    match *step {
        0 => {
            app.invoke_new_thread();
        }
        1 => {
            ensure(!app.get_active_session_id().is_empty(), "session missing")?;
            // The UI materializes one presentation-only greeting before the
            // transcript; a new session must not contain anything else.
            ensure(
                app.invoke_timeline_counts().rows == 0,
                "new session contains messages",
            )?;
            app.set_draft("native-long".into());
        }
        2 => {
            // The native handler drops the input while the passive observer is
            // still rehydrating the session, so the send is not fire-and-forget.
            // Prove it took -- the input was consumed or the turn started -- and
            // otherwise ask to be re-run, instead of letting step 3 compare an
            // empty timeline against the expected stream.
            app.invoke_send_message();
            if !app.get_busy() && app.get_draft() == "native-long" {
                return Ok(false);
            }
        }
        3 => {
            let expected = DELTA.repeat(600);
            let answer = app.invoke_timeline_last_answer();
            ensure(
                answer.as_str() == expected.trim_end(),
                &format!(
                    "stream text mismatch: got {} bytes want {} bytes; got_start={:?} got_end={:?} status={:?}",
                    answer.len(),
                    expected.trim_end().len(),
                    &answer.chars().take(24).collect::<String>(),
                    answer.chars().rev().take(24).collect::<String>(),
                    app.get_status(),
                ),
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
            // Remember which session this turn belongs to before navigating
            // away: step 4 has to reload exactly this one, and the restore
            // phase (a separate process) needs the id too -- titles come from
            // the first message, so they cannot identify this turn.
            let session_id = app.get_active_session_id().to_string();
            *streamed_session = Some(session_id.clone());
            std::fs::write(
                directory.join("streamed-session.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "session_id": session_id,
                    "answer": expected.trim_end(),
                }))?,
            )?;
            app.invoke_select_conversation(0);
        }
        4 => {
            let expected = DELTA.repeat(600);
            // Reload the session the stream ran in. Selecting by id matters:
            // the harness has already used other sessions by now, and
            // "conversation 0" is ordered by last activity, not by identity.
            let Some(session) = streamed_session.clone() else {
                return Err("step 3 did not record the streamed session".into());
            };
            let target = app
                .get_conversations()
                .iter()
                .position(|row| row.id == session.as_str());
            let Some(target) = target else {
                return Err(format!("streamed session {session} left the conversation list").into());
            };
            if app.get_active_session_id() != session.as_str() {
                app.invoke_select_conversation(target as i32);
                return Ok(false);
            }
            // The passive observer rehydrates history asynchronously after the
            // active stream detaches; wait for its reset before comparing.
            if app.invoke_timeline_counts().answers == 0 {
                ensure(
                    !app.get_status().starts_with("无法"),
                    "history reload failed",
                )?;
                return Ok(false);
            }
            // `timeline_last_answer` is a whole-timeline projection, so a durable
            // reload that carries more than the turn under test is not a
            // failure; the streamed content must be present in full.
            let actual = app.invoke_timeline_last_answer().to_string();
            let want = expected.trim_end();
            ensure(
                actual.contains(want),
                &format!(
                    "history differs from stream: rows={} status={:?} got {} bytes want {} bytes contained; got_start={:?}",
                    app.invoke_timeline_counts().rows,
                    app.get_status(),
                    actual.len(),
                    want.len(),
                    &actual.chars().take(24).collect::<String>(),
                ),
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
            // The stop request is only issued while the stream is busy, so the
            // short cancel stream can settle on its own first. The status text
            // differs between the two paths (stopped vs already finished) and is
            // a UI affordance, not the contract. What must hold is that
            // generation is over and nothing is left armed for the next turn.
            ensure(
                !app.get_busy() && !app.get_stopping(),
                &format!(
                    "cancel did not settle: busy={} stopping={} status={:?} bytes={}",
                    app.get_busy(),
                    app.get_stopping(),
                    app.get_status(),
                    app.get_stream_bytes(),
                ),
            )?;
            crate::smoke::snapshot(app, &directory.join("native-stopped.png"))?;
            // The agent category lives in the settings window now.
            app.set_settings_open(true);
            app.set_settings_active_panel(3);
        }
        8 => {
            // Single-agent: the fixed default agent must be present and named.
            ensure(
                app.get_agents().row_count() >= 1 && app.get_selected_agent_name() != "",
                "native UI default agent missing",
            )?;
            crate::smoke::snapshot(app, &directory.join("native-agents.png"))?;
            app.set_settings_active_panel(2);
            app.invoke_refresh_tools();
        }
        9 => {
            ensure(app.get_tools().row_count() > 0, "native tools empty")?;
            crate::smoke::snapshot(app, &directory.join("native-tools.png"))?;
            app.set_settings_open(true);
            app.set_model_key_draft("test-ui-model".into());
            app.set_model_provider_draft("openai".into());
            app.set_model_name_draft("test-model".into());
            app.set_model_base_url_draft("".into());
            app.set_model_token_draft("".into());
            app.set_model_type_draft("embedding".into());
            app.invoke_save_model();
        }
        10 => {
            ensure(
                app.get_models().iter().any(|m| m.key == "test-ui-model"),
                "native UI model save failed",
            )?;
            app.invoke_set_default_model("test-ui-model".into());
        }
        11 => {
            ensure(
                app.get_models()
                    .iter()
                    .any(|m| m.key == "test-ui-model" && m.is_default),
                "native UI default failed",
            )?;
            crate::smoke::snapshot(app, &directory.join("native-settings.png"))?;
            app.invoke_save_runtime(
                app.get_workspace_root(),
                "zh-CN".into(),
                "".into(),
                "".into(),
                "".into(),
            );
        }
        12 => {
            // §9.3 agent form: point the built-in agent at a configured model
            // key and drive the panel's own save action. This step cannot pass
            // without a native `save-agent` handler, which the settings panel
            // needs because the preview build owns the only other binding.
            app.set_settings_active_panel(3);
            // Take the model from the live catalogue: the runtime only keeps the
            // global default on the built-in agent, so an invented key would
            // exercise the rejection path instead of the save path.
            app.set_selected_agent_model(agent_form_model(&app)?.into());
            app.set_selected_agent_description("冒烟检查问候语".into());
            app.set_agent_question_draft("冒烟检查预设问题".into());
            app.invoke_add_agent_question();
            app.invoke_refresh_agent_dirty();
            ensure(app.get_agent_dirty(), "editing did not dirty the agent form")?;
            app.invoke_save_agent_form();
        }
        13 => {
            ensure(
                !app.get_status().starts_with("错误："),
                &format!("agent save failed: {}", app.get_status()),
            )?;
            ensure(
                app.get_selected_agent_model() == agent_form_model(&app)?.as_str()
                    && app.get_selected_agent_description() == "冒烟检查问候语",
                "saved agent settings did not project back",
            )?;
            ensure(
                app.get_selected_agent_preset_questions()
                    .iter()
                    .any(|question| question == "冒烟检查预设问题"),
                "saved preset questions did not survive the round trip",
            )?;
            ensure(!app.get_agent_dirty(), "a saved agent form is still dirty")?;
            ensure(
                app.get_expert_tool_groups()
                    .iter()
                    .any(|group| group.tools.row_count() > 0),
                "agent tool list is empty",
            )?;
            // The card picker's select-all drives the same draft the save handler
            // reads. Two clicks are not an identity (partial -> all -> none), so
            // the contract to prove is: it dirties the form, and re-loading the
            // agent rebases the snapshot.
            let group = app
                .get_expert_tool_groups()
                .row_data(0)
                .map(|group| group.title.to_string())
                .unwrap_or_default();
            app.invoke_toggle_expert_tool_group(group.as_str().into());
            ensure(
                app.get_agent_dirty(),
                "the tool card picker did not dirty the agent form",
            )?;
            app.invoke_select_agent(0);
            ensure(
                !app.get_agent_dirty(),
                "re-loading the agent did not rebase the saved selection",
            )?;
            // The agent form is a card picker, not a switch list: its rendered
            // shape is only provable from the window, so the snapshot is part of
            // the step rather than an optional artifact.
            crate::smoke::snapshot(app, &directory.join("native-agent-form.png"))?;
        }
        14 => {
            app.set_page(crate::DesktopPage::Messages);
            crate::smoke::snapshot(app, &directory.join("native-workspace.png"))?;
            return Ok(true);
        }
        _ => return Err("unexpected smoke step".into()),
    }
    *step += 1;
    Ok(false)
}


/// The model the built-in agent's form may legitimately carry: the catalogue's
/// global default, falling back to the first configured model.
fn agent_form_model(app: &MainWindow) -> Result<String, Box<dyn std::error::Error>> {
    app.get_models()
        .iter()
        .find(|entry| entry.is_default)
        .or_else(|| app.get_models().iter().next())
        .map(|entry| entry.key.to_string())
        .ok_or_else(|| "the runtime exposed no model to drive the agent form with".into())
}

pub(crate) fn ensure(condition: bool, error: &str) -> Result<(), Box<dyn std::error::Error>> {
    if condition {
        Ok(())
    } else {
        Err(error.into())
    }
}
