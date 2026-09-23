//! Native chat projection for the in-process desktop runtime.

use crate::{message_blocks::Blocks, ChatMessage, Conversation, MainWindow};
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use wunder_desktop::{NativeChatEvent, NativeChatInput, NativeDesktop, NativeStream};

struct Active {
    stream: NativeStream,
    session: String,
    blocks: Blocks,
    row: usize,
    model: Rc<VecModel<ChatMessage>>,
    state: String,
    round: i64,
}

struct State {
    timer: Timer,
    desktop: Arc<NativeDesktop>,
    active: Option<Active>,
    history_generation: Arc<AtomicU64>,
    drafts: std::collections::HashMap<String, String>,
}

pub fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    crate::subagent_pool::install_native(app, desktop.clone());
    app.set_connected(true);
    app.set_status("正在加载内嵌运行时…".into());
    app.set_conversations(ModelRc::default());
    app.set_messages(ModelRc::default());
    app.set_agents(ModelRc::default());
    app.set_tools(ModelRc::default());
    app.set_models(ModelRc::default());
    app.set_files(ModelRc::default());
    let state = Rc::new(RefCell::new(State {
        timer: Timer::default(),
        desktop,
        active: None,
        history_generation: Arc::new(AtomicU64::new(0)),
        drafts: std::collections::HashMap::new(),
    }));
    bind_refresh(app, state.clone());
    bind_selection(app, state.clone());
    bind_new_thread(app, state.clone());
    bind_send(app, state.clone());
    bind_stop(app, state.clone());
    let weak = app.as_weak();
    app.on_select_task(move |index| {
        if let Some(app) = weak.upgrade() {
            app.invoke_select_conversation(index);
        }
    });
    let weak = app.as_weak();
    app.on_copy_message(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if index < 0 {
            return;
        }
        let state = state.borrow();
        let text = state
            .active
            .as_ref()
            .filter(|active| active.row == index as usize)
            .map(|active| active.blocks.raw.clone())
            .or_else(|| {
                app.get_messages()
                    .row_data(index as usize)
                    .map(|row| row.text.to_string())
            });
        if let Some(text) = text {
            app.invoke_copy_raw(text.into());
        }
    });
    app.invoke_refresh_chat();
}

fn bind_refresh(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_refresh_chat(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_chat_loading()
            || app.get_session_loading()
            || app.get_busy()
            || app.get_creating_session()
        {
            return;
        }
        let desktop = state.borrow().desktop.clone();
        let weak = app.as_weak();
        app.set_chat_loading(true);
        std::thread::spawn(move || {
            let result = desktop.list_sessions();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_chat_loading(false);
                match result {
                    Ok(items) => {
                        let rows = items
                            .into_iter()
                            .take(100)
                            .map(|item| Conversation {
                                id: item.id.into(),
                                title: item.title.into(),
                                time: format_time(item.updated_at).into(),
                                ..Default::default()
                            })
                            .collect::<Vec<_>>();
                        app.set_conversations(ModelRc::new(VecModel::from(rows)));
                        if app.get_active_session_id().is_empty()
                            && app.get_conversations().row_count() > 0
                        {
                            app.invoke_select_conversation(0);
                        }
                        app.set_status("内嵌运行时已就绪".into());
                    }
                    Err(error) => app.set_status(format!("无法读取会话：{error}").into()),
                }
            });
        });
    });
}

fn agent_avatar_glyph(app: &MainWindow) -> slint::SharedString {
    usize::try_from(app.get_selected_agent())
        .ok()
        .and_then(|index| app.get_agents().row_data(index))
        .map(|agent| agent.icon_glyph)
        .unwrap_or_else(|| "✦".into())
}

fn agent_avatar_tone(app: &MainWindow) -> i32 {
    usize::try_from(app.get_selected_agent())
        .ok()
        .and_then(|index| app.get_agents().row_data(index))
        .map(|agent| agent.icon_tone)
        .unwrap_or(1)
}

fn agent_avatar_glyph_from_row(row: Option<ChatMessage>) -> slint::SharedString {
    row.map(|message| message.avatar_glyph).unwrap_or_else(|| "✦".into())
}

fn agent_avatar_tone_from_row(row: Option<ChatMessage>) -> i32 {
    row.map(|message| message.avatar_tone).unwrap_or(1)
}

fn bind_selection(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_select_conversation(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if index < 0 || app.get_busy() {
            return;
        }
        let Some(row) = app.get_conversations().row_data(index as usize) else {
            return;
        };
        let id = row.id.to_string();
        {
            let mut current = state.borrow_mut();
            if current.drafts.len() >= 100 {
                current.drafts.retain(|key, _| {
                    app.get_conversations()
                        .iter()
                        .any(|row| row.id == key.as_str())
                });
            }
            let previous = app.get_active_session_id().to_string();
            if !previous.is_empty() {
                current.drafts.insert(previous, app.get_draft().to_string());
            }
            app.set_draft(current.drafts.get(&id).cloned().unwrap_or_default().into());
        }
        let desktop = state.borrow().desktop.clone();
        let generation = state.borrow().history_generation.clone();
        let request = generation.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        let weak = app.as_weak();
        app.set_selected_conversation(index);
        app.set_active_session_id(id.clone().into());
        app.invoke_refresh_subagents();
        app.set_heading(row.title);
        app.set_session_loading(true);
        app.set_messages(ModelRc::default());
        std::thread::spawn(move || {
            let result = desktop.get_session(&id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if app.get_active_session_id() != id
                    || generation.load(Ordering::Relaxed) != request
                {
                    return;
                }
                app.set_session_loading(false);
                match result {
                    Ok((session, messages)) => {
                        let agent = session.agent_id.unwrap_or_else(|| "__default__".into());
                        if app.get_active_agent_id() != agent {
                            app.set_active_agent_id(agent.clone().into());
                            app.invoke_navigate_directory("".into());
                        }
                        crate::entity_state::restore_agent(&app, &agent);
                        app.set_heading(session.title.into());
                        app.set_messages(ModelRc::new(VecModel::from(
                            messages
                                .into_iter()
                                .map(|message| ChatMessage {
                                    text: message.text.clone().into(),
                                    mine: message.mine,
                                    time: format_time(message.created_at).into(),
                                    state: message.state.into(),
                                    blocks: crate::message_blocks::from_text(&message.text),
                                    avatar_glyph: agent_avatar_glyph(&app),
                                    avatar_tone: agent_avatar_tone(&app),
                                    ..Default::default()
                                })
                                .collect::<Vec<_>>(),
                        )));
                        app.set_scroll_revision(app.get_scroll_revision().wrapping_add(1));
                        app.set_status("内嵌运行时已就绪".into());
                    }
                    Err(error) => app.set_status(format!("无法加载会话：{error}").into()),
                }
            });
        });
    });
}

fn bind_new_thread(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_new_thread(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_busy()
            || app.get_creating_session()
            || app.get_session_loading()
            || app.get_chat_loading()
        {
            return;
        }
        app.set_creating_session(true);
        let agent = usize::try_from(app.get_selected_agent())
            .ok()
            .and_then(|index| app.get_agents().row_data(index))
            .map(|row| row.id.to_string());
        let desktop = state.borrow().desktop.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = desktop.create_session_for_agent(agent.as_deref());
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_creating_session(false);
                match result {
                    Ok(session) => {
                        let row = Conversation {
                            id: session.id.clone().into(),
                            title: session.title.into(),
                            time: format_time(session.updated_at).into(),
                            ..Default::default()
                        };
                        let mut rows = app.get_conversations().iter().collect::<Vec<_>>();
                        rows.insert(0, row);
                        rows.truncate(100);
                        app.set_conversations(ModelRc::new(VecModel::from(rows)));
                        app.set_section(0);
                        app.invoke_select_conversation(0);
                    }
                    Err(error) => app.set_status(format!("无法新建会话：{error}").into()),
                }
            });
        });
    });
}

fn bind_send(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_send_message(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_busy()
            || app.get_session_loading()
            || app.get_chat_loading()
            || app.get_creating_session()
        {
            return;
        }
        let session = app.get_active_session_id().to_string();
        let content = app.get_draft().trim().to_string();
        if session.is_empty() || content.is_empty() {
            return;
        }
        if content.len() > 16_384 {
            app.set_status("输入过长，请分段发送".into());
            return;
        }
        let stream = match state.borrow().desktop.send_chat(NativeChatInput {
            session_id: session.clone(),
            content: content.clone(),
            client_message_id: None,
        }) {
            Ok(stream) => stream,
            Err(error) => {
                app.set_status(format!("无法发送：{error}").into());
                return;
            }
        };
        let mut rows = app.get_messages().iter().collect::<Vec<_>>();
        if rows.len() > 98 {
            rows.drain(..rows.len() - 98);
        }
        rows.push(ChatMessage {
            text: content.clone().into(),
            mine: true,
            time: "刚刚".into(),
            blocks: crate::message_blocks::from_text(&content),
            avatar_glyph: "".into(),
            avatar_tone: 0,
            ..Default::default()
        });
        let blocks = Blocks::new();
        let row = rows.len();
        rows.push(ChatMessage {
            time: "刚刚".into(),
            state: "正在生成…".into(),
            blocks: ModelRc::from(blocks.model.clone()),
            avatar_glyph: agent_avatar_glyph(&app),
            avatar_tone: agent_avatar_tone(&app),
            ..Default::default()
        });
        let model = Rc::new(VecModel::from(rows));
        app.set_messages(ModelRc::from(model.clone()));
        app.set_draft("".into());
        app.set_busy(true);
        app.set_follow_output(true);
        app.set_stopping(false);
        app.set_stream_bytes(0);
        app.set_stream_updates(0);
        app.set_stream_max_ui_ms(0.0);
        app.set_stream_max_backlog(0);
        state.borrow_mut().active = Some(Active {
            stream,
            session,
            blocks,
            row,
            model,
            state: "正在生成…".into(),
            round: 0,
        });
        start_timer(&app, state.clone());
    });
}

fn bind_stop(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    app.on_stop_generation(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_stopping() {
            return;
        }
        if let Some(active) = state.borrow().active.as_ref() {
            app.set_stopping(true);
            active.stream.cancel();
            app.set_status("正在停止…".into());
        }
    });
}

fn start_timer(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    // The timer belongs to State: a weak callback avoids retaining the runtime
    // after the native window closes during a stream.
    let callback_state = Rc::downgrade(&state);
    state
        .borrow()
        .timer
        .start(TimerMode::Repeated, Duration::from_millis(33), move || {
            let (Some(app), Some(state)) = (weak.upgrade(), callback_state.upgrade()) else {
                return;
            };
            let mut state = state.borrow_mut();
            let Some(active) = state.active.as_mut() else {
                return;
            };
            let started = Instant::now();
            let mut done = false;
            let mut dirty = false;
            app.set_stream_max_backlog(
                app.get_stream_max_backlog()
                    .max(active.stream.pending_events() as i32),
            );
            for _ in 0..128 {
                let event = match active.stream.try_recv() {
                    Ok(Some(event)) => event,
                    Ok(None) => break,
                    Err(error) => {
                        active.state = error.into();
                        done = true;
                        break;
                    }
                };
                match event {
                    NativeChatEvent::Event(event) => {
                        if event["event"]
                            .as_str()
                            .is_some_and(|kind| kind.starts_with("subagent_"))
                        {
                            app.invoke_refresh_subagents();
                        }
                        match apply_event(active, &event) {
                            Ok(changed) => dirty |= changed,
                            Err(error) => {
                                active.state = error;
                                done = true;
                            }
                        }
                    }
                    NativeChatEvent::Queued => {
                        active.state = "任务已排队…".into();
                        dirty = true;
                    }
                    NativeChatEvent::Failed(error) => {
                        active.state = error;
                        done = true;
                    }
                    NativeChatEvent::Finished => done = true,
                }
                if done || started.elapsed() > Duration::from_millis(5) {
                    break;
                }
            }
            if dirty || done {
                // Flush once per frame, never once per token.
                active.blocks.flush();
                app.set_stream_bytes(active.blocks.raw.len().min(i32::MAX as usize) as i32);
                app.set_stream_updates(app.get_stream_updates().wrapping_add(1));
                app.set_status(active.state.as_str().into());
                if app.get_follow_output() {
                    app.set_scroll_revision(app.get_scroll_revision().wrapping_add(1));
                }
            }
            if done {
                if active.state == "正在生成…" {
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
                state.active = None;
                app.invoke_refresh_subagents();
            }
            app.set_stream_max_ui_ms(
                app.get_stream_max_ui_ms()
                    .max(started.elapsed().as_secs_f32() * 1000.0),
            );
        });
}

fn apply_event(active: &mut Active, event: &Value) -> Result<bool, String> {
    let kind = event["event"].as_str().unwrap_or("");
    let envelope = &event["data"];
    let data = envelope.get("data").unwrap_or(envelope);
    if data["session_id"]
        .as_str()
        .is_some_and(|id| id != active.session)
    {
        return Ok(false);
    }
    let round = data["model_round"].as_i64().unwrap_or(active.round);
    if round > active.round
        && active.round > 0
        && matches!(kind, "llm_output_delta" | "llm_request")
    {
        active.blocks.flush();
        let mut previous = active.model.row_data(active.row).unwrap_or_default();
        previous.text = active.blocks.raw.as_str().into();
        previous.state = "步骤完成".into();
        active.model.set_row_data(active.row, previous);
        active.blocks = Blocks::new();
        if active.model.row_count() >= 100 {
            active.model.remove(0);
        }
        active.row = active.model.row_count();
        active.model.push(ChatMessage {
            time: "刚刚".into(),
            blocks: ModelRc::from(active.blocks.model.clone()),
            avatar_glyph: agent_avatar_glyph_from_row(active.model.row_data(active.row)),
            avatar_tone: agent_avatar_tone_from_row(active.model.row_data(active.row)),
            ..Default::default()
        });
    }
    active.round = round;
    match kind {
        "llm_output_delta" | "delta" => {
            if let Some(delta) = data["delta"].as_str() {
                active.blocks.append(delta)?;
                active.state = "正在生成…".into();
                return Ok(true);
            }
        }
        "llm_output" | "final" => {
            if let Some(text) = data["answer"].as_str().or_else(|| data["content"].as_str()) {
                active.blocks.replace(text)?;
            }
            if kind == "final" {
                active.state = "任务完成".into();
            }
            return Ok(true);
        }
        "turn_terminal" => {
            active.state = match data["status"].as_str() {
                Some("cancelled" | "canceled") => "已停止",
                Some("failed" | "error" | "rejected") => "执行失败",
                _ => "任务完成",
            }
            .into();
        }
        "queue_finish" => active.state = "任务完成".into(),
        "tool_call" | "tool_start" => active.state = "正在执行工具…".into(),
        "error" | "queue_fail" => {
            active.state = data["message"].as_str().unwrap_or("执行失败").into()
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn format_time(value: f64) -> String {
    if value > 0.0 {
        "刚刚".to_string()
    } else {
        String::new()
    }
}
