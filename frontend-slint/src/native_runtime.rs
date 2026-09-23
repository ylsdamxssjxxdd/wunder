//! Native desktop runtime bootstrap.
//!
//! The first migration slice exposes the in-process runtime to the Slint host
//! without changing the existing HTTP-backed pages. Keeping this behind a
//! feature allows the Win7 build to validate the larger dependency closure
//! before switching the default launch path.

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
                Ok(runtime) => crate::native_chat::install(&app, runtime),
                Err(error) => app.set_status(format!("无法启动内嵌运行时：{error}").into()),
            }
        });
    });
}
