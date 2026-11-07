//! One place that decides which thread status a row may show.
//!
//! Two things live here. The first is the fencing between a status this process
//! authored and one it read back: a stale read must never reopen a thread the
//! local owner just settled. The second is settled freshness — a thread that
//! reached a terminal keeps its mark only while the mark is news. The durable
//! catalogue keeps answering the same terminal (`completed`, `cancelled`,
//! `failed`) for the rest of the thread's life, so without this every restart,
//! every poll and every pet projection re-announced an outcome that ended
//! yesterday — and a freshly selected thread flashed a stop square that had
//! nothing to do with what it is doing now.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::runtime_phase::Phase;

/// How long a settled mark stays once this process watched the thread settle.
/// A terminal is news, not a state: after a beat the row settles back to idle,
/// the sidebar stops claiming an outcome nobody is waiting for, and the desktop
/// pet stops reacting. This covers every settled phase alike — a completion, a
/// failure, and a user stop.
pub(super) const TERMINAL_HOLD: Duration = Duration::from_secs(6);

/// What a settled thread is allowed to show right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Completion {
    /// Not a settled thread.
    None,
    /// Watched here: the mark may stay until this instant.
    Until(Instant),
    /// Already settled when this process first read it — older news, so the
    /// row opens idle and a pet that never watched the run never reacts to it.
    History,
}

#[derive(Default)]
pub(super) struct StatusProjection {
    revision: u64,
    sessions: HashMap<String, SessionStatus>,
}

struct SessionStatus {
    revision: u64,
    status: String,
    local: bool,
    completion: Completion,
}

/// The verdict for one row: the phrase it should carry, whether this read was
/// adopted from the catalogue, and whether its settled mark has aged out.
pub(super) struct Reconciled {
    pub(super) status: String,
    pub(super) accepted: bool,
    /// True when the row must be presented as idle instead: the thread reached
    /// a terminal, but that terminal is no longer news.
    pub(super) retired: bool,
}

impl StatusProjection {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn record_local(&mut self, session: &str, status: &str, now: Instant) {
        self.revision = self.revision.saturating_add(1);
        let revision = self.revision;
        let entry = self
            .sessions
            .entry(session.to_string())
            .or_insert_with(|| SessionStatus {
                revision: 0,
                status: String::new(),
                local: false,
                completion: Completion::None,
            });
        entry.revision = revision;
        entry.local = true;
        note(entry, status, now);
    }

    pub(super) fn reconcile(
        &mut self,
        session: &str,
        status: &str,
        read_revision: u64,
        local_status: Option<&str>,
        now: Instant,
    ) -> Reconciled {
        let (status, accepted) = if let Some(status) = local_status {
            (status.to_string(), false)
        } else if let Some(current) = self.sessions.get(session) {
            if current.revision > read_revision {
                (current.status.clone(), false)
            } else if current.local
                && !catalogue_matches_local(current.status.as_str(), status)
                && (Phase::of(current.status.as_str()).terminal() || !Phase::of(status).terminal())
            {
                (current.status.clone(), false)
            } else {
                (status.to_string(), true)
            }
        } else {
            (status.to_string(), true)
        };
        let current = self
            .sessions
            .entry(session.to_string())
            .or_insert_with(|| SessionStatus {
                revision: 0,
                status: String::new(),
                local: false,
                completion: Completion::None,
            });
        if accepted {
            // An adopted catalogue read supersedes the local authorship: the
            // fence is a guard around a live local claim, not a permanent brand.
            // Without this a locally settled mark keeps every later catalogue
            // read out of the session for good.
            current.local = false;
            // A catalogue read is history unless this process watched the
            // thread become busy first; `note` makes that call.
            note(current, status.as_str(), now);
        }
        Reconciled {
            retired: retired(current, now),
            status,
            accepted,
        }
    }

    /// Whether this session's completion mark has aged out. Callers present an
    /// idle row when it returns true for a row that still shows the finish.
    pub(super) fn should_retire(&self, session: &str, now: Instant) -> bool {
        self.sessions
            .get(session)
            .is_some_and(|entry| retired(entry, now))
    }

    /// Drop everything this client knows about one session's status.
    ///
    /// A retired completion must not fence the thread: keeping a local "idle"
    /// record would make the next run start from another client invisible here,
    /// because a local terminal outranks every catalogue read.
    pub(super) fn forget(&mut self, session: &str) {
        self.sessions.remove(session);
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.sessions.retain(|session, _| keep(session));
    }
}

/// Fold one observed status into a session's settled bookkeeping.
fn note(entry: &mut SessionStatus, status: &str, now: Instant) {
    let phase = Phase::of(status);
    entry.completion = if phase.terminal() {
        match entry.completion {
            // A second report of the same terminal never extends the hold, and
            // never re-arms a mark that already aged out.
            Completion::Until(until) => Completion::Until(until),
            // Only a thread this process watched go busy counts as fresh news;
            // anything else arrived settled, so it is history.
            _ if Phase::of(entry.status.as_str()).live() => Completion::Until(now + TERMINAL_HOLD),
            _ => Completion::History,
        }
    } else {
        Completion::None
    };
    entry.status = status.to_string();
}

fn retired(entry: &SessionStatus, now: Instant) -> bool {
    // A settled mark is news: once it is no longer fresh (or never was) the row
    // presents idle again. Every terminal phase is covered — a completion that
    // ended yesterday must not sit on the row forever, and neither must a stop
    // square or a failure triangle that a fresh catalogue read resurfaces.
    if !Phase::of(entry.status.as_str()).terminal() {
        return false;
    }
    match entry.completion {
        Completion::Until(until) => until <= now,
        // Nothing to hold: the terminal was already old news on arrival.
        Completion::History | Completion::None => true,
    }
}

fn catalogue_matches_local(local_status: &str, catalogue_status: &str) -> bool {
    match status_kind(local_status) {
        "done" => status_kind(catalogue_status) == "done",
        "other" => status_kind(catalogue_status) == "other",
        _ => status_kind(catalogue_status) == "hot",
    }
}

fn status_kind(status: &str) -> &'static str {
    if matches!(
        status,
        "pending" | "queued" | "waiting_input" | "waiting_user_input"
    ) || status.starts_with("正在排队")
        || status.starts_with("任务已排队")
        || status.starts_with("等待")
        || status.starts_with("请求中")
        || matches!(status, "running" | "streaming" | "executing")
        || status.starts_with("正在")
        || status.starts_with("目标")
    {
        "hot"
    } else if matches!(status, "done" | "finished" | "completed" | "success")
        || status.starts_with("任务完成")
        || status.starts_with("输出已结束")
    {
        "done"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn an_old_read_is_rejected_even_after_the_local_owner_has_settled() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "请求中", at);
        let read_revision = projection.revision();
        projection.record_local("fixture-session", "任务完成", at);
        assert_eq!(
            projection
                .reconcile("fixture-session", "running", read_revision, None, at)
                .status,
            "任务完成".to_string(),
        );
    }

    #[test]
    fn a_newer_local_status_does_not_fence_an_independent_thread() {
        let at = now();
        let mut projection = StatusProjection::default();
        let read_revision = projection.revision();
        projection.record_local("fixture-local", "请求中", at);
        let reconciled =
            projection.reconcile("fixture-background", "done", read_revision, None, at);
        assert_eq!(reconciled.status, "done");
        assert!(reconciled.accepted);
    }

    #[test]
    fn an_old_terminal_stays_fenced_for_the_entire_local_execution() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "请求中", at);
        let read_revision = projection.revision();
        for status in ["请求中", "正在生成…", "正在执行工具…", "已停止"] {
            assert_eq!(
                projection
                    .reconcile("fixture-session", "done", read_revision, Some(status), at)
                    .status,
                status.to_string(),
            );
        }
    }

    #[test]
    fn the_next_read_can_reconcile_after_local_settlement() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "任务完成", at);
        let read_revision = projection.revision();
        assert!(
            projection
                .reconcile("fixture-session", "done", read_revision, None, at)
                .accepted
        );
        projection.retain(|_| false);
        assert!(projection.sessions.is_empty());
    }

    #[test]
    fn a_read_started_after_local_completion_cannot_reopen_running_state() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "任务完成", at);
        let read_revision = projection.revision();
        assert_eq!(
            projection
                .reconcile("fixture-session", "running", read_revision, None, at)
                .status,
            "任务完成".to_string(),
        );
        assert!(
            projection
                .reconcile("fixture-session", "done", read_revision, None, at)
                .accepted
        );
    }

    #[test]
    fn a_failed_or_stopped_local_terminal_cannot_become_success() {
        let at = now();
        let mut projection = StatusProjection::default();
        for terminal in ["执行失败", "已停止"] {
            projection.record_local("fixture-session", terminal, at);
            let read_revision = projection.revision();
            assert_eq!(
                projection
                    .reconcile("fixture-session", "done", read_revision, None, at)
                    .status,
                terminal.to_string(),
            );
        }
    }

    /// The catalogue keeps answering "completed" for the rest of a thread's
    /// life. A thread this process never watched run is not news, so the row
    /// must open idle instead of celebrating a finish from last week.
    #[test]
    fn a_completion_this_process_never_watched_is_not_celebrated() {
        let at = now();
        let mut projection = StatusProjection::default();
        let reconciled = projection.reconcile("fixture-session", "completed", 0, None, at);
        assert!(reconciled.retired);
        assert!(projection.should_retire("fixture-session", at));
    }

    #[test]
    fn a_completion_this_process_watched_held_then_aged_out() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "正在生成…", at);
        projection.record_local("fixture-session", "任务完成", at);
        assert!(!projection.should_retire("fixture-session", at));
        assert!(!projection.should_retire("fixture-session", at + TERMINAL_HOLD / 2));
        assert!(projection.should_retire("fixture-session", at + TERMINAL_HOLD * 2));
        // A repeated report of the same finish neither extends the hold nor
        // re-arms the mark.
        projection.record_local("fixture-session", "任务完成", at + TERMINAL_HOLD * 3);
        assert!(projection.should_retire("fixture-session", at + TERMINAL_HOLD * 3));
    }

    #[test]
    fn a_new_run_clears_a_retired_completion() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "正在生成…", at);
        projection.record_local("fixture-session", "任务完成", at);
        assert!(projection.should_retire("fixture-session", at + TERMINAL_HOLD * 2));
        projection.record_local("fixture-session", "请求中", at + TERMINAL_HOLD * 3);
        assert!(!projection.should_retire("fixture-session", at + TERMINAL_HOLD * 3));
    }

    /// A retired completion must not fence the thread: the next run, started
    /// from any client, has to reach this row.
    #[test]
    fn forgetting_a_retired_completion_lets_the_next_run_through() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "正在生成…", at);
        projection.record_local("fixture-session", "任务完成", at);
        assert!(projection.should_retire("fixture-session", at + TERMINAL_HOLD * 2));
        projection.forget("fixture-session");
        let reconciled = projection.reconcile("fixture-session", "running", 0, None, at);
        assert_eq!(reconciled.status, "running");
        assert!(reconciled.accepted);
        assert!(!reconciled.retired);
    }

    /// The catalogue answers `cancelled` for the rest of a thread's life after a
    /// stop. A thread this process never watched run is not news, so merely
    /// selecting it must not flash the stop square.
    #[test]
    fn a_stop_this_process_never_watched_is_not_a_state() {
        let at = now();
        let mut projection = StatusProjection::default();
        let reconciled = projection.reconcile("fixture-session", "cancelled", 0, None, at);
        assert!(reconciled.retired, "a cold stop must present as idle");
        assert!(projection.should_retire("fixture-session", at));
    }

    /// A stop this process watched happen is news: it holds for a beat, then
    /// retires to idle exactly like a completion.
    #[test]
    fn a_stop_this_process_watched_held_then_aged_out() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "正在生成…", at);
        projection.record_local("fixture-session", "已停止", at);
        assert!(!projection.should_retire("fixture-session", at));
        assert!(!projection.should_retire("fixture-session", at + TERMINAL_HOLD / 2));
        assert!(projection.should_retire("fixture-session", at + TERMINAL_HOLD * 2));
        // A repeated report of the same stop neither extends the hold nor
        // re-arms a mark that already aged out.
        projection.record_local("fixture-session", "已停止", at + TERMINAL_HOLD * 3);
        assert!(projection.should_retire("fixture-session", at + TERMINAL_HOLD * 3));
    }

    /// The same rule holds for a failure: a durable `failed` read cold is
    /// history, not a red triangle that clings to the row for good.
    #[test]
    fn a_failure_this_process_never_watched_is_not_a_state() {
        let at = now();
        let mut projection = StatusProjection::default();
        let reconciled = projection.reconcile("fixture-session", "failed", 0, None, at);
        assert!(reconciled.retired, "a cold failure must present as idle");
    }

    /// A retired stop must not fence the thread: the next run's status has to
    /// reach the row again.
    #[test]
    fn a_new_run_clears_a_retired_stop() {
        let at = now();
        let mut projection = StatusProjection::default();
        assert!(
            projection
                .reconcile("fixture-session", "cancelled", 0, None, at)
                .retired
        );
        projection.record_local("fixture-session", "请求中", at);
        assert!(!projection.should_retire("fixture-session", at));
    }

    /// A run this process started locally and then stopped driving (the user
    /// switched away) can settle on the backend while it is not selected. The
    /// catalogue's terminal must still reach the row: a leftover local "hot"
    /// mark must not fence it for good, or the sidebar keeps claiming the old
    /// run and its completion never raises the toast until it is reopened.
    #[test]
    fn an_abandoned_local_run_does_not_fence_a_background_settle() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "请求中", at);
        let read_revision = projection.revision();
        // The owner has moved on: no local override for this session any more.
        let reconciled =
            projection.reconcile("fixture-session", "completed", read_revision, None, at);
        assert!(reconciled.accepted, "the background settle must land");
        assert_eq!(reconciled.status, "completed");
        assert!(
            !reconciled.retired,
            "a settle this process watched go hot is fresh news"
        );
    }

    /// Once a catalogue read is adopted the local authorship is spent, so a run
    /// later started by another client is not fenced by a settled local mark.
    #[test]
    fn an_adopted_read_lets_a_later_remote_run_through() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "任务完成", at);
        let read_revision = projection.revision();
        assert!(
            projection
                .reconcile("fixture-session", "completed", read_revision, None, at)
                .accepted
        );
        let reconciled =
            projection.reconcile("fixture-session", "running", read_revision, None, at);
        assert!(reconciled.accepted, "a new remote run must not be fenced");
        assert_eq!(reconciled.status, "running");
    }
}
