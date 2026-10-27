//! On-demand process pages. Each expanded process retains one bounded page; reads and tool
//! parsing run off the UI thread, and replies are scoped to the selected thread.
use crate::{MainWindow, ProcessEntry, TimelineRow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{mpsc::sync_channel, Arc},
    time::Duration,
};
use wunder_desktop::{native::NativeProcessPage, NativeDesktop};

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
    } else if app.get_cloud_chat_active() {
        crate::cloud_threads::selected_id(app)
    } else {
        app.get_active_session_id().to_string()
    }
}

/// A page edge as a durable cursor. `-1` means "this edge is unknown", which
/// the read turns into an absent cursor: the reader then gets the newest page
/// instead of a page that cannot be continued.
fn page_edge(index: i32) -> Option<i64> {
    (index >= 0).then_some(index as i64)
}

fn bind(app: &MainWindow, api: Arc<NativeDesktop>, child: bool, pending: Rc<Cell<usize>>) {
    // One read in flight per (thread, process). The page edges themselves live
    // on the row, so a page always continues from what is actually loaded —
    // re-creating the view used to drop the cursor and re-read the latest page,
    // which made the "earlier" control look dead.
    let inflight = Rc::new(RefCell::new(HashSet::<(String, String)>::new()));
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
        if !(row.open || (row.visible && row.status_kind == crate::timeline::STATUS_RUNNING))
            || row.process_root.is_empty()
        {
            return;
        }
        // The stream owns this model until settlement. A durable page is
        // necessarily behind its active tail, and must never replace it.
        if row.process_local && row.status_kind == crate::timeline::STATUS_RUNNING {
            return;
        }
        if row.process_local && direction == 0 && !row.process_error {
            return;
        }
        let target = session(&app, child);
        let root = row.process_root.to_string();
        let key = (target.clone(), root.clone());
        // A retry repeats the page the reader asked for: an older page stays an
        // older page after a failed read, and a live tail refresh stays one.
        let direction = if direction == 0 && row.process_error && row.process_paged {
            if row.process_newer {
                1
            } else if row.process_older {
                -1
            } else {
                0
            }
        } else {
            direction
        };
        if direction == 0 && row.process_paged && !row.process_error {
            return;
        }
        if direction == 2 && row.process_newer {
            return;
        }
        let (before, after) = match direction {
            -1 if row.process_older => (page_edge(row.process_first), None),
            1 if row.process_newer => (None, page_edge(row.process_last)),
            -1 | 1 => return,
            _ => (None, None),
        };
        if pending.get() >= 4 {
            return;
        }
        if !inflight.borrow_mut().insert(key.clone()) {
            return;
        }
        pending.set(pending.get() + 1);
        let entries = row.process_entries.clone();
        // Background tail refreshes keep the existing layout and scroll anchor.
        row.process_loading = direction != 2;
        row.process_error = false;
        model.set_row_data(payload as usize, row);
        let (tx, rx) = sync_channel(1);
        let backend = api.clone();
        let lookup = target.clone();
        let root_id = root.clone();
        let cloud = !child && app.get_cloud_chat_active();
        let final_id = if cloud {
            crate::cloud_threads::final_message_id(&root)
        } else {
            String::new()
        };
        std::thread::spawn(move || {
            let result = if cloud {
                backend.cloud_thread_process_page(&lookup, &root_id, &final_id, before, after)
            } else {
                backend.chat_process_page(&lookup, &root_id, before, after)
            };
            let _ = tx.send(result);
        });
        let weak = app.as_weak();
        let inflight = inflight.clone();
        let pending = pending.clone();
        poll(move || {
            let result = match rx.try_recv() {
                Ok(value) => value,
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                Err(_) => Err(anyhow::anyhow!("process reader stopped")),
            };
            pending.set(pending.get().saturating_sub(1));
            inflight.borrow_mut().remove(&key);
            let Some(app) = weak.upgrade() else {
                return true;
            };
            if session(&app, child) != target || (!child && app.get_cloud_chat_active() != cloud) {
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
            if !page_is_current(&row, &entries) {
                return true;
            }
            row.process_loading = false;
            match result {
                Ok(page) => {
                    apply_page(&mut row, page, direction);
                    for reason in model.iter().filter(|reason| {
                        reason.kind == crate::timeline::KIND_REASON
                            && reason.process_idx == row.process_idx
                    }) {
                        crate::timeline_text::merge_reasoning_tail(
                            &row,
                            crate::timeline_text::process_entry(&reason),
                        );
                    }
                }
                // A failed tail refresh is retried by the live timer; only a
                // read the reader asked for is worth a retry strip.
                Err(_) => row.process_error = direction != 2,
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
    let (page_first, page_last) = (page.first, page.last);
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
    // Keep the page projection defensive when a storage adapter repeats one
    // immutable assistant item.  Round identity is deliberately not inferred
    // from reasoning text: two model rounds are allowed to have identical
    // thoughts.  Canonical local pages already filter random assistant copies
    // in `native_chat_process::project_page` by (execution, model_round).
    dedupe_reasoning_entries(&mut entries);
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
    // The page carries its own exclusive edges; keeping them on the row is what
    // makes the next read continue this page instead of restarting at the tail.
    row.process_first = page_first.unwrap_or(-1) as i32;
    row.process_last = page_last.unwrap_or(-1) as i32;
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

fn page_is_current(row: &TimelineRow, entries: &ModelRc<ProcessEntry>) -> bool {
    row.process_entries == *entries
        && !(row.process_local && row.status_kind == crate::timeline::STATUS_RUNNING)
}

fn stable_reasoning_item(id: &str) -> bool {
    let Some((_, round)) = id.rsplit_once(":text-") else {
        return false;
    };
    !round.is_empty() && round.chars().all(|ch| ch.is_ascii_digit())
}

fn dedupe_reasoning_entries(entries: &mut Vec<ProcessEntry>) {
    let mut stable_ids = HashSet::new();
    entries.retain(|entry| {
        if entry.kind != crate::timeline::KIND_REASON {
            return true;
        }
        let stable = stable_reasoning_item(entry.id.as_str());
        if stable {
            // A repeated revision of one stable item is one thought row. Keep
            // the first complete projection; revisions are merged by the
            // live-tail synchronizer when the page is refreshed.
            return stable_ids.insert(entry.id.to_string());
        }
        true
    });
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
    fn terminal_history_invalidates_a_paged_running_tool() {
        let old = TimelineRow {
            id: "fixture-process".into(),
            kind: crate::timeline::KIND_PROCESS,
            process_root: "fixture-root".into(),
            status_kind: crate::timeline::STATUS_RUNNING,
            process_paged: true,
            open: true,
            process_entries: ModelRc::new(VecModel::from(vec![ProcessEntry {
                id: "fixture-call".into(),
                kind: crate::timeline::KIND_TOOL,
                status_kind: crate::timeline::STATUS_RUNNING,
                ..Default::default()
            }])),
            ..Default::default()
        };
        let previous = VecModel::from(vec![old.clone()]);
        let mut next = vec![TimelineRow {
            status_kind: crate::timeline::STATUS_DONE,
            process_entries: ModelRc::default(),
            ..old
        }];
        crate::timeline_text::restore_folds(&mut next, &previous);
        assert!(next[0].open);
        assert!(
            !next[0].process_paged,
            "the terminal row must request a fresh page"
        );
        assert!(!page_is_current(
            &next[0],
            &previous.row_data(0).unwrap().process_entries
        ));
        apply_page(
            &mut next[0],
            NativeProcessPage {
                items: vec![NativeProcessItem {
                    id: "fixture-call".into(),
                    item_index: 0,
                    state: "completed".into(),
                    reasoning: String::new(),
                    text: String::new(),
                    tool: Some(wunder_desktop::native::NativeWorkflowEntry {
                        id: "fixture-call".into(),
                        state: "completed".into(),
                        title: "fixture tool".into(),
                        preview: String::new(),
                        detail: String::new(),
                        tokens: String::new(),
                        duration: String::new(),
                        sections: Vec::new(),
                        patch_files: Vec::new(),
                    }),
                }],
                first: Some(1),
                last: Some(1),
                has_older: false,
                has_newer: false,
            },
            0,
        );
        assert_eq!(
            next[0].process_entries.row_data(0).unwrap().status_kind,
            crate::timeline::STATUS_DONE
        );
    }

    #[test]
    fn terminal_history_preserves_an_older_page_and_its_disclosures() {
        let old = TimelineRow {
            id: "fixture-process".into(),
            kind: crate::timeline::KIND_PROCESS,
            status_kind: crate::timeline::STATUS_RUNNING,
            process_paged: true,
            process_newer: true,
            open: true,
            process_entries: ModelRc::new(VecModel::from(vec![ProcessEntry {
                id: "fixture-old-item".into(),
                status_kind: crate::timeline::STATUS_DONE,
                open: true,
                ..Default::default()
            }])),
            ..Default::default()
        };
        let entries = old.process_entries.clone();
        let previous = VecModel::from(vec![old.clone()]);
        let mut next = vec![TimelineRow {
            status_kind: crate::timeline::STATUS_DONE,
            ..old
        }];
        crate::timeline_text::restore_folds(&mut next, &previous);
        assert!(next[0].process_newer && next[0].process_paged && next[0].open);
        assert_eq!(next[0].process_entries, entries);
        assert!(next[0].process_entries.row_data(0).unwrap().open);
    }

    #[test]
    fn pages_keep_local_folds_and_rebase_actions_after_history_refresh() {
        let mut row = TimelineRow {
            kind: crate::timeline::KIND_PROCESS,
            process_idx: 7,
            ..Default::default()
        };
        let page = || NativeProcessPage {
            items: vec![NativeProcessItem {
                item_index: 0,
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

    #[test]
    fn delayed_process_page_keeps_the_live_thought_and_older_page_stays_put() {
        let mut row = TimelineRow {
            kind: crate::timeline::KIND_PROCESS,
            process_idx: 4,
            process_entries: ModelRc::new(VecModel::<ProcessEntry>::default()),
            ..Default::default()
        };
        let live = || ProcessEntry {
            id: "reason-fixture-item".into(),
            kind: crate::timeline::KIND_REASON,
            detail: "Fixture thought continued".into(),
            status_kind: crate::timeline::STATUS_RUNNING,
            ..Default::default()
        };
        crate::timeline_text::merge_reasoning_tail(&row, live());
        row.process_entries.set_row_data(
            0,
            ProcessEntry {
                open: true,
                ..row.process_entries.row_data(0).unwrap()
            },
        );
        let child = row.process_entries.clone();
        apply_page(
            &mut row,
            NativeProcessPage {
                items: vec![NativeProcessItem {
                    item_index: 0,
                    id: "fixture-item".into(),
                    state: "running".into(),
                    reasoning: "Fixture thought".into(),
                    text: String::new(),
                    tool: None,
                }],
                ..Default::default()
            },
            2,
        );
        crate::timeline_text::merge_reasoning_tail(&row, live());
        assert_eq!(row.process_entries, child);
        assert_eq!(child.row_count(), 1);
        assert!(child.row_data(0).unwrap().open);
        assert_eq!(
            child.row_data(0).unwrap().detail,
            "Fixture thought continued"
        );
        let stable = child.row_data(0).unwrap();
        row.process_newer = true;
        crate::timeline_text::merge_reasoning_tail(
            &row,
            ProcessEntry {
                id: "reason-fixture-next".into(),
                ..live()
            },
        );
        assert_eq!(child.row_count(), 1);
        assert_eq!(child.row_data(0).unwrap(), stable);
    }

    #[test]
    fn process_page_drops_repeated_stable_reasoning_item_but_keeps_distinct_rounds() {
        let mut row = TimelineRow {
            kind: crate::timeline::KIND_PROCESS,
            process_idx: 2,
            process_entries: ModelRc::new(VecModel::<ProcessEntry>::default()),
            ..Default::default()
        };
        let item = |id: &str, reasoning: &str| NativeProcessItem {
            item_index: 0,
            id: id.into(),
            state: "completed".into(),
            reasoning: reasoning.into(),
            text: String::new(),
            tool: None,
        };
        apply_page(
            &mut row,
            NativeProcessPage {
                items: vec![
                    item("fixture-root:text-1", "first thought"),
                    item("fixture-root:text-1", "first thought"),
                    item("fixture-root:text-2", "first thought"),
                ],
                ..Default::default()
            },
            -1,
        );
        let reasons: Vec<_> = row
            .process_entries
            .iter()
            .filter(|entry| entry.kind == crate::timeline::KIND_REASON)
            .collect();
        assert_eq!(reasons.len(), 2);
        assert_eq!(reasons[0].id, "reason-fixture-root:text-1");
        assert_eq!(reasons[1].id, "reason-fixture-root:text-2");
    }
}
