//! Sidebar working-directory region: one bounded, lazy file tree. Every
//! cloud round trip runs on a background thread through the synchronous
//! interlink façade and lands via `upgrade_in_event_loop`. The tree shares the
//! web left-rail's semantics: lazy directories, 200-entry pages with a
//! load-more tail, dirs-first natural order, click-to-select and double-click
//! to open with the OS default application.
//! A locally bound workspace folder mirrors the same tree from the disk with
//! the same menu (minus the cloud-only download/archive/clear trio) and
//! executes its writes directly through the `workspace_local_*` façade.
use crate::{MainWindow, WorkspaceFileRow};
use anyhow::Result;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use wunder_desktop::native::{CloudOpOutcome, NativeInterlinkEntry, NativeInterlinkListing};
use wunder_desktop::NativeDesktop;

/// Expanded directories reloaded by one refresh; a bound keeps the fan-out
/// finite even if a user expands a wide tree before hitting refresh.
const REFRESH_DIR_CAP: usize = 12;
const MAX_REQUESTS: u32 = 4;
const MAX_CACHED_DIRS: usize = 64;
const MAX_DIRECTORY_ENTRIES: usize = 2000;
const MAX_CACHED_ENTRIES: usize = 8000;

/// Context-menu action codes shared with the .slint side (aligned with the
/// web left-rail menu: open/preview, download, rename, delete, reference,
/// new dir, new file, download-as-zip, clear; refresh is wired inside the
/// panel header itself).
const MENU_OPEN: i32 = 0;
const MENU_DOWNLOAD: i32 = 1;
const MENU_RENAME: i32 = 2;
const MENU_DELETE: i32 = 3;
const MENU_REFERENCE: i32 = 4;
const MENU_NEW_DIR: i32 = 5;
const MENU_NEW_FILE: i32 = 6;
/// 打包下载 / 下载当前目录: zip one cloud directory (blank area = the root).
const MENU_ARCHIVE: i32 = 7;
/// 清空: drop everything under the shown root (blank area only).
const MENU_CLEAR: i32 = 8;

/// Drag-in uploads: one drop stages at most this many text files, each
/// bounded by the interlink inline cap (1 MiB, binary rejected).
const MAX_DROP_FILES: usize = 20;
const MAX_DROP_BYTES: u64 = 1024 * 1024;

/// Kind of the parked prompt: which cloud write the dialog confirms.
#[derive(Clone, Copy, PartialEq)]
enum PromptKind {
    Rename,
    Delete,
    NewDir,
    NewFile,
    Clear,
}

/// One parked prompt: the entry it acts on plus, once the cloud parks the
/// command for approval, the command id awaiting the desktop's decision.
struct PromptOp {
    kind: PromptKind,
    path: String,
    command_id: Option<String>,
    generation: u64,
    target: Target,
}

#[derive(Default)]
struct DirState {
    entries: Vec<NativeInterlinkEntry>,
    total: u64,
    /// At least one page arrived: expansion is pure projection until the
    /// user asks for more.
    loaded: bool,
    expanded: bool,
    loading: bool,
}

struct Panel {
    model: Rc<VecModel<WorkspaceFileRow>>,
    /// Page cache keyed by directory path; the root lives under `""`.
    dirs: HashMap<String, DirState>,
    in_flight: u32,
    refresh_pending: bool,
}

thread_local! {
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
    /// One-shot boot guard: the first connected tick loads root + stats;
    /// disconnecting re-arms it so signing back in reloads.
    static BOOTED: Cell<bool> = const { Cell::new(false) };
    static BUSY: Cell<u32> = const { Cell::new(0) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
    /// The prompt currently on screen, if any; drives both dialog stages.
    static PROMPT: RefCell<Option<PromptOp>> = const { RefCell::new(None) };
    /// One OS drag at a time: DoDragDrop is modal on its worker thread.
    static DRAG_OUT: Cell<bool> = const { Cell::new(false) };
}

/// Which workspace the file panel mirrors: the cloud account workspace
/// (default) or one locally bound workspace folder, keyed by its root path.
/// Switching targets resets the page cache so no stale row leaks across.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    Cloud,
    Local(String),
}

thread_local! {
    static TARGET: RefCell<Target> = const { RefCell::new(Target::Cloud) };
}

fn current_target() -> Target {
    TARGET.with(|slot| slot.borrow().clone())
}

fn generation() -> u64 {
    GENERATION.with(Cell::get)
}
fn is_current(request: u64) -> bool {
    request == generation()
}

/// True while the panel mirrors the cloud account workspace.
pub(crate) fn is_cloud_target() -> bool {
    matches!(current_target(), Target::Cloud)
}

/// Point the working-directory region at the cloud workspace or a local
/// workspace folder, then reload it. Called from the sidebar row so the panel
/// always follows the picked source; a no-op when the target is unchanged.
pub(crate) fn set_target(app: &MainWindow, api: Arc<NativeDesktop>, target: Target) {
    let switched = TARGET.with(|slot| {
        let mut current = slot.borrow_mut();
        if *current == target {
            false
        } else {
            *current = target;
            true
        }
    });
    if !switched {
        return;
    }
    reflect_target(app);
    match current_target() {
        Target::Cloud => refresh(app, api),
        Target::Local(_) => load_dir(&app.as_weak(), api, String::new(), 0),
    }
}

/// Coalesced refresh after an execution writes files; no per-token scans.
pub(crate) fn execution_finished(app: &MainWindow, api: Arc<NativeDesktop>) {
    if !is_cloud_target() {
        refresh(app, api);
    }
}

/// Push the target's chrome: the local flag swaps cloud-only actions out, the
/// offline hint (a local folder needs no tunnel) and a cleared usage line.
fn reflect_target(app: &MainWindow) {
    let cloud = is_cloud_target();
    app.set_selected_workspace(match current_target() {
        Target::Cloud => -1,
        Target::Local(root) => app
            .get_workspaces()
            .iter()
            .position(|card| card.root_path == root)
            .map_or(-1, |index| index as i32),
    });
    app.set_ws_file_local(!cloud);
    app.set_ws_file_stats("".into());
    app.set_ws_file_error("".into());
    if !cloud {
        app.set_ws_file_offline(false);
    }
    // Drop the cached pages so the next listing starts from a clean root.
    GENERATION.with(|slot| slot.set(slot.get().wrapping_add(1)));
    with_panel(|panel| {
        panel.dirs.clear();
        panel.model.set_vec(Vec::new());
        panel.in_flight = 0;
        panel.refresh_pending = false;
    });
    app.set_ws_file_active_path("".into());
    app.set_ws_file_active_index(-1);
    app.set_ws_file_busy(false);
    app.set_ws_file_loading(true);
    app.set_ws_prompt_open(false);
    app.set_ws_prompt_busy(false);
    PROMPT.with(|slot| *slot.borrow_mut() = None);
    BOOTED.with(|slot| slot.set(true));
    crate::navigation_ui::project_sidebar(app, None);
}

/// List one directory under the active target: the cloud workspace over the
/// interlink tunnel, or a workspace folder straight off the local disk.
fn list_dir(
    api: &Arc<NativeDesktop>,
    target: &Target,
    path: &str,
    offset: u64,
) -> Result<NativeInterlinkListing> {
    match target {
        Target::Cloud => api.interlink_cloud_listing(path, offset),
        Target::Local(root) => api.workspace_local_listing(root, path, offset),
    }
}

/// The error prefix for a failed root read, matching the active target.
fn read_error() -> &'static str {
    if is_cloud_target() {
        "无法读取云端工作目录"
    } else {
        "无法读取工作区目录"
    }
}

fn with_panel<R>(run: impl FnOnce(&mut Panel) -> R) -> Option<R> {
    PANEL.with(|slot| {
        let mut slot = slot.borrow_mut();
        let panel = slot.get_or_insert_with(|| Panel {
            model: Rc::new(VecModel::default()),
            dirs: HashMap::new(),
            in_flight: 0,
            refresh_pending: false,
        });
        Some(run(panel))
    })
}

/// Drop the page cache of one directory and everything below it (rename and
/// delete leave the old path behind; a refresh would fail to re-list it).
fn drop_dir_cache(path: &str) {
    if path.is_empty() {
        return;
    }
    let prefix = format!("{path}/");
    with_panel(|panel| {
        panel
            .dirs
            .retain(|key, _| key != path && !key.starts_with(&prefix));
    });
}

/// The model must be bound before the first poll tick can push rows into it,
/// so this runs before `native_interlink::install` starts its timer.
pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let model = with_panel(|panel| panel.model.clone()).expect("panel model");
    app.set_ws_file_rows(ModelRc::from(model));

    // Row click: a directory expands or collapses (lazy load on first open),
    // the load-more tail pages its directory, a file becomes selected.
    let weak = app.as_weak();
    let toggle_api = api.clone();
    app.on_ws_file_toggle(move |index| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let Some(row) = app.get_ws_file_rows().row_data(index as usize) else {
            return;
        };
        let path = row.path.to_string();
        app.set_ws_file_active_path(row.path.clone());
        app.set_ws_file_active_index(index);
        match row.kind {
            0 => {}
            1 => toggle_dir(&app, toggle_api.clone(), path),
            _ => load_more(&app, toggle_api.clone(), path),
        }
    });

    let weak = app.as_weak();
    let key_api = api.clone();
    app.on_ws_file_key(move |key| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if key == 9 {
            refresh(&app, key_api.clone());
            return;
        }
        let rows = app.get_ws_file_rows();
        let count = rows.row_count();
        if count == 0 {
            return;
        }
        let active = usize::try_from(app.get_ws_file_active_index())
            .ok()
            .filter(|&index| index < count);
        let index = match key {
            0 => active.unwrap_or(1).saturating_sub(1),
            1 => active.map_or(0, |i| (i + 1).min(count - 1)),
            4 => 0,
            5 => count - 1,
            _ => active.unwrap_or(0),
        };
        let Some(row) = rows.row_data(index) else {
            return;
        };
        app.set_ws_file_active_path(row.path.clone());
        app.set_ws_file_active_index(index as i32);
        match key {
            2 if row.kind == 1 && row.expanded => {
                toggle_dir(&app, key_api.clone(), row.path.to_string())
            }
            2 => {
                let parent = parent_cloud_dir(&row.path);
                if let Some(i) = rows
                    .iter()
                    .position(|candidate| candidate.path == parent && candidate.kind == 1)
                {
                    app.set_ws_file_active_path(parent.into());
                    app.set_ws_file_active_index(i as i32);
                }
            }
            3 if row.kind == 1 && !row.expanded => {
                toggle_dir(&app, key_api.clone(), row.path.to_string())
            }
            6 => app.invoke_ws_file_menu(index as i32, MENU_OPEN),
            7 if row.kind != 2 => app.invoke_ws_file_menu(index as i32, MENU_RENAME),
            8 if row.kind != 2 => app.invoke_ws_file_menu(index as i32, MENU_DELETE),
            _ => {}
        }
    });

    // Refresh: usage line, root, and every expanded directory in one bounded
    // background pass.
    let refresh_api = api.clone();
    let weak = app.as_weak();
    app.on_ws_file_refresh(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        refresh(&app, refresh_api.clone());
    });

    // Context menu (aligned with the web left-rail). `index` is -1 for the
    // blank-area entries (new entries use the selected directory); actions and
    // the destructive confirm dialog ride the shared prompt. Local folders
    // run the same menu minus the cloud-only download/archive/clear trio —
    // their writes execute straight off the disk in the prompt flow.
    let weak = app.as_weak();
    let menu_api = api.clone();
    app.on_ws_file_menu(move |index, action| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let row = (index >= 0)
            .then(|| app.get_ws_file_rows().row_data(index as usize))
            .flatten();
        if (app.get_ws_file_busy() || app.get_ws_prompt_open())
            && matches!(
                action,
                MENU_RENAME | MENU_DELETE | MENU_NEW_DIR | MENU_NEW_FILE | MENU_CLEAR
            )
        {
            return;
        }
        match action {
            MENU_OPEN => {
                if let Some(row) = row {
                    match row.kind {
                        0 => open_file(
                            &app,
                            menu_api.clone(),
                            row.path.to_string(),
                            row.name.to_string(),
                        ),
                        1 => toggle_dir(&app, menu_api.clone(), row.path.to_string()),
                        _ => load_more(&app, menu_api.clone(), row.path.to_string()),
                    }
                }
            }
            MENU_DOWNLOAD => {
                if !is_cloud_target() {
                    return;
                }
                if let Some(row) = row.filter(|row| row.kind == 0) {
                    download_file(
                        &app,
                        menu_api.clone(),
                        row.path.to_string(),
                        row.name.to_string(),
                    );
                }
            }
            MENU_RENAME => {
                if let Some(row) = row {
                    open_prompt(
                        &app,
                        PromptKind::Rename,
                        row.path.to_string(),
                        row.name.to_string(),
                    );
                }
            }
            MENU_DELETE => {
                if let Some(row) = row {
                    open_prompt(
                        &app,
                        PromptKind::Delete,
                        row.path.to_string(),
                        row.name.to_string(),
                    );
                }
            }
            MENU_REFERENCE => {
                if let Some(row) = row {
                    // 引用到聊天：把路径追加进草稿，模型可按路径取用。
                    let draft = app.get_draft();
                    let reference = match current_target() {
                        Target::Local(root) => std::path::Path::new(&root)
                            .join(row.path.as_str())
                            .to_string_lossy()
                            .into_owned(),
                        Target::Cloud => row.path.to_string(),
                    };
                    let separator = if draft.is_empty() || draft.ends_with(char::is_whitespace) {
                        ""
                    } else {
                        "\n"
                    };
                    app.set_draft(format!("{draft}{separator}@\"{reference}\" ").into());
                }
            }
            MENU_NEW_DIR => {
                let dir = row
                    .as_ref()
                    .map(|row| target_dir(&row.path, row.kind))
                    .unwrap_or_else(|| active_directory(&app));
                open_prompt(&app, PromptKind::NewDir, dir, String::new());
            }
            MENU_NEW_FILE => {
                let dir = row
                    .as_ref()
                    .map(|row| target_dir(&row.path, row.kind))
                    .unwrap_or_else(|| active_directory(&app));
                open_prompt(&app, PromptKind::NewFile, dir, String::new());
            }
            MENU_ARCHIVE => {
                // 打包下载：目录行打整棵子树，空白区打当前根（云端专属）。
                if !is_cloud_target() {
                    return;
                }
                let (path, name) = match row.as_ref() {
                    Some(row) if row.kind == 1 => (row.path.to_string(), row.name.to_string()),
                    _ => (String::new(), "workspace".to_string()),
                };
                archive_dir(&app, menu_api.clone(), path, name);
            }
            MENU_CLEAR => {
                // 清空只作用于云端工作目录根（对齐蜂巢空白区菜单，云端专属）。
                if !is_cloud_target() {
                    return;
                }
                if row.is_none() {
                    open_prompt(&app, PromptKind::Clear, String::new(), String::new());
                }
            }
            9 => {
                if let Target::Local(root) = current_target() {
                    let api = menu_api.clone();
                    let weak = app.as_weak();
                    std::thread::spawn(move || {
                        let result = api.open_workspace_resource(&root, "");
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            if let Err(error) = result {
                                app.set_status(format!("打开失败：{error}").into());
                            }
                        });
                    });
                }
            }
            _ => {}
        }
    });

    // Drag-out: a cloud file leaves the tree as a local OS copy drag. The
    // staged pull happens on the worker; the gesture itself is modal there.
    let weak = app.as_weak();
    let drag_api = api.clone();
    app.on_ws_file_drag(move |index| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let Some(row) = app.get_ws_file_rows().row_data(index as usize) else {
            return;
        };
        if row.kind != 0 {
            return;
        }
        if DRAG_OUT.get() {
            return;
        }
        DRAG_OUT.set(true);
        let path = row.path.to_string();
        let name = row.name.to_string();
        let weak = app.as_weak();
        let worker_api = drag_api.clone();
        let target = current_target();
        let request = generation();
        std::thread::spawn(move || {
            let outcome = drag_out(&worker_api, &target, &path, &name);
            let _ = weak.upgrade_in_event_loop(move |_app| {
                DRAG_OUT.set(false);
                if !is_current(request) {
                    return;
                }
                let _ = outcome;
            });
        });
    });

    // Drag-in: Explorer drops land inside the active target — text uploads
    // into the selected cloud directory (the gesture approves
    // the write), or a plain copy into the browsed local folder. A drop
    // carrying nothing uploadable simply does nothing here.
    let weak = app.as_weak();
    let drop_api = api.clone();
    app.on_ws_file_drop(move |event| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let local = !is_cloud_target();
        if app.get_ws_file_busy() {
            return;
        }
        if !local && app.get_ws_file_offline() {
            return;
        }
        let paths: Vec<PathBuf> = match event.data.file_paths() {
            Ok(iter) => iter
                .map(std::path::Path::to_path_buf)
                .take(MAX_DROP_FILES)
                .collect(),
            Err(_) => Vec::new(),
        };
        if paths.is_empty() {
            app.set_status(if local {
                "落点没有可复制的文件".into()
            } else {
                "落点没有可上传的文件（仅支持文本文件）".into()
            });
            return;
        }
        let count = paths.len();
        app.set_status(if local {
            format!("正在复制 {count} 个文件…").into()
        } else {
            format!("正在上传 {count} 个文件…").into()
        });
        let Some(request) = begin_busy(&app) else {
            return;
        };
        let directory = active_directory(&app);
        let weak = app.as_weak();
        let worker_api = drop_api.clone();
        let target = current_target();
        std::thread::spawn(move || {
            let (done, failed) = match &target {
                Target::Local(root) => copy_files(&worker_api, root, &directory, &paths),
                Target::Cloud => upload_files(&worker_api, &directory, &paths),
            };
            let refresh_api = worker_api;
            let _ = weak.upgrade_in_event_loop(move |app| {
                end_busy(&app, refresh_api.clone(), request);
                if !is_current(request) {
                    return;
                }
                refresh(&app, refresh_api);
                let mut message = format!(
                    "已{verb} {done}/{count} 个文件",
                    verb = if local { "复制" } else { "上传" }
                );
                if !failed.is_empty() {
                    message.push_str(&format!("；失败：{}", failed.join("；")));
                }
                app.set_status(message.into());
            });
        });
    });

    // Prompt dialog: one callback serves both stages (issue the cloud write,
    // then confirm the parked command) plus the destructive delete.
    let weak = app.as_weak();
    let prompt_api = api.clone();
    app.on_ws_prompt_accepted(move |value| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let Some(op) = PROMPT.with(|slot| slot.borrow_mut().take()) else {
            app.set_ws_prompt_open(false);
            return;
        };
        if app.get_ws_prompt_busy() {
            PROMPT.with(|slot| *slot.borrow_mut() = Some(op));
            return;
        }
        if !is_current(op.generation) || op.target != current_target() {
            return;
        }
        let value = value.trim().to_string();
        if op.command_id.is_none()
            && matches!(
                op.kind,
                PromptKind::Rename | PromptKind::NewDir | PromptKind::NewFile
            )
            && !valid_name(&value)
        {
            PROMPT.with(|slot| *slot.borrow_mut() = Some(op));
            app.set_ws_prompt_body(
                "请输入有效名称，不能包含路径分隔符或特殊字符（最多 120 字）".into(),
            );
            return;
        }
        if op.kind == PromptKind::Clear {
            clear_root(&app, prompt_api.clone());
            app.set_ws_prompt_open(false);
            return;
        }
        let request = op.generation;
        let target = op.target.clone();
        if begin_busy(&app).is_none() {
            PROMPT.with(|slot| *slot.borrow_mut() = Some(op));
            app.set_ws_prompt_body("操作正在处理中，请稍后重试".into());
            return;
        }
        // A local workspace folder writes straight off the disk: no parked
        // command, no approval stage — validate, run, refresh.
        if let Target::Local(root) = &target {
            app.set_ws_prompt_busy(true);
            let kind = op.kind;
            let path = op.path.clone();
            let weak = app.as_weak();
            let local_api = prompt_api.clone();
            let refresh_api = prompt_api.clone();
            let root = root.clone();
            std::thread::spawn(move || {
                let result = match kind {
                    PromptKind::Rename => local_api.workspace_local_rename(&root, &path, &value),
                    PromptKind::Delete => local_api.workspace_local_delete(&root, &path),
                    PromptKind::NewDir => {
                        local_api.workspace_local_mkdir(&root, &join_cloud(&path, &value))
                    }
                    PromptKind::NewFile => {
                        local_api.workspace_local_write(&root, &join_cloud(&path, &value))
                    }
                    PromptKind::Clear => unreachable!(),
                };
                let _ = weak.upgrade_in_event_loop(move |app| {
                    end_busy(&app, refresh_api.clone(), request);
                    if !is_current(request) {
                        return;
                    }
                    match result {
                        Ok(()) => {
                            // A renamed or removed directory leaves stale page
                            // cache entries behind; drop them before the refresh
                            // re-lists the expanded set.
                            match kind {
                                PromptKind::Rename | PromptKind::Delete => drop_dir_cache(&path),
                                _ => {}
                            }
                            app.set_ws_prompt_open(false);
                            PROMPT.with(|slot| *slot.borrow_mut() = None);
                            app.set_status(done_message(kind, &value).into());
                            refresh(&app, refresh_api);
                        }
                        Err(error) => {
                            app.set_ws_prompt_busy(false);
                            PROMPT.with(|slot| {
                                *slot.borrow_mut() = Some(PromptOp {
                                    kind,
                                    path,
                                    command_id: None,
                                    generation: request,
                                    target,
                                })
                            });
                            app.set_ws_prompt_body(format!("操作失败：{error}").into());
                        }
                    }
                });
            });
            return;
        }
        // Stage two: the cloud parked the command; this accept confirms it.
        if let Some(command_id) = op.command_id.clone() {
            app.set_ws_prompt_busy(true);
            let weak = app.as_weak();
            let confirm_api = prompt_api.clone();
            let refresh_api = prompt_api.clone();
            std::thread::spawn(move || {
                let result = confirm_api.interlink_cloud_confirm(&command_id);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    end_busy(&app, refresh_api.clone(), request);
                    if !is_current(request) {
                        return;
                    }
                    app.set_ws_prompt_busy(false);
                    match result {
                        Ok(CloudOpOutcome::Done(_)) => {
                            if matches!(op.kind, PromptKind::Rename | PromptKind::Delete) {
                                drop_dir_cache(&op.path);
                            }
                            app.set_ws_prompt_open(false);
                            PROMPT.with(|slot| *slot.borrow_mut() = None);
                            app.set_status("云端已执行".into());
                            refresh(&app, refresh_api);
                        }
                        Ok(CloudOpOutcome::NeedsApproval { command_id }) => {
                            // 确认路径不应停泊（等待终态）；保守恢复现场。
                            PROMPT.with(|slot| {
                                *slot.borrow_mut() = Some(PromptOp {
                                    kind: op.kind,
                                    path: op.path,
                                    command_id: Some(command_id),
                                    generation: request,
                                    target,
                                })
                            });
                            app.set_ws_prompt_body("云端命令仍在等待确认，请稍后重试。".into());
                        }
                        Ok(CloudOpOutcome::Failed(reason)) => {
                            PROMPT.with(|slot| *slot.borrow_mut() = Some(op));
                            app.set_ws_prompt_body(reason.into());
                        }
                        Err(error) => {
                            PROMPT.with(|slot| *slot.borrow_mut() = Some(op));
                            app.set_ws_prompt_body(format!("确认失败：{error}").into());
                        }
                    }
                });
            });
            return;
        }
        // Stage one: validate, then issue the cloud write command.
        app.set_ws_prompt_busy(true);
        let kind = op.kind;
        let path = op.path.clone();
        let weak = app.as_weak();
        let issue_api = prompt_api.clone();
        let refresh_api = prompt_api.clone();
        std::thread::spawn(move || {
            let result = match kind {
                PromptKind::Rename => issue_api.interlink_cloud_rename(&path, &value),
                PromptKind::Delete => issue_api.interlink_cloud_delete(&path),
                PromptKind::NewDir => issue_api.interlink_cloud_mkdir(&join_cloud(&path, &value)),
                PromptKind::NewFile => {
                    issue_api.interlink_cloud_write(&join_cloud(&path, &value), "")
                }
                PromptKind::Clear => unreachable!(),
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                end_busy(&app, refresh_api.clone(), request);
                if !is_current(request) {
                    return;
                }
                match result {
                    Ok(CloudOpOutcome::NeedsApproval { command_id }) => {
                        // 云端停泊了命令：切换为确认阶段，用户确认后才执行。
                        PROMPT.with(|slot| {
                            *slot.borrow_mut() = Some(PromptOp {
                                kind,
                                path,
                                command_id: Some(command_id),
                                generation: request,
                                target,
                            })
                        });
                        app.set_ws_prompt_busy(false);
                        app.set_ws_prompt_body("云端要求确认后才会执行此操作。".into());
                        app.set_ws_prompt_input(false);
                        app.set_ws_prompt_confirm("确认执行".into());
                        app.set_ws_prompt_danger(true);
                    }
                    Ok(CloudOpOutcome::Done(_)) => {
                        // Same stale-cache rule as the local path: the renamed or
                        // removed directory must not be re-listed on refresh.
                        if matches!(kind, PromptKind::Rename | PromptKind::Delete) {
                            drop_dir_cache(&path);
                        }
                        app.set_ws_prompt_open(false);
                        PROMPT.with(|slot| *slot.borrow_mut() = None);
                        app.set_status(done_message(kind, &value).into());
                        refresh(&app, refresh_api);
                    }
                    Ok(CloudOpOutcome::Failed(reason)) => {
                        app.set_ws_prompt_busy(false);
                        PROMPT.with(|slot| {
                            *slot.borrow_mut() = Some(PromptOp {
                                kind,
                                path,
                                command_id: None,
                                generation: request,
                                target,
                            })
                        });
                        app.set_ws_prompt_body(reason.into());
                    }
                    Err(error) => {
                        app.set_ws_prompt_busy(false);
                        PROMPT.with(|slot| {
                            *slot.borrow_mut() = Some(PromptOp {
                                kind,
                                path,
                                command_id: None,
                                generation: request,
                                target,
                            })
                        });
                        app.set_ws_prompt_body(format!("操作失败：{error}").into());
                    }
                }
            });
        });
    });

    let weak = app.as_weak();
    app.on_ws_prompt_canceled(move || {
        if let Some(app) = weak.upgrade() {
            if app.get_ws_prompt_busy() {
                return;
            }
            app.set_ws_prompt_open(false);
        }
        PROMPT.with(|slot| *slot.borrow_mut() = None);
    });
}

/// Open the shared prompt dialog for one workspace write (cloud or local);
/// destructive deletes hide the input and flip the dialog into its danger
/// styling.
fn open_prompt(app: &MainWindow, kind: PromptKind, path: String, name: String) {
    let local = !is_cloud_target();
    let root_word = if local {
        "工作目录"
    } else {
        "云端工作目录"
    };
    let (title, body, confirm, danger, input, seed) = match kind {
        PromptKind::Rename => (
            "重命名",
            format!("重命名「{name}」为："),
            "确认重命名",
            false,
            true,
            name,
        ),
        PromptKind::Delete => (
            "删除",
            if local {
                format!("删除「{name}」？本地文件删除后不可恢复。")
            } else {
                format!("删除「{name}」？云端文件不可恢复。")
            },
            "确认删除",
            true,
            false,
            String::new(),
        ),
        PromptKind::NewDir => (
            "新建目录",
            if path.is_empty() {
                format!("在{root_word}根下新建目录，名称：")
            } else {
                format!("在 {path} 下新建目录，名称：")
            },
            "新建",
            false,
            true,
            String::new(),
        ),
        PromptKind::NewFile => (
            "新建文件",
            if path.is_empty() {
                format!("在{root_word}根下新建空文件，名称：")
            } else {
                format!("在 {path} 下新建空文件，名称：")
            },
            "新建",
            false,
            true,
            String::new(),
        ),
        PromptKind::Clear => (
            "清空工作目录",
            "清空云端工作目录中的全部文件和子目录？此操作不可恢复。".into(),
            "确认清空",
            true,
            false,
            String::new(),
        ),
    };
    PROMPT.with(|slot| {
        *slot.borrow_mut() = Some(PromptOp {
            kind,
            path,
            command_id: None,
            generation: generation(),
            target: current_target(),
        })
    });
    app.set_ws_prompt_open(true);
    app.set_ws_prompt_title(title.into());
    app.set_ws_prompt_body(body.into());
    app.set_ws_prompt_input(input);
    app.set_ws_prompt_value(seed.into());
    app.set_ws_prompt_confirm(confirm.into());
    app.set_ws_prompt_danger(danger);
    app.set_ws_prompt_busy(false);
}

/// Drag-out on a worker thread: a local file leaves straight off the disk; a
/// cloud file is staged first (a fresh pull, so the handed-out copy is
/// current), then the modal OLE gesture runs.
fn drag_out(
    api: &Arc<NativeDesktop>,
    target: &Target,
    path: &str,
    name: &str,
) -> Result<bool, String> {
    let local = match target {
        Target::Local(root) => api
            .workspace_local_resolve(&root, path)
            .map_err(|error| format!("定位 {name} 失败：{error}"))?,
        Target::Cloud => api
            .interlink_cloud_stage(path)
            .map_err(|error| format!("暂存 {name} 失败：{error}"))?,
    };
    crate::os_drag::drag_file(&local)
}

/// Drop-in uploads: read each staged file as text (binary and oversized
/// files are rejected with a clear reason), write it to the selected directory, and
/// auto-confirm parked commands — the drag-in gesture is the user's own
/// approval for this write.
fn upload_files(
    api: &Arc<NativeDesktop>,
    directory: &str,
    paths: &[PathBuf],
) -> (usize, Vec<String>) {
    let mut done = 0usize;
    let mut failed = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        match upload_one(api, directory, path) {
            Ok(()) => done += 1,
            Err(error) => failed.push(format!("{name}（{error}）")),
        }
    }
    (done, failed)
}

/// Drop-in for a local workspace folder: copy each dropped OS file into the
/// folder as-is (no text-only or size caps — the copy never leaves the disk).
fn copy_files(
    api: &Arc<NativeDesktop>,
    root: &str,
    directory: &str,
    paths: &[PathBuf],
) -> (usize, Vec<String>) {
    let mut done = 0usize;
    let mut failed = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        match api.workspace_local_upload_to(root, directory, path) {
            Ok(_) => done += 1,
            Err(error) => failed.push(format!("{name}（{error}）")),
        }
    }
    (done, failed)
}

fn upload_one(api: &Arc<NativeDesktop>, directory: &str, path: &PathBuf) -> Result<(), String> {
    let meta = std::fs::metadata(path).map_err(|error| format!("读取失败：{error}"))?;
    if meta.is_dir() {
        return Err("目录不支持直接上传".to_string());
    }
    if meta.len() > MAX_DROP_BYTES {
        return Err(format!("超出单文件上限 {} KiB", MAX_DROP_BYTES / 1024));
    }
    let content =
        std::fs::read_to_string(path).map_err(|_| "仅支持文本文件（UTF-8）".to_string())?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or("无效的文件名")?;
    let outcome = match api.interlink_cloud_write(&join_cloud(directory, &name), &content) {
        Ok(outcome) => outcome,
        Err(error) => return Err(error.to_string()),
    };
    let outcome = match outcome {
        CloudOpOutcome::NeedsApproval { command_id } => api
            .interlink_cloud_confirm(&command_id)
            .unwrap_or(CloudOpOutcome::Failed("确认云端命令失败".to_string())),
        decided => decided,
    };
    match outcome {
        CloudOpOutcome::Done(_) => Ok(()),
        CloudOpOutcome::NeedsApproval { .. } => Err("云端命令仍待确认".to_string()),
        CloudOpOutcome::Failed(reason) => Err(reason),
    }
}

/// Directory a new entry lands in: a directory row is its own target, a
/// file row resolves to its parent. Blank-area actions use the active entry.
fn target_dir(path: &str, kind: i32) -> String {
    if kind == 1 {
        path.to_string()
    } else {
        parent_cloud_dir(path)
    }
}

fn active_directory(app: &MainWindow) -> String {
    let path = app.get_ws_file_active_path();
    app.get_ws_file_rows()
        .iter()
        .find(|row| row.path == path && row.kind != 2)
        .map(|row| target_dir(&row.path, row.kind))
        .unwrap_or_default()
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().count() <= 120
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        && !name.ends_with('.')
}

fn parent_cloud_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_string(),
        None => String::new(),
    }
}

fn join_cloud(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn done_message(kind: PromptKind, value: &str) -> String {
    match kind {
        PromptKind::Rename => format!("已重命名为 {value}"),
        PromptKind::Delete => "已删除".to_string(),
        PromptKind::NewDir => format!("已新建目录 {value}"),
        PromptKind::NewFile => format!("已新建文件 {value}"),
        PromptKind::Clear => "已清空工作目录".into(),
    }
}

/// Tunnel tick from the 2s interlink poll: project the offline state and boot
/// the tree once the tunnel comes up (re-armed after a disconnect).
pub fn tunnel_tick(app: &MainWindow, api: &Arc<NativeDesktop>, connected: bool) {
    // A local folder is served straight from disk and never rides the tunnel,
    // so the offline hint and the boot guard belong to the cloud target only.
    if !is_cloud_target() {
        app.set_ws_file_offline(false);
        return;
    }
    app.set_ws_file_offline(!connected);
    if !connected {
        BOOTED.with(|slot| slot.set(false));
        return;
    }
    let booted = BOOTED.with(|slot| {
        if slot.get() {
            true
        } else {
            slot.set(true);
            false
        }
    });
    if !booted {
        refresh(app, api.clone());
    }
}

/// Open one cloud file: pull it into the local workspace (an existing copy is
/// refreshed) and hand it to the OS. Feedback rides the status bar, matching
/// the other shell actions.
fn open_file(app: &MainWindow, api: Arc<NativeDesktop>, path: String, name: String) {
    let target = current_target();
    let Some(request) = begin_busy(app) else {
        return;
    };
    app.set_status(format!("正在打开 {name}…").into());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = match target {
            Target::Local(root) => api.open_workspace_resource(&root, &path),
            Target::Cloud => api.interlink_cloud_open(&path).map(|_| ()),
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            end_busy(&app, api, request);
            if !is_current(request) {
                return;
            }
            match result {
                Ok(()) => app.set_status(format!("已打开 {name}").into()),
                Err(error) => app.set_status(format!("打开失败：{error}").into()),
            };
        });
    });
}

/// Download one cloud file: the same staged local pull as drag-out, without
/// handing it to the OS. The local workspace copy is the download target.
fn download_file(app: &MainWindow, api: Arc<NativeDesktop>, path: String, name: String) {
    if app.get_ws_file_busy() {
        return;
    }
    let Some(request) = begin_busy(app) else {
        return;
    };
    app.set_status(format!("正在下载 {name}…").into());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.interlink_cloud_stage(&path);
        let _ = weak.upgrade_in_event_loop(move |app| {
            end_busy(&app, api, request);
            if !is_current(request) {
                return;
            }
            match result {
                Ok(_) => app.set_status(format!("已下载 {name} 到本地工作区").into()),
                Err(error) => app.set_status(format!("下载失败：{error}").into()),
            }
        });
    });
}

/// 打包下载：请求云端把目录（空白区为当前根）打成 zip，落到本地工作区并交给系统打开。
fn archive_dir(app: &MainWindow, api: Arc<NativeDesktop>, path: String, name: String) {
    if app.get_ws_file_busy() {
        return;
    }
    let Some(request) = begin_busy(app) else {
        return;
    };
    app.set_status(format!("正在打包 {name}…").into());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.interlink_cloud_archive(&path, &name);
        let _ = weak.upgrade_in_event_loop(move |app| {
            end_busy(&app, api, request);
            if !is_current(request) {
                return;
            }
            match result {
                Ok(pulled) => app.set_status(
                    format!("已打包下载 {}（{}）", name, format_size(pulled.bytes)).into(),
                ),
                Err(error) => app.set_status(format!("打包失败：{error}").into()),
            }
        });
    });
}

/// 清空工作目录根，随后整面板刷新（对齐蜂巢空白区菜单）。
fn clear_root(app: &MainWindow, api: Arc<NativeDesktop>) {
    if app.get_ws_file_busy() {
        return;
    }
    let Some(request) = begin_busy(app) else {
        return;
    };
    app.set_status("正在清空工作目录…".into());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.interlink_cloud_clear();
        let _ = weak.upgrade_in_event_loop(move |app| {
            end_busy(&app, api.clone(), request);
            if !is_current(request) {
                return;
            }
            match result {
                Ok(()) => {
                    with_panel(|panel| panel.dirs.clear());
                    app.set_status("已清空工作目录".into());
                    refresh(&app, api);
                }
                Err(error) => app.set_status(format!("清空失败：{error}").into()),
            }
        });
    });
}

/// Expand or collapse one directory. Collapsing keeps the page cache, so
/// re-opening is instant and costs no round trip.
fn toggle_dir(app: &MainWindow, api: Arc<NativeDesktop>, path: String) {
    let known = with_panel(|panel| {
        let Some(dir) = panel.dirs.get_mut(&path) else {
            return None;
        };
        dir.expanded = !dir.expanded;
        Some((dir.expanded, dir.loaded, dir.loading))
    })
    .flatten();
    let Some((expanded, loaded, loading)) = known else {
        // Never seen: the first open is always a fetch.
        load_dir(&app_weak(app), api, path, 0);
        return;
    };
    if loading {
        rebuild(app);
        return;
    }
    if !expanded {
        rebuild(app);
        return;
    }
    if loaded {
        rebuild(app);
        return;
    }
    load_dir(&app_weak(app), api, path, 0);
}

/// Page one directory further from its load-more tail.
fn load_more(app: &MainWindow, api: Arc<NativeDesktop>, path: String) {
    if with_panel(|panel| {
        panel
            .dirs
            .get(&path)
            .is_some_and(|dir| dir.loading || dir.entries.len() >= MAX_DIRECTORY_ENTRIES)
    })
    .unwrap_or(true)
    {
        return;
    }
    let loaded = with_panel(|panel| {
        panel
            .dirs
            .get(&path)
            .map(|dir| dir.entries.len() as u64)
            .unwrap_or(0)
    })
    .unwrap_or(0);
    load_dir(&app_weak(app), api, path, loaded);
}

fn app_weak(app: &MainWindow) -> slint::Weak<MainWindow> {
    app.as_weak()
}

/// Fetch one directory page on a background thread. The directory row shows a
/// loading tag while in flight; the root failure surfaces as the panel's
/// error block, child failures ride the status bar.
fn load_dir(weak: &slint::Weak<MainWindow>, api: Arc<NativeDesktop>, path: String, offset: u64) {
    let Some(app) = weak.upgrade() else {
        return;
    };
    if with_panel(|panel| panel.dirs.get(&path).is_some_and(|dir| dir.loading)).unwrap_or(false) {
        return;
    }
    if !make_cache_room(&path) {
        if path.is_empty() {
            app.set_ws_file_loading(false);
        }
        app.set_status("已达到目录浏览缓存上限，请收起部分目录后刷新".into());
        return;
    }
    let Some(request) = begin_busy(&app) else {
        with_panel(|panel| {
            if panel.dirs.len() < MAX_CACHED_DIRS {
                panel.dirs.entry(path).or_default().expanded = true;
            }
        });
        with_panel(|panel| panel.refresh_pending = true);
        return;
    };
    with_panel(|panel| {
        let dir = panel.dirs.entry(path.clone()).or_default();
        dir.loading = true;
        dir.expanded = true;
    });
    if path.is_empty() {
        app.set_ws_file_loading(true);
    }
    rebuild(&app);
    let weak = weak.clone();
    let target = current_target();
    std::thread::spawn(move || {
        let result = list_dir(&api, &target, &path, offset);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if is_current(request) {
                apply_request_listing(&app, request, path, offset, result);
            }
            end_busy(&app, api, request);
        });
    });
}

/// Refresh: usage line, root page and the expanded directories (bounded), all
/// sequentially on one background thread so the panel flips states once.
fn refresh(app: &MainWindow, api: Arc<NativeDesktop>) {
    if app.get_ws_file_busy() {
        with_panel(|panel| panel.refresh_pending = true);
        return;
    }
    let expanded: Vec<(String, usize)> = with_panel(|panel| {
        let mut expanded: Vec<_> = panel
            .dirs
            .iter()
            .filter(|(path, dir)| dir.expanded || path.is_empty())
            .map(|(path, dir)| (path.clone(), dir.entries.len()))
            .collect();
        if !expanded.iter().any(|(path, _)| path.is_empty()) {
            expanded.push((String::new(), 0));
        }
        expanded.sort_by(|a, b| {
            a.0.matches('/')
                .count()
                .cmp(&b.0.matches('/').count())
                .then_with(|| a.0.cmp(&b.0))
        });
        expanded.truncate(REFRESH_DIR_CAP);
        expanded
    })
    .unwrap_or_default();
    let Some(request) = begin_busy(app) else {
        with_panel(|panel| panel.refresh_pending = true);
        return;
    };
    // Reserve the refreshed directories so a simultaneous load-more cannot
    // append a page that the older refresh snapshot would then overwrite.
    with_panel(|panel| {
        for (path, _) in &expanded {
            panel.dirs.entry(path.clone()).or_default().loading = true;
        }
    });
    app.set_ws_file_loading(app.get_ws_file_rows().row_count() == 0);
    rebuild(app);
    let weak = app.as_weak();
    let target = current_target();
    std::thread::spawn(move || {
        // The usage line only exists for the cloud workspace; a local folder
        // shows none (the stats line is already cleared by `reflect_target`).
        let stats = match &target {
            Target::Cloud => api.interlink_cloud_stats().ok(),
            Target::Local(_) => None,
        };
        let mut listings = Vec::new();
        for (path, loaded) in expanded {
            // Keep already paged rows through refresh instead of jumping back
            // to the first 200 items while the user is browsing further down.
            let mut listing = list_dir(&api, &target, &path, 0);
            if let Ok(first) = &mut listing {
                while first.status == "succeeded"
                    && first.entries.len() < loaded.min(MAX_DIRECTORY_ENTRIES)
                    && (first.entries.len() as u64) < first.total
                {
                    let offset = first.entries.len() as u64;
                    match list_dir(&api, &target, &path, offset) {
                        Ok(next) if next.status == "succeeded" && !next.entries.is_empty() => {
                            first.entries.extend(next.entries);
                            first.total = next.total;
                        }
                        _ => break,
                    }
                }
            }
            listings.push((path, listing));
        }
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !is_current(request) {
                end_busy(&app, api, request);
                return;
            }
            if let Some(stats) = stats {
                app.set_ws_file_stats(stats_line(&stats).into());
            }
            for (path, listing) in listings {
                if path.is_empty()
                    || with_panel(|panel| panel.dirs.contains_key(&path)).unwrap_or(false)
                {
                    apply_listing_state(&app, path, 0, listing);
                }
            }
            rebuild(&app);
            end_busy(&app, api, request);
        });
    });
}

fn apply_request_listing(
    app: &MainWindow,
    request: u64,
    path: String,
    offset: u64,
    result: Result<NativeInterlinkListing>,
) -> bool {
    if !is_current(request) {
        return false;
    }
    apply_listing(app, path, offset, result);
    true
}

/// Apply one directory page: replace or append, then reproject the tree.
fn apply_listing(
    app: &MainWindow,
    path: String,
    offset: u64,
    result: Result<NativeInterlinkListing>,
) {
    apply_listing_state(app, path, offset, result);
    rebuild(app);
}

fn apply_listing_state(
    app: &MainWindow,
    path: String,
    offset: u64,
    result: Result<NativeInterlinkListing>,
) {
    if path.is_empty() {
        app.set_ws_file_loading(false);
    }
    let result = match result {
        Ok(listing) => listing,
        Err(error) => {
            if path.is_empty() {
                app.set_ws_file_error(format!("{}：{error}", read_error()).into());
            } else {
                app.set_status(format!("目录加载失败：{error}").into());
            }
            with_panel(|panel| {
                if let Some(dir) = panel.dirs.get_mut(&path) {
                    dir.loading = false;
                    if dir.entries.is_empty() {
                        dir.expanded = false;
                    }
                }
            });
            return;
        }
    };
    match listing_status(&result) {
        "succeeded" => {
            with_panel(|panel| {
                let budget = MAX_CACHED_ENTRIES.saturating_sub(
                    panel
                        .dirs
                        .iter()
                        .filter(|(key, _)| *key != &path)
                        .map(|(_, dir)| dir.entries.len())
                        .sum::<usize>(),
                );
                let dir = panel.dirs.entry(path.clone()).or_default();
                dir.loading = false;
                if offset > 0 && offset != dir.entries.len() as u64 {
                    return;
                }
                if path.is_empty() {
                    dir.expanded = true;
                }
                dir.loaded = true;
                if offset == 0 {
                    dir.entries = result.entries.clone();
                } else {
                    dir.entries.extend(result.entries.clone());
                }
                dir.total = result.total;
                dir.entries.truncate(MAX_DIRECTORY_ENTRIES.min(budget));
                let surviving: std::collections::HashSet<_> = dir
                    .entries
                    .iter()
                    .filter(|entry| matches!(entry.kind.as_str(), "dir" | "directory"))
                    .map(|entry| entry.path.clone())
                    .collect();
                // Remove cached descendants of folders deleted outside the UI.
                panel.dirs.retain(|key, _| {
                    key == &path
                        || !is_descendant(key, &path)
                        || surviving
                            .iter()
                            .any(|child| key == child || is_descendant(key, child))
                });
            });
            if path.is_empty() {
                app.set_ws_file_error(String::new().into());
            }
        }
        "pending" => {
            with_panel(|panel| {
                if let Some(dir) = panel.dirs.get_mut(&path) {
                    dir.loading = false;
                    dir.expanded = false;
                }
            });
            if path.is_empty() {
                app.set_ws_file_error("等待云端审批".into());
            } else {
                app.set_status("云端要求先批准该次读取".into());
            }
        }
        _ => {
            let message = result
                .error
                .clone()
                .unwrap_or_else(|| "云端未返回原因".to_string());
            with_panel(|panel| {
                if let Some(dir) = panel.dirs.get_mut(&path) {
                    dir.loading = false;
                    if dir.entries.is_empty() {
                        dir.expanded = false;
                    }
                }
            });
            if path.is_empty() {
                app.set_ws_file_error(format!("{}：{message}", read_error()).into());
            } else {
                app.set_status(format!("目录加载失败：{message}").into());
            }
        }
    }
}

/// Rebuild the flat visible rows from the page cache: expanded directories
/// inline their children (recursively), each paged directory ends with the
/// load-more tail when entries remain.
fn rebuild(app: &MainWindow) {
    with_panel(|panel| {
        let mut rows: Vec<WorkspaceFileRow> = Vec::new();
        walk(panel, &mut rows, "", 0);
        let count = panel.model.row_count();
        let new_count = rows.len();
        for (index, row) in rows.into_iter().enumerate() {
            if index < count {
                if panel.model.row_data(index).as_ref() != Some(&row) {
                    panel.model.set_row_data(index, row);
                }
            } else {
                panel.model.push(row);
            }
        }
        while panel.model.row_count() > new_count {
            panel.model.remove(panel.model.row_count() - 1);
        }
    });
    let mut active = app.get_ws_file_active_path().to_string();
    let rows = app.get_ws_file_rows();
    let index = loop {
        if let Some(index) = rows
            .iter()
            .position(|row| row.path == active && row.kind != 2)
        {
            break Some(index);
        }
        if active.is_empty() {
            break None;
        }
        active = parent_cloud_dir(&active);
    };
    if index.is_some() {
        app.set_ws_file_active_path(active.into());
    }
    app.set_ws_file_active_index(index.map_or(-1, |index| index as i32));
    if index.is_none() {
        app.set_ws_file_active_path("".into());
    }
}

fn is_descendant(path: &str, parent: &str) -> bool {
    if parent.is_empty() {
        !path.is_empty()
    } else {
        path.strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// Only collapsed branches are evicted; a visible expansion never disappears
/// just because another folder was opened. All cache growth has a hard bound.
fn make_cache_room(path: &str) -> bool {
    with_panel(|panel| {
        while (!panel.dirs.contains_key(path) && panel.dirs.len() >= MAX_CACHED_DIRS)
            || panel
                .dirs
                .values()
                .map(|dir| dir.entries.len())
                .sum::<usize>()
                >= MAX_CACHED_ENTRIES
        {
            let victim = panel
                .dirs
                .iter()
                .filter(|(key, dir)| {
                    !key.is_empty() && key.as_str() != path && !dir.expanded && !dir.loading
                })
                .map(|(key, _)| key.clone())
                .min();
            let Some(victim) = victim else {
                return false;
            };
            panel
                .dirs
                .retain(|key, _| key != &victim && !is_descendant(key, &victim));
        }
        true
    })
    .unwrap_or(false)
}

fn walk(panel: &Panel, rows: &mut Vec<WorkspaceFileRow>, key: &str, depth: usize) {
    let Some(dir) = panel.dirs.get(key) else {
        return;
    };
    for entry in &dir.entries {
        if entry.kind == "dir" || entry.kind == "directory" {
            let child = panel.dirs.get(&entry.path);
            rows.push(WorkspaceFileRow {
                path: entry.path.as_str().into(),
                name: entry.name.as_str().into(),
                kind: 1,
                size_text: slint::SharedString::default(),
                icon: slint::Image::default(),
                depth: depth as i32,
                expanded: child.is_some_and(|child| child.expanded),
                loading: child.is_some_and(|child| child.loading),
                remaining: 0,
            });
            if child.is_some_and(|child| child.expanded) {
                walk(panel, rows, &entry.path, depth + 1);
            }
        } else {
            rows.push(WorkspaceFileRow {
                path: entry.path.as_str().into(),
                name: entry.name.as_str().into(),
                kind: 0,
                size_text: format_size(entry.size).as_str().into(),
                icon: crate::file_icons::workspace_file_icon(&entry.name, &entry.kind),
                depth: depth as i32,
                expanded: false,
                loading: false,
                remaining: 0,
            });
        }
    }
    let loaded = dir.entries.len() as u64;
    if dir.total > loaded {
        let capped = loaded >= MAX_DIRECTORY_ENTRIES as u64
            || panel
                .dirs
                .values()
                .map(|dir| dir.entries.len())
                .sum::<usize>()
                >= MAX_CACHED_ENTRIES;
        rows.push(WorkspaceFileRow {
            path: key.to_string().as_str().into(),
            name: if capped {
                "已达浏览上限，请收起目录后刷新".to_string()
            } else {
                format!("加载更多（剩余 {} 项）", dir.total - loaded)
            }
            .as_str()
            .into(),
            kind: 2,
            size_text: slint::SharedString::default(),
            icon: slint::Image::default(),
            depth: depth as i32,
            expanded: false,
            loading: dir.loading || capped,
            remaining: if capped {
                -1
            } else {
                (dir.total - loaded).min(i32::MAX as u64) as i32
            },
        });
    }
}

fn listing_status(listing: &NativeInterlinkListing) -> &str {
    match listing.status.as_str() {
        "succeeded" | "pending" => listing.status.as_str(),
        _ => "failed",
    }
}

/// Usage line of the sidebar head: used bytes and file count, the web pair.
fn stats_line(stats: &wunder_desktop::native::NativeInterlinkStats) -> String {
    let mut line = format!(
        "已用 {} · {} 个文件",
        format_size(stats.used_bytes),
        stats.files
    );
    if stats.truncated {
        line.push_str(" · 统计为大目录下界");
    }
    line
}

/// The bounded human size the tree rows and the usage line share.
fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * KB;
    const GB: f64 = MB * KB;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.0} KB", value / KB)
    } else if bytes > 0 {
        format!("{bytes} B")
    } else {
        String::new()
    }
}

fn begin_busy(app: &MainWindow) -> Option<u64> {
    if BUSY.with(Cell::get) >= MAX_REQUESTS {
        return None;
    }
    BUSY.with(|slot| slot.set(slot.get() + 1));
    with_panel(|panel| panel.in_flight += 1);
    app.set_ws_file_busy(true);
    Some(generation())
}

fn end_busy(app: &MainWindow, api: Arc<NativeDesktop>, request: u64) {
    BUSY.with(|slot| slot.set(slot.get().saturating_sub(1)));
    let (busy, pending) = with_panel(|panel| {
        if is_current(request) {
            panel.in_flight = panel.in_flight.saturating_sub(1);
        }
        let busy = panel.in_flight > 0;
        let pending = !busy && panel.refresh_pending && BUSY.with(Cell::get) < MAX_REQUESTS;
        if pending {
            panel.refresh_pending = false;
        }
        (busy, pending)
    })
    .unwrap_or_default();
    app.set_ws_file_busy(busy);
    if pending {
        refresh(app, api);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, path: &str, kind: &str, size: u64) -> NativeInterlinkEntry {
        NativeInterlinkEntry {
            name: name.to_string(),
            path: path.to_string(),
            kind: kind.to_string(),
            size,
            updated_time: String::new(),
        }
    }

    /// The projection walks expanded directories inline, keeps collapsed ones
    /// as single rows and appends the load-more tail when entries remain.
    #[test]
    fn rows_walk_expanded_dirs_and_page_tails() {
        PANEL.with(|slot| {
            *slot.borrow_mut() = Some(Panel {
                model: Rc::new(VecModel::default()),
                dirs: HashMap::new(),
                in_flight: 0,
                refresh_pending: false,
            });
        });
        with_panel(|panel| {
            let root = panel.dirs.entry(String::new()).or_default();
            root.entries = vec![
                entry("notes", "notes", "dir", 0),
                entry("a.md", "a.md", "file", 2048),
            ];
            root.total = 3;
            let notes = panel.dirs.entry("notes".to_string()).or_default();
            notes.entries = vec![entry("b.md", "notes/b.md", "file", 4)];
            notes.total = 1;
            notes.expanded = true;
        });
        let mut rows = Vec::new();
        with_panel(|panel| walk(panel, &mut rows, "", 0));
        assert_eq!(rows.len(), 4);
        assert_eq!(
            (rows[0].kind, rows[0].name.as_str(), rows[0].expanded),
            (1, "notes", true)
        );
        assert_eq!((rows[1].kind, rows[1].depth), (0, 1));
        assert_eq!(rows[1].path.as_str(), "notes/b.md");
        assert_eq!((rows[2].kind, rows[2].size_text.as_str()), (0, "2 KB"));
        // Root kept its page tail: total 3 with 2 loaded.
        assert_eq!(rows[3].kind, 2);
        assert_eq!(rows[3].remaining, 1);
        assert!(rows[3].name.as_str().contains("剩余 1 项"));
        PANEL.with(|slot| *slot.borrow_mut() = None);
    }

    #[test]
    fn sizes_format_like_the_web_usage_line() {
        assert_eq!(format_size(0), "");
        assert_eq!(format_size(850), "850 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(format_size(5 * 1024 * 1024 * 1024), "5.0 GB");
        let stats = wunder_desktop::native::NativeInterlinkStats {
            files: 12,
            dirs: 3,
            used_bytes: 4096,
            truncated: true,
        };
        let line = stats_line(&stats);
        assert!(line.starts_with("已用 4 KB · 12 个文件"), "{line}");
        assert!(line.ends_with("统计为大目录下界"), "{line}");
    }

    /// New entries land inside a directory row, beside a file row, and in
    /// the root for blank-area actions; cloud paths join without doubles.
    #[test]
    fn prompt_targets_resolve_like_the_web_menu() {
        assert_eq!(target_dir("notes", 1), "notes");
        assert_eq!(target_dir("notes/todo.md", 0), "notes");
        assert_eq!(target_dir("todo.md", 0), "");
        assert_eq!(join_cloud("", "a.md"), "a.md");
        assert_eq!(join_cloud("notes", "a.md"), "notes/a.md");
    }

    #[test]
    fn prompt_messages_name_the_finished_write() {
        assert_eq!(done_message(PromptKind::Rename, "b.md"), "已重命名为 b.md");
        assert_eq!(done_message(PromptKind::Delete, "x"), "已删除");
        assert_eq!(
            done_message(PromptKind::NewDir, "notes"),
            "已新建目录 notes"
        );
        assert_eq!(done_message(PromptKind::NewFile, "a.md"), "已新建文件 a.md");
    }

    #[test]
    fn stale_workspace_read_is_rejected_even_after_returning_to_the_same_target() {
        GENERATION.with(|slot| slot.set(40));
        let old = generation();
        GENERATION.with(|slot| slot.set(42));
        assert!(!is_current(old));
        assert!(is_current(generation()));
        GENERATION.with(|slot| slot.set(0));
    }

    #[test]
    fn directory_cache_evicts_collapsed_branches_and_keeps_visible_folders() {
        PANEL.with(|slot| *slot.borrow_mut() = None);
        with_panel(|panel| {
            panel.dirs.insert(String::new(), DirState::default());
            for index in 0..MAX_CACHED_DIRS - 1 {
                panel.dirs.insert(
                    format!("folder{index}"),
                    DirState {
                        expanded: index != 0,
                        ..Default::default()
                    },
                );
            }
        });
        assert!(make_cache_room("new"));
        with_panel(|panel| {
            assert!(!panel.dirs.contains_key("folder0"));
            assert!(panel.dirs.contains_key("folder1"));
            panel.dirs.insert(
                "new".into(),
                DirState {
                    expanded: true,
                    ..Default::default()
                },
            );
        });
        assert!(!make_cache_room("another"));
        PANEL.with(|slot| *slot.borrow_mut() = None);
    }

    #[test]
    fn entry_names_and_cache_path_boundaries_are_explicit() {
        for name in ["sample.txt", "folder2", "示例"] {
            assert!(valid_name(name));
        }
        for name in ["", ".", "..", "a/b", "a\\b", "a:b", "a?b", "sample."] {
            assert!(!valid_name(name));
        }
        assert!(is_descendant("notes/sub/sample.txt", "notes"));
        assert!(!is_descendant("notes-other/sample.txt", "notes"));
        assert!(!is_descendant("notes", "notes"));
    }

    #[test]
    fn listing_projection_preserves_collapse_selection_and_rejects_old_results() {
        struct TestPlatform;
        impl slint::platform::Platform for TestPlatform {
            fn create_window_adapter(
                &self,
            ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
                Ok(
                    slint::platform::software_renderer::MinimalSoftwareWindow::new(
                        slint::platform::software_renderer::RepaintBufferType::NewBuffer,
                    ),
                )
            }
        }
        slint::platform::set_platform(Box::new(TestPlatform)).unwrap();
        crate::register_bundled_font().expect("register fixture font");
        let app = MainWindow::new().unwrap();
        PANEL.with(|slot| *slot.borrow_mut() = None);
        let model = with_panel(|panel| panel.model.clone()).unwrap();
        app.set_ws_file_rows(ModelRc::from(model));
        let listing =
            |path: &str, entries: Vec<NativeInterlinkEntry>, total| NativeInterlinkListing {
                status: "succeeded".into(),
                path: path.into(),
                entries,
                total,
                error: None,
                command_id: None,
            };
        GENERATION.with(|slot| slot.set(12));
        apply_listing(
            &app,
            "".into(),
            0,
            Ok(listing("", vec![entry("notes", "notes", "dir", 0)], 1)),
        );
        with_panel(|panel| {
            panel.dirs.insert(
                "notes".into(),
                DirState {
                    loading: true,
                    expanded: false,
                    ..Default::default()
                },
            );
        });
        apply_listing(
            &app,
            "notes".into(),
            0,
            Ok(listing(
                "notes",
                vec![entry("sample.txt", "notes/sample.txt", "file", 1)],
                1,
            )),
        );
        assert_eq!(
            app.get_ws_file_rows().row_count(),
            1,
            "late response must not reopen a collapsed folder"
        );
        with_panel(|panel| panel.dirs.get_mut("notes").unwrap().expanded = true);
        app.set_ws_file_active_path("notes/sample.txt".into());
        rebuild(&app);
        assert_eq!(app.get_ws_file_active_index(), 1);
        let stale = listing("", vec![entry("stale.txt", "stale.txt", "file", 1)], 1);
        assert!(!apply_request_listing(&app, 10, "".into(), 0, Ok(stale)));
        assert_eq!(app.get_ws_file_rows().row_count(), 2);
        with_panel(|panel| panel.dirs.get_mut("notes").unwrap().expanded = false);
        rebuild(&app);
        assert_eq!(app.get_ws_file_active_path(), "notes");
        assert_eq!(app.get_ws_file_active_index(), 0);
        apply_listing(&app, "".into(), 0, Ok(listing("", Vec::new(), 0)));
        assert_eq!(app.get_ws_file_rows().row_count(), 0);
        assert_eq!(with_panel(|panel| panel.dirs.len()), Some(1));
        assert_eq!(app.get_ws_file_active_index(), -1);
        PANEL.with(|slot| *slot.borrow_mut() = None);
        GENERATION.with(|slot| slot.set(0));
    }
}
