//! Shared axis model for the four trajectory surfaces.
//!
//! The ledger is paged, but the axis must describe the whole thread.  Every
//! surface projects the same domain into a pixel viewport, so the tick step,
//! label formatting, turn markers and pixel-level folding live here rather than
//! being reimplemented per shell.  The JS/TS ports in `web/shared` and
//! `frontend/src/views/trajectory` mirror this file; keep their test vectors in
//! sync.
//!
//! Two coordinate modes exist:
//! * `Sequence`: x is the durable event ordinal; labels are `#<ordinal>`.
//! * `Duration`: x is wall-clock milliseconds; labels are the offset from the
//!   domain origin, not an absolute clock reading.  The idle-compressed mode is
//!   still an offset axis, so `Δt` is used as the unit marker.

/// One tick per roughly this many pixels before label-collision widening.
pub const TICK_TARGET_PX: f64 = 120.0;
/// Minimum horizontal distance between two text labels.
pub const MIN_LABEL_GAP_PX: f64 = 56.0;
/// Minimum distance between two labelled turn markers.
pub const MIN_TURN_GAP_PX: f64 = 48.0;
/// Minimum painted width of one span; denser spans are folded per pixel.
pub const MIN_SPAN_PX: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisMode {
    Sequence,
    Duration,
}

impl AxisMode {
    /// Language-neutral unit marker drawn in the axis corner.
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Sequence => "#E",
            Self::Duration => "Δt",
        }
    }

    pub const fn timed(self) -> bool {
        matches!(self, Self::Duration)
    }
}

const SEQUENCE_MANTISSA: [f64; 3] = [1.0, 2.0, 5.0];

/// Human time steps in milliseconds: 1/2/5 decades of seconds, then
/// quarter/hour/day multiples.  A decimal-only ladder produces 100 s / 200 s /
/// 500 s steps, which read as `1m 40s`, `3m 20s`, `8m 20s` on a long thread.
const DURATION_STEPS_MS: [f64; 30] = [
    1.0,
    2.0,
    5.0,
    10.0,
    20.0,
    50.0,
    100.0,
    200.0,
    500.0,
    1_000.0,
    2_000.0,
    5_000.0,
    10_000.0,
    15_000.0,
    30_000.0,
    60_000.0,
    120_000.0,
    300_000.0,
    600_000.0,
    900_000.0,
    1_800_000.0,
    3_600_000.0,
    7_200_000.0,
    10_800_000.0,
    21_600_000.0,
    43_200_000.0,
    86_400_000.0,
    172_800_000.0,
    604_800_000.0,
    2_592_000_000.0,
];

const DAY_MS: f64 = 86_400_000.0;

fn finite(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

/// Smallest human step that is at least `raw`.
pub fn nice_step(raw: f64, mode: AxisMode) -> f64 {
    let raw = finite(raw, 1.0).max(f64::MIN_POSITIVE);
    match mode {
        AxisMode::Sequence => {
            let magnitude = 10_f64.powf(raw.log10().floor());
            let residual = raw / magnitude;
            let mantissa = if residual > 5.0 {
                10.0
            } else if residual > 2.0 {
                5.0
            } else if residual > 1.0 {
                2.0
            } else {
                1.0
            };
            mantissa * magnitude
        }
        AxisMode::Duration => DURATION_STEPS_MS
            .iter()
            .copied()
            .find(|step| *step >= raw)
            .unwrap_or_else(|| (raw / DAY_MS).ceil() * DAY_MS),
    }
}

/// Next step on the same ladder, used to widen ticks until labels do not
/// overlap.  Returns `None` when the ladder is exhausted.
pub fn next_nice_step(step: f64, mode: AxisMode) -> f64 {
    match mode {
        AxisMode::Sequence => {
            let magnitude = 10_f64.powf((step.max(f64::MIN_POSITIVE)).log10().floor());
            let residual = (step / magnitude).round();
            let next = SEQUENCE_MANTISSA
                .iter()
                .copied()
                .find(|m| *m > residual + 1e-9)
                .unwrap_or(10.0);
            next * magnitude
        }
        AxisMode::Duration => DURATION_STEPS_MS
            .iter()
            .copied()
            .find(|candidate| *candidate > step + 1e-9)
            .unwrap_or_else(|| step + DAY_MS),
    }
}

fn trim(value: f64) -> f64 {
    // Absolute timestamps are ~1e12; keep the arithmetic deterministic when a
    // viewport is many hours wide.
    if value.abs() < 1e-6 {
        0.0
    } else {
        value
    }
}

/// Compact offset label.  Kept short so ticks stay readable in a 10 px font.
pub fn format_offset(ms: f64) -> String {
    let value = trim(ms).abs();
    if value < 1_000.0 {
        return format!("{}ms", value.round() as i64);
    }
    if value < 60_000.0 {
        let seconds = value / 1_000.0;
        return if value < 10_000.0 {
            format!("{seconds:.1}s")
        } else {
            format!("{}s", seconds.round() as i64)
        };
    }
    if value < 3_600_000.0 {
        let minutes = (value / 60_000.0).floor() as i64;
        let seconds = ((value % 60_000.0) / 1_000.0).round() as i64;
        return if seconds > 0 {
            format!("{minutes}m {seconds:02}s")
        } else {
            format!("{minutes}m")
        };
    }
    if value < DAY_MS {
        let hours = (value / 3_600_000.0).floor() as i64;
        let minutes = ((value % 3_600_000.0) / 60_000.0).round() as i64;
        return if minutes > 0 {
            format!("{hours}h {minutes:02}m")
        } else {
            format!("{hours}h")
        };
    }
    let days = (value / DAY_MS).floor() as i64;
    let hours = ((value % DAY_MS) / 3_600_000.0).round() as i64;
    if hours > 0 {
        format!("{days}d {hours:02}h")
    } else {
        format!("{days}d")
    }
}

/// Label for an offset measured from the domain origin.
pub fn format_label(offset: f64, mode: AxisMode) -> String {
    match mode {
        AxisMode::Sequence => format!("#{}", trim(offset).round() as i64),
        AxisMode::Duration => format_offset(offset),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AxisTick {
    /// Domain value at this tick (absolute ordinal or timestamp).
    pub value: f64,
    /// Pixel position inside the viewport.
    pub x: f64,
    pub label: String,
}

/// Ticks aligned to the *domain origin*, never to absolute Unix time.
///
/// Aligning `ceil(view_start / step) * step` on a millisecond timestamp makes
/// the first label an arbitrary remainder and yields 100 s / 500 s ladders.
/// Offsetting from `domain_start` keeps the first label at zero and every step
/// on a human boundary.
pub fn axis_ticks(
    domain_start: f64,
    view_start: f64,
    view_span: f64,
    width: f64,
    mode: AxisMode,
) -> Vec<AxisTick> {
    let width = finite(width, 0.0).max(0.0);
    let span = finite(view_span, 0.0).max(f64::MIN_POSITIVE);
    if width <= 0.0 || !view_start.is_finite() || !domain_start.is_finite() {
        return Vec::new();
    }
    let target = (width / TICK_TARGET_PX).floor().max(2.0);
    let mut step = nice_step(span / target, mode);
    // Widen until adjacent labels have room; bounded so a degenerate viewport
    // cannot spin forever.
    for _ in 0..32 {
        if step * width / span >= MIN_LABEL_GAP_PX || step >= span {
            break;
        }
        let next = next_nice_step(step, mode);
        if next <= step {
            break;
        }
        step = next;
    }
    let relative_start = trim(view_start - domain_start);
    let first_offset = (relative_start / step).ceil() * step;
    let view_end_offset = relative_start + span;
    let mut ticks = Vec::new();
    let mut offset = first_offset;
    // A hard iteration bound keeps malformed domains from producing an
    // unbounded DOM/model.
    for _ in 0..512 {
        if offset > view_end_offset + step * 1e-9 {
            break;
        }
        let value = domain_start + offset;
        let x = (value - view_start) / span * width;
        if x >= -0.5 && x <= width + 0.5 {
            ticks.push(AxisTick {
                value,
                x,
                label: format_label(offset, mode),
            });
        }
        offset += step;
    }
    ticks
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnTick {
    pub turn: i64,
    pub x: f64,
    /// Language-neutral round marker, e.g. `R12`.
    pub label: String,
}

/// Thin turn markers to a readable density.  Input is `(turn, pixel_x)`; the
/// result is sorted by position and keeps a minimum gap.  When crowded, round
/// numbers win so the remaining labels still describe a stable cadence.
pub fn thin_turns(turns: &[(i64, f64)], width: f64) -> Vec<TurnTick> {
    let width = finite(width, 0.0).max(0.0);
    if width <= 0.0 || turns.is_empty() {
        return Vec::new();
    }
    let mut candidates: Vec<(i64, f64)> = turns
        .iter()
        .copied()
        .filter(|(_, x)| x.is_finite() && *x >= 0.0 && *x <= width)
        .collect();
    candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
    let capacity = (width / MIN_TURN_GAP_PX).floor().max(1.0) as usize;
    let cadence = if candidates.len() > capacity {
        let ratio = candidates.len() as f64 / capacity as f64;
        // 1, 2, 5, 10 ... cadence; at least every marker is a multiple of it.
        let magnitude = 10_f64.powf(ratio.log10().floor());
        let residual = ratio / magnitude;
        (if residual > 5.0 {
            10.0
        } else if residual > 2.0 {
            5.0
        } else if residual > 1.0 {
            2.0
        } else {
            1.0
        } * magnitude)
            .round()
            .max(1.0) as i64
    } else {
        1
    };
    let mut result: Vec<TurnTick> = Vec::with_capacity(capacity.min(candidates.len()));
    let mut last_x = f64::NEG_INFINITY;
    for (turn, x) in candidates {
        if cadence > 1 && turn % cadence != 0 {
            continue;
        }
        if x - last_x < MIN_TURN_GAP_PX {
            continue;
        }
        result.push(TurnTick {
            turn,
            x,
            label: format!("R{turn}"),
        });
        last_x = x;
    }
    result
}

/// One already-projected span in pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelSpan {
    pub left: f64,
    pub right: f64,
    pub lane: i32,
    pub error: bool,
    pub selected: bool,
    /// Search/selection state.  A folded mark stays bright when any member
    /// matches, so `dimmed`/`outside` are AND-ed while `error`/`selected` are
    /// OR-ed.
    pub dimmed: bool,
    pub outside: bool,
    /// Negative values mean "no stable source record" (aggregate tiles).
    pub index: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldedSpan {
    pub left: f64,
    pub right: f64,
    pub lane: i32,
    pub error: bool,
    pub selected: bool,
    pub dimmed: bool,
    pub outside: bool,
    pub index: i64,
    pub count: usize,
}

/// Fold spans that would be painted narrower than `min_span` into one mark per
/// (lane, pixel bucket).  Dense threads otherwise become three solid bars and
/// thousands of hit targets.  Sparse viewports are returned untouched so every
/// record stays individually selectable.
pub fn fold_spans(spans: &[PixelSpan], width: f64, min_span: f64) -> Vec<FoldedSpan> {
    let width = finite(width, 0.0).max(0.0);
    let min_span = finite(min_span, MIN_SPAN_PX).max(f64::MIN_POSITIVE);
    if width <= 0.0 || spans.is_empty() {
        return Vec::new();
    }
    let dense = spans.len() as f64 * min_span > width;
    let project = |span: &PixelSpan| FoldedSpan {
        left: span.left.clamp(0.0, width),
        right: span.right.clamp(0.0, width),
        lane: span.lane,
        error: span.error,
        selected: span.selected,
        dimmed: span.dimmed,
        outside: span.outside,
        index: span.index,
        count: 1,
    };
    if !dense {
        return spans
            .iter()
            .filter(|span| span.right > 0.0 && span.left < width)
            .map(project)
            .collect();
    }
    let mut buckets: Vec<(i32, i64, FoldedSpan)> = Vec::with_capacity(spans.len().min(4096));
    for span in spans {
        if !span.left.is_finite() || !span.right.is_finite() {
            continue;
        }
        let left = span.left.clamp(0.0, width);
        let right = span.right.clamp(0.0, width);
        if right <= 0.0 || left >= width {
            continue;
        }
        let bucket = (left / min_span).floor() as i64;
        let key = (span.lane, bucket);
        if let Some((_, _, merged)) = buckets.iter_mut().find(|(lane, b, _)| (*lane, *b) == key) {
            merged.left = merged.left.min(left);
            merged.right = merged.right.max(right);
            merged.error |= span.error;
            merged.selected |= span.selected;
            merged.dimmed &= span.dimmed;
            merged.outside &= span.outside;
            if merged.index < 0 && span.index >= 0 {
                merged.index = span.index;
            }
            if span.selected && span.index >= 0 {
                merged.index = span.index;
            }
            merged.count += 1;
        } else {
            buckets.push((
                span.lane,
                bucket,
                FoldedSpan {
                    left,
                    right,
                    lane: span.lane,
                    error: span.error,
                    selected: span.selected,
                    dimmed: span.dimmed,
                    outside: span.outside,
                    index: span.index,
                    count: 1,
                },
            ));
        }
    }
    buckets.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    buckets.into_iter().map(|(_, _, span)| span).collect()
}

/// Geometry of the horizontal viewport scrollbar drawn under the timeline.
/// Both fractions are relative to the whole track width, so every surface
/// renders the same thumb regardless of its pixel width.  The normalized
/// Slint viewport is simply `domain 0..1` with `view_span = view_width`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisScrollbar {
    /// Left edge of the thumb, 0..1 of the track.
    pub thumb_start: f64,
    /// Width of the thumb, 0..1 of the track.
    pub thumb_width: f64,
    /// False when the whole domain already fits (thumb fills the track).
    pub scrollable: bool,
}

pub fn axis_scrollbar(
    domain_start: f64,
    domain_end: f64,
    view_start: f64,
    view_span: f64,
) -> AxisScrollbar {
    let total = (domain_end - domain_start).max(1e-9);
    if !total.is_finite() || !view_start.is_finite() || !view_span.is_finite() {
        return AxisScrollbar {
            thumb_start: 0.0,
            thumb_width: 1.0,
            scrollable: false,
        };
    }
    let span = view_span.clamp(0.0, total);
    let start = view_start.clamp(domain_start, domain_end - span);
    let thumb_width = (span / total).clamp(0.0, 1.0);
    AxisScrollbar {
        thumb_start: ((start - domain_start) / total).clamp(0.0, 1.0 - thumb_width),
        thumb_width,
        scrollable: thumb_width < 0.999,
    }
}

/// Map a pointer position on the scrollbar track back to a viewport start.
/// `pointer_fraction` is 0..1 of the track width; the result is already
/// clamped so the viewport never runs past the domain end.
pub fn scrollbar_view_start(
    domain_start: f64,
    domain_end: f64,
    view_span: f64,
    pointer_fraction: f64,
) -> f64 {
    let total = (domain_end - domain_start).max(1e-9);
    if !total.is_finite() || !pointer_fraction.is_finite() || !view_span.is_finite() {
        return domain_start;
    }
    let span = view_span.clamp(0.0, total);
    let max_fraction = (1.0 - span / total).max(0.0);
    domain_start + pointer_fraction.clamp(0.0, max_fraction) * total
}

/// One idle gap collapsed by [`IdleWarp`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IdleGap {
    pub start: f64,
    pub end: f64,
    /// Time already removed before this gap started.
    pub removed_before: f64,
    /// Width the gap keeps after compression.
    pub kept: f64,
}

impl IdleGap {
    pub fn size(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    pub fn dropped(&self) -> f64 {
        (self.size() - self.kept).max(0.0)
    }
}

/// Piecewise-linear time warp that collapses long pauses between activity.
///
/// A thread can sit idle for hours between two rounds.  Painting wall-clock
/// time then collapses every span into a sliver at the left edge and the axis
/// becomes unreadable.  The warp keeps each covered interval at its real width
/// and clamps every uncovered gap to a bounded width, which preserves the shape
/// of the work while removing the dead space.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IdleWarp {
    /// Sorted, non-overlapping gaps between covered intervals.
    pub gaps: Vec<IdleGap>,
    /// Real coordinate that maps to zero.
    pub base: f64,
    /// Real coordinate of the compressed end.
    pub real_end: f64,
}

impl IdleWarp {
    pub fn is_empty(&self) -> bool {
        self.gaps.is_empty()
    }

    /// Real time that was removed from the axis.
    pub fn dropped(&self) -> f64 {
        self.gaps.iter().map(IdleGap::dropped).sum()
    }

    /// Compressed coordinate of a real timestamp.
    pub fn map(&self, time: f64) -> f64 {
        if self.gaps.is_empty() || !time.is_finite() {
            return time;
        }
        let mut removed = 0.0;
        for gap in &self.gaps {
            if time >= gap.end {
                removed = gap.removed_before + gap.size() - gap.kept;
                continue;
            }
            if time > gap.start {
                return gap.start - gap.removed_before + (time - gap.start).min(gap.kept);
            }
            break;
        }
        time - removed
    }

    /// Real timestamp behind a compressed coordinate.  Coordinates inside a
    /// collapsed gap resolve to the moment the idle started, so a tick landing
    /// there still describes a real instant instead of an invented one.
    pub fn unmap(&self, value: f64) -> f64 {
        if self.gaps.is_empty() || !value.is_finite() {
            return value;
        }
        let mut removed = 0.0;
        for gap in &self.gaps {
            let start = gap.start - gap.removed_before;
            let end = start + gap.kept;
            if value >= end {
                removed = gap.removed_before + gap.size() - gap.kept;
                continue;
            }
            if value > start {
                return gap.start;
            }
            break;
        }
        value + removed
    }
}

/// One gap may keep at most this much (wall-clock ms), so a single enormous
/// pause cannot dominate an otherwise busy axis.
pub const IDLE_GAP_MAX_MS: f64 = 20_000.0;
/// All collapsed pauses together keep at most this fraction of covered time:
/// a hairline that separates rounds without looking like a break in the work.
pub const IDLE_GAP_BUDGET: f64 = 0.01;

/// Build a warp from covered intervals (spans, aggregate buckets, round
/// anchors).  Overlapping intervals are merged first, so fragments that tile
/// the same activity never create fake gaps.
///
/// Compression policy: long pauses are collapsed to a hairline.  The kept idle
/// total is at most 1% of covered time and every gap keeps a width proportional
/// to its real length, so the axis reads as continuous activity while round
/// boundaries stay findable.  A wide per-gap cap would instead turn hours of
/// idle into large empty stretches, which is exactly what the ruler must avoid.
pub fn idle_warp(intervals: &[(f64, f64)]) -> IdleWarp {
    let mut ranges: Vec<(f64, f64)> = intervals
        .iter()
        .filter(|(start, end)| start.is_finite() && end.is_finite())
        .map(|(start, end)| (*start, (*end).max(*start)))
        .collect();
    if ranges.len() < 2 {
        return IdleWarp {
            base: ranges.first().map(|(start, _)| *start).unwrap_or(0.0),
            real_end: ranges.first().map(|(_, end)| *end).unwrap_or(0.0),
            gaps: Vec::new(),
        };
    }
    ranges.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let base = merged[0].0;
    let real_end = merged[merged.len() - 1].1;
    if merged.len() < 2 {
        return IdleWarp {
            base,
            real_end,
            gaps: Vec::new(),
        };
    }
    let active: f64 = merged.iter().map(|(start, end)| end - start).sum();
    let idle: f64 = merged.windows(2).map(|pair| pair[1].0 - pair[0].1).sum();
    if !(active > 0.0) || !(idle > 0.0) {
        return IdleWarp {
            base,
            real_end,
            gaps: Vec::new(),
        };
    }
    // Pauses that are already negligible stay untouched; longer ones collapse
    // to a shared hairline budget, distributed proportionally.
    let scale = (active * IDLE_GAP_BUDGET / idle).min(1.0);
    let mut gaps = Vec::with_capacity(merged.len() - 1);
    let mut removed = 0.0;
    for pair in merged.windows(2) {
        let start = pair[0].1;
        let end = pair[1].0;
        if end <= start {
            continue;
        }
        let kept = ((end - start) * scale).min(IDLE_GAP_MAX_MS);
        gaps.push(IdleGap {
            start,
            end,
            removed_before: removed,
            kept,
        });
        removed += (end - start) - kept;
    }
    IdleWarp {
        gaps,
        base,
        real_end,
    }
}

/// Initial viewport fraction for a dense thread.
///
/// Fitting thousands of events into one track paints solid ribbons: at three
/// events per pixel there is nothing left to read.  The first view instead
/// keeps a bounded number of events on screen (about 4px each), right-aligned
/// to the newest activity like the ledger; the viewport scrollbar still covers
/// the whole thread, so nothing is hidden — it is simply not all squeezed into
/// the first frame.
pub fn density_view_width(total_events: usize, axis_width_px: f64) -> f64 {
    let width = finite(axis_width_px, 0.0).max(1.0);
    // Four pixels per event leaves room for the fold gaps and lane labels.
    let target = (width / 4.0).max(50.0);
    let total = total_events as f64;
    if !total.is_finite() || total <= target {
        return 1.0;
    }
    (target / total).clamp(0.002, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_gaps_are_capped_and_activity_kept() {
        // 1s of work, a one hour pause, then 1s more.
        let warp = idle_warp(&[(0.0, 1_000.0), (3_600_000.0, 3_601_000.0)]);
        assert_eq!(warp.base, 0.0);
        assert_eq!(warp.real_end, 3_601_000.0);
        assert_eq!(warp.gaps.len(), 1);
        let gap = warp.gaps[0];
        assert!(gap.kept <= IDLE_GAP_MAX_MS);
        // Proportional policy: the kept pause mirrors the covered time, so
        // activity and idle split the axis roughly in half.
        assert!(gap.kept <= 2_000.0);
        // Covered time is untouched: the second block still spans 1000ms and
        // starts right after the compressed gap.
        assert_eq!(warp.map(0.0), 0.0);
        assert_eq!(warp.map(1_000.0), 1_000.0);
        assert!((warp.map(3_600_000.0) - (1_000.0 + gap.kept)).abs() < 1e-6);
        assert!((warp.map(3_601_000.0) - (2_000.0 + gap.kept)).abs() < 1e-6);
        // A two hour axis collapses to seconds of dead space.
        assert!(warp.map(3_601_000.0) < 30_000.0);
        assert!(warp.dropped() > 3_500_000.0);
    }

    #[test]
    fn idle_warp_round_trips_inside_activity() {
        let warp = idle_warp(&[(0.0, 5_000.0), (600_000.0, 605_000.0)]);
        for time in [0.0, 1_234.0, 5_000.0, 600_000.0, 602_000.0, 605_000.0] {
            let round = warp.unmap(warp.map(time));
            assert!((round - time).abs() < 1e-6, "{time} -> {round}");
        }
        // Ticks landing inside the collapsed gap resolve to the idle start.
        let inside = warp.map(5_050.0);
        assert!((warp.unmap(inside) - 5_000.0).abs() < 1e-6);
    }

    #[test]
    fn idle_warp_merges_touching_intervals() {
        let warp = idle_warp(&[(0.0, 1_000.0), (1_000.0, 2_000.0), (2_000.0, 3_000.0)]);
        assert!(warp.is_empty());
        assert_eq!(warp.map(1_500.0), 1_500.0);
    }

    #[test]
    fn idle_gaps_collapse_to_hairlines() {
        // 200 one-second blocks separated by 10 minutes each.  All pauses
        // together keep at most 1% of the covered time and stay uniform, so
        // rounds remain findable without any visible dead stretch.
        let intervals: Vec<(f64, f64)> = (0..200)
            .map(|i| {
                let start = i as f64 * 601_000.0;
                (start, start + 1_000.0)
            })
            .collect();
        let warp = idle_warp(&intervals);
        assert_eq!(warp.gaps.len(), 199);
        let active = 200.0 * 1_000.0;
        let idle_total = warp.gaps.iter().map(|gap| gap.kept).sum::<f64>();
        assert!(idle_total <= active * IDLE_GAP_BUDGET + 1.0, "{idle_total}");
        let width = warp.gaps[0].kept;
        assert!(width > 0.0 && width < 50.0, "hairline width {width}");
        assert!(warp
            .gaps
            .windows(2)
            .all(|pair| (pair[1].kept - pair[0].kept).abs() < 1.0));
    }

    #[test]
    fn dense_threads_open_on_a_readable_window() {
        assert_eq!(density_view_width(120, 1000.0), 1.0);
        assert_eq!(density_view_width(250, 1000.0), 1.0);
        // 2000 events -> about 250 in view, not a solid ribbon.
        assert!((density_view_width(2000, 1000.0) - 0.125).abs() < 1e-9);
        assert!(density_view_width(30_000, 1000.0) < 0.01);
        // Narrow tracks fit fewer events, so the window shrinks accordingly.
        assert!((density_view_width(10_000, 100.0) - 0.005).abs() < 1e-9);
    }

    #[test]
    fn duration_steps_are_human_units() {
        assert_eq!(nice_step(720_000.0, AxisMode::Duration), 900_000.0);
        assert_eq!(nice_step(3_000.0, AxisMode::Duration), 5_000.0);
        assert_eq!(nice_step(90_000.0, AxisMode::Duration), 120_000.0);
        assert_eq!(nice_step(1.0, AxisMode::Duration), 1.0);
    }

    #[test]
    fn sequence_steps_stay_decimal() {
        assert_eq!(nice_step(144.0, AxisMode::Sequence), 200.0);
        assert_eq!(nice_step(500.0, AxisMode::Sequence), 500.0);
        assert_eq!(nice_step(600.0, AxisMode::Sequence), 1000.0);
    }

    #[test]
    fn ticks_start_at_domain_origin() {
        let origin = 1_700_000_000_000.0;
        let ticks = axis_ticks(origin, origin, 7_200_000.0, 1200.0, AxisMode::Duration);
        assert!(!ticks.is_empty());
        assert_eq!(ticks[0].label, "0ms");
        assert_eq!(ticks[0].x, 0.0);
        assert_eq!(ticks[1].label, "15m");
        assert!(ticks.iter().all(|tick| tick.x <= 1200.5));
    }

    #[test]
    fn ticks_are_aligned_after_panning() {
        let origin = 1_700_000_000_000.0;
        let step = 900_000.0;
        let view_start = origin + step * 2.0 + 123.0;
        let ticks = axis_ticks(origin, view_start, 7_200_000.0, 1200.0, AxisMode::Duration);
        assert_eq!(ticks[0].value, origin + step * 3.0);
        assert_eq!(ticks[0].label, "45m");
    }

    #[test]
    fn sequence_ticks_are_ordinals() {
        let ticks = axis_ticks(0.0, 0.0, 1_200.0, 1200.0, AxisMode::Sequence);
        assert_eq!(ticks[0].label, "#0");
        assert_eq!(ticks[1].label, "#200");
    }

    #[test]
    fn labels_are_short_and_readable() {
        assert_eq!(format_offset(0.0), "0ms");
        assert_eq!(format_offset(1_500.0), "1.5s");
        assert_eq!(format_offset(90_000.0), "1m 30s");
        assert_eq!(format_offset(3_600_000.0), "1h");
        assert_eq!(format_offset(90_000_000.0), "1d 01h");
    }

    #[test]
    fn turn_markers_are_thinned_and_cadenced() {
        let turns: Vec<(i64, f64)> = (1..=120).map(|turn| (turn, turn as f64 * 8.0)).collect();
        let thinned = thin_turns(&turns, 960.0);
        assert!(!thinned.is_empty());
        assert!(thinned.len() <= 20);
        assert!(thinned
            .windows(2)
            .all(|pair| pair[1].x - pair[0].x >= MIN_TURN_GAP_PX));
        assert_eq!(thinned[0].label, "R10");
    }

    #[test]
    fn sparse_spans_are_not_folded() {
        let spans: Vec<PixelSpan> = (0..10)
            .map(|i| PixelSpan {
                left: i as f64 * 40.0,
                right: i as f64 * 40.0 + 20.0,
                lane: 1,
                error: false,
                selected: false,
                dimmed: false,
                outside: false,
                index: i,
            })
            .collect();
        let folded = fold_spans(&spans, 1000.0, MIN_SPAN_PX);
        assert_eq!(folded.len(), 10);
        assert!(folded.iter().all(|span| span.count == 1));
    }

    #[test]
    fn dense_spans_fold_per_pixel_and_keep_errors() {
        let spans: Vec<PixelSpan> = (0..5000)
            .map(|i| PixelSpan {
                left: i as f64 * 0.2,
                right: i as f64 * 0.2 + 0.3,
                lane: (i % 3) as i32,
                error: i == 17,
                selected: i == 42,
                dimmed: i % 2 == 0,
                outside: false,
                index: i,
            })
            .collect();
        let folded = fold_spans(&spans, 1000.0, MIN_SPAN_PX);
        assert!(folded.len() < spans.len());
        assert!(folded.len() <= 1500);
        assert!(folded.iter().any(|span| span.error));
        assert!(folded.iter().any(|span| span.selected));
        assert!(folded.iter().any(|span| !span.dimmed));
        assert!(folded.iter().all(|span| !span.outside));
        assert!(folded.iter().all(|span| span.count >= 1));
    }

    #[test]
    fn scrollbar_geometry_tracks_a_zoomed_viewport() {
        let full = axis_scrollbar(0.0, 1000.0, 0.0, 1000.0);
        assert_eq!(full.thumb_start, 0.0);
        assert_eq!(full.thumb_width, 1.0);
        assert!(!full.scrollable);

        let zoomed = axis_scrollbar(0.0, 1000.0, 250.0, 250.0);
        assert_eq!(zoomed.thumb_start, 0.25);
        assert_eq!(zoomed.thumb_width, 0.25);
        assert!(zoomed.scrollable);

        // Past the right edge the thumb must stay inside the track.
        let clamped = axis_scrollbar(100.0, 1100.0, 5000.0, 250.0);
        assert_eq!(clamped.thumb_start, 0.75);
        assert_eq!(clamped.thumb_width, 0.25);

        assert_eq!(scrollbar_view_start(0.0, 1000.0, 250.0, 0.5), 500.0);
        assert_eq!(scrollbar_view_start(0.0, 1000.0, 250.0, 1.0), 750.0);
        assert_eq!(scrollbar_view_start(100.0, 1100.0, 250.0, 0.5), 600.0);

        // The normalized Slint viewport is the same geometry on domain 0..1.
        let native = axis_scrollbar(0.0, 1.0, 0.4, 0.2);
        assert_eq!(native.thumb_start, 0.4);
        assert_eq!(native.thumb_width, 0.2);
    }

    #[test]
    fn degenerate_viewport_is_bounded() {
        assert!(axis_ticks(0.0, f64::NAN, 1.0, 1000.0, AxisMode::Sequence).is_empty());
        assert!(axis_ticks(0.0, 0.0, 0.0, 1000.0, AxisMode::Duration).len() <= 512);
        assert!(fold_spans(&[], 0.0, MIN_SPAN_PX).is_empty());
    }
}
