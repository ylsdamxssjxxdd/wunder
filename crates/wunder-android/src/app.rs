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
    CommandTask, DeviceScale, InquiryOption, InquiryQuestion, MobileBackup, MobilePermission,
    MobileSession, MobileWindow, MobileWorkspace, ModelCard, SubagentItem, Theme, TimelineRow,
    TurnMark, WorkspaceFile,
};
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(33);
/// 软键盘跟随的帧间隔：与 60Hz 的屏幕刷新对齐。
const IME_TICK: Duration = Duration::from_millis(16);
const CACHE_LIMIT: usize = 800;

pub fn build() -> MobileWindow {
    let (ui, model) = create_window();
    // The greeting mark ships one render per device-pixel ratio and picks the
    // one that lands on the phone's physical grid, so leaving this at the 1.0
    // default would upscale a 40px asset. The window is only mapped once
    // `run()` starts, hence the deferred read instead of one here.
    {
        let weak = ui.as_weak();
        Timer::single_shot(Duration::ZERO, move || {
            if let Some(ui) = weak.upgrade() {
                ui.global::<DeviceScale>().set_factor(ui.window().scale_factor());
            }
        });
    }
    // The engine outlives this window; a recreated activity attaches to the
    // same transcript and any turn that is still streaming.
    let handle = engine::acquire();
    let shared = handle.shared.clone();
    let commands = handle.command_sender();
    wire_callbacks(&ui, &commands, &shared);
    // 内嵌使用手册：目录与正文都由 CPU 侧投影，跟引擎无关，窗口一建好就绑上。
    crate::help::attach(&ui);
    start_tick(&ui, &shared, &model, commands);
    ui
}

thread_local! {
    // The tick timer belongs to one native activity thread. Android runs every
    // activity instance on its own native thread, so a thread-local slot drops
    // the previous timer when that activity goes away instead of leaking one
    // timer per recreation.
    static TICK_TIMER: std::cell::RefCell<Option<Timer>> = const { std::cell::RefCell::new(None) };
    /// The IME follow heartbeat, only alive while the keyboard animates.
    static IME_TIMER: std::cell::RefCell<Option<Timer>> = const { std::cell::RefCell::new(None) };
    /// The keyboard height already applied, plus how many frames in a row it
    /// has stayed there.
    static IME_STATE: std::cell::Cell<(f32, u8)> =
        const { std::cell::Cell::new((0.0, 0)) };
}

/// 当前软键盘高度。Slint 的 virtual-keyboard 矩形就是系统逐帧动画值。
fn ime_height(ui: &MobileWindow) -> f32 {
    ui.window()
        .virtual_keyboard(i_slint_core::InternalToken)
        .map(|(_origin, size)| size.height)
        .unwrap_or(0.0)
}

/// 已经落到布局里的键盘高度。
fn ime_applied() -> f32 {
    IME_STATE.with(|state| state.get().0)
}

/// 记下落定的键盘高度，并清空「连续没变化」的计数。
fn ime_settle(height: f32) {
    IME_STATE.with(|state| state.set((height, 0)));
}

/// 逐帧跟随软键盘。Android 的 WindowInsetsAnimation 每帧回调一次，键盘弹出/
/// 收起整段都有中间值；主心跳 33ms 采样只会得到 30Hz 的台阶，而「等动画落定
/// 再落一次布局」会让输入区在整段动画里被键盘盖住、结束时才猛地跳出来。所以
/// 键盘一动就切到 16ms 的跟随心跳，把每一帧都落进 `ime-height`；连续两拍不变
/// 即认为键盘停稳，立刻停表，空闲时不产生任何额外唤醒。
fn start_ime_follow(ui: &MobileWindow) {
    let weak = ui.as_weak();
    IME_TIMER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let timer = slot.get_or_insert_with(Timer::default);
        if timer.running() {
            return;
        }
        timer.start(TimerMode::Repeated, IME_TICK, move || {
            let Some(ui) = weak.upgrade() else {
                stop_ime_follow();
                return;
            };
            let ime = ime_height(&ui);
            let (applied, steady) = IME_STATE.with(|state| state.get());
            if (ime - applied).abs() > 0.5 {
                ui.set_ime_height(ime);
                ime_settle(ime);
            } else if steady >= 2 {
                ime_settle(ime);
                stop_ime_follow();
            } else {
                IME_STATE.with(|state| state.set((applied, steady + 1)));
            }
        });
    });
}

fn stop_ime_follow() {
    IME_TIMER.with(|slot| {
        if let Some(timer) = slot.borrow().as_ref() {
            timer.stop();
        }
    });
}

/// Window without the engine, used by the desktop preview binary. The layout
/// and every shared component are identical to the phone build.
pub fn build_preview() -> MobileWindow {
    let (ui, model) = create_window();
    let workspaces = Rc::new(VecModel::<MobileWorkspace>::from(Vec::new()));
    install_preview_data(&ui, &model, &workspaces);
    let model_for_toggle = model.clone();
    ui.on_toggle_row(move |index| {
        if let Some((bar, child)) = project::process_child_address(index) {
            if let Some(row) = model_for_toggle.row_data(bar) {
                if let Some(entry) = row.process_entries.row_data(child) {
                    let mut entry = entry;
                    entry.open = !entry.open;
                    // 用户亲手点过：此后自动开合不再改动这一条。
                    entry.manual = true;
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
        workspace: "心舰".into(),
        time: "10-28 18:40".into(),
        status: "idle".into(),
        active: false,
    }]));
    ui.set_archived_sessions(ModelRc::from(archived.clone()));
    ui.set_archived_total(1);
    let weak = ui.as_weak();
    ui.on_open_drawer(move || {
        if let Some(ui) = weak.upgrade() {
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
    ui.on_env_setup_dismiss(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_env_setup_open(false);
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
            let (audit, segments, bars) = preview_trajectory_audit();
            ui.set_trajectory_audit(audit);
            ui.set_trajectory_segments(ModelRc::from(Rc::new(VecModel::from(segments))));
            ui.set_trajectory_bars(ModelRc::from(Rc::new(VecModel::from(bars))));
        }
    });
    let weak = ui.as_weak();
    ui.on_trajectory_filter_changed(move |filter| {
        if let Some(ui) = weak.upgrade() {
            ui.set_trajectory_items(ModelRc::from(Rc::new(VecModel::from(filtered_trajectory(
                &preview_trajectory(),
                filter,
            )))));
        }
    });
    let weak = ui.as_weak();
    ui.on_load_prompt_preview(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_prompt_preview_loading(false);
            ui.set_prompt_preview(preview_system_prompt().into());
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
    ui.on_clear_workspace(|| {});
    ui.on_upload_workspace(|_| {});
    ui.on_download_workspace(|_| {});
    ui.on_load_more_workspace(|_, _| {});
    ui.on_workspace_rename(|_, _| {});
    ui.on_workspace_move(|_, _| {});
    ui.on_workspace_delete(|_| {});
    ui.on_open_workspace_file(|_| {});
    ui.on_open_workspace_location(|_, _| {});
    ui.on_open_workspaces(|| {});
    ui.on_refresh_workspaces(|| {});
    let workspaces_for_select = workspaces.clone();
    let weak = ui.as_weak();
    ui.on_select_workspace(move |id| {
        if let Some(ui) = weak.upgrade() {
            for index in 0..workspaces_for_select.row_count() {
                if let Some(mut workspace) = workspaces_for_select.row_data(index) {
                    workspace.active = workspace.id == id;
                    workspaces_for_select.set_row_data(index, workspace.clone());
                    if workspace.active {
                        ui.set_workspace_name(workspace.name);
                        ui.set_workspace_id(workspace.id.clone());
                        ui.set_workspace_icon(workspace.icon);
                        ui.set_workspace_color(workspace.color);
                        ui.set_workspace_thread_count(workspace.thread_count);
                    }
                }
            }
        }
    });
    let workspaces_for_new_task = workspaces.clone();
    ui.on_new_task_in_workspace(move |id| {
        for index in 0..workspaces_for_new_task.row_count() {
            if let Some(mut workspace) = workspaces_for_new_task.row_data(index) {
                workspace.active = workspace.id == id;
                workspaces_for_new_task.set_row_data(index, workspace);
            }
        }
    });
    let workspaces_for_create = workspaces.clone();
    let weak = ui.as_weak();
    ui.on_create_workspace(move |name, icon, color, folder| {
        if let Some(ui) = weak.upgrade() {
            // The editor sends an empty visual pair for new workspaces; the real
            // backend draws one, so the preview mirrors that instead of showing
            // the gray fallback.
            let (auto_icon, auto_color) = wunder_desktop::random_workspace_visual();
            let icon = if icon.is_empty() {
                auto_icon.into()
            } else {
                icon
            };
            let color = if color.is_empty() {
                auto_color.into()
            } else {
                color
            };
            let id: slint::SharedString =
                format!("preview-ws-{}", workspaces_for_create.row_count() + 1).into();
            let active_id = id.clone();
            for index in 0..workspaces_for_create.row_count() {
                if let Some(mut workspace) = workspaces_for_create.row_data(index) {
                    workspace.active = false;
                    workspaces_for_create.set_row_data(index, workspace);
                }
            }
            workspaces_for_create.push(MobileWorkspace {
                id,
                name: name.clone(),
                icon: icon.clone(),
                color: color.clone(),
                root: if folder.is_empty() {
                    "应用工作区 / preview".into()
                } else {
                    folder
                },
                thread_count: 0,
                active: true,
            });
            ui.set_workspace_name(name);
            ui.set_workspace_id(active_id);
            ui.set_workspace_icon(icon);
            ui.set_workspace_color(color);
            ui.set_workspace_thread_count(0);
        }
    });
    let workspaces_for_update = workspaces.clone();
    ui.on_update_workspace(move |id, name, icon, color, folder| {
        for index in 0..workspaces_for_update.row_count() {
            if let Some(mut workspace) = workspaces_for_update.row_data(index) {
                if workspace.id == id {
                    workspace.name = name.clone();
                    workspace.icon = icon.clone();
                    workspace.color = color.clone();
                    if !folder.is_empty() {
                        workspace.root = folder.clone();
                    }
                    workspaces_for_update.set_row_data(index, workspace);
                }
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_pick_workspace_folder(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_workspace_picked_folder("/storage/emulated/0/Download/预览工作区".into());
        }
    });
    let workspaces_for_delete = workspaces.clone();
    let weak = ui.as_weak();
    ui.on_delete_workspace(move |id| {
        if let Some(ui) = weak.upgrade() {
            let mut kept = Vec::new();
            for index in 0..workspaces_for_delete.row_count() {
                if let Some(workspace) = workspaces_for_delete.row_data(index) {
                    if workspace.id != id {
                        kept.push(workspace);
                    }
                }
            }
            if !kept.iter().any(|workspace| workspace.active) {
                if let Some(first) = kept.first_mut() {
                    first.active = true;
                    ui.set_workspace_name(first.name.clone());
                    ui.set_workspace_id(first.id.clone());
                    ui.set_workspace_icon(first.icon.clone());
                    ui.set_workspace_color(first.color.clone());
                    ui.set_workspace_thread_count(first.thread_count);
                }
            }
            workspaces_for_delete.set_vec(kept);
        }
    });
    ui.on_keyboard_dismiss(|| {});
    // 预览壳没有引擎，所以"发送"在这里只做回显：清空草稿、把这句作为一条
    // 用户行落到时间线底部，看起来就是发出去了，不会停在输入区。
    let weak = ui.as_weak();
    let model_for_send = model.clone();
    let mut send_seq = 0usize;
    ui.on_send(move || {
        let Some(ui) = weak.upgrade() else { return };
        let text = ui.get_draft().trim().to_string();
        if text.is_empty() {
            return;
        }
        ui.set_draft("".into());
        ui.set_follow_output(true);
        send_seq += 1;
        let mut row = RowData::new(project::KIND_USER, format!("preview-send-{send_seq}"));
        row.text = text.into();
        row.time = "刚刚".into();
        let mut renderer = MarkdownRenderer::new();
        model_for_send.push(row.to_row(&mut renderer));
    });
    crate::help::attach(&ui);
    ui
}

/// Representative frozen prompt for the preview binary. The real projection
/// comes from the engine; this keeps the sheet reviewable without a runtime.
fn preview_system_prompt() -> &'static str {
    "你是 wunder 智能体。先确认目标，再拆解任务并逐步执行。\n\n\
# 行为准则\n\
- 先理解目标，再调用工具。\n\
- 每次工具调用后说明结果。\n\
- 遇到无法完成的操作，如实说明原因。\n\n\
# 工具\n\
- 读取文件：read_file\n\
- 运行命令：execute_command\n\
- 网页搜索：web_search\n\n\
# 运行环境\n\
- 本地工作区：应用私有工作区\n\
- 运行环境：Ubuntu 24.04（默认内置）\n\n\
# 输出\n\
- 使用简洁的中文回复。\n\
- 代码块标注语言。\n"
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
            workspace: "心舰".into(),
            time: "11-06 14:20".into(),
            status: "running".into(),
            active: true,
        },
        MobileSession {
            id: "preview-2".into(),
            title: "检查无障碍控制链路".into(),
            workspace: "心舰".into(),
            time: "11-05 09:12".into(),
            status: "idle".into(),
            active: false,
        },
        MobileSession {
            id: "preview-3".into(),
            title: "整理运行时补充包".into(),
            workspace: "心舰".into(),
            time: "11-04 21:08".into(),
            status: "idle".into(),
            active: false,
        },
    ]
}

/// Mobile filter: 0 all, 1 conversation, 2 tools, 3 system. When a filter
/// drops the user row that carried the turn label, the first surviving row of
/// that turn inherits the label so filtered views keep their grouping.
fn filtered_trajectory(items: &[crate::TrajectoryItem], filter: i32) -> Vec<crate::TrajectoryItem> {
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
                "tool_name": "读取文件", "summary": "crates/wunder-desktop/frontend/ui/timeline.slint",
                "args_preview": "{\"path\":\"crates/wunder-desktop/frontend/ui/timeline.slint\"}",
                "result_preview": "eyJhcmdzIjp7InBhdGgiOiJjcmF0ZXMvd3VuZGVyLWRlc2t0b3AvZnJvbnRlbmQvdWkvdGltZWxpbmUuc2xpbnQiLCJvZmZzZXQiOjAsImxpbWl0IjoyMDAsImVuY29kaW5nIjoidXRmLTgiLCJ3cmFwIjoid29yZC13cmFwIiwicmVuZGVyZXIiOiJza2lhIiwiZGV2aWNlIjoiYW5kcm9pZCIsImZvbnQiOiJNaWNyb3NvZnQgWWFIZWkifX0=".repeat(4),
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
                "tool_name": "编辑文件", "summary": "crates/wunder-desktop/frontend/ui/composer.slint",
                "args_preview": "{\"path\":\"crates/wunder-desktop/frontend/ui/composer.slint\"}",
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

fn preview_trajectory_audit() -> (
    crate::TrajectoryAudit,
    Vec<crate::TrajectorySegment>,
    Vec<crate::TrajectoryBar>,
) {
    let projection = engine::trajectory_projection(&preview_trajectory_page());
    (projection.audit, projection.segments, projection.bars)
}

/// 预览用的二维码示意位图。预览进程不联网、也不会真的被扫，三只定位角加
/// 一段固定伪随机数据，只用来核对 220px 容器里的观感与状态覆盖层。
fn preview_qr_image() -> slint::Image {
    const MODULES: u32 = 33;
    const QUIET: u32 = 4;
    const SCALE: u32 = 8;
    let mut modules = vec![false; (MODULES * MODULES) as usize];
    let finder = |modules: &mut Vec<bool>, x0: u32, y0: u32| {
        for y in 0..7u32 {
            for x in 0..7u32 {
                let edge = x == 0 || x == 6 || y == 0 || y == 6;
                let core = (2..=4).contains(&x) && (2..=4).contains(&y);
                if edge || core {
                    modules[((y0 + y) * MODULES + (x0 + x)) as usize] = true;
                }
            }
        }
    };
    finder(&mut modules, 0, 0);
    finder(&mut modules, MODULES - 7, 0);
    finder(&mut modules, 0, MODULES - 7);
    let mut seed = 0x9e37_79b9u32;
    for y in 0..MODULES {
        for x in 0..MODULES {
            let near_finder =
                (x < 8 && y < 8) || (x >= MODULES - 8 && y < 8) || (x < 8 && y >= MODULES - 8);
            if near_finder {
                continue;
            }
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if (seed >> 9) & 1 == 1 {
                modules[(y * MODULES + x) as usize] = true;
            }
        }
    }
    let side = (MODULES + QUIET * 2) * SCALE;
    let mut rgba = vec![255u8; (side * side * 4) as usize];
    for y in 0..MODULES {
        for x in 0..MODULES {
            if !modules[(y * MODULES + x) as usize] {
                continue;
            }
            let px = (x + QUIET) * SCALE;
            let py = (y + QUIET) * SCALE;
            for dy in 0..SCALE {
                for dx in 0..SCALE {
                    let offset = (((py + dy) * side + (px + dx)) * 4) as usize;
                    rgba[offset] = 0;
                    rgba[offset + 1] = 0;
                    rgba[offset + 2] = 0;
                }
            }
        }
    }
    slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
        &rgba, side, side,
    ))
}

fn create_window() -> (MobileWindow, Rc<VecModel<TimelineRow>>) {
    crate::fonts::install();
    let ui = MobileWindow::new().expect("create the mobile window");
    // 手机默认字号 16px（设置页里的同一数值），首帧就按它排版，
    // 免得设置投影到达前先闪一帧 14px 的小字。
    ui.global::<Theme>().set_font_scale(16.0 / 14.0);
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
    // 界面语言：窗口从 `settings-language` 派生出 `ui-zh` 后回传，这里落到
    // `I18n.zh`。Slint 组件不能直接写全局，所以由宿主承接这一跳。
    let weak = ui.as_weak();
    ui.on_ui_language_changed(move |zh| {
        if let Some(ui) = weak.upgrade() {
            let i18n = ui.global::<crate::I18n>();
            if i18n.get_zh() != zh {
                i18n.set_zh(zh);
            }
            crate::help::set_language(&ui, zh);
        }
    });
    // Back 走 Slint 按键派发时（系统预测性返回的 Slint 回调、旧版输入队列、
    // 硬件返回键）只记一笔，页面状态与键盘状态统一在渲染 tick 里读。
    #[cfg(target_os = "android")]
    ui.on_back_requested(|| crate::android_host::request_back());

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_open_drawer(move || {
        let _ = sender.send(Command::RefreshSessions);
        let _ = sender.send(Command::RefreshArchived);
        // The drawer shows the working directory inline; refresh it alongside
        // the thread list so the files are never a stale snapshot.
        let _ = sender.send(Command::RefreshWorkspace);
        if let Some(ui) = weak.upgrade() {
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
        let cloud = weak
            .upgrade()
            .map(|ui| ui.get_settings_chat_target() == 1)
            .unwrap_or(false);
        if cloud {
            let _ = sender.send(Command::CloudSelectThread(id.to_string()));
        } else {
            let _ = sender.send(Command::Select(id.to_string()));
        }
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_rename_session(move |id, title| {
        let title = title.trim().to_string();
        if title.is_empty() {
            return;
        }
        if weak
            .upgrade()
            .map(|ui| ui.get_settings_chat_target() == 1)
            .unwrap_or(false)
        {
            let _ = sender.send(Command::CloudRename {
                id: id.to_string(),
                title,
            });
        } else {
            let _ = sender.send(Command::RenameSession {
                id: id.to_string(),
                title,
            });
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_archive_session(move |id| {
        if weak
            .upgrade()
            .map(|ui| ui.get_settings_chat_target() == 1)
            .unwrap_or(false)
        {
            let _ = sender.send(Command::CloudArchive(id.to_string()));
        } else {
            let _ = sender.send(Command::ArchiveSession(id.to_string()));
        }
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
    let weak = ui.as_weak();
    ui.on_load_trajectory(move |id, title| {
        if weak
            .upgrade()
            .map(|ui| ui.get_settings_chat_target() == 1)
            .unwrap_or(false)
        {
            return;
        }
        let _ = sender.send(Command::LoadTrajectory {
            id: id.to_string(),
            title: title.to_string(),
        });
    });

    let sender = commands.clone();
    ui.on_load_prompt_preview(move || {
        let _ = sender.send(Command::LoadPromptPreview);
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
    let sender = commands.clone();
    ui.on_clear_workspace(move || {
        let _ = sender.send(Command::ClearWorkspace);
    });
    ui.on_upload_workspace(move |dir| {
        #[cfg(target_os = "android")]
        crate::android_host::pick_files(&dir);
        #[cfg(not(target_os = "android"))]
        let _ = dir;
    });
    // 聊天页「上传文件」：同一套上传，但文件会挂在输入框上随消息告知模型。
    ui.on_attach_files(move || {
        #[cfg(target_os = "android")]
        crate::android_host::pick_attachment_files("");
    });
    // 拍照：相机的输出先落进应用缓存，再由引擎搬进工作区的
    // temp/chat_media/source，和上传一样挂到这条对话上。
    ui.on_take_photo(move || {
        #[cfg(target_os = "android")]
        crate::android_host::take_photo(crate::engine::CHAT_MEDIA_SOURCE);
    });
    // 按住说话：按下开录、松手结束、上滑取消。手机上不保留桌面版那种
    // 「再点一次停止」的切换，按住/松手更接近聊天软件的手感。
    let sender = commands.clone();
    ui.on_voice_start(move || {
        let _ = sender.send(Command::VoiceStart);
    });
    let sender = commands.clone();
    ui.on_voice_stop(move || {
        let _ = sender.send(Command::VoiceStop);
    });
    let sender = commands.clone();
    ui.on_voice_cancel(move || {
        let _ = sender.send(Command::VoiceCancel);
    });
    let sender = commands.clone();
    ui.on_remove_attachment(move |index| {
        let _ = sender.send(Command::RemoveUpload(index.max(0) as usize));
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
    ui.on_workspace_rename(move |path, name| {
        let _ = sender.send(Command::RenameWorkspaceEntry {
            path: path.to_string(),
            name: name.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_workspace_move(move |path, target_dir| {
        let _ = sender.send(Command::MoveWorkspaceEntry {
            path: path.to_string(),
            target_dir: target_dir.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_workspace_delete(move |path| {
        let _ = sender.send(Command::DeleteWorkspaceEntry {
            path: path.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_open_workspace_file(move |path| {
        let _ = sender.send(Command::OpenWorkspaceFile {
            path: path.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_open_workspace_location(move |path, dir| {
        let _ = sender.send(Command::OpenWorkspaceLocation {
            path: path.to_string(),
            dir,
        });
    });

    let sender = commands.clone();
    ui.on_open_workspaces(move || {
        let _ = sender.send(Command::RefreshWorkspaces);
    });
    let sender = commands.clone();
    ui.on_refresh_workspaces(move || {
        let _ = sender.send(Command::RefreshWorkspaces);
    });
    let sender = commands.clone();
    ui.on_select_workspace(move |id| {
        let _ = sender.send(Command::SelectWorkspace(id.to_string()));
    });
    let sender = commands.clone();
    ui.on_new_task_in_workspace(move |id| {
        let _ = sender.send(Command::NewThreadInWorkspace(id.to_string()));
    });
    let sender = commands.clone();
    ui.on_create_workspace(move |name, icon, color, folder| {
        let _ = sender.send(Command::CreateWorkspace {
            name: name.to_string(),
            icon: icon.to_string(),
            color: color.to_string(),
            folder: folder.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_update_workspace(move |id, name, icon, color, folder| {
        let _ = sender.send(Command::UpdateWorkspace {
            id: id.to_string(),
            name: name.to_string(),
            icon: icon.to_string(),
            color: color.to_string(),
            folder: folder.to_string(),
        });
    });
    let weak = ui.as_weak();
    ui.on_pick_workspace_folder(move || {
        if let Some(ui) = weak.upgrade() {
            // Clear first so picking the same folder twice still reaches the
            // editor through its change handler.
            ui.set_workspace_picked_folder("".into());
        }
        #[cfg(target_os = "android")]
        crate::android_host::pick_directory();
    });
    let sender = commands.clone();
    ui.on_delete_workspace(move |id| {
        let _ = sender.send(Command::DeleteWorkspace(id.to_string()));
    });

    let sender = commands.clone();
    ui.on_trajectory_filter_changed(move |filter| {
        let _ = sender.send(Command::TrajectoryFilter(filter));
    });

    let sender = commands.clone();
    ui.on_trajectory_export(move || {
        let _ = sender.send(Command::ExportTrajectoryLog);
    });

    ui.on_keyboard_dismiss(move || {
        #[cfg(target_os = "android")]
        crate::android_host::hide_keyboard();
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_new_session(move || {
        let cloud = weak
            .upgrade()
            .map(|ui| ui.get_settings_chat_target() == 1)
            .unwrap_or(false);
        let _ = if cloud {
            sender.send(Command::CloudNewThread)
        } else {
            sender.send(Command::NewThread)
        };
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_fork_thread(move |round| {
        let cloud = weak
            .upgrade()
            .map(|ui| ui.get_settings_chat_target() == 1)
            .unwrap_or(false);
        let _ = if cloud {
            sender.send(Command::CloudFork(round))
        } else {
            sender.send(Command::Fork(round))
        };
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_send(move || {
        let Some(ui) = weak.upgrade() else { return };
        // 安卓的英文会停在输入法的组合区（屏幕上带下划线），直到遇到分隔符才
        // 提交给 `text`。发送前先把组合区落进正文：否则 `draft` 是空的，消息
        // 会丢掉用户明明看得见的那几个字母，发送键也一直是语音键。
        #[cfg(target_os = "android")]
        i_slint_core::items::commit_focused_preedit(ui.window());
        let draft = ui.get_draft().to_string();
        let draft = draft.trim().to_string();
        // 只挂了附件、正文空着也能发：文件提示块本身就是这条消息的内容。
        if draft.is_empty() && ui.get_attachments().row_count() == 0 {
            return;
        }
        if ui.get_settings_chat_target() == 1 {
            let _ = sender.send(Command::CloudSend(draft));
        } else {
            let _ = sender.send(Command::Send(draft));
        }
        ui.set_draft("".into());
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_stop(move || {
        if weak
            .upgrade()
            .map(|ui| ui.get_settings_chat_target() == 1)
            .unwrap_or(false)
        {
            let _ = sender.send(Command::CloudStop);
        } else {
            let _ = sender.send(Command::Stop);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_select_model(move |_index, key| {
        if weak
            .upgrade()
            .map(|ui| ui.get_settings_chat_target() == 1)
            .unwrap_or(false)
        {
            return;
        }
        let _ = sender.send(Command::SelectModel(key.to_string()));
    });

    // 浮层里的后台任务 / 子智能体行：终止命令、中断子智能体都回到引擎，
    // 权限与结算语义仍由运行时决定（手机只转发）。
    let sender = commands.clone();
    ui.on_stop_task(move |id| {
        let _ = sender.send(Command::TerminateTask(id.to_string()));
    });
    let sender = commands.clone();
    ui.on_stop_subagent(move |id| {
        let _ = sender.send(Command::StopSubagent(id.to_string()));
    });

    // ---- settings sheet ----------------------------------------------
    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_open_settings(move |page| {
        log::info!("open settings page={page}");
        let _ = sender.send(Command::LoadSettings);
        let _ = sender.send(Command::RefreshArchived);
        let _ = sender.send(Command::RefreshUbuntu);
        let _ = sender.send(Command::RefreshCompanion);
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
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
    ui.on_refresh_permissions(move || {
        let _ = sender.send(Command::RefreshPermissions);
    });

    let sender = commands.clone();
    ui.on_open_permission(move |id| {
        let _ = sender.send(Command::OpenPermission(id.to_string()));
    });

    let sender = commands.clone();
    ui.on_refresh_backup(move || {
        let _ = sender.send(Command::RefreshBackup);
    });
    let sender = commands.clone();
    ui.on_backup_now(move || {
        let _ = sender.send(Command::BackupNow);
    });
    let sender = commands.clone();
    ui.on_restore_backup(move |path| {
        let _ = sender.send(Command::RestoreBackup {
            path: path.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_view_crash_log(move || {
        let _ = sender.send(Command::LoadCrashLog);
    });

    let sender = commands.clone();
    ui.on_check_update(move || {
        let _ = sender.send(Command::CheckAndroidUpdate);
    });
    let sender = commands.clone();
    ui.on_download_update(move || {
        let _ = sender.send(Command::DownloadAndroidUpdate);
    });
    let sender = commands.clone();
    ui.on_install_update(move || {
        let _ = sender.send(Command::InstallAndroidUpdate);
    });
    let sender = commands.clone();
    ui.on_select_update_source(move |source| {
        let _ = sender.send(Command::SetUpdateSource(source.to_string()));
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_save_theme(move |theme| {
        let Some(ui) = weak.upgrade() else { return };
        let _ = sender.send(Command::SavePreferences {
            theme: theme.to_string(),
            font_size: ui.get_settings_font_size(),
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
            font_size,
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
    ui.on_env_setup_retry(move || {
        let _ = sender.send(Command::InstallUbuntu);
    });
    let shared_for_dismiss = shared.clone();
    let weak = ui.as_weak();
    ui.on_env_setup_dismiss(move || {
        if let Ok(mut state) = shared_for_dismiss.lock() {
            state.env_setup_dismissed = true;
        }
        if let Some(ui) = weak.upgrade() {
            ui.set_env_setup_open(false);
        }
    });

    let sender = commands.clone();
    ui.on_save_companion(move |enabled, selected_id, scale, message_hints| {
        let _ = sender.send(Command::SaveCompanion {
            enabled,
            selected_id: selected_id.to_string(),
            scale,
            message_hints,
        });
    });

    let sender = commands.clone();
    ui.on_request_companion_permission(move || {
        let _ = sender.send(Command::CompanionPermission);
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

    // ---- tool manager ------------------------------------------------
    let sender = commands.clone();
    ui.on_refresh_tools(move || {
        let _ = sender.send(Command::RefreshTools);
    });
    let sender = commands.clone();
    ui.on_select_tool_server(move |index| {
        let _ = sender.send(Command::SelectToolServer { index });
    });
    let sender = commands.clone();
    ui.on_select_tool_resource(move |tab, index| {
        let _ = sender.send(Command::SelectToolResource { tab, index });
    });
    let sender = commands.clone();
    ui.on_load_tool_directory(move |directory| {
        let _ = sender.send(Command::LoadToolDirectory {
            directory: directory.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_more_tool_files(move || {
        let _ = sender.send(Command::MoreToolFiles);
    });
    let sender = commands.clone();
    ui.on_open_tool_file(move |path| {
        let _ = sender.send(Command::OpenToolFile {
            path: path.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_save_tool_file(move |path, content| {
        let _ = sender.send(Command::SaveToolFile {
            path: path.to_string(),
            content: content.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_close_tool_file(move || {
        let _ = sender.send(Command::CloseToolFile);
    });
    let sender = commands.clone();
    ui.on_create_tool_file(move |name| {
        let _ = sender.send(Command::CreateToolFile {
            name: name.to_string(),
        });
    });
    ui.on_upload_tool_files(move || {
        #[cfg(target_os = "android")]
        crate::android_host::pick_tool_files();
    });
    let sender = commands.clone();
    ui.on_delete_tool_resource(move |tab, name| {
        let _ = sender.send(Command::DeleteToolResource {
            tab,
            name: name.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_connect_tool_server(move |name| {
        let _ = sender.send(Command::ConnectToolServer {
            name: name.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_export_skill(move |name| {
        let _ = sender.send(Command::ExportSkill {
            name: name.to_string(),
        });
    });
    ui.on_import_skill(move || {
        #[cfg(target_os = "android")]
        crate::android_host::pick_skill_archive();
    });
    let sender = commands.clone();
    ui.on_open_tool_editor(move |kind, index| {
        let _ = sender.send(Command::OpenToolEditor { kind, index });
    });
    let sender = commands.clone();
    ui.on_close_tool_editor(move || {
        let _ = sender.send(Command::CloseToolEditor);
    });
    let sender = commands.clone();
    ui.on_save_tool_editor(move |kind, mcp, kb| {
        let _ = sender.send(Command::SaveToolEditor { kind, mcp, kb });
    });

    // ---- built-in agent ----------------------------------------------
    let sender = commands.clone();
    ui.on_refresh_agent(move || {
        let _ = sender.send(Command::RefreshAgent);
    });
    let sender = commands.clone();
    ui.on_search_agent_tools(move |query| {
        let _ = sender.send(Command::AgentToolSearch(query.to_string()));
    });
    let sender = commands.clone();
    ui.on_save_agent_text(move |description, prompt| {
        let _ = sender.send(Command::SaveAgentText {
            description: description.to_string(),
            system_prompt: prompt.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_toggle_agent_tool(move |name, enabled, description, prompt| {
        let _ = sender.send(Command::ToggleAgentTool {
            name: name.to_string(),
            enabled,
            description: description.to_string(),
            system_prompt: prompt.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_toggle_agent_tool_group(move |category, select, description, prompt| {
        let _ = sender.send(Command::ToggleAgentToolGroup {
            category: category.to_string(),
            select,
            description: description.to_string(),
            system_prompt: prompt.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_add_agent_question(move |question, description, prompt| {
        let _ = sender.send(Command::AddAgentQuestion {
            question: question.to_string(),
            description: description.to_string(),
            system_prompt: prompt.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_remove_agent_question(move |index, description, prompt| {
        let _ = sender.send(Command::RemoveAgentQuestion {
            index,
            description: description.to_string(),
            system_prompt: prompt.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_set_agent_approval_mode(move |mode| {
        let _ = sender.send(Command::SetAgentApprovalMode(mode.to_string()));
    });
    let sender = commands.clone();
    ui.on_decide_chat_approval(move |id, decision| {
        let _ = sender.send(Command::DecideChatApproval {
            id: id.to_string(),
            decision: decision.to_string(),
        });
    });

    // ---- cron jobs ---------------------------------------------------
    let sender = commands.clone();
    ui.on_refresh_cron(move || {
        let _ = sender.send(Command::RefreshCron);
    });
    let sender = commands.clone();
    ui.on_toggle_cron(move |id, enabled| {
        let _ = sender.send(Command::ToggleCron {
            id: id.to_string(),
            enabled,
        });
    });
    let sender = commands.clone();
    ui.on_delete_cron(move |id| {
        let _ = sender.send(Command::DeleteCron { id: id.to_string() });
    });
    let sender = commands.clone();
    ui.on_run_cron_now(move |id| {
        let _ = sender.send(Command::RunCronNow { id: id.to_string() });
    });
    let sender = commands.clone();
    ui.on_open_cron_runs(move |id| {
        let _ = sender.send(Command::OpenCronRuns { id: id.to_string() });
    });
    let sender = commands.clone();
    ui.on_close_cron_runs(move || {
        let _ = sender.send(Command::CloseCronRuns);
    });
    let sender = commands.clone();
    ui.on_open_cron_editor(move |id| {
        let id = id.to_string();
        let _ = sender.send(Command::OpenCronEditor {
            id: if id.is_empty() { None } else { Some(id) },
        });
    });
    let sender = commands.clone();
    ui.on_close_cron_editor(move || {
        let _ = sender.send(Command::CloseCronEditor);
    });
    let sender = commands.clone();
    ui.on_save_cron(move |draft| {
        let _ = sender.send(Command::SaveCron { draft });
    });

    // ---- memory fragments --------------------------------------------
    let sender = commands.clone();
    ui.on_refresh_memory(move || {
        let _ = sender.send(Command::RefreshMemory);
    });
    let sender = commands.clone();
    ui.on_search_memory(move |query| {
        let _ = sender.send(Command::MemorySearch(query.to_string()));
    });
    let sender = commands.clone();
    ui.on_set_memory_category(move |category| {
        let _ = sender.send(Command::MemoryCategory(category.to_string()));
    });
    let sender = commands.clone();
    ui.on_open_memory_editor(move |index| {
        let _ = sender.send(Command::OpenMemoryEditor { index });
    });
    let sender = commands.clone();
    ui.on_close_memory_editor(move || {
        let _ = sender.send(Command::CloseMemoryEditor);
    });
    let sender = commands.clone();
    ui.on_save_memory(move |draft| {
        let _ = sender.send(Command::SaveMemory { draft });
    });
    let sender = commands.clone();
    ui.on_delete_memory(move |id| {
        let _ = sender.send(Command::DeleteMemory { id: id.to_string() });
    });

    // ---- cloud account / device switch / approval --------------------
    let sender = commands.clone();
    ui.on_cloud_login(move |server, username, password| {
        let _ = sender.send(Command::CloudLogin {
            server: server.to_string(),
            username: username.to_string(),
            password: password.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_cloud_logout(move || {
        let _ = sender.send(Command::CloudLogout);
    });
    let sender = commands.clone();
    ui.on_cloud_refresh(move || {
        let _ = sender.send(Command::RefreshCloudRemote);
        let _ = sender.send(Command::RefreshUsage);
    });
    let sender = commands.clone();
    ui.on_cloud_toggle_sync(move |enabled| {
        let _ = sender.send(Command::CloudSetSync(enabled));
    });
    let sender = commands.clone();
    ui.on_cloud_set_sharing(move |enabled, full_shadow| {
        let _ = sender.send(Command::CloudSetSharing {
            enabled,
            full_shadow,
        });
    });
    let sender = commands.clone();
    ui.on_switch_chat_target(move |target| {
        let _ = sender.send(Command::SwitchChatTarget(target));
    });
    let sender = commands.clone();
    ui.on_refresh_devices(move || {
        let _ = sender.send(Command::RefreshDevices);
    });
    let sender = commands.clone();
    ui.on_cloud_refresh_threads(move || {
        let _ = sender.send(Command::RefreshCloudThreads);
    });
    let sender = commands.clone();
    ui.on_cloud_new_thread(move || {
        let _ = sender.send(Command::CloudNewThread);
    });
    let sender = commands.clone();
    ui.on_cloud_select_thread(move |id| {
        let _ = sender.send(Command::CloudSelectThread(id.to_string()));
    });
    let sender = commands.clone();
    ui.on_cloud_rename(move |id, title| {
        let _ = sender.send(Command::CloudRename {
            id: id.to_string(),
            title: title.to_string(),
        });
    });
    let sender = commands.clone();
    ui.on_cloud_archive(move |id| {
        let _ = sender.send(Command::CloudArchive(id.to_string()));
    });
    let sender = commands.clone();
    ui.on_decide_approval(move |id, approve| {
        let _ = sender.send(Command::DecideApproval {
            id: id.to_string(),
            approve,
        });
    });

    // ---- channels ----------------------------------------------------
    let sender = commands.clone();
    ui.on_refresh_channels(move || {
        let _ = sender.send(Command::RefreshChannels);
    });
    let sender = commands.clone();
    ui.on_select_channel_account(move |id| {
        let _ = sender.send(Command::SelectChannelAccount(id.to_string()));
    });
    let sender = commands.clone();
    ui.on_open_channel_editor(move |id| {
        let _ = sender.send(Command::OpenChannelEditor(id.to_string()));
    });
    let sender = commands.clone();
    ui.on_close_channel_editor(move || {
        let _ = sender.send(Command::CloseChannelEditor);
    });
    let sender = commands.clone();
    ui.on_save_channel(move |draft| {
        let _ = sender.send(Command::SaveChannel(draft));
    });
    let sender = commands.clone();
    ui.on_toggle_channel(move |id, enabled| {
        let _ = sender.send(Command::ToggleChannel {
            account_id: id.to_string(),
            enabled,
        });
    });
    let sender = commands.clone();
    ui.on_delete_channel(move |id| {
        let _ = sender.send(Command::DeleteChannel(id.to_string()));
    });
    let sender = commands.clone();
    ui.on_open_channel_logs(move |id| {
        let _ = sender.send(Command::OpenChannelLogs(id.to_string()));
    });
    let sender = commands.clone();
    ui.on_close_channel_logs(move || {
        let _ = sender.send(Command::CloseChannelLogs);
    });
    let sender = commands.clone();
    ui.on_delete_channel_binding(move |id| {
        let _ = sender.send(Command::DeleteChannelBinding(id.to_string()));
    });
    // 扫码登录只出现在微信机器人渠道：目标账号取编辑中的草稿（新建时为空，
    // 即登录成一个新账号；编辑已有账号时为空之外的 id，即重绑该账号）。
    let weak = ui.as_weak();
    let sender = commands.clone();
    ui.on_start_channel_qr(move |force| {
        let account_id = weak
            .upgrade()
            .map(|ui| ui.get_settings_channel_draft().account_id.to_string())
            .unwrap_or_default();
        let _ = sender.send(Command::StartChannelQr { account_id, force });
    });
    let sender = commands.clone();
    ui.on_cancel_channel_qr(move || {
        let _ = sender.send(Command::CancelChannelQr);
    });
    let sender = commands.clone();
    ui.on_save_channel_qr(move || {
        let _ = sender.send(Command::SaveChannelQr);
    });

    // 撤下一条还在等待发送的消息：文本由引擎退回草稿，这一帧只发命令。
    let sender = commands.clone();
    ui.on_withdraw_waiting(move |queue_id| {
        let _ = sender.send(Command::WithdrawWaiting(queue_id.to_string()));
    });

    // 编辑＝撤回并回填草稿；插话＝立刻交给引擎，跳下一个动作边界执行。
    let sender = commands.clone();
    ui.on_edit_waiting(move |queue_id| {
        let _ = sender.send(Command::EditWaiting(queue_id.to_string()));
    });
    let sender = commands.clone();
    ui.on_interject_waiting(move |queue_id| {
        let _ = sender.send(Command::InterjectWaiting(queue_id.to_string()));
    });

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

    // 首次配置引导：本机还没有真实模型。打开模型页，并在同一命令序列里新建一个
    // 草稿编辑器；引擎按顺序处理，所以设置拉取先落地，编辑器随后自动展开。
    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_setup_first_model(move || {
        let _ = sender.send(Command::LoadSettings);
        let _ = sender.send(Command::OpenModelEditor { key: None });
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
                    // 用户亲手点过：此后自动开合不再改动这一条。
                    child.manual = true;
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
            // 用户亲手点过：此后自动开合不再改动这一条。
            entry.manual = true;
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

    // ---- 目标横幅 / 计划面板 ------------------------------------------
    let sender = commands.clone();
    ui.on_goal_save(move |text| {
        let _ = sender.send(Command::GoalEdit(text.to_string()));
    });
    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_goal_pause_toggle(move || {
        let Some(ui) = weak.upgrade() else { return };
        let resume = ui.get_goal_paused()
            || ui.get_goal_phase() == "blocked"
            || ui.get_goal_activation() == "disarmed";
        let _ = sender.send(if resume {
            Command::GoalResume
        } else {
            Command::GoalPause
        });
    });
    let sender = commands.clone();
    ui.on_goal_clear(move || {
        let _ = sender.send(Command::GoalClear);
    });
    let weak = ui.as_weak();
    ui.on_activity_plan_dismiss(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_activity_plan_expanded(false);
        }
    });

    // The transcript's copy buttons and the message selection both land here;
    // the Android backend forwards the text to the system clipboard. Give the
    // tap visible feedback: the system clipboard has no toast of its own in a
    // NativeActivity window.
    //
    // The payload is bounded before it leaves Rust (`crate::clipboard`): a
    // work-process entry can carry the whole tool result, and an oversized
    // string never reaches the system clipboard — the Binder transaction is
    // dropped, the previous clip stays in place, and the toast still said
    // 已复制. The write is read back so that case is reported instead of hidden.
    let weak = ui.as_weak();
    let shared_for_copy = shared.clone();
    ui.on_copy_text(move |text| {
        let Some(ui) = weak.upgrade() else { return };
        let payload = crate::clipboard::prepare(text.as_str());
        // An empty payload (a row whose text is still empty) would otherwise
        // read back as "the old clip is still there" and report a write
        // failure. There is simply nothing to copy yet.
        if payload.text.is_empty() {
            if let Ok(mut state) = shared_for_copy.lock() {
                state.notify("没有可复制的内容");
                state.touch();
            }
            return;
        }
        let window = i_slint_core::window::WindowInner::from_pub(ui.window());
        let platform = window.context().platform();
        platform.set_clipboard_text(
            payload.text.as_str(),
            i_slint_core::platform::Clipboard::DefaultClipboard,
        );
        // Android can refuse the read-back when the app is not focused. Only a
        // read that succeeded and disagrees proves the write was dropped. A
        // stored prefix means the platform kept only part of the clip (some
        // ROMs do this below the Binder budget): say how much landed instead of
        // calling it a failure. Anything else is the previous clip still being
        // there, which is exactly the state that made the following paste fail.
        let message =
            match platform.clipboard_text(i_slint_core::platform::Clipboard::DefaultClipboard) {
                Some(current) if current != payload.text => {
                    log::warn!(
                        "clipboard write rejected: {} chars (of {}), read-back {} chars",
                        payload.text.chars().count(),
                        payload.total_chars,
                        current.chars().count()
                    );
                    if !current.is_empty() && payload.text.starts_with(current.as_str()) {
                        format!("内容过长，已复制前 {} 字", current.chars().count())
                    } else {
                        "复制失败：剪贴板未写入".to_string()
                    }
                }
                _ => payload.toast(),
            };
        if let Ok(mut state) = shared_for_copy.lock() {
            state.notify(message);
            state.touch();
        }
    });

    // Transcript pictures, document cards and file chips open the same file the
    // workspace menu opens; Markdown links go to the system browser. Resolution
    // stays on the engine side because it owns the workspace catalogue.
    let sender = commands.clone();
    ui.on_open_resource(move |path| {
        let _ = sender.send(Command::OpenResource {
            path: path.to_string(),
        });
    });

    // Dragging one of the floating knobs moves that end of the selection; the
    // core owns the text layout and reports the new handle point next tick.
    let weak = ui.as_weak();
    ui.on_selection_handle_drag(move |start, x, y| {
        let Some(ui) = weak.upgrade() else { return };
        i_slint_core::text_selection::move_handle(
            ui.window(),
            if start {
                i_slint_core::text_selection::SelectionHandle::Start
            } else {
                i_slint_core::text_selection::SelectionHandle::End
            },
            i_slint_core::lengths::LogicalPoint::new(x, y),
        );
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

/// One decoded image in UI form: a still has one frame, a GIF keeps every
/// bounded frame plus its per-frame delay in milliseconds.
#[derive(Clone)]
struct CachedImage {
    frames: Vec<slint::Image>,
    delays: Vec<i32>,
    bytes: usize,
}

/// Caches converted rows by id + content signature. Only the changed tail is
/// re-projected while a turn streams.
struct RowCache {
    renderer: MarkdownRenderer,
    rows: HashMap<String, (u64, TimelineRow)>,
    /// Decoded workspace images by source, shared with every block model the
    /// cache produced. `None` records a failed load.
    images: HashMap<String, Option<CachedImage>>,
    /// Sources seen in a projection but not decoded yet; the tick forwards a
    /// bounded batch to the engine.
    wanted: Vec<String>,
}

impl Default for RowCache {
    fn default() -> Self {
        Self {
            renderer: MarkdownRenderer::new(),
            rows: HashMap::new(),
            images: HashMap::new(),
            wanted: Vec::new(),
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
        self.hydrate_row(&row);
        self.rows.insert(data.id.clone(), (signature, row.clone()));
        row
    }

    fn from_iter<'a>(&mut self, data: impl Iterator<Item = &'a RowData>) -> Vec<TimelineRow> {
        data.map(|row| self.row(row)).collect()
    }

    /// Merge freshly decoded images into the cache. The pixels are shared with
    /// every block model, so this only records new sources.
    fn sync_images(&mut self, images: &HashMap<String, Option<crate::engine::SharedImage>>) {
        for (source, image) in images {
            if self.images.contains_key(source) {
                continue;
            }
            let image = image.as_ref().map(|pixels| CachedImage {
                frames: pixels
                    .frames
                    .iter()
                    .map(|rgba| {
                        slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
                            rgba,
                            pixels.width,
                            pixels.height,
                        ))
                    })
                    .collect(),
                delays: pixels.delays.iter().map(|delay| *delay as i32).collect(),
                bytes: pixels
                    .frames
                    .len()
                    .saturating_mul(pixels.width as usize)
                    .saturating_mul(pixels.height as usize)
                    .saturating_mul(4),
            });
            let held: usize = self
                .images
                .values()
                .flatten()
                .map(|image| image.bytes)
                .sum();
            if self.images.len() >= 64
                || held.saturating_add(image.as_ref().map_or(0, |image| image.bytes))
                    > 96 * 1024 * 1024
            {
                // Block models keep their own pixel handles; dropping this
                // lookup only makes a future projection request the source
                // again.
                self.images.clear();
            }
            self.images.insert(source.clone(), image);
        }
    }

    fn take_wanted(&mut self) -> Vec<String> {
        std::mem::take(&mut self.wanted)
    }

    /// Patch every cached row; called after new images landed. The rows share
    /// their block models with the published UI model, so the pictures appear
    /// without rebuilding the transcript.
    fn hydrate_all(&mut self) {
        let rows: Vec<TimelineRow> = self.rows.values().map(|(_, row)| row.clone()).collect();
        for row in &rows {
            self.hydrate_row(row);
        }
    }

    fn hydrate_row(&mut self, row: &TimelineRow) {
        self.hydrate_model(&row.blocks);
        self.hydrate_model(&row.reasoning_blocks);
        for entry in row.process_entries.iter() {
            self.hydrate_model(&entry.blocks);
        }
    }

    fn hydrate_model(&mut self, model: &ModelRc<crate::MdBlock>) {
        if model.row_count() == 0 {
            return;
        }
        for index in 0..model.row_count() {
            let Some(mut block) = model.row_data(index) else {
                continue;
            };
            if block.kind != crate::md::block::IMAGE || block.image_source.is_empty() {
                continue;
            }
            if block.image.size().width > 0 || block.image_error {
                continue;
            }
            let source = block.image_source.to_string();
            match self.images.get(&source) {
                Some(Some(image)) => {
                    if let Some(first) = image.frames.first() {
                        let size = first.size();
                        block.image = first.clone();
                        block.image_width = size.width as f32;
                        block.image_height = size.height as f32;
                        if image.frames.len() > 1 {
                            block.image_frames = ModelRc::new(VecModel::from(image.frames.clone()));
                            block.frame_delays = ModelRc::new(VecModel::from(image.delays.clone()));
                        } else {
                            block.image_frames = ModelRc::default();
                            block.frame_delays = ModelRc::default();
                        }
                    }
                    model.set_row_data(index, block);
                }
                Some(None) => {
                    block.image_error = true;
                    model.set_row_data(index, block);
                }
                None => {
                    if !self.wanted.iter().any(|held| held == &source) {
                        self.wanted.push(source);
                    }
                }
            }
        }
    }
}

/// A running local stream owns the visible transcript. The durable watcher
/// only publishes commit boundaries (one per model round or tool), so its copy
/// of the same turn is always behind the live stream; letting that copy shadow
/// the live footer hid the 请求中/模型输出中 status and froze the work-process
/// steps between commits.
fn live_is_authoritative(state: &Shared) -> bool {
    state.generating && !state.live_rows.is_empty() && state.live_session == state.session_id
}

/// Root id whose durable rows must be hidden while the live stream owns it.
/// Before the identity frame arrives the stream's root is still unknown; match
/// the newest durable running turn carrying the same user message instead, so
/// the optimistic bubble and its durable twin never render together.
fn live_owner(state: &Shared) -> Option<String> {
    if !live_is_authoritative(state) {
        return None;
    }
    if !state.live_root.is_empty() && state.live_root != "live" {
        return Some(state.live_root.clone());
    }
    let text = state
        .live_rows
        .iter()
        .find(|row| row.kind == project::KIND_USER)?
        .text
        .clone();
    project::running_root_for_text(&state.durable_rows, &text)
}

fn belongs_to_turn(row: &RowData, root: &str) -> bool {
    !root.is_empty()
        && (row.id == root
            || row
                .id
                .strip_prefix(root)
                .is_some_and(|rest| rest.starts_with('-')))
}

/// True once the durable projection carries the live turn's settled body or
/// error. Only then are the live rows redundant; a body from an earlier model
/// round must not hide the answer that is still streaming.
fn durable_covers_live(state: &Shared) -> bool {
    let root = state.live_root.as_str();
    if root.is_empty() || root == "live" {
        return false;
    }
    let live_body = state
        .live_rows
        .iter()
        .find(|row| row.kind == project::KIND_BODY)
        .map(|row| row.text.trim())
        .filter(|text| !text.is_empty());
    let live_error = state
        .live_rows
        .iter()
        .find(|row| row.kind == project::KIND_ERROR)
        .map(|row| row.text.trim())
        .filter(|text| !text.is_empty());
    let durable_text = |kind: i32| {
        state
            .durable_rows
            .iter()
            .filter(move |row| belongs_to_turn(row, root) && row.kind == kind)
            .map(|row| row.text.trim())
    };
    if let Some(text) = live_body {
        return durable_text(project::KIND_BODY).any(|body| body == text || body.starts_with(text));
    }
    if let Some(text) = live_error {
        return durable_text(project::KIND_ERROR).any(|error| error == text);
    }
    durable_text(project::KIND_BODY).next().is_some()
}

/// The live tail that the transcript should show this frame. While the stream
/// is attached every live row is kept, and its durable twin is hidden instead;
/// once the stream settles the durable copy takes over as soon as it carries
/// the same body.
fn visible_live_rows<'a>(state: &'a Shared) -> Vec<&'a RowData> {
    if state.live_rows.is_empty()
        || (!state.live_session.is_empty() && state.live_session != state.session_id)
    {
        return Vec::new();
    }
    if live_is_authoritative(state) {
        return state.live_rows.iter().collect();
    }
    if durable_covers_live(state) {
        return Vec::new();
    }
    let durable_ids: std::collections::HashSet<&str> = state
        .durable_rows
        .iter()
        .map(|row| row.id.as_str())
        .collect();
    state
        .live_rows
        .iter()
        .filter(|row| !durable_ids.contains(row.id.as_str()))
        .collect()
}

/// Same normalization as 桌面版: unknown or legacy theme values fall back to the
/// default accent instead of leaving the palette unset.
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

fn permission_row(item: &crate::permissions::PermissionItem) -> MobilePermission {
    MobilePermission {
        id: item.id.as_str().into(),
        title: item.title.as_str().into(),
        description: item.description.as_str().into(),
        state_text: item.state_text.as_str().into(),
        action_text: item.action_text.as_str().into(),
        granted: item.granted,
        available: item.available,
    }
}

fn update_percent(downloaded: u64, total: u64) -> i32 {
    if total == 0 {
        return 0;
    }
    (downloaded.min(total).saturating_mul(100) / total) as i32
}

fn update_progress_text(downloaded: u64, total: u64) -> String {
    let done = if downloaded == 0 {
        "0 B".to_string()
    } else {
        crate::update::format_size(downloaded)
    };
    if total == 0 {
        format!("已下载 {done}")
    } else {
        format!("已下载 {done} / {}", crate::update::format_size(total))
    }
}

/// 目标横幅的计时文案：省略前导的零单位，与桌面版一致。
fn goal_elapsed(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let rest = seconds % 60;
    if hours > 0 {
        format!("{hours}小时{minutes}分{rest}秒")
    } else if minutes > 0 {
        format!("{minutes}分{rest}秒")
    } else {
        format!("{rest}秒")
    }
}

/// 状态胶囊指向的步骤：执行中的优先，否则第一个未完成的；全部完成时为空。
fn plan_current(steps: &[crate::engine::PlanStepView]) -> (String, i32) {
    let index = steps
        .iter()
        .position(|step| step.status == "in_progress")
        .or_else(|| steps.iter().position(|step| step.status != "completed"));
    match index {
        Some(index) => (steps[index].step.clone(), index as i32 + 1),
        None => (String::new(), 0),
    }
}

/// 首装引导只在“还没有可用运行环境”时打开：等待自动安装（missing）或安装中。
fn env_setup_should_open(state: &str, dismissed: bool) -> bool {
    !dismissed && matches!(state, "missing" | "installing")
}

/// 引导页的单调进度：Java 的阶段切换会回退（94 → 85），这里只前进不后退。
fn next_env_progress(current: i32, state: &str, reported: i32) -> i32 {
    let reported = reported.clamp(0, 100);
    if matches!(state, "missing" | "installing") {
        current.max(reported)
    } else {
        reported
    }
}

/// Push the projected turn ruler into the shared model only when its visible
/// content changed; a streaming token usually alters only the last preview.
fn apply_turn_marks(
    model: &Rc<VecModel<TurnMark>>,
    last: &mut Vec<project::TurnMarkData>,
    marks: Vec<project::TurnMarkData>,
) {
    if *last == marks {
        return;
    }
    model.set_vec(
        marks
            .iter()
            .map(|mark| TurnMark {
                fraction: mark.fraction,
                user: mark.user.as_str().into(),
                agent: mark.agent.as_str().into(),
            })
            .collect::<Vec<TurnMark>>(),
    );
    *last = marks;
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
    let turn_marks_model = Rc::new(VecModel::<TurnMark>::default());
    ui.set_turn_marks(ModelRc::from(turn_marks_model.clone()));
    let mut last_marks: Vec<project::TurnMarkData> = Vec::new();
    let mut ubuntu_ticks = 0u32;
    let mut cache = RowCache::default();
    let mut last_rev = 0u64;
    // 待发条的键：(修订号, 当前线程, 本机/云端)。切线程或切到云端时即使
    // 没有新消息也要重投影，否则会把上一个线程（或本机会话）的等待行留在
    // 这一屏的输入框上沿。
    let mut last_waiting: (u64, String, i32) = (0, String::new(), 0);
    // 输入框附件卡的键：(上传修订号, 本机/云端)。切到云端时也要重投影一次，
    // 否则本机线程的附件卡会跟着挂到云端会话的输入框上。
    let mut last_uploads: (u64, i32) = (0, -1);
    // 加号面板预设问题的键：(预设修订号, 本机/云端)：云端线程没有本机智能体。
    let mut last_presets: (u64, i32) = (0, -1);
    // 语音投影的键：(修订号, 识别路线)。计时另外单独比较，避免每帧重写。
    let mut last_voice: (u64, i32) = (0, -1);
    let mut last_voice_elapsed = String::new();
    let mut last_rows = 0u64;
    let mut last_live = 0u64;
    let mut last_live_active = false;
    let mut last_hide_root = String::new();
    let mut last_durable_len = 0usize;
    let mut last_toast = 0u64;
    let mut toast_deadline: Option<Instant> = None;
    let mut last_patch = 0u64;
    let mut last_sessions = 0u64;
    let mut last_archived = 0u64;
    let mut last_trajectory = 0u64;
    let mut last_prompt_preview = 0u64;
    let mut last_workspace = 0u64;
    let mut last_workspaces = 0u64;
    let mut last_models = 0u64;
    let mut last_tools = 0u64;
    let mut last_tool_editor = 0u64;
    let mut last_tool_file = 0u64;
    let mut last_agent = 0u64;
    let mut last_greeting = 0u64;
    let mut last_cloud_greeting = 0u64;
    let mut last_agent_seed = 0u64;
    let mut last_cron = 0u64;
    let mut last_cron_editor = 0u64;
    let mut last_cron_runs = 0u64;
    let mut last_memory = 0u64;
    let mut last_memory_editor = 0u64;
    let mut last_memory_seed = 0u64;
    let mut last_target_rev = 0u64;
    let mut last_cloud = 0u64;
    let mut last_usage = 0u64;
    let mut last_cloud_threads = 0u64;
    let mut last_cloud_active = 0u64;
    let mut last_cloud_rows = 0u64;
    let mut last_devices = 0u64;
    let mut last_approval = 0u64;
    let mut last_channels = 0u64;
    let mut last_channel_editor = 0u64;
    let mut channel_editor_was_open = false;
    let mut last_channel_qr = 0u64;
    let mut last_channel_qr_pending = 0u64;
    let mut last_settings_page = 0;
    let mut tool_editor_was_open = false;
    let mut tool_file_was_open = false;
    let mut cron_editor_was_open = false;
    let mut memory_editor_was_open = false;
    let mut last_settings = 0u64;
    let mut last_ubuntu = 0u64;
    // 首装引导：单调进度、「已就绪」停留计时与液面动画相位。
    let mut env_progress = 0i32;
    let mut env_ready_at: Option<Instant> = None;
    let env_phase_origin = Instant::now();
    let mut last_companion = 0u64;
    let mut last_permissions = 0u64;
    let mut last_backup = u64::MAX;
    let mut last_context = u64::MAX;
    // 模型浮层活动区块（后台任务 / 子智能体）的修订号，以及浮层的开合状态。
    let mut last_sheet_activity = u64::MAX;
    let mut last_model_sheet = false;
    let mut last_update = 0u64;
    let mut last_update_progress = -1i32;
    let mut last_update_downloaded = String::new();
    let mut last_editor = 0u64;
    let mut last_probe = 0u64;
    let mut last_inquiry = 0u64;
    let mut last_chat_approval = 0u64;
    let mut last_goal = 0u64;
    let mut last_activity = 0u64;
    let mut editor_was_open = false;
    #[cfg(target_os = "android")]
    let mut last_back: Option<Instant> = None;
    #[cfg(target_os = "android")]
    let mut last_overlay_state = 0;
    let mut host_ticks = 0u32;
    let mut last_images = 0u64;
    let mut last_selection = String::new();
    let mut selection_lost_at: Option<Instant> = None;

    let timer = Timer::default();
    timer.start(TimerMode::Repeated, TICK, move || {
        let Some(ui) = weak.upgrade() else { return };
        // The window is edge-to-edge: the activity does not resize under the
        // soft keyboard, so the shell reads Slint's virtual-keyboard rect and
        // lifts the transcript and composer above it. This tick is only the
        // safety net and the trigger: the keyboard animation is followed by
        // the 16 ms timer above, so the input card rides with the IME instead
        // of waiting for it to settle.
        let ime = ime_height(&ui);
        if (ime - ime_applied()).abs() > 0.5 {
            ui.set_ime_height(ime);
            ime_settle(ime);
            start_ime_follow(&ui);
        }
        #[cfg(target_os = "android")]
        {
            let overlay_state = ui.get_overlay_state();
            if overlay_state != last_overlay_state {
                crate::android_host::set_input_overlay_open(overlay_state > 0);
                last_overlay_state = overlay_state;
            }
        }
        // 首装引导的液面动画相位：只在引导页打开时推进，页面关闭后不再产生
        // 任何属性更新。相位单调递增（以 4s 波浪周期为单位），不取模：气泡
        // travel=wrap(phase*speed) 只有在相位单调时才连续上升，取模成锯齿会
        // 让非整数速度的气泡在周期边界瞬移回底部。波浪在 Slint 侧 wrap(phase)。
        if ui.get_env_setup_open() {
            let cycles = env_phase_origin.elapsed().as_secs_f32() / 4.0;
            ui.set_env_setup_phase(cycles);
        }
        // Live command counts move at 1 Hz, not at the render tick. 浮层打开的
        // 那一刻先要一份活动快照，再交给这条秒级刷新维持耗时与状态。
        host_ticks = host_ticks.wrapping_add(1);
        let model_sheet = ui.get_model_sheet_open();
        if model_sheet != last_model_sheet {
            last_model_sheet = model_sheet;
            if model_sheet {
                let _ = commands.send(Command::RefreshTasks { deep: true });
            }
        }
        if host_ticks % 30 == 0 {
            let _ = commands.send(Command::RefreshTasks { deep: model_sheet });
            // 目标横幅的本地秒表：只在目标真正执行时前进。
            if let Ok(mut state) = shared.lock() {
                if state.goal.active() && !state.goal.paused() {
                    state.goal.seconds = state.goal.seconds.saturating_add(1);
                    state.goal_rev = state.goal_rev.wrapping_add(1);
                }
                // 工作过程条右端的耗时：运行中的回合由本地秒表按真实时间推进，
                // 与桌面版一致；等回合落定后统计值才接管。工具长时间不出事件时
                // 这条秒表仍在走，数字不会停在旧值上。
                if state.generating && state.live_session == state.session_id {
                    if let Some(started) = state.turn_started {
                        let label = project::live_duration_label(
                            started.elapsed().as_secs_f64(),
                        );
                        if state.set_live_duration(&label) {
                            state.live_rev = state.live_rev.wrapping_add(1);
                        }
                    }
                }
            }
        }
        // Settings pages 7 (tools) and 8 (agent) are opened inside the Slint
        // sheet, so the shell detects the page transition and asks the engine
        // for the matching projection once.
        let settings_page = ui.get_settings_page();
        if settings_page != last_settings_page {
            if settings_page == 7 {
                let _ = commands.send(Command::RefreshTools);
            } else if settings_page == 8 {
                let _ = commands.send(Command::RefreshAgent);
            } else if settings_page == 9 {
                let _ = commands.send(Command::RefreshCron);
            } else if settings_page == 10 {
                let _ = commands.send(Command::RefreshMemory);
            } else if settings_page == 13 {
                let _ = commands.send(Command::RefreshCloudAccount);
                let _ = commands.send(Command::RefreshUsage);
                let _ = commands.send(Command::RefreshDevices);
            } else if settings_page == 14 {
                let _ = commands.send(Command::RefreshChannels);
            } else if settings_page == 11 {
                let _ = commands.send(Command::RefreshCompanion);
            } else if settings_page == 12 {
                let _ = commands.send(Command::RefreshPermissions);
            } else if settings_page == 6 {
                let _ = commands.send(Command::CheckAndroidUpdate);
            }
            last_settings_page = settings_page;
        }
        // The overlay permission is granted in the system settings app; while
        // the companion page is open, poll so returning to the sheet flips the
        // permission row without another tap.
        if settings_page == 11 && host_ticks % 30 == 0 {
            let _ = commands.send(Command::RefreshCompanion);
        }
        // The permission manager polls while visible so returning from a
        // system settings page updates every row without a manual refresh.
        if settings_page == 12 && host_ticks % 30 == 0 {
            let _ = commands.send(Command::RefreshPermissions);
        }
        // The file picker and the legacy save dialog answer on Java threads.
        // Move their results into the engine queue from the UI tick so the
        // JNI callbacks never touch Slint or the engine state directly.
        #[cfg(target_os = "android")]
        if ui.get_workspace_open()
            || ui.get_settings_page() == 7
            // 抓拍/选文件期间每 ~100ms 收一次结果：照片回到输入框不该等
            // 空闲轮询的那一秒，也不必每帧都过一遍 JNI。
            || (crate::android_host::pick_pending() && host_ticks % 3 == 0)
            || host_ticks % 30 == 0
        {
            if let Some(picked) = crate::android_host::take_picked() {
                match picked {
                    crate::android_host::PickedFiles::Workspace { dir, files, attach } => {
                        // 相机回来的是一张照片：走单独的入口，量词是「张」，
                        // 也不刷新工作目录页（用户正在浏览的目录不该被切走）。
                        if attach && dir == crate::engine::CHAT_MEDIA_SOURCE {
                            for path in files {
                                let _ = commands.send(Command::UploadPhoto { path });
                            }
                        } else {
                            let _ = commands.send(Command::UploadWorkspace { dir, files, attach });
                        }
                    }
                    crate::android_host::PickedFiles::ToolFiles(files) => {
                        let _ = commands.send(Command::UploadToolFiles { files });
                    }
                    crate::android_host::PickedFiles::SkillArchive(path) => {
                        let _ = commands.send(Command::ImportSkill { path });
                    }
                    crate::android_host::PickedFiles::Failed(message) => {
                        if let Ok(mut state) = shared.lock() {
                            state.notify(message);
                            state.touch();
                        }
                    }
                }
            }
            for (ok, name) in crate::android_host::take_save_results() {
                let _ = commands.send(Command::WorkspaceSaved { ok, name });
            }
        }
        // The folder picker only matters while the workspace catalogue is
        // open; poll it there so returning from the system picker lands in the
        // editor without a visible delay.
        #[cfg(target_os = "android")]
        if ui.get_workspaces_open() && host_ticks % 6 == 0 {
            if let Some(result) = crate::android_host::take_picked_directory() {
                match result {
                    Ok(path) => ui.set_workspace_picked_folder(path.into()),
                    Err(message) => {
                        if let Ok(mut state) = shared.lock() {
                            state.notify(message);
                            state.touch();
                        }
                    }
                }
            }
        }
        // System shares are parked by Java in a static queue (the activity may
        // see them before Rust exists). Move them into the engine queue here
        // so no JNI work and no file copy ever runs on the render thread.
        #[cfg(target_os = "android")]
        if host_ticks % 6 == 0 {
            for _ in 0..4 {
                let Some(intent) = crate::intent::take() else {
                    break;
                };
                let command = match intent {
                    crate::intent::SharedIntent::Text(text) => Command::SharedIntent {
                        text,
                        url: String::new(),
                        files: Vec::new(),
                    },
                    crate::intent::SharedIntent::Link(url) => Command::SharedIntent {
                        text: String::new(),
                        url,
                        files: Vec::new(),
                    },
                    crate::intent::SharedIntent::Files { files, text } => Command::SharedIntent {
                        text: text.unwrap_or_default(),
                        url: String::new(),
                        files,
                    },
                };
                let _ = commands.send(command);
            }
        }
        // Voice results are posted by Java workers (recorder and the system
        // recognizer share one queue). Polling every 6 ticks keeps the strip
        // and the transcribed draft within ~200 ms of the device.
        #[cfg(target_os = "android")]
        if host_ticks % 6 == 0 {
            for event in crate::android_host::take_voice_events() {
                match event.as_str() {
                    "recording" => {
                        let _ = commands.send(Command::VoiceStarted);
                    }
                    "system|listening" => {
                        let _ = commands.send(Command::VoiceSystemStarted);
                    }
                    "denied" => {
                        let _ = commands.send(Command::VoiceFailed {
                            message: "录音权限被拒绝".into(),
                        });
                    }
                    "cancelled" => {
                        let _ = commands.send(Command::VoiceCancelled);
                    }
                    event => {
                        if let Some(path) = event.strip_prefix("done|") {
                            let _ = commands.send(Command::VoiceFinished {
                                path: path.to_string(),
                            });
                        } else if let Some(text) = event.strip_prefix("system|done|") {
                            let _ = commands.send(Command::VoiceText {
                                text: text.to_string(),
                            });
                        } else if let Some(message) = event.strip_prefix("system|error|") {
                            let _ = commands.send(Command::VoiceFailed {
                                message: format!("语音识别：{message}"),
                            });
                        } else if let Some(message) = event.strip_prefix("error|") {
                            let _ = commands.send(Command::VoiceFailed {
                                message: format!("录音失败：{message}"),
                            });
                        }
                    }
                }
            }
        }
        // Back presses arrive either from the activity's dispatcher callback or
        // from Slint's key pipeline (Key.Back). Resolve them on the UI thread
        // so page state, keyboard state and the exit prompt all read the same
        // snapshot the user is looking at.
        #[cfg(target_os = "android")]
        if host_ticks % 3 == 0 {
            let presses = crate::android_host::take_back_requests()
                + crate::android_host::take_slint_back_requests();
            for _ in 0..presses {
                // 录音中按返回：先丢掉这段录音，返回键不继续关页面，
                // 免得用户带着一条看不见的录音条离开聊天页。
                if ui.get_voice_phase() != 0 {
                    let _ = commands.send(Command::VoiceCancel);
                    continue;
                }
                if !handle_back(&ui, &shared, &mut last_back, ime) {
                    crate::android_host::finish_activity();
                    break;
                }
            }
        }
        // A cancelled or failed system install must return the update card to
        // a retryable state; otherwise it would stay "installing" forever.
        #[cfg(target_os = "android")]
        if host_ticks % 6 == 0 && ui.get_update_phase() == "installing" {
            if let Some(result) = crate::android_host::take_install_result() {
                if let Ok(mut state) = shared.lock() {
                    if state.update_phase == "installing" {
                        state.update_phase = "ready".into();
                        state.update_error.clear();
                        if let Some(message) = result.strip_prefix("failed:") {
                            let message = message.to_string();
                            state.notify(format!("安装失败：{message}"));
                        }
                        state.update_rev = state.update_rev.wrapping_add(1);
                        state.touch();
                    }
                }
            }
        }
        let pending_draft = shared
            .lock()
            .ok()
            .and_then(|mut state| state.pending_draft.take());
        if let Some(draft) = pending_draft {
            // 组词区先落进草稿：整段替换草稿时，输入法若还挂着组合区，会在
            // 焦点收回时被并进新文本。
            i_slint_core::items::commit_focused_preedit(ui.window());
            let current = ui.get_draft().to_string();
            let separator = if current.trim().is_empty() { "" } else { "\n" };
            let next = format!("{current}{separator}{}", draft.text.trim());
            if draft.from_voice {
                // 语音写入交给输入区完成：它会先收回焦点（结束输入法会话），
                // 再换文本并把光标放到末尾。宿主直接 set_draft 时，焦点还在
                // 的输入法会把整段新文本当成组合区显示成「全选」，点按也无
                // 法复位。
                ui.set_voice_draft_final(next.into());
                let rev = ui.get_voice_draft_rev().wrapping_add(1);
                ui.set_voice_draft_rev(rev);
            } else {
                ui.set_draft(next.into());
            }
        }
        // 待发消息只在当前这一轮结束后出发：正常收尾由直播流自己做，这里
        // 低频兜底一次，覆盖停止、发送失败与后台目标结束这些没有直播流收尾
        // 的路径。引擎侧对空队列是幂等的，重复调用不会多发一条。
        if host_ticks % 30 == 0
            && ui.get_settings_chat_target() == 0
            && ui.get_queue_turns().row_count() > 0
            && !ui.get_generating()
        {
            let _ = commands.send(Command::ReleaseWaiting);
        }
        // Cloud session polling and approval intake run at background cadence,
        // not at the 30 fps render tick.
        if host_ticks % 45 == 0 && ui.get_settings_chat_target() == 1 {
            let _ = commands.send(Command::CloudPoll);
        }
        if host_ticks % 60 == 0 && ui.get_settings_cloud_logged_in() {
            let _ = commands.send(Command::PollApprovals);
        }
        if host_ticks % 300 == 0 {
            let _ = commands.send(Command::RefreshCloudAccount);
            let _ = commands.send(Command::RefreshDevices);
        }
        if host_ticks % 1800 == 0 && ui.get_settings_cloud_logged_in() {
            let _ = commands.send(Command::RefreshCloudRemote);
        }
        let Ok(state) = shared.lock() else { return };

        // 运行中键入的消息只停在输入框上沿：对话里不会有第二条气泡，也
        // 不会出现用户没要求的「排队中」状态。待发条属于本机会话，切到
        // 云端时必须清空，否则本地线程的等待行会挂在云端会话的输入框上。
        let waiting_key = (state.waiting_rev, state.session_id.clone(), state.target);
        if waiting_key != last_waiting {
            last_waiting = waiting_key;
            let rows: Vec<crate::QueueTurn> = if state.target == 0 {
                state
                    .waiting_turns
                    .iter()
                    .filter(|turn| turn.session == state.session_id)
                    .map(|turn| crate::QueueTurn {
                        queue_id: turn.queue_id.as_str().into(),
                        text: turn.text.as_str().into(),
                        attachments: 0,
                        interjected: false,
                        waiting: true,
                    })
                    .collect()
            } else {
                Vec::new()
            };
            ui.set_queue_turns(ModelRc::from(Rc::new(VecModel::from(rows))));
        }

        let uploads_key = (state.uploads_rev, state.target);
        if uploads_key != last_uploads {
            last_uploads = uploads_key;
            let cards: Vec<crate::ChatAttachment> = if state.target == 0 {
                state
                    .pending_uploads
                    .iter()
                    .map(|file| crate::ChatAttachment {
                        name: file.name.as_str().into(),
                        mime_type: slint::SharedString::new(),
                        data_url: slint::SharedString::new(),
                        preview: file
                            .preview
                            .as_ref()
                            .map(|preview| {
                                slint::Image::from_rgba8(
                                    slint::SharedPixelBuffer::clone_from_slice(
                                        &preview.rgba,
                                        preview.width,
                                        preview.height,
                                    ),
                                )
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            ui.set_attachments(ModelRc::from(Rc::new(VecModel::from(cards))));
        }

        // 加号面板的预设问题跟着智能体投影走，云端线程不显示本机预设。
        let presets_key = (state.preset_rev, state.target);
        if presets_key != last_presets {
            last_presets = presets_key;
            let questions: Vec<slint::SharedString> = if state.target == 0 {
                state
                    .preset_questions
                    .iter()
                    .map(|question| question.as_str().into())
                    .collect()
            } else {
                Vec::new()
            };
            ui.set_preset_questions(ModelRc::from(Rc::new(VecModel::from(questions))));
        }

        // 语音：录音条自己走秒，所以计时每帧算、只在文本变化时写入；
        // 阶段与识别路线按修订号投影，避免每帧覆盖 Slint 侧状态。
        let voice_key = (state.voice_rev, state.voice_route);
        if voice_key != last_voice {
            last_voice = voice_key;
            ui.set_voice_phase(state.voice_phase);
            ui.set_voice_system(state.voice_route == 2);
        }
        let voice_elapsed = match (state.voice_phase, state.voice_started) {
            (1, Some(started)) => {
                let seconds = started.elapsed().as_secs().min(120);
                format!("{:02}:{:02}", seconds / 60, seconds % 60)
            }
            _ => String::new(),
        };
        if voice_elapsed != last_voice_elapsed {
            last_voice_elapsed = voice_elapsed;
            ui.set_voice_elapsed(last_voice_elapsed.as_str().into());
        }

        if state.rev != last_rev {
            ui.set_send_key(state.send_key.as_str().into());
            if state.target == 0 {
                ui.set_thread_title(state.title.as_str().into());
                ui.set_thread_subtitle(state.subtitle.as_str().into());
                ui.set_workspace_name(state.workspace.as_str().into());
                ui.set_workspace_id(state.workspace_id.as_str().into());
                ui.set_generating(state.generating);
                ui.set_stopping(state.stopping);
                ui.set_loading(state.loading);
                ui.set_spinning(state.generating || state.loading);
                ui.set_model_name(state.model_name.as_str().into());
                ui.set_model_key(state.model_key.as_str().into());
                ui.set_stream_revision(state.stream_revision);
            }
            last_rev = state.rev;
        }

        if state.target == 0 && state.sessions_rev != last_sessions {
            ui.set_sessions(ModelRc::from(Rc::new(VecModel::from(
                state.sessions.clone(),
            ))));
            last_sessions = state.sessions_rev;
        }

        if state.target_rev != last_target_rev {
            ui.set_settings_chat_target(state.target);
            if state.target == 1 {
                ui.set_sessions(ModelRc::from(Rc::new(VecModel::from(
                    state.cloud_threads.clone(),
                ))));
                let active = &state.cloud_active;
                ui.set_thread_title(
                    if active.title.is_empty() {
                        "云端会话"
                    } else {
                        active.title.as_str()
                    }
                    .into(),
                );
                ui.set_thread_subtitle(
                    if active.status.is_empty() {
                        "云端".to_string()
                    } else {
                        format!("云端 · {}", active.status)
                    }
                    .into(),
                );
                ui.set_workspace_name("云端工作区".into());
                ui.set_workspace_id("".into());
                ui.set_model_name(active.model.as_str().into());
                ui.set_model_key(active.model.as_str().into());
                ui.set_generating(false);
                ui.set_stopping(false);
                ui.set_loading(active.loading);
                ui.set_spinning(active.loading);
                ui.set_inquiry_questions(ModelRc::default());
                ui.set_inquiry_page(0);
                ui.set_inquiry_total(0);
                ui.set_inquiry_expanded(false);
                ui.set_inquiry_ready(false);
                // 目标与计划只属于本机会话。
                ui.set_goal_visible(false);
                ui.set_goal_active(false);
                ui.set_goal_paused(false);
                ui.set_activity_steps_total(0);
                ui.set_activity_running(false);
                ui.set_activity_plan_complete(false);
                ui.set_activity_plan_steps(ModelRc::default());
                // 后台任务与子智能体是本地线程的运行时状态，云端线程没有。
                ui.set_task_rows(ModelRc::default());
                ui.set_task_count(0);
                ui.set_subagent_rows(ModelRc::default());
                ui.set_subagent_count(0);
                // 切回本地时按本地修订号重新投影一次。
                last_sheet_activity = u64::MAX;
                if active.rows.is_empty() {
                    model.set_vec(Vec::new());
                } else {
                    model.set_vec(cache.from_iter(active.rows.iter()));
                }
                let mark_rows: Vec<&RowData> = active.rows.iter().collect();
                apply_turn_marks(
                    &turn_marks_model,
                    &mut last_marks,
                    project::turn_marks(&mark_rows),
                );
            } else {
                ui.set_sessions(ModelRc::from(Rc::new(VecModel::from(
                    state.sessions.clone(),
                ))));
                ui.set_thread_title(state.title.as_str().into());
                ui.set_thread_subtitle(state.subtitle.as_str().into());
                ui.set_workspace_name(state.workspace.as_str().into());
                ui.set_workspace_id(state.workspace_id.as_str().into());
                ui.set_generating(state.generating);
                ui.set_stopping(state.stopping);
                ui.set_loading(state.loading);
                ui.set_spinning(state.generating || state.loading);
                ui.set_model_name(state.model_name.as_str().into());
                ui.set_model_key(state.model_key.as_str().into());
                last_rows = u64::MAX;
                last_live = u64::MAX;
                last_inquiry = u64::MAX;
            }
            // 切回本机时强制重新投影目标与计划。
            last_goal = u64::MAX;
            last_activity = u64::MAX;
            last_target_rev = state.target_rev;
        }

        if state.cloud_rev != last_cloud {
            ui.set_settings_cloud_logged_in(state.cloud.logged_in);
            ui.set_settings_cloud_expired(state.cloud.expired);
            ui.set_settings_cloud_connection(state.cloud.connection.as_str().into());
            ui.set_settings_cloud_server(state.cloud.server.as_str().into());
            ui.set_settings_cloud_username(state.cloud.username.as_str().into());
            ui.set_settings_cloud_balance(state.cloud.balance.min(i32::MAX as i64) as i32);
            ui.set_settings_cloud_daily_grant(state.cloud.daily_grant.min(i32::MAX as i64) as i32);
            ui.set_settings_cloud_max_concurrent(
                state.cloud.max_concurrent.min(i32::MAX as u64) as i32
            );
            ui.set_settings_cloud_active_concurrent(
                state.cloud.active_concurrent.min(i32::MAX as u64) as i32,
            );
            ui.set_settings_cloud_sync_enabled(state.cloud.sync_enabled);
            ui.set_settings_cloud_inbound_enabled(state.cloud.inbound_enabled);
            ui.set_settings_cloud_full_shadow(state.cloud.full_shadow);
            ui.set_settings_cloud_last_error(state.cloud.last_error.as_str().into());
            ui.set_settings_cloud_last_success(state.cloud.last_success_at.as_str().into());
            ui.set_settings_cloud_next_retry(state.cloud.next_retry_at.as_str().into());
            ui.set_settings_cloud_device_name(state.cloud.device_name.as_str().into());
            last_cloud = state.cloud_rev;
        }

        if state.usage_rev != last_usage {
            ui.set_settings_usage_duration(state.usage_duration.as_str().into());
            ui.set_settings_usage_tokens(state.usage_tokens.as_str().into());
            ui.set_settings_usage_quota(state.usage_quota.as_str().into());
            last_usage = state.usage_rev;
        }

        if state.devices_rev != last_devices {
            ui.set_settings_devices(ModelRc::from(Rc::new(VecModel::from(
                state
                    .devices
                    .iter()
                    .map(|device| crate::MobileDevice {
                        id: device.id.as_str().into(),
                        kind: device.kind.as_str().into(),
                        label: device.label.as_str().into(),
                        status: device.status.as_str().into(),
                        connected: device.connected,
                        current: device.current,
                        selectable: device.selectable,
                    })
                    .collect::<Vec<_>>(),
            ))));
            let current = state.devices.iter().find(|device| device.current);
            let current_label = current
                .map(|device| device.label.clone())
                .unwrap_or_else(|| "本机".to_string());
            let current_kind = current
                .map(|device| device.kind.clone())
                .unwrap_or_else(|| "phone".to_string());
            ui.set_settings_current_device(current_label.into());
            ui.set_settings_current_device_kind(current_kind.into());
            last_devices = state.devices_rev;
        }

        if state.cloud_threads_rev != last_cloud_threads && state.target == 1 {
            ui.set_sessions(ModelRc::from(Rc::new(VecModel::from(
                state.cloud_threads.clone(),
            ))));
            last_cloud_threads = state.cloud_threads_rev;
        }

        if state.target == 1 && state.cloud_active_rev != last_cloud_active {
            let active = &state.cloud_active;
            ui.set_thread_title(
                if active.title.is_empty() {
                    "云端会话"
                } else {
                    active.title.as_str()
                }
                .into(),
            );
            ui.set_thread_subtitle(
                if active.status.is_empty() {
                    "云端".to_string()
                } else {
                    format!("云端 · {}", active.status)
                }
                .into(),
            );
            ui.set_model_name(active.model.as_str().into());
            ui.set_model_key(active.model.as_str().into());
            ui.set_generating(matches!(active.status.as_str(), "running" | "queued"));
            ui.set_stopping(false);
            ui.set_loading(active.loading);
            ui.set_spinning(active.loading || active.status == "running");
            ui.set_stream_revision(state.cloud_active_rev.min(i32::MAX as u64) as i32);
            last_cloud_active = state.cloud_active_rev;
        }

        if state.approval_rev != last_approval {
            match &state.approval {
                Some(approval) => {
                    ui.set_settings_approval_open(true);
                    ui.set_settings_approval_id(approval.id.as_str().into());
                    ui.set_settings_approval_from(approval.from_node.as_str().into());
                    ui.set_settings_approval_kind(approval.kind.as_str().into());
                    ui.set_settings_approval_risk(approval.risk.as_str().into());
                    ui.set_settings_approval_prompt(approval.prompt.as_str().into());
                }
                None => ui.set_settings_approval_open(false),
            }
            last_approval = state.approval_rev;
        }

        if state.channels_rev != last_channels {
            ui.set_settings_channel_accounts(ModelRc::from(Rc::new(VecModel::from(
                state
                    .channels
                    .accounts
                    .iter()
                    .map(|account| crate::MobileChannelAccount {
                        channel: account.channel.as_str().into(),
                        channel_name: account.channel_name.as_str().into(),
                        account_id: account.account_id.as_str().into(),
                        name: account.name.as_str().into(),
                        status: account.status.as_str().into(),
                        active: account.active,
                        configured: account.configured,
                        summary: account.summary.as_str().into(),
                        secret_set: account.secret_set,
                    })
                    .collect::<Vec<_>>(),
            ))));
            ui.set_settings_channel_catalog(ModelRc::from(Rc::new(VecModel::from(
                state
                    .channels
                    .catalog
                    .iter()
                    .map(|item| crate::MobileChannelCatalog {
                        channel: item.channel.as_str().into(),
                        name: item.name.as_str().into(),
                        description: item.description.as_str().into(),
                    })
                    .collect::<Vec<_>>(),
            ))));
            ui.set_settings_channel_catalog_ids(ModelRc::from(Rc::new(VecModel::from(
                state
                    .channels
                    .catalog
                    .iter()
                    .map(|item| item.channel.as_str().into())
                    .collect::<Vec<slint::SharedString>>(),
            ))));
            ui.set_settings_channel_catalog_names(ModelRc::from(Rc::new(VecModel::from(
                state
                    .channels
                    .catalog
                    .iter()
                    .map(|item| item.name.as_str().into())
                    .collect::<Vec<slint::SharedString>>(),
            ))));
            ui.set_settings_channel_bindings(ModelRc::from(Rc::new(VecModel::from(
                state
                    .channels
                    .bindings
                    .iter()
                    .map(|binding| crate::MobileChannelBinding {
                        binding_id: binding.binding_id.as_str().into(),
                        channel: binding.channel.as_str().into(),
                        account_id: binding.account_id.as_str().into(),
                        peer_kind: binding.peer_kind.as_str().into(),
                        peer_id: binding.peer_id.as_str().into(),
                        agent_id: binding.agent_id.as_str().into(),
                        enabled: binding.enabled,
                    })
                    .collect::<Vec<_>>(),
            ))));
            ui.set_settings_channel_logs(ModelRc::from(Rc::new(VecModel::from(
                state
                    .channels
                    .logs
                    .iter()
                    .map(|log| crate::MobileChannelLog {
                        time: log.time.as_str().into(),
                        level: log.level.as_str().into(),
                        event: log.event.as_str().into(),
                        message: log.message.as_str().into(),
                    })
                    .collect::<Vec<_>>(),
            ))));
            ui.set_settings_channel_selected(state.channels.selected.as_str().into());
            ui.set_settings_channel_logs_open(state.channels.logs_open);
            last_channels = state.channels_rev;
        }

        if state.channel_editor_rev != last_channel_editor {
            match &state.channel_editor {
                Some(editor) => {
                    ui.set_settings_channel_editor_open(true);
                    ui.set_settings_channel_editor_new(editor.is_new);
                    ui.set_settings_channel_editor_error(editor.error.as_str().into());
                    if !channel_editor_was_open {
                        ui.set_settings_channel_draft(editor.draft.clone());
                    }
                    channel_editor_was_open = true;
                }
                None => {
                    ui.set_settings_channel_editor_open(false);
                    channel_editor_was_open = false;
                }
            }
            last_channel_editor = state.channel_editor_rev;
        }

        if state.channel_qr_rev != last_channel_qr {
            let qr = &state.channel_qr;
            ui.set_settings_channel_qr_open(qr.open);
            ui.set_settings_channel_qr_status(qr.status);
            ui.set_settings_channel_qr_message(qr.message.as_str().into());
            if qr.image_width > 0 && qr.image_height > 0 && !qr.image_rgba.is_empty() {
                // The worker decoded the QR off the UI thread; wrapping the
                // pixels is the only per-frame-ish cost and it happens once
                // per new code, not per tick.
                let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                    &qr.image_rgba,
                    qr.image_width,
                    qr.image_height,
                );
                ui.set_settings_channel_qr_image(slint::Image::from_rgba8(buffer));
            } else {
                ui.set_settings_channel_qr_image(slint::Image::default());
            }
            // 扫码确认的凭据只改它自己的四个字段，其余内容取界面上的当前
            // 草稿：用户等待扫码时输入的账号名称不会被整份草稿盖掉。
            if let Some(credentials) = qr.pending.as_ref() {
                if qr.pending_seq != last_channel_qr_pending {
                    let mut draft = ui.get_settings_channel_draft();
                    if draft.channel.is_empty() {
                        draft.channel = "weixin".into();
                    }
                    if !credentials.bot_token.is_empty() {
                        draft.weixin_bot_token = credentials.bot_token.as_str().into();
                    }
                    if !credentials.bot_id.is_empty() {
                        draft.weixin_bot_id = credentials.bot_id.as_str().into();
                    }
                    if !credentials.user_id.is_empty() {
                        draft.weixin_user_id = credentials.user_id.as_str().into();
                    }
                    if !credentials.api_base.is_empty() {
                        draft.weixin_api_base = credentials.api_base.as_str().into();
                    }
                    if draft.name.trim().is_empty() {
                        draft.name = "微信机器人".into();
                    }
                    draft.enabled = true;
                    ui.set_settings_channel_draft(draft);
                    last_channel_qr_pending = qr.pending_seq;
                }
            }
            last_channel_qr = state.channel_qr_rev;
        }

        if state.archived_rev != last_archived {
            ui.set_archived_sessions(ModelRc::from(Rc::new(VecModel::from(
                state.archived_sessions.clone(),
            ))));
            ui.set_archived_total(state.archived_total as i32);
            last_archived = state.archived_rev;
        }

        // 目标横幅：本机线程的目标状态，随引擎观察实时更新。
        if state.target == 0 && state.goal_rev != last_goal {
            let goal = &state.goal;
            ui.set_goal_visible(goal.visible());
            ui.set_goal_active(goal.active());
            ui.set_goal_paused(goal.paused());
            ui.set_goal_objective(goal.objective.as_str().into());
            ui.set_goal_phase(goal.phase.as_str().into());
            ui.set_goal_activation(goal.activation.as_str().into());
            ui.set_goal_rounds(format!("{}/{}", goal.rounds_started, goal.max_goal_rounds).into());
            ui.set_goal_elapsed(goal_elapsed(goal.seconds).into());
            last_goal = state.goal_rev;
        }

        // 计划面板：步骤清单、完成数、当前步骤与展开态。
        if state.target == 0 && state.plan_rev != last_activity {
            let plan = &state.plan;
            let total = plan.steps.len() as i32;
            let done = plan
                .steps
                .iter()
                .filter(|step| step.status == "completed")
                .count() as i32;
            let (current, position) = plan_current(&plan.steps);
            ui.set_activity_running(plan.running);
            // 与桌面版一致：清单全部完成时胶囊换成对勾并停住，而不是继续转。
            ui.set_activity_plan_complete(engine::plan_all_complete(&plan.steps));
            ui.set_activity_steps_done(done);
            ui.set_activity_steps_total(total);
            ui.set_activity_plan_explanation(plan.explanation.as_str().into());
            ui.set_activity_plan_current(current.into());
            ui.set_activity_plan_position(position);
            let rows: Vec<crate::PlanStep> = plan
                .steps
                .iter()
                .map(|step| crate::PlanStep {
                    step: step.step.as_str().into(),
                    status: step.status.as_str().into(),
                })
                .collect();
            ui.set_activity_plan_steps(ModelRc::from(Rc::new(VecModel::from(rows))));
            last_activity = state.plan_rev;
        }

        if state.target == 0 {
            // 自由文本不重投影（保住光标与输入法候选），但提交键的门禁要跟着每个
            // 按键走：面板在的时候每帧按当前答案重算一次，等价于桌面版在
            // `on_inquiry_other_edited` 里直接写 `inquiry-ready`。
            if let Some(panel) = state.inquiry.as_ref() {
                ui.set_inquiry_ready(panel.has_answer());
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
                        ui.set_inquiry_ready(panel.has_answer());
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
            // 本地工具审批：权限审批选「建议 / 自动编辑」后，运行时把这一步挂在
            // 请求通道上等回答，回答前整轮停住。
            if state.chat_approval_rev != last_chat_approval {
                match state.chat_approval.as_ref() {
                    Some(approval) => {
                        ui.set_chat_approval_open(true);
                        ui.set_chat_approval_id(approval.id.as_str().into());
                        ui.set_chat_approval_tool(approval.tool.as_str().into());
                        ui.set_chat_approval_kind(approval.kind.as_str().into());
                        ui.set_chat_approval_summary(approval.summary.as_str().into());
                    }
                    None => {
                        ui.set_chat_approval_open(false);
                        ui.set_chat_approval_id("".into());
                        ui.set_chat_approval_tool("".into());
                        ui.set_chat_approval_kind("".into());
                        ui.set_chat_approval_summary("".into());
                    }
                }
                last_chat_approval = state.chat_approval_rev;
            }
        }

        if state.trajectory_rev != last_trajectory {
            ui.set_trajectory_title(state.trajectory_title.as_str().into());
            ui.set_trajectory_loading(state.trajectory_loading);
            ui.set_trajectory_error(state.trajectory_error.as_str().into());
            ui.set_trajectory_filter(state.trajectory_filter);
            ui.set_trajectory_items(ModelRc::from(Rc::new(VecModel::from(filtered_trajectory(
                &state.trajectory_items,
                state.trajectory_filter,
            )))));
            ui.set_trajectory_audit(state.trajectory_audit.clone());
            ui.set_trajectory_segments(ModelRc::from(Rc::new(VecModel::from(
                state.trajectory_segments.clone(),
            ))));
            ui.set_trajectory_bars(ModelRc::from(Rc::new(VecModel::from(
                state.trajectory_bars.clone(),
            ))));
            last_trajectory = state.trajectory_rev;
        }

        if state.prompt_preview_rev != last_prompt_preview {
            ui.set_prompt_preview_loading(state.prompt_preview_loading);
            ui.set_prompt_preview(state.prompt_preview.as_str().into());
            ui.set_prompt_preview_error(state.prompt_preview_error.as_str().into());
            last_prompt_preview = state.prompt_preview_rev;
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

        if state.workspaces_rev != last_workspaces {
            ui.set_workspaces(ModelRc::from(Rc::new(VecModel::from(
                state.workspaces.clone(),
            ))));
            if let Some(active) = state.workspaces.iter().find(|workspace| workspace.active) {
                ui.set_workspace_icon(active.icon.clone());
                ui.set_workspace_color(active.color.clone());
                ui.set_workspace_thread_count(active.thread_count);
            }
            last_workspaces = state.workspaces_rev;
        }

        if state.models_rev != last_models {
            ui.set_models(ModelRc::from(Rc::new(VecModel::from(state.models.clone()))));
            ui.set_model_catalogue_count(state.models.len() as i32);
            // Only the seeded 虚拟模型 (provider `virtual_replay`) is present:
            // chatting cannot reach a real LLM, so the chat surface offers the
            // first-run setup banner. The literal mirrors
            // services::virtual_llm::VIRTUAL_REPLAY_PROVIDER.
            ui.set_needs_model_setup(
                !state.models.is_empty()
                    && state
                        .models
                        .iter()
                        .all(|model| model.provider == "virtual_replay"),
            );
            last_models = state.models_rev;
        }

        if state.tools_rev != last_tools {
            // MCP 与技能各自成页，只投影资源列表；分类卡片只服务智能体工具选择。
            ui.set_settings_tools_servers(ModelRc::from(Rc::new(VecModel::from(
                state.tools.servers.clone(),
            ))));
            ui.set_settings_tools_skill_resources(ModelRc::from(Rc::new(VecModel::from(
                state.tools.skills.clone(),
            ))));
            ui.set_settings_tool_server_tools(ModelRc::from(Rc::new(VecModel::from(
                state.tools.server_tools.clone(),
            ))));
            ui.set_settings_tool_active_server(state.tools.active_server.as_str().into());
            ui.set_settings_tool_active_resource(state.tools.browse_resource.as_str().into());
            ui.set_settings_tool_resource_tab(state.tools.browse_tab);
            ui.set_settings_tool_resource_editable(state.tools.browse_editable);
            ui.set_settings_tool_files(ModelRc::from(Rc::new(VecModel::from(
                state.tools.files.clone(),
            ))));
            ui.set_settings_tool_directory(state.tools.directory.as_str().into());
            ui.set_settings_tool_has_more(state.tools.has_more);
            ui.set_settings_tool_connecting(state.tools.connecting.as_str().into());
            last_tools = state.tools_rev;
        }

        if state.tool_editor_rev != last_tool_editor {
            match &state.tool_editor {
                Some(editor) => {
                    ui.set_settings_tool_editor_kind(editor.kind);
                    ui.set_settings_tool_editor_new(editor.is_new);
                    ui.set_settings_tool_editor_error(editor.error.as_str().into());
                    if !tool_editor_was_open {
                        ui.set_settings_mcp_draft(editor.mcp.clone());
                        ui.set_settings_kb_draft(editor.kb.clone());
                    }
                    tool_editor_was_open = true;
                }
                None => {
                    ui.set_settings_tool_editor_kind(0);
                    tool_editor_was_open = false;
                }
            }
            last_tool_editor = state.tool_editor_rev;
        }

        if state.tool_file_rev != last_tool_file {
            match &state.tool_file {
                Some(file) => {
                    if !tool_file_was_open {
                        ui.set_settings_tool_file_content(file.content.as_str().into());
                    }
                    ui.set_settings_tool_file_path(file.path.as_str().into());
                    ui.set_settings_tool_file_readonly(file.readonly);
                    ui.set_settings_tool_file_error(file.error.as_str().into());
                    ui.set_settings_tool_file_open(true);
                    tool_file_was_open = true;
                }
                None => {
                    ui.set_settings_tool_file_open(false);
                    tool_file_was_open = false;
                }
            }
            last_tool_file = state.tool_file_rev;
        }

        if state.greeting_rev != last_greeting {
            // 桌面版把内置智能体的问候语当作转录区顶部的欢迎语；手机沿用
            // 同一句话，空串时才回落到 I18n.chat_greeting。
            ui.set_greeting(state.greeting.as_str().into());
            last_greeting = state.greeting_rev;
        }

        if state.cloud_greeting_rev != last_cloud_greeting {
            ui.set_cloud_greeting(state.cloud_greeting.as_str().into());
            last_cloud_greeting = state.cloud_greeting_rev;
        }

        if state.agent_rev != last_agent {
            ui.set_settings_agent_name(state.agent.name.as_str().into());
            ui.set_settings_agent_tool_count(state.agent.tool_count);
            let groups: Vec<crate::MobileToolGroup> = state
                .agent
                .groups
                .iter()
                .map(|group| crate::MobileToolGroup {
                    category: group.category.as_str().into(),
                    title: group.title.as_str().into(),
                    tools: ModelRc::new(VecModel::from(group.tools.clone())),
                    all_selected: group.all_selected,
                })
                .collect();
            ui.set_settings_agent_groups(ModelRc::from(Rc::new(VecModel::from(groups))));
            let questions: Vec<slint::SharedString> = state
                .agent
                .preset_questions
                .iter()
                .map(|question| question.as_str().into())
                .collect();
            ui.set_settings_agent_questions(ModelRc::from(Rc::new(VecModel::from(questions))));
            ui.set_settings_agent_approval_mode(state.agent.approval_mode.as_str().into());
            if state.agent.seed != last_agent_seed {
                ui.set_settings_agent_description(state.agent.description.as_str().into());
                ui.set_settings_agent_prompt(state.agent.system_prompt.as_str().into());
                ui.set_settings_agent_search(state.agent.search.as_str().into());
                last_agent_seed = state.agent.seed;
            }
            last_agent = state.agent_rev;
        }

        if state.cron_rev != last_cron {
            ui.set_settings_cron_jobs(ModelRc::from(Rc::new(VecModel::from(
                state.cron.jobs.clone(),
            ))));
            ui.set_settings_cron_workspaces(ModelRc::from(Rc::new(VecModel::from(
                state
                    .cron
                    .workspaces
                    .iter()
                    .map(|workspace| workspace.name.as_str().into())
                    .collect::<Vec<slint::SharedString>>(),
            ))));
            last_cron = state.cron_rev;
        }

        if state.cron_editor_rev != last_cron_editor {
            match &state.cron_editor {
                Some(editor) => {
                    ui.set_settings_cron_editor_open(true);
                    ui.set_settings_cron_editor_new(editor.is_new);
                    ui.set_settings_cron_editor_error(editor.error.as_str().into());
                    ui.set_settings_cron_editor_saving(editor.saving);
                    if !cron_editor_was_open {
                        ui.set_settings_cron_draft(editor.draft.clone());
                    }
                    cron_editor_was_open = true;
                }
                None => {
                    ui.set_settings_cron_editor_open(false);
                    ui.set_settings_cron_editor_saving(false);
                    cron_editor_was_open = false;
                }
            }
            last_cron_editor = state.cron_editor_rev;
        }

        if state.cron_runs_rev != last_cron_runs {
            ui.set_settings_cron_runs_open(state.cron_runs.open);
            ui.set_settings_cron_runs_loading(state.cron_runs.loading);
            ui.set_settings_cron_runs_title(state.cron_runs.title.as_str().into());
            ui.set_settings_cron_runs(ModelRc::from(Rc::new(VecModel::from(
                state.cron_runs.runs.clone(),
            ))));
            last_cron_runs = state.cron_runs_rev;
        }

        if state.memory_rev != last_memory {
            ui.set_settings_memories(ModelRc::from(Rc::new(VecModel::from(
                state.memory.memories.clone(),
            ))));
            ui.set_settings_memory_categories(ModelRc::from(Rc::new(VecModel::from(
                state
                    .memory
                    .categories
                    .iter()
                    .map(|category| category.as_str().into())
                    .collect::<Vec<slint::SharedString>>(),
            ))));
            ui.set_settings_memory_category(state.memory.category.as_str().into());
            ui.set_settings_memory_loading(state.memory.loading);
            ui.set_settings_memory_error(state.memory.error.as_str().into());
            if state.memory.seed != last_memory_seed {
                ui.set_settings_memory_query(state.memory.query.as_str().into());
                last_memory_seed = state.memory.seed;
            }
            last_memory = state.memory_rev;
        }

        if state.memory_editor_rev != last_memory_editor {
            match &state.memory_editor {
                Some(editor) => {
                    ui.set_settings_memory_editor_open(true);
                    ui.set_settings_memory_editor_new(editor.is_new);
                    ui.set_settings_memory_editor_error(editor.error.as_str().into());
                    ui.set_settings_memory_editor_saving(editor.saving);
                    if !memory_editor_was_open {
                        ui.set_settings_memory_draft(editor.draft.clone());
                    }
                    memory_editor_was_open = true;
                }
                None => {
                    ui.set_settings_memory_editor_open(false);
                    ui.set_settings_memory_editor_saving(false);
                    memory_editor_was_open = false;
                }
            }
            last_memory_editor = state.memory_editor_rev;
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
            ui.set_settings_language(state.settings.language.as_str().into());
            // 语言投影后立刻落一次 `I18n.zh`：不依赖 `changed ui-zh` 的通知时序，
            // 保存语言后整屏文案在同一帧切过去。
            ui.invoke_ui_language_changed(ui.get_ui_zh());
            ui.set_settings_workspace(state.settings.workspace_root.as_str().into());
            ui.set_settings_version(state.settings.version.as_str().into());
            ui.set_update_channel(
                if state.settings.update_source == "github" {
                    "github"
                } else {
                    "cloud"
                }
                .into(),
            );
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
            // 首装引导：还没有可用运行环境（等待自动安装或安装中）时全屏展示。
            env_progress = next_env_progress(
                env_progress,
                state.ubuntu.state.as_str(),
                state.ubuntu.progress,
            );
            ui.set_env_setup_progress(env_progress);
            if env_setup_should_open(state.ubuntu.state.as_str(), state.env_setup_dismissed) {
                ui.set_env_setup_open(true);
            }
            last_ubuntu = state.ubuntu_rev;
        }

        // 安装完成先停留片刻展示「已就绪」，再放行聊天界面。
        if ui.get_env_setup_open() {
            if state.ubuntu.state == "installed" {
                let ready_at = *env_ready_at.get_or_insert_with(Instant::now);
                // 液面缓动 850ms，再留约 0.6s 给完成时的气泡潮。
                if ready_at.elapsed() >= Duration::from_millis(1500) {
                    ui.set_env_setup_open(false);
                    env_ready_at = None;
                }
            } else if state.ubuntu.state == "unsupported" {
                ui.set_env_setup_open(false);
                env_ready_at = None;
            } else {
                env_ready_at = None;
            }
        }

        if state.companion_rev != last_companion {
            ui.set_settings_companion_enabled(state.companion.enabled);
            ui.set_settings_companion_selected(state.companion.selected_id.as_str().into());
            let selected_name = state
                .companion
                .companions
                .iter()
                .find(|choice| choice.id == state.companion.selected_id)
                .map(|choice| choice.name.as_str())
                .unwrap_or("");
            ui.set_settings_companion_selected_name(selected_name.into());
            ui.set_settings_companion_scale(state.companion.scale);
            ui.set_settings_companion_hints(state.companion.message_hints);
            ui.set_settings_companion_permission(state.companion.permission.as_str().into());
            let names: Vec<slint::SharedString> = state
                .companion
                .companions
                .iter()
                .map(|choice| choice.name.as_str().into())
                .collect();
            let ids: Vec<slint::SharedString> = state
                .companion
                .companions
                .iter()
                .map(|choice| choice.id.as_str().into())
                .collect();
            ui.set_settings_companion_names(ModelRc::from(Rc::new(VecModel::from(names))));
            ui.set_settings_companion_ids(ModelRc::from(Rc::new(VecModel::from(ids))));
            last_companion = state.companion_rev;
        }

        if state.permissions_rev != last_permissions {
            let items: Vec<MobilePermission> =
                state.permissions.iter().map(permission_row).collect();
            ui.set_permission_items(ModelRc::from(Rc::new(VecModel::from(items))));
            ui.set_permission_summary(state.permission_summary.as_str().into());
            last_permissions = state.permissions_rev;
        }

        if state.backup_rev != last_backup {
            ui.set_settings_backup_status(state.backup_message.as_str().into());
            ui.set_settings_backup_path(state.backup_path.as_str().into());
            let backups: Vec<MobileBackup> = state
                .backups
                .iter()
                .map(|entry| MobileBackup {
                    path: entry.path.as_str().into(),
                    name: entry.name.as_str().into(),
                    detail: entry.detail.as_str().into(),
                })
                .collect();
            ui.set_settings_backup_items(ModelRc::from(Rc::new(VecModel::from(backups))));
            ui.set_settings_backup_busy(state.backup_phase == "busy");
            ui.set_settings_backup_restart(state.backup_restart);
            ui.set_settings_crash_log_title(state.crash_log_title.as_str().into());
            ui.set_settings_crash_log_text(state.crash_log_text.as_str().into());
            last_backup = state.backup_rev;
        }

        if state.context_rev != last_context {
            ui.set_context_usage(state.context_usage);
            ui.set_context_known(state.context_known);
            ui.set_context_counts(state.context_counts.as_str().into());
            ui.set_context_compacted(state.context_compacted);
            last_context = state.context_rev;
        }

        // 模型浮层里的后台任务 / 子智能体：引擎每秒重投影，只在内容变化时
        // 写回 Slint 模型（行数已在 Rust 侧裁剪）。
        if state.target == 0 && state.activity_rev != last_sheet_activity {
            ui.set_task_rows(ModelRc::from(Rc::new(VecModel::from(
                state.activity.tasks.clone(),
            ))));
            ui.set_task_count(state.activity.task_count);
            ui.set_subagent_rows(ModelRc::from(Rc::new(VecModel::from(
                state.activity.subagents.clone(),
            ))));
            ui.set_subagent_count(state.activity.subagent_count);
            last_sheet_activity = state.activity_rev;
        }

        if state.update_rev != last_update {
            ui.set_update_phase(state.update_phase.as_str().into());
            ui.set_update_version(state.update_version.as_str().into());
            ui.set_update_notes(state.update_notes.as_str().into());
            ui.set_update_source(state.update_source.as_str().into());
            ui.set_update_size(state.update_size_text.as_str().into());
            ui.set_update_error(state.update_error.as_str().into());
            let percent = update_percent(
                state.update_progress_bytes.load(Ordering::Relaxed),
                state.update_size_bytes,
            );
            ui.set_update_progress(percent);
            let downloaded = update_progress_text(
                state.update_progress_bytes.load(Ordering::Relaxed),
                state.update_size_bytes,
            );
            ui.set_update_downloaded(downloaded.as_str().into());
            last_update_progress = percent;
            last_update_downloaded = downloaded;
            last_update = state.update_rev;
        } else if state.update_phase == "downloading" {
            // The download worker updates the atomic counter without touching
            // the revision; poll it here so the progress bar moves smoothly.
            let bytes = state.update_progress_bytes.load(Ordering::Relaxed);
            let percent = update_percent(bytes, state.update_size_bytes);
            if percent != last_update_progress {
                ui.set_update_progress(percent);
                last_update_progress = percent;
            }
            let text = update_progress_text(bytes, state.update_size_bytes);
            if text != last_update_downloaded {
                ui.set_update_downloaded(text.as_str().into());
                last_update_downloaded = text;
            }
        }

        if state.editor_rev != last_editor {
            if let Some(editor) = &state.editor {
                ui.set_editor_key(editor.original_key.as_str().into());
                ui.set_editor_new(editor.original_key.is_empty());
                ui.set_editor_default(editor.is_default);
                ui.set_editor_error(editor.error.as_str().into());
                ui.set_editor_probe(editor.probe_message.as_str().into());
                ui.set_editor_probing(editor.probing);
                ui.set_editor_saving(editor.saving);
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
                ui.set_editor_saving(false);
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

        if state.target == 1 && state.cloud_active_rev != last_cloud_rows {
            let rows = cache.from_iter(state.cloud_active.rows.iter());
            model.set_vec(rows);
            let mark_rows: Vec<&RowData> = state.cloud_active.rows.iter().collect();
            apply_turn_marks(
                &turn_marks_model,
                &mut last_marks,
                project::turn_marks(&mark_rows),
            );
            last_cloud_rows = state.cloud_active_rev;
        }

        if state.target == 0 && (state.rows_rev != last_rows || state.live_rev != last_live) {
            // While the local stream is attached its rows are authoritative
            // for that turn: the durable watcher publishes only commit
            // boundaries and would replace the live 请求中/模型输出中 footer
            // and the in-flight work-process steps with a stale copy.
            let live_active = live_is_authoritative(&state);
            let live = visible_live_rows(&state);
            let hide_root = if live_active {
                live_owner(&state).unwrap_or_default()
            } else {
                String::new()
            };
            let hide_root = hide_root.as_str();
            let durable_len = if hide_root.is_empty() {
                state.durable_rows.len()
            } else {
                state
                    .durable_rows
                    .iter()
                    .filter(|row| !belongs_to_turn(row, hide_root))
                    .count()
            };
            // A full rebuild is only needed when the durable prefix or its
            // visibility changed. A streaming token still patches just the
            // live tail, so the 33 ms tick never re-hashes the whole history.
            let full = state.rows_rev != last_rows
                || live_active != last_live_active
                || hide_root != last_hide_root.as_str()
                || durable_len != last_durable_len;
            if full {
                let mut rows = Vec::with_capacity(durable_len + live.len());
                for row in &state.durable_rows {
                    if !hide_root.is_empty() && belongs_to_turn(row, hide_root) {
                        continue;
                    }
                    rows.push(cache.row(row));
                }
                rows.extend(cache.from_iter(live.iter().copied()));
                model.set_vec(rows);
                last_rows = state.rows_rev;
                last_durable_len = durable_len;
            } else {
                let total = durable_len + live.len();
                let have = model.row_count();
                let rows = cache.from_iter(live.iter().copied());
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
            // The ruler follows the same visible rows as the transcript: the
            // durable prefix minus a hidden live twin, then the live tail.
            let mut mark_rows: Vec<&RowData> = Vec::with_capacity(durable_len + live.len());
            for row in &state.durable_rows {
                if !hide_root.is_empty() && belongs_to_turn(row, hide_root) {
                    continue;
                }
                mark_rows.push(row);
            }
            mark_rows.extend(live.iter().copied());
            apply_turn_marks(
                &turn_marks_model,
                &mut last_marks,
                project::turn_marks(&mark_rows),
            );
            last_live = state.live_rev;
            last_live_active = live_active;
            last_hide_root = hide_root.to_owned();
            last_patch = state.patch_rev;
            // Re-arm the transcript's follow-output pass for this frame. The
            // scalar revision is not bumped per token, so streaming frames
            // publish their own change signal here.
            ui.set_stream_revision(state.stream_revision);
        } else if state.patch_rev != last_patch {
            // Durable fold targets are absolute indexes before the hidden
            // live-root tail, so they map one-to-one onto the model prefix.
            if let Some(index) = state.patch_row {
                if index < last_durable_len {
                    if let Some(data) = state.durable_rows.get(index) {
                        model.set_row_data(index, cache.row(data));
                    }
                }
            }
            last_patch = state.patch_rev;
        }

        // Markdown images: merge decoded thumbnails, patch the cached block
        // models in place, then ask the engine for the remaining sources in a
        // bounded batch. The engine owns the workspace boundary and decoder.
        if state.images_rev != last_images {
            last_images = state.images_rev;
            cache.sync_images(&state.images);
            cache.hydrate_all();
        }
        for source in cache.take_wanted().into_iter().take(4) {
            let _ = commands.send(Command::LoadMarkdownImage { source });
        }

        // The long-press selection lives in the core's text layer; poll it so
        // the copy action can float above the composer. The grace window keeps
        // the bar alive while its own tap clears the selection on press.
        let selection = i_slint_core::text_selection::selection_text().unwrap_or_default();
        if !selection.is_empty() {
            selection_lost_at = None;
            let changed = ui.get_selection_text().as_str() != selection.as_str();
            last_selection = selection;
            if changed {
                let value = last_selection.as_str().into();
                ui.set_selection_text(value);
            }
        } else if !last_selection.is_empty() {
            let lost = *selection_lost_at.get_or_insert_with(Instant::now);
            if lost.elapsed() >= Duration::from_millis(700) {
                last_selection.clear();
                selection_lost_at = None;
                ui.set_selection_text("".into());
            }
        }

        // Two draggable knobs on the selection edges. Positions come from the
        // core's text layout, so they follow wrapping and scrolling; only the
        // changed values are pushed to avoid repainting the overlay.
        if let Some((start, end)) = i_slint_core::text_selection::handle_positions(ui.window()) {
            if !ui.get_selection_handles() {
                ui.set_selection_handles(true);
            }
            if (ui.get_selection_start_x() - start.x).abs() > 0.5 {
                ui.set_selection_start_x(start.x);
            }
            if (ui.get_selection_start_y() - start.y).abs() > 0.5 {
                ui.set_selection_start_y(start.y);
            }
            if (ui.get_selection_end_x() - end.x).abs() > 0.5 {
                ui.set_selection_end_x(end.x);
            }
            if (ui.get_selection_end_y() - end.y).abs() > 0.5 {
                ui.set_selection_end_y(end.y);
            }
            // The copy action floats next to the selected text: above the
            // first line, or below the last one when the header is in the way.
            let center_x = (start.x + end.x) / 2.0;
            if (ui.get_selection_bar_x() - center_x).abs() > 0.5 {
                ui.set_selection_bar_x(center_x);
            }
            let top = start.y.min(end.y);
            if (ui.get_selection_bar_y() - top).abs() > 0.5 {
                ui.set_selection_bar_y(top);
            }
            let bottom = start.y.max(end.y);
            if (ui.get_selection_bar_bottom() - bottom).abs() > 0.5 {
                ui.set_selection_bar_bottom(bottom);
            }
        } else if ui.get_selection_handles() {
            ui.set_selection_handles(false);
        }
    });
    // Slint cancels a dropped timer, so it must outlive `build`.
    TICK_TIMER.with(|slot| {
        *slot.borrow_mut() = Some(timer);
    });
}

/// Handle one Back press from the Android host. Every overlay is a level of
/// its own: Back closes the topmost one and only the chat root offers the
/// double-Back exit. Returns false when the activity should finish.
#[cfg(target_os = "android")]
fn handle_back(
    ui: &MobileWindow,
    shared: &Arc<Mutex<Shared>>,
    last_back: &mut Option<Instant>,
    ime_now: f32,
) -> bool {
    // The IME owns the first Back while the keyboard is up, matching every
    // Android input surface. The UI tick keeps `ime-height` current; the
    // composer keeps Slint's text focus, so the next tap re-opens the keyboard
    // through the backend's ensure-focus path. `ime_now` is the raw rect, so a
    // Back pressed while the keyboard is still animating is claimed by the IME
    // instead of arming the double-Back exit.
    if ui.get_ime_height() > 0.5 || ime_now > 0.5 {
        crate::android_host::hide_keyboard();
        *last_back = None;
        return true;
    }
    // 首装部署引导：安装期间返回键不离开页面；只有失败态可以退回聊天。
    if ui.get_env_setup_open() {
        if ui.get_settings_ubuntu_state() == "error" {
            ui.invoke_env_setup_dismiss();
        }
        *last_back = None;
        return true;
    }
    let mut closed = true;
    if ui.get_file_rename_open() {
        ui.set_file_rename_open(false);
    } else if ui.get_file_delete_open() {
        ui.set_file_delete_open(false);
    } else if ui.get_file_move_open() {
        ui.set_file_move_open(false);
    } else if ui.get_file_sheet_open() {
        ui.set_file_sheet_open(false);
    } else if ui.get_prompt_preview_open() {
        ui.set_prompt_preview_open(false);
    } else if ui.get_device_sheet_open() {
        ui.set_device_sheet_open(false);
    } else if ui.get_workspace_sheet_open() {
        ui.set_workspace_sheet_open(false);
        ui.set_workspace_delete_armed(false);
    } else if ui.get_settings_open()
        && ui.get_settings_page() == 14
        && ui.get_settings_channel_qr_open()
    {
        // 扫码面板叠在编辑面板之上，返回键先收起它并停掉轮询。
        ui.invoke_cancel_channel_qr();
    } else if ui.get_settings_open()
        && ui.get_settings_page() == 14
        && (ui.get_settings_channel_editor_open() || ui.get_settings_channel_logs_open())
    {
        if ui.get_settings_channel_editor_open() {
            ui.invoke_close_channel_editor();
        } else {
            ui.invoke_close_channel_logs();
        }
    } else if ui.get_rename_open() {
        ui.set_rename_open(false);
    } else if ui.get_model_sheet_open() {
        ui.set_model_sheet_open(false);
    } else if ui.get_plus_sheet_open() {
        // 加号面板是两层：先回三个入口，再收起整张面板。
        if ui.get_plus_sheet_page() > 0 {
            ui.set_plus_sheet_page(0);
        } else {
            ui.set_plus_sheet_open(false);
        }
    } else if ui.get_trajectory_open() {
        if ui.get_trajectory_expanded() >= 0 {
            ui.set_trajectory_expanded(-1);
        } else {
            ui.set_trajectory_open(false);
        }
    } else if ui.get_workspaces_open() {
        if ui.get_workspace_overlay() > 0 {
            ui.set_workspace_overlay(0);
        } else {
            ui.set_workspaces_open(false);
            ui.set_workspace_overlay(0);
        }
    } else if ui.get_workspace_open() {
        if !ui.get_workspace_path().is_empty() {
            ui.invoke_open_workspace(ui.get_workspace_parent_path());
        } else {
            ui.set_workspace_open(false);
        }
    } else if ui.get_settings_open() {
        if ui.get_editor_open() {
            ui.invoke_close_model_editor();
        } else if ui.get_settings_page() == 7 && ui.get_settings_tool_editor_kind() > 0 {
            ui.invoke_close_tool_editor();
        } else if ui.get_settings_page() == 7 && ui.get_settings_tool_file_open() {
            ui.invoke_close_tool_file();
        } else if ui.get_settings_page() == 9 && ui.get_settings_cron_editor_open() {
            ui.invoke_close_cron_editor();
        } else if ui.get_settings_page() == 9 && ui.get_settings_cron_runs_open() {
            ui.invoke_close_cron_runs();
        } else if ui.get_settings_page() == 10 && ui.get_settings_memory_editor_open() {
            ui.invoke_close_memory_editor();
        } else if ui.get_settings_page() == 16 {
            // 使用手册：读一篇时先回目录，目录再回「帮助与关于」。
            if ui.get_settings_help_topic_open() {
                ui.invoke_settings_help_back();
            } else {
                ui.set_settings_page(6);
            }
        } else if ui.get_settings_page() == 8 {
            // The on-screen back button flushes the agent form; hardware Back
            // bypasses Slint and must land the same pending text.
            let _ = crate::engine::acquire()
                .command_sender()
                .send(Command::SaveAgentText {
                    description: ui.get_settings_agent_description().to_string(),
                    system_prompt: ui.get_settings_agent_prompt().to_string(),
                });
            ui.set_settings_page(0);
        } else if ui.get_settings_page() != 0 {
            ui.set_settings_page(0);
        } else {
            ui.invoke_close_settings();
        }
    } else if ui.get_drawer_open() {
        ui.set_drawer_open(false);
    } else {
        closed = false;
    }
    if closed {
        *last_back = None;
        return true;
    }
    let now = Instant::now();
    if last_back.is_some_and(|last| now.duration_since(last) <= Duration::from_secs(2)) {
        return false;
    }
    *last_back = Some(now);
    if let Ok(mut state) = shared.lock() {
        state.notify("再按一次退出");
        state.touch();
    }
    true
}

/// Seed the window with a representative transcript for layout review on a
/// desktop (the `wunder-android-preview` binary). Kept next to the real
/// projection so preview rows go through the same renderer.
pub fn install_preview_data(
    ui: &MobileWindow,
    model: &Rc<VecModel<TimelineRow>>,
    workspaces: &Rc<VecModel<MobileWorkspace>>,
) {
    use crate::project::{KIND_BODY, KIND_ERROR, KIND_FOOTER, KIND_REASON, KIND_TOOL, KIND_USER};

    let sessions = vec![
        MobileSession {
            id: "preview-1".into(),
            title: "移动端聊天区对齐".into(),
            workspace: "心舰".into(),
            time: "11-06 14:20".into(),
            status: "running".into(),
            active: true,
        },
        MobileSession {
            id: "preview-2".into(),
            title: "检查无障碍控制链路".into(),
            workspace: "心舰".into(),
            time: "11-05 09:12".into(),
            status: "idle".into(),
            active: false,
        },
    ];
    ui.set_sessions(ModelRc::from(Rc::new(VecModel::from(sessions))));
    ui.set_thread_title("移动端聊天区对齐".into());
    ui.set_thread_subtitle("running · 心舰".into());
    ui.set_workspace_name("心舰".into());
    workspaces.set_vec(vec![
        MobileWorkspace {
            id: "preview-ws-1".into(),
            name: "心舰".into(),
            icon: "folder".into(),
            color: "blue".into(),
            root: "应用工作区 / 心舰".into(),
            thread_count: 3,
            active: true,
        },
        MobileWorkspace {
            id: "preview-ws-2".into(),
            name: "文档整理".into(),
            icon: "flask".into(),
            color: "teal".into(),
            root: "应用工作区 / 文档整理".into(),
            thread_count: 1,
            active: false,
        },
        MobileWorkspace {
            id: "preview-ws-3".into(),
            name: "实验场".into(),
            icon: "robot".into(),
            color: "orange".into(),
            root: "应用工作区 / 实验场".into(),
            thread_count: 0,
            active: false,
        },
    ]);
    ui.set_workspaces(ModelRc::from(workspaces.clone()));
    ui.set_workspace_icon("folder".into());
    ui.set_workspace_color("blue".into());
    ui.set_workspace_id("preview-ws-1".into());
    ui.set_workspace_thread_count(3);
    ui.set_model_name("预览模型".into());
    ui.set_model_key("preview-chat".into());
    ui.set_inquiry_questions(ModelRc::from(Rc::new(VecModel::from(vec![
        InquiryQuestion {
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
        },
    ]))));
    ui.set_inquiry_page(0);
    ui.set_inquiry_total(2);
    ui.set_inquiry_ready(false);
    // Settings-page fixtures: enough rows to review the index, the catalogue
    // and the editor on a workstation without starting the engine.
    ui.set_settings_theme("terracotta".into());
    ui.set_settings_font_size(16);
    ui.set_settings_language("zh-CN".into());
    // The preview has no engine, so nothing else would deliver the derived
    // language flag; the phone build gets it from the settings projection.
    ui.invoke_ui_language_changed(ui.get_ui_zh());
    ui.set_settings_workspace("应用私有目录 / workspaces".into());
    ui.set_settings_version(env!("CARGO_PKG_VERSION").into());
    ui.set_settings_ubuntu_state("installing".into());
    ui.set_settings_ubuntu_progress(42);
    ui.set_settings_ubuntu_stage("正在解压 Ubuntu 根文件系统…（4200 项）".into());
    ui.set_settings_ubuntu_enabled(true);
    ui.set_permission_items(ModelRc::from(Rc::new(VecModel::from(vec![
        MobilePermission {
            id: "notifications".into(),
            title: "通知".into(),
            description: "运行状态、完成提醒与前台服务通知".into(),
            state_text: "已开启".into(),
            action_text: "去管理".into(),
            granted: true,
            available: true,
        },
        MobilePermission {
            id: "microphone".into(),
            title: "麦克风".into(),
            description: "语音输入与转写".into(),
            state_text: "未开启".into(),
            action_text: "去开启".into(),
            granted: false,
            available: true,
        },
        MobilePermission {
            id: "overlay".into(),
            title: "悬浮窗".into(),
            description: "桌面伙伴显示在其他应用上层".into(),
            state_text: "已开启".into(),
            action_text: "去管理".into(),
            granted: true,
            available: true,
        },
        MobilePermission {
            id: "accessibility".into(),
            title: "无障碍服务".into(),
            description: "wunder 手机控制：界面读取、点击滑动输入与截图".into(),
            state_text: "未开启".into(),
            action_text: "去开启".into(),
            granted: false,
            available: true,
        },
        MobilePermission {
            id: "battery".into(),
            title: "电池优化".into(),
            description: "允许应用在后台持续执行任务".into(),
            state_text: "已开启".into(),
            action_text: "去管理".into(),
            granted: true,
            available: true,
        },
        MobilePermission {
            id: "install_packages".into(),
            title: "安装应用".into(),
            description: "在线更新与安装 APK".into(),
            state_text: "未开启".into(),
            action_text: "去开启".into(),
            granted: false,
            available: true,
        },
        MobilePermission {
            id: "all_files".into(),
            title: "所有文件访问".into(),
            description: "让工作区可以指向手机存储中的任意文件夹".into(),
            state_text: "未开启".into(),
            action_text: "去开启".into(),
            granted: false,
            available: true,
        },
    ]))));
    ui.set_permission_summary("已开启 3/7 项".into());
    ui.set_update_phase("available".into());
    ui.set_update_version("0.4.1".into());
    ui.set_update_notes("修复安卓版返回键、权限管理与流式状态刷新。".into());
    ui.set_update_source("GitHub · ylsdamxssjxxdd/wunder".into());
    ui.set_update_channel("github".into());
    ui.set_update_size("352 MB".into());
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

    // Tool manager and agent fixtures for layout review.
    let tool =
        |name: &str, description: &str, category: &str, icon: &str, tone: &str, enabled: bool| {
            crate::MobileTool {
                name: name.into(),
                title: name.into(),
                description: description.into(),
                category: category.into(),
                enabled,
                icon: icon.into(),
                tone: tone.into(),
            }
        };
    let builtin = vec![
        tool(
            "read_file",
            "读取工作区文件内容",
            "内置工具",
            "file-lines",
            "terminal",
            true,
        ),
        tool(
            "run_shell",
            "执行本地命令",
            "内置工具",
            "terminal",
            "terminal",
            true,
        ),
        tool(
            "web_search",
            "搜索网页",
            "内置工具",
            "magnifying-glass",
            "search",
            false,
        ),
    ];
    let mcp_tools = vec![tool(
        "weather_lookup",
        "查询城市天气",
        "MCP 工具",
        "plug",
        "mcp",
        false,
    )];
    let skill_tools = vec![tool(
        "summarize_page",
        "总结网页正文",
        "技能",
        "book",
        "skill",
        true,
    )];
    let knowledge_tools = vec![tool(
        "project_notes",
        "检索项目笔记",
        "知识库",
        "database",
        "knowledge",
        false,
    )];
    ui.set_settings_tools_servers(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileToolResource {
            name: "weather".into(),
            description: "天气 MCP 服务".into(),
            detail: "https://mcp.example.com/weather".into(),
            readonly: false,
        },
    ]))));
    ui.set_settings_tools_skill_resources(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileToolResource {
            name: "summarize-page".into(),
            description: "提取网页正文并生成摘要".into(),
            detail: String::new().into(),
            readonly: false,
        },
        crate::MobileToolResource {
            name: "release-notes".into(),
            description: "内置发布说明模板".into(),
            detail: String::new().into(),
            readonly: true,
        },
    ]))));
    ui.set_settings_tool_active_resource("summarize-page".into());
    ui.set_settings_tool_resource_tab(2);
    ui.set_settings_tool_resource_editable(true);
    ui.set_settings_tool_directory("references".into());
    ui.set_settings_tool_files(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileToolFile {
            name: "guide.md".into(),
            path: "references/guide.md".into(),
            directory: false,
            size: "2.1 KB".into(),
        },
        crate::MobileToolFile {
            name: "examples".into(),
            path: "references/examples".into(),
            directory: true,
            size: String::new().into(),
        },
    ]))));
    ui.set_settings_tool_server_tools(ModelRc::from(Rc::new(VecModel::from(vec![
        tool(
            "weather_current",
            "查询当前天气",
            "MCP 工具",
            "plug",
            "mcp",
            false,
        ),
        tool(
            "weather_forecast",
            "查询未来天气",
            "MCP 工具",
            "plug",
            "mcp",
            false,
        ),
    ]))));
    ui.set_settings_tool_active_server("weather".into());
    ui.set_settings_agent_name("wunder 内置智能体".into());
    ui.set_settings_agent_tool_count(4);
    ui.set_settings_agent_description("你好，我是 wunder，可以帮你拆解任务并调用工具。".into());
    ui.set_settings_agent_prompt("你是 wunder 智能体。先确认目标，再拆解任务并逐步执行。".into());
    let agent_group =
        |category: &str, title: &str, tools: Vec<crate::MobileTool>| crate::MobileToolGroup {
            category: category.into(),
            title: title.into(),
            all_selected: tools.iter().all(|tool| tool.enabled),
            tools: ModelRc::new(VecModel::from(tools)),
        };
    ui.set_settings_agent_groups(ModelRc::from(Rc::new(VecModel::from(vec![
        agent_group("内置工具", "内置工具", builtin),
        agent_group("MCP 工具", "MCP 工具", mcp_tools),
        agent_group("技能", "技能工具", skill_tools),
        agent_group("知识库", "知识库工具", knowledge_tools),
    ]))));
    ui.set_settings_agent_questions(ModelRc::from(Rc::new(VecModel::from(vec![
        slint::SharedString::from("总结当前进度"),
        slint::SharedString::from("检查最近的改动"),
    ]))));
    // 加号面板的预设问题走同一份智能体预设，预览里也给几条便于评审面板。
    ui.set_preset_questions(ModelRc::from(Rc::new(VecModel::from(vec![
        slint::SharedString::from("把这份周报的结论改成三条要点，语气收紧一点。"),
        slint::SharedString::from("总结当前进度"),
        slint::SharedString::from("检查最近的改动"),
    ]))));

    // Cron / memory fixtures for layout review.
    ui.set_settings_cron_workspaces(ModelRc::from(Rc::new(VecModel::from(vec![
        slint::SharedString::from("默认工作区"),
        slint::SharedString::from("文档整理"),
    ]))));
    ui.set_settings_cron_jobs(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileCronJob {
            id: "job-daily".into(),
            name: "每日进度汇总".into(),
            enabled: true,
            schedule: "每天 09:00".into(),
            next_run: "11-08 09:00".into(),
            workspace_name: "默认工作区".into(),
            message: "汇总昨天的改动并发到工作区。".into(),
            last_status: "success".into(),
            last_error: String::new().into(),
        },
        crate::MobileCronJob {
            id: "job-weekly".into(),
            name: "每周清理".into(),
            enabled: false,
            schedule: "每周一 08:30".into(),
            next_run: "—".into(),
            workspace_name: "文档整理".into(),
            message: "清理过期的临时文件。".into(),
            last_status: "failed".into(),
            last_error: "上次运行失败：工作区不可写".into(),
        },
    ]))));
    ui.set_settings_cron_runs(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileCronRun {
            status: "success".into(),
            trigger: "schedule".into(),
            summary: "已生成汇总并写入工作区".into(),
            error: String::new().into(),
            duration: "3.2s".into(),
            created_at: "11-07 09:00".into(),
        },
        crate::MobileCronRun {
            status: "failed".into(),
            trigger: "manual".into(),
            summary: String::new().into(),
            error: "工作区不可写".into(),
            duration: "1.1s".into(),
            created_at: "11-06 18:20".into(),
        },
    ]))));
    ui.set_settings_memories(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileMemory {
            id: "mem-1".into(),
            title: "发布流程偏好".into(),
            content: "构建产物先发内测，再替换正式包；发布说明按形态分节。".into(),
            category: "偏好".into(),
            source: "手动写入".into(),
            time: "2026/11/07 10:12".into(),
        },
        crate::MobileMemory {
            id: "mem-2".into(),
            title: "安卓模拟器验收环境".into(),
            content: "x86_64 AVD cocoon-test；真机验收看 ABI 与文件选择器。".into(),
            category: "项目".into(),
            source: "模型写入".into(),
            time: "2026/11/06 21:40".into(),
        },
    ]))));
    ui.set_settings_memory_categories(ModelRc::from(Rc::new(VecModel::from(vec![
        slint::SharedString::from("全部标签"),
        slint::SharedString::from("偏好"),
        slint::SharedString::from("项目"),
    ]))));
    ui.set_settings_memory_category("全部标签".into());
    ui.set_settings_memory_query("".into());

    // Cloud / channel / device fixtures.
    ui.set_settings_cron_workspaces(ModelRc::from(Rc::new(VecModel::from(vec![
        slint::SharedString::from("默认工作区"),
    ]))));
    ui.set_settings_channel_catalog(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileChannelCatalog {
            channel: "feishu".into(),
            name: "飞书".into(),
            description: "飞书自建应用".into(),
        },
        crate::MobileChannelCatalog {
            channel: "wechat".into(),
            name: "企业微信".into(),
            description: "企业微信应用".into(),
        },
        crate::MobileChannelCatalog {
            channel: "wechat_mp".into(),
            name: "微信公众号".into(),
            description: "公众号回调".into(),
        },
        crate::MobileChannelCatalog {
            channel: "weixin".into(),
            name: "微信机器人".into(),
            description: "微信 ClawBot".into(),
        },
    ]))));
    ui.set_settings_channel_accounts(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileChannelAccount {
            channel: "feishu".into(),
            channel_name: "飞书".into(),
            account_id: "feishu-main".into(),
            name: "飞书主账号".into(),
            status: "active".into(),
            active: true,
            configured: true,
            summary: "已连接飞书自建应用".into(),
            secret_set: true,
        },
        crate::MobileChannelAccount {
            channel: "wechat".into(),
            channel_name: "企业微信".into(),
            account_id: "wecom-ops".into(),
            name: "运营助手".into(),
            status: "paused".into(),
            active: false,
            configured: false,
            summary: "等待填写 Secret".into(),
            secret_set: false,
        },
    ]))));
    ui.set_settings_channel_catalog_ids(ModelRc::from(Rc::new(VecModel::from(vec![
        slint::SharedString::from("feishu"),
        slint::SharedString::from("wechat"),
        slint::SharedString::from("wechat_mp"),
        slint::SharedString::from("weixin"),
    ]))));
    ui.set_settings_channel_catalog_names(ModelRc::from(Rc::new(VecModel::from(vec![
        slint::SharedString::from("飞书"),
        slint::SharedString::from("企业微信"),
        slint::SharedString::from("微信公众号"),
        slint::SharedString::from("微信机器人"),
    ]))));
    ui.set_settings_channel_bindings(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileChannelBinding {
            binding_id: "bind-1".into(),
            channel: "feishu".into(),
            account_id: "feishu-main".into(),
            peer_kind: "group".into(),
            peer_id: "oc_demo".into(),
            agent_id: "__default__".into(),
            enabled: true,
        },
    ]))));
    ui.set_settings_channel_logs(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileChannelLog {
            time: "10:24".into(),
            level: "info".into(),
            event: "message".into(),
            message: "收到一条群消息并创建线程".into(),
        },
    ]))));
    ui.set_settings_devices(ModelRc::from(Rc::new(VecModel::from(vec![
        crate::MobileDevice {
            id: "local".into(),
            kind: "phone".into(),
            label: "Pixel 8".into(),
            status: "online".into(),
            connected: true,
            current: true,
            selectable: true,
        },
        crate::MobileDevice {
            id: "cloud".into(),
            kind: "cloud".into(),
            label: "云端".into(),
            status: "online".into(),
            connected: true,
            current: false,
            selectable: true,
        },
        crate::MobileDevice {
            id: "device:desktop".into(),
            kind: "desktop".into(),
            label: "工作室桌面".into(),
            status: "online".into(),
            connected: true,
            current: false,
            selectable: false,
        },
    ]))));
    ui.set_settings_current_device("Pixel 8".into());

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
            tool_name: "编辑文件".into(),
            tool_icon: "file-pen".into(),
            target: "crates/wunder-desktop/frontend/ui/timeline.slint".into(),
            summary: "已修改 1 个文件".into(),
            detail: String::new(),
            patch: vec![wunder_desktop::NativePatchFile {
                path: "crates/wunder-desktop/frontend/ui/timeline.slint".into(),
                action: "modify".into(),
                added: "2".into(),
                deleted: "1".into(),
                lines: vec![
                    wunder_desktop::NativePatchLine {
                        text: "@@ -1398,7 +1398,8 @@ component ProcessBar inherits Rectangle".into(),
                        kind: "header".into(),
                        number: String::new(),
                    },
                    wunder_desktop::NativePatchLine {
                        text: "                        private property <int> scroll-pending: true;".into(),
                        kind: "context".into(),
                        number: "1398".into(),
                    },
                    wunder_desktop::NativePatchLine {
                        text: "                        height: min(240px, thinking-column.preferred-height + 16px);".into(),
                        kind: "delete".into(),
                        number: "1399".into(),
                    },
                    wunder_desktop::NativePatchLine {
                        text: "                        height: min(320px, thinking-column.preferred-height + 16px); // keep the whole wrapped reasoning line reachable".into(),
                        kind: "add".into(),
                        number: "1399".into(),
                    },
                ],
            }],
            tokens: "2.4k token".into(),
            duration: "0.8s".into(),
            status_kind: crate::project::STATUS_DONE,
            open: true,
            ..Default::default()
        },
        crate::project::ProcessChildData {
            id: "p-cmd".into(),
            kind: KIND_TOOL,
            tool_name: "执行命令".into(),
            tool_icon: "terminal".into(),
            summary: "cargo test --lib — 全部通过".into(),
            detail: "执行 · 完成\n$ cargo test --lib -- --nocapture --test-threads=1\nname\tvalue\tstatus\tduration\towner\tpath\tnotes\tmore\tcolumns\t--verbose\n   Compiling wunder-android v0.4.0 (/home/sjxx/桌面/wunder/crates/wunder-android)\n    Finished `test` profile [unoptimized + debuginfo] target(s) in 12.3s with a deliberately long trailing sentence that must wrap inside the panel\ntest md::tests::thumbnails_keep_pixels_and_reject_garbage ... ok\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\ntest result: ok. 70 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\nexit 0".into(),
            status_kind: crate::project::STATUS_DONE,
            open: false,
            ..Default::default()
        },
    ];
    process.text = "工作过程 · 3 步".into();
    rows.push(process);

    let mut body = RowData::new(KIND_BODY, "p-body");
    let text = "聊天区现在复用了桌面版的同一套组件：\n\n- `TimelineView` 负责消息流\n- `Composer` 负责输入与模型选择\n\n| 文件 | 行数 | 修改类型 | 说明 | 状态 | 最后提交 |\n| --- | --- | --- | --- | --- | --- |\n| crates/wunder-desktop/frontend/ui/timeline.slint | 1284 | 修改 | 时间线组件与折叠交互调整 | 已提交 | 2026-11-08 18:00:00 |\n| crates/wunder-desktop/frontend/src/timeline.rs | 3315 | 修改 | 实时投影与视图缓存同步 | 待验证 | 2026-11-08 17:42:11 |\n| crates/wunder-android/src/project.rs | 1306 | 新增 | 手机端行数据投影 | 已提交 | 2026-11-08 16:20:33 |\n\n代码块样式也保持一致：\n\n```rust\nlet rows = project::project_turn(&turn);\n```\n\n> 手机上只保留单窗口与抽屉。

交付物见 [季度评估报告](documents/quarterly-report.pdf) 与 [核对清单](notes/checklist.md)。";
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
        ("gauge-high".into(), "18.4 t/s".into()),
        ("bolt".into(), "1.8k".into()),
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

    ui.set_turn_marks(ModelRc::from(Rc::new(VecModel::from(vec![
        TurnMark {
            fraction: 0.02,
            user: "把聊天区搬到手机上，样式和桌面版保持一致。".into(),
            agent: "先确认聊天组件的复用边界，再处理单窗口约束。".into(),
        },
        TurnMark {
            fraction: 0.28,
            user: "补充一下加号面板和会话操作。".into(),
            agent: "加号改为底部功能面板，会话行增加操作入口。".into(),
        },
        TurnMark {
            fraction: 0.55,
            user: "检查移动端输入区和键盘避让。".into(),
            agent: "输入区与会话列表已按手机尺寸收紧。".into(),
        },
        TurnMark {
            fraction: 0.78,
            user: "把运行状态同步到伙伴气泡。".into(),
            agent: "伙伴动画已跟随线程运行状态切换。".into(),
        },
        TurnMark {
            fraction: 0.96,
            user: "最后检查一遍回归。".into(),
            agent: "cargo test 与桌面预览均通过。".into(),
        },
    ]))));
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
    card.max_context = "262144".into();
    card.tool_call_mode = "function_call".into();
    card.support_vision = true;
    card.stream_include_usage = true;
    card
}

/// Opens one of the settings pages in the desktop preview so layout review can
/// reach the sheet without an engine: `settings`, `models` or `editor`.
pub fn open_preview_page(ui: &MobileWindow, page: &str) {
    match page {
        "workspaces" => {
            ui.set_workspaces_open(true);
        }
        "workspaces-editor" => {
            ui.set_workspaces_open(true);
            ui.set_workspace_overlay(2);
        }
        "workspaces-menu" => {
            ui.set_workspaces_open(true);
            ui.set_workspace_overlay(1);
        }
        "prompt-preview" => {
            ui.set_rows(ModelRc::default());
            // 与运行时内置智能体的默认问候语（DEFAULT_AGENT_GREETING）一致，
            // 预览看到的就是真机首屏那句话。
            ui.set_greeting(
                "我是 wunder 的驾驶员，很高兴帮助你，试着把整理资料，分析数据，写文章等工作交给我吧~"
                    .into(),
            );
            ui.set_loading(false);
        }
        "drawer" => {
            ui.set_drawer_open(true);
        }
        // 加号面板：`plus` 是三个入口，`plus-presets` 展开第二页的预设问题列表。
        "plus" => {
            ui.set_plus_sheet_open(true);
        }
        "plus-presets" => {
            ui.set_plus_sheet_page(1);
            ui.set_plus_sheet_open(true);
        }
        // 输入卡就地编辑：带一段多行草稿，方便看卡片随行数长高的样子。
        "compose" => {
            ui.set_draft(
                "把这份周报的结论改成三条要点，语气收紧一点。\n第二条保留数据口径，别改数字。"
                    .into(),
            );
        }
        // 空草稿的输入卡：右侧那颗键此时是麦克风，用来评审核按住说话的布局。
        "compose-empty" => {}
        "workspace-sheet" => {
            ui.set_drawer_open(true);
            ui.set_workspace_sheet_open(true);
        }
        "file-actions" => {
            ui.set_drawer_open(true);
            if let Some(entry) = preview_workspace().into_iter().find(|entry| !entry.dir) {
                ui.set_file_pending(entry);
                ui.set_file_sheet_open(true);
            }
        }
        "file-rename" => {
            ui.set_drawer_open(true);
            if let Some(entry) = preview_workspace().into_iter().find(|entry| !entry.dir) {
                ui.set_file_pending(entry);
                ui.set_file_rename_path(ui.get_file_pending().path);
                ui.set_file_rename_name(ui.get_file_pending().name);
                ui.set_file_rename_open(true);
            }
        }
        "file-move" => {
            ui.set_drawer_open(true);
            if let Some(entry) = preview_workspace().into_iter().find(|entry| !entry.dir) {
                ui.set_file_pending(entry);
                ui.set_file_move_open(true);
            }
        }
        "file-delete" => {
            ui.set_drawer_open(true);
            if let Some(entry) = preview_workspace().into_iter().find(|entry| !entry.dir) {
                ui.set_file_pending(entry);
                ui.set_file_delete_open(true);
            }
        }
        "workspace" => {
            ui.set_workspace_open(true);
        }
        "settings" => {
            ui.set_settings_page(0);
            ui.set_settings_open(true);
        }
        // 通用偏好页（主题、字号、语言）：核对没有「发送键」后的表单高度。
        "general" => {
            ui.set_settings_page(3);
            ui.set_settings_open(true);
        }
        // 智能体设置页：问候语换成一段真实长度的文案，核对单行输入框在文本超出
        // 宽度时的横向滚动与触摸端光标。
        "agent" => {
            ui.set_settings_agent_description(
                "你好，我是 wunder，可以帮你拆解任务并调用工具。把整理资料、分析数据、写文章这类工作交给我，我会先确认目标，再一步步做完并给出结果。"
                    .into(),
            );
            ui.set_settings_page(8);
            ui.set_settings_open(true);
        }

        "toast" => {
            ui.set_toast("已复制到剪贴板".into());
        }
        "goal-plan" | "goal-plan-expanded" => {
            ui.set_goal_visible(true);
            ui.set_goal_active(true);
            ui.set_goal_phase("active".into());
            ui.set_goal_activation("armed".into());
            ui.set_goal_objective("整理工作区结构并补齐文档".into());
            ui.set_goal_rounds("2/8".into());
            ui.set_goal_elapsed("1分12秒".into());
            ui.set_activity_steps_total(5);
            ui.set_activity_steps_done(2);
            ui.set_activity_running(true);
            ui.set_activity_plan_explanation("先梳理目录，再补文档".into());
            ui.set_activity_plan_current("核对模块清单".into());
            ui.set_activity_plan_position(3);
            let steps = vec![
                crate::PlanStep {
                    step: "扫描工作区".into(),
                    status: "completed".into(),
                },
                crate::PlanStep {
                    step: "合并重复模块".into(),
                    status: "completed".into(),
                },
                crate::PlanStep {
                    step: "核对模块清单".into(),
                    status: "in_progress".into(),
                },
                crate::PlanStep {
                    step: "补齐接口文档".into(),
                    status: "pending".into(),
                },
                crate::PlanStep {
                    step: "回归测试".into(),
                    status: "pending".into(),
                },
            ];
            ui.set_activity_plan_steps(ModelRc::from(Rc::new(VecModel::from(steps))));
            ui.set_activity_plan_expanded(page == "goal-plan-expanded");
        }
        "model-gauge" => {
            ui.set_context_usage(0.62);
            ui.set_context_known(true);
            ui.set_context_counts("32.1k / 51.8k tokens".into());
        }
        "model-sheet" => {
            ui.set_context_usage(0.62);
            ui.set_context_known(true);
            ui.set_context_counts("32.1k / 51.8k tokens".into());
            ui.set_model_sheet_open(true);
        }
        // `model-activity` 在模型浮层里叠上后台任务与子智能体两段：计数故意大于
        // 行数，核对「还有 N 条未显示」的收口行。
        "model-activity" => {
            ui.set_context_usage(0.62);
            ui.set_context_known(true);
            ui.set_context_counts("32.1k / 51.8k tokens".into());
            ui.set_task_rows(ModelRc::from(Rc::new(VecModel::from(vec![
                CommandTask {
                    id: "cmd-1".into(),
                    command: "cargo test --workspace".into(),
                    full_command: "".into(),
                    cwd: "".into(),
                    elapsed: "2m04s".into(),
                    expanded: false,
                    output: "".into(),
                    detail_state: 0,
                },
                CommandTask {
                    id: "cmd-2".into(),
                    command: "python scripts/build_help_manual.py".into(),
                    full_command: "".into(),
                    cwd: "".into(),
                    elapsed: "18s".into(),
                    expanded: false,
                    output: "".into(),
                    detail_state: 0,
                },
            ]))));
            ui.set_task_count(3);
            ui.set_subagent_rows(ModelRc::from(Rc::new(VecModel::from(vec![
                SubagentItem {
                    id: "sub-1".into(),
                    title: "整理资料".into(),
                    summary: "已读取 12 份文档，正在合并要点".into(),
                    status_kind: 1,
                    stoppable: true,
                    duration: "1m 05s".into(),
                    credits: "7".into(),
                    context: "25%".into(),
                },
                SubagentItem {
                    id: "sub-2".into(),
                    title: "核对数据".into(),
                    summary: "核对完成，未发现冲突".into(),
                    status_kind: 0,
                    stoppable: false,
                    duration: "42s".into(),
                    credits: "3".into(),
                    context: "12%".into(),
                },
            ]))));
            ui.set_subagent_count(4);
            ui.set_model_sheet_open(true);
        }
        "data" => {
            ui.set_settings_page(15);
            ui.set_settings_open(true);
            ui.set_settings_backup_status(
                "已备份到 /storage/emulated/0/Download/wunder-backup-20261108.zip".into(),
            );
            ui.set_settings_backup_path(
                "/storage/emulated/0/Download/wunder-backup-20261108.zip".into(),
            );
            ui.set_settings_backup_items(ModelRc::from(Rc::new(VecModel::from(vec![
                MobileBackup {
                    path: "/storage/emulated/0/Download/wunder-backup-20261121.zip".into(),
                    name: "今天 09:41".into(),
                    detail: "1.2 MB · wunder-backup-20261121.zip".into(),
                },
                MobileBackup {
                    path: "/storage/emulated/0/Download/wunder-backup-20261120.zip".into(),
                    name: "昨天 18:07".into(),
                    detail: "1.1 MB · wunder-backup-20261120.zip".into(),
                },
                MobileBackup {
                    path: "/storage/emulated/0/Download/wunder-backup-20261116.zip".into(),
                    name: "2026-11-16 21:52".into(),
                    detail: "1.0 MB · wunder-backup-20261116.zip".into(),
                },
            ]))));
            ui.set_settings_restore_path(
                "/storage/emulated/0/Download/wunder-backup-20261121.zip".into(),
            );
            ui.set_settings_crash_log_title("crash-rust-20261108-143012.log".into());
            ui.set_settings_crash_log_text(
                "panic 时间: 2026-11-08 14:30:12.418 +08:00\n\
                 进程: wunder 安卓版 v0.4.0 (android aarch64)\n\
                 线程: wunder-android-engine\n\
                 信息: 示例崩溃堆栈仅用于预览布局\n\
                 位置: crates/wunder-android/src/engine.rs:1:1\n"
                    .into(),
            );
        }
        "about" => {
            ui.set_settings_page(6);
            ui.set_settings_open(true);
        }
        "about-cloud" => {
            ui.set_settings_page(6);
            ui.set_settings_open(true);
            ui.set_update_channel("cloud".into());
            ui.set_update_source("云端更新源".into());
        }
        // 使用手册：`help` 只展开形态分区，`help-topic` 直接翻开第一篇
        // （含五个形态的表格，用来核对宽表在手机上的横向平移）。
        "help" => {
            ui.set_settings_page(16);
            ui.set_settings_open(true);
            ui.invoke_settings_help_toggle(0);
        }
        "help-topic" => {
            ui.set_settings_page(16);
            ui.set_settings_open(true);
            ui.invoke_settings_help_toggle(0);
            ui.invoke_settings_help_open(0);
        }
        // `help-art` 翻开带截图的主题，核对配图像素与图注排版。
        "help-art" => {
            ui.set_settings_page(16);
            ui.set_settings_open(true);
            crate::help::open_topic_id(ui, "forms-desktop-ui");
        }
        // `help-table` 翻开一篇以宽表开头的主题，用来核对触屏上的横向平移。
        "help-table" => {
            ui.set_settings_page(16);
            ui.set_settings_open(true);
            ui.invoke_settings_help_toggle(1);
            ui.invoke_settings_help_open(29);
        }
        "env-setup" => {
            ui.set_settings_ubuntu_state("installing".into());
            ui.set_settings_ubuntu_progress(42);
            ui.set_settings_ubuntu_stage("正在解压 Ubuntu 根文件系统…".into());
            ui.set_env_setup_progress(42);
            ui.set_env_setup_phase(0.28);
            ui.set_env_setup_open(true);
        }
        "env-setup-error" => {
            ui.set_settings_ubuntu_state("error".into());
            ui.set_settings_ubuntu_error("安装失败：设备剩余空间不足，请清理后重试。".into());
            ui.set_env_setup_open(true);
        }
        "env-setup-ready" => {
            ui.set_settings_ubuntu_state("installed".into());
            ui.set_settings_ubuntu_version("Ubuntu 24.04.1 LTS".into());
            ui.set_env_setup_progress(100);
            ui.set_env_setup_open(true);
        }
        "models" => {
            ui.set_settings_page(1);
            ui.set_settings_open(true);
        }
        "tools" => {
            ui.set_settings_page(7);
            ui.set_settings_open(true);
        }
        "tools-file" => {
            ui.set_settings_page(7);
            ui.set_settings_open(true);
            ui.set_settings_tool_file_path("references/guide.md".into());
            ui.set_settings_tool_file_content(
                "# 写摘要的约定\n\n1. 先提取正文\n2. 再压缩成三段\n3. 保留关键数据\n".into(),
            );
            ui.set_settings_tool_file_readonly(false);
            ui.set_settings_tool_file_open(true);
        }
        "tools-editor" => {
            ui.set_settings_page(7);
            ui.set_settings_open(true);
            ui.set_settings_tool_editor_kind(1);
            ui.set_settings_tool_editor_new(false);
            ui.set_settings_mcp_draft(crate::MobileMcpDraft {
                previous: "weather".into(),
                name: "weather".into(),
                display_name: "天气服务".into(),
                endpoint: "https://mcp.example.com/weather".into(),
                transport: "streamable-http".into(),
                description: "查询城市天气".into(),
                headers: "{\"Authorization\":\"Bearer ***\"}".into(),
            });
        }
        "tools-editor-kb" => {
            ui.set_settings_page(7);
            ui.set_settings_open(true);
            ui.set_settings_tool_editor_kind(2);
            ui.set_settings_tool_editor_new(false);
            ui.set_settings_kb_draft(crate::MobileKnowledgeDraft {
                previous: "project-notes".into(),
                name: "project-notes".into(),
                description: "项目背景与决策记录".into(),
                base_type: "vector".into(),
                embedding_model: "bge-m3".into(),
                chunk_size: "500".into(),
                chunk_overlap: "50".into(),
                dataset_id: "".into(),
            });
        }
        "agent" => {
            ui.set_settings_page(8);
            ui.set_settings_open(true);
        }
        // 本地工具审批对话框：权限审批选「建议 / 自动编辑」后弹出的那张卡。
        "chat-approval" => {
            ui.set_chat_approval_open(true);
            ui.set_chat_approval_id("preview-approval".into());
            ui.set_chat_approval_tool("execute_command".into());
            ui.set_chat_approval_kind("exec".into());
            ui.set_chat_approval_summary("execute_command: git status --short".into());
        }
        "cron" => {
            ui.set_settings_page(9);
            ui.set_settings_open(true);
        }
        "cron-editor" => {
            ui.set_settings_page(9);
            ui.set_settings_open(true);
            ui.set_settings_cron_editor_open(true);
            ui.set_settings_cron_editor_new(false);
            ui.set_settings_cron_draft(crate::MobileCronDraft {
                id: "job-daily".into(),
                name: "每日进度汇总".into(),
                schedule_kind: "every".into(),
                schedule_at: "2026-11-08 09:00".into(),
                every: "1".into(),
                interval_unit: "day".into(),
                cron_expr: String::new().into(),
                timezone: String::new().into(),
                message: "汇总昨天的改动并发到工作区。".into(),
                workspace_name: "默认工作区".into(),
                delete_after: false,
                enabled: true,
            });
        }
        "cron-runs" => {
            ui.set_settings_page(9);
            ui.set_settings_open(true);
            ui.set_settings_cron_runs_open(true);
            ui.set_settings_cron_runs_title("每日进度汇总".into());
        }
        "memory" => {
            ui.set_settings_page(10);
            ui.set_settings_open(true);
        }
        "account" => {
            ui.set_settings_page(13);
            ui.set_settings_open(true);
            ui.set_settings_usage_duration("3h 42m".into());
            ui.set_settings_usage_tokens("128.5K".into());
            ui.set_settings_usage_quota("1.2K".into());
        }
        "account-in" => {
            ui.set_settings_page(13);
            ui.set_settings_open(true);
            ui.set_settings_usage_duration("3h 42m".into());
            ui.set_settings_usage_tokens("128.5K".into());
            ui.set_settings_usage_quota("1.2K".into());
            ui.set_settings_cloud_logged_in(true);
            ui.set_settings_cloud_connection("online".into());
            ui.set_settings_cloud_server("https://cloud.example.com".into());
            ui.set_settings_cloud_username("wunder-demo".into());
            ui.set_settings_cloud_balance(8600);
            ui.set_settings_cloud_daily_grant(10000);
            ui.set_settings_cloud_max_concurrent(4);
            ui.set_settings_cloud_active_concurrent(1);
            ui.set_settings_cloud_sync_enabled(true);
            ui.set_settings_cloud_inbound_enabled(true);
            ui.set_settings_cloud_device_name("Pixel 8".into());
            ui.set_settings_cloud_last_success("刚刚".into());
        }
        "companion" => {
            ui.set_settings_page(11);
            ui.set_settings_open(true);
        }
        "channels" => {
            ui.set_settings_page(14);
            ui.set_settings_open(true);
        }
        "channel-editor" => {
            ui.set_settings_page(14);
            ui.set_settings_open(true);
            ui.set_settings_channel_editor_open(true);
            ui.set_settings_channel_editor_new(false);
            ui.set_settings_channel_draft(crate::MobileChannelDraft {
                account_id: "feishu-main".into(),
                channel: "feishu".into(),
                name: "飞书主账号".into(),
                enabled: true,
                ..crate::MobileChannelDraft::default()
            });
        }
        // 微信机器人编辑页：核对「扫码登录」入口与手动字段共存的样子。
        "channel-editor-weixin" => {
            ui.set_settings_page(14);
            ui.set_settings_open(true);
            ui.set_settings_channel_editor_open(true);
            ui.set_settings_channel_editor_new(true);
            ui.set_settings_channel_draft(crate::MobileChannelDraft {
                channel: "weixin".into(),
                name: "微信机器人".into(),
                enabled: true,
                ..crate::MobileChannelDraft::default()
            });
        }
        // 扫码面板：预览里没有网络，二维码用本地生成的示意位图，只核对版式。
        "channel-qr" | "channel-qr-expired" | "channel-qr-failed" => {
            ui.set_settings_page(14);
            ui.set_settings_open(true);
            ui.set_settings_channel_editor_open(true);
            ui.set_settings_channel_editor_new(true);
            ui.set_settings_channel_draft(crate::MobileChannelDraft {
                channel: "weixin".into(),
                name: "微信机器人".into(),
                enabled: true,
                ..crate::MobileChannelDraft::default()
            });
            ui.set_settings_channel_qr_open(true);
            ui.set_settings_channel_qr_status(match page {
                "channel-qr-expired" => 3,
                "channel-qr-failed" => 5,
                _ => 1,
            });
            ui.set_settings_channel_qr_message(if page == "channel-qr-failed" {
                "无法获取二维码：网络不可用".into()
            } else {
                "".into()
            });
            ui.set_settings_channel_qr_image(preview_qr_image());
        }
        "devices" => {
            ui.set_drawer_open(true);
            ui.set_device_sheet_open(true);
        }
        "memory-editor" => {
            ui.set_settings_page(10);
            ui.set_settings_open(true);
            ui.set_settings_memory_editor_open(true);
            ui.set_settings_memory_editor_new(false);
            ui.set_settings_memory_draft(crate::MobileMemoryDraft {
                id: "mem-1".into(),
                title: "发布流程偏好".into(),
                content: "构建产物先发内测，再替换正式包；发布说明按形态分节。".into(),
                category: "偏好".into(),
            });
        }
        "trajectory" | "trajectory-tools" | "trajectory-detail" => {
            let filter = if page == "trajectory-tools" { 2 } else { 0 };
            ui.set_trajectory_title("移动端聊天区对齐".into());
            ui.set_trajectory_session_title("预览线程".into());
            ui.set_trajectory_error("".into());
            ui.set_trajectory_loading(false);
            ui.set_trajectory_filter(filter);
            ui.set_trajectory_expanded(if page == "trajectory-detail" { 2 } else { -1 });
            ui.set_trajectory_items(ModelRc::from(Rc::new(VecModel::from(filtered_trajectory(
                &preview_trajectory(),
                filter,
            )))));
            let (audit, segments, bars) = preview_trajectory_audit();
            ui.set_trajectory_audit(audit);
            ui.set_trajectory_segments(ModelRc::from(Rc::new(VecModel::from(segments))));
            ui.set_trajectory_bars(ModelRc::from(Rc::new(VecModel::from(bars))));
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
            ui.set_editor_probe("探测成功：上下文 262144".into());
            ui.set_editor_open(true);
            // The preview window is not running yet, so the sheet's
            // `changed editor-open` handler has not been installed; select
            // the editor page explicitly.
            ui.set_settings_page(2);
        }
        "editor-nondefault" => {
            let card = preview_model_card(
                "preview-speech",
                "tts",
                "预览语音模型",
                "https://api.example.com/v1",
                false,
            );
            ui.set_settings_page(1);
            ui.set_settings_open(true);
            ui.set_editor_key(card.key.clone());
            ui.set_editor_new(false);
            ui.set_editor_default(false);
            ui.set_editor_draft(card);
            ui.set_editor_secret("".into());
            ui.set_editor_probe("".into());
            ui.set_editor_open(true);
            ui.set_settings_page(2);
        }
        // 图像模型的编辑表单：提供方只有 vllm_omni，基础区还会多出默认/最大
        // 尺寸两行，用来核对类型切换后的字段与行距。
        "editor-image" => {
            let mut card = preview_model_card(
                "preview-image",
                "image",
                "预览图像模型",
                "http://127.0.0.1:8000/v1",
                false,
            );
            card.provider = "vllm_omni".into();
            card.image_default_size = "1024x1024".into();
            card.image_max_size = "1440x1440".into();
            ui.set_settings_page(1);
            ui.set_settings_open(true);
            ui.set_editor_key(card.key.clone());
            ui.set_editor_new(false);
            ui.set_editor_default(false);
            ui.set_editor_draft(card);
            ui.set_editor_secret("".into());
            ui.set_editor_probe("".into());
            ui.set_editor_open(true);
            ui.set_settings_page(2);
        }
        "editor-new" => {
            let card = preview_model_card("", "llm", "", "", false);
            ui.set_settings_page(1);
            ui.set_settings_open(true);
            ui.set_editor_key("".into());
            ui.set_editor_new(true);
            ui.set_editor_default(false);
            ui.set_editor_draft(card);
            ui.set_editor_secret("".into());
            ui.set_editor_probe("".into());
            ui.set_editor_open(true);
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

    fn streaming_state(answer: &str) -> Shared {
        let mut state = Shared::default();
        state.session_id = "s".into();
        state.live_session = "s".into();
        state.generating = true;
        state.live_root = "turn-1".into();
        let mut live = crate::project::LiveTurn::new("hi".into());
        live.root_id = "turn-1".into();
        live.answer = answer.into();
        state.live_rows = live.rows();
        state.durable_rows = vec![
            RowData::new(project::KIND_USER, "turn-1-user"),
            RowData::new(project::KIND_FOOTER, "turn-1-footer"),
        ];
        state.durable_rows[0].text = "hi".into();
        state.durable_rows[1].stats = vec![("hashtag".into(), "1".into())];
        state
    }

    #[test]
    fn the_live_turn_owns_its_status_while_the_stream_is_attached() {
        let state = streaming_state("partial");
        assert!(live_is_authoritative(&state));
        let visible = visible_live_rows(&state);
        assert!(visible.iter().any(|row| {
            row.kind == project::KIND_FOOTER
                && row.stats.iter().any(|(key, _)| key == "status-requesting")
        }));
    }

    #[test]
    fn pending_identity_still_hides_its_durable_twin() {
        let mut state = streaming_state("partial");
        state.live_root = "live".into();
        state.live_rows = crate::project::LiveTurn::new("hi".into()).rows();
        state.durable_rows[1].status_kind = project::STATUS_RUNNING;
        assert_eq!(live_owner(&state).as_deref(), Some("turn-1"));
    }

    #[test]
    fn an_earlier_round_body_does_not_retire_the_streaming_answer() {
        let mut state = streaming_state("current streaming answer");
        state.generating = false;
        state
            .durable_rows
            .push(RowData::new(project::KIND_BODY, "turn-1-body-final"));
        state.durable_rows[2].text = "earlier round".into();
        assert!(!durable_covers_live(&state));
        let visible = visible_live_rows(&state);
        assert!(visible.iter().any(|row| {
            row.kind == project::KIND_BODY && row.text == "current streaming answer"
        }));
    }

    #[test]
    fn a_settled_durable_body_retires_the_live_tail() {
        let mut state = streaming_state("answer");
        state.generating = false;
        state
            .durable_rows
            .push(RowData::new(project::KIND_BODY, "turn-1-body-final"));
        state.durable_rows[2].text = "answer".into();
        assert!(durable_covers_live(&state));
        assert!(visible_live_rows(&state).is_empty());
    }

    #[test]
    fn cached_rows_pick_up_decoded_images_without_reprojection() {
        let mut cache = RowCache::default();
        let mut data = RowData::new(project::KIND_BODY, "row-1");
        let text = "![图](chart.png)";
        data.text = text.into();
        data.blocks_src = text.into();
        let row = cache.row(&data);
        let image = row.blocks.row_data(0).expect("image block");
        assert_eq!(image.kind, crate::md::block::IMAGE);
        assert_eq!(image.image.size().width, 0);
        assert_eq!(cache.take_wanted(), vec!["chart.png".to_string()]);

        cache.images.insert(
            "chart.png".into(),
            Some(CachedImage {
                frames: vec![slint::Image::from_rgba8(
                    slint::SharedPixelBuffer::clone_from_slice(&[255, 0, 0, 255], 1, 1),
                )],
                delays: Vec::new(),
                bytes: 4,
            }),
        );
        cache.hydrate_all();
        // The cached row shares its block model with the published row, so the
        // picture appears without rebuilding the transcript.
        let row = cache.row(&data);
        let image = row.blocks.row_data(0).expect("image block");
        assert_eq!(image.image.size().width, 1);
        assert_eq!(image.image_width, 1.0);
        assert_eq!(image.image_height, 1.0);
        assert!(cache.take_wanted().is_empty());
    }

    #[test]
    fn cached_rows_expose_gif_frames_and_delays() {
        let mut cache = RowCache::default();
        let mut data = RowData::new(project::KIND_BODY, "row-gif");
        data.blocks_src = "![动图](anim.gif)".into();
        let pixel = |color: [u8; 4]| {
            slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(&color, 1, 1))
        };
        cache.images.insert(
            "anim.gif".into(),
            Some(CachedImage {
                frames: vec![pixel([255, 0, 0, 255]), pixel([0, 255, 0, 255])],
                delays: vec![120],
                bytes: 8,
            }),
        );
        let row = cache.row(&data);
        let block = row.blocks.row_data(0).expect("image block");
        assert_eq!(block.image_frames.row_count(), 2);
        assert_eq!(block.frame_delays.row_data(0), Some(120));
        assert_eq!(block.image.size().width, 1);
    }

    #[test]
    fn env_setup_opens_only_before_the_environment_is_usable() {
        assert!(env_setup_should_open("missing", false));
        assert!(env_setup_should_open("installing", false));
        assert!(!env_setup_should_open("installed", false));
        assert!(!env_setup_should_open("unsupported", false));
        assert!(!env_setup_should_open("error", false));
        // 失败后被用户跳过：本进程内不再自动弹出。
        assert!(!env_setup_should_open("installing", true));
    }

    #[test]
    fn env_setup_progress_never_regresses_within_a_run() {
        // Java 在「验证 proot」后从 94% 退回 85% 开始装补充包。
        assert_eq!(next_env_progress(94, "installing", 85), 94);
        assert_eq!(next_env_progress(94, "installing", 97), 97);
        assert_eq!(next_env_progress(0, "missing", 0), 0);
        // 新一次安装（错误后重试）从低进度重新开始。
        assert_eq!(next_env_progress(94, "error", 0), 0);
        assert_eq!(next_env_progress(0, "installing", 150), 100);
    }

    #[test]
    fn goal_elapsed_drops_leading_zero_units() {
        assert_eq!(goal_elapsed(0), "0秒");
        assert_eq!(goal_elapsed(65), "1分5秒");
        assert_eq!(goal_elapsed(3725), "1小时2分5秒");
    }

    #[test]
    fn plan_current_prefers_running_then_first_unfinished() {
        let step = |status: &str| crate::engine::PlanStepView {
            step: format!("步骤-{status}"),
            status: status.into(),
        };
        let running = vec![step("completed"), step("in_progress"), step("pending")];
        assert_eq!(plan_current(&running), ("步骤-in_progress".into(), 2));
        let pending = vec![step("completed"), step("pending")];
        assert_eq!(plan_current(&pending), ("步骤-pending".into(), 2));
        let done = vec![step("completed"), step("completed")];
        assert_eq!(plan_current(&done), (String::new(), 0));
        assert_eq!(plan_current(&[]), (String::new(), 0));
    }
}
