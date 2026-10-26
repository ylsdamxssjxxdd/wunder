use std::collections::HashMap;

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

impl StatusProjection {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn record_local(&mut self, session: &str, status: &str) {
        self.revision = self.revision.saturating_add(1);
        self.sessions.insert(
            session.to_string(),
            SessionStatus {
                revision: self.revision,
                status: status.to_string(),
                local: true,
            },
        );
    }

    pub(super) fn reconcile(
        &mut self,
        session: &str,
        status: &str,
        read_revision: u64,
        local_status: Option<&str>,
    ) -> (String, bool) {
        if let Some(status) = local_status {
            return (status.to_string(), false);
        }
        if let Some(current) = self.sessions.get(session) {
            if current.revision > read_revision {
                return (current.status.clone(), false);
            }
            if current.local && !catalogue_matches_local(current.status.as_str(), status) {
                return (current.status.clone(), false);
            }
        }
        let current = self
            .sessions
            .entry(session.to_string())
            .or_insert_with(|| SessionStatus {
                revision: 0,
                status: String::new(),
                local: false,
            });
        current.status = status.to_string();
        current.local = false;
        (current.status.clone(), true)
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.sessions.retain(|session, _| keep(session));
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

    #[test]
    fn an_old_read_is_rejected_even_after_the_local_owner_has_settled() {
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "请求中");
        let read_revision = projection.revision();
        projection.record_local("fixture-session", "任务完成");
        assert_eq!(
            projection.reconcile("fixture-session", "running", read_revision, None),
            ("任务完成".into(), false),
        );
    }

    #[test]
    fn a_newer_local_status_does_not_fence_an_independent_thread() {
        let mut projection = StatusProjection::default();
        let read_revision = projection.revision();
        projection.record_local("fixture-local", "请求中");
        assert_eq!(
            projection.reconcile("fixture-background", "done", read_revision, None),
            ("done".into(), true),
        );
    }

    #[test]
    fn an_old_terminal_stays_fenced_for_the_entire_local_execution() {
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "请求中");
        let read_revision = projection.revision();
        for status in ["请求中", "正在生成…", "正在执行工具…", "已停止"] {
            assert_eq!(
                projection.reconcile("fixture-session", "done", read_revision, Some(status)),
                (status.into(), false),
            );
        }
    }

    #[test]
    fn the_next_read_can_reconcile_after_local_settlement() {
        let mut projection = StatusProjection::default();
        projection.record_local("fixture-session", "任务完成");
        let read_revision = projection.revision();
        assert_eq!(
            projection.reconcile("fixture-session", "done", read_revision, None),
            ("done".into(), true),
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
            projection.reconcile("fixture-session", "running", read_revision, None),
            ("任务完成".into(), false),
        );
        assert_eq!(
            projection.reconcile("fixture-session", "done", read_revision, None),
            ("done".into(), true),
        );
    }

    #[test]
    fn a_failed_or_stopped_local_terminal_cannot_become_success() {
        let mut projection = StatusProjection::default();
        for terminal in ["执行失败", "已停止"] {
            projection.record_local("fixture-session", terminal);
            let read_revision = projection.revision();
            assert_eq!(
                projection.reconcile("fixture-session", "done", read_revision, None),
                (terminal.into(), false),
            );
        }
    }
}
