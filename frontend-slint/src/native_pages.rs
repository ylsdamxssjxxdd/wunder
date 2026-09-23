//! Native entity-page callbacks. All storage and configuration work runs off the UI thread.
use crate::{AgentCard, MainWindow, ModelCard, ToolCard};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{rc::Rc, sync::Arc};
use wunder_desktop::{AgentRecord, DesktopSettings, LanPeerRecord, ModelEdit, NativeDesktop, ToolRecord, NativeProfile};

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    crate::entity_state::bind_selection(app);
    bind_agents(app, api.clone());
    bind_tools(app, api.clone());
    bind_settings(app, api.clone());
    bind_profile(app, api.clone());
    crate::workspace_ui::install(app, api.clone());
    crate::runtime_settings::install(app, api);
    app.invoke_refresh_agents();
    app.invoke_refresh_settings();
    app.invoke_refresh_files();
}

fn bind_profile(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let refresh_api = api.clone();
    app.on_refresh_profile(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_profile_loading() { return; }
        app.set_profile_loading(true);
        let weak = weak.clone();
        let api = refresh_api.clone();
        run_background(move || {
            let result = api.get_profile();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_profile_loading(false);
                match result {
                    Ok(profile) => apply_profile(&app, profile),
                    Err(error) => show_error(&app, format!("无法读取个人概况：{error}")),
                }
            });
        });
    });
    let weak = app.as_weak();
    let avatar_api = api.clone();
    app.on_save_profile_avatar(move |icon, color| {
        let Some(_app) = weak.upgrade() else { return };
        let weak = weak.clone();
        let api = avatar_api.clone();
        run_background(move || {
            let result = api.save_profile_avatar(&icon, &color);
            let _ = weak.upgrade_in_event_loop(move |app| match result { Ok(profile) => apply_profile(&app, profile), Err(error) => show_error(&app, format!("无法保存头像：{error}")) });
        });
    });
}

fn apply_profile(app: &MainWindow, profile: NativeProfile) {
    app.set_profile(crate::ProfileCard {
        user_id: profile.user_id.into(), username: profile.username.into(), email: profile.email.into(), unit: profile.unit.into(),
        sessions: profile.sessions.to_string().into(), sessions_last_7d: profile.sessions_last_7d.to_string().into(), tool_calls: profile.tool_calls.to_string().into(),
        tokens: profile.consumed_tokens.to_string().into(), agents: profile.agents.to_string().into(), last_active: profile.last_active_at.into(), avatar_icon: profile.avatar_icon.clone().into(), avatar_glyph: profile_glyph(&profile.avatar_icon).into(), avatar_color: profile.avatar_color.into(),
    });
}

fn profile_glyph(icon: &str) -> &'static str {
    match icon.trim() { "initial" => "✦", "qq-avatar-0001" => "◉", "qq-avatar-0002" => "❖", _ => "✦" }
}

fn bind_agents(app: &MainWindow, api: Arc<NativeDesktop>) {
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
    let agent_create_api = api.clone();
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
        let api = agent_create_api.clone();
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
    let weak = app.as_weak();
    app.on_save_agent(move |name, description, system_prompt, model, icon_name, icon_color| {
        let Some(app) = weak.upgrade() else { return };
        let Some(agent) = usize::try_from(app.get_selected_agent())
            .ok()
            .and_then(|index| app.get_agents().row_data(index))
        else {
            return;
        };
        if app.get_saving() || app.get_agents_loading() || agent.id.is_empty() {
            return;
        }
        app.set_saving(true);
        let weak = weak.clone();
        let api = api.clone();
        let id = agent.id.to_string();
        run_background(move || {
            let result = api.update_agent(&id, &name, &description, &system_prompt, &model, &icon_name, &icon_color);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(updated) => {
                        let selected = app.get_selected_agent();
                        let rows = app.get_agents();
                        if let Some(index) = rows.iter().position(|row| row.id == id) {
                            rows.set_row_data(index, to_agent_card(updated));
                            if selected == index as i32 {
                                app.invoke_select_agent(selected);
                            }
                        }
                        app.set_status("智能体配置已保存".into());
                    }
                    Err(error) => show_error(&app, format!("无法保存智能体配置：{error}")),
                }
            });
        });
    });
}

fn bind_tools(app: &MainWindow, api: Arc<NativeDesktop>) {
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

fn bind_settings(app: &MainWindow, api: Arc<NativeDesktop>) {
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
                let result = api.save_model(ModelEdit {
                    key: &key,
                    provider: &provider,
                    model: &model,
                    base_url: &base_url,
                    api_key: &access_key,
                    model_type: &model_type,
                });
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
    let default_api = api.clone();
    app.on_set_default_model(move |key| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() || app.get_agents_loading() {
            return;
        }
        app.set_saving(true);
        let weak = weak.clone();
        let api = default_api.clone();
        run_background(move || {
            let result = api.set_default_model(&key);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(settings) => {
                        apply_settings(&app, settings);
                        select_model_key(&app, &key);
                        app.set_status("默认模型已更新".into());
                        app.invoke_refresh_agents();
                    }
                    Err(error) => show_error(&app, format!("无法更新默认模型：{error}")),
                }
            });
        });
    });
    let weak = app.as_weak();
    let lan_api = api.clone();
    app.on_save_lan(move |enabled, name| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() { return; }
        app.set_saving(true);
        let weak = weak.clone();
        let api = lan_api.clone();
        run_background(move || {
            let result = api.save_lan(enabled, &name);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result { Ok(settings) => { apply_settings(&app, settings); app.set_status("内网通信设置已保存".into()); }, Err(error) => show_error(&app, format!("无法保存内网通信设置：{error}")) }
            });
        });
    });
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
    let tone = avatar_tone(&agent.icon_color);
    AgentCard {
        id: agent.id.into(),
        name: agent.name.into(),
        description: agent.description.into(),
        model: agent.model.into(),
        system_prompt: agent.system_prompt.into(),
        status: agent.status.into(),
        icon_name: agent.icon_name.into(),
        icon_color: agent.icon_color.into(),
        icon_glyph: agent.icon_glyph.into(),
        icon_tone: tone,
    }
}

fn avatar_tone(color: &str) -> i32 {
    match color.trim().to_ascii_lowercase().as_str() {
        "#f97316" | "#ef4444" | "#ec4899" | "#8b5cf6" => 1,
        "#3b82f6" => 3,
        "#10b981" => 2,
        _ => 1,
    }
}

fn to_tool_card(tool: ToolRecord) -> ToolCard {
    ToolCard {
        name: tool.name.into(),
        description: tool.description.into(),
        category: tool.category.into(),
    }
}

pub(crate) fn apply_settings(app: &MainWindow, settings: DesktopSettings) {
    let selected = app.get_selected_model_key();
    app.set_workspace_root(settings.workspace_root.into());
    app.set_runtime_language(settings.language.into());
    app.set_lan_enabled(settings.lan.enabled);
    app.set_lan_name(settings.lan.display_name.into());
    app.set_lan_peer_id(settings.lan.peer_id.into());
    app.set_lan_endpoint(format!("{}:{}", settings.lan.listen_host, settings.lan.listen_port).into());
    app.set_lan_peer_count(settings.lan.peer_count as i32);
    app.set_lan_peers(model_from(settings.lan.peers.into_iter().map(to_lan_peer_card).collect()));
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

fn to_lan_peer_card(peer: LanPeerRecord) -> crate::LanPeerCard {
    crate::LanPeerCard { peer_id: peer.peer_id.into(), display_name: peer.display_name.into(), address: format!("{}:{}", peer.lan_ip, peer.listen_port).into() }
}

fn select_model_key(app: &MainWindow, key: &str) {
    if let Some(index) = app.get_models().iter().position(|model| model.key == key) {
        app.invoke_select_model(index as i32);
    }
}

fn model_from<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

fn run_background(task: impl FnOnce() + Send + 'static) {
    std::thread::spawn(task);
}

fn show_error(app: &MainWindow, error: String) {
    app.set_status(format!("错误：{error}").into());
    app.set_dialog_title("操作失败".into());
    app.set_dialog_text(error.into());
    app.set_dialog_open(true);
}
