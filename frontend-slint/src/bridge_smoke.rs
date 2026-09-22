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
            Ok(_) => "PASS: bridge agent/catalog/settings load, create agent, selected-agent session, model create/edit/default, chat send/history, selection refresh.\n".to_string(),
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
    if app.get_saving()
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
            ensure(
                app.get_messages()
                    .iter()
                    .any(|message| !message.mine && message.text == "测试回复"),
                "chat reply missing",
            )?;
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
