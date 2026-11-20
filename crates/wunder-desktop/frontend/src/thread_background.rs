//! 线程的「后台运行中」状态投影。
//!
//! 模型把命令或子智能体交给后台并结束本轮后，最后一条轮次是完成的，但线程
//! 并没有闲着。这里每秒读一次仍在运行的命令会话与子运行，把对应线程行从终态
//! 切成运行中；后台工作结束（且没有新轮次）时恢复覆盖前的终态。
//!
//! 覆盖只发生在线程没有前台轮次时：正在生成/排队的线程本来就不是终态。

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Model, Timer, TimerMode};
use wunder_desktop::NativeDesktop;

use crate::runtime_phase::Phase;
use crate::MainWindow;

const TICK: Duration = Duration::from_millis(1000);
/// 最后一条后台工作结束后再等一小会儿再恢复终态：完成通知的合并窗口与
/// 唤醒轮次通常在这段时间里到达，避免「后台运行中 → 任务完成 → 正在生成」
/// 的闪烁。
const RESTORE_GRACE: Duration = Duration::from_secs(3);
pub(crate) const BACKGROUND_STATUS: &str = "后台运行中";

#[derive(Clone)]
struct Settled {
    status: String,
    phase: Phase,
    ended_at: f32,
}

thread_local! {
    static TIMER: Timer = Timer::default();
    static DESKTOP: RefCell<Option<Arc<NativeDesktop>>> = const { RefCell::new(None) };
    /// 覆盖成「后台运行中」之前的终态；后台工作结束时用它恢复。
    static SETTLED: RefCell<HashMap<String, Settled>> = RefCell::new(HashMap::new());
    /// 后台工作全部结束后的恢复时刻。
    static RESTORE_AT: RefCell<HashMap<String, Instant>> = RefCell::new(HashMap::new());
}

pub fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    DESKTOP.with(|slot| *slot.borrow_mut() = Some(desktop));
    let weak = app.as_weak();
    TIMER.with(|timer| {
        timer.start(TimerMode::Repeated, TICK, move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if !app.window().is_visible() {
                return;
            }
            tick(&app);
        });
    });
}

fn desktop() -> Option<Arc<NativeDesktop>> {
    DESKTOP.with(|slot| slot.borrow().clone())
}

/// 终态写入前的拦截：仍有后台工作时用运行中替代已完成，并记住原终态。
pub(crate) fn project_status(session: &str, status: &str, phase: Phase) -> (String, Phase) {
    if !phase.terminal() {
        return (status.to_string(), phase);
    }
    let Some(desktop) = desktop() else {
        return (status.to_string(), phase);
    };
    if !desktop.session_has_background_work(session) {
        return (status.to_string(), phase);
    }
    remember(
        session,
        status,
        phase,
        crate::native_chat::settled_now() as f32,
    );
    (BACKGROUND_STATUS.to_string(), Phase::Running)
}

fn remember(session: &str, status: &str, phase: Phase, ended_at: f32) {
    SETTLED.with(|slot| {
        slot.borrow_mut()
            .entry(session.to_string())
            .or_insert_with(|| Settled {
                status: status.to_string(),
                phase,
                ended_at,
            });
    });
}

fn tick(app: &MainWindow) {
    let Some(desktop) = desktop() else {
        return;
    };
    let background = desktop.background_work_sessions();
    let rows = app.get_conversations();
    for index in 0..rows.row_count() {
        let Some(row) = rows.row_data(index) else {
            continue;
        };
        let session = row.id.to_string();
        let active = background.contains(&session);
        let phase = Phase::from_code(row.runtime_phase);
        let shown = row.runtime_status.as_str() == BACKGROUND_STATUS;
        if active {
            RESTORE_AT.with(|slot| {
                slot.borrow_mut().remove(&session);
            });
            if !phase.live() {
                // 目录读回来的终态（或 idle）先记下来再覆盖。
                let status = row.runtime_status.to_string();
                let ended_at = if phase.terminal() {
                    row.runtime_ended_at
                } else {
                    0.0
                };
                remember(&session, &status, phase, ended_at);
                crate::native_chat::publish_background_status(
                    app,
                    &session,
                    BACKGROUND_STATUS,
                    Phase::Running,
                );
            }
        } else if shown {
            let now = Instant::now();
            let due = RESTORE_AT.with(|slot| {
                *slot
                    .borrow_mut()
                    .entry(session.clone())
                    .or_insert(now + RESTORE_GRACE)
            });
            if now < due {
                continue;
            }
            RESTORE_AT.with(|slot| {
                slot.borrow_mut().remove(&session);
            });
            let settled = SETTLED.with(|slot| slot.borrow_mut().remove(&session));
            match settled {
                Some(settled) if !settled.status.is_empty() => {
                    crate::native_chat::restore_thread_status(
                        app,
                        &session,
                        &settled.status,
                        settled.phase,
                        settled.ended_at,
                    );
                }
                _ => crate::native_chat::restore_thread_status(
                    app,
                    &session,
                    "idle",
                    Phase::Idle,
                    0.0,
                ),
            }
        } else {
            RESTORE_AT.with(|slot| {
                slot.borrow_mut().remove(&session);
            });
            SETTLED.with(|slot| {
                slot.borrow_mut().remove(&session);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_settled_row_with_background_work_is_relabelled() {
        assert_eq!(
            decide(true, Phase::Done),
            RowAction::Mark,
            "a finished turn with background work still runs"
        );
        assert_eq!(decide(true, Phase::Stopped), RowAction::Mark);
        assert_eq!(
            decide(true, Phase::Running),
            RowAction::Keep,
            "a live turn owns the status already"
        );
        assert_eq!(decide(false, Phase::Done), RowAction::Keep);
    }

    #[test]
    fn a_background_label_is_restored_when_work_ends() {
        assert_eq!(decide(false, Phase::Running), RowAction::Restore);
        assert_eq!(decide(true, Phase::Running), RowAction::Keep);
    }

    /// 与 `tick` 相同的判定，只抽出状态机便于测试。
    #[derive(Debug, PartialEq, Eq)]
    enum RowAction {
        Mark,
        Restore,
        Keep,
    }

    fn decide(active: bool, phase: Phase) -> RowAction {
        // tick 用 runtime_status 判断是否是我们覆盖的行；纯状态机里用 Running
        // 代表它（覆盖后的 phase 恒为 Running）。
        let shown = phase == Phase::Running;
        if active {
            if phase.live() {
                RowAction::Keep
            } else {
                RowAction::Mark
            }
        } else if shown {
            RowAction::Restore
        } else {
            RowAction::Keep
        }
    }
}
