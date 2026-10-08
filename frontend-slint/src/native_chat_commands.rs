//! UI routing for local commands; heavy work stays off the event loop.
use super::*;
use wunder_desktop::native::NativeChatCommand;
pub(super) fn dispatch(app: &MainWindow, state: &Rc<RefCell<State>>, content: &str) -> bool {
    let Some(command) = NativeChatCommand::parse(content) else {
        return false;
    };
    app.set_command_feedback("".into());
    if app.get_command_pending() || app.get_session_loading() || app.get_creating_session() {
        return true;
    }
    match command {
        NativeChatCommand::Help => {
            app.set_command_feedback("/new 新建线程 · /stop 停止生成 · /goal 目标 · /goal resume 继续 · /goal pause 暂停 · /goal clear 清除 · /compact 压缩上下文".into());
            app.set_draft("".into());
            return true;
        }
        NativeChatCommand::Stop => {
            app.invoke_stop_generation();
            app.set_draft("".into());
            return true;
        }
        NativeChatCommand::New => {
            app.invoke_new_thread();
            return true;
        }
        NativeChatCommand::Compact if app.get_busy() => {
            app.set_command_feedback("请先停止当前任务再压缩上下文".into());
            return true;
        }
        _ => {}
    }
    let session = app.get_active_session_id().to_string();
    if session.is_empty() {
        app.set_command_feedback("请先进入一个会话后再使用命令".into());
        return true;
    }
    if app.get_pending_attachments().row_count() > 0 {
        app.set_command_feedback("请先单独发送附件，再执行命令".into());
        return true;
    }
    app.set_command_pending(true);
    app.set_command_feedback("".into());
    app.set_status(
        if command == NativeChatCommand::Compact {
            "正在提交上下文压缩…"
        } else {
            "正在处理目标…"
        }
        .into(),
    );
    let desktop = state.borrow().desktop.clone();
    let weak = app.as_weak();
    let submitted = content.to_owned();
    std::thread::spawn(move || {
        let result = desktop.execute_chat_command(&session, command);
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_command_pending(false);
            if app.get_active_session_id() != session {
                return;
            }
            match result {
                Ok(message) => {
                    if app.get_draft() == submitted {
                        app.set_draft("".into());
                    }
                    app.set_command_feedback(message.into());
                    app.invoke_refresh_chat();
                }
                Err(error) => app.set_command_feedback(format!("命令执行失败：{error}").into()),
            }
        });
    });
    true
}
