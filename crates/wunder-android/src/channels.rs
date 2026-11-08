//! 安卓版 channel settings projection: accounts, bindings and runtime logs.
//! Secrets only travel into `save`; every projection stays secret-free.

use crate::engine::Shared;
use std::sync::{Arc, Mutex};
use wunder_desktop::native::{ChannelAccountCard, NativeChannelAccountEdit};
use wunder_desktop::NativeDesktop;

#[derive(Clone, Default)]
pub struct MobileChannelAccount {
    pub channel: String,
    pub channel_name: String,
    pub account_id: String,
    pub name: String,
    pub status: String,
    pub active: bool,
    pub configured: bool,
    pub summary: String,
    pub secret_set: bool,
    pub app_id: String,
    pub corp_id: String,
    pub wechat_agent_id: String,
    pub original_id: String,
    pub weixin_bot_id: String,
    pub weixin_user_id: String,
    pub bot_type: String,
    pub domain: String,
}

#[derive(Clone, Default)]
pub struct MobileChannelCatalog {
    pub channel: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Default)]
pub struct MobileChannelBinding {
    pub binding_id: String,
    pub channel: String,
    pub account_id: String,
    pub peer_kind: String,
    pub peer_id: String,
    pub agent_id: String,
    pub enabled: bool,
}

#[derive(Clone, Default)]
pub struct MobileChannelLog {
    pub time: String,
    pub level: String,
    pub event: String,
    pub message: String,
}

#[derive(Clone, Default)]
pub struct ChannelsView {
    pub accounts: Vec<MobileChannelAccount>,
    pub catalog: Vec<MobileChannelCatalog>,
    pub bindings: Vec<MobileChannelBinding>,
    pub logs: Vec<MobileChannelLog>,
    pub selected: String,
    pub logs_open: bool,
    pub logs_account: String,
    pub loading: bool,
    pub error: String,
}

impl ChannelsView {
    pub fn channel_name(&self, channel: &str) -> String {
        self.catalog
            .iter()
            .find(|item| item.channel == channel)
            .map(|item| item.name.clone())
            .unwrap_or_else(|| channel.to_string())
    }
}

#[derive(Clone, Default)]
pub struct ChannelEditorView {
    pub is_new: bool,
    pub draft: crate::MobileChannelDraft,
    pub error: String,
}

pub fn refresh(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    match desktop.list_channel_accounts() {
        Ok(listing) => {
            let catalog = listing
                .catalog
                .into_iter()
                .map(|item| MobileChannelCatalog {
                    channel: item.channel,
                    name: item.name,
                    description: item.description,
                })
                .collect::<Vec<_>>();
            let accounts = listing
                .items
                .into_iter()
                .map(|card| account_from(card, &catalog))
                .collect::<Vec<_>>();
            let selected = shared
                .lock()
                .map(|state| state.channels.selected.clone())
                .unwrap_or_default();
            let selected = if accounts.iter().any(|account| account.account_id == selected) {
                selected
            } else {
                String::new()
            };
            if let Ok(mut state) = shared.lock() {
                state.channels.accounts = accounts;
                state.channels.catalog = catalog;
                state.channels.selected = selected.clone();
                if selected.is_empty() {
                    state.channels.bindings.clear();
                }
                state.channels.loading = false;
                state.channels.error.clear();
                state.channels_rev = state.channels_rev.wrapping_add(1);
                state.touch();
            }
            if !selected.is_empty() {
                select(desktop, shared, &selected);
            }
        }
        Err(error) => set_error(shared, format!("无法读取渠道列表：{error}")),
    }
}

pub fn select(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, account_id: &str) {
    let (channel, name) = shared
        .lock()
        .ok()
        .and_then(|state| {
            state
                .channels
                .accounts
                .iter()
                .find(|account| account.account_id == account_id)
                .map(|account| (account.channel.clone(), account.account_id.clone()))
        })
        .unwrap_or_default();
    if channel.is_empty() {
        return;
    }
    let bindings = desktop
        .list_channel_bindings(Some(&channel), Some(&name))
        .unwrap_or_default()
        .into_iter()
        .map(|binding| MobileChannelBinding {
            binding_id: binding.binding_id,
            channel: binding.channel,
            account_id: binding.account_id,
            peer_kind: binding.peer_kind,
            peer_id: binding.peer_id,
            agent_id: binding.agent_id,
            enabled: binding.enabled,
        })
        .collect();
    if let Ok(mut state) = shared.lock() {
        state.channels.selected = name;
        state.channels.bindings = bindings;
        state.channels_rev = state.channels_rev.wrapping_add(1);
        state.touch();
    }
}

pub fn open_editor(_desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, account_id: &str) {
    let (draft, is_new) = {
        let state = match shared.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        let catalog = state.channels.catalog.clone();
        let account = state
            .channels
            .accounts
            .iter()
            .find(|account| account.account_id == account_id)
            .cloned();
        let is_new = account.is_none();
        let channel = account
            .as_ref()
            .map(|account| account.channel.clone())
            .or_else(|| catalog.first().map(|item| item.channel.clone()))
            .unwrap_or_default();
        let draft = crate::MobileChannelDraft {
            account_id: account
                .as_ref()
                .map(|account| account.account_id.clone())
                .unwrap_or_default()
                .into(),
            channel: channel.into(),
            name: account
                .as_ref()
                .map(|account| account.name.clone())
                .unwrap_or_default()
                .into(),
            enabled: account.as_ref().map(|account| account.active).unwrap_or(true),
            bot_type: String::new().into(),
            app_id: String::new().into(),
            app_secret: String::new().into(),
            corp_id: String::new().into(),
            wechat_agent_id: String::new().into(),
            wechat_secret: String::new().into(),
            wechat_token: String::new().into(),
            wechat_aes_key: String::new().into(),
            wechat_mp_token: String::new().into(),
            wechat_mp_aes_key: String::new().into(),
            original_id: String::new().into(),
            weixin_api_base: String::new().into(),
            weixin_bot_token: String::new().into(),
            weixin_bot_id: String::new().into(),
            weixin_user_id: String::new().into(),
            domain: String::new().into(),
            receive_group_chat: false,
            config_json: String::new().into(),
        };
        let draft = account
            .as_ref()
            .map(|account| prefill(draft.clone(), account))
            .unwrap_or(draft);
        (draft, is_new)
    };
    if let Ok(mut state) = shared.lock() {
        state.channel_editor = Some(ChannelEditorView {
            is_new,
            draft,
            error: String::new(),
        });
        state.channel_editor_rev = state.channel_editor_rev.wrapping_add(1);
        state.touch();
    }
}

fn prefill(mut draft: crate::MobileChannelDraft, account: &MobileChannelAccount) -> crate::MobileChannelDraft {
    draft.name = account.name.clone().into();
    draft.enabled = account.active;
    draft.app_id = account.app_id.clone().into();
    draft.corp_id = account.corp_id.clone().into();
    draft.wechat_agent_id = account.wechat_agent_id.clone().into();
    draft.original_id = account.original_id.clone().into();
    draft.weixin_bot_id = account.weixin_bot_id.clone().into();
    draft.weixin_user_id = account.weixin_user_id.clone().into();
    draft.domain = account.domain.clone().into();
    draft
}

pub fn close_editor(shared: &Arc<Mutex<Shared>>) {
    if let Ok(mut state) = shared.lock() {
        state.channel_editor = None;
        state.channel_editor_rev = state.channel_editor_rev.wrapping_add(1);
        state.touch();
    }
}

pub fn save(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    draft: crate::MobileChannelDraft,
) {
    let edit = NativeChannelAccountEdit {
        account_id: draft.account_id.trim().to_string(),
        channel: draft.channel.trim().to_string(),
        account_name: draft.name.trim().to_string(),
        agent_id: String::new(),
        enabled: draft.enabled,
        receive_group_chat: Some(draft.receive_group_chat),
        app_id: draft.app_id.trim().to_string(),
        app_secret: draft.app_secret.trim().to_string(),
        corp_id: draft.corp_id.trim().to_string(),
        wechat_agent_id: draft.wechat_agent_id.trim().to_string(),
        wechat_secret: draft.wechat_secret.trim().to_string(),
        wechat_token: draft.wechat_token.trim().to_string(),
        wechat_aes_key: draft.wechat_aes_key.trim().to_string(),
        wechat_mp_token: draft.wechat_mp_token.trim().to_string(),
        wechat_mp_aes_key: draft.wechat_mp_aes_key.trim().to_string(),
        wechat_mp_original_id: draft.original_id.trim().to_string(),
        weixin_api_base: draft.weixin_api_base.trim().to_string(),
        weixin_bot_token: draft.weixin_bot_token.trim().to_string(),
        weixin_bot_id: draft.weixin_bot_id.trim().to_string(),
        weixin_user_id: draft.weixin_user_id.trim().to_string(),
        domain: draft.domain.trim().to_string(),
        config_json: draft.config_json.trim().to_string(),
    };
    match desktop.save_channel_account(&edit) {
        Ok(card) => {
            if let Ok(mut state) = shared.lock() {
                state.channel_editor = None;
                state.channel_editor_rev = state.channel_editor_rev.wrapping_add(1);
                state.touch();
            }
            refresh(desktop, shared);
            notify(shared, format!("渠道账号「{}」已保存", card.name));
        }
        Err(error) => set_editor_error(shared, format!("保存失败：{error}")),
    }
}

pub fn toggle(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    account_id: &str,
    enabled: bool,
) {
    let channel = shared
        .lock()
        .ok()
        .and_then(|state| {
            state
                .channels
                .accounts
                .iter()
                .find(|account| account.account_id == account_id)
                .map(|account| account.channel.clone())
        })
        .unwrap_or_default();
    if channel.is_empty() {
        return;
    }
    match desktop.toggle_channel_account(&channel, account_id, enabled) {
        Ok(()) => refresh(desktop, shared),
        Err(error) => notify(shared, format!("更新渠道失败：{error}")),
    }
}

pub fn delete(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, account_id: &str) {
    let channel = shared
        .lock()
        .ok()
        .and_then(|state| {
            state
                .channels
                .accounts
                .iter()
                .find(|account| account.account_id == account_id)
                .map(|account| account.channel.clone())
        })
        .unwrap_or_default();
    if channel.is_empty() {
        return;
    }
    match desktop.delete_channel_account(&channel, account_id) {
        Ok(()) => {
            if let Ok(mut state) = shared.lock() {
                if state.channels.selected == account_id {
                    state.channels.selected.clear();
                    state.channels.bindings.clear();
                }
            }
            refresh(desktop, shared);
            notify(shared, "渠道账号已删除");
        }
        Err(error) => notify(shared, format!("删除失败：{error}")),
    }
}

pub fn open_logs(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, account_id: &str) {
    let (channel, name) = shared
        .lock()
        .ok()
        .and_then(|state| {
            state
                .channels
                .accounts
                .iter()
                .find(|account| account.account_id == account_id)
                .map(|account| (account.channel.clone(), account.account_id.clone()))
        })
        .unwrap_or_default();
    if channel.is_empty() {
        return;
    }
    let logs = desktop
        .list_channel_runtime_logs(Some(&channel), Some(&name), 200)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| MobileChannelLog {
            time: entry.time,
            level: entry.level,
            event: entry.event,
            message: entry.message,
        })
        .collect();
    if let Ok(mut state) = shared.lock() {
        state.channels.logs = logs;
        state.channels.logs_open = true;
        state.channels.logs_account = name;
        state.channels_rev = state.channels_rev.wrapping_add(1);
        state.touch();
    }
}

pub fn close_logs(shared: &Arc<Mutex<Shared>>) {
    if let Ok(mut state) = shared.lock() {
        state.channels.logs_open = false;
        state.channels.logs.clear();
        state.channels_rev = state.channels_rev.wrapping_add(1);
        state.touch();
    }
}

pub fn delete_binding(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    binding_id: &str,
) {
    let binding = shared.lock().ok().and_then(|state| {
        state
            .channels
            .bindings
            .iter()
            .find(|binding| binding.binding_id == binding_id)
            .map(|binding| {
                (
                    binding.channel.clone(),
                    binding.account_id.clone(),
                    binding.peer_kind.clone(),
                    binding.peer_id.clone(),
                )
            })
    });
    let Some((channel, account, peer_kind, peer_id)) = binding else {
        return;
    };
    match desktop.delete_channel_binding(&channel, &account, &peer_kind, &peer_id) {
        Ok(()) => {
            let selected = shared
                .lock()
                .map(|state| state.channels.selected.clone())
                .unwrap_or_default();
            if !selected.is_empty() {
                select(desktop, shared, &selected);
            }
            notify(shared, "渠道绑定已删除");
        }
        Err(error) => notify(shared, format!("删除绑定失败：{error}")),
    }
}

fn account_from(card: ChannelAccountCard, catalog: &[MobileChannelCatalog]) -> MobileChannelAccount {
    let channel_name = catalog
        .iter()
        .find(|item| item.channel == card.channel)
        .map(|item| item.name.clone())
        .unwrap_or_else(|| card.channel.clone());
    MobileChannelAccount {
        channel: card.channel,
        channel_name,
        account_id: card.account_id,
        name: card.name,
        status: card.status,
        active: card.active,
        configured: card.configured,
        summary: card.summary,
        secret_set: card.secret_set,
        app_id: card.app_id,
        corp_id: card.corp_id,
        wechat_agent_id: card.wechat_agent_id,
        original_id: card.original_id,
        weixin_bot_id: card.weixin_bot_id,
        weixin_user_id: card.weixin_user_id,
        bot_type: card.bot_type,
        domain: card.domain,
    }
}

fn set_error(shared: &Arc<Mutex<Shared>>, message: String) {
    if let Ok(mut state) = shared.lock() {
        state.channels.loading = false;
        state.channels.error = message.clone();
        state.channels_rev = state.channels_rev.wrapping_add(1);
        state.touch();
    }
    notify(shared, message);
}

fn set_editor_error(shared: &Arc<Mutex<Shared>>, message: String) {
    if let Ok(mut state) = shared.lock() {
        if let Some(editor) = state.channel_editor.as_mut() {
            editor.error = message;
        }
        state.channel_editor_rev = state.channel_editor_rev.wrapping_add(1);
        state.touch();
    }
}

fn notify(shared: &Arc<Mutex<Shared>>, message: impl Into<String>) {
    if let Ok(mut state) = shared.lock() {
        state.notify(message);
        state.touch();
    }
}
