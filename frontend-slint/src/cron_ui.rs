use crate::{CronCard, CronRunCard, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::sync::{Arc, Mutex, OnceLock};
use wunder_desktop::{NativeCronJobEdit, NativeDesktop};

fn workspace_ids() -> &'static Mutex<Vec<String>> {
    static IDS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    IDS.get_or_init(|| Mutex::new(Vec::new()))
}

fn format_duration(ms: i64) -> String {
    if ms <= 0 {
        return "0s".into();
    }
    let secs = ms as f64 / 1000.0;
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        format!("{}m{:.0}s", (secs / 60.0).floor(), secs % 60.0)
    }
}

/// RFC3339 or raw stored time text rendered as local "YYYY-MM-DD HH:MM".
fn format_local_at(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return parsed
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
    }
    trimmed.to_string()
}

fn load_workspace_names(app: &MainWindow, api: &Arc<NativeDesktop>, selected_id: String) {
    static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    app.set_cron_edit_workspace(-1);
    app.set_cron_workspace_names(ModelRc::default());
    let weak = app.as_weak();
    let api = api.clone();
    std::thread::spawn(move || {
        let result = api.list_workspaces();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if GENERATION.load(std::sync::atomic::Ordering::Relaxed) != generation {
                return;
            }
            match result {
                Ok(workspaces) => {
                    let ids = workspaces
                        .iter()
                        .map(|workspace| workspace.workspace_id.clone())
                        .collect::<Vec<_>>();
                    let names = workspaces
                        .into_iter()
                        .map(|workspace| workspace.name)
                        .collect::<Vec<_>>();
                    let selected = ids
                        .iter()
                        .position(|id| id == &selected_id)
                        .map(|index| index as i32)
                        .unwrap_or(if selected_id.is_empty() && !ids.is_empty() {
                            0
                        } else {
                            -1
                        });
                    *workspace_ids().lock().unwrap_or_else(|e| e.into_inner()) = ids;
                    app.set_cron_workspace_names(ModelRc::new(VecModel::from(
                        names
                            .into_iter()
                            .map(SharedString::from)
                            .collect::<Vec<_>>(),
                    )));
                    app.set_cron_edit_workspace(selected);
                }
                Err(error) => app.set_status(format!("无法读取工作区列表：{error}").into()),
            }
        });
    });
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let list_api = api.clone();
    app.on_refresh_cron(move || {
        let Some(app) = weak.upgrade() else { return };
        refresh_jobs(&app, &list_api);
    });
    let weak = app.as_weak();
    let action_api = api.clone();
    app.on_toggle_cron(move |id, enabled| {
        let Some(app) = weak.upgrade() else { return };
        let api = action_api.clone();
        let weak = app.as_weak();
        let id = id.to_string();
        std::thread::spawn(move || {
            let result = api.toggle_cron_job(&id, enabled);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => refresh_jobs(&app, &api),
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
        let id = id.to_string();
        std::thread::spawn(move || {
            let result = api.delete_cron_job(&id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => {
                    refresh_jobs(&app, &api);
                    app.set_status("定时任务已删除".into());
                }
                Err(error) => app.set_status(format!("无法删除定时任务：{error}").into()),
            });
        });
    });
    let weak = app.as_weak();
    let run_api = api.clone();
    app.on_run_cron_now(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let api = run_api.clone();
        let weak = app.as_weak();
        let id = id.to_string();
        std::thread::spawn(move || {
            let result = api.run_cron_job_now(&id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(status) if status == "running" => {
                    app.set_status("任务正在运行中，未重复排队".into());
                    refresh_jobs(&app, &api);
                }
                Ok(_) => {
                    app.set_status("已加入立即执行队列".into());
                    refresh_jobs(&app, &api);
                }
                Err(error) => app.set_status(format!("无法立即运行任务：{error}").into()),
            });
        });
    });
    let weak = app.as_weak();
    let runs_api = api.clone();
    app.on_open_cron_runs(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let title = app
            .get_cron_jobs()
            .iter()
            .find(|job| job.id == id)
            .map(|job| job.name.to_string())
            .unwrap_or_else(|| id.to_string());
        app.set_cron_runs_title(title.into());
        app.set_cron_runs_open(true);
        app.set_cron_runs_loading(true);
        app.set_cron_runs(ModelRc::default());
        let api = runs_api.clone();
        let id = id.to_string();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.list_cron_job_runs(&id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_cron_runs_loading(false);
                match result {
                    Ok(runs) => {
                        app.set_cron_runs(ModelRc::new(VecModel::from(
                            runs.into_iter()
                                .map(|run| CronRunCard {
                                    run_id: run.run_id.into(),
                                    status: run.status.into(),
                                    trigger: run.trigger.into(),
                                    summary: truncate_text(&run.summary, 120).into(),
                                    error: truncate_text(&run.error, 120).into(),
                                    duration: format_duration(run.duration_ms).into(),
                                    created_at: run.created_at.into(),
                                })
                                .collect::<Vec<_>>(),
                        )));
                    }
                    Err(error) => app.set_status(format!("无法读取运行记录：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    app.on_close_cron_runs(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_cron_runs_open(false);
        app.set_cron_runs(ModelRc::default());
    });
    let weak = app.as_weak();
    let creator_api = api.clone();
    app.on_open_cron_creator(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_cron_edit_id("".into());
        app.set_cron_edit_name("".into());
        app.set_cron_editor_kind(0);
        app.set_cron_edit_at(
            (chrono::Local::now() + chrono::Duration::minutes(5))
                .format("%Y-%m-%d %H:%M")
                .to_string()
                .into(),
        );
        app.set_cron_edit_every("5".into());
        app.set_cron_edit_interval_unit(0);
        app.set_cron_edit_cron("".into());
        app.set_cron_edit_tz("".into());
        app.set_cron_edit_message("".into());
        app.set_cron_edit_workspace(-1);
        app.set_cron_edit_delete_after(false);
        app.set_cron_edit_enabled(true);
        app.set_cron_editor_open(true);
        let workspace_id = app
            .get_workspaces()
            .row_data(app.get_selected_workspace().max(0) as usize)
            .map(|workspace| workspace.workspace_id.to_string())
            .unwrap_or_default();
        load_workspace_names(&app, &creator_api, workspace_id);
    });
    let weak = app.as_weak();
    let edit_api = api.clone();
    app.on_edit_cron_job(move |id| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cron_editor_saving() {
            return;
        }
        let api = edit_api.clone();
        let id = id.to_string();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.get_cron_job(&id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(job) => {
                    app.set_cron_edit_id(job.id.clone().into());
                    app.set_cron_edit_name(job.name.clone().into());
                    app.set_cron_editor_kind(match job.schedule_kind.as_str() {
                        "at" => 0,
                        "every" => 1,
                        _ => 2,
                    });
                    app.set_cron_edit_at(format_local_at(&job.schedule_at).into());
                    let (interval, unit) = if job.every_s >= 86400 && job.every_s % 86400 == 0 {
                        (job.every_s / 86400, 2)
                    } else if job.every_s >= 3600 && job.every_s % 3600 == 0 {
                        (job.every_s / 3600, 1)
                    } else if job.every_s % 60 == 0 {
                        (job.every_s / 60, 0)
                    } else {
                        (job.every_s, 3)
                    };
                    let every_text = if interval > 0 {
                        interval.to_string()
                    } else {
                        String::new()
                    };
                    app.set_cron_edit_every(every_text.into());
                    app.set_cron_edit_interval_unit(unit);
                    app.set_cron_edit_cron(job.cron_expr.clone().into());
                    app.set_cron_edit_tz(job.timezone.clone().into());
                    app.set_cron_edit_message(job.message.clone().into());
                    load_workspace_names(&app, &api, job.workspace_id.clone());
                    app.set_cron_edit_delete_after(job.delete_after_run);
                    app.set_cron_edit_enabled(job.enabled);
                    app.set_cron_editor_open(true);
                }
                Err(error) => app.set_status(format!("无法读取定时任务：{error}").into()),
            });
        });
    });
    let weak = app.as_weak();
    app.on_cancel_cron_editor(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_cron_editor_open(false);
    });
    let weak = app.as_weak();
    let save_api = api.clone();
    app.on_save_cron_editor(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cron_editor_saving() {
            return;
        }
        let message = app.get_cron_edit_message().trim().to_string();
        let name = if app.get_cron_edit_name().trim().is_empty() {
            message.chars().take(40).collect()
        } else {
            app.get_cron_edit_name().trim().to_string()
        };
        if message.is_empty() {
            app.set_status("执行消息不能为空".into());
            return;
        }
        let kind_index = app.get_cron_editor_kind();
        let unit = match app.get_cron_edit_interval_unit() {
            1 => 3600,
            2 => 86400,
            3 => 1,
            _ => 60,
        };
        let every_s = app
            .get_cron_edit_every()
            .trim()
            .parse::<i64>()
            .unwrap_or(0)
            .checked_mul(unit)
            .unwrap_or(0);
        if kind_index == 1 && !(1..=86_400).contains(&every_s) {
            app.set_status("重复周期必须为正整数，且不超过 1 天".into());
            return;
        }
        if kind_index == 2 && app.get_cron_edit_cron().trim().is_empty() {
            app.set_status("Cron 表达式不能为空".into());
            return;
        }
        let workspace_index = app.get_cron_edit_workspace();
        let workspace_id = workspace_ids()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(workspace_index as usize)
            .cloned()
            .unwrap_or_default();
        if workspace_id.is_empty() {
            app.set_status("请选择工作区".into());
            return;
        }
        let edit = NativeCronJobEdit {
            job_id: app.get_cron_edit_id().trim().to_string(),
            name,
            schedule_kind: match kind_index {
                0 => "at",
                1 => "every",
                _ => "cron",
            }
            .to_string(),
            schedule_at: app.get_cron_edit_at().trim().to_string(),
            every_s,
            cron_expr: app.get_cron_edit_cron().trim().to_string(),
            timezone: app.get_cron_edit_tz().trim().to_string(),
            message,
            workspace_id,
            delete_after_run: app.get_cron_edit_delete_after(),
            enabled: app.get_cron_edit_enabled(),
        };
        app.set_cron_editor_saving(true);
        let weak = app.as_weak();
        let api = save_api.clone();
        std::thread::spawn(move || {
            let result = api.save_cron_job(&edit);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_cron_editor_saving(false);
                match result {
                    Ok(_) => {
                        app.set_cron_editor_open(false);
                        refresh_jobs(&app, &api);
                        app.set_status("定时任务已保存".into());
                    }
                    Err(error) => app.set_status(format!("保存定时任务失败：{error}").into()),
                }
            });
        });
    });
}

fn refresh_jobs(app: &MainWindow, api: &Arc<NativeDesktop>) {
    if app.get_cron_loading() {
        return;
    }
    app.set_cron_loading(true);
    let weak = app.as_weak();
    let api = api.clone();
    std::thread::spawn(move || {
        let result = api.list_cron_jobs();
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_cron_loading(false);
            match result {
                Ok(items) => {
                    let selected_id = app
                        .get_cron_jobs()
                        .row_data(app.get_cron_selected() as usize)
                        .map(|job| job.id);
                    let selected = selected_id
                        .and_then(|id| items.iter().position(|item| item.id == id.as_str()))
                        .map(|index| index as i32)
                        .unwrap_or(if items.is_empty() { -1 } else { 0 });
                    app.set_cron_jobs(ModelRc::new(VecModel::from(
                        items
                            .into_iter()
                            .map(|item| CronCard {
                                id: item.id.into(),
                                name: item.name.into(),
                                enabled: item.enabled,
                                workspace_name: item.workspace_name.into(),
                                message: item.message.into(),
                                schedule: item.schedule.into(),
                                next_run: item.next_run.into(),
                                last_status: item.last_status.into(),
                                last_error: item.last_error.into(),
                            })
                            .collect::<Vec<_>>(),
                    )));
                    app.set_cron_selected(selected);
                }
                Err(error) => app.set_status(format!("无法读取定时任务：{error}").into()),
            }
        });
    });
}

fn truncate_text(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(max).collect();
    format!("{cut}…")
}
