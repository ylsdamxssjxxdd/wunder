use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;

pub(crate) struct MainLayout {
    pub(crate) status: Rect,
    pub(crate) transcript: Rect,
    pub(crate) popup: Option<Rect>,
    pub(crate) activity: Rect,
    pub(crate) input: Rect,
}

pub(crate) fn build_layout(area: Rect, popup_len: usize, activity_visible: bool) -> MainLayout {
    let activity_height = if activity_visible { 1 } else { 0 };
    if popup_len == 0 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(8),
                Constraint::Length(activity_height),
                Constraint::Length(6),
            ])
            .split(area);
        return MainLayout {
            status: chunks[0],
            transcript: chunks[1],
            popup: None,
            activity: chunks[2],
            input: chunks[3],
        };
    }

    let popup_height = (popup_len as u16).min(7).saturating_add(2);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Length(popup_height),
            Constraint::Length(activity_height),
            Constraint::Length(6),
        ])
        .split(area);
    MainLayout {
        status: chunks[0],
        transcript: chunks[1],
        popup: Some(chunks[2]),
        activity: chunks[3],
        input: chunks[4],
    }
}

pub(crate) fn inner_rect(rect: Rect) -> Rect {
    Rect {
        x: rect.x.saturating_add(1),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_layout_without_popup_uses_expected_sections() {
        let layout = build_layout(Rect::new(0, 0, 100, 30), 0, true);
        assert_eq!(layout.status.height, 1);
        assert!(layout.popup.is_none());
        assert_eq!(layout.activity.height, 1);
        assert_eq!(layout.input.height, 6);
    }

    #[test]
    fn build_layout_with_popup_clamps_popup_height() {
        let layout = build_layout(Rect::new(0, 0, 100, 30), 20, true);
        assert_eq!(layout.popup.expect("popup").height, 9);
        assert_eq!(layout.input.height, 6);
    }

    #[test]
    fn build_layout_hides_activity_row_when_not_needed() {
        let layout = build_layout(Rect::new(0, 0, 100, 30), 0, false);
        assert_eq!(layout.activity.height, 0);
        assert_eq!(layout.input.height, 6);
    }

    #[test]
    fn inner_rect_shrinks_borders() {
        let inner = inner_rect(Rect::new(4, 5, 20, 8));
        assert_eq!(inner, Rect::new(5, 6, 18, 6));
    }
}
