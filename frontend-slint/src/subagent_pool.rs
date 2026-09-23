//! Bounded parent-owned directory. Opening a child never changes the active conversation.
use crate::{Conversation, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
struct Source(std::sync::Arc<wunder_desktop::NativeDesktop>);
impl Source {
    fn list(&self, session: &str) -> Result<serde_json::Value, String> {
        self.0.list_subagents(session)
    }
    fn history(&self, session: &str) -> Result<String, String> {
        self.0
            .get_session(session)
            .map(|(_, messages)| {
                messages
                    .into_iter()
                    .map(|message| message.text)
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
            .map_err(|_| "无法读取子智能体历史".into())
    }
}
pub fn install_native(app: &MainWindow, desktop: std::sync::Arc<wunder_desktop::NativeDesktop>) {
    install_source(app, Source(desktop));
}

fn install_source(app: &MainWindow, api: Source) {
    let state = Rc::new(RefCell::new((String::new(), 0_u64, false, false)));
    let weak = app.as_weak();
    let directory_api = api.clone();
    app.on_refresh_subagents(move || {
        let Some(app) = weak.upgrade() else { return };
        let parent = app.get_active_session_id().to_string();
        let mut current = state.borrow_mut();
        if current.2 {
            current.3 = true;
            if current.0 != parent {
                app.set_subagents(ModelRc::default());
            }
            return;
        }
        if current.0 != parent {
            app.set_subagents(ModelRc::new(VecModel::<Conversation>::default()));
        }
        current.0 = parent.clone();
        current.1 = current.1.wrapping_add(1);
        if parent.is_empty() {
            app.set_subagents_status("暂无子智能体".into());
            return;
        }
        current.2 = true;
        let generation = current.1;
        app.set_subagents_status("正在同步…".into());
        let state = state.clone();
        let weak = app.as_weak();
        let api = directory_api.clone();
        // Rc stays on the event-loop thread; only the weak handle crosses threads.
        let apply = move |app: MainWindow, result: Result<serde_json::Value, String>| {
            let mut current = state.borrow_mut();
            current.2 = false;
            if current.3 || app.get_active_session_id() != parent {
                current.3 = false;
                let weak = app.as_weak();
                slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                    if let Some(app) = weak.upgrade() {
                        app.invoke_refresh_subagents();
                    }
                });
            }
            if current.1 != generation || app.get_active_session_id() != parent {
                return;
            }
            match result {
                Ok(payload) => {
                    let active = payload
                        .pointer("/data/items")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|items| {
                            items.iter().any(|item| {
                                matches!(
                                    item["status"].as_str(),
                                    Some("running" | "queued" | "waiting" | "cancelling")
                                )
                            })
                        });
                    let rows: Vec<_> = payload
                        .pointer("/data/items")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .take(200)
                        .map(|item| Conversation {
                            id: item["session_id"].as_str().unwrap_or_default().into(),
                            title: item["label"]
                                .as_str()
                                .or_else(|| item["title"].as_str())
                                .unwrap_or("子智能体")
                                .into(),
                            preview: status_label(item["status"].as_str().unwrap_or_default())
                                .into(),
                            time: "".into(),
                        })
                        .collect();
                    app.set_subagents_status(
                        if rows.is_empty() {
                            "暂无子智能体"
                        } else {
                            "保留历史，可由主智能体继续分派"
                        }
                        .into(),
                    );
                    app.set_subagents(ModelRc::new(VecModel::from(rows)));
                    if active {
                        // Background children can finish after the foreground stream closes.
                        let weak = app.as_weak();
                        let state = state.clone();
                        slint::Timer::single_shot(std::time::Duration::from_secs(2), move || {
                            if state.borrow().1 != generation {
                                return;
                            }
                            if let Some(app) = weak
                                .upgrade()
                                .filter(|app| app.get_active_session_id() == parent)
                            {
                                app.invoke_refresh_subagents();
                            }
                        });
                    }
                }
                Err(error) => app.set_subagents_status(format!("同步失败：{error}").into()),
            }
        };
        // A Slint timer owns the UI-only callback and polls one bounded worker result.
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let session = app.get_active_session_id().to_string();
        std::thread::spawn(move || {
            let result = api.list(&session);
            let _ = tx.send(result);
        });
        poll_result(weak, rx, apply);
    });
    let weak = app.as_weak();
    app.on_inspect_subagent(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(child) = app.get_subagents().row_data(index.max(0) as usize) else {
            return;
        };
        let api = api.clone();
        let weak = app.as_weak();
        let parent = app.get_active_session_id().to_string();
        std::thread::spawn(move || {
            let result = api.history(&child.id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if app.get_active_session_id() != parent {
                    return;
                }
                app.set_dialog_title(child.title);
                app.set_dialog_text(
                    match result {
                        Ok(text) => text,
                        Err(error) => format!("读取失败：{error}"),
                    }
                    .into(),
                );
                app.set_dialog_open(true);
            });
        });
    });
}

fn poll_result(
    weak: slint::Weak<MainWindow>,
    rx: std::sync::mpsc::Receiver<Result<serde_json::Value, String>>,
    apply: impl FnOnce(MainWindow, Result<serde_json::Value, String>) + 'static,
) {
    slint::Timer::single_shot(std::time::Duration::from_millis(100), move || {
        let Some(app) = weak.upgrade() else { return };
        match rx.try_recv() {
            Ok(result) => apply(app, result),
            Err(std::sync::mpsc::TryRecvError::Empty) => poll_result(weak, rx, apply),
            Err(_) => apply(app, Err("请求已结束".into())),
        }
    });
}

fn status_label(status: &str) -> &'static str {
    match status {
        "cancelled" | "canceled" | "cancelling" => "已中断 · 可复用",
        "success" | "completed" | "finished" => "已完成 · 可复用",
        "running" | "queued" | "waiting" => "执行中",
        "error" | "failed" | "timeout" => "执行失败 · 可复用",
        _ => "待命",
    }
}
