use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;

pub(crate) struct MainLayout {
    pub(crate) transcript: Rect,
    pub(crate) popup: Option<Rect>,
    pub(crate) banner: Option<Rect>,
    pub(crate) activity: Rect,
    pub(crate) input: Rect,
}

/// Split the frame. `banner_rows` is the band the welcome gear asks for, which sits
/// between the transcript and the working line so history never shifts under it.
///
/// The band is clamped here: a caller that asks for more than the screen can spare must
/// not end up with a composer that has lost its rows.
pub(crate) fn build_layout(
    area: Rect,
    popup_len: usize,
    _activity_visible: bool,
    banner_rows: u16,
) -> MainLayout {
    let activity_height = 1;
    let popup_height = if popup_len == 0 {
        0
    } else {
        (popup_len as u16).min(7).saturating_add(1)
    };
    // Everything the band may not eat: the popup, the working line, the composer and the
    // transcript floor.
    let spare = area
        .height
        .saturating_sub(popup_height + activity_height + 5 + 8);
    let banner_rows = banner_rows.min(spare);

    if popup_len == 0 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),
                Constraint::Length(banner_rows),
                Constraint::Length(activity_height),
                Constraint::Length(5),
            ])
            .split(area);
        return MainLayout {
            transcript: chunks[0],
            popup: None,
            banner: rect_or_none(chunks[1]),
            activity: chunks[2],
            input: chunks[3],
        };
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),
            Constraint::Length(popup_height),
            Constraint::Length(banner_rows),
            Constraint::Length(activity_height),
            Constraint::Length(5),
        ])
        .split(area);
    MainLayout {
        transcript: chunks[0],
        popup: Some(chunks[1]),
        banner: rect_or_none(chunks[2]),
        activity: chunks[3],
        input: chunks[4],
    }
}

fn rect_or_none(rect: Rect) -> Option<Rect> {
    (!rect.is_empty()).then_some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_layout_without_popup_uses_expected_sections() {
        let layout = build_layout(Rect::new(0, 0, 100, 30), 0, true, 0);
        assert!(layout.popup.is_none());
        assert_eq!(layout.activity.height, 1);
        assert_eq!(layout.input.height, 5);
    }

    #[test]
    fn build_layout_with_popup_clamps_popup_height() {
        let layout = build_layout(Rect::new(0, 0, 100, 30), 20, true, 0);
        assert_eq!(layout.popup.expect("popup").height, 8);
        assert_eq!(layout.input.height, 5);
    }

    #[test]
    fn build_layout_keeps_gap_when_activity_hidden() {
        let layout = build_layout(Rect::new(0, 0, 100, 30), 0, false, 0);
        assert_eq!(layout.activity.height, 1);
        assert_eq!(layout.input.height, 5);
    }

    #[test]
    fn banner_band_sits_between_transcript_and_activity() {
        let with = build_layout(Rect::new(0, 0, 100, 30), 0, false, 8);
        let without = build_layout(Rect::new(0, 0, 100, 30), 0, false, 0);
        let band = with.banner.expect("banner band");
        assert_eq!(band.height, 8);
        assert_eq!(band.y, with.transcript.bottom());
        assert_eq!(with.activity.y, band.bottom());
        assert_eq!(without.banner, None);
        assert_eq!(without.activity.y, without.transcript.bottom());
    }

    #[test]
    fn banner_band_survives_a_popup_and_short_screens() {
        let layout = build_layout(Rect::new(0, 0, 100, 30), 4, true, 6);
        assert!(layout.popup.is_some());
        assert_eq!(layout.banner.expect("band").height, 6);
        // A screen too short for the band clamps the band, not the composer.
        let tight = build_layout(Rect::new(0, 0, 60, 16), 0, false, 9);
        assert_eq!(tight.input.height, 5);
        assert_eq!(tight.activity.height, 1);
        assert_eq!(tight.banner.expect("clamped band").height, 2);
        assert_eq!(tight.transcript.height, 8);
        // A screen with no spare at all loses the band entirely.
        assert!(build_layout(Rect::new(0, 0, 60, 14), 0, false, 9)
            .banner
            .is_none());
    }
}
