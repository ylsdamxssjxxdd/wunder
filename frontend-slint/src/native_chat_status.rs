//! One place that decides which thread status a row may show.
//!
//! Two things live here. The first is the fencing between a status this process
//! authored and one it read back: a stale read must never reopen a thread the
//! local owner just settled. The second is completion freshness — a finished
//! thread keeps its mark only while the mark is news. The durable catalogue
//! keeps saying "completed" for the rest of the thread's life, so without this
//! every restart, every poll and every pet projection re-celebrated a run that
//! ended yesterday.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::runtime_phase::Phase;

/// How long a completion mark stays once this process watched the thread
/// finish. A completion is news, not a state: after a beat the row settles
/// back to idle, the sidebar stops claiming a success nobody is waiting for,
/// and the desktop pet stops celebrating.
pub(super) const DONE_HOLD: Duration = Duration::from_secs(6);

/// What a finished thread is allowed to show right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Completion {
    /// Not a finished thread.
    None,
    /// Watched here: the mark may stay until this instant.
    Until(Instant),
    /// Already finished when this process first read it — older news, so the
    /// row opens idle and a pet that never watched the run never celebrates it.
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
/// adopted from the catalogue, and whether its completion mark has aged out.
pub(super) struct Reconciled {
    pub(super) status: String,
    pub(super) accepted: bool,
    /// True when the row must be presented as idle instead: the thread is
    /// finished, but the finish is no longer news.
    pub(super) retired: bool,
}

impl StatusProjection {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn record_local(&mut self, session: &str, status: &str, now: Instant) {
        self.revision = self.revision.saturating_add(1);
        let revision = self.revision;
        let entry = self.sessions.entry(session.to_string()).or_insert_with(|| {
            SessionStatus {
                revision: 0,
                status: String::new(),
                local: false,
                completion: Completion::None,
            }
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
            } else if current.local && !catalogue_matches_local(current.status.as_str(), status) {
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

/// Fold one observed status into a session's completion bookkeeping.
fn note(entry: &mut SessionStatus, status: &str, now: Instant) {
    let phase = Phase::of(status);
    entry.completion = if phase == Phase::Done {
        match entry.completion {
            // A second report of the same finish never extends the hold, and
            // never re-arms a mark that already aged out.
            Completion::Until(until) => Completion::Until(until),
            // Only a thread this process watched go busy counts as fresh news;
            // anything else arrived finished.
            _ if Phase::of(entry.status.as_str()).live() => Completion::Until(now + DONE_HOLD),
            _ => Completion::History,
        }
    } else {
        Completion::None
    };
    entry.status = status.to_string();
}

fn retired(entry: &SessionStatus, now: Instant) -> bool {
    if Phase::of(entry.status.as_str()) != Phase::Done {
        return false;
    }
    match entry.completion {
        Completion::Until(until) => until <= now,
        // Nothing to hold: the finish was already old news on arrival.
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
        let reconciled = projection.reconcile("fixture-background", "done", read_revision, None, at);
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
        assert!(projection
            .reconcile("fixture-session", "done", read_revision, None, at)
            .accepted);
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
        assert!(projection
            .reconcile("fixture-session", "done", read_revision, None, at)
            .accepted);
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
        assert!(!projection.should_retire("fixture-session", at + DONE_HOLD / 2));
        assert!(projection.should_retire("fixture-session", at + DONE_HOLD * 2));
        // A repeated report of the same finish neither extends the hold nor
        // re-arms the mark.
        projection.record_local("fixture-session", "任务完成", at + DONE_HOLD * 3);
        assert!(projection.should_retire("fixture-session", at + DONE_HOLD * 3));
    }

    #[test]
    fn a_new_run_clears_a_retired_completion() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "正在生成…", at);
        projection.record_local("fixture-session", "任务完成", at);
        assert!(projection.should_retire("fixture-session", at + DONE_HOLD * 2));
        projection.record_local("fixture-session", "请求中", at + DONE_HOLD * 3);
        assert!(!projection.should_retire("fixture-session", at + DONE_HOLD * 3));
    }

    /// A retired completion must not fence the thread: the next run, started
    /// from any client, has to reach this row.
    #[test]
    fn forgetting_a_retired_completion_lets_the_next_run_through() {
        let at = now();
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "正在生成…", at);
        projection.record_local("fixture-session", "任务完成", at);
        assert!(projection.should_retire("fixture-session", at + DONE_HOLD * 2));
        projection.forget("fixture-session");
        let reconciled = projection.reconcile("fixture-session", "running", 0, None, at);
        assert_eq!(reconciled.status, "running");
        assert!(reconciled.accepted);
        assert!(!reconciled.retired);
    }
}
