//! Desktop runtime settings use the same control endpoint as the TS frontend.
use crate::{chat_api::ChatApi, MainWindow};
use serde_json::json;
use slint::ComponentHandle;

pub fn install(app: &MainWindow, api: ChatApi) {
    let weak = app.as_weak();
    app.on_save_runtime(move |workspace, language| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        if workspace.trim().is_empty() || !matches!(language.as_str(), "zh-CN" | "en-US") {
            app.set_status("请填写工作目录和有效的运行时语言".into());
            return;
        }
        app.set_saving(true);
        let api = api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api
                .put_json(
                    "/desktop/settings",
                    json!({"workspace_root": workspace.as_str(), "language": language.as_str()}),
                )
                .and_then(|_| api.get_desktop_settings());
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(settings) => {
                        crate::chat_runtime::apply_settings(&app, settings);
                        app.set_status("运行时设置已保存".into());
                        app.invoke_navigate_directory("".into());
                    }
                    Err(error) => {
                        app.set_dialog_title("保存失败".into());
                        app.set_dialog_text(error.into());
                        app.set_dialog_open(true);
                    }
                }
            });
        });
    });
}
