use crate::file_icons::decode_png_image;
use crate::{
    ChannelAccountCard, ChannelBindingCard, ChannelCatalogCard, ChannelLogCard, MainWindow,
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use wunder_desktop::{NativeChannelAccountEdit, NativeDesktop};

/// Set when the user closes the QR overlay; the poll loop checks it between
/// wait windows and stops without leaking threads.
static QR_CANCELLED: AtomicBool = AtomicBool::new(false);

const QR_POLL_WINDOW_MS: u64 = 3_000;

/// Agent ids backing the editor ComboBox: index 0 is the default agent. Shared
/// layout with the cron editor but scoped to channels so the two pages never
/// overwrite each other's prefill.
fn agent_ids() -> &'static Mutex<Vec<String>> {
    static IDS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    IDS.get_or_init(|| Mutex::new(vec![String::new()]))
}

fn load_agent_names(app: &MainWindow, api: &Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let api = api.clone();
    std::thread::spawn(move || {
        let result = api.list_agents();
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(agents) => {
                let mut names = vec!["默认智能体".to_string()];
                let mut ids = vec![String::new()];
                for agent in agents {
                    if agent.id == "__default__" {
                        names[0] = agent.name;
                        continue;
                    }
                    ids.push(agent.id.clone());
                    names.push(agent.name);
                }
                // A new channel account preselects the current agent. This used
                // to be gated on "is the agents page open", which the two-column
                // shell removed, leaving the preselect dead.
                if app.get_channel_edit_id().is_empty() {
                    let selected = crate::expert_ui::agent_id(&app).unwrap_or_default();
                    app.set_channel_edit_agent(
                        ids.iter().position(|id| id == &selected).unwrap_or(0) as i32,
                    );
                }
                *agent_ids().lock().unwrap_or_else(|e| e.into_inner()) = ids;
                app.set_channel_agent_names(ModelRc::new(VecModel::from(
                    names
                        .into_iter()
                        .map(SharedString::from)
                        .collect::<Vec<_>>(),
                )));
            }
            Err(error) => app.set_status(format!("无法读取智能体列表：{error}").into()),
        });
    });
}

fn agent_index_for(agent_id: &str) -> usize {
    let ids = agent_ids().lock().unwrap_or_else(|e| e.into_inner());
    let target = agent_id.trim();
    if target.is_empty() || target == "__default__" || target == "default" {
        return 0;
    }
    ids.iter().position(|id| id == target).unwrap_or(0).max(1)
}

fn agent_id_for(index: usize) -> String {
    agent_ids()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(index)
        .cloned()
        .unwrap_or_default()
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let list_api = api.clone();
    app.on_refresh_channels(move || {
        let Some(app) = weak.upgrade() else { return };
        refresh_channels(&app, &list_api);
    });
    let weak = app.as_weak();
    let select_api = api.clone();
    app.on_select_channel_account(move |index| {
        let Some(app) = weak.upgrade() else { return };
        app.set_selected_channel_account(index);
        refresh_bindings(&app, &select_api);
    });
    let weak = app.as_weak();
    let creator_api = api.clone();
    app.on_open_channel_creator(move || {
        let Some(app) = weak.upgrade() else { return };
        reset_editor(&app);
        app.set_channel_edit_id("".into());
        app.set_channel_editor_open(true);
        load_agent_names(&app, &creator_api);
        load_catalog_names(&app);
    });
    let weak = app.as_weak();
    let edit_api = api.clone();
    app.on_edit_channel_account(move || {
        let Some(app) = weak.upgrade() else { return };
        let selected = app.get_selected_channel_account().max(0) as usize;
        let Some(account) = app.get_channel_accounts().iter().nth(selected) else {
            return;
        };
        reset_editor(&app);
        prefill_editor(&app, &account);
        app.set_channel_edit_id(account.account_id.clone().into());
        app.set_channel_editor_open(true);
        load_agent_names(&app, &edit_api);
        load_catalog_names(&app);
    });
    let weak = app.as_weak();
    app.on_cancel_channel_editor(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_channel_editor_open(false);
        app.set_channel_edit_app_secret("".into());
        app.set_channel_edit_secret("".into());
        app.set_channel_edit_bot_token("".into());
        app.set_channel_edit_aes_key("".into());
    });
    let weak = app.as_weak();
    let save_api = api.clone();
    app.on_save_channel_editor(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_channel_editor_saving() {
            return;
        }
        let channel_index = app.get_channel_edit_kind().max(0) as usize;
        let channel = app
            .get_channel_catalog()
            .iter()
            .nth(channel_index)
            .map(|item| item.channel.to_string())
            .unwrap_or_default();
        if channel.is_empty() {
            app.set_status("请选择渠道类型".into());
            return;
        }
        let edit = collect_edit(&app, &channel);
        app.set_channel_editor_saving(true);
        let weak = app.as_weak();
        let api = save_api.clone();
        std::thread::spawn(move || {
            let result = api.save_channel_account(&edit);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_channel_editor_saving(false);
                match result {
                    Ok(_) => {
                        app.set_channel_editor_open(false);
                        clear_secret_fields(&app);
                        refresh_channels(&app, &api);
                        app.set_status("渠道账号已保存".into());
                    }
                    Err(error) => app.set_status(format!("保存渠道账号失败：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let toggle_api = api.clone();
    app.on_toggle_channel_account(move |enabled| {
        let Some(app) = weak.upgrade() else { return };
        let Some(account) = selected_account(&app) else {
            return;
        };
        let api = toggle_api.clone();
        let weak = app.as_weak();
        let channel = account.channel.to_string();
        let account_id = account.account_id.to_string();
        std::thread::spawn(move || {
            let result = api.toggle_channel_account(&channel, &account_id, enabled);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => {
                    refresh_channels(&app, &api);
                    app.set_status(
                        if enabled {
                            "渠道账号已启用"
                        } else {
                            "渠道账号已停用"
                        }
                        .into(),
                    );
                }
                Err(error) => app.set_status(format!("无法更新渠道账号：{error}").into()),
            });
        });
    });
    let weak = app.as_weak();
    app.on_request_delete_channel(move || {
        let Some(app) = weak.upgrade() else { return };
        if selected_account(&app).is_some() {
            app.set_channel_delete_open(true);
        }
    });
    let weak = app.as_weak();
    let delete_api = api.clone();
    app.on_confirm_delete_channel(move || {
        let Some(app) = weak.upgrade() else { return };
        let Some(account) = selected_account(&app) else {
            return;
        };
        app.set_channel_delete_open(false);
        let api = delete_api.clone();
        let weak = app.as_weak();
        let channel = account.channel.to_string();
        let account_id = account.account_id.to_string();
        std::thread::spawn(move || {
            let result = api.delete_channel_account(&channel, &account_id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => {
                    app.set_selected_channel_account(-1);
                    app.set_channel_bindings(ModelRc::default());
                    refresh_channels(&app, &api);
                    app.set_status("渠道账号已删除".into());
                }
                Err(error) => app.set_status(format!("无法删除渠道账号：{error}").into()),
            });
        });
    });
    let weak = app.as_weak();
    app.on_cancel_delete_channel(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_channel_delete_open(false);
    });
    let weak = app.as_weak();
    let logs_api = api.clone();
    app.on_open_channel_logs(move || {
        let Some(app) = weak.upgrade() else { return };
        load_channel_logs(app.as_weak(), logs_api.clone());
    });
    // §9.2 渠道设置 exposes the same list three ways: open (initial read),
    // probe (re-read on demand) and clear (drop what is on screen). They share
    // one reader so the three can never disagree about which account they show.
    let weak = app.as_weak();
    let probe_api = api.clone();
    app.on_probe_channel_logs(move || {
        let Some(app) = weak.upgrade() else { return };
        load_channel_logs(app.as_weak(), probe_api.clone());
    });
    let weak = app.as_weak();
    app.on_clear_channel_logs(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_channel_logs_loading() {
            return;
        }
        app.set_channel_logs(ModelRc::default());
        app.set_status("已清空运行日志视图".into());
    });
    let weak = app.as_weak();
    let qr_api = api.clone();
    app.on_start_channel_qr(move || {
        let Some(app) = weak.upgrade() else { return };
        let account_id = qr_target_account(&app);
        app.set_channel_qr_open(true);
        app.set_channel_qr_state(0);
        app.set_channel_qr_message("".into());
        QR_CANCELLED.store(false, Ordering::SeqCst);
        let api = qr_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let started = api.start_weixin_qr_login(account_id.as_deref(), false);
            let started = match started {
                Ok(started) => started,
                Err(error) => {
                    let error = error.to_string();
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        app.set_channel_qr_state(5);
                        app.set_channel_qr_message(error.into());
                    });
                    return;
                }
            };
            let session_key = started.session_key.clone();
            // slint::Image is not Send; only the raw PNG bytes cross the
            // thread boundary and the decode happens on the UI thread.
            let png_bytes = started
                .png_data_uri
                .strip_prefix("data:image/png;base64,")
                .and_then(|encoded| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .ok()
                });
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Some(bytes) = png_bytes.as_deref().and_then(decode_png_image) {
                    app.set_channel_qr_image(bytes);
                }
                app.set_channel_qr_state(1);
            });
            poll_weixin_qr(&weak, &api, &session_key);
        });
    });
    let weak = app.as_weak();
    app.on_cancel_channel_qr(move || {
        let Some(app) = weak.upgrade() else { return };
        QR_CANCELLED.store(true, Ordering::SeqCst);
        app.set_channel_qr_open(false);
        app.set_channel_qr_state(4);
    });
    let weak = app.as_weak();
    let reconnect_api = api.clone();
    app.on_reconnect_channel(move || {
        let Some(app) = weak.upgrade() else { return };
        let Some(account) = selected_account(&app) else {
            return;
        };
        let api = reconnect_api.clone();
        let weak = app.as_weak();
        let channel = account.channel.to_string();
        let account_id = account.account_id.to_string();
        std::thread::spawn(move || {
            let result = api.reconnect_channel_account(&channel, &account_id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => app.set_status("已请求重连，稍后查看运行日志确认".into()),
                Err(error) => app.set_status(format!("重连失败：{error}").into()),
            });
        });
    });
    let weak = app.as_weak();
    let binding_api = api.clone();
    app.on_delete_channel_binding(move |peer_kind, peer_id| {
        let Some(app) = weak.upgrade() else { return };
        let Some(account) = selected_account(&app) else {
            return;
        };
        let api = binding_api.clone();
        let weak = app.as_weak();
        let channel = account.channel.to_string();
        let account_id = account.account_id.to_string();
        let peer_kind = peer_kind.to_string();
        let peer_id = peer_id.to_string();
        std::thread::spawn(move || {
            let result = api.delete_channel_binding(&channel, &account_id, &peer_kind, &peer_id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => {
                    refresh_bindings(&app, &api);
                    app.set_status("绑定已删除".into());
                }
                Err(error) => app.set_status(format!("无法删除绑定：{error}").into()),
            });
        });
    });
}

/// Reads the selected account's runtime log page into the shell. Kept as one
/// function because the open / probe / clear actions are three views of the
/// same bounded read (200 rows, newest first).
fn load_channel_logs(weak: slint::Weak<MainWindow>, api: Arc<NativeDesktop>) {
    let Some(app) = weak.upgrade() else { return };
    let Some(account) = selected_account(&app) else {
        return;
    };
    app.set_channel_logs_loading(true);
    app.set_channel_logs(ModelRc::default());
    let channel = account.channel.to_string();
    let account_id = account.account_id.to_string();
    std::thread::spawn(move || {
        let result = api.list_channel_runtime_logs(Some(&channel), Some(&account_id), 200);
        let _ = weak.upgrade_in_event_loop(move |app| {
            // A late reply for an account the user has since left is dropped
            // rather than painted over the new selection.
            if selected_account(&app).is_none_or(|a| {
                a.account_id.as_str() != account_id || a.channel.as_str() != channel
            }) {
                return;
            }
            app.set_channel_logs_loading(false);
            match result {
                Ok(entries) => {
                    app.set_channel_logs(ModelRc::new(VecModel::from(
                        entries
                            .into_iter()
                            .map(|entry| ChannelLogCard {
                                time: entry.time.into(),
                                level: entry.level.into(),
                                event: entry.event.into(),
                                message: entry.message.into(),
                            })
                            .collect::<Vec<_>>(),
                    )));
                }
                Err(error) => app.set_status(format!("无法读取运行日志：{error}").into()),
            }
        });
    });
}

fn selected_account(app: &MainWindow) -> Option<ChannelAccountCard> {
    let selected = app.get_selected_channel_account();
    if selected < 0 {
        return None;
    }
    app.get_channel_accounts().iter().nth(selected as usize)
}

fn reset_editor(app: &MainWindow) {
    app.set_channel_edit_name("".into());
    app.set_channel_edit_enabled(true);
    app.set_channel_edit_group_chat(true);
    app.set_channel_edit_app_id("".into());
    app.set_channel_edit_app_secret("".into());
    app.set_channel_edit_corp_id("".into());
    app.set_channel_edit_wechat_agent("".into());
    app.set_channel_edit_secret("".into());
    app.set_channel_edit_token("".into());
    app.set_channel_edit_aes_key("".into());
    app.set_channel_edit_original_id("".into());
    app.set_channel_edit_api_base("".into());
    app.set_channel_edit_bot_token("".into());
    app.set_channel_edit_bot_id("".into());
    app.set_channel_edit_user_id("".into());
    app.set_channel_edit_domain("".into());
    app.set_channel_edit_config_json("".into());
    app.set_channel_edit_agent(0);
}

fn clear_secret_fields(app: &MainWindow) {
    app.set_channel_edit_app_secret("".into());
    app.set_channel_edit_secret("".into());
    app.set_channel_edit_bot_token("".into());
    app.set_channel_edit_aes_key("".into());
}

fn prefill_editor(app: &MainWindow, account: &ChannelAccountCard) {
    let catalog = app.get_channel_catalog();
    let index = catalog
        .iter()
        .position(|item| item.channel == account.channel)
        .unwrap_or(0);
    app.set_channel_edit_kind(index as i32);
    app.set_channel_edit_name(account.name.clone().into());
    app.set_channel_edit_enabled(account.active);
    app.set_channel_edit_group_chat(account.peer_kind != "user");
    app.set_channel_edit_app_id(account.app_id.clone().into());
    app.set_channel_edit_corp_id(account.corp_id.clone().into());
    app.set_channel_edit_wechat_agent(account.wechat_agent_id.clone().into());
    app.set_channel_edit_original_id(account.original_id.clone().into());
    app.set_channel_edit_api_base("".into());
    app.set_channel_edit_bot_id(account.weixin_bot_id.clone().into());
    app.set_channel_edit_user_id(account.weixin_user_id.clone().into());
    app.set_channel_edit_domain(account.domain.clone().into());
    app.set_channel_edit_agent(agent_index_for(&account.agent_id) as i32);
}

fn load_catalog_names(app: &MainWindow) {
    let names: Vec<SharedString> = app
        .get_channel_catalog()
        .iter()
        .map(|item| item.name.clone())
        .collect();
    app.set_channel_catalog_names(ModelRc::new(VecModel::from(names)));
}

fn collect_edit(app: &MainWindow, channel: &str) -> NativeChannelAccountEdit {
    NativeChannelAccountEdit {
        account_id: app.get_channel_edit_id().trim().to_string(),
        channel: channel.to_string(),
        account_name: app.get_channel_edit_name().trim().to_string(),
        agent_id: agent_id_for(app.get_channel_edit_agent().max(0) as usize),
        enabled: app.get_channel_edit_enabled(),
        receive_group_chat: (channel == "feishu").then(|| app.get_channel_edit_group_chat()),
        app_id: app.get_channel_edit_app_id().trim().to_string(),
        app_secret: app.get_channel_edit_app_secret().trim().to_string(),
        corp_id: app.get_channel_edit_corp_id().trim().to_string(),
        wechat_agent_id: app.get_channel_edit_wechat_agent().trim().to_string(),
        wechat_secret: app.get_channel_edit_secret().trim().to_string(),
        wechat_token: app.get_channel_edit_token().trim().to_string(),
        wechat_aes_key: app.get_channel_edit_aes_key().trim().to_string(),
        wechat_mp_token: app.get_channel_edit_token().trim().to_string(),
        wechat_mp_aes_key: app.get_channel_edit_aes_key().trim().to_string(),
        wechat_mp_original_id: app.get_channel_edit_original_id().trim().to_string(),
        weixin_api_base: app.get_channel_edit_api_base().trim().to_string(),
        weixin_bot_token: app.get_channel_edit_bot_token().trim().to_string(),
        weixin_bot_id: app.get_channel_edit_bot_id().trim().to_string(),
        weixin_user_id: app.get_channel_edit_user_id().trim().to_string(),
        domain: app.get_channel_edit_domain().trim().to_string(),
        config_json: app.get_channel_edit_config_json().trim().to_string(),
    }
}

fn refresh_channels(app: &MainWindow, api: &Arc<NativeDesktop>) {
    if app.get_channel_loading() {
        return;
    }
    app.set_channel_loading(true);
    let weak = app.as_weak();
    let api = api.clone();
    std::thread::spawn(move || {
        let result = api.list_channel_accounts();
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_channel_loading(false);
            match result {
                Ok(listing) => {
                    app.set_channel_catalog(ModelRc::new(VecModel::from(
                        listing
                            .catalog
                            .into_iter()
                            .map(|item| ChannelCatalogCard {
                                channel: item.channel.into(),
                                name: item.name.into(),
                                description: item.description.into(),
                            })
                            .collect::<Vec<_>>(),
                    )));
                    app.set_channel_accounts(ModelRc::new(VecModel::from(
                        listing
                            .items
                            .into_iter()
                            .map(|item| ChannelAccountCard {
                                channel: item.channel.into(),
                                account_id: item.account_id.into(),
                                name: item.name.into(),
                                status: item.status.into(),
                                active: item.active,
                                configured: item.configured,
                                peer_kind: item.peer_kind.into(),
                                agent_id: item.agent_id.into(),
                                created_at: item.created_at.into(),
                                updated_at: item.updated_at.into(),
                                summary: item.summary.into(),
                                secret_set: item.secret_set,
                                app_id: item.app_id.into(),
                                corp_id: item.corp_id.into(),
                                wechat_agent_id: item.wechat_agent_id.into(),
                                original_id: item.original_id.into(),
                                weixin_bot_id: item.weixin_bot_id.into(),
                                weixin_user_id: item.weixin_user_id.into(),
                                bot_type: item.bot_type.into(),
                                domain: item.domain.into(),
                            })
                            .collect::<Vec<_>>(),
                    )));
                    load_catalog_names(&app);
                    if app.get_channel_accounts().row_count() > 0 {
                        app.invoke_select_channel_account(0);
                    } else {
                        app.set_selected_channel_account(-1);
                    }
                }
                Err(error) => app.set_status(format!("无法读取渠道账号：{error}").into()),
            }
        });
    });
}

fn refresh_bindings(app: &MainWindow, api: &Arc<NativeDesktop>) {
    let Some(account) = selected_account(app) else {
        app.set_channel_bindings(ModelRc::default());
        return;
    };
    app.set_channel_bindings_loading(true);
    let weak = app.as_weak();
    let api = api.clone();
    let channel = account.channel.to_string();
    let account_id = account.account_id.to_string();
    std::thread::spawn(move || {
        let result = api.list_channel_bindings(Some(&channel), Some(&account_id));
        let _ = weak.upgrade_in_event_loop(move |app| {
            if selected_account(&app).is_none_or(|a| {
                a.account_id.as_str() != account_id || a.channel.as_str() != channel
            }) {
                return;
            }
            app.set_channel_bindings_loading(false);
            match result {
                Ok(bindings) => {
                    app.set_channel_bindings(ModelRc::new(VecModel::from(
                        bindings
                            .into_iter()
                            .map(|binding| ChannelBindingCard {
                                binding_id: binding.binding_id.into(),
                                peer_kind: binding.peer_kind.into(),
                                peer_id: binding.peer_id.into(),
                                agent_id: binding.agent_id.into(),
                                enabled: binding.enabled,
                            })
                            .collect::<Vec<_>>(),
                    )));
                }
                Err(error) => app.set_status(format!("无法读取绑定：{error}").into()),
            }
        });
    });
}

/// Empty while creating a new login; the editor account id wins when the QR
/// flow was started from the editor, otherwise the selected weixin account.
fn qr_target_account(app: &MainWindow) -> Option<String> {
    let editor_id = app.get_channel_edit_id().trim().to_string();
    if !editor_id.is_empty() {
        return Some(editor_id);
    }
    selected_account(app)
        .filter(|account| account.channel == "weixin")
        .map(|account| account.account_id.to_string())
}

/// Loops short wait windows until confirmed, expired, failed or cancelled.
/// Cancellation takes effect at the next poll boundary.
fn poll_weixin_qr(weak: &slint::Weak<MainWindow>, api: &Arc<NativeDesktop>, session_key: &str) {
    loop {
        if QR_CANCELLED.load(Ordering::SeqCst) {
            return;
        }
        match api.wait_weixin_qr_login(session_key, QR_POLL_WINDOW_MS) {
            Ok(status) if status.connected => {
                let confirmed = status;
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if !confirmed.bot_token.is_empty() {
                        app.set_channel_edit_bot_token(confirmed.bot_token.into());
                    }
                    if !confirmed.ilink_bot_id.is_empty() {
                        app.set_channel_edit_bot_id(confirmed.ilink_bot_id.into());
                    }
                    if !confirmed.ilink_user_id.is_empty() {
                        app.set_channel_edit_user_id(confirmed.ilink_user_id.into());
                    }
                    if !confirmed.api_base.is_empty() {
                        app.set_channel_edit_api_base(confirmed.api_base.into());
                    }
                    if app.get_channel_edit_name().trim().is_empty() {
                        app.set_channel_edit_name("微信客服".into());
                    }
                    app.set_channel_edit_enabled(true);
                    app.set_channel_qr_state(2);
                    app.set_channel_qr_open(false);
                    app.set_channel_editor_open(true);
                });
                return;
            }
            Ok(status) if status.status == "expired" => {
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.set_channel_qr_state(3);
                });
                return;
            }
            Ok(_) => continue,
            Err(error) => {
                let text = error.to_string();
                let expired = text.contains("expired");
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if expired {
                        app.set_channel_qr_state(3);
                    } else {
                        app.set_channel_qr_state(5);
                        app.set_channel_qr_message(text.into());
                    }
                });
                return;
            }
        }
    }
}
