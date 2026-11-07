//! Engine bridge: owns the in-process `NativeDesktop` and turns its durable
//! thread updates plus the live send stream into the flat models the phone UI
//! renders. All engine calls and markdown projection stay off the UI thread;
//! the UI only copies the latest projection on its 33 ms tick.

use crate::project::{self, LiveTurn, RowData, STATUS_FAILED, STATUS_STOPPED};
use crate::{MobileSession, ModelCard};
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
    ReasoningEffort(String),
    RefreshSessions,
    LoadSettings,
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
    pub models_rev: u64,
    pub settings_rev: u64,
    pub editor_rev: u64,
    pub probe_rev: u64,
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
    pub models: Vec<ModelCard>,
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
    let desktop = Arc::new(NativeDesktop::start()?);
    // Detect a previously installed Ubuntu rootfs before the first settings
    // card render. The environment itself is optional; a missing one must not
    // hold up the engine.
    #[cfg(target_os = "android")]
    crate::ubuntu::bind();
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
            Command::Send(text) => {
                let session = shared.lock().unwrap().session_id.clone();
                if session.is_empty() {
                    notify(&shared, "会话尚未就绪");
                    continue;
                }
                start_stream(desktop.clone(), shared.clone(), session, text);
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
            Command::ReasoningEffort(value) => {
                let session = shared.lock().unwrap().session_id.clone();
                if let Err(error) = desktop.save_session_reasoning_effort(&session, &value) {
                    notify(&shared, format!("保存推理强度失败：{error}"));
                }
            }
            Command::RefreshSessions | Command::LoadSettings => {
                let active = shared.lock().unwrap().session_id.clone();
                refresh_sessions(&desktop, &shared, Some(&active));
                refresh_ubuntu(&desktop, &shared);
            }
            Command::RefreshUbuntu => refresh_ubuntu(&desktop, &shared),
            Command::InstallUbuntu => {
                if crate::ubuntu::snapshot().map(|s| s.installing()).unwrap_or(false) {
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
            Command::SaveLanguage(language) => {
                match desktop.get_desktop_settings() {
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
                }
            }
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
                    .and_then(|state| state.editor.as_ref().map(|editor| editor.original_key.clone()))
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
                        state.editor.as_ref().map(|editor| editor.original_key.clone())
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
                probe_model(
                    desktop.clone(),
                    shared.clone(),
                    card,
                    secret,
                    original_key,
                );
            }
            Command::DeleteModel(key) => match desktop.delete_model(&key) {
                Ok(_) => {
                    if let Ok(mut state) = shared.lock() {
                        if state.editor.as_ref().map(|editor| editor.original_key.as_str())
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
    {
        let mut state = shared.lock().unwrap();
        state.session_id = snapshot.session.id.clone();
        state.title = if snapshot.session.title.trim().is_empty() {
            "新会话".into()
        } else {
            snapshot.session.title.clone()
        };
        state.workspace = snapshot.session.workspace_name.clone();
        state.subtitle = [snapshot.session.runtime_status.as_str(), snapshot.session.workspace_name.as_str()]
            .iter()
            .filter(|part| !part.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" · ");
        state.model_key = snapshot.session.session_model_name.clone().unwrap_or_default();
        state.model_name = state.model_key.clone();
        state.durable_rows = project_turns(&initial_turns);
        state.durable_turns = initial_turns.clone();
        state.live_rows.clear();
        state.live_root.clear();
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

fn project_turns(turns: &[wunder_desktop::NativeChatTurn]) -> Vec<RowData> {
    let mut rows = Vec::new();
    for turn in turns {
        rows.extend(project::project_turn(turn));
    }
    // `payload` reaches the fold callback, and the UI routes it back into the
    // flat durable row list. It must be the absolute row index, not the index
    // inside the turn: a per-turn index folds the wrong row (or nothing) from
    // the second turn onward.
    for (index, row) in rows.iter_mut().enumerate() {
        row.payload = index as i32;
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
    if opens.is_empty() {
        return rows;
    }
    for row in &mut rows {
        if let Some(open) = opens.get(row.id.as_str()) {
            row.open = *open;
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
        let need_detail = candidates.iter().any(|root| {
            state
                .durable_turns
                .iter()
                .find(|turn| &turn.root_id == root)
                .is_some_and(|turn| turn.rounds.is_empty())
        });
        let rows = project_turns_preserving(&state.durable_turns, &state.durable_rows);
        state.durable_rows = rows;
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
        let rows = project_turns_preserving(&state.durable_turns, &state.durable_rows);
        state.durable_rows = rows;
        state.rows_rev = state.rows_rev.wrapping_add(1);
        state.stream_revision = state.stream_revision.wrapping_add(1);
        state.touch();
    }
}

fn refresh_sessions(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    active: Option<&str>,
) {
    let Ok(list) = desktop.list_sessions(None) else {
        return;
    };
    let settings = desktop.get_desktop_settings().ok();
    let sessions: Vec<MobileSession> = list
        .iter()
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
        .collect();
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

fn open_model_editor(
    desktop: &NativeDesktop,
    shared: &Arc<Mutex<Shared>>,
    key: Option<&str>,
) {
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
                Ok(outcome) => (outcome.message, outcome.max_context.map(|value| value.to_string())),
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
        max_context: "128000".into(),
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
) {
    let input = NativeChatInput {
        session_id: session.clone(),
        content: text.clone(),
        client_message_id: None,
        reasoning_effort: None,
        inquiry_turn_id: None,
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
                        let rows =
                            project_turns_preserving(&state.durable_turns, &state.durable_rows);
                        state.durable_rows = rows;
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
    let rows = live.rows();
    if let Ok(mut state) = shared.lock() {
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
    let event = value
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let data = value.get("data").unwrap_or(value);
    let payload = data.get("data").unwrap_or(data);
    match event {
        "native_turn_identity" => {
            if let Some(root) = payload
                .get("root_turn_id")
                .and_then(Value::as_str)
                .filter(|root| !root.is_empty())
            {
                live.root_id = root.to_owned();
            }
        }
        "native_execution_started" | "native_execution_status" => {
            if let Some(status) = payload.get("status").and_then(Value::as_str) {
                live.status = project::status_kind(status);
            }
        }
        "reasoning_delta" => {
            if let Some(delta) = payload
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| payload.get("reasoning_delta").and_then(Value::as_str))
            {
                live.reasoning.push_str(delta);
            }
        }
        "llm_output" | "llm_output_delta" => {
            if let Some(reasoning) = payload
                .get("reasoning_delta")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                live.reasoning.push_str(reasoning);
            } else if let Some(reasoning) = payload
                .get("reasoning")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                live.reasoning.push_str(reasoning);
            }
            let text = payload
                .get("content")
                .and_then(Value::as_str)
                .or_else(|| payload.get("delta").and_then(Value::as_str))
                .unwrap_or_default();
            if !text.is_empty() {
                live.answer.push_str(text);
            }
        }
        "tool_call" | "tool_start" => {
            let id = payload
                .get("tool_call_id")
                .or_else(|| payload.get("item_id"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_owned();
            let title = payload
                .get("tool")
                .or_else(|| payload.get("tool_name"))
                .and_then(Value::as_str)
                .unwrap_or("工具")
                .to_owned();
            let state = payload
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("running")
                .to_owned();
            if let Some(existing) = live.tools.iter_mut().find(|tool| tool.id == id) {
                existing.title = title;
                existing.state = state;
            } else {
                live.tools.push(project::LiveTool {
                    id,
                    title,
                    state,
                    detail: String::new(),
                    target: String::new(),
                });
            }
        }
        "tool_result" | "tool_output" => {
            let id = payload
                .get("tool_call_id")
                .or_else(|| payload.get("item_id"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let detail = payload
                .get("display_result")
                .and_then(Value::as_str)
                .or_else(|| payload.get("output").and_then(Value::as_str))
                .or_else(|| payload.get("result").and_then(Value::as_str))
                .unwrap_or_default();
            let state = if payload.get("ok").and_then(Value::as_bool) == Some(false) {
                "failed"
            } else {
                "completed"
            };
            if let Some(existing) = live.tools.iter_mut().find(|tool| tool.id == id) {
                existing.state = state.to_owned();
                if !detail.is_empty() {
                    existing.detail = detail.to_owned();
                }
            } else {
                let entry = NativeWorkflowEntry::from_payload(
                    id,
                    payload
                        .get("tool")
                        .and_then(Value::as_str)
                        .unwrap_or("工具"),
                    detail.to_owned(),
                    state,
                    payload,
                );
                live.tools.push(project::LiveTool {
                    id: entry.id,
                    title: entry.title,
                    state: entry.state,
                    detail: entry.detail,
                    target: String::new(),
                });
            }
        }
        "final" => {
            if let Some(answer) = payload
                .get("answer")
                .and_then(Value::as_str)
                .filter(|answer| !answer.is_empty())
            {
                live.answer = answer.to_owned();
            }
        }
        "turn_terminal" => {
            let status = payload
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            live.status = match status {
                "cancelled" | "canceled" | "stopped" | "interrupted" => STATUS_STOPPED,
                "failed" | "error" | "rejected" => STATUS_FAILED,
                _ => project::STATUS_DONE,
            };
            if status == "failed" {
                if let Some(message) = payload
                    .get("message")
                    .or_else(|| payload.get("error"))
                    .and_then(Value::as_str)
                {
                    live.failure = Some(message.to_owned());
                }
            }
        }
        "error" => {
            if let Some(message) = payload
                .get("message")
                .or_else(|| payload.get("error_message"))
                .and_then(Value::as_str)
            {
                live.failure = Some(message.to_owned());
            }
        }
        _ => {}
    }
}
