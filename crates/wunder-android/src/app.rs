//! UI assembly and the 33 ms projection tick.
//!
//! Slint runs on the main thread; the engine owns everything blocking. The
//! engine publishes plain `RowData` (Send), and this module converts it to the
//! Slint row model, caching each row by content signature so a streaming token
//! only re-projects the live tail.

use crate::engine::{self, Command, Shared};
use crate::md::MarkdownRenderer;
use crate::project::{self, RowData};
use crate::{
    InquiryOption, InquiryQuestion, MobileSession, MobileWindow, ModelCard, Theme, TimelineRow,
    WorkspaceFile,
};
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wunder_desktop::{HostHud, HostHudSampler};

const TICK: Duration = Duration::from_millis(33);
const CACHE_LIMIT: usize = 800;

pub fn build() -> MobileWindow {
    let (ui, model) = create_window();
    // The engine outlives this window; a recreated activity attaches to the
    // same transcript and any turn that is still streaming.
    let handle = engine::acquire();
    let shared = handle.shared.clone();
    let commands = handle.command_sender();
    wire_callbacks(&ui, &commands, &shared);
    start_tick(&ui, &shared, &model, commands);
    ui
}

thread_local! {
    // The tick timer belongs to one native activity thread. Android runs every
    // activity instance on its own native thread, so a thread-local slot drops
    // the previous timer when that activity goes away instead of leaking one
    // timer per recreation.
    static TICK_TIMER: std::cell::RefCell<Option<Timer>> = const { std::cell::RefCell::new(None) };
}

/// Window without the engine, used by the desktop preview binary. The layout
/// and every shared component are identical to the phone build.
pub fn build_preview() -> MobileWindow {
    let (ui, model) = create_window();
    install_preview_data(&ui, &model);
    let model_for_toggle = model.clone();
    ui.on_toggle_row(move |index| {
        if let Some((bar, child)) = project::process_child_address(index) {
            if let Some(row) = model_for_toggle.row_data(bar) {
                if let Some(entry) = row.process_entries.row_data(child) {
                    let mut entry = entry;
                    entry.open = !entry.open;
                    row.process_entries.set_row_data(child, entry);
                    model_for_toggle.set_row_data(bar, row);
                }
            }
            return;
        }
        let index = index.max(0) as usize;
        if let Some(mut row) = model_for_toggle.row_data(index) {
            if row.foldable {
                row.open = !row.open;
                model_for_toggle.set_row_data(index, row);
            }
        }
    });

    // The preview has no engine, so the shell's intents are implemented over
    // local models. Every screen reachable on the phone stays reachable here.
    let sessions = Rc::new(VecModel::<MobileSession>::from(preview_sessions()));
    ui.set_sessions(ModelRc::from(sessions.clone()));
    let archived = Rc::new(VecModel::<MobileSession>::from(vec![MobileSession {
        id: "preview-archived-1".into(),
        title: "旧的运行时实验".into(),
        preview: "心舰".into(),
        time: "10-28 18:40".into(),
        status: "idle".into(),
        active: false,
    }]));
    ui.set_archived_sessions(ModelRc::from(archived.clone()));
    ui.set_archived_total(1);
    let weak = ui.as_weak();
    ui.on_open_drawer(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_plus_open(false);
            ui.set_drawer_open(true);
        }
    });
    let weak = ui.as_weak();
    ui.on_close_drawer(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
        }
    });
    let weak = ui.as_weak();
    ui.on_select_session(move |_id| {
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
        }
    });
    let weak = ui.as_weak();
    ui.on_new_session(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
        }
    });
    let sessions_for_rename = sessions.clone();
    ui.on_rename_session(move |id, title| {
        for index in 0..sessions_for_rename.row_count() {
            if let Some(mut session) = sessions_for_rename.row_data(index) {
                if session.id == id {
                    session.title = title.clone();
                    sessions_for_rename.set_row_data(index, session);
                    break;
                }
            }
        }
    });
    let sessions_for_archive = sessions.clone();
    let archived_for_archive = archived.clone();
    let ui_for_archive = ui.as_weak();
    ui.on_archive_session(move |id| {
        for index in 0..sessions_for_archive.row_count() {
            if sessions_for_archive
                .row_data(index)
                .is_some_and(|session| session.id == id)
            {
                let mut row = sessions_for_archive.row_data(index).unwrap();
                row.active = false;
                sessions_for_archive.remove(index);
                archived_for_archive.push(row);
                break;
            }
        }
        if let Some(ui) = ui_for_archive.upgrade() {
            ui.set_archived_total(archived_for_archive.row_count() as i32);
        }
    });
    let sessions_for_restore = sessions.clone();
    let archived_for_restore = archived.clone();
    let ui_for_restore = ui.as_weak();
    ui.on_restore_session(move |id| {
        for index in 0..archived_for_restore.row_count() {
            if archived_for_restore
                .row_data(index)
                .is_some_and(|session| session.id == id)
            {
                let row = archived_for_restore.row_data(index).unwrap();
                archived_for_restore.remove(index);
                sessions_for_restore.push(row);
                break;
            }
        }
        if let Some(ui) = ui_for_restore.upgrade() {
            ui.set_archived_total(archived_for_restore.row_count() as i32);
        }
    });
    let archived_for_delete = archived.clone();
    let ui_for_delete = ui.as_weak();
    ui.on_delete_archived_session(move |id| {
        for index in 0..archived_for_delete.row_count() {
            if archived_for_delete
                .row_data(index)
                .is_some_and(|session| session.id == id)
            {
                archived_for_delete.remove(index);
                break;
            }
        }
        if let Some(ui) = ui_for_delete.upgrade() {
            ui.set_archived_total(archived_for_delete.row_count() as i32);
        }
    });
    ui.on_refresh_archived(|| {});

    let weak = ui.as_weak();
    ui.on_open_settings(move |page| {
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
            ui.set_plus_open(false);
            ui.set_settings_page(page);
            ui.set_settings_open(true);
        }
    });
    let weak = ui.as_weak();
    ui.on_close_settings(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_open(false);
        }
    });
    let weak = ui.as_weak();
    ui.on_open_model_settings(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
            ui.set_settings_page(1);
            ui.set_settings_open(true);
        }
    });
    let weak = ui.as_weak();
    ui.on_save_theme(move |value| {
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_theme(value.clone());
            ui.global::<Theme>().set_palette(value);
        }
    });
    let weak = ui.as_weak();
    ui.on_save_font_size(move |size| {
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_font_size(size);
            ui.global::<Theme>()
                .set_font_scale(size.clamp(12, 20) as f32 / 14.0);
        }
    });
    let weak = ui.as_weak();
    ui.on_save_send_key(move |value| {
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_send_key(value);
        }
    });
    let weak = ui.as_weak();
    ui.on_save_language(move |value| {
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_language(value);
        }
    });
    let weak = ui.as_weak();
    ui.on_load_trajectory(move |_id, title| {
        if let Some(ui) = weak.upgrade() {
            ui.set_trajectory_title(title.clone());
            ui.set_trajectory_error("".into());
            ui.set_trajectory_loading(false);
            ui.set_trajectory_filter(0);
            ui.set_trajectory_items(ModelRc::from(Rc::new(VecModel::from(preview_trajectory()))));
            let (audit, segments) = preview_trajectory_audit();
            ui.set_trajectory_audit(audit);
            ui.set_trajectory_segments(ModelRc::from(Rc::new(VecModel::from(segments))));
        }
    });
    let weak = ui.as_weak();
    ui.on_trajectory_filter_changed(move |filter| {
        if let Some(ui) = weak.upgrade() {
            ui.set_trajectory_items(ModelRc::from(Rc::new(VecModel::from(
                filtered_trajectory(&preview_trajectory(), filter),
            ))));
        }
    });
    let workspace_root = Rc::new(VecModel::<WorkspaceFile>::from(preview_workspace()));
    ui.set_workspace_entries(ModelRc::from(workspace_root.clone()));
    ui.set_workspace_total(workspace_root.row_count() as i32);
    let weak = ui.as_weak();
    ui.on_open_workspace(move |path| {
        if let Some(ui) = weak.upgrade() {
            ui.set_workspace_path(path.clone());
            ui.set_workspace_parent_path(
                path.rsplit_once('/')
                    .map(|(parent, _)| parent.to_string())
                    .unwrap_or_default()
                    .into(),
            );
            ui.set_workspace_error("".into());
            ui.set_workspace_loading(false);
        }
    });
    ui.on_refresh_workspace(|| {});
    ui.on_upload_workspace(|_| {});
    ui.on_download_workspace(|_| {});
    ui.on_load_more_workspace(|_, _| {});
    ui.on_plus_panel_opened(|| {});
    ui.on_keyboard_dismiss(|| {});
    ui
}

fn preview_workspace() -> Vec<WorkspaceFile> {
    let rows = [
        ("文档", "文档", true, 0, "文件夹"),
        ("图片", "图片", true, 0, "文件夹"),
        ("运行报告.md", "运行报告.md", false, 4, "12.4 KB"),
        ("界面截图.png", "界面截图.png", false, 1, "1.8 MB"),
        ("补充包.zip", "补充包.zip", false, 3, "86 MB"),
        ("演示视频.mp4", "演示视频.mp4", false, 2, "24 MB"),
    ];
    rows.into_iter()
        .map(|(name, path, dir, kind, size)| WorkspaceFile {
            name: name.into(),
            path: path.into(),
            dir,
            kind,
            size_text: size.into(),
        })
        .collect()
}

fn preview_sessions() -> Vec<MobileSession> {
    vec![
        MobileSession {
            id: "preview-1".into(),
            title: "移动端聊天区对齐".into(),
            preview: "心舰".into(),
            time: "11-06 14:20".into(),
            status: "running".into(),
            active: true,
        },
        MobileSession {
            id: "preview-2".into(),
            title: "检查无障碍控制链路".into(),
            preview: "心舰".into(),
            time: "11-05 09:12".into(),
            status: "idle".into(),
            active: false,
        },
        MobileSession {
            id: "preview-3".into(),
            title: "整理运行时补充包".into(),
            preview: "心舰".into(),
            time: "11-04 21:08".into(),
            status: "idle".into(),
            active: false,
        },
    ]
}

/// Mobile filter: 0 all, 1 conversation, 2 tools, 3 system. When a filter
/// drops the user row that carried the turn label, the first surviving row of
/// that turn inherits the label so filtered views keep their grouping.
fn filtered_trajectory(
    items: &[crate::TrajectoryItem],
    filter: i32,
) -> Vec<crate::TrajectoryItem> {
    if filter <= 0 {
        return items.to_vec();
    }
    let mut out = Vec::new();
    let mut group = slint::SharedString::new();
    let mut emitted_in_group = false;
    for item in items {
        if !item.group.is_empty() {
            group = item.group.clone();
            emitted_in_group = false;
        }
        let keep = match filter {
            1 => item.kind <= 1,
            2 => item.kind == 2 || item.kind == 3,
            3 => item.kind >= 4,
            _ => true,
        };
        if !keep {
            continue;
        }
        let mut item = item.clone();
        if !emitted_in_group && item.group.is_empty() {
            item.group = group.clone();
        }
        emitted_in_group = true;
        out.push(item);
    }
    out
}

/// Preview fixture feeds the real projection, so demo rows, the audit strip
/// and token formatting cannot drift from the engine.
fn preview_trajectory_page() -> serde_json::Value {
    use chrono::{Local, TimeZone};
    let base = Local::now()
        .date_naive()
        .and_hms_opt(14, 20, 1)
        .and_then(|time| Local.from_local_datetime(&time).single())
        .map(|time| time.timestamp() as f64)
        .unwrap_or(0.0);
    let at = |offset: f64| base + offset;
    serde_json::json!({
        "trajectory": [
            {
                "kind": "user_message", "user_round": 1, "status": "completed",
                "summary": "把聊天区搬到手机上，样式和桌面版保持一致。",
                "started_at": at(0.0), "completed_at": at(1.0)
            },
            {
                "kind": "assistant_message", "user_round": 1, "status": "completed",
                "summary": "先确认聊天组件的复用边界，再处理单窗口约束。",
                "started_at": at(5.0), "completed_at": at(9.2),
                "stats": {
                    "request_consumed_tokens": 1240,
                    "prefill_duration_s": 0.8, "decode_duration_s": 3.4
                }
            },
            {
                "kind": "tool_call", "user_round": 1, "status": "completed",
                "tool_name": "读取文件", "summary": "frontend-slint/ui/timeline.slint",
                "args_preview": "{\"path\":\"frontend-slint/ui/timeline.slint\"}",
                "result_preview": "已读取 64 KB",
                "started_at": at(10.0), "completed_at": at(10.8)
            },
            {
                "kind": "tool_call", "user_round": 1, "status": "failed",
                "tool_name": "检索", "summary": "Composer / TimelineView",
                "started_at": at(12.0), "completed_at": at(12.6)
            },
            {
                "kind": "user_message", "user_round": 2, "status": "completed",
                "summary": "补充一下加号面板和会话操作。",
                "started_at": at(369.0), "completed_at": at(370.0)
            },
            {
                "kind": "assistant_message", "user_round": 2, "status": "completed",
                "summary": "加号改为底部功能面板，会话行增加操作入口。",
                "started_at": at(377.0), "completed_at": at(380.4),
                "stats": {
                    "request_consumed_tokens": 2100,
                    "prefill_duration_s": 0.6, "decode_duration_s": 2.8
                }
            },
            {
                "kind": "tool_call", "user_round": 2, "status": "completed",
                "tool_name": "编辑文件", "summary": "frontend-slint/ui/composer.slint",
                "args_preview": "{\"path\":\"frontend-slint/ui/composer.slint\"}",
                "result_preview": "已写入 3 处修改",
                "started_at": at(381.0), "completed_at": at(382.2)
            },
            {
                "kind": "compaction", "user_round": 2, "status": "completed",
                "summary": "压缩了 12 条历史消息",
                "started_at": at(389.0), "completed_at": at(389.0)
            }
        ]
    })
}

fn preview_trajectory() -> Vec<crate::TrajectoryItem> {
    engine::trajectory_projection(&preview_trajectory_page()).items
}

fn preview_trajectory_audit() -> (crate::TrajectoryAudit, Vec<crate::TrajectorySegment>) {
    let projection = engine::trajectory_projection(&preview_trajectory_page());
    (projection.audit, projection.segments)
}

fn create_window() -> (MobileWindow, Rc<VecModel<TimelineRow>>) {
    crate::fonts::install();
    let ui = MobileWindow::new().expect("create the mobile window");
    let model = Rc::new(VecModel::<TimelineRow>::default());
    ui.set_rows(ModelRc::from(model.clone()));
    ui.set_greeting("".into());
    (ui, model)
}

fn wire_callbacks(
    ui: &MobileWindow,
    commands: &std::sync::mpsc::Sender<Command>,
    shared: &Arc<Mutex<Shared>>,
) {
    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_open_drawer(move || {
        let _ = sender.send(Command::RefreshSessions);
        let _ = sender.send(Command::RefreshArchived);
        if let Some(ui) = weak.upgrade() {
            ui.set_plus_open(false);
            ui.set_drawer_open(true);
        }
    });
    let weak = ui.as_weak();
    ui.on_close_drawer(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_select_session(move |id| {
        let _ = sender.send(Command::Select(id.to_string()));
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
            ui.set_plus_open(false);
        }
    });

    let sender = commands.clone();
    ui.on_rename_session(move |id, title| {
        let title = title.trim().to_string();
        if title.is_empty() {
            return;
        }
        let _ = sender.send(Command::RenameSession {
            id: id.to_string(),
            title,
        });
    });

    let sender = commands.clone();
    ui.on_archive_session(move |id| {
        let _ = sender.send(Command::ArchiveSession(id.to_string()));
    });

    let sender = commands.clone();
    ui.on_restore_session(move |id| {
        let _ = sender.send(Command::RestoreSession(id.to_string()));
    });

    let sender = commands.clone();
    ui.on_delete_archived_session(move |id| {
        let _ = sender.send(Command::DeleteArchivedSession(id.to_string()));
    });

    let sender = commands.clone();
    ui.on_refresh_archived(move || {
        let _ = sender.send(Command::RefreshArchived);
    });

    let sender = commands.clone();
    ui.on_load_trajectory(move |id, title| {
        let _ = sender.send(Command::LoadTrajectory {
            id: id.to_string(),
            title: title.to_string(),
        });
    });

    let sender = commands.clone();
    ui.on_open_workspace(move |path| {
        let _ = sender.send(Command::LoadWorkspace {
            path: path.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_refresh_workspace(move || {
        let _ = sender.send(Command::RefreshWorkspace);
    });
    ui.on_upload_workspace(move |dir| {
        #[cfg(target_os = "android")]
        crate::android_host::pick_files(&dir);
        #[cfg(not(target_os = "android"))]
        let _ = dir;
    });
    let sender = commands.clone();
    ui.on_download_workspace(move |path| {
        let _ = sender.send(Command::DownloadWorkspace {
            path: path.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_load_more_workspace(move |path, offset| {
        let _ = sender.send(Command::LoadMoreWorkspace {
            path: path.to_string(),
            offset: offset.max(0) as u64,
        });
    });

    let sender = commands.clone();
    ui.on_trajectory_filter_changed(move |filter| {
        let _ = sender.send(Command::TrajectoryFilter(filter));
    });

    // Opening the phone's function panel replaces the keyboard, exactly like
    // the plus button in a messenger: the panel needs the bottom half of the
    // screen, not a keyboard above it.
    ui.on_plus_panel_opened(move || {
        #[cfg(target_os = "android")]
        crate::android_host::hide_keyboard();
    });
    ui.on_keyboard_dismiss(move || {
        #[cfg(target_os = "android")]
        crate::android_host::hide_keyboard();
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_new_session(move || {
        let _ = sender.send(Command::NewThread);
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_send(move || {
        let Some(ui) = weak.upgrade() else { return };
        let draft = ui.get_draft().to_string();
        let draft = draft.trim().to_string();
        if draft.is_empty() {
            return;
        }
        let _ = sender.send(Command::Send(draft));
        ui.set_draft("".into());
    });

    let sender = commands.clone();
    ui.on_stop(move || {
        let _ = sender.send(Command::Stop);
    });

    let sender = commands.clone();
    ui.on_select_model(move |_index, key| {
        let _ = sender.send(Command::SelectModel(key.to_string()));
    });

    let sender = commands.clone();
    ui.on_reasoning_effort_changed(move |value| {
        let _ = sender.send(Command::ReasoningEffort(value.to_string()));
    });

    // ---- settings sheet ----------------------------------------------
    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_open_settings(move |page| {
        log::info!("open settings page={page}");
        let _ = sender.send(Command::LoadSettings);
        let _ = sender.send(Command::RefreshArchived);
        let _ = sender.send(Command::RefreshUbuntu);
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
            ui.set_plus_open(false);
            ui.set_settings_page(page);
            ui.set_settings_open(true);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_close_settings(move || {
        log::info!("close settings");
        let _ = sender.send(Command::CloseModelEditor);
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_open(false);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_save_theme(move |theme| {
        let (send_key, font_size) = match weak.upgrade() {
            Some(ui) => (
                ui.get_settings_send_key().to_string(),
                ui.get_settings_font_size(),
            ),
            None => return,
        };
        let _ = sender.send(Command::SavePreferences {
            theme: theme.to_string(),
            send_key,
            font_size,
        });
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_save_font_size(move |font_size| {
        let Some(ui) = weak.upgrade() else { return };
        let font_size = font_size.clamp(12, 20);
        // The persisted value drives the live scale; applying it here keeps
        // the stepper responsive while the engine writes the preference.
        ui.global::<Theme>().set_font_scale(font_size as f32 / 14.0);
        let _ = sender.send(Command::SavePreferences {
            theme: ui.get_settings_theme().to_string(),
            send_key: ui.get_settings_send_key().to_string(),
            font_size,
        });
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_save_send_key(move |send_key| {
        let Some(ui) = weak.upgrade() else { return };
        let _ = sender.send(Command::SavePreferences {
            theme: ui.get_settings_theme().to_string(),
            send_key: send_key.to_string(),
            font_size: ui.get_settings_font_size(),
        });
    });

    let sender = commands.clone();
    ui.on_save_language(move |language| {
        let _ = sender.send(Command::SaveLanguage(language.to_string()));
    });

    let sender = commands.clone();
    ui.on_install_ubuntu(move || {
        let _ = sender.send(Command::InstallUbuntu);
    });

    let sender = commands.clone();
    ui.on_set_ubuntu_enabled(move |enabled| {
        let _ = sender.send(Command::SetUbuntuEnabled(enabled));
    });

    let sender = commands.clone();
    ui.on_open_model_editor(move |key| {
        let key = key.to_string();
        let _ = sender.send(Command::OpenModelEditor {
            key: if key.is_empty() { None } else { Some(key) },
        });
    });

    let sender = commands.clone();
    ui.on_close_model_editor(move || {
        let _ = sender.send(Command::CloseModelEditor);
    });

    let sender = commands.clone();
    ui.on_save_model(move |card, secret| {
        let _ = sender.send(Command::SaveModel {
            card,
            secret: secret.to_string(),
        });
    });

    let sender = commands.clone();
    ui.on_probe_model(move |card, secret| {
        let _ = sender.send(Command::ProbeModel {
            card,
            secret: secret.to_string(),
        });
    });

    let sender = commands.clone();
    ui.on_delete_model(move |key| {
        let _ = sender.send(Command::DeleteModel(key.to_string()));
    });

    let sender = commands.clone();
    ui.on_set_default_model(move |key| {
        let _ = sender.send(Command::SetDefaultModel(key.to_string()));
    });

    let sender = commands.clone();
    ui.on_permission_changed(move |value| {
        let _ = sender.send(Command::PermissionMode(value.to_string()));
    });
    ui.on_remove_attachment(|_index| {});

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_open_model_settings(move || {
        log::info!("open model settings");
        let _ = sender.send(Command::LoadSettings);
        let _ = sender.send(Command::RefreshUbuntu);
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
            ui.set_settings_page(1);
            ui.set_settings_open(true);
        }
    });

    // ---- row fold ----------------------------------------------------
    let shared_for_toggle = shared.clone();
    ui.on_toggle_row(move |index| {
        let Ok(mut state) = shared_for_toggle.lock() else {
            return;
        };
        // Live rows encode their fold target as a negative value because their
        // absolute index is not stable while the durable prefix grows: -1 is
        // the process bar, -2-n is its nth child. Flip the published rows in
        // place; the next stream publish carries the choice forward by id.
        if index < 0 {
            if let Some(bar) = state
                .live_rows
                .iter_mut()
                .find(|row| row.kind == project::KIND_PROCESS)
            {
                if index == -1 {
                    bar.open = !bar.open;
                } else if let Some(child) = bar.process_children.get_mut((-index - 2) as usize) {
                    child.open = !child.open;
                } else {
                    return;
                }
                state.live_rev = state.live_rev.wrapping_add(1);
                state.stream_revision = state.stream_revision.wrapping_add(1);
            }
            return;
        }
        // A child address packs its owning bar and position. The bar is what
        // the UI model has to rebuild, not the child (children are a nested
        // model that only the bar row owns).
        if let Some((bar, child)) = project::process_child_address(index) {
            let Some(row) = state.durable_rows.get_mut(bar) else {
                return;
            };
            let Some(entry) = row.process_children.get_mut(child) else {
                return;
            };
            entry.open = !entry.open;
            state.patch_row = Some(bar);
            state.patch_rev = state.patch_rev.wrapping_add(1);
            return;
        }
        let index = index as usize;
        let Some(row) = state.durable_rows.get_mut(index) else {
            return;
        };
        if !row.foldable {
            return;
        }
        row.open = !row.open;
        state.patch_row = Some(index);
        state.patch_rev = state.patch_rev.wrapping_add(1);
    });

    // ---- 问询面板 ----------------------------------------------------
    let sender = commands.clone();
    ui.on_inquiry_option_clicked(move |row, option| {
        let _ = sender.send(Command::InquiryOption { row, option });
    });
    let sender = commands.clone();
    ui.on_inquiry_no_preference(move |row| {
        let _ = sender.send(Command::InquiryNoPreference { row });
    });
    let sender = commands.clone();
    ui.on_inquiry_use_recommended(move |row| {
        let _ = sender.send(Command::InquiryUseRecommended { row });
    });
    let sender = commands.clone();
    ui.on_inquiry_other_edited(move |row, text| {
        let _ = sender.send(Command::InquiryOther {
            row,
            text: text.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_inquiry_page_move(move |delta| {
        let _ = sender.send(Command::InquiryPageMove(delta));
    });
    let sender = commands.clone();
    ui.on_inquiry_expanded_toggle(move || {
        let _ = sender.send(Command::InquiryExpandToggle);
    });
    let sender = commands.clone();
    ui.on_inquiry_submit(move || {
        let _ = sender.send(Command::InquirySubmit);
    });

    // The transcript's copy buttons and the message selection both land here;
    // the Android backend forwards the text to the system clipboard. Give the
    // tap visible feedback: the system clipboard has no toast of its own in a
    // NativeActivity window.
    let weak = ui.as_weak();
    let shared_for_copy = shared.clone();
    ui.on_copy_text(move |text| {
        let Some(ui) = weak.upgrade() else { return };
        i_slint_core::window::WindowInner::from_pub(ui.window())
            .context()
            .platform()
            .set_clipboard_text(
                text.as_str(),
                i_slint_core::platform::Clipboard::DefaultClipboard,
            );
        if let Ok(mut state) = shared_for_copy.lock() {
            state.notify("已复制");
            state.touch();
        }
    });
}

fn inquiry_row(index: usize, question: &engine::InquiryQuestionView) -> InquiryQuestion {
    InquiryQuestion {
        index: index as i32,
        question: question.question.as_str().into(),
        multiple: question.multiple,
        no_preference: question.no_preference,
        other: question.other.as_str().into(),
        options: ModelRc::new(VecModel::from(
            question
                .options
                .iter()
                .map(|option| InquiryOption {
                    label: option.label.as_str().into(),
                    description: option.description.as_str().into(),
                    recommended: option.recommended,
                    selected: option.selected,
                })
                .collect::<Vec<_>>(),
        )),
    }
}

/// Caches converted rows by id + content signature. Only the changed tail is
/// re-projected while a turn streams.
struct RowCache {
    renderer: MarkdownRenderer,
    rows: HashMap<String, (u64, TimelineRow)>,
}

impl Default for RowCache {
    fn default() -> Self {
        Self {
            renderer: MarkdownRenderer::new(),
            rows: HashMap::new(),
        }
    }
}

impl RowCache {
    fn row(&mut self, data: &RowData) -> TimelineRow {
        let signature = data.signature();
        if let Some((cached_signature, row)) = self.rows.get(&data.id) {
            if *cached_signature == signature {
                return row.clone();
            }
        }
        if self.rows.len() >= CACHE_LIMIT {
            self.rows.clear();
        }
        let row = data.to_row(&mut self.renderer);
        self.rows.insert(data.id.clone(), (signature, row.clone()));
        row
    }

    fn row_items(&mut self, data: &[RowData]) -> Vec<TimelineRow> {
        data.iter().map(|row| self.row(row)).collect()
    }
}

/// True once the durable projection carries the live turn's body or error.
/// From then on the durable rows are the complete turn and the live tail is
/// dropped instead of rendered twice.
fn durable_has_body(rows: &[RowData], root: &str) -> bool {
    !root.is_empty()
        && root != "live"
        && rows.iter().any(|row| {
            row.id
                .strip_prefix(root)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|rest| rest.starts_with("body") || rest.starts_with("error"))
        })
}

/// Same normalization as 桌面版: unknown or legacy theme values fall back to the
/// default accent instead of leaving the palette unset.
/// Byte count for the HUD detail card ("9.0G" / "683G"), one decimal under 10.
fn compact_bytes(value: u64) -> String {
    let mut amount = value as f64;
    for unit in ["B", "K", "M", "G"] {
        if amount < 1024.0 {
            return if amount < 10.0 {
                format!("{amount:.1}{unit}")
            } else {
                format!("{amount:.0}{unit}")
            };
        }
        amount /= 1024.0;
    }
    format!("{amount:.0}T")
}

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

fn start_tick(
    ui: &MobileWindow,
    shared: &Arc<Mutex<Shared>>,
    model: &Rc<VecModel<TimelineRow>>,
    commands: std::sync::mpsc::Sender<Command>,
) {
    let weak = ui.as_weak();
    let shared = shared.clone();
    let model = model.clone();
    let mut ubuntu_ticks = 0u32;
    let mut cache = RowCache::default();
    let mut last_rev = 0u64;
    let mut last_rows = 0u64;
    let mut last_live = 0u64;
    let mut last_toast = 0u64;
    let mut toast_deadline: Option<Instant> = None;
    let mut last_patch = 0u64;
    let mut last_sessions = 0u64;
    let mut last_archived = 0u64;
    let mut last_presets = 0u64;
    let mut last_trajectory = 0u64;
    let mut last_workspace = 0u64;
    let mut last_models = 0u64;
    let mut last_settings = 0u64;
    let mut last_ubuntu = 0u64;
    let mut last_editor = 0u64;
    let mut last_probe = 0u64;
    let mut last_inquiry = 0u64;
    let mut editor_was_open = false;
    let mut last_ime = -1.0f32;
    let mut last_host: Option<HostHud> = None;
    let mut host_ticks = 0u32;
    let sampler = HostHudSampler::start(Duration::from_secs(1));

    let timer = Timer::default();
    timer.start(TimerMode::Repeated, TICK, move || {
        let Some(ui) = weak.upgrade() else { return };
        // The window is edge-to-edge: the activity does not resize under the
        // soft keyboard, so the shell reads Slint's virtual-keyboard rect and
        // lifts the transcript and composer above it.
        let ime = ui
            .window()
            .virtual_keyboard(i_slint_core::InternalToken)
            .map(|(_origin, size)| size.height)
            .unwrap_or(0.0);
        if (ime - last_ime).abs() > 0.5 {
            ui.set_ime_height(ime);
            last_ime = ime;
        }
        // Hardware readings and live command counts move at 1 Hz, not at the
        // render tick. Formatting the strings here keeps the phone row cheap.
        host_ticks = host_ticks.wrapping_add(1);
        if host_ticks % 30 == 0 {
            let sample = sampler.snapshot();
            if last_host != Some(sample) {
                let cpu = sample.cpu_percent.round().clamp(0.0, 100.0) as i32;
                let memory = sample.memory_percent.round().clamp(0.0, 100.0) as i32;
                ui.set_host_cpu(cpu as f32 / 100.0);
                ui.set_host_memory(memory as f32 / 100.0);
                ui.set_host_cpu_text(format!("{cpu}%").as_str().into());
                ui.set_host_memory_text(format!("{memory}%").as_str().into());
                ui.set_host_memory_used(compact_bytes(sample.memory_used).as_str().into());
                ui.set_host_memory_total(compact_bytes(sample.memory_total).as_str().into());
                ui.set_host_cores(sample.cores as i32);
                last_host = Some(sample);
            }
            let _ = commands.send(Command::RefreshTasks);
        }
        // The file picker and the legacy save dialog answer on Java threads.
        // Move their results into the engine queue from the UI tick so the
        // JNI callbacks never touch Slint or the engine state directly.
        #[cfg(target_os = "android")]
        if ui.get_workspace_open() || host_ticks % 30 == 0 {
            for (dir, files) in crate::android_host::take_picked_uploads() {
                let _ = commands.send(Command::UploadWorkspace { dir, files });
            }
            for (ok, name) in crate::android_host::take_save_results() {
                let _ = commands.send(Command::WorkspaceSaved { ok, name });
            }
        }
        let Ok(state) = shared.lock() else { return };

        if state.rev != last_rev {
            ui.set_thread_title(state.title.as_str().into());
            ui.set_thread_subtitle(state.subtitle.as_str().into());
            ui.set_workspace_name(state.workspace.as_str().into());
            ui.set_connection_state(state.connection.as_str().into());
            ui.set_generating(state.generating);
            ui.set_stopping(state.stopping);
            ui.set_loading(state.loading);
            ui.set_spinning(state.generating || state.loading);
            ui.set_model_name(state.model_name.as_str().into());
            ui.set_model_key(state.model_key.as_str().into());
            ui.set_send_key(state.send_key.as_str().into());
            ui.set_stream_revision(state.stream_revision);
            ui.set_running_task_count(state.running_task_count);
            ui.set_subagent_count(state.subagent_count);
            last_rev = state.rev;
        }

        if state.sessions_rev != last_sessions {
            ui.set_sessions(ModelRc::from(Rc::new(VecModel::from(
                state.sessions.clone(),
            ))));
            last_sessions = state.sessions_rev;
        }

        if state.archived_rev != last_archived {
            ui.set_archived_sessions(ModelRc::from(Rc::new(VecModel::from(
                state.archived_sessions.clone(),
            ))));
            ui.set_archived_total(state.archived_total as i32);
            last_archived = state.archived_rev;
        }

        if state.preset_rev != last_presets {
            let questions: Vec<slint::SharedString> = state
                .preset_questions
                .iter()
                .map(|question| question.as_str().into())
                .collect();
            ui.set_preset_questions(ModelRc::from(Rc::new(VecModel::from(questions))));
            last_presets = state.preset_rev;
        }

        if state.inquiry_rev != last_inquiry {
            match &state.inquiry {
                Some(panel) => {
                    let rows: Vec<InquiryQuestion> = if panel.expanded {
                        panel
                            .questions
                            .iter()
                            .enumerate()
                            .map(|(index, question)| inquiry_row(index, question))
                            .collect()
                    } else {
                        let index = panel.current_index();
                        vec![inquiry_row(index, &panel.questions[index])]
                    };
                    ui.set_inquiry_questions(ModelRc::from(Rc::new(VecModel::from(rows))));
                    ui.set_inquiry_page(panel.current_index() as i32);
                    ui.set_inquiry_total(panel.questions.len() as i32);
                    ui.set_inquiry_expanded(panel.expanded);
                    ui.set_inquiry_ready(panel.ready());
                }
                None => {
                    ui.set_inquiry_questions(ModelRc::default());
                    ui.set_inquiry_page(0);
                    ui.set_inquiry_total(0);
                    ui.set_inquiry_expanded(false);
                    ui.set_inquiry_ready(false);
                }
            }
            last_inquiry = state.inquiry_rev;
        }

        if state.trajectory_rev != last_trajectory {
            ui.set_trajectory_title(state.trajectory_title.as_str().into());
            ui.set_trajectory_loading(state.trajectory_loading);
            ui.set_trajectory_error(state.trajectory_error.as_str().into());
            ui.set_trajectory_filter(state.trajectory_filter);
            ui.set_trajectory_items(ModelRc::from(Rc::new(VecModel::from(
                filtered_trajectory(&state.trajectory_items, state.trajectory_filter),
            ))));
            ui.set_trajectory_audit(state.trajectory_audit.clone());
            ui.set_trajectory_segments(ModelRc::from(Rc::new(VecModel::from(
                state.trajectory_segments.clone(),
            ))));
            last_trajectory = state.trajectory_rev;
        }

        if state.workspace_rev != last_workspace {
            ui.set_workspace_path(state.workspace_path.as_str().into());
            ui.set_workspace_parent_path(state.workspace_parent_path.as_str().into());
            ui.set_workspace_total(state.workspace_total.min(i32::MAX as u64) as i32);
            ui.set_workspace_loading(state.workspace_loading);
            ui.set_workspace_error(state.workspace_error.as_str().into());
            ui.set_workspace_entries(ModelRc::from(Rc::new(VecModel::from(
                state.workspace_entries.clone(),
            ))));
            last_workspace = state.workspace_rev;
        }

        if state.models_rev != last_models {
            ui.set_models(ModelRc::from(Rc::new(VecModel::from(state.models.clone()))));
            ui.set_model_catalogue_count(state.models.len() as i32);
            last_models = state.models_rev;
        }

        if state.ubuntu.state == "installing" {
            // Java owns extraction progress; poll it at 1 Hz instead of
            // touching JNI from the 33 ms render tick.
            ubuntu_ticks = ubuntu_ticks.wrapping_add(1);
            if ubuntu_ticks % 30 == 0 {
                let _ = commands.send(Command::RefreshUbuntu);
            }
        }

        if state.settings_rev != last_settings {
            ui.set_settings_theme(state.settings.theme.as_str().into());
            ui.set_settings_font_size(state.settings.font_size);
            ui.set_settings_send_key(state.settings.send_key.as_str().into());
            ui.set_settings_language(state.settings.language.as_str().into());
            ui.set_settings_workspace(state.settings.workspace_root.as_str().into());
            ui.set_settings_version(state.settings.version.as_str().into());
            let theme = ui.global::<Theme>();
            theme.set_palette(normalize_palette(&state.settings.theme).into());
            theme.set_font_scale(state.settings.font_size.clamp(12, 20) as f32 / 14.0);
            last_settings = state.settings_rev;
        }

        if state.ubuntu_rev != last_ubuntu {
            ui.set_settings_ubuntu_state(state.ubuntu.state.as_str().into());
            ui.set_settings_ubuntu_progress(state.ubuntu.progress);
            ui.set_settings_ubuntu_stage(state.ubuntu.stage.as_str().into());
            ui.set_settings_ubuntu_version(state.ubuntu.version.as_str().into());
            ui.set_settings_ubuntu_error(state.ubuntu.error.as_str().into());
            ui.set_settings_ubuntu_enabled(state.ubuntu.enabled);
            last_ubuntu = state.ubuntu_rev;
        }

        if state.editor_rev != last_editor {
            if let Some(editor) = &state.editor {
                ui.set_editor_key(editor.original_key.as_str().into());
                ui.set_editor_new(editor.original_key.is_empty());
                ui.set_editor_default(editor.is_default);
                ui.set_editor_error(editor.error.as_str().into());
                ui.set_editor_probe(editor.probe_message.as_str().into());
                ui.set_editor_probing(editor.probing);
                if !editor_was_open {
                    // Seed exactly once per opening. Later revisions carry a
                    // save error or a probe result and must never overwrite
                    // what the user has typed since.
                    ui.set_editor_draft(editor.card.clone());
                    ui.set_editor_secret("".into());
                    ui.set_editor_probed_context("".into());
                }
                ui.set_editor_open(true);
                editor_was_open = true;
            } else {
                ui.set_editor_open(false);
                editor_was_open = false;
            }
            last_editor = state.editor_rev;
        }

        if state.probe_rev != last_probe {
            ui.set_editor_probed_context(state.probed_context.as_str().into());
            last_probe = state.probe_rev;
        }

        if state.toast_rev != last_toast {
            ui.set_toast(state.toast.as_str().into());
            last_toast = state.toast_rev;
            toast_deadline = Some(Instant::now() + Duration::from_millis(2600));
        } else if toast_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            // The engine keeps the message for late ticks; the shell owns the
            // dismissal so a toast never parks over the next interaction.
            ui.set_toast("".into());
            toast_deadline = None;
        }

        if state.rows_rev != last_rows || state.live_rev != last_live {
            // The durable watcher and the live stream can describe the same
            // turn. Drop exact duplicates (the user bubble, an error card) as
            // soon as they exist in both, and once the durable projection has
            // the turn's body or error the live rows are redundant entirely.
            let live: Vec<RowData> = if state.live_rows.is_empty() {
                Vec::new()
            } else if durable_has_body(&state.durable_rows, &state.live_root) {
                Vec::new()
            } else {
                let durable_ids: std::collections::HashSet<&str> = state
                    .durable_rows
                    .iter()
                    .map(|row| row.id.as_str())
                    .collect();
                state
                    .live_rows
                    .iter()
                    .filter(|row| !durable_ids.contains(row.id.as_str()))
                    .cloned()
                    .collect()
            };
            if state.rows_rev != last_rows {
                let mut rows = Vec::with_capacity(state.durable_rows.len() + live.len());
                rows.extend(cache.row_items(&state.durable_rows));
                rows.extend(cache.row_items(&live));
                model.set_vec(rows);
                last_rows = state.rows_rev;
            } else {
                let durable_len = state.durable_rows.len();
                let total = durable_len + live.len();
                let have = model.row_count();
                let rows = cache.row_items(&live);
                for index in durable_len..total.min(have) {
                    model.set_row_data(index, rows[index - durable_len].clone());
                }
                for index in have..total {
                    model.push(rows[index - durable_len].clone());
                }
                while model.row_count() > total {
                    model.remove(total);
                }
            }
            last_live = state.live_rev;
            last_patch = state.patch_rev;
        } else if state.patch_rev != last_patch {
            if let Some(index) = state.patch_row {
                if let Some(data) = state.durable_rows.get(index) {
                    model.set_row_data(index, cache.row(data));
                }
            }
            last_patch = state.patch_rev;
        }
    });
    // Slint cancels a dropped timer, so it must outlive `build`.
    TICK_TIMER.with(|slot| {
        *slot.borrow_mut() = Some(timer);
    });
}

/// Seed the window with a representative transcript for layout review on a
/// desktop (the `wunder-android-preview` binary). Kept next to the real
/// projection so preview rows go through the same renderer.
pub fn install_preview_data(ui: &MobileWindow, model: &Rc<VecModel<TimelineRow>>) {
    use crate::project::{KIND_BODY, KIND_ERROR, KIND_FOOTER, KIND_REASON, KIND_TOOL, KIND_USER};

    let sessions = vec![
        MobileSession {
            id: "preview-1".into(),
            title: "移动端聊天区对齐".into(),
            preview: "心舰".into(),
            time: "11-06 14:20".into(),
            status: "running".into(),
            active: true,
        },
        MobileSession {
            id: "preview-2".into(),
            title: "检查无障碍控制链路".into(),
            preview: "心舰".into(),
            time: "11-05 09:12".into(),
            status: "idle".into(),
            active: false,
        },
    ];
    ui.set_sessions(ModelRc::from(Rc::new(VecModel::from(sessions))));
    ui.set_thread_title("移动端聊天区对齐".into());
    ui.set_thread_subtitle("running · 心舰".into());
    ui.set_workspace_name("心舰".into());
    ui.set_connection_state("本地引擎".into());
    ui.set_model_name("预览模型".into());
    ui.set_model_key("preview-chat".into());
    ui.set_preset_questions(ModelRc::from(Rc::new(VecModel::from(vec![
        slint::SharedString::from("继续"),
        slint::SharedString::from("总结一下当前进度"),
        slint::SharedString::from("检查一下最近的改动"),
    ]))));
    ui.set_inquiry_questions(ModelRc::from(Rc::new(VecModel::from(vec![InquiryQuestion {
        index: 0,
        question: "这次改动先发布到哪个渠道？".into(),
        multiple: false,
        no_preference: false,
        other: "".into(),
        options: ModelRc::new(VecModel::from(vec![
            InquiryOption {
                label: "内测".into(),
                description: "先给少量设备验证".into(),
                recommended: true,
                selected: true,
            },
            InquiryOption {
                label: "正式".into(),
                description: "直接替换线上包".into(),
                recommended: false,
                selected: false,
            },
        ])),
    }]))));
    ui.set_inquiry_page(0);
    ui.set_inquiry_total(2);
    ui.set_inquiry_ready(false);
    ui.set_host_cpu(0.32);
    ui.set_host_memory(0.58);
    ui.set_host_cpu_text("32%".into());
    ui.set_host_memory_text("58%".into());
    ui.set_host_memory_used("9.3G".into());
    ui.set_host_memory_total("16G".into());
    ui.set_host_cores(8);
    ui.set_running_task_count(1);
    ui.set_subagent_count(0);

    // Settings-page fixtures: enough rows to review the index, the catalogue
    // and the editor on a workstation without starting the engine.
    ui.set_settings_theme("terracotta".into());
    ui.set_settings_font_size(14);
    ui.set_settings_send_key("enter".into());
    ui.set_settings_language("zh-CN".into());
    ui.set_settings_workspace("应用私有目录 / workspaces".into());
    ui.set_settings_version(env!("CARGO_PKG_VERSION").into());
    ui.set_settings_ubuntu_state("installing".into());
    ui.set_settings_ubuntu_progress(42);
    ui.set_settings_ubuntu_stage("正在解压 Ubuntu 根文件系统…（4200 项）".into());
    ui.set_settings_ubuntu_enabled(true);
    let models = vec![
        preview_model_card(
            "preview-chat",
            "llm",
            "预览对话模型",
            "https://api.example.com/v1",
            true,
        ),
        preview_model_card(
            "preview-speech",
            "tts",
            "预览语音模型",
            "https://api.example.com/v1",
            false,
        ),
    ];
    ui.set_models(ModelRc::from(Rc::new(VecModel::from(models))));
    ui.set_model_catalogue_count(2);

    let mut rows: Vec<RowData> = Vec::new();
    let mut user = RowData::new(KIND_USER, "p-user");
    user.text = "把聊天区搬到手机上，样式和桌面版保持一致。".into();
    user.time = "14:20".into();
    rows.push(user);

    let mut process = RowData::new(crate::project::KIND_PROCESS, "p-process");
    process.text = "工作过程 · 2 步".into();
    process.summary = "已完成".into();
    process.foldable = true;
    process.open = true;
    process.process_children = vec![
        crate::project::ProcessChildData {
            id: "p-reason".into(),
            kind: KIND_REASON,
            tool_name: "思考".into(),
            summary: "先确认聊天组件的复用边界，再处理单窗口约束。".into(),
            detail: "聊天区必须直接 import 桌面版的 composer 与 timeline，避免出现第二套视觉。".into(),
            open: false,
            ..Default::default()
        },
        crate::project::ProcessChildData {
            id: "p-tool".into(),
            kind: KIND_TOOL,
            tool_name: "读取文件".into(),
            tool_icon: "file-lines".into(),
            target: "frontend-slint/ui/timeline.slint".into(),
            summary: "已读取 2.4k token".into(),
            detail: "读取 frontend-slint/ui/timeline.slint，共 1176 行。".into(),
            sections: vec![wunder_desktop::NativeWorkflowSection {
                title: "frontend-slint/ui/timeline.slint".into(),
                meta: "1176 行".into(),
                kind: "patch".into(),
                lines: vec![
                    wunder_desktop::NativeWorkflowLine {
                        ratio: 0.0,
                        text: "@@ -688,7 +688,7 @@ component ProcessBar".into(),
                        kind: "header".into(),
                        number: String::new(),
                    },
                    wunder_desktop::NativeWorkflowLine {
                        ratio: 0.0,
                        text: "height: root.data.open ? 40px + process-body.height : 34px;".into(),
                        kind: "add".into(),
                        number: "701".into(),
                    },
                    wunder_desktop::NativeWorkflowLine {
                        ratio: 0.0,
                        text: "height: root.data.open ? 36px + process-body.height : 34px;".into(),
                        kind: "delete".into(),
                        number: "701".into(),
                    },
                ],
            }],
            tokens: "2.4k token".into(),
            duration: "0.8s".into(),
            status_kind: crate::project::STATUS_DONE,
            open: true,
            ..Default::default()
        },
    ];
    rows.push(process);

    let mut body = RowData::new(KIND_BODY, "p-body");
    let text = "聊天区现在复用了桌面版的同一套组件：\n\n- `TimelineView` 负责消息流\n- `Composer` 负责输入与模型选择\n\n代码块样式也保持一致：\n\n```rust\nlet rows = project::project_turn(&turn);\n```\n\n> 手机上只保留单窗口与抽屉。";
    body.text = text.into();
    body.blocks_src = text.into();
    rows.push(body);

    let mut error = RowData::new(KIND_ERROR, "p-error");
    error.error_title = "执行失败".into();
    error.text = "LLM unavailable: LLM request failed".into();
    error.status_kind = 2;
    rows.push(error);

    let mut footer = RowData::new(KIND_FOOTER, "p-footer");
    footer.text = text.into();
    footer.stats = vec![
        ("hashtag".into(), "1".into()),
        ("stopwatch".into(), "3.2s".into()),
        ("gauge-high".into(), "18.4 token/s".into()),
        ("layer-group".into(), "12k".into()),
        ("bolt".into(), "1.8k".into()),
        ("screwdriver-wrench".into(), "2".into()),
        ("coins".into(), "1".into()),
    ];
    rows.push(footer);

    for (index, row) in rows.iter_mut().enumerate() {
        row.payload = index as i32;
        for (child_index, child) in row.process_children.iter_mut().enumerate() {
            child.payload = crate::project::PROCESS_CHILD_BASE
                .wrapping_add((index as i32).wrapping_shl(crate::project::PROCESS_CHILD_SHIFT))
                .wrapping_add(child_index as i32);
        }
    }
    let mut renderer = MarkdownRenderer::new();
    model.set_vec(
        rows.iter()
            .map(|row| row.to_row(&mut renderer))
            .collect::<Vec<TimelineRow>>(),
    );
    ui.set_stream_revision(1);
}

fn preview_model_card(
    key: &str,
    kind: &str,
    name: &str,
    base_url: &str,
    is_default: bool,
) -> ModelCard {
    let mut card = ModelCard::default();
    card.key = key.into();
    card.provider = "openai_compatible".into();
    card.model = name.into();
    card.base_url = base_url.into();
    card.model_type = kind.into();
    card.is_default = is_default;
    card.temperature = "0.7".into();
    card.timeout_s = "120".into();
    card.max_output = "32768".into();
    card.max_context = "128000".into();
    card.tool_call_mode = "function_call".into();
    card.support_vision = true;
    card.stream_include_usage = true;
    card
}

/// Opens one of the settings pages in the desktop preview so layout review can
/// reach the sheet without an engine: `settings`, `models` or `editor`.
pub fn open_preview_page(ui: &MobileWindow, page: &str) {
    match page {
        "settings" => {
            ui.set_settings_page(0);
            ui.set_settings_open(true);
        }
        "models" => {
            ui.set_settings_page(1);
            ui.set_settings_open(true);
        }
        "hud" => {
            ui.set_host_detail_open(true);
        }
        "trajectory" | "trajectory-tools" => {
            let filter = if page == "trajectory-tools" { 2 } else { 0 };
            ui.set_trajectory_title("移动端聊天区对齐".into());
            ui.set_trajectory_session_title("预览线程".into());
            ui.set_trajectory_error("".into());
            ui.set_trajectory_loading(false);
            ui.set_trajectory_filter(filter);
            ui.set_trajectory_expanded(-1);
            ui.set_trajectory_items(ModelRc::from(Rc::new(VecModel::from(
                filtered_trajectory(&preview_trajectory(), filter),
            ))));
            let (audit, segments) = preview_trajectory_audit();
            ui.set_trajectory_audit(audit);
            ui.set_trajectory_segments(ModelRc::from(Rc::new(VecModel::from(segments))));
            ui.set_trajectory_open(true);
        }
        "editor" => {
            let card = preview_model_card(
                "preview-chat",
                "llm",
                "预览对话模型",
                "https://api.example.com/v1",
                true,
            );
            ui.set_settings_page(1);
            ui.set_settings_open(true);
            ui.set_editor_key(card.key.clone());
            ui.set_editor_new(false);
            ui.set_editor_default(true);
            ui.set_editor_draft(card);
            ui.set_editor_secret("".into());
            ui.set_editor_probe("探测成功：上下文 128000".into());
            ui.set_editor_open(true);
            // The preview window is not running yet, so the sheet's
            // `changed editor-open` handler has not been installed; select
            // the editor page explicitly.
            ui.set_settings_page(2);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: i32, group: &str) -> crate::TrajectoryItem {
        let mut item = crate::TrajectoryItem::default();
        item.kind = kind;
        item.group = group.into();
        item
    }

    #[test]
    fn trajectory_tool_filter_keeps_its_turn_label() {
        let rows = vec![
            item(0, "第 1 轮"),
            item(1, ""),
            item(2, ""),
            item(0, "第 2 轮"),
            item(2, ""),
        ];
        let tools = filtered_trajectory(&rows, 2);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].group, "第 1 轮");
        assert_eq!(tools[1].group, "第 2 轮");
        let conversation = filtered_trajectory(&rows, 1);
        assert_eq!(conversation.len(), 3);
        assert_eq!(conversation[0].group, "第 1 轮");
        assert_eq!(conversation[1].group, "");
        assert_eq!(conversation[2].group, "第 2 轮");
    }
}
