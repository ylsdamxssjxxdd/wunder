//! Engine bridge: owns the in-process `NativeDesktop` and turns its durable
//! thread updates plus the live send stream into the flat models the phone UI
//! renders. All engine calls and markdown projection stay off the UI thread;
//! the UI only copies the latest projection on its 33 ms tick.

use crate::project::{
    self, LiveTurn, RowData, STATUS_DONE, STATUS_FAILED, STATUS_RUNNING, STATUS_STOPPED,
};
use crate::{
    MobileChannelDraft, MobileCronDraft, MobileCronJob, MobileCronRun, MobileKnowledgeDraft,
    MobileMcpDraft, MobileMemory, MobileMemoryDraft, MobileSession, MobileTool, MobileToolFile,
    MobileToolResource, MobileWorkspace, ModelCard, TrajectoryAudit, TrajectoryBar, TrajectoryItem,
    TrajectorySegment, WorkspaceFile,
};
use serde_json::Value;
#[cfg(target_os = "android")]
use std::sync::atomic::AtomicI32;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use wunder_desktop::native::{
    NativeChatCommand, NativeGoal, NativeKnowledgeDraft, NativeMcpServerDraft, NativeToolFile,
    NativeToolFiles, NativeToolResource, ToolResourceKind,
};
use wunder_desktop::{
    AgentSettingsEdit, ModelEdit, NativeChatEvent, NativeChatInput, NativeChatTurn,
    NativeCronJobEdit, NativeDesktop, NativeThreadUpdate, NativeThreadWatchCancel,
    NativeWorkflowEntry, ToolRecord, WorkspaceEdit, CURRENT_VERSION, DEFAULT_AGENT_ID,
};

pub enum Command {
    Select(String),
    NewThread,
    Fork(i32),
    Send(String),
    Stop,
    SelectModel(String),
    /// 目标横幅的编辑/暂停/恢复/清除，复用斜杠命令管线。
    GoalEdit(String),
    GoalResume,
    GoalPause,
    GoalClear,
    RefreshSessions,
    LoadSettings,
    RefreshPermissions,
    OpenPermission(String),
    CheckAndroidUpdate,
    DownloadAndroidUpdate,
    InstallAndroidUpdate,
    SetUpdateSource(String),
    RenameSession {
        id: String,
        title: String,
    },
    ArchiveSession(String),
    RestoreSession(String),
    DeleteArchivedSession(String),
    RefreshArchived,
    LoadTrajectory {
        id: String,
        title: String,
    },
    TrajectoryFilter(i32),
    LoadPromptPreview,
    LoadWorkspace {
        path: String,
    },
    LoadMoreWorkspace {
        path: String,
        offset: u64,
    },
    RefreshWorkspace,
    RefreshWorkspaces,
    SelectWorkspace(String),
    NewThreadInWorkspace(String),
    CreateWorkspace {
        name: String,
        icon: String,
        color: String,
        /// Empty = create a managed folder under the app workspace root.
        folder: String,
    },
    UpdateWorkspace {
        id: String,
        name: String,
        icon: String,
        color: String,
        /// Empty = keep the current root.
        folder: String,
    },
    DeleteWorkspace(String),
    RenameWorkspaceEntry {
        path: String,
        name: String,
    },
    MoveWorkspaceEntry {
        path: String,
        target_dir: String,
    },
    DeleteWorkspaceEntry {
        path: String,
    },
    OpenWorkspaceFile {
        path: String,
    },
    OpenWorkspaceLocation {
        path: String,
        dir: bool,
    },
    UploadWorkspace {
        dir: String,
        files: Vec<String>,
    },
    /// A system share / VIEW payload drained from `IntentBridge`.
    SharedIntent {
        text: String,
        url: String,
        files: Vec<String>,
    },
    DownloadWorkspace {
        path: String,
    },
    /// Decode one Markdown image for the transcript. The UI only knows the
    /// source string; the engine owns the workspace boundary and the decoder.
    LoadMarkdownImage {
        source: String,
    },
    /// A decoder worker finished one Markdown image; `None` is a failed load.
    MarkdownImageLoaded {
        source: String,
        image: Option<SharedImage>,
    },
    WorkspaceSaved {
        ok: bool,
        name: String,
    },
    VoiceToggle,
    VoiceStarted,
    VoiceFinished {
        path: String,
    },
    VoiceFailed {
        message: String,
    },
    CaptureScreenshot,
    ScreenshotResult {
        status: String,
    },
    SavePreferences {
        theme: String,
        send_key: String,
        font_size: i32,
    },
    SaveLanguage(String),
    PermissionMode(String),
    OpenModelEditor {
        key: Option<String>,
    },
    CloseModelEditor,
    SaveModel {
        card: ModelCard,
        secret: String,
    },
    ProbeModel {
        card: ModelCard,
        secret: String,
    },
    DeleteModel(String),
    RefreshUbuntu,
    InstallUbuntu,
    SetUbuntuEnabled(bool),
    RefreshCompanion,
    SaveCompanion {
        enabled: bool,
        selected_id: String,
        scale: f32,
        message_hints: bool,
    },
    CompanionPermission,
    InquiryOption {
        row: i32,
        option: i32,
    },
    InquiryNoPreference {
        row: i32,
    },
    InquiryUseRecommended {
        row: i32,
    },
    InquiryOther {
        row: i32,
        text: String,
    },
    InquiryPageMove(i32),
    InquiryExpandToggle,
    InquirySubmit,
    RefreshTasks,
    // ---- tool manager -------------------------------------------------
    RefreshTools,
    SelectToolServer {
        index: i32,
    },
    SelectToolResource {
        tab: i32,
        index: i32,
    },
    LoadToolDirectory {
        directory: String,
    },
    MoreToolFiles,
    OpenToolFile {
        path: String,
    },
    SaveToolFile {
        path: String,
        content: String,
    },
    CloseToolFile,
    CreateToolFile {
        name: String,
    },
    UploadToolFiles {
        files: Vec<String>,
    },
    DeleteToolResource {
        tab: i32,
        name: String,
    },
    ConnectToolServer {
        name: String,
    },
    ExportSkill {
        name: String,
    },
    ImportSkill {
        path: String,
    },
    SkillImported {
        error: String,
    },
    ToolServerConnected {
        name: String,
        error: String,
    },
    OpenToolEditor {
        kind: i32,
        index: i32,
    },
    CloseToolEditor,
    SaveToolEditor {
        kind: i32,
        mcp: MobileMcpDraft,
        kb: MobileKnowledgeDraft,
    },
    // ---- built-in agent ----------------------------------------------
    RefreshAgent,
    AgentToolSearch(String),
    SaveAgentText {
        description: String,
        system_prompt: String,
    },
    ToggleAgentTool {
        name: String,
        enabled: bool,
        description: String,
        system_prompt: String,
    },
    ToggleAgentToolGroup {
        title: String,
        select: bool,
        description: String,
        system_prompt: String,
    },
    AddAgentQuestion {
        question: String,
        description: String,
        system_prompt: String,
    },
    RemoveAgentQuestion {
        index: i32,
        description: String,
        system_prompt: String,
    },
    // ---- cron jobs ----------------------------------------------------
    RefreshCron,
    ToggleCron {
        id: String,
        enabled: bool,
    },
    DeleteCron {
        id: String,
    },
    RunCronNow {
        id: String,
    },
    OpenCronRuns {
        id: String,
    },
    CloseCronRuns,
    OpenCronEditor {
        id: Option<String>,
    },
    CloseCronEditor,
    SaveCron {
        draft: MobileCronDraft,
    },
    // ---- memory fragments --------------------------------------------
    RefreshMemory,
    MemorySearch(String),
    MemoryCategory(String),
    OpenMemoryEditor {
        index: i32,
    },
    CloseMemoryEditor,
    SaveMemory {
        draft: MobileMemoryDraft,
    },
    DeleteMemory {
        id: String,
    },
    // ---- cloud account / device switch / cloud chat -------------------
    SwitchChatTarget(i32),
    RefreshCloudAccount,
    RefreshUsage,
    RefreshCloudRemote,
    CloudLogin {
        server: String,
        username: String,
        password: String,
    },
    CloudLogout,
    CloudSetSync(bool),
    CloudSetSharing {
        enabled: bool,
        full_shadow: bool,
    },
    RefreshCloudThreads,
    CloudNewThread,
    CloudFork(i32),
    CloudSelectThread(String),
    CloudSend(String),
    CloudStop,
    CloudPoll,
    CloudRename {
        id: String,
        title: String,
    },
    CloudArchive(String),
    RefreshDevices,
    PollApprovals,
    DecideApproval {
        id: String,
        approve: bool,
    },
    // ---- channels ------------------------------------------------------
    RefreshChannels,
    SelectChannelAccount(String),
    OpenChannelEditor(String),
    CloseChannelEditor,
    SaveChannel(MobileChannelDraft),
    ToggleChannel {
        account_id: String,
        enabled: bool,
    },
    DeleteChannel(String),
    OpenChannelLogs(String),
    CloseChannelLogs,
    DeleteChannelBinding(String),
    // ---- local backup / crash reports ---------------------------------
    RefreshBackup,
    BackupNow,
    RestoreBackup {
        path: String,
    },
    LoadCrashLog,
}

/// Persisted preferences mirrored into the window on every change.
#[derive(Clone)]
pub struct SettingsView {
    pub theme: String,
    pub font_size: i32,
    pub send_key: String,
    pub language: String,
    pub workspace_root: String,
    pub update_source: String,
    pub version: String,
}

impl Default for SettingsView {
    fn default() -> Self {
        Self {
            theme: "terracotta".into(),
            font_size: 14,
            send_key: "enter".into(),
            language: "zh-CN".into(),
            workspace_root: String::new(),
            update_source: "cloud".into(),
            version: CURRENT_VERSION.into(),
        }
    }
}

/// One selectable companion library entry for the settings picker.
#[derive(Clone, Default)]
pub struct CompanionChoice {
    pub id: String,
    pub name: String,
}

/// Settings-page projection of the persisted companion overlay state.
#[derive(Clone, Default)]
pub struct CompanionView {
    pub enabled: bool,
    pub selected_id: String,
    pub scale: f32,
    pub message_hints: bool,
    /// `granted`, `denied` or `unknown` (bridge unavailable / host preview).
    pub permission: String,
    pub companions: Vec<CompanionChoice>,
}

/// One option of a published 问询面板 question, including the user's tick.
#[derive(Clone, Default)]
pub struct InquiryOptionView {
    pub label: String,
    pub description: String,
    pub recommended: bool,
    pub selected: bool,
}

/// One question of the interactive inquiry panel. Selection state lives with
/// the engine so the UI stays a pure projection and paging never loses an
/// answer.
#[derive(Clone, Default)]
pub struct InquiryQuestionView {
    pub question: String,
    pub multiple: bool,
    pub no_preference: bool,
    pub other: String,
    pub options: Vec<InquiryOptionView>,
}

/// A pending `question_panel` payload, bounded to the four questions/four
/// options the tool itself publishes.
#[derive(Clone, Default)]
pub struct InquiryView {
    pub turn_id: String,
    pub questions: Vec<InquiryQuestionView>,
    pub page: usize,
    pub expanded: bool,
}

impl InquiryView {
    fn from_payload(data: &Value) -> Option<Self> {
        if matches!(
            data.get("status").and_then(Value::as_str),
            Some("answered" | "dismissed" | "completed")
        ) {
            return None;
        }
        let questions: Vec<InquiryQuestionView> = data["questions"]
            .as_array()?
            .iter()
            .filter_map(|entry| {
                let options: Vec<InquiryOptionView> = entry["options"]
                    .as_array()?
                    .iter()
                    .filter_map(|option| {
                        let label = option["label"].as_str()?;
                        Some(InquiryOptionView {
                            label: label.to_string(),
                            description: option["description"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                            recommended: option["recommended"].as_bool().unwrap_or(false),
                            selected: false,
                        })
                    })
                    .take(4)
                    .collect();
                if options.is_empty() {
                    return None;
                }
                Some(InquiryQuestionView {
                    question: entry["question"].as_str()?.to_string(),
                    multiple: entry["multiple"].as_bool().unwrap_or(false),
                    no_preference: false,
                    other: String::new(),
                    options,
                })
            })
            .take(4)
            .collect();
        (!questions.is_empty()).then(|| Self {
            turn_id: data
                .get("turn_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            questions,
            page: 0,
            expanded: false,
        })
    }

    pub(crate) fn current_index(&self) -> usize {
        self.page.min(self.questions.len().saturating_sub(1))
    }

    fn question_index(&self, row: i32) -> usize {
        if self.expanded {
            (row.max(0) as usize).min(self.questions.len().saturating_sub(1))
        } else {
            self.current_index()
        }
    }

    fn is_answered(question: &InquiryQuestionView) -> bool {
        question.no_preference
            || !question.other.trim().is_empty()
            || question.options.iter().any(|option| option.selected)
    }

    pub(crate) fn ready(&self) -> bool {
        !self.questions.is_empty() && self.questions.iter().all(Self::is_answered)
    }

    fn toggle_option(&mut self, index: usize, option: usize) {
        let Some(question) = self.questions.get_mut(index) else {
            return;
        };
        if option >= question.options.len() {
            return;
        }
        question.no_preference = false;
        if question.multiple {
            question.options[option].selected = !question.options[option].selected;
            return;
        }
        let next = !question.options[option].selected;
        for candidate in &mut question.options {
            candidate.selected = false;
        }
        question.options[option].selected = next;
    }

    fn toggle_no_preference(&mut self, index: usize) {
        let Some(question) = self.questions.get_mut(index) else {
            return;
        };
        question.no_preference = !question.no_preference;
        if !question.no_preference {
            return;
        }
        question.other.clear();
        for option in &mut question.options {
            option.selected = false;
        }
    }

    fn use_recommended(&mut self, index: usize) {
        let Some(question) = self.questions.get_mut(index) else {
            return;
        };
        let recommended: Vec<usize> = question
            .options
            .iter()
            .enumerate()
            .filter_map(|(position, option)| option.recommended.then_some(position))
            .collect();
        if recommended.is_empty() {
            return;
        }
        question.no_preference = false;
        question.other.clear();
        for (position, option) in question.options.iter_mut().enumerate() {
            option.selected = if question.multiple {
                recommended.contains(&position)
            } else {
                position == recommended[0]
            };
        }
    }

    fn move_page(&mut self, delta: i32) {
        let last = self.questions.len().saturating_sub(1) as i32;
        self.page = (self.page as i32 + delta).clamp(0, last).max(0) as usize;
    }

    fn reply(&self) -> String {
        let total = self.questions.len();
        let mut lines = vec!["【问询面板选择】".to_string()];
        for (index, question) in self.questions.iter().enumerate() {
            lines.push(if total > 1 {
                format!("问题 {}/{total}：{}", index + 1, question.question)
            } else {
                format!("问题：{}", question.question)
            });
            let answer = if question.no_preference {
                "无偏好".to_string()
            } else {
                let mut parts: Vec<String> = question
                    .options
                    .iter()
                    .filter(|option| option.selected)
                    .map(|option| option.label.clone())
                    .collect();
                let other = question.other.trim();
                if !other.is_empty() {
                    parts.push(other.to_string());
                }
                parts.join("、")
            };
            lines.push(format!("答案：{answer}"));
        }
        lines.join("\n")
    }

    fn consumption_key(&self, session: &str) -> String {
        let mut key = format!("{session}:{}", self.turn_id);
        for question in &self.questions {
            key.push('|');
            key.push_str(&question.question);
            for option in &question.options {
                key.push('|');
                key.push_str(&option.label);
            }
        }
        key
    }
}

/// The bundled proot Ubuntu environment as the settings card renders it.
#[derive(Clone, Default)]
pub struct UbuntuView {
    pub state: String,
    pub progress: i32,
    pub stage: String,
    pub version: String,
    pub error: String,
    /// True when `settings.shell_path` points at the generated wrapper.
    pub enabled: bool,
    pub shell_path: String,
}

/// Draft the model editor is bound to. Empty `original_key` means a new model.
pub struct ModelEditor {
    pub original_key: String,
    pub card: ModelCard,
    pub is_default: bool,
    pub error: String,
    pub probe_message: String,
    pub probing: bool,
}

/// Phone projection of the tool manager. `browse_*` only describes the
/// resource whose file list is on screen; the lists themselves are bounded by
/// the native page size (200 entries).
#[derive(Clone, Default)]
pub struct ToolsView {
    pub builtin: Vec<MobileTool>,
    pub mcp_tools: Vec<MobileTool>,
    pub skill_tools: Vec<MobileTool>,
    pub knowledge_tools: Vec<MobileTool>,
    pub servers: Vec<MobileToolResource>,
    pub skills: Vec<MobileToolResource>,
    pub bases: Vec<MobileToolResource>,
    pub server_tools: Vec<MobileTool>,
    pub active_server: String,
    pub browse_tab: i32,
    pub browse_resource: String,
    pub browse_readonly: bool,
    /// False for vector/RAGFlow knowledge bases, which keep their text outside
    /// the file browser.
    pub browse_editable: bool,
    pub directory: String,
    pub files: Vec<MobileToolFile>,
    pub has_more: bool,
    pub error: String,
}

/// MCP/knowledge form opened from the tools page. The draft is seeded once
/// per opening; the UI edits it and sends it back on save.
#[derive(Clone, Default)]
pub struct ToolEditorView {
    pub kind: i32,
    pub is_new: bool,
    pub mcp: MobileMcpDraft,
    pub kb: MobileKnowledgeDraft,
    pub error: String,
}

/// One text file opened from a skill or knowledge base browser.
#[derive(Clone, Default)]
pub struct ToolFileView {
    pub path: String,
    pub content: String,
    pub readonly: bool,
    pub error: String,
}

/// Send-friendly copy of one tool group. Slint's `ModelRc` is UI-thread only,
/// so the engine never stores it; the tick converts this on the main thread.
#[derive(Clone, Default)]
pub struct AgentToolGroupView {
    pub title: String,
    pub tools: Vec<MobileTool>,
    pub all_selected: bool,
}

/// The single built-in agent as the phone edits it.
#[derive(Clone, Default)]
pub struct AgentView {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub tool_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub groups: Vec<AgentToolGroupView>,
    pub tool_count: i32,
    pub search: String,
    /// Bumped when the form should reload description/prompt from storage.
    pub seed: u64,
}

#[derive(Clone, Default)]
pub struct CronWorkspaceRef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Default)]
pub struct CronView {
    pub jobs: Vec<MobileCronJob>,
    pub workspaces: Vec<CronWorkspaceRef>,
    pub loading: bool,
    pub error: String,
}

#[derive(Clone, Default)]
pub struct CronEditorView {
    pub is_new: bool,
    pub draft: MobileCronDraft,
    pub error: String,
    pub saving: bool,
}

#[derive(Clone, Default)]
pub struct CronRunsView {
    pub open: bool,
    pub loading: bool,
    pub title: String,
    pub runs: Vec<MobileCronRun>,
    pub error: String,
}

#[derive(Clone)]
pub struct MemoryView {
    pub memories: Vec<MobileMemory>,
    pub categories: Vec<String>,
    pub query: String,
    pub category: String,
    pub loading: bool,
    pub error: String,
    /// Bumped when the query should be rebased into the search field.
    pub seed: u64,
}

impl Default for MemoryView {
    fn default() -> Self {
        Self {
            memories: Vec::new(),
            categories: vec!["全部标签".into()],
            query: String::new(),
            category: "全部标签".into(),
            loading: false,
            error: String::new(),
            seed: 0,
        }
    }
}

#[derive(Clone, Default)]
pub struct MemoryEditorView {
    pub is_new: bool,
    pub draft: MobileMemoryDraft,
    pub error: String,
    pub saving: bool,
}

/// One decoded Markdown image, already bounded to display size by the engine
/// worker. A still image has one frame; a GIF keeps every bounded frame and
/// its per-frame delay so the UI can animate it. The UI only wraps the frames
/// into `slint::Image`s.
pub struct SharedImage {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Vec<u8>>,
    pub delays: Vec<u32>,
}

/// 目标横幅：线程当前目标的只读快照与本地计时。
#[derive(Default, Clone)]
pub struct GoalView {
    pub objective: String,
    /// active | paused | blocked | complete
    pub phase: String,
    /// armed | disarmed
    pub activation: String,
    pub rounds_started: i64,
    pub max_goal_rounds: i64,
    pub seconds: i64,
}

impl GoalView {
    pub(crate) fn active(&self) -> bool {
        self.phase == "active" && self.activation == "armed"
    }

    pub(crate) fn paused(&self) -> bool {
        self.phase == "paused"
    }

    pub(crate) fn visible(&self) -> bool {
        !self.objective.trim().is_empty() && !self.phase.is_empty() && self.phase != "complete"
    }
}

#[derive(Default, Clone, PartialEq)]
pub struct PlanStepView {
    pub step: String,
    pub status: String,
}

/// 计划面板：最近一次计划清单与是否有回合正在执行。
#[derive(Default, Clone)]
pub struct PlanView {
    pub explanation: String,
    pub steps: Vec<PlanStepView>,
    pub running: bool,
}

/// Everything the UI tick reads. Revisions keep the tick cheap: scalar state
/// and the two row segments are copied only when the engine actually changed
/// them, so a streaming token never rebuilds the whole transcript model.
#[derive(Default)]
pub struct Shared {
    pub rev: u64,
    pub rows_rev: u64,
    pub live_rev: u64,
    pub toast_rev: u64,
    pub patch_rev: u64,
    pub patch_row: Option<usize>,
    pub sessions_rev: u64,
    pub archived_rev: u64,
    pub models_rev: u64,
    pub settings_rev: u64,
    pub editor_rev: u64,
    pub probe_rev: u64,
    pub trajectory_rev: u64,
    pub preset_rev: u64,
    /// Settings-page permission catalog and its last summary.
    pub permissions: Vec<crate::permissions::PermissionItem>,
    pub permission_summary: String,
    pub permissions_rev: u64,
    /// Android online update projection. `update_offer` is the verified
    /// release selected by the check worker; `update_progress_bytes` is shared
    /// with the download worker so the UI tick can read progress lock-free.
    pub update_phase: String,
    pub update_version: String,
    pub update_notes: String,
    pub update_source: String,
    pub update_size_text: String,
    pub update_size_bytes: u64,
    pub update_progress_bytes: Arc<AtomicU64>,
    pub update_error: String,
    pub update_rev: u64,
    pub update_offer: Option<crate::update::UpdateOffer>,
    pub update_path: Option<std::path::PathBuf>,
    pub update_checked_at: Option<Instant>,
    pub durable_rows: Vec<RowData>,
    /// Retained durable turns behind `durable_rows`. The watcher mixes compact
    /// header updates with detailed turn commits, so rows cannot be projected
    /// from a single update without losing either rounds or earlier turns.
    pub durable_turns: Vec<NativeChatTurn>,
    pub live_rows: Vec<RowData>,
    /// Root id of the turn the live stream is building. While the stream is
    /// attached this turn's live rows stay authoritative; the durable watcher
    /// only sees commit boundaries and must not shadow them.
    pub live_root: String,
    /// Session that owns `live_rows`. A stream must never paint its tail into
    /// anything but the thread the user is looking at.
    pub live_session: String,
    /// Id of the stream that currently owns `live_rows`. A queued second send
    /// in the same thread replaces the slot; the older stream keeps receiving
    /// output but may only surface it through the durable projection.
    pub live_stream_id: u64,
    /// Decoded Markdown images keyed by their message source. `None` records a
    /// failed load so a broken reference is not retried on every projection.
    pub images: std::collections::HashMap<String, Option<SharedImage>>,
    /// Sources a decoder worker is currently reading; duplicate requests while
    /// one is in flight are dropped instead of queued.
    pub images_inflight: std::collections::HashSet<String>,
    pub images_rev: u64,
    pub sessions: Vec<MobileSession>,
    pub archived_sessions: Vec<MobileSession>,
    pub archived_total: i64,
    pub preset_questions: Vec<String>,
    pub trajectory_title: String,
    pub trajectory_loading: bool,
    pub trajectory_error: String,
    pub trajectory_items: Vec<TrajectoryItem>,
    pub trajectory_audit: TrajectoryAudit,
    pub trajectory_segments: Vec<TrajectorySegment>,
    pub trajectory_bars: Vec<TrajectoryBar>,
    pub trajectory_filter: i32,
    /// System prompt preview opened by the greeting's eye button.
    pub prompt_preview: String,
    pub prompt_preview_loading: bool,
    pub prompt_preview_error: String,
    pub prompt_preview_rev: u64,
    pub workspace_rev: u64,
    pub workspace_path: String,
    pub workspace_parent_path: String,
    pub workspace_entries: Vec<WorkspaceFile>,
    pub workspace_total: u64,
    pub workspace_loading: bool,
    pub workspace_error: String,
    /// Workspace catalogue as the drawer and picker render it.
    pub workspaces: Vec<MobileWorkspace>,
    pub workspaces_rev: u64,
    /// Workspace new tasks land in and the working-directory page browses.
    /// Follows the opened thread, then the user's explicit pick.
    pub selected_workspace: String,
    pub voice_rev: u64,
    /// 0 idle, 1 recording, 2 transcribing.
    pub voice_phase: i32,
    /// Text produced off the UI thread (voice transcription) and appended to
    /// the draft by the UI tick, so a late result never overwrites typing.
    pub pending_draft: Option<String>,
    pub models: Vec<ModelCard>,
    pub tools_rev: u64,
    pub tools: ToolsView,
    pub tool_editor_rev: u64,
    pub tool_editor: Option<ToolEditorView>,
    pub tool_file_rev: u64,
    pub tool_file: Option<ToolFileView>,
    pub agent_rev: u64,
    pub agent: AgentView,
    /// Catalog behind `agent.groups`; kept so the search box filters locally
    /// instead of re-reading the runtime on every keystroke.
    pub agent_catalog: Vec<ToolRecord>,
    pub cron_rev: u64,
    pub cron: CronView,
    pub cron_editor_rev: u64,
    pub cron_editor: Option<CronEditorView>,
    pub cron_runs_rev: u64,
    pub cron_runs: CronRunsView,
    pub memory_rev: u64,
    pub memory: MemoryView,
    pub memory_editor_rev: u64,
    pub memory_editor: Option<MemoryEditorView>,
    pub usage_rev: u64,
    pub usage_duration: String,
    pub usage_tokens: String,
    pub usage_quota: String,
    pub target: i32,
    pub target_rev: u64,
    pub cloud_rev: u64,
    pub cloud: crate::cloud::CloudView,
    pub cloud_threads_rev: u64,
    pub cloud_threads: Vec<MobileSession>,
    pub cloud_active_rev: u64,
    pub cloud_active: crate::cloud::CloudActive,
    pub devices_rev: u64,
    pub devices: Vec<crate::cloud::DeviceView>,
    pub approval_rev: u64,
    pub approval: Option<crate::cloud::ApprovalView>,
    pub channels_rev: u64,
    pub channels: crate::channels::ChannelsView,
    pub channel_editor_rev: u64,
    pub channel_editor: Option<crate::channels::ChannelEditorView>,
    pub inquiry: Option<InquiryView>,
    pub inquiry_rev: u64,
    /// 目标横幅与计划面板（输入带上沿）。
    pub goal: GoalView,
    pub goal_rev: u64,
    pub plan: PlanView,
    pub plan_rev: u64,
    /// Live command sessions in the active thread and the child agents a
    /// finished turn still owns; both feed the composer's status pills.
    pub running_task_count: i32,
    pub subagent_count: i32,
    /// Consumption keys of panels the user already answered in this process.
    /// A late durable replay of the waiting turn must not reopen them.
    pub consumed_inquiries: std::collections::VecDeque<String>,
    pub settings: SettingsView,
    /// 数据与恢复页：本地备份、待重启的恢复与最近崩溃日志。
    pub backup_rev: u64,
    /// idle / busy / done / error.
    pub backup_phase: String,
    pub backup_message: String,
    pub backup_path: String,
    /// The staged restore is applied on the next process start.
    pub backup_restart: bool,
    pub crash_log_title: String,
    pub crash_log_text: String,
    pub companion: CompanionView,
    pub companion_rev: u64,
    /// Live turn phase (`requesting`/`thinking`/`outputting`/`tool`/`waiting`/
    /// `done`/`error`/`stopped`); the floating companion maps it onto sprites.
    pub turn_phase: String,
    /// Normalized final answer or failure message of the settled turn, shown
    /// as the companion's bubble hint.
    pub turn_hint: String,
    /// Active-thread context gauge: the liquid brain fill and the model
    /// picker's "used / capacity" line. `used`/`capacity` stay optional so a
    /// missing runtime event never reads as an empty context.
    pub context_used: Option<f64>,
    pub context_capacity: Option<f64>,
    pub context_usage: f32,
    pub context_known: bool,
    pub context_counts: String,
    pub context_rev: u64,
    pub ubuntu: UbuntuView,
    pub ubuntu_rev: u64,
    /// 用户在本进程内跳过了首装引导（仅失败态可达），避免每次刷新都重新弹出。
    pub env_setup_dismissed: bool,
    pub editor: Option<ModelEditor>,
    pub probe_message: String,
    pub probed_context: String,
    pub session_id: String,
    /// Durable workspace record the active thread is bound to.
    pub workspace_id: String,
    pub title: String,
    pub subtitle: String,
    pub workspace: String,
    pub model_name: String,
    pub model_key: String,
    pub send_key: String,
    pub permission_mode: String,
    pub generating: bool,
    pub stopping: bool,
    pub loading: bool,
    pub toast: String,
    pub stream_revision: i32,
}

impl Shared {
    pub(crate) fn touch(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    pub(crate) fn notify(&mut self, message: impl Into<String>) {
        self.toast = message.into();
        self.toast_rev = self.toast_rev.wrapping_add(1);
    }
}

/// The process-wide engine. Android can destroy and recreate the activity for
/// reasons unrelated to the agent (recents, "don't keep activities", a theme
/// change, the system reclaiming the window). Reusing one engine keeps an
/// in-flight turn streaming while no window exists and lets the next activity
/// attach to the current transcript instead of starting a second engine.
pub struct EngineHandle {
    pub shared: Arc<Mutex<Shared>>,
    commands: Mutex<Sender<Command>>,
}

impl EngineHandle {
    pub fn command_sender(&self) -> Sender<Command> {
        self.commands
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

pub fn acquire() -> &'static EngineHandle {
    static ENGINE: OnceLock<EngineHandle> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let sender = start(shared.clone());
        EngineHandle {
            shared,
            commands: Mutex::new(sender),
        }
    })
}

fn start(shared: Arc<Mutex<Shared>>) -> Sender<Command> {
    let (sender, receiver) = mpsc::channel::<Command>();
    let worker_sender = sender.clone();
    thread::Builder::new()
        .name("wunder-android-engine".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            if let Err(error) = run(shared.clone(), receiver, worker_sender) {
                log::error!("local engine failed: {error:#}");
                if let Ok(mut state) = shared.lock() {
                    state.notify(format!("本地引擎启动失败：{error:#}"));
                    state.loading = false;
                    state.touch();
                }
            }
        })
        .expect("spawn the Android engine thread");
    sender
}

fn run(
    shared: Arc<Mutex<Shared>>,
    commands: mpsc::Receiver<Command>,
    sender: Sender<Command>,
) -> anyhow::Result<()> {
    wunder_desktop::install_local_home_env();
    // The phone-control host must exist before the first turn freezes its tool
    // surface; installing it after the engine starts would hide the tool from
    // every thread created earlier in this process.
    #[cfg(target_os = "android")]
    crate::phone::install();
    // Screen capture and notification access are Android-only hosts. Install
    // them before the first turn freezes the tool surface.
    #[cfg(target_os = "android")]
    crate::screen::install();
    #[cfg(target_os = "android")]
    crate::notifications::install();
    // Same rule for the browser: swap the builtin Playwright provider for the
    // phone's system WebView before the first thread captures the tool list.
    #[cfg(target_os = "android")]
    crate::webview::install();
    // The cloud device label must be set before any login registers it.
    #[cfg(target_os = "android")]
    if let Some(name) = crate::android_host::device_name() {
        std::env::set_var("WUNDER_DEVICE_NAME", name);
    }
    // A staged restore replaces config and databases before SQLite opens them;
    // applying it while the engine runs would race the live connections.
    #[cfg(target_os = "android")]
    crate::backup::apply_pending();
    let desktop = Arc::new(NativeDesktop::start()?);
    // Register as an Android node, not a second desktop, so the cloud and the
    // device directory can tell phones and desktops apart.
    #[cfg(target_os = "android")]
    desktop.set_cloud_client_kind("android");
    // Detect a previously installed Ubuntu rootfs before the first settings
    // card render. The environment itself is optional; a missing one must not
    // hold up the engine.
    #[cfg(target_os = "android")]
    crate::ubuntu::bind();
    // The floating companion is process-scoped like the engine: it keeps
    // animating while the activity is gone and the UI tick is suspended.
    #[cfg(target_os = "android")]
    crate::companion::install(desktop.clone(), shared.clone());
    #[cfg(target_os = "android")]
    configure_browser_runtime();
    // A killed turn leaves its session execution lock behind for minutes.
    // The Android sandbox has no second local client, so every lock at this
    // point is stale; clear it before the first open.
    if let Err(error) = desktop.clear_stale_session_locks() {
        log::warn!("clear stale session locks failed: {error:#}");
    }
    {
        let mut state = shared.lock().unwrap();
        state.permission_mode = "full_auto".into();
        state.loading = true;
        state.touch();
    }

    refresh_sessions(&desktop, &shared, None);
    refresh_archived(&desktop, &shared);
    refresh_preset_questions(&desktop, &shared);
    refresh_ubuntu(&desktop, &shared);
    refresh_permissions(&shared);
    refresh_companion(&desktop, &shared);
    // The bundled runtime is part of the install experience: the first launch
    // after an APK install extracts it in the background instead of waiting for
    // the user to find the settings switch.
    #[cfg(target_os = "android")]
    maybe_auto_install_ubuntu(&desktop, &shared);

    let initial = shared
        .lock()
        .unwrap()
        .sessions
        .first()
        .map(|session| session.id.clone());
    let session = match initial {
        Some(id) => id.to_string(),
        None => {
            let workspace = shared.lock().unwrap().selected_workspace.clone();
            let created = desktop
                .create_session((!workspace.trim().is_empty()).then_some(workspace.as_str()))?;
            refresh_sessions(&desktop, &shared, Some(&created.id));
            created.id
        }
    };

    let generation = Arc::new(AtomicU64::new(1));
    let mut watch_cancel: Option<NativeThreadWatchCancel> = None;
    select_session(
        &desktop,
        &shared,
        &session,
        &generation,
        &mut watch_cancel,
        true,
    )?;

    for command in commands {
        match command {
            Command::Select(id) => {
                if let Err(error) = select_session(
                    &desktop,
                    &shared,
                    &id,
                    &generation,
                    &mut watch_cancel,
                    false,
                ) {
                    notify(&shared, format!("无法打开会话：{error}"));
                }
            }
            Command::NewThread => {
                if let Err(error) = open_thread_in_workspace(
                    &desktop,
                    &shared,
                    &generation,
                    &mut watch_cancel,
                    None,
                ) {
                    notify(&shared, format!("无法新建会话：{error}"));
                }
            }
            Command::Fork(round) => {
                let session = shared.lock().unwrap().session_id.clone();
                match desktop.fork_session(&session, round as i64) {
                    Ok(created) => {
                        refresh_sessions(&desktop, &shared, Some(&created.id));
                        if let Err(error) = select_session(
                            &desktop,
                            &shared,
                            &created.id,
                            &generation,
                            &mut watch_cancel,
                            false,
                        ) {
                            notify(&shared, format!("无法打开分叉会话：{error}"));
                        } else {
                            notify(&shared, "已创建分叉线程");
                        }
                    }
                    Err(error) => notify(&shared, format!("无法分叉会话：{error}")),
                }
            }
            Command::NewThreadInWorkspace(id) => {
                if let Err(error) = open_thread_in_workspace(
                    &desktop,
                    &shared,
                    &generation,
                    &mut watch_cancel,
                    Some(&id),
                ) {
                    notify(&shared, format!("无法在该工作区新建会话：{error}"));
                }
            }
            Command::RenameSession { id, title } => match desktop.rename_session(&id, &title) {
                Ok(()) => {
                    let active = shared.lock().unwrap().session_id.clone();
                    refresh_sessions(&desktop, &shared, Some(&active));
                    notify(&shared, "会话已重命名");
                }
                Err(error) => notify(&shared, format!("无法重命名会话：{error}")),
            },
            Command::ArchiveSession(id) => match desktop.archive_session(&id) {
                Ok(()) => {
                    let current = shared.lock().unwrap().session_id.clone();
                    refresh_archived(&desktop, &shared);
                    if current == id {
                        // The archived thread can no longer be selected; fall
                        // back to the newest remaining thread in the same
                        // workspace, or a fresh one there.
                        let workspace = shared
                            .lock()
                            .map(|state| state.selected_workspace.clone())
                            .unwrap_or_default();
                        let workspace_arg =
                            (!workspace.trim().is_empty()).then_some(workspace.as_str());
                        let next = desktop
                            .list_sessions(workspace_arg)
                            .ok()
                            .and_then(|list| list.first().map(|session| session.id.clone()));
                        let next = match next {
                            Some(id) => Some(id),
                            None => {
                                let workspace = shared.lock().unwrap().selected_workspace.clone();
                                desktop
                                    .create_session(
                                        (!workspace.trim().is_empty())
                                            .then_some(workspace.as_str()),
                                    )
                                    .ok()
                                    .map(|session| session.id)
                            }
                        };
                        match next {
                            Some(next) => {
                                if let Err(error) = select_session(
                                    &desktop,
                                    &shared,
                                    &next,
                                    &generation,
                                    &mut watch_cancel,
                                    false,
                                ) {
                                    notify(&shared, format!("无法打开会话：{error}"));
                                }
                            }
                            None => notify(&shared, "会话已归档，但没有可用会话"),
                        }
                    } else {
                        refresh_sessions(&desktop, &shared, Some(&current));
                    }
                    notify(&shared, "会话已归档");
                }
                Err(error) => notify(&shared, format!("无法归档会话：{error}")),
            },
            Command::RestoreSession(id) => match desktop.restore_session(&id) {
                Ok(()) => {
                    let active = shared.lock().unwrap().session_id.clone();
                    refresh_sessions(&desktop, &shared, Some(&active));
                    refresh_archived(&desktop, &shared);
                    notify(&shared, "会话已恢复");
                }
                Err(error) => notify(&shared, format!("无法恢复会话：{error}")),
            },
            Command::DeleteArchivedSession(id) => match desktop.delete_archived_session(&id) {
                Ok(()) => {
                    refresh_archived(&desktop, &shared);
                    notify(&shared, "归档会话已永久删除");
                }
                Err(error) => notify(&shared, format!("无法删除归档会话：{error}")),
            },
            Command::RefreshArchived => refresh_archived(&desktop, &shared),
            Command::LoadTrajectory { id, title } => {
                load_trajectory(&desktop, &shared, &id, &title);
            }
            Command::TrajectoryFilter(filter) => {
                let mut state = shared.lock().unwrap();
                state.trajectory_filter = filter.clamp(0, 3);
                state.trajectory_rev = state.trajectory_rev.wrapping_add(1);
                state.touch();
            }
            Command::LoadPromptPreview => load_prompt_preview(&desktop, &shared),
            Command::LoadWorkspace { path } => load_workspace(&desktop, &shared, &path),
            Command::LoadMoreWorkspace { path, offset } => {
                load_workspace_page(&desktop, &shared, &path, offset, true);
            }
            Command::RefreshWorkspace => {
                let path = shared.lock().unwrap().workspace_path.clone();
                load_workspace(&desktop, &shared, &path);
            }
            Command::RenameWorkspaceEntry { path, name } => {
                rename_workspace_entry(&desktop, &shared, &path, &name);
            }
            Command::MoveWorkspaceEntry { path, target_dir } => {
                move_workspace_entry(&desktop, &shared, &path, &target_dir);
            }
            Command::DeleteWorkspaceEntry { path } => {
                delete_workspace_entry(&desktop, &shared, &path);
            }
            Command::OpenWorkspaceFile { path } => {
                open_workspace_entry(&desktop, &shared, &path);
            }
            Command::OpenWorkspaceLocation { path, dir } => {
                open_workspace_location(&desktop, &shared, &path, dir);
            }
            Command::RefreshWorkspaces => {
                if let Err(error) = refresh_workspaces(&desktop, &shared) {
                    notify(&shared, format!("无法读取工作区：{error}"));
                }
            }
            Command::SelectWorkspace(id) => {
                match select_workspace(&desktop, &shared, &id) {
                    Ok(()) => {
                        let name = shared
                            .lock()
                            .map(|state| state.workspace.clone())
                            .unwrap_or_default();
                        // The phone header carries no thread title, so a
                        // selection that only moved the new-task target would
                        // look like the open thread moved with it. Open the
                        // workspace's newest thread instead; an empty
                        // workspace gets a fresh one.
                        let (current, target) = shared
                            .lock()
                            .map(|state| {
                                (state.workspace_id.clone(), state.selected_workspace.clone())
                            })
                            .unwrap_or_default();
                        if current != target {
                            let next = desktop.list_sessions(Some(target.as_str())).ok().and_then(
                                |sessions| sessions.first().map(|session| session.id.clone()),
                            );
                            let outcome = match next {
                                Some(session) => select_session(
                                    &desktop,
                                    &shared,
                                    &session,
                                    &generation,
                                    &mut watch_cancel,
                                    false,
                                ),
                                None => open_thread_in_workspace(
                                    &desktop,
                                    &shared,
                                    &generation,
                                    &mut watch_cancel,
                                    Some(target.as_str()),
                                ),
                            };
                            if let Err(error) = outcome {
                                notify(&shared, format!("无法打开该工作区的会话：{error}"));
                            }
                        }
                        if !name.is_empty() {
                            notify(&shared, format!("已切换到工作区「{name}」"));
                        }
                    }
                    Err(error) => notify(&shared, format!("无法切换工作区：{error}")),
                }
            }
            Command::CreateWorkspace {
                name,
                icon,
                color,
                folder,
            } => {
                if let Some(id) = create_workspace(&desktop, &shared, &name, &icon, &color, &folder)
                {
                    // A new workspace starts a thread so the drawer (filtered
                    // to the active workspace) is never left with no target
                    // while the previously open thread belongs elsewhere.
                    if let Err(error) = open_thread_in_workspace(
                        &desktop,
                        &shared,
                        &generation,
                        &mut watch_cancel,
                        Some(&id),
                    ) {
                        notify(&shared, format!("无法在该工作区新建会话：{error}"));
                    }
                }
            }
            Command::UpdateWorkspace {
                id,
                name,
                icon,
                color,
                folder,
            } => update_workspace(&desktop, &shared, &id, &name, &icon, &color, &folder),
            Command::DeleteWorkspace(id) => {
                delete_workspace(&desktop, &shared, &id, &generation, &mut watch_cancel);
            }
            Command::UploadWorkspace { dir, files } => {
                upload_workspace(&desktop, &shared, &dir, &files);
            }
            Command::SharedIntent { text, url, files } => {
                let has_text = !text.trim().is_empty();
                let has_url = !url.trim().is_empty();
                // Text/link shares land in the composer draft; nothing is
                // sent automatically. A file share carries an optional caption.
                let draft = if has_url {
                    Some(url)
                } else if has_text {
                    Some(text)
                } else {
                    None
                };
                if let Some(draft) = draft {
                    if let Ok(mut state) = shared.lock() {
                        state.pending_draft = Some(draft);
                        state.touch();
                    }
                }
                if files.is_empty() {
                    if has_text {
                        notify(&shared, "已把分享文本填入输入框，确认后发送");
                    } else if has_url {
                        notify(&shared, "已把分享链接填入输入框，确认后发送");
                    }
                } else {
                    notify(
                        &shared,
                        format!("收到 {} 个分享文件，正在加入当前工作区…", files.len()),
                    );
                    upload_workspace(&desktop, &shared, "", &files);
                }
            }
            Command::DownloadWorkspace { path } => download_workspace(&desktop, &shared, &path),
            Command::LoadMarkdownImage { source } => {
                start_markdown_image(&desktop, &shared, &sender, source)
            }
            Command::MarkdownImageLoaded { source, image } => {
                if let Ok(mut state) = shared.lock() {
                    state.images_inflight.remove(&source);
                    let incoming = image.as_ref().map(shared_image_bytes).unwrap_or(0);
                    let held: usize = state
                        .images
                        .values()
                        .flatten()
                        .map(shared_image_bytes)
                        .sum();
                    if state.images.len() >= IMAGE_CACHE_LIMIT
                        || held.saturating_add(incoming) > IMAGE_CACHE_BUDGET
                    {
                        state.images.clear();
                    }
                    state.images.insert(source, image);
                    state.images_rev = state.images_rev.wrapping_add(1);
                    state.touch();
                }
            }
            Command::WorkspaceSaved { ok, name } => {
                if ok {
                    notify(&shared, format!("已保存 {name}"));
                } else {
                    notify(&shared, format!("保存 {name} 失败"));
                }
            }
            Command::VoiceToggle => toggle_voice(&desktop, &shared),
            Command::VoiceStarted => set_voice_phase(&shared, 1),
            Command::VoiceFinished { path } => finish_voice(&desktop, &shared, path),
            Command::VoiceFailed { message } => {
                set_voice_phase(&shared, 0);
                notify(&shared, message);
            }
            Command::CaptureScreenshot => start_screenshot(&shared),
            Command::ScreenshotResult { status } => apply_screenshot(&desktop, &shared, &status),
            Command::Send(text) => {
                // Slash commands are local UI/engine commands, not model
                // messages. NativeDesktop::send_chat rejects them on purpose,
                // so route them here before opening a stream.
                if let Some(command) = NativeChatCommand::parse(&text) {
                    match command {
                        NativeChatCommand::Help => {
                            notify(
                                &shared,
                                "/new 新建线程 · /stop 停止生成 · /goal 目标 · /goal resume 继续 · /goal pause 暂停 · /goal clear 清除 · /compact 压缩上下文",
                            );
                        }
                        NativeChatCommand::New => {
                            if let Err(error) = open_thread_in_workspace(
                                &desktop,
                                &shared,
                                &generation,
                                &mut watch_cancel,
                                None,
                            ) {
                                notify(&shared, format!("无法新建会话：{error}"));
                            }
                        }
                        NativeChatCommand::Stop => stop_generation(&desktop, &shared),
                        compact @ NativeChatCommand::Compact => {
                            let busy = shared.lock().map(|state| state.generating).unwrap_or(false);
                            if busy {
                                notify(&shared, "请先停止当前任务再压缩上下文");
                            } else {
                                run_chat_command(desktop.clone(), shared.clone(), compact);
                            }
                        }
                        other => {
                            run_chat_command(desktop.clone(), shared.clone(), other);
                        }
                    }
                    continue;
                }
                let session = shared.lock().unwrap().session_id.clone();
                if session.is_empty() {
                    notify(&shared, "会话尚未就绪");
                    continue;
                }
                // A free-form reply while the panel waits is still an answer to
                // the waiting turn; settle it with the same inquiry identity.
                let inquiry_turn_id = take_inquiry_answer(&shared, &session, false)
                    .and_then(|(_text, turn_id)| turn_id);
                start_stream(
                    desktop.clone(),
                    shared.clone(),
                    session,
                    text,
                    inquiry_turn_id,
                );
            }
            Command::Stop => stop_generation(&desktop, &shared),
            Command::GoalEdit(text) => {
                run_chat_command(
                    desktop.clone(),
                    shared.clone(),
                    NativeChatCommand::GoalEdit(text),
                );
            }
            Command::GoalResume => {
                run_chat_command(
                    desktop.clone(),
                    shared.clone(),
                    NativeChatCommand::GoalResume,
                );
            }
            Command::GoalPause => {
                run_chat_command(
                    desktop.clone(),
                    shared.clone(),
                    NativeChatCommand::GoalPause,
                );
            }
            Command::GoalClear => {
                run_chat_command(
                    desktop.clone(),
                    shared.clone(),
                    NativeChatCommand::GoalClear,
                );
            }
            Command::SelectModel(key) => {
                let session = shared.lock().unwrap().session_id.clone();
                match desktop.set_session_model(&session, &key) {
                    Ok(()) => {
                        {
                            let mut state = shared.lock().unwrap();
                            state.model_key = key.clone();
                            state.model_name = key;
                            state.touch();
                        }
                        // The new model may have a different context window;
                        // drop the old capacity before re-reading the gauge.
                        reset_context_usage(&shared);
                        refresh_context_usage(&desktop, &shared, &session);
                    }
                    Err(error) => notify(&shared, format!("切换模型失败：{error}")),
                }
            }
            Command::RefreshSessions => {
                let active = shared.lock().unwrap().session_id.clone();
                refresh_sessions(&desktop, &shared, Some(&active));
                refresh_ubuntu(&desktop, &shared);
            }
            Command::LoadSettings => {
                let active = shared.lock().unwrap().session_id.clone();
                refresh_sessions(&desktop, &shared, Some(&active));
                refresh_ubuntu(&desktop, &shared);
                // The settings index shows agent/tool counts before either
                // page is opened, so load both projections with the sheet.
                refresh_agent(&desktop, &shared);
                refresh_tools(&desktop, &shared);
                refresh_cron(&desktop, &shared);
                refresh_memory(&desktop, &shared, true);
                crate::cloud::refresh_account(&desktop, &shared);
                crate::cloud::refresh_devices(&desktop, &shared);
                crate::channels::refresh(&desktop, &shared);
                refresh_permissions(&shared);
                refresh_backup(&shared);
            }
            Command::RefreshPermissions => refresh_permissions(&shared),
            Command::OpenPermission(id) => crate::permissions::open(&id),
            Command::CheckAndroidUpdate => start_update_check(desktop.clone(), shared.clone()),
            Command::DownloadAndroidUpdate => {
                start_update_download(desktop.clone(), shared.clone())
            }
            Command::InstallAndroidUpdate => install_update(&shared),
            Command::SetUpdateSource(source) => {
                let source = if source == "github" {
                    "github"
                } else {
                    "cloud"
                };
                match desktop.get_desktop_settings() {
                    Ok(settings) => match desktop.save_preferences(
                        &settings.theme,
                        &settings.send_key,
                        settings.font_size,
                        source,
                    ) {
                        Ok(_) => {
                            let active = shared.lock().unwrap().session_id.clone();
                            refresh_sessions(&desktop, &shared, Some(&active));
                            if let Ok(mut state) = shared.lock() {
                                // A source switch is an explicit request; do not
                                // reuse a check result from the previous source.
                                state.update_checked_at = None;
                            }
                            start_update_check(desktop.clone(), shared.clone());
                        }
                        Err(error) => notify(&shared, format!("切换更新源失败：{error}")),
                    },
                    Err(error) => notify(&shared, format!("读取更新源设置失败：{error}")),
                }
            }
            Command::RefreshTasks => {
                let session = shared.lock().unwrap().session_id.clone();
                let count = desktop.running_command_tasks(&session).len() as i32;
                if let Ok(mut state) = shared.lock() {
                    if state.running_task_count != count {
                        state.running_task_count = count;
                        state.touch();
                    }
                }
            }
            Command::RefreshUbuntu => refresh_ubuntu(&desktop, &shared),
            Command::InstallUbuntu => {
                if crate::ubuntu::snapshot()
                    .map(|s| s.installing())
                    .unwrap_or(false)
                {
                    notify(&shared, "Ubuntu 正在安装中");
                } else {
                    start_ubuntu_install(desktop.clone(), shared.clone());
                }
            }
            Command::SetUbuntuEnabled(enabled) => {
                let installed = crate::ubuntu::snapshot()
                    .map(|snapshot| snapshot.installed())
                    .unwrap_or(false);
                if enabled && !installed {
                    notify(&shared, "请先安装 Ubuntu 运行环境");
                    continue;
                }
                let shell = if enabled {
                    crate::ubuntu::shell_path()
                } else {
                    String::new()
                };
                match save_shell_path(&desktop, &shell) {
                    Ok(()) => {
                        refresh_ubuntu(&desktop, &shared);
                        notify(
                            &shared,
                            if enabled {
                                "命令将运行在 Ubuntu 中"
                            } else {
                                "命令已切回系统环境"
                            },
                        );
                    }
                    Err(error) => notify(&shared, format!("切换运行环境失败：{error}")),
                }
            }
            Command::RefreshCompanion => refresh_companion(&desktop, &shared),
            Command::SaveCompanion {
                enabled,
                selected_id,
                scale,
                message_hints,
            } => {
                let mut selected = selected_id.trim().to_string();
                if enabled && selected.is_empty() {
                    // 首次开启还没有形象：选库里的第一个，让开关即刻生效。
                    selected = desktop
                        .list_companions()
                        .ok()
                        .and_then(|list| list.first().map(|companion| companion.id.clone()))
                        .unwrap_or_default();
                }
                match desktop.save_companion_settings(enabled, &selected, scale, message_hints) {
                    Ok(_) => {
                        refresh_companion(&desktop, &shared);
                        if enabled && selected.is_empty() {
                            notify(&shared, "伙伴库为空，请先导入伙伴包");
                        }
                        #[cfg(target_os = "android")]
                        if enabled
                            && !selected.is_empty()
                            && crate::companion::permission_status() != "granted"
                        {
                            crate::companion::request_permission();
                        }
                    }
                    Err(error) => notify(&shared, format!("保存伙伴设置失败：{error}")),
                }
            }
            Command::CompanionPermission => {
                #[cfg(target_os = "android")]
                crate::companion::request_permission();
            }
            Command::InquiryOption { row, option } => {
                let session = shared.lock().unwrap().session_id.clone();
                mutate_inquiry(&shared, &session, |panel| {
                    let index = panel.question_index(row);
                    panel.toggle_option(index, option.max(0) as usize);
                });
            }
            Command::InquiryNoPreference { row } => {
                let session = shared.lock().unwrap().session_id.clone();
                mutate_inquiry(&shared, &session, |panel| {
                    let index = panel.question_index(row);
                    panel.toggle_no_preference(index);
                });
            }
            Command::InquiryUseRecommended { row } => {
                let session = shared.lock().unwrap().session_id.clone();
                mutate_inquiry(&shared, &session, |panel| {
                    let index = panel.question_index(row);
                    panel.use_recommended(index);
                });
            }
            Command::InquiryOther { row, text } => {
                let session = shared.lock().unwrap().session_id.clone();
                // Free text lands without a model republish: the TextInput
                // keeps the caret and the next selection change carries it.
                if let Ok(mut state) = shared.lock() {
                    if state.session_id != session {
                        continue;
                    }
                    let Some(panel) = state.inquiry.as_mut() else {
                        continue;
                    };
                    let index = panel.question_index(row);
                    if let Some(question) = panel.questions.get_mut(index) {
                        question.other = text;
                        if !question.other.trim().is_empty() {
                            question.no_preference = false;
                        }
                    }
                    state.touch();
                }
            }
            Command::InquiryPageMove(delta) => {
                let session = shared.lock().unwrap().session_id.clone();
                mutate_inquiry(&shared, &session, |panel| panel.move_page(delta));
            }
            Command::InquiryExpandToggle => {
                let session = shared.lock().unwrap().session_id.clone();
                mutate_inquiry(&shared, &session, |panel| {
                    panel.expanded = !panel.expanded;
                });
            }
            Command::InquirySubmit => {
                let session = shared.lock().unwrap().session_id.clone();
                if session.is_empty() {
                    notify(&shared, "会话尚未就绪");
                    continue;
                }
                let answer = take_inquiry_answer(&shared, &session, true);
                match answer {
                    Some((text, turn_id)) => {
                        start_stream(desktop.clone(), shared.clone(), session, text, turn_id);
                    }
                    None => {}
                }
            }
            Command::SavePreferences {
                theme,
                send_key,
                font_size,
            } => {
                let update_source = desktop
                    .get_desktop_settings()
                    .map(|settings| settings.update_source)
                    .unwrap_or_else(|_| "cloud".into());
                match desktop.save_preferences(&theme, &send_key, font_size, &update_source) {
                    Ok(_) => {
                        let active = shared.lock().unwrap().session_id.clone();
                        refresh_sessions(&desktop, &shared, Some(&active));
                    }
                    Err(error) => notify(&shared, format!("保存外观设置失败：{error}")),
                }
            }
            Command::SaveLanguage(language) => match desktop.get_desktop_settings() {
                Ok(settings) => match desktop.save_runtime(
                    &settings.workspace_root,
                    &language,
                    &settings.python_path,
                    &settings.git_path,
                    &settings.rg_path,
                    &settings.ffmpeg_path,
                    &settings.browser_path,
                    &settings.shell_path,
                ) {
                    Ok(_) => {
                        let active = shared.lock().unwrap().session_id.clone();
                        refresh_sessions(&desktop, &shared, Some(&active));
                        notify(&shared, "语言已更新");
                    }
                    Err(error) => notify(&shared, format!("保存语言失败：{error}")),
                },
                Err(error) => notify(&shared, format!("读取配置失败：{error}")),
            },
            Command::PermissionMode(mode) => {
                let label = match mode.as_str() {
                    "auto_edit" => "写入自动",
                    "suggest" => "执行前确认",
                    _ => "完全访问",
                };
                if let Ok(mut state) = shared.lock() {
                    state.permission_mode = mode;
                    state.notify(format!("权限模式已设为「{label}」"));
                    state.touch();
                }
            }
            Command::OpenModelEditor { key } => {
                open_model_editor(&desktop, &shared, key.as_deref());
            }
            // ---- tool manager -----------------------------------------
            Command::RefreshTools => refresh_tools(&desktop, &shared),
            Command::SelectToolServer { index } => {
                select_tool_server(&desktop, &shared, index);
            }
            Command::SelectToolResource { tab, index } => {
                select_tool_resource(&desktop, &shared, tab, index);
            }
            Command::LoadToolDirectory { directory } => {
                load_tool_directory(&desktop, &shared, &directory);
            }
            Command::MoreToolFiles => more_tool_files(&desktop, &shared),
            Command::OpenToolFile { path } => open_tool_file(&desktop, &shared, &path),
            Command::SaveToolFile { path, content } => {
                save_tool_file(&desktop, &shared, &path, &content);
            }
            Command::CloseToolFile => close_tool_file(&shared),
            Command::CreateToolFile { name } => create_tool_file(&desktop, &shared, &name),
            Command::UploadToolFiles { files } => upload_tool_files(&desktop, &shared, &files),
            Command::DeleteToolResource { tab, name } => {
                delete_tool_resource(&desktop, &shared, tab, &name);
            }
            Command::ConnectToolServer { name } => {
                let sender = sender.clone();
                let api = desktop.clone();
                thread::spawn(move || {
                    let error = api
                        .connect_tool_server(&name)
                        .err()
                        .map(|error| format!("{error}"))
                        .unwrap_or_default();
                    let _ = sender.send(Command::ToolServerConnected { name, error });
                });
            }
            Command::ToolServerConnected { name, error } => {
                if error.is_empty() {
                    notify(&shared, "MCP 服务已连接");
                    refresh_tools(&desktop, &shared);
                    load_server_tools(&desktop, &shared, &name);
                } else {
                    notify(&shared, format!("连接失败：{error}"));
                }
            }
            Command::ExportSkill { name } => export_skill(&desktop, &shared, &name),
            Command::ImportSkill { path } => {
                let sender = sender.clone();
                let api = desktop.clone();
                thread::spawn(move || {
                    let error = api
                        .import_tool_resource(ToolResourceKind::Skill, &path)
                        .err()
                        .map(|error| format!("{error}"))
                        .unwrap_or_default();
                    let _ = sender.send(Command::SkillImported { error });
                });
            }
            Command::SkillImported { error } => {
                if error.is_empty() {
                    notify(&shared, "技能已导入");
                    refresh_tools(&desktop, &shared);
                } else {
                    notify(&shared, format!("导入技能失败：{error}"));
                }
            }
            Command::OpenToolEditor { kind, index } => {
                open_tool_editor(&desktop, &shared, kind, index);
            }
            Command::CloseToolEditor => close_tool_editor(&shared),
            Command::SaveToolEditor { kind, mcp, kb } => {
                save_tool_editor(&desktop, &shared, kind, mcp, kb);
            }
            // ---- built-in agent ---------------------------------------
            Command::RefreshAgent => refresh_agent(&desktop, &shared),
            Command::AgentToolSearch(query) => search_agent_tools(&shared, &query),
            Command::SaveAgentText {
                description,
                system_prompt,
            } => {
                let (tools, questions) = agent_selection(&shared);
                persist_agent(
                    &desktop,
                    &shared,
                    description,
                    system_prompt,
                    tools,
                    questions,
                    "智能体设置已保存",
                );
            }
            Command::ToggleAgentTool {
                name,
                enabled,
                description,
                system_prompt,
            } => {
                let (mut tools, questions) = agent_selection(&shared);
                tools.retain(|tool| tool != &name);
                if enabled {
                    tools.push(name);
                }
                persist_agent(
                    &desktop,
                    &shared,
                    description,
                    system_prompt,
                    tools,
                    questions,
                    "工具已更新",
                );
            }
            Command::ToggleAgentToolGroup {
                title,
                select,
                description,
                system_prompt,
            } => {
                let (mut tools, questions) = agent_selection(&shared);
                let category = category_for_group(&title);
                match desktop.list_tools() {
                    Ok(catalog) => {
                        let names = catalog
                            .iter()
                            .filter(|tool| tool.category == category)
                            .map(|tool| tool.name.clone())
                            .collect::<Vec<_>>();
                        tools.retain(|tool| !names.contains(tool));
                        if select {
                            tools.extend(names);
                        }
                        persist_agent(
                            &desktop,
                            &shared,
                            description,
                            system_prompt,
                            tools,
                            questions,
                            "工具已更新",
                        );
                    }
                    Err(_) => notify(&shared, "无法读取工具列表"),
                }
            }
            Command::AddAgentQuestion {
                question,
                description,
                system_prompt,
            } => {
                let (tools, mut questions) = agent_selection(&shared);
                let question = question.trim().to_string();
                if !question.is_empty()
                    && questions.len() < 12
                    && !questions.iter().any(|existing| existing == &question)
                {
                    questions.push(question);
                    persist_agent(
                        &desktop,
                        &shared,
                        description,
                        system_prompt,
                        tools,
                        questions,
                        "预设问题已添加",
                    );
                }
            }
            Command::RemoveAgentQuestion {
                index,
                description,
                system_prompt,
            } => {
                let (tools, mut questions) = agent_selection(&shared);
                let index = index.max(0) as usize;
                if index < questions.len() {
                    questions.remove(index);
                    persist_agent(
                        &desktop,
                        &shared,
                        description,
                        system_prompt,
                        tools,
                        questions,
                        "预设问题已删除",
                    );
                }
            }
            // ---- cron jobs --------------------------------------------
            Command::RefreshCron => refresh_cron(&desktop, &shared),
            Command::ToggleCron { id, enabled } => match desktop.toggle_cron_job(&id, enabled) {
                Ok(()) => {
                    refresh_cron(&desktop, &shared);
                    notify(
                        &shared,
                        if enabled {
                            "定时任务已启用"
                        } else {
                            "定时任务已停用"
                        },
                    );
                }
                Err(error) => {
                    // The switch moved before the command reached us; reload
                    // the list so the row reflects the persisted state.
                    refresh_cron(&desktop, &shared);
                    notify(&shared, format!("无法更新定时任务：{error}"));
                }
            },
            Command::DeleteCron { id } => match desktop.delete_cron_job(&id) {
                Ok(()) => {
                    if let Ok(mut state) = shared.lock() {
                        state.cron_editor = None;
                        state.cron_editor_rev = state.cron_editor_rev.wrapping_add(1);
                        state.cron_runs = CronRunsView::default();
                        state.cron_runs_rev = state.cron_runs_rev.wrapping_add(1);
                    }
                    refresh_cron(&desktop, &shared);
                    notify(&shared, "定时任务已删除");
                }
                Err(error) => notify(&shared, format!("无法删除定时任务：{error}")),
            },
            Command::RunCronNow { id } => match desktop.run_cron_job_now(&id) {
                Ok(status) => {
                    notify(
                        &shared,
                        if status == "running" {
                            "任务正在运行中，未重复排队"
                        } else {
                            "已加入立即执行队列"
                        },
                    );
                    refresh_cron(&desktop, &shared);
                }
                Err(error) => notify(&shared, format!("无法立即运行任务：{error}")),
            },
            Command::OpenCronRuns { id } => open_cron_runs(&desktop, &shared, &id),
            Command::CloseCronRuns => close_cron_runs(&shared),
            Command::OpenCronEditor { id } => {
                open_cron_editor(&desktop, &shared, id.as_deref());
            }
            Command::CloseCronEditor => close_cron_editor(&shared),
            Command::SaveCron { draft } => save_cron(&desktop, &shared, draft),
            // ---- memory fragments -------------------------------------
            Command::RefreshMemory => refresh_memory(&desktop, &shared, true),
            Command::MemorySearch(query) => {
                if let Ok(mut state) = shared.lock() {
                    state.memory.query = query;
                }
                refresh_memory(&desktop, &shared, false);
            }
            Command::MemoryCategory(category) => {
                if let Ok(mut state) = shared.lock() {
                    state.memory.category = category;
                }
                refresh_memory(&desktop, &shared, false);
            }
            Command::OpenMemoryEditor { index } => {
                open_memory_editor(&shared, index);
            }
            Command::CloseMemoryEditor => close_memory_editor(&shared),
            Command::SaveMemory { draft } => save_memory(&desktop, &shared, draft),
            Command::DeleteMemory { id } => delete_memory(&desktop, &shared, &id),
            // ---- cloud account / device switch / cloud chat -----------
            Command::SwitchChatTarget(target) => {
                crate::cloud::switch_target(&desktop, &shared, target);
            }
            Command::RefreshCloudAccount => crate::cloud::refresh_account(&desktop, &shared),
            Command::RefreshUsage => refresh_usage(&desktop, &shared),
            Command::RefreshCloudRemote => {
                crate::cloud::refresh_remote_account(&desktop, &shared);
            }
            Command::CloudLogin {
                server,
                username,
                password,
            } => crate::cloud::login(&desktop, &shared, &server, &username, &password),
            Command::CloudLogout => crate::cloud::logout(&desktop, &shared),
            Command::CloudSetSync(enabled) => {
                crate::cloud::set_sync(&desktop, &shared, enabled);
            }
            Command::CloudSetSharing {
                enabled,
                full_shadow,
            } => crate::cloud::set_sharing(&desktop, &shared, enabled, full_shadow),
            Command::RefreshCloudThreads => crate::cloud::refresh_threads(&desktop, &shared),
            Command::CloudNewThread => crate::cloud::new_thread(&desktop, &shared),
            Command::CloudFork(round) => crate::cloud::fork(&desktop, &shared, round),
            Command::CloudSelectThread(id) => {
                crate::cloud::select_thread(&desktop, &shared, &id);
            }
            Command::CloudSend(text) => {
                if let Some(command) = NativeChatCommand::parse(&text) {
                    match command {
                        NativeChatCommand::Help => notify(
                            &shared,
                            "云端模式：/new 新建会话 · /stop 停止生成；/goal、/compact 请切到本机会话使用",
                        ),
                        NativeChatCommand::New => crate::cloud::new_thread(&desktop, &shared),
                        NativeChatCommand::Stop => crate::cloud::stop(&desktop, &shared),
                        _ => notify(&shared, "云端模式暂不支持该命令，请切到本机会话使用"),
                    }
                    continue;
                }
                crate::cloud::send(&desktop, &shared, text)
            }
            Command::CloudStop => crate::cloud::stop(&desktop, &shared),
            Command::CloudPoll => crate::cloud::poll(&desktop, &shared),
            Command::CloudRename { id, title } => {
                crate::cloud::rename(&desktop, &shared, &id, &title);
            }
            Command::CloudArchive(id) => crate::cloud::archive(&desktop, &shared, &id),
            Command::RefreshDevices => crate::cloud::refresh_devices(&desktop, &shared),
            Command::PollApprovals => crate::cloud::poll_approvals(&desktop, &shared),
            Command::DecideApproval { id, approve } => {
                crate::cloud::decide_approval(&desktop, &shared, &id, approve);
            }
            Command::RefreshChannels => crate::channels::refresh(&desktop, &shared),
            Command::SelectChannelAccount(id) => {
                crate::channels::select(&desktop, &shared, &id);
            }
            Command::OpenChannelEditor(id) => {
                crate::channels::open_editor(&desktop, &shared, &id);
            }
            Command::CloseChannelEditor => crate::channels::close_editor(&shared),
            Command::SaveChannel(draft) => crate::channels::save(&desktop, &shared, draft),
            Command::ToggleChannel {
                account_id,
                enabled,
            } => crate::channels::toggle(&desktop, &shared, &account_id, enabled),
            Command::DeleteChannel(id) => crate::channels::delete(&desktop, &shared, &id),
            Command::OpenChannelLogs(id) => crate::channels::open_logs(&desktop, &shared, &id),
            Command::CloseChannelLogs => crate::channels::close_logs(&shared),
            Command::DeleteChannelBinding(id) => {
                crate::channels::delete_binding(&desktop, &shared, &id);
            }
            Command::CloseModelEditor => {
                if let Ok(mut state) = shared.lock() {
                    state.editor = None;
                    state.editor_rev = state.editor_rev.wrapping_add(1);
                    state.touch();
                }
            }
            Command::SaveModel { card, secret } => {
                let original_key = shared
                    .lock()
                    .ok()
                    .and_then(|state| {
                        state
                            .editor
                            .as_ref()
                            .map(|editor| editor.original_key.clone())
                    })
                    .unwrap_or_default();
                let edit = model_edit(&card, &secret, &original_key);
                match desktop.save_model(edit) {
                    Ok(_) => {
                        // The switch in the editor is applied in the same save
                        // instead of forcing a second visit to the model row.
                        let default_error = if card.is_default {
                            desktop
                                .set_default_model(card.key.as_str())
                                .err()
                                .map(|error| format!("{error}"))
                        } else {
                            None
                        };
                        // Stay in the editor after saving: the user can keep
                        // tuning or set the default without losing the page.
                        let actual_default = if card.is_default && default_error.is_none() {
                            true
                        } else {
                            desktop
                                .get_desktop_settings()
                                .ok()
                                .and_then(|settings| {
                                    settings
                                        .models
                                        .iter()
                                        .find(|model| model.key == card.key.as_str())
                                        .map(|model| model.is_default)
                                })
                                .unwrap_or(false)
                        };
                        {
                            let mut state = shared.lock().unwrap();
                            if let Some(editor) = state.editor.as_mut() {
                                editor.original_key = card.key.to_string();
                                let mut saved_card = card.clone();
                                saved_card.is_default = actual_default;
                                editor.card = saved_card;
                                editor.is_default = actual_default;
                                editor.error.clear();
                                editor.probing = false;
                            }
                            state.editor_rev = state.editor_rev.wrapping_add(1);
                            state.notify(match &default_error {
                                Some(error) => format!("模型已保存，但设为默认失败：{error}"),
                                None => "模型已保存".to_string(),
                            });
                            state.touch();
                        }
                        let active = shared.lock().unwrap().session_id.clone();
                        refresh_sessions(&desktop, &shared, Some(&active));
                    }
                    Err(error) => {
                        if let Ok(mut state) = shared.lock() {
                            if let Some(editor) = state.editor.as_mut() {
                                editor.error = format!("{error:#}");
                                state.editor_rev = state.editor_rev.wrapping_add(1);
                                state.touch();
                            }
                        }
                    }
                }
            }
            Command::ProbeModel { card, secret } => {
                let original_key = shared
                    .lock()
                    .ok()
                    .and_then(|state| {
                        state
                            .editor
                            .as_ref()
                            .map(|editor| editor.original_key.clone())
                    })
                    .unwrap_or_default();
                if let Ok(mut state) = shared.lock() {
                    if let Some(editor) = state.editor.as_mut() {
                        editor.probing = true;
                        editor.probe_message.clear();
                    }
                    state.editor_rev = state.editor_rev.wrapping_add(1);
                    state.touch();
                }
                probe_model(desktop.clone(), shared.clone(), card, secret, original_key);
            }
            Command::DeleteModel(key) => match desktop.delete_model(&key) {
                Ok(_) => {
                    if let Ok(mut state) = shared.lock() {
                        if state
                            .editor
                            .as_ref()
                            .map(|editor| editor.original_key.as_str())
                            == Some(key.as_str())
                        {
                            state.editor = None;
                            state.editor_rev = state.editor_rev.wrapping_add(1);
                        }
                        state.notify("模型已删除");
                        state.touch();
                    }
                    let active = shared.lock().unwrap().session_id.clone();
                    refresh_sessions(&desktop, &shared, Some(&active));
                }
                Err(error) => notify(&shared, format!("删除模型失败：{error}")),
            },
            Command::RefreshBackup => refresh_backup(&shared),
            Command::BackupNow => start_backup(&shared),
            Command::RestoreBackup { path } => start_restore(&shared, path),
            Command::LoadCrashLog => load_crash_log(&shared),
        }
    }
    if let Some(cancel) = watch_cancel.take() {
        cancel.cancel();
    }
    Ok(())
}

fn notify(shared: &Arc<Mutex<Shared>>, message: impl Into<String>) {
    if let Ok(mut state) = shared.lock() {
        state.notify(message);
        state.touch();
    }
}

fn stop_generation(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let session = shared.lock().unwrap().session_id.clone();
    if session.is_empty() {
        notify(shared, "会话尚未就绪");
        return;
    }
    if let Err(error) = desktop.cancel_chat(&session) {
        notify(shared, format!("停止失败：{error}"));
    }
    if let Ok(mut state) = shared.lock() {
        state.stopping = true;
        state.touch();
    }
}

/// Goal/compaction commands do real engine work, so they run on a worker
/// thread and report their outcome through the existing toast surface.
fn run_chat_command(
    desktop: Arc<NativeDesktop>,
    shared: Arc<Mutex<Shared>>,
    command: NativeChatCommand,
) {
    let session = shared.lock().unwrap().session_id.clone();
    if session.is_empty() {
        notify(&shared, "请先进入一个会话后再使用命令");
        return;
    }
    let spawned = thread::Builder::new()
        .name("wunder-android-command".into())
        .spawn(move || {
            match desktop.execute_chat_command(&session, command) {
                Ok(message) => notify(&shared, message),
                Err(error) => notify(&shared, format!("命令执行失败：{error}")),
            }
            let active = session;
            refresh_sessions(&desktop, &shared, Some(&active));
        });
    if let Err(error) = spawned {
        log::error!("spawn android chat command failed: {error}");
    }
}

fn mutate_inquiry(
    shared: &Arc<Mutex<Shared>>,
    session: &str,
    change: impl FnOnce(&mut InquiryView),
) {
    let Ok(mut state) = shared.lock() else {
        return;
    };
    if state.session_id != session {
        return;
    }
    let Some(panel) = state.inquiry.as_mut() else {
        return;
    };
    change(panel);
    state.inquiry_rev = state.inquiry_rev.wrapping_add(1);
    state.touch();
}

fn remember_consumed_inquiry(state: &mut Shared, key: String) {
    if key.is_empty() || state.consumed_inquiries.iter().any(|item| item == &key) {
        return;
    }
    state.consumed_inquiries.push_back(key);
    while state.consumed_inquiries.len() > 64 {
        state.consumed_inquiries.pop_front();
    }
}

/// Remove the panel from the window and return the reply plus the waiting
/// execution identity. `require_ready` keeps the panel open when the user has
/// not answered every question; a free-form send instead settles it as-is.
fn take_inquiry_answer(
    shared: &Arc<Mutex<Shared>>,
    session: &str,
    require_ready: bool,
) -> Option<(String, Option<String>)> {
    let Ok(mut state) = shared.lock() else {
        return None;
    };
    if state.session_id != session {
        return None;
    }
    let Some(panel) = state.inquiry.take() else {
        return None;
    };
    if require_ready && !panel.ready() {
        state.inquiry = Some(panel);
        state.notify("请先回答全部问题");
        state.touch();
        return None;
    }
    let key = panel.consumption_key(session);
    remember_consumed_inquiry(&mut state, key);
    state.inquiry_rev = state.inquiry_rev.wrapping_add(1);
    state.touch();
    let turn_id = (!panel.turn_id.trim().is_empty()).then(|| panel.turn_id.clone());
    Some((panel.reply(), turn_id))
}

fn restore_pending_inquiry(state: &mut Shared, session: &str) {
    if state.session_id != session || state.inquiry.is_some() {
        return;
    }
    let panel = pending_inquiry(&state.durable_turns, session, &state.consumed_inquiries);
    if let Some(panel) = panel {
        state.inquiry = Some(panel);
        state.inquiry_rev = state.inquiry_rev.wrapping_add(1);
        state.touch();
    }
}

fn pending_inquiry(
    turns: &[NativeChatTurn],
    session: &str,
    consumed: &std::collections::VecDeque<String>,
) -> Option<InquiryView> {
    let panel = turns
        .iter()
        .rev()
        .find_map(|turn| turn.question_panel.as_ref())?;
    let mut view = InquiryView::from_payload(panel)?;
    if view.turn_id.is_empty() {
        view.turn_id = panel
            .get("turn_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
    }
    let key = view.consumption_key(session);
    (!consumed.iter().any(|item| item == &key)).then_some(view)
}

/// Publish a live `question_panel` / `question_panel_clear` frame. The event
/// envelope holds the waiting execution id outside the panel payload.
fn apply_inquiry_event(shared: &Arc<Mutex<Shared>>, value: &Value, session: &str) {
    let raw = value
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let envelope = value.get("data").unwrap_or(value);
    match raw {
        "question_panel" => {
            let data = envelope.get("data").unwrap_or(envelope);
            let Some(mut panel) = InquiryView::from_payload(data) else {
                return;
            };
            if panel.turn_id.is_empty() {
                panel.turn_id = envelope
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .or_else(|| data.get("turn_id").and_then(Value::as_str))
                    .unwrap_or_default()
                    .to_string();
            }
            let Ok(mut state) = shared.lock() else {
                return;
            };
            if state.session_id != session {
                return;
            }
            let key = panel.consumption_key(session);
            if state.consumed_inquiries.iter().any(|item| item == &key) {
                return;
            }
            state.inquiry = Some(panel);
            state.inquiry_rev = state.inquiry_rev.wrapping_add(1);
            state.touch();
        }
        "question_panel_clear" => {
            let Ok(mut state) = shared.lock() else {
                return;
            };
            if state.session_id != session || state.inquiry.is_none() {
                return;
            }
            state.inquiry = None;
            state.inquiry_rev = state.inquiry_rev.wrapping_add(1);
            state.touch();
        }
        _ => {}
    }
}

/// Read the Java snapshot into the shared view. `enabled` is derived from the
/// persisted shell path rather than a separate flag, so the toggle always
/// reflects what the command tool would actually spawn.
fn refresh_ubuntu(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let snapshot = crate::ubuntu::snapshot();
    let shell_path = crate::ubuntu::shell_path();
    let settings = desktop.get_desktop_settings().ok();
    let Ok(mut state) = shared.lock() else {
        return;
    };
    match snapshot {
        Some(snapshot) => {
            state.ubuntu.state = snapshot.state;
            state.ubuntu.progress = snapshot.progress;
            state.ubuntu.stage = snapshot.stage;
            state.ubuntu.version = snapshot.version;
            state.ubuntu.error = snapshot.error;
        }
        None => {
            state.ubuntu.state = "unsupported".into();
            state.ubuntu.stage = "当前构建不支持 Ubuntu 运行环境".into();
        }
    }
    state.ubuntu.shell_path = shell_path;
    state.ubuntu.enabled = settings
        .as_ref()
        .map(|settings| {
            let configured = settings.shell_path.trim();
            !configured.is_empty() && configured == state.ubuntu.shell_path
        })
        .unwrap_or(false);
    state.ubuntu_rev = state.ubuntu_rev.wrapping_add(1);
    state.touch();
}

/// Refresh the settings page's permission catalog from the Android host.
fn refresh_permissions(shared: &Arc<Mutex<Shared>>) {
    let items = crate::permissions::snapshot();
    let summary = crate::permissions::summary(&items);
    if let Ok(mut state) = shared.lock() {
        state.permission_summary = summary;
        state.permissions = items;
        state.permissions_rev = state.permissions_rev.wrapping_add(1);
        state.touch();
    }
}

/// Refresh the newest local archive shown by 设置 → 数据与恢复.
#[cfg(target_os = "android")]
fn refresh_backup(shared: &Arc<Mutex<Shared>>) {
    let latest = crate::backup::latest_backup()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Ok(mut state) = shared.lock() {
        if state.backup_path != latest {
            state.backup_path = latest;
            state.backup_rev = state.backup_rev.wrapping_add(1);
            state.touch();
        }
    }
}

#[cfg(not(target_os = "android"))]
fn refresh_backup(_shared: &Arc<Mutex<Shared>>) {}

/// Package config and databases on a worker; the engine loop keeps serving.
#[cfg(target_os = "android")]
fn start_backup(shared: &Arc<Mutex<Shared>>) {
    {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        if state.backup_phase == "busy" {
            state.notify("备份或恢复正在进行…");
            state.touch();
            return;
        }
        state.backup_phase = "busy".into();
        state.backup_message = "正在打包 config 与本地数据…".into();
        state.backup_rev = state.backup_rev.wrapping_add(1);
        state.touch();
    }
    let worker_shared = shared.clone();
    let spawned = thread::Builder::new()
        .name("wunder-android-backup".into())
        .spawn(move || {
            let result = crate::backup::backup();
            if let Ok(mut state) = worker_shared.lock() {
                match result {
                    Ok(path) => {
                        let path = path.to_string_lossy().into_owned();
                        state.backup_phase = "done".into();
                        state.backup_message = format!("已备份到 {path}");
                        state.backup_path = path;
                        state.notify("本地备份已完成");
                    }
                    Err(error) => {
                        state.backup_phase = "error".into();
                        state.backup_message = format!("备份失败：{error:#}");
                        state.notify(format!("备份失败：{error}"));
                    }
                }
                state.backup_rev = state.backup_rev.wrapping_add(1);
                state.touch();
            }
        });
    if let Err(error) = spawned {
        log::warn!("start the backup worker: {error}");
        if let Ok(mut state) = shared.lock() {
            state.backup_phase = "error".into();
            state.backup_message = format!("无法启动备份：{error}");
            state.backup_rev = state.backup_rev.wrapping_add(1);
            state.touch();
        }
    }
}

#[cfg(not(target_os = "android"))]
fn start_backup(shared: &Arc<Mutex<Shared>>) {
    notify(shared, "当前构建不支持本地备份");
}

/// Validate and stage an archive; the engine applies it on the next start.
#[cfg(target_os = "android")]
fn start_restore(shared: &Arc<Mutex<Shared>>, path: String) {
    if path.trim().is_empty() {
        notify(shared, "请先指定备份文件路径");
        return;
    }
    {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        if state.backup_phase == "busy" {
            state.notify("备份或恢复正在进行…");
            state.touch();
            return;
        }
        state.backup_phase = "busy".into();
        state.backup_message = "正在校验并解包备份…".into();
        state.backup_rev = state.backup_rev.wrapping_add(1);
        state.touch();
    }
    let worker_shared = shared.clone();
    let spawned = thread::Builder::new()
        .name("wunder-android-restore".into())
        .spawn(move || {
            let result = crate::backup::restore(&path);
            if let Ok(mut state) = worker_shared.lock() {
                match result {
                    Ok(crate::backup::RestoreOutcome::Restart) => {
                        state.backup_phase = "done".into();
                        state.backup_restart = true;
                        state.backup_message = "恢复已就绪，重启应用后生效".into();
                        state.notify("恢复已就绪，请重启应用");
                    }
                    Err(error) => {
                        state.backup_phase = "error".into();
                        state.backup_message = format!("恢复失败：{error:#}");
                        state.notify(format!("恢复失败：{error}"));
                    }
                }
                state.backup_rev = state.backup_rev.wrapping_add(1);
                state.touch();
            }
        });
    if let Err(error) = spawned {
        log::warn!("start the restore worker: {error}");
        if let Ok(mut state) = shared.lock() {
            state.backup_phase = "error".into();
            state.backup_message = format!("无法启动恢复：{error}");
            state.backup_rev = state.backup_rev.wrapping_add(1);
            state.touch();
        }
    }
}

#[cfg(not(target_os = "android"))]
fn start_restore(shared: &Arc<Mutex<Shared>>, _path: String) {
    notify(shared, "当前构建不支持本地恢复");
}

/// Read the newest Java or Rust crash report for the settings page.
#[cfg(target_os = "android")]
fn load_crash_log(shared: &Arc<Mutex<Shared>>) {
    match crate::crash::latest_report() {
        Some(report) => {
            if let Ok(mut state) = shared.lock() {
                state.crash_log_title = report.name;
                state.crash_log_text = report.text;
                state.backup_rev = state.backup_rev.wrapping_add(1);
                state.touch();
            }
        }
        None => notify(shared, "没有找到崩溃日志"),
    }
}

#[cfg(not(target_os = "android"))]
fn load_crash_log(shared: &Arc<Mutex<Shared>>) {
    notify(shared, "当前构建不支持查看崩溃日志");
}

fn start_update_check(desktop: Arc<NativeDesktop>, shared: Arc<Mutex<Shared>>) {
    let channel = {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        if matches!(
            state.update_phase.as_str(),
            "checking" | "downloading" | "installing"
        ) {
            return;
        }
        if state
            .update_checked_at
            .is_some_and(|checked| checked.elapsed() < Duration::from_secs(5))
        {
            return;
        }
        state.update_phase = "checking".into();
        state.update_error.clear();
        state.update_checked_at = Some(Instant::now());
        state.update_rev = state.update_rev.wrapping_add(1);
        state.touch();
        if state.settings.update_source == "github" {
            "github".to_string()
        } else {
            "cloud".to_string()
        }
    };
    let worker_shared = shared.clone();
    let check_channel = channel.clone();
    let spawned = thread::Builder::new()
        .name("wunder-android-update-check".into())
        .spawn(move || {
            let outcome = crate::update::check_offer(&desktop, &check_channel);
            if let Ok(mut state) = worker_shared.lock() {
                match outcome {
                    Ok(crate::update::CheckOutcome::Available(offer)) => {
                        state.update_phase = "available".into();
                        state.update_version = offer.release.version.clone();
                        state.update_notes = offer.notes.clone();
                        state.update_source = offer.source.clone();
                        state.update_size_text = offer.size_text.clone();
                        state.update_size_bytes = offer.release.size;
                        state.update_progress_bytes.store(0, Ordering::Relaxed);
                        state.update_error.clear();
                        state.update_offer = Some(offer);
                        state.update_path = None;
                    }
                    Ok(crate::update::CheckOutcome::UpToDate) => {
                        state.update_phase = "up_to_date".into();
                        state.update_version.clear();
                        state.update_notes.clear();
                        state.update_source = crate::update::source_label(&check_channel);
                        state.update_size_text.clear();
                        state.update_size_bytes = 0;
                        state.update_progress_bytes.store(0, Ordering::Relaxed);
                        state.update_error.clear();
                        state.update_offer = None;
                        state.update_path = None;
                    }
                    Ok(crate::update::CheckOutcome::NoAsset) => {
                        state.update_phase = "no_asset".into();
                        state.update_version.clear();
                        state.update_notes.clear();
                        state.update_source = crate::update::source_label(&check_channel);
                        state.update_size_text.clear();
                        state.update_size_bytes = 0;
                        state.update_progress_bytes.store(0, Ordering::Relaxed);
                        state.update_error.clear();
                        state.update_offer = None;
                        state.update_path = None;
                    }
                    Err(error) => {
                        state.update_phase = "failed".into();
                        state.update_version.clear();
                        state.update_notes.clear();
                        state.update_source = crate::update::source_label(&check_channel);
                        state.update_size_text.clear();
                        state.update_size_bytes = 0;
                        state.update_error = format!("{error:#}");
                        state.update_offer = None;
                        state.update_path = None;
                    }
                }
                state.update_rev = state.update_rev.wrapping_add(1);
                state.touch();
            }
        });
    if let Err(error) = spawned {
        log::error!("spawn android update check failed: {error}");
        if let Ok(mut state) = shared.lock() {
            state.update_phase = "failed".into();
            state.update_error = "无法启动更新检查".into();
            state.update_rev = state.update_rev.wrapping_add(1);
            state.touch();
        }
    }
}

fn start_update_download(desktop: Arc<NativeDesktop>, shared: Arc<Mutex<Shared>>) {
    let offer = {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        if matches!(
            state.update_phase.as_str(),
            "checking" | "downloading" | "installing" | "ready"
        ) {
            return;
        }
        let Some(offer) = state.update_offer.clone() else {
            return;
        };
        state.update_phase = "downloading".into();
        state.update_error.clear();
        state.update_progress_bytes.store(0, Ordering::Relaxed);
        state.update_path = None;
        state.update_rev = state.update_rev.wrapping_add(1);
        state.touch();
        offer
    };
    let progress = {
        let Ok(state) = shared.lock() else {
            return;
        };
        state.update_progress_bytes.clone()
    };
    let worker_shared = shared.clone();
    let spawned = thread::Builder::new()
        .name("wunder-android-update-download".into())
        .spawn(move || {
            let result = crate::update::download_offer(&desktop, &offer, &progress);
            if let Ok(mut state) = worker_shared.lock() {
                match result {
                    Ok(path) => {
                        state.update_phase = "ready".into();
                        state.update_path = Some(path);
                        state
                            .update_progress_bytes
                            .store(state.update_size_bytes, Ordering::Relaxed);
                        state.update_error.clear();
                    }
                    Err(error) => {
                        state.update_phase = "failed".into();
                        state.update_path = None;
                        state.update_error = format!("{error:#}");
                    }
                }
                state.update_rev = state.update_rev.wrapping_add(1);
                state.touch();
            }
        });
    if let Err(error) = spawned {
        log::error!("spawn android update download failed: {error}");
        if let Ok(mut state) = shared.lock() {
            state.update_phase = "failed".into();
            state.update_error = "无法启动更新下载".into();
            state.update_rev = state.update_rev.wrapping_add(1);
            state.touch();
        }
    }
}

fn install_update(shared: &Arc<Mutex<Shared>>) {
    let path = {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        if state.update_phase != "ready" && state.update_phase != "needs_install_permission" {
            return;
        }
        let Some(path) = state.update_path.clone() else {
            state.update_phase = "failed".into();
            state.update_error = "更新包不存在，请重新下载".into();
            state.update_rev = state.update_rev.wrapping_add(1);
            state.touch();
            return;
        };
        state.update_phase = "installing".into();
        state.update_error.clear();
        state.update_rev = state.update_rev.wrapping_add(1);
        state.touch();
        path
    };
    let outcome = crate::update::install(&path);
    if let Ok(mut state) = shared.lock() {
        match outcome {
            Ok(status) if status == "permission" => {
                state.update_phase = "needs_install_permission".into();
                state.update_error = "请允许 wunder 安装应用，返回后再次点击安装".into();
            }
            Ok(status) if status == "started" => {
                state.update_phase = "installing".into();
                state.update_error.clear();
            }
            Ok(status) => {
                state.update_phase = "failed".into();
                state.update_error = status
                    .strip_prefix("failed:")
                    .unwrap_or(&status)
                    .to_string();
            }
            Err(error) => {
                state.update_phase = "failed".into();
                state.update_error = format!("{error:#}");
            }
        }
        state.update_rev = state.update_rev.wrapping_add(1);
        state.touch();
    }
}

/// Project the persisted companion overlay state plus the library list into
/// the settings page. The floating controller reads the same projection.
fn refresh_companion(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let overlay = desktop.companion_overlay_state();
    let companions = desktop.list_companions().unwrap_or_default();
    #[cfg(target_os = "android")]
    let permission = crate::companion::permission_status();
    #[cfg(not(target_os = "android"))]
    let permission = "unknown".to_string();
    let Ok(mut state) = shared.lock() else {
        return;
    };
    state.companion = CompanionView {
        // 未选形象的开启态在设置页读作关闭，避免「开着却没有伙伴」。
        enabled: overlay.enabled && !overlay.selected_id.is_empty(),
        selected_id: overlay.selected_id,
        scale: overlay.scale,
        message_hints: overlay.message_hints,
        permission,
        companions: companions
            .into_iter()
            .map(|companion| CompanionChoice {
                id: companion.id,
                name: companion.display_name,
            })
            .collect(),
    };
    state.companion_rev = state.companion_rev.wrapping_add(1);
    state.touch();
}

/// Persist only the shell override; every other runtime path stays as the
/// user configured it.
fn save_shell_path(desktop: &NativeDesktop, shell_path: &str) -> anyhow::Result<()> {
    let settings = desktop.get_desktop_settings()?;
    desktop.save_runtime(
        &settings.workspace_root,
        &settings.language,
        &settings.python_path,
        &settings.git_path,
        &settings.rg_path,
        &settings.ffmpeg_path,
        &settings.browser_path,
        shell_path,
    )?;
    Ok(())
}

/// Publish the interpreter and browser the bridge must use. The bridge runs
/// from the Android host process, where neither exists; both live inside the
/// bundled rootfs and are reached through the generated proot wrappers.
#[cfg(target_os = "android")]
fn configure_browser_runtime() {
    let python = crate::ubuntu::python_path();
    if python.trim().is_empty() {
        std::env::remove_var("WUNDER_BROWSER_PYTHON");
    } else {
        std::env::set_var("WUNDER_BROWSER_PYTHON", python);
    }
    if crate::ubuntu::browser_installed() {
        let executable = crate::ubuntu::browser_executable();
        if !executable.trim().is_empty() {
            std::env::set_var("WUNDER_BROWSER_BIN", executable);
        }
    } else {
        std::env::remove_var("WUNDER_BROWSER_BIN");
    }
}

/// First launch after an install extracts the bundled runtime without waiting
/// for the settings card. Failures stay visible there and are retried only by
/// an explicit tap, so a broken install does not loop on every launch.
#[cfg(target_os = "android")]
fn maybe_auto_install_ubuntu(desktop: &Arc<NativeDesktop>, shared: &Arc<Mutex<Shared>>) {
    let Some(snapshot) = crate::ubuntu::snapshot() else {
        return;
    };
    if snapshot.state != "missing" {
        return;
    }
    log::info!("ubuntu runtime missing; starting first-launch extraction");
    start_ubuntu_install(desktop.clone(), shared.clone());
}

/// Extraction can take minutes on a phone. Run it off the engine loop and keep
/// the foreground service active meanwhile; the engine only publishes progress.
fn start_ubuntu_install(desktop: Arc<NativeDesktop>, shared: Arc<Mutex<Shared>>) {
    #[cfg(not(target_os = "android"))]
    {
        let _ = (desktop, shared);
    }
    #[cfg(target_os = "android")]
    let spawned = {
        let shared = shared.clone();
        thread::Builder::new()
            .name("wunder-ubuntu-install".into())
            .spawn(move || {
                let _install = InstallGuard::start();
                crate::ubuntu::install();
                let mut last_state = String::new();
                loop {
                    thread::sleep(std::time::Duration::from_millis(600));
                    let Some(snapshot) = crate::ubuntu::snapshot() else {
                        break;
                    };
                    last_state = snapshot.state.clone();
                    if let Ok(mut state) = shared.lock() {
                        state.ubuntu.state = snapshot.state.clone();
                        state.ubuntu.progress = snapshot.progress;
                        state.ubuntu.stage = snapshot.stage.clone();
                        state.ubuntu.version = snapshot.version.clone();
                        state.ubuntu.error = snapshot.error.clone();
                        state.ubuntu.shell_path = crate::ubuntu::shell_path();
                        state.ubuntu_rev = state.ubuntu_rev.wrapping_add(1);
                        state.touch();
                    }
                    if !snapshot.installing() {
                        break;
                    }
                }
                match last_state.as_str() {
                    "installed" => {
                        let settings = desktop.get_desktop_settings();
                        let needs_enable = settings
                            .as_ref()
                            .map(|settings| settings.shell_path.trim().is_empty())
                            .unwrap_or(false);
                        if needs_enable {
                            let shell = crate::ubuntu::shell_path();
                            if let Err(error) = save_shell_path(&desktop, &shell) {
                                log::warn!("enable ubuntu shell failed: {error:#}");
                            }
                        }
                        refresh_ubuntu(&desktop, &shared);
                        configure_browser_runtime();
                        notify(&shared, "Ubuntu 运行环境已就绪，命令工具将使用 Ubuntu");
                    }
                    "error" => {
                        let detail = crate::ubuntu::snapshot()
                            .map(|snapshot| snapshot.error)
                            .unwrap_or_default();
                        refresh_ubuntu(&desktop, &shared);
                        notify(
                            &shared,
                            if detail.is_empty() {
                                "Ubuntu 安装失败".to_string()
                            } else {
                                format!("Ubuntu 安装失败：{detail}")
                            },
                        );
                    }
                    _ => refresh_ubuntu(&desktop, &shared),
                }
            })
    };
    #[cfg(target_os = "android")]
    if let Err(error) = spawned {
        log::error!("spawn ubuntu installer failed: {error}");
        notify(&shared, "无法启动 Ubuntu 安装");
    }
}

fn reset_context_usage(shared: &Arc<Mutex<Shared>>) {
    if let Ok(mut state) = shared.lock() {
        state.context_used = None;
        state.context_capacity = None;
        state.context_usage = 0.0;
        state.context_known = false;
        state.context_counts.clear();
        state.context_rev = state.context_rev.wrapping_add(1);
    }
}

fn apply_context_usage(shared: &Arc<Mutex<Shared>>, usage: wunder_desktop::NativeContextUsage) {
    if usage.used.is_none() && usage.capacity.is_none() {
        return;
    }
    let Ok(mut state) = shared.lock() else {
        return;
    };
    if let Some(used) = usage.used {
        state.context_used = Some(used.max(0.0));
    }
    if let Some(capacity) = usage.capacity {
        if capacity > 0.0 {
            state.context_capacity = Some(capacity);
        }
    }
    let used = state.context_used.unwrap_or(0.0);
    let capacity = state.context_capacity.unwrap_or(0.0);
    state.context_known = state.context_used.is_some() && capacity > 0.0;
    state.context_usage = if state.context_known {
        (used / capacity).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let used_text = state
        .context_used
        .map(|value| format_tokens(value.max(0.0) as i64))
        .unwrap_or_else(|| "—".into());
    let capacity_text = state
        .context_capacity
        .map(|value| format_tokens(value.max(0.0) as i64))
        .unwrap_or_else(|| "—".into());
    state.context_counts = format!("{used_text} / {capacity_text} tokens");
    state.context_rev = state.context_rev.wrapping_add(1);
}

fn refresh_context_usage(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, session: &str) {
    if session.trim().is_empty() {
        return;
    }
    match desktop.context_usage(session) {
        Ok(usage) => apply_context_usage(shared, usage),
        Err(error) => log::warn!("refresh context usage failed: {error:#}"),
    }
}

fn select_session(
    desktop: &Arc<NativeDesktop>,
    shared: &Arc<Mutex<Shared>>,
    session: &str,
    generation: &Arc<AtomicU64>,
    watch_cancel: &mut Option<NativeThreadWatchCancel>,
    wait_for_lease: bool,
) -> anyhow::Result<()> {
    if let Some(cancel) = watch_cancel.take() {
        cancel.cancel();
    }
    // A switch must not reuse the previous thread's gauge while the new
    // snapshot is being read.
    reset_context_usage(shared);
    // The Android app is single-process, so a desktop lease owned by another
    // instance id belongs to a process the system already killed. Drop it
    // instead of letting the first open wait out its 15 s TTL.
    if let Err(error) = desktop.reclaim_previous_process_lease(session) {
        log::warn!("reclaim previous thread lease failed: {error:#}");
    }
    let snapshot = if wait_for_lease {
        // A process killed by the system (swipe-away, low memory) leaves its
        // thread-open lease in SQLite until the 15 s TTL lapses; the next
        // launch would otherwise greet the user with a false "another
        // instance owns this thread" error. Only the first open after startup
        // waits that window out; later user-driven switches report a genuine
        // conflict immediately.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(18);
        loop {
            match desktop.open_chat_snapshot(session) {
                Err(error)
                    if error.to_string().contains("THREAD_IN_USE")
                        && std::time::Instant::now() < deadline =>
                {
                    thread::sleep(std::time::Duration::from_millis(300));
                }
                other => break other?,
            }
        }
    } else {
        desktop.open_chat_snapshot(session)?
    };
    // The compact thread-open snapshot deliberately carries no rounds, so it
    // cannot feed the inline work-process rows the phone renders. Seed the
    // transcript from the detailed projection and keep the compact copy only
    // as a fallback for an empty/unreadable detail read.
    let initial_turns = desktop
        .load_chat_turns(session)
        .ok()
        .filter(|turns| !turns.is_empty())
        .unwrap_or_else(|| snapshot.turns.clone());
    let running_tasks = desktop.running_command_tasks(session).len() as i32;
    {
        let mut state = shared.lock().unwrap();
        state.session_id = snapshot.session.id.clone();
        state.title = if snapshot.session.title.trim().is_empty() {
            "新会话".into()
        } else {
            snapshot.session.title.clone()
        };
        state.workspace = snapshot.session.workspace_name.clone();
        state.workspace_id = snapshot.session.workspace_id.clone().unwrap_or_default();
        // Opening a thread focuses its workspace, so a later new-task lands in
        // the same one. An explicit pick made before the open is replaced,
        // matching the desktop sidebar.
        state.selected_workspace = state.workspace_id.clone();
        state.subtitle = status_subtitle(
            snapshot.session.workspace_name.as_str(),
            &snapshot.session.runtime_status,
        );
        state.model_key = snapshot
            .session
            .session_model_name
            .clone()
            .unwrap_or_default();
        state.model_name = state.model_key.clone();
        state.durable_rows = project_turns(&initial_turns);
        state.durable_turns = initial_turns.clone();
        // 目标随线程恢复；计时从 0 重新开始。
        state.goal = GoalView {
            objective: snapshot.session.goal.objective.clone(),
            phase: snapshot.session.goal.phase.clone(),
            activation: snapshot.session.goal.activation.clone(),
            rounds_started: snapshot.session.goal.rounds_started,
            max_goal_rounds: snapshot.session.goal.max_goal_rounds,
            seconds: 0,
        };
        state.goal_rev = state.goal_rev.wrapping_add(1);
        state.inquiry = pending_inquiry(&initial_turns, session, &state.consumed_inquiries);
        state.inquiry_rev = state.inquiry_rev.wrapping_add(1);
        state.live_rows.clear();
        state.live_root.clear();
        state.running_task_count = running_tasks;
        state.subagent_count = running_subagents(&initial_turns);
        state.rows_rev = state.rows_rev.wrapping_add(1);
        state.live_rev = state.live_rev.wrapping_add(1);
        state.stream_revision = state.stream_revision.wrapping_add(1);
        state.loading = false;
        state.generating = matches!(
            snapshot.session.runtime_status.as_str(),
            "running" | "queued" | "pending"
        );
        state.stopping = false;
        state.turn_phase = if state.generating {
            "running".into()
        } else {
            String::new()
        };
        state.turn_hint.clear();
        // 计划面板恢复该线程最后一次发布的清单。
        let (plan_explanation, plan_steps) = plan_from_turns(&state.durable_turns);
        let plan_running = state.generating;
        set_plan(&mut state, plan_explanation, plan_steps, plan_running);
        state.touch();
    }
    refresh_context_usage(desktop, shared, session);
    let active = shared.lock().unwrap().session_id.clone();
    refresh_sessions(desktop, shared, Some(&active));

    let epoch = generation.fetch_add(1, Ordering::SeqCst) + 1;
    let desktop = desktop.clone();
    let shared = shared.clone();
    let session = session.to_owned();
    let generation = generation.clone();
    let watch = desktop.watch_chat_with_initial(&session, Some(initial_turns));
    *watch_cancel = Some(watch.cancel_handle());
    thread::Builder::new()
        .name("wunder-android-watch".into())
        .spawn(move || {
            let mut watch = watch;
            loop {
                if generation.load(Ordering::SeqCst) != epoch {
                    break;
                }
                match watch.blocking_recv() {
                    Some(NativeThreadUpdate::Reset(turns))
                    | Some(NativeThreadUpdate::Turns(turns)) => {
                        if generation.load(Ordering::SeqCst) != epoch {
                            break;
                        }
                        publish_durable(&desktop, &shared, &session, turns);
                    }
                    Some(NativeThreadUpdate::Goal(goal)) => {
                        update_goal(&shared, &goal);
                    }
                    Some(NativeThreadUpdate::Failed(message)) => {
                        notify(&shared, message);
                    }
                    Some(_) => {}
                    None => break,
                }
            }
        })
        .ok();
    Ok(())
}

/// 目标更新：objective 变化或从非活跃变为活跃时重置本地计时。
fn update_goal(shared: &Arc<Mutex<Shared>>, goal: &NativeGoal) {
    let Ok(mut state) = shared.lock() else {
        return;
    };
    let objective_changed = state.goal.objective != goal.objective;
    let was_active = state.goal.active();
    state.goal.objective = goal.objective.clone();
    state.goal.phase = goal.phase.clone();
    state.goal.activation = goal.activation.clone();
    state.goal.rounds_started = goal.rounds_started;
    state.goal.max_goal_rounds = goal.max_goal_rounds;
    if objective_changed || (state.goal.active() && !was_active) {
        state.goal.seconds = 0;
    }
    state.goal_rev = state.goal_rev.wrapping_add(1);
    state.touch();
}

/// 取最近一轮带计划的回合（桌面语义一致），供锁外计算、锁内写入。
fn plan_from_turns(turns: &[NativeChatTurn]) -> (String, Vec<PlanStepView>) {
    let Some(plan) = turns.iter().rev().find_map(|turn| turn.plan.as_ref()) else {
        return (String::new(), Vec::new());
    };
    let steps = plan
        .steps
        .iter()
        .take(32)
        .map(|step| PlanStepView {
            step: step.step.clone(),
            status: step.status.clone(),
        })
        .collect();
    (plan.explanation.clone(), steps)
}

fn set_plan(state: &mut Shared, explanation: String, steps: Vec<PlanStepView>, running: bool) {
    if state.plan.explanation == explanation
        && state.plan.steps == steps
        && state.plan.running == running
    {
        return;
    }
    state.plan = PlanView {
        explanation,
        steps,
        running,
    };
    state.plan_rev = state.plan_rev.wrapping_add(1);
    state.touch();
}

/// 流内的计划事件：工具每次发布整份清单，整体替换；最多保留 32 步。
fn apply_plan_event(shared: &Arc<Mutex<Shared>>, value: &serde_json::Value) {
    let envelope = value.get("data").unwrap_or(value);
    let data = envelope.get("data").unwrap_or(envelope);
    let explanation = data
        .get("explanation")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let steps: Vec<PlanStepView> = data
        .get("plan")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(32)
                .filter_map(|entry| {
                    Some(PlanStepView {
                        step: entry.get("step")?.as_str()?.to_string(),
                        status: entry
                            .get("status")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("pending")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let Ok(mut state) = shared.lock() else {
        return;
    };
    state.plan = PlanView {
        explanation,
        steps,
        running: true,
    };
    state.plan_rev = state.plan_rev.wrapping_add(1);
    state.touch();
}

fn running_subagents(turns: &[NativeChatTurn]) -> i32 {
    turns
        .iter()
        .flat_map(|turn| turn.subagents.iter())
        .filter(|card| !card.terminal)
        .count() as i32
}

fn project_turns(turns: &[wunder_desktop::NativeChatTurn]) -> Vec<RowData> {
    let mut rows = Vec::new();
    for turn in turns {
        rows.extend(project::project_turn(turn));
    }
    // `payload` reaches the fold callback, and the UI routes it back into the
    // flat durable row list. It must be the absolute row index, not the index
    // inside the turn: a per-turn index folds the wrong row (or nothing) from
    // the second turn onward. Process children pack their owning bar and their
    // position into the same value, so no side table has to stay in sync.
    for (index, row) in rows.iter_mut().enumerate() {
        let bar = index as i32;
        row.payload = bar;
        for (child_index, child) in row.process_children.iter_mut().enumerate() {
            child.payload = project::PROCESS_CHILD_BASE
                .wrapping_add(bar.wrapping_shl(project::PROCESS_CHILD_SHIFT))
                .wrapping_add(child_index as i32);
        }
    }
    rows
}

/// Re-project with the user's fold state carried forward. Row ids are stable
/// across watcher updates; a plain re-projection would collapse every open
/// work-process entry each time a turn commits.
fn project_turns_preserving(turns: &[NativeChatTurn], previous: &[RowData]) -> Vec<RowData> {
    let mut rows = project_turns(turns);
    let opens: std::collections::HashMap<&str, bool> = previous
        .iter()
        .filter(|row| row.foldable)
        .map(|row| (row.id.as_str(), row.open))
        .collect();
    let child_opens: std::collections::HashMap<&str, bool> = previous
        .iter()
        .flat_map(|row| row.process_children.iter())
        .map(|child| (child.id.as_str(), child.open))
        .collect();
    for row in &mut rows {
        if let Some(open) = opens.get(row.id.as_str()) {
            row.open = *open;
        }
        for child in &mut row.process_children {
            if let Some(open) = child_opens.get(child.id.as_str()) {
                child.open = *open;
            }
        }
    }
    rows
}

/// Merge one watcher batch into the retained turn list. The observer mixes
/// full header resets with detailed per-root commits, so a batch must never
/// replace the whole transcript: that drops earlier turns, and a compact
/// header must not erase rounds the detailed projection already loaded.
fn merge_turns(retained: &mut Vec<NativeChatTurn>, incoming: Vec<NativeChatTurn>) {
    for turn in incoming {
        match retained.iter_mut().find(|old| old.root_id == turn.root_id) {
            Some(old) => {
                let mut next = turn;
                if next.rounds.is_empty() && !old.rounds.is_empty() {
                    next.rounds = std::mem::take(&mut old.rounds);
                    next.process_steps = next.process_steps.max(old.process_steps);
                }
                if next.assistant.text.trim().is_empty() && !old.assistant.text.trim().is_empty() {
                    next.assistant.text = old.assistant.text.clone();
                }
                if next.failure.is_none() {
                    next.failure = old.failure.clone();
                }
                if next.plan.is_none() {
                    next.plan = old.plan.clone();
                }
                if next.question_panel.is_none() {
                    next.question_panel = old.question_panel.clone();
                }
                if next.subagents.is_empty() {
                    next.subagents = old.subagents.clone();
                }
                *old = next;
            }
            None => retained.push(turn),
        }
    }
    retained.sort_by_key(|turn| turn.user.user_round);
}

fn publish_durable(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    session: &str,
    turns: Vec<NativeChatTurn>,
) {
    let candidates: Vec<String> = turns
        .iter()
        .filter(|turn| turn.rounds.is_empty() && turn.process_steps > 0)
        .map(|turn| turn.root_id.clone())
        .collect();
    let need_detail = {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        if state.session_id != session {
            return;
        }
        // While the local stream owns this turn its live rows are the truth;
        // commit boundaries only advance the retained durable copy. Skip the
        // whole-history re-projection here so a long chat cannot block the UI
        // tick behind the shared lock during generation.
        let live_owned = state.generating
            && state.live_session == session
            && !state.live_root.is_empty()
            && state.live_root != "live"
            && !turns.is_empty()
            && turns.iter().all(|turn| turn.root_id == state.live_root);
        merge_turns(&mut state.durable_turns, turns);
        let subagents = running_subagents(&state.durable_turns);
        if state.subagent_count != subagents {
            state.subagent_count = subagents;
            state.touch();
        }
        if live_owned {
            restore_pending_inquiry(&mut state, session);
            false
        } else {
            let need_detail = candidates.iter().any(|root| {
                state
                    .durable_turns
                    .iter()
                    .find(|turn| &turn.root_id == root)
                    .is_some_and(|turn| turn.rounds.is_empty())
            });
            let rows = project_turns_preserving(&state.durable_turns, &state.durable_rows);
            state.durable_rows = rows;
            restore_pending_inquiry(&mut state, session);
            // 落库是权威时同步计划面板；实时流期间由 plan_update 事件接管。
            let (plan_explanation, plan_steps) = plan_from_turns(&state.durable_turns);
            let plan_running = state.generating;
            set_plan(&mut state, plan_explanation, plan_steps, plan_running);
            state.rows_rev = state.rows_rev.wrapping_add(1);
            state.stream_revision = state.stream_revision.wrapping_add(1);
            state.touch();
            need_detail
        }
    };
    if !need_detail {
        return;
    }
    // A compact reset introduced a turn whose work process is not loaded yet.
    // Backfill from the detailed projection; commit batches already carry
    // rounds, so this path only runs for genuinely header-only turns.
    let Ok(detailed) = desktop.load_chat_turns(session) else {
        return;
    };
    if let Ok(mut state) = shared.lock() {
        if state.session_id != session {
            return;
        }
        merge_turns(&mut state.durable_turns, detailed);
        state.subagent_count = running_subagents(&state.durable_turns);
        let rows = project_turns_preserving(&state.durable_turns, &state.durable_rows);
        state.durable_rows = rows;
        restore_pending_inquiry(&mut state, session);
        let (plan_explanation, plan_steps) = plan_from_turns(&state.durable_turns);
        let plan_running = state.generating;
        set_plan(&mut state, plan_explanation, plan_steps, plan_running);
        state.rows_rev = state.rows_rev.wrapping_add(1);
        state.stream_revision = state.stream_revision.wrapping_add(1);
        state.touch();
    }
}

fn refresh_sessions(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, active: Option<&str>) {
    // The drawer mirrors the desktop sidebar: one workspace at a time. Fall
    // back to the opened thread's workspace before the catalogue arrives.
    let workspace = shared
        .lock()
        .map(|state| {
            if state.selected_workspace.trim().is_empty() {
                state.workspace_id.clone()
            } else {
                state.selected_workspace.clone()
            }
        })
        .unwrap_or_default();
    let workspace_arg = (!workspace.trim().is_empty()).then_some(workspace.as_str());
    let Ok(list) = desktop.list_sessions(workspace_arg) else {
        return;
    };
    let settings = desktop.get_desktop_settings().ok();
    let sessions = session_cards(&list, active);
    let models = settings
        .as_ref()
        .map(|settings| settings.models.iter().map(model_card).collect::<Vec<_>>())
        .unwrap_or_default();
    let default_model = settings.as_ref().and_then(|settings| {
        settings
            .models
            .iter()
            .find(|model| model.is_default && model.enabled)
            .map(|model| (model.key.clone(), model.key.clone()))
    });
    if let Ok(mut state) = shared.lock() {
        state.sessions = sessions;
        state.sessions_rev = state.sessions_rev.wrapping_add(1);
        state.models = models;
        state.models_rev = state.models_rev.wrapping_add(1);
        if let Some(settings) = &settings {
            state.send_key = settings.send_key.clone();
            state.settings = SettingsView {
                theme: settings.theme.clone(),
                font_size: settings.font_size,
                send_key: settings.send_key.clone(),
                language: settings.language.clone(),
                workspace_root: settings.workspace_root.clone(),
                update_source: settings.update_source.clone(),
                version: CURRENT_VERSION.into(),
            };
            state.settings_rev = state.settings_rev.wrapping_add(1);
        }
        if state.model_name.is_empty() {
            if let Some((key, name)) = default_model {
                state.model_key = key;
                state.model_name = name;
            }
        }
        state.touch();
    }
    // Keep the workspace catalogue's thread counts and active row in step with
    // the session projection (a new thread, an archive or a rename all land
    // here). Failures stay in the log: this is a background refresher, not a
    // user action that needs a toast.
    if let Err(error) = refresh_workspaces(desktop, shared) {
        log::warn!("refresh workspace catalogue failed: {error:#}");
    }
}

fn session_cards(
    list: &[wunder_desktop::NativeSession],
    active: Option<&str>,
) -> Vec<MobileSession> {
    list.iter()
        .map(|session| MobileSession {
            id: session.id.as_str().into(),
            title: if session.title.trim().is_empty() {
                "新会话".into()
            } else {
                session.title.clone()
            }
            .into(),
            preview: session.workspace_name.as_str().into(),
            time: project::session_time(session.updated_at).into(),
            status: session.runtime_status.as_str().into(),
            active: active == Some(session.id.as_str()),
        })
        .collect()
}

fn refresh_archived(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let Ok((list, total)) = desktop.list_archived_sessions(0, 100) else {
        return;
    };
    let archived = session_cards(&list, None);
    if let Ok(mut state) = shared.lock() {
        state.archived_sessions = archived;
        state.archived_total = total;
        state.archived_rev = state.archived_rev.wrapping_add(1);
        state.touch();
    }
}

fn refresh_preset_questions(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let Ok(agent) = desktop.default_agent() else {
        return;
    };
    if let Ok(mut state) = shared.lock() {
        state.preset_questions = agent
            .preset_questions
            .into_iter()
            .map(|question| question.trim().to_string())
            .filter(|question| !question.is_empty())
            .take(12)
            .collect();
        state.preset_rev = state.preset_rev.wrapping_add(1);
        state.touch();
    }
}

// ---- tool manager -----------------------------------------------------

fn tool_kind(tab: i32) -> Option<ToolResourceKind> {
    match tab {
        1 => Some(ToolResourceKind::Mcp),
        2 => Some(ToolResourceKind::Skill),
        3 => Some(ToolResourceKind::Knowledge),
        _ => None,
    }
}

fn mobile_tool(tool: &ToolRecord, enabled: bool) -> MobileTool {
    // Same icon and badge tone as the desktop agent/tools page: the shared
    // resolver keeps per-tool glyphs (terminal, file, search, …) instead of one
    // category glyph for every builtin.
    let visual =
        wunder_desktop::tool_icons::tool_card_visual(&tool.name, &tool.description, &tool.category);
    MobileTool {
        name: tool.name.as_str().into(),
        description: tool.description.as_str().into(),
        category: tool.category.as_str().into(),
        enabled,
        icon: visual.icon.into(),
        tone: visual.tone.into(),
    }
}

fn mobile_resource(resource: NativeToolResource) -> MobileToolResource {
    MobileToolResource {
        name: resource.name.into(),
        description: resource.description.into(),
        detail: resource.detail.into(),
        readonly: resource.readonly,
    }
}

fn mobile_file(file: NativeToolFile) -> MobileToolFile {
    MobileToolFile {
        name: file.name.into(),
        path: file.path.into(),
        directory: file.directory,
        size: human_size(file.size).into(),
    }
}

fn join_tool_path(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_string()
    } else {
        format!("{directory}/{name}")
    }
}

fn dedup_text(values: Vec<String>, limit: usize) -> Vec<String> {
    let mut result = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        if value.is_empty() || result.contains(&value) {
            continue;
        }
        result.push(value);
        if result.len() >= limit {
            break;
        }
    }
    result
}

fn category_for_group(title: &str) -> &str {
    match title {
        "技能工具" => "技能",
        "知识库工具" => "知识库",
        "自定义工具" => "用户工具",
        other => other,
    }
}

fn agent_groups(
    catalog: &[ToolRecord],
    tool_names: &[String],
    query: &str,
) -> Vec<AgentToolGroupView> {
    let query = query.trim().to_lowercase();
    let mut groups = Vec::new();
    for category in [
        "内置工具",
        "MCP 工具",
        "A2A 工具",
        "技能",
        "知识库",
        "用户工具",
    ] {
        let tools = catalog
            .iter()
            .filter(|tool| tool.category == category)
            .filter(|tool| {
                query.is_empty()
                    || tool.name.to_lowercase().contains(&query)
                    || tool.description.to_lowercase().contains(&query)
            })
            .map(|tool| mobile_tool(tool, tool_names.iter().any(|name| name == &tool.name)))
            .collect::<Vec<_>>();
        if tools.is_empty() {
            continue;
        }
        let title = match category {
            "技能" => "技能工具",
            "知识库" => "知识库工具",
            "用户工具" => "自定义工具",
            other => other,
        };
        let all_selected = tools.iter().all(|tool| tool.enabled);
        groups.push(AgentToolGroupView {
            title: title.into(),
            tools,
            all_selected,
        });
    }
    groups
}

fn agent_selection(shared: &Arc<Mutex<Shared>>) -> (Vec<String>, Vec<String>) {
    shared
        .lock()
        .map(|state| {
            (
                state.agent.tool_names.clone(),
                state.agent.preset_questions.clone(),
            )
        })
        .unwrap_or_default()
}

fn refresh_tools(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let catalog = match desktop.list_tools() {
        Ok(catalog) => catalog,
        Err(error) => {
            set_tools_error(shared, format!("无法读取工具列表：{error}"));
            return;
        }
    };
    let manager = match desktop.tool_manager() {
        Ok(manager) => manager,
        Err(error) => {
            set_tools_error(shared, format!("无法读取工具资源：{error}"));
            return;
        }
    };
    let tools = ToolsView {
        builtin: catalog
            .iter()
            .filter(|tool| tool.category == "内置工具")
            .map(|tool| mobile_tool(tool, false))
            .collect(),
        mcp_tools: catalog
            .iter()
            .filter(|tool| tool.category == "MCP 工具")
            .map(|tool| mobile_tool(tool, false))
            .collect(),
        skill_tools: catalog
            .iter()
            .filter(|tool| tool.category == "技能")
            .map(|tool| mobile_tool(tool, false))
            .collect(),
        knowledge_tools: catalog
            .iter()
            .filter(|tool| tool.category == "知识库")
            .map(|tool| mobile_tool(tool, false))
            .collect(),
        servers: manager.servers.into_iter().map(mobile_resource).collect(),
        skills: manager.skills.into_iter().map(mobile_resource).collect(),
        bases: manager.bases.into_iter().map(mobile_resource).collect(),
        ..ToolsView::default()
    };
    if let Ok(mut state) = shared.lock() {
        state.tools = tools;
        state.tools_rev = state.tools_rev.wrapping_add(1);
        state.touch();
    }
}

fn set_tools_error(shared: &Arc<Mutex<Shared>>, message: String) {
    if let Ok(mut state) = shared.lock() {
        state.tools.error = message.clone();
        state.tools_rev = state.tools_rev.wrapping_add(1);
        state.touch();
    }
    notify(shared, message);
}

fn apply_tool_files(
    shared: &Arc<Mutex<Shared>>,
    tab: i32,
    resource: &str,
    readonly: bool,
    directory: &str,
    files: NativeToolFiles,
    append: bool,
) {
    let Ok(mut state) = shared.lock() else {
        return;
    };
    let entries = files
        .entries
        .into_iter()
        .map(mobile_file)
        .collect::<Vec<_>>();
    state.tools.browse_tab = tab;
    state.tools.browse_resource = resource.to_string();
    state.tools.browse_readonly = readonly;
    state.tools.directory = directory.to_string();
    if append {
        state.tools.files.extend(entries);
    } else {
        state.tools.files = entries;
    }
    state.tools.has_more = files.has_more && state.tools.files.len() < 2000;
    state.tools.error.clear();
    state.tools_rev = state.tools_rev.wrapping_add(1);
    state.touch();
}

fn select_tool_server(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, index: i32) {
    let name = shared
        .lock()
        .ok()
        .and_then(|state| {
            state
                .tools
                .servers
                .get(index.max(0) as usize)
                .map(|server| server.name.to_string())
        })
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }
    load_server_tools(desktop, shared, &name);
}

fn load_server_tools(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, name: &str) {
    match desktop.tool_server_tools(name) {
        Ok(list) => {
            if let Ok(mut state) = shared.lock() {
                state.tools.active_server = name.to_string();
                state.tools.server_tools =
                    list.iter().map(|tool| mobile_tool(tool, false)).collect();
                state.tools_rev = state.tools_rev.wrapping_add(1);
                state.touch();
            }
        }
        Err(error) => notify(shared, format!("无法读取 MCP 工具：{error}")),
    }
}

fn select_tool_resource(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    tab: i32,
    index: i32,
) {
    let (name, readonly) = match shared.lock() {
        Ok(state) => {
            let list = if tab == 2 {
                &state.tools.skills
            } else {
                &state.tools.bases
            };
            match list.get(index.max(0) as usize) {
                Some(item) => (item.name.to_string(), item.readonly),
                None => return,
            }
        }
        Err(_) => return,
    };
    let Some(kind) = tool_kind(tab) else {
        return;
    };
    let editable = if kind == ToolResourceKind::Knowledge {
        desktop
            .tool_manager()
            .ok()
            .and_then(|manager| {
                manager
                    .base_configs
                    .into_iter()
                    .find(|config| config.name == name)
            })
            .map(|config| config.base_type == "literal")
            .unwrap_or(false)
    } else {
        !readonly
    };
    match desktop.tool_resource_files(kind, &name, "", 0) {
        Ok(files) => {
            if let Ok(mut state) = shared.lock() {
                state.tools.active_server.clear();
                state.tools.server_tools.clear();
                state.tools.browse_editable = editable;
            }
            apply_tool_files(shared, tab, &name, readonly, "", files, false);
        }
        Err(error) => notify(shared, format!("无法读取资源目录：{error}")),
    }
}

fn load_tool_directory(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, directory: &str) {
    let (tab, resource, current, readonly) = match shared.lock() {
        Ok(state) => (
            state.tools.browse_tab,
            state.tools.browse_resource.to_string(),
            state.tools.directory.to_string(),
            state.tools.browse_readonly,
        ),
        Err(_) => return,
    };
    if resource.is_empty() {
        return;
    }
    let directory = if directory == ".." {
        current
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_string())
            .unwrap_or_default()
    } else {
        directory.to_string()
    };
    let Some(kind) = tool_kind(tab) else {
        return;
    };
    match desktop.tool_resource_files(kind, &resource, &directory, 0) {
        Ok(files) => apply_tool_files(shared, tab, &resource, readonly, &directory, files, false),
        Err(error) => notify(shared, format!("无法打开目录：{error}")),
    }
}

fn more_tool_files(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let (tab, resource, directory, readonly, offset) = match shared.lock() {
        Ok(state) => (
            state.tools.browse_tab,
            state.tools.browse_resource.to_string(),
            state.tools.directory.to_string(),
            state.tools.browse_readonly,
            state.tools.files.len(),
        ),
        Err(_) => return,
    };
    let Some(kind) = tool_kind(tab) else {
        return;
    };
    match desktop.tool_resource_files(kind, &resource, &directory, offset) {
        Ok(files) => apply_tool_files(shared, tab, &resource, readonly, &directory, files, true),
        Err(error) => notify(shared, format!("无法加载更多：{error}")),
    }
}

fn reload_tool_files(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let (tab, resource, directory, readonly) = match shared.lock() {
        Ok(state) => (
            state.tools.browse_tab,
            state.tools.browse_resource.to_string(),
            state.tools.directory.to_string(),
            state.tools.browse_readonly,
        ),
        Err(_) => return,
    };
    if resource.is_empty() {
        return;
    }
    let Some(kind) = tool_kind(tab) else {
        return;
    };
    if let Ok(files) = desktop.tool_resource_files(kind, &resource, &directory, 0) {
        apply_tool_files(shared, tab, &resource, readonly, &directory, files, false);
    }
}

fn open_tool_file(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, path: &str) {
    let (tab, resource, readonly) = match shared.lock() {
        Ok(state) => (
            state.tools.browse_tab,
            state.tools.browse_resource.to_string(),
            state.tools.browse_readonly,
        ),
        Err(_) => return,
    };
    let Some(kind) = tool_kind(tab) else {
        return;
    };
    match desktop.read_tool_resource(kind, &resource, path) {
        Ok(content) => {
            if let Ok(mut state) = shared.lock() {
                state.tool_file = Some(ToolFileView {
                    path: path.to_string(),
                    content,
                    readonly,
                    error: String::new(),
                });
                state.tool_file_rev = state.tool_file_rev.wrapping_add(1);
                state.touch();
            }
        }
        Err(error) => notify(shared, format!("无法打开文件：{error}")),
    }
}

fn save_tool_file(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, path: &str, content: &str) {
    let (tab, resource, readonly, editable) = match shared.lock() {
        Ok(state) => (
            state.tools.browse_tab,
            state.tools.browse_resource.to_string(),
            state.tools.browse_readonly,
            state.tools.browse_editable,
        ),
        Err(_) => return,
    };
    if readonly {
        notify(shared, "全局技能只读");
        return;
    }
    if !editable {
        notify(shared, "该知识库类型不支持文本文件编辑");
        return;
    }
    let Some(kind) = tool_kind(tab) else {
        return;
    };
    match desktop.save_tool_resource(kind, &resource, path, content, false) {
        Ok(()) => {
            if let Ok(mut state) = shared.lock() {
                if let Some(file) = state.tool_file.as_mut() {
                    file.content = content.to_string();
                    file.error.clear();
                }
                state.tool_file_rev = state.tool_file_rev.wrapping_add(1);
                state.touch();
            }
            reload_tool_files(desktop, shared);
            notify(shared, "文件已保存");
        }
        Err(error) => {
            let message = format!("保存失败：{error}");
            if let Ok(mut state) = shared.lock() {
                if let Some(file) = state.tool_file.as_mut() {
                    file.error = message.clone();
                }
                state.tool_file_rev = state.tool_file_rev.wrapping_add(1);
                state.touch();
            }
            notify(shared, message);
        }
    }
}

fn close_tool_file(shared: &Arc<Mutex<Shared>>) {
    if let Ok(mut state) = shared.lock() {
        state.tool_file = None;
        state.tool_file_rev = state.tool_file_rev.wrapping_add(1);
        state.touch();
    }
}

fn create_tool_file(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, name: &str) {
    let name = name.trim();
    if name.is_empty() {
        notify(shared, "请输入文件名");
        return;
    }
    let (tab, resource, directory, readonly, editable) = match shared.lock() {
        Ok(state) => (
            state.tools.browse_tab,
            state.tools.browse_resource.to_string(),
            state.tools.directory.to_string(),
            state.tools.browse_readonly,
            state.tools.browse_editable,
        ),
        Err(_) => return,
    };
    if readonly {
        notify(shared, "全局技能只读");
        return;
    }
    if !editable {
        notify(shared, "该知识库类型不支持文本文件编辑");
        return;
    }
    let Some(kind) = tool_kind(tab) else {
        return;
    };
    let target = join_tool_path(&directory, name);
    match desktop.save_tool_resource(kind, &resource, &target, "", true) {
        Ok(()) => {
            reload_tool_files(desktop, shared);
            notify(shared, "文件已创建");
        }
        Err(error) => notify(shared, format!("无法创建文件：{error}")),
    }
}

fn upload_tool_files(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, files: &[String]) {
    let (tab, resource, directory, readonly, editable) = match shared.lock() {
        Ok(state) => (
            state.tools.browse_tab,
            state.tools.browse_resource.to_string(),
            state.tools.directory.to_string(),
            state.tools.browse_readonly,
            state.tools.browse_editable,
        ),
        Err(_) => return,
    };
    if readonly {
        notify(shared, "全局技能只读");
        return;
    }
    if !editable {
        notify(shared, "该知识库类型不支持文本文件编辑");
        return;
    }
    let Some(kind) = tool_kind(tab) else {
        return;
    };
    let mut saved = 0;
    let mut skipped = 0;
    for path in files {
        let source = std::path::Path::new(path);
        let size = std::fs::metadata(source)
            .map(|meta| meta.len())
            .unwrap_or(0);
        let Some(name) = source.file_name().and_then(|name| name.to_str()) else {
            skipped += 1;
            continue;
        };
        if size > 2 * 1024 * 1024 {
            skipped += 1;
            continue;
        }
        let Ok(text) = std::fs::read_to_string(source) else {
            skipped += 1;
            continue;
        };
        let target = join_tool_path(&directory, name);
        if desktop
            .save_tool_resource(kind, &resource, &target, &text, true)
            .is_ok()
        {
            saved += 1;
        } else {
            skipped += 1;
        }
    }
    reload_tool_files(desktop, shared);
    notify(
        shared,
        if skipped == 0 {
            format!("已上传 {saved} 个文件")
        } else {
            format!("已上传 {saved} 个文件，跳过 {skipped} 个（非文本或过大）")
        },
    );
}

fn delete_tool_resource(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    tab: i32,
    name: &str,
) {
    let Some(kind) = tool_kind(tab) else {
        return;
    };
    match desktop.delete_tool_resource(kind, name) {
        Ok(()) => {
            if let Ok(mut state) = shared.lock() {
                state.tool_file = None;
                state.tool_file_rev = state.tool_file_rev.wrapping_add(1);
            }
            refresh_tools(desktop, shared);
            notify(shared, "已删除");
        }
        Err(error) => notify(shared, format!("删除失败：{error}")),
    }
}

fn export_skill(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, name: &str) {
    let target = std::env::temp_dir().join(format!("{name}.zip"));
    let target_text = target.to_string_lossy().to_string();
    match desktop.export_tool_skill(name, &target_text) {
        Ok(()) => {
            #[cfg(target_os = "android")]
            {
                let status =
                    crate::android_host::save_file_to_phone(&target_text, &format!("{name}.zip"));
                if let Some(reason) = status.strip_prefix("failed:") {
                    notify(shared, format!("导出失败：{reason}"));
                } else {
                    notify(shared, "正在导出技能…");
                }
            }
            #[cfg(not(target_os = "android"))]
            notify(shared, format!("技能已导出到 {}", target.display()));
        }
        Err(error) => notify(shared, format!("导出技能失败：{error}")),
    }
}

fn open_tool_editor(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, kind: i32, index: i32) {
    let manager = match desktop.tool_manager() {
        Ok(manager) => manager,
        Err(error) => {
            notify(shared, format!("无法读取工具配置：{error}"));
            return;
        }
    };
    let (mcp, kb, is_new) = match kind {
        1 => {
            let config = if index >= 0 {
                manager.server_configs.get(index as usize)
            } else {
                None
            };
            if index >= 0 && config.is_none() {
                return;
            }
            let mcp = config
                .map(|config| MobileMcpDraft {
                    previous: config.name.as_str().into(),
                    name: config.name.as_str().into(),
                    display_name: config.display_name.as_str().into(),
                    endpoint: config.endpoint.as_str().into(),
                    transport: {
                        let transport = if config.transport.trim().is_empty() {
                            "auto"
                        } else {
                            config.transport.as_str()
                        };
                        transport.into()
                    },
                    description: config.description.as_str().into(),
                    headers: config.headers.as_str().into(),
                })
                .unwrap_or_else(|| MobileMcpDraft {
                    transport: "auto".into(),
                    ..MobileMcpDraft::default()
                });
            (mcp, MobileKnowledgeDraft::default(), index < 0)
        }
        2 => {
            let config = if index >= 0 {
                manager.base_configs.get(index as usize)
            } else {
                None
            };
            if index >= 0 && config.is_none() {
                return;
            }
            let kb = config
                .map(|config| MobileKnowledgeDraft {
                    previous: config.name.as_str().into(),
                    name: config.name.as_str().into(),
                    description: config.description.as_str().into(),
                    base_type: config.base_type.as_str().into(),
                    embedding_model: config.embedding_model.as_str().into(),
                    chunk_size: config.chunk_size.as_str().into(),
                    chunk_overlap: config.chunk_overlap.as_str().into(),
                    dataset_id: config.ragflow_dataset_id.as_str().into(),
                })
                .unwrap_or_else(|| MobileKnowledgeDraft {
                    base_type: "vector".into(),
                    ..MobileKnowledgeDraft::default()
                });
            (MobileMcpDraft::default(), kb, index < 0)
        }
        _ => return,
    };
    if let Ok(mut state) = shared.lock() {
        state.tool_editor = Some(ToolEditorView {
            kind,
            is_new,
            mcp,
            kb,
            error: String::new(),
        });
        state.tool_editor_rev = state.tool_editor_rev.wrapping_add(1);
        state.touch();
    }
}

fn close_tool_editor(shared: &Arc<Mutex<Shared>>) {
    if let Ok(mut state) = shared.lock() {
        state.tool_editor = None;
        state.tool_editor_rev = state.tool_editor_rev.wrapping_add(1);
        state.touch();
    }
}

fn save_tool_editor(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    kind: i32,
    mcp: MobileMcpDraft,
    kb: MobileKnowledgeDraft,
) {
    let result = match kind {
        1 => desktop.save_mcp_server(&NativeMcpServerDraft {
            previous: mcp.previous.to_string(),
            name: mcp.name.to_string(),
            display_name: mcp.display_name.to_string(),
            endpoint: mcp.endpoint.to_string(),
            transport: mcp.transport.to_string(),
            description: mcp.description.to_string(),
            headers: mcp.headers.to_string(),
        }),
        2 => desktop.save_knowledge_base(&NativeKnowledgeDraft {
            previous: kb.previous.to_string(),
            name: kb.name.to_string(),
            description: kb.description.to_string(),
            base_type: kb.base_type.to_string(),
            embedding_model: kb.embedding_model.to_string(),
            chunk_size: kb.chunk_size.to_string(),
            chunk_overlap: kb.chunk_overlap.to_string(),
            ragflow_dataset_id: kb.dataset_id.to_string(),
        }),
        _ => return,
    };
    match result {
        Ok(()) => {
            if let Ok(mut state) = shared.lock() {
                state.tool_editor = None;
                state.tool_editor_rev = state.tool_editor_rev.wrapping_add(1);
                state.touch();
            }
            refresh_tools(desktop, shared);
            notify(
                shared,
                if kind == 1 {
                    "MCP 服务器已保存"
                } else {
                    "知识库已保存"
                },
            );
        }
        Err(error) => {
            let message = format!("保存失败：{error}");
            if let Ok(mut state) = shared.lock() {
                if let Some(editor) = state.tool_editor.as_mut() {
                    editor.error = message.clone();
                }
                state.tool_editor_rev = state.tool_editor_rev.wrapping_add(1);
                state.touch();
            }
            notify(shared, message);
        }
    }
}

// ---- built-in agent ---------------------------------------------------

fn refresh_agent(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let agent = match desktop.default_agent() {
        Ok(agent) => agent,
        Err(error) => {
            notify(shared, format!("无法读取智能体设置：{error}"));
            return;
        }
    };
    let catalog = desktop.list_tools().unwrap_or_default();
    // A fresh load always starts unfiltered; the tick pushes the empty search
    // back so the field and the list never disagree.
    let query = String::new();
    let groups = agent_groups(&catalog, &agent.tool_names, &query);
    let preset_questions = dedup_text(agent.preset_questions.clone(), 12);
    if let Ok(mut state) = shared.lock() {
        let seed = state.agent.seed.wrapping_add(1);
        state.agent = AgentView {
            name: agent.name.clone(),
            description: agent.description.clone(),
            system_prompt: agent.system_prompt.clone(),
            tool_names: agent.tool_names.clone(),
            preset_questions: preset_questions.clone(),
            groups,
            tool_count: agent.tool_names.len() as i32,
            search: query,
            seed,
        };
        state.agent_catalog = catalog;
        state.preset_questions = preset_questions;
        state.preset_rev = state.preset_rev.wrapping_add(1);
        state.agent_rev = state.agent_rev.wrapping_add(1);
        state.touch();
    }
}

fn search_agent_tools(shared: &Arc<Mutex<Shared>>, query: &str) {
    let (catalog, tool_names) = match shared.lock() {
        Ok(state) => (state.agent_catalog.clone(), state.agent.tool_names.clone()),
        Err(_) => return,
    };
    let groups = agent_groups(&catalog, &tool_names, query);
    if let Ok(mut state) = shared.lock() {
        state.agent.search = query.to_string();
        state.agent.groups = groups;
        state.agent_rev = state.agent_rev.wrapping_add(1);
        state.touch();
    }
}

fn persist_agent(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    description: String,
    system_prompt: String,
    tool_names: Vec<String>,
    preset_questions: Vec<String>,
    note: &str,
) {
    let current = match desktop.default_agent() {
        Ok(agent) => agent,
        Err(error) => {
            notify(shared, format!("无法读取智能体设置：{error}"));
            return;
        }
    };
    let icon = {
        let payload = current.icon_config.to_payload();
        if payload.trim().is_empty() {
            None
        } else {
            Some(payload)
        }
    };
    let edit = AgentSettingsEdit {
        name: current.name.clone(),
        description: description.trim().to_string(),
        system_prompt,
        model_name: current.model.clone(),
        icon_name: current.icon_name.clone(),
        icon_color: current.icon_color.clone(),
        icon,
        tool_names: dedup_text(tool_names, usize::MAX),
        preset_questions: dedup_text(preset_questions, 12),
        approval_mode: current.approval_mode.clone(),
        preview_skill: current.preview_skill,
        silent: current.silent,
        prefer_mother: current.prefer_mother,
    };
    match desktop.update_agent_settings(DEFAULT_AGENT_ID, edit) {
        Ok(updated) => {
            let catalog = desktop.list_tools().unwrap_or_default();
            let preset_questions = dedup_text(updated.preset_questions.clone(), 12);
            if let Ok(mut state) = shared.lock() {
                let query = state.agent.search.clone();
                let seed = state.agent.seed;
                let groups = agent_groups(&catalog, &updated.tool_names, &query);
                state.agent = AgentView {
                    name: updated.name.clone(),
                    description: updated.description.clone(),
                    system_prompt: updated.system_prompt.clone(),
                    tool_names: updated.tool_names.clone(),
                    preset_questions: preset_questions.clone(),
                    groups,
                    tool_count: updated.tool_names.len() as i32,
                    search: query,
                    seed,
                };
                state.agent_catalog = catalog;
                state.preset_questions = preset_questions;
                state.preset_rev = state.preset_rev.wrapping_add(1);
                state.agent_rev = state.agent_rev.wrapping_add(1);
                state.notify(note);
                state.touch();
            }
        }
        Err(error) => notify(shared, format!("保存智能体设置失败：{error}")),
    }
}

// ---- cron jobs --------------------------------------------------------

/// RFC3339 or raw stored time rendered as local "YYYY-MM-DD HH:MM".
fn format_local_at(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return parsed
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
    }
    trimmed.to_string()
}

/// Cron run timestamps arrive as RFC3339 with the server offset; render them
/// in the phone's local zone like the rest of the settings surfaces.
fn cron_run_time(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return parsed
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
    }
    trimmed.to_string()
}

fn format_memory_time(updated_at: f64) -> String {
    chrono::DateTime::from_timestamp(updated_at as i64, 0)
        .map(|value| {
            value
                .with_timezone(&chrono::Local)
                .format("%Y/%m/%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// CronRecord.schedule is the raw backend summary (RFC3339 for one-shot
/// jobs); the phone renders the same facts as terse local text.
fn cron_schedule_text(job: &wunder_desktop::CronRecord) -> String {
    match job.schedule_kind.as_str() {
        "every" => {
            let seconds = job.every_s.max(0);
            if seconds >= 86_400 && seconds % 86_400 == 0 {
                format!("每 {} 天", seconds / 86_400)
            } else if seconds >= 3_600 && seconds % 3_600 == 0 {
                format!("每 {} 小时", seconds / 3_600)
            } else if seconds >= 60 && seconds % 60 == 0 {
                format!("每 {} 分钟", seconds / 60)
            } else {
                format!("每 {seconds} 秒")
            }
        }
        "cron" => {
            let expression = job.cron_expr.trim();
            let timezone = job.timezone.trim();
            if timezone.is_empty() {
                format!("Cron {expression}")
            } else {
                format!("Cron {expression} · {timezone}")
            }
        }
        _ => {
            let at = format_local_at(&job.schedule_at);
            if at.is_empty() {
                "定时一次".to_string()
            } else {
                format!("定时一次 {at}")
            }
        }
    }
}

/// `next_run` is formatted in UTC by the native façade; show it in the
/// phone's local zone like every other timestamp in settings.
fn cron_next_run_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "未计划" || trimmed == "未知" {
        return String::new();
    }
    if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M") {
        return parsed
            .and_utc()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
    }
    trimmed.to_string()
}

fn refresh_cron(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    match desktop.list_cron_jobs() {
        Ok(jobs) => {
            let jobs = jobs
                .into_iter()
                .map(|job| {
                    let schedule = cron_schedule_text(&job);
                    let next_run = cron_next_run_text(&job.next_run);
                    MobileCronJob {
                        id: job.id.into(),
                        name: job.name.into(),
                        enabled: job.enabled,
                        schedule: schedule.into(),
                        next_run: next_run.into(),
                        workspace_name: job.workspace_name.into(),
                        message: job.message.into(),
                        last_status: job.last_status.into(),
                        last_error: job.last_error.into(),
                    }
                })
                .collect::<Vec<_>>();
            let workspaces = desktop
                .list_workspaces()
                .unwrap_or_default()
                .into_iter()
                .map(|workspace| CronWorkspaceRef {
                    id: workspace.workspace_id.into(),
                    name: workspace.name.into(),
                })
                .collect::<Vec<_>>();
            if let Ok(mut state) = shared.lock() {
                state.cron.jobs = jobs;
                state.cron.workspaces = workspaces;
                state.cron.loading = false;
                state.cron.error.clear();
                state.cron_rev = state.cron_rev.wrapping_add(1);
                state.touch();
            }
        }
        Err(error) => {
            if let Ok(mut state) = shared.lock() {
                state.cron.loading = false;
                state.cron.error = format!("无法读取定时任务：{error}");
                state.cron_rev = state.cron_rev.wrapping_add(1);
                state.touch();
            }
        }
    }
}

fn set_cron_editor_error(shared: &Arc<Mutex<Shared>>, message: String) {
    if let Ok(mut state) = shared.lock() {
        if let Some(editor) = state.cron_editor.as_mut() {
            editor.error = message;
            editor.saving = false;
        }
        state.cron_editor_rev = state.cron_editor_rev.wrapping_add(1);
        state.touch();
    }
}

fn open_cron_editor(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, id: Option<&str>) {
    let workspaces = match desktop.list_workspaces() {
        Ok(workspaces) => workspaces,
        Err(error) => {
            notify(shared, format!("无法读取工作区列表：{error}"));
            return;
        }
    };
    let current_workspace = shared
        .lock()
        .map(|state| state.workspace_id.clone())
        .unwrap_or_default();
    let default_workspace = workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == current_workspace)
        .or_else(|| workspaces.first())
        .map(|workspace| workspace.name.clone())
        .unwrap_or_default();
    let (draft, is_new) = match id {
        Some(id) if !id.trim().is_empty() => match desktop.get_cron_job(id) {
            Ok(job) => {
                let (every_text, interval_unit) =
                    if job.every_s >= 86400 && job.every_s % 86400 == 0 {
                        ((job.every_s / 86400).to_string(), "day")
                    } else if job.every_s >= 3600 && job.every_s % 3600 == 0 {
                        ((job.every_s / 3600).to_string(), "hour")
                    } else if job.every_s > 0 && job.every_s % 60 == 0 {
                        ((job.every_s / 60).to_string(), "minute")
                    } else if job.every_s > 0 {
                        (job.every_s.to_string(), "second")
                    } else {
                        ("5".to_string(), "minute")
                    };
                (
                    MobileCronDraft {
                        id: job.id.clone().into(),
                        name: job.name.clone().into(),
                        schedule_kind: job.schedule_kind.clone().into(),
                        schedule_at: format_local_at(&job.schedule_at).into(),
                        every: every_text.into(),
                        interval_unit: interval_unit.into(),
                        cron_expr: job.cron_expr.clone().into(),
                        timezone: job.timezone.clone().into(),
                        message: job.message.clone().into(),
                        workspace_name: if job.workspace_name.trim().is_empty() {
                            default_workspace.as_str().into()
                        } else {
                            job.workspace_name.as_str().into()
                        },
                        delete_after: job.delete_after_run,
                        enabled: job.enabled,
                    },
                    false,
                )
            }
            Err(error) => {
                notify(shared, format!("无法读取定时任务：{error}"));
                return;
            }
        },
        _ => (
            MobileCronDraft {
                schedule_kind: "at".into(),
                schedule_at: (chrono::Local::now() + chrono::Duration::minutes(5))
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
                    .into(),
                every: "5".into(),
                interval_unit: "minute".into(),
                workspace_name: default_workspace.as_str().into(),
                enabled: true,
                ..MobileCronDraft::default()
            },
            true,
        ),
    };
    if let Ok(mut state) = shared.lock() {
        state.cron.workspaces = workspaces
            .into_iter()
            .map(|workspace| CronWorkspaceRef {
                id: workspace.workspace_id.into(),
                name: workspace.name.into(),
            })
            .collect();
        state.cron_editor = Some(CronEditorView {
            is_new,
            draft,
            error: String::new(),
            saving: false,
        });
        state.cron_editor_rev = state.cron_editor_rev.wrapping_add(1);
        state.touch();
    }
}

fn close_cron_editor(shared: &Arc<Mutex<Shared>>) {
    if let Ok(mut state) = shared.lock() {
        state.cron_editor = None;
        state.cron_editor_rev = state.cron_editor_rev.wrapping_add(1);
        state.touch();
    }
}

fn save_cron(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, draft: MobileCronDraft) {
    let message = draft.message.trim().to_string();
    if message.is_empty() {
        set_cron_editor_error(shared, "执行消息不能为空".into());
        return;
    }
    let name = {
        let trimmed = draft.name.trim();
        if trimmed.is_empty() {
            message.chars().take(40).collect::<String>()
        } else {
            trimmed.to_string()
        }
    };
    let interval_unit = draft.interval_unit.as_str();
    let unit_seconds = match interval_unit {
        "hour" => 3600,
        "day" => 86400,
        "second" => 1,
        _ => 60,
    };
    let every_s = draft
        .every
        .trim()
        .parse::<i64>()
        .unwrap_or(0)
        .checked_mul(unit_seconds)
        .unwrap_or(0);
    let schedule_kind = draft.schedule_kind.to_string();
    if schedule_kind == "every" && !(1..=86_400).contains(&every_s) {
        set_cron_editor_error(shared, "重复周期必须为正整数，且不超过 1 天".into());
        return;
    }
    let cron_expr = draft.cron_expr.trim().to_string();
    if schedule_kind == "cron" && cron_expr.is_empty() {
        set_cron_editor_error(shared, "Cron 表达式不能为空".into());
        return;
    }
    let workspace_id = shared
        .lock()
        .ok()
        .and_then(|state| {
            state
                .cron
                .workspaces
                .iter()
                .find(|workspace| workspace.name == draft.workspace_name.as_str())
                .map(|workspace| workspace.id.to_string())
        })
        .unwrap_or_default();
    if workspace_id.is_empty() {
        set_cron_editor_error(shared, "请选择工作区".into());
        return;
    }
    let edit = NativeCronJobEdit {
        job_id: draft.id.trim().to_string(),
        name,
        schedule_kind,
        schedule_at: draft.schedule_at.trim().to_string(),
        every_s,
        cron_expr,
        timezone: draft.timezone.trim().to_string(),
        message,
        workspace_id,
        delete_after_run: draft.delete_after,
        enabled: draft.enabled,
    };
    match desktop.save_cron_job(&edit) {
        Ok(job) => {
            if let Ok(mut state) = shared.lock() {
                state.cron_editor = None;
                state.cron_editor_rev = state.cron_editor_rev.wrapping_add(1);
                state.touch();
            }
            refresh_cron(desktop, shared);
            notify(shared, format!("定时任务「{}」已保存", job.name));
        }
        Err(error) => set_cron_editor_error(shared, format!("保存失败：{error}")),
    }
}

fn open_cron_runs(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, id: &str) {
    let title = shared
        .lock()
        .ok()
        .and_then(|state| {
            state
                .cron
                .jobs
                .iter()
                .find(|job| job.id == id)
                .map(|job| job.name.to_string())
        })
        .unwrap_or_else(|| id.to_string());
    if let Ok(mut state) = shared.lock() {
        state.cron_runs = CronRunsView {
            open: true,
            loading: true,
            title,
            runs: Vec::new(),
            error: String::new(),
        };
        state.cron_runs_rev = state.cron_runs_rev.wrapping_add(1);
        state.touch();
    }
    match desktop.list_cron_job_runs(id) {
        Ok(runs) => {
            let runs = runs
                .into_iter()
                .map(|run| MobileCronRun {
                    status: run.status.into(),
                    trigger: run.trigger.into(),
                    summary: truncate_chars(run.summary, 160).into(),
                    error: truncate_chars(run.error, 160).into(),
                    duration: format_duration(run.duration_ms as f64 / 1000.0).into(),
                    created_at: cron_run_time(&run.created_at).into(),
                })
                .collect();
            if let Ok(mut state) = shared.lock() {
                state.cron_runs.loading = false;
                state.cron_runs.runs = runs;
                state.cron_runs_rev = state.cron_runs_rev.wrapping_add(1);
                state.touch();
            }
        }
        Err(error) => {
            if let Ok(mut state) = shared.lock() {
                state.cron_runs.loading = false;
                state.cron_runs.error = format!("无法读取运行记录：{error}");
                state.cron_runs_rev = state.cron_runs_rev.wrapping_add(1);
                state.touch();
            }
        }
    }
}

fn close_cron_runs(shared: &Arc<Mutex<Shared>>) {
    if let Ok(mut state) = shared.lock() {
        state.cron_runs = CronRunsView::default();
        state.cron_runs_rev = state.cron_runs_rev.wrapping_add(1);
        state.touch();
    }
}

// ---- memory fragments -------------------------------------------------

fn refresh_memory(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, reset_filters: bool) {
    let (query, category, all) = if reset_filters {
        if let Ok(mut state) = shared.lock() {
            state.memory.query.clear();
            state.memory.category = "全部标签".into();
            state.memory.seed = state.memory.seed.wrapping_add(1);
        }
        (String::new(), "全部标签".to_string(), true)
    } else {
        shared
            .lock()
            .map(|state| {
                (
                    state.memory.query.clone(),
                    state.memory.category.clone(),
                    state.memory.category == "全部标签",
                )
            })
            .unwrap_or((String::new(), "全部标签".to_string(), true))
    };
    let filter_category = if all { "" } else { category.as_str() };
    match desktop.expert_memories(DEFAULT_AGENT_ID, &query, filter_category) {
        Ok(rows) => {
            let mut categories = vec!["全部标签".to_string()];
            if all {
                for row in &rows {
                    if !row.category.is_empty()
                        && !categories.iter().any(|item| item == &row.category)
                    {
                        categories.push(row.category.clone());
                    }
                }
            }
            let memories = rows
                .into_iter()
                .map(|row| MobileMemory {
                    id: row.id.into(),
                    title: row.title.into(),
                    content: row.content.into(),
                    category: row.category.into(),
                    source: if row.source == "manual" {
                        "手动写入".into()
                    } else {
                        "模型写入".into()
                    },
                    time: format_memory_time(row.updated_at).into(),
                })
                .collect::<Vec<_>>();
            if let Ok(mut state) = shared.lock() {
                state.memory.memories = memories;
                if all {
                    state.memory.categories = categories;
                }
                state.memory.loading = false;
                state.memory.error.clear();
                state.memory_rev = state.memory_rev.wrapping_add(1);
                state.touch();
            }
        }
        Err(error) => {
            if let Ok(mut state) = shared.lock() {
                state.memory.loading = false;
                state.memory.error = format!("无法读取记忆碎片：{error}");
                state.memory_rev = state.memory_rev.wrapping_add(1);
                state.touch();
            }
        }
    }
}

/// Local runtime usage for the account page, mirroring 桌面版's three tiles
/// (runtime / consumed tokens / quota used). Reads the same engine summary the
/// desktop expert page uses; independent of cloud sign-in.
fn refresh_usage(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let Ok(value) = desktop.expert_runtime(DEFAULT_AGENT_ID, None) else {
        return;
    };
    let summary = &value["data"]["summary"];
    let seconds = summary["runtime_seconds"].as_f64().unwrap_or(0.0) as u64;
    let duration = format!("{}h {}m", seconds / 3600, seconds % 3600 / 60);
    let tokens = compact_usage(summary["consumed_tokens"].as_f64().unwrap_or(0.0));
    let quota = compact_usage(summary["quota_consumed"].as_f64().unwrap_or(0.0));
    if let Ok(mut state) = shared.lock() {
        state.usage_duration = duration;
        state.usage_tokens = tokens;
        state.usage_quota = quota;
        state.usage_rev = state.usage_rev.wrapping_add(1);
        state.touch();
    }
}

fn compact_usage(value: f64) -> String {
    if value >= 1_000_000.0 {
        format!("{:.2}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else {
        format!("{value:.0}")
    }
}

fn open_memory_editor(shared: &Arc<Mutex<Shared>>, index: i32) {
    let row = if index >= 0 {
        shared
            .lock()
            .ok()
            .and_then(|state| state.memory.memories.get(index as usize).cloned())
    } else {
        None
    };
    let (draft, is_new) = match row {
        Some(memory) => {
            let category = if memory.category.trim().is_empty() {
                "session_summary".into()
            } else {
                memory.category
            };
            (
                MobileMemoryDraft {
                    id: memory.id,
                    title: memory.title,
                    content: memory.content,
                    category,
                },
                false,
            )
        }
        None if index < 0 => (
            MobileMemoryDraft {
                category: "session_summary".into(),
                ..MobileMemoryDraft::default()
            },
            true,
        ),
        None => return,
    };
    if let Ok(mut state) = shared.lock() {
        state.memory_editor = Some(MemoryEditorView {
            is_new,
            draft,
            error: String::new(),
            saving: false,
        });
        state.memory_editor_rev = state.memory_editor_rev.wrapping_add(1);
        state.touch();
    }
}

fn close_memory_editor(shared: &Arc<Mutex<Shared>>) {
    if let Ok(mut state) = shared.lock() {
        state.memory_editor = None;
        state.memory_editor_rev = state.memory_editor_rev.wrapping_add(1);
        state.touch();
    }
}

fn save_memory(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, draft: MobileMemoryDraft) {
    let title = draft.title.trim().to_string();
    let content = draft.content.trim().to_string();
    if title.is_empty() || content.is_empty() {
        if let Ok(mut state) = shared.lock() {
            if let Some(editor) = state.memory_editor.as_mut() {
                editor.error = "标题和内容不能为空".into();
            }
            state.memory_editor_rev = state.memory_editor_rev.wrapping_add(1);
            state.touch();
        }
        return;
    }
    let category = draft.category.trim().to_string();
    match desktop.save_expert_memory(
        DEFAULT_AGENT_ID,
        draft.id.trim(),
        &title,
        &content,
        &category,
    ) {
        Ok(()) => {
            if let Ok(mut state) = shared.lock() {
                state.memory_editor = None;
                state.memory_editor_rev = state.memory_editor_rev.wrapping_add(1);
                state.touch();
            }
            refresh_memory(desktop, shared, false);
            notify(shared, "记忆碎片已保存");
        }
        Err(error) => {
            if let Ok(mut state) = shared.lock() {
                if let Some(editor) = state.memory_editor.as_mut() {
                    editor.error = format!("保存失败：{error}");
                }
                state.memory_editor_rev = state.memory_editor_rev.wrapping_add(1);
                state.touch();
            }
        }
    }
}

fn delete_memory(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, id: &str) {
    match desktop.delete_expert_memory(DEFAULT_AGENT_ID, id) {
        Ok(()) => {
            if let Ok(mut state) = shared.lock() {
                state.memory_editor = None;
                state.memory_editor_rev = state.memory_editor_rev.wrapping_add(1);
                state.touch();
            }
            refresh_memory(desktop, shared, false);
            notify(shared, "记忆碎片已删除");
        }
        Err(error) => {
            if let Ok(mut state) = shared.lock() {
                if let Some(editor) = state.memory_editor.as_mut() {
                    editor.error = format!("删除失败：{error}");
                }
                state.memory_editor_rev = state.memory_editor_rev.wrapping_add(1);
                state.touch();
            }
        }
    }
}

/// Read one directory of the workspace. The listing itself is bounded (200
/// entries per page) and cheap enough for the engine thread; uploads use the
/// same path so both entries land in the directory the user is looking at.
/// Project one durable workspace record onto the phone row. The catalogue is
/// bounded by the store's own workspace count (users create a handful), so the
/// full clone on the 33 ms tick stays cheap.
fn mobile_workspace(record: &wunder_desktop::NativeWorkspace, active: bool) -> MobileWorkspace {
    MobileWorkspace {
        id: record.workspace_id.as_str().into(),
        name: record.name.as_str().into(),
        icon: record.icon.as_str().into(),
        color: record.color.as_str().into(),
        root: record.root_path.as_str().into(),
        thread_count: record.thread_count.min(i32::MAX as i64) as i32,
        active,
    }
}

/// Reload the catalogue and keep the current selection when it still exists.
/// The opened thread's workspace wins over a stale selection the first time
/// this runs after startup.
fn refresh_workspaces(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) -> anyhow::Result<()> {
    let list = desktop.list_workspaces()?;
    // The system DocumentsProvider is read-only and mirrors this catalogue;
    // push it before the UI lock so Binder readers never wait on the engine.
    #[cfg(target_os = "android")]
    crate::workspace_docs::publish(&list);
    let Ok(mut state) = shared.lock() else {
        return Ok(());
    };
    let preferred = if state.selected_workspace.trim().is_empty() {
        state.workspace_id.clone()
    } else {
        state.selected_workspace.clone()
    };
    let selected = list
        .iter()
        .find(|workspace| workspace.workspace_id == preferred)
        .or_else(|| list.first())
        .map(|workspace| workspace.workspace_id.clone())
        .unwrap_or_default();
    state.workspaces = list
        .iter()
        .map(|workspace| mobile_workspace(workspace, workspace.workspace_id == selected))
        .collect();
    state.selected_workspace = selected;
    if let Some(active) = state.workspaces.iter().find(|workspace| workspace.active) {
        state.workspace = active.name.to_string();
    }
    state.workspaces_rev = state.workspaces_rev.wrapping_add(1);
    state.touch();
    Ok(())
}

/// Make one workspace the target for new tasks and for 工作目录.
fn select_workspace(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    id: &str,
) -> anyhow::Result<()> {
    let list = desktop.list_workspaces()?;
    let cleaned = id.trim();
    let target = list
        .iter()
        .find(|workspace| workspace.workspace_id == cleaned)
        .ok_or_else(|| anyhow::anyhow!("工作区不存在"))?;
    let cards = list
        .iter()
        .map(|workspace| mobile_workspace(workspace, workspace.workspace_id == cleaned))
        .collect::<Vec<_>>();
    let Ok(mut state) = shared.lock() else {
        return Ok(());
    };
    state.workspaces = cards;
    state.selected_workspace = target.workspace_id.clone();
    // The composer chip follows the selected workspace, matching the desktop
    // shell; the thread subtitle keeps its own workspace.
    state.workspace = target.name.clone();
    state.workspaces_rev = state.workspaces_rev.wrapping_add(1);
    state.touch();
    Ok(())
}

/// Create a workspace folder under the app's managed workspace root and bind a
/// durable record to it. 安卓版 has no host folder chooser that proot could
/// reach, so the folder name is derived from the display name and kept unique.
fn managed_workspace_base(desktop: &NativeDesktop) -> anyhow::Result<std::path::PathBuf> {
    let settings = desktop.get_desktop_settings()?;
    Ok(std::path::PathBuf::from(settings.workspace_root)
        .join(desktop.user_id())
        .join("workspaces"))
}

fn managed_workspace_slug(name: &str, base: &std::path::Path) -> String {
    let mut cleaned = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            cleaned.push('-');
        } else {
            cleaned.push(ch);
        }
    }
    let cleaned = cleaned.trim().trim_matches('.').trim();
    let stem: String = if cleaned.is_empty() {
        "workspace".to_string()
    } else {
        cleaned.chars().take(40).collect()
    };
    let mut candidate = stem.clone();
    let mut suffix = 2;
    while base.join(&candidate).exists() {
        candidate = format!("{stem}-{suffix}");
        suffix += 1;
    }
    candidate
}

fn create_managed_workspace(
    desktop: &NativeDesktop,
    name: &str,
    icon: &str,
    color: &str,
    folder: &str,
) -> anyhow::Result<wunder_desktop::NativeWorkspace> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("请输入工作区名称");
    }
    if name.chars().count() > 40 {
        anyhow::bail!("工作区名称不能超过 40 个字符");
    }
    if name.chars().any(char::is_control) {
        anyhow::bail!("工作区名称无效");
    }
    let chosen = folder.trim();
    // A picked folder is used as-is; an empty choice keeps the managed
    // app-private folder so quick creation still needs no storage access.
    let (root, managed) = if chosen.is_empty() {
        let base = managed_workspace_base(desktop)?;
        std::fs::create_dir_all(&base)?;
        (base.join(managed_workspace_slug(name, &base)), true)
    } else {
        (std::path::PathBuf::from(chosen), false)
    };
    if managed {
        std::fs::create_dir_all(&root)?;
    }
    match desktop.create_workspace(name, root.to_string_lossy().as_ref(), icon, color) {
        Ok(workspace) => Ok(workspace),
        Err(error) => {
            // Leave no orphan managed folder behind when the record write is
            // rejected. A user-picked folder is never removed.
            if managed {
                let _ = std::fs::remove_dir(&root);
            }
            Err(error)
        }
    }
}

fn create_workspace(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    name: &str,
    icon: &str,
    color: &str,
    folder: &str,
) -> Option<String> {
    match create_managed_workspace(desktop, name, icon, color, folder) {
        Ok(workspace) => {
            if let Err(error) = select_workspace(desktop, shared, &workspace.workspace_id) {
                log::warn!("select the new workspace failed: {error:#}");
            }
            notify(shared, format!("已创建工作区「{}」", workspace.name));
            Some(workspace.workspace_id)
        }
        Err(error) => {
            notify(shared, format!("无法创建工作区：{error}"));
            None
        }
    }
}

fn update_workspace(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    id: &str,
    name: &str,
    icon: &str,
    color: &str,
    folder: &str,
) {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
        notify(shared, "工作区名称无效");
        return;
    }
    let current = match desktop.get_workspace(id) {
        Ok(workspace) => workspace,
        Err(error) => {
            notify(shared, format!("无法读取工作区：{error}"));
            return;
        }
    };
    let chosen = folder.trim();
    let edit = WorkspaceEdit {
        workspace_id: current.workspace_id.clone(),
        name: name.to_string(),
        root_path: if chosen.is_empty() {
            current.root_path.clone()
        } else {
            chosen.to_string()
        },
        icon: icon.to_string(),
        color: color.to_string(),
    };
    match desktop.update_workspace(&edit) {
        Ok(_) => {
            if let Err(error) = refresh_workspaces(desktop, shared) {
                log::warn!("refresh workspace catalogue failed: {error:#}");
            }
            let active = shared
                .lock()
                .map(|state| state.session_id.clone())
                .unwrap_or_default();
            refresh_sessions(desktop, shared, Some(&active));
            notify(shared, "工作区已更新");
        }
        Err(error) => notify(shared, format!("无法更新工作区：{error}")),
    }
}

/// Delete a workspace binding. Threads are archived (the desktop dialog's
/// recommended default); its folder and files stay on disk. When the open
/// thread lived in the deleted workspace, the phone moves to the newest thread
/// of the next workspace or opens a fresh one there.
fn delete_workspace(
    desktop: &Arc<NativeDesktop>,
    shared: &Arc<Mutex<Shared>>,
    id: &str,
    generation: &Arc<AtomicU64>,
    watch_cancel: &mut Option<NativeThreadWatchCancel>,
) {
    let id = id.trim();
    if id.is_empty() {
        return;
    }
    let list = match desktop.list_workspaces() {
        Ok(list) => list,
        Err(error) => {
            notify(shared, format!("无法读取工作区：{error}"));
            return;
        }
    };
    if list.len() <= 1 {
        notify(shared, "至少保留一个工作区");
        return;
    }
    let name = list
        .iter()
        .find(|workspace| workspace.workspace_id == id)
        .map(|workspace| workspace.name.clone())
        .unwrap_or_default();
    let current_inside = shared
        .lock()
        .map(|state| state.workspace_id == id)
        .unwrap_or(false);
    match desktop.delete_workspace(id, false) {
        Ok(_) => {
            if let Err(error) = refresh_workspaces(desktop, shared) {
                log::warn!("refresh workspace catalogue failed: {error:#}");
            }
            if current_inside {
                let next_workspace = shared
                    .lock()
                    .map(|state| state.selected_workspace.clone())
                    .unwrap_or_default();
                let next_session = desktop
                    .list_sessions(Some(next_workspace.as_str()))
                    .ok()
                    .and_then(|sessions| sessions.first().map(|session| session.id.clone()));
                if let Some(session) = next_session {
                    let active = shared.lock().unwrap().session_id.clone();
                    refresh_sessions(desktop, shared, Some(&active));
                    if let Err(error) =
                        select_session(desktop, shared, &session, generation, watch_cancel, false)
                    {
                        notify(shared, format!("无法打开会话：{error}"));
                    }
                } else if let Err(error) = open_thread_in_workspace(
                    desktop,
                    shared,
                    generation,
                    watch_cancel,
                    Some(&next_workspace),
                ) {
                    notify(shared, format!("无法新建会话：{error}"));
                }
            } else {
                let active = shared.lock().unwrap().session_id.clone();
                refresh_sessions(desktop, shared, Some(&active));
            }
            notify(shared, format!("已删除工作区「{name}」"));
        }
        Err(error) => notify(shared, format!("无法删除工作区：{error}")),
    }
}

/// Open the workspace's untouched draft thread, or create a new thread when
/// there is none. Mirrors the desktop `new-thread` rule: selecting a workspace
/// and pressing new-task never stacks empty sessions.
fn open_thread_in_workspace(
    desktop: &Arc<NativeDesktop>,
    shared: &Arc<Mutex<Shared>>,
    generation: &Arc<AtomicU64>,
    watch_cancel: &mut Option<NativeThreadWatchCancel>,
    workspace: Option<&str>,
) -> anyhow::Result<()> {
    let workspace = match workspace.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => {
            select_workspace(desktop, shared, id)?;
            id.to_string()
        }
        None => shared
            .lock()
            .map(|state| {
                if state.selected_workspace.trim().is_empty() {
                    state.workspace_id.clone()
                } else {
                    state.selected_workspace.clone()
                }
            })
            .unwrap_or_default(),
    };
    let workspace_arg = (!workspace.trim().is_empty()).then_some(workspace.as_str());
    let draft = desktop
        .list_sessions(workspace_arg)
        .ok()
        .and_then(|sessions| {
            sessions
                .into_iter()
                .find(|session| {
                    session.draft
                        && !matches!(
                            session.runtime_status.as_str(),
                            "running" | "queued" | "pending"
                        )
                })
                .map(|session| session.id)
        });
    let target = match draft {
        Some(id) => id,
        None => desktop.create_session(workspace_arg)?.id,
    };
    refresh_sessions(desktop, shared, Some(&target));
    select_session(desktop, shared, &target, generation, watch_cancel, false)
}

/// The browsing root for the phone's 工作目录. Prefer the workspace the user
/// selected (or the opened thread focuses); a thread is bound to one durable
/// workspace record, so its root is the fallback when the selection is stale.
fn active_workspace_root(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
) -> Result<String, String> {
    let (selected, thread_workspace) = shared
        .lock()
        .map(|state| (state.selected_workspace.clone(), state.workspace_id.clone()))
        .unwrap_or_default();
    for workspace_id in [selected, thread_workspace] {
        if workspace_id.trim().is_empty() {
            continue;
        }
        if let Ok(workspace) = desktop.get_workspace(&workspace_id) {
            if !workspace.root_path.trim().is_empty() {
                return Ok(workspace.root_path);
            }
        }
    }
    if let Ok(workspaces) = desktop.list_workspaces() {
        if let Some(workspace) = workspaces
            .into_iter()
            .find(|workspace| !workspace.root_path.trim().is_empty())
        {
            return Ok(workspace.root_path);
        }
    }
    log::warn!("no workspace record is available; falling back to the container root");
    if let Ok(roots) = desktop.container_roots() {
        if let Some((_, root)) = roots
            .into_iter()
            .find(|(container_id, _)| *container_id == 0)
        {
            if !root.trim().is_empty() {
                return Ok(root);
            }
        }
    }
    desktop
        .get_desktop_settings()
        .map(|settings| settings.workspace_root)
        .map_err(|error| error.to_string())
}

/// Decoded transcript images kept in the shared slot. Entries are bounded
/// thumbnails, but GIFs hold every frame, so the cache also enforces a total
/// RGBA budget instead of trusting the entry count alone.
const IMAGE_CACHE_LIMIT: usize = 48;
const IMAGE_CACHE_BUDGET: usize = 64 * 1024 * 1024;

fn shared_image_bytes(image: &SharedImage) -> usize {
    image.frames.iter().map(Vec::len).sum()
}

/// Candidate workspace roots for one image source. The active workspace goes
/// first, then every known workspace: a message may reference an absolute path
/// in the workspace it was produced in while the user reads another thread.
fn markdown_image_roots(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) -> Vec<String> {
    let mut roots = Vec::new();
    if let Ok(root) = active_workspace_root(desktop, shared) {
        roots.push(root);
    }
    if let Ok(list) = desktop.list_workspaces() {
        for workspace in list {
            if !workspace.root_path.trim().is_empty() && !roots.contains(&workspace.root_path) {
                roots.push(workspace.root_path);
            }
        }
    }
    if let Ok(containers) = desktop.container_roots() {
        for (_, root) in containers {
            if !root.trim().is_empty() && !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

/// Read and decode one image, trying each root in order. Every attempt still
/// goes through the workspace boundary check.
fn read_markdown_image(
    desktop: &NativeDesktop,
    roots: &[String],
    source: &str,
) -> Option<SharedImage> {
    for root in roots {
        let Ok(bytes) = desktop.workspace_image_bytes(root, source) else {
            continue;
        };
        let image = crate::md::decode_image(&bytes).map(|decoded| SharedImage {
            width: decoded.width,
            height: decoded.height,
            frames: decoded.frames,
            delays: decoded.delays,
        });
        if image.is_some() {
            return image;
        }
    }
    None
}

/// Queue one image decode on a worker. The engine loop stays free for stop,
/// send and settings commands while a large JPEG decodes.
fn start_markdown_image(
    desktop: &Arc<NativeDesktop>,
    shared: &Arc<Mutex<Shared>>,
    sender: &Sender<Command>,
    source: String,
) {
    let source = source.trim().to_owned();
    if source.is_empty() {
        return;
    }
    let roots = match shared.lock() {
        Ok(mut state) => {
            if state.images.contains_key(&source) || !state.images_inflight.insert(source.clone()) {
                return;
            }
            drop(state);
            markdown_image_roots(desktop, shared)
        }
        Err(_) => return,
    };
    let api = desktop.clone();
    let sender = sender.clone();
    let failed_source = source.clone();
    let spawned = thread::Builder::new()
        .name("wunder-android-image".into())
        .spawn(move || {
            let image = read_markdown_image(&api, &roots, &source);
            let _ = sender.send(Command::MarkdownImageLoaded { source, image });
        });
    if spawned.is_err() {
        if let Ok(mut state) = shared.lock() {
            state.images_inflight.remove(&failed_source);
        }
    }
}

fn load_workspace(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, path: &str) {
    load_workspace_page(desktop, shared, path, 0, false);
}

/// `append` continues the current directory after the 200-entry page limit.
/// A failed append keeps the rows already on screen and only raises a toast.
fn load_workspace_page(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    path: &str,
    offset: u64,
    append: bool,
) {
    let root = match active_workspace_root(desktop, shared) {
        Ok(root) => root,
        Err(error) => {
            if let Ok(mut state) = shared.lock() {
                state.workspace_loading = false;
                state.workspace_error = format!("无法读取工作区设置：{error}");
                state.workspace_rev = state.workspace_rev.wrapping_add(1);
                state.touch();
            }
            return;
        }
    };
    if !append {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        state.workspace_loading = true;
        state.workspace_error.clear();
        state.workspace_path = path.to_string();
        if offset == 0 {
            state.workspace_entries.clear();
            state.workspace_total = 0;
        }
        state.workspace_rev = state.workspace_rev.wrapping_add(1);
        state.touch();
    }
    match desktop.workspace_local_listing(&root, path, offset) {
        Ok(listing) => {
            let entries: Vec<WorkspaceFile> = listing
                .entries
                .into_iter()
                .map(|entry| {
                    let dir = entry.kind == "dir";
                    let kind = workspace_kind(&entry.path, dir);
                    WorkspaceFile {
                        name: entry.name.into(),
                        path: entry.path.into(),
                        dir,
                        kind,
                        size_text: if dir {
                            "文件夹".into()
                        } else {
                            human_size(entry.size).into()
                        },
                    }
                })
                .collect();
            let Ok(mut state) = shared.lock() else {
                return;
            };
            state.workspace_path = listing.path.clone();
            state.workspace_parent_path = parent_workspace_path(&listing.path);
            if append {
                state.workspace_entries.extend(entries);
            } else {
                state.workspace_entries = entries;
            }
            state.workspace_total = listing.total;
            state.workspace_loading = false;
            state.workspace_error.clear();
            state.workspace_rev = state.workspace_rev.wrapping_add(1);
            state.touch();
        }
        Err(error) if append => {
            notify(shared, format!("无法加载更多：{error}"));
        }
        Err(error) => {
            let Ok(mut state) = shared.lock() else {
                return;
            };
            state.workspace_loading = false;
            state.workspace_path = path.to_string();
            state.workspace_parent_path = parent_workspace_path(path);
            state.workspace_entries.clear();
            state.workspace_total = 0;
            state.workspace_error = format!("无法读取工作目录：{error}");
            state.workspace_rev = state.workspace_rev.wrapping_add(1);
            state.touch();
        }
    }
}

/// Reload the directory that is currently on screen after a mutation. The
/// operation always targets a child of that directory, so the same path stays
/// valid; a failed reload leaves the old rows on screen.
fn refresh_workspace_after_mutation(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let path = shared
        .lock()
        .map(|state| state.workspace_path.clone())
        .unwrap_or_default();
    load_workspace(desktop, shared, &path);
}

fn rename_workspace_entry(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    path: &str,
    name: &str,
) {
    let root = match active_workspace_root(desktop, shared) {
        Ok(root) => root,
        Err(error) => {
            notify(shared, format!("无法读取工作区设置：{error}"));
            return;
        }
    };
    match desktop.workspace_local_rename(&root, path, name) {
        Ok(()) => {
            notify(shared, "已重命名");
            refresh_workspace_after_mutation(desktop, shared);
        }
        Err(error) => notify(shared, format!("重命名失败：{error:#}")),
    }
}

fn move_workspace_entry(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    path: &str,
    target_dir: &str,
) {
    let root = match active_workspace_root(desktop, shared) {
        Ok(root) => root,
        Err(error) => {
            notify(shared, format!("无法读取工作区设置：{error}"));
            return;
        }
    };
    match desktop.workspace_local_move(&root, path, target_dir) {
        Ok(()) => {
            notify(shared, "已移动");
            refresh_workspace_after_mutation(desktop, shared);
        }
        Err(error) => notify(shared, format!("移动失败：{error:#}")),
    }
}

fn delete_workspace_entry(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, path: &str) {
    let root = match active_workspace_root(desktop, shared) {
        Ok(root) => root,
        Err(error) => {
            notify(shared, format!("无法读取工作区设置：{error}"));
            return;
        }
    };
    match desktop.workspace_local_delete(&root, path) {
        Ok(()) => {
            notify(shared, "已删除");
            refresh_workspace_after_mutation(desktop, shared);
        }
        Err(error) => notify(shared, format!("删除失败：{error:#}")),
    }
}

/// Active workspace id and root for system hand-offs. Unlike the listing
/// fallback, this never invents a root: the document provider needs a
/// registered workspace id.
fn active_workspace_identity(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
) -> Result<(String, String), String> {
    let (selected, thread_workspace) = shared
        .lock()
        .map(|state| (state.selected_workspace.clone(), state.workspace_id.clone()))
        .unwrap_or_default();
    for id in [selected, thread_workspace] {
        if id.trim().is_empty() {
            continue;
        }
        if let Ok(workspace) = desktop.get_workspace(&id) {
            if !workspace.root_path.trim().is_empty() {
                return Ok((workspace.workspace_id, workspace.root_path));
            }
        }
    }
    if let Ok(workspaces) = desktop.list_workspaces() {
        if let Some(workspace) = workspaces
            .into_iter()
            .find(|workspace| !workspace.root_path.trim().is_empty())
        {
            return Ok((workspace.workspace_id, workspace.root_path));
        }
    }
    Err("没有可用的工作区".into())
}

fn open_workspace_entry(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, path: &str) {
    #[cfg(target_os = "android")]
    {
        match active_workspace_identity(desktop, shared) {
            Ok((workspace_id, _root)) => {
                if !crate::android_host::open_workspace_file(&workspace_id, path) {
                    notify(shared, "没有可打开此文件的应用");
                }
            }
            Err(error) => notify(shared, error),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (desktop, shared, path);
        notify(shared, "仅安卓版支持用系统应用打开文件");
    }
}

fn open_workspace_location(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    path: &str,
    dir: bool,
) {
    let relative = if dir {
        path.to_string()
    } else {
        parent_workspace_path(path)
    };
    #[cfg(target_os = "android")]
    {
        match active_workspace_identity(desktop, shared) {
            Ok((workspace_id, _root)) => {
                if !crate::android_host::open_workspace_location(&workspace_id, &relative) {
                    notify(shared, "没有可打开文件管理器的应用");
                }
            }
            Err(error) => notify(shared, error),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (desktop, shared, relative);
        notify(shared, "仅安卓版支持打开系统文件管理器");
    }
}

/// Copy staged picker files into the workspace, keeping existing files by
/// numbering the incoming name. Staged copies and their per-pick directory are
/// removed once handled, whether they succeeded or not.
fn upload_workspace(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    dir: &str,
    files: &[String],
) {
    if files.is_empty() {
        return;
    }
    let root = match active_workspace_root(desktop, shared) {
        Ok(root) => root,
        Err(error) => {
            notify(shared, format!("无法读取工作区设置：{error}"));
            return;
        }
    };
    let mut existing: std::collections::HashSet<String> = desktop
        .workspace_local_listing(&root, dir, 0)
        .map(|listing| {
            listing
                .entries
                .into_iter()
                .map(|entry| entry.name)
                .collect()
        })
        .unwrap_or_default();
    let mut uploaded = 0usize;
    let mut failed = 0usize;
    for file in files {
        let source = std::path::Path::new(file);
        let Some(name) = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            failed += 1;
            let _ = std::fs::remove_file(source);
            continue;
        };
        let unique = unique_upload_name(&existing, &name);
        let mut staged = source.to_path_buf();
        if unique != name {
            let renamed = source.with_file_name(&unique);
            match std::fs::rename(source, &renamed) {
                Ok(()) => staged = renamed,
                Err(error) => {
                    failed += 1;
                    log::warn!("rename a staged upload {name}: {error}");
                    let _ = std::fs::remove_file(source);
                    continue;
                }
            }
        }
        match desktop.workspace_local_upload_to(&root, dir, &staged) {
            Ok(_) => {
                uploaded += 1;
                existing.insert(unique);
            }
            Err(error) => {
                failed += 1;
                log::warn!("upload {name} into the workspace: {error}");
            }
        }
        let _ = std::fs::remove_file(&staged);
        if let Some(parent) = staged.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
    load_workspace(desktop, shared, dir);
    if failed == 0 {
        notify(shared, format!("已上传 {uploaded} 个文件"));
    } else if uploaded == 0 {
        notify(shared, format!("上传失败（{failed} 个文件）"));
    } else {
        notify(shared, format!("已上传 {uploaded} 个，{failed} 个失败"));
    }
}

fn unique_upload_name(existing: &std::collections::HashSet<String>, name: &str) -> String {
    if !existing.contains(name) {
        return name.to_string();
    }
    let path = std::path::Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(name);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    for index in 2..1000 {
        let candidate = format!("{stem} ({index}){extension}");
        if !existing.contains(&candidate) {
            return candidate;
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("{stem}-{stamp}{extension}")
}

/// Resolve one workspace file and hand it to the Android save flow.
#[cfg(target_os = "android")]
fn host_start_voice() -> String {
    crate::android_host::start_voice()
}

#[cfg(not(target_os = "android"))]
fn host_start_voice() -> String {
    "failed:unsupported".into()
}

#[cfg(target_os = "android")]
fn host_stop_voice() -> String {
    crate::android_host::stop_voice()
}

#[cfg(not(target_os = "android"))]
fn host_stop_voice() -> String {
    "failed:unsupported".into()
}

#[cfg(target_os = "android")]
fn host_capture_screenshot() -> String {
    crate::android_host::capture_screenshot()
}

#[cfg(not(target_os = "android"))]
fn host_capture_screenshot() -> String {
    "failed:unsupported".into()
}

fn set_voice_phase(shared: &Arc<Mutex<Shared>>, phase: i32) {
    if let Ok(mut state) = shared.lock() {
        if state.voice_phase == phase {
            return;
        }
        state.voice_phase = phase;
        state.voice_rev = state.voice_rev.wrapping_add(1);
        state.touch();
    }
}

fn toggle_voice(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>) {
    let phase = shared.lock().map(|state| state.voice_phase).unwrap_or(0);
    if phase == 2 {
        return;
    }
    if phase == 1 {
        let status = host_stop_voice();
        if let Some(error) = status.strip_prefix("failed:") {
            set_voice_phase(shared, 0);
            notify(shared, format!("停止录音失败：{error}"));
        } else {
            // The Java capture thread publishes the WAV path next; showing
            // "transcribing" now covers the device teardown gap.
            set_voice_phase(shared, 2);
        }
        return;
    }
    if !desktop.has_asr_model() {
        notify(shared, "请先在设置中配置默认语音识别模型");
        return;
    }
    match host_start_voice() {
        status if status == "recording" => set_voice_phase(shared, 1),
        status if status == "permission" => {
            notify(shared, "请在系统提示中允许录音权限");
        }
        status => notify(shared, format!("无法开始录音：{status}")),
    }
}

fn finish_voice(desktop: &Arc<NativeDesktop>, shared: &Arc<Mutex<Shared>>, path: String) {
    set_voice_phase(shared, 2);
    let desktop = Arc::clone(desktop);
    let worker_shared = shared.clone();
    let spawned = thread::Builder::new()
        .name("wunder-android-asr".into())
        .spawn(move || {
            let result = std::fs::read(&path)
                .map_err(|error| format!("读取录音失败：{error}"))
                .and_then(|bytes| {
                    desktop
                        .transcribe_audio(
                            "wunder-voice.wav".to_string(),
                            "audio/wav".to_string(),
                            bytes,
                        )
                        .map_err(|error| error.to_string())
                });
            let _ = std::fs::remove_file(&path);
            let Ok(mut state) = worker_shared.lock() else {
                return;
            };
            state.voice_phase = 0;
            state.voice_rev = state.voice_rev.wrapping_add(1);
            match result {
                Ok(text) if !text.trim().is_empty() => {
                    state.pending_draft = Some(text);
                    state.notify("语音已转写到输入区，可编辑后发送");
                }
                Ok(_) => state.notify("语音识别没有返回文字"),
                Err(error) => state.notify(format!("语音识别失败：{error}")),
            }
            state.touch();
        });
    if let Err(error) = spawned {
        log::warn!("start the asr worker: {error}");
        set_voice_phase(shared, 0);
        notify(shared, "无法启动语音识别");
    }
}

fn start_screenshot(shared: &Arc<Mutex<Shared>>) {
    match host_capture_screenshot() {
        status if status == "pending" => notify(shared, "正在截取屏幕…"),
        status => notify(shared, format!("截图失败：{status}")),
    }
}

fn apply_screenshot(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, status: &str) {
    if status == "no_service" {
        notify(shared, "请先在系统设置中开启 wunder 无障碍服务，再使用截图");
        return;
    }
    let Some(source) = status.strip_prefix("ok|") else {
        notify(
            shared,
            format!(
                "截图失败：{}",
                status.strip_prefix("error|").unwrap_or(status)
            ),
        );
        return;
    };
    let saved = (|| -> Result<String, String> {
        let root = active_workspace_root(desktop, shared)?;
        let directory = std::path::Path::new(&root).join("screenshots");
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("无法创建截图目录：{error}"))?;
        let now = chrono::Local::now();
        let name = format!(
            "screenshot-{}-{:03}.png",
            now.format("%Y%m%d-%H%M%S"),
            now.timestamp_subsec_millis()
        );
        std::fs::copy(source, directory.join(&name))
            .map_err(|error| format!("无法写入截图：{error}"))?;
        Ok(format!("screenshots/{name}"))
    })();
    let _ = std::fs::remove_file(source);
    match saved {
        Ok(relative) => notify(shared, format!("截图已保存到工作目录：{relative}")),
        Err(error) => notify(shared, format!("截图保存失败：{error}")),
    }
}

fn download_workspace(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, relative: &str) {
    let root = match active_workspace_root(desktop, shared) {
        Ok(root) => root,
        Err(error) => {
            notify(shared, format!("无法读取工作区设置：{error}"));
            return;
        }
    };
    let target = match desktop.workspace_local_resolve(&root, relative) {
        Ok(target) => target,
        Err(error) => {
            notify(shared, format!("无法读取文件：{error}"));
            return;
        }
    };
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    #[cfg(target_os = "android")]
    let status = {
        let source = target.to_string_lossy().into_owned();
        crate::android_host::save_file_to_phone(&source, &name)
    };
    #[cfg(not(target_os = "android"))]
    let status = "failed:preview".to_string();
    if status == "saved" {
        notify(shared, format!("已保存 {name} 到手机的下载目录"));
    } else if status == "pending" {
        notify(shared, "请在系统对话框中选择保存位置");
    } else {
        notify(shared, format!("保存 {name} 失败"));
    }
}

fn parent_workspace_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn workspace_kind(path: &str, dir: bool) -> i32 {
    if dir {
        return 0;
    }
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "heic" | "heif" => 1,
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "mp3" | "wav" | "m4a" | "flac" | "ogg" => 2,
        "zip" | "rar" | "7z" | "tar" | "gz" | "xz" | "bz2" | "zst" => 3,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "json"
        | "toml" | "yaml" | "yml" | "csv" | "xml" | "html" | "css" | "js" | "ts" | "rs" | "py"
        | "java" | "kt" | "c" | "h" | "cpp" | "hpp" | "sh" | "sql" => 4,
        _ => 5,
    }
}

fn human_size(bytes: u64) -> String {
    let mut amount = bytes as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if amount < 1024.0 {
            if unit == "B" {
                return format!("{bytes} B");
            }
            return if amount < 10.0 {
                format!("{amount:.1} {unit}")
            } else {
                format!("{amount:.0} {unit}")
            };
        }
        amount /= 1024.0;
    }
    format!("{amount:.0} TB")
}

/// Bounded, read-only trajectory projection for the phone page. The active
/// thread is already leased; any other thread is opened for the read and
/// released immediately so the chosen transcript keeps its lease.
fn load_trajectory(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, id: &str, title: &str) {
    {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        state.trajectory_title = title.to_string();
        state.trajectory_loading = true;
        state.trajectory_error.clear();
        state.trajectory_items.clear();
        state.trajectory_audit = TrajectoryAudit::default();
        state.trajectory_segments.clear();
        state.trajectory_bars.clear();
        state.trajectory_filter = 0;
        state.trajectory_rev = state.trajectory_rev.wrapping_add(1);
        state.touch();
    }
    let active = shared
        .lock()
        .map(|state| state.session_id.clone())
        .unwrap_or_default();
    let result = desktop.thread_trajectory_page(id, None, 160);
    if active != id {
        let _ = desktop.close_session(id);
    }
    let Ok(mut state) = shared.lock() else {
        return;
    };
    state.trajectory_loading = false;
    match result {
        Ok(page) => {
            let projection = trajectory_projection(&page);
            state.trajectory_items = projection.items;
            state.trajectory_audit = projection.audit;
            state.trajectory_segments = projection.segments;
            state.trajectory_bars = projection.bars;
        }
        Err(error) => {
            state.trajectory_error = format!("无法读取线程轨迹：{error:#}");
        }
    }
    state.trajectory_rev = state.trajectory_rev.wrapping_add(1);
    state.touch();
}

/// The greeting's eye button asks for the frozen prompt of the open thread.
/// Building it can run agent/tool preparation, so keep it off the engine
/// command loop the same way the desktop shell does.
fn load_prompt_preview(desktop: &Arc<NativeDesktop>, shared: &Arc<Mutex<Shared>>) {
    let (session, cloud, cloud_thread) = match shared.lock() {
        Ok(state) => (
            state.session_id.clone(),
            state.target == 1,
            state.cloud_active.id.clone(),
        ),
        Err(_) => return,
    };
    {
        let Ok(mut state) = shared.lock() else {
            return;
        };
        state.prompt_preview.clear();
        state.prompt_preview_error.clear();
        state.prompt_preview_loading = true;
        state.prompt_preview_rev = state.prompt_preview_rev.wrapping_add(1);
        state.touch();
    }
    let desktop = desktop.clone();
    let shared = shared.clone();
    thread::Builder::new()
        .name("wunder-android-prompt-preview".into())
        .spawn(move || {
            let result = if cloud {
                if cloud_thread.trim().is_empty() {
                    Err(anyhow::anyhow!("云端线程尚未选择"))
                } else {
                    desktop.cloud_thread_system_prompt(&cloud_thread)
                }
            } else {
                let session = (!session.trim().is_empty()).then_some(session.as_str());
                desktop.preview_thread_system_prompt(session, Some(DEFAULT_AGENT_ID))
            };
            let Ok(mut state) = shared.lock() else {
                return;
            };
            // A result for a thread the user already left must not replace the
            // current preview.
            if cloud != (state.target == 1)
                || (cloud && state.cloud_active.id != cloud_thread)
                || (!cloud && state.session_id != session)
            {
                return;
            }
            state.prompt_preview_loading = false;
            match result {
                Ok(prompt) => state.prompt_preview = prompt,
                Err(error) => state.prompt_preview_error = format!("无法读取系统提示词：{error}"),
            }
            state.prompt_preview_rev = state.prompt_preview_rev.wrapping_add(1);
            state.touch();
        })
        .ok();
}

pub(crate) struct TrajectoryProjection {
    pub(crate) items: Vec<TrajectoryItem>,
    pub(crate) audit: TrajectoryAudit,
    pub(crate) segments: Vec<TrajectorySegment>,
    pub(crate) bars: Vec<TrajectoryBar>,
}

/// One timed row before it is normalized into a timeline span and a duration
/// bar. `title` labels the bar when the user taps it.
struct TrajectorySpanPoint {
    start: f64,
    duration: f64,
    kind: i32,
    lane: i32,
    boundary: bool,
    failed: bool,
    time: String,
    title: String,
}

/// Mirror the desktop three-lane timeline: input, model, tool/subagent.
fn trajectory_lane(kind: i32) -> i32 {
    match kind {
        1 | 4 => 1,
        2 | 3 => 2,
        _ => 0,
    }
}

/// Approximate the wrapped line count of a detail dump at the detail font.
/// ASCII glyphs count one cell and wide glyphs two; 52 cells matches the
/// phone's detail panel with a safety margin. The cap keeps a malformed
/// payload from asking for an unbounded panel.
fn detail_line_estimate(text: &str) -> i32 {
    const CELLS_PER_LINE: usize = 52;
    let mut lines = 0usize;
    for line in text.lines() {
        let cells: usize = line
            .chars()
            .map(|ch| if ch.is_ascii() { 1 } else { 2 })
            .sum();
        lines += cells.div_ceil(CELLS_PER_LINE).max(1);
    }
    lines.clamp(1, 60) as i32
}

/// Project the bounded trajectory page into single-line rows plus the
/// page-level audit facts (span, consumption, failures) the top strip shows.
pub(crate) fn trajectory_projection(page: &serde_json::Value) -> TrajectoryProjection {
    let rows = page
        .get("trajectory")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut items = Vec::with_capacity(rows.len());
    let mut current_turn = i64::MIN;
    let mut span_start = f64::INFINITY;
    let mut span_end = f64::NEG_INFINITY;
    let mut spans: Vec<TrajectorySpanPoint> = Vec::with_capacity(rows.len());
    let mut tokens = 0i64;
    let mut tools = 0i32;
    let mut runs = 0i32;
    let mut failed_count = 0i32;
    let mut turns = std::collections::BTreeSet::new();
    for row in &rows {
        let raw_kind = row
            .get("kind")
            .and_then(|value| value.as_str())
            .unwrap_or("context");
        let (kind, title) = trajectory_kind(raw_kind);
        let tool = row
            .get("tool_name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let title = if tool.is_empty() {
            title.to_string()
        } else {
            format!("{title} · {tool}")
        };
        let summary = row
            .get("summary")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let mut detail = String::new();
        if let Some(args) = row.get("args_preview").and_then(|value| value.as_str()) {
            if !args.trim().is_empty() {
                detail.push_str("参数\n");
                detail.push_str(args.trim());
            }
        }
        if let Some(result) = row.get("result_preview").and_then(|value| value.as_str()) {
            if !result.trim().is_empty() {
                if !detail.is_empty() {
                    detail.push_str("\n\n");
                }
                detail.push_str("结果\n");
                detail.push_str(result.trim());
            }
        }
        let raw_status = row
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let detail = truncate_chars(detail, 900);
        let failed = matches!(raw_status, "failed" | "error" | "rejected");
        let status = match raw_status {
            "completed" | "complete" | "ok" => "完成",
            "failed" | "error" | "rejected" => "失败",
            "running" | "streaming" => "运行中",
            "queued" | "pending" => "排队中",
            "cancelled" | "canceled" | "stopped" => "已停止",
            other => other,
        }
        .to_string();
        let turn = row
            .get("user_round")
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        let group = if turn != current_turn {
            current_turn = turn;
            if turn > 0 {
                format!("第 {turn} 轮")
            } else {
                "会话准备".to_string()
            }
        } else {
            String::new()
        };
        let started = row
            .get("started_at")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0);
        let duration = row
            .get("completed_at")
            .and_then(|value| value.as_f64())
            .filter(|end| *end > started)
            .map(|end| end - started)
            .or_else(|| row_duration(row))
            .unwrap_or(0.0);
        let consumed = row_tokens(row);
        let time = format_clock(started);
        let duration_text = format_duration(duration);
        let mut meta = Vec::new();
        if !time.is_empty() {
            meta.push(format!("开始 {time}"));
        }
        if !duration_text.is_empty() {
            meta.push(format!("耗时 {duration_text}"));
        }
        if consumed > 0 {
            meta.push(format!("消耗 {}", format_tokens(consumed)));
        }
        if !status.is_empty() {
            meta.push(format!("状态 {status}"));
        }
        let boundary = !group.is_empty() && turn > 0;
        let detail_lines = detail_line_estimate(&detail);
        items.push(TrajectoryItem {
            group: group.into(),
            kind,
            title: title.into(),
            summary: summary.into(),
            detail: detail.into(),
            detail_lines,
            status: status.into(),
            time: time.into(),
            failed,
            meta: meta.join(" · ").into(),
            round: turn.clamp(0, i64::from(i32::MAX)) as i32,
        });

        if failed {
            failed_count = failed_count.saturating_add(1);
        }
        match kind {
            2 => tools = tools.saturating_add(1),
            1 => runs = runs.saturating_add(1),
            _ => {}
        }
        if turn > 0 {
            turns.insert(turn);
        }
        tokens = tokens.saturating_add(consumed);
        if started.is_finite() && started > 0.0 {
            span_start = span_start.min(started);
            span_end = span_end.max(started + duration.max(0.0));
            let (time, title) = items
                .last()
                .map(|item| (item.time.to_string(), item.title.to_string()))
                .unwrap_or_default();
            spans.push(TrajectorySpanPoint {
                start: started,
                duration: duration.max(0.0),
                kind,
                lane: trajectory_lane(kind),
                boundary,
                failed,
                time,
                title,
            });
        }
    }

    if spans.is_empty() || !span_start.is_finite() || !span_end.is_finite() {
        return TrajectoryProjection {
            items,
            audit: TrajectoryAudit::default(),
            segments: Vec::new(),
            bars: Vec::new(),
        };
    }
    let span = (span_end - span_start).max(0.0);
    let domain = if span > 0.0 { span } else { 1.0 };
    let segments = spans
        .iter()
        .map(|point| TrajectorySegment {
            offset: ((point.start - span_start) / domain).clamp(0.0, 1.0) as f32,
            width: (point.duration / domain).clamp(0.0, 1.0) as f32,
            kind: point.kind,
            lane: point.lane,
            boundary: point.boundary,
            failed: point.failed,
        })
        .collect();
    // Per-step duration histogram. Only model / tool / subagent work consumes
    // time in a way the chart is meant to compare; prompts and system rows
    // stay on the timeline but get no bar.
    let max_duration = spans
        .iter()
        .filter(|point| matches!(point.kind, 1 | 2 | 3))
        .map(|point| point.duration)
        .fold(0.0, f64::max);
    let bars = spans
        .iter()
        .filter(|point| matches!(point.kind, 1 | 2 | 3))
        .map(|point| {
            let reported = point.duration > 0.0;
            let mut label = Vec::new();
            if !point.time.is_empty() {
                label.push(point.time.clone());
            }
            label.push(point.title.clone());
            label.push(if reported {
                format_duration(point.duration)
            } else {
                "耗时未知".to_string()
            });
            TrajectoryBar {
                ratio: if reported && max_duration > 0.0 {
                    (point.duration / max_duration).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                },
                kind: point.kind,
                failed: point.failed,
                reported,
                peak: reported && point.duration >= max_duration,
                label: label.join(" · ").into(),
            }
        })
        .collect();
    let mut summary = Vec::new();
    if tokens > 0 {
        summary.push(format!("消耗 {}", format_tokens(tokens)));
    }
    summary.push(format!("工具 {tools}"));
    if runs > 0 {
        summary.push(format!("模型 {runs}"));
    }
    if !turns.is_empty() {
        summary.push(format!("{} 轮", turns.len()));
    }
    TrajectoryProjection {
        items,
        audit: TrajectoryAudit {
            enabled: true,
            start: format_clock(span_start).into(),
            end: format_clock(span_end).into(),
            span: format_duration(span).into(),
            peak: if max_duration > 0.0 {
                format_duration(max_duration)
            } else {
                String::new()
            }
            .into(),
            summary: summary.join(" · ").into(),
            failed: failed_count,
        },
        segments,
        bars,
    }
}

/// Token usage carried on the trajectory row's bounded `stats` projection.
/// Tool rows have no stats; assistant rows carry one request each.
fn row_tokens(row: &serde_json::Value) -> i64 {
    let stats = row.get("stats");
    stat_number(stats, "request_consumed_tokens")
        .or_else(|| stat_number(stats, "consumed_tokens"))
        .or_else(|| usage_number(stats, "total_tokens"))
        .unwrap_or(0.0)
        .round()
        .clamp(0.0, i32::MAX as f64) as i64
}

/// Own-step duration when the row has no explicit start/end pair. Provider
/// messages persist prefill/decode separately; tool rows carry none.
fn row_duration(row: &serde_json::Value) -> Option<f64> {
    let stats = row.get("stats");
    if let Some(value) = stat_number(stats, "interaction_duration_s") {
        return Some(value);
    }
    let prefill = stat_number(stats, "prefill_duration_s").unwrap_or(0.0);
    let decode = stat_number(stats, "decode_duration_s").unwrap_or(0.0);
    if prefill + decode > 0.0 {
        return Some(prefill + decode);
    }
    stat_number(stats, "duration_s")
}

fn stat_number(stats: Option<&serde_json::Value>, key: &str) -> Option<f64> {
    json_number(stats?.get(key)?)
}

fn usage_number(stats: Option<&serde_json::Value>, key: &str) -> Option<f64> {
    let stats = stats?;
    ["usage", "round_usage", "roundUsage"]
        .iter()
        .find_map(|usage| json_number(stats.get(*usage)?.get(key)?))
}

fn json_number(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return String::new();
    }
    if seconds < 1.0 {
        return format!("{}ms", (seconds * 1000.0).round().max(1.0) as i64);
    }
    if seconds < 60.0 {
        return format!("{seconds:.1}s");
    }
    let total = seconds.round() as i64;
    if total < 3600 {
        return format!("{}m {:02}s", total / 60, total % 60);
    }
    format!("{}h {:02}m", total / 3600, total % 3600 / 60)
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        return format!("{:.1}M", tokens as f64 / 1_000_000.0);
    }
    if tokens >= 10_000 {
        return format!("{:.0}k", tokens as f64 / 1_000.0);
    }
    if tokens >= 1_000 {
        return format!("{:.1}k", tokens as f64 / 1_000.0);
    }
    tokens.to_string()
}

fn trajectory_kind(raw: &str) -> (i32, &'static str) {
    match raw {
        "user_message" | "user" => (0, "用户消息"),
        "assistant_message" | "assistant" => (1, "模型回复"),
        "tool_call" | "tool_result" | "tool_message" | "tool" => (2, "工具调用"),
        "subagent" | "subagent_run" | "subagent_message" => (3, "子智能体"),
        "compaction" | "compacted" | "compaction_summary" => (4, "上下文压缩"),
        "system_message" | "system" => (5, "系统提示"),
        _ => (6, "上下文"),
    }
}

fn truncate_chars(mut text: String, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text;
    }
    text = text.chars().take(limit).collect();
    text.push('…');
    text
}

fn format_clock(seconds: f64) -> String {
    let millis = (seconds * 1000.0).round() as i64;
    if millis <= 0 {
        return String::new();
    }
    let Some(time) =
        chrono::DateTime::from_timestamp(millis / 1000, ((millis % 1000) * 1_000_000) as u32)
    else {
        return String::new();
    };
    let local = time.with_timezone(&chrono::Local);
    if local.date_naive() == chrono::Local::now().date_naive() {
        local.format("%H:%M:%S").to_string()
    } else {
        local.format("%m-%d %H:%M").to_string()
    }
}

fn open_model_editor(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, key: Option<&str>) {
    let settings = desktop.get_desktop_settings();
    let editor = match key {
        Some(key) => {
            let found = settings
                .as_ref()
                .ok()
                .and_then(|settings| settings.models.iter().find(|model| model.key == key));
            let Some(record) = found else {
                notify(shared, "模型不存在");
                return;
            };
            ModelEditor {
                original_key: record.key.clone(),
                card: model_card(record),
                is_default: record.is_default,
                error: String::new(),
                probe_message: String::new(),
                probing: false,
            }
        }
        None => ModelEditor {
            original_key: String::new(),
            card: new_model_card(),
            is_default: false,
            error: String::new(),
            probe_message: String::new(),
            probing: false,
        },
    };
    if let Ok(mut state) = shared.lock() {
        state.editor = Some(editor);
        state.probe_message.clear();
        state.probed_context.clear();
        state.editor_rev = state.editor_rev.wrapping_add(1);
        state.touch();
    }
}

fn probe_model(
    desktop: Arc<NativeDesktop>,
    shared: Arc<Mutex<Shared>>,
    card: ModelCard,
    secret: String,
    original_key: String,
) {
    let provider = card.provider.to_string();
    let model = card.model.to_string();
    let base_url = card.base_url.to_string();
    thread::Builder::new()
        .name("wunder-android-probe".into())
        .spawn(move || {
            let api_key = if secret.trim().is_empty() {
                None
            } else {
                Some(secret.as_str())
            };
            // A blank key means "keep the stored secret"; the key-based probe
            // is the only path that can read it, because secrets never enter
            // the UI projection.
            let outcome = if api_key.is_none() && !original_key.is_empty() {
                desktop.probe_model_context_window(&original_key, None)
            } else {
                desktop.probe_model_window(&provider, &model, &base_url, api_key)
            };
            let (message, context) = match outcome {
                Ok(outcome) => (
                    outcome.message,
                    outcome.max_context.map(|value| value.to_string()),
                ),
                Err(error) => (format!("探测失败：{error:#}"), None),
            };
            if let Ok(mut state) = shared.lock() {
                if let Some(editor) = state.editor.as_mut() {
                    editor.probing = false;
                    editor.probe_message = message.clone();
                    if let Some(value) = &context {
                        editor.card.max_context = value.as_str().into();
                    }
                }
                state.probe_message = message;
                state.probed_context = context.unwrap_or_default();
                state.probe_rev = state.probe_rev.wrapping_add(1);
                state.editor_rev = state.editor_rev.wrapping_add(1);
                state.touch();
            }
        })
        .ok();
}

fn model_edit<'a>(card: &'a ModelCard, secret: &'a str, original_key: &'a str) -> ModelEdit<'a> {
    ModelEdit {
        key: card.key.as_str(),
        original_key,
        provider: card.provider.as_str(),
        model: card.model.as_str(),
        base_url: card.base_url.as_str(),
        api_key: secret,
        model_type: card.model_type.as_str(),
        temperature: card.temperature.as_str(),
        timeout_s: card.timeout_s.as_str(),
        max_output: card.max_output.as_str(),
        thinking_token_budget: card.thinking_token_budget.as_str(),
        max_rounds: card.max_rounds.as_str(),
        max_context: card.max_context.as_str(),
        history_compaction_ratio: card.history_compaction_ratio.as_str(),
        reasoning_effort: card.reasoning_effort.as_str(),
        simulation_speed: card.simulation_speed.as_str(),
        support_vision: card.support_vision,
        support_hearing: card.support_hearing,
        stream_include_usage: card.stream_include_usage,
        tool_call_mode: card.tool_call_mode.as_str(),
        tts_voice: card.tts_voice.as_str(),
        tts_response_format: card.tts_response_format.as_str(),
        tts_speed: card.tts_speed.as_str(),
        tts_instructions: card.tts_instructions.as_str(),
        asr_language: card.asr_language.as_str(),
        asr_response_format: card.asr_response_format.as_str(),
        asr_temperature: card.asr_temperature.as_str(),
        asr_prompt: card.asr_prompt.as_str(),
        image_size: card.image_size.as_str(),
        image_output_format: card.image_output_format.as_str(),
        image_steps: card.image_steps.as_str(),
        image_guidance_scale: card.image_guidance_scale.as_str(),
        image_negative_prompt: card.image_negative_prompt.as_str(),
        video_size: card.video_size.as_str(),
        video_seconds: card.video_seconds.as_str(),
        video_fps: card.video_fps.as_str(),
        video_num_frames: card.video_num_frames.as_str(),
        video_negative_prompt: card.video_negative_prompt.as_str(),
        video_steps: card.video_steps.as_str(),
        video_guidance_scale: card.video_guidance_scale.as_str(),
        video_guidance_scale_2: card.video_guidance_scale_2.as_str(),
        video_boundary_ratio: card.video_boundary_ratio.as_str(),
        video_flow_shift: card.video_flow_shift.as_str(),
        video_frame_interpolation: card.video_frame_interpolation,
        video_sync_mode: card.video_sync_mode,
    }
}

fn new_model_card() -> ModelCard {
    ModelCard {
        key: "".into(),
        provider: "openai_compatible".into(),
        model: "".into(),
        base_url: "".into(),
        model_type: "llm".into(),
        is_default: false,
        is_cloud: false,
        disabled: false,
        temperature: "0.7".into(),
        timeout_s: "120".into(),
        max_output: "32768".into(),
        thinking_token_budget: "4096".into(),
        max_rounds: "1000".into(),
        max_context: "262144".into(),
        history_compaction_ratio: "0.9".into(),
        reasoning_effort: "".into(),
        simulation_speed: "fast".into(),
        support_vision: true,
        support_hearing: false,
        stream_include_usage: true,
        tool_call_mode: "function_call".into(),
        tts_voice: "".into(),
        tts_response_format: "".into(),
        tts_speed: "1".into(),
        tts_instructions: "".into(),
        asr_language: "".into(),
        asr_response_format: "".into(),
        asr_temperature: "0".into(),
        asr_prompt: "".into(),
        image_size: "".into(),
        image_output_format: "".into(),
        image_steps: "30".into(),
        image_guidance_scale: "7.5".into(),
        image_negative_prompt: "".into(),
        video_size: "".into(),
        video_seconds: "4".into(),
        video_fps: "24".into(),
        video_num_frames: "".into(),
        video_negative_prompt: "".into(),
        video_steps: "".into(),
        video_guidance_scale: "".into(),
        video_guidance_scale_2: "".into(),
        video_boundary_ratio: "".into(),
        video_flow_shift: "".into(),
        video_frame_interpolation: false,
        video_sync_mode: false,
    }
}

fn model_card(record: &wunder_desktop::ModelRecord) -> ModelCard {
    ModelCard {
        key: record.key.as_str().into(),
        provider: record.provider.as_str().into(),
        model: record.model.as_str().into(),
        base_url: record.base_url.as_str().into(),
        model_type: record.model_type.as_str().into(),
        is_default: record.is_default,
        is_cloud: false,
        disabled: !record.enabled,
        temperature: record.temperature.as_str().into(),
        timeout_s: record.timeout_s.as_str().into(),
        max_output: record.max_output.as_str().into(),
        thinking_token_budget: record.thinking_token_budget.as_str().into(),
        max_rounds: record.max_rounds.as_str().into(),
        max_context: record.max_context.as_str().into(),
        history_compaction_ratio: record.history_compaction_ratio.as_str().into(),
        reasoning_effort: record.reasoning_effort.as_str().into(),
        simulation_speed: record.simulation_speed.as_str().into(),
        support_vision: record.support_vision,
        support_hearing: record.support_hearing,
        stream_include_usage: record.stream_include_usage,
        tool_call_mode: record.tool_call_mode.as_str().into(),
        tts_voice: record.tts_voice.as_str().into(),
        tts_response_format: record.tts_response_format.as_str().into(),
        tts_speed: record.tts_speed.as_str().into(),
        tts_instructions: record.tts_instructions.as_str().into(),
        asr_language: record.asr_language.as_str().into(),
        asr_response_format: record.asr_response_format.as_str().into(),
        asr_temperature: record.asr_temperature.as_str().into(),
        asr_prompt: record.asr_prompt.as_str().into(),
        image_size: record.image_size.as_str().into(),
        image_output_format: record.image_output_format.as_str().into(),
        image_steps: record.image_steps.as_str().into(),
        image_guidance_scale: record.image_guidance_scale.as_str().into(),
        image_negative_prompt: record.image_negative_prompt.as_str().into(),
        video_size: record.video_size.as_str().into(),
        video_seconds: record.video_seconds.as_str().into(),
        video_fps: record.video_fps.as_str().into(),
        video_num_frames: record.video_num_frames.as_str().into(),
        video_negative_prompt: record.video_negative_prompt.as_str().into(),
        video_steps: record.video_steps.as_str().into(),
        video_guidance_scale: record.video_guidance_scale.as_str().into(),
        video_guidance_scale_2: record.video_guidance_scale_2.as_str().into(),
        video_boundary_ratio: record.video_boundary_ratio.as_str().into(),
        video_flow_shift: record.video_flow_shift.as_str().into(),
        video_frame_interpolation: record.video_frame_interpolation,
        video_sync_mode: record.video_sync_mode,
    }
}

/// Stream threads currently executing in this process. The notification mirrors
/// 桌面版's "运行中 N 线程" title instead of phase detail.
#[cfg(target_os = "android")]
static RUNNING_TURNS: AtomicI32 = AtomicI32::new(0);

/// Releases the foreground notification and wake lock on every exit path of a
/// stream thread, including an unwind. The count stays authoritative when
/// another thread is still running.
#[cfg(target_os = "android")]
struct BusyGuard;

#[cfg(target_os = "android")]
impl BusyGuard {
    fn start() -> Self {
        let running = RUNNING_TURNS.fetch_add(1, Ordering::SeqCst) + 1;
        crate::android_host::set_busy(true);
        crate::android_host::set_running_threads(running);
        Self
    }
}

#[cfg(target_os = "android")]
impl Drop for BusyGuard {
    fn drop(&mut self) {
        let running = RUNNING_TURNS
            .fetch_sub(1, Ordering::SeqCst)
            .saturating_sub(1);
        if running > 0 {
            crate::android_host::set_running_threads(running);
        } else {
            crate::android_host::set_busy(false);
        }
    }
}

/// Keeps the foreground service and wake lock alive for the Ubuntu install
/// without counting as an agent thread, so the notification reads "正在安装
/// 运行环境…" instead of "运行中 1 线程". Released on every exit path.
#[cfg(target_os = "android")]
struct InstallGuard;

#[cfg(target_os = "android")]
impl InstallGuard {
    fn start() -> Self {
        crate::android_host::set_installing(true);
        Self
    }
}

#[cfg(target_os = "android")]
impl Drop for InstallGuard {
    fn drop(&mut self) {
        crate::android_host::set_installing(false);
    }
}

/// Coalesce model deltas into at most one published live frame per UI tick.
/// Structural or phase changes still publish immediately so 请求中/模型输出中,
/// tool admissions and question panels never wait for the frame budget.
const LIVE_PUBLISH_FRAME: Duration = Duration::from_millis(33);

fn start_stream(
    desktop: Arc<NativeDesktop>,
    shared: Arc<Mutex<Shared>>,
    session: String,
    text: String,
    inquiry_turn_id: Option<String>,
) {
    let input = NativeChatInput {
        session_id: session.clone(),
        content: text.clone(),
        client_message_id: None,
        reasoning_effort: None,
        inquiry_turn_id,
        attachments: Vec::new(),
    };
    // Publish the user bubble and 请求中 before the submit call, so the thread
    // reacts to the tap even when opening the lease or building the request
    // takes a moment. The live rows are replaced in place by the first stream
    // frame; a failed submit removes them again and restores the draft.
    let mut live = LiveTurn::new(text.clone());
    let initial_rows = live.rows();
    let stream_id = {
        let mut state = shared.lock().unwrap();
        state.generating = true;
        state.stopping = false;
        state.turn_phase = "requesting".into();
        state.turn_hint.clear();
        state.live_session = session.clone();
        state.live_stream_id = state.live_stream_id.wrapping_add(1);
        let stream_id = state.live_stream_id;
        state.live_root = live.root_id.clone();
        state.live_rows = initial_rows;
        state.live_rev = state.live_rev.wrapping_add(1);
        state.stream_revision = state.stream_revision.wrapping_add(1);
        update_live_subtitle(&mut state, &live);
        state.touch();
        stream_id
    };
    let stream = match desktop.send_chat(input) {
        Ok(stream) => stream,
        Err(error) => {
            if let Ok(mut state) = shared.lock() {
                if state.live_stream_id == stream_id {
                    state.live_rows.clear();
                    state.live_root.clear();
                    state.live_session.clear();
                    state.generating = false;
                    state.pending_draft = Some(text);
                    state.live_rev = state.live_rev.wrapping_add(1);
                    state.stream_revision = state.stream_revision.wrapping_add(1);
                    state.notify(format!("发送失败：{error}"));
                    state.touch();
                }
            }
            return;
        }
    };
    thread::Builder::new()
        .name("wunder-android-stream".into())
        .spawn(move || {
            #[cfg(target_os = "android")]
            let _busy = BusyGuard::start();
            let mut stream = stream;
            let mut last_publish = Instant::now() - LIVE_PUBLISH_FRAME;
            let mut pending = false;
            loop {
                match stream.blocking_recv() {
                    Some(NativeChatEvent::Event(value)) => {
                        let phase_before = live.phase.clone();
                        let root_before = live.root_id.clone();
                        let tools_before = live.tool_count();
                        let raw = value
                            .get("event")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        apply_event(&mut live, &value);
                        if matches!(raw.as_str(), "question_panel" | "question_panel_clear") {
                            apply_inquiry_event(&shared, &value, &session);
                        }
                        if matches!(raw.as_str(), "context_usage" | "round_usage") {
                            apply_context_usage(
                                &shared,
                                wunder_desktop::NativeContextUsage::from_value(&value),
                            );
                        }
                        // 计划工具每次发布整份清单，实时覆盖计划面板；回合结束后
                        // 由落库投影接管。
                        if raw == "plan_update"
                            || value
                                .get("data")
                                .and_then(|data| data.get("source_event"))
                                .and_then(Value::as_str)
                                .is_some_and(|kind| kind == "plan_update")
                        {
                            apply_plan_event(&shared, &value);
                        }
                        pending = true;
                        // A new tool step, a new phase or a turn identity is
                        // structural: publish at once. Token deltas inside the
                        // same phase are coalesced onto the next frame.
                        let structural = live.phase != phase_before
                            || live.root_id != root_before
                            || live.tool_count() != tools_before
                            || matches!(
                                raw.as_str(),
                                "native_turn_identity"
                                    | "native_execution_identity"
                                    | "native_execution_started"
                                    | "native_execution_status"
                                    | "turn_terminal"
                                    | "question_panel"
                                    | "question_panel_clear"
                                    | "error"
                                    | "tool_call"
                                    | "tool_start"
                                    | "tool_result"
                                    | "tool_output"
                                    | "compaction"
                                    | "compaction_start"
                                    | "compaction_completed"
                            );
                        if structural || last_publish.elapsed() >= LIVE_PUBLISH_FRAME {
                            publish_live(&shared, &session, stream_id, &mut live);
                            last_publish = Instant::now();
                            pending = false;
                        }
                    }
                    Some(NativeChatEvent::Queued) => {}
                    Some(NativeChatEvent::Finished) => {
                        log::info!("chat stream finished session={session}");
                        if pending {
                            publish_live(&shared, &session, stream_id, &mut live);
                        }
                        // A stream that produced neither text nor an error is
                        // an empty turn, not a successful reply. Surface it so
                        // the transcript cannot end in silence.
                        if live.answer.trim().is_empty()
                            && live.failure.is_none()
                            && live.tool_count() == 0
                        {
                            live.failure = Some("模型未返回内容，请重试".into());
                            publish_live(&shared, &session, stream_id, &mut live);
                        }
                        break;
                    }
                    Some(NativeChatEvent::Failed(message)) => {
                        log::warn!("chat stream failed session={session}: {message}");
                        notify(&shared, message.clone());
                        live.failure = Some(message);
                        publish_live(&shared, &session, stream_id, &mut live);
                        break;
                    }
                    None => {
                        // The producer disappeared without a terminal event.
                        // Never let that look like a completed turn.
                        log::warn!("chat stream disconnected session={session}");
                        let message = "对话已中断，请重试".to_string();
                        notify(&shared, message.clone());
                        live.failure = Some(message);
                        publish_live(&shared, &session, stream_id, &mut live);
                        break;
                    }
                }
            }
            // The floating companion reads the settled phase and bubble hint
            // from the shared projection. A newer stream or a thread switch
            // owns the slot now; this late result must not paint over it.
            if let Ok(mut state) = shared.lock() {
                if state.live_stream_id == stream_id && state.session_id == session {
                    state.turn_phase = if live.failure.is_some() {
                        "error".into()
                    } else {
                        live.phase.clone()
                    };
                    state.turn_hint =
                        project::bubble_hint(live.failure.as_deref().unwrap_or(&live.answer));
                }
            }
            // The compact post-finish snapshot carries no rounds, so it must
            // not overwrite the durable projection: load the detailed turns so
            // the settled turn keeps its work-process rows.
            let snapshot = desktop.open_chat_snapshot(&session).ok();
            let title = snapshot
                .as_ref()
                .map(|snapshot| snapshot.session.title.clone())
                .filter(|title| !title.trim().is_empty());
            let snapshot_status = snapshot
                .as_ref()
                .map(|snapshot| snapshot.session.runtime_status.clone())
                .filter(|status| {
                    !matches!(
                        status.as_str(),
                        "" | "idle" | "ready" | "running" | "queued" | "pending" | "streaming"
                    )
                })
                .unwrap_or_default();
            if let Ok(turns) = desktop.load_chat_turns(&session) {
                if let Ok(mut state) = shared.lock() {
                    if state.session_id == session {
                        merge_turns(&mut state.durable_turns, turns);
                        state.subagent_count = running_subagents(&state.durable_turns);
                        let rows =
                            project_turns_preserving(&state.durable_turns, &state.durable_rows);
                        state.durable_rows = rows;
                        restore_pending_inquiry(&mut state, &session);
                        state.rows_rev = state.rows_rev.wrapping_add(1);
                        if let Some(title) = title {
                            state.title = title;
                        }
                        if state.live_stream_id == stream_id {
                            state.subtitle =
                                status_subtitle(state.workspace.as_str(), &snapshot_status);
                        }
                    }
                }
            }
            if let Ok(mut state) = shared.lock() {
                // A completed snapshot sometimes lands before the runtime
                // commits the answer. Keep the live rows in that case so the
                // reply the user just watched does not vanish; the UI filters
                // the rows that the durable copy already carries. When the
                // stream never received a turn identity, fall back to matching
                // the settled body/error text instead of the root id.
                if state.live_stream_id == stream_id && state.live_session == session {
                    let visible = state.session_id == session;
                    let covered = visible && live_covered(&state.durable_rows, &live);
                    // Leaving the thread does not stop the turn: drop the
                    // hidden tail instead of holding it for the next selection,
                    // but never touch the generating flag of the thread the
                    // user is actually looking at.
                    if covered || !visible || state.live_rows.is_empty() {
                        state.live_rows.clear();
                        state.live_root.clear();
                    }
                    state.live_session.clear();
                    state.live_rev = state.live_rev.wrapping_add(1);
                    if visible {
                        state.generating = false;
                        state.stopping = false;
                    }
                    state.stream_revision = state.stream_revision.wrapping_add(1);
                    state.touch();
                }
            }
            refresh_context_usage(&desktop, &shared, &session);
            let active = shared
                .lock()
                .map(|state| state.session_id.clone())
                .unwrap_or_default();
            refresh_sessions(&desktop, &shared, Some(&active));
        })
        .ok();
}

/// Thread subtitle from a durable runtime status phrase. English runtime
/// values (`running`, `idle`, `completed`) become the same Chinese labels the
/// transcript footer uses; an idle or settled thread keeps only the workspace.
fn status_subtitle(workspace: &str, status: &str) -> String {
    let label = project::status_metric(status)
        .map(|(_, value)| value)
        .unwrap_or_default();
    join_status(workspace, &label)
}

fn join_status(workspace: &str, label: &str) -> String {
    [label, workspace]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Mirror the live phase into the thread subtitle so the header reflects the
/// same 请求中/模型输出中/使用工具中 status as the transcript footer. Returns
/// whether the subtitle changed, so steady streaming frames do not bump the
/// scalar revision on every token.
fn update_live_subtitle(state: &mut Shared, live: &LiveTurn) -> bool {
    let Some((_, label)) = project::live_status_metric(&live.phase) else {
        return false;
    };
    let subtitle = join_status(state.workspace.as_str(), &label);
    if state.subtitle == subtitle {
        return false;
    }
    state.subtitle = subtitle;
    true
}

fn publish_live(shared: &Arc<Mutex<Shared>>, session: &str, stream_id: u64, live: &mut LiveTurn) {
    // The identity frame can trail an early status frame. Adopt the durable
    // root of the just-submitted turn as soon as it appears, so the live rows
    // and the durable copy of the same turn share one identity and the tick
    // can hide the stale copy instead of rendering both.
    if live.root_id == "live" {
        if let Ok(state) = shared.lock() {
            if state.session_id == session {
                if let Some(root) = project::running_root_for_text(&state.durable_rows, &live.user)
                {
                    live.root_id = root;
                }
            }
        }
    }
    let mut rows = live.rows();
    if let Ok(mut state) = shared.lock() {
        // A replaced stream (another submit took the live slot, or the user
        // switched threads) must not paint its tail into the transcript.
        if state.live_stream_id != stream_id
            || state.session_id != session
            || state.live_session != session
        {
            return;
        }
        // A fold tap writes straight into the published live rows. Carry those
        // choices across stream frames by row/child identity; the live turn is
        // the only writer of these ids, so no stale turn can match them.
        if state.live_root == live.root_id {
            let opens: std::collections::HashMap<&str, bool> = state
                .live_rows
                .iter()
                .filter(|row| row.foldable)
                .map(|row| (row.id.as_str(), row.open))
                .collect();
            let child_opens: std::collections::HashMap<&str, bool> = state
                .live_rows
                .iter()
                .flat_map(|row| row.process_children.iter())
                .map(|child| (child.id.as_str(), child.open))
                .collect();
            for row in &mut rows {
                if let Some(open) = opens.get(row.id.as_str()) {
                    row.open = *open;
                }
                for child in &mut row.process_children {
                    if let Some(open) = child_opens.get(child.id.as_str()) {
                        child.open = *open;
                    }
                }
            }
        }
        state.live_session = session.to_owned();
        state.live_root = live.root_id.clone();
        state.live_rows = rows;
        state.turn_phase = live.phase.clone();
        // Selecting a thread mid-run reads a snapshot that may still say
        // idle. A live frame proves the turn is running, so it re-asserts the
        // busy flag instead of leaving the status footer non-authoritative.
        state.generating = true;
        state.live_rev = state.live_rev.wrapping_add(1);
        state.stream_revision = state.stream_revision.wrapping_add(1);
        if update_live_subtitle(&mut state, live) {
            state.touch();
        }
    }
}

fn durable_has_body(rows: &[RowData], root: &str) -> bool {
    !root.is_empty()
        && root != "live"
        && rows.iter().any(|row| {
            row.id
                .strip_prefix(root)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|rest| rest.starts_with("body") || rest.starts_with("error"))
        })
}

/// True once the durable projection carries the just-finished live turn.
/// Matching by root id is preferred; matching the settled text keeps the
/// fallback working for streams that failed before publishing an identity.
/// An intermediate round's body must not count as coverage, or the answer the
/// user just watched is cleared before the final commit lands.
fn live_covered(rows: &[RowData], live: &LiveTurn) -> bool {
    let root = live.root_id.as_str();
    let belongs = |row: &RowData| {
        !root.is_empty()
            && root != "live"
            && (row.id == root
                || row
                    .id
                    .strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('-')))
    };
    if !live.answer.trim().is_empty() {
        let answer = live.answer.trim();
        if root.is_empty() || root == "live" {
            // Identity-less streams fall back to the settled text; the durable
            // watcher may have committed the same answer under its own root.
            return rows
                .iter()
                .rev()
                .find(|row| row.kind == project::KIND_BODY)
                .is_some_and(|row| row.text.trim() == answer);
        }
        return rows
            .iter()
            .filter(|row| belongs(row) && row.kind == project::KIND_BODY)
            .any(|row| row.text.trim() == answer);
    }
    if let Some(message) = &live.failure {
        let message = message.trim();
        if root.is_empty() || root == "live" {
            return rows
                .iter()
                .rev()
                .find(|row| row.kind == project::KIND_ERROR)
                .is_some_and(|row| row.text.trim() == message);
        }
        return rows
            .iter()
            .filter(|row| belongs(row) && row.kind == project::KIND_ERROR)
            .any(|row| row.text.trim() == message);
    }
    durable_has_body(rows, root)
}

fn apply_event(live: &mut LiveTurn, value: &Value) {
    use wunder_desktop::native::NativeMessageStats;

    let raw = value
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let envelope = value.get("data").unwrap_or(value);
    // Tool frames keep their fields on the envelope; every other frame nests
    // one `data` object. Descending blindly into `data` hid the tool name and
    // arguments until the result frame repeated them.
    let data = if envelope.get("tool").is_some() || envelope.get("tool_name").is_some() {
        envelope
    } else {
        envelope.get("data").unwrap_or(envelope)
    };
    // The online stream collapses every `*_delta` frame into
    // `thread_item_delta` and keeps the semantic type in `source_event`.
    let kind = if raw == "thread_item_delta" {
        data.get("source_event")
            .and_then(Value::as_str)
            .unwrap_or(raw)
    } else {
        raw
    };
    // Round identity of the model frame. A reasoning entry belongs to one
    // round; the next round (or continuation execution) opens a new one.
    let round = data["model_round"].as_i64().unwrap_or(live.round);
    let model_frame = matches!(
        kind,
        "llm_output_delta"
            | "llm_output"
            | "llm_request"
            | "delta"
            | "final"
            | "reasoning"
            | "reasoning_delta"
    );
    if let Some(execution) = data
        .get("turn_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    {
        if live.execution.is_empty() {
            live.execution = execution.to_owned();
        } else if live.execution != execution && model_frame {
            // A late frame from an execution this turn already replaced must
            // not reopen an older round's thinking or answer.
            return;
        }
    }
    if round > live.round && live.round > 0 && model_frame {
        live.close_reason(STATUS_DONE);
    }

    if let Some(round) = data["user_round"]
        .as_i64()
        .or_else(|| envelope["user_round"].as_i64())
        .filter(|round| *round > 0)
    {
        live.user_round = round;
    }

    match kind {
        "native_turn_identity" | "native_execution_identity" => {
            if let Some(root) = data
                .get("root_turn_id")
                .and_then(Value::as_str)
                .filter(|root| !root.is_empty())
            {
                live.root_id = root.to_owned();
            }
        }
        "native_execution_started" => {
            // Background goal executions restart model_round at one while the
            // same user turn keeps its earlier steps.
            live.close_reason(STATUS_DONE);
            live.round = 0;
            live.status = STATUS_RUNNING;
            live.phase = "requesting".into();
            if let Some(execution) = data
                .get("turn_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            {
                live.execution = execution.to_owned();
            }
            if let Some(root) = data
                .get("root_turn_id")
                .and_then(Value::as_str)
                .filter(|root| !root.is_empty())
            {
                live.root_id = root.to_owned();
            }
        }
        "native_execution_status" => {
            match data.get("status").and_then(Value::as_str).unwrap_or("") {
                "queued" | "pending" => {
                    live.status = STATUS_RUNNING;
                    live.phase = "queued".into();
                }
                "waiting_input" | "waiting_user_input" | "waiting_approval" => {
                    live.close_reason(STATUS_DONE);
                    live.phase = "waiting".into();
                }
                "cancelled" | "canceled" | "interrupted" | "stopped" => {
                    live.close_reason(STATUS_STOPPED);
                    live.status = STATUS_STOPPED;
                    live.phase = "stopped".into();
                }
                "failed" | "error" | "rejected" => {
                    live.close_reason(STATUS_FAILED);
                    live.status = STATUS_FAILED;
                    live.phase = "error".into();
                }
                _ => {
                    live.status = STATUS_RUNNING;
                    live.phase = "requesting".into();
                }
            }
            if let Some(stats) = event_message_stats(data) {
                live.stats = stats;
            }
        }
        "reasoning" => {
            if let Some(text) = data
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| data.get("reasoning").and_then(Value::as_str))
            {
                live.append_reasoning(round, text, true);
                live.round = live.round.max(round);
            }
        }
        "reasoning_delta" => {
            if let Some(delta) = data
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| data.get("reasoning_delta").and_then(Value::as_str))
            {
                live.append_reasoning(round, delta, false);
                live.round = live.round.max(round);
            }
        }
        "llm_request" => {
            // Admission of a model call: the previous thought is finished.
            live.close_reason(STATUS_DONE);
            live.phase = "requesting".into();
        }
        "llm_output_delta" | "delta" => {
            if let Some(reasoning) = data
                .get("reasoning_delta")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                live.append_reasoning(round, reasoning, false);
                live.round = live.round.max(round);
            }
            if let Some(delta) = data
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| data.get("content").and_then(Value::as_str))
            {
                if !delta.is_empty() {
                    live.close_reason(STATUS_DONE);
                    live.round = live.round.max(round);
                    live.answer.push_str(delta);
                    live.phase = "outputting".into();
                }
            }
        }
        "llm_output" | "final" => {
            if let Some(reasoning) = data
                .get("reasoning_delta")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                live.append_reasoning(round, reasoning, false);
            }
            if let Some(answer) = data
                .get("answer")
                .and_then(Value::as_str)
                .or_else(|| data.get("content").and_then(Value::as_str))
                .filter(|answer| !answer.is_empty())
            {
                live.close_reason(STATUS_DONE);
                live.answer = answer.to_owned();
            }
            if let Some(stats) = event_message_stats(data) {
                live.stats = stats;
            }
            if kind == "final" {
                live.close_reason(project::STATUS_DONE);
                live.status = project::STATUS_DONE;
                live.phase = "done".into();
            }
        }
        "native_stats" => {
            let stats = data
                .get("message_stats")
                .or_else(|| data.get("data"))
                .unwrap_or(data);
            let parsed = NativeMessageStats::from_payload(stats);
            if !parsed.is_empty() {
                live.stats = parsed;
            }
        }
        "context_usage" | "round_usage" => {
            let usage = wunder_desktop::NativeContextUsage::from_value(value);
            if let Some(used) = usage.used {
                live.stats.context_tokens = Some(used);
            }
        }
        "tool_call" | "tool_start" | "tool_result" | "tool_output" => {
            let pending = matches!(kind, "tool_call" | "tool_start");
            if pending {
                // The model acted: its current thinking entry is complete. A
                // late result frame of an older call must not close a newer
                // thought, so only admission closes it.
                live.close_reason(STATUS_DONE);
            }
            let id = data
                .get("tool_call_id")
                .or_else(|| data.get("item_id"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .unwrap_or("tool");
            let title = data
                .get("tool")
                .or_else(|| data.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or("工具");
            let failed = data["success"] == false
                || data["ok"] == false
                || data["result"]["ok"] == false
                || data["result"]["success"] == false;
            let state = if pending {
                "running"
            } else if failed {
                "failed"
            } else {
                "completed"
            };
            let detail = data
                .get("display_result")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_default();
            let entry = NativeWorkflowEntry::from_payload(id, title, detail, state, data);
            let label = project::tool_label(&entry.title);
            let icon = project::tool_icon_key(&entry.title).to_string();
            let target = project::tool_target(&entry);
            if let Some(existing) = live.tool_mut(&entry.id) {
                existing.title = label;
                existing.tool_icon = icon;
                existing.state = entry.state;
                existing.target = target;
                existing.sections = entry.sections;
                existing.patch = entry.patch_files;
                if !entry.preview.is_empty() {
                    existing.summary = entry.preview;
                }
                if !entry.detail.is_empty() {
                    existing.detail = entry.detail;
                }
                existing.tokens = entry.tokens;
                existing.duration = entry.duration;
            } else {
                live.push_tool(project::LiveTool {
                    id: entry.id,
                    title: label,
                    tool_icon: icon,
                    summary: entry.preview,
                    state: entry.state,
                    detail: entry.detail,
                    target,
                    sections: entry.sections,
                    patch: entry.patch_files,
                    tokens: entry.tokens,
                    duration: entry.duration,
                });
            }
            live.phase = if pending { "tool" } else { "requesting" }.into();
        }
        "command_session_start"
        | "command_session_delta"
        | "command_session_status"
        | "command_session_exit"
        | "command_session_summary" => {
            live.close_reason(STATUS_DONE);
            let session = data
                .get("command_session_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !session.is_empty() {
                let index = data
                    .get("command_index")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let key = format!("{session}#{index}");
                let id = data
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .unwrap_or(key.as_str())
                    .to_owned();
                let tool = data
                    .get("tool_display_name")
                    .and_then(Value::as_str)
                    .unwrap_or("运行命令");
                let mut output = live.command_output.get(&key).cloned().unwrap_or_default();
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    output.push_str(delta);
                    if output.len() > 12_000 {
                        let mut start = output.len() - 12_000;
                        while start < output.len() && !output.is_char_boundary(start) {
                            start += 1;
                        }
                        output = output.split_off(start);
                    }
                    live.command_output.insert(key.clone(), output.clone());
                }
                let command = data.get("command").and_then(Value::as_str).unwrap_or("");
                let terminal = matches!(kind, "command_session_exit" | "command_session_summary");
                let exit_code = data.get("exit_code").and_then(Value::as_i64);
                let failed = exit_code.is_some_and(|code| code != 0)
                    || data["success"] == false
                    || data["ok"] == false
                    || matches!(
                        data.get("status").and_then(Value::as_str),
                        Some("failed" | "failed_to_start")
                    );
                let cancelled = matches!(
                    data.get("status").and_then(Value::as_str),
                    Some("cancelled" | "canceled")
                );
                let state = if !terminal {
                    "running"
                } else if failed {
                    "failed"
                } else {
                    "completed"
                };
                let status_label = if cancelled {
                    "已取消"
                } else if !terminal {
                    "运行中"
                } else if failed {
                    "失败"
                } else {
                    "完成"
                };
                let mut detail = format!("执行 · {status_label}");
                if !command.is_empty() {
                    detail.push('\n');
                    detail.push_str(&command.chars().take(240).collect::<String>());
                }
                if !output.is_empty() {
                    detail.push('\n');
                    detail.push_str(&command_tail(&output));
                }
                if let Some(code) = exit_code {
                    detail.push_str(&format!("\nexit {code}"));
                }
                let summary = command
                    .lines()
                    .next()
                    .map(|line| line.trim().chars().take(120).collect::<String>())
                    .filter(|line| !line.is_empty())
                    .unwrap_or_else(|| status_label.to_string());
                if let Some(existing) = live.tool_mut(&id) {
                    existing.state = state.into();
                    existing.summary = summary;
                    existing.detail = detail;
                } else {
                    live.push_tool(project::LiveTool {
                        id,
                        title: tool.to_owned(),
                        tool_icon: "terminal".into(),
                        summary,
                        state: state.into(),
                        detail,
                        target: String::new(),
                        sections: Vec::new(),
                        patch: Vec::new(),
                        tokens: String::new(),
                        duration: String::new(),
                    });
                }
                live.phase = "tool".into();
            }
        }
        "compaction" | "compaction_completed" | "compaction_start" | "compaction_progress" => {
            live.close_reason(STATUS_DONE);
            let summary = data
                .get("summary_text")
                .and_then(Value::as_str)
                .unwrap_or("上下文压缩");
            let running = matches!(kind, "compaction_start" | "compaction_progress")
                || matches!(data.get("status").and_then(Value::as_str), Some("running"));
            let id = data
                .get("item_id")
                .or_else(|| data.get("compaction_id"))
                .and_then(Value::as_str)
                .unwrap_or("compaction")
                .to_owned();
            let state = if running { "running" } else { "completed" };
            if let Some(existing) = live.tool_mut(&id) {
                existing.state = state.into();
                existing.summary = summary.to_owned();
            } else {
                live.push_tool(project::LiveTool {
                    id,
                    title: "上下文压缩".into(),
                    tool_icon: "compress".into(),
                    summary: summary.to_owned(),
                    state: state.into(),
                    detail: summary.to_owned(),
                    target: String::new(),
                    sections: Vec::new(),
                    patch: Vec::new(),
                    tokens: String::new(),
                    duration: String::new(),
                });
            }
            live.phase = if running { "compacting" } else { "requesting" }.into();
        }
        "question_panel" => {
            live.close_reason(STATUS_DONE);
            live.question_panel = Some(data.clone());
            live.phase = "waiting".into();
        }
        "question_panel_clear" => {
            live.question_panel = None;
            live.phase = "requesting".into();
        }
        "turn_terminal" => {
            if let Some(stats) = event_message_stats(data) {
                live.stats = stats;
            }
            let status = data
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            let (kind_status, phase) = match status {
                "cancelled" | "canceled" | "stopped" | "interrupted" => (STATUS_STOPPED, "stopped"),
                "failed" | "error" | "rejected" => (STATUS_FAILED, "error"),
                "waiting_input" | "waiting_user_input" | "waiting_approval" => {
                    (STATUS_RUNNING, "waiting")
                }
                _ => (project::STATUS_DONE, "done"),
            };
            live.close_reason(kind_status);
            live.status = kind_status;
            live.phase = phase.into();
            if matches!(status, "failed" | "error" | "rejected") {
                if let Some(message) = data
                    .get("message")
                    .or_else(|| data.get("error"))
                    .and_then(Value::as_str)
                {
                    live.failure = Some(message.to_owned());
                }
            }
        }
        "error" => {
            live.close_reason(STATUS_FAILED);
            if let Some(message) = data
                .get("message")
                .or_else(|| data.get("error_message"))
                .and_then(Value::as_str)
            {
                live.failure = Some(message.to_owned());
            }
            live.phase = "error".into();
        }
        _ => {}
    }
}

fn event_message_stats(data: &Value) -> Option<wunder_desktop::native::NativeMessageStats> {
    let stats = data
        .get("message_stats")
        .or_else(|| data.pointer("/meta/message_stats"))?;
    let parsed = wunder_desktop::native::NativeMessageStats::from_payload(stats);
    (!parsed.is_empty()).then_some(parsed)
}

fn command_tail(output: &str) -> String {
    const MAX_LINES: usize = 10;
    const MAX_CHARS: usize = 1200;
    let mut lines: Vec<&str> = output.lines().collect();
    if lines.len() > MAX_LINES {
        lines = lines.split_off(lines.len() - MAX_LINES);
    }
    let mut text = lines.join("\n");
    if text.chars().count() > MAX_CHARS {
        let tail: String = text
            .chars()
            .skip(text.chars().count() - MAX_CHARS)
            .collect();
        text = format!("…{tail}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trajectory_rows_group_turns_and_localize_status() {
        let page = json!({
            "trajectory": [
                {
                    "kind": "user_message", "user_round": 2, "status": "completed",
                    "summary": "继续", "started_at": 1_700_000_000.0
                },
                {
                    "kind": "tool_call", "user_round": 2, "status": "failed",
                    "tool_name": "shell", "summary": "执行命令",
                    "args_preview": "{\"cmd\":\"ls\"}"
                }
            ]
        });
        let items = trajectory_projection(&page).items;
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].group.as_str(), "第 2 轮");
        assert_eq!(items[1].group.as_str(), "");
        assert_eq!(items[0].status.as_str(), "完成");
        assert_eq!(items[1].status.as_str(), "失败");
        assert!(items[1].failed);
        assert_eq!(items[1].title.as_str(), "工具调用 · shell");
        assert!(items[1].detail.contains("参数"));
    }

    #[test]
    fn trajectory_detail_stays_bounded() {
        let page = json!({
            "trajectory": [{
                "kind": "tool_result", "user_round": 1, "status": "completed",
                "args_preview": "x".repeat(4000)
            }]
        });
        let items = trajectory_projection(&page).items;
        assert_eq!(items[0].detail.chars().count(), 901);
        assert!(items[0].detail.ends_with('…'));
    }

    #[test]
    fn trajectory_detail_line_estimate_counts_wrapped_rows() {
        assert_eq!(detail_line_estimate(""), 1);
        assert_eq!(detail_line_estimate("短"), 1);
        assert_eq!(detail_line_estimate("a\nb"), 2);
        assert_eq!(detail_line_estimate(&"x".repeat(52)), 1);
        assert_eq!(detail_line_estimate(&"x".repeat(53)), 2);
        assert_eq!(detail_line_estimate(&"字".repeat(53)), 3);
        assert_eq!(detail_line_estimate(&"x".repeat(10_000)), 60);
    }

    #[test]
    fn trajectory_audit_sums_consumption_and_keeps_the_time_bar() {
        let page = json!({
            "trajectory": [
                {
                    "kind": "user_message", "user_round": 1, "status": "completed",
                    "summary": "开始", "started_at": 1_700_000_000.0,
                    "completed_at": 1_700_000_001.0
                },
                {
                    "kind": "assistant_message", "user_round": 1, "status": "completed",
                    "summary": "回复", "started_at": 1_700_000_005.0,
                    "completed_at": 1_700_000_009.2,
                    "stats": {"request_consumed_tokens": 1240}
                },
                {
                    "kind": "tool_call", "user_round": 1, "status": "failed",
                    "tool_name": "shell", "summary": "失败",
                    "started_at": 1_700_000_010.0, "completed_at": 1_700_000_010.8
                },
                {
                    "kind": "assistant_message", "user_round": 2, "status": "completed",
                    "summary": "继续", "started_at": 1_700_000_020.0,
                    "completed_at": 1_700_000_023.0,
                    "stats": {"usage": {"total_tokens": "860"}}
                }
            ]
        });
        let projection = trajectory_projection(&page);
        assert!(projection.audit.enabled);
        assert_eq!(projection.audit.failed, 1);
        assert_eq!(projection.audit.span.as_str(), "23.0s");
        assert_eq!(projection.audit.peak.as_str(), "4.2s");
        assert!(projection.audit.summary.contains("消耗 2.1k"));
        assert!(projection.audit.summary.contains("工具 1"));
        assert!(projection.audit.summary.contains("模型 2"));
        assert!(projection.audit.summary.contains("2 轮"));
        assert_eq!(projection.segments.len(), 4);
        assert_eq!(projection.segments[0].lane, 0);
        assert_eq!(projection.segments[1].lane, 1);
        assert_eq!(projection.segments[2].lane, 2);
        assert_eq!(projection.segments[3].lane, 1);
        assert!(projection.segments[0].boundary);
        assert!(!projection.segments[1].boundary);
        assert!(!projection.segments[2].boundary);
        assert!(projection.segments[3].boundary);
        assert!((projection.segments[0].offset - 0.0).abs() < f32::EPSILON);
        assert!((projection.segments[3].offset - 20.0 / 23.0).abs() < 0.001);
        assert!((projection.segments[3].width - 3.0 / 23.0).abs() < 0.001);
        assert_eq!(projection.bars.len(), 3);
        assert!(projection.bars[0].reported && projection.bars[0].peak);
        assert!((projection.bars[0].ratio - 1.0).abs() < f32::EPSILON);
        assert_eq!(projection.bars[1].kind, 2);
        assert!(projection.bars[1].failed);
        assert!((projection.bars[1].ratio - 0.8 / 4.2).abs() < 0.001);
        assert!((projection.bars[2].ratio - 3.0 / 4.2).abs() < 0.001);
        assert!(projection.bars[0].label.contains("4.2s"));
        assert!(projection.bars[0].label.contains("模型回复"));
    }

    #[test]
    fn trajectory_bars_mark_unreported_steps_and_lanes() {
        let page = json!({
            "trajectory": [
                {
                    "kind": "tool_call", "user_round": 1, "status": "completed",
                    "tool_name": "shell", "started_at": 100.0
                },
                {
                    "kind": "assistant_message", "user_round": 1, "status": "completed",
                    "started_at": 110.0, "completed_at": 112.0
                }
            ]
        });
        let projection = trajectory_projection(&page);
        assert_eq!(projection.segments[0].lane, 2);
        assert_eq!(projection.segments[1].lane, 1);
        assert_eq!(projection.bars.len(), 2);
        assert!(!projection.bars[0].reported);
        assert_eq!(projection.bars[0].ratio, 0.0);
        assert!(projection.bars[0].label.contains("耗时未知"));
        assert!(projection.bars[1].reported && projection.bars[1].peak);
        assert!((projection.bars[1].ratio - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn trajectory_rows_carry_expanded_audit_meta() {
        let page = json!({
            "trajectory": [{
                "kind": "assistant_message", "user_round": 1, "status": "completed",
                "summary": "回复", "started_at": 1_700_000_000.0,
                "completed_at": 1_700_000_004.2,
                "stats": {"request_consumed_tokens": 1240}
            }]
        });
        let projection = trajectory_projection(&page);
        let item = &projection.items[0];
        assert!(item.meta.contains("耗时 4.2s"));
        assert!(item.meta.contains("消耗 1.2k"));
        assert!(item.meta.contains("状态 完成"));
        assert_eq!(item.round, 1);
    }

    #[test]
    fn workspace_uploads_keep_names_unique_and_kinds_follow_extensions() {
        let mut existing = std::collections::HashSet::new();
        existing.insert("report.md".to_string());
        existing.insert("report (2).md".to_string());
        assert_eq!(unique_upload_name(&existing, "report.md"), "report (3).md");
        assert_eq!(unique_upload_name(&existing, "fresh.txt"), "fresh.txt");
        assert_eq!(workspace_kind("a/photo.JPG", false), 1);
        assert_eq!(workspace_kind("a/archive.zip", false), 3);
        assert_eq!(workspace_kind("a/code.rs", false), 4);
        assert_eq!(workspace_kind("a/unknown.bin", false), 5);
        assert_eq!(workspace_kind("a/folder", true), 0);
        assert_eq!(parent_workspace_path("a/b/c"), "a/b");
        assert_eq!(parent_workspace_path("top"), "");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
    }

    #[test]
    fn stream_delta_reaches_the_live_answer_before_the_final_frame() {
        let mut live = LiveTurn::new("hi".into());
        for delta in ["你", "好"] {
            apply_event(
                &mut live,
                &json!({
                    "event": "thread_item_delta",
                    "data": {"session_id": "s", "data": {
                        "source_event": "llm_output_delta", "delta": delta
                    }}
                }),
            );
        }
        assert_eq!(live.answer, "你好");
        assert_eq!(live.phase, "outputting");
    }

    #[test]
    fn tool_call_shows_a_preview_before_its_result_frame() {
        let mut live = LiveTurn::new("hi".into());
        apply_event(
            &mut live,
            &json!({
                "event": "thread_item_delta",
                "data": {"session_id": "s", "data": {
                    "source_event": "tool_call",
                    "tool": "shell",
                    "tool_call_id": "call-1",
                    "arguments": {"command": "ls -la"}
                }}
            }),
        );
        assert_eq!(live.tool_count(), 1);
        let tool = match &live.steps[0] {
            project::LiveStep::Tool(tool) => tool,
            _ => panic!("tool step"),
        };
        assert_eq!(tool.title, "运行命令");
        assert_eq!(tool.tool_icon, "terminal");
        assert_eq!(tool.summary, "ls -la");
        assert_eq!(tool.state, "running");
        assert_eq!(live.phase, "tool");
    }

    #[test]
    fn live_thinking_entries_split_at_tools_and_rounds() {
        let mut live = LiveTurn::new("hi".into());
        apply_event(
            &mut live,
            &json!({"event":"reasoning_delta","data":{"model_round":1,"delta":"第一段"}}),
        );
        apply_event(
            &mut live,
            &json!({"event":"tool_call","data":{
                "tool":"shell","tool_call_id":"call-1","arguments":{"command":"ls"}
            }}),
        );
        apply_event(
            &mut live,
            &json!({"event":"reasoning_delta","data":{"model_round":2,"delta":"第二段"}}),
        );
        apply_event(
            &mut live,
            &json!({"event":"llm_output_delta","data":{"model_round":2,"delta":"答案"}}),
        );

        let rows = live.rows();
        let bar = rows
            .iter()
            .find(|row| row.kind == project::KIND_PROCESS)
            .expect("process bar");
        assert_eq!(bar.process_children.len(), 3);
        assert_eq!(bar.process_children[0].detail, "第一段");
        assert_eq!(bar.process_children[0].status_kind, project::STATUS_DONE);
        assert_eq!(bar.process_children[1].kind, project::KIND_TOOL);
        assert_eq!(bar.process_children[2].detail, "第二段");
        assert_eq!(bar.process_children[2].status_kind, project::STATUS_DONE);
        // The answer starts a new phase; no thought is left spinning.
        assert!(bar
            .process_children
            .iter()
            .filter(|child| child.kind == project::KIND_REASON)
            .all(|child| child.status_kind != project::STATUS_RUNNING));
    }

    #[test]
    fn intermediate_round_body_never_counts_as_final_coverage() {
        let mut live = LiveTurn::new("hi".into());
        live.root_id = "turn-1".into();
        live.answer = "final answer".into();
        let mut body = RowData::new(project::KIND_BODY, "turn-1-body-final");
        body.text = "earlier round".into();
        assert!(!live_covered(&[body], &live));
        let mut settled = RowData::new(project::KIND_BODY, "turn-1-body-final");
        settled.text = "final answer".into();
        assert!(live_covered(&[settled], &live));
    }

    #[test]
    fn pending_identity_adopts_the_running_durable_root() {
        let mut user = RowData::new(project::KIND_USER, "turn-9-user");
        user.text = "hi".into();
        let mut footer = RowData::new(project::KIND_FOOTER, "turn-9-footer");
        footer.status_kind = project::STATUS_RUNNING;
        assert_eq!(
            project::running_root_for_text(&[user, footer], "hi").as_deref(),
            Some("turn-9")
        );
    }

    #[test]
    fn question_panel_event_unwraps_two_envelopes_and_queues_the_panel() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        shared.lock().unwrap().session_id = "s".into();
        apply_inquiry_event(
            &shared,
            &json!({
                "event": "question_panel",
                "data": {"session_id": "s", "timestamp": "0", "data": {
                    "turn_id": "turn-1",
                    "questions": [{
                        "question": "选哪个方案？",
                        "options": [{"label": "A", "recommended": true}]
                    }]
                }}
            }),
            "s",
        );
        let state = shared.lock().unwrap();
        let panel = state.inquiry.as_ref().expect("the panel is queued");
        assert_eq!(panel.turn_id, "turn-1");
        assert_eq!(panel.questions.len(), 1);
        assert!(panel.questions[0].options[0].recommended);
    }

    fn tool_record(name: &str, category: &str) -> ToolRecord {
        ToolRecord {
            name: name.into(),
            description: format!("{name} description"),
            category: category.into(),
        }
    }

    #[test]
    fn agent_groups_follow_desktop_categories_and_selection() {
        let catalog = vec![
            tool_record("read_file", "内置工具"),
            tool_record("run_shell", "内置工具"),
            tool_record("summarize", "技能"),
            tool_record("weather", "MCP 工具"),
        ];
        let groups = agent_groups(&catalog, &["read_file".to_string()], "");
        let titles = groups.iter().map(|g| g.title.as_str()).collect::<Vec<_>>();
        assert_eq!(titles, ["内置工具", "MCP 工具", "技能工具"]);
        assert_eq!(groups[0].tools.len(), 2);
        assert!(groups[0].tools[0].enabled);
        assert!(!groups[0].tools[1].enabled);
        assert!(!groups[0].all_selected);
        // Builtins resolve per tool like the desktop settings page, not one
        // category glyph for the whole group.
        assert_eq!(groups[0].tools[0].icon.as_str(), "file-lines");
        assert_eq!(groups[0].tools[1].icon.as_str(), "terminal");

        let filtered = agent_groups(&catalog, &[], "WEATHER");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].tools[0].name.as_str(), "weather");
        assert!(!filtered[0].all_selected);
    }

    #[test]
    fn group_titles_map_back_to_tool_categories() {
        assert_eq!(category_for_group("技能工具"), "技能");
        assert_eq!(category_for_group("知识库工具"), "知识库");
        assert_eq!(category_for_group("自定义工具"), "用户工具");
        assert_eq!(category_for_group("内置工具"), "内置工具");
    }

    #[test]
    fn agent_text_fields_are_trimmed_deduped_and_bounded() {
        let values = vec![
            " a ".to_string(),
            String::new(),
            "b".to_string(),
            "a".to_string(),
            "c".to_string(),
        ];
        assert_eq!(dedup_text(values, 2), ["a", "b"]);
        assert_eq!(join_tool_path("", "x.md"), "x.md");
        assert_eq!(join_tool_path("notes", "x.md"), "notes/x.md");
    }

    fn cron_record(kind: &str) -> wunder_desktop::CronRecord {
        wunder_desktop::CronRecord {
            id: "job-1".into(),
            name: "测试任务".into(),
            enabled: true,
            schedule: String::new(),
            next_run: String::new(),
            last_status: "未运行".into(),
            last_error: String::new(),
            workspace_id: "ws".into(),
            workspace_name: "默认工作区".into(),
            message: "hello".into(),
            delete_after_run: false,
            schedule_kind: kind.into(),
            schedule_at: "2026-11-07T23:14:00+08:00".into(),
            every_s: 3600,
            cron_expr: "0 9 * * 1-5".into(),
            timezone: "Asia/Shanghai".into(),
        }
    }

    #[test]
    fn cron_schedule_text_is_human_readable() {
        assert_eq!(cron_schedule_text(&cron_record("every")), "每 1 小时");
        assert!(cron_schedule_text(&cron_record("cron")).contains("Cron 0 9 * * 1-5"));
        assert!(cron_schedule_text(&cron_record("at")).contains("定时一次"));
    }

    #[test]
    fn cron_timestamps_are_rebased_to_local_time() {
        let raw = "2026-11-07T15:25:09.541+00:00";
        let expected = chrono::DateTime::parse_from_rfc3339(raw)
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
        assert_eq!(cron_run_time(raw), expected);
        assert_eq!(cron_run_time(""), "");

        let next = "2026-11-07 15:14";
        let expected_next = chrono::NaiveDateTime::parse_from_str(next, "%Y-%m-%d %H:%M")
            .unwrap()
            .and_utc()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
        assert_eq!(cron_next_run_text(next), expected_next);
        assert_eq!(cron_next_run_text("未计划"), "");
    }

    #[test]
    fn managed_workspace_folders_are_sanitized_and_deduplicated() {
        let base = std::env::temp_dir().join(format!(
            "wunder-workspace-slug-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert_eq!(managed_workspace_slug("我的/项目", &base), "我的-项目");
        assert_eq!(managed_workspace_slug(" .. ", &base), "workspace");
        std::fs::create_dir_all(base.join("项目")).unwrap();
        assert_eq!(managed_workspace_slug("项目", &base), "项目-2");
        std::fs::create_dir_all(base.join("项目-2")).unwrap();
        assert_eq!(managed_workspace_slug("项目", &base), "项目-3");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn workspace_cards_follow_the_record_and_selection() {
        let record = wunder_desktop::NativeWorkspace {
            workspace_id: "ws_one".into(),
            name: "文档整理".into(),
            root_path: "/tmp/workspaces/one".into(),
            icon: "flask".into(),
            color: "teal".into(),
            sort_index: 0,
            created_at: 0.0,
            updated_at: 0.0,
            thread_count: 3,
        };
        let card = mobile_workspace(&record, true);
        assert_eq!(card.id.as_str(), "ws_one");
        assert_eq!(card.name.as_str(), "文档整理");
        assert_eq!(card.icon.as_str(), "flask");
        assert_eq!(card.color.as_str(), "teal");
        assert_eq!(card.thread_count, 3);
        assert!(card.active);
        assert!(!mobile_workspace(&record, false).active);
    }

    // Runtime startup projects process-wide state; keep this fixture
    // serialized and isolated under its own WUNDER_HOME.
    #[test]
    fn a_created_workspace_owns_its_folder_and_new_threads() {
        static FIXTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = FIXTURE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!(
            "wunder-android-workspace-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let config = root.join("runtime/config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("wunder.yaml"), "{}\n").unwrap();
        let settings = serde_json::json!({
            "workspace_root": "", "desktop_token": "", "updated_at": 0,
            "lan_mesh": {"enabled": false},
            "llm": {"default": "test-model", "models": {
                "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                               "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                               "support_vision": false}}}
        });
        std::fs::write(
            config.join("desktop.settings.json"),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        std::env::set_var("WUNDER_HOME", root.join("runtime"));
        let mut args = wunder_desktop::args::DesktopArgs::native_defaults();
        args.temp_root = Some(root.join("runtime").canonicalize().unwrap());
        args.workspace = Some(root.join("workspace"));
        let desktop = NativeDesktop::start_with_args(args).expect("isolated runtime");
        let shared = Arc::new(Mutex::new(Shared::default()));

        let workspace = create_managed_workspace(&desktop, "测试 工作区", "flask", "teal", "")
            .expect("create the managed workspace");
        assert!(std::path::Path::new(&workspace.root_path).is_dir());
        assert!(workspace
            .root_path
            .replace('\\', "/")
            .contains("/workspaces/"));
        assert_eq!(workspace.name, "测试 工作区");
        assert_eq!(workspace.icon, "flask");

        // A picked host folder is bound as-is instead of creating a managed
        // folder; this is the path the editor's "选择" button supplies.
        let picked = root.join("picked-folder");
        std::fs::create_dir_all(&picked).unwrap();
        let external = create_managed_workspace(
            &desktop,
            "手机目录",
            "folder",
            "blue",
            picked.to_str().unwrap(),
        )
        .expect("create the picked folder workspace");
        assert!(std::path::Path::new(&external.root_path).is_dir());
        assert!(external.root_path.ends_with("picked-folder"));
        assert!(!external
            .root_path
            .replace('\\', "/")
            .contains("/workspaces/"));

        select_workspace(&desktop, &shared, &workspace.workspace_id).expect("select");
        assert_eq!(
            shared.lock().unwrap().selected_workspace,
            workspace.workspace_id
        );
        refresh_workspaces(&desktop, &shared).expect("refresh");
        assert!(shared
            .lock()
            .unwrap()
            .workspaces
            .iter()
            .find(|card| card.id == workspace.workspace_id)
            .is_some_and(|card| card.active));

        let session = desktop
            .create_session(Some(&workspace.workspace_id))
            .expect("session in the new workspace");
        assert_eq!(session.workspace_name, "测试 工作区");
        assert!(desktop
            .list_sessions(Some(&workspace.workspace_id))
            .unwrap()
            .iter()
            .any(|candidate| candidate.id == session.id));

        // The drawer mirrors the desktop sidebar: a selected workspace hides
        // threads that live in another one.
        let other = desktop
            .list_workspaces()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.workspace_id != workspace.workspace_id)
            .expect("the seeded default workspace");
        let other_session = desktop
            .create_session(Some(&other.workspace_id))
            .expect("session in the default workspace");
        shared.lock().unwrap().selected_workspace = workspace.workspace_id.clone();
        refresh_sessions(&desktop, &shared, Some(&session.id));
        let visible = shared
            .lock()
            .unwrap()
            .sessions
            .iter()
            .map(|candidate| candidate.id.to_string())
            .collect::<Vec<_>>();
        assert!(visible.contains(&session.id));
        assert!(!visible.contains(&other_session.id));

        // A Markdown image referenced by its absolute workspace path decodes
        // into the shared slot the transcript hydrates from.
        let image_path = std::path::Path::new(&workspace.root_path).join("chart.png");
        std::fs::write(
            &image_path,
            include_bytes!("../../wunder-desktop/frontend/assets/markdown-example.png"),
        )
        .unwrap();
        let source = image_path.to_string_lossy().to_string();
        let roots = markdown_image_roots(&desktop, &shared);
        let image = read_markdown_image(&desktop, &roots, &source).expect("the image decodes");
        assert_eq!((image.width, image.height), (640, 280));
        assert_eq!(image.frames.len(), 1);
        assert!(image.delays.is_empty());
        assert_eq!(image.frames[0].len(), 640 * 280 * 4);

        // A GIF in the same workspace keeps every frame and its delay.
        use image::{Delay, Frame, Rgba, RgbaImage};
        let gif_path = std::path::Path::new(&workspace.root_path).join("anim.gif");
        {
            let mut encoded = Vec::new();
            {
                let mut encoder = image::codecs::gif::GifEncoder::new(&mut encoded);
                for red in [255u8, 128, 0] {
                    encoder
                        .encode_frame(Frame::from_parts(
                            RgbaImage::from_pixel(8, 6, Rgba([red, 0, 0, 255])),
                            0,
                            0,
                            Delay::from_numer_denom_ms(120, 1),
                        ))
                        .unwrap();
                }
            }
            std::fs::write(&gif_path, encoded).unwrap();
        }
        let gif_source = gif_path.to_string_lossy().to_string();
        let gif = read_markdown_image(&desktop, &roots, &gif_source).expect("the GIF decodes");
        assert_eq!(gif.frames.len(), 3);
        assert_eq!(gif.delays, vec![120, 120, 120]);
        assert_eq!((gif.width, gif.height), (8, 6));

        drop(desktop);
        let _ = std::fs::remove_dir_all(&root);
    }
}
