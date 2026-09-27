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
    let menu_api = api.clone();
    app.on_thread_menu(move |index, _action| {
        let Some(app) = weak.upgrade() else { return };
        let Some(row) = app.get_conversations().row_data(index.max(0) as usize) else { return };
        let id = row.id.to_string();
        let title = row.title.to_string();
        let api = menu_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            // The compact menu defaults to details; rename/archive remain
            // available through the same façade for the richer native menu.
            let detail = api.session_detail_page(&id, 0, 100);
            let _ = weak.upgrade_in_event_loop(move |app| {
                match detail {
                    Ok(value) => {
                        app.set_thread_log_title(format!("线程日志 · {title}").into());
                        let session = value.get("session").cloned().unwrap_or_default();
                        app.set_thread_log_status(session.get("status").and_then(serde_json::Value::as_str).unwrap_or("未知").into());
                        app.set_thread_log_elapsed(session.get("elapsed_s").map(|v| format!("{}s", v)).unwrap_or_default().into());
                        app.set_thread_log_rounds(session.get("user_rounds").map(|v| v.to_string()).unwrap_or_default().into());
                        app.set_thread_log_tools(session.get("tool_calls").map(|v| v.to_string()).unwrap_or_default().into());
                        app.set_thread_log_tokens(session.get("consumed_tokens").map(|v| v.to_string()).unwrap_or_default().into());
                        app.set_thread_log_speed(session.get("decode_speed_tps").map(|v| format!("{v}/s")).unwrap_or_default().into());
                        let events = value.get("events").and_then(serde_json::Value::as_array).into_iter().flatten().map(|event| {
                            let data = event.get("data").unwrap_or(event);
                            crate::ThreadLogEvent {
                                event_type: event.get("type").and_then(serde_json::Value::as_str).unwrap_or("event").into(),
                                time: event.get("timestamp").map(|v| v.to_string()).unwrap_or_default().into(),
                                summary: data.get("summary").and_then(serde_json::Value::as_str).unwrap_or("").into(),
                                raw: serde_json::to_string(event).unwrap_or_default().into(),
                            }
                        }).collect::<Vec<_>>();
                        app.set_thread_log_events(ModelRc::new(VecModel::from(events)));
                        app.set_thread_log_open(true);
                    }
                    Err(error) => {
                        app.set_dialog_title("线程日志".into());
                        app.set_dialog_text(format!("无法读取线程日志：{error}").into());
                        app.set_dialog_open(true);
                    }
                }
            });
        });
    });
    let weak = app.as_weak();
    app.on_thread_drop(move |from, to| {
        let Some(app) = weak.upgrade() else { return };
        let mut rows = app.get_conversations().iter().collect::<Vec<_>>();
        let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) else { return };
        if from < rows.len() && to < rows.len() && from != to {
            let row = rows.remove(from);
            rows.insert(to, row);
            app.set_conversations(ModelRc::new(VecModel::from(rows)));
        }
    });
    let weak = app.as_weak();
    let rename_api = api.clone();
    app.on_rename_thread(move |index, title| {
        let Some(app) = weak.upgrade() else { return };
        let Some(row) = app.get_conversations().row_data(index.max(0) as usize) else { return };
        let id = row.id.to_string();
        let api = rename_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.rename_session(&id, &title);
            let _ = weak.upgrade_in_event_loop(move |app| {
                match result {
                    Ok(()) => {
                        if let Some(mut row) = app.get_conversations().row_data(index.max(0) as usize) { row.title = title.into(); app.get_conversations().set_row_data(index.max(0) as usize, row); }
                        app.set_status("工作线程已重命名".into());
                    }
                    Err(error) => app.set_status(format!("无法重命名工作线程：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let archive_api = api.clone();
    app.on_archive_thread(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(row) = app.get_conversations().row_data(index.max(0) as usize) else { return };
        let id = row.id.to_string();
        let api = archive_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.archive_session(&id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                match result {
                    Ok(()) => { app.invoke_refresh_chat(); app.set_status("工作线程已归档".into()); }
                    Err(error) => app.set_status(format!("无法归档工作线程：{error}").into()),
                }
            });
        });
    });
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
    let bind_api = api.clone();
    app.on_save_workspace_binding(move |container, path| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_files_loading() { return; }
        let agent = app.get_active_agent_id().to_string();
        if agent.trim().is_empty() { return; }
        app.set_files_loading(true);
        let api = bind_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.save_workspace_binding(&agent, container, &path);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_files_loading(false);
                match result {
                    Ok(()) => {
                        app.set_directory_path("".into());
                        app.set_files_offset(0);
                        app.set_files(ModelRc::new(VecModel::<FileCard>::default()));
                        app.set_status("工作目录已切换".into());
                        app.invoke_refresh_files();
                    }
                    Err(error) => app.set_status(format!("无法切换工作目录：{error}").into()),
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
