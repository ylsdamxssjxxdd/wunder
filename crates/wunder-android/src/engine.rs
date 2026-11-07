//! Engine bridge: owns the in-process `NativeDesktop` and turns its durable
//! thread updates plus the live send stream into the flat models the phone UI
//! renders. All engine calls and markdown projection stay off the UI thread;
//! the UI only copies the latest projection on its 33 ms tick.

use crate::project::{
    self, LiveTurn, RowData, STATUS_FAILED, STATUS_RUNNING, STATUS_STOPPED,
};
use crate::{
    MobileSession, ModelCard, TrajectoryAudit, TrajectoryItem, TrajectorySegment, WorkspaceFile,
};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use wunder_desktop::{
    ModelEdit, NativeChatEvent, NativeChatInput, NativeChatTurn, NativeDesktop, NativeThreadUpdate,
    NativeThreadWatchCancel, NativeWorkflowEntry, CURRENT_VERSION,
};

pub enum Command {
    Select(String),
    NewThread,
    Send(String),
    Stop,
    SelectModel(String),
    RefreshSessions,
    LoadSettings,
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
    LoadWorkspace {
        path: String,
    },
    LoadMoreWorkspace {
        path: String,
        offset: u64,
    },
    RefreshWorkspace,
    UploadWorkspace {
        dir: String,
        files: Vec<String>,
    },
    DownloadWorkspace {
        path: String,
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
    SetDefaultModel(String),
    RefreshUbuntu,
    InstallUbuntu,
    SetUbuntuEnabled(bool),
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
}

/// Persisted preferences mirrored into the window on every change.
#[derive(Clone)]
pub struct SettingsView {
    pub theme: String,
    pub font_size: i32,
    pub send_key: String,
    pub language: String,
    pub workspace_root: String,
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
            version: CURRENT_VERSION.into(),
        }
    }
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
    pub durable_rows: Vec<RowData>,
    /// Retained durable turns behind `durable_rows`. The watcher mixes compact
    /// header updates with detailed turn commits, so rows cannot be projected
    /// from a single update without losing either rounds or earlier turns.
    pub durable_turns: Vec<NativeChatTurn>,
    pub live_rows: Vec<RowData>,
    /// Root id of the turn the live stream is building. Once the durable
    /// snapshot carries the same turn, the durable copy is authoritative and
    /// the live rows are dropped so the transcript never shows a turn twice.
    pub live_root: String,
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
    pub trajectory_filter: i32,
    pub workspace_rev: u64,
    pub workspace_path: String,
    pub workspace_parent_path: String,
    pub workspace_entries: Vec<WorkspaceFile>,
    pub workspace_total: u64,
    pub workspace_loading: bool,
    pub workspace_error: String,
    pub voice_rev: u64,
    /// 0 idle, 1 recording, 2 transcribing.
    pub voice_phase: i32,
    /// Text produced off the UI thread (voice transcription) and appended to
    /// the draft by the UI tick, so a late result never overwrites typing.
    pub pending_draft: Option<String>,
    pub models: Vec<ModelCard>,
    pub inquiry: Option<InquiryView>,
    pub inquiry_rev: u64,
    /// Live command sessions in the active thread and the child agents a
    /// finished turn still owns; both feed the composer's status pills.
    pub running_task_count: i32,
    pub subagent_count: i32,
    /// Consumption keys of panels the user already answered in this process.
    /// A late durable replay of the waiting turn must not reopen them.
    pub consumed_inquiries: std::collections::VecDeque<String>,
    pub settings: SettingsView,
    pub ubuntu: UbuntuView,
    pub ubuntu_rev: u64,
    pub editor: Option<ModelEditor>,
    pub probe_message: String,
    pub probed_context: String,
    pub session_id: String,
    pub title: String,
    pub subtitle: String,
    pub workspace: String,
    pub connection: String,
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
    thread::Builder::new()
        .name("wunder-android-engine".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            if let Err(error) = run(shared.clone(), receiver) {
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

fn run(shared: Arc<Mutex<Shared>>, commands: mpsc::Receiver<Command>) -> anyhow::Result<()> {
    wunder_desktop::install_local_home_env();
    // The phone-control host must exist before the first turn freezes its tool
    // surface; installing it after the engine starts would hide the tool from
    // every thread created earlier in this process.
    #[cfg(target_os = "android")]
    crate::phone::install();
    // Same rule for the browser: swap the builtin Playwright provider for the
    // phone's system WebView before the first thread captures the tool list.
    #[cfg(target_os = "android")]
    crate::webview::install();
    let desktop = Arc::new(NativeDesktop::start()?);
    // Detect a previously installed Ubuntu rootfs before the first settings
    // card render. The environment itself is optional; a missing one must not
    // hold up the engine.
    #[cfg(target_os = "android")]
    crate::ubuntu::bind();
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
        state.connection = "本地引擎".into();
        state.permission_mode = "full_auto".into();
        state.loading = true;
        state.touch();
    }

    refresh_sessions(&desktop, &shared, None);
    refresh_archived(&desktop, &shared);
    refresh_preset_questions(&desktop, &shared);
    refresh_ubuntu(&desktop, &shared);
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
            let created = desktop.create_session(None)?;
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
            Command::NewThread => match desktop.create_session(None) {
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
                        notify(&shared, format!("无法打开新会话：{error}"));
                    }
                }
                Err(error) => notify(&shared, format!("无法新建会话：{error}")),
            },
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
                        // back to the newest remaining thread or a fresh one.
                        let next = desktop
                            .list_sessions(None)
                            .ok()
                            .and_then(|list| list.first().map(|session| session.id.clone()));
                        let next = match next {
                            Some(id) => Some(id),
                            None => desktop.create_session(None).ok().map(|session| session.id),
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
            Command::LoadWorkspace { path } => load_workspace(&desktop, &shared, &path),
            Command::LoadMoreWorkspace { path, offset } => {
                load_workspace_page(&desktop, &shared, &path, offset, true);
            }
            Command::RefreshWorkspace => {
                let path = shared.lock().unwrap().workspace_path.clone();
                load_workspace(&desktop, &shared, &path);
            }
            Command::UploadWorkspace { dir, files } => {
                upload_workspace(&desktop, &shared, &dir, &files);
            }
            Command::DownloadWorkspace { path } => download_workspace(&desktop, &shared, &path),
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
                let session = shared.lock().unwrap().session_id.clone();
                if session.is_empty() {
                    notify(&shared, "会话尚未就绪");
                    continue;
                }
                // A free-form reply while the panel waits is still an answer to
                // the waiting turn; settle it with the same inquiry identity.
                let inquiry_turn_id = take_inquiry_answer(&shared, &session, false)
                    .and_then(|(_text, turn_id)| turn_id);
                start_stream(desktop.clone(), shared.clone(), session, text, inquiry_turn_id);
            }
            Command::Stop => {
                let session = shared.lock().unwrap().session_id.clone();
                if let Err(error) = desktop.cancel_chat(&session) {
                    notify(&shared, format!("停止失败：{error}"));
                }
                let mut state = shared.lock().unwrap();
                state.stopping = true;
                state.touch();
            }
            Command::SelectModel(key) => {
                let session = shared.lock().unwrap().session_id.clone();
                match desktop.set_session_model(&session, &key) {
                    Ok(()) => {
                        let mut state = shared.lock().unwrap();
                        state.model_key = key.clone();
                        state.model_name = key;
                        state.touch();
                    }
                    Err(error) => notify(&shared, format!("切换模型失败：{error}")),
                }
            }
            Command::RefreshSessions | Command::LoadSettings => {
                let active = shared.lock().unwrap().session_id.clone();
                refresh_sessions(&desktop, &shared, Some(&active));
                refresh_ubuntu(&desktop, &shared);
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
                        {
                            let mut state = shared.lock().unwrap();
                            state.editor = None;
                            state.editor_rev = state.editor_rev.wrapping_add(1);
                            state.notify("模型已保存");
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
            Command::SetDefaultModel(key) => match desktop.set_default_model(&key) {
                Ok(_) => {
                    if let Ok(mut state) = shared.lock() {
                        if let Some(editor) = state.editor.as_mut() {
                            editor.is_default = editor.original_key == key;
                            state.editor_rev = state.editor_rev.wrapping_add(1);
                        }
                        state.notify("已设为默认模型");
                        state.touch();
                    }
                    let active = shared.lock().unwrap().session_id.clone();
                    refresh_sessions(&desktop, &shared, Some(&active));
                }
                Err(error) => notify(&shared, format!("设置默认模型失败：{error}")),
            },
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
/// the foreground service busy meanwhile; the engine only publishes progress.
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
                crate::android_host::set_busy(true, "正在准备 Ubuntu 运行环境");
                let _busy = BusyGuard;
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
        state.subtitle = [
            snapshot.session.runtime_status.as_str(),
            snapshot.session.workspace_name.as_str(),
        ]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" · ");
        state.model_key = snapshot
            .session
            .session_model_name
            .clone()
            .unwrap_or_default();
        state.model_name = state.model_key.clone();
        state.durable_rows = project_turns(&initial_turns);
        state.durable_turns = initial_turns.clone();
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
        state.touch();
    }
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
        merge_turns(&mut state.durable_turns, turns);
        state.subagent_count = running_subagents(&state.durable_turns);
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
        state.rows_rev = state.rows_rev.wrapping_add(1);
        state.stream_revision = state.stream_revision.wrapping_add(1);
        state.touch();
        need_detail
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
        state.rows_rev = state.rows_rev.wrapping_add(1);
        state.stream_revision = state.stream_revision.wrapping_add(1);
        state.touch();
    }
}

fn refresh_sessions(desktop: &NativeDesktop, shared: &Arc<Mutex<Shared>>, active: Option<&str>) {
    let Ok(list) = desktop.list_sessions(None) else {
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
            .map(|model| (model.key.clone(), model.model.clone()))
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

/// Read one directory of the workspace. The listing itself is bounded (200
/// entries per page) and cheap enough for the engine thread; uploads use the
/// same path so both entries land in the directory the user is looking at.
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
    let root = match desktop.get_desktop_settings() {
        Ok(settings) => settings.workspace_root,
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
    let root = match desktop.get_desktop_settings() {
        Ok(settings) => settings.workspace_root,
        Err(error) => {
            notify(shared, format!("无法读取工作区设置：{error}"));
            return;
        }
    };
    let mut existing: std::collections::HashSet<String> = desktop
        .workspace_local_listing(&root, dir, 0)
        .map(|listing| listing.entries.into_iter().map(|entry| entry.name).collect())
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
        notify(
            shared,
            "请先在系统设置中开启 wunder 无障碍服务，再使用截图",
        );
        return;
    }
    let Some(source) = status.strip_prefix("ok|") else {
        notify(
            shared,
            format!("截图失败：{}", status.strip_prefix("error|").unwrap_or(status)),
        );
        return;
    };
    let saved = (|| -> Result<String, String> {
        let settings = desktop
            .get_desktop_settings()
            .map_err(|error| error.to_string())?;
        let directory = std::path::Path::new(&settings.workspace_root).join("screenshots");
        std::fs::create_dir_all(&directory).map_err(|error| format!("无法创建截图目录：{error}"))?;
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
    let root = match desktop.get_desktop_settings() {
        Ok(settings) => settings.workspace_root,
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
        | "toml" | "yaml" | "yml" | "csv" | "xml" | "html" | "css" | "js" | "ts" | "rs"
        | "py" | "java" | "kt" | "c" | "h" | "cpp" | "hpp" | "sh" | "sql" => 4,
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
        }
        Err(error) => {
            state.trajectory_error = format!("无法读取线程轨迹：{error:#}");
        }
    }
    state.trajectory_rev = state.trajectory_rev.wrapping_add(1);
    state.touch();
}

pub(crate) struct TrajectoryProjection {
    pub(crate) items: Vec<TrajectoryItem>,
    pub(crate) audit: TrajectoryAudit,
    pub(crate) segments: Vec<TrajectorySegment>,
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
    let mut spans: Vec<(f64, f64, i32, bool)> = Vec::with_capacity(rows.len());
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
        items.push(TrajectoryItem {
            group: group.into(),
            kind,
            title: title.into(),
            summary: summary.into(),
            detail: detail.into(),
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
            spans.push((started, duration.max(0.0), kind, failed));
        }
    }

    if spans.is_empty() || !span_start.is_finite() || !span_end.is_finite() {
        return TrajectoryProjection {
            items,
            audit: TrajectoryAudit::default(),
            segments: Vec::new(),
        };
    }
    let span = (span_end - span_start).max(0.0);
    let domain = if span > 0.0 { span } else { 1.0 };
    let segments = spans
        .into_iter()
        .map(|(start, duration, kind, failed)| TrajectorySegment {
            offset: ((start - span_start) / domain).clamp(0.0, 1.0) as f32,
            width: (duration / domain).clamp(0.0, 1.0) as f32,
            kind,
            failed,
        })
        .collect();
    let mut summary = Vec::new();
    let span_text = format_duration(span);
    if !span_text.is_empty() {
        summary.push(span_text);
    }
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
            summary: summary.join(" · ").into(),
            failed: failed_count,
        },
        segments,
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

/// Releases the foreground notification and wake lock on every exit path of a
/// stream thread, including an unwind.
#[cfg(target_os = "android")]
struct BusyGuard;

#[cfg(target_os = "android")]
impl Drop for BusyGuard {
    fn drop(&mut self) {
        crate::android_host::set_busy(false, "");
    }
}

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
    let stream = match desktop.send_chat(input) {
        Ok(stream) => stream,
        Err(error) => {
            notify(&shared, format!("发送失败：{error}"));
            return;
        }
    };
    let mut live = LiveTurn::new(text);
    {
        let mut state = shared.lock().unwrap();
        state.generating = true;
        state.stopping = false;
        state.live_root = live.root_id.clone();
        state.live_rows = live.rows();
        state.live_rev = state.live_rev.wrapping_add(1);
        state.stream_revision = state.stream_revision.wrapping_add(1);
        state.touch();
    }
    #[cfg(target_os = "android")]
    let busy_title = shared
        .lock()
        .map(|state| state.title.clone())
        .unwrap_or_default();
    thread::Builder::new()
        .name("wunder-android-stream".into())
        .spawn(move || {
            // Both statements live at function scope: the guard must be
            // dropped when the stream ends, not at the end of this block.
            #[cfg(target_os = "android")]
            crate::android_host::set_busy(true, &busy_title);
            #[cfg(target_os = "android")]
            let _busy = BusyGuard;
            let mut stream = stream;
            loop {
                match stream.blocking_recv() {
                    Some(NativeChatEvent::Event(value)) => {
                        apply_event(&mut live, &value);
                        apply_inquiry_event(&shared, &value, &session);
                        publish_live(&shared, &live);
                    }
                    Some(NativeChatEvent::Queued) => {}
                    Some(NativeChatEvent::Finished) => {
                        log::info!("chat stream finished session={session}");
                        // A stream that produced neither text nor an error is
                        // an empty turn, not a successful reply. Surface it so
                        // the transcript cannot end in silence.
                        if live.answer.trim().is_empty()
                            && live.failure.is_none()
                            && live.tools.is_empty()
                        {
                            live.failure = Some("模型未返回内容，请重试".into());
                            publish_live(&shared, &live);
                        }
                        break;
                    }
                    Some(NativeChatEvent::Failed(message)) => {
                        log::warn!("chat stream failed session={session}: {message}");
                        notify(&shared, message.clone());
                        live.failure = Some(message);
                        publish_live(&shared, &live);
                        break;
                    }
                    None => {
                        // The producer disappeared without a terminal event.
                        // Never let that look like a completed turn.
                        log::warn!("chat stream disconnected session={session}");
                        let message = "对话已中断，请重试".to_string();
                        notify(&shared, message.clone());
                        live.failure = Some(message);
                        publish_live(&shared, &live);
                        break;
                    }
                }
            }
            // The compact post-finish snapshot carries no rounds, so it must
            // not overwrite the durable projection: load the detailed turns so
            // the settled turn keeps its work-process rows.
            let title = desktop
                .open_chat_snapshot(&session)
                .ok()
                .map(|snapshot| snapshot.session.title)
                .filter(|title| !title.trim().is_empty());
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
                let covered = live_covered(&state.durable_rows, &live);
                if covered || state.live_rows.is_empty() {
                    state.live_rows.clear();
                    state.live_root.clear();
                }
                state.live_rev = state.live_rev.wrapping_add(1);
                state.generating = false;
                state.stopping = false;
                state.stream_revision = state.stream_revision.wrapping_add(1);
                state.touch();
            }
        })
        .ok();
}

fn publish_live(shared: &Arc<Mutex<Shared>>, live: &LiveTurn) {
    let mut rows = live.rows();
    if let Ok(mut state) = shared.lock() {
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
        state.live_root = live.root_id.clone();
        state.live_rows = rows;
        state.live_rev = state.live_rev.wrapping_add(1);
        state.stream_revision = state.stream_revision.wrapping_add(1);
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
fn live_covered(rows: &[RowData], live: &LiveTurn) -> bool {
    if durable_has_body(rows, &live.root_id) {
        return true;
    }
    if !live.answer.trim().is_empty() {
        return rows
            .iter()
            .rev()
            .find(|row| row.kind == project::KIND_BODY)
            .is_some_and(|row| row.text.trim() == live.answer.trim());
    }
    if let Some(message) = &live.failure {
        return rows
            .iter()
            .rev()
            .find(|row| row.kind == project::KIND_ERROR)
            .is_some_and(|row| row.text.trim() == message.trim());
    }
    false
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
        data.get("source_event").and_then(Value::as_str).unwrap_or(raw)
    } else {
        raw
    };

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
            live.status = STATUS_RUNNING;
            live.phase = "requesting".into();
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
                    live.phase = "waiting".into();
                }
                "cancelled" | "canceled" | "interrupted" | "stopped" => {
                    live.status = STATUS_STOPPED;
                    live.phase = "stopped".into();
                }
                "failed" | "error" | "rejected" => {
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
                live.reasoning = text.to_owned();
                live.phase = "thinking".into();
            }
        }
        "reasoning_delta" => {
            if let Some(delta) = data
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| data.get("reasoning_delta").and_then(Value::as_str))
            {
                live.reasoning.push_str(delta);
                live.phase = "thinking".into();
            }
        }
        "llm_request" => {
            live.phase = "requesting".into();
        }
        "llm_output_delta" | "delta" => {
            if let Some(reasoning) = data
                .get("reasoning_delta")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                live.reasoning.push_str(reasoning);
                live.phase = "thinking".into();
            }
            if let Some(delta) = data
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| data.get("content").and_then(Value::as_str))
            {
                if !delta.is_empty() {
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
                live.reasoning.push_str(reasoning);
            }
            if let Some(answer) = data
                .get("answer")
                .and_then(Value::as_str)
                .or_else(|| data.get("content").and_then(Value::as_str))
                .filter(|answer| !answer.is_empty())
            {
                live.answer = answer.to_owned();
            }
            if let Some(stats) = event_message_stats(data) {
                live.stats = stats;
            }
            if kind == "final" {
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
            let entry =
                NativeWorkflowEntry::from_payload(id, title, detail, state, data);
            let label = project::tool_label(&entry.title);
            let icon = project::tool_icon_key(&entry.title).to_string();
            let target = project::tool_target(&entry);
            if let Some(existing) = live.tools.iter_mut().find(|tool| tool.id == entry.id) {
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
                live.tools.push(project::LiveTool {
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
            live.phase = if pending { "tool" } else { "thinking" }.into();
        }
        "command_session_start"
        | "command_session_delta"
        | "command_session_status"
        | "command_session_exit"
        | "command_session_summary" => {
            let session = data
                .get("command_session_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !session.is_empty() {
                let index = data.get("command_index").and_then(Value::as_i64).unwrap_or(0);
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
                let terminal = matches!(
                    kind,
                    "command_session_exit" | "command_session_summary"
                );
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
                if let Some(existing) = live.tools.iter_mut().find(|tool| tool.id == id) {
                    existing.state = state.into();
                    existing.summary = summary;
                    existing.detail = detail;
                } else {
                    live.tools.push(project::LiveTool {
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
        "compaction"
        | "compaction_completed"
        | "compaction_start"
        | "compaction_progress" => {
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
            if let Some(existing) = live.tools.iter_mut().find(|tool| tool.id == id) {
                existing.state = state.into();
                existing.summary = summary.to_owned();
            } else {
                live.tools.push(project::LiveTool {
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
            live.phase = "compacting".into();
        }
        "question_panel" => {
            live.question_panel = Some(data.clone());
            live.phase = "waiting".into();
        }
        "question_panel_clear" => {
            live.question_panel = None;
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
                "cancelled" | "canceled" | "stopped" | "interrupted" => {
                    (STATUS_STOPPED, "stopped")
                }
                "failed" | "error" | "rejected" => (STATUS_FAILED, "error"),
                "waiting_input" | "waiting_user_input" | "waiting_approval" => {
                    (STATUS_RUNNING, "waiting")
                }
                _ => (project::STATUS_DONE, "done"),
            };
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
        assert!(projection.audit.summary.contains("消耗 2.1k"));
        assert!(projection.audit.summary.contains("工具 1"));
        assert!(projection.audit.summary.contains("模型 2"));
        assert!(projection.audit.summary.contains("2 轮"));
        assert_eq!(projection.segments.len(), 4);
        assert!((projection.segments[0].offset - 0.0).abs() < f32::EPSILON);
        assert!((projection.segments[3].offset - 20.0 / 23.0).abs() < 0.001);
        assert!((projection.segments[3].width - 3.0 / 23.0).abs() < 0.001);
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
            apply_event(&mut live, &json!({
                "event": "thread_item_delta",
                "data": {"session_id": "s", "data": {
                    "source_event": "llm_output_delta", "delta": delta
                }}
            }));
        }
        assert_eq!(live.answer, "你好");
        assert_eq!(live.phase, "outputting");
    }

    #[test]
    fn tool_call_shows_a_preview_before_its_result_frame() {
        let mut live = LiveTurn::new("hi".into());
        apply_event(&mut live, &json!({
            "event": "thread_item_delta",
            "data": {"session_id": "s", "data": {
                "source_event": "tool_call",
                "tool": "shell",
                "tool_call_id": "call-1",
                "arguments": {"command": "ls -la"}
            }}
        }));
        assert_eq!(live.tools.len(), 1);
        assert_eq!(live.tools[0].title, "运行命令");
        assert_eq!(live.tools[0].tool_icon, "terminal");
        assert_eq!(live.tools[0].summary, "ls -la");
        assert_eq!(live.tools[0].state, "running");
        assert_eq!(live.phase, "tool");
    }

    #[test]
    fn question_panel_event_unwraps_two_envelopes_and_queues_the_panel() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        shared.lock().unwrap().session_id = "s".into();
        apply_inquiry_event(&shared, &json!({
            "event": "question_panel",
            "data": {"session_id": "s", "timestamp": "0", "data": {
                "turn_id": "turn-1",
                "questions": [{
                    "question": "选哪个方案？",
                    "options": [{"label": "A", "recommended": true}]
                }]
            }}
        }), "s");
        let state = shared.lock().unwrap();
        let panel = state.inquiry.as_ref().expect("the panel is queued");
        assert_eq!(panel.turn_id, "turn-1");
        assert_eq!(panel.questions.len(), 1);
        assert!(panel.questions[0].options[0].recommended);
    }
}
