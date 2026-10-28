use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub(crate) struct MainLayout {
    pub(crate) transcript: Rect,
    pub(crate) popup: Option<Rect>,
    pub(crate) activity: Rect,
    pub(crate) input: Rect,
}

pub(crate) fn build_layout(
    area: Rect,
    popup_len: usize,
    activity_visible: bool,
    input_rows: u16,
) -> MainLayout {
    let popup_height = if popup_len == 0 {
        0
    } else {
        (popup_len.min(7) as u16) + 1
    };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(popup_height),
            Constraint::Length(u16::from(activity_visible)),
            Constraint::Length(input_rows.min(area.height.saturating_sub(1))),
        ])
        .split(area);
    MainLayout {
        transcript: sections[0],
        popup: (popup_len > 0).then_some(sections[1]),
        activity: sections[2],
        input: sections[3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_layout_has_no_logo_or_activity_gap() {
        let layout = build_layout(Rect::new(0, 0, 80, 24), 0, false, 4);
        assert_eq!(layout.input.height, 4);
        assert_eq!(layout.activity.height, 0);
        assert_eq!(layout.transcript.bottom(), layout.input.y);
        assert!(layout.popup.is_none());
    }

    #[test]
    fn a_popup_does_not_consume_the_draft_on_short_screens() {
        let layout = build_layout(Rect::new(0, 0, 40, 12), 20, true, 4);
        assert!(layout.input.height >= 3);
        assert!(layout.popup.is_some());
    }
}
