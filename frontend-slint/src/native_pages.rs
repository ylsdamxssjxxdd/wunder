//! Native entity-page callbacks. All storage and configuration work runs off the UI thread.
use crate::{AgentCard, MainWindow, ModelCard, ToolCard};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{rc::Rc, sync::Arc};
use wunder_desktop::{
    AgentRecord, AgentSettingsEdit, DesktopSettings, LanPeerRecord, ModelEdit, NativeDesktop,
    NativeProfile, ToolRecord,
};

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    crate::entity_state::bind_selection(app);
    bind_agents(app, api.clone());
    bind_agent_cards(app, api.clone());
    bind_model_probes(app, api.clone());
    bind_preferences_and_prompts(app, api.clone());
    bind_tools(app, api.clone());
    bind_settings(app, api.clone());
    bind_profile(app, api.clone());
    crate::workspace_ui::install(app, api.clone());
    crate::world_ui::install(app, api.clone());
    crate::cron_ui::install(app, api.clone());
    crate::channel_ui::install(app, api.clone());
    crate::plaza_ui::install(app, api.clone());
    crate::runtime_settings::install(app, api);
    app.invoke_refresh_agents();
    app.invoke_refresh_settings();
    app.invoke_refresh_files();
}

fn bind_model_probes(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let context_api = api.clone();
    app.on_probe_model_context(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_model_name_draft().trim().is_empty() {
            return;
        }
        let key = app.get_model_key_draft().trim().to_string();
        let provider = app.get_model_provider_draft().trim().to_string();
        let model = app.get_model_name_draft().trim().to_string();
        let base_url = app.get_model_base_url_draft().trim().to_string();
        let token = app.get_model_token_draft().trim().to_string();
        let known_key = !key.is_empty()
            && app
                .get_models()
                .iter()
                .any(|entry| entry.key == key);
        let token_override = (!token.is_empty()).then(|| token.clone());
        let weak = app.as_weak();
        let api = context_api.clone();
        app.set_status("正在探测上下文…".into());
        run_background(move || {
            let result = if known_key {
                api.probe_model_context_window(&key, token_override.as_deref())
            } else {
                api.probe_model_window(&provider, &model, &base_url, token_override.as_deref())
            };
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(outcome) => {
                    app.set_status(outcome.message.clone().into());
                    app.set_dialog_title("上下文探测".into());
                    app.set_dialog_text(outcome.message.into());
                    app.set_dialog_open(true);
                }
                Err(error) => show_error(&app, format!("无法探测上下文：{error}")),
            });
        });
    });
    let weak = app.as_weak();
    let voice_api = api.clone();
    app.on_probe_model_voices(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_model_name_draft().trim().is_empty() {
            return;
        }
        if app.get_model_type_draft().trim() != "tts" {
            show_error(&app, "只有语音合成模型支持语音列表探测".into());
            return;
        }
        let key = app.get_model_key_draft().trim().to_string();
        let provider = app.get_model_provider_draft().trim().to_string();
        let model = app.get_model_name_draft().trim().to_string();
        let base_url = app.get_model_base_url_draft().trim().to_string();
        let token = app.get_model_token_draft().trim().to_string();
        let known_key = !key.is_empty()
            && app
                .get_models()
                .iter()
                .any(|entry| entry.key == key);
        let token_override = (!token.is_empty()).then(|| token.clone());
        let weak = app.as_weak();
        let api = voice_api.clone();
        app.set_status("正在探测语音列表…".into());
        run_background(move || {
            let result = if known_key {
                api.probe_model_voices(&key, token_override.as_deref())
            } else {
                api.probe_model_voice_list(&provider, &model, &base_url, token_override.as_deref())
            };
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(voices) => {
                    if voices.is_empty() {
                        app.set_status("提供方未返回语音列表".into());
                        app.set_dialog_title("语音列表探测".into());
                        app.set_dialog_text("提供方未返回语音列表。".into());
                    } else {
                        let preview = voices.iter().take(12).cloned().collect::<Vec<_>>().join("、");
                        let more = if voices.len() > 12 {
                            format!(" 等 {} 项", voices.len())
                        } else {
                            String::new()
                        };
                        app.set_status(format!("语音列表探测成功：{} 项", voices.len()).into());
                        app.set_dialog_title("语音列表探测".into());
                        app.set_dialog_text(format!("{preview}{more}").into());
                    }
                    app.set_dialog_open(true);
                }
                Err(error) => show_error(&app, format!("无法探测语音列表：{error}")),
            });
        });
    });
}

const PROMPT_SEGMENT_KEYS: [&str; 6] = [
    "role",
    "engineering",
    "tools_protocol",
    "skills_protocol",
    "memory",
    "extra",
];

fn bind_preferences_and_prompts(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let prefs_api = api.clone();
    app.on_save_preferences(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() {
            return;
        }
        app.set_saving(true);
        let theme = app.get_runtime_theme().trim().to_string();
        let send_key = app.get_runtime_send_key().trim().to_string();
        let weak = app.as_weak();
        let api = prefs_api.clone();
        run_background(move || {
            let result = api.save_preferences(&theme, &send_key);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(settings) => {
                        apply_settings(&app, settings);
                        app.set_status("偏好已保存".into());
                    }
                    Err(error) => show_error(&app, format!("无法保存偏好：{error}")),
                }
            });
        });
    });

    let weak = app.as_weak();
    let packs_api = api.clone();
    app.on_refresh_prompt_packs(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_prompt_loading() {
            return;
        }
        app.set_prompt_loading(true);
        let weak = app.as_weak();
        let api = packs_api.clone();
        run_background(move || {
            let result = api.list_prompt_packs();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_prompt_loading(false);
                match result {
                    Ok((active, packs, _segments)) => {
                        app.set_prompt_active(active.clone().into());
                        app.set_prompt_packs(ModelRc::new(VecModel::from(
                            packs
                                .iter()
                                .map(|pack| crate::PromptPackCard {
                                    id: pack.id.clone().into(),
                                    readonly: pack.readonly,
                                    locale: pack.locale.clone().into(),
                                    is_active: pack.id == active,
                                    is_lang_default: pack.is_system_language_default,
                                })
                                .collect::<Vec<_>>(),
                        )));
                        let selected = app.get_prompt_selected_pack().to_string();
                        let selected = if selected.is_empty() {
                            active.clone()
                        } else {
                            selected
                        };
                        app.set_prompt_selected_pack(selected.into());
                        app.invoke_select_prompt_pack(
                            app.get_prompt_selected_pack().clone(),
                        );
                    }
                    Err(error) => show_error(&app, format!("无法读取提示词包：{error}")),
                }
            });
        });
    });

    let weak = app.as_weak();
    let select_api = api.clone();
    app.on_select_prompt_pack(move |pack_id| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_prompt_loading() || pack_id.trim().is_empty() {
            return;
        }
        let segment_index = app.get_prompt_selected_segment().max(0) as usize;
        let Some(key) = PROMPT_SEGMENT_KEYS.get(segment_index) else {
            return;
        };
        let pack_id = pack_id.trim().to_string();
        app.set_prompt_selected_pack(pack_id.clone().into());
        let activate = app.get_prompt_active().trim() != pack_id;
        let weak = app.as_weak();
        let api = select_api.clone();
        run_background(move || {
            let read = api.read_prompt_segment(&pack_id, key);
            let activate = if activate {
                api.set_active_prompt_pack(&pack_id).is_ok()
            } else {
                false
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                match read {
                    Ok(segment) => {
                        app.set_prompt_draft(segment.content.into());
                        app.set_prompt_readonly(segment.readonly);
                        if activate {
                            app.set_prompt_active(pack_id.clone().into());
                            app.set_prompt_packs(app.get_prompt_packs());
                        }
                    }
                    Err(error) => show_error(&app, format!("无法读取提示词分段：{error}")),
                }
            });
        });
    });

    let weak = app.as_weak();
    let save_api = api.clone();
    app.on_save_prompt_segment(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_prompt_loading() || app.get_prompt_readonly() {
            return;
        }
        let pack_id = app.get_prompt_selected_pack().trim().to_string();
        if pack_id.is_empty() {
            return;
        }
        let segment_index = app.get_prompt_selected_segment().max(0) as usize;
        let Some(key) = PROMPT_SEGMENT_KEYS.get(segment_index) else {
            return;
        };
        let content = app.get_prompt_draft().to_string();
        app.set_prompt_loading(true);
        let weak = app.as_weak();
        let api = save_api.clone();
        run_background(move || {
            let result = api.write_prompt_segment(&pack_id, key, &content);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_prompt_loading(false);
                match result {
                    Ok(()) => app.set_status("提示词分段已保存".into()),
                    Err(error) => show_error(&app, format!("无法保存提示词分段：{error}")),
                }
            });
        });
    });

    let weak = app.as_weak();
    let create_api = api.clone();
    app.on_create_prompt_pack(move || {
        let Some(app) = weak.upgrade() else { return };
        let name = app.get_pack_name_draft().trim().to_string();
        if name.is_empty() || app.get_prompt_loading() {
            return;
        }
        app.set_prompt_loading(true);
        let weak = app.as_weak();
        let api = create_api.clone();
        run_background(move || {
            let result = api
                .create_prompt_pack(&name)
                .and_then(|_| api.set_active_prompt_pack(&name));
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_prompt_loading(false);
                match result {
                    Ok(()) => {
                        app.set_pack_name_draft("".into());
                        app.set_prompt_selected_pack(name.clone().into());
                        app.invoke_refresh_prompt_packs();
                        app.set_status(format!("提示词包已创建：{name}").into());
                    }
                    Err(error) => show_error(&app, format!("无法创建提示词包：{error}")),
                }
            });
        });
    });

    let weak = app.as_weak();
    let delete_api = api.clone();
    app.on_delete_prompt_pack(move || {
        let Some(app) = weak.upgrade() else { return };
        let pack_id = app.get_prompt_selected_pack().trim().to_string();
        if pack_id.is_empty() || app.get_prompt_loading() {
            return;
        }
        app.set_prompt_loading(true);
        let weak = app.as_weak();
        let api = delete_api.clone();
        run_background(move || {
            let result = api.delete_prompt_pack(&pack_id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_prompt_loading(false);
                match result {
                    Ok(()) => {
                        app.set_prompt_selected_pack("".into());
                        app.invoke_refresh_prompt_packs();
                        app.set_status("提示词包已删除".into());
                    }
                    Err(error) => show_error(&app, format!("无法删除提示词包：{error}")),
                }
            });
        });
    });

    let weak = app.as_weak();
    let diag_api = api.clone();
    app.on_export_diagnostics(move || {
        let Some(app) = weak.upgrade() else { return };
        let dir = std::path::PathBuf::from(app.get_workspace_root().to_string())
            .join("diagnostics");
        let weak = app.as_weak();
        let api = diag_api.clone();
        run_background(move || {
            let result = api.export_diagnostics(&dir);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(path) => {
                    let path = path.display().to_string();
                    app.set_status(format!("诊断信息已导出：{path}").into());
                    app.set_dialog_title("诊断信息已导出".into());
                    app.set_dialog_text(format!("已写入 {path}").into());
                    app.set_dialog_open(true);
                }
                Err(error) => show_error(&app, format!("无法导出诊断信息：{error}")),
            });
        });
    });

    let weak = app.as_weak();
    let reset_api = api.clone();
    app.on_reset_work_state(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() {
            return;
        }
        app.set_saving(true);
        let weak = app.as_weak();
        let api = reset_api.clone();
        run_background(move || {
            let result = api.reset_work_state();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(summary) => {
                        let text = format!(
                            "已停止 {} 个运行中任务，清理 {} 个工作区条目；会话历史与文件已保留。",
                            summary.cancelled_sessions + summary.cancelled_tasks,
                            summary.removed_workspace_entries
                        );
                        app.set_status(text.clone().into());
                        app.set_dialog_title("工作状态已重置".into());
                        app.set_dialog_text(text.into());
                        app.set_dialog_open(true);
                    }
                    Err(error) => show_error(&app, format!("无法重置工作状态：{error}")),
                }
            });
        });
    });
}

fn bind_profile(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let refresh_api = api.clone();
    app.on_refresh_profile(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_profile_loading() {
            return;
        }
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
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(profile) => apply_profile(&app, profile),
                Err(error) => show_error(&app, format!("无法保存头像：{error}")),
            });
        });
    });
    let weak = app.as_weak();
    let profile_api = api.clone();
    app.on_save_profile(move |username, email, unit| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_profile_loading() {
            return;
        }
        app.set_profile_loading(true);
        let weak = app.as_weak();
        let api = profile_api.clone();
        run_background(move || {
            let result = api.update_profile(&username, &email, &unit);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_profile_loading(false);
                match result {
                    Ok(profile) => {
                        apply_profile(&app, profile);
                        app.set_status("个人资料已保存".into());
                    }
                    Err(error) => show_error(&app, format!("无法保存个人资料：{error}")),
                }
            });
        });
    });
}

fn apply_profile(app: &MainWindow, profile: NativeProfile) {
    app.set_profile(crate::ProfileCard {
        user_id: profile.user_id.into(),
        username: profile.username.into(),
        email: profile.email.into(),
        unit: profile.unit.into(),
        unit_id: profile.unit_id.into(),
        sessions: profile.sessions.to_string().into(),
        sessions_last_7d: profile.sessions_last_7d.to_string().into(),
        tool_calls: profile.tool_calls.to_string().into(),
        tokens: profile.consumed_tokens.to_string().into(),
        agents: profile.agents.to_string().into(),
        last_active: profile.last_active_at.into(),
        avatar_icon: profile.avatar_icon.clone().into(),
        avatar_glyph: profile_glyph(&profile.avatar_icon).into(),
        avatar_color: profile.avatar_color.into(),
    });
}

fn profile_glyph(icon: &str) -> &'static str {
    match icon.trim() {
        "initial" => "✦",
        "qq-avatar-0001" => "◉",
        "qq-avatar-0002" => "❖",
        _ => "✦",
    }
}

fn bind_agents(app: &MainWindow, api: Arc<NativeDesktop>) {
    let save_agent_api = api.clone();
    let weak = app.as_weak();
    app.on_toggle_agent_tool(move |name| {
        let Some(app) = weak.upgrade() else { return };
        let mut names = app
            .get_selected_agent_tool_names()
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>();
        if let Some(pos) = names.iter().position(|value| value == name.as_str()) {
            names.remove(pos);
        } else {
            names.push(name.to_string());
        }
        app.set_selected_agent_tool_names(string_model(names));
        crate::entity_state::sync_tool_selection(&app);
    });
    let weak = app.as_weak();
    app.on_add_agent_question(move || {
        let Some(app) = weak.upgrade() else { return };
        let mut questions = app
            .get_selected_agent_preset_questions()
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>();
        if questions.len() < 12 && !app.get_agent_question_draft().trim().is_empty() {
            questions.push(app.get_agent_question_draft().trim().to_string());
            app.set_selected_agent_preset_questions(string_model(questions));
            app.set_agent_question_draft("".into());
        }
    });
    let weak = app.as_weak();
    app.on_remove_agent_question(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let mut questions = app
            .get_selected_agent_preset_questions()
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>();
        if let Ok(index) = usize::try_from(index) {
            if index < questions.len() {
                questions.remove(index);
                app.set_selected_agent_preset_questions(string_model(questions));
            }
        }
    });
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
    app.on_save_agent(
        move |name,
              description,
              system_prompt,
              model,
              icon_name,
              icon_color,
              tool_names,
              preset_questions,
              sandbox_container_id,
              approval_mode,
              preview_skill,
              silent,
              prefer_mother| {
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
            let api = save_agent_api.clone();
            let id = agent.id.to_string();
            let tool_names = tool_names.iter().map(|v| v.to_string()).collect::<Vec<_>>();
            let preset_questions = preset_questions
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>();
            run_background(move || {
                let result = api.update_agent_settings(
                    &id,
                    AgentSettingsEdit {
                        name: name.to_string(),
                        description: description.to_string(),
                        system_prompt: system_prompt.to_string(),
                        model_name: model.to_string(),
                        icon_name: icon_name.to_string(),
                        icon_color: icon_color.to_string(),
                        tool_names,
                        preset_questions,
                        sandbox_container_id,
                        approval_mode: approval_mode.to_string(),
                        preview_skill,
                        silent,
                        prefer_mother,
                    },
                );
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
        },
    );
    let weak = app.as_weak();
    let delete_api = api.clone();
    app.on_delete_agent(move || {
        let Some(app) = weak.upgrade() else { return };
        let Some(index) = usize::try_from(app.get_selected_agent()).ok() else {
            return;
        };
        let Some(row) = app.get_agents().row_data(index) else {
            return;
        };
        if row.is_shared || row.id.is_empty() || app.get_saving() {
            return;
        }
        app.set_saving(true);
        let id = row.id.to_string();
        let weak = app.as_weak();
        let api = delete_api.clone();
        run_background(move || {
            let result = api.delete_agent(&id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(()) => {
                        let mut rows = app.get_agents().iter().collect::<Vec<_>>();
                        rows.retain(|item| item.id != id);
                        app.set_agents(model_from(rows));
                        app.set_selected_agent(-1);
                        app.set_status("智能体已删除".into());
                    }
                    Err(error) => show_error(&app, format!("无法删除智能体：{error}")),
                }
            });
        });
    });
}

/// The card directory lives inside the workspace so exports are easy to find
/// and are covered by the workspace path safety rules.
fn agent_card_dir(app: &MainWindow) -> std::path::PathBuf {
    std::path::PathBuf::from(app.get_workspace_root().to_string())
        .join("agent-cards")
}

fn bind_agent_cards(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let export_api = api.clone();
    app.on_export_agent_card(move || {
        let Some(app) = weak.upgrade() else { return };
        let Some(index) = usize::try_from(app.get_selected_agent()).ok() else {
            return;
        };
        let Some(row) = app.get_agents().row_data(index) else {
            return;
        };
        if row.id.is_empty() || app.get_saving() {
            return;
        }
        let dir = agent_card_dir(&app);
        let weak = app.as_weak();
        let api = export_api.clone();
        let id = row.id.to_string();
        run_background(move || {
            let result = api
                .export_agent_to_file(&id, &dir)
                .map(|path| path.display().to_string());
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(path) => {
                    app.set_status(format!("工蜂卡已导出：{path}").into());
                    app.set_dialog_title("工蜂卡已导出".into());
                    app.set_dialog_text(format!("已写入 {path}；导入时从同一目录选择。").into());
                    app.set_dialog_open(true);
                }
                Err(error) => show_error(&app, format!("无法导出工蜂卡：{error}")),
            });
        });
    });
    let weak = app.as_weak();
    let list_api = api.clone();
    app.on_open_agent_card_import(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_agent_cards_open(true);
        app.set_agent_card_files(ModelRc::default());
        let dir = agent_card_dir(&app);
        let weak = app.as_weak();
        let api = list_api.clone();
        run_background(move || {
            let result = api.list_agent_card_files(&dir);
            let _ = weak.upgrade_in_event_loop(move |app| {
                match result {
                    Ok(files) => {
                        app.set_agent_card_files(ModelRc::new(VecModel::from(
                            files
                                .into_iter()
                                .map(|(path, label)| crate::AgentCardFile {
                                    path: path.into(),
                                    label: label.into(),
                                })
                                .collect::<Vec<_>>(),
                        )));
                    }
                    Err(error) => show_error(&app, format!("无法扫描卡片目录：{error}")),
                }
                app.set_agent_cards_loading(false);
            });
        });
        app.set_agent_cards_loading(true);
    });
    let weak = app.as_weak();
    app.on_close_agent_card_import(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_agent_cards_open(false);
        app.set_agent_card_files(ModelRc::default());
    });
    let weak = app.as_weak();
    let import_api = api.clone();
    app.on_import_agent_card(move |path| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_agent_cards_loading() || path.is_empty() {
            return;
        }
        let overwrite = app.get_agent_card_overwrite();
        // Overwriting an existing agent is destructive and needs its own
        // confirmation beyond the checkbox.
        if overwrite {
            app.set_dialog_title("确认覆盖导入".into());
            app.set_dialog_text("将用卡片内容覆盖同名专家的名称、提示词、工具与运行策略，且不可自动恢复。确认继续？".into());
            app.set_dialog_open(true);
        }
        let path = path.to_string();
        let weak = app.as_weak();
        let api = import_api.clone();
        let confirmed = overwrite;
        // The dialog is informational; the checkbox is the explicit opt-in.
        let _ = confirmed;
        run_background(move || {
            let result = api.import_agent_from_file(std::path::Path::new(&path), overwrite);
            let _ = weak.upgrade_in_event_loop(move |app| {
                match result {
                    Ok(outcomes) => {
                        for outcome in &outcomes {
                            if outcome.missing_tools.is_empty() && outcome.missing_skills.is_empty()
                            {
                                continue;
                            }
                            show_error(
                                &app,
                                format!(
                                    "导入的工蜂卡缺少依赖：工具 {:?}；技能 {:?}（已在专家中保留声明）",
                                    outcome.missing_tools, outcome.missing_skills
                                ),
                            );
                        }
                        app.invoke_refresh_agents();
                        let names: Vec<String> = outcomes
                            .iter()
                            .map(|outcome| {
                                format!(
                                    "{}（{}）",
                                    outcome.agent.name,
                                    if outcome.created { "新建" } else { "覆盖" }
                                )
                            })
                            .collect();
                        app.set_status(format!("已导入：{}", names.join("、")).into());
                    }
                    Err(error) => show_error(&app, format!("无法导入工蜂卡：{error}")),
                }
                app.set_agent_cards_open(false);
                app.set_agent_card_files(ModelRc::default());
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
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        app.set_saving(true);
        let weak = weak.clone();
        let api = lan_api.clone();
        run_background(move || {
            let result = api.save_lan(enabled, &name);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(settings) => {
                        apply_settings(&app, settings);
                        app.set_status("内网通信设置已保存".into());
                    }
                    Err(error) => show_error(&app, format!("无法保存内网通信设置：{error}")),
                }
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
        tool_count: agent.tool_names.len() as i32,
        tool_names: string_model(agent.tool_names.clone()),
        preset_questions: string_model(agent.preset_questions.clone()),
        sandbox_container_id: agent.sandbox_container_id,
        approval_mode: agent.approval_mode.into(),
        preview_skill: agent.preview_skill,
        silent: agent.silent,
        prefer_mother: agent.prefer_mother,
        preset_question_count: agent.preset_questions.len() as i32,
        is_shared: agent.is_shared,
        group_name: agent.hive_id.into(),
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
        enabled: false,
    }
}

pub(crate) fn apply_settings(app: &MainWindow, settings: DesktopSettings) {
    let selected = app.get_selected_model_key();
    app.set_workspace_root(settings.workspace_root.into());
    app.set_runtime_language(settings.language.into());
    app.set_runtime_theme(settings.theme.into());
    app.set_runtime_send_key(settings.send_key.into());
    app.set_lan_enabled(settings.lan.enabled);
    app.set_lan_name(settings.lan.display_name.into());
    app.set_lan_peer_id(settings.lan.peer_id.into());
    app.set_lan_endpoint(
        format!("{}:{}", settings.lan.listen_host, settings.lan.listen_port).into(),
    );
    app.set_lan_peer_count(settings.lan.peer_count as i32);
    app.set_lan_peers(model_from(
        settings
            .lan
            .peers
            .into_iter()
            .map(to_lan_peer_card)
            .collect(),
    ));
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
    crate::LanPeerCard {
        peer_id: peer.peer_id.into(),
        display_name: peer.display_name.into(),
        address: format!("{}:{}", peer.lan_ip, peer.listen_port).into(),
    }
}

fn select_model_key(app: &MainWindow, key: &str) {
    if let Some(index) = app.get_models().iter().position(|model| model.key == key) {
        app.invoke_select_model(index as i32);
    }
}

fn model_from<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

fn string_model(rows: Vec<String>) -> ModelRc<slint::SharedString> {
    model_from(rows.into_iter().map(Into::into).collect())
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
