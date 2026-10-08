//! Timing for the banner: how long the sweep runs and where the wheel is at a given moment.

use std::time::Duration;

/// Wall-clock length of the steering sweep; after it the pose holds.
pub(crate) const SPIN_DURATION: Duration = Duration::from_millis(2_400);

/// Start hard over (about 150 degrees of tiller) and let the wheel come back to centre.
const SWEEP_START_RAD: f32 = -2.60;

/// Damping of the return, in seconds.
const SWEEP_DECAY_SECS: f32 = 0.58;

/// Wobble rate of the overshoot.
const SWEEP_RATE_RAD_PER_SEC: f32 = 8.6;

/// Wheel angle at `elapsed_secs`, ending on the centred pose.
///
/// A damped overshoot rather than a linear ease: the wheel has to look like mass on a
/// rack, and the last few degrees of shudder are what sells it.
pub(crate) fn sweep_angle(elapsed_secs: f32) -> f32 {
    let t = elapsed_secs.max(0.0).min(SPIN_DURATION.as_secs_f32());
    SWEEP_START_RAD * (-t / SWEEP_DECAY_SECS).exp() * (SWEEP_RATE_RAD_PER_SEC * t).cos()
}

/// The pose the banner holds once the sweep is over.
pub(crate) const SETTLED_ANGLE: f32 = 0.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_starts_hard_over_and_returns_to_centre() {
        assert!(sweep_angle(0.0) < -2.0, "must start hard over");
        assert!(sweep_angle(0.6) > sweep_angle(0.0), "must swing back");
        assert!(
            sweep_angle(SPIN_DURATION.as_secs_f32()).abs() < 0.05,
            "must be within a few degrees of centre"
        );
    }

    #[test]
    fn sweep_overshoots_before_settling() {
        let samples: Vec<f32> = (0..24)
            .map(|step| sweep_angle(step as f32 * SPIN_DURATION.as_secs_f32() / 24.0))
            .collect();
        assert!(
            samples.iter().any(|value| *value > 0.05),
            "a damped return must overshoot once: {samples:?}"
        );
        assert!(
            samples.last().is_some_and(|value| value.abs() < 0.05),
            "the sweep must end near centre"
        );
    }

    #[test]
    fn clamps_input_outside_the_sweep() {
        assert_eq!(sweep_angle(-1.0), sweep_angle(0.0));
        assert_eq!(
            sweep_angle(9_999.0),
            sweep_angle(SPIN_DURATION.as_secs_f32())
        );
    }
}
