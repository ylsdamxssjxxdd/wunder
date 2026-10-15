//! Sidebar working-directory region: the cloud workspace file tree. One flat
//! row model, rebuilt on the UI thread from a per-directory page cache; every
//! cloud round trip runs on a background thread through the synchronous
//! interlink façade and lands via `upgrade_in_event_loop`. The tree shares the
//! web left-rail's semantics: lazy directories, 200-entry pages with a
//! load-more tail, dirs-first natural order, and a click on a file pulls it
//! into the local workspace and opens it with the OS default application.
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
}

/// One parked prompt: the entry it acts on plus, once the cloud parks the
/// command for approval, the command id awaiting the desktop's decision.
struct PromptOp {
    kind: PromptKind,
    path: String,
    command_id: Option<String>,
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
}

thread_local! {
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
    /// One-shot boot guard: the first connected tick loads root + stats;
    /// disconnecting re-arms it so signing back in reloads.
    static BOOTED: Cell<bool> = const { Cell::new(false) };
    static BUSY: Cell<u32> = const { Cell::new(0) };
    /// The prompt currently on screen, if any; drives both dialog stages.
    static PROMPT: RefCell<Option<PromptOp>> = const { RefCell::new(None) };
    /// One OS drag at a time: DoDragDrop is modal on its worker thread.
    static DRAG_OUT: Cell<bool> = const { Cell::new(false) };
}

/// Which workspace the file panel mirrors: the cloud account workspace
/// (default) or one locally bound workspace folder, keyed by its root path.
/// Switching targets resets the page cache so no stale row leaks across.
#[derive(Clone, PartialEq)]
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

/// Push the target's chrome: the local flag swaps cloud-only actions out, the
/// offline hint (a local folder needs no tunnel) and a cleared usage line.
fn reflect_target(app: &MainWindow) {
    let cloud = is_cloud_target();
    app.set_ws_file_local(!cloud);
    app.set_ws_file_stats("".into());
    app.set_ws_file_error("".into());
    if !cloud {
        app.set_ws_file_offline(false);
    }
    // Drop the cached pages so the next listing starts from a clean root.
    with_panel(|panel| panel.dirs.clear());
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
    // the load-more tail pages its directory, a file pulls and opens.
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
        match row.kind {
            0 => open_file(&app, toggle_api.clone(), path, row.name.to_string()),
            1 => toggle_dir(&app, toggle_api.clone(), path),
            _ => load_more(&app, toggle_api.clone(), path),
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
    // blank-area entries (new dir / new file land in the root); actions and
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
                        _ => {}
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
                    app.set_draft(format!("{draft}@{} ", row.path).into());
                }
            }
            MENU_NEW_DIR => {
                let dir = row
                    .as_ref()
                    .map(|row| target_dir(&row.path, row.kind))
                    .unwrap_or_default();
                open_prompt(&app, PromptKind::NewDir, dir, String::new());
            }
            MENU_NEW_FILE => {
                let dir = row
                    .as_ref()
                    .map(|row| target_dir(&row.path, row.kind))
                    .unwrap_or_default();
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
                    clear_root(&app, menu_api.clone());
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
            app.set_status("目录暂不支持拖出，请使用下载或引用".into());
            return;
        }
        if DRAG_OUT.get() {
            return;
        }
        DRAG_OUT.set(true);
        let path = row.path.to_string();
        let name = row.name.to_string();
        app.set_status(format!("正在准备拖出 {name}…").into());
        let weak = app.as_weak();
        let worker_api = drag_api.clone();
        std::thread::spawn(move || {
            let outcome = drag_out(&worker_api, &path, &name);
            let _ = weak.upgrade_in_event_loop(move |app| {
                DRAG_OUT.set(false);
                match outcome {
                    Ok(copied) => {
                        if copied {
                            app.set_status(format!("已复制 {name} 到目标位置").into());
                        } else {
                            app.set_status(
                                format!("已取消拖出 {name}（本地副本保留在工作区）").into(),
                            );
                        }
                    }
                    Err(error) => app.set_status(format!("拖出失败：{error}").into()),
                }
            });
        });
    });

    // Drag-in: Explorer drops land inside the active target — text uploads
    // into the cloud workspace root (the gesture is the user's approval for
    // the write), or a plain copy into the browsed local folder. A drop
    // carrying nothing uploadable simply does nothing here.
    let weak = app.as_weak();
    let drop_api = api.clone();
    app.on_ws_file_drop(move |event| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let local = !is_cloud_target();
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
        begin_busy(&app.as_weak());
        let weak = app.as_weak();
        let worker_api = drop_api.clone();
        let target = current_target();
        std::thread::spawn(move || {
            let (done, failed) = match &target {
                Target::Local(root) => copy_files(&worker_api, root, &paths),
                Target::Cloud => upload_files(&worker_api, &paths),
            };
            let refresh_api = worker_api;
            let _ = weak.upgrade_in_event_loop(move |app| {
                end_busy(&app);
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
        let value = value.trim().to_string();
        // A local workspace folder writes straight off the disk: no parked
        // command, no approval stage — validate, run, refresh.
        if let Target::Local(root) = current_target() {
            if op.kind != PromptKind::Delete && value.is_empty() {
                PROMPT.with(|slot| *slot.borrow_mut() = Some(op));
                app.set_ws_prompt_body("名称不能为空".into());
                return;
            }
            app.set_ws_prompt_busy(true);
            let kind = op.kind;
            let path = op.path.clone();
            let weak = app.as_weak();
            let local_api = prompt_api.clone();
            let refresh_api = prompt_api.clone();
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
                };
                let _ = weak.upgrade_in_event_loop(move |app| match result {
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
                            })
                        });
                        app.set_ws_prompt_body(format!("操作失败：{error}").into());
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
                    app.set_ws_prompt_busy(false);
                    match result {
                        Ok(CloudOpOutcome::Done(_)) => {
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
        if op.kind != PromptKind::Delete && value.is_empty() {
            PROMPT.with(|slot| *slot.borrow_mut() = Some(op));
            app.set_ws_prompt_body("名称不能为空".into());
            return;
        }
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
            };
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(CloudOpOutcome::NeedsApproval { command_id }) => {
                    // 云端停泊了命令：切换为确认阶段，用户确认后才执行。
                    PROMPT.with(|slot| {
                        *slot.borrow_mut() = Some(PromptOp {
                            kind,
                            path,
                            command_id: Some(command_id),
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
                        })
                    });
                    app.set_ws_prompt_body(format!("操作失败：{error}").into());
                }
            });
        });
    });

    let weak = app.as_weak();
    app.on_ws_prompt_canceled(move || {
        if let Some(app) = weak.upgrade() {
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
    };
    PROMPT.with(|slot| {
        *slot.borrow_mut() = Some(PromptOp {
            kind,
            path,
            command_id: None,
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
fn drag_out(api: &Arc<NativeDesktop>, path: &str, name: &str) -> Result<bool, String> {
    let local = match current_target() {
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
/// files are rejected with a clear reason), write it to the cloud root, and
/// auto-confirm parked commands — the drag-in gesture is the user's own
/// approval for this write.
fn upload_files(api: &Arc<NativeDesktop>, paths: &[PathBuf]) -> (usize, Vec<String>) {
    let mut done = 0usize;
    let mut failed = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        match upload_one(api, path) {
            Ok(()) => done += 1,
            Err(error) => failed.push(format!("{name}（{error}）")),
        }
    }
    (done, failed)
}

/// Drop-in for a local workspace folder: copy each dropped OS file into the
/// folder as-is (no text-only or size caps — the copy never leaves the disk).
fn copy_files(api: &Arc<NativeDesktop>, root: &str, paths: &[PathBuf]) -> (usize, Vec<String>) {
    let mut done = 0usize;
    let mut failed = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        match api.workspace_local_upload(root, path) {
            Ok(_) => done += 1,
            Err(error) => failed.push(format!("{name}（{error}）")),
        }
    }
    (done, failed)
}

fn upload_one(api: &Arc<NativeDesktop>, path: &PathBuf) -> Result<(), String> {
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
    let outcome = match api.interlink_cloud_write(&name, &content) {
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
/// file row and the blank area fall back to the entry's parent (root).
fn target_dir(path: &str, kind: i32) -> String {
    if kind == 1 {
        path.to_string()
    } else {
        parent_cloud_dir(path)
    }
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
    if app.get_ws_file_busy() {
        return;
    }
    // A local file opens straight from the workspace folder; the cloud path
    // pulls a local copy first.
    if let Target::Local(root) = current_target() {
        match api.open_workspace_resource(&root, &path) {
            Ok(()) => app.set_status(format!("已打开 {name}").into()),
            Err(error) => app.set_status(format!("打开失败：{error}").into()),
        }
        return;
    }
    app.set_status(format!("正在拉取 {name}…").into());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.interlink_cloud_open(&path);
        let _ = weak.upgrade_in_event_loop(move |app| {
            match result {
                Ok(pulled) => app
                    .set_status(format!("已打开 {}（{}）", name, format_size(pulled.bytes)).into()),
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
    app.set_status(format!("正在下载 {name}…").into());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.interlink_cloud_stage(&path);
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(_) => app.set_status(format!("已下载 {name} 到本地工作区").into()),
            Err(error) => app.set_status(format!("下载失败：{error}").into()),
        });
    });
}

/// 打包下载：请求云端把目录（空白区为当前根）打成 zip，落到本地工作区并交给系统打开。
fn archive_dir(app: &MainWindow, api: Arc<NativeDesktop>, path: String, name: String) {
    if app.get_ws_file_busy() {
        return;
    }
    app.set_status(format!("正在打包 {name}…").into());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.interlink_cloud_archive(&path, &name);
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(pulled) => app
                .set_status(format!("已打包下载 {}（{}）", name, format_size(pulled.bytes)).into()),
            Err(error) => app.set_status(format!("打包失败：{error}").into()),
        });
    });
}

/// 清空工作目录根，随后整面板刷新（对齐蜂巢空白区菜单）。
fn clear_root(app: &MainWindow, api: Arc<NativeDesktop>) {
    if app.get_ws_file_busy() {
        return;
    }
    app.set_status("正在清空工作目录…".into());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.interlink_cloud_clear();
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(()) => {
                app.set_status("已清空工作目录".into());
                refresh(&app, api);
            }
            Err(error) => app.set_status(format!("清空失败：{error}").into()),
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
        if dir.loading {
            return Some((dir.expanded, dir.loaded, true));
        }
        dir.expanded = !dir.expanded;
        Some((dir.expanded, dir.loaded, false))
    })
    .flatten();
    let Some((expanded, loaded, loading)) = known else {
        // Never seen: the first open is always a fetch.
        load_dir(&app_weak(app), api, path, 0);
        return;
    };
    if loading {
        return;
    }
    if !expanded {
        rebuild();
        return;
    }
    if loaded {
        rebuild();
        return;
    }
    load_dir(&app_weak(app), api, path, 0);
}

/// Page one directory further from its load-more tail.
fn load_more(app: &MainWindow, api: Arc<NativeDesktop>, path: String) {
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
    with_panel(|panel| {
        let dir = panel.dirs.entry(path.clone()).or_default();
        dir.loading = true;
        dir.expanded = true;
    });
    begin_busy(weak);
    rebuild();
    let weak = weak.clone();
    let target = current_target();
    std::thread::spawn(move || {
        let result = list_dir(&api, &target, &path, offset);
        let _ = weak.upgrade_in_event_loop(move |app| {
            apply_listing(&app, path, offset, result);
            end_busy(&app);
        });
    });
}

/// Refresh: usage line, root page and the expanded directories (bounded), all
/// sequentially on one background thread so the panel flips states once.
fn refresh(app: &MainWindow, api: Arc<NativeDesktop>) {
    if app.get_ws_file_busy() {
        return;
    }
    let expanded: Vec<String> = with_panel(|panel| {
        panel
            .dirs
            .iter()
            .filter(|(path, dir)| dir.expanded && !path.is_empty())
            .map(|(path, _)| path.clone())
            .take(REFRESH_DIR_CAP)
            .collect()
    })
    .unwrap_or_default();
    begin_busy(&app.as_weak());
    rebuild();
    let weak = app.as_weak();
    let target = current_target();
    std::thread::spawn(move || {
        // The usage line only exists for the cloud workspace; a local folder
        // shows none (the stats line is already cleared by `reflect_target`).
        let stats = match &target {
            Target::Cloud => api.interlink_cloud_stats().ok(),
            Target::Local(_) => None,
        };
        let root = list_dir(&api, &target, "", 0);
        let mut children = Vec::new();
        for path in &expanded {
            children.push((path.clone(), list_dir(&api, &target, path, 0)));
        }
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Some(stats) = stats {
                app.set_ws_file_stats(stats_line(&stats).into());
            }
            match root {
                Ok(root) => apply_listing(&app, String::new(), 0, Ok(root)),
                Err(error) => app.set_ws_file_error(format!("{}：{error}", read_error()).into()),
            }
            for (path, listing) in children {
                if let Ok(listing) = listing {
                    apply_listing(&app, path, 0, Ok(listing));
                }
            }
            end_busy(&app);
        });
    });
}

/// Apply one directory page: replace or append, then reproject the tree.
fn apply_listing(
    app: &MainWindow,
    path: String,
    offset: u64,
    result: Result<NativeInterlinkListing>,
) {
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
            rebuild();
            return;
        }
    };
    match listing_status(&result) {
        "succeeded" => {
            with_panel(|panel| {
                let dir = panel.dirs.entry(path.clone()).or_default();
                dir.loading = false;
                dir.expanded = true;
                dir.loaded = true;
                if offset == 0 {
                    dir.entries = result.entries.clone();
                } else {
                    dir.entries.extend(result.entries.clone());
                }
                dir.total = result.total;
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
    rebuild();
}

/// Rebuild the flat visible rows from the page cache: expanded directories
/// inline their children (recursively), each paged directory ends with the
/// load-more tail when entries remain.
fn rebuild() {
    with_panel(|panel| {
        let mut rows: Vec<WorkspaceFileRow> = Vec::new();
        walk(panel, &mut rows, "", 0);
        panel.model.set_vec(rows);
    });
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
        rows.push(WorkspaceFileRow {
            path: key.to_string().as_str().into(),
            name: format!("加载更多（剩余 {} 项）", dir.total - loaded)
                .as_str()
                .into(),
            kind: 2,
            size_text: slint::SharedString::default(),
            icon: slint::Image::default(),
            depth: depth as i32,
            expanded: false,
            loading: false,
            remaining: (dir.total - loaded).min(i32::MAX as u64) as i32,
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

fn begin_busy(weak: &slint::Weak<MainWindow>) {
    BUSY.with(|slot| slot.set(slot.get().saturating_add(1)));
    let _ = weak.upgrade_in_event_loop(|app| app.set_ws_file_busy(true));
}

fn end_busy(app: &MainWindow) {
    let still_busy = BUSY.with(|slot| {
        slot.set(slot.get().saturating_sub(1));
        slot.get() > 0
    });
    app.set_ws_file_busy(still_busy);
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
}
