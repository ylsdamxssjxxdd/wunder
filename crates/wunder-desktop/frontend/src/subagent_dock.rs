use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use wunder_desktop::{NativeDesktop, NativeSubagentCard};

use crate::{MainWindow, SubagentItem};

const TICK: Duration = Duration::from_millis(1000);
const MAX_ROWS: usize = 6;

type CardsResult = anyhow::Result<Vec<NativeSubagentCard>>;

thread_local! {
    static TIMER: Timer = Timer::default();
    static BRIDGE: RefCell<Option<Rc<RefCell<Bridge>>>> = const { RefCell::new(None) };
}

struct Bridge {
    app: slint::Weak<MainWindow>,
    desktop: Arc<NativeDesktop>,
    model: Rc<VecModel<SubagentItem>>,
    rows: Vec<SubagentItem>,
    session: String,
    pending: Option<mpsc::Receiver<CardsResult>>,
}

pub fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    let model = Rc::new(VecModel::<SubagentItem>::default());
    app.set_subagent_items(ModelRc::from(model.clone()));
    app.set_subagent_count(0);
    app.set_subagent_running_count(0);
    let bridge = Rc::new(RefCell::new(Bridge {
        app: app.as_weak(),
        desktop,
        model,
        rows: Vec::new(),
        session: String::new(),
        pending: None,
    }));
    BRIDGE.with(|slot| *slot.borrow_mut() = Some(bridge.clone()));

    let weak = Rc::downgrade(&bridge);
    TIMER.with(|timer| {
        timer.start(TimerMode::Repeated, TICK, move || {
            let Some(bridge) = weak.upgrade() else {
                return;
            };
            tick(&mut bridge.borrow_mut());
        });
    });
}

fn tick(bridge: &mut Bridge) {
    let Some(app) = bridge.app.upgrade() else {
        return;
    };
    if !app.window().is_visible() {
        return;
    }
    let session = if app.get_cloud_chat_active() {
        String::new()
    } else {
        app.get_active_session_id().trim().to_string()
    };
    if session != bridge.session {
        bridge.session = session;
        bridge.pending = None;
        publish(bridge, Vec::new());
    }
    if let Some(pending) = &bridge.pending {
        match pending.try_recv() {
            Ok(Ok(cards)) => {
                bridge.pending = None;
                let rows = cards.iter().map(project).collect();
                publish(bridge, rows);
            }
            Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                bridge.pending = None;
            }
            Err(mpsc::TryRecvError::Empty) => return,
        }
    }
    if bridge.pending.is_some() || bridge.session.is_empty() || app.get_cloud_chat_active() {
        return;
    }

    let (tx, rx) = mpsc::sync_channel(1);
    bridge.pending = Some(rx);
    let desktop = bridge.desktop.clone();
    let session = bridge.session.clone();
    std::thread::spawn(move || {
        let _ = tx.send(desktop.subagent_cards(&session));
    });
}

fn project(card: &NativeSubagentCard) -> SubagentItem {
    let status_kind = if card.failed || matches!(card.status.as_str(), "failed" | "error") {
        crate::timeline::STATUS_FAILED
    } else if card.is_running() {
        crate::timeline::STATUS_RUNNING
    } else {
        crate::timeline::STATUS_DONE
    };
    let message = if card.latest_message.trim().is_empty() {
        card.error_message.as_str()
    } else {
        card.latest_message.as_str()
    };
    let mut summary: String = message
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(160)
        .collect();
    if summary.is_empty() && status_kind == crate::timeline::STATUS_RUNNING {
        summary = "正在执行…".into();
    }
    SubagentItem {
        id: card.session_id.as_str().into(),
        title: card.title.as_str().into(),
        summary: summary.into(),
        status_kind,
        stoppable: card.can_terminate && status_kind == crate::timeline::STATUS_RUNNING,
    }
}

fn publish(bridge: &mut Bridge, mut rows: Vec<SubagentItem>) {
    let Some(app) = bridge.app.upgrade() else {
        return;
    };
    let count = rows.len() as i32;
    if app.get_subagent_count() != count {
        app.set_subagent_count(count);
    }
    let running = rows
        .iter()
        .filter(|row| row.status_kind == crate::timeline::STATUS_RUNNING)
        .count() as i32;
    if app.get_subagent_running_count() != running {
        app.set_subagent_running_count(running);
    }
    rows.truncate(MAX_ROWS);
    let reshaped = rows.len() != bridge.rows.len()
        || rows
            .iter()
            .zip(bridge.rows.iter())
            .any(|(next, previous)| next.id != previous.id);
    if reshaped {
        bridge.model.set_vec(rows.clone());
    } else {
        for (index, row) in rows.iter().enumerate() {
            if bridge.rows.get(index) != Some(row) {
                bridge.model.set_row_data(index, row.clone());
            }
        }
    }
    bridge.rows = rows;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_card_is_stoppable_and_summarized() {
        let card = NativeSubagentCard {
            session_id: "child".into(),
            title: "Child".into(),
            latest_message: "working\nmore".into(),
            can_terminate: true,
            ..Default::default()
        };
        let item = project(&card);
        assert_eq!(item.id, "child");
        assert_eq!(item.summary, "working");
        assert_eq!(item.status_kind, crate::timeline::STATUS_RUNNING);
        assert!(item.stoppable);
    }

    #[test]
    fn terminal_card_cannot_be_stopped() {
        let card = NativeSubagentCard {
            session_id: "child".into(),
            terminal: true,
            can_terminate: true,
            ..Default::default()
        };
        let item = project(&card);
        assert_eq!(item.status_kind, crate::timeline::STATUS_DONE);
        assert!(!item.stoppable);
    }
}
