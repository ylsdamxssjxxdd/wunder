//! Single-agent navigation: thread ordering comes straight from storage
//! (recency), so the module keeps stable agent ids, the pet projection and
//! the sidebar tree projection (workspaces with their threads).
use crate::{MainWindow, SidebarMetrics, SidebarRow, WorkspaceCard};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    sync::Arc,
};
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
    /// Only the selected workspace reveals its thread tree.
    static EXPANDED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Dragged workspace order, persisted through the native workspace façade.
    static WORKSPACE_ORDER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Manual thread display order (session ids), same semantics as above.
    static THREAD_ORDER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Multi-select checked threads (session ids), the desktop twin of the
    /// web tree's selection set. Only meaningful while the tree is in
    /// selection mode; every projection re-derives each row's `checked` from
    /// it so status refreshes and reorders never drop a tick.
    static THREAD_SELECTION: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Last selected local workspace id, remembered while the cloud device is
    /// active so switching back lands on the same workspace instead of index 0.
    static LAST_LOCAL_WORKSPACE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Monotonic guard for asynchronous workspace catalogue reads. A stale
    /// SQLite result must never replace a newer catalogue result.
    static WORKSPACE_REFRESH_GENERATION: Cell<u64> = const { Cell::new(0) };
}

fn next_workspace_refresh_generation() -> u64 {
    WORKSPACE_REFRESH_GENERATION.with(|slot| {
        let next = slot.get().wrapping_add(1);
        slot.set(next);
        next
    })
}

fn workspace_refresh_is_current(request: u64) -> bool {
    WORKSPACE_REFRESH_GENERATION.with(|slot| slot.get() == request)
}

fn selected_workspace_id(app: &MainWindow) -> Option<String> {
    usize::try_from(app.get_selected_workspace())
        .ok()
        .and_then(|index| app.get_workspaces().row_data(index))
        .map(|card| card.workspace_id.to_string())
}

fn refreshed_workspace_selection(
    selected_id: Option<&str>,
    workspace_ids: &[String],
    cloud_target: bool,
) -> i32 {
    selected_id
        .and_then(|id| {
            workspace_ids
                .iter()
                .position(|candidate| candidate.as_str() == id)
        })
        .map(|index| index as i32)
        .unwrap_or_else(|| {
            if cloud_target || workspace_ids.is_empty() {
                -1
            } else {
                0
            }
        })
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
            2 => "cloud".to_string(),
            _ => String::new(),
        },
    }
}

/// Rows that may take one another's place: every workspace header forms one
/// group, while threads only ever reorder inside their own workspace.
fn group_key(row: &RowSpec) -> (i32, &str) {
    (
        if row.kind == 0 || row.kind == 2 {
            0
        } else {
            row.kind
        },
        if row.kind == 1 {
            row.bucket.as_str()
        } else {
            ""
        },
    )
}

/// A workspace header drags only against another header. Its child threads are
/// intentionally excluded: visual hierarchy must not turn a drag from the
/// cloud row into a thread move target.
fn is_workspace_header(row: &RowSpec) -> bool {
    row.kind == 0 || row.kind == 2
}

fn row_height(kind: i32, thread_px: f32, group_px: f32) -> f32 {
    if kind == 1 {
        thread_px
    } else {
        group_px
    }
}

/// The row the pointer sits in, plus whether the drop lands after it. `dy` is
/// measured from the top of the dragged row's cell and the tree stacks rows
/// without gaps, so accumulating each row's own height is the whole geometry.
fn row_slot(
    rows: &[RowSpec],
    from: usize,
    dy: f32,
    thread_px: f32,
    group_px: f32,
) -> (usize, bool) {
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
        .filter(|&index| {
            if is_workspace_header(&rows[from]) {
                is_workspace_header(&rows[index])
            } else {
                group_key(&rows[index]) == key
            }
        })
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

/// A thread may be archived whenever it is not actively owned by another
/// instance. The native facade settles running/queued work as part of the
/// archive transaction, so a stale status cannot make a thread permanent.
fn thread_archivable(status: &str, locked: bool) -> bool {
    let _ = status;
    !locked
}

/// Rebuild the sidebar from the workspace facade plus the bounded conversation
/// list. `desktop` is only needed on the first call and after workspace CRUD;
/// row rebuilds reuse the existing cards so selection changes cost no I/O.
/// It is an `Arc` because the refresh owns a background worker; accepting a
/// borrowed facade here would let that reference escape into the worker.
pub(crate) fn project_sidebar(app: &MainWindow, desktop: Option<Arc<NativeDesktop>>) {
    if let Some(desktop) = desktop {
        // Workspace counting opens SQLite too. Rendering a refreshed session
        // catalogue must not make the Slint event loop wait for that I/O.
        let request = next_workspace_refresh_generation();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = desktop.list_workspaces();
            let _ = weak.upgrade_in_event_loop(move |app| {
                if !workspace_refresh_is_current(request) {
                    return;
                }
                match result {
                    Ok(list) => {
                        // Resolve the selected id at commit time. A user can
                        // click another workspace while the HDD is reading;
                        // the request-start index is no longer authoritative.
                        let selected_id = selected_workspace_id(&app);
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
                        let workspace_ids: Vec<String> = app
                            .get_workspaces()
                            .iter()
                            .map(|card| card.workspace_id.to_string())
                            .collect();
                        app.set_selected_workspace(refreshed_workspace_selection(
                            selected_id.as_deref(),
                            &workspace_ids,
                            crate::workspace_files::is_cloud_target(),
                        ));
                        rebuild_rows(&app);
                    }
                    Err(error) => app.set_status(format!("无法读取工作区：{error}").into()),
                }
            });
        });
        // Retain the current sidebar projection until the background refresh
        // returns. Rebuilding here is pure model work and gives clicks an
        // immediate visual response without blocking for SQLite.
        rebuild_rows(app);
        return;
    }
    // A direct UI projection (workspace click, archive completion, target
    // switch) is newer than every outstanding catalogue read. Invalidate the
    // worker before applying the new rows so a slow HDD response cannot jump
    // the selection back afterwards.
    next_workspace_refresh_generation();
    rebuild_rows(app);
}

fn rebuild_rows(app: &MainWindow) {
    let cards_model = app.get_workspaces();
    // The device selector shows exactly one device tree. Cloud is a device of
    // its own with a single pinned workspace row; a signed-out cloud target
    // cannot host a tree, so it falls back to the local device.
    let cloud_available = app.get_cloud_logged_in();
    let cloud_active = cloud_available && crate::workspace_files::is_cloud_target();
    let cloud = cloud_active.then(|| cloud_row(app));
    let selected = if crate::workspace_files::is_cloud_target() {
        None
    } else {
        usize::try_from(app.get_selected_workspace())
            .ok()
            .filter(|&index| index < cards_model.row_count())
    };
    if let Some(card) = selected.and_then(|index| cards_model.row_data(index)) {
        LAST_LOCAL_WORKSPACE.with(|slot| *slot.borrow_mut() = Some(card.workspace_id.to_string()));
    }
    let conversations = app.get_conversations();
    let active = app.get_active_session_id().to_string();
    // Multi-select bookkeeping: drop ticks for conversations that no longer
    // exist, then re-derive every row's `checked` from the surviving set.
    let existing: HashSet<String> = conversations
        .iter()
        .map(|conv| conv.id.to_string())
        .collect();
    THREAD_SELECTION.with(|slot| {
        slot.borrow_mut().retain(|id| existing.contains(id));
    });
    let selection_mode = app.get_sidebar_selection_mode();
    let mut rows: Vec<SidebarRow> = Vec::with_capacity(cards_model.row_count() * 4 + 1);
    let cloud_threads = app.get_cloud_thread_rows();
    // Persisted manual order; un-ordered ids keep their facade position. The
    // order list still carries both devices so switching back keeps the local
    // ordering, but only the active device's rows are emitted.
    let cards: Vec<(usize, WorkspaceCard)> = cards_model.iter().enumerate().collect();
    let mut ordered_keys = Vec::with_capacity(cards.len() + 1);
    let cloud_expanded = EXPANDED.with(|slot| slot.borrow().contains("cloud"));
    if cloud_available {
        ordered_keys.push("cloud".to_string());
    }
    ordered_keys.extend(cards.iter().map(|(_, card)| card.workspace_id.to_string()));
    WORKSPACE_ORDER.with(|slot| {
        let order = slot.borrow();
        ordered_keys.sort_by_key(|key| order_rank(&order, key));
    });
    EXPANDED.with(|slot| {
        let expanded = slot.borrow();
        for key in ordered_keys {
            if key == "cloud" {
                if !cloud_active {
                    continue;
                }
                let Some(mut cloud) = cloud.clone() else {
                    continue;
                };
                cloud.thread_count = cloud_threads.row_count() as i32;
                cloud.expanded = cloud_expanded;
                rows.push(cloud);
                if cloud_expanded {
                    for (index, thread) in cloud_threads
                        .iter()
                        .take(MAX_TREE_ROWS.saturating_sub(rows.len()))
                        .enumerate()
                    {
                        rows.push(SidebarRow {
                            kind: 3,
                            workspace_id: "".into(),
                            name: "".into(),
                            icon: "".into(),
                            color: "".into(),
                            thread_count: 0,
                            expanded: false,
                            selected: false,
                            thread_id: thread.id.clone(),
                            title: thread.title.clone(),
                            status: thread.status.clone(),
                            phase: crate::runtime_phase::Phase::of(thread.status.as_str()).code(),
                            active: app.get_cloud_chat_active()
                                && app.get_cloud_active_thread_id() == thread.id,
                            payload: index as i32,
                            origin: "cloud".into(),
                            archivable: thread_archivable(thread.status.as_str(), false),
                            checked: false,
                            path: "".into(),
                        });
                    }
                }
                continue;
            }
            if cloud_active {
                continue;
            }
            let Some((wi, card)) = cards
                .iter()
                .find(|(_, card)| card.workspace_id.as_str() == key)
                .cloned()
            else {
                continue;
            };
            let expanded = selected == Some(wi) && expanded.contains(card.workspace_id.as_str());
            rows.push(SidebarRow {
                kind: 0,
                workspace_id: card.workspace_id.clone(),
                name: card.name.clone(),
                icon: card.icon.clone(),
                color: card.color.clone(),
                thread_count: card.thread_count,
                expanded,
                selected: selected == Some(wi),
                thread_id: "".into(),
                title: "".into(),
                status: "".into(),
                phase: -1,
                active: false,
                payload: wi as i32,
                origin: "".into(),
                archivable: false,
                checked: false,
                path: card.root_path.clone(),
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
                let id = conv.id.to_string();
                let archivable = thread_archivable(conv.runtime_status.as_str(), conv.locked);
                let checked = selection_mode
                    && THREAD_SELECTION.with(|slot| slot.borrow().contains(id.as_str()));
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
                    phase: conv.runtime_phase,
                    active: conv.id == active,
                    payload: ci as i32,
                    origin: conv.origin.clone(),
                    archivable,
                    checked,
                    path: "".into(),
                });
            }
        }
    });
    let archivable_count = rows
        .iter()
        .filter(|row| row.kind == 1 && row.archivable)
        .count() as i32;
    app.set_sidebar_rows(ModelRc::new(VecModel::from(rows)));
    app.set_sidebar_archivable_count(archivable_count);
    app.set_sidebar_selected_count(THREAD_SELECTION.with(|slot| slot.borrow().len()) as i32);
}

/// Point update for one local thread's runtime status. The tree rows are a
/// snapshot of the conversation list, so a live transition (running, waiting,
/// done) must be patched into the matching row here; waiting for the next
/// catalogue rebuild is what left entries stale until a thread switch.
pub(crate) fn update_thread_status(
    app: &MainWindow,
    session: &str,
    status: &str,
    phase: crate::runtime_phase::Phase,
) {
    patch_thread_status(&app.get_sidebar_rows(), session, status, phase);
}

fn patch_thread_status(
    rows: &ModelRc<SidebarRow>,
    session: &str,
    status: &str,
    phase: crate::runtime_phase::Phase,
) -> bool {
    let Some(index) = rows
        .iter()
        .position(|row| row.kind == 1 && row.thread_id.as_str() == session)
    else {
        return false;
    };
    let Some(mut row) = rows.row_data(index) else {
        return false;
    };
    if row.status.as_str() == status && row.phase == phase.code() {
        return false;
    }
    row.status = status.into();
    row.phase = phase.code();
    rows.set_row_data(index, row);
    true
}

/// The pinned "cloud workspace" row: a sentinel (kind 2) whose click points
/// the working-directory panel back at the cloud account workspace.
fn cloud_row(app: &MainWindow) -> SidebarRow {
    let zh = app.global::<crate::I18n>().get_zh();
    SidebarRow {
        kind: 2,
        workspace_id: "".into(),
        name: if zh {
            "云端工作区".into()
        } else {
            "Cloud workspace".into()
        },
        icon: "cloud".into(),
        color: "".into(),
        thread_count: 0,
        expanded: true,
        selected: crate::workspace_files::is_cloud_target(),
        thread_id: "".into(),
        title: "".into(),
        status: "".into(),
        phase: -1,
        active: false,
        payload: -1,
        origin: "".into(),
        archivable: false,
        checked: false,
        path: "".into(),
    }
}

/// Resolve the local workspace a device switch should land on: the last local
/// selection (by id, so catalogue refreshes cannot shift it), else the first.
fn selected_local_workspace_index(app: &MainWindow) -> Option<usize> {
    let cards = app.get_workspaces();
    let last = LAST_LOCAL_WORKSPACE.with(|slot| slot.borrow().clone());
    if let Some(id) = last {
        if let Some(index) = cards
            .iter()
            .position(|card| card.workspace_id == id.as_str())
        {
            return Some(index);
        }
    }
    (cards.row_count() > 0).then_some(0)
}

/// Enter the cloud device: pin the cloud workspace row, drop the local
/// selection and point the working-directory panel at the cloud root.
fn activate_cloud_device(app: &MainWindow, api: Arc<NativeDesktop>, force_expand: bool) {
    let should_expand = EXPANDED.with(|slot| {
        let mut expanded = slot.borrow_mut();
        let was_expanded = expanded.contains("cloud");
        expanded.clear();
        if force_expand || !was_expanded {
            expanded.insert("cloud".to_string());
        }
        force_expand || !was_expanded
    });
    app.set_selected_workspace(-1);
    let was_cloud = app.get_cloud_chat_active();
    if !was_cloud {
        crate::cloud_threads::leave(app);
    }
    THREAD_SELECTION.with(|slot| slot.borrow_mut().clear());
    app.set_sidebar_selection_mode(false);
    crate::workspace_files::set_target(app, api, crate::workspace_files::Target::Cloud);
    project_sidebar(app, None);
    if should_expand && !was_cloud {
        app.invoke_cloud_open_threads();
    }
}

/// Enter a local workspace: mirror its folder in the working-directory panel,
/// expand its thread tree and leave the cloud conversation view.
fn activate_local_workspace(
    app: &MainWindow,
    api: Arc<NativeDesktop>,
    workspace: usize,
    force_expand: bool,
) {
    let Some(card) = app.get_workspaces().row_data(workspace) else {
        return;
    };
    let id = card.workspace_id.to_string();
    let was_cloud = app.get_cloud_chat_active();
    EXPANDED.with(|slot| {
        let mut expanded = slot.borrow_mut();
        let was_expanded = expanded.contains(id.as_str());
        expanded.clear();
        if force_expand || !was_expanded {
            expanded.insert(id.clone());
        }
    });
    THREAD_SELECTION.with(|slot| slot.borrow_mut().clear());
    app.set_sidebar_selection_mode(false);
    crate::cloud_threads::leave(app);
    if was_cloud {
        crate::native_chat::prepare_external_timeline(app);
        app.set_selected_model_key("".into());
        app.set_reasoning_effort("default".into());
        crate::entity_state::restore_agent(app, app.get_active_agent_id().as_str());
    }
    app.set_selected_workspace(workspace as i32);
    LAST_LOCAL_WORKSPACE.with(|slot| *slot.borrow_mut() = Some(id));
    let root = card.root_path.to_string();
    if !root.is_empty() {
        crate::workspace_files::set_target(app, api, crate::workspace_files::Target::Local(root));
    }
    project_sidebar(app, None);
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
    let Some(workspace_id) = workspace_id else {
        return;
    };
    for (index, card) in app.get_workspaces().iter().enumerate() {
        if card.workspace_id.as_str() == workspace_id {
            app.set_selected_workspace(index as i32);
            // The focused thread's workspace must show its threads, so the
            // focus implicitly expands the workspace.
            EXPANDED.with(|slot| {
                let mut expanded = slot.borrow_mut();
                expanded.clear();
                expanded.insert(workspace_id.to_string());
            });
            let root = card.root_path.to_string();
            if !root.is_empty() {
                crate::workspace_files::set_target(
                    app,
                    api,
                    crate::workspace_files::Target::Local(root),
                );
            }
            project_sidebar(app, None);
            return;
        }
    }
}

/// Sidebar interactions and the workspace entry points on the window.
pub fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let layout_api = desktop.clone();
    std::thread::spawn(move || {
        let ratio = layout_api.sidebar_split_ratio().unwrap_or(0.6);
        let navigation = layout_api.navigation_order().unwrap_or_default();
        let order = navigation.threads;
        let workspace_order = navigation.workspaces;
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_sidebar_split_ratio(ratio);
            THREAD_ORDER.with(|slot| *slot.borrow_mut() = order);
            WORKSPACE_ORDER.with(|slot| *slot.borrow_mut() = workspace_order);
            project_sidebar(&app, None);
        });
    });
    // Coalesce quick keyboard nudges and drags; one worker owns each save.
    let layout_timer = std::rc::Rc::new(slint::Timer::default());
    let weak = app.as_weak();
    let layout_api = desktop.clone();
    app.on_sidebar_layout_changed(move || {
        let weak = weak.clone();
        let api = layout_api.clone();
        layout_timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(400),
            move || {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                let ratio = app.get_sidebar_split_ratio();
                let api = api.clone();
                std::thread::spawn(move || {
                    let _ = api.save_sidebar_split_ratio(ratio);
                });
            },
        );
    });
    let api = desktop.clone();
    let weak = app.as_weak();
    app.on_select_row(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(row) = app.get_sidebar_rows().row_data(index as usize) else {
            return;
        };
        if row.kind == 2 {
            // Cloud workspace row: toggles its thread tree without leaving
            // the cloud device.
            activate_cloud_device(&app, api.clone(), false);
        } else if row.kind == 0 {
            // A workspace header is also its tree disclosure. Do not open its
            // first thread: on Win7/HDD that would start a lease and history
            // read while the user is only trying to reveal the tree.
            if let Ok(workspace) = usize::try_from(row.payload) {
                activate_local_workspace(&app, api.clone(), workspace, false);
            }
        } else if row.kind == 3 {
            app.set_cloud_thread_index(row.payload + 1);
            app.set_cloud_chat_active(true);
            // The selected marker is projected from the thread index, so
            // rebuild immediately instead of waiting for the next catalog
            // refresh. This keeps the tree and the content panel convergent.
            crate::navigation_ui::project_sidebar(&app, None);
            app.invoke_cloud_select_thread(row.payload + 1);
        } else {
            app.invoke_select_conversation(row.payload);
        }
    });
    let weak = app.as_weak();
    let device_api = desktop.clone();
    app.on_select_device(move |device| {
        let Some(app) = weak.upgrade() else { return };
        match device {
            1 if app.get_cloud_logged_in() => {
                if crate::workspace_files::is_cloud_target() {
                    return;
                }
                activate_cloud_device(&app, device_api.clone(), true);
            }
            0 => {
                if !crate::workspace_files::is_cloud_target() {
                    return;
                }
                if let Some(workspace) = selected_local_workspace_index(&app) {
                    activate_local_workspace(&app, device_api.clone(), workspace, true);
                }
            }
            _ => {}
        }
    });
    let weak = app.as_weak();
    // Drag reorder: the grabbed row reports how far the pointer travelled, the
    // tree pitch says which row that lands on, and the row takes that slot in
    // its own group. Threads cannot leave their workspace; the cloud header
    // shares the local-workspace group and persists as the `cloud` sentinel.
    let order_api = desktop.clone();
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
            || !matches!(specs[from].kind, 0 | 1 | 2)
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
            if specs[from].kind == 0 || specs[from].kind == 2 {
                let api = order_api.clone();
                let weak = app.as_weak();
                std::thread::spawn(move || {
                    let result = api.save_workspace_order(&next);
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if let Err(error) = result {
                            app.set_status(format!("无法保存工作区顺序：{error}").into());
                        }
                    });
                });
            } else {
                let ids: Vec<String> = THREAD_ORDER.with(|slot| {
                    slot.borrow()
                        .iter()
                        .filter(|id| {
                            app.get_conversations()
                                .iter()
                                .any(|row| row.id == id.as_str())
                        })
                        .take(100)
                        .cloned()
                        .collect()
                });
                THREAD_ORDER.with(|slot| *slot.borrow_mut() = ids.clone());
                let api = order_api.clone();
                let weak = app.as_weak();
                std::thread::spawn(move || {
                    let result = api.save_thread_order(&ids);
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if let Err(error) = result {
                            app.set_status(format!("无法保存线程顺序：{error}").into());
                        }
                    });
                });
            }
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
        match action {
            0 => {
                crate::cloud_threads::leave(&app);
                focus_workspace_of(&app, api.clone(), Some(card.workspace_id.as_str()));
                app.invoke_new_thread();
            }
            1 => crate::workspace_ui::open_edit(&app, workspace as usize),
            2 => {
                let root = card.root_path.to_string();
                let api = api.clone();
                let weak = weak.clone();
                std::thread::spawn(move || {
                    let result = api.open_workspace_resource(&root, "");
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if let Err(error) = result {
                            app.set_status(format!("无法打开文件夹：{error}").into());
                        }
                    });
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
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Err(error) = result {
                    app.set_status(format!("无法重命名会话：{error}").into());
                }
                app.invoke_refresh_chat();
            });
        });
    });
    let weak = app.as_weak();
    let api = desktop.clone();
    app.on_archive_thread(move |index| {
        if weak.upgrade().is_none() {
            return;
        }
        let id = index.trim().to_string();
        if id.is_empty() {
            return;
        }
        let api = api.clone();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let result = api.archive_session(&id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Err(error) = result {
                    app.set_status(format!("无法归档会话：{error}").into());
                    return;
                }
                // A tick on the archived thread must not survive into the
                // selection set.
                THREAD_SELECTION.with(|slot| {
                    slot.borrow_mut().remove(id.as_str());
                });
                // Decide from the state that exists *now*, rather than from
                // the click-time snapshot. The user may have opened another
                // thread while the archive transaction was on disk.
                if app.get_active_session_id().as_str() == id.as_str() {
                    app.set_active_session_id("".into());
                }
                app.invoke_refresh_chat();
                crate::navigation_ui::project_sidebar(&app, None);
            });
        });
    });
    // Multi-select (web alignment): the tree reports intent, this module owns
    // the checked set, the mode flag and the counts. Toggles re-project so a
    // concurrent status refresh can never resurrect a stale box.
    let weak = app.as_weak();
    app.on_sidebar_begin_selection(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_sidebar_selection_mode(true);
        crate::navigation_ui::project_sidebar(&app, None);
    });
    let weak = app.as_weak();
    app.on_sidebar_exit_selection(move || {
        let Some(app) = weak.upgrade() else { return };
        THREAD_SELECTION.with(|slot| slot.borrow_mut().clear());
        app.set_sidebar_selection_mode(false);
        app.set_sidebar_selected_count(0);
        crate::navigation_ui::project_sidebar(&app, None);
    });
    let weak = app.as_weak();
    app.on_sidebar_toggle_selection(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(conv) = app.get_conversations().row_data(index as usize) else {
            return;
        };
        let id = conv.id.to_string();
        let count = THREAD_SELECTION.with(|slot| {
            let mut selection = slot.borrow_mut();
            if !selection.remove(id.as_str()) {
                selection.insert(id);
            }
            selection.len() as i32
        });
        app.set_sidebar_selected_count(count);
        crate::navigation_ui::project_sidebar(&app, None);
    });
    let weak = app.as_weak();
    app.on_sidebar_select_all(move || {
        let Some(app) = weak.upgrade() else { return };
        // Only rows the archive gate allows take part, as on the web.
        let archivable: Vec<String> = app
            .get_sidebar_rows()
            .iter()
            .filter(|row| row.kind == 1 && row.archivable)
            .map(|row| row.thread_id.to_string())
            .collect();
        let all_selected = !archivable.is_empty()
            && THREAD_SELECTION.with(|slot| {
                let selection = slot.borrow();
                archivable.iter().all(|id| selection.contains(id))
            });
        let count = THREAD_SELECTION.with(|slot| {
            let mut selection = slot.borrow_mut();
            if all_selected {
                for id in &archivable {
                    selection.remove(id);
                }
            } else {
                for id in &archivable {
                    selection.insert(id.clone());
                }
            }
            selection.len() as i32
        });
        app.set_sidebar_selected_count(count);
        crate::navigation_ui::project_sidebar(&app, None);
    });
    let weak = app.as_weak();
    let api = desktop.clone();
    app.on_sidebar_archive_selected(move || {
        if weak.upgrade().is_none() {
            return;
        }
        let ids: Vec<String> =
            THREAD_SELECTION.with(|slot| slot.borrow().iter().cloned().collect());
        if ids.is_empty() {
            return;
        }
        let api = api.clone();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let mut archived = 0usize;
            let mut archived_ids = HashSet::new();
            let mut first_error: Option<String> = None;
            for id in &ids {
                match api.archive_session(id) {
                    Ok(()) => {
                        archived += 1;
                        archived_ids.insert(id.clone());
                    }
                    Err(error) => {
                        if first_error.is_none() {
                            first_error = Some(format!("{error}"));
                        }
                    }
                }
            }
            let _ = weak.upgrade_in_event_loop(move |app| {
                THREAD_SELECTION.with(|slot| slot.borrow_mut().clear());
                app.set_sidebar_selection_mode(false);
                app.set_sidebar_selected_count(0);
                if let Some(error) = first_error {
                    app.set_status(format!("已归档 {archived} 个线程，其余失败：{error}").into());
                } else {
                    app.set_status(format!("已归档 {archived} 个线程").into());
                }
                // A newer user selection wins over the old multi-select
                // batch. Only clear the session that was actually archived.
                if archived_ids.contains(app.get_active_session_id().as_str()) {
                    app.set_active_session_id("".into());
                }
                if archived > 0 {
                    app.invoke_refresh_chat();
                }
                crate::navigation_ui::project_sidebar(&app, None);
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
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Err(error) = result {
                    app.set_status(format!("无法打开文件夹：{error}").into());
                }
            });
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
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Err(error) = result {
                    app.set_status(format!("无法打开文件夹：{error}").into());
                }
            });
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

    fn thread_row(id: &str, status: &str) -> SidebarRow {
        SidebarRow {
            kind: 1,
            workspace_id: "w1".into(),
            name: "".into(),
            icon: "".into(),
            color: "".into(),
            thread_count: 0,
            expanded: false,
            selected: false,
            thread_id: id.into(),
            title: id.into(),
            status: status.into(),
            phase: crate::runtime_phase::Phase::of(status).code(),
            active: false,
            payload: 0,
            origin: "".into(),
            archivable: true,
            checked: false,
            path: "".into(),
        }
    }

    /// The sidebar tree snapshots the conversation list, so a live status
    /// transition is patched into the one matching tree row: the marker moves
    /// immediately instead of waiting for the next catalogue rebuild.
    #[test]
    fn status_patch_rewrites_only_the_matching_thread_row() {
        let rows = ModelRc::new(VecModel::from(vec![
            thread_row("a", "idle"),
            thread_row("b", "任务完成"),
            SidebarRow {
                kind: 3,
                ..thread_row("a", "idle")
            },
        ]));
        let running = crate::runtime_phase::Phase::Running;
        assert!(patch_thread_status(&rows, "a", "正在生成…", running));
        assert_eq!(rows.row_data(0).unwrap().status, "正在生成…");
        assert_eq!(rows.row_data(0).unwrap().phase, running.code());
        assert_eq!(rows.row_data(1).unwrap().status, "任务完成");
        // Cloud rows carry their own status pipeline and stay untouched.
        assert_eq!(rows.row_data(2).unwrap().status, "idle");
        // A repeated or unknown update changes nothing and reports so.
        assert!(!patch_thread_status(&rows, "a", "正在生成…", running));
        assert!(!patch_thread_status(&rows, "gone", "正在生成…", running));
    }

    #[test]
    fn refreshed_workspace_selection_keeps_the_stable_id() {
        let ids = ["workspace-a", "workspace-b", "workspace-c"].map(str::to_string);
        assert_eq!(
            refreshed_workspace_selection(Some("workspace-b"), &ids, false),
            1
        );
    }

    #[test]
    fn refreshed_workspace_selection_never_maps_an_invalid_choice_to_the_tail() {
        let ids = ["workspace-a", "workspace-b", "workspace-c"].map(str::to_string);
        assert_eq!(refreshed_workspace_selection(Some("gone"), &ids, false), 0);
        assert_eq!(refreshed_workspace_selection(None, &ids, true), -1);
        assert_eq!(refreshed_workspace_selection(None, &[], false), -1);
    }

    #[test]
    fn agent_key_normalizes_legacy_ids() {
        assert_eq!(agent_key(Some("default")), agent_key(None));
        assert_eq!(agent_key(Some("__default__")), "__default__");
        assert_eq!(agent_key(Some("custom")), "custom");
    }

    fn header(id: &str) -> RowSpec {
        RowSpec {
            kind: 0,
            bucket: String::new(),
            id: id.to_string(),
        }
    }

    fn cloud_header() -> RowSpec {
        RowSpec {
            kind: 2,
            bucket: String::new(),
            id: "cloud".to_string(),
        }
    }

    fn thread(id: &str, workspace: &str) -> RowSpec {
        RowSpec {
            kind: 1,
            bucket: workspace.to_string(),
            id: id.to_string(),
        }
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
    fn cloud_header_reorders_with_local_workspace_headers() {
        let rows = vec![
            cloud_header(),
            header("w1"),
            thread("a", "w1"),
            header("w2"),
        ];
        assert_eq!(dragged(&rows, 0, 60.0), "w1>cloud>w2");
        assert_eq!(dragged(&rows, 3, -4000.0), "w2>cloud>w1");
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
        let next = ["c", "a", "b"]
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<String>>();
        assert!(reorder_in_order(&mut order, &next));
        assert_eq!(order, vec!["c", "other", "a", "b"]);

        // An id the manual list has never seen joins the tail and still lands
        // where the drag put it.
        let mut order = vec!["a".to_string(), "b".to_string()];
        let next = ["b", "a", "new"]
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<String>>();
        assert!(reorder_in_order(&mut order, &next));
        assert_eq!(order, vec!["b", "a", "new"]);

        // A list that cannot host the whole sequence is left alone.
        let mut order = vec!["a".to_string(), "a".to_string()];
        let next = ["a".to_string(), "b".to_string()];
        assert!(!reorder_in_order(&mut order, &next));
        assert_eq!(order, vec!["a", "a", "b"]);
    }
}
