//! Daily activity buckets behind the desktop new-thread heatmap.
//!
//! The grid needs one number per calendar day for a year. Both series come
//! from indexed `GROUP BY` aggregates, so an empty thread never pays for a
//! scan of the whole conversation history.
use super::NativeDesktop;
use anyhow::Result;
use chrono::{Local, NaiveDate, TimeZone};
use serde_json::{json, Value};

/// 53 weekly columns x 7 days is the widest grid the UI renders.
pub const DEFAULT_ACTIVITY_DAYS: i64 = 371;
const MAX_ACTIVITY_DAYS: i64 = 378;

/// First instant of the local day `days - 1` counted back from `today`.
pub fn activity_window_start(today: NaiveDate, days: i64) -> f64 {
    let start = today - chrono::Duration::days(days.saturating_sub(1));
    start
        .and_hms_opt(0, 0, 0)
        .and_then(|at| Local.from_local_datetime(&at).single())
        .map(|at| at.timestamp() as f64)
        .unwrap_or(0.0)
}

fn series(buckets: Vec<(String, i64)>) -> Vec<Value> {
    buckets
        .into_iter()
        .map(|(date, value)| json!({ "date": date, "value": value }))
        .collect()
}

impl NativeDesktop {
    /// Sparse day buckets for the signed-in desktop user: threads opened per
    /// day and quota credits spent per day. Days without activity are omitted.
    pub fn daily_activity(&self, days: Option<i64>) -> Result<Value> {
        let days = days
            .unwrap_or(DEFAULT_ACTIVITY_DAYS)
            .clamp(1, MAX_ACTIVITY_DAYS);
        let today = Local::now().date_naive();
        let since_at = activity_window_start(today, days);
        let user_id = self.user_id();
        let storage = self.state().storage.clone();
        let sessions = storage.count_chat_sessions_by_day(user_id, since_at)?;
        let credits = storage.sum_monitor_quota_used_by_day(user_id, since_at)?;
        Ok(json!({
            "today": today.format("%Y-%m-%d").to_string(),
            "days": days,
            "sessions": series(sessions),
            "credits": series(credits),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn window_starts_at_local_midnight_of_the_first_day() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let since_at = activity_window_start(today, 371);
        let first = Local
            .timestamp_opt(since_at as i64, 0)
            .single()
            .map(|at| at.date_naive());
        assert_eq!(first, Some(today - chrono::Duration::days(370)));
        assert_eq!(
            Local
                .timestamp_opt(activity_window_start(today, 1) as i64, 0)
                .single()
                .map(|at| at.date_naive()),
            Some(today)
        );
    }

    #[test]
    fn series_keeps_date_and_value_pairs() {
        let values = series(vec![("2026-10-12".to_string(), 3)]);
        assert_eq!(values[0]["date"], json!("2026-10-12"));
        assert_eq!(values[0]["value"], json!(3));
    }
}
