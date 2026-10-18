//! Cloud thread controller.
//!
//! Cloud threads are a data source for the ordinary desktop transcript, not a
//! second chat product. This module owns only the bounded catalogue,
//! page/watch lifecycle and command hand-off. `TimelineView` and `Composer`
//! remain the single reader and writer surface for every thread.
use crate::{CloudThreadCard, MainWindow, ModelCard};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use wunder_desktop::{
    native::{NativeCloudAgent, NativeCloudThread},
    NativeDesktop,
};
use wunder_server::{cloud::CloudThreadEvent, stable_core::thread_view::{ChatViewRow, ThreadViewPage}};

thread_local! {
    static CATALOG: RefCell<(Vec<NativeCloudAgent>, Vec<NativeCloudThread>)> = RefCell::new((Vec::new(), Vec::new()));
    static PENDING_SELECTION: RefCell<Option<String>> = const { RefCell::new(None) };
    static WATCH: RefCell<Option<wunder_server::cloud::CloudThreadCancellation>> = const { RefCell::new(None) };
    static MESSAGES: RefCell<Vec<ChatViewRow>> = const { RefCell::new(Vec::new()) };
    static LOADED_ID: RefCell<String> = const { RefCell::new(String::new()) };
    static DRAFTS: RefCell<std::collections::HashMap<String, String>> = RefCell::new(std::collections::HashMap::new());
    static ACCOUNT_KEY: RefCell<(bool, String, String)> = RefCell::new((false, String::new(), String::new()));
}

static GENERATION: AtomicU64 = AtomicU64::new(0);
static ACCOUNT_GENERATION: AtomicU64 = AtomicU64::new(0);
static CATALOG_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(crate) fn account_status(app: &MainWindow, logged_in: bool, server: &str, username: &str) {
    let key = (logged_in, server.to_string(), username.to_string());
    let changed = ACCOUNT_KEY.with(|slot| {
        let changed = *slot.borrow() != key;
        *slot.borrow_mut() = key;
        changed
    });
    if changed { reset_account(app); }
}

pub(crate) fn reset_account(app: &MainWindow) {
    let was_cloud = app.get_cloud_chat_active();
    leave(app);
    if was_cloud { crate::native_chat::prepare_external_timeline(app); }
    ACCOUNT_GENERATION.fetch_add(1, Ordering::AcqRel);
    CATALOG_GENERATION.fetch_add(1, Ordering::AcqRel);
    CATALOG.with(|slot| *slot.borrow_mut() = (Vec::new(), Vec::new()));
    MESSAGES.with(|slot| slot.borrow_mut().clear());
    DRAFTS.with(|slot| slot.borrow_mut().clear());
    LOADED_ID.with(|slot| slot.borrow_mut().clear());
    PENDING_SELECTION.with(|slot| *slot.borrow_mut() = None);
    app.set_cloud_active_thread_id("".into());
    app.set_cloud_thread_index(0);
    app.set_cloud_thread_rows(ModelRc::default());
    app.set_cloud_thread_models(ModelRc::default());
    app.set_cloud_greeting("".into());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(round: i64, revision: i64) -> ChatViewRow {
        ChatViewRow { root_turn_id: format!("fixture-{round}"), user_round: round, revision, ..Default::default() }
    }

    #[test]
    fn cloud_replay_keeps_order_rejects_old_revisions_and_bounds_history() {
        let mut rows = (1..=20).map(|round| row(round, 2)).collect::<Vec<_>>();
        merge_messages(&mut rows, &[row(19, 1), row(21, 1), row(20, 3)], false);
        assert_eq!(rows.len(), 20);
        assert_eq!(rows.first().unwrap().user_round, 2);
        assert_eq!(rows.last().unwrap().user_round, 21);
        assert_eq!(rows.iter().find(|row| row.user_round == 19).unwrap().revision, 2);
        assert_eq!(rows.iter().find(|row| row.user_round == 20).unwrap().revision, 3);
        merge_messages(&mut rows, &[row(40, 1)], true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].user_round, 40);
    }
}

fn invalidate() -> u64 {
    WATCH.with(|watch| {
        if let Some(cancel) = watch.borrow_mut().take() {
            cancel.cancel();
        }
    });
    GENERATION.fetch_add(1, Ordering::AcqRel) + 1
}

fn active(generation: u64) -> bool {
    GENERATION.load(Ordering::Acquire) == generation
}

pub(crate) fn leave(app: &MainWindow) {
    save_draft(app);
    invalidate();
    if app.get_cloud_chat_active() {
        app.set_busy(false);
        app.set_stopping(false);
        app.set_draft("".into());
    }
    app.set_cloud_chat_active(false);
    app.set_cloud_thread_busy(false);
    LOADED_ID.with(|slot| slot.borrow_mut().clear());
}

fn save_draft(app: &MainWindow) {
    LOADED_ID.with(|id| {
        if id.borrow().is_empty() { return; }
        DRAFTS.with(|slot| {
            let mut drafts = slot.borrow_mut();
            if drafts.len() >= 100 && !drafts.contains_key(id.borrow().as_str()) { drafts.clear(); }
            drafts.insert(id.borrow().clone(), app.get_draft().to_string());
        });
    });
}

pub(crate) fn is_active(app: &MainWindow) -> bool {
    app.get_cloud_chat_active() && !selected_id(app).is_empty()
}

pub(crate) fn selected_id(app: &MainWindow) -> String {
    app.get_cloud_active_thread_id().to_string()
}

pub(crate) fn final_message_id(root: &str) -> String {
    MESSAGES.with(|slot| slot.borrow().iter()
        .find(|row| row.root_turn_id == root)
        .map(|row| row.assistant.item_id.clone()).unwrap_or_default())
}

fn selected_agent() -> String {
    CATALOG.with(|catalog| {
        catalog
            .borrow()
            .0
            .first()
            .map(|agent| agent.id.clone())
            .unwrap_or_default()
    })
}

fn merge_messages(messages: &mut Vec<ChatViewRow>, incoming: &[ChatViewRow], replace: bool) {
    if replace { messages.clear(); }
    for row in incoming {
        if let Some(existing) = messages.iter_mut().find(|old| old.root_turn_id == row.root_turn_id) {
            if row.revision >= existing.revision { *existing = row.clone(); }
        } else {
            messages.push(row.clone());
        }
    }
    messages.sort_by(|left, right| left.user_round.cmp(&right.user_round)
        .then_with(|| left.created_time.total_cmp(&right.created_time))
        .then_with(|| left.root_turn_id.cmp(&right.root_turn_id)));
    if messages.len() > 20 { let remove = messages.len() - 20; messages.drain(..remove); }
}

fn publish_page(app: &MainWindow, page: &ThreadViewPage, replace: bool) {
    // A degraded bootstrap is not an empty thread. Keep the committed view.
    if page.degraded {
        app.set_status("云端线程暂时读取缓慢，正在恢复连接".into());
        return;
    }
    let rows = MESSAGES.with(|slot| {
        let mut messages = slot.borrow_mut();
        merge_messages(&mut messages, &page.chat, replace);
        crate::native_chat::project_cloud_history(&messages, app.get_timeline())
    });
    crate::native_chat::replace_external_timeline(app, rows, replace);
    app.set_busy(matches!(page.runtime_status.as_str(), "running" | "queued" | "pending" | "waiting_approval"));
    app.set_stopping(false);
    app.set_cloud_thread_status(page.runtime_status.clone().into());
}

fn load(app: &MainWindow, api: Arc<NativeDesktop>) {
    let id = selected_id(app);
    let generation = invalidate();
    let changed = LOADED_ID.with(|slot| *slot.borrow() != id);
    if changed {
        save_draft(app);
        crate::native_chat::prepare_external_timeline(app);
        MESSAGES.with(|slot| slot.borrow_mut().clear());
        LOADED_ID.with(|slot| *slot.borrow_mut() = id.clone());
        app.set_draft(DRAFTS.with(|slot| slot.borrow().get(&id).cloned().unwrap_or_default()).into());
        app.set_selected_model_key("".into());
        app.set_selected_agent_model("".into());
        app.set_reasoning_effort("default".into());
        app.set_context_known(false);
        app.set_cloud_greeting("".into());
    }
    app.set_cloud_thread_busy(!id.is_empty());
    if id.is_empty() {
        crate::native_chat::replace_external_timeline(app, Vec::new(), true);
        return;
    }
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let mut failures = 0u32;
        let result = loop {
            if !active(generation) { return; }
            let result = api.cloud_thread_bootstrap(&id);
            if result.as_ref().err().is_none_or(|error| !wunder_server::cloud::retryable_cloud_read(error)) {
                break result;
            }
            failures = failures.saturating_add(1);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if active(generation) {
                    app.set_cloud_thread_busy(false);
                    app.set_cloud_thread_status("reconnecting".into());
                    app.set_status("云端暂时不可达，正在自动重连".into());
                }
            });
            // One reader per selection, cancellable bounded backoff. A switch
            // must not retain sleeping workers or accept an old bootstrap.
            for _ in 0..((1u64 << failures.min(5)).min(30) * 10) {
                if !active(generation) { return; }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        };
        let (view, mut watch) = match result {
            Ok(view) => {
                let watch = api.cloud_thread_watch(&id, view.page.view_seq);
                (view, watch)
            }
            Err(error) => {
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if active(generation) {
                        app.set_cloud_thread_busy(false);
                        app.set_status(format!("无法读取云端线程：{error}").into());
                    }
                });
                return;
            }
        };
        let token = watch.cancel_token();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) || !is_active(&app) {
                token.cancel();
                return;
            }
            WATCH.with(|watch| *watch.borrow_mut() = Some(token));
            app.set_cloud_greeting(CATALOG.with(|catalog| catalog.borrow().0.iter()
                .find(|agent| agent.id == view.agent_id || (view.agent_id.is_empty() && agent.id == "__default__"))
                .map(|agent| agent.description.trim().to_string()).unwrap_or_default()).into());
            app.set_selected_model_key(view.model_key.into());
            app.set_reasoning_effort(view.reasoning_effort.into());
            publish_page(&app, &view.page, changed);
            app.set_cloud_thread_busy(false);
        });
        // The command acknowledgement only means the turn was admitted.  A
        // fast cloud run can finish between bootstrap and the websocket watch
        // subscription, leaving the initial `running` projection stranded
        // until the user changes threads.  Keep one bounded, cancellable
        // reconciliation loop alongside the live stream so terminal state is
        // visible without navigation.
        let poll_api = api.clone();
        let poll_weak = weak.clone();
        let poll_id = id.clone();
        std::thread::spawn(move || {
            for _ in 0..400 {
                std::thread::sleep(std::time::Duration::from_millis(750));
                if !active(generation) { return; }
                let Ok(view) = poll_api.cloud_thread_bootstrap(&poll_id) else { continue; };
                let terminal = matches!(
                    view.page.runtime_status.as_str(),
                    "completed" | "failed" | "cancelled" | "canceled" | "timeout"
                );
                let _ = poll_weak.upgrade_in_event_loop(move |app| {
                    if active(generation) && is_active(&app) {
                        publish_page(&app, &view.page, false);
                        if terminal {
                            app.set_cloud_thread_busy(false);
                            app.set_stopping(false);
                        }
                    }
                });
                if terminal { return; }
            }
        });
        while active(generation) {
            let Some(event) = watch.blocking_recv() else { break };
            std::thread::sleep(std::time::Duration::from_millis(25));
            let mut events = vec![event];
            while events.len() < 32 {
                match watch.try_recv() {
                    Some(event) => events.push(event),
                    None => break,
                }
            }
            let (applied, ack) = std::sync::mpsc::sync_channel(1);
            let weak_event = weak.clone();
            let event_api = api.clone();
            let _ = weak_event.upgrade_in_event_loop(move |app| {
                if !active(generation) || !is_active(&app) {
                    let _ = applied.send(());
                    return;
                }
                for event in events {
                    match event {
                        CloudThreadEvent::Change(change) => {
                            let page = ThreadViewPage {
                                runtime_status: change.runtime_status,
                                chat: change.rows,
                                ..ThreadViewPage::default()
                            };
                            publish_page(&app, &page, false);
                        }
                        CloudThreadEvent::Connection(status) => {
                            app.set_cloud_thread_status(status.into());
                        }
                        CloudThreadEvent::Reload => {
                            load(&app, event_api.clone());
                            break;
                        }
                    }
                }
                let _ = applied.send(());
            });
            while active(generation) {
                match ack.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }
    });
}

fn refresh(app: &MainWindow, api: Arc<NativeDesktop>, preserve: Option<String>) {
    if app.get_cloud_thread_busy() {
        return;
    }
    app.set_cloud_thread_busy(true);
    let request = CATALOG_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let selection_generation = GENERATION.load(Ordering::Acquire);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api
            .cloud_agents()
            .and_then(|agents| api.cloud_threads(0).map(|threads| (agents, threads)));
        let models = api.cloud_thread_models();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if CATALOG_GENERATION.load(Ordering::Acquire) != request || !app.get_cloud_logged_in() { return; }
            app.set_cloud_thread_busy(false);
            match result {
                Ok((agents, threads)) => {
                    if let Ok(models) = models {
                        app.set_cloud_thread_models(ModelRc::new(VecModel::from(models.into_iter().map(|key| ModelCard {
                            key: key.clone().into(), model: key.into(), provider: "cloud".into(), is_cloud: true,
                            ..Default::default()
                        }).collect::<Vec<_>>())));
                    }
                    let pending = PENDING_SELECTION.with(|slot| slot.borrow_mut().take());
                    let current = selected_id(&app);
                    let preserve = if active(selection_generation) { preserve } else { Some(current) };
                    let wanted = pending.or(preserve).or_else(|| {
                        app.get_cloud_chat_active().then(|| selected_id(&app))
                    });
                    let selected = wanted
                        .as_deref()
                        .and_then(|id| threads.iter().position(|thread| thread.id == id))
                        .map(|index| index as i32 + 1)
                        .unwrap_or(0);
                    app.set_cloud_thread_rows(ModelRc::new(VecModel::from(
                        threads
                            .iter()
                            .map(|thread| CloudThreadCard {
                                id: thread.id.clone().into(),
                                title: thread.title.clone().into(),
                                status: thread.status.clone().into(),
                            })
                            .collect::<Vec<_>>(),
                    )));
                    CATALOG.with(|catalog| *catalog.borrow_mut() = (agents, threads));
                    app.set_cloud_thread_index(selected);
                    // Refreshing another row never changes the active thread.
                    if app.get_cloud_chat_active() {
                        app.set_cloud_active_thread_id(wanted.filter(|_| selected > 0).unwrap_or_default().into());
                    }
                    crate::navigation_ui::project_sidebar(&app, None);
                    if app.get_cloud_chat_active() && active(selection_generation) {
                        load(&app, api);
                    }
                }
                Err(error) => app.set_status(format!("无法读取云端线程：{error}").into()),
            }
        });
    });
}

pub(crate) fn send(app: &MainWindow, api: Arc<NativeDesktop>, message: String) {
    let id = selected_id(app);
    if id.is_empty() || message.trim().is_empty() || app.get_cloud_thread_busy() {
        return;
    }
    // The selected thread already owns its agent. Do not substitute the first
    // agent from the account catalog when sending an existing conversation.
    let agent = String::new();
    let generation = GENERATION.load(Ordering::Acquire);
    app.set_cloud_thread_busy(true);
    app.set_busy(true);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.cloud_thread_send(&id, &agent, &message);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) || !is_active(&app) {
                return;
            }
            app.set_cloud_thread_busy(false);
            match result {
                Ok(_) => {
                    app.set_draft("".into());
                    load(&app, api);
                }
                Err(error) => {
                    app.set_busy(false);
                    app.set_status(format!("无法发送云端消息：{error}").into());
                    load(&app, api);
                }
            }
        });
    });
}

pub(crate) fn cancel(app: &MainWindow, api: Arc<NativeDesktop>) {
    let id = selected_id(app);
    if id.is_empty() || app.get_cloud_thread_busy() {
        return;
    }
    let generation = GENERATION.load(Ordering::Acquire);
    app.set_cloud_thread_busy(true);
    app.set_stopping(true);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.cloud_thread_cancel(&id);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) || !is_active(&app) {
                return;
            }
            app.set_cloud_thread_busy(false);
            app.set_stopping(false);
            match result {
                Ok(()) => load(&app, api),
                Err(error) => {
                    app.set_status(format!("无法停止云端线程：{error}").into());
                    load(&app, api);
                }
            }
        });
    });
}

pub(crate) fn set_model(app: &MainWindow, api: Arc<NativeDesktop>, key: String) {
    if !is_active(app) || app.get_cloud_thread_busy() { return; }
    let id = selected_id(app);
    let generation = GENERATION.load(Ordering::Acquire);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.cloud_thread_set_model(&id, &key);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) || !is_active(&app) { return; }
            match result {
                Ok(()) => app.set_selected_model_key(key.into()),
                Err(error) => app.set_status(format!("无法切换云端模型：{error}").into()),
            }
        });
    });
}

pub(crate) fn set_effort(app: &MainWindow, api: Arc<NativeDesktop>, effort: String) {
    if !is_active(app) || app.get_cloud_thread_busy() { return; }
    let id = selected_id(app);
    let generation = GENERATION.load(Ordering::Acquire);
    let previous = app.get_reasoning_effort();
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.cloud_thread_set_effort(&id, &effort);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) || !is_active(&app) { return; }
            match result {
                Ok(value) => app.set_reasoning_effort(value.into()),
                Err(error) => {
                    app.set_reasoning_effort(previous);
                    app.set_status(format!("无法保存云端思考等级：{error}").into());
                }
            }
        });
    });
}

pub(crate) fn rename(app: &MainWindow, api: Arc<NativeDesktop>, id: String, title: String) {
    if id.is_empty() || title.trim().is_empty() {
        return;
    }
    let weak = app.as_weak();
    let account = ACCOUNT_GENERATION.load(Ordering::Acquire);
    std::thread::spawn(move || {
        let result = api.cloud_thread_rename(&id, &title);
        let _ = weak.upgrade_in_event_loop(move |app| { if ACCOUNT_GENERATION.load(Ordering::Acquire) != account { return; } match result {
            Ok(()) => refresh(&app, api, Some(selected_id(&app))),
            Err(error) => app.set_status(format!("无法重命名云端线程：{error}").into()),
        }});
    });
}

pub(crate) fn archive(app: &MainWindow, api: Arc<NativeDesktop>, id: String) {
    if id.is_empty() {
        return;
    }
    let account = ACCOUNT_GENERATION.load(Ordering::Acquire);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.cloud_thread_archive(&id);
        let _ = weak.upgrade_in_event_loop(move |app| { if ACCOUNT_GENERATION.load(Ordering::Acquire) != account { return; } match result {
            Ok(()) => {
                if is_active(&app) && selected_id(&app) == id {
                    invalidate();
                    app.set_cloud_thread_index(0);
                    app.set_cloud_active_thread_id("".into());
                    app.set_busy(false);
                    crate::native_chat::replace_external_timeline(&app, Vec::new(), true);
                }
                refresh(&app, api, None);
            }
            Err(error) => app.set_status(format!("无法归档云端线程：{error}").into()),
        }});
    });
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_copy_original(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let root = id.strip_prefix("cloud-answer-").unwrap_or_default();
        let Some(message) = MESSAGES.with(|slot| slot.borrow().iter().find(|row| row.root_turn_id == root).map(|row| row.assistant.clone())) else { return };
        let thread = selected_id(&app);
        let generation = GENERATION.load(Ordering::Acquire);
        let weak = app.as_weak();
        let backend = backend.clone();
        std::thread::spawn(move || {
            let result = backend.cloud_message_text(&thread, &message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if !active(generation) || !is_active(&app) { return; }
                match result {
                    Ok(text) => app.invoke_copy_raw(text.into()),
                    Err(error) => app.set_status(format!("无法复制消息原文：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_rename_thread(move |id, title| {
        if let Some(app) = weak.upgrade() {
            rename(&app, backend.clone(), id.to_string(), title.to_string());
        }
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_archive_thread(move |id| {
        if let Some(app) = weak.upgrade() {
            archive(&app, backend.clone(), id.to_string());
        }
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_open_threads(move || {
        if let Some(app) = weak.upgrade() {
            if !app.get_cloud_chat_active() {
                crate::native_chat::prepare_external_timeline(&app);
                LOADED_ID.with(|slot| slot.borrow_mut().clear());
            }
            app.set_cloud_chat_active(true);
            refresh(&app, backend.clone(), Some(selected_id(&app)));
        }
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_new_thread(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_thread_busy() || !app.get_cloud_logged_in() {
            return;
        }
        // The workspace add button owns its target even when the previous
        // chat and directory belonged to a local workspace.
        crate::workspace_files::set_target(&app, backend.clone(), crate::workspace_files::Target::Cloud);
        app.set_selected_workspace(-1);
        let agent = selected_agent();
        app.set_cloud_thread_busy(true);
        let account = ACCOUNT_GENERATION.load(Ordering::Acquire);
        let weak = app.as_weak();
        let api = backend.clone();
        std::thread::spawn(move || {
            let result = api.cloud_thread_create(&agent, "");
            let _ = weak.upgrade_in_event_loop(move |app| {
                if ACCOUNT_GENERATION.load(Ordering::Acquire) != account { return; }
                app.set_cloud_thread_busy(false);
                match result {
                    Ok(thread) => {
                        PENDING_SELECTION.with(|slot| *slot.borrow_mut() = Some(thread.id));
                        app.set_cloud_chat_active(true);
                        refresh(&app, api, None);
                    }
                    Err(error) => app.set_status(format!("无法新建云端线程：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_reload_threads(move || {
        if let Some(app) = weak.upgrade() {
            refresh(&app, backend.clone(), Some(selected_id(&app)));
        }
    });
    let weak = app.as_weak();
    app.on_cloud_select_thread(move |index| {
        if let Some(app) = weak.upgrade() {
            let id = CATALOG.with(|catalog| {
                catalog.borrow().1.get(index.saturating_sub(1) as usize)
                    .filter(|_| index > 0).map(|thread| thread.id.clone()).unwrap_or_default()
            });
            app.set_cloud_active_thread_id(id.into());
            crate::navigation_ui::project_sidebar(&app, None);
            load(&app, api.clone());
        }
    });
}
