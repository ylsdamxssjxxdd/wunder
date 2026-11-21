//! 安卓版 channel settings projection: accounts, bindings and runtime logs.
//! Secrets only travel into `save`; every projection stays secret-free.

use crate::engine::Shared;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use wunder_desktop::native::{
    ChannelAccountCard, NativeChannelAccountEdit, WeixinQrLoginStatus,
};
use wunder_desktop::NativeDesktop;

/// One short window of the weixin QR long poll. The loop re-reads the sheet
/// between windows so cancelling takes effect within ~3 s.
const QR_POLL_WINDOW_MS: u64 = 3_000;

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

#[derive(Clone, Default)]
pub struct ChannelEditorView {
    pub is_new: bool,
    pub draft: crate::MobileChannelDraft,
    pub error: String,
}

/// State of the native weixin QR login sheet. The bitmap is decoded on the
/// polling worker, so the render tick only wraps the pixels into a
/// `slint::Image` and no PNG work happens on the UI thread.
#[derive(Clone, Default)]
pub struct ChannelQrView {
    pub open: bool,
    /// 0 正在获取 · 1 等待扫码 · 2 已确认 · 3 已过期 · 4 已取消 · 5 失败
    pub status: i32,
    pub message: String,
    pub image_rgba: Vec<u8>,
    /// 同一张码的 PNG 原件，供「保存图片」在不重编码的情况下落盘。
    pub image_png: Vec<u8>,
    pub image_width: u32,
    pub image_height: u32,
    /// 扫码确认后待回填的凭据。UI 线程按 `pending_seq` 消费一次，只改这些
    /// 字段，用户在等待期间输入的账号名称等内容不会被整份草稿覆盖。
    pub pending: Option<ChannelQrCredentials>,
    pub pending_seq: u64,
}

/// Credentials a confirmed QR login hands to the editor draft.
#[derive(Clone, Default)]
pub struct ChannelQrCredentials {
    pub bot_token: String,
    pub bot_id: String,
    pub user_id: String,
    pub api_base: String,
}

/// Bumped on every start / close. A polling thread only writes while its own
/// generation is current, so a late reply never paints over a newer attempt.
static QR_GENERATION: AtomicU64 = AtomicU64::new(0);

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

/// Opens the weixin QR login sheet and starts one polling worker. `force`
/// discards a still-valid cached QR so 「刷新二维码」 always produces a new one.
pub fn start_qr_login(
    desktop: &Arc<NativeDesktop>,
    shared: &Arc<Mutex<Shared>>,
    account_id: &str,
    force: bool,
) {
    let target = {
        let trimmed = account_id.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    };
    let generation = QR_GENERATION.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
    mutate_qr(shared, generation, |qr| {
        qr.open = true;
        qr.status = 0;
        qr.message.clear();
        qr.image_rgba.clear();
        qr.image_png.clear();
        qr.image_width = 0;
        qr.image_height = 0;
    });

    let worker_desktop = Arc::clone(desktop);
    let worker_shared = Arc::clone(shared);
    let spawned = std::thread::Builder::new()
        .name("wunder-android-weixin-qr".into())
        .spawn(move || {
            let started = match worker_desktop.start_weixin_qr_login(target.as_deref(), force) {
                Ok(started) => started,
                Err(error) => {
                    let message = format!("无法获取二维码：{error}");
                    mutate_qr(&worker_shared, generation, |qr| {
                        qr.open = true;
                        qr.status = 5;
                        qr.message = message;
                    });
                    return;
                }
            };
            let Some((width, height, rgba, png)) = decode_qr_png(&started.png_data_uri) else {
                mutate_qr(&worker_shared, generation, |qr| {
                    qr.open = true;
                    qr.status = 5;
                    qr.message = "二维码渲染失败，请刷新重试".into();
                });
                return;
            };
            mutate_qr(&worker_shared, generation, |qr| {
                qr.open = true;
                qr.status = 1;
                qr.message.clear();
                qr.image_width = width;
                qr.image_height = height;
                qr.image_rgba = rgba;
                qr.image_png = png;
            });
            poll_qr_login(
                &worker_desktop,
                &worker_shared,
                generation,
                &started.session_key,
            );
        });
    if let Err(error) = spawned {
        log::error!("spawn android weixin qr worker failed: {error}");
        mutate_qr(shared, QR_GENERATION.load(Ordering::SeqCst), |qr| {
            qr.open = true;
            qr.status = 5;
            qr.message = "无法启动扫码线程".into();
        });
    }
}

/// Closes the sheet. The generation bump stops the polling worker at its next
/// window boundary; no thread handle has to be joined.
pub fn cancel_qr(shared: &Arc<Mutex<Shared>>) {
    QR_GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut state) = shared.lock() {
        state.channel_qr.open = false;
        state.channel_qr.status = 4;
        state.channel_qr.message.clear();
        state.channel_qr_rev = state.channel_qr_rev.wrapping_add(1);
        state.touch();
    }
}

/// Saves the current QR to the phone's Downloads so a user who only has this
/// phone can pick it in WeChat's 「扫一扫 → 相册」.
pub fn save_qr_image(shared: &Arc<Mutex<Shared>>) {
    let png = shared
        .lock()
        .ok()
        .map(|state| state.channel_qr.image_png.clone())
        .filter(|png| !png.is_empty());
    let Some(png) = png else {
        notify(shared, "二维码还没生成，请稍后重试");
        return;
    };
    let path = std::env::temp_dir().join("wunder-weixin-qr.png");
    if let Err(error) = std::fs::write(&path, &png) {
        notify(shared, format!("无法写入二维码：{error}"));
        return;
    }
    #[cfg(target_os = "android")]
    {
        match crate::android_host::save_file_to_phone(
            &path.to_string_lossy(),
            "wunder-weixin-qr.png",
        )
        .as_str()
        {
            "saved" => notify(shared, "二维码已存到「下载」，可在微信「扫一扫 → 相册」中选择"),
            "pending" => notify(shared, "请选择保存位置"),
            _ => notify(shared, "保存失败，请稍后重试"),
        }
    }
    // 桌面预览没有手机下载目录，只把文件落到临时目录并如实说明。
    #[cfg(not(target_os = "android"))]
    notify(
        shared,
        format!("二维码已写入 {}", path.display()),
    );
}

/// Loops short wait windows until confirmed, expired, failed or superseded.
fn poll_qr_login(
    desktop: &Arc<NativeDesktop>,
    shared: &Arc<Mutex<Shared>>,
    generation: u64,
    session_key: &str,
) {
    loop {
        if QR_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        match desktop.wait_weixin_qr_login(session_key, QR_POLL_WINDOW_MS) {
            Ok(status) if status.connected => {
                hand_off_qr_credentials(shared, generation, &status);
                if let Ok(mut state) = shared.lock() {
                    state.notify("扫码成功，凭据已填入，保存后生效");
                    state.touch();
                }
                return;
            }
            Ok(status) if status.status == "expired" => {
                mutate_qr(shared, generation, |qr| {
                    qr.open = true;
                    qr.status = 3;
                    qr.message.clear();
                });
                return;
            }
            Ok(_) => continue,
            Err(error) => {
                let text = error.to_string();
                let expired = text.contains("expired");
                mutate_qr(shared, generation, |qr| {
                    qr.open = true;
                    qr.status = if expired { 3 } else { 5 };
                    qr.message = if expired { String::new() } else { text };
                });
                return;
            }
        }
    }
}

/// Hands the confirmed credentials to the UI thread. The editor draft itself
/// is merged there, because only the UI owns a draft the user has been typing
/// into while the code was on screen.
fn hand_off_qr_credentials(
    shared: &Arc<Mutex<Shared>>,
    generation: u64,
    status: &WeixinQrLoginStatus,
) {
    let credentials = ChannelQrCredentials {
        bot_token: status.bot_token.clone(),
        bot_id: status.ilink_bot_id.clone(),
        user_id: status.ilink_user_id.clone(),
        api_base: status.api_base.clone(),
    };
    mutate_qr(shared, generation, |qr| {
        qr.open = false;
        qr.status = 2;
        qr.message.clear();
        qr.pending = Some(credentials);
        qr.pending_seq = qr.pending_seq.wrapping_add(1);
    });
}

/// Applies one change to the sheet unless a newer attempt has superseded it.
fn mutate_qr(shared: &Arc<Mutex<Shared>>, generation: u64, change: impl FnOnce(&mut ChannelQrView)) {
    if QR_GENERATION.load(Ordering::SeqCst) != generation {
        return;
    }
    let Ok(mut state) = shared.lock() else {
        return;
    };
    if QR_GENERATION.load(Ordering::SeqCst) != generation {
        return;
    }
    change(&mut state.channel_qr);
    state.channel_qr_rev = state.channel_qr_rev.wrapping_add(1);
    state.touch();
}

/// Decodes the locally rendered QR PNG into RGBA pixels and keeps the PNG
/// bytes beside them: Slint needs pixels, 「保存图片」 needs the file. The code
/// is opaque, so the decoder's straight alpha is already what Slint's
/// premultiplied buffer needs.
fn decode_qr_png(data_uri: &str) -> Option<(u32, u32, Vec<u8>, Vec<u8>)> {
    use base64::Engine;
    let encoded = data_uri.strip_prefix("data:image/png;base64,")?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(&png))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height, rgba.into_raw(), png))
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
