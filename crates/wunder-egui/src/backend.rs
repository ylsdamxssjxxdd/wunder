//! Command/event boundary. Adapters perform all blocking and network work.

use crate::model::{
    AppState, MessageView, ModelOption, ProcessPageView, SessionView, TurnView, WorkflowKind,
    WorkflowView, WorkspaceFileView,
};
#[cfg(not(target_arch = "wasm32"))]
use serde_json::Value;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

const COMMAND_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 256;
/// The settings facade can contain models used by other capabilities. Keep
/// only a small LLM catalogue in the composer projection so an accidental
/// oversized configuration cannot grow the immediate-mode UI indefinitely.
const MODEL_OPTION_LIMIT: usize = 128;
#[cfg(not(target_arch = "wasm32"))]
const MODEL_TEXT_LIMIT: usize = 256;
const PROCESS_ITEM_LIMIT: usize = 300;
const PROCESS_DETAIL_BYTES: usize = 16 * 1024;
const PROCESS_DETAIL_LINES: usize = 120;

#[derive(Clone, Debug)]
pub enum BackendCommand {
    RefreshSessions,
    OpenSession(String),
    /// Read one bounded durable process page only after its disclosure has
    /// been opened.  Cursors are exclusive item indexes supplied by the
    /// preceding page, so the worker never scans an entire long-running turn.
    LoadProcessPage {
        session_id: String,
        turn_id: String,
        before: Option<i64>,
        after: Option<i64>,
    },
    SendMessage {
        session_id: String,
        content: String,
    },
    Cancel {
        session_id: String,
    },
    RenameSession {
        session_id: String,
        title: String,
    },
    CreateSession,
    ArchiveSession(String),
    RefreshWorkspaceFiles {
        session_id: String,
        /// Workspace-relative directory to list; empty means the root.
        path: String,
        /// Monotonic client request id. The reply echoes it so a listing for
        /// a directory the user already left cannot replace the current one.
        request_id: u64,
    },
    CreateWorkspaceDirectory {
        session_id: String,
        path: String,
    },
    CreateWorkspaceFile {
        session_id: String,
        path: String,
    },
    /// Rename one entry inside its own parent directory. The façade validates
    /// both the new name and the boundary; the UI never touches paths.
    RenameWorkspaceEntry {
        session_id: String,
        path: String,
        new_name: String,
    },
    /// Delete one entry, including a directory subtree. The caller is
    /// responsible for the confirmation step; this command always deletes.
    DeleteWorkspaceEntry {
        session_id: String,
        path: String,
    },
    OpenWorkspaceFile {
        session_id: String,
        path: String,
    },
    OpenLocalWorkspace(String),
    PreviewSystemPrompt {
        session_id: String,
        request_id: u64,
    },
    /// Load the LLM-only projection needed by the composer picker.
    LoadModels,
    /// Persist a model pin for one thread. An empty key clears the pin and
    /// returns the thread to its agent/default model chain.
    SetSessionModel {
        session_id: String,
        model_key: String,
    },
    /// Persist the thread-scoped reasoning strength used for later turns.
    SaveSessionReasoningEffort {
        session_id: String,
        value: String,
    },
}

#[derive(Clone, Debug)]
pub enum BackendEvent {
    Ready,
    Sessions(Vec<SessionView>),
    History {
        session_id: String,
        turns: Vec<TurnView>,
        /// `true` only for an authoritative snapshot. Passive observers send
        /// changed roots as patches, never as a replacement for the whole
        /// chat timeline.
        replace: bool,
    },
    Delta {
        session_id: String,
        turn_id: String,
        text: String,
    },
    Workflow {
        session_id: String,
        turn_id: String,
        item: WorkflowView,
    },
    /// Incremental hidden reasoning from the active model round.  It is kept
    /// separate from visible answer deltas so the chat body stays stable while
    /// the work-process row grows.
    ReasoningDelta {
        session_id: String,
        turn_id: String,
        item_id: String,
        text: String,
        /// A final `llm_output.reasoning` snapshot replaces the accumulated
        /// stream tail instead of appending the same prefix once more.
        replace: bool,
    },
    /// Runtime status for a durable user turn. It lets a process tail appear
    /// before the next lightweight history projection reaches the UI.
    TurnStatus {
        session_id: String,
        turn_id: String,
        state: String,
    },
    ProcessPage {
        session_id: String,
        turn_id: String,
        /// Cursor direction used for this page. Keeping it in the event lets
        /// the reducer merge an older page before the retained tail without
        /// guessing from opaque item ids.
        before: Option<i64>,
        after: Option<i64>,
        result: Result<ProcessPageView, String>,
    },
    Queued {
        session_id: String,
    },
    Finished {
        session_id: String,
    },
    SystemPromptPreview {
        session_id: String,
        request_id: u64,
        result: Result<(String, bool), String>,
    },
    ThreadRenamed {
        session_id: String,
        title: String,
        result: Result<(), String>,
    },
    SessionCreated {
        result: Result<SessionView, String>,
    },
    SessionArchived {
        session_id: String,
        result: Result<(), String>,
    },
    WorkspaceFiles {
        /// The request owner is carried back with the response. A directory
        /// listing can finish after the user selected another thread, and it
        /// must never replace that thread's file projection.
        session_id: String,
        path: String,
        /// Echo of `BackendCommand::RefreshWorkspaceFiles::request_id`.
        request_id: u64,
        files: Result<Vec<WorkspaceFileView>, String>,
    },
    WorkspaceAction {
        label: String,
        result: Result<(), String>,
    },
    /// A catalogue read can fail independently from the active chat runtime.
    Models {
        result: Result<Vec<ModelOption>, String>,
    },
    /// Success is the only case where the reducer updates `SessionView`.
    /// Failure deliberately leaves its previous pin untouched so callers can
    /// render optimistically without an incorrect durable state.
    SessionModelSaved {
        session_id: String,
        model_key: String,
        result: Result<(), String>,
    },
    /// The returned value is the façade's normalized persisted strength.
    SessionReasoningEffortSaved {
        session_id: String,
        value: String,
        result: Result<String, String>,
    },
    /// Current runtime occupancy for the active thread. It is intentionally
    /// separate from session lifetime usage totals and may be unknown.
    ContextUsage {
        session_id: String,
        used_tokens: Option<u32>,
        capacity_tokens: Option<u32>,
    },
    Failed {
        session_id: Option<String>,
        message: String,
    },
    Disconnected(String),
}

pub trait BackendPort {
    fn submit(&mut self, command: BackendCommand) -> Result<(), String>;
    fn poll(&mut self, budget: usize) -> Vec<BackendEvent>;
    fn wake(&self) {}
}

/// Fixed-data adapter used by the native visual review mode.
///
/// Keeping this behind the same UI boundary makes `--demo` deterministic: it
/// never starts the local runtime, opens SQLite, or reacts to host state.
pub struct DemoBackend {
    pending: std::collections::VecDeque<BackendEvent>,
}

impl Default for DemoBackend {
    fn default() -> Self {
        Self {
            pending: std::collections::VecDeque::new(),
        }
    }
}

impl BackendPort for DemoBackend {
    fn submit(&mut self, command: BackendCommand) -> Result<(), String> {
        match command {
            BackendCommand::PreviewSystemPrompt {
                session_id,
                request_id,
            } => self.pending.push_back(BackendEvent::SystemPromptPreview {
                session_id,
                request_id,
                result: Ok(("你是一个智能体助手。请根据用户的目标整理信息，清晰说明步骤，并保留可验证的结果。\n\n这是演示用的中性提示词示例。".to_owned(), false)),
            }),
            BackendCommand::LoadProcessPage {
                session_id,
                turn_id,
                before,
                after,
            } => {
                let mut page = ProcessPageView::default();
                if session_id == "fixture-thread" && turn_id == "fixture-turn" {
                    page.items = crate::fixtures::demo_state()
                        .turns
                        .into_iter()
                        .find(|turn| turn.id == turn_id)
                        .map(|turn| turn.workflow)
                        .unwrap_or_default();
                }
                self.pending.push_back(BackendEvent::ProcessPage {
                    session_id,
                    turn_id,
                    before,
                    after,
                    result: Ok(page),
                });
            }
            BackendCommand::RenameSession { session_id, title } => {
                self.pending.push_back(BackendEvent::ThreadRenamed {
                    session_id,
                    title,
                    result: Ok(()),
                });
            }
            BackendCommand::RefreshWorkspaceFiles {
                session_id,
                path,
                request_id,
            } => {
                self.pending.push_back(BackendEvent::WorkspaceFiles {
                    session_id,
                    path,
                    request_id,
                    files: Ok(Vec::new()),
                });
            }
            BackendCommand::CreateSession
            | BackendCommand::ArchiveSession(_)
            | BackendCommand::CreateWorkspaceDirectory { .. }
            | BackendCommand::CreateWorkspaceFile { .. }
            | BackendCommand::RenameWorkspaceEntry { .. }
            | BackendCommand::DeleteWorkspaceEntry { .. }
            | BackendCommand::OpenWorkspaceFile { .. }
            | BackendCommand::OpenLocalWorkspace(_) => {
                return Err("演示模式使用固定数据，不能执行本地任务".to_owned());
            }
            BackendCommand::LoadModels => self.pending.push_back(BackendEvent::Models {
                result: Ok(crate::fixtures::demo_models()),
            }),
            BackendCommand::SetSessionModel {
                session_id,
                model_key,
            } => {
                let result = crate::fixtures::demo_models()
                    .iter()
                    .any(|model| model.key == model_key)
                    .then_some(())
                    .ok_or_else(|| "演示模型不存在或不可用".to_owned());
                self.pending.push_back(BackendEvent::SessionModelSaved {
                    session_id,
                    model_key,
                    result,
                });
            }
            BackendCommand::SaveSessionReasoningEffort { session_id, value } => {
                let result = normalize_reasoning_effort(&value)
                    .ok_or_else(|| "不支持的思考力度".to_owned());
                self.pending
                    .push_back(BackendEvent::SessionReasoningEffortSaved {
                        session_id,
                        value,
                        result,
                    });
            }
            _ => return Err("演示模式使用固定数据，不能执行本地任务".to_owned()),
        }
        Ok(())
    }

    fn poll(&mut self, budget: usize) -> Vec<BackendEvent> {
        if budget == 0 {
            Vec::new()
        } else {
            (0..budget)
                .filter_map(|_| self.pending.pop_front())
                .collect()
        }
    }
}

pub struct ChannelBackend {
    commands: SyncSender<BackendCommand>,
    events: Receiver<BackendEvent>,
}

impl ChannelBackend {
    fn new(commands: SyncSender<BackendCommand>, events: Receiver<BackendEvent>) -> Self {
        Self { commands, events }
    }
}

impl BackendPort for ChannelBackend {
    fn submit(&mut self, command: BackendCommand) -> Result<(), String> {
        self.commands
            .try_send(command)
            .map_err(|error| format!("后台队列已满或已关闭：{error}"))
    }

    fn poll(&mut self, budget: usize) -> Vec<BackendEvent> {
        let mut events = Vec::with_capacity(budget.min(32));
        for _ in 0..budget {
            match self.events.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        events
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct NativeBackend {
    channel: ChannelBackend,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeBackend {
    pub fn start() -> Self {
        let (command_tx, command_rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (event_tx, event_rx) = mpsc::sync_channel(EVENT_CAPACITY);
        std::thread::Builder::new()
            .name("wunder-egui-native".to_string())
            .spawn(move || native_worker(command_rx, event_tx))
            .ok();
        Self {
            channel: ChannelBackend::new(command_tx, event_rx),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl BackendPort for NativeBackend {
    fn submit(&mut self, command: BackendCommand) -> Result<(), String> {
        self.channel.submit(command)
    }

    fn poll(&mut self, budget: usize) -> Vec<BackendEvent> {
        self.channel.poll(budget)
    }

    fn wake(&self) {
        // eframe repaints continuously while the native worker is starting or streaming.
    }
}

#[cfg(target_arch = "wasm32")]
pub struct RemoteBackend {
    channel: ChannelBackend,
}

#[cfg(target_arch = "wasm32")]
impl RemoteBackend {
    pub fn start() -> Self {
        let (command_tx, command_rx) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (event_tx, event_rx) = mpsc::sync_channel(EVENT_CAPACITY);
        let _ = command_rx;
        let _ = event_tx.try_send(BackendEvent::Disconnected(
            "WASM 远程适配器尚未连接舰体；当前显示协议边界预览".to_string(),
        ));
        Self {
            channel: ChannelBackend::new(command_tx, event_rx),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl BackendPort for RemoteBackend {
    fn submit(&mut self, command: BackendCommand) -> Result<(), String> {
        let unavailable = match command {
            BackendCommand::PreviewSystemPrompt { .. } => Some("读取系统提示词"),
            BackendCommand::RenameSession { .. } => Some("重命名线程"),
            BackendCommand::LoadModels => Some("读取模型目录"),
            BackendCommand::SetSessionModel { .. } => Some("切换线程模型"),
            BackendCommand::SaveSessionReasoningEffort { .. } => Some("保存思考力度"),
            BackendCommand::CreateSession => Some("新建线程"),
            BackendCommand::ArchiveSession(_) => Some("归档线程"),
            BackendCommand::RefreshWorkspaceFiles { .. } => Some("刷新工作目录"),
            BackendCommand::CreateWorkspaceDirectory { .. } => Some("新建目录"),
            BackendCommand::CreateWorkspaceFile { .. } => Some("新建文件"),
            BackendCommand::RenameWorkspaceEntry { .. } => Some("重命名工作目录条目"),
            BackendCommand::DeleteWorkspaceEntry { .. } => Some("删除工作目录条目"),
            BackendCommand::OpenWorkspaceFile { .. } => Some("打开工作目录条目"),
            BackendCommand::OpenLocalWorkspace(_) => Some("打开本地工作区"),
            BackendCommand::LoadProcessPage { .. } => Some("读取工作过程"),
            _ => None,
        };
        if let Some(action) = unavailable {
            return Err(format!("远程工作台尚未连接，暂时无法{action}"));
        }
        self.channel.submit(command)
    }

    fn poll(&mut self, budget: usize) -> Vec<BackendEvent> {
        self.channel.poll(budget)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_worker(command_rx: Receiver<BackendCommand>, event_tx: SyncSender<BackendEvent>) {
    use wunder_desktop::{
        NativeChatEvent, NativeChatInput, NativeDesktop, NativeStream, NativeThreadUpdate,
    };

    let desktop = match NativeDesktop::start() {
        Ok(desktop) => desktop,
        Err(error) => {
            let _ = event_tx.send(BackendEvent::Failed {
                session_id: None,
                message: format!("本地运行时启动失败：{error}"),
            });
            return;
        }
    };
    let _ = event_tx.send(BackendEvent::Ready);
    let mut active_session: Option<String> = None;
    let mut active_root = String::new();
    let mut active_execution = String::new();
    let mut stream: Option<NativeStream> = None;
    let mut watch: Option<wunder_desktop::NativeThreadWatch> = None;
    let mut pending_delta: Option<PendingDelta> = None;
    let mut last_delta_flush = Instant::now();

    loop {
        if pending_delta.is_some() && last_delta_flush.elapsed() >= Duration::from_millis(16) {
            if !flush_pending_delta(&event_tx, &mut pending_delta) {
                break;
            }
            last_delta_flush = Instant::now();
        }

        let mut stream_closed = false;
        if let Some(current) = stream.as_mut() {
            // Drain a bounded batch from the runtime on every worker tick.  Text is
            // accumulated below and published once per frame instead of making the
            // immediate-mode UI relayout on every token.
            for _ in 0..64 {
                match current.try_recv() {
                    Ok(Some(NativeChatEvent::Event(value))) => {
                        handle_native_event(
                            &event_tx,
                            active_session.as_deref(),
                            &mut active_root,
                            &mut active_execution,
                            &mut pending_delta,
                            value,
                        );
                    }
                    Ok(Some(NativeChatEvent::Queued)) => {
                        if !flush_pending_delta(&event_tx, &mut pending_delta) {
                            return;
                        }
                        if let Some(session_id) = active_session.clone() {
                            if event_tx.send(BackendEvent::Queued { session_id }).is_err() {
                                return;
                            }
                        }
                    }
                    Ok(Some(NativeChatEvent::Finished)) => {
                        if !flush_pending_delta(&event_tx, &mut pending_delta) {
                            return;
                        }
                        if let Some(session_id) = active_session.clone() {
                            if event_tx
                                .send(BackendEvent::Finished { session_id })
                                .is_err()
                            {
                                return;
                            }
                        }
                        stream_closed = true;
                        break;
                    }
                    Ok(Some(NativeChatEvent::Failed(message))) => {
                        if !flush_pending_delta(&event_tx, &mut pending_delta) {
                            return;
                        }
                        if event_tx
                            .send(BackendEvent::Failed {
                                session_id: active_session.clone(),
                                message,
                            })
                            .is_err()
                        {
                            return;
                        }
                        stream_closed = true;
                        break;
                    }
                    Ok(None) => break,
                    Err(message) => {
                        if !flush_pending_delta(&event_tx, &mut pending_delta) {
                            return;
                        }
                        if event_tx
                            .send(BackendEvent::Failed {
                                session_id: active_session.clone(),
                                message: message.to_string(),
                            })
                            .is_err()
                        {
                            return;
                        }
                        stream_closed = true;
                        break;
                    }
                }
            }
        }
        if stream_closed {
            stream = None;
        }
        if let Some(current) = watch.as_mut() {
            while let Some(update) = current.try_recv() {
                match update {
                    NativeThreadUpdate::Reset(turns) => {
                        if let Some(session_id) = active_session.clone() {
                            if event_tx
                                .send(BackendEvent::History {
                                    session_id,
                                    turns: turns.into_iter().map(turn_view).collect(),
                                    replace: true,
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    NativeThreadUpdate::Turns(turns) => {
                        if let Some(session_id) = active_session.clone() {
                            if event_tx
                                .send(BackendEvent::History {
                                    session_id,
                                    turns: turns.into_iter().map(turn_view).collect(),
                                    replace: false,
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    NativeThreadUpdate::Context(usage) => {
                        if let Some(session_id) = active_session.clone() {
                            if event_tx
                                .send(BackendEvent::ContextUsage {
                                    session_id,
                                    used_tokens: context_token_count(usage.used),
                                    capacity_tokens: context_token_count(usage.capacity),
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    NativeThreadUpdate::Failed(message) => {
                        if event_tx
                            .send(BackendEvent::Failed {
                                session_id: active_session.clone(),
                                message,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                    NativeThreadUpdate::Goal(_) => {}
                }
            }
        }

        match command_rx.recv_timeout(Duration::from_millis(16)) {
            Ok(BackendCommand::RefreshSessions) => match desktop.list_sessions(None) {
                Ok(sessions) => {
                    if event_tx
                        .send(BackendEvent::Sessions(
                            sessions.into_iter().map(session_view).collect(),
                        ))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => {
                    if event_tx
                        .send(BackendEvent::Failed {
                            session_id: None,
                            message: error.to_string(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            },
            Ok(BackendCommand::OpenSession(session_id)) => {
                if !flush_pending_delta(&event_tx, &mut pending_delta) {
                    return;
                }
                if let Err(error) = desktop.open_session(&session_id) {
                    if event_tx
                        .send(BackendEvent::Failed {
                            session_id: Some(session_id),
                            message: error.to_string(),
                        })
                        .is_err()
                    {
                        return;
                    }
                    continue;
                }
                active_session = Some(session_id.clone());
                active_root.clear();
                active_execution.clear();
                stream = None;
                match desktop.open_chat_snapshot(&session_id) {
                    Ok(snapshot) => {
                        watch = Some(desktop.watch_chat_with_initial_cursor(
                            &session_id,
                            Some(snapshot.turns.clone()),
                            Some(snapshot.change_cursor),
                        ));
                        if event_tx
                            .send(BackendEvent::History {
                                session_id,
                                turns: snapshot.turns.into_iter().map(turn_view).collect(),
                                replace: true,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        if event_tx
                            .send(BackendEvent::Failed {
                                session_id: active_session.clone(),
                                message: error.to_string(),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            Ok(BackendCommand::LoadProcessPage {
                session_id,
                turn_id,
                before,
                after,
            }) => {
                let result = desktop
                    .chat_process_page(&session_id, &turn_id, before, after)
                    .map(process_page_view)
                    .map_err(display_error);
                if event_tx
                    .send(BackendEvent::ProcessPage {
                        session_id,
                        turn_id,
                        before,
                        after,
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::SendMessage {
                session_id,
                content,
            }) => {
                if active_session.as_deref() != Some(session_id.as_str()) {
                    if event_tx
                        .send(BackendEvent::Failed {
                            session_id: Some(session_id),
                            message: "请先打开目标线程".to_string(),
                        })
                        .is_err()
                    {
                        return;
                    }
                    continue;
                }
                match desktop.send_chat(NativeChatInput {
                    session_id,
                    content,
                    client_message_id: None,
                    reasoning_effort: None,
                    attachments: Vec::new(),
                }) {
                    Ok(next) => {
                        active_root.clear();
                        active_execution.clear();
                        stream = Some(next);
                    }
                    Err(error) => {
                        if event_tx
                            .send(BackendEvent::Failed {
                                session_id: active_session.clone(),
                                message: error.to_string(),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            Ok(BackendCommand::PreviewSystemPrompt {
                session_id,
                request_id,
            }) => {
                // The façade returns the frozen prompt for an initialized
                // thread. This path never edits settings or thread state.
                let result = desktop
                    .preview_thread_system_prompt(Some(&session_id), None)
                    .map(bounded_system_prompt)
                    .map_err(|error| error.to_string().chars().take(1024).collect());
                if event_tx
                    .send(BackendEvent::SystemPromptPreview {
                        session_id,
                        request_id,
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::RenameSession { session_id, title }) => {
                let result = desktop
                    .rename_session(&session_id, &title)
                    .map_err(|error| error.to_string());
                if event_tx
                    .send(BackendEvent::ThreadRenamed {
                        session_id: session_id.clone(),
                        title,
                        result,
                    })
                    .is_err()
                {
                    return;
                }
                match desktop.list_sessions(None) {
                    Ok(sessions) => {
                        if event_tx
                            .send(BackendEvent::Sessions(
                                sessions.into_iter().map(session_view).collect(),
                            ))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        if event_tx
                            .send(BackendEvent::Failed {
                                session_id: None,
                                message: error.to_string(),
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            Ok(BackendCommand::CreateSession) => {
                let result = desktop
                    .create_session(None)
                    .map(session_view)
                    .map_err(display_error);
                if event_tx
                    .send(BackendEvent::SessionCreated { result })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::ArchiveSession(session_id)) => {
                let result = desktop.archive_session(&session_id).map_err(display_error);
                if event_tx
                    .send(BackendEvent::SessionArchived { session_id, result })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::RefreshWorkspaceFiles {
                session_id,
                path,
                request_id,
            }) => {
                let files = workspace_files_for_session(&desktop, &session_id, &path);
                if event_tx
                    .send(BackendEvent::WorkspaceFiles {
                        session_id,
                        path,
                        request_id,
                        files,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::RenameWorkspaceEntry {
                session_id,
                path,
                new_name,
            }) => {
                let result = workspace_root_for_session(&desktop, &session_id).and_then(|root| {
                    desktop
                        .workspace_local_rename(&root, &path, &new_name)
                        .map_err(display_error)
                });
                if event_tx
                    .send(BackendEvent::WorkspaceAction {
                        label: "已重命名".to_owned(),
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::DeleteWorkspaceEntry { session_id, path }) => {
                let result = workspace_root_for_session(&desktop, &session_id).and_then(|root| {
                    desktop
                        .workspace_local_delete(&root, &path)
                        .map_err(display_error)
                });
                if event_tx
                    .send(BackendEvent::WorkspaceAction {
                        label: "已删除".to_owned(),
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::CreateWorkspaceDirectory { session_id, path }) => {
                let result = workspace_create_for_session(&desktop, &session_id, &path, true);
                if event_tx
                    .send(BackendEvent::WorkspaceAction {
                        label: "目录已创建".to_owned(),
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::CreateWorkspaceFile { session_id, path }) => {
                let result = workspace_create_for_session(&desktop, &session_id, &path, false);
                if event_tx
                    .send(BackendEvent::WorkspaceAction {
                        label: "文件已创建".to_owned(),
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::OpenWorkspaceFile { session_id, path }) => {
                let result = workspace_root_for_session(&desktop, &session_id).and_then(|root| {
                    desktop
                        .open_workspace_resource(&root, &path)
                        .map_err(display_error)
                });
                if event_tx
                    .send(BackendEvent::WorkspaceAction {
                        label: "已打开工作目录条目".to_owned(),
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::OpenLocalWorkspace(session_id)) => {
                let result = workspace_root_for_session(&desktop, &session_id).and_then(|root| {
                    desktop
                        .open_workspace_resource(&root, "")
                        .map_err(display_error)
                });
                if event_tx
                    .send(BackendEvent::WorkspaceAction {
                        label: "已打开本地工作区".to_owned(),
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::LoadModels) => {
                let result = desktop
                    .get_desktop_settings()
                    .map(model_options)
                    .map_err(display_error);
                if event_tx.send(BackendEvent::Models { result }).is_err() {
                    return;
                }
            }
            Ok(BackendCommand::SetSessionModel {
                session_id,
                model_key,
            }) => {
                let result = desktop
                    .set_session_model(&session_id, &model_key)
                    .map_err(display_error);
                if event_tx
                    .send(BackendEvent::SessionModelSaved {
                        session_id,
                        model_key,
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::SaveSessionReasoningEffort { session_id, value }) => {
                let result = desktop
                    .save_session_reasoning_effort(&session_id, &value)
                    .map_err(display_error);
                if event_tx
                    .send(BackendEvent::SessionReasoningEffortSaved {
                        session_id,
                        value,
                        result,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(BackendCommand::Cancel { session_id }) => {
                if !flush_pending_delta(&event_tx, &mut pending_delta) {
                    return;
                }
                if let Some(current) = stream.as_ref() {
                    current.cancel();
                }
                if let Err(error) = desktop.cancel_chat(&session_id) {
                    if event_tx
                        .send(BackendEvent::Failed {
                            session_id: Some(session_id),
                            message: error.to_string(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn handle_native_event(
    event_tx: &SyncSender<BackendEvent>,
    session_id: Option<&str>,
    active_root: &mut String,
    active_execution: &mut String,
    pending_delta: &mut Option<PendingDelta>,
    value: Value,
) {
    let Some(session_id) = session_id else { return };
    let event = value
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let data = value.get("data").unwrap_or(&value);
    if event == "native_turn_identity" {
        if let Some(root) = data.get("root_turn_id").and_then(Value::as_str) {
            *active_root = root.to_string();
            if active_execution.is_empty() {
                *active_execution = root.to_owned();
            }
        }
        return;
    }
    if event == "reasoning_delta" {
        let payload = data.get("data").unwrap_or(data);
        let text = payload
            .get("delta")
            .and_then(Value::as_str)
            .or_else(|| payload.get("content").and_then(Value::as_str))
            .unwrap_or_default();
        if !text.is_empty() {
            emit_reasoning_delta(
                event_tx,
                session_id,
                active_root,
                active_execution,
                payload,
                text,
                false,
                pending_delta,
            );
        }
        return;
    }
    if matches!(event, "llm_output" | "llm_output_delta") {
        let payload = data.get("data").unwrap_or(data);
        // The runtime streams visible answer and hidden reasoning together in
        // `llm_output_delta`.  Project the latter into the work-process tail
        // before coalescing the visible answer, otherwise a reasoning-only
        // chunk is silently discarded by the chat projection.
        let reasoning = payload
            .get(if event == "llm_output" {
                "reasoning"
            } else {
                "reasoning_delta"
            })
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !reasoning.is_empty() {
            emit_reasoning_delta(
                event_tx,
                session_id,
                active_root,
                active_execution,
                payload,
                reasoning,
                event == "llm_output",
                pending_delta,
            );
        }
        let text = payload
            .get("content")
            .and_then(Value::as_str)
            .or_else(|| payload.get("delta").and_then(Value::as_str))
            .unwrap_or_default();
        if !text.is_empty() {
            append_delta(event_tx, pending_delta, session_id, active_root, text);
        }
        return;
    }
    if matches!(
        event,
        "native_execution_started" | "native_execution_status" | "turn_terminal"
    ) {
        let payload = data.get("data").unwrap_or(data);
        if event == "native_execution_started" {
            if let Some(execution) = payload.get("turn_id").and_then(Value::as_str) {
                *active_execution = execution.to_owned();
            }
        }
        let root = payload
            .get("root_turn_id")
            .and_then(Value::as_str)
            .filter(|root| !root.is_empty())
            .unwrap_or(active_root);
        if !root.is_empty() {
            let state = match event {
                "turn_terminal" => payload
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed"),
                _ => payload
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("running"),
            };
            let _ = event_tx.send(BackendEvent::TurnStatus {
                session_id: session_id.to_owned(),
                turn_id: root.to_owned(),
                state: normalize_turn_state(state),
            });
        }
        return;
    }
    if matches!(
        event,
        "tool_call" | "tool_start" | "tool_result" | "tool_output"
    ) {
        let payload = data.get("data").unwrap_or(data);
        let title = payload
            .get("tool")
            .or_else(|| payload.get("tool_name"))
            .and_then(Value::as_str)
            .unwrap_or("工具");
        let pending = matches!(event, "tool_call" | "tool_start");
        let failed = payload["ok"] == false
            || payload["success"] == false
            || payload["result"]["ok"] == false
            || payload["result"]["success"] == false;
        let state = if pending {
            "running"
        } else if failed {
            "failed"
        } else {
            "completed"
        };
        let id = payload
            .get("tool_call_id")
            .or_else(|| payload.get("item_id"))
            .and_then(Value::as_str)
            .unwrap_or(event);
        let detail = value
            .get("display_result")
            .and_then(Value::as_str)
            .or_else(|| payload.get("display_result").and_then(Value::as_str))
            .or_else(|| payload.get("output").and_then(Value::as_str))
            .map(bounded_detail)
            .unwrap_or_default();
        let entry =
            wunder_desktop::NativeWorkflowEntry::from_payload(id, title, detail.0, state, payload);
        let mut item = native_workflow_view(entry);
        item.detail_truncated |= detail.1;
        let targets = invocation_targets(payload);
        if !targets.is_empty() {
            item.targets = targets;
        }
        let _ = flush_pending_delta(event_tx, pending_delta);
        let _ = event_tx.send(BackendEvent::Workflow {
            session_id: session_id.to_string(),
            turn_id: active_root.to_string(),
            item,
        });
    }
}

/// Build the same durable identity used by `NativeProcessItem`: an assistant
/// message stores one model round as `{root}:text-{round}`, and the egui
/// process projection prefixes its reasoning entry with `reason-`.  Using it
/// for live deltas makes a subsequently loaded process page update the active
/// entry instead of rendering a duplicate thought.
#[cfg(not(target_arch = "wasm32"))]
fn reasoning_item_id(root: &str, round: i64) -> String {
    format!("reason-{root}:text-{round}")
}

#[cfg(not(target_arch = "wasm32"))]
fn emit_reasoning_delta(
    event_tx: &SyncSender<BackendEvent>,
    session_id: &str,
    active_root: &str,
    active_execution: &str,
    payload: &Value,
    text: &str,
    replace: bool,
    pending_delta: &mut Option<PendingDelta>,
) {
    let round = payload
        .get("model_round")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let _ = flush_pending_delta(event_tx, pending_delta);
    let _ = event_tx.send(BackendEvent::ReasoningDelta {
        session_id: session_id.to_owned(),
        turn_id: active_root.to_owned(),
        item_id: reasoning_item_id(
            if active_execution.is_empty() {
                active_root
            } else {
                active_execution
            },
            round,
        ),
        text: text.to_owned(),
        replace,
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct PendingDelta {
    session_id: String,
    turn_id: String,
    text: String,
}

/// Adjacent output for the same turn becomes one UI event.  Flushing through
/// `send` deliberately propagates queue pressure back to the worker: output
/// bytes and terminal states must never be discarded merely because a frame is
/// late.
#[cfg(not(target_arch = "wasm32"))]
fn append_delta(
    event_tx: &SyncSender<BackendEvent>,
    pending: &mut Option<PendingDelta>,
    session_id: &str,
    turn_id: &str,
    text: &str,
) {
    let same_turn = pending
        .as_ref()
        .is_some_and(|item| item.session_id == session_id && item.turn_id == turn_id);
    if !same_turn {
        let _ = flush_pending_delta(event_tx, pending);
        *pending = Some(PendingDelta {
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            text: String::new(),
        });
    }
    if let Some(item) = pending {
        item.text.push_str(text);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn flush_pending_delta(
    event_tx: &SyncSender<BackendEvent>,
    pending: &mut Option<PendingDelta>,
) -> bool {
    let Some(delta) = pending.take() else {
        return true;
    };
    event_tx
        .send(BackendEvent::Delta {
            session_id: delta.session_id,
            turn_id: delta.turn_id,
            text: delta.text,
        })
        .is_ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn session_view(session: wunder_desktop::NativeSession) -> SessionView {
    SessionView {
        id: session.id,
        title: session.title,
        workspace: session.workspace_name,
        status: session.runtime_status,
        updated_at: session.updated_at,
        locked: session.locked,
        model_key: session.session_model_name,
        reasoning_effort: session.reasoning_effort,
        // `consumed_tokens` is a lifetime aggregate and must never be shown
        // as live context occupancy. The watcher supplies that separately.
        context_tokens: None,
        context_capacity_tokens: None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn workspace_root_for_session(
    desktop: &wunder_desktop::NativeDesktop,
    session_id: &str,
) -> Result<String, String> {
    let session = desktop
        .get_session_info(session_id)
        .map_err(display_error)?;
    let workspace_id = session
        .workspace_id
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| "当前线程未绑定本地工作区".to_owned())?;
    let workspace = desktop
        .get_workspace(&workspace_id)
        .map_err(display_error)?;
    if workspace.root_path.trim().is_empty() {
        return Err("当前工作区未提供本地路径".to_owned());
    }
    Ok(workspace.root_path)
}

#[cfg(not(target_arch = "wasm32"))]
fn workspace_files_for_session(
    desktop: &wunder_desktop::NativeDesktop,
    session_id: &str,
    path: &str,
) -> Result<Vec<WorkspaceFileView>, String> {
    let root = workspace_root_for_session(desktop, session_id)?;
    let listing = desktop
        .workspace_local_listing(&root, path, 0)
        .map_err(display_error)?;
    if listing.status != "succeeded" {
        return Err(listing
            .error
            .unwrap_or_else(|| "读取工作目录失败".to_owned()));
    }
    Ok(listing
        .entries
        .into_iter()
        .map(|entry| WorkspaceFileView {
            name: entry.name,
            path: entry.path,
            is_directory: entry.kind == "dir",
            size: entry.size,
        })
        .collect())
}

#[cfg(not(target_arch = "wasm32"))]
fn workspace_create_for_session(
    desktop: &wunder_desktop::NativeDesktop,
    session_id: &str,
    path: &str,
    directory: bool,
) -> Result<(), String> {
    let root = workspace_root_for_session(desktop, session_id)?;
    if directory {
        desktop.workspace_local_mkdir(&root, path)
    } else {
        desktop.workspace_local_write(&root, path)
    }
    .map_err(display_error)
}

#[cfg(not(target_arch = "wasm32"))]
fn model_options(settings: wunder_desktop::DesktopSettings) -> Vec<ModelOption> {
    let mut models: Vec<ModelOption> = settings
        .models
        .into_iter()
        .filter(|model| is_llm_model(&model.model_type))
        .map(|model| ModelOption {
            key: bounded_model_text(model.key),
            provider: bounded_model_text(model.provider),
            model: bounded_model_text(model.model),
            is_default: model.is_default,
            max_context_tokens: model
                .max_context
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|v| *v > 0),
            reasoning_effort: bounded_model_text(model.reasoning_effort),
        })
        .filter(|model| !model.key.is_empty())
        .collect();
    // The native settings projection is normally ordered, but pin the UI
    // order deterministically before applying the hard display bound.
    models.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.model.cmp(&right.model))
            .then_with(|| left.key.cmp(&right.key))
    });
    models.truncate(MODEL_OPTION_LIMIT);
    models
}

#[cfg(not(target_arch = "wasm32"))]
fn is_llm_model(model_type: &str) -> bool {
    matches!(model_type.trim().to_ascii_lowercase().as_str(), "" | "llm")
}

#[cfg(not(target_arch = "wasm32"))]
fn bounded_model_text(text: String) -> String {
    let mut end = text.len().min(MODEL_TEXT_LIMIT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(not(target_arch = "wasm32"))]
fn context_token_count(value: Option<f64>) -> Option<u32> {
    value
        .filter(|value| value.is_finite() && *value >= 0.0 && *value <= u32::MAX as f64)
        .map(|value| value as u32)
}

#[cfg(not(target_arch = "wasm32"))]
fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string().chars().take(1024).collect()
}

fn normalize_reasoning_effort(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    matches!(
        value.as_str(),
        "default" | "none" | "minimal" | "low" | "medium" | "high" | "xhigh"
    )
    .then_some(value)
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn normalize_turn_state(state: &str) -> String {
    match state.trim().to_ascii_lowercase().as_str() {
        "failed" | "rejected" | "error" | "执行失败" => "failed",
        "cancelled" | "canceled" | "interrupted" | "stopped" | "已停止" => "stopped",
        "completed" | "complete" | "done" | "任务完成" => "completed",
        "queued" | "正在排队" => "queued",
        "waiting_input" | "waiting_user_input" | "等待用户输入" => "waiting_input",
        "waiting_approval" | "等待确认" => "waiting_approval",
        _ => "running",
    }
    .to_owned()
}

fn append_bounded_detail(item: &mut WorkflowView, delta: &str) {
    if item.detail_truncated || delta.is_empty() {
        return;
    }
    let mut combined = String::with_capacity(item.detail.len().saturating_add(delta.len()));
    combined.push_str(&item.detail);
    combined.push_str(delta);
    let (detail, truncated) =
        bounded_process_text(&combined, PROCESS_DETAIL_BYTES, PROCESS_DETAIL_LINES);
    item.detail = detail;
    item.detail_truncated = truncated;
    item.preview = bounded_preview_text(&item.detail, 640);
}

fn bounded_process_text(text: &str, byte_limit: usize, line_limit: usize) -> (String, bool) {
    let mut end = text.len().min(byte_limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut lines = 0;
    for (index, character) in text[..end].char_indices() {
        if character == '\n' {
            lines += 1;
            if lines >= line_limit {
                end = index;
                break;
            }
        }
    }
    (text[..end].to_owned(), end < text.len())
}

fn bounded_preview_text(text: &str, limit: usize) -> String {
    text.chars()
        .take(limit)
        .map(|character| {
            if character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn turn_view(turn: wunder_desktop::NativeChatTurn) -> TurnView {
    TurnView {
        id: turn.root_id,
        user: MessageView {
            id: turn.user.turn_id,
            text: turn.user.text,
            mine: true,
            state: turn.user.state,
        },
        assistant: MessageView {
            id: turn.assistant.turn_id,
            text: turn.assistant.text,
            mine: false,
            state: normalize_turn_state(&turn.assistant.state),
        },
        process_steps: turn.process_steps.max(0) as usize,
        workflow: turn
            .rounds
            .into_iter()
            .flat_map(|round| {
                let mut rows = Vec::with_capacity(round.items.len().saturating_add(2));
                // Keep live/history/process-page identities interchangeable.
                // NativeProcessItem uses the durable assistant item id
                // (`{root}:text-{round}`), so prefixing that same id avoids a
                // duplicate reasoning row when a disclosure is opened after
                // the lightweight history snapshot has already rendered it.
                let identity = if round.item_id.is_empty() {
                    format!("{}:text-{}", round.execution, round.round)
                } else {
                    round.item_id.clone()
                };
                if !round.reasoning.is_empty() {
                    let (detail, detail_truncated) = bounded_detail(&round.reasoning);
                    rows.push(WorkflowView {
                        id: format!("reason-{identity}"),
                        kind: WorkflowKind::Reasoning,
                        title: "已思考".into(),
                        preview: bounded_preview(&round.reasoning),
                        state: "completed".into(),
                        detail,
                        detail_truncated,
                        ..Default::default()
                    });
                }
                if !round.is_answer && !round.text.is_empty() {
                    let (detail, detail_truncated) = bounded_detail(&round.text);
                    rows.push(WorkflowView {
                        id: format!("print-{identity}"),
                        kind: WorkflowKind::Text,
                        title: "中间输出".into(),
                        preview: bounded_preview(&round.text),
                        state: "completed".into(),
                        detail,
                        detail_truncated,
                        ..Default::default()
                    });
                }
                rows.extend(round.items.into_iter().map(native_workflow_view));
                rows
            })
            .take(100)
            .collect(),
        ..Default::default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn process_page_view(page: wunder_desktop::NativeProcessPage) -> ProcessPageView {
    let mut items = Vec::with_capacity(page.items.len().saturating_mul(2));
    for item in page.items {
        if !item.reasoning.is_empty() {
            let (detail, detail_truncated) = bounded_detail(&item.reasoning);
            items.push(WorkflowView {
                id: format!("reason-{}", item.id),
                storage_index: Some(item.item_index),
                kind: WorkflowKind::Reasoning,
                title: if matches!(item.state.as_str(), "running" | "streaming" | "queued") {
                    "正在思考…"
                } else {
                    "已思考"
                }
                .to_owned(),
                preview: bounded_preview(&item.reasoning),
                state: item.state.clone(),
                detail,
                detail_truncated,
                ..Default::default()
            });
        }
        if !item.text.is_empty() {
            let (detail, detail_truncated) = bounded_detail(&item.text);
            items.push(WorkflowView {
                id: format!("print-{}", item.id),
                storage_index: Some(item.item_index),
                kind: WorkflowKind::Text,
                title: "中间输出".to_owned(),
                preview: bounded_preview(&item.text),
                state: item.state.clone(),
                detail,
                detail_truncated,
                ..Default::default()
            });
        }
        if let Some(tool) = item.tool {
            let mut view = native_workflow_view(tool);
            view.storage_index = Some(item.item_index);
            items.push(view);
        }
    }
    ProcessPageView {
        items,
        first: page.first,
        last: page.last,
        has_older: page.has_older,
        has_newer: page.has_newer,
    }
}

// Display output is bounded before entering the UI inbox. The durable
// result remains in NativeDesktop; this is never the full-result copy source.
#[cfg(not(target_arch = "wasm32"))]
const WORKFLOW_DETAIL_BYTES: usize = 16 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const WORKFLOW_DETAIL_LINES: usize = 120;

#[cfg(not(target_arch = "wasm32"))]
fn bounded_detail(text: &str) -> (String, bool) {
    let mut end = text.len().min(WORKFLOW_DETAIL_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut lines = 0;
    for (index, character) in text[..end].char_indices() {
        if character == '\n' {
            lines += 1;
            if lines >= WORKFLOW_DETAIL_LINES {
                end = index;
                break;
            }
        }
    }
    (text[..end].to_owned(), end < text.len())
}

#[cfg(not(target_arch = "wasm32"))]
fn bounded_preview(text: &str) -> String {
    text.chars()
        .take(640)
        .map(|character| {
            if character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn native_workflow_view(item: wunder_desktop::NativeWorkflowEntry) -> WorkflowView {
    // The items field of NativeChatRound is typed as tool entries. Reasoning
    // and intermediate text come from their separate fields above.
    let (mut detail, mut detail_truncated) = bounded_detail(&item.detail);
    if detail.is_empty() {
        // Sections may contain a structured result even when the flat copy
        // field is absent. Preserve actual lines only, never substitute the
        // invocation preview for an unavailable result.
        for section in item.sections.iter().take(8) {
            for line in section.lines.iter().take(WORKFLOW_DETAIL_LINES) {
                if detail.len() >= WORKFLOW_DETAIL_BYTES {
                    detail_truncated = true;
                    break;
                }
                if !detail.is_empty() {
                    detail.push('\n');
                }
                let (text, clipped) = bounded_detail(&line.text);
                let remaining = WORKFLOW_DETAIL_BYTES.saturating_sub(detail.len());
                let mut end = text.len().min(remaining);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                detail.push_str(&text[..end]);
                detail_truncated |= clipped || end < text.len();
            }
        }
        let (bounded, clipped) = bounded_detail(&detail);
        detail = bounded;
        detail_truncated |= clipped;
    }
    let mut targets: Vec<String> = item
        .patch_files
        .iter()
        .take(4)
        .filter(|file| !file.path.is_empty())
        .map(|file| file.path.chars().take(1024).collect())
        .collect();
    // NativeWorkflowEntry.preview is the bounded call argument projection.
    // For these exact runtime tool ids it is a file target (same contract as
    // Slint's tool_target); arbitrary tools keep the preview as plain text.
    if targets.is_empty()
        && item.preview.chars().count() < 640
        && matches!(
            item.title.as_str(),
            "read_file"
                | "读取文件"
                | "write_file"
                | "写入文件"
                | "list_files"
                | "列出文件"
                | "list_file"
                | "read_image"
                | "view_image"
                | "读图"
        )
        && !item.preview.trim().is_empty()
    {
        targets.push(item.preview.clone());
    }
    WorkflowView {
        id: item.id,
        storage_index: None,
        kind: WorkflowKind::Tool,
        tool_name: item.title.clone(),
        title: item.title,
        preview: bounded_preview(&item.preview),
        state: item.state,
        targets,
        detail,
        detail_truncated,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn invocation_targets(payload: &Value) -> Vec<String> {
    let Some(args) = payload.get("args").or_else(|| payload.get("arguments")) else {
        return Vec::new();
    };
    let parsed = args
        .as_str()
        .filter(|text| text.len() <= 8192)
        .and_then(|text| serde_json::from_str::<Value>(text).ok());
    let args = parsed.as_ref().unwrap_or(args);
    let mut targets = Vec::with_capacity(4);
    for key in [
        "path",
        "file",
        "filename",
        "file_path",
        "filePath",
        "target_path",
        "source_path",
        "to_path",
        "output_path",
        "input_path",
        "dir",
        "directory",
    ] {
        if let Some(path) = args
            .get(key)
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
        {
            let path: String = path.chars().take(1024).collect();
            if !targets.contains(&path) {
                targets.push(path);
            }
            if targets.len() == 4 {
                return targets;
            }
        }
    }
    if let Some(paths) = args
        .get("paths")
        .or_else(|| args.get("files"))
        .and_then(Value::as_array)
    {
        for path in paths.iter().take(4).filter_map(Value::as_str) {
            let path: String = path.chars().take(1024).collect();
            if !path.is_empty() && !targets.contains(&path) {
                targets.push(path);
            }
            if targets.len() == 4 {
                break;
            }
        }
    }
    targets
}

#[cfg(not(target_arch = "wasm32"))]
fn bounded_system_prompt(mut text: String) -> (String, bool) {
    const MAX_BYTES: usize = 128 * 1024;
    let mut end = text.len().min(MAX_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = end < text.len();
    text.truncate(end);
    if truncated {
        text.shrink_to_fit();
    }
    (text, truncated)
}

pub fn apply_event(state: &mut AppState, event: BackendEvent) {
    match event {
        BackendEvent::SystemPromptPreview {
            session_id,
            request_id,
            result,
        } if state.prompt_preview.open
            && state.prompt_preview.request_id == request_id
            && state.selected_session.as_deref() == Some(session_id.as_str())
            && state.prompt_preview.session_id.as_deref() == Some(session_id.as_str()) =>
        {
            state.prompt_preview.loading = false;
            match result {
                Ok((text, truncated)) => {
                    state.prompt_preview.text = text;
                    state.prompt_preview.truncated = truncated;
                    state.prompt_preview.error.clear();
                }
                Err(error) => state.prompt_preview.error = error,
            }
        }
        BackendEvent::WorkspaceFiles {
            session_id,
            path,
            request_id,
            files,
        } if state.selected_session.as_deref() == Some(session_id.as_str())
            && request_id == state.workspace_request_id =>
        {
            state.workspace_file_loading = false;
            state.workspace_file_path = path;
            match files {
                Ok(files) => {
                    state.replace_workspace_files(files);
                    state.workspace_file_error.clear();
                }
                Err(error) => state.workspace_file_error = error,
            }
        }
        BackendEvent::Ready => {
            state.connection = crate::model::ConnectionState::Ready;
            state.status = "本地运行时已就绪".to_string();
        }
        BackendEvent::Sessions(sessions) => {
            if state.selected_session.is_none() {
                state.selected_session = sessions.first().map(|s| s.id.clone());
            }
            state.sessions = sessions;
        }
        BackendEvent::Models { result } => {
            state.model_picker_loading = false;
            match result {
                Ok(models) => {
                    state.models = models.into_iter().take(MODEL_OPTION_LIMIT).collect();
                    state.model_picker_error.clear();
                }
                Err(error) => state.model_picker_error = error,
            }
        }
        BackendEvent::SessionModelSaved {
            session_id,
            model_key,
            result,
        } => {
            state.model_picker_saving = false;
            match result {
                Ok(()) => {
                    if let Some(session) = state
                        .sessions
                        .iter_mut()
                        .find(|session| session.id == session_id)
                    {
                        session.model_key = (!model_key.trim().is_empty()).then_some(model_key);
                    }
                    state.model_picker_error.clear();
                }
                Err(error) => state.model_picker_error = error,
            }
        }
        BackendEvent::SessionReasoningEffortSaved {
            session_id,
            value,
            result,
        } => {
            state.model_picker_saving = false;
            match result {
                Ok(saved) => {
                    if let Some(session) = state
                        .sessions
                        .iter_mut()
                        .find(|session| session.id == session_id)
                    {
                        session.reasoning_effort = saved;
                    }
                    state.model_picker_error.clear();
                }
                Err(error) => {
                    // Keep the requested value in this arm so event payloads
                    // stay useful to an optimistic view, while preserving the
                    // existing persisted value in AppState.
                    let _ = value;
                    state.model_picker_error = error;
                }
            }
        }
        BackendEvent::ContextUsage {
            session_id,
            used_tokens,
            capacity_tokens,
        } => {
            if let Some(session) = state
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            {
                session.context_tokens = used_tokens;
                session.context_capacity_tokens = capacity_tokens;
            }
        }
        BackendEvent::History {
            session_id,
            turns,
            replace,
        } if state.selected_session.as_deref() == Some(session_id.as_str()) => {
            // A watcher patch can race the native stream. It is not a lifecycle
            // acknowledgement, so a committed running root must keep the
            // composer in its stop state. Only a terminal history update for a
            // root we already know is active may settle that state; `Finished`,
            // `Failed`, and `TurnStatus` remain the normal lifecycle owners.
            let settles_active_turn = state.busy
                && turns.iter().any(|next| {
                    is_terminal_turn_state(&next.assistant.state)
                        && state.turns.iter().any(|current| {
                            current.id == next.id && is_active_turn_state(&current.assistant.state)
                        })
                });
            merge_history(&mut state.turns, turns, replace);
            if settles_active_turn
                && !state
                    .turns
                    .iter()
                    .any(|turn| is_active_turn_state(&turn.assistant.state))
            {
                state.busy = false;
            }
        }
        BackendEvent::ProcessPage {
            session_id,
            turn_id,
            before,
            after,
            result,
        } if state.selected_session.as_deref() == Some(session_id.as_str()) => {
            if let Some(turn) = state.turns.iter_mut().find(|turn| turn.id == turn_id) {
                turn.process_loading = false;
                match result {
                    Ok(page) => {
                        // Keep the active streaming tail when a durable page
                        // arrives. Older/newer pages are inserted on the
                        // corresponding side; identical ids are updated in
                        // place so expansion state remains stable in egui.
                        let page_items = page.items;
                        let mut merged = Vec::with_capacity(
                            turn.workflow.len().saturating_add(page_items.len()),
                        );
                        if before.is_some() {
                            merged.extend(page_items);
                            merged.extend(turn.workflow.iter().cloned());
                        } else {
                            merged.extend(turn.workflow.iter().cloned());
                            merged.extend(page_items);
                        }
                        let mut positions = HashMap::with_capacity(merged.len());
                        let mut deduped: Vec<WorkflowView> = Vec::with_capacity(merged.len());
                        for item in merged {
                            if let Some(index) = positions.get(&item.id) {
                                deduped[*index] = item;
                            } else {
                                positions.insert(item.id.clone(), deduped.len());
                                deduped.push(item);
                            }
                        }
                        let over_limit = deduped.len() > PROCESS_ITEM_LIMIT;
                        if over_limit {
                            if before.is_some() {
                                // Older pages are prepended. Keep the oldest
                                // visible window so the newly read page is
                                // useful immediately; the discarded newer
                                // tail remains reachable through `after`.
                                deduped.truncate(PROCESS_ITEM_LIMIT);
                            } else {
                                // Newer pages are appended. Keep the newest
                                // window and expose an older-page affordance
                                // for rows evicted from the front.
                                let start = deduped.len() - PROCESS_ITEM_LIMIT;
                                deduped = deduped.split_off(start);
                            }
                        }
                        // A durable page is indexed by storage item, while a
                        // single item can expand into reasoning, text and a
                        // tool row. Derive cursors from the rows that survive
                        // the presentation cap; retaining a page cursor for
                        // a trimmed-away row would make the next/previous
                        // page action skip an unreachable interval.
                        let (retained_first, retained_last) = workflow_storage_bounds(&deduped);
                        turn.workflow = deduped;
                        if turn.process_steps == 0 {
                            turn.process_steps = turn.workflow.len();
                        }
                        // `page.first`/`page.last` are raw storage cursors and
                        // must remain authoritative for the direction that
                        // was just read. A page can contain only rows that the
                        // presentation projection filters (for example an
                        // empty assistant snapshot), leaving `retained_first`
                        // and `retained_last` anchored to the already visible
                        // rows. Preferring those bounds in that case causes a
                        // repeated request with the same exclusive cursor.
                        // Keep the raw side cursor even when the projected page
                        // contributes no UI rows; use retained bounds only for
                        // the opposite side of a bidirectional window.
                        if before.is_some() {
                            turn.process_first =
                                page.first.or(retained_first).or(turn.process_first);
                            turn.process_last = retained_last.or(page.last).or(turn.process_last);
                        } else if after.is_some() {
                            turn.process_first =
                                retained_first.or(page.first).or(turn.process_first);
                            turn.process_last = page.last.or(retained_last).or(turn.process_last);
                        } else {
                            turn.process_first =
                                retained_first.or(page.first).or(turn.process_first);
                            turn.process_last = retained_last.or(page.last).or(turn.process_last);
                        }
                        turn.process_has_older = page.has_older || (over_limit && after.is_some());
                        turn.process_has_newer = page.has_newer || (over_limit && before.is_some());
                        turn.process_loaded = true;
                        turn.process_error.clear();
                    }
                    Err(error) => turn.process_error = error,
                }
            }
        }
        BackendEvent::ReasoningDelta {
            session_id,
            turn_id,
            item_id,
            text,
            replace,
        } if state.selected_session.as_deref() == Some(session_id.as_str()) => {
            if let Some(turn) = state.turns.iter_mut().find(|turn| turn.id == turn_id) {
                let mut added = false;
                if let Some(item) = turn.workflow.iter_mut().find(|item| item.id == item_id) {
                    if replace {
                        let (detail, detail_truncated) =
                            bounded_process_text(&text, PROCESS_DETAIL_BYTES, PROCESS_DETAIL_LINES);
                        item.detail = detail;
                        item.detail_truncated = detail_truncated;
                        item.preview = bounded_preview_text(&item.detail, 640);
                        item.state = "completed".to_owned();
                    } else {
                        append_bounded_detail(item, &text);
                        item.state = "running".to_owned();
                    }
                } else if turn.workflow.len() < PROCESS_ITEM_LIMIT {
                    let (detail, detail_truncated) =
                        bounded_process_text(&text, PROCESS_DETAIL_BYTES, PROCESS_DETAIL_LINES);
                    turn.workflow.push(WorkflowView {
                        id: item_id,
                        kind: WorkflowKind::Reasoning,
                        title: "正在思考…".to_owned(),
                        preview: bounded_preview_text(&detail, 640),
                        detail,
                        detail_truncated,
                        state: if replace { "completed" } else { "running" }.to_owned(),
                        ..Default::default()
                    });
                    added = true;
                }
                if added {
                    turn.process_steps = turn.process_steps.saturating_add(1);
                }
            }
        }
        BackendEvent::TurnStatus {
            session_id,
            turn_id,
            state: status,
        } if state.selected_session.as_deref() == Some(session_id.as_str()) => {
            state.busy = matches!(status.as_str(), "running" | "queued");
            if let Some(turn) = state.turns.iter_mut().find(|turn| turn.id == turn_id) {
                turn.assistant.state = status;
            } else {
                state.turns.push(TurnView {
                    id: turn_id.clone(),
                    assistant: MessageView {
                        id: turn_id,
                        mine: false,
                        state: status,
                        ..Default::default()
                    },
                    ..Default::default()
                });
            }
        }
        BackendEvent::Delta {
            session_id,
            turn_id,
            text,
        } if state.selected_session.as_deref() == Some(session_id.as_str()) => {
            state.busy = true;
            if let Some(turn) = state.turns.iter_mut().find(|turn| turn.id == turn_id) {
                turn.assistant.text.push_str(&text);
            } else {
                state.turns.push(TurnView {
                    id: turn_id.clone(),
                    assistant: MessageView {
                        id: turn_id,
                        text,
                        mine: false,
                        state: "running".to_string(),
                    },
                    ..Default::default()
                });
            }
        }
        BackendEvent::Workflow {
            session_id,
            turn_id,
            item,
        } if state.selected_session.as_deref() == Some(session_id.as_str()) => {
            if let Some(turn) = state.turns.iter_mut().find(|turn| turn.id == turn_id) {
                let mut added = false;
                if let Some(current) = turn
                    .workflow
                    .iter_mut()
                    .find(|current| current.id == item.id)
                {
                    *current = item;
                } else if turn.workflow.len() < PROCESS_ITEM_LIMIT {
                    turn.workflow.push(item);
                    added = true;
                }
                if added {
                    turn.process_steps = turn.process_steps.saturating_add(1);
                }
            }
        }
        BackendEvent::Queued { session_id }
            if state.selected_session.as_deref() == Some(session_id.as_str()) =>
        {
            state.busy = true;
            state.status = "已排队，等待运行时接手".to_string();
        }
        BackendEvent::Finished { session_id }
            if state.selected_session.as_deref() == Some(session_id.as_str()) =>
        {
            state.busy = false;
            state.status = "已完成".to_string();
        }
        BackendEvent::Failed {
            session_id,
            message,
        } if session_id.is_none() || state.selected_session.as_deref() == session_id.as_deref() => {
            state.busy = false;
            state.connection = crate::model::ConnectionState::Failed;
            state.status = message;
        }
        BackendEvent::Disconnected(message) => {
            state.connection = crate::model::ConnectionState::Disconnected;
            state.status = message;
        }
        _ => {}
    }
}

/// A history projection has no separate stream identity, so only concrete
/// assistant lifecycle states can affect the composer action.
fn is_active_turn_state(state: &str) -> bool {
    matches!(
        state.trim().to_ascii_lowercase().as_str(),
        "running" | "streaming" | "queued"
    )
}

/// The durable header can confirm a completed, failed, or stopped root. States
/// waiting for user input or approval are deliberately not terminal here.
fn is_terminal_turn_state(state: &str) -> bool {
    matches!(
        state.trim().to_ascii_lowercase().as_str(),
        "completed"
            | "complete"
            | "done"
            | "failed"
            | "rejected"
            | "error"
            | "stopped"
            | "cancelled"
            | "canceled"
            | "interrupted"
            | "任务完成"
            | "执行失败"
            | "已停止"
    )
}
/// Merge a compact history projection without throwing away local process
/// pages that were explicitly opened by the reader. The observer emits only
/// changed roots after its initial snapshot, so a patch must never delete
/// unrelated historical turns.
fn merge_history(current: &mut Vec<TurnView>, incoming: Vec<TurnView>, replace: bool) {
    if replace {
        let mut merged = Vec::with_capacity(incoming.len());
        for mut next in incoming {
            normalize_history_turn_state(&mut next);
            if let Some(previous) = current.iter().find(|turn| turn.id == next.id) {
                retain_process_projection(&mut next, previous);
            }
            merged.push(next);
        }
        *current = merged;
        return;
    }
    for mut next in incoming {
        normalize_history_turn_state(&mut next);
        if let Some(index) = current.iter().position(|turn| turn.id == next.id) {
            let previous = &current[index];
            retain_process_projection(&mut next, previous);
            current[index] = next;
        } else {
            current.push(next);
        }
    }
}

fn normalize_history_turn_state(turn: &mut TurnView) {
    if !turn.assistant.state.trim().is_empty() {
        turn.assistant.state = normalize_turn_state(&turn.assistant.state);
    }
}

fn retain_process_projection(next: &mut TurnView, previous: &TurnView) {
    next.process_steps = next.process_steps.max(previous.process_steps);
    if previous.process_loaded
        || previous.process_loading
        || !previous.process_error.is_empty()
        || !previous.workflow.is_empty()
    {
        next.workflow = previous.workflow.clone();
        next.process_loaded = previous.process_loaded;
        next.process_loading = previous.process_loading;
        next.process_error.clone_from(&previous.process_error);
        next.process_first = previous.process_first;
        next.process_last = previous.process_last;
        next.process_has_older = previous.process_has_older;
        next.process_has_newer = previous.process_has_newer;
    }
}

/// Return the durable keyset bounds represented by a capped UI projection.
/// Several visible rows can share one storage item, so row count must never
/// be used as a cursor. Live-only tail rows intentionally carry no index.
fn workflow_storage_bounds(items: &[WorkflowView]) -> (Option<i64>, Option<i64>) {
    items
        .iter()
        .filter_map(|item| item.storage_index)
        .fold((None, None), |(first, last), index| {
            (
                Some(first.map_or(index, |current: i64| current.min(index))),
                Some(last.map_or(index, |current: i64| current.max(index))),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ConnectionState;

    #[test]
    fn prompt_preview_drops_stale_results_without_changing_chat_state() {
        let mut state = AppState {
            selected_session: Some("thread-current".to_owned()),
            connection: ConnectionState::Ready,
            busy: true,
            prompt_preview: crate::model::PromptPreview {
                open: true,
                loading: true,
                request_id: 2,
                session_id: Some("thread-current".to_owned()),
                ..Default::default()
            },
            ..Default::default()
        };
        for (session_id, request_id) in [("thread-previous", 2), ("thread-current", 1)] {
            apply_event(
                &mut state,
                BackendEvent::SystemPromptPreview {
                    session_id: session_id.to_owned(),
                    request_id,
                    result: Ok(("stale preview".to_owned(), false)),
                },
            );
        }
        assert!(state.prompt_preview.loading);
        assert!(state.prompt_preview.text.is_empty());
        apply_event(
            &mut state,
            BackendEvent::SystemPromptPreview {
                session_id: "thread-current".to_owned(),
                request_id: 2,
                result: Err("preview unavailable".to_owned()),
            },
        );
        assert!(!state.prompt_preview.loading);
        assert_eq!(state.prompt_preview.error, "preview unavailable");
        assert_eq!(state.connection, ConnectionState::Ready);
        assert!(state.busy);
    }

    #[test]
    fn system_prompt_projection_is_bounded_and_utf8_safe() {
        let original = "示例".repeat(24_000);
        let (text, truncated) = bounded_system_prompt(original.clone());
        assert!(truncated);
        assert!(text.len() <= 128 * 1024);
        assert!(original.starts_with(&text));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tool_projection_keeps_arguments_and_bounded_results_separate() {
        let payload = serde_json::json!({
            "args": {"path": "notes/example.txt"},
            "result": {"ok": true, "data": {"path": "notes/other.txt", "content": "甲".repeat(20000)}}
        });
        let entry = wunder_desktop::NativeWorkflowEntry::from_payload(
            "call",
            "read_file",
            String::new(),
            "completed",
            &payload,
        );
        let projected = native_workflow_view(entry);
        assert_eq!(projected.kind, WorkflowKind::Tool);
        assert_eq!(projected.targets, vec!["notes/example.txt"]);
        assert_eq!(invocation_targets(&payload), vec!["notes/example.txt"]);
        assert!(projected.detail.len() <= WORKFLOW_DETAIL_BYTES);
        assert!(projected.detail_truncated);
        assert_ne!(projected.detail, projected.preview);
        let (lines, clipped) = bounded_detail(&"line\n".repeat(200));
        assert!(lines.lines().count() <= WORKFLOW_DETAIL_LINES);
        assert!(clipped);
    }

    #[test]
    fn reducer_ignores_events_for_another_thread() {
        let mut state = AppState {
            selected_session: Some("selected".into()),
            ..Default::default()
        };
        apply_event(
            &mut state,
            BackendEvent::Delta {
                session_id: "other".into(),
                turn_id: "turn".into(),
                text: "late".into(),
            },
        );
        assert!(state.turns.is_empty());
        assert!(!state.busy);
    }

    #[test]
    fn workspace_listing_event_keeps_its_requesting_thread() {
        let mut state = AppState {
            selected_session: Some("selected".into()),
            workspace_request_id: 7,
            workspace_files: vec![WorkspaceFileView {
                path: "current.md".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        apply_event(
            &mut state,
            BackendEvent::WorkspaceFiles {
                session_id: "other".into(),
                path: String::new(),
                request_id: 7,
                files: Ok(vec![WorkspaceFileView {
                    path: "stale.md".into(),
                    ..Default::default()
                }]),
            },
        );
        assert_eq!(state.workspace_files[0].path, "current.md");
    }

    #[test]
    fn late_directory_listing_cannot_replace_the_current_one() {
        // Two refreshes raced: the older page arrives last. Only the newest
        // request id may update the projection, and the newer directory must
        // stay selected.
        let mut state = AppState {
            selected_session: Some("selected".into()),
            ..Default::default()
        };
        let first = state.begin_workspace_refresh();
        let second = state.begin_workspace_refresh();
        state.workspace_file_path = "notes".to_owned();

        apply_event(
            &mut state,
            BackendEvent::WorkspaceFiles {
                session_id: "selected".into(),
                path: String::new(),
                request_id: first,
                files: Ok(vec![WorkspaceFileView {
                    path: "root.md".into(),
                    ..Default::default()
                }]),
            },
        );
        assert!(state.workspace_files.is_empty(), "过期页不应写入文件列表");
        assert_eq!(state.workspace_file_path, "notes");
        assert!(state.workspace_file_loading, "仍在等待最新一页");

        apply_event(
            &mut state,
            BackendEvent::WorkspaceFiles {
                session_id: "selected".into(),
                path: "notes".into(),
                request_id: second,
                files: Ok(vec![WorkspaceFileView {
                    path: "notes/plan.md".into(),
                    ..Default::default()
                }]),
            },
        );
        assert_eq!(state.workspace_files[0].path, "notes/plan.md");
        assert_eq!(state.workspace_file_path, "notes");
        assert!(!state.workspace_file_loading);
    }

    #[test]
    fn history_patch_keeps_busy_while_the_same_turn_is_streaming() {
        let mut state = AppState {
            selected_session: Some("selected".into()),
            busy: true,
            turns: vec![TurnView {
                id: "turn".into(),
                assistant: MessageView {
                    state: "running".into(),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        apply_event(
            &mut state,
            BackendEvent::History {
                session_id: "selected".into(),
                turns: vec![TurnView {
                    id: "turn".into(),
                    assistant: MessageView {
                        text: "已提交的流式片段".into(),
                        state: "running".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                replace: false,
            },
        );
        assert!(state.busy);
        assert_eq!(state.turns[0].assistant.state, "running");
        assert_eq!(state.turns[0].assistant.text, "已提交的流式片段");
    }

    #[test]
    fn terminal_history_patch_settles_its_active_turn() {
        let mut state = AppState {
            selected_session: Some("selected".into()),
            busy: true,
            turns: vec![TurnView {
                id: "turn".into(),
                assistant: MessageView {
                    state: "running".into(),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        apply_event(
            &mut state,
            BackendEvent::History {
                session_id: "selected".into(),
                turns: vec![TurnView {
                    id: "turn".into(),
                    assistant: MessageView {
                        state: "任务完成".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                replace: false,
            },
        );
        assert!(!state.busy);
        assert_eq!(state.turns[0].assistant.state, "completed");
    }
    #[test]
    fn history_patch_retains_open_process_page() {
        let mut state = AppState {
            selected_session: Some("selected".into()),
            turns: vec![TurnView {
                id: "turn".into(),
                workflow: vec![WorkflowView {
                    id: "loaded-tool".into(),
                    title: "工具".into(),
                    ..Default::default()
                }],
                process_loaded: true,
                process_first: Some(101),
                process_last: Some(200),
                ..Default::default()
            }],
            ..Default::default()
        };
        apply_event(
            &mut state,
            BackendEvent::History {
                session_id: "selected".into(),
                turns: vec![TurnView {
                    id: "turn".into(),
                    assistant: MessageView {
                        text: "durable answer".into(),
                        state: "running".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                replace: false,
            },
        );
        let turn = &state.turns[0];
        assert_eq!(turn.assistant.text, "durable answer");
        assert!(turn.process_loaded);
        assert_eq!(turn.workflow[0].id, "loaded-tool");
        assert_eq!(turn.process_last, Some(200));
    }

    #[test]
    fn reasoning_delta_stays_bounded() {
        let mut state = AppState {
            selected_session: Some("selected".into()),
            turns: vec![TurnView {
                id: "turn".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        apply_event(
            &mut state,
            BackendEvent::ReasoningDelta {
                session_id: "selected".into(),
                turn_id: "turn".into(),
                item_id: "thought".into(),
                text: "甲".repeat(PROCESS_DETAIL_BYTES),
                replace: false,
            },
        );
        let item = &state.turns[0].workflow[0];
        assert!(item.detail.len() <= PROCESS_DETAIL_BYTES);
        assert!(item.detail_truncated);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn streamed_reasoning_identity_matches_durable_execution_item() {
        assert_eq!(
            reasoning_item_id("continuation-execution", 3),
            "reason-continuation-execution:text-3"
        );
    }

    #[test]
    fn workflow_storage_bounds_ignore_live_rows_and_keep_item_cursors() {
        let rows = vec![
            WorkflowView {
                id: "live".to_owned(),
                ..Default::default()
            },
            WorkflowView {
                id: "reason-11".to_owned(),
                storage_index: Some(11),
                ..Default::default()
            },
            WorkflowView {
                id: "tool-12".to_owned(),
                storage_index: Some(12),
                ..Default::default()
            },
            WorkflowView {
                id: "text-12".to_owned(),
                storage_index: Some(12),
                ..Default::default()
            },
        ];
        assert_eq!(workflow_storage_bounds(&rows), (Some(11), Some(12)));
    }

    #[test]
    fn filtered_process_pages_advance_the_older_cursor() {
        let mut state = AppState {
            selected_session: Some("selected".into()),
            turns: vec![TurnView {
                id: "turn".into(),
                // A visible tail was already loaded. The following pages
                // contain storage items that `process_page_view` legitimately
                // filters, so their raw cursor is the only progress signal.
                workflow: vec![WorkflowView {
                    id: "visible-tail".into(),
                    storage_index: Some(401),
                    ..Default::default()
                }],
                process_loaded: true,
                process_first: Some(401),
                process_last: Some(401),
                process_has_older: true,
                ..Default::default()
            }],
            ..Default::default()
        };

        for (before, first, last, has_older) in [
            (401, 301, 400, true),
            (301, 201, 300, true),
            (201, 101, 200, false),
        ] {
            apply_event(
                &mut state,
                BackendEvent::ProcessPage {
                    session_id: "selected".into(),
                    turn_id: "turn".into(),
                    before: Some(before),
                    after: None,
                    result: Ok(ProcessPageView {
                        // The raw page is non-empty in durable storage, but
                        // no record is presentable in the work-process view.
                        items: Vec::new(),
                        first: Some(first),
                        last: Some(last),
                        has_older,
                        has_newer: true,
                    }),
                },
            );
            let turn = &state.turns[0];
            assert_eq!(turn.process_first, Some(first));
            assert_eq!(turn.process_last, Some(401));
            assert_eq!(turn.process_has_older, has_older);
        }
    }

    #[test]
    fn reducer_merges_stream_delta_and_finishes() {
        let mut state = AppState {
            selected_session: Some("selected".into()),
            connection: ConnectionState::Ready,
            ..Default::default()
        };
        for text in ["甲", "乙", "丙"] {
            apply_event(
                &mut state,
                BackendEvent::Delta {
                    session_id: "selected".into(),
                    turn_id: "turn".into(),
                    text: text.into(),
                },
            );
        }
        apply_event(
            &mut state,
            BackendEvent::Finished {
                session_id: "selected".into(),
            },
        );
        assert_eq!(state.turns[0].assistant.text, "甲乙丙");
        assert!(!state.busy);
        assert_eq!(state.status, "已完成");
    }

    #[test]
    fn model_save_updates_matching_thread_without_changing_chat_health() {
        let mut state = AppState {
            sessions: vec![SessionView {
                id: "selected".into(),
                model_key: Some("previous".into()),
                ..Default::default()
            }],
            selected_session: Some("selected".into()),
            connection: ConnectionState::Ready,
            busy: true,
            model_picker_saving: true,
            ..Default::default()
        };
        apply_event(
            &mut state,
            BackendEvent::SessionModelSaved {
                session_id: "selected".into(),
                model_key: "next".into(),
                result: Ok(()),
            },
        );
        assert_eq!(state.sessions[0].model_key.as_deref(), Some("next"));
        assert!(!state.model_picker_saving);
        assert_eq!(state.connection, ConnectionState::Ready);
        assert!(state.busy);

        apply_event(
            &mut state,
            BackendEvent::SessionModelSaved {
                session_id: "selected".into(),
                model_key: "rejected".into(),
                result: Err("model unavailable".into()),
            },
        );
        assert_eq!(state.sessions[0].model_key.as_deref(), Some("next"));
        assert_eq!(state.model_picker_error, "model unavailable");
        assert_eq!(state.connection, ConnectionState::Ready);
        assert!(state.busy);
    }

    #[test]
    fn runtime_context_projection_is_thread_scoped() {
        let mut state = AppState {
            sessions: vec![
                SessionView {
                    id: "selected".into(),
                    ..Default::default()
                },
                SessionView {
                    id: "other".into(),
                    ..Default::default()
                },
            ],
            selected_session: Some("selected".into()),
            connection: ConnectionState::Ready,
            busy: true,
            ..Default::default()
        };
        apply_event(
            &mut state,
            BackendEvent::ContextUsage {
                session_id: "other".into(),
                used_tokens: Some(512),
                capacity_tokens: Some(4096),
            },
        );
        assert_eq!(state.sessions[0].context_tokens, None);
        assert_eq!(state.sessions[1].context_tokens, Some(512));
        assert_eq!(state.sessions[1].context_capacity_tokens, Some(4096));
        assert_eq!(state.connection, ConnectionState::Ready);
        assert!(state.busy);
    }

    #[test]
    fn reasoning_effort_values_are_explicit() {
        assert_eq!(normalize_reasoning_effort(" HIGH "), Some("high".into()));
        assert_eq!(normalize_reasoning_effort("unsupported"), None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn model_type_filter_accepts_only_llm_entries() {
        assert!(is_llm_model("llm"));
        assert!(is_llm_model("LLM"));
        assert!(is_llm_model(""));
        assert!(!is_llm_model("asr"));
    }
}
