//! A bounded channel feeds a 30 Hz UI timer; only the active block model mutates.
use crate::{
    chat_api::ChatApi,
    chat_stream::{self, Lifetime, Update},
    message_blocks::Blocks,
    ChatMessage, MainWindow,
};
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, Receiver},
        Arc,
    },
    time::{Duration, Instant},
};

pub struct StreamUi {
    timer: Timer,
    current: Option<Active>,
    api: ChatApi,
}
struct Active {
    session: String,
    lifetime: Lifetime,
    rx: Receiver<Update>,
    blocks: Blocks,
    row: usize,
    model: Rc<VecModel<ChatMessage>>,
    round: i64,
    settled: bool,
    state: String,
}
pub type Shared = Rc<RefCell<StreamUi>>;

pub fn install(app: &MainWindow, api: ChatApi) -> Shared {
    let shared = Rc::new(RefCell::new(StreamUi {
        timer: Timer::default(),
        current: None,
        api,
    }));
    let weak = app.as_weak();
    let state = shared.clone();
    app.on_stop_generation(move || {
        let Some(app) = weak.upgrade() else { return };
        if !app.get_busy() || app.get_stopping() {
            return;
        }
        app.set_stopping(true);
        app.set_status("正在停止…".into());
        let api = state.borrow().api.clone();
        let session = app.get_active_session_id().to_string();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.cancel(&session);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_stopping(false);
                if let Err(error) = result {
                    app.set_status(format!("停止失败：{error}").into());
                }
            });
        });
    });
    let weak = app.as_weak();
    let state = shared.clone();
    app.on_copy_message(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if index < 0 {
            return;
        }
        let state = state.borrow();
        let active = state.current.as_ref().filter(|active| {
            app.get_busy()
                && active.row == index as usize
                && app.get_active_session_id() == active.session
        });
        let raw = active.map(|active| active.blocks.raw.clone()).or_else(|| {
            app.get_messages()
                .row_data(index.max(0) as usize)
                .map(|row| row.text.to_string())
        });
        if let Some(raw) = raw {
            app.invoke_copy_raw(raw.into());
            app.set_status("已复制原始内容".into());
        }
    });
    shared
}

pub fn start(shared: &Shared, app: &MainWindow, content: Option<String>) {
    shared.borrow().timer.stop();
    let session = app.get_active_session_id().to_string();
    let mut rows: Vec<_> = app.get_messages().iter().collect();
    if let Some(content) = &content {
        rows.push(ChatMessage {
            text: content.into(),
            mine: true,
            time: "刚刚".into(),
            blocks: crate::message_blocks::from_text(content),
            ..Default::default()
        });
    }
    if rows.len() >= 100 {
        rows.drain(..rows.len() - 99);
    }
    let blocks = Blocks::new();
    let row = rows.len();
    rows.push(ChatMessage {
        state: "正在生成…".into(),
        time: "刚刚".into(),
        blocks: ModelRc::from(blocks.model.clone()),
        ..Default::default()
    });
    let model = Rc::new(VecModel::from(rows));
    app.set_messages(ModelRc::from(model.clone()));
    app.set_busy(true);
    app.set_stopping(false);
    app.set_stream_updates(0);
    app.set_stream_bytes(0);
    app.set_stream_max_ui_ms(0.0);
    app.set_follow_output(true);
    app.set_status("正在连接…".into());
    app.set_draft("".into());
    let (tx, rx) = sync_channel(128);
    let closed = Arc::new(AtomicBool::new(false));
    let api = shared.borrow().api.clone();
    shared.borrow_mut().current = Some(Active {
        session: session.clone(),
        lifetime: Lifetime(closed.clone()),
        rx,
        blocks,
        row,
        model,
        round: 0,
        settled: false,
        state: "正在生成…".into(),
    });
    std::thread::spawn(move || chat_stream::run(api, session, content, tx, closed));
    let weak = app.as_weak();
    let state = Rc::downgrade(shared);
    shared
        .borrow()
        .timer
        .start(TimerMode::Repeated, Duration::from_millis(33), move || {
            let (Some(app), Some(state)) = (weak.upgrade(), state.upgrade()) else {
                return;
            };
            let mut state = state.borrow_mut();
            let started = Instant::now();
            let Some(active) = state.current.as_mut() else {
                return;
            };
            let mut dirty = false;
            let mut done = false;
            // Time-budget draining prevents a replay burst from monopolizing the UI.
            for _ in 0..128 {
                let Ok(update) = active.rx.try_recv() else {
                    break;
                };
                match update {
                    Update::Event(event) => {
                        if event["event"].as_str().is_some_and(|kind| kind.starts_with("subagent_")) {
                            app.invoke_refresh_subagents();
                        }
                        match active.apply(&event) {
                            Ok(changed) => dirty |= changed,
                            Err(error) => {
                                active.state = error;
                                done = true;
                            }
                        }
                        if crate::stream_events::is_terminal(&event) {
                            done = true;
                        }
                    }
                    Update::Status(status) => app.set_status(status.into()),
                    Update::Finished => {
                        done = true;
                    }
                    Update::Failed(error) => {
                        active.state = error;
                        done = true;
                    }
                }
                if started.elapsed() > Duration::from_millis(5) || done {
                    break;
                }
            }
            if dirty || done {
                active.blocks.flush();
                app.set_stream_bytes(active.blocks.raw.len().min(i32::MAX as usize) as i32);
                app.set_stream_updates(app.get_stream_updates() + 1);
                if dirty {
                    app.set_status(active.state.as_str().into());
                }
                if app.get_follow_output() {
                    app.set_scroll_revision(app.get_scroll_revision().wrapping_add(1));
                }
            }
            if done {
                active.lifetime.0.store(true, Ordering::Relaxed);
                if !active.settled && active.state == "正在生成…" {
                    active.state = "输出已结束".into();
                }
                let mut message = active.model.row_data(active.row).unwrap_or_default();
                message.text = active.blocks.raw.as_str().into();
                message.state = active.state.as_str().into();
                active.model.set_row_data(active.row, message);
                app.set_busy(false);
                app.set_stopping(false);
                app.set_status(active.state.as_str().into());
                state.timer.stop();
                // Settled text is on the row; release the old model and channel.
                state.current = None;
                app.invoke_refresh_files();
                app.invoke_refresh_subagents();
            }
            app.set_stream_max_ui_ms(
                app.get_stream_max_ui_ms()
                    .max(started.elapsed().as_secs_f32() * 1000.0),
            );
        });
}

impl Active {
    fn apply(&mut self, event: &Value) -> Result<bool, String> {
        // Live delivery is flat, while replay records from older bridge versions
        // may retain the persisted `{ data: ... }` envelope. Accept both shapes
        // so a reconnect can never turn a completed answer into an empty bubble.
        let envelope_data = &event["data"];
        let data = envelope_data.get("data").unwrap_or(envelope_data);
        let kind = event["event"].as_str().unwrap_or("");
        let round = data["model_round"].as_i64().unwrap_or(self.round);
        if round > self.round
            && self.round > 0
            && matches!(kind, "llm_output_delta" | "llm_request")
        {
            self.blocks.flush();
            let mut previous = self.model.row_data(self.row).unwrap_or_default();
            previous.text = self.blocks.raw.as_str().into();
            previous.state = "步骤完成".into();
            self.model.set_row_data(self.row, previous);
            self.blocks = Blocks::new();
            if self.model.row_count() >= 100 {
                self.model.remove(0);
            }
            self.row = self.model.row_count();
            self.model.push(ChatMessage {
                time: "刚刚".into(),
                blocks: ModelRc::from(self.blocks.model.clone()),
                ..Default::default()
            });
        }
        self.round = round;
        match kind {
            "llm_output_delta" | "delta" => {
                if let Some(delta) = data["delta"].as_str() {
                    self.blocks.append(delta)?;
                    self.state = "正在生成…".into();
                    return Ok(true);
                }
                self.state = "正在思考…".into();
            }
            "llm_output" | "final" => {
                if let Some(text) = data["answer"].as_str().or_else(|| data["content"].as_str()) {
                    self.blocks.replace(text)?;
                }
                if kind == "final" {
                    self.state = "任务完成".into();
                    self.settled = true;
                }
                return Ok(true);
            }
            "turn_terminal" => {
                self.state = match data["status"].as_str() {
                    Some("cancelled" | "canceled") => "已停止",
                    Some("failed" | "error" | "rejected") => "执行失败",
                    _ => "任务完成",
                }
                .into();
                self.settled = true;
            }
            "error" | "queue_fail" => {
                self.state = data["message"].as_str().unwrap_or("执行失败").into();
                self.settled = true;
            }
            "tool_call" | "tool_start" => {
                self.state = format!(
                    "正在执行工具：{}",
                    data["tool"]
                        .as_str()
                        .or_else(|| data["name"].as_str())
                        .unwrap_or("")
                );
            }
            "approval_request" => {
                self.state = "等待工具审批".into();
            }
            "queued" => {
                self.state = "任务已排队…".into();
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
