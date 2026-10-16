//! Bounded trajectory reader, sharing the runtime projection and body cursors.
#[path = "trajectory/actions.rs"]
mod actions;
#[path = "trajectory/view.rs"]
mod view;

use super::TuiApp;
use crate::trajectory::Detail;
use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
pub(crate) use view::draw;
use wunder_server::stable_core::thread_view::{ThreadViewPage, TrajectoryTile, TrajectoryViewRow};

const PAGE: usize = 80;
const MAX_ROWS: usize = 600;
const REFRESH: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Latest,
    Older,
    Newer,
    Refresh,
    Reload,
}
enum Reply {
    Page(Direction, ThreadViewPage),
    Search(String, Vec<TrajectoryViewRow>, bool),
    Detail(String, i64, Detail),
    Export(std::path::PathBuf),
}
struct Pending {
    receiver: oneshot::Receiver<anyhow::Result<Reply>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) struct TrajectoryView {
    session: String,
    root: Option<String>,
    rows: Vec<TrajectoryViewRow>,
    tiles: Vec<TrajectoryTile>,
    tile_level: usize,
    selected: usize,
    older: bool,
    newer: bool,
    epoch: Option<i64>,
    latest_round: i64,
    projection: String,
    diagnostic: Option<String>,
    pending: Option<Pending>,
    error: Option<String>,
    notice: String,
    refreshed: Instant,
    live: bool,
    search: String,
    searching: bool,
    hits: Option<Vec<TrajectoryViewRow>>,
    more_hits: bool,
    duration: bool,
    zoom: u8,
    fold_turns: bool,
    fold_calls: bool,
    folded: HashSet<String>,
    return_anchor: Option<i64>,
    detail: Detail,
    detail_key: Option<(String, i64)>,
    tab: usize,
    detail_focus: bool,
    detail_scroll: usize,
    detail_x: usize,
    detail_height: usize,
    ledger_area: Rect,
    detail_area: Rect,
    timeline_area: Rect,
    ledger_offset: usize,
    timeline_range: (f64, f64),
    help: bool,
}
impl TrajectoryView {
    fn new(session: String) -> Self {
        Self {
            session,
            root: None,
            rows: vec![],
            tiles: vec![],
            tile_level: 0,
            selected: 0,
            older: false,
            newer: false,
            epoch: None,
            latest_round: 0,
            projection: String::new(),
            diagnostic: None,
            pending: None,
            error: None,
            notice: String::new(),
            refreshed: Instant::now(),
            live: true,
            search: String::new(),
            searching: false,
            hits: None,
            more_hits: false,
            duration: false,
            zoom: 0,
            fold_turns: false,
            fold_calls: false,
            folded: HashSet::new(),
            return_anchor: None,
            detail: Detail::default(),
            detail_key: None,
            tab: 0,
            detail_focus: false,
            detail_scroll: 0,
            detail_x: 0,
            detail_height: 0,
            ledger_area: Rect::default(),
            detail_area: Rect::default(),
            timeline_area: Rect::default(),
            ledger_offset: 0,
            timeline_range: (0., 1.),
            help: false,
        }
    }
    fn active_rows(&self) -> &[TrajectoryViewRow] {
        self.hits.as_deref().unwrap_or(&self.rows)
    }
    fn row(&self) -> Option<&TrajectoryViewRow> {
        self.active_rows().get(self.selected)
    }
    fn visible(&self) -> Vec<usize> {
        if self.hits.is_some() {
            return (0..self.active_rows().len()).collect();
        }
        let mut seen_turns = HashSet::new();
        let mut seen_calls = HashSet::new();
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                if (self.fold_turns || self.folded.contains(&r.root_turn_id))
                    && !seen_turns.insert(&r.root_turn_id)
                {
                    return None;
                }
                if self.fold_calls
                    && is_tool(&r.kind)
                    && !seen_calls.insert((&r.turn_id, r.model_round))
                {
                    return None;
                }
                Some(i)
            })
            .collect()
    }
    fn reconcile(&mut self) {
        let ids = self.visible();
        if !ids.contains(&self.selected) {
            let root = self.row().map(|r| r.root_turn_id.as_str());
            self.selected = ids
                .iter()
                .copied()
                .find(|i| Some(self.active_rows()[*i].root_turn_id.as_str()) == root)
                .or_else(|| ids.first().copied())
                .unwrap_or(0);
        }
    }
    fn move_by(&mut self, delta: isize) {
        let ids = self.visible();
        if ids.is_empty() {
            return;
        }
        let at = ids.iter().position(|i| *i == self.selected).unwrap_or(0);
        self.selected = ids[(at as isize + delta).clamp(0, ids.len() as isize - 1) as usize];
        self.detail_scroll = 0;
    }
    fn apply_page(&mut self, direction: Direction, mut page: ThreadViewPage) {
        let anchor = self.row().map(|r| r.item_id.clone());
        let changed_epoch = self.epoch.is_some_and(|e| e != page.epoch);
        self.refreshed = Instant::now();
        self.latest_round = page.latest_round;
        self.projection = page.projection_state;
        self.diagnostic = page.diagnostic;
        if page.degraded {
            return;
        } // keep committed rows during bounded-read fallback
        self.epoch = Some(page.epoch);
        self.tiles = page.tiles;
        self.tile_level = page.tile_level;
        if direction == Direction::Latest || changed_epoch {
            self.rows.clear();
            self.folded.clear();
            self.older = page.has_more;
            self.newer = page.has_newer;
        } else if direction == Direction::Older {
            self.older = page.has_more;
        } else if direction == Direction::Newer {
            self.newer = page.has_newer;
        } else if direction == Direction::Reload {
            // Refresh the current bounded page without moving either history edge.
        } else {
            self.newer = page.has_newer;
            if self.rows.is_empty() {
                self.older = page.has_more;
            }
        }
        let mut by_id: HashMap<String, TrajectoryViewRow> = self
            .rows
            .drain(..)
            .map(|r| (r.item_id.clone(), r))
            .collect();
        for row in page.trajectory.drain(..) {
            by_id.insert(row.item_id.clone(), row);
        }
        self.rows = by_id.into_values().collect();
        self.rows.sort_by_key(|r| r.created_seq);
        if self.rows.len() > MAX_ROWS {
            if direction == Direction::Older {
                self.rows.truncate(MAX_ROWS);
                self.newer = true;
            } else {
                self.rows.drain(..self.rows.len() - MAX_ROWS);
                self.older = true;
            }
        }
        self.folded
            .retain(|root| self.rows.iter().any(|r| &r.root_turn_id == root));
        self.selected = anchor
            .as_ref()
            .and_then(|id| self.rows.iter().position(|r| &r.item_id == id))
            .unwrap_or_else(|| {
                if direction == Direction::Older {
                    0
                } else {
                    self.rows.len().saturating_sub(1)
                }
            });
        if direction == Direction::Refresh && self.live {
            self.selected = self.rows.len().saturating_sub(1);
        }
        self.reconcile();
    }
    fn domain(&self) -> (f64, f64) {
        let mut start = f64::INFINITY;
        let mut end = f64::NEG_INFINITY;
        let size = (128_i64 << (self.tile_level.min(7) * 3)) as f64;
        if self.root.is_none() && self.hits.is_none() {
            for t in &self.tiles {
                let (a, b) = if self.duration {
                    (t.start, t.end)
                } else {
                    (t.bucket as f64 * size, (t.bucket as f64 + 1.) * size)
                };
                if a.is_finite() && b.is_finite() && (!self.duration || a > 0.) {
                    start = start.min(a);
                    end = end.max(b);
                }
            }
        }
        for r in self.active_rows() {
            let (a, b) = row_span(r, self.duration);
            if a.is_finite() && b.is_finite() {
                start = start.min(a);
                end = end.max(b);
            }
        }
        if !start.is_finite() {
            return (0., 1.);
        }
        end = end.max(start + if self.duration { 0.001 } else { 1. });
        if self.zoom > 0 {
            let width = (end - start) / 2_f64.powi(self.zoom as i32);
            let center = self
                .row()
                .map(|r| row_span(r, self.duration).0)
                .unwrap_or(start);
            start = (center - width / 2.).clamp(start, (end - width).max(start));
            end = start + width;
        }
        (start, end)
    }
    fn scroll_detail(&mut self, delta: isize) {
        self.detail_scroll = (self.detail_scroll as isize + delta).max(0) as usize;
        self.detail_scroll = self.detail_scroll.min(
            self.detail_height
                .saturating_sub(self.detail_area.height.saturating_sub(3) as usize),
        );
    }
}
fn lane(kind: &str) -> usize {
    match kind {
        "user_message" | "system_message" | "context" | "compaction" => 0,
        "assistant_message" | "model_call" => 1,
        _ => 2,
    }
}
fn is_tool(kind: &str) -> bool {
    matches!(kind, "tool_call" | "tool_result" | "subagent_run")
}
fn row_span(row: &TrajectoryViewRow, duration: bool) -> (f64, f64) {
    if duration {
        (
            row.started_at,
            row.completed_at
                .unwrap_or_else(|| {
                    if matches!(row.status.as_str(), "running" | "waiting_input") {
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs_f64()
                    } else {
                        row.started_at
                    }
                })
                .max(row.started_at),
        )
    } else {
        (row.created_seq as f64, row.created_seq as f64)
    }
}
fn coordinate(value: f64, range: (f64, f64), width: usize) -> usize {
    (((value - range.0) / (range.1 - range.0).max(f64::EPSILON)).clamp(0., 1.)
        * width.saturating_sub(1) as f64)
        .round() as usize
}

#[cfg(test)]
#[path = "trajectory/tests.rs"]
mod tests;
