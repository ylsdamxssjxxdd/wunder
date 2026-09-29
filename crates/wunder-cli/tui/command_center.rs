use crate::ResumeSessionSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadStatusFilter {
    All,
    NeedsYou,
    Working,
    Ready,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadGroupMode {
    Parent,
    Status,
    Agent,
}

impl ThreadGroupMode {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Parent => Self::Status,
            Self::Status => Self::Agent,
            Self::Agent => Self::Parent,
        }
    }

    pub(crate) fn label(self, is_zh: bool) -> &'static str {
        match (self, is_zh) {
            (Self::Parent, true) => "父子谱系",
            (Self::Status, true) => "状态",
            (Self::Agent, true) => "智能体",
            (Self::Parent, false) => "Parent",
            (Self::Status, false) => "Status",
            (Self::Agent, false) => "Agent",
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

    pub(crate) fn matches(self, item: &ResumeSessionSummary, _active_session_id: &str) -> bool {
        match self {
            Self::All => true,
            Self::Working => is_working(item),
            Self::NeedsYou => is_needs_you(item),
            Self::Ready => is_ready(item),
            Self::Inactive => !is_working(item) && !is_needs_you(item) && !is_ready(item),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CommandCenterState {
    pub(crate) sessions: Vec<ResumeSessionSummary>,
    pub(crate) selected: usize,
    pub(crate) filter: ThreadStatusFilter,
    pub(crate) search: String,
    pub(crate) searching: bool,
    pub(crate) help: bool,
    pub(crate) group: ThreadGroupMode,
}

impl CommandCenterState {
    pub(crate) fn new(sessions: Vec<ResumeSessionSummary>, active_session_id: &str) -> Self {
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

    pub(crate) fn visible_indices(&self, active_session_id: &str) -> Vec<usize> {
        let query = self.search.trim().to_ascii_lowercase();
        let mut indices: Vec<usize> = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, item)| self.filter.matches(item, active_session_id))
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

    pub(crate) fn reconcile_selection(&mut self, active_session_id: &str) {
        let visible = self.visible_indices(active_session_id);
        if !visible.contains(&self.selected) {
            self.selected = visible.first().copied().unwrap_or(0);
        }
    }

    pub(crate) fn selected_session_id(&self, active_session_id: &str) -> Option<String> {
        self.visible_indices(active_session_id)
            .contains(&self.selected)
            .then(|| self.sessions.get(self.selected))
            .flatten()
            .map(|item| item.session_id.clone())
    }

    pub(crate) fn move_selection(&mut self, active_session_id: &str, step: isize) {
        let visible = self.visible_indices(active_session_id);
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

    pub(crate) fn filter_count(
        &self,
        filter: ThreadStatusFilter,
        active_session_id: &str,
    ) -> usize {
        self.sessions
            .iter()
            .filter(|item| filter.matches(item, active_session_id))
            .count()
    }
}

fn group_key(mode: ThreadGroupMode, item: &ResumeSessionSummary) -> String {
    match mode {
        ThreadGroupMode::Parent => item
            .parent_session_id
            .clone()
            .unwrap_or_else(|| "root".to_string()),
        ThreadGroupMode::Status => normalized_status(item),
        ThreadGroupMode::Agent => item
            .agent_id
            .clone()
            .unwrap_or_else(|| "default".to_string()),
    }
}

pub(crate) fn status_label(
    item: &ResumeSessionSummary,
    _active_session_id: &str,
    is_zh: bool,
) -> &'static str {
    if is_working(item) {
        return if is_zh { "运行中" } else { "Working" };
    }
    if is_needs_you(item) {
        return if is_zh { "待处理" } else { "Needs you" };
    }
    if is_ready(item) {
        return if is_zh { "就绪" } else { "Ready" };
    }
    if normalized_status(item) == "failed" {
        return if is_zh { "失败" } else { "Failed" };
    }
    if is_zh {
        "已结束"
    } else {
        "Inactive"
    }
}

pub(crate) fn status_marker(item: &ResumeSessionSummary, _active_session_id: &str) -> char {
    if is_working(item) {
        '●'
    } else if is_needs_you(item) {
        '!'
    } else if is_ready(item) {
        '○'
    } else {
        '·'
    }
}

fn searchable_text(item: &ResumeSessionSummary) -> String {
    format!(
        "{} {} {} {} {} {}",
        item.session_id,
        item.title,
        item.status,
        item.agent_id.as_deref().unwrap_or_default(),
        item.parent_session_id.as_deref().unwrap_or_default(),
        item.spawn_label.as_deref().unwrap_or_default(),
    )
}

fn normalized_status(item: &ResumeSessionSummary) -> String {
    item.status.trim().to_ascii_lowercase()
}

fn is_working(item: &ResumeSessionSummary) -> bool {
    matches!(
        normalized_status(item).as_str(),
        "working" | "running" | "queued" | "active" | "pending"
    )
}

fn is_needs_you(item: &ResumeSessionSummary) -> bool {
    matches!(
        normalized_status(item).as_str(),
        "needs_you" | "needs_input" | "needs_approval" | "waiting_input" | "waiting_approval"
    )
}

fn is_ready(item: &ResumeSessionSummary) -> bool {
    matches!(
        normalized_status(item).as_str(),
        "ready" | "paused" | "idle"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, status: &str) -> ResumeSessionSummary {
        ResumeSessionSummary {
            session_id: id.to_string(),
            title: format!("Task {id}"),
            status: status.to_string(),
            agent_id: None,
            parent_session_id: None,
            spawn_label: None,
            spawned_by: None,
            updated_at: 0.0,
            last_message_at: 0.0,
            child_threads: 0,
        }
    }

    #[test]
    fn visible_thread_is_not_implicitly_working() {
        let ready = item("selected", "ready");
        assert!(!ThreadStatusFilter::Working.matches(&ready, "selected"));
        assert_eq!(status_label(&ready, "selected", false), "Ready");
        let waiting = item("selected", "needs_you");
        assert_eq!(status_marker(&waiting, "selected"), '!');
    }

    #[test]
    fn filter_and_search_keep_the_selected_thread_in_view() {
        let mut state = CommandCenterState::new(
            vec![item("finished", "completed"), item("worker", "running")],
            "other",
        );
        state.filter = ThreadStatusFilter::Working;
        state.reconcile_selection("other");
        assert_eq!(
            state.selected_session_id("other").as_deref(),
            Some("worker")
        );
        state.search = "worker".to_string();
        assert_eq!(state.visible_indices("other"), vec![1]);
    }

    #[test]
    fn catalog_status_names_are_filterable() {
        let mut state = CommandCenterState::new(
            vec![item("working", "working"), item("waiting", "needs_you")],
            "none",
        );
        state.filter = ThreadStatusFilter::Working;
        assert_eq!(state.visible_indices("none"), vec![0]);
        state.filter = ThreadStatusFilter::NeedsYou;
        assert_eq!(state.visible_indices("none"), vec![1]);
    }
}
