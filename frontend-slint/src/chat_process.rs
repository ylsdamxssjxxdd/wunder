//! On-demand process pages. Each expanded process retains one bounded page; reads and tool
//! parsing run off the UI thread, and replies are scoped to the selected thread.
use crate::{MainWindow, ProcessEntry, TimelineRow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{mpsc::sync_channel, Arc},
    time::Duration,
};
use wunder_desktop::{native::NativeProcessPage, NativeDesktop};

#[derive(Default)]
struct Cursor {
    first: Option<i64>,
    last: Option<i64>,
    busy: bool,
    retry: i32,
    entries: ModelRc<ProcessEntry>,
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let pending = Rc::new(Cell::new(0usize));
    bind(app, api.clone(), false, pending.clone());
    bind(app, api, true, pending);
}

fn rows(app: &MainWindow, child: bool) -> ModelRc<TimelineRow> {
    if child {
        app.get_subagent_rows()
    } else {
        app.get_timeline()
    }
}

fn session(app: &MainWindow, child: bool) -> String {
    if child {
        app.get_process_child_session().to_string()
    } else {
        app.get_active_session_id().to_string()
    }
}

fn bind(app: &MainWindow, api: Arc<NativeDesktop>, child: bool, pending: Rc<Cell<usize>>) {
    let cursors = Rc::new(RefCell::new(HashMap::<(String, String), Cursor>::new()));
    let weak = app.as_weak();
    let callback = move |payload: i32, direction: i32| {
        let Some(app) = weak.upgrade() else { return };
        let model = rows(&app, child);
        let Some(mut row) = usize::try_from(payload)
            .ok()
            .and_then(|index| model.row_data(index))
        else {
            return;
        };
        if !row.open || row.process_root.is_empty() {
            return;
        }
        if row.process_local && direction == 0 && !row.process_error {
            return;
        }
        let target = session(&app, child);
        let root = row.process_root.to_string();
        let key = (target.clone(), root.clone());
        let mut held = cursors.borrow_mut();
        // A switched thread has no retained page state. Prune both axes rather
        // than growing a cache as the user visits more conversations.
        held.retain(|(session_id, root_id), _| {
            session_id == &target && model.iter().any(|row| row.process_root == root_id.as_str())
        });
        if pending.get() >= 4 {
            return;
        }
        let cursor = held.entry(key.clone()).or_default();
        // The main transcript model is stable across navigation; its process
        // child model identifies the actual loaded page, including A → B → A.
        if cursor.entries != row.process_entries {
            *cursor = Cursor {
                entries: row.process_entries.clone(),
                ..Default::default()
            };
        }
        let direction = if row.process_error && direction == 0 {
            cursor.retry
        } else {
            direction
        };
        if cursor.busy || (direction == 0 && row.process_paged && !row.process_error) {
            return;
        }
        if direction == 2 && row.process_newer {
            return;
        }
        let (before, after) = match direction {
            -1 if row.process_older => (cursor.first, None),
            1 if row.process_newer => (None, cursor.last),
            -1 | 1 => return,
            _ => (None, None),
        };
        cursor.busy = true;
        cursor.retry = direction;
        pending.set(pending.get() + 1);
        drop(held);
        let entries = row.process_entries.clone();
        // Background tail refreshes keep the existing layout and scroll anchor.
        row.process_loading = direction != 2;
        row.process_error = false;
        model.set_row_data(payload as usize, row);
        let (tx, rx) = sync_channel(1);
        let backend = api.clone();
        let lookup = target.clone();
        let root_id = root.clone();
        std::thread::spawn(move || {
            let _ = tx.send(backend.chat_process_page(&lookup, &root_id, before, after));
        });
        let weak = app.as_weak();
        let cursors = cursors.clone();
        let pending = pending.clone();
        poll(move || {
            let result = match rx.try_recv() {
                Ok(value) => value,
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                Err(_) => Err(anyhow::anyhow!("process reader stopped")),
            };
            pending.set(pending.get().saturating_sub(1));
            if let Some(cursor) = cursors
                .borrow_mut()
                .get_mut(&key)
                .filter(|cursor| cursor.entries == entries)
            {
                cursor.busy = false;
            }
            let Some(app) = weak.upgrade() else {
                return true;
            };
            if session(&app, child) != target {
                return true;
            }
            let model = rows(&app, child);
            let Some(index) = model
                .iter()
                .position(|row| row.process_root == root.as_str())
            else {
                return true;
            };
            let Some(mut row) = model.row_data(index) else {
                return true;
            };
            if row.process_entries != entries {
                return true;
            }
            row.process_loading = false;
            match result {
                Ok(page) => {
                    let (page_first, page_last) = (page.first, page.last);
                    apply_page(&mut row, page, direction);
                    if let Some(cursor) = cursors.borrow_mut().get_mut(&key) {
                        cursor.first = page_first;
                        cursor.last = page_last;
                        cursor.entries = row.process_entries.clone();
                    }
                }
                Err(_) => {
                    row.process_error = true;
                }
            }
            model.set_row_data(index, row);
            true
        });
    };
    if child {
        app.on_subagent_process_page(callback);
    } else {
        app.on_timeline_process_page(callback);
    }
}

fn apply_page(row: &mut TimelineRow, page: NativeProcessPage, direction: i32) {
    let previous: HashMap<_, _> = row
        .process_entries
        .iter()
        .map(|entry| (entry.id.clone(), entry.open))
        .collect();
    let mut entries = Vec::new();
    for item in page.items {
        if !item.reasoning.is_empty() {
            entries.push(ProcessEntry {
                kind: crate::timeline::KIND_REASON,
                id: format!("reason-{}", item.id).into(),
                tool_name: if crate::timeline::status_kind(&item.state)
                    == crate::timeline::STATUS_RUNNING
                {
                    crate::timeline_text::thought_running()
                } else {
                    crate::timeline_text::thought_done()
                }
                .into(),
                summary: crate::message_blocks::reasoning_preview(&item.reasoning),
                detail: item.reasoning.into(),
                status_kind: crate::timeline::status_kind(&item.state),
                ..Default::default()
            });
        }
        if !item.text.is_empty() {
            entries.push(ProcessEntry {
                kind: crate::timeline::KIND_BODY,
                id: format!("print-{}", item.id).into(),
                blocks: crate::message_blocks::from_text(&item.text),
                text: item.text.into(),
                ..Default::default()
            });
        }
        if let Some(tool) = item.tool {
            let (target, patch) = crate::timeline_text::tool_target(&tool);
            entries.push(ProcessEntry {
                kind: crate::timeline::KIND_TOOL,
                id: format!("tool-{}", tool.id).into(),
                tool_name: crate::timeline_text::tool_label(&tool.title).into(),
                tool_icon: crate::tool_icons::workflow_icon(&tool.title).into(),
                target: target.into(),
                summary: crate::timeline_text::tool_summary(&tool).into(),
                detail: crate::timeline_text::copy_text(&[], &tool.detail),
                sections: crate::timeline_text::sections(&tool.sections),
                patch: if patch {
                    crate::timeline_text::patch_cards(&tool.patch_files)
                } else {
                    ModelRc::default()
                },
                tokens: tool.tokens.into(),
                duration: tool.duration.into(),
                status_kind: crate::timeline::status_kind(&tool.state),
                ..Default::default()
            });
        }
    }
    for (index, entry) in entries.iter_mut().enumerate() {
        // Negative payloads address a child in its own process, never an outer
        // timeline index. The bounded process slot makes this unambiguous.
        entry.payload = -1 - row.process_idx * 1024 - index as i32;
        entry.open = previous.get(&entry.id).copied().unwrap_or(false);
    }
    if let Some(model) = row
        .process_entries
        .as_any()
        .downcast_ref::<VecModel<ProcessEntry>>()
    {
        if direction == 2
            && model.row_count() == entries.len()
            && model
                .iter()
                .zip(&entries)
                .all(|(old, new)| old.id == new.id)
        {
            for (index, entry) in entries.into_iter().enumerate() {
                if model.row_data(index).as_ref() != Some(&entry) {
                    model.set_row_data(index, entry);
                }
            }
        } else {
            model.set_vec(entries);
        }
    } else {
        row.process_entries = ModelRc::new(VecModel::from(entries));
    }
    row.process_paged = true;
    row.process_local = false;
    row.process_older = page.has_older;
    row.process_newer = page.has_newer;
    row.process_error = false;
    // Sign tells the viewport which edge to retain after changing pages.
    if direction != 2 {
        row.process_revision = if direction < 0 {
            -row.process_revision.abs().saturating_add(1)
        } else {
            row.process_revision.abs().saturating_add(1)
        };
    }
}

pub fn toggle_entry(model: &impl Model<Data = TimelineRow>, payload: i32) {
    let token = -1 - payload;
    let process = token / 1024;
    let index = (token % 1024) as usize;
    if let Some(bar) = model
        .iter()
        .find(|row| row.kind == crate::timeline::KIND_PROCESS && row.process_idx == process)
    {
        if let Some(mut entry) = bar.process_entries.row_data(index) {
            entry.open = !entry.open;
            bar.process_entries.set_row_data(index, entry);
        }
    }
}

fn poll(mut receive: impl FnMut() -> bool + 'static) {
    slint::Timer::single_shot(Duration::from_millis(33), move || {
        if !receive() {
            poll(receive);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use wunder_desktop::native::NativeProcessItem;

    #[test]
    fn pages_keep_local_folds_and_rebase_actions_after_history_refresh() {
        let mut row = TimelineRow {
            kind: crate::timeline::KIND_PROCESS,
            process_idx: 7,
            ..Default::default()
        };
        let page = || NativeProcessPage {
            items: vec![NativeProcessItem {
                id: "fixture-item".into(),
                state: "completed".into(),
                reasoning: "Fixture thought".into(),
                text: String::new(),
                tool: None,
            }],
            first: Some(101),
            last: Some(200),
            has_older: true,
            has_newer: true,
        };
        apply_page(&mut row, page(), -1);
        let model = VecModel::from(vec![row.clone()]);
        let token = row.process_entries.row_data(0).unwrap().payload;
        toggle_entry(&model, token);
        assert!(row.process_entries.row_data(0).unwrap().open);
        apply_page(&mut row, page(), 2);
        assert!(row.process_entries.row_data(0).unwrap().open);
        assert!(row.process_revision < 0);
        let mut refreshed = vec![TimelineRow {
            id: row.id.clone(),
            kind: crate::timeline::KIND_PROCESS,
            process_idx: 3,
            ..Default::default()
        }];
        crate::timeline_text::restore_folds(&mut refreshed, &model);
        let token = refreshed[0].process_entries.row_data(0).unwrap().payload;
        assert_eq!(token, -3073);
        let refreshed = VecModel::from(refreshed);
        toggle_entry(&refreshed, token);
        assert!(
            !refreshed
                .row_data(0)
                .unwrap()
                .process_entries
                .row_data(0)
                .unwrap()
                .open
        );
    }
}
