mod composer;
mod layout;
mod modals;
mod popup;
mod transcript;

use super::app::TuiApp;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &mut TuiApp) {
    let is_zh = app.is_zh_language();
    let popup_view = app.popup_view();
    let activity_visible = app.activity_highlighted();
    let layout = layout::build_layout(frame.area(), popup_view.lines.len(), activity_visible);

    let transcript_viewport = layout::inner_rect(layout.transcript);
    transcript::draw(frame, layout.transcript, transcript_viewport, app, is_zh);

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
    let inner = layout::inner_rect(layout.input);
    composer::draw_input(frame, layout.input, inner, app, is_zh);

    if let Some((rows, selected)) = app.resume_picker_rows() {
        modals::draw_resume_modal(frame, frame.area(), rows, selected, is_zh);
    }

    if app.shortcuts_visible() {
        modals::draw_shortcuts_modal(frame, frame.area(), app.shortcuts_lines(), is_zh);
    }

    if let Some(lines) = app.approval_modal_lines() {
        modals::draw_approval_modal(frame, frame.area(), layout.input, lines, is_zh);
    } else if let Some(lines) = app.inquiry_modal_lines() {
        modals::draw_inquiry_modal(frame, frame.area(), layout.input, lines, is_zh);
    }
}
