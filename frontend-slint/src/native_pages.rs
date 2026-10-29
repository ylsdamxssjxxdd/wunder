//! Native entity-page callbacks. All storage and configuration work runs off the UI thread.
use crate::{AgentCard, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{rc::Rc, sync::Arc};
use wunder_desktop::{
    AgentRecord, CloudStatusView, DesktopSettings, NativeDesktop, NativeProfile, RuntimeToolStatus,
};

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    crate::agent_editor::install(app);
    crate::agent_editor::install_save_handler(app, api.clone());
    crate::entity_state::bind_selection(app);
    crate::avatar_ui::install(app, api.clone());
    crate::pet_settings::install(app, api.clone());
    bind_agents(app, api.clone());
    bind_model_probes(app, api.clone());
    bind_preferences_and_prompts(app, api.clone());
    crate::tools_ui::install(app, api.clone());
    bind_settings(app, api.clone());
    bind_cloud(app, api.clone());
    bind_profile(app, api.clone());
    crate::native_interlink::install(app, api.clone());
    crate::cloud_threads::install(app, api.clone());
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
        let known_key = !app.get_model_creating()
            && !key.is_empty()
            && app.get_models().iter().any(|entry| entry.key == key);
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
                    show_toast(&app, outcome.message, 1);
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
        let known_key = !app.get_model_creating()
            && !key.is_empty()
            && app.get_models().iter().any(|entry| entry.key == key);
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
                        show_toast(&app, "提供方未返回语音列表。".into(), 1);
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
                        show_toast(&app, format!("{preview}{more}"), 1);
                    }
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
                            packs
                                .iter()
                                .map(|pack| pack.id.clone().into())
                                .collect::<Vec<_>>(),
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
}
/// Project the façade's `CloudStatusView` onto the shell properties the
/// settings card and the sidebar quota badge bind to. Signed-in drafts show
/// the session values; signed-out drafts keep whatever the user typed.
fn push_cloud_status(app: &MainWindow, status: CloudStatusView) {
    let CloudStatusView {
        inbound_enabled,
        full_shadow,
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
    let was_logged_in = app.get_cloud_logged_in();
    crate::cloud_threads::account_status(app, logged_in, &server, &username);
    app.set_cloud_logged_in(logged_in);
    app.set_cloud_inbound_enabled(inbound_enabled);
    app.set_cloud_full_shadow(full_shadow);
    // The sidebar pins a "cloud workspace" row while signed in; rebuild it on
    // the sign-in flip so the row appears/disappears with the account.
    if was_logged_in != logged_in {
        crate::navigation_ui::project_sidebar(app, None);
    }
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

// One bounded background read keeps the settings card and sidebar synchronized
// even when settings are closed. No network request is issued by cloud_status.
thread_local! {
    static CLOUD_POLL_TIMER: slint::Timer = slint::Timer::default();
}

fn restart_cloud_status_poll(app_weak: slint::Weak<MainWindow>, api: Arc<NativeDesktop>) {
    let in_flight = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // `cloud_status` is intentionally a local, lock-free projection so the
    // sidebar remains responsive while offline.  Every fifth tick performs a
    // bounded account fetch as well: the web client reads the authoritative
    // server snapshot, while the desktop otherwise only has its session-file
    // cache.  Without this calibration another client can spend quota and the
    // desktop badge would stay stale until the next sign-in.
    // Start with a refresh rather than waiting for the first five lightweight
    // ticks.  This makes a restored desktop converge with the web account
    // view as soon as the shell is ready, while subsequent refreshes remain
    // rate-limited to one request per fifty seconds.
    let refresh_tick = Arc::new(std::sync::atomic::AtomicU8::new(0));
    CLOUD_POLL_TIMER.with(|timer| {
        timer.start(
            slint::TimerMode::Repeated,
            // Account state is cached locally and changes are applied directly
            // after login/logout.  This is only a reconnect/backstop read;
            // polling it every two seconds kept the idle desktop waking even
            // when no cloud account was configured.
            std::time::Duration::from_secs(10),
            move || {
                let Some(app) = app_weak.upgrade() else {
                    return;
                };
                if app.get_cloud_busy() || in_flight.swap(true, std::sync::atomic::Ordering::AcqRel)
                {
                    return;
                }
                let weak = app_weak.clone();
                let api = api.clone();
                let pending = in_flight.clone();
                let refresh_account =
                    refresh_tick.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 5 == 0;
                run_background(move || {
                    let status = if refresh_account {
                        // Keep projecting the cached state when the network is
                        // unavailable.  A 401 still marks the service
                        // expired, so the fallback status makes that state
                        // visible without replacing the balance with zero.
                        match api.cloud_refresh_account() {
                            Ok(status) => Ok(status),
                            Err(_) => api.cloud_status(),
                        }
                    } else {
                        api.cloud_status()
                    };
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if !app.get_cloud_busy() {
                            if let Ok(status) = status {
                                push_cloud_status(&app, status);
                            }
                        }
                        pending.store(false, std::sync::atomic::Ordering::Release);
                    });
                });
            },
        );
    });
}

/// Cloud account façade wiring (§6.1): login/logout/refresh/preference-sync
/// all run on background threads through the `NativeDesktop` façade, then
/// land their result back on the UI thread. The password draft only travels
/// from the shell into the call; it is never logged nor persisted.
fn bind_cloud(app: &MainWindow, api: Arc<NativeDesktop>) {
    restart_cloud_status_poll(app.as_weak(), api.clone());
    let weak = app.as_weak();
    let browser_api = api.clone();
    let opening = Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.on_open_wunder_model(move || {
        let Some(app) = weak.upgrade() else { return };
        if opening.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        let server = app.get_cloud_server_draft().to_string();
        let api = browser_api.clone();
        let opening = opening.clone();
        let weak = app.as_weak();
        run_background(move || {
            let result = api.open_bridge_model(&server);
            opening.store(false, std::sync::atomic::Ordering::Release);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => app.set_status("已在系统浏览器打开眺望心舰".into()),
                Err(error) => show_error(&app, error.to_string()),
            });
        });
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_toggle_sharing(move |enabled, full| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_busy() {
            return;
        }
        app.set_cloud_busy(true);
        let weak = app.as_weak();
        let backend = backend.clone();
        std::thread::spawn(move || {
            let result = backend.cloud_set_local_sharing(enabled, full);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_cloud_busy(false);
                match result {
                    Ok(status) => push_cloud_status(&app, status),
                    Err(error) => app.set_cloud_error(error.to_string().into()),
                }
            });
        });
    });
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
        if let Err(error) = wunder_desktop::validate_cloud_password(&password) {
            app.set_cloud_error(error.to_string().into());
            app.set_status("密码只能使用 ASCII 字符，请勿输入中文或控制字符".into());
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
    app.on_cloud_password_edited(move |value| {
        let Some(app) = weak.upgrade() else { return };
        let filtered: String = value
            .chars()
            .filter(|ch| ch.is_ascii() && !ch.is_control())
            .collect();
        if value.as_str() != filtered {
            app.set_cloud_pass_draft(filtered.into());
            app.set_cloud_error("密码只能使用 ASCII 字符".into());
        }
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
                        app.set_status(if enabled {
                            "已开启基础设置同步".into()
                        } else {
                            "已关闭基础设置同步".into()
                        });
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
            return slint::Color::from_rgb_u8((value >> 16) as u8, (value >> 8) as u8, value as u8);
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
        app.invoke_queue_agent_save();
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
            app.invoke_queue_agent_save();
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
                app.invoke_queue_agent_save();
            }
        }
    });
    // §9.3 agent form: the tool switches hand back a tool name, and the
    // settings search box re-projects the tool list because Slint 1.18 strings
    // have no substring test.
    let weak = app.as_weak();
    app.on_toggle_agent_tool_option(move |name| {
        let Some(app) = weak.upgrade() else { return };
        // The toggle owns the selection set; `sync_tool_selection` re-projects
        // the card picker's enabled flags from it.
        app.invoke_toggle_agent_tool(name);
        app.invoke_refresh_agent_dirty();
        app.invoke_queue_agent_save();
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
    // 线程渲染压测：解析规模和个数后由 facade 顺序写入。每个完整线程提交后才会
    // 出现在当前工作区，进度更新留在工作线程之外以保持 Slint 流畅。
    let weak = app.as_weak();
    app.on_create_model(move || {
        let Some(app) = weak.upgrade() else { return };
        if crate::model_editor::defer_creation(&app) {
            return;
        }
        if app.get_saving() || app.get_settings_loading() {
            return;
        }
        // Only one unsaved draft exists; "添加模型" while the draft is on screen
        // (or parked behind another selection) re-opens it instead of stacking a
        // second "新模型" row.
        if let Some(row_key) = crate::entity_state::active_model_draft_key(&app) {
            if let Some(index) = app
                .get_models()
                .iter()
                .position(|entry| entry.key == row_key.as_str())
            {
                app.set_selected_model(index as i32);
                if crate::entity_state::restore_model_draft(&app, index as i32, &row_key) {
                    app.set_status("已回到未保存的模型草稿".into());
                } else {
                    // Already editing this row: keep the live drafts untouched.
                    app.set_status("当前正在编辑未保存的模型草稿".into());
                }
                return;
            }
        }
        let mut rows = app.get_models().iter().collect::<Vec<_>>();
        let mut key = "新模型".to_string();
        let mut suffix = 1usize;
        while rows.iter().any(|entry| entry.key == key) {
            key = format!("新模型{suffix}");
            suffix += 1;
        }
        let entry = crate::entity_state::new_model_card(key);
        rows.insert(0, entry.clone());
        model_catalogue().set(&app, rows);
        app.set_selected_model(0);
        crate::entity_state::apply_new_model_draft(&app, entry);
        // 新建草稿不自动保存：填完基础设置后由表单上的「保存」提交。
        app.set_status("已创建模型草稿，填写基础设置后保存".into());
    });
    let weak = app.as_weak();
    app.on_discard_model_draft(move || {
        let Some(app) = weak.upgrade() else { return };
        let stashed_key = crate::entity_state::model_draft_row_key();
        if !app.get_model_creating() && stashed_key.is_none() {
            return;
        }
        let selected_key = usize::try_from(app.get_selected_model())
            .ok()
            .and_then(|index| app.get_models().row_data(index))
            .map(|row| row.key.to_string());
        let row_key = if app.get_model_creating() {
            selected_key.clone()
        } else {
            stashed_key.clone()
        };
        crate::entity_state::clear_model_draft();
        if let Some(row_key) = row_key {
            let rows = app
                .get_models()
                .iter()
                .filter(|entry| entry.key != row_key.as_str())
                .collect::<Vec<_>>();
            model_catalogue().set(&app, rows);
        }
        if app.get_model_creating() {
            app.set_model_creating(false);
            crate::entity_state::restore_model(&app, "");
        }
    });
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
        let Ok(count) = app.get_stress_count().trim().parse::<i64>() else {
            app.set_stress_status("⚠ 个数必须是正整数".into());
            return;
        };
        if count <= 0 {
            app.set_stress_status("⚠ 个数必须是正整数".into());
            return;
        }
        let workspace_id = usize::try_from(app.get_selected_workspace())
            .ok()
            .and_then(|index| app.get_workspaces().row_data(index))
            .map(|workspace| workspace.workspace_id.to_string());
        app.set_stress_running(true);
        app.set_stress_status(format!("正在生成 {count} 个 {user_rounds}×{model_rounds} 压测线程…").into());
        let weak = app.as_weak();
        let api = stress_api.clone();
        std::thread::spawn(move || {
            let total = user_rounds;
            let progress_app = weak.clone();
            let mut last_report = std::time::Instant::now();
            let result = api.generate_stress_threads(
                user_rounds,
                model_rounds,
                count,
                workspace_id.as_deref(),
                move |current, done, items| {
                    // 节流到约每 300ms 一次；批量写入在两次报告之间全速进行。
                    let now = std::time::Instant::now();
                    if done >= total || now.duration_since(last_report).as_millis() >= 300 {
                        last_report = now;
                        let weak = progress_app.clone();
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            app.set_stress_status(
                                format!(
                                    "正在生成第 {current}/{count} 个… 已完成 {done}/{total} 轮次，写入 {items} 条消息"
                                )
                                .into(),
                            );
                        });
                    }
                },
            );
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_stress_running(false);
                match result {
                    Ok(stats) => {
                        let user_turns = stats.iter().map(|entry| entry.user_turns).sum::<i64>();
                        let items = stats.iter().map(|entry| entry.items).sum::<i64>();
                        let tool_calls = stats.iter().map(|entry| entry.tool_calls).sum::<i64>();
                        let text = format!(
                            "已生成 {} 个线程 / {user_turns} 轮次 / {items} 条消息 / {tool_calls} 次工具调用",
                            stats.len(),
                        );
                        app.set_stress_status(text.clone().into());
                        app.set_dialog_title("压测线程已生成".into());
                        app.set_dialog_text(
                            format!(
                                "{text}\n会话 ID：{}",
                                stats
                                    .iter()
                                    .map(|entry| entry.session_id.as_str())
                                    .collect::<Vec<_>>()
                                    .join("、")
                            )
                            .into(),
                        );
                        app.set_dialog_open(true);
                        app.invoke_refresh_chat();
                        // A catalogue read can already be in flight when the
                        // final bulk transaction makes these sessions visible.
                        // Schedule one bounded follow-up after it has settled
                        // so a generated thread appears in the current work
                        // area without requiring manual navigation.
                        let refresh = app.as_weak();
                        slint::Timer::single_shot(std::time::Duration::from_millis(350), move || {
                            if let Some(app) = refresh.upgrade() {
                                app.invoke_refresh_chat();
                            }
                        });
                    }
                    Err(error) => {
                        app.set_stress_status(format!("⚠ 生成失败：{error}").into());
                    }
                }
            });
        });
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
        if crate::model_editor::defer_refresh(&app) {
            return;
        }
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
                match result {
                    Ok(settings) => {
                        apply_settings(&app, settings);
                    }
                    Err(error) => show_error(&app, format!("无法读取本地设置：{error}")),
                }
                match cloud {
                    Ok(status) => push_cloud_status(&app, status),
                    Err(_) => {}
                }
                app.set_settings_loading(false);
            });
        });
    });
    crate::model_editor::install(app, api.clone());
    let weak = app.as_weak();
    let delete_api = api.clone();
    app.on_delete_model(move |key| {
        let Some(app) = weak.upgrade() else { return };
        if crate::model_editor::defer_delete(&app, key.to_string()) {
            return;
        }
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
        if crate::model_editor::defer_default(&app, key.to_string()) {
            return;
        }
        if app.get_saving() || app.get_settings_loading() || app.get_agents_loading() {
            return;
        }
        // Optimistic switch: project the picked card into the composer label in
        // the same frame; persistence races behind and reverts on failure.
        let previous_key = app.get_settings_model_key().to_string();
        crate::entity_state::restore_model(&app, &key);
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
                        crate::entity_state::restore_model(&app, &key);
                        app.set_status("默认模型已更新".into());
                        app.invoke_refresh_agents();
                    }
                    Err(error) => {
                        crate::entity_state::restore_model(&app, &previous_key);
                        show_error(&app, format!("无法更新默认模型：{error}"));
                    }
                }
            });
        });
    });
    let weak = app.as_weak();
    let order_api = api.clone();
    app.on_move_model(move |from, delta| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_model_creating() {
            return;
        }
        let Ok(from) = usize::try_from(from) else {
            return;
        };
        let mut rows = app.get_models().iter().collect::<Vec<_>>();
        if from >= rows.len() || rows.is_empty() {
            return;
        }
        let to = (from as i64 + i64::from(delta)).clamp(0, rows.len() as i64 - 1) as usize;
        if to == from {
            return;
        }
        let row = rows.remove(from);
        rows.insert(to, row);
        // The highlight follows the model, not the slot it occupied.
        let selected_key = app.get_settings_model_key().to_string();
        let selected = rows
            .iter()
            .position(|entry| entry.key == selected_key)
            .map(|index| index as i32)
            .unwrap_or(-1);
        app.set_selected_model(selected);
        let keys: Vec<String> = rows.iter().map(|entry| entry.key.to_string()).collect();
        model_catalogue().set(&app, rows);
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
    let projecting = app.get_model_projecting();
    app.set_model_projecting(true);
    let selected = app.get_settings_model_key();
    app.set_workspace_root(settings.workspace_root.into());
    app.set_runtime_language(settings.language.into());
    // 帮助窗口独立持有一份手册语言，跟随这里的界面语言变化。
    crate::help_browser::sync_language(&app);
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
    // The catalogue object projects the composer's full model list and mirrors
    // its size for the picker's "empty list" versus "nothing configured" split.
    app.set_virtual_replay_logs(string_model(settings.virtual_replay_logs));
    model_catalogue().set(
        app,
        settings.models.into_iter().map(to_model_card).collect(),
    );
    if !crate::model_editor::has_unsaved(app)
        || (!app.get_model_creating()
            && !app.get_models().iter().any(|model| model.key == selected))
    {
        crate::entity_state::restore_model(app, &selected);
    }
    let current = app.get_selected_model_key();
    let inherited = app.get_selected_agent_model();
    let models = app.get_composer_models();
    let fallback = models
        .iter()
        .find(|model| model.is_default)
        .or_else(|| models.iter().next());
    if current.is_empty() {
        if !models.iter().any(|model| model.key == inherited) {
            app.set_selected_agent_model(fallback.map(|model| model.key).unwrap_or_default());
        }
    } else if models.iter().any(|model| model.key == current) {
        select_model_key(app, &current);
    } else {
        select_model_key(
            app,
            fallback.map(|model| model.key).unwrap_or_default().as_str(),
        );
    }
    app.set_model_projecting(projecting);
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
        disabled: !model.enabled,
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
        history_compaction_ratio: model.history_compaction_ratio.into(),
        reasoning_effort: model.reasoning_effort.into(),
        simulation_speed: model.simulation_speed.into(),
        support_vision: model.support_vision,
        support_hearing: model.support_hearing,
        stream_include_usage: model.stream_include_usage,
        tool_call_mode: model.tool_call_mode.into(),
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
        video_num_frames: model.video_num_frames.into(),
        video_negative_prompt: model.video_negative_prompt.into(),
        video_steps: model.video_steps.into(),
        video_guidance_scale: model.video_guidance_scale.into(),
        video_guidance_scale_2: model.video_guidance_scale_2.into(),
        video_boundary_ratio: model.video_boundary_ratio.into(),
        video_flow_shift: model.video_flow_shift.into(),
        video_frame_interpolation: model.video_frame_interpolation,
        video_sync_mode: model.video_sync_mode,
    }
}

/// Map a persisted theme value onto a `Theme.palette` name. Only the accent
/// family is selectable: the settings card offers terracotta (橙汁),
/// hula-green (薄荷), forest (森林) and honey (蜜蜂); the legacy "light"/
/// "eva-orange" values, and anything unrecognised, resolve to the 橙汁
/// default. 橙汁 的色值以本文件的 design_tokens.slint 为准。
fn normalize_palette(theme: &str) -> &'static str {
    match theme.trim() {
        "hula-green" => "hula-green",
        "forest" => "forest",
        "honey" => "honey",
        "minimal" => "minimal",
        "tech-blue" => "tech-blue",
        _ => "terracotta",
    }
}

/// User-facing one-line status for a configured runtime tool path. A warning
/// mark keeps an invalid configured path visible instead of silently falling
/// back to the system interpreter.
fn tool_status_text(status: &RuntimeToolStatus) -> String {
    match status.source.as_str() {
        "custom" => "✓ 自定义路径已生效".to_string(),
        "embedded" => "✓ 内置运行时已就绪".to_string(),
        "invalid" => "⚠ 配置路径无效，已回退系统环境".to_string(),
        "system" => "✓ 使用系统 PATH".to_string(),
        _ => "⚠ 未找到可用的 Python 运行时，需要配置或安装补充包".to_string(),
    }
}

pub(crate) fn select_model_key(app: &MainWindow, key: &str) {
    let models = app.get_composer_models();
    let model = models
        .iter()
        .find(|model| model.key == key)
        .unwrap_or_default();
    // This helper is used by the composer and session restore. Keep the
    // composer projection updated without routing through the settings
    // editor selection callback.
    app.set_selected_model_key(model.key);
    app.set_selected_model_provider(model.provider);
    app.set_selected_model_name(model.model);
    app.set_selected_model_base_url(model.base_url);
    app.set_selected_model_type(model.model_type);
    app.set_selected_model_is_default(model.is_default);
}

/// The model catalogue, kept natively so the settings projection and the
/// composer's picker always render the same list from one source.
pub(crate) struct ModelCatalogue;

thread_local! {
    /// One catalogue per UI thread. The native shell owns exactly one
    /// `MainWindow` on this thread, and `apply_settings` is reached from three
    /// modules that all already share that window; a thread-local keeps the
    /// list beside it instead of threading a handle through every settings
    /// callback.
    static MODEL_CATALOGUE: Rc<ModelCatalogue> = Rc::new(ModelCatalogue);
}

/// The UI thread's model catalogue.
pub(crate) fn model_catalogue() -> Rc<ModelCatalogue> {
    MODEL_CATALOGUE.with(Rc::clone)
}

impl ModelCatalogue {
    /// Replaces the catalogue and projects the composer's full view. The
    /// unsaved new-model row is re-inserted so a settings refresh cannot
    /// silently drop it; callers that mean to discard it clear the draft
    /// before rebuilding.
    pub(crate) fn set(&self, app: &MainWindow, models: Vec<crate::ModelCard>) {
        let mut rows = models;
        let active = app
            .get_model_creating()
            .then(|| app.get_selected_model_card());
        if let Some(draft) = crate::entity_state::model_draft_row().or(active) {
            if !rows.iter().any(|entry| entry.key == draft.key) {
                rows.insert(0, draft);
            }
        }
        let draft_key = crate::entity_state::active_model_draft_key(app);
        let selectable = rows
            .iter()
            .filter(|model| {
                // Only the type, the disabled flag and an unsaved draft keep a
                // row out of the picker. A blank model name is still a usable
                // chat entry — the seeded 虚拟模型 carries no name and the
                // runtime resolves it from the provider.
                model.model_type.eq_ignore_ascii_case("llm")
                    && !model.disabled
                    && draft_key.as_deref() != Some(model.key.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        app.set_model_catalogue_count(selectable.len() as i32);
        app.set_composer_models(model_from(selectable));
        app.set_models(model_from(rows));
    }
}

pub(crate) fn model_from<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
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
}

/// Show short-lived feedback through the shell-owned toast layer. Unlike the
/// legacy info dialog this stays above the settings overlay and does not block
/// the editor while a probe result is being read.
pub(crate) fn show_toast(app: &MainWindow, message: String, kind: i32) {
    let toast = app.global::<crate::ToastState>();
    toast.set_message(message.into());
    toast.set_kind(kind);
    toast.set_revision(toast.get_revision().wrapping_add(1));
}

#[cfg(test)]
mod tests {
    use super::{normalize_palette, profile_brush, profile_glyph};

    /// The backend default (`DEFAULT_AVATAR_COLOR` in `wunder-desktop`), which
    /// every unreadable preference falls back to.
    const DEFAULT_BRUSH: slint::Color = slint::Color::from_rgb_u8(0x3b, 0x82, 0xf6);

    #[test]
    fn composer_catalogue_includes_nondefault_llms_and_excludes_drafts_and_disabled_models() {
        use slint::Model;
        let _ui = crate::test_support::lock();
        crate::register_bundled_font().expect("bundled font");
        let app = crate::MainWindow::new().expect("window");
        let card = |key: &str| crate::ModelCard {
            key: key.into(),
            model: "sample-model".into(),
            model_type: "llm".into(),
            ..Default::default()
        };
        let mut disabled = card("disabled-model");
        disabled.disabled = true;
        let mut embedding = card("embedding-model");
        embedding.model_type = "embedding".into();
        // The seeded 虚拟模型 has no model name; it is still a chat entry.
        let mut nameless = card("虚拟模型");
        nameless.model = "".into();
        let draft = card("draft-model");
        app.set_model_creating(true);
        app.set_selected_model_card(draft.clone());
        app.set_selected_model(0);
        app.set_models(super::model_from(vec![draft]));
        super::model_catalogue().set(
            &app,
            vec![card("extra-model"), disabled, embedding, nameless],
        );
        assert_eq!(app.get_composer_models().row_count(), 2);
        assert_eq!(
            app.get_composer_models().row_data(0).unwrap().key,
            "extra-model"
        );
        assert_eq!(
            app.get_composer_models().row_data(1).unwrap().key,
            "虚拟模型",
            "a blank model name does not hide the entry"
        );
        assert_eq!(app.get_model_catalogue_count(), 2);
        assert_eq!(app.get_models().row_count(), 5);
        super::select_model_key(&app, "extra-model");
        assert_eq!(app.get_selected_model_key(), "extra-model");
        super::select_model_key(&app, "missing-model");
        assert!(app.get_selected_model_key().is_empty());
    }

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

    /// Every palette id the settings card writes must survive the restart
    /// round trip; one that `normalize_palette` drops silently comes back
    /// terracotta.
    #[test]
    fn the_four_selectable_palettes_survive_a_restart() {
        for palette in ["terracotta", "hula-green", "forest", "honey"] {
            assert_eq!(normalize_palette(palette), palette);
            assert_eq!(normalize_palette(&format!(" {palette} ")), palette);
        }
        assert_eq!(normalize_palette("light"), "terracotta");
        assert_eq!(normalize_palette("nonsense"), "terracotta");
    }
}
