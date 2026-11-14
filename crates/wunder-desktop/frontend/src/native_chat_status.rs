//! One place that fences a status this process authored from a stale read.
//!
//! A status this process wrote outranks a catalogue read that started before
//! it; otherwise a late poll could reopen a thread the local owner just
//! settled. Freshness ("is this outcome still news?") deliberately does not
//! live here: the toast and the companion own that short-lived signal, while
//! the thread row keeps its durable outcome until the next turn starts.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime_phase::Phase;

#[derive(Default)]
pub(super) struct StatusProjection {
    revision: u64,
    sessions: HashMap<String, SessionStatus>,
}

struct SessionStatus {
    revision: u64,
    status: String,
    local: bool,
}

/// The verdict for one row: the phrase it should carry and whether this read
/// was adopted from the catalogue.
pub(super) struct Reconciled {
    pub(super) status: String,
    pub(super) accepted: bool,
}

impl StatusProjection {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn record_local(&mut self, session: &str, status: &str) {
        self.revision = self.revision.saturating_add(1);
        let revision = self.revision;
        let entry = self.entry(session);
        entry.revision = revision;
        entry.local = true;
        entry.status = status.to_string();
    }

    #[cfg(test)]
    pub(super) fn reconcile(
        &mut self,
        session: &str,
        status: &str,
        read_revision: u64,
        local_status: Option<&str>,
    ) -> Reconciled {
        self.reconcile_settled(session, status, read_revision, local_status)
    }

    /// Reconcile one catalogue read. A terminal read is a durable outcome, not
    /// news: no matter how old it is, the row keeps it. Only a live transition
    /// or a newer local claim can replace it.
    pub(super) fn reconcile_settled(
        &mut self,
        session: &str,
        status: &str,
        read_revision: u64,
        local_status: Option<&str>,
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
        if accepted {
            // An adopted read supersedes the local authorship: the fence is a
            // guard around a live local claim, not a permanent brand.
            let current = self.entry(session);
            current.local = false;
            current.status = status.clone();
        }
        Reconciled { status, accepted }
    }

    /// Drop everything this client knows about one session's status. Releasing
    /// the fence lets the next run — from this client or another one — reach
    /// the row. It does not change the row's durable outcome.
    pub(super) fn forget(&mut self, session: &str) {
        self.sessions.remove(session);
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.sessions.retain(|session, _| keep(session));
    }

    fn entry(&mut self, session: &str) -> &mut SessionStatus {
        self.sessions
            .entry(session.to_string())
            .or_insert_with(|| SessionStatus {
                revision: 0,
                status: String::new(),
                local: false,
            })
    }
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

/// The unix second a status change happened, for a row that reports a settle
/// locally. A local report is the moment the turn settled, so callers stamp it
/// with this instead of leaving the settle time unknown.
pub(crate) fn settled_now() -> f64 {
    unix_now()
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

    #[test]
    fn an_old_read_is_rejected_even_after_the_local_owner_has_settled() {
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "请求中");
        let read_revision = projection.revision();
        projection.record_local("fixture-session", "任务完成");
        assert_eq!(
            projection
                .reconcile("fixture-session", "running", read_revision, None)
                .status,
            "任务完成".to_string(),
        );
    }

    #[test]
    fn a_newer_local_status_does_not_fence_an_independent_thread() {
        let mut projection = StatusProjection::default();
        let read_revision = projection.revision();
        projection.record_local("fixture-local", "请求中");
        let reconciled = projection.reconcile("fixture-background", "done", read_revision, None);
        assert_eq!(reconciled.status, "done");
        assert!(reconciled.accepted);
    }

    #[test]
    fn an_old_terminal_stays_fenced_for_the_entire_local_execution() {
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "请求中");
        let read_revision = projection.revision();
        for status in ["请求中", "正在生成…", "正在执行工具…", "已停止"] {
            assert_eq!(
                projection
                    .reconcile("fixture-session", "done", read_revision, Some(status))
                    .status,
                status.to_string(),
            );
        }
    }

    #[test]
    fn the_next_read_can_reconcile_after_local_settlement() {
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "任务完成");
        let read_revision = projection.revision();
        assert!(
            projection
                .reconcile("fixture-session", "done", read_revision, None)
                .accepted
        );
        projection.retain(|_| false);
        assert!(projection.sessions.is_empty());
    }

    #[test]
    fn a_read_started_after_local_completion_cannot_reopen_running_state() {
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "任务完成");
        let read_revision = projection.revision();
        assert_eq!(
            projection
                .reconcile("fixture-session", "running", read_revision, None)
                .status,
            "任务完成".to_string(),
        );
        assert!(
            projection
                .reconcile("fixture-session", "done", read_revision, None)
                .accepted
        );
    }

    #[test]
    fn a_failed_or_stopped_local_terminal_cannot_become_success() {
        let mut projection = StatusProjection::default();
        for terminal in ["执行失败", "已停止"] {
            projection.record_local("fixture-session", terminal);
            let read_revision = projection.revision();
            assert_eq!(
                projection
                    .reconcile("fixture-session", "done", read_revision, None)
                    .status,
                terminal.to_string(),
            );
        }
    }

    /// The catalogue keeps answering "completed" for the rest of a thread's
    /// life. Time must never demote that outcome back to idle: freshness is a
    /// notification concern, not a row-state concern.
    #[test]
    fn a_catalogue_terminal_stays_the_row_state_however_old_it_is() {
        let mut projection = StatusProjection::default();
        let first = projection.reconcile_settled("fixture-session", "completed", 0, None);
        assert_eq!(first.status, "completed");
        assert!(first.accepted);
        let repeated = projection.reconcile_settled("fixture-session", "completed", 0, None);
        assert_eq!(repeated.status, "completed");
        let stopped = projection.reconcile_settled("fixture-session", "cancelled", 0, None);
        assert_eq!(stopped.status, "cancelled", "a real new outcome still wins");
    }

    #[test]
    fn forgetting_releases_the_fence_without_changing_the_row() {
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "任务完成");
        projection.forget("fixture-session");
        let reconciled = projection.reconcile("fixture-session", "running", 0, None);
        assert_eq!(reconciled.status, "running");
        assert!(reconciled.accepted);
    }

    /// A run this process started and then stopped driving can settle on the
    /// backend while it is not selected. Its terminal must still reach the row.
    #[test]
    fn an_abandoned_local_run_does_not_fence_a_background_settle() {
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "请求中");
        let read_revision = projection.revision();
        let reconciled =
            projection.reconcile_settled("fixture-session", "completed", read_revision, None);
        assert!(reconciled.accepted, "the background settle must land");
        assert_eq!(reconciled.status, "completed");
    }

    #[test]
    fn an_adopted_read_lets_a_later_remote_run_through() {
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "任务完成");
        let read_revision = projection.revision();
        assert!(
            projection
                .reconcile("fixture-session", "completed", read_revision, None)
                .accepted
        );
        let reconciled = projection.reconcile("fixture-session", "running", read_revision, None);
        assert!(reconciled.accepted, "a new remote run must not be fenced");
        assert_eq!(reconciled.status, "running");
    }
}
