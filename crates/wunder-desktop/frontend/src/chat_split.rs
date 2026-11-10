use crate::{MainWindow, TimelineRow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use wunder_desktop::native::{NativeChatTurn, NativeThreadUpdate, NativeThreadWatchCancel};
use wunder_desktop::NativeDesktop;

struct Slot {
    session: String,
    turns: Vec<NativeChatTurn>,
    cancel: Option<NativeThreadWatchCancel>,
    receiver: std::sync::mpsc::Receiver<NativeThreadUpdate>,
    generation: u64,
}

impl Slot {
    fn empty() -> Self {
        let (_sender, receiver) = std::sync::mpsc::sync_channel(8);
        Self {
            session: String::new(),
            turns: Vec::new(),
            cancel: None,
            receiver,
            generation: 0,
        }
    }

    fn clear(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
        self.session.clear();
        self.turns.clear();
        self.receiver = std::sync::mpsc::sync_channel(8).1;
    }
}

enum PendingAction {
    Send(String),
    Stop,
    Model(String),
    Toggle(i32),
}

struct PendingEntry {
    session: String,
    action: PendingAction,
    started: std::time::Instant,
}

thread_local! {
    static SLOTS: RefCell<Vec<Slot>> = RefCell::new((0..4).map(|_| Slot::empty()).collect());
    static PENDING: RefCell<HashMap<usize, PendingEntry>> = RefCell::new(HashMap::new());
    static DRAFTS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn session(app: &MainWindow, slot: usize) -> String {
    match slot {
        0 => app.get_split_session_0(),
        1 => app.get_split_session_1(),
        2 => app.get_split_session_2(),
        _ => app.get_split_session_3(),
    }
    .to_string()
}

fn set_session(app: &MainWindow, slot: usize, value: &str) {
    let value = value.into();
    match slot {
        0 => app.set_split_session_0(value),
        1 => app.set_split_session_1(value),
        2 => app.set_split_session_2(value),
        _ => app.set_split_session_3(value),
    }
}

fn set_title(app: &MainWindow, slot: usize, value: &str) {
    let value = value.into();
    match slot {
        0 => app.set_split_title_0(value),
        1 => app.set_split_title_1(value),
        2 => app.set_split_title_2(value),
        _ => app.set_split_title_3(value),
    }
}

fn draft(app: &MainWindow, slot: usize) -> String {
    match slot {
        0 => app.get_split_draft_0(),
        1 => app.get_split_draft_1(),
        2 => app.get_split_draft_2(),
        _ => app.get_split_draft_3(),
    }
    .to_string()
}

fn set_draft(app: &MainWindow, slot: usize, value: String) {
    let value = value.into();
    match slot {
        0 => app.set_split_draft_0(value),
        1 => app.set_split_draft_1(value),
        2 => app.set_split_draft_2(value),
        _ => app.set_split_draft_3(value),
    }
}

fn set_model(app: &MainWindow, slot: usize, name: &str, key: &str) {
    if slot >= 4 {
        return;
    }
    let mut names = app.get_split_model_names().iter().collect::<Vec<_>>();
    let mut keys = app.get_split_model_keys().iter().collect::<Vec<_>>();
    names.resize(4, Default::default());
    keys.resize(4, Default::default());
    names[slot] = name.into();
    keys[slot] = key.into();
    app.set_split_model_names(ModelRc::new(VecModel::from(names)));
    app.set_split_model_keys(ModelRc::new(VecModel::from(keys)));
}

fn cache_drafts(app: &MainWindow) {
    for slot in 0..4 {
        let id = session(app, slot);
        if !id.is_empty() {
            DRAFTS.with(|drafts| drafts.borrow_mut().insert(id, draft(app, slot)));
        }
    }
}

fn restore_draft(app: &MainWindow, slot: usize) {
    let id = session(app, slot);
    let saved = DRAFTS
        .with(|drafts| drafts.borrow().get(&id).cloned())
        .unwrap_or_default();
    set_draft(app, slot, saved);
}

fn set_loading(app: &MainWindow, slot: usize, value: bool) {
    match slot {
        0 => app.set_split_loading_0(value),
        1 => app.set_split_loading_1(value),
        2 => app.set_split_loading_2(value),
        _ => app.set_split_loading_3(value),
    }
}

fn set_busy(app: &MainWindow, slot: usize, value: bool) {
    match slot {
        0 => app.set_split_busy_0(value),
        1 => app.set_split_busy_1(value),
        2 => app.set_split_busy_2(value),
        _ => app.set_split_busy_3(value),
    }
}

fn publish_model(app: &MainWindow, slot: usize, rows: ModelRc<TimelineRow>) {
    match slot {
        0 => app.set_split_rows_0(rows),
        1 => app.set_split_rows_1(rows),
        2 => app.set_split_rows_2(rows),
        _ => app.set_split_rows_3(rows),
    }
}

fn publish_rows(app: &MainWindow, slot: usize, rows: Vec<TimelineRow>) {
    publish_model(app, slot, ModelRc::new(VecModel::from(rows)));
}

fn clear_pane(app: &MainWindow, slot: usize) {
    PENDING.with(|pending| pending.borrow_mut().remove(&slot));
    SLOTS.with(|slots| slots.borrow_mut()[slot].clear());
    set_session(app, slot, "");
    set_title(app, slot, "");
    set_draft(app, slot, String::new());
    set_model(app, slot, "", "");
    publish_rows(app, slot, Vec::new());
    set_loading(app, slot, false);
    set_busy(app, slot, false);
}

fn publish_slot(app: &MainWindow, index: usize) {
    SLOTS.with(|slots| {
        let slots = slots.borrow();
        let Some(slot) = slots.get(index) else { return };
        publish_rows(app, index, crate::native_chat::project_history(&slot.turns));
        set_busy(
            app,
            index,
            slot.turns.iter().any(|turn| {
                matches!(
                    turn.assistant.state.as_str(),
                    "running" | "queued" | "accepted" | "waiting_input" | "waiting_approval"
                )
            }),
        );
    });
}

fn activate(app: &MainWindow, slot: usize) {
    let id = session(app, slot);
    if id.is_empty() || app.get_active_session_id() == id {
        return;
    }
    if let Some(index) = app.get_conversations().iter().position(|row| row.id == id) {
        cache_drafts(app);
        app.invoke_select_conversation(index as i32);
    }
}

fn run_pending(app: &MainWindow, slot: usize) {
    let snapshot = PENDING.with(|pending| {
        pending
            .borrow()
            .get(&slot)
            .map(|entry| (entry.session.clone(), entry.started))
    });
    let Some((id, started)) = snapshot else {
        return;
    };
    if started.elapsed() > Duration::from_secs(10)
        || (app.get_active_session_id().is_empty() && !app.get_session_loading())
    {
        PENDING.with(|pending| pending.borrow_mut().remove(&slot));
        app.set_status("分屏线程尚未就绪，操作已取消".into());
        return;
    }
    if app.get_active_session_id() != id || app.get_session_loading() {
        let weak = app.as_weak();
        slint::Timer::single_shot(Duration::from_millis(50), move || {
            if let Some(app) = weak.upgrade() {
                run_pending(&app, slot);
            }
        });
        return;
    }
    let action = PENDING.with(|pending| pending.borrow_mut().remove(&slot));
    let Some(entry) = action else { return };
    match entry.action {
        PendingAction::Send(content) => {
            app.set_draft(content.into());
            app.invoke_send_message();
        }
        PendingAction::Stop => app.invoke_stop_generation(),
        PendingAction::Model(key) => {
            set_model(app, slot, &key, &key);
            app.invoke_select_thread_model(key.into());
        }
        PendingAction::Toggle(index) => app.invoke_timeline_toggle(index),
    }
}

fn schedule(app: &MainWindow, slot: usize, action: PendingAction) {
    let id = session(app, slot);
    if id.is_empty() {
        return;
    }
    PENDING.with(|pending| {
        pending.borrow_mut().insert(
            slot,
            PendingEntry {
                session: id,
                action,
                started: std::time::Instant::now(),
            },
        );
    });
    activate(app, slot);
    run_pending(app, slot);
}

fn open_slot(app: &MainWindow, desktop: Arc<NativeDesktop>, slot: usize, row: usize) {
    let Some(conversation) = app.get_conversations().row_data(row) else {
        return;
    };
    let id = conversation.id.to_string();
    cache_drafts(app);
    if app.get_split_count() == 1 {
        app.set_split_count(2);
        let active = app.get_active_session_id().to_string();
        set_session(app, 0, &active);
        let active_title = app
            .get_conversations()
            .iter()
            .find(|row| row.id == active)
            .map(|row| row.title.to_string())
            .unwrap_or_default();
        set_title(app, 0, &active_title);
        publish_rows(app, 0, app.get_timeline().iter().collect());
        set_draft(app, 0, app.get_draft().to_string());
        if let Some(active_index) = app
            .get_conversations()
            .iter()
            .position(|row| row.id == active)
        {
            open_slot(app, desktop.clone(), 0, active_index);
        }
    }
    let previous = session(app, slot);
    let watching = SLOTS.with(|slots| slots.borrow()[slot].cancel.is_some());
    if previous == id && watching {
        return;
    }
    let duplicate = (0..app.get_split_count() as usize)
        .find(|index| *index != slot && session(app, *index) == id);
    if previous.is_empty() {
        if let Some(existing) = duplicate {
            activate(app, existing);
            return;
        }
    }
    SLOTS.with(|slots| slots.borrow_mut()[slot].clear());
    set_session(app, slot, &id);
    restore_draft(app, slot);
    set_title(app, slot, &conversation.title);
    let selected_key = app.get_selected_model_key().to_string();
    let fallback_name = if selected_key.is_empty() {
        app.get_selected_agent_model().to_string()
    } else {
        selected_key.clone()
    };
    if app.get_active_session_id() == id {
        set_model(app, slot, &fallback_name, &selected_key);
    } else {
        set_model(app, slot, &fallback_name, &fallback_name);
    }
    publish_rows(app, slot, Vec::new());
    set_loading(app, slot, true);
    set_busy(app, slot, false);
    if let Some(other) = duplicate {
        SLOTS.with(|slots| slots.borrow_mut()[other].clear());
        set_session(app, other, &previous);
        let previous_title = app
            .get_conversations()
            .iter()
            .find(|row| row.id == previous)
            .map(|row| row.title.to_string())
            .unwrap_or_default();
        set_title(app, other, &previous_title);
        publish_rows(app, other, Vec::new());
        if !previous.is_empty() {
            if let Some(index) = app
                .get_conversations()
                .iter()
                .position(|row| row.id == previous)
            {
                open_slot(app, desktop.clone(), other, index);
            }
        }
    }
    let open = (0..4)
        .filter(|slot| !session(app, *slot).is_empty())
        .count();
    if open > 0 {
        app.set_split_count(if open > 2 { 4 } else { 2 });
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(8);
    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        slots[slot].session = id.clone();
        slots[slot].receiver = receiver;
    });
    let generation = SLOTS.with(|slots| slots.borrow()[slot].generation);
    let weak = app.as_weak();
    let wake_weak = weak.clone();
    std::thread::spawn(move || {
        let result = desktop.load_chat_headers(&id).and_then(|turns| {
            let session = desktop.get_session_info(&id)?;
            Ok((turns, session.session_model_name))
        });
        let _ = weak.upgrade_in_event_loop(move |app| {
            let slot = SLOTS.with(|slots| {
                slots.borrow().iter().position(|candidate| {
                    candidate.generation == generation && candidate.session == id
                })
            });
            let Some(slot) = slot else { return };
            match result {
                Ok((turns, pinned_model)) => {
                    if let Some(model) = pinned_model {
                        set_model(&app, slot, &model, &model);
                    }
                    let watch = desktop.watch_chat_with_initial(&id, Some(turns.clone()));
                    let cancel = watch.cancel_handle();
                    let mut watch = Some(watch);
                    SLOTS.with(|slots| {
                        let mut slots = slots.borrow_mut();
                        slots[slot].turns = turns;
                        slots[slot].cancel = Some(cancel.clone());
                    });
                    publish_slot(&app, slot);
                    set_loading(&app, slot, false);
                    let thread = std::thread::spawn(move || {
                        let mut watch = watch.take().expect("watch");
                        while let Some(update) = watch.blocking_recv() {
                            if sender.send(update).is_err() {
                                break;
                            }
                            let _ = wake_weak.upgrade_in_event_loop(|app| {
                                app.invoke_split_thread_update_ready()
                            });
                        }
                    });
                    drop(thread);
                }
                Err(error) => {
                    set_loading(&app, slot, false);
                    app.set_status(format!("无法读取分屏线程：{error}").into());
                }
            }
        });
    });
    activate(app, slot);
}

fn compact_layout(app: &MainWindow) {
    let sources = (0..4)
        .filter(|slot| !session(app, *slot).is_empty())
        .collect::<Vec<_>>();
    if sources == (0..sources.len()).collect::<Vec<_>>() {
        return;
    }
    let names = app.get_split_model_names();
    let keys = app.get_split_model_keys();
    let panes = sources
        .iter()
        .map(|source| {
            let source = *source;
            let title = match source {
                0 => app.get_split_title_0(),
                1 => app.get_split_title_1(),
                2 => app.get_split_title_2(),
                _ => app.get_split_title_3(),
            };
            let rows = match source {
                0 => app.get_split_rows_0(),
                1 => app.get_split_rows_1(),
                2 => app.get_split_rows_2(),
                _ => app.get_split_rows_3(),
            };
            let loading = match source {
                0 => app.get_split_loading_0(),
                1 => app.get_split_loading_1(),
                2 => app.get_split_loading_2(),
                _ => app.get_split_loading_3(),
            };
            let busy = match source {
                0 => app.get_split_busy_0(),
                1 => app.get_split_busy_1(),
                2 => app.get_split_busy_2(),
                _ => app.get_split_busy_3(),
            };
            (
                source,
                session(app, source),
                title,
                draft(app, source),
                rows,
                loading,
                busy,
                names.row_data(source).unwrap_or_default(),
                keys.row_data(source).unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();

    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let mut previous = std::mem::replace(&mut *slots, (0..4).map(|_| Slot::empty()).collect());
        for (target, source) in sources.iter().enumerate() {
            slots[target] = std::mem::replace(&mut previous[*source], Slot::empty());
        }
    });
    PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        let mut previous = std::mem::take(&mut *pending);
        for (target, source) in sources.iter().enumerate() {
            if let Some(entry) = previous.remove(source) {
                pending.insert(target, entry);
            }
        }
    });

    for target in 0..4 {
        if let Some((_, id, title, draft, rows, loading, busy, _, _)) = panes.get(target) {
            set_session(app, target, id);
            set_title(app, target, title);
            set_draft(app, target, draft.clone());
            publish_model(app, target, rows.clone());
            set_loading(app, target, *loading);
            set_busy(app, target, *busy);
        } else {
            set_session(app, target, "");
            set_title(app, target, "");
            set_draft(app, target, String::new());
            publish_rows(app, target, Vec::new());
            set_loading(app, target, false);
            set_busy(app, target, false);
        }
    }
    let mut names = panes.iter().map(|pane| pane.7.clone()).collect::<Vec<_>>();
    let mut keys = panes.iter().map(|pane| pane.8.clone()).collect::<Vec<_>>();
    names.resize(4, Default::default());
    keys.resize(4, Default::default());
    app.set_split_model_names(ModelRc::new(VecModel::from(names)));
    app.set_split_model_keys(ModelRc::new(VecModel::from(keys)));
    let pending = PENDING.with(|pending| pending.borrow().keys().copied().collect::<Vec<_>>());
    for slot in pending {
        run_pending(app, slot);
    }
}

fn normalize_layout(app: &MainWindow) {
    let open = (0..4)
        .filter(|slot| !session(app, *slot).is_empty())
        .count();
    if open <= 1 {
        app.invoke_split_layout(1);
    } else {
        app.set_split_count(if open == 2 { 2 } else { 4 });
    }
}

pub(crate) fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    app.on_split_thread_update_ready(move || {
        let Some(app) = weak.upgrade() else { return };
        for index in 0..4 {
            let mut changed = false;
            SLOTS.with(|slots| {
                let mut slots = slots.borrow_mut();
                let slot = &mut slots[index];
                for _ in 0..8 {
                    let Ok(update) = slot.receiver.try_recv() else {
                        break;
                    };
                    match update {
                        NativeThreadUpdate::Reset(turns) => {
                            slot.turns = turns;
                            changed = true;
                        }
                        NativeThreadUpdate::Turns(turns) => {
                            for turn in turns {
                                if let Some(old) = slot
                                    .turns
                                    .iter_mut()
                                    .find(|old| old.root_id == turn.root_id)
                                {
                                    if *old != turn {
                                        *old = turn;
                                        changed = true;
                                    }
                                } else {
                                    slot.turns.push(turn);
                                    changed = true;
                                }
                            }
                            slot.turns.sort_by_key(|turn| turn.user.user_round);
                        }
                        _ => {}
                    }
                }
            });
            if changed {
                publish_slot(&app, index);
            }
        }
    });
    let weak = app.as_weak();
    let layout_desktop = desktop.clone();
    app.on_split_layout(move |count| {
        let Some(app) = weak.upgrade() else { return };
        if count == 1 {
            cache_drafts(&app);
            let active = app.get_active_session_id();
            for slot in 0..4 {
                if active == session(&app, slot) {
                    app.set_draft(draft(&app, slot).into());
                }
            }
            app.set_split_count(1);
            PENDING.with(|pending| pending.borrow_mut().clear());
            for slot in 0..4 {
                clear_pane(&app, slot);
            }
            return;
        }
        let count = if count >= 4 { 4 } else { 2 };
        if app.get_split_count() == 1 {
            set_session(&app, 0, &app.get_active_session_id());
            set_draft(&app, 0, app.get_draft().to_string());
            publish_rows(&app, 0, app.get_timeline().iter().collect());
            let id = session(&app, 0);
            if let Some(index) = app.get_conversations().iter().position(|row| row.id == id) {
                let title = app
                    .get_conversations()
                    .row_data(index)
                    .map(|row| row.title.to_string())
                    .unwrap_or_default();
                set_title(&app, 0, &title);
                open_slot(&app, layout_desktop.clone(), 0, index);
            }
        }
        app.set_split_count(count);
    });
    let weak = app.as_weak();
    app.on_split_close(move |slot| {
        let Some(app) = weak.upgrade() else { return };
        let slot = slot.clamp(0, 3) as usize;
        let closed_active = app.get_active_session_id() == session(&app, slot);
        cache_drafts(&app);
        clear_pane(&app, slot);
        if closed_active {
            if let Some(next) = (0..4).find(|index| !session(&app, *index).is_empty()) {
                activate(&app, next);
            }
        }
        compact_layout(&app);
        normalize_layout(&app);
    });
    let weak = app.as_weak();
    let drop_desktop = desktop.clone();
    app.on_split_drop_row(move |slot, row| {
        if let Some(app) = weak.upgrade() {
            open_slot(
                &app,
                drop_desktop.clone(),
                slot.clamp(0, 3) as usize,
                row.max(0) as usize,
            );
        }
    });
    let weak = app.as_weak();
    app.on_split_activate(move |slot| {
        if let Some(app) = weak.upgrade() {
            activate(&app, slot.clamp(0, 3) as usize);
        }
    });
    let weak = app.as_weak();
    app.on_split_send(move |slot| {
        if let Some(app) = weak.upgrade() {
            let slot = slot.clamp(0, 3) as usize;
            let content = draft(&app, slot);
            set_draft(&app, slot, String::new());
            schedule(&app, slot, PendingAction::Send(content));
        }
    });
    let weak = app.as_weak();
    app.on_split_stop(move |slot| {
        if let Some(app) = weak.upgrade() {
            schedule(&app, slot.clamp(0, 3) as usize, PendingAction::Stop);
        }
    });
    let weak = app.as_weak();
    app.on_split_select_model(move |slot, key| {
        if let Some(app) = weak.upgrade() {
            schedule(
                &app,
                slot.clamp(0, 3) as usize,
                PendingAction::Model(key.to_string()),
            );
        }
    });
    let weak = app.as_weak();
    app.on_split_toggle_row(move |slot, index| {
        if let Some(app) = weak.upgrade() {
            schedule(
                &app,
                slot.clamp(0, 3) as usize,
                PendingAction::Toggle(index),
            );
        }
    });
}
