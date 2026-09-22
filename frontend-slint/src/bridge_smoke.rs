//! Explicit, opt-in smoke driver. Run only against an isolated local bridge.
use crate::MainWindow;
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

pub fn run(app: &MainWindow, directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&directory)?;
    let result = Rc::new(RefCell::new(None));
    let output = result.clone();
    let weak = app.as_weak();
    let timer = Timer::default();
    let mut step = 0;
    let mut session = String::new();
    let start = Instant::now();
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(app) = weak.upgrade() else { return };
        let checked = if app.get_dialog_open() {
            Err(format!("step {step}: {}", app.get_dialog_text()))
        } else if start.elapsed() > Duration::from_secs(90) {
            Err(format!("step {step}: timed out"))
        } else {
            advance(&app, &directory, &mut step, &mut session).map_err(|error| error.to_string())
        };
        if matches!(checked, Ok(false)) { return; }
        let report = match &checked {
            Ok(_) => "PASS: bridge agent/catalog/settings load, create agent, selected-agent session, model create/edit/default, chat send/history, selection refresh, agent edit, directory navigation/preview, runtime settings.\n".to_string(),
            Err(error) => format!("FAIL: {error}\n"),
        };
        let checked = std::fs::write(directory.join("smoke.txt"), report)
            .map_err(|error| error.to_string()).and(checked.map(|_| ()));
        *output.borrow_mut() = Some(checked);
        let _ = slint::quit_event_loop();
    });
    app.run()?;
    timer.stop();
    result
        .borrow_mut()
        .take()
        .ok_or("bridge smoke check did not run")??;
    Ok(())
}

fn advance(
    app: &MainWindow,
    directory: &std::path::Path,
    step: &mut u8,
    session: &mut String,
) -> Result<bool, Box<dyn std::error::Error>> {
    if *step == 23 && app.get_busy() && app.get_stream_updates() > 3 {
        // Input and pointer navigation must remain responsive during real deltas.
        app.set_draft("输出期间草稿".into());
        crate::smoke::click(app, 28.0, 160.0)?;
        ensure(app.get_section() == 2, "navigation blocked by stream")?;
        crate::smoke::click(app, 28.0, 102.0)?;
        app.set_follow_output(false);
    }
    if app.get_files_loading()
        || app.get_preview_loading()
        || app.get_saving()
        || app.get_chat_loading()
        || app.get_session_loading()
        || app.get_creating_session()
        || app.get_agents_loading()
        || app.get_tools_loading()
        || app.get_settings_loading()
        || app.get_busy()
    {
        return Ok(false);
    }
    match *step {
        0 => {
            ensure(
                app.get_agents()
                    .iter()
                    .any(|agent| agent.id == "__default__"),
                "default agent missing",
            )?;
            app.invoke_refresh_tools();
            app.invoke_refresh_settings();
        }
        1 => {
            ensure(app.get_tools().row_count() > 0, "tool catalog empty")?;
            ensure(
                !app.get_workspace_root().is_empty(),
                "workspace settings missing",
            )?;
            app.set_section(3);
            crate::smoke::snapshot(app, &directory.join("connected-tools.png"))?;
            app.invoke_create_agent("测试智能体".into());
        }
        2 => {
            ensure(
                app.get_selected_agent_name() == "测试智能体",
                "agent creation failed",
            )?;
            app.invoke_refresh_agents();
        }
        3 => {
            ensure(
                app.get_selected_agent_name() == "测试智能体",
                "agent selection changed after refresh",
            )?;
            app.set_section(2);
            crate::smoke::snapshot(app, &directory.join("connected-agents.png"))?;
            // The harness seeds an isolated local model endpoint; no external LLM is used.
            let model = app
                .get_models()
                .iter()
                .find(|model| model.key == "test-model")
                .ok_or("isolated test model missing")?;
            app.invoke_save_model(
                "测试配置".into(),
                model.provider,
                model.model,
                model.base_url,
                "".into(),
                "llm".into(),
            );
        }
        4 => {
            ensure(
                app.get_selected_model_key() == "测试配置",
                "model creation failed",
            )?;
            app.invoke_save_model(
                "测试配置".into(),
                app.get_selected_model_provider(),
                "test-model-edited".into(),
                app.get_selected_model_base_url(),
                "".into(),
                "llm".into(),
            );
        }
        5 => {
            ensure(
                app.get_selected_model_name() == "test-model-edited",
                "model edit failed",
            )?;
            app.invoke_set_default_model("测试配置".into());
        }
        6 => {
            ensure(app.get_selected_model_is_default(), "model default failed")?;
            app.invoke_refresh_settings();
        }
        7 => {
            ensure(
                app.get_selected_model_key() == "测试配置" && app.get_selected_model_is_default(),
                "model selection changed after refresh",
            )?;
            app.set_section(4);
            crate::smoke::snapshot(app, &directory.join("connected-settings.png"))?;
            app.invoke_new_thread();
        }
        8 => {
            let agent = app
                .get_agents()
                .row_data(app.get_selected_agent() as usize)
                .ok_or("agent not selected")?;
            ensure(
                app.get_active_agent_id() == agent.id,
                "session used the wrong agent",
            )?;
            *session = app.get_active_session_id().to_string();
            app.set_section(0);
            app.set_draft("测试消息".into());
            app.invoke_send_message();
            app.invoke_send_message();
            app.invoke_new_thread();
            ensure(
                app.get_active_session_id() == session.as_str(),
                "sending allowed a session change",
            )?;
        }
        9 => {
            if !app
                .get_messages()
                .iter()
                .any(|message| !message.mine && message.text == "测试回复")
            {
                let rows = app
                    .get_messages()
                    .iter()
                    .map(|message| format!("{}:{}", message.mine, message.text))
                    .collect::<Vec<_>>()
                    .join(" | ");
                return Err(format!(
                    "chat reply missing; status={}; rows={rows}",
                    app.get_status()
                )
                .into());
            }
            app.invoke_select_conversation(app.get_selected_conversation());
        }
        10 => {
            ensure(
                app.get_active_session_id() == session.as_str(),
                "session changed",
            )?;
            ensure(
                app.get_messages()
                    .iter()
                    .any(|message| !message.mine && message.text == "测试回复"),
                "persisted history missing",
            )?;
            crate::smoke::snapshot(app, &directory.join("connected-chat.png"))?;
            app.set_draft("未发送内容".into());
            app.invoke_new_thread();
        }
        11 => {
            ensure(
                app.get_active_session_id() != session.as_str() && app.get_draft().is_empty(),
                "new session retained the wrong draft",
            )?;
            let index = app
                .get_conversations()
                .iter()
                .position(|row| row.id == session.as_str())
                .ok_or("previous session missing")?;
            app.invoke_select_conversation(index as i32);
        }
        12 => {
            ensure(
                app.get_draft() == "未发送内容",
                "new session lost the previous draft",
            )?;
            ensure(
                app.get_messages()
                    .iter()
                    .filter(|message| message.mine && message.text == "测试消息")
                    .count()
                    == 1,
                "duplicate send was persisted",
            )?;
            app.invoke_refresh_chat();
        }
        13 => {
            ensure(
                app.get_active_session_id() == session.as_str() && app.get_draft() == "未发送内容",
                "refresh changed session or draft",
            )?;
            app.invoke_save_agent(
                "测试智能体".into(),
                "测试描述".into(),
                "  测试提示词\n第二行\n".into(),
                "".into(),
            );
        }
        14 => {
            app.invoke_refresh_agents();
        }
        15 => {
            ensure(
                app.get_selected_agent_system_prompt().trim() == "测试提示词\n第二行",
                "persisted prompt changed",
            )?;
            ensure(
                app.get_selected_agent_model().is_empty(),
                "default model inheritance lost",
            )?;
            app.set_section(2);
            crate::smoke::snapshot(app, &directory.join("connected-agent-edit.png"))?;
            // File fixtures are seeded into the default workspace, independently of agent containers.
            app.set_active_agent_id("".into());
            app.invoke_navigate_directory("".into());
        }
        16 => {
            ensure(app.get_files_error().is_empty(), "workspace load failed")?;
            let directory_entry = app
                .get_files()
                .iter()
                .find(|file| file.name == "test-directory")
                .ok_or("seeded directory missing")?;
            ensure(directory_entry.entry_type == "dir", "wrong directory type")?;
            app.invoke_open_file(directory_entry.path);
        }
        17 => {
            ensure(
                app.get_directory_path().contains("test-directory"),
                "directory navigation failed",
            )?;
            let file = app
                .get_files()
                .iter()
                .find(|file| file.name == "test.txt")
                .ok_or("seeded file missing")?;
            app.set_section(5);
            crate::smoke::snapshot(app, &directory.join("connected-files.png"))?;
            app.invoke_open_file(file.path);
        }
        18 => {
            ensure(
                app.get_preview_text() == "测试文件\n第二行\n",
                "workspace preview changed",
            )?;
            crate::smoke::snapshot(app, &directory.join("connected-file-preview.png"))?;
            app.set_preview_open(false);
            app.invoke_navigate_directory(app.get_directory_parent());
        }
        19 => {
            ensure(
                app.get_files()
                    .iter()
                    .any(|file| file.name == "test-directory"),
                "parent directory failed",
            )?;
            app.invoke_save_runtime(app.get_workspace_root(), "en-US".into());
        }
        20 => {
            app.invoke_refresh_settings();
        }
        21 => {
            ensure(
                app.get_runtime_language() == "en-US",
                "runtime settings were not persisted",
            )?;
            app.set_section(0);
            app.invoke_new_thread();
        }
        22 => {
            app.set_draft("流式压力测试".into());
            app.invoke_send_message();
        }
        23 => {
            let expected = format!("{}\n", "增量测试内容。".repeat(8)).repeat(600);
            ensure(
                app.get_messages()
                    .iter()
                    .any(|row| !row.mine && row.text == expected),
                "stream text incomplete",
            )?;
            ensure(app.get_draft() == "输出期间草稿", "stream lost input draft")?;
            ensure(!app.get_follow_output(), "stream forced scroll following")?;
            ensure(
                app.get_stream_updates() > 10,
                "reply did not stream incrementally",
            )?;
            std::fs::write(
                directory.join("stream-metrics.json"),
                serde_json::json!({
                    "bytes": app.get_stream_bytes(), "updates": app.get_stream_updates(),
                    "max_ui_apply_ms": app.get_stream_max_ui_ms(), "channel_capacity": 128,
                    "text_integrity": true, "input_during_stream": true,
                    "navigation_during_stream": true, "follow_output_preserved": true,
                })
                .to_string(),
            )?;
            return Ok(true);
        }
        _ => return Err("invalid smoke step".into()),
    }
    *step += 1;
    Ok(false)
}

fn ensure(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
