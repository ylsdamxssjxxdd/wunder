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
        duration: duration_text(card, now_epoch_seconds()).as_str().into(),
        credits: format_compact_count(card.credits).as_str().into(),
        context: context_text(card).as_str().into(),
    }
}

fn now_epoch_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

/// 运行时长：在跑的按开始时间实时推算；已结束优先用运行记录自己的 elapsed_s。
fn duration_seconds(card: &NativeSubagentCard, now_s: f64) -> f64 {
    if card.is_running() && card.started_at > 0.0 {
        return (now_s - card.started_at).max(0.0);
    }
    if card.elapsed_s > 0.0 {
        return card.elapsed_s;
    }
    if card.started_at > 0.0 && card.finished_at >= card.started_at {
        return card.finished_at - card.started_at;
    }
    0.0
}

/// 与云端 `formatDurationSeconds` 同形：`1m 5s` / `2h 3m 4s`。
fn format_elapsed(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let secs = total % 60;
    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

fn duration_text(card: &NativeSubagentCard, now_s: f64) -> String {
    let seconds = duration_seconds(card, now_s);
    if seconds < 0.5 {
        "--".into()
    } else {
        format_elapsed(seconds)
    }
}

/// 与云端 `formatCompactCount` 同形：`7` / `1.2k` / `3m`。
fn format_compact_count(value: i64) -> String {
    let count = value.max(0);
    if count < 1_000 {
        return count.to_string();
    }
    let (divisor, unit) = if count >= 1_000_000 {
        (1_000_000.0, "m")
    } else {
        (1_000.0, "k")
    };
    let compact = count as f64 / divisor;
    let digits = if compact >= 100.0 { 0 } else { 1 };
    let mut text = format!("{:.*}", digits, compact);
    if text.ends_with(".0") {
        text.truncate(text.len() - 2);
    }
    format!("{}{}", text, unit)
}

fn context_text(card: &NativeSubagentCard) -> String {
    if card.context_tokens <= 0 && card.max_context <= 0 {
        return "--".into();
    }
    if card.max_context > 0 {
        let percent = (card.context_tokens.max(0) as f64 / card.max_context as f64 * 100.0).round();
        return format!("{}%", percent as i64);
    }
    format_compact_count(card.context_tokens)
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
    fn running_card_reports_live_duration_and_stats() {
        let card = NativeSubagentCard {
            session_id: "child".into(),
            terminal: false,
            status: "running".into(),
            started_at: now_epoch_seconds() - 125.0,
            credits: 7,
            context_tokens: 2_048,
            max_context: 8_192,
            ..Default::default()
        };
        let item = project(&card);
        assert!(item.duration.as_str().starts_with("2m 5"), "{}", item.duration);
        assert_eq!(item.credits.as_str(), "7");
        assert_eq!(item.context.as_str(), "25%");
    }

    #[test]
    fn finished_card_prefers_recorded_elapsed() {
        let card = NativeSubagentCard {
            session_id: "child".into(),
            terminal: true,
            status: "success".into(),
            started_at: 1_000.0,
            finished_at: 1_030.0,
            elapsed_s: 42.0,
            ..Default::default()
        };
        assert_eq!(duration_text(&card, 9_999.0), "42s");
        // 没有时间戳时退回 --，不显示 0s。
        let bare = NativeSubagentCard { session_id: "child".into(), ..Default::default() };
        assert_eq!(duration_text(&bare, 9_999.0), "--");
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
