use serde::Serialize;

pub const MAX_USER_LEVEL: i64 = 200;

const BASE_LEVEL_XP: i64 = 240;
const LINEAR_LEVEL_XP: i64 = 28;
const QUADRATIC_LEVEL_XP_NUM: i64 = 9;
const QUADRATIC_LEVEL_XP_DEN: i64 = 10;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserLevelSnapshot {
    pub level: i64,
    pub max_level: i64,
    pub experience_total: i64,
    pub experience_current: i64,
    pub experience_for_next_level: i64,
    pub experience_remaining: i64,
    pub experience_progress: f64,
    pub reached_max_level: bool,
}

pub fn normalize_total_experience(total: i64) -> i64 {
    total.max(0)
}

pub fn experience_from_runtime_seconds(runtime_s: f64) -> i64 {
    if !runtime_s.is_finite() || runtime_s <= 0.0 {
        return 0;
    }
    runtime_s.floor() as i64
}

pub fn experience_required_for_level(level: i64) -> i64 {
    let level = level.clamp(1, MAX_USER_LEVEL.saturating_sub(1));
    let quadratic = (level * level * QUADRATIC_LEVEL_XP_NUM) / QUADRATIC_LEVEL_XP_DEN;
    BASE_LEVEL_XP + (LINEAR_LEVEL_XP * level) + quadratic
}

pub fn build_user_level_snapshot(total: i64) -> UserLevelSnapshot {
    let experience_total = normalize_total_experience(total);
    let mut remaining = experience_total;
    let mut level = 1_i64;

    while level < MAX_USER_LEVEL {
        let required = experience_required_for_level(level);
        if remaining < required {
            let progress = if required > 0 {
                remaining as f64 / required as f64
            } else {
                1.0
            };
            return UserLevelSnapshot {
                level,
                max_level: MAX_USER_LEVEL,
                experience_total,
                experience_current: remaining,
                experience_for_next_level: required,
                experience_remaining: (required - remaining).max(0),
                experience_progress: progress.clamp(0.0, 1.0),
                reached_max_level: false,
            };
        }
        remaining -= required;
        level += 1;
    }

    UserLevelSnapshot {
        level: MAX_USER_LEVEL,
        max_level: MAX_USER_LEVEL,
        experience_total,
        experience_current: 0,
        experience_for_next_level: 0,
        experience_remaining: 0,
        experience_progress: 1.0,
        reached_max_level: true,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_user_level_snapshot, experience_from_runtime_seconds, experience_required_for_level,
        MAX_USER_LEVEL,
    };

    #[test]
    fn runtime_seconds_convert_to_whole_experience_points() {
        assert_eq!(experience_from_runtime_seconds(-1.0), 0);
        assert_eq!(experience_from_runtime_seconds(0.9), 0);
        assert_eq!(experience_from_runtime_seconds(1.0), 1);
        assert_eq!(experience_from_runtime_seconds(12.8), 12);
    }

    #[test]
    fn level_curve_starts_hard_and_keeps_growing() {
        assert_eq!(experience_required_for_level(1), 268);
        assert_eq!(experience_required_for_level(2), 299);
        assert_eq!(experience_required_for_level(10), 610);
        assert!(experience_required_for_level(50) > experience_required_for_level(10));
        assert!(experience_required_for_level(100) > experience_required_for_level(50));
    }

    #[test]
    fn snapshot_advances_levels_and_caps_at_max() {
        let level_one = build_user_level_snapshot(100);
        assert_eq!(level_one.level, 1);
        assert_eq!(level_one.experience_current, 100);
        assert_eq!(level_one.experience_for_next_level, 268);

        let level_two = build_user_level_snapshot(268);
        assert_eq!(level_two.level, 2);
        assert_eq!(level_two.experience_current, 0);
        assert_eq!(level_two.experience_for_next_level, 299);

        let mut total = 0_i64;
        for level in 1..MAX_USER_LEVEL {
            total += experience_required_for_level(level);
        }
        let maxed = build_user_level_snapshot(total);
        assert_eq!(maxed.level, MAX_USER_LEVEL);
        assert!(maxed.reached_max_level);
        assert_eq!(maxed.experience_progress, 1.0);
    }
}
