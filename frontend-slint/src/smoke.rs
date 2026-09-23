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
    app.set_subagents(slint::ModelRc::new(slint::VecModel::from(vec![crate::Conversation {
        id: "child".into(), title: "子智能体".into(), preview: "已中断 · 可复用".into(), time: "".into(),
    }])));
    app.set_subagents_status("保留历史，可由主智能体继续分派".into());
    snapshot(app, &directory.join("native-preview.png"))?;
    // Dispatch pointer events through Slint hit testing. Invoking callbacks
    // directly cannot detect focus-only first clicks or decorative overlays.
    for (section, y) in [(2, 160.0), (3, 218.0), (0, 102.0)] {
        click(app, 28.0, y)?;
        require(
            app.get_section() == section,
            "rail navigation required another click",
        )?;
    }
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
    // The prototype follows the original light visual and no longer exposes
    // a theme switch. Keep a light snapshot for regression.
    snapshot(app, &directory.join("native-light.png"))?;
    app.set_section(2);
    let count = app.get_agents().row_count();
    app.invoke_create_agent("测试智能体".into());
    require(
        app.get_agents().row_count() == count + 1 && app.get_selected_agent_name() == "测试智能体",
        "agent creation failed",
    )?;
    app.invoke_save_agent(
        "测试智能体".into(),
        "测试描述".into(),
        "  测试提示词\n第二行\n".into(),
        "".into(),
        "spark".into(),
        "#f97316".into(),
    );
    require(
        app.get_selected_agent_system_prompt() == "  测试提示词\n第二行\n",
        "prompt edit lost whitespace",
    )?;
    snapshot(app, &directory.join("native-agents.png"))?;
    app.set_section(3);
    snapshot(app, &directory.join("native-tools.png"))?;
    // The workspace stays in the chat dock; the standalone files rail entry
    // was removed to match the web messenger information architecture.
    snapshot(app, &directory.join("native-workspace.png"))?;
    app.set_section(4);
    app.invoke_save_model(
        "测试模型".into(),
        "openai".into(),
        "test-model".into(),
        "".into(),
        "".into(),
        "llm".into(),
    );
    app.invoke_set_default_model("测试模型".into());
    require(
        app.get_selected_model_key() == "测试模型" && app.get_selected_model_is_default(),
        "model/default update failed",
    )?;
    snapshot(app, &directory.join("native-settings.png"))?;
    app.set_section(5);
    snapshot(app, &directory.join("native-profile.png"))?;
    app.set_model_key_draft(app.get_selected_model_key());
    app.set_model_editor_open(true);
    app.set_dialog_title("操作失败".into());
    app.set_dialog_text("请检查模型配置后重试。".into());
    app.set_dialog_open(true);
    snapshot(app, &directory.join("native-form-error.png"))?;
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
