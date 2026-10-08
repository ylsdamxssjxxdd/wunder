//! Thread center state: tabs, grouping, selection and search over the thread catalog.
//!
//! Items are `ThreadSnapshot`s taken straight from the runtime catalog service, so this view
//! never re-infers a thread's state from a status string. Normalization has one home; the TUI
//! only layers the two facts it holds in-process (approvals this UI is holding, streams this UI
//! is watching) over the catalog value.

use wunder_server::{ThreadPendingReason, ThreadSnapshot, ThreadStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadStatusFilter {
    All,
    NeedsYou,
    Working,
    Ready,
    Inactive,
}

/// How rows are grouped. The list is a task list: parent lineage, status, and
/// the workspace the work belongs to. Agent identity is gone with multi-agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadGroupMode {
    Parent,
    Status,
    Workspace,
}

impl ThreadGroupMode {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Parent => Self::Status,
            Self::Status => Self::Workspace,
            Self::Workspace => Self::Parent,
        }
    }

    pub(crate) fn label(self, is_zh: bool) -> &'static str {
        match (self, is_zh) {
            (Self::Parent, true) => "父子谱系",
            (Self::Status, true) => "状态",
            (Self::Workspace, true) => "工作区",
            (Self::Parent, false) => "Parent",
            (Self::Status, false) => "Status",
            (Self::Workspace, false) => "Workspace",
        }
    }
}

impl ThreadStatusFilter {
    pub(crate) const ALL: [Self; 5] = [
        Self::All,
        Self::NeedsYou,
        Self::Working,
        Self::Ready,
        Self::Inactive,
    ];

    pub(crate) fn next(self, backwards: bool) -> Self {
        let current = Self::ALL.iter().position(|item| *item == self).unwrap_or(0);
        let index = if backwards {
            current.checked_sub(1).unwrap_or(Self::ALL.len() - 1)
        } else {
            (current + 1) % Self::ALL.len()
        };
        Self::ALL[index]
    }

    pub(crate) fn label(self, is_zh: bool) -> &'static str {
        match (self, is_zh) {
            (Self::All, true) => "全部",
            (Self::NeedsYou, true) => "待处理",
            (Self::Working, true) => "运行中",
            (Self::Ready, true) => "就绪",
            (Self::Inactive, true) => "已结束",
            (Self::All, false) => "All",
            (Self::NeedsYou, false) => "Needs you",
            (Self::Working, false) => "Working",
            (Self::Ready, false) => "Ready",
            (Self::Inactive, false) => "Inactive",
        }
    }

    pub(crate) fn matches(self, item: &ThreadSnapshot) -> bool {
        match self {
            Self::All => true,
            Self::Working => item.status == ThreadStatus::Working,
            Self::NeedsYou => item.status == ThreadStatus::NeedsYou,
            Self::Ready => item.status == ThreadStatus::Ready,
            // The fifth tab is everything that cannot be resumed into work: both terminal
            // outcomes, so a failed thread is never mistaken for an idle one.
            Self::Inactive => {
                matches!(item.status, ThreadStatus::Failed | ThreadStatus::Finished)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CommandCenterState {
    pub(crate) sessions: Vec<ThreadSnapshot>,
    pub(crate) selected: usize,
    pub(crate) filter: ThreadStatusFilter,
    pub(crate) search: String,
    pub(crate) searching: bool,
    pub(crate) help: bool,
    pub(crate) group: ThreadGroupMode,
}

impl CommandCenterState {
    pub(crate) fn new(sessions: Vec<ThreadSnapshot>, active_session_id: &str) -> Self {
        let selected = sessions
            .iter()
            .position(|item| item.session_id == active_session_id)
            .unwrap_or(0);
        Self {
            sessions,
            selected,
            filter: ThreadStatusFilter::All,
            search: String::new(),
            searching: false,
            help: false,
            group: ThreadGroupMode::Parent,
        }
    }

    pub(crate) fn visible_indices(&self) -> Vec<usize> {
        let query = self.search.trim().to_ascii_lowercase();
        let mut indices: Vec<usize> = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, item)| self.filter.matches(item))
            .filter(|(_, item)| {
                query.is_empty()
                    || searchable_text(item)
                        .to_ascii_lowercase()
                        .contains(query.as_str())
            })
            .map(|(index, _)| index)
            .collect();
        indices.sort_by_key(|index| group_key(self.group, &self.sessions[*index]));
        indices
    }

    pub(crate) fn reconcile_selection(&mut self) {
        let visible = self.visible_indices();
        if !visible.contains(&self.selected) {
            self.selected = visible.first().copied().unwrap_or(0);
        }
    }

    pub(crate) fn selected_session_id(&self) -> Option<String> {
        self.visible_indices()
            .contains(&self.selected)
            .then(|| self.sessions.get(self.selected))
            .flatten()
            .map(|item| item.session_id.clone())
    }

    pub(crate) fn move_selection(&mut self, step: isize) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.selected = 0;
            return;
        }
        let current = visible
            .iter()
            .position(|index| *index == self.selected)
            .unwrap_or(0);
        let next = if step < 0 {
            current.saturating_sub(step.unsigned_abs())
        } else {
            current.saturating_add(step as usize).min(visible.len() - 1)
        };
        self.selected = visible[next];
    }

    pub(crate) fn filter_count(&self, filter: ThreadStatusFilter) -> usize {
        self.sessions
            .iter()
            .filter(|item| filter.matches(item))
            .count()
    }
}

fn group_key(mode: ThreadGroupMode, item: &ThreadSnapshot) -> String {
    match mode {
        ThreadGroupMode::Parent => item
            .parent_session_id
            .clone()
            .unwrap_or_else(|| "root".to_string()),
        ThreadGroupMode::Status => item.status.as_str().to_string(),
        // Threads of one workspace stay together; a legacy row without a
        // workspace sorts last instead of being hidden.
        ThreadGroupMode::Workspace => item
            .workspace_id
            .clone()
            .unwrap_or_else(|| "~unassigned".to_string()),
    }
}

/// The workspace cell of a row: the name the user gave the folder, falling back
/// to the id for a workspace the registry no longer knows.
pub(crate) fn workspace_cell(item: &ThreadSnapshot) -> Option<String> {
    let name = item
        .workspace_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| item.workspace_id.clone())?;
    Some(name)
}

pub(crate) fn status_label(status: ThreadStatus, is_zh: bool) -> &'static str {
    match status {
        ThreadStatus::Working => {
            if is_zh {
                "运行中"
            } else {
                "Working"
            }
        }
        ThreadStatus::NeedsYou => {
            if is_zh {
                "待处理"
            } else {
                "Needs you"
            }
        }
        ThreadStatus::Ready => {
            if is_zh {
                "就绪"
            } else {
                "Ready"
            }
        }
        ThreadStatus::Failed => {
            if is_zh {
                "失败"
            } else {
                "Failed"
            }
        }
        ThreadStatus::Finished => {
            if is_zh {
                "已结束"
            } else {
                "Inactive"
            }
        }
    }
}

pub(crate) fn status_marker(status: ThreadStatus) -> char {
    match status {
        ThreadStatus::Working => '●',
        ThreadStatus::NeedsYou => '!',
        ThreadStatus::Ready => '○',
        ThreadStatus::Failed => '✗',
        ThreadStatus::Finished => '·',
    }
}

/// Names the blocker of a `needs_you` thread, taken from the catalog rather
/// than inferred from the pending-card list of whichever thread is on screen.
pub(crate) fn pending_reason_label(reason: ThreadPendingReason, is_zh: bool) -> &'static str {
    match (reason, is_zh) {
        (ThreadPendingReason::Approval, true) => "等待授权",
        (ThreadPendingReason::Approval, false) => "approval",
        (ThreadPendingReason::UserInput, true) => "等待回答",
        (ThreadPendingReason::UserInput, false) => "your input",
    }
}

fn searchable_text(item: &ThreadSnapshot) -> String {
    format!(
        "{} {} {} {} {} {} {}",
        item.session_id,
        item.title,
        item.status.as_str(),
        item.workspace_name.as_deref().unwrap_or_default(),
        item.workspace_id.as_deref().unwrap_or_default(),
        item.spawn_label.as_deref().unwrap_or_default(),
        item.parent_session_id.as_deref().unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory row with every durable field present, so a contract change
    /// cannot slip past this fixture unnoticed.
    fn item(id: &str, status: ThreadStatus) -> ThreadSnapshot {
        ThreadSnapshot {
            session_id: id.to_string(),
            title: format!("Task {id}"),
            status,
            agent_id: None,
            workspace_id: Some("ws_fixture".to_string()),
            workspace_name: Some("fixture".to_string()),
            workspace_color: Some("blue".to_string()),
            parent_session_id: None,
            spawn_label: None,
            spawned_by: None,
            updated_at: 0.0,
            last_message_at: 0.0,
            child_threads: 0,
            user_round: 0,
            turn_status: String::new(),
            last_activity: None,
            context_tokens: 0,
            consumed_tokens: 0,
            tool_calls: 0,
            model_rounds: None,
            change_seq: 0,
            pending_reason: None,
        }
    }

    #[test]
    fn needs_you_rows_name_the_blocker_the_catalog_reported() {
        let mut waiting = item("waiting", ThreadStatus::NeedsYou);
        waiting.pending_reason = Some(ThreadPendingReason::Approval);
        let reason = waiting.pending_reason.expect("catalog reports the blocker");
        assert_eq!(pending_reason_label(reason, false), "approval");
        assert_eq!(pending_reason_label(reason, true), "等待授权");
        // No blocker reported for a thread that is not waiting on the user.
        assert_eq!(item("running", ThreadStatus::Working).pending_reason, None);
    }

    #[test]
    fn tabs_read_the_catalog_status_and_never_guess_one() {
        let ready = item("selected", ThreadStatus::Ready);
        assert!(!ThreadStatusFilter::Working.matches(&ready));
        assert!(ThreadStatusFilter::Ready.matches(&ready));
        assert_eq!(status_label(ready.status, false), "Ready");
        assert_eq!(status_marker(ThreadStatus::NeedsYou), '!');
        // A failed thread is terminal, not merely idle.
        let failed = item("broken", ThreadStatus::Failed);
        assert!(ThreadStatusFilter::Inactive.matches(&failed));
        assert!(!ThreadStatusFilter::Ready.matches(&failed));
        assert_eq!(status_label(failed.status, false), "Failed");
        assert_eq!(status_marker(failed.status), '✗');
    }

    #[test]
    fn filter_and_search_keep_the_selected_thread_in_view() {
        let mut state = CommandCenterState::new(
            vec![
                item("finished", ThreadStatus::Finished),
                item("worker", ThreadStatus::Working),
            ],
            "other",
        );
        state.filter = ThreadStatusFilter::Working;
        state.reconcile_selection();
        assert_eq!(state.selected_session_id().as_deref(), Some("worker"));
        state.search = "worker".to_string();
        assert_eq!(state.visible_indices(), vec![1]);
    }

    #[test]
    fn counts_cover_every_tab_of_the_loaded_page() {
        let state = CommandCenterState::new(
            vec![
                item("a", ThreadStatus::Working),
                item("b", ThreadStatus::NeedsYou),
                item("c", ThreadStatus::Ready),
                item("d", ThreadStatus::Failed),
                item("e", ThreadStatus::Finished),
            ],
            "none",
        );
        assert_eq!(state.filter_count(ThreadStatusFilter::All), 5);
        assert_eq!(state.filter_count(ThreadStatusFilter::Working), 1);
        assert_eq!(state.filter_count(ThreadStatusFilter::NeedsYou), 1);
        assert_eq!(state.filter_count(ThreadStatusFilter::Ready), 1);
        assert_eq!(state.filter_count(ThreadStatusFilter::Inactive), 2);
    }

    #[test]
    fn grouping_and_search_read_the_normalized_projection() {
        let mut child = item("child-one", ThreadStatus::Working);
        child.parent_session_id = Some("root-one".to_string());
        let mut state =
            CommandCenterState::new(vec![child, item("root-one", ThreadStatus::Ready)], "none");
        state.group = ThreadGroupMode::Status;
        assert_eq!(
            state.visible_indices(),
            vec![1, 0],
            "ready groups before working"
        );
        state.group = ThreadGroupMode::Parent;
        assert_eq!(
            state.visible_indices(),
            vec![1, 0],
            "a root sorts before its own child"
        );
        state.search = "CHILD-ONE".to_string();
        assert_eq!(state.visible_indices(), vec![0]);
    }

    #[test]
    fn workspace_grouping_replaces_the_agent_grouping() {
        // Three labels, and the third is the workspace: agent identity is gone.
        assert_eq!(ThreadGroupMode::Parent.next(), ThreadGroupMode::Status);
        assert_eq!(ThreadGroupMode::Status.next(), ThreadGroupMode::Workspace);
        assert_eq!(ThreadGroupMode::Workspace.next(), ThreadGroupMode::Parent);
        assert_eq!(ThreadGroupMode::Workspace.label(true), "工作区");

        let mut alpha = item("alpha-one", ThreadStatus::Ready);
        alpha.workspace_id = Some("ws_alpha".to_string());
        alpha.workspace_name = Some("alpha".to_string());
        let mut beta = item("beta-one", ThreadStatus::Ready);
        beta.workspace_id = Some("ws_beta".to_string());
        beta.workspace_name = Some("beta".to_string());
        let mut legacy = item("legacy", ThreadStatus::Ready);
        legacy.workspace_id = None;
        legacy.workspace_name = None;

        let state = CommandCenterState::new(vec![beta, alpha, legacy], "none");
        let state = {
            let mut state = state;
            state.group = ThreadGroupMode::Workspace;
            state
        };
        // `ws_alpha` < `ws_beta` < the unassigned marker, so one workspace's
        // threads stay together instead of interleaving by recency.
        let ordered: Vec<&str> = state
            .visible_indices()
            .into_iter()
            .map(|index| state.sessions[index].session_id.as_str())
            .collect();
        assert_eq!(ordered, vec!["alpha-one", "beta-one", "legacy"]);

        // The row cell names the workspace and falls back to the id.
        assert_eq!(workspace_cell(&state.sessions[1]).as_deref(), Some("alpha"));
        let mut unnamed = item("unnamed", ThreadStatus::Ready);
        unnamed.workspace_name = None;
        assert_eq!(workspace_cell(&unnamed).as_deref(), Some("ws_fixture"));
        assert_eq!(workspace_cell(&state.sessions[2]), None);
    }
}
