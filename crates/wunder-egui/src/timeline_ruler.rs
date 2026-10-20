//! Right-edge turn ruler: tick geometry, hover summary and click target.
//!
//! Geometry only; the timeline paints it.
//!
//! # Coordinate space
//!
//! Everything here lives in **ruler space**: the origin is the top edge of the
//! 18px lane at the right of the reading column, `y` grows downwards and
//! `0.0..=ruler_height` is the strip. The painter adds the lane's screen origin
//! before drawing, and subtracts it from `pointer_y` before calling
//! [`hovered`]. `ruler_top` is still an argument of [`ticks`] because the lane
//! can be clipped by the viewport; a strip that is not on screen cannot be
//! hovered or clicked, so it projects nothing.
//!
//! `scroll_target` returns a **content** offset (the same space as the scroll
//! container's `offset.y`), while [`visible_window`] returns fractions of the
//! strip height because its signature does not know `ruler_height`.
//!
//! The module deliberately avoids egui types: the same numbers drive native
//! and web painters, and plain floats are unit-testable without a frame.

/// Width reserved for the lane; the reading column keeps its own 1000px cap and
/// the ruler never takes part in the scrolling content layout.
pub const WIDTH: f32 = 18.0;

/// Smallest painted tick, so a 200-turn thread still shows a continuous line
/// instead of vanishing into sub-pixel marks.
pub const MIN_TICK_HEIGHT: f32 = 3.0;

/// Suggested hover/drag tolerance; the pointer may sit in the gap between two
/// ticks and still bind to the closest one.
pub const HIT_TOLERANCE: f32 = 6.0;

/// Character budget for the hover summary's first line, counted in `char`s so
/// a Chinese preview is not cut three times shorter than a Latin one.
pub const SUMMARY_MAX_CHARS: usize = 32;

/// One turn's slice of the strip.
///
/// `y` is the turn's top edge in ruler space and `height` covers the whole
/// turn band, so consecutive ticks tile the strip without gaps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RulerTick {
    /// Zero-based index into the thread's turns; hover and click report it back.
    pub turn_index: usize,
    /// Top edge of the turn band, in ruler space.
    pub y: f32,
    /// Height of the turn band (at least [`MIN_TICK_HEIGHT`]).
    pub height: f32,
}

/// A single turn reads as one message without a ruler, so the lane only shows
/// from the second turn on.
pub fn visible(turn_count: usize) -> bool {
    turn_count > 1
}

/// Project every turn onto the strip.
///
/// The model exposes no per-turn heights, so a turn's band is its equal share
/// of `content_height`; the band's top edge maps proportionally onto
/// `ruler_height`. That keeps the mapping a pure ratio of the whole thread:
/// scrolling never moves a tick, which is what makes a fixed lane usable as an
/// overview of the conversation.
///
/// Returns nothing when the strip is degenerate (`content_height` or
/// `ruler_height` is zero/negative) or when the lane lies completely outside
/// the viewport, because neither case can produce a hover or a click.
pub fn ticks(
    turn_count: usize,
    content_height: f32,
    viewport_top: f32,
    viewport_height: f32,
    ruler_top: f32,
    ruler_height: f32,
) -> Vec<RulerTick> {
    // Finiteness is checked first: `NaN` fails every ordered comparison, so a
    // single `is_finite` gate keeps the `<= 0.0` tests below unambiguous.
    if turn_count == 0
        || !content_height.is_finite()
        || !ruler_height.is_finite()
        || !ruler_top.is_finite()
        || !viewport_top.is_finite()
        || !viewport_height.is_finite()
        || content_height <= 0.0
        || ruler_height <= 0.0
        || viewport_height <= 0.0
    {
        return Vec::new();
    }

    // A lane scrolled out of (or clipped by) the viewport is not interactive.
    if ruler_top + ruler_height <= viewport_top || ruler_top >= viewport_top + viewport_height {
        return Vec::new();
    }

    let band = ruler_height / turn_count as f32;
    let height = band.max(MIN_TICK_HEIGHT);
    (0..turn_count)
        .map(|turn_index| RulerTick {
            turn_index,
            y: band * turn_index as f32,
            height,
        })
        .collect()
}

/// Which tick the pointer is over.
///
/// `pointer_y` is in ruler space. A pointer inside a tick binds to it; a
/// pointer in the gap between ticks binds to the closest one as long as it is
/// within `tolerance`. Returns the tick's `turn_index`, never a position, so
/// callers do not have to reason about overlap when a dense thread forces
/// [`MIN_TICK_HEIGHT`] bands to overlap.
pub fn hovered(tick: &[RulerTick], pointer_y: f32, tolerance: f32) -> Option<usize> {
    if tick.is_empty() || !pointer_y.is_finite() {
        return None;
    }
    let tolerance = if tolerance.is_finite() {
        tolerance.max(0.0)
    } else {
        0.0
    };

    let mut best: Option<((f32, f32), usize)> = None;
    for item in tick {
        if !item.y.is_finite() || !item.height.is_finite() {
            continue;
        }
        let height = item.height.max(0.0);
        let bottom = item.y + height;
        let gap = if pointer_y < item.y {
            item.y - pointer_y
        } else if pointer_y > bottom {
            pointer_y - bottom
        } else {
            0.0
        };
        if gap > tolerance {
            continue;
        }
        // Lexicographic rank: being inside a tick always beats sitting in a
        // gap, and inside an overlap the nearest centre wins, so a dense or a
        // tall tick cannot steal a pointer from the mark directly under it.
        let center = item.y + height * 0.5;
        let rank = (gap, (pointer_y - center).abs());
        let better = match best {
            None => true,
            Some((best_rank, best_index)) => {
                rank < best_rank || (rank == best_rank && item.turn_index < best_index)
            }
        };
        if better {
            best = Some((rank, item.turn_index));
        }
    }

    best.map(|(_, turn_index)| turn_index)
}

/// Hover text: `第 3 轮 · 帮我整理这份资料`.
///
/// `turn_index` is zero-based like the rest of this module, but the user sees
/// 1-based ordinals (matching the footer's "第 1 轮"). Truncation counts
/// `char`s, not bytes: a 33-character Chinese line must lose one character, not
/// a third of itself.
pub fn summary_text(turn_index: usize, user_first_line: &str) -> String {
    let number = turn_index.saturating_add(1);
    let line = user_first_line.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return format!("第 {number} 轮");
    }

    let mut excerpt: String = line.chars().take(SUMMARY_MAX_CHARS).collect();
    if line.chars().count() > SUMMARY_MAX_CHARS {
        excerpt.push('…');
    }
    format!("第 {number} 轮 · {excerpt}")
}

/// Content offset to scroll to after clicking a tick.
///
/// Alignment strategy: **top-align** the clicked turn, i.e. scroll so the
/// turn's band starts at the top of the reading column. Centring would need the
/// viewport height, which this signature does not carry, and top-aligning is
/// also what the reader wants — the turn's first line is what they clicked to
/// read. The result is clamped to `0.0..=content_height`; the scroll container
/// applies its own bottom clamp.
pub fn scroll_target(tick: &RulerTick, content_height: f32, ruler_height: f32) -> f32 {
    if !content_height.is_finite()
        || !ruler_height.is_finite()
        || !tick.y.is_finite()
        || content_height <= 0.0
        || ruler_height <= 0.0
    {
        return 0.0;
    }
    let fraction = (tick.y / ruler_height).clamp(0.0, 1.0);
    (fraction * content_height).clamp(0.0, content_height)
}

/// The visible slice of the thread, expressed as `(top, bottom)` fractions of
/// the strip height, for painting a position indicator.
///
/// `None` when the thread fits the viewport (nothing to indicate) or when the
/// ruler is hidden because there is only one turn. Fractions instead of pixels
/// because `ruler_height` is not part of the signature: the painter multiplies
/// them by its own strip height.
pub fn visible_window(
    turn_count: usize,
    scroll_offset: f32,
    content_height: f32,
    viewport_height: f32,
) -> Option<(f32, f32)> {
    if !visible(turn_count)
        || !content_height.is_finite()
        || !viewport_height.is_finite()
        || content_height <= 0.0
        || viewport_height <= 0.0
    {
        return None;
    }
    if content_height <= viewport_height {
        return None;
    }

    let max_scroll = content_height - viewport_height;
    // Overscroll (rubber-banding) and NaN offsets both fall back to the top
    // instead of producing a window that points outside the strip.
    let scroll = if scroll_offset.is_finite() {
        scroll_offset.clamp(0.0, max_scroll)
    } else {
        0.0
    };
    let top = (scroll / content_height).clamp(0.0, 1.0);
    let bottom = ((scroll + viewport_height) / content_height).clamp(0.0, 1.0);
    Some((top, bottom))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() < 1e-3
    }

    fn tick(turn_index: usize, y: f32, height: f32) -> RulerTick {
        RulerTick {
            turn_index,
            y,
            height,
        }
    }

    #[test]
    fn single_turn_hides_the_ruler_and_multi_turn_shows_it() {
        assert!(!visible(0));
        assert!(!visible(1));
        assert!(visible(2));
        assert!(visible(37));
    }

    #[test]
    fn ticks_tile_the_strip_without_overlap() {
        let ticks = ticks(8, 4_000.0, 0.0, 600.0, 0.0, 480.0);
        assert_eq!(ticks.len(), 8);
        assert!(approx(ticks[0].y, 0.0));
        for pair in ticks.windows(2) {
            let earlier = pair[0];
            let later = pair[1];
            assert!(later.y > earlier.y, "tick y must strictly increase");
            assert!(
                earlier.y + earlier.height <= later.y + 1e-3,
                "tick bands must not overlap"
            );
        }
        let last = ticks[ticks.len() - 1];
        assert!(approx(last.y + last.height, 480.0));
        for (index, item) in ticks.iter().enumerate() {
            assert_eq!(item.turn_index, index);
            assert!(item.height >= MIN_TICK_HEIGHT);
        }
    }

    #[test]
    fn dense_thread_keeps_the_minimum_tick_height() {
        // 600px / 400 turns = 1.5px per band, below the 3px floor.
        let ticks = ticks(400, 40_000.0, 0.0, 600.0, 0.0, 600.0);
        assert_eq!(ticks.len(), 400);
        for item in &ticks {
            assert!(approx(item.height, MIN_TICK_HEIGHT));
        }
        for pair in ticks.windows(2) {
            assert!(pair[1].y > pair[0].y);
        }
    }

    #[test]
    fn degenerate_or_offscreen_strips_return_empty_without_panicking() {
        assert!(ticks(0, 1_000.0, 0.0, 600.0, 0.0, 600.0).is_empty());
        assert!(ticks(5, 0.0, 0.0, 600.0, 0.0, 600.0).is_empty());
        assert!(ticks(5, -10.0, 0.0, 600.0, 0.0, 600.0).is_empty());
        assert!(ticks(5, 1_000.0, 0.0, 600.0, 0.0, 0.0).is_empty());
        assert!(ticks(5, 1_000.0, 0.0, 0.0, 0.0, 600.0).is_empty());
        // Lane clipped above or scrolled past the viewport.
        assert!(ticks(5, 1_000.0, 600.0, 600.0, 0.0, 600.0).is_empty());
        assert!(ticks(5, 1_000.0, 0.0, 600.0, 700.0, 600.0).is_empty());
        // Fully visible lane still projects.
        assert_eq!(ticks(5, 1_000.0, 0.0, 600.0, 0.0, 600.0).len(), 5);
    }

    #[test]
    fn hover_binds_gaps_to_the_nearest_tick_and_rejects_far_pointers() {
        let ticks = [tick(0, 10.0, 3.0), tick(1, 50.0, 3.0), tick(2, 90.0, 3.0)];

        // Inside marks.
        assert_eq!(hovered(&ticks, 11.0, 0.0), Some(0));
        assert_eq!(hovered(&ticks, 51.0, 0.0), Some(1));

        // 30.0 is 17px below tick 0's bottom and 20px above tick 1's top.
        assert_eq!(hovered(&ticks, 30.0, 20.0), Some(0));
        // 40.0 is 27px from tick 0 and 10px from tick 1.
        assert_eq!(hovered(&ticks, 40.0, 20.0), Some(1));
        // Without tolerance the gap is dead.
        assert_eq!(hovered(&ticks, 30.0, 0.0), None);
        // Outside the whole strip (and beyond tolerance).
        assert_eq!(hovered(&ticks, -40.0, HIT_TOLERANCE), None);
        assert_eq!(hovered(&ticks, 400.0, HIT_TOLERANCE), None);
        assert_eq!(hovered(&[], 10.0, HIT_TOLERANCE), None);
    }

    #[test]
    fn hover_prefers_the_nearest_centre_inside_overlapping_ticks() {
        // Dense strip: bands overlap because of the minimum height.
        let ticks = [tick(0, 10.0, 6.0), tick(1, 12.0, 6.0), tick(2, 14.0, 6.0)];
        assert_eq!(hovered(&ticks, 11.0, 0.0), Some(0));
        assert_eq!(hovered(&ticks, 15.0, 0.0), Some(1));
        assert_eq!(hovered(&ticks, 18.0, 0.0), Some(2));
    }

    #[test]
    fn summary_truncates_by_chars_and_handles_empty_first_line() {
        assert_eq!(
            summary_text(2, "帮我整理这份资料"),
            "第 3 轮 · 帮我整理这份资料"
        );

        let long = "汉".repeat(40);
        let summary = summary_text(0, &long);
        assert_eq!(
            summary,
            format!("第 1 轮 · {}", "汉".repeat(SUMMARY_MAX_CHARS)) + "…"
        );
        // 32 chars + ellipsis; the Chinese bytes must not be sliced.
        assert_eq!(
            summary.chars().count(),
            "第 1 轮 · ".chars().count() + SUMMARY_MAX_CHARS + 1
        );

        let exact = "a".repeat(SUMMARY_MAX_CHARS);
        assert!(!summary_text(0, &exact).contains('…'));

        assert_eq!(summary_text(4, ""), "第 5 轮");
        assert_eq!(summary_text(4, "   "), "第 5 轮");
        // Only the first line reaches the tooltip.
        assert_eq!(summary_text(0, "第一行\n第二行"), "第 1 轮 · 第一行");
    }

    #[test]
    fn scroll_target_stays_inside_the_content() {
        let strip = ticks(5, 2_500.0, 0.0, 500.0, 0.0, 500.0);
        assert!(approx(scroll_target(&strip[0], 2_500.0, 500.0), 0.0));
        for item in &strip {
            let target = scroll_target(item, 2_500.0, 500.0);
            assert!((0.0..=2_500.0).contains(&target));
            assert!(approx(target, item.y / 500.0 * 2_500.0));
        }
        // Top-aligning: the second turn of five starts at 20% of the content.
        assert!(approx(scroll_target(&strip[1], 2_500.0, 500.0), 500.0));
        // Degenerate input never leaves the valid range.
        assert_eq!(scroll_target(&tick(0, 10.0, 3.0), 0.0, 500.0), 0.0);
        assert_eq!(scroll_target(&tick(0, 10.0, 3.0), 2_500.0, 0.0), 0.0);
    }

    #[test]
    fn visible_window_is_none_when_the_content_fits() {
        assert_eq!(visible_window(4, 0.0, 400.0, 400.0), None);
        assert_eq!(visible_window(4, 0.0, 300.0, 400.0), None);
        assert_eq!(visible_window(1, 0.0, 4_000.0, 400.0), None);
        assert_eq!(visible_window(4, 0.0, 4_000.0, 0.0), None);
    }

    #[test]
    fn visible_window_maps_scroll_into_strip_fractions() {
        let (top, bottom) = visible_window(4, 0.0, 4_000.0, 400.0).unwrap_or((0.0, 0.0));
        assert!(approx(top, 0.0));
        assert!(approx(bottom, 0.1));

        // Overscroll clamps to the top edge.
        let (top, bottom) = visible_window(4, -80.0, 4_000.0, 400.0).unwrap_or((0.0, 0.0));
        assert!(approx(top, 0.0));
        assert!(approx(bottom, 0.1));

        // Bottom of the thread: 4_000 - 400 = 3_600.
        let (top, bottom) = visible_window(4, 9_999.0, 4_000.0, 400.0).unwrap_or((0.0, 0.0));
        assert!(approx(top, 0.9));
        assert!(approx(bottom, 1.0));
    }
}
