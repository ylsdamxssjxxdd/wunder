//! Settings-page software-update wiring: pushes native update snapshots into
//! the shell properties and relays the one-shot check/download/apply requests
//! to the façade. While the native state machine is mid-flight (checking /
//! downloading / applying) a 250 ms poll keeps the card in sync — the façade
//! stays UI-agnostic, so progress travels through `update_view` snapshots.

use crate::MainWindow;
use slint::ComponentHandle;
use std::sync::Arc;
use std::time::Duration;
use wunder_desktop::{NativeDesktop, TARGET_DESKTOP, UpdateStateView};

const POLL_INTERVAL: Duration = Duration::from_millis(250);

// Timers must fire on the UI thread; every entry point here runs on it.
thread_local! {
    static POLL_TIMER: slint::Timer = slint::Timer::default();
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    // Startup projection: a staged package restored across restarts shows as
    // "restart to apply" without any user action.
    project(app, &api.update_view());

    let weak = app.as_weak();
    let check_api = api.clone();
    app.on_check_updates(move |source, target| {
        let Some(app) = weak.upgrade() else { return };
        let phase = app.get_update_phase().to_string();
        if matches!(phase.as_str(), "checking" | "downloading" | "applying") {
            return;
        }
        start_polling(&app, &check_api);
        let api = check_api.clone();
        let weak = weak.clone();
        let source = source.trim().to_string();
        let target = target.trim().to_string();
        crate::native_pages::run_background(move || {
            let result = api.update_check(&source, &target);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Err(error) = result {
                    // The state machine already recorded in-flight failures;
                    // this only catches fast bails (no cloud session) that
                    // never entered it.
                    app.set_status(format!("检查更新失败：{error}").into());
                }
                // The timer projects the settled phase; stop it on completion.
                let view = api.update_view();
                project(&app, &view);
                if !is_active(&view.phase) {
                    POLL_TIMER.with(|timer| timer.stop());
                }
            });
        });
    });

    let weak = app.as_weak();
    let download_api = api.clone();
    app.on_download_update(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_update_phase() != "available" {
            return;
        }
        start_polling(&app, &download_api);
        let api = download_api.clone();
        let weak = weak.clone();
        crate::native_pages::run_background(move || {
            let result = api.update_download();
            let _ = weak.upgrade_in_event_loop(move |app| {
                match result {
                    Ok(()) => app.set_status("更新包下载完成".into()),
                    Err(error) => app.set_status(format!("更新下载失败：{error}").into()),
                }
                let view = api.update_view();
                project(&app, &view);
                if !is_active(&view.phase) {
                    POLL_TIMER.with(|timer| timer.stop());
                }
            });
        });
    });

    let weak = app.as_weak();
    let apply_api = api.clone();
    app.on_apply_update(move || {
        let Some(app) = weak.upgrade() else { return };
        let phase = app.get_update_phase().to_string();
        if !matches!(phase.as_str(), "staged" | "failed") {
            return;
        }
        start_polling(&app, &apply_api);
        let api = apply_api.clone();
        let weak = weak.clone();
        crate::native_pages::run_background(move || {
            let result = api.update_apply();
            let _ = weak.upgrade_in_event_loop(move |app| {
                match result {
                    Ok(()) => {
                        if app.get_update_target() == TARGET_DESKTOP {
                            app.set_status("更新已应用，正在重启…".into());
                            // The replacement binary is already running with the
                            // handover flag; terminate is the only reliable
                            // process boundary on Win7 tray stacks (see
                            // shutdown.rs).
                            crate::shutdown::request_exit();
                        } else {
                            // CLI and supplement targets finish in place; the
                            // state machine already recorded the summary.
                            app.set_status("更新完成".into());
                            let view = api.update_view();
                            project(&app, &view);
                            POLL_TIMER.with(|timer| timer.stop());
                        }
                    }
                    Err(error) => {
                        app.set_status(format!("应用更新失败：{error}").into());
                        let view = api.update_view();
                        project(&app, &view);
                        POLL_TIMER.with(|timer| timer.stop());
                    }
                }
            });
        });
    });
}

fn is_active(phase: &str) -> bool {
    matches!(phase, "checking" | "downloading" | "applying")
}

/// Arm the snapshot poller. Runs on the UI thread; re-arming an armed timer
/// just resets it, so double clicks stay harmless.
fn start_polling(app: &MainWindow, api: &Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let api = api.clone();
    POLL_TIMER.with(|timer| {
        timer.start(slint::TimerMode::Repeated, POLL_INTERVAL, move || {
            let Some(app) = weak.upgrade() else {
                POLL_TIMER.with(|timer| timer.stop());
                return;
            };
            let view = api.update_view();
            let active = is_active(&view.phase);
            project(&app, &view);
            if !active {
                POLL_TIMER.with(|timer| timer.stop());
            }
        });
    });
}

fn project(app: &MainWindow, view: &UpdateStateView) {
    app.set_update_current_version(view.current_version.clone().into());
    app.set_update_phase(view.phase.clone().into());
    app.set_update_target(view.target.clone().into());
    app.set_update_local_version(view.local_version.clone().into());
    app.set_update_latest_version(view.latest_version.clone().into());
    app.set_update_notes(view.notes.clone().into());
    app.set_update_mandatory(view.mandatory);
    app.set_update_error(view.error.clone().into());
    app.set_update_progress(view.progress);
    app.set_update_progress_text(view.progress_text.clone().into());
}
