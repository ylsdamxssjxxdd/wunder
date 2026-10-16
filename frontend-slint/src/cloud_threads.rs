//! Cloud view work runs off the UI thread; only bounded row deltas reach Slint.
use crate::{CloudTurn, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use wunder_desktop::{
    native::{NativeCloudAgent, NativeCloudThread},
    NativeDesktop,
};
use wunder_server::{
    cloud::CloudThreadEvent,
    stable_core::thread_view::{ChatViewRow, ThreadViewPage},
};

thread_local! {
    static CATALOG: RefCell<(Vec<NativeCloudAgent>,Vec<NativeCloudThread>)> = RefCell::new((Vec::new(),Vec::new()));
    static ROWS: Rc<VecModel<CloudTurn>> = Rc::new(VecModel::default());
    static WATCH: RefCell<Option<wunder_server::cloud::CloudThreadCancellation>> = const { RefCell::new(None) };
}
static GENERATION: AtomicU64 = AtomicU64::new(0);
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
fn thread_id(app: &MainWindow) -> String {
    CATALOG.with(|catalog| {
        catalog
            .borrow()
            .1
            .get(app.get_cloud_thread_index().saturating_sub(1) as usize)
            .filter(|_| app.get_cloud_thread_index() > 0)
            .map(|row| row.id.clone())
            .unwrap_or_default()
    })
}
fn row(value: ChatViewRow) -> CloudTurn {
    let assistant = if value.assistant.tail.is_empty() {
        value.assistant.content
    } else if value.assistant.tail_offset == 0 {
        value.assistant.tail
    } else {
        format!("{}\n…\n{}", value.assistant.content, value.assistant.tail)
    };
    CloudTurn {
        id: value.root_turn_id.into(),
        user: value.user.content.into(),
        assistant: assistant.into(),
        status: value.status.into(),
    }
}
fn apply_rows(app: &MainWindow, rows: Vec<ChatViewRow>, reset: bool) {
    ROWS.with(|model| {
        if reset {
            model.set_vec(
                rows.into_iter()
                    .rev()
                    .take(20)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .map(row)
                    .collect::<Vec<_>>(),
            );
        } else {
            for incoming in rows {
                let incoming = row(incoming);
                if let Some(index) = (0..model.row_count()).find(|&index| {
                    model
                        .row_data(index)
                        .is_some_and(|old| old.id == incoming.id)
                }) {
                    if model.row_data(index).is_some_and(|old| old != incoming) {
                        model.set_row_data(index, incoming);
                    }
                } else {
                    model.push(incoming);
                }
            }
            while model.row_count() > 20 {
                model.remove(0);
            }
        }
        app.set_cloud_turns(ModelRc::from(model.clone()));
    });
}
fn load(app: &MainWindow, api: Arc<NativeDesktop>) {
    let id = thread_id(app);
    let generation = invalidate();
    app.set_cloud_thread_busy(!id.is_empty());
    if id.is_empty() {
        apply_rows(app, Vec::new(), true);
        return;
    }
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let page = api.cloud_thread_view(&id);
        let (page, mut watch) = match page {
            Ok(page) => {
                let watch = api.cloud_thread_watch(&id, page.view_seq);
                (page, watch)
            }
            Err(error) => {
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if active(generation) {
                        app.set_cloud_thread_busy(false);
                        app.set_cloud_thread_status(error.to_string().into());
                    }
                });
                return;
            }
        };
        let token = watch.cancel_token();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) {
                token.cancel();
                return;
            }
            WATCH.with(|watch| *watch.borrow_mut() = Some(token));
            apply_page(&app, page);
            app.set_cloud_thread_busy(false);
        });
        while active(generation) {
            let Some(event) = watch.blocking_recv() else {
                break;
            };
            // Bounded producer/UI handoff. At most one callback can be queued;
            // the upstream channel supplies backpressure while UI is busy.
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
            let _ = weak_event.upgrade_in_event_loop(move |app| {
                let _ = applied.send(());
                if !active(generation) {
                    return;
                }
                for event in events {
                    match event {
                        CloudThreadEvent::Change(change) => {
                            apply_rows(&app, change.rows, false);
                            app.set_cloud_thread_status(change.runtime_status.into());
                        }
                        CloudThreadEvent::Connection(state) => {
                            app.set_cloud_thread_status(state.into())
                        }
                        CloudThreadEvent::Reload => {
                            app.invoke_cloud_select_thread(app.get_cloud_thread_index())
                        }
                    }
                }
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
fn apply_page(app: &MainWindow, page: ThreadViewPage) {
    app.set_cloud_thread_status(page.runtime_status.into());
    apply_rows(app, page.chat, true);
}
fn refresh(app: &MainWindow, api: Arc<NativeDesktop>) {
    if app.get_cloud_thread_busy() {
        return;
    }
    app.set_cloud_thread_busy(true);
    let generation = invalidate();
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api
            .cloud_agents()
            .and_then(|agents| api.cloud_threads(0).map(|threads| (agents, threads)));
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) {
                return;
            }
            app.set_cloud_thread_busy(false);
            match result {
                Ok((agents, threads)) => {
                    app.set_cloud_agent_names(ModelRc::from(Rc::new(VecModel::from(
                        agents
                            .iter()
                            .map(|row| row.name.clone().into())
                            .collect::<Vec<slint::SharedString>>(),
                    ))));
                    let mut names: Vec<slint::SharedString> = vec!["＋ 新线程 / New thread".into()];
                    names.extend(
                        threads
                            .iter()
                            .map(|row| format!("{} · {}", row.title, row.status).into()),
                    );
                    app.set_cloud_thread_names(ModelRc::from(Rc::new(VecModel::from(names))));
                    CATALOG.with(|catalog| *catalog.borrow_mut() = (agents, threads));
                    app.set_cloud_thread_index(0);
                    app.set_cloud_agent_index(0);
                    apply_rows(&app, Vec::new(), true);
                    app.set_cloud_thread_status("最近 50 个线程 / Up to 50 recent threads".into());
                }
                Err(error) => app.set_cloud_thread_status(error.to_string().into()),
            }
        });
    });
}
pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_open_threads(move || {
        if let Some(app) = weak.upgrade() {
            app.set_cloud_threads_open(true);
            refresh(&app, backend.clone());
        }
    });
    let weak = app.as_weak();
    app.on_cloud_close_threads(move || {
        invalidate();
        if let Some(app) = weak.upgrade() {
            app.set_cloud_threads_open(false);
            app.set_cloud_thread_busy(false);
        }
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_reload_threads(move || {
        if let Some(app) = weak.upgrade() {
            refresh(&app, backend.clone());
        }
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_select_thread(move |_| {
        if let Some(app) = weak.upgrade() {
            load(&app, backend.clone());
        }
    });
    for action in 0..3 {
        let weak = app.as_weak();
        let backend = api.clone();
        let run = move |approve: bool| {
            let Some(app) = weak.upgrade() else { return };
            if app.get_cloud_thread_busy() {
                return;
            }
            let id = thread_id(&app);
            let message = app.get_cloud_thread_message().to_string();
            let approval = app.get_cloud_thread_approval().to_string();
            let agent = CATALOG.with(|catalog| {
                catalog
                    .borrow()
                    .0
                    .get(app.get_cloud_agent_index() as usize)
                    .map(|agent| agent.id.clone())
                    .unwrap_or_default()
            });
            let generation = invalidate();
            app.set_cloud_thread_busy(true);
            let backend = backend.clone();
            let weak = app.as_weak();
            std::thread::spawn(move || {
                let result = match action {
                    0 => backend.cloud_thread_send(&id, &agent, &message),
                    1 => backend.cloud_thread_cancel(&id).map(|()| id.clone()),
                    _ => backend
                        .cloud_thread_answer(&id, &approval, approve)
                        .map(|()| id.clone()),
                };
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if !active(generation) {
                        return;
                    }
                    app.set_cloud_thread_busy(false);
                    match result {
                        Ok(id) => {
                            if action == 0 {
                                app.set_cloud_thread_message("".into());
                            }
                            let index = CATALOG.with(|catalog| {
                                let mut catalog = catalog.borrow_mut();
                                if let Some(index) = catalog.1.iter().position(|row| row.id == id) {
                                    index + 1
                                } else {
                                    catalog.1.insert(
                                        0,
                                        NativeCloudThread {
                                            id: id.clone(),
                                            title: "新线程 / New thread".into(),
                                            status: "active".into(),
                                        },
                                    );
                                    catalog.1.truncate(50);
                                    let mut names: Vec<slint::SharedString> =
                                        vec!["＋ 新线程 / New thread".into()];
                                    names.extend(
                                        catalog.1.iter().map(|row| row.title.clone().into()),
                                    );
                                    app.set_cloud_thread_names(ModelRc::from(Rc::new(
                                        VecModel::from(names),
                                    )));
                                    1
                                }
                            });
                            app.set_cloud_thread_index(index as i32);
                            load(&app, backend);
                        }
                        Err(error) => app.set_cloud_thread_status(error.to_string().into()),
                    }
                });
            });
        };
        match action {
            0 => app.on_cloud_send_thread(move || run(true)),
            1 => app.on_cloud_stop_thread(move || run(true)),
            _ => app.on_cloud_answer_thread(run),
        };
    }
}
