//! Sidebar working-directory region: the cloud workspace file tree. One flat
//! row model, rebuilt on the UI thread from a per-directory page cache; every
//! cloud round trip runs on a background thread through the synchronous
//! interlink façade and lands via `upgrade_in_event_loop`. The tree shares the
//! web left-rail's semantics: lazy directories, 200-entry pages with a
//! load-more tail, dirs-first natural order, and a click on a file pulls it
//! into the local workspace and opens it with the OS default application.
use crate::{MainWindow, WorkspaceFileRow};
use anyhow::Result;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use wunder_desktop::native::{NativeInterlinkEntry, NativeInterlinkListing};
use wunder_desktop::NativeDesktop;

/// Expanded directories reloaded by one refresh; a bound keeps the fan-out
/// finite even if a user expands a wide tree before hitting refresh.
const REFRESH_DIR_CAP: usize = 12;

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
    busy: u32,
}

thread_local! {
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
    /// One-shot boot guard: the first connected tick loads root + stats;
    /// disconnecting re-arms it so signing back in reloads.
    static BOOTED: Cell<bool> = const { Cell::new(false) };
    static BUSY: Cell<u32> = const { Cell::new(0) };
}

fn with_panel<R>(run: impl FnOnce(&mut Panel) -> R) -> Option<R> {
    PANEL.with(|slot| {
        let mut slot = slot.borrow_mut();
        let panel = slot.get_or_insert_with(|| Panel {
            model: Rc::new(VecModel::default()),
            dirs: HashMap::new(),
            busy: 0,
        });
        Some(run(panel))
    })
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
    let weak = app.as_weak();
    app.on_ws_file_refresh(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        refresh(&app, api.clone());
    });
}

/// Tunnel tick from the 2s interlink poll: project the offline state and boot
/// the tree once the tunnel comes up (re-armed after a disconnect).
pub fn tunnel_tick(app: &MainWindow, api: &Arc<NativeDesktop>, connected: bool) {
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
    app.set_status(format!("正在拉取 {name}…").into());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.interlink_cloud_open(&path);
        let _ = weak.upgrade_in_event_loop(move |app| {
            match result {
                Ok(pulled) => {
                    app.set_status(format!("已打开 {}（{}）", name, format_size(pulled.bytes)).into())
                }
                Err(error) => app.set_status(format!("打开失败：{error}").into()),
            };
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
    std::thread::spawn(move || {
        let result = api.interlink_cloud_listing(&path, offset);
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
    std::thread::spawn(move || {
        let stats = api.interlink_cloud_stats().ok();
        let root = api.interlink_cloud_listing("", 0);
        let mut children = Vec::new();
        for path in &expanded {
            children.push((path.clone(), api.interlink_cloud_listing(path, 0)));
        }
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Some(stats) = stats {
                app.set_ws_file_stats(stats_line(&stats).into());
            }
            match root {
                Ok(root) => apply_listing(&app, String::new(), 0, Ok(root)),
                Err(error) => {
                    app.set_ws_file_error(format!("无法读取云端工作目录：{error}").into())
                }
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
fn apply_listing(app: &MainWindow, path: String, offset: u64, result: Result<NativeInterlinkListing>) {
    let result = match result {
        Ok(listing) => listing,
        Err(error) => {
            if path.is_empty() {
                app.set_ws_file_error(format!("无法读取云端工作目录：{error}").into());
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
            let message = result.error.clone().unwrap_or_else(|| "云端未返回原因".to_string());
            with_panel(|panel| {
                if let Some(dir) = panel.dirs.get_mut(&path) {
                    dir.loading = false;
                    if dir.entries.is_empty() {
                        dir.expanded = false;
                    }
                }
            });
            if path.is_empty() {
                app.set_ws_file_error(format!("无法读取云端工作目录：{message}").into());
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
            name: format!("加载更多（剩余 {} 项）", dir.total - loaded).as_str().into(),
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
    let mut line = format!("已用 {} · {} 个文件", format_size(stats.used_bytes), stats.files);
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
                busy: 0,
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
        assert_eq!((rows[0].kind, rows[0].name.as_str(), rows[0].expanded), (1, "notes", true));
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
}
