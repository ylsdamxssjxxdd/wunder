//! New-thread activity heatmap: a year of sparse day buckets from the
//! embedded runtime, projected into the weekly columns the card renders.
use crate::{ActivityCell, ActivityMonth, ActivityWeek, MainWindow};
use chrono::{Datelike, Local, NaiveDate};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use wunder_desktop::NativeDesktop;

/// Weekly columns on the card; seven days each, Sunday first.
pub const WEEK_COLUMNS: usize = 53;
const WEEK_DAYS: usize = 7;
/// A month is only named on a week that sits fully inside it, and labels keep
/// this much distance so they never collide.
const MIN_LABEL_GAP: usize = 4;
/// Labels that close would overflow the card, so the last weeks stay unnamed.
const LABEL_EDGE: usize = 4;
/// A new thread is entered often; one indexed year read is cheap but not free,
/// so repeated empties share a single refresh window.
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

pub(crate) struct Grid {
    pub weeks: Vec<Vec<ActivityCell>>,
    pub months: Vec<ActivityMonth>,
}

fn parse_day(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").ok()
}

pub(crate) fn parse_series(value: &serde_json::Value) -> Vec<(NaiveDate, i64)> {
    let Some(rows) = value.as_array() else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let date = parse_day(row["date"].as_str().unwrap_or(""))?;
            let value = row["value"].as_i64().unwrap_or(0).max(0);
            (value > 0).then_some((date, value))
        })
        .collect()
}

/// Steps relative to the rendered peak, so one busy day is the darkest cell
/// and a sparse year still reads as activity instead of a flat ramp.
pub(crate) fn level_of(value: i64, peak: i64) -> i32 {
    if value <= 0 || peak <= 0 {
        return 0;
    }
    let ratio = value as f64 / peak as f64;
    if ratio >= 1.0 {
        4
    } else if ratio >= 0.66 {
        3
    } else if ratio >= 0.33 {
        2
    } else {
        1
    }
}

pub(crate) fn build_grid(points: &[(NaiveDate, i64)], today: NaiveDate) -> Grid {
    // The window ends on the Saturday of the current week, so every column
    // keeps the same weekday row and the days after today stay visible as
    // placeholders.
    let end = today + chrono::Duration::days(6 - today.weekday().num_days_from_sunday() as i64);
    let start = end - chrono::Duration::days((WEEK_COLUMNS * WEEK_DAYS - 1) as i64);

    let mut values = HashMap::<NaiveDate, i64>::new();
    for (date, value) in points {
        if *date < start || *date > end {
            continue;
        }
        *values.entry(*date).or_insert(0) += value;
    }
    let peak = values.values().copied().max().unwrap_or(0);

    let day_at = |index: usize| start + chrono::Duration::days(index as i64);
    let mut weeks = Vec::with_capacity(WEEK_COLUMNS);
    for week in 0..WEEK_COLUMNS {
        let mut cells = Vec::with_capacity(WEEK_DAYS);
        for offset in 0..WEEK_DAYS {
            let date = day_at(week * WEEK_DAYS + offset);
            cells.push(ActivityCell {
                level: level_of(values.get(&date).copied().unwrap_or(0), peak),
                future: date > today,
            });
        }
        weeks.push(cells);
    }

    let mut months: Vec<ActivityMonth> = Vec::new();
    let mut named: Option<u32> = None;
    for week in 0..WEEK_COLUMNS {
        let first = day_at(week * WEEK_DAYS);
        let last = first + chrono::Duration::days(6);
        if first.month() != last.month() || named == Some(first.month()) {
            continue;
        }
        if let Some(previous) = months.last() {
            if (week - previous.column as usize) < MIN_LABEL_GAP {
                continue;
            }
        }
        if week + LABEL_EDGE > WEEK_COLUMNS {
            continue;
        }
        months.push(ActivityMonth {
            month: first.month() as i32,
            column: week as i32,
        });
        named = Some(first.month());
    }

    Grid { weeks, months }
}

fn weeks_model(weeks: Vec<Vec<ActivityCell>>) -> ModelRc<ActivityWeek> {
    ModelRc::new(VecModel::from(
        weeks
            .into_iter()
            .map(|cells| ActivityWeek {
                cells: ModelRc::new(VecModel::from(cells)),
            })
            .collect::<Vec<ActivityWeek>>(),
    ))
}

/// The card is fed from the single timeline projection point, so the runtime
/// handle and the refresh throttles live once per process.
struct ActivityContext {
    app: slint::Weak<MainWindow>,
    api: Arc<NativeDesktop>,
    last: Mutex<Instant>,
    busy: AtomicBool,
}

static CONTEXT: OnceLock<ActivityContext> = OnceLock::new();

/// Start the feed: one read at startup, then a refresh whenever a thread is
/// published with no message yet.
pub fn install(app: &MainWindow, runtime: Arc<NativeDesktop>) {
    let _ = CONTEXT.set(ActivityContext {
        app: app.as_weak(),
        api: runtime,
        last: Mutex::new(Instant::now() - REFRESH_INTERVAL * 2),
        busy: AtomicBool::new(false),
    });
    request_refresh();
}

/// Called by the timeline projection. An empty transcript is the only state
/// that shows the card, so it is also the moment to re-read the day buckets.
pub fn notify_timeline_rows(rows: usize) {
    if rows == 0 {
        request_refresh();
    }
}

fn request_refresh() {
    let Some(context) = CONTEXT.get() else {
        return;
    };
    let now = Instant::now();
    let last = *context
        .last
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if now < last + REFRESH_INTERVAL {
        return;
    }
    if context.busy.swap(true, Ordering::Relaxed) {
        return;
    }
    *context
        .last
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = now;
    let weak = context.app.clone();
    let api = context.api.clone();
    std::thread::spawn(move || {
        let payload = api.daily_activity(None).ok();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Some(context) = CONTEXT.get() {
                context.busy.store(false, Ordering::Relaxed);
            }
            let Some(payload) = payload else { return };
            let today = parse_day(payload["today"].as_str().unwrap_or(""))
                .unwrap_or_else(|| Local::now().date_naive());
            let sessions = build_grid(&parse_series(&payload["sessions"]), today);
            let credits = build_grid(&parse_series(&payload["credits"]), today);
            app.set_activity_sessions(weeks_model(sessions.weeks));
            app.set_activity_credits(weeks_model(credits.weeks));
            app.set_activity_months(ModelRc::new(VecModel::from(sessions.months)));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid day")
    }

    fn flat(grid: &Grid) -> Vec<&ActivityCell> {
        grid.weeks.iter().flatten().collect()
    }

    #[test]
    fn every_column_keeps_the_same_weekday_rows() {
        // A Monday anchor, so the trailing column is the current partial week.
        let grid = build_grid(&[], day(2026, 10, 12));
        assert_eq!(grid.weeks.len(), WEEK_COLUMNS);
        assert!(grid.weeks.iter().all(|week| week.len() == WEEK_DAYS));
        assert_eq!(flat(&grid).len(), WEEK_COLUMNS * WEEK_DAYS);
        let last = grid.weeks.last().expect("last column");
        assert!(!last[0].future);
        assert!(last[2].future);
        // Only the current week carries placeholders: Tuesday..Saturday.
        assert_eq!(flat(&grid).iter().filter(|cell| cell.future).count(), 5);
    }

    #[test]
    fn buckets_only_light_their_own_day() {
        let points = vec![(day(2026, 10, 12), 4), (day(2026, 10, 11), 2)];
        let grid = build_grid(&points, day(2026, 10, 12));
        assert_eq!(flat(&grid).iter().filter(|cell| cell.level > 0).count(), 2);
    }

    #[test]
    fn levels_step_from_the_rendered_peak() {
        assert_eq!(level_of(0, 10), 0);
        assert_eq!(level_of(10, 10), 4);
        assert_eq!(level_of(7, 10), 3);
        assert_eq!(level_of(4, 10), 2);
        assert_eq!(level_of(1, 10), 1);
        assert_eq!(level_of(3, 0), 0);
    }

    #[test]
    fn month_labels_need_a_full_week_and_a_gap() {
        let grid = build_grid(&[], day(2026, 10, 12));
        assert!(grid.months.len() >= 10, "a year spans months");
        let mut previous: Option<usize> = None;
        for label in &grid.months {
            let column = label.column as usize;
            if let Some(previous) = previous {
                assert!(column - previous >= MIN_LABEL_GAP, "{column} too close");
            }
            // A name needs its own columns of room, so the closing weeks of the
            // window stay unnamed instead of running past the card edge.
            assert!(column + 4 <= WEEK_COLUMNS, "{column} overflows the card");
            assert!((1..=12).contains(&(label.month as u32)));
            previous = Some(column);
        }
        let named: Vec<i32> = grid.months.iter().map(|label| label.month).collect();
        assert_eq!(named.first(), Some(&10), "the window opens on October");
        let unique: std::collections::HashSet<i32> = named.iter().copied().collect();
        assert_eq!(
            unique.len(),
            named.len(),
            "no month is named twice: {named:?}"
        );
    }

    #[test]
    fn out_of_window_days_never_shift_the_ramp() {
        // An entry far before the window is dropped instead of widening the
        // grid or stealing the darkest shade.
        let points = vec![(day(2019, 1, 1), 500), (day(2026, 10, 12), 5)];
        let grid = build_grid(&points, day(2026, 10, 12));
        assert_eq!(flat(&grid).iter().filter(|cell| cell.level == 4).count(), 1);
    }

    #[test]
    fn series_reader_skips_unusable_rows() {
        let value = serde_json::json!([
            { "date": "2026-10-12", "value": 3 },
            { "date": "2026-02-30", "value": 9 },
            { "date": "", "value": 4 },
            { "date": "2026-10-11", "value": 0 },
            { "value": 7 }
        ]);
        assert_eq!(parse_series(&value), vec![(day(2026, 10, 12), 3)]);
        assert!(parse_series(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn day_keys_are_strict_so_a_bad_anchor_cannot_move_the_grid() {
        assert_eq!(parse_day("2026-10-12"), Some(day(2026, 10, 12)));
        assert_eq!(parse_day("2026-13-01"), None);
        assert_eq!(parse_day("2026-02-30"), None);
        assert_eq!(parse_day("not-a-day"), None);
    }
}
