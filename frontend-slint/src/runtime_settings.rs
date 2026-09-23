//! Desktop configuration callback backed by the embedded runtime.
use crate::MainWindow;
use slint::ComponentHandle;
use std::sync::Arc;
use wunder_desktop::NativeDesktop;

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
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
            let result = api.save_runtime(&workspace, &language);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(settings) => {
                        crate::native_pages::apply_settings(&app, settings);
                        app.set_status("运行时设置已保存".into());
                        app.invoke_navigate_directory("".into());
                    }
                    Err(error) => {
                        app.set_dialog_title("保存失败".into());
                        app.set_dialog_text("配置保存失败，请检查目录权限后重试".into());
                        eprintln!("native settings failed: {error:#}");
                        app.set_dialog_open(true);
                    }
                }
            });
        });
    });
}
