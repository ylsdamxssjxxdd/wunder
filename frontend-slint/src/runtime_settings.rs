//! Desktop configuration callback backed by the embedded runtime.
use crate::MainWindow;
use slint::ComponentHandle;
use std::sync::Arc;
use wunder_desktop::NativeDesktop;

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    install_supplement_import(app, api.clone());
    install_tool_path_pickers(app, api.clone());
    let weak = app.as_weak();
    app.on_save_runtime(move || {
        let Some(app) = weak.upgrade() else { return };
        persist(&app, &api);
    });
}

/// Write the shell's current runtime drafts back to the configuration. The
/// settings page has no submit button: a picked or typed executable path is
/// effective on its own, so every entry point funnels here.
fn persist(app: &MainWindow, api: &Arc<NativeDesktop>) {
    if app.get_saving() || app.get_settings_loading() {
        return;
    }
    let workspace = app.get_workspace_root().to_string();
    if workspace.trim().is_empty()
        || !matches!(app.get_runtime_language().as_str(), "zh-CN" | "en-US")
    {
        app.set_status("请填写工作目录和有效的运行时语言".into());
        return;
    }
    app.set_saving(true);
    let language = app.get_runtime_language().to_string();
    let python = app.get_runtime_python().to_string();
    let git = app.get_runtime_git().to_string();
    let rg = app.get_runtime_rg().to_string();
    let ffmpeg = app.get_runtime_ffmpeg().to_string();
    let browser = app.get_runtime_browser().to_string();
    let shell = app.get_runtime_shell().to_string();
    let api = api.clone();
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result =
            api.save_runtime(&workspace, &language, &python, &git, &rg, &ffmpeg, &browser, &shell);
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_saving(false);
            match result {
                Ok(settings) => {
                    crate::native_pages::apply_settings(&app, settings);
                    app.set_status("运行时设置已保存".into());
                }
                Err(error) => {
                    eprintln!("native settings failed: {error:#}");
                    crate::native_pages::show_error(
                        &app,
                        "配置保存失败，请检查目录权限后重试".to_string(),
                    );
                }
            }
        });
    });
}

/// Supplement import: the native file dialog runs on the UI thread (modal is
/// expected), extraction and environment refresh run on a worker thread.
fn install_supplement_import(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    app.on_import_supplement(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_importing_supplement() || app.get_saving() || app.get_settings_loading() {
            return;
        }
        let Some(archive) = crate::file_dialog::pick_supplement_archive() else {
            return;
        };
        app.set_importing_supplement(true);
        app.set_supplement_status("正在导入补充包…".into());
        let api = api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.import_supplement(&archive);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_importing_supplement(false);
                match result {
                    Ok(report) => {
                        crate::native_pages::apply_settings(&app, report.settings);
                        // Imported binaries are immediately visible in the
                        // editable fields; the status line remains concise.
                        if !report.python_path.trim().is_empty() {
                            app.set_runtime_python(report.python_path.clone().into());
                        }
                        if !report.git_path.trim().is_empty() {
                            app.set_runtime_git(report.git_path.clone().into());
                        }
                        if !report.rg_path.trim().is_empty() {
                            app.set_runtime_rg(report.rg_path.clone().into());
                        }
                        // The editable fields are the source of truth after an
                        // import; do not repeat the paths in a status line.
                        app.set_supplement_status("".into());
                        app.set_status("补充包导入完成，运行时路径已更新".into());
                    }
                    Err(error) => {
                        app.set_supplement_status(format!("导入失败：{error}").into());
                        app.set_status(format!("补充包导入失败：{error}").into());
                    }
                }
            });
        });
    });
}

/// Runtime tool path pickers: each handler opens the native single-file
/// dialog and writes the picked executable into the settings draft. The native
/// dialog runs modally on the UI thread, and the picked path is persisted and
/// re-projected immediately - the settings page has no submit button.
fn install_tool_path_pickers(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let persist_api = api.clone();
    app.on_pick_python_path(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        if let Some(path) = crate::file_dialog::pick_executable("python") {
            app.set_runtime_python(path.into());
            persist(&app, &persist_api);
        }
    });
    let weak = app.as_weak();
    let persist_api = api.clone();
    app.on_pick_git_path(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        if let Some(path) = crate::file_dialog::pick_executable("git") {
            app.set_runtime_git(path.into());
            persist(&app, &persist_api);
        }
    });
    let weak = app.as_weak();
    let persist_api = api.clone();
    app.on_pick_rg_path(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        if let Some(path) = crate::file_dialog::pick_executable("rg") {
            app.set_runtime_rg(path.into());
            persist(&app, &persist_api);
        }
    });
    let weak = app.as_weak();
    let persist_api = api.clone();
    app.on_pick_ffmpeg_path(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        if let Some(path) = crate::file_dialog::pick_executable("ffmpeg") {
            app.set_runtime_ffmpeg(path.into());
            persist(&app, &persist_api);
        }
    });
    let weak = app.as_weak();
    let persist_api = api.clone();
    app.on_pick_browser_path(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        if let Some(path) = crate::file_dialog::pick_executable("browser") {
            app.set_runtime_browser(path.into());
            persist(&app, &persist_api);
        }
    });
    let weak = app.as_weak();
    let persist_api = api.clone();
    app.on_pick_shell_path(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        if let Some(path) = crate::file_dialog::pick_executable("shell") {
            app.set_runtime_shell(path.into());
            persist(&app, &persist_api);
        }
    });
}
