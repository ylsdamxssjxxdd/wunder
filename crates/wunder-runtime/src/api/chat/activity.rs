use super::{error_response, now_ts};
use crate::api::user_context::resolve_user;
use crate::core::blocking;
use crate::i18n;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, Json, Router};
use chrono::{Local, TimeZone};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::warn;

/// 53 weekly columns x 7 days is the widest grid the UI renders.
const DEFAULT_ACTIVITY_DAYS: i64 = 371;
const MAX_ACTIVITY_DAYS: i64 = 378;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new().route("/wunder/chat/activity/daily", get(daily_activity))
}

#[derive(Debug, Deserialize)]
struct DailyActivityQuery {
    #[serde(default)]
    days: Option<i64>,
}

fn resolve_since_at(today: chrono::NaiveDate, days: i64) -> f64 {
    let start_day = today - chrono::Duration::days(days.saturating_sub(1));
    start_day
        .and_hms_opt(0, 0, 0)
        .and_then(|start| Local.from_local_datetime(&start).single())
        .map(|start| start.timestamp() as f64)
        .unwrap_or_else(|| now_ts() - (days as f64) * 86_400.0)
}

fn day_series_to_json(buckets: Vec<(String, i64)>) -> Vec<Value> {
    buckets
        .into_iter()
        .map(|(date, value)| json!({ "date": date, "value": value }))
        .collect()
}

/// Daily thread-start and credit-consumption buckets for the signed-in user.
/// Only days with activity are returned; the caller fills the gaps.
async fn daily_activity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<DailyActivityQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let days = query
        .days
        .unwrap_or(DEFAULT_ACTIVITY_DAYS)
        .clamp(1, MAX_ACTIVITY_DAYS);
    let today = Local::now().date_naive();
    let since_at = resolve_since_at(today, days);

    let storage = state.storage.clone();
    let user_id = resolved.user.user_id.clone();
    let (sessions, credits) = blocking::run_db(
        "api.chat.daily_activity",
        move || -> anyhow::Result<(Vec<(String, i64)>, Vec<(String, i64)>)> {
            Ok((
                storage.count_chat_sessions_by_day(&user_id, since_at)?,
                storage.sum_monitor_quota_used_by_day(&user_id, since_at)?,
            ))
        },
    )
    .await
    .map_err(|err| {
        warn!(error = %err, "daily activity read failed");
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            i18n::t("error.internal_error"),
        )
    })?;

    Ok(Json(json!({
        "data": {
            "today": today.format("%Y-%m-%d").to_string(),
            "days": days,
            "sessions": day_series_to_json(sessions),
            "credits": day_series_to_json(credits),
        }
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn daily_activity_window_is_bounded_to_the_grid() {
        let clamped = |days: Option<i64>| {
            days.unwrap_or(DEFAULT_ACTIVITY_DAYS)
                .clamp(1, MAX_ACTIVITY_DAYS)
        };
        assert_eq!(clamped(None), 371);
        assert_eq!(clamped(Some(-5)), 1);
        assert_eq!(clamped(Some(10_000)), 378);
    }

    #[test]
    fn since_at_starts_at_local_midnight_of_the_first_day() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 12).unwrap();
        let since_at = resolve_since_at(today, 371);
        let first_day = Local
            .timestamp_opt(since_at as i64, 0)
            .single()
            .map(|value| value.date_naive());
        assert_eq!(first_day, Some(today - chrono::Duration::days(370)));
        // A one-day window covers only today.
        let single = resolve_since_at(today, 1);
        assert_eq!(
            Local
                .timestamp_opt(single as i64, 0)
                .single()
                .map(|value| value.date_naive()),
            Some(today)
        );
    }

    #[test]
    fn sparse_series_keep_date_and_value_pairs() {
        let series = day_series_to_json(vec![
            ("2026-01-02".to_string(), 3),
            ("2026-01-05".to_string(), 0),
        ]);
        assert_eq!(series.len(), 2);
        assert_eq!(series[0]["date"], json!("2026-01-02"));
        assert_eq!(series[0]["value"], json!(3));
    }
}
