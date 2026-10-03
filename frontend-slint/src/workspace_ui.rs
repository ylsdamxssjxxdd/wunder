//! Independent, single-flight workspace projection shared by the dock and files page.
use crate::{FileCard, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use wunder_desktop::NativeDesktop;

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let thread_log_source: std::rc::Rc<std::cell::RefCell<Vec<crate::ThreadLogEvent>>> =
        Default::default();
    let revision = Arc::new(AtomicU64::new(0));
    let create_api = api.clone();
    let create_weak = app.as_weak();
    app.on_create_workspace_file(move |kind, path| {
        let Some(app) = create_weak.upgrade() else {
            return;
        };
        let agent = app.get_active_agent_id().to_string();
        let base = if path.trim().is_empty() {
            "".to_string()
        } else {
            format!("{}/", path.trim_end_matches('/'))
        };
        let (name, content) = match kind.as_str() {
            "directory" => {
                let relative = if path.trim().is_empty() {
                    "新建目录".to_string()
                } else {
                    format!("{}/新建目录", path.trim_end_matches('/'))
                };
                let api = create_api.clone();
                let weak = app.as_weak();
                let agent = agent.clone();
                std::thread::spawn(move || {
                    let result = api.create_workspace_directory(&agent, &relative);
                    let _ = weak.upgrade_in_event_loop(move |app| match result {
                        Ok(()) => {
                            app.set_status("目录已创建".into());
                            app.invoke_refresh_files();
                        }
                        Err(error) => app.set_status(format!("无法创建目录：{error}").into()),
                    });
                });
                return;
            }
            "markdown" => ("notes.md", "# Title\n"),
            "word" => ("document.docx", ""),
            "sheet" => ("sheet.xlsx", ""),
            "slides" => ("slides.pptx", ""),
            "flowchart" => (
                "flowchart.drawio",
                "<mxfile><diagram name=\"Flowchart\"></diagram></mxfile>",
            ),
            _ => ("untitled.txt", ""),
        };
        let relative = format!("{base}{name}");
        let api = create_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.create_workspace_file(&agent, &relative, content);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => {
                    app.set_status("文件已创建".into());
                    app.invoke_refresh_files();
                }
                Err(error) => app.set_status(format!("无法创建文件：{error}").into()),
            });
        });
    });
    let action_api = api.clone();
    let action_weak = app.as_weak();
    app.on_workspace_action(move |kind, path, value| {
        let Some(app) = action_weak.upgrade() else {
            return;
        };
        if app.get_files_loading() {
            return;
        }
        let agent = app.get_active_agent_id().to_string();
        if agent.trim().is_empty() {
            return;
        }
        app.set_files_loading(true);
        let api = action_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = match kind.as_str() {
                "delete" => api.delete_workspace_entry(&agent, &path),
                "rename" => api
                    .rename_workspace_entry(&agent, &path, &value)
                    .map(|_| ()),
                "copy" => api.copy_workspace_entry(&agent, &path, &value),
                "move" => api.move_workspace_entry(&agent, &path, &value),
                _ => Err(
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "未知文件操作").into(),
                ),
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_files_loading(false);
                match result {
                    Ok(()) => {
                        app.set_status("文件操作已完成".into());
                        app.invoke_refresh_files();
                    }
                    Err(error) => app.set_status(format!("文件操作失败：{error}").into()),
                }
            });
        });
    });
    let save_api = api.clone();
    let save_weak = app.as_weak();
    app.on_save_workspace_text(move |path, content| {
        let Some(app) = save_weak.upgrade() else {
            return;
        };
        if app.get_preview_loading() {
            return;
        }
        let agent = app.get_active_agent_id().to_string();
        app.set_preview_loading(true);
        app.set_preview_editable(false);
        let api = save_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.save_workspace_text(&agent, &path, &content);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_preview_loading(false);
                match result {
                    Ok(()) => {
                        app.set_status("文件已保存".into());
                        app.invoke_refresh_files();
                    }
                    Err(error) => app.set_status(format!("无法保存文件：{error}").into()),
                }
            });
        });
    });
    let filter_source = thread_log_source.clone();
    let filter_weak = app.as_weak();
    app.on_filter_thread_log(move |query, filter| {
        let query = query.to_lowercase();
        let matching = filter_source
            .borrow()
            .iter()
            .filter(|event| {
                let kind = event.event_type.to_lowercase();
                let text_matches = query.is_empty()
                    || kind.contains(&query)
                    || event.summary.to_lowercase().contains(&query)
                    || event.raw.to_lowercase().contains(&query);
                let type_matches = filter == "all"
                    || (filter == "tools" && kind.contains("tool"))
                    || (filter == "models" && (kind.contains("llm") || kind.contains("model")));
                text_matches && type_matches
            })
            .cloned()
            .collect::<Vec<_>>();
        if let Some(app) = filter_weak.upgrade() {
            app.set_thread_log_events(ModelRc::new(VecModel::from(matching)));
        }
    });
    let weak = app.as_weak();
    app.on_choose_workspace_directory(move || {
        let weak = weak.clone();
        std::thread::spawn(move || {
            #[cfg(windows)]
            let selected = choose_windows_directory();
            #[cfg(not(windows))]
            let selected: Option<String> = None;
            if let Some(path) = selected {
                let _ = weak
                    .upgrade_in_event_loop(move |app| app.set_workspace_binding_path(path.into()));
            }
        });
    });
    let weak = app.as_weak();
    let menu_api = api.clone();
    app.on_thread_menu(move |index, _action| {
        let Some(app) = weak.upgrade() else { return };
        let Some(row) = app.get_conversations().row_data(index.max(0) as usize) else {
            return;
        };
        let id = row.id.to_string();
        let title = row.title.to_string();
        let api = menu_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            // The compact menu defaults to details; rename/archive remain
            // available through the same façade for the richer native menu.
            let detail = api.session_detail_page(&id, 0, 100);
            let _ = weak.upgrade_in_event_loop(move |app| match detail {
                Ok(value) => {
                    app.set_thread_log_title(format!("线程日志 · {title}").into());
                    let session = value.get("session").cloned().unwrap_or_default();
                    app.set_thread_log_status(
                        session
                            .get("status")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("未知")
                            .into(),
                    );
                    app.set_thread_log_elapsed(
                        session
                            .get("elapsed_s")
                            .map(|v| format!("{}s", v))
                            .unwrap_or_default()
                            .into(),
                    );
                    app.set_thread_log_rounds(
                        session
                            .get("user_rounds")
                            .map(|v| v.to_string())
                            .unwrap_or_default()
                            .into(),
                    );
                    app.set_thread_log_tools(
                        session
                            .get("tool_calls")
                            .map(|v| v.to_string())
                            .unwrap_or_default()
                            .into(),
                    );
                    app.set_thread_log_tokens(
                        session
                            .get("consumed_tokens")
                            .map(|v| v.to_string())
                            .unwrap_or_default()
                            .into(),
                    );
                    app.set_thread_log_speed(
                        session
                            .get("decode_speed_tps")
                            .map(|v| format!("{v}/s"))
                            .unwrap_or_default()
                            .into(),
                    );
                    let events = value
                        .get("events")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(|event| {
                            let data = event.get("data").unwrap_or(event);
                            crate::ThreadLogEvent {
                                event_type: event
                                    .get("type")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("event")
                                    .into(),
                                time: event
                                    .get("timestamp")
                                    .map(|v| v.to_string())
                                    .unwrap_or_default()
                                    .into(),
                                summary: data
                                    .get("summary")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("")
                                    .into(),
                                raw: serde_json::to_string(event).unwrap_or_default().into(),
                            }
                        })
                        .collect::<Vec<_>>();
                    app.set_thread_log_query("".into());
                    app.set_thread_log_filter("全部".into());
                    app.set_thread_log_events(ModelRc::new(VecModel::from(events)));
                    app.set_thread_log_open(true);
                }
                Err(error) => {
                    app.set_dialog_title("线程日志".into());
                    app.set_dialog_text(format!("无法读取线程日志：{error}").into());
                    app.set_dialog_open(true);
                }
            });
        });
    });
    let weak = app.as_weak();
    app.on_thread_drop(move |from, to| {
        let Some(app) = weak.upgrade() else { return };
        let mut rows = app.get_conversations().iter().collect::<Vec<_>>();
        let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) else {
            return;
        };
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
        let Some(row) = app.get_conversations().row_data(index.max(0) as usize) else {
            return;
        };
        let id = row.id.to_string();
        let api = rename_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.rename_session(&id, &title);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => {
                    if let Some(mut row) = app.get_conversations().row_data(index.max(0) as usize) {
                        row.title = title.into();
                        app.get_conversations()
                            .set_row_data(index.max(0) as usize, row);
                    }
                    app.set_status("工作线程已重命名".into());
                }
                Err(error) => app.set_status(format!("无法重命名工作线程：{error}").into()),
            });
        });
    });
    let weak = app.as_weak();
    let archive_api = api.clone();
    app.on_archive_thread(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(row) = app.get_conversations().row_data(index.max(0) as usize) else {
            return;
        };
        let id = row.id.to_string();
        let api = archive_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.archive_session(&id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => {
                    app.invoke_refresh_chat();
                    app.set_status("工作线程已归档".into());
                }
                Err(error) => app.set_status(format!("无法归档工作线程：{error}").into()),
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
        if app.get_files_loading() {
            return;
        }
        let agent = app.get_active_agent_id().to_string();
        if agent.trim().is_empty() {
            return;
        }
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
    let preview_api = api.clone();
    let native_api = api.clone();
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
        app.set_preview_editing(false);
        app.set_preview_loading(true);
        app.set_preview_path(path.clone());
        app.set_preview_text("正在读取…".into());
        let agent = app.get_active_agent_id().to_string();
        let weak = app.as_weak();
        let api = preview_api.clone();
        std::thread::spawn(move || {
            let result = api.workspace_preview_detail(&agent, &path);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_preview_loading(false);
                if app.get_preview_path() == path && app.get_active_agent_id() == agent {
                    match result {
                        Ok(preview) => {
                            app.set_preview_editable(preview.editable);
                            app.set_preview_text(preview.text.into());
                        }
                        Err(error) => app.set_preview_text(format!("无法预览：{error}").into()),
                    }
                } else {
                    app.set_preview_open(false);
                }
            });
        });
    });
    let weak = app.as_weak();
    let upload_api = api.clone();
    app.on_upload_workspace_file(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if app.get_files_loading() {
            return;
        }
        let agent = app.get_active_agent_id().to_string();
        if agent.trim().is_empty() {
            app.set_status("请先选择智能体".into());
            return;
        }
        // Local-link transfer: the source path comes from the system file
        // dialog and the façade copies it directly into the container.
        let directory = app.get_directory_path().to_string();
        app.set_files_loading(true);
        let weak = app.as_weak();
        let api = upload_api.clone();
        std::thread::spawn(move || {
            let outcome = pick_windows_file("选择要导入的文件").map(|source| {
                let name = std::path::Path::new(&source)
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let destination = if directory.trim().is_empty() {
                    name
                } else {
                    format!("{}/{}", directory.trim_end_matches('/'), name)
                };
                (source, destination)
            });
            let result = match outcome {
                Some((source, destination)) => api
                    .import_workspace_file(&agent, &source, &destination)
                    .map(|_| destination),
                None => Err(anyhow::anyhow!("已取消选择")),
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_files_loading(false);
                match result {
                    Ok(destination) => {
                        app.set_status(format!("已导入 {destination}").into());
                        app.invoke_refresh_files();
                    }
                    Err(error) if error.to_string() == "已取消选择" => {}
                    Err(error) => app.set_status(format!("导入失败：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let download_api = api.clone();
    app.on_download_workspace_file(move |path, name| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if app.get_files_loading() {
            return;
        }
        let agent = app.get_active_agent_id().to_string();
        if agent.trim().is_empty() {
            return;
        }
        app.set_files_loading(true);
        let weak = app.as_weak();
        let api = download_api.clone();
        let name = name.to_string();
        std::thread::spawn(move || {
            let result = match save_windows_file("保存到本机", &name) {
                Some(target) => {
                    api.export_workspace_file(&agent, &path, &target)
                        .map(|_| target)
                }
                None => Err(anyhow::anyhow!("已取消选择")),
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_files_loading(false);
                match result {
                    Ok(target) => app.set_status(format!("已导出到 {target}").into()),
                    Err(error) if error.to_string() == "已取消选择" => {}
                    Err(error) => app.set_status(format!("导出失败：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    app.on_open_file_native(move |path| {
        let Some(app) = weak.upgrade() else { return };
        let agent = app.get_active_agent_id().to_string();
        let weak = app.as_weak();
        let api = native_api.clone();
        std::thread::spawn(move || {
            let result = api.open_workspace_file(&agent, &path);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Err(error) = result {
                    app.set_status(format!("无法使用系统程序打开：{error}").into());
                }
            });
        });
    });
}

#[cfg(windows)]
fn choose_windows_directory() -> Option<String> {
    use std::{ffi::c_void, ptr};
    #[repr(C)]
    struct BrowseInfoW {
        hwnd_owner: isize,
        pidl_root: *mut c_void,
        display_name: *mut u16,
        title: *const u16,
        flags: u32,
        callback: *const c_void,
        param: isize,
        image: i32,
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHBrowseForFolderW(info: *const BrowseInfoW) -> *mut c_void;
        fn SHGetPathFromIDListW(pidl: *const c_void, path: *mut u16) -> i32;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(value: *const c_void);
    }
    const BIF_RETURNONLYFSDIRS: u32 = 0x0001;
    const BIF_NEWDIALOGSTYLE: u32 = 0x0040;
    let title: Vec<u16> = "选择工作目录\0".encode_utf16().collect();
    let mut display = [0u16; 260];
    let info = BrowseInfoW {
        hwnd_owner: 0,
        pidl_root: ptr::null_mut(),
        display_name: display.as_mut_ptr(),
        title: title.as_ptr(),
        flags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        callback: ptr::null(),
        param: 0,
        image: 0,
    };
    let pidl = unsafe { SHBrowseForFolderW(&info) };
    if pidl.is_null() {
        return None;
    }
    let mut path = [0u16; 260];
    let ok = unsafe { SHGetPathFromIDListW(pidl, path.as_mut_ptr()) } != 0;
    unsafe {
        CoTaskMemFree(pidl);
    }
    if !ok {
        return None;
    }
    let end = path.iter().position(|v| *v == 0).unwrap_or(path.len());
    Some(String::from_utf16_lossy(&path[..end]))
}

/// System open-file dialog. Desktop file transfer stays on the local link:
/// the dialog only yields a host path and the façade copies from disk.
#[cfg(windows)]
fn pick_windows_file(title: &str) -> Option<String> {
    file_windows_dialog(title, "", 0x0000_1804, false) // OFN_HIDEREADONLY|FILEMUSTEXIST|PATHMUSTEXIST
}

/// System save-as dialog seeded with the workspace file name.
#[cfg(windows)]
fn save_windows_file(title: &str, default_name: &str) -> Option<String> {
    file_windows_dialog(title, default_name, 0x0000_0802, true) // OFN_OVERWRITEPROMPT|PATHMUSTEXIST
}

#[cfg(windows)]
fn file_windows_dialog(title: &str, default_name: &str, flags: u32, save: bool) -> Option<String> {
    use std::{ffi::c_void, ptr};
    #[repr(C)]
    struct OpenFileNameW {
        struct_size: u32,
        owner: isize,
        instance: isize,
        filter: *const u16,
        custom_filter: *mut u16,
        max_custom_filter: u32,
        filter_index: u32,
        file: *mut u16,
        max_file: u32,
        file_title: *mut u16,
        max_file_title: u32,
        initial_dir: *const u16,
        title_ptr: *const u16,
        flags: u32,
        file_offset: u16,
        file_extension: u16,
        default_ext: *const u16,
        cust_data: isize,
        hook: isize,
        template_name: *const u16,
        reserved_ptr: *mut c_void,
        reserved_u32: u32,
        flags_ex: u32,
    }
    #[link(name = "comdlg32")]
    unsafe extern "system" {
        fn GetOpenFileNameW(info: *mut OpenFileNameW) -> i32;
        fn GetSaveFileNameW(info: *mut OpenFileNameW) -> i32;
    }
    const OFN_ALLOWMULTISELECT_UNUSED: u32 = 0;
    let _ = OFN_ALLOWMULTISELECT_UNUSED;
    let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buffer = [0u16; 1024];
    let seed = default_name.trim();
    for (index, unit) in seed.encode_utf16().take(buffer.len() - 1).enumerate() {
        buffer[index] = unit;
    }
    let mut info = OpenFileNameW {
        struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
        owner: 0,
        instance: 0,
        filter: ptr::null(),
        custom_filter: ptr::null_mut(),
        max_custom_filter: 0,
        filter_index: 0,
        file: buffer.as_mut_ptr(),
        max_file: buffer.len() as u32,
        file_title: ptr::null_mut(),
        max_file_title: 0,
        initial_dir: ptr::null(),
        title_ptr: title_wide.as_ptr(),
        flags,
        file_offset: 0,
        file_extension: 0,
        default_ext: ptr::null(),
        cust_data: 0,
        hook: 0,
        template_name: ptr::null(),
        reserved_ptr: ptr::null_mut(),
        reserved_u32: 0,
        flags_ex: 0,
    };
    let ok = if save {
        unsafe { GetSaveFileNameW(&mut info) }
    } else {
        unsafe { GetOpenFileNameW(&mut info) }
    } != 0;
    if !ok {
        return None;
    }
    let end = buffer.iter().position(|v| *v == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

#[cfg(not(windows))]
fn pick_windows_file(_title: &str) -> Option<String> {
    None
}

#[cfg(not(windows))]
fn save_windows_file(_title: &str, _default_name: &str) -> Option<String> {
    None
}
