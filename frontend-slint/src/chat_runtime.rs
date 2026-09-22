//! Slint presentation bindings for the established `/wunder/chat` API.
use crate::chat_api::{
    AgentRecord, ChatApi, ChatSession, ConnectionConfig, DesktopSettings, ToolRecord,
    TranscriptMessage,
};
use crate::{AgentCard, ChatMessage, Conversation, MainWindow, ModelCard, ToolCard};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

type Drafts = Rc<RefCell<HashMap<String, String>>>;

const SESSION_LIMIT: usize = 100;
const MESSAGE_LIMIT: usize = 100;
const INPUT_LIMIT_BYTES: usize = 16_384;
const STATUS_ERROR_PREFIX: &str = "错误：";

pub fn install(app: &MainWindow, connection: ConnectionConfig) {
    let api = match ChatApi::new(connection) {
        Ok(api) => api,
        Err(error) => {
            app.set_status(format!("无法初始化聊天连接：{error}").into());
            return;
        }
    };
    app.set_conversations(empty_model());
    app.set_messages(empty_model());
    app.set_agents(empty_model());
    app.set_tools(empty_model());
    app.set_models(empty_model());
    app.set_connected(true);
    app.set_status("正在连接本地运行时…".into());
    crate::entity_state::bind_selection(app);
    bind_refresh(app, api.clone());
    let drafts = Rc::new(RefCell::new(HashMap::new()));
    bind_session_selection(app, api.clone(), drafts.clone());
    bind_new_thread(app, api.clone(), drafts);
    bind_send(app, api.clone());
    bind_agents(app, api.clone());
    bind_tools(app, api.clone());
    bind_settings(app, api);
    app.invoke_refresh_chat();
    app.invoke_refresh_agents();
}

fn bind_refresh(app: &MainWindow, api: ChatApi) {
    let weak = app.as_weak();
    app.on_refresh_chat(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_chat_loading()
            || app.get_busy()
            || app.get_creating_session()
            || app.get_session_loading()
        {
            return;
        }
        app.set_chat_loading(true);
        app.set_status("正在同步…".into());
        let weak = weak.clone();
        let api = api.clone();
        run_background(move || {
            let sessions = api.list_sessions();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_chat_loading(false);
                match sessions {
                    Ok(sessions) => {
                        apply_sessions(&app, sessions);
                        if app.get_active_session_id().is_empty()
                            && app.get_conversations().row_count() > 0
                        {
                            app.invoke_select_conversation(0);
                        } else if app.get_active_session_id().is_empty() {
                            app.set_messages(empty_model());
                            app.set_heading("新会话".into());
                            app.set_status("没有可用会话，点击新建线程开始聊天".into());
                        } else {
                            app.invoke_select_conversation(app.get_selected_conversation());
                        }
                    }
                    Err(error) => show_error(&app, format!("无法同步聊天列表：{error}")),
                }
            });
        });
    });
}

fn bind_session_selection(app: &MainWindow, api: ChatApi, drafts: Drafts) {
    let weak = app.as_weak();
    app.on_select_conversation(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if index < 0
            || app.get_busy()
            || app.get_session_loading()
            || app.get_creating_session()
            || app.get_chat_loading()
        {
            return;
        }
        let Some(conversation) = app.get_conversations().row_data(index.max(0) as usize) else {
            return;
        };
        let session_id = conversation.id.to_string();
        if session_id.is_empty() {
            return;
        }
        let previous = app.get_active_session_id().to_string();
        if previous != session_id {
            save_draft(&app, &drafts);
            app.set_draft(
                drafts
                    .borrow()
                    .get(&session_id)
                    .cloned()
                    .unwrap_or_default()
                    .into(),
            );
        }
        app.set_session_loading(true);
        app.set_selected_conversation(index.max(0));
        app.set_active_session_id(session_id.as_str().into());
        app.set_heading(conversation.title);
        app.set_messages(empty_model());
        app.set_status("正在加载聊天记录…".into());
        let weak = app.as_weak();
        let api = api.clone();
        run_background(move || {
            let loaded = api.get_session(&session_id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_session_loading(false);
                if app.get_active_session_id() != session_id {
                    return;
                }
                match loaded {
                    Ok((session, transcript)) => {
                        apply_active_session(&app, &session, transcript);
                    }
                    Err(error) => show_error(&app, format!("无法加载会话：{error}")),
                }
            });
        });
    });
}

fn bind_new_thread(app: &MainWindow, api: ChatApi, drafts: Drafts) {
    let weak = app.as_weak();
    app.on_new_thread(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_creating_session()
            || app.get_busy()
            || app.get_session_loading()
            || app.get_chat_loading()
            || app.get_agents_loading()
        {
            return;
        }
        save_draft(&app, &drafts);
        app.set_creating_session(true);
        let agent_id = app
            .get_agents()
            .row_data(app.get_selected_agent() as usize)
            .map(|agent| agent.id.to_string())
            .filter(|id| id != "__default__");
        let weak = weak.clone();
        let api = api.clone();
        run_background(move || {
            let created = api.create_session_for_agent(agent_id.as_deref());
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_creating_session(false);
                match created {
                    Ok(session) => {
                        prepend_session(&app, &session);
                        app.set_selected_conversation(0);
                        app.set_draft("".into());
                        apply_active_session(&app, &session, Vec::new());
                        app.set_status("新会话已创建".into());
                    }
                    Err(error) => show_error(&app, format!("无法创建会话：{error}")),
                }
            });
        });
    });
}

fn bind_agents(app: &MainWindow, api: ChatApi) {
    let weak = app.as_weak();
    let refresh_api = api.clone();
    app.on_refresh_agents(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_agents_loading() || app.get_saving() {
            return;
        }
        app.set_agents_loading(true);
        app.set_status("正在同步…".into());
        let weak = weak.clone();
        let api = refresh_api.clone();
        run_background(move || {
            let result = api.list_agents();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_agents_loading(false);
                match result {
                    Ok(agents) => {
                        apply_agents(&app, agents);
                        if app.get_agents().row_count() > 0 && app.get_selected_agent() < 0 {
                            app.invoke_select_agent(0);
                        }
                        app.set_status("智能体列表已同步".into());
                    }
                    Err(error) => show_error(&app, format!("无法同步智能体：{error}")),
                }
            });
        });
    });
    let weak = app.as_weak();
    app.on_create_agent(move |requested_name| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() || app.get_agents_loading() {
            return;
        }
        let name = requested_name.trim().to_string();
        if name.is_empty() || name.chars().count() > 80 {
            if let Some(app) = weak.upgrade() {
                app.set_status("智能体名称不能为空且不能超过 80 个字符".into());
            }
            return;
        }
        app.set_saving(true);
        let weak = weak.clone();
        let api = api.clone();
        run_background(move || {
            let result = api.create_agent(&name);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(agent) => {
                        app.set_agent_creator_open(false);
                        let mut rows = app.get_agents().iter().collect::<Vec<_>>();
                        rows.insert(0, to_agent_card(agent));
                        rows.truncate(100);
                        app.set_agents(model_from(rows));
                        app.invoke_select_agent(0);
                        app.set_status("新智能体已创建".into());
                    }
                    Err(error) => show_error(&app, format!("无法创建智能体：{error}")),
                }
            });
        });
    });
}

fn bind_tools(app: &MainWindow, api: ChatApi) {
    let weak = app.as_weak();
    app.on_refresh_tools(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_tools_loading() {
            return;
        }
        app.set_tools_loading(true);
        app.set_status("正在同步…".into());
        let weak = weak.clone();
        let api = api.clone();
        run_background(move || {
            let result = api.list_tools();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_tools_loading(false);
                match result {
                    Ok(tools) => {
                        app.set_tools(model_from(tools.into_iter().map(to_tool_card).collect()));
                        app.set_status("工具目录已同步".into());
                    }
                    Err(error) => show_error(&app, format!("无法同步工具目录：{error}")),
                }
            });
        });
    });
}

fn bind_settings(app: &MainWindow, api: ChatApi) {
    let weak = app.as_weak();
    let refresh_api = api.clone();
    app.on_refresh_settings(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_settings_loading() || app.get_saving() {
            return;
        }
        app.set_settings_loading(true);
        app.set_status("正在同步…".into());
        let weak = weak.clone();
        let api = refresh_api.clone();
        run_background(move || {
            let result = api.get_desktop_settings();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_settings_loading(false);
                match result {
                    Ok(settings) => {
                        apply_settings(&app, settings);
                        app.set_status("本地设置已同步".into());
                    }
                    Err(error) => show_error(&app, format!("无法读取本地设置：{error}")),
                }
            });
        });
    });
    let weak = app.as_weak();
    let save_api = api.clone();
    app.on_save_model(
        move |key, provider, model, base_url, access_key, model_type| {
            let Some(app) = weak.upgrade() else { return };
            if app.get_saving() || app.get_settings_loading() || app.get_agents_loading() {
                return;
            }
            app.set_saving(true);
            let weak = weak.clone();
            let api = save_api.clone();
            run_background(move || {
                let result =
                    api.save_model(&key, &provider, &model, &base_url, &access_key, &model_type);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.set_saving(false);
                    match result {
                        Ok(settings) => {
                            app.set_model_editor_open(false);
                            app.set_model_token_draft("".into());
                            apply_settings(&app, settings);
                            select_model_key(&app, &key);
                            app.set_status("模型配置已保存到本地运行时".into());
                        }
                        Err(error) => show_error(&app, format!("无法保存模型配置：{error}")),
                    }
                });
            });
        },
    );
    let weak = app.as_weak();
    app.on_set_default_model(move |key| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() || app.get_agents_loading() {
            return;
        }
        app.set_saving(true);
        let weak = weak.clone();
        let api = api.clone();
        run_background(move || {
            let result = api.set_default_model(&key);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(settings) => {
                        apply_settings(&app, settings);
                        select_model_key(&app, &key);
                        app.set_status("默认模型已更新".into());
                    }
                    Err(error) => show_error(&app, format!("无法更新默认模型：{error}")),
                }
            });
        });
    });
}

fn bind_send(app: &MainWindow, api: ChatApi) {
    let weak = app.as_weak();
    app.on_send_message(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_busy()
            || app.get_session_loading()
            || app.get_creating_session()
            || app.get_chat_loading()
        {
            return;
        }
        let session_id = app.get_active_session_id().to_string();
        let content = app.get_draft().trim().to_string();
        if content.is_empty() {
            return;
        }
        if content.len() > INPUT_LIMIT_BYTES {
            app.set_status("消息过长，请缩短后重试".into());
            return;
        }
        if session_id.is_empty() {
            app.set_status("请先新建或选择一个会话".into());
            return;
        }
        app.set_busy(true);
        app.set_draft("".into());
        app.set_status("正在发送到本地运行时…".into());
        append_message(
            &app,
            ChatMessage {
                text: content.clone().into(),
                mine: true,
                time: "刚刚".into(),
                workflow: false,
                state: "".into(),
            },
        );
        scroll_to_end(&app);
        let weak = app.as_weak();
        let api = api.clone();
        run_background(move || {
            let sent = api.send_message(&session_id, &content);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_busy(false);
                if app.get_active_session_id() != session_id {
                    return;
                }
                match sent {
                    Ok(answer) => {
                        append_message(
                            &app,
                            ChatMessage {
                                text: answer.into(),
                                mine: false,
                                time: "刚刚".into(),
                                workflow: false,
                                state: "任务完成".into(),
                            },
                        );
                        app.set_status("已完成".into());
                    }
                    Err(error) => {
                        append_message(
                            &app,
                            ChatMessage {
                                text: "未能确认本轮结果，请刷新会话查看最新记录。".into(),
                                mine: false,
                                time: "刚刚".into(),
                                workflow: false,
                                state: format!("失败：{error}").into(),
                            },
                        );
                        show_error(
                            &app,
                            format!("未能确认回复，请刷新会话后再决定是否重发：{error}"),
                        );
                    }
                }
                scroll_to_end(&app);
            });
        });
    });
}

fn save_draft(app: &MainWindow, drafts: &Drafts) {
    let id = app.get_active_session_id().to_string();
    if id.is_empty() {
        return;
    }
    let mut drafts = drafts.borrow_mut();
    if !drafts.contains_key(&id) && drafts.len() >= SESSION_LIMIT {
        let retained = app
            .get_conversations()
            .iter()
            .map(|row| row.id.to_string())
            .collect::<std::collections::HashSet<_>>();
        drafts.retain(|key, _| retained.contains(key));
        if drafts.len() >= SESSION_LIMIT {
            if let Some(key) = drafts.keys().next().cloned() {
                drafts.remove(&key);
            }
        }
    }
    drafts.insert(id, app.get_draft().to_string());
}

fn apply_sessions(app: &MainWindow, sessions: Vec<ChatSession>) {
    let active_id = app.get_active_session_id().to_string();
    let rows = sessions
        .iter()
        .take(SESSION_LIMIT)
        .map(to_conversation)
        .collect::<Vec<_>>();
    app.set_conversations(model_from(rows));
    if let Some(index) = sessions
        .iter()
        .take(SESSION_LIMIT)
        .position(|session| session.id == active_id)
    {
        app.set_selected_conversation(index as i32);
    } else {
        app.set_selected_conversation(-1);
        app.set_active_session_id("".into());
        app.set_active_agent_id("".into());
        app.set_messages(empty_model());
    }
}

fn apply_active_session(
    app: &MainWindow,
    session: &ChatSession,
    transcript: Vec<TranscriptMessage>,
) {
    app.set_active_session_id(session.id.as_str().into());
    app.set_active_agent_id(session.agent_id.as_str().into());
    app.set_heading(session.title.as_str().into());
    app.set_messages(model_from(
        transcript
            .into_iter()
            .take(MESSAGE_LIMIT)
            .map(to_chat_message)
            .collect::<Vec<_>>(),
    ));
    app.set_status("聊天记录已加载".into());
    scroll_to_end(app);
}

fn prepend_session(app: &MainWindow, session: &ChatSession) {
    let mut rows = app.get_conversations().iter().collect::<Vec<_>>();
    rows.insert(0, to_conversation(session));
    rows.truncate(SESSION_LIMIT);
    app.set_conversations(model_from(rows));
}

fn to_conversation(session: &ChatSession) -> Conversation {
    Conversation {
        id: session.id.as_str().into(),
        title: session.title.as_str().into(),
        preview: "本地聊天会话".into(),
        time: session.updated_at.as_str().into(),
    }
}

fn to_chat_message(message: TranscriptMessage) -> ChatMessage {
    ChatMessage {
        text: message.text.into(),
        mine: message.mine,
        time: message.time.into(),
        workflow: false,
        state: message.state.into(),
    }
}

fn apply_agents(app: &MainWindow, agents: Vec<AgentRecord>) {
    let selected = usize::try_from(app.get_selected_agent())
        .ok()
        .and_then(|index| app.get_agents().row_data(index))
        .map(|agent| agent.id)
        .unwrap_or_default();
    app.set_agents(model_from(agents.into_iter().map(to_agent_card).collect()));
    crate::entity_state::restore_agent(app, &selected);
}

fn to_agent_card(agent: AgentRecord) -> AgentCard {
    AgentCard {
        id: agent.id.into(),
        name: agent.name.into(),
        description: agent.description.into(),
        model: agent.model.into(),
        status: agent.status.into(),
    }
}

fn to_tool_card(tool: ToolRecord) -> ToolCard {
    ToolCard {
        name: tool.name.into(),
        description: tool.description.into(),
        category: tool.category.into(),
    }
}

fn apply_settings(app: &MainWindow, settings: DesktopSettings) {
    let selected = app.get_selected_model_key();
    app.set_workspace_root(settings.workspace_root.into());
    app.set_runtime_language(settings.language.into());
    app.set_models(model_from(
        settings
            .models
            .into_iter()
            .map(|model| ModelCard {
                key: model.key.into(),
                provider: model.provider.into(),
                model: model.model.into(),
                base_url: model.base_url.into(),
                model_type: model.model_type.into(),
                is_default: model.is_default,
            })
            .collect(),
    ));
    crate::entity_state::restore_model(app, &selected);
}

fn select_model_key(app: &MainWindow, key: &str) {
    if let Some(index) = app.get_models().iter().position(|model| model.key == key) {
        app.invoke_select_model(index as i32);
    }
}

fn append_message(app: &MainWindow, message: ChatMessage) {
    let mut rows = app.get_messages().iter().collect::<Vec<_>>();
    while rows.len() >= MESSAGE_LIMIT {
        rows.remove(0);
    }
    rows.push(message);
    app.set_messages(model_from(rows));
}

fn model_from<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

fn empty_model<T: Clone + 'static>() -> ModelRc<T> {
    model_from(Vec::new())
}

fn scroll_to_end(app: &MainWindow) {
    let weak = app.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(16), move || {
        if let Some(app) = weak.upgrade() {
            app.set_scroll_revision(app.get_scroll_revision().wrapping_add(1));
        }
    });
}

fn run_background(task: impl FnOnce() + Send + 'static) {
    std::thread::spawn(task);
}

fn show_error(app: &MainWindow, error: String) {
    app.set_status(format!("{STATUS_ERROR_PREFIX}{error}").into());
    app.set_dialog_title("操作失败".into());
    app.set_dialog_text(error.into());
    app.set_dialog_open(true);
}
