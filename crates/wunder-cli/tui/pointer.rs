//! Wheel semantics for the inline-viewport TUI.
//!
//! The main screen keeps finished transcript rows in the console's own
//! scrollback. Which side owns the wheel is therefore decided once, by the
//! console's capabilities, not per pointer position: a console that cannot be
//! trusted to deliver wheel records keeps its own scrolling (native mode),
//! while a modern one lets the app own clicks and the wheel (capture mode).
//!
//! In capture mode a notch must always land somewhere. Gating scrolling on a
//! hit rect is what makes a console look dead when its rects are stale or have
//! not been painted yet, so the caller routes the wheel to the transcript by
//! default and only lets the composer claim it when the composer can actually
//! move its own band.

use crossterm::event::MouseEventKind;

/// Rows one wheel notch moves by default. Kept at the transcript's historical
/// step so the fix does not silently make scrolling three times slower; the
/// accumulator below makes fractional speeds possible without losing notches.
pub(crate) const DEFAULT_WHEEL_SPEED: f64 = 3.0;

/// Fractional wheel notches that do not yet add up to a whole row.
///
/// A terminal may be configured to scroll slower than one row per notch, and a
/// console may deliver several notches inside one frame. Carrying the fraction
/// is what keeps both cases honest. Reversing direction clears the remainder so
/// a reader never has to "pay back" a previous scroll before the view moves.
#[derive(Debug, Clone)]
pub(crate) struct WheelScroll {
    pending: f64,
    speed: f64,
}

impl WheelScroll {
    pub(crate) fn new(speed: f64) -> Self {
        Self {
            pending: 0.0,
            speed: if speed.is_finite() && speed > 0.0 {
                speed
            } else {
                DEFAULT_WHEEL_SPEED
            },
        }
    }

    /// Consume one notch. `direction` is negative for up / away from the tail,
    /// positive for down / towards it. Returns the signed row delta, which may
    /// be zero while a fractional remainder is still accumulating.
    pub(crate) fn notch(&mut self, direction: f64) -> isize {
        if direction == 0.0 || !direction.is_finite() {
            self.pending = 0.0;
            return 0;
        }
        let direction = direction.signum();
        if self.pending * direction < 0.0 {
            self.pending = 0.0;
        }
        self.pending += direction * self.speed;
        // Repeated addition can land a hair away from a whole row.
        let rounded = self.pending.round();
        if (self.pending - rounded).abs() < 1e-9 {
            self.pending = rounded;
        }
        let rows = self.pending.trunc() as isize;
        self.pending -= f64::from(rows as i32);
        rows
    }

    pub(crate) fn reset(&mut self) {
        self.pending = 0.0;
    }
}

impl Default for WheelScroll {
    fn default() -> Self {
        Self::new(DEFAULT_WHEEL_SPEED)
    }
}

pub(crate) fn wheel_direction(kind: MouseEventKind) -> Option<f64> {
    match kind {
        MouseEventKind::ScrollUp => Some(-1.0),
        MouseEventKind::ScrollDown => Some(1.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_moves_one_notch_worth_of_rows() {
        let mut wheel = WheelScroll::new(3.0);
        assert_eq!(wheel.notch(-1.0), -3);
        assert_eq!(wheel.notch(-1.0), -3);
        assert_eq!(wheel.notch(1.0), 3);
    }

    #[test]
    fn reversing_the_wheel_clears_the_remainder_immediately() {
        let mut wheel = WheelScroll::new(0.5);
        assert_eq!(wheel.notch(1.0), 0, "half a row is still pending");
        assert_eq!(wheel.notch(-1.0), 0, "the pending half is dropped");
        assert_eq!(wheel.notch(-1.0), -1, "the next notch moves a whole row");
    }

    #[test]
    fn fractional_speeds_eventually_add_up_without_drifting() {
        let mut wheel = WheelScroll::new(0.4);
        let mut total = 0;
        for _ in 0..10 {
            total += wheel.notch(1.0);
        }
        assert_eq!(total, 4, "ten notches at 0.4 rows are four rows");
        wheel.reset();
        assert_eq!(wheel.notch(1.0), 0);
    }

    #[test]
    fn an_invalid_speed_falls_back_to_the_default() {
        let mut wheel = WheelScroll::new(f64::NAN);
        assert_eq!(wheel.notch(1.0), DEFAULT_WHEEL_SPEED as isize);
    }

    #[test]
    fn only_vertical_wheel_kinds_have_a_direction() {
        assert_eq!(wheel_direction(MouseEventKind::ScrollUp), Some(-1.0));
        assert_eq!(wheel_direction(MouseEventKind::ScrollDown), Some(1.0));
        assert_eq!(wheel_direction(MouseEventKind::Moved), None);
    }
}
