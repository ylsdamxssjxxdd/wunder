use crate::{CronCard, MainWindow};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::sync::Arc;
use wunder_desktop::NativeDesktop;

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let list_api = api.clone();
    app.on_refresh_cron(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cron_loading() {
            return;
        }
        app.set_cron_loading(true);
        let weak = app.as_weak();
        let api = list_api.clone();
        std::thread::spawn(move || {
            let result = api.list_cron_jobs();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_cron_loading(false);
                match result {
                    Ok(items) => app.set_cron_jobs(ModelRc::new(VecModel::from(
                        items
                            .into_iter()
                            .map(|item| CronCard {
                                id: item.id.into(),
                                name: item.name.into(),
                                enabled: item.enabled,
                                schedule: item.schedule.into(),
                                next_run: item.next_run.into(),
                                last_status: item.last_status.into(),
                                last_error: item.last_error.into(),
                            })
                            .collect::<Vec<_>>(),
                    ))),
                    Err(error) => app.set_status(format!("无法读取定时任务：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let action_api = api.clone();
    app.on_toggle_cron(move |id, enabled| {
        let Some(app) = weak.upgrade() else { return };
        let api = action_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.toggle_cron_job(&id, enabled);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => app.invoke_refresh_cron(),
                Err(error) => app.set_status(format!("无法更新定时任务：{error}").into()),
            });
        });
    });
    let weak = app.as_weak();
    let delete_api = api.clone();
    app.on_delete_cron(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let api = delete_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.delete_cron_job(&id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => app.invoke_refresh_cron(),
                Err(error) => app.set_status(format!("无法删除定时任务：{error}").into()),
            });
        });
    });
}
