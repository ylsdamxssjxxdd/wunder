//! Interlink (设备互通 §I9) shell bindings: a 2s poll pushes the tunnel
//! status and drains the approval queue, while devices and cloud listings are
//! fetched on demand through the synchronous `NativeDesktop` façade. Network
//! work (device list, cloud listing, cloud decision) always runs on a
//! background thread via `crate::native_pages::run_background`; only the
//! in-process status / pending-approval reads happen on the UI thread.
use crate::{native_pages, InterlinkApproval, InterlinkEntry, InterlinkNode, InterlinkStatus, MainWindow};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, sync::Arc};
use wunder_desktop::NativeDesktop;

thread_local! {
    /// One standing poll; started by [install] and kept alive for the whole
    /// session. The two façade reads per tick are in-process map lookups, so
    /// the idle cost is negligible.
    static POLL_TIMER: RefCell<slint::Timer> = RefCell::new(slint::Timer::default());
    /// Command id of the cloud listing currently waiting for approval. The
    /// panel only shows one pending listing, so a single slot suffices.
    static PENDING_COMMAND: RefCell<String> = RefCell::new(String::new());
}

/// Two-second cadence: fast enough that a remote approval request never feels
/// lost, slow enough to stay invisible on the UI thread.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Human sentence for the tunnel status row. Chinese copy lives here (the
/// native side owns its own strings); the state word itself is translated on
/// the Slint side, the note only adds detail.
fn status_note(state: &str, last_error: Option<&str>, dropped_frames: u64) -> String {
    let base = match state {
        "connected" => "互通隧道已连接，其他设备可以查看本机工作区",
        "connecting" => "正在接入舰体…",
        "reconnecting" => "连接中断，正在自动重试…",
        _ => "互通未启用，可在登录云端后开启",
    };
    // A saturated tunnel drops non-critical frames instead of blocking; the
    // count is the only sign the operator gets that projection degraded.
    let degraded = if dropped_frames > 0 {
        format!("；隧道拥塞，已丢弃 {dropped_frames} 个可丢帧")
    } else {
        String::new()
    };
    match last_error {
        Some(error) if !error.is_empty() => format!("{base}；最近错误：{error}{degraded}"),
        _ => format!("{base}{degraded}"),
    }
}

/// One poll tick: push status, then open the next approval if the card is
/// free. Runs on the UI thread; both façade calls are synchronous in-process.
fn poll(app: &MainWindow, api: &NativeDesktop) {
    let status = api.interlink_status();
    app.set_interlink_status(InterlinkStatus {
        state: status.state.clone().into(),
        note: status_note(&status.state, status.last_error.as_deref(), status.dropped_frames).into(),
    });
    if app.get_interlink_approval_open() {
        return;
    }
    if let Some(next) = api.interlink_pending_approvals().first() {
        app.set_interlink_approval(InterlinkApproval {
            approval_id: next.approval_id.clone().into(),
            command_id: next.command_id.clone().into(),
            kind: next.kind.clone().into(),
            level: next.level.clone().into(),
            risk: next.risk.clone().into(),
            from_node: next.from_node.clone().into(),
            prompt: next.prompt.clone().into(),
        });
        app.set_interlink_approval_open(true);
    }
}

/// Push one cloud listing onto the panel. `status` selects the pending strip;
/// the note line carries the human summary (count, wait state or error).
fn push_listing(app: &MainWindow, listing: wunder_desktop::native::NativeInterlinkListing) {
    let note = match (listing.status.as_str(), listing.error.as_deref()) {
        ("pending", _) => "等待云端审批…".to_string(),
        ("failed", Some(error)) => format!("打开失败：{error}"),
        ("failed", None) => "打开失败：云端未返回原因".to_string(),
        _ => format!("共 {} 项", listing.total),
    };
    PENDING_COMMAND.with(|slot| {
        *slot.borrow_mut() = listing.command_id.unwrap_or_default();
    });
    app.set_interlink_cloud_path(listing.path.into());
    app.set_interlink_cloud_note(note.into());
    app.set_interlink_cloud_pending(listing.status == "pending");
    let entries: Vec<InterlinkEntry> = listing
        .entries
        .into_iter()
        .map(|entry| InterlinkEntry {
            name: entry.name.into(),
            path: entry.path.into(),
            kind: entry.kind.into(),
            size: entry.size.min(i32::MAX as u64) as i32,
            updated_time: entry.updated_time.into(),
        })
        .collect();
    app.set_interlink_entries(ModelRc::new(VecModel::from(entries)));
}

/// Open (or re-open) a cloud path on a background thread.
fn open_path(app_weak: slint::Weak<MainWindow>, api: Arc<NativeDesktop>, path: String) {
    native_pages::run_background(move || {
        let result = api.interlink_cloud_listing(&path);
        let _ = app_weak.upgrade_in_event_loop(move |app| {
            app.set_interlink_cloud_busy(false);
            match result {
                Ok(listing) => push_listing(&app, listing),
                Err(error) => {
                    app.set_interlink_cloud_busy(false);
                    app.set_interlink_cloud_note(format!("无法打开：{error}").into());
                }
            }
        });
    });
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    // Standing 2s poll; the first tick fires immediately so the panel is
    // never blank while the process is connected.
    let weak = app.as_weak();
    let poll_api = api.clone();
    POLL_TIMER.with(|timer| {
        timer.borrow().start(slint::TimerMode::Repeated, POLL_INTERVAL, move || {
            let Some(app) = weak.upgrade() else { return };
            poll(&app, &poll_api);
        });
    });
    // Devices: fetch on demand (the list needs the tunnel, so a poll would
    // only burn requests while signed out).
    let weak = app.as_weak();
    let refresh_api = api.clone();
    app.on_interlink_refresh(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_interlink_loading() {
            return;
        }
        app.set_interlink_loading(true);
        let weak = weak.clone();
        let api = refresh_api.clone();
        native_pages::run_background(move || {
            let result = api.interlink_devices();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_interlink_loading(false);
                match result {
                    Ok(nodes) => {
                        let rows: Vec<InterlinkNode> = nodes
                            .into_iter()
                            .map(|node| InterlinkNode {
                                node_id: node.node_id.into(),
                                node_type: node.node_type.into(),
                                label: node.label.into(),
                                status: node.status.into(),
                                connected: node.connected,
                                shadow_revision: node.shadow_revision as i32,
                            })
                            .collect();
                        app.set_interlink_nodes(ModelRc::new(VecModel::from(rows)));
                    }
                    Err(error) => native_pages::show_error(&app, format!("无法读取设备列表：{error}")),
                }
            });
        });
    });
    // Cloud listing: the façade blocks up to 15s, so this must stay on a
    // worker thread.
    let weak = app.as_weak();
    let open_api = api.clone();
    app.on_interlink_open(move |path| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_interlink_cloud_busy() {
            return;
        }
        app.set_interlink_cloud_busy(true);
        let weak = weak.clone();
        let api = open_api.clone();
        open_path(weak, api, path.trim().to_string());
    });
    // Up one level: the parent is computed from the canonical path the last
    // listing returned, because Slint strings have no rsplit.
    let weak = app.as_weak();
    let up_api = api.clone();
    app.on_interlink_cloud_up(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_interlink_cloud_busy() {
            return;
        }
        let current = app.get_interlink_cloud_path().trim().trim_end_matches('/').to_string();
        let parent = match current.rsplit_once('/') {
            Some(("", _)) => "/".to_string(),
            Some((parent, _)) => parent.to_string(),
            None => String::new(),
        };
        app.set_interlink_cloud_busy(true);
        let weak = weak.clone();
        let api = up_api.clone();
        open_path(weak, api, parent);
    });
    // Approve / reject a pending cloud listing command, then re-open the path
    // so the panel reflects the decision without a manual refresh.
    let weak = app.as_weak();
    let decide_api = api.clone();
    app.on_interlink_cloud_decide(move |approve| {
        let Some(app) = weak.upgrade() else { return };
        let command_id = PENDING_COMMAND.with(|slot| slot.borrow().clone());
        if command_id.is_empty() {
            return;
        }
        app.set_interlink_cloud_busy(true);
        let weak = weak.clone();
        let api = decide_api.clone();
        native_pages::run_background(move || {
            let result = api.interlink_cloud_decide(&command_id, approve);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Err(error) = result {
                    app.set_interlink_cloud_busy(false);
                    app.set_interlink_cloud_note(format!("审批提交失败：{error}").into());
                    return;
                }
                let path = app.get_interlink_cloud_path().trim().to_string();
                let api = api.clone();
                let weak = app.as_weak();
                open_path(weak, api, path);
            });
        });
    });
    // Approval card: the in-process decision is fast enough to run inline,
    // then the poll picks up the next pending approval immediately.
    let weak = app.as_weak();
    let card_api = api.clone();
    let decide_card = move |approve: bool| {
        let Some(app) = weak.upgrade() else { return };
        let approval_id = app.get_interlink_approval().approval_id.to_string();
        if approval_id.is_empty() {
            return;
        }
        card_api.interlink_decide_approval(&approval_id, approve, false);
        app.set_interlink_approval_open(false);
        poll(&app, &card_api);
    };
    app.on_interlink_approve({
        let decide = decide_card.clone();
        move || decide(true)
    });
    let decide_card = decide_card.clone();
    app.on_interlink_reject(move || decide_card(false));
}
