//! Subagent detail dialog: the child thread rendered by the same timeline
//! projection the main chat uses. Opening a card resolves the child's own
//! turns through `load_chat_turns`, projects them with `project_history`, and
//! hands the rows to the dialog's `TimelineView` — one rendering path, no
//! parallel one. While the dialog is open a bounded timer refreshes the rows
//! and the header card, so a streaming child keeps the live tail moving.
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use wunder_desktop::NativeDesktop;

/// Refresh cadence while the dialog is open. The reload is paged and bounded,
/// so the cadence only decides latency, never the cost shape.
const REFRESH_INTERVAL: Duration = Duration::from_millis(1500);

struct State {
    /// The child thread the dialog shows.
    child: String,
    /// The parent thread the card belongs to; the header card and interrupt
    /// resolve through it.
    parent: String,
    model: Rc<VecModel<crate::TimelineRow>>,
    clock: slint::Timer,
}

pub fn install(app: &crate::MainWindow, api: Arc<NativeDesktop>) {
    let state = Rc::new(RefCell::new(State {
        child: String::new(),
        parent: String::new(),
        model: Rc::new(VecModel::default()),
        clock: slint::Timer::default(),
    }));
    let weak = app.as_weak();

    // A card click opens the dialog on the child thread it carries.
    let (open_weak, open_state, open_api) = (weak.clone(), state.clone(), api.clone());
    app.on_timeline_open_subagent(move |child| {
        let child = child.trim().to_string();
        if child.is_empty() {
            return;
        }
        let Some(app) = open_weak.upgrade() else {
            return;
        };
        let parent = app.get_active_session_id().to_string();
        let model = open_state.borrow().model.clone();
        {
            let mut current = open_state.borrow_mut();
            current.child = child.clone();
            current.parent = parent.clone();
            model.set_vec(Vec::new());
        }
        app.set_subagent_rows(ModelRc::from(model));
        app.set_subagent_title("子智能体线程".into());
        app.set_subagent_task("".into());
        app.set_subagent_metrics("".into());
        app.set_subagent_status_kind(crate::timeline::STATUS_RUNNING);
        app.set_subagent_running(true);
        app.set_subagent_stoppable(false);
        app.set_subagent_loading(true);
        app.set_subagent_failed(false);
        app.set_subagent_open(true);
        // The refresh clock applies every snapshot; one immediate pass keeps
        // the header from waiting a full interval.
        let mut tick = refresh_tick(open_weak.clone(), open_state.clone(), open_api.clone());
        tick();
        open_state
            .borrow_mut()
            .clock
            .start(slint::TimerMode::Repeated, REFRESH_INTERVAL, tick);
    });

    // Interrupt from the card or from the dialog header: the runtime settles
    // the child; the card and the header pick the new state up on their own
    // feeds (the durable reload and the refresh clock). Either way the parent
    // is the thread the user is looking at — the card lives in the active
    // timeline and the dialog overlays it — so the active session is the
    // parent identity, not the dialog's stored one (a card can be stopped
    // before the dialog has ever been opened).
    let stop_api = api.clone();
    let stop = move |app: &crate::MainWindow, child: String| {
        let parent = app.get_active_session_id().trim().to_string();
        if child.is_empty() || parent.is_empty() {
            return;
        }
        let backend = stop_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            if let Err(error) = backend.subagent_control(&parent, "interrupt", &child) {
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.set_status(format!("中断子智能体失败：{error}").into());
                });
            }
        });
    };
    let (card_weak, card_stop) = (weak.clone(), stop.clone());
    app.on_timeline_stop_subagent(move |child| {
        let Some(app) = card_weak.upgrade() else {
            return;
        };
        card_stop(&app, child.trim().to_string());
    });
    let (head_weak, head_stop, head_state) = (weak.clone(), stop.clone(), state.clone());
    app.on_subagent_interrupt(move || {
        let Some(app) = head_weak.upgrade() else {
            return;
        };
        let child = head_state.borrow().child.clone();
        head_stop(&app, child);
    });

    let close_weak = weak.clone();
    let close_state = state.clone();
    app.on_subagent_close(move || {
        let Some(app) = close_weak.upgrade() else {
            return;
        };
        close_state.borrow().clock.stop();
        app.set_subagent_open(false);
    });

    // Fold toggles patch the dialog's own model in place, the same rule the
    // main timeline's reducer applies to its rows.
    let model = state.borrow().model.clone();
    app.on_subagent_toggle(move |payload| {
        let Ok(payload) = usize::try_from(payload) else {
            return;
        };
        let Some(row) = model.row_data(payload) else {
            return;
        };
        if row.kind == crate::timeline::KIND_GROUP {
            let open = !row.open;
            let group = row.group_idx;
            for index in 0..model.row_count() {
                let Some(mut target) = model.row_data(index) else {
                    continue;
                };
                if target.group_idx != group {
                    continue;
                }
                if target.kind == crate::timeline::KIND_GROUP {
                    target.open = open;
                }
                target.group_open = open;
                model.set_row_data(index, target);
            }
            return;
        }
        let mut row = row;
        row.open = !row.open;
        model.set_row_data(payload, row);
    });

    let save_weak = weak.clone();
    app.on_subagent_download(move |text| {
        let Some(app) = save_weak.upgrade() else {
            return;
        };
        let text = text.to_string();
        if text.trim().is_empty() {
            return;
        }
        match crate::file_dialog::save_text_file("保存回复", &text) {
            Ok(Some(path)) => app.set_status(format!("已保存到 {path}").into()),
            Ok(None) => {}
            Err(error) => app.set_status(format!("保存失败：{error}").into()),
        }
    });

    let open_weak = weak.clone();
    app.on_subagent_open_resource(move |resource| {
        let Some(app) = open_weak.upgrade() else {
            return;
        };
        let resource = resource.to_string();
        if resource.is_empty() {
            return;
        }
        app.invoke_open_file_native(resource.into());
    });
}

/// One refresh pass: reload the header card and the child's turns off the UI
/// thread, then apply whichever arrived. Runs on the clock while the dialog is
/// open and once right after opening.
fn refresh_tick(
    weak: slint::Weak<crate::MainWindow>,
    state: Rc<RefCell<State>>,
    api: Arc<NativeDesktop>,
) -> impl FnMut() + 'static {
    move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if !app.get_subagent_open() {
            return;
        }
        let (child, parent) = {
            let current = state.borrow();
            (current.child.clone(), current.parent.clone())
        };
        if child.is_empty() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let backend = api.clone();
        let lookup = child.clone();
        std::thread::spawn(move || {
            let cards = if parent.is_empty() {
                Vec::new()
            } else {
                backend.subagent_cards(&parent).unwrap_or_default()
            };
            let turns = backend.load_chat_turns(&child).ok();
            let _ = tx.send((cards, turns));
        });
        let (weak, state) = (weak.clone(), state.clone());
        poll(move || {
            let Ok((cards, turns)) = rx.try_recv() else {
                return false;
            };
            let Some(app) = weak.upgrade() else {
                return true;
            };
            if !app.get_subagent_open() {
                return true;
            }
            if let Some(card) = cards.iter().find(|card| card.session_id == lookup.as_str()) {
                apply_card(&app, card);
            }
            match turns {
                Some(turns) => {
                    let rows = crate::native_chat::project_history(&turns);
                    state.borrow().model.set_vec(rows);
                    app.set_subagent_loading(false);
                    app.set_subagent_failed(false);
                    if let Some(task) = turns
                        .iter()
                        .find(|turn| !turn.user.text.is_empty())
                        .map(|turn| turn.user.text.clone())
                    {
                        app.set_subagent_task(task.into());
                    }
                }
                None => {
                    app.set_subagent_loading(false);
                    app.set_subagent_failed(true);
                }
            }
            true
        });
    }
}

/// Project one card onto the dialog header: identity, live status and the
/// bounded counter line.
fn apply_card(app: &crate::MainWindow, card: &wunder_desktop::NativeSubagentCard) {
    if !card.title.is_empty() {
        app.set_subagent_title(card.title.as_str().into());
    }
    let status_kind = if card.failed || matches!(card.status.as_str(), "failed" | "error") {
        crate::timeline::STATUS_FAILED
    } else if card.is_running() {
        crate::timeline::STATUS_RUNNING
    } else {
        crate::timeline::STATUS_DONE
    };
    app.set_subagent_status_kind(status_kind);
    app.set_subagent_running(status_kind == crate::timeline::STATUS_RUNNING);
    app.set_subagent_stoppable(
        card.can_terminate && status_kind == crate::timeline::STATUS_RUNNING,
    );
    let mut metrics = format!(
        "{} 次工具调用 · {} 次模型请求",
        card.tool_calls, card.model_requests
    );
    if card.context_tokens > 0 {
        metrics.push_str(&format!(" · 上下文 {} tok", card.context_tokens));
    }
    app.set_subagent_metrics(metrics.into());
}

/// Repeat a check on the UI thread until it reports done, the same cadence the
/// trajectory dialog loads its snapshot with.
fn poll(mut receive: impl FnMut() -> bool + 'static) {
    slint::Timer::single_shot(Duration::from_millis(33), move || {
        if !receive() {
            poll(receive);
        }
    });
}
