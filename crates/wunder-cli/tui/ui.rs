pub(crate) mod centered_picker;
mod command_center;
mod composer;
mod layout;
mod modals;
mod popup;
mod transcript;

use super::app::TuiApp;
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &mut TuiApp) {
    let is_zh = app.is_zh_language();
    if app.trajectory_view_open() {
        trajectory::draw(frame, frame.area(), app, is_zh);
        draw_blocking_choice(frame, app);
        return;
    }
    if let Some(view) = app.command_center_view() {
        command_center::draw(frame, frame.area(), view, is_zh);
        return;
    }
    if app.transcript_view_open() {
        transcript::draw_view(frame, frame.area(), app, is_zh);
        return;
    }
    let popup_view = app.popup_view();
    let activity_visible = app.activity_visible();
    let banner_rows = app.banner_reserved_rows(frame.area());
    let layout = layout::build_layout(
        frame.area(),
        popup_view.lines.len(),
        activity_visible,
        banner_rows,
    );

    transcript::draw(frame, layout.transcript, layout.transcript, app, is_zh);

    if let Some(band) = layout.banner {
        if let Some(pose) = app.paint_banner(frame.area()) {
            let offset = band.width.saturating_sub(pose.width) / 2;
            frame.render_widget(
                Paragraph::new(Text::from(pose.lines)),
                Rect::new(band.x + offset, band.y, pose.width, band.height),
            );
        }
    }

    if let Some(popup_area) = layout.popup {
        popup::draw(
            frame,
            popup_area,
            app.popup_title(),
            popup_view.lines.as_slice(),
            popup_view.selected_index,
        );
    }

    if activity_visible {
        composer::draw_activity(frame, layout.activity, app);
    }

    app.set_mouse_regions(layout.transcript, layout.input);
    composer::draw_input(frame, layout.input, app, is_zh);

    draw_blocking_choice(frame, app);

    if app.shortcuts_visible() {
        modals::draw_shortcuts_modal(frame, frame.area(), app.shortcuts_lines(), is_zh);
    }

    if let Some(lines) = app.warning_panel_lines(is_zh) {
        modals::draw_shortcuts_modal(frame, frame.area(), lines, is_zh);
    }
}

/// 审批、提问与线程恢复是打断了当前回合的三种选择，都以 codex 的
/// 居中选择面板浮在整个视口中央；同一时刻至多一个在等待回答。
fn draw_blocking_choice(frame: &mut Frame, app: &mut TuiApp) {
    if let Some(spec) = app.resume_picker_spec() {
        centered_picker::draw(frame, frame.area(), &spec);
    } else if let Some(spec) = app.approval_modal_spec() {
        centered_picker::draw(frame, frame.area(), &spec);
    } else if let Some(spec) = app.remote_approval_modal_spec() {
        centered_picker::draw(frame, frame.area(), &spec);
    } else if let Some(spec) = app.inquiry_modal_spec() {
        centered_picker::draw(frame, frame.area(), &spec);
    }
}

mod trajectory {
    pub(super) use crate::tui::app::trajectory::draw;
}
