//! 设置 → 数据与恢复：本地备份、从已有备份恢复与崩溃日志。
//!
//! 备份打包 `<home>/config` 与顶层 SQLite 库；恢复只做校验与解包，真正的
//! 覆盖发生在下一次启动（见 `wunder_desktop::native_backup`）。打包与解包都
//! 有几百 MB 的 IO，一律放后台线程，界面只投影结果。

use crate::{LocalBackupRow, MainWindow};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;
use wunder_desktop::native_backup;

pub fn install(app: &MainWindow) {
    refresh(app);

    let weak = app.as_weak();
    app.on_refresh_backups(move || {
        if let Some(app) = weak.upgrade() {
            refresh(&app);
        }
    });

    let weak = app.as_weak();
    app.on_backup_now(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_data_backup_busy() {
            return;
        }
        app.set_data_backup_busy(true);
        app.set_data_backup_status("正在打包 config 与本地数据库…".into());
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = native_backup::create_backup();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_data_backup_busy(false);
                match result {
                    Ok(path) => {
                        let path = path.to_string_lossy().into_owned();
                        app.set_data_restore_path(path.as_str().into());
                        app.set_data_backup_status(format!("已备份到 {path}").into());
                        app.set_status("本地备份已完成".into());
                        refresh(&app);
                    }
                    Err(error) => {
                        app.set_data_backup_status(format!("备份失败：{error:#}").into());
                        app.set_status(format!("本地备份失败：{error}").into());
                    }
                }
            });
        });
    });

    // 选择任意位置的备份文件：列表之外的 zip（别的机器、下载目录）。
    let weak = app.as_weak();
    app.on_pick_backup_file(move || {
        let Some(app) = weak.upgrade() else { return };
        if let Some(path) = crate::file_dialog::pick_backup_archive() {
            app.set_data_restore_path(path.into());
        }
    });

    let weak = app.as_weak();
    app.on_restore_backup(move |path| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_data_backup_busy() {
            return;
        }
        let path = path.to_string();
        if path.trim().is_empty() {
            app.set_status("请先选择一个备份文件".into());
            return;
        }
        app.set_data_backup_busy(true);
        app.set_data_backup_status("正在校验并解包备份…".into());
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = native_backup::stage_restore(&path);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_data_backup_busy(false);
                match result {
                    Ok(()) => {
                        app.set_data_backup_restart(true);
                        app.set_data_backup_status(
                            "恢复已就绪：请完全退出并重新打开应用后生效。".into(),
                        );
                        app.set_status("恢复已就绪，请重启应用".into());
                    }
                    Err(error) => {
                        app.set_data_backup_status(format!("恢复失败：{error:#}").into());
                        app.set_status(format!("恢复失败：{error}").into());
                    }
                }
            });
        });
    });

    let weak = app.as_weak();
    app.on_view_crash_log(move || {
        let Some(app) = weak.upgrade() else { return };
        match native_backup::latest_crash_log() {
            Some((name, text)) => {
                app.set_data_crash_title(name.into());
                app.set_data_crash_text(text.into());
            }
            None => {
                app.set_data_crash_title("".into());
                app.set_data_crash_text("".into());
                app.set_status("没有找到崩溃日志".into());
            }
        }
    });
}

/// 把已有备份投影成列表；每次切换页面或完成一次备份后调用。
fn refresh(app: &MainWindow) {
    let rows: Vec<LocalBackupRow> = native_backup::list_backups()
        .into_iter()
        .map(|entry| LocalBackupRow {
            path: entry.path.into(),
            name: entry.name.into(),
            detail: entry.detail.into(),
        })
        .collect();
    let selected = app.get_data_restore_path().to_string();
    // 选中项还在列表里就保留；否则默认选最新一份，按钮直接可用。
    let selected_still_there = rows.iter().any(|row| row.path.as_str() == selected);
    app.set_data_backups(ModelRc::from(Rc::new(VecModel::from(rows.clone()))));
    if !selected_still_there {
        let newest = rows
            .first()
            .map(|row| row.path.to_string())
            .unwrap_or_default();
        app.set_data_restore_path(newest.into());
    }
    if app.get_data_backup_status().is_empty() {
        let note = match rows.len() {
            0 => "还没有本地备份".to_string(),
            count => format!(
                "共 {count} 份备份，最近一份 {}",
                rows.first().map(|row| row.name.as_str()).unwrap_or("")
            ),
        };
        app.set_data_backup_status(note.into());
    }
}
