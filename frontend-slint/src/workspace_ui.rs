//! Workspace dialog controller (§6): create/edit plus the delete confirmation.
//!
//! Dialog state lives in the WorkspaceDialogState global; the native folder
//! picker, path validation and workspace CRUD run on background threads and
//! land through the event loop, so the UI thread never blocks on IO.

use crate::{MainWindow, WorkspaceDialogState};
use slint::{ComponentHandle, Model};
use std::sync::Arc;
use wunder_desktop::{NativeDesktop, WorkspaceEdit, WORKSPACE_COLORS, WORKSPACE_ICONS};

/// The dialog enforces the same 1–40 glyph bound before submit (§6.1).
const MAX_NAME_CHARS: usize = 40;

fn clamp_name(raw: &str) -> String {
    let name = raw.trim();
    if name.chars().count() > MAX_NAME_CHARS {
        name.chars().take(MAX_NAME_CHARS).collect()
    } else {
        name.to_string()
    }
}

fn choice(allowed: &[&str], index: i32, fallback: &str) -> String {
    allowed
        .get(usize::try_from(index).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or(fallback)
        .to_string()
}

/// Human-readable validation outcome; empty means the folder is usable.
fn validation_error(report: &wunder_desktop::WorkspacePathReport) -> String {
    if !report.exists {
        return "文件夹不存在或不可访问".into();
    }
    if !report.is_directory {
        return "所选路径不是文件夹".into();
    }
    if !report.writable {
        return "文件夹不可写".into();
    }
    if let Some(name) = &report.occupied_by {
        return format!("该文件夹已被工作区「{name}」占用");
    }
    if let Some(name) = &report.nested_in {
        return format!("该文件夹位于工作区「{name}」内部");
    }
    if let Some(name) = &report.contains {
        return format!("该文件夹包含工作区「{name}」的文件夹");
    }
    String::new()
}

/// Kick off an async path validation; only the report for the still-selected
/// folder is applied, so stale results cannot overwrite newer input.
fn spawn_validation(api: Arc<NativeDesktop>, weak: slint::Weak<MainWindow>, path: String) {
    std::thread::spawn(move || {
        let outcome = match api.validate_workspace_path(&path) {
            Ok(report) => validation_error(&report),
            Err(error) => format!("无法校验该文件夹：{error}"),
        };
        let _ = slint::invoke_from_event_loop(move || {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<WorkspaceDialogState>();
            state.set_validating(false);
            if state.get_folder_path().as_str() == path {
                state.set_validate_error(outcome.into());
            }
        });
    });
}

/// Open the dialog in create mode with factory defaults.
pub(crate) fn open_create(app: &MainWindow) {
    let state = app.global::<WorkspaceDialogState>();
    state.set_edit_mode(false);
    state.set_workspace_id("".into());
    state.set_folder_path("".into());
    state.set_name_draft("".into());
    state.set_icon_index(0);
    state.set_color_index(2);
    state.set_validating(false);
    state.set_validate_error("".into());
    state.set_open(true);
}

/// Open the dialog pre-filled from the workspace row the menu ran on (§6.4).
pub(crate) fn open_edit(app: &MainWindow, workspace_index: usize) {
    let Some(card) = app.get_workspaces().row_data(workspace_index) else {
        return;
    };
    let state = app.global::<WorkspaceDialogState>();
    state.set_edit_mode(true);
    state.set_workspace_id(card.workspace_id.to_string().into());
    state.set_folder_path(card.root_path.to_string().into());
    state.set_name_draft(card.name.to_string().into());
    let icon = card.icon.to_string();
    state.set_icon_index(
        WORKSPACE_ICONS
            .iter()
            .position(|key| *key == icon)
            .unwrap_or(0) as i32,
    );
    let color = card.color.to_string();
    state.set_color_index(
        WORKSPACE_COLORS
            .iter()
            .position(|key| *key == color)
            .unwrap_or(2) as i32,
    );
    state.set_validating(false);
    state.set_validate_error("".into());
    state.set_open(true);
}

/// Open the delete confirmation for the row the menu ran on (§6.4).
pub(crate) fn open_delete(app: &MainWindow, workspace_index: usize) {
    let Some(card) = app.get_workspaces().row_data(workspace_index) else {
        return;
    };
    let state = app.global::<WorkspaceDialogState>();
    state.set_workspace_id(card.workspace_id.to_string().into());
    state.set_delete_name(card.name.to_string().into());
    state.set_delete_mode(0);
    state.set_delete_open(true);
}

/// Bind dialog callbacks; call once after the window is created.
pub fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    let state = app.global::<WorkspaceDialogState>();
    let weak = app.as_weak();
    let api = desktop.clone();
    state.on_pick_folder(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<WorkspaceDialogState>();
        if state.get_busy() {
            return;
        }
        let weak = weak.clone();
        let api = api.clone();
        // The native chooser blocks its thread; keep it off the UI thread.
        std::thread::spawn(move || {
            let picked = crate::file_dialog::pick_directory("选择工作区文件夹");
            let _ = slint::invoke_from_event_loop(move || {
                let Some(app) = weak.upgrade() else { return };
                let Some(picked) = picked else { return };
                let state = app.global::<WorkspaceDialogState>();
                state.set_folder_path(picked.clone().into());
                state.set_validate_error("".into());
                // New workspaces default the name to the folder's own (§6.1).
                if !state.get_edit_mode() {
                    let base = std::path::Path::new(&picked)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    state.set_name_draft(clamp_name(&base).into());
                }
                state.set_validating(true);
                spawn_validation(api, app.as_weak(), picked);
            });
        });
    });
    let weak = app.as_weak();
    state.on_clear_folder(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<WorkspaceDialogState>();
        state.set_folder_path("".into());
        state.set_validate_error("".into());
        state.set_validating(false);
    });
    let weak = app.as_weak();
    let api = desktop.clone();
    state.on_accept(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<WorkspaceDialogState>();
        if state.get_busy() || state.get_validating() {
            return;
        }
        let name = clamp_name(&state.get_name_draft());
        if name.is_empty() {
            state.set_validate_error("请填写工作区名称".into());
            return;
        }
        let folder = state.get_folder_path().to_string();
        if folder.is_empty() {
            return;
        }
        let edit_mode = state.get_edit_mode();
        let workspace_id = state.get_workspace_id().to_string();
        let icon = choice(WORKSPACE_ICONS, state.get_icon_index(), "folder");
        let color = choice(WORKSPACE_COLORS, state.get_color_index(), "blue");
        state.set_busy(true);
        let weak = weak.clone();
        let api = api.clone();
        std::thread::spawn(move || {
            let result = if edit_mode {
                api.update_workspace(&WorkspaceEdit {
                    workspace_id,
                    name: name.clone(),
                    root_path: folder,
                    icon,
                    color,
                })
                .map(|workspace| workspace.workspace_id)
            } else {
                api.create_workspace(&name, &folder, &icon, &color)
                    .map(|workspace| workspace.workspace_id)
            };
            let api_refresh = api.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(app) = weak.upgrade() else { return };
                let state = app.global::<WorkspaceDialogState>();
                state.set_busy(false);
                match result {
                    Ok(id) => {
                        state.set_open(false);
                        crate::navigation_ui::project_sidebar(&app, Some(&api_refresh));
                        crate::navigation_ui::focus_workspace_of(&app, Some(&id));
                    }
                    Err(error) => state.set_validate_error(format!("{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    state.on_canceled(move || {
        if let Some(app) = weak.upgrade() {
            app.global::<WorkspaceDialogState>().set_open(false);
        }
    });
    let weak = app.as_weak();
    let api = desktop.clone();
    state.on_reveal_folder(move || {
        let Some(app) = weak.upgrade() else { return };
        let folder = app.global::<WorkspaceDialogState>().get_folder_path().to_string();
        if folder.is_empty() {
            return;
        }
        let weak = weak.clone();
        let api = api.clone();
        // Off-thread like every other facade call; errors land on status.
        std::thread::spawn(move || {
            let result = api.open_workspace_resource(&folder, "");
            if let (Err(error), Some(app)) = (result, weak.upgrade()) {
                app.set_status(format!("无法打开文件夹：{error}").into());
            }
        });
    });
    let weak = app.as_weak();
    state.on_request_delete(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<WorkspaceDialogState>();
        state.set_delete_name(state.get_name_draft());
        state.set_delete_mode(0);
        state.set_delete_open(true);
    });
    let weak = app.as_weak();
    let api = desktop.clone();
    state.on_delete_confirmed(move || {
        let Some(app) = weak.upgrade() else { return };
        let state = app.global::<WorkspaceDialogState>();
        let workspace_id = state.get_workspace_id().to_string();
        if workspace_id.is_empty() {
            return;
        }
        let delete_threads = state.get_delete_mode() == 1;
        state.set_delete_open(false);
        let api = api.clone();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let result = api.delete_workspace(&workspace_id, delete_threads);
            let _ = weak.upgrade_in_event_loop(move |app| {
                match result {
                    Ok(_) => {
                        crate::navigation_ui::project_sidebar(&app, Some(&api));
                        app.invoke_refresh_chat();
                    }
                    Err(error) => app.set_status(format!("无法删除工作区：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    state.on_delete_canceled(move || {
        if let Some(app) = weak.upgrade() {
            app.global::<WorkspaceDialogState>().set_delete_open(false);
        }
    });
    let weak = app.as_weak();
    app.on_new_workspace(move || {
        let Some(app) = weak.upgrade() else { return };
        open_create(&app);
    });
}
