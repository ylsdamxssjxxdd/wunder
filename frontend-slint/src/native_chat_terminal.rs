//! Native terminal window bindings. The session worker owns screen parsing;
//! the UI applies only changed runs from its latest bounded projection.
use super::*;
use crate::{
    terminal_grid::Ink,
    terminal_session::{Control, Session},
};
use wunder_desktop::NativeTerminalSpec;

fn selected_workspace(app: &MainWindow) -> Result<(String, String, String), String> {
    let index =
        usize::try_from(app.get_selected_workspace()).map_err(|_| "没有选中的本地工作区")?;
    let card = app
        .get_workspaces()
        .row_data(index)
        .ok_or("没有选中的本地工作区")?;
    if card.workspace_id.is_empty() || card.root_path.is_empty() {
        return Err("当前工作区没有本地目录".into());
    }
    let root = std::path::Path::new(card.root_path.as_str())
        .canonicalize()
        .map_err(|e| format!("无法解析工作区目录：{e}"))?;
    if !root.is_dir() {
        return Err("当前工作区目录不存在".into());
    }
    Ok((
        card.workspace_id.to_string(),
        card.name.to_string(),
        root.to_string_lossy().into_owned(),
    ))
}

fn ensure(host: &mut State, app: &MainWindow) -> Result<(), String> {
    let (id, name, root) = selected_workspace(app)?;
    if host
        .terminals
        .get(&id)
        .is_some_and(|w| w.workspace_root == root && w.session.is_some())
    {
        host.terminal_workspace = Some(id);
        return Ok(());
    }
    if host.terminals.len() >= 8 && !host.terminals.contains_key(&id) {
        return Err("工作区终端已达上限，请先退出一个终端".into());
    }
    if let Some(old) = host.terminals.remove(&id).and_then(|w| w.session) {
        drop(old);
    }
    let window = host.terminal_window.as_ref().ok_or("终端窗口不可用")?;
    let session_id = format!("terminal:{id}");
    let spec = NativeTerminalSpec {
        terminal_id: String::new(),
        session_id: session_id.clone(),
        workspace_id: id.clone(),
        command: String::new(),
        cwd: root.clone(),
        shell: String::new(),
    };
    // Read the old run before starting the new one, so replay never includes
    // bytes that the live subscription will also deliver.
    let raw = host.desktop.terminal_transcript(&session_id);
    let snapshot = host
        .desktop
        .terminal_start(&spec)
        .map_err(|e| e.to_string())?;
    let mut from = raw.len().saturating_sub(128 * 1024);
    while from < raw.len() && !raw.is_char_boundary(from) {
        from += 1;
    }
    if from > 0 {
        from += raw[from..].find('\n').map_or(0, |n| n + 1);
    }
    let io = Session::start(
        host.desktop.clone(),
        session_id.clone(),
        snapshot.terminal_id.clone(),
        window.as_weak(),
        raw[from..].to_string(),
    )?;
    host.terminals.insert(
        id.clone(),
        WorkspaceTerminal {
            session: Some(TerminalSession { io }),
            workspace_name: name,
            workspace_root: root,
            projection: None,
        },
    );
    host.terminal_workspace = Some(id);
    Ok(())
}

fn active(host: &State) -> Option<&WorkspaceTerminal> {
    host.terminals.get(host.terminal_workspace.as_ref()?)
}
fn control(host: &State, event: Control) {
    if let Some(s) = active(host).and_then(|w| w.session.as_ref()) {
        let _ = s.io.control(event);
    }
}

fn paint(host: &mut State) {
    let Some(w) = host.terminal_window.as_ref() else {
        return;
    };
    let Some(workspace) = active(host) else {
        return;
    };
    w.set_workspace_name(workspace.workspace_name.as_str().into());
    let root = workspace
        .workspace_root
        .strip_prefix(r"\\?\UNC\")
        .map(|p| format!(r"\\{p}"))
        .unwrap_or_else(|| {
            workspace
                .workspace_root
                .strip_prefix(r"\\?\")
                .unwrap_or(&workspace.workspace_root)
                .into()
        });
    w.set_workspace_root(root.into());
    let Some(p) = workspace.projection.as_ref() else {
        w.set_input_ready(false);
        w.set_status("启动中…".into());
        return;
    };
    // Keep the model stable: changing one echoed character updates its run,
    // not every item and layout in the terminal viewport.
    let count = p.spans.len();
    for (index, s) in p.spans.iter().enumerate() {
        let row = TermSpan {
            text: s.text.as_str().into(),
            row: s.row,
            col: s.col,
            cells: s.cells,
            bold: s.bold,
            underline: s.underline,
            fg: match s.fg {
                Ink::Default => slint::Color::from_rgb_u8(0xe8, 0xee, 0xf5),
                Ink::Rgb(c) => slint::Color::from_rgb_u8(c[0], c[1], c[2]),
            },
            bg: s
                .bg
                .map(|c| slint::Color::from_rgb_u8(c[0], c[1], c[2]))
                .unwrap_or_default(),
        };
        if index >= host.terminal_model.row_count() {
            host.terminal_model.push(row);
        } else if host.terminal_model.row_data(index).as_ref() != Some(&row) {
            host.terminal_model.set_row_data(index, row);
        }
    }
    while host.terminal_model.row_count() > count {
        host.terminal_model
            .remove(host.terminal_model.row_count() - 1);
    }
    w.set_rows(p.rows.into());
    w.set_cols(p.cols.into());
    w.set_view_offset(p.offset as i32);
    w.set_history_depth(p.history as i32);
    w.set_mouse_reporting(p.mouse);
    w.set_application_cursor(p.application_cursor);
    w.set_input_ready(p.ready);
    w.set_status(p.status.as_str().into());
    w.set_error(p.error);
    w.set_caret_visible(p.ready && p.caret.is_some());
    if let Some((row, col)) = p.caret {
        w.set_caret_row(row);
        w.set_caret_col(col);
    }
}

fn geometry(host: &State) {
    if let Some(w) = host.terminal_window.as_ref() {
        let rows = w.get_measured_rows().clamp(3, 200) as u16;
        let cols = w.get_measured_cols().clamp(20, 500) as u16;
        control(host, Control::Resize(cols, rows));
    }
}

pub(super) fn bind_terminal(app: &MainWindow, state: Rc<RefCell<State>>) {
    let Some(window) = state.borrow().terminal_window.clone() else {
        return;
    };
    window.set_spans(ModelRc::from(state.borrow().terminal_model.clone()));
    let host = Rc::downgrade(&state);
    let weak = app.as_weak();
    app.on_terminal_toggled(move |open| {
        let (Some(app), Some(host)) = (weak.upgrade(), host.upgrade()) else {
            return;
        };
        if !open {
            if let Some(w) = host.borrow().terminal_window.as_ref() {
                let _ = w.hide();
            }
            return;
        }
        let mut host = host.borrow_mut();
        if let Err(error) = ensure(&mut host, &app) {
            app.set_status(error.into());
            return;
        }
        paint(&mut host);
        if let Some(w) = host.terminal_window.as_ref() {
            let _ = w.show();
            w.invoke_focus_input();
        }
        geometry(&host);
    });
    let host = Rc::downgrade(&state);
    window.on_frame_ready(move || {
        let Some(host) = host.upgrade() else { return };
        let mut host = host.borrow_mut();
        for workspace in host.terminals.values_mut() {
            if let Some(p) = workspace.session.as_ref().and_then(|s| s.io.take()) {
                let ended = p.ended;
                workspace.projection = Some(p);
                if ended {
                    workspace.session = None;
                }
            }
        }
        if host
            .terminal_window
            .as_ref()
            .is_some_and(|w| w.window().is_visible())
        {
            paint(&mut host);
        }
    });
    let host = Rc::downgrade(&state);
    let weak = app.as_weak();
    window.on_key_input(move |text| {
        let (Some(app), Some(host)) = (weak.upgrade(), host.upgrade()) else {
            return;
        };
        let host = host.borrow();
        if let Some(s) = active(&host).and_then(|w| w.session.as_ref()) {
            if let Err(error) = s.io.send(text.as_bytes().to_vec()) {
                app.set_status(error.into());
            }
        }
    });
    let host = Rc::downgrade(&state);
    let weak = app.as_weak();
    window.on_pasted(move |text| {
        let (Some(app), Some(host)) = (weak.upgrade(), host.upgrade()) else {
            return;
        };
        let host = host.borrow();
        let Some(w) = active(&host) else { return };
        let Some(s) = w.session.as_ref() else { return };
        // Newlines are Return in terminal input. The shell owns editing and
        // multiline bracketed paste, never a second local command composer.
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let bytes = if w.projection.as_ref().is_some_and(|p| p.bracketed_paste) {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.replace('\n', "\r").into_bytes()
        };
        if let Err(error) = s.io.send(bytes) {
            app.set_status(error.into());
        }
    });
    let weak = window.as_weak();
    window.on_paste_requested(move || {
        if let Some(w) = weak.upgrade() {
            w.set_paste_request(w.get_paste_request().wrapping_add(1));
        }
    });
    let host = Rc::downgrade(&state);
    window.on_geometry_changed(move || {
        if let Some(h) = host.upgrade() {
            geometry(&h.borrow());
        }
    });
    let host = Rc::downgrade(&state);
    window.on_mouse_event(move |a, b, c, r| {
        if let Some(h) = host.upgrade() {
            control(&h.borrow(), Control::Mouse(a, b, c, r));
        }
    });
    let host = Rc::downgrade(&state);
    window.on_scrolled(move |n| {
        if let Some(h) = host.upgrade() {
            control(&h.borrow(), Control::Scroll(n));
        }
    });
    let host = Rc::downgrade(&state);
    window.on_scroll_to(move |n| {
        if let Some(h) = host.upgrade() {
            control(&h.borrow(), Control::ScrollTo(n.max(0) as usize));
        }
    });
    let host = Rc::downgrade(&state);
    window.on_clear(move || {
        if let Some(h) = host.upgrade() {
            control(&h.borrow(), Control::Clear);
        }
    });
    let host = Rc::downgrade(&state);
    window.on_interrupt(move || {
        if let Some(h) = host.upgrade() {
            if let Some(s) = active(&h.borrow()).and_then(|w| w.session.as_ref()) {
                let _ = s.io.interrupt();
            }
        }
    });
    let host = Rc::downgrade(&state);
    let weak = app.as_weak();
    let weak_window = window.as_weak();
    window.on_exit_requested(move || {
        let (Some(app), Some(h)) = (weak.upgrade(), host.upgrade()) else {
            return;
        };
        let mut h = h.borrow_mut();
        if let Some(id) = h.terminal_workspace.take() {
            h.terminals.remove(&id);
        }
        if let Some(w) = weak_window.upgrade() {
            let _ = w.hide();
        }
        app.set_terminal_mode(false);
    });
    let weak = app.as_weak();
    let weak_window = window.as_weak();
    window.window().on_close_requested(move || {
        if let Some(app) = weak.upgrade() {
            app.set_terminal_mode(false);
        }
        if let Some(w) = weak_window.upgrade() {
            let _ = w.hide();
        }
        slint::CloseRequestResponse::HideWindow
    });
}
