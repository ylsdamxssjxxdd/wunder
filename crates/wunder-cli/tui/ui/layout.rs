use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Columns the main column keeps clear on both sides. The transcript bullets, the wrapped
/// continuation rows and the draft's prompt all start one gutter in from the terminal edge,
/// so output never touches the frame and the composer lines up with the cards above it.
pub(crate) const CONTENT_GUTTER_COLS: u16 = 2;

/// Blank rows between the transcript and the composer band. Output and the draft are two
/// different surfaces; without this row the last card reads as part of the input.
pub(crate) const COMPOSER_TOP_GAP_ROWS: u16 = 1;

pub(crate) struct MainLayout {
    pub(crate) transcript: Rect,
    pub(crate) popup: Option<Rect>,
    pub(crate) activity: Rect,
    pub(crate) input: Rect,
}

/// Keep `area`'s rows but pull its sides in by the content gutter. A frame too narrow to
/// hold both gutters keeps a single column of content and a single column of margin.
pub(crate) fn padded_columns(area: Rect) -> Rect {
    let gutter = CONTENT_GUTTER_COLS.min(area.width.saturating_sub(1) / 2);
    Rect {
        x: area.x.saturating_add(gutter),
        width: area.width.saturating_sub(gutter.saturating_mul(2)),
        ..area
    }
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
    // The gap is a comfort row, not a promise: on a screen that cannot spare it the
    // transcript keeps its rows and the band closes up instead.
    let gap_rows = if area.height > input_rows.saturating_add(popup_height).saturating_add(4) {
        COMPOSER_TOP_GAP_ROWS
    } else {
        0
    };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(gap_rows),
            Constraint::Length(popup_height),
            Constraint::Length(u16::from(activity_visible)),
            Constraint::Length(input_rows.min(area.height.saturating_sub(1))),
        ])
        .split(area);
    MainLayout {
        transcript: padded_columns(sections[0]),
        popup: (popup_len > 0).then(|| padded_columns(sections[2])),
        activity: padded_columns(sections[3]),
        // The band itself stays full-bleed: it is the composer's own surface and reads as
        // one bar across the bottom. Its text is inset inside the band.
        input: sections[4],
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
        assert!(layout.popup.is_none());
        assert_eq!(
            layout.transcript.bottom() + COMPOSER_TOP_GAP_ROWS,
            layout.input.y
        );
    }

    #[test]
    fn the_transcript_column_keeps_a_gutter_on_both_sides() {
        let layout = build_layout(Rect::new(0, 0, 80, 24), 0, false, 4);
        assert_eq!(layout.transcript.x, CONTENT_GUTTER_COLS);
        assert_eq!(layout.transcript.right() + CONTENT_GUTTER_COLS, 80);
        // The band is full-bleed, the row inside it is not.
        assert_eq!(layout.input.x, 0);
        assert_eq!(layout.input.width, 80);
    }

    #[test]
    fn a_popup_does_not_consume_the_draft_on_short_screens() {
        let layout = build_layout(Rect::new(0, 0, 40, 12), 20, true, 4);
        assert!(layout.input.height >= 3);
        assert!(layout.popup.is_some());
    }

    #[test]
    fn a_short_frame_gives_up_the_gap_before_the_transcript() {
        let layout = build_layout(Rect::new(0, 0, 40, 8), 0, true, 4);
        assert_eq!(layout.transcript.bottom(), layout.activity.y);
        assert!(layout.transcript.height >= 1);
    }

    #[test]
    fn a_narrow_frame_keeps_one_column_of_content() {
        let layout = build_layout(Rect::new(0, 0, 3, 10), 0, false, 3);
        assert!(layout.transcript.width >= 1);
        assert!(layout.transcript.right() <= 3);
    }
}
