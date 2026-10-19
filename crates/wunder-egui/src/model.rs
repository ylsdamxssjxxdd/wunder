//! Small, bounded presentation model shared by native and web adapters.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub id: String,
    pub title: String,
    pub workspace: String,
    pub status: String,
    pub updated_at: f64,
    pub locked: bool,
    /// Explicit per-thread model pin. `None` means this thread follows the
    /// selected agent or the configured default model.
    pub model_key: Option<String>,
    /// The thread-scoped reasoning strength persisted by `NativeDesktop`.
    pub reasoning_effort: String,
    /// Current context occupancy, when the runtime can observe it. This is
    /// deliberately not the cumulative billing or output-token counter.
    pub context_tokens: Option<u32>,
    /// Current context window capacity reported by the runtime, when known.
    pub context_capacity_tokens: Option<u32>,
}

/// Bounded local workspace projection used by the sidebar file tree.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceFileView {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size: u64,
}

/// Small model projection used by the composer picker.
///
/// It intentionally omits connection data and generation configuration: the
/// picker needs a stable label, the thread-pinning key, and enough context
/// metadata to describe the active window without copying settings objects
/// into the immediate-mode tree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelOption {
    pub key: String,
    pub provider: String,
    pub model: String,
    pub is_default: bool,
    pub max_context_tokens: Option<u32>,
    pub reasoning_effort: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MessageView {
    pub id: String,
    pub text: String,
    pub mine: bool,
    pub state: String,
}

/// The source contract determines the row type; labels never determine it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowKind {
    Tool,
    Reasoning,
    Text,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkflowView {
    pub id: String,
    /// Durable process item cursor when this row came from a paged native
    /// read. Live-only rows leave it unset; the reducer uses it to keep
    /// pagination reachable after the presentation cap trims expanded rows.
    #[serde(default)]
    pub storage_index: Option<i64>,
    #[serde(default)]
    pub kind: WorkflowKind,
    /// Runtime tool identifier, separate from the human-readable title.
    #[serde(default)]
    pub tool_name: String,
    pub title: String,
    pub preview: String,
    pub state: String,
    /// At most four invocation targets; results never supply call targets.
    #[serde(default)]
    pub targets: Vec<String>,
    /// Bounded display text only. Empty means no detail was supplied.
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub detail_truncated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TurnView {
    pub id: String,
    pub user: MessageView,
    pub assistant: MessageView,
    /// The authoritative number of process steps known by the lightweight
    /// thread header.  The entries themselves stay unloaded until the user
    /// opens this turn's work-process disclosure.
    #[serde(default)]
    pub process_steps: usize,
    /// The currently displayed bounded process page.  During a live turn it
    /// can also hold the active reasoning/tool tail.
    pub workflow: Vec<WorkflowView>,
    /// `true` after the work-process page has been read, including a valid
    /// empty page.  This prevents an empty completed turn from continuously
    /// re-reading local storage every egui frame.
    #[serde(default)]
    pub process_loaded: bool,
    #[serde(default)]
    pub process_loading: bool,
    #[serde(default)]
    pub process_error: String,
    /// Exclusive keyset cursors supplied by `NativeDesktop::chat_process_page`.
    #[serde(default)]
    pub process_first: Option<i64>,
    #[serde(default)]
    pub process_last: Option<i64>,
    #[serde(default)]
    pub process_has_older: bool,
    #[serde(default)]
    pub process_has_newer: bool,
}

/// A user turn always keeps a work-process disclosure, even when storage has
/// no reasoning or tool records for it.  That makes an empty process explicit
/// instead of making the chat layout jump between otherwise similar turns.
pub fn process_panel_visible(turn: &TurnView) -> bool {
    !turn.user.text.trim().is_empty()
        || turn.process_steps > 0
        || !turn.workflow.is_empty()
        || matches!(
            turn.assistant.state.as_str(),
            "running" | "streaming" | "queued"
        )
}

/// Bounded process page returned by the native adapter.  `items` is at most
/// one storage page (100 entries) and never contains the final answer body.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessPageView {
    pub items: Vec<WorkflowView>,
    pub first: Option<i64>,
    pub last: Option<i64>,
    pub has_older: bool,
    pub has_newer: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnectionState {
    #[default]
    Starting,
    Ready,
    Connecting,
    Disconnected,
    Failed,
}

/// Read-only, bounded projection of the current thread's system prompt.
#[derive(Clone, Debug, Default)]
pub struct PromptPreview {
    pub open: bool,
    pub loading: bool,
    pub request_id: u64,
    pub session_id: Option<String>,
    pub text: String,
    pub error: String,
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub sessions: Vec<SessionView>,
    pub workspace_files: Vec<WorkspaceFileView>,
    /// The selected workspace row is presentation-only. File opening remains
    /// explicit (double-click or the row context menu), matching the desktop
    /// workbench where a single click only changes the focused item.
    pub selected_workspace_file: Option<String>,
    pub workspace_file_path: String,
    pub workspace_file_loading: bool,
    pub workspace_file_error: String,
    pub selected_session: Option<String>,
    /// Bounded LLM-only catalogue for the composer model picker.
    pub models: Vec<ModelOption>,
    pub model_picker_open: bool,
    pub model_picker_loading: bool,
    pub model_picker_saving: bool,
    /// Local picker feedback. Model operations never alter chat connection or
    /// stream state, so this remains separate from the general status bar.
    pub model_picker_error: String,
    pub turns: Vec<TurnView>,
    pub composer: String,
    pub connection: ConnectionState,
    pub status: String,
    pub busy: bool,
    pub should_follow_tail: bool,
    pub prompt_preview: PromptPreview,
    /// Whether the Hive-style settings window is visible.
    pub settings_open: bool,
    /// Active settings category id. Kept as a stable string so native and
    /// remote adapters can persist it without coupling to the view module.
    pub settings_category: String,
    pub settings_search: String,
    pub settings_theme: String,
    pub settings_language: String,
    pub settings_send_key: String,
    pub settings_font_scale: f32,
    pub companion_enabled: bool,
    pub companion_hints: bool,
    pub companion_scale: f32,
    pub settings_tools_tab: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            workspace_files: Vec::new(),
            selected_workspace_file: None,
            workspace_file_path: String::new(),
            workspace_file_loading: false,
            workspace_file_error: String::new(),
            selected_session: None,
            models: Vec::new(),
            model_picker_open: false,
            model_picker_loading: false,
            model_picker_saving: false,
            model_picker_error: String::new(),
            turns: Vec::new(),
            composer: String::new(),
            connection: ConnectionState::Starting,
            status: String::new(),
            busy: false,
            should_follow_tail: true,
            prompt_preview: PromptPreview::default(),
            settings_open: false,
            settings_category: "general".to_owned(),
            settings_search: String::new(),
            settings_theme: "浅色".to_owned(),
            settings_language: "跟随系统".to_owned(),
            settings_send_key: "Enter 发送".to_owned(),
            settings_font_scale: 1.0,
            companion_enabled: true,
            companion_hints: true,
            companion_scale: 1.0,
            settings_tools_tab: "builtin".to_owned(),
        }
    }
}

impl AppState {
    pub fn selected(&self) -> Option<&SessionView> {
        let id = self.selected_session.as_deref()?;
        self.sessions.iter().find(|session| session.id == id)
    }

    pub fn select_session(&mut self, id: String) {
        if self.sessions.iter().any(|session| session.id == id) {
            self.selected_session = Some(id);
            self.selected_workspace_file = None;
            self.turns.clear();
            self.busy = false;
            self.model_picker_open = false;
            self.model_picker_error.clear();
            self.prompt_preview.open = false;
            self.prompt_preview.loading = false;
            self.prompt_preview.text.clear();
        }
    }

    /// Begin a directory refresh while retaining the visual selection until
    /// the replacement list proves that its path disappeared. This avoids a
    /// distracting deselect/reselect flash when the focused file still exists.
    pub fn begin_workspace_refresh(&mut self) {
        self.workspace_file_loading = true;
        self.workspace_file_error.clear();
    }

    pub fn select_workspace_file(&mut self, path: String) {
        self.selected_workspace_file = Some(path);
    }

    /// Apply one refreshed, already bounded directory listing. A focused path
    /// remains selected when it still exists; a renamed or removed path is
    /// cleared only after the worker returns the authoritative replacement.
    pub fn replace_workspace_files(&mut self, files: Vec<WorkspaceFileView>) {
        if self
            .selected_workspace_file
            .as_ref()
            .is_some_and(|selected| !files.iter().any(|file| &file.path == selected))
        {
            self.selected_workspace_file = None;
        }
        self.workspace_files = files;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_selection_survives_refresh_only_when_path_still_exists() {
        let mut state = AppState::default();
        state.select_workspace_file("notes/plan.md".to_owned());
        state.replace_workspace_files(vec![WorkspaceFileView {
            name: "plan.md".to_owned(),
            path: "notes/plan.md".to_owned(),
            ..Default::default()
        }]);
        assert_eq!(
            state.selected_workspace_file.as_deref(),
            Some("notes/plan.md")
        );

        state.replace_workspace_files(vec![WorkspaceFileView {
            name: "other.md".to_owned(),
            path: "notes/other.md".to_owned(),
            ..Default::default()
        }]);
        assert_eq!(state.selected_workspace_file, None);
    }

    #[test]
    fn user_turn_keeps_process_disclosure_without_persisted_steps() {
        assert!(process_panel_visible(&TurnView {
            user: MessageView {
                text: "请开始".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        }));
        assert!(!process_panel_visible(&TurnView::default()));
    }
}
