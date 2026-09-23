//! Start the embedded runtime without delaying the first frame.
use slint::ComponentHandle;
use std::sync::Arc;
use wunder_desktop::NativeDesktop;

pub fn install(app: &crate::MainWindow) {
    app.set_conversations(slint::ModelRc::default());
    app.set_messages(slint::ModelRc::default());
    app.set_agents(slint::ModelRc::default());
    app.set_tools(slint::ModelRc::default());
    app.set_models(slint::ModelRc::default());
    app.set_chat_loading(true);
    app.set_status("正在启动内嵌运行时…".into());
    let weak = app.as_weak();
    // SQLite setup and resource discovery must not block the first frame.
    std::thread::spawn(move || {
        let result = NativeDesktop::start().map(Arc::new);
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_chat_loading(false);
            match result {
                Ok(runtime) => install_ready(&app, runtime),
                Err(error) => app.set_status(format!("无法启动内嵌运行时：{error}").into()),
            }
        });
    });
}

pub fn install_ready(app: &crate::MainWindow, runtime: Arc<NativeDesktop>) {
    crate::native_chat::install(app, runtime.clone());
    crate::native_pages::install(app, runtime);
}
