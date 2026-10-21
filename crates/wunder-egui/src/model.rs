//! Small, bounded presentation model shared by native and web adapters.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub id: String,
    pub title: String,
    pub workspace: String,
    /// Durable id of the workspace that owns this thread. The rail groups
    /// threads by it; the display name alone cannot survive a rename.
    #[serde(default)]
    pub workspace_id: Option<String>,
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

/// One workspace row in the left rail.
///
/// Only the fields the rail draws cross the boundary: the directory binding,
/// timestamps and sort order stay in `NativeDesktop`, which remains the single
/// owner of workspace records.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceView {
    pub id: String,
    pub name: String,
    /// Absolute folder the workspace is bound to. The edit dialog shows it and
    /// the façade re-validates it on save, so the rail carries it rather than
    /// issuing a separate read per edit.
    pub root_path: String,
    /// Palette icon name (`folder`, `monitor`, …). Unknown names fall back to
    /// the generic folder glyph.
    pub icon: String,
    /// Palette colour name (`red`, `teal`, …). Unknown names fall back to grey.
    pub color: String,
    /// Non-archived threads bound to this workspace, as counted by the
    /// backend. The rail prefers its own live count when they disagree.
    pub thread_count: i32,
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

/// A live assistant message is drawn as plain text; Markdown is parsed once
/// the turn settles. Parsing mid-stream would re-wrap the message on every
/// token, which is why the Slint shell also defers its projection.
pub fn assistant_streaming(message: &MessageView) -> bool {
    matches!(message.state.as_str(), "running" | "streaming" | "queued")
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
    /// Workspace rows for the left rail, in backend sort order. The rail draws
    /// device → workspace → thread, so threads alone cannot describe the tree.
    pub workspaces: Vec<WorkspaceView>,
    /// Device choice and the single expanded workspace.
    pub sidebar: crate::sidebar_tree::SidebarTree,
    /// Presentation-only: the backend reported no workspace at all, so the
    /// rail must say so instead of rendering an empty tree.
    pub workspaces_loaded: bool,
    pub workspace_files: Vec<WorkspaceFileView>,
    /// The selected workspace row is presentation-only. File opening remains
    /// explicit (double-click or the row context menu), matching the desktop
    /// workbench where a single click only changes the focused item.
    pub selected_workspace_file: Option<String>,
    /// Monotonic id of the newest directory listing request. Replies echo it
    /// so a slow page for a directory the user already left is discarded
    /// instead of replacing the current listing.
    pub workspace_request_id: u64,
    pub workspace_file_loading: bool,
    pub workspace_file_error: String,
    /// Expanded directories and their lazily loaded children. The root listing
    /// stays in `workspace_files`; `workspace_tree` owns everything below it, so
    /// expanding a folder never overwrites the root projection.
    pub workspace_tree: crate::workspace_tree::Tree,
    /// Multi-select state for the directory tree (enter/toggle/select-all).
    pub workspace_selection: crate::workspace_tree::FileSelection,
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
    /// Pending attachments, the send queue and the up/down history live with
    /// the draft: the composer draws them and `composer_attachments` owns the
    /// limits, so every entry path (drop, picker, reference) shares one budget.
    pub attachments: crate::composer_attachments::Attachments,
    pub prompt_queue: crate::composer_attachments::PromptQueue,
    pub prompt_history: crate::composer_attachments::History,
    /// One-line composer feedback (attachment refused, prompt queued). Kept
    /// apart from `status`, which reports backend work.
    pub composer_notice: String,
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
    /// Executable path of the running engine. Empty until the native adapter
    /// reports it, so the settings page can render an explicit placeholder.
    pub runtime_path: String,
    /// Process-level root of the runtime. The panel's tree works in paths
    /// relative to this root, so it never needs the root itself.
    pub workspace_root: String,
    /// Feedback for the runtime card's copy/open actions. A string (instead of
    /// a bool) lets the page explain why an action could not run.
    pub workspace_action_note: String,
    /// Extra local directories offered to tool discovery.
    pub tool_paths: Vec<String>,
    /// Editor buffer for the next tool path. Stored in `AppState` so the text
    /// survives frames without a temporary egui buffer.
    pub tool_path_input: String,
    /// Validation message for the tool path editor; empty means no error.
    pub tool_path_error: String,
    /// Optional supplement capabilities with their enabled flag. A plain pair
    /// keeps the model free of settings-page presentation concerns.
    pub supplement_bundles: Vec<(String, bool)>,
    /// Result text of the last local self check; empty before the first run.
    pub self_check_state: String,
    /// egui time of the last self check, used to show a relative timestamp.
    pub self_check_at: Option<f64>,
    /// Scheduled-task editor draft. The runtime exposes no cron command yet, so
    /// these fields only drive validation and preview; nothing here may claim
    /// the task was persisted.
    pub cron_editor_open: bool,
    /// Displayed schedule kind ("一次性" / "固定间隔" / "Cron 表达式"). Stored as
    /// the same label the select shows, so the model needs no id mapping.
    pub cron_kind: String,
    pub cron_name: String,
    pub cron_at: String,
    pub cron_every: String,
    pub cron_expression: String,
    pub cron_timezone: String,
    pub cron_message: String,
    pub cron_target_agent: String,
    pub cron_delete_after: bool,
    pub cron_enabled: bool,
    /// Display-only: an adapter sets this while it fetches the job list. The
    /// page has no local setter, so it can never fake a pending request.
    pub cron_loading: bool,
    pub cron_runs_open: bool,
    /// Feedback for the last editor action. A string keeps the reason visible
    /// instead of collapsing every outcome into a success flag.
    pub cron_note: String,
    pub cron_note_error: bool,
    /// Device interlink switches. The tunnel table itself is adapter-owned, so
    /// only these booleans are local preference state.
    pub devices_allow_remote: bool,
    pub devices_share_catalog: bool,
    pub devices_cloud_shadow: bool,
    /// Display-only, like `cron_loading`: only an adapter can end the loading
    /// state, so a local button cannot leave the page spinning forever.
    pub devices_loading: bool,
    /// Adapter-reported tunnel failure. Kept apart from `devices_note` so the
    /// page can colour a real failure differently from an informational note.
    pub devices_error: String,
    pub devices_note: String,
    /// Built-in agent form. The name is preset-owned and shown read-only.
    pub agent_name: String,
    pub agent_greeting: String,
    pub agent_extra_prompt: String,
    pub agent_tool_search: String,
    /// One flag per tool in `settings::AGENT_TOOL_GROUPS`, flattened in
    /// catalogue order. The catalogue stays the only source of tool names.
    pub agent_tool_enabled: Vec<bool>,
    pub agent_preset_questions: Vec<String>,
    pub agent_question_draft: String,
    pub agent_note: String,
    pub agent_note_error: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            workspaces: Vec::new(),
            sidebar: Default::default(),
            workspaces_loaded: false,
            workspace_files: Vec::new(),
            selected_workspace_file: None,
            workspace_request_id: 0,
            workspace_file_loading: false,
            workspace_tree: Default::default(),
            workspace_selection: Default::default(),
            workspace_file_error: String::new(),
            selected_session: None,
            models: Vec::new(),
            model_picker_open: false,
            model_picker_loading: false,
            model_picker_saving: false,
            model_picker_error: String::new(),
            turns: Vec::new(),
            composer: String::new(),
            attachments: Default::default(),
            prompt_queue: Default::default(),
            prompt_history: Default::default(),
            composer_notice: String::new(),
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
            runtime_path: String::new(),
            workspace_root: String::new(),
            workspace_action_note: String::new(),
            tool_paths: Vec::new(),
            tool_path_input: String::new(),
            tool_path_error: String::new(),
            // Names must stay in sync with `settings::SUPPLEMENT_CATALOGUE`;
            // the settings tests assert that both lists agree.
            supplement_bundles: vec![
                ("扩展语法高亮".to_owned(), true),
                ("离线文档索引".to_owned(), true),
                ("实验性工具栏".to_owned(), false),
            ],
            self_check_state: String::new(),
            self_check_at: None,
            cron_editor_open: false,
            cron_kind: "一次性".to_owned(),
            cron_name: String::new(),
            cron_at: String::new(),
            cron_every: String::new(),
            cron_expression: String::new(),
            cron_timezone: String::new(),
            cron_message: String::new(),
            cron_target_agent: "默认智能体".to_owned(),
            cron_delete_after: false,
            cron_enabled: true,
            cron_loading: false,
            cron_runs_open: false,
            cron_note: String::new(),
            cron_note_error: false,
            devices_allow_remote: false,
            devices_share_catalog: false,
            // The cloud shadow is the default projection: it only publishes a
            // device summary, never file contents, so it is safe to start on.
            devices_cloud_shadow: true,
            devices_loading: false,
            devices_error: String::new(),
            devices_note: String::new(),
            agent_name: "默认智能体".to_owned(),
            agent_greeting: String::new(),
            agent_extra_prompt: String::new(),
            agent_tool_search: String::new(),
            // Mirrors the flattened `settings::AGENT_TOOL_GROUPS` order; the
            // settings tests fail when the two lengths drift apart. Enabled
            // and disabled entries are mixed so both row states are visible.
            agent_tool_enabled: vec![true, true, false, false, false, true, false, false],
            agent_preset_questions: Vec::new(),
            agent_question_draft: String::new(),
            agent_note: String::new(),
            agent_note_error: false,
        }
    }
}

impl AppState {
    pub fn selected(&self) -> Option<&SessionView> {
        let id = self.selected_session.as_deref()?;
        self.sessions.iter().find(|session| session.id == id)
    }

    pub fn select_session(&mut self, id: String) {
        if let Some(session) = self.sessions.iter().find(|session| session.id == id) {
            // Keep the owning group open: a thread selected from a queued
            // prompt or a restore has no row to point at otherwise.
            if let Some(workspace_id) = session.workspace_id.clone() {
                self.sidebar.reveal(&workspace_id);
            }
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
    pub fn begin_workspace_refresh(&mut self) -> u64 {
        self.workspace_file_loading = true;
        self.workspace_file_error.clear();
        self.workspace_request_id = self.workspace_request_id.wrapping_add(1);
        self.workspace_request_id
    }

    pub fn select_workspace_file(&mut self, path: String) {
        self.selected_workspace_file = Some(path);
    }

    /// Apply one refreshed workspace catalogue. An expansion that points at a
    /// deleted workspace is dropped here instead of leaving an open group with
    /// no rows beneath it.
    pub fn replace_workspaces(&mut self, workspaces: Vec<WorkspaceView>) {
        if let Some(expanded) = self.sidebar.expanded().map(str::to_owned) {
            if !workspaces.iter().any(|workspace| workspace.id == expanded) {
                self.sidebar.collapse();
            }
        }
        self.workspaces = workspaces;
        self.workspaces_loaded = true;
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
