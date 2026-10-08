//! Desktop configuration callback backed by the embedded runtime.
use crate::MainWindow;
use slint::ComponentHandle;
use std::sync::Arc;
use wunder_desktop::NativeDesktop;

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    install_supplement_import(app, api.clone());
    install_tool_path_pickers(app);
    let weak = app.as_weak();
    app.on_save_runtime(move |workspace, language, python, git, rg| {
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
            let result = api.save_runtime(&workspace, &language, &python, &git, &rg);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(settings) => {
                        crate::native_pages::apply_settings(&app, settings);
                        app.set_status("运行时设置已保存".into());
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
                        let summary = report.summary_line();
                        crate::native_pages::apply_settings(&app, report.settings);
                        app.set_supplement_status(summary.into());                        app.set_dialog_title("补充包导入完成".into());
                        app.set_dialog_text(
                            format!(
                                "已解压到 {}\nPython：{}\nGit：{}\nrg：{}\n\n对新启动的工具立即生效。",
                                report.target_dir,
                                non_empty_or(&report.python_path),
                                non_empty_or(&report.git_path),
                                non_empty_or(&report.rg_path),
                            )
                            .into(),
                        );
                        app.set_dialog_open(true);
                    }
                    Err(error) => {
                        app.set_supplement_status(format!("导入失败：{error}").into());
                        app.set_dialog_title("补充包导入失败".into());
                        app.set_dialog_text(format!("{error:#}").into());
                        app.set_dialog_open(true);
                    }
                }
            });
        });
    });
}

fn non_empty_or(value: &str) -> String {
    if value.trim().is_empty() {
        "未检测到".into()
    } else {
        value.to_string()
    }
}

/// Runtime tool path pickers: each handler opens the native single-file
/// dialog and writes the picked executable into the settings draft. The native
/// dialog runs modally on the UI thread; the draft only becomes effective after
/// the save callback persists it, and the status line refreshes from the
/// backend after a load or save.
fn install_tool_path_pickers(app: &MainWindow) {
    let weak = app.as_weak();
    app.on_pick_python_path(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        if let Some(path) = crate::file_dialog::pick_executable("python") {
            app.set_runtime_python(path.into());
        }
    });
    let weak = app.as_weak();
    app.on_pick_git_path(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        if let Some(path) = crate::file_dialog::pick_executable("git") {
            app.set_runtime_git(path.into());
        }
    });
    let weak = app.as_weak();
    app.on_pick_rg_path(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        if let Some(path) = crate::file_dialog::pick_executable("rg") {
            app.set_runtime_rg(path.into());
        }
    });
}
