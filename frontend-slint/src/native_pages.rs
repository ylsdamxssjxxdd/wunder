//! Native entity-page callbacks. All storage and configuration work runs off the UI thread.
use crate::{AgentCard, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wunder_desktop::{
    AgentRecord, CloudStatusView, DesktopSettings, LanPeerRecord, ModelEdit, NativeDesktop,
    NativeProfile, RuntimeToolStatus,
};

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    crate::agent_editor::install(app);
    crate::agent_editor::install_save_handler(app, api.clone());
    crate::entity_state::bind_selection(app);
    crate::avatar_ui::install(app, api.clone());
    bind_agents(app, api.clone());
    bind_model_probes(app, api.clone());
    bind_preferences_and_prompts(app, api.clone());
    crate::tools_ui::install(app, api.clone());
    bind_settings(app, api.clone());
    bind_cloud(app, api.clone());
    bind_profile(app, api.clone());
    crate::native_interlink::install(app, api.clone());
    crate::cron_ui::install(app, api.clone());
    crate::channel_ui::install(app, api.clone());
    crate::expert_ui::install(app, api.clone());
    crate::runtime_settings::install(app, api);
    app.invoke_refresh_agents();
    app.invoke_refresh_tools();
    app.invoke_refresh_settings();
    // The profile owns the signed-in avatar the sidebar settings button shows,
    // so it loads with the rest of the shell instead of staying at its default
    // until some panel happens to refresh it.
    app.invoke_refresh_profile();
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
        let known_key = !key.is_empty() && app.get_models().iter().any(|entry| entry.key == key);
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
        let known_key = !key.is_empty() && app.get_models().iter().any(|entry| entry.key == key);
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
                        let preview = voices
                            .iter()
                            .take(12)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("、");
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
        let font_size = app.get_runtime_font_size().round() as i32;
        let weak = app.as_weak();
        let api = prefs_api.clone();
        run_background(move || {
            let result = api.save_preferences(&theme, &send_key, font_size);
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
                        app.set_prompt_pack_names(ModelRc::new(VecModel::from(
                            packs.iter().map(|pack| pack.id.clone().into()).collect::<Vec<_>>(),
                        )));
                        let selected = app.get_prompt_selected_pack().to_string();
                        let selected = if selected.is_empty() {
                            active.clone()
                        } else {
                            selected
                        };
                        app.set_prompt_selected_pack(selected.into());
                        app.invoke_select_prompt_pack(app.get_prompt_selected_pack().clone());
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
            let _ = weak.upgrade_in_event_loop(move |app| match read {
                Ok(segment) => {
                    app.set_prompt_draft(segment.content.into());
                    app.set_prompt_readonly(segment.readonly);
                    if activate {
                        app.set_prompt_active(pack_id.clone().into());
                        app.set_prompt_packs(app.get_prompt_packs());
                    }
                }
                Err(error) => show_error(&app, format!("无法读取提示词分段：{error}")),
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
    let set_active_api = api.clone();
    app.on_set_active_prompt_pack(move || {
        let Some(app) = weak.upgrade() else { return };
        let pack_id = app.get_prompt_selected_pack().trim().to_string();
        if pack_id.is_empty() || app.get_prompt_loading() {
            return;
        }
        if app.get_prompt_active().trim() == pack_id {
            return;
        }
        app.set_prompt_loading(true);
        let weak = app.as_weak();
        let api = set_active_api.clone();
        run_background(move || {
            let result = api.set_active_prompt_pack(&pack_id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_prompt_loading(false);
                match result {
                    Ok(()) => {
                        app.invoke_refresh_prompt_packs();
                        app.invoke_refresh_prompt_preview();
                        app.set_status("已设为启用".into());
                    }
                    Err(error) => show_error(&app, format!("无法启用提示词包：{error}")),
                }
            });
        });
    });

    let weak = app.as_weak();
    let preview_api = api.clone();
    app.on_refresh_prompt_preview(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_prompt_preview_loading() {
            return;
        }
        app.set_prompt_preview_loading(true);
        let weak = app.as_weak();
        let api = preview_api.clone();
        run_background(move || {
            let result = api.preview_system_prompt();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_prompt_preview_loading(false);
                match result {
                    Ok(prompt) => app.set_prompt_preview(prompt.into()),
                    Err(error) => {
                        app.set_prompt_preview("".into());
                        show_error(&app, format!("系统提示词预览失败：{error}"));
                    }
                }
            });
        });
    });

    let weak = app.as_weak();
    let diag_api = api.clone();
    app.on_export_diagnostics(move || {
        let Some(app) = weak.upgrade() else { return };
        let dir =
            std::path::PathBuf::from(app.get_workspace_root().to_string()).join("diagnostics");
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

/// Project the façade's `CloudStatusView` onto the shell properties the
/// settings card and the sidebar quota badge bind to. Signed-in drafts show
/// the session values; signed-out drafts keep whatever the user typed.
fn push_cloud_status(app: &MainWindow, status: CloudStatusView) {
    let CloudStatusView {
        logged_in,
        expired,
        server,
        username,
        balance,
        daily_grant,
        max_concurrent_calls,
        concurrency_active,
        preferences_sync_enabled,
        connection,
        last_error,
        last_success_at,
        next_retry_at,
    } = status;
    app.set_cloud_logged_in(logged_in);
    app.set_cloud_expired(expired);
    app.set_cloud_username(username.clone().into());
    app.set_cloud_balance(balance.to_string().into());
    app.set_cloud_concurrency(format!("{}/{}", concurrency_active, max_concurrent_calls).into());
    // The badge warns at or below 10% of the daily grant.
    app.set_cloud_quota_warn(logged_in && balance * 10 <= daily_grant);
    app.set_cloud_sync_enabled(preferences_sync_enabled);
    app.set_cloud_connection(connection.into());
    app.set_cloud_last_error(last_error.into());
    app.set_cloud_last_success(last_success_at.into());
    app.set_cloud_next_retry(next_retry_at.into());
    if logged_in {
        app.set_cloud_server_draft(server.into());
        app.set_cloud_user_draft(username.into());
    }
    app.set_cloud_pass_draft("".into());
    app.set_cloud_error("".into());
}

// Poll timer for the settings cloud card. One thread-local timer reused for
// the whole process: it is (re)started whenever the settings page opens and
// stops itself on the first tick after the page closes, so the only standing
// cost while the page is hidden is nothing at all.
thread_local! {
    static CLOUD_POLL_TIMER: slint::Timer = slint::Timer::default();
}

/// (Re)start the 30s cloud-status poll. Every path that opens the settings
/// page goes through `refresh-settings`, which lands here. The tick skips the
/// fetch unless the page is open on the cloud card's panel, and stops itself
/// once the page has closed.
fn restart_cloud_status_poll(app_weak: slint::Weak<MainWindow>, api: Arc<NativeDesktop>) {
    CLOUD_POLL_TIMER.with(|timer| {
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(30), move || {
            let Some(app) = app_weak.upgrade() else { return };
            if !app.get_settings_open() || app.get_settings_active_panel() != 0 {
                // Page closed or the cloud card's tab is hidden: stop polling.
                CLOUD_POLL_TIMER.with(slint::Timer::stop);
                return;
            }
            if app.get_cloud_busy() {
                return;
            }
            let weak = app_weak.clone();
            let api = api.clone();
            run_background(move || {
                let status = api.cloud_status();
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if let Ok(status) = status {
                        push_cloud_status(&app, status);
                    }
                });
            });
        });
    });
}

/// Cloud account façade wiring (§6.1): login/logout/refresh/preference-sync
/// all run on background threads through the `NativeDesktop` façade, then
/// land their result back on the UI thread. The password draft only travels
/// from the shell into the call; it is never logged nor persisted.
fn bind_cloud(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let login_api = api.clone();
    app.on_cloud_login(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_busy() {
            return;
        }
        let server = app.get_cloud_server_draft().trim().to_string();
        let username = app.get_cloud_user_draft().trim().to_string();
        let password = app.get_cloud_pass_draft().to_string();
        // Slint-side guard: an obviously invalid address never leaves the shell.
        if server.is_empty() || username.is_empty() || password.is_empty() {
            app.set_cloud_error("请填写服务地址、用户名和密码".into());
            return;
        }
        app.set_cloud_busy(true);
        app.set_cloud_error("".into());
        let weak = weak.clone();
        let api = login_api.clone();
        run_background(move || {
            let result = api.cloud_login(&server, &username, &password);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_cloud_busy(false);
                match result {
                    Ok(status) => {
                        push_cloud_status(&app, status);
                        app.set_status("已登录云端".into());
                        // The engine just synthesized cloud/<id> models; the
                        // settings projection picks them up here.
                        app.invoke_refresh_settings();
                    }
                    Err(error) => {
                        app.set_cloud_error(error.to_string().into());
                        app.set_status(format!("云端登录失败：{error}").into());
                    }
                }
            });
        });
    });
    let weak = app.as_weak();
    let logout_api = api.clone();
    app.on_cloud_logout(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_busy() {
            return;
        }
        app.set_cloud_busy(true);
        app.set_cloud_error("".into());
        let weak = weak.clone();
        let api = logout_api.clone();
        run_background(move || {
            let result = api.cloud_logout();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_cloud_busy(false);
                match result {
                    Ok(status) => {
                        push_cloud_status(&app, status);
                        app.set_status("已断开云端".into());
                        app.invoke_refresh_settings();
                    }
                    Err(error) => {
                        app.set_cloud_error(error.to_string().into());
                        app.set_status(format!("云端登出失败：{error}").into());
                    }
                }
            });
        });
    });
    let weak = app.as_weak();
    let refresh_api = api.clone();
    app.on_cloud_refresh_account(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_busy() {
            return;
        }
        app.set_cloud_busy(true);
        app.set_cloud_error("".into());
        let weak = weak.clone();
        let api = refresh_api.clone();
        run_background(move || {
            let result = api.cloud_refresh_account();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_cloud_busy(false);
                match result {
                    Ok(status) => {
                        push_cloud_status(&app, status);
                        app.set_status("云端额度已刷新".into());
                    }
                    Err(error) => {
                        app.set_cloud_error(error.to_string().into());
                        // A 401 lands as "登录已过期，请重新登录" from the
                        // façade; refresh the badge anyway so the shell knows.
                        let status = api.cloud_status().ok();
                        if let Some(status) = status {
                            push_cloud_status(&app, status);
                        }
                    }
                }
            });
        });
    });
    let weak = app.as_weak();
    let sync_api = api.clone();
    app.on_cloud_toggle_sync(move |enabled| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_busy() {
            return;
        }
        app.set_cloud_busy(true);
        app.set_cloud_error("".into());
        let weak = weak.clone();
        let api = sync_api.clone();
        run_background(move || {
            let result = api.cloud_set_sync_preferences(enabled);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_cloud_busy(false);
                match result {
                    Ok(status) => {
                        push_cloud_status(&app, status);
                        app.set_status(if enabled { "已开启基础设置同步".into() } else { "已关闭基础设置同步".into() });
                    }
                    Err(error) => {
                        app.set_cloud_error(error.to_string().into());
                    }
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
    let avatar_brush = profile_brush(&profile.avatar_color);
    let avatar_glyph: slint::SharedString = profile_glyph(&profile.username).into();
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
        avatar_glyph,
        avatar_icon: profile.avatar_icon.into(),
        avatar_color: profile.avatar_color.into(),
        avatar_brush,
    });
}

// `avatar_color` is a `#rrggbb` preference string and Slint has no string→color
// cast, so the shell resolves the brush once here; avatar surfaces then paint a
// real color instead of re-parsing on every property change. Unknown values
// keep the backend default (`DEFAULT_AVATAR_COLOR` in `wunder-desktop`).
fn profile_brush(color: &str) -> slint::Color {
    let hex = color.trim().strip_prefix('#').unwrap_or_default();
    if hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        if let Ok(value) = u32::from_str_radix(hex, 16) {
            return slint::Color::from_rgb_u8(
                (value >> 16) as u8,
                (value >> 8) as u8,
                value as u8,
            );
        }
    }
    slint::Color::from_rgb_u8(0x3b, 0x82, 0xf6)
}

// The mark printed on the profile avatar bubble. The desktop ships no avatar
// artwork: `avatar_icon` is either `initial` or a `qq-avatar-NNNN` key whose web
// image set has no desktop counterpart, and the Unicode symbols the old mapping
// chose (✦ ◉ ❖) are absent from the bundled Microsoft YaHei (`assets/fonts/
// msyh.ttc`), so they rendered as tofu. The signed-in name's first character is
// the one mark that always draws; an empty name leaves the bubble to the shared
// "me" label on the Slint side.
fn profile_glyph(username: &str) -> String {
    match username.trim().chars().next() {
        Some(first) => first.to_uppercase().collect(),
        None => String::new(),
    }
}

fn bind_agents(app: &MainWindow, api: Arc<NativeDesktop>) {
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
    // §9.3 agent form: the model dropdown hands back a key, the tool switches
    // hand back a tool name, and the settings search box re-projects the tool
    // list because Slint 1.18 strings have no substring test.
    let weak = app.as_weak();
    app.on_agent_model_picked(move |key| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_models().iter().any(|card| card.key == key) {
            app.set_selected_agent_model(key);
        }
    });
    let weak = app.as_weak();
    app.on_toggle_agent_tool_option(move |name| {
        let Some(app) = weak.upgrade() else { return };
        // The toggle owns the selection set; `sync_tool_selection` re-projects
        // the card picker's enabled flags from it.
        app.invoke_toggle_agent_tool(name);
        app.invoke_refresh_agent_dirty();
    });
    let weak = app.as_weak();
    app.on_refresh_agent_model_options(move || {
        if let Some(app) = weak.upgrade() {
            update_agent_model_options(&app);
        }
    });
    let weak = app.as_weak();
    let refresh_api = api.clone();
    app.on_refresh_agents(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_agents_loading() || app.get_saving() {
            return;
        }
        // Refreshing re-projects the loaded agent over the editor drafts, so
        // pending edits need the same discard confirmation as leaving.
        if app.get_agent_dirty() {
            crate::agent_editor::request_leave(
                &app,
                crate::agent_editor::LeaveAction::RefreshAgents,
            );
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
}

fn bind_settings(app: &MainWindow, api: Arc<NativeDesktop>) {
    // 线程渲染压测：解析两个轮次输入后交给 facade 同步生成，工作线程把节流后的
    // 进度推回状态行；完成后刷新线程列表，让新线程立即可见。
    let weak = app.as_weak();
    let stress_api = api.clone();
    app.on_generate_stress_thread(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_stress_running() || app.get_saving() || app.get_settings_loading() {
            return;
        }
        let Ok(user_rounds) = app.get_stress_user_rounds().trim().parse::<i64>() else {
            app.set_stress_status("⚠ 用户轮次必须是正整数".into());
            return;
        };
        let Ok(model_rounds) = app.get_stress_model_rounds().trim().parse::<i64>() else {
            app.set_stress_status("⚠ 每轮模型轮次必须是正整数".into());
            return;
        };
        app.set_stress_running(true);
        app.set_stress_status(format!("正在生成 {user_rounds}×{model_rounds} 压测线程…").into());
        let weak = app.as_weak();
        let api = stress_api.clone();
        std::thread::spawn(move || {
            let total = user_rounds;
            let progress_app = weak.clone();
            let mut last_report = std::time::Instant::now();
            let result = api.generate_stress_thread(user_rounds, model_rounds, move |done, items| {
                // 节流到约每 300ms 一次；批量写入在两次报告之间全速进行。
                let now = std::time::Instant::now();
                if done >= total || now.duration_since(last_report).as_millis() >= 300 {
                    last_report = now;
                    let weak = progress_app.clone();
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        app.set_stress_status(
                            format!("正在生成… 已完成 {done}/{total} 轮次，写入 {items} 条消息").into(),
                        );
                    });
                }
            });
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_stress_running(false);
                match result {
                    Ok(stats) => {
                        let text = format!(
                            "已生成 {} 轮次 / {} 条消息 / {} 次工具调用",
                            stats.user_turns, stats.items, stats.tool_calls
                        );
                        app.set_stress_status(text.clone().into());
                        app.set_dialog_title("压测线程已生成".into());
                        app.set_dialog_text(format!("{text}\n会话 ID：{}", stats.session_id).into());
                        app.set_dialog_open(true);
                        app.invoke_refresh_chat();
                    }
                    Err(error) => {
                        app.set_stress_status(format!("⚠ 生成失败：{error}").into());
                    }
                }
            });
        });
    });
    // §8.3 model search: the popover hands over its query and gets the filtered
    // catalogue back in `models`.
    let weak = app.as_weak();
    let catalogue = model_catalogue();
    app.on_search_models(move |query| {
        let Some(app) = weak.upgrade() else { return };
        catalogue.filter(&app, &query);
    });
    // §9.1 settings search. Slint 1.18 has neither substring tests nor bitwise
    // operators, so the query is matched here and the result comes back as one
    // boolean per §9.2 category.
    let weak = app.as_weak();
    app.on_filter_settings_categories(move |query| {
        let Some(app) = weak.upgrade() else { return };
        let flags = crate::settings_search::category_flags(&query);
        app.set_settings_category_flags(ModelRc::new(VecModel::from(flags)));
    });
    let weak = app.as_weak();
    let refresh_api = api.clone();
    app.on_refresh_settings(move || {
        let Some(app) = weak.upgrade() else { return };
        // Every open of the settings page lands here: keep the 30s cloud
        // status poll running while the page stays open.
        restart_cloud_status_poll(app.as_weak(), refresh_api.clone());
        if app.get_settings_loading() || app.get_saving() {
            return;
        }
        app.set_settings_loading(true);
        app.set_status("正在同步…".into());
        let weak = weak.clone();
        let api = refresh_api.clone();
        run_background(move || {
            // The settings pull doubles as the cloud-status pull (§6.1): the
            // settings card and the sidebar quota badge stay in sync with one
            // refresh, and a cloud error never blocks the settings payload.
            let result = api.get_desktop_settings();
            let cloud = api.cloud_status();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_settings_loading(false);
                match result {
                    Ok(settings) => {
                        apply_settings(&app, settings);
                        app.set_status("本地设置已同步".into());
                    }
                    Err(error) => show_error(&app, format!("无法读取本地设置：{error}")),
                }
                match cloud {
                    Ok(status) => push_cloud_status(&app, status),
                    Err(_) => {}
                }
            });
        });
    });
    let weak = app.as_weak();
    let save_api = api.clone();
    app.on_save_model(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() || app.get_agents_loading() {
            return;
        }
        app.set_saving(true);
        // Bind editor drafts to locals first: ModelEdit borrows strings, and
        // getter temporaries must not be dropped before the background task.
        let key = app.get_model_key_draft().trim().to_string();
        let provider = app.get_model_provider_draft().trim().to_string();
        let model = app.get_model_name_draft().trim().to_string();
        let base_url = app.get_model_base_url_draft().trim().to_string();
        let api_key = app.get_model_token_draft().trim().to_string();
        let model_type = app.get_model_type_draft().trim().to_string();
        let temperature = app.get_model_temperature_draft().trim().to_string();
        let timeout_s = app.get_model_timeout_draft().trim().to_string();
        let max_output = app.get_model_max_output_draft().trim().to_string();
        let thinking_token_budget = app.get_model_thinking_budget_draft().trim().to_string();
        let max_rounds = app.get_model_max_rounds_draft().trim().to_string();
        let max_context = app.get_model_max_context_draft().trim().to_string();
        let tts_voice = app.get_model_tts_voice_draft().trim().to_string();
        let tts_response_format = app.get_model_tts_format_draft().trim().to_string();
        let tts_speed = app.get_model_tts_speed_draft().trim().to_string();
        let tts_instructions = app.get_model_tts_instructions_draft().trim().to_string();
        let asr_language = app.get_model_asr_language_draft().trim().to_string();
        let asr_response_format = app.get_model_asr_format_draft().trim().to_string();
        let asr_temperature = app.get_model_asr_temperature_draft().trim().to_string();
        let asr_prompt = app.get_model_asr_prompt_draft().trim().to_string();
        let image_size = app.get_model_image_size_draft().trim().to_string();
        let image_output_format = app.get_model_image_format_draft().trim().to_string();
        let image_steps = app.get_model_image_steps_draft().trim().to_string();
        let image_guidance_scale = app.get_model_image_guidance_draft().trim().to_string();
        let image_negative_prompt = app.get_model_image_negative_draft().trim().to_string();
        let video_size = app.get_model_video_size_draft().trim().to_string();
        let video_seconds = app.get_model_video_seconds_draft().trim().to_string();
        let video_fps = app.get_model_video_fps_draft().trim().to_string();
        let video_negative_prompt = app.get_model_video_negative_draft().trim().to_string();
        let weak = weak.clone();
        let api = save_api.clone();
        run_background(move || {
            let result = api.save_model(ModelEdit {
                key: &key,
                provider: &provider,
                model: &model,
                base_url: &base_url,
                api_key: &api_key,
                model_type: &model_type,
                temperature: &temperature,
                timeout_s: &timeout_s,
                max_output: &max_output,
                thinking_token_budget: &thinking_token_budget,
                max_rounds: &max_rounds,
                max_context: &max_context,
                tts_voice: &tts_voice,
                tts_response_format: &tts_response_format,
                tts_speed: &tts_speed,
                tts_instructions: &tts_instructions,
                asr_language: &asr_language,
                asr_response_format: &asr_response_format,
                asr_temperature: &asr_temperature,
                asr_prompt: &asr_prompt,
                image_size: &image_size,
                image_output_format: &image_output_format,
                image_steps: &image_steps,
                image_guidance_scale: &image_guidance_scale,
                image_negative_prompt: &image_negative_prompt,
                video_size: &video_size,
                video_seconds: &video_seconds,
                video_fps: &video_fps,
                video_negative_prompt: &video_negative_prompt,
            });
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(settings) => {
                        app.set_model_token_draft("".into());
                        apply_settings(&app, settings);
                        select_model_key(&app, &key);
                        app.set_status("模型配置已保存到本地运行时".into());
                    }
                    Err(error) => show_error(&app, format!("无法保存模型配置：{error}")),
                }
            });
        });
    });
    let weak = app.as_weak();
    let delete_api = api.clone();
    app.on_delete_model(move |key| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() || app.get_settings_loading() || app.get_agents_loading() {
            return;
        }
        let key = key.trim().to_string();
        if key.is_empty() {
            return;
        }
        app.set_saving(true);
        let weak = weak.clone();
        let api = delete_api.clone();
        run_background(move || {
            let result = api.delete_model(&key);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(settings) => {
                        apply_settings(&app, settings);
                        app.set_status("模型配置已删除".into());
                        app.invoke_refresh_agents();
                    }
                    Err(error) => show_error(&app, format!("无法删除模型配置：{error}")),
                }
            });
        });
    });
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
    let order_api = api.clone();
    app.on_move_model(move |from, delta| {
        let Some(app) = weak.upgrade() else { return };
        let Ok(from) = usize::try_from(from) else { return };
        let mut rows = app.get_models().iter().collect::<Vec<_>>();
        if from >= rows.len() || rows.is_empty() {
            return;
        }
        let to = (from as i64 + i64::from(delta))
            .clamp(0, rows.len() as i64 - 1) as usize;
        if to == from {
            return;
        }
        let row = rows.remove(from);
        rows.insert(to, row);
        // The highlight follows the model, not the slot it occupied.
        let selected_key = app.get_selected_model_key().to_string();
        let selected = rows
            .iter()
            .position(|entry| entry.key == selected_key)
            .map(|index| index as i32)
            .unwrap_or(-1);
        app.set_selected_model(selected);
        let keys: Vec<String> = rows.iter().map(|entry| entry.key.to_string()).collect();
        app.set_models(model_from(rows));
        update_agent_model_options(&app);
        let weak = weak.clone();
        let api = order_api.clone();
        run_background(move || {
            if let Err(error) = api.save_model_order(&keys) {
                let _ = weak.upgrade_in_event_loop(move |app| {
                    show_error(&app, format!("无法保存模型排序：{error}"))
                });
            }
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
    replace_agents(app, agents.into_iter().map(agent_record_to_card).collect());
    crate::entity_state::restore_agent(app, &selected);
    crate::navigation_ui::project(app);
}

/// Install a projected agent list. Shared with the save path so both routes
/// produce identical cards.
pub(crate) fn replace_agents(app: &MainWindow, cards: Vec<AgentCard>) {
    app.set_agents(model_from(cards));
}

/// The conversational models the agent form may name. The runtime only accepts
/// a key that is still configured; a stored key that has since been deleted
/// stays listed so the editor never drops it silently.
pub(crate) fn update_agent_model_options(app: &MainWindow) {
    let mut options: Vec<slint::SharedString> = app
        .get_models()
        .iter()
        .filter(|model| model.model_type == "llm" || model.model_type.is_empty())
        .map(|model| model.key)
        .collect();
    let stored = app.get_selected_agent_model();
    if !stored.is_empty() && !options.contains(&stored) {
        options.push(stored);
    }
    app.set_agent_model_options(model_from(options));
}

pub(crate) fn agent_record_to_card(agent: AgentRecord) -> AgentCard {
    let tone = avatar_tone(&agent.icon_color);
    let icon_config = agent.icon_config.to_payload();
    let icon_image = crate::avatar_ui::avatar_visual(&agent.icon_config);
    AgentCard {
        goal_active: false,
        id: agent.id.into(),
        name: agent.name.into(),
        description: agent.description.into(),
        model: agent.model.into(),
        system_prompt: agent.system_prompt.into(),
        status: agent.status.into(),
        icon_glyph: agent.icon_glyph.into(),
        icon_tone: tone,
        icon_config: icon_config.into(),
        icon_image,
        tool_count: agent.tool_names.len() as i32,
        tool_names: string_model(agent.tool_names.clone()),
        preset_questions: string_model(agent.preset_questions.clone()),
        approval_mode: agent.approval_mode.into(),
        preview_skill: agent.preview_skill,
        silent: agent.silent,
        prefer_mother: agent.prefer_mother,
        preset_question_count: agent.preset_questions.len() as i32,
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

pub(crate) fn apply_settings(app: &MainWindow, settings: DesktopSettings) {
    let selected = app.get_selected_model_key();
    app.set_workspace_root(settings.workspace_root.into());
    app.set_runtime_language(settings.language.into());
    app.set_runtime_theme(settings.theme.clone().into());
    app.set_runtime_send_key(settings.send_key.into());
    app.set_runtime_font_size(settings.font_size as f32);
    // Theme tokens are bindings over the palette/font-scale globals, so the
    // whole shell re-renders in the new accent and chat font scale at once.
    let theme_global = app.global::<crate::Theme>();
    theme_global.set_palette(normalize_palette(&settings.theme).into());
    theme_global.set_font_scale(settings.font_size.clamp(12, 20) as f32 / 14.0);
    app.set_runtime_python(settings.python_path.into());
    app.set_runtime_git(settings.git_path.into());
    app.set_runtime_rg(settings.rg_path.into());
    let status = |tool: &str| {
        settings
            .tool_status
            .iter()
            .find(|entry| entry.tool == tool)
            .map(tool_status_text)
            .unwrap_or_default()
    };
    app.set_runtime_python_status(status("python").into());
    app.set_runtime_git_status(status("git").into());
    app.set_runtime_rg_status(status("rg").into());
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
    // The catalogue object is the search authority: it keeps the unfiltered
    // list and re-projects the composer's filtered view from it.
    model_catalogue().set(
        app,
        settings.models.into_iter().map(to_model_card).collect(),
    );
    crate::entity_state::restore_model(app, &selected);
}

fn to_model_card(model: wunder_desktop::ModelRecord) -> crate::ModelCard {
    let is_cloud = model.key.starts_with("cloud/");
    crate::ModelCard {
        key: model.key.into(),
        provider: model.provider.into(),
        model: model.model.into(),
        base_url: model.base_url.into(),
        model_type: model.model_type.into(),
        is_default: model.is_default,
        // Synthesized cloud entries (`cloud/<id>`) carry the badge and stay
        // read-only in the editor; the shell also greys them in the picker
        // while the cloud session is missing or expired.
        is_cloud,
        temperature: model.temperature.into(),
        timeout_s: model.timeout_s.into(),
        max_output: model.max_output.into(),
        thinking_token_budget: model.thinking_token_budget.into(),
        max_rounds: model.max_rounds.into(),
        max_context: model.max_context.into(),
        tts_voice: model.tts_voice.into(),
        tts_response_format: model.tts_response_format.into(),
        tts_speed: model.tts_speed.into(),
        tts_instructions: model.tts_instructions.into(),
        asr_language: model.asr_language.into(),
        asr_response_format: model.asr_response_format.into(),
        asr_temperature: model.asr_temperature.into(),
        asr_prompt: model.asr_prompt.into(),
        image_size: model.image_size.into(),
        image_output_format: model.image_output_format.into(),
        image_steps: model.image_steps.into(),
        image_guidance_scale: model.image_guidance_scale.into(),
        image_negative_prompt: model.image_negative_prompt.into(),
        video_size: model.video_size.into(),
        video_seconds: model.video_seconds.into(),
        video_fps: model.video_fps.into(),
        video_negative_prompt: model.video_negative_prompt.into(),
    }
}

/// Map a persisted theme value onto a `Theme.palette` name. Only the accent
/// family is selectable: the legacy "light"/"eva-orange" values, and anything
/// unrecognised, resolve to the 蜂巢 terracotta default.
fn normalize_palette(theme: &str) -> &'static str {
    match theme.trim() {
        "hula-green" => "hula-green",
        "minimal" => "minimal",
        "tech-blue" => "tech-blue",
        _ => "terracotta",
    }
}

fn to_lan_peer_card(peer: LanPeerRecord) -> crate::LanPeerCard {
    crate::LanPeerCard {
        peer_id: peer.peer_id.into(),
        display_name: peer.display_name.into(),
        address: format!("{}:{}", peer.lan_ip, peer.listen_port).into(),
    }
}

/// User-facing one-line status for a configured runtime tool path. A warning
/// mark keeps an invalid configured path visible instead of silently falling
/// back to the system interpreter.
fn tool_status_text(status: &RuntimeToolStatus) -> String {
    let detail = if status.effective.is_empty() {
        String::new()
    } else {
        format!(" · {}", status.effective)
    };
    match status.source.as_str() {
        "custom" => format!("✓ 自定义路径已生效{detail}"),
        "embedded" => format!("✓ 已使用内置补充包{detail}"),
        "invalid" => "⚠ 配置路径无效，已回退系统环境".to_string(),
        "system" => "使用系统 PATH".to_string(),
        _ => "⚠ 未找到可用的 Python 运行时，需要配置或安装补充包".to_string(),
    }
}

fn select_model_key(app: &MainWindow, key: &str) {
    if let Some(index) = app.get_models().iter().position(|model| model.key == key) {
        app.invoke_select_model(index as i32);
    }
}

/// The unfiltered model catalogue, kept beside the filtered view the composer
/// popover renders. Search runs here because Slint 1.18 strings have no
/// substring test; keeping the catalogue natively also means a query never has
/// to round-trip through storage.
pub(crate) struct ModelCatalogue(RefCell<Vec<crate::ModelCard>>);

thread_local! {
    /// One catalogue per UI thread. The native shell owns exactly one
    /// `MainWindow` on this thread, and `apply_settings` is reached from three
    /// modules that all already share that window; a thread-local keeps the
    /// unfiltered list beside it instead of threading a handle through every
    /// settings callback.
    static MODEL_CATALOGUE: Rc<ModelCatalogue> = Rc::new(ModelCatalogue(RefCell::new(Vec::new())));
}

/// The UI thread's model catalogue.
pub(crate) fn model_catalogue() -> Rc<ModelCatalogue> {
    MODEL_CATALOGUE.with(Rc::clone)
}

impl ModelCatalogue {
    /// Replaces the catalogue and shows it unfiltered, so a fresh projection
    /// never leaves a stale query applied.
    pub(crate) fn set(&self, app: &MainWindow, models: Vec<crate::ModelCard>) {
        *self.0.borrow_mut() = models;
        app.set_model_catalogue_count(self.0.borrow().len() as i32);
        self.filter(app, "");
    }

    /// Case-insensitive containment over model, provider and type; an empty
    /// query keeps the whole catalogue.
    pub(crate) fn filter(&self, app: &MainWindow, query: &str) {
        let models = self.0.borrow();
        let needle = query.trim().to_lowercase();
        let rows: Vec<crate::ModelCard> = if needle.is_empty() {
            models.clone()
        } else {
            models
                .iter()
                .filter(|model| {
                    [&model.model, &model.provider, &model.model_type, &model.key]
                        .iter()
                        .any(|field| field.to_lowercase().contains(&needle))
                })
                .cloned()
                .collect()
        };
        app.set_models(model_from(rows));
    }
}

fn model_from<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(rows)))
}


fn string_model(rows: Vec<String>) -> ModelRc<slint::SharedString> {
    model_from(rows.into_iter().map(Into::into).collect())
}

pub(crate) fn run_background(task: impl FnOnce() + Send + 'static) {
    std::thread::spawn(task);
}

pub(crate) fn show_error(app: &MainWindow, error: String) {
    app.set_status(format!("错误：{error}").into());
    app.set_dialog_title("操作失败".into());
    app.set_dialog_text(error.into());
    app.set_dialog_open(true);
}

#[cfg(test)]
mod tests {
    use super::{profile_brush, profile_glyph};

    /// The backend default (`DEFAULT_AVATAR_COLOR` in `wunder-desktop`), which
    /// every unreadable preference falls back to.
    const DEFAULT_BRUSH: slint::Color = slint::Color::from_rgb_u8(0x3b, 0x82, 0xf6);

    #[test]
    fn profile_glyph_takes_the_first_name_character() {
        assert_eq!(profile_glyph("  示例用户 "), "示");
        assert_eq!(profile_glyph("ada"), "A");
        // An unknown name leaves the mark to the shared "me" label.
        assert_eq!(profile_glyph(""), "");
        assert_eq!(profile_glyph("   "), "");
    }

    #[test]
    fn profile_brush_reads_the_hex_preference() {
        assert_eq!(
            profile_brush("#6f6aed"),
            slint::Color::from_rgb_u8(0x6f, 0x6a, 0xed)
        );
        assert_eq!(profile_brush(" #3B82F6 "), DEFAULT_BRUSH);
    }

    #[test]
    fn profile_brush_keeps_the_default_for_unusable_values() {
        for value in ["", "  ", "red", "#12345", "#1234567", "#zzzzzz", "#12345g"] {
            assert_eq!(profile_brush(value), DEFAULT_BRUSH, "value {value:?}");
        }
    }
}
