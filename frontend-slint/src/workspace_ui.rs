//! Independent, single-flight workspace projection shared by the dock and files page.
use crate::{FileCard, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use wunder_desktop::NativeDesktop;

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let revision = Arc::new(AtomicU64::new(0));
    let weak = app.as_weak();
    let list_api = api.clone();
    app.on_refresh_files(move || {
        let Some(app) = weak.upgrade() else { return };
        revision.fetch_add(1, Ordering::Relaxed);
        if app.get_files_loading() {
            return;
        }
        let revision = revision.clone();
        let requested = revision.load(Ordering::Relaxed);
        let agent = app.get_active_agent_id().to_string();
        let path = app.get_directory_path().to_string();
        let offset = app.get_files_offset();
        app.set_files_loading(true);
        app.set_files_error("".into());
        let weak = app.as_weak();
        let api = list_api.clone();
        let request_agent = agent.clone();
        let request_path = path.clone();
        std::thread::spawn(move || {
            let result = api.workspace_directory(&request_agent, &request_path, offset);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_files_loading(false);
                if revision.load(Ordering::Relaxed) != requested
                    || app.get_active_agent_id() != agent
                    || app.get_directory_path() != path
                    || app.get_files_offset() != offset
                {
                    app.invoke_refresh_files();
                    return;
                }
                match result {
                    Ok(page) => {
                        app.set_directory_path(page.path.into());
                        app.set_directory_parent(page.parent.into());
                        app.set_directory_container_id(page.container_id);
                        app.set_directory_root(page.root.into());
                        app.set_files_total(page.total);
                        app.set_files(ModelRc::new(VecModel::from(
                            page.entries
                                .into_iter()
                                .map(|file| FileCard {
                                    name: file.name.into(),
                                    path: file.path.into(),
                                    entry_type: file.entry_type.into(),
                                    size: file.size.into(),
                                })
                                .collect::<Vec<_>>(),
                        )));
                    }
                    Err(error) => app.set_files_error(format!("无法读取工作目录：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    app.on_navigate_directory(move |path| {
        let Some(app) = weak.upgrade() else { return };
        app.set_directory_path(path);
        app.set_files_offset(0);
        app.set_files(ModelRc::new(VecModel::<FileCard>::default()));
        app.set_files_total(0);
        app.invoke_refresh_files();
    });
    let weak = app.as_weak();
    app.on_open_file(move |path| {
        let Some(app) = weak.upgrade() else { return };
        let entry = app.get_files().iter().find(|file| file.path == path);
        if entry.is_some_and(|file| file.entry_type == "dir") {
            app.invoke_navigate_directory(path);
            return;
        }
        if app.get_preview_loading() {
            return;
        }
        app.set_preview_open(true);
        app.set_preview_loading(true);
        app.set_preview_path(path.clone());
        app.set_preview_text("正在读取…".into());
        let agent = app.get_active_agent_id().to_string();
        let weak = app.as_weak();
        let api = api.clone();
        std::thread::spawn(move || {
            let result = api.workspace_preview(&agent, &path);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_preview_loading(false);
                if app.get_preview_path() == path && app.get_active_agent_id() == agent {
                    app.set_preview_text(
                        result
                            .unwrap_or_else(|error| format!("无法预览：{error}"))
                            .into(),
                    );
                } else {
                    app.set_preview_open(false);
                }
            });
        });
    });
}
