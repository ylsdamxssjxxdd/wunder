//! Single-agent navigation: thread ordering comes straight from storage
//! (recency), so the module keeps stable agent ids, the pet projection and
//! the sidebar tree projection (workspaces with their threads).
use crate::{MainWindow, SidebarRow, WorkspaceCard};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, collections::HashSet, sync::Arc};
use wunder_desktop::NativeDesktop;

pub(crate) fn agent_key(id: Option<&str>) -> &str {
    match id.map(str::trim) {
        None | Some("" | "default" | "__default__") => "__default__",
        Some(id) => id,
    }
}

pub(crate) fn project(app: &MainWindow) {
    crate::companion_pet::sync(app);
}

thread_local! {
    /// Workspaces the user collapsed manually; every workspace starts expanded.
    static COLLAPSED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Last workspace row clicked, so a second click folds it instead of
    /// flipping the selection back and forth.
    static LAST_SELECTED: RefCell<i32> = RefCell::new(-1);
    /// Manual workspace display order (workspace ids), materialized from the
    /// facade order on the first drag. Session-scoped until the facade grows a
    /// persisted ordering API; unknown ids rank before the ordered tail so new
    /// workspaces keep their facade (top) position.
    static WORKSPACE_ORDER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Manual thread display order (session ids), same semantics as above.
    static THREAD_ORDER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Rank of `id` in a manual order list: unknown ids come first (stable), the
/// ordered tail follows its stored sequence. Deletion gaps are harmless.
fn order_rank(order: &[String], id: &str) -> usize {
    order
        .iter()
        .position(|known| known.as_str() == id)
        .map_or(0, |position| position + 1)
}

/// Hard cap on sidebar tree rows. The conversation list itself is bounded at
/// 100 entries, so this only guards against pathological projections.
const MAX_TREE_ROWS: usize = 300;

/// Rebuild the sidebar from the workspace facade plus the bounded conversation
/// list. `desktop` is only needed on the first call and after workspace CRUD;
/// row rebuilds reuse the existing cards so selection changes cost no I/O.
pub(crate) fn project_sidebar(app: &MainWindow, desktop: Option<&NativeDesktop>) {
    if let Some(desktop) = desktop {
        match desktop.list_workspaces() {
            Ok(list) => {
                let cards: Vec<WorkspaceCard> = list
                    .iter()
                    .map(|w| WorkspaceCard {
                        workspace_id: w.workspace_id.as_str().into(),
                        name: w.name.as_str().into(),
                        icon: w.icon.as_str().into(),
                        color: w.color.as_str().into(),
                        root_path: w.root_path.as_str().into(),
                        thread_count: w.thread_count as i32,
                        expanded: true,
                        selected: false,
                    })
                    .collect();
                app.set_workspaces(ModelRc::new(VecModel::from(cards)));
                let count = app.get_workspaces().row_count();
                let selected = app.get_selected_workspace();
                if !(0..count).contains(&(selected as usize)) {
                    app.set_selected_workspace(if count > 0 { 0 } else { -1 });
                }
            }
            Err(error) => app.set_status(format!("无法读取工作区：{error}").into()),
        }
    }
    rebuild_rows(app);
}

fn rebuild_rows(app: &MainWindow) {
    let cards_model = app.get_workspaces();
    if cards_model.row_count() == 0 {
        app.set_sidebar_rows(ModelRc::default());
        return;
    }
    let selected = (usize::try_from(app.get_selected_workspace()).unwrap_or(0))
        .min(cards_model.row_count() - 1);
    if app.get_selected_workspace() != selected as i32 {
        app.set_selected_workspace(selected as i32);
    }
    let conversations = app.get_conversations();
    let active = app.get_active_session_id().to_string();
    let mut rows: Vec<SidebarRow> = Vec::with_capacity(cards_model.row_count() * 4);
    // Manual drag order is session-scoped; un-ordered ids keep facade position.
    let mut cards: Vec<(usize, WorkspaceCard)> = cards_model.iter().enumerate().collect();
    WORKSPACE_ORDER.with(|slot| {
        let order = slot.borrow();
        cards.sort_by_key(|(_, card)| order_rank(&order, card.workspace_id.as_str()));
    });
    COLLAPSED.with(|slot| {
        let collapsed = slot.borrow();
        for (wi, card) in cards {
            let expanded = !collapsed.contains(card.workspace_id.as_str());
            rows.push(SidebarRow {
                kind: 0,
                workspace_id: card.workspace_id.clone(),
                name: card.name.clone(),
                icon: card.icon.clone(),
                color: card.color.clone(),
                thread_count: card.thread_count,
                expanded,
                selected: wi == selected,
                thread_id: "".into(),
                title: "".into(),
                status: "".into(),
                active: false,
                payload: wi as i32,
            });
            if !expanded {
                continue;
            }
            let mut matches: Vec<usize> = (0..conversations.row_count())
                .filter(|&ci| {
                    conversations.row_data(ci).is_some_and(|conv| {
                        conv.workspace_id.as_str() == card.workspace_id.as_str()
                    })
                })
                .collect();
            THREAD_ORDER.with(|slot| {
                let order = slot.borrow();
                matches.sort_by_key(|&ci| {
                    let id = conversations
                        .row_data(ci)
                        .map(|conv| conv.id.to_string())
                        .unwrap_or_default();
                    order_rank(&order, id.as_str())
                });
            });
            for ci in matches {
                if rows.len() >= MAX_TREE_ROWS {
                    break;
                }
                let Some(conv) = conversations.row_data(ci) else {
                    continue;
                };
                rows.push(SidebarRow {
                    kind: 1,
                    workspace_id: card.workspace_id.clone(),
                    name: "".into(),
                    icon: "".into(),
                    color: "".into(),
                    thread_count: 0,
                    expanded: false,
                    selected: false,
                    thread_id: conv.id.clone(),
                    title: conv.title.clone(),
                    status: conv.runtime_status.clone(),
                    active: conv.id == active,
                    payload: ci as i32,
                });
            }
        }
    });
    app.set_sidebar_rows(ModelRc::new(VecModel::from(rows)));
}

/// Highlight the workspace an opened thread belongs to, so a later new-task
/// lands in the same workspace.
pub(crate) fn focus_workspace_of(app: &MainWindow, workspace_id: Option<&str>) {
    let Some(workspace_id) = workspace_id else { return };
    for (index, card) in app.get_workspaces().iter().enumerate() {
        if card.workspace_id.as_str() == workspace_id {
            app.set_selected_workspace(index as i32);
            return;
        }
    }
}

/// Sidebar interactions and the workspace entry points on the window.
pub fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    app.on_select_row(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(row) = app.get_sidebar_rows().row_data(index as usize) else {
            return;
        };
        if row.kind == 0 {
            let workspace = row.payload;
            let id = row.workspace_id.to_string();
            let repeat = LAST_SELECTED.with(|slot| *slot.borrow() == workspace);
            LAST_SELECTED.with(|slot| *slot.borrow_mut() = workspace);
            if repeat {
                COLLAPSED.with(|slot| {
                    let mut collapsed = slot.borrow_mut();
                    if !collapsed.remove(id.as_str()) {
                        collapsed.insert(id);
                    }
                });
            }
            app.set_selected_workspace(workspace);
            crate::navigation_ui::project_sidebar(&app, None);
        } else {
            app.invoke_select_conversation(row.payload);
        }
    });
    let weak = app.as_weak();
    // Drag reorder: a thread only swaps with an adjacent thread (workspace
    // headers block it); a workspace jumps to the nearest workspace header.
    // The order lists materialize on first drag and persist for the session.
    app.on_move_row(move |index, delta| {
        let Some(app) = weak.upgrade() else { return };
        if delta == 0 {
            return;
        }
        let rows_model = app.get_sidebar_rows();
        let count = rows_model.row_count();
        let Some(from) = usize::try_from(index).ok().filter(|&i| i < count) else {
            return;
        };
        let Some(source) = rows_model.row_data(from) else { return };
        let step = if delta > 0 { 1isize } else { -1isize };
        let mut target: Option<usize> = None;
        let mut cursor = from as isize + step;
        while (0..count as isize).contains(&cursor) {
            let Some(row) = rows_model.row_data(cursor as usize) else { break };
            if row.kind == source.kind {
                target = Some(cursor as usize);
                if source.kind == 1 {
                    break;
                }
            } else if source.kind == 1 {
                break;
            }
            cursor += step;
        }
        let Some(ti) = target else { return };
        let Some(target_row) = rows_model.row_data(ti) else { return };
        if source.kind == 0 {
            let (a, b) = (
                source.workspace_id.to_string(),
                target_row.workspace_id.to_string(),
            );
            let current: Vec<String> = app
                .get_workspaces()
                .iter()
                .map(|card| card.workspace_id.to_string())
                .collect();
            WORKSPACE_ORDER.with(|slot| {
                let mut order = slot.borrow().clone();
                if order.is_empty() {
                    order = current;
                }
                let (Some(ia), Some(ib)) = (
                    order.iter().position(|x| x.as_str() == a.as_str()),
                    order.iter().position(|x| x.as_str() == b.as_str()),
                ) else {
                    return;
                };
                order.swap(ia, ib);
                *slot.borrow_mut() = order;
            });
        } else {
            let (a, b) = (source.thread_id.to_string(), target_row.thread_id.to_string());
            let current: Vec<String> = app
                .get_conversations()
                .iter()
                .map(|conv| conv.id.to_string())
                .collect();
            THREAD_ORDER.with(|slot| {
                let mut order = slot.borrow().clone();
                if order.is_empty() {
                    order = current;
                }
                let (Some(ia), Some(ib)) = (
                    order.iter().position(|x| x.as_str() == a.as_str()),
                    order.iter().position(|x| x.as_str() == b.as_str()),
                ) else {
                    return;
                };
                order.swap(ia, ib);
                *slot.borrow_mut() = order;
            });
        }
        crate::navigation_ui::project_sidebar(&app, None);
    });
    let weak = app.as_weak();
    let api = desktop.clone();
    // 0 = new thread, 1 = edit, 2 = reveal in file manager, 3 = delete.
    app.on_workspace_menu(move |workspace, action| {
        let Some(app) = weak.upgrade() else { return };
        let Some(card) = app.get_workspaces().row_data(workspace as usize) else {
            return;
        };
        app.set_selected_workspace(workspace);
        match action {
            0 => {
                app.invoke_new_thread();
            }
            1 => crate::workspace_ui::open_edit(&app, workspace as usize),
            2 => {
                let root = card.root_path.to_string();
                let api = api.clone();
                let weak = weak.clone();
                std::thread::spawn(move || {
                    let result = api.open_workspace_resource(&root, "");
                    if let (Err(error), Some(app)) = (result, weak.upgrade()) {
                        app.set_status(format!("无法打开文件夹：{error}").into());
                    }
                });
            }
            3 => crate::workspace_ui::open_delete(&app, workspace as usize),
            _ => {}
        }
    });
    let weak = app.as_weak();
    let api = desktop.clone();
    app.on_rename_thread(move |index, title| {
        let Some(app) = weak.upgrade() else { return };
        let Some(row) = app.get_conversations().row_data(index as usize) else {
            return;
        };
        let id = row.id.to_string();
        let api = api.clone();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let result = api.rename_session(&id, &title);
            if let (Err(error), Some(app)) = (result, weak.upgrade()) {
                app.set_status(format!("无法重命名会话：{error}").into());
            }
        });
        crate::navigation_ui::project_sidebar(&app, None);
    });
    let weak = app.as_weak();
    let api = desktop.clone();
    app.on_archive_thread(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(row) = app.get_conversations().row_data(index as usize) else {
            return;
        };
        let id = row.id.to_string();
        let was_active = app.get_active_session_id().as_str() == id.as_str();
        let api = api.clone();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let result = api.archive_session(&id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Err(error) = result {
                    app.set_status(format!("无法归档会话：{error}").into());
                    return;
                }
                app.invoke_refresh_chat();
                if was_active {
                    app.set_active_session_id("".into());
                }
            });
        });
    });
    let weak = app.as_weak();
    let api = desktop.clone();
    app.on_open_workspace_folder(move |root| {
        let api = api.clone();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let result = api.open_workspace_resource(root.as_str(), "");
            if let (Err(error), Some(app)) = (result, weak.upgrade()) {
                app.set_status(format!("无法打开文件夹：{error}").into());
            }
        });
    });
    // §8.2: the composer's more-menu opens the active workspace folder. It
    // reuses the same facade call and off-thread shape as the sidebar action.
    let weak = app.as_weak();
    let api = desktop.clone();
    app.on_open_active_workspace(move || {
        let Some(app) = weak.upgrade() else { return };
        let root = app.get_active_workspace_root().to_string();
        if root.is_empty() {
            return;
        }
        let api = api.clone();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let result = api.open_workspace_resource(root.as_str(), "");
            if let (Err(error), Some(app)) = (result, weak.upgrade()) {
                app.set_status(format!("无法打开文件夹：{error}").into());
            }
        });
    });
    // §8.4: the permission chip is a recorded preference. The runtime reads
    // `security.approval_mode`, and the local desktop link has no approval
    // round-trip yet, so this deliberately does not pretend to enforce.
    let weak = app.as_weak();
    app.on_save_permission_mode(move |mode| {
        let Some(app) = weak.upgrade() else { return };
        let label = match mode.as_str() {
            "auto_edit" => "写入自动",
            "suggest" => "执行前确认",
            _ => "完全访问",
        };
        app.set_status(format!("权限模式已设为「{label}」").into());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_key_normalizes_legacy_ids() {
        assert_eq!(agent_key(Some("default")), agent_key(None));
        assert_eq!(agent_key(Some("__default__")), "__default__");
        assert_eq!(agent_key(Some("custom")), "custom");
    }
}
