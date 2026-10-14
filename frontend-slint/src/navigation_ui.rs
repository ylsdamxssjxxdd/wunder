//! Single-agent navigation: thread ordering comes straight from storage
//! (recency), so the module keeps stable agent ids, the pet projection and
//! the sidebar tree projection (workspaces with their threads).
use crate::{MainWindow, SidebarMetrics, SidebarRow, WorkspaceCard};
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
    /// Manual workspace display order (workspace ids). Session-scoped until the
    /// facade grows a persisted ordering API; a row joins the list the first
    /// time it takes part in a drag, and unknown ids rank before the ordered
    /// tail so a fresh workspace keeps its facade (top) position.
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

/// A tree row reduced to what a drop needs: its kind, the workspace a thread
/// belongs to, and the id the manual order lists carry.
#[derive(Clone, Debug)]
struct RowSpec {
    kind: i32,
    bucket: String,
    id: String,
}

fn row_spec(row: SidebarRow) -> RowSpec {
    RowSpec {
        kind: row.kind,
        bucket: if row.kind == 1 {
            row.workspace_id.to_string()
        } else {
            String::new()
        },
        id: match row.kind {
            0 => row.workspace_id.to_string(),
            1 => row.thread_id.to_string(),
            _ => String::new(),
        },
    }
}

/// Rows that may take one another's place: every workspace header forms one
/// group, while threads only ever reorder inside their own workspace.
fn group_key(row: &RowSpec) -> (i32, &str) {
    (
        row.kind,
        if row.kind == 1 {
            row.bucket.as_str()
        } else {
            ""
        },
    )
}

fn row_height(kind: i32, thread_px: f32, group_px: f32) -> f32 {
    if kind == 1 { thread_px } else { group_px }
}

/// The row the pointer sits in, plus whether the drop lands after it. `dy` is
/// measured from the top of the dragged row's cell and the tree stacks rows
/// without gaps, so accumulating each row's own height is the whole geometry.
fn row_slot(rows: &[RowSpec], from: usize, dy: f32, thread_px: f32, group_px: f32) -> (usize, bool) {
    let mut top = 0f32;
    if dy >= 0.0 {
        for (index, row) in rows.iter().enumerate().skip(from) {
            let height = row_height(row.kind, thread_px, group_px);
            if dy < top + height {
                return (index, dy >= top + height / 2.0);
            }
            top += height;
        }
        return (rows.len() - 1, true);
    }
    for index in (0..from).rev() {
        let height = row_height(rows[index].kind, thread_px, group_px);
        top -= height;
        if dy >= top {
            return (index, dy >= top + height / 2.0);
        }
    }
    (0, false)
}

/// Where a released drag lands, counted inside the dragged row's own group.
struct DragPlan {
    /// Tree indexes of the group, in display order.
    members: Vec<usize>,
    from_pos: usize,
    to_pos: usize,
}

fn plan_drag(
    rows: &[RowSpec],
    from: usize,
    dy: f32,
    thread_px: f32,
    group_px: f32,
) -> Option<DragPlan> {
    let key = group_key(&rows[from]);
    let members: Vec<usize> = (0..rows.len())
        .filter(|&index| group_key(&rows[index]) == key)
        .collect();
    let from_pos = members.iter().position(|&index| index == from)?;
    // The slot the pointer opened, as "insert before this tree row". A slot
    // outside the group resolves to its ends, which is where the seam stops.
    let (slot, after) = row_slot(rows, from, dy, thread_px, group_px);
    let insert_before = slot + usize::from(after);
    let found = members
        .iter()
        .position(|&index| index >= insert_before)
        .unwrap_or(members.len());
    Some(DragPlan {
        members,
        from_pos,
        to_pos: if found > from_pos { found - 1 } else { found },
    })
}

/// Move one entry to another position of the same list: remove, then insert, so
/// a long drag lands several places away instead of only trading slots with its
/// neighbour.
fn reposition(ids: &mut Vec<String>, from_pos: usize, to_pos: usize) {
    if from_pos >= ids.len() {
        return;
    }
    let moved = ids.remove(from_pos);
    let target = to_pos.min(ids.len());
    ids.insert(target, moved);
}

/// Re-sequence one group inside a manual order list, leaving every other entry
/// where it is. Ids the list has never seen join its tail first, so a row
/// created after the last drag can still be dropped anywhere in its group.
fn reorder_in_order(order: &mut Vec<String>, next: &[String]) -> bool {
    for id in next {
        if !order.iter().any(|known| known == id) {
            order.push(id.clone());
        }
    }
    let slots: Vec<usize> = order
        .iter()
        .enumerate()
        .filter(|(_, id)| next.iter().any(|wanted| wanted.as_str() == id.as_str()))
        .map(|(index, _)| index)
        .collect();
    if slots.len() != next.len() {
        return false;
    }
    for (slot, id) in slots.into_iter().zip(next) {
        order[slot] = id.clone();
    }
    true
}

/// Swap a group's new sequence into the manual order list it lives in.
fn commit_order(cell: &RefCell<Vec<String>>, next: &[String]) -> bool {
    let mut order = cell.borrow().clone();
    if !reorder_in_order(&mut order, next) {
        return false;
    }
    *cell.borrow_mut() = order;
    true
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
    // The cloud workspace is a fixed sentinel row pinned above the local
    // workspaces while the account is signed in.
    let cloud = app.get_cloud_logged_in().then(|| cloud_row(app));
    if cards_model.row_count() == 0 {
        let rows: Vec<SidebarRow> = cloud.into_iter().collect();
        app.set_sidebar_rows(ModelRc::new(VecModel::from(rows)));
        return;
    }
    let selected = (usize::try_from(app.get_selected_workspace()).unwrap_or(0))
        .min(cards_model.row_count() - 1);
    if app.get_selected_workspace() != selected as i32 {
        app.set_selected_workspace(selected as i32);
    }
    let conversations = app.get_conversations();
    let active = app.get_active_session_id().to_string();
    let mut rows: Vec<SidebarRow> = Vec::with_capacity(cards_model.row_count() * 4 + 1);
    if let Some(cloud) = cloud {
        rows.push(cloud);
    }
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
                origin: "".into(),
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
                    origin: conv.origin.clone(),
                });
            }
        }
    });
    app.set_sidebar_rows(ModelRc::new(VecModel::from(rows)));
}

/// The pinned "cloud workspace" row: a sentinel (kind 2) whose click points
/// the working-directory panel back at the cloud account workspace.
fn cloud_row(app: &MainWindow) -> SidebarRow {
    let zh = app.global::<crate::I18n>().get_zh();
    SidebarRow {
        kind: 2,
        workspace_id: "".into(),
        name: if zh { "云端工作区".into() } else { "Cloud workspace".into() },
        icon: "cloud".into(),
        color: "".into(),
        thread_count: 0,
        expanded: true,
        selected: crate::workspace_files::is_cloud_target(),
        thread_id: "".into(),
        title: "".into(),
        status: "".into(),
        active: false,
        payload: -1,
        origin: "".into(),
    }
}

/// Highlight the workspace an opened thread belongs to, so a later new-task
/// lands in the same workspace, and point the working-directory panel at that
/// workspace's folder: the panel mirrors the directory of whatever thread is
/// focused, not a fixed default.
pub(crate) fn focus_workspace_of(
    app: &MainWindow,
    api: Arc<NativeDesktop>,
    workspace_id: Option<&str>,
) {
    let Some(workspace_id) = workspace_id else { return };
    for (index, card) in app.get_workspaces().iter().enumerate() {
        if card.workspace_id.as_str() == workspace_id {
            app.set_selected_workspace(index as i32);
            let root = card.root_path.to_string();
            if !root.is_empty() {
                crate::workspace_files::set_target(
                    app,
                    api,
                    crate::workspace_files::Target::Local(root),
                );
            }
            return;
        }
    }
}

/// Sidebar interactions and the workspace entry points on the window.
pub fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    let api = desktop.clone();
    let weak = app.as_weak();
    app.on_select_row(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(row) = app.get_sidebar_rows().row_data(index as usize) else {
            return;
        };
        if row.kind == 2 {
            // Pinned cloud row: point the file panel back at the cloud
            // workspace and drop the local selection.
            LAST_SELECTED.with(|slot| *slot.borrow_mut() = -1);
            app.set_selected_workspace(-1);
            crate::workspace_files::set_target(
                &app,
                api.clone(),
                crate::workspace_files::Target::Cloud,
            );
            crate::navigation_ui::project_sidebar(&app, None);
        } else if row.kind == 0 {
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
            // Mirror this workspace's folder in the working-directory panel.
            if let Some(card) = app.get_workspaces().row_data(workspace as usize) {
                let root = card.root_path.to_string();
                if !root.is_empty() {
                    crate::workspace_files::set_target(
                        &app,
                        api.clone(),
                        crate::workspace_files::Target::Local(root),
                    );
                }
            }
            crate::navigation_ui::project_sidebar(&app, None);
        } else {
            app.invoke_select_conversation(row.payload);
        }
    });
    let weak = app.as_weak();
    // Drag reorder: the grabbed row reports how far the pointer travelled, the
    // tree pitch says which row that lands on, and the row takes that slot in
    // its own group. Threads cannot leave their workspace and the pinned cloud
    // row cannot be dragged or dropped on at all.
    app.on_move_row(move |index, travel| {
        let Some(app) = weak.upgrade() else { return };
        let rows_model = app.get_sidebar_rows();
        let count = rows_model.row_count();
        let Some(from) = usize::try_from(index).ok().filter(|&i| i < count) else {
            return;
        };
        let specs: Vec<RowSpec> = (0..count)
            .filter_map(|i| rows_model.row_data(i).map(row_spec))
            .collect();
        if specs.len() != count
            || !matches!(specs[from].kind, 0 | 1)
            || specs[from].id.is_empty()
        {
            return;
        }
        let metrics = app.global::<SidebarMetrics>();
        let thread_px = metrics.get_thread_height();
        let group_px = metrics.get_group_height();
        let Some(plan) = plan_drag(&specs, from, travel as f32, thread_px, group_px) else {
            return;
        };
        if plan.from_pos == plan.to_pos {
            return;
        }
        let mut next: Vec<String> = plan.members.iter().map(|&i| specs[i].id.clone()).collect();
        reposition(&mut next, plan.from_pos, plan.to_pos);
        let applied = if specs[from].kind == 1 {
            THREAD_ORDER.with(|cell| commit_order(cell, &next))
        } else {
            WORKSPACE_ORDER.with(|cell| commit_order(cell, &next))
        };
        if applied {
            crate::navigation_ui::project_sidebar(&app, None);
        }
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

    fn header(id: &str) -> RowSpec {
        RowSpec { kind: 0, bucket: String::new(), id: id.to_string() }
    }

    fn thread(id: &str, workspace: &str) -> RowSpec {
        RowSpec { kind: 1, bucket: workspace.to_string(), id: id.to_string() }
    }

    /// The group's sequence after a drag that travelled `dy` px from the top of
    /// the grabbed row's cell, or the untouched sequence when it lands back on
    /// its own slot.
    fn dragged(rows: &[RowSpec], from: usize, dy: f32) -> String {
        let plan = plan_drag(rows, from, dy, 28.0, 34.0).expect("the row is in a group");
        let mut ids: Vec<String> = plan.members.iter().map(|&i| rows[i].id.clone()).collect();
        reposition(&mut ids, plan.from_pos, plan.to_pos);
        ids.join(">")
    }

    fn tree() -> Vec<RowSpec> {
        vec![
            header("w1"),
            thread("a", "w1"),
            thread("b", "w1"),
            thread("c", "w1"),
            header("w2"),
            thread("d", "w2"),
        ]
    }

    #[test]
    fn drag_crosses_several_rows_at_once() {
        let rows = tree();
        // One row down: 42px is the lower half of `b`.
        assert_eq!(dragged(&rows, 1, 42.0), "b>a>c");
        // Two rows down: 80px is the lower half of `c`.
        assert_eq!(dragged(&rows, 1, 80.0), "b>c>a");
        // Two rows up from the bottom of the group.
        assert_eq!(dragged(&rows, 3, -56.0), "c>a>b");
    }

    #[test]
    fn drag_inside_the_own_cell_changes_nothing() {
        let rows = tree();
        assert_eq!(dragged(&rows, 1, 8.0), "a>b>c");
        assert_eq!(dragged(&rows, 1, 20.0), "a>b>c");
    }

    #[test]
    fn thread_drag_clamps_at_the_workspace_bounds() {
        let rows = tree();
        // Far below the group: still the last thread of w1, never past the w2
        // header and never into w2's own threads.
        assert_eq!(dragged(&rows, 1, 4000.0), "b>c>a");
        // Far above it: the w1 header keeps its place in front.
        assert_eq!(dragged(&rows, 3, -4000.0), "c>a>b");
    }

    #[test]
    fn workspace_drag_walks_over_the_thread_rows_it_hovers() {
        let rows = tree();
        // 120px is the upper half of the w2 header: w1 stays in front.
        assert_eq!(dragged(&rows, 0, 120.0), "w1>w2");
        // 140px is its lower half: the two headers trade places.
        assert_eq!(dragged(&rows, 0, 140.0), "w2>w1");
    }

    #[test]
    fn slot_walk_uses_each_rows_own_pitch() {
        let rows = tree();
        // Below the threads the header still costs its own 34px, so 115px is
        // inside it; a uniform 28px pitch would read the same pointer as `d`.
        assert_eq!(row_slot(&rows, 1, 115.0, 28.0, 34.0), (4, true));
        // The midline of that header above the first thread sits at 17px, not
        // at 14px.
        assert_eq!(row_slot(&rows, 1, -15.0, 28.0, 34.0), (0, true));
        assert_eq!(row_slot(&rows, 1, -20.0, 28.0, 34.0), (0, false));
        // Between threads it is the 28px pitch that decides the half.
        assert_eq!(row_slot(&rows, 3, -27.0, 28.0, 34.0), (2, false));
        assert_eq!(row_slot(&rows, 3, -13.0, 28.0, 34.0), (2, true));
        // Past the last row the drop stays on the tail.
        assert_eq!(row_slot(&rows, 4, 4000.0, 28.0, 34.0), (5, true));
    }

    #[test]
    fn group_order_rewrites_only_the_group() {
        let mut order = vec!["a", "other", "b", "c"]
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<String>>();
        let next = ["c", "a", "b"].iter().map(|id| id.to_string()).collect::<Vec<String>>();
        assert!(reorder_in_order(&mut order, &next));
        assert_eq!(order, vec!["c", "other", "a", "b"]);

        // An id the manual list has never seen joins the tail and still lands
        // where the drag put it.
        let mut order = vec!["a".to_string(), "b".to_string()];
        let next = ["b", "a", "new"].iter().map(|id| id.to_string()).collect::<Vec<String>>();
        assert!(reorder_in_order(&mut order, &next));
        assert_eq!(order, vec!["b", "a", "new"]);

        // A list that cannot host the whole sequence is left alone.
        let mut order = vec!["a".to_string(), "a".to_string()];
        let next = ["a".to_string(), "b".to_string()];
        assert!(!reorder_in_order(&mut order, &next));
        assert_eq!(order, vec!["a", "a", "b"]);
    }
}
