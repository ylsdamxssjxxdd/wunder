//! Interlink (§I9) shell bindings: a 2s poll reads the tunnel state and drains
//! the approval queue in-process. The 设备互通 settings page is gone, so the
//! cloud-workspace browsing that used to ride the same façade now lives only in
//! the working-directory panel (`crate::workspace_files`).
use crate::{InterlinkApproval, MainWindow};
use slint::ComponentHandle;
use std::{cell::RefCell, sync::Arc};
use wunder_desktop::NativeDesktop;

thread_local! {
    /// One standing poll; started by [install] and kept alive for the whole
    /// session. The two façade reads per tick are in-process map lookups, so
    /// the idle cost is negligible.
    static POLL_TIMER: RefCell<slint::Timer> = RefCell::new(slint::Timer::default());
}

/// Two-second cadence: fast enough that a remote approval request never feels
/// lost, slow enough to stay invisible on the UI thread.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// One poll tick: push status, then open the next approval if the card is
/// free. Runs on the UI thread; both façade calls are synchronous in-process.
fn poll(app: &MainWindow, api: &Arc<NativeDesktop>) {
    let status = api.interlink_status();
    // The working-directory region rides the same tick: it derives its
    // offline state from the tunnel and boots the cloud tree on the first
    // connected tick after sign-in.
    crate::workspace_files::tunnel_tick(app, api, status.state == "connected");
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

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    // Standing 2s poll; the first tick fires immediately so the panel is
    // never blank while the process is connected.
    let weak = app.as_weak();
    let poll_api = api.clone();
    POLL_TIMER.with(|timer| {
        timer
            .borrow()
            .start(slint::TimerMode::Repeated, POLL_INTERVAL, move || {
                let Some(app) = weak.upgrade() else { return };
                poll(&app, &poll_api);
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
